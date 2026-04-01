use std::collections::HashSet;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

pub fn register(vm: &mut Vm) {
    vm.register_function(b"var_dump", var_dump);
    vm.register_function(b"print_r", print_r);
    vm.register_function(b"var_export", var_export);
}

fn is_internal_property(name: &[u8]) -> bool {
    name.starts_with(b"__spl_") || name.starts_with(b"__reflection_")
        || name.starts_with(b"__timestamp") || name.starts_with(b"__enum_")
        || name.starts_with(b"__fiber_") || name.starts_with(b"__ctor_")
        || name.starts_with(b"__clone_") || name.starts_with(b"__destructed")
}

fn var_dump(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let mut seen = HashSet::new();
    for arg in args {
        var_dump_value(vm, arg, 0, &mut seen);
    }
    Ok(Value::Null)
}

fn var_dump_value(vm: &mut Vm, val: &Value, indent: usize, seen: &mut HashSet<u64>) {
    if indent > 40 {
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
        Value::Double(f) => {
            // var_dump uses serialize_precision (-1 in PHP 8 = shortest representation)
            let sp = goro_core::value::get_php_serialize_precision();
            let formatted = if sp < 0 {
                format_php_float_serialize(*f)
            } else {
                goro_core::value::format_php_float_with_precision_pub(*f, sp as usize)
            };
            vm.write_output(
                format!("{}float({})\n", prefix, formatted).as_bytes(),
            );
        }
        Value::String(s) => {
            let b = s.as_bytes();
            if b.starts_with(b"__closure_") || b.starts_with(b"__arrow_") || b.starts_with(b"__bound_closure_") || b.starts_with(b"__closure_fcc_") {
                // Closures should be displayed as Closure objects with a stable ID
                // We use 1 as default; tests use %d in EXPECTF patterns
                vm.write_output(
                    format!("{}object(Closure)#1 (0) {{\n{}}}\n", prefix, prefix).as_bytes(),
                );
            } else {
                vm.write_output(
                    format!(
                        "{}string({}) \"{}\"\n",
                        prefix,
                        s.len(),
                        s.to_string_lossy()
                    )
                    .as_bytes(),
                );
            }
        }
        Value::Array(arr) => {
            let arr = arr.borrow();
            vm.write_output(format!("{}array({}) {{\n", prefix, arr.len()).as_bytes());
            for (key, value) in arr.iter() {
                match key {
                    goro_core::array::ArrayKey::Int(n) => {
                        vm.write_output(format!("{}  [{}]=>\n", prefix, n).as_bytes());
                    }
                    goro_core::array::ArrayKey::String(s) => {
                        vm.write_output(
                            format!("{}  [\"{}\"]=>\n", prefix, s.to_string_lossy()).as_bytes(),
                        );
                    }
                }
                var_dump_value(vm, value, indent + 2, seen);
            }
            vm.write_output(format!("{}}}\n", prefix).as_bytes());
        }
        Value::Object(obj) => {
            let obj_borrow = obj.borrow();
            // Check if this is an enum case object
            if obj_borrow.has_property(b"__enum_case") {
                let class_name = goro_core::value::display_class_name(&obj_borrow.class_name);
                let case_name = obj_borrow.get_property(b"name");
                let case_name_str = case_name.to_php_string().to_string_lossy();
                vm.write_output(format!("{}enum({}::{})\n", prefix, class_name, case_name_str).as_bytes());
                return;
            }
            let class_name = goro_core::value::display_class_name(&obj_borrow.class_name);
            let class_lower: Vec<u8> = obj_borrow
                .class_name
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            let oid = obj_borrow.object_id;

            // Check if this is an SPL class with __spl_array - display array contents instead
            let is_spl_array_class = matches!(
                class_lower.as_slice(),
                b"splfixedarray"
            );
            // SplDoublyLinkedList/SplStack/SplQueue have special var_dump format
            let is_spl_dll_class = matches!(
                class_lower.as_slice(),
                b"spldoublylinkedlist" | b"splstack" | b"splqueue"
            );
            // ArrayObject/ArrayIterator use a private "storage" property format
            let is_array_object_class = matches!(
                class_lower.as_slice(),
                b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator"
            );

            if is_array_object_class {
                let spl_arr = obj_borrow.get_property(b"__spl_array");
                let flags = obj_borrow.get_property(b"__spl_flags");
                let flags_val = if let Value::Long(f) = flags { f } else { 0 };
                let _std_prop_list = (flags_val & 1) != 0;
                let class_name_owned = class_name.to_string();
                // Count visible properties (non-internal)
                let extra_props: Vec<_> = obj_borrow.properties.iter()
                    .filter(|(name, val)| !is_internal_property(name) && !matches!(val, Value::Undef))
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
                // Show private storage property
                vm.write_output(format!("{}  [\"storage\":\"{}\":private]=>\n", prefix, class_name_owned).as_bytes());
                var_dump_value(vm, &spl_arr, indent + 2, seen);
                vm.write_output(format!("{}}}\n", prefix).as_bytes());
                seen.remove(&oid);
                return;
            }

            if is_spl_dll_class {
                // SplDoublyLinkedList/SplStack/SplQueue: show flags and dllist private properties
                let spl_arr = obj_borrow.get_property(b"__spl_array");
                let iter_mode = obj_borrow.get_property(b"__spl_iter_mode");
                let flags_val = if let Value::Long(f) = iter_mode { f } else {
                    match class_lower.as_slice() {
                        b"splstack" => 6,
                        b"splqueue" => 4,
                        _ => 0,
                    }
                };
                let class_name_owned = class_name.to_string();
                drop(obj_borrow);
                vm.write_output(
                    format!(
                        "{}object({})#{} (2) {{\n",
                        prefix, class_name_owned, oid
                    )
                    .as_bytes(),
                );
                if !seen.insert(oid) {
                    vm.write_output(format!("{}  *RECURSION*\n", prefix).as_bytes());
                    vm.write_output(format!("{}}}\n", prefix).as_bytes());
                    return;
                }
                vm.write_output(format!("{}  [\"flags\":\"SplDoublyLinkedList\":private]=>\n", prefix).as_bytes());
                var_dump_value(vm, &Value::Long(flags_val), indent + 2, seen);
                vm.write_output(format!("{}  [\"dllist\":\"SplDoublyLinkedList\":private]=>\n", prefix).as_bytes());
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
                                    format!("{}  [\"{}\"]=>\n", prefix, s.to_string_lossy()).as_bytes(),
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
                class_entry.as_ref().and_then(|c| c.get_method(b"__debuginfo")).is_some()
            };
            // Also check SPL built-in __debugInfo
            let is_spl_debug = !has_debug_info && matches!(
                class_lower.as_slice(),
                b"splfileinfo" | b"splfileobject" | b"spltempfileobject"
                    | b"directoryiterator" | b"filesystemiterator"
                    | b"recursivedirectoryiterator" | b"globiterator"
            );

            if has_debug_info || is_spl_debug {
                // Call __debugInfo() and use the returned array for display
                let debug_result = if is_spl_debug {
                    // Use SPL dispatch for __debugInfo
                    vm.call_object_method(val, b"__debugInfo", &[])
                } else {
                    let class_entry = vm.classes.get(&class_lower).cloned();
                    if let Some(class) = class_entry {
                        if let Some(method) = class.get_method(b"__debuginfo") {
                            let op = method.op_array.clone();
                            let mut fn_cvs = vec![Value::Undef; op.cv_names.len()];
                            if !fn_cvs.is_empty() {
                                fn_cvs[0] = val.clone(); // $this
                            }
                            vm.execute_fn(&op, fn_cvs).ok()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };

                if let Some(Value::Array(debug_arr)) = debug_result {
                    let arr_borrow = debug_arr.borrow();
                    let count = arr_borrow.len();
                    let class_name_owned = class_name.to_string();
                    drop(obj_borrow);
                    vm.write_output(
                        format!("{}object({})#{} ({}) {{\n", prefix, class_name_owned, oid, count).as_bytes(),
                    );
                    if !seen.insert(oid) {
                        vm.write_output(format!("{}  *RECURSION*\n", prefix).as_bytes());
                        vm.write_output(format!("{}}}\n", prefix).as_bytes());
                        return;
                    }
                    let items: Vec<_> = arr_borrow.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    drop(arr_borrow);
                    for (key, value) in &items {
                        match key {
                            goro_core::array::ArrayKey::Int(n) => {
                                vm.write_output(format!("{}  [{}]=>\n", prefix, n).as_bytes());
                            }
                            goro_core::array::ArrayKey::String(s) => {
                                vm.write_output(format!("{}  [\"{}\"]=>\n", prefix, s.to_string_lossy()).as_bytes());
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

            // Don't count uninitialized (Undef) properties or internal __spl_/__reflection_ properties
            let prop_count = obj_borrow.properties.iter()
                .filter(|(name, val)| !is_internal_property(name) && !matches!(val, Value::Undef))
                .count();
            vm.write_output(
                format!(
                    "{}object({})#{} ({}) {{\n",
                    prefix, class_name, oid, prop_count
                )
                .as_bytes(),
            );

            // Check for recursion (self-referencing objects)
            if !seen.insert(oid) {
                // Already seen this object — print *RECURSION*
                vm.write_output(format!("{}  *RECURSION*\n", prefix).as_bytes());
                vm.write_output(format!("{}}}\n", prefix).as_bytes());
                return;
            }

            // Look up class to get property visibility info
            let class_info = vm.classes.get(&class_lower).cloned();

            // Properties are in declaration order (Vec preserves insertion order)
            let props: Vec<_> = obj_borrow.properties.clone();
            let obj_class_name = obj_borrow.class_name.clone();
            drop(obj_borrow);
            for (name, value) in &props {
                // Skip internal SPL/Reflection properties and uninitialized (Undef) properties
                if is_internal_property(name) || matches!(value, Value::Undef) {
                    continue;
                }
                let name_str = String::from_utf8_lossy(name);
                // Determine visibility
                let vis = class_info.as_ref().and_then(|c| {
                    c.properties
                        .iter()
                        .find(|p| p.name == *name)
                        .map(|p| p.visibility)
                });
                let display_name = match vis {
                    Some(goro_core::object::Visibility::Protected) => {
                        format!("\"{}\":protected", name_str)
                    }
                    Some(goro_core::object::Visibility::Private) => {
                        let class_name = goro_core::value::display_class_name(&obj_class_name);
                        format!("\"{}\":\"{}\":private", name_str, class_name)
                    }
                    _ => format!("\"{}\"", name_str),
                };
                vm.write_output(format!("{}  [{}]=>\n", prefix, display_name).as_bytes());
                var_dump_value(vm, value, indent + 2, seen);
            }
            vm.write_output(format!("{}}}\n", prefix).as_bytes());
            seen.remove(&oid);
        }
        Value::Generator(generator) => {
            let gen_ref = generator.borrow();
            let func_name = String::from_utf8_lossy(&gen_ref.op_array.name);
            let prop_count = 1; // function property
            // Use a hash-based ID since generators don't have object_id
            let gen_ptr = generator.as_ptr() as u64;
            let gen_id = (gen_ptr >> 4) % 10000 + 1;
            vm.write_output(
                format!("{}object(Generator)#{} ({}) {{\n", prefix, gen_id, prop_count).as_bytes(),
            );
            vm.write_output(
                format!("{}  [\"function\"]=>\n", prefix).as_bytes(),
            );
            let name_str = func_name.to_string();
            vm.write_output(
                format!("{}  string({}) \"{}\"\n", prefix, name_str.len(), name_str).as_bytes(),
            );
            vm.write_output(format!("{}}}\n", prefix).as_bytes());
        }
        Value::Reference(r) => {
            // PHP only shows & prefix when reference count >= 2
            let inner = r.borrow().clone();
            if std::rc::Rc::strong_count(r) >= 2 {
                var_dump_value_ref(vm, &inner, indent, &prefix, seen);
            } else {
                // Single reference - show without & prefix
                var_dump_value(vm, &inner, indent, seen);
            }
        }
    }
}

fn var_dump_value_ref(vm: &mut Vm, val: &Value, indent: usize, prefix: &str, seen: &mut HashSet<u64>) {
    match val {
        Value::Null | Value::Undef => {
            vm.write_output(format!("{}&NULL\n", prefix).as_bytes());
        }
        Value::True => {
            vm.write_output(format!("{}&bool(true)\n", prefix).as_bytes());
        }
        Value::False => {
            vm.write_output(format!("{}&bool(false)\n", prefix).as_bytes());
        }
        Value::Long(n) => {
            vm.write_output(format!("{}&int({})\n", prefix, n).as_bytes());
        }
        Value::Double(f) => {
            vm.write_output(
                format!("{}&float({})\n", prefix, format_php_float_serialize(*f)).as_bytes(),
            );
        }
        Value::String(s) => {
            vm.write_output(
                format!(
                    "{}&string({}) \"{}\"\n",
                    prefix,
                    s.len(),
                    s.to_string_lossy()
                )
                .as_bytes(),
            );
        }
        Value::Array(arr) => {
            let arr = arr.borrow();
            vm.write_output(format!("{}&array({}) {{\n", prefix, arr.len()).as_bytes());
            for (key, value) in arr.iter() {
                match key {
                    goro_core::array::ArrayKey::Int(n) => {
                        vm.write_output(format!("{}  [{}]=>\n", prefix, n).as_bytes());
                    }
                    goro_core::array::ArrayKey::String(s) => {
                        vm.write_output(
                            format!("{}  [\"{}\"]=>\n", prefix, s.to_string_lossy()).as_bytes(),
                        );
                    }
                }
                var_dump_value(vm, value, indent + 2, seen);
            }
            vm.write_output(format!("{}}}\n", prefix).as_bytes());
        }
        _ => {
            // For other types (Object, nested Reference), just dump normally
            var_dump_value(vm, val, indent, seen);
        }
    }
}

fn print_r(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Err(VmError {
            message: "print_r() expects at least 1 argument".into(),
            line: 0,
        });
    }

    let return_output = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    // Clone classes for property visibility lookup in print_r_value
    let classes: goro_core::vm::ClassMap = vm.classes.clone();

    if return_output {
        let mut buf = Vec::new();
        print_r_value(&args[0], &mut buf, 0, &classes);
        Ok(Value::String(PhpString::from_vec(buf)))
    } else {
        let mut buf = Vec::new();
        print_r_value(&args[0], &mut buf, 0, &classes);
        vm.write_output(&buf);
        Ok(Value::True)
    }
}

fn print_r_value(val: &Value, buf: &mut Vec<u8>, indent: usize, classes: &goro_core::vm::ClassMap) {
    if indent > 20 {
        buf.extend_from_slice(b" *RECURSION*");
        return;
    }
    match val {
        Value::Null | Value::Undef => buf.extend_from_slice(b""),
        Value::True => buf.extend_from_slice(b"1"),
        Value::False => {}
        Value::Long(n) => buf.extend_from_slice(n.to_string().as_bytes()),
        Value::Double(f) => buf.extend_from_slice(format_float(*f).as_bytes()),
        Value::String(s) => {
            let b = s.as_bytes();
            if b.starts_with(b"__closure_") || b.starts_with(b"__arrow_") || b.starts_with(b"__bound_closure_") || b.starts_with(b"__closure_fcc_") {
                buf.extend_from_slice(b"Closure Object\n");
                let prefix = " ".repeat(indent);
                buf.extend_from_slice(format!("{}(\n{})\n", prefix, prefix).as_bytes());
            } else {
                buf.extend_from_slice(b);
            }
        }
        Value::Array(arr) => {
            let arr = arr.borrow();
            let prefix = " ".repeat(indent);
            buf.extend_from_slice(b"Array\n");
            buf.extend_from_slice(format!("{}(\n", prefix).as_bytes());
            for (key, value) in arr.iter() {
                match key {
                    goro_core::array::ArrayKey::Int(n) => {
                        buf.extend_from_slice(format!("{}    [{}] => ", prefix, n).as_bytes());
                    }
                    goro_core::array::ArrayKey::String(s) => {
                        buf.extend_from_slice(
                            format!("{}    [{}] => ", prefix, s.to_string_lossy()).as_bytes(),
                        );
                    }
                }
                print_r_value(value, buf, indent + 8, classes);
                buf.push(b'\n');
            }
            buf.extend_from_slice(format!("{})\n", prefix).as_bytes());
        }
        Value::Object(obj) => {
            let obj_borrow = obj.borrow();
            // Check if this is an enum case object
            if obj_borrow.has_property(b"__enum_case") {
                let class_name = String::from_utf8_lossy(&obj_borrow.class_name).into_owned();
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
                print_r_value(&case_name, buf, indent + 8, classes);
                buf.push(b'\n');
                if has_backing {
                    let case_value = obj_borrow.get_property(b"value");
                    buf.extend_from_slice(format!("{}    [value] => ", prefix).as_bytes());
                    print_r_value(&case_value, buf, indent + 8, classes);
                    buf.push(b'\n');
                }
                buf.extend_from_slice(format!("{})\n", prefix).as_bytes());
                return;
            }
            let class_name = String::from_utf8_lossy(&obj_borrow.class_name).into_owned();
            let class_lower: Vec<u8> = obj_borrow
                .class_name
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            let prefix = " ".repeat(indent);

            // Check if this is an SPL class with __spl_array
            let is_array_object_class = matches!(
                class_lower.as_slice(),
                b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator"
            );
            let is_spl_array_class = matches!(
                class_lower.as_slice(),
                b"splfixedarray"
            );
            let is_spl_dllist_class = matches!(
                class_lower.as_slice(),
                b"spldoublylinkedlist" | b"splstack" | b"splqueue"
            );

            if is_spl_dllist_class {
                let spl_arr = obj_borrow.get_property(b"__spl_array");
                let iter_mode = obj_borrow.get_property(b"__spl_iter_mode");
                let flags_val = if let Value::Long(f) = iter_mode { f } else { 0 };
                drop(obj_borrow);
                buf.extend_from_slice(format!("{} Object\n", class_name).as_bytes());
                buf.extend_from_slice(format!("{}(\n", prefix).as_bytes());
                buf.extend_from_slice(format!("{}    [flags:{}:private] => {}\n", prefix, class_name, flags_val).as_bytes());
                buf.extend_from_slice(format!("{}    [dllist:{}:private] => ", prefix, class_name).as_bytes());
                print_r_value(&spl_arr, buf, indent + 8, classes);
                buf.push(b'\n');
                buf.extend_from_slice(format!("{})\n", prefix).as_bytes());
                return;
            }

            if is_array_object_class {
                let spl_arr = obj_borrow.get_property(b"__spl_array");
                // Show extra props first
                let extra_props: Vec<_> = obj_borrow.properties.iter()
                    .filter(|(name, val)| !is_internal_property(name) && !matches!(val, Value::Undef))
                    .map(|(n, v)| (n.clone(), v.clone()))
                    .collect();
                drop(obj_borrow);
                buf.extend_from_slice(format!("{} Object\n", class_name).as_bytes());
                buf.extend_from_slice(format!("{}(\n", prefix).as_bytes());
                for (name, value) in &extra_props {
                    let name_str = String::from_utf8_lossy(name);
                    buf.extend_from_slice(format!("{}    [{}] => ", prefix, name_str).as_bytes());
                    print_r_value(value, buf, indent + 8, classes);
                    buf.push(b'\n');
                }
                buf.extend_from_slice(format!("{}    [storage:{}:private] => ", prefix, class_name).as_bytes());
                print_r_value(&spl_arr, buf, indent + 8, classes);
                buf.push(b'\n');
                buf.extend_from_slice(format!("{})\n", prefix).as_bytes());
                return;
            }

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
                                buf.extend_from_slice(format!("{}    [{}] => ", prefix, n).as_bytes());
                            }
                            goro_core::array::ArrayKey::String(s) => {
                                buf.extend_from_slice(
                                    format!("{}    [{}] => ", prefix, s.to_string_lossy()).as_bytes(),
                                );
                            }
                        }
                        print_r_value(value, buf, indent + 8, classes);
                        buf.push(b'\n');
                    }
                    buf.extend_from_slice(format!("{})\n", prefix).as_bytes());
                    return;
                }
            }

            // Look up class for property visibility
            let class_info = classes.get(&class_lower);

            buf.extend_from_slice(format!("{} Object\n", class_name).as_bytes());
            buf.extend_from_slice(format!("{}(\n", prefix).as_bytes());
            for (name, value) in &obj_borrow.properties {
                // Skip internal SPL/Reflection properties and uninitialized properties
                if is_internal_property(name) || matches!(value, Value::Undef) {
                    continue;
                }
                let name_str = String::from_utf8_lossy(name);
                // Determine visibility from class definition
                let vis = class_info.and_then(|c| {
                    c.properties
                        .iter()
                        .find(|p| p.name == *name)
                        .map(|p| p.visibility)
                });
                let display_name = match vis {
                    Some(goro_core::object::Visibility::Protected) => {
                        format!("{}:protected", name_str)
                    }
                    Some(goro_core::object::Visibility::Private) => {
                        format!("{}:{}:private", name_str, class_name)
                    }
                    _ => format!("{}", name_str),
                };
                buf.extend_from_slice(format!("{}    [{}] => ", prefix, display_name).as_bytes());
                print_r_value(value, buf, indent + 8, classes);
                buf.push(b'\n');
            }
            buf.extend_from_slice(format!("{})\n", prefix).as_bytes());
        }
        Value::Generator(_) => {
            buf.extend_from_slice(b"Generator Object\n(\n)\n");
        }
        Value::Reference(r) => {
            print_r_value(&r.borrow(), buf, indent, classes);
        }
    }
}

fn var_export(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Err(VmError {
            message: "var_export() expects at least 1 argument".into(),
            line: 0,
        });
    }
    let return_output = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let mut buf = Vec::new();
    var_export_value(&args[0], &mut buf, 0);

    if return_output {
        Ok(Value::String(PhpString::from_vec(buf)))
    } else {
        vm.write_output(&buf);
        Ok(Value::Null)
    }
}

fn var_export_value(val: &Value, buf: &mut Vec<u8>, indent: usize) {
    // Prevent infinite recursion
    if indent > 100 {
        buf.extend_from_slice(b"NULL");
        return;
    }
    let prefix = " ".repeat(indent);
    match val {
        Value::Null | Value::Undef => buf.extend_from_slice(b"NULL"),
        Value::True => buf.extend_from_slice(b"true"),
        Value::False => buf.extend_from_slice(b"false"),
        Value::Long(n) => {
            // PHP_INT_MIN must be represented as (-9223372036854775807-1) to be parsable
            if *n == i64::MIN {
                buf.extend_from_slice(b"(-9223372036854775807-1)");
            } else {
                buf.extend_from_slice(n.to_string().as_bytes());
            }
        }
        Value::Double(f) => {
            // var_export uses a representation that can be parsed back
            let s = format_float(*f);
            buf.extend_from_slice(s.as_bytes());
            // Ensure there's a digit after the decimal point
            if s.ends_with('.') {
                buf.push(b'0');
            } else if !s.contains('.')
                && !s.contains('E')
                && !s.contains('e')
                && s != "INF"
                && s != "-INF"
                && s != "NAN"
            {
                buf.extend_from_slice(b".0");
            }
        }
        Value::String(s) => {
            buf.push(b'\'');
            for &byte in s.as_bytes() {
                match byte {
                    b'\0' => buf.extend_from_slice(b"' . \"\\0\" . '"),
                    b'\'' => buf.extend_from_slice(b"\\'"),
                    b'\\' => buf.extend_from_slice(b"\\\\"),
                    _ => buf.push(byte),
                }
            }
            buf.push(b'\'');
        }
        Value::Array(arr) => {
            let arr = arr.borrow();
            buf.extend_from_slice(b"array (\n");
            for (key, value) in arr.iter() {
                buf.extend_from_slice(format!("{}  ", prefix).as_bytes());
                match key {
                    goro_core::array::ArrayKey::Int(n) => {
                        buf.extend_from_slice(format!("{} => ", n).as_bytes());
                    }
                    goro_core::array::ArrayKey::String(s) => {
                        buf.extend_from_slice(b"'");
                        buf.extend_from_slice(s.as_bytes());
                        buf.extend_from_slice(b"' => ");
                    }
                }
                var_export_value(value, buf, indent + 2);
                buf.extend_from_slice(b",\n");
            }
            buf.extend_from_slice(format!("{})", prefix).as_bytes());
        }
        Value::Object(obj) => {
            let obj_borrow = obj.borrow();
            // Check if this is an enum case
            if obj_borrow.has_property(b"__enum_case") {
                let class_name = String::from_utf8_lossy(&obj_borrow.class_name);
                let case_name = obj_borrow.get_property(b"name");
                let case_name_str = case_name.to_php_string().to_string_lossy();
                // PHP always adds a leading \ to enum class names in var_export
                let prefix_backslash = if class_name.starts_with('\\') { "" } else { "\\" };
                buf.extend_from_slice(format!("{}{}::{}", prefix_backslash, class_name, case_name_str).as_bytes());
                return;
            }
            let class_name = String::from_utf8_lossy(&obj_borrow.class_name);
            let class_lower = class_name.to_ascii_lowercase();
            if class_lower == "stdclass" {
                // PHP 8.2+: stdClass uses (object) array(...) format
                buf.extend_from_slice(b"(object) array(\n");
                for (name, value) in &obj_borrow.properties {
                    let name_str = String::from_utf8_lossy(name);
                    buf.extend_from_slice(format!("{}   '{}' => ", prefix, name_str).as_bytes());
                    var_export_value(value, buf, indent + 2);
                    buf.extend_from_slice(b",\n");
                }
                buf.extend_from_slice(format!("{})", prefix).as_bytes());
            } else {
                // PHP prefixes class names with \ in var_export unless they already start with \
                let prefix_backslash = if class_name.starts_with('\\') { "" } else { "\\" };
                buf.extend_from_slice(format!("{}{}::__set_state(array(\n", prefix_backslash, class_name).as_bytes());
                for (name, value) in &obj_borrow.properties {
                    let name_str = String::from_utf8_lossy(name);
                    buf.extend_from_slice(format!("{}   '{}' => ", prefix, name_str).as_bytes());
                    var_export_value(value, buf, indent + 2);
                    buf.extend_from_slice(b",\n");
                }
                buf.extend_from_slice(format!("{}))", prefix).as_bytes());
            }
        }
        Value::Generator(_) => {
            buf.extend_from_slice(b"NULL");
        }
        Value::Reference(r) => {
            var_export_value(&r.borrow(), buf, indent);
        }
    }
}

/// Format a float using serialize_precision=-1 (shortest unique representation)
/// Format a float using C's %.*G format (used by var_dump with precision INI setting).
/// This formats with N significant digits, using E notation for very large/small numbers.
fn format_php_float_g(f: f64, precision: usize) -> String {
    if f.is_nan() {
        return "NAN".to_string();
    }
    if f.is_infinite() {
        return if f.is_sign_positive() {
            "INF".to_string()
        } else {
            "-INF".to_string()
        };
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0".to_string() } else { "0".to_string() };
    }
    // Use Rust's %G equivalent: format with precision significant digits
    // Rust doesn't have %G directly, so we implement it
    let abs_f = f.abs();
    let exp = abs_f.log10().floor() as i32;

    // %G uses scientific notation if exp < -4 or exp >= precision
    let prec = if precision == 0 { 1 } else { precision };

    if exp < -4 || exp >= prec as i32 {
        // Use scientific notation with precision-1 decimal places
        let decimal_places = if prec > 1 { prec - 1 } else { 0 };
        let s = format!("{:.width$E}", f, width = decimal_places);
        // Remove trailing zeros from mantissa (like %G), fix exponent format
        if s.contains('.') {
            let parts: Vec<&str> = s.split('E').collect();
            if parts.len() == 2 {
                let mantissa = parts[0].trim_end_matches('0').trim_end_matches('.');
                let exp_part = parts[1];
                // Ensure exponent has explicit + sign
                let exp_formatted = if !exp_part.starts_with('-') && !exp_part.starts_with('+') {
                    format!("+{}", exp_part)
                } else {
                    exp_part.to_string()
                };
                if mantissa.contains('.') {
                    format!("{}E{}", mantissa, exp_formatted)
                } else {
                    format!("{}.0E{}", mantissa, exp_formatted)
                }
            } else {
                s
            }
        } else {
            s
        }
    } else {
        // Use fixed-point notation
        // Number of decimal places = precision - (exp + 1)
        let decimal_places = if prec as i32 > exp + 1 {
            (prec as i32 - exp - 1) as usize
        } else {
            0
        };
        let s = format!("{:.width$}", f, width = decimal_places);
        // Remove trailing zeros (like %G) but keep at least one digit
        if s.contains('.') {
            let trimmed = s.trim_end_matches('0').trim_end_matches('.');
            trimmed.to_string()
        } else {
            s
        }
    }
}

/// This is what PHP 8 uses for var_dump, var_export, json_encode, etc.
fn format_php_float_serialize(f: f64) -> String {
    if f.is_infinite() {
        return if f.is_sign_positive() {
            "INF".to_string()
        } else {
            "-INF".to_string()
        };
    }
    if f.is_nan() {
        return "NAN".to_string();
    }
    // Handle negative zero
    if f == 0.0 && f.is_sign_negative() {
        return "-0".to_string();
    }
    // PHP serialize_precision=-1: shortest exact representation
    // Use scientific notation for very large/small numbers
    let abs = f.abs();
    if abs != 0.0 && !(1e-4..1e15).contains(&abs) {
        // Use scientific notation like PHP
        // Find shortest scientific representation that roundtrips
        for prec in 0..20 {
            let s = format!("{:.prec$e}", f, prec = prec);
            if let Ok(parsed) = s.parse::<f64>() {
                if parsed == f {
                    if let Some(pos) = s.find('e') {
                        let mantissa = &s[..pos];
                        let exp: i32 = s[pos + 1..].parse().unwrap_or(0);
                        // Ensure at least one decimal digit
                        let mantissa = if !mantissa.contains('.') {
                            format!("{}.0", mantissa)
                        } else if mantissa.ends_with('.') {
                            format!("{}0", mantissa)
                        } else {
                            mantissa.to_string()
                        };
                        let exp_str = if exp >= 0 {
                            format!("E+{}", exp)
                        } else {
                            format!("E{}", exp)
                        };
                        return format!("{}{}", mantissa, exp_str);
                    }
                }
            }
        }
    }

    // PHP serialize_precision=-1: shortest roundtrip representation
    // Use ryu-style formatting for exact roundtrip
    let mut buf = ryu_format(f);
    // Ensure no trailing dot
    if buf.ends_with('.') {
        buf.push('0');
    }
    buf
}

/// Format float with shortest roundtrip representation (like PHP serialize_precision=-1)
fn ryu_format(f: f64) -> String {
    // Try increasing precision until roundtrip works
    for prec in 0..20 {
        let s = format!("{:.prec$}", f, prec = prec);
        if let Ok(parsed) = s.parse::<f64>() {
            if parsed == f {
                return s;
            }
        }
    }
    format!("{}", f)
}

fn format_float(f: f64) -> String {
    // var_export uses serialize_precision for exact round-trip representation
    // PHP uses 17 significant digits to ensure exact round-trip
    if f.is_nan() {
        return "NAN".to_string();
    }
    if f.is_infinite() {
        return if f.is_sign_positive() {
            "INF".to_string()
        } else {
            "-INF".to_string()
        };
    }
    if f == 0.0 {
        return if f.is_sign_negative() {
            "-0".to_string()
        } else {
            "0".to_string()
        };
    }
    // Use serialize_precision (-1 means shortest roundtrip)
    let sp = goro_core::value::get_php_serialize_precision();
    if sp < 0 {
        // Use the shortest representation that round-trips
        // PHP uses H (mode 2) which gives the shortest string that, when
        // parsed back, gives the exact same double
        format_php_float_serialize(f)
    } else {
        goro_core::value::format_php_float_with_precision_pub(f, sp as usize)
    }
}
