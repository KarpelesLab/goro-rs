//! Domain to ASCII / to Unicode as PHP 8.5's WHATWG URL parser does it.
//!
//! The processing is UTS #46 (Unicode 16.0 mapping table, nontransitional,
//! UseSTD3ASCIIRules false) followed by NFC, as the URL Standard asks, with
//! the validity criteria PHP 8.5 applies (observed on PHP 8.5 over every
//! code point and over the WPT IdnaTestV2/toascii inputs):
//!
//! - labels written in Unicode are not checked further (no CheckBidi, no
//!   CheckJoiners, no leading-combining-mark or NFC check);
//! - an `xn--` label (after mapping, so `XN--` too) is Punycode-decoded;
//!   it fails when the Punycode is invalid, decodes to nothing, decodes to
//!   a code point whose UTS #46 status is not "valid", or decodes to a label
//!   that itself starts with `xn--`. The label is then written back from its
//!   decoded form (`xn--ASCII-` becomes `ascii`).
//!
//! A failure of either function is silent: PHP reports no validation error
//! for it (WPT urltestdata `http://a.b.c.xn--pokxncvks`).
//!
//! Punycode is RFC 3492.

use super::idna_table::{MAPPINGS, RANGES};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Status {
    Valid,
    Mapped(&'static [u8]),
    Ignored,
    Disallowed,
}

pub(crate) fn status(cp: u32) -> Status {
    let i = match RANGES.binary_search_by(|&(start, _)| start.cmp(&cp)) {
        Ok(i) => i,
        Err(i) => i - 1,
    };
    let v = RANGES[i].1;
    match v >> 30 {
        0 => Status::Valid,
        1 => {
            let off = ((v >> 8) & 0x3f_ffff) as usize;
            let len = (v & 0xff) as usize;
            Status::Mapped(&MAPPINGS[off..off + len])
        }
        2 => Status::Ignored,
        _ => Status::Disallowed,
    }
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

fn digit(d: u32) -> u8 {
    if d < 26 {
        b'a' + d as u8
    } else {
        b'0' + (d - 26) as u8
    }
}

/// Encodes a label (the part after `xn--`).
pub(crate) fn punycode_encode(input: &[char], out: &mut Vec<u8>) -> Option<()> {
    let mut n = INITIAL_N;
    let mut delta: u32 = 0;
    let mut bias = INITIAL_BIAS;
    let mut basic = 0u32;
    for &c in input {
        if c.is_ascii() {
            out.push(c as u8);
            basic += 1;
        }
    }
    let mut h = basic;
    if basic > 0 {
        out.push(b'-');
    }
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
                    out.push(digit(t + (q - t) % (BASE - t)));
                    q = (q - t) / (BASE - t);
                    k += BASE;
                }
                out.push(digit(q));
                bias = adapt(delta, h + 1, h == basic);
                delta = 0;
                h += 1;
            }
        }
        delta = delta.checked_add(1)?;
        n = n.checked_add(1)?;
    }
    Some(())
}

/// Decodes a label (the part after `xn--`).
pub(crate) fn punycode_decode(input: &[u8]) -> Option<Vec<char>> {
    let (basic, rest) = match input.iter().rposition(|&c| c == b'-') {
        Some(p) => (&input[..p], &input[p + 1..]),
        None => (&input[..0], input),
    };
    let mut out: Vec<u32> = Vec::with_capacity(input.len());
    for &c in basic {
        if !c.is_ascii() {
            return None;
        }
        out.push(c as u32);
    }
    let mut n = INITIAL_N;
    let mut i: u32 = 0;
    let mut bias = INITIAL_BIAS;
    let mut p = 0;
    while p < rest.len() {
        let old_i = i;
        let mut w: u32 = 1;
        let mut k = BASE;
        loop {
            let c = *rest.get(p)?;
            p += 1;
            let d = match c {
                b'a'..=b'z' => (c - b'a') as u32,
                b'A'..=b'Z' => (c - b'A') as u32,
                b'0'..=b'9' => (c - b'0') as u32 + 26,
                _ => return None,
            };
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
        if n > 0x10FFFF || (0xD800..=0xDFFF).contains(&n) {
            return None;
        }
        out.insert(i as usize, n);
        i += 1;
    }
    out.into_iter().map(char::from_u32).collect()
}

fn starts_with_xn(label: &[u8]) -> bool {
    label.len() >= 4 && label[..4].eq_ignore_ascii_case(b"xn--")
}

/// Domain to ASCII for a host that is not plain ASCII without `xn--`
/// labels (those are only lowercased by the caller). `domain` is UTF-8.
pub(crate) fn to_ascii(domain: &str) -> Option<Vec<u8>> {
    // UTS #46 mapping.
    let mut mapped = String::with_capacity(domain.len());
    for c in domain.chars() {
        match status(c as u32) {
            Status::Valid => mapped.push(c),
            Status::Mapped(m) => mapped.push_str(std::str::from_utf8(m).ok()?),
            Status::Ignored => {}
            Status::Disallowed => return None,
        }
    }
    let normalized: String = if mapped.is_ascii() {
        mapped
    } else {
        intl::unicode::nfc(mapped.chars()).collect()
    };
    let mut out = Vec::with_capacity(normalized.len() + 8);
    let mut chars: Vec<char> = Vec::new();
    for (i, label) in normalized.split('.').enumerate() {
        if i > 0 {
            out.push(b'.');
        }
        chars.clear();
        if starts_with_xn(label.as_bytes()) {
            let decoded = punycode_decode(&label.as_bytes()[4..])?;
            if decoded.is_empty() || !valid_decoded(&decoded) {
                return None;
            }
            chars.extend(decoded);
        } else {
            chars.extend(label.chars());
        }
        if chars.iter().all(char::is_ascii) {
            out.extend(chars.iter().map(|&c| c as u8));
        } else {
            out.extend_from_slice(b"xn--");
            punycode_encode(&chars, &mut out)?;
        }
    }
    Some(out)
}

/// The checks on a decoded `xn--` label.
fn valid_decoded(decoded: &[char]) -> bool {
    if decoded.len() >= 4
        && decoded[..4]
            .iter()
            .zip("xn--".chars())
            .all(|(a, b)| a.to_ascii_lowercase() == b)
    {
        return false;
    }
    decoded.iter().all(|&c| status(c as u32) == Status::Valid)
}

/// Domain to Unicode of a serialized host (`getUnicodeHost()`): ASCII
/// letters lowercased and `xn--` labels decoded. `None` when a label does
/// not decode to valid code points (PHP then gives an empty host and stops
/// the Unicode serialization there; observed on PHP 8.5 with
/// `foo://xn--zz/`).
pub(crate) fn to_unicode(host: &[u8]) -> Option<Vec<u8>> {
    let lower = host.to_ascii_lowercase();
    if !lower.windows(4).any(|w| w == b"xn--") {
        return Some(lower);
    }
    let mut out = Vec::with_capacity(lower.len() + 8);
    for (i, label) in lower.split(|&c| c == b'.').enumerate() {
        if i > 0 {
            out.push(b'.');
        }
        if starts_with_xn(label) {
            let decoded = punycode_decode(&label[4..])?;
            if !decoded.is_empty() && !valid_decoded(&decoded) {
                return None;
            }
            let mut b = [0u8; 4];
            for c in decoded {
                out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
            }
        } else {
            out.extend_from_slice(label);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punycode_rfc3492_samples() {
        let enc = |s: &str| {
            let v: Vec<char> = s.chars().collect();
            let mut o = Vec::new();
            punycode_encode(&v, &mut o).unwrap();
            String::from_utf8(o).unwrap()
        };
        assert_eq!(enc("bücher"), "bcher-kva");
        assert_eq!(enc("💩"), "ls8h");
        assert_eq!(enc("你好你好"), "6qqa088eba");
        let dec =
            |s: &str| punycode_decode(s.as_bytes()).map(|v| v.into_iter().collect::<String>());
        assert_eq!(dec("bcher-kva").as_deref(), Some("bücher"));
        assert_eq!(dec("ascii-").as_deref(), Some("ascii"));
        assert_eq!(dec("").as_deref(), Some(""));
        assert_eq!(dec("ls8h").as_deref(), Some("💩"));
    }

    #[test]
    fn to_ascii_samples() {
        let a = |s: &str| to_ascii(s).map(|v| String::from_utf8(v).unwrap());
        // tests 005.phpt, 011.phpt, 019.phpt
        assert_eq!(a("héééostname").as_deref(), Some("xn--hostname-b1aaa"));
        assert_eq!(a("你好你好").as_deref(), Some("xn--6qqa088eba"));
        assert_eq!(a("🐘.com").as_deref(), Some("xn--go8h.com"));
        assert_eq!(a("xn--ASCII-").as_deref(), Some("ascii"));
        assert_eq!(a("xn--"), None);
        assert_eq!(a("a\u{200d}b").as_deref(), Some("xn--ab-m1t"));
    }
}
