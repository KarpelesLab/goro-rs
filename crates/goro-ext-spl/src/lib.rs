use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

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
        // Default autoloader (spl_autoload) - record and return
        // Check for duplicate (spl_autoload is unique by string name)
        let already = vm.autoload_functions.iter().any(|v| match v {
            Value::String(s) => s.as_bytes() == b"spl_autoload",
            _ => false,
        });
        if !already {
            vm.autoload_functions.push(Value::String(PhpString::from_bytes(b"spl_autoload")));
        }
        return Ok(Value::True);
    }
    // Validate callback (string callback must point to an existing function).
    if let Value::String(s) = &callback {
        let cb_name = s.as_bytes().to_vec();
        let cb_lower: Vec<u8> = cb_name.iter().map(|b| b.to_ascii_lowercase()).collect();
        // spl_autoload_call can never be registered as an autoloader.
        if cb_lower == b"spl_autoload_call" {
            let msg = "spl_autoload_register(): Argument #1 ($callback) must not be the spl_autoload_call() function";
            let line = vm.current_line;
            let exc = vm.create_exception(b"ValueError", msg, line);
            vm.current_exception = Some(exc);
            return Ok(Value::False);
        }
        let exists = vm.functions.contains_key(cb_lower.as_slice())
            || vm.user_functions.contains_key(cb_lower.as_slice())
            || cb_lower == b"spl_autoload";
        if !exists {
            let msg = format!(
                "spl_autoload_register(): Argument #1 ($callback) must be a valid callback or null, function \"{}\" not found or invalid function name",
                String::from_utf8_lossy(s.as_bytes())
            );
            let line = vm.current_line;
            let exc = vm.create_exception(b"TypeError", &msg, line);
            vm.current_exception = Some(exc);
            return Ok(Value::False);
        }
    }
    // Optional second arg: $do_throw (default true). PHP emits a notice
    // when this is explicitly set to false (the parameter is ignored —
    // spl_autoload_register() always throws on invalid input).
    if let Some(v) = args.get(1) {
        if !v.to_bool() {
            let line = vm.current_line;
            vm.emit_notice_at(
                "spl_autoload_register(): Argument #2 ($do_throw) has been ignored, spl_autoload_register() will always throw",
                line,
            );
        }
    }
    // Optional third arg: prepend (default false)
    let prepend = args.get(2).map_or(false, |v| v.to_bool());

    // Deduplicate: don't register the same callback twice. Compare by
    // string name (for string callbacks) and by identity for arrays/objects.
    let is_dup = vm.autoload_functions.iter().any(|existing| {
        match (existing, &callback) {
            (Value::String(a), Value::String(b)) => a.as_bytes().eq_ignore_ascii_case(b.as_bytes()),
            (Value::Array(a), Value::Array(b)) => {
                let ab = a.borrow();
                let bb = b.borrow();
                ab.len() == bb.len()
                    && ab.iter().zip(bb.iter()).all(|((_, va), (_, vb))| match (va, vb) {
                        (Value::String(sa), Value::String(sb)) => sa.as_bytes().eq_ignore_ascii_case(sb.as_bytes()),
                        (Value::Object(oa), Value::Object(ob)) => Rc::ptr_eq(oa, ob),
                        _ => false,
                    })
            }
            (Value::Object(a), Value::Object(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    });
    if !is_dup {
        if prepend {
            vm.autoload_functions.insert(0, callback);
        } else {
            vm.autoload_functions.push(callback);
        }
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

    // Special case: unregistering 'spl_autoload_call' clears the entire
    // autoload chain (deprecated since PHP 8.0).
    if let Value::String(s) = &callback {
        if s.as_bytes().eq_ignore_ascii_case(b"spl_autoload_call") {
            vm.emit_deprecated_at(
                "spl_autoload_unregister(): Using spl_autoload_call() as a callback for spl_autoload_unregister() is deprecated, to remove all registered autoloaders, call spl_autoload_unregister() for all values returned from spl_autoload_functions()",
                vm.current_line,
            );
            vm.autoload_functions.clear();
            return Ok(Value::True);
        }
    }

    // Find a matching entry.
    let pos = vm.autoload_functions.iter().position(|existing| {
        match (existing, &callback) {
            (Value::String(a), Value::String(b)) => a.as_bytes().eq_ignore_ascii_case(b.as_bytes()),
            (Value::Array(a), Value::Array(b)) => {
                let ab = a.borrow();
                let bb = b.borrow();
                ab.len() == bb.len()
                    && ab.iter().zip(bb.iter()).all(|((_, va), (_, vb))| match (va, vb) {
                        (Value::String(sa), Value::String(sb)) => sa.as_bytes().eq_ignore_ascii_case(sb.as_bytes()),
                        (Value::Object(oa), Value::Object(ob)) => Rc::ptr_eq(oa, ob),
                        _ => false,
                    })
            }
            (Value::Object(a), Value::Object(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    });
    if let Some(p) = pos {
        vm.autoload_functions.remove(p);
        Ok(Value::True)
    } else {
        Ok(Value::False)
    }
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
