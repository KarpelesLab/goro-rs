//! The php_stream operations mailparse uses, over PHP stream resources
//! (through the stream functions of ext/standard) or over a string (the
//! read-only memory streams the C code opens on a string source).

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

pub fn call(vm: &mut Vm, name: &[u8], args: &[Value]) -> Result<Value, VmError> {
    match vm.functions.get(name).copied() {
        Some(f) => f(vm, args),
        None => Ok(Value::False),
    }
}

/// Whether `v` is an open stream resource.
pub fn is_stream(v: &Value) -> bool {
    match v.deref() {
        Value::Resource(r) => {
            goro_core::value::is_open_resource(r.id())
                && matches!(
                    goro_core::value::resource_type_of(r.id()).as_deref(),
                    Some(b"stream") | Some(b"persistent stream")
                )
        }
        _ => false,
    }
}

/// A readable source.
pub enum Src {
    Bytes { data: PhpString, pos: usize },
    Stream(Value),
}

impl Src {
    pub fn bytes(data: PhpString) -> Src {
        Src::Bytes { data, pos: 0 }
    }

    /// php_stream_seek(SEEK_SET): false on failure.
    pub fn seek(&mut self, vm: &mut Vm, off: i64) -> Result<bool, VmError> {
        match self {
            // (a memory stream seeks past its end)
            Src::Bytes { pos, .. } => {
                if off < 0 {
                    return Ok(false);
                }
                *pos = off as usize;
                Ok(true)
            }
            Src::Stream(s) => {
                let r = call(vm, b"fseek", &[s.clone(), Value::Long(off), Value::Long(0)])?;
                Ok(r.to_long() == 0)
            }
        }
    }

    pub fn rewind(&mut self, vm: &mut Vm) -> Result<(), VmError> {
        match self {
            Src::Bytes { pos, .. } => {
                *pos = 0;
                Ok(())
            }
            Src::Stream(s) => call(vm, b"rewind", std::slice::from_ref(s)).map(|_| ()),
        }
    }

    pub fn tell(&mut self, vm: &mut Vm) -> Result<i64, VmError> {
        match self {
            Src::Bytes { pos, .. } => Ok(*pos as i64),
            Src::Stream(s) => Ok(match call(vm, b"ftell", std::slice::from_ref(s))? {
                Value::Long(n) => n,
                _ => -1,
            }),
        }
    }

    pub fn eof(&mut self, vm: &mut Vm) -> Result<bool, VmError> {
        match self {
            Src::Bytes { data, pos } => Ok(*pos >= data.len()),
            Src::Stream(s) => Ok(call(vm, b"feof", std::slice::from_ref(s))?.is_truthy()),
        }
    }

    /// php_stream_read(): up to `n` bytes (empty at the end).
    pub fn read(&mut self, vm: &mut Vm, n: usize) -> Result<Vec<u8>, VmError> {
        if n == 0 {
            return Ok(Vec::new());
        }
        match self {
            Src::Bytes { data, pos } => {
                let b = data.as_bytes();
                let end = (*pos + n).min(b.len());
                let out = b[(*pos).min(end)..end].to_vec();
                *pos = end;
                Ok(out)
            }
            Src::Stream(s) => match call(vm, b"fread", &[s.clone(), Value::Long(n as i64)])? {
                Value::String(r) => Ok(r.as_bytes().to_vec()),
                _ => Ok(Vec::new()),
            },
        }
    }

    /// php_stream_gets(buf, maxlen): a line of at most `maxlen - 1` bytes;
    /// None at the end.
    pub fn gets(&mut self, vm: &mut Vm, maxlen: usize) -> Result<Option<Vec<u8>>, VmError> {
        match self {
            Src::Bytes { data, pos } => {
                let b = data.as_bytes();
                if *pos >= b.len() {
                    return Ok(None);
                }
                let limit = (*pos + maxlen - 1).min(b.len());
                let end = match b[*pos..limit].iter().position(|&c| c == b'\n') {
                    Some(i) => *pos + i + 1,
                    None => limit,
                };
                let out = b[*pos..end].to_vec();
                *pos = end;
                Ok(Some(out))
            }
            Src::Stream(s) => match call(vm, b"fgets", &[s.clone(), Value::Long(maxlen as i64)])? {
                Value::String(r) => Ok(Some(r.as_bytes().to_vec())),
                _ => Ok(None),
            },
        }
    }
}

/// Write `data` to a PHP stream.
pub fn write(vm: &mut Vm, stream: &Value, data: &[u8]) -> Result<(), VmError> {
    if data.is_empty() {
        return Ok(());
    }
    call(
        vm,
        b"fwrite",
        &[stream.clone(), Value::String(PhpString::from_bytes(data))],
    )
    .map(|_| ())
}

/// php_stream_open_wrapper(path, "rb", REPORT_ERRORS): through fopen(),
/// its warnings reported under `fname`.
pub fn open(vm: &mut Vm, fname: &str, path: &PhpString) -> Result<Option<Value>, VmError> {
    let args = [
        Value::String(path.clone()),
        Value::String(PhpString::from_bytes(b"rb")),
    ];
    let prev = vm.warning_capture.replace(Vec::new());
    let r = call(vm, b"fopen", &args);
    let captured = std::mem::replace(&mut vm.warning_capture, prev).unwrap_or_default();
    for w in captured {
        // "fopen(x): msg" -> "fname(x): msg"
        let msg = match w.find('(') {
            Some(i)
                if w[..i]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_') =>
            {
                format!("{}{}", fname, &w[i..])
            }
            _ => w,
        };
        vm.emit_warning(&msg);
    }
    match r? {
        v @ Value::Resource(_) => Ok(Some(v)),
        _ => Ok(None),
    }
}

pub fn close(vm: &mut Vm, stream: &Value) {
    let _ = call(vm, b"fclose", std::slice::from_ref(stream));
}
