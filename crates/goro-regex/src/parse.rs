//! Pattern parser: PCRE2 syntax (pcre2pattern, pcre2syntax) to [`Node`]s.
//!
//! Error kinds and offsets follow what PHP 8.5 (PCRE2 10.46) reports; the
//! offsets were observed with batched spot checks (see tests/fixtures).

use crate::Flags;
use crate::ast::*;
use crate::class::{self, CaseMode, CharSet, ClassBuilder, HighItem};
use crate::error::{CompileError, ErrorKind as E};
use crate::unicode;
use crate::utf8;
use std::sync::Arc;

type PResult<T> = Result<T, CompileError>;

/// Maximum nesting of parentheses (PCRE2's default parens_nest_limit, as
/// observed: 250 levels compile, 251 do not).
const PARENS_NEST_LIMIT: u32 = 250;
const MAX_NAME_LEN: usize = 128;
const MAX_VERB_NAME: usize = 255;
const MAX_QUANT: u32 = 65535;
/// The version (?(VERSION...) compares against.
const VERSION: (u32, u32) = (10, 46);

/// Options that can change inside a pattern.
#[derive(Clone, Copy, Debug, Default)]
pub struct Opts {
    pub caseless: bool,
    pub multiline: bool,
    pub dotall: bool,
    pub extended: bool,
    pub extended_more: bool,
    pub no_auto_capture: bool,
    pub ungreedy: bool,
    pub dupnames: bool,
    pub caseless_restrict: bool,
    pub ascii_bsd: bool,
    pub ascii_bss: bool,
    pub ascii_bsw: bool,
    pub ascii_posix: bool,
    pub ascii_digit: bool,
}

/// The result of parsing.
#[derive(Debug)]
pub struct Parsed {
    pub node: Node,
    pub ncap: u32,
    /// (name, group number) pairs, in order of definition.
    pub names: Vec<(Arc<[u8]>, u32)>,
    pub utf: bool,
    pub ucp: bool,
    pub newline: Newline,
    pub bsr_anycrlf: bool,
    pub no_start_opt: bool,
    pub no_auto_possess: bool,
    pub no_dotstar_anchor: bool,
    pub notempty: bool,
    pub notempty_atstart: bool,
    pub limit_match: Option<u32>,
    pub limit_depth: Option<u32>,
    pub limit_heap: Option<u32>,
    pub has_branch_reset: bool,
    pub has_dup_names: bool,
    pub turkish: bool,
    pub has_cr_lf: bool,
    pub has_ext_class: bool,
}

/// Escapes recognised by [`Parser::escape`].
enum Esc {
    Lit(u32),
    /// `\d \s \w \h \v` and negations.
    Type(CharType, bool),
    Prop(PropSpec),
    NotNewline,
    Newline,
    Grapheme,
    CodeUnit,
    Assert(Assertion),
    Keep,
    Backref(GroupRef),
    Call(GroupRef),
    QuoteStart,
    QuoteEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CharType {
    Digit,
    Space,
    Word,
    HSpace,
    VSpace,
}

#[derive(Clone, Copy, Debug)]
enum PropKind {
    Any,
    Gc(u32),
    Script(u8),
    Scx(u8),
    Bidi(u8),
    Bprop(u8),
    Ascii,
    Xan,
    Xps,
    Xwd,
    Xuc,
}

#[derive(Clone, Copy, Debug)]
struct PropSpec {
    kind: PropKind,
    neg: bool,
}

/// One branch under construction.
struct Branch {
    items: Vec<Node>,
    /// Whether the last item can take a quantifier.
    repeatable: bool,
}

pub struct Parser<'a> {
    p: &'a [u8],
    i: usize,
    utf: bool,
    ucp: bool,
    o: Opts,
    dollar_endonly: bool,
    newline: Newline,
    bsr_anycrlf: bool,
    turkish: bool,
    allow_empty_class: bool,
    ncap: u32,
    names: Vec<(Arc<[u8]>, u32)>,
    depth: u32,
    lookaround_depth: u32,
    keep_in_lookaround: bool,
    has_branch_reset: bool,
    has_dup_names: bool,
    /// Group numbers that already have a name (for the "different names"
    /// check in branch-reset groups).
    group_names: Vec<(u32, Arc<[u8]>)>,
    has_cr_lf: bool,
    has_ext_class: bool,
}

#[inline]
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

impl<'a> Parser<'a> {
    fn err<T>(&self, kind: E, off: usize) -> PResult<T> {
        Err(CompileError::new(kind, off))
    }

    #[inline]
    fn peek(&self) -> Option<u8> {
        self.p.get(self.i).copied()
    }

    #[inline]
    fn peek_at(&self, k: usize) -> Option<u8> {
        self.p.get(self.i + k).copied()
    }

    fn at_end(&self) -> bool {
        self.i >= self.p.len()
    }

    fn starts_with(&self, s: &[u8]) -> bool {
        self.p[self.i..].starts_with(s)
    }

    /// Reads one literal character at the current position.
    fn read_char(&mut self) -> u32 {
        if self.utf && self.p[self.i] >= 0x80 {
            let (c, l) = utf8::decode(self.p, self.i);
            self.i += l;
            c
        } else {
            let c = self.p[self.i] as u32;
            self.i += 1;
            c
        }
    }

    fn case_mode(&self) -> CaseMode {
        if !self.o.caseless {
            CaseMode::None
        } else if self.utf || self.ucp {
            if self.o.caseless_restrict {
                CaseMode::UnicodeRestrict
            } else {
                CaseMode::Unicode
            }
        } else {
            CaseMode::Ascii
        }
    }

    /// Whether `c` has other cases under the current options.
    fn has_other_case(&self, c: u32) -> bool {
        let mode = self.case_mode();
        if self.turkish && mode != CaseMode::None && matches!(c, 0x49 | 0x69 | 0x130 | 0x131) {
            return true;
        }
        let mut v = Vec::new();
        class::case_equivalents(c, mode, &mut v);
        v.len() > 1
    }

    fn char_node(&mut self, c: u32) -> Node {
        if c == 10 || c == 13 {
            self.has_cr_lf = true;
        }
        if self.o.caseless && self.has_other_case(c) {
            if self.turkish && matches!(c, 0x49 | 0x69 | 0x130 | 0x131) {
                // Turkish casing (pcre2unicode): i = U+0130, U+0131 = I.
                let mut b = ClassBuilder::new();
                match c {
                    0x69 | 0x130 => {
                        b.add_char(0x69);
                        b.add_char(0x130);
                    }
                    _ => {
                        b.add_char(0x49);
                        b.add_char(0x131);
                    }
                }
                return Node::Set(Arc::new(b.build(false)));
            }
            if self.case_mode() == CaseMode::UnicodeRestrict {
                let mut b = ClassBuilder::new();
                b.add_char_caseless(c, CaseMode::UnicodeRestrict);
                return Node::Set(Arc::new(b.build(false)));
            }
            Node::Char { c, caseless: true }
        } else {
            Node::Char { c, caseless: false }
        }
    }

    /// Whitespace for the x option (pcre2api PCRE2_EXTENDED).
    fn is_pattern_space(&self, at: usize) -> Option<usize> {
        let b = *self.p.get(at)?;
        if matches!(b, 9..=13 | 32) {
            return Some(1);
        }
        if self.utf && b >= 0x80 {
            let (c, l) = utf8::decode(self.p, at);
            if matches!(c, 0x85 | 0x200e | 0x200f | 0x2028 | 0x2029) {
                return Some(l);
            }
        }
        None
    }

    /// Length of a newline sequence at `at` under the current convention.
    fn newline_len_at(&self, at: usize) -> Option<usize> {
        let b = *self.p.get(at)?;
        let next = self.p.get(at + 1).copied();
        match self.newline {
            Newline::Lf => (b == b'\n').then_some(1),
            Newline::Cr => (b == b'\r').then_some(1),
            Newline::CrLf => (b == b'\r' && next == Some(b'\n')).then_some(2),
            Newline::Nul => (b == 0).then_some(1),
            Newline::AnyCrLf => match b {
                b'\r' if next == Some(b'\n') => Some(2),
                b'\r' | b'\n' => Some(1),
                _ => None,
            },
            Newline::Any => match b {
                b'\r' if next == Some(b'\n') => Some(2),
                0x0a..=0x0d => Some(1),
                0x85 if !self.utf => Some(1),
                0xc2 if self.utf && next == Some(0x85) => Some(2),
                0xe2 if self.utf
                    && next == Some(0x80)
                    && matches!(self.p.get(at + 2), Some(0xa8) | Some(0xa9)) =>
                {
                    Some(3)
                }
                _ => None,
            },
        }
    }

    /// Skips white space and #-comments in extended mode.
    fn skip_extended(&mut self) {
        if !self.o.extended {
            return;
        }
        loop {
            if let Some(l) = self.is_pattern_space(self.i) {
                self.i += l;
                continue;
            }
            if self.peek() == Some(b'#') {
                self.i += 1;
                while !self.at_end() {
                    if let Some(l) = self.newline_len_at(self.i) {
                        self.i += l;
                        break;
                    }
                    self.i += 1;
                }
                continue;
            }
            break;
        }
    }

    // -----------------------------------------------------------------------
    // Top level

    fn parse_alternation(&mut self, reset: Option<u32>) -> PResult<(Node, usize)> {
        let mut branches = Vec::new();
        let mut max_cap = self.ncap;
        loop {
            let b = self.parse_branch()?;
            branches.push(b);
            if self.peek() == Some(b'|') {
                self.i += 1;
                if let Some(start) = reset {
                    max_cap = max_cap.max(self.ncap);
                    self.ncap = start;
                }
                continue;
            }
            break;
        }
        if reset.is_some() {
            self.ncap = max_cap.max(self.ncap);
        }
        let n = branches.len();
        let node = if n == 1 {
            branches.pop().unwrap()
        } else {
            Node::Alt(branches)
        };
        Ok((node, n))
    }

    fn parse_branch(&mut self) -> PResult<Node> {
        let mut br = Branch {
            items: Vec::new(),
            repeatable: false,
        };
        loop {
            self.skip_extended();
            let Some(b) = self.peek() else { break };
            match b {
                b'|' | b')' => break,
                b'*' | b'+' | b'?' => {
                    let start = self.i;
                    self.i += 1;
                    let (min, max) = match b {
                        b'*' => (0, None),
                        b'+' => (1, None),
                        _ => (0, Some(1)),
                    };
                    self.apply_quantifier(&mut br, min, max, start)?;
                }
                b'{' => {
                    let start = self.i;
                    match self.try_brace_quantifier()? {
                        Some((min, max)) => self.apply_quantifier(&mut br, min, max, start)?,
                        None => {
                            self.i += 1;
                            br.items.push(self.char_node(b'{' as u32));
                            br.repeatable = true;
                        }
                    }
                }
                _ => self.parse_atom(&mut br)?,
            }
        }
        Ok(Node::seq(br.items))
    }

    /// Parses `{n}`, `{n,}`, `{n,m}` or `{,m}` at the current position. Returns
    /// None (without moving) when the text is not a quantifier.
    fn try_brace_quantifier(&mut self) -> PResult<Option<(u32, Option<u32>)>> {
        let p = self.p;
        let mut j = self.i + 1;
        let skip_sp = |j: &mut usize| {
            while *j < p.len() && (p[*j] == b' ' || p[*j] == b'\t') {
                *j += 1;
            }
        };
        let read_num = |j: &mut usize| -> Option<u64> {
            let s = *j;
            let mut v: u64 = 0;
            while *j < p.len() && p[*j].is_ascii_digit() {
                v = (v * 10 + (p[*j] - b'0') as u64).min(u64::MAX / 100);
                *j += 1;
            }
            (*j > s).then_some(v)
        };
        skip_sp(&mut j);
        let min = read_num(&mut j);
        skip_sp(&mut j);
        let (max, comma) = if j < p.len() && p[j] == b',' {
            j += 1;
            skip_sp(&mut j);
            let m = read_num(&mut j);
            skip_sp(&mut j);
            (m, true)
        } else {
            (min, false)
        };
        if j >= p.len() || p[j] != b'}' {
            return Ok(None);
        }
        if min.is_none() && max.is_none() {
            return Ok(None);
        }
        if !comma && min.is_none() {
            return Ok(None);
        }
        let close = j;
        let min_v = min.unwrap_or(0);
        if min_v > MAX_QUANT as u64 || max.is_some_and(|m| m > MAX_QUANT as u64) {
            return self.err(E::QuantifierTooBig, close);
        }
        if let Some(m) = max
            && m < min_v
        {
            return self.err(E::QuantifierOutOfOrder, close);
        }
        self.i = close + 1;
        Ok(Some((min_v as u32, max.map(|m| m as u32))))
    }

    fn apply_quantifier(
        &mut self,
        br: &mut Branch,
        min: u32,
        max: Option<u32>,
        start: usize,
    ) -> PResult<()> {
        let qend = self.i - 1;
        let _ = start;
        if !br.repeatable || br.items.is_empty() {
            return self.err(E::QuantifierInvalid, qend);
        }
        // A + or ? after the quantifier makes it possessive or lazy;
        // ignorable items in between are skipped (observed on PHP 8.5:
        // /a+ ?/x and /a+(?#c)?/ are lazy).
        self.skip_ignorable();
        let mut greed = Greed::Greedy;
        match self.peek() {
            Some(b'+') => {
                self.i += 1;
                greed = Greed::Possessive;
            }
            Some(b'?') => {
                self.i += 1;
                greed = Greed::Lazy;
            }
            _ => {}
        }
        if self.o.ungreedy {
            greed = match greed {
                Greed::Greedy => Greed::Lazy,
                Greed::Lazy => Greed::Greedy,
                g => g,
            };
        }
        let node = br.items.pop().unwrap();
        br.repeatable = false;
        // {0} drops non-group items (pcre2pattern); keep groups so they stay
        // callable as subroutines.
        let mut max = max;
        let mut min = min;
        if let Node::Group {
            kind: GroupKind::LookAhead { .. } | GroupKind::LookBehind { .. },
            ..
        } = &node
        {
            // pcre2pattern: an unlimited maximum on an assertion becomes one
            // more than the minimum.
            if max.is_none() {
                max = Some(min + 1);
            }
            if min > 1 {
                min = 1;
            }
            if let Some(m) = max {
                max = Some(m.min(1));
            }
        }
        if min == 1 && max == Some(1) {
            br.items.push(node);
            return Ok(());
        }
        br.items.push(Node::Repeat {
            node: Box::new(node),
            min,
            max,
            greed,
        });
        Ok(())
    }

    /// Skips items that do not separate a quantifier from its + or ?
    /// suffix: white space and #-comments in extended mode, (?#...)
    /// comments and empty \Q\E pairs.
    fn skip_ignorable(&mut self) {
        loop {
            let before = self.i;
            self.skip_extended();
            if self.starts_with(b"(?#")
                && let Some(k) = self.p[self.i..].iter().position(|&b| b == b')')
            {
                self.i += k + 1;
            }
            while self.starts_with(b"\\Q\\E") {
                self.i += 4;
            }
            if self.starts_with(b"\\E") {
                self.i += 2;
            }
            if self.i == before {
                break;
            }
        }
    }

    fn push_item(&self, br: &mut Branch, n: Node, repeatable: bool) {
        br.items.push(n);
        br.repeatable = repeatable;
    }

    fn parse_atom(&mut self, br: &mut Branch) -> PResult<()> {
        let b = self.p[self.i];
        match b {
            b'^' => {
                self.i += 1;
                self.push_item(
                    br,
                    Node::Assert(Assertion::Circumflex {
                        multiline: self.o.multiline,
                    }),
                    false,
                );
            }
            b'$' => {
                self.i += 1;
                self.push_item(
                    br,
                    Node::Assert(Assertion::Dollar {
                        multiline: self.o.multiline,
                        endonly: self.dollar_endonly,
                    }),
                    false,
                );
            }
            b'.' => {
                self.i += 1;
                self.push_item(
                    br,
                    Node::Dot {
                        dotall: self.o.dotall,
                    },
                    true,
                );
            }
            b'[' => {
                let n = self.parse_class()?;
                self.push_item(br, n, true);
            }
            b'(' => self.parse_paren(br)?,
            b'\\' => self.parse_escape_atom(br)?,
            _ => {
                let c = self.read_char();
                let n = self.char_node(c);
                self.push_item(br, n, true);
            }
        }
        Ok(())
    }

    fn parse_escape_atom(&mut self, br: &mut Branch) -> PResult<()> {
        let start = self.i;
        let esc = self.escape(false)?;
        match esc {
            Esc::Lit(c) => {
                let n = self.char_node(c);
                self.push_item(br, n, true);
            }
            Esc::Type(t, neg) => {
                let mut b = ClassBuilder::new();
                self.add_type(&mut b, t, neg);
                self.push_item(br, Node::Set(Arc::new(b.build(false))), true);
            }
            Esc::Prop(p) => {
                let mut b = ClassBuilder::new();
                self.add_prop(&mut b, p);
                self.push_item(br, Node::Set(Arc::new(b.build(false))), true);
            }
            Esc::NotNewline => self.push_item(br, Node::NotNewline, true),
            Esc::Newline => self.push_item(
                br,
                Node::Newline {
                    anycrlf: self.bsr_anycrlf,
                },
                true,
            ),
            Esc::Grapheme => self.push_item(br, Node::Grapheme, true),
            Esc::CodeUnit => self.push_item(br, Node::CodeUnit, true),
            Esc::Assert(a) => self.push_item(br, Node::Assert(a), false),
            Esc::Keep => {
                if self.lookaround_depth > 0 {
                    self.keep_in_lookaround = true;
                }
                self.push_item(br, Node::Keep, false)
            }
            Esc::Backref(r) => {
                let caseless = self.o.caseless;
                let restrict = self.o.caseless_restrict || !(self.utf || self.ucp);
                self.push_item(
                    br,
                    Node::Backref {
                        target: r,
                        caseless,
                        case_restrict: restrict,
                    },
                    true,
                );
            }
            Esc::Call(r) => self.push_item(br, Node::Call { target: r }, true),
            Esc::QuoteStart => {
                // Literal text up to \E or the end of the pattern.
                let _ = start;
                loop {
                    if self.at_end() {
                        break;
                    }
                    if self.starts_with(b"\\E") {
                        self.i += 2;
                        break;
                    }
                    let c = self.read_char();
                    let n = self.char_node(c);
                    self.push_item(br, n, true);
                }
            }
            Esc::QuoteEnd => {}
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Escapes

    /// Parses an escape starting at the backslash.
    fn escape(&mut self, in_class: bool) -> PResult<Esc> {
        let bs = self.i;
        self.i += 1;
        let Some(c) = self.peek() else {
            return self.err(E::EndBackslash, self.p.len());
        };
        if c >= 0x80 || !c.is_ascii_alphanumeric() {
            // Any non-alphanumeric character (including those above 127)
            // stands for itself.
            let ch = self.read_char();
            return Ok(Esc::Lit(ch));
        }
        self.i += 1;
        let after = self.i;
        let lit = |v: u32| Ok(Esc::Lit(v));
        match c {
            b'a' => lit(7),
            b'e' => lit(0x1b),
            b'f' => lit(0x0c),
            b'n' => lit(0x0a),
            b'r' => lit(0x0d),
            b't' => lit(0x09),
            b'b' if in_class => lit(0x08),
            b'0' => {
                // \0 followed by up to two more octal digits.
                let mut v = 0u32;
                let mut k = 0;
                while k < 2 && matches!(self.peek(), Some(b'0'..=b'7')) {
                    v = v * 8 + (self.p[self.i] - b'0') as u32;
                    self.i += 1;
                    k += 1;
                }
                lit(v)
            }
            b'1'..=b'9' => {
                if in_class {
                    if c >= b'8' {
                        return lit(c as u32);
                    }
                    self.i = after - 1;
                    return self.octal3(bs);
                }
                // pcre2pattern "Perl rules for non-class backslash 1-9".
                let mut j = after - 1;
                let mut n: u64 = 0;
                while j < self.p.len() && self.p[j].is_ascii_digit() {
                    n = (n * 10 + (self.p[j] - b'0') as u64).min(1 << 40);
                    j += 1;
                }
                if n < 10 || c >= b'8' || n <= self.ncap as u64 {
                    self.i = j;
                    if n > MAX_QUANT as u64 {
                        return self.err(E::SubpatternNumberTooBig, j);
                    }
                    // A reference to a missing group is reported at the
                    // last digit (observed on PHP 8.5: \9 at 1, \93 at 2).
                    return Ok(Esc::Backref(GroupRef::num(n as u32, j - 1)));
                }
                self.i = after - 1;
                self.octal3(bs)
            }
            b'o' => {
                if self.peek() != Some(b'{') {
                    let off = if self.at_end() { after - 1 } else { after };
                    return self.err(E::BackslashONoBrace, off);
                }
                self.i += 1;
                self.braced_number(8, bs)
            }
            b'x' => {
                if self.peek() == Some(b'{') {
                    self.i += 1;
                    return self.braced_number(16, bs);
                }
                let mut v = 0u32;
                let mut k = 0;
                while k < 2 {
                    match self.peek() {
                        Some(h) if h.is_ascii_hexdigit() => {
                            v = v * 16 + (h as char).to_digit(16).unwrap();
                            self.i += 1;
                            k += 1;
                        }
                        _ => break,
                    }
                }
                if k == 0 {
                    return self.err(E::MissingOctalOrHexDigits, self.i);
                }
                lit(v)
            }
            b'c' => {
                let Some(x) = self.peek() else {
                    return self.err(E::EndBackslashC, self.p.len());
                };
                if !(32..=126).contains(&x) {
                    return self.err(E::BackslashCSyntax, self.i);
                }
                self.i += 1;
                lit((x.to_ascii_uppercase() ^ 0x40) as u32)
            }
            b'N' if self.peek() == Some(b'{')
                && (in_class || !self.brace_quantifier_at(self.i)) =>
            {
                // \N{U+hhh} (UTF only); \N{name} is not supported. \N
                // followed by a quantifier is \N repeated.
                if !self.p[self.i..].starts_with(b"{U+") {
                    return self.err(E::UnsupportedEscapeSequence, self.i);
                }
                if !self.utf {
                    return self.err(E::BackslashNNoUtf, self.i);
                }
                self.i += 3;
                self.braced_number(16, bs)
            }
            b'N' => {
                if in_class {
                    return self.err(E::BackslashNInClass, self.i);
                }
                Ok(Esc::NotNewline)
            }
            b'd' | b'D' => Ok(Esc::Type(CharType::Digit, c == b'D')),
            b's' | b'S' => Ok(Esc::Type(CharType::Space, c == b'S')),
            b'w' | b'W' => Ok(Esc::Type(CharType::Word, c == b'W')),
            b'h' | b'H' => Ok(Esc::Type(CharType::HSpace, c == b'H')),
            b'v' | b'V' => Ok(Esc::Type(CharType::VSpace, c == b'V')),
            b'p' | b'P' => {
                let spec = self.property(c == b'P', bs)?;
                Ok(Esc::Prop(spec))
            }
            b'Q' => Ok(Esc::QuoteStart),
            b'E' => Ok(Esc::QuoteEnd),
            b'F' | b'L' | b'l' | b'U' | b'u' => self.err(E::UnsupportedEscapeSequence, after),
            b'B' | b'R' | b'X' if in_class => self.err(E::EscapeInvalidInClass, after - 1),
            // \g and \k are not special in a class (observed on PHP 8.5:
            // /[\g]/ matches "g").
            b'g' | b'k' if in_class => lit(c as u32),
            _ if in_class && matches!(c, b'A' | b'z' | b'Z' | b'G' | b'K' | b'C') => {
                self.err(E::EscapeInvalidInClass, after - 1)
            }
            b'R' => Ok(Esc::Newline),
            b'X' => Ok(Esc::Grapheme),
            b'C' => Ok(Esc::CodeUnit),
            b'b' => Ok(Esc::Assert(Assertion::WordBoundary {
                negated: false,
                ucp: self.word_ucp(),
            })),
            b'B' => Ok(Esc::Assert(Assertion::WordBoundary {
                negated: true,
                ucp: self.word_ucp(),
            })),
            b'A' => Ok(Esc::Assert(Assertion::StartSubject)),
            b'z' => Ok(Esc::Assert(Assertion::EndSubject)),
            b'Z' => Ok(Esc::Assert(Assertion::EndSubjectOrNewline)),
            b'G' => Ok(Esc::Assert(Assertion::StartOfMatch)),
            b'K' => Ok(Esc::Keep),
            b'g' => self.escape_g(bs),
            b'k' => self.escape_k(bs),
            _ => self.err(E::UnknownEscape, after - 1),
        }
    }

    /// Whether a `{` at `at` starts a quantifier.
    fn brace_quantifier_at(&mut self, at: usize) -> bool {
        let save = self.i;
        self.i = at;
        let r = self.try_brace_quantifier();
        self.i = save;
        !matches!(r, Ok(None))
    }

    fn word_ucp(&self) -> bool {
        self.ucp && !self.o.ascii_bsw
    }

    /// Up to three octal digits at the current position (the digits are
    /// known to start with an octal digit).
    fn octal3(&mut self, _bs: usize) -> PResult<Esc> {
        let mut v = 0u32;
        let mut k = 0;
        while k < 3 && matches!(self.peek(), Some(b'0'..=b'7')) {
            v = v * 8 + (self.p[self.i] - b'0') as u32;
            self.i += 1;
            k += 1;
        }
        if !self.utf && v > 0xff {
            return self.err(E::OctalByteTooBig, self.i);
        }
        Ok(Esc::Lit(v))
    }

    /// Digits of \o{...}, \x{...} or \N{U+...} after the opening brace.
    fn braced_number(&mut self, radix: u32, _bs: usize) -> PResult<Esc> {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.i += 1;
        }
        let mut v: u64 = 0;
        let mut k = 0;
        let limit: u64 = if self.utf { 0x10ffff } else { 0xff };
        while let Some(d) = self.peek() {
            let Some(x) = (d as char).to_digit(radix) else {
                break;
            };
            v = (v * radix as u64 + x as u64).min(1 << 40);
            self.i += 1;
            k += 1;
            // Too large is noticed as soon as the digits exceed the limit
            // (observed on PHP 8.5: [a-\x{ca0-9] fails after the "0").
            if v > limit {
                return self.err(E::CodePointTooBig, self.i);
            }
        }
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.i += 1;
        }
        match self.peek() {
            Some(b'}') if k > 0 => {}
            Some(b'}') | None if k == 0 => {
                return self.err(E::MissingOctalOrHexDigits, self.i);
            }
            None => {
                let kind = if radix == 8 {
                    E::InvalidOctal
                } else {
                    E::InvalidHexadecimal
                };
                return self.err(kind, self.p.len() - 1);
            }
            Some(_) => {
                let kind = if radix == 8 {
                    E::InvalidOctal
                } else {
                    E::InvalidHexadecimal
                };
                return self.err(kind, self.i);
            }
        }
        let close = self.i;
        self.i += 1;
        let limit: u64 = if self.utf { 0x10ffff } else { 0xff };
        if v > limit {
            return self.err(E::CodePointTooBig, close);
        }
        if self.utf && (0xd800..=0xdfff).contains(&v) {
            return self.err(E::UnicodeDisallowedCodePoint, close);
        }
        Ok(Esc::Lit(v as u32))
    }

    /// `\g` forms (after `\g`).
    fn escape_g(&mut self, bs: usize) -> PResult<Esc> {
        let after_g = self.i;
        match self.peek() {
            Some(b'<') | Some(b'\'') => {
                let close = if self.peek() == Some(b'<') {
                    b'>'
                } else {
                    b'\''
                };
                let open = self.i;
                self.i += 1;
                // Number (absolute or relative) or name.
                let r = self.ref_number_or_name(close, open, true)?;
                Ok(Esc::Call(r))
            }
            Some(b'{') => {
                let open = self.i;
                self.i += 1;
                while matches!(self.peek(), Some(b' ' | b'\t')) {
                    self.i += 1;
                }
                let numeric = match self.peek() {
                    Some(b'0'..=b'9') => true,
                    Some(b'+' | b'-') => self.peek_at(1).is_some_and(|d| d.is_ascii_digit()),
                    _ => false,
                };
                if numeric {
                    // The whole {[+-]digits} must be well formed first
                    // (observed on PHP 8.5: \g{1 and \g{0,2} are \g syntax
                    // errors at the \g).
                    let mut j = self.i;
                    let sign = matches!(self.p[j], b'+' | b'-');
                    if sign {
                        j += 1;
                    }
                    let ds = j;
                    let mut zero = true;
                    while j < self.p.len() && self.p[j].is_ascii_digit() {
                        zero &= self.p[j] == b'0';
                        j += 1;
                    }
                    let de = j;
                    while j < self.p.len() && matches!(self.p[j], b' ' | b'\t') {
                        j += 1;
                    }
                    if de == ds || j >= self.p.len() || self.p[j] != b'}' {
                        return self.err(E::BackslashGSyntax, after_g);
                    }
                    if sign && zero {
                        return self.err(E::ZeroRelativeReference, after_g);
                    }
                    let r = self.signed_ref(open, b'}')?;
                    while matches!(self.peek(), Some(b' ' | b'\t')) {
                        self.i += 1;
                    }
                    if self.peek() != Some(b'}') {
                        return self.err(E::MissingNameTerminator, self.i);
                    }
                    self.i += 1;
                    return Ok(Esc::Backref(r));
                }
                let name_start = self.i;
                let name = self.read_name(name_start)?;
                while matches!(self.peek(), Some(b' ' | b'\t')) {
                    self.i += 1;
                }
                if self.peek() != Some(b'}') {
                    return self.err(E::MissingNameTerminator, self.i);
                }
                self.i += 1;
                Ok(Esc::Backref(GroupRef::name(&name, name_start)))
            }
            Some(b'+' | b'-' | b'0'..=b'9') => {
                let r = self.signed_ref(after_g, 0)?;
                Ok(Esc::Backref(r))
            }
            _ => {
                let _ = bs;
                self.err(E::BackslashGSyntax, after_g)
            }
        }
    }

    /// A signed or unsigned group number for \g; `open` is where relative
    /// underflow errors are reported.
    fn signed_ref(&mut self, open: usize, close: u8) -> PResult<GroupRef> {
        let sign = match self.peek() {
            Some(b'+') => {
                self.i += 1;
                1
            }
            Some(b'-') => {
                self.i += 1;
                -1
            }
            _ => 0,
        };
        let ds = self.i;
        let mut n: u64 = 0;
        while let Some(d) = self.peek().filter(u8::is_ascii_digit) {
            n = (n * 10 + (d - b'0') as u64).min(1 << 40);
            self.i += 1;
        }
        if self.i == ds {
            if close == 0 {
                return self.err(E::BackslashGSyntax, open);
            }
            return self.err(E::SubpatternNameExpected, self.i);
        }
        let end_off = if close == b'}' && self.peek() == Some(b'}') {
            self.i + 1
        } else {
            self.i
        };
        match sign {
            0 => {
                if n == 0 {
                    return self.err(E::BadSubpatternReference, end_off);
                }
                if n > MAX_QUANT as u64 {
                    return self.err(E::SubpatternNumberTooBig, self.i);
                }
                Ok(GroupRef::num(
                    n as u32,
                    if close == b'}' { self.i } else { end_off - 1 },
                ))
            }
            -1 => {
                if n == 0 {
                    return self.err(E::ZeroRelativeReference, self.i);
                }
                if n > self.ncap as u64 {
                    return self.err(E::BadSubpatternReference, open);
                }
                Ok(GroupRef::num(self.ncap + 1 - n as u32, open))
            }
            _ => {
                if n == 0 {
                    return self.err(E::ZeroRelativeReference, self.i);
                }
                let v = self.ncap as u64 + n;
                if v > MAX_QUANT as u64 {
                    return self.err(E::SubpatternNumberTooBig, self.i);
                }
                Ok(GroupRef::num(
                    v as u32,
                    if close == b'}' { self.i } else { end_off },
                ))
            }
        }
    }

    /// For `\g<...>` / `\g'...'`: a (signed) number or a name, then `close`.
    fn ref_number_or_name(&mut self, close: u8, open: usize, _call: bool) -> PResult<GroupRef> {
        if matches!(self.peek(), Some(b'+' | b'-' | b'0'..=b'9')) {
            let sign = self.peek();
            let mut r = self.signed_ref(open, close)?;
            if self.peek() != Some(close) {
                return self.err(E::MissingNameTerminator, self.i);
            }
            if !matches!(sign, Some(b'-')) {
                r.offset = self.i;
            }
            self.i += 1;
            return Ok(r);
        }
        let name_start = self.i;
        let name = self.read_name(name_start)?;
        if self.peek() != Some(close) {
            return self.err(E::MissingNameTerminator, self.i);
        }
        self.i += 1;
        Ok(GroupRef::name(&name, name_start))
    }

    /// `\k<name>`, `\k'name'`, `\k{name}`.
    fn escape_k(&mut self, _bs: usize) -> PResult<Esc> {
        let close = match self.peek() {
            Some(b'<') => b'>',
            Some(b'\'') => b'\'',
            Some(b'{') => b'}',
            _ => return self.err(E::BackslashKSyntax, self.i),
        };
        self.i += 1;
        if close == b'}' {
            while matches!(self.peek(), Some(b' ' | b'\t')) {
                self.i += 1;
            }
        }
        let name_start = self.i;
        let name = self.read_name(name_start)?;
        if close == b'}' {
            while matches!(self.peek(), Some(b' ' | b'\t')) {
                self.i += 1;
            }
        }
        if self.peek() != Some(close) {
            return self.err(E::MissingNameTerminator, self.i);
        }
        self.i += 1;
        Ok(Esc::Backref(GroupRef::name(&name, name_start)))
    }

    /// Reads a group name at the current position (pcre2pattern: ASCII
    /// letters, digits and underscore; in UTF mode Unicode letters and
    /// decimal digits too; not starting with a digit; at most 128 code
    /// units).
    fn read_name(&mut self, start: usize) -> PResult<Vec<u8>> {
        let s = self.i;
        if let Some(d) = self.peek()
            && d.is_ascii_digit()
        {
            return self.err(E::InvalidSubpatternName, self.i);
        }
        while let Some(b) = self.peek() {
            if b < 0x80 {
                if is_word_byte(b) {
                    self.i += 1;
                    continue;
                }
                break;
            }
            if self.utf {
                let (c, l) = utf8::decode(self.p, self.i);
                let g = unicode::gc(c);
                if class::gc_in(c, class::GC_MASK_L) || g == unicode::GC_ND {
                    self.i += l;
                    continue;
                }
            }
            break;
        }
        let _ = start;
        if self.i == s {
            return self.err(E::SubpatternNameExpected, self.i);
        }
        if self.i - s > MAX_NAME_LEN {
            return self.err(E::SubpatternNameTooLong, self.i);
        }
        Ok(self.p[s..self.i].to_vec())
    }

    // -----------------------------------------------------------------------
    // Properties

    /// Parses `\p`/`\P` after the letter.
    fn property(&mut self, mut neg: bool, _bs: usize) -> PResult<PropSpec> {
        let name: Vec<u8>;
        match self.peek() {
            None => return self.err(E::MalformedUnicodeProperty, self.i),
            Some(b'{') => {
                self.i += 1;
                let s = self.i;
                while let Some(b) = self.peek() {
                    if b == b'}' {
                        break;
                    }
                    self.i += 1;
                    // Characters that cannot be part of a property name
                    // (observed on PHP 8.5).
                    if b >= 0x80
                        || matches!(b, b'!' | b'"' | b'#' | b'$' | b'%' | b'{' | b'|' | b'~')
                    {
                        return self.err(E::MalformedUnicodeProperty, self.i);
                    }
                }
                if self.at_end() {
                    return self.err(E::MalformedUnicodeProperty, self.i);
                }
                name = self.p[s..self.i].to_vec();
                self.i += 1;
            }
            Some(b) => {
                self.i += 1;
                if !b.is_ascii_alphabetic() {
                    return self.err(E::MalformedUnicodeProperty, self.i);
                }
                name = vec![b];
            }
        }
        // Loose matching: ignore ASCII white space, hyphens and underscores,
        // compare case-insensitively.
        let mut norm: Vec<u8> = Vec::with_capacity(name.len());
        let mut caret_ok = true;
        for &b in &name {
            if matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | b'-' | b'_') {
                continue;
            }
            if b == b'^' && caret_ok && norm.is_empty() {
                neg = !neg;
                caret_ok = false;
                continue;
            }
            caret_ok = false;
            norm.push(b.to_ascii_lowercase());
        }
        match lookup_property(&norm) {
            Some(kind) => Ok(PropSpec { kind, neg }),
            None => self.err(E::UnknownUnicodeProperty, self.i),
        }
    }

    // -----------------------------------------------------------------------
    // Building sets from types and properties

    fn add_type(&self, b: &mut ClassBuilder, t: CharType, neg: bool) {
        let ucp = self.ucp;
        match t {
            CharType::Digit => {
                if ucp && !self.o.ascii_bsd {
                    let nd = class::gc_bit(unicode::GC_ND);
                    b.add_pred(
                        |c| (unicode::gc(c) == unicode::GC_ND) != neg,
                        Some(HighItem::Gc { mask: nd, neg }),
                    );
                } else {
                    b.add_pred(
                        |c| (c < 128 && (c as u8).is_ascii_digit()) != neg,
                        neg.then_some(HighItem::All),
                    );
                }
            }
            CharType::Space => {
                if ucp && !self.o.ascii_bss {
                    b.add_pred(
                        |c| class::is_ucp_space(c) != neg,
                        Some(HighItem::Pred {
                            f: class::is_ucp_space,
                            neg,
                        }),
                    );
                } else {
                    b.add_pred(
                        |c| class::is_ascii_space(c) != neg,
                        neg.then_some(HighItem::All),
                    );
                }
            }
            CharType::Word => {
                if ucp && !self.o.ascii_bsw {
                    b.add_pred(
                        |c| class::is_ucp_word(c) != neg,
                        Some(HighItem::Gc {
                            mask: class::GC_MASK_WORD,
                            neg,
                        }),
                    );
                } else {
                    b.add_pred(
                        |c| class::is_ascii_word(c) != neg,
                        neg.then_some(HighItem::All),
                    );
                }
            }
            CharType::HSpace => {
                if neg {
                    b.add_pred(
                        |c| !class::is_hspace(c),
                        Some(HighItem::NotRanges(class::HSPACE_HIGH.to_vec())),
                    );
                } else {
                    b.add_pred(class::is_hspace, None);
                    for &(lo, hi) in class::HSPACE_HIGH {
                        b.add_range(lo, hi);
                    }
                }
            }
            CharType::VSpace => {
                if neg {
                    b.add_pred(
                        |c| !class::is_vspace(c),
                        Some(HighItem::NotRanges(class::VSPACE_HIGH.to_vec())),
                    );
                } else {
                    b.add_pred(class::is_vspace, None);
                    for &(lo, hi) in class::VSPACE_HIGH {
                        b.add_range(lo, hi);
                    }
                }
            }
        }
    }

    fn add_prop(&self, b: &mut ClassBuilder, p: PropSpec) {
        let neg = p.neg;
        let mut kind = p.kind;
        if self.o.caseless
            && let PropKind::Gc(m) = kind
        {
            // pcre2pattern (10.45): Lu, Ll and Lt are treated as Lc when
            // matching caselessly.
            let one = |g: u8| class::gc_bit(g);
            if m == one(unicode::GC_LU) || m == one(unicode::GC_LL) || m == one(unicode::GC_LT) {
                kind = PropKind::Gc(class::GC_MASK_LC);
            }
        }
        match kind {
            PropKind::Any => b.add_pred(|_| !neg, (!neg).then_some(HighItem::All)),
            PropKind::Gc(mask) => b.add_pred(
                |c| class::gc_in(c, mask) != neg,
                Some(HighItem::Gc { mask, neg }),
            ),
            PropKind::Script(idx) => b.add_pred(
                |c| (unicode::script(c) == idx) != neg,
                Some(HighItem::Script { idx, neg }),
            ),
            PropKind::Scx(idx) => b.add_pred(
                |c| unicode::scx_contains(c, idx) != neg,
                Some(HighItem::Scx { idx, neg }),
            ),
            PropKind::Bidi(idx) => b.add_pred(
                |c| (unicode::bidi(c) == idx) != neg,
                Some(HighItem::Bidi { idx, neg }),
            ),
            PropKind::Bprop(idx) => b.add_pred(
                |c| unicode::has_bprop(c, idx) != neg,
                Some(HighItem::Bprop { idx, neg }),
            ),
            PropKind::Ascii => b.add_pred(|c| (c < 128) != neg, neg.then_some(HighItem::All)),
            PropKind::Xan => b.add_pred(
                |c| class::gc_in(c, class::GC_MASK_ALNUM) != neg,
                Some(HighItem::Gc {
                    mask: class::GC_MASK_ALNUM,
                    neg,
                }),
            ),
            PropKind::Xwd => b.add_pred(
                |c| class::gc_in(c, class::GC_MASK_WORD) != neg,
                Some(HighItem::Gc {
                    mask: class::GC_MASK_WORD,
                    neg,
                }),
            ),
            PropKind::Xps => b.add_pred(
                |c| class::is_posix_space_ucp(c) != neg,
                Some(HighItem::Pred {
                    f: class::is_posix_space_ucp,
                    neg,
                }),
            ),
            PropKind::Xuc => b.add_pred(
                |c| class::is_xuc(c) != neg,
                Some(HighItem::Pred {
                    f: class::is_xuc,
                    neg,
                }),
            ),
        }
    }

    /// Adds a POSIX class (`[:name:]`) to `b`.
    fn add_posix(&self, b: &mut ClassBuilder, name: &str, neg: bool) {
        let ucp_all = self.ucp && !self.o.ascii_posix;
        let ucp_digit = ucp_all && !self.o.ascii_digit;
        if ucp_all {
            let prop = |kind| PropSpec { kind, neg };
            match name {
                "alnum" => return self.add_prop_nocase(b, prop(PropKind::Xan)),
                "alpha" => return self.add_prop_nocase(b, prop(PropKind::Gc(class::GC_MASK_L))),
                "blank" => return self.add_type(b, CharType::HSpace, neg),
                "cntrl" => {
                    return self
                        .add_prop_nocase(b, prop(PropKind::Gc(class::gc_bit(unicode::GC_CC))));
                }
                "lower" => {
                    return self
                        .add_prop_nocase(b, prop(PropKind::Gc(class::gc_bit(unicode::GC_LL))));
                }
                "space" => return self.add_prop_nocase(b, prop(PropKind::Xps)),
                "upper" => {
                    return self
                        .add_prop_nocase(b, prop(PropKind::Gc(class::gc_bit(unicode::GC_LU))));
                }
                "word" => return self.add_prop_nocase(b, prop(PropKind::Xwd)),
                "graph" => {
                    return b.add_pred(
                        |c| class::ucp_graph(c) != neg,
                        Some(HighItem::Pred {
                            f: class::ucp_graph,
                            neg,
                        }),
                    );
                }
                "print" => {
                    return b.add_pred(
                        |c| class::ucp_print(c) != neg,
                        Some(HighItem::Pred {
                            f: class::ucp_print,
                            neg,
                        }),
                    );
                }
                "punct" => {
                    return b.add_pred(
                        |c| class::ucp_punct(c) != neg,
                        Some(HighItem::Pred {
                            f: class::ucp_punct,
                            neg,
                        }),
                    );
                }
                _ => {}
            }
        }
        if ucp_digit {
            match name {
                "digit" => {
                    return self.add_prop_nocase(
                        b,
                        prop_of(PropKind::Gc(class::gc_bit(unicode::GC_ND)), neg),
                    );
                }
                "xdigit" => {
                    return b.add_pred(
                        |c| class::ucp_xdigit(c) != neg,
                        Some(HighItem::Pred {
                            f: class::ucp_xdigit,
                            neg,
                        }),
                    );
                }
                _ => {}
            }
        }
        // C locale definitions. Caseless [:upper:] and [:lower:] match all
        // letters (observed on PHP 8.5: /[[:upper:]]/i matches "a").
        let eff = if self.o.caseless && (name == "upper" || name == "lower") {
            "alpha"
        } else {
            name
        };
        let eff = eff.to_string();
        b.add_pred(
            move |c| class::posix_ascii(&eff, c) != neg,
            neg.then_some(HighItem::All),
        );
    }

    fn add_prop_nocase(&self, b: &mut ClassBuilder, p: PropSpec) {
        let mut me = self.o;
        me.caseless = false;
        let saved = self.o;
        let _ = saved;
        // add_prop only consults `caseless` for the Lc conversion.
        let tmp = Parser {
            o: me,
            ..self.shallow()
        };
        tmp.add_prop(b, p);
    }

    /// A copy of the parser state sufficient for the set-building helpers.
    fn shallow(&self) -> Parser<'a> {
        Parser {
            p: self.p,
            i: self.i,
            utf: self.utf,
            ucp: self.ucp,
            o: self.o,
            dollar_endonly: self.dollar_endonly,
            newline: self.newline,
            bsr_anycrlf: self.bsr_anycrlf,
            turkish: self.turkish,
            allow_empty_class: self.allow_empty_class,
            ncap: 0,
            names: Vec::new(),
            depth: 0,
            lookaround_depth: 0,
            keep_in_lookaround: false,
            has_branch_reset: false,
            has_dup_names: false,
            group_names: Vec::new(),
            has_cr_lf: false,
            has_ext_class: false,
        }
    }

    // -----------------------------------------------------------------------
    // Character classes

    /// Checks for POSIX syntax `[:name:]` (or `[.x.]`, `[=x=]`) at `at`
    /// (pointing at the `[`). Returns the index just past the closing `]`.
    fn posix_syntax_at(&self, at: usize) -> Option<usize> {
        let p = self.p;
        let term = *p.get(at + 1)?;
        if !matches!(term, b':' | b'.' | b'=') {
            return None;
        }
        let mut j = at + 2;
        while j < p.len() {
            let b = p[j];
            if b == b'\\' && matches!(p.get(j + 1), Some(b']') | Some(b'\\')) {
                j += 2;
                continue;
            }
            if b == b'[' && matches!(p.get(j + 1), Some(b':' | b'.' | b'=')) {
                return None;
            }
            if b == b']' {
                return None;
            }
            if b == term && p.get(j + 1) == Some(&b']') {
                return Some(j + 2);
            }
            j += 1;
        }
        None
    }

    fn parse_class(&mut self) -> PResult<Node> {
        let start = self.i;
        // [[:<:]] and [[:>:]] (pcre2pattern "Compatibility feature for word
        // boundaries").
        if self.p[start..].starts_with(b"[[:<:]]") || self.p[start..].starts_with(b"[[:>:]]") {
            let lookahead = self.p[start + 3] == b'<';
            self.i += 7;
            let mut wb = ClassBuilder::new();
            self.add_type(&mut wb, CharType::Word, false);
            let word = Node::Set(Arc::new(wb.build(false)));
            let kind = if lookahead {
                GroupKind::LookAhead {
                    negated: false,
                    atomic: true,
                }
            } else {
                GroupKind::LookBehind {
                    negated: false,
                    atomic: true,
                }
            };
            return Ok(Node::Seq(vec![
                Node::Assert(Assertion::WordBoundary {
                    negated: false,
                    ucp: self.word_ucp(),
                }),
                Node::Group {
                    kind,
                    body: Box::new(word),
                    offset: start,
                },
            ]));
        }
        if self.posix_syntax_at(start).is_some() {
            return self.err(E::PosixClassNotInClass, start);
        }
        self.i += 1;
        let mut negated = false;
        if self.peek() == Some(b'^') {
            negated = true;
            self.i += 1;
        }
        let mut b = ClassBuilder::new();
        let mode = self.case_mode();
        let mut first = true;
        // The previous item, when it was a single character that may start a
        // range.
        let mut prev_char: Option<u32> = None;
        // Whether the previous item was a type or POSIX class (a following
        // hyphen must then be the last character).
        let mut prev_type = false;
        let mut in_quote = false;
        loop {
            if self.at_end() {
                return self.err(E::MissingSquareBracket, self.p.len());
            }
            let c = self.p[self.i];
            if in_quote {
                if self.starts_with(b"\\E") {
                    self.i += 2;
                    in_quote = false;
                    continue;
                }
                let ch = self.read_char();
                self.class_char(&mut b, ch, mode, &mut prev_char, &mut prev_type, &mut first)?;
                continue;
            }
            if c == b']' && (!first || self.allow_empty_class) {
                self.i += 1;
                break;
            }
            if self.o.extended_more && (c == b' ' || c == b'\t') {
                self.i += 1;
                continue;
            }
            first = false;
            if c == b'[' {
                if let Some(end) = self.posix_syntax_at(self.i) {
                    let term = self.p[self.i + 1];
                    if term != b':' {
                        return self.err(E::PosixNoSupportCollating, end);
                    }
                    let mut ns = self.i + 2;
                    let mut pneg = false;
                    if self.p[ns] == b'^' {
                        pneg = true;
                        ns += 1;
                    }
                    let name = &self.p[ns..end - 2];
                    let name = std::str::from_utf8(name).unwrap_or("");
                    if !class::POSIX_NAMES.contains(&name) {
                        return self.err(E::UnknownPosixClass, end);
                    }
                    if prev_char.is_some() && self.p[self.i - 1] == b'-' {
                        // unreachable: ranges are handled below
                    }
                    self.i = end;
                    let name = name.to_string();
                    self.add_posix(&mut b, &name, pneg);
                    self.after_type_item(&mut prev_char, &mut prev_type)?;
                    continue;
                }
                self.i += 1;
                self.class_char(
                    &mut b,
                    b'[' as u32,
                    mode,
                    &mut prev_char,
                    &mut prev_type,
                    &mut first,
                )?;
                continue;
            }
            if c == b'\\' {
                // \Q and \E work inside classes too.
                if self.starts_with(b"\\Q") {
                    self.i += 2;
                    in_quote = true;
                    continue;
                }
                if self.starts_with(b"\\E") {
                    self.i += 2;
                    continue;
                }
                let esc = self.escape(true)?;
                match esc {
                    Esc::Lit(ch) => {
                        self.class_char(
                            &mut b,
                            ch,
                            mode,
                            &mut prev_char,
                            &mut prev_type,
                            &mut first,
                        )?;
                    }
                    Esc::Type(t, neg) => {
                        self.add_type(&mut b, t, neg);
                        self.after_type_item(&mut prev_char, &mut prev_type)?;
                    }
                    Esc::Prop(p) => {
                        self.add_prop(&mut b, p);
                        self.after_type_item(&mut prev_char, &mut prev_type)?;
                    }
                    _ => return self.err(E::EscapeInvalidInClass, self.i - 1),
                }
                continue;
            }
            let ch = self.read_char();
            self.class_char(&mut b, ch, mode, &mut prev_char, &mut prev_type, &mut first)?;
        }
        let set = b.build(negated);
        Ok(Node::Set(Arc::new(set)))
    }

    /// After a type escape or POSIX class: a hyphen that is not last is an
    /// error.
    fn after_type_item(
        &mut self,
        prev_char: &mut Option<u32>,
        prev_type: &mut bool,
    ) -> PResult<()> {
        *prev_char = None;
        *prev_type = true;
        if self.peek() == Some(b'-') && self.peek_at(1).is_some_and(|b| b != b']') {
            return self.err(E::ClassInvalidRange, self.i + 1);
        }
        Ok(())
    }

    /// Adds a literal character, handling a following range.
    fn class_char(
        &mut self,
        b: &mut ClassBuilder,
        ch: u32,
        mode: CaseMode,
        prev_char: &mut Option<u32>,
        prev_type: &mut bool,
        first: &mut bool,
    ) -> PResult<()> {
        *first = false;
        *prev_type = false;
        // A range?
        if self.peek() == Some(b'-') {
            let save = self.i;
            self.i += 1;
            // Skip ignorable space in xx mode.
            if self.o.extended_more {
                while matches!(self.peek(), Some(b' ' | b'\t')) {
                    self.i += 1;
                }
            }
            match self.peek() {
                None => {
                    return self.err(E::MissingSquareBracket, self.p.len());
                }
                Some(b']') => {
                    // Literal hyphen at the end.
                    self.i = save;
                }
                Some(nb) => {
                    let end_char: u32 = if nb == b'\\' {
                        if self.starts_with(b"\\Q") {
                            // Range end inside \Q...\E.
                            self.i += 2;
                            if self.at_end() {
                                return self.err(E::MissingSquareBracket, self.p.len());
                            }
                            if self.starts_with(b"\\E") {
                                self.i = save;
                                self.add_class_lit(b, ch, mode);
                                *prev_char = Some(ch);
                                return Ok(());
                            }
                            let c2 = self.read_char();
                            if self.starts_with(b"\\E") {
                                self.i += 2;
                            }
                            c2
                        } else if self.starts_with(b"\\E") {
                            self.i = save;
                            self.add_class_lit(b, ch, mode);
                            *prev_char = Some(ch);
                            return Ok(());
                        } else {
                            match self.escape(true)? {
                                Esc::Lit(c2) => c2,
                                Esc::Type(..) | Esc::Prop(..) => {
                                    return self.err(E::ClassInvalidRange, self.i);
                                }
                                _ => return self.err(E::EscapeInvalidInClass, self.i - 1),
                            }
                        }
                    } else if nb == b'[' && self.posix_syntax_at(self.i).is_some() {
                        let end = self.posix_syntax_at(self.i).unwrap();
                        return self.err(E::ClassInvalidRange, end);
                    } else {
                        self.read_char()
                    };
                    if end_char < ch {
                        return self.err(E::ClassRangeOrder, self.i - 1);
                    }
                    if mode == CaseMode::None {
                        b.add_range(ch, end_char);
                    } else {
                        b.add_range_caseless(ch, end_char, mode);
                        if self.turkish {
                            self.turkish_fix_range(b, ch, end_char);
                        }
                    }
                    *prev_char = None;
                    return Ok(());
                }
            }
        }
        self.add_class_lit(b, ch, mode);
        *prev_char = Some(ch);
        Ok(())
    }

    /// Perl extended classes `(?[...])` (pcre2pattern "Perl extended
    /// character classes"); the position is after `(?[`. Error kinds and
    /// offsets as observed on PHP 8.5.
    fn parse_ext_class(&mut self) -> PResult<Node> {
        use crate::class::SetExpr;
        fn prec(op: u8) -> u8 {
            if op == b'&' { 2 } else { 1 }
        }
        fn reduce(out: &mut Vec<SetExpr>, op: u8) {
            let b = Box::new(out.pop().unwrap());
            let a = Box::new(out.pop().unwrap());
            out.push(match op {
                b'&' => SetExpr::And(a, b),
                b'-' => SetExpr::Diff(a, b),
                b'^' => SetExpr::Xor(a, b),
                _ => SetExpr::Or(a, b),
            });
        }
        self.has_ext_class = true;
        let saved = self.o;
        // The /xx option is always on inside (pcre2pattern).
        self.o.extended_more = true;
        let mut out: Vec<SetExpr> = Vec::new();
        let mut ops: Vec<u8> = Vec::new();
        let mut expect_operand = true;
        let mut just_opened = true;
        let mut depth = 0u32;
        loop {
            while matches!(self.peek(), Some(b' ' | b'\t')) {
                self.i += 1;
            }
            let Some(c) = self.peek() else {
                return self.err(E::MissingSquareBracket, self.p.len());
            };
            let operand: Option<CharSet> = match c {
                b']' => {
                    if depth > 0 {
                        return self.err(E::MissingClosingParenthesis, self.i);
                    }
                    self.i += 1;
                    if expect_operand {
                        let k = if just_opened {
                            E::ExtClassEmpty
                        } else {
                            E::ExtClassExpectedOperand
                        };
                        return self.err(k, self.i);
                    }
                    if self.peek() != Some(b')') {
                        return self.err(E::ExtClassMissingParen, self.i);
                    }
                    self.i += 1;
                    break;
                }
                b'(' => {
                    self.i += 1;
                    if !expect_operand {
                        return self.err(E::ExtClassNoOperator, self.i);
                    }
                    ops.push(b'(');
                    depth += 1;
                    just_opened = true;
                    None
                }
                b')' => {
                    self.i += 1;
                    if depth == 0 {
                        return self.err(E::UnmatchedClosingParenthesis, self.i);
                    }
                    if expect_operand {
                        let k = if just_opened {
                            E::ExtClassEmpty
                        } else {
                            E::ExtClassExpectedOperand
                        };
                        return self.err(k, self.i);
                    }
                    while let Some(op) = ops.pop() {
                        if op == b'(' {
                            break;
                        }
                        reduce(&mut out, op);
                    }
                    depth -= 1;
                    // A complement applies to the parenthesised operand.
                    while ops.last() == Some(&b'!') {
                        ops.pop();
                        let a = out.pop().unwrap();
                        out.push(SetExpr::Not(Box::new(a)));
                    }
                    expect_operand = false;
                    just_opened = false;
                    None
                }
                b'&' | b'|' | b'+' | b'-' | b'^' => {
                    self.i += 1;
                    if expect_operand {
                        return self.err(E::ExtClassNoOperand, self.i);
                    }
                    while let Some(&top) = ops.last() {
                        if top == b'(' || top == b'!' || prec(top) < prec(c) {
                            break;
                        }
                        ops.pop();
                        reduce(&mut out, top);
                    }
                    ops.push(c);
                    expect_operand = true;
                    just_opened = false;
                    None
                }
                b'!' => {
                    self.i += 1;
                    if !expect_operand {
                        return self.err(E::ExtClassNoOperator, self.i);
                    }
                    ops.push(b'!');
                    just_opened = false;
                    None
                }
                b'\\' => {
                    if self.starts_with(b"\\Q") || self.starts_with(b"\\E") {
                        self.i += 2;
                        continue;
                    }
                    let mut b = ClassBuilder::new();
                    match self.escape(true)? {
                        Esc::Lit(ch) => {
                            let mode = self.case_mode();
                            self.add_class_lit(&mut b, ch, mode);
                        }
                        Esc::Type(t, neg) => self.add_type(&mut b, t, neg),
                        Esc::Prop(p) => self.add_prop(&mut b, p),
                        _ => return self.err(E::EscapeInvalidInClass, self.i - 1),
                    }
                    Some(b.build(false))
                }
                b'[' => {
                    if let Some(end) = self.posix_syntax_at(self.i)
                        && self.p[self.i + 1] == b':'
                    {
                        let mut ns = self.i + 2;
                        let mut pneg = false;
                        if self.p[ns] == b'^' {
                            pneg = true;
                            ns += 1;
                        }
                        let name = std::str::from_utf8(&self.p[ns..end - 2])
                            .unwrap_or("")
                            .to_string();
                        if !class::POSIX_NAMES.contains(&name.as_str()) {
                            return self.err(E::UnknownPosixClass, end);
                        }
                        self.i = end;
                        let mut b = ClassBuilder::new();
                        self.add_posix(&mut b, &name, pneg);
                        Some(b.build(false))
                    } else {
                        match self.parse_class()? {
                            Node::Set(s) => Some((*s).clone()),
                            _ => return self.err(E::ExtClassUnexpectedChar, self.i),
                        }
                    }
                }
                _ => {
                    self.read_char();
                    return self.err(E::ExtClassUnexpectedChar, self.i);
                }
            };
            if let Some(set) = operand {
                if !expect_operand {
                    return self.err(E::ExtClassNoOperator, self.i);
                }
                let mut e = SetExpr::Leaf(Arc::new(set));
                while ops.last() == Some(&b'!') {
                    ops.pop();
                    e = SetExpr::Not(Box::new(e));
                }
                out.push(e);
                expect_operand = false;
                just_opened = false;
            }
        }
        while let Some(op) = ops.pop() {
            reduce(&mut out, op);
        }
        self.o = saved;
        let expr = out.pop().unwrap();
        Ok(Node::Set(Arc::new(expr.into_set())))
    }

    fn add_class_lit(&mut self, b: &mut ClassBuilder, ch: u32, mode: CaseMode) {
        if ch == 10 || ch == 13 {
            self.has_cr_lf = true;
        }
        if self.turkish && mode != CaseMode::None && matches!(ch, 0x49 | 0x69 | 0x130 | 0x131) {
            match ch {
                0x69 | 0x130 => {
                    b.add_char(0x69);
                    b.add_char(0x130);
                }
                _ => {
                    b.add_char(0x49);
                    b.add_char(0x131);
                }
            }
            return;
        }
        if mode == CaseMode::None {
            b.add_char(ch);
        } else {
            b.add_char_caseless(ch, mode);
        }
    }

    fn turkish_fix_range(&mut self, b: &mut ClassBuilder, lo: u32, hi: u32) {
        for c in [0x49u32, 0x69, 0x130, 0x131] {
            if lo <= c && c <= hi {
                self.add_class_lit(b, c, CaseMode::Unicode);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Parentheses

    fn open_group_depth(&mut self) -> PResult<()> {
        self.depth += 1;
        if self.depth > PARENS_NEST_LIMIT {
            return self.err(E::ParenthesesNestTooDeep, self.i);
        }
        Ok(())
    }

    /// Parses a group body up to and including its closing parenthesis.
    /// Options changed inside are restored afterwards.
    fn group_body(&mut self, reset: Option<u32>) -> PResult<(Node, usize)> {
        let saved = self.o;
        let (node, n) = self.parse_alternation(reset)?;
        if self.peek() != Some(b')') {
            return self.err(E::MissingClosingParenthesis, self.p.len());
        }
        self.i += 1;
        self.o = saved;
        self.depth -= 1;
        Ok((node, n))
    }

    fn new_capture(&mut self) -> PResult<u32> {
        if self.ncap >= MAX_QUANT {
            return self.err(E::TooManyCaptures, self.i);
        }
        self.ncap += 1;
        Ok(self.ncap)
    }

    fn capture_group(&mut self, name: Option<(Vec<u8>, usize)>, start: usize) -> PResult<Node> {
        let n = self.new_capture()?;
        if let Some((name, after)) = name {
            self.define_name(&name, n, after)?;
        }
        let (body, _) = self.group_body(None)?;
        Ok(Node::Group {
            kind: GroupKind::Capture(n),
            body: Box::new(body),
            offset: start,
        })
    }

    fn define_name(&mut self, name: &[u8], n: u32, off: usize) -> PResult<()> {
        let rc: Arc<[u8]> = Arc::from(name);
        // A group number may carry only one name.
        if let Some((_, existing)) = self.group_names.iter().find(|(g, _)| *g == n) {
            if existing.as_ref() != name {
                return self.err(E::SubpatternNamesMismatch, off);
            }
            // Same name, same number (branch reset): fine.
            return Ok(());
        }
        if let Some(&(_, g)) = self.names.iter().find(|(nm, _)| nm.as_ref() == name)
            && g != n
        {
            if !self.o.dupnames {
                return self.err(E::DuplicateSubpatternName, off);
            }
            self.has_dup_names = true;
        }
        self.names.push((rc.clone(), n));
        self.group_names.push((n, rc));
        Ok(())
    }

    fn lookaround(&mut self, kind: GroupKind, start: usize) -> PResult<Node> {
        self.lookaround_depth += 1;
        let (body, _) = self.group_body(None)?;
        self.lookaround_depth -= 1;
        Ok(Node::Group {
            kind,
            body: Box::new(body),
            offset: start,
        })
    }

    fn parse_paren(&mut self, br: &mut Branch) -> PResult<()> {
        let start = self.i;
        self.i += 1;
        if self.peek() == Some(b'*')
            && let Some(b) = self.peek_at(1)
            && b != b')'
        {
            if b.is_ascii_alphanumeric() || b == b'_' || b == b':' {
                return self.parse_star(br, start);
            }
            // "(*" followed by anything else (observed on PHP 8.5: "(*("
            // is a verb error at offset 2, "(*)" a quantifier error).
            return self.err(E::VerbUnknown, self.i + 1);
        }
        if self.peek() != Some(b'?') {
            self.open_group_depth()?;
            let node = if self.o.no_auto_capture {
                let (body, _) = self.group_body(None)?;
                Node::Group {
                    kind: GroupKind::NonCapture,
                    body: Box::new(body),
                    offset: start,
                }
            } else {
                self.capture_group(None, start)?
            };
            self.push_item(br, node, true);
            return Ok(());
        }
        self.i += 1;
        let Some(c) = self.peek() else {
            return self.err(E::MissingClosingParenthesis, self.p.len());
        };
        match c {
            b'#' => {
                // Comment up to the next ')'.
                while let Some(b) = self.peek() {
                    if b == b')' {
                        break;
                    }
                    self.i += 1;
                }
                if self.at_end() {
                    return self.err(E::MissingCommentClosing, self.p.len());
                }
                self.i += 1;
                Ok(())
            }
            b':' | b'|' | b'>' | b'=' | b'!' => {
                self.i += 1;
                self.open_group_depth()?;
                let node = match c {
                    b':' => {
                        let (body, _) = self.group_body(None)?;
                        Node::Group {
                            kind: GroupKind::NonCapture,
                            body: Box::new(body),
                            offset: start,
                        }
                    }
                    b'|' => {
                        self.has_branch_reset = true;
                        let base = self.ncap;
                        let (body, _) = self.group_body(Some(base))?;
                        Node::Group {
                            kind: GroupKind::NonCapture,
                            body: Box::new(body),
                            offset: start,
                        }
                    }
                    b'>' => {
                        let (body, _) = self.group_body(None)?;
                        Node::Group {
                            kind: GroupKind::Atomic,
                            body: Box::new(body),
                            offset: start,
                        }
                    }
                    b'=' => self.lookaround(
                        GroupKind::LookAhead {
                            negated: false,
                            atomic: true,
                        },
                        start,
                    )?,
                    _ => self.lookaround(
                        GroupKind::LookAhead {
                            negated: true,
                            atomic: true,
                        },
                        start,
                    )?,
                };
                self.push_item(br, node, true);
                Ok(())
            }
            b'*' => {
                // (?* non-atomic positive lookahead.
                self.i += 1;
                self.open_group_depth()?;
                let node = self.lookaround(
                    GroupKind::LookAhead {
                        negated: false,
                        atomic: false,
                    },
                    start,
                )?;
                self.push_item(br, node, true);
                Ok(())
            }
            b'<' => match self.peek_at(1) {
                Some(b'=') | Some(b'!') | Some(b'*') => {
                    let k = self.p[self.i + 1];
                    self.i += 2;
                    self.open_group_depth()?;
                    let kind = match k {
                        b'=' => GroupKind::LookBehind {
                            negated: false,
                            atomic: true,
                        },
                        b'!' => GroupKind::LookBehind {
                            negated: true,
                            atomic: true,
                        },
                        _ => GroupKind::LookBehind {
                            negated: false,
                            atomic: false,
                        },
                    };
                    let node = self.lookaround(kind, start)?;
                    self.push_item(br, node, true);
                    Ok(())
                }
                _ => {
                    self.i += 1;
                    self.named_group(br, b'>', start)
                }
            },
            b'\'' => {
                self.i += 1;
                self.named_group(br, b'\'', start)
            }
            b'P' => {
                self.i += 1;
                match self.peek() {
                    Some(b'<') => {
                        self.i += 1;
                        self.named_group(br, b'>', start)
                    }
                    Some(b'=') => {
                        self.i += 1;
                        let ns = self.i;
                        let name = self.read_name(ns)?;
                        if self.peek() != Some(b')') {
                            return self.err(E::MissingNameTerminator, self.i);
                        }
                        self.i += 1;
                        let caseless = self.o.caseless;
                        let restrict = self.o.caseless_restrict || !(self.utf || self.ucp);
                        self.push_item(
                            br,
                            Node::Backref {
                                target: GroupRef::name(&name, ns),
                                caseless,
                                case_restrict: restrict,
                            },
                            true,
                        );
                        Ok(())
                    }
                    Some(b'>') => {
                        self.i += 1;
                        let ns = self.i;
                        let name = self.read_name(ns)?;
                        if self.peek() != Some(b')') {
                            return self.err(E::MissingNameTerminator, self.i);
                        }
                        self.i += 1;
                        self.push_item(
                            br,
                            Node::Call {
                                target: GroupRef::name(&name, ns),
                            },
                            true,
                        );
                        Ok(())
                    }
                    None => self.err(E::MissingClosingParenthesis, self.p.len()),
                    Some(_) => self.err(E::UnrecognizedAfterQueryP, self.i),
                }
            }
            b'&' => {
                self.i += 1;
                let ns = self.i;
                let name = self.read_name(ns)?;
                if self.peek() != Some(b')') {
                    return self.err(E::MissingNameTerminator, self.i);
                }
                self.i += 1;
                self.push_item(
                    br,
                    Node::Call {
                        target: GroupRef::name(&name, ns),
                    },
                    true,
                );
                Ok(())
            }
            b'R' => {
                self.i += 1;
                if self.peek() != Some(b')') {
                    return self.err(E::ParensQueryRMissingClosing, self.i);
                }
                self.i += 1;
                self.push_item(
                    br,
                    Node::Call {
                        target: GroupRef::num(0, self.i - 1),
                    },
                    true,
                );
                Ok(())
            }
            b'+' if !self.peek_at(1).is_some_and(|d| d.is_ascii_digit()) => {
                self.err(E::BadRelativeReference, self.i)
            }
            b'0'..=b'9' | b'+' | b'-'
                if c.is_ascii_digit() || self.peek_at(1).is_some_and(|d| d.is_ascii_digit()) =>
            {
                let sign = match c {
                    b'+' => {
                        self.i += 1;
                        1
                    }
                    b'-' => {
                        self.i += 1;
                        -1
                    }
                    _ => 0,
                };
                let mut n: u64 = 0;
                while let Some(d) = self.peek().filter(u8::is_ascii_digit) {
                    n = (n * 10 + (d - b'0') as u64).min(1 << 40);
                    self.i += 1;
                }
                if self.peek() != Some(b')') {
                    if self.at_end() {
                        return self.err(E::MissingClosingParenthesis, self.p.len());
                    }
                    return self.err(E::MissingClosingParenthesis, self.i);
                }
                let close = self.i;
                let g = match sign {
                    0 => {
                        if n > MAX_QUANT as u64 {
                            return self.err(E::SubpatternNumberTooBig, close);
                        }
                        n as u32
                    }
                    -1 => {
                        if n == 0 {
                            return self.err(E::ZeroRelativeReference, close);
                        }
                        if n > self.ncap as u64 {
                            return self.err(E::BadSubpatternReference, close);
                        }
                        self.ncap + 1 - n as u32
                    }
                    _ => {
                        if n == 0 {
                            return self.err(E::ZeroRelativeReference, close);
                        }
                        let v = self.ncap as u64 + n;
                        if v > MAX_QUANT as u64 {
                            return self.err(E::SubpatternNumberTooBig, close);
                        }
                        v as u32
                    }
                };
                self.i += 1;
                self.push_item(
                    br,
                    Node::Call {
                        target: GroupRef::num(g, close),
                    },
                    true,
                );
                Ok(())
            }
            b'(' => {
                self.open_group_depth()?;
                let node = self.parse_conditional(start)?;
                self.push_item(br, node, true);
                Ok(())
            }
            b'C' => {
                self.parse_callout()?;
                Ok(())
            }
            b'[' => {
                self.i += 1;
                let n = self.parse_ext_class()?;
                self.push_item(br, n, true);
                Ok(())
            }
            _ => self.parse_options(br, start),
        }
    }

    fn named_group(&mut self, br: &mut Branch, close: u8, start: usize) -> PResult<()> {
        let ns = self.i;
        let name = self.read_name(ns)?;
        if self.peek() != Some(close) {
            return self.err(E::MissingNameTerminator, self.i);
        }
        self.i += 1;
        self.open_group_depth()?;
        let after = self.i;
        let node = self.capture_group(Some((name, after)), start)?;
        self.push_item(br, node, true);
        Ok(())
    }

    /// `(?C...)` callouts are accepted and ignored.
    fn parse_callout(&mut self) -> PResult<()> {
        self.i += 1;
        match self.peek() {
            Some(b')') => {
                self.i += 1;
                Ok(())
            }
            Some(d) if d.is_ascii_digit() => {
                let mut n: u32 = 0;
                while let Some(d) = self.peek().filter(u8::is_ascii_digit) {
                    n = (n * 10 + (d - b'0') as u32).min(100000);
                    self.i += 1;
                }
                if n > 255 {
                    return self.err(E::CalloutNumberTooBig, self.i);
                }
                if self.peek() != Some(b')') {
                    return self.err(E::MissingCalloutClosing, self.i);
                }
                self.i += 1;
                Ok(())
            }
            Some(d) if matches!(d, b'`' | b'\'' | b'"' | b'^' | b'%' | b'#' | b'$' | b'{') => {
                let end = if d == b'{' { b'}' } else { d };
                let s = self.i;
                self.i += 1;
                loop {
                    match self.peek() {
                        None => return self.err(E::CalloutNoStringDelimiter, s),
                        Some(x) if x == end => {
                            if self.peek_at(1) == Some(end) {
                                self.i += 2;
                                continue;
                            }
                            self.i += 1;
                            break;
                        }
                        _ => self.i += 1,
                    }
                }
                if self.peek() != Some(b')') {
                    return self.err(E::MissingCalloutClosing, self.i);
                }
                self.i += 1;
                Ok(())
            }
            None => self.err(E::MissingClosingParenthesis, self.p.len()),
            Some(_) => self.err(E::CalloutBadStringDelimiter, self.i),
        }
    }

    /// Option settings `(?imnsxJUr-...)`, `(?^...)`, optionally followed by
    /// `:` to start a group.
    fn parse_options(&mut self, br: &mut Branch, start: usize) -> PResult<()> {
        let mut o = self.o;
        let mut unset = false;
        let mut seen_caret = false;
        if self.peek() == Some(b'^') {
            seen_caret = true;
            self.i += 1;
            // (?^) is (?-imnrsx).
            o.caseless = false;
            o.multiline = false;
            o.no_auto_capture = false;
            o.caseless_restrict = false;
            o.dotall = false;
            o.extended = false;
            o.extended_more = false;
        }
        loop {
            let Some(c) = self.peek() else {
                return self.err(E::MissingClosingParenthesis, self.p.len());
            };
            let set = !unset;
            match c {
                b')' | b':' => break,
                b'-' => {
                    if unset || seen_caret {
                        return self.err(E::InvalidHyphenInOptions, self.i);
                    }
                    unset = true;
                }
                b'i' => o.caseless = set,
                b'm' => o.multiline = set,
                b'n' => o.no_auto_capture = set,
                b's' => o.dotall = set,
                b'J' => o.dupnames = set,
                b'U' => o.ungreedy = set,
                b'r' => o.caseless_restrict = set,
                b'x' => {
                    if set && self.peek_at(1) == Some(b'x') {
                        self.i += 1;
                        o.extended = true;
                        o.extended_more = true;
                    } else {
                        o.extended = set;
                        if !set {
                            o.extended_more = false;
                        }
                    }
                }
                b'a' => {
                    let next = self.peek_at(1);
                    match next {
                        Some(b'D') => {
                            self.i += 1;
                            o.ascii_bsd = set;
                        }
                        Some(b'S') => {
                            self.i += 1;
                            o.ascii_bss = set;
                        }
                        Some(b'W') => {
                            self.i += 1;
                            o.ascii_bsw = set;
                        }
                        Some(b'P') => {
                            self.i += 1;
                            o.ascii_posix = set;
                            o.ascii_digit = set;
                        }
                        Some(b'T') => {
                            self.i += 1;
                            o.ascii_digit = set;
                        }
                        _ => {
                            o.ascii_bsd = set;
                            o.ascii_bss = set;
                            o.ascii_bsw = set;
                            o.ascii_posix = set;
                            o.ascii_digit = set;
                        }
                    }
                }
                _ => return self.err(E::InvalidAfterParensQuery, self.i),
            }
            self.i += 1;
        }
        if self.peek() == Some(b')') {
            self.i += 1;
            self.o = o;
            // An option setting is not a repeatable item.
            br.repeatable = false;
            return Ok(());
        }
        // ':' — a non-capturing group with these options.
        self.i += 1;
        self.open_group_depth()?;
        let saved = self.o;
        self.o = o;
        let (body, _) = self.parse_alternation(None)?;
        if self.peek() != Some(b')') {
            return self.err(E::MissingClosingParenthesis, self.p.len());
        }
        self.i += 1;
        self.o = saved;
        self.depth -= 1;
        self.push_item(
            br,
            Node::Group {
                kind: GroupKind::NonCapture,
                body: Box::new(body),
                offset: start,
            },
            true,
        );
        Ok(())
    }

    /// `(?(` conditionals; the position is at the inner `(`.
    fn parse_conditional(&mut self, start: usize) -> PResult<Node> {
        let cond_open = self.i;
        self.i += 1;
        let cond = self.parse_condition(start, cond_open)?;
        let (body, nbr) = self.group_body(None)?;
        let (yes, no) = match body {
            Node::Alt(mut v) if nbr == 2 => {
                let no = v.pop().unwrap();
                let yes = v.pop().unwrap();
                (yes, Some(Box::new(no)))
            }
            Node::Alt(v) => (Node::Alt(v), None),
            other => (other, None),
        };
        Ok(Node::Cond {
            cond,
            yes: Box::new(yes),
            no,
            branches: nbr,
            offset: start,
        })
    }

    fn parse_condition(&mut self, start: usize, cond_open: usize) -> PResult<Condition> {
        let _ = start;
        let Some(c) = self.peek() else {
            return self.err(E::MissingClosingParenthesis, self.p.len());
        };
        // Assertion conditions.
        if c == b'?' || c == b'*' {
            let save = self.i;
            if c == b'?' && self.peek_at(1) == Some(b'C') {
                // A callout before the assertion.
                self.i += 1;
                self.parse_callout()?;
                if !self.starts_with(b"(?") && !self.starts_with(b"(*") {
                    return self.err(E::ConditionAssertionExpected, self.i);
                }
                self.i += 1;
                return self.parse_condition(start, cond_open);
            }
            if c == b'?' && self.peek_at(1) == Some(b'#') {
                // A comment before the assertion (observed on PHP 8.5:
                // (?(?#x)(?=a)b) is valid).
                let Some(k) = self.p[self.i..].iter().position(|&b| b == b')') else {
                    return self.err(E::MissingCommentClosing, self.p.len());
                };
                self.i += k + 1;
                if !self.starts_with(b"(?") && !self.starts_with(b"(*") {
                    return self.err(E::ConditionAtomicAssertionExpected, self.i);
                }
                let open = self.i;
                self.i += 1;
                return self.parse_condition(start, open);
            }
            let kind = if c == b'?' {
                match (self.peek_at(1), self.peek_at(2)) {
                    (Some(b'='), _) => Some((
                        GroupKind::LookAhead {
                            negated: false,
                            atomic: true,
                        },
                        2,
                    )),
                    (Some(b'!'), _) => Some((
                        GroupKind::LookAhead {
                            negated: true,
                            atomic: true,
                        },
                        2,
                    )),
                    (Some(b'<'), Some(b'=')) => Some((
                        GroupKind::LookBehind {
                            negated: false,
                            atomic: true,
                        },
                        3,
                    )),
                    (Some(b'<'), Some(b'!')) => Some((
                        GroupKind::LookBehind {
                            negated: true,
                            atomic: true,
                        },
                        3,
                    )),
                    _ => None,
                }
            } else {
                let rest = &self.p[self.i + 1..];
                let names: &[(&[u8], GroupKind)] = &[
                    (
                        b"pla:",
                        GroupKind::LookAhead {
                            negated: false,
                            atomic: true,
                        },
                    ),
                    (
                        b"positive_lookahead:",
                        GroupKind::LookAhead {
                            negated: false,
                            atomic: true,
                        },
                    ),
                    (
                        b"nla:",
                        GroupKind::LookAhead {
                            negated: true,
                            atomic: true,
                        },
                    ),
                    (
                        b"negative_lookahead:",
                        GroupKind::LookAhead {
                            negated: true,
                            atomic: true,
                        },
                    ),
                    (
                        b"plb:",
                        GroupKind::LookBehind {
                            negated: false,
                            atomic: true,
                        },
                    ),
                    (
                        b"positive_lookbehind:",
                        GroupKind::LookBehind {
                            negated: false,
                            atomic: true,
                        },
                    ),
                    (
                        b"nlb:",
                        GroupKind::LookBehind {
                            negated: true,
                            atomic: true,
                        },
                    ),
                    (
                        b"negative_lookbehind:",
                        GroupKind::LookBehind {
                            negated: true,
                            atomic: true,
                        },
                    ),
                ];
                names
                    .iter()
                    .find(|(n, _)| rest.starts_with(n))
                    .map(|(n, k)| (*k, n.len() + 1))
            };
            let Some((kind, skip)) = kind else {
                self.i = save;
                return self.err(E::ConditionAtomicAssertionExpected, cond_open);
            };
            self.i += skip;
            self.open_group_depth()?;
            let node = self.lookaround(kind, cond_open)?;
            return Ok(Condition::Assert(Box::new(node)));
        }
        // Reference conditions.
        if c.is_ascii_digit()
            || ((c == b'+' || c == b'-') && self.peek_at(1).is_some_and(|d| d.is_ascii_digit()))
        {
            let sign = match c {
                b'+' => {
                    self.i += 1;
                    1
                }
                b'-' => {
                    self.i += 1;
                    -1
                }
                _ => 0,
            };
            let mut n: u64 = 0;
            while let Some(d) = self.peek().filter(u8::is_ascii_digit) {
                n = (n * 10 + (d - b'0') as u64).min(1 << 40);
                self.i += 1;
            }
            if self.at_end() {
                return self.err(E::MissingConditionClosing, self.i);
            }
            // Zero is rejected before the closing parenthesis is looked at
            // (observed on PHP 8.5: (?(0?=K) fails at the "?").
            if n == 0 {
                let kind = if sign == 0 {
                    E::BadSubpatternReference
                } else {
                    E::ZeroRelativeReference
                };
                return self.err(kind, self.i);
            }
            if self.peek() != Some(b')') {
                if self.at_end() {
                    return self.err(E::MissingClosingParenthesis, self.p.len());
                }
                return self.err(E::MissingConditionClosing, self.i);
            }
            let close = self.i;
            self.i += 1;
            let g = match sign {
                0 => {
                    if n == 0 {
                        return self.err(E::BadSubpatternReference, close);
                    }
                    if n > MAX_QUANT as u64 {
                        return self.err(E::SubpatternNumberTooBig, close);
                    }
                    n as u32
                }
                -1 => {
                    if n == 0 {
                        return self.err(E::ZeroRelativeReference, close);
                    }
                    if n > self.ncap as u64 {
                        return self.err(E::BadSubpatternReference, close);
                    }
                    self.ncap + 1 - n as u32
                }
                _ => {
                    if n == 0 {
                        return self.err(E::ZeroRelativeReference, close);
                    }
                    (self.ncap as u64 + n).min(MAX_QUANT as u64 + 1) as u32
                }
            };
            return Ok(Condition::GroupSet(GroupRef::num(
                g,
                close.saturating_sub(2).max(cond_open),
            )));
        }
        if c == b'<' || c == b'\'' {
            let close = if c == b'<' { b'>' } else { b'\'' };
            self.i += 1;
            let ns = self.i;
            let name = self.read_name(ns)?;
            if self.peek() != Some(close) {
                return self.err(E::MissingNameTerminator, self.i);
            }
            self.i += 1;
            if self.peek() != Some(b')') {
                return self.err(E::MissingConditionClosing, self.i);
            }
            self.i += 1;
            return Ok(Condition::GroupSet(GroupRef::name(&name, ns)));
        }
        if self.starts_with(b"R&") {
            self.i += 2;
            let ns = self.i;
            let name = self.read_name(ns)?;
            if self.peek() != Some(b')') {
                return self.err(E::MissingConditionClosing, self.i);
            }
            self.i += 1;
            return Ok(Condition::Recursion(Some(GroupRef::name(&name, ns))));
        }
        if self.starts_with(b"VERSION") && matches!(self.peek_at(7), Some(b'=') | Some(b'>')) {
            self.i += 7;
            let ge = if self.peek() == Some(b'>') {
                self.i += 1;
                if self.peek() != Some(b'=') {
                    return self.err(E::VersionConditionSyntax, self.i);
                }
                true
            } else {
                false
            };
            self.i += 1;
            let mut major: u32 = 0;
            let ds = self.i;
            while let Some(d) = self.peek().filter(u8::is_ascii_digit) {
                major = (major * 10 + (d - b'0') as u32).min(100000);
                self.i += 1;
            }
            if self.i == ds || self.i - ds > 3 {
                return self.err(E::VersionConditionSyntax, self.i);
            }
            let mut minor: u32 = 0;
            if self.peek() == Some(b'.') {
                self.i += 1;
                let ms = self.i;
                while let Some(d) = self.peek().filter(u8::is_ascii_digit) {
                    minor = minor * 10 + (d - b'0') as u32;
                    self.i += 1;
                }
                if self.i == ms || self.i - ms > 2 {
                    return self.err(E::VersionConditionSyntax, self.i);
                }
                if self.i - ms == 1 {
                    minor *= 10;
                }
            }
            if self.peek() != Some(b')') {
                return self.err(E::VersionConditionSyntax, self.i);
            }
            self.i += 1;
            let (vm, vn) = VERSION;
            let r = if ge {
                (vm, vn) >= (major, minor)
            } else {
                (vm, vn) == (major, minor)
            };
            return Ok(Condition::Version(r));
        }
        // A bare name: R, Rdigits, DEFINE or a group name.
        let ns = self.i;
        let _ = cond_open;
        let name = self.read_name(ns)?;
        if self.peek() != Some(b')') {
            return self.err(E::MissingNameTerminator, self.i);
        }
        self.i += 1;
        if name == b"DEFINE" {
            return Ok(Condition::Define);
        }
        if name[0] == b'R' && name[1..].iter().all(u8::is_ascii_digit) {
            // Recursion test unless a group of that name exists (decided by
            // the resolver).
            let num: u32 = if name.len() == 1 {
                0
            } else {
                std::str::from_utf8(&name[1..])
                    .unwrap()
                    .parse::<u64>()
                    .unwrap_or(u64::MAX)
                    .min(u32::MAX as u64) as u32
            };
            let mut r = GroupRef::name(&name, ns);
            r.num = num;
            return Ok(Condition::Recursion(if name.len() == 1 && num == 0 {
                Some(GroupRef { num: u32::MAX, ..r })
            } else {
                Some(r)
            }));
        }
        Ok(Condition::GroupSet(GroupRef::name(&name, ns)))
    }

    /// `(*` items in the body: verbs and alphabetic assertions.
    fn parse_star(&mut self, br: &mut Branch, start: usize) -> PResult<()> {
        self.i += 1; // past '*'
        let ns = self.i;
        while self
            .peek()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            self.i += 1;
        }
        let word = self.p[ns..self.i].to_vec();
        let name_end = self.i;
        // Names starting with a lower case letter are alphabetic assertions
        // (observed on PHP 8.5: "(*pLA:a)" and "(*sr)" are
        // "(*alpha_assertion) not recognized" at the end of the name).
        let alpha = word.first().is_some_and(|b| b.is_ascii_lowercase());
        if alpha && self.peek() != Some(b':') {
            return self.err(E::AlphaAssertionUnknown, name_end);
        }
        if self.at_end() {
            return self.err(E::VerbUnknown, self.p.len());
        }
        if !alpha
            && !matches!(
                word.as_slice(),
                b"" | b"MARK"
                    | b"ACCEPT"
                    | b"FAIL"
                    | b"F"
                    | b"COMMIT"
                    | b"PRUNE"
                    | b"SKIP"
                    | b"THEN"
            )
        {
            return self.err(E::VerbUnknown, name_end);
        }
        let next = self.p[self.i];
        if next == b':' && alpha {
            // Alphabetic assertion or group.
            let colon = self.i;
            self.i += 1;
            let kind = match word.as_slice() {
                b"pla" | b"positive_lookahead" => Some(GroupKind::LookAhead {
                    negated: false,
                    atomic: true,
                }),
                b"nla" | b"negative_lookahead" => Some(GroupKind::LookAhead {
                    negated: true,
                    atomic: true,
                }),
                b"plb" | b"positive_lookbehind" => Some(GroupKind::LookBehind {
                    negated: false,
                    atomic: true,
                }),
                b"nlb" | b"negative_lookbehind" => Some(GroupKind::LookBehind {
                    negated: true,
                    atomic: true,
                }),
                b"napla" | b"non_atomic_positive_lookahead" => Some(GroupKind::LookAhead {
                    negated: false,
                    atomic: false,
                }),
                b"naplb" | b"non_atomic_positive_lookbehind" => Some(GroupKind::LookBehind {
                    negated: false,
                    atomic: false,
                }),
                b"atomic" => Some(GroupKind::Atomic),
                b"sr" | b"script_run" => Some(GroupKind::ScriptRun { atomic: false }),
                b"asr" | b"atomic_script_run" => Some(GroupKind::ScriptRun { atomic: true }),
                b"scs" | b"scan_substring" => None,
                _ => return self.err(E::AlphaAssertionUnknown, colon),
            };
            self.open_group_depth()?;
            let node = match kind {
                Some(k @ (GroupKind::LookAhead { .. } | GroupKind::LookBehind { .. })) => {
                    self.lookaround(k, start)?
                }
                Some(GroupKind::ScriptRun { atomic }) => {
                    let (body, _) = self.group_body(None)?;
                    let body = if atomic {
                        Node::Group {
                            kind: GroupKind::Atomic,
                            body: Box::new(body),
                            offset: start,
                        }
                    } else {
                        body
                    };
                    Node::Group {
                        kind: GroupKind::ScriptRun { atomic },
                        body: Box::new(body),
                        offset: start,
                    }
                }
                Some(k) => {
                    let (body, _) = self.group_body(None)?;
                    Node::Group {
                        kind: k,
                        body: Box::new(body),
                        offset: start,
                    }
                }
                None => self.parse_scan_substring(start)?,
            };
            self.push_item(br, node, true);
            return Ok(());
        }
        // A verb: NAME or NAME:arg.
        let mut arg: Option<Vec<u8>> = None;
        if next == b':' {
            self.i += 1;
            let as_ = self.i;
            while let Some(b) = self.peek() {
                if b == b')' {
                    break;
                }
                self.i += 1;
            }
            if self.at_end() {
                return self.err(E::VerbUnknown, self.p.len());
            }
            if self.i - as_ > MAX_VERB_NAME {
                return self.err(E::VerbNameTooLong, self.i);
            }
            if self.i > as_ {
                arg = Some(self.p[as_..self.i].to_vec());
            }
        } else if next != b')' {
            return self.err(E::VerbUnknown, self.i);
        }
        let close = self.i;
        self.i += 1; // ')'
        let verb = match word.as_slice() {
            b"" | b"MARK" => {
                if arg.is_none() {
                    return self.err(E::MarkMissingArgument, close);
                }
                Verb::Mark
            }
            b"ACCEPT" => Verb::Accept,
            b"FAIL" | b"F" => Verb::Fail,
            b"COMMIT" => Verb::Commit,
            b"PRUNE" => Verb::Prune,
            b"SKIP" => {
                if arg.is_some() {
                    Verb::SkipName
                } else {
                    Verb::Skip
                }
            }
            b"THEN" => Verb::Then,
            _ => return self.err(E::VerbUnknown, close),
        };
        let name = arg.map(|a| Arc::from(a.as_slice()));
        let repeatable = verb == Verb::Accept;
        // (*ACCEPT:NAME) and (*FAIL:NAME) record the mark first.
        self.push_item(br, Node::Verb { verb, name }, repeatable);
        Ok(())
    }

    /// `(*scan_substring:(grouplist)...)` after the colon.
    fn parse_scan_substring(&mut self, start: usize) -> PResult<Node> {
        let _ = start;
        if self.peek() != Some(b'(') {
            return self.err(E::MissingClosingParenthesis, self.i);
        }
        self.i += 1;
        let mut groups = Vec::new();
        loop {
            match self.peek() {
                Some(b'<') | Some(b'\'') => {
                    let close = if self.peek() == Some(b'<') {
                        b'>'
                    } else {
                        b'\''
                    };
                    self.i += 1;
                    let ns = self.i;
                    let name = self.read_name(ns)?;
                    if self.peek() != Some(close) {
                        return self.err(E::MissingNameTerminator, self.i);
                    }
                    self.i += 1;
                    groups.push(GroupRef::name(&name, ns));
                }
                Some(b'+' | b'-' | b'0'..=b'9') => {
                    let sign = self.peek();
                    if matches!(sign, Some(b'+' | b'-')) {
                        self.i += 1;
                    }
                    let ds = self.i;
                    let mut n: u64 = 0;
                    while let Some(d) = self.peek().filter(u8::is_ascii_digit) {
                        n = (n * 10 + (d - b'0') as u64).min(1 << 40);
                        self.i += 1;
                    }
                    if self.i == ds {
                        return self.err(E::MissingClosingParenthesis, self.i);
                    }
                    let g = match sign {
                        Some(b'+') => self.ncap as u64 + n,
                        Some(b'-') => {
                            if n == 0 || n > self.ncap as u64 {
                                return self.err(E::BadSubpatternReference, self.i);
                            }
                            (self.ncap + 1) as u64 - n
                        }
                        _ => n,
                    };
                    groups.push(GroupRef::num(g.min(u32::MAX as u64) as u32, self.i));
                }
                _ => return self.err(E::MissingClosingParenthesis, self.i),
            }
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b')') => {
                    self.i += 1;
                    break;
                }
                _ => return self.err(E::MissingClosingParenthesis, self.i),
            }
        }
        let (body, _) = self.group_body(None)?;
        Ok(Node::ScanSubstring {
            groups,
            body: Box::new(body),
        })
    }
}

fn prop_of(kind: PropKind, neg: bool) -> PropSpec {
    PropSpec { kind, neg }
}

/// Resolves a loosely-normalised property name (lowercase, without spaces,
/// hyphens and underscores).
fn lookup_property(n: &[u8]) -> Option<PropKind> {
    let s = std::str::from_utf8(n).ok()?;
    // Prefixed forms: sc:, scx:, script:, scriptextensions:, bc:, bidiclass:
    if let Some(pos) = s.find([':', '=']) {
        let (k, v) = (&s[..pos], &s[pos + 1..]);
        return match k {
            "sc" | "script" => Some(PropKind::Script(lookup_script_name(v)?)),
            "scx" | "scriptextensions" => Some(PropKind::Scx(lookup_script_name(v)?)),
            "bc" | "bidiclass" => {
                let up = v.to_ascii_uppercase();
                let idx = unicode::BIDI_NAMES.iter().position(|b| *b == up)?;
                Some(PropKind::Bidi(idx as u8))
            }
            _ => None,
        };
    }
    match s {
        "any" => return Some(PropKind::Any),
        "l&" | "lc" => return Some(PropKind::Gc(class::GC_MASK_LC)),
        "xan" => return Some(PropKind::Xan),
        "xps" | "xsp" => return Some(PropKind::Xps),
        "xwd" => return Some(PropKind::Xwd),
        "xuc" => return Some(PropKind::Xuc),
        "ascii" => return Some(PropKind::Ascii),
        "c" => return Some(PropKind::Gc(class::GC_MASK_C)),
        "l" => return Some(PropKind::Gc(class::GC_MASK_L)),
        "m" => return Some(PropKind::Gc(class::GC_MASK_M)),
        "n" => return Some(PropKind::Gc(class::GC_MASK_N)),
        "p" => return Some(PropKind::Gc(class::GC_MASK_P)),
        "s" => return Some(PropKind::Gc(class::GC_MASK_S)),
        "z" => return Some(PropKind::Gc(class::GC_MASK_Z)),
        _ => {}
    }
    if s.len() == 2 {
        for (i, g) in unicode::GC_NAMES.iter().enumerate() {
            if g.eq_ignore_ascii_case(s) {
                return Some(PropKind::Gc(1 << i));
            }
        }
    }
    if let Some(idx) = lookup_script_name(s) {
        return Some(PropKind::Scx(idx));
    }
    if let Some(idx) = unicode::lookup_bprop(s) {
        return Some(PropKind::Bprop(idx));
    }
    None
}

fn lookup_script_name(s: &str) -> Option<u8> {
    if s.eq_ignore_ascii_case("hrkt") || s.eq_ignore_ascii_case("katakanaorhiragana") {
        return None;
    }
    unicode::lookup_script(s)
}

/// Compile-time configuration for the parser.
#[derive(Clone, Debug)]
pub struct ParseConfig {
    pub flags: Flags,
    pub newline: Newline,
    pub bsr_anycrlf: bool,
}

/// Parses a pattern.
pub fn parse(pattern: &[u8], cfg: &ParseConfig) -> Result<Parsed, CompileError> {
    let f = cfg.flags;
    let mut ps = Parser {
        p: pattern,
        i: 0,
        utf: f.contains(Flags::UTF),
        ucp: f.contains(Flags::UCP),
        o: Opts {
            caseless: f.contains(Flags::CASELESS),
            multiline: f.contains(Flags::MULTILINE),
            dotall: f.contains(Flags::DOTALL),
            extended: f.contains(Flags::EXTENDED) || f.contains(Flags::EXTENDED_MORE),
            extended_more: f.contains(Flags::EXTENDED_MORE),
            no_auto_capture: f.contains(Flags::NO_AUTO_CAPTURE),
            ungreedy: f.contains(Flags::UNGREEDY),
            dupnames: f.contains(Flags::DUPNAMES),
            caseless_restrict: f.contains(Flags::CASELESS_RESTRICT),
            ..Opts::default()
        },
        dollar_endonly: f.contains(Flags::DOLLAR_ENDONLY),
        newline: cfg.newline,
        bsr_anycrlf: cfg.bsr_anycrlf,
        turkish: false,
        allow_empty_class: f.contains(Flags::ALLOW_EMPTY_CLASS),
        ncap: 0,
        names: Vec::new(),
        depth: 0,
        lookaround_depth: 0,
        keep_in_lookaround: false,
        has_branch_reset: false,
        has_dup_names: false,
        group_names: Vec::new(),
        has_cr_lf: false,
        has_ext_class: false,
    };
    let mut out = Parsed {
        node: Node::Empty,
        ncap: 0,
        names: Vec::new(),
        utf: false,
        ucp: false,
        newline: cfg.newline,
        bsr_anycrlf: cfg.bsr_anycrlf,
        no_start_opt: f.contains(Flags::NO_START_OPTIMIZE),
        no_auto_possess: f.contains(Flags::NO_AUTO_POSSESS),
        no_dotstar_anchor: f.contains(Flags::NO_DOTSTAR_ANCHOR),
        notempty: false,
        notempty_atstart: false,
        limit_match: None,
        limit_depth: None,
        limit_heap: None,
        has_branch_reset: false,
        has_dup_names: false,
        turkish: false,
        has_cr_lf: false,
        has_ext_class: false,
    };
    // Start-of-pattern items (pcre2pattern "Special start-of-pattern items").
    loop {
        let rest = &pattern[ps.i..];
        if !rest.starts_with(b"(*") {
            break;
        }
        let simple: &[(&[u8], u8)] = &[
            (b"(*UTF)", 1),
            (b"(*UCP)", 2),
            (b"(*CR)", 3),
            (b"(*LF)", 4),
            (b"(*CRLF)", 5),
            (b"(*ANYCRLF)", 6),
            (b"(*ANY)", 7),
            (b"(*NUL)", 8),
            (b"(*BSR_ANYCRLF)", 9),
            (b"(*BSR_UNICODE)", 10),
            (b"(*NO_AUTO_POSSESS)", 11),
            (b"(*NO_DOTSTAR_ANCHOR)", 12),
            (b"(*NO_JIT)", 13),
            (b"(*NO_START_OPT)", 14),
            (b"(*NOTEMPTY)", 15),
            (b"(*NOTEMPTY_ATSTART)", 16),
            (b"(*CASELESS_RESTRICT)", 17),
            (b"(*TURKISH_CASING)", 18),
        ];
        if let Some(&(s, k)) = simple.iter().find(|(s, _)| rest.starts_with(s)) {
            ps.i += s.len();
            match k {
                1 => ps.utf = true,
                2 => ps.ucp = true,
                3 => ps.newline = Newline::Cr,
                4 => ps.newline = Newline::Lf,
                5 => ps.newline = Newline::CrLf,
                6 => ps.newline = Newline::AnyCrLf,
                7 => ps.newline = Newline::Any,
                8 => ps.newline = Newline::Nul,
                9 => ps.bsr_anycrlf = true,
                10 => ps.bsr_anycrlf = false,
                11 => out.no_auto_possess = true,
                12 => out.no_dotstar_anchor = true,
                13 => {}
                14 => out.no_start_opt = true,
                15 => out.notempty = true,
                16 => out.notempty_atstart = true,
                17 => ps.o.caseless_restrict = true,
                _ => ps.turkish = true,
            }
            continue;
        }
        let limits: &[(&[u8], u8)] = &[
            (b"(*LIMIT_HEAP=", 1),
            (b"(*LIMIT_MATCH=", 2),
            (b"(*LIMIT_DEPTH=", 3),
            (b"(*LIMIT_RECURSION=", 3),
        ];
        if let Some(&(s, k)) = limits.iter().find(|(s, _)| rest.starts_with(s)) {
            let mut j = ps.i + s.len();
            let ds = j;
            let mut v: u64 = 0;
            while j < pattern.len() && pattern[j].is_ascii_digit() {
                v = (v * 10 + (pattern[j] - b'0') as u64).min(u32::MAX as u64);
                j += 1;
            }
            if j == ds || j >= pattern.len() || pattern[j] != b')' {
                return Err(CompileError::new(E::VerbUnknown, j));
            }
            let v = v as u32;
            let slot = match k {
                1 => &mut out.limit_heap,
                2 => &mut out.limit_match,
                _ => &mut out.limit_depth,
            };
            *slot = Some(slot.map_or(v, |o| o.min(v)));
            ps.i = j + 1;
            continue;
        }
        break;
    }
    if ps.utf
        && !f.contains(Flags::NO_UTF_CHECK)
        && let Err((e, off)) = utf8::validate(pattern)
    {
        return Err(CompileError::new(E::Utf8(e), off));
    }
    let (node, _) = ps.parse_alternation(None)?;
    if ps.i < pattern.len() {
        // Only a ')' can stop the top level.
        return Err(CompileError::new(E::UnmatchedClosingParenthesis, ps.i));
    }
    if ps.keep_in_lookaround {
        return Err(CompileError::new(E::BackslashKInLookaround, pattern.len()));
    }
    out.node = node;
    out.ncap = ps.ncap;
    out.names = ps.names;
    out.utf = ps.utf;
    out.ucp = ps.ucp;
    out.newline = ps.newline;
    out.bsr_anycrlf = ps.bsr_anycrlf;
    out.has_branch_reset = ps.has_branch_reset;
    out.has_dup_names = ps.has_dup_names;
    out.turkish = ps.turkish;
    out.has_cr_lf = ps.has_cr_lf;
    out.has_ext_class = ps.has_ext_class;
    Ok(out)
}

/// The class used for `\w` when matching word boundaries (shared by the
/// compiler).
pub fn word_set(ucp: bool) -> CharSet {
    let mut b = ClassBuilder::new();
    if ucp {
        b.add_pred(
            class::is_ucp_word,
            Some(HighItem::Gc {
                mask: class::GC_MASK_WORD,
                neg: false,
            }),
        );
    } else {
        b.add_pred(class::is_ascii_word, None);
    }
    b.build(false)
}
