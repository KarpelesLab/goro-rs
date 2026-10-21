use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

/// Check if a Value is an internal closure representation (string or array starting with __closure_/__arrow_/__bound_closure_/__closure_fcc_)
fn is_closure_value(val: &Value) -> bool {
    match val {
        Value::String(s) => {
            let bytes = s.as_bytes();
            bytes.starts_with(b"__closure_")
                || bytes.starts_with(b"__arrow_")
                || bytes.starts_with(b"__bound_closure_")
                || bytes.starts_with(b"__closure_fcc_")
        }
        Value::Array(arr) => {
            let arr = arr.borrow();
            if let Some(first) = arr.values().next() {
                if let Value::String(s) = first {
                    let bytes = s.as_bytes();
                    return bytes.starts_with(b"__closure_")
                        || bytes.starts_with(b"__arrow_")
                        || bytes.starts_with(b"__bound_closure_")
                        || bytes.starts_with(b"__closure_fcc_");
                }
            }
            false
        }
        _ => false,
    }
}

pub fn register(vm: &mut Vm) {
    vm.register_function(b"gettype", gettype);
    vm.register_function(b"is_null", is_null);
    vm.register_function(b"is_bool", is_bool);
    vm.register_function(b"is_int", is_int);
    vm.register_function(b"is_integer", is_int);
    vm.register_function(b"is_long", is_int);
    vm.register_function(b"is_float", is_float);
    vm.register_function(b"is_double", is_float);
    vm.register_function(b"is_string", is_string);
    vm.register_function(b"is_array", is_array);
    vm.register_function(b"is_numeric", is_numeric);
    vm.register_function(b"intval", intval);
    vm.register_function(b"floatval", floatval);
    vm.register_function(b"doubleval", floatval);
    vm.register_function(b"strval", strval);
    vm.register_function(b"boolval", boolval);
    vm.register_function(b"settype", settype);
    vm.register_function(b"isset", php_isset);
    vm.register_function(b"empty", php_empty);
    vm.register_function(b"count", count);
    vm.register_function(b"sizeof", sizeof);
    vm.register_function(b"is_scalar", is_scalar);
    vm.register_function(b"is_resource", is_resource);
    vm.register_function(b"is_countable", is_countable);
    vm.register_function(b"is_iterable", is_iterable);
    vm.register_function(b"get_debug_type", get_debug_type);
}

fn gettype(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    if val.is_resource() {
        let label = if goro_core::value::is_open_resource(val.resource_id()) {
            "resource"
        } else {
            "resource (closed)"
        };
        return Ok(Value::String(PhpString::from_bytes(label.as_bytes())));
    }
    let type_name = match val {
        Value::Undef | Value::Null => "NULL",
        Value::True | Value::False => "boolean",
        Value::Long(_) => "integer",
        Value::Double(_) => "double",
        Value::String(_) if is_closure_value(val) => "object",
        Value::String(_) => "string",
        Value::Array(_) if is_closure_value(val) => "object",
        Value::Array(_) => "array",
        Value::Object(_) | Value::Generator(_) => "object",
        Value::Reference(r) => return gettype(_vm, &[r.borrow().clone()]),
        Value::Resource(_) => unreachable!(),
    };
    Ok(Value::String(PhpString::from_bytes(type_name.as_bytes())))
}

fn is_null(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(match args.first().unwrap_or(&Value::Null) {
        Value::Null | Value::Undef => Value::True,
        _ => Value::False,
    })
}

fn is_bool(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(match args.first().unwrap_or(&Value::Null) {
        Value::True | Value::False => Value::True,
        _ => Value::False,
    })
}

fn is_int(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(match args.first().unwrap_or(&Value::Null) {
        Value::Long(_) => Value::True,
        _ => Value::False,
    })
}

fn is_float(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(match args.first().unwrap_or(&Value::Null) {
        Value::Double(_) => Value::True,
        _ => Value::False,
    })
}

fn is_string(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    Ok(match val {
        Value::String(_) if is_closure_value(val) => Value::False,
        Value::String(_) => Value::True,
        Value::Reference(r) => {
            let inner = r.borrow();
            match &*inner {
                Value::String(_) if is_closure_value(&inner) => Value::False,
                Value::String(_) => Value::True,
                _ => Value::False,
            }
        }
        _ => Value::False,
    })
}

fn is_array(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(match args.first().unwrap_or(&Value::Null) {
        Value::Array(_) => Value::True,
        _ => Value::False,
    })
}

fn is_numeric(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        let msg = "is_numeric() expects exactly 1 argument, 0 given";
        let exc = vm.create_exception(b"ArgumentCountError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.into(), line: vm.current_line });
    }
    if args.len() > 1 {
        let msg = format!("is_numeric() expects exactly 1 argument, {} given", args.len());
        let exc = vm.create_exception(b"ArgumentCountError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let val = &args[0];
    Ok(match val {
        Value::Long(_) | Value::Double(_) => Value::True,
        Value::String(s) => {
            if php_is_numeric_string(s.as_bytes()) {
                Value::True
            } else {
                Value::False
            }
        }
        _ => Value::False,
    })
}

/// Check if a byte string is a valid PHP numeric string.
/// PHP allows leading whitespace but not trailing whitespace.
fn php_is_numeric_string(s: &[u8]) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut i = 0;
    // Skip leading whitespace
    while i < s.len() && (s[i] == b' ' || s[i] == b'\t' || s[i] == b'\n' || s[i] == b'\r' || s[i] == 0x0b || s[i] == 0x0c) {
        i += 1;
    }
    if i >= s.len() {
        return false;
    }
    // Optional sign
    if s[i] == b'+' || s[i] == b'-' {
        i += 1;
    }
    if i >= s.len() {
        return false;
    }
    let mut has_digits = false;
    // Integer part
    while i < s.len() && s[i].is_ascii_digit() {
        has_digits = true;
        i += 1;
    }
    // Decimal point and fractional part
    if i < s.len() && s[i] == b'.' {
        i += 1;
        while i < s.len() && s[i].is_ascii_digit() {
            has_digits = true;
            i += 1;
        }
    }
    if !has_digits {
        return false;
    }
    // Exponent part
    if i < s.len() && (s[i] == b'e' || s[i] == b'E') {
        i += 1;
        if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
            i += 1;
        }
        if i >= s.len() || !s[i].is_ascii_digit() {
            return false;
        }
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
        }
    }
    // Skip trailing whitespace (PHP allows it)
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    i == s.len()
}

#[allow(clippy::if_same_then_else)] // branches mirror php-src
fn intval(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let base = args.get(1).map(|v| v.to_long()).unwrap_or(10);

    if let Value::String(s) = val {
        let s_str = s.to_string_lossy();
        let trimmed = s_str.trim();

        // Handle sign
        let (sign, trimmed) = if let Some(rest) = trimmed.strip_prefix('-') {
            (-1i64, rest.trim_start())
        } else if let Some(rest) = trimmed.strip_prefix('+') {
            (1i64, rest.trim_start())
        } else {
            (1i64, trimmed)
        };

        if base == 0 {
            // Auto-detect base from prefix
            let result = if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
                parse_int_str(&trimmed[2..], 16)
            } else if trimmed.starts_with("0b") || trimmed.starts_with("0B") {
                parse_int_str(&trimmed[2..], 2)
            } else if trimmed.starts_with("0o") || trimmed.starts_with("0O") {
                parse_int_str(&trimmed[2..], 8)
            } else if trimmed.starts_with('0') && trimmed.len() > 1 {
                parse_int_str(&trimmed[1..], 8)
            } else {
                parse_int_str(trimmed, 10)
            };
            return Ok(Value::Long(sign * result));
        } else if (2..=36).contains(&base) {
            // Strip prefix for matching bases
            let digits = if (base == 16) && (trimmed.starts_with("0x") || trimmed.starts_with("0X")) {
                &trimmed[2..]
            } else if (base == 2) && (trimmed.starts_with("0b") || trimmed.starts_with("0B")) {
                &trimmed[2..]
            } else if (base == 8) && (trimmed.starts_with("0o") || trimmed.starts_with("0O")) {
                &trimmed[2..]
            } else {
                trimmed
            };
            let result = parse_int_str(digits, base as u32);
            return Ok(Value::Long(sign * result));
        }
    }

    if base != 10 {
        // Non-string with non-10 base: convert to string first
        let s = val.to_php_string();
        let trimmed = s.to_string_lossy();
        let trimmed = trimmed.trim();
        if (2..=36).contains(&base) {
            return Ok(Value::Long(parse_int_str(trimmed, base as u32)));
        }
    }

    Ok(Value::Long(val.to_long()))
}

/// Parse an integer string in a given radix, stopping at the first invalid character.
/// Returns 0 for empty/invalid strings.
fn parse_int_str(s: &str, radix: u32) -> i64 {
    let mut result: i64 = 0;
    for c in s.chars() {
        match c.to_digit(radix) {
            Some(d) => {
                result = result.wrapping_mul(radix as i64).wrapping_add(d as i64);
            }
            None => break,
        }
    }
    result
}

fn floatval(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double(),
    ))
}

fn strval(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let derefed = val.deref();
    if matches!(&derefed, Value::Array(_)) {
        vm.emit_warning("Array to string conversion");
    }
    // Object without a __toString hook is not convertible — PHP throws Error.
    if let Value::Object(obj) = &derefed {
        let class_name_bytes = obj.borrow().class_name.clone();
        let class_lower: Vec<u8> = class_name_bytes.iter().map(|b| b.to_ascii_lowercase()).collect();
        let has_to_string = {
            let mut found = false;
            let mut cur = Some(class_lower);
            while let Some(cname) = cur {
                if let Some(cls) = vm.classes.get(&cname) {
                    if cls.methods.contains_key(b"__tostring".as_slice()) {
                        found = true;
                        break;
                    }
                    cur = cls.parent.as_ref().map(|p| p.iter().map(|b| b.to_ascii_lowercase()).collect());
                } else {
                    break;
                }
            }
            found
        };
        if !has_to_string {
            let class_name = goro_core::value::display_class_name(&class_name_bytes);
            let msg = format!("Object of class {} could not be converted to string", class_name);
            let exc = vm.create_exception(b"Error", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: vm.current_line });
        }
        return Ok(Value::String(vm.value_to_string_checked(&derefed)?));
    }
    Ok(Value::String(val.to_php_string()))
}

fn boolval(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(if args.first().unwrap_or(&Value::Null).is_truthy() {
        Value::True
    } else {
        Value::False
    })
}

#[allow(clippy::blocks_in_conditions)] // scrutinee emits the NaN warning before matching
fn settype(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let var_ref = args.first().unwrap_or(&Value::Null);
    let type_name = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let type_str = type_name.to_string_lossy();

    if let Value::Reference(r) = var_ref {
        let current = r.borrow().clone();
        let new_val = match type_str.to_ascii_lowercase().as_str() {
            "int" | "integer" => {
                if let Value::Object(obj) = &current {
                    let class_name = goro_core::value::display_class_name(&obj.borrow().class_name);
                    _vm.emit_warning(&format!("Object of class {} could not be converted to int", class_name));
                }
                let l = current.to_long();
                let line = _vm.current_line;
                _vm.warn_oob_float_to_int(&current, line);
                Value::Long(l)
            }
            "float" | "double" => {
                if let Value::Object(obj) = &current {
                    let class_name = goro_core::value::display_class_name(&obj.borrow().class_name);
                    _vm.emit_warning(&format!("Object of class {} could not be converted to float", class_name));
                }
                Value::Double(current.to_double())
            }
            "string" => {
                if matches!(&current, Value::Array(_)) {
                    _vm.emit_warning("Array to string conversion");
                }
                if let Value::Object(_) = &current {
                    match _vm.value_to_string_checked(&current) {
                        Ok(s) => Value::String(s),
                        Err(e) => {
                            // convert_to_string() leaves an empty string behind on failure.
                            *r.borrow_mut() = Value::String(PhpString::empty());
                            return Err(e);
                        }
                    }
                } else {
                    let line = _vm.current_line;
                    _vm.warn_nan_coercion(&current, "string", line);
                    Value::String(current.to_php_string())
                }
            }
            "bool" | "boolean" => {
                let line = _vm.current_line;
                _vm.warn_nan_coercion(&current, "bool", line);
                if current.is_truthy() {
                    Value::True
                } else {
                    Value::False
                }
            }
            "array" => match { let line = _vm.current_line; _vm.warn_nan_coercion(&current, "array", line); if matches!(current, Value::Double(d) if d.is_nan()) { r.borrow().clone() } else { current } } {
                cur @ Value::Array(_) => cur,
                Value::Null | Value::Undef => {
                    Value::Array(std::rc::Rc::new(std::cell::RefCell::new(
                        goro_core::array::PhpArray::new(),
                    )))
                }
                Value::Object(ref o) => Value::Array(_vm.object_to_array(o)),
                other => {
                    let mut arr = goro_core::array::PhpArray::new();
                    arr.push(other);
                    Value::Array(std::rc::Rc::new(std::cell::RefCell::new(arr)))
                }
            },
            "object" => match { let line = _vm.current_line; _vm.warn_nan_coercion(&current, "object", line); if matches!(current, Value::Double(d) if d.is_nan()) { r.borrow().clone() } else { current } } {
                cur @ Value::Object(_) => cur,
                Value::Array(arr) => {
                    let mut obj = goro_core::object::PhpObject::new(b"stdClass".to_vec(), _vm.alloc_object_id());
                    {
                        let arr_borrow = arr.borrow();
                        for (k, v) in arr_borrow.iter() {
                            let key_bytes = match k {
                                goro_core::array::ArrayKey::Int(i) => {
                                    format!("{}", i).into_bytes()
                                }
                                goro_core::array::ArrayKey::String(s) => s.as_bytes().to_vec(),
                            };
                            obj.set_property(key_bytes, v.clone());
                        }
                    }
                    Value::Object(std::rc::Rc::new(std::cell::RefCell::new(obj)))
                }
                cur => {
                    let mut obj = goro_core::object::PhpObject::new(b"stdClass".to_vec(), _vm.alloc_object_id());
                    if !matches!(cur, Value::Null | Value::Undef) {
                        obj.set_property(b"scalar".to_vec(), cur);
                    }
                    Value::Object(std::rc::Rc::new(std::cell::RefCell::new(obj)))
                }
            },
            "null" => {
                let line = _vm.current_line;
                _vm.warn_nan_coercion(&current, "null", line);
                Value::Null
            }
            "resource" => {
                let msg = "Cannot convert to resource type";
                let exc = _vm.create_exception(b"ValueError", msg, _vm.current_line);
                _vm.current_exception = Some(exc);
                return Err(VmError { message: format!("Uncaught ValueError: {}", msg), line: _vm.current_line });
            }
            _ => {
                let msg = "settype(): Argument #2 ($type) must be a valid type";
                let exc = _vm.create_exception(b"ValueError", msg, _vm.current_line);
                _vm.current_exception = Some(exc);
                return Err(VmError { message: msg.to_string(), line: _vm.current_line });
            }
        };
        *r.borrow_mut() = new_val;
        Ok(Value::True)
    } else {
        // Not a reference - can't modify
        Ok(Value::True)
    }
}

fn php_isset(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    for arg in args {
        match arg {
            Value::Undef | Value::Null => return Ok(Value::False),
            _ => {}
        }
    }
    Ok(Value::True)
}

fn php_empty(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    Ok(if val.is_truthy() {
        Value::False
    } else {
        Value::True
    })
}

fn count(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    count_impl(vm, args, "count")
}

fn sizeof(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    count_impl(vm, args, "sizeof")
}

fn count_impl(vm: &mut Vm, args: &[Value], fname: &str) -> Result<Value, VmError> {
    let val_raw = args.first().unwrap_or(&Value::Null);
    // Dereference if a reference
    let val_owned;
    let val: &Value = match val_raw {
        Value::Reference(r) => { val_owned = r.borrow().clone(); &val_owned }
        _ => val_raw,
    };
    let mode = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    // Validate mode: PHP 8+ throws ValueError for anything other than 0/1.
    // Note: bool TRUE casts to 1, FALSE casts to 0, so they're valid.
    if mode != 0 && mode != 1 {
        let msg = format!(
            "{}(): Argument #2 ($mode) must be either COUNT_NORMAL or COUNT_RECURSIVE",
            fname
        );
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    match val {
        Value::Array(arr) => {
            if mode == 1 {
                // COUNT_RECURSIVE
                Ok(Value::Long(count_recursive(vm, arr, &mut Vec::new())))
            } else {
                Ok(Value::Long(arr.borrow().len() as i64))
            }
        }
        Value::Object(obj) => {
            // Check for Countable interface - try to call count() method
            let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();

            // First check if user class has overridden count() method
            let has_user_count = vm.classes.get(&class_lower)
                .map(|c| c.get_method(b"count").is_some())
                .unwrap_or(false);

            if has_user_count {
                // Call user-defined count() method; the result goes through
                // zval_get_long().
                let result = vm.call_object_method(val, b"count", &[]);
                if vm.current_exception.is_some() {
                    return Err(VmError { message: "Uncaught exception".to_string(), line: vm.current_line });
                }
                let n = match result.unwrap_or(Value::Null).deref() {
                    Value::Object(o) => {
                        let cn = goro_core::value::display_class_name(&o.borrow().class_name).to_string();
                        vm.emit_warning(&format!("Object of class {} could not be converted to int", cn));
                        1
                    }
                    Value::Array(a) => if !a.borrow().is_empty() { 1 } else { 0 },
                    other => other.to_long(),
                };
                return Ok(Value::Long(n));
            }

            // Check if this is an SPL class with __spl_array
            let ob = obj.borrow();
            let spl_arr = ob.get_property(b"__spl_array");
            if let Value::Array(a) = spl_arr {
                return Ok(Value::Long(a.borrow().len() as i64));
            }
            drop(ob);

            // Try to call $obj->count() if it implements Countable (built-in SPL)
            if let Some(result) = vm.call_object_method(val, b"count", &[]) {
                return Ok(result);
            }

            // Non-countable object: TypeError in PHP 8+
            let class_name = String::from_utf8_lossy(&obj.borrow().class_name).into_owned();
            let err_msg = format!("{}(): Argument #1 ($value) must be of type Countable|array, {} given", fname, class_name);
            let exc = vm.throw_type_error(err_msg.clone());
            vm.current_exception = Some(exc);
            Err(VmError { message: format!("Uncaught TypeError: {}", err_msg), line: vm.current_line })
        }
        Value::Generator(_) => {
            let err_msg = format!("{}(): Argument #1 ($value) must be of type Countable|array, Generator given", fname);
            let exc = vm.throw_type_error(err_msg.clone());
            vm.current_exception = Some(exc);
            Err(VmError { message: format!("Uncaught TypeError: {}", err_msg), line: vm.current_line })
        }
        _ => {
            // null, bool, int, float, string -> TypeError in PHP 8+
            let type_name = Vm::value_type_name(val);
            let err_msg = format!("{}(): Argument #1 ($value) must be of type Countable|array, {} given", fname, type_name);
            let exc = vm.throw_type_error(err_msg.clone());
            vm.current_exception = Some(exc);
            Err(VmError { message: format!("Uncaught TypeError: {}", err_msg), line: vm.current_line })
        }
    }
}

/// php_count_recursive()
fn count_recursive(vm: &mut Vm, arr: &std::rc::Rc<std::cell::RefCell<goro_core::array::PhpArray>>, stack: &mut Vec<usize>) -> i64 {
    let ptr = std::rc::Rc::as_ptr(arr) as usize;
    if stack.contains(&ptr) {
        vm.emit_warning("count(): Recursion detected");
        return 0;
    }
    stack.push(ptr);
    let children: Vec<std::rc::Rc<std::cell::RefCell<goro_core::array::PhpArray>>> = arr.borrow().values()
        .filter_map(|v| match v.deref() { Value::Array(a) => Some(a), _ => None })
        .collect();
    let mut total = arr.borrow().len() as i64;
    for c in children {
        total += count_recursive(vm, &c, stack);
    }
    stack.pop();
    total
}

fn is_scalar(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    Ok(match val {
        Value::String(_) if is_closure_value(val) => Value::False,
        Value::Long(_) | Value::Double(_) | Value::String(_) | Value::True | Value::False => {
            Value::True
        }
        Value::Reference(r) => {
            let inner = r.borrow();
            match &*inner {
                Value::String(_) if is_closure_value(&inner) => Value::False,
                Value::Long(_)
                | Value::Double(_)
                | Value::String(_)
                | Value::True
                | Value::False => Value::True,
                _ => Value::False,
            }
        }
        _ => Value::False,
    })
}

fn is_resource(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    // is_resource returns false for closed resources (matches PHP semantics)
    Ok(if val.is_resource() && goro_core::value::is_open_resource(val.resource_id()) {
        Value::True
    } else {
        Value::False
    })
}

/// `$obj instanceof $target` for user and builtin classes/interfaces.
fn class_instanceof(vm: &Vm, class_lower: &[u8], target: &[u8]) -> bool {
    let mut cur = class_lower.to_vec();
    for _ in 0..64 {
        if cur == target || vm.class_implements_interface(&cur, target)
            || goro_core::vm::is_builtin_subclass(&cur, target)
        {
            return true;
        }
        let next = vm.classes.get(&cur).and_then(|c| c.parent.as_ref())
            .map(|p| p.iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<u8>>());
        match next {
            Some(n) => cur = n,
            None => return false,
        }
    }
    false
}

fn is_countable(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    Ok(match &val {
        Value::Array(_) => Value::True,
        Value::Object(obj) => {
            let cl: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if class_instanceof(vm, &cl, b"countable") { Value::True } else { Value::False }
        }
        _ => Value::False,
    })
}

fn is_iterable(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    Ok(match &val {
        Value::Array(_) | Value::Generator(_) => Value::True,
        Value::Object(obj) => {
            let cl: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if class_instanceof(vm, &cl, b"traversable") { Value::True } else { Value::False }
        }
        _ => Value::False,
    })
}

fn get_debug_type(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use goro_core::string::PhpString;
    let val = args.first().unwrap_or(&Value::Null);
    let type_name = match val {
        Value::Null | Value::Undef => "null",
        Value::True | Value::False => "bool",
        Value::Long(_) => "int",
        Value::Double(_) => "float",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(obj) => {
            // Anonymous classes: "Parent@anonymous" (the part before the NUL byte).
            let name = goro_core::value::display_class_name(&obj.borrow().class_name);
            return Ok(Value::String(PhpString::from_string(name)));
        }
        Value::Generator(_) => "Generator",
        Value::Reference(r) => {
            let args = [r.borrow().clone()];
            return get_debug_type(_vm, &args);
        }
        Value::Resource(id) => {
            let t = goro_core::value::resource_type_of(*id)
                .map(|v| String::from_utf8_lossy(&v).to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            let s = if goro_core::value::is_open_resource(*id) {
                format!("resource ({})", t)
            } else {
                "resource (closed)".to_string()
            };
            return Ok(Value::String(PhpString::from_string(s)));
        }
    };
    Ok(Value::String(goro_core::string::PhpString::from_bytes(
        type_name.as_bytes(),
    )))
}
