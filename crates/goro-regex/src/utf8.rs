//! UTF-8 validation with PCRE2's error classification, and decoding helpers
//! for subjects that have already been validated.

use crate::error::Utf8Error;

/// Validates `s`, returning the first error and the offset of the first byte
/// of the offending character (pcre2unicode: "Errors in UTF-8 strings"; the
/// RFC 2279 forms of up to 6 bytes are recognised first, so a truncated
/// 5-byte form reports 4 missing bytes).
pub fn validate(s: &[u8]) -> Result<(), (Utf8Error, usize)> {
    let mut i = 0;
    let n = s.len();
    // Fast skip over ASCII, eight bytes at a time.
    while i < n {
        if i + 8 <= n {
            let w = u64::from_le_bytes(s[i..i + 8].try_into().unwrap());
            if w & 0x8080_8080_8080_8080 == 0 {
                i += 8;
                continue;
            }
        }
        let c = s[i];
        if c < 0x80 {
            i += 1;
            continue;
        }
        match check_char(s, i) {
            Ok(len) => i += len,
            Err(e) => return Err((e, i)),
        }
    }
    Ok(())
}

/// Checks the character whose lead byte (>= 0x80) is at `i`; returns its
/// length.
fn check_char(s: &[u8], i: usize) -> Result<usize, Utf8Error> {
    let c = s[i];
    if c < 0xc0 {
        return Err(Utf8Error::IsolatedContinuation);
    }
    if c >= 0xfe {
        return Err(Utf8Error::IllegalByte);
    }
    let extra = if c < 0xe0 {
        1
    } else if c < 0xf0 {
        2
    } else if c < 0xf8 {
        3
    } else if c < 0xfc {
        4
    } else {
        5
    };
    let avail = s.len() - i - 1;
    if avail < extra {
        return Err(Utf8Error::Truncated((extra - avail) as u8));
    }
    for k in 1..=extra {
        if s[i + k] & 0xc0 != 0x80 {
            return Err(Utf8Error::BadContinuation((k + 1) as u8));
        }
    }
    let b1 = s[i + 1] as u32 & 0x3f;
    match extra {
        1 => {
            if c < 0xc2 {
                return Err(Utf8Error::Overlong(2));
            }
        }
        2 => {
            let v = ((c as u32 & 0x0f) << 12) | (b1 << 6) | (s[i + 2] as u32 & 0x3f);
            if v < 0x800 {
                return Err(Utf8Error::Overlong(3));
            }
            if (0xd800..=0xdfff).contains(&v) {
                return Err(Utf8Error::Surrogate);
            }
        }
        3 => {
            let v = ((c as u32 & 0x07) << 18)
                | (b1 << 12)
                | ((s[i + 2] as u32 & 0x3f) << 6)
                | (s[i + 3] as u32 & 0x3f);
            if v < 0x10000 {
                return Err(Utf8Error::Overlong(4));
            }
            if v > 0x10ffff {
                return Err(Utf8Error::TooLarge);
            }
        }
        4 => {
            // 0xf8 followed by a second byte below 0x88 encodes less than
            // 0x200000.
            if c == 0xf8 && b1 < 0x08 {
                return Err(Utf8Error::Overlong(5));
            }
            return Err(Utf8Error::FiveByte);
        }
        _ => {
            if c == 0xfc && b1 < 0x04 {
                return Err(Utf8Error::Overlong(6));
            }
            return Err(Utf8Error::SixByte);
        }
    }
    Ok(extra + 1)
}

/// Decodes the character at `i` of a valid UTF-8 string, returning the code
/// point and its length.
#[inline(always)]
pub fn decode(s: &[u8], i: usize) -> (u32, usize) {
    let c = s[i] as u32;
    if c < 0x80 {
        return (c, 1);
    }
    decode_multi(s, i)
}

#[inline]
fn decode_multi(s: &[u8], i: usize) -> (u32, usize) {
    let c = s[i] as u32;
    // Out-of-range indices can only happen with unchecked (invalid) input;
    // treat missing bytes as zero rather than panicking.
    let b = |k: usize| s.get(i + k).map_or(0, |&b| b as u32 & 0x3f);
    if c < 0xe0 {
        (((c & 0x1f) << 6) | b(1), 2.min(s.len() - i))
    } else if c < 0xf0 {
        (((c & 0x0f) << 12) | (b(1) << 6) | b(2), 3.min(s.len() - i))
    } else {
        (
            ((c & 0x07) << 18) | (b(1) << 12) | (b(2) << 6) | b(3),
            4.min(s.len() - i),
        )
    }
}

/// Decodes the character that ends just before `i` in a valid UTF-8 string.
#[inline]
pub fn decode_before(s: &[u8], i: usize) -> (u32, usize) {
    let mut j = i - 1;
    if s[j] < 0x80 {
        return (s[j] as u32, 1);
    }
    let lim = i.saturating_sub(4);
    while j > lim && s[j] & 0xc0 == 0x80 {
        j -= 1;
    }
    let (c, _) = decode(s, j);
    (c, i - j)
}

/// The start of the character before `i` (UTF-8 mode).
#[inline]
pub fn prev_start(s: &[u8], i: usize) -> usize {
    let mut j = i - 1;
    while j > 0 && s[j] & 0xc0 == 0x80 {
        j -= 1;
    }
    j
}

/// The length of the character whose lead byte is `b`.
#[inline(always)]
pub fn len_from_lead(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b < 0xe0 {
        2
    } else if b < 0xf0 {
        3
    } else {
        4
    }
}

/// Encodes `c` into `out`.
pub fn encode(c: u32, out: &mut Vec<u8>) {
    if c < 0x80 {
        out.push(c as u8);
    } else if c < 0x800 {
        out.push(0xc0 | (c >> 6) as u8);
        out.push(0x80 | (c & 0x3f) as u8);
    } else if c < 0x10000 {
        out.push(0xe0 | (c >> 12) as u8);
        out.push(0x80 | ((c >> 6) & 0x3f) as u8);
        out.push(0x80 | (c & 0x3f) as u8);
    } else {
        out.push(0xf0 | (c >> 18) as u8);
        out.push(0x80 | ((c >> 12) & 0x3f) as u8);
        out.push(0x80 | ((c >> 6) & 0x3f) as u8);
        out.push(0x80 | (c & 0x3f) as u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors() {
        // Observed on PHP 8.5 as pattern errors ("UTF-8 error: ...").
        assert_eq!(validate(b"\xff"), Err((Utf8Error::IllegalByte, 0)));
        assert_eq!(validate(b"a\xc3"), Err((Utf8Error::Truncated(1), 1)));
        assert_eq!(
            validate(b"\xc3\x28"),
            Err((Utf8Error::BadContinuation(2), 0))
        );
        assert_eq!(validate(b"\xe2\x82"), Err((Utf8Error::Truncated(1), 0)));
        assert_eq!(validate(b"\xed\xa0\x80"), Err((Utf8Error::Surrogate, 0)));
        assert_eq!(validate(b"\xf4\x90\x80\x80"), Err((Utf8Error::TooLarge, 0)));
        assert_eq!(validate(b"\xc0\xae"), Err((Utf8Error::Overlong(2), 0)));
        assert_eq!(validate(b"\x80"), Err((Utf8Error::IsolatedContinuation, 0)));
        assert_eq!(
            validate(b"\xf8\x88\x80\x80\x80"),
            Err((Utf8Error::FiveByte, 0))
        );
        assert_eq!(validate("héllo €".as_bytes()), Ok(()));
    }

    #[test]
    fn decoding() {
        let s = "a€𝄞".as_bytes();
        assert_eq!(decode(s, 0), ('a' as u32, 1));
        assert_eq!(decode(s, 1), (0x20ac, 3));
        assert_eq!(decode(s, 4), (0x1d11e, 4));
        assert_eq!(decode_before(s, 8), (0x1d11e, 4));
        assert_eq!(decode_before(s, 4), (0x20ac, 3));
        assert_eq!(prev_start(s, 4), 1);
    }
}
