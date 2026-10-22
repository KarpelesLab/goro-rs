//! libsodium's utils.c / codecs.c: constant-time helpers, hex and base64
//! codecs, ISO/IEC 7816-4 padding.

pub const VARIANT_NO_PADDING_MASK: u32 = 0x2;
pub const VARIANT_URLSAFE_MASK: u32 = 0x4;

/// A C `ignore` string: strchr(ignore, c) is true for its characters before
/// the first NUL, and for NUL itself (the terminator).
fn ignored(ignore: Option<&[u8]>, c: u8) -> bool {
    match ignore {
        None => false,
        Some(set) => c == 0 || set.iter().take_while(|&&b| b != 0).any(|&b| b == c),
    }
}

/// sodium_bin2hex()
pub fn bin2hex(bin: &[u8]) -> Vec<u8> {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::with_capacity(bin.len() * 2);
    for &b in bin {
        out.push(H[(b >> 4) as usize]);
        out.push(H[(b & 15) as usize]);
    }
    out
}

/// sodium_hex2bin() with a `hex_end` pointer: (status, bytes, end offset).
pub fn hex2bin(hex: &[u8], ignore: Option<&[u8]>, bin_maxlen: usize) -> (bool, Vec<u8>, usize) {
    let mut bin = Vec::with_capacity(bin_maxlen);
    let mut hex_pos = 0usize;
    let mut ok = true;
    let mut state = false;
    let mut acc = 0u8;
    while hex_pos < hex.len() {
        let c = hex[hex_pos];
        let val = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => {
                if !state && ignored(ignore, c) {
                    hex_pos += 1;
                    continue;
                }
                break;
            }
        };
        if bin.len() >= bin_maxlen {
            ok = false;
            break;
        }
        if !state {
            acc = val * 16;
        } else {
            bin.push(acc | val);
        }
        state = !state;
        hex_pos += 1;
    }
    if state {
        hex_pos -= 1;
        ok = false;
    }
    if !ok {
        bin.clear();
    }
    (ok, bin, hex_pos)
}

/// sodium_base64_ENCODED_LEN() (including the NUL terminator).
pub fn base64_encoded_len(bin_len: usize, variant: u32) -> usize {
    let rem = bin_len - (bin_len / 3) * 3;
    let has_rem = (rem | (rem >> 1)) & 1;
    let pad = (variant & 2) >> 1;
    (bin_len / 3) * 4
        + has_rem * (4 - ((!pad.wrapping_sub(1) as usize) & (3usize.wrapping_sub(rem))))
        + 1
}

/// sodium_bin2base64()
pub fn bin2base64(bin: &[u8], variant: u32) -> Vec<u8> {
    let table: &[u8; 64] = if variant & VARIANT_URLSAFE_MASK != 0 {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
    } else {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
    };
    let mut out = Vec::with_capacity(bin.len() / 3 * 4 + 4);
    let mut acc: u32 = 0;
    let mut acc_len = 0u32;
    for &b in bin {
        acc = (acc << 8) + b as u32;
        acc_len += 8;
        while acc_len >= 6 {
            acc_len -= 6;
            out.push(table[((acc >> acc_len) & 0x3f) as usize]);
        }
    }
    if acc_len > 0 {
        out.push(table[((acc << (6 - acc_len)) & 0x3f) as usize]);
    }
    if variant & VARIANT_NO_PADDING_MASK == 0 {
        while out.len() % 4 != 0 {
            out.push(b'=');
        }
    }
    out
}

fn b64_char_to_byte(c: u8, urlsafe: bool) -> Option<u32> {
    Some(match c {
        b'A'..=b'Z' => (c - b'A') as u32,
        b'a'..=b'z' => (c - b'a') as u32 + 26,
        b'0'..=b'9' => (c - b'0') as u32 + 52,
        b'+' if !urlsafe => 62,
        b'/' if !urlsafe => 63,
        b'-' if urlsafe => 62,
        b'_' if urlsafe => 63,
        _ => return None,
    })
}

/// sodium_base642bin() with a `b64_end` pointer: (status, bytes, end
/// offset).
pub fn base642bin(
    b64: &[u8],
    ignore: Option<&[u8]>,
    variant: u32,
    bin_maxlen: usize,
) -> (bool, Vec<u8>, usize) {
    let urlsafe = variant & VARIANT_URLSAFE_MASK != 0;
    let mut acc_len = 0u32;
    let mut acc: u32 = 0;
    let mut pos = 0usize;
    let mut bin = Vec::with_capacity(bin_maxlen);
    let mut ok = true;
    while pos < b64.len() {
        let c = b64[pos];
        let Some(d) = b64_char_to_byte(c, urlsafe) else {
            if ignored(ignore, c) {
                pos += 1;
                continue;
            }
            break;
        };
        acc = (acc << 6).wrapping_add(d);
        acc_len += 6;
        if acc_len >= 8 {
            acc_len -= 8;
            if bin.len() >= bin_maxlen {
                ok = false;
                break;
            }
            bin.push((acc >> acc_len) as u8);
        }
        pos += 1;
    }
    if acc_len > 4 || (acc & ((1u32 << acc_len) - 1)) != 0 {
        ok = false;
    } else if ok && variant & VARIANT_NO_PADDING_MASK == 0 {
        // _sodium_base642bin_skip_padding()
        let mut padding_len = acc_len / 2;
        while padding_len > 0 {
            if pos >= b64.len() {
                ok = false;
                break;
            }
            let c = b64[pos];
            if c == b'=' {
                padding_len -= 1;
            } else if !ignored(ignore, c) {
                ok = false;
                break;
            }
            pos += 1;
        }
    }
    if !ok {
        bin.clear();
    } else if ignore.is_some() {
        while pos < b64.len() && ignored(ignore, b64[pos]) {
            pos += 1;
        }
    }
    (ok, bin, pos)
}

/// sodium_increment()
pub fn increment(n: &mut [u8]) {
    let mut c: u16 = 1;
    for b in n.iter_mut() {
        c += *b as u16;
        *b = c as u8;
        c >>= 8;
    }
}

/// sodium_add()
pub fn add(a: &mut [u8], b: &[u8]) {
    let mut c: u16 = 0;
    for (x, &y) in a.iter_mut().zip(b.iter()) {
        c += *x as u16 + y as u16;
        *x = c as u8;
        c >>= 8;
    }
}

/// sodium_sub()
pub fn sub(a: &mut [u8], b: &[u8]) {
    let mut c: u16 = 0;
    for (x, &y) in a.iter_mut().zip(b.iter()) {
        c = (*x as u16).wrapping_sub(y as u16).wrapping_sub(c);
        *x = c as u8;
        c = (c >> 8) & 1;
    }
}

/// sodium_compare(): the little-endian comparison, in constant time.
pub fn compare(b1: &[u8], b2: &[u8]) -> i64 {
    let mut gt: u32 = 0;
    let mut eq: u32 = 1;
    let mut i = b1.len();
    while i != 0 {
        i -= 1;
        let x1 = b1[i] as u32;
        let x2 = b2[i] as u32;
        gt |= (x2.wrapping_sub(x1) >> 8) & eq;
        eq &= ((x2 ^ x1).wrapping_sub(1)) >> 8;
    }
    (gt + gt + eq) as i64 - 1
}

/// sodium_memcmp(): 0 when equal, -1 otherwise, in constant time.
pub fn memcmp(a: &[u8], b: &[u8]) -> i64 {
    let mut d = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        d |= x ^ y;
    }
    if d == 0 { 0 } else { -1 }
}

/// sodium_is_zero()
pub fn is_zero(n: &[u8]) -> bool {
    n.iter().fold(0u8, |a, &b| a | b) == 0
}

/// sodium_unpad(): the unpadded length.
pub fn unpad(padded: &[u8], blocksize: usize) -> Result<usize, ()> {
    if padded.len() < blocksize || blocksize == 0 {
        return Err(());
    }
    let tail = padded.len() - 1;
    let mut acc: u8 = 0;
    let mut valid: u8 = 0;
    let mut pad_len: usize = 0;
    for i in 0..blocksize {
        let c = padded[tail - i];
        let is_barrier = (((acc as u32).wrapping_sub(1) as usize)
            & pad_len.wrapping_sub(1)
            & (((c ^ 0x80) as u32).wrapping_sub(1) as usize))
            >> 8
            & 1;
        acc |= c;
        pad_len |= i & 1usize.wrapping_add(!is_barrier);
        valid |= is_barrier as u8;
    }
    if valid == 0 {
        return Err(());
    }
    Ok(padded.len() - 1 - pad_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b64_roundtrip() {
        for v in [1u32, 3, 5, 7] {
            for n in 0..20usize {
                let bin: Vec<u8> = (0..n as u8).map(|i| i.wrapping_mul(37)).collect();
                let enc = bin2base64(&bin, v);
                assert_eq!(enc.len() + 1, base64_encoded_len(n, v));
                let (ok, dec, end) = base642bin(&enc, None, v, enc.len() / 4 * 3 + 2);
                assert!(ok);
                assert_eq!(end, enc.len());
                assert_eq!(dec, bin);
            }
        }
    }

    #[test]
    fn unpad_works() {
        assert_eq!(unpad(b"xyz\x80\0\0\0\0", 8), Ok(3));
        assert_eq!(unpad(b"xyz\0\0\0\0\0", 8), Err(()));
        assert_eq!(unpad(b"\x80", 1), Ok(0));
    }
}
