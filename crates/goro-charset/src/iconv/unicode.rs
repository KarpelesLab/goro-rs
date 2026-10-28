//! The Unicode encoding forms as glibc's iconv implements them (observed on
//! PHP 8.5 with glibc 2.43):
//!
//! - UTF-8 decodes the historical five- and six-byte forms up to U+7FFFFFFF,
//!   but rejects overlong forms and surrogates. A lead byte followed only by
//!   continuation bytes at the end of the input is incomplete.
//! - UTF-16 and UTF-32 without a byte order mark are little-endian; "UTF-16"
//!   and "UTF-32" consume a leading BOM and write one (FF FE ...) before the
//!   first character. UNICODE is UCS-2 with the same BOM handling.
//! - UCS-2 is little-endian, UCS-4 big-endian, UCS-4LE (WCHAR_T) little-endian;
//!   UCS-4 accepts any value up to 0x7FFFFFFF, surrogates included.

use super::convert::DecodeStatus;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Order {
    Le,
    Be,
}

#[inline]
fn rd16(o: Order, b: &[u8]) -> u32 {
    u32::from(match o {
        Order::Le => u16::from_le_bytes([b[0], b[1]]),
        Order::Be => u16::from_be_bytes([b[0], b[1]]),
    })
}

#[inline]
fn rd32(o: Order, b: &[u8]) -> u32 {
    match o {
        Order::Le => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        Order::Be => u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
    }
}

#[inline]
fn wr16(o: Order, v: u32, out: &mut Vec<u8>) {
    let v = v as u16;
    out.extend_from_slice(&match o {
        Order::Le => v.to_le_bytes(),
        Order::Be => v.to_be_bytes(),
    });
}

#[inline]
fn wr32(o: Order, v: u32, out: &mut Vec<u8>) {
    out.extend_from_slice(&match o {
        Order::Le => v.to_le_bytes(),
        Order::Be => v.to_be_bytes(),
    });
}

/// Length of the run of ASCII bytes at the start of `s`, a word at a time.
#[inline]
pub(crate) fn ascii_prefix(s: &[u8]) -> usize {
    const HI: u64 = 0x8080_8080_8080_8080;
    let mut i = 0;
    while i + 8 <= s.len() {
        let w = u64::from_le_bytes([
            s[i],
            s[i + 1],
            s[i + 2],
            s[i + 3],
            s[i + 4],
            s[i + 5],
            s[i + 6],
            s[i + 7],
        ]);
        if w & HI != 0 {
            return i + (w & HI).trailing_zeros() as usize / 8;
        }
        i += 8;
    }
    while i < s.len() && s[i] < 0x80 {
        i += 1;
    }
    i
}

/// UTF-8 decoding (glibc flavour, see the module documentation).
pub(crate) fn decode_utf8(
    input: &[u8],
    pos: &mut usize,
    out: &mut Vec<u32>,
    limit: usize,
) -> DecodeStatus {
    let len = input.len();
    while *pos < len && out.len() < limit {
        let b = input[*pos];
        if b < 0x80 {
            let room = limit - out.len();
            let end = pos.saturating_add(room).min(len);
            let n = ascii_prefix(&input[*pos..end]).max(1);
            out.extend(input[*pos..*pos + n].iter().map(|&c| u32::from(c)));
            *pos += n;
            continue;
        }
        let (need, min) = match b {
            0xC2..=0xDF => (1, 0x80),
            0xE0..=0xEF => (2, 0x800),
            0xF0..=0xF7 => (3, 0x1_0000),
            0xF8..=0xFB => (4, 0x20_0000),
            0xFC..=0xFD => (5, 0x400_0000),
            _ => return DecodeStatus::Illegal(1),
        };
        let mut cp = u32::from(b) & (0x7F >> (need + 1));
        for k in 1..=need {
            match input.get(*pos + k) {
                None => return DecodeStatus::Incomplete,
                Some(&c) if c & 0xC0 == 0x80 => cp = (cp << 6) | u32::from(c & 0x3F),
                Some(_) => return DecodeStatus::Illegal(1),
            }
        }
        if cp < min || (0xD800..0xE000).contains(&cp) {
            return DecodeStatus::Illegal(1);
        }
        out.push(cp);
        *pos += need + 1;
    }
    DecodeStatus::Ok
}

/// UTF-8 encoding up to U+7FFFFFFF, rejecting surrogates.
#[inline]
pub(crate) fn encode_utf8(cps: &[u32], out: &mut Vec<u8>) -> Result<(), usize> {
    for (i, &cp) in cps.iter().enumerate() {
        if cp < 0x80 {
            out.push(cp as u8);
        } else if cp < 0x800 {
            out.extend_from_slice(&[0xC0 | (cp >> 6) as u8, 0x80 | (cp & 0x3F) as u8]);
        } else if cp < 0x1_0000 {
            if (0xD800..0xE000).contains(&cp) {
                return Err(i);
            }
            out.extend_from_slice(&[
                0xE0 | (cp >> 12) as u8,
                0x80 | ((cp >> 6) & 0x3F) as u8,
                0x80 | (cp & 0x3F) as u8,
            ]);
        } else if cp < 0x20_0000 {
            out.extend_from_slice(&[
                0xF0 | (cp >> 18) as u8,
                0x80 | ((cp >> 12) & 0x3F) as u8,
                0x80 | ((cp >> 6) & 0x3F) as u8,
                0x80 | (cp & 0x3F) as u8,
            ]);
        } else if cp < 0x400_0000 {
            out.extend_from_slice(&[
                0xF8 | (cp >> 24) as u8,
                0x80 | ((cp >> 18) & 0x3F) as u8,
                0x80 | ((cp >> 12) & 0x3F) as u8,
                0x80 | ((cp >> 6) & 0x3F) as u8,
                0x80 | (cp & 0x3F) as u8,
            ]);
        } else if cp < 0x8000_0000 {
            out.extend_from_slice(&[
                0xFC | (cp >> 30) as u8,
                0x80 | ((cp >> 24) & 0x3F) as u8,
                0x80 | ((cp >> 18) & 0x3F) as u8,
                0x80 | ((cp >> 12) & 0x3F) as u8,
                0x80 | ((cp >> 6) & 0x3F) as u8,
                0x80 | (cp & 0x3F) as u8,
            ]);
        } else {
            return Err(i);
        }
    }
    Ok(())
}

/// Decoder for the UTF-16, UCS-2, UTF-32 and UCS-4 families.
pub(crate) struct WideDecoder {
    pub order: Order,
    /// Look for a byte order mark at the start of the input.
    pub bom: bool,
    /// 2 or 4 bytes per unit.
    pub width: u8,
    /// UTF-16 (surrogate pairs) rather than UCS-2; UTF-32 rather than UCS-4.
    pub utf: bool,
    started: bool,
}

impl WideDecoder {
    pub(crate) fn new(order: Order, bom: bool, width: u8, utf: bool) -> Self {
        WideDecoder {
            order,
            bom,
            width,
            utf,
            started: false,
        }
    }

    pub(crate) fn decode(
        &mut self,
        input: &[u8],
        pos: &mut usize,
        out: &mut Vec<u32>,
        limit: usize,
    ) -> DecodeStatus {
        let w = usize::from(self.width);
        if !self.started && self.bom {
            if input.len() - *pos < w {
                return if *pos == input.len() {
                    DecodeStatus::Ok
                } else {
                    DecodeStatus::Incomplete
                };
            }
            let s = &input[*pos..];
            let le = if w == 2 {
                s[0] == 0xFF && s[1] == 0xFE
            } else {
                s[..4] == [0xFF, 0xFE, 0, 0]
            };
            let be = if w == 2 {
                s[0] == 0xFE && s[1] == 0xFF
            } else {
                s[..4] == [0, 0, 0xFE, 0xFF]
            };
            if le {
                self.order = Order::Le;
                *pos += w;
            } else if be {
                self.order = Order::Be;
                *pos += w;
            }
        }
        self.started = true;
        let o = self.order;
        let len = input.len();
        while *pos < len && out.len() < limit {
            if len - *pos < w {
                return DecodeStatus::Incomplete;
            }
            let s = &input[*pos..];
            if w == 2 {
                let u = rd16(o, s);
                if (0xD800..0xE000).contains(&u) {
                    if !self.utf || u >= 0xDC00 {
                        return DecodeStatus::Illegal(2);
                    }
                    if s.len() < 4 {
                        return DecodeStatus::Incomplete;
                    }
                    let lo = rd16(o, &s[2..]);
                    if !(0xDC00..0xE000).contains(&lo) {
                        return DecodeStatus::Illegal(2);
                    }
                    out.push(0x1_0000 + ((u - 0xD800) << 10) + (lo - 0xDC00));
                    *pos += 4;
                } else {
                    out.push(u);
                    *pos += 2;
                }
            } else {
                let u = rd32(o, s);
                let bad = if self.utf {
                    u > 0x10_FFFF || (0xD800..0xE000).contains(&u)
                } else {
                    u > 0x7FFF_FFFF
                };
                if bad {
                    return DecodeStatus::Illegal(4);
                }
                out.push(u);
                *pos += 4;
            }
        }
        DecodeStatus::Ok
    }
}

/// Encoder for the UTF-16, UCS-2, UTF-32 and UCS-4 families.
pub(crate) struct WideEncoder {
    pub order: Order,
    /// Write a byte order mark before the first character.
    pub bom: bool,
    pub width: u8,
    pub utf: bool,
}

impl WideEncoder {
    pub(crate) fn can_encode(&self, cp: u32) -> bool {
        match (self.width, self.utf) {
            (2, true) | (4, true) => cp <= 0x10_FFFF && !(0xD800..0xE000).contains(&cp),
            (2, false) => cp < 0x1_0000 && !(0xD800..0xE000).contains(&cp),
            _ => cp <= 0x7FFF_FFFF,
        }
    }

    pub(crate) fn encode(&mut self, cps: &[u32], out: &mut Vec<u8>) -> Result<(), usize> {
        let o = self.order;
        // The byte order mark goes out with the first character the encoder
        // sees, even one it can't encode (observed).
        if self.bom && !cps.is_empty() {
            self.bom = false;
            if self.width == 2 {
                wr16(o, 0xFEFF, out);
            } else {
                wr32(o, 0xFEFF, out);
            }
        }
        for (i, &cp) in cps.iter().enumerate() {
            if !self.can_encode(cp) {
                return Err(i);
            }
            if self.width == 4 {
                wr32(o, cp, out);
            } else if cp >= 0x1_0000 {
                let c = cp - 0x1_0000;
                wr16(o, 0xD800 | (c >> 10), out);
                wr16(o, 0xDC00 | (c & 0x3FF), out);
            } else {
                wr16(o, cp, out);
            }
        }
        Ok(())
    }
}
