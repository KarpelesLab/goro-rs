//! Host parsing (URL Standard, "Hosts (domains and IP addresses)") with
//! PHP 8.5's error reporting: errors are reported in the order the parser
//! meets them, IPv4 parts are checked one after the other (so a host with
//! too many parts reports the parts before the fifth first; WPT urltestdata
//! `http://256.256.256.256.256`), a failing domain-to-ASCII is silent, and
//! a percent-decoded ASCII domain keeps its letter case (observed on
//! PHP 8.5: `http://EX%41.com/` serializes as `http://EXA.com/`).

use super::errors::{ErrorType, Errors};
use super::idna;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostKind {
    Domain,
    Ipv4,
    Ipv6,
    Opaque,
    /// The empty host (`file:///x`, `foo://`).
    Empty,
}

/// A parsed host: its kind and its serialization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Host {
    pub kind: HostKind,
    pub text: Vec<u8>,
}

impl Host {
    pub(crate) fn empty() -> Host {
        Host {
            kind: HostKind::Empty,
            text: Vec::new(),
        }
    }
}

/// Forbidden host code points (URL Standard).
#[inline]
pub(crate) fn is_forbidden_host(c: u8) -> bool {
    matches!(
        c,
        0 | b'\t'
            | b'\n'
            | b'\r'
            | b' '
            | b'#'
            | b'/'
            | b':'
            | b'<'
            | b'>'
            | b'?'
            | b'@'
            | b'['
            | b'\\'
            | b']'
            | b'^'
            | b'|'
    )
}

/// Forbidden domain code points: the forbidden host code points, C0
/// controls, `%` and DELETE.
#[inline]
pub(crate) fn is_forbidden_domain(c: u8) -> bool {
    is_forbidden_host(c) || c <= 0x1f || c == b'%' || c == 0x7f
}

/// Where the host input sits in the parser's input, for error contexts.
pub(crate) struct Source<'a> {
    /// The parser's (cleaned) input.
    pub input: &'a [u8],
    /// Offset of the host string in `input`.
    pub start: usize,
}

/// Host parser. `host` is the host string (`input[start..start+len]` for
/// the parser, or a setter's value). Errors are pushed to `errs`.
pub(crate) fn parse_host(
    host: &[u8],
    src: &Source<'_>,
    not_special: bool,
    errs: &mut Errors,
) -> Option<Host> {
    if host.first() == Some(&b'[') {
        if host.last() != Some(&b']') || host.len() < 2 {
            errs.push(
                ErrorType::Ipv6Unclosed,
                src.input,
                src.start + host.len() - 1,
            );
            return None;
        }
        let inner = &host[1..host.len() - 1];
        let a = parse_ipv6(inner, src.input, src.start + 1, errs)?;
        let mut text = Vec::with_capacity(41);
        serialize_ipv6(&a, &mut text);
        return Some(Host {
            kind: HostKind::Ipv6,
            text,
        });
    }
    if not_special {
        return parse_opaque(host, src, errs);
    }
    // Percent-decoding, then UTF-8 decoding (failure is silent).
    let had_percent = host.contains(&b'%');
    let decoded: std::borrow::Cow<'_, [u8]> = if had_percent {
        std::borrow::Cow::Owned(percent_decode(host))
    } else {
        std::borrow::Cow::Borrowed(host)
    };
    let ascii: Vec<u8> = if decoded.is_ascii() && !has_xn_label(&decoded) {
        if had_percent {
            decoded.into_owned()
        } else {
            decoded.to_ascii_lowercase()
        }
    } else {
        let s = lax_utf8(&decoded)?;
        let a = idna::to_ascii(&s)?;
        if a.is_empty() {
            // WPT `https://\u00ad/`: an empty result fails, silently.
            return None;
        }
        a
    };
    if let Some(p) = ascii.iter().position(|&c| is_forbidden_domain(c)) {
        errs.push(ErrorType::DomainInvalidCodePoint, &ascii, p);
        return None;
    }
    if ends_in_number(&ascii) {
        let first = errs.list.len();
        let r = parse_ipv4(&ascii, errs);
        errs.ipv4 = Some(super::errors::Ipv4Errors {
            at: (first..errs.list.len())
                .map(|i| (i, ascii.len() - errs.list[i].context.len()))
                .collect(),
            plain: !had_percent && host.is_ascii(),
            domain: ascii.clone(),
        });
        let v = r?;
        let mut text = Vec::with_capacity(15);
        serialize_ipv4(v, &mut text);
        return Some(Host {
            kind: HostKind::Ipv4,
            text,
        });
    }
    Some(Host {
        kind: if ascii.is_empty() {
            HostKind::Empty
        } else {
            HostKind::Domain
        },
        text: ascii,
    })
}

/// UTF-8 decoding the way PHP 8.5 does it for hosts: a lead byte takes the
/// bytes its length says without checking them (observed: `%e2%98%8` is
/// the domain "\u{2625}8"); a stray byte or a cut sequence fails.
fn lax_utf8(s: &[u8]) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let b = s[i];
        let (need, init) = match b {
            0x00..=0x7F => (1, b as u32),
            0xC0..=0xDF => (2, b as u32 & 0x1F),
            0xE0..=0xEF => (3, b as u32 & 0x0F),
            0xF0..=0xF7 => (4, b as u32 & 0x07),
            _ => return None,
        };
        if i + need > s.len() {
            return None;
        }
        let mut cp = init;
        for &c in &s[i + 1..i + need] {
            cp = cp << 6 | (c as u32 & 0x3F);
        }
        out.push(char::from_u32(cp)?);
        i += need;
    }
    Some(out)
}

fn has_xn_label(s: &[u8]) -> bool {
    s.split(|&c| c == b'.')
        .any(|l| l.len() >= 4 && l[..4].eq_ignore_ascii_case(b"xn--"))
}

pub(crate) fn percent_decode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%'
            && i + 2 < s.len()
            && s[i + 1].is_ascii_hexdigit()
            && s[i + 2].is_ascii_hexdigit()
        {
            out.push(hexval(s[i + 1]) << 4 | hexval(s[i + 2]));
            i += 3;
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}

#[inline]
pub(crate) fn hexval(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        _ => c - b'A' + 10,
    }
}

/// Opaque-host parser.
fn parse_opaque(host: &[u8], src: &Source<'_>, errs: &mut Errors) -> Option<Host> {
    if let Some(p) = host.iter().position(|&c| is_forbidden_host(c)) {
        errs.push(ErrorType::HostInvalidCodePoint, src.input, src.start + p);
        return None;
    }
    // PHP reports no invalid-URL-unit errors here (WPT `sc://%/`).
    let mut text = Vec::with_capacity(host.len());
    for &c in host {
        if c <= 0x1f || c >= 0x7f {
            super::parser::push_pct(&mut text, c);
        } else {
            text.push(c);
        }
    }
    Some(Host {
        kind: if text.is_empty() {
            HostKind::Empty
        } else {
            HostKind::Opaque
        },
        text,
    })
}

/// The "ends in a number" checker.
pub(crate) fn ends_in_number(s: &[u8]) -> bool {
    let mut parts: Vec<&[u8]> = s.split(|&c| c == b'.').collect();
    if parts.last().is_some_and(|p| p.is_empty()) {
        if parts.len() == 1 {
            return false;
        }
        parts.pop();
    }
    let last = parts.last().unwrap();
    if !last.is_empty() && last.iter().all(u8::is_ascii_digit) {
        return true;
    }
    ipv4_number(last).is_some()
}

/// The IPv4 number parser: (value, non-decimal). `None` on failure; the
/// value saturates (only "greater than" comparisons follow).
fn ipv4_number(s: &[u8]) -> Option<(u64, bool)> {
    if s.is_empty() {
        return None;
    }
    let (digits, radix, nondec) = if s.len() >= 2 && (s[..2] == *b"0x" || s[..2] == *b"0X") {
        (&s[2..], 16, true)
    } else if s.len() >= 2 && s[0] == b'0' {
        (&s[1..], 8, true)
    } else {
        (s, 10, false)
    };
    if digits.is_empty() {
        return Some((0, true));
    }
    let mut v: u64 = 0;
    for &c in digits {
        let d = match radix {
            16 if c.is_ascii_hexdigit() => hexval(c) as u64,
            8 if (b'0'..=b'7').contains(&c) => (c - b'0') as u64,
            10 if c.is_ascii_digit() => (c - b'0') as u64,
            _ => return None,
        };
        v = v.saturating_mul(radix).saturating_add(d);
        // A decimal number past 32 bits ends the parse there, as a number
        // too large (observed on PHP 8.5: `a.9999999999b` is an IPv4
        // address with a non-numeric first part, `a.999999999b` a domain).
        if radix == 10 && v > u32::MAX as u64 {
            return Some((v, nondec));
        }
    }
    Some((v, nondec))
}

/// The IPv4 parser, checking the parts in order (see the module docs).
fn parse_ipv4(s: &[u8], errs: &mut Errors) -> Option<u32> {
    let mut numbers: [u64; 4] = [0; 4];
    let mut count = 0usize;
    let mut fail = false;
    let mut pos = 0usize;
    let n = s.len();
    let mut idx = 0usize;
    loop {
        let end = s[pos..]
            .iter()
            .position(|&c| c == b'.')
            .map_or(n, |p| pos + p);
        let part = &s[pos..end];
        let last = end == n;
        if part.is_empty() {
            if last && idx > 0 {
                errs.push(ErrorType::Ipv4EmptyPart, s, pos);
                break;
            }
            errs.push(ErrorType::Ipv4NonNumericPart, s, pos);
            return None;
        }
        if idx >= 4 {
            errs.push(ErrorType::Ipv4TooManyParts, s, pos);
            return None;
        }
        let Some((v, nondec)) = ipv4_number(part) else {
            errs.push(ErrorType::Ipv4NonNumericPart, s, pos);
            return None;
        };
        if nondec {
            errs.push(ErrorType::Ipv4NonDecimalPart, s, pos);
        }
        if v > 255 {
            errs.push(ErrorType::Ipv4OutOfRangePart, s, pos);
        }
        numbers[idx] = v;
        count = idx + 1;
        idx += 1;
        if last {
            break;
        }
        pos = end + 1;
    }
    for &v in &numbers[..count - 1] {
        if v > 255 {
            fail = true;
        }
    }
    let lastv = numbers[count - 1];
    if lastv >= 1u64 << (8 * (5 - count)) {
        fail = true;
    }
    if fail {
        return None;
    }
    let mut ipv4 = lastv;
    for (i, &v) in numbers[..count - 1].iter().enumerate() {
        ipv4 += v << (8 * (3 - i));
    }
    Some(ipv4 as u32)
}

pub(crate) fn serialize_ipv4(v: u32, out: &mut Vec<u8>) {
    for (i, sh) in [24, 16, 8, 0].iter().enumerate() {
        if i > 0 {
            out.push(b'.');
        }
        let b = (v >> sh) as u8;
        if b >= 100 {
            out.push(b'0' + b / 100);
        }
        if b >= 10 {
            out.push(b'0' + b / 10 % 10);
        }
        out.push(b'0' + b % 10);
    }
}

/// The IPv6 parser; `at` is the offset of `s` in `input` (error contexts).
fn parse_ipv6(s: &[u8], input: &[u8], at: usize, errs: &mut Errors) -> Option<[u16; 8]> {
    let mut address = [0u16; 8];
    let mut piece = 0usize;
    let mut compress: Option<usize> = None;
    let mut p = 0usize;
    let n = s.len();
    let c = |p: usize| s.get(p).copied();
    macro_rules! fail {
        ($t:expr, $p:expr) => {{
            errs.push($t, input, at + $p);
            return None;
        }};
    }
    if c(p) == Some(b':') {
        if c(p + 1) != Some(b':') {
            // PHP's context starts after the ':' unless the input ends
            // there (WPT `data://[:1]`, `http://[:]`).
            fail!(
                ErrorType::Ipv6InvalidCompression,
                if p + 1 < n { p + 1 } else { p }
            );
        }
        p += 2;
        piece += 1;
        compress = Some(piece);
    }
    while p < n {
        if piece == 8 {
            fail!(ErrorType::Ipv6TooManyPieces, p);
        }
        if c(p) == Some(b':') {
            if compress.is_some() {
                fail!(ErrorType::Ipv6MultipleCompression, p);
            }
            p += 1;
            piece += 1;
            compress = Some(piece);
            continue;
        }
        let mut value: u32 = 0;
        let mut length = 0;
        while length < 4 && p < n && s[p].is_ascii_hexdigit() {
            value = value * 16 + hexval(s[p]) as u32;
            p += 1;
            length += 1;
        }
        if c(p) == Some(b'.') {
            if length == 0 {
                fail!(ErrorType::Ipv4InIpv6InvalidCodePoint, p);
            }
            p -= length;
            if piece > 6 {
                fail!(ErrorType::Ipv4InIpv6TooManyPieces, p);
            }
            let mut seen = 0;
            while p < n {
                let mut v4: Option<u32> = None;
                if seen > 0 {
                    if s[p] == b'.' && seen < 4 {
                        p += 1;
                        // PHP stops at the end here and reports too few
                        // parts (WPT `http://[::1.2.3.]`).
                        if p >= n {
                            break;
                        }
                    } else {
                        fail!(ErrorType::Ipv4InIpv6InvalidCodePoint, p);
                    }
                }
                if p >= n || !s[p].is_ascii_digit() {
                    fail!(ErrorType::Ipv4InIpv6InvalidCodePoint, p);
                }
                while p < n && s[p].is_ascii_digit() {
                    let d = (s[p] - b'0') as u32;
                    v4 = match v4 {
                        None => Some(d),
                        Some(0) => fail!(ErrorType::Ipv4InIpv6InvalidCodePoint, p),
                        Some(v) => Some(v * 10 + d),
                    };
                    if v4.unwrap() > 255 {
                        fail!(ErrorType::Ipv4InIpv6OutOfRangePart, p);
                    }
                    p += 1;
                }
                address[piece] = (address[piece] as u32 * 0x100 + v4.unwrap()) as u16;
                seen += 1;
                if seen == 2 || seen == 4 {
                    piece += 1;
                }
            }
            if seen != 4 {
                fail!(ErrorType::Ipv4InIpv6TooFewParts, p);
            }
            break;
        } else if c(p) == Some(b':') {
            p += 1;
            if p >= n {
                fail!(ErrorType::Ipv6InvalidCodePoint, p);
            }
        } else if p < n {
            fail!(ErrorType::Ipv6InvalidCodePoint, p);
        }
        address[piece] = value as u16;
        piece += 1;
    }
    if let Some(comp) = compress {
        let mut swaps = piece - comp;
        piece = 7;
        while piece != 0 && swaps > 0 {
            address.swap(piece, comp + swaps - 1);
            piece -= 1;
            swaps -= 1;
        }
    } else if piece != 8 {
        fail!(ErrorType::Ipv6TooFewPieces, p);
    }
    Some(address)
}

pub(crate) fn serialize_ipv6(a: &[u16; 8], out: &mut Vec<u8>) {
    // The first longest run of two or more zero pieces is compressed.
    let mut best: Option<(usize, usize)> = None;
    let mut i = 0;
    while i < 8 {
        if a[i] == 0 {
            let start = i;
            while i < 8 && a[i] == 0 {
                i += 1;
            }
            let len = i - start;
            if len >= 2 && best.is_none_or(|(_, l)| len > l) {
                best = Some((start, len));
            }
        } else {
            i += 1;
        }
    }
    out.push(b'[');
    let compress = best.map(|(s, _)| s);
    let mut ignore0 = false;
    for (i, &v) in a.iter().enumerate() {
        if ignore0 && v == 0 {
            continue;
        }
        ignore0 = false;
        if compress == Some(i) {
            out.extend_from_slice(if i == 0 { b"::" } else { b":" });
            ignore0 = true;
            continue;
        }
        let mut started = false;
        for sh in [12, 8, 4, 0] {
            let d = (v >> sh) & 15;
            if d != 0 || started || sh == 0 {
                started = true;
                out.push(b"0123456789abcdef"[d as usize]);
            }
        }
        if i != 7 {
            out.push(b':');
        }
    }
    out.push(b']');
}
