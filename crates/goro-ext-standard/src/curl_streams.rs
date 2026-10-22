//! Stream and callable services for ext/curl: the FILE* handlers
//! (CURLOPT_FILE / INFILE / WRITEHEADER / STDERR) work on PHP streams, and
//! the CURLOPT_*FUNCTION options need zend_is_callable_ex().

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::misc;

/// Whether `fid` is an open stream (zend_fetch_resource2_ex(..., php_file_le_stream())).
pub fn is_stream(fid: i64) -> bool {
    misc::has_file_handle(fid)
}

/// The stream's mode as given to fopen() (php_stream->mode).
pub fn mode(fid: i64) -> Option<String> {
    misc::stream_raw_mode(fid)
}

/// php_stream_write() / fwrite() to the stream; returns the bytes written.
pub fn write(vm: &mut Vm, fid: i64, data: &[u8]) -> usize {
    match call_builtin(
        vm,
        b"fwrite",
        &[
            Value::resource(fid),
            Value::String(PhpString::from_bytes(data)),
        ],
    ) {
        Some(Value::Long(n)) if n >= 0 => n as usize,
        _ => 0,
    }
}

/// fread(); None on error / EOF.
pub fn read(vm: &mut Vm, fid: i64, max: usize) -> Option<Vec<u8>> {
    match call_builtin(
        vm,
        b"fread",
        &[Value::resource(fid), Value::Long(max as i64)],
    ) {
        Some(Value::String(s)) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

/// fflush()
pub fn flush(vm: &mut Vm, fid: i64) {
    let _ = call_builtin(vm, b"fflush", &[Value::resource(fid)]);
}

/// The whole contents of `path` through the stream wrappers
/// (php_stream_open_wrapper + read), None when it cannot be opened.
pub fn read_file(vm: &mut Vm, path: &[u8]) -> Option<Vec<u8>> {
    let r = call_builtin(
        vm,
        b"file_get_contents",
        &[Value::String(PhpString::from_bytes(path))],
    );
    match r {
        Some(Value::String(s)) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

/// zend_is_callable_ex(): Ok(()) or the error text ("function \"x\" not
/// found or invalid function name").
pub fn check_callable(vm: &mut Vm, v: &Value) -> Result<(), String> {
    let (ok, _, err) = misc::check_callable(vm, v, false);
    if ok {
        Ok(())
    } else {
        Err(err.unwrap_or_else(|| "no array or string given".to_string()))
    }
}

/// Call a registered builtin by name, with its diagnostics silenced
/// (stream operations done on behalf of libcurl do not report errors).
fn call_builtin(vm: &mut Vm, name: &[u8], args: &[Value]) -> Option<Value> {
    let f = vm.functions.get(name).copied()?;
    let saved = vm.error_reporting;
    vm.error_reporting = 0;
    let r = f(vm, args);
    vm.error_reporting = saved;
    r.ok()
}
