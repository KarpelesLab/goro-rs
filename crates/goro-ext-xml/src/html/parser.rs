//! Port of libxml2 2.13's legacy HTML parser (HTMLparser.c: htmlParseDocument
//! and the non-recursive htmlParseContentInternal), together with the parts
//! of SAX2.c that build the tree for HTML documents.
//!
//! The input is kept in a single byte buffer. Until an encoding is known the
//! buffer holds the raw bytes; when the parser switches encoding (meta
//! charset, the first non-ASCII byte, a BOM...) the not yet consumed part of
//! the buffer is converted to UTF-8 in place, which is what libxml2 does with
//! its raw/decoded input buffers.

use super::tables;
use crate::parser::{
    ERR_ERROR, ERR_FATAL, ERR_WARNING, FROM_IO, FROM_PARSER, FROM_VALID, XmlError, decode_to_utf8,
};
use crate::tree::{NType, NodeId, Tree};

pub const FROM_HTML: i32 = 5;

pub const HTML_PARSE_RECOVER: i64 = 1;
pub const HTML_PARSE_NODEFDTD: i64 = 4;
pub const HTML_PARSE_NOERROR: i64 = 32;
pub const HTML_PARSE_NOWARNING: i64 = 64;
pub const HTML_PARSE_PEDANTIC: i64 = 128;
pub const HTML_PARSE_NOBLANKS: i64 = 256;
pub const HTML_PARSE_NONET: i64 = 2048;
pub const HTML_PARSE_NOIMPLIED: i64 = 8192;
pub const HTML_PARSE_COMPACT: i64 = 65536;
pub const HTML_PARSE_HUGE: i64 = 524288;
pub const HTML_PARSE_IGNORE_ENC: i64 = 1 << 21;
pub const HTML_PARSE_BIG_LINES: i64 = 1 << 22;

// error codes (xmlerror.h)
const XML_ERR_INTERNAL_ERROR: i32 = 1;
const XML_ERR_DOCUMENT_EMPTY: i32 = 4;
const XML_ERR_INVALID_HEX_CHARREF: i32 = 6;
const XML_ERR_INVALID_DEC_CHARREF: i32 = 7;
const XML_ERR_INVALID_CHARREF: i32 = 8;
const XML_ERR_INVALID_CHAR: i32 = 9;
const XML_ERR_ENTITYREF_SEMICOL_MISSING: i32 = 23;
const XML_ERR_UNSUPPORTED_ENCODING: i32 = 32;
const XML_ERR_ATTRIBUTE_NOT_FINISHED: i32 = 40;
const XML_ERR_ATTRIBUTE_WITHOUT_VALUE: i32 = 41;
const XML_ERR_ATTRIBUTE_REDEFINED: i32 = 42;
const XML_ERR_LITERAL_NOT_STARTED: i32 = 43;
const XML_ERR_LITERAL_NOT_FINISHED: i32 = 44;
const XML_ERR_COMMENT_NOT_FINISHED: i32 = 45;
const XML_ERR_PI_NOT_STARTED: i32 = 46;
const XML_ERR_PI_NOT_FINISHED: i32 = 47;
const XML_ERR_DOCTYPE_NOT_FINISHED: i32 = 61;
const XML_ERR_SPACE_REQUIRED: i32 = 65;
const XML_ERR_NAME_REQUIRED: i32 = 68;
const XML_ERR_URI_REQUIRED: i32 = 70;
const XML_ERR_PUBID_REQUIRED: i32 = 71;
const XML_ERR_GT_REQUIRED: i32 = 73;
const XML_ERR_LTSLASH_REQUIRED: i32 = 74;
const XML_ERR_TAG_NAME_MISMATCH: i32 = 76;
const XML_ERR_INVALID_ENCODING: i32 = 81;
const XML_ERR_NAME_TOO_LONG: i32 = 110;
const XML_ERR_COMMENT_ABRUPTLY_ENDED: i32 = 112;
const XML_WAR_ENCODING_MISMATCH: i32 = 113;
const XML_DTD_ID_REDEFINED: i32 = 513;
const XML_HTML_STRUCURE_ERROR: i32 = 800;
const XML_HTML_UNKNOWN_TAG: i32 = 801;
const XML_HTML_INCORRECTLY_OPENED_COMMENT: i32 = 802;

const HTML_PARSER_BUFFER_SIZE: usize = 100;
const HTML_PARSER_BIG_BUFFER_SIZE: usize = 1000;
const XML_MAX_NAME_LENGTH: usize = 50000;
const XML_MAX_TEXT_LENGTH: usize = 10_000_000;
const XML_MAX_HUGE_LENGTH: usize = 1_000_000_000;
const XML_MAX_ERRORS: usize = 100;

/// Elements in which whitespace-only text is kept (areBlanks).
static ALLOW_PCDATA: &[&str] = &[
    "a",
    "abbr",
    "acronym",
    "address",
    "applet",
    "b",
    "bdo",
    "big",
    "blockquote",
    "body",
    "button",
    "caption",
    "center",
    "cite",
    "code",
    "dd",
    "del",
    "dfn",
    "div",
    "dt",
    "em",
    "font",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "i",
    "iframe",
    "ins",
    "kbd",
    "label",
    "legend",
    "li",
    "noframes",
    "noscript",
    "object",
    "p",
    "pre",
    "q",
    "s",
    "samp",
    "small",
    "span",
    "strike",
    "strong",
    "td",
    "th",
    "tt",
    "u",
    "var",
];

/// HTML element description (the fields of htmlElemDesc the parser and the
/// serializer use).
#[derive(Clone, Copy, Debug)]
pub struct ElemDesc {
    pub name: &'static str,
    pub start_tag: u8,
    pub end_tag: u8,
    pub save_end_tag: u8,
    pub empty: bool,
    pub depr: u8,
    pub dtd: u8,
    pub isinline: bool,
}

/// htmlTagLookup (case-insensitive).
pub fn tag_lookup(tag: &str) -> Option<ElemDesc> {
    let key = tag.as_bytes();
    let r = tables::ELEMENTS.binary_search_by(|e| {
        let a = e.0.as_bytes();
        // xmlStrcasecmp(tag, desc->name), reversed for binary_search_by
        let n = a.len().min(key.len());
        for i in 0..n {
            let x = a[i].to_ascii_lowercase();
            let y = key[i].to_ascii_lowercase();
            if x != y {
                return x.cmp(&y);
            }
        }
        a.len().cmp(&key.len())
    });
    r.ok().map(|i| {
        let e = tables::ELEMENTS[i];
        ElemDesc {
            name: e.0,
            start_tag: e.1,
            end_tag: e.2,
            save_end_tag: e.3,
            empty: e.4 != 0,
            depr: e.5,
            dtd: e.6,
            isinline: e.7 != 0,
        }
    })
}

/// htmlEntityLookup
pub fn entity_lookup(name: &[u8]) -> Option<u32> {
    tables::ENTITIES
        .iter()
        .find(|e| e.1.as_bytes() == name)
        .map(|e| e.0)
}

/// htmlEntityValueLookup
pub fn entity_value_lookup(v: u32) -> Option<&'static str> {
    tables::ENTITIES
        .binary_search_by(|e| e.0.cmp(&v))
        .ok()
        .map(|i| tables::ENTITIES[i].1)
}

/// htmlCheckAutoClose: does the start of `newtag` close `oldtag`?
fn check_auto_close(newtag: &str, oldtag: &str) -> bool {
    tables::START_CLOSE
        .binary_search_by(|e| (e.0, e.1).cmp(&(oldtag, newtag)))
        .is_ok()
}

/// htmlGetEndPriority
fn end_priority(name: &str) -> i32 {
    match name {
        "div" => 150,
        "td" | "th" => 160,
        "tr" => 170,
        "thead" | "tbody" | "tfoot" => 180,
        "table" => 190,
        "head" | "body" => 200,
        "html" => 220,
        _ => 100,
    }
}

/// htmlIsBooleanAttr
pub fn is_boolean_attr(name: &str) -> bool {
    const B: &[&str] = &[
        "checked", "compact", "declare", "defer", "disabled", "ismap", "multiple", "nohref",
        "noresize", "noshade", "nowrap", "readonly", "selected",
    ];
    B.iter().any(|b| b.eq_ignore_ascii_case(name))
}

fn is_char(c: u32) -> bool {
    crate::parser::is_char(c)
}

fn is_blank_ch(c: u8) -> bool {
    c == 0x20 || c == 0x9 || c == 0xA || c == 0xD
}

fn is_ascii_letter(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

/// IS_LETTER / IS_DIGIT / IS_COMBINING / IS_EXTENDER approximations for
/// non-ASCII characters (libxml2 uses the XML 1.0 4th edition tables).
fn is_letter(c: u32) -> bool {
    if c < 0x80 {
        return (c as u8).is_ascii_alphabetic();
    }
    char::from_u32(c)
        .map(|ch| ch.is_alphabetic())
        .unwrap_or(false)
        && c != 0xD7
        && c != 0xF7
}

fn is_digit(c: u32) -> bool {
    if c < 0x80 {
        return (c as u8).is_ascii_digit();
    }
    char::from_u32(c).map(|ch| ch.is_numeric()).unwrap_or(false)
}

fn is_combining(c: u32) -> bool {
    (0x300..=0x36F).contains(&c)
        || (0x483..=0x486).contains(&c)
        || (0x591..=0x5C4).contains(&c)
        || (0x64B..=0x652).contains(&c)
        || (0x93C..=0x94D).contains(&c)
        || (0x20D0..=0x20E1).contains(&c)
        || (0x302A..=0x302F).contains(&c)
        || c == 0x3099
        || c == 0x309A
}

fn is_extender(c: u32) -> bool {
    matches!(c, 0xB7 | 0x2D0 | 0x2D1 | 0x387 | 0x640 | 0xE46 | 0xEC6 | 0x3005 | 0x3031..=0x3035 | 0x309D..=0x309E | 0x30FC..=0x30FE)
}

fn is_pubid_char(c: u8) -> bool {
    matches!(c, 0x20 | 0xD | 0xA | b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'\'' | b'(' | b')' | b'+' | b',' | b'.' | b'/' | b':' | b'=' | b'?' | b';' | b'!' | b'*' | b'#' | b'@' | b'$' | b'_' | b'%')
}

fn push_utf8(buf: &mut Vec<u8>, c: u32) {
    // xmlCopyCharMultiByte / the inline encoders in HTMLparser.c
    if c < 0x80 {
        buf.push(c as u8);
    } else if c < 0x800 {
        buf.push((((c >> 6) & 0x1F) | 0xC0) as u8);
        buf.push(((c & 0x3F) | 0x80) as u8);
    } else if c < 0x10000 {
        buf.push((((c >> 12) & 0x0F) | 0xE0) as u8);
        buf.push((((c >> 6) & 0x3F) | 0x80) as u8);
        buf.push(((c & 0x3F) | 0x80) as u8);
    } else {
        buf.push((((c >> 18) & 0x07) | 0xF0) as u8);
        buf.push((((c >> 12) & 0x3F) | 0x80) as u8);
        buf.push((((c >> 6) & 0x3F) | 0x80) as u8);
        buf.push(((c & 0x3F) | 0x80) as u8);
    }
}

/// COPY_BUF: a one byte "char" is copied verbatim (this is how raw bytes
/// end up in the tree after an encoding error).
fn copy_buf(buf: &mut Vec<u8>, l: usize, c: u32) {
    if l == 1 {
        buf.push(c as u8);
    } else {
        push_utf8(buf, c);
    }
}

fn to_string(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => String::from_utf8_lossy(b).into_owned(),
    }
}

/// Case-insensitive substring search on the NUL-terminated part of `hay`
/// (xmlStrcasestr).
fn strcasestr(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let end = hay.iter().position(|&c| c == 0).unwrap_or(hay.len());
    let hay = &hay[..end];
    if needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

/// Result of looking up an input encoding (xmlOpenCharEncodingHandler).
enum EncLookup {
    /// UTF-8: no conversion handler needed
    Utf8,
    /// A conversion handler, with its name
    Handler(String),
    Unsupported,
}

/// Encoding names not known to libxml2 itself go to iconv, which (glibc)
/// ignores characters other than alphanumerics and `_-.,:/` in names.
fn iconv_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric() || "_-.,:/".contains(*c))
        .collect()
}

fn open_encoding(name: &str) -> EncLookup {
    if name.eq_ignore_ascii_case("UTF-8") || name.eq_ignore_ascii_case("UTF8") {
        return EncLookup::Utf8;
    }
    if decode_to_utf8(b"", &iconv_name(name)).is_none() {
        return EncLookup::Unsupported;
    }
    // built-in handlers have canonical names, iconv ones keep the given one
    const BUILTIN: &[&str] = &[
        "UTF-16LE",
        "UTF-16BE",
        "UTF-16",
        "ISO-8859-1",
        "ASCII",
        "US-ASCII",
    ];
    for b in BUILTIN {
        if b.eq_ignore_ascii_case(name) {
            return EncLookup::Handler(b.to_string());
        }
    }
    EncLookup::Handler(name.to_string())
}

pub struct HtmlParser<'a> {
    t: &'a mut Tree,
    buf: Vec<u8>,
    pos: usize,
    line: u32,
    col: u32,
    filename: Option<String>,
    // encoding state (XML_INPUT_* flags)
    has_encoding: bool,
    uses_enc_decl: bool,
    auto_enc: Option<&'static str>,
    encoder: Option<String>,
    enc_error: bool,
    /// Bumped whenever the input buffer is replaced (libxml2's input->base
    /// changes), which makes htmlParseNameComplex restart.
    base_gen: u32,
    declared: Option<String>,
    // options
    options: i64,
    recovery: bool,
    keep_blanks: bool,
    linenumbers: bool,
    // parser state
    names: Vec<String>,
    html: u8,
    depth: i32,
    disable_sax: u8,
    well_formed: bool,
    nb_errors: usize,
    nb_warnings: usize,
    pub errors: Vec<XmlError>,
    // tree building (SAX2)
    pub doc: Option<NodeId>,
    nodes: Vec<NodeId>,
    url: Option<String>,
}

impl<'a> HtmlParser<'a> {
    pub fn new(
        t: &'a mut Tree,
        data: &[u8],
        url: Option<&str>,
        options: i64,
        encoding: Option<&str>,
    ) -> Self {
        let mut p = HtmlParser {
            t,
            buf: data.to_vec(),
            pos: 0,
            line: 1,
            col: 1,
            filename: url.map(|s| s.to_string()),
            has_encoding: false,
            uses_enc_decl: false,
            auto_enc: None,
            encoder: None,
            enc_error: false,
            base_gen: 0,
            declared: None,
            options: 0,
            recovery: false,
            keep_blanks: true,
            linenumbers: false,
            names: Vec::new(),
            html: 1,
            depth: 0,
            disable_sax: 0,
            well_formed: true,
            nb_errors: 0,
            nb_warnings: 0,
            errors: Vec::new(),
            doc: None,
            nodes: Vec::new(),
            url: url.map(|s| s.to_string()),
        };
        // xmlNewInputMemory(..., encoding): an explicit encoding
        if let Some(e) = encoding {
            p.switch_encoding_name(e);
        }
        // htmlCtxtUseOptions (PHP only calls it for non-zero options; with
        // zero options line numbers stay disabled)
        if options != 0 {
            p.use_options(options);
        }
        p
    }

    fn use_options(&mut self, options: i64) {
        let keep = HTML_PARSE_NOWARNING
            | HTML_PARSE_NOERROR
            | HTML_PARSE_PEDANTIC
            | HTML_PARSE_NOBLANKS
            | HTML_PARSE_COMPACT
            | HTML_PARSE_HUGE
            | HTML_PARSE_NODEFDTD
            | HTML_PARSE_IGNORE_ENC
            | HTML_PARSE_NOIMPLIED;
        self.options |= options & keep;
        self.keep_blanks = options & HTML_PARSE_NOBLANKS == 0;
        self.recovery = options & HTML_PARSE_RECOVER != 0;
        self.linenumbers = true;
    }

    // ------------------------------------------------------------------
    // Input access

    #[inline]
    fn cur(&self) -> u8 {
        *self.buf.get(self.pos).unwrap_or(&0)
    }

    #[inline]
    fn nxt(&self, n: usize) -> u8 {
        *self.buf.get(self.pos + n).unwrap_or(&0)
    }

    #[inline]
    fn upp(&self, n: usize) -> u8 {
        self.nxt(n).to_ascii_uppercase()
    }

    #[inline]
    fn skip(&mut self, n: usize) {
        self.pos += n;
        self.col += n as u32;
    }

    /// xmlNextChar
    fn next(&mut self) {
        if self.pos >= self.buf.len() {
            return;
        }
        let c = self.buf[self.pos];
        if c < 0x80 {
            if c == b'\n' {
                self.pos += 1;
                self.line += 1;
                self.col = 1;
            } else if c == b'\r' {
                self.pos += if self.nxt(1) == b'\n' { 2 } else { 1 };
                self.line += 1;
                self.col = 1;
            } else {
                self.pos += 1;
                self.col += 1;
            }
            return;
        }
        self.col += 1;
        let avail = self.buf.len() - self.pos;
        let b = &self.buf[self.pos..];
        let ok = (|| {
            if avail < 2 || (b[1] & 0xc0) != 0x80 {
                return 0;
            }
            if c < 0xe0 {
                if c < 0xc2 {
                    return 0;
                }
                return 2;
            }
            let val = ((c as u32) << 8) | b[1] as u32;
            if avail < 3 || (b[2] & 0xc0) != 0x80 {
                return 0;
            }
            if c < 0xf0 {
                if val < 0xe0a0 || (0xeda0..0xee00).contains(&val) {
                    return 0;
                }
                return 3;
            }
            if avail < 4 || (b[3] & 0xc0) != 0x80 {
                return 0;
            }
            if !(0xf090..0xf490).contains(&val) {
                return 0;
            }
            4
        })();
        if ok == 0 {
            if !self.enc_error {
                self.err_io_encoding();
                self.enc_error = true;
            }
            self.pos += 1;
        } else {
            self.pos += ok;
        }
    }

    /// NEXTL(l)
    #[inline]
    fn nextl(&mut self, l: usize) {
        if self.cur() == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        self.pos += l;
    }

    /// htmlSkipBlankChars
    fn skip_blanks(&mut self) -> usize {
        let mut res = 0;
        while is_blank_ch(self.cur()) && self.pos < self.buf.len() {
            if self.cur() == b'\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            self.pos += 1;
            res += 1;
        }
        res
    }

    /// htmlCurrentChar
    fn cur_char(&mut self) -> (u32, usize) {
        if !self.has_encoding {
            let c = self.cur();
            if c < 0x80 {
                if c == 0 {
                    if self.pos < self.buf.len() {
                        self.err_int(XML_ERR_INVALID_CHAR, "Char 0x%X out of allowed range\n", 0);
                        return (b' ' as u32, 1);
                    }
                    return (0, 0);
                }
                return (c as u32, 1);
            }
            // automatic flow conversion
            match self.find_encoding() {
                None => {
                    self.switch_to(Some("ISO-8859-1".to_string()));
                }
                Some(g) => {
                    self.switch_encoding_name(&g);
                }
            }
            self.has_encoding = true;
        }
        let avail = self.buf.len().saturating_sub(self.pos);
        let c = self.cur();
        if c & 0x80 != 0 {
            let b = &self.buf[self.pos..];
            let r: Option<(u32, usize)> = (|| {
                if c & 0x40 == 0 {
                    return None;
                }
                if avail < 2 || (b[1] & 0xc0) != 0x80 {
                    return None;
                }
                if c & 0xe0 == 0xe0 {
                    if avail < 3 || (b[2] & 0xc0) != 0x80 {
                        return None;
                    }
                    if c & 0xf0 == 0xf0 {
                        if (c & 0xf8) != 0xf0 || avail < 4 || (b[3] & 0xc0) != 0x80 {
                            return None;
                        }
                        let val = ((b[0] as u32 & 0x7) << 18)
                            | ((b[1] as u32 & 0x3f) << 12)
                            | ((b[2] as u32 & 0x3f) << 6)
                            | (b[3] as u32 & 0x3f);
                        if val < 0x10000 {
                            return None;
                        }
                        Some((val, 4))
                    } else {
                        let val = ((b[0] as u32 & 0xf) << 12)
                            | ((b[1] as u32 & 0x3f) << 6)
                            | (b[2] as u32 & 0x3f);
                        if val < 0x800 {
                            return None;
                        }
                        Some((val, 3))
                    }
                } else {
                    let val = ((b[0] as u32 & 0x1f) << 6) | (b[1] as u32 & 0x3f);
                    if val < 0x80 {
                        return None;
                    }
                    Some((val, 2))
                }
            })();
            match r {
                Some((val, l)) => {
                    if !is_char(val) {
                        self.err_int(
                            XML_ERR_INVALID_CHAR,
                            "Char 0x%X out of allowed range\n",
                            val as i64,
                        );
                    }
                    (val, l)
                }
                None => {
                    self.err_io_encoding();
                    if !self.has_encoding {
                        self.switch_to(Some("ISO-8859-1".to_string()));
                    }
                    (self.cur() as u32, 1)
                }
            }
        } else {
            if c == 0 {
                if self.pos < self.buf.len() {
                    self.err_int(XML_ERR_INVALID_CHAR, "Char 0x%X out of allowed range\n", 0);
                    return (b' ' as u32, 1);
                }
                return (0, 0);
            }
            (c as u32, 1)
        }
    }

    // ------------------------------------------------------------------
    // Encodings

    /// htmlFindEncoding: heuristic used on the first non-ASCII byte.
    fn find_encoding(&self) -> Option<String> {
        if self.has_encoding {
            return None;
        }
        let s = &self.buf[self.pos.min(self.buf.len())..];
        let a = strcasestr(s, b"HTTP-EQUIV")?;
        let s = &s[a..];
        let b = strcasestr(s, b"CONTENT")?;
        let s = &s[b..];
        let c = strcasestr(s, b"CHARSET=")?;
        let s = &s[c + 8..];
        let n = s
            .iter()
            .take_while(|&&ch| {
                ch.is_ascii_alphanumeric() || ch == b'-' || ch == b'_' || ch == b':' || ch == b'/'
            })
            .count();
        if n == 0 {
            return None;
        }
        Some(String::from_utf8_lossy(&s[..n]).into_owned())
    }

    /// xmlSwitchInputEncoding: `handler` None is UTF-8 (no conversion).
    fn switch_to(&mut self, handler: Option<String>) {
        self.has_encoding = true;
        let h = match handler {
            None => return,
            Some(h) => h,
        };
        if self.encoder.is_some() {
            self.encoder = Some(h);
            return;
        }
        let start = self.pos.min(self.buf.len());
        if start < self.buf.len() {
            if let Some(conv) = decode_to_utf8(&self.buf[start..], &iconv_name(&h)) {
                self.buf.truncate(start);
                self.base_gen += 1;
                self.buf.extend_from_slice(&conv);
            }
        }
        self.encoder = Some(h);
    }

    /// xmlSwitchEncodingName / xmlSwitchInputEncodingName
    fn switch_encoding_name(&mut self, name: &str) -> bool {
        match open_encoding(name) {
            EncLookup::Unsupported => {
                self.err(
                    FROM_PARSER,
                    XML_ERR_UNSUPPORTED_ENCODING,
                    ERR_WARNING,
                    format!("Unsupported encoding: {}\n", name),
                );
                false
            }
            EncLookup::Utf8 => {
                self.switch_to(None);
                true
            }
            EncLookup::Handler(h) => {
                self.switch_to(Some(h));
                true
            }
        }
    }

    /// xmlSetDeclaredEncoding
    fn set_declared_encoding(&mut self, enc: String) {
        let mut enc = enc;
        if !self.has_encoding && self.options & HTML_PARSE_IGNORE_ENC == 0 {
            match open_encoding(&enc) {
                EncLookup::Unsupported => {
                    self.err(
                        FROM_PARSER,
                        XML_ERR_UNSUPPORTED_ENCODING,
                        ERR_FATAL,
                        format!("Unsupported encoding: {}\n", enc),
                    );
                    return;
                }
                EncLookup::Utf8 => self.switch_to(None),
                EncLookup::Handler(h) => self.switch_to(Some(h)),
            }
            self.uses_enc_decl = true;
        } else if let Some(auto) = self.auto_enc {
            let allowed: &[&str] = match auto {
                "UTF-8" => &["UTF-8", "UTF8"],
                "UTF-16LE" => &["UTF-16", "UTF-16LE", "UTF16"],
                "UTF-16BE" => &["UTF-16", "UTF-16BE", "UTF16"],
                _ => &[],
            };
            if !allowed.is_empty() && !allowed.iter().any(|a| a.eq_ignore_ascii_case(&enc)) {
                self.err(
                    FROM_PARSER,
                    XML_WAR_ENCODING_MISMATCH,
                    ERR_WARNING,
                    format!(
                        "Encoding '{}' doesn't match auto-detected '{}'\n",
                        enc, auto
                    ),
                );
                enc = auto.to_string();
            }
        }
        self.declared = Some(enc);
    }

    /// xmlDetectEncoding
    fn detect_encoding(&mut self) {
        if self.buf.len() - self.pos < 4 {
            return;
        }
        let i = &self.buf[self.pos..self.pos + 4];
        if self.has_encoding {
            if i[0] == 0xEF && i[1] == 0xBB && i[2] == 0xBF {
                self.pos += 3;
            }
            return;
        }
        let (i0, i1, i2, i3) = (i[0], i[1], i[2], i[3]);
        let mut enc: Option<&'static str> = None;
        let mut auto: Option<&'static str> = None;
        let mut bom = 0;
        match i0 {
            0x00 => {
                if i1 == 0x3C && i2 == 0x00 && i3 == 0x3F {
                    enc = Some("UTF-16BE");
                    auto = Some("UTF-16BE");
                }
            }
            0x3C => {
                if i1 == 0x00 && i2 == 0x3F && i3 == 0x00 {
                    enc = Some("UTF-16LE");
                    auto = Some("UTF-16LE");
                }
            }
            0xEF => {
                if i1 == 0xBB && i2 == 0xBF {
                    enc = Some("UTF-8");
                    auto = Some("UTF-8");
                    bom = 3;
                }
            }
            0xFE => {
                if i1 == 0xFF {
                    enc = Some("UTF-16BE");
                    auto = Some("UTF-16BE");
                    bom = 2;
                }
            }
            0xFF => {
                if i1 == 0xFE {
                    enc = Some("UTF-16LE");
                    auto = Some("UTF-16LE");
                    bom = 2;
                }
            }
            _ => {}
        }
        self.pos += bom;
        if let Some(e) = enc {
            self.auto_enc = auto;
            if e == "UTF-8" {
                self.switch_to(None);
            } else {
                self.switch_to(Some(e.to_string()));
            }
        }
    }

    /// The encoding stored in the document (xmlGetActualEncoding).
    fn actual_encoding(&self) -> Option<String> {
        if self.uses_enc_decl || self.auto_enc.is_some() {
            self.declared.clone()
        } else if let Some(e) = &self.encoder {
            Some(e.clone())
        } else if self.has_encoding {
            Some("UTF-8".to_string())
        } else {
            None
        }
    }

    // ------------------------------------------------------------------
    // Errors

    /// xmlCtxtVErr
    fn err(&mut self, domain: i32, code: i32, level: i32, msg: String) {
        self.err_node(domain, code, level, msg, None);
    }

    fn err_node(
        &mut self,
        domain: i32,
        code: i32,
        level: i32,
        msg: String,
        node_name: Option<String>,
    ) {
        if self.disable_sax > 1 {
            return;
        }
        let mut report = true;
        if level == ERR_WARNING {
            if self.nb_warnings >= XML_MAX_ERRORS {
                report = false;
            } else {
                self.nb_warnings += 1;
            }
        } else if self.nb_errors >= XML_MAX_ERRORS && (level < ERR_FATAL || !self.well_formed) {
            report = false;
        } else {
            self.nb_errors += 1;
        }
        // NOERROR/NOWARNING only remove libxml2's error channel: the error
        // still reaches a global structured handler (libxml_use_internal_errors),
        // so it is returned; see `error_is_silenced`.
        if report {
            let line = self.line as i32;
            let col = self.col as i32;
            let formatted =
                self.format_generic(domain, code, level, line, &msg, node_name.as_deref());
            self.errors.push(XmlError {
                domain,
                code,
                level,
                message: msg,
                file: self.filename.clone(),
                line,
                col,
                formatted,
            });
        }
        if level == ERR_FATAL {
            self.well_formed = false;
            if !self.recovery {
                self.disable_sax = 1;
            }
        }
    }

    /// htmlParseErr
    fn perr(&mut self, code: i32, msg: &str, s1: Option<&str>, s2: Option<&str>) {
        let mut m = msg.to_string();
        if let Some(a) = s1 {
            if let Some(i) = m.find("%s") {
                m.replace_range(i..i + 2, a);
            }
        }
        if let Some(b) = s2 {
            if let Some(i) = m.find("%s") {
                m.replace_range(i..i + 2, b);
            }
        }
        self.err(FROM_HTML, code, ERR_ERROR, m);
    }

    /// htmlParseErrInt
    fn err_int(&mut self, code: i32, msg: &str, val: i64) {
        let m = if msg.contains("%X") {
            msg.replace("%X", &format!("{:X}", val))
        } else {
            msg.replace("%d", &val.to_string())
        };
        self.err(FROM_HTML, code, ERR_ERROR, m);
    }

    /// xmlCtxtErrIO(ctxt, XML_ERR_INVALID_ENCODING, NULL)
    fn err_io_encoding(&mut self) {
        self.err(
            FROM_IO,
            XML_ERR_INVALID_ENCODING,
            ERR_FATAL,
            "Invalid bytes in character encoding\n".to_string(),
        );
    }

    /// xmlFormatError output, one chunk per line as PHP's generic handler
    /// groups it.
    fn format_generic(
        &self,
        domain: i32,
        code: i32,
        level: i32,
        line: i32,
        msg: &str,
        node_name: Option<&str>,
    ) -> Vec<String> {
        let mut chunks = Vec::new();
        let mut f = String::new();
        if let Some(fname) = &self.filename {
            f.push_str(&format!("{}:{}: ", fname, line));
        } else if line != 0 && domain == FROM_PARSER {
            f.push_str(&format!("Entity: line {}: ", line));
        }
        if let Some(n) = node_name {
            f.push_str(&format!("element {}: ", n));
        }
        f.push_str(match domain {
            FROM_PARSER => "parser ",
            FROM_HTML => "HTML parser ",
            FROM_IO => "I/O ",
            FROM_VALID => "validity ",
            _ => "",
        });
        f.push_str(if level == ERR_WARNING {
            "warning : "
        } else {
            "error : "
        });
        f.push_str(msg.trim_end_matches('\n'));
        chunks.push(f);
        if code == XML_ERR_INVALID_ENCODING && self.encoder.is_none() && self.pos < self.buf.len() {
            let mut b = String::from("Bytes:");
            for i in 0..4 {
                if self.pos + i >= self.buf.len() {
                    break;
                }
                b.push_str(&format!(" 0x{:02X}", self.buf[self.pos + i]));
            }
            chunks.push(b);
        }
        let (a, b) = self.context_lines();
        chunks.push(a);
        chunks.push(b);
        chunks
    }

    /// xmlParserPrintFileContextInternal
    fn context_lines(&self) -> (String, String) {
        let d = &self.buf;
        let size = 80usize;
        let end = d.len();
        let at = |i: usize| -> u8 { if i < end { d[i] } else { 0 } };
        let pos = self.pos.min(end);
        let mut cur = pos;
        while cur > 0 && (at(cur) == b'\n' || at(cur) == b'\r') {
            cur -= 1;
        }
        let mut n = 0;
        while n < size && cur > 0 && at(cur) != b'\n' && at(cur) != b'\r' {
            cur -= 1;
            n += 1;
        }
        if n > 0 && (at(cur) == b'\n' || at(cur) == b'\r') {
            cur += 1;
        } else {
            while cur < pos && (at(cur) & 0xC0) == 0x80 {
                cur += 1;
            }
        }
        let mut col = pos - cur;
        let start = cur;
        n = 0;
        while at(cur) != 0 && at(cur) != b'\n' && at(cur) != b'\r' {
            let len = match crate::parser::utf8_char(d, cur) {
                Some((_, l)) => l,
                None => 1,
            };
            if n + len > size {
                break;
            }
            cur += len;
            n += len;
        }
        if col >= n {
            col = if n < size { n } else { size - 1 };
        }
        let content = &d[start..(start + n).min(end)];
        let a = String::from_utf8_lossy(content).into_owned();
        let mut marker: Vec<u8> = content.to_vec();
        marker.truncate(col);
        for m in marker.iter_mut() {
            if *m != b'\t' {
                *m = b' ';
            }
        }
        marker.push(b'^');
        (a, String::from_utf8_lossy(&marker).into_owned())
    }

    // ------------------------------------------------------------------
    // SAX2 tree building (xmlSAX2* in HTML mode)

    fn line_no(&self) -> u32 {
        if self.linenumbers {
            self.line.min(65535)
        } else {
            0
        }
    }

    fn sax_start_document(&mut self) {
        if self.doc.is_some() {
            return;
        }
        let d = self.t.new_html_doc();
        if let Some(di) = self.t.doc_info_mut(d) {
            di.standalone = 1;
            di.url = self.url.clone();
            di.parse_options = self.options;
        }
        self.doc = Some(d);
    }

    fn sax_end_document(&mut self) {
        let enc = self.actual_encoding();
        if let Some(d) = self.doc {
            if let Some(di) = self.t.doc_info_mut(d) {
                if di.encoding.is_none() {
                    di.encoding = enc;
                }
            }
        }
    }

    fn create_html_int_subset(&mut self, name: &str, ext: Option<&str>, sys: Option<&str>) {
        let d = match self.doc {
            Some(d) => d,
            None => return,
        };
        // xmlCreateIntSubset: for HTML documents the DTD becomes the first child
        let dtd = self.t.create_int_subset(None, name, ext, sys);
        self.t.nodes[dtd].doc = Some(d);
        match self.t.nodes[d].children {
            Some(first) => self.t.insert_before_raw(first, dtd),
            None => self.t.link_child_raw(d, dtd),
        }
        if let Some(di) = self.t.doc_info_mut(d) {
            di.int_subset = Some(dtd);
        }
    }

    fn sax_internal_subset(&mut self, name: Option<&str>, ext: Option<&str>, sys: Option<&str>) {
        let d = match self.doc {
            Some(d) => d,
            None => return,
        };
        if self.t.int_subset(d).is_some() {
            return;
        }
        self.create_html_int_subset(name.unwrap_or(""), ext, sys);
    }

    fn append_child(&mut self, n: NodeId) {
        let parent = match self.nodes.last() {
            Some(&p) => p,
            None => match self.doc {
                Some(d) => d,
                None => return,
            },
        };
        self.t.link_child_raw(parent, n);
    }

    fn sax_start_element(&mut self, name: &str, atts: &[(String, Option<String>)]) {
        let doc = match self.doc {
            Some(d) => d,
            None => return,
        };
        let el = self.t.new_element(Some(doc), name);
        self.t.nodes[el].line = self.line_no();
        self.append_child(el);
        self.nodes.push(el);
        let mut id_errors = Vec::new();
        for (aname, aval) in atts {
            let value: Option<&str> = match aval {
                None if is_boolean_attr(aname) => Some(aname.as_str()),
                None => None,
                Some(v) => Some(v.as_str()),
            };
            let at = self.t.new_node(Some(doc), NType::Attribute, aname);
            self.t.link_child_raw(el, at);
            if let Some(v) = value {
                let tx = self.t.new_text(Some(doc), v);
                self.t.link_child_raw(at, tx);
                // xmlIsID for HTML documents
                if aname == "id" || (aname == "name" && name == "a") {
                    let content = v.to_string();
                    self.t.nodes[at].atype = 2;
                    let exists = self
                        .t
                        .doc_info(doc)
                        .map(|di| di.ids.contains_key(&content))
                        .unwrap_or(false);
                    if exists {
                        id_errors.push(content);
                    } else if let Some(di) = self.t.doc_info_mut(doc) {
                        di.ids.insert(content, at);
                    }
                }
            }
        }
        for v in id_errors {
            self.err_node(
                FROM_VALID,
                XML_DTD_ID_REDEFINED,
                ERR_ERROR,
                format!("ID {} already defined\n", v),
                Some(name.to_string()),
            );
        }
    }

    fn sax_end_element(&mut self) {
        self.nodes.pop();
    }

    fn sax_text(&mut self, s: &[u8], cdata: bool) {
        let parent = match self.nodes.last() {
            Some(&p) => p,
            None => return,
        };
        let text = to_string(s);
        if let Some(last) = self.t.nodes[parent].last {
            let ln = &self.t.nodes[last];
            let coalesce = if cdata {
                ln.t == NType::CData
            } else {
                ln.t == NType::Text && ln.name == "text"
            };
            if coalesce {
                self.t.nodes[last].content.push_str(&text);
                if !cdata {
                    self.t.nodes[last].line = self.line_no();
                }
                return;
            }
        }
        let doc = self.doc;
        let n = if cdata {
            self.t.new_cdata(doc, &text)
        } else {
            self.t.new_text(doc, &text)
        };
        if !cdata {
            self.t.nodes[n].line = self.line_no();
        }
        self.t.link_child_raw(parent, n);
    }

    fn sax_characters(&mut self, s: &[u8]) {
        self.sax_text(s, false);
    }

    fn sax_comment(&mut self, s: &[u8]) {
        let doc = self.doc;
        let c = self.t.new_comment(doc, &to_string(s));
        self.t.nodes[c].line = self.line_no();
        self.append_child(c);
    }

    fn sax_pi(&mut self, target: &str, data: Option<&[u8]>) {
        let doc = self.doc;
        let c = self
            .t
            .new_pi(doc, target, &data.map(to_string).unwrap_or_default());
        self.t.nodes[c].line = self.line_no();
        self.append_child(c);
    }

    // ------------------------------------------------------------------
    // Name stack

    fn name_push(&mut self, name: &str) {
        if self.html < 3 && name == "head" {
            self.html = 3;
        }
        if self.html < 10 && name == "body" {
            self.html = 10;
        }
        self.names.push(name.to_string());
    }

    fn name_pop(&mut self) -> Option<String> {
        self.names.pop()
    }

    fn name(&self) -> Option<&str> {
        self.names.last().map(|s| s.as_str())
    }

    fn end_and_pop(&mut self) {
        self.sax_end_element();
        self.name_pop();
    }

    // ------------------------------------------------------------------
    // Auto-close / implied elements

    /// htmlAutoCloseOnClose
    fn auto_close_on_close(&mut self, newtag: &str) {
        let priority = end_priority(newtag);
        let mut found = false;
        for i in (0..self.names.len()).rev() {
            if self.names[i] == newtag {
                found = true;
                break;
            }
            if end_priority(&self.names[i]) > priority {
                return;
            }
        }
        if !found {
            return;
        }
        while self.name() != Some(newtag) {
            let cur = self.name().unwrap_or("").to_string();
            if let Some(info) = tag_lookup(&cur) {
                if info.end_tag == 3 {
                    self.perr(
                        XML_ERR_TAG_NAME_MISMATCH,
                        "Opening and ending tag mismatch: %s and %s\n",
                        Some(newtag),
                        Some(&cur),
                    );
                }
            }
            self.end_and_pop();
        }
    }

    /// htmlAutoCloseOnEnd
    fn auto_close_on_end(&mut self) {
        while !self.names.is_empty() {
            self.end_and_pop();
        }
    }

    /// htmlAutoClose
    fn auto_close(&mut self, newtag: &str) {
        while let Some(n) = self.name() {
            if !check_auto_close(newtag, n) {
                break;
            }
            self.end_and_pop();
        }
    }

    fn push_implied(&mut self, name: &str) {
        self.name_push(name);
        self.sax_start_element(name, &[]);
    }

    /// htmlCheckImplied
    fn check_implied(&mut self, newtag: &str) {
        if self.options & HTML_PARSE_NOIMPLIED != 0 {
            return;
        }
        if newtag == "html" {
            return;
        }
        if self.names.is_empty() {
            self.push_implied("html");
        }
        if newtag == "body" || newtag == "head" {
            return;
        }
        if self.names.len() <= 1
            && matches!(
                newtag,
                "script" | "style" | "meta" | "link" | "title" | "base"
            )
        {
            if self.html >= 3 {
                return;
            }
            self.push_implied("head");
        } else if newtag != "noframes" && newtag != "frame" && newtag != "frameset" {
            if self.html >= 10 {
                return;
            }
            if self.names.iter().any(|n| n == "body" || n == "head") {
                return;
            }
            self.push_implied("body");
        }
    }

    /// htmlCheckParagraph
    fn check_paragraph(&mut self) {
        let implied_p = match self.name() {
            None => true,
            Some(tag) => tag == "html" || tag == "head",
        };
        if implied_p {
            self.auto_close("p");
            self.check_implied("p");
            self.push_implied("p");
        }
    }

    /// areBlanks
    fn are_blanks(&self, s: &[u8]) -> bool {
        if !s.iter().all(|&c| is_blank_ch(c)) {
            return false;
        }
        if self.cur() == 0 {
            return true;
        }
        if self.cur() != b'<' {
            return false;
        }
        let name = match self.name() {
            None => return true,
            Some(n) => n,
        };
        if name == "html" || name == "head" {
            return true;
        }
        if name == "body" {
            if let Some(d) = self.doc {
                if let Some(dtd) = self.t.int_subset(d) {
                    if let Some(e) = self.t.dtd_info(dtd).and_then(|i| i.external_id.as_deref()) {
                        if e.eq_ignore_ascii_case("-//W3C//DTD HTML 4.01//EN")
                            || e.eq_ignore_ascii_case("-//W3C//DTD HTML 4//EN")
                        {
                            return true;
                        }
                    }
                }
            }
        }
        let node = match self.nodes.last() {
            None => return false,
            Some(&n) => n,
        };
        let mut last = self.t.nodes[node].last;
        while let Some(l) = last {
            if self.t.nodes[l].t == NType::Comment {
                last = self.t.nodes[l].prev;
            } else {
                break;
            }
        }
        match last {
            None => {
                if ALLOW_PCDATA.contains(&name) {
                    return false;
                }
            }
            Some(l) => {
                if self.t.nodes[l].t == NType::Text {
                    return false;
                }
                let ln = self.t.nodes[l].name.as_str();
                if ALLOW_PCDATA.contains(&ln) {
                    return false;
                }
            }
        }
        true
    }

    // ------------------------------------------------------------------
    // Names

    /// htmlParseHTMLName: tag or attribute name, lowercased.
    fn parse_html_name(&mut self) -> Option<String> {
        let c = self.cur();
        if !is_ascii_letter(c) && c != b'_' && c != b':' && c != b'.' {
            return None;
        }
        let mut loc = Vec::new();
        while loc.len() < HTML_PARSER_BUFFER_SIZE {
            let c = self.cur();
            if !(is_ascii_letter(c)
                || c.is_ascii_digit()
                || c == b':'
                || c == b'-'
                || c == b'_'
                || c == b'.')
            {
                break;
            }
            loc.push(c.to_ascii_lowercase());
            self.next();
        }
        Some(String::from_utf8(loc).unwrap_or_default())
    }

    /// htmlParseHTMLName_nonInvasive
    fn parse_html_name_non_invasive(&self) -> Option<String> {
        let c = self.nxt(1);
        if !is_ascii_letter(c) && c != b'_' && c != b':' {
            return None;
        }
        let mut loc = Vec::new();
        while loc.len() < HTML_PARSER_BUFFER_SIZE {
            let c = self.nxt(1 + loc.len());
            if !(is_ascii_letter(c) || c.is_ascii_digit() || c == b':' || c == b'-' || c == b'_') {
                break;
            }
            loc.push(c.to_ascii_lowercase());
        }
        Some(String::from_utf8(loc).unwrap_or_default())
    }

    /// htmlParseName (case sensitive)
    fn parse_name(&mut self) -> Option<String> {
        let b = &self.buf;
        let mut i = self.pos;
        let at = |i: usize| -> u8 { *b.get(i).unwrap_or(&0) };
        let c = at(i);
        if c.is_ascii_alphabetic() || c == b'_' || c == b':' {
            i += 1;
            while {
                let c = at(i);
                c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b':' || c == b'.'
            } {
                i += 1;
            }
            if i >= b.len() {
                return None;
            }
            let c = at(i);
            if c > 0 && c < 0x80 {
                let count = i - self.pos;
                let name = String::from_utf8_lossy(&b[self.pos..i]).into_owned();
                self.pos = i;
                self.col += count as u32;
                return Some(name);
            }
        }
        self.parse_name_complex()
    }

    fn parse_name_complex(&mut self) -> Option<String> {
        let max_len = if self.options & HTML_PARSE_HUGE != 0 {
            XML_MAX_TEXT_LENGTH
        } else {
            XML_MAX_NAME_LENGTH
        };
        let base = self.base_gen;
        let (mut c, mut l) = self.cur_char();
        if c == b' ' as u32
            || c == b'>' as u32
            || c == b'/' as u32
            || (!is_letter(c) && c != b'_' as u32 && c != b':' as u32)
        {
            return None;
        }
        let mut out = Vec::new();
        while c != b' ' as u32
            && c != b'>' as u32
            && c != b'/' as u32
            && (is_letter(c)
                || is_digit(c)
                || c == b'.' as u32
                || c == b'-' as u32
                || c == b'_' as u32
                || c == b':' as u32
                || is_combining(c)
                || is_extender(c))
        {
            if out.len() + l > max_len {
                self.perr(XML_ERR_NAME_TOO_LONG, "name too long", None, None);
                return None;
            }
            copy_buf(&mut out, l, c);
            self.nextl(l);
            let r = self.cur_char();
            c = r.0;
            l = r.1;
            if self.base_gen != base {
                // the encoding changed: libxml2 starts again from the
                // current position (dropping what was read so far)
                return self.parse_name_complex();
            }
        }
        if out.is_empty() {
            self.perr(
                XML_ERR_INTERNAL_ERROR,
                "unexpected change of input buffer",
                None,
                None,
            );
            return None;
        }
        Some(to_string(&out))
    }

    // ------------------------------------------------------------------
    // References

    /// htmlParseCharRef
    fn parse_char_ref(&mut self) -> u32 {
        let mut val: u32 = 0;
        if self.cur() == b'&' && self.nxt(1) == b'#' && (self.nxt(2) == b'x' || self.nxt(2) == b'X')
        {
            self.skip(3);
            while self.cur() != b';' {
                let c = self.cur();
                let d = match c {
                    b'0'..=b'9' => (c - b'0') as u32,
                    b'a'..=b'f' => (c - b'a') as u32 + 10,
                    b'A'..=b'F' => (c - b'A') as u32 + 10,
                    _ => {
                        self.perr(
                            XML_ERR_INVALID_HEX_CHARREF,
                            "htmlParseCharRef: missing semicolon\n",
                            None,
                            None,
                        );
                        break;
                    }
                };
                if val < 0x110000 {
                    val = val * 16 + d;
                }
                self.next();
            }
            if self.cur() == b';' {
                self.next();
            }
        } else if self.cur() == b'&' && self.nxt(1) == b'#' {
            self.skip(2);
            while self.cur() != b';' {
                let c = self.cur();
                if c.is_ascii_digit() {
                    if val < 0x110000 {
                        val = val * 10 + (c - b'0') as u32;
                    }
                } else {
                    self.perr(
                        XML_ERR_INVALID_DEC_CHARREF,
                        "htmlParseCharRef: missing semicolon\n",
                        None,
                        None,
                    );
                    break;
                }
                self.next();
            }
            if self.cur() == b';' {
                self.next();
            }
        } else {
            self.perr(
                XML_ERR_INVALID_CHARREF,
                "htmlParseCharRef: invalid value\n",
                None,
                None,
            );
        }
        if is_char(val) {
            val
        } else if val >= 0x110000 {
            self.perr(
                XML_ERR_INVALID_CHAR,
                "htmlParseCharRef: value too large\n",
                None,
                None,
            );
            0
        } else {
            self.err_int(
                XML_ERR_INVALID_CHAR,
                "htmlParseCharRef: invalid xmlChar value %d\n",
                val as i64,
            );
            0
        }
    }

    /// htmlParseEntityRef: returns (entity value, name)
    fn parse_entity_ref(&mut self) -> (Option<u32>, Option<String>) {
        let mut ent = None;
        let mut out_name = None;
        if self.cur() == b'&' {
            self.next();
            match self.parse_name() {
                None => {
                    self.perr(
                        XML_ERR_NAME_REQUIRED,
                        "htmlParseEntityRef: no name\n",
                        None,
                        None,
                    );
                }
                Some(name) => {
                    if self.cur() == b';' {
                        ent = entity_lookup(name.as_bytes());
                        if ent.is_some() {
                            self.next();
                        }
                    } else {
                        self.perr(
                            XML_ERR_ENTITYREF_SEMICOL_MISSING,
                            "htmlParseEntityRef: expecting ';'\n",
                            None,
                            None,
                        );
                    }
                    out_name = Some(name);
                }
            }
        }
        (ent, out_name)
    }

    /// htmlParseReference
    fn parse_reference(&mut self) {
        if self.cur() != b'&' {
            return;
        }
        if self.nxt(1) == b'#' {
            let c = self.parse_char_ref();
            if c == 0 {
                return;
            }
            let mut out = Vec::new();
            push_utf8(&mut out, c);
            self.check_paragraph();
            self.sax_characters(&out);
        } else {
            let (ent, name) = self.parse_entity_ref();
            let name = match name {
                None => {
                    self.check_paragraph();
                    self.sax_characters(b"&");
                    return;
                }
                Some(n) => n,
            };
            match ent {
                Some(v) if v > 0 => {
                    let mut out = Vec::new();
                    push_utf8(&mut out, v);
                    self.check_paragraph();
                    self.sax_characters(&out);
                }
                _ => {
                    self.check_paragraph();
                    self.sax_characters(b"&");
                    self.sax_characters(name.as_bytes());
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Attributes

    /// htmlParseHTMLAttribute
    fn parse_html_attribute(&mut self, stop: u8) -> Option<Vec<u8>> {
        let max_len = if self.options & HTML_PARSE_HUGE != 0 {
            XML_MAX_HUGE_LENGTH
        } else {
            XML_MAX_TEXT_LENGTH
        };
        let mut out: Vec<u8> = Vec::new();
        while self.cur() != 0 && self.cur() != stop {
            if stop == 0 && self.cur() == b'>' {
                break;
            }
            if stop == 0 && is_blank_ch(self.cur()) {
                break;
            }
            if self.cur() == b'&' {
                if self.nxt(1) == b'#' {
                    let c = self.parse_char_ref();
                    push_utf8(&mut out, c);
                } else {
                    let (ent, name) = self.parse_entity_ref();
                    match name {
                        None => out.push(b'&'),
                        Some(n) => match ent {
                            None => {
                                out.push(b'&');
                                out.extend_from_slice(n.as_bytes());
                            }
                            Some(v) => push_utf8(&mut out, v),
                        },
                    }
                }
            } else {
                let (c, l) = self.cur_char();
                push_utf8(&mut out, c);
                self.nextl(l);
            }
            if out.len() > max_len {
                self.perr(
                    XML_ERR_ATTRIBUTE_NOT_FINISHED,
                    "attribute value too long\n",
                    None,
                    None,
                );
                return None;
            }
        }
        // the C string ends at the first NUL (e.g. from "&#0;")
        if let Some(z) = out.iter().position(|&c| c == 0) {
            out.truncate(z);
        }
        Some(out)
    }

    /// htmlParseAttValue
    fn parse_att_value(&mut self) -> Option<Vec<u8>> {
        let q = self.cur();
        if q == b'"' || q == b'\'' {
            self.next();
            let ret = self.parse_html_attribute(q);
            if self.cur() != q {
                let m = if q == b'"' {
                    "AttValue: \" expected\n"
                } else {
                    "AttValue: ' expected\n"
                };
                self.perr(XML_ERR_ATTRIBUTE_NOT_FINISHED, m, None, None);
            } else {
                self.next();
            }
            ret
        } else {
            let ret = self.parse_html_attribute(0);
            if ret.is_none() {
                self.perr(
                    XML_ERR_ATTRIBUTE_WITHOUT_VALUE,
                    "AttValue: no value found\n",
                    None,
                    None,
                );
            }
            ret
        }
    }

    /// htmlParseAttribute
    fn parse_attribute(&mut self) -> Option<(String, Option<Vec<u8>>)> {
        let name = match self.parse_html_name() {
            None => {
                self.perr(
                    XML_ERR_NAME_REQUIRED,
                    "error parsing attribute name\n",
                    None,
                    None,
                );
                return None;
            }
            Some(n) => n,
        };
        self.skip_blanks();
        let mut val = None;
        if self.cur() == b'=' {
            self.next();
            self.skip_blanks();
            val = self.parse_att_value();
        }
        Some((name, val))
    }

    /// htmlCheckEncoding (http-equiv content attribute)
    fn check_encoding(&mut self, attvalue: &[u8]) {
        let mut enc: Option<usize> = strcasestr(attvalue, b"charset").map(|i| i + 7);
        if let Some(e) = enc {
            if is_blank_ch(*attvalue.get(e).unwrap_or(&0)) {
                enc = strcasestr(attvalue, b"=");
            }
        }
        if let Some(e) = enc {
            if attvalue.get(e) == Some(&b'=') {
                let rest = &attvalue[e + 1..];
                let end = rest.iter().position(|&c| c == 0).unwrap_or(rest.len());
                self.set_declared_encoding(to_string(&rest[..end]));
            }
        }
    }

    /// htmlCheckMeta
    fn check_meta(&mut self, atts: &[(String, Option<Vec<u8>>)]) {
        let mut http = false;
        let mut content: Option<Vec<u8>> = None;
        for (att, value) in atts {
            if let Some(v) = value {
                if att.eq_ignore_ascii_case("http-equiv") && v.eq_ignore_ascii_case(b"Content-Type")
                {
                    http = true;
                } else if att.eq_ignore_ascii_case("charset") {
                    self.set_declared_encoding(to_string(v));
                } else if att.eq_ignore_ascii_case("content") {
                    content = Some(v.clone());
                }
            }
        }
        if http {
            if let Some(c) = content {
                self.check_encoding(&c);
            }
        }
    }

    // ------------------------------------------------------------------
    // Tags

    /// htmlParseStartTag: -1 error, 0 ok, 1 discarded
    fn parse_start_tag(&mut self) -> i32 {
        if self.cur() != b'<' {
            return -1;
        }
        self.next();
        let name = match self.parse_html_name() {
            None => {
                self.perr(
                    XML_ERR_NAME_REQUIRED,
                    "htmlParseStartTag: invalid element name\n",
                    None,
                    None,
                );
                while self.cur() != 0 && self.cur() != b'>' {
                    self.next();
                }
                return -1;
            }
            Some(n) => n,
        };
        let meta = name == "meta";
        self.auto_close(&name);
        self.check_implied(&name);
        let mut discardtag = false;
        if !self.names.is_empty() && name == "html" {
            self.perr(
                XML_HTML_STRUCURE_ERROR,
                "htmlParseStartTag: misplaced <html> tag\n",
                None,
                None,
            );
            discardtag = true;
            self.depth += 1;
        }
        if self.names.len() != 1 && name == "head" {
            self.perr(
                XML_HTML_STRUCURE_ERROR,
                "htmlParseStartTag: misplaced <head> tag\n",
                None,
                None,
            );
            discardtag = true;
            self.depth += 1;
        }
        if name == "body" {
            let n = self.names.iter().filter(|n| *n == "body").count();
            for _ in 0..n {
                self.perr(
                    XML_HTML_STRUCURE_ERROR,
                    "htmlParseStartTag: misplaced <body> tag\n",
                    None,
                    None,
                );
                discardtag = true;
                self.depth += 1;
            }
        }
        let mut atts: Vec<(String, Option<Vec<u8>>)> = Vec::new();
        self.skip_blanks();
        while self.cur() != 0
            && self.cur() != b'>'
            && (self.cur() != b'/' || self.nxt(1) != b'>')
            && self.disable_sax <= 1
        {
            match self.parse_attribute() {
                Some((attname, attvalue)) => {
                    if atts.iter().any(|(n, _)| *n == attname) {
                        self.perr(
                            XML_ERR_ATTRIBUTE_REDEFINED,
                            "Attribute %s redefined\n",
                            Some(&attname),
                            None,
                        );
                    } else {
                        atts.push((attname, attvalue));
                    }
                }
                None => {
                    while self.cur() != 0
                        && !is_blank_ch(self.cur())
                        && self.cur() != b'>'
                        && (self.cur() != b'/' || self.nxt(1) != b'>')
                    {
                        self.next();
                    }
                }
            }
            self.skip_blanks();
        }
        if meta && !atts.is_empty() {
            self.check_meta(&atts);
        }
        if !discardtag {
            self.name_push(&name);
            let a: Vec<(String, Option<String>)> = atts
                .into_iter()
                .map(|(n, v)| (n, v.map(|v| to_string(&v))))
                .collect();
            self.sax_start_element(&name, &a);
        }
        if discardtag { 1 } else { 0 }
    }

    /// htmlParseEndTag: returns true if the current level was closed
    fn parse_end_tag(&mut self) -> bool {
        if self.cur() != b'<' || self.nxt(1) != b'/' {
            self.perr(
                XML_ERR_LTSLASH_REQUIRED,
                "htmlParseEndTag: '</' not found\n",
                None,
                None,
            );
            return false;
        }
        self.skip(2);
        let name = match self.parse_html_name() {
            None => return false,
            Some(n) => n,
        };
        self.skip_blanks();
        if self.cur() != b'>' {
            self.perr(XML_ERR_GT_REQUIRED, "End tag : expected '>'\n", None, None);
            while self.cur() != 0 && self.cur() != b'>' {
                self.next();
            }
        }
        if self.cur() == b'>' {
            self.next();
        }
        if self.depth > 0 && (name == "html" || name == "body" || name == "head") {
            self.depth -= 1;
            return false;
        }
        if !self.names.iter().any(|n| *n == name) {
            self.perr(
                XML_ERR_TAG_NAME_MISMATCH,
                "Unexpected end tag : %s\n",
                Some(&name),
                None,
            );
            return false;
        }
        self.auto_close_on_close(&name);
        if let Some(cur) = self.name() {
            if cur != name {
                let cur = cur.to_string();
                self.perr(
                    XML_ERR_TAG_NAME_MISMATCH,
                    "Opening and ending tag mismatch: %s and %s\n",
                    Some(&name),
                    Some(&cur),
                );
            }
        }
        if self.name() == Some(name.as_str()) {
            self.end_and_pop();
            true
        } else {
            false
        }
    }

    // ------------------------------------------------------------------
    // Content

    /// htmlParseScript
    fn parse_script(&mut self) {
        let mut buf: Vec<u8> = Vec::new();
        let (mut c, mut l) = self.cur_char();
        while c != 0 {
            if c == b'<' as u32 && self.nxt(1) == b'/' {
                if self.recovery {
                    let name = self.name().unwrap_or("").to_string();
                    let nb = name.as_bytes();
                    let rest = &self.buf[(self.pos + 2).min(self.buf.len())..];
                    let matches = rest.len() >= nb.len()
                        && rest[..nb.len()].eq_ignore_ascii_case(nb)
                        && !rest[..nb.len()].contains(&0);
                    if matches {
                        break;
                    } else {
                        self.perr(
                            XML_ERR_TAG_NAME_MISMATCH,
                            "Element %s embeds close tag\n",
                            Some(&name),
                            None,
                        );
                    }
                } else if self.nxt(2).is_ascii_alphabetic() {
                    break;
                }
            }
            if is_char(c) {
                copy_buf(&mut buf, l, c);
            } else {
                self.err_int(
                    XML_ERR_INVALID_CHAR,
                    "Invalid char in CDATA 0x%X\n",
                    c as i64,
                );
            }
            self.nextl(l);
            if buf.len() >= HTML_PARSER_BIG_BUFFER_SIZE {
                self.sax_text(&buf, true);
                buf.clear();
            }
            let r = self.cur_char();
            c = r.0;
            l = r.1;
        }
        if !buf.is_empty() && self.disable_sax == 0 {
            self.sax_text(&buf, true);
        }
    }

    fn flush_char_data(&mut self, buf: &[u8]) {
        if self.disable_sax != 0 {
            return;
        }
        if self.are_blanks(buf) {
            if self.keep_blanks {
                self.sax_characters(buf);
            }
        } else {
            self.check_paragraph();
            self.sax_characters(buf);
        }
    }

    /// htmlParseCharDataInternal
    fn parse_char_data(&mut self) {
        let mut buf: Vec<u8> = Vec::new();
        let (mut c, mut l) = self.cur_char();
        while c != b'<' as u32 && c != b'&' as u32 && c != 0 && self.disable_sax <= 1 {
            if !is_char(c) {
                self.err_int(
                    XML_ERR_INVALID_CHAR,
                    "Invalid char in CDATA 0x%X\n",
                    c as i64,
                );
            } else {
                copy_buf(&mut buf, l, c);
            }
            self.nextl(l);
            if buf.len() >= HTML_PARSER_BIG_BUFFER_SIZE {
                let b = std::mem::take(&mut buf);
                self.flush_char_data(&b);
            }
            let r = self.cur_char();
            c = r.0;
            l = r.1;
        }
        if !buf.is_empty() {
            self.flush_char_data(&buf);
        }
    }

    /// htmlParseSystemLiteral
    fn parse_system_literal(&mut self) -> Option<String> {
        let q = self.cur();
        if q != b'"' && q != b'\'' {
            self.perr(
                XML_ERR_LITERAL_NOT_STARTED,
                "SystemLiteral \" or ' expected\n",
                None,
                None,
            );
            return None;
        }
        self.next();
        let start = self.pos;
        let mut len = 0;
        let mut err = false;
        while self.cur() != 0 && self.cur() != q {
            let c = self.cur();
            if !(c == 0x9 || c == 0xA || c == 0xD || c >= 0x20) {
                self.err_int(
                    XML_ERR_INVALID_CHAR,
                    "Invalid char in SystemLiteral 0x%X\n",
                    c as i64,
                );
                err = true;
            }
            self.next();
            len += 1;
        }
        if self.cur() != q {
            self.perr(
                XML_ERR_LITERAL_NOT_FINISHED,
                "Unfinished SystemLiteral\n",
                None,
                None,
            );
            None
        } else {
            let r = if !err {
                Some(to_string(
                    &self.buf[start..(start + len).min(self.buf.len())],
                ))
            } else {
                None
            };
            self.next();
            r
        }
    }

    /// htmlParsePubidLiteral
    fn parse_pubid_literal(&mut self) -> Option<String> {
        let q = self.cur();
        if q != b'"' && q != b'\'' {
            self.perr(
                XML_ERR_LITERAL_NOT_STARTED,
                "PubidLiteral \" or ' expected\n",
                None,
                None,
            );
            return None;
        }
        self.next();
        let start = self.pos;
        let mut len = 0;
        let mut err = false;
        while self.cur() != 0 && self.cur() != q {
            let c = self.cur();
            if !is_pubid_char(c) {
                self.err_int(
                    XML_ERR_INVALID_CHAR,
                    "Invalid char in PubidLiteral 0x%X\n",
                    c as i64,
                );
                err = true;
            }
            len += 1;
            self.next();
        }
        if self.cur() != q {
            self.perr(
                XML_ERR_LITERAL_NOT_FINISHED,
                "Unfinished PubidLiteral\n",
                None,
                None,
            );
            None
        } else {
            let r = if !err {
                Some(to_string(
                    &self.buf[start..(start + len).min(self.buf.len())],
                ))
            } else {
                None
            };
            self.next();
            r
        }
    }

    /// htmlParseExternalID: returns (system URI, public ID)
    fn parse_external_id(&mut self) -> (Option<String>, Option<String>) {
        let mut uri = None;
        let mut public = None;
        if self.upp(0) == b'S'
            && self.upp(1) == b'Y'
            && self.upp(2) == b'S'
            && self.upp(3) == b'T'
            && self.upp(4) == b'E'
            && self.upp(5) == b'M'
        {
            self.skip(6);
            if !is_blank_ch(self.cur()) {
                self.perr(
                    XML_ERR_SPACE_REQUIRED,
                    "Space required after 'SYSTEM'\n",
                    None,
                    None,
                );
            }
            self.skip_blanks();
            uri = self.parse_system_literal();
            if uri.is_none() {
                self.perr(
                    XML_ERR_URI_REQUIRED,
                    "htmlParseExternalID: SYSTEM, no URI\n",
                    None,
                    None,
                );
            }
        } else if self.upp(0) == b'P'
            && self.upp(1) == b'U'
            && self.upp(2) == b'B'
            && self.upp(3) == b'L'
            && self.upp(4) == b'I'
            && self.upp(5) == b'C'
        {
            self.skip(6);
            if !is_blank_ch(self.cur()) {
                self.perr(
                    XML_ERR_SPACE_REQUIRED,
                    "Space required after 'PUBLIC'\n",
                    None,
                    None,
                );
            }
            self.skip_blanks();
            public = self.parse_pubid_literal();
            if public.is_none() {
                self.perr(
                    XML_ERR_PUBID_REQUIRED,
                    "htmlParseExternalID: PUBLIC, no Public Identifier\n",
                    None,
                    None,
                );
            }
            self.skip_blanks();
            if self.cur() == b'"' || self.cur() == b'\'' {
                uri = self.parse_system_literal();
            }
        }
        (uri, public)
    }

    /// htmlParsePI
    fn parse_pi(&mut self) {
        if self.cur() != b'<' || self.nxt(1) != b'?' {
            return;
        }
        let max_len = if self.options & HTML_PARSE_HUGE != 0 {
            XML_MAX_HUGE_LENGTH
        } else {
            XML_MAX_TEXT_LENGTH
        };
        self.skip(2);
        let target = match self.parse_name() {
            None => {
                self.perr(
                    XML_ERR_PI_NOT_STARTED,
                    "PI is not started correctly",
                    None,
                    None,
                );
                return;
            }
            Some(t) => t,
        };
        if self.cur() == b'>' {
            self.skip(1);
            if self.disable_sax == 0 {
                self.sax_pi(&target, None);
            }
            return;
        }
        if !is_blank_ch(self.cur()) {
            self.perr(
                XML_ERR_SPACE_REQUIRED,
                "ParsePI: PI %s space expected\n",
                Some(&target),
                None,
            );
        }
        self.skip_blanks();
        let mut buf = Vec::new();
        let (mut c, mut l) = self.cur_char();
        while c != 0 && c != b'>' as u32 {
            if is_char(c) {
                copy_buf(&mut buf, l, c);
            } else {
                self.err_int(
                    XML_ERR_INVALID_CHAR,
                    "Invalid char in processing instruction 0x%X\n",
                    c as i64,
                );
            }
            if buf.len() > max_len {
                self.perr(
                    XML_ERR_PI_NOT_FINISHED,
                    "PI %s too long",
                    Some(&target),
                    None,
                );
                return;
            }
            self.nextl(l);
            let r = self.cur_char();
            c = r.0;
            l = r.1;
        }
        if c != b'>' as u32 {
            self.perr(
                XML_ERR_PI_NOT_FINISHED,
                "ParsePI: PI %s never end ...\n",
                Some(&target),
                None,
            );
        } else {
            self.skip(1);
            if self.disable_sax == 0 {
                self.sax_pi(&target, Some(&buf));
            }
        }
    }

    /// htmlParseComment
    fn parse_comment(&mut self) {
        if self.cur() != b'<' || self.nxt(1) != b'!' || self.nxt(2) != b'-' || self.nxt(3) != b'-' {
            return;
        }
        let max_len = if self.options & HTML_PARSE_HUGE != 0 {
            XML_MAX_HUGE_LENGTH
        } else {
            XML_MAX_TEXT_LENGTH
        };
        self.skip(4);
        let mut buf: Vec<u8> = Vec::new();
        let finished: bool;
        'outer: {
            let (mut q, mut ql) = self.cur_char();
            if q == 0 {
                finished = false;
                break 'outer;
            }
            if q == b'>' as u32 {
                self.perr(
                    XML_ERR_COMMENT_ABRUPTLY_ENDED,
                    "Comment abruptly ended",
                    None,
                    None,
                );
                finished = true;
                break 'outer;
            }
            self.nextl(ql);
            let (mut r, mut rl) = self.cur_char();
            if r == 0 {
                finished = false;
                break 'outer;
            }
            if q == b'-' as u32 && r == b'>' as u32 {
                self.perr(
                    XML_ERR_COMMENT_ABRUPTLY_ENDED,
                    "Comment abruptly ended",
                    None,
                    None,
                );
                finished = true;
                break 'outer;
            }
            self.nextl(rl);
            let (mut cur, mut l) = self.cur_char();
            let mut gt = false;
            while cur != 0 && (cur != b'>' as u32 || r != b'-' as u32 || q != b'-' as u32) {
                self.nextl(l);
                let (next, nl) = self.cur_char();
                if q == b'-' as u32 && r == b'-' as u32 && cur == b'!' as u32 && next == b'>' as u32
                {
                    self.perr(
                        XML_ERR_COMMENT_NOT_FINISHED,
                        "Comment incorrectly closed by '--!>'",
                        None,
                        None,
                    );
                    gt = true;
                    break;
                }
                if is_char(q) {
                    copy_buf(&mut buf, ql, q);
                } else {
                    self.err_int(
                        XML_ERR_INVALID_CHAR,
                        "Invalid char in comment 0x%X\n",
                        q as i64,
                    );
                }
                if buf.len() > max_len {
                    self.perr(XML_ERR_COMMENT_NOT_FINISHED, "comment too long", None, None);
                    return;
                }
                q = r;
                ql = rl;
                r = cur;
                rl = l;
                cur = next;
                l = nl;
            }
            finished = gt || cur == b'>' as u32;
        }
        if finished {
            self.next();
            if self.disable_sax == 0 {
                self.sax_comment(&buf);
            }
            return;
        }
        let shown: Vec<u8> = buf.iter().take(50).copied().collect();
        let msg = format!("Comment not terminated \n<!--{}\n", to_string(&shown));
        self.err(FROM_HTML, XML_ERR_COMMENT_NOT_FINISHED, ERR_ERROR, msg);
    }

    /// htmlSkipBogusComment
    fn skip_bogus_comment(&mut self) {
        self.perr(
            XML_HTML_INCORRECTLY_OPENED_COMMENT,
            "Incorrectly opened comment\n",
            None,
            None,
        );
        loop {
            let c = self.cur();
            if c == 0 {
                break;
            }
            self.next();
            if c == b'>' {
                break;
            }
        }
    }

    /// htmlParseDocTypeDecl
    fn parse_doctype_decl(&mut self) {
        self.skip(9);
        self.skip_blanks();
        let name = self.parse_name();
        if name.is_none() {
            self.perr(
                XML_ERR_NAME_REQUIRED,
                "htmlParseDocTypeDecl : no DOCTYPE name !\n",
                None,
                None,
            );
        }
        self.skip_blanks();
        let (uri, ext) = self.parse_external_id();
        self.skip_blanks();
        if self.cur() != b'>' {
            self.perr(
                XML_ERR_DOCTYPE_NOT_FINISHED,
                "DOCTYPE improperly terminated\n",
                None,
                None,
            );
            while self.cur() != 0 && self.cur() != b'>' {
                self.next();
            }
        }
        if self.cur() == b'>' {
            self.next();
        }
        if self.disable_sax == 0 {
            self.sax_internal_subset(name.as_deref(), ext.as_deref(), uri.as_deref());
        }
    }

    /// htmlParserFinishElementParsing
    fn finish_element_parsing(&mut self) {
        if self.cur() == 0 {
            self.auto_close_on_end();
        }
    }

    /// htmlParseElementInternal
    fn parse_element_internal(&mut self) {
        let failed = self.parse_start_tag();
        let name = match self.name() {
            Some(n) if failed != -1 => n.to_string(),
            _ => {
                if self.cur() == b'>' {
                    self.next();
                }
                return;
            }
        };
        let info = tag_lookup(&name);
        if info.is_none() {
            self.perr(XML_HTML_UNKNOWN_TAG, "Tag %s invalid\n", Some(&name), None);
        }
        if self.cur() == b'/' && self.nxt(1) == b'>' {
            self.skip(2);
            self.end_and_pop();
            return;
        }
        if self.cur() == b'>' {
            self.next();
        } else {
            self.perr(
                XML_ERR_GT_REQUIRED,
                "Couldn't find end of Start Tag %s\n",
                Some(&name),
                None,
            );
            if self.name() == Some(name.as_str()) {
                self.nodes.pop();
                self.name_pop();
            }
            self.finish_element_parsing();
            return;
        }
        if let Some(i) = info {
            if i.empty {
                self.end_and_pop();
            }
        }
    }

    /// htmlParseContentInternal
    fn parse_content_internal(&mut self) {
        let mut depth = self.names.len();
        let mut current: Option<String> = self.name().map(|s| s.to_string());
        while self.disable_sax <= 1 {
            if self.cur() == b'<' && self.nxt(1) == b'/' {
                if self.parse_end_tag() && (current.is_some() || self.names.is_empty()) {
                    depth = self.names.len();
                    current = self.name().map(|s| s.to_string());
                }
                continue;
            } else if self.cur() == b'<'
                && (is_ascii_letter(self.nxt(1)) || self.nxt(1) == b'_' || self.nxt(1) == b':')
            {
                let name = self.parse_html_name_non_invasive();
                match name {
                    None => {
                        // unreachable in practice (see libxml2)
                        self.perr(
                            XML_ERR_NAME_REQUIRED,
                            "htmlParseStartTag: invalid element name\n",
                            None,
                            None,
                        );
                        self.finish_element_parsing();
                        current = self.name().map(|s| s.to_string());
                        depth = self.names.len();
                        continue;
                    }
                    Some(n) => {
                        if let Some(cn) = self.name() {
                            if check_auto_close(&n, cn) {
                                self.auto_close(&n);
                                continue;
                            }
                        }
                    }
                }
            }
            // has this node been popped out during parsing of the next element
            if !self.names.is_empty()
                && depth >= self.names.len()
                && current.as_deref() != self.name()
            {
                self.finish_element_parsing();
                current = self.name().map(|s| s.to_string());
                depth = self.names.len();
                continue;
            }
            if self.cur() != 0 && matches!(current.as_deref(), Some("script") | Some("style")) {
                self.parse_script();
            } else if self.cur() == b'<' && self.nxt(1) == b'!' {
                if self.upp(2) == b'D'
                    && self.upp(3) == b'O'
                    && self.upp(4) == b'C'
                    && self.upp(5) == b'T'
                    && self.upp(6) == b'Y'
                    && self.upp(7) == b'P'
                    && self.upp(8) == b'E'
                {
                    self.perr(
                        XML_HTML_STRUCURE_ERROR,
                        "Misplaced DOCTYPE declaration\n",
                        Some("DOCTYPE"),
                        None,
                    );
                    self.parse_doctype_decl();
                } else if self.nxt(2) == b'-' && self.nxt(3) == b'-' {
                    self.parse_comment();
                } else {
                    self.skip_bogus_comment();
                }
            } else if self.cur() == b'<' && self.nxt(1) == b'?' {
                self.parse_pi();
            } else if self.cur() == b'<' && is_ascii_letter(self.nxt(1)) {
                self.parse_element_internal();
                current = self.name().map(|s| s.to_string());
                depth = self.names.len();
            } else if self.cur() == b'<' {
                if self.disable_sax == 0 {
                    self.sax_characters(b"<");
                }
                self.next();
            } else if self.cur() == b'&' {
                self.parse_reference();
            } else if self.cur() == 0 {
                self.auto_close_on_end();
                break;
            } else {
                self.parse_char_data();
            }
        }
    }

    /// htmlParseDocument
    pub fn parse_document(&mut self) -> bool {
        self.detect_encoding();
        if !self.has_encoding && self.buf[self.pos.min(self.buf.len())..].starts_with(b"<?xm") {
            self.switch_to(None);
        }
        self.skip_blanks();
        if self.cur() == 0 {
            self.perr(XML_ERR_DOCUMENT_EMPTY, "Document is empty\n", None, None);
        }
        if self.disable_sax == 0 {
            self.sax_start_document();
        }
        while (self.cur() == b'<'
            && self.nxt(1) == b'!'
            && self.nxt(2) == b'-'
            && self.nxt(3) == b'-')
            || (self.cur() == b'<' && self.nxt(1) == b'?')
        {
            self.parse_comment();
            self.parse_pi();
            self.skip_blanks();
        }
        if self.cur() == b'<'
            && self.nxt(1) == b'!'
            && self.upp(2) == b'D'
            && self.upp(3) == b'O'
            && self.upp(4) == b'C'
            && self.upp(5) == b'T'
            && self.upp(6) == b'Y'
            && self.upp(7) == b'P'
            && self.upp(8) == b'E'
        {
            self.parse_doctype_decl();
        }
        self.skip_blanks();
        while self.disable_sax <= 1
            && ((self.cur() == b'<'
                && self.nxt(1) == b'!'
                && self.nxt(2) == b'-'
                && self.nxt(3) == b'-')
                || (self.cur() == b'<' && self.nxt(1) == b'?'))
        {
            self.parse_comment();
            self.parse_pi();
            self.skip_blanks();
        }
        self.parse_content_internal();
        if self.cur() == 0 {
            self.auto_close_on_end();
        }
        self.sax_end_document();
        if self.options & HTML_PARSE_NODEFDTD == 0 {
            if let Some(d) = self.doc {
                if self.t.int_subset(d).is_none() {
                    self.create_html_int_subset(
                        "html",
                        Some("-//W3C//DTD HTML 4.0 Transitional//EN"),
                        Some("http://www.w3.org/TR/REC-html40/loose.dtd"),
                    );
                }
            }
        }
        self.well_formed
    }
}
