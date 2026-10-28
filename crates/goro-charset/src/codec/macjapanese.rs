//! The MacJapanese (SJIS-mac) encoder. Decoding shares the Shift_JIS
//! decoder; the table is Apple's JAPANESE.TXT.

use super::{Encode, Subst, fail};
use crate::tables::{jis as t, rev};

/// How a buffered run of code points relates to MacJapanese's
/// multi-code-point mappings.
enum MacSeq {
    /// It is one of them, with this code.
    Complete(u16),
    /// It starts at least one of them.
    Prefix,
    /// Neither.
    None,
}

fn mac_seq(buf: &[u32]) -> MacSeq {
    let seqs = &t::SJISMAC_SEQS;
    let start = seqs.partition_point(|e| e.0 < buf);
    match seqs.get(start) {
        Some(e) if e.0 == buf => MacSeq::Complete(e.1),
        Some(e) if e.0.starts_with(buf) => MacSeq::Prefix,
        _ => MacSeq::None,
    }
}

/// Apple's table maps some characters to sequences: a U+F860..U+F862 prefix
/// and two to four characters, or a character and a U+F87A/U+F87E/U+F87F/
/// U+20DD suffix. A broken prefix sequence makes every character in it an
/// error; a base character not followed by its suffix is written alone (test
/// ext/mbstring/tests/sjismac_encoding.phpt).
pub(crate) struct MacEnc {
    buf: Vec<u32>,
}

impl MacEnc {
    pub(crate) fn new() -> Self {
        MacEnc {
            buf: Vec::with_capacity(5),
        }
    }

    fn single(cp: u32) -> Option<u16> {
        if cp < 0x80 && cp != 0x5C {
            return Some(cp as u16);
        }
        match rev(&t::SJISMAC_REV_IDX, &t::SJISMAC_REV_DATA, cp) {
            0 => None,
            c => Some(c),
        }
    }

    fn emit(code: u16, out: &mut Vec<u8>) {
        if code > 0xFF {
            out.extend_from_slice(&code.to_be_bytes());
        } else {
            out.push(code as u8);
        }
    }

    /// Writes out a buffered run that did not complete a sequence.
    fn drain(&mut self, out: &mut Vec<u8>, sub: &Subst, errors: &mut usize) {
        let buf = std::mem::take(&mut self.buf);
        let prefixed = matches!(buf.first(), Some(0xF860..=0xF862));
        for &c in &buf {
            match Self::single(c) {
                Some(code) if !prefixed => Self::emit(code, out),
                _ => fail(self, c, out, sub, errors),
            }
        }
        self.buf = buf;
        self.buf.clear();
    }

    fn step(&mut self, cp: u32, out: &mut Vec<u8>, sub: &Subst, errors: &mut usize) {
        if !self.buf.is_empty() {
            self.buf.push(cp);
            match mac_seq(&self.buf) {
                MacSeq::Complete(code) => {
                    self.buf.clear();
                    Self::emit(code, out);
                    return;
                }
                MacSeq::Prefix => return,
                MacSeq::None => {
                    self.buf.pop();
                    self.drain(out, sub, errors);
                }
            }
        }
        if let MacSeq::Prefix = mac_seq(std::slice::from_ref(&cp)) {
            self.buf.push(cp);
            return;
        }
        match Self::single(cp) {
            Some(code) => Self::emit(code, out),
            None => fail(self, cp, out, sub, errors),
        }
    }
}

impl Encode for MacEnc {
    /// Used only to write substitutes, which are never part of a sequence.
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        match Self::single(cp) {
            Some(code) => {
                Self::emit(code, out);
                true
            }
            None => false,
        }
    }

    fn encode(&mut self, cps: &[u32], out: &mut Vec<u8>, sub: &Subst, errors: &mut usize) {
        for &cp in cps {
            self.step(cp, out, sub, errors);
        }
    }

    fn finish(&mut self, out: &mut Vec<u8>, sub: &Subst, errors: &mut usize) {
        self.drain(out, sub, errors);
    }
}
