//! iconv_mime_encode(), iconv_mime_decode() and
//! iconv_mime_decode_headers() (_php_iconv_mime_encode() /
//! _php_iconv_mime_decode()).

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::conv::{Errno, Iconv};
use crate::{CSNMAXLEN, Err, Level, R, docref, internal_encoding, show_error};

const ASCII: &[u8] = b"ASCII";
const MODE_STRICT: i64 = 1;
const MODE_CONTINUE_ON_ERROR: i64 = 2;

/// _php_iconv_appendl(): convert all of `s` (None: the shift-back
/// sequence) appending to `d`.
fn appendl(d: &mut Vec<u8>, s: Option<&[u8]>, cd: &mut Iconv) -> Err {
    // (smart_str_alloc() happens before the conversion: the buffer exists
    // even when it fails, which iconv_mime_decode_headers() tests through
    // the capacity)
    if s.is_none_or(|s| !s.is_empty()) {
        d.reserve(1);
    }
    match s {
        Some(s) => {
            // The output of the iconv() call that fails is not kept (the
            // smart_str length is only advanced after a successful or
            // E2BIG call, in 128, 256, ... byte steps).
            let mut pos = 0;
            let mut growth = 128usize;
            while pos < s.len() {
                let before = d.len();
                match cd.conv(s, &mut pos, d, Some(growth)) {
                    Ok(()) | Err(Errno::TooBig) => {}
                    Err(e) => {
                        d.truncate(before);
                        return if e == Errno::Inval {
                            Err::IllegalChar
                        } else {
                            Err::IllegalSeq
                        };
                    }
                }
                growth <<= 1;
            }
            Err::Success
        }
        None => match cd.flush(d, None) {
            Ok(()) => Err::Success,
            Err(_) => Err::Unknown(7),
        },
    }
}

fn appendc(d: &mut Vec<u8>, c: u8, cd: &mut Iconv) -> Err {
    appendl(d, Some(&[c]), cd)
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len().div_ceil(3) * 4);
    for c in s.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        out.push(BASE64[(b[0] >> 2) as usize]);
        out.push(BASE64[(((b[0] & 3) << 4) | (b[1] >> 4)) as usize]);
        out.push(if c.len() > 1 {
            BASE64[(((b[1] & 0xF) << 2) | (b[2] >> 6)) as usize]
        } else {
            b'='
        });
        out.push(if c.len() > 2 {
            BASE64[(b[2] & 0x3F) as usize]
        } else {
            b'='
        });
    }
    out
}

/// php_base64_decode() (non-strict: unknown characters are skipped).
fn base64_decode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4 + 1);
    let mut i = 0usize;
    let mut cur: u8 = 0;
    for &c in s {
        if c == b'=' {
            continue;
        }
        let Some(v) = BASE64.iter().position(|&x| x == c) else {
            continue;
        };
        let v = v as u8;
        match i % 4 {
            0 => cur = v << 2,
            1 => {
                out.push(cur | (v >> 4));
                cur = (v & 0x0f) << 4;
            }
            2 => {
                out.push(cur | (v >> 2));
                cur = (v & 0x03) << 6;
            }
            _ => out.push(cur | v),
        }
        i += 1;
    }
    out
}

fn hexval(c: u8) -> u32 {
    match c {
        b'0'..=b'9' => (c - b'0') as u32,
        b'A'..=b'F' => (c - b'A' + 10) as u32,
        b'a'..=b'f' => (c - b'a' + 10) as u32,
        b'\t' | b' ' => 32,
        b'\n' | b'\r' => 16,
        _ => 64,
    }
}

/// php_quot_print_decode(str, len, replace_us_by_ws = 1)
fn quot_print_decode(s: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = s.len();
    let mut p = 0usize;
    let at = |p: usize| s.get(p).copied().unwrap_or(0);
    while i > 0 && at(p) != 0 {
        if at(p) == b'=' {
            i -= 1;
            p += 1;
            if i == 0 || at(p) == 0 {
                break;
            }
            let mut h = hexval(at(p));
            if h < 16 {
                i -= 1;
                if i == 0 {
                    return None;
                }
                p += 1;
                let l = hexval(at(p));
                if l >= 16 {
                    return None;
                }
                out.push(((h << 4) | l) as u8);
                i -= 1;
                p += 1;
            } else if h < 64 {
                while h == 32 {
                    i -= 1;
                    if i == 0 {
                        return None;
                    }
                    p += 1;
                    h = hexval(at(p));
                    if h == 64 {
                        return None;
                    }
                }
                if at(p) == b'\r' && i >= 2 && at(p + 1) == b'\n' {
                    i -= 1;
                    p += 1;
                }
                i -= 1;
                p += 1;
            } else {
                return None;
            }
        } else {
            out.push(if at(p) == b'_' { b' ' } else { at(p) });
            i -= 1;
            p += 1;
        }
    }
    Some(out)
}

const QP_TABLE: [usize; 256] = {
    let mut t = [3usize; 256];
    let mut c = 0x21;
    while c < 0x7f {
        t[c] = 1;
        c += 1;
    }
    t[0x3d] = 3; // =
    t[0x3f] = 3; // ?
    t[0x5f] = 3; // _
    t
};

#[derive(Clone, Copy, PartialEq)]
enum Scheme {
    Base64,
    Qprint,
}

/// _php_iconv_mime_encode()
fn mime_encode(
    out: &mut Vec<u8>,
    fname: &[u8],
    fval: &[u8],
    max_line_len: usize,
    lfchars: &[u8],
    scheme: Scheme,
    out_charset: &[u8],
    enc: &[u8],
) -> Err {
    let out_charset_len = out_charset.len();
    if fname.len() + 2 >= max_line_len || out_charset_len + 12 >= max_line_len {
        return Err::TooBig;
    }
    let Some(mut cd_pl) = Iconv::open(ASCII, enc) else {
        return Err::WrongCharset;
    };
    let Some(mut cd) = Iconv::open(out_charset, enc) else {
        return Err::WrongCharset;
    };
    let mut char_cnt = max_line_len as i64;
    let _ = appendl(out, Some(fname), &mut cd_pl);
    char_cnt -= fname.len() as i64;
    out.extend_from_slice(b": ");
    char_cnt -= 2;

    let mut in_pos = 0usize;
    let in_len = fval.len();
    loop {
        let encoded_word_min_len =
            7 + out_charset_len as i64 + if scheme == Scheme::Base64 { 4 } else { 3 };
        if char_cnt < encoded_word_min_len + lfchars.len() as i64 + 1 {
            out.extend_from_slice(lfchars);
            out.push(b' ');
            char_cnt = max_line_len as i64 - 1;
        }
        out.extend_from_slice(b"=?");
        char_cnt -= 2;
        out.extend_from_slice(out_charset);
        char_cnt -= out_charset_len as i64;
        out.push(b'?');
        char_cnt -= 1;
        match scheme {
            Scheme::Base64 => {
                out.push(b'B');
                char_cnt -= 1;
                out.push(b'?');
                char_cnt -= 1;
                let prev_in_pos = in_pos;
                let ini_in_pos = in_pos;
                let mut out_reserved = 4i64;
                let out_size = (char_cnt - 2) / 4 * 3;
                let mut buf: Vec<u8>;
                loop {
                    buf = Vec::new();
                    if out_size <= out_reserved {
                        return Err::TooBig;
                    }
                    let out_left = (out_size - out_reserved) as usize;
                    match cd.conv(fval, &mut in_pos, &mut buf, Some(out_left)) {
                        Ok(()) => {}
                        Err(Errno::Inval) => return Err::IllegalChar,
                        Err(Errno::Ilseq) => return Err::IllegalSeq,
                        Err(Errno::TooBig) => {
                            if prev_in_pos == in_pos {
                                return Err::TooBig;
                            }
                        }
                    }
                    let left = out_size as usize - buf.len();
                    if cd.flush(&mut buf, Some(left)).is_ok() {
                        break;
                    }
                    cd.reset();
                    out_reserved += 4;
                    in_pos = ini_in_pos;
                }
                let encoded = base64_encode(&buf);
                if char_cnt < encoded.len() as i64 {
                    return Err::Unknown(crate::conv::last_errno());
                }
                out.extend_from_slice(&encoded);
                char_cnt -= encoded.len() as i64;
                out.extend_from_slice(b"?=");
                char_cnt -= 2;
            }
            Scheme::Qprint => {
                out.push(b'Q');
                char_cnt -= 1;
                out.push(b'?');
                char_cnt -= 1;
                let prev_in_pos = in_pos;
                let ini_in_pos = in_pos;
                let mut out_size = char_cnt - 2;
                let mut buf: Vec<u8> = Vec::new();
                while out_size > 0 {
                    buf = Vec::new();
                    match cd.conv(fval, &mut in_pos, &mut buf, Some(out_size as usize)) {
                        Ok(()) => {}
                        Err(Errno::Inval) => return Err::IllegalChar,
                        Err(Errno::Ilseq) => return Err::IllegalSeq,
                        Err(Errno::TooBig) => {
                            if prev_in_pos == in_pos {
                                return Err::Unknown(7);
                            }
                        }
                    }
                    let left = out_size as usize - buf.len();
                    let _ = cd.flush(&mut buf, Some(left));
                    let nbytes_required: usize = buf.iter().map(|&b| QP_TABLE[b as usize]).sum();
                    if nbytes_required as i64 <= char_cnt - 2 {
                        break;
                    }
                    out_size -= ((nbytes_required as i64 - (char_cnt - 2)) + 2) / 3;
                    in_pos = ini_in_pos;
                }
                const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
                for &b in &buf {
                    if QP_TABLE[b as usize] == 1 {
                        out.push(b);
                        char_cnt -= 1;
                    } else {
                        out.push(b'=');
                        out.push(DIGITS[(b >> 4) as usize]);
                        out.push(DIGITS[(b & 0x0f) as usize]);
                        char_cnt -= 3;
                    }
                }
                out.extend_from_slice(b"?=");
                char_cnt -= 2;
                cd.reset();
            }
        }
        if in_pos >= in_len {
            break;
        }
    }
    Err::Success
}

fn arg(args: &[Value], i: usize) -> Value {
    match args.get(i) {
        Some(v) => v.deref(),
        None => Value::Undef,
    }
}

fn too_long(vm: &mut Vm) {
    docref(
        vm,
        Level::Warning,
        &format!(
            "Encoding parameter exceeds the maximum allowed length of {} characters",
            CSNMAXLEN
        ),
    );
}

pub(crate) fn iconv_mime_encode(vm: &mut Vm, args: &[Value]) -> R {
    let field_name = arg(args, 0).to_php_string().as_bytes().to_vec();
    let field_value = arg(args, 1).to_php_string().as_bytes().to_vec();
    let mut in_charset = internal_encoding(vm);
    let mut out_charset = in_charset.clone();
    let mut line_len: i64 = 76;
    let mut lfchars: Vec<u8> = b"\r\n".to_vec();
    let mut scheme = Scheme::Base64;
    if let Value::Array(pref) = arg(args, 2) {
        let pref = pref.borrow();
        let get = |k: &str| pref.get_str(k.as_bytes()).map(|v| v.deref());
        if let Some(Value::String(s)) = get("scheme") {
            match s.as_bytes().first() {
                Some(b'B' | b'b') => scheme = Scheme::Base64,
                Some(b'Q' | b'q') => scheme = Scheme::Qprint,
                _ => {}
            }
        }
        if let Some(Value::String(s)) = get("input-charset") {
            if s.as_bytes().len() >= CSNMAXLEN {
                drop(pref);
                too_long(vm);
                return Ok(Value::False);
            }
            if !s.as_bytes().is_empty() {
                in_charset = s.as_bytes().to_vec();
            }
        }
        if let Some(Value::String(s)) = get("output-charset") {
            if s.as_bytes().len() >= CSNMAXLEN {
                drop(pref);
                too_long(vm);
                return Ok(Value::False);
            }
            if !s.as_bytes().is_empty() {
                out_charset = s.as_bytes().to_vec();
            }
        }
        if let Some(v) = get("line-length") {
            line_len = v.to_long();
        }
        if let Some(v) = get("line-break-chars") {
            let v = v.clone();
            drop(pref);
            let s = vm.value_to_string(&v);
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            // (a C string: up to the first NUL byte)
            let b = s.as_bytes();
            lfchars = b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())].to_vec();
        }
    }
    let mut out = Vec::new();
    let err = mime_encode(
        &mut out,
        &field_name,
        &field_value,
        line_len.max(0) as usize,
        &lfchars,
        scheme,
        &out_charset,
        &in_charset,
    );
    show_error(vm, err, &out_charset, &in_charset);
    Ok(if err == Err::Success {
        Value::String(PhpString::from_vec(out))
    } else {
        Value::False
    })
}

/// _php_iconv_mime_decode(): Err, and the position where decoding
/// stopped (the next header).
fn mime_decode(out: &mut Vec<u8>, s: &[u8], enc: &[u8], mode: i64) -> (Err, usize) {
    let Some(mut cd_pl) = Iconv::open(enc, ASCII) else {
        return (Err::WrongCharset, 0);
    };
    let mut cd: Option<Iconv> = None;
    let strict = mode & MODE_STRICT != 0;
    let cont = mode & MODE_CONTINUE_ON_ERROR != 0;
    let at = |p: isize| -> u8 {
        if p < 0 {
            0
        } else {
            s.get(p as usize).copied().unwrap_or(0)
        }
    };
    let mut scan_stat = 0u32;
    let mut csname: Option<isize> = None;
    let mut encoded_text: isize = 0;
    let mut encoded_text_len: usize = 0;
    let mut encoded_word: Option<isize> = None;
    let mut spaces: Option<isize> = None;
    let mut enc_scheme = Scheme::Base64;
    let mut p1: isize = 0;
    let mut str_left = s.len();
    macro_rules! seg {
        ($from:expr, $to:expr) => {
            &s[($from as usize)..(($to as usize).min(s.len()))]
        };
    }
    macro_rules! fail {
        ($e:expr) => {
            return ($e, 0)
        };
    }
    while str_left > 0 {
        let mut eos = false;
        let c = at(p1);
        match scan_stat {
            0 => match c {
                b'\r' => scan_stat = 7,
                b'\n' => scan_stat = 8,
                b'=' => {
                    encoded_word = Some(p1);
                    scan_stat = 1;
                }
                b' ' | b'\t' => {
                    spaces = Some(p1);
                    scan_stat = 11;
                }
                _ => {
                    let e = appendc(out, c, &mut cd_pl);
                    if e != Err::Success && !cont {
                        fail!(e);
                    }
                    encoded_word = None;
                    if strict {
                        scan_stat = 12;
                    }
                }
            },
            1 => {
                if c != b'?' {
                    if c == b'\r' || c == b'\n' {
                        p1 -= 1;
                    }
                    let ew = encoded_word.unwrap_or(p1);
                    let e = appendl(out, Some(seg!(ew, p1 + 1)), &mut cd_pl);
                    if e != Err::Success {
                        fail!(e);
                    }
                    encoded_word = None;
                    scan_stat = if strict { 12 } else { 0 };
                } else {
                    csname = Some(p1 + 1);
                    scan_stat = 2;
                }
            }
            2 => {
                match c {
                    b'?' => scan_stat = 3,
                    b'*' => scan_stat = 10,
                    b'\r' | b'\n' => {
                        p1 -= 1;
                        appendc(out, b'=', &mut cd_pl);
                        appendc(out, b'?', &mut cd_pl);
                        let cs = csname.unwrap_or(p1);
                        let e = appendl(out, Some(seg!(cs, p1 + 1)), &mut cd_pl);
                        if e != Err::Success {
                            fail!(e);
                        }
                        csname = None;
                        scan_stat = if strict { 12 } else { 0 };
                        str_left -= 1;
                        p1 += 1;
                        continue;
                    }
                    _ => {}
                }
                if scan_stat != 2 {
                    let Some(cs) = csname else {
                        fail!(Err::Malformed);
                    };
                    let csname_len = (p1 - cs) as usize;
                    if csname_len > 79 {
                        if cont {
                            let ew = encoded_word.unwrap_or(p1);
                            let e = appendl(out, Some(seg!(ew, p1 + 1)), &mut cd_pl);
                            if e != Err::Success {
                                fail!(e);
                            }
                            encoded_word = None;
                            scan_stat = if strict { 12 } else { 0 };
                        } else {
                            fail!(Err::Malformed);
                        }
                    } else {
                        let name = seg!(cs, p1).to_vec();
                        cd = Iconv::open(enc, &name);
                        if cd.is_none() {
                            if cont {
                                let mut qmarks = 2;
                                while qmarks > 0 && str_left > 1 {
                                    p1 += 1;
                                    if at(p1) == b'?' {
                                        qmarks -= 1;
                                    }
                                    str_left -= 1;
                                }
                                if at(p1 + 1) == b'=' {
                                    p1 += 1;
                                    if str_left > 1 {
                                        str_left -= 1;
                                    }
                                }
                                let ew = encoded_word.unwrap_or(p1);
                                let e = appendl(out, Some(seg!(ew, p1 + 1)), &mut cd_pl);
                                if e != Err::Success {
                                    fail!(e);
                                }
                                scan_stat = 12;
                            } else {
                                fail!(Err::WrongCharset);
                            }
                        }
                    }
                }
            }
            3 => match c {
                b'b' | b'B' => {
                    enc_scheme = Scheme::Base64;
                    scan_stat = 4;
                }
                b'q' | b'Q' => {
                    enc_scheme = Scheme::Qprint;
                    scan_stat = 4;
                }
                _ => {
                    if cont {
                        let ew = encoded_word.unwrap_or(p1);
                        let e = appendl(out, Some(seg!(ew, p1 + 1)), &mut cd_pl);
                        if e != Err::Success {
                            fail!(e);
                        }
                        encoded_word = None;
                        scan_stat = if strict { 12 } else { 0 };
                    } else {
                        fail!(Err::Malformed);
                    }
                }
            },
            4 => {
                if c != b'?' {
                    if cont {
                        let ew = encoded_word.unwrap_or(p1);
                        let e = appendl(out, Some(seg!(ew, p1 + 1)), &mut cd_pl);
                        if e != Err::Success {
                            fail!(e);
                        }
                        encoded_word = None;
                        scan_stat = if strict { 12 } else { 0 };
                    } else {
                        fail!(Err::Malformed);
                    }
                } else {
                    encoded_text = p1 + 1;
                    scan_stat = 5;
                }
            }
            5 => {
                if c == b'?' {
                    encoded_text_len = (p1 - encoded_text) as usize;
                    scan_stat = 6;
                }
            }
            7 => {
                if c == b'\n' {
                    scan_stat = 8;
                } else {
                    appendc(out, b'\r', &mut cd_pl);
                    appendc(out, c, &mut cd_pl);
                    scan_stat = 0;
                }
            }
            8 => {
                if c != b' ' && c != b'\t' {
                    p1 -= 1;
                    str_left = 1;
                } else {
                    if encoded_word.is_none() {
                        appendc(out, b' ', &mut cd_pl);
                    }
                    spaces = None;
                    scan_stat = 11;
                }
            }
            6 | 9 => {
                let mut go9 = scan_stat == 9;
                if scan_stat == 6 {
                    if c != b'=' {
                        if cont {
                            let ew = encoded_word.unwrap_or(p1);
                            let e = appendl(out, Some(seg!(ew, p1 + 1)), &mut cd_pl);
                            if e != Err::Success {
                                fail!(e);
                            }
                            encoded_word = None;
                            scan_stat = if strict { 12 } else { 0 };
                        } else {
                            fail!(Err::Malformed);
                        }
                    } else {
                        scan_stat = 9;
                        if str_left == 1 {
                            eos = true;
                            go9 = true;
                        }
                    }
                }
                if go9 {
                    let c = at(p1);
                    let mut decode = true;
                    if !matches!(c, b'\r' | b'\n' | b' ' | b'\t') && !eos && strict {
                        let ew = encoded_word.unwrap_or(p1);
                        let e = appendl(out, Some(seg!(ew, p1 + 1)), &mut cd_pl);
                        if e != Err::Success {
                            fail!(e);
                        }
                        scan_stat = 12;
                        decode = false;
                    }
                    if decode {
                        let text = seg!(encoded_text, encoded_text as usize + encoded_text_len);
                        let decoded = match enc_scheme {
                            Scheme::Base64 => Some(base64_decode(text)),
                            Scheme::Qprint => quot_print_decode(text),
                        };
                        let mut skip_rest = false;
                        match decoded {
                            None => {
                                if cont {
                                    let ew = encoded_word.unwrap_or(p1);
                                    let e = appendl(out, Some(seg!(ew, p1 + 1)), &mut cd_pl);
                                    if e != Err::Success {
                                        fail!(e);
                                    }
                                    encoded_word = None;
                                    scan_stat = if strict { 12 } else { 0 };
                                    skip_rest = true;
                                } else {
                                    fail!(Err::Unknown(crate::conv::last_errno()));
                                }
                            }
                            Some(d) => {
                                let mut e = Err::Unknown(crate::conv::last_errno());
                                if let Some(cdv) = cd.as_mut() {
                                    e = appendl(out, Some(&d), cdv);
                                    if e == Err::Success {
                                        e = appendl(out, None, cdv);
                                    }
                                }
                                if e != Err::Success {
                                    if cont {
                                        let ew = encoded_word.unwrap_or(p1);
                                        let e2 = appendl(out, Some(seg!(ew, p1)), &mut cd_pl);
                                        encoded_word = None;
                                        if e2 != Err::Success {
                                            skip_rest = true;
                                        }
                                    } else {
                                        fail!(e);
                                    }
                                }
                            }
                        }
                        if !skip_rest {
                            if eos {
                                scan_stat = 0;
                            } else {
                                match c {
                                    b'\r' => scan_stat = 7,
                                    b'\n' => scan_stat = 8,
                                    b'=' => scan_stat = 1,
                                    b' ' | b'\t' => {
                                        spaces = Some(p1);
                                        scan_stat = 11;
                                    }
                                    _ => {
                                        appendc(out, c, &mut cd_pl);
                                        scan_stat = 12;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            10 => {
                if c == b'?' {
                    scan_stat = 3;
                }
            }
            11 => match c {
                b'\r' => scan_stat = 7,
                b'\n' => scan_stat = 8,
                b'=' => {
                    if let (Some(sp), None) = (spaces, encoded_word) {
                        appendl(out, Some(seg!(sp, p1)), &mut cd_pl);
                        spaces = None;
                    }
                    encoded_word = Some(p1);
                    scan_stat = 1;
                }
                b' ' | b'\t' => {}
                _ => {
                    if let Some(sp) = spaces {
                        appendl(out, Some(seg!(sp, p1)), &mut cd_pl);
                        spaces = None;
                    }
                    appendc(out, c, &mut cd_pl);
                    encoded_word = None;
                    scan_stat = if strict { 12 } else { 0 };
                }
            },
            _ => match c {
                // 12
                b'\r' => scan_stat = 7,
                b'\n' => scan_stat = 8,
                b' ' | b'\t' => {
                    spaces = Some(p1);
                    scan_stat = 11;
                }
                b'=' if !strict => {
                    encoded_word = Some(p1);
                    scan_stat = 1;
                }
                _ => {
                    appendc(out, c, &mut cd_pl);
                }
            },
        }
        str_left -= 1;
        p1 += 1;
    }
    match scan_stat {
        0 | 8 | 11 | 12 => {}
        _ => {
            if cont {
                if scan_stat == 1 {
                    appendc(out, b'=', &mut cd_pl);
                }
            } else {
                return (Err::Malformed, 0);
            }
        }
    }
    (Err::Success, p1.max(0) as usize)
}

fn mode_and_charset(vm: &mut Vm, args: &[Value]) -> Result<(i64, Vec<u8>), ()> {
    let mode = match arg(args, 1) {
        Value::Undef => 0,
        v => v.to_long(),
    };
    let cs = match arg(args, 2) {
        Value::Undef | Value::Null => internal_encoding(vm),
        v => {
            let s = v.to_php_string().as_bytes().to_vec();
            if s.len() >= CSNMAXLEN {
                too_long(vm);
                return Err(());
            }
            s
        }
    };
    Ok((mode, cs))
}

pub(crate) fn iconv_mime_decode(vm: &mut Vm, args: &[Value]) -> R {
    let s = arg(args, 0).to_php_string().as_bytes().to_vec();
    let Ok((mode, cs)) = mode_and_charset(vm, args) else {
        return Ok(Value::False);
    };
    let mut out = Vec::new();
    let (err, _) = mime_decode(&mut out, &s, &cs, mode);
    show_error(vm, err, &cs, b"???");
    Ok(if err == Err::Success {
        Value::String(PhpString::from_vec(out))
    } else {
        Value::False
    })
}

pub(crate) fn iconv_mime_decode_headers(vm: &mut Vm, args: &[Value]) -> R {
    let s = arg(args, 0).to_php_string().as_bytes().to_vec();
    let Ok((mode, cs)) = mode_and_charset(vm, args) else {
        return Ok(Value::False);
    };
    let mut ret = PhpArray::new();
    let mut err = Err::Success;
    let mut pos = 0usize;
    while pos < s.len() {
        let mut decoded = Vec::new();
        let (e, next) = mime_decode(&mut decoded, &s[pos..], &cs, mode);
        if e != Err::Success {
            err = e;
            break;
        }
        // (decoded_header.s == NULL: nothing was ever appended)
        if decoded.capacity() == 0 {
            break;
        }
        if let Some(colon) = decoded.iter().position(|&c| c == b':') {
            let name = decoded[..colon].to_vec();
            let mut p = colon + 1;
            while p < decoded.len() && (decoded[p] == b' ' || decoded[p] == b'\t') {
                p += 1;
            }
            let value = Value::String(PhpString::from_vec(decoded[p..].to_vec()));
            let key = ArrayKey::String(PhpString::from_vec(name));
            match ret.get(&key).cloned() {
                Some(Value::Array(a)) => {
                    a.borrow_mut().push(value);
                }
                Some(existing) => {
                    let mut arr = PhpArray::new();
                    arr.push(existing);
                    arr.push(value);
                    ret.set(key, Value::Array(Rc::new(RefCell::new(arr))));
                }
                None => ret.set(key, value),
            }
        }
        if next == 0 {
            break;
        }
        pos += next;
    }
    if err != Err::Success {
        show_error(vm, err, &cs, b"???");
        return Ok(Value::False);
    }
    Ok(Value::Array(Rc::new(RefCell::new(ret))))
}
