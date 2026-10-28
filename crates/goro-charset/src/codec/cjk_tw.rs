//! BIG-5, CP950 and EUC-TW.
//!
//! Tables: unicode.org's BIG5.TXT, CNS11643.TXT and Microsoft's CP950.TXT
//! (deviations in tools/charset/gen_tables.py).

use super::{Decode, ERR, Encode};
use crate::Encoding;
use crate::tables::{big5 as t, rev};

#[inline]
fn big5_trail(b: u8) -> bool {
    (0x40..=0x7E).contains(&b) || (0xA1..=0xFE).contains(&b)
}

#[inline]
fn trail_offset(b: u8) -> usize {
    if b < 0x7F {
        (b - 0x40) as usize
    } else {
        (b - 0x62) as usize
    }
}

pub(crate) struct Big5Dec {
    cp950: bool,
}

impl Big5Dec {
    pub(crate) fn new(enc: Encoding) -> Self {
        Big5Dec {
            cp950: enc == Encoding::Cp950,
        }
    }
}

impl Decode for Big5Dec {
    fn decode(&mut self, s: &[u8], pos: &mut usize, out: &mut Vec<u32>, limit: usize) {
        let mut i = *pos;
        let n = s.len();
        let (first, last) = if self.cp950 { (0x81, 0xFE) } else { (0xA1, 0xF9) };
        while i < n && out.len() < limit {
            let b = s[i];
            if b < 0x80 {
                out.push(b as u32);
                i += 1;
                continue;
            }
            if b < first || b > last || i + 1 >= n {
                out.push(ERR);
                i += 1;
                continue;
            }
            let c = s[i + 1];
            if !big5_trail(c) {
                out.push(ERR);
                i += 2;
                continue;
            }
            if !self.cp950 && b == 0xC8 {
                // Big5 has no characters with lead byte 0xC8; with a valid
                // trail byte only the lead is an error (observed on PHP 8.5).
                out.push(ERR);
                i += 1;
                continue;
            }
            i += 2;
            let idx = (b - first) as usize * 157 + trail_offset(c);
            let cp = if self.cp950 { t::CP950[idx] } else { t::BIG5[idx] };
            out.push(if cp == 0 { ERR } else { cp as u32 });
        }
        *pos = i;
    }
}

pub(crate) struct Big5Enc {
    cp950: bool,
}

impl Big5Enc {
    pub(crate) fn new(enc: Encoding) -> Self {
        Big5Enc {
            cp950: enc == Encoding::Cp950,
        }
    }
}

impl Encode for Big5Enc {
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        if cp < 0x80 {
            out.push(cp as u8);
            return true;
        }
        let (i, first) = if self.cp950 {
            (rev(&t::CP950_REV_IDX, &t::CP950_REV_DATA, cp), 0x81)
        } else {
            (rev(&t::BIG5_REV_IDX, &t::BIG5_REV_DATA, cp), 0xA1)
        };
        if i == 0 {
            return false;
        }
        let i = i as usize - 1;
        let lead = (i / 157) as u8 + first;
        let o = (i % 157) as u8;
        out.extend_from_slice(&[lead, if o < 63 { o + 0x40 } else { o + 0x62 }]);
        true
    }
}

/// CNS 11643 planes EUC-TW reaches: 1 and 2 (also as 0x8E 0xA1/0xA2) and 14
/// (0x8E 0xAE), the planes of unicode.org's CNS11643.TXT.
fn cns_plane_slot(plane_byte: u8) -> Option<usize> {
    match plane_byte {
        0xA1 => Some(0),
        0xA2 => Some(1),
        0xAE => Some(2),
        _ => None,
    }
}

/// Whether a CNS 11643 row has any character (rows without any make the
/// row byte an error by itself; observed on PHP 8.5).
fn cns_row_used(slot: usize, row_byte: u8) -> bool {
    if !(0xA1..=0xFE).contains(&row_byte) {
        return false;
    }
    let base = slot * 94 * 94 + (row_byte - 0xA1) as usize * 94;
    t::CNS[base..base + 94].iter().any(|&v| v != 0)
}

pub(crate) struct EucTwDec;

impl Decode for EucTwDec {
    fn decode(&mut self, s: &[u8], pos: &mut usize, out: &mut Vec<u32>, limit: usize) {
        let mut i = *pos;
        let n = s.len();
        while i < n && out.len() < limit {
            let b = s[i];
            if b < 0x80 {
                out.push(b as u32);
                i += 1;
                continue;
            }
            if b == 0x8E {
                if i + 1 >= n {
                    out.push(ERR);
                    i += 1;
                    continue;
                }
                let Some(slot) = cns_plane_slot(s[i + 1]) else {
                    out.push(ERR);
                    i += 2;
                    continue;
                };
                if i + 2 >= n {
                    out.push(ERR);
                    i = n;
                    continue;
                }
                let r = s[i + 2];
                if !cns_row_used(slot, r) {
                    out.push(ERR);
                    i += 3;
                    continue;
                }
                if i + 3 >= n {
                    out.push(ERR);
                    i = n;
                    continue;
                }
                let c = s[i + 3];
                i += 4;
                out.push(cns_lookup(slot, r, c));
                continue;
            }
            if !cns_row_used(0, b) {
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
            out.push(cns_lookup(0, b, c));
        }
        *pos = i;
    }
}

fn cns_lookup(slot: usize, r: u8, c: u8) -> u32 {
    if !(0xA1..=0xFE).contains(&c) {
        return ERR;
    }
    match t::CNS[slot * 94 * 94 + (r - 0xA1) as usize * 94 + (c - 0xA1) as usize] {
        0 => ERR,
        cp => cp,
    }
}

pub(crate) struct EucTwEnc;

impl Encode for EucTwEnc {
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        if cp < 0x80 {
            out.push(cp as u8);
            return true;
        }
        let i = rev(&t::CNS_REV_IDX, &t::CNS_REV_DATA, cp);
        if i == 0 {
            return false;
        }
        let i = i as usize - 1;
        let slot = i / (94 * 94);
        let k = i % (94 * 94);
        let (r, c) = ((k / 94) as u8 + 0xA1, (k % 94) as u8 + 0xA1);
        if slot == 0 {
            out.extend_from_slice(&[r, c]);
        } else {
            out.extend_from_slice(&[0x8E, if slot == 1 { 0xA2 } else { 0xAE }, r, c]);
        }
        true
    }
}
