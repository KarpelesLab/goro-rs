//! IDNA with UTS #46 processing (`idn_to_ascii()`, `idn_to_utf8()`; PHP
//! manual: IDN functions): the Unicode 17 mapping table
//! (tools/gen_intl_uts46.py), NFC, Punycode (RFC 3492), the validity
//! criteria of UTS #46 section 4.1, CheckBidi (RFC 5893) and CheckJoiners
//! (RFC 5892 appendix A), with ICU's UIDNA_ERROR_* bits (ICU API
//! documentation, uidna.h).

use std::sync::OnceLock;

// Options (PHP's IDNA_* constants).
pub const USE_STD3_RULES: i64 = 2;
pub const CHECK_BIDI: i64 = 4;
pub const CHECK_CONTEXTJ: i64 = 8;
pub const NONTRANSITIONAL_TO_ASCII: i64 = 16;
pub const NONTRANSITIONAL_TO_UNICODE: i64 = 32;
pub const CHECK_CONTEXTO: i64 = 64;

// Error bits.
pub const ERROR_EMPTY_LABEL: i64 = 1;
pub const ERROR_LABEL_TOO_LONG: i64 = 2;
pub const ERROR_DOMAIN_NAME_TOO_LONG: i64 = 4;
pub const ERROR_LEADING_HYPHEN: i64 = 8;
pub const ERROR_TRAILING_HYPHEN: i64 = 0x10;
pub const ERROR_HYPHEN_3_4: i64 = 0x20;
pub const ERROR_LEADING_COMBINING_MARK: i64 = 0x40;
pub const ERROR_DISALLOWED: i64 = 0x80;
pub const ERROR_PUNYCODE: i64 = 0x100;
pub const ERROR_LABEL_HAS_DOT: i64 = 0x200;
pub const ERROR_INVALID_ACE_LABEL: i64 = 0x400;
pub const ERROR_BIDI: i64 = 0x800;
pub const ERROR_CONTEXTJ: i64 = 0x1000;
pub const ERROR_CONTEXTO_PUNCTUATION: i64 = 0x2000;
pub const ERROR_CONTEXTO_DIGITS: i64 = 0x4000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum St {
    Valid,
    Mapped(u32, u8),
    Ignored,
    Disallowed,
    Deviation(u32, u8),
}

struct Table {
    starts: Vec<u32>,
    st: Vec<St>,
    maps: Vec<char>,
}

fn table() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = Table {
            starts: Vec::new(),
            st: Vec::new(),
            maps: Vec::new(),
        };
        for line in include_str!("data/uts46.txt").lines() {
            if line.starts_with('#') {
                continue;
            }
            let mut it = line.split(' ');
            let (Some(cp), Some(s)) = (it.next(), it.next()) else {
                continue;
            };
            let cp = u32::from_str_radix(cp, 16).unwrap_or(0);
            let off = t.maps.len() as u32;
            for m in it {
                if let Some(c) = u32::from_str_radix(m, 16).ok().and_then(char::from_u32) {
                    t.maps.push(c);
                }
            }
            let len = (t.maps.len() as u32 - off) as u8;
            let st = match s {
                "v" => St::Valid,
                "m" => St::Mapped(off, len),
                "i" => St::Ignored,
                "x" => St::Deviation(off, len),
                _ => St::Disallowed,
            };
            t.starts.push(cp);
            t.st.push(st);
        }
        t
    })
}

fn status(c: char) -> St {
    let t = table();
    let cp = c as u32;
    let i = match t.starts.binary_search(&cp) {
        Ok(i) => i,
        Err(0) => return St::Disallowed,
        Err(i) => i - 1,
    };
    match t.st[i] {
        // merged ranges only hold statuses without mappings
        St::Mapped(_, l) | St::Deviation(_, l) if l > 0 && t.starts[i] != cp => St::Disallowed,
        s => s,
    }
}

fn mapping(off: u32, len: u8) -> &'static [char] {
    &table().maps[off as usize..off as usize + len as usize]
}

/// The result of a conversion.
#[derive(Debug, Clone, Default)]
pub struct IdnaResult {
    pub result: String,
    pub errors: i64,
    pub is_transitional_different: bool,
}

// ---- Punycode (RFC 3492) ----

const BASE: u32 = 36;
const TMIN: u32 = 1;
const TMAX: u32 = 26;
const SKEW: u32 = 38;
const DAMP: u32 = 700;
const INITIAL_BIAS: u32 = 72;
const INITIAL_N: u32 = 128;

fn adapt(mut delta: u32, num_points: u32, first: bool) -> u32 {
    delta /= if first { DAMP } else { 2 };
    delta += delta / num_points;
    let mut k = 0;
    while delta > ((BASE - TMIN) * TMAX) / 2 {
        delta /= BASE - TMIN;
        k += BASE;
    }
    k + (BASE - TMIN + 1) * delta / (delta + SKEW)
}

pub fn punycode_encode(input: &[char]) -> Option<String> {
    let mut out = String::new();
    let mut basic = 0u32;
    for &c in input {
        if (c as u32) < 0x80 {
            out.push(c);
            basic += 1;
        }
    }
    let mut h = basic;
    if basic > 0 {
        out.push('-');
    }
    let mut n = INITIAL_N;
    let mut delta: u32 = 0;
    let mut bias = INITIAL_BIAS;
    let total = input.len() as u32;
    while h < total {
        let m = input.iter().map(|&c| c as u32).filter(|&c| c >= n).min()?;
        delta = delta.checked_add((m - n).checked_mul(h + 1)?)?;
        n = m;
        for &c in input {
            let c = c as u32;
            if c < n {
                delta = delta.checked_add(1)?;
            }
            if c == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let t = if k <= bias {
                        TMIN
                    } else if k >= bias + TMAX {
                        TMAX
                    } else {
                        k - bias
                    };
                    if q < t {
                        break;
                    }
                    let d = t + (q - t) % (BASE - t);
                    out.push(digit(d));
                    q = (q - t) / (BASE - t);
                    k += BASE;
                }
                out.push(digit(q));
                bias = adapt(delta, h + 1, h == basic);
                delta = 0;
                h += 1;
            }
        }
        delta += 1;
        n += 1;
    }
    Some(out)
}

fn digit(d: u32) -> char {
    if d < 26 {
        (b'a' + d as u8) as char
    } else {
        (b'0' + (d - 26) as u8) as char
    }
}

pub fn punycode_decode(input: &str) -> Option<Vec<char>> {
    let (basic, rest) = match input.rfind('-') {
        Some(i) => (&input[..i], &input[i + 1..]),
        None => ("", input),
    };
    if !basic.is_ascii() {
        return None;
    }
    let mut out: Vec<char> = basic.chars().collect();
    let mut n = INITIAL_N;
    let mut i: u32 = 0;
    let mut bias = INITIAL_BIAS;
    let bytes = rest.as_bytes();
    let mut p = 0;
    while p < bytes.len() {
        let old_i = i;
        let mut w: u32 = 1;
        let mut k = BASE;
        loop {
            let b = *bytes.get(p)?;
            p += 1;
            let d = match b {
                b'a'..=b'z' => b - b'a',
                b'A'..=b'Z' => b - b'A',
                b'0'..=b'9' => b - b'0' + 26,
                _ => return None,
            } as u32;
            i = i.checked_add(d.checked_mul(w)?)?;
            let t = if k <= bias {
                TMIN
            } else if k >= bias + TMAX {
                TMAX
            } else {
                k - bias
            };
            if d < t {
                break;
            }
            w = w.checked_mul(BASE - t)?;
            k += BASE;
        }
        let len = out.len() as u32 + 1;
        bias = adapt(i - old_i, len, old_i == 0);
        n = n.checked_add(i / len)?;
        i %= len;
        let c = char::from_u32(n)?;
        out.insert(i as usize, c);
        i += 1;
    }
    Some(out)
}

// ---- processing ----

fn is_bidi_rtl(c: char) -> bool {
    use intl::unicode::bidi::BidiClass as B;
    matches!(intl::unicode::bidi::bidi_class(c), B::R | B::AL | B::AN)
}

fn check_bidi(label: &[char]) -> bool {
    use intl::unicode::bidi::BidiClass as B;
    use intl::unicode::bidi::bidi_class;
    let Some(&first) = label.first() else {
        return true;
    };
    let fc = bidi_class(first);
    let last = label.iter().rev().map(|&c| bidi_class(c)).find(|b| *b != B::NSM);
    if fc == B::R || fc == B::AL {
        // B2, B3, B4
        if !label.iter().all(|&c| {
            matches!(
                bidi_class(c),
                B::R | B::AL | B::AN | B::EN | B::ES | B::CS | B::ET | B::ON | B::BN | B::NSM
            )
        }) {
            return false;
        }
        if !matches!(last, Some(B::R | B::AL | B::EN | B::AN)) {
            return false;
        }
        let en = label.iter().any(|&c| bidi_class(c) == B::EN);
        let an = label.iter().any(|&c| bidi_class(c) == B::AN);
        !(en && an)
    } else if fc == B::L {
        label.iter().all(|&c| {
            matches!(
                bidi_class(c),
                B::L | B::EN | B::ES | B::CS | B::ET | B::ON | B::BN | B::NSM
            )
        }) && matches!(last, Some(B::L | B::EN))
    } else {
        false
    }
}

/// CheckJoiners (RFC 5892 appendix A.1/A.2), the virama rule.
fn check_joiners(label: &[char]) -> bool {
    for (i, &c) in label.iter().enumerate() {
        if c == '\u{200C}' || c == '\u{200D}' {
            let prev_virama = i > 0 && intl::unicode::canonical_combining_class(label[i - 1]) == 9;
            if !prev_virama {
                return false;
            }
        }
    }
    true
}

fn is_mark(c: char) -> bool {
    use intl::unicode::GeneralCategory as G;
    matches!(
        intl::unicode::general_category(c),
        G::NonspacingMark | G::SpacingMark | G::EnclosingMark
    )
}

/// Maps and normalizes (UTS #46 section 4 steps 1-2), recording
/// disallowed characters and deviations.
fn map(input: &str, transitional: bool, std3: bool, errors: &mut i64, deviation: &mut bool) -> String {
    let mut out = String::new();
    for c in input.chars() {
        match status(c) {
            St::Valid => {
                if std3 && c.is_ascii() && !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.') {
                    *errors |= ERROR_DISALLOWED;
                }
                out.push(c);
            }
            St::Mapped(o, l) => {
                for &m in mapping(o, l) {
                    if std3 && m.is_ascii() && !(m.is_ascii_lowercase() || m.is_ascii_digit() || m == '-' || m == '.') {
                        *errors |= ERROR_DISALLOWED;
                    }
                    out.push(m);
                }
            }
            St::Ignored => {}
            St::Deviation(o, l) => {
                *deviation = true;
                if transitional {
                    out.extend(mapping(o, l));
                } else {
                    out.push(c);
                }
            }
            St::Disallowed => {
                *errors |= ERROR_DISALLOWED;
                out.push(c);
            }
        }
    }
    intl::unicode::nfc(out.chars()).collect()
}

/// Validates a label (UTS #46 section 4.1).
fn validate(label: &[char], transitional: bool, opts: i64, bidi_domain: bool, errors: &mut i64) {
    if label.is_empty() {
        return;
    }
    let s: String = label.iter().collect();
    let nfc: String = intl::unicode::nfc(s.chars()).collect();
    if nfc != s {
        *errors |= ERROR_INVALID_ACE_LABEL;
    }
    if label.len() >= 4 && label[2] == '-' && label[3] == '-' {
        *errors |= ERROR_HYPHEN_3_4;
    }
    if label[0] == '-' {
        *errors |= ERROR_LEADING_HYPHEN;
    }
    if *label.last().unwrap() == '-' {
        *errors |= ERROR_TRAILING_HYPHEN;
    }
    if label.contains(&'.') {
        *errors |= ERROR_LABEL_HAS_DOT;
    }
    if is_mark(label[0]) {
        *errors |= ERROR_LEADING_COMBINING_MARK;
    }
    for &c in label {
        match status(c) {
            St::Valid => {}
            St::Deviation(..) if !transitional => {}
            _ => *errors |= ERROR_DISALLOWED,
        }
    }
    if opts & CHECK_CONTEXTJ != 0 && !check_joiners(label) {
        *errors |= ERROR_CONTEXTJ;
    }
    if opts & CHECK_BIDI != 0 && bidi_domain && !check_bidi(label) {
        *errors |= ERROR_BIDI;
    }
}

fn process(domain: &str, opts: i64, to_ascii: bool) -> IdnaResult {
    let transitional = if to_ascii {
        opts & NONTRANSITIONAL_TO_ASCII == 0
    } else {
        opts & NONTRANSITIONAL_TO_UNICODE == 0
    };
    let std3 = opts & USE_STD3_RULES != 0;
    let mut errors = 0;
    let mut deviation = false;
    let mapped = map(domain, transitional, std3, &mut errors, &mut deviation);
    // labels, with xn-- labels decoded
    let mut labels: Vec<(Vec<char>, bool)> = Vec::new();
    for l in mapped.split('.') {
        if l.len() >= 4 && l[..4].eq_ignore_ascii_case("xn--") {
            match punycode_decode(&l[4..]) {
                Some(d) if !d.is_empty() && !l.ends_with('-') => {
                    // a decoded label must be NFC and its characters valid
                    let s: String = d.iter().collect();
                    let nfc: String = intl::unicode::nfc(s.chars()).collect();
                    if nfc != s || d.iter().all(char::is_ascii) {
                        errors |= ERROR_INVALID_ACE_LABEL;
                    }
                    labels.push((d, true));
                }
                _ => {
                    errors |= ERROR_PUNYCODE;
                    labels.push((l.chars().collect(), false));
                }
            }
        } else {
            labels.push((l.chars().collect(), false));
        }
    }
    let bidi_domain = labels.iter().any(|(l, _)| l.iter().any(|&c| is_bidi_rtl(c)));
    let n = labels.len();
    for (i, (l, ace)) in labels.iter().enumerate() {
        if l.is_empty() {
            // the root label (a trailing dot) is allowed
            if i + 1 != n || n == 1 {
                errors |= ERROR_EMPTY_LABEL;
            }
            continue;
        }
        validate(l, transitional && !ace, opts, bidi_domain, &mut errors);
    }
    let mut out = String::new();
    for (i, (l, ace)) in labels.iter().enumerate() {
        if i > 0 {
            out.push('.');
        }
        if to_ascii {
            if l.iter().all(char::is_ascii) && !ace {
                let s: String = l.iter().collect();
                out.push_str(&s);
            } else {
                match punycode_encode(l) {
                    Some(p) => {
                        out.push_str("xn--");
                        out.push_str(&p);
                    }
                    None => errors |= ERROR_PUNYCODE,
                }
            }
        } else {
            out.extend(l.iter());
        }
    }
    if to_ascii {
        for l in out.split('.') {
            if l.len() > 63 {
                errors |= ERROR_LABEL_TOO_LONG;
            }
        }
        let body = out.strip_suffix('.').unwrap_or(&out);
        if body.len() > 253 {
            errors |= ERROR_DOMAIN_NAME_TOO_LONG;
        }
    }
    IdnaResult {
        result: out,
        errors,
        is_transitional_different: deviation,
    }
}

/// UTS #46 ToASCII with PHP's option bits.
pub fn to_ascii(domain: &str, opts: i64) -> IdnaResult {
    process(domain, opts, true)
}

/// UTS #46 ToUnicode with PHP's option bits.
pub fn to_unicode(domain: &str, opts: i64) -> IdnaResult {
    process(domain, opts, false)
}
