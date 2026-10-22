//! The Directory class returned by dir() (ext/standard/dir.c): a final,
//! non-constructible, non-cloneable class with readonly `path` / `handle`
//! properties whose methods operate on the directory stream in `handle`.

use super::*;

pub(crate) fn register(vm: &mut Vm) {
    let Some(stub) = goro_core::stubs::class_entry(vm, b"Directory") else {
        return;
    };
    let mut ce = stub.clone();
    // the methods are native
    ce.methods.clear();
    vm.classes.insert(b"directory".to_vec(), ce);
    vm.uncloneable_classes.push(b"directory".to_vec());
    vm.native_class_handlers.insert(
        b"directory".to_vec(),
        goro_core::vm::NativeClassHandler {
            has_method: |_c, m, _s| {
                [&b"__construct"[..], b"read", b"rewind", b"close"]
                    .iter()
                    .any(|n| m.eq_ignore_ascii_case(n))
            },
            call: directory_call,
            prop_added: None,
            prop_write: None,
            compare_check: None,
            prop_read: None,
            param_names: None,
        },
    );
}

/// object_init_ex(dir_class_entry_ptr) with the path and handle set.
pub(crate) fn new_directory(vm: &mut Vm, path: Value, handle: Value) -> Value {
    let obj = vm.spl_instantiate(b"Directory");
    {
        let mut o = obj.borrow_mut();
        o.set_property(b"path".to_vec(), path);
        o.set_property(b"handle".to_vec(), handle);
    }
    Value::Object(obj)
}

fn is_dir_stream(id: i64) -> bool {
    DIR_HANDLES.with(|dh| dh.borrow().contains_key(&id)) || userstream::is_user_dir(id)
}

fn throw(vm: &mut Vm, class: &[u8], msg: &str) {
    let exc = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(exc);
}

fn directory_call(
    vm: &mut Vm,
    _class: &[u8],
    method: &[u8],
    _called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    let m = method.to_ascii_lowercase();
    if m == b"__construct" {
        throw(
            vm,
            b"Error",
            "Cannot directly construct Directory, use dir() instead",
        );
        return Value::Null;
    }
    let (name, func): (&str, &[u8]) = match m.as_slice() {
        b"read" => ("read", b"readdir"),
        b"rewind" => ("rewind", b"rewinddir"),
        _ => ("close", b"closedir"),
    };
    if !args.is_empty() {
        let msg = format!(
            "Directory::{}() expects exactly 0 arguments, {} given",
            name,
            args.len()
        );
        throw(vm, b"ArgumentCountError", &msg);
        return Value::Null;
    }
    // php_dir_get_directory_stream_from_this()
    let handle = match this {
        Some(Value::Object(o)) => o.borrow().get_property(b"handle"),
        _ => Value::Undef,
    };
    let Value::Resource(id) = handle.deref() else {
        throw(vm, b"Error", "Internal directory stream has been altered");
        return Value::Null;
    };
    if !goro_core::value::is_open_resource(id) {
        let msg = format!(
            "Directory::{}(): cannot use Directory resource after it has been closed",
            name
        );
        let exc = vm.throw_type_error(msg);
        vm.current_exception = Some(exc);
        return Value::Null;
    }
    if !is_dir_stream(id) {
        throw(vm, b"Error", "Internal directory stream has been altered");
        return Value::Null;
    }
    let Some(f) = vm.functions.get(func).copied() else {
        return Value::Null;
    };
    match f(vm, &[Value::Resource(id)]) {
        Ok(v) => v,
        Err(_) => Value::Null,
    }
}
