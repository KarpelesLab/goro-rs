//! ASCII, 8bit/ISO-8859-1 and the table-driven single-byte encodings.

use super::{Decode, ERR, Encode, Subst, fail};
use crate::Encoding;
use crate::tables::single as t;
use std::sync::OnceLock;

/// A single-byte encoding whose lower half is ASCII.
pub(crate) struct Table {
    /// Code points of 0x80..=0xFF; 0xFFFF where unassigned.
    hi: &'static [u16; 128],
    /// (code point, byte) sorted by code point, built on first use.
    rev: OnceLock<Vec<(u16, u8)>>,
    /// Whether some ASCII characters are written with upper-half bytes.
    ascii_override: bool,
}

impl Table {
    const fn new(hi: &'static [u16; 128]) -> Table {
        let mut ascii_override = false;
        let mut i = 0;
        while i < 128 {
            if hi[i] < 0x80 {
                ascii_override = true;
            }
            i += 1;
        }
        Table {
            hi,
            rev: OnceLock::new(),
            ascii_override,
        }
    }

    fn rev(&self) -> &[(u16, u8)] {
        self.rev.get_or_init(|| {
            let mut v: Vec<(u16, u8)> = Vec::with_capacity(128);
            for (i, &cp) in self.hi.iter().enumerate() {
                if cp != 0xFFFF {
                    v.push((cp, 0x80 + i as u8));
                }
            }
            v.sort();
            v.dedup_by_key(|e| e.0);
            v
        })
    }

    /// The byte for `cp`, if any.
    #[inline]
    pub(crate) fn lookup(&self, cp: u32) -> Option<u8> {
        // ArmSCII-8 writes ( ) , - . with its Armenian punctuation bytes
        // (observed on PHP 8.5).
        if cp < 0x80 && !self.ascii_override {
            return Some(cp as u8);
        }
        if cp > 0xFFFF {
            return None;
        }
        let rev = self.rev();
        match rev.binary_search_by_key(&(cp as u16), |e| e.0) {
            Ok(i) => Some(rev[i].1),
            Err(_) if cp < 0x80 => Some(cp as u8),
            Err(_) => None,
        }
    }

    /// The code point of byte `b`, or [`ERR`].
    #[inline]
    pub(crate) fn get(&self, b: u8) -> u32 {
        if b < 0x80 {
            b as u32
        } else {
            match self.hi[(b - 0x80) as usize] {
                0xFFFF => ERR,
                cp => cp as u32,
            }
        }
    }
}

macro_rules! tables {
    ($($s:ident = $t:ident;)*) => {
        $(static $s: Table = Table::new(&t::$t);)*
    };
}

tables! {
    ISO8859_2 = ISO8859_2;
    ISO8859_3 = ISO8859_3;
    ISO8859_4 = ISO8859_4;
    ISO8859_5 = ISO8859_5;
    ISO8859_6 = ISO8859_6;
    ISO8859_7 = ISO8859_7;
    ISO8859_8 = ISO8859_8;
    ISO8859_9 = ISO8859_9;
    ISO8859_10 = ISO8859_10;
    ISO8859_13 = ISO8859_13;
    ISO8859_14 = ISO8859_14;
    ISO8859_15 = ISO8859_15;
    ISO8859_16 = ISO8859_16;
    CP1251 = CP1251;
    CP1252 = CP1252;
    CP1254 = CP1254;
    CP866 = CP866;
    CP850 = CP850;
    KOI8R = KOI8R;
    KOI8U = KOI8U;
    ARMSCII8 = ARMSCII8;
}

/// The table of a table-driven single-byte encoding.
pub(crate) fn table(enc: Encoding) -> Option<&'static Table> {
    use Encoding::*;
    Some(match enc {
        Iso8859_2 => &ISO8859_2,
        Iso8859_3 => &ISO8859_3,
        Iso8859_4 => &ISO8859_4,
        Iso8859_5 => &ISO8859_5,
        Iso8859_6 => &ISO8859_6,
        Iso8859_7 => &ISO8859_7,
        Iso8859_8 => &ISO8859_8,
        Iso8859_9 => &ISO8859_9,
        Iso8859_10 => &ISO8859_10,
        Iso8859_13 => &ISO8859_13,
        Iso8859_14 => &ISO8859_14,
        Iso8859_15 => &ISO8859_15,
        Iso8859_16 => &ISO8859_16,
        Cp1251 => &CP1251,
        Cp1252 => &CP1252,
        Cp1254 => &CP1254,
        Cp866 => &CP866,
        Cp850 => &CP850,
        Koi8R => &KOI8R,
        Koi8U => &KOI8U,
        ArmScii8 => &ARMSCII8,
        _ => return None,
    })
}

pub(crate) struct SingleDec(&'static Table);

impl SingleDec {
    pub(crate) fn new(t: &'static Table) -> Self {
        SingleDec(t)
    }
}

impl Decode for SingleDec {
    fn decode(&mut self, s: &[u8], pos: &mut usize, out: &mut Vec<u32>, limit: usize) {
        let room = limit.saturating_sub(out.len()).max(1);
        let end = (*pos + room).min(s.len());
        out.extend(s[*pos..end].iter().map(|&b| self.0.get(b)));
        *pos = end;
    }
}

pub(crate) struct SingleEnc(&'static Table);

impl SingleEnc {
    pub(crate) fn new(t: &'static Table) -> Self {
        SingleEnc(t)
    }
}

impl Encode for SingleEnc {
    #[inline]
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        match self.0.lookup(cp) {
            Some(b) => {
                out.push(b);
                true
            }
            None => false,
        }
    }

    fn encode(&mut self, cps: &[u32], out: &mut Vec<u8>, sub: &Subst, errors: &mut usize) {
        out.reserve(cps.len());
        let fast = !self.0.ascii_override;
        for &cp in cps {
            if cp < 0x80 && fast {
                out.push(cp as u8);
            } else if !self.put(cp, out) {
                fail(self, cp, out, sub, errors);
            }
        }
    }
}

/// ASCII and 7bit: bytes 0x80..=0xFF are invalid.
pub(crate) struct AsciiDec;

impl Decode for AsciiDec {
    fn decode(&mut self, s: &[u8], pos: &mut usize, out: &mut Vec<u32>, limit: usize) {
        let room = limit.saturating_sub(out.len()).max(1);
        let end = (*pos + room).min(s.len());
        out.extend(
            s[*pos..end]
                .iter()
                .map(|&b| if b < 0x80 { b as u32 } else { ERR }),
        );
        *pos = end;
    }
}

pub(crate) struct AsciiEnc;

impl Encode for AsciiEnc {
    #[inline]
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        if cp < 0x80 {
            out.push(cp as u8);
            true
        } else {
            false
        }
    }
}

/// ISO-8859-1 and 8bit: every byte is the code point of the same value.
pub(crate) struct Latin1Dec;

impl Decode for Latin1Dec {
    fn decode(&mut self, s: &[u8], pos: &mut usize, out: &mut Vec<u32>, limit: usize) {
        let room = limit.saturating_sub(out.len()).max(1);
        let end = (*pos + room).min(s.len());
        out.extend(s[*pos..end].iter().map(|&b| b as u32));
        *pos = end;
    }
}

pub(crate) struct Latin1Enc;

impl Encode for Latin1Enc {
    #[inline]
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        if cp < 0x100 {
            out.push(cp as u8);
            true
        } else {
            false
        }
    }

    fn encode(&mut self, cps: &[u32], out: &mut Vec<u8>, sub: &Subst, errors: &mut usize) {
        out.reserve(cps.len());
        for &cp in cps {
            if cp < 0x100 {
                out.push(cp as u8);
            } else {
                fail(self, cp, out, sub, errors);
            }
        }
    }
}
