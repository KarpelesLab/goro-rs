//! Stream access for extensions living in other crates (exif, fileinfo,
//! zip...): php_stream_open_wrapper() / read / seek / tell / close on top
//! of the stream functions registered here, so that every wrapper (plain
//! files, data:, php://memory, user wrappers...) is served the same way.

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

/// Call a registered builtin function by its (lowercase) name.
pub fn call(vm: &mut Vm, name: &[u8], args: &[Value]) -> Result<Value, VmError> {
    let caller = vm.current_builtin_name();
    call_with(vm, caller.as_deref(), name, args)
}

/// `call()` for an extension function or method named `caller`, which
/// the diagnostics of the called builtin name (like php_error_docref()).
pub fn call_as(vm: &mut Vm, caller: &str, name: &[u8], args: &[Value]) -> Result<Value, VmError> {
    call_with(vm, Some(caller), name, args)
}

fn call_with(
    vm: &mut Vm,
    caller: Option<&str>,
    name: &[u8],
    args: &[Value],
) -> Result<Value, VmError> {
    match vm.functions.get(name).copied() {
        Some(f) => {
            crate::misc::userstream::set_caller_override(caller);
            let r = f(vm, args);
            crate::misc::userstream::set_caller_override(None);
            r
        }
        None => Ok(Value::False),
    }
}

fn res_id(v: &Value) -> Option<i64> {
    match v {
        Value::Resource(n) => Some(n.id()),
        _ => None,
    }
}

/// php_stream_open_wrapper(path, mode, IGNORE_PATH [| REPORT_ERRORS]):
/// the opened stream's resource id. Without `report`, the warnings of the
/// open are swallowed.
pub fn open(vm: &mut Vm, path: &[u8], mode: &str, report: bool) -> Result<Option<i64>, VmError> {
    let caller = vm.current_builtin_name();
    open_as(vm, caller.as_deref(), path, mode, report)
}

/// `open()` on behalf of the extension function or method `caller`.
pub fn open_as(
    vm: &mut Vm,
    caller: Option<&str>,
    path: &[u8],
    mode: &str,
    report: bool,
) -> Result<Option<i64>, VmError> {
    let saved = if report {
        None
    } else {
        Some(vm.warning_capture.replace(Vec::new()))
    };
    let r = call_with(
        vm,
        caller,
        b"fopen",
        &[
            Value::String(PhpString::from_bytes(path)),
            Value::String(PhpString::from_bytes(mode.as_bytes())),
        ],
    );
    if let Some(s) = saved {
        vm.warning_capture = s;
    }
    Ok(res_id(&r?.deref()))
}

/// php_stream_close()
pub fn close(vm: &mut Vm, id: i64) {
    let _ = call(vm, b"fclose", &[Value::resource(id)]);
}

/// Whether `v` is an open stream resource.
pub fn is_stream(vm: &mut Vm, v: &Value) -> bool {
    match v.deref() {
        Value::Resource(id) => {
            let t = call(vm, b"get_resource_type", &[Value::resource(&id)]).ok();
            matches!(t, Some(Value::String(s)) if s.as_bytes() == b"stream")
        }
        _ => false,
    }
}

/// What stream_get_meta_data() tells about a stream.
#[derive(Default, Debug, Clone)]
pub struct Meta {
    pub stream_type: Vec<u8>,
    pub wrapper_type: Vec<u8>,
    pub uri: Option<Vec<u8>>,
    pub seekable: bool,
}

impl Meta {
    /// php_stream_is(stream, PHP_STREAM_IS_STDIO)
    pub fn is_stdio(&self) -> bool {
        self.stream_type == b"STDIO"
    }
}

pub fn meta(vm: &mut Vm, id: i64) -> Meta {
    let mut m = Meta::default();
    if let Ok(Value::Array(a)) = call(vm, b"stream_get_meta_data", &[Value::resource(id)]) {
        let a = a.borrow();
        let get = |k: &[u8]| {
            a.get(&goro_core::array::ArrayKey::String(PhpString::from_bytes(
                k,
            )))
            .cloned()
        };
        if let Some(Value::String(s)) = get(b"stream_type") {
            m.stream_type = s.as_bytes().to_vec();
        }
        if let Some(Value::String(s)) = get(b"wrapper_type") {
            m.wrapper_type = s.as_bytes().to_vec();
        }
        if let Some(Value::String(s)) = get(b"uri") {
            m.uri = Some(s.as_bytes().to_vec());
        }
        m.seekable = matches!(get(b"seekable"), Some(Value::True));
    }
    m
}

/// php_stream_tell()
pub fn tell(vm: &mut Vm, id: i64) -> i64 {
    match call(vm, b"ftell", &[Value::resource(id)]) {
        Ok(Value::Long(n)) => n,
        _ => 0,
    }
}

/// php_stream_seek(): true on success.
pub fn seek(vm: &mut Vm, id: i64, offset: i64, whence: i64) -> bool {
    matches!(
        call(
            vm,
            b"fseek",
            &[
                Value::resource(id),
                Value::Long(offset),
                Value::Long(whence)
            ],
        ),
        Ok(Value::Long(0))
    )
}

/// php_stream_read() looped until `n` bytes or EOF.
pub fn read(vm: &mut Vm, id: i64, n: usize) -> Vec<u8> {
    let mut out = Vec::new();
    while out.len() < n {
        let want = (n - out.len()).min(1 << 20);
        match call(
            vm,
            b"fread",
            &[Value::resource(id), Value::Long(want as i64)],
        ) {
            Ok(Value::String(s)) if !s.is_empty() => out.extend_from_slice(s.as_bytes()),
            _ => break,
        }
    }
    out
}

/// The rest of the stream from the current position.
pub fn read_all(vm: &mut Vm, id: i64) -> Vec<u8> {
    match call(vm, b"stream_get_contents", &[Value::resource(id)]) {
        Ok(Value::String(s)) => s.as_bytes().to_vec(),
        _ => Vec::new(),
    }
}

/// php_stream_write()
pub fn write(vm: &mut Vm, id: i64, data: &[u8]) -> i64 {
    match call(
        vm,
        b"fwrite",
        &[
            Value::resource(id),
            Value::String(PhpString::from_bytes(data)),
        ],
    ) {
        Ok(Value::Long(n)) => n,
        _ => -1,
    }
}

/// php_basename(path, NULL)
pub fn basename(path: &[u8]) -> Vec<u8> {
    crate::misc::php_basename_bytes(path)
}

/// The PHP-visible image type helpers of ext/standard/image.c.
pub use crate::image::{image_type_from_bytes, image_type_to_mime_type, is_avif_data};

/// Streams and wrappers of other crates served by the stream layer.
pub use crate::misc::userstream::{
    NativeOpened, NativeStream, NativeWrapperOps, StatBuf, ext_alloc_stream,
    register_native_wrapper,
};

/// The value of option `wrapper`/`name` of stream context `ctx`.
pub fn context_option(ctx: Option<i64>, wrapper: &[u8], name: &[u8]) -> Option<Value> {
    crate::misc::stream_context_option(ctx?, wrapper, name)
}
