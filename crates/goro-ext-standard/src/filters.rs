//! PHP stream filter infrastructure.
//!
//! Implements `stream_filter_register`, `stream_filter_append`,
//! `stream_filter_prepend`, `stream_filter_remove`, `stream_get_filters`
//! and the built-in filters (zlib.*, bzip2.*, string.*, convert.*, dechunk,
//! consumed).
//!
//! Filter state is kept in thread-local maps keyed by either:
//!   * a stream resource id (the chain attached to that stream)
//!   * a filter resource id (used by `stream_filter_remove`)
//!
//! The fread / fwrite / fgets / fclose implementations in misc.rs call the
//! helpers exposed here (`apply_read_chain`, `apply_write_chain`,
//! `flush_close`) to run the chain.

use goro_core::array::ArrayKey;
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub const STREAM_FILTER_READ: i64 = 1;
pub const STREAM_FILTER_WRITE: i64 = 2;
pub const STREAM_FILTER_ALL: i64 = STREAM_FILTER_READ | STREAM_FILTER_WRITE;

pub const PSFS_PASS_ON: i64 = 2;
pub const PSFS_FEED_ME: i64 = 1;
pub const PSFS_ERR_FATAL: i64 = 0;

/// Built-in filter kinds. The user filter variant carries a PhpObject
/// instance constructed from the registered class.
#[derive(Clone)]
pub enum BuiltinKind {
    /// zlib.deflate (raw deflate, no zlib header)
    ZlibDeflate { level: u32 },
    /// zlib.inflate (raw deflate decode)
    ZlibInflate,
    /// bzip2.compress
    Bzip2Compress { level: u32 },
    /// bzip2.decompress
    Bzip2Decompress,
    /// string.toupper
    StringToUpper,
    /// string.tolower
    StringToLower,
    /// string.rot13
    StringRot13,
    /// string.strip_tags (allowed tags)
    StringStripTags { allowed: Vec<u8> },
    /// convert.base64-encode
    ConvertBase64Encode { line_length: usize, line_break: Vec<u8> },
    /// convert.base64-decode
    ConvertBase64Decode,
    /// convert.quoted-printable-encode
    ConvertQpEncode { line_length: usize, line_break: Vec<u8>, binary: bool },
    /// convert.quoted-printable-decode
    ConvertQpDecode,
    /// dechunk - HTTP chunked transfer decoding
    Dechunk,
    /// consumed - passthrough, no-op
    Consumed,
}

/// Live filter instance attached to a stream.
pub struct FilterInstance {
    /// Filter name (e.g. "zlib.deflate", "myfilter").
    pub name: Vec<u8>,
    /// Resource id of this filter (returned to PHP from
    /// stream_filter_append/prepend, looked up by stream_filter_remove).
    pub fid: i64,
    /// Stream resource id this filter is attached to.
    pub stream_id: i64,
    /// Direction flags: read, write, or both.
    pub direction: i64,
    /// Built-in implementation, if any.
    pub builtin: Option<BuiltinKind>,
    /// User filter implementation: object instance whose class extends
    /// php_user_filter. None for built-in filters.
    pub user_obj: Option<Rc<RefCell<PhpObject>>>,
    /// Persistent state: bytes accumulated for decoders that need a
    /// look-ahead (e.g. base64 partial blocks, qp partial =hex, dechunk
    /// partial chunk header / chunk body).
    pub leftover: Vec<u8>,
    /// Compressors that maintain a streaming context (zlib / bzip2).
    pub deflate_ctx: Option<DeflateCtx>,
    pub inflate_ctx: Option<InflateCtx>,
    pub bz_compress: Option<BzCompressCtx>,
    pub bz_decompress: Option<BzDecompressCtx>,
    /// For dechunk: state tracking the current chunk size remaining.
    pub dechunk_state: DechunkState,
    /// For base64 / qp encoders: current column position so line wrapping
    /// continues correctly across fwrite calls.
    pub col: usize,
    /// True once the filter has been driven with `closing=true` exactly
    /// once. Used by flush_chain to avoid re-flushing.
    pub closed: bool,
}

pub struct DeflateCtx {
    pub enc: flate2::Compress,
}

pub struct InflateCtx {
    pub dec: flate2::Decompress,
}

pub struct BzCompressCtx {
    pub enc: bzip2::Compress,
}

pub struct BzDecompressCtx {
    pub dec: bzip2::Decompress,
}

#[derive(Clone, Copy, Default)]
pub enum DechunkState {
    #[default]
    Header,
    Body(usize),
    AfterChunk,
    Done,
}

thread_local! {
    /// Map from filter resource id -> Rc<RefCell<FilterInstance>>.
    static FILTERS: RefCell<HashMap<i64, Rc<RefCell<FilterInstance>>>> = RefCell::new(HashMap::new());
    /// Map from stream resource id -> ordered list of attached filters.
    static STREAM_CHAINS: RefCell<HashMap<i64, Vec<Rc<RefCell<FilterInstance>>>>> = RefCell::new(HashMap::new());
    /// Registered user filter classes: filter name -> class name.
    static USER_FILTER_CLASSES: RefCell<HashMap<Vec<u8>, Vec<u8>>> = RefCell::new(HashMap::new());
    /// Counter for filter resource ids. Starts high so it doesn't clash
    /// with file/dir/curl handles allocated elsewhere.
    static NEXT_FILTER_ID: std::cell::Cell<i64> = const { std::cell::Cell::new(900_000) };
}

fn alloc_filter_id() -> i64 {
    NEXT_FILTER_ID.with(|c| {
        let n = c.get();
        c.set(n + 1);
        n
    })
}

/// Return list of all built-in + registered filter names.
pub fn list_filter_names() -> Vec<Vec<u8>> {
    let mut names: Vec<Vec<u8>> = vec![
        b"zlib.deflate".to_vec(),
        b"zlib.inflate".to_vec(),
        b"zlib.compress".to_vec(),
        b"zlib.uncompress".to_vec(),
        b"bzip2.compress".to_vec(),
        b"bzip2.decompress".to_vec(),
        b"string.toupper".to_vec(),
        b"string.tolower".to_vec(),
        b"string.rot13".to_vec(),
        b"string.strip_tags".to_vec(),
        b"convert.base64-encode".to_vec(),
        b"convert.base64-decode".to_vec(),
        b"convert.quoted-printable-encode".to_vec(),
        b"convert.quoted-printable-decode".to_vec(),
        b"convert.iconv.*".to_vec(),
        b"dechunk".to_vec(),
        b"consumed".to_vec(),
    ];
    USER_FILTER_CLASSES.with(|m| {
        for k in m.borrow().keys() {
            names.push(k.clone());
        }
    });
    names
}

/// Implements `stream_filter_register($filter_name, $class)`. Returns true
/// if the registration succeeded, false if the name was already taken.
pub fn register_user_filter(name: &[u8], class: &[u8]) -> bool {
    USER_FILTER_CLASSES.with(|m| {
        let mut m = m.borrow_mut();
        if m.contains_key(name) {
            return false;
        }
        m.insert(name.to_vec(), class.to_vec());
        true
    })
}

/// True iff the filter name matches a registered user filter class.
pub fn is_user_filter(name: &[u8]) -> bool {
    USER_FILTER_CLASSES.with(|m| m.borrow().contains_key(name))
}

/// Return the registered user filter class name for the given filter name,
/// if any.
pub fn user_filter_class(name: &[u8]) -> Option<Vec<u8>> {
    USER_FILTER_CLASSES.with(|m| m.borrow().get(name).cloned())
}

/// Parse the `params` argument of stream_filter_append/prepend for zlib's
/// `level` option. Returns the requested level clamped to 0..=9 (-1 ==
/// default).
fn parse_zlib_level(params: &Value) -> u32 {
    let level = lookup_array_int(params, b"level", -1);
    if level == -1 {
        return flate2::Compression::default().level();
    }
    level.clamp(0, 9) as u32
}

fn parse_bzip2_level(params: &Value) -> u32 {
    let level = lookup_array_int(params, b"blocks", 4);
    level.clamp(1, 9) as u32
}

fn lookup_array_int(v: &Value, key: &[u8], default: i64) -> i64 {
    match v {
        Value::Array(a) => {
            let a = a.borrow();
            a.get(&ArrayKey::String(PhpString::from_bytes(key))).map(|v| v.to_long()).unwrap_or(default)
        }
        Value::Reference(r) => lookup_array_int(&r.borrow(), key, default),
        Value::Object(obj) => {
            let v = obj.borrow().get_property(key);
            if matches!(v, Value::Null | Value::Undef) { default } else { v.to_long() }
        }
        _ => default,
    }
}

fn lookup_array_str(v: &Value, key: &[u8]) -> Option<Vec<u8>> {
    match v {
        Value::Array(a) => {
            let a = a.borrow();
            a.get(&ArrayKey::String(PhpString::from_bytes(key)))
                .map(|v| v.to_php_string().as_bytes().to_vec())
        }
        Value::Reference(r) => lookup_array_str(&r.borrow(), key),
        Value::Object(obj) => {
            let v = obj.borrow().get_property(key);
            if matches!(v, Value::Null | Value::Undef) { None } else { Some(v.to_php_string().as_bytes().to_vec()) }
        }
        _ => None,
    }
}

fn lookup_array_bool(v: &Value, key: &[u8], default: bool) -> bool {
    match v {
        Value::Array(a) => {
            let a = a.borrow();
            a.get(&ArrayKey::String(PhpString::from_bytes(key))).map(|v| v.to_bool()).unwrap_or(default)
        }
        Value::Reference(r) => lookup_array_bool(&r.borrow(), key, default),
        Value::Object(obj) => {
            let v = obj.borrow().get_property(key);
            if matches!(v, Value::Null | Value::Undef) { default } else { v.to_bool() }
        }
        _ => default,
    }
}

/// Resolve a filter name to a built-in implementation, if it matches one
/// of the registered built-ins.
pub fn builtin_for_name(name: &[u8], params: &Value) -> Option<BuiltinKind> {
    let lname = name.to_ascii_lowercase();
    let s: &[u8] = &lname;
    match s {
        b"zlib.deflate" | b"zlib.compress" => Some(BuiltinKind::ZlibDeflate {
            level: parse_zlib_level(params),
        }),
        b"zlib.inflate" | b"zlib.uncompress" => Some(BuiltinKind::ZlibInflate),
        b"bzip2.compress" => Some(BuiltinKind::Bzip2Compress {
            level: parse_bzip2_level(params),
        }),
        b"bzip2.decompress" => Some(BuiltinKind::Bzip2Decompress),
        b"string.toupper" => Some(BuiltinKind::StringToUpper),
        b"string.tolower" => Some(BuiltinKind::StringToLower),
        b"string.rot13" => Some(BuiltinKind::StringRot13),
        b"string.strip_tags" => Some(BuiltinKind::StringStripTags {
            allowed: lookup_array_str(params, b"allowed_tags").unwrap_or_default(),
        }),
        b"convert.base64-encode" => Some(BuiltinKind::ConvertBase64Encode {
            line_length: lookup_array_int(params, b"line-length", 0).max(0) as usize,
            line_break: lookup_array_str(params, b"line-break-chars").unwrap_or_else(|| b"\r\n".to_vec()),
        }),
        b"convert.base64-decode" => Some(BuiltinKind::ConvertBase64Decode),
        b"convert.quoted-printable-encode" => Some(BuiltinKind::ConvertQpEncode {
            line_length: lookup_array_int(params, b"line-length", 76).max(0) as usize,
            line_break: lookup_array_str(params, b"line-break-chars").unwrap_or_else(|| b"\r\n".to_vec()),
            binary: lookup_array_bool(params, b"binary", false),
        }),
        b"convert.quoted-printable-decode" => Some(BuiltinKind::ConvertQpDecode),
        b"dechunk" => Some(BuiltinKind::Dechunk),
        b"consumed" => Some(BuiltinKind::Consumed),
        _ => None,
    }
}

/// Create and attach a filter to a stream. `prepend = true` puts it at the
/// front of the chain. Returns the filter resource id on success, or None
/// if the filter could not be resolved.
pub fn attach_filter(
    stream_id: i64,
    name: &[u8],
    direction: i64,
    params: &Value,
    prepend: bool,
    user_obj: Option<Rc<RefCell<PhpObject>>>,
) -> Option<i64> {
    let builtin = if user_obj.is_some() {
        None
    } else {
        Some(builtin_for_name(name, params)?)
    };
    let fid = alloc_filter_id();
    let inst = FilterInstance {
        name: name.to_vec(),
        fid,
        stream_id,
        direction,
        builtin,
        user_obj,
        leftover: Vec::new(),
        deflate_ctx: None,
        inflate_ctx: None,
        bz_compress: None,
        bz_decompress: None,
        dechunk_state: DechunkState::Header,
        col: 0,
        closed: false,
    };
    let rc = Rc::new(RefCell::new(inst));
    FILTERS.with(|m| { m.borrow_mut().insert(fid, rc.clone()); });
    STREAM_CHAINS.with(|m| {
        let mut m = m.borrow_mut();
        let chain = m.entry(stream_id).or_default();
        if prepend {
            chain.insert(0, rc);
        } else {
            chain.push(rc);
        }
    });
    goro_core::value::register_resource(fid, b"stream filter");
    Some(fid)
}

/// Remove a filter from its stream. Returns true if the filter id was
/// known.
pub fn remove_filter(fid: i64) -> bool {
    let inst = FILTERS.with(|m| m.borrow_mut().remove(&fid));
    if let Some(inst) = inst {
        let stream_id = inst.borrow().stream_id;
        STREAM_CHAINS.with(|m| {
            let mut m = m.borrow_mut();
            if let Some(chain) = m.get_mut(&stream_id) {
                chain.retain(|f| f.borrow().fid != fid);
            }
        });
        goro_core::value::close_resource(fid);
        true
    } else {
        false
    }
}

/// Remove every filter attached to the given stream id. Called on fclose.
pub fn remove_stream_filters(stream_id: i64) {
    let chain = STREAM_CHAINS.with(|m| m.borrow_mut().remove(&stream_id));
    if let Some(chain) = chain {
        FILTERS.with(|m| {
            let mut m = m.borrow_mut();
            for f in &chain {
                let fid = f.borrow().fid;
                m.remove(&fid);
                goro_core::value::close_resource(fid);
            }
        });
    }
}

/// True iff the resource id is a registered stream filter (open or
/// closed).
pub fn is_filter_resource(fid: i64) -> bool {
    FILTERS.with(|m| m.borrow().contains_key(&fid))
}

/// True iff at least one filter on `stream_id` matches `direction`.
pub fn chain_has_direction(stream_id: i64, direction: i64) -> bool {
    STREAM_CHAINS.with(|m| {
        m.borrow().get(&stream_id).is_some_and(|chain| {
            chain.iter().any(|f| f.borrow().direction & direction != 0)
        })
    })
}

/// True iff the stream has any filters attached (used by stream_get_contents
/// to decide whether to route through the filter chain).
pub fn chain_has_any(stream_id: i64) -> bool {
    STREAM_CHAINS.with(|m| m.borrow().get(&stream_id).is_some_and(|c| !c.is_empty()))
}

/// Apply the write filter chain to `data` for the given stream. Returns
/// the filtered output bytes ready to be physically written.
pub fn apply_write_chain(vm: &mut Vm, stream_id: i64, data: &[u8]) -> Vec<u8> {
    let chain = STREAM_CHAINS.with(|m| m.borrow().get(&stream_id).cloned()).unwrap_or_default();
    let mut buf = data.to_vec();
    for f in &chain {
        let dir = f.borrow().direction;
        if dir & STREAM_FILTER_WRITE == 0 {
            continue;
        }
        buf = run_filter(vm, f, &buf, false);
    }
    buf
}

/// Apply the read filter chain to `data`.
pub fn apply_read_chain(vm: &mut Vm, stream_id: i64, data: &[u8]) -> Vec<u8> {
    let chain = STREAM_CHAINS.with(|m| m.borrow().get(&stream_id).cloned()).unwrap_or_default();
    let mut buf = data.to_vec();
    for f in &chain {
        let dir = f.borrow().direction;
        if dir & STREAM_FILTER_READ == 0 {
            continue;
        }
        buf = run_filter(vm, f, &buf, false);
    }
    buf
}

/// Flush filters at close. Each filter is called with closing=true and any
/// pending output is returned. Direction (READ/WRITE) selects which side
/// of the chain to flush.
pub fn flush_chain(vm: &mut Vm, stream_id: i64, direction: i64) -> Vec<u8> {
    let chain = STREAM_CHAINS.with(|m| m.borrow().get(&stream_id).cloned()).unwrap_or_default();
    let mut buf = Vec::new();
    for f in &chain {
        let dir = f.borrow().direction;
        if dir & direction == 0 { continue; }
        // Push prior output through, then emit closing exactly once per
        // filter (subsequent calls would re-invoke user `onClose`-like
        // logic, leading to infinite loops in read mode).
        if !buf.is_empty() {
            buf = run_filter(vm, f, &buf, false);
        }
        let already_closed = f.borrow().closed;
        if already_closed { continue; }
        let mut close_out = run_filter(vm, f, &[], true);
        f.borrow_mut().closed = true;
        buf.append(&mut close_out);
    }
    buf
}

/// Drive a single filter instance over `input`. If `closing` is true,
/// compressors/decoders should finalize.
fn run_filter(vm: &mut Vm, f: &Rc<RefCell<FilterInstance>>, input: &[u8], closing: bool) -> Vec<u8> {
    // Built-in: dispatch on kind.
    let kind_opt = f.borrow().builtin.clone();
    if let Some(kind) = kind_opt {
        return run_builtin(f, kind, input, closing);
    }
    // User filter: invoke filter() method.
    run_user_filter(vm, f, input, closing)
}

fn run_builtin(f: &Rc<RefCell<FilterInstance>>, kind: BuiltinKind, input: &[u8], closing: bool) -> Vec<u8> {
    match kind {
        BuiltinKind::ZlibDeflate { level } => {
            let mut fi = f.borrow_mut();
            if fi.deflate_ctx.is_none() {
                fi.deflate_ctx = Some(DeflateCtx {
                    enc: flate2::Compress::new(flate2::Compression::new(level), false),
                });
            }
            let ctx = fi.deflate_ctx.as_mut().unwrap();
            let mut out = Vec::with_capacity(input.len() + 64);
            let mut input_pos = 0usize;
            let mut buf = [0u8; 4096];
            // PHP uses Z_NO_FLUSH during writes (only Z_FINISH on close).
            // This matches the byte-for-byte output of the C implementation
            // for tests that depend on it.
            loop {
                let in_before = ctx.enc.total_in();
                let out_before = ctx.enc.total_out();
                let flush = if closing { flate2::FlushCompress::Finish } else { flate2::FlushCompress::None };
                let _ = ctx.enc.compress(&input[input_pos..], &mut buf, flush);
                let consumed_in = (ctx.enc.total_in() - in_before) as usize;
                let produced_out = (ctx.enc.total_out() - out_before) as usize;
                if produced_out > 0 {
                    out.extend_from_slice(&buf[..produced_out]);
                }
                input_pos += consumed_in;
                if input_pos >= input.len() && produced_out < buf.len() {
                    break;
                }
                if consumed_in == 0 && produced_out == 0 {
                    break;
                }
            }
            out
        }
        BuiltinKind::ZlibInflate => {
            let mut fi = f.borrow_mut();
            if fi.inflate_ctx.is_none() {
                fi.inflate_ctx = Some(InflateCtx {
                    dec: flate2::Decompress::new(false),
                });
            }
            let ctx = fi.inflate_ctx.as_mut().unwrap();
            let mut out = Vec::with_capacity(input.len() * 2 + 64);
            let mut input_pos = 0usize;
            let mut buf = [0u8; 4096];
            let flush = if closing { flate2::FlushDecompress::Finish } else { flate2::FlushDecompress::Sync };
            loop {
                let in_before = ctx.dec.total_in();
                let out_before = ctx.dec.total_out();
                let res = ctx.dec.decompress(&input[input_pos..], &mut buf, flush);
                let consumed_in = (ctx.dec.total_in() - in_before) as usize;
                let produced_out = (ctx.dec.total_out() - out_before) as usize;
                if produced_out > 0 {
                    out.extend_from_slice(&buf[..produced_out]);
                }
                input_pos += consumed_in;
                if let Ok(flate2::Status::StreamEnd) = res {
                    break;
                }
                if input_pos >= input.len() && produced_out < buf.len() {
                    break;
                }
                if consumed_in == 0 && produced_out == 0 {
                    break;
                }
            }
            out
        }
        BuiltinKind::Bzip2Compress { level } => {
            let mut fi = f.borrow_mut();
            if fi.bz_compress.is_none() {
                fi.bz_compress = Some(BzCompressCtx {
                    enc: bzip2::Compress::new(bzip2::Compression::new(level), 0),
                });
            }
            let ctx = fi.bz_compress.as_mut().unwrap();
            let mut out = Vec::with_capacity(input.len() + 128);
            let mut input_pos = 0usize;
            let mut buf = [0u8; 4096];
            let action = if closing { bzip2::Action::Finish } else { bzip2::Action::Run };
            loop {
                let in_before = ctx.enc.total_in();
                let out_before = ctx.enc.total_out();
                let res = ctx.enc.compress(&input[input_pos..], &mut buf, action);
                let consumed_in = (ctx.enc.total_in() - in_before) as usize;
                let produced_out = (ctx.enc.total_out() - out_before) as usize;
                if produced_out > 0 {
                    out.extend_from_slice(&buf[..produced_out]);
                }
                input_pos += consumed_in;
                if let Ok(bzip2::Status::StreamEnd) = res {
                    break;
                }
                if input_pos >= input.len() && produced_out < buf.len() {
                    break;
                }
                if consumed_in == 0 && produced_out == 0 {
                    break;
                }
            }
            out
        }
        BuiltinKind::Bzip2Decompress => {
            let mut fi = f.borrow_mut();
            if fi.bz_decompress.is_none() {
                fi.bz_decompress = Some(BzDecompressCtx {
                    dec: bzip2::Decompress::new(false),
                });
            }
            let ctx = fi.bz_decompress.as_mut().unwrap();
            let mut out = Vec::with_capacity(input.len() * 2 + 128);
            let mut input_pos = 0usize;
            let mut buf = [0u8; 4096];
            loop {
                let in_before = ctx.dec.total_in();
                let out_before = ctx.dec.total_out();
                let res = ctx.dec.decompress(&input[input_pos..], &mut buf);
                let consumed_in = (ctx.dec.total_in() - in_before) as usize;
                let produced_out = (ctx.dec.total_out() - out_before) as usize;
                if produced_out > 0 {
                    out.extend_from_slice(&buf[..produced_out]);
                }
                input_pos += consumed_in;
                if let Ok(bzip2::Status::StreamEnd) = res {
                    break;
                }
                if input_pos >= input.len() && produced_out < buf.len() {
                    break;
                }
                if consumed_in == 0 && produced_out == 0 {
                    break;
                }
            }
            out
        }
        BuiltinKind::StringToUpper => {
            let mut out = Vec::with_capacity(input.len());
            for &b in input { out.push(b.to_ascii_uppercase()); }
            out
        }
        BuiltinKind::StringToLower => {
            let mut out = Vec::with_capacity(input.len());
            for &b in input { out.push(b.to_ascii_lowercase()); }
            out
        }
        BuiltinKind::StringRot13 => {
            let mut out = Vec::with_capacity(input.len());
            for &b in input {
                out.push(match b {
                    b'A'..=b'M' | b'a'..=b'm' => b + 13,
                    b'N'..=b'Z' | b'n'..=b'z' => b - 13,
                    _ => b,
                });
            }
            out
        }
        BuiltinKind::StringStripTags { allowed: _ } => {
            // Simple naive HTML tag stripping. We process leftover state
            // so partial tags across writes work.
            let mut fi = f.borrow_mut();
            let mut data = std::mem::take(&mut fi.leftover);
            data.extend_from_slice(input);
            let mut out = Vec::with_capacity(data.len());
            let mut in_tag = false;
            let mut i = 0;
            while i < data.len() {
                let b = data[i];
                if !in_tag {
                    if b == b'<' { in_tag = true; }
                    else { out.push(b); }
                } else if b == b'>' {
                    in_tag = false;
                }
                i += 1;
            }
            if in_tag && !closing {
                // Stash incomplete tag to consume next call.
                fi.leftover = b"<".to_vec();
            }
            out
        }
        BuiltinKind::ConvertBase64Encode { line_length, line_break } => {
            base64_encode_streaming(f, input, closing, line_length, &line_break)
        }
        BuiltinKind::ConvertBase64Decode => {
            base64_decode_streaming(f, input, closing)
        }
        BuiltinKind::ConvertQpEncode { line_length, line_break, binary } => {
            qp_encode_streaming(f, input, closing, line_length, &line_break, binary)
        }
        BuiltinKind::ConvertQpDecode => {
            qp_decode_streaming(f, input, closing)
        }
        BuiltinKind::Dechunk => {
            dechunk_streaming(f, input, closing)
        }
        BuiltinKind::Consumed => input.to_vec(),
    }
}

fn base64_encode_streaming(f: &Rc<RefCell<FilterInstance>>, input: &[u8], closing: bool, line_length: usize, line_break: &[u8]) -> Vec<u8> {
    let mut fi = f.borrow_mut();
    // Accumulate full triplets; carry over any partial.
    let mut data = std::mem::take(&mut fi.leftover);
    data.extend_from_slice(input);
    let mut out = Vec::with_capacity(data.len() * 4 / 3 + 16);
    let take_len = if closing { data.len() } else { (data.len() / 3) * 3 };
    let to_encode = &data[..take_len];
    let encoded = base64_encode_no_wrap(to_encode, closing);
    // Apply line wrapping if requested.
    if line_length > 0 {
        for &b in encoded.as_bytes() {
            if fi.col == line_length {
                out.extend_from_slice(line_break);
                fi.col = 0;
            }
            out.push(b);
            fi.col += 1;
        }
    } else {
        out.extend_from_slice(encoded.as_bytes());
        fi.col += encoded.len();
    }
    fi.leftover = data[take_len..].to_vec();
    if closing { fi.leftover.clear(); }
    out
}

fn base64_encode_no_wrap(input: &[u8], finish: bool) -> String {
    // Plain base64 (no padding mid-stream; only at the very end).
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len() * 4 / 3 + 4);
    let mut i = 0;
    while i + 3 <= input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        out.push(TABLE[(n & 0x3f) as usize] as char);
        i += 3;
    }
    if finish && i < input.len() {
        let rem = input.len() - i;
        let n = match rem {
            1 => (input[i] as u32) << 16,
            2 => ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8),
            _ => 0,
        };
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        if rem == 2 {
            out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        out.push('=');
    }
    out
}

fn base64_decode_streaming(f: &Rc<RefCell<FilterInstance>>, input: &[u8], closing: bool) -> Vec<u8> {
    let mut fi = f.borrow_mut();
    let mut data = std::mem::take(&mut fi.leftover);
    // Skip whitespace, accept only valid base64 characters.
    for &b in input {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' | b'=' => data.push(b),
            _ => {}
        }
    }
    let chunks = if closing { (data.len() + 3) / 4 } else { data.len() / 4 };
    let consume = chunks * 4;
    let consume = consume.min(data.len());
    let mut out = Vec::with_capacity(consume * 3 / 4 + 4);
    let mut i = 0;
    while i + 4 <= consume {
        let q: [u8; 4] = [data[i], data[i + 1], data[i + 2], data[i + 3]];
        let mut n: u32 = 0;
        let mut padcount = 0u32;
        for &c in &q {
            let v: u32 = match c {
                b'A'..=b'Z' => (c - b'A') as u32,
                b'a'..=b'z' => (c - b'a' + 26) as u32,
                b'0'..=b'9' => (c - b'0' + 52) as u32,
                b'+' => 62,
                b'/' => 63,
                b'=' => { padcount += 1; 0 }
                _ => 0,
            };
            n = (n << 6) | v;
        }
        out.push(((n >> 16) & 0xff) as u8);
        if padcount < 2 { out.push(((n >> 8) & 0xff) as u8); }
        if padcount < 1 { out.push((n & 0xff) as u8); }
        i += 4;
    }
    if closing && i < consume {
        // Trailing partial group (no padding): decode whatever's there.
        let rem = &data[i..consume];
        let mut buf = rem.to_vec();
        while buf.len() % 4 != 0 { buf.push(b'='); }
        let mut n: u32 = 0;
        let mut padcount = 0u32;
        for &c in &buf {
            let v: u32 = match c {
                b'A'..=b'Z' => (c - b'A') as u32,
                b'a'..=b'z' => (c - b'a' + 26) as u32,
                b'0'..=b'9' => (c - b'0' + 52) as u32,
                b'+' => 62,
                b'/' => 63,
                b'=' => { padcount += 1; 0 }
                _ => 0,
            };
            n = (n << 6) | v;
        }
        out.push(((n >> 16) & 0xff) as u8);
        if padcount < 2 { out.push(((n >> 8) & 0xff) as u8); }
        if padcount < 1 { out.push((n & 0xff) as u8); }
    }
    fi.leftover = if closing { Vec::new() } else { data[consume..].to_vec() };
    out
}

fn qp_encode_streaming(f: &Rc<RefCell<FilterInstance>>, input: &[u8], closing: bool, line_length: usize, line_break: &[u8], _binary: bool) -> Vec<u8> {
    // PHP's quoted-printable encoder is line-aware: it preserves SP/HT
    // bytes (encoded as themselves) inside a line, but escapes them as
    // =20/=09 when they would otherwise appear at the end of a line
    // (before LF or before a soft break). Newline ('\n') resets the
    // column and is emitted verbatim. '=' is always escaped.
    let mut fi = f.borrow_mut();
    let mut leftover = std::mem::take(&mut fi.leftover);
    leftover.extend_from_slice(input);
    let data = leftover;
    let mut out = Vec::with_capacity(data.len() * 2);
    // PHP's line_length includes the trailing `=` soft-break marker; the
    // effective content limit per line is line_length - 1.
    let max_col = if line_length == 0 { 75 } else { line_length.saturating_sub(1) };
    let mut col = fi.col;
    let mut i = 0;
    let n = data.len();
    while i < n {
        let b = data[i];
        // Newline: re-encode all pending trailing whitespace before lb.
        // If preceded by '\r' in the input (i.e. the previous byte was a
        // CR), both bytes are kept as hex so the receiver gets `=0D=0A`.
        let prev_was_cr = i > 0 && data[i - 1] == b'\r';
        if b == b'\n' && !prev_was_cr {
            // Collect trailing SP/HT chars (in reverse order).
            let mut trailing: Vec<u8> = Vec::new();
            while let Some(&last) = out.last() {
                if last == b' ' || last == b'\t' {
                    out.pop();
                    trailing.push(last);
                } else {
                    break;
                }
            }
            // Re-emit them as =HH (in original order).
            for &c in trailing.iter().rev() {
                out.extend_from_slice(format!("={:02X}", c).as_bytes());
            }
            out.extend_from_slice(line_break);
            col = 0;
            i += 1;
            continue;
        }
        let encode = b == b'=' || b < 0x20 || b > 0x7e;
        // Decide chunk length needed.
        let chunk_len = if encode { 3 } else { 1 };
        if col + chunk_len > max_col {
            // Soft break: peek backwards: if last char is whitespace, encode it.
            if !out.is_empty() {
                let last = *out.last().unwrap();
                if last == b' ' || last == b'\t' {
                    out.pop();
                    out.extend_from_slice(format!("={:02X}", last).as_bytes());
                }
            }
            out.push(b'=');
            out.extend_from_slice(line_break);
            col = 0;
        }
        if encode {
            out.extend_from_slice(format!("={:02X}", b).as_bytes());
            col += 3;
        } else {
            out.push(b);
            col += 1;
        }
        i += 1;
    }
    if closing {
        // Encode trailing space/tab so they're not lost.
        if !out.is_empty() {
            let last = *out.last().unwrap();
            if last == b' ' || last == b'\t' {
                out.pop();
                out.extend_from_slice(format!("={:02X}", last).as_bytes());
            }
        }
        fi.leftover.clear();
        fi.col = 0;
    } else {
        fi.col = col;
        fi.leftover.clear();
    }
    out
}

fn qp_decode_streaming(f: &Rc<RefCell<FilterInstance>>, input: &[u8], closing: bool) -> Vec<u8> {
    let mut fi = f.borrow_mut();
    let mut data = std::mem::take(&mut fi.leftover);
    data.extend_from_slice(input);
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    let n = data.len();
    while i < n {
        let b = data[i];
        if b == b'=' {
            // Need at least 2 more bytes (hex digits) to decode.
            if i + 2 < n {
                let h1 = data[i + 1];
                let h2 = data[i + 2];
                // Soft line break: =\r\n
                if h1 == b'\r' && h2 == b'\n' {
                    i += 3;
                    continue;
                }
                // Soft line break: =\n or =\r alone.
                if h1 == b'\n' || h1 == b'\r' {
                    i += 2;
                    continue;
                }
                if let (Some(v1), Some(v2)) = (hex_digit(h1), hex_digit(h2)) {
                    out.push((v1 << 4) | v2);
                    i += 3;
                    continue;
                }
                // Invalid - keep as-is.
                out.push(b);
                i += 1;
                continue;
            } else if i + 1 < n && (data[i + 1] == b'\n' || data[i + 1] == b'\r') {
                // Soft line break with only one byte after `=`.
                i += 2;
                continue;
            } else if !closing {
                // Carry over incomplete trailer.
                fi.leftover = data[i..].to_vec();
                return out;
            } else {
                // At end of stream with incomplete `=X` or just `=`:
                // strip the trailer.
                break;
            }
        } else {
            out.push(b);
            i += 1;
        }
    }
    if closing { fi.leftover.clear(); }
    out
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn dechunk_streaming(f: &Rc<RefCell<FilterInstance>>, input: &[u8], closing: bool) -> Vec<u8> {
    let mut fi = f.borrow_mut();
    let mut data = std::mem::take(&mut fi.leftover);
    data.extend_from_slice(input);
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    let n = data.len();
    loop {
        match fi.dechunk_state {
            DechunkState::Done => break,
            DechunkState::Header => {
                // Read chunk size: hex digits up to ';' or '\n' (LF or CRLF).
                let line_end = find_line_end(&data[i..]);
                let Some((line_len, skip)) = line_end else {
                    // Need more data.
                    break;
                };
                let line = &data[i..i + line_len];
                // Take hex digits up to ';' or whitespace.
                let mut hex_end = 0usize;
                for &b in line {
                    if matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F') {
                        hex_end += 1;
                    } else {
                        break;
                    }
                }
                let size_str = std::str::from_utf8(&line[..hex_end]).unwrap_or("0");
                let size = usize::from_str_radix(size_str, 16).unwrap_or(0);
                i += line_len + skip;
                if size == 0 {
                    fi.dechunk_state = DechunkState::Done;
                    // Consume trailing trailers / blank line.
                    break;
                }
                fi.dechunk_state = DechunkState::Body(size);
            }
            DechunkState::Body(remaining) => {
                let take = remaining.min(n - i);
                out.extend_from_slice(&data[i..i + take]);
                i += take;
                let new_remaining = remaining - take;
                if new_remaining == 0 {
                    fi.dechunk_state = DechunkState::AfterChunk;
                } else {
                    fi.dechunk_state = DechunkState::Body(new_remaining);
                    break;
                }
            }
            DechunkState::AfterChunk => {
                // Consume optional CRLF after chunk body.
                if i >= n { break; }
                if data[i] == b'\r' && i + 1 < n && data[i + 1] == b'\n' { i += 2; }
                else if data[i] == b'\n' { i += 1; }
                fi.dechunk_state = DechunkState::Header;
            }
        }
    }
    if closing { fi.leftover.clear(); }
    else { fi.leftover = data[i..].to_vec(); }
    out
}

fn find_line_end(buf: &[u8]) -> Option<(usize, usize)> {
    for (idx, &b) in buf.iter().enumerate() {
        if b == b'\r' && idx + 1 < buf.len() && buf[idx + 1] == b'\n' {
            return Some((idx, 2));
        }
        if b == b'\n' {
            return Some((idx, 1));
        }
    }
    None
}

fn run_user_filter(vm: &mut Vm, f: &Rc<RefCell<FilterInstance>>, input: &[u8], closing: bool) -> Vec<u8> {
    let obj = match f.borrow().user_obj.clone() {
        Some(o) => o,
        None => return input.to_vec(),
    };
    // Build $in and $out bucket brigades. We represent each as a PHP
    // object with a sentinel resource id; the bucket helper functions
    // resolve those ids through BUCKET_BRIGADES.
    let in_brigade = new_brigade();
    if !input.is_empty() {
        push_bucket(in_brigade, input.to_vec());
    }
    let out_brigade = new_brigade();
    // Create resource objects.
    let in_res = Value::Resource(in_brigade);
    goro_core::value::register_resource(in_brigade, b"userfilter.bucket brigade");
    let out_res = Value::Resource(out_brigade);
    goro_core::value::register_resource(out_brigade, b"userfilter.bucket brigade");
    // consumed must be passed by reference; we wrap it in a Reference.
    let consumed_rc = Rc::new(RefCell::new(Value::Long(0)));
    let consumed_ref = Value::Reference(consumed_rc.clone());
    let closing_val = if closing { Value::True } else { Value::False };
    // Invoke filter() method on the object.
    let args = vec![in_res, out_res, consumed_ref, closing_val];
    let obj_val = Value::Object(obj.clone());
    let _ = vm.call_object_method(&obj_val, b"filter", &args);
    // Drain output brigade.
    let out = drain_brigade(out_brigade);
    // Cleanup brigades.
    BUCKET_BRIGADES.with(|m| {
        let mut m = m.borrow_mut();
        m.remove(&in_brigade);
        m.remove(&out_brigade);
    });
    goro_core::value::close_resource(in_brigade);
    goro_core::value::close_resource(out_brigade);
    if closing && out.is_empty() {
        // No more output expected from this filter.
    }
    out
}

// --------- Bucket brigade primitives (for user filters) ----------

thread_local! {
    static BUCKET_BRIGADES: RefCell<HashMap<i64, Vec<Vec<u8>>>> = RefCell::new(HashMap::new());
    static NEXT_BRIGADE_ID: std::cell::Cell<i64> = const { std::cell::Cell::new(800_000) };
}

fn new_brigade() -> i64 {
    let id = NEXT_BRIGADE_ID.with(|c| { let n = c.get(); c.set(n + 1); n });
    BUCKET_BRIGADES.with(|m| { m.borrow_mut().insert(id, Vec::new()); });
    id
}

fn push_bucket(brigade: i64, data: Vec<u8>) {
    BUCKET_BRIGADES.with(|m| {
        if let Some(b) = m.borrow_mut().get_mut(&brigade) {
            b.push(data);
        }
    });
}

fn pop_bucket(brigade: i64) -> Option<Vec<u8>> {
    BUCKET_BRIGADES.with(|m| {
        m.borrow_mut().get_mut(&brigade).and_then(|b| {
            if b.is_empty() { None } else { Some(b.remove(0)) }
        })
    })
}

fn drain_brigade(brigade: i64) -> Vec<u8> {
    BUCKET_BRIGADES.with(|m| {
        let mut m = m.borrow_mut();
        let v = m.get_mut(&brigade);
        let mut out = Vec::new();
        if let Some(v) = v {
            for chunk in v.drain(..) {
                out.extend_from_slice(&chunk);
            }
        }
        out
    })
}

/// PHP function: stream_bucket_make_writeable($brigade): ?object
/// Returns an object with `data`, `datalen`, `eof` properties or null.
pub fn stream_bucket_make_writeable(vm: &mut Vm, args: &[Value]) -> Result<Value, goro_core::vm::VmError> {
    let id = args.first().map(|v| v.to_long()).unwrap_or(0);
    match pop_bucket(id) {
        Some(data) => {
            let obj_id = vm.next_object_id();
            let mut obj = PhpObject::new(b"stdClass".to_vec(), obj_id);
            let len = data.len() as i64;
            obj.set_property(b"data".to_vec(), Value::String(PhpString::from_vec(data)));
            obj.set_property(b"datalen".to_vec(), Value::Long(len));
            obj.set_property(b"eof".to_vec(), Value::False);
            Ok(Value::Object(Rc::new(RefCell::new(obj))))
        }
        None => Ok(Value::Null),
    }
}

/// PHP function: stream_bucket_append($brigade, $bucket): void
pub fn stream_bucket_append(_vm: &mut Vm, args: &[Value]) -> Result<Value, goro_core::vm::VmError> {
    let id = args.first().map(|v| v.to_long()).unwrap_or(0);
    let bucket = args.get(1).cloned().unwrap_or(Value::Null);
    if let Value::Object(obj) = bucket {
        let data_val = obj.borrow().get_property(b"data");
        let bytes = data_val.to_php_string().as_bytes().to_vec();
        push_bucket(id, bytes);
    }
    Ok(Value::Null)
}

/// PHP function: stream_bucket_prepend($brigade, $bucket): void
pub fn stream_bucket_prepend(_vm: &mut Vm, args: &[Value]) -> Result<Value, goro_core::vm::VmError> {
    let id = args.first().map(|v| v.to_long()).unwrap_or(0);
    let bucket = args.get(1).cloned().unwrap_or(Value::Null);
    if let Value::Object(obj) = bucket {
        let data_val = obj.borrow().get_property(b"data");
        let bytes = data_val.to_php_string().as_bytes().to_vec();
        BUCKET_BRIGADES.with(|m| {
            if let Some(b) = m.borrow_mut().get_mut(&id) {
                b.insert(0, bytes);
            }
        });
    }
    Ok(Value::Null)
}

/// PHP function: stream_bucket_new($stream, $data): object
pub fn stream_bucket_new(vm: &mut Vm, args: &[Value]) -> Result<Value, goro_core::vm::VmError> {
    let data = args.get(1).cloned().unwrap_or(Value::Null);
    let bytes = data.to_php_string().as_bytes().to_vec();
    let len = bytes.len() as i64;
    let obj_id = vm.next_object_id();
    let mut obj = PhpObject::new(b"stdClass".to_vec(), obj_id);
    obj.set_property(b"data".to_vec(), Value::String(PhpString::from_vec(bytes)));
    obj.set_property(b"datalen".to_vec(), Value::Long(len));
    obj.set_property(b"eof".to_vec(), Value::False);
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

/// Reset all filter state (between independent tests). Mirrors
/// `reset_resources()` in goro_core::value.
pub fn reset_all() {
    FILTERS.with(|m| m.borrow_mut().clear());
    STREAM_CHAINS.with(|m| m.borrow_mut().clear());
    USER_FILTER_CLASSES.with(|m| m.borrow_mut().clear());
    BUCKET_BRIGADES.with(|m| m.borrow_mut().clear());
}
