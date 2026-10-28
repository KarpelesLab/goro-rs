//! The HTML tokenizer (WHATWG HTML, "13.2.5 Tokenization").
//!
//! Works on preprocessed UTF-8 input (newlines normalized, see
//! [`preprocess`]). All characters the state machine treats specially are
//! ASCII, so the machine runs on bytes and copies non-ASCII bytes through
//! unchanged; runs of ordinary text are found with `memchr`.
//!
//! Consecutive character tokens are merged into one [`Token::Text`] until the
//! next non-character token: this matches how PHP (lexbor) reports the span
//! of a character token in tree errors (observed on PHP 8.5: for
//! `<>x</> <!doctype html>` the text token spans columns 1-7).
//!
//! Processing instructions are not part of the HTML syntax PHP 8.5
//! implements: `<?` starts a bogus comment (observed on PHP 8.5).

use super::entities::ENTITIES;

/// Tokenizer states (the spec's names).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum State {
    Data,
    RcData,
    RawText,
    ScriptData,
    PlainText,
    TagOpen,
    EndTagOpen,
    TagName,
    RcDataLt,
    RcDataEndTagOpen,
    RcDataEndTagName,
    RawTextLt,
    RawTextEndTagOpen,
    RawTextEndTagName,
    ScriptDataLt,
    ScriptDataEndTagOpen,
    ScriptDataEndTagName,
    ScriptDataEscapeStart,
    ScriptDataEscapeStartDash,
    ScriptDataEscaped,
    ScriptDataEscapedDash,
    ScriptDataEscapedDashDash,
    ScriptDataEscapedLt,
    ScriptDataEscapedEndTagOpen,
    ScriptDataEscapedEndTagName,
    ScriptDataDoubleEscapeStart,
    ScriptDataDoubleEscaped,
    ScriptDataDoubleEscapedDash,
    ScriptDataDoubleEscapedDashDash,
    ScriptDataDoubleEscapedLt,
    ScriptDataDoubleEscapeEnd,
    BeforeAttrName,
    AttrName,
    AfterAttrName,
    BeforeAttrValue,
    AttrValueDq,
    AttrValueSq,
    AttrValueUnq,
    AfterAttrValueQuoted,
    SelfClosingStartTag,
    BogusComment,
    MarkupDeclOpen,
    CommentStart,
    CommentStartDash,
    Comment,
    CommentLt,
    CommentLtBang,
    CommentLtBangDash,
    CommentLtBangDashDash,
    CommentEndDash,
    CommentEnd,
    CommentEndBang,
    Doctype,
    BeforeDoctypeName,
    DoctypeName,
    AfterDoctypeName,
    AfterDoctypePublicKw,
    BeforeDoctypePublicId,
    DoctypePublicIdDq,
    DoctypePublicIdSq,
    AfterDoctypePublicId,
    BetweenDoctypePublicSystem,
    AfterDoctypeSystemKw,
    BeforeDoctypeSystemId,
    DoctypeSystemIdDq,
    DoctypeSystemIdSq,
    AfterDoctypeSystemId,
    BogusDoctype,
    CdataSection,
    CdataSectionBracket,
    CdataSectionEnd,
}

/// An attribute of a tag token.
#[derive(Clone, Debug, Default)]
pub(crate) struct Attr {
    pub name: String,
    pub value: String,
}

/// A start or end tag token.
#[derive(Clone, Debug, Default)]
pub(crate) struct Tag {
    pub end: bool,
    pub name: String,
    pub attrs: Vec<Attr>,
    pub self_closing: bool,
    /// Byte offset of the `<`.
    pub start: usize,
    /// Byte offsets of the tag name (`name_end` exclusive).
    pub name_first: usize,
    pub name_end: usize,
}

impl Tag {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.value.as_str())
    }
}

/// A DOCTYPE token.
#[derive(Clone, Debug, Default)]
pub(crate) struct Doctype {
    pub name: Option<String>,
    pub public_id: Option<String>,
    pub system_id: Option<String>,
    pub force_quirks: bool,
    /// Byte offset of the `<`.
    pub start: usize,
    /// Offsets of the `DOCTYPE` keyword (`kw_end` exclusive).
    pub kw_first: usize,
    pub kw_end: usize,
}

/// A token.
#[derive(Debug)]
pub(crate) enum Token {
    Tag(Tag),
    Doctype(Box<Doctype>),
    Comment {
        text: String,
        /// Offset of the `<`.
        start: usize,
        /// Offset where the comment data starts in the source.
        data: usize,
    },
    /// A run of character tokens covering the source bytes `first..end`.
    Text {
        text: String,
        first: usize,
        end: usize,
    },
    Eof {
        pos: usize,
    },
}

/// A tokenizer parse error: code and byte offset.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TokError {
    pub code: &'static str,
    pub pos: usize,
}

/// Normalizes newlines (CR LF and lone CR become LF), per "13.2.3.5
/// Preprocessing the input stream".
pub(crate) fn preprocess(input: String) -> String {
    if memchr::memchr(b'\r', input.as_bytes()).is_none() {
        return input;
    }
    let b = input.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(off) = memchr::memchr(b'\r', &b[i..]) {
        out.extend_from_slice(&b[i..i + off]);
        out.push(b'\n');
        i += off + 1;
        if b.get(i) == Some(&b'\n') {
            i += 1;
        }
    }
    out.extend_from_slice(&b[i..]);
    String::from_utf8(out).expect("newline normalization keeps UTF-8")
}

#[inline]
fn is_ws(c: u8) -> bool {
    matches!(c, b'\t' | b'\n' | 0x0C | b' ')
}

#[inline]
fn bytes_to_string(v: Vec<u8>) -> String {
    match String::from_utf8(v) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    }
}

/// Windows-1252 replacements for numeric references 0x80..0x9F
/// ("13.2.5.84 Numeric character reference end state").
fn c1_replacement(n: u32) -> Option<u32> {
    Some(match n {
        0x80 => 0x20AC,
        0x82 => 0x201A,
        0x83 => 0x0192,
        0x84 => 0x201E,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02C6,
        0x89 => 0x2030,
        0x8A => 0x0160,
        0x8B => 0x2039,
        0x8C => 0x0152,
        0x8E => 0x017D,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201C,
        0x94 => 0x201D,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02DC,
        0x99 => 0x2122,
        0x9A => 0x0161,
        0x9B => 0x203A,
        0x9C => 0x0153,
        0x9E => 0x017E,
        0x9F => 0x0178,
        _ => return None,
    })
}

/// How many leading bytes of `s` are a prefix of some reference name.
fn prefix_scan_len(s: &[u8]) -> usize {
    let mut l = 0;
    while l < s.len() && l < 40 {
        let c = s[l];
        if !(c.is_ascii_alphanumeric() || c == b';') {
            break;
        }
        let pre = &s[..l + 1];
        let i = ENTITIES.partition_point(|(k, _)| k.as_bytes() < pre);
        if i >= ENTITIES.len() || !ENTITIES[i].0.as_bytes().starts_with(pre) {
            break;
        }
        l += 1;
        if c == b';' {
            break;
        }
    }
    l
}

/// Looks up the longest named character reference at the start of `s`
/// (which follows the `&`). Returns the matched length and the replacement.
pub(crate) fn match_named_ref(s: &[u8]) -> Option<(usize, &'static str)> {
    let mut n = 0;
    while n < s.len() && n < 40 && s[n].is_ascii_alphanumeric() {
        n += 1;
    }
    if n == 0 {
        return None;
    }
    let find = |name: &[u8]| -> Option<&'static str> {
        ENTITIES
            .binary_search_by(|(k, _)| k.as_bytes().cmp(name))
            .ok()
            .map(|i| ENTITIES[i].1)
    };
    if s.get(n) == Some(&b';')
        && n < 40
        && let Some(v) = find(&s[..n + 1])
    {
        return Some((n + 1, v));
    }
    // Legacy names without semicolon are at most 6 characters long.
    let mut l = n.min(6);
    while l > 0 {
        if let Some(v) = find(&s[..l]) {
            return Some((l, v));
        }
        l -= 1;
    }
    None
}

pub(crate) struct Tokenizer {
    input: Vec<u8>,
    pos: usize,
    pub state: State,
    return_state: State,
    /// The tree builder sets this when the adjusted current node is not in
    /// the HTML namespace (CDATA sections allowed).
    pub allow_cdata: bool,
    pub collect_errors: bool,
    /// Follow the WHATWG standard where PHP 8.5 differs.
    pub standard: bool,
    pub errors: Vec<TokError>,

    tag: Tag,
    tag_name: Vec<u8>,
    attr_name: Vec<u8>,
    attr_value: Vec<u8>,
    attr_open: bool,
    comment: Vec<u8>,
    comment_start: usize,
    comment_data: usize,
    doctype: Doctype,
    dt_buf: Vec<u8>,
    temp: Vec<u8>,
    last_start_tag: Vec<u8>,

    text: Vec<u8>,
    text_first: usize,
    text_end: usize,
    /// A markup construct started after the pending text: the text is
    /// complete even if the input ends inside the construct.
    text_closed: bool,
    queue: std::collections::VecDeque<(Token, bool)>,
    eof_emitted: bool,
    /// Whether the token returned by `step` was produced by the end of the
    /// input.
    front_flag: bool,
    /// Whether the last token returned by [`Tokenizer::next_token`] was
    /// produced by the end of the input (PHP reports no tree errors for
    /// those, observed on PHP 8.5: `abc` alone gives no
    /// `unexpected-token-in-initial-mode`).
    pub last_at_eof: bool,
}

impl Tokenizer {
    pub fn new(input: String) -> Tokenizer {
        Tokenizer {
            input: input.into_bytes(),
            pos: 0,
            state: State::Data,
            return_state: State::Data,
            allow_cdata: false,
            collect_errors: true,
            standard: false,
            errors: Vec::new(),
            tag: Tag::default(),
            tag_name: Vec::new(),
            attr_name: Vec::new(),
            attr_value: Vec::new(),
            attr_open: false,
            comment: Vec::new(),
            comment_start: 0,
            comment_data: 0,
            doctype: Doctype::default(),
            dt_buf: Vec::new(),
            temp: Vec::new(),
            last_start_tag: Vec::new(),
            text: Vec::new(),
            text_first: 0,
            text_end: 0,
            text_closed: false,
            queue: std::collections::VecDeque::new(),
            eof_emitted: false,
            front_flag: false,
            last_at_eof: false,
        }
    }

    /// The (preprocessed) input.
    pub fn input(&self) -> &[u8] {
        &self.input
    }

    /// Records an error, except the ones PHP never reports (observed on PHP
    /// 8.5: end-of-file errors, `duplicate-attribute`,
    /// `end-tag-with-attributes`, `end-tag-with-trailing-solidus`,
    /// `cdata-in-html-content`, `unknown-named-character-reference`).
    #[inline]
    fn err(&mut self, code: &'static str, pos: usize) {
        if self.collect_errors
            && !code.starts_with("eof-")
            && !matches!(
                code,
                "duplicate-attribute"
                    | "end-tag-with-attributes"
                    | "end-tag-with-trailing-solidus"
                    | "cdata-in-html-content"
                    | "unknown-named-character-reference"
            )
        {
            self.errors.push(TokError { code, pos });
        }
    }

    #[inline]
    fn cur(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    // ----- text accumulation ------------------------------------------------

    #[inline]
    fn text_mark(&mut self, first: usize, end: usize) {
        if self.text.is_empty() {
            self.text_first = first;
            self.text_closed = false;
        }
        self.text_end = end;
    }

    /// Emits the input bytes `[a, b)` as character tokens.
    #[inline]
    fn emit_input(&mut self, a: usize, b: usize) {
        if a == b {
            return;
        }
        self.text_mark(a, b);
        let (input, text) = (&self.input, &mut self.text);
        text.extend_from_slice(&input[a..b]);
    }

    /// Emits a string as character tokens attributed to the source range
    /// `first..=last`.
    #[inline]
    fn emit_str(&mut self, s: &[u8], first: usize, last: usize) {
        self.text_mark(first, last + 1);
        self.text.extend_from_slice(s);
    }

    fn take_text(&mut self) -> Option<Token> {
        if self.text.is_empty() {
            return None;
        }
        let text = bytes_to_string(std::mem::take(&mut self.text));
        Some(Token::Text {
            text,
            first: self.text_first,
            end: self.text_end,
        })
    }

    /// Emits a non-character token (flushing pending text first).
    fn emit(&mut self, t: Token) -> Token {
        self.front_flag = false;
        match self.take_text() {
            Some(text) => {
                self.queue.push_back((t, false));
                text
            }
            None => t,
        }
    }

    /// Ends the input: emits the pending text, then `t` (an unfinished
    /// comment or doctype), then EOF.
    fn finish(&mut self, t: Option<Token>) -> Token {
        self.eof_emitted = true;
        if let Some(text) = self.take_text() {
            let closed = self.text_closed;
            self.queue.push_back((text, !closed));
        }
        if let Some(t) = t {
            self.queue.push_back((t, true));
        }
        self.queue.push_back((
            Token::Eof {
                pos: self.input.len(),
            },
            true,
        ));
        let (t, f) = self.queue.pop_front().unwrap();
        self.front_flag = f;
        t
    }

    /// Marks the start of a markup construct (the pending text is complete).
    #[inline]
    fn close_text(&mut self) {
        self.text_closed = true;
    }

    // ----- tag helpers --------------------------------------------------------

    fn new_tag(&mut self, end: bool, lt: usize) {
        self.tag = Tag {
            end,
            start: lt,
            name_first: self.pos,
            name_end: self.pos,
            ..Tag::default()
        };
        self.tag_name.clear();
        self.attr_open = false;
        self.close_text();
    }

    fn start_attr(&mut self) {
        self.finish_attr();
        self.attr_name.clear();
        self.attr_value.clear();
        self.attr_open = true;
    }

    fn finish_attr(&mut self) {
        if !self.attr_open {
            return;
        }
        self.attr_open = false;
        let name = bytes_to_string(std::mem::take(&mut self.attr_name));
        let value = bytes_to_string(std::mem::take(&mut self.attr_value));
        if self.tag.attrs.iter().any(|a| a.name == name) {
            let p = self.pos;
            self.err("duplicate-attribute", p);
            return;
        }
        self.tag.attrs.push(Attr { name, value });
    }

    fn emit_tag(&mut self) -> Token {
        self.finish_attr();
        let mut tag = std::mem::take(&mut self.tag);
        tag.name = bytes_to_string(std::mem::take(&mut self.tag_name));
        if tag.end {
            if !tag.attrs.is_empty() {
                let p = self.pos.saturating_sub(1);
                self.err("end-tag-with-attributes", p);
            }
            if tag.self_closing {
                let p = self.pos.saturating_sub(1);
                self.err("end-tag-with-trailing-solidus", p);
            }
        } else {
            self.last_start_tag.clear();
            self.last_start_tag.extend_from_slice(tag.name.as_bytes());
        }
        self.emit(Token::Tag(tag))
    }

    fn is_appropriate_end_tag(&self) -> bool {
        !self.last_start_tag.is_empty() && self.tag_name == self.last_start_tag
    }

    fn comment_token(&mut self) -> Token {
        let text = bytes_to_string(std::mem::take(&mut self.comment));
        Token::Comment {
            text,
            start: self.comment_start,
            data: self.comment_data,
        }
    }

    fn emit_comment(&mut self) -> Token {
        let t = self.comment_token();
        self.emit(t)
    }

    /// Starts a comment token whose data begins at the current position.
    fn begin_comment(&mut self, lt: usize) {
        self.comment.clear();
        self.comment_start = lt;
        self.comment_data = self.pos;
        self.close_text();
    }

    fn emit_doctype(&mut self) -> Token {
        let d = std::mem::take(&mut self.doctype);
        self.emit(Token::Doctype(Box::new(d)))
    }

    fn emit_eof(&mut self) -> Token {
        self.finish(None)
    }

    /// True when the next bytes match `s` ASCII case-insensitively.
    fn lookahead_ci(&self, s: &[u8]) -> bool {
        self.input.len() >= self.pos + s.len()
            && self.input[self.pos..self.pos + s.len()].eq_ignore_ascii_case(s)
    }

    // ----- character references ----------------------------------------------

    fn in_attr(&self) -> bool {
        matches!(
            self.return_state,
            State::AttrValueDq | State::AttrValueSq | State::AttrValueUnq
        )
    }

    fn flush_ref(&mut self, s: &[u8], first: usize, last: usize) {
        if self.in_attr() {
            self.attr_value.extend_from_slice(s);
        } else {
            self.emit_str(s, first, last);
        }
    }

    /// Handles a character reference; `self.pos` is just past the `&`.
    /// Leaves `self.pos` after the consumed characters (the state remains the
    /// return state).
    fn char_ref(&mut self) {
        let amp = self.pos - 1;
        match self.cur() {
            Some(c) if c.is_ascii_alphanumeric() => self.named_ref(amp),
            Some(b'#') => self.numeric_ref(amp),
            _ => self.flush_ref(b"&", amp, amp),
        }
    }

    fn named_ref(&mut self, amp: usize) {
        let rest = &self.input[self.pos..];
        // PHP scans as far as the characters still begin some reference
        // name, and uses the character there for the attribute rule and
        // for the position of the error (observed on PHP 8.5: `&notit;` is
        // reported at the second `t`; `a='&noti'` gives "¬i").
        let scan_end = self.pos + prefix_scan_len(rest);
        match match_named_ref(rest) {
            Some((len, value)) => {
                let last_semicolon = rest[len - 1] == b';';
                let end = self.pos + len;
                let next = self.input.get(scan_end).copied();
                if self.in_attr()
                    && !last_semicolon
                    && next.is_some_and(|c| c == b'=' || c.is_ascii_alphanumeric())
                {
                    // Historical: left as is in attributes.
                    let s = self.input[amp..end].to_vec();
                    self.flush_ref(&s, amp, end - 1);
                    self.pos = end;
                    return;
                }
                if !last_semicolon && scan_end < self.input.len() {
                    self.err("missing-semicolon-after-character-reference", scan_end);
                }
                self.flush_ref(value.as_bytes(), amp, end - 1);
                self.pos = end;
            }
            None => {
                // Ambiguous ampersand: the alphanumerics are literal.
                let mut n = 0;
                while rest.get(n).is_some_and(|c| c.is_ascii_alphanumeric()) {
                    n += 1;
                }
                let end = self.pos + n;
                if self.input.get(end) == Some(&b';') {
                    self.err("unknown-named-character-reference", end);
                }
                let s = self.input[amp..end].to_vec();
                self.flush_ref(&s, amp, end - 1);
                self.pos = end;
            }
        }
    }

    fn numeric_ref(&mut self, amp: usize) {
        // self.pos at '#'
        let mut p = self.pos + 1;
        let hex = matches!(self.input.get(p), Some(b'x' | b'X'));
        if hex {
            p += 1;
        }
        let digits_start = p;
        let mut code: u32 = 0;
        while let Some(&c) = self.input.get(p) {
            let d = if hex {
                match c {
                    b'0'..=b'9' => c - b'0',
                    b'a'..=b'f' => c - b'a' + 10,
                    b'A'..=b'F' => c - b'A' + 10,
                    _ => break,
                }
            } else {
                match c {
                    b'0'..=b'9' => c - b'0',
                    _ => break,
                }
            } as u32;
            code = code
                .saturating_mul(if hex { 16 } else { 10 })
                .saturating_add(d);
            if code > 0x10FFFF {
                code = 0x110000;
            }
            p += 1;
        }
        // Errors are reported at the character after the ampersand, and
        // not at all when the reference runs into the end of the input;
        // a missing semicolon is not reported (observed on PHP 8.5).
        let epos = amp + 1;
        let at_end = p >= self.input.len();
        if p == digits_start {
            if !at_end {
                self.err("absence-of-digits-in-numeric-character-reference", p);
            }
            let s = self.input[amp..p].to_vec();
            self.flush_ref(&s, amp, p - 1);
            self.pos = p;
            return;
        }
        if self.input.get(p) == Some(&b';') {
            p += 1;
        }
        let collect = self.collect_errors;
        if at_end {
            self.collect_errors = false;
        }
        let mut cp = code;
        if code == 0 {
            self.err("null-character-reference", epos);
            cp = 0xFFFD;
        } else if code > 0x10FFFF {
            self.err("character-reference-outside-unicode-range", epos);
            cp = 0xFFFD;
        } else if (0xD800..=0xDFFF).contains(&code) {
            self.err("surrogate-character-reference", epos);
            cp = 0xFFFD;
        } else if (0xFDD0..=0xFDEF).contains(&code) || (code & 0xFFFE) == 0xFFFE {
            self.err("noncharacter-character-reference", epos);
        } else if code == 0x0D
            || ((code < 0x20 || (0x7F..=0x9F).contains(&code))
                && !matches!(code, 0x09 | 0x0A | 0x0C | 0x20))
        {
            self.err("control-character-reference", epos);
            if let Some(r) = c1_replacement(code) {
                cp = r;
            }
        }
        self.collect_errors = collect;
        let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf).as_bytes().to_vec();
        self.flush_ref(&s, amp, p - 1);
        self.pos = p;
    }

    // ----- the state machine ----------------------------------------------------

    /// Returns the next token. After the EOF token, keeps returning EOF.
    pub fn next_token(&mut self) -> Token {
        if let Some((t, f)) = self.queue.pop_front() {
            self.last_at_eof = f;
            return t;
        }
        if self.eof_emitted {
            self.last_at_eof = true;
            return Token::Eof {
                pos: self.input.len(),
            };
        }
        loop {
            if let Some(t) = self.step() {
                self.last_at_eof = self.front_flag;
                return t;
            }
        }
    }

    /// Runs the machine until a token is complete.
    fn step(&mut self) -> Option<Token> {
        use State::*;
        match self.state {
            Data => {
                let start = self.pos;
                match memchr::memchr3(b'<', b'&', 0, &self.input[start..]) {
                    None => {
                        let end = self.input.len();
                        self.emit_input(start, end);
                        self.pos = end;
                        return Some(self.emit_eof());
                    }
                    Some(off) => {
                        let at = start + off;
                        self.emit_input(start, at);
                        self.pos = at + 1;
                        match self.input[at] {
                            b'<' => self.state = TagOpen,
                            b'&' => {
                                self.return_state = Data;
                                self.char_ref();
                            }
                            _ => {
                                self.err("unexpected-null-character", at);
                                // The NUL is emitted; the tree builder drops it.
                                self.emit_input(at, at + 1);
                            }
                        }
                    }
                }
            }
            RcData | RawText | ScriptData | PlainText => {
                let st = self.state;
                let start = self.pos;
                let found = match st {
                    RcData => memchr::memchr3(b'<', b'&', 0, &self.input[start..]),
                    PlainText => memchr::memchr(0, &self.input[start..]),
                    _ => memchr::memchr2(b'<', 0, &self.input[start..]),
                };
                match found {
                    None => {
                        let end = self.input.len();
                        self.emit_input(start, end);
                        self.pos = end;
                        return Some(self.emit_eof());
                    }
                    Some(off) => {
                        let at = start + off;
                        self.emit_input(start, at);
                        self.pos = at + 1;
                        match self.input[at] {
                            b'<' => {
                                self.state = match st {
                                    RcData => RcDataLt,
                                    RawText => RawTextLt,
                                    _ => ScriptDataLt,
                                }
                            }
                            b'&' => {
                                self.return_state = RcData;
                                self.char_ref();
                            }
                            _ => {
                                self.err("unexpected-null-character", at);
                                self.emit_str("\u{FFFD}".as_bytes(), at, at);
                            }
                        }
                    }
                }
            }
            TagOpen => {
                let lt = self.pos - 1;
                match self.cur() {
                    Some(b'!') => {
                        self.pos += 1;
                        self.state = MarkupDeclOpen;
                    }
                    Some(b'/') => {
                        self.pos += 1;
                        self.state = EndTagOpen;
                    }
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.new_tag(false, lt);
                        self.state = TagName;
                    }
                    Some(b'?') => {
                        self.err("unexpected-question-mark-instead-of-tag-name", self.pos);
                        self.begin_comment(lt);
                        self.state = BogusComment;
                    }
                    None => {
                        self.err("eof-before-tag-name", self.pos);
                        self.emit_str(b"<", lt, lt);
                        return Some(self.emit_eof());
                    }
                    Some(_) => {
                        self.err("invalid-first-character-of-tag-name", self.pos);
                        self.emit_str(b"<", lt, lt);
                        self.state = Data;
                    }
                }
            }
            EndTagOpen => {
                let lt = self.pos - 2;
                match self.cur() {
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.new_tag(true, lt);
                        self.state = TagName;
                    }
                    Some(b'>') => {
                        self.err("missing-end-tag-name", self.pos);
                        self.pos += 1;
                        // The ignored characters still belong to the
                        // surrounding text span.
                        if !self.text.is_empty() {
                            self.text_end = self.pos;
                        }
                        self.state = Data;
                    }
                    None => {
                        self.err("eof-before-tag-name", self.pos);
                        self.emit_str(b"</", lt, lt + 1);
                        return Some(self.emit_eof());
                    }
                    Some(_) => {
                        self.err("invalid-first-character-of-tag-name", self.pos);
                        self.begin_comment(lt);
                        self.state = BogusComment;
                    }
                }
            }
            TagName => {
                // Fast path over the name.
                let start = self.pos;
                let mut p = start;
                let n = self.input.len();
                while p < n {
                    let c = self.input[p];
                    if is_ws(c) || c == b'/' || c == b'>' || c == 0 {
                        break;
                    }
                    p += 1;
                }
                for &c in &self.input[start..p] {
                    self.tag_name.push(c.to_ascii_lowercase());
                }
                if p > start {
                    self.tag.name_end = p;
                }
                self.pos = p;
                match self.cur() {
                    None => {
                        self.err("eof-in-tag", self.pos);
                        return Some(self.emit_eof());
                    }
                    Some(c) => {
                        self.pos += 1;
                        match c {
                            b'/' => self.state = SelfClosingStartTag,
                            b'>' => {
                                self.state = Data;
                                return Some(self.emit_tag());
                            }
                            0 => {
                                self.err("unexpected-null-character", self.pos - 1);
                                self.tag_name.extend_from_slice("\u{FFFD}".as_bytes());
                                self.tag.name_end = self.pos;
                            }
                            _ => self.state = BeforeAttrName,
                        }
                    }
                }
            }
            RcDataLt | RawTextLt | ScriptDataLt => {
                let st = self.state;
                let lt = self.pos - 1;
                match self.cur() {
                    Some(b'/') => {
                        self.pos += 1;
                        self.temp.clear();
                        self.state = match st {
                            RcDataLt => RcDataEndTagOpen,
                            RawTextLt => RawTextEndTagOpen,
                            _ => ScriptDataEndTagOpen,
                        };
                    }
                    Some(b'!') if st == ScriptDataLt => {
                        self.pos += 1;
                        self.emit_str(b"<!", lt, lt + 1);
                        self.state = ScriptDataEscapeStart;
                    }
                    _ => {
                        self.emit_str(b"<", lt, lt);
                        self.state = match st {
                            RcDataLt => RcData,
                            RawTextLt => RawText,
                            _ => ScriptData,
                        };
                    }
                }
            }
            RcDataEndTagOpen
            | RawTextEndTagOpen
            | ScriptDataEndTagOpen
            | ScriptDataEscapedEndTagOpen => {
                let st = self.state;
                let lt = self.pos - 2;
                match self.cur() {
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.new_tag(true, lt);
                        self.state = match st {
                            RcDataEndTagOpen => RcDataEndTagName,
                            RawTextEndTagOpen => RawTextEndTagName,
                            ScriptDataEndTagOpen => ScriptDataEndTagName,
                            _ => ScriptDataEscapedEndTagName,
                        };
                    }
                    _ => {
                        self.emit_str(b"</", lt, lt + 1);
                        self.state = match st {
                            RcDataEndTagOpen => RcData,
                            RawTextEndTagOpen => RawText,
                            ScriptDataEndTagOpen => ScriptData,
                            _ => ScriptDataEscaped,
                        };
                    }
                }
            }
            RcDataEndTagName
            | RawTextEndTagName
            | ScriptDataEndTagName
            | ScriptDataEscapedEndTagName => {
                let st = self.state;
                let back = match st {
                    RcDataEndTagName => RcData,
                    RawTextEndTagName => RawText,
                    ScriptDataEndTagName => ScriptData,
                    _ => ScriptDataEscaped,
                };
                let c = self.cur();
                match c {
                    Some(c)
                        if (is_ws(c) || c == b'/' || c == b'>')
                            && self.is_appropriate_end_tag() =>
                    {
                        self.pos += 1;
                        match c {
                            b'/' => self.state = SelfClosingStartTag,
                            b'>' => {
                                self.state = Data;
                                return Some(self.emit_tag());
                            }
                            _ => self.state = BeforeAttrName,
                        }
                    }
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.tag_name.push(c.to_ascii_lowercase());
                        self.temp.push(c);
                        self.tag.name_end = self.pos + 1;
                        self.pos += 1;
                    }
                    _ => {
                        let lt = self.tag.start;
                        let mut s = b"</".to_vec();
                        s.extend_from_slice(&self.temp);
                        let last = self.pos.saturating_sub(1);
                        self.emit_str(&s, lt, last);
                        self.state = back;
                    }
                }
            }
            ScriptDataEscapeStart | ScriptDataEscapeStartDash => {
                let st = self.state;
                match self.cur() {
                    Some(b'-') => {
                        let p = self.pos;
                        self.pos += 1;
                        self.emit_str(b"-", p, p);
                        self.state = if st == ScriptDataEscapeStart {
                            ScriptDataEscapeStartDash
                        } else {
                            ScriptDataEscapedDashDash
                        };
                    }
                    _ => self.state = ScriptData,
                }
            }
            ScriptDataEscaped | ScriptDataDoubleEscaped => {
                let st = self.state;
                let start = self.pos;
                match memchr::memchr3(b'-', b'<', 0, &self.input[start..]) {
                    None => {
                        let end = self.input.len();
                        self.emit_input(start, end);
                        self.pos = end;
                        self.err("eof-in-script-html-comment-like-text", end);
                        return Some(self.emit_eof());
                    }
                    Some(off) => {
                        let at = start + off;
                        self.emit_input(start, at);
                        self.pos = at + 1;
                        match self.input[at] {
                            b'-' => {
                                self.emit_str(b"-", at, at);
                                self.state = if st == ScriptDataEscaped {
                                    ScriptDataEscapedDash
                                } else {
                                    ScriptDataDoubleEscapedDash
                                };
                            }
                            b'<' => {
                                if st == ScriptDataEscaped {
                                    self.state = ScriptDataEscapedLt;
                                } else {
                                    self.emit_str(b"<", at, at);
                                    self.state = ScriptDataDoubleEscapedLt;
                                }
                            }
                            _ => {
                                self.err("unexpected-null-character", at);
                                self.emit_str("\u{FFFD}".as_bytes(), at, at);
                            }
                        }
                    }
                }
            }
            ScriptDataEscapedDash
            | ScriptDataEscapedDashDash
            | ScriptDataDoubleEscapedDash
            | ScriptDataDoubleEscapedDashDash => {
                let st = self.state;
                let double = matches!(
                    st,
                    ScriptDataDoubleEscapedDash | ScriptDataDoubleEscapedDashDash
                );
                let dashdash = matches!(
                    st,
                    ScriptDataEscapedDashDash | ScriptDataDoubleEscapedDashDash
                );
                let base = if double {
                    ScriptDataDoubleEscaped
                } else {
                    ScriptDataEscaped
                };
                let p = self.pos;
                match self.cur() {
                    None => {
                        self.err("eof-in-script-html-comment-like-text", p);
                        return Some(self.emit_eof());
                    }
                    Some(c) => {
                        self.pos += 1;
                        match c {
                            b'-' => {
                                self.emit_str(b"-", p, p);
                                self.state = if double {
                                    ScriptDataDoubleEscapedDashDash
                                } else {
                                    ScriptDataEscapedDashDash
                                };
                            }
                            b'<' => {
                                if double {
                                    self.emit_str(b"<", p, p);
                                    self.state = ScriptDataDoubleEscapedLt;
                                } else {
                                    self.state = ScriptDataEscapedLt;
                                }
                            }
                            b'>' if dashdash => {
                                self.emit_str(b">", p, p);
                                self.state = ScriptData;
                            }
                            0 => {
                                self.err("unexpected-null-character", p);
                                self.emit_str("\u{FFFD}".as_bytes(), p, p);
                                self.state = base;
                            }
                            _ => {
                                self.emit_input(p, p + 1);
                                self.state = base;
                            }
                        }
                    }
                }
            }
            ScriptDataEscapedLt => {
                let lt = self.pos - 1;
                match self.cur() {
                    Some(b'/') => {
                        self.pos += 1;
                        self.temp.clear();
                        self.state = ScriptDataEscapedEndTagOpen;
                    }
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.temp.clear();
                        self.emit_str(b"<", lt, lt);
                        self.state = ScriptDataDoubleEscapeStart;
                    }
                    _ => {
                        self.emit_str(b"<", lt, lt);
                        self.state = ScriptDataEscaped;
                    }
                }
            }
            ScriptDataDoubleEscapeStart | ScriptDataDoubleEscapeEnd => {
                let st = self.state;
                let p = self.pos;
                match self.cur() {
                    Some(c) if is_ws(c) || c == b'/' || c == b'>' => {
                        self.pos += 1;
                        self.emit_input(p, p + 1);
                        let is_script = self.temp == b"script";
                        self.state = if st == ScriptDataDoubleEscapeStart {
                            if is_script {
                                ScriptDataDoubleEscaped
                            } else {
                                ScriptDataEscaped
                            }
                        } else if is_script {
                            ScriptDataEscaped
                        } else {
                            ScriptDataDoubleEscaped
                        };
                    }
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.pos += 1;
                        self.temp.push(c.to_ascii_lowercase());
                        self.emit_input(p, p + 1);
                    }
                    _ => {
                        self.state = if st == ScriptDataDoubleEscapeStart {
                            ScriptDataEscaped
                        } else {
                            ScriptDataDoubleEscaped
                        };
                    }
                }
            }
            ScriptDataDoubleEscapedLt => match self.cur() {
                Some(b'/') => {
                    let p = self.pos;
                    self.pos += 1;
                    self.temp.clear();
                    self.emit_str(b"/", p, p);
                    self.state = ScriptDataDoubleEscapeEnd;
                }
                _ => self.state = ScriptDataDoubleEscaped,
            },
            BeforeAttrName => {
                while self.cur().is_some_and(is_ws) {
                    self.pos += 1;
                }
                match self.cur() {
                    Some(b'/') | Some(b'>') | None => self.state = AfterAttrName,
                    Some(b'=') => {
                        self.err("unexpected-equals-sign-before-attribute-name", self.pos);
                        self.start_attr();
                        self.attr_name.push(b'=');
                        self.pos += 1;
                        self.state = AttrName;
                    }
                    Some(_) => {
                        self.start_attr();
                        self.state = AttrName;
                    }
                }
            }
            AttrName => {
                let n = self.input.len();
                loop {
                    if self.pos >= n {
                        self.state = AfterAttrName;
                        break;
                    }
                    let c = self.input[self.pos];
                    match c {
                        b'\t' | b'\n' | 0x0C | b' ' | b'/' | b'>' => {
                            self.state = AfterAttrName;
                            break;
                        }
                        b'=' => {
                            self.pos += 1;
                            self.state = BeforeAttrValue;
                            break;
                        }
                        0 => {
                            self.err("unexpected-null-character", self.pos);
                            self.attr_name.extend_from_slice("\u{FFFD}".as_bytes());
                        }
                        b'"' | b'\'' | b'<' => {
                            self.err("unexpected-character-in-attribute-name", self.pos);
                            self.attr_name.push(c);
                        }
                        _ => self.attr_name.push(c.to_ascii_lowercase()),
                    }
                    self.pos += 1;
                }
            }
            AfterAttrName => {
                while self.cur().is_some_and(is_ws) {
                    self.pos += 1;
                }
                match self.cur() {
                    None => {
                        self.err("eof-in-tag", self.pos);
                        return Some(self.emit_eof());
                    }
                    Some(b'/') => {
                        self.pos += 1;
                        self.state = SelfClosingStartTag;
                    }
                    Some(b'=') => {
                        self.pos += 1;
                        self.state = BeforeAttrValue;
                    }
                    Some(b'>') => {
                        self.pos += 1;
                        self.state = Data;
                        return Some(self.emit_tag());
                    }
                    Some(_) => {
                        self.start_attr();
                        self.state = AttrName;
                    }
                }
            }
            BeforeAttrValue => {
                while self.cur().is_some_and(is_ws) {
                    self.pos += 1;
                }
                match self.cur() {
                    Some(b'"') => {
                        self.pos += 1;
                        self.state = AttrValueDq;
                    }
                    Some(b'\'') => {
                        self.pos += 1;
                        self.state = AttrValueSq;
                    }
                    Some(b'>') => {
                        self.err("missing-attribute-value", self.pos);
                        self.pos += 1;
                        self.state = Data;
                        return Some(self.emit_tag());
                    }
                    _ => self.state = AttrValueUnq,
                }
            }
            AttrValueDq | AttrValueSq => {
                let st = self.state;
                let q = if st == AttrValueDq { b'"' } else { b'\'' };
                let start = self.pos;
                match memchr::memchr3(q, b'&', 0, &self.input[start..]) {
                    None => {
                        let end = self.input.len();
                        let (input, v) = (&self.input, &mut self.attr_value);
                        v.extend_from_slice(&input[start..end]);
                        self.pos = end;
                        self.err("eof-in-tag", end);
                        return Some(self.emit_eof());
                    }
                    Some(off) => {
                        let at = start + off;
                        let (input, v) = (&self.input, &mut self.attr_value);
                        v.extend_from_slice(&input[start..at]);
                        self.pos = at + 1;
                        match self.input[at] {
                            b'&' => {
                                self.return_state = st;
                                self.char_ref();
                            }
                            0 => {
                                self.err("unexpected-null-character", at);
                                self.attr_value.extend_from_slice("\u{FFFD}".as_bytes());
                            }
                            _ => self.state = AfterAttrValueQuoted,
                        }
                    }
                }
            }
            AttrValueUnq => {
                let n = self.input.len();
                loop {
                    if self.pos >= n {
                        self.err("eof-in-tag", self.pos);
                        return Some(self.emit_eof());
                    }
                    let c = self.input[self.pos];
                    self.pos += 1;
                    match c {
                        b'\t' | b'\n' | 0x0C | b' ' => {
                            self.state = BeforeAttrName;
                            break;
                        }
                        b'&' => {
                            self.return_state = AttrValueUnq;
                            self.char_ref();
                        }
                        b'>' => {
                            self.state = Data;
                            return Some(self.emit_tag());
                        }
                        0 => {
                            self.err("unexpected-null-character", self.pos - 1);
                            self.attr_value.extend_from_slice("\u{FFFD}".as_bytes());
                        }
                        b'"' | b'\'' | b'<' | b'=' | b'`' => {
                            // Reported at the end of the tag name (observed
                            // on PHP 8.5).
                            let at = self.tag.name_end;
                            self.err("unexpected-character-in-unquoted-attribute-value", at);
                            self.attr_value.push(c);
                        }
                        _ => self.attr_value.push(c),
                    }
                }
            }
            AfterAttrValueQuoted => match self.cur() {
                Some(c) if is_ws(c) => {
                    self.pos += 1;
                    self.state = BeforeAttrName;
                }
                Some(b'/') => {
                    self.pos += 1;
                    self.state = SelfClosingStartTag;
                }
                Some(b'>') => {
                    self.pos += 1;
                    self.state = Data;
                    return Some(self.emit_tag());
                }
                None => {
                    self.err("eof-in-tag", self.pos);
                    return Some(self.emit_eof());
                }
                Some(_) => {
                    self.err("missing-whitespace-between-attributes", self.pos);
                    self.state = BeforeAttrName;
                }
            },
            SelfClosingStartTag => match self.cur() {
                Some(b'>') => {
                    self.pos += 1;
                    self.tag.self_closing = true;
                    self.state = Data;
                    return Some(self.emit_tag());
                }
                None => {
                    self.err("eof-in-tag", self.pos);
                    return Some(self.emit_eof());
                }
                Some(_) => {
                    self.err("unexpected-solidus-in-tag", self.pos);
                    self.state = BeforeAttrName;
                }
            },
            BogusComment => {
                let start = self.pos;
                match memchr::memchr2(b'>', 0, &self.input[start..]) {
                    None => {
                        let end = self.input.len();
                        let (input, c) = (&self.input, &mut self.comment);
                        c.extend_from_slice(&input[start..end]);
                        self.pos = end;
                        let t = self.comment_token();
                        return Some(self.finish(Some(t)));
                    }
                    Some(off) => {
                        let at = start + off;
                        let (input, c) = (&self.input, &mut self.comment);
                        c.extend_from_slice(&input[start..at]);
                        self.pos = at + 1;
                        if self.input[at] == b'>' {
                            self.state = Data;
                            return Some(self.emit_comment());
                        }
                        self.err("unexpected-null-character", at);
                        self.comment.extend_from_slice("\u{FFFD}".as_bytes());
                    }
                }
            }
            MarkupDeclOpen => {
                let lt = self.pos - 2;
                if self.input[self.pos..].starts_with(b"--") {
                    self.pos += 2;
                    self.begin_comment(lt);
                    self.state = CommentStart;
                } else if !self.standard
                    && self.pos + 2 > self.input.len()
                    && b"--".starts_with(&self.input[self.pos..])
                {
                    // `<!` or `<!-` at the end of the input: an empty
                    // comment, no error (observed on PHP 8.5).
                    self.pos = self.input.len();
                    self.begin_comment(lt);
                    let t = self.comment_token();
                    return Some(self.finish(Some(t)));
                } else if self.lookahead_ci(b"doctype") {
                    self.doctype = self::Doctype {
                        start: lt,
                        kw_first: self.pos,
                        kw_end: self.pos + 7,
                        ..self::Doctype::default()
                    };
                    self.pos += 7;
                    self.close_text();
                    self.state = Doctype;
                } else if self.input[self.pos..].starts_with(b"[CDATA[") {
                    self.pos += 7;
                    if self.allow_cdata {
                        self.state = CdataSection;
                    } else {
                        self.err("cdata-in-html-content", self.pos - 7);
                        self.begin_comment(lt);
                        self.comment_data = self.pos - 7;
                        self.comment.extend_from_slice(b"[CDATA[");
                        self.state = BogusComment;
                    }
                } else {
                    self.err("incorrectly-opened-comment", self.pos);
                    self.begin_comment(lt);
                    self.state = BogusComment;
                }
            }
            CommentStart => match self.cur() {
                Some(b'-') => {
                    self.pos += 1;
                    self.state = CommentStartDash;
                }
                Some(b'>') => {
                    self.err("abrupt-closing-of-empty-comment", self.pos);
                    self.pos += 1;
                    self.state = Data;
                    return Some(self.emit_comment());
                }
                _ => self.state = Comment,
            },
            CommentStartDash => match self.cur() {
                Some(b'-') => {
                    self.pos += 1;
                    self.state = CommentEnd;
                }
                Some(b'>') => {
                    self.err("abrupt-closing-of-empty-comment", self.pos);
                    self.pos += 1;
                    self.state = Data;
                    return Some(self.emit_comment());
                }
                None => {
                    // `<!---` at the end of the input gives the comment "-"
                    // (observed on PHP 8.5).
                    if !self.standard {
                        self.comment.push(b'-');
                    }
                    return Some(self.eof_in_comment());
                }
                Some(_) => {
                    self.comment.push(b'-');
                    self.state = Comment;
                }
            },
            Comment => {
                let start = self.pos;
                match memchr::memchr3(b'<', b'-', 0, &self.input[start..]) {
                    None => {
                        let end = self.input.len();
                        let (input, c) = (&self.input, &mut self.comment);
                        c.extend_from_slice(&input[start..end]);
                        self.pos = end;
                        return Some(self.eof_in_comment());
                    }
                    Some(off) => {
                        let at = start + off;
                        let (input, c) = (&self.input, &mut self.comment);
                        c.extend_from_slice(&input[start..at]);
                        self.pos = at + 1;
                        match self.input[at] {
                            b'<' => {
                                self.comment.push(b'<');
                                self.state = CommentLt;
                            }
                            b'-' => self.state = CommentEndDash,
                            _ => {
                                self.err("unexpected-null-character", at);
                                self.comment.extend_from_slice("\u{FFFD}".as_bytes());
                            }
                        }
                    }
                }
            }
            CommentLt => match self.cur() {
                Some(b'!') => {
                    self.pos += 1;
                    self.comment.push(b'!');
                    self.state = CommentLtBang;
                }
                Some(b'<') => {
                    self.pos += 1;
                    self.comment.push(b'<');
                }
                _ => self.state = Comment,
            },
            CommentLtBang => match self.cur() {
                Some(b'-') => {
                    self.pos += 1;
                    self.state = CommentLtBangDash;
                }
                _ => self.state = Comment,
            },
            CommentLtBangDash => match self.cur() {
                Some(b'-') => {
                    self.pos += 1;
                    self.state = CommentLtBangDashDash;
                }
                _ => self.state = CommentEndDash,
            },
            CommentLtBangDashDash => match self.cur() {
                Some(b'>') | None => self.state = CommentEnd,
                Some(_) => {
                    self.err("nested-comment", self.pos);
                    self.state = CommentEnd;
                }
            },
            CommentEndDash => match self.cur() {
                Some(b'-') => {
                    self.pos += 1;
                    self.state = CommentEnd;
                }
                None => return Some(self.eof_in_comment()),
                Some(_) => {
                    self.comment.push(b'-');
                    self.state = Comment;
                }
            },
            CommentEnd => match self.cur() {
                Some(b'>') => {
                    self.pos += 1;
                    self.state = Data;
                    return Some(self.emit_comment());
                }
                Some(b'!') => {
                    self.pos += 1;
                    self.state = CommentEndBang;
                }
                Some(b'-') => {
                    self.pos += 1;
                    self.comment.push(b'-');
                }
                None => return Some(self.eof_in_comment()),
                Some(_) => {
                    self.comment.extend_from_slice(b"--");
                    self.state = Comment;
                }
            },
            // PHP drops the "--!" of the spec in this state (observed on
            // PHP 8.5: `<!-- a --! b-->` gives the comment " a  b").
            CommentEndBang => match self.cur() {
                Some(b'-') => {
                    self.pos += 1;
                    if self.standard {
                        self.comment.extend_from_slice(b"--!");
                    }
                    self.state = CommentEndDash;
                }
                Some(b'>') => {
                    self.err("incorrectly-closed-comment", self.pos);
                    self.pos += 1;
                    self.state = Data;
                    return Some(self.emit_comment());
                }
                None => return Some(self.eof_in_comment()),
                Some(_) => {
                    if self.standard {
                        self.comment.extend_from_slice(b"--!");
                    }
                    self.state = Comment;
                }
            },
            Doctype => match self.cur() {
                Some(c) if is_ws(c) => {
                    self.pos += 1;
                    self.state = BeforeDoctypeName;
                }
                Some(b'>') => self.state = BeforeDoctypeName,
                None => {
                    self.err("eof-in-doctype", self.pos);
                    self.doctype.force_quirks = true;
                    return Some(self.doctype_then_eof());
                }
                Some(_) => {
                    self.err("missing-whitespace-before-doctype-name", self.pos);
                    self.state = BeforeDoctypeName;
                }
            },
            BeforeDoctypeName => {
                while self.cur().is_some_and(is_ws) {
                    self.pos += 1;
                }
                match self.cur() {
                    Some(b'>') => {
                        self.err("missing-doctype-name", self.pos);
                        self.pos += 1;
                        self.doctype.force_quirks = true;
                        self.state = Data;
                        return Some(self.emit_doctype());
                    }
                    None => {
                        self.err("eof-in-doctype", self.pos);
                        self.doctype.force_quirks = true;
                        return Some(self.doctype_then_eof());
                    }
                    Some(_) => {
                        self.dt_buf.clear();
                        self.state = DoctypeName;
                    }
                }
            }
            DoctypeName => loop {
                match self.cur() {
                    Some(c) if is_ws(c) => {
                        self.pos += 1;
                        self.set_dt_name();
                        self.state = AfterDoctypeName;
                        break;
                    }
                    Some(b'>') => {
                        self.pos += 1;
                        self.set_dt_name();
                        self.state = Data;
                        return Some(self.emit_doctype());
                    }
                    Some(0) => {
                        self.err("unexpected-null-character", self.pos);
                        self.pos += 1;
                        self.dt_buf.extend_from_slice("\u{FFFD}".as_bytes());
                    }
                    None => {
                        self.err("eof-in-doctype", self.pos);
                        self.set_dt_name();
                        self.doctype.force_quirks = true;
                        return Some(self.doctype_then_eof());
                    }
                    Some(c) => {
                        self.pos += 1;
                        self.dt_buf.push(c.to_ascii_lowercase());
                    }
                }
            },
            AfterDoctypeName => {
                while self.cur().is_some_and(is_ws) {
                    self.pos += 1;
                }
                match self.cur() {
                    Some(b'>') => {
                        self.pos += 1;
                        self.state = Data;
                        return Some(self.emit_doctype());
                    }
                    None => {
                        self.err("eof-in-doctype", self.pos);
                        self.doctype.force_quirks = true;
                        return Some(self.doctype_then_eof());
                    }
                    Some(_) => {
                        if self.lookahead_ci(b"public") {
                            self.pos += 6;
                            self.state = AfterDoctypePublicKw;
                        } else if self.lookahead_ci(b"system") {
                            self.pos += 6;
                            self.state = AfterDoctypeSystemKw;
                        } else {
                            // Not reported when the input ends before six
                            // characters (observed on PHP 8.5).
                            let rest = &self.input[self.pos..];
                            let partial = rest.len() < 6
                                && (b"public"[..rest.len()].eq_ignore_ascii_case(rest)
                                    || b"system"[..rest.len()].eq_ignore_ascii_case(rest));
                            if !partial {
                                self.err("invalid-character-sequence-after-doctype-name", self.pos);
                            }
                            self.doctype.force_quirks = true;
                            self.state = BogusDoctype;
                        }
                    }
                }
            }
            AfterDoctypePublicKw | AfterDoctypeSystemKw => {
                let public = self.state == AfterDoctypePublicKw;
                match self.cur() {
                    Some(c) if is_ws(c) => {
                        self.pos += 1;
                        self.state = if public {
                            BeforeDoctypePublicId
                        } else {
                            BeforeDoctypeSystemId
                        };
                    }
                    Some(q @ (b'"' | b'\'')) => {
                        self.err(
                            if public {
                                "missing-whitespace-after-doctype-public-keyword"
                            } else {
                                "missing-whitespace-after-doctype-system-keyword"
                            },
                            self.pos,
                        );
                        self.pos += 1;
                        self.begin_dt_id(public, q);
                    }
                    Some(b'>') => {
                        self.err(
                            if public {
                                "missing-doctype-public-identifier"
                            } else {
                                "missing-doctype-system-identifier"
                            },
                            self.pos,
                        );
                        self.pos += 1;
                        self.doctype.force_quirks = true;
                        self.state = Data;
                        return Some(self.emit_doctype());
                    }
                    None => {
                        self.err("eof-in-doctype", self.pos);
                        self.doctype.force_quirks = true;
                        return Some(self.doctype_then_eof());
                    }
                    Some(_) => {
                        self.err(
                            if public {
                                "missing-quote-before-doctype-public-identifier"
                            } else {
                                "missing-quote-before-doctype-system-identifier"
                            },
                            self.pos,
                        );
                        self.doctype.force_quirks = true;
                        self.state = BogusDoctype;
                    }
                }
            }
            BeforeDoctypePublicId | BeforeDoctypeSystemId => {
                let public = self.state == BeforeDoctypePublicId;
                while self.cur().is_some_and(is_ws) {
                    self.pos += 1;
                }
                match self.cur() {
                    Some(q @ (b'"' | b'\'')) => {
                        self.pos += 1;
                        self.begin_dt_id(public, q);
                    }
                    Some(b'>') => {
                        self.err(
                            if public {
                                "missing-doctype-public-identifier"
                            } else {
                                "missing-doctype-system-identifier"
                            },
                            self.pos,
                        );
                        self.pos += 1;
                        self.doctype.force_quirks = true;
                        self.state = Data;
                        return Some(self.emit_doctype());
                    }
                    None => {
                        self.err("eof-in-doctype", self.pos);
                        self.doctype.force_quirks = true;
                        return Some(self.doctype_then_eof());
                    }
                    Some(_) => {
                        self.err(
                            if public {
                                "missing-quote-before-doctype-public-identifier"
                            } else {
                                "missing-quote-before-doctype-system-identifier"
                            },
                            self.pos,
                        );
                        self.doctype.force_quirks = true;
                        self.state = BogusDoctype;
                    }
                }
            }
            DoctypePublicIdDq | DoctypePublicIdSq | DoctypeSystemIdDq | DoctypeSystemIdSq => {
                let st = self.state;
                let public = matches!(st, DoctypePublicIdDq | DoctypePublicIdSq);
                let q = if matches!(st, DoctypePublicIdDq | DoctypeSystemIdDq) {
                    b'"'
                } else {
                    b'\''
                };
                loop {
                    match self.cur() {
                        Some(c) if c == q => {
                            self.pos += 1;
                            self.set_dt_id(public);
                            self.state = if public {
                                AfterDoctypePublicId
                            } else {
                                AfterDoctypeSystemId
                            };
                            break;
                        }
                        Some(0) => {
                            self.err("unexpected-null-character", self.pos);
                            self.pos += 1;
                            self.dt_buf.extend_from_slice("\u{FFFD}".as_bytes());
                        }
                        Some(b'>') => {
                            self.err(
                                if public {
                                    "abrupt-doctype-public-identifier"
                                } else {
                                    "abrupt-doctype-system-identifier"
                                },
                                self.pos,
                            );
                            self.pos += 1;
                            self.set_dt_id(public);
                            self.doctype.force_quirks = true;
                            self.state = Data;
                            return Some(self.emit_doctype());
                        }
                        None => {
                            self.err("eof-in-doctype", self.pos);
                            self.set_dt_id(public);
                            self.doctype.force_quirks = true;
                            return Some(self.doctype_then_eof());
                        }
                        Some(c) => {
                            self.pos += 1;
                            self.dt_buf.push(c);
                        }
                    }
                }
            }
            AfterDoctypePublicId | BetweenDoctypePublicSystem => {
                let between = self.state == BetweenDoctypePublicSystem;
                if !between {
                    if let Some(c) = self.cur()
                        && is_ws(c)
                    {
                        self.pos += 1;
                        self.state = BetweenDoctypePublicSystem;
                        return None;
                    }
                } else {
                    while self.cur().is_some_and(is_ws) {
                        self.pos += 1;
                    }
                }
                match self.cur() {
                    Some(b'>') => {
                        self.pos += 1;
                        self.state = Data;
                        return Some(self.emit_doctype());
                    }
                    Some(q @ (b'"' | b'\'')) => {
                        if !between {
                            self.err(
                                "missing-whitespace-between-doctype-public-and-system-identifiers",
                                self.pos,
                            );
                        }
                        self.pos += 1;
                        self.begin_dt_id(false, q);
                    }
                    None => {
                        self.err("eof-in-doctype", self.pos);
                        self.doctype.force_quirks = true;
                        return Some(self.doctype_then_eof());
                    }
                    Some(_) => {
                        self.err("missing-quote-before-doctype-system-identifier", self.pos);
                        self.doctype.force_quirks = true;
                        self.state = BogusDoctype;
                    }
                }
            }
            AfterDoctypeSystemId => {
                while self.cur().is_some_and(is_ws) {
                    self.pos += 1;
                }
                match self.cur() {
                    Some(b'>') => {
                        self.pos += 1;
                        self.state = Data;
                        return Some(self.emit_doctype());
                    }
                    None => {
                        self.err("eof-in-doctype", self.pos);
                        self.doctype.force_quirks = true;
                        return Some(self.doctype_then_eof());
                    }
                    Some(_) => {
                        self.err(
                            "unexpected-character-after-doctype-system-identifier",
                            self.pos,
                        );
                        self.state = BogusDoctype;
                    }
                }
            }
            BogusDoctype => loop {
                match self.cur() {
                    Some(b'>') => {
                        self.pos += 1;
                        self.state = Data;
                        return Some(self.emit_doctype());
                    }
                    Some(0) => {
                        self.err("unexpected-null-character", self.pos);
                        self.pos += 1;
                    }
                    None => return Some(self.doctype_then_eof()),
                    Some(_) => self.pos += 1,
                }
            },
            CdataSection => {
                let start = self.pos;
                match memchr::memchr(b']', &self.input[start..]) {
                    None => {
                        let end = self.input.len();
                        self.emit_input(start, end);
                        self.pos = end;
                        self.err("eof-in-cdata", end);
                        return Some(self.emit_eof());
                    }
                    Some(off) => {
                        let at = start + off;
                        self.emit_input(start, at);
                        self.pos = at + 1;
                        self.state = CdataSectionBracket;
                    }
                }
            }
            CdataSectionBracket => {
                let p = self.pos - 1;
                match self.cur() {
                    Some(b']') => {
                        self.pos += 1;
                        self.state = CdataSectionEnd;
                    }
                    _ => {
                        self.emit_str(b"]", p, p);
                        self.state = CdataSection;
                    }
                }
            }
            CdataSectionEnd => {
                let p = self.pos - 2;
                match self.cur() {
                    Some(b']') => {
                        self.emit_str(b"]", p, p);
                        self.pos += 1;
                    }
                    Some(b'>') => {
                        self.pos += 1;
                        self.state = Data;
                        // The section's text is a token of its own.
                        if let Some(t) = self.take_text() {
                            self.front_flag = false;
                            return Some(t);
                        }
                    }
                    _ => {
                        self.emit_str(b"]]", p, p + 1);
                        self.state = CdataSection;
                    }
                }
            }
        }
        None
    }

    fn eof_in_comment(&mut self) -> Token {
        self.err("eof-in-comment", self.pos);
        let t = self.comment_token();
        self.finish(Some(t))
    }

    fn doctype_then_eof(&mut self) -> Token {
        let d = std::mem::take(&mut self.doctype);
        self.finish(Some(Token::Doctype(Box::new(d))))
    }

    fn set_dt_name(&mut self) {
        self.doctype.name = Some(bytes_to_string(std::mem::take(&mut self.dt_buf)));
    }

    fn begin_dt_id(&mut self, public: bool, q: u8) {
        self.dt_buf.clear();
        if public {
            self.doctype.public_id = Some(String::new());
        } else {
            self.doctype.system_id = Some(String::new());
        }
        self.state = match (public, q) {
            (true, b'"') => State::DoctypePublicIdDq,
            (true, _) => State::DoctypePublicIdSq,
            (false, b'"') => State::DoctypeSystemIdDq,
            (false, _) => State::DoctypeSystemIdSq,
        };
    }

    fn set_dt_id(&mut self, public: bool) {
        let v = Some(bytes_to_string(std::mem::take(&mut self.dt_buf)));
        if public {
            self.doctype.public_id = v;
        } else {
            self.doctype.system_id = v;
        }
    }
}
