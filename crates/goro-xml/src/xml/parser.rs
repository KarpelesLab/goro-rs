//! The XML 1.0 event parser.
//!
//! Written from the XML 1.0 (Fifth Edition) and Namespaces in XML 1.0
//! recommendations. Error codes and messages are those PHP 8.5 reports
//! through libxml2 (observed on PHP 8.5, fixtures `tests/fixtures/errors*`).
//!
//! Input is decoded to UTF-8 with normalized line ends before parsing. The
//! parser works one construct ("step") at a time; when fed in chunks
//! ([`Parser::feed`] with `is_final == false`) a construct cut by the end of
//! the available input is retried when more input arrives.

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

use super::{EntityLoader, LoadError, ParseOptions, Sink, opt};
use crate::chars::{is_name_char, is_name_start, is_space, is_xml_char};
use crate::error::{Domain, LEVEL_ERROR, LEVEL_FATAL, LEVEL_WARNING, XmlError};
use crate::tree::{
    AttrDefault, AttrType, ContentModel, DtdData, EntityDecl, EntityKind, NsDecl, QName, Str,
    XML_NS,
};

/// One piece of an attribute value as written: text, or a reference to a
/// declared general entity (kept unexpanded when entities are not
/// substituted, as libxml keeps them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttrPiece {
    /// Literal text (after character references and normalization).
    Text(String),
    /// `&name;`
    Entity(Str),
}

/// A parsed attribute.
#[derive(Clone, Debug)]
pub struct Attribute {
    /// Name and resolved namespace.
    pub name: QName,
    /// Normalized value with all references expanded.
    pub value: String,
    /// The pieces, when the value referenced declared entities that the
    /// tree should keep as entity references.
    pub pieces: Option<Vec<AttrPiece>>,
    /// Added from a DTD default.
    pub defaulted: bool,
    /// DTD-declared type.
    pub atype: AttrType,
    /// Is an ID (DTD type ID or `xml:id`).
    pub is_id: bool,
}

/// A start tag as reported to the sink.
#[derive(Debug)]
pub struct StartTag<'a> {
    /// The element name (with namespace when namespaces are processed).
    pub name: &'a QName,
    /// Attributes, in document order, then defaulted ones.
    pub attrs: &'a [Attribute],
    /// Namespace declarations on this element, in document order.
    pub ns_decls: &'a [NsDecl],
    /// Source line of the `<`.
    pub line: u32,
    /// `<a/>`
    pub empty: bool,
}

/// Result of [`Parser::feed`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Waiting for more input.
    NeedMore,
    /// The document was fully parsed and is well-formed.
    Done,
    /// Parsing stopped on a fatal error.
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Start,
    Prolog,
    Content,
    Epilog,
    Done,
}

/// How a construct ended.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Halt {
    /// A fatal error: stop.
    Fatal,
    /// The input ended inside the construct (chunked mode).
    Incomplete,
}

pub(crate) type R<T> = Result<T, Halt>;

struct Open {
    name: QName,
    raw: Str,
    line: u32,
    ns_mark: usize,
    preserve: bool,
    had_text: bool,
    had_child: bool,
}

/// The parser.
pub struct Parser<'l> {
    pub(crate) opts: ParseOptions,
    pub(crate) loader: Option<&'l mut dyn EntityLoader>,
    raw: Vec<u8>,
    decided: bool,
    pub(crate) buf: Vec<u8>,
    pub(crate) pos: usize,
    final_: bool,
    hit_eof: Cell<bool>,
    enc_error: bool,
    phase: Phase,
    stack: Vec<Open>,
    ns: Vec<(Option<Str>, Str)>,
    names: HashSet<Str>,
    /// The internal subset, once read.
    pub dtd: Option<DtdData>,
    /// The external subset, when loaded.
    pub ext: Option<DtdData>,
    pub(crate) standalone: i8,
    /// The declared version.
    pub version: Option<String>,
    /// The declared encoding.
    pub encoding: Option<String>,
    pub(crate) stopped: bool,
    pub(crate) halted: bool,
    pub(crate) errs: Vec<XmlError>,
    /// All errors reported so far.
    pub errors: Vec<XmlError>,
    line_pos: usize,
    line_no: u32,
    col_pos: usize,
    col_val: u32,
    entity_stack: Vec<Str>,
    pub(crate) err_pos: Option<(u32, u32)>,
    text: String,
    pub(crate) has_pe_refs: bool,
    max_depth: usize,
    root_seen: bool,
    expansions: usize,
    /// Byte offset in the decoded input of the construct being processed.
    pub event_offset: usize,
    fragment: bool,
    tag_end_lc: Option<usize>,
    name_end_lc: Option<usize>,
    /// Byte position errors are reported at (start-tag checks), resolved
    /// to a line and column only when an error is reported.
    err_at: Option<usize>,
    halt_lc: Option<(u32, u32)>,
    utf16_input: bool,
    pub(crate) no_subset_error: bool,
    pub(crate) pe_from_internal: bool,
    pub(crate) splices: usize,
    cr_positions: Vec<usize>,
    utf8_chunks: bool,
    final_pending: bool,
    cr_next: usize,
    text_begin: usize,
    tag_start: usize,
    /// Bytes before the decoded text (a byte order mark).
    pub bom_len: usize,
}

struct RawAttr {
    name: Str,
    value: String,
    pieces: Option<Vec<AttrPiece>>,
    name_lc: usize,
    value_lc: usize,
}

impl<'l> Parser<'l> {
    /// A parser with options and an optional loader for external resources.
    pub fn new(opts: ParseOptions, loader: Option<&'l mut dyn EntityLoader>) -> Parser<'l> {
        // ext/xml's SAX parsing has no depth limit (observed on PHP 8.5).
        let max_depth = if opts.sax {
            usize::MAX
        } else if opts.has(opt::PARSEHUGE) {
            2048
        } else {
            256
        };
        Parser {
            opts,
            loader,
            raw: Vec::new(),
            decided: false,
            buf: Vec::new(),
            pos: 0,
            final_: false,
            hit_eof: Cell::new(false),
            enc_error: false,
            phase: Phase::Start,
            stack: Vec::new(),
            ns: Vec::new(),
            names: HashSet::new(),
            dtd: None,
            ext: None,
            standalone: -1,
            version: None,
            encoding: None,
            stopped: false,
            halted: false,
            errs: Vec::new(),
            errors: Vec::new(),
            line_pos: 0,
            line_no: 1,
            col_pos: 0,
            col_val: 1,
            entity_stack: Vec::new(),
            err_pos: None,
            text: String::new(),
            has_pe_refs: false,
            max_depth,
            root_seen: false,
            expansions: 0,
            event_offset: 0,
            fragment: false,
            tag_end_lc: None,
            name_end_lc: None,
            err_at: None,
            halt_lc: None,
            utf16_input: false,
            no_subset_error: false,
            pe_from_internal: false,
            splices: 0,
            cr_positions: Vec::new(),
            utf8_chunks: false,
            final_pending: false,
            cr_next: 0,
            text_begin: 0,
            tag_start: 0,
            bom_len: 0,
        }
    }

    /// A parser for well-balanced content (`DOMDocumentFragment::appendXML`,
    /// SimpleXML/DOM fragment parsing): no prolog, any number of top-level
    /// nodes. Namespace bindings in scope can be added with [`Parser::push_ns`].
    pub fn new_fragment(
        opts: ParseOptions,
        loader: Option<&'l mut dyn EntityLoader>,
    ) -> Parser<'l> {
        let mut p = Parser::new(opts, loader);
        p.fragment = true;
        p.phase = Phase::Content;
        p.root_seen = true;
        p
    }

    /// Declares a namespace binding in scope before parsing (fragments).
    pub fn push_ns(&mut self, prefix: Option<&str>, uri: &str) {
        let p = prefix.map(|p| self.intern(p));
        let u = self.intern(uri);
        self.ns.push((p, u));
    }

    /// Gives the parser a DTD to resolve entity references against
    /// (fragments parsed in the context of a document).
    pub fn set_dtd(&mut self, dtd: Option<DtdData>, ext: Option<DtdData>) {
        self.dtd = dtd;
        self.ext = ext;
    }

    /// Parses a complete document.
    pub fn parse(&mut self, input: &[u8], sink: &mut dyn Sink) -> Status {
        self.feed(input, true, sink)
    }

    /// Feeds input. With `is_final == false` more input may follow.
    pub fn feed(&mut self, input: &[u8], is_final: bool, sink: &mut dyn Sink) -> Status {
        if self.phase == Phase::Done {
            return if self.stopped {
                Status::Failed
            } else {
                Status::Done
            };
        }
        self.raw.extend_from_slice(input);
        if !self.decided {
            let wait = if self.raw.starts_with(b"<?xml")
                || b"<?xml".starts_with(&self.raw[..self.raw.len().min(5)])
            {
                self.raw.len() < 1024 && memchr::memmem::find(&self.raw, b"?>").is_none()
            } else {
                self.raw.len() < 4
            };
            if !is_final && wait {
                return Status::NeedMore;
            }
            self.final_pending = false;
            if !self.decide_encoding(sink) {
                self.phase = Phase::Done;
                return Status::Failed;
            }
            if self.utf8_chunks {
                self.decode_more(is_final);
            }
        } else {
            self.decode_more(is_final);
        }
        self.final_ = is_final;
        self.run(sink)
    }

    /// Current line (1-based) at the parser position.
    pub fn current_line(&mut self) -> u32 {
        self.line_col(self.pos).0
    }
    /// Current column (1-based characters).
    pub fn current_column(&mut self) -> u32 {
        self.line_col(self.pos).1
    }
    /// Current byte offset in the decoded input.
    pub fn current_offset(&self) -> usize {
        self.pos
    }

    /// SAX mode: reports the position of offset `p` to the sink.
    fn emit_position(&mut self, sink: &mut dyn Sink, p: usize) {
        let (o, l, c) = self.orig_position(p);
        sink.position(o, l, c);
    }

    /// The position of decoded offset `p` in the original input: byte
    /// offset, line and column as PHP's libxml-based ext/xml counts them
    /// (CR LF is one line break, a lone CR none; observed on PHP 8.5).
    pub fn orig_position(&mut self, p: usize) -> (usize, u32, u32) {
        let (line, col) = self.line_col(p);
        if self.cr_positions.is_empty() {
            return (p + self.bom_len, line, col);
        }
        let idx = self.cr_positions.partition_point(|&e| (e >> 1) < p);
        let crlf = self.cr_positions[..idx]
            .iter()
            .filter(|&&e| e & 1 == 1)
            .count();
        let lone = idx - crlf;
        // Column: characters since the last line feed that was not a lone CR.
        let mut start = p.min(self.buf.len());
        let ls = loop {
            match memchr::memrchr(b'\n', &self.buf[..start]) {
                Some(i) => {
                    let lone_cr = self.cr_positions.binary_search(&(i << 1)).is_ok();
                    if lone_cr {
                        start = i;
                        continue;
                    }
                    break i + 1;
                }
                None => break 0,
            }
        };
        let pe = p.min(self.buf.len());
        let col = self.buf[ls..pe]
            .iter()
            .filter(|&&b| (b & 0xC0) != 0x80)
            .count() as u32
            + 1;
        (p + crlf + self.bom_len, line - lone as u32, col)
    }

    /// The current position as [`Parser::orig_position`] gives it, with
    /// pending character data not yet consumed.
    pub fn sax_position(&mut self) -> (usize, u32, u32) {
        let p = if self.text.is_empty() {
            self.pos
        } else {
            self.text_begin
        };
        self.orig_position(p)
    }

    /// The byte offset of a 1-based line and column in the decoded input.
    pub fn offset_of(&self, line: u32, column: u32) -> usize {
        let mut off = 0;
        for _ in 1..line {
            match memchr::memchr(b'\n', &self.buf[off..]) {
                Some(i) => off += i + 1,
                None => return self.buf.len(),
            }
        }
        let mut chars = 1;
        while chars < column && off < self.buf.len() {
            off += 1;
            while off < self.buf.len() && (self.buf[off] & 0xC0) == 0x80 {
                off += 1;
            }
            chars += 1;
        }
        off
    }

    fn decide_encoding(&mut self, sink: &mut dyn Sink) -> bool {
        self.decided = true;
        let raw = std::mem::take(&mut self.raw);
        let (sniffed, _bom) = crate::encoding::sniff(&raw);
        let mut enc = None;
        if let Some(name) = &self.opts.encoding_override {
            enc = crate::encoding::lookup(name);
        } else if let Some(e) = sniffed {
            enc = Some(e);
            // A UTF-16 document still has its declaration: fine.
        } else if let Some((name, off)) = crate::encoding::declared_encoding(&raw) {
            match crate::encoding::lookup(&name) {
                Some(e) => {
                    if !matches!(
                        e,
                        goro_charset::Encoding::Utf16
                            | goro_charset::Encoding::Utf16Le
                            | goro_charset::Encoding::Utf16Be
                            | goro_charset::Encoding::Ucs2
                            | goro_charset::Encoding::Ucs4
                    ) {
                        enc = Some(e);
                    }
                }
                None => {
                    // Position: the end of the declaration's encoding value.
                    let col = (off + name.len() + 2) as u32;
                    let e = XmlError {
                        domain: Domain::Parser,
                        code: 32,
                        level: LEVEL_FATAL,
                        message: format!("Unsupported encoding: {name}\n"),
                        file: self.opts.url.clone(),
                        line: 1,
                        column: col,
                    };
                    sink.error(&e);
                    self.errors.push(e);
                    self.stopped = true;
                    return false;
                }
            }
        }
        self.utf16_input = matches!(
            enc,
            Some(
                goro_charset::Encoding::Utf16Le
                    | goro_charset::Encoding::Utf16Be
                    | goro_charset::Encoding::Utf16
            )
        );
        if matches!(enc, None | Some(goro_charset::Encoding::Utf8)) && !self.final_pending {
            // UTF-8 can be decoded chunk by chunk.
            if raw.starts_with(&[0xEF, 0xBB, 0xBF]) {
                self.bom_len = 3;
            }
            self.raw = raw;
            self.utf8_chunks = true;
            return true;
        }
        let d = crate::encoding::decode(&raw, enc);
        if d.bom {
            self.bom_len = if raw.starts_with(&[0xEF, 0xBB, 0xBF]) {
                3
            } else {
                2
            };
        }
        if self.opts.sax {
            self.cr_positions = d.cr;
        }
        self.buf = d.text;
        self.enc_error = d.error_at.is_some();
        true
    }

    fn decode_more(&mut self, is_final: bool) {
        if self.raw.is_empty() {
            return;
        }
        // Chunked input: UTF-8 only (and ISO-8859-1 via the first chunk's
        // decision is rare); keep an incomplete trailing sequence or a
        // trailing CR for the next chunk.
        let raw = std::mem::take(&mut self.raw);
        let mut cut = raw.len();
        if !is_final {
            if raw.last() == Some(&b'\r') {
                cut -= 1;
            }
            let mut i = cut;
            while i > 0 && i + 4 > cut && (raw[i - 1] & 0xC0) == 0x80 {
                i -= 1;
            }
            if i > 0 && raw[i - 1] >= 0xC0 {
                let need = if raw[i - 1] >= 0xF0 {
                    4
                } else if raw[i - 1] >= 0xE0 {
                    3
                } else {
                    2
                };
                if cut - (i - 1) < need {
                    cut = i - 1;
                }
            }
        }
        let d = if self.buf.is_empty() || !raw.starts_with(&[0xEF, 0xBB, 0xBF]) {
            crate::encoding::decode(&raw[..cut], None)
        } else {
            // A BOM-like sequence inside the document is data.
            let mut d = crate::encoding::decode(&[b"x", &raw[..cut]].concat(), None);
            d.text.remove(0);
            for c in d.cr.iter_mut() {
                *c -= 2;
            }
            d
        };
        if self.opts.sax {
            let base = self.buf.len();
            self.cr_positions
                .extend(d.cr.iter().map(|p| (((p >> 1) + base) << 1) | (p & 1)));
        }
        self.buf.extend_from_slice(&d.text);
        if d.error_at.is_some() {
            self.enc_error = true;
        }
        self.raw = raw[cut..].to_vec();
    }

    // ----- low-level helpers ----------------------------------------------

    #[inline]
    pub(crate) fn at(&self, i: usize) -> u8 {
        match self.buf.get(i) {
            Some(&b) => b,
            None => {
                self.hit_eof.set(true);
                0
            }
        }
    }
    #[inline]
    pub(crate) fn cur(&self) -> u8 {
        self.at(self.pos)
    }
    #[inline]
    pub(crate) fn eof(&self) -> bool {
        if self.pos >= self.buf.len() {
            self.hit_eof.set(true);
            true
        } else {
            false
        }
    }
    pub(crate) fn starts_with(&self, s: &[u8]) -> bool {
        let end = self.pos + s.len();
        if end > self.buf.len() {
            self.hit_eof.set(true);
            return false;
        }
        &self.buf[self.pos..end] == s
    }
    pub(crate) fn incomplete(&self) -> bool {
        self.hit_eof.get() && !self.final_
    }
    pub(crate) fn skip_spaces(&mut self) -> bool {
        let start = self.pos;
        while self.pos < self.buf.len() && is_space(self.buf[self.pos]) {
            self.pos += 1;
        }
        if self.pos >= self.buf.len() {
            self.hit_eof.set(true);
        }
        self.pos > start
    }
    pub(crate) fn char_at(&self, i: usize) -> Option<char> {
        let b = self.at(i);
        if b < 0x80 {
            return if i < self.buf.len() {
                Some(b as char)
            } else {
                None
            };
        }
        let len = if b >= 0xF0 {
            4
        } else if b >= 0xE0 {
            3
        } else {
            2
        };
        if i + len > self.buf.len() {
            self.hit_eof.set(true);
            return None;
        }
        std::str::from_utf8(&self.buf[i..i + len])
            .ok()?
            .chars()
            .next()
    }

    /// Reads a `Name` at the position; returns the slice range.
    pub(crate) fn scan_name(&mut self) -> Option<(usize, usize)> {
        let start = self.pos;
        let c = self.char_at(self.pos)?;
        if !is_name_start(c) {
            return None;
        }
        self.pos += c.len_utf8();
        loop {
            let b = self.at(self.pos);
            if b < 0x80 {
                if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':') {
                    self.pos += 1;
                    continue;
                }
                break;
            }
            match self.char_at(self.pos) {
                Some(c) if is_name_char(c) => self.pos += c.len_utf8(),
                _ => break,
            }
        }
        Some((start, self.pos))
    }

    pub(crate) fn slice(&self, r: (usize, usize)) -> &str {
        // The buffer is valid UTF-8 and ranges are on char boundaries.
        unsafe { std::str::from_utf8_unchecked(&self.buf[r.0..r.1]) }
    }

    /// Interns the name at a buffer range.
    pub(crate) fn intern_range(&mut self, r: (usize, usize)) -> Str {
        let s = unsafe { std::str::from_utf8_unchecked(&self.buf[r.0..r.1]) };
        if let Some(x) = self.names.get(s) {
            return x.clone();
        }
        let rc: Str = Rc::from(s);
        self.names.insert(rc.clone());
        rc
    }

    pub(crate) fn intern(&mut self, s: &str) -> Str {
        if let Some(x) = self.names.get(s) {
            return x.clone();
        }
        let r: Str = Rc::from(s);
        self.names.insert(r.clone());
        r
    }

    /// Line and column (1-based, in characters) of a position, computed
    /// incrementally from the last position asked for.
    pub(crate) fn line_col(&mut self, pos: usize) -> (u32, u32) {
        let pos = self.advance_lines(pos);
        self.col_val += self.buf[self.col_pos..pos]
            .iter()
            .filter(|&&b| (b & 0xC0) != 0x80)
            .count() as u32;
        self.col_pos = pos;
        (self.line_no, self.col_val)
    }

    /// Counts lines up to `pos` (the column cache stays at a line start or
    /// an earlier position on the same line).
    fn advance_lines(&mut self, pos: usize) -> usize {
        let pos = pos.min(self.buf.len());
        if pos < self.line_pos || pos < self.col_pos {
            self.line_pos = 0;
            self.line_no = 1;
            self.col_pos = 0;
            self.col_val = 1;
        }
        let seg = &self.buf[self.line_pos..pos];
        if let Some(i) = memchr::memrchr(b'\n', seg) {
            self.line_no += memchr::memchr_iter(b'\n', seg).count() as u32;
            self.col_pos = self.line_pos + i + 1;
            self.col_val = 1;
        }
        self.line_pos = pos;
        pos
    }

    pub(crate) fn line(&mut self, pos: usize) -> u32 {
        if let Some((l, _)) = self.err_pos {
            return l;
        }
        self.advance_lines(pos);
        self.line_no
    }

    // ----- errors ----------------------------------------------------------

    pub(crate) fn report(&mut self, domain: Domain, code: u32, level: u8, msg: String) {
        let (line, column) = match (self.err_pos, self.halt_lc) {
            (Some(p), _) => p,
            (None, Some(p)) => p,
            (None, None) => self.line_col(self.err_at.unwrap_or(self.pos)),
        };
        let e = XmlError {
            domain,
            code,
            level,
            message: msg,
            file: self.opts.url.clone(),
            line,
            column,
        };
        let suppressed = (level == LEVEL_WARNING && self.opts.has(opt::NOWARNING))
            || (level >= LEVEL_ERROR && self.opts.has(opt::NOERROR));
        if !suppressed {
            self.errs.push(e);
        }
    }

    /// Records a fatal well-formedness error; parsing stops after the
    /// current construct.
    pub(crate) fn fatal(&mut self, code: u32, msg: impl Into<String>) -> Halt {
        let mut m = msg.into();
        m.push('\n');
        self.report(Domain::Parser, code, LEVEL_FATAL, m);
        self.stopped = true;
        Halt::Fatal
    }

    /// A fatal error that also discards the rest of the input.
    pub(crate) fn halt(&mut self, code: u32, msg: impl Into<String>) -> Halt {
        if self.err_pos.is_none() {
            self.halt_lc = Some(self.line_col(self.pos));
        }
        let h = self.fatal(code, msg);
        self.pos = self.buf.len();
        self.halted = true;
        h
    }

    pub(crate) fn warn(&mut self, domain: Domain, code: u32, msg: impl Into<String>) {
        let mut m = msg.into();
        m.push('\n');
        self.report(domain, code, LEVEL_WARNING, m);
    }

    pub(crate) fn error(&mut self, domain: Domain, code: u32, msg: impl Into<String>) {
        let mut m = msg.into();
        m.push('\n');
        self.report(domain, code, LEVEL_ERROR, m);
    }

    fn eof_error(&mut self) -> Halt {
        if self.enc_error {
            return self.halt(81, "Invalid bytes in character encoding");
        }
        if let Some(o) = self.stack.last() {
            let (n, l) = (o.raw.clone(), o.line);
            return self.fatal(77, format!("Premature end of data in tag {n} line {l}"));
        }
        self.fatal(4, "Start tag expected, '<' not found")
    }

    fn flush_errors(&mut self, sink: &mut dyn Sink) {
        for e in self.errs.drain(..) {
            sink.error(&e);
            self.errors.push(e);
        }
    }

    // ----- driver ----------------------------------------------------------

    fn run(&mut self, sink: &mut dyn Sink) -> Status {
        loop {
            if self.phase == Phase::Done {
                self.flush_errors(sink);
                return if self.stopped && !self.recover() {
                    Status::Failed
                } else {
                    Status::Done
                };
            }
            let save_pos = self.pos;
            let save_stopped = self.stopped;
            let nerr = self.errs.len();
            self.hit_eof.set(false);
            let r = self.step(sink);
            match r {
                Ok(()) => {
                    self.flush_errors(sink);
                    if self.stopped && !self.recover() {
                        self.phase = Phase::Done;
                    }
                }
                Err(Halt::Incomplete) => {
                    if self.final_ {
                        // Cannot happen in final mode; treat as fatal.
                        self.flush_errors(sink);
                        self.phase = Phase::Done;
                        continue;
                    }
                    self.pos = save_pos;
                    self.stopped = save_stopped;
                    self.errs.truncate(nerr);
                    return Status::NeedMore;
                }
                Err(Halt::Fatal) => {
                    if self.incomplete() && !self.halted {
                        self.pos = save_pos;
                        self.stopped = save_stopped;
                        self.errs.truncate(nerr);
                        return Status::NeedMore;
                    }
                    self.flush_errors(sink);
                    if self.recover() && !self.halted && self.phase == Phase::Content {
                        self.resync();
                        continue;
                    }
                    self.phase = Phase::Done;
                }
            }
        }
    }

    pub(crate) fn recover(&self) -> bool {
        self.opts.has(opt::RECOVER)
    }

    fn resync(&mut self) {
        // Skip to the next markup.
        if self.pos < self.buf.len() {
            self.pos += 1;
        }
        while self.pos < self.buf.len() && self.buf[self.pos] != b'<' {
            self.pos += 1;
        }
    }

    fn step(&mut self, sink: &mut dyn Sink) -> R<()> {
        match self.phase {
            Phase::Start => {
                if self.starts_with(b"<?xml") && is_space(self.at(self.pos + 5)) {
                    self.parse_xml_decl(sink)?;
                } else if self.incomplete() && b"<?xml ".starts_with(&self.buf[self.pos..]) {
                    return Err(Halt::Incomplete);
                }
                self.phase = Phase::Prolog;
                Ok(())
            }
            Phase::Prolog | Phase::Epilog => {
                self.skip_spaces();
                if self.eof() {
                    if !self.final_ {
                        return Err(Halt::Incomplete);
                    }
                    if self.phase == Phase::Prolog {
                        return Err(self.eof_error());
                    }
                    if self.enc_error {
                        return Err(self.halt(81, "Invalid bytes in character encoding"));
                    }
                    self.phase = Phase::Done;
                    return Ok(());
                }
                self.event_offset = self.pos;
                if self.cur() == b'<' {
                    let n = self.at(self.pos + 1);
                    if n == b'?' {
                        return self.parse_pi(sink);
                    }
                    if n == b'!' {
                        if self.starts_with(b"<!--") {
                            return self.parse_comment(sink);
                        }
                        if self.phase == Phase::Prolog && self.starts_with(b"<!DOCTYPE") {
                            if self.dtd.is_some() || self.root_seen {
                                return Err(
                                    self.fatal(5, "Extra content at the end of the document")
                                );
                            }
                            self.parse_doctype(sink)?;
                            return Ok(());
                        }
                        if self.incomplete() {
                            return Err(Halt::Incomplete);
                        }
                    }
                    if self.phase == Phase::Prolog && n == b'/' {
                        self.pos += 1;
                        return Err(self.fatal(68, "StartTag: invalid element name"));
                    }
                    if self.phase == Phase::Prolog && n != b'/' && n != b'!' && n != 0 {
                        self.phase = Phase::Content;
                        self.root_seen = true;
                        return self.parse_start_tag(sink);
                    }
                    if self.incomplete() {
                        return Err(Halt::Incomplete);
                    }
                }
                if self.phase == Phase::Prolog {
                    if self.cur() == b'<'
                        && self.at(self.pos + 1) != b'/'
                        && self.at(self.pos + 1) != b'!'
                    {
                        self.pos += 1;
                        return Err(self.fatal(68, "StartTag: invalid element name"));
                    }
                    return Err(self.fatal(4, "Start tag expected, '<' not found"));
                }
                Err(self.fatal(5, "Extra content at the end of the document"))
            }
            Phase::Content => self.content_step(sink),
            Phase::Done => Ok(()),
        }
    }

    fn content_step(&mut self, sink: &mut dyn Sink) -> R<()> {
        if self.eof() {
            if !self.final_ {
                return Err(Halt::Incomplete);
            }
            if self.fragment && self.stack.is_empty() && self.entity_stack.is_empty() {
                self.flush_text(sink, false);
                if self.enc_error {
                    return Err(self.halt(81, "Invalid bytes in character encoding"));
                }
                self.phase = Phase::Done;
                return Ok(());
            }
            return Err(self.eof_error());
        }
        self.event_offset = self.pos;
        match self.cur() {
            b'<' => {
                let n = self.at(self.pos + 1);
                if n == 0 && self.incomplete() {
                    return Err(Halt::Incomplete);
                }
                match n {
                    b'/' => {
                        self.flush_text(sink, true);
                        self.parse_end_tag(sink)
                    }
                    b'!' => {
                        if self.starts_with(b"<!--") {
                            self.flush_text(sink, false);
                            self.parse_comment(sink)
                        } else if self.starts_with(b"<![CDATA[") {
                            self.flush_text(sink, false);
                            self.parse_cdata(sink)
                        } else if self.incomplete() {
                            Err(Halt::Incomplete)
                        } else {
                            self.pos += 2;
                            Err(self.fatal(68, "StartTag: invalid element name"))
                        }
                    }
                    b'?' => {
                        self.flush_text(sink, false);
                        self.parse_pi(sink)
                    }
                    _ => {
                        self.flush_text(sink, false);
                        self.parse_start_tag(sink)
                    }
                }
            }
            b'&' => self.parse_reference(sink),
            _ => self.parse_text(sink),
        }
    }

    // ----- text -----------------------------------------------------------

    fn parse_text(&mut self, sink: &mut dyn Sink) -> R<()> {
        if self.opts.sax && self.cr_next < self.cr_positions.len() {
            return self.parse_text_split(sink);
        }
        self.parse_text_limited(usize::MAX)
    }

    /// Scans character data up to markup, a reference, or `limit`.
    fn parse_text_limited(&mut self, limit: usize) -> R<()> {
        let start = self.pos;
        let buf = &self.buf;
        let full = buf.len();
        let len = full.min(limit);
        let mut i = self.pos;
        while i < len {
            let b = buf[i];
            match b {
                b'<' | b'&' => break,
                b']' => {
                    if i + 2 < full && buf[i + 1] == b']' && buf[i + 2] == b'>' {
                        self.text_push(start, i);
                        self.pos = i;
                        return Err(self.fatal(62, "Sequence ']]>' not allowed in content"));
                    }
                    if i + 2 >= full && !self.final_ {
                        break;
                    }
                    i += 1;
                }
                0x20..=0x7F | b'\t' | b'\n' | b'\r' => i += 1,
                0..=0x1F => {
                    self.text_push(start, i);
                    self.pos = i;
                    return Err(self.halt(9, format!("PCDATA invalid Char value {b}")));
                }
                0xEF => {
                    if i + 2 < len
                        && buf[i + 1] == 0xBF
                        && (buf[i + 2] == 0xBE || buf[i + 2] == 0xBF)
                    {
                        let v = if buf[i + 2] == 0xBE { 0xFFFE } else { 0xFFFF };
                        self.text_push(start, i);
                        self.pos = i;
                        return Err(self.halt(9, format!("PCDATA invalid Char value {v}")));
                    }
                    i += 1;
                }
                _ => i += 1,
            }
        }
        self.text_push(start, i);
        self.pos = i;
        if i >= full {
            self.hit_eof.set(true);
        }
        Ok(())
    }

    /// SAX mode text with carriage returns in it: character data is
    /// reported in pieces that start at each former CR (observed on PHP 8.5).
    fn parse_text_split(&mut self, sink: &mut dyn Sink) -> R<()> {
        while self.cr_next < self.cr_positions.len()
            && (self.cr_positions[self.cr_next] >> 1) < self.pos
        {
            self.cr_next += 1;
        }
        let limit = self.cr_positions.get(self.cr_next).map(|e| e >> 1);
        match limit {
            Some(l) if l == self.pos => {
                // A piece starts here: flush what came before.
                self.flush_text(sink, false);
                self.cr_next += 1;
                let next = self
                    .cr_positions
                    .get(self.cr_next)
                    .map_or(usize::MAX, |e| e >> 1);
                self.parse_text_until(next)
            }
            Some(l) => self.parse_text_until(l),
            None => self.parse_text_until(usize::MAX),
        }
    }

    fn parse_text_until(&mut self, limit: usize) -> R<()> {
        self.parse_text_limited(limit)
    }

    fn text_push(&mut self, start: usize, end: usize) {
        if end > start {
            if self.text.is_empty() {
                self.text_begin = start;
            }
            let s = unsafe { std::str::from_utf8_unchecked(&self.buf[start..end]) };
            self.text.push_str(s);
        }
    }

    fn flush_text(&mut self, sink: &mut dyn Sink, before_end_tag: bool) {
        if self.text.is_empty() {
            return;
        }
        if self.opts.sax {
            self.emit_position(sink, self.pos);
        }
        let text = std::mem::take(&mut self.text);
        let blank = text.bytes().all(is_space);
        let ignorable = blank && self.is_ignorable_blank(before_end_tag);
        if let Some(o) = self.stack.last_mut() {
            if ignorable {
                sink.ignorable_whitespace(&text);
            } else {
                o.had_text = true;
                sink.text(&text);
            }
        } else if !blank || self.fragment {
            sink.text(&text);
        }
        self.text = text;
        self.text.clear();
    }

    /// The "blank node" judgement for LIBXML_NOBLANKS (observed on
    /// PHP 8.5): whitespace-only text is dropped unless `xml:space`
    /// preserves it, the DTD declares mixed content, earlier text was kept
    /// in the same element, or it is the element's only content.
    fn is_ignorable_blank(&self, before_end_tag: bool) -> bool {
        if !self.opts.has(opt::NOBLANKS) {
            return false;
        }
        let Some(o) = self.stack.last() else {
            return false;
        };
        if o.preserve {
            return false;
        }
        if let Some(d) = &self.dtd
            && let Some(decl) = d.elements.get(o.raw.as_ref())
        {
            return matches!(
                decl.content,
                ContentModel::Children(_) | ContentModel::Empty
            );
        }
        if o.had_text {
            return false;
        }
        if !o.had_child && before_end_tag {
            return false;
        }
        true
    }

    // ----- XML declaration --------------------------------------------------

    fn parse_xml_decl(&mut self, sink: &mut dyn Sink) -> R<()> {
        self.pos += 5;
        self.skip_spaces();

        let version = if self.starts_with(b"version") {
            self.pos += 7;
            self.skip_spaces();
            if self.cur() != b'=' {
                return Err(self.fatal(75, "Specification mandates value for attribute version"));
            }
            self.pos += 1;
            self.skip_spaces();
            let v = self.parse_decl_value()?;
            if !v.starts_with("1.") || v.len() < 3 || !v[2..].bytes().all(|b| b.is_ascii_digit()) {
                return Err(self.fatal(108, format!("Unsupported version '{v}'")));
            }
            if v != "1.0" {
                self.warn(Domain::Parser, 97, format!("Unsupported version '{v}'"));
            }
            Some(v)
        } else {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            self.fatal(96, "Malformed declaration expecting version");
            return Err(self.fatal(65, "Blank needed here"));
        };
        let had_space = self.skip_spaces();
        let mut encoding = None;
        if self.starts_with(b"encoding") {
            if !had_space {
                self.fatal(65, "Blank needed here");
            }
            self.pos += 8;
            self.skip_spaces();
            if self.cur() != b'=' {
                return Err(self.fatal(75, "Specification mandates value for attribute encoding"));
            }
            self.pos += 1;
            self.skip_spaces();
            let e = self.parse_decl_value()?;
            let valid_name = e.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                && e.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
            if !valid_name {
                return Err(self.fatal(81, "Invalid XML encoding name"));
            }
            let up = e.to_ascii_uppercase();
            if (up == "UTF-16" || up == "UTF16" || up == "ISO-10646-UCS-2" || up == "UCS-2")
                && !self.utf16_input
            {
                // The declaration switches to a 16-bit encoding the bytes
                // are not in (observed on PHP 8.5).
                self.fatal(65, "Blank needed here");
                return Err(self.fatal(57, "parsing XML declaration: '?>' expected"));
            }
            encoding = Some(e);
        }
        let had_space = self.skip_spaces() || had_space && encoding.is_none();
        if self.starts_with(b"standalone") {
            if !had_space {
                self.fatal(65, "Blank needed here");
            }
            self.pos += 10;
            self.skip_spaces();
            if self.cur() != b'=' {
                return Err(self.fatal(75, "Specification mandates value for attribute standalone"));
            }
            self.pos += 1;
            self.skip_spaces();
            let q = self.cur();
            if q == b'"' || q == b'\'' {
                self.pos += 1;
                if self.starts_with(b"yes") && self.at(self.pos + 3) == q {
                    self.standalone = 1;
                    self.pos += 4;
                } else if self.starts_with(b"no") && self.at(self.pos + 2) == q {
                    self.standalone = 0;
                    self.pos += 3;
                } else {
                    if self.incomplete() {
                        return Err(Halt::Incomplete);
                    }
                    self.fatal(78, "standalone accepts only 'yes' or 'no'");
                    self.fatal(34, "String not closed expecting \" or '");
                    return Err(self.fatal(57, "parsing XML declaration: '?>' expected"));
                }
            } else {
                return Err(self.fatal(78, "standalone accepts only 'yes' or 'no'"));
            }
        }
        self.skip_spaces();
        if !self.starts_with(b"?>") {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            return Err(self.fatal(57, "parsing XML declaration: '?>' expected"));
        }
        self.pos += 2;
        if let Some(e) = &encoding
            && crate::encoding::lookup(e).is_none()
        {
            return Err(self.fatal(32, format!("Unsupported encoding: {e}")));
        }
        sink.xml_decl(
            version.as_deref().unwrap_or("1.0"),
            encoding.as_deref(),
            self.standalone,
        );
        self.version = version;
        self.encoding = encoding;
        Ok(())
    }

    fn parse_decl_value(&mut self) -> R<String> {
        let q = self.cur();
        if q != b'"' && q != b'\'' {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            return Err(self.fatal(34, "String not started expecting ' or \""));
        }
        self.pos += 1;
        let start = self.pos;
        while !self.eof() && self.cur() != q {
            self.pos += 1;
        }
        if self.eof() {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            return Err(self.fatal(34, "String not closed expecting \" or '"));
        }
        let v = self.slice((start, self.pos)).to_string();
        self.pos += 1;
        Ok(v)
    }

    // ----- comments, PIs, CDATA ---------------------------------------------

    fn parse_comment(&mut self, sink: &mut dyn Sink) -> R<()> {
        self.pos += 4;
        let start = self.pos;
        loop {
            match memchr::memchr(b'-', &self.buf[self.pos.min(self.buf.len())..]) {
                Some(off) => {
                    let i = self.pos + off;
                    if self.at(i + 1) == b'-' {
                        if self.at(i + 2) == b'>' {
                            self.check_chars(start, i)?;
                            let text = self.slice((start, i)).to_string();
                            self.pos = i + 3;
                            if self.incomplete() {
                                return Err(Halt::Incomplete);
                            }
                            if !self.stopped {
                                if self.opts.sax {
                                    let raw = self.slice((start - 4, self.pos)).to_string();
                                    self.emit_position(sink, self.pos);
                                    sink.raw(&raw);
                                }
                                sink.comment(&text);
                            }
                            if let Some(o) = self.stack.last_mut() {
                                o.had_child = true;
                            }
                            return Ok(());
                        }
                        if self.incomplete() {
                            return Err(Halt::Incomplete);
                        }
                        let so_far: String = self.slice((start, i)).chars().take(50).collect();
                        self.pos = i;
                        return Err(
                            self.fatal(80, format!("Double hyphen within comment: <!--{so_far}"))
                        );
                    }
                    self.pos = i + 1;
                }
                None => {
                    self.pos = self.buf.len();
                    self.hit_eof.set(true);
                    if !self.final_ {
                        return Err(Halt::Incomplete);
                    }
                    if self.enc_error {
                        return Err(self.halt(81, "Invalid bytes in character encoding"));
                    }
                    return Err(self.fatal(45, "Comment not terminated"));
                }
            }
        }
    }

    pub(crate) fn check_chars(&mut self, start: usize, end: usize) -> R<()> {
        let s = &self.buf[start..end];
        for (i, &b) in s.iter().enumerate() {
            if b < 0x20 && b != b'\t' && b != b'\n' && b != b'\r' {
                self.pos = start + i;
                return Err(self.halt(9, format!("xmlParseComment: invalid xmlChar value {b}")));
            }
            if b == 0xEF
                && i + 2 < s.len()
                && s[i + 1] == 0xBF
                && (s[i + 2] == 0xBE || s[i + 2] == 0xBF)
            {
                self.pos = start + i;
                let v = if s[i + 2] == 0xBE { 0xFFFE } else { 0xFFFF };
                return Err(self.halt(9, format!("xmlParseComment: invalid xmlChar value {v}")));
            }
        }
        Ok(())
    }

    fn parse_pi(&mut self, sink: &mut dyn Sink) -> R<()> {
        self.tag_start = self.pos;
        self.pos += 2;
        let Some(r) = self.scan_name() else {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            return Err(self.fatal(46, "xmlParsePI : no target name"));
        };
        let target = self.slice(r).to_string();
        if target.eq_ignore_ascii_case("xml") {
            if target != "xml" {
                return Err(self.fatal(64, "Invalid PI name"));
            }
            // Reported, then the PI is still read (observed on PHP 8.5).
            self.fatal(
                64,
                "XML declaration allowed only at the start of the document",
            );
        }
        if target.len() > 3 && target.as_bytes()[..3].eq_ignore_ascii_case(b"xml") {
            self.warn(
                Domain::Parser,
                64,
                format!("xmlParsePITarget: invalid name prefix '{}'", &target[..3]),
            );
        }
        if self.opts.namespaces && target.contains(':') {
            self.error(
                Domain::Namespace,
                205,
                format!("colons are forbidden from PI names '{target}'"),
            );
        }
        let data;
        if self.starts_with(b"?>") {
            self.pos += 2;
            data = String::new();
        } else {
            if !self.skip_spaces() {
                if self.incomplete() {
                    return Err(Halt::Incomplete);
                }
                self.fatal(65, format!("ParsePI: PI {target} space expected"));
                if memchr::memmem::find(&self.buf[self.pos.min(self.buf.len())..], b"?>").is_none()
                {
                    self.pos = self.buf.len();
                    return Err(self.fatal(47, format!("ParsePI: PI {target} never end ...")));
                }
                return Err(Halt::Fatal);
            }
            let start = self.pos;
            match memchr::memmem::find(&self.buf[self.pos..], b"?>") {
                Some(off) => {
                    let end = self.pos + off;
                    self.check_chars(start, end)?;
                    data = self.slice((start, end)).to_string();
                    self.pos = end + 2;
                }
                None => {
                    self.pos = self.buf.len();
                    self.hit_eof.set(true);
                    if !self.final_ {
                        return Err(Halt::Incomplete);
                    }
                    return Err(self.fatal(47, format!("ParsePI: PI {target} never end ...")));
                }
            }
        }
        if !self.stopped {
            if self.opts.sax {
                let raw = self.slice((self.tag_start, self.pos)).to_string();
                self.emit_position(sink, self.pos);
                sink.raw(&raw);
            }
            sink.pi(&target, &data);
        }
        if let Some(o) = self.stack.last_mut() {
            o.had_child = true;
        }
        Ok(())
    }

    fn parse_cdata(&mut self, sink: &mut dyn Sink) -> R<()> {
        self.pos += 9;
        let start = self.pos;
        match memchr::memmem::find(&self.buf[self.pos..], b"]]>") {
            Some(off) => {
                let end = self.pos + off;
                for i in start..end {
                    let b = self.buf[i];
                    if b < 0x20 && b != b'\t' && b != b'\n' {
                        self.pos = i;
                        return Err(self.halt(9, format!("PCDATA invalid Char value {b}")));
                    }
                }
                let text = self.slice((start, end)).to_string();
                self.pos = end + 3;
                if !self.stopped {
                    if self.opts.has(opt::NOCDATA) {
                        if let Some(o) = self.stack.last_mut() {
                            o.had_text = true;
                        }
                        self.text.push_str(&text);
                    } else {
                        sink.cdata(&text);
                        if let Some(o) = self.stack.last_mut() {
                            o.had_text = true;
                        }
                    }
                }
                Ok(())
            }
            None => {
                self.pos = self.buf.len();
                self.hit_eof.set(true);
                if !self.final_ {
                    return Err(Halt::Incomplete);
                }
                let s = self
                    .slice((start, self.buf.len().saturating_sub(2).max(start)))
                    .to_string();
                Err(self.fatal(63, format!("CData section not finished\n{s}")))
            }
        }
    }

    // ----- elements -----------------------------------------------------------

    fn parse_start_tag(&mut self, sink: &mut dyn Sink) -> R<()> {
        let tag_start = self.pos;
        self.tag_start = tag_start;
        self.pos += 1;
        let Some(nr) = self.scan_name() else {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            return Err(self.fatal(68, "StartTag: invalid element name"));
        };
        let raw_name = self.intern_range(nr);
        let line = self.line(tag_start);
        self.name_end_lc = Some(self.pos);
        let mut raw_attrs: Vec<RawAttr> = Vec::new();
        let mut empty = false;
        let mut failed = false;
        loop {
            let had_space = self.skip_spaces();
            let b = self.cur();
            if b == b'>' {
                self.pos += 1;
                break;
            }
            if b == b'/' && self.at(self.pos + 1) == b'>' {
                self.pos += 2;
                empty = true;
                break;
            }
            if self.eof() || self.incomplete() {
                if self.incomplete() {
                    return Err(Halt::Incomplete);
                }
                if self.enc_error {
                    return Err(self.halt(81, "Invalid bytes in character encoding"));
                }
                if !failed {
                    return Err(self.fatal(
                        73,
                        format!("Couldn't find end of Start Tag {raw_name} line {line}"),
                    ));
                }
                self.fatal(65, "attributes construct error");
                return Err(self.fatal(
                    73,
                    format!("Couldn't find end of Start Tag {raw_name} line {line}"),
                ));
            }
            if failed {
                self.fatal(65, "attributes construct error");
                return Err(self.fatal(
                    73,
                    format!("Couldn't find end of Start Tag {raw_name} line {line}"),
                ));
            }
            let Some(an) = self.scan_name() else {
                self.fatal(68, "error parsing attribute name");
                return Err(self.fatal(
                    73,
                    format!("Couldn't find end of Start Tag {raw_name} line {line}"),
                ));
            };
            if !had_space {
                self.pos = an.0;
                self.fatal(65, "attributes construct error");
                return Err(self.fatal(
                    73,
                    format!("Couldn't find end of Start Tag {raw_name} line {line}"),
                ));
            }
            let name_end = self.pos;
            let aname = self.intern_range(an);
            self.skip_spaces();
            if self.cur() != b'=' {
                if self.incomplete() {
                    return Err(Halt::Incomplete);
                }
                return Err(self.fatal(
                    41,
                    format!("Specification mandates value for attribute {aname}"),
                ));
            }
            self.pos += 1;
            self.skip_spaces();
            let q = self.cur();
            if q != b'"' && q != b'\'' {
                if self.incomplete() {
                    return Err(Halt::Incomplete);
                }
                self.fatal(39, "AttValue: \" or ' expected");
                failed = true;
                continue;
            }
            match self.parse_att_value(&aname)? {
                Some((v, pieces)) => {
                    if raw_attrs.iter().any(|r| r.name == aname) {
                        return Err(self.fatal(42, format!("Attribute {aname} redefined")));
                    }
                    let name = aname;
                    let name_lc = name_end;
                    let value_lc = self.pos;
                    raw_attrs.push(RawAttr {
                        name,
                        value: v,
                        pieces,
                        name_lc,
                        value_lc,
                    });
                }
                None => {
                    failed = true;
                }
            }
        }
        if self.incomplete() {
            return Err(Halt::Incomplete);
        }
        if failed || self.stopped {
            return Err(Halt::Fatal);
        }
        let end_pos = self.pos - if empty { 2 } else { 1 };
        self.tag_end_lc = Some(end_pos);
        if self.stack.len() >= self.max_depth {
            let end_lc = self.line_col(end_pos);
            self.err_pos.get_or_insert(end_lc);
            return Err(self.halt(
                114,
                format!(
                    "Excessive depth in document: {}, use XML_PARSE_HUGE option",
                    self.max_depth
                ),
            ));
        }
        self.finish_start_tag(sink, &raw_name, raw_attrs, line, empty)
    }

    fn finish_start_tag(
        &mut self,
        sink: &mut dyn Sink,
        raw_name: &str,
        raw_attrs: Vec<RawAttr>,
        line: u32,
        empty: bool,
    ) -> R<()> {
        let ns_mark = self.ns.len();
        let mut ns_decls: Vec<NsDecl> = Vec::new();
        let mut attrs: Vec<Attribute> = Vec::with_capacity(raw_attrs.len());
        // DTD info for this element.
        let decls: Vec<crate::tree::AttrDecl> = self.attr_decls(raw_name);
        let mut preserve = self.stack.last().is_some_and(|o| o.preserve);
        let outer_err = self.err_pos;
        let in_entity = !self.entity_stack.is_empty();
        if let Some(p) = self.name_end_lc.take()
            && !in_entity
        {
            self.err_at = Some(p);
        }
        if self.opts.namespaces && raw_name.contains(':') && split_qname(raw_name).is_none() {
            self.error(
                Domain::Namespace,
                202,
                format!("Failed to parse QName '{raw_name}'"),
            );
        }
        for RawAttr {
            name: an,
            value: mut v,
            pieces,
            name_lc,
            value_lc,
        } in raw_attrs
        {
            if !in_entity {
                self.err_at = Some(value_lc);
            }
            if self.opts.namespaces && an.contains(':') && split_qname(&an).is_none() {
                if !in_entity {
                    self.err_at = Some(name_lc);
                }
                self.error(
                    Domain::Namespace,
                    202,
                    format!("Failed to parse QName '{an}'"),
                );
                if !in_entity {
                    self.err_at = Some(value_lc);
                }
            }
            let atype = decls
                .iter()
                .find(|d| *d.name == *an)
                .map_or(AttrType::Cdata, |d| d.atype);
            if atype != AttrType::Cdata {
                v = collapse_spaces(&v);
            }
            if self.opts.namespaces {
                if &*an == "xmlns" {
                    if !v.is_empty() {
                        self.check_ns_uri("xmlns", &v, true);
                    }
                    if v == XML_NS {
                        self.error(
                            Domain::Namespace,
                            200,
                            "xml namespace URI cannot be the default namespace",
                        );
                        continue;
                    }
                    ns_decls.push(NsDecl {
                        prefix: None,
                        uri: self.intern(&v),
                    });
                    continue;
                }
                if let Some(p) = an.strip_prefix("xmlns:") {
                    if p == "xml" {
                        if v != XML_NS {
                            self.error(
                                Domain::Namespace,
                                200,
                                "xml namespace prefix mapped to wrong URI",
                            );
                        }
                        continue;
                    }
                    if v == XML_NS {
                        self.error(
                            Domain::Namespace,
                            200,
                            "xml namespace URI mapped to wrong prefix",
                        );
                        continue;
                    }
                    if p == "xmlns" {
                        self.error(
                            Domain::Namespace,
                            200,
                            "redefinition of the xmlns prefix is forbidden",
                        );
                        continue;
                    }
                    if v.is_empty() {
                        self.error(
                            Domain::Namespace,
                            200,
                            format!("xmlns:{p}: Empty XML namespace is not allowed"),
                        );
                        continue;
                    }
                    self.check_ns_uri(&format!("xmlns:{p}"), &v, false);
                    let pfx = self.intern(p);
                    ns_decls.push(NsDecl {
                        prefix: Some(pfx),
                        uri: self.intern(&v),
                    });
                    continue;
                }
            }
            if &*an == "xml:space" {
                preserve = v == "preserve";
            }
            let is_id = atype == AttrType::Id || &*an == "xml:id";
            attrs.push(Attribute {
                name: QName {
                    prefix: None,
                    local: an,
                    ns: None,
                },
                value: v,
                pieces,
                defaulted: false,
                atype,
                is_id,
            });
        }
        // DTD defaults.
        if !decls.is_empty() {
            for d in &decls {
                if matches!(d.default, AttrDefault::None | AttrDefault::Fixed)
                    && let Some(val) = &d.value
                {
                    let specified = attrs.iter().any(|a| *a.name.local == *d.name)
                        || (d.name.as_ref() == "xmlns"
                            && ns_decls.iter().any(|n| n.prefix.is_none()))
                        || d.name.strip_prefix("xmlns:").is_some_and(|p| {
                            ns_decls.iter().any(|n| n.prefix.as_deref() == Some(p))
                        });
                    if specified {
                        continue;
                    }
                    if self.opts.namespaces && (&*d.name == "xmlns" || d.name.starts_with("xmlns:"))
                    {
                        let prefix = d.name.strip_prefix("xmlns:").map(|p| self.intern(p));
                        let uri = self.intern(val);
                        ns_decls.push(NsDecl { prefix, uri });
                        continue;
                    }
                    if self.opts.has(opt::DTDATTR) {
                        attrs.push(Attribute {
                            name: QName {
                                prefix: None,
                                local: d.name.clone(),
                                ns: None,
                            },
                            value: val.clone(),
                            pieces: None,
                            defaulted: true,
                            atype: d.atype,
                            is_id: d.atype == AttrType::Id,
                        });
                    }
                }
            }
        }
        if let Some(p) = self.tag_end_lc.take()
            && !in_entity
        {
            self.err_at = Some(p);
        }
        // Namespace resolution.
        for d in &ns_decls {
            self.ns.push((d.prefix.clone(), d.uri.clone()));
        }
        let name = if self.opts.namespaces {
            match split_qname(raw_name) {
                Some((p, l)) => {
                    let ns = self.resolve(Some(p));
                    if ns.is_none() {
                        self.error(
                            Domain::Namespace,
                            201,
                            format!("Namespace prefix {p} on {l} is not defined"),
                        );
                    }
                    QName {
                        prefix: Some(self.intern(p)),
                        local: self.intern(l),
                        ns,
                    }
                }
                None => {
                    let ns = self.resolve(None);
                    QName {
                        prefix: None,
                        local: self.intern(raw_name),
                        ns,
                    }
                }
            }
        } else {
            QName {
                prefix: None,
                local: self.intern(raw_name),
                ns: None,
            }
        };
        if self.opts.namespaces {
            #[allow(clippy::needless_range_loop)]
            for i in 0..attrs.len() {
                let local = attrs[i].name.local.clone();
                if let Some((p, l)) = split_qname(&local) {
                    let ns = if p == "xml" {
                        Some(self.intern(XML_NS))
                    } else {
                        self.resolve(Some(p))
                    };
                    if ns.is_none() {
                        self.error(
                            Domain::Namespace,
                            201,
                            format!(
                                "Namespace prefix {p} for {l} on {} is not defined",
                                raw_name
                            ),
                        );
                    }
                    let pfx = self.intern(p);
                    let loc = self.intern(l);
                    attrs[i].name = QName {
                        prefix: Some(pfx),
                        local: loc,
                        ns,
                    };
                }
            }
            // Same expanded name twice.
            for i in 1..attrs.len() {
                for j in 0..i {
                    if attrs[i].name.ns.is_some()
                        && attrs[i].name.ns == attrs[j].name.ns
                        && attrs[i].name.local == attrs[j].name.local
                    {
                        let ns = attrs[i].name.ns.clone().unwrap();
                        self.error(
                            Domain::Namespace,
                            203,
                            format!(
                                "Namespaced Attribute {} in '{}' redefined",
                                attrs[i].name.local, ns
                            ),
                        );
                    }
                }
            }
        }
        if self.opts.has(opt::NSCLEAN) {
            // Drop declarations that repeat what is already in scope.
            let mut kept = Vec::new();
            for d in ns_decls.drain(..) {
                let before = self.ns[..ns_mark]
                    .iter()
                    .rev()
                    .find(|(p, _)| *p == d.prefix)
                    .map(|(_, u)| u.clone());
                if before.as_deref() == Some(&*d.uri) {
                    continue;
                }
                kept.push(d);
            }
            ns_decls = kept;
        }
        self.err_pos = outer_err;
        self.err_at = None;
        if let Some(o) = self.stack.last_mut() {
            o.had_child = true;
        }
        sink.set_line(line);
        if self.opts.sax && !self.stopped {
            let raw = self.slice((self.tag_start, self.pos)).to_string();
            let marker = self.pos - if empty { 2 } else { 1 };
            self.emit_position(sink, marker);
            sink.raw(&raw);
        }
        if !self.stopped {
            sink.start_element(&StartTag {
                name: &name,
                attrs: &attrs,
                ns_decls: &ns_decls,
                line,
                empty,
            });
        }
        if empty {
            if !self.stopped {
                if self.opts.sax {
                    self.emit_position(sink, self.pos);
                }
                sink.end_element(&name);
            }
            self.ns.truncate(ns_mark);
            if self.stack.is_empty() && self.entity_stack.is_empty() && !self.fragment {
                self.phase = Phase::Epilog;
            }
        } else {
            let raw = self.intern(raw_name);
            self.stack.push(Open {
                name,
                raw,
                line,
                ns_mark,
                preserve,
                had_text: false,
                had_child: false,
            });
        }
        Ok(())
    }

    fn check_ns_uri(&mut self, attr: &str, v: &str, default: bool) {
        if !is_valid_uri(v) {
            self.error(
                Domain::Namespace,
                99,
                format!("{attr}: '{v}' is not a valid URI"),
            );
        } else if default && !is_absolute_uri(v) {
            self.warn(
                Domain::Namespace,
                100,
                format!("{attr}: URI {v} is not absolute"),
            );
        }
    }

    fn resolve(&self, prefix: Option<&str>) -> Option<Str> {
        if prefix == Some("xml") {
            return Some(Rc::from(XML_NS));
        }
        for (p, u) in self.ns.iter().rev() {
            if p.as_deref() == prefix {
                return if u.is_empty() { None } else { Some(u.clone()) };
            }
        }
        None
    }

    fn attr_decls(&self, el: &str) -> Vec<crate::tree::AttrDecl> {
        let mut v = Vec::new();
        for d in [&self.dtd, &self.ext].into_iter().flatten() {
            if let Some(l) = d.attlists.get(el) {
                for a in l {
                    if !v.iter().any(|x: &crate::tree::AttrDecl| x.name == a.name) {
                        v.push(a.clone());
                    }
                }
            }
        }
        v
    }

    fn parse_end_tag(&mut self, sink: &mut dyn Sink) -> R<()> {
        self.tag_start = self.pos;
        self.pos += 2;
        let r = self.scan_name();
        if self.incomplete() {
            return Err(Halt::Incomplete);
        }
        let Some(r) = r else {
            return Err(self.fatal(74, "expected '>'"));
        };
        let name = self.slice(r).to_string();
        self.skip_spaces();
        if self.cur() != b'>' {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            return Err(self.fatal(73, "expected '>'"));
        }
        self.pos += 1;
        let Some(top) = self.stack.last() else {
            return Err(self.fatal(5, "Extra content at the end of the document"));
        };
        if *top.raw != *name {
            let (n, l) = (top.raw.clone(), top.line);
            let e = self.fatal(
                76,
                format!("Opening and ending tag mismatch: {n} line {l} and {name}"),
            );
            if self.recover() {
                // Pop to the matching element if open, else ignore.
                if self.stack.iter().any(|o| *o.raw == *name) {
                    while let Some(o) = self.stack.pop() {
                        sink.end_element(&o.name);
                        self.ns.truncate(o.ns_mark);
                        if *o.raw == *name {
                            break;
                        }
                    }
                    if self.stack.is_empty() && self.entity_stack.is_empty() && !self.fragment {
                        self.phase = Phase::Epilog;
                    }
                }
                return Ok(());
            }
            return Err(e);
        }
        let o = self.stack.pop().unwrap();
        if !self.stopped {
            if self.opts.sax {
                let raw = self.slice((self.tag_start, self.pos)).to_string();
                self.emit_position(sink, self.pos);
                sink.raw(&raw);
            }
            sink.end_element(&o.name);
        }
        self.ns.truncate(o.ns_mark);
        if self.stack.is_empty() && self.entity_stack.is_empty() && !self.fragment {
            self.phase = Phase::Epilog;
        }
        Ok(())
    }

    // ----- references ---------------------------------------------------------

    /// Parses `&#...;` at the position (after `&`); returns the char.
    fn parse_char_ref(&mut self) -> R<char> {
        // At "&#".
        self.pos += 2;
        let mut v: u32 = 0;
        let hex = self.cur() == b'x';
        if hex {
            self.pos += 1;
        }
        let start = self.pos;
        let mut overflow = false;
        loop {
            let b = self.cur();
            let d = if hex {
                (b as char).to_digit(16)
            } else {
                (b as char).to_digit(10)
            };
            match d {
                Some(d) if b != 0 => {
                    v = v
                        .checked_mul(if hex { 16 } else { 10 })
                        .and_then(|x| x.checked_add(d))
                        .unwrap_or_else(|| {
                            overflow = true;
                            0x110000
                        });
                    self.pos += 1;
                }
                _ => break,
            }
        }
        if self.incomplete() {
            return Err(Halt::Incomplete);
        }
        if self.cur() != b';' || self.pos == start {
            if hex {
                self.fatal(6, "CharRef: invalid hexadecimal value");
            } else {
                self.fatal(7, "CharRef: invalid decimal value");
            }
            return Err(self.halt(9, "xmlParseCharRef: invalid xmlChar value 0"));
        }
        self.pos += 1;
        let _ = overflow;
        if v > 0x10FFFF {
            return Err(self.halt(
                9,
                "xmlParseCharRef: character reference out of bounds".to_string(),
            ));
        }
        match char::from_u32(v) {
            Some(c) if is_xml_char(v) => Ok(c),
            _ => Err(self.halt(9, format!("xmlParseCharRef: invalid xmlChar value {v}"))),
        }
    }

    fn parse_reference(&mut self, sink: &mut dyn Sink) -> R<()> {
        let ref_start = self.pos;
        if self.at(self.pos + 1) == b'#' {
            let c = self.parse_char_ref()?;
            if self.opts.sax {
                self.flush_text(sink, false);
                let raw = self.slice((ref_start, self.pos)).to_string();
                let mut b = [0u8; 4];
                sink.reference(&raw, c.encode_utf8(&mut b));
                return Ok(());
            }
            if self.opts.split_text_at_refs {
                self.flush_text(sink, false);
            }
            self.text.push(c);
            if self.opts.split_text_at_refs {
                self.flush_text(sink, false);
            }
            return Ok(());
        }
        self.pos += 1;
        let Some(r) = self.scan_name() else {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            return Err(self.fatal(68, "xmlParseEntityRef: no name"));
        };
        let name = self.slice(r).to_string();
        if self.cur() != b';' {
            if self.incomplete() {
                return Err(Halt::Incomplete);
            }
            return Err(self.fatal(23, "EntityRef: expecting ';'"));
        }
        self.pos += 1;
        if let Some(c) = crate::tree::predefined_entity(&name) {
            if self.opts.sax {
                self.flush_text(sink, false);
                let raw = self.slice((ref_start, self.pos)).to_string();
                let mut b = [0u8; 4];
                sink.reference(&raw, c.encode_utf8(&mut b));
                return Ok(());
            }
            if self.opts.split_text_at_refs {
                self.flush_text(sink, false);
            }
            self.text.push(c);
            if self.opts.split_text_at_refs {
                self.flush_text(sink, false);
            }
            return Ok(());
        }
        self.expand_general_entity(sink, &name)
    }

    pub(crate) fn find_entity(&self, name: &str, parameter: bool) -> Option<EntityDecl> {
        for d in [&self.dtd, &self.ext].into_iter().flatten() {
            let map = if parameter { &d.pentities } else { &d.entities };
            if let Some(e) = map.get(name) {
                return Some(e.clone());
            }
        }
        None
    }

    fn undeclared_entity(&mut self, name: &str) -> R<()> {
        // XML 1.0 4.1 WFC Entity Declared / VC Entity Declared.
        let external_unread = self
            .dtd
            .as_ref()
            .is_some_and(|d| d.system_id.is_some() || d.public_id.is_some())
            && self.ext.is_none();
        if self.standalone != 1 && (self.has_pe_refs || external_unread) {
            self.error(Domain::Parser, 27, format!("Entity '{name}' not defined"));
            return Ok(());
        }
        Err(self.fatal(26, format!("Entity '{name}' not defined")))
    }

    /// WFC Entity Declared: a standalone document may not reference
    /// entities declared in the external subset.
    pub(crate) fn declared_externally(&self, name: &str) -> bool {
        self.standalone == 1
            && !self
                .dtd
                .as_ref()
                .is_some_and(|d| d.entities.contains_key(name))
            && self
                .ext
                .as_ref()
                .is_some_and(|d| d.entities.contains_key(name))
    }

    fn expand_general_entity(&mut self, sink: &mut dyn Sink, name: &str) -> R<()> {
        if self.declared_externally(name) {
            return Err(self.fatal(26, format!("Entity '{name}' not defined")));
        }
        let Some(ent) = self.find_entity(name, false) else {
            if self.opts.sax {
                self.flush_text(sink, false);
                sink.undefined_entity(name);
            }
            let r = self.undeclared_entity(name);
            if r.is_ok() {
                self.flush_text(sink, false);
                sink.entity_start(name, None);
                sink.entity_end(name);
            }
            return r;
        };
        if ent.kind == EntityKind::ExternalGeneralUnparsed {
            return Err(self.fatal(28, format!("Entity reference to unparsed entity {name}")));
        }
        if self.entity_stack.iter().any(|e| **e == *name) {
            return Err(self.halt(89, "Detected an entity reference loop"));
        }
        self.flush_text(sink, false);
        let expand = sink.entity_start(name, Some(&ent));
        let content = match ent.kind {
            EntityKind::InternalGeneral => ent.value.clone(),
            EntityKind::ExternalGeneralParsed => {
                if expand && self.opts.load_entities() {
                    self.load_external(
                        ent.public_id.as_deref(),
                        ent.system_id.as_deref().unwrap_or(""),
                        name,
                    )?
                } else {
                    None
                }
            }
            _ => None,
        };
        if expand && let Some(text) = content {
            self.expansions += 1;
            if self.expansions > 100_000 && !self.opts.has(opt::PARSEHUGE) {
                return Err(self.halt(89, "Maximum entity amplification factor exceeded"));
            }
            self.parse_entity_content(sink, name, text)?;
        }
        sink.entity_end(name);
        if let Some(o) = self.stack.last_mut() {
            o.had_child = true;
        }
        Ok(())
    }

    fn load_external(
        &mut self,
        public: Option<&str>,
        system: &str,
        name: &str,
    ) -> R<Option<String>> {
        let base = self.opts.url.clone();
        let res = match self.loader.as_mut() {
            Some(l) => l.load(public, system, base.as_deref()),
            None => Err(LoadError::NotFound),
        };
        match res {
            Ok(bytes) => {
                let text = self.decode_external(&bytes)?;
                let s = String::from_utf8(text).unwrap_or_default();
                Ok(Some(s))
            }
            Err(LoadError::NotFound) => {
                let url = resolve_system_id(system, base.as_deref());
                self.warn(
                    Domain::Io,
                    1549,
                    format!("failed to load external entity \"{url}\""),
                );
                let _ = name;
                Ok(None)
            }
            Err(LoadError::Refused) => Ok(None),
        }
    }

    /// Parses an entity's replacement text as content (XML 1.0 4.3.2: it
    /// must match `content`).
    fn parse_entity_content(&mut self, sink: &mut dyn Sink, name: &str, text: String) -> R<()> {
        let outer_pos = self.line_col(self.pos);
        let saved_err_pos = self.err_pos;
        if self.err_pos.is_none() {
            self.err_pos = Some((outer_pos.0, outer_pos.1.saturating_sub(1).max(1)));
        }
        let saved_buf = std::mem::replace(&mut self.buf, text.into_bytes());
        let saved_pos = std::mem::replace(&mut self.pos, 0);
        let saved_final = std::mem::replace(&mut self.final_, true);
        let saved_line = (self.line_pos, self.line_no, self.col_pos, self.col_val);
        let saved_enc = std::mem::replace(&mut self.enc_error, false);
        self.line_pos = 0;
        self.line_no = 1;
        self.col_pos = 0;
        self.col_val = 1;
        let ename = self.intern(name);
        self.entity_stack.push(ename);
        let depth = self.stack.len();
        let saved_phase = self.phase;
        self.phase = Phase::Content;
        let mut result = Ok(());
        while self.pos < self.buf.len() {
            if let Err(e) = self.content_step(sink) {
                result = Err(e);
                break;
            }
            if self.stack.len() < depth {
                result = Err(self.fatal(5, "Extra content at the end of the document"));
                break;
            }
            if self.stopped {
                result = Err(Halt::Fatal);
                break;
            }
        }
        if result.is_ok() {
            self.flush_text(sink, false);
            if self.stack.len() > depth {
                let o = self.stack.last().unwrap();
                let (n, l) = (o.raw.clone(), o.line);
                result = Err(self.fatal(77, format!("Premature end of data in tag {n} line {l}")));
            }
        }
        self.phase = saved_phase;
        self.entity_stack.pop();
        self.buf = saved_buf;
        self.pos = saved_pos;
        self.final_ = saved_final;
        self.enc_error = saved_enc;
        (self.line_pos, self.line_no, self.col_pos, self.col_val) = saved_line;
        self.err_pos = saved_err_pos;
        if self.halted {
            self.pos = self.buf.len();
        }
        result
    }

    /// Parses a quoted attribute value at the position. Returns `None` when
    /// an error was reported and the tag must fail.
    fn parse_att_value(&mut self, aname: &str) -> R<Option<(String, Option<Vec<AttrPiece>>)>> {
        let q = self.cur();
        self.pos += 1;
        let mut out = String::new();
        let mut pieces: Option<Vec<AttrPiece>> = None;
        let mut seg_start = out.len();
        let keep_refs = !self.opts.has(opt::NOENT);
        loop {
            let start = self.pos;
            // Fast scan.
            let mut i = self.pos;
            let len = self.buf.len();
            while i < len {
                let b = self.buf[i];
                if b == q || b == b'&' || b == b'<' || b < 0x20 {
                    break;
                }
                if b == 0xEF && i + 2 < len && self.buf[i + 1] == 0xBF && self.buf[i + 2] >= 0xBE {
                    break;
                }
                i += 1;
            }
            if i > start {
                out.push_str(unsafe { std::str::from_utf8_unchecked(&self.buf[start..i]) });
            }
            self.pos = i;
            if i >= len {
                self.hit_eof.set(true);
                if !self.final_ {
                    return Err(Halt::Incomplete);
                }
                if self.enc_error {
                    return Err(self.halt(81, "Invalid bytes in character encoding"));
                }
                self.fatal(40, "AttValue: ' expected");
                return Ok(None);
            }
            let b = self.buf[i];
            if b == q {
                self.pos += 1;
                break;
            }
            match b {
                b'<' => {
                    self.fatal(38, "Unescaped '<' not allowed in attributes values");
                    self.pos += 1;
                    out.push('<');
                }
                b'\t' | b'\n' => {
                    out.push(' ');
                    self.pos += 1;
                }
                b'&' => {
                    if self.at(self.pos + 1) == b'#' {
                        match self.parse_char_ref() {
                            Ok(c) => out.push(c),
                            Err(Halt::Incomplete) => return Err(Halt::Incomplete),
                            Err(Halt::Fatal) => return Ok(None),
                        }
                        continue;
                    }
                    self.pos += 1;
                    let Some(r) = self.scan_name() else {
                        if self.incomplete() {
                            return Err(Halt::Incomplete);
                        }
                        self.fatal(68, "xmlParseEntityRef: no name");
                        return Ok(None);
                    };
                    let name = self.slice(r).to_string();
                    if self.cur() != b';' {
                        if self.incomplete() {
                            return Err(Halt::Incomplete);
                        }
                        self.fatal(23, "EntityRef: expecting ';'");
                        return Ok(None);
                    }
                    self.pos += 1;
                    if let Some(c) = crate::tree::predefined_entity(&name) {
                        out.push(c);
                        continue;
                    }
                    if self.declared_externally(&name) {
                        self.fatal(26, format!("Entity '{name}' not defined"));
                        continue;
                    }
                    match self.find_entity(&name, false) {
                        None => {
                            self.undeclared_entity(&name).ok();
                        }
                        Some(e) => {
                            if e.kind != EntityKind::InternalGeneral {
                                self.fatal(
                                    29,
                                    format!("Attribute references external entity '{name}'"),
                                );
                                continue;
                            }
                            let mut exp = String::new();
                            if !self.expand_in_attr(&e, &mut exp, 0)? {
                                return Ok(None);
                            }
                            if keep_refs {
                                let p = pieces.get_or_insert_with(Vec::new);
                                if out.len() > seg_start {
                                    p.push(AttrPiece::Text(out[seg_start..].to_string()));
                                }
                                p.push(AttrPiece::Entity(self.intern(&name)));
                                out.push_str(&exp);
                                seg_start = out.len();
                            } else {
                                out.push_str(&exp);
                            }
                        }
                    }
                }
                0xEF => {
                    self.fatal(9, "invalid character in attribute value");
                    self.pos += 3;
                }
                _ => {
                    // Other control characters.
                    self.fatal(9, "invalid character in attribute value");
                    self.pos += 1;
                }
            }
        }
        let _ = aname;
        if let Some(p) = &mut pieces
            && out.len() > seg_start
        {
            p.push(AttrPiece::Text(out[seg_start..].to_string()));
        }
        Ok(Some((out, pieces)))
    }

    pub(crate) fn expand_in_attr(
        &mut self,
        e: &EntityDecl,
        out: &mut String,
        depth: usize,
    ) -> R<bool> {
        if depth > 40 || self.entity_stack.contains(&e.name) {
            self.halt(89, "Detected an entity reference loop");
            return Ok(false);
        }
        let text = e.value.clone().unwrap_or_default();
        let mut chars = text.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            match c {
                '<' => {
                    self.fatal(
                        38,
                        format!(
                            "'<' in entity '{}' is not allowed in attributes values",
                            e.name
                        ),
                    );
                    return Ok(false);
                }
                '\t' | '\n' | '\r' => out.push(' '),
                '&' => {
                    let rest = &text[i + 1..];
                    let Some(end) = rest.find(';') else {
                        self.fatal(23, "EntityRef: expecting ';'");
                        return Ok(false);
                    };
                    let name = &rest[..end];
                    for _ in 0..=end {
                        chars.next();
                    }
                    if let Some(num) = name.strip_prefix('#') {
                        let v = if let Some(h) = num.strip_prefix('x') {
                            u32::from_str_radix(h, 16).ok()
                        } else {
                            num.parse().ok()
                        };
                        match v.and_then(char::from_u32) {
                            Some(c) => out.push(c),
                            None => {
                                self.halt(9, "xmlParseCharRef: invalid xmlChar value 0");
                                return Ok(false);
                            }
                        }
                    } else if let Some(c) = crate::tree::predefined_entity(name) {
                        out.push(c);
                    } else {
                        match self.find_entity(name, false) {
                            Some(inner) if inner.kind == EntityKind::InternalGeneral => {
                                let n = e.name.clone();
                                self.entity_stack.push(n);
                                let ok = self.expand_in_attr(&inner, out, depth + 1)?;
                                self.entity_stack.pop();
                                if !ok {
                                    return Ok(false);
                                }
                            }
                            Some(_) => {
                                self.fatal(
                                    29,
                                    format!("Attribute references external entity '{name}'"),
                                );
                                return Ok(false);
                            }
                            None => {
                                self.undeclared_entity(name).ok();
                                if self.stopped {
                                    return Ok(false);
                                }
                            }
                        }
                    }
                }
                c => out.push(c),
            }
        }
        Ok(true)
    }

    pub(crate) fn finish_doctype(&mut self, sink: &mut dyn Sink) {
        if let Some(d) = &self.dtd {
            sink.doctype(d, self.ext.as_ref());
        }
    }

    pub(crate) fn interned_or(&mut self, s: &str) -> Str {
        self.intern(s)
    }
}

/// Splits `p:l` into prefix and local part when both are non-empty and the
/// local part has no further colon.
pub(crate) fn split_qname(s: &str) -> Option<(&str, &str)> {
    let (p, l) = s.split_once(':')?;
    if p.is_empty() || l.is_empty() || l.contains(':') {
        return None;
    }
    Some((p, l))
}

/// Attribute-value normalization for non-CDATA types (XML 1.0 3.3.3).
pub(crate) fn collapse_spaces(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for w in v.split(' ').filter(|w| !w.is_empty()) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(w);
    }
    out
}

/// A loose RFC 3986 URI-reference check (what libxml rejects in namespace
/// names: spaces and other characters never allowed in URIs).
pub(crate) fn is_valid_uri(s: &str) -> bool {
    !s.bytes().any(|b| {
        b <= 0x20
            || matches!(
                b,
                b'"' | b'<' | b'>' | b'\\' | b'^' | b'`' | b'{' | b'|' | b'}'
            )
    })
}

/// True if the URI has a scheme (RFC 3986 section 3.1).
pub(crate) fn is_absolute_uri(s: &str) -> bool {
    let mut it = s.bytes();
    match it.next() {
        Some(b) if b.is_ascii_alphabetic() => {}
        _ => return false,
    }
    for b in it {
        if b == b':' {
            return true;
        }
        if !(b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.')) {
            return false;
        }
    }
    false
}

/// Resolves a system identifier against a base URL (RFC 3986 section 5,
/// simplified for file paths).
pub fn resolve_system_id(system: &str, base: Option<&str>) -> String {
    if is_absolute_uri(system) || system.starts_with('/') {
        return system.to_string();
    }
    match base {
        Some(b) => match b.rfind('/') {
            Some(i) => format!("{}{}", &b[..=i], system),
            None => system.to_string(),
        },
        None => system.to_string(),
    }
}
