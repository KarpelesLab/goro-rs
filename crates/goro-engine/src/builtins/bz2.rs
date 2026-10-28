//! ext/bz2 (PHP manual: Bzip2) on compcol's bzip2 codec: bzcompress()/
//! bzdecompress(), the bzopen() streams with their bzerr*() state, the
//! `compress.bzip2://` wrapper and the `bzip2.compress`/`bzip2.decompress`
//! filters. Error codes and names are libbzip2's public ones (bzip2
//! manual, "Error handling"); behaviour from tests ext/bz2/tests.

use std::io::Write as _;

use compcol::{Decoder as _, Encoder as _, Status};
use goro_runtime::{Key, PhpArray, PhpStr, ResRef, Value};

use super::file::{Backing, Stream, new_stream_res, open_stream};
use super::streams::{
    FilterSide, NativeStream, NativeWrapper, StreamFilter, register_filter, register_wrapper,
};
use crate::func::{Args, FuncKind};
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "bz2",
    functions: &[
        ("function bzopen($file, string $mode)", bzopen),
        (
            "function bzread($bz, int $length = 1024): string|false",
            bzread,
        ),
        (
            "function bzwrite($bz, string $data, ?int $length = null): int|false",
            bzwrite,
        ),
        ("function bzflush($bz): bool", bzflush),
        ("function bzclose($bz): bool", bzclose),
        ("function bzerrno($bz): int", bzerrno),
        ("function bzerrstr($bz): string", bzerrstr),
        ("function bzerror($bz): array", bzerror),
        (
            "function bzcompress(string $data, int $block_size = 4, int $work_factor = 0): string|int",
            bzcompress,
        ),
        (
            "function bzdecompress(string $data, bool $use_less_memory = false): string|int|false",
            bzdecompress,
        ),
    ],
    init: Some(init),
    ..Module::EMPTY
};

static WRAPPER: NativeWrapper = NativeWrapper::new(wrapper_open);

fn init(vm: &mut Vm) {
    register_wrapper(vm, "compress.bzip2", &WRAPPER);
    register_filter(vm, "bzip2.*", bz2_filter);
}

// ---------------------------------------------------------------- errors

/// libbzip2's return codes (bzip2 manual: BZ_OK, BZ_DATA_ERROR...).
const BZ_OK: i64 = 0;
const BZ_DATA_ERROR: i64 = -4;
const BZ_DATA_ERROR_MAGIC: i64 = -5;
const BZ_IO_ERROR: i64 = -6;
const BZ_UNEXPECTED_EOF: i64 = -7;

/// BZ2_bzerror()'s names for the codes.
fn errstr(n: i64) -> &'static str {
    match n {
        0 => "OK",
        -1 => "SEQUENCE_ERROR",
        -2 => "PARAM_ERROR",
        -3 => "MEM_ERROR",
        -4 => "DATA_ERROR",
        -5 => "DATA_ERROR_MAGIC",
        -6 => "IO_ERROR",
        -7 => "UNEXPECTED_EOF",
        -8 => "OUTBUFF_FULL",
        -9 => "CONFIG_ERROR",
        _ => "???",
    }
}

/// Whether `data` starts with a stream header ("BZh" and a block size).
fn good_magic(data: &[u8]) -> bool {
    data.len() >= 4 && &data[..3] == b"BZh" && (b'1'..=b'9').contains(&data[3])
}

/// The libbzip2 code of a decoding error: a bad stream header is
/// DATA_ERROR_MAGIC, anything wrong after it DATA_ERROR (compcol reports
/// both bad stream and bad block magics as BadHeader); `head` is the
/// start of the input.
fn decode_errno(e: compcol::Error, head: &[u8]) -> i64 {
    // the header is checked byte by byte: a short input that already
    // differs is a bad magic (test 005.phpt: bzdecompress("1"))
    let n = head.len().min(4);
    let prefix_ok = head[..n.min(3)] == b"BZh"[..n.min(3)]
        && (n < 4 || (b'1'..=b'9').contains(&head[3]));
    match e {
        compcol::Error::BadHeader if !good_magic(head) => BZ_DATA_ERROR_MAGIC,
        compcol::Error::UnexpectedEnd if !prefix_ok => BZ_DATA_ERROR_MAGIC,
        compcol::Error::UnexpectedEnd => BZ_UNEXPECTED_EOF,
        _ => BZ_DATA_ERROR,
    }
}

// ---------------------------------------------------------------- one-shot

fn encoder(level: i64) -> compcol::bzip2::Encoder {
    compcol::bzip2::Encoder::with_config(compcol::bzip2::EncoderConfig { level: level as u8 })
}

/// Compresses all of `data` at block size `level`.
pub fn compress(data: &[u8], level: i64) -> Vec<u8> {
    let mut enc = encoder(level);
    let mut out = Vec::with_capacity(data.len() / 2 + 64);
    let mut buf = vec![0u8; 64 * 1024];
    let mut pos = 0;
    while pos < data.len() {
        let Ok((p, _)) = enc.encode(&data[pos..], &mut buf) else {
            break;
        };
        out.extend_from_slice(&buf[..p.written]);
        pos += p.consumed;
        if p.consumed == 0 && p.written == 0 {
            break;
        }
    }
    loop {
        let Ok((p, st)) = enc.finish(&mut buf) else {
            break;
        };
        out.extend_from_slice(&buf[..p.written]);
        if st == Status::StreamEnd || (p.written == 0 && st != Status::OutputFull) {
            break;
        }
    }
    out
}

/// Decompresses the first bzip2 stream of `data` (bzdecompress()): the
/// bytes, or a libbzip2 error code.
pub fn decompress(data: &[u8]) -> Result<Vec<u8>, i64> {
    let mut dec = compcol::bzip2::Decoder::with_config(
        compcol::bzip2::DecoderConfig::default().with_multi_stream(false),
    );
    let mut out = Vec::with_capacity(data.len() * 3);
    let mut buf = vec![0u8; 64 * 1024];
    let mut pos = 0;
    loop {
        let r = if pos < data.len() {
            dec.decode(&data[pos..], &mut buf)
        } else {
            dec.finish(&mut buf)
        };
        let (p, st) = r.map_err(|e| decode_errno(e, data))?;
        out.extend_from_slice(&buf[..p.written]);
        pos += p.consumed;
        match st {
            Status::StreamEnd => return Ok(out),
            Status::OutputFull => {}
            Status::InputEmpty => {
                if pos >= data.len() && p.written == 0 && p.consumed == 0 {
                    // finish made no progress: the stream is cut short
                    return Err(BZ_UNEXPECTED_EOF);
                }
            }
        }
    }
}

fn bzcompress(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.bytes(0);
    let bs = a.int_or(1, 4);
    let wf = a.int_or(2, 0);
    if data.len() as u64 > u32::MAX as u64 {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #1 ($data) must have a length less than or equal to {}",
                u32::MAX
            ),
        );
    }
    if !(1..=9).contains(&bs) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($block_size) must be between 1 and 9",
        );
    }
    if !(0..=250).contains(&wf) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($work_factor) must be between 0 and 250",
        );
    }
    Ok(Value::Str(PhpStr::from(compress(a.bytes(0), bs))))
}

fn bzdecompress(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(match decompress(a.bytes(0)) {
        Ok(b) => Value::Str(PhpStr::from(b)),
        Err(n) => Value::Int(n),
    })
}

// ---------------------------------------------------------------- streams

/// Where a bzip2 stream's compressed bytes come from or go to.
enum Inner {
    /// A plain file (finished on drop when never closed).
    File(std::fs::File),
    /// A stream from another wrapper, opened for this stream.
    Stream(Box<Stream>),
    /// A stream resource given to bzopen().
    Res(ResRef),
}

/// Runs `f` on the stream inside a stream resource.
fn on_res<T>(
    r: &ResRef,
    f: impl FnOnce(&mut Stream) -> std::io::Result<T>,
) -> std::io::Result<T> {
    let data = r.res().data.borrow();
    let cell = data
        .as_ref()
        .and_then(|d| d.downcast_ref::<std::cell::RefCell<Stream>>())
        .ok_or_else(|| std::io::Error::other("not a stream"))?;
    let mut s = cell
        .try_borrow_mut()
        .map_err(|_| std::io::Error::other("stream busy"))?;
    f(&mut s)
}

impl Inner {
    fn read(&mut self, vm: &mut Vm, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Inner::File(f) => std::io::Read::read(f, buf),
            Inner::Stream(s) => s.read_some(vm, buf),
            Inner::Res(r) => on_res(r, |s| s.read_some(vm, buf)),
        }
    }
    fn write(&mut self, vm: &mut Vm, data: &[u8]) -> std::io::Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        match self {
            Inner::File(f) => f.write_all(data),
            Inner::Stream(s) => s.write_all(vm, data).map(|_| ()),
            Inner::Res(r) => on_res(r, |s| s.write_all(vm, data).map(|_| ())),
        }
    }
}

/// A bzopen() stream: decoding or encoding, with the last error.
pub struct BzStream {
    inner: Option<Inner>,
    dec: Option<Box<compcol::bzip2::Decoder>>,
    enc: Option<Box<compcol::bzip2::Encoder>>,
    inbuf: Vec<u8>,
    inpos: usize,
    in_eof: bool,
    head: Vec<u8>,
    done: bool,
    errno: i64,
}

impl BzStream {
    fn reader(inner: Inner) -> BzStream {
        BzStream {
            inner: Some(inner),
            dec: Some(Box::new(compcol::bzip2::Decoder::with_config(
                compcol::bzip2::DecoderConfig::default().with_multi_stream(false),
            ))),
            enc: None,
            inbuf: Vec::new(),
            inpos: 0,
            in_eof: false,
            head: Vec::new(),
            done: false,
            errno: BZ_OK,
        }
    }

    fn writer(inner: Inner, level: i64) -> BzStream {
        BzStream {
            inner: Some(inner),
            dec: None,
            enc: Some(Box::new(encoder(level))),
            inbuf: Vec::new(),
            inpos: 0,
            in_eof: false,
            head: Vec::new(),
            done: false,
            errno: BZ_OK,
        }
    }

    fn fail(&mut self, n: i64) -> std::io::Error {
        self.errno = n;
        std::io::Error::other(errstr(n))
    }

    /// Writes encoder output until `step` reports nothing more.
    fn pump(
        &mut self,
        vm: Option<&mut Vm>,
        mut step: impl FnMut(
            &mut compcol::bzip2::Encoder,
            &mut [u8],
        ) -> Result<(compcol::Progress, Status), compcol::Error>,
    ) -> std::io::Result<()> {
        let mut out = Vec::new();
        let mut buf = vec![0u8; 32 * 1024];
        if let Some(enc) = self.enc.as_mut() {
            loop {
                let (p, st) = step(enc, &mut buf).map_err(std::io::Error::other)?;
                out.extend_from_slice(&buf[..p.written]);
                if st != Status::OutputFull {
                    break;
                }
            }
        }
        match (self.inner.as_mut(), vm) {
            (Some(Inner::File(f)), _) => f.write_all(&out),
            (Some(i), Some(vm)) => i.write(vm, &out),
            _ => Ok(()),
        }
    }

    /// Ends the compressed stream (close, or the stream going away).
    fn finish(&mut self, vm: Option<&mut Vm>) {
        if self.enc.is_some() && !self.done {
            self.done = true;
            let _ = self.pump(vm, |e, b| e.finish(b));
        }
    }
}

impl Drop for BzStream {
    fn drop(&mut self) {
        if matches!(self.inner, Some(Inner::File(_))) {
            self.finish(None);
        }
    }
}

impl NativeStream for BzStream {
    fn read(&mut self, vm: &mut Vm, out: &mut [u8]) -> std::io::Result<usize> {
        if self.errno != BZ_OK {
            return Err(std::io::Error::other(errstr(self.errno)));
        }
        if self.dec.is_none() || self.done || out.is_empty() {
            return Ok(0);
        }
        loop {
            if self.inpos >= self.inbuf.len() && !self.in_eof {
                self.inbuf.resize(8192, 0);
                self.inpos = 0;
                let n = match self.inner.as_mut() {
                    Some(i) => match i.read(vm, &mut self.inbuf) {
                        Ok(n) => n,
                        Err(_) => {
                            self.inbuf.clear();
                            return Err(self.fail(BZ_IO_ERROR));
                        }
                    },
                    None => 0,
                };
                self.inbuf.truncate(n);
                if n == 0 {
                    self.in_eof = true;
                } else if self.head.len() < 4 {
                    let k = (4 - self.head.len()).min(n);
                    self.head.extend_from_slice(&self.inbuf[..k]);
                }
            }
            let dec = self.dec.as_mut().unwrap();
            let r = if self.inpos < self.inbuf.len() {
                dec.decode(&self.inbuf[self.inpos..], out)
            } else {
                dec.finish(out)
            };
            let (p, st) = match r {
                Ok(x) => x,
                Err(e) => {
                    let n = decode_errno(e, &self.head);
                    return Err(self.fail(n));
                }
            };
            self.inpos += p.consumed;
            if st == Status::StreamEnd {
                self.done = true;
                return Ok(p.written);
            }
            if p.written > 0 {
                return Ok(p.written);
            }
            if self.in_eof && self.inpos >= self.inbuf.len() && p.consumed == 0 {
                return Err(self.fail(BZ_UNEXPECTED_EOF));
            }
        }
    }

    fn write(&mut self, vm: &mut Vm, data: &[u8]) -> std::io::Result<usize> {
        if self.enc.is_none() {
            return Err(std::io::Error::other("stream is not writable"));
        }
        let mut pos = 0;
        let mut out = Vec::new();
        let mut buf = vec![0u8; 32 * 1024];
        let enc = self.enc.as_mut().unwrap();
        while pos < data.len() {
            let (p, _) = enc
                .encode(&data[pos..], &mut buf)
                .map_err(std::io::Error::other)?;
            out.extend_from_slice(&buf[..p.written]);
            pos += p.consumed;
            if p.consumed == 0 && p.written == 0 {
                break;
            }
        }
        if let Some(i) = self.inner.as_mut() {
            i.write(vm, &out)?;
        }
        Ok(data.len())
    }

    fn flush(&mut self, vm: &mut Vm) -> std::io::Result<()> {
        if self.enc.is_some() && !self.done {
            self.pump(Some(vm), |e, b| e.flush(b, compcol::Flush::Sync))?;
        }
        Ok(())
    }

    fn eof(&self) -> bool {
        self.done || self.errno != BZ_OK
    }

    fn close(&mut self, vm: &mut Vm) -> R<()> {
        self.finish(Some(vm));
        if let Some(Inner::Stream(mut s)) = self.inner.take() {
            super::streams::close_stream(vm, &mut s)?;
        }
        Ok(())
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

/// Opens a bzip2 stream on a path or URL for reading (`r`) or writing.
fn open_bz(
    vm: &mut Vm,
    a: &Args,
    path: &[u8],
    write: bool,
    uri: &[u8],
    quiet: bool,
) -> R<Option<Stream>> {
    let mode: &[u8] = if write { b"wb" } else { b"rb" };
    let Some(mut s) = open_stream(vm, a, path, mode, quiet)? else {
        return Ok(None);
    };
    let inner = match std::mem::replace(
        &mut s.backing,
        Backing::Memory {
            data: Vec::new(),
            pos: 0,
        },
    ) {
        Backing::File(f) => Inner::File(f),
        other => {
            s.backing = other;
            Inner::Stream(Box::new(s))
        }
    };
    Ok(Some(new_bz_stream(inner, write, uri)))
}

fn new_bz_stream(inner: Inner, write: bool, uri: &[u8]) -> Stream {
    let bz = if write {
        // libbzip2's BZ2_bzopen() default block size (bzip2 manual)
        BzStream::writer(inner, 9)
    } else {
        BzStream::reader(inner)
    };
    let mode: &[u8] = if write { b"w" } else { b"r" };
    Stream::new(Backing::Native(Box::new(bz)), mode, uri, "", "BZip2")
}

fn wrapper_open(vm: &mut Vm, a: &Args, url: &[u8], mode: &[u8], quiet: bool) -> R<Option<Stream>> {
    let path = &url[b"compress.bzip2://".len()..];
    let write = match mode.iter().find(|c| !matches!(c, b'b' | b't')) {
        Some(b'r') if !mode.contains(&b'+') => false,
        Some(b'w') if !mode.contains(&b'+') => true,
        _ => {
            if !quiet {
                vm.warn(&format!(
                    "{}({}): Failed to open stream: operation failed",
                    a.fname(),
                    String::from_utf8_lossy(url)
                ))?;
            }
            return Ok(None);
        }
    };
    open_bz(vm, a, path, write, url, quiet)
}

fn bzopen(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mode = a.str(1);
    let write = match &mode[..] {
        b"r" => false,
        b"w" => true,
        _ => {
            if let Value::Str(s) = a.val(0)
                && s.is_empty()
            {
                return vm.fthrow(a, "ValueError", "Argument #1 ($file) must not be empty");
            }
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #2 ($mode) must be either \"r\" or \"w\"",
            );
        }
    };
    match a.val(0) {
        Value::Resource(r) => {
            let smode = {
                let data = r.res().data.borrow();
                data.as_ref()
                    .and_then(|d| d.downcast_ref::<std::cell::RefCell<Stream>>())
                    .map(|c| c.borrow().mode.to_vec())
            };
            let Some(smode) = smode else {
                return vm.fthrow(
                    a,
                    "TypeError",
                    "Argument #1 ($file) must be of type string or file-resource, resource given",
                );
            };
            // the stream's own mode decides (tests 002.phpt, gh19810.phpt)
            let core: Vec<u8> = smode
                .iter()
                .copied()
                .filter(|c| !matches!(c, b'b' | b't' | b'e'))
                .collect();
            if core.len() > 1 {
                vm.fwarn(
                    a,
                    &format!(
                        "Cannot use stream opened in mode '{}'",
                        String::from_utf8_lossy(&smode)
                    ),
                )?;
                return Ok(Value::Bool(false));
            }
            let readable = core.first() == Some(&b'r');
            if !write && !readable {
                vm.fwarn(a, "Cannot read from a stream opened in write only mode")?;
                return Ok(Value::Bool(false));
            }
            if write && readable {
                vm.fwarn(a, "cannot write to a stream opened in read only mode")?;
                return Ok(Value::Bool(false));
            }
            let uri = {
                let data = r.res().data.borrow();
                data.as_ref()
                    .and_then(|d| d.downcast_ref::<std::cell::RefCell<Stream>>())
                    .map(|c| c.borrow().uri.to_vec())
                    .unwrap_or_default()
            };
            let s = new_bz_stream(Inner::Res(r), write, &uri);
            Ok(Value::Resource(new_stream_res(s)))
        }
        Value::Str(f) => {
            if f.is_empty() {
                return vm.fthrow(a, "ValueError", "Argument #1 ($file) must not be empty");
            }
            if f.contains(&0) {
                return vm.fthrow(
                    a,
                    "TypeError",
                    "Argument #1 ($file) must not contain null bytes",
                );
            }
            match open_bz(vm, a, &f, write, &f, false)? {
                Some(s) => Ok(Value::Resource(new_stream_res(s))),
                None => Ok(Value::Bool(false)),
            }
        }
        other => vm.fthrow(
            a,
            "TypeError",
            &format!(
                "Argument #1 ($file) must be of type string or file-resource, {} given",
                Vm::type_name_given(&other)
            ),
        ),
    }
}

/// Runs the file function `name` with these arguments (bzread is fread on
/// the stream bzopen() returned, PHP manual).
fn delegate(vm: &mut Vm, a: &mut Args, name: &str) -> R<Value> {
    if let Value::Resource(r) = a.val(0)
        && r.is_closed()
    {
        return vm.fthrow(
            a,
            "TypeError",
            "Argument #1 ($bz) must be an open stream resource",
        );
    }
    let f = vm.functions.get(&PhpStr::from(name)).cloned();
    match f.as_deref().map(|f| &f.kind) {
        Some(FuncKind::Native { handler, .. }) => handler(vm, a),
        _ => Err(vm.fatal(&format!("Call to undefined function {name}()"))),
    }
}

/// The error code of a bzopen() stream; none for another stream.
fn stream_errno(vm: &mut Vm, a: &Args) -> R<Option<i64>> {
    super::file::with_stream(vm, a, 0, |_, s| {
        Ok(match &mut s.backing {
            Backing::Native(n) => n
                .as_any_mut()
                .and_then(|x| x.downcast_mut::<BzStream>())
                .map(|b| b.errno),
            _ => None,
        })
    })
}

fn bzread(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if let Value::Resource(r) = a.val(0)
        && r.is_closed()
    {
        return vm.fthrow(
            a,
            "TypeError",
            "Argument #1 ($bz) must be an open stream resource",
        );
    }
    let n = a.int_or(1, 1024);
    if n < 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($length) must be greater than or equal to 0",
        );
    }
    if n == 0 {
        return Ok(Value::from(""));
    }
    // a read that fails decoding gives false (tests 004.phpt, bug72613)
    if stream_errno(vm, a)?.is_some_and(|e| e != BZ_OK) {
        return Ok(Value::Bool(false));
    }
    if a.len() < 2 {
        a.vals.push(Value::Int(n));
    }
    let v = delegate(vm, a, "fread")?;
    if stream_errno(vm, a)?.is_some_and(|e| e != BZ_OK) {
        return Ok(Value::Bool(false));
    }
    Ok(v)
}

fn bzwrite(vm: &mut Vm, a: &mut Args) -> R<Value> {
    delegate(vm, a, "fwrite")
}
fn bzflush(vm: &mut Vm, a: &mut Args) -> R<Value> {
    delegate(vm, a, "fflush")
}
fn bzclose(vm: &mut Vm, a: &mut Args) -> R<Value> {
    delegate(vm, a, "fclose")
}

/// The code of a bz2 stream argument, or the TypeErrors.
fn err_arg(vm: &mut Vm, a: &Args) -> R<i64> {
    if let Value::Resource(r) = a.val(0)
        && r.is_closed()
    {
        return vm.fthrow(
            a,
            "TypeError",
            "Argument #1 ($bz) must be an open stream resource",
        );
    }
    match stream_errno(vm, a)? {
        Some(n) => Ok(n),
        None => vm.fthrow(a, "TypeError", "Argument #1 ($bz) must be a bz2 stream"),
    }
}

fn bzerrno(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(err_arg(vm, a)?))
}
fn bzerrstr(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = err_arg(vm, a)?;
    Ok(Value::from(errstr(n)))
}
fn bzerror(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = err_arg(vm, a)?;
    let mut arr = PhpArray::new();
    arr.insert(Key::from("errno"), Value::Int(n));
    arr.insert(Key::from("errstr"), Value::from(errstr(n)));
    Ok(Value::Array(arr))
}

// ---------------------------------------------------------------- filters

struct CompressF {
    enc: compcol::bzip2::Encoder,
    done: bool,
}

impl CompressF {
    fn run(
        &mut self,
        input: &[u8],
        out: &mut Vec<u8>,
        last: Option<bool>,
    ) -> Result<(), compcol::Error> {
        let mut buf = vec![0u8; 32 * 1024];
        let mut pos = 0;
        while pos < input.len() {
            let (p, _) = self.enc.encode(&input[pos..], &mut buf)?;
            out.extend_from_slice(&buf[..p.written]);
            pos += p.consumed;
            if p.consumed == 0 && p.written == 0 {
                break;
            }
        }
        match last {
            Some(true) => loop {
                let (p, st) = self.enc.finish(&mut buf)?;
                out.extend_from_slice(&buf[..p.written]);
                if st != Status::OutputFull {
                    self.done = true;
                    break;
                }
            },
            Some(false) => loop {
                let (p, st) = self.enc.flush(&mut buf, compcol::Flush::Sync)?;
                out.extend_from_slice(&buf[..p.written]);
                if st != Status::OutputFull {
                    break;
                }
            },
            None => {}
        }
        Ok(())
    }
}

impl StreamFilter for CompressF {
    fn filter(&mut self, _vm: &mut Vm, input: &[u8], closing: bool, out: &mut Vec<u8>) -> R<bool> {
        if self.done {
            return Ok(true);
        }
        Ok(self.run(input, out, closing.then_some(true)).is_ok())
    }
    fn flush(&mut self, _vm: &mut Vm, out: &mut Vec<u8>) -> R<bool> {
        if self.done {
            return Ok(true);
        }
        Ok(self.run(&[], out, Some(false)).is_ok())
    }
}

struct DecompressF {
    dec: compcol::bzip2::Decoder,
    concatenated: bool,
    done: bool,
    failed: bool,
}

impl StreamFilter for DecompressF {
    fn filter(&mut self, vm: &mut Vm, input: &[u8], closing: bool, out: &mut Vec<u8>) -> R<bool> {
        if self.failed {
            return Ok(false);
        }
        let mut buf = vec![0u8; 32 * 1024];
        let mut pos = 0;
        while pos < input.len() && !self.done {
            match self.dec.decode(&input[pos..], &mut buf) {
                Ok((p, st)) => {
                    out.extend_from_slice(&buf[..p.written]);
                    pos += p.consumed;
                    if st == Status::StreamEnd {
                        if self.concatenated && pos < input.len() {
                            self.dec.reset();
                            continue;
                        }
                        self.done = true;
                    }
                    if p.consumed == 0 && p.written == 0 {
                        break;
                    }
                }
                Err(_) => {
                    self.failed = true;
                    vm.notice_msg(&format!(
                        "{}(): bzip2 decompression failed",
                        super::streams::caller(vm)
                    ))?;
                    return Ok(false);
                }
            }
        }
        // a stream cut short ends without an error (test bug71263.phpt)
        if closing && !self.done {
            loop {
                match self.dec.finish(&mut buf) {
                    Ok((p, st)) => {
                        out.extend_from_slice(&buf[..p.written]);
                        if st != Status::OutputFull {
                            break;
                        }
                    }
                    Err(compcol::Error::UnexpectedEnd) => break,
                    Err(_) => {
                        self.failed = true;
                        vm.notice_msg(&format!(
                            "{}(): bzip2 decompression failed",
                            super::streams::caller(vm)
                        ))?;
                        return Ok(false);
                    }
                }
            }
        }
        Ok(true)
    }
}

/// A filter parameter by name, from an array or an object's properties.
fn param(vm: &mut Vm, p: &Value, k: &[u8]) -> Option<Value> {
    let map = match p {
        Value::Array(a) => a.clone(),
        Value::Object(o) => vm.object_props_cast(o),
        _ => return None,
    };
    map.get_bytes(k).map(|v| v.deref())
}

fn bz2_filter(
    vm: &mut Vm,
    name: &[u8],
    p: &Value,
    _s: FilterSide,
) -> R<Option<Box<dyn StreamFilter>>> {
    let p = p.deref();
    match &name.to_ascii_lowercase()[..] {
        b"bzip2.compress" => {
            // parameters `blocks` (1-9, default 4) and `work` (0-250),
            // an integer alone being the block count (test bug72447.phpt)
            let mut level = 4;
            let blocks = match &p {
                Value::Array(_) | Value::Object(_) => param(vm, &p, b"blocks"),
                Value::Null => None,
                other => Some(other.clone()),
            };
            if let Some(b) = blocks {
                let n = vm.to_int(&b)?;
                if (1..=9).contains(&n) {
                    level = n;
                } else {
                    vm.warn(&format!(
                        "stream_filter_append(): Invalid parameter given for number of blocks to allocate ({n})"
                    ))?;
                }
            }
            if let Some(w) = param(vm, &p, b"work") {
                let n = vm.to_int(&w)?;
                if !(0..=250).contains(&n) {
                    vm.warn(&format!(
                        "stream_filter_append(): Invalid parameter given for work factor ({n})"
                    ))?;
                }
            }
            Ok(Some(Box::new(CompressF {
                enc: encoder(level),
                done: false,
            })))
        }
        b"bzip2.decompress" => {
            let concatenated = match &p {
                Value::Array(_) | Value::Object(_) => match param(vm, &p, b"concatenated") {
                    Some(v) => vm.to_bool(&v),
                    None => false,
                },
                Value::Null => false,
                other => vm.to_bool(other),
            };
            Ok(Some(Box::new(DecompressF {
                dec: compcol::bzip2::Decoder::with_config(
                    compcol::bzip2::DecoderConfig::default().with_multi_stream(false),
                ),
                concatenated,
                done: false,
                failed: false,
            })))
        }
        _ => Ok(None),
    }
}
