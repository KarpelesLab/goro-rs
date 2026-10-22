//! entities.c: the HTML named character entities.

use crate::doc::*;
use crate::entitydefs::ENTITIES;

/// entitiesLookup(): the table is sorted by first letter.
fn entities_lookup(s: &[u8]) -> Option<&'static (&'static [u8], u32, u32)> {
    let s = crate::tmbstr::cstr(s);
    let ch = *s.first()?;
    let mut i = ENTITIES.iter().position(|e| e.0[0] == ch)?;
    while i < ENTITIES.len() {
        let e = &ENTITIES[i];
        if e.0[0] != ch {
            break;
        }
        if e.0 == s {
            return Some(e);
        }
        i += 1;
    }
    None
}

/// `sscanf(s, "%u")` / `sscanf(s, "%x")` (glibc): leading white space, an
/// optional sign, for hex an optional 0x prefix; the value is that of
/// strtoul() truncated to unsigned int. None when nothing matched.
pub fn scanf_uint(s: &[u8], hex: bool) -> Option<u32> {
    let s = crate::tmbstr::cstr(s);
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let radix = if hex { 16 } else { 10 };
    let mut any = false;
    if hex && i + 1 < s.len() && s[i] == b'0' && (s[i + 1] == b'x' || s[i + 1] == b'X') {
        any = true;
        i += 2;
    }
    let mut v: u64 = 0;
    let mut overflow = false;
    while i < s.len() {
        let d = match (s[i] as char).to_digit(radix) {
            Some(d) => d as u64,
            None => break,
        };
        any = true;
        match v.checked_mul(radix as u64).and_then(|x| x.checked_add(d)) {
            Some(x) => v = x,
            None => overflow = true,
        }
        i += 1;
    }
    if !any {
        return None;
    }
    if overflow {
        v = u64::MAX;
    } else if neg {
        v = v.wrapping_neg();
    }
    Some(v as u32)
}

/// EntityInfo(): `name` starts with '&'; returns (found, code, versions).
pub fn entity_info(name: &[u8], is_xml: bool) -> (bool, u32, u32) {
    let at = |i: usize| name.get(i).copied().unwrap_or(0);
    if at(1) == b'#' {
        let res = if at(2) == b'x' || (!is_xml && at(2) == b'X') {
            scanf_uint(name.get(3..).unwrap_or(&[]), true)
        } else {
            scanf_uint(name.get(2..).unwrap_or(&[]), false)
        };
        return match res {
            Some(c) => (true, c, VERS_ALL),
            None => (false, 0, if is_xml { VERS_XML } else { VERS_PROPRIETARY }),
        };
    }
    if let Some(np) = entities_lookup(name.get(1..).unwrap_or(&[])) {
        return (true, np.2, np.1);
    }
    (false, 0, if is_xml { VERS_XML } else { VERS_PROPRIETARY })
}

/// EntityName(): the first entity name for a code in the given versions.
pub fn entity_name(ch: u32, versions: u32) -> Option<&'static [u8]> {
    for e in ENTITIES {
        if e.2 == ch && (e.1 & versions) != 0 {
            return Some(e.0);
        }
    }
    None
}
