//! utf8.c: UTF-8 / UTF-16 helpers.

use crate::doc::END_OF_STREAM;
use crate::streamio::InputSource;

const K_UTF8_BYTE_SWAP_NOT_A_CHAR: u32 = 0xFFFE;
const K_UTF8_NOT_A_CHAR: u32 = 0xFFFF;
const K_MAX_UTF8_FROM_UCS4: u32 = 0x10FFFF;
const K_UTF16_SURROGATES_BEGIN: u32 = 0x10000;
const K_MAX_UTF16_FROM_UCS4: u32 = 0x10FFFF;
const K_UTF16_LOW_SURROGATE_BEGIN: u32 = 0xD800;
const K_UTF16_LOW_SURROGATE_END: u32 = 0xDBFF;
const K_UTF16_HIGH_SURROGATE_BEGIN: u32 = 0xDC00;
const K_UTF16_HIGH_SURROGATE_END: u32 = 0xDFFF;

const OFFSET_UTF8_SEQUENCES: [usize; 5] = [0, 1, 2, 4, 7];

struct ValidUtf8Sequence {
    low_char: u32,
    high_char: u32,
    valid_bytes: [u8; 8],
}

const VALID_UTF8: [ValidUtf8Sequence; 7] = [
    ValidUtf8Sequence {
        low_char: 0x0000,
        high_char: 0x007F,
        valid_bytes: [0x00, 0x7F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    },
    ValidUtf8Sequence {
        low_char: 0x0080,
        high_char: 0x07FF,
        valid_bytes: [0xC2, 0xDF, 0x80, 0xBF, 0x00, 0x00, 0x00, 0x00],
    },
    ValidUtf8Sequence {
        low_char: 0x0800,
        high_char: 0x0FFF,
        valid_bytes: [0xE0, 0xE0, 0xA0, 0xBF, 0x80, 0xBF, 0x00, 0x00],
    },
    ValidUtf8Sequence {
        low_char: 0x1000,
        high_char: 0xFFFF,
        valid_bytes: [0xE1, 0xEF, 0x80, 0xBF, 0x80, 0xBF, 0x00, 0x00],
    },
    ValidUtf8Sequence {
        low_char: 0x10000,
        high_char: 0x3FFFF,
        valid_bytes: [0xF0, 0xF0, 0x90, 0xBF, 0x80, 0xBF, 0x80, 0xBF],
    },
    ValidUtf8Sequence {
        low_char: 0x40000,
        high_char: 0xFFFFF,
        valid_bytes: [0xF1, 0xF3, 0x80, 0xBF, 0x80, 0xBF, 0x80, 0xBF],
    },
    ValidUtf8Sequence {
        low_char: 0x100000,
        high_char: 0x10FFFF,
        valid_bytes: [0xF4, 0xF4, 0x80, 0x8F, 0x80, 0xBF, 0x80, 0xBF],
    },
];

/// Where DecodeUTF8BytesToChar takes the successor bytes from.
pub enum Successors<'a> {
    /// `successorBytes` (NUL-terminated: reading past the end gives 0)
    Bytes(&'a [u8]),
    /// `inp`
    Source(&'a mut InputSource),
    /// neither
    None,
}

/// DecodeUTF8BytesToChar(): returns (error, char, count).
pub fn decode_utf8_bytes_to_char(first_byte: u32, succ: Successors) -> (bool, u32, i32) {
    let mut buf = [0u8; 10];
    let mut n: u32;
    let mut bytes: i32;
    let mut has_error = false;

    if first_byte == END_OF_STREAM {
        return (false, first_byte, 1);
    }
    let ch = first_byte;
    if ch <= 0x7F {
        n = ch;
        bytes = 1;
    } else if (ch & 0xE0) == 0xC0 {
        n = ch & 31;
        bytes = 2;
    } else if (ch & 0xF0) == 0xE0 {
        n = ch & 15;
        bytes = 3;
    } else if (ch & 0xF8) == 0xF0 {
        n = ch & 7;
        bytes = 4;
    } else if (ch & 0xFC) == 0xF8 {
        n = ch & 3;
        bytes = 5;
        has_error = true;
    } else if (ch & 0xFE) == 0xFC {
        n = ch & 1;
        bytes = 6;
        has_error = true;
    } else {
        n = ch;
        bytes = 1;
        has_error = true;
    }

    let mut from_bytes: Option<&[u8]> = None;
    match succ {
        Successors::Bytes(s) => {
            from_bytes = Some(s);
            let mut i = 0;
            while i < bytes - 1 {
                let b = s.get(i as usize).copied().unwrap_or(0);
                if b == 0 || (b & 0xC0) != 0x80 {
                    has_error = true;
                    bytes = i + 1;
                    break;
                }
                n = (n << 6) | (b as u32 & 0x3F);
                i += 1;
            }
        }
        Successors::Source(inp) => {
            let mut i = 0;
            while i < bytes - 1 && !inp.eof() {
                let b = inp.get_byte();
                buf[i as usize] = b as u8;
                if b == END_OF_STREAM || (buf[i as usize] & 0xC0) != 0x80 {
                    has_error = true;
                    bytes = i + 1;
                    if b != END_OF_STREAM {
                        inp.unget_byte(buf[i as usize]);
                    }
                    break;
                }
                n = (n << 6) | (buf[i as usize] as u32 & 0x3F);
                i += 1;
            }
        }
        Successors::None => {
            if bytes > 1 {
                has_error = true;
                bytes = 1;
            }
        }
    }

    if !has_error && (n == K_UTF8_BYTE_SWAP_NOT_A_CHAR || n == K_UTF8_NOT_A_CHAR) {
        has_error = true;
    }
    if !has_error && n > K_MAX_UTF8_FROM_UCS4 {
        has_error = true;
    }
    if !has_error {
        let lo = OFFSET_UTF8_SEQUENCES[(bytes - 1) as usize];
        let hi = OFFSET_UTF8_SEQUENCES[bytes as usize] - 1;
        if n < VALID_UTF8[lo].low_char || n > VALID_UTF8[hi].high_char {
            has_error = true;
        } else {
            has_error = true;
            for seq in &VALID_UTF8[lo..=hi] {
                for temp_count in 0..bytes as usize {
                    let the_byte = if temp_count == 0 {
                        first_byte as u8
                    } else if let Some(s) = from_bytes {
                        s.get(temp_count - 1).copied().unwrap_or(0)
                    } else {
                        buf[temp_count - 1]
                    };
                    if the_byte >= seq.valid_bytes[temp_count * 2]
                        && the_byte <= seq.valid_bytes[temp_count * 2 + 1]
                    {
                        has_error = false;
                    }
                    if has_error {
                        break;
                    }
                }
            }
        }
    }
    (has_error, n, bytes)
}

/// EncodeCharToUTF8Bytes(): returns (error, bytes, count). Callers that
/// write to a sink only write the bytes when there is no error.
pub fn encode_char_to_utf8_bytes(c: u32) -> (bool, [u8; 6], i32) {
    let mut buf = [0u8; 6];
    let bytes: i32;
    let mut has_error = false;
    if c <= 0x7F {
        buf[0] = c as u8;
        bytes = 1;
    } else if c <= 0x7FF {
        buf[0] = (0xC0 | (c >> 6)) as u8;
        buf[1] = (0x80 | (c & 0x3F)) as u8;
        bytes = 2;
    } else if c <= 0xFFFF {
        buf[0] = (0xE0 | (c >> 12)) as u8;
        buf[1] = (0x80 | ((c >> 6) & 0x3F)) as u8;
        buf[2] = (0x80 | (c & 0x3F)) as u8;
        bytes = 3;
        if c == K_UTF8_BYTE_SWAP_NOT_A_CHAR || c == K_UTF8_NOT_A_CHAR {
            has_error = true;
        }
    } else if c <= 0x1FFFFF {
        buf[0] = (0xF0 | (c >> 18)) as u8;
        buf[1] = (0x80 | ((c >> 12) & 0x3F)) as u8;
        buf[2] = (0x80 | ((c >> 6) & 0x3F)) as u8;
        buf[3] = (0x80 | (c & 0x3F)) as u8;
        bytes = 4;
        if c > K_MAX_UTF8_FROM_UCS4 {
            has_error = true;
        }
    } else if c <= 0x3FFFFFF {
        buf[0] = (0xF8 | (c >> 24)) as u8;
        buf[1] = (0x80 | (c >> 18)) as u8;
        buf[2] = (0x80 | ((c >> 12) & 0x3F)) as u8;
        buf[3] = (0x80 | ((c >> 6) & 0x3F)) as u8;
        buf[4] = (0x80 | (c & 0x3F)) as u8;
        bytes = 5;
        has_error = true;
    } else if c <= 0x7FFFFFFF {
        buf[0] = (0xFC | (c >> 30)) as u8;
        buf[1] = (0x80 | ((c >> 24) & 0x3F)) as u8;
        buf[2] = (0x80 | ((c >> 18) & 0x3F)) as u8;
        buf[3] = (0x80 | ((c >> 12) & 0x3F)) as u8;
        buf[4] = (0x80 | ((c >> 6) & 0x3F)) as u8;
        buf[5] = (0x80 | (c & 0x3F)) as u8;
        bytes = 6;
        has_error = true;
    } else {
        has_error = true;
        bytes = 0;
    }
    (has_error, buf, bytes)
}

/// GetUTF8(): decodes the char at the start of `s`; returns (bytes - 1, ch).
pub fn get_utf8(s: &[u8]) -> (usize, u32) {
    let first = s.first().copied().unwrap_or(0) as u32;
    let rest = if s.is_empty() { &[][..] } else { &s[1..] };
    let (err, n, bytes) = decode_utf8_bytes_to_char(first, Successors::Bytes(rest));
    let ch = if err { 0xFFFD } else { n };
    ((bytes - 1).max(0) as usize, ch)
}

/// PutUTF8(): appends the UTF-8 encoding of c (U+FFFD on error).
pub fn put_utf8(buf: &mut Vec<u8>, c: u32) {
    let (err, b, count) = encode_char_to_utf8_bytes(c);
    if err {
        buf.extend_from_slice(&[0xEF, 0xBF, 0xBD]);
    } else {
        buf.extend_from_slice(&b[..count as usize]);
    }
}

pub fn is_valid_utf16_from_ucs4(ucs4: u32) -> bool {
    ucs4 <= K_MAX_UTF16_FROM_UCS4
}

pub fn is_high_surrogate(ch: u32) -> bool {
    (K_UTF16_HIGH_SURROGATE_BEGIN..=K_UTF16_HIGH_SURROGATE_END).contains(&ch)
}

pub fn is_low_surrogate(ch: u32) -> bool {
    (K_UTF16_LOW_SURROGATE_BEGIN..=K_UTF16_LOW_SURROGATE_END).contains(&ch)
}

pub fn combine_surrogate_pair(high: u32, low: u32) -> u32 {
    ((low.wrapping_sub(K_UTF16_LOW_SURROGATE_BEGIN)).wrapping_mul(0x400))
        .wrapping_add(high)
        .wrapping_sub(K_UTF16_HIGH_SURROGATE_BEGIN)
        .wrapping_add(0x10000)
}

/// SplitSurrogatePair(): (low, high)
pub fn split_surrogate_pair(utf16: u32) -> Option<(u32, u32)> {
    if is_valid_combined_char(utf16) {
        let low = (utf16 - K_UTF16_SURROGATES_BEGIN) / 0x400 + K_UTF16_LOW_SURROGATE_BEGIN;
        let high = (utf16 - K_UTF16_SURROGATES_BEGIN) % 0x400 + K_UTF16_HIGH_SURROGATE_BEGIN;
        Some((low, high))
    } else {
        None
    }
}

pub fn is_valid_combined_char(ch: u32) -> bool {
    ch >= K_UTF16_SURROGATES_BEGIN
        && (ch & 0x0000FFFE) != 0x0000FFFE
        && (ch & 0x0000FFFF) != 0x0000FFFF
}

pub fn is_combined_char(ch: u32) -> bool {
    ch >= K_UTF16_SURROGATES_BEGIN
}
