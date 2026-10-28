//! FILTER_VALIDATE_IP: IPv4 and IPv6 syntax and the range flags.

use crate::consts::*;

/// A parsed IP address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ip {
    V4([u8; 4]),
    /// The eight groups; `embedded_v4` when the last 32 bits were written as
    /// a dotted IPv4 address.
    V6 {
        groups: [u16; 8],
        embedded_v4: bool,
    },
}

/// Dotted-quad IPv4 as FILTER_VALIDATE_IP accepts it: four decimal parts of
/// at most 255 without leading zeros (tests 018.phpt, 019.phpt,
/// bug39846.phpt; observed on PHP 8.5: "01.2.3.4" and "1.2.3.04" fail).
pub(crate) fn parse_v4(s: &[u8]) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut part = 0;
    let mut v = 0u32;
    let mut len = 0;
    for &c in s {
        if c == b'.' {
            if len == 0 || part == 3 {
                return None;
            }
            out[part] = v as u8;
            part += 1;
            v = 0;
            len = 0;
        } else {
            if !c.is_ascii_digit() || (len == 1 && v == 0) {
                return None;
            }
            v = v * 10 + (c - b'0') as u32;
            len += 1;
            if v > 255 {
                return None;
            }
        }
    }
    if len == 0 || part != 3 {
        return None;
    }
    out[3] = v as u8;
    Some(out)
}

#[inline]
fn hex_digit(c: u8) -> Option<u16> {
    match c {
        b'0'..=b'9' => Some((c - b'0') as u16),
        b'a'..=b'f' => Some((c - b'a' + 10) as u16),
        b'A'..=b'F' => Some((c - b'A' + 10) as u16),
        _ => None,
    }
}

/// IPv6 text form, in one pass. `max_comp` is the most groups written
/// explicitly with `::` (7 for FILTER_VALIDATE_IP per RFC 4291 section 2.2;
/// 6 in e-mail address literals per RFC 5321 section 4.1.3, which also caps
/// the groups before an embedded IPv4 address at 4 instead of 5). Groups
/// are one to four hex digits; a dotted IPv4 address may stand for the last
/// two.
pub(crate) fn parse_v6(s: &[u8], max_comp: usize, max_comp_v4: usize) -> Option<([u16; 8], bool)> {
    let n = s.len();
    let mut g = [0u16; 8];
    let mut ng = 0usize;
    let mut gap: Option<usize> = None;
    let mut v4 = false;
    let mut i = 0;
    if n >= 2 && s[0] == b':' && s[1] == b':' {
        gap = Some(0);
        i = 2;
    }
    if i < n {
        loop {
            let start = i;
            let mut v = 0u16;
            while i < n && i - start < 5 {
                match hex_digit(s[i]) {
                    Some(d) => v = v << 4 | d,
                    None => break,
                }
                i += 1;
            }
            if i < n && s[i] == b'.' {
                let a = parse_v4(&s[start..])?;
                if ng > 6 {
                    return None;
                }
                g[ng] = u16::from_be_bytes([a[0], a[1]]);
                g[ng + 1] = u16::from_be_bytes([a[2], a[3]]);
                ng += 2;
                v4 = true;
                break;
            }
            let len = i - start;
            if len == 0 || len > 4 || ng == 8 {
                return None;
            }
            g[ng] = v;
            ng += 1;
            if i == n {
                break;
            }
            if s[i] != b':' {
                return None;
            }
            i += 1;
            if i == n {
                return None;
            }
            if s[i] == b':' {
                if gap.is_some() {
                    return None;
                }
                gap = Some(ng);
                i += 1;
                if i == n {
                    break;
                }
            }
        }
    }
    match gap {
        Some(at) => {
            let limit = if v4 { max_comp_v4 + 2 } else { max_comp };
            if ng > limit {
                return None;
            }
            // move the groups after the gap to the end
            let tail = ng - at;
            g.copy_within(at..ng, 8 - tail);
            g[at..8 - tail].fill(0);
        }
        None => {
            if ng != 8 {
                return None;
            }
        }
    }
    Some((g, v4))
}

/// Parses an IPv4 or IPv6 address as FILTER_VALIDATE_IP does: a ':' makes
/// it IPv6, else a '.' IPv4.
pub fn parse_ip(s: &[u8]) -> Option<Ip> {
    if s.contains(&b':') {
        parse_v6(s, 7, 5).map(|(groups, embedded_v4)| Ip::V6 {
            groups,
            embedded_v4,
        })
    } else {
        parse_v4(s).map(Ip::V4)
    }
}

fn in_v4(a: u32, net: u32, len: u32) -> bool {
    let mask = if len == 0 { 0 } else { u32::MAX << (32 - len) };
    a & mask == net
}

fn in_v6(a: u128, net: u128, len: u32) -> bool {
    let mask = if len == 0 {
        0
    } else {
        u128::MAX << (128 - len)
    };
    a & mask == net
}

const fn v4(a: u8, b: u8, c: u8, d: u8) -> u32 {
    u32::from_be_bytes([a, b, c, d])
}

/// FILTER_FLAG_NO_PRIV_RANGE for IPv4 (RFC 1918).
const V4_PRIV: [(u32, u32); 3] = [
    (v4(10, 0, 0, 0), 8),
    (v4(172, 16, 0, 0), 12),
    (v4(192, 168, 0, 0), 16),
];
/// FILTER_FLAG_NO_RES_RANGE for IPv4 (observed on PHP 8.5 at every block
/// boundary of the IANA IPv4 special-purpose registry: "this network",
/// loopback, link local, and 240.0.0.0/4 with the broadcast address).
const V4_RES: [(u32, u32); 4] = [
    (v4(0, 0, 0, 0), 8),
    (v4(127, 0, 0, 0), 8),
    (v4(169, 254, 0, 0), 16),
    (v4(240, 0, 0, 0), 4),
];
/// The further blocks FILTER_FLAG_GLOBAL_RANGE rejects (test
/// bug77221.phpt; RFC 6890 blocks not marked globally reachable).
const V4_NON_GLOBAL: [(u32, u32); 6] = [
    (v4(100, 64, 0, 0), 10),
    (v4(192, 0, 0, 0), 24),
    (v4(192, 0, 2, 0), 24),
    (v4(198, 18, 0, 0), 15),
    (v4(198, 51, 100, 0), 24),
    (v4(203, 0, 113, 0), 24),
];

/// FILTER_FLAG_NO_PRIV_RANGE for IPv6: unique local addresses (RFC 4193).
const V6_PRIV: [(u128, u32); 1] = [(0xfc00 << 112, 7)];
/// FILTER_FLAG_NO_RES_RANGE for IPv6 (tests bug47435.phpt, gh16944.phpt;
/// observed on PHP 8.5): unspecified, loopback, IPv4-mapped, link local.
const V6_RES: [(u128, u32); 4] = [(0, 128), (1, 128), (0xffff << 32, 96), (0xfe80 << 112, 10)];
/// The further blocks FILTER_FLAG_GLOBAL_RANGE rejects (test
/// bug77221.phpt; observed on PHP 8.5: 2002::/16 too).
const V6_NON_GLOBAL: [(u128, u32); 4] = [
    (0x0100 << 112, 64),
    (0x2001 << 112, 23),
    ((0x2001 << 112) | (0x0db8 << 96), 32),
    (0x2002 << 112, 16),
];

/// FILTER_VALIDATE_IP with its flags (PHP manual; tests 018.phpt,
/// 030.phpt, bug47435.phpt, bug53150.phpt, bug61700.phpt, bug77221.phpt,
/// filter_ipv4_rfc6890.phpt, gh16944.phpt, gh18309.phpt).
///
/// Observed on PHP 8.5: an IPv6 address with an embedded dotted IPv4 part
/// is never accepted with FILTER_FLAG_NO_RES_RANGE or
/// FILTER_FLAG_GLOBAL_RANGE and always passes FILTER_FLAG_NO_PRIV_RANGE,
/// whatever its value (fc00::1.2.3.4 and ::ffff:192.168.0.1 pass
/// FILTER_FLAG_NO_PRIV_RANGE).
pub(crate) fn validate(s: &[u8], flags: i64) -> bool {
    let (mut want4, mut want6) = (flags & FILTER_FLAG_IPV4 != 0, flags & FILTER_FLAG_IPV6 != 0);
    if !want4 && !want6 {
        want4 = true;
        want6 = true;
    }
    let global = flags & FILTER_FLAG_GLOBAL_RANGE != 0;
    let no_priv = global || flags & FILTER_FLAG_NO_PRIV_RANGE != 0;
    let no_res = global || flags & FILTER_FLAG_NO_RES_RANGE != 0;
    match parse_ip(s) {
        Some(Ip::V4(a)) => {
            if !want4 {
                return false;
            }
            let a = u32::from_be_bytes(a);
            let hit = |t: &[(u32, u32)]| t.iter().any(|&(n, l)| in_v4(a, n, l));
            !(no_priv && hit(&V4_PRIV) || no_res && hit(&V4_RES) || global && hit(&V4_NON_GLOBAL))
        }
        Some(Ip::V6 {
            groups,
            embedded_v4,
        }) => {
            if !want6 {
                return false;
            }
            if !no_priv && !no_res {
                return true;
            }
            let a = groups.iter().fold(0u128, |a, &g| a << 16 | g as u128);
            let hit = |t: &[(u128, u32)]| t.iter().any(|&(n, l)| in_v6(a, n, l));
            !(no_priv && !embedded_v4 && hit(&V6_PRIV)
                || no_res && (embedded_v4 || hit(&V6_RES))
                || global && hit(&V6_NON_GLOBAL))
        }
        None => false,
    }
}
