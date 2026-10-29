//! Stream wrappers and stream filters provided by Rust modules (PHP
//! manual: Streams; stream_filter_append, stream_get_filters), and the
//! built-in `string.*` and `convert.base64-*` filters.
//!
//! Extensions register from their module's `init`:
//! - [`register_wrapper`]: a scheme (`compress.zlib`) whose `open` builds
//!   the [`Stream`] (usually [`Backing::Native`] around a [`NativeStream`]);
//! - [`register_filter`]: a filter name (`zlib.deflate`) or family
//!   (`convert.iconv.*`) whose factory builds a [`StreamFilter`].

use std::collections::HashMap;

use goro_runtime::{PhpArray, PhpStr, ResRef, Value};

use super::file::{Backing, Stream, with_stream};
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "streams",
    functions: &[
        (
            "function stream_filter_append($stream, string $filter_name, int $mode = 0, mixed $params = null)",
            stream_filter_append,
        ),
        (
            "function stream_filter_prepend($stream, string $filter_name, int $mode = 0, mixed $params = null)",
            stream_filter_prepend,
        ),
        (
            "function stream_filter_remove($stream_filter): bool",
            stream_filter_remove,
        ),
        ("function stream_get_filters(): array", stream_get_filters),
        (
            "function stream_filter_register(string $filter_name, string $class): bool",
            stream_filter_register,
        ),
        (
            "function stream_bucket_make_writeable($brigade): ?StreamBucket",
            stream_bucket_make_writeable,
        ),
        (
            "function stream_bucket_append($brigade, StreamBucket $bucket): void",
            stream_bucket_append,
        ),
        (
            "function stream_bucket_prepend($brigade, StreamBucket $bucket): void",
            stream_bucket_prepend,
        ),
        (
            "function stream_bucket_new($stream, string $buffer): StreamBucket",
            stream_bucket_new,
        ),
    ],
    classes: &[
        (
            "php_user_filter",
            "<?php class php_user_filter {
                public string $filtername = \"\";
                public mixed $params = \"\";
                public $stream = null;
                public function filter($in, $out, &$consumed, bool $closing): int { return PSFS_ERR_FATAL; }
                public function onCreate(): bool { return true; }
                public function onClose(): void {}
            }",
        ),
        (
            "streambucket",
            "<?php final class StreamBucket {
                public $bucket;
                public string $data;
                public int $datalen;
                private function __construct() {}
            }",
        ),
    ],
    constants: &[
        ("STREAM_FILTER_READ", ConstVal::Int(1)),
        ("STREAM_FILTER_WRITE", ConstVal::Int(2)),
        ("STREAM_FILTER_ALL", ConstVal::Int(3)),
        ("PSFS_PASS_ON", ConstVal::Int(2)),
        ("PSFS_FEED_ME", ConstVal::Int(1)),
        ("PSFS_ERR_FATAL", ConstVal::Int(0)),
        ("PSFS_FLAG_NORMAL", ConstVal::Int(0)),
        ("PSFS_FLAG_FLUSH_INC", ConstVal::Int(1)),
        ("PSFS_FLAG_FLUSH_CLOSE", ConstVal::Int(2)),
    ],
    init: Some(init),
    listed: false,
    ..Module::EMPTY
};

/// The I/O of a stream an extension implements (the inside of
/// `compress.zlib://`...). Positions and EOF are the stream's own.
pub trait NativeStream {
    /// Reads up to `buf.len()` bytes; 0 at the end.
    fn read(&mut self, vm: &mut Vm, buf: &mut [u8]) -> std::io::Result<usize>;
    /// Writes `data`, returning how much was taken.
    fn write(&mut self, _vm: &mut Vm, _data: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("stream is not writable"))
    }
    /// fflush().
    fn flush(&mut self, _vm: &mut Vm) -> std::io::Result<()> {
        Ok(())
    }
    /// fseek(): the new position, or `None` when the stream can't seek.
    fn seek(&mut self, _vm: &mut Vm, _pos: std::io::SeekFrom) -> Option<std::io::Result<u64>> {
        None
    }
    /// ftell().
    fn tell(&mut self) -> Option<u64> {
        None
    }
    /// Whether the end was reached.
    fn eof(&self) -> bool {
        false
    }
    /// fclose() (or the resource going away): trailers and the like.
    fn close(&mut self, _vm: &mut Vm) -> R<()> {
        Ok(())
    }
    /// The stream as `Any`, for modules that downcast their own streams.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }
    /// The descriptor stream_select() polls, if any.
    fn raw_fd(&self) -> Option<i32> {
        None
    }
    /// Whether reads can return data without touching the descriptor
    /// (decrypted TLS bytes): stream_select() then reports it readable.
    fn buffered(&self) -> bool {
        false
    }
    /// A socket: reads return what is available (like pipes), and
    /// stream_get_meta_data() has no wrapper fields.
    fn is_socket(&self) -> bool {
        false
    }
    /// stream_set_blocking(); false when unsupported.
    fn set_blocking(&mut self, _blocking: bool) -> bool {
        false
    }
    /// stream_set_timeout(); false when unsupported.
    fn set_timeout(&mut self, _secs: f64) -> bool {
        false
    }
    /// Entries for stream_get_meta_data() (timed_out, blocked, crypto...).
    fn meta(&self, _m: &mut goro_runtime::PhpArray) {}
    /// The stream context attached to the stream, if any.
    fn context(&self) -> Option<Value> {
        None
    }
    /// stream_get_meta_data()'s "wrapper_data" (http://: the response
    /// headers).
    fn wrapper_data(&self) -> Option<Value> {
        None
    }
}

/// A stream wrapper implemented in Rust.
pub struct NativeWrapper {
    /// Opens `url` with fopen's `mode`; warns (unless `quiet`) and returns
    /// none on failure, as fopen() does.
    pub open: fn(&mut Vm, &Args, &[u8], &[u8], bool) -> R<Option<Stream>>,
}

/// Which side of a stream a filter is attached to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FilterSide {
    Read,
    Write,
}

/// A stream filter (PHP manual: php_user_filter for the model): data
/// passes through `filter` in chunks; `closing` is set once at the end
/// (EOF on read, fclose() on write) for the filter to flush its state.
pub trait StreamFilter {
    /// Appends the filtered form of `input` to `out`. An `Err` is a
    /// diagnostic already raised; `Ok(false)` a fatal filter error (the
    /// read or write fails).
    fn filter(&mut self, vm: &mut Vm, input: &[u8], closing: bool, out: &mut Vec<u8>) -> R<bool>;
    /// The filter is removed or its stream closed.
    fn on_close(&mut self, _vm: &mut Vm) -> R<()> {
        Ok(())
    }
    /// `fflush()` or a seek: whatever the filter holds back (zlib: a sync
    /// flush).
    fn flush(&mut self, _vm: &mut Vm, _out: &mut Vec<u8>) -> R<bool> {
        Ok(true)
    }
}

/// Runs the write filters' flush and writes what comes out.
pub(crate) fn flush_write_filters(vm: &mut Vm, s: &mut Stream) -> R<()> {
    if s.write_filters.is_empty() {
        return Ok(());
    }
    let mut chain = std::mem::take(&mut s.write_filters);
    let mut cur: Vec<u8> = Vec::new();
    let mut r = Ok(true);
    for f in chain.iter_mut() {
        let mut out = Vec::new();
        if !cur.is_empty() {
            match f.filter.filter(vm, &cur, false, &mut out) {
                Ok(true) => {}
                other => {
                    r = other;
                    break;
                }
            }
        }
        match f.filter.flush(vm, &mut out) {
            Ok(true) => {}
            other => {
                r = other;
                break;
            }
        }
        cur = out;
    }
    s.write_filters = chain;
    if matches!(r, Ok(true)) && !cur.is_empty() {
        let _ = s.write_raw(vm, &cur);
    }
    r.map(|_| ())
}

/// Builds a filter for `name` with `params` on a side of a stream; none
/// when the name or parameters are not accepted ("Unable to create or
/// locate filter").
pub type FilterFactory = fn(&mut Vm, &[u8], &Value, FilterSide) -> R<Option<Box<dyn StreamFilter>>>;

/// The registered wrappers and filters.
#[derive(Default)]
pub struct StreamRegistry {
    wrappers: HashMap<Vec<u8>, &'static NativeWrapper>,
    /// (name or `family.*`, factory) in registration order.
    filters: Vec<(&'static str, FilterFactory)>,
    /// stream_filter_register(): (name or `family.*`, class name).
    user_filters: Vec<(Vec<u8>, PhpStr)>,
}

/// Registers a stream wrapper for `scheme` (lower case, without `://`).
pub fn register_wrapper(vm: &mut Vm, scheme: &str, w: &'static NativeWrapper) {
    vm.ext::<StreamRegistry>()
        .wrappers
        .insert(scheme.as_bytes().to_ascii_lowercase(), w);
}

/// Registers a filter name (`zlib.inflate`) or family (`convert.iconv.*`,
/// matching every name with that prefix).
pub fn register_filter(vm: &mut Vm, name: &'static str, f: FilterFactory) {
    vm.ext::<StreamRegistry>().filters.push((name, f));
}

/// The internal function running (`fread`, `fwrite`...), which prefixes
/// the diagnostics of filters and wrappers.
pub fn caller(vm: &Vm) -> String {
    vm.ex
        .native_calls
        .last()
        .map_or_else(String::new, |c| c.func.display_name())
}

/// The native wrapper of `url`'s scheme, if one is registered.
pub(crate) fn wrapper_for(vm: &mut Vm, url: &[u8]) -> Option<&'static NativeWrapper> {
    let p = url.windows(3).position(|w| w == b"://")?;
    let scheme = url[..p].to_ascii_lowercase();
    vm.ext::<StreamRegistry>().wrappers.get(&scheme).copied()
}

/// The schemes of the registered wrappers (stream_get_wrappers()).
pub(crate) fn wrapper_names(vm: &mut Vm) -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = vm
        .ext::<StreamRegistry>()
        .wrappers
        .keys()
        .cloned()
        .collect();
    v.sort();
    v
}

fn find_factory(vm: &mut Vm, name: &[u8]) -> Option<FilterFactory> {
    // An exact name wins, then the longest family prefix
    // (`convert.iconv.*` before `convert.*`).
    let reg = vm.ext::<StreamRegistry>();
    let mut best: Option<(usize, FilterFactory)> = None;
    for (n, f) in &reg.filters {
        if let Some(prefix) = n.strip_suffix('*') {
            if name.len() > prefix.len()
                && name[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
                && best.is_none_or(|(l, _)| prefix.len() > l)
            {
                best = Some((prefix.len(), *f));
            }
        } else if name.eq_ignore_ascii_case(n.as_bytes()) {
            return Some(*f);
        }
    }
    best.map(|(_, f)| f)
}

// ---------------------------------------------------------------- filters on streams

/// A filter attached to a stream.
pub struct AttachedFilter {
    pub id: u64,
    pub side: FilterSide,
    pub filter: Box<dyn StreamFilter>,
}

/// Runs data through a chain of filters.
pub(crate) fn run_chain(
    vm: &mut Vm,
    chain: &mut [AttachedFilter],
    data: &[u8],
    closing: bool,
) -> R<Option<Vec<u8>>> {
    let mut cur = data.to_vec();
    for f in chain.iter_mut() {
        let mut out = Vec::new();
        if !f.filter.filter(vm, &cur, closing, &mut out)? {
            return Ok(None);
        }
        cur = out;
    }
    Ok(Some(cur))
}

/// The `stream filter` resource: which stream and which of its filters.
struct FilterRes {
    stream: ResRef,
    id: u64,
}

fn attach(vm: &mut Vm, a: &Args, prepend: bool) -> R<Value> {
    let name = a.str(1);
    let mode = a.int_or(2, 0);
    let params = a.val(3);
    let Value::Resource(res) = a.val(0).deref() else {
        return with_stream(vm, a, 0, |_, _| Ok(Value::Bool(false)));
    };
    let user = user_filter_class(vm, &name);
    let factory = find_factory(vm, &name);
    if user.is_none() && factory.is_none() {
        vm.fwarn(
            a,
            &format!("Unable to locate filter \"{}\"", name.to_string_lossy()),
        )?;
        return Ok(Value::Bool(false));
    }
    let (readable, writable) = with_stream(vm, a, 0, |_, s| Ok((s.readable, s.writable)))?;
    let sides: Vec<FilterSide> = match mode {
        1 => vec![FilterSide::Read],
        2 => vec![FilterSide::Write],
        3 => vec![FilterSide::Read, FilterSide::Write],
        _ => {
            let mut v = Vec::new();
            if readable {
                v.push(FilterSide::Read);
            }
            if writable {
                v.push(FilterSide::Write);
            }
            v
        }
    };
    let id = {
        let n = &mut vm.ext::<FilterIds>().0;
        *n += 1;
        *n
    };
    for side in sides {
        let made = match (&user, factory) {
            (Some(cls), _) => new_user_filter(vm, cls, &name, &params, Some(&res))?,
            (None, Some(f)) => f(vm, &name, &params, side)?,
            (None, None) => None,
        };
        let Some(f) = made else {
            vm.fwarn(
                a,
                &format!(
                    "Unable to create or locate filter \"{}\"",
                    name.to_string_lossy()
                ),
            )?;
            return Ok(Value::Bool(false));
        };
        let af = AttachedFilter {
            id,
            side,
            filter: f,
        };
        with_stream(vm, a, 0, |_, s| {
            let chain = match side {
                FilterSide::Read => &mut s.read_filters,
                FilterSide::Write => &mut s.write_filters,
            };
            if prepend {
                chain.insert(0, af);
            } else {
                chain.push(af);
            }
            Ok(())
        })?;
    }
    Ok(Value::Resource(ResRef::new(
        PhpStr::intern(b"stream filter"),
        Box::new(FilterRes {
            stream: res.clone(),
            id,
        }),
    )))
}

#[derive(Default)]
struct FilterIds(u64);

fn stream_filter_append(vm: &mut Vm, a: &mut Args) -> R<Value> {
    attach(vm, a, false)
}

fn stream_filter_prepend(vm: &mut Vm, a: &mut Args) -> R<Value> {
    attach(vm, a, true)
}

fn stream_filter_remove(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let found = match &v {
        Value::Resource(r) if !r.is_closed() => {
            let data = r.res().data.borrow();
            data.as_ref()
                .and_then(|d| d.downcast_ref::<FilterRes>())
                .map(|f| (f.stream.clone(), f.id))
        }
        _ => None,
    };
    let Some((stream, id)) = found.filter(|(s, _)| !s.is_closed()) else {
        return vm.fthrow(
            a,
            "TypeError",
            "supplied resource is not a valid stream filter resource",
        );
    };
    // Pending write-side output is flushed through the rest of the chain.
    let data = stream.res().data.borrow();
    if let Some(cell) = data
        .as_ref()
        .and_then(|d| d.downcast_ref::<std::cell::RefCell<Stream>>())
    {
        let cell: *const std::cell::RefCell<Stream> = cell;
        drop(data);
        // SAFETY: `stream` keeps the resource alive for the call.
        let mut s = unsafe { (*cell).borrow_mut() };
        let mut gone: Vec<AttachedFilter> = Vec::new();
        let st = &mut *s;
        for chain in [&mut st.read_filters, &mut st.write_filters] {
            let mut i = 0;
            while i < chain.len() {
                if chain[i].id == id {
                    gone.push(chain.remove(i));
                } else {
                    i += 1;
                }
            }
        }
        drop(s);
        for f in gone.iter_mut() {
            f.filter.on_close(vm)?;
        }
    }
    if let Value::Resource(r) = &v {
        r.close();
    }
    Ok(Value::Bool(true))
}

fn stream_get_filters(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let names: Vec<Value> = vm
        .ext::<StreamRegistry>()
        .filters
        .iter()
        .map(|(n, _)| Value::from(*n))
        .collect();
    Ok(Value::Array(PhpArray::from_values(names)))
}

// ---------------------------------------------------------------- built-in filters

struct MapFilter(fn(u8) -> u8);

impl StreamFilter for MapFilter {
    fn filter(&mut self, _vm: &mut Vm, input: &[u8], _c: bool, out: &mut Vec<u8>) -> R<bool> {
        out.extend(input.iter().map(|&b| (self.0)(b)));
        Ok(true)
    }
}

fn rot13(b: u8) -> u8 {
    match b {
        b'a'..=b'z' => (b - b'a' + 13) % 26 + b'a',
        b'A'..=b'Z' => (b - b'A' + 13) % 26 + b'A',
        _ => b,
    }
}

fn string_filters(
    _vm: &mut Vm,
    name: &[u8],
    _p: &Value,
    _s: FilterSide,
) -> R<Option<Box<dyn StreamFilter>>> {
    let f: fn(u8) -> u8 = match &name.to_ascii_lowercase()[..] {
        b"string.rot13" => rot13,
        b"string.toupper" => |b| b.to_ascii_uppercase(),
        b"string.tolower" => |b| b.to_ascii_lowercase(),
        _ => return Ok(None),
    };
    Ok(Some(Box::new(MapFilter(f))))
}

/// convert.base64-encode / -decode (RFC 4648), streaming; the encoder
/// takes `line-length` and `line-break-chars` (PHP manual: Conversion
/// Filters).
struct Base64 {
    encode: bool,
    carry: Vec<u8>,
    line_len: usize,
    line_break: Vec<u8>,
    col: usize,
}

fn wrap_push(out: &mut Vec<u8>, b: u8, line_len: usize, brk: &[u8], col: &mut usize) {
    if line_len > 0 && *col >= line_len {
        out.extend_from_slice(brk);
        *col = 0;
    }
    out.push(b);
    *col += 1;
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

impl StreamFilter for Base64 {
    fn filter(&mut self, vm: &mut Vm, input: &[u8], closing: bool, out: &mut Vec<u8>) -> R<bool> {
        self.carry.extend_from_slice(input);
        if self.encode {
            let whole = if closing {
                self.carry.len()
            } else {
                self.carry.len() / 3 * 3
            };
            for c in self.carry[..whole].chunks(3) {
                let n = (c[0] as u32) << 16
                    | (*c.get(1).unwrap_or(&0) as u32) << 8
                    | *c.get(2).unwrap_or(&0) as u32;
                let quad = [
                    B64[(n >> 18) as usize & 63],
                    B64[(n >> 12) as usize & 63],
                    if c.len() > 1 {
                        B64[(n >> 6) as usize & 63]
                    } else {
                        b'='
                    },
                    if c.len() > 2 {
                        B64[n as usize & 63]
                    } else {
                        b'='
                    },
                ];
                for q in quad {
                    wrap_push(out, q, self.line_len, &self.line_break, &mut self.col);
                }
            }
            self.carry.drain(..whole);
            return Ok(true);
        }
        // Decoding skips whitespace; other bytes are an error.
        let mut vals = Vec::new();
        for &b in &self.carry {
            match b {
                b'A'..=b'Z' => vals.push(b - b'A'),
                b'a'..=b'z' => vals.push(b - b'a' + 26),
                b'0'..=b'9' => vals.push(b - b'0' + 52),
                b'+' => vals.push(62),
                b'/' => vals.push(63),
                b'=' | b' ' | b'\t' | b'\r' | b'\n' => {}
                _ => {
                    let f = vm.active_function_name();
                    vm.warn(&format!(
                        "{}(): Stream filter (convert.base64-decode): invalid byte sequence",
                        f
                    ))?;
                    return Ok(false);
                }
            }
        }
        let whole = if closing {
            vals.len()
        } else {
            vals.len() / 4 * 4
        };
        for c in vals[..whole].chunks(4) {
            let mut n = 0u32;
            for (i, &v) in c.iter().enumerate() {
                n |= (v as u32) << (18 - 6 * i);
            }
            let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
            out.extend_from_slice(&bytes[..c.len().saturating_sub(1)]);
        }
        // Keep the undecoded tail (as base64 digits) for the next chunk.
        let rest: Vec<u8> = vals[whole..].iter().map(|&v| B64[v as usize]).collect();
        self.carry = rest;
        Ok(true)
    }
}

/// convert.quoted-printable-encode / -decode (RFC 2045 6.7), streaming.
struct QuotedPrintable {
    encode: bool,
    binary: bool,
    /// line-break-chars given: input line breaks stay line breaks
    keep_breaks: bool,
    line_len: usize,
    line_break: Vec<u8>,
    col: usize,
    carry: Vec<u8>,
}

impl StreamFilter for QuotedPrintable {
    fn filter(&mut self, vm: &mut Vm, input: &[u8], closing: bool, out: &mut Vec<u8>) -> R<bool> {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        if self.encode {
            // Line breaks of the input (line-break-chars) stay line breaks
            // and white space before them is encoded (tests
            // streams/bug64166*.phpt); a tail that may continue in the next
            // chunk is kept.
            self.carry.extend_from_slice(input);
            let input = std::mem::take(&mut self.carry);
            let lb = self.line_break.clone();
            let mut i = 0;
            while i < input.len() {
                let c = input[i];
                if self.keep_breaks && !self.binary && !lb.is_empty() {
                    let rest = &input[i..];
                    if rest.starts_with(&lb) {
                        out.extend_from_slice(&lb);
                        self.col = 0;
                        i += lb.len();
                        continue;
                    }
                    if !closing && lb.starts_with(rest) {
                        self.carry = rest.to_vec();
                        return Ok(true);
                    }
                }
                let mut trailing = false;
                if (c == b' ' || c == b'\t') && !self.binary && self.keep_breaks {
                    let mut j = i;
                    while j < input.len() && (input[j] == b' ' || input[j] == b'\t') {
                        j += 1;
                    }
                    if j == input.len() && !closing {
                        self.carry = input[i..].to_vec();
                        return Ok(true);
                    }
                    trailing = j == input.len() || input[j..].starts_with(&lb);
                }
                let plain = !trailing
                    && (((33..=126).contains(&c) && c != b'=') || (c == b' ' && !self.binary));
                let enc: Vec<u8> = if plain {
                    vec![c]
                } else {
                    vec![b'=', HEX[(c >> 4) as usize], HEX[(c & 15) as usize]]
                };
                if self.line_len > 0 && self.col + enc.len() > self.line_len.saturating_sub(1) {
                    out.push(b'=');
                    out.extend_from_slice(&self.line_break);
                    self.col = 0;
                }
                self.col += enc.len();
                out.extend_from_slice(&enc);
                i += 1;
            }
            return Ok(true);
        }
        self.carry.extend_from_slice(input);
        let d = std::mem::take(&mut self.carry);
        let mut i = 0;
        while i < d.len() {
            let c = d[i];
            if c != b'=' {
                out.push(c);
                i += 1;
                continue;
            }
            // Need two more bytes (or a soft line break).
            if i + 2 >= d.len() + usize::from(closing) && !closing && d.len() - i < 3 {
                self.carry = d[i..].to_vec();
                return Ok(true);
            }
            let a1 = d.get(i + 1).copied();
            let a2 = d.get(i + 2).copied();
            match (a1, a2) {
                (Some(b'\r'), Some(b'\n')) => i += 3,
                (Some(b'\n'), _) => i += 2,
                (Some(h), Some(l)) if h.is_ascii_hexdigit() && l.is_ascii_hexdigit() => {
                    let v = |x: u8| (x as char).to_digit(16).unwrap() as u8;
                    out.push(v(h) * 16 + v(l));
                    i += 3;
                }
                _ => {
                    vm.warn(&format!(
                        "{}(): Stream filter (convert.quoted-printable-decode): invalid byte sequence",
                        vm.active_function_name()
                    ))?;
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

fn opt(p: &Value, key: &str) -> Option<Value> {
    match p.deref() {
        Value::Array(a) => a.get_str(&PhpStr::from(key)).map(|v| v.deref()),
        _ => None,
    }
}

fn convert_filters(
    vm: &mut Vm,
    name: &[u8],
    p: &Value,
    _s: FilterSide,
) -> R<Option<Box<dyn StreamFilter>>> {
    let line_len = match opt(p, "line-length") {
        Some(v) => vm.to_int(&v)?.max(0) as usize,
        None => 0,
    };
    let keep_breaks = matches!(opt(p, "line-break-chars"), Some(Value::Str(_)));
    let line_break = match opt(p, "line-break-chars") {
        Some(Value::Str(s)) => s.to_vec(),
        _ => b"\r\n".to_vec(),
    };
    let binary = opt(p, "binary").is_some_and(|v| vm.to_bool(&v));
    Ok(match &name.to_ascii_lowercase()[..] {
        b"convert.base64-encode" => Some(Box::new(Base64 {
            encode: true,
            carry: Vec::new(),
            line_len,
            line_break,
            col: 0,
        })),
        b"convert.base64-decode" => Some(Box::new(Base64 {
            encode: false,
            carry: Vec::new(),
            line_len: 0,
            line_break: Vec::new(),
            col: 0,
        })),
        b"convert.quoted-printable-encode" => Some(Box::new(QuotedPrintable {
            encode: true,
            binary,
            keep_breaks,
            line_len,
            line_break,
            col: 0,
            carry: Vec::new(),
        })),
        b"convert.quoted-printable-decode" => Some(Box::new(QuotedPrintable {
            encode: false,
            binary,
            keep_breaks: false,
            line_len: 0,
            line_break: Vec::new(),
            col: 0,
            carry: Vec::new(),
        })),
        _ => None,
    })
}

fn init(vm: &mut Vm) {
    // stream_get_filters() order as observed on PHP 8.5 for these.
    register_filter(vm, "string.rot13", string_filters);
    register_filter(vm, "string.toupper", string_filters);
    register_filter(vm, "string.tolower", string_filters);
    register_filter(vm, "convert.*", convert_filters);
}

/// Closes a stream's native backing and flushes its write filters
/// (fclose()).
pub(crate) fn close_stream(vm: &mut Vm, s: &mut Stream) -> R<()> {
    if !s.write_filters.is_empty() {
        let mut chain = std::mem::take(&mut s.write_filters);
        let r = run_chain(vm, &mut chain, &[], true);
        if let Ok(Some(rest)) = &r
            && !rest.is_empty()
        {
            let _ = s.write_raw(vm, rest);
        }
        s.write_filters = chain;
        r?;
    }
    let mut all: Vec<AttachedFilter> = std::mem::take(&mut s.read_filters);
    all.extend(std::mem::take(&mut s.write_filters));
    for f in all.iter_mut() {
        f.filter.on_close(vm)?;
    }
    if let Backing::Native(n) = &mut s.backing {
        n.close(vm)?;
    }
    Ok(())
}

// ---------------------------------------------------------------- user filters

/// The class registered for `name` (exact, then `family.*` patterns).
fn user_filter_class(vm: &mut Vm, name: &[u8]) -> Option<PhpStr> {
    let reg = vm.ext::<StreamRegistry>();
    if let Some((_, c)) = reg
        .user_filters
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
    {
        return Some(c.clone());
    }
    // `a.b.c` also matches `a.b.*` and `a.*` (PHP manual:
    // stream_filter_register).
    let mut cut = name.len();
    while let Some(p) = name[..cut].iter().rposition(|&c| c == b'.') {
        let mut pat = name[..=p].to_vec();
        pat.push(b'*');
        if let Some((_, c)) = reg
            .user_filters
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(&pat))
        {
            return Some(c.clone());
        }
        cut = p;
    }
    None
}

fn stream_filter_register(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let name = a.str(0);
    let class = a.str(1);
    if name.is_empty() {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($filter_name) must be a non-empty string",
        );
    }
    if class.is_empty() {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($class) must be a non-empty string",
        );
    }
    let taken = find_factory(vm, &name).is_some()
        || vm
            .ext::<StreamRegistry>()
            .user_filters
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case(&name));
    if taken {
        return Ok(Value::Bool(false));
    }
    vm.ext::<StreamRegistry>()
        .user_filters
        .push((name.to_vec(), class));
    Ok(Value::Bool(true))
}

/// A php_user_filter instance on a stream.
struct UserFilter {
    obj: goro_runtime::ObjRef,
}

/// The data of a `userfilter.bucket brigade` resource.
#[derive(Default)]
struct Brigade(std::cell::RefCell<std::collections::VecDeque<goro_runtime::ObjRef>>);

fn new_user_filter(
    vm: &mut Vm,
    class: &PhpStr,
    name: &[u8],
    params: &Value,
    stream: Option<&ResRef>,
) -> R<Option<Box<dyn StreamFilter>>> {
    let Some(cls) = vm.find_class(class, true)? else {
        vm.warn(&format!(
            "stream_filter_append(): User-filter \"{}\" requires class \"{}\", but that class is not defined",
            String::from_utf8_lossy(name),
            class.to_string_lossy()
        ))?;
        return Ok(None);
    };
    let o = match vm.instantiate(&cls) {
        Ok(o) => o,
        Err(e) => {
            // The warnings come first, then the error (test
            // ext/standard/tests/filters/object_init_failure.phpt).
            let f = vm.active_function_name();
            let n = String::from_utf8_lossy(name).into_owned();
            vm.warn(&format!(
                "{}(): Unable to create or locate filter \"{}\"",
                f, n
            ))?;
            if stream.is_none() {
                vm.warn(&format!("{}(): Unable to create filter ({})", f, n))?;
            }
            return Err(e);
        }
    };
    // Only declared properties are filled (tests
    // ext/standard/tests/filters/gh20370*.phpt).
    let declared = |vm: &mut Vm, n: &[u8]| {
        let c = vm.class_of(&o);
        c.prop_map.borrow().contains_key(&PhpStr::new(n))
    };
    if declared(vm, b"filtername") {
        vm.set_prop_raw(&o, b"filtername", Value::Str(PhpStr::new(name)));
    }
    if declared(vm, b"params") {
        vm.set_prop_raw(&o, b"params", params.clone());
    }
    if declared(vm, b"stream") {
        vm.set_prop_raw(
            &o,
            b"stream",
            stream
                .map(|r| Value::Resource(r.clone()))
                .unwrap_or(Value::Null),
        );
    }
    if vm.class_of(&o).find_method(b"oncreate").is_some() {
        let ok = vm.call_method(&o, b"oncreate", Vec::new())?;
        if matches!(ok, Value::Bool(false)) {
            return Ok(None);
        }
    }
    Ok(Some(Box::new(UserFilter { obj: o })))
}

fn new_bucket(vm: &mut Vm, data: &[u8]) -> R<goro_runtime::ObjRef> {
    let cls = vm
        .find_class(b"StreamBucket", false)?
        .expect("StreamBucket class");
    let o = vm.instantiate_raw(&cls)?;
    vm.set_prop_raw(&o, b"data", Value::Str(PhpStr::new(data)));
    vm.set_prop_raw(&o, b"datalen", Value::Int(data.len() as i64));
    Ok(o)
}

impl StreamFilter for UserFilter {
    fn filter(&mut self, vm: &mut Vm, input: &[u8], closing: bool, out: &mut Vec<u8>) -> R<bool> {
        let inb = Brigade::default();
        if !input.is_empty() {
            let b = new_bucket(vm, input)?;
            inb.0.borrow_mut().push_back(b);
        }
        let in_res = ResRef::new(PhpStr::intern(b"userfilter.bucket brigade"), Box::new(inb));
        let out_res = ResRef::new(
            PhpStr::intern(b"userfilter.bucket brigade"),
            Box::new(Brigade::default()),
        );
        let consumed = goro_runtime::PhpRef::new(Value::Int(0));
        let r = vm.call_method(
            &self.obj,
            b"filter",
            vec![
                Value::Resource(in_res.clone()),
                Value::Resource(out_res.clone()),
                Value::Ref(consumed),
                Value::Bool(closing),
            ],
        )?;
        let code = match r {
            Value::Int(i) => i,
            _ => 0,
        };
        let left = in_res
            .res()
            .data
            .borrow()
            .as_ref()
            .and_then(|d| d.downcast_ref::<Brigade>())
            .is_some_and(|b| !b.0.borrow().is_empty());
        if left {
            let f = vm.active_function_name();
            vm.warn(&format!(
                "{}(): Unprocessed filter buckets remaining on input brigade",
                f
            ))?;
        }
        let data = out_res.res().data.borrow();
        if let Some(b) = data.as_ref().and_then(|d| d.downcast_ref::<Brigade>()) {
            for bucket in b.0.borrow().iter() {
                if let Value::Str(s) = vm.get_prop_raw(bucket, b"data") {
                    out.extend_from_slice(&s);
                }
            }
        }
        drop(data);
        if code == 0 {
            return Ok(false);
        }
        let _ = self.has(vm, b"filter");
        if code == 1 {
            // PSFS_FEED_ME: nothing passed on yet.
            out.clear();
        }
        Ok(true)
    }

    fn on_close(&mut self, vm: &mut Vm) -> R<()> {
        if self.has(vm, b"onclose") {
            vm.call_method(&self.obj, b"onclose", Vec::new())?;
        }
        Ok(())
    }
}

impl UserFilter {
    fn has(&self, vm: &Vm, m: &[u8]) -> bool {
        vm.class_of(&self.obj).find_method(m).is_some()
    }
}

fn brigade_of(v: &Value) -> Option<&ResRef> {
    match v {
        Value::Resource(r) => Some(r),
        _ => None,
    }
}

fn stream_bucket_make_writeable(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0).deref();
    let Some(r) = brigade_of(&v) else {
        return vm.fthrow(
            a,
            "TypeError",
            "Argument #1 ($brigade) must be of type resource",
        );
    };
    let data = r.res().data.borrow();
    let Some(b) = data.as_ref().and_then(|d| d.downcast_ref::<Brigade>()) else {
        drop(data);
        return vm.fthrow(
            a,
            "TypeError",
            "stream_bucket_make_writeable(): supplied resource is not a valid userfilter.bucket brigade resource",
        );
    };
    let next = b.0.borrow_mut().pop_front();
    drop(data);
    Ok(next.map(Value::Object).unwrap_or(Value::Null))
}

fn bucket_add(vm: &mut Vm, a: &Args, front: bool) -> R<Value> {
    let v = a.val(0).deref();
    let Some(r) = brigade_of(&v) else {
        return vm.fthrow(
            a,
            "TypeError",
            "Argument #1 ($brigade) must be of type resource",
        );
    };
    let Some(bucket) = a.obj(1) else {
        return Ok(Value::Null);
    };
    // The bucket's data property is what passes on; keep datalen in step.
    if let Value::Str(s) = vm.get_prop_raw(&bucket, b"data") {
        vm.set_prop_raw(&bucket, b"datalen", Value::Int(s.len() as i64));
    }
    let data = r.res().data.borrow();
    if let Some(b) = data.as_ref().and_then(|d| d.downcast_ref::<Brigade>()) {
        let mut q = b.0.borrow_mut();
        q.retain(|x| !goro_runtime::ObjRef::ptr_eq(x, &bucket));
        if front {
            q.push_front(bucket);
        } else {
            q.push_back(bucket);
        }
    }
    Ok(Value::Null)
}

fn stream_bucket_append(vm: &mut Vm, a: &mut Args) -> R<Value> {
    bucket_add(vm, a, false)
}

fn stream_bucket_prepend(vm: &mut Vm, a: &mut Args) -> R<Value> {
    bucket_add(vm, a, true)
}

fn stream_bucket_new(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.str(1);
    Ok(Value::Object(new_bucket(vm, &data)?))
}

/// A filter by name (user filters first, then the registered ones).
fn make_filter(vm: &mut Vm, name: &[u8], side: FilterSide) -> R<Option<Box<dyn StreamFilter>>> {
    if let Some(cls) = user_filter_class(vm, name) {
        return new_user_filter(vm, &cls, name, &Value::Null, None);
    }
    match find_factory(vm, name) {
        Some(f) => f(vm, name, &Value::Null, side),
        None => Ok(None),
    }
}

/// `php://filter/read=a|b/write=c/resource=URL` (PHP manual: php://
/// wrappers): the resource opened with the filters attached. Messages as
/// observed on PHP 8.5.
pub(crate) fn open_php_filter(
    vm: &mut Vm,
    a: &Args,
    spec: &[u8],
    mode: &[u8],
    quiet: bool,
) -> R<Option<Stream>> {
    let at = spec
        .windows(10)
        .position(|w| w.eq_ignore_ascii_case(b"/resource="))
        .map(|p| (p, p + 10))
        .or_else(|| {
            spec.get(..9)
                .filter(|w| w.eq_ignore_ascii_case(b"resource="))
                .map(|_| (0, 9))
        });
    let Some((end, rstart)) = at else {
        return vm.throw("Error", "No URL resource specified");
    };
    let resource = &spec[rstart..];
    // With nothing before `resource=`, PHP reads the resource part as a
    // filter list too.
    let filters = if end == 0 { spec } else { &spec[..end] };
    let Some(mut s) = super::file::open_stream(vm, a, resource, mode, quiet)? else {
        return Ok(None);
    };
    for part in filters.split(|&c| c == b'/') {
        if part.is_empty() {
            continue;
        }
        let (sides, list): (&[FilterSide], &[u8]) =
            if part.len() >= 5 && part[..5].eq_ignore_ascii_case(b"read=") {
                (&[FilterSide::Read], &part[5..])
            } else if part.len() >= 6 && part[..6].eq_ignore_ascii_case(b"write=") {
                (&[FilterSide::Write], &part[6..])
            } else {
                (&[FilterSide::Read, FilterSide::Write], part)
            };
        for name in list.split(|&c| c == b'|') {
            if name.is_empty() {
                continue;
            }
            let name = urldecode(name);
            for &side in sides {
                if side == FilterSide::Read && !s.readable
                    || side == FilterSide::Write && !s.writable
                {
                    continue;
                }
                match make_filter(vm, &name, side)? {
                    Some(f) => {
                        let id = {
                            let n = &mut vm.ext::<FilterIds>().0;
                            *n += 1;
                            *n
                        };
                        let af = AttachedFilter {
                            id,
                            side,
                            filter: f,
                        };
                        match side {
                            FilterSide::Read => s.read_filters.push(af),
                            FilterSide::Write => s.write_filters.push(af),
                        }
                    }
                    None => {
                        let shown = String::from_utf8_lossy(&name).into_owned();
                        let f = vm.active_function_name();
                        if user_filter_class(vm, &name).is_some()
                            || find_factory(vm, &name).is_some()
                        {
                            vm.warn(&format!(
                                "{}(): Unable to create or locate filter \"{}\"",
                                f, shown
                            ))?;
                        } else {
                            vm.warn(&format!("{}(): Unable to locate filter \"{}\"", f, shown))?;
                        }
                        vm.warn(&format!("{}(): Unable to create filter ({})", f, shown))?;
                    }
                }
            }
        }
    }
    Ok(Some(s))
}

/// `%xx` decoding of a filter name.
fn urldecode(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && b[i + 1].is_ascii_hexdigit()
            && b[i + 2].is_ascii_hexdigit()
        {
            let h = |c: u8| (c as char).to_digit(16).unwrap() as u8;
            out.push(h(b[i + 1]) * 16 + h(b[i + 2]));
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}
