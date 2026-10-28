//! FILTER_VALIDATE_INT, _FLOAT, _BOOL, _REGEXP, _DOMAIN and _MAC.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goro_regex::{Captures, MatchOptions, Regex, Scratch};

use crate::FilterHost;
use crate::consts::*;

/// The whitespace the numeric and boolean validators trim at both ends:
/// space, \t, \n, \v and \r, not \f or NUL (observed on PHP 8.5 for every
/// byte before and after "1"; test ext/filter/tests/013.phpt).
#[inline]
pub(crate) fn trim(s: &[u8]) -> &[u8] {
    let ws = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | 0x0B | b'\r');
    let mut a = 0;
    let mut b = s.len();
    while a < b && ws(s[a]) {
        a += 1;
    }
    while b > a && ws(s[b - 1]) {
        b -= 1;
    }
    &s[a..b]
}

/// Digits in `radix` as a u64, `None` on an empty run, a bad digit or
/// overflow.
fn radix_u64(d: &[u8], radix: u32) -> Option<u64> {
    if d.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    for &c in d {
        let x = (c as char).to_digit(radix)? as u64;
        v = v.checked_mul(radix as u64)?.checked_add(x)?;
    }
    Some(v)
}

/// FILTER_VALIDATE_INT without the range options.
///
/// PHP manual (Validate filters) and observed on PHP 8.5 (tests 013.phpt,
/// 046.phpt-048.phpt, bug80584.phpt, surprising_integer_literals.phpt):
/// decimal is `[+-]?(0|[1-9][0-9]*)` within the int range; with
/// FILTER_FLAG_ALLOW_HEX `0x`/`0X` and hex digits, with
/// FILTER_FLAG_ALLOW_OCTAL `0` or `0o`/`0O` and octal digits, unsigned,
/// up to 64 bits, wrapping into the int range (0xffffffffffffffff is -1).
pub(crate) fn int(s: &[u8], flags: i64) -> Option<i64> {
    let s = trim(s);
    if s.is_empty() {
        return None;
    }
    if flags & FILTER_FLAG_ALLOW_HEX != 0 && s.len() >= 2 && s[0] == b'0' && s[1] | 0x20 == b'x' {
        return radix_u64(&s[2..], 16).map(|v| v as i64);
    }
    if flags & FILTER_FLAG_ALLOW_OCTAL != 0 && s[0] == b'0' && s.len() > 1 {
        let d = if s[1] | 0x20 == b'o' {
            &s[2..]
        } else {
            &s[1..]
        };
        return radix_u64(d, 8).map(|v| v as i64);
    }
    let (neg, d) = match s[0] {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    if d.first() == Some(&b'0') {
        return if d.len() == 1 { Some(0) } else { None };
    }
    let v = radix_u64(d, 10)?;
    if neg {
        if v <= 1 << 63 {
            Some((v as i64).wrapping_neg())
        } else {
            None
        }
    } else {
        i64::try_from(v).ok()
    }
}

/// FILTER_VALIDATE_FLOAT without the range options.
///
/// Observed on PHP 8.5 (the fixture's float section; tests 031.phpt,
/// 049.phpt, 050.phpt, bug7715.phpt, bug7733.phpt, bug51368.phpt): an
/// optional sign, an integer part, an optional `decimal` and fraction, an
/// optional exponent `[eE][+-]?digits`; at least one mantissa digit. At
/// each position a digit wins over the decimal character, which wins over
/// an exponent marker, which wins over a thousands separator (allowed with
/// FILTER_FLAG_ALLOW_THOUSAND in the integer part only, between groups of
/// exactly three digits after a first group of one to three). Infinite
/// results fail, and so does a zero result when any digit 1-9 was given,
/// exponent included ("0e1" fails, "0e0" does not). Without a fraction or
/// exponent the value goes through the int range first, so "-0" is 0.0.
pub(crate) fn float(s: &[u8], flags: i64, dec: u8, thousand: &[u8]) -> Option<f64> {
    let s = trim(s);
    let n = s.len();
    let mut i = 0;
    let neg = i < n && s[i] == b'-';
    if i < n && (s[i] == b'-' || s[i] == b'+') {
        i += 1;
    }
    let allow_thousand = flags & FILTER_FLAG_ALLOW_THOUSAND != 0;
    // The mantissa as an integer while it fits (`exact`), with the count of
    // digits after the decimal point.
    let mut m: u64 = 0;
    let mut exact = true;
    let mut nonzero = false;
    // integer part
    let int_start = i;
    let mut int_digits = 0usize;
    let mut group = 0usize; // digits since the last separator
    let mut seps = 0usize;
    while i < n {
        let c = s[i];
        if c.is_ascii_digit() {
            let d = (c - b'0') as u64;
            nonzero |= d != 0;
            match m.checked_mul(10).and_then(|x| x.checked_add(d)) {
                Some(x) => m = x,
                None => exact = false,
            }
            int_digits += 1;
            group += 1;
            i += 1;
        } else if c == dec || c == b'e' || c == b'E' {
            break;
        } else if allow_thousand && thousand.contains(&c) {
            if group == 0 || group > 3 || (seps > 0 && group != 3) {
                return None;
            }
            seps += 1;
            group = 0;
            i += 1;
        } else {
            break;
        }
    }
    if seps > 0 && group != 3 {
        return None;
    }
    let int_end = i;
    let mut frac = (0, 0);
    let mut is_int = true;
    if i < n && s[i] == dec {
        is_int = false;
        i += 1;
        let start = i;
        while i < n && s[i].is_ascii_digit() {
            let d = (s[i] - b'0') as u64;
            nonzero |= d != 0;
            match m.checked_mul(10).and_then(|x| x.checked_add(d)) {
                Some(x) => m = x,
                None => exact = false,
            }
            i += 1;
        }
        frac = (start, i);
    }
    let frac_digits = frac.1 - frac.0;
    if int_digits + frac_digits == 0 {
        return None;
    }
    let mut exp: i64 = 0;
    let mut exp_text = (0, 0);
    if i < n && (s[i] == b'e' || s[i] == b'E') {
        is_int = false;
        i += 1;
        let start = i;
        let eneg = i < n && s[i] == b'-';
        if i < n && (s[i] == b'-' || s[i] == b'+') {
            i += 1;
        }
        let dstart = i;
        while i < n && s[i].is_ascii_digit() {
            nonzero |= s[i] != b'0';
            exp = (exp * 10 + (s[i] - b'0') as i64).min(1 << 40);
            i += 1;
        }
        if i == dstart {
            return None;
        }
        if eneg {
            exp = -exp;
        }
        exp_text = (start, i);
    }
    if i != n {
        return None;
    }
    if is_int && exact && m <= i64::MAX as u64 {
        // within the int range: through the int (no -0.0)
        let v = m as i64;
        return Some((if neg { -v } else { v }) as f64);
    }
    let e10 = exp - frac_digits as i64;
    let f = if exact && m <= 1 << 53 && (-22..=22).contains(&e10) {
        // Both operands are exact, so one IEEE operation rounds correctly.
        const P10: [f64; 23] = [
            1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15,
            1e16, 1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
        ];
        let v = m as f64;
        let v = if e10 >= 0 {
            v * P10[e10 as usize]
        } else {
            v / P10[(-e10) as usize]
        };
        if neg { -v } else { v }
    } else {
        let mut num = Vec::with_capacity(n + 2);
        if neg {
            num.push(b'-');
        }
        num.extend(s[int_start..int_end].iter().filter(|c| c.is_ascii_digit()));
        num.push(b'.');
        num.extend_from_slice(&s[frac.0..frac.1]);
        if int_digits == 0 && frac_digits == 0 {
            return None;
        }
        if exp_text.1 > exp_text.0 {
            num.push(b'e');
            num.extend_from_slice(&s[exp_text.0..exp_text.1]);
        }
        std::str::from_utf8(&num).ok()?.parse::<f64>().ok()?
    };
    if !f.is_finite() || (f == 0.0 && nonzero) {
        return None;
    }
    Some(f)
}

/// FILTER_VALIDATE_BOOL: `Some(true)` for "1", "true", "on", "yes",
/// `Some(false)` for "0", "false", "off", "no" and "", `None` otherwise;
/// ASCII case-insensitive, trimmed (tests 014.phpt, 034.phpt,
/// bug49510.phpt; observed on PHP 8.5).
pub(crate) fn bool(s: &[u8]) -> Option<bool> {
    let s = trim(s);
    if s.len() > 5 {
        return None;
    }
    let mut buf = [0u8; 5];
    for (d, c) in buf.iter_mut().zip(s) {
        *d = c.to_ascii_lowercase();
    }
    match &buf[..s.len()] {
        b"1" | b"true" | b"on" | b"yes" => Some(true),
        b"0" | b"false" | b"off" | b"no" | b"" => Some(false),
        _ => None,
    }
}

type Compiled = Rc<Result<Regex, String>>;

thread_local! {
    /// Compiled patterns, like ext/pcre's cache.
    static REGEX_CACHE: RefCell<HashMap<Vec<u8>, Compiled>> = RefCell::new(HashMap::new());
}

fn compiled(pattern: &[u8]) -> Compiled {
    REGEX_CACHE.with(|c| {
        if let Some(r) = c.borrow().get(pattern) {
            return r.clone();
        }
        let r: Compiled = Rc::new(goro_regex::php::compile(pattern).map_err(|e| e.message()));
        let mut c = c.borrow_mut();
        if c.len() >= 4096 {
            c.clear();
        }
        c.insert(pattern.to_vec(), r.clone());
        r
    })
}

/// FILTER_VALIDATE_REGEXP: whether `pattern` matches somewhere in `s`;
/// `Err` is the compile warning (shown as "filter_var(): <message>", test
/// observed on PHP 8.5). Match errors (backtracking limits, bad UTF-8 with
/// /u) are a non-match.
pub(crate) fn regexp<H: FilterHost>(host: &H, pattern: &[u8], s: &[u8]) -> Result<bool, String> {
    let re = compiled(pattern);
    let re = match &*re {
        Ok(re) => re,
        Err(m) => return Err(m.clone()),
    };
    let mut scratch = Scratch::new();
    let mut caps = Captures::default();
    Ok(re
        .exec_with(
            &mut scratch,
            s,
            0,
            MatchOptions::default(),
            host.pcre_limits(),
            &mut caps,
        )
        .unwrap_or(false))
}

/// FILTER_VALIDATE_DOMAIN (PHP manual: "Validates whether the domain name
/// label lengths are valid"; observed on PHP 8.5, tests 056.phpt,
/// gh16523.phpt): dot-separated non-empty labels of at most 63 bytes, at
/// most 253 bytes in all, one trailing dot allowed. Without
/// FILTER_FLAG_HOSTNAME any byte may appear and the empty string is valid;
/// with it labels are letters, digits and inner hyphens.
pub fn valid_domain(s: &[u8], hostname: bool) -> bool {
    if s.is_empty() {
        return !hostname;
    }
    let s = s.strip_suffix(b".").unwrap_or(s);
    if s.is_empty() || s.len() > 253 {
        return false;
    }
    s.split(|&c| c == b'.').all(|l| {
        !l.is_empty()
            && l.len() <= 63
            && (!hostname
                || (l[0].is_ascii_alphanumeric()
                    && l[l.len() - 1].is_ascii_alphanumeric()
                    && l.iter().all(|&c| c.is_ascii_alphanumeric() || c == b'-')))
    })
}

/// FILTER_VALIDATE_MAC (PHP manual; test 055.phpt, observed on PHP 8.5):
/// six pairs of hex digits separated by ':' or '-' (the same throughout),
/// or three quads separated by '.'; the "separator" option must then be
/// that character.
pub(crate) fn mac(s: &[u8], sep: Option<u8>) -> bool {
    let (sep_char, step, groups) = match s.len() {
        14 => (b'.', 5, 3),
        17 => {
            if s[2] != b':' && s[2] != b'-' {
                return false;
            }
            (s[2], 3, 6)
        }
        _ => return false,
    };
    if sep.is_some_and(|c| c != sep_char) {
        return false;
    }
    (0..groups).all(|g| {
        let start = g * step;
        let digits = &s[start..start + step - 1];
        digits.iter().all(|c| c.is_ascii_hexdigit())
            && (g == groups - 1 || s[start + step - 1] == sep_char)
    })
}
