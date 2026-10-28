//! EUC-KR, UHC (CP949) and ISO-2022-KR.
//!
//! Tables: unicode.org's KSX1001.TXT and Microsoft's CP949.TXT (deviations
//! in tools/charset/gen_tables.py). ISO-2022-KR follows RFC 1557.

use super::{Decode, ERR, Encode, Subst, fail};
use crate::Encoding;
use crate::tables::{kr as t, rev};

#[inline]
fn uhc_trail(b: u8) -> Option<usize> {
    match b {
        0x41..=0x5A => Some((b - 0x41) as usize),
        0x61..=0x7A => Some((b - 0x61) as usize + 26),
        0x81..=0xFE => Some((b - 0x81) as usize + 52),
        _ => None,
    }
}

pub(crate) struct KrDec {
    uhc: bool,
}

impl KrDec {
    pub(crate) fn new(enc: Encoding) -> Self {
        KrDec {
            uhc: enc == Encoding::Uhc,
        }
    }
}

impl Decode for KrDec {
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
            if self.uhc {
                // UHC: leads 0x81..0xFD; 0xC9 (user-defined) takes only an
                // invalid trail byte with it (observed on PHP 8.5).
                if !(0x81..=0xFD).contains(&b) || i + 1 >= n {
                    out.push(ERR);
                    i += 1;
                    continue;
                }
                let c = s[i + 1];
                if b == 0xC9 && (0x41..=0xFE).contains(&c) {
                    out.push(ERR);
                    i += 1;
                    continue;
                }
                i += 2;
                let cp = match uhc_trail(c) {
                    Some(o) => t::UHC[(b - 0x81) as usize * 178 + o],
                    None => 0,
                };
                out.push(if cp == 0 { ERR } else { cp as u32 });
                continue;
            }
            // EUC-KR: rows 0xAD..0xAF and 0xC9 (user-defined) and 0xFE are
            // not lead bytes (observed on PHP 8.5).
            if !(0xA1..=0xFD).contains(&b) || (0xAD..=0xAF).contains(&b) || b == 0xC9 || i + 1 >= n
            {
                out.push(ERR);
                i += 1;
                continue;
            }
            let c = s[i + 1];
            i += 2;
            let cp = if (0xA1..=0xFE).contains(&c) {
                t::KSX1001[(b - 0xA1) as usize * 94 + (c - 0xA1) as usize]
            } else {
                0
            };
            out.push(if cp == 0 { ERR } else { cp as u32 });
        }
        *pos = i;
    }
}

pub(crate) struct KrEnc {
    uhc: bool,
}

impl KrEnc {
    pub(crate) fn new(enc: Encoding) -> Self {
        KrEnc {
            uhc: enc == Encoding::Uhc,
        }
    }
}

impl Encode for KrEnc {
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        if cp < 0x80 {
            out.push(cp as u8);
            return true;
        }
        if self.uhc {
            let i = rev(&t::UHC_REV_IDX, &t::UHC_REV_DATA, cp);
            if i == 0 {
                return false;
            }
            let i = i as usize - 1;
            let o = (i % 178) as u8;
            let trail = match o {
                0..=25 => o + 0x41,
                26..=51 => o - 26 + 0x61,
                _ => o - 52 + 0x81,
            };
            out.extend_from_slice(&[(i / 178) as u8 + 0x81, trail]);
            return true;
        }
        let i = rev(&t::KSX1001_REV_IDX, &t::KSX1001_REV_DATA, cp);
        if i == 0 {
            return false;
        }
        let i = i as usize - 1;
        out.extend_from_slice(&[(i / 94) as u8 + 0xA1, (i % 94) as u8 + 0xA1]);
        true
    }
}

/// ISO-2022-KR decoder: `ESC $ ) C` designates KS X 1001 to G1, SO and SI
/// shift in and out of it (test ext/mbstring/tests/iso2022kr_encoding.phpt).
#[derive(Default)]
pub(crate) struct Iso2022KrDec {
    ksc: bool,
}

impl Decode for Iso2022KrDec {
    fn decode(&mut self, s: &[u8], pos: &mut usize, out: &mut Vec<u32>, limit: usize) {
        let mut i = *pos;
        let n = s.len();
        while i < n && out.len() < limit {
            let b = s[i];
            match b {
                0x1B => {
                    // ESC $ ) C; a broken or truncated sequence is one error
                    // covering the bytes read.
                    let want = b"\x1B$)C";
                    let mut k = 1;
                    while k < 4 && i + k < n && s[i + k] == want[k] {
                        k += 1;
                    }
                    if k == 4 {
                        i += 4;
                    } else {
                        out.push(ERR);
                        i = (i + k + 1).min(n);
                    }
                }
                0x0E => {
                    self.ksc = true;
                    i += 1;
                }
                0x0F => {
                    self.ksc = false;
                    i += 1;
                }
                0x80.. => {
                    out.push(ERR);
                    i += 1;
                }
                ..=0x20 | 0x7F if self.ksc => {
                    // Shifted out, control characters are errors (observed
                    // on PHP 8.5).
                    out.push(ERR);
                    i += 1;
                }
                0x21..=0x7E if self.ksc => {
                    if i + 1 >= n {
                        out.push(ERR);
                        i += 1;
                        continue;
                    }
                    let c = s[i + 1];
                    i += 2;
                    // ISO-2022-KR lacks the KS X 1001:1998 additions at
                    // 0x2266/0x2267 (observed on PHP 8.5).
                    let cp = if b < 0x7E && (0x21..=0x7E).contains(&c) && !(b == 0x22 && (c == 0x66 || c == 0x67)) {
                        t::KSX1001[(b - 0x21) as usize * 94 + (c - 0x21) as usize]
                    } else {
                        0
                    };
                    out.push(if cp == 0 { ERR } else { cp as u32 });
                }
                _ => {
                    out.push(b as u32);
                    i += 1;
                }
            }
        }
        *pos = i;
    }
}

/// ISO-2022-KR encoder: the designation goes at the start of any non-empty
/// output, and the output ends in ASCII (test
/// ext/mbstring/tests/iso2022kr_encoding.phpt).
#[derive(Default)]
pub(crate) struct Iso2022KrEnc {
    started: bool,
    ksc: bool,
}

impl Encode for Iso2022KrEnc {
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        if cp < 0x80 {
            if self.ksc {
                out.push(0x0F);
                self.ksc = false;
            }
            out.push(cp as u8);
            return true;
        }
        // (The encoder does write the euro and registered signs, which the
        // decoder rejects; observed on PHP 8.5.)
        let i = rev(&t::KSX1001_REV_IDX, &t::KSX1001_REV_DATA, cp);
        // PHP 8.5 writes a code point KS X 1001 lacks in U+2121..U+8080 as
        // its own two bytes, shifted out (observed on PHP 8.5).
        let bytes = if i != 0 {
            let i = i as usize - 1;
            [(i / 94) as u8 + 0x21, (i % 94) as u8 + 0x21]
        } else if (0x2121..=0x8080).contains(&cp) {
            [(cp >> 8) as u8, cp as u8]
        } else {
            return false;
        };
        if !self.ksc {
            out.push(0x0E);
            self.ksc = true;
        }
        out.extend_from_slice(&bytes);
        true
    }

    fn encode(&mut self, cps: &[u32], out: &mut Vec<u8>, sub: &Subst, errors: &mut usize) {
        if !self.started && !cps.is_empty() {
            out.extend_from_slice(b"\x1B$)C");
            self.started = true;
        }
        for &cp in cps {
            if !self.put(cp, out) {
                fail(self, cp, out, sub, errors);
            }
        }
    }

    fn finish(&mut self, out: &mut Vec<u8>, _sub: &Subst, _errors: &mut usize) {
        if self.ksc {
            out.push(0x0F);
            self.ksc = false;
        }
    }
}
