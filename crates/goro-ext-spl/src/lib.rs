use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

mod autoload;
use autoload::{
    spl_autoload_call_fn, spl_autoload_extensions_fn, spl_autoload_fn, spl_autoload_functions_fn,
    spl_autoload_register_fn, spl_autoload_unregister_fn,
};

/// Register all SPL extension functions
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"spl");
    // Autoload functions
    vm.register_function(b"spl_autoload_register", spl_autoload_register_fn);
    vm.register_function(b"spl_autoload_unregister", spl_autoload_unregister_fn);
    vm.register_function(b"spl_autoload_functions", spl_autoload_functions_fn);
    vm.register_function(b"spl_autoload", spl_autoload_fn);
    vm.register_function(b"spl_autoload_extensions", spl_autoload_extensions_fn);
    vm.register_function(b"spl_autoload_call", spl_autoload_call_fn);

    // SPL utility functions
    vm.register_function(b"spl_classes", spl_classes_fn);
    vm.register_function(b"spl_object_id", spl_object_id_fn);
    vm.register_function(b"spl_object_hash", spl_object_hash_fn);

    // Iterator functions
    vm.register_function(b"iterator_to_array", iterator_to_array_fn);
    vm.register_function(b"iterator_count", iterator_count_fn);
    vm.register_function(b"iterator_apply", iterator_apply_fn);

    // Class introspection functions
    vm.register_function(b"class_implements", class_implements_fn);
    vm.register_function(b"class_parents", class_parents_fn);
    vm.register_function(b"class_uses", class_uses_fn);

    // Native constructors
    vm.object_construct_handlers.insert(
        b"splfileobject".to_vec(),
        goro_core::spl_file::spl_file_object_construct_native,
    );
    vm.object_construct_handlers.insert(
        b"spltempfileobject".to_vec(),
        goro_core::spl_file::spl_file_object_construct_native,
    );
    vm.object_construct_handlers.insert(
        b"splfileinfo".to_vec(),
        goro_core::spl_file::spl_file_info_construct_native,
    );
    vm.object_construct_handlers.insert(
        b"splfixedarray".to_vec(),
        goro_core::spl_fixed::spl_fixed_construct_native,
    );
    for cls in goro_core::spl_iter::ITER_CLASSES {
        vm.object_construct_handlers
            .insert(cls.to_vec(), goro_core::spl_iter::spl_iter_construct_native);
    }

    // Register parameter names for named argument support
    register_param_names(vm);
}

fn register_param_names(vm: &mut Vm) {
    macro_rules! params {
        ($name:expr, $($p:expr),+) => {
            vm.builtin_param_names.insert($name.to_vec(), vec![$($p.to_vec()),+]);
        }
    }

    params!(b"spl_autoload_register", b"callback", b"throw", b"prepend");
    params!(b"spl_autoload_unregister", b"callback");
    params!(b"spl_autoload", b"class", b"file_extensions");
    params!(b"spl_autoload_extensions", b"file_extensions");
    params!(b"spl_autoload_call", b"class");
    params!(b"spl_object_id", b"object");
    params!(b"spl_object_hash", b"object");
    params!(b"iterator_to_array", b"iterator", b"preserve_keys");
    params!(b"iterator_count", b"iterator");
    params!(b"iterator_apply", b"iterator", b"callback", b"args");
    params!(b"class_implements", b"object_or_class", b"autoload");
    params!(b"class_parents", b"object_or_class", b"autoload");
    params!(b"class_uses", b"object_or_class", b"autoload");
}

// --- Autoload functions ---

// --- SPL utility functions ---

fn spl_classes_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // Returns an array of all available SPL classes
    let spl_class_names = [
        "AppendIterator",
        "ArrayIterator",
        "ArrayObject",
        "BadFunctionCallException",
        "BadMethodCallException",
        "CachingIterator",
        "CallbackFilterIterator",
        "DirectoryIterator",
        "DomainException",
        "EmptyIterator",
        "FilesystemIterator",
        "FilterIterator",
        "GlobIterator",
        "InfiniteIterator",
        "InvalidArgumentException",
        "IteratorIterator",
        "LengthException",
        "LimitIterator",
        "LogicException",
        "MultipleIterator",
        "NoRewindIterator",
        "OutOfBoundsException",
        "OutOfRangeException",
        "OverflowException",
        "ParentIterator",
        "RangeException",
        "RecursiveArrayIterator",
        "RecursiveCachingIterator",
        "RecursiveCallbackFilterIterator",
        "RecursiveDirectoryIterator",
        "RecursiveFilterIterator",
        "RecursiveIteratorIterator",
        "RecursiveRegexIterator",
        "RecursiveTreeIterator",
        "RegexIterator",
        "RuntimeException",
        "SplDoublyLinkedList",
        "SplFileInfo",
        "SplFileObject",
        "SplFixedArray",
        "SplHeap",
        "SplMaxHeap",
        "SplMinHeap",
        "SplObjectStorage",
        "SplPriorityQueue",
        "SplQueue",
        "SplStack",
        "SplTempFileObject",
        "UnderflowException",
        "UnexpectedValueException",
    ];

    let mut result = PhpArray::new();
    for name in &spl_class_names {
        let key = PhpString::from_bytes(name.as_bytes());
        let val = PhpString::from_bytes(name.as_bytes());
        result.set(ArrayKey::String(key), Value::String(val));
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn spl_object_hash_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(obj)) = args.first() {
        let id = obj.borrow().object_id;
        Ok(Value::String(PhpString::from_string(format!(
            "{:032x}",
            id
        ))))
    } else {
        Err(VmError {
            message: "spl_object_hash() expects an object".into(),
            line: 0,
        })
    }
}

fn spl_object_id_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(obj)) = args.first() {
        Ok(Value::Long(obj.borrow().object_id as i64))
    } else {
        Err(VmError {
            message: "spl_object_id() expects an object".into(),
            line: 0,
        })
    }
}

// --- Iterator functions ---

fn iterator_to_array_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Push a call-stack frame so any exception thrown while iterating the
    // generator/iterator captures `iterator_to_array(...)` in its backtrace.
    vm.call_stack.push((
        "iterator_to_array".to_string(),
        vm.current_file.clone(),
        vm.current_line,
        args.to_vec(),
        false,
    ));
    let r = iterator_to_array_impl(vm, args);
    vm.call_stack.pop();
    r
}

fn iterator_to_array_impl(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let use_keys = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
    // Type check first argument
    if let Some(first) = args.first() {
        let type_name = match first {
            Value::Array(_) | Value::Object(_) | Value::Generator(_) => None,
            Value::String(_) => Some("string"),
            Value::Long(_) => Some("int"),
            Value::Double(_) => Some("float"),
            Value::True | Value::False => Some("bool"),
            Value::Null => Some("null"),
            _ => Some("unknown"),
        };
        if let Some(tn) = type_name {
            let msg = format!("iterator_to_array(): Argument #1 ($iterator) must be of type Traversable|array, {} given", tn);
            let line = vm.current_line;
            let exc = vm.create_exception(b"TypeError", &msg, line);
            vm.current_exception = Some(exc);
            return Ok(Value::Null);
        }
    }
    match args.first() {
        Some(Value::Array(arr)) => {
            if use_keys {
                Ok(Value::Array(arr.clone()))
            } else {
                // Reindex with sequential integer keys.
                let mut result = PhpArray::new();
                for (_, v) in arr.borrow().iter() {
                    result.push(v.clone());
                }
                Ok(Value::Array(Rc::new(RefCell::new(result))))
            }
        }
        Some(val @ Value::Object(obj)) => {
            // IteratorAggregate: dispatch to getIterator()'s return value.
            let cls_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            let mut is_aggregate = false;
            let mut cur = cls_lower.clone();
            for _ in 0..32 {
                if let Some(c) = vm.classes.get(&cur) {
                    if c.interfaces.iter().any(|n| n.eq_ignore_ascii_case(b"IteratorAggregate")) {
                        is_aggregate = true;
                        break;
                    }
                    if c.interfaces.iter().any(|n| n.eq_ignore_ascii_case(b"Iterator")) {
                        break;
                    }
                    if let Some(p) = &c.parent {
                        cur = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                    } else { break; }
                } else { break; }
            }
            // Extension-provided aggregates (e.g. DatePeriod)
            if !is_aggregate {
                if let Some((native, _)) = vm.find_native_class_handler(&cls_lower) {
                    is_aggregate = vm.builtin_implements_interface(&native, b"iteratoraggregate");
                }
            }
            if is_aggregate {
                let iter_val = vm.call_object_method(val, b"getIterator", &[]).unwrap_or(Value::Null);
                return iterator_to_array_impl(vm, &[iter_val, if use_keys { Value::True } else { Value::False }]);
            }
            let mut result = PhpArray::new();
            // Call rewind, then iterate with valid/current/key/next
            vm.call_object_method(val, b"rewind", &[]);
            for _ in 0..100000 {
                let valid = vm.call_object_method(val, b"valid", &[]).unwrap_or(Value::False);
                if !valid.is_truthy() { break; }
                let current = vm.call_object_method(val, b"current", &[]).unwrap_or(Value::Null);
                if use_keys {
                    let key = vm.call_object_method(val, b"key", &[]).unwrap_or(Value::Null);
                    match key {
                        Value::Long(n) => result.set(ArrayKey::Int(n), current),
                        Value::String(s) => result.set(ArrayKey::String(s), current),
                        _ => result.push(current),
                    }
                } else {
                    result.push(current);
                }
                vm.call_object_method(val, b"next", &[]);
            }
            Ok(Value::Array(Rc::new(RefCell::new(result))))
        }
        Some(Value::Generator(gen_rc)) => {
            let mut result = PhpArray::new();
            // Advance to first yield if needed
            {
                let mut gen_init = gen_rc.borrow_mut();
                if gen_init.state == goro_core::generator::GeneratorState::Created {
                    let _ = gen_init.resume(vm);
                }
            }
            for _ in 0..100000 {
                let gen_b = gen_rc.borrow();
                if gen_b.state == goro_core::generator::GeneratorState::Completed { break; }
                let value = gen_b.current_value.clone();
                let key = gen_b.current_key.clone();
                drop(gen_b);
                if use_keys {
                    match key {
                        Value::Long(n) => result.set(ArrayKey::Int(n), value),
                        Value::String(s) => {
                            // PHP coerces numeric strings to int keys in array
                            // context. Match the engine's behaviour.
                            let key_bytes = s.as_bytes();
                            let trimmed = std::str::from_utf8(key_bytes).ok().and_then(|t| t.parse::<i64>().ok());
                            match trimmed {
                                Some(n) if n.to_string().as_bytes() == key_bytes => {
                                    result.set(ArrayKey::Int(n), value)
                                }
                                _ => result.set(ArrayKey::String(s), value),
                            }
                        }
                        Value::Double(f) => {
                            // PHP emits a deprecation notice for non-integer
                            // float keys and truncates toward zero.
                            if f.fract() != 0.0 {
                                vm.emit_deprecated(&format!(
                                    "Implicit conversion from float {} to int loses precision",
                                    f
                                ));
                            }
                            result.set(ArrayKey::Int(f as i64), value);
                        }
                        Value::True => result.set(ArrayKey::Int(1), value),
                        Value::False => result.set(ArrayKey::Int(0), value),
                        Value::Null => {
                            vm.emit_deprecated("Using null as an array offset is deprecated, use an empty string instead");
                            result.set(ArrayKey::String(PhpString::from_bytes(b"")), value);
                        }
                        Value::Array(_) => {
                            // PHP throws TypeError "Cannot access offset of
                            // type array on array".
                            let line = vm.current_line;
                            let exc = vm.create_exception(
                                b"TypeError",
                                "Cannot access offset of type array on array",
                                line,
                            );
                            vm.current_exception = Some(exc);
                            return Ok(Value::Array(Rc::new(RefCell::new(result))));
                        }
                        Value::Object(_) => {
                            let line = vm.current_line;
                            let exc = vm.create_exception(
                                b"TypeError",
                                "Cannot access offset of type object on array",
                                line,
                            );
                            vm.current_exception = Some(exc);
                            return Ok(Value::Array(Rc::new(RefCell::new(result))));
                        }
                        _ => result.push(value),
                    }
                } else {
                    result.push(value);
                }
                let mut gen_bm = gen_rc.borrow_mut();
                gen_bm.write_send_value();
                let _ = gen_bm.resume(vm);
            }
            Ok(Value::Array(Rc::new(RefCell::new(result))))
        }
        _ => Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
    }
}

fn iterator_count_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Push a call-stack frame so any exception thrown while iterating the
    // iterator (or by the type-check below) captures `iterator_count(...)` in
    // its backtrace.
    vm.call_stack.push((
        "iterator_count".to_string(),
        vm.current_file.clone(),
        vm.current_line,
        args.to_vec(),
        false,
    ));
    let r = iterator_count_impl(vm, args);
    vm.call_stack.pop();
    r
}

fn iterator_count_impl(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Type check first argument
    if let Some(first) = args.first() {
        let type_name = match first {
            Value::Array(_) | Value::Object(_) | Value::Generator(_) => None,
            Value::String(_) => Some("string"),
            Value::Long(_) => Some("int"),
            Value::Double(_) => Some("float"),
            Value::True | Value::False => Some("bool"),
            Value::Null => Some("null"),
            _ => Some("unknown"),
        };
        if let Some(tn) = type_name {
            let msg = format!("iterator_count(): Argument #1 ($iterator) must be of type Traversable|array, {} given", tn);
            let line = vm.current_line;
            let exc = vm.create_exception(b"TypeError", &msg, line);
            vm.current_exception = Some(exc);
            return Ok(Value::Null);
        }
    }
    match args.first() {
        Some(Value::Array(arr)) => Ok(Value::Long(arr.borrow().len() as i64)),
        Some(val @ Value::Object(_)) => {
            let mut count: i64 = 0;
            vm.call_object_method(val, b"rewind", &[]);
            for _ in 0..100000 {
                let valid = vm.call_object_method(val, b"valid", &[]).unwrap_or(Value::False);
                if !valid.is_truthy() { break; }
                count += 1;
                vm.call_object_method(val, b"next", &[]);
            }
            Ok(Value::Long(count))
        }
        Some(Value::Generator(gen_rc)) => {
            let mut count: i64 = 0;
            {
                let mut gen_init = gen_rc.borrow_mut();
                if gen_init.state == goro_core::generator::GeneratorState::Created {
                    let _ = gen_init.resume(vm);
                }
            }
            loop {
                let gen_check = gen_rc.borrow();
                if gen_check.state == goro_core::generator::GeneratorState::Completed { break; }
                count += 1;
                drop(gen_check);
                let mut gen_bm = gen_rc.borrow_mut();
                gen_bm.write_send_value();
                let _ = gen_bm.resume(vm);
            }
            Ok(Value::Long(count))
        }
        _ => Ok(Value::Long(0)),
    }
}

fn iterator_apply_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Arg count: max 3 user args.
    if args.len() > 3 {
        let msg = format!("iterator_apply() expects at most 3 arguments, {} given", args.len());
        let line = vm.current_line;
        let exc = vm.create_exception(b"ArgumentCountError", &msg, line);
        vm.current_exception = Some(exc);
        return Ok(Value::Null);
    }

    let iterator = args.first().unwrap_or(&Value::Null).clone();
    let callback = args.get(1).unwrap_or(&Value::Null).clone();
    // Validate $args is array|null
    let cb_args = match args.get(2) {
        None | Some(Value::Null) => None,
        Some(Value::Array(a)) => Some(Value::Array(a.clone())),
        Some(other) => {
            let type_name = match other {
                Value::String(_) => "string",
                Value::Long(_) => "int",
                Value::Double(_) => "float",
                Value::True | Value::False => "bool",
                Value::Object(_) => "object",
                _ => "unknown",
            };
            let msg = format!(
                "iterator_apply(): Argument #3 ($args) must be of type ?array, {} given",
                type_name
            );
            let line = vm.current_line;
            let exc = vm.create_exception(b"TypeError", &msg, line);
            vm.current_exception = Some(exc);
            return Ok(Value::Null);
        }
    };

    // Validate callback before iterating.
    if let Value::String(s) = &callback {
        let cb_name = s.as_bytes().to_vec();
        let cb_lower: Vec<u8> = cb_name.iter().map(|b| b.to_ascii_lowercase()).collect();
        let exists = vm.functions.contains_key(cb_lower.as_slice())
            || vm.user_functions.contains_key(cb_lower.as_slice());
        if !exists {
            let msg = format!(
                "iterator_apply(): Argument #2 ($callback) must be a valid callback, function \"{}\" not found or invalid function name",
                String::from_utf8_lossy(s.as_bytes())
            );
            let line = vm.current_line;
            let exc = vm.create_exception(b"TypeError", &msg, line);
            vm.current_exception = Some(exc);
            return Ok(Value::Null);
        }
    }

    // Iterate over the iterator and call the callback for each element
    if let Value::Object(_) = &iterator {
        // Call rewind on the iterator. Propagate exceptions thrown by rewind
        // back to the caller (don't keep iterating).
        let _ = vm.call_object_method(&iterator, b"rewind", &[]);
        if vm.current_exception.is_some() {
            return Ok(Value::Null);
        }

        let mut count = 0i64;
        loop {
            // Check valid()
            let valid = vm.call_object_method(&iterator, b"valid", &[]);
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            match valid {
                Some(v) if !v.is_truthy() => break,
                None => break,
                _ => {}
            }

            // Call the callback via the registered call_user_func
            let call_user_func = vm.functions.get(b"call_user_func".as_slice()).copied();
            if let Some(cuf) = call_user_func {
                let mut call_fn_args = vec![callback.clone()];
                if let Some(Value::Array(extra)) = &cb_args {
                    let extra_borrow = extra.borrow();
                    for (_, v) in extra_borrow.iter() {
                        call_fn_args.push(v.clone());
                    }
                }
                let result = cuf(vm, &call_fn_args)?;
                if vm.current_exception.is_some() {
                    return Ok(Value::Null);
                }
                count += 1;

                if !result.is_truthy() {
                    break;
                }
            } else {
                // Fallback: no call_user_func registered, just count
                count += 1;
            }

            // Call next()
            let _ = vm.call_object_method(&iterator, b"next", &[]);
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
        }
        Ok(Value::Long(count))
    } else {
        Ok(Value::Long(0))
    }
}

// --- Class introspection functions ---

fn class_implements_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (class_name, from_string) = match args.first().unwrap_or(&Value::Null) {
        Value::String(s) => (s.as_bytes().to_vec(), true),
        Value::Object(obj) => (obj.borrow().class_name.clone(), false),
        _ => return Ok(Value::False),
    };
    let autoload = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    if from_string {
        let exists = vm.classes.contains_key(&class_lower)
            || vm.is_known_builtin_class(&class_lower);
        if !exists {
            if autoload {
                vm.try_autoload_class(&class_name);
            }
            let still_exists = vm.classes.contains_key(&class_lower)
                || vm.is_known_builtin_class(&class_lower);
            if !still_exists {
                let suffix = if autoload { " and could not be loaded" } else { "" };
                let class_str = String::from_utf8_lossy(&class_name);
                vm.emit_warning(&format!("class_implements(): Class {} does not exist{}", class_str, suffix));
                return Ok(Value::False);
            }
        }
    }
    let mut result = PhpArray::new();

    // Get interfaces from the class definition
    if let Some(class) = vm.classes.get(&class_lower) {
        for iface in &class.interfaces {
            let iface_str = PhpString::from_vec(iface.clone());
            result.set(ArrayKey::String(iface_str.clone()), Value::String(iface_str));
        }
    }

    // Also check built-in interface implementations
    let builtins = goro_core::vm::get_builtin_interfaces(&class_lower);
    for iface in builtins {
        let iface_str = PhpString::from_vec(iface.clone());
        result.set(ArrayKey::String(iface_str.clone()), Value::String(iface_str));
    }

    // Check for implicit Stringable (class or any parent has __tostring)
    if vm.class_implements_interface(&class_lower, b"stringable") {
        let iface_str = PhpString::from_vec(b"Stringable".to_vec());
        if !result.iter().any(|(k, _)| matches!(k, ArrayKey::String(s) if s.as_bytes().eq_ignore_ascii_case(b"Stringable"))) {
            result.set(ArrayKey::String(iface_str.clone()), Value::String(iface_str));
        }
    }

    // Walk parent chain for inherited interfaces
    let mut current = class_lower.clone();
    for _ in 0..50 {
        let parent = if let Some(class) = vm.classes.get(&current) {
            class.parent.clone()
        } else {
            None
        };
        if let Some(parent_name) = parent {
            let parent_lower: Vec<u8> = parent_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if let Some(parent_class) = vm.classes.get(&parent_lower) {
                for iface in &parent_class.interfaces {
                    let iface_str = PhpString::from_vec(iface.clone());
                    result.set(ArrayKey::String(iface_str.clone()), Value::String(iface_str));
                }
            }
            let parent_builtins = goro_core::vm::get_builtin_interfaces(&parent_lower);
            for iface in parent_builtins {
                let iface_str = PhpString::from_vec(iface.clone());
                result.set(ArrayKey::String(iface_str.clone()), Value::String(iface_str));
            }
            current = parent_lower;
        } else {
            break;
        }
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn class_parents_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (class_name, from_string) = match args.first().unwrap_or(&Value::Null) {
        Value::String(s) => (s.as_bytes().to_vec(), true),
        Value::Object(obj) => (obj.borrow().class_name.clone(), false),
        _ => return Ok(Value::False),
    };
    let autoload = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    // If the class came from a string argument, it must exist (or be
    // loadable). PHP emits a warning and returns false otherwise.
    if from_string {
        let exists = vm.classes.contains_key(&class_lower)
            || vm.is_known_builtin_class(&class_lower);
        if !exists {
            if autoload {
                vm.try_autoload_class(&class_name);
            }
            let still_exists = vm.classes.contains_key(&class_lower)
                || vm.is_known_builtin_class(&class_lower);
            if !still_exists {
                let suffix = if autoload { " and could not be loaded" } else { "" };
                let class_str = String::from_utf8_lossy(&class_name);
                vm.emit_warning(&format!("class_parents(): Class {} does not exist{}", class_str, suffix));
                return Ok(Value::False);
            }
        }
    }
    let mut result = PhpArray::new();

    let mut current = class_lower;
    for _ in 0..50 {
        let parent = if let Some(class) = vm.classes.get(&current) {
            class.parent.clone()
        } else {
            // Check built-in parent chains
            let bp = goro_core::vm::get_builtin_parent(&current);
            bp.map(|p| p.to_vec())
        };
        if let Some(parent_name) = parent {
            let parent_lower: Vec<u8> = parent_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            let display_name = if let Some(class) = vm.classes.get(&parent_lower) {
                class.name.clone()
            } else {
                // Canonicalize built-in class names
                goro_core::vm::canonicalize_class_name(&parent_lower)
            };
            let name_str = PhpString::from_vec(display_name);
            result.set(ArrayKey::String(name_str.clone()), Value::String(name_str));
            current = parent_lower;
        } else {
            break;
        }
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn class_uses_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let class_name = match args.first().unwrap_or(&Value::Null) {
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Object(obj) => obj.borrow().class_name.clone(),
        _ => return Ok(Value::False),
    };
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    let mut result = PhpArray::new();

    if let Some(class) = vm.classes.get(&class_lower) {
        for trait_name in &class.traits {
            let trait_str = PhpString::from_vec(trait_name.clone());
            result.set(ArrayKey::String(trait_str.clone()), Value::String(trait_str));
        }
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}
