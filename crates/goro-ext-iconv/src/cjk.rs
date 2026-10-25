//! The code tables of glibc's JOHAB, BIG5-HKSCS, JIS X 0213 (EUC-JISX0213,
//! SHIFT_JISX0213, ISO-2022-JP-3), CNS 11643 planes 3-7 (EUC-TW,
//! ISO-2022-CN-EXT) and ISO-IR-165, generated from glibc's conversions by
//! tools/gen_iconv_cjk (see gen.py there for the blob's format). They are
//! inflated and indexed on first use.

use crate::charset::Dec;
use std::collections::HashMap;
use std::sync::OnceLock;

static BLOB: &[u8] = include_bytes!("cjk_tables.bin");

/// A grid value standing for two code points (an index into the pairs).
pub const PAIR: u32 = 0x200000;
/// A single-byte grid value: a lead byte (incomplete alone).
pub const LEAD: u32 = 0x1F_FFFF;

/// Decoded values of a lead x trail byte range (0 = undefined).
pub struct Grid {
    llo: u8,
    lhi: u8,
    tlo: u8,
    thi: u8,
    cells: Vec<u32>,
}

impl Grid {
    pub fn get(&self, l: u8, t: u8) -> u32 {
        if l < self.llo || l > self.lhi || t < self.tlo || t > self.thi {
            return 0;
        }
        let w = (self.thi - self.tlo) as usize + 1;
        self.cells[(l - self.llo) as usize * w + (t - self.tlo) as usize]
    }

    /// Every defined cell: (lead, trail, value), in code order.
    fn iter(&self) -> impl Iterator<Item = (u8, u8, u32)> + '_ {
        let w = (self.thi - self.tlo) as usize + 1;
        self.cells
            .iter()
            .enumerate()
            .filter(|(_, v)| **v != 0)
            .map(move |(i, &v)| (self.llo + (i / w) as u8, self.tlo + (i % w) as u8, v))
    }
}

/// A charset's encoder: code point to code (sorted), and the compositions
/// (first, second) -> code.
#[derive(Default)]
pub struct Enc {
    map: Vec<(u32, u32)>,
    comp: Vec<((u32, u32), u32)>,
    /// The code points a composition starts with.
    bases: Vec<u32>,
}

impl Enc {
    pub fn get(&self, cp: u32) -> Option<u32> {
        self.map
            .binary_search_by_key(&cp, |&(c, _)| c)
            .ok()
            .map(|i| self.map[i].1)
    }

    /// Whether `cp` may combine with the character after it.
    pub fn is_base(&self, cp: u32) -> bool {
        self.bases.binary_search(&cp).is_ok()
    }

    pub fn compose(&self, a: u32, b: u32) -> Option<u32> {
        self.comp
            .binary_search_by_key(&(a, b), |&(k, _)| k)
            .ok()
            .map(|i| self.comp[i].1)
    }
}

pub struct Tables {
    pub johab1: Grid,
    pub johab: Grid,
    johab_skip2: Vec<u8>,
    pub johab_enc: Enc,
    pub hkscs1: Grid,
    pub hkscs: Grid,
    pub hkscs_pairs: Vec<(u32, u32)>,
    pub hkscs_enc: Enc,
    /// JIS X 0213 planes 1 and 2 (rows and columns 0x21-0x7E).
    pub jis1: Grid,
    pub jis2: Grid,
    pub jis_pairs: Vec<(u32, u32)>,
    /// Codes: a byte (ASCII), 0x8Exx (half-width katakana), a plane 1
    /// row/column, 0x10000 | a plane 2 row/column.
    pub jis_enc: Enc,
    /// CNS 11643 planes 3 to 7 and 15.
    pub cns: [Grid; 6],
    /// Codes: plane << 16 | row/column.
    pub cns_enc: Enc,
    pub ir165: Grid,
    pub ir165_enc: Enc,
}

impl Tables {
    /// Whether //IGNORE skips both bytes of an illegal JOHAB pair.
    pub fn johab_skip2(&self, l: u8, t: u8) -> bool {
        let k = (l as usize) << 8 | t as usize;
        self.johab_skip2
            .get(k >> 3)
            .is_some_and(|b| b & (1 << (k & 7)) != 0)
    }
}

pub fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(load)
}

enum Rec {
    Grid(Grid),
    Enc(Vec<(u32, u32)>),
    Pairs(Vec<(u32, u32)>),
    Bits(Vec<u8>),
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> u8 {
        self.p += 1;
        self.b[self.p - 1]
    }

    fn varint(&mut self) -> u32 {
        let mut v = 0u32;
        let mut shift = 0;
        loop {
            let b = self.byte();
            v |= ((b & 0x7F) as u32) << shift;
            if b & 0x80 == 0 {
                return v;
            }
            shift += 7;
        }
    }
}

fn inflate() -> Vec<u8> {
    use goro_zlib::{Inflate, Io, Z_FINISH};
    let len = u32::from_le_bytes([BLOB[0], BLOB[1], BLOB[2], BLOB[3]]) as usize;
    let mut out = vec![0u8; len];
    let mut inf = Inflate::new(15).expect("inflate");
    let mut io = Io::new(&BLOB[4..], &mut out);
    inf.inflate(&mut io, Z_FINISH);
    out
}

fn load() -> Tables {
    let raw = inflate();
    let mut r = Reader { b: &raw, p: 0 };
    let mut recs: HashMap<String, Rec> = HashMap::new();
    while r.p < raw.len() {
        let n = r.byte() as usize;
        let name = String::from_utf8_lossy(&raw[r.p..r.p + n]).into_owned();
        r.p += n;
        let rec = match r.byte() {
            b'G' => {
                let (llo, lhi, tlo, thi) = (r.byte(), r.byte(), r.byte(), r.byte());
                let count = (lhi - llo) as usize + 1;
                let count = count * ((thi - tlo) as usize + 1);
                let mut cells = Vec::with_capacity(count);
                let mut prev = 0i64;
                for _ in 0..count {
                    let t = r.varint();
                    if t == 0 {
                        cells.push(0);
                        continue;
                    }
                    let z = (t - 1) as i64;
                    prev += if z & 1 == 0 { z >> 1 } else { -((z + 1) >> 1) };
                    cells.push(prev as u32);
                }
                Rec::Grid(Grid {
                    llo,
                    lhi,
                    tlo,
                    thi,
                    cells,
                })
            }
            b'E' => {
                let n = r.varint();
                let mut cp = 0;
                let mut v = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    cp += r.varint();
                    v.push((cp, r.varint()));
                }
                Rec::Enc(v)
            }
            b'P' => {
                let n = r.varint();
                Rec::Pairs((0..n).map(|_| (r.varint(), r.varint())).collect())
            }
            b'B' => {
                let n = r.varint() as usize;
                r.p += n;
                Rec::Bits(raw[r.p - n..r.p].to_vec())
            }
            t => panic!("cjk_tables: record type {t}"),
        };
        recs.insert(name, rec);
    }
    let mut grid = |n: &str| match recs.remove(n) {
        Some(Rec::Grid(g)) => g,
        _ => panic!("cjk_tables: {n}"),
    };
    let johab1 = grid("JOHAB1");
    let johab = grid("JOHAB");
    let hkscs1 = grid("HKSCS1");
    let hkscs = grid("HKSCS");
    let jis1 = grid("JISX0213_1");
    let jis2 = grid("JISX0213_2");
    let cns = [
        grid("CNS3"),
        grid("CNS4"),
        grid("CNS5"),
        grid("CNS6"),
        grid("CNS7"),
        grid("CNS15"),
    ];
    let ir165 = grid("IR165");
    let mut list = |n: &str| match recs.remove(n) {
        Some(Rec::Enc(v) | Rec::Pairs(v)) => v,
        _ => panic!("cjk_tables: {n}"),
    };
    let johab_x = list("JOHAB_E");
    let hkscs_pairs = list("HKSCS_P");
    let hkscs_x = list("HKSCS_E");
    let jis_pairs = list("JISX0213_P");
    let jis_x = list("JISX0213_E");
    let cns_x = list("CNS_E");
    let johab_skip2 = match recs.remove("JOHAB_SKIP2") {
        Some(Rec::Bits(b)) => b,
        _ => panic!("cjk_tables: JOHAB_SKIP2"),
    };
    let byte = |l: u8, t: u8| (l as u32) << 8 | t as u32;
    let johab_enc = enc(
        &[(&johab1, &|_, t| t as u32), (&johab, &byte)],
        &johab_x,
        &[],
    );
    let hkscs_enc = enc(
        &[(&hkscs1, &|_, t| t as u32), (&hkscs, &byte)],
        &hkscs_x,
        &hkscs_pairs,
    );
    let jis_enc = enc(
        &[(&jis1, &byte), (&jis2, &|l, t| 0x10000 | byte(l, t))],
        &jis_x,
        &jis_pairs,
    );
    let cns_enc = {
        let codes: [&dyn Fn(u8, u8) -> u32; 6] = [
            &|l, t| 3 << 16 | byte(l, t),
            &|l, t| 4 << 16 | byte(l, t),
            &|l, t| 5 << 16 | byte(l, t),
            &|l, t| 6 << 16 | byte(l, t),
            &|l, t| 7 << 16 | byte(l, t),
            &|l, t| 15 << 16 | byte(l, t),
        ];
        let grids: Vec<(&Grid, &dyn Fn(u8, u8) -> u32)> = cns.iter().zip(codes).collect();
        enc(&grids, &cns_x, &[])
    };
    let ir165_enc = enc(&[(&ir165, &byte)], &[], &[]);
    Tables {
        johab1,
        johab,
        johab_skip2,
        johab_enc,
        hkscs1,
        hkscs,
        hkscs_pairs,
        hkscs_enc,
        jis1,
        jis2,
        jis_pairs,
        jis_enc,
        cns,
        cns_enc,
        ir165,
        ir165_enc,
    }
}

type CodeFn<'a> = &'a dyn Fn(u8, u8) -> u32;

/// The encoder of decode grids (the first code of a character wins, as in
/// gen.py), with the generated exceptions applied (code 0: none).
fn enc(grids: &[(&Grid, CodeFn)], exceptions: &[(u32, u32)], pairs: &[(u32, u32)]) -> Enc {
    let mut m: HashMap<u32, u32> = HashMap::new();
    let mut comp = Vec::new();
    for (g, code) in grids {
        for (l, t, v) in g.iter() {
            if v >= PAIR {
                comp.push((pairs[(v - PAIR) as usize], code(l, t)));
                continue;
            }
            let c = code(l, t);
            if v == LEAD || (v < 0x80 && c == v) {
                continue;
            }
            m.entry(v).or_insert(c);
        }
    }
    for &(cp, c) in exceptions {
        m.insert(cp, c);
    }
    let mut map: Vec<(u32, u32)> = m.into_iter().collect();
    map.sort_unstable();
    comp.sort_unstable();
    let mut bases: Vec<u32> = comp.iter().map(|&((a, _), _)| a).collect();
    bases.dedup();
    Enc { map, comp, bases }
}

/// A decoded grid value: one or two code points.
fn chars(v: u32, pairs: &[(u32, u32)]) -> (u32, u32) {
    if v >= PAIR {
        pairs[(v - PAIR) as usize]
    } else {
        (v, 0)
    }
}

/// A decoder's result and the second character of a pair (0: none).
pub type Decoded = (Dec, u32);

/// A single-byte grid lookup (byte 0x00 is U+0000, which a grid cannot
/// hold).
fn single(g: &Grid, b: u8) -> u32 {
    if b == 0 { 0x80000000 } else { g.get(0, b) }
}

fn dbcs(
    sb: &Grid,
    g: &Grid,
    pairs: &[(u32, u32)],
    s: &[u8],
    skip2: impl Fn(u8, u8) -> bool,
) -> Decoded {
    match single(sb, s[0]) {
        0 => (Dec::Illegal(1), 0),
        0x80000000 => (Dec::Char(0, 1), 0),
        LEAD => {
            if s.len() < 2 {
                return (Dec::Incomplete, 0);
            }
            match g.get(s[0], s[1]) {
                0 => (Dec::Illegal(if skip2(s[0], s[1]) { 2 } else { 1 }), 0),
                v => {
                    let (a, b) = chars(v, pairs);
                    (Dec::Char(a, 2), b)
                }
            }
        }
        v => (Dec::Char(v, 1), 0),
    }
}

pub fn decode_johab(s: &[u8]) -> Decoded {
    let t = tables();
    dbcs(&t.johab1, &t.johab, &[], s, |l, b| t.johab_skip2(l, b))
}

pub fn decode_hkscs(s: &[u8]) -> Decoded {
    let t = tables();
    dbcs(&t.hkscs1, &t.hkscs, &t.hkscs_pairs, s, |_, _| false)
}

/// A JIS X 0213 plane 1 (0x21-0x7E bytes) character.
pub fn jis_plane1(l: u8, c: u8) -> (u32, u32) {
    let t = tables();
    chars(t.jis1.get(l, c), &t.jis_pairs)
}

pub fn decode_euc_jisx0213(s: &[u8]) -> Decoded {
    let t = tables();
    let c = s[0];
    let gr = |b: u8| (0xA1..=0xFE).contains(&b);
    match c {
        0..=0x7F => (Dec::Char(c as u32, 1), 0),
        0x8E => {
            if s.len() < 2 {
                return (Dec::Incomplete, 0);
            }
            if (0xA1..=0xDF).contains(&s[1]) {
                (Dec::Char(0xFF61 + (s[1] - 0xA1) as u32, 2), 0)
            } else {
                (Dec::Illegal(1), 0)
            }
        }
        0x8F => {
            if s.len() < 2 {
                return (Dec::Incomplete, 0);
            }
            if !gr(s[1]) {
                return (Dec::Illegal(1), 0);
            }
            if s.len() < 3 {
                return (Dec::Incomplete, 0);
            }
            match if gr(s[2]) {
                t.jis2.get(s[1] - 0x80, s[2] - 0x80)
            } else {
                0
            } {
                0 => (Dec::Illegal(1), 0),
                v => (Dec::Char(v, 3), 0),
            }
        }
        0xA1..=0xFE => {
            if s.len() < 2 {
                return (Dec::Incomplete, 0);
            }
            match if gr(s[1]) {
                jis_plane1(c - 0x80, s[1] - 0x80)
            } else {
                (0, 0)
            } {
                (0, _) => (Dec::Illegal(1), 0),
                (a, b) => (Dec::Char(a, 2), b),
            }
        }
        _ => (Dec::Illegal(1), 0),
    }
}

/// The JIS X 0213 plane (1 or 2), row and column (1-94) of a Shift_JIS
/// pair (JIS X 0213:2004 annex 1).
fn sjis_to_jis(l: u8, t: u8) -> Option<(u8, u8, u8)> {
    let (plane, odd_row, even_row) = match l {
        0x81..=0x9F => (1, (l - 0x81) * 2 + 1, (l - 0x81) * 2 + 2),
        0xE0..=0xEF => (1, (l - 0xC1) * 2 + 1, (l - 0xC1) * 2 + 2),
        0xF0 => (2, 1, 8),
        0xF1 => (2, 3, 4),
        0xF2 => (2, 5, 12),
        0xF3 => (2, 13, 14),
        0xF4 => (2, 15, 78),
        0xF5..=0xFC => (2, (l - 0xCD) * 2 - 1, (l - 0xCD) * 2),
        _ => return None,
    };
    match t {
        0x40..=0x7E => Some((plane, odd_row, t - 0x3F)),
        0x80..=0x9E => Some((plane, odd_row, t - 0x40)),
        0x9F..=0xFC => Some((plane, even_row, t - 0x9E)),
        _ => None,
    }
}

/// The Shift_JIS pair of a JIS X 0213 plane, row and column.
fn jis_to_sjis(plane: u8, row: u8, col: u8) -> (u8, u8) {
    let (lead, odd) = if plane == 1 {
        (
            row.div_ceil(2) + if row <= 62 { 0x80 } else { 0xC0 },
            row % 2 == 1,
        )
    } else if row >= 79 {
        (row.div_ceil(2) + 0xCD, row % 2 == 1)
    } else {
        match row {
            1 => (0xF0, true),
            8 => (0xF0, false),
            3 => (0xF1, true),
            4 => (0xF1, false),
            5 => (0xF2, true),
            12 => (0xF2, false),
            13 => (0xF3, true),
            14 => (0xF3, false),
            15 => (0xF4, true),
            _ => (0xF4, false),
        }
    };
    let trail = if !odd {
        col + 0x9E
    } else if col <= 63 {
        col + 0x3F
    } else {
        col + 0x40
    };
    (lead, trail)
}

pub fn decode_sjisx0213(s: &[u8]) -> Decoded {
    let t = tables();
    let c = s[0];
    match c {
        0x5C => (Dec::Char(0xA5, 1), 0),
        0x7E => (Dec::Char(0x203E, 1), 0),
        0..=0x7F => (Dec::Char(c as u32, 1), 0),
        0xA1..=0xDF => (Dec::Char(0xFF61 + (c - 0xA1) as u32, 1), 0),
        0x81..=0x9F | 0xE0..=0xFC => {
            if s.len() < 2 {
                return (Dec::Incomplete, 0);
            }
            let v = match sjis_to_jis(c, s[1]) {
                Some((1, r, col)) => jis_plane1(r + 0x20, col + 0x20),
                Some((_, r, col)) => (t.jis2.get(r + 0x20, col + 0x20), 0),
                None => (0, 0),
            };
            match v {
                (0, _) => (Dec::Illegal(1), 0),
                (a, b) => (Dec::Char(a, 2), b),
            }
        }
        _ => (Dec::Illegal(1), 0),
    }
}

/// A CNS 11643 plane 3-7 or 15 character (0x21-0x7E bytes).
pub fn cns_high(plane: u8, l: u8, c: u8) -> u32 {
    match plane {
        3..=7 => tables().cns[(plane - 3) as usize].get(l, c),
        15 => tables().cns[5].get(l, c),
        _ => 0,
    }
}

/// The plane 3-7 or 15 code of a character: plane << 16 | row/column
/// bytes.
pub fn cns_high_code(cp: u32) -> Option<u32> {
    tables().cns_enc.get(cp).filter(|&c| c != 0)
}

impl Enc {
    /// The code of `cp`: a table entry, else an ASCII character as itself
    /// when `ident`.
    pub fn code(&self, cp: u32, ident: bool) -> Option<u32> {
        match self.get(cp) {
            Some(0) => None,
            Some(c) => Some(c),
            None => (cp < 0x80 && ident).then_some(cp),
        }
    }
}

fn push_code(out: &mut Vec<u8>, c: u32) {
    if c >= 0x100 {
        out.push((c >> 8) as u8);
    }
    out.push(c as u8);
}

pub fn encode_johab(cp: u32, out: &mut Vec<u8>) -> bool {
    let t = tables();
    let ident = cp < 0x80 && single(&t.johab1, cp as u8) == if cp == 0 { 0x80000000 } else { cp };
    match t.johab_enc.code(cp, ident) {
        Some(c) => {
            push_code(out, c);
            true
        }
        None => false,
    }
}

/// The charsets whose encoders combine a character with the next one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Comp {
    Hkscs,
    EucJis,
    SjisJis,
}

pub fn comp_enc(c: Comp) -> &'static Enc {
    let t = tables();
    match c {
        Comp::Hkscs => &t.hkscs_enc,
        Comp::EucJis | Comp::SjisJis => &t.jis_enc,
    }
}

/// One character (or a composition's code) in a combining charset.
pub fn encode_code(c: Comp, code: u32, out: &mut Vec<u8>) {
    match c {
        Comp::Hkscs => push_code(out, code),
        Comp::EucJis => match code {
            0..=0xFF => out.push(code as u8),
            0x8E00..=0x8EFF => out.extend_from_slice(&[0x8E, code as u8]),
            0x10000.. => {
                out.extend_from_slice(&[0x8F, (code >> 8) as u8 | 0x80, code as u8 | 0x80])
            }
            _ => out.extend_from_slice(&[(code >> 8) as u8 | 0x80, code as u8 | 0x80]),
        },
        Comp::SjisJis => match code {
            0..=0xFF => out.push(code as u8),
            0x8E00..=0x8EFF => out.push(code as u8),
            _ => {
                let plane = if code >= 0x10000 { 2 } else { 1 };
                let (l, t) = jis_to_sjis(
                    plane,
                    ((code >> 8) & 0xFF) as u8 - 0x20,
                    (code & 0xFF) as u8 - 0x20,
                );
                out.extend_from_slice(&[l, t]);
            }
        },
    }
}

/// The code of one character in a combining charset.
pub fn comp_code(c: Comp, cp: u32) -> Option<u32> {
    let t = tables();
    match c {
        Comp::Hkscs => {
            let ident =
                cp < 0x80 && single(&t.hkscs1, cp as u8) == if cp == 0 { 0x80000000 } else { cp };
            t.hkscs_enc.code(cp, ident)
        }
        Comp::EucJis => t.jis_enc.code(cp, true),
        Comp::SjisJis => match cp {
            0xA5 => Some(0x5C),
            0x203E => Some(0x7E),
            _ => t.jis_enc.code(cp, true),
        },
    }
}
