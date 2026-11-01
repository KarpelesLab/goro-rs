//! Input decoding and the tokenizer: tags, text, comments, the doctype and
//! the pseudo elements the ext/tidy tests show (`<?php ?>`, `<% %>`, `<# #>`,
//! `<![CDATA[ ]]>`; tests 023.phpt, 025.phpt, bug77040.phpt).
//!
//! Every token carries the line and column of its first character, which is
//! what `tidyNode::$line`/`$column` and the messages report (tests 010.phpt,
//! 012.phpt).

use crate::config::Encoding;
use crate::{Attr, Level};

/// Characters of encodings handled as opaque bytes (raw, and the multibyte
/// and legacy encodings without a table here) live in a private use range
/// and go back to their byte on output.
pub const OPAQUE_BASE: u32 = 0xF700;

/// An `&` that opens a well-formed entity kept as written
/// (`preserve-entities`).
pub const KEPT_AMP: char = '\u{F6FF}';

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokKind {
    Text,
    Start,
    End,
    Comment,
    Doctype,
    ProcIns,
    Asp,
    Jste,
    Php,
    Cdata,
    Section,
    XmlDecl,
    Eof,
}

#[derive(Debug)]
pub struct Token {
    pub kind: TokKind,
    pub name: String,
    pub attrs: Vec<Attr>,
    pub text: String,
    pub self_closed: bool,
    pub line: u32,
    pub col: u32,
}

pub struct Lexer {
    c: Vec<char>,
    pos: usize,
    line: u32,
    col: u32,
    tab: u32,
    xml: bool,
    preserve_entities: bool,
    xml_procins: bool,
    pub msgs: Vec<(Level, u32, u32, String)>,
}

/// Windows-1252's 0x80 to 0x9F (the WHATWG Encoding Standard's index).
const WIN1252: [u32; 32] = [
    0x20AC, 0x81, 0x201A, 0x192, 0x201E, 0x2026, 0x2020, 0x2021, 0x2C6, 0x2030, 0x160, 0x2039,
    0x152, 0x8D, 0x17D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x2DC,
    0x2122, 0x161, 0x203A, 0x153, 0x9D, 0x17E, 0x178,
];

/// ISO 8859-15's differences from ISO 8859-1.
const LATIN0: [(u8, u32); 8] = [
    (0xA4, 0x20AC),
    (0xA6, 0x160),
    (0xA8, 0x161),
    (0xB4, 0x17D),
    (0xB8, 0x17E),
    (0xBC, 0x152),
    (0xBD, 0x153),
    (0xBE, 0x178),
];

/// The input as characters: line ends normalized to `\n`, and NUL and other
/// control characters removed (test 018.phpt shows a NUL dropped).
pub fn decode_input(input: &[u8], enc: Encoding) -> (Vec<char>, bool) {
    let mut invalid = false;
    let mut out: Vec<char> = Vec::with_capacity(input.len());
    match enc {
        Encoding::Utf8 => {
            let bytes = input.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(input);
            let mut rest = bytes;
            loop {
                match std::str::from_utf8(rest) {
                    Ok(s) => {
                        out.extend(s.chars());
                        break;
                    }
                    Err(e) => {
                        let (good, bad) = rest.split_at(e.valid_up_to());
                        out.extend(String::from_utf8_lossy(good).chars());
                        out.push('\u{FFFD}');
                        invalid = true;
                        rest = &bad[e.error_len().unwrap_or(bad.len())..];
                    }
                }
            }
        }
        Encoding::Latin1 => out.extend(input.iter().map(|&b| b as char)),
        Encoding::Latin0 => out.extend(input.iter().map(|&b| {
            let cp = LATIN0
                .iter()
                .find(|(x, _)| *x == b)
                .map_or(b as u32, |(_, c)| *c);
            char::from_u32(cp).unwrap_or('\u{FFFD}')
        })),
        Encoding::Win1252 | Encoding::Ascii => out.extend(input.iter().map(|&b| {
            if (0x80..0xA0).contains(&b) {
                char::from_u32(WIN1252[b as usize - 0x80]).unwrap_or('\u{FFFD}')
            } else {
                b as char
            }
        })),
        Encoding::Utf16 | Encoding::Utf16Le | Encoding::Utf16Be => {
            let (bytes, le) = match (enc, input) {
                (Encoding::Utf16Le, _) => (input, true),
                (Encoding::Utf16Be, _) => (input, false),
                (_, [0xFF, 0xFE, r @ ..]) => (r, true),
                (_, [0xFE, 0xFF, r @ ..]) => (r, false),
                _ => (input, false),
            };
            let units = bytes.as_chunks::<2>().0.iter().map(|&p| {
                if le {
                    u16::from_le_bytes(p)
                } else {
                    u16::from_be_bytes(p)
                }
            });
            for r in char::decode_utf16(units) {
                match r {
                    Ok(c) => out.push(c),
                    Err(_) => {
                        out.push('\u{FFFD}');
                        invalid = true;
                    }
                }
            }
        }
        Encoding::Raw
        | Encoding::Iso2022
        | Encoding::Mac
        | Encoding::Ibm858
        | Encoding::Big5
        | Encoding::ShiftJis => out.extend(input.iter().map(|&b| {
            if b < 0x80 {
                b as char
            } else {
                char::from_u32(OPAQUE_BASE + b as u32).unwrap_or('\u{FFFD}')
            }
        })),
    }
    let mut norm = Vec::with_capacity(out.len());
    let mut i = 0;
    while i < out.len() {
        let c = out[i];
        i += 1;
        match c {
            '\r' => {
                if out.get(i) == Some(&'\n') {
                    i += 1;
                }
                norm.push('\n');
            }
            '\n' | '\t' => norm.push(c),
            c if (c as u32) < 0x20 || c == '\u{7F}' => {}
            c => norm.push(c),
        }
    }
    (norm, invalid)
}

/// The output as bytes in the output encoding. Characters the encoding lacks
/// were written as character references by the printer.
pub fn encode_output(s: &str, enc: Encoding) -> Vec<u8> {
    fn opaque(c: char) -> Option<u8> {
        let v = c as u32;
        (OPAQUE_BASE + 0x80..OPAQUE_BASE + 0x100)
            .contains(&v)
            .then(|| (v - OPAQUE_BASE) as u8)
    }
    match enc {
        Encoding::Utf8 => {
            if s.chars().any(|c| opaque(c).is_some()) {
                let mut out = Vec::with_capacity(s.len());
                let mut buf = [0; 4];
                for c in s.chars() {
                    match opaque(c) {
                        Some(b) => out.push(b),
                        None => out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes()),
                    }
                }
                out
            } else {
                s.as_bytes().to_vec()
            }
        }
        Encoding::Utf16 | Encoding::Utf16Be => {
            let mut out = Vec::with_capacity(s.len() * 2 + 2);
            if enc == Encoding::Utf16 {
                out.extend_from_slice(&[0xFE, 0xFF]);
            }
            for u in s.encode_utf16() {
                out.extend_from_slice(&u.to_be_bytes());
            }
            out
        }
        Encoding::Utf16Le => {
            let mut out = Vec::with_capacity(s.len() * 2);
            for u in s.encode_utf16() {
                out.extend_from_slice(&u.to_le_bytes());
            }
            out
        }
        _ => {
            let mut out = Vec::with_capacity(s.len());
            let mut buf = [0; 4];
            for c in s.chars() {
                if let Some(b) = opaque(c) {
                    out.push(b);
                } else if let Some(b) = byte_in(c, enc) {
                    out.push(b);
                } else {
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
            }
            out
        }
    }
}

/// The byte of `c` in a single-byte encoding with a table here.
fn byte_in(c: char, enc: Encoding) -> Option<u8> {
    let v = c as u32;
    if v < 0x80 {
        return Some(v as u8);
    }
    match enc {
        Encoding::Latin1 => (v < 0x100).then_some(v as u8),
        Encoding::Latin0 => {
            if let Some((b, _)) = LATIN0.iter().find(|(_, u)| *u == v) {
                return Some(*b);
            }
            (v < 0x100 && !LATIN0.iter().any(|(b, _)| *b as u32 == v)).then_some(v as u8)
        }
        Encoding::Win1252 => {
            if let Some(i) = WIN1252.iter().position(|u| *u == v) {
                return Some(0x80 + i as u8);
            }
            (0xA0..0x100).contains(&v).then_some(v as u8)
        }
        _ => None,
    }
}

/// Whether the output encoding can write `c` as itself.
pub fn representable(c: char, enc: Encoding) -> bool {
    match enc {
        Encoding::Utf8 | Encoding::Utf16 | Encoding::Utf16Le | Encoding::Utf16Be => true,
        Encoding::Ascii => (c as u32) < 0x80,
        Encoding::Latin1 | Encoding::Latin0 | Encoding::Win1252 => byte_in(c, enc).is_some(),
        Encoding::Raw
        | Encoding::Iso2022
        | Encoding::Mac
        | Encoding::Ibm858
        | Encoding::Big5
        | Encoding::ShiftJis => (c as u32) < 0x80,
    }
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || (c as u32) >= 0x80
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-' | '.') || (c as u32) >= 0x80
}

impl Lexer {
    pub fn new(
        chars: Vec<char>,
        tab: u32,
        xml: bool,
        preserve_entities: bool,
        xml_procins: bool,
    ) -> Lexer {
        Lexer {
            c: chars,
            pos: 0,
            line: 1,
            col: 1,
            tab: tab.max(1),
            xml,
            preserve_entities,
            xml_procins,
            msgs: Vec::new(),
        }
    }

    fn peek(&self, off: usize) -> Option<char> {
        self.c.get(self.pos + off).copied()
    }

    fn at_eof(&self) -> bool {
        self.pos >= self.c.len()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek(0)?;
        self.pos += 1;
        match c {
            '\n' => {
                self.line += 1;
                self.col = 1;
            }
            '\t' => self.col = ((self.col - 1) / self.tab + 1) * self.tab + 1,
            _ => self.col += 1,
        }
        Some(c)
    }

    fn bump_n(&mut self, n: usize) {
        for _ in 0..n {
            self.bump();
        }
    }

    fn looking_at(&self, s: &str, ignore_case: bool) -> bool {
        s.chars().enumerate().all(|(i, x)| match self.peek(i) {
            Some(c) if ignore_case => c.eq_ignore_ascii_case(&x),
            Some(c) => c == x,
            None => false,
        })
    }

    /// The text up to `end`, which is consumed; `None` for the end when it's
    /// missing (the rest of the input is taken).
    fn until(&mut self, end: &str) -> (String, bool) {
        let mut s = String::new();
        while !self.at_eof() {
            if self.looking_at(end, false) {
                self.bump_n(end.chars().count());
                return (s, true);
            }
            if let Some(c) = self.bump() {
                s.push(c);
            }
        }
        (s, false)
    }

    fn token(&self, kind: TokKind, line: u32, col: u32) -> Token {
        Token {
            kind,
            name: String::new(),
            attrs: Vec::new(),
            text: String::new(),
            self_closed: false,
            line,
            col,
        }
    }

    /// The next token, in normal markup mode.
    pub fn next_token(&mut self) -> Token {
        let (line, col) = (self.line, self.col);
        if self.at_eof() {
            return self.token(TokKind::Eof, line, col);
        }
        if self.peek(0) == Some('<')
            && let Some(t) = self.markup(line, col)
        {
            return t;
        }
        self.text_token(line, col)
    }

    fn text_token(&mut self, line: u32, col: u32) -> Token {
        let mut raw = String::new();
        // A `<` that opens no markup is text.
        if self.peek(0) == Some('<') {
            self.msgs.push((
                Level::Warning,
                line,
                col,
                "unescaped < which should be written as &lt;".to_string(),
            ));
            raw.push('<');
            self.bump();
        }
        while let Some(c) = self.peek(0) {
            if c == '<' && self.opens_markup() {
                break;
            }
            raw.push(c);
            self.bump();
        }
        let mut t = self.token(TokKind::Text, line, col);
        t.text = self.decode_entities(&raw, line, col);
        t
    }

    fn opens_markup(&self) -> bool {
        match self.peek(1) {
            Some('/') => self.peek(2).is_some_and(is_name_start),
            Some('!') | Some('?') => true,
            Some('%') | Some('#') => !self.xml,
            Some(c) => is_name_start(c),
            None => false,
        }
    }

    fn markup(&mut self, line: u32, col: u32) -> Option<Token> {
        if !self.opens_markup() {
            return None;
        }
        Some(match self.peek(1)? {
            '/' => self.end_tag(line, col),
            '!' => self.bang(line, col),
            '?' => self.proc_ins(line, col),
            '%' => {
                self.bump_n(2);
                let (text, closed) = self.until("%>");
                if !closed {
                    self.warn(line, col, "unterminated <% ... %> section");
                }
                let mut t = self.token(TokKind::Asp, line, col);
                t.text = text;
                t
            }
            '#' => {
                self.bump_n(2);
                let (text, closed) = self.until("#>");
                if !closed {
                    self.warn(line, col, "unterminated <# ... #> section");
                }
                let mut t = self.token(TokKind::Jste, line, col);
                t.text = text;
                t
            }
            _ => self.start_tag(line, col),
        })
    }

    fn warn(&mut self, line: u32, col: u32, m: &str) {
        self.msgs.push((Level::Warning, line, col, m.to_string()));
    }

    fn name(&mut self) -> String {
        let mut n = String::new();
        while let Some(c) = self.peek(0) {
            if !is_name_char(c) {
                break;
            }
            n.push(c);
            self.bump();
        }
        if !self.xml {
            n.make_ascii_lowercase();
        }
        n
    }

    fn end_tag(&mut self, line: u32, col: u32) -> Token {
        self.bump_n(2);
        let name = self.name();
        // Anything up to the closing `>` is ignored; an end tag cut off by
        // the end of the input still counts (test 029.phpt ends in "</html").
        while let Some(c) = self.peek(0) {
            if c == '<' {
                self.warn(line, col, &format!("</{name}> missing '>' for end of tag"));
                break;
            }
            self.bump();
            if c == '>' {
                break;
            }
        }
        let mut t = self.token(TokKind::End, line, col);
        t.name = name;
        t
    }

    fn bang(&mut self, line: u32, col: u32) -> Token {
        if self.looking_at("<!--", false) {
            self.bump_n(4);
            let (text, closed) = self.until("-->");
            if !closed {
                self.warn(line, col, "missing end of comment");
            }
            let mut t = self.token(TokKind::Comment, line, col);
            t.text = text;
            return t;
        }
        if self.looking_at("<![CDATA[", false) {
            self.bump_n(9);
            let (text, _) = self.until("]]>");
            let mut t = self.token(TokKind::Cdata, line, col);
            t.text = text;
            return t;
        }
        if self.looking_at("<![", false) {
            self.bump_n(2);
            let (text, _) = self.until("]>");
            let mut t = self.token(TokKind::Section, line, col);
            t.text = format!("{text}]");
            return t;
        }
        if self.looking_at("<!doctype", true) {
            self.bump_n(9);
            let (text, _) = self.until(">");
            let mut t = self.token(TokKind::Doctype, line, col);
            t.text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            return t;
        }
        // Any other declaration is reported and dropped: an empty text token
        // takes its place.
        self.bump_n(2);
        let (text, _) = self.until(">");
        self.warn(
            line,
            col,
            &format!("discarding malformed declaration <!{text}>"),
        );
        self.token(TokKind::Text, line, col)
    }

    fn proc_ins(&mut self, line: u32, col: u32) -> Token {
        self.bump_n(2);
        if self.looking_at("php", true) {
            let (text, closed) = self.until("?>");
            if !closed {
                self.warn(line, col, "unterminated <?php ... ?> section");
            }
            let mut t = self.token(TokKind::Php, line, col);
            t.text = text;
            return t;
        }
        if self.looking_at("xml", true)
            && self.peek(3).is_none_or(|c| c.is_whitespace() || c == '?')
        {
            let (text, _) = self.until("?>");
            let mut t = self.token(TokKind::XmlDecl, line, col);
            t.text = text;
            return t;
        }
        // In HTML an SGML processing instruction ends at `>`; a trailing `?`
        // stays part of its text, so it prints back as written.
        let (text, _) = if self.xml || self.xml_procins {
            let (t, c) = self.until("?>");
            (format!("{t}?"), c)
        } else {
            self.until(">")
        };
        let mut t = self.token(TokKind::ProcIns, line, col);
        t.text = text;
        t
    }

    fn start_tag(&mut self, line: u32, col: u32) -> Token {
        self.bump();
        let name = self.name();
        let mut t = self.token(TokKind::Start, line, col);
        t.name = name;
        loop {
            while self.peek(0).is_some_and(char::is_whitespace) {
                self.bump();
            }
            match self.peek(0) {
                None => {
                    self.warn(
                        line,
                        col,
                        &format!("<{}> missing '>' for end of tag", t.name),
                    );
                    break;
                }
                Some('>') => {
                    self.bump();
                    break;
                }
                Some('/') => {
                    self.bump();
                    if self.peek(0) == Some('>') {
                        self.bump();
                        t.self_closed = true;
                        break;
                    }
                }
                Some('<') => {
                    self.warn(
                        line,
                        col,
                        &format!("<{}> missing '>' for end of tag", t.name),
                    );
                    break;
                }
                Some(_) => {
                    let (aline, acol) = (self.line, self.col);
                    if let Some(a) = self.attribute(aline, acol) {
                        t.attrs.push(a);
                    }
                }
            }
        }
        t
    }

    fn attribute(&mut self, line: u32, col: u32) -> Option<Attr> {
        let mut name = String::new();
        while let Some(c) = self.peek(0) {
            if c.is_whitespace() || matches!(c, '=' | '>' | '/' | '<') {
                break;
            }
            name.push(c);
            self.bump();
        }
        if name.is_empty() {
            // A stray `=`.
            self.bump();
            return None;
        }
        if !self.xml {
            name.make_ascii_lowercase();
        }
        let save = (self.pos, self.line, self.col);
        while self.peek(0).is_some_and(char::is_whitespace) {
            self.bump();
        }
        if self.peek(0) != Some('=') {
            (self.pos, self.line, self.col) = save;
            return Some(Attr { name, value: None });
        }
        self.bump();
        while self.peek(0).is_some_and(char::is_whitespace) {
            self.bump();
        }
        let mut raw = String::new();
        match self.peek(0) {
            Some(q @ ('"' | '\'')) => {
                self.bump();
                // Without a closing quote the value ends at the tag's end.
                let close = self.c[self.pos..].iter().position(|&c| c == q);
                match close {
                    Some(n) => {
                        for _ in 0..n {
                            if let Some(c) = self.bump() {
                                raw.push(c);
                            }
                        }
                        self.bump();
                    }
                    None => {
                        self.warn(
                            line,
                            col,
                            &format!("attribute \"{name}\" lacks closing quote"),
                        );
                        while let Some(c) = self.peek(0) {
                            if c == '>' {
                                break;
                            }
                            raw.push(c);
                            self.bump();
                        }
                    }
                }
            }
            _ => {
                while let Some(c) = self.peek(0) {
                    if c.is_whitespace() || c == '>' {
                        break;
                    }
                    raw.push(c);
                    self.bump();
                }
            }
        }
        let value = self.decode_entities(&raw, line, col);
        Some(Attr {
            name,
            value: Some(value),
        })
    }

    /// The raw text of an element up to its end tag, which isn't consumed.
    pub fn raw_until_end(&mut self, name: &str, decode: bool) -> (String, u32, u32) {
        let (line, col) = (self.line, self.col);
        let close: String = format!("</{name}");
        let mut raw = String::new();
        while !self.at_eof() {
            if self.peek(0) == Some('<') && self.looking_at(&close, true) {
                let after = self.peek(close.chars().count());
                if after.is_none_or(|c| c.is_whitespace() || c == '>') {
                    break;
                }
            }
            if let Some(c) = self.bump() {
                raw.push(c);
            }
        }
        let text = if decode {
            self.decode_entities(&raw, line, col)
        } else {
            raw
        };
        (text, line, col)
    }

    /// Replaces character references (HTML 4.01 section 5.3).
    fn decode_entities(&mut self, raw: &str, line: u32, col: u32) -> String {
        if !raw.contains('&') {
            return raw.to_string();
        }
        let mut out = String::with_capacity(raw.len());
        let mut rest = raw;
        while let Some(i) = rest.find('&') {
            out.push_str(&rest[..i]);
            let after = &rest[i + 1..];
            let (consumed, res) = self.one_entity(after, line, col);
            match res {
                Some(s) => out.push_str(&s),
                None => out.push('&'),
            }
            rest = &after[consumed..];
        }
        out.push_str(rest);
        out
    }

    /// One reference after its `&`: the bytes used and the replacement.
    fn one_entity(&mut self, s: &str, line: u32, col: u32) -> (usize, Option<String>) {
        if let Some(num) = s.strip_prefix('#') {
            let (radix, digits, skip) = match num.strip_prefix(['x', 'X']) {
                Some(h) => (16, h, 2),
                None => (10, num, 1),
            };
            let n = digits
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric())
                .count();
            let semi = digits[n..].starts_with(';');
            if let Ok(v) = u32::from_str_radix(&digits[..n], radix) {
                // Numeric references to C1 controls mean Windows-1252's
                // characters (HTML Standard, numeric character reference end
                // state).
                let v = if (0x80..0xA0).contains(&v) {
                    WIN1252[v as usize - 0x80]
                } else {
                    v
                };
                if let Some(c) = char::from_u32(v).filter(|c| *c != '\0') {
                    if !semi {
                        self.warn(
                            line,
                            col,
                            &format!(
                                "numeric character reference \"&{}\" doesn't end in ';'",
                                &s[..skip + n]
                            ),
                        );
                    }
                    let used = skip + n + semi as usize;
                    if self.preserve_entities && semi {
                        return (used, Some(format!("{KEPT_AMP}{}", &s[..used])));
                    }
                    return (used, Some(c.to_string()));
                }
            }
            self.warn(line, col, "unescaped & which should be written as &amp;");
            return (0, None);
        }
        let n = s.bytes().take_while(|b| b.is_ascii_alphanumeric()).count();
        if n == 0 {
            self.warn(line, col, "unescaped & which should be written as &amp;");
            return (0, None);
        }
        let name = &s[..n];
        let semi = s[n..].starts_with(';');
        match goro_xml::html::named_entity(name) {
            Some(c) => {
                if !semi {
                    self.warn(line, col, &format!("entity \"&{name}\" doesn't end in ';'"));
                }
                let used = n + semi as usize;
                if self.preserve_entities && semi {
                    return (used, Some(format!("{KEPT_AMP}{name};")));
                }
                (used, Some(c.to_string()))
            }
            None => {
                let shown = if semi {
                    format!("&{name};")
                } else {
                    format!("&{name}")
                };
                self.warn(
                    line,
                    col,
                    &format!("unescaped & or unknown entity \"{shown}\""),
                );
                (0, None)
            }
        }
    }
}
