//! Parser for the magic(5) text format (file(1)'s rule language).
//!
//! Written from the magic(5) manual page (offset / type / test / message
//! fields, continuation levels, indirect and relative offsets, string,
//! search and regex flags, `!:mime`, `!:apple`, `!:ext`, `!:strength`,
//! `name`/`use`, `default`/`clear`). Diagnostics and their wording are the
//! ones PHP 8.5's finfo_open() prints for malformed files (tests
//! ext/fileinfo/tests/bug61964.phpt, bug77961.phpt and observation).

use std::collections::HashMap;
use std::rc::Rc;

use goro_regex::{Flags as RxFlags, Regex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endian {
    Little,
    Big,
    /// PDP-11 order: 16-bit halves big-endian-first, each half little-endian.
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateStyle {
    /// Seconds since the epoch, printed in UTC.
    Unix,
    /// Seconds since the epoch, printed in local time.
    UnixLocal,
    /// Windows FILETIME (100 ns ticks since 1601).
    Windows,
    MsdosDate,
    MsdosTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Int(u8),
    Float(u8),
    Date(u8, DateStyle),
    /// 32-bit ID3 syncsafe length (7 bits per byte).
    Id3,
    Str,
    /// Pascal string: length size, length byte order, length includes itself.
    PStr(u8, Endian, bool),
    Str16,
    Search,
    Regex,
    Octal,
    Indirect,
    Name,
    Use,
    Default,
    Clear,
    Der,
    Guid,
    Offset,
}

impl Kind {
    pub fn is_numeric(self) -> bool {
        matches!(
            self,
            Kind::Int(_) | Kind::Float(_) | Kind::Date(..) | Kind::Id3 | Kind::Offset
        )
    }
    pub fn is_string(self) -> bool {
        matches!(
            self,
            Kind::Str | Kind::PStr(..) | Kind::Str16 | Kind::Search | Kind::Regex | Kind::Octal
        )
    }
}

// String flags (magic(5): string type modifiers).
pub const SF_W: u32 = 1; // compact whitespace
pub const SF_SW: u32 = 2; // optional blanks (w)
pub const SF_LC: u32 = 4; // c: lower case in magic matches both
pub const SF_UC: u32 = 8; // C: upper case in magic matches both
pub const SF_TEXT: u32 = 16; // t
pub const SF_BIN: u32 = 32; // b
pub const SF_TRIM: u32 = 64; // T
pub const SF_FULLWORD: u32 = 128; // f
pub const SF_START: u32 = 256; // s: offset is the start of the match
pub const SF_LINES: u32 = 512; // regex l
pub const SF_REL: u32 = 1024; // indirect r

/// A value read from the file at an indirect offset.
#[derive(Clone, Debug)]
pub struct Indirect {
    /// The read position is relative to the last up-level match.
    pub rel: bool,
    pub base: i64,
    pub size: u8,
    pub endian: Endian,
    pub signed: bool,
    /// 'i'/'I' ID3 length, 'o' octal text.
    pub special: u8,
    pub op: u8,
    pub operand: i64,
    /// The operand is itself read from the file at `operand`.
    pub operand_ind: bool,
}

#[derive(Clone, Debug)]
pub struct Offset {
    /// `&`: relative to the end of the last up-level match.
    pub rel: bool,
    pub base: i64,
    pub ind: Option<Box<Indirect>>,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub level: usize,
    pub off: Offset,
    pub kind: Kind,
    pub type_name: String,
    pub endian: Endian,
    pub unsigned: bool,
    /// Arithmetic applied to the value read (`&`, `|`, `^`, `+`, `-`, `*`,
    /// `/`, `%`) and its operand.
    pub op: u8,
    pub operand: u64,
    pub sflags: u32,
    /// Search range, regex range, string width, or pstring length size.
    pub range: u64,
    /// `=`, `!`, `<`, `>`, `&`, `^`, or `x` (any).
    pub reln: u8,
    pub num: u64,
    pub fnum: f64,
    pub sval: Vec<u8>,
    pub regex: Option<Rc<Regex>>,
    pub desc: Vec<u8>,
    pub mime: Option<Vec<u8>>,
    pub apple: Option<Vec<u8>>,
    pub ext: Option<Vec<u8>>,
    /// Swap the byte order of the named rule (`use ^name`).
    pub flip: bool,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub lines: Vec<Line>,
    pub strength: i64,
    pub text: bool,
    pub strength_adj: Option<(u8, i64)>,
}

#[derive(Default, Debug)]
pub struct Magic {
    pub entries: Vec<Entry>,
    pub names: HashMap<Vec<u8>, Entry>,
}

#[derive(Debug)]
pub struct ParseError {
    /// Warnings PHP prints (some failures are silent).
    pub warnings: Vec<String>,
}

fn is_ws(c: u8) -> bool {
    c == b' ' || c == b'\t'
}

fn skip_ws(s: &[u8], mut i: usize) -> usize {
    while i < s.len() && is_ws(s[i]) {
        i += 1;
    }
    i
}

/// strtoull/strtoll with base detection; returns (value, bytes used).
pub fn parse_num(s: &[u8]) -> Option<(i64, usize)> {
    let mut i = 0;
    let mut neg = false;
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        neg = s[i] == b'-';
        i += 1;
    }
    let start = i;
    let (radix, mut j) = if s.len() > i + 1 && s[i] == b'0' && (s[i + 1] | 0x20) == b'x' {
        (16, i + 2)
    } else if i < s.len() && s[i] == b'0' {
        (8, i)
    } else {
        (10, i)
    };
    let mut v: u64 = 0;
    let dstart = j;
    while j < s.len() {
        let d = match s[j] {
            c @ b'0'..=b'9' => (c - b'0') as u32,
            c @ b'a'..=b'f' => (c - b'a' + 10) as u32,
            c @ b'A'..=b'F' => (c - b'A' + 10) as u32,
            _ => break,
        };
        if d >= radix {
            break;
        }
        v = v.wrapping_mul(radix as u64).wrapping_add(d as u64);
        j += 1;
    }
    if j == dstart {
        if radix == 16 {
            // "0x" with no digits: strtoull reads the 0.
            return Some((0, start + 1));
        }
        return None;
    }
    let v = if neg { (v as i64).wrapping_neg() } else { v as i64 };
    Some((v, j))
}

fn parse_float(s: &[u8]) -> Option<(f64, usize)> {
    let mut j = 0;
    if j < s.len() && (s[j] == b'-' || s[j] == b'+') {
        j += 1;
    }
    let st = j;
    while j < s.len() && (s[j].is_ascii_digit() || s[j] == b'.') {
        j += 1;
    }
    if j < s.len() && (s[j] | 0x20) == b'e' {
        let mut k = j + 1;
        if k < s.len() && (s[k] == b'-' || s[k] == b'+') {
            k += 1;
        }
        if k < s.len() && s[k].is_ascii_digit() {
            while k < s.len() && s[k].is_ascii_digit() {
                k += 1;
            }
            j = k;
        }
    }
    if j == st {
        return None;
    }
    std::str::from_utf8(&s[..j])
        .ok()?
        .parse::<f64>()
        .ok()
        .map(|f| (f, j))
}

/// Decodes a magic string field (C escapes, `\ ` for a blank) up to the
/// first unescaped blank. Returns the bytes and the length consumed.
pub fn parse_escaped(s: &[u8]) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if is_ws(c) {
            break;
        }
        if c != b'\\' {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        if i >= s.len() {
            out.push(b'\\');
            break;
        }
        let e = s[i];
        i += 1;
        match e {
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'v' => out.push(11),
            b'a' => out.push(7),
            b'0'..=b'7' => {
                let mut v = (e - b'0') as u32;
                let mut n = 1;
                while n < 3 && i < s.len() && (b'0'..=b'7').contains(&s[i]) {
                    v = v * 8 + (s[i] - b'0') as u32;
                    i += 1;
                    n += 1;
                }
                out.push(v as u8);
            }
            b'x' => {
                let mut v = 0u32;
                let mut n = 0;
                while n < 2 && i < s.len() && s[i].is_ascii_hexdigit() {
                    v = v * 16 + (s[i] as char).to_digit(16).unwrap();
                    i += 1;
                    n += 1;
                }
                if n == 0 {
                    out.push(b'x');
                } else {
                    out.push(v as u8);
                }
            }
            other => out.push(other),
        }
    }
    (out, i)
}

/// The raw (still escaped) regex source up to the first unescaped blank.
fn raw_token(s: &[u8]) -> usize {
    let mut i = 0;
    while i < s.len() && !is_ws(s[i]) {
        if s[i] == b'\\' && i + 1 < s.len() {
            i += 2;
        } else {
            i += 1;
        }
    }
    i.min(s.len())
}

/// The pattern given to the PCRE-compatible engine (PHP's libmagic
/// matches regex rules with PCRE): the field after magic escape decoding
/// (`\\.` is an escaped dot, `\^` an anchor), with unprintable bytes
/// written as `\x{..}`.
fn regex_source(dec: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(dec.len());
    for &c in dec {
        if (0x20..0x7f).contains(&c) {
            out.push(c);
        } else {
            out.extend_from_slice(format!("\\x{{{:x}}}", c).as_bytes());
        }
    }
    out
}

fn type_table(name: &str) -> Option<(Kind, Endian)> {
    use DateStyle::*;
    use Endian::*;
    let le = Little;
    Some(match name {
        "byte" | "dC" | "d1" => (Kind::Int(1), le),
        "short" | "dS" | "d2" => (Kind::Int(2), le),
        "long" | "dI" | "dL" | "d4" => (Kind::Int(4), le),
        "quad" | "dQ" | "d8" => (Kind::Int(8), le),
        "uC" | "u1" => (Kind::Int(1), le),
        "uS" | "u2" => (Kind::Int(2), le),
        "uI" | "uL" | "u4" => (Kind::Int(4), le),
        "uQ" | "u8" => (Kind::Int(8), le),
        "float" => (Kind::Float(4), le),
        "double" => (Kind::Float(8), le),
        "string" => (Kind::Str, le),
        "pstring" => (Kind::PStr(1, Little, false), le),
        "date" => (Kind::Date(4, Unix), le),
        "qdate" => (Kind::Date(8, Unix), le),
        "ldate" => (Kind::Date(4, UnixLocal), le),
        "qldate" => (Kind::Date(8, UnixLocal), le),
        "qwdate" => (Kind::Date(8, Windows), le),
        "msdosdate" => (Kind::Date(2, MsdosDate), le),
        "msdostime" => (Kind::Date(2, MsdosTime), le),
        "beid3" => (Kind::Id3, Big),
        "leid3" => (Kind::Id3, le),
        "beshort" => (Kind::Int(2), Big),
        "belong" => (Kind::Int(4), Big),
        "bequad" => (Kind::Int(8), Big),
        "befloat" => (Kind::Float(4), Big),
        "bedouble" => (Kind::Float(8), Big),
        "bedate" => (Kind::Date(4, Unix), Big),
        "beqdate" => (Kind::Date(8, Unix), Big),
        "beldate" => (Kind::Date(4, UnixLocal), Big),
        "beqldate" => (Kind::Date(8, UnixLocal), Big),
        "beqwdate" => (Kind::Date(8, Windows), Big),
        "bemsdosdate" => (Kind::Date(2, MsdosDate), Big),
        "bemsdostime" => (Kind::Date(2, MsdosTime), Big),
        "bestring16" => (Kind::Str16, Big),
        "leshort" => (Kind::Int(2), le),
        "lelong" => (Kind::Int(4), le),
        "lequad" => (Kind::Int(8), le),
        "lefloat" => (Kind::Float(4), le),
        "ledouble" => (Kind::Float(8), le),
        "ledate" => (Kind::Date(4, Unix), le),
        "leqdate" => (Kind::Date(8, Unix), le),
        "leldate" => (Kind::Date(4, UnixLocal), le),
        "leqldate" => (Kind::Date(8, UnixLocal), le),
        "leqwdate" => (Kind::Date(8, Windows), le),
        "lemsdosdate" => (Kind::Date(2, MsdosDate), le),
        "lemsdostime" => (Kind::Date(2, MsdosTime), le),
        "lestring16" => (Kind::Str16, le),
        "melong" => (Kind::Int(4), Middle),
        "medate" => (Kind::Date(4, Unix), Middle),
        "meldate" => (Kind::Date(4, UnixLocal), Middle),
        "indirect" => (Kind::Indirect, le),
        "name" => (Kind::Name, le),
        "use" => (Kind::Use, le),
        "regex" => (Kind::Regex, le),
        "search" => (Kind::Search, le),
        "default" => (Kind::Default, le),
        "clear" => (Kind::Clear, le),
        "der" => (Kind::Der, le),
        "guid" => (Kind::Guid, le),
        "offset" => (Kind::Offset, le),
        "octal" => (Kind::Octal, le),
        _ => return None,
    })
}

struct Parser {
    warnings: Vec<String>,
    failed: bool,
    entries: Vec<Entry>,
    names: HashMap<Vec<u8>, Entry>,
    cur: Option<(Entry, Option<Vec<u8>>)>,
    /// The fixed rule order of goro's own database (no strength sort).
    keep_order: bool,
}

impl Parser {
    fn warn(&mut self, m: String) {
        self.warnings.push(m);
        self.failed = true;
    }

    fn flush(&mut self) {
        if let Some((mut e, name)) = self.cur.take() {
            if let Some(n) = name {
                self.names.insert(n, e);
            } else {
                e.strength = strength(&e);
                self.entries.push(e);
            }
        }
    }

    fn last_line(&mut self) -> Option<&mut Line> {
        self.cur.as_mut().and_then(|(e, _)| e.lines.last_mut())
    }

    fn directive(&mut self, s: &[u8]) {
        let i = s.iter().position(|&c| is_ws(c)).unwrap_or(s.len());
        let key = &s[..i];
        let val = trim(&s[i..]);
        // Value up to the first blank or comment.
        let val_tok: Vec<u8> = {
            let e = val
                .iter()
                .position(|&c| is_ws(c) || c == b'#')
                .unwrap_or(val.len());
            val[..e].to_vec()
        };
        match key {
            b"mime" | b"apple" | b"ext" => {
                if val_tok.is_empty() {
                    self.failed = true;
                    return;
                }
                let Some(l) = self.last_line() else {
                    self.failed = true;
                    return;
                };
                match key {
                    b"mime" => l.mime = Some(val_tok),
                    b"apple" => l.apple = Some(val_tok),
                    _ => l.ext = Some(val_tok),
                }
            }
            b"strength" => {
                let v = trim(val);
                if v.is_empty() || self.cur.is_none() {
                    self.failed = true;
                    return;
                }
                let op = v[0];
                if !matches!(op, b'+' | b'-' | b'*' | b'/') {
                    self.failed = true;
                    return;
                }
                let rest = trim(&v[1..]);
                let Some((n, _)) = parse_num(rest) else {
                    self.failed = true;
                    return;
                };
                if let Some((e, _)) = self.cur.as_mut() {
                    if e.lines.len() == 1 {
                        e.strength_adj = Some((op, n));
                    }
                }
            }
            _ => {
                self.failed = true;
            }
        }
    }

    fn line(&mut self, raw: &[u8]) {
        let mut s = raw;
        while let Some(r) = s.strip_suffix(b"\n") {
            s = r;
        }
        let t = skip_ws(s, 0);
        if t >= s.len() || s[t] == b'#' {
            return;
        }
        if s[t..].starts_with(b"!:") {
            self.directive(&s[t + 2..]);
            return;
        }
        let mut i = 0;
        let mut level = 0;
        while i < s.len() && s[i] == b'>' {
            level += 1;
            i += 1;
        }
        if level > 0 && self.cur.is_none() {
            self.failed = true;
            return;
        }
        let prev_level = self
            .cur
            .as_ref()
            .and_then(|(e, _)| e.lines.last())
            .map(|l| l.level);
        if level > 0
            && let Some(p) = prev_level
            && level > p + 1
        {
            // Reported but not fatal (observed on PHP 8.5).
            self.warnings.push(format!(
                "New continuation level {} is more than one larger than current level {}",
                level, p
            ));
        }
        let rest = &s[i..];
        let Some((off, used)) = parse_offset(rest) else {
            self.warn(format!(
                "offset `{}' invalid",
                String::from_utf8_lossy(rest)
            ));
            return;
        };
        let mut j = skip_ws(rest, used);
        // Type name.
        let tstart = j;
        while j < rest.len() && (rest[j].is_ascii_alphanumeric() || rest[j] == b'_') {
            j += 1;
        }
        let tname = String::from_utf8_lossy(&rest[tstart..j]).into_owned();
        let (mut kind, mut endian, unsigned) = match type_table(&tname) {
            Some((k, e)) => (k, e, false),
            None => match tname.strip_prefix('u').and_then(type_table) {
                Some((k, e)) if k.is_numeric() || k.is_string() => (k, e, true),
                _ => {
                    self.warn(format!(
                        "type `{}' invalid",
                        String::from_utf8_lossy(&rest[tstart..])
                    ));
                    return;
                }
            },
        };
        if let Kind::PStr(_, ref mut e, _) = kind {
            *e = Endian::Little;
        }
        let mut line = Line {
            level,
            off,
            kind,
            type_name: tname.clone(),
            endian,
            unsigned,
            op: 0,
            operand: 0,
            sflags: 0,
            range: 0,
            reln: b'=',
            num: 0,
            fnum: 0.0,
            sval: Vec::new(),
            regex: None,
            desc: Vec::new(),
            mime: None,
            apple: None,
            ext: None,
            flip: false,
        };
        // Modifiers.
        if kind.is_string() || matches!(kind, Kind::Indirect | Kind::Der) {
            while j < rest.len() && rest[j] == b'/' {
                j += 1;
                while j < rest.len() && !is_ws(rest[j]) && rest[j] != b'/' {
                    let c = rest[j];
                    if c.is_ascii_digit() {
                        // Ranges may be written in hex ("search/0x1000").
                        let (v, n) = parse_num(&rest[j..]).unwrap_or((0, 1));
                        line.range = v.max(0) as u64;
                        j += n;
                        continue;
                    }
                    let ok = match (kind, c) {
                        (Kind::Indirect, b'r') => {
                            line.sflags |= SF_REL;
                            true
                        }
                        (Kind::PStr(..), b'B') => {
                            kind = Kind::PStr(1, Endian::Little, pjay(kind));
                            true
                        }
                        (Kind::PStr(..), b'H') => {
                            kind = Kind::PStr(2, Endian::Big, pjay(kind));
                            true
                        }
                        (Kind::PStr(..), b'h') => {
                            kind = Kind::PStr(2, Endian::Little, pjay(kind));
                            true
                        }
                        (Kind::PStr(..), b'L') => {
                            kind = Kind::PStr(4, Endian::Big, pjay(kind));
                            true
                        }
                        (Kind::PStr(..), b'l') => {
                            kind = Kind::PStr(4, Endian::Little, pjay(kind));
                            true
                        }
                        (Kind::PStr(n, e, _), b'J') => {
                            kind = Kind::PStr(n, e, true);
                            true
                        }
                        (Kind::Regex, b'l') => {
                            line.sflags |= SF_LINES;
                            true
                        }
                        (Kind::Regex, b'c') => {
                            line.sflags |= SF_LC;
                            true
                        }
                        (Kind::Regex, b's') | (Kind::Search, b's') => {
                            line.sflags |= SF_START;
                            true
                        }
                        (Kind::Regex, b't') => {
                            line.sflags |= SF_TEXT;
                            true
                        }
                        (Kind::Regex, b'b') => {
                            line.sflags |= SF_BIN;
                            true
                        }
                        (Kind::Regex, _) | (Kind::Indirect, _) | (Kind::Der, _) => false,
                        (_, b'W') => {
                            line.sflags |= SF_W;
                            true
                        }
                        (_, b'w') => {
                            line.sflags |= SF_SW;
                            true
                        }
                        (_, b'c') => {
                            line.sflags |= SF_LC;
                            true
                        }
                        (_, b'C') => {
                            line.sflags |= SF_UC;
                            true
                        }
                        (_, b't') => {
                            line.sflags |= SF_TEXT;
                            true
                        }
                        (_, b'b') => {
                            line.sflags |= SF_BIN;
                            true
                        }
                        (_, b'T') => {
                            line.sflags |= SF_TRIM;
                            true
                        }
                        (_, b'f') => {
                            line.sflags |= SF_FULLWORD;
                            true
                        }
                        (Kind::Search, _) | (Kind::Str, _) | (Kind::Str16, _) => false,
                        _ => false,
                    };
                    if !ok {
                        self.warn(format!("string modifier `{}' invalid", c as char));
                        return;
                    }
                    j += 1;
                }
            }
            line.kind = kind;
        } else if kind.is_numeric() && j < rest.len() {
            let c = rest[j];
            if matches!(c, b'&' | b'|' | b'^' | b'+' | b'-' | b'*' | b'/' | b'%') {
                let Some((v, n)) = parse_num(&rest[j + 1..]) else {
                    self.warn(format!(
                        "Unparsable number `{}'",
                        String::from_utf8_lossy(&rest[j + 1..])
                    ));
                    return;
                };
                line.op = c;
                line.operand = v as u64;
                j += 1 + n;
            }
        }
        if let Kind::PStr(..) = kind {
            endian = Endian::Little;
        }
        line.endian = endian;
        j = skip_ws(rest, j);
        let t = &rest[j..];
        // The test.
        let mut k = 0;
        let is_x = |t: &[u8], k: usize| t.get(k) == Some(&b'x') && t.get(k + 1).is_none_or(|&c| is_ws(c));
        match kind {
            Kind::Name | Kind::Use => {
                let (mut v, n) = parse_escaped(t);
                if kind == Kind::Use && v.first() == Some(&b'^') {
                    v.remove(0);
                    line.flip = true;
                }
                line.sval = v;
                line.reln = b'=';
                k = n;
            }
            Kind::Indirect | Kind::Default | Kind::Clear | Kind::Der => {
                if kind == Kind::Der {
                    // der tests name an ASN.1 element type (seq, set,
                    // obj_id3=..., prt_str=x ...): kept as text.
                    let (v, n) = parse_escaped(t);
                    line.sval = v;
                    line.reln = b'x';
                    k = n;
                } else if is_x(t, 0) || t.is_empty() {
                    line.reln = b'x';
                    k = t.len().min(1);
                } else {
                    self.warn(format!("Expected numeric type got `{}'", tname));
                    return;
                }
            }
            Kind::Guid => {
                if is_x(t, 0) {
                    line.reln = b'x';
                    k = 1;
                } else {
                    let (v, n) = parse_escaped(t);
                    line.sval = v;
                    k = n;
                }
            }
            _ if kind.is_numeric() => {
                if matches!(t.first(), Some(b'=' | b'<' | b'>' | b'!' | b'&' | b'^')) {
                    line.reln = t[0];
                    // Blanks may follow the operator ("< 10", "& 1").
                    k = skip_ws(t, 1);
                }
                if line.reln == b'=' && is_x(t, k) {
                    line.reln = b'x';
                    k += 1;
                } else if let Kind::Float(_) = kind {
                    match parse_float(&t[k..]) {
                        Some((f, n)) => {
                            line.fnum = f;
                            k += n;
                        }
                        None => {
                            self.warn(format!(
                                "Unparsable number `{}'",
                                String::from_utf8_lossy(&t[k..])
                            ));
                            return;
                        }
                    }
                } else {
                    match parse_num(&t[k..]) {
                        Some((v, n)) => {
                            line.num = v as u64;
                            k += n;
                        }
                        None => {
                            self.warn(format!(
                                "Unparsable number `{}'",
                                String::from_utf8_lossy(&t[k..])
                            ));
                            return;
                        }
                    }
                }
            }
            Kind::Regex => {
                if matches!(t.first(), Some(b'=' | b'!')) {
                    line.reln = t[0];
                    k = 1;
                }
                if line.reln == b'=' && is_x(t, k) {
                    line.reln = b'x';
                    k += 1;
                } else {
                    let n = raw_token(&t[k..]);
                    let src = regex_source(&parse_escaped(&t[k..k + n]).0);
                    let mut fl = RxFlags::MULTILINE;
                    if line.sflags & SF_LC != 0 {
                        fl |= RxFlags::CASELESS;
                    }
                    match Regex::new(&src, fl) {
                        Ok(r) => line.regex = Some(Rc::new(r)),
                        Err(e) => {
                            self.warn(format!("Compilation failed: {}", e));
                            return;
                        }
                    }
                    line.sval = parse_escaped(&t[k..k + n]).0;
                    k += n;
                }
            }
            _ => {
                if matches!(t.first(), Some(b'=' | b'<' | b'>' | b'!')) {
                    line.reln = t[0];
                    k = 1;
                }
                if line.reln == b'=' && is_x(t, k) {
                    line.reln = b'x';
                    k += 1;
                } else {
                    let (v, n) = parse_escaped(&t[k..]);
                    line.sval = v;
                    k += n;
                }
            }
        }
        let d = skip_ws(t, k);
        line.desc = t[d.min(t.len())..].to_vec();
        if let Some(m) = check_format(&line) {
            self.warn(m);
            return;
        }
        if level == 0 {
            self.flush();
            let name = (kind == Kind::Name).then(|| line.sval.clone());
            let text = is_text_line(&line);
            self.cur = Some((
                Entry {
                    lines: vec![line],
                    strength: 0,
                    text,
                    strength_adj: None,
                },
                name,
            ));
        } else if let Some((e, _)) = self.cur.as_mut() {
            if !is_text_line(&line) {
                e.text = false;
            }
            e.lines.push(line);
        }
    }
}

fn pjay(k: Kind) -> bool {
    matches!(k, Kind::PStr(_, _, true))
}

fn trim(s: &[u8]) -> &[u8] {
    let a = skip_ws(s, 0);
    let mut b = s.len();
    while b > a && (is_ws(s[b - 1]) || s[b - 1] == b'\r') {
        b -= 1;
    }
    &s[a..b]
}

/// magic(5): "Each top-level pattern is binary unless all its tests are
/// text"; regex and search tests are text unless their pattern holds
/// unprintable bytes, string tests only with the `t` flag.
fn is_text_line(l: &Line) -> bool {
    if l.sflags & SF_BIN != 0 {
        return false;
    }
    if l.sflags & SF_TEXT != 0 {
        return true;
    }
    match l.kind {
        Kind::Search | Kind::Regex => l
            .sval
            .iter()
            .all(|&c| (0x20..0x7f).contains(&c) || matches!(c, b'\t' | b'\n' | b'\r')),
        // Lines that test nothing keep the entry's class.
        Kind::Default | Kind::Clear | Kind::Name | Kind::Use => true,
        _ => l.reln == b'x' && l.level > 0,
    }
}

/// One printf conversion at most (observed on PHP 8.5: "Too many format
/// strings (should have at most one) for `byte' with description ...").
fn check_format(l: &Line) -> Option<String> {
    let d = &l.desc;
    let mut n = 0;
    let mut i = 0;
    while i < d.len() {
        if d[i] == b'%' {
            n += 1;
            i += 2;
            continue;
        }
        i += 1;
    }
    if n > 1 && !matches!(l.kind, Kind::Name | Kind::Use | Kind::Default | Kind::Clear) {
        return Some(format!(
            "Too many format strings (should have at most one) for `{}' with description `{}'",
            l.type_name,
            String::from_utf8_lossy(d)
        ));
    }
    None
}

/// The default order of entries: more specific tests first (goro's own
/// measure, tuned to the order PHP 8.5 picks between overlapping rules).
fn strength(e: &Entry) -> i64 {
    let l = &e.lines[0];
    let mut s: i64 = 20;
    match l.kind {
        Kind::Int(n) | Kind::Float(n) | Kind::Date(n, _) => {
            s += match n {
                1 => 10,
                2 => 15,
                4 => 35,
                _ => 75,
            }
        }
        Kind::Id3 => s += 35,
        Kind::Str | Kind::PStr(..) | Kind::Octal => {
            s += 10 * l.sval.len() as i64;
            if l.sflags & (SF_LC | SF_UC) != 0 {
                s += 5;
            }
            if l.sflags & SF_SW != 0 {
                s += 2;
            }
        }
        Kind::Str16 => s += 5 * l.sval.len() as i64,
        Kind::Search => {
            s += 10 * l.sval.len() as i64;
            s -= (l.range.max(1) as f64).log2() as i64;
        }
        Kind::Regex => {
            let lit = l.sval.iter().filter(|c| c.is_ascii_alphanumeric()).count();
            s += 10 * lit as i64;
        }
        Kind::Guid => s += 160,
        Kind::Der => s += 10,
        Kind::Default | Kind::Clear | Kind::Offset => s = 0,
        Kind::Indirect | Kind::Name | Kind::Use => {}
    }
    match l.reln {
        b'x' => s = s.min(1).max(0),
        b'!' => s -= 15,
        b'<' | b'>' => s -= 8,
        b'&' | b'^' => s -= 10,
        _ => {}
    }
    if let Some((op, n)) = e.strength_adj {
        s = match op {
            b'+' => s + n,
            b'-' => s - n,
            b'*' => s * n,
            b'/' if n != 0 => s / n,
            _ => s,
        };
    }
    s.max(1)
}

/// Parses an offset field; returns it and the bytes used.
fn parse_offset(s: &[u8]) -> Option<(Offset, usize)> {
    let mut i = 0;
    let mut off = Offset {
        rel: false,
        base: 0,
        ind: None,
    };
    if s.get(i) == Some(&b'&') {
        off.rel = true;
        i += 1;
    }
    if s.get(i) == Some(&b'(') {
        i += 1;
        let mut ind = Indirect {
            rel: false,
            base: 0,
            size: 4,
            endian: Endian::Little,
            signed: false,
            special: 0,
            op: 0,
            operand: 0,
            operand_ind: false,
        };
        if s.get(i) == Some(&b'&') {
            ind.rel = true;
            i += 1;
        }
        let (v, n) = parse_num(&s[i..])?;
        ind.base = v;
        i += n;
        if matches!(s.get(i), Some(b'.' | b',')) {
            ind.signed = s[i] == b',';
            i += 1;
            let c = *s.get(i)?;
            let (size, endian, special) = match c {
                b'b' | b'c' | b'B' | b'C' => (1, Endian::Little, 0),
                b'h' | b's' => (2, Endian::Little, 0),
                b'H' | b'S' => (2, Endian::Big, 0),
                b'l' => (4, Endian::Little, 0),
                b'L' => (4, Endian::Big, 0),
                b'q' => (8, Endian::Little, 0),
                b'Q' => (8, Endian::Big, 0),
                b'm' => (4, Endian::Middle, 0),
                b'i' => (4, Endian::Little, b'i'),
                b'I' => (4, Endian::Big, b'i'),
                b'o' => (4, Endian::Little, b'o'),
                b'e' | b'f' | b'g' => (4, Endian::Little, 0),
                b'E' | b'F' | b'G' => (4, Endian::Big, 0),
                _ => return None,
            };
            ind.size = size;
            ind.endian = endian;
            ind.special = special;
            i += 1;
        }
        if let Some(&c) = s.get(i)
            && matches!(c, b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^' | b'~')
        {
            i += 1;
            ind.op = c;
            if s.get(i) == Some(&b'(') {
                i += 1;
                let (v, n) = parse_num(&s[i..])?;
                ind.operand = v;
                ind.operand_ind = true;
                i += n;
                if s.get(i) != Some(&b')') {
                    return None;
                }
                i += 1;
            } else if c != b'~' {
                let (v, n) = parse_num(&s[i..])?;
                ind.operand = v;
                i += n;
            }
        }
        if s.get(i) != Some(&b')') {
            return None;
        }
        i += 1;
        off.ind = Some(Box::new(ind));
        // An addend after the parenthesis.
        if let Some(&c) = s.get(i)
            && (c == b'+' || c == b'-')
            && let Some((v, n)) = parse_num(&s[i..])
        {
            off.base = v;
            i += n;
        }
        return Some((off, i));
    }
    let (v, n) = parse_num(&s[i..])?;
    off.base = v;
    Some((off, i + n))
}

impl Magic {
    /// Parses one or more magic sources (a directory's files are given in
    /// name order). Returns the database (or the failure) and the warnings
    /// PHP prints, fatal or not.
    pub fn parse(
        sources: &[&[u8]],
        keep_order: bool,
    ) -> (Result<Magic, ParseError>, Vec<String>) {
        let mut p = Parser {
            warnings: Vec::new(),
            failed: false,
            entries: Vec::new(),
            names: HashMap::new(),
            cur: None,
            keep_order,
        };
        let mut any = false;
        for src in sources {
            for raw in src.split(|&c| c == b'\n') {
                p.line(raw);
            }
            p.flush();
            any |= !p.entries.is_empty() || !p.names.is_empty();
        }
        p.flush();
        let warnings = std::mem::take(&mut p.warnings);
        if p.failed || !any {
            return (
                Err(ParseError {
                    warnings: warnings.clone(),
                }),
                warnings,
            );
        }
        let mut m = Magic {
            entries: p.entries,
            names: p.names,
        };
        if !p.keep_order {
            m.entries.sort_by(|a, b| b.strength.cmp(&a.strength));
        }
        (Ok(m), warnings)
    }
}
