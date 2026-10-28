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
    ],
    constants: &[
        ("STREAM_FILTER_READ", ConstVal::Int(1)),
        ("STREAM_FILTER_WRITE", ConstVal::Int(2)),
        ("STREAM_FILTER_ALL", ConstVal::Int(3)),
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
    let reg = vm.ext::<StreamRegistry>();
    for (n, f) in &reg.filters {
        if let Some(prefix) = n.strip_suffix('*') {
            if name.len() > prefix.len()
                && name[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
            {
                return Some(*f);
            }
        } else if name.eq_ignore_ascii_case(n.as_bytes()) {
            return Some(*f);
        }
    }
    None
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
    let Some(factory) = find_factory(vm, &name) else {
        vm.fwarn(
            a,
            &format!("Unable to locate filter \"{}\"", name.to_string_lossy()),
        )?;
        return Ok(Value::Bool(false));
    };
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
        let Some(f) = factory(vm, &name, &params, side)? else {
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
    let Some((stream, id)) = found else {
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
        s.read_filters.retain(|f| f.id != id);
        s.write_filters.retain(|f| f.id != id);
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

/// convert.base64-encode / -decode (RFC 4648), streaming.
struct Base64 {
    encode: bool,
    carry: Vec<u8>,
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
                out.push(B64[(n >> 18) as usize & 63]);
                out.push(B64[(n >> 12) as usize & 63]);
                out.push(if c.len() > 1 {
                    B64[(n >> 6) as usize & 63]
                } else {
                    b'='
                });
                out.push(if c.len() > 2 {
                    B64[n as usize & 63]
                } else {
                    b'='
                });
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
                    vm.warn("stream filter (convert.base64-decode): invalid byte sequence")?;
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

fn convert_filters(
    _vm: &mut Vm,
    name: &[u8],
    _p: &Value,
    _s: FilterSide,
) -> R<Option<Box<dyn StreamFilter>>> {
    Ok(match &name.to_ascii_lowercase()[..] {
        b"convert.base64-encode" => Some(Box::new(Base64 {
            encode: true,
            carry: Vec::new(),
        })),
        b"convert.base64-decode" => Some(Box::new(Base64 {
            encode: false,
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
        r?;
    }
    if let Backing::Native(n) = &mut s.backing {
        n.close(vm)?;
    }
    Ok(())
}
