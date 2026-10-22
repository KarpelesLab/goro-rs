//! The validation filters (logical_filters.c).

use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::*;

/// PHP_FILTER_TRIM_DEFAULT_EX: (start, len) of `s` without the default
/// whitespace around it; None when it is empty and `return_if_empty`.
fn trim_default(s: &[u8], return_if_empty: bool) -> Option<(usize, usize)> {
    let ws = |c: u8| matches!(c, b' ' | b'\t' | b'\r' | 0x0b | b'\n');
    let mut p = 0;
    let mut len = s.len();
    while len > 0 && ws(s[p]) {
        p += 1;
        len -= 1;
    }
    if len < 1 && return_if_empty {
        return None;
    }
    while len > 0 && ws(s[p + len - 1]) {
        len -= 1;
    }
    Some((p, len))
}

const MAX_LENGTH_OF_LONG: usize = 20;

/// php_filter_parse_int()
fn parse_int(s: &[u8]) -> Option<i64> {
    let mut i = 0;
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut neg = false;
    match at(0) {
        b'-' => {
            neg = true;
            i += 1;
        }
        b'+' => i += 1,
        _ => {}
    }
    let end = s.len();
    if at(i) == b'0' && i + 1 == end {
        return Some(0);
    }
    let mut v: i64;
    if i < end && (b'1'..=b'9').contains(&s[i]) {
        v = (if neg { -1 } else { 1 }) * (s[i] - b'0') as i64;
        i += 1;
    } else {
        return None;
    }
    if end - i > MAX_LENGTH_OF_LONG - 1 {
        return None;
    }
    while i < end {
        let c = s[i];
        if !c.is_ascii_digit() {
            return None;
        }
        let d = (c - b'0') as i64;
        i += 1;
        if !neg && v <= (i64::MAX - d) / 10 {
            v = v * 10 + d;
        } else if neg && v >= (i64::MIN + d) / 10 {
            v = v * 10 - d;
        } else {
            return None;
        }
    }
    Some(v)
}

/// php_filter_parse_octal()
fn parse_octal(s: &[u8]) -> Option<i64> {
    let mut v: u64 = 0;
    for &c in s {
        if !(b'0'..=b'7').contains(&c) {
            return None;
        }
        let n = (c - b'0') as u64;
        if v > u64::MAX / 8 {
            return None;
        }
        v *= 8;
        if v > u64::MAX - n {
            return None;
        }
        v += n;
    }
    Some(v as i64)
}

/// php_filter_parse_hex()
pub(crate) fn parse_hex(s: &[u8]) -> Option<i64> {
    let mut v: u64 = 0;
    for &c in s {
        let n = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return None,
        } as u64;
        if v > u64::MAX / 16 {
            return None;
        }
        v *= 16;
        if v > u64::MAX - n {
            return None;
        }
        v += n;
    }
    Some(v as i64)
}

fn long_opt(ctx: &Ctx, name: &str) -> Option<i64> {
    ctx.opt(name).map(|v| v.to_long())
}

pub(crate) fn filter_int(vm: &mut Vm, value: &mut Value, ctx: &Ctx) -> bool {
    let min = long_opt(ctx, "min_range");
    let max = long_opt(ctx, "max_range");
    let flags = ctx.flags;
    let s = bytes(value);
    if s.is_empty() {
        return validation_failed(vm, value, flags);
    }
    let allow_octal = flags & FILTER_FLAG_ALLOW_OCTAL != 0;
    let allow_hex = flags & FILTER_FLAG_ALLOW_HEX != 0;
    let Some((p, len)) = trim_default(&s, true) else {
        return validation_failed(vm, value, flags);
    };
    let t = &s[p..p + len];
    let at = |i: usize| t.get(i).copied().unwrap_or(0);
    let mut error = false;
    let mut ctx_value: i64 = 0;
    if at(0) == b'0' {
        let rest = &t[1..];
        if allow_hex && (at(1) == b'x' || at(1) == b'X') {
            let r = &rest[1..];
            if r.is_empty() {
                return validation_failed(vm, value, flags);
            }
            match parse_hex(r) {
                Some(v) => ctx_value = v,
                None => error = true,
            }
        } else if allow_octal {
            let mut r = rest;
            if at(1) == b'o' || at(1) == b'O' {
                r = &rest[1..];
                if r.is_empty() {
                    return validation_failed(vm, value, flags);
                }
            }
            match parse_octal(r) {
                Some(v) => ctx_value = v,
                None => error = true,
            }
        } else if !rest.is_empty() {
            error = true;
        }
    } else {
        match parse_int(t) {
            Some(v) => ctx_value = v,
            None => error = true,
        }
    }
    if error || min.is_some_and(|m| ctx_value < m) || max.is_some_and(|m| ctx_value > m) {
        return validation_failed(vm, value, flags);
    }
    *value = Value::Long(ctx_value);
    true
}

pub(crate) fn filter_boolean(vm: &mut Vm, value: &mut Value, ctx: &Ctx) -> bool {
    let s = bytes(value);
    let (p, len) = trim_default(&s, false).unwrap_or((0, 0));
    let t = &s[p..p + len];
    let eq = |w: &[u8]| t.eq_ignore_ascii_case(w);
    let ret = match len {
        0 => Some(false),
        1 => match t[0] {
            b'1' => Some(true),
            b'0' => Some(false),
            _ => None,
        },
        2 if eq(b"on") => Some(true),
        2 if eq(b"no") => Some(false),
        3 if eq(b"yes") => Some(true),
        3 if eq(b"off") => Some(false),
        4 if eq(b"true") => Some(true),
        5 if eq(b"false") => Some(false),
        _ => None,
    };
    match ret {
        None => validation_failed(vm, value, ctx.flags),
        Some(b) => {
            *value = if b { Value::True } else { Value::False };
            true
        }
    }
}

pub(crate) fn filter_float(vm: &mut Vm, value: &mut Value, ctx: &Ctx) -> bool {
    let flags = ctx.flags;
    let s = bytes(value);
    let Some((p0, len)) = trim_default(&s, true) else {
        return validation_failed(vm, value, flags);
    };
    let src = &s[p0..p0 + len];
    let mut dec_sep = b'.';
    if let Some(Value::String(d)) = ctx.opt("decimal") {
        if d.as_bytes().len() != 1 {
            throw(
                vm,
                b"ValueError",
                &format!(
                    "{}(): \"decimal\" option must be one character long",
                    ctx.fname
                ),
            );
            return validation_failed(vm, value, flags);
        }
        dec_sep = d.as_bytes()[0];
    }
    let tsd_sep: Vec<u8> = match ctx.opt("thousand") {
        Some(Value::String(t)) => {
            if t.as_bytes().is_empty() {
                throw(
                    vm,
                    b"ValueError",
                    &format!("{}(): \"thousand\" option must not be empty", ctx.fname),
                );
                return validation_failed(vm, value, flags);
            }
            t.as_bytes().to_vec()
        }
        _ => b"',.".to_vec(),
    };
    let min = ctx.opt("min_range").map(|v| v.to_double());
    let max = ctx.opt("max_range").map(|v| v.to_double());

    let end = src.len();
    let at = |i: usize| src.get(i).copied().unwrap_or(0);
    let mut num: Vec<u8> = Vec::with_capacity(len + 1);
    let mut i = 0;
    if i < end && (src[i] == b'+' || src[i] == b'-') {
        num.push(src[i]);
        i += 1;
    }
    let mut first = true;
    let ok = loop {
        let mut n = 0;
        while i < end && src[i].is_ascii_digit() {
            n += 1;
            num.push(src[i]);
            i += 1;
        }
        if i == end || at(i) == dec_sep || at(i) == b'e' || at(i) == b'E' {
            if !first && n != 3 {
                break false;
            }
            if i < end && at(i) == dec_sep {
                num.push(b'.');
                i += 1;
                while i < end && src[i].is_ascii_digit() {
                    num.push(src[i]);
                    i += 1;
                }
            }
            if i < end && (at(i) == b'e' || at(i) == b'E') {
                num.push(src[i]);
                i += 1;
                if i < end && (src[i] == b'+' || src[i] == b'-') {
                    num.push(src[i]);
                    i += 1;
                }
                while i < end && src[i].is_ascii_digit() {
                    num.push(src[i]);
                    i += 1;
                }
            }
            break true;
        }
        if flags & FILTER_FLAG_ALLOW_THOUSAND != 0 && i < end && tsd_sep.contains(&src[i]) {
            if if first { !(1..=3).contains(&n) } else { n != 3 } {
                break false;
            }
            first = false;
            i += 1;
        } else {
            break false;
        }
    };
    if !ok || i != end {
        return validation_failed(vm, value, flags);
    }
    use goro_core::value::{NumStr, parse_numeric_str};
    match parse_numeric_str(&num, false) {
        Some((NumStr::Long(l), false)) => {
            let d = l as f64;
            if min.is_some_and(|m| d < m) || max.is_some_and(|m| d > m) {
                return validation_failed(vm, value, flags);
            }
            *value = Value::Double(d);
            true
        }
        Some((NumStr::Double(d), false)) => {
            let has_nonzero = num.iter().any(|c| (b'1'..=b'9').contains(c));
            if (d == 0.0 && num.len() > 1 && has_nonzero) || !d.is_finite() {
                return validation_failed(vm, value, flags);
            }
            if min.is_some_and(|m| d < m) || max.is_some_and(|m| d > m) {
                return validation_failed(vm, value, flags);
            }
            *value = Value::Double(d);
            true
        }
        _ => validation_failed(vm, value, flags),
    }
}

pub(crate) fn validate_regexp(vm: &mut Vm, value: &mut Value, ctx: &Ctx) -> bool {
    let Some(Value::String(re)) = ctx.opt("regexp") else {
        throw(
            vm,
            b"ValueError",
            &format!("{}(): \"regexp\" option is missing", ctx.fname),
        );
        return validation_failed(vm, value, ctx.flags);
    };
    let Some(pce) = goro_ext_standard::regex::get_compiled_regex(vm, re.as_bytes(), ctx.fname)
    else {
        return validation_failed(vm, value, ctx.flags);
    };
    let s = bytes(value);
    if goro_ext_standard::regex::pcre_match_subject(vm, &pce, &s) < 0 {
        return validation_failed(vm, value, ctx.flags);
    }
    true
}

fn isalnum(c: u8) -> bool {
    c.is_ascii_alphanumeric()
}

/// php_filter_validate_domain_ex()
pub(crate) fn validate_domain_ex(domain: &[u8], flags: i64) -> bool {
    let hostname = flags & FILTER_FLAG_HOSTNAME != 0;
    let at = |i: usize| domain.get(i).copied().unwrap_or(0);
    let mut l = domain.len();
    let mut e = l;
    if l > 0 && domain[l - 1] == b'.' {
        e = l - 1;
        l -= 1;
    }
    if l > 253 {
        return false;
    }
    if at(0) == b'.' || (hostname && !isalnum(at(0))) {
        return false;
    }
    let mut i: u32 = 1;
    let mut s = 0;
    while s < e {
        if domain[s] == b'.' {
            if at(s + 1) == b'.'
                || (hostname && (s == 0 || !isalnum(at(s - 1)) || !isalnum(at(s + 1))))
            {
                return false;
            }
            i = 1;
        } else {
            if i > 63 || (hostname && (domain[s] != b'-' || at(s + 1) == 0) && !isalnum(domain[s]))
            {
                return false;
            }
            i += 1;
        }
        s += 1;
    }
    true
}

pub(crate) fn validate_domain(vm: &mut Vm, value: &mut Value, ctx: &Ctx) -> bool {
    if !validate_domain_ex(&bytes(value), ctx.flags) {
        return validation_failed(vm, value, ctx.flags);
    }
    true
}

fn is_userinfo_valid(s: &[u8]) -> bool {
    let valid = b"-._~!$&'()*+,;=:";
    let mut p = 0;
    while p < s.len() {
        let c = s[p];
        if c.is_ascii_alphabetic() || c.is_ascii_digit() || valid.contains(&c) {
            p += 1;
        } else if c == b'%'
            && p + 3 <= s.len()
            && s[p + 1].is_ascii_digit()
            && s[p + 2].is_ascii_hexdigit()
        {
            p += 3;
        } else {
            return false;
        }
    }
    true
}

fn is_valid_ipv6_hostname(s: &[u8]) -> bool {
    s.len() >= 2
        && s[0] == b'['
        && s[s.len() - 1] == b']'
        && validate_ipv6(&s[1..s.len() - 1], None)
}

pub(crate) fn validate_url(vm: &mut Vm, value: &mut Value, ctx: &Ctx) -> bool {
    let old_len = bytes(value).len();
    crate::sanitize::filter_url(vm, value, ctx);
    if !matches!(value, Value::String(_)) || old_len != bytes(value).len() {
        return validation_failed(vm, value, ctx.flags);
    }
    // FETCH_STR_OPTION(): a non-string option is ignored
    let parser_name = match ctx.opt("uri_parser_class") {
        Some(Value::String(p)) => Some(p.as_bytes().to_vec()),
        _ => None,
    };
    let Some(parser) = goro_core::uri::get_parser(parser_name.as_deref()) else {
        throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): \"uri_parser_class\" option has invalid value",
                ctx.fname
            ),
        );
        return validation_failed(vm, value, ctx.flags);
    };
    let s = bytes(value);
    if parser != goro_core::uri::Parser::PhpParseUrl {
        // php_uri_parse_to_struct() with a URI class's parser: its own
        // validation replaces the hostname and userinfo checks.
        let Some(c) =
            goro_core::uri::parse_components(vm, parser, &s, goro_core::uri::ReadMode::Raw, true)
        else {
            return validation_failed(vm, value, ctx.flags);
        };
        let str_of = |v: &Value| match v {
            Value::String(s) => Some(s.as_bytes().to_vec()),
            _ => None,
        };
        let (scheme, host, path, query) = (str_of(&c[0]), str_of(&c[3]), &c[5], &c[6]);
        if let Some(sc) = &scheme {
            if (sc.eq_ignore_ascii_case(b"http") || sc.eq_ignore_ascii_case(b"https"))
                && host.is_none()
            {
                return validation_failed(vm, value, ctx.flags);
            }
        }
        let Some(sc) = &scheme else {
            return validation_failed(vm, value, ctx.flags);
        };
        if (host.is_none() && sc != b"mailto" && sc != b"news" && sc != b"file")
            || (ctx.flags & FILTER_FLAG_PATH_REQUIRED != 0 && !matches!(path, Value::String(_)))
            || (ctx.flags & FILTER_FLAG_QUERY_REQUIRED != 0 && !matches!(query, Value::String(_)))
        {
            return validation_failed(vm, value, ctx.flags);
        }
        return true;
    }
    let parsed = match vm.functions.get(b"parse_url".as_slice()).copied() {
        Some(f) => f(vm, &[sval(s.clone())]).unwrap_or(Value::False),
        None => Value::False,
    };
    let Value::Array(parts) = parsed else {
        return validation_failed(vm, value, ctx.flags);
    };
    let parts = parts.borrow();
    let get = |k: &str| -> Option<Vec<u8>> {
        match parts.get_str(k.as_bytes()) {
            Some(Value::String(s)) => Some(s.as_bytes().to_vec()),
            _ => None,
        }
    };
    let scheme = get("scheme");
    let host = get("host");
    if let Some(sc) = &scheme {
        if sc.eq_ignore_ascii_case(b"http") || sc.eq_ignore_ascii_case(b"https") {
            let Some(h) = &host else {
                return validation_failed(vm, value, ctx.flags);
            };
            if !is_valid_ipv6_hostname(h) && !validate_domain_ex(h, FILTER_FLAG_HOSTNAME) {
                return validation_failed(vm, value, ctx.flags);
            }
        }
    }
    let Some(sc) = &scheme else {
        return validation_failed(vm, value, ctx.flags);
    };
    if (host.is_none() && sc != b"mailto" && sc != b"news" && sc != b"file")
        || (ctx.flags & FILTER_FLAG_PATH_REQUIRED != 0 && get("path").is_none())
        || (ctx.flags & FILTER_FLAG_QUERY_REQUIRED != 0 && get("query").is_none())
    {
        return validation_failed(vm, value, ctx.flags);
    }
    if get("user").is_some_and(|u| !is_userinfo_valid(&u))
        || get("pass").is_some_and(|p| !is_userinfo_valid(&p))
    {
        return validation_failed(vm, value, ctx.flags);
    }
    true
}

const EMAIL_RE_UNICODE: &str = "/^(?!(?:(?:\\x22?\\x5C[\\x00-\\x7E]\\x22?)|(?:\\x22?[^\\x5C\\x22]\\x22?)){255,})(?!(?:(?:\\x22?\\x5C[\\x00-\\x7E]\\x22?)|(?:\\x22?[^\\x5C\\x22]\\x22?)){65,}@)(?:(?:[\\x21\\x23-\\x27\\x2A\\x2B\\x2D\\x2F-\\x39\\x3D\\x3F\\x5E-\\x7E\\pL\\pN]+)|(?:\\x22(?:[\\x01-\\x08\\x0B\\x0C\\x0E-\\x1F\\x21\\x23-\\x5B\\x5D-\\x7F\\pL\\pN]|(?:\\x5C[\\x00-\\x7F]))*\\x22))(?:\\.(?:(?:[\\x21\\x23-\\x27\\x2A\\x2B\\x2D\\x2F-\\x39\\x3D\\x3F\\x5E-\\x7E\\pL\\pN]+)|(?:\\x22(?:[\\x01-\\x08\\x0B\\x0C\\x0E-\\x1F\\x21\\x23-\\x5B\\x5D-\\x7F\\pL\\pN]|(?:\\x5C[\\x00-\\x7F]))*\\x22)))*@(?:(?:(?!.*[^.]{64,})(?:(?:(?:xn--)?[a-z0-9]+(?:-+[a-z0-9]+)*\\.){1,126}){1,}(?:(?:[a-z][a-z0-9]*)|(?:(?:xn--)[a-z0-9]+))(?:-+[a-z0-9]+)*)|(?:\\[(?:(?:IPv6:(?:(?:[a-f0-9]{1,4}(?::[a-f0-9]{1,4}){7})|(?:(?!(?:.*[a-f0-9][:\\]]){7,})(?:[a-f0-9]{1,4}(?::[a-f0-9]{1,4}){0,5})?::(?:[a-f0-9]{1,4}(?::[a-f0-9]{1,4}){0,5})?)))|(?:(?:IPv6:(?:(?:[a-f0-9]{1,4}(?::[a-f0-9]{1,4}){5}:)|(?:(?!(?:.*[a-f0-9]:){5,})(?:[a-f0-9]{1,4}(?::[a-f0-9]{1,4}){0,3})?::(?:[a-f0-9]{1,4}(?::[a-f0-9]{1,4}){0,3}:)?)))?(?:(?:25[0-5])|(?:2[0-4][0-9])|(?:1[0-9]{2})|(?:[1-9]?[0-9]))(?:\\.(?:(?:25[0-5])|(?:2[0-4][0-9])|(?:1[0-9]{2})|(?:[1-9]?[0-9]))){3}))\\]))$/iDu";
const EMAIL_RE: &str = "/^(?!(?:(?:\\x22?\\x5C[\\x00-\\x7E]\\x22?)|(?:\\x22?[^\\x5C\\x22]\\x22?)){255,})(?!(?:(?:\\x22?\\x5C[\\x00-\\x7E]\\x22?)|(?:\\x22?[^\\x5C\\x22]\\x22?)){65,}@)(?:(?:[\\x21\\x23-\\x27\\x2A\\x2B\\x2D\\x2F-\\x39\\x3D\\x3F\\x5E-\\x7E]+)|(?:\\x22(?:[\\x01-\\x08\\x0B\\x0C\\x0E-\\x1F\\x21\\x23-\\x5B\\x5D-\\x7F]|(?:\\x5C[\\x00-\\x7F]))*\\x22))(?:\\.(?:(?:[\\x21\\x23-\\x27\\x2A\\x2B\\x2D\\x2F-\\x39\\x3D\\x3F\\x5E-\\x7E]+)|(?:\\x22(?:[\\x01-\\x08\\x0B\\x0C\\x0E-\\x1F\\x21\\x23-\\x5B\\x5D-\\x7F]|(?:\\x5C[\\x00-\\x7F]))*\\x22)))*@(?:(?:(?!.*[^.]{64,})(?:(?:(?:xn--)?[a-z0-9]+(?:-+[a-z0-9]+)*\\.){1,126}){1,}(?:(?:[a-z][a-z0-9]*)|(?:(?:xn--)[a-z0-9]+))(?:-+[a-z0-9]+)*)|(?:\\[(?:(?:IPv6:(?:(?:[a-f0-9]{1,4}(?::[a-f0-9]{1,4}){7})|(?:(?!(?:.*[a-f0-9][:\\]]){7,})(?:[a-f0-9]{1,4}(?::[a-f0-9]{1,4}){0,5})?::(?:[a-f0-9]{1,4}(?::[a-f0-9]{1,4}){0,5})?)))|(?:(?:IPv6:(?:(?:[a-f0-9]{1,4}(?::[a-f0-9]{1,4}){5}:)|(?:(?!(?:.*[a-f0-9]:){5,})(?:[a-f0-9]{1,4}(?::[a-f0-9]{1,4}){0,3})?::(?:[a-f0-9]{1,4}(?::[a-f0-9]{1,4}){0,3}:)?)))?(?:(?:25[0-5])|(?:2[0-4][0-9])|(?:1[0-9]{2})|(?:[1-9]?[0-9]))(?:\\.(?:(?:25[0-5])|(?:2[0-4][0-9])|(?:1[0-9]{2})|(?:[1-9]?[0-9]))){3}))\\]))$/iD";

pub(crate) fn validate_email(vm: &mut Vm, value: &mut Value, ctx: &Ctx) -> bool {
    let re = if ctx.flags & FILTER_FLAG_EMAIL_UNICODE != 0 {
        EMAIL_RE_UNICODE
    } else {
        EMAIL_RE
    };
    let s = bytes(value);
    if s.len() > 320 {
        return validation_failed(vm, value, ctx.flags);
    }
    let Some(pce) = goro_ext_standard::regex::get_compiled_regex(vm, re.as_bytes(), ctx.fname)
    else {
        return validation_failed(vm, value, ctx.flags);
    };
    if goro_ext_standard::regex::pcre_match_subject(vm, &pce, &s) < 0 {
        return validation_failed(vm, value, ctx.flags);
    }
    true
}

/// _php_filter_validate_ipv4()
fn validate_ipv4(s: &[u8], ip: &mut [i32; 4]) -> bool {
    let end = s.len();
    let mut i = 0;
    let mut n = 0;
    while i < end {
        if !s[i].is_ascii_digit() {
            return false;
        }
        let leading_zero = s[i] == b'0';
        let mut m = 1;
        let mut num = (s[i] - b'0') as i32;
        i += 1;
        while i < end && s[i].is_ascii_digit() {
            num = num * 10 + (s[i] - b'0') as i32;
            i += 1;
            m += 1;
            if num > 255 || m > 3 {
                return false;
            }
        }
        if leading_zero && (num != 0 || m > 1) {
            return false;
        }
        ip[n] = num;
        n += 1;
        if n == 4 {
            return i == end;
        } else if i >= end || s[i] != b'.' {
            return false;
        }
        i += 1;
    }
    false
}

/// _php_filter_validate_ipv6()
pub(crate) fn validate_ipv6(full: &[u8], mut ip: Option<&mut [i32; 8]>) -> bool {
    if !full.contains(&b':') {
        return false;
    }
    let mut str_len = full.len();
    let mut ip4elm = [0i32; 4];
    let mut blocks: i32 = 0;
    let mut has_ipv4 = false;
    if let Some(dot) = full.iter().position(|&c| c == b'.') {
        let mut ipv4 = dot;
        while ipv4 > 0 && full[ipv4 - 1] != b':' {
            ipv4 -= 1;
        }
        if !validate_ipv4(&full[ipv4..], &mut ip4elm) {
            return false;
        }
        str_len = ipv4;
        if str_len < 2 {
            return false;
        }
        if full[ipv4 - 2] != b':' {
            str_len -= 1;
        }
        blocks = 2;
        has_ipv4 = true;
    }
    let s = &full[..str_len];
    let end = s.len();
    let mut i = 0;
    let mut compressed_pos: i32 = -1;
    while i < end {
        if s[i] == b':' {
            i += 1;
            if i >= end {
                return false;
            }
            if s[i] == b':' {
                if compressed_pos >= 0 {
                    return false;
                }
                if let Some(ip) = ip.as_deref_mut() {
                    if blocks < 8 {
                        ip[blocks as usize] = -1;
                    }
                }
                compressed_pos = blocks;
                blocks += 1;
                i += 1;
                if i == end {
                    if blocks > 8 {
                        return false;
                    }
                    break;
                }
            } else if i - 1 == 0 {
                return false;
            }
        }
        let mut num: u32 = 0;
        let mut n = 0;
        while i < end {
            let c = s[i];
            let d = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => break,
            };
            num = num.wrapping_mul(16).wrapping_add(d as u32);
            n += 1;
            i += 1;
        }
        if let Some(ip) = ip.as_deref_mut() {
            if blocks < 8 {
                ip[blocks as usize] = num as i32;
            }
        }
        if !(1..=4).contains(&n) {
            return false;
        }
        blocks += 1;
        if blocks > 8 {
            return false;
        }
    }
    // fixup_ip:
    if let Some(ip) = ip {
        if has_ipv4 {
            for x in ip.iter_mut().take(5) {
                *x = 0;
            }
            ip[5] = 0xffff;
            ip[6] = 256 * ip4elm[0] + ip4elm[1];
            ip[7] = 256 * ip4elm[2] + ip4elm[3];
        } else if compressed_pos >= 0 && blocks <= 8 {
            let offset = 8 - blocks;
            let mut k = 7;
            while k > compressed_pos + offset {
                ip[k as usize] = ip[(k - offset) as usize];
                k -= 1;
            }
            let mut k = compressed_pos + offset;
            while k >= compressed_pos {
                ip[k as usize] = 0;
                k -= 1;
            }
        }
    }
    (compressed_pos >= 0 && blocks <= 8) || blocks == 8
}

/// ipv4_get_status_flags(): Some((global, reserved, private)) for a
/// special block (one branch per RFC 6890 table entry, as in C).
#[allow(clippy::if_same_then_else)]
fn ipv4_status(ip: &[i32; 4]) -> Option<(bool, bool, bool)> {
    let (a, b, c) = (ip[0], ip[1], ip[2]);
    Some(if a == 0 {
        (false, true, false)
    } else if a == 10 {
        (false, false, true)
    } else if a == 100 && (64..=127).contains(&b) {
        (false, false, false)
    } else if a == 127 {
        (false, true, false)
    } else if a == 169 && b == 254 {
        (false, true, false)
    } else if a == 172 && (16..=31).contains(&b) {
        (false, false, true)
    } else if a == 192 && b == 0 && c == 0 {
        (false, false, false)
    } else if a == 192 && b == 0 && c == 2 {
        (false, false, false)
    } else if a == 192 && b == 88 && c == 99 {
        (true, false, false)
    } else if a == 192 && b == 168 {
        (false, false, true)
    } else if a == 198 && (18..=19).contains(&b) {
        (false, false, false)
    } else if a == 198 && b == 51 && c == 100 {
        (false, false, false)
    } else if a == 203 && b == 0 && c == 113 {
        (false, false, false)
    } else if a >= 240 && b <= 255 {
        (false, true, false)
    } else {
        return None;
    })
}

/// ipv6_get_status_flags()
#[allow(clippy::if_same_then_else)]
fn ipv6_status(ip: &[i32; 8]) -> Option<(bool, bool, bool)> {
    let z = |n: usize| ip[..n].iter().all(|&x| x == 0);
    Some(if z(8) {
        (false, true, false)
    } else if z(7) && ip[7] == 1 {
        (false, true, false)
    } else if ip[0] == 0x0064 && ip[1] == 0xff9b {
        (true, false, false)
    } else if z(5) && ip[5] == 0xffff {
        (false, true, false)
    } else if ip[0] == 0x0100 && ip[1] == 0 && ip[2] == 0 && ip[3] == 0 {
        (false, false, false)
    } else if ip[0] == 0x2001 && ip[1] <= 0x01ff {
        (false, false, false)
    } else if ip[0] == 0x2001 && ip[1] == 0x0db8 {
        (false, false, false)
    } else if ip[0] == 0x2001 && (0x0010..=0x001f).contains(&ip[1]) {
        (false, false, false)
    } else if ip[0] == 0x2002 {
        (false, false, false)
    } else if (0xfc00..=0xfdff).contains(&ip[0]) {
        (false, false, true)
    } else if (0xfe80..=0xfebf).contains(&ip[0]) {
        (false, true, false)
    } else {
        return None;
    })
}

pub(crate) fn validate_ip(vm: &mut Vm, value: &mut Value, ctx: &Ctx) -> bool {
    let flags = ctx.flags;
    let s = bytes(value);
    let v6 = if s.contains(&b':') {
        true
    } else if s.contains(&b'.') {
        false
    } else {
        return validation_failed(vm, value, flags);
    };
    let f4 = flags & FILTER_FLAG_IPV4 != 0;
    let f6 = flags & FILTER_FLAG_IPV6 != 0;
    if !(f4 && f6) && ((f4 && v6) || (f6 && !v6)) {
        return validation_failed(vm, value, flags);
    }
    let status = if !v6 {
        let mut ip = [0i32; 4];
        if !validate_ipv4(&s, &mut ip) {
            return validation_failed(vm, value, flags);
        }
        ipv4_status(&ip)
    } else {
        let mut ip = [0i32; 8];
        if !validate_ipv6(&s, Some(&mut ip)) {
            return validation_failed(vm, value, flags);
        }
        ipv6_status(&ip)
    };
    let Some((global, reserved, private)) = status else {
        return true;
    };
    if (flags & FILTER_FLAG_GLOBAL_RANGE != 0 && !global)
        || (flags & FILTER_FLAG_NO_PRIV_RANGE != 0 && private)
        || (flags & FILTER_FLAG_NO_RES_RANGE != 0 && reserved)
    {
        return validation_failed(vm, value, flags);
    }
    true
}

pub(crate) fn validate_mac(vm: &mut Vm, value: &mut Value, ctx: &Ctx) -> bool {
    let flags = ctx.flags;
    let input = bytes(value);
    let exp_sep = match ctx.opt("separator") {
        Some(Value::String(s)) => {
            if s.as_bytes().len() != 1 {
                throw(
                    vm,
                    b"ValueError",
                    &format!(
                        "{}(): \"separator\" option must be one character long",
                        ctx.fname
                    ),
                );
                return validation_failed(vm, value, flags);
            }
            Some(s.as_bytes()[0])
        }
        _ => None,
    };
    let (tokens, length, separator) = if input.len() == 14 {
        (3, 4, b'.')
    } else if input.len() == 17 && input[2] == b'-' {
        (6, 2, b'-')
    } else if input.len() == 17 && input[2] == b':' {
        (6, 2, b':')
    } else {
        return validation_failed(vm, value, flags);
    };
    if exp_sep.is_some_and(|e| e != separator) {
        return validation_failed(vm, value, flags);
    }
    for i in 0..tokens {
        let offset = i * (length + 1);
        if i < tokens - 1 && input[offset + length] != separator {
            return validation_failed(vm, value, flags);
        }
        if parse_hex(&input[offset..offset + length]).is_none() {
            return validation_failed(vm, value, flags);
        }
    }
    true
}
