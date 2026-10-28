//! ISO-2022-JP (RFC 1468) and ISO-2022-KR (RFC 1557), on top of the EUC-JP
//! and EUC-KR tables (a JIS X 0208 / KS X 1001 pair is the EUC pair with the
//! high bits cleared). Behaviour observed on PHP 8.5 with glibc 2.43:
//!
//! ISO-2022-JP
//! - decodes ESC ( B (ASCII), ESC ( J (JIS X 0201 Roman), ESC $ @ and ESC $ B
//!   (JIS X 0208); any other escape is passed through as characters;
//! - in two-byte mode, bytes below 0x21 and 0x7F are single characters, and
//!   an unmapped pair is illegal (//IGNORE skips both bytes);
//! - encodes ASCII as such, U+00A5 and U+203E with ESC ( J (staying in it for
//!   the ASCII characters it shares), JIS X 0208 with ESC $ B, and returns to
//!   ASCII at the end.
//!
//! ISO-2022-KR
//! - the encoder always starts with the ESC $ ) C designation, even for empty
//!   input; SO/SI switch between ASCII and KS X 1001, and SI is written
//!   before every ASCII character (newlines included) and at the end;
//! - the decoder skips designations, and in SO mode reads every byte below
//!   0x80 (SO/SI apart) as the first of a pair.

use super::convert::DecodeStatus;
use super::table::{Reverse, Table};

const ESC: u8 = 0x1B;

#[derive(Clone, Copy, PartialEq, Eq)]
enum JpSet {
    Ascii,
    Roman,
    Kanji,
}

/// Decodes one EUC two-byte sequence through `table`.
fn euc_pair(table: &'static Table, l: u8, t: u8) -> Option<u32> {
    table.decode_pair(l | 0x80, t | 0x80)
}

pub(crate) struct Iso2022JpDecoder {
    table: &'static Table,
    set: JpSet,
}

impl Iso2022JpDecoder {
    pub(crate) fn new(table: &'static Table) -> Self {
        Iso2022JpDecoder {
            table,
            set: JpSet::Ascii,
        }
    }

    pub(crate) fn decode(
        &mut self,
        input: &[u8],
        pos: &mut usize,
        out: &mut Vec<u32>,
        limit: usize,
    ) -> DecodeStatus {
        while *pos < input.len() && out.len() < limit {
            let s = &input[*pos..];
            let b = s[0];
            if b == ESC {
                // An escape needs three bytes before it is decided, whatever
                // they are (observed).
                if s.len() < 3 {
                    return DecodeStatus::Incomplete;
                }
                let set = match s.get(1..3) {
                    Some(b"(B") => Some(JpSet::Ascii),
                    Some(b"(J") => Some(JpSet::Roman),
                    Some(b"$@") | Some(b"$B") => Some(JpSet::Kanji),
                    _ => None,
                };
                if let Some(set) = set {
                    self.set = set;
                    *pos += 3;
                    continue;
                }
                out.push(u32::from(ESC));
                *pos += 1;
                continue;
            }
            if b >= 0x80 {
                return DecodeStatus::Illegal(1);
            }
            match self.set {
                JpSet::Ascii => out.push(u32::from(b)),
                JpSet::Roman => out.push(match b {
                    0x5C => 0xA5,
                    0x7E => 0x203E,
                    _ => u32::from(b),
                }),
                JpSet::Kanji => {
                    if !(0x21..=0x7E).contains(&b) {
                        out.push(u32::from(b));
                    } else {
                        let Some(&t) = s.get(1) else {
                            return DecodeStatus::Incomplete;
                        };
                        match euc_pair(self.table, b, t).filter(|_| (0x21..=0x7E).contains(&t)) {
                            Some(cp) => out.push(cp),
                            None => return DecodeStatus::Illegal(2),
                        }
                        *pos += 2;
                        continue;
                    }
                }
            }
            *pos += 1;
        }
        DecodeStatus::Ok
    }
}

/// The JIS X 0208 / KS X 1001 pair of a code point, from the EUC table;
/// only round-trip mappings count (EUC-KR's one-way U+20A9 doesn't).
fn gl_pair(table: &Table, rev: &Reverse, cp: u32) -> Option<[u8; 2]> {
    let v = rev.get(cp)?;
    let (l, t) = ((v >> 8) as u8, v as u8);
    ((0x100..0x1_0000).contains(&v)
        && (0xA1..=0xFE).contains(&l)
        && (0xA1..=0xFE).contains(&t)
        && table.decode_pair(l, t) == Some(cp))
    .then_some([l & 0x7F, t & 0x7F])
}

pub(crate) struct Iso2022JpEncoder {
    table: &'static Table,
    rev: &'static Reverse,
    set: JpSet,
}

impl Iso2022JpEncoder {
    pub(crate) fn new(table: &'static Table) -> Self {
        Iso2022JpEncoder {
            table,
            rev: table.reverse(),
            set: JpSet::Ascii,
        }
    }

    pub(crate) fn can_encode(&self, cp: u32) -> bool {
        cp < 0x80 || cp == 0xA5 || cp == 0x203E || gl_pair(self.table, self.rev, cp).is_some()
    }

    fn switch(&mut self, set: JpSet, out: &mut Vec<u8>) {
        if self.set != set {
            out.extend_from_slice(match set {
                JpSet::Ascii => b"\x1b(B",
                JpSet::Roman => b"\x1b(J",
                JpSet::Kanji => b"\x1b$B",
            });
            self.set = set;
        }
    }

    pub(crate) fn encode(&mut self, cps: &[u32], out: &mut Vec<u8>) -> Result<(), usize> {
        for (i, &cp) in cps.iter().enumerate() {
            if cp < 0x80 {
                if self.set == JpSet::Kanji
                    || (self.set == JpSet::Roman && (cp == 0x5C || cp == 0x7E))
                {
                    self.switch(JpSet::Ascii, out);
                }
                out.push(cp as u8);
            } else if cp == 0xA5 || cp == 0x203E {
                self.switch(JpSet::Roman, out);
                out.push(if cp == 0xA5 { 0x5C } else { 0x7E });
            } else if let Some(p) = gl_pair(self.table, self.rev, cp) {
                self.switch(JpSet::Kanji, out);
                out.extend_from_slice(&p);
            } else {
                return Err(i);
            }
        }
        Ok(())
    }

    pub(crate) fn finish(&mut self, out: &mut Vec<u8>) {
        self.switch(JpSet::Ascii, out);
    }
}

pub(crate) struct Iso2022KrDecoder {
    table: &'static Table,
    so: bool,
}

impl Iso2022KrDecoder {
    pub(crate) fn new(table: &'static Table) -> Self {
        Iso2022KrDecoder { table, so: false }
    }

    pub(crate) fn decode(
        &mut self,
        input: &[u8],
        pos: &mut usize,
        out: &mut Vec<u32>,
        limit: usize,
    ) -> DecodeStatus {
        while *pos < input.len() && out.len() < limit {
            let s = &input[*pos..];
            let b = s[0];
            match b {
                ESC => {
                    // The designation is skipped; any other escape is a
                    // character (observed).
                    if s.len() < 4 && b"\x1b$)C".starts_with(s) {
                        return DecodeStatus::Incomplete;
                    }
                    if s.starts_with(b"\x1b$)C") {
                        *pos += 4;
                        continue;
                    }
                    out.push(u32::from(ESC));
                    *pos += 1;
                }
                0x0E => {
                    self.so = true;
                    *pos += 1;
                }
                0x0F => {
                    self.so = false;
                    *pos += 1;
                }
                0x80.. => return DecodeStatus::Illegal(1),
                _ if self.so => {
                    let Some(&t) = s.get(1) else {
                        return DecodeStatus::Incomplete;
                    };
                    let cp = if (0x21..=0x7E).contains(&b) && (0x21..=0x7E).contains(&t) {
                        euc_pair(self.table, b, t)
                    } else {
                        None
                    };
                    match cp {
                        Some(cp) => out.push(cp),
                        None => return DecodeStatus::Illegal(2),
                    }
                    *pos += 2;
                }
                _ => {
                    out.push(u32::from(b));
                    *pos += 1;
                }
            }
        }
        DecodeStatus::Ok
    }
}

pub(crate) struct Iso2022KrEncoder {
    table: &'static Table,
    rev: &'static Reverse,
    so: bool,
    header: bool,
}

impl Iso2022KrEncoder {
    pub(crate) fn new(table: &'static Table) -> Self {
        Iso2022KrEncoder {
            table,
            rev: table.reverse(),
            so: false,
            header: false,
        }
    }

    pub(crate) fn can_encode(&self, cp: u32) -> bool {
        cp < 0x80 || gl_pair(self.table, self.rev, cp).is_some()
    }

    pub(crate) fn start(&mut self, out: &mut Vec<u8>) {
        if !self.header {
            out.extend_from_slice(b"\x1b$)C");
            self.header = true;
        }
    }

    pub(crate) fn encode(&mut self, cps: &[u32], out: &mut Vec<u8>) -> Result<(), usize> {
        self.start(out);
        for (i, &cp) in cps.iter().enumerate() {
            if cp < 0x80 {
                if self.so {
                    out.push(0x0F);
                    self.so = false;
                }
                out.push(cp as u8);
            } else if let Some(p) = gl_pair(self.table, self.rev, cp) {
                if !self.so {
                    out.push(0x0E);
                    self.so = true;
                }
                out.extend_from_slice(&p);
            } else {
                return Err(i);
            }
        }
        Ok(())
    }

    pub(crate) fn finish(&mut self, out: &mut Vec<u8>) {
        self.start(out);
        if self.so {
            out.push(0x0F);
            self.so = false;
        }
    }
}
