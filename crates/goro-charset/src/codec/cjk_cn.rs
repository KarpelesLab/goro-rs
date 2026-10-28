//! EUC-CN, CP936, GB18030, GB18030-2022 and HZ.
//!
//! GB18030-2022 follows the WHATWG Encoding Standard's gb18030 index and
//! ranges; the other tables are derived from Microsoft's CP936.TXT (see
//! tools/charset/gen_tables.py).

use super::{Decode, ERR, Encode, Subst};
use crate::Encoding;
use crate::tables::{gb as t, rev};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    EucCn,
    Cp936,
    Gb2005,
    Gb2022,
}

fn kind(enc: Encoding) -> Kind {
    match enc {
        Encoding::EucCn => Kind::EucCn,
        Encoding::Cp936 => Kind::Cp936,
        Encoding::Gb18030 => Kind::Gb2005,
        _ => Kind::Gb2022,
    }
}

#[inline]
fn gbk_pointer(lead: u8, trail: u8) -> usize {
    (lead - 0x81) as usize * 190 + (trail - if trail < 0x7F { 0x40 } else { 0x41 }) as usize
}

#[inline]
fn gbk_trail(b: u8) -> bool {
    (0x40..=0x7E).contains(&b) || (0x80..=0xFE).contains(&b)
}

/// The code point of a GB18030 four-byte pointer (WHATWG "index gb18030
/// ranges code point"), or [`ERR`].
fn gb_ranges_cp(pointer: u32) -> u32 {
    if (189000..=1237575).contains(&pointer) {
        return 0x10000 + pointer - 189000;
    }
    if pointer > 39419 {
        return ERR;
    }
    let r = &t::GB18030_RANGES;
    let i = r.partition_point(|e| e.0 <= pointer) - 1;
    r[i].1 + pointer - r[i].0
}

/// The four-byte pointer of a code point outside the two-byte table.
fn gb_ranges_pointer(cp: u32) -> Option<u32> {
    if cp >= 0x10000 {
        return (cp <= 0x10FFFF).then(|| 189000 + cp - 0x10000);
    }
    if (0xD800..=0xDFFF).contains(&cp) {
        return None;
    }
    let r = &t::GB18030_RANGES;
    let i = r.partition_point(|e| e.1 <= cp) - 1;
    Some(r[i].0 + cp - r[i].1)
}

pub(crate) struct GbDec {
    kind: Kind,
}

impl GbDec {
    pub(crate) fn new(enc: Encoding) -> Self {
        GbDec { kind: kind(enc) }
    }
}

impl Decode for GbDec {
    fn decode(&mut self, s: &[u8], pos: &mut usize, out: &mut Vec<u32>, limit: usize) {
        let mut i = *pos;
        let n = s.len();
        let kind = self.kind;
        while i < n && out.len() < limit {
            let b = s[i];
            if b < 0x80 {
                out.push(b as u32);
                i += 1;
                continue;
            }
            if kind == Kind::EucCn {
                if !((0xA1..=0xA9).contains(&b) || (0xB0..=0xF7).contains(&b)) {
                    out.push(ERR);
                    i += 1;
                    continue;
                }
                if i + 1 >= n {
                    out.push(ERR);
                    i += 1;
                    continue;
                }
                let c = s[i + 1];
                i += 2;
                let cp = if (0xA1..=0xFE).contains(&c) {
                    t::GB2312[(b - 0xA1) as usize * 94 + (c - 0xA1) as usize]
                } else {
                    0
                };
                out.push(if cp == 0 { ERR } else { cp as u32 });
                continue;
            }
            if b == 0x80 || b == 0xFF {
                // CP936 has the euro sign at 0x80 and U+F8F5 at 0xFF, as
                // Windows (observed on PHP 8.5).
                out.push(match (kind, b) {
                    (Kind::Cp936, 0x80) => 0x20AC,
                    (Kind::Cp936, _) => 0xF8F5,
                    _ => ERR,
                });
                i += 1;
                continue;
            }
            if i + 1 >= n {
                out.push(ERR);
                i += 1;
                continue;
            }
            let c = s[i + 1];
            if kind != Kind::Cp936 && (0x30..=0x39).contains(&c) {
                // A four-byte sequence, for leads that can start a valid
                // one; PHP consumes the bytes read up to the first bad one
                // (observed on PHP 8.5).
                if !((0x81..=0x84).contains(&b) || (0x90..=0xE3).contains(&b)) {
                    out.push(ERR);
                    i += 2;
                    continue;
                }
                if i + 2 >= n || !(0x81..=0xFE).contains(&s[i + 2]) || i + 3 >= n {
                    out.push(ERR);
                    i = (i + 3).min(n);
                    continue;
                }
                let (d, e) = (s[i + 2], s[i + 3]);
                i += 4;
                if !(0x30..=0x39).contains(&e) {
                    out.push(ERR);
                    continue;
                }
                let pointer = (((b - 0x81) as u32 * 10 + (c - 0x30) as u32) * 126
                    + (d - 0x81) as u32)
                    * 10
                    + (e - 0x30) as u32;
                // The ranges are GB18030-2005's; the 2022 edition gives the
                // four-byte codes of the 19 moved characters their old PUA
                // code points (observed on PHP 8.5).
                let mut cp = gb_ranges_cp(pointer);
                if kind == Kind::Gb2022
                    && let Some(sw) = t::GB18030_2005_SWAPS.iter().find(|s| s.2 == cp)
                {
                    cp = sw.1;
                }
                out.push(cp);
                continue;
            }
            i += 2;
            if !gbk_trail(c) {
                out.push(ERR);
                continue;
            }
            let p = gbk_pointer(b, c);
            let cp = match kind {
                Kind::Cp936 => t::CP936[p] as u32,
                _ => {
                    let code = ((b as u16) << 8) | c as u16;
                    match t::GB18030_2005_SWAPS.iter().find(|s| s.0 == code) {
                        Some(sw) if kind == Kind::Gb2005 => sw.1,
                        _ => t::GB18030_2022[p] as u32,
                    }
                }
            };
            out.push(if cp == 0 { ERR } else { cp });
        }
        *pos = i;
    }
}

pub(crate) struct GbEnc {
    kind: Kind,
}

impl GbEnc {
    pub(crate) fn new(enc: Encoding) -> Self {
        GbEnc { kind: kind(enc) }
    }
}

#[inline]
fn push_gbk(p: usize, out: &mut Vec<u8>) {
    let lead = (p / 190) as u8 + 0x81;
    let t = (p % 190) as u8;
    out.extend_from_slice(&[lead, if t < 0x3F { t + 0x40 } else { t + 0x41 }]);
}

impl Encode for GbEnc {
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        if cp < 0x80 {
            out.push(cp as u8);
            return true;
        }
        match self.kind {
            Kind::EucCn => {
                let i = rev(&t::GB2312_REV_IDX, &t::GB2312_REV_DATA, cp);
                if i == 0 {
                    return false;
                }
                let i = i as usize - 1;
                out.extend_from_slice(&[(i / 94) as u8 + 0xA1, (i % 94) as u8 + 0xA1]);
                true
            }
            Kind::Cp936 => {
                match cp {
                    0x20AC => {
                        out.push(0x80);
                        return true;
                    }
                    0xF8F5 => {
                        out.push(0xFF);
                        return true;
                    }
                    _ => {}
                }
                let p = rev(&t::CP936_REV_IDX, &t::CP936_REV_DATA, cp);
                if p == 0 {
                    return false;
                }
                push_gbk(p as usize - 1, out);
                true
            }
            Kind::Gb2005 | Kind::Gb2022 => {
                let mut cp = cp;
                if let Some(sw) = t::GB18030_2005_SWAPS
                    .iter()
                    .find(|s| s.1 == cp || s.2 == cp)
                {
                    let two_byte = if self.kind == Kind::Gb2005 { sw.1 } else { sw.2 };
                    if two_byte == cp {
                        out.extend_from_slice(&sw.0.to_be_bytes());
                        return true;
                    }
                    // The other one has the four-byte code of the standard
                    // code point.
                    cp = sw.2;
                } else {
                    let p = rev(&t::GB18030_2022_REV_IDX, &t::GB18030_2022_REV_DATA, cp);
                    if p != 0 {
                        push_gbk(p as usize - 1, out);
                        return true;
                    }
                }
                let Some(mut p) = gb_ranges_pointer(cp) else {
                    return false;
                };
                let b4 = (p % 10) as u8 + 0x30;
                p /= 10;
                let b3 = (p % 126) as u8 + 0x81;
                p /= 126;
                let b2 = (p % 10) as u8 + 0x30;
                let b1 = (p / 10) as u8 + 0x81;
                out.extend_from_slice(&[b1, b2, b3, b4]);
                true
            }
        }
    }
}

/// HZ (RFC 1843): GB 2312 inside `~{` ... `~}`, `~~` for a tilde and `~`
/// before a newline for a line continuation.
#[derive(Default)]
pub(crate) struct HzDec {
    gb: bool,
}

impl Decode for HzDec {
    fn decode(&mut self, s: &[u8], pos: &mut usize, out: &mut Vec<u32>, limit: usize) {
        let mut i = *pos;
        let n = s.len();
        while i < n && out.len() < limit {
            let b = s[i];
            if b == b'~' {
                if i + 1 >= n {
                    // A lone tilde at the end is dropped (test
                    // ext/mbstring/tests/hz_encoding.phpt).
                    i += 1;
                    continue;
                }
                let c = s[i + 1];
                i += 2;
                match c {
                    b'~' if !self.gb => out.push(b'~' as u32),
                    b'\n' => {}
                    b'{' if !self.gb => self.gb = true,
                    b'}' if self.gb => self.gb = false,
                    _ => out.push(ERR),
                }
                continue;
            }
            if b >= 0x80 {
                out.push(ERR);
                i += 1;
                continue;
            }
            if !self.gb {
                out.push(b as u32);
                i += 1;
                continue;
            }
            if !(0x21..=0x7E).contains(&b) {
                out.push(b as u32);
                i += 1;
                continue;
            }
            if i + 1 >= n {
                out.push(ERR);
                i += 1;
                continue;
            }
            let c = s[i + 1];
            i += 2;
            let cp = if (0x21..=0x7E).contains(&c) {
                if b == 0x21 && c == 0x2C {
                    // HZ's GB 2312 has PARALLEL TO here (test
                    // ext/mbstring/tests/hz_encoding.phpt).
                    0x2225
                } else {
                    t::GB2312[(b - 0x21) as usize * 94 + (c - 0x21) as usize]
                }
            } else {
                0
            };
            out.push(if cp == 0 { ERR } else { cp as u32 });
        }
        *pos = i;
    }
}

#[derive(Default)]
pub(crate) struct HzEnc {
    gb: bool,
}

impl Encode for HzEnc {
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        if cp < 0x80 {
            if self.gb {
                out.extend_from_slice(b"~}");
                self.gb = false;
            }
            if cp == b'~' as u32 {
                out.extend_from_slice(b"~~");
            } else {
                out.push(cp as u8);
            }
            return true;
        }
        // 0x212C is PARALLEL TO here, so DOUBLE VERTICAL LINE has no code
        // (observed on PHP 8.5).
        let i = rev(&t::GB2312_REV_IDX, &t::GB2312_REV_DATA, cp);
        if i == 0 || cp == 0x2016 {
            return false;
        }
        if !self.gb {
            out.extend_from_slice(b"~{");
            self.gb = true;
        }
        let i = i as usize - 1;
        out.extend_from_slice(&[(i / 94) as u8 + 0x21, (i % 94) as u8 + 0x21]);
        true
    }

    fn finish(&mut self, out: &mut Vec<u8>, _sub: &Subst, _errors: &mut usize) {
        if self.gb {
            out.extend_from_slice(b"~}");
            self.gb = false;
        }
    }
}
