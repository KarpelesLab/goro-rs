use goro_core::array::PhpArray;
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

/// Create a JsonException with the proper error code set
fn create_json_exception(vm: &mut Vm, class: &[u8], message: &str, code: i64) -> Value {
    let exc = vm.create_exception(class, message, 0);
    if let Value::Object(ref obj) = exc {
        obj.borrow_mut().set_property(b"code".to_vec(), Value::Long(code));
    }
    exc
}

/// Register all JSON extension functions
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"json");
    vm.register_function(b"json_encode", json_encode);
    vm.register_function(b"json_decode", json_decode);
    vm.register_function(b"json_last_error", json_last_error);
    vm.register_function(b"json_last_error_msg", json_last_error_msg);
    vm.register_function(b"json_validate", json_validate);
}

// JSON encode option flags
const JSON_HEX_TAG: i64 = 1;
const JSON_HEX_AMP: i64 = 2;
const JSON_HEX_APOS: i64 = 4;
const JSON_HEX_QUOT: i64 = 8;
const JSON_FORCE_OBJECT: i64 = 16;
const JSON_NUMERIC_CHECK: i64 = 32;
const JSON_UNESCAPED_SLASHES: i64 = 64;
const JSON_PRETTY_PRINT: i64 = 128;
const JSON_UNESCAPED_UNICODE: i64 = 256;
const JSON_PARTIAL_OUTPUT_ON_ERROR: i64 = 512;
const JSON_PRESERVE_ZERO_FRACTION: i64 = 1024;
const JSON_UNESCAPED_LINE_TERMINATORS: i64 = 2048;
const JSON_THROW_ON_ERROR: i64 = 4194304;
const JSON_OBJECT_AS_ARRAY: i64 = 1; // for json_decode

fn json_encode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // For lazy objects, trigger initialization and redirect proxy to real
    // before encoding. This ensures json_encode sees the realized state.
    let original = args.first().cloned().unwrap_or(Value::Null);
    let val_owned: Value = if let Value::Object(obj) = original.deref() {
        let is_lazy = matches!(
            obj.borrow().get_property(b"__lazy_state"),
            Value::String(ref s) if s.as_bytes() == b"ghost" || s.as_bytes() == b"proxy"
        );
        if is_lazy {
            vm.maybe_run_lazy_init(&obj);
            if vm.current_exception.is_some() {
                return Err(VmError { message: "Uncaught exception".to_string(), line: 0 });
            }
            Value::Object(vm.proxy_redirect(&obj))
        } else {
            original.clone()
        }
    } else {
        original.clone()
    };
    // Cross-call recursion guard for JsonSerializable: when called from within
    // jsonSerialize() for the same object, return false (matches PHP).
    if let Value::Object(obj) = &val_owned {
        let ptr = Rc::as_ptr(obj) as usize;
        let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
        let has_json_serialize = if let Some(class) = vm.classes.get(&class_lower) {
            class.interfaces.iter().any(|i| i.eq_ignore_ascii_case(b"JsonSerializable"))
                || class.get_method(b"jsonserialize").is_some()
        } else {
            false
        };
        if has_json_serialize && vm.json_encoding_objects.contains(&ptr) {
            // Recursion detected
            vm.json_last_error = 6;
            return Ok(Value::False);
        }
    }
    let val = &val_owned;
    let flags = match args.get(1) {
        Some(v) => v.to_long(),
        None => 0,
    };
    let max_depth: usize = match args.get(2) {
        Some(Value::Long(n)) if *n <= 0 => {
            // depth <= 0: return false immediately
            vm.json_last_error = 1; // JSON_ERROR_DEPTH
            return Ok(Value::False);
        }
        Some(Value::Long(n)) => *n as usize,
        None => 512,
        _ => 512,
    };
    // Clamp max_depth to a value that's safe for the host Rust stack. The
    // encoder recurses once per nested array/object, so accepting the PHP
    // userland "default" of 2^31 means a long linked list overflows the OS
    // stack instead of returning a clean JSON_ERROR_DEPTH. PHP itself uses
    // zend.max_allowed_stack_size for this; we approximate with a fixed cap.
    let max_depth = max_depth.min(1024);
    let throw_on_error = flags & JSON_THROW_ON_ERROR != 0;

    // Check for NAN/INF at top level
    if let Value::Double(f) = val {
        if f.is_nan() || f.is_infinite() {
            if throw_on_error {
                let exc = create_json_exception(vm, b"JsonException", "Inf and NaN cannot be JSON encoded", 7);
                vm.current_exception = Some(exc);
                return Err(VmError { message: "Uncaught JsonException: Inf and NaN cannot be JSON encoded".to_string(), line: 0 });
            }
            vm.json_last_error = 7; // JSON_ERROR_INF_OR_NAN
            // JSON_PARTIAL_OUTPUT_ON_ERROR flag
            if flags & JSON_PARTIAL_OUTPUT_ON_ERROR != 0 {
                return Ok(Value::String(PhpString::from_bytes(b"0")));
            }
            return Ok(Value::False);
        }
    }

    // When JSON_PARTIAL_OUTPUT_ON_ERROR is set, it overrides JSON_THROW_ON_ERROR
    let partial_output = flags & JSON_PARTIAL_OUTPUT_ON_ERROR != 0;
    let effective_throw = throw_on_error && !partial_output;

    if !effective_throw {
        vm.json_last_error = 0;
    }
    // Capture original obj pointer if input is a JsonSerializable object so we
    // can detect cyclic references during encoding (Bug #61978).
    let orig_obj_ptr: Option<usize> = if let Value::Object(obj) = val {
        let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
        let has_json_serialize = if let Some(class) = vm.classes.get(&class_lower) {
            class.interfaces.iter().any(|i| i.eq_ignore_ascii_case(b"JsonSerializable"))
                || class.get_method(b"jsonserialize").is_some()
        } else {
            false
        };
        if has_json_serialize { Some(Rc::as_ptr(obj) as usize) } else { None }
    } else { None };
    // Pre-process value: call jsonSerialize() on JsonSerializable objects
    let val_owned2 = resolve_json_serializable(vm, val);
    if vm.current_exception.is_some() {
        // jsonSerialize threw — propagate.
        return Err(VmError { message: "Uncaught exception in jsonSerialize".to_string(), line: 0 });
    }
    let val = &val_owned2;
    // If jsonSerialize did NOT return the same object (e.g. returned an array
    // containing the original obj), pre-populate the encoder's seen set with
    // the original obj_ptr so cyclic references are detected. When it returns
    // $this, fall through to normal encoding without pre-pop.
    let mut seen: std::collections::HashSet<usize> = std::collections::HashSet::new();
    if let Some(orig_ptr) = orig_obj_ptr {
        let resolved_same = matches!(val, Value::Object(o) if Rc::as_ptr(o) as usize == orig_ptr);
        if !resolved_same {
            seen.insert(orig_ptr);
        }
    }
    let mut recursion_detected = false;
    let s = json_encode_value_flags_tracked(val, 0, flags, max_depth, &mut seen, &mut recursion_detected);
    if s.contains("\x00ENUM_ERROR") {
        // Non-backed enum cannot be serialized
        let err_msg = "Non-backed enums have no default serialization";
        if !effective_throw {
            vm.json_last_error = 11; // JSON_ERROR_NON_BACKED_ENUM
        }
        if effective_throw {
            let exc = create_json_exception(vm, b"JsonException", err_msg, 11);
            vm.current_exception = Some(exc);
            return Err(VmError { message: format!("Uncaught JsonException: {}", err_msg), line: 0 });
        }
        return Ok(Value::False);
    }
    if s == "\x00DEPTH_ERROR" {
        if !effective_throw {
            vm.json_last_error = 1; // JSON_ERROR_DEPTH
        }
        if effective_throw {
            let exc = create_json_exception(vm, b"JsonException", "Maximum stack depth exceeded", 1);
            vm.current_exception = Some(exc);
            return Err(VmError { message: "Uncaught JsonException: Maximum stack depth exceeded".to_string(), line: 0 });
        }
        if partial_output {
            return Ok(Value::String(PhpString::from_bytes(b"false")));
        }
        return Ok(Value::False);
    }
    if recursion_detected {
        if !effective_throw {
            vm.json_last_error = 6; // JSON_ERROR_RECURSION
        }
        if partial_output {
            return Ok(Value::String(PhpString::from_string(s)));
        }
        return Ok(Value::False);
    }
    if s.contains("\x00UTF8_ERROR") {
        if !effective_throw {
            vm.json_last_error = 5; // JSON_ERROR_UTF8
        }
        if partial_output {
            // PARTIAL_OUTPUT_ON_ERROR overrides THROW_ON_ERROR
            return Ok(Value::String(PhpString::from_bytes(b"null")));
        }
        if effective_throw {
            let exc = create_json_exception(vm, b"JsonException", "Malformed UTF-8 characters, possibly incorrectly encoded", 5);
            vm.current_exception = Some(exc);
            return Err(VmError { message: "Uncaught JsonException: Malformed UTF-8 characters, possibly incorrectly encoded".to_string(), line: 0 });
        }
        return Ok(Value::False);
    }
    if s.contains("\x00RESOURCE_ERROR") {
        // Resource type cannot be JSON encoded
        if !effective_throw {
            vm.json_last_error = 8; // JSON_ERROR_UNSUPPORTED_TYPE
        }
        if partial_output {
            return Ok(Value::String(PhpString::from_bytes(b"null")));
        }
        if effective_throw {
            let exc = create_json_exception(vm, b"JsonException", "Type is not supported", 8);
            vm.current_exception = Some(exc);
            return Err(VmError { message: "Uncaught JsonException: Type is not supported".to_string(), line: 0 });
        }
        return Ok(Value::False);
    }
    Ok(Value::String(PhpString::from_string(s)))
}

/// Recursively resolve JsonSerializable objects by calling their jsonSerialize() method
fn resolve_json_serializable(vm: &mut Vm, val: &Value) -> Value {
    let mut seen = std::collections::HashSet::new();
    resolve_json_serializable_depth(vm, val, 0, &mut seen)
}

fn resolve_json_serializable_depth(
    vm: &mut Vm,
    val: &Value,
    depth: usize,
    seen: &mut std::collections::HashSet<usize>,
) -> Value {
    if depth > 64 {
        return val.clone(); // Prevent infinite recursion
    }
    match val {
        Value::Object(obj) => {
            let obj_ptr = Rc::as_ptr(obj) as usize;
            let obj_ref = obj.borrow();
            // Skip enum cases
            if obj_ref.has_property(b"__enum_case") {
                return val.clone();
            }
            let class_lower: Vec<u8> = obj_ref.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            drop(obj_ref);
            // Check if the class implements JsonSerializable
            let has_json_serialize = {
                if let Some(class) = vm.classes.get(&class_lower) {
                    class.interfaces.iter().any(|i| i.eq_ignore_ascii_case(b"JsonSerializable"))
                        || class.get_method(b"jsonserialize").is_some()
                } else {
                    false
                }
            };
            // Also check built-in SPL classes that implement JsonSerializable
            let is_builtin_json = matches!(class_lower.as_slice(),
                b"splfixedarray" | b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator"
            );
            if has_json_serialize || is_builtin_json {
                // Recursion guard: if we're already resolving this object via
                // jsonSerialize higher up the stack, fall through to plain
                // object encoding (this matches PHP's behaviour when
                // jsonSerialize returns $this — it doesn't keep invoking the
                // method, just encodes public properties).
                if seen.insert(obj_ptr) {
                    // Mark this object as being encoded so that recursive
                    // json_encode($this) calls from within jsonSerialize fail.
                    let inserted_global = vm.json_encoding_objects.insert(obj_ptr);
                    // Try call_object_method which handles both user and built-in SPL dispatch
                    let call_result = vm.call_object_method(val, b"jsonSerialize", &[]);
                    if vm.current_exception.is_some() {
                        seen.remove(&obj_ptr);
                        if inserted_global {
                            vm.json_encoding_objects.remove(&obj_ptr);
                        }
                        return val.clone();
                    }
                    if let Some(result) = call_result {
                        let r = resolve_json_serializable_depth(vm, &result, depth + 1, seen);
                        seen.remove(&obj_ptr);
                        if inserted_global {
                            vm.json_encoding_objects.remove(&obj_ptr);
                        }
                        return r;
                    }
                    if has_json_serialize {
                        // Fallback: try user-defined method directly
                        let method = {
                            let class = vm.classes.get(&class_lower);
                            class.and_then(|c| c.get_method(b"jsonserialize").cloned())
                        };
                        if let Some(method) = method {
                            let op = method.op_array.clone();
                            let mut fn_cvs = vec![Value::Undef; op.cv_names.len()];
                            if !fn_cvs.is_empty() {
                                fn_cvs[0] = val.clone(); // $this
                            }
                            if let Ok(result) = vm.execute_fn(&op, fn_cvs) {
                                let r = resolve_json_serializable_depth(vm, &result, depth + 1, seen);
                                seen.remove(&obj_ptr);
                                if inserted_global {
                                    vm.json_encoding_objects.remove(&obj_ptr);
                                }
                                return r;
                            }
                        }
                    }
                    seen.remove(&obj_ptr);
                    if inserted_global {
                        vm.json_encoding_objects.remove(&obj_ptr);
                    }
                }
                // Already in `seen`: fall through to plain encoding below
            }
            // Objects with hooked properties: public properties in declaration
            // order, get hooks are called, virtual properties without get skipped.
            if vm.class_has_hooked_props(&class_lower) {
                let visible = {
                    let ob = obj.borrow();
                    let mut a = PhpArray::new();
                    for (k, v) in ob.properties.iter() {
                        if k.starts_with(b"__") || k.first() == Some(&0) || matches!(v, Value::Undef) {
                            continue;
                        }
                        a.set(goro_core::array::ArrayKey::String(PhpString::from_vec(k.clone())), v.clone());
                    }
                    a
                };
                let props = match vm.hooked_object_props_array(obj, &visible, true) {
                    Ok(p) => p,
                    Err(_) => return val.clone(),
                };
                let new_obj_id = vm.next_object_id();
                let mut new_obj = goro_core::object::PhpObject::new(b"stdClass".to_vec(), new_obj_id);
                for (k, v) in props.iter() {
                    let name = match k {
                        goro_core::array::ArrayKey::String(s) => s.as_bytes().to_vec(),
                        goro_core::array::ArrayKey::Int(n) => n.to_string().into_bytes(),
                    };
                    let resolved = resolve_json_serializable_depth(vm, v, depth + 1, seen);
                    new_obj.set_property(name, resolved);
                }
                return Value::Object(Rc::new(RefCell::new(new_obj)));
            }
            // No JsonSerializable: collect private/protected property names so they can be filtered.
            // We build a HashSet and store it on the cloned object via an internal __json_nonpublic prop.
            // Cleaner: filter the cloned object in place by removing private/protected properties.
            let nonpublic_names: std::collections::HashSet<Vec<u8>> = {
                if let Some(class) = vm.classes.get(&class_lower) {
                    class.properties.iter()
                        .filter(|p| !p.is_static && p.visibility != goro_core::object::Visibility::Public)
                        .map(|p| p.name.clone())
                        .collect()
                } else {
                    std::collections::HashSet::new()
                }
            };
            if nonpublic_names.is_empty() {
                // No filtering needed. Keep original Rc identity so that
                // recursion detection works downstream. Nested objects with
                // jsonSerialize will be processed when we recurse into them
                // during encoding.
                return val.clone();
            }
            // Build a filtered clone
            let obj_ref = obj.borrow();
            let new_obj_id = vm.next_object_id();
            let mut new_obj = goro_core::object::PhpObject::new(obj_ref.class_name.clone(), new_obj_id);
            let props: Vec<_> = obj_ref.properties.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            drop(obj_ref);
            for (name, value) in props {
                if nonpublic_names.contains(&name) {
                    continue;
                }
                let resolved = resolve_json_serializable_depth(vm, &value, depth + 1, seen);
                new_obj.set_property(name, resolved);
            }
            Value::Object(Rc::new(RefCell::new(new_obj)))
        }
        Value::Array(arr) => {
            // Check if any values in the array contain objects/refs that may
            // need to be resolved.
            let arr_ref = arr.borrow();
            let has_nested = arr_ref.iter().any(|(_, v)| {
                matches!(v, Value::Object(_) | Value::Array(_) | Value::Reference(_))
            });
            if !has_nested {
                return val.clone();
            }
            let items: Vec<_> = arr_ref.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            drop(arr_ref);
            let mut new_arr = PhpArray::new();
            for (key, value) in items {
                let resolved = resolve_json_serializable_depth(vm, &value, depth + 1, seen);
                if vm.current_exception.is_some() {
                    // Stop processing - exception will propagate to top of json_encode.
                    return val.clone();
                }
                new_arr.set(key, resolved);
            }
            Value::Array(Rc::new(RefCell::new(new_arr)))
        }
        Value::Reference(r) => resolve_json_serializable_depth(vm, &r.borrow(), depth, seen),
        _ => val.clone(),
    }
}

#[allow(dead_code)]
fn json_encode_value_flags(val: &Value, depth: usize, flags: i64) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut recursion_detected = false;
    json_encode_value_flags_tracked(val, depth, flags, 512, &mut seen, &mut recursion_detected)
}

fn json_encode_value_flags_tracked(val: &Value, depth: usize, flags: i64, max_depth: usize, seen: &mut std::collections::HashSet<usize>, recursion_detected: &mut bool) -> String {
    // Check depth limit - depth starts at 0 for top-level
    // For arrays/objects, we check depth+1 against max_depth before recursing
    if depth >= max_depth {
        // Only trigger depth error for compound types
        match val {
            Value::Array(_) | Value::Object(_) => {
                return "\x00DEPTH_ERROR".to_string();
            }
            _ => {}
        }
    }
    let indent = if flags & JSON_PRETTY_PRINT != 0 {
        "    ".repeat(depth)
    } else {
        String::new()
    };
    let inner_indent = if flags & JSON_PRETTY_PRINT != 0 {
        "    ".repeat(depth + 1)
    } else {
        String::new()
    };
    let nl = if flags & JSON_PRETTY_PRINT != 0 { "\n" } else { "" };
    let sep = if flags & JSON_PRETTY_PRINT != 0 { ": " } else { ":" };

    match val {
        Value::Null | Value::Undef => "null".to_string(),
        Value::True => "true".to_string(),
        Value::False => "false".to_string(),
        Value::Long(n) => n.to_string(),
        Value::Resource(_) => "\x00RESOURCE_ERROR".to_string(),
        Value::Double(f) => {
            if f.is_infinite() || f.is_nan() {
                // PHP json_encode emits "0" for inf/nan in nested contexts;
                // the top-level check intercepts these for return-false handling.
                "0".to_string()
            } else {
                // PHP uses serialize_precision for json_encode
                let sp = goro_core::value::get_php_serialize_precision();
                let s = if sp < 0 {
                    // Shortest roundtrip: use scientific notation for very
                    // small/large values, decimal otherwise (matches
                    // format_php_float_shortest behaviour).
                    let abs = f.abs();
                    if abs != 0.0 && (abs >= 1e15 || abs < 1e-4) {
                        // Scientific notation: lowercase 'e', no '+' for
                        // positive exponents (PHP convention for json).
                        let mut best = format!("{:e}", f);
                        for prec in 0..20 {
                            let candidate = format!("{:.prec$e}", f, prec = prec);
                            if let Ok(parsed) = candidate.parse::<f64>() {
                                if parsed == *f {
                                    best = candidate;
                                    break;
                                }
                            }
                        }
                        // Convert "1.23456789e-13" → "1.23456789e-13" (already correct)
                        // But Rust may emit "1e-13" without dot; PHP keeps no extra dots.
                        // Also format the exponent with explicit + sign for positives
                        // (PHP uses "1.23e+34" for 1.23e34, not "1.23e34").
                        if let Some(pos) = best.find('e') {
                            let mantissa = &best[..pos];
                            let exp_str: i32 = best[pos+1..].parse().unwrap_or(0);
                            let exp_fmt = if exp_str >= 0 {
                                format!("e+{}", exp_str)
                            } else {
                                format!("e{}", exp_str)
                            };
                            format!("{}{}", mantissa, exp_fmt)
                        } else {
                            best
                        }
                    } else {
                        let mut best = format!("{}", f);
                        for prec in 0..20 {
                            let candidate = format!("{:.prec$}", f, prec = prec);
                            if let Ok(parsed) = candidate.parse::<f64>() {
                                if parsed == *f {
                                    best = candidate;
                                    break;
                                }
                            }
                        }
                        best
                    }
                } else {
                    goro_core::value::format_php_float_with_precision_pub(*f, sp as usize)
                };
                // For json_encode, integer-valued floats are output without .0
                // e.g., 1230.0 -> "1230", not "1230.0"
                // Unless JSON_PRESERVE_ZERO_FRACTION is set
                if flags & JSON_PRESERVE_ZERO_FRACTION != 0 {
                    if !s.contains('.') && !s.contains('e') && !s.contains('E') {
                        format!("{}.0", s)
                    } else {
                        s
                    }
                } else {
                    s
                }
            }
        }
        Value::String(s) => {
            // JSON_NUMERIC_CHECK: encode numeric strings as numbers
            if flags & JSON_NUMERIC_CHECK != 0 {
                let sv = s.to_string_lossy();
                let trimmed = sv.trim();
                if !trimmed.is_empty() {
                    if let Ok(n) = trimmed.parse::<i64>() {
                        return n.to_string();
                    }
                    if let Ok(f) = trimmed.parse::<f64>() {
                        if !f.is_infinite() && !f.is_nan() {
                            let formatted = format!("{}", f);
                            return formatted;
                        }
                    }
                }
            }
            json_encode_string(s.as_bytes(), flags)
        }
        Value::Array(arr) => {
            // Recursion detection for arrays
            let arr_ptr = Rc::as_ptr(arr) as usize;
            if !seen.insert(arr_ptr) {
                *recursion_detected = true;
                seen.remove(&arr_ptr);
                return "null".to_string();
            }
            let arr_ref = arr.borrow();
            let force_object = flags & JSON_FORCE_OBJECT != 0;
            let is_list = !force_object && arr_ref.iter().enumerate().all(
                |(i, (k, _))| matches!(k, goro_core::array::ArrayKey::Int(n) if *n == i as i64),
            );
            if arr_ref.is_empty() {
                seen.remove(&arr_ptr);
                if is_list {
                    return "[]".to_string();
                } else {
                    return "{}".to_string();
                }
            }
            let partial_output = flags & JSON_PARTIAL_OUTPUT_ON_ERROR != 0;
            let result = if is_list {
                let parts: Vec<String> = arr_ref.values().map(|v| {
                    let encoded = json_encode_value_flags_tracked(v, depth + 1, flags, max_depth, seen, recursion_detected);
                    if encoded == "\x00DEPTH_ERROR" {
                        seen.remove(&arr_ptr);
                        return "\x00DEPTH_ERROR".to_string();
                    }
                    if partial_output && (encoded.contains("\x00UTF8_ERROR") || encoded.contains("\x00RESOURCE_ERROR")) {
                        "null".to_string()
                    } else {
                        encoded
                    }
                }).collect();
                if parts.iter().any(|p| p == "\x00DEPTH_ERROR") {
                    seen.remove(&arr_ptr);
                    return "\x00DEPTH_ERROR".to_string();
                }
                if flags & JSON_PRETTY_PRINT != 0 {
                    format!("[{nl}{}{nl}{}]", parts.iter().map(|p| format!("{}{}", inner_indent, p)).collect::<Vec<_>>().join(&format!(",{nl}")), indent)
                } else {
                    format!("[{}]", parts.join(","))
                }
            } else {
                let parts: Vec<String> = arr_ref
                    .iter()
                    .map(|(k, v)| {
                        let key_str = match k {
                            goro_core::array::ArrayKey::Int(n) => format!("\"{}\"", n),
                            goro_core::array::ArrayKey::String(s) => {
                                let encoded_key = json_encode_string(s.as_bytes(), flags);
                                if partial_output && encoded_key.contains("\x00UTF8_ERROR") {
                                    "\"\"".to_string()
                                } else {
                                    encoded_key
                                }
                            }
                        };
                        let encoded_val = json_encode_value_flags_tracked(v, depth + 1, flags, max_depth, seen, recursion_detected);
                        if encoded_val == "\x00DEPTH_ERROR" {
                            return "\x00DEPTH_ERROR".to_string();
                        }
                        let encoded_val = if partial_output && (encoded_val.contains("\x00UTF8_ERROR") || encoded_val.contains("\x00RESOURCE_ERROR")) {
                            "null".to_string()
                        } else {
                            encoded_val
                        };
                        format!("{}{}{}", key_str, sep, encoded_val)
                    })
                    .collect();
                if parts.iter().any(|p| p == "\x00DEPTH_ERROR") {
                    seen.remove(&arr_ptr);
                    return "\x00DEPTH_ERROR".to_string();
                }
                if flags & JSON_PRETTY_PRINT != 0 {
                    format!("{{{nl}{}{nl}{}}}", parts.iter().map(|p| format!("{}{}", inner_indent, p)).collect::<Vec<_>>().join(&format!(",{nl}")), indent)
                } else {
                    format!("{{{}}}", parts.join(","))
                }
            };
            seen.remove(&arr_ptr);
            result
        }
        Value::Object(obj) => {
            // Recursion detection for objects
            let obj_ptr = Rc::as_ptr(obj) as usize;
            if !seen.insert(obj_ptr) {
                *recursion_detected = true;
                seen.remove(&obj_ptr);
                return "null".to_string();
            }
            let obj_ref = obj.borrow();
            // Special handling for enum cases
            if obj_ref.has_property(b"__enum_case") {
                seen.remove(&obj_ptr);
                // Check if this is a backed enum
                if obj_ref.has_property(b"__enum_backing_type") {
                    // Backed enum: encode the backing value
                    let value = obj_ref.get_property(b"value");
                    return json_encode_value_flags_tracked(&value, depth, flags, max_depth, seen, recursion_detected);
                } else {
                    // Non-backed enum: cannot be serialized
                    // Return empty string to signal error
                    return "\x00ENUM_ERROR".to_string();
                }
            }
            // Filter out internal properties
            let visible_props: Vec<_> = obj_ref.properties.iter()
                .filter(|(name, val)| {
                    !name.starts_with(b"__spl_") && !name.starts_with(b"__reflection_")
                    && !name.starts_with(b"__timestamp") && !name.starts_with(b"__enum_")
                    && !name.starts_with(b"__fiber_") && !name.starts_with(b"__ctor_")
                    && !name.starts_with(b"__lazy_") && !name.starts_with(b"__uri_")
                    && !matches!(val, Value::Undef)
                })
                .collect();
            if visible_props.is_empty() {
                seen.remove(&obj_ptr);
                return "{}".to_string();
            }
            let parts: Vec<String> = visible_props.iter().map(|(name, val)| {
                let key = json_encode_string(name, flags);
                let encoded_val = json_encode_value_flags_tracked(val, depth + 1, flags, max_depth, seen, recursion_detected);
                if encoded_val == "\x00DEPTH_ERROR" {
                    return "\x00DEPTH_ERROR".to_string();
                }
                format!("{}{}{}", key, sep, encoded_val)
            }).collect();
            if parts.iter().any(|p| p == "\x00DEPTH_ERROR") {
                seen.remove(&obj_ptr);
                return "\x00DEPTH_ERROR".to_string();
            }
            let result = if flags & JSON_PRETTY_PRINT != 0 {
                format!("{{{nl}{}{nl}{}}}", parts.iter().map(|p| format!("{}{}", inner_indent, p)).collect::<Vec<_>>().join(&format!(",{nl}")), indent)
            } else {
                format!("{{{}}}", parts.join(","))
            };
            seen.remove(&obj_ptr);
            result
        }
        Value::Generator(_) => "null".to_string(),
        Value::Reference(r) => json_encode_value_flags_tracked(&r.borrow(), depth, flags, max_depth, seen, recursion_detected),
    }
}

fn json_encode_string(bytes: &[u8], flags: i64) -> String {
    let mut result = String::from("\"");
    let unescaped_unicode = flags & JSON_UNESCAPED_UNICODE != 0;
    let unescaped_slashes = flags & JSON_UNESCAPED_SLASHES != 0;
    let hex_tag = flags & JSON_HEX_TAG != 0;
    let hex_amp = flags & JSON_HEX_AMP != 0;
    let hex_apos = flags & JSON_HEX_APOS != 0;
    let hex_quot = flags & JSON_HEX_QUOT != 0;

    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'"' => {
                if hex_quot {
                    result.push_str("\\u0022");
                } else {
                    result.push_str("\\\"");
                }
            }
            b'\\' => result.push_str("\\\\"),
            b'/' => {
                if unescaped_slashes {
                    result.push('/');
                } else {
                    result.push_str("\\/");
                }
            }
            b'\n' => result.push_str("\\n"),
            b'\r' => result.push_str("\\r"),
            b'\t' => result.push_str("\\t"),
            0x08 => result.push_str("\\b"),
            0x0C => result.push_str("\\f"),
            b'<' if hex_tag => result.push_str("\\u003C"),
            b'>' if hex_tag => result.push_str("\\u003E"),
            b'&' if hex_amp => result.push_str("\\u0026"),
            b'\'' if hex_apos => result.push_str("\\u0027"),
            b if b < 0x20 => result.push_str(&format!("\\u{:04x}", b)),
            b if b < 0x80 => result.push(b as char),
            _ => {
                // Multi-byte UTF-8 sequence
                let remaining = &bytes[i..];
                let (codepoint, len) = decode_utf8_char(remaining);
                if let Some(cp) = codepoint {
                    let unescaped_line_terminators = flags & JSON_UNESCAPED_LINE_TERMINATORS != 0;
                    if unescaped_unicode {
                        // Even with UNESCAPED_UNICODE, U+2028 and U+2029 must be escaped
                        // unless JSON_UNESCAPED_LINE_TERMINATORS is also set
                        if !unescaped_line_terminators && (cp == 0x2028 || cp == 0x2029) {
                            result.push_str(&format!("\\u{:04x}", cp));
                        } else if let Some(c) = char::from_u32(cp) {
                            result.push(c);
                        } else {
                            for j in 0..len {
                                result.push(bytes[i + j] as char);
                            }
                        }
                    } else {
                        // Escape as \uXXXX (or surrogate pair for > U+FFFF)
                        if cp <= 0xFFFF {
                            result.push_str(&format!("\\u{:04x}", cp));
                        } else {
                            // UTF-16 surrogate pair
                            let cp = cp - 0x10000;
                            let high = 0xD800 + (cp >> 10);
                            let low = 0xDC00 + (cp & 0x3FF);
                            result.push_str(&format!("\\u{:04x}\\u{:04x}", high, low));
                        }
                    }
                    i += len;
                    continue;
                } else {
                    // Invalid UTF-8: skip the expected sequence length so one
                    // bad start byte + continuation bytes is treated as a single
                    // invalid sequence (per PHP behaviour).
                    let max_len = utf8_seq_len(bytes[i]);
                    let mut skip = 1usize;
                    while skip < max_len && i + skip < bytes.len() {
                        let nb = bytes[i + skip];
                        if nb & 0xC0 == 0x80 {
                            skip += 1;
                        } else {
                            break;
                        }
                    }
                    if flags & JSON_INVALID_UTF8_IGNORE != 0 {
                        i += skip;
                        continue;
                    } else if flags & JSON_INVALID_UTF8_SUBSTITUTE != 0 {
                        if flags & JSON_UNESCAPED_UNICODE != 0 {
                            result.push('\u{FFFD}');
                        } else {
                            result.push_str("\\ufffd");
                        }
                        i += skip;
                        continue;
                    } else {
                        return "\x00UTF8_ERROR".to_string();
                    }
                }
            }
        }
        i += 1;
    }
    result.push('"');
    result
}

/// How many bytes a UTF-8 start byte would consume (1..=4), assuming a valid sequence.
/// Returns 1 for invalid start bytes or continuation bytes.
fn utf8_seq_len(b0: u8) -> usize {
    if b0 < 0x80 { 1 }
    else if b0 & 0xE0 == 0xC0 { 2 }
    else if b0 & 0xF0 == 0xE0 { 3 }
    else if b0 & 0xF8 == 0xF0 { 4 }
    else { 1 }
}

fn decode_utf8_char(bytes: &[u8]) -> (Option<u32>, usize) {
    if bytes.is_empty() {
        return (None, 0);
    }
    let b0 = bytes[0];
    if b0 < 0x80 {
        (Some(b0 as u32), 1)
    } else if b0 & 0xE0 == 0xC0 {
        if bytes.len() < 2 || bytes[1] & 0xC0 != 0x80 {
            return (None, 1);
        }
        let cp = ((b0 as u32 & 0x1F) << 6) | (bytes[1] as u32 & 0x3F);
        (Some(cp), 2)
    } else if b0 & 0xF0 == 0xE0 {
        if bytes.len() < 3 || bytes[1] & 0xC0 != 0x80 || bytes[2] & 0xC0 != 0x80 {
            return (None, 1);
        }
        let cp = ((b0 as u32 & 0x0F) << 12) | ((bytes[1] as u32 & 0x3F) << 6) | (bytes[2] as u32 & 0x3F);
        (Some(cp), 3)
    } else if b0 & 0xF8 == 0xF0 {
        if bytes.len() < 4 || bytes[1] & 0xC0 != 0x80 || bytes[2] & 0xC0 != 0x80 || bytes[3] & 0xC0 != 0x80 {
            return (None, 1);
        }
        let cp = ((b0 as u32 & 0x07) << 18) | ((bytes[1] as u32 & 0x3F) << 12) | ((bytes[2] as u32 & 0x3F) << 6) | (bytes[3] as u32 & 0x3F);
        (Some(cp), 4)
    } else {
        (None, 1)
    }
}

fn json_decode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Emit deprecation warning for null argument
    let is_null_arg = matches!(args.first(), Some(Value::Null) | Some(Value::Undef) | None);
    if is_null_arg {
        vm.emit_deprecated_at("json_decode(): Passing null to parameter #1 ($json) of type string is deprecated", vm.current_line);
    }
    let json_str = match args.first() {
        Some(v) => v.to_php_string(),
        None => return Ok(Value::Null),
    };
    let json_bytes = json_str.as_bytes();

    // $associative parameter: null (default) means objects become stdClass, true means arrays
    let mut associative = match args.get(1) {
        Some(Value::True) => true,
        Some(Value::Null) | Some(Value::Undef) | None => false,
        Some(Value::False) => false,
        Some(v) => v.is_truthy(),
    };

    // $depth parameter (arg 2)
    let max_depth: usize = match args.get(2) {
        Some(Value::Long(n)) if *n > 0 && *n <= 0x7FFFFFFF => *n as usize,
        Some(Value::Long(n)) if *n > 0x7FFFFFFF => {
            // Too large depth - ValueError
            let exc = vm.create_exception(b"ValueError", &format!("json_decode(): Argument #3 ($depth) must be less than {}", 0x7FFFFFFF_i64), 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: "Uncaught ValueError".to_string(), line: 0 });
        }
        Some(Value::Long(n)) if *n <= 0 => {
            let exc = vm.create_exception(b"ValueError", "json_decode(): Argument #3 ($depth) must be greater than 0", 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: "Uncaught ValueError".to_string(), line: 0 });
        }
        Some(Value::Long(_)) => return Ok(Value::Null),
        None => 512,
        _ => 512,
    };

    // $flags parameter (arg 3) - parse relevant flags
    let flags: i64 = match args.get(3) {
        Some(v) => v.to_long(),
        None => 0,
    };
    let bigint_as_string = (flags & 2) != 0; // JSON_BIGINT_AS_STRING = 2
    let throw_on_error = (flags & JSON_THROW_ON_ERROR) != 0;
    let invalid_utf8_ignore = (flags & JSON_INVALID_UTF8_IGNORE) != 0;
    let invalid_utf8_substitute = (flags & JSON_INVALID_UTF8_SUBSTITUTE) != 0;

    // JSON_OBJECT_AS_ARRAY flag
    if (flags & JSON_OBJECT_AS_ARRAY) != 0 {
        associative = true;
    }

    // Pre-process bytes for JSON_INVALID_UTF8_* flags: scrub invalid UTF-8 in
    // string literals (between quotes) per PHP semantics.
    let processed_owned: Vec<u8>;
    let json_bytes: &[u8] = if invalid_utf8_ignore || invalid_utf8_substitute {
        processed_owned = scrub_invalid_utf8_in_strings(json_bytes, invalid_utf8_substitute);
        &processed_owned
    } else {
        json_bytes
    };

    // Pre-validate UTF-8 of the input (skip if JSON_INVALID_UTF8_* flags are set)
    let _skip_utf8_check = invalid_utf8_ignore || invalid_utf8_substitute;
    if !_skip_utf8_check {
    if let Some(utf8_err) = check_json_utf8(json_bytes) {
        match utf8_err {
            JsonUtf8Error::InvalidUtf8 => {
                if throw_on_error {
                    let exc = create_json_exception(vm, b"JsonException", "Malformed UTF-8 characters, possibly incorrectly encoded", 5);
                    vm.current_exception = Some(exc);
                    return Err(VmError { message: "Uncaught JsonException".to_string(), line: 0 });
                }
                vm.json_last_error = 5; // JSON_ERROR_UTF8
                return Ok(Value::Null);
            }
            JsonUtf8Error::ControlChar => {
                if throw_on_error {
                    let exc = create_json_exception(vm, b"JsonException", "Control character error, possibly incorrectly encoded", 3);
                    vm.current_exception = Some(exc);
                    return Err(VmError { message: "Uncaught JsonException".to_string(), line: 0 });
                }
                vm.json_last_error = 3; // JSON_ERROR_CTRL_CHAR
                return Ok(Value::Null);
            }
        }
    }
    }

    let mut parser = JsonParser {
        input: json_bytes,
        pos: 0,
        depth: 0,
        max_depth,
        associative,
        bigint_as_string,
        error_code: 0,
        vm,
    };

    match parser.parse_value() {
        Some(val) => {
            parser.skip_whitespace();
            let error_code = parser.error_code;
            if error_code != 0 {
                // Parser encountered an error (e.g., null byte in property name)
                let error_msg = json_error_msg(error_code);
                if throw_on_error {
                    let exc = create_json_exception(vm, b"JsonException", error_msg, error_code);
                    vm.current_exception = Some(exc);
                    return Err(VmError { message: format!("Uncaught JsonException: {}", error_msg), line: 0 });
                }
                vm.json_last_error = error_code;
                return Ok(Value::Null);
            }
            if parser.pos < parser.input.len() {
                // Trailing data after valid JSON
                if throw_on_error {
                    let exc = create_json_exception(vm, b"JsonException", "Syntax error", 4);
                    vm.current_exception = Some(exc);
                    return Err(VmError { message: "Uncaught JsonException: Syntax error".to_string(), line: 0 });
                }
                vm.json_last_error = 4; // JSON_ERROR_SYNTAX
                Ok(Value::Null)
            } else {
                if !throw_on_error {
                    vm.json_last_error = 0;
                }
                Ok(val)
            }
        }
        None => {
            // Determine error type
            let error_code;
            let error_msg;
            let parser_error = parser.error_code;
            if parser_error != 0 {
                error_code = parser_error;
                error_msg = json_error_msg(error_code);
            } else if json_bytes.is_empty() {
                error_code = 4;
                error_msg = "Syntax error";
            } else if parser.depth >= parser.max_depth {
                error_code = 1; // JSON_ERROR_DEPTH
                error_msg = "Maximum stack depth exceeded";
            } else {
                error_code = 4;
                error_msg = "Syntax error";
            }
            if throw_on_error {
                let exc = create_json_exception(vm, b"JsonException", error_msg, error_code);
                vm.current_exception = Some(exc);
                return Err(VmError { message: format!("Uncaught JsonException: {}", error_msg), line: 0 });
            }
            vm.json_last_error = error_code;
            Ok(Value::Null)
        }
    }
}

fn json_error_msg(code: i64) -> &'static str {
    match code {
        0 => "No error",
        1 => "Maximum stack depth exceeded",
        2 => "State mismatch (invalid or malformed JSON)",
        3 => "Control character error, possibly incorrectly encoded",
        4 => "Syntax error",
        5 => "Malformed UTF-8 characters, possibly incorrectly encoded",
        6 => "Recursion detected",
        7 => "Inf and NaN cannot be JSON encoded",
        8 => "Type is not supported",
        9 => "The decoded property name is invalid",
        10 => "Single unpaired UTF-16 surrogate in unicode escape",
        11 => "Non-backed enums have no default serialization",
        _ => "Unknown error",
    }
}

enum JsonUtf8Error {
    InvalidUtf8,
    ControlChar,
}

/// Scrub invalid UTF-8 byte sequences in JSON string literals.
/// If `substitute` is true, replace each invalid byte with U+FFFD (0xEF 0xBF 0xBD).
/// Otherwise (ignore mode), drop the invalid bytes.
/// Bytes outside string literals are left untouched.
fn scrub_invalid_utf8_in_strings(input: &[u8], substitute: bool) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(input.len());
    let mut i = 0;
    let mut in_string = false;
    while i < input.len() {
        let b = input[i];
        if !in_string {
            if b == b'"' {
                in_string = true;
            }
            out.push(b);
            i += 1;
            continue;
        }
        // In string
        if b == b'\\' {
            // Escape sequence, copy two bytes (backslash + next byte) as-is
            out.push(b);
            i += 1;
            if i < input.len() {
                out.push(input[i]);
                i += 1;
            }
            continue;
        }
        if b == b'"' {
            in_string = false;
            out.push(b);
            i += 1;
            continue;
        }
        if b < 0x80 {
            out.push(b);
            i += 1;
            continue;
        }
        // High bit set — try to decode UTF-8
        let (cp, len) = decode_utf8_char(&input[i..]);
        match cp {
            Some(cp_val) if !(0xD800..=0xDFFF).contains(&cp_val) => {
                // Valid UTF-8, copy all bytes
                for j in 0..len {
                    if i + j < input.len() {
                        out.push(input[i + j]);
                    }
                }
                i += len;
            }
            _ => {
                // Invalid byte
                if substitute {
                    out.extend_from_slice(&[0xEF, 0xBF, 0xBD]);
                }
                i += 1;
            }
        }
    }
    out
}

/// Check raw JSON bytes for invalid UTF-8 or bare control characters (outside string escapes).
/// Returns Some(error) if there's a problem.
fn check_json_utf8(bytes: &[u8]) -> Option<JsonUtf8Error> {
    let mut i = 0;
    let mut in_string = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if b == b'\\' {
                // Skip escaped character
                i += 2;
                continue;
            }
            if b == b'"' {
                in_string = false;
                i += 1;
                continue;
            }
            // Check for bare control characters inside strings
            if b < 0x20 {
                return Some(JsonUtf8Error::ControlChar);
            }
            // Check for valid UTF-8 inside strings
            if b >= 0x80 {
                let (cp, len) = decode_utf8_char(&bytes[i..]);
                if cp.is_none() {
                    return Some(JsonUtf8Error::InvalidUtf8);
                }
                // Check for surrogate codepoints (U+D800-U+DFFF) - invalid in UTF-8
                if let Some(cp_val) = cp {
                    if (0xD800..=0xDFFF).contains(&cp_val) {
                        return Some(JsonUtf8Error::InvalidUtf8);
                    }
                }
                i += len;
                continue;
            }
            i += 1;
            continue;
        }
        // Not in string
        if b == b'"' {
            in_string = true;
            i += 1;
            continue;
        }
        // Outside strings, check for bare control chars (but tabs/newlines/spaces are whitespace)
        if b < 0x20 && b != b'\t' && b != b'\n' && b != b'\r' {
            return Some(JsonUtf8Error::ControlChar);
        }
        // Check for invalid UTF-8 outside strings
        if b >= 0x80 {
            let (cp, len) = decode_utf8_char(&bytes[i..]);
            if cp.is_none() {
                return Some(JsonUtf8Error::InvalidUtf8);
            }
            if let Some(cp_val) = cp {
                if (0xD800..=0xDFFF).contains(&cp_val) {
                    return Some(JsonUtf8Error::InvalidUtf8);
                }
            }
            i += len;
            continue;
        }
        i += 1;
    }
    None
}

/// Hand-rolled JSON parser that converts JSON to PHP values.
struct JsonParser<'a, 'b> {
    input: &'a [u8],
    pos: usize,
    depth: usize,
    max_depth: usize,
    associative: bool,
    bigint_as_string: bool,
    error_code: i64,
    vm: &'b mut Vm,
}

impl<'a, 'b> JsonParser<'a, 'b> {
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let ch = self.input.get(self.pos).copied();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() {
            match self.input[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn parse_value(&mut self) -> Option<Value> {
        self.skip_whitespace();
        let ch = self.peek()?;
        match ch {
            b'"' => self
                .parse_string()
                .map(|s| Value::String(PhpString::from_vec(s))),
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            b't' => self.parse_literal(b"true", Value::True),
            b'f' => self.parse_literal(b"false", Value::False),
            b'n' => self.parse_literal(b"null", Value::Null),
            b'-' | b'0'..=b'9' => self.parse_number(),
            _ => None,
        }
    }

    fn parse_literal(&mut self, expected: &[u8], value: Value) -> Option<Value> {
        if self.input[self.pos..].starts_with(expected) {
            self.pos += expected.len();
            Some(value)
        } else {
            None
        }
    }

    fn parse_string(&mut self) -> Option<Vec<u8>> {
        // Skip opening quote
        if self.advance()? != b'"' {
            return None;
        }

        let mut result = Vec::new();
        loop {
            let ch = self.advance()?;
            match ch {
                b'"' => return Some(result),
                b'\\' => {
                    let esc = self.advance()?;
                    match esc {
                        b'"' => result.push(b'"'),
                        b'\\' => result.push(b'\\'),
                        b'/' => result.push(b'/'),
                        b'n' => result.push(b'\n'),
                        b'r' => result.push(b'\r'),
                        b't' => result.push(b'\t'),
                        b'b' => result.push(0x08),
                        b'f' => result.push(0x0C),
                        b'u' => {
                            let cp = self.parse_hex4()?;
                            // Handle UTF-16 surrogate pairs
                            if (0xD800..=0xDBFF).contains(&cp) {
                                // High surrogate, expect \uXXXX low surrogate
                                let saved_pos = self.pos;
                                let has_low = self.pos + 1 < self.input.len()
                                    && self.input[self.pos] == b'\\'
                                    && self.input[self.pos + 1] == b'u';
                                if has_low {
                                    self.pos += 2; // skip \u
                                    let low = self.parse_hex4()?;
                                    if !(0xDC00..=0xDFFF).contains(&low) {
                                        // High surrogate followed by non-low-surrogate
                                        self.error_code = 10; // JSON_ERROR_UTF16
                                        return None;
                                    }
                                    let codepoint =
                                        0x10000 + ((cp as u32 - 0xD800) << 10) + (low as u32 - 0xDC00);
                                    if let Some(c) = char::from_u32(codepoint) {
                                        let mut buf = [0u8; 4];
                                        let s = c.encode_utf8(&mut buf);
                                        result.extend_from_slice(s.as_bytes());
                                    } else {
                                        self.error_code = 10;
                                        return None;
                                    }
                                } else {
                                    // Lone high surrogate
                                    self.pos = saved_pos;
                                    self.error_code = 10; // JSON_ERROR_UTF16
                                    return None;
                                }
                            } else if (0xDC00..=0xDFFF).contains(&cp) {
                                // Lone low surrogate is invalid
                                self.error_code = 10; // JSON_ERROR_UTF16
                                return None;
                            } else {
                                {
                                    let c = char::from_u32(cp as u32)?;
                                    let mut buf = [0u8; 4];
                                    let s = c.encode_utf8(&mut buf);
                                    result.extend_from_slice(s.as_bytes());
                                }
                            }
                        }
                        _ => return None, // Invalid escape sequence
                    }
                }
                // Control characters (0x00-0x1F) are invalid unescaped in JSON
                b if b < 0x20 => return None,
                _ => result.push(ch),
            }
        }
    }

    fn parse_hex4(&mut self) -> Option<u16> {
        let mut val: u16 = 0;
        for _ in 0..4 {
            let ch = self.advance()?;
            let digit = match ch {
                b'0'..=b'9' => ch - b'0',
                b'a'..=b'f' => ch - b'a' + 10,
                b'A'..=b'F' => ch - b'A' + 10,
                _ => return None,
            };
            val = val * 16 + digit as u16;
        }
        Some(val)
    }

    fn parse_number(&mut self) -> Option<Value> {
        let start = self.pos;

        // Optional minus
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }

        // Integer part
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                // After leading 0, next char must not be a digit (no leading zeros)
            }
            Some(b'1'..=b'9') => {
                self.pos += 1;
                while let Some(b'0'..=b'9') = self.peek() {
                    self.pos += 1;
                }
            }
            _ => return None,
        }

        let mut is_float = false;

        // Fractional part
        if self.peek() == Some(b'.') {
            is_float = true;
            self.pos += 1;
            // Must have at least one digit after decimal point
            match self.peek() {
                Some(b'0'..=b'9') => {}
                _ => return None,
            }
            while let Some(b'0'..=b'9') = self.peek() {
                self.pos += 1;
            }
        }

        // Exponent part
        if let Some(b'e') | Some(b'E') = self.peek() {
            is_float = true;
            self.pos += 1;
            // Optional sign
            if let Some(b'+') | Some(b'-') = self.peek() {
                self.pos += 1;
            }
            // Must have at least one digit
            match self.peek() {
                Some(b'0'..=b'9') => {}
                _ => return None,
            }
            while let Some(b'0'..=b'9') = self.peek() {
                self.pos += 1;
            }
        }

        let num_str = std::str::from_utf8(&self.input[start..self.pos]).ok()?;

        if is_float {
            let f: f64 = num_str.parse().ok()?;
            Some(Value::Double(f))
        } else {
            // Try to parse as i64 first, fall back to f64 for very large numbers
            if let Ok(n) = num_str.parse::<i64>() {
                Some(Value::Long(n))
            } else if self.bigint_as_string {
                Some(Value::String(PhpString::from_string(num_str.to_string())))
            } else {
                // Number too large for i64, parse as float
                let f: f64 = num_str.parse().ok()?;
                Some(Value::Double(f))
            }
        }
    }

    fn parse_array(&mut self) -> Option<Value> {
        // Skip opening bracket
        if self.advance()? != b'[' {
            return None;
        }

        self.depth += 1;
        if self.depth >= self.max_depth {
            return None;
        }

        let mut arr = PhpArray::new();

        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.pos += 1;
            self.depth -= 1;
            return Some(Value::Array(Rc::new(RefCell::new(arr))));
        }

        loop {
            let val = self.parse_value()?;
            arr.push(val);

            self.skip_whitespace();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b']') => {
                    self.pos += 1;
                    self.depth -= 1;
                    return Some(Value::Array(Rc::new(RefCell::new(arr))));
                }
                Some(_) => {
                    // State mismatch — expected ',' or ']' but found something else
                    self.error_code = 2;
                    return None;
                }
                None => {
                    // EOF mid-array — syntax error
                    self.error_code = 4;
                    return None;
                }
            }
        }
    }

    fn parse_object(&mut self) -> Option<Value> {
        // Skip opening brace
        if self.advance()? != b'{' {
            return None;
        }

        self.depth += 1;
        if self.depth >= self.max_depth {
            return None;
        }

        self.skip_whitespace();

        if self.associative {
            // Decode as associative array
            let mut arr = PhpArray::new();

            if self.peek() == Some(b'}') {
                self.pos += 1;
                self.depth -= 1;
                return Some(Value::Array(Rc::new(RefCell::new(arr))));
            }

            loop {
                self.skip_whitespace();
                let key = self.parse_string()?;

                self.skip_whitespace();
                if self.advance()? != b':' {
                    return None;
                }

                let val = self.parse_value()?;
                arr.set(
                    goro_core::array::ArrayKey::String(PhpString::from_vec(key)),
                    val,
                );

                self.skip_whitespace();
                match self.peek() {
                    Some(b',') => {
                        self.pos += 1;
                    }
                    Some(b'}') => {
                        self.pos += 1;
                        self.depth -= 1;
                        return Some(Value::Array(Rc::new(RefCell::new(arr))));
                    }
                    Some(_) => {
                        self.error_code = 2;
                        return None;
                    }
                    None => {
                        self.error_code = 4;
                        return None;
                    }
                }
            }
        } else {
            // Decode as stdClass object
            let obj_id = self.vm.next_object_id();
            let mut obj = PhpObject::new(b"stdClass".to_vec(), obj_id);

            if self.peek() == Some(b'}') {
                self.pos += 1;
                self.depth -= 1;
                return Some(Value::Object(Rc::new(RefCell::new(obj))));
            }

            loop {
                self.skip_whitespace();
                let key = self.parse_string()?;

                // Check for null bytes in property names (invalid for stdClass)
                if key.contains(&0) {
                    self.error_code = 9; // JSON_ERROR_INVALID_PROPERTY_NAME
                    return None;
                }

                self.skip_whitespace();
                if self.advance()? != b':' {
                    return None;
                }

                let val = self.parse_value()?;
                obj.set_property(key, val);

                self.skip_whitespace();
                match self.peek() {
                    Some(b',') => {
                        self.pos += 1;
                    }
                    Some(b'}') => {
                        self.pos += 1;
                        self.depth -= 1;
                        return Some(Value::Object(Rc::new(RefCell::new(obj))));
                    }
                    Some(_) => {
                        self.error_code = 2;
                        return None;
                    }
                    None => {
                        self.error_code = 4;
                        return None;
                    }
                }
            }
        }
    }
}
fn json_last_error(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if !args.is_empty() {
        let msg = format!("json_last_error() expects exactly 0 arguments, {} given", args.len());
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    Ok(Value::Long(vm.json_last_error))
}
fn json_last_error_msg(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if !args.is_empty() {
        let msg = format!("json_last_error_msg() expects exactly 0 arguments, {} given", args.len());
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let msg = json_error_msg(vm.json_last_error);
    Ok(Value::String(PhpString::from_bytes(msg.as_bytes())))
}

const JSON_INVALID_UTF8_IGNORE: i64 = 1048576;
const JSON_INVALID_UTF8_SUBSTITUTE: i64 = 2097152;

fn json_validate(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let json_str = match args.first() {
        Some(v) => v.to_php_string(),
        None => return Ok(Value::False),
    };
    let json_bytes = json_str.as_bytes();

    let max_depth: usize = match args.get(1) {
        Some(Value::Long(n)) if *n > 0 && *n <= 0x7FFFFFFF => *n as usize,
        Some(Value::Long(n)) if *n > 0x7FFFFFFF => {
            let exc = vm.create_exception(b"ValueError", &format!("json_validate(): Argument #2 ($depth) must be less than {}", 0x7FFFFFFF_i64), 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: "Uncaught ValueError".to_string(), line: 0 });
        }
        Some(Value::Long(n)) if *n == 0 => {
            // depth == 0 is a ValueError
            let exc = vm.create_exception(b"ValueError", "json_validate(): Argument #2 ($depth) must be greater than 0", 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: "Uncaught ValueError".to_string(), line: 0 });
        }
        Some(Value::Long(n)) if *n < 0 => {
            // Negative depth: return false with syntax error
            vm.json_last_error = 4;
            return Ok(Value::False);
        }
        Some(Value::Long(_)) => {
            vm.json_last_error = 4;
            return Ok(Value::False);
        }
        None => 512,
        _ => 512,
    };

    let flags: i64 = match args.get(2) {
        Some(v) => v.to_long(),
        None => 0,
    };

    // Only JSON_INVALID_UTF8_IGNORE is allowed
    if flags != 0 && flags != JSON_INVALID_UTF8_IGNORE {
        let exc = vm.create_exception(b"ValueError", "json_validate(): Argument #3 ($flags) must be a valid flag (allowed flags: JSON_INVALID_UTF8_IGNORE)", 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: "Uncaught ValueError".to_string(), line: 0 });
    }

    let ignore_utf8 = flags & JSON_INVALID_UTF8_IGNORE != 0;

    // Check UTF-8 validity unless JSON_INVALID_UTF8_IGNORE is set
    if !ignore_utf8 {
        if let Some(utf8_err) = check_json_utf8(json_bytes) {
            match utf8_err {
                JsonUtf8Error::InvalidUtf8 => {
                    vm.json_last_error = 5;
                    return Ok(Value::False);
                }
                JsonUtf8Error::ControlChar => {
                    // Control chars inside strings are syntax errors for validate
                    vm.json_last_error = 5;
                    return Ok(Value::False);
                }
            }
        }
    }

    let mut parser = JsonParser {
        input: json_bytes,
        pos: 0,
        depth: 0,
        max_depth,
        associative: true,
        bigint_as_string: false,
        error_code: 0,
        vm,
    };

    match parser.parse_value() {
        Some(_) => {
            parser.skip_whitespace();
            if parser.pos < parser.input.len() {
                vm.json_last_error = 4;
                Ok(Value::False)
            } else {
                vm.json_last_error = 0;
                Ok(Value::True)
            }
        }
        None => {
            let error_code = if parser.error_code != 0 {
                parser.error_code
            } else if parser.depth >= parser.max_depth {
                1 // JSON_ERROR_DEPTH
            } else {
                4 // JSON_ERROR_SYNTAX
            };
            vm.json_last_error = error_code;
            Ok(Value::False)
        }
    }
}
