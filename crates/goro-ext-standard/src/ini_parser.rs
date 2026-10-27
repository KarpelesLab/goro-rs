//! INI parser for parse_ini_string()/parse_ini_file(): a port of
//! Zend/zend_ini_scanner.l (re2c, case-inverted) and Zend/zend_ini_parser.y.

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use std::cell::RefCell;
use std::rc::Rc;

pub const SCANNER_NORMAL: i64 = 0;
pub const SCANNER_RAW: i64 = 1;
pub const SCANNER_TYPED: i64 = 2;

#[derive(Clone, Copy, PartialEq, Debug)]
enum St {
    Initial,
    Offset,
    SectionValue,
    Value,
    SectionRaw,
    DoubleQuotes,
    VarName,
    VarFallback,
    Raw,
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    End,
    Section,
    Raw,
    Constant,
    Number,
    Str,
    Whitespace,
    Label,
    Offset,
    DollarCurly,
    VarName,
    QuotedString,
    Fallback,
    BoolTrue,
    BoolFalse,
    Null,
    Eol,
    Ch(u8),
}

impl Tok {
    fn name(&self) -> String {
        match self {
            Tok::End => "end of file".into(),
            Tok::Section => "TC_SECTION".into(),
            Tok::Raw => "TC_RAW".into(),
            Tok::Constant => "TC_CONSTANT".into(),
            Tok::Number => "TC_NUMBER".into(),
            Tok::Str => "TC_STRING".into(),
            Tok::Whitespace => "TC_WHITESPACE".into(),
            Tok::Label => "TC_LABEL".into(),
            Tok::Offset => "TC_OFFSET".into(),
            Tok::DollarCurly => "TC_DOLLAR_CURLY".into(),
            Tok::VarName => "TC_VARNAME".into(),
            Tok::QuotedString => "TC_QUOTED_STRING".into(),
            Tok::Fallback => "TC_FALLBACK".into(),
            Tok::BoolTrue => "BOOL_TRUE".into(),
            Tok::BoolFalse => "BOOL_FALSE".into(),
            Tok::Null => "NULL_NULL".into(),
            Tok::Eol => "END_OF_LINE".into(),
            Tok::Ch(c) => format!("'{}'", *c as char),
        }
    }
}

/// A semantic value: a zval plus the INI_ZVAL_IS_NUMBER extra flag.
#[derive(Clone, Debug)]
struct Sv {
    v: Value,
    is_number: bool,
}

impl Sv {
    fn s(b: &[u8]) -> Sv {
        Sv {
            v: Value::String(PhpString::from_bytes(b)),
            is_number: false,
        }
    }
    fn empty() -> Sv {
        Sv::s(b"")
    }
    fn bytes(&self) -> Vec<u8> {
        self.v.to_php_string().as_bytes().to_vec()
    }
}

struct Scanner<'a> {
    d: &'a [u8],
    pos: usize,
    st: St,
    stack: Vec<St>,
    lineno: u32,
    mode: i64,
}

fn is_label_char(c: u8) -> bool {
    !matches!(
        c,
        b'=' | b'\n'
            | b'\r'
            | b'\t'
            | b';'
            | b'&'
            | b'|'
            | b'^'
            | b'$'
            | b'~'
            | b'('
            | b')'
            | b'{'
            | b'}'
            | b'!'
            | b'"'
            | b'['
            | b']'
            | 0
    )
}

impl<'a> Scanner<'a> {
    fn at(&self, i: usize) -> u8 {
        self.d.get(i).copied().unwrap_or(0)
    }

    fn begin(&mut self, s: St) {
        self.st = s;
    }

    fn push(&mut self, s: St) {
        self.stack.push(self.st);
        self.st = s;
    }

    fn pop(&mut self) {
        self.st = self.stack.pop().unwrap_or(St::Initial);
    }

    fn tok(&self, t: Tok, text: &[u8]) -> (Tok, Sv) {
        if self.mode == SCANNER_TYPED && matches!(self.st, St::Value | St::Raw) {
            match t {
                Tok::BoolTrue => {
                    return (
                        t,
                        Sv {
                            v: Value::True,
                            is_number: false,
                        },
                    );
                }
                Tok::BoolFalse => {
                    return (
                        t,
                        Sv {
                            v: Value::False,
                            is_number: false,
                        },
                    );
                }
                Tok::Null => {
                    return (
                        t,
                        Sv {
                            v: Value::Null,
                            is_number: false,
                        },
                    );
                }
                _ => {}
            }
        }
        (t, Sv::s(text))
    }

    // ---- pattern helpers: return match length at p (0 = no match) ----
    fn m_tabs(&self, p: usize) -> usize {
        let mut e = p;
        while matches!(self.at(e), b' ' | b'\t') {
            e += 1;
        }
        e - p
    }
    fn m_newline(&self, p: usize) -> usize {
        match self.at(p) {
            b'\r' if self.at(p + 1) == b'\n' => 2,
            b'\r' | b'\n' => 1,
            _ => 0,
        }
    }
    fn m_label(&self, p: usize) -> usize {
        let mut e = p;
        while e < self.d.len() && is_label_char(self.at(e)) {
            e += 1;
        }
        e - p
    }
    fn m_ci(&self, p: usize, word: &[u8]) -> usize {
        for (i, &w) in word.iter().enumerate() {
            if !self.at(p + i).eq_ignore_ascii_case(&w) {
                return 0;
            }
        }
        word.len()
    }
    fn m_constant(&self, p: usize) -> usize {
        let c = self.at(p);
        if !(c.is_ascii_alphabetic() || c == b'_') {
            return 0;
        }
        let mut e = p + 1;
        while self.at(e).is_ascii_alphanumeric() || self.at(e) == b'_' {
            e += 1;
        }
        e - p
    }
    fn m_digits(&self, p: usize) -> usize {
        let mut e = p;
        while self.at(e).is_ascii_digit() {
            e += 1;
        }
        e - p
    }
    /// NUMBER = [-]?LNUM | DNUM
    fn m_number(&self, p: usize) -> usize {
        let mut best = 0;
        // [-]?LNUM
        let s = if self.at(p) == b'-' { p + 1 } else { p };
        let l = self.m_digits(s);
        if l > 0 {
            best = s + l - p;
        }
        // DNUM ([0-9]*[.][0-9]+)|([0-9]+[.][0-9]*)
        let a = self.m_digits(p);
        if self.at(p + a) == b'.' {
            let b = self.m_digits(p + a + 1);
            if a > 0 || b > 0 {
                best = best.max(a + 1 + b);
            }
        }
        best
    }
    /// LITERAL_DOLLAR at p (p points at '$'): returns length or 0.
    fn m_literal_dollar(&self, p: usize) -> usize {
        if self.at(p) != b'$' {
            return 0;
        }
        let c = self.at(p + 1);
        if p + 1 >= self.d.len() {
            return 0;
        }
        if c == b'\\' && p + 2 < self.d.len() {
            // both alternatives match; longest wins
            return 3;
        }
        if c != b'{' && c != 0 { 2 } else { 0 }
    }
    fn m_chars_plus(&self, p: usize, plain: impl Fn(u8) -> bool, backslash_any: bool) -> usize {
        let mut e = p;
        loop {
            if e >= self.d.len() {
                break;
            }
            let c = self.at(e);
            if c == b'$' {
                let l = self.m_literal_dollar(e);
                if l == 0 {
                    break;
                }
                e += l;
                continue;
            }
            if backslash_any && c == b'\\' {
                if e + 1 < self.d.len() {
                    e += 2;
                    continue;
                }
                break;
            }
            if plain(c) {
                e += 1;
                continue;
            }
            break;
        }
        e - p
    }
    fn m_value_chars(&self, p: usize) -> usize {
        self.m_chars_plus(
            p,
            |c| {
                !matches!(
                    c,
                    b'$' | b'='
                        | b' '
                        | b'\t'
                        | b'\n'
                        | b'\r'
                        | b';'
                        | b'&'
                        | b'|'
                        | b'^'
                        | b'~'
                        | b'('
                        | b')'
                        | b'!'
                        | b'"'
                        | b'\''
                        | 0
                )
            },
            false,
        )
    }
    fn m_fallback_chars(&self, p: usize) -> usize {
        self.m_chars_plus(
            p,
            |c| !matches!(c, b'$' | b'\n' | b'\r' | b';' | b'"' | b'\'' | b'}' | b'\\'),
            true,
        )
    }
    fn m_section_value_chars(&self, p: usize) -> usize {
        self.m_chars_plus(
            p,
            |c| !matches!(c, b'$' | b'\n' | b'\r' | b';' | b'"' | b'\'' | b']' | b'\\'),
            true,
        )
    }

    fn eat_ws_lead_trail(text: &[u8], trail_extra: Option<u8>) -> &[u8] {
        let mut s = 0;
        let mut e = text.len();
        while s < e && matches!(text[s], b' ' | b'\t') {
            s += 1;
        }
        while e > s
            && (matches!(text[e - 1], b'\n' | b'\r' | b'\t' | b' ')
                || Some(text[e - 1]) == trail_extra)
        {
            e -= 1;
        }
        &text[s..e]
    }

    /// zend_ini_escape_string()
    fn escape_string(&mut self, text: &[u8], quote: u8) -> Vec<u8> {
        let mut out = Vec::with_capacity(text.len());
        let mut i = 0;
        while i < text.len() {
            let c = text[i];
            if c == b'\\' {
                i += 1;
                if i >= text.len() {
                    out.push(b'\\');
                    continue;
                }
                let n = text[i];
                match n {
                    b'"' if n != quote => {
                        out.push(b'\\');
                        out.push(n);
                    }
                    b'"' | b'\\' | b'$' => out.push(n),
                    _ => {
                        out.push(b'\\');
                        out.push(n);
                    }
                }
                if n == b'\n' || (n == b'\r' && text.get(i + 1) != Some(&b'\n')) {
                    self.lineno += 1;
                }
            } else {
                out.push(c);
                if c == b'\n' || (c == b'\r' && text.get(i + 1) != Some(&b'\n')) {
                    self.lineno += 1;
                }
            }
            i += 1;
        }
        out
    }

    fn lex(&mut self) -> (Tok, Sv) {
        loop {
            let start = self.pos;
            if start >= self.d.len() {
                if matches!(self.st, St::Value | St::Raw) {
                    self.begin(St::Initial);
                    return (Tok::Eol, Sv::empty());
                }
                return (Tok::End, Sv::empty());
            }
            if start == 0 && self.d.len() > 3 && self.d.starts_with(b"\xef\xbb\xbf") {
                self.pos = 3;
                continue;
            }
            let st = self.st;
            let raw_mode = self.mode == SCANNER_RAW;
            // Candidate rules: (length, rule id). Longest wins; ties → lowest id.
            let mut best: (usize, u32) = (0, u32::MAX);
            let cand = |len: usize, id: u32, best: &mut (usize, u32)| {
                if len > best.0 || (len == best.0 && len > 0 && id < best.1) {
                    *best = (len, id);
                }
            };
            let c0 = self.at(start);
            use St::*;
            if st == Initial && c0 == b'[' {
                cand(1, 1, &mut best);
            }
            if matches!(st, Value | SectionValue | Offset) && c0 == b'\'' {
                let mut e = start + 1;
                while e < self.d.len() && self.at(e) != b'\'' {
                    e += 1;
                }
                if e > start + 1 && self.at(e) == b'\'' {
                    cand(e + 1 - start, 2, &mut best);
                }
                // An unterminated raw string runs past the end of the
                // buffer: YYFILL() ends the scan (an unexpected end of file).
                if e >= self.d.len() {
                    self.pos = self.d.len();
                    return (Tok::End, Sv::empty());
                }
            }
            if matches!(st, SectionRaw | SectionValue) && c0 == b']' {
                let t = self.m_tabs(start + 1);
                let n = self.m_newline(start + 1 + t);
                cand(1 + t + n, 3, &mut best);
            }
            if st == Initial {
                let l = self.m_label(start);
                if l > 0 && self.at(start + l) == b'[' {
                    let t = self.m_tabs(start + l + 1);
                    cand(l + 1 + t, 4, &mut best);
                }
            }
            if st == Offset {
                let t = self.m_tabs(start);
                if self.at(start + t) == b']' {
                    cand(t + 1, 5, &mut best);
                }
            }
            if matches!(
                st,
                DoubleQuotes | SectionValue | Value | Offset | VarFallback
            ) && c0 == b'$'
                && self.at(start + 1) == b'{'
            {
                cand(2, 6, &mut best);
            }
            if st == VarName && c0 == b':' && self.at(start + 1) == b'-' {
                cand(2, 7, &mut best);
            }
            if st == VarName && is_label_char(c0) && start < self.d.len() {
                cand(1, 8, &mut best);
            }
            if matches!(st, VarName | VarFallback) && c0 == b'}' {
                cand(1, 9, &mut best);
            }
            if matches!(st, Initial | Value) {
                for w in [&b"true"[..], b"on", b"yes"] {
                    let l = self.m_ci(start, w);
                    if l > 0 {
                        cand(l + self.m_tabs(start + l), 10, &mut best);
                    }
                }
                for w in [&b"false"[..], b"off", b"no", b"none"] {
                    let l = self.m_ci(start, w);
                    if l > 0 {
                        cand(l + self.m_tabs(start + l), 11, &mut best);
                    }
                }
                let l = self.m_ci(start, b"null");
                if l > 0 {
                    cand(l + self.m_tabs(start + l), 12, &mut best);
                }
            }
            if st == Initial {
                cand(self.m_label(start), 13, &mut best);
                let t = self.m_tabs(start);
                if self.at(start + t) == b'=' {
                    cand(t + 1 + self.m_tabs(start + t + 1), 14, &mut best);
                }
            }
            if st == Raw && !matches!(c0, b'\n' | b'\r' | b';' | 0) {
                cand(1, 15, &mut best);
            }
            if st == SectionRaw {
                let mut e = start;
                while e < self.d.len() && !matches!(self.at(e), b']' | b'\n' | b'\r') {
                    e += 1;
                }
                cand(e - start, 16, &mut best);
            }
            if matches!(st, Value | Raw) {
                let t = self.m_tabs(start);
                let n = self.m_newline(start + t);
                if n > 0 {
                    cand(t + n, 17, &mut best);
                }
            }
            if matches!(st, SectionValue | Value | VarFallback | Offset) {
                cand(self.m_constant(start), 18, &mut best);
                cand(self.m_number(start), 19, &mut best);
            }
            if st == Initial && b":,.[]\"'()&|^+-/*=%$!~<>?@{}".contains(&c0) && c0 != 0 {
                cand(1, 20, &mut best);
            }
            if st == Value && b"&|^~()!".contains(&c0) && c0 != 0 {
                cand(1 + self.m_tabs(start + 1), 21, &mut best);
            }
            if st == Value && c0 == b'=' {
                cand(1, 22, &mut best);
            }
            if st == Value {
                cand(self.m_value_chars(start), 23, &mut best);
            }
            if st == VarFallback {
                cand(self.m_fallback_chars(start), 24, &mut best);
            }
            if matches!(st, SectionValue | Offset) {
                cand(self.m_section_value_chars(start), 25, &mut best);
            }
            if matches!(st, SectionValue | Value | VarFallback | Offset) {
                let t = self.m_tabs(start);
                if self.at(start + t) == b'"' {
                    cand(t + 1, 26, &mut best);
                }
            }
            if st == DoubleQuotes {
                if c0 == b'"' {
                    cand(1 + self.m_tabs(start + 1), 27, &mut best);
                }
                cand(1, 28, &mut best);
            }
            if matches!(st, SectionValue | Value | Offset | VarFallback) {
                cand(self.m_tabs(start), 29, &mut best);
            }
            if matches!(st, Initial | Raw) {
                cand(self.m_tabs(start), 30, &mut best);
            }
            if st == Initial {
                let t = self.m_tabs(start);
                let n = self.m_newline(start + t);
                if n > 0 {
                    cand(t + n, 31, &mut best);
                }
            }
            if matches!(st, Initial | Value | Raw) {
                let t = self.m_tabs(start);
                if self.at(start + t) == b';' {
                    let mut e = start + t + 1;
                    while e < self.d.len() && !matches!(self.at(e), b'\r' | b'\n') {
                        e += 1;
                    }
                    let n = self.m_newline(e);
                    if n > 0 {
                        cand(e + n - start, 32, &mut best);
                    }
                }
            }
            if matches!(st, Value | Raw) {
                cand(1, 33, &mut best);
            }
            if best.0 == 0 {
                return (Tok::End, Sv::empty());
            }
            let (len, id) = best;
            let end = start + len;
            self.pos = end;
            let text = &self.d[start..end];
            match id {
                1 => {
                    self.begin(if raw_mode { SectionRaw } else { SectionValue });
                    return (Tok::Section, Sv::empty());
                }
                2 => return self.tok(Tok::Raw, &text[1..text.len() - 1]),
                3 => {
                    self.begin(Initial);
                    self.lineno += 1;
                    return (Tok::Ch(b']'), Sv::empty());
                }
                4 => {
                    let t = Self::eat_ws_lead_trail(text, Some(b'[')).to_vec();
                    self.begin(Offset);
                    return self.tok(Tok::Offset, &t);
                }
                5 => {
                    self.begin(Initial);
                    return (Tok::Ch(b']'), Sv::empty());
                }
                6 => {
                    self.push(VarName);
                    return (Tok::DollarCurly, Sv::empty());
                }
                7 => {
                    self.pop();
                    self.push(VarFallback);
                    return (Tok::Fallback, Sv::empty());
                }
                8 => {
                    let mut cur = self.pos;
                    if self.at(cur) == b':' && self.at(cur + 1) == b'-' {
                        self.pos = cur + 1;
                        self.pop();
                        self.push(VarFallback);
                        return (Tok::Fallback, Sv::empty());
                    }
                    while cur < self.d.len() {
                        let c = self.at(cur);
                        cur += 1;
                        let stop = match c {
                            b'=' | b'\n' | b'\r' | b'\t' | b';' | b'&' | b'|' | b'^' | b'$'
                            | b'~' | b'(' | b')' | b'{' | b'}' | b'!' | b'"' | b'[' | b']' => true,
                            b':' => self.at(cur) == b'-',
                            _ => false,
                        };
                        if stop {
                            cur -= 1;
                            break;
                        }
                    }
                    self.pos = cur;
                    let t = Self::eat_ws_lead_trail(&self.d[start..cur], None).to_vec();
                    return self.tok(Tok::VarName, &t);
                }
                9 => {
                    self.pop();
                    return (Tok::Ch(b'}'), Sv::empty());
                }
                10 => return self.tok(Tok::BoolTrue, b"1"),
                11 => return self.tok(Tok::BoolFalse, b""),
                12 => return self.tok(Tok::Null, b""),
                13 => {
                    let t = Self::eat_ws_lead_trail(text, None).to_vec();
                    return self.tok(Tok::Label, &t);
                }
                14 => {
                    self.begin(if raw_mode { Raw } else { Value });
                    return (Tok::Ch(b'='), Sv::empty());
                }
                15 => {
                    // raw value up to end of line; ';' starts a comment unless quoted
                    let mut s = start;
                    while matches!(self.at(s), b' ' | b'\t') && s < self.d.len() {
                        s += 1;
                    }
                    let mut cur = start + 1;
                    let mut sc: Option<usize> = None;
                    while cur < self.d.len() {
                        match self.at(cur) {
                            b'\n' | b'\r' => break,
                            b';' => {
                                if sc.is_none() {
                                    sc = Some(cur);
                                }
                                cur += 1;
                            }
                            b'"' => {
                                if self.at(s) == b'"' {
                                    sc = None;
                                }
                                cur += 1;
                            }
                            _ => cur += 1,
                        }
                    }
                    self.pos = cur;
                    let e = sc.unwrap_or(cur);
                    let mut v =
                        Self::eat_ws_lead_trail(&self.d[start..e.max(start)], None).to_vec();
                    if v.len() > 1 && v[0] == b'"' && v[v.len() - 1] == b'"' {
                        v = v[1..v.len() - 1].to_vec();
                    }
                    return self.tok(Tok::Raw, &v);
                }
                16 => return self.tok(Tok::Raw, text),
                17 => {
                    self.begin(Initial);
                    self.lineno += 1;
                    return (Tok::Eol, Sv::empty());
                }
                18 => return self.tok(Tok::Constant, text),
                19 => return self.tok(Tok::Number, text),
                20 => return (Tok::Ch(c0), Sv::empty()),
                21 => return (Tok::Ch(c0), Sv::empty()),
                22 => {
                    self.pos = start;
                    self.begin(Initial);
                    return (Tok::Eol, Sv::empty());
                }
                23..=25 => return self.tok(Tok::Str, text),
                26 => {
                    self.push(DoubleQuotes);
                    return (Tok::Ch(b'"'), Sv::empty());
                }
                27 => {
                    self.pop();
                    return (Tok::Ch(b'"'), Sv::empty());
                }
                28 => {
                    let mut s = start;
                    while s < self.d.len() {
                        let c = self.at(s);
                        s += 1;
                        match c {
                            b'"' => {
                                s -= 1;
                                break;
                            }
                            b'$' => {
                                if s < self.d.len() && self.at(s) == b'{' {
                                    s -= 1;
                                    break;
                                }
                            }
                            b'\\' => {
                                if s < self.d.len() {
                                    let esc = self.at(s);
                                    s += 1;
                                    if esc == b'"'
                                        && (s >= self.d.len()
                                            || matches!(self.at(s), b'\n' | b'\r'))
                                    {
                                        s -= 1;
                                        break;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    self.pos = s;
                    let raw = self.d[start..s].to_vec();
                    let v = self.escape_string(&raw, b'"');
                    return (
                        Tok::QuotedString,
                        Sv {
                            v: goro_core::value::Value::String(PhpString::from_vec(v)),
                            is_number: false,
                        },
                    );
                }
                29 => return self.tok(Tok::Whitespace, text),
                30 => continue,
                31 => {
                    self.lineno += 1;
                    return (Tok::Eol, Sv::empty());
                }
                32 => {
                    self.begin(Initial);
                    self.lineno += 1;
                    return (Tok::Eol, Sv::empty());
                }
                33 => {
                    self.begin(Initial);
                    return (Tok::Eol, Sv::empty());
                }
                _ => return (Tok::End, Sv::empty()),
            }
        }
    }
}

struct Parser<'a, 'b> {
    sc: Scanner<'a>,
    la: Option<(Tok, Sv)>,
    vm: &'b mut Vm,
    filename: String,
    process_sections: bool,
    root: PhpArray,
    section: Option<(ArrayKey, PhpArray)>,
    /// zend_parse_ini_string() with a callback of its own: the parsed
    /// entries are recorded instead of building an array.
    events: Option<Vec<IniEvent>>,
    /// CG(ini_parser_unbuffered_errors): the syntax error is kept for the
    /// caller instead of raising a warning.
    unbuffered_error: Option<Option<String>>,
}

/// One callback of zend_parse_ini_string() (ZEND_INI_PARSER_*).
#[derive(Clone, Debug)]
pub enum IniEvent {
    /// ZEND_INI_PARSER_ENTRY: name = value
    Entry(Vec<u8>, Vec<u8>),
    /// ZEND_INI_PARSER_SECTION: [name]
    Section(Vec<u8>),
    /// ZEND_INI_PARSER_POP_ENTRY: name[offset] = value
    PopEntry(Vec<u8>, Vec<u8>, Vec<u8>),
}

type PResult<T> = Result<T, ()>;

fn symtable_key(b: &[u8]) -> ArrayKey {
    Vm::value_to_array_key(Value::String(PhpString::from_bytes(b)))
}

impl<'a, 'b> Parser<'a, 'b> {
    fn peek(&mut self) -> &Tok {
        if self.la.is_none() {
            self.la = Some(self.sc.lex());
        }
        &self.la.as_ref().unwrap().0
    }

    fn next(&mut self) -> (Tok, Sv) {
        match self.la.take() {
            Some(t) => t,
            None => self.sc.lex(),
        }
    }

    fn error(&mut self, expecting: &[&str]) -> PResult<()> {
        let unexpected = self.peek().clone();
        let mut msg = format!("syntax error, unexpected {}", unexpected.name());
        if !expecting.is_empty() {
            msg.push_str(", expecting ");
            msg.push_str(&expecting.join(" or "));
        }
        let full = format!("{} in {} on line {}\n", msg, self.filename, self.sc.lineno);
        if let Some(slot) = &mut self.unbuffered_error {
            *slot = Some(full);
            return Err(());
        }
        self.vm.emit_warning(&full);
        Err(())
    }

    fn concat(a: Sv, b: Sv) -> Sv {
        let mut s = a.bytes();
        s.extend_from_slice(&b.bytes());
        Sv {
            v: Value::String(PhpString::from_vec(s)),
            is_number: a.is_number,
        }
    }

    fn get_constant(&mut self, name: Sv) -> Sv {
        let n = name.bytes();
        if !n.contains(&b':') {
            if n.eq_ignore_ascii_case(b"true") {
                return Sv::s(b"1");
            }
            if n.eq_ignore_ascii_case(b"false") || n.eq_ignore_ascii_case(b"null") {
                return Sv::empty();
            }
            // (INI settings share goro's constant table; their names are
            // lowercase, which constants are not)
            if n.iter().any(|c| c.is_ascii_uppercase()) {
                if let Some(c) = self.vm.constants.get(&n) {
                    let s = c.to_php_string();
                    return Sv {
                        v: Value::String(s),
                        is_number: false,
                    };
                }
            }
        }
        name
    }

    fn get_var(&mut self, name: &[u8], fallback: Option<Sv>) -> Sv {
        // Configuration directives (ini settings share goro's constant table;
        // directive names are lowercase, constants conventionally are not).
        if name
            .iter()
            .any(|c| c.is_ascii_lowercase() || *c == b':' || *c == b'.')
        {
            if let Some(v) = self.vm.constants.get(name) {
                return Sv {
                    v: Value::String(v.to_php_string()),
                    is_number: false,
                };
            }
        }
        if let Ok(e) = std::env::var(String::from_utf8_lossy(name).as_ref()) {
            return Sv::s(e.as_bytes());
        }
        match fallback {
            Some(f) => Sv {
                v: Value::String(PhpString::from_vec(f.bytes())),
                is_number: false,
            },
            None => Sv::empty(),
        }
    }

    /// cfg_var_ref (after TC_DOLLAR_CURLY was consumed)
    fn cfg_var_ref(&mut self) -> PResult<Sv> {
        if *self.peek() != Tok::VarName {
            return self.error(&["TC_VARNAME"]).map(|_| Sv::empty());
        }
        let (_, name) = self.next();
        match self.peek().clone() {
            Tok::Ch(b'}') => {
                self.next();
                Ok(self.get_var(&name.bytes(), None))
            }
            Tok::Fallback => {
                self.next();
                let fb = if self.starts_var_string() {
                    self.var_string_list(true)?
                } else {
                    Sv::empty()
                };
                if *self.peek() != Tok::Ch(b'}') {
                    return self.error(&["'}'"]).map(|_| Sv::empty());
                }
                self.next();
                Ok(self.get_var(&name.bytes(), Some(fb)))
            }
            _ => self.error(&["TC_FALLBACK", "'}'"]).map(|_| Sv::empty()),
        }
    }

    /// '"' encapsed_list '"' (after the opening quote was consumed)
    fn encapsed_list(&mut self) -> PResult<Sv> {
        let mut acc = Sv::empty();
        loop {
            match self.peek().clone() {
                Tok::DollarCurly => {
                    self.next();
                    let v = self.cfg_var_ref()?;
                    acc = Self::concat(acc, v);
                }
                Tok::QuotedString => {
                    let (_, v) = self.next();
                    acc = Self::concat(acc, v);
                }
                Tok::Ch(b'"') => {
                    self.next();
                    return Ok(acc);
                }
                _ => {
                    return self
                        .error(&["TC_DOLLAR_CURLY", "TC_QUOTED_STRING", "'\"'"])
                        .map(|_| Sv::empty());
                }
            }
        }
    }

    fn starts_var_string(&mut self) -> bool {
        matches!(
            self.peek(),
            Tok::DollarCurly
                | Tok::Constant
                | Tok::Raw
                | Tok::Number
                | Tok::Str
                | Tok::Whitespace
                | Tok::Ch(b'"')
        )
    }

    /// var_string_list (constants resolved) or var_string_list_section (literal).
    fn var_string_list(&mut self, resolve_constants: bool) -> PResult<Sv> {
        let mut acc: Option<Sv> = None;
        while self.starts_var_string() {
            let piece = match self.peek().clone() {
                Tok::DollarCurly => {
                    self.next();
                    self.cfg_var_ref()?
                }
                Tok::Ch(b'"') => {
                    self.next();
                    self.encapsed_list()?
                }
                Tok::Constant => {
                    let (_, v) = self.next();
                    if resolve_constants {
                        self.get_constant(v)
                    } else {
                        v
                    }
                }
                Tok::Number => {
                    let (_, mut v) = self.next();
                    if resolve_constants {
                        v.is_number = true;
                    }
                    v
                }
                _ => self.next().1,
            };
            acc = Some(match acc {
                None => piece,
                Some(a) => Self::concat(a, piece),
            });
        }
        Ok(acc.unwrap_or_else(Sv::empty))
    }

    fn int_val(&self, v: &Sv) -> i64 {
        match &v.v {
            Value::Long(n) => *n as i32 as i64,
            Value::Double(f) => *f as i32 as i64,
            other => {
                // atoi()
                let b = other.to_php_string().as_bytes().to_vec();
                let mut i = 0;
                while i < b.len() && (b[i] as char).is_ascii_whitespace() {
                    i += 1;
                }
                let neg = i < b.len() && b[i] == b'-';
                if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
                    i += 1;
                }
                let mut n: i64 = 0;
                while i < b.len() && b[i].is_ascii_digit() {
                    n = n.wrapping_mul(10).wrapping_add((b[i] - b'0') as i64);
                    i += 1;
                }
                (if neg { -n } else { n }) as i32 as i64
            }
        }
    }

    fn do_op(&self, op: u8, a: &Sv, b: Option<&Sv>) -> Sv {
        let x = self.int_val(a) as i32;
        let y = b.map(|b| self.int_val(b) as i32).unwrap_or(0);
        let r: i32 = match op {
            b'|' => x | y,
            b'&' => x & y,
            b'^' => x ^ y,
            b'~' => !x,
            b'!' => (x == 0) as i32,
            _ => 0,
        };
        if self.sc.mode == SCANNER_TYPED {
            Sv {
                v: Value::Long(r as i64),
                is_number: false,
            }
        } else {
            Sv::s(r.to_string().as_bytes())
        }
    }

    fn unary(&mut self) -> PResult<Sv> {
        match self.peek().clone() {
            Tok::Ch(c @ (b'~' | b'!')) => {
                self.next();
                let v = self.unary()?;
                Ok(self.do_op(c, &v, None))
            }
            Tok::Ch(b'(') => {
                self.next();
                let v = self.expr()?;
                if *self.peek() != Tok::Ch(b')') {
                    self.error(&[])?;
                }
                self.next();
                Ok(v)
            }
            _ => {
                if self.starts_var_string() {
                    self.var_string_list(true)
                } else {
                    self.error(&[]).map(|_| Sv::empty())
                }
            }
        }
    }

    fn expr(&mut self) -> PResult<Sv> {
        let mut left = self.unary()?;
        while let Tok::Ch(c @ (b'|' | b'&' | b'^')) = self.peek().clone() {
            self.next();
            let right = self.unary()?;
            left = self.do_op(c, &left, Some(&right));
        }
        Ok(left)
    }

    fn normalize(&self, v: Sv) -> Value {
        if self.sc.mode == SCANNER_TYPED && v.is_number {
            if let Value::String(s) = &v.v {
                let st = s.to_string_lossy();
                if let Ok(n) = st.parse::<i64>() {
                    return Value::Long(n);
                }
                if let Ok(f) = st.parse::<f64>() {
                    if f.is_finite() {
                        return Value::Double(f);
                    }
                }
            }
        }
        v.v
    }

    fn string_or_value(&mut self) -> PResult<Value> {
        match self.peek().clone() {
            Tok::BoolTrue | Tok::BoolFalse | Tok::Null => Ok(self.next().1.v),
            Tok::Eol => {
                self.next();
                Ok(Value::String(PhpString::empty()))
            }
            _ => {
                let v = self.expr()?;
                Ok(self.normalize(v))
            }
        }
    }

    fn active(&mut self) -> &mut PhpArray {
        match &mut self.section {
            Some((_, arr)) => arr,
            None => &mut self.root,
        }
    }

    fn flush_section(&mut self) {
        if let Some((k, arr)) = self.section.take() {
            self.root.set(k, Value::Array(Rc::new(RefCell::new(arr))));
        }
    }

    fn entry(&mut self, key: &[u8], value: Value) {
        if let Some(ev) = &mut self.events {
            ev.push(IniEvent::Entry(
                key.to_vec(),
                value.to_php_string().as_bytes().to_vec(),
            ));
            return;
        }
        self.active().set(symtable_key(key), value);
    }

    fn pop_entry(&mut self, key: &[u8], value: Value, offset: Option<Value>) {
        if let Some(ev) = &mut self.events {
            ev.push(IniEvent::PopEntry(
                key.to_vec(),
                offset
                    .map(|o| o.to_php_string().as_bytes().to_vec())
                    .unwrap_or_default(),
                value.to_php_string().as_bytes().to_vec(),
            ));
            return;
        }
        let k = if !(key.len() > 1 && key[0] == b'0') {
            match std::str::from_utf8(key)
                .ok()
                .and_then(|s| s.parse::<i64>().ok())
            {
                Some(n) => ArrayKey::Int(n),
                None => ArrayKey::String(PhpString::from_bytes(key)),
            }
        } else {
            ArrayKey::String(PhpString::from_bytes(key))
        };
        let arr = self.active();
        let existing = match arr.get(&k) {
            Some(Value::Array(a)) => Some(a.clone()),
            _ => None,
        };
        let target = match existing {
            Some(a) => a,
            None => {
                let a = Rc::new(RefCell::new(PhpArray::new()));
                arr.set(k, Value::Array(a.clone()));
                a
            }
        };
        match offset {
            Some(Value::String(s)) if !s.as_bytes().is_empty() => {
                target.borrow_mut().set(symtable_key(s.as_bytes()), value);
            }
            Some(o) if !matches!(o, Value::String(_)) => {
                target.borrow_mut().set(Vm::value_to_array_key(o), value);
            }
            _ => target.borrow_mut().push(value),
        }
    }

    fn parse(&mut self) -> PResult<()> {
        loop {
            match self.peek().clone() {
                Tok::End => return Ok(()),
                Tok::Eol => {
                    self.next();
                }
                Tok::Section => {
                    self.next();
                    let name = if self.starts_var_string() {
                        self.var_string_list(false)?
                    } else {
                        Sv::empty()
                    };
                    if *self.peek() != Tok::Ch(b']') {
                        return self.error(&["']'"]);
                    }
                    self.next();
                    if let Some(ev) = &mut self.events {
                        ev.push(IniEvent::Section(name.bytes()));
                    } else if self.process_sections {
                        self.flush_section();
                        self.section = Some((symtable_key(&name.bytes()), PhpArray::new()));
                    }
                }
                Tok::Label => {
                    let (_, label) = self.next();
                    if *self.peek() == Tok::Ch(b'=') {
                        self.next();
                        let v = self.string_or_value()?;
                        self.entry(&label.bytes(), v);
                    }
                }
                Tok::Offset => {
                    let (_, name) = self.next();
                    let off = if self.starts_var_string() {
                        self.var_string_list(true)?
                    } else {
                        Sv::empty()
                    };
                    if *self.peek() != Tok::Ch(b']') {
                        return self.error(&["']'"]);
                    }
                    self.next();
                    if *self.peek() != Tok::Ch(b'=') {
                        return self.error(&["'='"]);
                    }
                    self.next();
                    let v = self.string_or_value()?;
                    self.pop_entry(&name.bytes(), v, Some(off.v));
                }
                _ => return self.error(&[]),
            }
        }
    }
}

/// Parse INI data into an array (php_simple_ini_parser_cb /
/// php_ini_parser_cb_with_sections). Returns false on a syntax error.
pub fn parse_ini(
    vm: &mut Vm,
    data: &[u8],
    process_sections: bool,
    mode: i64,
    filename: &str,
) -> Value {
    if !matches!(mode, SCANNER_NORMAL | SCANNER_RAW | SCANNER_TYPED) {
        vm.emit_warning("Invalid scanner mode");
        return Value::False;
    }
    let mut p = Parser {
        sc: Scanner {
            d: data,
            pos: 0,
            st: St::Initial,
            stack: Vec::new(),
            lineno: 1,
            mode,
        },
        la: None,
        vm,
        filename: filename.to_string(),
        process_sections,
        root: PhpArray::new(),
        section: None,
        events: None,
        unbuffered_error: None,
    };
    if p.parse().is_err() {
        return Value::False;
    }
    p.flush_section();
    Value::Array(Rc::new(RefCell::new(p.root)))
}

/// zend_parse_ini_string() with a parser callback (process_sections on):
/// the callbacks it makes, in order, or Err on a syntax error (reported as
/// a warning).
pub fn parse_ini_events(
    vm: &mut Vm,
    data: &[u8],
    mode: i64,
    filename: &str,
) -> Result<Vec<IniEvent>, Vec<IniEvent>> {
    let mut p = Parser {
        sc: Scanner {
            d: data,
            pos: 0,
            st: St::Initial,
            stack: Vec::new(),
            lineno: 1,
            mode,
        },
        la: None,
        vm,
        filename: filename.to_string(),
        process_sections: true,
        root: PhpArray::new(),
        section: None,
        events: Some(Vec::new()),
        unbuffered_error: None,
    };
    let r = p.parse();
    let ev = p.events.take().unwrap_or_default();
    if r.is_err() { Err(ev) } else { Ok(ev) }
}

/// php_init_config() / the -d entries: zend_parse_ini_string() with
/// unbuffered errors. The callbacks made before a syntax error, and the
/// error ("syntax error, ... in <file> on line N\n").
pub fn parse_ini_startup(
    vm: &mut Vm,
    data: &[u8],
    filename: &str,
) -> (Vec<IniEvent>, Option<String>) {
    let mut p = Parser {
        sc: Scanner {
            d: data,
            pos: 0,
            st: St::Initial,
            stack: Vec::new(),
            lineno: 1,
            mode: SCANNER_NORMAL,
        },
        la: None,
        vm,
        filename: filename.to_string(),
        process_sections: true,
        root: PhpArray::new(),
        section: None,
        events: Some(Vec::new()),
        unbuffered_error: Some(None),
    };
    let _ = p.parse();
    let ev = p.events.take().unwrap_or_default();
    (ev, p.unbuffered_error.flatten())
}
