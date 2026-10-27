//! zlib.c: the one-shot encoders/decoders and the incremental
//! deflate_init()/inflate_init() contexts.

use goro_core::array::ArrayKey;
use goro_core::object::{NativeData, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm, VmError};
use goro_zlib::*;
use std::cell::RefCell;
use std::rc::Rc;

pub(crate) const PHP_ZLIB_ENCODING_RAW: i64 = -0xf;
pub(crate) const PHP_ZLIB_ENCODING_GZIP: i64 = 0x1f;
pub(crate) const PHP_ZLIB_ENCODING_DEFLATE: i64 = 0x0f;
pub(crate) const PHP_ZLIB_ENCODING_ANY: i64 = 0x2f;

/// PHP_ZLIB_BUFFER_SIZE_GUESS()
pub(crate) fn buffer_size_guess(in_len: usize) -> usize {
    ((in_len as f64 * 1.015) as usize) + 10 + 8 + 4 + 1
}

pub(crate) fn throw(vm: &mut Vm, class: &[u8], msg: &str) -> VmError {
    let exc = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError {
        message: msg.to_string(),
        line: vm.current_line,
    }
}

fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).map(|v| v.deref()).unwrap_or(Value::Null)
}

fn arg_long(args: &[Value], i: usize, default: i64) -> i64 {
    match args.get(i).map(|v| v.deref()) {
        None | Some(Value::Undef) => default,
        Some(v) => v.to_long(),
    }
}

/// zend_parse_arg_str() for a string parameter (weak mode).
pub(crate) fn zpp_str(
    vm: &mut Vm,
    v: &Value,
    fname: &str,
    n: usize,
    pname: &str,
) -> Result<PhpString, VmError> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Long(_) | Value::Double(_) | Value::True | Value::False => Ok(v.to_php_string()),
        Value::Null | Value::Undef => {
            vm.emit_deprecated(&format!(
                "{}(): Passing null to parameter #{} (${}) of type string is deprecated",
                fname, n, pname
            ));
            Ok(PhpString::empty())
        }
        Value::Object(o) => {
            let cl = o.borrow().class_name.to_ascii_lowercase();
            let has_tostring = vm
                .classes
                .get(&cl)
                .is_some_and(|c| c.get_method(b"__tostring").is_some());
            if has_tostring {
                vm.value_to_string_checked(v)
            } else {
                Err(throw(
                    vm,
                    b"TypeError",
                    &format!(
                        "{}(): Argument #{} (${}) must be of type string, {} given",
                        fname,
                        n,
                        pname,
                        type_name(v)
                    ),
                ))
            }
        }
        _ => Err(throw(
            vm,
            b"TypeError",
            &format!(
                "{}(): Argument #{} (${}) must be of type string, {} given",
                fname,
                n,
                pname,
                type_name(v)
            ),
        )),
    }
}

/// zend_parse_arg_long() for an int parameter (weak mode).
pub(crate) fn zpp_long(
    vm: &mut Vm,
    v: &Value,
    fname: &str,
    n: usize,
    pname: &str,
) -> Result<i64, VmError> {
    let type_err = |vm: &mut Vm, v: &Value| {
        throw(
            vm,
            b"TypeError",
            &format!(
                "{}(): Argument #{} (${}) must be of type int, {} given",
                fname,
                n,
                pname,
                type_name(v)
            ),
        )
    };
    let from_double = |vm: &mut Vm, d: f64, v: &Value| -> Result<i64, VmError> {
        if !d.is_finite() || d >= 9.223372036854776e18 || d < -9.223372036854776e18 {
            return Err(type_err(vm, v));
        }
        if d.fract() != 0.0 {
            vm.emit_deprecated(&format!(
                "Implicit conversion from float {} to int loses precision",
                Value::Double(d).to_php_string().to_string_lossy()
            ));
        }
        Ok(d as i64)
    };
    match v {
        Value::Long(l) => Ok(*l),
        Value::True => Ok(1),
        Value::False => Ok(0),
        Value::Double(d) => from_double(vm, *d, v),
        Value::Null | Value::Undef => {
            vm.emit_deprecated(&format!(
                "{}(): Passing null to parameter #{} (${}) of type int is deprecated",
                fname, n, pname
            ));
            Ok(0)
        }
        Value::String(s) => {
            let t = s
                .to_string_lossy()
                .trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}'))
                .to_string();
            if let Ok(l) = t.parse::<i64>() {
                return Ok(l);
            }
            let numeric = !t.is_empty()
                && t.bytes()
                    .all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-'))
                && t.bytes().any(|c| c.is_ascii_digit());
            match t.parse::<f64>() {
                Ok(d) if numeric => from_double(vm, d, v),
                _ => Err(type_err(vm, v)),
            }
        }
        _ => Err(type_err(vm, v)),
    }
}

/// php_zlib_encode()
pub(crate) fn php_zlib_encode(vm: &mut Vm, input: &[u8], encoding: i64, level: i64) -> Value {
    let (status, out) = deflate_once(
        input,
        level as i32,
        encoding as i32,
        MAX_MEM_LEVEL,
        Z_DEFAULT_STRATEGY,
        buffer_size_guess(input.len()),
    );
    if status == Z_STREAM_END {
        return Value::String(PhpString::from_vec(out));
    }
    vm.emit_warning(&format!(
        "{}(): {}",
        vm.current_builtin_name().unwrap_or_default(),
        z_error(status)
    ));
    Value::False
}

/// php_zlib_inflate_rounds()
fn inflate_rounds(z: &mut Inflate, input: &[u8], max: usize) -> (i32, Vec<u8>) {
    let mut status;
    let mut round = 0;
    let mut data: Vec<u8> = Vec::new();
    let mut used = 0usize;
    let mut in_pos = 0usize;
    let avail_in = input.len();
    let mut size = if max != 0 && max < avail_in {
        max
    } else {
        avail_in
    };
    loop {
        if max != 0 && max <= used {
            status = Z_MEM_ERROR;
        } else {
            data.resize(size, 0);
            let mut io = Io::new(input, &mut data[..size]);
            io.in_pos = in_pos;
            io.out_pos = used;
            status = z.inflate(&mut io, Z_NO_FLUSH);
            in_pos = io.in_pos;
            used = io.out_pos;
            size += (size >> 3) + 1;
        }
        round += 1;
        if !((status == Z_BUF_ERROR || (status == Z_OK && in_pos < input.len())) && round < 100) {
            break;
        }
    }
    if status == Z_STREAM_END {
        data.truncate(used);
        (status, data)
    } else {
        (
            if status == Z_OK { Z_DATA_ERROR } else { status },
            Vec::new(),
        )
    }
}

/// php_zlib_decode()
pub(crate) fn php_zlib_decode(
    vm: &mut Vm,
    input: &[u8],
    encoding: i64,
    max_len: usize,
) -> Option<Vec<u8>> {
    let mut status = Z_DATA_ERROR;
    if !input.is_empty() {
        // NOTE: data must be zero terminated (avail_in = in_len + 1)
        let mut buf = Vec::with_capacity(input.len() + 1);
        buf.extend_from_slice(input);
        buf.push(0);
        let mut encoding = encoding;
        loop {
            match Inflate::new(encoding as i32) {
                Ok(mut z) => {
                    let (st, out) = inflate_rounds(&mut z, &buf, max_len);
                    status = st;
                    if st == Z_STREAM_END {
                        return Some(out);
                    }
                    if st == Z_DATA_ERROR && encoding == PHP_ZLIB_ENCODING_ANY {
                        encoding = PHP_ZLIB_ENCODING_RAW;
                        continue;
                    }
                }
                Err(e) => status = e,
            }
            break;
        }
    }
    vm.emit_warning(&format!(
        "{}(): {}",
        vm.current_builtin_name().unwrap_or_default(),
        z_error(status)
    ));
    None
}

fn encode_common(
    vm: &mut Vm,
    args: &[Value],
    default_encoding: i64,
    fname: &str,
) -> Result<Value, VmError> {
    let data = vm.value_to_string_checked(&arg(args, 0))?;
    let (level, encoding) = if default_encoding != 0 {
        (arg_long(args, 1, -1), arg_long(args, 2, default_encoding))
    } else {
        (arg_long(args, 2, -1), arg_long(args, 1, 0))
    };
    if !(-1..=9).contains(&level) {
        let n = if default_encoding != 0 { 2 } else { 3 };
        return Err(throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #{} ($level) must be between -1 and 9",
                fname, n
            ),
        ));
    }
    match encoding {
        PHP_ZLIB_ENCODING_RAW | PHP_ZLIB_ENCODING_GZIP | PHP_ZLIB_ENCODING_DEFLATE => {}
        _ => {
            let n = if default_encoding != 0 { 3 } else { 2 };
            return Err(throw(
                vm,
                b"ValueError",
                &format!(
                    "{}(): Argument #{} ($encoding) must be one of ZLIB_ENCODING_RAW, ZLIB_ENCODING_GZIP, or ZLIB_ENCODING_DEFLATE",
                    fname, n
                ),
            ));
        }
    }
    Ok(php_zlib_encode(vm, data.as_bytes(), encoding, level))
}

fn decode_common(
    vm: &mut Vm,
    args: &[Value],
    encoding: i64,
    fname: &str,
) -> Result<Value, VmError> {
    let data = vm.value_to_string_checked(&arg(args, 0))?;
    let max_len = arg_long(args, 1, 0);
    if max_len < 0 {
        return Err(throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #2 ($max_length) must be greater than or equal to 0",
                fname
            ),
        ));
    }
    Ok(
        match php_zlib_decode(vm, data.as_bytes(), encoding, max_len as usize) {
            Some(out) => Value::String(PhpString::from_vec(out)),
            None => Value::False,
        },
    )
}

pub(crate) fn zlib_encode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    encode_common(vm, args, 0, "zlib_encode")
}
pub(crate) fn gzdeflate(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    encode_common(vm, args, PHP_ZLIB_ENCODING_RAW, "gzdeflate")
}
pub(crate) fn gzencode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    encode_common(vm, args, PHP_ZLIB_ENCODING_GZIP, "gzencode")
}
pub(crate) fn gzcompress(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    encode_common(vm, args, PHP_ZLIB_ENCODING_DEFLATE, "gzcompress")
}
pub(crate) fn zlib_decode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    decode_common(vm, args, PHP_ZLIB_ENCODING_ANY, "zlib_decode")
}
pub(crate) fn gzinflate(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    decode_common(vm, args, PHP_ZLIB_ENCODING_RAW, "gzinflate")
}
pub(crate) fn gzdecode(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    decode_common(vm, args, PHP_ZLIB_ENCODING_GZIP, "gzdecode")
}
pub(crate) fn gzuncompress(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    decode_common(vm, args, PHP_ZLIB_ENCODING_DEFLATE, "gzuncompress")
}

// ---------------------------------------------------------------------------
// DeflateContext / InflateContext
// ---------------------------------------------------------------------------

/// php_zlib_context
pub(crate) enum ZCtx {
    Deflate(Box<Deflate>),
    Inflate {
        z: Box<Inflate>,
        dict: Option<Vec<u8>>,
        status: i32,
    },
}

/// zend_objects_not_comparable: distinct contexts are never equal.
fn not_comparable(a: &Value, b: &Value) -> Option<i64> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) if Rc::ptr_eq(x, y) => Some(0),
        (Value::Object(_), Value::Object(_)) => Some(1),
        _ => None,
    }
}

static CTX_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    compare: Some(not_comparable),
    ..NativeObjectHandlers::EMPTY
};

fn ctx_of(v: &Value, class: &[u8]) -> Option<Rc<RefCell<ZCtx>>> {
    if let Value::Object(o) = v.deref() {
        let o = o.borrow();
        if !o.class_name.eq_ignore_ascii_case(class) {
            return None;
        }
        if let Some(n) = &o.native {
            return n.data.clone().downcast::<RefCell<ZCtx>>().ok();
        }
    }
    None
}

fn type_name(v: &Value) -> String {
    Vm::value_type_name(&v.deref())
}

/// zend_hash_str_find() on an options array/object.
fn option(opts: &Value, name: &[u8]) -> Option<Value> {
    match opts.deref() {
        Value::Array(a) => a
            .borrow()
            .get(&ArrayKey::String(PhpString::from_bytes(name)))
            .map(|v| v.deref()),
        Value::Object(o) => o
            .borrow()
            .properties
            .iter()
            .find(|(k, _)| k.as_slice() == name)
            .map(|(_, v)| v.deref()),
        _ => None,
    }
}

/// zlib_create_dictionary_string(): Ok(None) when no dictionary given.
fn create_dictionary(vm: &mut Vm, fname: &str, opts: &Value) -> Result<Option<Vec<u8>>, VmError> {
    let Some(v) = option(opts, b"dictionary") else {
        return Ok(None);
    };
    match &v {
        Value::String(s) => Ok(Some(s.as_bytes().to_vec())),
        Value::Array(a) => {
            let items: Vec<Value> = a.borrow().iter().map(|(_, v)| v.deref()).collect();
            if items.is_empty() {
                return Ok(None);
            }
            let mut dict = Vec::new();
            for item in items {
                let s = vm.value_to_string_checked(&item)?;
                let b = s.as_bytes();
                if b.is_empty() {
                    return Err(throw(
                        vm,
                        b"ValueError",
                        &format!(
                            "{}(): Argument #2 ($options) must not contain empty strings",
                            fname
                        ),
                    ));
                }
                if b.contains(&0) {
                    return Err(throw(
                        vm,
                        b"ValueError",
                        &format!(
                            "{}(): Argument #2 ($options) must not contain strings with null bytes",
                            fname
                        ),
                    ));
                }
                dict.extend_from_slice(b);
                dict.push(0);
            }
            Ok(Some(dict))
        }
        other => Err(throw(
            vm,
            b"TypeError",
            &format!(
                "{}(): Argument #2 ($options) must be of type zero-terminated string or array, {} given",
                fname,
                type_name(other)
            ),
        )),
    }
}

fn check_options_arg(vm: &mut Vm, args: &[Value], fname: &str) -> Result<Value, VmError> {
    let opts = arg(args, 1);
    match &opts {
        Value::Null | Value::Undef if args.len() < 2 => Ok(Value::Null),
        Value::Array(_) | Value::Object(_) => Ok(opts),
        other => Err(throw(
            vm,
            b"TypeError",
            &format!(
                "{}(): Argument #2 ($options) must be of type array, {} given",
                fname,
                type_name(other)
            ),
        )),
    }
}

/// PHP_FUNCTION(inflate_init)
pub(crate) fn inflate_init(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let encoding = arg_long(args, 0, 0);
    let opts = check_options_arg(vm, args, "inflate_init")?;
    let mut window = 15i64;
    if let Some(v) = option(&opts, b"window") {
        window = v.to_long();
    }
    if !(8..=15).contains(&window) {
        return Err(throw(
            vm,
            b"ValueError",
            &format!(
                "zlib window size (logarithm) ({}) must be within 8..15",
                window
            ),
        ));
    }
    match encoding {
        PHP_ZLIB_ENCODING_RAW | PHP_ZLIB_ENCODING_GZIP | PHP_ZLIB_ENCODING_DEFLATE => {}
        _ => {
            return Err(throw(
                vm,
                b"ValueError",
                "Encoding mode must be ZLIB_ENCODING_RAW, ZLIB_ENCODING_GZIP or ZLIB_ENCODING_DEFLATE",
            ));
        }
    }
    let mut dict = create_dictionary(vm, "inflate_init", &opts)?;
    let encoding = if encoding < 0 {
        encoding + 15 - window
    } else {
        encoding - (15 - window)
    };
    // The object is created before inflateInit2() (object ids).
    let id = vm.next_object_id();
    let mut obj = PhpObject::new(b"InflateContext".to_vec(), id);
    let mut z = match Inflate::new(encoding as i32) {
        Ok(z) => z,
        Err(_) => {
            vm.emit_warning("inflate_init(): Failed allocating zlib.inflate context");
            return Ok(Value::False);
        }
    };
    if encoding == PHP_ZLIB_ENCODING_RAW && dict.as_ref().is_some_and(|d| !d.is_empty()) {
        let d = dict.take().unwrap_or_default();
        if z.set_dictionary(&d) == Z_DATA_ERROR {
            vm.emit_warning(
                "inflate_init(): Dictionary does not match expected dictionary (incorrect adler32 hash)",
            );
        }
    }
    let data: Rc<dyn std::any::Any> = Rc::new(RefCell::new(ZCtx::Inflate {
        z,
        dict,
        status: Z_OK,
    }));
    obj.native = Some(NativeData {
        data,
        handlers: &CTX_HANDLERS,
    });
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

fn flush_mode_arg(vm: &mut Vm, args: &[Value], fname: &str) -> Result<i32, VmError> {
    let f = match args.get(2).map(|v| v.deref()) {
        None | Some(Value::Undef) => Z_SYNC_FLUSH as i64,
        Some(v) => zpp_long(vm, &v, fname, 3, "flush_mode")?,
    };
    match f as i32 {
        Z_NO_FLUSH | Z_PARTIAL_FLUSH | Z_SYNC_FLUSH | Z_FULL_FLUSH | Z_BLOCK | Z_FINISH
            if (0..=5).contains(&f) =>
        {
            Ok(f as i32)
        }
        _ => Err(throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #3 ($flush_mode) must be one of ZLIB_NO_FLUSH, ZLIB_PARTIAL_FLUSH, ZLIB_SYNC_FLUSH, ZLIB_FULL_FLUSH, ZLIB_BLOCK, or ZLIB_FINISH",
                fname
            ),
        )),
    }
}

/// PHP_FUNCTION(inflate_add)
pub(crate) fn inflate_add(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a0 = arg(args, 0);
    let Some(ctx) = ctx_of(&a0, b"InflateContext") else {
        return Err(throw(
            vm,
            b"TypeError",
            &format!(
                "inflate_add(): Argument #1 ($context) must be of type InflateContext, {} given",
                type_name(&a0)
            ),
        ));
    };
    let data = zpp_str(vm, &arg(args, 1), "inflate_add", 2, "data")?;
    let flush_type = flush_mode_arg(vm, args, "inflate_add")?;
    let mut c = ctx.borrow_mut();
    let ZCtx::Inflate { z, dict, status } = &mut *c else {
        return Ok(Value::False);
    };
    if *status == Z_STREAM_END {
        *status = Z_OK;
        z.reset();
    }
    let input = data.as_bytes();
    if input.is_empty() && flush_type != Z_FINISH {
        return Ok(Value::String(PhpString::empty()));
    }
    const CHUNK_SIZE: usize = 8192;
    let mut out = vec![0u8; input.len().max(CHUNK_SIZE)];
    let mut in_pos = 0usize;
    let mut used = 0usize;
    loop {
        let mut io = Io::new(input, &mut out);
        io.in_pos = in_pos;
        io.out_pos = used;
        let st = z.inflate(&mut io, flush_type);
        in_pos = io.in_pos;
        used = io.out_pos;
        let avail_out = out.len() - used;
        *status = st;
        match st {
            Z_OK => {
                if avail_out == 0 {
                    out.resize(out.len() + CHUNK_SIZE, 0);
                } else {
                    break;
                }
            }
            Z_STREAM_END => break,
            Z_BUF_ERROR => {
                if flush_type == Z_FINISH && avail_out == 0 {
                    out.resize(out.len() + CHUNK_SIZE, 0);
                } else {
                    break;
                }
            }
            Z_NEED_DICT => {
                if let Some(d) = dict.take() {
                    if z.set_dictionary(&d) == Z_DATA_ERROR {
                        drop(c);
                        vm.emit_warning(
                            "inflate_add(): Dictionary does not match expected dictionary (incorrect adler32 hash)",
                        );
                        return Ok(Value::False);
                    }
                } else {
                    drop(c);
                    vm.emit_warning(
                        "inflate_add(): Inflating this data requires a preset dictionary, please specify it in the options array of inflate_init()",
                    );
                    return Ok(Value::False);
                }
            }
            _ => {
                drop(c);
                vm.emit_warning(&format!("inflate_add(): {}", z_error(st)));
                return Ok(Value::False);
            }
        }
    }
    out.truncate(used);
    Ok(Value::String(PhpString::from_vec(out)))
}

fn inflate_ctx_arg(vm: &mut Vm, args: &[Value], fname: &str) -> Result<Rc<RefCell<ZCtx>>, VmError> {
    let a0 = arg(args, 0);
    match ctx_of(&a0, b"InflateContext") {
        Some(c) => Ok(c),
        None => Err(throw(
            vm,
            b"TypeError",
            &format!(
                "{}(): Argument #1 ($context) must be of type InflateContext, {} given",
                fname,
                type_name(&a0)
            ),
        )),
    }
}

/// PHP_FUNCTION(inflate_get_status)
pub(crate) fn inflate_get_status(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let ctx = inflate_ctx_arg(vm, args, "inflate_get_status")?;
    let c = ctx.borrow();
    Ok(match &*c {
        ZCtx::Inflate { status, .. } => Value::Long(*status as i64),
        _ => Value::False,
    })
}

/// PHP_FUNCTION(inflate_get_read_len)
pub(crate) fn inflate_get_read_len(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let ctx = inflate_ctx_arg(vm, args, "inflate_get_read_len")?;
    let c = ctx.borrow();
    Ok(match &*c {
        ZCtx::Inflate { z, .. } => Value::Long(z.total_in as i64),
        _ => Value::False,
    })
}

/// PHP_FUNCTION(deflate_init)
pub(crate) fn deflate_init(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let encoding = arg_long(args, 0, 0);
    let opts = check_options_arg(vm, args, "deflate_init")?;
    let mut level = -1i64;
    if let Some(v) = option(&opts, b"level") {
        level = v.to_long();
    }
    if !(-1..=9).contains(&level) {
        return Err(throw(
            vm,
            b"ValueError",
            "deflate_init(): \"level\" option must be between -1 and 9",
        ));
    }
    let mut memory = 8i64;
    if let Some(v) = option(&opts, b"memory") {
        memory = v.to_long();
    }
    if !(1..=9).contains(&memory) {
        return Err(throw(
            vm,
            b"ValueError",
            "deflate_init(): \"memory\" option must be between 1 and 9",
        ));
    }
    let mut window = 15i64;
    if let Some(v) = option(&opts, b"window") {
        window = v.to_long();
    }
    if !(8..=15).contains(&window) {
        return Err(throw(
            vm,
            b"ValueError",
            "deflate_init(): \"window\" option must be between 8 and 15",
        ));
    }
    let mut strategy = Z_DEFAULT_STRATEGY as i64;
    if let Some(v) = option(&opts, b"strategy") {
        strategy = v.to_long();
    }
    if !(0..=4).contains(&strategy) {
        return Err(throw(
            vm,
            b"ValueError",
            "deflate_init(): \"strategy\" option must be one of ZLIB_FILTERED, ZLIB_HUFFMAN_ONLY, ZLIB_RLE, ZLIB_FIXED, or ZLIB_DEFAULT_STRATEGY",
        ));
    }
    match encoding {
        PHP_ZLIB_ENCODING_RAW | PHP_ZLIB_ENCODING_GZIP | PHP_ZLIB_ENCODING_DEFLATE => {}
        _ => {
            return Err(throw(
                vm,
                b"ValueError",
                "deflate_init(): Argument #1 ($encoding) must be one of ZLIB_ENCODING_RAW, ZLIB_ENCODING_GZIP, or ZLIB_ENCODING_DEFLATE",
            ));
        }
    }
    let dict = create_dictionary(vm, "deflate_init", &opts)?;
    let encoding = if encoding < 0 {
        encoding + 15 - window
    } else {
        encoding - (15 - window)
    };
    let id = vm.next_object_id();
    let mut obj = PhpObject::new(b"DeflateContext".to_vec(), id);
    let mut z = match Deflate::new(
        level as i32,
        Z_DEFLATED,
        encoding as i32,
        memory as i32,
        strategy as i32,
    ) {
        Ok(z) => z,
        Err(_) => {
            vm.emit_warning("deflate_init(): Failed allocating zlib.deflate context");
            return Ok(Value::False);
        }
    };
    if let Some(d) = dict {
        z.set_dictionary(&d);
    }
    let data: Rc<dyn std::any::Any> = Rc::new(RefCell::new(ZCtx::Deflate(z)));
    obj.native = Some(NativeData {
        data,
        handlers: &CTX_HANDLERS,
    });
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

/// PHP_FUNCTION(deflate_add)
pub(crate) fn deflate_add(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a0 = arg(args, 0);
    let Some(ctx) = ctx_of(&a0, b"DeflateContext") else {
        return Err(throw(
            vm,
            b"TypeError",
            &format!(
                "deflate_add(): Argument #1 ($context) must be of type DeflateContext, {} given",
                type_name(&a0)
            ),
        ));
    };
    let data = zpp_str(vm, &arg(args, 1), "deflate_add", 2, "data")?;
    let flush_type = flush_mode_arg(vm, args, "deflate_add")?;
    let input = data.as_bytes();
    if input.is_empty() && flush_type != Z_FINISH {
        return Ok(Value::String(PhpString::empty()));
    }
    let mut c = ctx.borrow_mut();
    let ZCtx::Deflate(z) = &mut *c else {
        return Ok(Value::False);
    };
    let out_size = buffer_size_guess(input.len()).max(64);
    let mut out = vec![0u8; out_size];
    let mut in_pos = 0usize;
    let mut used = 0usize;
    let mut status;
    loop {
        if out.len() == used {
            out.resize(out.len() + 64, 0);
        }
        let mut io = Io::new(input, &mut out);
        io.in_pos = in_pos;
        io.out_pos = used;
        status = z.deflate(&mut io, flush_type);
        in_pos = io.in_pos;
        used = io.out_pos;
        if !(status == Z_OK && out.len() == used) {
            break;
        }
    }
    match status {
        Z_OK => {
            out.truncate(used);
            Ok(Value::String(PhpString::from_vec(out)))
        }
        Z_STREAM_END => {
            out.truncate(used);
            z.reset();
            Ok(Value::String(PhpString::from_vec(out)))
        }
        st => {
            drop(c);
            vm.emit_warning(&format!("deflate_add(): zlib error ({})", z_error(st)));
            Ok(Value::False)
        }
    }
}

/// Creates the class entries of DeflateContext / InflateContext.
pub(crate) fn register_classes(vm: &mut Vm) {
    for name in [&b"DeflateContext"[..], &b"InflateContext"[..]] {
        let mut ce = goro_core::object::ClassEntry::new(name.to_vec());
        ce.is_final = true;
        vm.classes.insert(name.to_ascii_lowercase(), ce);
    }
}
