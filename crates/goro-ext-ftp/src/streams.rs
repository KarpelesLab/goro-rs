//! The php_stream operations the FTP client does on PHP streams, through
//! the standard extension's stream functions.

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

/// Call a registered builtin by name with its diagnostics silenced.
fn call(vm: &mut Vm, name: &[u8], args: &[Value]) -> Option<Value> {
    let f = vm.functions.get(name).copied()?;
    let saved = vm.error_reporting;
    vm.error_reporting = 0;
    let r = f(vm, args);
    vm.error_reporting = saved;
    r.ok()
}

fn s(b: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(b))
}

/// php_stream_write(): the bytes written.
pub fn write(vm: &mut Vm, id: i64, data: &[u8]) -> usize {
    if data.is_empty() {
        return 0;
    }
    match call(vm, b"fwrite", &[Value::resource(&id), s(data)]) {
        Some(Value::Long(n)) if n >= 0 => n as usize,
        _ => 0,
    }
}

/// php_stream_read(): None on error.
pub fn read(vm: &mut Vm, id: i64, max: usize) -> Option<Vec<u8>> {
    match call(
        vm,
        b"fread",
        &[Value::resource(&id), Value::Long(max as i64)],
    ) {
        Some(Value::String(v)) => Some(v.as_bytes().to_vec()),
        _ => None,
    }
}

/// php_stream_get_line() into a buffer of `maxlen` bytes: at most
/// maxlen - 1 bytes, up to and including a "\n".
pub fn get_line(vm: &mut Vm, id: i64, maxlen: usize) -> Option<Vec<u8>> {
    match call(
        vm,
        b"fgets",
        &[Value::resource(&id), Value::Long(maxlen as i64)],
    ) {
        Some(Value::String(v)) => Some(v.as_bytes().to_vec()),
        _ => None,
    }
}

/// php_stream_eof()
pub fn eof(vm: &mut Vm, id: i64) -> bool {
    !matches!(
        call(vm, b"feof", &[Value::resource(&id)]),
        Some(Value::False)
    )
}

/// php_stream_seek()
pub fn seek(vm: &mut Vm, id: i64, offset: i64, whence: i64) {
    let _ = call(
        vm,
        b"fseek",
        &[
            Value::resource(&id),
            Value::Long(offset),
            Value::Long(whence),
        ],
    );
}

/// php_stream_tell()
pub fn tell(vm: &mut Vm, id: i64) -> i64 {
    match call(vm, b"ftell", &[Value::resource(&id)]) {
        Some(Value::Long(n)) => n,
        _ => -1,
    }
}

/// php_stream_close()
pub fn close(vm: &mut Vm, id: i64) {
    let _ = call(vm, b"fclose", &[Value::resource(&id)]);
}

/// php_stream_open_wrapper(path, mode, REPORT_ERRORS): None after the
/// "Failed to open stream" warning (attributed to `func`).
pub fn open(vm: &mut Vm, func: &str, path: &[u8], mode: &str) -> Option<i64> {
    match call(vm, b"fopen", &[s(path), s(mode.as_bytes())]) {
        Some(Value::Resource(id)) => Some(id.id()),
        _ => {
            let reason = open_error(path, mode);
            vm.emit_warning(&format!(
                "{}({}): Failed to open stream: {}",
                func,
                String::from_utf8_lossy(path),
                reason
            ));
            None
        }
    }
}

/// Why opening a plain file failed (the error text of the plain wrapper),
/// found without creating anything.
fn open_error(path: &[u8], mode: &str) -> String {
    use std::os::unix::ffi::OsStrExt;
    if path.windows(3).any(|w| w == b"://") {
        return "operation failed".to_string();
    }
    let p = std::path::Path::new(std::ffi::OsStr::from_bytes(path));
    let writing = !mode.starts_with('r') || mode.contains('+');
    let r = if p.exists() {
        let mut o = std::fs::OpenOptions::new();
        o.read(!writing).write(writing);
        o.open(p).map(|_| ())
    } else if mode.starts_with('r') {
        Err(std::io::Error::from_raw_os_error(libc::ENOENT))
    } else {
        match p.parent() {
            Some(d) if !d.as_os_str().is_empty() && !d.is_dir() => {
                Err(std::io::Error::from_raw_os_error(libc::ENOENT))
            }
            _ => Err(std::io::Error::from_raw_os_error(libc::EACCES)),
        }
    };
    let code = match r {
        Ok(()) => return "operation failed".to_string(),
        Err(e) => e.raw_os_error().unwrap_or(0),
    };
    unsafe { std::ffi::CStr::from_ptr(libc::strerror(code)) }
        .to_string_lossy()
        .into_owned()
}
