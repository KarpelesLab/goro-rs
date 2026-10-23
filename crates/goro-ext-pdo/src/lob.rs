//! The stream operations PDO needs for LOB parameters and columns, through
//! the standard extension's stream functions.

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

fn call(vm: &mut Vm, name: &[u8], args: &[Value]) -> Option<Value> {
    let f = vm.functions.get(name).copied()?;
    // (a stream made here is owned by the zval PDO hands out)
    goro_core::value::call_builtin_adopting(vm, f, args).ok()
}

/// php_stream_from_zval_no_verify + php_stream_copy_to_mem: None when the
/// value is not a stream.
pub fn read_all(vm: &mut Vm, v: &Value) -> Option<Vec<u8>> {
    let v = v.deref();
    if !matches!(v, Value::Resource(_)) {
        return None;
    }
    let ty = v.resource_type_name().unwrap_or_default();
    if ty != b"stream" {
        return None;
    }
    match call(vm, b"stream_get_contents", std::slice::from_ref(&v)) {
        Some(Value::String(s)) => Some(s.as_bytes().to_vec()),
        Some(_) => Some(Vec::new()),
        None => None,
    }
}

/// php_stream_memory_open(TEMP_STREAM_READONLY, data) as a zval
pub fn memory_stream(vm: &mut Vm, data: &[u8]) -> Value {
    let s = |b: &[u8]| Value::String(PhpString::from_bytes(b));
    let h = match call(vm, b"fopen", &[s(b"php://memory"), s(b"w+b")]) {
        Some(h @ Value::Resource(_)) => h,
        _ => return Value::False,
    };
    call(vm, b"fwrite", &[h.clone(), s(data)]);
    call(vm, b"rewind", std::slice::from_ref(&h));
    h
}
