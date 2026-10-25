//! php-src's ext/iconv on top of an emulation of glibc's iconv(3)
//! (`conv`): iconv(), the iconv_str*() functions, the MIME header
//! functions, iconv_get/set_encoding(), the ob_iconv_handler output handler
//! and the convert.iconv.* stream filter.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

pub mod charset;
mod cjk;
pub mod conv;
mod iso2022;
mod mime;
#[rustfmt::skip]
mod sbcs_tables;
#[rustfmt::skip]
mod comp_tables;
#[rustfmt::skip]
mod big5_glibc;
#[rustfmt::skip]
mod translit_table;

use conv::{Errno, Iconv};

/// The extension name (`extension_loaded('iconv')`).
pub const NAME: &[u8] = b"iconv";

type R = Result<Value, VmError>;

/// ICONV_CSNMAXLEN (glibc)
pub(crate) const CSNMAXLEN: usize = 64;

/// The generic superset the string functions convert through
/// (ICONV_UCS4_ENCODING).
pub(crate) const UCS4: &[u8] = b"UCS-4LE";

/// php_iconv_err_t
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Err {
    Success,
    WrongCharset,
    TooBig,
    IllegalSeq,
    IllegalChar,
    /// (with errno)
    Unknown(i32),
    Malformed,
    OutByBounds,
}

impl From<Errno> for Err {
    fn from(e: Errno) -> Err {
        match e {
            Errno::Ilseq => Err::IllegalSeq,
            Errno::Inval => Err::IllegalChar,
            Errno::TooBig => Err::TooBig,
        }
    }
}

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("iconv_strlen", iconv_strlen),
    ("iconv_substr", iconv_substr),
    ("iconv_strpos", iconv_strpos),
    ("iconv_strrpos", iconv_strrpos),
    ("iconv_mime_encode", iconv_mime_encode),
    ("iconv_mime_decode", iconv_mime_decode),
    ("iconv_mime_decode_headers", iconv_mime_decode_headers),
    ("iconv", iconv_fn),
    ("iconv_set_encoding", iconv_set_encoding),
    ("iconv_get_encoding", iconv_get_encoding),
];

/// MINIT
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    for &(name, f) in FUNCTIONS {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), f, &params);
        vm.zpp_extension_verified.insert(
            name.as_bytes().to_vec(),
            goro_core::arginfo::CHECK_ARITY | goro_core::arginfo::CHECK_TYPES,
        );
    }
    let s = |v: &str| Value::String(PhpString::from_string(v.to_string()));
    vm.constants.insert(b"ICONV_IMPL".to_vec(), s("glibc"));
    vm.constants.insert(b"ICONV_VERSION".to_vec(), s("2.43"));
    vm.constants
        .insert(b"ICONV_MIME_DECODE_STRICT".to_vec(), Value::Long(1));
    vm.constants.insert(
        b"ICONV_MIME_DECODE_CONTINUE_ON_ERROR".to_vec(),
        Value::Long(2),
    );
    vm.module_constants.push((
        NAME.to_vec(),
        [
            "ICONV_IMPL",
            "ICONV_VERSION",
            "ICONV_MIME_DECODE_STRICT",
            "ICONV_MIME_DECODE_CONTINUE_ON_ERROR",
        ]
        .iter()
        .map(|n| n.as_bytes().to_vec())
        .collect(),
    ));
    for k in [
        &b"iconv.input_encoding"[..],
        b"iconv.output_encoding",
        b"iconv.internal_encoding",
    ] {
        vm.constants
            .insert(k.to_vec(), Value::String(PhpString::empty()));
    }
    vm.ini_validators
        .insert(b"iconv.input_encoding".to_vec(), on_update_input);
    vm.ini_validators
        .insert(b"iconv.output_encoding".to_vec(), on_update_output);
    vm.ini_validators
        .insert(b"iconv.internal_encoding".to_vec(), on_update_internal);
    goro_ext_standard::ob::register_output_handler_alias(b"ob_iconv_handler", ob_handler_init);
    goro_ext_standard::ob::register_output_handler_conflict(b"ob_iconv_handler", ob_conflict);
    goro_ext_standard::filters::register_ext_filter_factory(b"convert.iconv.*", filter_create);
}

fn on_update(vm: &mut Vm, value: &[u8], startup: bool, which: &str) -> bool {
    if value.len() >= CSNMAXLEN {
        return false;
    }
    if !startup {
        docref(
            vm,
            Level::Deprecated,
            &format!("Use of iconv.{} is deprecated", which),
        );
    }
    true
}

fn on_update_input(vm: &mut Vm, value: &[u8], startup: bool) -> bool {
    on_update(vm, value, startup, "input_encoding")
}

fn on_update_output(vm: &mut Vm, value: &[u8], startup: bool) -> bool {
    on_update(vm, value, startup, "output_encoding")
}

fn on_update_internal(vm: &mut Vm, value: &[u8], startup: bool) -> bool {
    on_update(vm, value, startup, "internal_encoding")
}

fn ini_str(vm: &Vm, k: &[u8]) -> Vec<u8> {
    vm.constants
        .get(k)
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default()
}

/// php_get_internal_encoding() / input / output: the INI setting, else
/// default_charset.
fn php_encoding(vm: &Vm, key: &[u8]) -> Vec<u8> {
    let v = ini_str(vm, key);
    if !v.is_empty() {
        return v;
    }
    let d = ini_str(vm, b"default_charset");
    if d.is_empty() { b"UTF-8".to_vec() } else { d }
}

pub(crate) fn internal_encoding(vm: &Vm) -> Vec<u8> {
    let v = ini_str(vm, b"iconv.internal_encoding");
    if !v.is_empty() {
        return v;
    }
    php_encoding(vm, b"internal_encoding")
}

fn input_encoding(vm: &Vm) -> Vec<u8> {
    let v = ini_str(vm, b"iconv.input_encoding");
    if !v.is_empty() {
        return v;
    }
    php_encoding(vm, b"input_encoding")
}

fn output_encoding(vm: &Vm) -> Vec<u8> {
    let v = ini_str(vm, b"iconv.output_encoding");
    if !v.is_empty() {
        return v;
    }
    php_encoding(vm, b"output_encoding")
}

pub(crate) enum Level {
    Warning,
    Notice,
    Deprecated,
}

/// php_error_docref(): "function(): message".
pub(crate) fn docref(vm: &mut Vm, level: Level, msg: &str) {
    let full = match vm.current_builtin_name() {
        Some(f) => format!("{}(): {}", f, msg),
        None => msg.to_string(),
    };
    let line = vm.current_line;
    match level {
        Level::Warning => vm.emit_warning(&full),
        Level::Notice => vm.emit_notice_at(&full, line),
        Level::Deprecated => vm.emit_deprecated_at(&full, line),
    }
}

/// _php_iconv_show_error()
pub(crate) fn show_error(vm: &mut Vm, err: Err, out_charset: &[u8], in_charset: &[u8]) {
    let l = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    match err {
        Err::Success => {}
        Err::WrongCharset => docref(
            vm,
            Level::Warning,
            &format!(
                "Wrong encoding, conversion from \"{}\" to \"{}\" is not allowed",
                l(in_charset),
                l(out_charset)
            ),
        ),
        Err::IllegalChar => docref(
            vm,
            Level::Notice,
            "Detected an incomplete multibyte character in input string",
        ),
        Err::IllegalSeq => docref(
            vm,
            Level::Notice,
            "Detected an illegal character in input string",
        ),
        Err::TooBig => docref(vm, Level::Warning, "Buffer length exceeded"),
        Err::Malformed => docref(vm, Level::Warning, "Malformed string"),
        Err::OutByBounds => {
            let f = vm.current_builtin_name().unwrap_or_default();
            let exc = vm.create_exception(
                b"ValueError",
                &format!(
                    "{}(): Argument #3 ($offset) must be contained in argument #1 ($haystack)",
                    f
                ),
                vm.current_line,
            );
            vm.current_exception = Some(exc);
        }
        Err::Unknown(errno) => docref(vm, Level::Notice, &format!("Unknown error ({})", errno)),
    }
}

/// _php_check_ignore()
fn check_ignore(charset: &[u8]) -> bool {
    let n = charset.len();
    (n >= 9 && charset.ends_with(b"//IGNORE"))
        || (n >= 19 && charset.ends_with(b"//IGNORE//TRANSLIT"))
}

/// php_iconv_string(): the converted string (possibly partial on error)
/// and the error.
pub(crate) fn iconv_string(
    input: &[u8],
    out_charset: &[u8],
    in_charset: &[u8],
) -> (Option<Vec<u8>>, Err) {
    let Some(mut cd) = Iconv::open(out_charset, in_charset) else {
        return (None, Err::WrongCharset);
    };
    let ignore_ilseq = check_ignore(out_charset);
    // (php's buffer: the input's length + 32 bytes, grown by the input's
    // length when full; glibc's stateful encoders show where it was full)
    let mut bsz = input.len() + 32;
    let mut out = Vec::with_capacity(bsz);
    let mut pos = 0;
    let mut result: Result<(), Errno> = Ok(());
    while pos < input.len() {
        let room = bsz - out.len();
        result = cd.conv(input, &mut pos, &mut out, Some(room));
        if let Err(Errno::Ilseq) = result {
            if ignore_ilseq {
                if input.len() - pos <= 1 {
                    result = Ok(());
                } else {
                    pos += 1;
                    continue;
                }
            }
        }
        if result == Err(Errno::TooBig) && pos < input.len() {
            bsz += input.len();
            continue;
        }
        break;
    }
    if result.is_ok() {
        loop {
            let room = bsz - out.len();
            result = cd.flush(&mut out, Some(room));
            if result == Err(Errno::TooBig) {
                bsz += 16;
                continue;
            }
            break;
        }
    }
    match result {
        Ok(()) => (Some(out), Err::Success),
        Err(e) => (Some(out), e.into()),
    }
}

fn arg(args: &[Value], i: usize) -> Value {
    match args.get(i) {
        Some(v) => v.deref(),
        None => Value::Undef,
    }
}

fn str_arg(args: &[Value], i: usize) -> Vec<u8> {
    arg(args, i).to_php_string().as_bytes().to_vec()
}

/// An optional (nullable) charset argument: Ok(None) when absent.
fn charset_arg(vm: &mut Vm, args: &[Value], i: usize) -> Result<Vec<u8>, ()> {
    match arg(args, i) {
        Value::Undef | Value::Null => Ok(internal_encoding(vm)),
        v => {
            let s = v.to_php_string().as_bytes().to_vec();
            if s.len() >= CSNMAXLEN {
                docref(
                    vm,
                    Level::Warning,
                    &format!(
                        "Encoding parameter exceeds the maximum allowed length of {} characters",
                        CSNMAXLEN
                    ),
                );
                return Err(());
            }
            Ok(s)
        }
    }
}

fn sval(v: Vec<u8>) -> Value {
    Value::String(PhpString::from_vec(v))
}

/// _php_iconv_strlen()
fn iconv_strlen_impl(s: &[u8], enc: &[u8]) -> (Option<usize>, Err) {
    match conv::decode_all(s, enc) {
        None => (None, Err::WrongCharset),
        Some((chars, None)) => (Some(chars.len()), Err::Success),
        Some((_, Some(e))) => (None, e.into()),
    }
}

fn iconv_strlen(vm: &mut Vm, args: &[Value]) -> R {
    let s = str_arg(args, 0);
    let Ok(cs) = charset_arg(vm, args, 1) else {
        return Ok(Value::False);
    };
    let (n, err) = iconv_strlen_impl(&s, &cs);
    show_error(vm, err, UCS4, &cs);
    Ok(match n {
        Some(n) if err == Err::Success => Value::Long(n as i64),
        _ => Value::False,
    })
}

/// _php_iconv_substr()
fn iconv_substr_impl(
    s: &[u8],
    mut offset: i64,
    mut len: i64,
    enc: &[u8],
) -> (Option<Vec<u8>>, Err) {
    let (total, err) = iconv_strlen_impl(s, enc);
    let Some(total) = total else {
        return (None, err);
    };
    let total = total as i64;
    if offset < 0 {
        offset += total;
        if offset < 0 {
            offset = 0;
        }
    } else if offset > total {
        offset = total;
    }
    if len < 0 {
        len += total - offset;
        if len < 0 {
            len = 0;
        }
    } else if len > total {
        len = total;
    }
    if offset + len > total {
        len = total - offset;
    }
    if len == 0 {
        return (Some(Vec::new()), Err::Success);
    }
    let mut second = Vec::new();
    let Some((chars, _)) = conv::decode_all_marked(s, enc, &mut second) else {
        return (None, Err::WrongCharset);
    };
    let Some(mut encoder) = conv::encoder(enc) else {
        return (None, Err::WrongCharset);
    };
    let mut out = Vec::new();
    // (php converts one character at a time: glibc keeps the second
    // character of a sequence decoding to two for the flush after the last
    // one taken, which php then takes too)
    let mut take = len as usize;
    let end = offset as usize + take;
    if end < chars.len() && second[end] {
        take += 1;
    }
    for &cp in chars.iter().skip(offset as usize).take(take) {
        if !encoder.encode(cp, &mut out) {
            break;
        }
    }
    out.extend_from_slice(&encoder.reset_bytes());
    (Some(out), Err::Success)
}

fn iconv_substr(vm: &mut Vm, args: &[Value]) -> R {
    let s = str_arg(args, 0);
    let offset = arg(args, 1).to_long();
    let length = match arg(args, 2) {
        Value::Undef | Value::Null => s.len() as i64,
        v => v.to_long(),
    };
    let Ok(cs) = charset_arg(vm, args, 3) else {
        return Ok(Value::False);
    };
    let (r, err) = iconv_substr_impl(&s, offset, length, &cs);
    show_error(vm, err, UCS4, &cs);
    Ok(match r {
        Some(r) if err == Err::Success => sval(r),
        _ => Value::False,
    })
}

/// _php_iconv_strpos(): the position of the (first or last) match.
fn iconv_strpos_impl(
    haystack: &[u8],
    needle: &[u8],
    offset: usize,
    enc: &[u8],
    reverse: bool,
) -> (Option<usize>, Err) {
    let (ndl, err) = iconv_string(needle, UCS4, enc);
    if err != Err::Success {
        return (None, err);
    }
    let ndl: Vec<u32> = ndl
        .unwrap_or_default()
        .chunks(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let Some((hay, hay_err)) = conv::decode_all(haystack, enc) else {
        return (None, Err::WrongCharset);
    };
    // glibc's two-step conversion reports a bad sequence along with the
    // character before it (the C code checks errno each character).
    let mut err = Err::Success;
    let mut ret: Option<usize> = None;
    let mut match_ofs: Option<usize> = None;
    let mut np = 0usize; // ndl_buf_p (in characters)
    let mut cnt = 0usize;
    for &c in &hay {
        if cnt + 1 == hay.len() {
            if let Some(e) = hay_err {
                err = e.into();
            }
        }
        if cnt >= offset {
            if c == ndl[np] {
                if match_ofs.is_none() {
                    match_ofs = Some(cnt);
                }
                np += 1;
                if np == ndl.len() {
                    ret = match_ofs;
                    if reverse {
                        np = 0;
                        match_ofs = None;
                    } else {
                        break;
                    }
                }
            } else {
                // The C code's partial-match fallback.
                let lim = np;
                let mut i = 0usize;
                let mut j = 1usize;
                while j < lim {
                    if ndl[j] == ndl[i] {
                        i += 1;
                    } else {
                        j -= i;
                        i = 0;
                    }
                    j += 1;
                }
                if c == ndl[i] {
                    match_ofs = match_ofs.map(|m| m + (lim - i));
                    i += 1;
                    np = i;
                } else {
                    match_ofs = None;
                    np = 0;
                }
            }
        }
        cnt += 1;
    }
    // (searching forward, cnt stops at the end of the match)
    let cnt_final = if !reverse {
        ret.map(|r| r + ndl.len() - 1).unwrap_or(cnt)
    } else {
        cnt
    };
    if err != Err::Success {
        return (None, err);
    }
    if offset > cnt_final {
        return (None, Err::OutByBounds);
    }
    (ret, Err::Success)
}

fn iconv_strpos(vm: &mut Vm, args: &[Value]) -> R {
    let hay = str_arg(args, 0);
    let ndl = str_arg(args, 1);
    let mut offset = match arg(args, 2) {
        Value::Undef => 0,
        v => v.to_long(),
    };
    let Ok(cs) = charset_arg(vm, args, 3) else {
        return Ok(Value::False);
    };
    if offset < 0 {
        let (n, err) = iconv_strlen_impl(&hay, &cs);
        let Some(n) = n else {
            show_error(vm, err, UCS4, &cs);
            return Ok(Value::False);
        };
        offset += n as i64;
        if offset < 0 {
            let exc = vm.create_exception(
                b"ValueError",
                "iconv_strpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)",
                vm.current_line,
            );
            vm.current_exception = Some(exc);
            return Ok(Value::Null);
        }
    }
    if ndl.is_empty() {
        return Ok(Value::False);
    }
    let (r, err) = iconv_strpos_impl(&hay, &ndl, offset as usize, &cs, false);
    show_error(vm, err, UCS4, &cs);
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    Ok(match r {
        Some(p) if err == Err::Success => Value::Long(p as i64),
        _ => Value::False,
    })
}

fn iconv_strrpos(vm: &mut Vm, args: &[Value]) -> R {
    let hay = str_arg(args, 0);
    let ndl = str_arg(args, 1);
    if ndl.is_empty() {
        return Ok(Value::False);
    }
    let Ok(cs) = charset_arg(vm, args, 2) else {
        return Ok(Value::False);
    };
    let (r, err) = iconv_strpos_impl(&hay, &ndl, 0, &cs, true);
    show_error(vm, err, UCS4, &cs);
    Ok(match r {
        Some(p) if err == Err::Success => Value::Long(p as i64),
        _ => Value::False,
    })
}

fn iconv_fn(vm: &mut Vm, args: &[Value]) -> R {
    let in_cs = str_arg(args, 0);
    let out_cs = str_arg(args, 1);
    let s = str_arg(args, 2);
    if in_cs.len() >= CSNMAXLEN || out_cs.len() >= CSNMAXLEN {
        docref(
            vm,
            Level::Warning,
            &format!(
                "Encoding parameter exceeds the maximum allowed length of {} characters",
                CSNMAXLEN
            ),
        );
        return Ok(Value::False);
    }
    let (out, err) = iconv_string(&s, &out_cs, &in_cs);
    show_error(vm, err, &out_cs, &in_cs);
    Ok(match out {
        Some(o) if err == Err::Success => sval(o),
        _ => Value::False,
    })
}

fn iconv_set_encoding(vm: &mut Vm, args: &[Value]) -> R {
    let ty = str_arg(args, 0);
    let cs = str_arg(args, 1);
    if cs.len() >= CSNMAXLEN {
        docref(
            vm,
            Level::Warning,
            &format!(
                "Encoding parameter exceeds the maximum allowed length of {} characters",
                CSNMAXLEN
            ),
        );
        return Ok(Value::False);
    }
    let key: &[u8] = if ty.eq_ignore_ascii_case(b"input_encoding") {
        b"iconv.input_encoding"
    } else if ty.eq_ignore_ascii_case(b"output_encoding") {
        b"iconv.output_encoding"
    } else if ty.eq_ignore_ascii_case(b"internal_encoding") {
        b"iconv.internal_encoding"
    } else {
        return Ok(Value::False);
    };
    // zend_alter_ini_entry(PHP_INI_STAGE_RUNTIME)
    if let Some(v) = vm.ini_validators.get(key).copied() {
        if !v(vm, &cs, false) {
            return Ok(Value::False);
        }
    }
    if !vm.ini_originals.contains_key(key) {
        let cur = vm.constants.get(key).cloned();
        vm.ini_originals.insert(key.to_vec(), cur);
    }
    vm.constants.insert(key.to_vec(), sval(cs));
    Ok(Value::True)
}

fn iconv_get_encoding(vm: &mut Vm, args: &[Value]) -> R {
    let ty = match arg(args, 0) {
        Value::Undef => b"all".to_vec(),
        v => v.to_php_string().as_bytes().to_vec(),
    };
    if ty.eq_ignore_ascii_case(b"all") {
        let mut arr = PhpArray::new();
        let k = |s: &str| ArrayKey::String(PhpString::from_string(s.to_string()));
        arr.set(k("input_encoding"), sval(input_encoding(vm)));
        arr.set(k("output_encoding"), sval(output_encoding(vm)));
        arr.set(k("internal_encoding"), sval(internal_encoding(vm)));
        return Ok(Value::Array(Rc::new(RefCell::new(arr))));
    }
    Ok(if ty.eq_ignore_ascii_case(b"input_encoding") {
        sval(input_encoding(vm))
    } else if ty.eq_ignore_ascii_case(b"output_encoding") {
        sval(output_encoding(vm))
    } else if ty.eq_ignore_ascii_case(b"internal_encoding") {
        sval(internal_encoding(vm))
    } else {
        Value::False
    })
}

fn iconv_mime_encode(vm: &mut Vm, args: &[Value]) -> R {
    mime::iconv_mime_encode(vm, args)
}

fn iconv_mime_decode(vm: &mut Vm, args: &[Value]) -> R {
    mime::iconv_mime_decode(vm, args)
}

fn iconv_mime_decode_headers(vm: &mut Vm, args: &[Value]) -> R {
    mime::iconv_mime_decode_headers(vm, args)
}

// ---------------------------------------------------------------------------
// ob_iconv_handler

fn ob_handler_init(
    _vm: &mut Vm,
    name: &[u8],
    chunk_size: usize,
    flags: i64,
) -> Option<goro_core::ob::ObHandler> {
    Some(Vm::ob_make_handler(
        name.to_vec(),
        goro_core::ob::ObKind::InternalFallible(ob_handler),
        chunk_size,
        flags,
    ))
}

/// php_iconv_output_conflict()
fn ob_conflict(vm: &mut Vm, name: &[u8]) -> bool {
    if vm.ob_level() > 0 {
        for set in [&b"ob_iconv_handler"[..], b"mb_output_handler"] {
            if handler_conflict(vm, name, set) {
                return false;
            }
        }
    }
    true
}

/// php_output_handler_conflict()
fn handler_conflict(vm: &mut Vm, new: &[u8], set: &[u8]) -> bool {
    if vm.ob_handler_started(set) {
        let msg = if new != set {
            format!(
                "Output handler '{}' conflicts with '{}'",
                goro_core::string::msg_str(new),
                goro_core::string::msg_str(set)
            )
        } else {
            format!(
                "Output handler '{}' cannot be used twice",
                goro_core::string::msg_str(new)
            )
        };
        docref(vm, Level::Warning, &msg);
        return true;
    }
    false
}

/// php_iconv_output_handler(): None is FAILURE (the handler is disabled
/// and the output passes through).
fn ob_handler(vm: &mut Vm, data: &[u8], op: i64) -> Option<Vec<u8>> {
    const START: i64 = 1;
    const CLEAN: i64 = 2;
    const FINAL: i64 = 8;
    if op & START != 0 {
        // (once output was sent, the charset can no longer be declared)
        if vm.headers_sent {
            return None;
        }
        let mimetype = match goro_core::sapi_headers::mimetype() {
            Some(m) if m.len() >= 5 && m[..5].eq_ignore_ascii_case(b"text/") => {
                Some(m[..m.iter().position(|&c| c == b';').unwrap_or(m.len())].to_vec())
            }
            _ if goro_core::sapi_headers::send_default_content_type() => Some(
                vm.constants
                    .get(b"default_mimetype".as_slice())
                    .map(|v| v.to_php_string().as_bytes().to_vec())
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| b"text/html".to_vec()),
            ),
            _ => None,
        };
        if let Some(mt) = mimetype {
            if op & CLEAN == 0 || op & FINAL == 0 {
                let enc = output_encoding(vm);
                let cs = match enc.windows(2).position(|w| w == b"//") {
                    Some(p) => &enc[..p],
                    None => &enc[..],
                };
                let mut line = b"Content-Type:".to_vec();
                line.extend_from_slice(&mt);
                line.extend_from_slice(b"; charset=");
                line.extend_from_slice(cs);
                goro_core::sapi_headers::add_header_checked(vm, &line, false);
            }
        }
    }
    if data.is_empty() {
        return Some(Vec::new());
    }
    let out_cs = output_encoding(vm);
    let in_cs = internal_encoding(vm);
    let (out, err) = iconv_string(data, &out_cs, &in_cs);
    show_error(vm, err, &out_cs, &in_cs);
    Some(out.unwrap_or_default())
}

// ---------------------------------------------------------------------------
// convert.iconv.* stream filter

struct IconvFilter {
    cd: Iconv,
    to: Vec<u8>,
    from: Vec<u8>,
    stub: Vec<u8>,
}

/// php_iconv_stream_filter_factory_create(): "convert.iconv.FROM/TO" or
/// "convert.iconv.FROM.TO".
fn filter_create(
    _vm: &mut Vm,
    name: &[u8],
    _params: Option<&Value>,
) -> Option<Box<dyn goro_ext_standard::filters::ExtFilter>> {
    let p1 = name.iter().position(|&c| c == b'.')?;
    let rest = &name[p1 + 1..];
    let p2 = rest.iter().position(|&c| c == b'.')?;
    let from_start = &rest[p2 + 1..];
    let sep = from_start.iter().position(|&c| c == b'/' || c == b'.')?;
    let from = &from_start[..sep];
    let to = &from_start[sep + 1..];
    if from.len() >= CSNMAXLEN || to.len() >= CSNMAXLEN {
        return None;
    }
    let cd = Iconv::open(to, from)?;
    Some(Box::new(IconvFilter {
        cd,
        to: to.to_vec(),
        from: from.to_vec(),
        stub: Vec::new(),
    }))
}

impl IconvFilter {
    fn warning(&self, what: &str) -> String {
        format!(
            "iconv stream filter (\"{}\"=>\"{}\"): {}",
            String::from_utf8_lossy(&self.from),
            String::from_utf8_lossy(&self.to),
            what
        )
    }

    /// php_iconv_stream_filter_append_bucket(): Err(warning) on failure.
    fn append(&mut self, ps: Option<&[u8]>, out: &mut Vec<u8>) -> Result<usize, String> {
        // (the output buffer starts at the input's size, 64 bytes for the
        // flush, and doubles whenever iconv() finds it full: glibc's
        // stateful encoders show where)
        const STUB: usize = 128;
        let start = out.len();
        let mut size = ps.map_or(64, |d| d.len());
        let room = |out: &Vec<u8>, size: usize| size.saturating_sub(out.len() - start);
        let mut data = ps.unwrap_or(&[]);
        if !self.stub.is_empty() {
            let mut pt = 0;
            while pt < self.stub.len() {
                let stub = self.stub.clone();
                match self.cd.conv(&stub, &mut pt, out, Some(room(out, size))) {
                    Ok(()) => {}
                    Err(Errno::Ilseq) => return Err(self.warning("invalid multibyte sequence")),
                    Err(Errno::Inval) => {
                        if ps.is_none() {
                            return Err(self.warning("invalid multibyte sequence"));
                        }
                        let Some((&b, rest)) = data.split_first() else {
                            // (php then drops the stub)
                            self.stub.clear();
                            pt = 0;
                            break;
                        };
                        if self.stub.len() >= STUB {
                            return Err(self.warning("insufficient buffer"));
                        }
                        self.stub.drain(..pt);
                        self.stub.push(b);
                        data = rest;
                        pt = 0;
                    }
                    Err(Errno::TooBig) => size *= 2,
                }
            }
            self.stub.drain(..pt.min(self.stub.len()));
        }
        let Some(_) = ps else {
            loop {
                match self.cd.flush(out, Some(room(out, size))) {
                    Ok(()) => return Ok(0),
                    Err(Errno::TooBig) => size *= 2,
                    Err(_) => return Err(self.warning("unknown error")),
                }
            }
        };
        let mut pos = 0;
        while pos < data.len() {
            match self.cd.conv(data, &mut pos, out, Some(room(out, size))) {
                Ok(()) => {}
                Err(Errno::Ilseq) => return Err(self.warning("invalid multibyte sequence")),
                Err(Errno::Inval) => {
                    let rest = &data[pos..];
                    if rest.len() > STUB {
                        return Err(self.warning("insufficient buffer"));
                    }
                    self.stub = rest.to_vec();
                    break;
                }
                Err(Errno::TooBig) => size *= 2,
            }
        }
        Ok(ps.map_or(0, |d| d.len()))
    }
}

impl goro_ext_standard::filters::ExtFilter for IconvFilter {
    fn filter(
        &mut self,
        _vm: &mut Vm,
        inp: &[Vec<u8>],
        flags: i32,
    ) -> goro_ext_standard::filters_conv::FilterOutput {
        use goro_ext_standard::filters::{PSFS_ERR_FATAL, PSFS_FLAG_NORMAL, PSFS_PASS_ON};
        let mut r = goro_ext_standard::filters_conv::FilterOutput {
            status: PSFS_PASS_ON,
            out: Vec::new(),
            consumed: 0,
            warnings: Vec::new(),
        };
        for b in inp {
            let mut out = Vec::new();
            match self.append(Some(b), &mut out) {
                Ok(c) => {
                    r.consumed += c;
                    if !out.is_empty() {
                        r.out.push(out);
                    }
                }
                Err(w) => {
                    r.warnings.push(w);
                    r.status = PSFS_ERR_FATAL;
                    r.consumed = 0;
                    return r;
                }
            }
        }
        if flags != PSFS_FLAG_NORMAL {
            let mut out = Vec::new();
            match self.append(None, &mut out) {
                Ok(_) => {
                    if !out.is_empty() {
                        r.out.push(out);
                    }
                }
                Err(w) => {
                    r.warnings.push(w);
                    r.status = PSFS_ERR_FATAL;
                    r.consumed = 0;
                    return r;
                }
            }
        }
        r
    }
}
