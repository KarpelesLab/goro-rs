use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

/// Register all SPL extension functions
pub fn register(vm: &mut Vm) {
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

fn spl_autoload_register_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let callback = args.first().cloned().unwrap_or(Value::Null);
    if matches!(callback, Value::Null) {
        // Default autoloader (spl_autoload) - not implemented, just return
        return Ok(Value::True);
    }
    // Optional second arg: throw (default true) - ignored for now
    // Optional third arg: prepend (default false)
    let prepend = args.get(2).map_or(false, |v| v.to_bool());
    if prepend {
        vm.autoload_functions.insert(0, callback);
    } else {
        vm.autoload_functions.push(callback);
    }
    Ok(Value::True)
}

fn spl_autoload_functions_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    for (i, func) in vm.autoload_functions.iter().enumerate() {
        arr.set(ArrayKey::Int(i as i64), func.clone());
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn spl_autoload_unregister_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let callback = args.first().cloned().unwrap_or(Value::Null);
    // Try to remove by matching the callback
    // Simple approach: remove first matching entry
    if let Some(pos) = vm.autoload_functions.iter().position(|f| {
        // Compare by string representation for simplicity
        format!("{:?}", f) == format!("{:?}", callback)
    }) {
        vm.autoload_functions.remove(pos);
    }
    Ok(Value::True)
}

fn spl_autoload_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // spl_autoload() is the default autoloader.
    // It tries to load a file named after the class (lowercased) with registered extensions.
    // For now, this is a stub since file-based autoloading requires include path support.
    Ok(Value::Null)
}

fn spl_autoload_extensions_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // spl_autoload_extensions() gets/sets the file extensions for spl_autoload
    // Default is ".inc,.php"
    if args.is_empty() {
        Ok(Value::String(PhpString::from_bytes(b".inc,.php")))
    } else {
        // Setting extensions - stub for now, just return the provided value
        let ext = args[0].to_php_string();
        Ok(Value::String(ext))
    }
}

fn spl_autoload_call_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // spl_autoload_call() calls all registered autoloaders for the given class
    let class_name = args.first().map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b""));
    vm.try_autoload_class(class_name.as_bytes());
    Ok(Value::Null)
}

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
    let use_keys = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
    match args.first() {
        Some(Value::Array(arr)) => Ok(Value::Array(arr.clone())),
        Some(val @ Value::Object(_)) => {
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
                        Value::String(s) => result.set(ArrayKey::String(s), value),
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
    let iterator = args.first().unwrap_or(&Value::Null).clone();
    let callback = args.get(1).unwrap_or(&Value::Null).clone();
    let cb_args = args.get(2).cloned();

    // Iterate over the iterator and call the callback for each element
    if let Value::Object(_) = &iterator {
        // Call rewind on the iterator
        let _ = vm.call_object_method(&iterator, b"rewind", &[iterator.clone()]);

        let mut count = 0i64;
        loop {
            // Check valid()
            let valid = vm.call_object_method(&iterator, b"valid", &[iterator.clone()]);
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
                count += 1;

                if !result.is_truthy() {
                    break;
                }
            } else {
                // Fallback: no call_user_func registered, just count
                count += 1;
            }

            // Call next()
            let _ = vm.call_object_method(&iterator, b"next", &[iterator.clone()]);
        }
        Ok(Value::Long(count))
    } else {
        Ok(Value::Long(0))
    }
}

// --- Class introspection functions ---

fn class_implements_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let class_name = match args.first().unwrap_or(&Value::Null) {
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Object(obj) => obj.borrow().class_name.clone(),
        _ => return Ok(Value::False),
    };
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
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
    let class_name = match args.first().unwrap_or(&Value::Null) {
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Object(obj) => obj.borrow().class_name.clone(),
        _ => return Ok(Value::False),
    };
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
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
