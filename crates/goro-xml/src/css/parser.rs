//! Selector tokenizer (CSS Syntax Level 3, the parts selectors use) and
//! parser (Selectors Level 4), with the error messages PHP reports
//! (observed on PHP 8.5, and in ext/dom/tests/modern/css_selectors).

use super::{CssError, CssErrorKind};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Tok {
    Ident(String),
    Function(String),
    AtKeyword(String),
    Hash(String),
    Str(String),
    Number(f64, String),
    Dimension(f64, String, String),
    Percentage(f64),
    Ws,
    Colon,
    Comma,
    LBracket,
    RBracket,
    LParen,
    RParen,
    LBrace,
    RBrace,
    /// `~=` `|=` `^=` `$=` `*=`
    Match(char),
    /// `||`
    Column,
    Delim(char),
    Eof,
}

impl Tok {
    /// How PHP names the token in "Unexpected token: ..." messages.
    fn display(&self) -> String {
        match self {
            Tok::Ident(s) => s.clone(),
            Tok::Function(s) => format!("{s}("),
            Tok::AtKeyword(s) => format!("@{s}"),
            Tok::Hash(s) => format!("#{s}"),
            Tok::Str(s) => format!("\"{s}\""),
            Tok::Number(n, _) => fmt_num(*n),
            Tok::Dimension(n, _, u) => format!("{}{u}", fmt_num(*n)),
            Tok::Percentage(n) => format!("{}%", fmt_num(*n)),
            Tok::Ws => " ".into(),
            Tok::Colon => ":".into(),
            Tok::Comma => ",".into(),
            Tok::LBracket => "[".into(),
            Tok::RBracket => "]".into(),
            Tok::LParen => "(".into(),
            Tok::RParen => ")".into(),
            Tok::LBrace => "{".into(),
            Tok::RBrace => "}".into(),
            Tok::Match(c) => format!("{c}="),
            Tok::Column => "||".into(),
            Tok::Delim(c) => c.to_string(),
            Tok::Eof => "END-OF-FILE".into(),
        }
    }
}

fn fmt_num(n: f64) -> String {
    format!("{n}")
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || (c as u32) >= 0x80
}

fn is_name_char(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '-'
}

struct Lexer {
    c: Vec<char>,
    i: usize,
}

impl Lexer {
    fn peek(&self, k: usize) -> Option<char> {
        self.c.get(self.i + k).copied()
    }

    fn valid_escape(&self, k: usize) -> bool {
        self.peek(k) == Some('\\') && self.peek(k + 1).is_some_and(|c| c != '\n')
    }

    fn starts_ident(&self, k: usize) -> bool {
        match self.peek(k) {
            Some('-') => {
                self.peek(k + 1)
                    .is_some_and(|c| is_name_start(c) || c == '-')
                    || self.valid_escape(k + 1)
            }
            Some('\\') => self.valid_escape(k),
            Some(c) => is_name_start(c),
            None => false,
        }
    }

    fn escape(&mut self) -> char {
        // self.i at the character after the backslash
        let mut hex = String::new();
        while hex.len() < 6 && self.peek(0).is_some_and(|c| c.is_ascii_hexdigit()) {
            hex.push(self.peek(0).unwrap());
            self.i += 1;
        }
        if !hex.is_empty() {
            if self.peek(0).is_some_and(|c| c.is_ascii_whitespace()) {
                self.i += 1;
            }
            let v = u32::from_str_radix(&hex, 16).unwrap_or(0xFFFD);
            return match char::from_u32(v) {
                Some(c) if v != 0 => c,
                _ => '\u{FFFD}',
            };
        }
        match self.peek(0) {
            Some(c) => {
                self.i += 1;
                c
            }
            None => '\u{FFFD}',
        }
    }

    fn name(&mut self) -> String {
        let mut s = String::new();
        loop {
            match self.peek(0) {
                Some(c) if is_name_char(c) => {
                    s.push(c);
                    self.i += 1;
                }
                Some('\\') if self.valid_escape(0) => {
                    self.i += 1;
                    s.push(self.escape());
                }
                _ => return s,
            }
        }
    }

    fn starts_number(&self) -> bool {
        match self.peek(0) {
            Some('+') | Some('-') => {
                self.peek(1).is_some_and(|c| c.is_ascii_digit())
                    || (self.peek(1) == Some('.')
                        && self.peek(2).is_some_and(|c| c.is_ascii_digit()))
            }
            Some('.') => self.peek(1).is_some_and(|c| c.is_ascii_digit()),
            Some(c) => c.is_ascii_digit(),
            None => false,
        }
    }

    fn number(&mut self) -> (f64, String) {
        let start = self.i;
        if matches!(self.peek(0), Some('+') | Some('-')) {
            self.i += 1;
        }
        while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        if self.peek(0) == Some('.') && self.peek(1).is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
            while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
                self.i += 1;
            }
        }
        if matches!(self.peek(0), Some('e') | Some('E')) {
            let k = if matches!(self.peek(1), Some('+') | Some('-')) {
                2
            } else {
                1
            };
            if self.peek(k).is_some_and(|c| c.is_ascii_digit()) {
                self.i += k;
                while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
                    self.i += 1;
                }
            }
        }
        let raw: String = self.c[start..self.i].iter().collect();
        let mut t = raw.clone();
        if t.starts_with('+') {
            t.remove(0);
        }
        let t = t.replace("-.", "-0.");
        let t = if t.starts_with('.') {
            format!("0{t}")
        } else {
            t
        };
        (t.parse().unwrap_or(0.0), raw)
    }

    fn next(&mut self) -> Tok {
        let Some(c) = self.peek(0) else {
            return Tok::Eof;
        };
        match c {
            ' ' | '\t' | '\n' | '\r' | '\x0C' => {
                while self
                    .peek(0)
                    .is_some_and(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C'))
                {
                    self.i += 1;
                }
                Tok::Ws
            }
            '"' | '\'' => {
                self.i += 1;
                let mut s = String::new();
                loop {
                    match self.peek(0) {
                        None => break,
                        Some(x) if x == c => {
                            self.i += 1;
                            break;
                        }
                        Some('\\') => {
                            self.i += 1;
                            match self.peek(0) {
                                None => {}
                                Some('\n') => self.i += 1,
                                Some(_) => s.push(self.escape()),
                            }
                        }
                        Some(x) => {
                            s.push(x);
                            self.i += 1;
                        }
                    }
                }
                Tok::Str(s)
            }
            '#' => {
                if self.peek(1).is_some_and(is_name_char) || self.valid_escape(1) {
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
            ',' => {
                self.i += 1;
                Tok::Comma
            }
            ':' => {
                self.i += 1;
                Tok::Colon
            }
            '~' | '^' | '$' | '*' if self.peek(1) == Some('=') => {
                self.i += 2;
                Tok::Match(c)
            }
            '|' if self.peek(1) == Some('=') => {
                self.i += 2;
                Tok::Match('|')
            }
            '|' if self.peek(1) == Some('|') => {
                self.i += 2;
                Tok::Column
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
            _ if self.starts_number() => {
                let (n, raw) = self.number();
                if self.starts_ident(0) {
                    let u = self.name();
                    Tok::Dimension(n, raw, u)
                } else if self.peek(0) == Some('%') {
                    self.i += 1;
                    Tok::Percentage(n)
                } else {
                    Tok::Number(n, raw)
                }
            }
            _ if self.starts_ident(0) => {
                let n = self.name();
                if self.peek(0) == Some('(') {
                    self.i += 1;
                    Tok::Function(n)
                } else {
                    Tok::Ident(n)
                }
            }
            _ => {
                self.i += 1;
                Tok::Delim(c)
            }
        }
    }
}

fn tokenize(s: &str) -> Vec<(Tok, usize, usize)> {
    let mut l = Lexer {
        c: s.chars().collect(),
        i: 0,
    };
    let mut out = Vec::new();
    loop {
        let start = l.i;
        let t = l.next();
        let end = l.i;
        let eof = t == Tok::Eof;
        out.push((t, start, end));
        if eof {
            return out;
        }
    }
}

// ----- selector structures ------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Combinator {
    Descendant,
    Child,
    NextSibling,
    SubsequentSibling,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AttrOp {
    Exists,
    Equals,
    Includes,
    DashMatch,
    Prefix,
    Suffix,
    Substring,
}

#[derive(Clone, Debug)]
pub(crate) struct AttrSel {
    pub name: String,
    pub op: AttrOp,
    pub value: String,
    /// `Some(true)` for `i`, `Some(false)` for `s`.
    pub case_insensitive: Option<bool>,
}

#[derive(Clone, Debug)]
pub(crate) struct Nth {
    pub a: i64,
    pub b: i64,
    pub of: Option<Vec<Complex>>,
}

#[derive(Clone, Debug)]
pub(crate) enum Pseudo {
    Not(Vec<Complex>),
    Is(Vec<Complex>),
    Has(Vec<Complex>),
    NthChild(Nth),
    NthLastChild(Nth),
    NthOfType(Nth),
    NthLastOfType(Nth),
    FirstChild,
    LastChild,
    OnlyChild,
    FirstOfType,
    LastOfType,
    OnlyOfType,
    Root,
    Empty,
    Checked,
    Disabled,
    Enabled,
    Link,
    Required,
    Optional,
    PlaceholderShown,
    ReadOnly,
    ReadWrite,
    /// `:current(S)`: matches like `:is(S)` in a static document.
    Current(Vec<Complex>),
    /// Interaction states that never match (`:hover`, `:focus`, ...).
    Never,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Compound {
    /// `None` = no type selector or `*`.
    pub tag: Option<String>,
    pub ids: Vec<String>,
    pub classes: Vec<String>,
    pub attrs: Vec<AttrSel>,
    pub pseudos: Vec<Pseudo>,
}

/// A complex selector: compounds from left to right; `combs[i]` joins
/// `parts[i]` and `parts[i + 1]`. For relative selectors (`:has()`),
/// `lead` is the combinator to the anchor.
#[derive(Clone, Debug)]
pub(crate) struct Complex {
    pub lead: Option<Combinator>,
    pub parts: Vec<Compound>,
    pub combs: Vec<Combinator>,
}

// ----- parser --------------------------------------------------------------------

struct P {
    toks: Vec<(Tok, usize, usize)>,
    pos: usize,
    src: Vec<char>,
}

type R<T> = Result<T, CssError>;

fn unexpected(t: &Tok) -> CssError {
    CssError::syntax(format!("Unexpected token: {}", t.display()))
}

const NOT_SUPPORTED: &[&str] = &[
    "default",
    "dir",
    "focus-visible",
    "focus-within",
    "fullscreen",
    "future",
    "in-range",
    "indeterminate",
    "invalid",
    "lang",
    "local-link",
    "nth-col",
    "nth-last-col",
    "out-of-range",
    "past",
    "scope",
    "target",
    "target-within",
    "user-invalid",
    "valid",
    "visited",
    "warning",
];

const PSEUDO_ELEMENTS: &[&str] = &[
    "after",
    "backdrop",
    "before",
    "first-letter",
    "first-line",
    "grammar-error",
    "inactive-selection",
    "marker",
    "placeholder",
    "selection",
    "spelling-error",
    "target-text",
];

pub(crate) fn parse(s: &str) -> R<Vec<Complex>> {
    let mut p = P {
        toks: tokenize(s),
        pos: 0,
        src: s.chars().collect(),
    };
    p.skip_ws();
    let list = p.list(false)?;
    p.skip_ws();
    match p.peek() {
        Tok::Eof => Ok(list),
        t => Err(unexpected(t)),
    }
}

impl P {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos.min(self.toks.len() - 1)].0
    }

    fn bump(&mut self) -> Tok {
        let t = self.peek().clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn skip_ws(&mut self) {
        while *self.peek() == Tok::Ws {
            self.pos += 1;
        }
    }

    /// A selector list, ending before `)` or EOF.
    fn list(&mut self, relative: bool) -> R<Vec<Complex>> {
        let mut v = Vec::new();
        loop {
            self.skip_ws();
            v.push(self.complex(relative)?);
            self.skip_ws();
            if *self.peek() == Tok::Comma {
                self.bump();
                continue;
            }
            return Ok(v);
        }
    }

    fn combinator(&mut self) -> R<Option<Combinator>> {
        let had_ws = *self.peek() == Tok::Ws;
        self.skip_ws();
        let c = match self.peek() {
            Tok::Delim('>') => Some(Combinator::Child),
            Tok::Delim('+') => Some(Combinator::NextSibling),
            Tok::Delim('~') => Some(Combinator::SubsequentSibling),
            Tok::Column => {
                return Err(CssError {
                    kind: CssErrorKind::Unsupported,
                    message: "contains an unsupported selector".into(),
                });
            }
            _ => None,
        };
        if let Some(c) = c {
            self.bump();
            self.skip_ws();
            return Ok(Some(c));
        }
        Ok(if had_ws && self.starts_compound() {
            Some(Combinator::Descendant)
        } else {
            None
        })
    }

    fn starts_compound(&self) -> bool {
        matches!(
            self.peek(),
            Tok::Ident(_)
                | Tok::Hash(_)
                | Tok::Colon
                | Tok::LBracket
                | Tok::Delim('*')
                | Tok::Delim('.')
                | Tok::Delim('|')
                | Tok::Function(_)
        )
    }

    fn complex(&mut self, relative: bool) -> R<Complex> {
        let mut lead = None;
        if relative {
            match self.peek() {
                Tok::Delim('>') => lead = Some(Combinator::Child),
                Tok::Delim('+') => lead = Some(Combinator::NextSibling),
                Tok::Delim('~') => lead = Some(Combinator::SubsequentSibling),
                _ => {}
            }
            if lead.is_some() {
                self.bump();
                self.skip_ws();
            } else {
                lead = Some(Combinator::Descendant);
            }
        }
        let mut parts = vec![self.compound()?];
        let mut combs = Vec::new();
        while let Some(c) = self.combinator()? {
            combs.push(c);
            parts.push(self.compound()?);
        }
        Ok(Complex { lead, parts, combs })
    }

    fn compound(&mut self) -> R<Compound> {
        let mut c = Compound::default();
        let mut any = false;
        // Type selector, with an optional (ignored) namespace prefix
        // (observed on PHP 8.5: `svg|circle` and `|p` match like `circle`
        // and `p`).
        match self.peek().clone() {
            Tok::Ident(n) => {
                self.bump();
                if *self.peek() == Tok::Delim('|') {
                    self.bump();
                    c.tag = self.type_name()?;
                } else {
                    c.tag = Some(n);
                }
                any = true;
            }
            Tok::Delim('*') => {
                self.bump();
                if *self.peek() == Tok::Delim('|') {
                    self.bump();
                    c.tag = self.type_name()?;
                }
                any = true;
            }
            Tok::Delim('|') => {
                self.bump();
                c.tag = self.type_name()?;
                any = true;
            }
            _ => {}
        }
        loop {
            match self.peek().clone() {
                Tok::Hash(h) => {
                    self.bump();
                    c.ids.push(h);
                }
                Tok::Delim('.') => {
                    self.bump();
                    match self.bump() {
                        Tok::Ident(n) => c.classes.push(n),
                        t => return Err(unexpected(&t)),
                    }
                }
                Tok::LBracket => {
                    self.bump();
                    c.attrs.push(self.attr()?);
                }
                Tok::Colon => {
                    self.bump();
                    if *self.peek() == Tok::Colon {
                        self.bump();
                        return Err(match self.bump() {
                            Tok::Ident(n) | Tok::Function(n)
                                if PSEUDO_ELEMENTS.contains(&n.to_ascii_lowercase().as_str()) =>
                            {
                                CssError::syntax(format!("Not supported: {n}"))
                            }
                            t => unexpected(&t),
                        });
                    }
                    if let Some(p) = self.pseudo()? {
                        c.pseudos.push(p);
                    }
                }
                _ => break,
            }
            any = true;
        }
        if !any {
            return Err(unexpected(self.peek()));
        }
        Ok(c)
    }

    fn type_name(&mut self) -> R<Option<String>> {
        match self.bump() {
            Tok::Ident(n) => Ok(Some(n)),
            Tok::Delim('*') => Ok(None),
            t => Err(unexpected(&t)),
        }
    }

    fn attr(&mut self) -> R<AttrSel> {
        self.skip_ws();
        let name = match self.bump() {
            Tok::Ident(n) => {
                if *self.peek() == Tok::Delim('|') {
                    self.bump();
                    match self.bump() {
                        Tok::Ident(n) => n,
                        t => return Err(unexpected(&t)),
                    }
                } else {
                    n
                }
            }
            Tok::Delim('|') => match self.bump() {
                Tok::Ident(n) => n,
                t => return Err(unexpected(&t)),
            },
            t => return Err(unexpected(&t)),
        };
        self.skip_ws();
        let op = match self.bump() {
            Tok::RBracket => {
                return Ok(AttrSel {
                    name,
                    op: AttrOp::Exists,
                    value: String::new(),
                    case_insensitive: None,
                });
            }
            Tok::Delim('=') => AttrOp::Equals,
            Tok::Match('~') => AttrOp::Includes,
            Tok::Match('|') => AttrOp::DashMatch,
            Tok::Match('^') => AttrOp::Prefix,
            Tok::Match('$') => AttrOp::Suffix,
            Tok::Match('*') => AttrOp::Substring,
            t => return Err(unexpected(&t)),
        };
        self.skip_ws();
        let value = match self.bump() {
            Tok::Ident(v) | Tok::Str(v) => v,
            t => return Err(unexpected(&t)),
        };
        self.skip_ws();
        let mut ci = None;
        if let Tok::Ident(f) = self.peek().clone() {
            match f.as_str() {
                "i" => ci = Some(true),
                "s" => ci = Some(false),
                _ => return Err(unexpected(self.peek())),
            }
            self.bump();
            self.skip_ws();
        }
        match self.bump() {
            Tok::RBracket => Ok(AttrSel {
                name,
                op,
                value,
                case_insensitive: ci,
            }),
            t => Err(unexpected(&t)),
        }
    }

    fn pseudo(&mut self) -> R<Option<Pseudo>> {
        match self.bump() {
            Tok::Ident(n) => {
                let l = n.to_ascii_lowercase();
                Ok(Some(match l.as_str() {
                    "first-child" => Pseudo::FirstChild,
                    "last-child" => Pseudo::LastChild,
                    "only-child" => Pseudo::OnlyChild,
                    "first-of-type" => Pseudo::FirstOfType,
                    "last-of-type" => Pseudo::LastOfType,
                    "only-of-type" => Pseudo::OnlyOfType,
                    "root" => Pseudo::Root,
                    "empty" => Pseudo::Empty,
                    "checked" => Pseudo::Checked,
                    "disabled" => Pseudo::Disabled,
                    "enabled" => Pseudo::Enabled,
                    "link" | "any-link" => Pseudo::Link,
                    "required" => Pseudo::Required,
                    "optional" => Pseudo::Optional,
                    "placeholder-shown" => Pseudo::PlaceholderShown,
                    "read-only" => Pseudo::ReadOnly,
                    "read-write" => Pseudo::ReadWrite,
                    "hover" | "focus" | "active" | "current" => Pseudo::Never,
                    "blank" => {
                        return Err(CssError {
                            kind: CssErrorKind::Blank,
                            message: ":blank selector is not implemented because CSSWG has not yet decided its semantics (https://github.com/w3c/csswg-drafts/issues/1967)".into(),
                        });
                    }
                    _ if NOT_SUPPORTED.contains(&l.as_str()) => {
                        return Err(CssError::syntax(format!("Not supported: {n}")));
                    }
                    _ => return Err(CssError::syntax(format!("Unexpected token: {n}"))),
                }))
            }
            Tok::Function(n) => {
                let l = n.to_ascii_lowercase();
                self.skip_ws();
                if *self.peek() == Tok::RParen {
                    return Err(CssError::syntax(format!(
                        "Pseudo function can't be empty: {n}()"
                    )));
                }
                let p = match l.as_str() {
                    "not" => Pseudo::Not(self.list(false)?),
                    "is" | "where" => Pseudo::Is(self.list(false)?),
                    "has" => Pseudo::Has(self.list(true)?),
                    "current" => Pseudo::Current(self.list(false)?),
                    "nth-child" => Pseudo::NthChild(self.nth(true)?),
                    "nth-last-child" => Pseudo::NthLastChild(self.nth(true)?),
                    "nth-of-type" => Pseudo::NthOfType(self.nth(false)?),
                    "nth-last-of-type" => Pseudo::NthLastOfType(self.nth(false)?),
                    _ if NOT_SUPPORTED.contains(&l.as_str()) => {
                        return Err(CssError::syntax(format!("Not supported: {n}")));
                    }
                    _ => return Err(CssError::syntax(format!("Unexpected token: {n}"))),
                };
                self.skip_ws();
                match self.bump() {
                    Tok::RParen => Ok(Some(p)),
                    t => Err(unexpected(&t)),
                }
            }
            t => Err(unexpected(&t)),
        }
    }

    /// `An+B` (and `odd`/`even`), optionally followed by `of S`.
    fn nth(&mut self, allow_of: bool) -> R<Nth> {
        // Gather the raw text of the An+B part.
        let start = self.toks[self.pos].1;
        let mut end = start;
        loop {
            match self.peek() {
                Tok::RParen | Tok::Eof => break,
                Tok::Ident(n) if n.eq_ignore_ascii_case("of") => break,
                _ => {
                    end = self.toks[self.pos].2;
                    self.bump();
                }
            }
        }
        let raw: String = self.src[start..end].iter().collect();
        let raw = raw.trim().to_ascii_lowercase();
        let Some((a, b)) = parse_anb(&raw) else {
            let first = tokenize(&raw)
                .into_iter()
                .next()
                .map(|t| t.0)
                .unwrap_or(Tok::Eof);
            return Err(unexpected(&first));
        };
        let mut of = None;
        if let Tok::Ident(n) = self.peek().clone()
            && n.eq_ignore_ascii_case("of")
        {
            if !allow_of {
                return Err(unexpected(self.peek()));
            }
            self.bump();
            self.skip_ws();
            of = Some(self.list(false)?);
        }
        Ok(Nth { a, b, of })
    }
}

/// Parses `An+B`, `odd`, `even` (CSS Syntax Level 3 section 6).
fn parse_anb(s: &str) -> Option<(i64, i64)> {
    let s: String = s.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    match s.as_str() {
        "odd" => return Some((2, 1)),
        "even" => return Some((2, 0)),
        "" => return None,
        _ => {}
    }
    if let Some(i) = s.find('n') {
        let (a, rest) = s.split_at(i);
        let rest = &rest[1..];
        let a = match a {
            "" | "+" => 1,
            "-" => -1,
            x => x.parse::<i64>().ok()?,
        };
        let b = if rest.is_empty() {
            0
        } else {
            if !(rest.starts_with('+') || rest.starts_with('-')) {
                return None;
            }
            let r = rest.strip_prefix('+').unwrap_or(rest);
            r.parse::<i64>().ok()?
        };
        Some((a, b))
    } else {
        let b = s.strip_prefix('+').unwrap_or(&s);
        if b.starts_with('+') {
            return None;
        }
        Some((0, b.parse::<i64>().ok()?))
    }
}
