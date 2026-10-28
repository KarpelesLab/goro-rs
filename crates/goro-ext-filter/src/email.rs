//! FILTER_VALIDATE_EMAIL.
//!
//! PHP manual (Validate filters): "In general, this validates e-mail
//! addresses against the addr-spec syntax in RFC 822, with the exceptions
//! that comments and whitespace folding and dotless domain names are not
//! supported." The details below were observed on PHP 8.5 (the fixture's
//! e-mail section: every byte at every position of several addresses,
//! length limits, address literals; tests 016.phpt, 058.phpt,
//! CVE-2007-1900.phpt, bug44445.phpt, bug50158.phpt, bug52929.phpt,
//! bug55478.phpt):
//!
//! - local part: dot-separated words (RFC 5322 obs-local-part), each an
//!   atom of RFC 5322 atext or a quoted string of qtext without space and
//!   tab, or quoted pairs of a backslash and any ASCII byte;
//! - domain: at least two dot-separated labels of letters, digits and inner
//!   hyphens, at most 63 bytes, the last one starting with a letter; or an
//!   IPv4 address literal, or "IPv6:" and an IPv6 address with RFC 5321's
//!   group counts;
//! - the local part is at most 64 and the address at most 254 "units",
//!   where a unit is one character or quoted pair with the quotes around
//!   it (so a quoted local part may have 64 characters between its quotes);
//! - with FILTER_FLAG_EMAIL_UNICODE (RFC 6531) the local part may also
//!   contain letters and numbers (Unicode general categories L and N) beyond
//!   ASCII, and the address must be valid UTF-8.

use std::sync::OnceLock;

use crate::ip;

fn atext(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"!#$%&'*+-/=?^_`{|}~".contains(&c)
}

fn qtext(c: u8) -> bool {
    matches!(c, 0x01..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0x21 | 0x23..=0x5B | 0x5D..=0x7F)
}

/// Whether code point `c` is a letter or a number (observed on PHP 8.5 for
/// U+0080 to U+2FFFF: exactly \p{L} and \p{N}, in atoms and quoted strings).
fn letter_or_number(c: u32) -> bool {
    static MASK: OnceLock<u32> = OnceLock::new();
    let mask = *MASK.get_or_init(|| {
        goro_regex::unicode::lookup_gc_group("L").unwrap_or(0)
            | goro_regex::unicode::lookup_gc_group("N").unwrap_or(0)
    });
    mask >> goro_regex::unicode::gc(c) & 1 != 0
}

/// The length of a UTF-8 letter or number at `s[i..]` (already known to be
/// valid UTF-8), or 0.
fn unicode_ln(s: &[u8], i: usize) -> usize {
    let c = s[i];
    let len = match c {
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => return 0,
    };
    let Some(ch) = std::str::from_utf8(&s[i..(i + len).min(s.len())])
        .ok()
        .and_then(|t| t.chars().next())
    else {
        return 0;
    };
    if letter_or_number(ch as u32) { len } else { 0 }
}

/// The index of the '@' ending a valid local part.
fn local_part(s: &[u8], unicode: bool) -> Option<usize> {
    let n = s.len();
    let mut i = 0;
    loop {
        if i < n && s[i] == b'"' {
            i += 1;
            loop {
                let c = *s.get(i)?;
                if c == b'"' {
                    i += 1;
                    break;
                }
                if c == b'\\' {
                    if s.get(i + 1).is_some_and(|&x| x < 0x80) {
                        i += 2;
                        continue;
                    }
                    return None;
                }
                if qtext(c) {
                    i += 1;
                    continue;
                }
                if unicode && c >= 0x80 {
                    let l = unicode_ln(s, i);
                    if l > 0 {
                        i += l;
                        continue;
                    }
                }
                return None;
            }
        } else {
            let start = i;
            while i < n {
                let c = s[i];
                if atext(c) {
                    i += 1;
                } else if unicode && c >= 0x80 {
                    let l = unicode_ln(s, i);
                    if l == 0 {
                        break;
                    }
                    i += l;
                } else {
                    break;
                }
            }
            if i == start {
                return None;
            }
        }
        match s.get(i) {
            Some(b'.') => i += 1,
            Some(b'@') => return Some(i),
            _ => return None,
        }
    }
}

fn label_ok(l: &[u8]) -> bool {
    !l.is_empty()
        && l.len() <= 63
        && l[0].is_ascii_alphanumeric()
        && l[l.len() - 1].is_ascii_alphanumeric()
        && l.iter().all(|&c| c.is_ascii_alphanumeric() || c == b'-')
}

fn domain_ok(d: &[u8]) -> bool {
    if let Some(inner) = d.strip_prefix(b"[") {
        let Some(inner) = inner.strip_suffix(b"]") else {
            return false;
        };
        if inner.len() >= 5 && inner[..5].eq_ignore_ascii_case(b"ipv6:") {
            let v6 = &inner[5..];
            return v6.contains(&b':') && ip::parse_v6(v6, 6, 4).is_some();
        }
        return ip::parse_v4(inner).is_some();
    }
    let mut labels = 0;
    let mut last: &[u8] = b"";
    for l in d.split(|&c| c == b'.') {
        if !label_ok(l) {
            return false;
        }
        labels += 1;
        last = l;
    }
    labels >= 2 && last[0].is_ascii_alphabetic()
}

/// The length limits, counted in units of one character (one code point
/// with the Unicode flag) or one quoted pair, each with an optional quote
/// before and after it: rejected when some prefix of at least 65 units is
/// followed by '@', or when a prefix of 255 units exists. A run of quotes
/// that can't be shared out (two at the start, three between characters)
/// ends the units.
fn units_ok(s: &[u8], unicode: bool) -> bool {
    let n = s.len();
    let mut i = 0;
    let mut count = 0usize;
    loop {
        let mut q = 0;
        while i < n && s[i] == b'"' {
            q += 1;
            i += 1;
        }
        if count >= 65 && i < n && s[i] == b'@' && (q == 0 || q == 1) {
            return false;
        }
        if q > if count == 0 { 1 } else { 2 } || i >= n {
            return true;
        }
        if s[i] == b'\\' {
            if i + 1 < n && s[i + 1] < 0x80 {
                i += 2;
            } else {
                return true;
            }
        } else if unicode && s[i] >= 0xC0 {
            i += match s[i] {
                0xC0..=0xDF => 2,
                0xE0..=0xEF => 3,
                _ => 4,
            };
        } else {
            i += 1;
        }
        count += 1;
        if count >= 255 {
            return false;
        }
    }
}

/// PHP's matching gives up (and the address fails) on local parts with many
/// quoted words, apparently when its regular expression exceeds the PCRE
/// backtracking limit: observed on PHP 8.5, eight `"a".` words always fail,
/// seven fail once 14 characters follow the last quote, six once 59 do, and
/// longer quoted words fail sooner.
///
/// Modelled as a search tree: each quote with a character on both sides
/// could close one unit or open the next, doubling the paths explored from
/// there on; a segment of `n` units between such quotes costs `2n + 1` per
/// path. The total fits every observation when the budget is 245 757 (the
/// cost of both "seven words, 13 characters" and "six words, 58
/// characters", which pass).
fn backtracking_ok(s: &[u8], unicode: bool) -> bool {
    const BUDGET: u64 = 245_757;
    let n = s.len();
    let mut paths = 1u64;
    let mut cost = 0u64;
    let mut seg = 0u64;
    let mut i = 0;
    let mut prev_core = false;
    while i < n {
        if s[i] == b'"' {
            let next_core = i + 1 < n && s[i + 1] != b'"';
            if prev_core && next_core {
                cost += paths * (2 * seg + 1);
                if cost > BUDGET {
                    return false;
                }
                paths *= 2;
                seg = 0;
            }
            prev_core = false;
            i += 1;
        } else {
            i += if s[i] == b'\\' && i + 1 < n {
                2
            } else if unicode && s[i] >= 0xC0 {
                match s[i] {
                    0xC0..=0xDF => 2,
                    0xE0..=0xEF => 3,
                    _ => 4,
                }
            } else {
                1
            };
            seg += 1;
            prev_core = true;
        }
    }
    if paths == 1 {
        return true;
    }
    cost += paths * (2 * seg + 1);
    cost <= BUDGET
}

/// FILTER_VALIDATE_EMAIL; `unicode` is FILTER_FLAG_EMAIL_UNICODE.
pub fn valid_email(s: &[u8], unicode: bool) -> bool {
    if unicode && std::str::from_utf8(s).is_err() {
        return false;
    }
    if !units_ok(s, unicode) {
        return false;
    }
    let Some(at) = local_part(s, unicode) else {
        return false;
    };
    if !domain_ok(&s[at + 1..]) {
        return false;
    }
    // The '@' and the domain are characters for the estimate too: the quote
    // before '@' is ambiguous and the tail runs to the end of the address.
    backtracking_ok(s, unicode)
}
