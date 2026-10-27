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
    for cls in [
        &b"arrayobject"[..],
        b"arrayiterator",
        b"recursivearrayiterator",
    ] {
        vm.object_construct_handlers.insert(
            cls.to_vec(),
            goro_core::spl_array::spl_array_construct_native,
        );
    }
    vm.object_construct_handlers.insert(
        b"multipleiterator".to_vec(),
        goro_core::spl_storage::spl_multiple_construct_native,
    );
    vm.object_construct_handlers.insert(
        b"splfixedarray".to_vec(),
        goro_core::spl_fixed::spl_fixed_construct_native,
    );
    for cls in goro_core::spl_dir::DIR_CLASSES {
        vm.object_construct_handlers
            .insert(cls.to_vec(), goro_core::spl_dir::spl_dir_construct_native);
    }
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
    // The order of spl_classes() in php-src (SPL_LIST_CLASSES), interfaces included.
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
        "OuterIterator",
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
        "RecursiveIterator",
        "RecursiveIteratorIterator",
        "RecursiveRegexIterator",
        "RecursiveTreeIterator",
        "RegexIterator",
        "RuntimeException",
        "SeekableIterator",
        "SplDoublyLinkedList",
        "SplFileInfo",
        "SplFileObject",
        "SplFixedArray",
        "SplHeap",
        "SplMinHeap",
        "SplMaxHeap",
        "SplObjectStorage",
        "SplObserver",
        "SplPriorityQueue",
        "SplQueue",
        "SplStack",
        "SplSubject",
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

/// The zpp error of spl_object_id()/spl_object_hash()'s `object $object`.
fn object_arg_error(vm: &mut Vm, name: &str, args: &[Value]) -> VmError {
    let (class, msg): (&[u8], String) = if args.len() != 1 {
        (
            b"ArgumentCountError",
            format!(
                "{}() expects exactly 1 argument, {} given",
                name,
                args.len()
            ),
        )
    } else {
        (
            b"TypeError",
            format!(
                "{}(): Argument #1 ($object) must be of type object, {} given",
                name,
                Vm::value_type_name(&args[0].deref())
            ),
        )
    };
    let exc = vm.create_exception(class, &msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError {
        message: msg,
        line: vm.current_line,
    }
}

fn spl_object_hash_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = match args.first() {
        Some(Value::Generator(g)) => Some(vm.generator_handle(g)),
        Some(Value::Object(obj)) => Some(obj.borrow().object_id),
        _ => None,
    };
    match id {
        Some(id) => Ok(Value::String(PhpString::from_string(format!(
            "{:016x}0000000000000000",
            id
        )))),
        None => Err(object_arg_error(vm, "spl_object_hash", args)),
    }
}

fn spl_object_id_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Generator(g)) = args.first() {
        return Ok(Value::Long(vm.generator_handle(g) as i64));
    }
    if let Some(Value::Object(obj)) = args.first() {
        Ok(Value::Long(obj.borrow().object_id as i64))
    } else {
        Err(object_arg_error(vm, "spl_object_id", args))
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
    let r = iterator_to_array_impl(vm, args, true);
    vm.call_stack.pop();
    r
}

/// `take_handle`: the zend_object_iterator spl_iterator_apply() gets takes
/// an object handle (released after the iterator it wraps).
fn iterator_to_array_impl(
    vm: &mut Vm,
    args: &[Value],
    take_handle: bool,
) -> Result<Value, VmError> {
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
            let msg = format!(
                "iterator_to_array(): Argument #1 ($iterator) must be of type Traversable|array, {} given",
                tn
            );
            let line = vm.current_line;
            let exc = vm.create_exception(b"TypeError", &msg, line);
            vm.current_exception = Some(exc);
            return Ok(Value::Null);
        }
    }
    // (bool $preserve_keys, parsed after the iterator)
    let coerced;
    let args = if args.len() > 1 && !matches!(args[1].deref(), Value::True | Value::False) {
        use goro_core::arginfo::{CHECK_ARITY, CHECK_PAST_STOP, CHECK_TYPES};
        let flags = CHECK_ARITY | CHECK_TYPES | CHECK_PAST_STOP;
        coerced = vm.zpp_self_check_flags(b"iterator_to_array", args, flags)?;
        &coerced[..]
    } else {
        args
    };
    let use_keys = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
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
            let mut is_aggregate = !vm.obj_instanceof(val, b"iterator")
                && vm.obj_instanceof(val, b"iteratoraggregate");
            // Extension-provided aggregates (e.g. DatePeriod)
            if !is_aggregate {
                let cls_lower: Vec<u8> = obj
                    .borrow()
                    .class_name
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect();
                if let Some((native, _)) = vm.find_native_class_handler(&cls_lower) {
                    is_aggregate = vm.builtin_implements_interface(&native, b"iteratoraggregate");
                }
            }
            if is_aggregate {
                // (an internal class's get_iterator handler: the one
                // zend_object_iterator, as for foreach)
                vm.foreach_native_iter = u8::from(obj.borrow().native.is_some());
                let iter_val = vm.spl_resolve_traversable(val);
                let native_iter = std::mem::take(&mut vm.foreach_native_iter) == 2;
                if vm.current_exception.is_some() {
                    return Ok(Value::Null);
                }
                let handle = (take_handle && !native_iter).then(|| vm.internal_iterator_handle());
                let r = iterator_to_array_impl(
                    vm,
                    &[iter_val, if use_keys { Value::True } else { Value::False }],
                    false,
                );
                drop(handle);
                return r;
            }
            let _handle = take_handle.then(|| vm.internal_iterator_handle());
            let it = object_iterator(vm, obj).unwrap_or_else(|| val.clone());
            iterator_to_array_iterate(vm, &it, use_keys)
        }
        Some(g @ Value::Generator(_)) => {
            let _handle = take_handle.then(|| vm.internal_iterator_handle());
            if !vm.generator_traverse_start(g) {
                return Ok(Value::Null);
            }
            iterator_to_array_iterate(vm, g, use_keys)
        }
        _ => Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
    }
}

/// The independent zend_object_iterator a class's get_iterator creates
/// (SplDoublyLinkedList: its own cursor), when it is not the object itself.
fn object_iterator(vm: &mut Vm, obj: &Rc<RefCell<goro_core::object::PhpObject>>) -> Option<Value> {
    vm.dll_uses_internal_iterator(obj)
        .then(|| vm.dll_new_iterator_at(obj, false))
}

/// spl_iterator_apply() with spl_iterator_to_array_apply(): walk the
/// iterator through its zend_object_iterator funcs (a generator is already
/// rewound), stopping at the first exception.
fn iterator_to_array_iterate(vm: &mut Vm, it: &Value, use_keys: bool) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    if matches!(it, Value::Object(_)) {
        vm.it_rewind(it);
    }
    while vm.current_exception.is_none() && vm.it_valid(it) {
        if vm.current_exception.is_some() {
            break;
        }
        // (ZVAL_DEREF: an element reference is stored by value)
        let value = vm.it_current_data(it).deref();
        if vm.current_exception.is_some() {
            break;
        }
        if use_keys {
            let key = vm.it_key(it);
            if vm.current_exception.is_some() || !array_set_zval_key(vm, &mut result, key, value) {
                break;
            }
        } else {
            result.push(value);
        }
        vm.it_next(it);
    }
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

/// array_set_zval_key(): store `value` under an iterator key.
fn array_set_zval_key(vm: &mut Vm, result: &mut PhpArray, key: Value, value: Value) -> bool {
    match key.deref() {
        Value::Long(n) => result.set(ArrayKey::Int(n), value),
        Value::String(s) => {
            let k = Vm::value_to_array_key(Value::String(s));
            result.set(k, value)
        }
        Value::Double(f) => {
            // zend_dval_to_lval_safe()
            if f.fract() != 0.0 {
                vm.emit_deprecated(&format!(
                    "Implicit conversion from float {} to int loses precision",
                    goro_core::value::format_php_float_repr(f)
                ));
            }
            result.set(ArrayKey::Int(f as i64), value);
        }
        Value::True => result.set(ArrayKey::Int(1), value),
        Value::False => result.set(ArrayKey::Int(0), value),
        Value::Resource(r) => {
            let id = r.id();
            vm.emit_warning(&format!(
                "Resource ID#{} used as offset, casting to integer ({})",
                id, id
            ));
            result.set(ArrayKey::Int(id), value);
        }
        Value::Null | Value::Undef => {
            vm.emit_deprecated(
                "Using null as an array offset is deprecated, use an empty string instead",
            );
            if vm.current_exception.is_some() {
                return false;
            }
            result.set(ArrayKey::String(PhpString::from_bytes(b"")), value);
        }
        o => {
            let line = vm.current_line;
            let msg = format!(
                "Cannot access offset of type {} on array",
                Vm::value_type_name(&o)
            );
            let exc = vm.create_exception(b"TypeError", &msg, line);
            vm.current_exception = Some(exc);
            return false;
        }
    }
    true
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

/// iterator_count() of a resolved Traversable: its zend_object_iterator
/// takes a handle, released after what it iterates.
fn iterator_count_resolved(vm: &mut Vm, resolved: Option<Value>) -> Result<Value, VmError> {
    let handle = matches!(resolved, Some(Value::Object(_) | Value::Generator(_)))
        .then(|| vm.internal_iterator_handle());
    let r = iterator_count_iterate(vm, resolved.as_ref());
    drop(resolved);
    drop(handle);
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
            let msg = format!(
                "iterator_count(): Argument #1 ($iterator) must be of type Traversable|array, {} given",
                tn
            );
            let line = vm.current_line;
            let exc = vm.create_exception(b"TypeError", &msg, line);
            vm.current_exception = Some(exc);
            return Ok(Value::Null);
        }
    }
    match args.first() {
        Some(v @ Value::Object(_)) => {
            let Some((r, handle)) = spl_get_iterator(vm, v) else {
                return Ok(Value::Null);
            };
            let res = iterator_count_iterate(vm, Some(&r));
            drop(r);
            drop(handle);
            res
        }
        other => iterator_count_resolved(vm, other.cloned()),
    }
}

fn iterator_count_iterate(vm: &mut Vm, resolved: Option<&Value>) -> Result<Value, VmError> {
    match resolved {
        Some(Value::Array(arr)) => Ok(Value::Long(arr.borrow().len() as i64)),
        Some(it @ (Value::Object(_) | Value::Generator(_))) => {
            // spl_iterator_apply() with spl_iterator_count_apply()
            let own = match it {
                Value::Object(o) => object_iterator(vm, o),
                _ => None,
            };
            let it = own.as_ref().unwrap_or(it);
            let mut count: i64 = 0;
            if let Value::Generator(_) = it {
                if !vm.generator_traverse_start(it) {
                    return Ok(Value::Null);
                }
            } else {
                vm.it_rewind(it);
            }
            while vm.current_exception.is_none() && vm.it_valid(it) {
                if vm.current_exception.is_some() {
                    break;
                }
                count += 1;
                vm.it_next(it);
            }
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            Ok(Value::Long(count))
        }
        _ => Ok(Value::Long(0)),
    }
}

fn iterator_apply_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Arg count: max 3 user args.
    if args.len() > 3 {
        let msg = format!(
            "iterator_apply() expects at most 3 arguments, {} given",
            args.len()
        );
        let line = vm.current_line;
        let exc = vm.create_exception(b"ArgumentCountError", &msg, line);
        vm.current_exception = Some(exc);
        return Ok(Value::Null);
    }

    if args.len() < 2 {
        let msg = format!(
            "iterator_apply() expects at least 2 arguments, {} given",
            args.len()
        );
        let line = vm.current_line;
        let exc = vm.create_exception(b"ArgumentCountError", &msg, line);
        vm.current_exception = Some(exc);
        return Ok(Value::Null);
    }
    let iterator = args[0].deref();
    let callback = args[1].clone();
    // Z_PARAM_OBJECT_OF_CLASS(zend_ce_traversable)
    let traversable = match &iterator {
        Value::Generator(_) => true,
        Value::Object(o) => {
            let cls = o.borrow().class_name.to_ascii_lowercase();
            vm.class_implements_interface(&cls, b"traversable")
        }
        _ => false,
    };
    if !traversable {
        let msg = format!(
            "iterator_apply(): Argument #1 ($iterator) must be of type Traversable, {} given",
            Vm::value_type_name(&iterator)
        );
        let line = vm.current_line;
        let exc = vm.create_exception(b"TypeError", &msg, line);
        vm.current_exception = Some(exc);
        return Ok(Value::Null);
    }
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
    if let Some(e) = vm.callable_error(&callback) {
        if vm.current_exception.is_none() {
            let msg = format!(
                "iterator_apply(): Argument #2 ($callback) must be a valid callback, {}",
                e
            );
            let line = vm.current_line;
            let exc = vm.create_exception(b"TypeError", &msg, line);
            vm.current_exception = Some(exc);
        }
        return Ok(Value::Null);
    }

    // spl_iterator_apply(): the Traversable's zend_object_iterator (an
    // aggregate's getIterator() result's)
    let Some((resolved, handle)) = spl_get_iterator(vm, &iterator) else {
        return Ok(Value::Null);
    };
    let own = match &resolved {
        Value::Object(o) => object_iterator(vm, o),
        _ => None,
    };
    let it = own.as_ref().unwrap_or(&resolved).clone();
    let r = iterator_apply_iterate(vm, &it, &callback, cb_args.as_ref());
    drop(it);
    drop(own);
    drop(resolved);
    drop(handle);
    r
}

/// The zend_object_iterator spl_iterator_apply() gets for a Traversable:
/// what an aggregate's getIterator() returns, and the handle the iterator
/// takes (an internal class's get_iterator handler: the InternalIterator
/// its getIterator() makes is that one object). None on an exception.
fn spl_get_iterator(vm: &mut Vm, v: &Value) -> Option<(Value, Option<Value>)> {
    match v {
        Value::Object(o) => {
            vm.foreach_native_iter = u8::from(o.borrow().native.is_some());
            let r = vm.spl_resolve_traversable(v);
            let native_iter = std::mem::take(&mut vm.foreach_native_iter) == 2;
            if vm.current_exception.is_some() {
                return None;
            }
            let handle = (!native_iter && matches!(r, Value::Object(_) | Value::Generator(_)))
                .then(|| vm.internal_iterator_handle());
            Some((r, handle))
        }
        other => {
            let handle = vm.internal_iterator_handle();
            Some((other.clone(), Some(handle)))
        }
    }
}

fn iterator_apply_iterate(
    vm: &mut Vm,
    it: &Value,
    callback: &Value,
    cb_args: Option<&Value>,
) -> Result<Value, VmError> {
    if let Value::Generator(_) = it {
        if !vm.generator_traverse_start(it) {
            return Ok(Value::Null);
        }
    } else {
        vm.it_rewind(it);
    }
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    let mut count = 0i64;
    let call_user_func = vm.functions.get(b"call_user_func".as_slice()).copied();
    let deprecated_form = !vm.is_callable_pure(callback, 0).deprecations.is_empty();
    while vm.it_valid(it) {
        if vm.current_exception.is_some() {
            return Ok(Value::Null);
        }
        // spl_iterator_func_apply()
        count += 1;
        if let Some(cuf) = call_user_func {
            let mut call_fn_args = vec![callback.clone()];
            if let Some(Value::Array(extra)) = cb_args {
                for (_, v) in extra.borrow().iter() {
                    call_fn_args.push(v.clone());
                }
            }
            // (the callback was checked once, before the iteration)
            vm.mute_callable_deprecations = deprecated_form;
            let result = cuf(vm, &call_fn_args);
            vm.mute_callable_deprecations = false;
            let result = result?;
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            if !vm.is_true(&result) {
                break;
            }
        }
        vm.it_next(it);
        if vm.current_exception.is_some() {
            return Ok(Value::Null);
        }
    }
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    Ok(Value::Long(count))
}

// --- Class introspection functions ---

/// Common argument handling of class_implements/class_parents/class_uses:
/// Ok(Some(class name)), Ok(None) after the "does not exist" warning, or a
/// TypeError for invalid argument types.
fn class_arg(vm: &mut Vm, fname: &str, args: &[Value]) -> Result<Option<(Vec<u8>, bool)>, VmError> {
    let (class_name, from_string) = match args.first().map(|v| v.deref()).unwrap_or(Value::Null) {
        Value::String(s) => (s.as_bytes().to_vec(), true),
        Value::Object(obj) => (obj.borrow().class_name.clone(), false),
        Value::Generator(_) => (b"Generator".to_vec(), false),
        other => {
            let msg = format!(
                "{}(): Argument #1 ($object_or_class) must be of type object|string, {} given",
                fname,
                Vm::value_type_name(&other)
            );
            let line = vm.current_line;
            let exc = vm.create_exception(b"TypeError", &msg, line);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line });
        }
    };
    if from_string {
        let autoload = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
        let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
        let exists =
            vm.classes.contains_key(&class_lower) || vm.is_known_builtin_class(&class_lower);
        if !exists {
            if autoload {
                // (an exception the autoloader throws stays pending: the
                // class is still reported missing, spl_find_ce_by_name())
                vm.try_autoload_class(&class_name);
            }
            let still =
                vm.classes.contains_key(&class_lower) || vm.is_known_builtin_class(&class_lower);
            if !still {
                let suffix = if autoload {
                    " and could not be loaded"
                } else {
                    ""
                };
                vm.emit_warning(&format!(
                    "{}(): Class {} does not exist{}",
                    fname,
                    goro_core::string::msg_cstr(&class_name),
                    suffix
                ));
                return Ok(None);
            }
        }
    }
    Ok(Some((class_name, from_string)))
}

fn class_implements_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let class_name = match class_arg(vm, "class_implements", args)? {
        Some((c, _)) => c,
        None => return Ok(Value::False),
    };
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    let mut result = PhpArray::new();
    // ce->interfaces (as ReflectionClass::getInterfaceNames()): inherited
    // ones and the parents of implemented interfaces included.
    let mut add = |name: &[u8]| {
        let s = PhpString::from_vec(name.to_vec());
        if !result.iter().any(
            |(k, _)| matches!(k, ArrayKey::String(k) if k.as_bytes().eq_ignore_ascii_case(name)),
        ) {
            result.set(ArrayKey::String(s.clone()), Value::String(s));
        }
    };
    for iface in goro_core::reflection::class_all_interface_names(vm, &class_lower) {
        add(iface.as_bytes());
    }
    // The implicit Stringable (traits implement no interfaces).
    let is_trait = vm.classes.get(&class_lower).is_some_and(|c| c.is_trait);
    if !is_trait && vm.class_implements_interface(&class_lower, b"stringable") {
        add(b"Stringable");
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn class_parents_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let class_name = match class_arg(vm, "class_parents", args)? {
        Some((c, _)) => c,
        None => return Ok(Value::False),
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
            let parent_lower: Vec<u8> =
                parent_name.iter().map(|b| b.to_ascii_lowercase()).collect();
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
    let class_name = match class_arg(vm, "class_uses", args)? {
        Some((c, _)) => c,
        None => return Ok(Value::False),
    };
    let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    let mut result = PhpArray::new();

    if let Some(class) = vm.classes.get(&class_lower) {
        for trait_name in &class.traits {
            let trait_str = PhpString::from_vec(trait_name.clone());
            result.set(
                ArrayKey::String(trait_str.clone()),
                Value::String(trait_str),
            );
        }
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}
