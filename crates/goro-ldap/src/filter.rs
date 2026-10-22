//! RFC 4515 search-filter string → BER (filter.c: ldap_pvt_put_filter and
//! helpers), and ldap_put_vrFilter for the ValuesReturnFilter control.

use crate::ber::{Writer, TAG_OCTETSTRING};
use crate::consts::*;

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r')
}
fn is_alpha(c: u8) -> bool {
    c.is_ascii_alphabetic()
}
fn is_ldh(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-'
}
fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}
fn hex2value(c: u8) -> i32 {
    match c {
        b'0'..=b'9' => (c - b'0') as i32,
        b'A'..=b'F' => (c - b'A' + 10) as i32,
        b'a'..=b'f' => (c - b'a' + 10) as i32,
        _ => -1,
    }
}

fn skip_spaces(s: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    &s[i..]
}

/// ldap_is_oid
fn is_oid(s: &[u8]) -> bool {
    if s.is_empty() {
        return false;
    }
    if is_alpha(s[0]) {
        s[1..].iter().all(|&c| is_ldh(c))
    } else if is_digit(s[0]) {
        let mut dot = 0;
        for &c in &s[1..] {
            if is_digit(c) {
                dot = 0;
            } else if c == b'.' {
                dot += 1;
                if dot > 1 {
                    return false;
                }
            } else {
                return false;
            }
        }
        dot == 0
    } else {
        false
    }
}

/// ldap_is_desc (attribute description: type with optional ;options)
fn is_desc(s: &[u8]) -> bool {
    if s.is_empty() {
        return false;
    }
    // split off options at ';'
    let mut parts = s.split(|&c| c == b';');
    let first = parts.next().unwrap();
    if first.is_empty() {
        return false;
    }
    let base_ok = if is_alpha(first[0]) {
        first[1..].iter().all(|&c| is_ldh(c))
    } else if is_digit(first[0]) {
        let mut dot = 0;
        let mut ok = true;
        for &c in &first[1..] {
            if is_digit(c) {
                dot = 0;
            } else if c == b'.' {
                dot += 1;
                if dot > 1 {
                    ok = false;
                    break;
                }
            } else {
                ok = false;
                break;
            }
        }
        ok && dot == 0
    } else {
        false
    };
    if !base_ok {
        return false;
    }
    for opt in parts {
        if opt.is_empty() || !opt.iter().all(|&c| is_ldh(c)) {
            return false;
        }
    }
    true
}

/// ldap_pvt_find_wildcard: index of next '*' (the escape-aware scan).
/// Returns Some(index of '*') or Some(len) at end, or None on bad escape/paren.
fn find_wildcard(s: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'*' => return Some(i),
            b'(' | b')' => return None,
            b'\\' => {
                if i + 1 >= s.len() {
                    return None;
                }
                if i + 2 < s.len() && s[i + 1].is_ascii_hexdigit() && s[i + 2].is_ascii_hexdigit() {
                    i += 2;
                } else {
                    match s[i + 1] {
                        b'*' | b'(' | b')' | b'\\' => i += 1,
                        _ => return None,
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    Some(s.len())
}

/// ldap_pvt_filter_value_unescape: unescape a filter assertion value.
/// Returns None (-1) on illegal escape / unescaped '(' ')' '*'.
fn value_unescape(fval: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(fval.len());
    let mut v = 0;
    while v < fval.len() {
        match fval[v] {
            b'(' | b')' | b'*' => return None,
            b'\\' => {
                v += 1;
                if v >= fval.len() {
                    return None;
                }
                let v1 = hex2value(fval[v]);
                if v1 >= 0 {
                    if v + 1 >= fval.len() {
                        return None;
                    }
                    let v2 = hex2value(fval[v + 1]);
                    if v2 < 0 {
                        return None;
                    }
                    out.push((v1 * 16 + v2) as u8);
                    v += 1;
                } else {
                    match fval[v] {
                        b'(' | b')' | b'*' | b'\\' => out.push(fval[v]),
                        _ => return None,
                    }
                }
            }
            c => out.push(c),
        }
        v += 1;
    }
    Some(out)
}

/// find_right_paren: index of the matching ')' in `s` (after the opening
/// '(' already consumed), honoring escapes. None if unbalanced.
fn find_right_paren(s: &[u8]) -> Option<usize> {
    let mut balance = 1;
    let mut escape = false;
    let mut i = 0;
    while i < s.len() && balance != 0 {
        if !escape {
            if s[i] == b'(' {
                balance += 1;
            } else if s[i] == b')' {
                balance -= 1;
            }
        }
        escape = s[i] == b'\\' && !escape;
        if balance != 0 {
            i += 1;
        }
    }
    if i < s.len() && balance == 0 {
        Some(i)
    } else {
        None
    }
}

/// Encode the whole filter string. Returns None on a parse error
/// (LDAP_FILTER_ERROR).
pub fn encode(filter: &[u8]) -> Option<Vec<u8>> {
    let mut w = Writer::new();
    let rest = put_filter(&mut w, filter)?;
    if !rest.is_empty() {
        return None;
    }
    Some(w.finish())
}

/// Encode one filter at the head of `s`; return the remainder.
fn put_filter<'a>(w: &mut Writer, s: &'a [u8]) -> Option<&'a [u8]> {
    let s = skip_spaces(s);
    if s.first() == Some(&b'(') {
        let inner = skip_spaces(&s[1..]);
        match inner.first() {
            Some(&b'&') => {
                w.start(LDAP_FILTER_AND);
                let rest = put_filter_list(w, &inner[1..], LDAP_FILTER_AND)?;
                w.end();
                expect_close(rest)
            }
            Some(&b'|') => {
                w.start(LDAP_FILTER_OR);
                let rest = put_filter_list(w, &inner[1..], LDAP_FILTER_OR)?;
                w.end();
                expect_close(rest)
            }
            Some(&b'!') => {
                w.start(LDAP_FILTER_NOT);
                let rest = put_filter_list(w, &inner[1..], LDAP_FILTER_NOT)?;
                w.end();
                expect_close(rest)
            }
            Some(&b'(') => None,
            Some(_) => {
                // simple filter: read until the matching ')'
                let end = find_right_paren(inner)?;
                put_simple(w, &inner[..end])?;
                expect_close(&inner[end..])
            }
            None => None,
        }
    } else if s.is_empty() {
        None
    } else {
        // bare "type=value" filter: whole remaining string
        put_simple(w, s)?;
        Some(&[])
    }
}

fn expect_close(s: &[u8]) -> Option<&[u8]> {
    if s.first() == Some(&b')') {
        Some(&s[1..])
    } else {
        None
    }
}

/// put_filter_list: a run of "(filter)" items up to the enclosing ')'.
/// For NOT, exactly one item.
fn put_filter_list<'a>(w: &mut Writer, s: &'a [u8], tag: u32) -> Option<&'a [u8]> {
    let mut s = s;
    let mut count = 0;
    loop {
        s = skip_spaces(s);
        if s.first() != Some(&b'(') {
            break;
        }
        s = put_filter(w, s)?;
        count += 1;
        if tag == LDAP_FILTER_NOT {
            break;
        }
    }
    if tag == LDAP_FILTER_NOT && count != 1 {
        return None;
    }
    Some(s)
}

/// put_simple_filter: `str` is one assertion without surrounding parens.
pub fn put_simple(w: &mut Writer, s: &[u8]) -> Option<()> {
    if s.first() == Some(&b'=') {
        return None;
    }
    let eq = s.iter().position(|&c| c == b'=')?;
    let value = &s[eq + 1..];
    // char before '=' may be '<','>','~',':' extending the operator
    let (ftype, type_end) = if eq == 0 {
        (LDAP_FILTER_EQUALITY, 0)
    } else {
        match s[eq - 1] {
            b'<' => (LDAP_FILTER_LE, eq - 1),
            b'>' => (LDAP_FILTER_GE, eq - 1),
            b'~' => (LDAP_FILTER_APPROX, eq - 1),
            b':' => return put_ext(w, &s[..eq - 1], &s[eq - 1..eq], value, eq),
            _ => (LDAP_FILTER_EQUALITY, eq),
        }
    };
    let typ = &s[..type_end];
    if ftype == LDAP_FILTER_EQUALITY {
        // may be present or substrings
        let star = find_wildcard(value)?;
        if star == value.len() {
            // plain equality
            if !is_desc(typ) {
                return None;
            }
            let v = value_unescape(value)?;
            return put_ava(w, LDAP_FILTER_EQUALITY, typ, &v);
        } else if value == b"*" {
            if !is_desc(typ) {
                return None;
            }
            w.raw(LDAP_FILTER_PRESENT, typ);
            return Some(());
        } else {
            return put_substrings(w, typ, value, star);
        }
    }
    if !is_desc(typ) {
        return None;
    }
    let v = value_unescape(value)?;
    put_ava(w, ftype, typ, &v)
}

fn put_ava(w: &mut Writer, ftype: u32, typ: &[u8], value: &[u8]) -> Option<()> {
    w.start(ftype);
    w.octets(typ);
    w.raw(TAG_OCTETSTRING, value);
    w.end();
    Some(())
}

/// put_substring_filter
fn put_substrings(w: &mut Writer, typ: &[u8], val: &[u8], mut star: usize) -> Option<()> {
    if !is_desc(typ) {
        return None;
    }
    w.start(LDAP_FILTER_SUBSTRINGS);
    w.octets(typ);
    w.start_seq();
    let mut val = val;
    let mut gotstar = 0;
    loop {
        if val.is_empty() {
            break;
        }
        if gotstar > 0 {
            star = find_wildcard(val)?;
        }
        let ftype;
        let piece;
        if star == val.len() {
            ftype = LDAP_SUBSTRING_FINAL;
            piece = val;
            val = &val[val.len()..];
        } else {
            piece = &val[..star];
            val = &val[star + 1..];
            if gotstar == 0 {
                ftype = LDAP_SUBSTRING_INITIAL;
            } else {
                ftype = LDAP_SUBSTRING_ANY;
            }
            gotstar += 1;
        }
        if !piece.is_empty() || ftype == LDAP_SUBSTRING_ANY {
            let v = value_unescape(piece)?;
            if v.is_empty() && ftype != LDAP_SUBSTRING_ANY {
                // libldap requires len>0 for initial/final
                return None;
            }
            w.raw(ftype, &v);
        }
        if star == 0 && gotstar == 0 {
            // leading star consumed with empty initial handled above
        }
        if val.is_empty() && ftype == LDAP_SUBSTRING_FINAL {
            break;
        }
    }
    w.end();
    w.end();
    Some(())
}

/// Extensible match: `type` before the first ':', `value` after '='.
fn put_ext(w: &mut Writer, typ: &[u8], _colon: &[u8], value: &[u8], _eq: usize) -> Option<()> {
    // type part may contain :dn and :rule (type was s[..eq-1] = everything
    // before the ':'); we re-derive from the original by splitting on ':'.
    // typ here is the attribute (may be empty), and we must recover dn/rule
    // from the ':' section. Reconstruct: the operator was ":=", and before
    // it there may be more colons. We received typ = part before first ':'.
    // The caller passed s[..eq-1] which already stripped one ':'. To parse
    // dn/rule we need the full left side; do it here from typ + remainder.
    //
    // Simplement: parse the left side given as `left` = typ + the colon
    // section. But we only have typ (before first colon). Reconstruct using
    // the colon marker: everything between the first ':' and '=' is dn/rule.
    // Our caller gave typ=s[..eq-1]; that includes any inner colons. So
    // split typ on ':'.
    let mut it = typ.split(|&c| c == b':');
    let attr = it.next().unwrap_or(b"");
    let rest: Vec<&[u8]> = it.collect();
    let mut dn: Option<&[u8]> = None;
    let mut rule: Option<&[u8]> = None;
    match rest.len() {
        0 => {}
        1 => {
            // one colon: either "dn" or a rule
            if rest[0].eq_ignore_ascii_case(b"dn") {
                if !is_desc(attr) {
                    return None;
                }
                rule = Some(b"");
            } else {
                rule = Some(rest[0]);
            }
        }
        2 => {
            // two colons: dn:rule
            if !rest[0].eq_ignore_ascii_case(b"dn") {
                return None;
            }
            dn = Some(rest[0]);
            rule = Some(rest[1]);
        }
        _ => return None,
    }
    if attr.is_empty() && rule.map(|r| r.is_empty()).unwrap_or(true) {
        return None;
    }
    if !attr.is_empty() && !is_desc(attr) {
        return None;
    }
    if let Some(r) = rule {
        if !r.is_empty() && !is_oid(r) {
            return None;
        }
    }
    w.start(LDAP_FILTER_EXT);
    if let Some(r) = rule {
        if !r.is_empty() {
            w.raw(LDAP_FILTER_EXT_OID, r);
        }
    }
    if !attr.is_empty() {
        w.raw(LDAP_FILTER_EXT_TYPE, attr);
    }
    let v = value_unescape(value)?;
    w.raw(LDAP_FILTER_EXT_VALUE, &v);
    if dn.is_some() {
        w.bool_tagged(LDAP_FILTER_EXT_DNATTRS, true);
    }
    w.end();
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn simple() {
        assert!(encode(b"(objectClass=person)").is_some());
        assert!(encode(b"objectClass=*").is_some());
        assert!(encode(b"(cn=user*)").is_some());
        assert!(encode(b"(&(cn=a)(sn=b))").is_some());
        assert!(encode(b"(!(cn=a))").is_some());
        assert!(encode(b"(|(cn=a)(cn=b))").is_some());
        assert!(encode(b"(o=my-domain)").is_some());
        assert!(encode(b"(cn=a*b*c)").is_some());
        assert!(encode(b"(").is_none());
        assert!(encode(b"(&)").is_some());
    }
}
