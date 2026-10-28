//! Shift_JIS and EUC-JP families: SJIS, CP932/SJIS-win, SJIS-mac,
//! SJIS-2004, EUC-JP, eucJP-win, CP51932, EUC-JP-2004.
//!
//! Tables: JIS X 0208/0212 from unicode.org's JIS0208.TXT/JIS0212.TXT, CP932
//! from Microsoft's CP932.TXT, MacJapanese from Apple's JAPANESE.TXT, JIS X
//! 0213:2004 from Project X0213's table; deviations are listed in
//! tools/charset/gen_tables.py.

use super::{Decode, ERR, Encode, Subst};
use crate::Encoding;
use crate::tables::{jis as t, rev, seq};

/// Kuten index (0-based row * 94 + cell) of a Shift_JIS double-byte code.
/// The trail byte must be in 0x40..=0x7E or 0x80..=0xFC.
#[inline]
pub(crate) fn sjis_to_index(lead: u8, trail: u8) -> usize {
    let base = if lead <= 0x9F {
        (lead - 0x81) as usize * 2
    } else {
        (lead - 0xC1) as usize * 2
    };
    if trail >= 0x9F {
        (base + 1) * 94 + (trail - 0x9F) as usize
    } else {
        base * 94 + (trail - 0x40 - (trail >= 0x80) as u8) as usize
    }
}

/// The Shift_JIS bytes of a kuten index (rows 1..=120).
#[inline]
pub(crate) fn index_to_sjis(idx: usize) -> (u8, u8) {
    let row = idx / 94;
    let cell = (idx % 94) as u8;
    let lead = if row < 62 {
        0x81 + (row / 2) as u8
    } else {
        0xC1 + (row / 2) as u8
    };
    let trail = if row % 2 == 1 {
        cell + 0x9F
    } else if cell + 0x40 >= 0x7F {
        cell + 0x41
    } else {
        cell + 0x40
    };
    (lead, trail)
}

#[inline]
fn valid_trail(b: u8) -> bool {
    (0x40..=0x7E).contains(&b) || (0x80..=0xFC).contains(&b)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SjisKind {
    Sjis,
    Cp932,
    Mac,
    X0213,
}

fn sjis_kind(enc: Encoding) -> SjisKind {
    match enc {
        Encoding::Sjis => SjisKind::Sjis,
        Encoding::SjisMac => SjisKind::Mac,
        Encoding::Sjis2004 => SjisKind::X0213,
        _ => SjisKind::Cp932,
    }
}

/// Shift_JIS-2004 lead bytes 0xF0..=0xFC address JIS X 0213 plane 2 rows
/// 1, 8, 3, 4, 5, 12, 13, 14, 15, 78..=94 (JIS X 0213:2004 Annex 1).
fn x0213_plane2_index(lead: u8, trail: u8) -> usize {
    const ROWS: [usize; 10] = [1, 8, 3, 4, 5, 12, 13, 14, 15, 78];
    let second = trail >= 0x9F;
    let cell = if second {
        (trail - 0x9F) as usize
    } else {
        (trail - 0x40 - (trail >= 0x80) as u8) as usize
    };
    let row = if lead <= 0xF4 {
        ROWS[(lead - 0xF0) as usize * 2 + second as usize]
    } else {
        79 + (lead - 0xF5) as usize * 2 + second as usize
    };
    94 * 94 + (row - 1) * 94 + cell
}

/// The Shift_JIS-2004 bytes of a JIS X 0213 kuten index (planes 1 and 2).
fn x0213_to_sjis(idx: usize) -> Option<(u8, u8)> {
    if idx < 94 * 94 {
        return Some(index_to_sjis(idx));
    }
    let i = idx - 94 * 94;
    let row = i / 94 + 1;
    let cell = (i % 94) as u8;
    let (lead, second) = match row {
        1 => (0xF0, false),
        8 => (0xF0, true),
        3 => (0xF1, false),
        4 => (0xF1, true),
        5 => (0xF2, false),
        12 => (0xF2, true),
        13 => (0xF3, false),
        14 => (0xF3, true),
        15 => (0xF4, false),
        78 => (0xF4, true),
        79..=94 => (0xF5 + ((row - 79) / 2) as u8, (row - 79) % 2 == 1),
        _ => return None,
    };
    let trail = if second {
        cell + 0x9F
    } else if cell + 0x40 >= 0x7F {
        cell + 0x41
    } else {
        cell + 0x40
    };
    Some((lead, trail))
}

fn push_x0213(idx: usize, out: &mut Vec<u32>) {
    let v = t::X0213[idx];
    if v == 0 {
        out.push(ERR);
    } else {
        out.extend_from_slice(seq(&v, &t::X0213_SEQ, &t::X0213_SEQ_OFF));
    }
}

pub(crate) struct SjisDec {
    kind: SjisKind,
}

impl SjisDec {
    pub(crate) fn new(enc: Encoding) -> Self {
        SjisDec {
            kind: sjis_kind(enc),
        }
    }
}

impl Decode for SjisDec {
    fn decode(&mut self, s: &[u8], pos: &mut usize, out: &mut Vec<u32>, limit: usize) {
        let mut i = *pos;
        let n = s.len();
        let kind = self.kind;
        while i < n && out.len() < limit {
            let b = s[i];
            if kind == SjisKind::Mac {
                if b < 0x80 {
                    // Apple's table: 0x5C is YEN SIGN.
                    out.push(if b == 0x5C { 0xA5 } else { b as u32 });
                    i += 1;
                    continue;
                }
                let v = t::SJISMAC_SINGLE[b as usize];
                if v != 0 {
                    out.extend_from_slice(seq(&v, &t::SJISMAC_SEQ, &t::SJISMAC_SEQ_OFF));
                    i += 1;
                    continue;
                }
                if (0xA1..=0xDF).contains(&b) {
                    out.push(0xFF61 + (b - 0xA1) as u32);
                    i += 1;
                    continue;
                }
            } else if b < 0x80 {
                // SJIS-2004's single bytes are JIS X 0201 Roman: 0x5C is YEN
                // SIGN and 0x7E OVERLINE (observed on PHP 8.5).
                out.push(match (kind, b) {
                    (SjisKind::X0213, 0x5C) => 0xA5,
                    (SjisKind::X0213, 0x7E) => 0x203E,
                    _ => b as u32,
                });
                i += 1;
                continue;
            } else if (0xA1..=0xDF).contains(&b) {
                out.push(0xFF61 + (b - 0xA1) as u32);
                i += 1;
                continue;
            }
            let lead_ok = match kind {
                SjisKind::Sjis => (0x81..=0x9F).contains(&b) || (0xE0..=0xEF).contains(&b),
                SjisKind::Mac => (0x81..=0x9F).contains(&b) || (0xE0..=0xED).contains(&b),
                _ => (0x81..=0x9F).contains(&b) || (0xE0..=0xFC).contains(&b),
            };
            if !lead_ok {
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
            if !valid_trail(c) {
                out.push(ERR);
                continue;
            }
            match kind {
                SjisKind::Sjis => {
                    let cp = t::JIS0208[sjis_to_index(b, c)];
                    out.push(if cp == 0 { ERR } else { cp as u32 });
                }
                SjisKind::Cp932 => {
                    let cp = t::CP932[sjis_to_index(b, c)];
                    out.push(if cp == 0 { ERR } else { cp as u32 });
                }
                SjisKind::Mac => {
                    let v = t::SJISMAC[sjis_to_index(b, c)];
                    if v == 0 {
                        out.push(ERR);
                    } else {
                        out.extend_from_slice(seq(&v, &t::SJISMAC_SEQ, &t::SJISMAC_SEQ_OFF));
                    }
                }
                SjisKind::X0213 => {
                    let idx = if b >= 0xF0 {
                        x0213_plane2_index(b, c)
                    } else {
                        sjis_to_index(b, c)
                    };
                    push_x0213(idx, out);
                }
            }
        }
        *pos = i;
    }
}

/// Encoder state for JIS X 0213 combining sequences: a base character held
/// back until the next code point shows whether they combine.
#[derive(Default)]
pub(crate) struct Combine {
    pending: Option<(u32, u16)>,
}

impl Combine {
    /// Whether `cp` starts a JIS X 0213 combining sequence.
    fn is_base(cp: u32) -> bool {
        t::X0213_COMBOS
            .binary_search_by_key(&cp, |e| e.0)
            .is_ok()
    }

    fn combo(a: u32, b: u32) -> Option<u16> {
        t::X0213_COMBOS
            .iter()
            .find(|e| e.0 == a && e.1 == b)
            .map(|e| e.2)
    }
}

/// Looks a code point up in JIS X 0213 (kuten index + 1, 0 if none).
fn x0213_rev(cp: u32) -> u16 {
    rev(&t::X0213_REV_IDX, &t::X0213_REV_DATA, cp)
}

pub(crate) struct SjisEnc {
    kind: SjisKind,
    comb: Combine,
    /// SJIS-win: YEN SIGN and OVERLINE take the fullwidth JIS X 0208 codes
    /// rather than 0x5C and 0x7E as in CP932 (observed on PHP 8.5).
    win: bool,
}

impl SjisEnc {
    pub(crate) fn new(enc: Encoding) -> Self {
        SjisEnc {
            kind: sjis_kind(enc),
            win: enc == Encoding::SjisWin,
            comb: Combine::default(),
        }
    }

    fn emit_index(&self, idx: usize, out: &mut Vec<u8>) {
        let (l, t) = if self.kind == SjisKind::X0213 {
            x0213_to_sjis(idx).expect("plane 2 row")
        } else {
            index_to_sjis(idx)
        };
        out.extend_from_slice(&[l, t]);
    }

    fn flush(&mut self, out: &mut Vec<u8>) {
        if let Some((_, idx)) = self.comb.pending.take() {
            self.emit_index(idx as usize - 1, out);
        }
    }

    fn lookup(&self, cp: u32) -> Option<Code> {
        if cp < 0x80 {
            return Some(Code::Byte(cp as u8));
        }
        if (0xFF61..=0xFF9F).contains(&cp) {
            return Some(Code::Byte((cp - 0xFF61) as u8 + 0xA1));
        }
        if self.kind == SjisKind::X0213 {
            // JIS X 0201 Roman (observed on PHP 8.5).
            match cp {
                0xA5 => return Some(Code::Byte(0x5C)),
                0x203E => return Some(Code::Byte(0x7E)),
                _ => {}
            }
        }
        if cp > 0xFFFF && self.kind != SjisKind::X0213 {
            return None;
        }
        let idx = match self.kind {
            SjisKind::Sjis => {
                // test ext/mbstring/tests/sjis_encoding.phpt
                let special = match cp {
                    0x203E | 0xAF => 0x8150,
                    0xA5 => 0x818F,
                    0xFFE2 => 0x81CA,
                    0xFFE1 => 0x8192,
                    0xFFE0 => 0x8191,
                    0xFF5E => 0x8160,
                    0xFF0D => 0x817C,
                    0x2225 => 0x8161,
                    _ => 0,
                };
                if special != 0 {
                    return Some(Code::Index(sjis_to_index(
                        (special >> 8) as u8,
                        special as u8,
                    )));
                }
                let i = rev(&t::JIS0208_REV_IDX, &t::JIS0208_REV_DATA, cp);
                if i == 0 || i as usize > 84 * 94 {
                    return None;
                }
                i
            }
            SjisKind::Cp932 => {
                let i = rev(&t::CP932_REV_IDX, &t::CP932_REV_DATA, cp);
                if i != 0 {
                    i
                } else {
                    match ms_compat(cp, self.win) {
                        Compat::Byte(b) => return Some(Code::Byte(b)),
                        Compat::Cp(c) => rev(&t::CP932_REV_IDX, &t::CP932_REV_DATA, c),
                        Compat::None => 0,
                    }
                }
            }
            SjisKind::Mac => return None,
            SjisKind::X0213 => x0213_rev(cp),
        };
        if idx == 0 {
            None
        } else {
            Some(Code::Index(idx as usize - 1))
        }
    }
}

enum Code {
    Byte(u8),
    Index(usize),
}

enum Compat {
    Byte(u8),
    Cp(u32),
    None,
}

/// What the Microsoft-flavoured encoders (CP932, SJIS-win, eucJP-win,
/// CP51932) write for the JIS X 0208 characters whose Unicode mapping
/// differs between the JIS and Microsoft tables (observed on PHP 8.5).
/// `cp51932` selects CP51932's choices for YEN SIGN and OVERLINE.
fn ms_compat(cp: u32, cp51932: bool) -> Compat {
    Compat::Cp(match cp {
        0xA2 => 0xFFE0,
        0xA3 => 0xFFE1,
        0xAC => 0xFFE2,
        0xAF => 0xFFE3,
        0x2016 => 0x2225,
        0x2212 => 0xFF0D,
        0x301C => 0xFF5E,
        0xA5 if cp51932 => 0xFFE5,
        0x203E if cp51932 => 0xFFE3,
        0xA5 => return Compat::Byte(0x5C),
        0x203E => return Compat::Byte(0x7E),
        _ => return Compat::None,
    })
}

/// The JIS X 0208 code points EUC-JP writes for the Microsoft-table ones
/// (observed on PHP 8.5).
fn jis_compat(cp: u32) -> u32 {
    match cp {
        0xFF5E => 0x301C,
        0x2225 => 0x2016,
        0xFF0D => 0x2212,
        0xFFE0 => 0xA2,
        0xFFE1 => 0xA3,
        0xFFE2 => 0xAC,
        0x203E => 0xFFE3,
        _ => cp,
    }
}

impl Encode for SjisEnc {
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        if self.kind == SjisKind::X0213 {
            if let Some((base, _)) = self.comb.pending
                && let Some(i) = Combine::combo(base, cp)
            {
                self.comb.pending = None;
                self.emit_index(i as usize - 1, out);
                return true;
            }
            let code = self.lookup(cp);
            let Some(code) = code else { return false };
            self.flush(out);
            match code {
                Code::Byte(b) => out.push(b),
                Code::Index(i) => {
                    if Combine::is_base(cp) {
                        self.comb.pending = Some((cp, i as u16 + 1));
                    } else {
                        self.emit_index(i, out);
                    }
                }
            }
            return true;
        }
        match self.lookup(cp) {
            Some(Code::Byte(b)) => out.push(b),
            Some(Code::Index(i)) => self.emit_index(i, out),
            None => return false,
        }
        true
    }

    fn finish(&mut self, out: &mut Vec<u8>, _sub: &Subst, _errors: &mut usize) {
        self.flush(out);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EucKind {
    EucJp,
    Win,
    Cp51932,
    X0213,
}

fn euc_kind(enc: Encoding) -> EucKind {
    match enc {
        Encoding::EucJp => EucKind::EucJp,
        Encoding::EucJpWin => EucKind::Win,
        Encoding::Cp51932 => EucKind::Cp51932,
        _ => EucKind::X0213,
    }
}

pub(crate) struct EucJpDec {
    kind: EucKind,
}

impl EucJpDec {
    pub(crate) fn new(enc: Encoding) -> Self {
        EucJpDec {
            kind: euc_kind(enc),
        }
    }
}

#[inline]
fn gr(b: u8) -> bool {
    (0xA1..=0xFE).contains(&b)
}

impl Decode for EucJpDec {
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
            if gr(b) {
                if i + 1 >= n {
                    out.push(ERR);
                    i += 1;
                    continue;
                }
                let c = s[i + 1];
                i += 2;
                if !gr(c) {
                    out.push(ERR);
                    continue;
                }
                let idx = (b - 0xA1) as usize * 94 + (c - 0xA1) as usize;
                let cp = match kind {
                    EucKind::EucJp => t::JIS0208[idx],
                    EucKind::Win => t::EUCJPWIN[idx],
                    EucKind::Cp51932 => t::CP932[idx],
                    EucKind::X0213 => {
                        push_x0213(idx, out);
                        continue;
                    }
                };
                out.push(if cp == 0 { ERR } else { cp as u32 });
                continue;
            }
            if b == 0x8E {
                if i + 1 >= n {
                    out.push(ERR);
                    i += 1;
                    continue;
                }
                let c = s[i + 1];
                i += 2;
                out.push(if (0xA1..=0xDF).contains(&c) {
                    0xFF61 + (c - 0xA1) as u32
                } else {
                    ERR
                });
                continue;
            }
            if b == 0x8F && kind == EucKind::X0213 {
                // Only the JIS X 0213 plane 2 rows take a third byte; 0x8F
                // and another second byte make one error (observed on PHP
                // 8.5).
                if i + 1 >= n {
                    out.push(ERR);
                    i += 1;
                    continue;
                }
                let c = s[i + 1];
                let row = c.wrapping_sub(0xA0) as usize;
                if !matches!(row, 1 | 3 | 4 | 5 | 8 | 12..=15 | 78..=94) || i + 2 >= n {
                    out.push(ERR);
                    i += 2;
                    continue;
                }
                let d = s[i + 2];
                i += 3;
                if gr(d) {
                    push_x0213(94 * 94 + (row - 1) * 94 + (d - 0xA1) as usize, out);
                } else {
                    out.push(ERR);
                }
                continue;
            }
            if b == 0x8F && kind != EucKind::Cp51932 {
                if i + 2 >= n {
                    out.push(ERR);
                    i = n;
                    continue;
                }
                let (c, d) = (s[i + 1], s[i + 2]);
                i += 3;
                if !gr(c) || !gr(d) {
                    out.push(ERR);
                    continue;
                }
                let idx = (c - 0xA1) as usize * 94 + (d - 0xA1) as usize;
                let cp = match kind {
                    EucKind::EucJp => t::JIS0212[idx],
                    EucKind::Win => t::EUCJPWIN_8F[idx],
                    _ => {
                        push_x0213(94 * 94 + idx, out);
                        continue;
                    }
                };
                out.push(if cp == 0 { ERR } else { cp as u32 });
                continue;
            }
            out.push(ERR);
            i += 1;
        }
        *pos = i;
    }
}

pub(crate) struct EucJpEnc {
    kind: EucKind,
    comb: Combine,
}

impl EucJpEnc {
    pub(crate) fn new(enc: Encoding) -> Self {
        EucJpEnc {
            kind: euc_kind(enc),
            comb: Combine::default(),
        }
    }

    fn emit_index(idx: usize, out: &mut Vec<u8>) {
        if idx >= 94 * 94 {
            let i = idx - 94 * 94;
            out.extend_from_slice(&[0x8F, (i / 94) as u8 + 0xA1, (i % 94) as u8 + 0xA1]);
        } else {
            out.extend_from_slice(&[(idx / 94) as u8 + 0xA1, (idx % 94) as u8 + 0xA1]);
        }
    }

    fn flush(&mut self, out: &mut Vec<u8>) {
        if let Some((_, idx)) = self.comb.pending.take() {
            Self::emit_index(idx as usize - 1, out);
        }
    }

    /// The kuten index (plane 2 / JIS X 0212 offset by 94*94) of `cp`.
    fn lookup(&self, cp: u32) -> Option<Lookup> {
        if cp > 0xFFFF && self.kind != EucKind::X0213 {
            return None;
        }
        let (a, b) = match self.kind {
            EucKind::EucJp => {
                let a = rev(&t::JIS0208_REV_IDX, &t::JIS0208_REV_DATA, cp);
                let b = rev(&t::JIS0212_REV_IDX, &t::JIS0212_REV_DATA, cp);
                if a == 0 && b == 0 {
                    let c = jis_compat(cp);
                    (rev(&t::JIS0208_REV_IDX, &t::JIS0208_REV_DATA, c), 0)
                } else {
                    (a, b)
                }
            }
            EucKind::Win => {
                let look = |c: u32| {
                    (
                        rev(&t::EUCJPWIN_REV_IDX, &t::EUCJPWIN_REV_DATA, c),
                        rev(&t::EUCJPWIN_8F_REV_IDX, &t::EUCJPWIN_8F_REV_DATA, c),
                    )
                };
                let (a, b) = look(cp);
                if a == 0 && b == 0 {
                    match cp {
                        0x2014 => look(0x2015),
                        0xA6 => look(0xFFE4),
                        _ => match ms_compat(cp, false) {
                            Compat::Byte(b) => return Some(Lookup::Byte(b)),
                            Compat::Cp(c) => look(c),
                            Compat::None => (0, 0),
                        },
                    }
                } else {
                    (a, b)
                }
            }
            EucKind::Cp51932 => {
                let look = |c: u32| rev(&t::CP51932_REV_IDX, &t::CP51932_REV_DATA, c);
                let a = look(cp);
                if a != 0 {
                    (a, 0)
                } else {
                    match ms_compat(cp, true) {
                        Compat::Cp(c) => (look(c), 0),
                        _ => (0, 0),
                    }
                }
            }
            EucKind::X0213 => (x0213_rev(cp), 0),
        };
        if a != 0 {
            Some(Lookup::Index(a as usize - 1))
        } else if b != 0 {
            Some(Lookup::Index(94 * 94 + b as usize - 1))
        } else {
            None
        }
    }
}

enum Lookup {
    Byte(u8),
    Index(usize),
}

impl Encode for EucJpEnc {
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        if self.kind == EucKind::X0213
            && let Some((base, _)) = self.comb.pending
            && let Some(i) = Combine::combo(base, cp)
        {
            self.comb.pending = None;
            Self::emit_index(i as usize - 1, out);
            return true;
        }
        if cp < 0x80 {
            self.flush(out);
            out.push(cp as u8);
            return true;
        }
        if (0xFF61..=0xFF9F).contains(&cp) {
            self.flush(out);
            out.extend_from_slice(&[0x8E, (cp - 0xFF61) as u8 + 0xA1]);
            return true;
        }
        if self.kind == EucKind::X0213 && (cp == 0xA5 || cp == 0x203E) {
            // As in SJIS-2004 (observed on PHP 8.5).
            self.flush(out);
            out.push(if cp == 0xA5 { 0x5C } else { 0x7E });
            return true;
        }
        let idx = match self.lookup(cp) {
            None => return false,
            Some(Lookup::Byte(b)) => {
                self.flush(out);
                out.push(b);
                return true;
            }
            Some(Lookup::Index(i)) => i,
        };
        self.flush(out);
        if self.kind == EucKind::X0213 && Combine::is_base(cp) {
            self.comb.pending = Some((cp, idx as u16 + 1));
        } else {
            Self::emit_index(idx, out);
        }
        true
    }

    fn finish(&mut self, out: &mut Vec<u8>, _sub: &Subst, _errors: &mut usize) {
        self.flush(out);
    }
}
