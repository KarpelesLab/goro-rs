//! CSS selectors for querySelector(All)/matches/closest: a CSS Syntax 3
//! tokenizer, a Selectors 4 parser with lexbor's error messages, and the
//! matching rules of php-src's adapted lexbor engine
//! (ext/dom/lexbor/selectors-adapted/selectors.c).

use super::*;

// ---------------------------------------------------------------------------
// Tokenizer

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Function(String),
    AtKeyword(String),
    Hash(String),
    Str(String),
    BadStr,
    Url,
    BadUrl,
    Delim(char),
    Number(f64, bool),
    Percentage,
    Dimension(f64, bool, String),
    Ws,
    Cdo,
    Cdc,
    Colon,
    Semicolon,
    Comma,
    LBracket,
    RBracket,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Eof,
}

#[derive(Clone, Debug)]
struct Token {
    t: Tok,
    /// source text, for error messages
    raw: String,
}

struct Tokenizer<'a> {
    s: &'a [char],
    i: usize,
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || (c as u32) >= 0x80
}

fn is_name(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '-'
}

impl<'a> Tokenizer<'a> {
    fn peek(&self, k: usize) -> Option<char> {
        self.s.get(self.i + k).copied()
    }

    fn valid_escape(&self, k: usize) -> bool {
        self.peek(k) == Some('\\') && !matches!(self.peek(k + 1), None | Some('\n'))
    }

    fn starts_ident(&self, k: usize) -> bool {
        match self.peek(k) {
            Some('-') => {
                matches!(self.peek(k + 1), Some(c) if is_name_start(c) || c == '-')
                    || self.valid_escape(k + 1)
            }
            Some(c) if is_name_start(c) => true,
            Some('\\') => self.valid_escape(k),
            _ => false,
        }
    }

    fn starts_number(&self, k: usize) -> bool {
        match self.peek(k) {
            Some('+') | Some('-') => match self.peek(k + 1) {
                Some(c) if c.is_ascii_digit() => true,
                Some('.') => matches!(self.peek(k + 2), Some(c) if c.is_ascii_digit()),
                _ => false,
            },
            Some('.') => matches!(self.peek(k + 1), Some(c) if c.is_ascii_digit()),
            Some(c) => c.is_ascii_digit(),
            None => false,
        }
    }

    fn escape(&mut self) -> char {
        // the backslash is consumed already
        match self.peek(0) {
            None => '\u{FFFD}',
            Some(c) if c.is_ascii_hexdigit() => {
                let mut v = 0u32;
                let mut n = 0;
                while n < 6 {
                    match self.peek(0) {
                        Some(h) if h.is_ascii_hexdigit() => {
                            v = v * 16 + h.to_digit(16).unwrap();
                            self.i += 1;
                            n += 1;
                        }
                        _ => break,
                    }
                }
                if matches!(self.peek(0), Some(' ') | Some('\t') | Some('\n')) {
                    self.i += 1;
                }
                if v == 0 || (0xD800..=0xDFFF).contains(&v) || v > 0x10FFFF {
                    '\u{FFFD}'
                } else {
                    char::from_u32(v).unwrap_or('\u{FFFD}')
                }
            }
            Some(c) => {
                self.i += 1;
                c
            }
        }
    }

    fn name(&mut self) -> String {
        let mut out = String::new();
        loop {
            match self.peek(0) {
                Some(c) if is_name(c) => {
                    out.push(c);
                    self.i += 1;
                }
                Some('\\') if self.valid_escape(0) => {
                    self.i += 1;
                    out.push(self.escape());
                }
                _ => return out,
            }
        }
    }

    fn number(&mut self) -> (f64, bool) {
        let start = self.i;
        let mut int = true;
        if matches!(self.peek(0), Some('+') | Some('-')) {
            self.i += 1;
        }
        while matches!(self.peek(0), Some(c) if c.is_ascii_digit()) {
            self.i += 1;
        }
        if self.peek(0) == Some('.') && matches!(self.peek(1), Some(c) if c.is_ascii_digit()) {
            int = false;
            self.i += 1;
            while matches!(self.peek(0), Some(c) if c.is_ascii_digit()) {
                self.i += 1;
            }
        }
        if matches!(self.peek(0), Some('e') | Some('E')) {
            let k = if matches!(self.peek(1), Some('+') | Some('-')) {
                2
            } else {
                1
            };
            if matches!(self.peek(k), Some(c) if c.is_ascii_digit()) {
                int = false;
                self.i += k;
                while matches!(self.peek(0), Some(c) if c.is_ascii_digit()) {
                    self.i += 1;
                }
            }
        }
        let s: String = self.s[start..self.i].iter().collect();
        (s.parse::<f64>().unwrap_or(0.0), int)
    }

    fn string(&mut self, q: char) -> Tok {
        let mut out = String::new();
        loop {
            match self.peek(0) {
                None => return Tok::Str(out),
                Some(c) if c == q => {
                    self.i += 1;
                    return Tok::Str(out);
                }
                Some('\n') => return Tok::BadStr,
                Some('\\') => match self.peek(1) {
                    None => self.i += 1,
                    Some('\n') => self.i += 2,
                    _ => {
                        self.i += 1;
                        out.push(self.escape());
                    }
                },
                Some(c) => {
                    out.push(c);
                    self.i += 1;
                }
            }
        }
    }

    fn next(&mut self) -> Token {
        let start = self.i;
        let t = self.next_tok();
        let raw: String = self.s[start..self.i].iter().collect();
        Token { t, raw }
    }

    fn next_tok(&mut self) -> Tok {
        // comments
        while self.peek(0) == Some('/') && self.peek(1) == Some('*') {
            self.i += 2;
            while self.i < self.s.len() && !(self.peek(0) == Some('*') && self.peek(1) == Some('/'))
            {
                self.i += 1;
            }
            self.i = (self.i + 2).min(self.s.len());
        }
        let c = match self.peek(0) {
            None => return Tok::Eof,
            Some(c) => c,
        };
        match c {
            ' ' | '\t' | '\n' | '\r' | '\x0C' => {
                while matches!(
                    self.peek(0),
                    Some(' ') | Some('\t') | Some('\n') | Some('\r') | Some('\x0C')
                ) {
                    self.i += 1;
                }
                Tok::Ws
            }
            '"' | '\'' => {
                self.i += 1;
                self.string(c)
            }
            '#' => {
                if matches!(self.peek(1), Some(x) if is_name(x)) || self.valid_escape(1) {
                    self.i += 1;
                    Tok::Hash(self.name())
                } else {
                    self.i += 1;
                    Tok::Delim('#')
                }
            }
            '(' => {
                self.i += 1;
                Tok::LParen
            }
            ')' => {
                self.i += 1;
                Tok::RParen
            }
            '+' | '.' if self.starts_number(0) => self.numeric(),
            '-' if self.starts_number(0) => self.numeric(),
            '-' if self.peek(1) == Some('-') && self.peek(2) == Some('>') => {
                self.i += 3;
                Tok::Cdc
            }
            '-' if self.starts_ident(0) => self.ident_like(),
            ',' => {
                self.i += 1;
                Tok::Comma
            }
            ':' => {
                self.i += 1;
                Tok::Colon
            }
            ';' => {
                self.i += 1;
                Tok::Semicolon
            }
            '<' if self.peek(1) == Some('!')
                && self.peek(2) == Some('-')
                && self.peek(3) == Some('-') =>
            {
                self.i += 4;
                Tok::Cdo
            }
            '@' => {
                if self.starts_ident(1) {
                    self.i += 1;
                    Tok::AtKeyword(self.name())
                } else {
                    self.i += 1;
                    Tok::Delim('@')
                }
            }
            '[' => {
                self.i += 1;
                Tok::LBracket
            }
            ']' => {
                self.i += 1;
                Tok::RBracket
            }
            '{' => {
                self.i += 1;
                Tok::LBrace
            }
            '}' => {
                self.i += 1;
                Tok::RBrace
            }
            '\\' => {
                if self.valid_escape(0) {
                    self.ident_like()
                } else {
                    self.i += 1;
                    Tok::Delim('\\')
                }
            }
            c if c.is_ascii_digit() => self.numeric(),
            c if is_name_start(c) => self.ident_like(),
            c => {
                self.i += 1;
                Tok::Delim(c)
            }
        }
    }

    fn numeric(&mut self) -> Tok {
        let (v, int) = self.number();
        if self.starts_ident(0) {
            let unit = self.name();
            Tok::Dimension(v, int, unit)
        } else if self.peek(0) == Some('%') {
            self.i += 1;
            Tok::Percentage
        } else {
            Tok::Number(v, int)
        }
    }

    fn ident_like(&mut self) -> Tok {
        let name = self.name();
        if self.peek(0) == Some('(') {
            self.i += 1;
            if name.eq_ignore_ascii_case("url") {
                // url( with an unquoted argument
                while matches!(self.peek(0), Some(' ') | Some('\t') | Some('\n')) {
                    self.i += 1;
                }
                if !matches!(self.peek(0), Some('"') | Some('\'')) {
                    loop {
                        match self.peek(0) {
                            None => return Tok::Url,
                            Some(')') => {
                                self.i += 1;
                                return Tok::Url;
                            }
                            Some('\\') if self.valid_escape(0) => {
                                self.i += 1;
                                self.escape();
                            }
                            Some('"') | Some('\'') | Some('(') => {
                                while !matches!(self.peek(0), None | Some(')')) {
                                    self.i += 1;
                                }
                                if self.peek(0).is_some() {
                                    self.i += 1;
                                }
                                return Tok::BadUrl;
                            }
                            _ => self.i += 1,
                        }
                    }
                }
            }
            return Tok::Function(name);
        }
        Tok::Ident(name)
    }
}

fn tokenize(s: &str) -> Vec<Token> {
    let chars: Vec<char> = s
        .chars()
        .map(|c| if c == '\0' { '\u{FFFD}' } else { c })
        .collect();
    let chars: Vec<char> = {
        // preprocessing: CR LF / CR / FF -> LF
        let mut out = Vec::with_capacity(chars.len());
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '\r' => {
                    out.push('\n');
                    if chars.get(i + 1) == Some(&'\n') {
                        i += 1;
                    }
                }
                '\x0C' => out.push('\n'),
                c => out.push(c),
            }
            i += 1;
        }
        out
    };
    let mut tz = Tokenizer { s: &chars, i: 0 };
    let mut out = Vec::new();
    loop {
        let t = tz.next();
        let eof = t.t == Tok::Eof;
        out.push(t);
        if eof {
            break;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Selector model

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Comb {
    Descendant,
    Child,
    Next,
    Subsequent,
    /// the column combinator `||` (parsed, but unsupported when matching)
    Column,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum AttrOp {
    Eq,
    Include,
    Dash,
    Prefix,
    Suffix,
    Substring,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum PC {
    Active,
    AnyLink,
    Blank,
    Checked,
    Disabled,
    Empty,
    Enabled,
    FirstChild,
    FirstOfType,
    Focus,
    Hover,
    LastChild,
    LastOfType,
    Link,
    OnlyChild,
    OnlyOfType,
    Optional,
    PlaceholderShown,
    ReadOnly,
    ReadWrite,
    Required,
    Root,
}

#[derive(Clone, Debug)]
enum Simple {
    Any,
    Type(String),
    Id(String),
    Class(String),
    Attr {
        name: String,
        op: Option<(AttrOp, String, bool)>,
    },
    Pseudo(PC),
    Not(List),
    Is(List),
    Has(List),
    NthChild {
        a: i64,
        b: i64,
        of: Option<List>,
        last: bool,
    },
    NthOfType {
        a: i64,
        b: i64,
        last: bool,
    },
}

#[derive(Clone, Debug, Default)]
struct Compound(Vec<Simple>);

/// A complex selector: compounds from left to right; the combinator of the
/// first compound is the relative combinator (for :has()).
#[derive(Clone, Debug)]
struct Complex(Vec<(Comb, Compound)>);

#[derive(Clone, Debug, Default)]
pub struct List(Vec<Complex>);

// ---------------------------------------------------------------------------
// Parser

struct Parser {
    toks: Vec<Token>,
    i: usize,
}

type PErr = String;

fn unexpected(t: &Token) -> PErr {
    let s = match t.t {
        Tok::Eof => "END-OF-FILE".to_string(),
        // (lexbor serializes a number token from its double value)
        Tok::Number(v, _) => number_text(v),
        _ => t.raw.clone(),
    };
    format!("Selectors. Unexpected token: {}", s)
}

/// The shortest round-trip digits of `v`, written out without an exponent
/// below 1e21 (9223372036854775807 reads back as 9223372036854776000).
fn number_text(v: f64) -> String {
    if !v.is_finite() {
        return v.to_string();
    }
    let e = format!("{:e}", v);
    let (mant, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let neg = mant.starts_with('-');
    let digits: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    let n = digits.len() as i32;
    let body = if !(-7..21).contains(&exp) {
        return format!("{}e{}{}", mant, if exp < 0 { "-" } else { "+" }, exp.abs());
    } else if exp >= n - 1 {
        format!("{}{}", digits, "0".repeat((exp - (n - 1)) as usize))
    } else if exp >= 0 {
        let (a, b) = digits.split_at((exp + 1) as usize);
        format!("{}.{}", a, b)
    } else {
        format!("0.{}{}", "0".repeat((-exp - 1) as usize), digits)
    };
    if neg { format!("-{}", body) } else { body }
}

fn not_supported(name: &str) -> PErr {
    format!("Selectors. Not supported: {}", name)
}

impl Parser {
    fn cur(&self) -> &Token {
        &self.toks[self.i.min(self.toks.len() - 1)]
    }

    fn bump(&mut self) {
        if self.i < self.toks.len() - 1 {
            self.i += 1;
        }
    }

    fn skip_ws(&mut self) {
        while self.cur().t == Tok::Ws {
            self.bump();
        }
    }

    fn err(&self) -> PErr {
        unexpected(self.cur())
    }

    /// selector list up to `end` (Eof or RParen)
    fn list(&mut self, relative: bool, forgiving: bool) -> Result<List, PErr> {
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            let c = self.complex(relative)?;
            out.push(c);
            self.skip_ws();
            match self.cur().t {
                Tok::Comma => {
                    self.bump();
                }
                Tok::Eof | Tok::RParen => break,
                _ => return Err(self.err()),
            }
        }
        let _ = forgiving;
        Ok(List(out))
    }

    fn combinator(&mut self) -> Option<Comb> {
        match self.cur().t {
            Tok::Delim('>') => {
                self.bump();
                Some(Comb::Child)
            }
            Tok::Delim('+') => {
                self.bump();
                Some(Comb::Next)
            }
            Tok::Delim('~') => {
                self.bump();
                Some(Comb::Subsequent)
            }
            Tok::Delim('|')
                if self
                    .toks
                    .get(self.i + 1)
                    .map(|t| t.t == Tok::Delim('|'))
                    .unwrap_or(false) =>
            {
                self.bump();
                self.bump();
                Some(Comb::Column)
            }
            _ => None,
        }
    }

    fn complex(&mut self, relative: bool) -> Result<Complex, PErr> {
        let mut parts = Vec::new();
        let mut comb = Comb::Descendant;
        if relative {
            if let Some(c) = self.combinator() {
                comb = c;
                self.skip_ws();
            }
        }
        loop {
            let comp = self.compound()?;
            parts.push((comb, comp));
            // combinator?
            let had_ws = self.cur().t == Tok::Ws;
            self.skip_ws();
            match self.cur().t {
                Tok::Eof | Tok::Comma | Tok::RParen => break,
                _ => {}
            }
            comb = match self.combinator() {
                Some(c) => {
                    self.skip_ws();
                    c
                }
                None => {
                    if had_ws {
                        Comb::Descendant
                    } else {
                        return Err(self.err());
                    }
                }
            };
        }
        Ok(Complex(parts))
    }

    /// `ns|` prefix handling: returns the local name part (namespaces are
    /// not checked by the matching engine).
    fn type_selector(&mut self) -> Result<Option<Simple>, PErr> {
        let first = self.cur().clone();
        match &first.t {
            Tok::Ident(n) => {
                self.bump();
                if self.cur().t == Tok::Delim('|') {
                    self.bump();
                    return self.type_after_ns();
                }
                Ok(Some(Simple::Type(n.clone())))
            }
            Tok::Delim('*') => {
                self.bump();
                if self.cur().t == Tok::Delim('|') {
                    self.bump();
                    return self.type_after_ns();
                }
                Ok(Some(Simple::Any))
            }
            Tok::Delim('|') => {
                self.bump();
                self.type_after_ns()
            }
            _ => Ok(None),
        }
    }

    fn type_after_ns(&mut self) -> Result<Option<Simple>, PErr> {
        match self.cur().t.clone() {
            Tok::Ident(n) => {
                self.bump();
                Ok(Some(Simple::Type(n)))
            }
            Tok::Delim('*') => {
                self.bump();
                Ok(Some(Simple::Any))
            }
            _ => Err(self.err()),
        }
    }

    fn compound(&mut self) -> Result<Compound, PErr> {
        let mut out = Vec::new();
        if let Some(t) = self.type_selector()? {
            out.push(t);
        }
        loop {
            match self.cur().t.clone() {
                Tok::Hash(h) => {
                    self.bump();
                    out.push(Simple::Id(h));
                }
                Tok::Delim('.') => {
                    self.bump();
                    match self.cur().t.clone() {
                        Tok::Ident(n) => {
                            self.bump();
                            out.push(Simple::Class(n));
                        }
                        _ => return Err(self.err()),
                    }
                }
                Tok::LBracket => {
                    self.bump();
                    out.push(self.attribute()?);
                }
                Tok::Colon => {
                    self.bump();
                    if self.cur().t == Tok::Colon {
                        self.bump();
                        return Err(self.pseudo_element());
                    }
                    out.push(self.pseudo_class()?);
                }
                _ => break,
            }
        }
        if out.is_empty() {
            return Err(self.err());
        }
        Ok(Compound(out))
    }

    fn pseudo_element(&mut self) -> PErr {
        match self.cur().t.clone() {
            Tok::Ident(n) => {
                let l = n.to_ascii_lowercase();
                if matches!(
                    l.as_str(),
                    "after"
                        | "backdrop"
                        | "before"
                        | "first-letter"
                        | "first-line"
                        | "grammar-error"
                        | "inactive-selection"
                        | "marker"
                        | "placeholder"
                        | "selection"
                        | "spelling-error"
                        | "target-text"
                ) {
                    not_supported(&n)
                } else {
                    self.err()
                }
            }
            _ => self.err(),
        }
    }

    fn attribute(&mut self) -> Result<Simple, PErr> {
        self.skip_ws();
        // optional namespace prefix
        let name = match self.cur().t.clone() {
            Tok::Ident(n) => {
                self.bump();
                if self.cur().t == Tok::Delim('|') {
                    // ns|attr (but not |=)
                    self.bump();
                    match self.cur().t.clone() {
                        Tok::Ident(n2) => {
                            self.bump();
                            n2
                        }
                        Tok::Delim('=') => {
                            // was "|=" after a name
                            self.i -= 1;
                            n
                        }
                        _ => return Err(self.err()),
                    }
                } else {
                    n
                }
            }
            Tok::Delim('*') => {
                let star = self.cur().clone();
                self.bump();
                if self.cur().t != Tok::Delim('|') {
                    return Err(unexpected(&star));
                }
                self.bump();
                match self.cur().t.clone() {
                    Tok::Ident(n2) => {
                        self.bump();
                        n2
                    }
                    _ => return Err(unexpected(&star)),
                }
            }
            Tok::Delim('|') => {
                self.bump();
                match self.cur().t.clone() {
                    Tok::Ident(n2) => {
                        self.bump();
                        n2
                    }
                    _ => return Err(self.err()),
                }
            }
            _ => return Err(self.err()),
        };
        self.skip_ws();
        let op = match self.cur().t.clone() {
            Tok::RBracket => {
                self.bump();
                return Ok(Simple::Attr { name, op: None });
            }
            Tok::Delim('=') => {
                self.bump();
                AttrOp::Eq
            }
            Tok::Delim(c @ ('~' | '|' | '^' | '$' | '*')) => {
                self.bump();
                if self.cur().t != Tok::Delim('=') {
                    return Err(self.err());
                }
                self.bump();
                match c {
                    '~' => AttrOp::Include,
                    '|' => AttrOp::Dash,
                    '^' => AttrOp::Prefix,
                    '$' => AttrOp::Suffix,
                    _ => AttrOp::Substring,
                }
            }
            _ => return Err(self.err()),
        };
        self.skip_ws();
        let value = match self.cur().t.clone() {
            Tok::Ident(v) | Tok::Str(v) => {
                self.bump();
                v
            }
            _ => return Err(self.err()),
        };
        self.skip_ws();
        let mut insensitive = false;
        if let Tok::Ident(m) = self.cur().t.clone() {
            if m.eq_ignore_ascii_case("i") {
                insensitive = true;
            } else if !m.eq_ignore_ascii_case("s") {
                return Err(self.err());
            }
            self.bump();
            self.skip_ws();
        }
        if self.cur().t != Tok::RBracket {
            return Err(self.err());
        }
        self.bump();
        Ok(Simple::Attr {
            name,
            op: Some((op, value, insensitive)),
        })
    }

    fn pseudo_class(&mut self) -> Result<Simple, PErr> {
        let tok = self.cur().clone();
        match tok.t.clone() {
            Tok::Ident(n) => {
                let l = n.to_ascii_lowercase();
                let pc = match l.as_str() {
                    "active" => PC::Active,
                    "any-link" => PC::AnyLink,
                    "blank" => PC::Blank,
                    "checked" => PC::Checked,
                    "disabled" => PC::Disabled,
                    "empty" => PC::Empty,
                    "enabled" => PC::Enabled,
                    "first-child" => PC::FirstChild,
                    "first-of-type" => PC::FirstOfType,
                    "focus" => PC::Focus,
                    "hover" => PC::Hover,
                    "last-child" => PC::LastChild,
                    "last-of-type" => PC::LastOfType,
                    "link" => PC::Link,
                    "only-child" => PC::OnlyChild,
                    "only-of-type" => PC::OnlyOfType,
                    "optional" => PC::Optional,
                    "placeholder-shown" => PC::PlaceholderShown,
                    "read-only" => PC::ReadOnly,
                    "read-write" => PC::ReadWrite,
                    "required" => PC::Required,
                    "root" => PC::Root,
                    "current" | "default" | "focus-visible" | "focus-within" | "fullscreen"
                    | "future" | "in-range" | "indeterminate" | "invalid" | "local-link"
                    | "out-of-range" | "past" | "scope" | "target" | "target-within"
                    | "user-invalid" | "valid" | "visited" | "warning" => {
                        return Err(not_supported(&n));
                    }
                    _ => return Err(self.err()),
                };
                self.bump();
                Ok(Simple::Pseudo(pc))
            }
            Tok::Function(n) => {
                let l = n.to_ascii_lowercase();
                match l.as_str() {
                    "dir" | "lang" | "nth-col" | "nth-last-col" => Err(not_supported(&n)),
                    "not" | "is" | "where" | "current" | "has" => {
                        self.bump();
                        let list = self.list(l == "has", l != "not")?;
                        if self.cur().t != Tok::RParen {
                            return Err(self.err());
                        }
                        self.bump();
                        Ok(match l.as_str() {
                            "not" => Simple::Not(list),
                            "has" => Simple::Has(list),
                            _ => Simple::Is(list),
                        })
                    }
                    "nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type" => {
                        self.bump();
                        self.skip_ws();
                        let (a, b) = self.anb()?;
                        self.skip_ws();
                        let mut of = None;
                        if l == "nth-child" || l == "nth-last-child" {
                            if let Tok::Ident(o) = self.cur().t.clone() {
                                if o.eq_ignore_ascii_case("of") {
                                    self.bump();
                                    of = Some(self.list(false, false)?);
                                }
                            }
                        }
                        self.skip_ws();
                        if self.cur().t != Tok::RParen {
                            return Err(self.err());
                        }
                        self.bump();
                        Ok(match l.as_str() {
                            "nth-child" => Simple::NthChild {
                                a,
                                b,
                                of,
                                last: false,
                            },
                            "nth-last-child" => Simple::NthChild {
                                a,
                                b,
                                of,
                                last: true,
                            },
                            "nth-of-type" => Simple::NthOfType { a, b, last: false },
                            _ => Simple::NthOfType { a, b, last: true },
                        })
                    }
                    _ => Err(self.err()),
                }
            }
            _ => Err(self.err()),
        }
    }

    /// An+B microsyntax
    fn anb(&mut self) -> Result<(i64, i64), PErr> {
        let t = self.cur().clone();
        let (a, mut b_part): (i64, Option<String>);
        match &t.t {
            Tok::Ident(s_) if s_.eq_ignore_ascii_case("odd") => {
                self.bump();
                return Ok((2, 1));
            }
            Tok::Ident(s_) if s_.eq_ignore_ascii_case("even") => {
                self.bump();
                return Ok((2, 0));
            }
            Tok::Number(v, true) => {
                self.bump();
                return Ok((0, *v as i64));
            }
            Tok::Dimension(v, true, unit) => {
                let ul = unit.to_ascii_lowercase();
                if ul == "n" {
                    a = *v as i64;
                    b_part = None;
                } else if let Some(rest) = ul.strip_prefix("n-") {
                    if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()) {
                        return Err(self.err());
                    }
                    a = *v as i64;
                    b_part = Some(format!("-{}", rest));
                } else if ul == "n-" {
                    a = *v as i64;
                    b_part = Some("-".into());
                } else {
                    return Err(self.err());
                }
                self.bump();
            }
            Tok::Ident(s_) => {
                let l = s_.to_ascii_lowercase();
                let (sign, rest) = if let Some(r) = l.strip_prefix('-') {
                    (-1, r.to_string())
                } else {
                    (1, l.clone())
                };
                if rest == "n" {
                    a = sign;
                    b_part = None;
                } else if rest == "n-" {
                    a = sign;
                    b_part = Some("-".into());
                } else if let Some(d) = rest.strip_prefix("n-") {
                    if d.is_empty() || !d.chars().all(|c| c.is_ascii_digit()) {
                        return Err(self.err());
                    }
                    a = sign;
                    b_part = Some(format!("-{}", d));
                } else {
                    return Err(self.err());
                }
                self.bump();
            }
            Tok::Delim('+') => {
                self.bump();
                match self.cur().t.clone() {
                    Tok::Ident(s_) => {
                        let l = s_.to_ascii_lowercase();
                        if l == "n" {
                            a = 1;
                            b_part = None;
                        } else if l == "n-" {
                            a = 1;
                            b_part = Some("-".into());
                        } else if let Some(d) = l.strip_prefix("n-") {
                            a = 1;
                            b_part = Some(format!("-{}", d));
                        } else {
                            return Err(self.err());
                        }
                        self.bump();
                    }
                    _ => return Err(self.err()),
                }
            }
            _ => return Err(self.err()),
        }
        // b
        match b_part.take() {
            Some(s_) if s_ != "-" => return Ok((a, s_.parse::<i64>().unwrap_or(0))),
            Some(_) => {
                // "n-" followed by a number
                self.skip_ws();
                match self.cur().t.clone() {
                    Tok::Number(v, true) if v >= 0.0 => {
                        self.bump();
                        return Ok((a, -(v as i64)));
                    }
                    _ => return Err(self.err()),
                }
            }
            None => {}
        }
        let save = self.i;
        self.skip_ws();
        match self.cur().t.clone() {
            Tok::Number(v, true)
                if self.cur().raw.starts_with('+') || self.cur().raw.starts_with('-') =>
            {
                self.bump();
                Ok((a, v as i64))
            }
            Tok::Delim(c @ ('+' | '-')) => {
                self.bump();
                self.skip_ws();
                match self.cur().t.clone() {
                    Tok::Number(v, true)
                        if !self.cur().raw.starts_with('+') && !self.cur().raw.starts_with('-') =>
                    {
                        self.bump();
                        Ok((a, if c == '-' { -(v as i64) } else { v as i64 }))
                    }
                    _ => Err(self.err()),
                }
            }
            _ => {
                self.i = save;
                Ok((a, 0))
            }
        }
    }
}

/// Parse a selector list; Err(message) as lexbor logs it.
pub fn parse(s_: &str) -> Result<List, String> {
    let toks = tokenize(s_);
    let mut p = Parser { toks, i: 0 };
    p.skip_ws();
    if p.cur().t == Tok::Eof {
        return Err(unexpected(p.cur()));
    }
    let l = p.list(false, false)?;
    if p.cur().t != Tok::Eof {
        return Err(p.err());
    }
    Ok(l)
}

// ---------------------------------------------------------------------------
// Matching

struct Ctx {
    quirks: bool,
    error: Option<(i64, String)>,
}

fn is_html_el(t: &Tree, n: NodeId) -> bool {
    ns_is(t, t.nodes[n].ns, XHTML_NS)
}

fn html_name(t: &Tree, n: NodeId, name: &str) -> bool {
    t.nodes[n].name == name
}

/// lxb_selectors_adapted_attr: an attribute without namespace by name
fn adapted_attr(t: &Tree, n: NodeId, name: &str) -> Option<NodeId> {
    let a = if ns_is_html_and_doc_is_html(t, n) {
        t.attrs_of(n)
            .into_iter()
            .find(|&a| t.nodes[a].name == name.to_ascii_lowercase())
    } else {
        t.has_prop(n, name)
    };
    a.filter(|&a| t.nodes[a].ns.is_none())
}

fn has_attr(t: &Tree, n: NodeId, name: &str) -> bool {
    adapted_attr(t, n, name).is_some()
}

fn eq_ci(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn match_class(target: &str, src: &str, ins: bool) -> bool {
    if src.is_empty() {
        return false;
    }
    target
        .split(['\t', '\n', '\x0C', '\r', ' '])
        .any(|tok| if ins { eq_ci(tok, src) } else { tok == src })
}

fn starts_ci(s_: &str, p: &str) -> bool {
    s_.len() >= p.len() && s_.as_bytes()[..p.len()].eq_ignore_ascii_case(p.as_bytes())
}

fn match_attr_value(op: AttrOp, ins: bool, trg: &str, src: &str) -> bool {
    let eq = |a: &str, b: &str| if ins { eq_ci(a, b) } else { a == b };
    match op {
        AttrOp::Eq => eq(trg, src),
        AttrOp::Include => match_class(trg, src, ins),
        AttrOp::Dash => {
            if trg.len() == src.len() {
                return eq(trg, src);
            }
            trg.len() > src.len()
                && trg.is_char_boundary(src.len())
                && eq(&trg[..src.len()], src)
                && trg.as_bytes()[src.len()] == b'-'
        }
        AttrOp::Prefix => {
            !src.is_empty()
                && trg.len() >= src.len()
                && if ins {
                    starts_ci(trg, src)
                } else {
                    trg.starts_with(src)
                }
        }
        AttrOp::Suffix => {
            !src.is_empty() && trg.len() >= src.len() && {
                let tail = &trg.as_bytes()[trg.len() - src.len()..];
                if ins {
                    tail.eq_ignore_ascii_case(src.as_bytes())
                } else {
                    tail == src.as_bytes()
                }
            }
        }
        AttrOp::Substring => {
            !src.is_empty()
                && if ins {
                    trg.to_ascii_lowercase().contains(&src.to_ascii_lowercase())
                } else {
                    trg.contains(src)
                }
        }
    }
}

const LOWERCASED_HTML_ATTRS: &[&str] = &[
    "accept",
    "accept-charset",
    "align",
    "alink",
    "axis",
    "bgcolor",
    "charset",
    "checked",
    "clear",
    "codetype",
    "color",
    "compact",
    "declare",
    "defer",
    "dir",
    "direction",
    "disabled",
    "enctype",
    "face",
    "frame",
    "hreflang",
    "http-equiv",
    "lang",
    "language",
    "link",
    "media",
    "method",
    "multiple",
    "nohref",
    "noresize",
    "noshade",
    "nowrap",
    "readonly",
    "rel",
    "rev",
    "rules",
    "scope",
    "scrolling",
    "selected",
    "shape",
    "target",
    "text",
    "type",
    "valign",
    "valuetype",
    "vlink",
];

fn disabled(t: &Tree, n: NodeId) -> bool {
    if !is_html_el(t, n) {
        return false;
    }
    let name = t.nodes[n].name.as_str();
    if has_attr(t, n, "disabled")
        && matches!(
            name,
            "button" | "input" | "select" | "textarea" | "optgroup" | "fieldset"
        )
    {
        return true;
    }
    if name == "fieldset" {
        let fieldset = n;
        let mut cur = t.nodes[n].parent;
        while let Some(x) = cur {
            if t.nodes[x].t != NType::Element {
                break;
            }
            if is_html_el(t, x) && html_name(t, x, "fieldset") && has_attr(t, x, "disabled") {
                let mut sc = t.nodes[x].children;
                while let Some(s_) = sc {
                    if t.nodes[s_].t == NType::Element
                        && is_html_el(t, s_)
                        && html_name(t, s_, "legend")
                    {
                        let mut inner = Some(fieldset);
                        while let Some(i) = inner {
                            if i == s_ {
                                return false;
                            }
                            inner = t.nodes[i].parent;
                        }
                        return true;
                    }
                    sc = t.nodes[s_].next;
                }
            }
            cur = t.nodes[x].parent;
        }
    }
    false
}

fn same_type(t: &Tree, a: NodeId, b: NodeId) -> bool {
    t.nodes[a].t == NType::Element
        && t.nodes[a].name == t.nodes[b].name
        && (t.nodes[a].ns == t.nodes[b].ns
            || matches!((t.nodes[a].ns, t.nodes[b].ns), (Some(x), Some(y)) if t.nss[x].href == t.nss[y].href))
}

fn first_child(t: &Tree, n: NodeId) -> bool {
    let mut p = t.nodes[n].prev;
    while let Some(x) = p {
        if t.nodes[x].t == NType::Element {
            return false;
        }
        p = t.nodes[x].prev;
    }
    true
}

fn last_child(t: &Tree, n: NodeId) -> bool {
    let mut p = t.nodes[n].next;
    while let Some(x) = p {
        if t.nodes[x].t == NType::Element {
            return false;
        }
        p = t.nodes[x].next;
    }
    true
}

fn first_of_type(t: &Tree, n: NodeId) -> bool {
    let mut p = t.nodes[n].prev;
    while let Some(x) = p {
        if same_type(t, x, n) {
            return false;
        }
        p = t.nodes[x].prev;
    }
    true
}

fn last_of_type(t: &Tree, n: NodeId) -> bool {
    let mut p = t.nodes[n].next;
    while let Some(x) = p {
        if same_type(t, x, n) {
            return false;
        }
        p = t.nodes[x].next;
    }
    true
}

fn read_write(t: &Tree, n: NodeId) -> bool {
    if is_html_el(t, n) {
        if html_name(t, n, "input") || html_name(t, n, "textarea") {
            return !has_attr(t, n, "readonly") && !has_attr(t, n, "disabled");
        }
        return match adapted_attr(t, n, "contenteditable") {
            Some(a) => t.node_get_content(a) != "false",
            None => false,
        };
    }
    false
}

fn anb_calc(a: i64, b: i64, index: i64) -> bool {
    if a == 0 {
        return b >= 0 && b == index;
    }
    let num = (index as f64 - b as f64) / a as f64;
    num >= 0.0 && num.fract() == 0.0
}

fn pseudo(ctx: &mut Ctx, t: &Tree, n: NodeId, pc: PC) -> bool {
    match pc {
        PC::Active | PC::Focus | PC::Hover => false,
        PC::AnyLink | PC::Link => {
            is_html_el(t, n)
                && (html_name(t, n, "a") || html_name(t, n, "area"))
                && has_attr(t, n, "href")
        }
        PC::Blank => {
            if ctx.error.is_none() {
                ctx.error = Some((NOT_SUPPORTED_ERR, ":blank selector is not implemented because CSSWG has not yet decided its semantics (https://github.com/w3c/csswg-drafts/issues/1967)".into()));
            }
            false
        }
        PC::Checked => {
            if !is_html_el(t, n) {
                return false;
            }
            if html_name(t, n, "input") {
                match adapted_attr(t, n, "type") {
                    Some(a) => {
                        let v = t.node_get_content(a);
                        (eq_ci(&v, "checkbox") || eq_ci(&v, "radio")) && has_attr(t, n, "checked")
                    }
                    None => false,
                }
            } else if html_name(t, n, "option") {
                has_attr(t, n, "selected")
            } else {
                false
            }
        }
        PC::Disabled => disabled(t, n),
        PC::Enabled => !disabled(t, n),
        PC::Empty => {
            let mut c = t.nodes[n].children;
            while let Some(x) = c {
                if !matches!(t.nodes[x].t, NType::Comment | NType::Pi) {
                    return false;
                }
                c = t.nodes[x].next;
            }
            true
        }
        PC::FirstChild => first_child(t, n),
        PC::FirstOfType => first_of_type(t, n),
        PC::LastChild => last_child(t, n),
        PC::LastOfType => last_of_type(t, n),
        PC::OnlyChild => first_child(t, n) && last_child(t, n),
        PC::OnlyOfType => first_of_type(t, n) && last_of_type(t, n),
        PC::Optional => {
            is_html_el(t, n)
                && matches!(t.nodes[n].name.as_str(), "input" | "select" | "textarea")
                && !has_attr(t, n, "required")
        }
        PC::Required => {
            is_html_el(t, n)
                && matches!(t.nodes[n].name.as_str(), "input" | "select" | "textarea")
                && has_attr(t, n, "required")
        }
        PC::PlaceholderShown => {
            is_html_el(t, n)
                && matches!(t.nodes[n].name.as_str(), "input" | "textarea")
                && has_attr(t, n, "placeholder")
        }
        PC::ReadOnly => !read_write(t, n),
        PC::ReadWrite => read_write(t, n),
        PC::Root => t.nodes[n]
            .parent
            .map(|p| {
                matches!(
                    t.nodes[p].t,
                    NType::DocumentFrag | NType::Document | NType::HtmlDocument
                )
            })
            .unwrap_or(false),
    }
}

fn simple(ctx: &mut Ctx, t: &Tree, n: NodeId, s_: &Simple) -> bool {
    match s_ {
        Simple::Any => true,
        Simple::Type(name) => {
            if ns_is_html_and_doc_is_html(t, n) {
                t.nodes[n].name == name.to_ascii_lowercase()
            } else {
                t.nodes[n].name == *name
            }
        }
        Simple::Id(id) => match adapted_attr(t, n, "id") {
            Some(a) => {
                let v = t.node_get_content(a);
                if ctx.quirks { eq_ci(&v, id) } else { v == *id }
            }
            None => false,
        },
        Simple::Class(c) => match adapted_attr(t, n, "class") {
            Some(a) => {
                let v = t.node_get_content(a);
                !v.is_empty() && match_class(&v, c, ctx.quirks)
            }
            None => false,
        },
        Simple::Attr { name, op } => {
            let a = match adapted_attr(t, n, name) {
                Some(a) => a,
                None => return false,
            };
            match op {
                None => true,
                Some((op, value, ins)) => {
                    let v = t.node_get_content(a);
                    let force = LOWERCASED_HTML_ATTRS.iter().any(|x| eq_ci(x, name))
                        && ns_is_html_and_doc_is_html(t, n);
                    match_attr_value(*op, *ins || force, &v, value)
                }
            }
        }
        Simple::Pseudo(pc) => pseudo(ctx, t, n, *pc),
        Simple::Not(l) => !matches_list(ctx, t, n, l),
        Simple::Is(l) => matches_list(ctx, t, n, l),
        Simple::Has(l) => l.0.iter().any(|c| has_relative(ctx, t, n, c)),
        Simple::NthChild { a, b, of, last } => {
            let mut index = 0i64;
            let mut cur = Some(n);
            while let Some(x) = cur {
                if t.nodes[x].t == NType::Element {
                    let counts = match of {
                        Some(l) => matches_list(ctx, t, x, l),
                        None => true,
                    };
                    if counts {
                        index += 1;
                    }
                }
                cur = if *last {
                    t.nodes[x].next
                } else {
                    t.nodes[x].prev
                };
            }
            if let Some(l) = of {
                if !matches_list(ctx, t, n, l) {
                    return false;
                }
            }
            anb_calc(*a, *b, index)
        }
        Simple::NthOfType { a, b, last } => {
            let mut index = 0i64;
            let mut cur = Some(n);
            while let Some(x) = cur {
                if same_type(t, x, n) {
                    index += 1;
                }
                cur = if *last {
                    t.nodes[x].next
                } else {
                    t.nodes[x].prev
                };
            }
            anb_calc(*a, *b, index)
        }
    }
}

fn compound(ctx: &mut Ctx, t: &Tree, n: NodeId, c: &Compound) -> bool {
    t.nodes[n].t == NType::Element && c.0.iter().all(|s_| simple(ctx, t, n, s_))
}

/// Does `n` match compounds[0..=idx] (right-to-left)?
fn complex_at(ctx: &mut Ctx, t: &Tree, n: NodeId, c: &Complex, idx: usize) -> bool {
    if !compound(ctx, t, n, &c.0[idx].1) {
        return false;
    }
    if idx == 0 {
        return true;
    }
    let comb = c.0[idx].0;
    match comb {
        Comb::Descendant => {
            let mut p = t.nodes[n].parent;
            while let Some(x) = p {
                if t.nodes[x].t == NType::Element && complex_at(ctx, t, x, c, idx - 1) {
                    return true;
                }
                p = t.nodes[x].parent;
            }
            false
        }
        Comb::Child => match t.nodes[n].parent {
            Some(p) if t.nodes[p].t == NType::Element => complex_at(ctx, t, p, c, idx - 1),
            _ => false,
        },
        Comb::Next => {
            let mut p = t.nodes[n].prev;
            while let Some(x) = p {
                if t.nodes[x].t == NType::Element {
                    return complex_at(ctx, t, x, c, idx - 1);
                }
                p = t.nodes[x].prev;
            }
            false
        }
        Comb::Subsequent => {
            let mut p = t.nodes[n].prev;
            while let Some(x) = p {
                if t.nodes[x].t == NType::Element && complex_at(ctx, t, x, c, idx - 1) {
                    return true;
                }
                p = t.nodes[x].prev;
            }
            false
        }
        Comb::Column => {
            ctx.error = Some((UNSUPPORTED, String::new()));
            false
        }
    }
}

fn matches_list(ctx: &mut Ctx, t: &Tree, n: NodeId, l: &List) -> bool {
    l.0.iter().any(|c| complex_at(ctx, t, n, c, c.0.len() - 1))
}

/// :has(): is there an element related to `anchor` by the relative selector?
fn has_relative(ctx: &mut Ctx, t: &Tree, anchor: NodeId, c: &Complex) -> bool {
    // candidates for the last compound, then check that the chain leads back
    // to the anchor through the first (relative) combinator
    let first_comb = c.0[0].0;
    let candidates: Vec<NodeId> = match first_comb {
        Comb::Descendant | Comb::Child => {
            let mut v = Vec::new();
            let mut cur = t.nodes[anchor].children;
            while let Some(x) = cur {
                if t.nodes[x].t == NType::Element {
                    v.push(x);
                }
                cur = next_in_tree_order(t, x, Some(anchor));
            }
            v
        }
        Comb::Column => {
            ctx.error = Some((UNSUPPORTED, String::new()));
            Vec::new()
        }
        Comb::Next | Comb::Subsequent => {
            // the subject can be a following sibling or a descendant of one
            let mut v = Vec::new();
            let mut sib = t.nodes[anchor].next;
            while let Some(s_) = sib {
                if t.nodes[s_].t == NType::Element {
                    v.push(s_);
                    let mut cur = t.nodes[s_].children;
                    while let Some(x) = cur {
                        if t.nodes[x].t == NType::Element {
                            v.push(x);
                        }
                        cur = next_in_tree_order(t, x, Some(s_));
                    }
                }
                sib = t.nodes[s_].next;
            }
            v
        }
    };
    for cand in candidates {
        if relative_chain(ctx, t, cand, c, c.0.len() - 1, anchor) {
            return true;
        }
    }
    false
}

fn relative_chain(
    ctx: &mut Ctx,
    t: &Tree,
    n: NodeId,
    c: &Complex,
    idx: usize,
    anchor: NodeId,
) -> bool {
    if !compound(ctx, t, n, &c.0[idx].1) {
        return false;
    }
    let comb = c.0[idx].0;
    let check_prev = |ctx: &mut Ctx, x: NodeId| -> bool {
        if idx == 0 {
            x == anchor
        } else {
            relative_chain(ctx, t, x, c, idx - 1, anchor)
        }
    };
    match comb {
        Comb::Descendant => {
            let mut p = t.nodes[n].parent;
            while let Some(x) = p {
                if idx == 0 {
                    if x == anchor {
                        return true;
                    }
                } else if t.nodes[x].t == NType::Element && check_prev(ctx, x) {
                    return true;
                }
                if x == anchor {
                    return false;
                }
                p = t.nodes[x].parent;
            }
            false
        }
        Comb::Child => match t.nodes[n].parent {
            Some(p) => check_prev(ctx, p),
            None => false,
        },
        Comb::Next => {
            let mut p = t.nodes[n].prev;
            while let Some(x) = p {
                if t.nodes[x].t == NType::Element {
                    return check_prev(ctx, x);
                }
                p = t.nodes[x].prev;
            }
            false
        }
        Comb::Subsequent => {
            let mut p = t.nodes[n].prev;
            while let Some(x) = p {
                if t.nodes[x].t == NType::Element && check_prev(ctx, x) {
                    return true;
                }
                p = t.nodes[x].prev;
            }
            false
        }
        Comb::Column => {
            ctx.error = Some((UNSUPPORTED, String::new()));
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Entry points

fn parse_or_throw(vm: &mut Vm, sel: &str) -> R<List> {
    match parse(sel) {
        Ok(l) => Ok(l),
        Err(m) => {
            dom_err_msg(vm, SYNTAX_ERR, &format!("Invalid selector ({})", m));
            Err(())
        }
    }
}

/// Pseudo error code: lexbor reports a failed execution status, which PHP
/// turns into "contains an unsupported selector".
const UNSUPPORTED: i64 = -1;

fn finish(vm: &mut Vm, ctx: Ctx) -> R<()> {
    if let Some((UNSUPPORTED, _)) = ctx.error {
        let f = crate::dom::active_fn(vm);
        throw_error(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #1 ($selectors) contains an unsupported selector",
                f
            ),
        );
        return Err(());
    }
    if let Some((code, msg)) = ctx.error {
        dom_err_msg(vm, code, &msg);
        return Err(());
    }
    Ok(())
}

fn quirks(n: NodeId) -> bool {
    doc_props(Some(doc_of_node(n).unwrap_or(n))).quirks_mode != 0
}

/// querySelectorAll: matching descendants of `root` in tree order.
pub fn select(vm: &mut Vm, root: NodeId, sel: &str, first_only: bool) -> R<Vec<NodeId>> {
    let list = parse_or_throw(vm, sel)?;
    let mut ctx = Ctx {
        quirks: quirks(root),
        error: None,
    };
    let out = tree::with(|t| {
        let mut out = Vec::new();
        let mut cur = t.nodes[root].children;
        while let Some(x) = cur {
            if t.nodes[x].t == NType::Element && matches_list(&mut ctx, t, x, &list) {
                out.push(x);
                if first_only {
                    break;
                }
            }
            if ctx.error.is_some() {
                break;
            }
            cur = next_in_tree_order(t, x, Some(root));
        }
        out
    });
    finish(vm, ctx)?;
    Ok(out)
}

/// Element::matches
pub fn matches_node(vm: &mut Vm, n: NodeId, sel: &str) -> R<bool> {
    let list = parse_or_throw(vm, sel)?;
    let mut ctx = Ctx {
        quirks: quirks(n),
        error: None,
    };
    let r = tree::with(|t| t.nodes[n].t == NType::Element && matches_list(&mut ctx, t, n, &list));
    finish(vm, ctx)?;
    Ok(r)
}

/// Element::closest
pub fn closest(vm: &mut Vm, n: NodeId, sel: &str) -> R<Option<NodeId>> {
    let list = parse_or_throw(vm, sel)?;
    let mut ctx = Ctx {
        quirks: quirks(n),
        error: None,
    };
    let r = tree::with(|t| {
        let mut cur = Some(n);
        while let Some(x) = cur {
            if t.nodes[x].t == NType::Element && matches_list(&mut ctx, t, x, &list) {
                return Some(x);
            }
            if ctx.error.is_some() {
                return None;
            }
            cur = t.nodes[x].parent;
        }
        None
    });
    finish(vm, ctx)?;
    Ok(r)
}
