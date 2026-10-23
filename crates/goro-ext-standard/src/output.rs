use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::collections::HashSet;
use std::rc::Rc;

pub fn register(vm: &mut Vm) {
    vm.register_function(b"var_dump", var_dump);
    vm.register_function(b"print_r", print_r);
    vm.register_function(b"var_export", var_export);
}

pub(crate) fn is_internal_property(name: &[u8]) -> bool {
    name.starts_with(b"__spl_") || name.starts_with(b"__reflection_")
        || name.starts_with(b"__timestamp") || name.starts_with(b"__enum_")
        || name.starts_with(b"__fiber_") || name.starts_with(b"__ctor_")
        || name.starts_with(b"__clone_") || name.starts_with(b"__destructed")
        || name.starts_with(b"__weak_ref_id") || name.starts_with(b"__sxml_")
        || name.starts_with(b"__hash_") || name.starts_with(b"__hmac_")
        || name.starts_with(b"__lazy_")
        || name.starts_with(b"__dir_") || name.starts_with(b"__rng_")
        || name.starts_with(b"__pkey_id")
        || name.starts_with(b"__cert_id")
        || name.starts_with(b"__csr_id")
        || name.starts_with(b"__xml_")
        || name.starts_with(b"__sensitive_value")
        // ReflectionType internal storage (PHP exposes no public properties on
        // ReflectionNamedType / ReflectionUnionType / ReflectionIntersectionType).
        || matches!(name, b"__type_name" | b"__allows_null" | b"__is_builtin" | b"__types")
        // DateInterval components from createFromDateString are stored under
        // these short hidden names; PHP only exposes from_string + date_string.
        || matches!(name, b"__y" | b"__m" | b"__d" | b"__h" | b"__i" | b"__s" | b"__f" | b"__invert")
}

fn var_dump(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        let msg = "var_dump() expects at least 1 argument, 0 given";
        let exc = vm.create_exception(b"ArgumentCountError", msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.into(),
            line: vm.current_line,
        });
    }
    // Objects an outer var_dump() is dumping while it runs __debugInfo()
    // stay protected (ZEND_GUARD_PROTECT_RECURSION(guard, DEBUG)).
    let mut seen = DUMPING.with(|d| d.borrow().clone());
    for arg in args {
        var_dump_value(vm, arg, 0, &mut seen);
        if vm.fatal_bailout {
            return Err(VmError {
                message: String::new(),
                line: vm.current_line,
            });
        }
    }
    Ok(Value::Null)
}

/// A property-table key as php_object_property_dump() shows it: mangled
/// names display their visibility.
fn write_mangled_dump_key(vm: &mut Vm, prefix: &str, b: &[u8]) {
    let mangled = if b.first() == Some(&0u8) {
        b[1..]
            .iter()
            .position(|&c| c == 0)
            .map(|e| (&b[1..1 + e], &b[2 + e..]))
    } else {
        None
    };
    let out = match mangled {
        Some((b"*", name)) => [prefix.as_bytes(), b"  [\"", name, b"\":protected]=>\n"].concat(),
        Some((cls, name)) => [
            prefix.as_bytes(),
            b"  [\"",
            name,
            b"\":\"",
            cls,
            b"\":private]=>\n",
        ]
        .concat(),
        None => [prefix.as_bytes(), b"  [\"", b, b"\"]=>\n"].concat(),
    };
    vm.write_output(&out);
}

thread_local! {
    /// The objects var_dump() is dumping while it calls back into PHP code.
    static DUMPING: std::cell::RefCell<HashSet<u64>> = std::cell::RefCell::new(HashSet::new());
}

thread_local! {
    /// Set while debug_zval_dump() runs: var_dump()'s output with the
    /// refcount / interned / packed annotations of php_debug_zval_dump().
    static ZVAL_DUMP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Holders the dumper itself added to the value being dumped: the
    /// copies it makes of an object's properties or a reference's value
    /// before descending into them.
    static ZVAL_BIAS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// debug_zval_dump(). goro does not keep PHP refcounts: the counts shown
/// are the number of holders of the shared string / array / object /
/// reference cell (the variable and the argument for a dumped variable,
/// like PHP), which can differ from PHP's where goro holds extra copies
/// (values copied while an object's properties are dumped, temporaries).
/// "interned" marks compiled literals, one-byte and empty strings, and the
/// empty array; "packed" is decided from the array's keys.
pub(crate) fn debug_zval_dump(vm: &mut Vm, args: &[Value]) {
    let prev = ZVAL_DUMP.with(|z| z.replace(true));
    let mut seen = HashSet::new();
    for arg in args {
        var_dump_value(vm, arg, 0, &mut seen);
    }
    ZVAL_DUMP.with(|z| z.set(prev));
}

/// php_object_property_dump()'s key: `"name"`, `"name":protected` or
/// `"name":"Class":private` from a property table key (int-like keys of
/// GMP's table print bare). A key starting with a NUL that is no mangled
/// name raises a notice and prints as it is (debug_zval_dump(): up to its
/// first NUL, zval_object_property_dump() prints it with `%s`).
fn debug_prop_key(vm: &mut Vm, key: &[u8], int_keys: bool) -> String {
    match goro_core::proptable::unmangle_property_name_ex(key) {
        Ok((Some(class), name)) => {
            let name = String::from_utf8_lossy(name);
            if class == b"*" {
                return format!("\"{}\":protected", name);
            }
            return format!(
                "\"{}\":\"{}\":private",
                name,
                goro_core::value::display_class_name(class)
            );
        }
        Ok((None, _)) => {}
        Err(notice) => {
            let line = vm.current_line;
            vm.emit_notice_at(notice, line);
            if ZVAL_DUMP.with(|z| z.get()) {
                let end = key.iter().position(|&c| c == 0).unwrap_or(key.len());
                return format!("\"{}\"", String::from_utf8_lossy(&key[..end]));
            }
        }
    }
    let name = String::from_utf8_lossy(key);
    if int_keys {
        if let Ok(n) = name.parse::<i64>() {
            if n.to_string() == name {
                return name.to_string();
            }
        }
    }
    format!("\"{}\"", name)
}

/// Run `__debugInfo()` for var_dump() with the object protected from
/// recursion, also for var_dump() calls it makes.
fn debug_info_protected(
    vm: &mut Vm,
    val: &Value,
    oid: u64,
    seen: &mut HashSet<u64>,
) -> Option<Value> {
    seen.insert(oid);
    let outer = DUMPING.with(|d| d.replace(seen.clone()));
    let r = vm.call_object_method(val, b"__debugInfo", &[]);
    DUMPING.with(|d| *d.borrow_mut() = outer);
    seen.remove(&oid);
    r
}

fn var_dump_value(vm: &mut Vm, val: &Value, indent: usize, seen: &mut HashSet<u64>) {
    if !ZVAL_DUMP.with(|z| z.get()) {
        return var_dump_value_inner(vm, val, indent, seen);
    }
    let bias = ZVAL_BIAS.with(|b| b.replace(0));
    zval_dump_value(vm, val, indent, seen, bias);
    ZVAL_BIAS.with(|b| b.set(bias));
}

/// Dump `val` (whose refcount is `bias` too high) the way
/// php_debug_zval_dump() does.
fn zval_dump_value(vm: &mut Vm, val: &Value, indent: usize, seen: &mut HashSet<u64>, bias: usize) {
    let prefix = " ".repeat(indent);
    match val {
        Value::String(s) if !goro_core::closure::is_closure_fn_name(s.as_bytes()) => {
            vm.write_output(format!("{}string({}) \"", prefix, s.len()).as_bytes());
            vm.write_output(s.as_bytes());
            if s.is_interned() {
                vm.write_output(b"\" interned\n");
            } else {
                let rc = s.refcount().saturating_sub(bias);
                vm.write_output(format!("\" refcount({})\n", rc).as_bytes());
            }
        }
        Value::Resource(id) => {
            let t = if goro_core::value::is_open_resource(id.id()) {
                goro_core::value::resource_type_of(id.id())
                    .map(|v| String::from_utf8_lossy(&v).to_string())
                    .unwrap_or_else(|| "Unknown".to_string())
            } else {
                "Unknown".to_string()
            };
            // (the holders of the resource: the variable and the argument
            // for a dumped variable, like PHP)
            let rc = id.refcount().saturating_sub(bias);
            vm.write_output(
                format!(
                    "{}resource({}) of type ({}) refcount({})\n",
                    prefix, id, t, rc
                )
                .as_bytes(),
            );
        }
        Value::Reference(r) => {
            // A variable of the global scope is a cell shared by the
            // symbol table entry, the main script's CV slot and that
            // frame's binding record; in PHP only the table entry holds it
            // (the CV is an IS_INDIRECT slot).
            let mut rc = Rc::strong_count(r).saturating_sub(bias);
            let in_symtab = vm
                .globals_array()
                .borrow()
                .iter()
                .any(|(_, v)| matches!(v, Value::Reference(c) if Rc::ptr_eq(c, r)));
            if in_symtab {
                rc = rc.saturating_sub(2).max(1);
            }
            vm.write_output(format!("{}reference refcount({}) {{\n", prefix, rc).as_bytes());
            let inner = r.borrow().clone();
            ZVAL_BIAS.with(|b| b.set(1));
            var_dump_value(vm, &inner, indent + 2, seen);
            ZVAL_BIAS.with(|b| b.set(0));
            vm.write_output(format!("{}}}\n", prefix).as_bytes());
        }
        Value::Array(arr) => {
            let akey = arr.borrow().identity() | (1u64 << 63);
            if !seen.insert(akey) {
                vm.write_output(format!("{}*RECURSION*\n", prefix).as_bytes());
                return;
            }
            // (the copies sharing its storage are holders of PHP's array)
            let rc = (Rc::strong_count(arr) + arr.borrow().shared_copies()).saturating_sub(bias);
            let a = arr.borrow();
            if a.is_empty() {
                vm.write_output(format!("{}array(0) interned {{\n", prefix).as_bytes());
            } else {
                let packed = if a.looks_packed() { "packed " } else { "" };
                vm.write_output(
                    format!(
                        "{}array({}) {}refcount({}){{\n",
                        prefix,
                        a.len(),
                        packed,
                        rc
                    )
                    .as_bytes(),
                );
            }
            for (key, value) in a.iter() {
                match key {
                    goro_core::array::ArrayKey::Int(n) => {
                        vm.write_output(format!("{}  [{}]=>\n", prefix, n).as_bytes());
                    }
                    goro_core::array::ArrayKey::String(k) => {
                        vm.write_output(
                            &[prefix.as_bytes(), b"  [\"", k.as_bytes(), b"\"]=>\n"].concat(),
                        );
                    }
                }
                var_dump_value(vm, value, indent + 2, seen);
            }
            drop(a);
            vm.write_output(format!("{}}}\n", prefix).as_bytes());
            seen.remove(&akey);
        }
        Value::Object(obj) if obj.borrow().has_property(b"__enum_case") => {
            // php_debug_zval_dump() has no enum case: an object with its
            // name (and value) properties.
            let rc = Rc::strong_count(obj).saturating_sub(bias);
            let (class_name, oid, props) = {
                let b = obj.borrow();
                let mut props = vec![(b"name".as_slice(), b.get_property(b"name"))];
                if b.has_property(b"value") {
                    props.push((b"value".as_slice(), b.get_property(b"value")));
                }
                (
                    goro_core::value::display_class_name(&b.class_name).to_string(),
                    b.object_id,
                    props,
                )
            };
            vm.write_output(
                format!(
                    "{}object({})#{} ({}) refcount({}){{\n",
                    prefix,
                    class_name,
                    oid,
                    props.len(),
                    rc
                )
                .as_bytes(),
            );
            for (name, value) in &props {
                vm.write_output(&[prefix.as_bytes(), b"  [\"", name, b"\"]=>\n"].concat());
                ZVAL_BIAS.with(|b| b.set(1));
                var_dump_value(vm, value, indent + 2, seen);
                ZVAL_BIAS.with(|b| b.set(0));
            }
            vm.write_output(format!("{}}}\n", prefix).as_bytes());
        }
        Value::Object(obj) => {
            // var_dump()'s object dump, with the refcount added to its
            // header line ("object(C)#N (M) {").
            let mut rc = Rc::strong_count(obj).saturating_sub(bias);
            // SplFixedArray's get_properties_for(DEBUG) table, built before
            // the refcount is read, references the elements once more.
            if class_extends(
                vm,
                &obj.borrow().class_name.to_ascii_lowercase(),
                b"splfixedarray",
            ) {
                if let Value::Array(a) = obj.borrow().get_property(b"__spl_array") {
                    rc += a
                        .borrow()
                        .values()
                        .filter(|v| matches!(v, Value::Object(o) if Rc::ptr_eq(o, obj)))
                        .count();
                }
            }
            let start = vm.active_output_buffer().len();
            ZVAL_BIAS.with(|b| b.set(1));
            var_dump_value_inner(vm, val, indent, seen);
            ZVAL_BIAS.with(|b| b.set(0));
            let buf = vm.active_output_buffer();
            if start <= buf.len() {
                let line_end = buf[start..]
                    .iter()
                    .position(|&b| b == b'\n')
                    .map(|p| start + p);
                if let Some(end) = line_end {
                    if buf[start..end].ends_with(b" {") {
                        let suffix = format!(" refcount({}){{", rc);
                        buf.splice(end - 2..end, suffix.into_bytes());
                    }
                }
            }
        }
        _ => var_dump_value_inner(vm, val, indent, seen),
    }
}

/// Whether class `class_lower` is `ancestor` or extends it.
fn class_extends(vm: &Vm, class_lower: &[u8], ancestor: &[u8]) -> bool {
    let mut c = class_lower.to_vec();
    for _ in 0..64 {
        if c == ancestor {
            return true;
        }
        let next = vm
            .classes
            .get(&c)
            .and_then(|ce| ce.parent.as_ref().map(|p| p.to_ascii_lowercase()))
            .or_else(|| goro_core::vm::get_builtin_parent(&c).map(|p| p.to_ascii_lowercase()));
        match next {
            Some(n) => c = n,
            None => return false,
        }
    }
    false
}

#[allow(clippy::if_same_then_else)] // branches mirror php-src
/// C stack used by one php_var_dump() + php_object_property_dump() level.
const VAR_DUMP_FRAME_COST: usize = 256;

fn var_dump_value_inner(vm: &mut Vm, val: &Value, indent: usize, seen: &mut HashSet<u64>) {
    if indent > 8000 {
        return;
    }
    let prefix = " ".repeat(indent);
    match val {
        Value::Null | Value::Undef => {
            vm.write_output(format!("{}NULL\n", prefix).as_bytes());
        }
        Value::True => {
            vm.write_output(format!("{}bool(true)\n", prefix).as_bytes());
        }
        Value::False => {
            vm.write_output(format!("{}bool(false)\n", prefix).as_bytes());
        }
        Value::Long(n) => {
            vm.write_output(format!("{}int({})\n", prefix, n).as_bytes());
        }
        Value::Resource(id) => {
            if goro_core::value::is_open_resource(id.id()) {
                let t = goro_core::value::resource_type_of(id.id())
                    .map(|v| String::from_utf8_lossy(&v).to_string())
                    .unwrap_or_else(|| "Unknown".to_string());
                vm.write_output(format!("{}resource({}) of type ({})\n", prefix, id, t).as_bytes());
            } else if goro_core::value::is_registered_resource(id.id()) {
                // Closed resource: PHP shows "resource(N) of type (Unknown)"
                vm.write_output(
                    format!("{}resource({}) of type (Unknown)\n", prefix, id).as_bytes(),
                );
            } else {
                vm.write_output(format!("{}resource(Unknown)\n", prefix).as_bytes());
            }
        }
        Value::Double(f) => {
            // var_dump uses serialize_precision (-1 in PHP 8 = shortest representation)
            let sp = goro_core::value::get_php_serialize_precision();
            let formatted = if sp < 0 {
                format_php_float_serialize(*f)
            } else {
                goro_core::value::format_php_float_with_precision_pub(*f, sp as usize)
            };
            vm.write_output(format!("{}float({})\n", prefix, formatted).as_bytes());
        }
        Value::String(s) => {
            let b = s.as_bytes();
            // (an internal closure name; a user string that merely looks
            // like one is a string)
            if (b.starts_with(b"__closure_")
                || b.starts_with(b"__arrow_")
                || b.starts_with(b"__bound_closure_")
                || b.starts_with(b"__closure_fcc_"))
                && vm.user_functions.contains_key(&b.to_ascii_lowercase())
            {
                // Closures should be displayed as Closure objects
                // Look up the closure function to get name, file, line info
                let closure_name_str = s.to_string_lossy();
                let fn_lower = closure_name_str
                    .as_bytes()
                    .iter()
                    .map(|c| c.to_ascii_lowercase())
                    .collect::<Vec<u8>>();

                // Collect closure info before writing output
                struct ClosureDisplayInfo {
                    name: String,
                    file: String,
                    line: u32,
                    params: Vec<(String, bool)>, // (name, has_default)
                    is_variadic_last: bool,
                }

                // Check if this is an FCC (first-class callable) with a known target
                let fcc_target = vm
                    .user_functions
                    .get(&fn_lower)
                    .and_then(|op| op.fcc_target_name.clone());

                if let Some(target_name) = fcc_target {
                    // FCC closure: display with ["function"] and target's parameters
                    let target_display = goro_core::string::msg_str(&target_name).to_string();

                    // Look up the target function's parameters
                    let target_lower: Vec<u8> =
                        target_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                    let target_params: Vec<(String, bool)> = if let Some(target_op) =
                        vm.user_functions.get(&target_lower)
                    {
                        let param_count = target_op.param_count as usize;
                        let mut params = Vec::new();
                        for i in 0..param_count {
                            if let Some(cv_name) = target_op.cv_names.get(i) {
                                let pname = format!("${}", String::from_utf8_lossy(cv_name));
                                let has_default =
                                    target_op.param_has_default.get(i).copied().unwrap_or(false);
                                params.push((pname, has_default));
                            }
                        }
                        params
                    } else if let Some(fi) = goro_core::arginfo::lookup(&target_lower) {
                        // Internal function: its stub arginfo (optional and
                        // variadic parameters are "<optional>").
                        fi.params
                            .iter()
                            .enumerate()
                            .map(|(i, p)| {
                                (
                                    format!("${}", p.name),
                                    p.variadic || i >= fi.required as usize,
                                )
                            })
                            .collect()
                    } else if let Some(param_names) = vm.builtin_param_names.get(&target_lower) {
                        // Built-in function: get parameter names from registered info
                        param_names
                            .iter()
                            .map(|name| {
                                let pname = format!("${}", String::from_utf8_lossy(name));
                                (pname, false) // We don't track defaults for builtins in detail
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };

                    let has_params = !target_params.is_empty();
                    let prop_count = 1 + if has_params { 1 } else { 0 }; // function[, parameter]
                    vm.write_output(
                        format!("{}object(Closure)#1 ({}) {{\n", prefix, prop_count).as_bytes(),
                    );
                    vm.write_output(format!("{}  [\"function\"]=>\n", prefix).as_bytes());
                    vm.write_output(
                        format!(
                            "{}  string({}) \"{}\"\n",
                            prefix,
                            target_display.len(),
                            target_display
                        )
                        .as_bytes(),
                    );
                    if has_params {
                        vm.write_output(format!("{}  [\"parameter\"]=>\n", prefix).as_bytes());
                        vm.write_output(
                            format!("{}  array({}) {{\n", prefix, target_params.len()).as_bytes(),
                        );
                        for (pname, has_default) in &target_params {
                            let label = if *has_default {
                                "<optional>"
                            } else {
                                "<required>"
                            };
                            vm.write_output(
                                format!("{}    [\"{}\"]=>\n", prefix, pname).as_bytes(),
                            );
                            vm.write_output(
                                format!("{}    string({}) \"{}\"\n", prefix, label.len(), label)
                                    .as_bytes(),
                            );
                        }
                        vm.write_output(format!("{}  }}\n", prefix).as_bytes());
                    }
                    vm.write_output(format!("{}}}\n", prefix).as_bytes());
                } else {
                    let closure_info = vm.user_functions.get(&fn_lower).map(|op| {
                        let file = String::from_utf8_lossy(&op.filename).to_string();
                        let line = op.decl_line;
                        let name = if op.name.starts_with(b"__closure_fcc_") {
                            "{closure}".to_string()
                        } else if !op.display_name.is_empty() {
                            String::from_utf8_lossy(&op.display_name).to_string()
                        } else {
                            format!("{{closure:{}:{}}}", file, line)
                        };
                        let param_count = op.param_count as usize;
                        let mut params = Vec::new();
                        for i in 0..param_count {
                            if let Some(cv_name) = op.cv_names.get(i) {
                                let pname = format!("${}", String::from_utf8_lossy(cv_name));
                                let has_default =
                                    op.param_has_default.get(i).copied().unwrap_or(false);
                                params.push((pname, has_default));
                            }
                        }
                        let is_variadic_last = op.variadic_param.is_some();
                        ClosureDisplayInfo {
                            name,
                            file,
                            line,
                            params,
                            is_variadic_last,
                        }
                    });
                    if let Some(info) = closure_info {
                        let has_params = !info.params.is_empty();
                        let prop_count = 3 + if has_params { 1 } else { 0 }; // name, file, line[, parameter]
                        vm.write_output(
                            format!("{}object(Closure)#1 ({}) {{\n", prefix, prop_count).as_bytes(),
                        );
                        vm.write_output(format!("{}  [\"name\"]=>\n", prefix).as_bytes());
                        vm.write_output(
                            format!(
                                "{}  string({}) \"{}\"\n",
                                prefix,
                                info.name.len(),
                                info.name
                            )
                            .as_bytes(),
                        );
                        vm.write_output(format!("{}  [\"file\"]=>\n", prefix).as_bytes());
                        vm.write_output(
                            format!(
                                "{}  string({}) \"{}\"\n",
                                prefix,
                                info.file.len(),
                                info.file
                            )
                            .as_bytes(),
                        );
                        vm.write_output(format!("{}  [\"line\"]=>\n", prefix).as_bytes());
                        vm.write_output(format!("{}  int({})\n", prefix, info.line).as_bytes());
                        if has_params {
                            vm.write_output(format!("{}  [\"parameter\"]=>\n", prefix).as_bytes());
                            vm.write_output(
                                format!("{}  array({}) {{\n", prefix, info.params.len()).as_bytes(),
                            );
                            for (i, (pname, has_default)) in info.params.iter().enumerate() {
                                let is_variadic =
                                    info.is_variadic_last && i == info.params.len() - 1;
                                let label = if is_variadic || *has_default {
                                    "<optional>"
                                } else {
                                    "<required>"
                                };
                                vm.write_output(
                                    format!("{}    [\"{}\"]=>\n", prefix, pname).as_bytes(),
                                );
                                vm.write_output(
                                    format!(
                                        "{}    string({}) \"{}\"\n",
                                        prefix,
                                        label.len(),
                                        label
                                    )
                                    .as_bytes(),
                                );
                            }
                            vm.write_output(format!("{}  }}\n", prefix).as_bytes());
                        }
                        vm.write_output(format!("{}}}\n", prefix).as_bytes());
                    } else {
                        vm.write_output(
                            format!("{}object(Closure)#1 (0) {{\n{}}}\n", prefix, prefix)
                                .as_bytes(),
                        );
                    }
                }
            } else {
                vm.write_output(format!("{}string({}) \"", prefix, s.len()).as_bytes());
                vm.write_output(s.as_bytes());
                vm.write_output(b"\"\n");
            }
        }
        Value::Array(arr) => {
            let akey = arr.borrow().identity() | (1u64 << 63);
            if !seen.insert(akey) {
                vm.write_output(format!("{}*RECURSION*\n", prefix).as_bytes());
                return;
            }
            let arr = arr.borrow();
            vm.write_output(format!("{}array({}) {{\n", prefix, arr.len()).as_bytes());
            for (key, value) in arr.iter() {
                match key {
                    goro_core::array::ArrayKey::Int(n) => {
                        vm.write_output(format!("{}  [{}]=>\n", prefix, n).as_bytes());
                    }
                    goro_core::array::ArrayKey::String(s) => {
                        vm.write_output(
                            &[prefix.as_bytes(), b"  [\"", s.as_bytes(), b"\"]=>\n"].concat(),
                        );
                    }
                }
                var_dump_value(vm, value, indent + 2, seen);
            }
            vm.write_output(format!("{}}}\n", prefix).as_bytes());
            seen.remove(&akey);
        }
        Value::Object(obj) => {
            if let Some(arr) = native_props(vm, obj, 0) {
                let (cn, oid) = {
                    let b = obj.borrow();
                    (
                        goro_core::value::display_class_name(&b.class_name).to_string(),
                        b.object_id,
                    )
                };
                if seen.contains(&oid) {
                    vm.write_output(format!("{}*RECURSION*\n", prefix).as_bytes());
                    return;
                }
                vm.write_output(
                    format!("{}object({})#{} ({}) {{\n", prefix, cn, oid, arr.len()).as_bytes(),
                );
                seen.insert(oid);
                let items: Vec<_> = arr.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                for (key, value) in &items {
                    match key {
                        goro_core::array::ArrayKey::Int(n) => {
                            vm.write_output(format!("{}  [{}]=>\n", prefix, n).as_bytes())
                        }
                        goro_core::array::ArrayKey::String(s) => {
                            let k = debug_prop_key(vm, s.as_bytes(), false);
                            vm.write_output(format!("{}  [{}]=>\n", prefix, k).as_bytes())
                        }
                    }
                    var_dump_value(vm, value, indent + 2, seen);
                }
                vm.write_output(format!("{}}}\n", prefix).as_bytes());
                seen.remove(&oid);
                return;
            }
            let mut obj_borrow = obj.borrow();
            // Check if this is an enum case object
            if obj_borrow.has_property(b"__enum_case") {
                let class_name = goro_core::value::display_class_name(&obj_borrow.class_name);
                let case_name = obj_borrow.get_property(b"name");
                let case_name_str = case_name.to_php_string().to_string_lossy();
                vm.write_output(
                    format!("{}enum({}::{})\n", prefix, class_name, case_name_str).as_bytes(),
                );
                return;
            }
            // An object already being dumped prints just *RECURSION*.
            if seen.contains(&obj_borrow.object_id) {
                vm.write_output(format!("{}*RECURSION*\n", prefix).as_bytes());
                return;
            }
            let class_name = goro_core::value::display_class_name(&obj_borrow.class_name);
            let class_lower: Vec<u8> = obj_borrow
                .class_name
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            let oid = obj_borrow.object_id;
            // PHP prints a bare *RECURSION* (no object header) when the
            // object is already being dumped higher up the stack.
            if seen.contains(&oid) {
                vm.write_output(format!("{}*RECURSION*\n", prefix).as_bytes());
                return;
            }

            if obj_borrow.closure.is_some() {
                drop(obj_borrow);
                let props = vm.closure_debug_info(val);
                vm.write_output(
                    format!("{}object(Closure)#{} ({}) {{\n", prefix, oid, props.len()).as_bytes(),
                );
                seen.insert(oid);
                for (k, v) in &props {
                    vm.write_output(
                        format!("{}  [\"{}\"]=>\n", prefix, String::from_utf8_lossy(k)).as_bytes(),
                    );
                    var_dump_value(vm, v, indent + 2, seen);
                }
                vm.write_output(format!("{}}}\n", prefix).as_bytes());
                seen.remove(&oid);
                return;
            }

            // WeakReference: show ["object"] => referent or NULL
            if class_lower == b"weakreference" {
                let weak_id = obj_borrow.get_property(b"__weak_ref_id").to_long() as u64;
                drop(obj_borrow);
                let referent = if let Some(weak) = vm.weak_refs.get(&weak_id) {
                    weak.upgrade().map(Value::Object)
                } else {
                    None
                };
                vm.write_output(
                    format!("{}object(WeakReference)#{} (1) {{\n", prefix, oid).as_bytes(),
                );
                if !seen.insert(oid) {
                    vm.write_output(format!("{}  *RECURSION*\n", prefix).as_bytes());
                    vm.write_output(format!("{}}}\n", prefix).as_bytes());
                    return;
                }
                vm.write_output(format!("{}  [\"object\"]=>\n", prefix).as_bytes());
                if let Some(ref_val) = referent {
                    var_dump_value(vm, &ref_val, indent + 2, seen);
                } else {
                    vm.write_output(format!("{}  NULL\n", prefix).as_bytes());
                }
                vm.write_output(format!("{}}}\n", prefix).as_bytes());
                seen.remove(&oid);
                return;
            }

            // WeakMap: show entries as indexed array of ["key"] => obj, ["value"] => val
            if class_lower == b"weakmap" {
                drop(obj_borrow);
                // Get live entries from weak_map_entries
                let entries: Vec<(Value, Value)> =
                    if let Some(wm_entries) = vm.weak_map_entries.get(&oid) {
                        wm_entries
                            .iter()
                            .filter_map(|(_, weak, val)| {
                                weak.upgrade()
                                    .map(|strong| (Value::Object(strong), val.clone()))
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };
                let count = entries.len();
                vm.write_output(
                    format!("{}object(WeakMap)#{} ({}) {{\n", prefix, oid, count).as_bytes(),
                );
                if !seen.insert(oid) {
                    vm.write_output(format!("{}  *RECURSION*\n", prefix).as_bytes());
                    vm.write_output(format!("{}}}\n", prefix).as_bytes());
                    return;
                }
                for (idx, (key, value)) in entries.iter().enumerate() {
                    vm.write_output(format!("{}  [{}]=>\n", prefix, idx).as_bytes());
                    vm.write_output(format!("{}  array(2) {{\n", prefix).as_bytes());
                    vm.write_output(format!("{}    [\"key\"]=>\n", prefix).as_bytes());
                    var_dump_value(vm, key, indent + 4, seen);
                    vm.write_output(format!("{}    [\"value\"]=>\n", prefix).as_bytes());
                    var_dump_value(vm, value, indent + 4, seen);
                    vm.write_output(format!("{}  }}\n", prefix).as_bytes());
                }
                vm.write_output(format!("{}}}\n", prefix).as_bytes());
                seen.remove(&oid);
                return;
            }

            // User-defined __debugInfo on a subclass of an SPL/built-in class
            // takes precedence over the storage view: PHP calls the override.
            let has_user_debug_info = {
                let class_entry = vm.classes.get(&class_lower).cloned();
                class_entry
                    .as_ref()
                    .and_then(|c| c.get_method(b"__debuginfo"))
                    .is_some()
            };
            if has_user_debug_info {
                // Release our borrow: __debugInfo() may modify the object
                // (e.g. initialize a lazy object).
                drop(obj_borrow);
                let debug_result = debug_info_protected(vm, val, oid, seen);
                obj_borrow = obj.borrow();
                // Lazy prefix reflects the state after __debugInfo() ran.
                let udbg_lazy_prefix = match obj_borrow.get_property(b"__lazy_state") {
                    Value::String(s) if s.as_bytes() == b"ghost" => "lazy ghost ",
                    Value::String(s) if s.as_bytes() == b"proxy" => "lazy proxy ",
                    _ => "",
                };
                let debug_result = match debug_result {
                    Some(Value::Reference(r)) => Some(r.borrow().clone()),
                    other => other,
                };
                // zend_std_get_debug_info(): null is deprecated (an empty
                // table), anything else but an array is fatal.
                let debug_result = match debug_result {
                    Some(Value::Null) if vm.current_exception.is_none() => {
                        let cname = class_name.to_string();
                        drop(obj_borrow);
                        let line = vm.current_line;
                        vm.emit_deprecated_at(
                            &format!(
                                "Returning null from {}::__debugInfo() is deprecated, return an empty array instead",
                                cname
                            ),
                            line,
                        );
                        obj_borrow = obj.borrow();
                        Some(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(
                            goro_core::array::PhpArray::new(),
                        ))))
                    }
                    Some(Value::Array(a)) => Some(Value::Array(a)),
                    Some(_) if vm.current_exception.is_none() => {
                        drop(obj_borrow);
                        let line = vm.current_line;
                        vm.emit_fatal_at("__debuginfo() must return an array", line);
                        vm.fatal_bailout = true;
                        return;
                    }
                    other => other,
                };
                if let Some(Value::Array(debug_arr)) = debug_result {
                    let arr_borrow = debug_arr.borrow();
                    let count = arr_borrow.len();
                    let class_name_owned = class_name.to_string();
                    drop(obj_borrow);
                    vm.write_output(
                        format!(
                            "{}{}object({})#{} ({}) {{\n",
                            prefix, udbg_lazy_prefix, class_name_owned, oid, count
                        )
                        .as_bytes(),
                    );
                    if !seen.insert(oid) {
                        vm.write_output(format!("{}  *RECURSION*\n", prefix).as_bytes());
                        vm.write_output(format!("{}}}\n", prefix).as_bytes());
                        return;
                    }
                    let items: Vec<_> = arr_borrow
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    drop(arr_borrow);
                    for (key, value) in &items {
                        match key {
                            goro_core::array::ArrayKey::Int(n) => {
                                vm.write_output(format!("{}  [{}]=>\n", prefix, n).as_bytes());
                            }
                            goro_core::array::ArrayKey::String(s) => {
                                write_mangled_dump_key(vm, &prefix, s.as_bytes());
                            }
                        }
                        var_dump_value(vm, value, indent + 2, seen);
                    }
                    vm.write_output(format!("{}}}\n", prefix).as_bytes());
                    seen.remove(&oid);
                    return;
                }
                // __debugInfo returned non-array or null - fall through to
                // normal display.
            }
            // SPL classes with a native debug-info handler (heaps, ...):
            // render the returned property table with mangled names.
            if let Some(table) = vm.spl_debug_info(obj) {
                let class_name_owned = class_name.to_string();
                drop(obj_borrow);
                var_dump_mangled_table(vm, &class_name_owned, oid, &table, indent, seen);
                return;
            }
            // Check if this is an SPL class with __spl_array - display array contents instead
            let is_spl_array_class = matches!(class_lower.as_slice(), b"splfixedarray");
            // SplDoublyLinkedList/SplStack/SplQueue have special var_dump format
            let is_spl_dll_class = matches!(
                class_lower.as_slice(),
                b"spldoublylinkedlist" | b"splstack" | b"splqueue"
            );
            // ArrayObject/ArrayIterator use a private "storage" property format.
            // Subclasses (e.g. `class Name extends ArrayObject`) inherit the
            // same storage view — find the closest built-in ancestor.
            let array_object_class_lower: Option<&'static [u8]> = if matches!(
                class_lower.as_slice(),
                b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator"
            ) {
                match class_lower.as_slice() {
                    b"arrayobject" => Some(b"ArrayObject"),
                    b"arrayiterator" => Some(b"ArrayIterator"),
                    b"recursivearrayiterator" => Some(b"ArrayIterator"),
                    _ => None,
                }
            } else if vm.class_extends(&class_lower, b"arrayobject") {
                Some(b"ArrayObject")
            } else if vm.class_extends(&class_lower, b"arrayiterator") {
                Some(b"ArrayIterator")
            } else {
                None
            };
            let is_array_object_class = array_object_class_lower.is_some();
            // SplObjectStorage uses a private "storage" property containing an
            // indexed array of {obj, inf} entries.
            let is_spl_obj_storage = class_lower.as_slice() == b"splobjectstorage";

            if is_spl_obj_storage {
                let objs_val = obj_borrow.get_property(b"__spl_objects");
                let infos_val = obj_borrow.get_property(b"__spl_array");
                let class_name_owned = class_name.to_string();
                drop(obj_borrow);
                // PHP suppresses the object header when this object is already
                // being walked: just emit *RECURSION*.
                if !seen.insert(oid) {
                    vm.write_output(format!("{}*RECURSION*\n", prefix).as_bytes());
                    return;
                }
                vm.write_output(
                    format!("{}object({})#{} (1) {{\n", prefix, class_name_owned, oid).as_bytes(),
                );
                vm.write_output(
                    format!("{}  [\"storage\":\"SplObjectStorage\":private]=>\n", prefix)
                        .as_bytes(),
                );
                // Build the storage array view: [0 => {"obj" => X, "inf" => Y}, ...]
                let mut storage_view = goro_core::array::PhpArray::new();
                if let (Value::Array(objs), Value::Array(infos)) = (&objs_val, &infos_val) {
                    let ob = objs.borrow();
                    let inb = infos.borrow();
                    for (idx, ((_, o), (_, inf))) in ob.iter().zip(inb.iter()).enumerate() {
                        let mut entry = goro_core::array::PhpArray::new();
                        entry.set(
                            goro_core::array::ArrayKey::String(
                                goro_core::string::PhpString::from_bytes(b"obj"),
                            ),
                            o.clone(),
                        );
                        entry.set(
                            goro_core::array::ArrayKey::String(
                                goro_core::string::PhpString::from_bytes(b"inf"),
                            ),
                            inf.clone(),
                        );
                        storage_view.set(
                            goro_core::array::ArrayKey::Int(idx as i64),
                            Value::Array(std::rc::Rc::new(std::cell::RefCell::new(entry))),
                        );
                    }
                }
                let view_val =
                    Value::Array(std::rc::Rc::new(std::cell::RefCell::new(storage_view)));
                var_dump_value(vm, &view_val, indent + 2, seen);
                vm.write_output(format!("{}}}\n", prefix).as_bytes());
                seen.remove(&oid);
                return;
            }

            // ReflectionAttribute: only show "name" property in var_dump
            if class_lower == b"reflectionattribute" {
                let name_val = obj_borrow.get_property(b"name");
                let class_name_owned = class_name.to_string();
                drop(obj_borrow);
                vm.write_output(
                    format!("{}object({})#{} (1) {{\n", prefix, class_name_owned, oid).as_bytes(),
                );
                if !seen.insert(oid) {
                    vm.write_output(format!("{}  *RECURSION*\n", prefix).as_bytes());
                    vm.write_output(format!("{}}}\n", prefix).as_bytes());
                    return;
                }
                vm.write_output(format!("{}  [\"name\"]=>\n", prefix).as_bytes());
                var_dump_value(vm, &name_val, indent + 2, seen);
                vm.write_output(format!("{}}}\n", prefix).as_bytes());
                seen.remove(&oid);
                return;
            }

            if is_array_object_class {
                let spl_arr = obj_borrow.get_property(b"__spl_array");
                let flags = obj_borrow.get_property(b"__spl_flags");
                let flags_val = if let Value::Long(f) = flags { f } else { 0 };
                let _std_prop_list = (flags_val & 1) != 0;
                let class_name_owned = class_name.to_string();
                // Count visible properties (non-internal)
                let extra_props: Vec<_> = obj_borrow
                    .properties
                    .iter()
                    .filter(|(name, val)| {
                        !is_internal_property(name) && !matches!(val, Value::Undef)
                    })
                    .map(|(n, v)| (n.clone(), v.clone()))
                    .collect();
                let prop_count = 1 + extra_props.len(); // storage + any extra properties
                drop(obj_borrow);
                vm.write_output(
                    format!(
                        "{}object({})#{} ({}) {{\n",
                        prefix, class_name_owned, oid, prop_count
                    )
                    .as_bytes(),
                );
                if !seen.insert(oid) {
                    vm.write_output(format!("{}  *RECURSION*\n", prefix).as_bytes());
                    vm.write_output(format!("{}}}\n", prefix).as_bytes());
                    return;
                }
                // Show extra (user) properties first
                for (name, value) in &extra_props {
                    let name_str = String::from_utf8_lossy(name);
                    vm.write_output(format!("{}  [\"{}\"]=>\n", prefix, name_str).as_bytes());
                    var_dump_value(vm, value, indent + 2, seen);
                }
                // Show private storage property — declaring class is the
                // closest built-in ArrayObject/ArrayIterator ancestor, not
                // the actual subclass.
                let storage_decl =
                    String::from_utf8_lossy(array_object_class_lower.unwrap_or(b"ArrayObject"))
                        .to_string();
                vm.write_output(
                    format!("{}  [\"storage\":\"{}\":private]=>\n", prefix, storage_decl)
                        .as_bytes(),
                );
                var_dump_value(vm, &spl_arr, indent + 2, seen);
                vm.write_output(format!("{}}}\n", prefix).as_bytes());
                seen.remove(&oid);
                return;
            }

            if is_spl_dll_class {
                // SplDoublyLinkedList/SplStack/SplQueue: show flags and dllist private properties
                let spl_arr = goro_core::spl_dll::spl_storage_view(&obj_borrow);
                let iter_mode = obj_borrow.get_property(b"__spl_iter_mode");
                let flags_val = if let Value::Long(f) = iter_mode {
                    f
                } else {
                    match class_lower.as_slice() {
                        b"splstack" => 6,
                        b"splqueue" => 4,
                        _ => 0,
                    }
                };
                let class_name_owned = class_name.to_string();
                drop(obj_borrow);
                vm.write_output(
                    format!("{}object({})#{} (2) {{\n", prefix, class_name_owned, oid).as_bytes(),
                );
                if !seen.insert(oid) {
                    vm.write_output(format!("{}  *RECURSION*\n", prefix).as_bytes());
                    vm.write_output(format!("{}}}\n", prefix).as_bytes());
                    return;
                }
                vm.write_output(
                    format!(
                        "{}  [\"flags\":\"SplDoublyLinkedList\":private]=>\n",
                        prefix
                    )
                    .as_bytes(),
                );
                var_dump_value(vm, &Value::Long(flags_val), indent + 2, seen);
                vm.write_output(
                    format!(
                        "{}  [\"dllist\":\"SplDoublyLinkedList\":private]=>\n",
                        prefix
                    )
                    .as_bytes(),
                );
                var_dump_value(vm, &spl_arr, indent + 2, seen);
                vm.write_output(format!("{}}}\n", prefix).as_bytes());
                seen.remove(&oid);
                return;
            }

            if is_spl_array_class {
                let spl_arr = obj_borrow.get_property(b"__spl_array");
                if let Value::Array(a) = spl_arr {
                    // Clone to avoid borrow issues
                    let arr_clone = a.borrow().clone();
                    let count = arr_clone.len();
                    let class_name_owned = class_name.to_string();
                    drop(obj_borrow);
                    vm.write_output(
                        format!(
                            "{}object({})#{} ({}) {{\n",
                            prefix, class_name_owned, oid, count
                        )
                        .as_bytes(),
                    );
                    if !seen.insert(oid) {
                        vm.write_output(format!("{}  *RECURSION*\n", prefix).as_bytes());
                        vm.write_output(format!("{}}}\n", prefix).as_bytes());
                        return;
                    }
                    for (key, value) in arr_clone.iter() {
                        match key {
                            goro_core::array::ArrayKey::Int(n) => {
                                vm.write_output(format!("{}  [{}]=>\n", prefix, n).as_bytes());
                            }
                            goro_core::array::ArrayKey::String(s) => {
                                vm.write_output(
                                    &[prefix.as_bytes(), b"  [\"", s.as_bytes(), b"\"]=>\n"]
                                        .concat(),
                                );
                            }
                        }
                        var_dump_value(vm, value, indent + 2, seen);
                    }
                    vm.write_output(format!("{}}}\n", prefix).as_bytes());
                    seen.remove(&oid);
                    return;
                }
            }

            // Check if class has __debugInfo method (user-defined or SPL built-in)
            let has_debug_info = {
                let class_entry = vm.classes.get(&class_lower).cloned();
                class_entry
                    .as_ref()
                    .and_then(|c| c.get_method(b"__debuginfo"))
                    .is_some()
            };
            // Also check SPL built-in __debugInfo
            let is_spl_debug = !has_debug_info
                && matches!(
                    class_lower.as_slice(),
                    b"splfileinfo"
                        | b"splfileobject"
                        | b"spltempfileobject"
                        | b"directoryiterator"
                        | b"filesystemiterator"
                        | b"recursivedirectoryiterator"
                        | b"globiterator"
                        | b"random\\engine\\mt19937"
                        | b"random\\engine\\xoshiro256starstar"
                        | b"random\\engine\\pcgoneseq128xslrr64"
                );

            if has_debug_info || is_spl_debug {
                // Call __debugInfo() and use the returned array for display.
                // Use call_object_method so the class scope / called-class stacks
                // are pushed correctly — without them, the method body cannot
                // access private/protected properties of $this.
                // Release our borrow: __debugInfo() may modify the object
                // (e.g. initialize a lazy object).
                drop(obj_borrow);
                let debug_result = debug_info_protected(vm, val, oid, seen);
                obj_borrow = obj.borrow();
                // Lazy prefix reflects the state after __debugInfo() ran.
                let dbg_lazy_prefix = match obj_borrow.get_property(b"__lazy_state") {
                    Value::String(s) if s.as_bytes() == b"ghost" => "lazy ghost ",
                    Value::String(s) if s.as_bytes() == b"proxy" => "lazy proxy ",
                    _ => "",
                };

                // Unwrap a by-reference return — var_dump dereferences before
                // walking the array, mirroring PHP's behaviour for
                // `&__debugInfo()`.
                let debug_result = match debug_result {
                    Some(Value::Reference(r)) => Some(r.borrow().clone()),
                    other => other,
                };
                if let Some(Value::Array(debug_arr)) = debug_result {
                    let arr_borrow = debug_arr.borrow();
                    let count = arr_borrow.len();
                    let class_name_owned = class_name.to_string();
                    drop(obj_borrow);
                    vm.write_output(
                        format!(
                            "{}{}object({})#{} ({}) {{\n",
                            prefix, dbg_lazy_prefix, class_name_owned, oid, count
                        )
                        .as_bytes(),
                    );
                    if !seen.insert(oid) {
                        vm.write_output(format!("{}  *RECURSION*\n", prefix).as_bytes());
                        vm.write_output(format!("{}}}\n", prefix).as_bytes());
                        return;
                    }
                    let items: Vec<_> = arr_borrow
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    drop(arr_borrow);
                    for (key, value) in &items {
                        match key {
                            goro_core::array::ArrayKey::Int(n) => {
                                vm.write_output(format!("{}  [{}]=>\n", prefix, n).as_bytes());
                            }
                            goro_core::array::ArrayKey::String(s) => {
                                // Mangled property names from __debugInfo:
                                //   "\0ClassName\0propname" -> private declared by ClassName
                                //   "\0*\0propname"          -> protected
                                //   "propname"                -> public
                                let bytes = s.as_bytes();
                                if bytes.first() == Some(&0u8) {
                                    if let Some(end) = bytes[1..].iter().position(|&b| b == 0u8) {
                                        let cls = &bytes[1..1 + end];
                                        let name = &bytes[2 + end..];
                                        let name_str = String::from_utf8_lossy(name);
                                        if cls == b"*" {
                                            vm.write_output(
                                                format!(
                                                    "{}  [\"{}\":protected]=>\n",
                                                    prefix, name_str
                                                )
                                                .as_bytes(),
                                            );
                                        } else {
                                            let cls_str = String::from_utf8_lossy(cls);
                                            vm.write_output(
                                                format!(
                                                    "{}  [\"{}\":\"{}\":private]=>\n",
                                                    prefix, name_str, cls_str
                                                )
                                                .as_bytes(),
                                            );
                                        }
                                    } else {
                                        vm.write_output(
                                            &[
                                                prefix.as_bytes(),
                                                b"  [\"",
                                                s.as_bytes(),
                                                b"\"]=>\n",
                                            ]
                                            .concat(),
                                        );
                                    }
                                } else {
                                    vm.write_output(
                                        &[prefix.as_bytes(), b"  [\"", s.as_bytes(), b"\"]=>\n"]
                                            .concat(),
                                    );
                                }
                            }
                        }
                        var_dump_value(vm, value, indent + 2, seen);
                    }
                    vm.write_output(format!("{}}}\n", prefix).as_bytes());
                    seen.remove(&oid);
                    return;
                }
                // If __debugInfo didn't return an array, fall through to normal display
            }

            // The property table (zend_get_properties_for(DEBUG)).
            drop(obj_borrow);
            let table: Vec<(Vec<u8>, Value, Option<String>)> = vm
                .object_debug_table(obj)
                .into_iter()
                .filter(|(k, _, _)| !is_internal_property(k))
                .collect();
            obj_borrow = obj.borrow();
            // Uninitialized typed properties are shown but not counted.
            let prop_count = table
                .iter()
                .filter(|(_, v, _)| !matches!(v, Value::Undef))
                .count()
                + vm.stale_prop_count(obj);
            // Lazy object prefix: "lazy ghost " / "lazy proxy " before object(...)
            let lazy_prefix: &str = match obj_borrow.get_property(b"__lazy_state") {
                Value::String(s) if s.as_bytes() == b"ghost" => "lazy ghost ",
                Value::String(s) if s.as_bytes() == b"proxy" => "lazy proxy ",
                _ => "",
            };

            // Initialized proxy: show as `lazy proxy object(C)#N (1) { ["instance"]=> <real> }`
            if lazy_prefix == "lazy proxy " {
                if let Value::Object(real) = obj_borrow.get_property(b"__lazy_real") {
                    drop(obj_borrow);
                    vm.write_output(
                        format!(
                            "{}lazy proxy object({})#{} (1) {{\n",
                            prefix, class_name, oid
                        )
                        .as_bytes(),
                    );
                    if !seen.insert(oid) {
                        vm.write_output(format!("{}  *RECURSION*\n", prefix).as_bytes());
                        vm.write_output(format!("{}}}\n", prefix).as_bytes());
                        return;
                    }
                    vm.write_output(format!("{}  [\"instance\"]=>\n", prefix).as_bytes());
                    // The instance is shown from a temporary table (which
                    // holds it in PHP too): no dumper copy to discount.
                    ZVAL_BIAS.with(|b| b.set(0));
                    var_dump_value(vm, &Value::Object(real), indent + 2, seen);
                    vm.write_output(format!("{}}}\n", prefix).as_bytes());
                    seen.remove(&oid);
                    return;
                }
            }

            let header = format!(
                "{}{}object({})#{} ({}) {{\n",
                prefix, lazy_prefix, class_name, oid, prop_count
            );
            vm.write_output(&restore_raw_name(header, &obj_borrow.class_name));

            // Check for recursion (self-referencing objects)
            if !seen.insert(oid) {
                // Already seen this object — print *RECURSION*
                vm.write_output(format!("{}  *RECURSION*\n", prefix).as_bytes());
                vm.write_output(format!("{}}}\n", prefix).as_bytes());
                return;
            }
            drop(obj_borrow);
            for (key, value, uninit_type) in table {
                // Unshared references (held only by this slot) print
                // without `&`.
                let value = match value {
                    Value::Reference(r) if std::rc::Rc::strong_count(&r) == 2 => r.borrow().clone(),
                    v => v,
                };
                let display_name = debug_prop_key(vm, &key, class_lower.as_slice() == b"gmp");
                // A non-UTF-8 name is printed verbatim.
                let bare = match key.iter().rposition(|&c| c == 0) {
                    Some(p) => &key[p + 1..],
                    None => &key[..],
                };
                let line = restore_raw_name(format!("{}  [{}]=>\n", prefix, display_name), bare);
                if let Some(t) = uninit_type {
                    vm.write_output(&line);
                    vm.write_output(format!("{}  uninitialized({})\n", prefix, t).as_bytes());
                    continue;
                }
                // php_object_property_dump(): ZEND_CHECK_STACK_LIMIT
                if !goro_core::stack::enter_native(VAR_DUMP_FRAME_COST) {
                    vm.write_output(format!("{}  nesting level too deep", prefix).as_bytes());
                    continue;
                }
                vm.write_output(&line);
                // (debug_zval_dump(): `table` holds the value once more)
                ZVAL_BIAS.with(|b| b.set(1));
                var_dump_value(vm, &value, indent + 2, seen);
                ZVAL_BIAS.with(|b| b.set(0));
                goro_core::stack::leave_native(VAR_DUMP_FRAME_COST);
            }
            vm.write_output(format!("{}}}\n", prefix).as_bytes());
            seen.remove(&oid);
        }
        Value::Generator(generator) => {
            let name_str = vm.generator_function_name(generator);
            let prop_count = 1; // function property
            let gen_id = vm.generator_handle(generator);
            vm.write_output(
                format!(
                    "{}object(Generator)#{} ({}) {{\n",
                    prefix, gen_id, prop_count
                )
                .as_bytes(),
            );
            vm.write_output(format!("{}  [\"function\"]=>\n", prefix).as_bytes());
            vm.write_output(
                format!("{}  string({}) \"{}\"\n", prefix, name_str.len(), name_str).as_bytes(),
            );
            vm.write_output(format!("{}}}\n", prefix).as_bytes());
        }
        Value::Reference(r) => {
            // PHP shows & prefix only when reference is shared (refcount >= 2).
            // After function with by-ref params returns, local refs are dropped,
            // so a single holder (the array element) shouldn't show &.
            let inner = r.borrow().clone();
            if std::rc::Rc::strong_count(r) >= 2 {
                // PHP prefixes the header line with '&' (COMMON); a
                // *RECURSION* marker carries no '&'.
                let pos = vm.active_output_buffer().len() + indent;
                var_dump_value(vm, &inner, indent, seen);
                let buf = vm.active_output_buffer();
                if pos <= buf.len() && !buf[pos..].starts_with(b"*RECURSION*") {
                    buf.insert(pos, b'&');
                }
            } else {
                var_dump_value(vm, &inner, indent, seen);
            }
        }
    }
}

/// Output text naming a class or property whose raw name is not UTF-8: put
/// the raw bytes back in place of their lossy rendering.
fn restore_raw_name(text: String, raw: &[u8]) -> Vec<u8> {
    if std::str::from_utf8(raw).is_ok() {
        return text.into_bytes();
    }
    let lossy = String::from_utf8_lossy(raw);
    match text.find(lossy.as_ref()) {
        Some(p) => [
            &text.as_bytes()[..p],
            raw,
            &text.as_bytes()[p + lossy.len()..],
        ]
        .concat(),
        None => text.into_bytes(),
    }
}

/// zend_wrong_parameters_count_error(): an ArgumentCountError.
fn arg_count_error(vm: &mut Vm, name: &str, min: usize, max: usize, given: usize) -> VmError {
    let (which, n) = if given < min {
        (if min == max { "exactly" } else { "at least" }, min)
    } else {
        (if min == max { "exactly" } else { "at most" }, max)
    };
    let msg = format!(
        "{}() expects {} {} argument{}, {} given",
        name,
        which,
        n,
        if n == 1 { "" } else { "s" },
        given
    );
    let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError {
        message: format!("Uncaught ArgumentCountError: {}", msg),
        line: vm.current_line,
    }
}

fn print_r(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() || args.len() > 2 {
        return Err(arg_count_error(vm, "print_r", 1, 2, args.len()));
    }

    let return_output = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    // Clone classes for property visibility lookup in print_r_value
    let classes: goro_core::vm::ClassMap = vm.classes.clone();
    // Snapshot closure metadata so print_r can render closure values like
    // PHP does (with name/file/line/parameters), without holding a vm borrow.
    let closure_info_table: std::collections::HashMap<Vec<u8>, ClosurePrintInfo> = vm
        .user_functions
        .iter()
        .filter_map(|(k, op)| {
            let name = if op.name.starts_with(b"__closure_fcc_") {
                "{closure}".to_string()
            } else if !op.display_name.is_empty() {
                String::from_utf8_lossy(&op.display_name).to_string()
            } else {
                return None;
            };
            let file = String::from_utf8_lossy(&op.filename).to_string();
            let line = op.decl_line;
            let mut params = Vec::with_capacity(op.param_count as usize);
            for i in 0..op.param_count as usize {
                if let Some(cv_name) = op.cv_names.get(i) {
                    let pname = format!("${}", String::from_utf8_lossy(cv_name));
                    let has_default = op.param_has_default.get(i).copied().unwrap_or(false);
                    params.push((pname, has_default));
                }
            }
            Some((
                k.clone(),
                ClosurePrintInfo {
                    name,
                    file,
                    line,
                    params,
                },
            ))
        })
        .collect();

    prepare_closure_debug(vm, &args[0]);
    let materialized = materialize_print_r(vm, &args[0], &mut std::collections::HashMap::new());
    let args: &[Value] = match &materialized {
        Some(v) => std::slice::from_ref(v),
        None => args,
    };
    PRINT_R_NANS.with(|n| n.set(0));
    PRINT_R_NOTICES.with(|n| n.borrow_mut().clear());
    let mut buf = Vec::new();
    print_r_value(&args[0], &mut buf, 0, &classes, &closure_info_table);
    for notice in PRINT_R_NOTICES.with(|n| std::mem::take(&mut *n.borrow_mut())) {
        let line = vm.current_line;
        vm.emit_notice_at(notice, line);
    }
    // zend_double_to_str() of each NAN printed
    for _ in 0..PRINT_R_NANS.with(|n| n.replace(0)) {
        vm.emit_warning("unexpected NAN value was coerced to string");
    }
    if return_output {
        Ok(Value::String(PhpString::from_vec(buf)))
    } else {
        vm.write_output(&buf);
        Ok(Value::True)
    }
}

thread_local! {
    /// NAN values print_r() converted (it builds its output without the VM).
    static PRINT_R_NANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// The notices of zend_unmangle_property_name_ex() print_r() raised.
    static PRINT_R_NOTICES: std::cell::RefCell<Vec<&'static str>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

thread_local! {
    /// Debug-info properties of the Closure objects reachable from the value
    /// being printed (object id -> props), computed up front because the
    /// printers run without access to the VM.
    static CLOSURE_DBG: std::cell::RefCell<std::collections::HashMap<u64, Vec<(Vec<u8>, Value)>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Fill CLOSURE_DBG for every Closure object reachable from `v`.
pub(crate) fn prepare_closure_debug(vm: &Vm, v: &Value) {
    fn walk(
        vm: &Vm,
        v: &Value,
        seen: &mut HashSet<usize>,
        out: &mut std::collections::HashMap<u64, Vec<(Vec<u8>, Value)>>,
    ) {
        match v {
            Value::Reference(r) => {
                let inner = r.borrow().clone();
                walk(vm, &inner, seen, out);
            }
            Value::Array(a) => {
                if !seen.insert(std::rc::Rc::as_ptr(a) as *const u8 as usize) {
                    return;
                }
                let vals: Vec<Value> = a.borrow().values().cloned().collect();
                for x in &vals {
                    walk(vm, x, seen, out);
                }
            }
            Value::Object(o) => {
                if !seen.insert(std::rc::Rc::as_ptr(o) as *const u8 as usize) {
                    return;
                }
                let (is_closure, oid) = match o.try_borrow() {
                    Ok(b) => (b.closure.is_some(), b.object_id),
                    Err(_) => return,
                };
                let vals: Vec<Value> = if is_closure {
                    let info = vm.closure_debug_info(v);
                    let vals = info.iter().map(|(_, x)| x.clone()).collect();
                    out.insert(oid, info);
                    vals
                } else {
                    o.borrow()
                        .properties
                        .iter()
                        .map(|(_, x)| x.clone())
                        .collect()
                };
                for x in &vals {
                    walk(vm, x, seen, out);
                }
            }
            _ => {}
        }
    }
    let mut out = std::collections::HashMap::new();
    let mut seen = HashSet::new();
    walk(vm, v, &mut seen, &mut out);
    CLOSURE_DBG.with(|m| *m.borrow_mut() = out);
}

/// Debug-info properties for a Closure object (see prepare_closure_debug).
pub(crate) fn closure_debug_props(oid: u64) -> Vec<(Vec<u8>, Value)> {
    CLOSURE_DBG
        .with(|m| m.borrow().get(&oid).cloned())
        .unwrap_or_default()
}

struct ClosurePrintInfo {
    name: String,
    file: String,
    line: u32,
    params: Vec<(String, bool)>,
}

fn render_closure_print_r(buf: &mut Vec<u8>, indent: usize, info: &ClosurePrintInfo) {
    let prefix = " ".repeat(indent);
    buf.extend_from_slice(b"Closure Object\n");
    buf.extend_from_slice(format!("{}(\n", prefix).as_bytes());
    buf.extend_from_slice(format!("{}    [name] => {}\n", prefix, info.name).as_bytes());
    buf.extend_from_slice(format!("{}    [file] => {}\n", prefix, info.file).as_bytes());
    buf.extend_from_slice(format!("{}    [line] => {}\n", prefix, info.line).as_bytes());
    if !info.params.is_empty() {
        buf.extend_from_slice(format!("{}    [parameter] => Array\n", prefix).as_bytes());
        buf.extend_from_slice(format!("{}        (\n", prefix).as_bytes());
        for (pname, has_default) in &info.params {
            let label = if *has_default {
                "<optional>"
            } else {
                "<required>"
            };
            buf.extend_from_slice(
                format!("{}            [{}] => {}\n", prefix, pname, label).as_bytes(),
            );
        }
        buf.extend_from_slice(format!("{}        )\n\n", prefix).as_bytes());
    }
    buf.extend_from_slice(format!("{})\n", prefix).as_bytes());
}

thread_local! {
    /// Arrays/objects currently being printed by print_r (recursion guard).
    static PRINT_R_SEEN: std::cell::RefCell<HashSet<usize>> = std::cell::RefCell::new(HashSet::new());
}

fn print_r_value(
    val: &Value,
    buf: &mut Vec<u8>,
    indent: usize,
    classes: &goro_core::vm::ClassMap,
    closure_info_table: &std::collections::HashMap<Vec<u8>, ClosurePrintInfo>,
) {
    let key = match val {
        Value::Array(a) => Some((a.borrow().identity() | (1u64 << 63)) as usize),
        Value::Object(o) => {
            if o.try_borrow()
                .map(|b| b.has_property(b"__enum_case"))
                .unwrap_or(true)
            {
                None
            } else {
                Some(std::rc::Rc::as_ptr(o) as *const u8 as usize)
            }
        }
        _ => None,
    };
    let Some(key) = key else {
        return print_r_value_inner(val, buf, indent, classes, closure_info_table);
    };
    if !PRINT_R_SEEN.with(|s| s.borrow_mut().insert(key)) {
        match val {
            Value::Object(o) => {
                let cn = goro_core::value::display_class_name(&o.borrow().class_name).to_string();
                buf.extend_from_slice(cn.as_bytes());
                buf.extend_from_slice(b" Object\n *RECURSION*");
            }
            _ => buf.extend_from_slice(b"Array\n *RECURSION*"),
        }
        return;
    }
    print_r_value_inner(val, buf, indent, classes, closure_info_table);
    PRINT_R_SEEN.with(|s| s.borrow_mut().remove(&key));
}

fn print_r_value_inner(
    val: &Value,
    buf: &mut Vec<u8>,
    indent: usize,
    classes: &goro_core::vm::ClassMap,
    closure_info_table: &std::collections::HashMap<Vec<u8>, ClosurePrintInfo>,
) {
    // (deep but finite nesting is printed; real recursion is detected by
    // PRINT_R_SEEN)
    if goro_core::stack::with_room(|| {
        print_r_value_body(val, buf, indent, classes, closure_info_table)
    })
    .is_err()
    {
        buf.extend_from_slice(b" *RECURSION*");
    }
}

fn print_r_value_body(
    val: &Value,
    buf: &mut Vec<u8>,
    indent: usize,
    classes: &goro_core::vm::ClassMap,
    closure_info_table: &std::collections::HashMap<Vec<u8>, ClosurePrintInfo>,
) {
    match val {
        Value::Null | Value::Undef => buf.extend_from_slice(b""),
        Value::True => buf.extend_from_slice(b"1"),
        Value::False => {}
        Value::Long(n) => {
            buf.extend_from_slice(n.to_string().as_bytes());
        }
        Value::Resource(id) => {
            buf.extend_from_slice(format!("Resource id #{}", id).as_bytes());
        }
        Value::Double(f) => {
            if f.is_nan() {
                PRINT_R_NANS.with(|n| n.set(n.get() + 1));
            }
            buf.extend_from_slice(Value::Double(*f).to_php_string().as_bytes())
        }
        Value::String(s) => {
            let b = s.as_bytes();
            if b.starts_with(b"__closure_")
                || b.starts_with(b"__arrow_")
                || b.starts_with(b"__bound_closure_")
                || b.starts_with(b"__closure_fcc_")
            {
                let lower: Vec<u8> = b.iter().map(|c| c.to_ascii_lowercase()).collect();
                if let Some(info) = closure_info_table.get(&lower) {
                    render_closure_print_r(buf, indent, info);
                } else {
                    buf.extend_from_slice(b"Closure Object\n");
                    let prefix = " ".repeat(indent);
                    buf.extend_from_slice(format!("{}(\n{})\n", prefix, prefix).as_bytes());
                }
            } else {
                buf.extend_from_slice(b);
            }
        }
        Value::Array(arr) => {
            // Closure with captured values (use vars or implicit $this) is
            // stored as `[closure_name, captured1, ...]` — render as a Closure
            // Object, not as a regular array.
            {
                let arr_b = arr.borrow();
                // A closure-with-captures has the form `[name, capture1, ...]`,
                // i.e. at least 2 elements. Require that to avoid misidentifying
                // a regular 1-element args array containing a closure value.
                let len = arr_b.iter().count();
                if len >= 2 {
                    if let Some(Value::String(s)) = arr_b.values().next() {
                        let b = s.as_bytes();
                        if b.starts_with(b"__closure_")
                            || b.starts_with(b"__arrow_")
                            || b.starts_with(b"__bound_closure_")
                            || b.starts_with(b"__closure_fcc_")
                        {
                            let lower: Vec<u8> = b.iter().map(|c| c.to_ascii_lowercase()).collect();
                            if let Some(info) = closure_info_table.get(&lower) {
                                render_closure_print_r(buf, indent, info);
                                return;
                            }
                            let prefix = " ".repeat(indent);
                            buf.extend_from_slice(b"Closure Object\n");
                            buf.extend_from_slice(format!("{}(\n{})\n", prefix, prefix).as_bytes());
                            return;
                        }
                    }
                }
            }
            let _arr_ptr = Rc::as_ptr(arr) as *const u8 as usize;
            buf.extend_from_slice(b"Array\n");
            let arr = arr.borrow();
            let prefix = " ".repeat(indent);
            buf.extend_from_slice(format!("{}(\n", prefix).as_bytes());
            for (key, value) in arr.iter() {
                match key {
                    goro_core::array::ArrayKey::Int(n) => {
                        buf.extend_from_slice(format!("{}    [{}] => ", prefix, n).as_bytes());
                    }
                    goro_core::array::ArrayKey::String(s) => {
                        buf.extend_from_slice(format!("{}    [", prefix).as_bytes());
                        buf.extend_from_slice(s.as_bytes());
                        buf.extend_from_slice(b"] => ");
                    }
                }
                print_r_value(value, buf, indent + 8, classes, closure_info_table);
                buf.push(b'\n');
            }
            buf.extend_from_slice(format!("{})\n", prefix).as_bytes());
        }
        Value::Object(obj) => {
            let obj_borrow = obj.borrow();
            // Check if this is an enum case object
            if obj_borrow.has_property(b"__enum_case") {
                let class_name = goro_core::value::display_class_name(&obj_borrow.class_name);
                let prefix = " ".repeat(indent);
                // Determine backing type from stored property
                let has_backing = obj_borrow.has_property(b"__enum_backing_type");
                let header = if has_backing {
                    let bt = obj_borrow.get_property(b"__enum_backing_type");
                    let bt_str = bt.to_php_string().to_string_lossy();
                    format!("{} Enum:{}\n", class_name, bt_str)
                } else {
                    format!("{} Enum\n", class_name)
                };
                buf.extend_from_slice(header.as_bytes());
                buf.extend_from_slice(format!("{}(\n", prefix).as_bytes());
                let case_name = obj_borrow.get_property(b"name");
                buf.extend_from_slice(format!("{}    [name] => ", prefix).as_bytes());
                print_r_value(&case_name, buf, indent + 8, classes, closure_info_table);
                buf.push(b'\n');
                if has_backing {
                    let case_value = obj_borrow.get_property(b"value");
                    buf.extend_from_slice(format!("{}    [value] => ", prefix).as_bytes());
                    print_r_value(&case_value, buf, indent + 8, classes, closure_info_table);
                    buf.push(b'\n');
                }
                buf.extend_from_slice(format!("{})\n", prefix).as_bytes());
                return;
            }
            if obj_borrow.closure.is_some() {
                let oid = obj_borrow.object_id;
                drop(obj_borrow);
                let prefix = " ".repeat(indent);
                buf.extend_from_slice(b"Closure Object\n");
                buf.extend_from_slice(format!("{}(\n", prefix).as_bytes());
                for (k, v) in closure_debug_props(oid) {
                    buf.extend_from_slice(
                        format!("{}    [{}] => ", prefix, String::from_utf8_lossy(&k)).as_bytes(),
                    );
                    print_r_value(&v, buf, indent + 8, classes, closure_info_table);
                    buf.push(b'\n');
                }
                buf.extend_from_slice(format!("{})\n", prefix).as_bytes());
                return;
            }
            let class_name = goro_core::value::display_class_name(&obj_borrow.class_name);
            let class_lower: Vec<u8> = obj_borrow
                .class_name
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            let prefix = " ".repeat(indent);

            // Check if this is an SPL class with __spl_array
            let is_spl_array_class = matches!(class_lower.as_slice(), b"splfixedarray");
            if is_spl_array_class {
                let spl_arr = obj_borrow.get_property(b"__spl_array");
                if let Value::Array(a) = spl_arr {
                    let arr_clone = a.borrow().clone();
                    drop(obj_borrow);
                    buf.extend_from_slice(format!("{} Object\n", class_name).as_bytes());
                    buf.extend_from_slice(format!("{}(\n", prefix).as_bytes());
                    for (key, value) in arr_clone.iter() {
                        match key {
                            goro_core::array::ArrayKey::Int(n) => {
                                buf.extend_from_slice(
                                    format!("{}    [{}] => ", prefix, n).as_bytes(),
                                );
                            }
                            goro_core::array::ArrayKey::String(s) => {
                                buf.extend_from_slice(
                                    &[prefix.as_bytes(), b"    [", s.as_bytes(), b"] => "].concat(),
                                );
                            }
                        }
                        print_r_value(value, buf, indent + 8, classes, closure_info_table);
                        buf.push(b'\n');
                    }
                    buf.extend_from_slice(format!("{})\n", prefix).as_bytes());
                    return;
                }
            }

            // A property table materialized for print_r: keys as in the
            // table (mangled names carry the visibility).
            if is_print_r_shadow(&obj_borrow) {
                buf.extend_from_slice(format!("{} Object\n", class_name).as_bytes());
                buf.extend_from_slice(format!("{}(\n", prefix).as_bytes());
                for (name, value) in &obj_borrow.properties {
                    buf.extend_from_slice(format!("{}    [", prefix).as_bytes());
                    print_r_key(buf, name);
                    buf.extend_from_slice(b"] => ");
                    print_r_value(value, buf, indent + 8, classes, closure_info_table);
                    buf.push(b'\n');
                }
                buf.extend_from_slice(format!("{})\n", prefix).as_bytes());
                return;
            }

            // Look up class for property visibility
            let class_info = classes.get(&class_lower);

            buf.extend_from_slice(format!("{} Object\n", class_name).as_bytes());
            let _obj_ptr = Rc::as_ptr(obj) as *const u8 as usize;
            buf.extend_from_slice(format!("{}(\n", prefix).as_bytes());
            for (name, value) in &obj_borrow.properties {
                // Skip internal SPL/Reflection properties and uninitialized properties
                if is_internal_property(name) || matches!(value, Value::Undef) {
                    continue;
                }
                // Demangle private/protected storage keys (\0Class\0name).
                let (bare_name, mangled_decl): (Vec<u8>, Option<Vec<u8>>) =
                    if name.first() == Some(&0) {
                        if let Some(sep) = name[1..].iter().position(|&b| b == 0) {
                            (
                                name[1 + sep + 1..].to_vec(),
                                Some(name[1..1 + sep].to_vec()),
                            )
                        } else {
                            (name.clone(), None)
                        }
                    } else {
                        (name.clone(), None)
                    };
                let name_str = String::from_utf8_lossy(&bare_name);
                // Determine visibility and declaring class from class definition.
                // Private properties always print with their declaring class —
                // PHP shows `[name:Declaring:private]`, NOT the current class.
                // Instance properties are non-static, so prefer a non-static def.
                let prop_info = class_info.and_then(|c| {
                    let decl_lower = mangled_decl.as_ref().map(|d| {
                        d.iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect::<Vec<u8>>()
                    });
                    c.properties
                        .iter()
                        .filter(|p| p.name == bare_name && !p.is_static)
                        .find(|p| match &decl_lower {
                            Some(dl) => {
                                let pdl: Vec<u8> = p
                                    .declaring_class
                                    .iter()
                                    .map(|b| b.to_ascii_lowercase())
                                    .collect();
                                pdl == *dl
                            }
                            None => true,
                        })
                        .or_else(|| {
                            c.properties
                                .iter()
                                .find(|p| p.name == bare_name && !p.is_static)
                        })
                        .map(|p| (p.visibility, p.declaring_class.clone()))
                });
                let display_name = match prop_info {
                    Some((goro_core::object::Visibility::Protected, _)) => {
                        format!("{}:protected", name_str)
                    }
                    Some((goro_core::object::Visibility::Private, decl)) => {
                        let decl_raw = mangled_decl
                            .clone()
                            .or(if decl.is_empty() { None } else { Some(decl) })
                            .unwrap_or_else(|| class_name.clone().into_bytes());
                        let decl_lower: Vec<u8> =
                            decl_raw.iter().map(|b| b.to_ascii_lowercase()).collect();
                        let canonical = classes
                            .get(&decl_lower)
                            .map(|c| c.name.clone())
                            .unwrap_or(decl_raw);
                        let decl_disp = goro_core::value::display_class_name(&canonical);
                        format!("{}:{}:private", name_str, decl_disp)
                    }
                    _ => format!("{}", name_str),
                };
                buf.extend_from_slice(format!("{}    [{}] => ", prefix, display_name).as_bytes());
                print_r_value(value, buf, indent + 8, classes, closure_info_table);
                buf.push(b'\n');
            }
            buf.extend_from_slice(format!("{})\n", prefix).as_bytes());
        }
        Value::Generator(_) => {
            buf.extend_from_slice(b"Generator Object\n(\n)\n");
        }
        Value::Reference(r) => {
            print_r_value(&r.borrow(), buf, indent, classes, closure_info_table);
        }
    }
}

fn var_export(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() || args.len() > 2 {
        return Err(arg_count_error(vm, "var_export", 1, 2, args.len()));
    }
    let return_output = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    // For lazy objects, trigger init and redirect proxy to real first.
    let original = args[0].clone();
    let val_owned: Value = if let Value::Object(obj) = original.deref() {
        let is_lazy = matches!(
            obj.borrow().get_property(b"__lazy_state"),
            Value::String(ref s) if s.as_bytes() == b"ghost" || s.as_bytes() == b"proxy"
        );
        if is_lazy {
            vm.maybe_run_lazy_init(&obj);
            if vm.current_exception.is_some() {
                return Err(VmError {
                    message: "Uncaught exception".to_string(),
                    line: 0,
                });
            }
            Value::Object(vm.proxy_redirect(&obj))
        } else {
            original
        }
    } else {
        original
    };
    // Objects with hooked properties export the values of their get hooks
    // (zend_hooked_object_build_properties()).
    let val_owned: Value = match &val_owned {
        Value::Object(obj) => {
            let class_lower: Vec<u8> = obj
                .borrow()
                .class_name
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            if vm.class_has_hooked_props(&class_lower) {
                let props = vm.hooked_object_build_properties(obj)?;
                let (cn, id) = {
                    let ob = obj.borrow();
                    (ob.class_name.clone(), ob.object_id)
                };
                let mut tmp = goro_core::object::PhpObject::new(cn, id);
                for (k, v) in props {
                    tmp.set_property(k, v);
                }
                Value::Object(std::rc::Rc::new(std::cell::RefCell::new(tmp)))
            } else {
                val_owned.clone()
            }
        }
        _ => val_owned,
    };
    let mut buf = Vec::new();
    var_export_value(vm, &val_owned, &mut buf, 1);

    if return_output {
        Ok(Value::String(PhpString::from_vec(buf)))
    } else {
        vm.write_output(&buf);
        Ok(Value::Null)
    }
}

thread_local! {
    /// Arrays/objects currently being exported (recursion guard).
    static VAR_EXPORT_STACK: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn var_export_spaces(buf: &mut Vec<u8>, n: usize) {
    buf.extend(std::iter::repeat_n(b' ', n));
}

/// php_addcslashes(s, "'\\") + "\0" → `' . "\0" . '`
fn var_export_escape(buf: &mut Vec<u8>, s: &[u8], nul_concat: bool) {
    for &byte in s {
        match byte {
            b'\0' if nul_concat => buf.extend_from_slice(b"' . \"\\0\" . '"),
            b'\'' => buf.extend_from_slice(b"\\'"),
            b'\\' => buf.extend_from_slice(b"\\\\"),
            _ => buf.push(byte),
        }
    }
}

/// Port of php_var_export_ex().
fn var_export_value(vm: &mut Vm, val: &Value, buf: &mut Vec<u8>, level: usize) {
    match val {
        Value::Null | Value::Undef | Value::Resource(_) | Value::Generator(_) => {
            buf.extend_from_slice(b"NULL")
        }
        Value::True => buf.extend_from_slice(b"true"),
        Value::False => buf.extend_from_slice(b"false"),
        Value::Long(n) => {
            if *n == i64::MIN {
                buf.extend_from_slice(b"-9223372036854775807-1");
            } else {
                buf.extend_from_slice(n.to_string().as_bytes());
            }
        }
        Value::Double(f) => {
            let s = crate::serialize::format_serialize_double(*f);
            buf.extend_from_slice(s.as_bytes());
            if f.is_finite() && !s.contains('.') {
                buf.extend_from_slice(b".0");
            }
        }
        Value::String(s) => {
            let b = s.as_bytes();
            if (b.starts_with(b"__closure_")
                || b.starts_with(b"__arrow_")
                || b.starts_with(b"__bound_closure_"))
                && vm.user_functions.contains_key(&b.to_ascii_lowercase())
            {
                // Closures are objects without __set_state support.
                buf.extend_from_slice(b"\\Closure::__set_state(array(\n");
                if level > 1 {
                    var_export_spaces(buf, level - 1);
                }
                buf.extend_from_slice(b"))");
                return;
            }
            buf.push(b'\'');
            var_export_escape(buf, b, true);
            buf.push(b'\'');
        }
        Value::Reference(r) => {
            let inner = r.borrow().clone();
            var_export_value(vm, &inner, buf, level);
        }
        Value::Array(arr) => {
            let ptr = (arr.borrow().identity() | (1u64 << 63)) as usize;
            if VAR_EXPORT_STACK.with(|st| st.borrow().contains(&ptr)) {
                buf.extend_from_slice(b"NULL");
                vm.emit_warning("var_export does not handle circular references");
                return;
            }
            VAR_EXPORT_STACK.with(|st| st.borrow_mut().push(ptr));
            if level > 1 {
                buf.push(b'\n');
                var_export_spaces(buf, level - 1);
            }
            buf.extend_from_slice(b"array (\n");
            let items: Vec<(goro_core::array::ArrayKey, Value)> = arr
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (key, value) in &items {
                var_export_spaces(buf, level + 1);
                match key {
                    goro_core::array::ArrayKey::Int(n) => {
                        buf.extend_from_slice(n.to_string().as_bytes());
                        buf.extend_from_slice(b" => ");
                    }
                    goro_core::array::ArrayKey::String(s) => {
                        buf.push(b'\'');
                        var_export_escape(buf, s.as_bytes(), true);
                        buf.extend_from_slice(b"' => ");
                    }
                }
                var_export_value(vm, value, buf, level + 2);
                buf.extend_from_slice(b",\n");
            }
            VAR_EXPORT_STACK.with(|st| {
                st.borrow_mut().pop();
            });
            if level > 1 {
                var_export_spaces(buf, level - 1);
            }
            buf.push(b')');
        }
        Value::Object(obj) => {
            let ptr = Rc::as_ptr(obj) as *const u8 as usize;
            if VAR_EXPORT_STACK.with(|st| st.borrow().contains(&ptr)) {
                buf.extend_from_slice(b"NULL");
                vm.emit_warning("var_export does not handle circular references");
                return;
            }
            let (class_name, is_enum, class_lower) = {
                let ob = obj.borrow();
                let cl: Vec<u8> = ob
                    .class_name
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect();
                let name = vm
                    .classes
                    .get(&cl)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| ob.class_name.clone());
                (
                    // (an anonymous class keeps its whole NUL-separated name)
                    if name.contains(&0) {
                        name.clone()
                    } else {
                        goro_core::value::display_class_name(&name).into_bytes()
                    },
                    ob.has_property(b"__enum_case"),
                    cl,
                )
            };
            // Properties as exposed for var_export (zend_get_properties_for).
            let mut is_fixed_array = false;
            let is_array_object = {
                let mut c = class_lower.clone();
                let mut found = false;
                for _ in 0..32 {
                    if matches!(
                        c.as_slice(),
                        b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator"
                    ) {
                        found = true;
                        break;
                    }
                    if c == b"splfixedarray" {
                        is_fixed_array = true;
                        break;
                    }
                    let next = vm
                        .classes
                        .get(&c)
                        .and_then(|ce| ce.parent.clone())
                        .map(|p| {
                            p.iter()
                                .map(|b| b.to_ascii_lowercase())
                                .collect::<Vec<u8>>()
                        })
                        .or_else(|| goro_core::vm::get_builtin_parent(&c).map(|p| p.to_vec()));
                    match next {
                        Some(n) => c = n,
                        None => break,
                    }
                }
                found
            };
            let mut native_int_keys: Vec<Vec<u8>> = Vec::new();
            let native_table = native_props(vm, obj, 1);
            let props: Vec<(Vec<u8>, Value)> = if let Some(arr) = native_table {
                arr.iter()
                    .map(|(k, v)| {
                        let kb = match k {
                            goro_core::array::ArrayKey::Int(n) => {
                                let kb = n.to_string().into_bytes();
                                native_int_keys.push(kb.clone());
                                kb
                            }
                            goro_core::array::ArrayKey::String(s) => s.as_bytes().to_vec(),
                        };
                        (kb, v.clone())
                    })
                    .collect()
            } else if is_fixed_array {
                // get_properties_for(VAR_EXPORT): the elements, then the
                // properties (as for an array cast).
                let arr = vm.object_to_array(obj);
                let arr = arr.borrow();
                arr.iter()
                    .map(|(k, v)| {
                        let kb = match k {
                            goro_core::array::ArrayKey::Int(n) => {
                                let kb = n.to_string().into_bytes();
                                native_int_keys.push(kb.clone());
                                kb
                            }
                            goro_core::array::ArrayKey::String(s) => s.as_bytes().to_vec(),
                        };
                        (kb, v.clone())
                    })
                    .collect()
            } else if is_array_object
                && obj.borrow().get_property(b"__spl_flags").to_long() & 1 == 0
            {
                // ArrayObject without STD_PROP_LIST exports its storage
                // (get_properties_for(VAR_EXPORT): a wrapped object's or
                // ArrayObject's table too).
                vm.object_to_array(obj)
                    .borrow()
                    .iter()
                    .map(|(k, v)| {
                        let kb = match k {
                            goro_core::array::ArrayKey::Int(n) => n.to_string().into_bytes(),
                            goro_core::array::ArrayKey::String(s) => s.as_bytes().to_vec(),
                        };
                        (kb, v.clone())
                    })
                    .collect()
            } else {
                crate::serialize::mangled_props(vm, obj)
                    .into_iter()
                    .filter(|e| !matches!(e.value, Value::Undef))
                    .map(|e| (e.php_key, e.value))
                    .collect()
            };
            VAR_EXPORT_STACK.with(|st| st.borrow_mut().push(ptr));
            if level > 1 {
                buf.push(b'\n');
                var_export_spaces(buf, level - 1);
            }
            let is_std = class_lower == b"stdclass";
            if is_std {
                buf.extend_from_slice(b"(object) array(\n");
            } else {
                buf.push(b'\\');
                buf.extend_from_slice(&class_name);
                if is_enum {
                    let case_name = obj.borrow().get_property(b"name").to_php_string();
                    buf.extend_from_slice(b"::");
                    buf.extend_from_slice(case_name.as_bytes());
                } else {
                    buf.extend_from_slice(b"::__set_state(array(\n");
                }
            }
            if !is_enum {
                for (key, value) in &props {
                    var_export_spaces(buf, level + 2);
                    let name = match goro_core::proptable::unmangle_property_name_ex(key) {
                        Ok((_, n)) => n.to_vec(),
                        Err(notice) => {
                            let line = vm.current_line;
                            vm.emit_notice_at(notice, line);
                            key.clone()
                        }
                    };
                    let is_int = native_int_keys.contains(&name)
                        || is_array_object && !name.is_empty() && {
                            let s = std::str::from_utf8(&name).unwrap_or("");
                            s.parse::<i64>()
                                .map(|n| n.to_string() == s)
                                .unwrap_or(false)
                        };
                    if is_int {
                        buf.extend_from_slice(&name);
                    } else {
                        buf.push(b'\'');
                        var_export_escape(buf, &name, false);
                        buf.push(b'\'');
                    }
                    buf.extend_from_slice(b" => ");
                    var_export_value(vm, value, buf, level + 2);
                    buf.extend_from_slice(b",\n");
                }
            }
            VAR_EXPORT_STACK.with(|st| {
                st.borrow_mut().pop();
            });
            if level > 1 && !is_enum {
                var_export_spaces(buf, level - 1);
            }
            if is_std {
                buf.push(b')');
            } else if !is_enum {
                buf.extend_from_slice(b"))");
            }
        }
    }
}

/// var_dump()'s `%.*H` with serialize_precision -1: zend_gcvt() of the
/// shortest round-trip digits (smart_str_append_double()).
fn format_php_float_serialize(f: f64) -> String {
    String::from_utf8(goro_core::value::zend_gcvt(f, -1, b'.', b'E')).unwrap_or_default()
}

/// var_dump an object given its (mangled-key) property table, as produced by
/// a get_debug_info handler.
fn var_dump_mangled_table(
    vm: &mut Vm,
    class_name: &str,
    oid: u64,
    table: &goro_core::array::PhpArray,
    indent: usize,
    seen: &mut HashSet<u64>,
) {
    let prefix = " ".repeat(indent);
    if !seen.insert(oid) {
        vm.write_output(format!("{}*RECURSION*\n", prefix).as_bytes());
        return;
    }
    vm.write_output(
        format!(
            "{}object({})#{} ({}) {{\n",
            prefix,
            class_name,
            oid,
            table.len()
        )
        .as_bytes(),
    );
    let items: Vec<_> = table.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (key, value) in &items {
        match key {
            goro_core::array::ArrayKey::Int(n) => {
                vm.write_output(format!("{}  [{}]=>\n", prefix, n).as_bytes());
            }
            goro_core::array::ArrayKey::String(s) => {
                let bytes = s.as_bytes();
                if bytes.first() == Some(&0u8) {
                    if let Some(end) = bytes[1..].iter().position(|&b| b == 0u8) {
                        let cls = &bytes[1..1 + end];
                        let name_str = String::from_utf8_lossy(&bytes[2 + end..]);
                        if cls == b"*" {
                            vm.write_output(
                                format!("{}  [\"{}\":protected]=>\n", prefix, name_str).as_bytes(),
                            );
                        } else {
                            let cls_str = String::from_utf8_lossy(cls);
                            vm.write_output(
                                format!(
                                    "{}  [\"{}\":\"{}\":private]=>\n",
                                    prefix, name_str, cls_str
                                )
                                .as_bytes(),
                            );
                        }
                        var_dump_value(vm, value, indent + 2, seen);
                        continue;
                    }
                }
                vm.write_output(
                    format!("{}  [\"{}\"]=>\n", prefix, s.to_string_lossy()).as_bytes(),
                );
            }
        }
        var_dump_value(vm, value, indent + 2, seen);
    }
    vm.write_output(format!("{}}}\n", prefix).as_bytes());
    seen.remove(&oid);
}

/// Property table of an extension object (SimpleXML, DOM) for dumping, or
/// None for ordinary objects. A user-defined `__debugInfo` wins for debug
/// output.
pub fn native_props(
    vm: &mut Vm,
    obj: &std::rc::Rc<std::cell::RefCell<goro_core::object::PhpObject>>,
    purpose: u8,
) -> Option<goro_core::array::PhpArray> {
    let f = obj
        .borrow()
        .native
        .as_ref()
        .and_then(|n| n.handlers.get_properties)?;
    if purpose == 0 {
        let cl: Vec<u8> = obj.borrow().class_name.to_ascii_lowercase();
        if vm
            .classes
            .get(&cl)
            .and_then(|c| c.get_method(b"__debuginfo"))
            .is_some()
        {
            return None;
        }
    }
    f(vm, obj, purpose)
}

/// Marks the shadow objects materialize_print_r() makes.
static PRINT_R_SHADOW: goro_core::vm::NativeObjectHandlers =
    goro_core::vm::NativeObjectHandlers::EMPTY;

fn is_print_r_shadow(o: &goro_core::object::PhpObject) -> bool {
    o.native
        .as_ref()
        .is_some_and(|n| std::ptr::eq(n.handlers, &PRINT_R_SHADOW))
}

/// print_hash()'s object key: `name`, `name:protected` or
/// `name:Class:private` from a property table key.
fn print_r_key(buf: &mut Vec<u8>, key: &[u8]) {
    let unmangled = goro_core::proptable::unmangle_property_name_ex(key);
    if let Err(notice) = unmangled {
        PRINT_R_NOTICES.with(|n| n.borrow_mut().push(notice));
    }
    if let Ok((Some(class), name)) = unmangled {
        {
            buf.extend_from_slice(name);
            if class == b"*" {
                buf.extend_from_slice(b":protected");
            } else {
                buf.push(b':');
                buf.extend_from_slice(goro_core::value::display_class_name(class).as_bytes());
                buf.extend_from_slice(b":private");
            }
            return;
        }
    }
    buf.extend_from_slice(key);
}

/// For print_r(): replace the objects inside `v` by shadow objects holding
/// the property table print_r() shows (zend_get_properties_for(DEBUG):
/// __debugInfo(), an extension's table, or the standard table with mangled
/// keys). Objects print_r() renders itself are kept. Returns None when
/// nothing changed.
pub fn materialize_print_r(
    vm: &mut Vm,
    v: &Value,
    memo: &mut std::collections::HashMap<usize, Option<Value>>,
) -> Option<Value> {
    use std::cell::RefCell;
    use std::rc::Rc;
    match v {
        Value::Reference(r) => {
            let inner = r.borrow().clone();
            materialize_print_r(vm, &inner, memo)
        }
        Value::Array(a) => {
            let key = Rc::as_ptr(a) as *const u8 as usize;
            if let Some(m) = memo.get(&key) {
                return m.clone();
            }
            memo.insert(key, None);
            let entries: Vec<_> = a
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let mut changed: Vec<(usize, Value)> = Vec::new();
            for (i, (_, ev)) in entries.iter().enumerate() {
                if let Some(nv) = materialize_print_r(vm, ev, memo) {
                    changed.push((i, nv));
                }
            }
            memo.remove(&key);
            if changed.is_empty() {
                return None;
            }
            let mut out = goro_core::array::PhpArray::new();
            let mut ci = 0;
            for (i, (k, ev)) in entries.into_iter().enumerate() {
                if ci < changed.len() && changed[ci].0 == i {
                    out.set(k, changed[ci].1.clone());
                    ci += 1;
                } else {
                    out.set(k, ev);
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(out))))
        }
        Value::Object(o) => {
            let key = Rc::as_ptr(o) as *const u8 as usize;
            if let Some(m) = memo.get(&key) {
                return m.clone();
            }
            let (cl, cn, keep) = {
                let b = o.borrow();
                let cl = b.class_name.to_ascii_lowercase();
                let keep = b.closure.is_some()
                    || b.has_property(b"__enum_case")
                    || b.has_property(b"__lazy_state");
                (cl, b.class_name.clone(), keep)
            };
            if keep {
                // Rendered by print_r itself: materialize what it holds.
                memo.insert(key, None);
                let props: Vec<(Vec<u8>, Value)> = o.borrow().properties.clone();
                let mut changed = false;
                let mut newp = Vec::with_capacity(props.len());
                for (k, pv) in props {
                    match materialize_print_r(vm, &pv, memo) {
                        Some(nv) => {
                            changed = true;
                            newp.push((k, nv));
                        }
                        None => newp.push((k, pv)),
                    }
                }
                memo.remove(&key);
                if !changed {
                    return None;
                }
                let b = o.borrow();
                let mut shadow = goro_core::object::PhpObject::new(b.class_name.clone(), 0);
                shadow.closure = b.closure.clone();
                shadow.properties = newp;
                return Some(Value::Object(Rc::new(RefCell::new(shadow))));
            }
            let mut shadow = goro_core::object::PhpObject::new(cn, 0);
            shadow.native = Some(goro_core::object::NativeData {
                data: Rc::new(()),
                handlers: &PRINT_R_SHADOW,
            });
            let shadow = Rc::new(RefCell::new(shadow));
            let shadow_val = Value::Object(shadow.clone());
            memo.insert(key, Some(shadow_val.clone()));
            let rows: Vec<(Vec<u8>, Value)> = if let Some(rows) = internal_debug_rows(vm, o, &cl) {
                rows
            } else if let Some(arr) = native_props(vm, o, 0) {
                arr.iter()
                    .map(|(k, v)| (array_key_bytes(k), v.clone()))
                    .collect()
            } else if vm
                .classes
                .get(&cl)
                .and_then(|c| c.get_method(b"__debuginfo"))
                .is_some()
            {
                match vm
                    .call_object_method(v, b"__debugInfo", &[])
                    .map(|r| r.deref())
                {
                    Some(Value::Array(a)) => a
                        .borrow()
                        .iter()
                        .map(|(k, v)| (array_key_bytes(k), v.clone()))
                        .collect(),
                    _ => Vec::new(),
                }
            } else if let Some(t) = vm.spl_debug_info(o) {
                // An SPL class's get_debug_info handler (inherited by
                // subclasses: ArrayObject's storage, ...).
                t.iter()
                    .map(|(k, v)| (array_key_bytes(k), v.clone()))
                    .collect()
            } else {
                vm.object_debug_table(o)
                    .into_iter()
                    .filter(|(k, v, _)| !matches!(v, Value::Undef) && !is_internal_property(k))
                    .map(|(k, v, _)| (k, v))
                    .collect()
            };
            let mut props = Vec::with_capacity(rows.len());
            for (k, pv) in rows {
                let nv = materialize_print_r(vm, &pv, memo).unwrap_or(pv);
                props.push((k, nv));
            }
            shadow.borrow_mut().properties = props;
            Some(shadow_val)
        }
        // zend_generator_get_debug_info(): the generator's function.
        Value::Generator(g) => {
            let name = vm.generator_function_name(g);
            let mut shadow = goro_core::object::PhpObject::new(b"Generator".to_vec(), 0);
            shadow.native = Some(goro_core::object::NativeData {
                data: Rc::new(()),
                handlers: &PRINT_R_SHADOW,
            });
            shadow.properties = vec![(
                b"function".to_vec(),
                Value::String(PhpString::from_string(name.to_string())),
            )];
            Some(Value::Object(Rc::new(RefCell::new(shadow))))
        }
        _ => None,
    }
}

/// The get_debug_info handlers of internal classes whose objects keep their
/// state outside the property table (what var_dump() shows of them):
/// WeakReference's referent, WeakMap's entries, a random engine's states.
fn internal_debug_rows(
    vm: &mut Vm,
    o: &std::rc::Rc<std::cell::RefCell<goro_core::object::PhpObject>>,
    cl: &[u8],
) -> Option<Vec<(Vec<u8>, Value)>> {
    use std::cell::RefCell;
    use std::rc::Rc;
    let has_user_debug_info = vm
        .classes
        .get(cl)
        .and_then(|c| c.get_method(b"__debuginfo"))
        .is_some();
    match cl {
        b"weakreference" => {
            let weak_id = o.borrow().get_property(b"__weak_ref_id").to_long() as u64;
            let referent = vm
                .weak_refs
                .get(&weak_id)
                .and_then(|w| w.upgrade())
                .map(Value::Object)
                .unwrap_or(Value::Null);
            Some(vec![(b"object".to_vec(), referent)])
        }
        b"weakmap" => {
            let oid = o.borrow().object_id;
            let entries: Vec<(Value, Value)> = vm
                .weak_map_entries
                .get(&oid)
                .map(|es| {
                    es.iter()
                        .filter_map(|(_, w, v)| w.upgrade().map(|s| (Value::Object(s), v.clone())))
                        .collect()
                })
                .unwrap_or_default();
            Some(
                entries
                    .into_iter()
                    .enumerate()
                    .map(|(i, (k, v))| {
                        let mut e = goro_core::array::PhpArray::new();
                        e.set(
                            goro_core::array::ArrayKey::String(PhpString::from_bytes(b"key")),
                            k,
                        );
                        e.set(
                            goro_core::array::ArrayKey::String(PhpString::from_bytes(b"value")),
                            v.deref(),
                        );
                        (
                            i.to_string().into_bytes(),
                            Value::Array(Rc::new(RefCell::new(e))),
                        )
                    })
                    .collect(),
            )
        }
        b"random\\engine\\mt19937"
        | b"random\\engine\\xoshiro256starstar"
        | b"random\\engine\\pcgoneseq128xslrr64"
            if !has_user_debug_info =>
        {
            match vm
                .call_object_method(&Value::Object(o.clone()), b"__debugInfo", &[])
                .map(|r| r.deref())
            {
                Some(Value::Array(a)) => Some(
                    a.borrow()
                        .iter()
                        .map(|(k, v)| (array_key_bytes(k), v.clone()))
                        .collect(),
                ),
                _ => None,
            }
        }
        _ => None,
    }
}

fn array_key_bytes(k: &goro_core::array::ArrayKey) -> Vec<u8> {
    match k {
        goro_core::array::ArrayKey::Int(n) => n.to_string().into_bytes(),
        goro_core::array::ArrayKey::String(s) => s.as_bytes().to_vec(),
    }
}

/// Replace extension objects inside `v` by plain shadow objects holding
/// their property tables, for dumpers that work without VM access
/// (print_r, var_export). Returns None when nothing changed.
pub fn materialize_native(
    vm: &mut Vm,
    v: &Value,
    purpose: u8,
    seen: &mut HashSet<usize>,
) -> Option<Value> {
    use std::cell::RefCell;
    use std::rc::Rc;
    match v {
        Value::Reference(r) => {
            let inner = r.borrow().clone();
            materialize_native(vm, &inner, purpose, seen)
        }
        Value::Array(a) => {
            let key = Rc::as_ptr(a) as *const u8 as usize;
            if !seen.insert(key) {
                return None;
            }
            let entries: Vec<_> = a
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let mut changed: Vec<(usize, Value)> = Vec::new();
            for (i, (_, ev)) in entries.iter().enumerate() {
                if let Some(nv) = materialize_native(vm, ev, purpose, seen) {
                    changed.push((i, nv));
                }
            }
            seen.remove(&key);
            if changed.is_empty() {
                return None;
            }
            let mut out = goro_core::array::PhpArray::new();
            let mut ci = 0;
            for (i, (k, ev)) in entries.into_iter().enumerate() {
                if ci < changed.len() && changed[ci].0 == i {
                    out.set(k, changed[ci].1.clone());
                    ci += 1;
                } else {
                    out.set(k, ev);
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(out))))
        }
        Value::Object(o) => {
            let key = Rc::as_ptr(o) as *const u8 as usize;
            if !seen.insert(key) {
                return None;
            }
            let result = if let Some(arr) = native_props(vm, o, purpose) {
                let cn = o.borrow().class_name.clone();
                let mut shadow = goro_core::object::PhpObject::new(cn, 0);
                for (k, ev) in arr.iter() {
                    let nv =
                        materialize_native(vm, ev, purpose, seen).unwrap_or_else(|| ev.clone());
                    let name = match k {
                        goro_core::array::ArrayKey::Int(n) => n.to_string().into_bytes(),
                        goro_core::array::ArrayKey::String(s) => s.as_bytes().to_vec(),
                    };
                    shadow.properties.push((name, nv));
                }
                Some(Value::Object(Rc::new(RefCell::new(shadow))))
            } else {
                let props: Vec<(Vec<u8>, Value)> = o.borrow().properties.clone();
                let mut changed = false;
                let mut newp = Vec::with_capacity(props.len());
                for (k, pv) in props {
                    match materialize_native(vm, &pv, purpose, seen) {
                        Some(nv) => {
                            changed = true;
                            newp.push((k, nv));
                        }
                        None => newp.push((k, pv)),
                    }
                }
                if changed {
                    let b = o.borrow();
                    let mut shadow = goro_core::object::PhpObject::new(b.class_name.clone(), 0);
                    shadow.properties = newp;
                    Some(Value::Object(Rc::new(RefCell::new(shadow))))
                } else {
                    None
                }
            };
            seen.remove(&key);
            result
        }
        _ => None,
    }
}
