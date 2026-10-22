//! The sanitizing filters (sanitizing_filters.c).

use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::*;

/// php_filter_encode_html(): `&#N;` for the bytes marked in `chars`.
fn encode_html(s: &[u8], chars: &[bool; 256]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for &c in s {
        if chars[c as usize] {
            out.extend_from_slice(format!("&#{};", c).as_bytes());
        } else {
            out.push(c);
        }
    }
    out
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// php_filter_encode_url(): %XX for everything but `chars`.
fn encode_url(s: &[u8], keep: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3);
    for &c in s {
        // (byte 255 is never encoded: memset(tmp, 1, sizeof(tmp)-1))
        if keep.contains(&c) || c == 255 {
            out.push(c);
        } else {
            out.push(b'%');
            out.push(HEX[(c >> 4) as usize]);
            out.push(HEX[(c & 15) as usize]);
        }
    }
    out
}

/// php_filter_strip()
fn strip(s: Vec<u8>, flags: i64) -> Vec<u8> {
    if flags & (FILTER_FLAG_STRIP_LOW | FILTER_FLAG_STRIP_HIGH | FILTER_FLAG_STRIP_BACKTICK) == 0 {
        return s;
    }
    s.into_iter()
        .filter(|&c| {
            !((c >= 127 && flags & FILTER_FLAG_STRIP_HIGH != 0)
                || (c < 32 && flags & FILTER_FLAG_STRIP_LOW != 0)
                || (c == b'`' && flags & FILTER_FLAG_STRIP_BACKTICK != 0))
        })
        .collect()
}

/// filter_map_apply(): keep only the bytes in `allowed`.
fn map_apply(s: &[u8], allowed: &[u8]) -> Vec<u8> {
    s.iter().copied().filter(|c| allowed.contains(c)).collect()
}

const LOWALPHA: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
const HIALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGIT: &[u8] = b"0123456789";

fn concat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

pub(crate) fn filter_string(_vm: &mut Vm, value: &mut Value, ctx: &Ctx) -> bool {
    let flags = ctx.flags;
    let s = strip(bytes(value), flags);
    let mut enc = [false; 256];
    if flags & FILTER_FLAG_NO_ENCODE_QUOTES == 0 {
        enc[b'\'' as usize] = true;
        enc[b'"' as usize] = true;
    }
    if flags & FILTER_FLAG_ENCODE_AMP != 0 {
        enc[b'&' as usize] = true;
    }
    if flags & FILTER_FLAG_ENCODE_LOW != 0 {
        enc[..32].iter_mut().for_each(|e| *e = true);
    }
    if flags & FILTER_FLAG_ENCODE_HIGH != 0 {
        enc[127..].iter_mut().for_each(|e| *e = true);
    }
    let s = if s.is_empty() {
        s
    } else {
        encode_html(&s, &enc)
    };
    let s = strip_tags(&s);
    *value = if s.is_empty() {
        if flags & FILTER_FLAG_EMPTY_STRING_NULL != 0 {
            Value::Null
        } else {
            sval(Vec::new())
        }
    } else {
        sval(s)
    };
    true
}

pub(crate) fn filter_encoded(_vm: &mut Vm, value: &mut Value, ctx: &Ctx) -> bool {
    let s = strip(bytes(value), ctx.flags);
    let keep = concat(&[LOWALPHA, HIALPHA, DIGIT, b"-._"]);
    *value = sval(encode_url(&s, &keep));
    true
}

pub(crate) fn filter_special_chars(_vm: &mut Vm, value: &mut Value, ctx: &Ctx) -> bool {
    let s = strip(bytes(value), ctx.flags);
    let mut enc = [false; 256];
    for c in [b'\'', b'"', b'<', b'>', b'&', 0] {
        enc[c as usize] = true;
    }
    enc[..32].iter_mut().for_each(|e| *e = true);
    if ctx.flags & FILTER_FLAG_ENCODE_HIGH != 0 {
        enc[127..].iter_mut().for_each(|e| *e = true);
    }
    *value = sval(encode_html(&s, &enc));
    true
}

pub(crate) fn filter_full_special_chars(vm: &mut Vm, value: &mut Value, ctx: &Ctx) -> bool {
    // php_escape_html_entities_ex(all = 1, ENT_QUOTES / ENT_NOQUOTES,
    // default charset, double_encode = 0)
    let quotes = if ctx.flags & FILTER_FLAG_NO_ENCODE_QUOTES == 0 {
        3
    } else {
        0
    };
    let s = value.clone();
    if let Some(f) = vm.functions.get(b"htmlentities".as_slice()).copied() {
        if let Ok(r) = f(vm, &[s, Value::Long(quotes), Value::Null, Value::False]) {
            *value = r;
        }
    }
    true
}

pub(crate) fn filter_unsafe_raw(_vm: &mut Vm, value: &mut Value, ctx: &Ctx) -> bool {
    let flags = ctx.flags;
    let s = bytes(value);
    if flags != 0 && !s.is_empty() {
        let s = strip(s, flags);
        let mut enc = [false; 256];
        if flags & FILTER_FLAG_ENCODE_AMP != 0 {
            enc[b'&' as usize] = true;
        }
        if flags & FILTER_FLAG_ENCODE_LOW != 0 {
            enc[..32].iter_mut().for_each(|e| *e = true);
        }
        if flags & FILTER_FLAG_ENCODE_HIGH != 0 {
            enc[127..].iter_mut().for_each(|e| *e = true);
        }
        *value = sval(if s.is_empty() {
            s
        } else {
            encode_html(&s, &enc)
        });
    } else if flags & FILTER_FLAG_EMPTY_STRING_NULL != 0 && s.is_empty() {
        *value = Value::Null;
    }
    true
}

pub(crate) fn filter_email(_vm: &mut Vm, value: &mut Value, _ctx: &Ctx) -> bool {
    let allowed = concat(&[LOWALPHA, HIALPHA, DIGIT, b"!#$%&'*+-=?^_`{|}~@.[]"]);
    *value = sval(map_apply(&bytes(value), &allowed));
    true
}

pub(crate) fn filter_url(_vm: &mut Vm, value: &mut Value, _ctx: &Ctx) -> bool {
    let allowed = concat(&[
        LOWALPHA,
        HIALPHA,
        DIGIT,
        b"$-_.+",
        b"!*'(),",
        b"{}|\\^~[]`",
        b"<>#%\"",
        b";/?:@&=",
    ]);
    *value = sval(map_apply(&bytes(value), &allowed));
    true
}

pub(crate) fn filter_number_int(_vm: &mut Vm, value: &mut Value, _ctx: &Ctx) -> bool {
    *value = sval(map_apply(&bytes(value), b"+-0123456789"));
    true
}

pub(crate) fn filter_number_float(_vm: &mut Vm, value: &mut Value, ctx: &Ctx) -> bool {
    let mut allowed = b"+-0123456789".to_vec();
    if ctx.flags & FILTER_FLAG_ALLOW_FRACTION != 0 {
        allowed.push(b'.');
    }
    if ctx.flags & FILTER_FLAG_ALLOW_THOUSAND != 0 {
        allowed.push(b',');
    }
    if ctx.flags & FILTER_FLAG_ALLOW_SCIENTIFIC != 0 {
        allowed.extend_from_slice(b"eE");
    }
    *value = sval(map_apply(&bytes(value), &allowed));
    true
}

pub(crate) fn filter_add_slashes(_vm: &mut Vm, value: &mut Value, _ctx: &Ctx) -> bool {
    let s = bytes(value);
    let mut out = Vec::with_capacity(s.len());
    for c in s {
        match c {
            0 => out.extend_from_slice(b"\\0"),
            b'\'' | b'"' | b'\\' => {
                out.push(b'\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    *value = sval(out);
    true
}

/// php_strip_tags_ex(allow = NULL, allow_tag_spaces = 1)
fn strip_tags(buf: &[u8]) -> Vec<u8> {
    let end = buf.len();
    let at = |i: usize| buf.get(i).copied().unwrap_or(0);
    let mut out = Vec::with_capacity(end);
    let mut lc: u8 = 0;
    let mut br: i32 = 0;
    let mut depth: i32 = 0;
    let mut in_q: u8 = 0;
    let mut is_xml = false;
    let mut p = 0usize;
    let mut state = 0u8;
    while p < end {
        let c = buf[p];
        match state {
            0 => match c {
                0 => {}
                b'<' => {
                    if in_q == 0 {
                        lc = b'<';
                        state = 1;
                    }
                }
                b'>' => {
                    if depth > 0 {
                        depth -= 1;
                    } else if in_q == 0 {
                        out.push(c);
                    }
                }
                _ => out.push(c),
            },
            1 => match c {
                0 => {}
                b'<' => {
                    if in_q == 0 {
                        depth += 1;
                    }
                }
                b'>' => {
                    if depth > 0 {
                        depth -= 1;
                    } else if in_q == 0 {
                        lc = b'>';
                        if !(is_xml && p >= 1 && buf[p - 1] == b'-') {
                            in_q = 0;
                            state = 0;
                            is_xml = false;
                        }
                    }
                }
                b'"' | b'\'' => {
                    if p != 0 && (in_q == 0 || c == in_q) {
                        in_q = if in_q != 0 { 0 } else { c };
                    }
                }
                b'!' => {
                    if p >= 1 && buf[p - 1] == b'<' {
                        state = 3;
                        lc = c;
                    }
                }
                b'?' => {
                    if p >= 1 && buf[p - 1] == b'<' {
                        br = 0;
                        state = 2;
                    }
                }
                _ => {}
            },
            2 => match c {
                b'(' => {
                    if lc != b'"' && lc != b'\'' {
                        lc = b'(';
                        br += 1;
                    }
                }
                b')' => {
                    if lc != b'"' && lc != b'\'' {
                        lc = b')';
                        br -= 1;
                    }
                }
                b'>' => {
                    if depth > 0 {
                        depth -= 1;
                    } else if in_q == 0 && br == 0 && p >= 1 && lc != b'"' && buf[p - 1] == b'?' {
                        in_q = 0;
                        state = 0;
                    }
                }
                b'"' | b'\'' => {
                    if p >= 1 && buf[p - 1] != b'\\' {
                        if lc == c {
                            lc = 0;
                        } else if lc != b'\\' {
                            lc = c;
                        }
                        if p != 0 && (in_q == 0 || c == in_q) {
                            in_q = if in_q != 0 { 0 } else { c };
                        }
                    }
                }
                b'l' | b'L' => {
                    if p > 4
                        && (at(p - 1) == b'm' || at(p - 1) == b'M')
                        && (at(p - 2) == b'x' || at(p - 2) == b'X')
                        && at(p - 3) == b'?'
                        && at(p - 4) == b'<'
                    {
                        state = 1;
                        is_xml = true;
                    }
                }
                _ => {}
            },
            3 => match c {
                b'>' => {
                    if depth > 0 {
                        depth -= 1;
                    } else if in_q == 0 {
                        in_q = 0;
                        state = 0;
                    }
                }
                b'"' | b'\'' => {
                    if p != 0 && buf[p - 1] != b'\\' && (in_q == 0 || c == in_q) {
                        in_q = if in_q != 0 { 0 } else { c };
                    }
                }
                b'-' => {
                    if p >= 2 && buf[p - 1] == b'-' && buf[p - 2] == b'!' {
                        state = 4;
                    }
                }
                b'E' | b'e' => {
                    if p > 6
                        && at(p - 1).eq_ignore_ascii_case(&b'p')
                        && at(p - 2).eq_ignore_ascii_case(&b'y')
                        && at(p - 3).eq_ignore_ascii_case(&b't')
                        && at(p - 4).eq_ignore_ascii_case(&b'c')
                        && at(p - 5).eq_ignore_ascii_case(&b'o')
                        && at(p - 6).eq_ignore_ascii_case(&b'd')
                    {
                        state = 1;
                    }
                }
                _ => {}
            },
            _ => {
                // state 4: inside a comment
                if c == b'>' && in_q == 0 && p >= 2 && buf[p - 1] == b'-' && buf[p - 2] == b'-' {
                    in_q = 0;
                    state = 0;
                }
            }
        }
        p += 1;
    }
    out
}
