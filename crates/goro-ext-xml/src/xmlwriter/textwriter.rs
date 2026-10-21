//! A port of libxml2's xmlTextWriter (xmlwriter.c) state machine together
//! with the parts of xmlOutputBuffer it relies on (buffering, output
//! encoding, error latching).
//!
//! The writer itself never talks to the outside world: everything it emits
//! goes into `OutBuf::buffer`. The PHP layer moves the bytes to the actual
//! sink (memory, stream) with `OutBuf::pump`, which mirrors the moments at
//! which libxml2 invokes its write callback.

use std::collections::HashMap;

/// libxml2's MINLEN: the output buffer is handed to the write callback once
/// it holds at least this many bytes.
pub const MINLEN: usize = 4000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum St {
    Name,
    Attribute,
    Text,
    Pi,
    PiText,
    Cdata,
    Dtd,
    DtdText,
    DtdElem,
    DtdElemText,
    DtdAttl,
    DtdAttlText,
    DtdEnty,
    DtdEntyText,
    DtdPent,
    Comment,
}

struct Node {
    name: Vec<u8>,
    state: St,
    id: u64,
}

struct NsEntry {
    prefix: Vec<u8>,
    uri: Vec<u8>,
    /// Identity of the element (front node) the declaration belongs to.
    elem: Option<u64>,
}

// ── Output encoding ─────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub enum EncKind {
    Utf8,
    Latin1,
    Ascii,
    Utf16Le,
    Utf16Be,
    /// "UTF-16": little endian, preceded by a BOM.
    Utf16,
    /// Converted through mbstring (stands in for iconv).
    Mb(String),
}

pub struct Encoder {
    pub name: Vec<u8>,
    pub kind: EncKind,
    /// Per-codepoint conversion cache for `EncKind::Mb` (None = unmappable).
    pub cache: HashMap<u32, Option<Vec<u8>>>,
}

/// Look up one of the encodings libxml2 handles natively.
pub fn builtin_encoder(name: &[u8]) -> Option<Encoder> {
    let lower = String::from_utf8_lossy(name).to_ascii_lowercase();
    let kind = match lower.as_str() {
        "utf-8" | "utf8" => {
            return Some(Encoder {
                name: b"UTF-8".to_vec(),
                kind: EncKind::Utf8,
                cache: HashMap::new(),
            });
        }
        "iso-8859-1" | "iso_8859-1" | "iso8859-1" | "iso88591" | "latin1" | "l1" | "iso-ir-100"
        | "ibm819" | "cp819" | "csisolatin1" | "iso latin 1" | "iso-latin-1"
        | "iso_8859-1:1987" => EncKind::Latin1,
        "ascii" | "us-ascii" | "ansi_x3.4-1968" | "ansi_x3.4-1986" | "iso646-us"
        | "iso_646.irv:1991" | "us" | "ibm367" | "cp367" | "csascii" | "iso-ir-6" => EncKind::Ascii,
        "utf-16" | "utf16" | "csunicode" => EncKind::Utf16,
        "utf-16le" | "utf16le" => EncKind::Utf16Le,
        "utf-16be" | "utf16be" => EncKind::Utf16Be,
        _ => return None,
    };
    Some(Encoder {
        name: name.to_vec(),
        kind,
        cache: HashMap::new(),
    })
}

/// Decode one UTF-8 character at the start of `s`. Returns (codepoint, len),
/// `Err(true)` for an incomplete sequence and `Err(false)` for invalid input.
pub fn utf8_char(s: &[u8]) -> Result<(u32, usize), bool> {
    let c = s[0];
    let len = if c < 0x80 {
        return Ok((c as u32, 1));
    } else if c & 0xE0 == 0xC0 {
        2
    } else if c & 0xF0 == 0xE0 {
        3
    } else if c & 0xF8 == 0xF0 {
        4
    } else {
        return Err(false);
    };
    if s.len() < len {
        // Incomplete only if the bytes present are valid continuations.
        if s[1..].iter().all(|b| b & 0xC0 == 0x80) {
            return Err(true);
        }
        return Err(false);
    }
    match std::str::from_utf8(&s[..len]) {
        Ok(st) => Ok((st.chars().next().unwrap() as u32, len)),
        Err(_) => Err(false),
    }
}

// ── Output buffer ───────────────────────────────────────────────────────────

pub struct OutBuf {
    /// UTF-8 data written by the writer, not yet converted / handed out.
    pub buffer: Vec<u8>,
    /// Encoded data (only present while an encoder is installed).
    pub conv: Option<Vec<u8>>,
    pub encoder: Option<Encoder>,
    /// Latched write error: every further write fails.
    pub error: bool,
}

impl OutBuf {
    fn new() -> Self {
        OutBuf {
            buffer: Vec::new(),
            conv: None,
            encoder: None,
            error: false,
        }
    }

    pub fn write(&mut self, data: &[u8]) -> i32 {
        if self.error {
            return -1;
        }
        self.buffer.extend_from_slice(data);
        data.len() as i32
    }

    fn set_encoder(&mut self, enc: Option<Encoder>) {
        match enc {
            Some(e) => {
                let conv = self.conv.get_or_insert_with(Vec::new);
                if matches!(e.kind, EncKind::Utf16) {
                    conv.extend_from_slice(&[0xFF, 0xFE]);
                }
                self.encoder = Some(e);
            }
            None => {
                self.encoder = None;
                self.conv = None;
            }
        }
    }
}

// ── The writer ──────────────────────────────────────────────────────────────

pub struct TextWriter {
    pub out: OutBuf,
    nodes: Vec<Node>,
    nsstack: Option<Vec<NsEntry>>,
    indent: bool,
    doindent: bool,
    ichar: Vec<u8>,
    qchar: u8,
    /// Whether the internal document got an encoding (startDocument with an
    /// encoder); without one, non-ASCII attribute content is escaped.
    doc_has_encoding: bool,
    next_node_id: u64,
    /// libxml2 errors raised during the current operation.
    pub errors: Vec<String>,
    /// Set by endDocument: the caller must flush the output buffer.
    pub flush_requested: bool,
}

/// Escaping tables of libxml2's xmlEscapeText / xmlSerializeText.
fn escape_entry(c: u8, attr: bool) -> Option<&'static [u8]> {
    match c {
        b'\t' if attr => Some(b"&#9;"),
        b'\n' if attr => Some(b"&#10;"),
        b'\t' | b'\n' => None,
        b'\r' => Some(b"&#13;"),
        0x01..=0x1F => Some(b"&#xFFFD;"),
        b'"' => Some(b"&quot;"),
        b'&' => Some(b"&amp;"),
        b'<' => Some(b"&lt;"),
        b'>' => Some(b"&gt;"),
        _ => None,
    }
}

fn hex_charref(out: &mut Vec<u8>, val: u32) {
    out.extend_from_slice(format!("&#x{:X};", val).as_bytes());
}

/// xmlEscapeText(content, XML_ESCAPE_QUOT) — text content.
fn escape_text(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for &c in s {
        match escape_entry(c, false) {
            Some(r) => out.extend_from_slice(r),
            None => out.push(c),
        }
    }
    out
}

/// xmlBufAttrSerializeTxtContent — attribute values.
fn escape_attr(s: &[u8], non_ascii: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c < 0x80 {
            match escape_entry(c, true) {
                Some(r) => out.extend_from_slice(r),
                None => out.push(c),
            }
            i += 1;
        } else if non_ascii {
            match utf8_char(&s[i..]) {
                Ok((mut v, len)) => {
                    if v == 0xFFFE || v == 0xFFFF {
                        v = 0xFFFD;
                    }
                    hex_charref(&mut out, v);
                    i += len;
                }
                Err(_) => {
                    hex_charref(&mut out, 0xFFFD);
                    i += 1;
                }
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

impl TextWriter {
    pub fn new(indent_default: bool) -> Self {
        TextWriter {
            out: OutBuf::new(),
            nodes: Vec::new(),
            nsstack: Some(Vec::new()),
            indent: indent_default,
            doindent: false,
            ichar: b" ".to_vec(),
            qchar: b'"',
            doc_has_encoding: false,
            next_node_id: 1,
            errors: Vec::new(),
            flush_requested: false,
        }
    }

    fn err(&mut self, msg: &str) {
        self.errors.push(msg.to_string());
    }

    fn w(&mut self, data: &[u8]) -> i32 {
        self.out.write(data)
    }

    fn q(&mut self) -> i32 {
        let q = [self.qchar];
        self.out.write(&q)
    }

    fn front_state(&self) -> Option<St> {
        self.nodes.last().map(|n| n.state)
    }

    fn set_front_state(&mut self, st: St) {
        if let Some(n) = self.nodes.last_mut() {
            n.state = st;
        }
    }

    fn push_node(&mut self, name: &[u8], state: St) {
        let id = self.next_node_id;
        self.next_node_id += 1;
        self.nodes.push(Node {
            name: name.to_vec(),
            state,
            id,
        });
    }

    fn write_indent(&mut self) -> i32 {
        let lksize = self.nodes.len();
        if lksize < 1 {
            return -1;
        }
        for _ in 0..lksize - 1 {
            let ichar = self.ichar.clone();
            if self.w(&ichar) == -1 {
                return -1;
            }
        }
        (lksize - 1) as i32
    }

    fn output_nsdecl(&mut self) -> i32 {
        if self.nsstack.is_none() {
            return -1;
        }
        let mut sum = 0;
        while let Some(np) = self.nsstack.as_mut().and_then(|s| s.pop()) {
            let count = self.write_attribute(&np.prefix, &np.uri);
            if count < 0 {
                self.nsstack = None;
                return -1;
            }
            sum += count;
        }
        sum
    }

    fn handle_state_dependencies(&mut self) -> i32 {
        let st = match self.front_state() {
            None => return 0,
            Some(s) => s,
        };
        let mut sum = 0;
        let mut extra: Vec<u8> = Vec::new();
        match st {
            St::Name => {
                let count = self.output_nsdecl();
                if count < 0 {
                    return -1;
                }
                sum += count;
                extra.push(b'>');
                self.set_front_state(St::Text);
            }
            St::Pi => {
                extra.push(b' ');
                self.set_front_state(St::PiText);
            }
            St::Dtd => {
                extra.extend_from_slice(b" [");
                self.set_front_state(St::DtdText);
            }
            St::DtdElem => {
                extra.push(b' ');
                self.set_front_state(St::DtdElemText);
            }
            St::DtdAttl => {
                extra.push(b' ');
                self.set_front_state(St::DtdAttlText);
            }
            St::DtdEnty | St::DtdPent => {
                extra.push(b' ');
                extra.push(self.qchar);
                self.set_front_state(St::DtdEntyText);
            }
            _ => {}
        }
        if !extra.is_empty() {
            let count = self.w(&extra);
            if count < 0 {
                return -1;
            }
            sum += count;
        }
        sum
    }

    // ── Document ──

    /// `enc`: None = no encoding requested; Some(None) = unsupported encoding.
    pub fn start_document(
        &mut self,
        version: Option<&[u8]>,
        enc: Option<Option<Encoder>>,
        standalone: Option<&[u8]>,
    ) -> i32 {
        if !self.nodes.is_empty() {
            self.err("xmlTextWriterStartDocument : not allowed in this context!");
            return -1;
        }
        let encoder = match enc {
            None => None,
            Some(None) => {
                self.err("xmlTextWriterStartDocument : unsupported encoding");
                return -1;
            }
            Some(Some(e)) => Some(e),
        };
        if encoder.is_some() {
            self.doc_has_encoding = true;
        }
        self.out.set_encoder(encoder);
        let mut sum = 0;
        macro_rules! wr {
            ($e:expr) => {{
                let c = $e;
                if c < 0 {
                    return -1;
                }
                sum += c;
            }};
        }
        wr!(self.w(b"<?xml version="));
        wr!(self.q());
        wr!(self.w(version.unwrap_or(b"1.0")));
        wr!(self.q());
        if let Some(name) = self.out.encoder.as_ref().map(|e| e.name.clone()) {
            wr!(self.w(b" encoding="));
            wr!(self.q());
            wr!(self.w(&name));
            wr!(self.q());
        }
        if let Some(sa) = standalone {
            wr!(self.w(b" standalone="));
            wr!(self.q());
            wr!(self.w(sa));
            wr!(self.q());
        }
        wr!(self.w(b"?>\n"));
        sum
    }

    pub fn end_document(&mut self) -> i32 {
        let mut sum = 0;
        while let Some(st) = self.front_state() {
            let count = match st {
                St::Name | St::Attribute | St::Text => self.end_element(),
                St::Pi | St::PiText => self.end_pi(),
                St::Cdata => self.end_cdata(),
                St::Dtd
                | St::DtdText
                | St::DtdElem
                | St::DtdElemText
                | St::DtdAttl
                | St::DtdAttlText
                | St::DtdEnty
                | St::DtdEntyText
                | St::DtdPent => self.end_dtd(),
                St::Comment => self.end_comment(),
            };
            if count < 0 {
                return -1;
            }
            sum += count;
        }
        if !self.indent {
            let count = self.w(b"\n");
            if count < 0 {
                return -1;
            }
            sum += count;
        }
        self.flush_requested = true;
        sum
    }

    // ── Comments ──

    pub fn start_comment(&mut self) -> i32 {
        let mut sum = 0;
        if let Some(st) = self.front_state() {
            match st {
                St::Text => {}
                St::Name => {
                    let count = self.output_nsdecl();
                    if count < 0 {
                        return -1;
                    }
                    sum += count;
                    let count = self.w(b">");
                    if count < 0 {
                        return -1;
                    }
                    sum += count;
                    if self.indent {
                        let count = self.w(b"\n");
                        if count < 0 {
                            return -1;
                        }
                        sum += count;
                    }
                    self.set_front_state(St::Text);
                }
                _ => return -1,
            }
        }
        self.push_node(b"", St::Comment);
        if self.indent {
            let count = self.write_indent();
            if count < 0 {
                return -1;
            }
            sum += count;
        }
        let count = self.w(b"<!--");
        if count < 0 {
            return -1;
        }
        sum + count
    }

    pub fn end_comment(&mut self) -> i32 {
        let mut sum = match self.front_state() {
            Some(St::Comment) => {
                let c = self.w(b"-->");
                if c < 0 {
                    return -1;
                }
                c
            }
            _ => return -1,
        };
        if self.indent {
            let c = self.w(b"\n");
            if c < 0 {
                return -1;
            }
            sum += c;
        }
        self.nodes.pop();
        sum
    }

    pub fn write_comment(&mut self, content: &[u8]) -> i32 {
        let a = self.start_comment();
        if a < 0 {
            return -1;
        }
        let b = self.write_string(content);
        if b < 0 {
            return -1;
        }
        let c = self.end_comment();
        if c < 0 {
            return -1;
        }
        a + b + c
    }

    // ── Elements ──

    /// Close a pending start tag (state Attribute/Name) before new content.
    fn close_start_tag(&mut self, st: St, newline: bool) -> i32 {
        let mut sum = 0;
        if st == St::Attribute {
            let count = self.end_attribute();
            if count < 0 {
                return -1;
            }
            sum += count;
        }
        let count = self.output_nsdecl();
        if count < 0 {
            return -1;
        }
        sum += count;
        let count = self.w(b">");
        if count < 0 {
            return -1;
        }
        sum += count;
        if newline && self.indent {
            self.w(b"\n");
        }
        self.set_front_state(St::Text);
        sum
    }

    pub fn start_element(&mut self, name: &[u8]) -> i32 {
        if name.is_empty() {
            return -1;
        }
        let mut sum = 0;
        if let Some(st) = self.front_state() {
            match st {
                St::Pi | St::PiText => return -1,
                St::Attribute | St::Name => {
                    let count = self.close_start_tag(st, true);
                    if count < 0 {
                        return -1;
                    }
                    sum += count;
                }
                _ => {}
            }
        }
        self.push_node(name, St::Name);
        if self.indent {
            sum += self.write_indent();
        }
        let count = self.w(b"<");
        if count < 0 {
            return -1;
        }
        sum += count;
        let count = self.w(name);
        if count < 0 {
            return -1;
        }
        sum + count
    }

    pub fn start_element_ns(
        &mut self,
        prefix: Option<&[u8]>,
        name: &[u8],
        uri: Option<&[u8]>,
    ) -> i32 {
        if self.nsstack.is_none() || name.is_empty() {
            return -1;
        }
        let mut buf = Vec::new();
        if let Some(p) = prefix {
            buf.extend_from_slice(p);
            buf.push(b':');
        }
        buf.extend_from_slice(name);
        let count = self.start_element(&buf);
        if count < 0 {
            return -1;
        }
        if let Some(u) = uri {
            let mut p = b"xmlns".to_vec();
            if let Some(pr) = prefix {
                p.push(b':');
                p.extend_from_slice(pr);
            }
            let elem = self.nodes.last().map(|n| n.id);
            if let Some(ns) = self.nsstack.as_mut() {
                ns.push(NsEntry {
                    prefix: p,
                    uri: u.to_vec(),
                    elem,
                });
            }
        }
        count
    }

    fn write_end_tag(&mut self) -> i32 {
        let mut sum = 0;
        if self.indent && self.doindent {
            sum += self.write_indent();
        }
        self.doindent = true;
        let name = self
            .nodes
            .last()
            .map(|n| n.name.clone())
            .unwrap_or_default();
        for part in [&b"</"[..], &name, b">"] {
            let count = self.w(part);
            if count < 0 {
                return -1;
            }
            sum += count;
        }
        sum
    }

    pub fn end_element(&mut self) -> i32 {
        let st = match self.front_state() {
            None => {
                self.nsstack = None;
                return -1;
            }
            Some(s) => s,
        };
        let mut sum = 0;
        match st {
            St::Attribute | St::Name => {
                if st == St::Attribute {
                    let count = self.end_attribute();
                    if count < 0 {
                        self.nsstack = None;
                        return -1;
                    }
                    sum += count;
                }
                let count = self.output_nsdecl();
                if count < 0 {
                    return -1;
                }
                sum += count;
                if self.indent {
                    self.doindent = true;
                }
                let count = self.w(b"/>");
                if count < 0 {
                    return -1;
                }
                sum += count;
            }
            St::Text => {
                let count = self.write_end_tag();
                if count < 0 {
                    return -1;
                }
                sum += count;
            }
            _ => return -1,
        }
        if self.indent {
            sum += self.w(b"\n");
        }
        self.nodes.pop();
        sum
    }

    pub fn full_end_element(&mut self) -> i32 {
        let st = match self.front_state() {
            None => return -1,
            Some(s) => s,
        };
        let mut sum = 0;
        match st {
            St::Attribute | St::Name | St::Text => {
                if st != St::Text {
                    if st == St::Attribute {
                        let count = self.end_attribute();
                        if count < 0 {
                            return -1;
                        }
                        sum += count;
                    }
                    let count = self.output_nsdecl();
                    if count < 0 {
                        return -1;
                    }
                    sum += count;
                    let count = self.w(b">");
                    if count < 0 {
                        return -1;
                    }
                    sum += count;
                    if self.indent {
                        self.doindent = false;
                    }
                }
                let count = self.write_end_tag();
                if count < 0 {
                    return -1;
                }
                sum += count;
            }
            _ => return -1,
        }
        if self.indent {
            sum += self.w(b"\n");
        }
        self.nodes.pop();
        sum
    }

    pub fn write_element(&mut self, name: &[u8], content: Option<&[u8]>) -> i32 {
        let a = self.start_element(name);
        if a == -1 {
            return -1;
        }
        let mut b = 0;
        if let Some(c) = content {
            b = self.write_string(c);
            if b == -1 {
                return -1;
            }
        }
        let c = self.end_element();
        if c == -1 {
            return -1;
        }
        a + b + c
    }

    pub fn write_element_ns(
        &mut self,
        prefix: Option<&[u8]>,
        name: &[u8],
        uri: Option<&[u8]>,
        content: &[u8],
    ) -> i32 {
        if name.is_empty() {
            return -1;
        }
        let a = self.start_element_ns(prefix, name, uri);
        if a < 0 {
            return -1;
        }
        let b = self.write_string(content);
        if b == -1 {
            return -1;
        }
        let c = self.end_element();
        if c == -1 {
            return -1;
        }
        a + b + c
    }

    // ── Text ──

    pub fn write_raw(&mut self, content: &[u8]) -> i32 {
        let mut sum = 0;
        if !self.nodes.is_empty() {
            let count = self.handle_state_dependencies();
            if count < 0 {
                return -1;
            }
            sum += count;
        }
        if self.indent {
            self.doindent = false;
        }
        let count = self.w(content);
        if count < 0 {
            return -1;
        }
        sum + count
    }

    pub fn write_string(&mut self, content: &[u8]) -> i32 {
        let buf: Option<Vec<u8>> = match self.front_state() {
            Some(St::Name) | Some(St::Text) => Some(escape_text(content)),
            Some(St::Attribute) => {
                let esc = escape_attr(content, !self.doc_has_encoding);
                self.w(&esc);
                None
            }
            _ => Some(content.to_vec()),
        };
        match buf {
            Some(b) => {
                let count = self.write_raw(&b);
                if count < 0 {
                    return -1;
                }
                count
            }
            None => 0,
        }
    }

    // ── Attributes ──

    pub fn start_attribute(&mut self, name: &[u8]) -> i32 {
        if name.is_empty() {
            return -1;
        }
        let st = match self.front_state() {
            None => return -1,
            Some(s) => s,
        };
        let mut sum = 0;
        match st {
            St::Attribute | St::Name => {
                if st == St::Attribute {
                    let count = self.end_attribute();
                    if count < 0 {
                        return -1;
                    }
                    sum += count;
                }
                for part in [&b" "[..], name, b"="] {
                    let count = self.w(part);
                    if count < 0 {
                        return -1;
                    }
                    sum += count;
                }
                let count = self.q();
                if count < 0 {
                    return -1;
                }
                sum += count;
                self.set_front_state(St::Attribute);
            }
            _ => return -1,
        }
        sum
    }

    pub fn start_attribute_ns(
        &mut self,
        prefix: Option<&[u8]>,
        name: &[u8],
        uri: Option<&[u8]>,
    ) -> i32 {
        if self.nsstack.is_none() || name.is_empty() {
            return -1;
        }
        if let Some(u) = uri {
            let mut p = b"xmlns".to_vec();
            if let Some(pr) = prefix {
                p.push(b':');
                p.extend_from_slice(pr);
            }
            let elem = self.nodes.last().map(|n| n.id);
            let ns = self.nsstack.as_mut().unwrap();
            // xmlListSearch walks from the front (most recent entry).
            match ns.iter().rev().find(|e| e.prefix == p && e.elem == elem) {
                Some(cur) => {
                    if cur.uri != u {
                        return -1;
                    }
                }
                None => ns.push(NsEntry {
                    prefix: p,
                    uri: u.to_vec(),
                    elem,
                }),
            }
        }
        let mut buf = Vec::new();
        if let Some(p) = prefix {
            buf.extend_from_slice(p);
            buf.push(b':');
        }
        buf.extend_from_slice(name);
        let count = self.start_attribute(&buf);
        if count < 0 {
            return -1;
        }
        count
    }

    pub fn end_attribute(&mut self) -> i32 {
        match self.front_state() {
            Some(St::Attribute) => {
                self.set_front_state(St::Name);
                let c = self.q();
                if c < 0 {
                    return -1;
                }
                c
            }
            _ => -1,
        }
    }

    pub fn write_attribute(&mut self, name: &[u8], content: &[u8]) -> i32 {
        let a = self.start_attribute(name);
        if a < 0 {
            return -1;
        }
        let b = self.write_string(content);
        if b < 0 {
            return -1;
        }
        let c = self.end_attribute();
        if c < 0 {
            return -1;
        }
        a + b + c
    }

    pub fn write_attribute_ns(
        &mut self,
        prefix: Option<&[u8]>,
        name: &[u8],
        uri: Option<&[u8]>,
        content: &[u8],
    ) -> i32 {
        if name.is_empty() {
            return -1;
        }
        let a = self.start_attribute_ns(prefix, name, uri);
        if a < 0 {
            return -1;
        }
        let b = self.write_string(content);
        if b < 0 {
            return -1;
        }
        let c = self.end_attribute();
        if c < 0 {
            return -1;
        }
        a + b + c
    }

    // ── Processing instructions ──

    pub fn start_pi(&mut self, target: &[u8]) -> i32 {
        if target.is_empty() {
            return -1;
        }
        if target.eq_ignore_ascii_case(b"xml") {
            self.err("xmlTextWriterStartPI : target name [Xx][Mm][Ll] is reserved for xml standardization!");
            return -1;
        }
        let mut sum = 0;
        if let Some(st) = self.front_state() {
            match st {
                St::Attribute | St::Name => {
                    let count = self.close_start_tag(st, false);
                    if count < 0 {
                        return -1;
                    }
                    sum += count;
                }
                St::Text | St::Dtd => {}
                St::Pi | St::PiText => {
                    self.err("xmlTextWriterStartPI : nested PI!");
                    return -1;
                }
                _ => return -1,
            }
        }
        self.push_node(target, St::Pi);
        for part in [&b"<?"[..], target] {
            let count = self.w(part);
            if count < 0 {
                return -1;
            }
            sum += count;
        }
        sum
    }

    pub fn end_pi(&mut self) -> i32 {
        let mut sum = match self.front_state() {
            None => return 0,
            Some(St::Pi) | Some(St::PiText) => {
                let c = self.w(b"?>");
                if c < 0 {
                    return -1;
                }
                c
            }
            _ => return -1,
        };
        if self.indent {
            let c = self.w(b"\n");
            if c < 0 {
                return -1;
            }
            sum += c;
        }
        self.nodes.pop();
        sum
    }

    pub fn write_pi(&mut self, target: &[u8], content: &[u8]) -> i32 {
        let a = self.start_pi(target);
        if a == -1 {
            return -1;
        }
        let b = self.write_string(content);
        if b == -1 {
            return -1;
        }
        let c = self.end_pi();
        if c == -1 {
            return -1;
        }
        a + b + c
    }

    // ── CDATA ──

    pub fn start_cdata(&mut self) -> i32 {
        let mut sum = 0;
        if let Some(st) = self.front_state() {
            match st {
                St::Text | St::Pi | St::PiText => {}
                St::Attribute | St::Name => {
                    let count = self.close_start_tag(st, false);
                    if count < 0 {
                        return -1;
                    }
                    sum += count;
                }
                St::Cdata => {
                    self.err("xmlTextWriterStartCDATA : CDATA not allowed in this context!");
                    return -1;
                }
                _ => return -1,
            }
        }
        self.push_node(b"", St::Cdata);
        let count = self.w(b"<![CDATA[");
        if count < 0 {
            return -1;
        }
        sum + count
    }

    pub fn end_cdata(&mut self) -> i32 {
        match self.front_state() {
            Some(St::Cdata) => {
                let c = self.w(b"]]>");
                if c < 0 {
                    return -1;
                }
                self.nodes.pop();
                c
            }
            _ => -1,
        }
    }

    pub fn write_cdata(&mut self, content: &[u8]) -> i32 {
        let a = self.start_cdata();
        if a == -1 {
            return -1;
        }
        let b = self.write_string(content);
        if b == -1 {
            return -1;
        }
        let c = self.end_cdata();
        if c == -1 {
            return -1;
        }
        a + b + c
    }

    // ── DTD ──

    pub fn start_dtd(&mut self, name: &[u8], pubid: Option<&[u8]>, sysid: Option<&[u8]>) -> i32 {
        if name.is_empty() {
            return -1;
        }
        if !self.nodes.is_empty() {
            self.err("xmlTextWriterStartDTD : DTD allowed only in prolog!");
            return -1;
        }
        self.push_node(name, St::Dtd);
        let mut sum = 0;
        macro_rules! wr {
            ($e:expr) => {{
                let c = $e;
                if c < 0 {
                    return -1;
                }
                sum += c;
            }};
        }
        wr!(self.w(b"<!DOCTYPE "));
        wr!(self.w(name));
        if let Some(p) = pubid {
            if sysid.is_none() {
                self.err("xmlTextWriterStartDTD : system identifier needed!");
                return -1;
            }
            wr!(self.w(if self.indent { b"\n" } else { b" " }));
            wr!(self.w(b"PUBLIC "));
            wr!(self.q());
            wr!(self.w(p));
            wr!(self.q());
        }
        if let Some(s) = sysid {
            if pubid.is_none() {
                wr!(self.w(if self.indent { b"\n" } else { b" " }));
                wr!(self.w(b"SYSTEM "));
            } else {
                wr!(self.w(if self.indent { &b"\n       "[..] } else { b" " }));
            }
            wr!(self.q());
            wr!(self.w(s));
            wr!(self.q());
        }
        sum
    }

    pub fn end_dtd(&mut self) -> i32 {
        let mut sum = 0;
        while let Some(st) = self.front_state() {
            let count = match st {
                St::DtdText | St::Dtd => {
                    if st == St::DtdText {
                        let c = self.w(b"]");
                        if c < 0 {
                            return -1;
                        }
                        sum += c;
                    }
                    let mut c = self.w(b">");
                    if self.indent {
                        if c < 0 {
                            return -1;
                        }
                        sum += c;
                        c = self.w(b"\n");
                    }
                    self.nodes.pop();
                    c
                }
                St::DtdElem | St::DtdElemText => self.end_dtd_element(),
                St::DtdAttl | St::DtdAttlText => self.end_dtd_attlist(),
                St::DtdEnty | St::DtdPent | St::DtdEntyText => self.end_dtd_entity(),
                St::Comment => self.end_comment(),
                _ => break,
            };
            if count < 0 {
                return -1;
            }
            sum += count;
        }
        sum
    }

    pub fn write_dtd(
        &mut self,
        name: &[u8],
        pubid: Option<&[u8]>,
        sysid: Option<&[u8]>,
        subset: Option<&[u8]>,
    ) -> i32 {
        let a = self.start_dtd(name, pubid, sysid);
        if a == -1 {
            return -1;
        }
        let mut b = 0;
        if let Some(s) = subset {
            b = self.write_string(s);
            if b == -1 {
                return -1;
            }
        }
        let c = self.end_dtd();
        if c == -1 {
            return -1;
        }
        a + b + c
    }

    /// Common prologue of the DTD declarations: open the internal subset.
    fn open_internal_subset(&mut self, allow_none: bool) -> i32 {
        let mut sum = 0;
        if let Some(st) = self.front_state() {
            match st {
                St::Dtd => {
                    let c = self.w(b" [");
                    if c < 0 {
                        return -1;
                    }
                    sum += c;
                    if self.indent {
                        let c = self.w(b"\n");
                        if c < 0 {
                            return -1;
                        }
                        sum += c;
                    }
                    self.set_front_state(St::DtdText);
                }
                St::DtdText => {}
                _ => return -1,
            }
        } else if !allow_none {
            return -1;
        }
        sum
    }

    fn start_dtd_decl(&mut self, name: &[u8], state: St, keyword: &[u8], pe: bool) -> i32 {
        if name.is_empty() {
            return -1;
        }
        let mut sum = self.open_internal_subset(true);
        if sum < 0 {
            return -1;
        }
        self.push_node(name, state);
        if self.indent {
            let c = self.write_indent();
            if c < 0 {
                return -1;
            }
            sum += c;
        }
        let c = self.w(keyword);
        if c < 0 {
            return -1;
        }
        sum += c;
        if pe {
            let c = self.w(b"% ");
            if c < 0 {
                return -1;
            }
            sum += c;
        }
        let c = self.w(name);
        if c < 0 {
            return -1;
        }
        sum + c
    }

    fn end_dtd_decl(&mut self, states: &[St], quote_state: Option<St>) -> i32 {
        let st = match self.front_state() {
            None => return -1,
            Some(s) => s,
        };
        let mut sum = 0;
        if Some(st) == quote_state {
            let c = self.q();
            if c < 0 {
                return -1;
            }
            sum += c;
        } else if !states.contains(&st) {
            return -1;
        }
        let c = self.w(b">");
        if c < 0 {
            return -1;
        }
        sum += c;
        if self.indent {
            let c = self.w(b"\n");
            if c < 0 {
                return -1;
            }
            sum += c;
        }
        self.nodes.pop();
        sum
    }

    pub fn start_dtd_element(&mut self, name: &[u8]) -> i32 {
        self.start_dtd_decl(name, St::DtdElem, b"<!ELEMENT ", false)
    }

    pub fn end_dtd_element(&mut self) -> i32 {
        self.end_dtd_decl(&[St::DtdElem, St::DtdElemText], None)
    }

    pub fn write_dtd_element(&mut self, name: &[u8], content: &[u8]) -> i32 {
        let a = self.start_dtd_element(name);
        if a == -1 {
            return -1;
        }
        let b = self.write_string(content);
        if b == -1 {
            return -1;
        }
        let c = self.end_dtd_element();
        if c == -1 {
            return -1;
        }
        a + b + c
    }

    pub fn start_dtd_attlist(&mut self, name: &[u8]) -> i32 {
        self.start_dtd_decl(name, St::DtdAttl, b"<!ATTLIST ", false)
    }

    pub fn end_dtd_attlist(&mut self) -> i32 {
        self.end_dtd_decl(&[St::DtdAttl, St::DtdAttlText], None)
    }

    pub fn write_dtd_attlist(&mut self, name: &[u8], content: &[u8]) -> i32 {
        let a = self.start_dtd_attlist(name);
        if a == -1 {
            return -1;
        }
        let b = self.write_string(content);
        if b == -1 {
            return -1;
        }
        let c = self.end_dtd_attlist();
        if c == -1 {
            return -1;
        }
        a + b + c
    }

    pub fn start_dtd_entity(&mut self, pe: bool, name: &[u8]) -> i32 {
        self.start_dtd_decl(
            name,
            if pe { St::DtdPent } else { St::DtdEnty },
            b"<!ENTITY ",
            pe,
        )
    }

    pub fn end_dtd_entity(&mut self) -> i32 {
        self.end_dtd_decl(&[St::DtdEnty, St::DtdPent], Some(St::DtdEntyText))
    }

    pub fn write_dtd_entity(
        &mut self,
        pe: bool,
        name: &[u8],
        pubid: Option<&[u8]>,
        sysid: Option<&[u8]>,
        ndataid: Option<&[u8]>,
        content: &[u8],
    ) -> i32 {
        if pe && ndataid.is_some() {
            return -1;
        }
        if pubid.is_none() && sysid.is_none() {
            // xmlTextWriterWriteDTDInternalEntity
            if name.is_empty() {
                return -1;
            }
            let a = self.start_dtd_entity(pe, name);
            if a == -1 {
                return -1;
            }
            let b = self.write_string(content);
            if b == -1 {
                return -1;
            }
            let c = self.end_dtd_entity();
            if c == -1 {
                return -1;
            }
            return a + b + c;
        }
        // xmlTextWriterWriteDTDExternalEntity
        let a = self.start_dtd_entity(pe, name);
        if a == -1 {
            return -1;
        }
        let b = self.write_dtd_external_entity_contents(pubid, sysid, ndataid);
        if b < 0 {
            return -1;
        }
        let c = self.end_dtd_entity();
        if c == -1 {
            return -1;
        }
        a + b + c
    }

    fn write_dtd_external_entity_contents(
        &mut self,
        pubid: Option<&[u8]>,
        sysid: Option<&[u8]>,
        ndataid: Option<&[u8]>,
    ) -> i32 {
        const NEED_START: &str = "xmlTextWriterWriteDTDExternalEntityContents: you must call xmlTextWriterStartDTDEntity before the call to this function!";
        match self.front_state() {
            None => return -1,
            Some(St::DtdEnty) => {}
            Some(St::DtdPent) => {
                if ndataid.is_some() {
                    self.err("xmlTextWriterWriteDTDExternalEntityContents: notation not allowed with parameter entities!");
                    return -1;
                }
            }
            _ => {
                self.err(NEED_START);
                return -1;
            }
        }
        let mut sum = 0;
        macro_rules! wr {
            ($e:expr) => {{
                let c = $e;
                if c < 0 {
                    return -1;
                }
                sum += c;
            }};
        }
        if let Some(p) = pubid {
            if sysid.is_none() {
                self.err("xmlTextWriterWriteDTDExternalEntityContents: system identifier needed!");
                return -1;
            }
            wr!(self.w(b" PUBLIC "));
            wr!(self.q());
            wr!(self.w(p));
            wr!(self.q());
        }
        if let Some(s) = sysid {
            if pubid.is_none() {
                wr!(self.w(b" SYSTEM"));
            }
            wr!(self.w(b" "));
            wr!(self.q());
            wr!(self.w(s));
            wr!(self.q());
        }
        if let Some(n) = ndataid {
            wr!(self.w(b" NDATA "));
            wr!(self.w(n));
        }
        sum
    }

    // ── Settings ──

    pub fn set_indent(&mut self, indent: bool) -> i32 {
        self.indent = indent;
        self.doindent = true;
        0
    }

    pub fn set_indent_string(&mut self, s: &[u8]) -> i32 {
        self.ichar = s.to_vec();
        0
    }
}

// ── Name validation (xmlValidateName with space = 0) ────────────────────────

fn is_name_start_char(c: u32) -> bool {
    matches!(c,
        0x61..=0x7A | 0x41..=0x5A | 0x5F | 0x3A
        | 0xC0..=0xD6 | 0xD8..=0xF6 | 0xF8..=0x2FF | 0x370..=0x37D | 0x37F..=0x1FFF
        | 0x200C..=0x200D | 0x2070..=0x218F | 0x2C00..=0x2FEF | 0x3001..=0xD7FF
        | 0xF900..=0xFDCF | 0xFDF0..=0xFFFD | 0x10000..=0xEFFFF)
}

fn is_name_char(c: u32) -> bool {
    is_name_start_char(c)
        || matches!(c, 0x30..=0x39 | 0x2D | 0x2E | 0xB7 | 0x300..=0x36F | 0x203F..=0x2040)
}

/// True when `name` (a C string, i.e. already cut at the first NUL) is a
/// valid XML Name.
pub fn valid_name(name: &[u8]) -> bool {
    let mut i = 0;
    let mut first = true;
    while i < name.len() {
        let (c, len) = match utf8_char(&name[i..]) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let ok = if first {
            is_name_start_char(c)
        } else {
            is_name_char(c)
        };
        if !ok {
            return false;
        }
        first = false;
        i += len;
    }
    !first
}
