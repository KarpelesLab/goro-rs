//! The PHP lexer.
//!
//! It produces the token stream PHP exposes through `token_get_all()`: same
//! token kinds, same text, same line numbers (PHP manual: "List of Parser
//! Tokens"; details observed on PHP 8.5 with `token_get_all()`).
//!
//! The lexer works on bytes: PHP source need not be valid UTF-8, and
//! identifiers may contain any byte from 0x80 to 0xff.

use crate::token::{T, TF_BINARY_PREFIX, TF_DOUBLE, TF_ESCAPES, TF_TAG, Token};

/// An error the lexer detects: PHP reports these as "Parse error: <msg>".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LexError {
    pub message: String,
    pub line: u32,
    pub offset: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    /// Outside `<?php ... ?>`.
    Initial,
    Scripting,
    /// After `->` / `?->`: the next label is a `T_STRING`.
    LookingForProperty,
    DoubleQuotes,
    Backquote,
    Heredoc,
    Nowdoc,
    /// After `$var` inside a string, when `[` follows.
    VarOffset,
    /// After `${` inside a string.
    LookingForVarname,
    /// After `$var` inside a string, when `->label` or `?->label` follows.
    PropertyInString,
}

#[derive(Clone, Copy, Debug)]
struct Heredoc {
    label_start: u32,
    label_len: u32,
    /// Indentation of the closing marker.
    indent: u32,
    /// The character the closing marker is indented with (0: none).
    indent_char: u8,
}

/// Configuration of the lexer.
#[derive(Clone, Copy, Debug)]
pub struct LexerOptions {
    /// `short_open_tag` INI setting: whether `<?` opens PHP code.
    pub short_open_tag: bool,
    /// Parser mode: check brackets, report lexing errors.
    pub parse_mode: bool,
}

impl Default for LexerOptions {
    fn default() -> Self {
        LexerOptions {
            short_open_tag: true,
            parse_mode: false,
        }
    }
}

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: u32,
    state: State,
    stack: Vec<State>,
    heredocs: Vec<Heredoc>,
    opts: LexerOptions,
    /// Open brackets (character, line), parser mode only.
    brackets: Vec<(u8, u32)>,
    /// The first error detected.
    pub error: Option<LexError>,
    /// After `__halt_compiler`: number of significant tokens still to lex.
    halt: u8,
    halted: bool,
    finished: bool,
}

#[inline]
pub(crate) fn is_label_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80
}

#[inline]
pub(crate) fn is_label_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

/// Counts line breaks: `\n`, `\r\n` and a lone `\r` each count once.
#[inline]
pub(crate) fn count_newlines(s: &[u8]) -> u32 {
    let mut n = 0u32;
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'\n' => n += 1,
            b'\r' if i + 1 >= s.len() || s[i + 1] != b'\n' => n += 1,
            _ => {}
        }
        i += 1;
    }
    n
}

/// Keyword lookup (case-insensitive). PHP manual: "List of Keywords".
pub(crate) fn keyword(text: &[u8]) -> Option<T> {
    if text.len() < 2 || text.len() > 15 {
        return None;
    }
    let mut buf = [0u8; 15];
    for (i, &c) in text.iter().enumerate() {
        buf[i] = c.to_ascii_lowercase();
    }
    let w = &buf[..text.len()];
    Some(match w {
        b"abstract" => T::Abstract,
        b"and" => T::LogicalAnd,
        b"array" => T::Array,
        b"as" => T::As,
        b"break" => T::Break,
        b"callable" => T::Callable,
        b"case" => T::Case,
        b"catch" => T::Catch,
        b"class" => T::Class,
        b"clone" => T::Clone,
        b"const" => T::Const,
        b"continue" => T::Continue,
        b"declare" => T::Declare,
        b"default" => T::Default,
        b"die" | b"exit" => T::Exit,
        b"do" => T::Do,
        b"echo" => T::Echo,
        b"enum" => T::Enum,
        b"else" => T::Else,
        b"elseif" => T::Elseif,
        b"empty" => T::Empty,
        b"enddeclare" => T::Enddeclare,
        b"endfor" => T::Endfor,
        b"endforeach" => T::Endforeach,
        b"endif" => T::Endif,
        b"endswitch" => T::Endswitch,
        b"endwhile" => T::Endwhile,
        b"eval" => T::Eval,
        b"extends" => T::Extends,
        b"final" => T::Final,
        b"finally" => T::Finally,
        b"fn" => T::Fn,
        b"for" => T::For,
        b"foreach" => T::Foreach,
        b"function" => T::Function,
        b"global" => T::Global,
        b"goto" => T::Goto,
        b"if" => T::If,
        b"implements" => T::Implements,
        b"include" => T::Include,
        b"include_once" => T::IncludeOnce,
        b"instanceof" => T::Instanceof,
        b"insteadof" => T::Insteadof,
        b"interface" => T::Interface,
        b"isset" => T::Isset,
        b"list" => T::List,
        b"match" => T::Match,
        b"namespace" => T::Namespace,
        b"new" => T::New,
        b"or" => T::LogicalOr,
        b"print" => T::Print,
        b"private" => T::Private,
        b"protected" => T::Protected,
        b"public" => T::Public,
        b"readonly" => T::Readonly,
        b"require" => T::Require,
        b"require_once" => T::RequireOnce,
        b"return" => T::Return,
        b"static" => T::Static,
        b"switch" => T::Switch,
        b"throw" => T::Throw,
        b"trait" => T::Trait,
        b"try" => T::Try,
        b"unset" => T::Unset,
        b"use" => T::Use,
        b"var" => T::Var,
        b"while" => T::While,
        b"xor" => T::LogicalXor,
        b"yield" => T::Yield,
        b"__class__" => T::ClassC,
        b"__dir__" => T::Dir,
        b"__file__" => T::File,
        b"__function__" => T::FuncC,
        b"__line__" => T::Line,
        b"__method__" => T::MethodC,
        b"__namespace__" => T::NsC,
        b"__property__" => T::PropertyC,
        b"__trait__" => T::TraitC,
        _ => {
            if w == b"__halt_compiler" {
                T::HaltCompiler
            } else {
                return None;
            }
        }
    })
}

fn eq_ic(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_ascii_lowercase() == *y)
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a [u8], opts: LexerOptions) -> Self {
        Lexer {
            src,
            pos: 0,
            line: 1,
            state: State::Initial,
            stack: Vec::new(),
            heredocs: Vec::new(),
            opts,
            brackets: Vec::new(),
            error: None,
            halt: 0,
            halted: false,
            finished: false,
        }
    }

    /// Starts directly in PHP code (as `eval()` and `token_get_all()` do
    /// not; used for `eval()`'d code).
    pub fn new_in_scripting(src: &'a [u8], opts: LexerOptions) -> Self {
        let mut l = Lexer::new(src, opts);
        l.state = State::Scripting;
        l
    }

    /// The current line.
    pub fn line(&self) -> u32 {
        self.line
    }

    /// Byte offset where lexing stopped after `__halt_compiler();`, if it
    /// did.
    pub fn halted(&self) -> bool {
        self.halted
    }

    #[inline]
    fn at(&self, i: usize) -> u8 {
        if i < self.src.len() { self.src[i] } else { 0 }
    }

    fn set_error(&mut self, msg: impl Into<String>, line: u32, offset: usize) {
        if self.error.is_none() {
            self.error = Some(LexError {
                message: msg.into(),
                line,
                offset: offset as u32,
            });
        }
    }

    #[inline]
    fn tok(&mut self, kind: T, start: usize, flags: u16) -> Token {
        let line = self.line;
        let text = &self.src[start..self.pos];
        if text.len() > 1 || (text.len() == 1 && (text[0] == b'\n' || text[0] == b'\r')) {
            self.line += count_newlines(text);
        }
        Token {
            kind,
            flags,
            start: start as u32,
            end: self.pos as u32,
            line,
        }
    }

    /// Returns the next token, or `None` at end of input.
    pub fn next_token(&mut self) -> Option<Token> {
        if self.finished {
            return None;
        }
        if self.pos >= self.src.len() {
            self.finished = true;
            if self.opts.parse_mode
                && let Some(&(c, line)) = self.brackets.last()
            {
                let msg = if line != self.line {
                    format!("Unclosed '{}' on line {}", c as char, line)
                } else {
                    format!("Unclosed '{}'", c as char)
                };
                let (l, p) = (self.line, self.pos);
                self.set_error(msg, l, p);
            }
            return None;
        }
        if self.halt == u8::MAX {
            // `__halt_compiler();` seen: the rest is raw data.
            self.finished = true;
            self.halted = true;
            if self.opts.parse_mode {
                // Open brackets are reported here too (test
                // Zend/tests/grammar/bug60099.phpt).
                if let Some(&(c, line)) = self.brackets.last() {
                    let msg = if line != self.line {
                        format!("Unclosed '{}' on line {}", c as char, line)
                    } else {
                        format!("Unclosed '{}'", c as char)
                    };
                    let (l, p) = (self.line, self.pos);
                    self.set_error(msg, l, p);
                }
                return None;
            }
            let start = self.pos;
            self.pos = self.src.len();
            return Some(self.tok(T::InlineHtml, start, 0));
        }
        let t = match self.state {
            State::Initial => self.lex_initial(),
            State::Scripting => self.lex_scripting(),
            State::LookingForProperty => self.lex_looking_for_property(),
            State::DoubleQuotes => self.lex_dq_body(b'"'),
            State::Backquote => self.lex_dq_body(b'`'),
            State::Heredoc => self.lex_heredoc_body(),
            State::Nowdoc => self.lex_nowdoc_body(),
            State::VarOffset => self.lex_var_offset(),
            State::LookingForVarname => self.lex_looking_for_varname(),
            State::PropertyInString => self.lex_property_in_string(),
        };
        if self.halt > 0 && !t.kind.is_trivia() && t.kind != T::Comment {
            self.halt -= 1;
            if self.halt == 0 {
                self.halt = u8::MAX;
            }
        }
        if t.kind == T::HaltCompiler {
            self.halt = 3;
        }
        Some(t)
    }

    // ----- INITIAL (inline HTML) -----

    fn lex_initial(&mut self) -> Token {
        let start = self.pos;
        let src = self.src;
        // At an open tag?
        if let Some((kind, len)) = self.open_tag_at(start) {
            self.pos = start + len;
            self.state = State::Scripting;
            let mut flags = 0;
            if kind == T::OpenTagWithEcho {
                flags = TF_TAG;
            }
            return self.tok(kind, start, flags);
        }
        let mut i = start;
        loop {
            match memchr(b'<', &src[i..]) {
                None => {
                    i = src.len();
                    break;
                }
                Some(off) => {
                    let p = i + off;
                    if self.open_tag_at(p).is_some() {
                        i = p;
                        break;
                    }
                    i = p + 1;
                }
            }
        }
        self.pos = i;
        self.tok(T::InlineHtml, start, 0)
    }

    /// An open tag at `p`: its kind and length.
    fn open_tag_at(&self, p: usize) -> Option<(T, usize)> {
        let src = self.src;
        if self.at(p) != b'<' || self.at(p + 1) != b'?' {
            return None;
        }
        if self.at(p + 2) == b'=' {
            return Some((T::OpenTagWithEcho, 3));
        }
        if p + 5 <= src.len() && eq_ic(&src[p + 2..p + 5], b"php") {
            let c = self.at(p + 5);
            if p + 5 == src.len() {
                return Some((T::OpenTag, 5));
            }
            match c {
                b' ' | b'\t' | b'\n' => return Some((T::OpenTag, 6)),
                b'\r' => {
                    let len = if self.at(p + 6) == b'\n' { 7 } else { 6 };
                    return Some((T::OpenTag, len));
                }
                _ => {}
            }
        }
        if self.opts.short_open_tag {
            return Some((T::OpenTag, 2));
        }
        None
    }

    // ----- brackets (parser mode) -----

    fn open_bracket(&mut self, c: u8) {
        if self.opts.parse_mode {
            self.brackets.push((c, self.line));
        }
    }

    fn close_bracket(&mut self, c: u8, offset: usize) {
        if !self.opts.parse_mode {
            return;
        }
        let want = match c {
            b')' => b'(',
            b']' => b'[',
            _ => b'{',
        };
        match self.brackets.pop() {
            None => {
                let l = self.line;
                self.set_error(format!("Unmatched '{}'", c as char), l, offset);
            }
            Some((open, line)) if open != want => {
                let l = self.line;
                let msg = if line != self.line {
                    format!(
                        "Unclosed '{}' on line {} does not match '{}'",
                        open as char, line, c as char
                    )
                } else {
                    format!("Unclosed '{}' does not match '{}'", open as char, c as char)
                };
                self.set_error(msg, l, offset);
            }
            Some(_) => {}
        }
    }

    // ----- IN_SCRIPTING -----

    fn lex_scripting(&mut self) -> Token {
        let src = self.src;
        let start = self.pos;
        let c = src[start];
        match c {
            b' ' | b'\t' | b'\n' | b'\r' => {
                let mut i = start + 1;
                while i < src.len() && matches!(src[i], b' ' | b'\t' | b'\n' | b'\r') {
                    i += 1;
                }
                self.pos = i;
                self.tok(T::Whitespace, start, 0)
            }
            b'$' => {
                if is_label_start(self.at(start + 1)) {
                    let i = self.scan_label(start + 1);
                    self.pos = i;
                    self.tok(T::Variable, start, 0)
                } else {
                    self.pos = start + 1;
                    self.tok(T::Dollar, start, 0)
                }
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' | 0x80..=0xff => self.lex_label_or_name(start),
            b'\\' => {
                if is_label_start(self.at(start + 1)) {
                    let i = self.scan_qualified(start + 1);
                    self.pos = i;
                    self.tok(T::NameFullyQualified, start, 0)
                } else {
                    self.pos = start + 1;
                    self.tok(T::NsSeparator, start, 0)
                }
            }
            b'0'..=b'9' => self.lex_number(start),
            b'.' => {
                if self.at(start + 1).is_ascii_digit() {
                    return self.lex_number(start);
                }
                if self.at(start + 1) == b'.' && self.at(start + 2) == b'.' {
                    self.pos = start + 3;
                    return self.tok(T::Ellipsis, start, 0);
                }
                if self.at(start + 1) == b'=' {
                    self.pos = start + 2;
                    return self.tok(T::ConcatEqual, start, 0);
                }
                self.pos = start + 1;
                self.tok(T::Dot, start, 0)
            }
            b'\'' => self.lex_single_quoted(start, start + 1, 0),
            b'"' => self.lex_double_quoted(start, start + 1, 0),
            b'`' => {
                self.pos = start + 1;
                self.stack.push(State::Scripting);
                self.state = State::Backquote;
                self.tok(T::Backtick, start, 0)
            }
            b'#' => {
                if self.at(start + 1) == b'[' {
                    self.pos = start + 2;
                    let line = self.line;
                    let t = self.tok(T::Attribute, start, 0);
                    if self.opts.parse_mode {
                        self.brackets.push((b'[', line));
                    }
                    return t;
                }
                self.lex_line_comment(start)
            }
            b'/' => match self.at(start + 1) {
                b'/' => self.lex_line_comment(start),
                b'*' => self.lex_block_comment(start),
                b'=' => {
                    self.pos = start + 2;
                    self.tok(T::DivEqual, start, 0)
                }
                _ => {
                    self.pos = start + 1;
                    self.tok(T::Slash, start, 0)
                }
            },
            b'?' => {
                let n = self.at(start + 1);
                if n == b'>' {
                    // `?>` with one optional newline (observed on PHP 8.5).
                    let mut i = start + 2;
                    if self.at(i) == b'\n' {
                        i += 1;
                    } else if self.at(i) == b'\r' {
                        i += 1;
                        if self.at(i) == b'\n' {
                            i += 1;
                        }
                    }
                    self.pos = i;
                    self.state = State::Initial;
                    // The newline belongs to the tag; PHP counts it on the
                    // next token's line.
                    return self.tok(T::CloseTag, start, TF_TAG);
                }
                if n == b'?' {
                    if self.at(start + 2) == b'=' {
                        self.pos = start + 3;
                        return self.tok(T::CoalesceEqual, start, 0);
                    }
                    self.pos = start + 2;
                    return self.tok(T::Coalesce, start, 0);
                }
                if n == b'-' && self.at(start + 2) == b'>' {
                    self.pos = start + 3;
                    self.stack.push(State::Scripting);
                    self.state = State::LookingForProperty;
                    return self.tok(T::NullsafeObjectOperator, start, 0);
                }
                self.pos = start + 1;
                self.tok(T::Question, start, 0)
            }
            b'(' => {
                if let Some((kind, end)) = self.cast_at(start) {
                    if kind == T::BadCharacter {
                        // `(real)`: removed in PHP 8.0.
                        let l = self.line;
                        self.set_error(
                            "The (real) cast has been removed, use (float) instead",
                            l,
                            start,
                        );
                        self.pos = end;
                        return self.tok(T::DoubleCast, start, 0);
                    }
                    self.pos = end;
                    return self.tok(kind, start, 0);
                }
                self.pos = start + 1;
                self.open_bracket(b'(');
                self.tok(T::LParen, start, 0)
            }
            b')' | b']' => {
                self.pos = start + 1;
                self.close_bracket(c, start);
                self.tok(if c == b')' { T::RParen } else { T::RBracket }, start, 0)
            }
            b'[' => {
                self.pos = start + 1;
                self.open_bracket(b'[');
                self.tok(T::LBracket, start, 0)
            }
            b'{' => {
                self.pos = start + 1;
                self.open_bracket(b'{');
                self.stack.push(State::Scripting);
                self.tok(T::LBrace, start, 0)
            }
            b'}' => {
                self.pos = start + 1;
                self.close_bracket(b'}', start);
                if let Some(s) = self.stack.pop() {
                    self.state = s;
                }
                self.tok(T::RBrace, start, 0)
            }
            b'-' => {
                let (k, l) = match self.at(start + 1) {
                    b'>' => {
                        self.pos = start + 2;
                        self.stack.push(State::Scripting);
                        self.state = State::LookingForProperty;
                        return self.tok(T::ObjectOperator, start, 0);
                    }
                    b'-' => (T::Dec, 2),
                    b'=' => (T::MinusEqual, 2),
                    _ => (T::Minus, 1),
                };
                self.pos = start + l;
                self.tok(k, start, 0)
            }
            b'<' => {
                if self.at(start + 1) == b'<'
                    && self.at(start + 2) == b'<'
                    && let Some(t) = self.lex_heredoc_start(start, start + 3, 0)
                {
                    return t;
                }
                let (k, l) = match (self.at(start + 1), self.at(start + 2)) {
                    (b'<', b'=') => (T::SlEqual, 3),
                    (b'<', _) => (T::Sl, 2),
                    (b'=', b'>') => (T::Spaceship, 3),
                    (b'=', _) => (T::IsSmallerOrEqual, 2),
                    (b'>', _) => (T::IsNotEqual, 2),
                    _ => (T::Lt, 1),
                };
                self.pos = start + l;
                self.tok(k, start, 0)
            }
            _ => self.lex_operator(start),
        }
    }

    fn lex_operator(&mut self, start: usize) -> Token {
        let c = self.src[start];
        let n1 = self.at(start + 1);
        let n2 = self.at(start + 2);
        let (k, l) = match c {
            b';' => (T::Semicolon, 1),
            b',' => (T::Comma, 1),
            b':' => {
                if n1 == b':' {
                    (T::DoubleColon, 2)
                } else {
                    (T::Colon, 1)
                }
            }
            b'=' => match (n1, n2) {
                (b'=', b'=') => (T::IsIdentical, 3),
                (b'=', _) => (T::IsEqual, 2),
                (b'>', _) => (T::DoubleArrow, 2),
                _ => (T::Eq, 1),
            },
            b'!' => match (n1, n2) {
                (b'=', b'=') => (T::IsNotIdentical, 3),
                (b'=', _) => (T::IsNotEqual, 2),
                _ => (T::Bang, 1),
            },
            b'>' => match (n1, n2) {
                (b'>', b'=') => (T::SrEqual, 3),
                (b'>', _) => (T::Sr, 2),
                (b'=', _) => (T::IsGreaterOrEqual, 2),
                _ => (T::Gt, 1),
            },
            b'+' => match n1 {
                b'+' => (T::Inc, 2),
                b'=' => (T::PlusEqual, 2),
                _ => (T::Plus, 1),
            },
            b'*' => match (n1, n2) {
                (b'*', b'=') => (T::PowEqual, 3),
                (b'*', _) => (T::Pow, 2),
                (b'=', _) => (T::MulEqual, 2),
                _ => (T::Star, 1),
            },
            b'%' => {
                if n1 == b'=' {
                    (T::ModEqual, 2)
                } else {
                    (T::Percent, 1)
                }
            }
            b'&' => match n1 {
                b'&' => (T::BooleanAnd, 2),
                b'=' => (T::AndEqual, 2),
                _ => {
                    // PHP 8.1: `&` is split by what follows (observed on
                    // PHP 8.5 with token_get_all()).
                    let mut i = start + 1;
                    loop {
                        match self.at(i) {
                            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
                            b'#' if self.at(i + 1) != b'[' => i = self.skip_line_comment(i),
                            b'/' if self.at(i + 1) == b'/' => i = self.skip_line_comment(i),
                            b'/' if self.at(i + 1) == b'*' => i = self.skip_block_comment(i),
                            _ => break,
                        }
                    }
                    let followed = (self.at(i) == b'$')
                        || (self.at(i) == b'.' && self.at(i + 1) == b'.' && self.at(i + 2) == b'.');
                    if followed {
                        (T::AmpFollowedByVarOrVararg, 1)
                    } else {
                        (T::AmpNotFollowedByVarOrVararg, 1)
                    }
                }
            },
            b'|' => match n1 {
                b'|' => (T::BooleanOr, 2),
                b'=' => (T::OrEqual, 2),
                b'>' => (T::PipeGt, 2),
                _ => (T::Pipe, 1),
            },
            b'^' => {
                if n1 == b'=' {
                    (T::XorEqual, 2)
                } else {
                    (T::Caret, 1)
                }
            }
            b'~' => (T::Tilde, 1),
            b'@' => (T::At, 1),
            _ => (T::BadCharacter, 1),
        };
        self.pos = start + l;
        self.tok(k, start, 0)
    }

    fn skip_line_comment(&self, mut i: usize) -> usize {
        let src = self.src;
        while i < src.len() {
            match src[i] {
                b'\n' | b'\r' => break,
                b'?' if self.at(i + 1) == b'>' => break,
                _ => i += 1,
            }
        }
        i
    }

    fn skip_block_comment(&self, i: usize) -> usize {
        let src = self.src;
        let mut j = i + 2;
        while j + 1 < src.len() {
            if src[j] == b'*' && src[j + 1] == b'/' {
                return j + 2;
            }
            j += 1;
        }
        src.len()
    }

    fn lex_line_comment(&mut self, start: usize) -> Token {
        self.pos = self.skip_line_comment(start);
        self.tok(T::Comment, start, 0)
    }

    fn lex_block_comment(&mut self, start: usize) -> Token {
        let doc = self.at(start + 2) == b'*'
            && matches!(self.at(start + 3), b' ' | b'\t' | b'\n' | b'\r');
        let end = self.skip_block_comment(start);
        let terminated = end >= start + 4 && self.src[end - 2] == b'*' && self.src[end - 1] == b'/';
        if !terminated || end < start + 4 {
            let l = self.line;
            self.set_error(
                format!("Unterminated comment starting line {}", l),
                l,
                start,
            );
        }
        self.pos = end;
        self.tok(if doc { T::DocComment } else { T::Comment }, start, 0)
    }

    fn scan_label(&self, mut i: usize) -> usize {
        let src = self.src;
        while i < src.len() && is_label_char(src[i]) {
            i += 1;
        }
        i
    }

    /// Scans `label(\label)*` from `i` (at a label start).
    fn scan_qualified(&self, i: usize) -> usize {
        let mut i = self.scan_label(i);
        while self.at(i) == b'\\' && is_label_start(self.at(i + 1)) {
            i = self.scan_label(i + 1);
        }
        i
    }

    fn lex_label_or_name(&mut self, start: usize) -> Token {
        let c = self.src[start];
        // b"..." / b'...' / b<<<
        if (c == b'b' || c == b'B') && start + 1 < self.src.len() {
            match self.src[start + 1] {
                b'\'' => return self.lex_single_quoted(start, start + 2, TF_BINARY_PREFIX),
                b'"' => return self.lex_double_quoted(start, start + 2, TF_BINARY_PREFIX),
                b'<' if self.at(start + 2) == b'<' && self.at(start + 3) == b'<' => {
                    if let Some(t) = self.lex_heredoc_start(start, start + 4, TF_BINARY_PREFIX) {
                        return t;
                    }
                }
                _ => {}
            }
        }
        let end = self.scan_label(start);
        if self.at(end) == b'\\' && is_label_start(self.at(end + 1)) {
            let e = self.scan_qualified(end + 1);
            self.pos = e;
            let kind = if eq_ic(&self.src[start..end], b"namespace") {
                T::NameRelative
            } else {
                T::NameQualified
            };
            return self.tok(kind, start, 0);
        }
        let text = &self.src[start..end];
        let kind = match keyword(text) {
            None => T::String,
            Some(T::Yield) => {
                // `yield from`, with whitespace and comments between
                // (observed on PHP 8.5).
                let mut i = end;
                let mut any = false;
                loop {
                    match self.at(i) {
                        b' ' | b'\t' | b'\n' | b'\r' => i += 1,
                        b'#' if self.at(i + 1) != b'[' => i = self.skip_line_comment(i),
                        b'/' if self.at(i + 1) == b'/' => i = self.skip_line_comment(i),
                        b'/' if self.at(i + 1) == b'*' => i = self.skip_block_comment(i),
                        _ => break,
                    }
                    any = true;
                }
                if any
                    && i + 4 <= self.src.len()
                    && eq_ic(&self.src[i..i + 4], b"from")
                    && !is_label_char(self.at(i + 4))
                {
                    self.pos = i + 4;
                    return self.tok(T::YieldFrom, start, 0);
                }
                T::Yield
            }
            Some(T::Enum) => {
                // `enum` is a keyword only before a name that is not
                // `extends` or `implements` (observed on PHP 8.5).
                let mut i = end;
                let mut any = false;
                loop {
                    match self.at(i) {
                        b' ' | b'\t' | b'\n' | b'\r' => i += 1,
                        b'#' if self.at(i + 1) != b'[' => i = self.skip_line_comment(i),
                        b'/' if self.at(i + 1) == b'/' => i = self.skip_line_comment(i),
                        b'/' if self.at(i + 1) == b'*' => i = self.skip_block_comment(i),
                        _ => break,
                    }
                    any = true;
                }
                if any && is_label_start(self.at(i)) {
                    let e = self.scan_label(i);
                    let w = &self.src[i..e];
                    if eq_ic(w, b"extends") || eq_ic(w, b"implements") {
                        T::String
                    } else {
                        T::Enum
                    }
                } else {
                    T::String
                }
            }
            Some(k @ (T::Public | T::Private | T::Protected)) => {
                // PHP 8.4 asymmetric visibility: `public(set)` is one token.
                if end + 5 <= self.src.len() && eq_ic(&self.src[end..end + 5], b"(set)") {
                    self.pos = end + 5;
                    let k = match k {
                        T::Public => T::PublicSet,
                        T::Private => T::PrivateSet,
                        _ => T::ProtectedSet,
                    };
                    return self.tok(k, start, 0);
                }
                k
            }
            Some(k) => k,
        };
        self.pos = end;
        self.tok(kind, start, 0)
    }

    /// A cast token at `start` (`(` already seen): `( type )` with spaces or
    /// tabs. `T::BadCharacter` stands for the removed `(real)` cast.
    fn cast_at(&self, start: usize) -> Option<(T, usize)> {
        let src = self.src;
        let mut i = start + 1;
        while matches!(self.at(i), b' ' | b'\t') {
            i += 1;
        }
        let ws = i;
        while i < src.len() && src[i].is_ascii_alphabetic() {
            i += 1;
        }
        if i == ws || i - ws > 7 {
            return None;
        }
        let word = &src[ws..i];
        while matches!(self.at(i), b' ' | b'\t') {
            i += 1;
        }
        if self.at(i) != b')' {
            return None;
        }
        let mut buf = [0u8; 7];
        for (k, &b) in word.iter().enumerate() {
            buf[k] = b.to_ascii_lowercase();
        }
        let kind = match &buf[..word.len()] {
            b"int" | b"integer" => T::IntCast,
            b"bool" | b"boolean" => T::BoolCast,
            b"float" | b"double" => T::DoubleCast,
            b"real" => T::BadCharacter,
            b"string" | b"binary" => T::StringCast,
            b"array" => T::ArrayCast,
            b"object" => T::ObjectCast,
            b"unset" => T::UnsetCast,
            b"void" => T::VoidCast,
            _ => return None,
        };
        Some((kind, i + 1))
    }

    // ----- numbers -----

    fn lex_number(&mut self, start: usize) -> Token {
        let src = self.src;
        let c0 = src[start];
        if c0 == b'0' {
            let p = self.at(start + 1);
            let (radix, ok): (u32, fn(u8) -> bool) = match p {
                b'x' | b'X' => (16, |c: u8| c.is_ascii_hexdigit()),
                b'b' | b'B' => (2, |c: u8| c == b'0' || c == b'1'),
                b'o' | b'O' => (8, |c: u8| (b'0'..=b'7').contains(&c)),
                _ => (0, |_| false),
            };
            if radix != 0 {
                if ok(self.at(start + 2)) {
                    let mut i = start + 3;
                    loop {
                        let c = self.at(i);
                        if ok(c) {
                            i += 1;
                        } else if c == b'_' && ok(self.at(i + 1)) {
                            i += 2;
                        } else {
                            break;
                        }
                    }
                    self.pos = i;
                    let digits = &src[start + 2..i];
                    let kind = if fits_i64_radix(digits, radix) {
                        T::LNumber
                    } else {
                        T::DNumber
                    };
                    return self.tok(kind, start, 0);
                }
                // `0x` alone: the integer 0, then a label.
                self.pos = start + 1;
                return self.tok(T::LNumber, start, 0);
            }
        }
        // Decimal: digits with `_` between digits, optional fraction and
        // exponent (PHP manual: "Integers", "Floating point numbers").
        let scan_digits = |mut i: usize| -> usize {
            while i < src.len() {
                let c = src[i];
                if c.is_ascii_digit() {
                    i += 1;
                } else if c == b'_' && i + 1 < src.len() && src[i + 1].is_ascii_digit() {
                    i += 2;
                } else {
                    break;
                }
            }
            i
        };
        let mut is_float = false;
        let mut i = start;
        if c0 != b'.' {
            i = scan_digits(i);
        }
        let int_end = i;
        if self.at(i) == b'.' {
            if self.at(i + 1).is_ascii_digit() {
                is_float = true;
                i = scan_digits(i + 1);
            } else if c0 != b'.' {
                is_float = true;
                i += 1;
            }
        }
        if matches!(self.at(i), b'e' | b'E') {
            let mut j = i + 1;
            if matches!(self.at(j), b'+' | b'-') {
                j += 1;
            }
            if self.at(j).is_ascii_digit() {
                is_float = true;
                i = scan_digits(j);
            }
        }
        self.pos = i;
        if is_float {
            return self.tok(T::DNumber, start, 0);
        }
        let digits = &src[start..int_end];
        if c0 == b'0' && digits.len() > 1 {
            // Legacy octal: every digit must be 0-7.
            if digits.iter().any(|&c| c == b'8' || c == b'9') {
                let l = self.line;
                self.set_error("Invalid numeric literal", l, start);
                return self.tok(T::LNumber, start, 0);
            }
            let kind = if fits_i64_radix(&digits[1..], 8) {
                T::LNumber
            } else {
                T::DNumber
            };
            return self.tok(kind, start, 0);
        }
        let kind = if fits_i64_radix(digits, 10) {
            T::LNumber
        } else {
            T::DNumber
        };
        self.tok(kind, start, 0)
    }

    // ----- strings -----

    fn lex_single_quoted(&mut self, start: usize, body: usize, flags: u16) -> Token {
        let src = self.src;
        let mut i = body;
        let mut esc = false;
        while i < src.len() {
            match src[i] {
                b'\'' => {
                    self.pos = i + 1;
                    let f = flags | if esc { TF_ESCAPES } else { 0 };
                    return self.tok(T::ConstantEncapsedString, start, f);
                }
                b'\\' => {
                    esc = true;
                    i += 2;
                }
                _ => i += 1,
            }
        }
        // Unterminated: the rest is string content (observed on PHP 8.5).
        self.pos = src.len();
        self.tok(T::EncapsedAndWhitespace, start, flags)
    }

    /// Validates the `\u{...}` escapes of a double-quoted literal segment.
    fn check_escapes(&mut self, from: usize, to: usize, line: u32) {
        if !self.opts.parse_mode || self.error.is_some() {
            return;
        }
        let src = self.src;
        let mut i = from;
        let mut line = line;
        while i < to {
            match src[i] {
                b'\\' => {
                    if i + 1 < to && src[i + 1] == b'u' && i + 2 < to && src[i + 2] == b'{' {
                        let mut j = i + 3;
                        let mut v: u64 = 0;
                        while j < to && src[j].is_ascii_hexdigit() {
                            v = v
                                .saturating_mul(16)
                                .saturating_add((src[j] as char).to_digit(16).unwrap() as u64);
                            j += 1;
                        }
                        if j == i + 3 || j >= to || src[j] != b'}' {
                            self.set_error("Invalid UTF-8 codepoint escape sequence", line, i);
                            return;
                        }
                        if v > 0x10FFFF {
                            self.set_error(
                                "Invalid UTF-8 codepoint escape sequence: Codepoint too large",
                                line,
                                i,
                            );
                            return;
                        }
                        i = j + 1;
                        continue;
                    }
                    i += 1;
                    if i < to {
                        if src[i] == b'\n' || (src[i] == b'\r' && self.at(i + 1) != b'\n') {
                            line += 1;
                        }
                        i += 1;
                    }
                }
                b'\n' => {
                    line += 1;
                    i += 1;
                }
                b'\r' => {
                    if self.at(i + 1) != b'\n' {
                        line += 1;
                    }
                    i += 1;
                }
                _ => i += 1,
            }
        }
    }

    fn lex_double_quoted(&mut self, start: usize, body: usize, flags: u16) -> Token {
        let src = self.src;
        let mut i = body;
        let mut esc = false;
        while i < src.len() {
            match src[i] {
                b'"' => {
                    self.pos = i + 1;
                    let f = flags | TF_DOUBLE | if esc { TF_ESCAPES } else { 0 };
                    if esc {
                        let l = self.line;
                        self.check_escapes(body, i, l);
                    }
                    return self.tok(T::ConstantEncapsedString, start, f);
                }
                b'\\' => {
                    esc = true;
                    i += 2;
                }
                b'$' if is_label_start(self.at(i + 1)) || self.at(i + 1) == b'{' => break,
                b'{' if self.at(i + 1) == b'$' => break,
                _ => i += 1,
            }
        }
        // Interpolated (or unterminated): `"` then the parts.
        self.pos = body;
        self.stack.push(State::Scripting);
        self.state = State::DoubleQuotes;
        self.tok(T::DoubleQuote, start, flags)
    }

    /// Lexes inside `"..."` or `` `...` `` after the opening quote.
    fn lex_dq_body(&mut self, quote: u8) -> Token {
        let src = self.src;
        let start = self.pos;
        let c = src[start];
        if c == quote {
            self.pos = start + 1;
            self.state = self.stack.pop().unwrap_or(State::Scripting);
            return self.tok(
                if quote == b'"' {
                    T::DoubleQuote
                } else {
                    T::Backtick
                },
                start,
                0,
            );
        }
        if let Some(t) = self.lex_interpolation_start(start) {
            return t;
        }
        let mut i = start;
        let mut esc = false;
        while i < src.len() {
            match src[i] {
                b'\\' => {
                    esc = true;
                    i += 2;
                }
                b'$' if is_label_start(self.at(i + 1)) || self.at(i + 1) == b'{' => break,
                b'{' if self.at(i + 1) == b'$' => break,
                x if x == quote => break,
                _ => i += 1,
            }
        }
        let i = i.min(src.len());
        self.pos = i;
        if esc && quote == b'"' {
            let l = self.line;
            self.check_escapes(start, i, l);
        }
        self.tok(
            T::EncapsedAndWhitespace,
            start,
            if esc { TF_ESCAPES } else { 0 },
        )
    }

    /// `$var`, `${` or `{$` at `start` inside an interpolated string.
    fn lex_interpolation_start(&mut self, start: usize) -> Option<Token> {
        let c = self.src[start];
        if c == b'$' {
            let n = self.at(start + 1);
            if is_label_start(n) {
                let end = self.scan_label(start + 1);
                self.pos = end;
                // What follows decides the next state.
                if self.at(end) == b'[' {
                    self.stack.push(self.state);
                    self.state = State::VarOffset;
                } else if (self.at(end) == b'-'
                    && self.at(end + 1) == b'>'
                    && is_label_start(self.at(end + 2)))
                    || (self.at(end) == b'?'
                        && self.at(end + 1) == b'-'
                        && self.at(end + 2) == b'>'
                        && is_label_start(self.at(end + 3)))
                {
                    self.stack.push(self.state);
                    self.state = State::PropertyInString;
                }
                return Some(self.tok(T::Variable, start, 0));
            }
            if n == b'{' {
                self.pos = start + 2;
                self.open_bracket(b'{');
                self.stack.push(self.state);
                self.state = State::LookingForVarname;
                return Some(self.tok(T::DollarOpenCurlyBraces, start, 0));
            }
        } else if c == b'{' && self.at(start + 1) == b'$' {
            self.pos = start + 1;
            self.open_bracket(b'{');
            self.stack.push(self.state);
            self.state = State::Scripting;
            return Some(self.tok(T::CurlyOpen, start, 0));
        }
        None
    }

    fn lex_var_offset(&mut self) -> Token {
        let src = self.src;
        let start = self.pos;
        let c = src[start];
        match c {
            b'0'..=b'9' => {
                // Any integer literal form is a T_NUM_STRING here
                // (observed on PHP 8.5: "$a[0x1]", "$a[1_0]").
                let mut i = start + 1;
                if c == b'0'
                    && matches!(self.at(i), b'x' | b'X' | b'b' | b'B' | b'o' | b'O')
                    && self.at(i + 1).is_ascii_hexdigit()
                {
                    i += 1;
                    while self.at(i).is_ascii_hexdigit()
                        || (self.at(i) == b'_' && self.at(i + 1).is_ascii_hexdigit())
                    {
                        i += 1;
                    }
                } else {
                    while self.at(i).is_ascii_digit()
                        || (self.at(i) == b'_' && self.at(i + 1).is_ascii_digit())
                    {
                        i += 1;
                    }
                }
                self.pos = i;
                self.tok(T::NumString, start, 0)
            }
            b'$' if is_label_start(self.at(start + 1)) => {
                self.pos = self.scan_label(start + 1);
                self.tok(T::Variable, start, 0)
            }
            b']' => {
                self.pos = start + 1;
                self.state = self.stack.pop().unwrap_or(State::Scripting);
                self.tok(T::RBracket, start, 0)
            }
            b' ' | b'\n' | b'\r' | b'\t' | b'\\' | b'\'' | b'#' => {
                // Invalid character: an empty string part, back to the
                // string (observed on PHP 8.5).
                self.state = self.stack.pop().unwrap_or(State::Scripting);
                self.tok(T::EncapsedAndWhitespace, start, 0)
            }
            _ if is_label_start(c) => {
                self.pos = self.scan_label(start);
                self.tok(T::String, start, 0)
            }
            b';' | b':' | b',' | b'.' | b'[' | b'(' | b')' | b'|' | b'^' | b'&' | b'+' | b'-'
            | b'/' | b'*' | b'=' | b'%' | b'!' | b'~' | b'$' | b'<' | b'>' | b'?' | b'@' | b'{'
            | b'}' | b'"' | b'`' => {
                self.pos = start + 1;
                self.tok(T::from_id(c as u16).unwrap_or(T::BadCharacter), start, 0)
            }
            _ => {
                self.pos = start + 1;
                self.tok(T::BadCharacter, start, 0)
            }
        }
    }

    fn lex_looking_for_varname(&mut self) -> Token {
        let start = self.pos;
        if is_label_start(self.src[start]) {
            let end = self.scan_label(start);
            if matches!(self.at(end), b'[' | b'}') {
                self.pos = end;
                self.state = State::Scripting;
                return self.tok(T::StringVarname, start, 0);
            }
        }
        self.state = State::Scripting;
        self.lex_scripting()
    }

    fn lex_property_in_string(&mut self) -> Token {
        let start = self.pos;
        if self.src[start] == b'-' {
            self.pos = start + 2;
            return self.tok(T::ObjectOperator, start, 0);
        }
        if self.src[start] == b'?' {
            self.pos = start + 3;
            return self.tok(T::NullsafeObjectOperator, start, 0);
        }
        let end = self.scan_label(start);
        self.pos = end;
        self.state = self.stack.pop().unwrap_or(State::Scripting);
        self.tok(T::String, start, 0)
    }

    fn lex_looking_for_property(&mut self) -> Token {
        let src = self.src;
        let start = self.pos;
        let c = src[start];
        match c {
            b' ' | b'\t' | b'\n' | b'\r' => {
                let mut i = start + 1;
                while i < src.len() && matches!(src[i], b' ' | b'\t' | b'\n' | b'\r') {
                    i += 1;
                }
                self.pos = i;
                self.tok(T::Whitespace, start, 0)
            }
            b'-' if self.at(start + 1) == b'>' => {
                self.pos = start + 2;
                self.tok(T::ObjectOperator, start, 0)
            }
            b'?' if self.at(start + 1) == b'-' && self.at(start + 2) == b'>' => {
                self.pos = start + 3;
                self.tok(T::NullsafeObjectOperator, start, 0)
            }
            b'#' if self.at(start + 1) != b'[' => self.lex_line_comment(start),
            b'/' if self.at(start + 1) == b'/' => self.lex_line_comment(start),
            b'/' if self.at(start + 1) == b'*' => self.lex_block_comment(start),
            _ if is_label_start(c) => {
                self.pos = self.scan_label(start);
                self.state = self.stack.pop().unwrap_or(State::Scripting);
                self.tok(T::String, start, 0)
            }
            _ => {
                self.state = self.stack.pop().unwrap_or(State::Scripting);
                self.lex_scripting()
            }
        }
    }

    // ----- heredoc / nowdoc -----

    /// `<<<` at `start`, label from `p`.
    fn lex_heredoc_start(&mut self, start: usize, p: usize, flags: u16) -> Option<Token> {
        let src = self.src;
        let mut i = p;
        while matches!(self.at(i), b' ' | b'\t') {
            i += 1;
        }
        let quote = self.at(i);
        let (nowdoc, double) = match quote {
            b'\'' => (true, false),
            b'"' => (false, true),
            _ => (false, false),
        };
        if nowdoc || double {
            i += 1;
        }
        if !is_label_start(self.at(i)) {
            return None;
        }
        let label_start = i;
        let label_end = self.scan_label(i);
        i = label_end;
        if nowdoc || double {
            if self.at(i) != quote {
                return None;
            }
            i += 1;
        }
        match self.at(i) {
            b'\n' => i += 1,
            b'\r' => {
                i += 1;
                if self.at(i) == b'\n' {
                    i += 1;
                }
            }
            _ => return None,
        }
        self.pos = i;
        let doc = Heredoc {
            label_start: label_start as u32,
            label_len: (label_end - label_start) as u32,
            indent: 0,
            indent_char: 0,
        };
        let t = self.tok(
            T::StartHeredoc,
            start,
            flags | if double || !nowdoc { TF_DOUBLE } else { 0 },
        );
        self.heredocs.push(doc);
        self.stack.push(State::Scripting);
        self.state = if nowdoc {
            State::Nowdoc
        } else {
            State::Heredoc
        };
        // Lex ahead to the closing marker to learn its indentation (PHP 7.3
        // flexible heredoc RFC). Real lexing, not a text search: nested
        // heredocs may reuse the label (test
        // Zend/tests/heredoc_nowdoc/flexible-heredoc-complex-test2.phpt).
        let depth = self.heredocs.len();
        let mut scan = Lexer {
            src: self.src,
            pos: self.pos,
            line: self.line,
            state: self.state,
            stack: self.stack.clone(),
            heredocs: self.heredocs.clone(),
            opts: LexerOptions {
                parse_mode: false,
                ..self.opts
            },
            brackets: Vec::new(),
            error: None,
            halt: 0,
            halted: false,
            finished: false,
        };
        let mut mixed = false;
        while let Some(st) = scan.next_token() {
            if st.kind == T::EndHeredoc && scan.heredocs.len() < depth {
                let text = st.text(src);
                let n = text
                    .iter()
                    .take_while(|&&c| c == b' ' || c == b'\t')
                    .count();
                let d = self.heredocs.last_mut().expect("heredoc");
                d.indent = n as u32;
                if n > 0 {
                    d.indent_char = text[0];
                    mixed = text[..n].iter().any(|&b| b != text[0]);
                }
                break;
            }
        }
        if mixed {
            let l = self.line;
            self.set_error(
                "Invalid indentation - tabs and spaces cannot be mixed",
                l,
                i,
            );
        }
        Some(t)
    }

    /// The closing marker at line start `ls`: its end.
    fn closing_marker_at(&self, ls: usize) -> Option<usize> {
        let doc = self.heredocs.last()?;
        let src = self.src;
        let mut j = ls;
        while matches!(self.at(j), b' ' | b'\t') {
            j += 1;
        }
        let label = &src[doc.label_start as usize..(doc.label_start + doc.label_len) as usize];
        // The marker needs a following character: at end of input it is
        // body text (observed on PHP 8.5).
        if j + label.len() < src.len()
            && &src[j..j + label.len()] == label
            && !is_label_char(src[j + label.len()])
        {
            Some(j + label.len())
        } else {
            None
        }
    }

    fn end_heredoc(&mut self, start: usize, end: usize) -> Token {
        self.pos = end;
        self.heredocs.pop();
        self.state = self.stack.pop().unwrap_or(State::Scripting);
        self.tok(T::EndHeredoc, start, 0)
    }

    /// Checks the indentation of the body line starting at `ls`.
    fn check_indent(&mut self, ls: usize, line: u32) {
        if !self.opts.parse_mode || self.error.is_some() {
            return;
        }
        let Some(doc) = self.heredocs.last().copied() else {
            return;
        };
        if doc.indent == 0 {
            return;
        }
        let src = self.src;
        let mut j = ls;
        let lim = ls + doc.indent as usize;
        while j < lim && matches!(self.at(j), b' ' | b'\t') {
            if src[j] != doc.indent_char {
                self.set_error(
                    "Invalid indentation - tabs and spaces cannot be mixed",
                    line,
                    j,
                );
                return;
            }
            j += 1;
        }
        if j < lim && j < src.len() && !matches!(src[j], b'\n' | b'\r') {
            self.set_error(
                format!(
                    "Invalid body indentation level (expecting an indentation level of at least {})",
                    doc.indent
                ),
                line,
                j,
            );
        }
    }

    fn lex_heredoc_body(&mut self) -> Token {
        let src = self.src;
        let start = self.pos;
        let at_line_start = start == 0 || matches!(src[start - 1], b'\n' | b'\r');
        if at_line_start && let Some(end) = self.closing_marker_at(start) {
            return self.end_heredoc(start, end);
        }
        if at_line_start {
            let l = self.line;
            self.check_indent(start, l);
        }
        if let Some(t) = self.lex_interpolation_start(start) {
            return t;
        }
        let mut i = start;
        let mut esc = false;
        let mut line = self.line;
        while i < src.len() {
            match src[i] {
                b'\\' => {
                    esc = true;
                    if matches!(self.at(i + 1), b'\n' | b'\r') {
                        i += 1;
                    } else {
                        i += 2;
                    }
                }
                b'$' if is_label_start(self.at(i + 1)) || self.at(i + 1) == b'{' => break,
                b'{' if self.at(i + 1) == b'$' => break,
                b'\n' | b'\r' => {
                    i += 1;
                    if src[i - 1] == b'\r' && self.at(i) == b'\n' {
                        i += 1;
                    }
                    line += 1;
                    if self.closing_marker_at(i).is_some() {
                        break;
                    }
                    self.check_indent(i, line);
                }
                _ => i += 1,
            }
        }
        let i = i.min(src.len());
        self.pos = i;
        if esc {
            let l = self.line;
            self.check_escapes(start, i, l);
        }
        self.tok(
            T::EncapsedAndWhitespace,
            start,
            if esc { TF_ESCAPES } else { 0 },
        )
    }

    fn lex_nowdoc_body(&mut self) -> Token {
        let src = self.src;
        let start = self.pos;
        if let Some(end) = self.closing_marker_at(start) {
            return self.end_heredoc(start, end);
        }
        let mut i = start;
        let mut line = self.line;
        self.check_indent(start, line);
        while i < src.len() {
            if matches!(src[i], b'\n' | b'\r') {
                i += 1;
                if src[i - 1] == b'\r' && self.at(i) == b'\n' {
                    i += 1;
                }
                line += 1;
                if self.closing_marker_at(i).is_some() {
                    break;
                }
                self.check_indent(i, line);
            } else {
                i += 1;
            }
        }
        self.pos = i;
        self.tok(T::EncapsedAndWhitespace, start, 0)
    }
}

/// Whether the digits (with `_` separators) of an integer literal fit in a
/// PHP int.
fn fits_i64_radix(digits: &[u8], radix: u32) -> bool {
    let mut v: u64 = 0;
    for &c in digits {
        if c == b'_' {
            continue;
        }
        let d = (c as char).to_digit(radix).unwrap_or(0) as u64;
        v = match v.checked_mul(radix as u64).and_then(|v| v.checked_add(d)) {
            Some(v) => v,
            None => return false,
        };
    }
    v <= i64::MAX as u64
}

#[inline]
pub(crate) fn memchr(c: u8, s: &[u8]) -> Option<usize> {
    s.iter().position(|&b| b == c)
}

/// Tokenizes a whole source the way `token_get_all()` does (no parser
/// checks).
pub fn tokenize(src: &[u8], short_open_tag: bool) -> Vec<Token> {
    let mut lx = Lexer::new(
        src,
        LexerOptions {
            short_open_tag,
            parse_mode: false,
        },
    );
    let mut out = Vec::with_capacity(src.len() / 4 + 4);
    while let Some(t) = lx.next_token() {
        out.push(t);
    }
    out
}
