//! UTF-8, UTF-16, UTF-32, UCS-2 and UCS-4.
//!
//! UTF-8 decoding follows the WHATWG Encoding Standard's UTF-8 decoder,
//! including how many error markers each invalid sequence yields (test
//! ext/mbstring/tests/utf8_error_handling.phpt).

use super::{Decode, ERR, Encode, Subst, fail};

const HI: u64 = 0x8080_8080_8080_8080;

/// The length of the longest prefix of `s` that is valid UTF-8 (WHATWG
/// rules: no surrogates, no overlongs, at most U+10FFFF), in bytes.
pub(crate) fn utf8_valid_up_to(s: &[u8]) -> usize {
    let n = s.len();
    let mut i = 0;
    while i < n {
        // Skip ASCII 16 bytes at a time.
        if s[i] < 0x80 {
            while i + 16 <= n {
                let a = u64::from_le_bytes(s[i..i + 8].try_into().unwrap());
                let b = u64::from_le_bytes(s[i + 8..i + 16].try_into().unwrap());
                if (a | b) & HI != 0 {
                    break;
                }
                i += 16;
            }
            while i < n && s[i] < 0x80 {
                i += 1;
            }
            if i >= n {
                break;
            }
        }
        let b = s[i];
        let need = match b {
            0xC2..=0xDF => 1,
            0xE0..=0xEF => 2,
            0xF0..=0xF4 => 3,
            _ => return i,
        };
        if i + need >= n {
            return i;
        }
        let c1 = s[i + 1];
        let ok1 = match b {
            0xE0 => (0xA0..=0xBF).contains(&c1),
            0xED => (0x80..=0x9F).contains(&c1),
            0xF0 => (0x90..=0xBF).contains(&c1),
            0xF4 => (0x80..=0x8F).contains(&c1),
            _ => (0x80..=0xBF).contains(&c1),
        };
        if !ok1 {
            return i;
        }
        for k in 2..=need {
            if !(0x80..=0xBF).contains(&s[i + k]) {
                return i;
            }
        }
        i += need + 1;
    }
    n
}

/// Decodes one UTF-8 character at `s[i]` (WHATWG). Returns the value and the
/// number of bytes consumed.
#[inline]
pub(crate) fn utf8_decode_one(s: &[u8], i: usize) -> (u32, usize) {
    let b = s[i];
    if b < 0x80 {
        return (b as u32, 1);
    }
    let (need, mut cp, lo, hi) = match b {
        0xC2..=0xDF => (1, (b & 0x1F) as u32, 0x80, 0xBF),
        0xE0 => (2, 0, 0xA0, 0xBF),
        0xE1..=0xEC | 0xEE..=0xEF => (2, (b & 0x0F) as u32, 0x80, 0xBF),
        0xED => (2, 0x0D, 0x80, 0x9F),
        0xF0 => (3, 0, 0x90, 0xBF),
        0xF1..=0xF3 => (3, (b & 0x07) as u32, 0x80, 0xBF),
        0xF4 => (3, 4, 0x80, 0x8F),
        _ => return (ERR, 1),
    };
    let mut j = i + 1;
    let mut lo = lo;
    let mut hi = hi;
    for _ in 0..need {
        if j >= s.len() {
            return (ERR, j - i);
        }
        let c = s[j];
        if c < lo || c > hi {
            return (ERR, j - i);
        }
        lo = 0x80;
        hi = 0xBF;
        cp = (cp << 6) | (c & 0x3F) as u32;
        j += 1;
    }
    (cp, j - i)
}

pub(crate) struct Utf8Dec;

impl Decode for Utf8Dec {
    fn decode(&mut self, s: &[u8], pos: &mut usize, out: &mut Vec<u32>, limit: usize) {
        let mut i = *pos;
        let n = s.len();
        while i < n && out.len() < limit {
            if s[i] < 0x80 {
                // ASCII run, 8 bytes at a time.
                while i + 8 <= n && out.len() + 8 <= limit {
                    let w = u64::from_le_bytes(s[i..i + 8].try_into().unwrap());
                    if w & HI != 0 {
                        break;
                    }
                    out.extend(s[i..i + 8].iter().map(|&b| b as u32));
                    i += 8;
                }
                while i < n && s[i] < 0x80 && out.len() < limit {
                    out.push(s[i] as u32);
                    i += 1;
                }
                continue;
            }
            let (cp, len) = utf8_decode_one(s, i);
            out.push(cp);
            i += len;
        }
        *pos = i;
    }
}

#[inline]
pub(crate) fn push_utf8(cp: u32, out: &mut Vec<u8>) {
    if cp < 0x80 {
        out.push(cp as u8);
    } else if cp < 0x800 {
        out.extend_from_slice(&[0xC0 | (cp >> 6) as u8, 0x80 | (cp & 0x3F) as u8]);
    } else if cp < 0x10000 {
        out.extend_from_slice(&[
            0xE0 | (cp >> 12) as u8,
            0x80 | ((cp >> 6) & 0x3F) as u8,
            0x80 | (cp & 0x3F) as u8,
        ]);
    } else {
        out.extend_from_slice(&[
            0xF0 | (cp >> 18) as u8,
            0x80 | ((cp >> 12) & 0x3F) as u8,
            0x80 | ((cp >> 6) & 0x3F) as u8,
            0x80 | (cp & 0x3F) as u8,
        ]);
    }
}

pub(crate) struct Utf8Enc;

impl Encode for Utf8Enc {
    #[inline]
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        // Surrogate code points are written as three-byte sequences
        // (observed on PHP 8.5: UCS-2BE D800 converts to UTF-8 ED A0 80).
        if cp > 0x10FFFF {
            return false;
        }
        push_utf8(cp, out);
        true
    }

    fn encode(&mut self, cps: &[u32], out: &mut Vec<u8>, sub: &Subst, errors: &mut usize) {
        out.reserve(cps.len());
        for &cp in cps {
            if cp < 0x80 {
                out.push(cp as u8);
            } else if !self.put(cp, out) {
                fail(self, cp, out, sub, errors);
            }
        }
    }
}

/// How a UTF-16/UTF-32 decoder picks its byte order.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bom {
    /// Big endian unless the string starts with a little-endian BOM; a
    /// leading BOM is dropped.
    Detect,
    Be,
    Le,
}

pub(crate) struct Utf16Dec {
    bom: Bom,
    le: bool,
    /// UTF-16 (pairs surrogates) rather than UCS-2 (passes units through).
    utf16: bool,
}

impl Utf16Dec {
    pub(crate) fn new(bom: Bom, utf16: bool) -> Self {
        Utf16Dec {
            bom,
            le: bom == Bom::Le,
            utf16,
        }
    }
}

impl Decode for Utf16Dec {
    fn decode(&mut self, s: &[u8], pos: &mut usize, out: &mut Vec<u32>, limit: usize) {
        let mut i = *pos;
        let n = s.len();
        if i == 0 && self.bom == Bom::Detect && n >= 2 {
            if s[0] == 0xFE && s[1] == 0xFF {
                i = 2;
            } else if s[0] == 0xFF && s[1] == 0xFE {
                self.le = true;
                i = 2;
            }
        }
        let le = self.le;
        let unit = |i: usize| -> u32 {
            if le {
                u16::from_le_bytes([s[i], s[i + 1]]) as u32
            } else {
                u16::from_be_bytes([s[i], s[i + 1]]) as u32
            }
        };
        while i < n && out.len() < limit {
            if i + 1 >= n {
                out.push(ERR);
                i = n;
                break;
            }
            let u = unit(i);
            i += 2;
            if !self.utf16 || !(0xD800..=0xDFFF).contains(&u) {
                out.push(u);
            } else if u >= 0xDC00 {
                out.push(ERR);
            } else if i + 1 >= n {
                // A high surrogate with no complete unit after it.
                out.push(ERR);
                if i < n {
                    out.push(ERR);
                    i = n;
                }
            } else {
                let u2 = unit(i);
                if (0xDC00..=0xDFFF).contains(&u2) {
                    i += 2;
                    out.push(0x10000 + ((u - 0xD800) << 10) + (u2 - 0xDC00));
                } else {
                    out.push(ERR);
                }
            }
        }
        *pos = i;
    }
}

pub(crate) struct Utf16Enc {
    le: bool,
    utf16: bool,
}

impl Utf16Enc {
    pub(crate) fn new(le: bool, utf16: bool) -> Self {
        Utf16Enc { le, utf16 }
    }

    #[inline]
    fn unit(&self, u: u32, out: &mut Vec<u8>) {
        let u = u as u16;
        if self.le {
            out.extend_from_slice(&u.to_le_bytes());
        } else {
            out.extend_from_slice(&u.to_be_bytes());
        }
    }
}

impl Encode for Utf16Enc {
    #[inline]
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        if cp < 0x10000 {
            // Surrogate code points are written as single units (observed
            // on PHP 8.5).
            self.unit(cp, out);
            true
        } else if self.utf16 && cp <= 0x10FFFF {
            let c = cp - 0x10000;
            self.unit(0xD800 | (c >> 10), out);
            self.unit(0xDC00 | (c & 0x3FF), out);
            true
        } else {
            false
        }
    }
}

pub(crate) struct Utf32Dec {
    bom: Bom,
    le: bool,
    /// UTF-32 (rejects surrogates and values past U+10FFFF) rather than
    /// UCS-4 (passes values through).
    utf32: bool,
}

impl Utf32Dec {
    pub(crate) fn new(bom: Bom, utf32: bool) -> Self {
        Utf32Dec {
            bom,
            le: bom == Bom::Le,
            utf32,
        }
    }
}

impl Decode for Utf32Dec {
    fn decode(&mut self, s: &[u8], pos: &mut usize, out: &mut Vec<u32>, limit: usize) {
        let mut i = *pos;
        let n = s.len();
        if i == 0 && self.bom == Bom::Detect && n >= 4 {
            if s[..4] == [0, 0, 0xFE, 0xFF] {
                i = 4;
            } else if s[..4] == [0xFF, 0xFE, 0, 0] {
                self.le = true;
                i = 4;
            }
        }
        while i < n && out.len() < limit {
            if i + 4 > n {
                out.push(ERR);
                i = n;
                break;
            }
            let b = [s[i], s[i + 1], s[i + 2], s[i + 3]];
            let v = if self.le {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            };
            i += 4;
            if self.utf32 && (v > 0x10FFFF || (0xD800..=0xDFFF).contains(&v)) {
                out.push(ERR);
            } else {
                out.push(v);
            }
        }
        *pos = i;
    }
}

pub(crate) struct Utf32Enc {
    le: bool,
    utf32: bool,
}

impl Utf32Enc {
    pub(crate) fn new(le: bool, utf32: bool) -> Self {
        Utf32Enc { le, utf32 }
    }
}

impl Encode for Utf32Enc {
    #[inline]
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        // UTF-32 takes surrogate code points; UCS-4 takes any value
        // (observed on PHP 8.5).
        if cp == ERR || (self.utf32 && cp > 0x10FFFF) {
            return false;
        }
        if self.le {
            out.extend_from_slice(&cp.to_le_bytes());
        } else {
            out.extend_from_slice(&cp.to_be_bytes());
        }
        true
    }
}
