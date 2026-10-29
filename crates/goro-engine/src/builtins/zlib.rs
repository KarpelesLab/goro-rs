//! ext/zlib (PHP manual: Zlib Compression) on the goro-ext-zlib crate:
//! the one-shot functions, DeflateContext/InflateContext, the gz* file
//! functions and `compress.zlib://`, the `zlib.*` stream filters and
//! ob_gzhandler. Behaviour from the manual, tests ext/zlib/tests and
//! observations on PHP 8.5.

use std::io::{Cursor, Read, Seek, SeekFrom, Write};

use goro_ext_zlib::gzfile::{GzMode, GzReader, GzWriter, Whence};
use goro_ext_zlib::{self as zl, ZlibError, consts::ConstValue};
use goro_runtime::{ObjRef, PhpArray, PhpStr, Value};

use super::file::{Backing, Stream, new_stream_res, open_stream};
use super::streams::{
    FilterSide, NativeStream, NativeWrapper, StreamFilter, register_filter, register_wrapper,
};
use crate::class::{NativeHooks, TypeName, cflags};
use crate::func::{Args, FuncKind};
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "zlib",
    functions: &[
        (
            "function gzcompress(string $data, int $level = -1, int $encoding = ZLIB_ENCODING_DEFLATE): string|false",
            gzcompress,
        ),
        (
            "function gzdeflate(string $data, int $level = -1, int $encoding = ZLIB_ENCODING_RAW): string|false",
            gzdeflate,
        ),
        (
            "function gzencode(string $data, int $level = -1, int $encoding = ZLIB_ENCODING_GZIP): string|false",
            gzencode,
        ),
        (
            "function zlib_encode(string $data, int $encoding, int $level = -1): string|false",
            zlib_encode,
        ),
        (
            "function gzuncompress(string $data, int $max_length = 0): string|false",
            gzuncompress,
        ),
        (
            "function gzinflate(string $data, int $max_length = 0): string|false",
            gzinflate,
        ),
        (
            "function gzdecode(string $data, int $max_length = 0): string|false",
            gzdecode,
        ),
        (
            "function zlib_decode(string $data, int $max_length = 0): string|false",
            zlib_decode,
        ),
        (
            "function deflate_init(int $encoding, array|object $options = []): DeflateContext|false",
            deflate_init,
        ),
        (
            "function deflate_add(DeflateContext $context, string $data, int $flush_mode = ZLIB_SYNC_FLUSH): string|false",
            deflate_add,
        ),
        (
            "function inflate_init(int $encoding, array|object $options = []): InflateContext|false",
            inflate_init,
        ),
        (
            "function inflate_add(InflateContext $context, string $data, int $flush_mode = ZLIB_SYNC_FLUSH): string|false",
            inflate_add,
        ),
        (
            "function inflate_get_status(InflateContext $context): int",
            inflate_get_status,
        ),
        (
            "function inflate_get_read_len(InflateContext $context): int",
            inflate_get_read_len,
        ),
        (
            "function gzopen(string $filename, string $mode, int $use_include_path = 0)",
            gzopen,
        ),
        (
            "function gzfile(string $filename, int $use_include_path = 0): array|false",
            gzfile,
        ),
        (
            "function readgzfile(string $filename, int $use_include_path = 0): int|false",
            readgzfile,
        ),
        (
            "function gzread($stream, int $length): string|false",
            gzread,
        ),
        (
            "function gzwrite($stream, string $data, ?int $length = null): int|false",
            gzwrite,
        ),
        (
            "function gzputs($stream, string $data, ?int $length = null): int|false",
            gzwrite,
        ),
        (
            "function gzgets($stream, ?int $length = null): string|false",
            gzgets,
        ),
        ("function gzgetc($stream): string|false", gzgetc),
        ("function gzpassthru($stream): int", gzpassthru),
        (
            "function gzseek($stream, int $offset, int $whence = SEEK_SET): int",
            gzseek,
        ),
        ("function gztell($stream): int|false", gztell),
        ("function gzrewind($stream): bool", gzrewind),
        ("function gzeof($stream): bool", gzeof),
        ("function gzclose($stream): bool", gzclose),
        (
            "function ob_gzhandler(string $data, int $flags): string|false",
            ob_gzhandler,
        ),
        (
            "function zlib_get_coding_type(): string|false",
            zlib_get_coding_type,
        ),
    ],
    classes: &[
        (
            "deflatecontext",
            "<?php #[NotSerializable] final class DeflateContext {}",
        ),
        (
            "inflatecontext",
            "<?php #[NotSerializable] final class InflateContext {}",
        ),
    ],
    hooks: &[
        ("deflatecontext", context_hooks),
        ("inflatecontext", context_hooks),
    ],
    constants: &CONSTANTS,
    // PHP manual: zlib runtime configuration.
    ini: &[
        ("zlib.output_compression", "0"),
        ("zlib.output_compression_level", "-1"),
        ("zlib.output_handler", ""),
    ],
    init: Some(init),
    listed: true,
    ..Module::EMPTY
};

const fn consts() -> [(&'static str, ConstVal); 27] {
    let mut out = [("", ConstVal::Null); 27];
    let mut i = 0;
    while i < 27 {
        let (n, v) = zl::consts::ALL[i];
        out[i] = (
            n,
            match v {
                ConstValue::Int(x) => ConstVal::Int(x),
                ConstValue::Str(s) => ConstVal::Str(s),
            },
        );
        i += 1;
    }
    out
}

static CONSTANTS: [(&str, ConstVal); 27] = consts();

static WRAPPER: NativeWrapper = NativeWrapper::new(wrapper_open);

fn init(vm: &mut Vm) {
    register_filter(vm, "zlib.*", zlib_filter);
    register_wrapper(vm, "compress.zlib", &WRAPPER);
}

fn context_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|vm, o| {
            let cls = vm.class_of(o);
            cls.flags.set(
                cls.flags.get()
                    | cflags::NO_DYNAMIC
                    | cflags::NOT_SERIALIZABLE
                    | cflags::NOT_CLONEABLE,
            );
        }),
        ..Default::default()
    }
}

struct DeflateObj(zl::DeflateContext);
impl goro_runtime::NativeData for DeflateObj {}
struct InflateObj(zl::InflateContext);
impl goro_runtime::NativeData for InflateObj {}

fn with_native<T: 'static, U>(o: &ObjRef, f: impl FnOnce(&mut T) -> U) -> Option<U> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<T>().map(f)
}

/// A library error: thrown, or a warning and false.
fn zerr(vm: &mut Vm, a: &Args, e: ZlibError) -> R<Value> {
    match e {
        ZlibError::ValueError(m) => vm.throw("ValueError", &m),
        ZlibError::TypeError(m) => vm.throw("TypeError", &m),
        ZlibError::Warning(m) => {
            vm.fwarn(a, &m)?;
            Ok(Value::Bool(false))
        }
    }
}

fn bytes_result(vm: &mut Vm, a: &Args, r: Result<Vec<u8>, ZlibError>) -> R<Value> {
    match r {
        Ok(b) => Ok(Value::Str(PhpStr::from(b))),
        Err(e) => zerr(vm, a, e),
    }
}

// ---------------------------------------------------------------- one-shot

fn gzcompress(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = zl::gzcompress(a.bytes(0), a.int_or(1, -1), a.int_or(2, 15));
    bytes_result(vm, a, r)
}
fn gzdeflate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = zl::gzdeflate(a.bytes(0), a.int_or(1, -1), a.int_or(2, -15));
    bytes_result(vm, a, r)
}
fn gzencode(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = zl::gzencode(a.bytes(0), a.int_or(1, -1), a.int_or(2, 31));
    bytes_result(vm, a, r)
}
fn zlib_encode(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = zl::zlib_encode(a.bytes(0), a.int(1), a.int_or(2, -1));
    bytes_result(vm, a, r)
}
fn gzuncompress(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = zl::gzuncompress(a.bytes(0), a.int_or(1, 0));
    bytes_result(vm, a, r)
}
fn gzinflate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = zl::gzinflate(a.bytes(0), a.int_or(1, 0));
    bytes_result(vm, a, r)
}
fn gzdecode(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = zl::gzdecode(a.bytes(0), a.int_or(1, 0));
    bytes_result(vm, a, r)
}
fn zlib_decode(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = zl::zlib_decode(a.bytes(0), a.int_or(1, 0));
    bytes_result(vm, a, r)
}

// ---------------------------------------------------------------- contexts

/// The value of option `k` as `(int)`.
fn opt_int(vm: &mut Vm, o: &PhpArray, k: &[u8]) -> R<Option<i64>> {
    match o.get_bytes(k) {
        Some(v) => {
            let v = v.deref();
            Ok(Some(match &v {
                Value::Str(s) => goro_runtime::convert::str_to_int(s),
                other => vm.to_int(other)?,
            }))
        }
        None => Ok(None),
    }
}

fn dict_opt(vm: &mut Vm, o: &PhpArray) -> R<Option<zl::DictOption>> {
    let Some(v) = o.get_bytes(b"dictionary") else {
        return Ok(None);
    };
    Ok(Some(match v.deref() {
        Value::Str(s) => zl::DictOption::Str(s.as_bytes().to_vec()),
        Value::Array(arr) => {
            let mut items = Vec::new();
            for (_, x) in arr.iter() {
                items.push(vm.to_php_str(&x.deref())?.as_bytes().to_vec());
            }
            zl::DictOption::List(items)
        }
        other => zl::DictOption::Other(other.tname()),
    }))
}

/// The `$options` of deflate_init()/inflate_init(): an array, or an
/// object's properties (test gh17745.phpt).
fn options_arr(vm: &mut Vm, a: &Args) -> PhpArray {
    match a.val(1) {
        Value::Object(o) => vm.object_props_cast(&o),
        Value::Array(arr) => arr,
        _ => PhpArray::new(),
    }
}

fn new_context(vm: &mut Vm, class: &str, data: Box<dyn goro_runtime::NativeData>) -> R<Value> {
    let Some(cls) = vm.find_class(class.as_bytes(), false)? else {
        return Err(vm.fatal(&format!("Class \"{class}\" not found")));
    };
    let o = vm.instantiate(&cls)?;
    *o.obj().native.borrow_mut() = Some(data);
    Ok(Value::Object(o))
}

fn deflate_init(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = options_arr(vm, a);
    let opts = zl::DeflateOptions {
        level: opt_int(vm, &o, b"level")?,
        memory: opt_int(vm, &o, b"memory")?,
        window: opt_int(vm, &o, b"window")?,
        strategy: opt_int(vm, &o, b"strategy")?,
        dictionary: dict_opt(vm, &o)?,
    };
    match zl::deflate_init(a.int(0), &opts) {
        Ok(c) => new_context(vm, "DeflateContext", Box::new(DeflateObj(c))),
        Err(e) => zerr(vm, a, e),
    }
}

fn inflate_init(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = options_arr(vm, a);
    let opts = zl::InflateOptions {
        window: opt_int(vm, &o, b"window")?,
        dictionary: dict_opt(vm, &o)?,
    };
    match zl::inflate_init(a.int(0), &opts) {
        Ok(c) => new_context(vm, "InflateContext", Box::new(InflateObj(c))),
        Err(e) => zerr(vm, a, e),
    }
}

fn uninit_ctx(vm: &mut Vm, a: &Args, class: &str) -> R<Value> {
    vm.fthrow(a, "Error", &format!("{class} object is not initialized"))
}

fn deflate_add(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(o) = a.obj(0) else {
        return Ok(Value::Bool(false));
    };
    let data = a.bytes(1).to_vec();
    let mode = a.int_or(2, 2);
    match with_native::<DeflateObj, _>(&o, |c| c.0.add(&data, mode)) {
        Some(r) => bytes_result(vm, a, r),
        None => uninit_ctx(vm, a, "DeflateContext"),
    }
}

fn inflate_add(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(o) = a.obj(0) else {
        return Ok(Value::Bool(false));
    };
    let data = a.bytes(1).to_vec();
    let mode = a.int_or(2, 2);
    match with_native::<InflateObj, _>(&o, |c| c.0.add(&data, mode)) {
        Some(r) => bytes_result(vm, a, r),
        None => uninit_ctx(vm, a, "InflateContext"),
    }
}

fn inflate_get_status(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(o) = a.obj(0) else {
        return Ok(Value::Bool(false));
    };
    match with_native::<InflateObj, _>(&o, |c| c.0.status()) {
        Some(n) => Ok(Value::Int(n)),
        None => uninit_ctx(vm, a, "InflateContext"),
    }
}

fn inflate_get_read_len(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(o) = a.obj(0) else {
        return Ok(Value::Bool(false));
    };
    match with_native::<InflateObj, _>(&o, |c| c.0.read_len()) {
        Some(n) => Ok(Value::Int(n)),
        None => uninit_ctx(vm, a, "InflateContext"),
    }
}

// ---------------------------------------------------------------- gz files

enum Src {
    File(std::fs::File),
    Mem(Cursor<Vec<u8>>),
}

impl Read for Src {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Src::File(f) => f.read(buf),
            Src::Mem(c) => c.read(buf),
        }
    }
}

impl Seek for Src {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        match self {
            Src::File(f) => f.seek(pos),
            Src::Mem(c) => c.seek(pos),
        }
    }
}

enum Sink {
    File(std::fs::File),
    Mem(Vec<u8>),
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Sink::File(f) => f.write(buf),
            Sink::Mem(v) => {
                v.extend_from_slice(buf);
                Ok(buf.len())
            }
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Sink::File(f) => f.flush(),
            Sink::Mem(_) => Ok(()),
        }
    }
}

/// A gzip reader read ahead in 8 KiB chunks, as PHP's stream layer reads:
/// the end is seen when a chunk comes up short, so feof() is true right
/// after the last line (test gzgets_basic.phpt).
struct GzRead {
    r: GzReader<Src>,
    buf: Vec<u8>,
    pos: usize,
}

impl GzRead {
    fn read(&mut self, out: &mut [u8]) -> Result<usize, goro_ext_zlib::gzfile::GzError> {
        if self.pos >= self.buf.len() {
            self.buf.resize(8192, 0);
            self.pos = 0;
            let n = match self.r.read(&mut self.buf) {
                Ok(n) => n,
                Err(e) => {
                    self.buf.clear();
                    return Err(e);
                }
            };
            self.buf.truncate(n);
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
    fn buffered(&self) -> i64 {
        (self.buf.len() - self.pos) as i64
    }
    fn tell(&self) -> i64 {
        self.r.tell() - self.buffered()
    }
    fn seek(&mut self, off: i64, wh: Whence) -> i64 {
        let target = match wh {
            Whence::Set => off,
            Whence::Cur => self.tell() + off,
        };
        self.buf.clear();
        self.pos = 0;
        self.r.seek(target, Whence::Set)
    }
    fn eof(&self) -> bool {
        self.r.eof() && self.pos >= self.buf.len()
    }
}

/// The stream under gzopen() and compress.zlib://.
enum GzStream {
    Read(Box<GzRead>),
    /// The writer, and the stream it writes to when that is not a file.
    Write(Option<Box<GzWriter<Sink>>>, Option<Box<Stream>>),
}

/// A file written without gzclose() is still finished when the stream
/// goes away (PHP closes open streams at the end of the request).
impl Drop for GzStream {
    fn drop(&mut self) {
        if let GzStream::Write(w, None) = self
            && let Some(mut w) = w.take()
        {
            let _ = w.flush(zl::Flush::Sync);
            let _ = (*w).close();
        }
    }
}

fn io_err(e: goro_ext_zlib::gzfile::GzError) -> std::io::Error {
    std::io::Error::other(e.message)
}

impl NativeStream for GzStream {
    fn read(&mut self, _vm: &mut Vm, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            GzStream::Read(r) => r.read(buf).map_err(io_err),
            GzStream::Write(..) => Ok(0),
        }
    }
    fn write(&mut self, _vm: &mut Vm, data: &[u8]) -> std::io::Result<usize> {
        match self {
            GzStream::Write(Some(w), _) => w.write(data),
            _ => Err(std::io::Error::other("stream is not writable")),
        }
    }
    fn flush(&mut self, _vm: &mut Vm) -> std::io::Result<()> {
        match self {
            GzStream::Write(Some(w), _) => w.flush(zl::Flush::Sync),
            _ => Ok(()),
        }
    }
    fn seek(&mut self, _vm: &mut Vm, pos: SeekFrom) -> Option<std::io::Result<u64>> {
        let (off, wh) = match pos {
            SeekFrom::Start(n) => (n as i64, Whence::Set),
            SeekFrom::Current(n) => (n, Whence::Cur),
            SeekFrom::End(_) => return Some(Err(std::io::Error::other("SEEK_END not supported"))),
        };
        let r = match self {
            GzStream::Read(r) => r.seek(off, wh),
            GzStream::Write(Some(w), _) => w.seek(off, wh),
            GzStream::Write(None, _) => -1,
        };
        Some(if r < 0 {
            Err(std::io::Error::other("seek failed"))
        } else {
            Ok(r as u64)
        })
    }
    fn tell(&mut self) -> Option<u64> {
        Some(match self {
            GzStream::Read(r) => r.tell() as u64,
            GzStream::Write(Some(w), _) => w.tell() as u64,
            GzStream::Write(None, _) => 0,
        })
    }
    fn eof(&self) -> bool {
        match self {
            GzStream::Read(r) => r.eof(),
            GzStream::Write(..) => false,
        }
    }
    fn close(&mut self, vm: &mut Vm) -> R<()> {
        if let GzStream::Write(w, inner) = self
            && let Some(mut w) = w.take()
        {
            let _ = w.flush(zl::Flush::Sync);
            if let Ok(Sink::Mem(data)) = (*w).close()
                && let Some(s) = inner.as_mut()
            {
                let _ = s.write_all(vm, &data);
                super::streams::close_stream(vm, s)?;
            }
        }
        Ok(())
    }
}

/// Resolves `name` on the include_path for reading.
fn include_path_file(vm: &Vm, name: &[u8]) -> Vec<u8> {
    if name.starts_with(b"/") || name.windows(3).any(|w| w == b"://") {
        return name.to_vec();
    }
    for dir in vm.ini.include_path.split(|&c| c == b':') {
        if dir.is_empty() {
            continue;
        }
        let mut p = dir.to_vec();
        p.push(b'/');
        p.extend_from_slice(name);
        if vm.fs_path(&p).exists() {
            return p;
        }
    }
    // then the script's directory, then the working directory (test
    // gzopen_variation4.phpt)
    let dir = super::file::dirname_bytes(&vm.script_path);
    let mut p = dir;
    p.push(b'/');
    p.extend_from_slice(name);
    if vm.fs_path(&p).exists() {
        return p;
    }
    name.to_vec()
}

/// Opens a gzip stream on `path` (a file name or another stream URL).
fn open_gz(
    vm: &mut Vm,
    a: &Args,
    path: &[u8],
    mode: &[u8],
    uri: &[u8],
    quiet: bool,
) -> R<Option<Stream>> {
    let m: GzMode = match goro_ext_zlib::gzfile::parse_mode(mode) {
        Ok(m) => m,
        Err(goro_ext_zlib::gzfile::GzModeError::ReadWrite) => {
            if !quiet {
                vm.fwarn(a, goro_ext_zlib::gzfile::GzModeError::ReadWrite.message())?;
            }
            return Ok(None);
        }
        Err(e) => {
            // the file is opened first (zlib manual)
            // with the mode as given: "e" alone fails to open (test
            // gzopen_variation9.phpt)
            if !mode.iter().any(|c| b"rwaxc".contains(c)) {
                if !quiet {
                    vm.warn(&format!(
                        "{}({}): Failed to open stream: Invalid argument",
                        a.fname(),
                        String::from_utf8_lossy(path)
                    ))?;
                }
                return Ok(None);
            }
            if open_stream(vm, a, path, mode, quiet)?.is_none() {
                return Ok(None);
            }
            if !quiet {
                vm.fwarn(a, e.message())?;
            }
            return Ok(None);
        }
    };
    let inner_mode: &[u8] = if m.read {
        b"rb"
    } else if m.exclusive {
        b"xb"
    } else if m.append {
        b"ab"
    } else {
        b"wb"
    };
    let Some(mut inner) = open_stream(vm, a, path, inner_mode, quiet)? else {
        return Ok(None);
    };
    let backing = std::mem::replace(
        &mut inner.backing,
        Backing::Memory {
            data: Vec::new(),
            pos: 0,
        },
    );
    let gz = if m.read {
        let src = match backing {
            Backing::File(f) => Src::File(f),
            other => {
                inner.backing = other;
                let mut data = Vec::new();
                let mut buf = vec![0u8; 8192];
                loop {
                    match inner.read_raw(vm, &mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => data.extend_from_slice(&buf[..n]),
                    }
                }
                super::streams::close_stream(vm, &mut inner)?;
                Src::Mem(Cursor::new(data))
            }
        };
        GzStream::Read(Box::new(GzRead {
            r: GzReader::new(src),
            buf: Vec::new(),
            pos: 0,
        }))
    } else {
        match backing {
            Backing::File(f) => {
                GzStream::Write(Some(Box::new(GzWriter::from_mode(Sink::File(f), &m))), None)
            }
            other => {
                inner.backing = other;
                GzStream::Write(
                    Some(Box::new(GzWriter::from_mode(Sink::Mem(Vec::new()), &m))),
                    Some(Box::new(inner)),
                )
            }
        }
    };
    // gzopen() streams have no wrapper, compress.zlib:// ones the ZLIB one
    // (test zlib_wrapper_meta_data_basic.phpt)
    let wrapper = if uri.starts_with(b"compress.zlib://") {
        "ZLIB"
    } else {
        ""
    };
    Ok(Some(Stream::new(
        Backing::Native(Box::new(gz)),
        mode,
        uri,
        wrapper,
        "ZLIB",
    )))
}

fn wrapper_open(vm: &mut Vm, a: &Args, url: &[u8], mode: &[u8], quiet: bool) -> R<Option<Stream>> {
    let path = &url[b"compress.zlib://".len()..];
    open_gz(vm, a, path, mode, url, quiet)
}

fn gzopen(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    let mode = a.str(1);
    let path = if a.int_or(2, 0) != 0 && mode.contains(&b'r') {
        include_path_file(vm, &f)
    } else {
        f.as_bytes().to_vec()
    };
    match open_gz(vm, a, &path, &mode, &path, false)? {
        Some(s) => Ok(Value::Resource(new_stream_res(s))),
        None => Ok(Value::Bool(false)),
    }
}

/// Reads a whole gzip file (gzfile, readgzfile).
fn read_all(vm: &mut Vm, a: &Args) -> R<Option<Vec<u8>>> {
    let f = a.str(0);
    let path = if a.int_or(1, 0) != 0 {
        include_path_file(vm, &f)
    } else {
        f.as_bytes().to_vec()
    };
    let Some(mut s) = open_gz(vm, a, &path, b"rb", &path, false)? else {
        return Ok(None);
    };
    let mut out = Vec::new();
    let mut buf = vec![0u8; 8192];
    loop {
        match s.read_raw(vm, &mut buf) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
    }
    super::streams::close_stream(vm, &mut s)?;
    Ok(Some(out))
}

/// gzfile(): the lines, each cut at its first NUL byte (observed on PHP
/// 8.5, per the crate's notes).
fn gzfile(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(data) = read_all(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    let mut out = PhpArray::new();
    let mut rest = &data[..];
    while !rest.is_empty() {
        let end = rest
            .iter()
            .position(|&c| c == b'\n')
            .map_or(rest.len(), |p| p + 1);
        let line = &rest[..end];
        let line = match line.iter().position(|&c| c == 0) {
            Some(z) => &line[..z],
            None => line,
        };
        out.push(Value::Str(PhpStr::from(line.to_vec()))).ok();
        rest = &rest[end..];
    }
    Ok(Value::Array(out))
}

fn readgzfile(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(data) = read_all(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    vm.echo(&data);
    Ok(Value::Int(data.len() as i64))
}

/// Runs the file function `name` with these arguments: the gz* stream
/// functions are those of the stream layer (PHP manual: gzread "binary-safe
/// gz-file read", on the stream gzopen() returned).
fn delegate(vm: &mut Vm, a: &mut Args, name: &str) -> R<Value> {
    // test gzclose_basic.phpt
    if let Value::Resource(r) = a.val(0)
        && r.is_closed()
    {
        return vm.fthrow(
            a,
            "TypeError",
            "Argument #1 ($stream) must be an open stream resource",
        );
    }
    let f = vm.functions.get(&PhpStr::from(name)).cloned();
    match f.as_deref().map(|f| &f.kind) {
        Some(FuncKind::Native { handler, .. }) => handler(vm, a),
        _ => Err(vm.fatal(&format!("Call to undefined function {name}()"))),
    }
}

/// Whether argument 0 is a stream open for writing only.
fn write_only(vm: &mut Vm, a: &Args) -> R<bool> {
    if !matches!(a.val(0), Value::Resource(ref r) if !r.is_closed()) {
        return Ok(false);
    }
    super::file::with_stream(vm, a, 0, |_, s| Ok(!s.readable))
}

/// Reading a gzip stream opened for writing gives false without a
/// diagnostic (test gzread_variation1.phpt).
fn gzread(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if write_only(vm, a)? {
        return Ok(Value::Bool(false));
    }
    delegate(vm, a, "fread")
}
/// Writing to a gzip stream opened for reading writes nothing (test
/// gzwrite_variation1.phpt).
fn gzwrite(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if matches!(a.val(0), Value::Resource(ref r) if !r.is_closed())
        && super::file::with_stream(vm, a, 0, |_, s| Ok(!s.writable))?
    {
        return Ok(Value::Int(0));
    }
    delegate(vm, a, "fwrite")
}
fn gzgets(vm: &mut Vm, a: &mut Args) -> R<Value> {
    delegate(vm, a, "fgets")
}
fn gzgetc(vm: &mut Vm, a: &mut Args) -> R<Value> {
    delegate(vm, a, "fgetc")
}
fn gzpassthru(vm: &mut Vm, a: &mut Args) -> R<Value> {
    delegate(vm, a, "fpassthru")
}
fn gzseek(vm: &mut Vm, a: &mut Args) -> R<Value> {
    // zlib has no SEEK_END (test gzseek_variation6.phpt)
    if a.int_or(2, 0) == 2 {
        vm.fwarn(a, "SEEK_END is not supported")?;
        return Ok(Value::Int(-1));
    }
    delegate(vm, a, "fseek")
}
fn gztell(vm: &mut Vm, a: &mut Args) -> R<Value> {
    delegate(vm, a, "ftell")
}
fn gzrewind(vm: &mut Vm, a: &mut Args) -> R<Value> {
    delegate(vm, a, "rewind")
}
fn gzeof(vm: &mut Vm, a: &mut Args) -> R<Value> {
    delegate(vm, a, "feof")
}
fn gzclose(vm: &mut Vm, a: &mut Args) -> R<Value> {
    delegate(vm, a, "fclose")
}

// ---------------------------------------------------------------- output

/// ob_gzhandler: compresses the output when the client accepts gzip or
/// deflate (HTTP_ACCEPT_ENCODING); without one, it returns false and the
/// output passes unchanged (PHP manual: ob_gzhandler).
#[derive(Default)]
struct ObGz {
    d: Option<zl::Deflater>,
    coding: Option<&'static str>,
}

fn accept_encoding(vm: &mut Vm) -> Option<&'static str> {
    let v = std::env::var("HTTP_ACCEPT_ENCODING").ok()?;
    let _ = vm;
    if v.contains("gzip") {
        Some("gzip")
    } else if v.contains("deflate") {
        Some("deflate")
    } else {
        None
    }
}

fn ob_gzhandler(vm: &mut Vm, a: &mut Args) -> R<Value> {
    const START: i64 = 1;
    const FINAL: i64 = 8;
    let flags = a.int(1);
    let data = a.bytes(0).to_vec();
    if flags & START != 0 {
        let coding = accept_encoding(vm);
        let st = vm.ext::<ObGz>();
        st.coding = coding;
        st.d = coding.and_then(|c| {
            zl::Deflater::new(zl::DeflateParams {
                level: -1,
                window_bits: 15,
                mem_level: 8,
                strategy: zl::Strategy::Default,
                wrap: if c == "gzip" {
                    zl::Wrap::Gzip
                } else {
                    zl::Wrap::Zlib
                },
            })
            .ok()
        });
        // A web SAPI tells the client: Content-Encoding when compressing,
        // and Vary in any case, added to the script's own (tests
        // ext/zlib/tests/gh7953.phpt, bug65391.phpt).
        if vm.out.sapi.web() && !crate::builtins::headers::sent(vm) {
            if let Some(c) = coding {
                let ce = format!("Content-Encoding: {}", c);
                crate::builtins::headers::add_header(vm, ce.as_bytes(), true, 0)?;
            }
            crate::builtins::headers::add_header(vm, b"Vary: Accept-Encoding", false, 0)?;
        }
    }
    let st = vm.ext::<ObGz>();
    let Some(d) = st.d.as_mut() else {
        return Ok(Value::Bool(false));
    };
    let mut out = Vec::new();
    let flush = if flags & FINAL != 0 {
        zl::Flush::Finish
    } else {
        zl::Flush::Sync
    };
    d.deflate(&data, flush, &mut out);
    if flags & FINAL != 0 {
        st.d = None;
    }
    Ok(Value::Str(PhpStr::from(out)))
}

fn zlib_get_coding_type(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let st = vm.ext::<ObGz>();
    Ok(match st.coding {
        Some(c) if st.d.is_some() => Value::from(c),
        _ => Value::Bool(false),
    })
}

// ---------------------------------------------------------------- filters

/// The deflate filter and whether data went in: a flush before any (a
/// seek right after the filter is attached) writes nothing, so it can't
/// leave a header where the data then goes (test bug75273.phpt).
struct DeflateF(goro_ext_zlib::filter::DeflateFilter, bool);
struct InflateF(goro_ext_zlib::filter::InflateFilter);

impl StreamFilter for DeflateF {
    fn filter(&mut self, _vm: &mut Vm, input: &[u8], closing: bool, out: &mut Vec<u8>) -> R<bool> {
        let fl = if closing {
            goro_ext_zlib::filter::FilterFlush::Close
        } else {
            goro_ext_zlib::filter::FilterFlush::None
        };
        self.1 |= !input.is_empty();
        out.extend(self.0.filter(input, fl));
        Ok(true)
    }
    fn flush(&mut self, _vm: &mut Vm, out: &mut Vec<u8>) -> R<bool> {
        if self.1 {
            out.extend(self.0.filter(&[], goro_ext_zlib::filter::FilterFlush::Inc));
        }
        Ok(true)
    }
}

impl StreamFilter for InflateF {
    fn filter(&mut self, vm: &mut Vm, input: &[u8], closing: bool, out: &mut Vec<u8>) -> R<bool> {
        let fl = if closing {
            goro_ext_zlib::filter::FilterFlush::Close
        } else {
            goro_ext_zlib::filter::FilterFlush::None
        };
        match self.0.filter(input, fl) {
            Ok(b) => {
                out.extend(b);
                Ok(true)
            }
            Err(zl::Diag::Notice(m)) => {
                vm.notice_msg(&format!("{}(): {m}", super::streams::caller(vm)))?;
                Ok(false)
            }
            Err(zl::Diag::Warning(m)) => {
                vm.warn(&format!("{}(): {m}", super::streams::caller(vm)))?;
                Ok(false)
            }
        }
    }
}

fn filter_params(vm: &mut Vm, p: &Value) -> R<goro_ext_zlib::filter::FilterParams> {
    use goro_ext_zlib::filter::FilterParams;
    let map = match p {
        Value::Null => return Ok(FilterParams::None),
        Value::Array(a) => a.clone(),
        Value::Object(o) => vm.object_props_cast(o),
        other => return Ok(FilterParams::Scalar(vm.to_int(other)?)),
    };
    Ok(FilterParams::Map {
        level: opt_int(vm, &map, b"level")?,
        window: opt_int(vm, &map, b"window")?,
        memory: opt_int(vm, &map, b"memory")?,
    })
}

fn zlib_filter(
    vm: &mut Vm,
    name: &[u8],
    p: &Value,
    _s: FilterSide,
) -> R<Option<Box<dyn StreamFilter>>> {
    let lname = name.to_ascii_lowercase();
    let params = filter_params(vm, &p.deref())?;
    let mut diags = Vec::new();
    let f: Option<Box<dyn StreamFilter>> = match &lname[..] {
        b"zlib.deflate" => goro_ext_zlib::filter::deflate_filter(&params, &mut diags)
            .map(|f| Box::new(DeflateF(f, false)) as Box<dyn StreamFilter>),
        b"zlib.inflate" => goro_ext_zlib::filter::inflate_filter(&params, &mut diags)
            .map(|f| Box::new(InflateF(f)) as Box<dyn StreamFilter>),
        _ => None,
    };
    for d in diags {
        match d {
            zl::Diag::Warning(m) => vm.warn(&format!("stream_filter_append(): {m}"))?,
            zl::Diag::Notice(m) => vm.notice_msg(&format!("stream_filter_append(): {m}"))?,
        }
    }
    Ok(f)
}
