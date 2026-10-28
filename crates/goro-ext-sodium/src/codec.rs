//! Constant-time hexadecimal and Base64 codecs (`sodium_bin2hex()`,
//! `sodium_hex2bin()`, `sodium_bin2base64()`, `sodium_base642bin()`).
//!
//! Behaviour from libsodium's documentation ("Helpers": the codecs run in
//! constant time for a given length; `ignore` lists characters to skip)
//! and from PHP 8.5 observations (`tests/fixtures`): the ignore list is a C
//! string, so it ends at its first NUL byte and a NUL in the input is
//! always skippable; hex digits pair up, and ignored characters are only
//! skipped between pairs; Base64 decoding is strict (no non-zero bits after
//! the last full byte, exact padding for the padded variants).
//!
//! Character classification is branch-free and table-free, so the time
//! taken does not depend on the secret bytes being encoded or decoded (only
//! on where an invalid or ignored character stops or interrupts decoding).

/// Branch-free mask: 0xff if `a < b` (both < 2^15), else 0.
#[inline(always)]
fn lt_mask(a: u32, b: u32) -> u32 {
    (a.wrapping_sub(b) >> 16) & 0xff
}

/// Branch-free mask: 0xff if `a == b` (both < 2^15), else 0.
#[inline(always)]
fn eq_mask(a: u32, b: u32) -> u32 {
    let x = a ^ b;
    (x.wrapping_sub(1) >> 16) & 0xff
}

/// Lower-case hex of `bin`.
pub fn bin2hex(bin: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; bin.len() * 2];
    for (o, &b) in out.as_chunks_mut::<2>().0.iter_mut().zip(bin) {
        let hi = (b >> 4) as u32;
        let lo = (b & 15) as u32;
        // n < 10: '0' + n; else 'a' + n - 10 = '0' + n + 39.
        o[0] = (48 + hi + (lt_mask(9, hi) & 39)) as u8;
        o[1] = (48 + lo + (lt_mask(9, lo) & 39)) as u8;
    }
    out
}

/// Value of a hex digit, or `None`. Branch-free in the character.
#[inline(always)]
fn hex_val(c: u8) -> Option<u8> {
    let c = c as u32;
    let num = c ^ 48;
    let num_ok = lt_mask(num, 10);
    let alpha = (c & !32).wrapping_sub(55);
    let alpha_ok = lt_mask(alpha.wrapping_sub(10) & 0xffff, 6) & !lt_mask(alpha & 0xffff, 10);
    let ok = num_ok | alpha_ok;
    let v = (num_ok & num) | (alpha_ok & alpha);
    if ok != 0 { Some(v as u8) } else { None }
}

/// The effective ignore set: `ignore` up to its first NUL (a C string).
fn ignore_set(ignore: &[u8]) -> &[u8] {
    match ignore.iter().position(|&b| b == 0) {
        Some(n) => &ignore[..n],
        None => ignore,
    }
}

#[inline]
fn ignored(set: &[u8], c: u8) -> bool {
    c == 0 || set.contains(&c)
}

/// Decodes hex, skipping characters of `ignore` between byte pairs.
/// `None` when the input is not entirely consumed (an invalid character, a
/// lone trailing digit).
pub fn hex2bin(hex: &[u8], ignore: &[u8]) -> Option<Vec<u8>> {
    let set = ignore_set(ignore);
    let mut out = Vec::with_capacity(hex.len() / 2);
    // Fast path: whole pairs of digits.
    let mut i = 0;
    while i + 1 < hex.len() {
        match (hex_val(hex[i]), hex_val(hex[i + 1])) {
            (Some(h), Some(l)) => {
                out.push((h << 4) | l);
                i += 2;
            }
            _ => break,
        }
    }
    let mut hi: Option<u8> = None;
    for &c in &hex[i..] {
        match hex_val(c) {
            Some(v) => match hi.take() {
                None => hi = Some(v),
                Some(h) => out.push((h << 4) | v),
            },
            None => {
                if hi.is_none() && ignored(set, c) {
                    continue;
                }
                return None;
            }
        }
    }
    if hi.is_some() {
        return None;
    }
    Some(out)
}

/// A Base64 variant (`SODIUM_BASE64_VARIANT_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Variant {
    pub urlsafe: bool,
    pub padded: bool,
}

impl Variant {
    /// The variant for an id, or `None` for an invalid id (valid ids are
    /// 1, 3, 5 and 7: bit 1 means "no padding", bit 2 "URL-safe").
    pub fn from_id(id: i64) -> Option<Variant> {
        if id & !6 != 1 {
            return None;
        }
        Some(Variant {
            urlsafe: id & 4 != 0,
            padded: id & 2 == 0,
        })
    }
}

/// Encoded length of `n` bytes.
pub fn base64_encoded_len(n: usize, v: Variant) -> usize {
    if v.padded {
        n.div_ceil(3) * 4
    } else {
        n / 3 * 4 + [0, 2, 3][n % 3]
    }
}

#[inline(always)]
fn b64_char(x: u32, urlsafe: bool) -> u8 {
    // 0..25 'A'.., 26..51 'a'.., 52..61 '0'.., 62 '+'/'-', 63 '/'/'_'.
    let mut c = x + 65;
    c += lt_mask(25, x) & 6; // 'a' - 26 = 71 = 65 + 6
    c = c.wrapping_sub(lt_mask(51, x) & 75); // '0' - 52 = -4 = 71 - 75
    let (p, s) = if urlsafe { (45u32, 95u32) } else { (43, 47) };
    // x == 62: c is 62 - 4 = 58 + ... compute replacement by masks.
    let m62 = eq_mask(x, 62);
    let m63 = eq_mask(x, 63);
    c = (c & !(m62 | m63)) | (m62 & p) | (m63 & s);
    c as u8
}

/// Encodes `bin` in the given variant.
pub fn bin2base64(bin: &[u8], v: Variant) -> Vec<u8> {
    let mut out = Vec::with_capacity(base64_encoded_len(bin.len(), v));
    let (chunks, rem) = bin.as_chunks::<3>();
    for c in chunks {
        let n = (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32;
        out.push(b64_char(n >> 18, v.urlsafe));
        out.push(b64_char((n >> 12) & 63, v.urlsafe));
        out.push(b64_char((n >> 6) & 63, v.urlsafe));
        out.push(b64_char(n & 63, v.urlsafe));
    }
    match rem.len() {
        1 => {
            let n = (rem[0] as u32) << 16;
            out.push(b64_char(n >> 18, v.urlsafe));
            out.push(b64_char((n >> 12) & 63, v.urlsafe));
            if v.padded {
                out.extend_from_slice(b"==");
            }
        }
        2 => {
            let n = (rem[0] as u32) << 16 | (rem[1] as u32) << 8;
            out.push(b64_char(n >> 18, v.urlsafe));
            out.push(b64_char((n >> 12) & 63, v.urlsafe));
            out.push(b64_char((n >> 6) & 63, v.urlsafe));
            if v.padded {
                out.push(b'=');
            }
        }
        _ => {}
    }
    out
}

/// Value of a Base64 character in the variant's alphabet. Branch-free in
/// the character.
#[inline(always)]
fn b64_val(c: u8, urlsafe: bool) -> Option<u32> {
    let c = c as u32;
    let up = lt_mask(c.wrapping_sub(65) & 0xffff, 26);
    let lo = lt_mask(c.wrapping_sub(97) & 0xffff, 26);
    let dg = lt_mask(c.wrapping_sub(48) & 0xffff, 10);
    let (p, s) = if urlsafe { (45u32, 95u32) } else { (43, 47) };
    let pm = eq_mask(c, p);
    let sm = eq_mask(c, s);
    let v = (up & c.wrapping_sub(65))
        | (lo & c.wrapping_sub(71))
        | (dg & c.wrapping_add(4))
        | (pm & 62)
        | (sm & 63);
    if (up | lo | dg | pm | sm) != 0 {
        Some(v & 63)
    } else {
        None
    }
}

/// Decodes Base64 in the given variant, skipping characters of `ignore`.
/// `None` when the input is invalid or not entirely consumed.
pub fn base642bin(b64: &[u8], v: Variant, ignore: &[u8]) -> Option<Vec<u8>> {
    let set = ignore_set(ignore);
    let mut out = Vec::with_capacity(b64.len() / 4 * 3 + 3);
    let mut acc: u32 = 0;
    let mut acc_len: u32 = 0;
    let mut i = 0;
    while i < b64.len() {
        let c = b64[i];
        match b64_val(c, v.urlsafe) {
            Some(d) => {
                acc = (acc << 6) | d;
                acc_len += 6;
                if acc_len >= 8 {
                    acc_len -= 8;
                    out.push((acc >> acc_len) as u8);
                }
            }
            None => {
                if ignored(set, c) {
                    i += 1;
                    continue;
                }
                break;
            }
        }
        i += 1;
    }
    // A single leftover character (6 bits) cannot end a byte sequence, and
    // the bits after the last byte must be zero.
    if acc_len > 4 || acc & ((1 << acc_len) - 1) != 0 {
        return None;
    }
    if v.padded {
        let mut pad = acc_len / 2;
        while pad > 0 {
            if i >= b64.len() {
                return None;
            }
            let c = b64[i];
            if c == b'=' {
                pad -= 1;
            } else if !ignored(set, c) {
                return None;
            }
            i += 1;
        }
    }
    while i < b64.len() && ignored(set, b64[i]) {
        i += 1;
    }
    if i != b64.len() {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let all: Vec<u8> = (0..=255).collect();
        let h = bin2hex(&all);
        assert_eq!(
            h,
            all.iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
                .into_bytes()
        );
        assert_eq!(hex2bin(&h, b"").unwrap(), all);
        assert_eq!(hex2bin(&h.to_ascii_uppercase(), b"").unwrap(), all);
        for c in 0..=255u8 {
            let ok = c.is_ascii_hexdigit();
            assert_eq!(hex_val(c).is_some(), ok, "{c}");
        }
    }

    #[test]
    fn b64_alphabet() {
        for urlsafe in [false, true] {
            let alpha: &[u8] = if urlsafe {
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
            } else {
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
            };
            for x in 0..64u32 {
                assert_eq!(b64_char(x, urlsafe), alpha[x as usize]);
            }
            for c in 0..=255u8 {
                let want = alpha.iter().position(|&a| a == c).map(|p| p as u32);
                assert_eq!(b64_val(c, urlsafe), want, "{c}");
            }
        }
    }

    #[test]
    fn b64_roundtrip() {
        for v in [1, 3, 5, 7] {
            let v = Variant::from_id(v).unwrap();
            for n in 0..40 {
                let bin: Vec<u8> = (0..n as u8).map(|i| i.wrapping_mul(37) ^ 0xa5).collect();
                let e = bin2base64(&bin, v);
                assert_eq!(e.len(), base64_encoded_len(n, v));
                assert_eq!(base642bin(&e, v, b"").unwrap(), bin);
            }
        }
    }
}
