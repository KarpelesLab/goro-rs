//! apprentice.c: loading compiled and text magic files.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::magic::*;

struct TypeTbl {
    name: &'static [u8],
    typ: u8,
    format: u8,
}

macro_rules! tt {
    ($n:literal, $t:expr, $f:expr) => {
        TypeTbl {
            name: $n,
            typ: $t,
            format: $f,
        }
    };
}

static TYPE_TBL: &[TypeTbl] = &[
    tt!(b"invalid", FILE_INVALID, FILE_FMT_NONE),
    tt!(b"byte", FILE_BYTE, FILE_FMT_NUM),
    tt!(b"short", FILE_SHORT, FILE_FMT_NUM),
    tt!(b"default", FILE_DEFAULT, FILE_FMT_NONE),
    tt!(b"long", FILE_LONG, FILE_FMT_NUM),
    tt!(b"string", FILE_STRING, FILE_FMT_STR),
    tt!(b"date", FILE_DATE, FILE_FMT_STR),
    tt!(b"beshort", FILE_BESHORT, FILE_FMT_NUM),
    tt!(b"belong", FILE_BELONG, FILE_FMT_NUM),
    tt!(b"bedate", FILE_BEDATE, FILE_FMT_STR),
    tt!(b"leshort", FILE_LESHORT, FILE_FMT_NUM),
    tt!(b"lelong", FILE_LELONG, FILE_FMT_NUM),
    tt!(b"ledate", FILE_LEDATE, FILE_FMT_STR),
    tt!(b"pstring", FILE_PSTRING, FILE_FMT_STR),
    tt!(b"ldate", FILE_LDATE, FILE_FMT_STR),
    tt!(b"beldate", FILE_BELDATE, FILE_FMT_STR),
    tt!(b"leldate", FILE_LELDATE, FILE_FMT_STR),
    tt!(b"regex", FILE_REGEX, FILE_FMT_STR),
    tt!(b"bestring16", FILE_BESTRING16, FILE_FMT_STR),
    tt!(b"lestring16", FILE_LESTRING16, FILE_FMT_STR),
    tt!(b"search", FILE_SEARCH, FILE_FMT_STR),
    tt!(b"medate", FILE_MEDATE, FILE_FMT_STR),
    tt!(b"meldate", FILE_MELDATE, FILE_FMT_STR),
    tt!(b"melong", FILE_MELONG, FILE_FMT_NUM),
    tt!(b"quad", FILE_QUAD, FILE_FMT_QUAD),
    tt!(b"lequad", FILE_LEQUAD, FILE_FMT_QUAD),
    tt!(b"bequad", FILE_BEQUAD, FILE_FMT_QUAD),
    tt!(b"qdate", FILE_QDATE, FILE_FMT_STR),
    tt!(b"leqdate", FILE_LEQDATE, FILE_FMT_STR),
    tt!(b"beqdate", FILE_BEQDATE, FILE_FMT_STR),
    tt!(b"qldate", FILE_QLDATE, FILE_FMT_STR),
    tt!(b"leqldate", FILE_LEQLDATE, FILE_FMT_STR),
    tt!(b"beqldate", FILE_BEQLDATE, FILE_FMT_STR),
    tt!(b"float", FILE_FLOAT, FILE_FMT_FLOAT),
    tt!(b"befloat", FILE_BEFLOAT, FILE_FMT_FLOAT),
    tt!(b"lefloat", FILE_LEFLOAT, FILE_FMT_FLOAT),
    tt!(b"double", FILE_DOUBLE, FILE_FMT_DOUBLE),
    tt!(b"bedouble", FILE_BEDOUBLE, FILE_FMT_DOUBLE),
    tt!(b"ledouble", FILE_LEDOUBLE, FILE_FMT_DOUBLE),
    tt!(b"leid3", FILE_LEID3, FILE_FMT_NUM),
    tt!(b"beid3", FILE_BEID3, FILE_FMT_NUM),
    tt!(b"indirect", FILE_INDIRECT, FILE_FMT_NUM),
    tt!(b"qwdate", FILE_QWDATE, FILE_FMT_STR),
    tt!(b"leqwdate", FILE_LEQWDATE, FILE_FMT_STR),
    tt!(b"beqwdate", FILE_BEQWDATE, FILE_FMT_STR),
    tt!(b"name", FILE_NAME, FILE_FMT_NONE),
    tt!(b"use", FILE_USE, FILE_FMT_NONE),
    tt!(b"clear", FILE_CLEAR, FILE_FMT_NONE),
    tt!(b"der", FILE_DER, FILE_FMT_STR),
    tt!(b"guid", FILE_GUID, FILE_FMT_STR),
    tt!(b"offset", FILE_OFFSET, FILE_FMT_QUAD),
    tt!(b"bevarint", FILE_BEVARINT, FILE_FMT_STR),
    tt!(b"levarint", FILE_LEVARINT, FILE_FMT_STR),
    tt!(b"msdosdate", FILE_MSDOSDATE, FILE_FMT_STR),
    tt!(b"lemsdosdate", FILE_LEMSDOSDATE, FILE_FMT_STR),
    tt!(b"bemsdosdate", FILE_BEMSDOSDATE, FILE_FMT_STR),
    tt!(b"msdostime", FILE_MSDOSTIME, FILE_FMT_STR),
    tt!(b"lemsdostime", FILE_LEMSDOSTIME, FILE_FMT_STR),
    tt!(b"bemsdostime", FILE_BEMSDOSTIME, FILE_FMT_STR),
    tt!(b"octal", FILE_OCTAL, FILE_FMT_STR),
];

static SPECIAL_TBL: &[TypeTbl] = &[
    tt!(b"der", FILE_DER, FILE_FMT_STR),
    tt!(b"name", FILE_NAME, FILE_FMT_STR),
    tt!(b"use", FILE_USE, FILE_FMT_STR),
    tt!(b"octal", FILE_OCTAL, FILE_FMT_STR),
];

pub fn file_name(t: u8) -> &'static [u8] {
    TYPE_TBL.get(t as usize).map_or(b"", |e| e.name)
}

fn file_format(t: u8) -> u8 {
    TYPE_TBL.get(t as usize).map_or(FILE_FMT_NONE, |e| e.format)
}

// ---------------------------------------------------------------------------
// a cursor on a NUL-terminated line

struct Cur<'a> {
    s: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    fn c(&self) -> u8 {
        self.at(0)
    }
    fn at(&self, i: usize) -> u8 {
        *self.s.get(self.p + i).unwrap_or(&0)
    }
    /// the C string at the cursor
    fn rest(&self) -> &'a [u8] {
        cstr(&self.s[self.p.min(self.s.len())..])
    }
    fn eatab(&mut self) {
        while self.c() < 0x80 && c_isspace(self.c()) && self.c() != 0 {
            self.p += 1;
        }
    }
}

fn get_type(tbl: &[TypeTbl], l: &Cur) -> (u8, usize) {
    let rest = l.rest();
    for e in tbl {
        if rest.starts_with(e.name) {
            return (e.typ, e.name.len());
        }
    }
    (FILE_INVALID, 0)
}

fn get_standard_integer_type(l: &Cur) -> (u8, usize) {
    let c1 = l.at(1);
    if l.c() == 0 || c1 == 0 {
        return (FILE_INVALID, 0);
    }
    if c1.is_ascii_alphabetic() {
        let t = match c1 {
            b'C' => FILE_BYTE,
            b'S' => FILE_SHORT,
            b'I' | b'L' => FILE_LONG,
            b'Q' => FILE_QUAD,
            _ => return (FILE_INVALID, 0),
        };
        (t, 2)
    } else if c1.is_ascii_digit() {
        if l.at(2).is_ascii_digit() {
            return (FILE_INVALID, 0);
        }
        let t = match c1 {
            b'1' => FILE_BYTE,
            b'2' => FILE_SHORT,
            b'4' => FILE_LONG,
            b'8' => FILE_QUAD,
            _ => return (FILE_INVALID, 0),
        };
        (t, 2)
    } else {
        (FILE_LONG, 1)
    }
}

// ---------------------------------------------------------------------------
// strength

fn nonmagic(s: &[u8]) -> usize {
    let s = cstr(s);
    let mut rv = 0usize;
    let mut p = 0usize;
    let at = |i: usize| *s.get(i).unwrap_or(&0);
    while at(p) != 0 {
        match at(p) {
            b'\\' => {
                p += 1;
                if at(p) == 0 {
                    p -= 1;
                }
                rv += 1;
            }
            b'?' | b'*' | b'.' | b'+' | b'^' | b'$' => {}
            b'[' => {
                while at(p) != 0 && at(p) != b']' {
                    p += 1;
                }
                p = p.wrapping_sub(1);
            }
            b'{' => {
                while at(p) != 0 && at(p) != b'}' {
                    p += 1;
                }
                if at(p) == 0 {
                    p -= 1;
                }
            }
            _ => rv += 1,
        }
        p = p.wrapping_add(1);
    }
    if rv == 0 { 1 } else { rv }
}

pub fn typesize(t: u8) -> usize {
    match t {
        FILE_BYTE => 1,
        FILE_SHORT | FILE_LESHORT | FILE_BESHORT | FILE_MSDOSDATE | FILE_BEMSDOSDATE
        | FILE_LEMSDOSDATE | FILE_MSDOSTIME | FILE_BEMSDOSTIME | FILE_LEMSDOSTIME => 2,
        FILE_LONG | FILE_LELONG | FILE_BELONG | FILE_MELONG => 4,
        FILE_DATE | FILE_LEDATE | FILE_BEDATE | FILE_MEDATE | FILE_LDATE | FILE_LELDATE
        | FILE_BELDATE | FILE_MELDATE | FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT | FILE_BEID3
        | FILE_LEID3 => 4,
        FILE_QUAD | FILE_BEQUAD | FILE_LEQUAD | FILE_QDATE | FILE_LEQDATE | FILE_BEQDATE
        | FILE_QLDATE | FILE_LEQLDATE | FILE_BEQLDATE | FILE_QWDATE | FILE_LEQWDATE
        | FILE_BEQWDATE | FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE | FILE_OFFSET
        | FILE_BEVARINT | FILE_LEVARINT => 8,
        FILE_GUID => 16,
        _ => FILE_BADSIZE,
    }
}

fn strength_1(m: &Magic, warn: &mut dyn FnMut(String)) -> isize {
    const MULT: isize = 10;
    let mut val: isize = 2 * MULT;
    match m.typ {
        FILE_DEFAULT => {
            if m.factor_op != FILE_FACTOR_OP_NONE {
                warn(format!("Unsupported factor_op in default {}", m.factor_op));
            }
            return 0;
        }
        FILE_PSTRING | FILE_STRING | FILE_OCTAL => val += m.vallen as isize * MULT,
        FILE_BESTRING16 | FILE_LESTRING16 => val += m.vallen as isize * MULT / 2,
        FILE_SEARCH => {
            if m.vallen != 0 {
                val += m.vallen as isize * (MULT / m.vallen as isize).max(1);
            }
        }
        FILE_REGEX => {
            let v = nonmagic(&m.value) as isize;
            val += v * (MULT / v).max(1);
        }
        FILE_INDIRECT | FILE_NAME | FILE_USE | FILE_CLEAR => {}
        FILE_DER => val += MULT,
        t => {
            let ts = typesize(t);
            if ts != FILE_BADSIZE {
                val += ts as isize * MULT;
            }
        }
    }
    match m.reln {
        b'x' | b'!' => val = 0,
        b'=' => val += MULT,
        b'>' | b'<' => val -= 2 * MULT,
        b'^' | b'&' => val -= MULT,
        _ => {}
    }
    val
}

/// file_magic_strength()
pub fn magic_strength(m: &Magic, warn: &mut dyn FnMut(String)) -> usize {
    let mut val = strength_1(m, warn);
    match m.factor_op {
        FILE_FACTOR_OP_PLUS => val += m.factor as isize,
        FILE_FACTOR_OP_MINUS => val -= m.factor as isize,
        FILE_FACTOR_OP_TIMES => val *= m.factor as isize,
        FILE_FACTOR_OP_DIV => {
            if m.factor != 0 {
                val /= m.factor as isize
            }
        }
        _ => {}
    }
    if val <= 0 {
        val = 1;
    }
    if m.desc[0] == 0 {
        val += 1;
    }
    val as usize
}

fn set_test_type(m: &mut Magic) {
    match m.typ {
        FILE_STRING | FILE_PSTRING | FILE_BESTRING16 | FILE_LESTRING16 => {
            if m.str_flags() & STRING_TEXTTEST != 0 {
                m.flag |= TEXTTEST;
            } else {
                m.flag |= BINTEST;
            }
        }
        FILE_REGEX | FILE_SEARCH => {
            if m.str_flags() & STRING_BINTEST != 0 {
                m.flag |= BINTEST;
            }
            if m.str_flags() & STRING_TEXTTEST != 0 {
                m.flag |= TEXTTEST;
            }
            if m.flag & (TEXTTEST | BINTEST) != 0 {
                return;
            }
            let n = m.vallen as usize;
            if crate::encoding::looks_utf8(&m.value[..n.min(MAXSTRING)], None) <= 0 {
                m.flag |= BINTEST;
            } else {
                m.flag |= TEXTTEST;
            }
        }
        FILE_DEFAULT | FILE_INVALID => {}
        FILE_BYTE..=FILE_OCTAL => m.flag |= BINTEST,
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// parsing a text magic file

thread_local! {
    /// parse()'s static last_cont_level
    static LAST_CONT_LEVEL: Cell<u32> = const { Cell::new(0) };
}

type Entry = Vec<Magic>;

struct Loader<'a> {
    ms: &'a mut MagicSet,
    host: &'a mut dyn Host,
}

impl Loader<'_> {
    fn check(&self) -> bool {
        self.ms.flags & MAGIC_CHECK != 0
    }
    fn magwarn(&mut self, msg: String) {
        self.host.warn(msg.as_bytes());
    }
    fn magwarn_b(&mut self, msg: &[u8]) {
        self.host.warn(msg);
    }

    fn file_check_mem(&mut self, level: usize) {
        self.ms.check_mem(level);
    }

    fn check_cond(&mut self, cond: u8, cont_level: usize) -> Result<(), ()> {
        if cont_level >= self.ms.c.len() {
            self.ms.c.resize(cont_level + 1, LevelInfo::default());
        }
        let mut last_cond = self.ms.c[cont_level].last_cond;
        match cond {
            COND_IF => {
                if last_cond != COND_NONE && last_cond != COND_ELIF {
                    if self.check() {
                        self.magwarn("syntax error: `if'".into());
                    }
                    return Err(());
                }
                last_cond = COND_IF;
            }
            COND_ELIF => {
                if last_cond != COND_IF && last_cond != COND_ELIF {
                    if self.check() {
                        self.magwarn("syntax error: `elif'".into());
                    }
                    return Err(());
                }
                last_cond = COND_ELIF;
            }
            COND_ELSE => {
                if last_cond != COND_IF && last_cond != COND_ELIF {
                    if self.check() {
                        self.magwarn("syntax error: `else'".into());
                    }
                    return Err(());
                }
                last_cond = COND_NONE;
            }
            _ => last_cond = COND_NONE,
        }
        self.ms.c[cont_level].last_cond = last_cond;
        Ok(())
    }

    /// parse(): 0 ok, 1 new entry needed, -1 error
    fn parse(&mut self, me: &mut Option<Entry>, file: &[u8], line: &[u8], lineno: usize) -> i32 {
        let mut l = Cur { s: line, p: 0 };
        let mut cont_level: u32 = 0;
        while l.c() == b'>' {
            l.p += 1;
            cont_level += 1;
        }
        let last = LAST_CONT_LEVEL.with(|c| c.get());
        if cont_level == 0 || cont_level > last {
            self.file_check_mem(cont_level as usize);
        }
        LAST_CONT_LEVEL.with(|c| c.set(cont_level));
        if cont_level != 0 {
            let Some(e) = me.as_mut() else {
                self.ms
                    .magerror(b"No current entry for continuation".to_vec());
                return -1;
            };
            if e.is_empty() {
                self.ms
                    .magerror(b"Continuations present with 0 count".to_vec());
                return -1;
            }
            let prev = e.last().unwrap().cont_level;
            let diff = cont_level as i32 - prev as i32;
            if diff > 1 {
                self.magwarn(format!(
                    "New continuation level {} is more than one larger than current level {}",
                    cont_level, prev
                ));
            }
            let m = Magic {
                cont_level: cont_level as u16,
                ..Default::default()
            };
            me.as_mut().unwrap().push(m);
        } else {
            if me.is_some() {
                return 1;
            }
            let m = Magic {
                factor_op: FILE_FACTOR_OP_NONE,
                ..Default::default()
            };
            *me = Some(vec![m]);
        }
        let mut m = me.as_mut().unwrap().pop().unwrap();
        let r = self.parse_magic(&mut m, &mut l, file, cont_level, lineno);
        me.as_mut().unwrap().push(m);
        r
    }

    fn parse_magic(
        &mut self,
        m: &mut Magic,
        l: &mut Cur,
        file: &[u8],
        cont_level: u32,
        lineno: usize,
    ) -> i32 {
        m.lineno = lineno as u32;
        if l.c() == b'&' {
            l.p += 1;
            m.flag |= OFFADD;
        }
        if l.c() == b'(' {
            l.p += 1;
            m.flag |= INDIR;
            if m.flag & OFFADD != 0 {
                m.flag = (m.flag & !OFFADD) | INDIROFFADD;
            }
            if l.c() == b'&' {
                l.p += 1;
                m.flag |= OFFADD;
            }
        }
        if m.cont_level == 0 && m.flag & (OFFADD | INDIROFFADD) != 0 {
            if self.check() {
                self.magwarn("relative offset at level 0".into());
            }
            return -1;
        }
        if l.c() == b'-' {
            l.p += 1;
            m.flag |= OFFNEGATIVE;
        }
        let (v, n) = c_strtol(l.rest(), 0);
        m.offset = v as i32;
        if n == 0 {
            if self.check() {
                let mut w = b"offset `".to_vec();
                w.extend_from_slice(l.rest());
                w.extend_from_slice(b"' invalid");
                self.magwarn_b(&w);
            }
            return -1;
        }
        l.p += n;
        // `t' of strtol(), used in a message below
        let mut t_char = l.c();
        if m.flag & INDIR != 0 {
            m.in_type = FILE_LONG;
            m.in_offset = 0;
            m.in_op = 0;
            if l.c() == b'.' || l.c() == b',' {
                if l.c() == b',' {
                    m.in_op |= FILE_OPSIGNED;
                }
                l.p += 1;
                m.in_type = match l.c() {
                    b'l' => FILE_LELONG,
                    b'L' => FILE_BELONG,
                    b'm' => FILE_MELONG,
                    b'h' | b's' => FILE_LESHORT,
                    b'H' | b'S' => FILE_BESHORT,
                    b'c' | b'b' | b'C' | b'B' => FILE_BYTE,
                    b'e' | b'f' | b'g' => FILE_LEDOUBLE,
                    b'E' | b'F' | b'G' => FILE_BEDOUBLE,
                    b'i' => FILE_LEID3,
                    b'I' => FILE_BEID3,
                    b'o' => FILE_OCTAL,
                    b'q' => FILE_LEQUAD,
                    b'Q' => FILE_BEQUAD,
                    c => {
                        if self.check() {
                            let mut w = b"indirect offset type `".to_vec();
                            w.push(c);
                            w.extend_from_slice(b"' invalid");
                            self.magwarn_b(&w);
                        }
                        return -1;
                    }
                };
                l.p += 1;
            }
            if l.c() == b'~' {
                m.in_op |= FILE_OPINVERSE;
                l.p += 1;
            }
            if let Some(op) = get_op(l.c()) {
                m.in_op |= op;
                l.p += 1;
            }
            if l.c() == b'(' {
                m.in_op |= FILE_OPINDIRECT;
                l.p += 1;
            }
            if l.c().is_ascii_digit() || l.c() == b'-' {
                let (v, n) = c_strtol(l.rest(), 0);
                if n == 0 {
                    if self.check() {
                        let mut w = b"in_offset `".to_vec();
                        w.extend_from_slice(l.rest());
                        w.extend_from_slice(b"' invalid");
                        self.magwarn_b(&w);
                    }
                    return -1;
                }
                m.in_offset = v as i32;
                l.p += n;
                t_char = l.c();
            }
            let c = l.c();
            l.p += 1;
            let bad = if c != b')' {
                true
            } else if m.in_op & FILE_OPINDIRECT != 0 {
                let c2 = l.c();
                l.p += 1;
                c2 != b')'
            } else {
                false
            };
            if bad {
                if self.check() {
                    self.magwarn("missing ')' in indirect offset".into());
                }
                return -1;
            }
        }
        l.eatab();
        m.cond = get_cond(l);
        if self.check_cond(m.cond, cont_level as usize).is_err() {
            return -1;
        }
        l.eatab();

        // the type
        if l.c() == b'u' {
            let sub = Cur { s: l.s, p: l.p + 1 };
            let (t, n) = get_type(TYPE_TBL, &sub);
            m.typ = t;
            if t != FILE_INVALID {
                l.p += 1 + n;
            } else {
                let (t, n) = get_standard_integer_type(l);
                m.typ = t;
                l.p += n;
            }
            if m.typ != FILE_INVALID {
                m.flag |= UNSIGNED;
            }
        } else {
            let (t, n) = get_type(TYPE_TBL, l);
            m.typ = t;
            l.p += n;
            if m.typ == FILE_INVALID {
                if l.c() == b'd' {
                    let (t, n) = get_standard_integer_type(l);
                    m.typ = t;
                    l.p += n;
                } else if l.c() == b's' && !l.at(1).is_ascii_alphabetic() {
                    m.typ = FILE_STRING;
                    l.p += 1;
                }
            }
        }
        if m.typ == FILE_INVALID {
            let (t, n) = get_type(SPECIAL_TBL, l);
            m.typ = t;
            l.p += n;
        }
        if m.typ == FILE_INVALID {
            if self.check() {
                let mut w = b"type `".to_vec();
                w.extend_from_slice(l.rest());
                w.extend_from_slice(b"' invalid");
                self.magwarn_b(&w);
            }
            return -1;
        }
        if m.typ == FILE_NAME && cont_level != 0 {
            if self.check() {
                let mut w = b"`name".to_vec();
                w.extend_from_slice(l.rest());
                w.extend_from_slice(b"' entries can only be declared at top level");
                self.magwarn_b(&w);
            }
            return -1;
        }
        m.mask_op = 0;
        if l.c() == b'~' {
            if !is_libmagic_string(m.typ) {
                m.mask_op |= FILE_OPINVERSE;
            } else if self.check() {
                self.magwarn("'~' invalid for string types".into());
            }
            l.p += 1;
        }
        m.set_str_range(0);
        m.set_str_flags(if m.typ == FILE_PSTRING {
            PSTRING_1_LE
        } else {
            0
        });
        if let Some(op) = get_op(l.c()) {
            if is_libmagic_string(m.typ) {
                if op != FILE_OPDIVIDE {
                    if self.check() {
                        let mut w = b"invalid string/indirect op: `".to_vec();
                        w.push(t_char);
                        w.push(b'\'');
                        self.magwarn_b(&w);
                    }
                    return -1;
                }
                let r = if m.typ == FILE_INDIRECT {
                    self.parse_indirect_modifier(m, l)
                } else {
                    self.parse_string_modifier(m, l)
                };
                if r.is_err() {
                    return -1;
                }
            } else {
                self.parse_op_modifier(m, l, op);
            }
        }
        l.eatab();
        match l.c() {
            b'>' | b'<' => {
                m.reln = l.c();
                l.p += 1;
                if l.c() == b'=' {
                    if self.check() {
                        self.magwarn(format!("{}= not supported", m.reln as char));
                        return -1;
                    }
                    l.p += 1;
                }
            }
            b'&' | b'^' | b'=' => {
                m.reln = l.c();
                l.p += 1;
                if l.c() == b'=' {
                    l.p += 1;
                }
            }
            b'!' => {
                m.reln = l.c();
                l.p += 1;
            }
            _ => {
                m.reln = b'=';
                if l.c() == b'x' && ((l.at(1) < 0x80 && c_isspace(l.at(1))) || l.at(1) == 0) {
                    m.reln = b'x';
                    l.p += 1;
                }
            }
        }
        if m.reln != b'x' && self.getvalue(m, l).is_err() {
            return -1;
        }
        l.eatab();
        if l.c() == 0x08 {
            l.p += 1;
            m.flag |= NOSPACE;
        } else if l.c() == b'\\' && l.at(1) == b'b' {
            l.p += 2;
            m.flag |= NOSPACE;
        }
        let rest = l.rest();
        let mut i = 0usize;
        loop {
            if i >= 64 {
                break;
            }
            let c = *rest.get(i).unwrap_or(&0);
            m.desc[i] = c;
            i += 1;
            if c == 0 {
                break;
            }
        }
        if m.desc[0] == 0 {
            let n = file.len().min(62);
            m.desc[1..1 + n].copy_from_slice(&file[..n]);
            m.desc[1 + n] = 0;
        }
        if i == 64 {
            m.desc[63] = 0;
            if self.check() {
                let mut w = b"description `".to_vec();
                w.extend_from_slice(cstr(&m.desc));
                w.extend_from_slice(b"' truncated");
                self.magwarn_b(&w);
            }
        }
        if self.check() && self.check_format(m).is_err() {
            return -1;
        }
        m.mimetype[0] = 0;
        0
    }

    fn parse_indirect_modifier(&mut self, m: &mut Magic, l: &mut Cur) -> Result<(), ()> {
        loop {
            l.p += 1;
            let c = l.c();
            if c_isspace(c) {
                break;
            }
            match c {
                b'r' => {
                    let f = m.str_flags() | INDIRECT_RELATIVE;
                    m.set_str_flags(f);
                }
                _ => {
                    if self.check() {
                        let mut w = b"indirect modifier `".to_vec();
                        w.push(c);
                        w.extend_from_slice(b"' invalid");
                        self.magwarn_b(&w);
                    }
                    return Err(());
                }
            }
        }
        Ok(())
    }

    fn parse_op_modifier(&mut self, m: &mut Magic, l: &mut Cur, op: u8) {
        l.p += 1;
        m.mask_op |= op;
        let (v, n, _) = c_strtoull(l.rest(), 0);
        l.p += n;
        let v = self.ms.signextend(m, v, self.host);
        m.set_num_mask(v);
        eatsize(l);
    }

    fn parse_string_modifier(&mut self, m: &mut Magic, l: &mut Cur) -> Result<(), ()> {
        let mut have_range = false;
        loop {
            l.p += 1;
            let c = l.c();
            if c_isspace(c) {
                break;
            }
            let mut flags = m.str_flags();
            let bad;
            match c {
                b'0'..=b'9' => {
                    if have_range && self.check() {
                        self.magwarn("multiple ranges".into());
                    }
                    have_range = true;
                    let (v, n, _) = c_strtoull(l.rest(), 0);
                    m.set_str_range(v as u32);
                    if m.str_range() == 0 {
                        self.magwarn("zero range".into());
                    }
                    l.p += n - 1;
                    bad = false;
                }
                b'W' => {
                    flags |= STRING_COMPACT_WHITESPACE;
                    bad = false;
                }
                b'w' => {
                    flags |= STRING_COMPACT_OPTIONAL_WHITESPACE;
                    bad = false;
                }
                b'c' => {
                    flags |= STRING_IGNORE_LOWERCASE;
                    bad = false;
                }
                b'C' => {
                    flags |= STRING_IGNORE_UPPERCASE;
                    bad = false;
                }
                b's' => {
                    flags |= REGEX_OFFSET_START;
                    bad = false;
                }
                b'b' => {
                    flags |= STRING_BINTEST;
                    bad = false;
                }
                b't' => {
                    flags |= STRING_TEXTTEST;
                    bad = false;
                }
                b'T' => {
                    flags |= STRING_TRIM;
                    bad = false;
                }
                b'f' => {
                    flags |= STRING_FULL_WORD;
                    bad = false;
                }
                b'B' | b'H' | b'h' | b'L' => {
                    if m.typ != FILE_PSTRING {
                        bad = true;
                    } else {
                        let len = match c {
                            b'B' => PSTRING_1_LE,
                            b'H' => PSTRING_2_BE,
                            b'h' => PSTRING_2_LE,
                            _ => PSTRING_4_BE,
                        };
                        flags = (flags & !PSTRING_LEN) | len;
                        bad = false;
                    }
                }
                b'l' => {
                    if m.typ != FILE_PSTRING && m.typ != FILE_REGEX {
                        bad = true;
                    } else {
                        flags = (flags & !PSTRING_LEN) | PSTRING_4_LE;
                        bad = false;
                    }
                }
                b'J' => {
                    if m.typ != FILE_PSTRING {
                        bad = true;
                    } else {
                        flags |= PSTRING_LENGTH_INCLUDES_ITSELF;
                        bad = false;
                    }
                }
                _ => bad = true,
            }
            if bad {
                if self.check() {
                    let mut w = b"string modifier `".to_vec();
                    w.push(c);
                    w.extend_from_slice(b"' invalid");
                    self.magwarn_b(&w);
                }
                return Err(());
            }
            m.set_str_flags(flags);
            if l.at(1) == b'/' && !c_isspace(l.at(2)) {
                l.p += 1;
            }
        }
        if self.string_modifier_check(m).is_err() {
            return Err(());
        }
        Ok(())
    }

    fn string_modifier_check(&mut self, m: &mut Magic) -> Result<(), ()> {
        if !self.check() {
            return Ok(());
        }
        let f = m.str_flags();
        if (m.typ != FILE_REGEX || f & REGEX_LINE_COUNT == 0)
            && (m.typ != FILE_PSTRING && f & PSTRING_LEN != 0)
        {
            self.magwarn("'/BHhLl' modifiers are only allowed for pascal strings\n".into());
            return Err(());
        }
        match m.typ {
            FILE_BESTRING16 | FILE_LESTRING16 => {
                if f != 0 {
                    self.magwarn("no modifiers allowed for 16-bit strings\n".into());
                    return Err(());
                }
            }
            FILE_STRING | FILE_PSTRING => {
                if f & REGEX_OFFSET_START != 0 {
                    self.magwarn("'/s' only allowed on regex and search\n".into());
                    return Err(());
                }
            }
            FILE_SEARCH => {
                if m.str_range() == 0 {
                    self.magwarn(format!(
                        "missing range; defaulting to {}\n",
                        STRING_DEFAULT_RANGE
                    ));
                    m.set_str_range(STRING_DEFAULT_RANGE);
                    return Err(());
                }
            }
            FILE_REGEX => {
                if f & STRING_COMPACT_WHITESPACE != 0 {
                    self.magwarn("'/W' not allowed on regex\n".into());
                    return Err(());
                }
                if f & STRING_COMPACT_OPTIONAL_WHITESPACE != 0 {
                    self.magwarn("'/w' not allowed on regex\n".into());
                    return Err(());
                }
            }
            t => {
                self.magwarn(format!("coding error: m->type={}\n", t));
                return Err(());
            }
        }
        Ok(())
    }

    fn getvalue(&mut self, m: &mut Magic, l: &mut Cur) -> Result<(), ()> {
        match m.typ {
            FILE_BESTRING16 | FILE_LESTRING16 | FILE_STRING | FILE_PSTRING | FILE_REGEX
            | FILE_SEARCH | FILE_NAME | FILE_USE | FILE_DER | FILE_OCTAL => {
                match self.getstr(m, l) {
                    Some(p) => l.p = p,
                    None => {
                        if self.check() {
                            let mut w = b"cannot get string from `".to_vec();
                            w.extend_from_slice(cstr(&m.value));
                            w.push(b'\'');
                            self.magwarn_b(&w);
                        }
                        return Err(());
                    }
                }
                if m.typ == FILE_REGEX {
                    let s = cstr(&m.value).to_vec();
                    let pat = convert_libmagic_pattern(&s, false, false);
                    if !self.host.regex_compile(&pat) {
                        return Err(());
                    }
                }
                return Ok(());
            }
            _ => {
                if m.reln == b'x' {
                    return Ok(());
                }
            }
        }
        match m.typ {
            FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT => {
                let (v, n, ok) = c_strtod(l.rest());
                let fv = v as f32;
                m.value[..4].copy_from_slice(&fv.to_bits().to_le_bytes());
                if ok && !(fv.is_infinite() && v.is_finite()) {
                    l.p += n;
                }
                Ok(())
            }
            FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE => {
                let (v, n, ok) = c_strtod(l.rest());
                m.value[..8].copy_from_slice(&v.to_bits().to_le_bytes());
                if ok {
                    l.p += n;
                }
                Ok(())
            }
            FILE_GUID => {
                let Some(g) = parse_guid(l.rest()) else {
                    return Err(());
                };
                m.value[..16].copy_from_slice(&g);
                l.p += 36;
                Ok(())
            }
            _ => {
                let (ull0, n, ovf) = c_strtoull(l.rest(), 0);
                let v = self.ms.signextend(m, ull0, self.host);
                m.set_value_q(v);
                if n == 0 {
                    let mut w = b"Unparsable number `".to_vec();
                    w.extend_from_slice(l.rest());
                    w.push(b'\'');
                    self.magwarn_b(&w);
                    return Err(());
                }
                let ts = typesize(m.typ);
                if ts == FILE_BADSIZE {
                    let mut w = b"Expected numeric type got `".to_vec();
                    w.extend_from_slice(file_name(m.typ));
                    w.push(b'\'');
                    self.magwarn_b(&w);
                    return Err(());
                }
                let rest = l.rest();
                let mut q = 0;
                while q < rest.len() && c_isspace(rest[q]) {
                    q += 1;
                }
                let mut ull = ull0;
                if rest.get(q) == Some(&b'-') && ull != u64::MAX {
                    ull = (ull as i64).wrapping_neg() as u64;
                }
                let (x, y) = match ts {
                    1 => {
                        let x = ull & !0xff;
                        (x, (x & !0xff) != !0xff)
                    }
                    2 => {
                        let x = ull & !0xffff;
                        (x, (x & !0xffff) != !0xffff)
                    }
                    4 => {
                        let x = ull & !0xffff_ffff;
                        (x, (x & !0xffff_ffff) != !0xffff_ffff)
                    }
                    _ => (0, false),
                };
                if x != 0 && y {
                    let mut w = b"Overflow for numeric type `".to_vec();
                    w.extend_from_slice(file_name(m.typ));
                    w.extend_from_slice(format!("' value {:#x}", ull).as_bytes());
                    self.magwarn_b(&w);
                    return Err(());
                }
                if !ovf {
                    l.p += n;
                    eatsize(l);
                }
                Ok(())
            }
        }
    }

    /// getstr(): the new cursor position
    fn getstr(&mut self, m: &mut Magic, l: &Cur) -> Option<usize> {
        let origs = l.rest().to_vec();
        let mut s = l.p;
        let at = |i: usize| *l.s.get(i).unwrap_or(&0);
        let mut p = 0usize;
        let pmax = MAXSTRING - 1;
        m.value = [0; MAXSTRING];
        loop {
            let c = at(s);
            s += 1;
            if c == 0 {
                break;
            }
            if c_isspace(c) {
                break;
            }
            if p >= pmax {
                let mut e = b"string too long: `".to_vec();
                e.extend_from_slice(&origs);
                e.push(b'\'');
                self.ms.error(0, e);
                return None;
            }
            if c != b'\\' {
                m.value[p] = c;
                p += 1;
                continue;
            }
            let c = at(s);
            s += 1;
            match c {
                0 => {
                    s -= 1;
                    m.value[p] = 0;
                    return self.getstr_out(m, p, s);
                }
                b'a' => {
                    m.value[p] = 0x07;
                    p += 1;
                }
                b'b' => {
                    m.value[p] = 0x08;
                    p += 1;
                }
                b'f' => {
                    m.value[p] = 0x0c;
                    p += 1;
                }
                b'n' => {
                    m.value[p] = b'\n';
                    p += 1;
                }
                b'r' => {
                    m.value[p] = b'\r';
                    p += 1;
                }
                b't' => {
                    m.value[p] = b'\t';
                    p += 1;
                }
                b'v' => {
                    m.value[p] = 0x0b;
                    p += 1;
                }
                b'0'..=b'7' => {
                    let mut val = (c - b'0') as u32;
                    let c2 = at(s);
                    s += 1;
                    if (b'0'..=b'7').contains(&c2) {
                        val = (val << 3) | (c2 - b'0') as u32;
                        let c3 = at(s);
                        s += 1;
                        if (b'0'..=b'7').contains(&c3) {
                            val = (val << 3) | (c3 - b'0') as u32;
                        } else {
                            s -= 1;
                        }
                    } else {
                        s -= 1;
                    }
                    m.value[p] = val as u8;
                    p += 1;
                }
                b'x' => {
                    let mut val = b'x' as u32;
                    let c2 = hextoint(at(s));
                    s += 1;
                    if let Some(h) = c2 {
                        val = h;
                        let c3 = hextoint(at(s));
                        s += 1;
                        if let Some(h2) = c3 {
                            val = (val << 4) + h2;
                        } else {
                            s -= 1;
                        }
                    } else {
                        s -= 1;
                    }
                    m.value[p] = val as u8;
                    p += 1;
                }
                c => {
                    m.value[p] = c;
                    p += 1;
                }
            }
        }
        s -= 1;
        self.getstr_out(m, p, s)
    }

    fn getstr_out(&mut self, m: &mut Magic, p: usize, s: usize) -> Option<usize> {
        m.value[p] = 0;
        m.vallen = p as u8;
        if m.typ == FILE_PSTRING {
            let l = self.ms.pstring_length_size(m);
            if l == FILE_BADSIZE {
                return None;
            }
            m.vallen = m.vallen.wrapping_add(l as u8);
        }
        Some(s)
    }

    fn check_format(&mut self, m: &Magic) -> Result<(), ()> {
        let desc = cstr(&m.desc);
        let Some(pos) = desc.iter().position(|&c| c == b'%') else {
            return Ok(());
        };
        if file_format(m.typ) == FILE_FMT_NONE {
            let mut w = b"No format string for `".to_vec();
            w.extend_from_slice(desc);
            w.extend_from_slice(b"' with description `");
            w.extend_from_slice(file_name(m.typ));
            w.push(b'\'');
            self.magwarn_b(&w);
            return Err(());
        }
        let ptr = &desc[pos + 1..];
        match check_format_type(ptr, m.typ) {
            Err(estr) => {
                let mut w = format!("Printf format is {} for type `", estr).into_bytes();
                w.extend_from_slice(file_name(m.typ));
                w.extend_from_slice(b"' in description `");
                w.extend_from_slice(desc);
                w.push(b'\'');
                self.magwarn_b(&w);
                Err(())
            }
            Ok(end) => {
                if ptr[end..].contains(&b'%') {
                    let mut w = b"Too many format strings (should have at most one) for `".to_vec();
                    w.extend_from_slice(file_name(m.typ));
                    w.extend_from_slice(b"' with description `");
                    w.extend_from_slice(desc);
                    w.push(b'\'');
                    self.magwarn_b(&w);
                    return Err(());
                }
                Ok(())
            }
        }
    }

    fn parse_extra(
        &mut self,
        me: &mut Entry,
        line: &[u8],
        llen: usize,
        field: u8,
        name: &str,
        extra: &[u8],
        nt: bool,
    ) -> Result<(), ()> {
        let m = me.last_mut().unwrap();
        let has_desc = m.desc[0] != 0;
        let buf: &mut [u8] = match field {
            0 => &mut m.mimetype,
            1 => &mut m.apple,
            _ => &mut m.ext,
        };
        let len = buf.len();
        let mut l = Cur { s: line, p: 0 };
        if buf[0] != 0 {
            let shown = if nt { cstr(buf).to_vec() } else { buf.to_vec() };
            let shown = cstr(&shown).to_vec();
            let mut w = format!("Current entry already has a {} type `", name).into_bytes();
            w.extend_from_slice(&shown);
            w.extend_from_slice(b"', new type `");
            w.extend_from_slice(l.rest());
            w.push(b'\'');
            self.magwarn_b(&w);
            return Err(());
        }
        if !has_desc {
            self.magwarn(format!(
                "Current entry does not yet have a description for adding a {} type",
                name
            ));
            return Err(());
        }
        l.eatab();
        let goodchar = |x: u8| x.is_ascii_alphanumeric() || x == 0 || extra.contains(&x);
        let mut i = 0usize;
        let mut tmp = vec![0u8; len];
        while l.c() != 0 && i < llen && i < len && goodchar(l.c()) {
            tmp[i] = l.c();
            i += 1;
            l.p += 1;
        }
        let m = me.last_mut().unwrap();
        let buf: &mut [u8] = match field {
            0 => &mut m.mimetype,
            1 => &mut m.apple,
            _ => &mut m.ext,
        };
        buf[..i].copy_from_slice(&tmp[..i]);
        if i == len && l.c() != 0 {
            if nt {
                buf[len - 1] = 0;
            }
            if self.check() {
                let mut w = format!("{} type `", name).into_bytes();
                w.extend_from_slice(cstr(line));
                w.extend_from_slice(format!("' truncated {}", i).as_bytes());
                self.magwarn_b(&w);
            }
        } else {
            if !c_isspace(l.c()) && !goodchar(l.c()) {
                let mut w = format!("{} type `", name).into_bytes();
                w.extend_from_slice(cstr(line));
                w.extend_from_slice(b"' has bad char '");
                w.push(l.c());
                w.push(b'\'');
                self.magwarn_b(&w);
            }
            if nt && i < len {
                buf[i] = 0;
            }
        }
        if i > 0 {
            return Ok(());
        }
        let mut e = b"Bad magic entry '".to_vec();
        e.extend_from_slice(cstr(line));
        e.push(b'\'');
        self.ms.magerror(e);
        Err(())
    }

    fn parse_strength(&mut self, me: &mut Entry, line: &[u8]) -> Result<(), ()> {
        let m = &me[0];
        if m.factor_op != FILE_FACTOR_OP_NONE {
            self.magwarn(format!(
                "Current entry already has a strength type: {} {}",
                m.factor_op as char, m.factor
            ));
            return Err(());
        }
        if m.typ == FILE_NAME {
            let mut w = printable(self.ms.flags, &m.value, 512);
            w.extend_from_slice(b": Strength setting is not supported in \"name\" magic entries");
            self.magwarn_b(&w);
            return Err(());
        }
        let mut l = Cur { s: line, p: 0 };
        l.eatab();
        let op = match l.c() {
            0 => FILE_FACTOR_OP_NONE,
            c @ (b'+' | b'-' | b'*' | b'/') => {
                l.p += 1;
                c
            }
            c => {
                let mut w = b"Unknown factor op `".to_vec();
                w.push(c);
                w.push(b'\'');
                self.magwarn_b(&w);
                return Err(());
            }
        };
        me[0].factor_op = op;
        l.eatab();
        let (factor, n, _) = c_strtoull(l.rest(), 0);
        let el = l.at(n);
        let fail = if factor > 255 {
            self.magwarn(format!("Too large factor `{}'", factor));
            true
        } else if el != 0 && !c_isspace(el) {
            let mut w = b"Bad factor `".to_vec();
            w.extend_from_slice(l.rest());
            w.push(b'\'');
            self.magwarn_b(&w);
            true
        } else {
            me[0].factor = factor as u8;
            if me[0].factor == 0 && me[0].factor_op == FILE_FACTOR_OP_DIV {
                self.magwarn(format!(
                    "Cannot have factor op `{}' and factor {}",
                    me[0].factor_op as char, me[0].factor
                ));
                true
            } else {
                false
            }
        };
        if fail {
            me[0].factor_op = FILE_FACTOR_OP_NONE;
            me[0].factor = 0;
            return Err(());
        }
        Ok(())
    }

    /// load_1()
    fn load_1(&mut self, fname: &[u8], errs: &mut i32, mset: &mut [Vec<Entry>; 2]) {
        self.ms.file = fname.to_vec();
        let Some(data) = self.host.read_file(fname) else {
            if self.host.stat(fname).is_some() {
                let mut e = b"cannot read magic file `".to_vec();
                e.extend_from_slice(fname);
                e.push(b'\'');
                self.ms.error(0, e);
            }
            *errs += 1;
            return;
        };
        let mut me: Option<Entry> = None;
        let mut lineno = 0usize;
        let mut pos = 0usize;
        self.ms.line = 1;
        while pos < data.len() {
            // php_stream_get_line(stream, buffer, BUFSIZ, &len)
            let maxlen = 8191;
            let end = match data[pos..].iter().take(maxlen).position(|&c| c == b'\n') {
                Some(n) => pos + n + 1,
                None => (pos + maxlen).min(data.len()),
            };
            let mut line = data[pos..end].to_vec();
            pos = end;
            let len = line.len();
            if self.ms.magwarn >= self.ms.magwarn_max {
                break;
            }
            if len == 0 {
                self.ms.line += 1;
                continue;
            }
            if line[len - 1] == b'\n' {
                lineno += 1;
                line[len - 1] = 0;
            }
            line.push(0);
            match line[0] {
                0 | b'#' => {}
                b'!' if line[1] == b':' => {
                    let bang: [(&[u8], u8); 4] =
                        [(b"mime", 0), (b"apple", 1), (b"ext", 2), (b"strength", 3)];
                    let found = bang
                        .iter()
                        .find(|(n, _)| len.wrapping_sub(2) > n.len() && line[2..].starts_with(n));
                    match found {
                        None => {
                            let mut e = b"Unknown !: entry `".to_vec();
                            e.extend_from_slice(cstr(&line));
                            e.push(b'\'');
                            self.ms.error(0, e);
                            *errs += 1;
                        }
                        Some(&(n, which)) => match me.as_mut() {
                            None => {
                                let mut e = b"No current entry for :!".to_vec();
                                e.extend_from_slice(n);
                                e.extend_from_slice(b" type");
                                self.ms.error(0, e);
                                *errs += 1;
                            }
                            Some(e) => {
                                let rest = &line[n.len() + 2..];
                                let rlen = len - n.len() - 2;
                                let r = match which {
                                    0 => self.parse_extra(
                                        e,
                                        rest,
                                        rlen,
                                        0,
                                        "MIME",
                                        b"+-/.$?:{};=",
                                        true,
                                    ),
                                    1 => self
                                        .parse_extra(e, rest, rlen, 1, "APPLE", b"!+-./?", false),
                                    2 => self.parse_extra(
                                        e,
                                        rest,
                                        rlen,
                                        2,
                                        "EXTENSION",
                                        b",!+-/@?_$&~",
                                        false,
                                    ),
                                    _ => self.parse_strength(e, rest),
                                };
                                if r.is_err() {
                                    *errs += 1;
                                }
                            }
                        },
                    }
                }
                _ => loop {
                    match self.parse(&mut me, fname, &line, lineno) {
                        0 => break,
                        1 => {
                            addentry(me.take().unwrap(), mset);
                        }
                        _ => {
                            *errs += 1;
                            break;
                        }
                    }
                },
            }
            self.ms.line += 1;
        }
        if let Some(e) = me {
            addentry(e, mset);
        }
    }
}

fn addentry(e: Entry, mset: &mut [Vec<Entry>; 2]) {
    let i = if e[0].typ == FILE_NAME { 1 } else { 0 };
    mset[i].push(e);
}

fn get_op(c: u8) -> Option<u8> {
    Some(match c {
        b'&' => FILE_OPAND,
        b'|' => FILE_OPOR,
        b'^' => FILE_OPXOR,
        b'+' => FILE_OPADD,
        b'-' => FILE_OPMINUS,
        b'*' => FILE_OPMULTIPLY,
        b'/' => FILE_OPDIVIDE,
        b'%' => FILE_OPMODULO,
        _ => return None,
    })
}

fn get_cond(l: &mut Cur) -> u8 {
    for (name, cond) in [
        (&b"if"[..], COND_IF),
        (&b"elif"[..], COND_ELIF),
        (&b"else"[..], COND_ELSE),
    ] {
        if l.rest().starts_with(name) && c_isspace(l.at(name.len())) {
            l.p += name.len();
            return cond;
        }
    }
    COND_NONE
}

fn eatsize(l: &mut Cur) {
    if l.c().eq_ignore_ascii_case(&b'u') {
        l.p += 1;
    }
    if matches!(l.c().to_ascii_lowercase(), b'l' | b's' | b'h' | b'b' | b'c') {
        l.p += 1;
    }
}

fn hextoint(c: u8) -> Option<u32> {
    (c as char).to_digit(16)
}

/// check_format_type(): Ok(index after the conversion)
fn check_format_type(s: &[u8], typ: u8) -> Result<usize, &'static str> {
    let at = |i: usize| *s.get(i).unwrap_or(&0);
    let mut p = 0usize;
    if at(0) == 0 {
        return Err("missing format spec");
    }
    let checklen = |p: &mut usize| -> Result<(), &'static str> {
        let mut len = 0usize;
        let mut cnt = 0usize;
        while at(*p).is_ascii_digit() {
            len = len * 10 + (at(*p) - b'0') as usize;
            *p += 1;
            cnt += 1;
        }
        if cnt > 5 || len > 1024 {
            return Err("too long");
        }
        Ok(())
    };
    match file_format(typ) {
        f @ (FILE_FMT_QUAD | FILE_FMT_NUM) => {
            let quad = f == FILE_FMT_QUAD;
            let h = if quad {
                0
            } else {
                match typ {
                    FILE_BYTE => 2,
                    FILE_SHORT | FILE_BESHORT | FILE_LESHORT => 1,
                    _ => 0,
                }
            };
            while at(p) != 0 && b"-.#".contains(&at(p)) {
                p += 1;
            }
            checklen(&mut p)?;
            if at(p) == b'.' {
                p += 1;
            }
            checklen(&mut p)?;
            if quad {
                if at(p) != b'l' {
                    return Err("not valid");
                }
                p += 1;
                if at(p) != b'l' {
                    return Err("not valid");
                }
                p += 1;
            }
            let c = at(p);
            p += 1;
            match c {
                b'c' => {
                    if h == 2 {
                        Ok(p)
                    } else {
                        Err("not valid")
                    }
                }
                b'i' | b'd' | b'u' | b'o' | b'x' | b'X' => Ok(p),
                _ => Err("not valid"),
            }
        }
        FILE_FMT_FLOAT | FILE_FMT_DOUBLE => {
            if at(p) == b'-' {
                p += 1;
            }
            if at(p) == b'.' {
                p += 1;
            }
            checklen(&mut p)?;
            if at(p) == b'.' {
                p += 1;
            }
            checklen(&mut p)?;
            let c = at(p);
            p += 1;
            match c {
                b'e' | b'E' | b'f' | b'F' | b'g' | b'G' => Ok(p),
                _ => Err("not valid"),
            }
        }
        FILE_FMT_STR => {
            if at(p) == b'-' {
                p += 1;
            }
            while at(p).is_ascii_digit() {
                p += 1;
            }
            if at(p) == b'.' {
                p += 1;
                while at(p).is_ascii_digit() {
                    p += 1;
                }
            }
            let c = at(p);
            p += 1;
            if c == b's' { Ok(p) } else { Err("not valid") }
        }
        _ => Err("not valid"),
    }
}

/// strtod(): (value, consumed, ok (no ERANGE))
pub fn c_strtod(s: &[u8]) -> (f64, usize, bool) {
    let mut i = 0;
    while i < s.len() && c_isspace(s[i]) {
        i += 1;
    }
    let start = i;
    let mut j = i;
    if j < s.len() && (s[j] == b'+' || s[j] == b'-') {
        j += 1;
    }
    let lower: Vec<u8> = s[j..]
        .iter()
        .take(8)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if lower.starts_with(b"inf") || lower.starts_with(b"nan") {
        let n = if lower.starts_with(b"infinity") { 8 } else { 3 };
        let neg = s.get(start) == Some(&b'-');
        let v = if lower.starts_with(b"nan") {
            f64::NAN
        } else if neg {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
        return (v, j + n, true);
    }
    let mut k = j;
    let mut digits = 0;
    while k < s.len() && s[k].is_ascii_digit() {
        k += 1;
        digits += 1;
    }
    if k < s.len() && s[k] == b'.' {
        k += 1;
        while k < s.len() && s[k].is_ascii_digit() {
            k += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return (0.0, 0, true);
    }
    if k < s.len() && (s[k] | 0x20) == b'e' {
        let mut e = k + 1;
        if e < s.len() && (s[e] == b'+' || s[e] == b'-') {
            e += 1;
        }
        if e < s.len() && s[e].is_ascii_digit() {
            while e < s.len() && s[e].is_ascii_digit() {
                e += 1;
            }
            k = e;
        }
    }
    let txt = std::str::from_utf8(&s[start..k]).unwrap_or("0");
    let v: f64 = txt.parse().unwrap_or(0.0);
    (v, k, v.is_finite())
}

/// file_parse_guid(): the struct guid bytes
fn parse_guid(s: &[u8]) -> Option<[u8; 16]> {
    let mut p = 0usize;
    let hex = |p: &mut usize, n: usize| -> Option<u32> {
        while *p < s.len() && c_isspace(s[*p]) {
            *p += 1;
        }
        let mut v = 0u32;
        let mut k = 0;
        while k < n && *p < s.len() && (s[*p] as char).is_ascii_hexdigit() {
            v = v * 16 + (s[*p] as char).to_digit(16).unwrap();
            *p += 1;
            k += 1;
        }
        if k == 0 { None } else { Some(v) }
    };
    let mut g = [0u8; 16];
    let d1 = hex(&mut p, 8)?;
    if s.get(p) != Some(&b'-') {
        return None;
    }
    p += 1;
    let d2 = hex(&mut p, 4)?;
    if s.get(p) != Some(&b'-') {
        return None;
    }
    p += 1;
    let d3 = hex(&mut p, 4)?;
    if s.get(p) != Some(&b'-') {
        return None;
    }
    p += 1;
    g[0..4].copy_from_slice(&d1.to_le_bytes());
    g[4..6].copy_from_slice(&(d2 as u16).to_le_bytes());
    g[6..8].copy_from_slice(&(d3 as u16).to_le_bytes());
    for (i, b) in g[8..16].iter_mut().enumerate() {
        if i == 2 {
            if s.get(p) != Some(&b'-') {
                return None;
            }
            p += 1;
        }
        *b = hex(&mut p, 2)? as u8;
    }
    Some(g)
}

/// convert_libmagic_pattern()
pub fn convert_libmagic_pattern(val: &[u8], caseless: bool, multiline: bool) -> Vec<u8> {
    let mut t = vec![b'~'];
    for &c in val {
        match c {
            b'~' => t.extend_from_slice(b"\\~"),
            0 => t.extend_from_slice(b"\\x00"),
            c => t.push(c),
        }
    }
    t.push(b'~');
    if caseless {
        t.push(b'i');
    }
    if multiline {
        t.push(b'm');
    }
    t
}

/// file_printable()
pub fn printable(flags: i32, s: &[u8], bufsiz: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let emax = bufsiz - 1;
    for &c in s {
        if out.len() >= emax || c == 0 {
            break;
        }
        if flags & MAGIC_RAW != 0 || c_isprint(c) {
            out.push(c);
            continue;
        }
        if out.len() + 3 >= emax {
            break;
        }
        out.push(b'\\');
        out.push(b'0' + ((c >> 6) & 7));
        out.push(b'0' + ((c >> 3) & 7));
        out.push(b'0' + (c & 7));
    }
    out
}

// ---------------------------------------------------------------------------
// loading

/// apprentice_load(): the two sets of a text magic file or directory
fn apprentice_load(ms: &mut MagicSet, host: &mut dyn Host, fname: &[u8]) -> Option<[MList; 2]> {
    let mut errs = 0;
    let mut mset: [Vec<Entry>; 2] = [Vec::new(), Vec::new()];
    ms.flags |= MAGIC_CHECK;
    let is_dir = matches!(host.stat(fname), Some((mode, _)) if mode & S_IFMT == S_IFDIR);
    {
        let mut ld = Loader { ms, host };
        if is_dir {
            match ld.host.readdir(fname) {
                None => errs += 1,
                Some(names) => {
                    let mut files: Vec<Vec<u8>> = Vec::new();
                    for n in names {
                        let mut p = fname.to_vec();
                        p.push(b'/');
                        p.extend_from_slice(&n);
                        match ld.host.stat(&p) {
                            Some((mode, _)) if mode & S_IFMT == S_IFREG => files.push(p),
                            _ => {}
                        }
                    }
                    files.sort();
                    for f in files {
                        ld.load_1(&f, &mut errs, &mut mset);
                    }
                }
            }
        } else {
            ld.load_1(fname, &mut errs, &mut mset);
        }
    }
    if errs != 0 {
        return None;
    }
    let mut out: Vec<MList> = Vec::new();
    for set in mset.iter_mut() {
        for e in set.iter_mut() {
            set_test_type(&mut e[0]);
        }
        let warned = RefCell::new(Vec::<String>::new());
        let mut keyed: Vec<(usize, Vec<u8>, Entry)> = set
            .drain(..)
            .map(|e| {
                let s = magic_strength(&e[0], &mut |w| warned.borrow_mut().push(w));
                let mut m = e[0].clone();
                m.lineno = 0;
                (s, m.to_bytes(), e)
            })
            .collect();
        for w in warned.into_inner() {
            host.warn(w.as_bytes());
        }
        keyed.sort_by(|a, b| {
            if a.0 == b.0 {
                b.1.cmp(&a.1)
            } else {
                b.0.cmp(&a.0)
            }
        });
        let entries: Vec<Entry> = keyed.into_iter().map(|k| k.2).collect();
        // set_last_default()
        for i in 0..entries.len() {
            if entries[i][0].typ == FILE_DEFAULT {
                if i + 1 != entries.len() {
                    ms.line = entries[i + 1][0].lineno as usize;
                    host.warn(b"level 0 \"default\" did not sort last");
                }
                break;
            }
        }
        out.push(Rc::new(entries.into_iter().flatten().collect()));
    }
    let b = out.pop().unwrap();
    let a = out.pop().unwrap();
    Some([a, b])
}

thread_local! {
    static BUILTIN: RefCell<Option<[MList; 2]>> = const { RefCell::new(None) };
}

static MAGIC_DB: &[u8] = include_bytes!("magic.mgc.z");

fn inflate_db() -> Vec<u8> {
    use goro_zlib::{Inflate, Io, Z_FINISH};
    let mut out = vec![0u8; 10355472];
    let mut inf = Inflate::new(15).expect("inflate");
    let mut io = Io::new(MAGIC_DB, &mut out);
    inf.inflate(&mut io, Z_FINISH);
    out
}

/// Parse the entries of a compiled database: Err(message)
fn parse_compiled(data: &[u8], dbname: &[u8]) -> Result<[MList; 2], Vec<u8>> {
    let w0 = u32::from_le_bytes(data[0..4].try_into().unwrap());
    let swap = if w0 == MAGICNO {
        false
    } else if w0.swap_bytes() == MAGICNO {
        true
    } else {
        let mut e = b"bad magic in `".to_vec();
        e.extend_from_slice(dbname);
        e.push(b'\'');
        return Err(e);
    };
    let rd = |i: usize| {
        let v = u32::from_le_bytes(data[i * 4..i * 4 + 4].try_into().unwrap());
        if swap { v.swap_bytes() } else { v }
    };
    let version = rd(1);
    if version != VERSIONNO {
        let mut e = format!(
            "File {} supports only version {} magic files. `",
            MAGIC_VERSION, VERSIONNO
        )
        .into_bytes();
        e.extend_from_slice(dbname);
        e.extend_from_slice(format!("' is version {}", version).as_bytes());
        return Err(e);
    }
    let n0 = rd(2) as usize;
    let n1 = rd(3) as usize;
    let entries = data.len() / FILE_MAGICSIZE;
    if entries != n0 + n1 + 1 {
        let mut e = b"Inconsistent entries in `".to_vec();
        e.extend_from_slice(dbname);
        e.extend_from_slice(format!("' {} != {}", entries, n0 + n1 + 1).as_bytes());
        return Err(e);
    }
    let get = |i: usize| {
        let o = (i + 1) * FILE_MAGICSIZE;
        Magic::from_bytes(&data[o..o + FILE_MAGICSIZE], swap)
    };
    let a: Vec<Magic> = (0..n0).map(get).collect();
    let b: Vec<Magic> = (n0..n0 + n1).map(get).collect();
    Ok([Rc::new(a), Rc::new(b)])
}

/// The built-in database (php_magic_database), decoded once.
pub fn builtin() -> [MList; 2] {
    BUILTIN.with(|b| {
        let mut b = b.borrow_mut();
        if b.is_none() {
            let data = inflate_db();
            *b = Some(parse_compiled(&data, b"").expect("built-in magic database"));
        }
        b.clone().unwrap()
    })
}

/// mkdbname()
fn mkdbname(ms: &mut MagicSet, host: &mut dyn Host, fname: &[u8]) -> Vec<u8> {
    let base = fname.strip_suffix(b".mgc").unwrap_or(fname);
    if ms.flags & MAGIC_MIME != 0 {
        let mut b = base.to_vec();
        b.extend_from_slice(b".mime.mgc");
        if host.stat(&b).is_some() {
            ms.flags &= MAGIC_MIME_TYPE;
            return b;
        }
    }
    let mut b = base.to_vec();
    b.extend_from_slice(b".mgc");
    if fname.windows(5).any(|w| w == b".mime") {
        ms.flags &= MAGIC_MIME_TYPE;
    }
    b
}

/// apprentice_map()
fn apprentice_map(
    ms: &mut MagicSet,
    host: &mut dyn Host,
    fname: Option<&[u8]>,
) -> Option<[MList; 2]> {
    let Some(fname) = fname else {
        return Some(builtin());
    };
    let dbname = mkdbname(ms, host, fname);
    let data = host.read_file(fname)?;
    if data.len() < 8 {
        let mut e = b"file `".to_vec();
        e.extend_from_slice(&dbname);
        e.extend_from_slice(b"' is too small");
        ms.error(0, e);
        return None;
    }
    let w0 = u32::from_le_bytes(data[0..4].try_into().unwrap());
    if w0 != MAGICNO && w0.swap_bytes() != MAGICNO {
        let mut e = b"bad magic in `".to_vec();
        e.extend_from_slice(&dbname);
        e.push(b'\'');
        ms.error(0, e);
        return None;
    }
    if data.len() % FILE_MAGICSIZE != 0 {
        let mut e = b"Size of `".to_vec();
        e.extend_from_slice(&dbname);
        e.extend_from_slice(
            format!("' {} is not a multiple of {}", data.len(), FILE_MAGICSIZE).as_bytes(),
        );
        ms.error(0, e);
        return None;
    }
    match parse_compiled(&data, &dbname) {
        Ok(m) => Some(m),
        Err(e) => {
            ms.error(0, e);
            None
        }
    }
}

/// apprentice_1()
fn apprentice_1(ms: &mut MagicSet, host: &mut dyn Host, fname: Option<&[u8]>) -> i32 {
    let map = match apprentice_map(ms, host, fname) {
        Some(m) => m,
        None => {
            let f = fname.unwrap_or_default();
            if ms.flags & MAGIC_CHECK != 0 {
                let mut w = b"using regular magic file `".to_vec();
                w.extend_from_slice(f);
                w.push(b'\'');
                host.warn(&w);
            }
            match apprentice_load(ms, host, f) {
                Some(m) => m,
                None => return -1,
            }
        }
    };
    let [a, b] = map;
    ms.mlist[0].get_or_insert_with(Vec::new).push(a);
    ms.mlist[1].get_or_insert_with(Vec::new).push(b);
    0
}

/// file_apprentice(ms, fn, FILE_LOAD)
pub fn file_apprentice(ms: &mut MagicSet, host: &mut dyn Host, fname: Option<&[u8]>) -> i32 {
    let _ = ms.reset(false);
    ms.mlist = [Some(Vec::new()), Some(Vec::new())];
    let Some(fname) = fname else {
        return apprentice_1(ms, host, None);
    };
    let mut errs = -1;
    for part in fname.split(|&c| c == b':') {
        if part.is_empty() {
            break;
        }
        let r = apprentice_1(ms, host, Some(part));
        errs = errs.max(r);
    }
    if errs == -1 {
        ms.mlist = [None, None];
        ms.error(0, b"could not find any valid magic files!".to_vec());
        return -1;
    }
    0
}

impl MagicSet {
    /// file_magicfind(): the named entry of set 1 and its continuations
    pub fn magicfind(&self, name: &[u8]) -> Option<(MList, usize, usize)> {
        for ml in self.mlist[1].as_ref()? {
            for i in 0..ml.len() {
                if ml[i].typ != FILE_NAME {
                    continue;
                }
                if cstr(&ml[i].value) == name {
                    let mut j = i + 1;
                    while j < ml.len() && ml[j].cont_level != 0 {
                        j += 1;
                    }
                    return Some((ml.clone(), i, j - i));
                }
            }
        }
        None
    }

    /// file_signextend()
    pub fn signextend(&mut self, m: &Magic, v: u64, host: &mut dyn Host) -> u64 {
        if m.flag & UNSIGNED != 0 {
            return v;
        }
        match m.typ {
            FILE_BYTE => v as i8 as i64 as u64,
            FILE_SHORT | FILE_BESHORT | FILE_LESHORT => v as i16 as i64 as u64,
            FILE_DATE | FILE_BEDATE | FILE_LEDATE | FILE_MEDATE | FILE_LDATE | FILE_BELDATE
            | FILE_LELDATE | FILE_MELDATE | FILE_LONG | FILE_BELONG | FILE_LELONG | FILE_MELONG
            | FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT | FILE_MSDOSDATE | FILE_BEMSDOSDATE
            | FILE_LEMSDOSDATE | FILE_MSDOSTIME | FILE_BEMSDOSTIME | FILE_LEMSDOSTIME => {
                v as i32 as i64 as u64
            }
            FILE_QUAD | FILE_BEQUAD | FILE_LEQUAD | FILE_QDATE | FILE_QLDATE | FILE_QWDATE
            | FILE_BEQDATE | FILE_BEQLDATE | FILE_BEQWDATE | FILE_LEQDATE | FILE_LEQLDATE
            | FILE_LEQWDATE | FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE | FILE_OFFSET
            | FILE_BEVARINT | FILE_LEVARINT => v,
            FILE_STRING | FILE_PSTRING | FILE_BESTRING16 | FILE_LESTRING16 | FILE_REGEX
            | FILE_SEARCH | FILE_DEFAULT | FILE_INDIRECT | FILE_NAME | FILE_USE | FILE_CLEAR
            | FILE_DER | FILE_GUID | FILE_OCTAL => v,
            t => {
                if self.flags & MAGIC_CHECK != 0 {
                    host.warn(format!("cannot happen: m->type={}\n", t).as_bytes());
                }
                FILE_BADSIZE as u64
            }
        }
    }

    /// file_pstring_length_size()
    pub fn pstring_length_size(&mut self, m: &Magic) -> usize {
        match m.str_flags() & PSTRING_LEN {
            PSTRING_1_LE => 1,
            PSTRING_2_LE | PSTRING_2_BE => 2,
            PSTRING_4_LE | PSTRING_4_BE => 4,
            f => {
                self.error(
                    0,
                    format!("corrupt magic file (bad pascal string length {})", f).into_bytes(),
                );
                FILE_BADSIZE
            }
        }
    }

    /// file_pstring_get_length()
    pub fn pstring_get_length(&mut self, m: &Magic, s: &[u8]) -> usize {
        let b = |i: usize| *s.get(i).unwrap_or(&0) as usize;
        let mut len = match m.str_flags() & PSTRING_LEN {
            PSTRING_1_LE => b(0),
            PSTRING_2_LE => (b(1) << 8) | b(0),
            PSTRING_2_BE => (b(0) << 8) | b(1),
            PSTRING_4_LE => (b(3) << 24) | (b(2) << 16) | (b(1) << 8) | b(0),
            PSTRING_4_BE => (b(0) << 24) | (b(1) << 16) | (b(2) << 8) | b(3),
            f => {
                self.error(
                    0,
                    format!("corrupt magic file (bad pascal string length {})", f).into_bytes(),
                );
                return FILE_BADSIZE;
            }
        };
        if m.str_flags() & PSTRING_LENGTH_INCLUDES_ITSELF != 0 {
            let l = self.pstring_length_size(m);
            if l == FILE_BADSIZE {
                return l;
            }
            len = len.wrapping_sub(l);
        }
        len
    }
}
