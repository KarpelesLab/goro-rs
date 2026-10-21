//! XML 1.0 parser closely following the structure (and error reporting) of
//! libxml2's parser.c. Events are delivered to a [`Sink`] (SAX2-like); the
//! tree builder lives in `sax2.rs`.

use std::collections::HashMap;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Options (XML_PARSE_*)

pub const XML_PARSE_RECOVER: i64 = 1;
pub const XML_PARSE_NOENT: i64 = 2;
pub const XML_PARSE_DTDLOAD: i64 = 4;
pub const XML_PARSE_DTDATTR: i64 = 8;
pub const XML_PARSE_DTDVALID: i64 = 16;
pub const XML_PARSE_NOERROR: i64 = 32;
pub const XML_PARSE_NOWARNING: i64 = 64;
pub const XML_PARSE_PEDANTIC: i64 = 128;
pub const XML_PARSE_NOBLANKS: i64 = 256;
pub const XML_PARSE_XINCLUDE: i64 = 1024;
pub const XML_PARSE_NONET: i64 = 2048;
pub const XML_PARSE_NSCLEAN: i64 = 8192;
pub const XML_PARSE_NOCDATA: i64 = 16384;
pub const XML_PARSE_COMPACT: i64 = 65536;
pub const XML_PARSE_OLD10: i64 = 131072;
pub const XML_PARSE_HUGE: i64 = 524288;
pub const XML_PARSE_BIG_LINES: i64 = 1 << 22;
pub const XML_PARSE_NO_XXE: i64 = 1 << 23;

// Domains
pub const FROM_PARSER: i32 = 1;
pub const FROM_NAMESPACE: i32 = 3;
pub const FROM_DTD: i32 = 4;
pub const FROM_IO: i32 = 8;
pub const FROM_VALID: i32 = 23;

// Levels
pub const ERR_WARNING: i32 = 1;
pub const ERR_ERROR: i32 = 2;
pub const ERR_FATAL: i32 = 3;

// Error codes
pub const XML_ERR_INTERNAL_ERROR: i32 = 1;
pub const XML_ERR_DOCUMENT_EMPTY: i32 = 4;
pub const XML_ERR_DOCUMENT_END: i32 = 5;
pub const XML_ERR_INVALID_HEX_CHARREF: i32 = 6;
pub const XML_ERR_INVALID_DEC_CHARREF: i32 = 7;
pub const XML_ERR_INVALID_CHARREF: i32 = 8;
pub const XML_ERR_INVALID_CHAR: i32 = 9;
pub const XML_ERR_PEREF_NO_NAME: i32 = 24;
pub const XML_ERR_PEREF_SEMICOL_MISSING: i32 = 25;
pub const XML_ERR_ENTITYREF_SEMICOL_MISSING: i32 = 23;
pub const XML_ERR_UNDECLARED_ENTITY: i32 = 26;
pub const XML_WAR_UNDECLARED_ENTITY: i32 = 27;
pub const XML_ERR_UNPARSED_ENTITY: i32 = 28;
pub const XML_ERR_ENTITY_IS_EXTERNAL: i32 = 29;
pub const XML_ERR_UNSUPPORTED_ENCODING: i32 = 32;
pub const XML_ERR_STRING_NOT_STARTED: i32 = 33;
pub const XML_ERR_STRING_NOT_CLOSED: i32 = 34;
pub const XML_ERR_ENTITY_NOT_STARTED: i32 = 36;
pub const XML_ERR_ENTITY_NOT_FINISHED: i32 = 37;
pub const XML_ERR_LT_IN_ATTRIBUTE: i32 = 38;
pub const XML_ERR_ATTRIBUTE_NOT_STARTED: i32 = 39;
pub const XML_ERR_ATTRIBUTE_NOT_FINISHED: i32 = 40;
pub const XML_ERR_ATTRIBUTE_WITHOUT_VALUE: i32 = 41;
pub const XML_ERR_ATTRIBUTE_REDEFINED: i32 = 42;
pub const XML_ERR_LITERAL_NOT_STARTED: i32 = 43;
pub const XML_ERR_LITERAL_NOT_FINISHED: i32 = 44;
pub const XML_ERR_COMMENT_NOT_FINISHED: i32 = 45;
pub const XML_ERR_PI_NOT_STARTED: i32 = 46;
pub const XML_ERR_PI_NOT_FINISHED: i32 = 47;
pub const XML_ERR_NOTATION_NOT_STARTED: i32 = 48;
pub const XML_ERR_NOTATION_NOT_FINISHED: i32 = 49;
pub const XML_ERR_ATTLIST_NOT_STARTED: i32 = 50;
pub const XML_ERR_ATTLIST_NOT_FINISHED: i32 = 51;
pub const XML_ERR_MIXED_NOT_STARTED: i32 = 52;
pub const XML_ERR_ELEMCONTENT_NOT_STARTED: i32 = 54;
pub const XML_ERR_ELEMCONTENT_NOT_FINISHED: i32 = 55;
pub const XML_ERR_XMLDECL_NOT_FINISHED: i32 = 57;
pub const XML_ERR_DOCTYPE_NOT_FINISHED: i32 = 61;
pub const XML_ERR_MISPLACED_CDATA_END: i32 = 62;
pub const XML_ERR_CDATA_NOT_FINISHED: i32 = 63;
pub const XML_ERR_RESERVED_XML_NAME: i32 = 64;
pub const XML_ERR_SPACE_REQUIRED: i32 = 65;
pub const XML_ERR_SEPARATOR_REQUIRED: i32 = 66;
pub const XML_ERR_NMTOKEN_REQUIRED: i32 = 67;
pub const XML_ERR_NAME_REQUIRED: i32 = 68;
pub const XML_ERR_PCDATA_REQUIRED: i32 = 69;
pub const XML_ERR_URI_REQUIRED: i32 = 70;
pub const XML_ERR_PUBID_REQUIRED: i32 = 71;
pub const XML_ERR_GT_REQUIRED: i32 = 73;
pub const XML_ERR_LTSLASH_REQUIRED: i32 = 74;
pub const XML_ERR_EQUAL_REQUIRED: i32 = 75;
pub const XML_ERR_TAG_NAME_MISMATCH: i32 = 76;
pub const XML_ERR_TAG_NOT_FINISHED: i32 = 77;
pub const XML_ERR_STANDALONE_VALUE: i32 = 78;
pub const XML_ERR_ENCODING_NAME: i32 = 79;
pub const XML_ERR_HYPHEN_IN_COMMENT: i32 = 80;
pub const XML_ERR_INVALID_ENCODING: i32 = 81;
pub const XML_ERR_VALUE_REQUIRED: i32 = 84;
pub const XML_ERR_NOT_WELL_BALANCED: i32 = 85;
pub const XML_ERR_EXTRA_CONTENT: i32 = 86;
pub const XML_ERR_ENTITY_LOOP: i32 = 89;
pub const XML_ERR_URI_FRAGMENT: i32 = 92;
pub const XML_ERR_VERSION_MISSING: i32 = 96;
pub const XML_WAR_UNKNOWN_VERSION: i32 = 97;
pub const XML_WAR_NS_URI: i32 = 99;
pub const XML_WAR_NS_URI_RELATIVE: i32 = 100;
pub const XML_WAR_SPACE_VALUE: i32 = 102;
pub const XML_WAR_ENTITY_REDEFINED: i32 = 107;
pub const XML_ERR_UNKNOWN_VERSION: i32 = 108;
pub const XML_ERR_NAME_TOO_LONG: i32 = 110;
pub const XML_ERR_RESOURCE_LIMIT: i32 = 114;
pub const XML_ERR_REDECL_PREDEF_ENTITY: i32 = 117;
pub const XML_ERR_INT_SUBSET_NOT_FINISHED: i32 = 118;
pub const XML_NS_ERR_XML_NAMESPACE: i32 = 200;
pub const XML_NS_ERR_UNDEFINED_NAMESPACE: i32 = 201;
pub const XML_NS_ERR_QNAME: i32 = 202;
pub const XML_NS_ERR_ATTRIBUTE_REDEFINED: i32 = 203;
pub const XML_NS_ERR_COLON: i32 = 205;
pub const XML_DTD_ID_REDEFINED: i32 = 513;
pub const XML_DTD_UNKNOWN_ENTITY: i32 = 529;

pub fn err_string(code: i32) -> &'static str {
    match code {
        XML_ERR_INVALID_HEX_CHARREF => "CharRef: invalid hexadecimal value",
        XML_ERR_INVALID_DEC_CHARREF => "CharRef: invalid decimal value",
        XML_ERR_INVALID_CHARREF => "CharRef: invalid value",
        XML_ERR_INTERNAL_ERROR => "internal error",
        18 => "PEReference at end of document",
        19 => "PEReference in prolog",
        20 => "PEReference in epilog",
        XML_ERR_PEREF_NO_NAME => "PEReference: no name",
        XML_ERR_PEREF_SEMICOL_MISSING => "PEReference: expecting ';'",
        XML_ERR_ENTITY_LOOP => "Detected an entity reference loop",
        XML_ERR_ENTITY_NOT_STARTED => "EntityValue: \" or ' expected",
        88 => "PEReferences forbidden in internal subset",
        XML_ERR_ENTITY_NOT_FINISHED => "EntityValue: \" or ' expected",
        XML_ERR_ATTRIBUTE_NOT_STARTED => "AttValue: \" or ' expected",
        XML_ERR_LT_IN_ATTRIBUTE => "Unescaped '<' not allowed in attributes values",
        XML_ERR_LITERAL_NOT_STARTED => "SystemLiteral \" or ' expected",
        XML_ERR_LITERAL_NOT_FINISHED => "Unfinished System or Public ID \" or ' expected",
        XML_ERR_MISPLACED_CDATA_END => "Sequence ']]>' not allowed in content",
        XML_ERR_URI_REQUIRED => "SYSTEM or PUBLIC, the URI is missing",
        XML_ERR_PUBID_REQUIRED => "PUBLIC, the Public Identifier is missing",
        XML_ERR_HYPHEN_IN_COMMENT => "Comment must not contain '--' (double-hyphen)",
        XML_ERR_PI_NOT_STARTED => "xmlParsePI : no target name",
        XML_ERR_RESERVED_XML_NAME => "Invalid PI name",
        XML_ERR_NOTATION_NOT_STARTED => "NOTATION: Name expected here",
        XML_ERR_NOTATION_NOT_FINISHED => "'>' required to close NOTATION declaration",
        XML_ERR_VALUE_REQUIRED => "Entity value required",
        XML_ERR_URI_FRAGMENT => "Fragment not allowed",
        XML_ERR_ATTLIST_NOT_STARTED => "'(' required to start ATTLIST enumeration",
        XML_ERR_NMTOKEN_REQUIRED => "NmToken expected in ATTLIST enumeration",
        XML_ERR_ATTLIST_NOT_FINISHED => "')' required to finish ATTLIST enumeration",
        XML_ERR_MIXED_NOT_STARTED => "MixedContentDecl : '|' or ')*' expected",
        XML_ERR_PCDATA_REQUIRED => "MixedContentDecl : '#PCDATA' expected",
        XML_ERR_ELEMCONTENT_NOT_STARTED => "ContentDecl : Name or '(' expected",
        XML_ERR_ELEMCONTENT_NOT_FINISHED => "ContentDecl : ',' '|' or ')' expected",
        21 => "PEReference: forbidden within markup decl in internal subset",
        XML_ERR_GT_REQUIRED => "expected '>'",
        83 => "XML conditional section '[' expected",
        XML_ERR_INT_SUBSET_NOT_FINISHED => "Content error in the internal subset",
        60 => "Content error in the external subset",
        95 => "conditional section INCLUDE or IGNORE keyword expected",
        59 => "XML conditional section not closed",
        56 => "Text declaration '<?xml' required",
        XML_ERR_XMLDECL_NOT_FINISHED => "parsing XML declaration: '?>' expected",
        82 => "external parsed entities cannot be standalone",
        XML_ERR_ENTITYREF_SEMICOL_MISSING => "EntityRef: expecting ';'",
        XML_ERR_DOCTYPE_NOT_FINISHED => "DOCTYPE improperly terminated",
        XML_ERR_LTSLASH_REQUIRED => "EndTag: '</' not found",
        XML_ERR_EQUAL_REQUIRED => "expected '='",
        XML_ERR_STRING_NOT_CLOSED => "String not closed expecting \" or '",
        XML_ERR_STRING_NOT_STARTED => "String not started expecting ' or \"",
        XML_ERR_ENCODING_NAME => "Invalid XML encoding name",
        XML_ERR_STANDALONE_VALUE => "standalone accepts only 'yes' or 'no'",
        XML_ERR_DOCUMENT_EMPTY => "Document is empty",
        XML_ERR_DOCUMENT_END => "Extra content at the end of the document",
        XML_ERR_NOT_WELL_BALANCED => "chunk is not well balanced",
        XML_ERR_EXTRA_CONTENT => "extra content at the end of well balanced chunk",
        XML_ERR_VERSION_MISSING => "Malformed declaration expecting version",
        XML_ERR_NAME_TOO_LONG => "Name too long",
        XML_ERR_INVALID_ENCODING => "Invalid bytes in character encoding",
        XML_ERR_RESOURCE_LIMIT => "Resource limit exceeded",
        XML_ERR_REDECL_PREDEF_ENTITY => "Invalid redeclaration of predefined entity",
        XML_ERR_UNSUPPORTED_ENCODING => "Unsupported encoding",
        XML_ERR_INVALID_CHAR => "Invalid character",
        _ => "Unregistered error message",
    }
}

#[derive(Clone, Debug)]
pub struct XmlError {
    pub domain: i32,
    pub code: i32,
    pub level: i32,
    /// Message, normally with a trailing newline.
    pub message: String,
    pub file: Option<String>,
    pub line: i32,
    pub col: i32,
    /// What libxml2's default (generic) error handler would print, split
    /// the way PHP's error handler groups it (one entry per warning).
    pub formatted: Vec<String>,
    /// Raised while the parser context had no input (PHP's DOM handler
    /// then prints the bare message, without "in Entity, line: N").
    pub no_ctx: bool,
}

impl XmlError {
    pub fn simple(domain: i32, code: i32, level: i32, message: String) -> XmlError {
        let formatted = vec![message.trim_end_matches('\n').to_string()];
        XmlError {
            domain,
            code,
            level,
            message,
            file: None,
            line: 0,
            col: 0,
            formatted,
            no_ctx: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Character classes

pub fn is_char(c: u32) -> bool {
    c == 0x9
        || c == 0xA
        || c == 0xD
        || (0x20..=0xD7FF).contains(&c)
        || (0xE000..=0xFFFD).contains(&c)
        || (0x10000..=0x10FFFF).contains(&c)
}

pub fn is_blank(c: u8) -> bool {
    c == 0x20 || c == 0x9 || c == 0xA || c == 0xD
}

pub fn is_name_start_char(c: u32) -> bool {
    matches!(c, 0x3A | 0x41..=0x5A | 0x5F | 0x61..=0x7A | 0xC0..=0xD6 | 0xD8..=0xF6 | 0xF8..=0x2FF | 0x370..=0x37D | 0x37F..=0x1FFF | 0x200C..=0x200D | 0x2070..=0x218F | 0x2C00..=0x2FEF | 0x3001..=0xD7FF | 0xF900..=0xFDCF | 0xFDF0..=0xFFFD | 0x10000..=0xEFFFF)
}

pub fn is_name_char(c: u32) -> bool {
    is_name_start_char(c)
        || matches!(c, 0x2D | 0x2E | 0x30..=0x39 | 0xB7 | 0x300..=0x36F | 0x203F..=0x2040)
}

/// Is `s` a valid XML Name?
pub fn is_valid_name(s: &str) -> bool {
    let mut it = s.chars();
    match it.next() {
        Some(c) if is_name_start_char(c as u32) => {}
        _ => return false,
    }
    it.all(|c| is_name_char(c as u32))
}

/// Is `s` a valid NCName?
pub fn is_valid_ncname(s: &str) -> bool {
    !s.contains(':') && is_valid_name(s)
}

/// Is `s` a valid QName (prefix:local with both NCNames, or NCName)?
pub fn is_valid_qname(s: &str) -> bool {
    match s.find(':') {
        None => is_valid_ncname(s),
        Some(i) => is_valid_ncname(&s[..i]) && is_valid_ncname(&s[i + 1..]),
    }
}

/// Decode one UTF-8 char at `b[i..]`. Returns (char, len) or None if invalid.
pub fn utf8_char(b: &[u8], i: usize) -> Option<(u32, usize)> {
    let c = *b.get(i)?;
    if c < 0x80 {
        return Some((c as u32, 1));
    }
    let (len, init) = if c & 0xE0 == 0xC0 {
        (2, (c & 0x1F) as u32)
    } else if c & 0xF0 == 0xE0 {
        (3, (c & 0x0F) as u32)
    } else if c & 0xF8 == 0xF0 {
        (4, (c & 0x07) as u32)
    } else {
        return None;
    };
    if i + len > b.len() {
        return None;
    }
    let mut v = init;
    for k in 1..len {
        let cc = b[i + k];
        if cc & 0xC0 != 0x80 {
            return None;
        }
        v = (v << 6) | (cc & 0x3F) as u32;
    }
    // reject overlong / surrogates
    let ok = match len {
        2 => v >= 0x80,
        3 => v >= 0x800 && !(0xD800..=0xDFFF).contains(&v),
        _ => (0x10000..=0x10FFFF).contains(&v),
    };
    if !ok {
        return None;
    }
    Some((v, len))
}

fn push_char(s: &mut Vec<u8>, c: u32) {
    if let Some(ch) = char::from_u32(c) {
        let mut buf = [0u8; 4];
        s.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
    }
}

// ---------------------------------------------------------------------------
// Encodings

/// Convert bytes in `enc` to UTF-8. Returns None for unsupported encodings.
pub fn decode_to_utf8(data: &[u8], enc: &str) -> Option<Vec<u8>> {
    let e = enc.to_ascii_uppercase();
    match e.as_str() {
        "UTF-8" | "UTF8" => Some(data.to_vec()),
        "ISO-8859-1" | "ISO_8859-1" | "LATIN1" | "LATIN-1" | "ISO8859-1" | "ISO-LATIN-1" | "L1"
        | "IBM819" | "CP819" | "CSISOLATIN1" | "ISO-IR-100" | "ISO_8859-1:1987" => {
            let mut out = Vec::with_capacity(data.len());
            for &b in data {
                push_char(&mut out, b as u32);
            }
            Some(out)
        }
        "US-ASCII" | "ASCII" | "US_ASCII" | "ISO646-US" => Some(data.to_vec()),
        "WINDOWS-1252" | "CP1252" => {
            let mut out = Vec::with_capacity(data.len());
            for &b in data {
                push_char(&mut out, cp1252(b));
            }
            Some(out)
        }
        "ISO-8859-15" | "ISO_8859-15" | "LATIN-9" | "LATIN9" => {
            let mut out = Vec::with_capacity(data.len());
            for &b in data {
                let c = match b {
                    0xA4 => 0x20AC,
                    0xA6 => 0x160,
                    0xA8 => 0x161,
                    0xB4 => 0x17D,
                    0xB8 => 0x17E,
                    0xBC => 0x152,
                    0xBD => 0x153,
                    0xBE => 0x178,
                    _ => b as u32,
                };
                push_char(&mut out, c);
            }
            Some(out)
        }
        "UTF-16LE" | "UTF-16" | "UTF16" | "UTF-16BE" => {
            let be = e == "UTF-16BE";
            let mut units = Vec::new();
            let mut i = 0;
            while i + 1 < data.len() {
                let u = if be {
                    ((data[i] as u16) << 8) | data[i + 1] as u16
                } else {
                    ((data[i + 1] as u16) << 8) | data[i] as u16
                };
                units.push(u);
                i += 2;
            }
            let s: String = char::decode_utf16(units)
                .map(|r| r.unwrap_or('\u{FFFD}'))
                .collect();
            Some(s.into_bytes())
        }
        _ => None,
    }
}

fn cp1252(b: u8) -> u32 {
    const T: [u32; 32] = [
        0x20AC, 0x81, 0x201A, 0x192, 0x201E, 0x2026, 0x2020, 0x2021, 0x2C6, 0x2030, 0x160, 0x2039,
        0x152, 0x8D, 0x17D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
        0x2DC, 0x2122, 0x161, 0x203A, 0x153, 0x9D, 0x17E, 0x178,
    ];
    if (0x80..0xA0).contains(&b) {
        T[(b - 0x80) as usize]
    } else {
        b as u32
    }
}

/// Encode UTF-8 text into `enc`. Unrepresentable characters are written as
/// decimal character references (libxml2 behaviour). Returns None for
/// unsupported encodings.
pub fn encode_from_utf8(s: &str, enc: &str) -> Option<Vec<u8>> {
    let e = enc.to_ascii_uppercase();
    match e.as_str() {
        "UTF-8" | "UTF8" => Some(s.as_bytes().to_vec()),
        "ISO-8859-1" | "ISO_8859-1" | "LATIN1" | "LATIN-1" | "ISO8859-1" | "ISO-LATIN-1" | "L1" => {
            let mut out = Vec::new();
            for c in s.chars() {
                let v = c as u32;
                if v < 0x100 {
                    out.push(v as u8);
                } else {
                    out.extend_from_slice(format!("&#{};", v).as_bytes());
                }
            }
            Some(out)
        }
        "US-ASCII" | "ASCII" => {
            let mut out = Vec::new();
            for c in s.chars() {
                let v = c as u32;
                if v < 0x80 {
                    out.push(v as u8);
                } else {
                    out.extend_from_slice(format!("&#{};", v).as_bytes());
                }
            }
            Some(out)
        }
        "WINDOWS-1252" | "CP1252" => {
            let mut out = Vec::new();
            for c in s.chars() {
                let v = c as u32;
                if v < 0x80 || (0xA0..0x100).contains(&v) {
                    out.push(v as u8);
                } else if let Some(b) = (0x80u8..0xA0).find(|b| cp1252(*b) == v) {
                    out.push(b);
                } else {
                    out.extend_from_slice(format!("&#{};", v).as_bytes());
                }
            }
            Some(out)
        }
        "UTF-16LE" | "UTF-16" | "UTF-16BE" => {
            let be = e == "UTF-16BE";
            let mut out = Vec::new();
            if e == "UTF-16" {
                out.extend_from_slice(&[0xFF, 0xFE]);
            }
            for u in s.encode_utf16() {
                if be {
                    out.push((u >> 8) as u8);
                    out.push(u as u8);
                } else {
                    out.push(u as u8);
                    out.push((u >> 8) as u8);
                }
            }
            Some(out)
        }
        _ => None,
    }
}

pub fn encoding_supported(enc: &str) -> bool {
    decode_to_utf8(b"", enc).is_some()
}

// ---------------------------------------------------------------------------
// SAX sink

#[derive(Clone, Debug)]
pub struct SaxAttr {
    pub localname: String,
    pub prefix: Option<String>,
    pub uri: Option<String>,
    pub value: String,
    pub defaulted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntKind {
    InternalGeneral = 1,
    ExternalGeneralParsed = 2,
    ExternalGeneralUnparsed = 3,
    InternalParameter = 4,
    ExternalParameter = 5,
    Predefined = 6,
}

/// xmlElementContentType
pub const CT_PCDATA: u8 = 1;
pub const CT_ELEMENT: u8 = 2;
pub const CT_SEQ: u8 = 3;
pub const CT_OR: u8 = 4;
/// xmlElementContentOccur
pub const OC_ONCE: u8 = 1;
pub const OC_OPT: u8 = 2;
pub const OC_MULT: u8 = 3;
pub const OC_PLUS: u8 = 4;

/// One xmlElementContent node.
#[derive(Clone, Debug, PartialEq)]
pub struct CNode {
    pub ctype: u8,
    pub ocur: u8,
    /// Local name (element nodes).
    pub name: String,
    pub prefix: Option<String>,
    pub c1: Option<usize>,
    pub c2: Option<usize>,
    pub parent: Option<usize>,
}

/// Element content model (for <!ELEMENT>): libxml2's binary
/// xmlElementContent tree, stored in an arena.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ElemContent {
    pub nodes: Vec<CNode>,
    pub root: usize,
}

impl ElemContent {
    /// xmlNewDocElementContent (the name is split as a QName).
    pub fn new_node(&mut self, ctype: u8, name: Option<&str>) -> usize {
        let (prefix, local) = match name {
            Some(n) => match n.find(':') {
                Some(i) if i > 0 && i + 1 < n.len() => {
                    (Some(n[..i].to_string()), n[i + 1..].to_string())
                }
                _ => (None, n.to_string()),
            },
            None => (None, String::new()),
        };
        self.nodes.push(CNode {
            ctype,
            ocur: OC_ONCE,
            name: local,
            prefix,
            c1: None,
            c2: None,
            parent: None,
        });
        self.nodes.len() - 1
    }

    pub fn set_c1(&mut self, n: usize, c: usize) {
        self.nodes[n].c1 = Some(c);
        self.nodes[c].parent = Some(n);
    }

    pub fn set_c2(&mut self, n: usize, c: usize) {
        self.nodes[n].c2 = Some(c);
        self.nodes[c].parent = Some(n);
    }

    fn occur_str(o: u8) -> &'static str {
        match o {
            OC_OPT => "?",
            OC_MULT => "*",
            OC_PLUS => "+",
            _ => "",
        }
    }

    fn qname(&self, n: usize) -> String {
        let c = &self.nodes[n];
        match &c.prefix {
            Some(p) => format!("{}:{}", p, c.name),
            None => c.name.clone(),
        }
    }

    /// xmlBufDumpElementContent (DTD serialization).
    pub fn dump(&self) -> String {
        let mut out = String::from("(");
        let content = self.root;
        let mut cur = content;
        loop {
            match self.nodes[cur].ctype {
                CT_PCDATA => out.push_str("#PCDATA"),
                CT_ELEMENT => out.push_str(&self.qname(cur)),
                _ => {
                    let c = &self.nodes[cur];
                    if cur != content {
                        if let Some(p) = c.parent {
                            if c.ctype != self.nodes[p].ctype || c.ocur != OC_ONCE {
                                out.push('(');
                            }
                        }
                    }
                    match c.c1 {
                        Some(c1) => {
                            cur = c1;
                            continue;
                        }
                        None => return out,
                    }
                }
            }
            while cur != content {
                let parent = match self.nodes[cur].parent {
                    Some(p) => p,
                    None => return out,
                };
                let c = &self.nodes[cur];
                if (c.ctype == CT_OR || c.ctype == CT_SEQ)
                    && (c.ctype != self.nodes[parent].ctype || c.ocur != OC_ONCE)
                {
                    out.push(')');
                }
                out.push_str(Self::occur_str(c.ocur));
                if Some(cur) == self.nodes[parent].c1 {
                    if self.nodes[parent].ctype == CT_SEQ {
                        out.push_str(" , ");
                    } else if self.nodes[parent].ctype == CT_OR {
                        out.push_str(" | ");
                    }
                    match self.nodes[parent].c2 {
                        Some(c2) => cur = c2,
                        None => return out,
                    }
                    break;
                }
                cur = parent;
            }
            if cur == content {
                break;
            }
        }
        out.push(')');
        out.push_str(Self::occur_str(self.nodes[content].ocur));
        out
    }

    /// xmlSnprintfElementContent with a 5000 byte buffer.
    pub fn snprintf(&self, englob: bool) -> String {
        let mut buf = String::new();
        self.snprintf_rec(&mut buf, 5000, Some(self.root), englob);
        buf
    }

    fn snprintf_rec(&self, buf: &mut String, size: usize, content: Option<usize>, englob: bool) {
        let n = match content {
            Some(n) => n,
            None => return,
        };
        let len = buf.len();
        if size - len < 50 {
            if size - len > 4 && !buf.ends_with('.') {
                buf.push_str(" ...");
            }
            return;
        }
        if englob {
            buf.push('(');
        }
        let c = &self.nodes[n];
        match c.ctype {
            CT_PCDATA => buf.push_str("#PCDATA"),
            CT_ELEMENT => {
                let mut qlen = c.name.len();
                if let Some(p) = &c.prefix {
                    qlen += p.len() + 1;
                }
                if size - len < qlen + 10 {
                    buf.push_str(" ...");
                    return;
                }
                buf.push_str(&self.qname(n));
            }
            CT_SEQ | CT_OR => {
                let sep = if c.ctype == CT_SEQ { " , " } else { " | " };
                let other = if c.ctype == CT_SEQ { CT_OR } else { CT_SEQ };
                let c1g =
                    c.c1.map(|x| matches!(self.nodes[x].ctype, CT_OR | CT_SEQ))
                        .unwrap_or(false);
                self.snprintf_rec(buf, size, c.c1, c1g);
                let len = buf.len();
                if size - len < 50 {
                    if size - len > 4 && !buf.ends_with('.') {
                        buf.push_str(" ...");
                    }
                    return;
                }
                buf.push_str(sep);
                let c2g =
                    c.c2.map(|x| {
                        (self.nodes[x].ctype == other || self.nodes[x].ocur != OC_ONCE)
                            && self.nodes[x].ctype != CT_ELEMENT
                    })
                    .unwrap_or(false);
                self.snprintf_rec(buf, size, c.c2, c2g);
            }
            _ => {}
        }
        if size - buf.len() <= 2 {
            return;
        }
        if englob {
            buf.push(')');
        }
        buf.push_str(Self::occur_str(c.ocur));
    }
}
/// Element declaration type (XML_ELEMENT_TYPE_*): 1 EMPTY, 2 ANY, 3 MIXED, 4 ELEMENT
#[derive(Clone, Debug)]
pub struct ElementDecl {
    pub name: String,
    pub etype: u8,
    pub content: Option<ElemContent>,
}

#[derive(Clone, Debug)]
pub struct AttrDeclInfo {
    pub elem: String,
    pub name: String,
    /// 1 CDATA, 2 ID, 3 IDREF, 4 IDREFS, 5 ENTITY, 6 ENTITIES, 7 NMTOKEN, 8 NMTOKENS, 9 ENUMERATION, 10 NOTATION
    pub atype: u8,
    /// 1 NONE, 2 REQUIRED, 3 IMPLIED, 4 FIXED
    pub def: u8,
    pub default: Option<String>,
    pub tree: Vec<String>,
}

pub trait Sink {
    fn start_document(&mut self, _version: &str, _encoding: Option<&str>, _standalone: i32) {}
    fn end_document(&mut self, _encoding: Option<&str>) {}
    fn internal_subset(
        &mut self,
        _name: Option<&str>,
        _ext_id: Option<&str>,
        _sys_id: Option<&str>,
    ) {
    }
    fn external_subset(
        &mut self,
        _name: Option<&str>,
        _ext_id: Option<&str>,
        _sys_id: Option<&str>,
    ) {
    }
    fn entity_decl(
        &mut self,
        _name: &str,
        _kind: EntKind,
        _public: Option<&str>,
        _system: Option<&str>,
        _content: Option<&str>,
        _orig: Option<&str>,
    ) {
    }
    fn unparsed_entity_decl(
        &mut self,
        _name: &str,
        _public: Option<&str>,
        _system: Option<&str>,
        _notation: &str,
    ) {
    }
    fn notation_decl(&mut self, _name: &str, _public: Option<&str>, _system: Option<&str>) {}
    fn element_decl(&mut self, _decl: &ElementDecl) {}
    fn attribute_decl(&mut self, _decl: &AttrDeclInfo) {}
    fn start_element(
        &mut self,
        _p: &mut ParserState,
        _localname: &str,
        _prefix: Option<&str>,
        _uri: Option<&str>,
        _nsdecls: &[(Option<String>, String)],
        _attrs: &[SaxAttr],
        _nb_defaulted: usize,
        _line: u32,
    ) {
    }
    fn end_element(
        &mut self,
        _p: &mut ParserState,
        _localname: &str,
        _prefix: Option<&str>,
        _uri: Option<&str>,
    ) {
    }
    fn characters(&mut self, _p: &mut ParserState, _text: &str) {}
    fn ignorable_whitespace(&mut self, _p: &mut ParserState, _text: &str) {}
    fn cdata_block(&mut self, _p: &mut ParserState, _text: &str) {}
    fn comment(&mut self, _p: &mut ParserState, _text: &str) {}
    fn pi(&mut self, _p: &mut ParserState, _target: &str, _data: Option<&str>) {}
    fn reference(&mut self, _p: &mut ParserState, _name: &str) {}
    // Declaration callbacks with access to the parser state (for
    // diagnostics); they default to the plain callbacks.
    fn end_document_p(&mut self, _p: &mut ParserState, encoding: Option<&str>) {
        self.end_document(encoding)
    }
    #[allow(clippy::too_many_arguments)]
    fn entity_decl_p(
        &mut self,
        _p: &mut ParserState,
        name: &str,
        kind: EntKind,
        public: Option<&str>,
        system: Option<&str>,
        content: Option<&str>,
        orig: Option<&str>,
    ) {
        self.entity_decl(name, kind, public, system, content, orig)
    }
    fn unparsed_entity_decl_p(
        &mut self,
        _p: &mut ParserState,
        name: &str,
        public: Option<&str>,
        system: Option<&str>,
        notation: &str,
    ) {
        self.unparsed_entity_decl(name, public, system, notation)
    }
    fn notation_decl_p(
        &mut self,
        _p: &mut ParserState,
        name: &str,
        public: Option<&str>,
        system: Option<&str>,
    ) {
        self.notation_decl(name, public, system)
    }
    fn element_decl_p(&mut self, _p: &mut ParserState, decl: &ElementDecl) {
        self.element_decl(decl)
    }
    fn attribute_decl_p(&mut self, _p: &mut ParserState, decl: &AttrDeclInfo) {
        self.attribute_decl(decl)
    }
    /// Called on the first reference to an internal entity when entities are
    /// not substituted. Return true if the sink wants the entity content
    /// (subsequent events up to `end_entity_content` belong to the entity).
    fn begin_entity_content(&mut self, _name: &str) -> bool {
        false
    }
    fn end_entity_content(&mut self, _name: &str) {}
    /// Whether the sink builds a tree (affects entity handling).
    fn builds_tree(&self) -> bool {
        false
    }
    /// The DTD-free part of libxml2's areBlanks() heuristic: inspect the
    /// current node. `next_lt` / `next_lt_slash` describe the lookahead.
    fn are_blanks_tree(
        &self,
        _next_lt: bool,
        _next_lt_slash: bool,
        _elem_type: Option<u8>,
    ) -> bool {
        false
    }
    /// Current element name (for areBlanks DTD lookup).
    fn current_element(&self) -> Option<(String, Option<String>)> {
        None
    }
    /// SAX getEntity, called (only with `ParserState::sax_get_entity`) for
    /// each general entity reference outside the DTD, with the entity the
    /// parser knows (predefined or declared) and whether the reference is in
    /// content rather than in an attribute value.
    fn get_entity(
        &mut self,
        _p: &mut ParserState,
        _name: &str,
        _ent: Option<&PEntity>,
        _in_content: bool,
    ) {
    }
    /// Error notification (the sink may collect or format it).
    fn error(&mut self, _err: &XmlError) {}
    /// Called when an ID attribute is found that is already defined
    /// Whether the sink wants the external subset loaded and parsed (the
    /// SAX2 externalSubset handler does so when validating or loading the
    /// DTD).
    fn wants_external_subset(&self) -> bool {
        false
    }
    fn wants_line_numbers(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Parser state

pub struct Input {
    pub data: Rc<Vec<u8>>,
    pub pos: usize,
    pub filename: Option<String>,
    pub entity: Option<String>,
    /// line/col cache
    lc_pos: usize,
    lc_line: u32,
    lc_col: u32,
    pub base_line: u32,
    /// Column at `data[0]` (non-1 after the push parser shrank the buffer).
    pub base_col: u32,
    /// Bytes dropped from the front of `data` (xmlParserInput.consumed).
    pub consumed: u64,
}

impl Input {
    fn new(data: Rc<Vec<u8>>, filename: Option<String>) -> Input {
        Input {
            data,
            pos: 0,
            filename,
            entity: None,
            lc_pos: 0,
            lc_line: 1,
            lc_col: 1,
            base_line: 1,
            base_col: 1,
            consumed: 0,
        }
    }
    /// Drop consumed bytes before `pos - keep` (xmlParserShrink).
    fn shrink(&mut self, keep: usize) {
        if self.pos <= keep {
            return;
        }
        let cut = self.pos - keep;
        let (l, c) = self.line_col(cut);
        let d = Rc::make_mut(&mut self.data);
        d.drain(..cut);
        self.pos -= cut;
        self.consumed += cut as u64;
        self.base_line = l;
        self.base_col = c;
        self.lc_pos = 0;
        self.lc_line = l;
        self.lc_col = c;
    }
    fn line_col(&mut self, pos: usize) -> (u32, u32) {
        if pos < self.lc_pos {
            self.lc_pos = 0;
            self.lc_line = self.base_line;
            self.lc_col = self.base_col;
        }
        let d = &self.data;
        let mut i = self.lc_pos;
        let mut line = self.lc_line;
        let mut col = self.lc_col;
        while i < pos && i < d.len() {
            let c = d[i];
            if c == b'\n' {
                line += 1;
                col = 1;
            } else if c & 0xC0 != 0x80 {
                col += 1;
            }
            i += 1;
        }
        self.lc_pos = i;
        self.lc_line = line;
        self.lc_col = col;
        (line, col)
    }
}

#[derive(Clone, Debug)]
pub struct PEntity {
    pub name: String,
    pub kind: EntKind,
    pub content: String,
    pub orig: Option<String>,
    pub public: Option<String>,
    pub system: Option<String>,
    pub notation: Option<String>,
    pub parsed: bool,
    pub expanding: bool,
    /// Resolved URI of an external entity (ent->URI).
    pub uri: Option<String>,
}

struct NsEntry {
    prefix: Option<String>,
    uri: String,
    elem: usize,
}

/// State shared with the sink during parsing (positions, options).
pub struct ParserState {
    pub inputs: Vec<Input>,
    pub options: i64,
    pub well_formed: bool,
    pub ns_well_formed: bool,
    pub valid: bool,
    pub disable_sax: u8,
    pub errors: Vec<XmlError>,
    pub nb_errors: usize,
    pub nb_warnings: usize,
    pub replace_entities: bool,
    pub keep_blanks: bool,
    pub recovery: bool,
    pub in_subset: u8,
    pub standalone: i32,
    pub version: Option<String>,
    pub encoding: Option<String>,
    pub input_encoding: Option<String>,
    pub has_external_subset: bool,
    pub has_perefs: bool,
    pub html: bool,
    pub stop: bool,
    /// Errors are being suppressed (e.g. internal reparse).
    pub quiet: bool,
    /// When set, ID duplicates are checked by the sink.
    pub ids: HashMap<String, ()>,
    /// Code of the last error (ctxt->errNo); warnings don't set it.
    pub err_no: i32,
    /// Level of the last reported diagnostic (ctxt->lastError.level).
    pub last_err_level: i32,
    /// SAX1 mode (ctxt->sax2 == 0): no namespace processing, element and
    /// attribute names are reported as written. Push parser only.
    pub sax1: bool,
    /// Call `Sink::get_entity` on general entity references (a SAX
    /// getEntity callback with XML_PARSE_OLDSAX, as ext/xml installs).
    pub sax_get_entity: bool,
    /// Base URI for resolving system ids when the input has no filename
    /// (ctxt->directory; PHP's DOM sets it to the current directory when
    /// loading from memory).
    pub directory: Option<String>,
}

impl ParserState {
    /// (line, column, byte index) of the current input position, as
    /// xmlParserInput line/col and consumed + (cur - base).
    pub fn position(&mut self) -> (u32, u32, u64) {
        let n = self.inputs.len();
        if n == 0 {
            return (0, 0, 0);
        }
        let inp = &mut self.inputs[n - 1];
        let pos = inp.pos;
        let (l, c) = inp.line_col(pos);
        (l, c, inp.consumed + pos as u64)
    }

    pub fn cur_line(&mut self) -> u32 {
        // line of the "real" input (entity inputs without filename use parent)
        let n = self.inputs.len();
        if n == 0 {
            return 0;
        }
        let pos = self.inputs[n - 1].pos;
        self.inputs[n - 1].line_col(pos).0
    }

    /// Report an error (xmlCtxtVErr).
    pub fn report(&mut self, domain: i32, code: i32, level: i32, msg: String) -> Option<XmlError> {
        self.report_node(domain, code, level, msg, None, false)
    }

    /// xmlCtxtVErr with a node (validity errors): the node supplies the
    /// "element X:" prefix of the generic output and the file/line when
    /// the input has none. `no_input` emulates a NULL ctxt->input (as
    /// xmlValidateDocumentFinal arranges): no position, no context lines.
    pub fn report_node(
        &mut self,
        domain: i32,
        code: i32,
        level: i32,
        msg: String,
        node: Option<&NodeRef>,
        no_input: bool,
    ) -> Option<XmlError> {
        if self.stop {
            return None;
        }
        if level == ERR_WARNING {
            if self.nb_warnings >= 100 {
                return None;
            }
            self.nb_warnings += 1;
        } else {
            if code == XML_ERR_RESOURCE_LIMIT
                || code == XML_ERR_ENTITY_LOOP
                || code == XML_ERR_INTERNAL_ERROR
            {
                self.disable_sax = 2;
            } else {
                if self.nb_errors >= 100 && (level < ERR_FATAL || !self.well_formed) {
                    return None;
                }
                if level == ERR_FATAL && !self.recovery {
                    self.disable_sax = 1;
                }
            }
            if level == ERR_FATAL {
                self.well_formed = false;
            }
            self.err_no = code;
            self.nb_errors += 1;
        }
        self.last_err_level = level;
        let silent = self.quiet
            || (self.options & XML_PARSE_NOERROR != 0)
            || (level == ERR_WARNING && self.options & XML_PARSE_NOWARNING != 0);
        // file / line from the current input; internal entities report the
        // position in their parent
        let n = self.inputs.len();
        let mut idx = n.saturating_sub(1);
        let mut entity_idx = None;
        if n > 1 && self.inputs[idx].filename.is_none() {
            entity_idx = Some(idx);
            idx = n - 2;
        }
        let (mut file, mut line, col) = if n > 0 && !no_input {
            let pos = self.inputs[idx].pos;
            let (l, c) = self.inputs[idx].line_col(pos);
            (self.inputs[idx].filename.clone(), l as i32, c as i32)
        } else {
            (None, 0, 0)
        };
        if let Some(nr) = node {
            if file.is_none() {
                file = nr.url.clone();
            }
            if line == 0 {
                line = nr.line;
            }
        }
        let ename = node.and_then(|nr| nr.elem_name.as_deref());
        let f = if no_input || n == 0 {
            vec![format_no_input(
                file.as_deref(),
                line,
                ename,
                domain,
                level,
                &msg,
            )]
        } else {
            format_generic(
                &self.inputs,
                idx,
                entity_idx,
                domain,
                code,
                level,
                line,
                ename,
                &msg,
            )
        };
        let e = XmlError {
            domain,
            code,
            level,
            message: msg,
            file,
            line,
            col,
            formatted: f,
            no_ctx: no_input,
        };
        if !silent {
            self.errors.push(e.clone());
            Some(e)
        } else {
            None
        }
    }
}

/// Node information attached to a (validity) error.
#[derive(Clone, Debug, Default)]
pub struct NodeRef {
    /// Name of the element the error is about (xmlFormatError prints
    /// "element NAME: " for element nodes).
    pub elem_name: Option<String>,
    /// The node's document URL.
    pub url: Option<String>,
    /// The node's line.
    pub line: i32,
}

fn domain_str(domain: i32) -> &'static str {
    match domain {
        FROM_PARSER => "parser ",
        FROM_NAMESPACE => "namespace ",
        FROM_DTD | FROM_VALID => "validity ",
        FROM_IO => "I/O ",
        _ => "",
    }
}

/// xmlFormatError when the parser context has no input.
pub fn format_no_input(
    file: Option<&str>,
    line: i32,
    ename: Option<&str>,
    domain: i32,
    level: i32,
    msg: &str,
) -> String {
    let mut f = String::new();
    if let Some(fname) = file {
        f.push_str(&format!("{}:{}: ", fname, line));
    } else if line != 0 && (domain == FROM_PARSER || domain == FROM_DTD) {
        f.push_str(&format!("Entity: line {}: ", line));
    }
    if let Some(n) = ename {
        f.push_str(&format!("element {}: ", n));
    }
    f.push_str(domain_str(domain));
    f.push_str(if level == ERR_WARNING {
        "warning : "
    } else {
        "error : "
    });
    f.push_str(msg.trim_end_matches('\n'));
    f
}

/// xmlFormatError output, chunked per generic-channel line as PHP sees it.
#[allow(clippy::too_many_arguments)]
fn format_generic(
    inputs: &[Input],
    idx: usize,
    entity_idx: Option<usize>,
    domain: i32,
    code: i32,
    level: i32,
    line: i32,
    ename: Option<&str>,
    msg: &str,
) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut f = String::new();
    if !inputs.is_empty() {
        if let Some(fname) = &inputs[idx].filename {
            f.push_str(&format!("{}:{}: ", fname, line));
        } else if line != 0 && domain == FROM_PARSER {
            f.push_str(&format!("Entity: line {}: ", line));
        }
    }
    if let Some(n) = ename {
        f.push_str(&format!("element {}: ", n));
    }
    f.push_str(domain_str(domain));
    f.push_str(match level {
        ERR_WARNING => "warning : ",
        _ => "error : ",
    });
    f.push_str(msg.trim_end_matches('\n'));
    chunks.push(f);
    if !inputs.is_empty() {
        if code == XML_ERR_INVALID_ENCODING {
            let inp = &inputs[idx];
            let mut b = String::from("Bytes:");
            for i in 0..4 {
                if inp.pos + i >= inp.data.len() {
                    break;
                }
                b.push_str(&format!(" 0x{:02X}", inp.data[inp.pos + i]));
            }
            chunks.push(b);
        }
        let (c, m) = context_lines(&inputs[idx]);
        chunks.push(c);
        chunks.push(m);
        if let Some(ei) = entity_idx {
            let mut tmp = Input::new(inputs[ei].data.clone(), None);
            tmp.pos = inputs[ei].pos;
            let (el, _) = tmp.line_col(tmp.pos);
            chunks.push(format!("Entity: line {}: ", el));
            let (c, m) = context_lines(&inputs[ei]);
            chunks.push(c);
            chunks.push(m);
        }
    }
    chunks
}

fn context_lines(input: &Input) -> (String, String) {
    let w = context_window(input);
    let mut it = w.split('\n');
    let a = it.next().unwrap_or("").to_string();
    let b = it.next().unwrap_or("").to_string();
    (a, b)
}

/// xmlParserPrintFileContextInternal for the current position of an input.
pub fn context_window(input: &Input) -> String {
    let d = &input.data;
    let size = 80usize;
    let end = d.len();
    let mut cur = input.pos.min(end);
    let base = 0usize;
    let at = |i: usize| -> u8 { if i < end { d[i] } else { 0 } };
    while cur > base && (at(cur) == b'\n' || at(cur) == b'\r') {
        cur -= 1;
    }
    let mut n = 0;
    while n < size && cur > base && at(cur) != b'\n' && at(cur) != b'\r' {
        cur -= 1;
        n += 1;
    }
    if n > 0 && (at(cur) == b'\n' || at(cur) == b'\r') {
        cur += 1;
    } else {
        while cur < input.pos && (at(cur) & 0xC0) == 0x80 {
            cur += 1;
        }
    }
    let mut col = input.pos.min(end) - cur;
    let start = cur;
    n = 0;
    while at(cur) != 0 && at(cur) != b'\n' && at(cur) != b'\r' {
        let len = match utf8_char(d, cur) {
            Some((_, l)) => l,
            None => break,
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
    let content = &d[start..start + n];
    let mut out = String::from_utf8_lossy(content).into_owned();
    out.push('\n');
    let mut marker: Vec<u8> = content.to_vec();
    for i in 0..col {
        if marker[i] != b'\t' {
            marker[i] = b' ';
        }
    }
    marker.truncate(col);
    marker.push(b'^');
    out.push_str(&String::from_utf8_lossy(&marker));
    out.push('\n');
    out
}

struct NameEntry {
    name: String,
    prefix: Option<String>,
    uri: Option<String>,
    line: u32,
    ns_nr: usize,
    has_node: bool,
}

pub struct Parser<'a, S: Sink> {
    pub st: ParserState,
    pub sink: &'a mut S,
    entities: HashMap<String, PEntity>,
    pentities: HashMap<String, PEntity>,
    ns: Vec<NsEntry>,
    ns_min: usize,
    elem_id: usize,
    names: Vec<NameEntry>,
    space: Vec<i32>,
    /// (elem qname, attr qname) -> attribute type for normalization
    atts_special: HashMap<(String, String), u8>,
    /// elem qname -> defaults (prefix, localname, value)
    atts_default: HashMap<String, Vec<(Option<String>, String, String)>>,
    elem_decls: HashMap<String, u8>,
    int_sub_name: Option<String>,
    ext_sub_uri: Option<String>,
    ext_sub_system: Option<String>,
    depth_limit: usize,
    // Push parser (xmlParseChunk) state
    push_state: PushState,
    check_index: usize,
    end_check_state: u8,
    /// Encoding the raw input is converted from, for appended chunks.
    conv_enc: Option<String>,
    /// Raw bytes of an incomplete UTF-16 unit at the end of a chunk.
    conv_pending: Vec<u8>,
    /// Character data may continue in a later chunk (an incomplete UTF-8
    /// sequence at the end of the buffer is not an error yet).
    partial: bool,
}

/// xmlParserInputState for the push parser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushState {
    Start,
    XmlDecl,
    Misc,
    Prolog,
    Epilog,
    StartTag,
    Content,
    EndTag,
    Dtd,
    Eof,
}

const NS_XML: &str = "http://www.w3.org/XML/1998/namespace";

impl<'a, S: Sink> Parser<'a, S> {
    pub fn new(sink: &'a mut S, data: Vec<u8>, filename: Option<String>, options: i64) -> Self {
        let st = ParserState {
            inputs: vec![Input::new(Rc::new(data), filename)],
            options,
            well_formed: true,
            ns_well_formed: true,
            valid: true,
            disable_sax: 0,
            errors: Vec::new(),
            nb_errors: 0,
            nb_warnings: 0,
            replace_entities: options & XML_PARSE_NOENT != 0,
            keep_blanks: options & XML_PARSE_NOBLANKS == 0,
            recovery: options & XML_PARSE_RECOVER != 0,
            in_subset: 0,
            standalone: -1,
            version: None,
            encoding: None,
            input_encoding: None,
            has_external_subset: false,
            has_perefs: false,
            html: false,
            stop: false,
            quiet: false,
            ids: HashMap::new(),
            err_no: 0,
            last_err_level: 0,
            sax1: false,
            sax_get_entity: false,
            directory: None,
        };
        Parser {
            st,
            sink,
            entities: HashMap::new(),
            pentities: HashMap::new(),
            ns: Vec::new(),
            ns_min: 0,
            elem_id: 0,
            names: Vec::new(),
            space: Vec::new(),
            atts_special: HashMap::new(),
            atts_default: HashMap::new(),
            elem_decls: HashMap::new(),
            int_sub_name: None,
            ext_sub_uri: None,
            ext_sub_system: None,
            depth_limit: if options & XML_PARSE_HUGE != 0 {
                2048
            } else {
                256
            },
            push_state: PushState::Start,
            check_index: 0,
            end_check_state: 0,
            conv_enc: None,
            conv_pending: Vec::new(),
            partial: false,
        }
    }

    /// xmlSwitchEncodingName before parsing started: the main input (and
    /// data pushed later) is in `enc`, whatever the document declares.
    pub fn switch_encoding(&mut self, enc: &str) {
        let up = enc.to_ascii_uppercase();
        if up != "UTF-8" && up != "UTF8" {
            let raw = self.st.inputs[0].data[self.st.inputs[0].pos..].to_vec();
            self.st.inputs[0].data = Rc::new(Vec::new());
            self.st.inputs[0].pos = 0;
            self.conv_enc = Some(enc.to_string());
            self.push_append(&raw);
        }
        self.st.input_encoding = Some(enc.to_string());
    }

    // ------------------------------------------------------------------
    // Low-level input access

    #[inline]
    fn inp(&self) -> &Input {
        self.st.inputs.last().unwrap()
    }
    #[inline]
    fn inp_mut(&mut self) -> &mut Input {
        self.st.inputs.last_mut().unwrap()
    }
    #[inline]
    fn cur(&self) -> u8 {
        let i = self.inp();
        if i.pos < i.data.len() {
            i.data[i.pos]
        } else {
            0
        }
    }
    #[inline]
    fn nxt(&self, n: usize) -> u8 {
        let i = self.inp();
        let p = i.pos + n;
        if p < i.data.len() { i.data[p] } else { 0 }
    }
    #[inline]
    fn at_end(&self) -> bool {
        let i = self.inp();
        i.pos >= i.data.len()
    }
    #[inline]
    fn skip(&mut self, n: usize) {
        let i = self.inp_mut();
        i.pos = (i.pos + n).min(i.data.len());
    }
    fn cmp(&self, s: &[u8]) -> bool {
        let i = self.inp();
        i.data.len() >= i.pos + s.len() && &i.data[i.pos..i.pos + s.len()] == s
    }
    fn pos(&self) -> usize {
        self.inp().pos
    }
    fn stopped(&self) -> bool {
        self.st.disable_sax >= 2 || self.st.stop
    }
    fn sax_ok(&self) -> bool {
        self.st.disable_sax == 0
    }

    /// Current character (decoded). Returns (char, len); invalid UTF-8 is
    /// reported once and the byte is treated as Latin-1.
    fn cur_char(&mut self) -> (u32, usize) {
        if self.cur() == b'\r' {
            // 2.11 End-of-Line Handling (xmlCurrentChar skips the CR of
            // a CR LF pair as a side effect)
            if self.nxt(1) == b'\n' {
                self.skip(1);
            }
            return (0xA, 1);
        }
        self.cur_char_raw()
    }

    /// The current character without end-of-line translation (no side
    /// effect on the position).
    fn cur_char_raw(&mut self) -> (u32, usize) {
        let c = self.cur();
        if c < 0x80 {
            if c == 0 && self.at_end() {
                return (0, 0);
            }
            return (c as u32, 1);
        }
        let pos = self.pos();
        match utf8_char(&self.inp().data, pos) {
            Some(v) => v,
            None if self.partial && incomplete_utf8_at_end(&self.inp().data, pos) => (0, 0),
            None => {
                self.encoding_error();
                // the rest of the input is now Latin-1 converted to UTF-8
                utf8_char(&self.inp().data, pos).unwrap_or((c as u32, 1))
            }
        }
    }

    fn encoding_error(&mut self) {
        // libxml2: "Input is not proper UTF-8, indicate encoding !"
        let i = self.inp();
        let incomplete = {
            let c = i.data[i.pos];
            let need = if c & 0xE0 == 0xC0 {
                2
            } else if c & 0xF0 == 0xE0 {
                3
            } else if c & 0xF8 == 0xF0 {
                4
            } else {
                1
            };
            i.pos + need > i.data.len()
        };
        if incomplete {
            let c = i.data[i.pos];
            self.fatal_msg(
                XML_ERR_INVALID_ENCODING,
                format!("Incomplete UTF-8 sequence starting with {:02X}\n", c),
            );
        } else {
            self.fatal_msg(
                XML_ERR_INVALID_ENCODING,
                "Input is not proper UTF-8, indicate encoding !\n".to_string(),
            );
        }
        // libxml2 switches the input to ISO-8859-1 after this error: convert the
        // rest of the input so that further parsing works.
        let pos = self.pos();
        let inp = self.inp_mut();
        let rest: Vec<u8> = inp.data[pos..].to_vec();
        let mut conv = inp.data[..pos].to_vec();
        for b in rest {
            push_char(&mut conv, b as u32);
        }
        inp.data = Rc::new(conv);
        if self.st.inputs.len() == 1 {
            self.conv_enc = Some("ISO-8859-1".into());
        }
    }

    fn skip_blanks(&mut self) -> usize {
        let mut n = 0;
        while is_blank(self.cur()) {
            self.skip(1);
            n += 1;
        }
        n
    }

    /// SKIP_BLANKS_PE: skip blanks, handling PE references in the DTD.
    fn skip_blanks_pe(&mut self) -> usize {
        let mut n = 0;
        loop {
            if is_blank(self.cur()) {
                self.skip(1);
                n += 1;
            } else if self.cur() == b'%' && self.st.in_subset != 0 {
                // PE reference within markup declaration
                let nb = self.nxt(1);
                if is_blank(nb) || nb == 0 {
                    break;
                }
                self.parse_pe_reference(false);
                n += 1;
            } else if self.at_end()
                && self.st.inputs.len() > 1
                && self.inp().entity.is_some()
                && self.st.in_subset != 0
            {
                self.st.inputs.pop();
                n += 1;
            } else {
                break;
            }
        }
        n
    }

    // ------------------------------------------------------------------
    // Errors

    fn err_full(&mut self, domain: i32, code: i32, level: i32, msg: String) {
        if let Some(e) = self.st.report(domain, code, level, msg) {
            self.sink.error(&e);
        }
    }
    fn fatal(&mut self, code: i32, info: Option<&str>) {
        let m = err_string(code);
        let msg = match info {
            None => format!("{}\n", m),
            Some(i) => format!("{}: {}\n", m, i),
        };
        self.err_full(FROM_PARSER, code, ERR_FATAL, msg);
    }
    fn fatal_msg(&mut self, code: i32, msg: String) {
        self.err_full(FROM_PARSER, code, ERR_FATAL, msg);
    }
    fn err_msg(&mut self, code: i32, msg: String) {
        self.err_full(FROM_PARSER, code, ERR_ERROR, msg);
    }
    fn warning(&mut self, code: i32, msg: String) {
        self.err_full(FROM_PARSER, code, ERR_WARNING, msg);
    }
    fn ns_err(&mut self, code: i32, msg: String) {
        self.st.ns_well_formed = false;
        self.err_full(FROM_NAMESPACE, code, ERR_ERROR, msg);
    }
    fn ns_warn(&mut self, code: i32, msg: String) {
        self.err_full(FROM_NAMESPACE, code, ERR_WARNING, msg);
    }
    pub fn valid_err(&mut self, code: i32, msg: String) {
        self.st.valid = false;
        self.err_full(FROM_VALID, code, ERR_ERROR, msg);
    }

    // ------------------------------------------------------------------
    // Names

    fn parse_name_generic(&mut self, nc: bool) -> Option<String> {
        let start = self.pos();
        let (c, l) = self.cur_char_raw();
        if l == 0 || !is_name_start_char(c) || (nc && c == b':' as u32) {
            return None;
        }
        self.skip(l);
        loop {
            let (c, l) = self.cur_char_raw();
            if l == 0 || !is_name_char(c) || (nc && c == b':' as u32) {
                break;
            }
            self.skip(l);
        }
        let end = self.pos();
        if end - start > self.max_name_length() {
            self.fatal(
                XML_ERR_NAME_TOO_LONG,
                Some(if nc { "NCName" } else { "Name" }),
            );
            return None;
        }
        Some(String::from_utf8_lossy(&self.inp().data[start..end]).into_owned())
    }

    /// XML_MAX_NAME_LENGTH, or XML_MAX_TEXT_LENGTH with XML_PARSE_HUGE.
    fn max_name_length(&self) -> usize {
        if self.st.options & XML_PARSE_HUGE != 0 {
            10_000_000
        } else {
            50_000
        }
    }

    fn parse_name(&mut self) -> Option<String> {
        self.parse_name_generic(false)
    }

    fn parse_ncname(&mut self) -> Option<String> {
        self.parse_name_generic(true)
    }

    fn parse_nmtoken(&mut self) -> Option<String> {
        let start = self.pos();
        loop {
            let (c, l) = self.cur_char_raw();
            if l == 0 || !is_name_char(c) {
                break;
            }
            self.skip(l);
        }
        let end = self.pos();
        if end == start {
            None
        } else {
            Some(String::from_utf8_lossy(&self.inp().data[start..end]).into_owned())
        }
    }

    /// xmlParseQNameHashed: returns (prefix, localname)
    fn parse_qname(&mut self) -> Option<(Option<String>, String)> {
        let start = self.pos();
        let mut is_ncname = false;
        let mut l = self.parse_ncname();
        let mut p = None;
        if l.is_some() {
            is_ncname = true;
            if self.cur() == b':' {
                self.skip(1);
                p = l.take();
                l = self.parse_ncname();
            }
        }
        if l.is_none() || self.cur() == b':' {
            if !is_ncname && self.cur() != b':' {
                return None;
            }
            self.parse_nmtoken();
            let end = self.pos();
            let name = String::from_utf8_lossy(&self.inp().data[start..end]).into_owned();
            self.ns_err(
                XML_NS_ERR_QNAME,
                format!("Failed to parse QName '{}'\n", name),
            );
            return Some((None, name));
        }
        Some((p, l.unwrap()))
    }

    // ------------------------------------------------------------------
    // Entry points

    /// Detect BOM / encoding and convert the main input to UTF-8.
    fn detect_encoding(&mut self) {
        let data = self.inp().data.clone();
        let d = &data[..];
        if d.len() >= 3 && d[0] == 0xEF && d[1] == 0xBB && d[2] == 0xBF {
            self.skip(3);
            self.st.input_encoding = Some("UTF-8".into());
            return;
        }
        if d.len() >= 2 && ((d[0] == 0xFF && d[1] == 0xFE) || (d[0] == 0xFE && d[1] == 0xFF)) {
            let enc = if d[0] == 0xFF { "UTF-16LE" } else { "UTF-16BE" };
            let conv = decode_to_utf8(&d[2..], enc).unwrap_or_default();
            self.inp_mut().data = Rc::new(conv);
            self.inp_mut().pos = 0;
            self.st.input_encoding = Some("UTF-16".into());
            self.conv_enc = Some(enc.into());
            return;
        }
        if d.len() >= 4 && d[0] == b'<' && d[1] == 0 && d[2] == b'?' && d[3] == 0 {
            let conv = decode_to_utf8(d, "UTF-16LE").unwrap_or_default();
            self.inp_mut().data = Rc::new(conv);
            self.st.input_encoding = Some("UTF-16LE".into());
            self.conv_enc = Some("UTF-16LE".into());
            return;
        }
        if d.len() >= 4 && d[0] == 0 && d[1] == b'<' && d[2] == 0 && d[3] == b'?' {
            let conv = decode_to_utf8(d, "UTF-16BE").unwrap_or_default();
            self.inp_mut().data = Rc::new(conv);
            self.st.input_encoding = Some("UTF-16BE".into());
            self.conv_enc = Some("UTF-16BE".into());
        }
    }

    /// Switch the remaining input to a declared encoding.
    fn set_declared_encoding(&mut self, enc: &str) {
        self.st.encoding = Some(enc.to_string());
        if self.st.input_encoding.is_some() {
            // BOM-detected encoding wins (UTF-16 documents declare UTF-16)
            return;
        }
        let up = enc.to_ascii_uppercase();
        if up == "UTF-8" || up == "UTF8" {
            return;
        }
        if up == "UTF-16" || up == "UTF16" || up == "UTF-16LE" || up == "UTF-16BE" {
            // Document labelled UTF-16 but has UTF-8 content
            self.warning(
                113,
                "Document labelled UTF-16 but has UTF-8 content\n".to_string(),
            );
            return;
        }
        match decode_to_utf8(b"", enc) {
            Some(_) => {
                let pos = self.pos();
                let inp = self.inp_mut();
                let head = inp.data[..pos].to_vec();
                let rest = decode_to_utf8(&inp.data[pos..], enc).unwrap_or_default();
                let mut all = head;
                all.extend_from_slice(&rest);
                inp.data = Rc::new(all);
                self.st.input_encoding = Some(enc.to_string());
                self.conv_enc = Some(enc.to_string());
            }
            None => {
                self.fatal(XML_ERR_UNSUPPORTED_ENCODING, Some(enc));
            }
        }
    }

    pub fn parse_document(&mut self) -> bool {
        self.detect_encoding();
        if self.at_end() {
            self.fatal(XML_ERR_DOCUMENT_EMPTY, None);
            return false;
        }
        if self.cmp(b"<?xml") && is_blank(self.nxt(5)) {
            self.parse_xml_decl();
            self.skip_blanks();
        } else {
            self.st.version = Some("1.0".into());
        }
        if self.sax_ok() {
            let v = self.st.version.clone().unwrap_or_else(|| "1.0".into());
            let e = self.st.encoding.clone();
            let sa = self.st.standalone;
            self.sink.start_document(&v, e.as_deref(), sa);
        }
        self.parse_misc();
        if self.cmp(b"<!DOCTYPE") {
            self.st.in_subset = 1;
            self.parse_doctype_decl();
            if self.cur() == b'[' {
                self.parse_internal_subset();
            } else if self.cur() == b'>' {
                self.skip(1);
            }
            self.st.in_subset = 2;
            if self.sax_ok() {
                self.load_external_subset();
            }
            self.st.in_subset = 0;
            self.clean_special_attr();
            self.parse_misc();
        }
        if self.cur() != b'<' {
            if self.st.well_formed {
                self.fatal_msg(
                    XML_ERR_DOCUMENT_EMPTY,
                    "Start tag expected, '<' not found\n".into(),
                );
            }
        } else {
            self.parse_element();
            self.parse_misc();
            // 2.13: only reported while still well-formed
            if !self.at_end() && !self.stopped() && self.st.well_formed {
                self.fatal(XML_ERR_DOCUMENT_END, None);
            }
        }
        let enc = self.st.encoding.clone();
        self.sink.end_document_p(&mut self.st, enc.as_deref());
        if !self.st.well_formed {
            self.st.valid = false;
            return false;
        }
        true
    }

    /// Parse a well-balanced chunk of content (xmlParseInNodeContext-like).
    /// Returns well-formedness.
    pub fn parse_content_chunk(&mut self, ns_in_scope: &[(Option<String>, String)]) -> bool {
        for (p, u) in ns_in_scope {
            self.ns.push(NsEntry {
                prefix: p.clone(),
                uri: u.clone(),
                elem: 0,
            });
        }
        self.elem_id = 1;
        self.names.push(NameEntry {
            name: "#root".into(),
            prefix: None,
            uri: None,
            line: 0,
            ns_nr: 0,
            has_node: false,
        });
        self.space.push(-1);
        self.parse_content_internal();
        if !self.at_end() && !self.stopped() {
            self.fatal(XML_ERR_NOT_WELL_BALANCED, None);
        }
        self.st.well_formed
    }

    /// Register an entity known before parsing (e.g. document DTD when
    /// parsing a fragment in context).
    pub fn add_known_entity(&mut self, e: PEntity) {
        if matches!(
            e.kind,
            EntKind::InternalParameter | EntKind::ExternalParameter
        ) {
            self.pentities.insert(e.name.clone(), e);
        } else {
            self.entities.insert(e.name.clone(), e);
        }
    }

    pub fn set_has_dtd(&mut self) {
        self.st.has_external_subset = true;
    }

    // ------------------------------------------------------------------
    // XML declaration

    fn parse_xml_decl(&mut self) {
        self.st.standalone = -2;
        self.skip(5);
        if !is_blank(self.cur()) {
            self.fatal_msg(
                XML_ERR_SPACE_REQUIRED,
                "Blank needed after '<?xml'\n".into(),
            );
        }
        self.skip_blanks();
        let version = self.parse_version_info();
        match version {
            None => self.fatal(XML_ERR_VERSION_MISSING, None),
            Some(v) => {
                if v != "1.0" {
                    if self.st.options & XML_PARSE_OLD10 != 0 {
                        self.fatal_msg(
                            XML_ERR_UNKNOWN_VERSION,
                            format!("Unsupported version '{}'\n", v),
                        );
                    } else if v.starts_with("1.") {
                        self.warning(
                            XML_WAR_UNKNOWN_VERSION,
                            format!("Unsupported version '{}'\n", v),
                        );
                    } else {
                        self.fatal_msg(
                            XML_ERR_UNKNOWN_VERSION,
                            format!("Unsupported version '{}'\n", v),
                        );
                    }
                }
                self.st.version = Some(v);
            }
        }
        if !is_blank(self.cur()) {
            if self.cur() == b'?' && self.nxt(1) == b'>' {
                self.skip(2);
                return;
            }
            self.fatal_msg(XML_ERR_SPACE_REQUIRED, "Blank needed here\n".into());
        }
        self.parse_encoding_decl();
        if self.st.encoding.is_some() && !is_blank(self.cur()) {
            if self.cur() == b'?' && self.nxt(1) == b'>' {
                self.skip(2);
                return;
            }
            self.fatal_msg(XML_ERR_SPACE_REQUIRED, "Blank needed here\n".into());
        }
        self.skip_blanks();
        self.st.standalone = self.parse_sd_decl();
        self.skip_blanks();
        if self.cur() == b'?' && self.nxt(1) == b'>' {
            self.skip(2);
        } else if self.cur() == b'>' {
            self.fatal(XML_ERR_XMLDECL_NOT_FINISHED, None);
            self.skip(1);
        } else {
            self.fatal(XML_ERR_XMLDECL_NOT_FINISHED, None);
            while !self.stopped() && !self.at_end() {
                let c = self.cur();
                self.skip(1);
                if c == b'>' {
                    break;
                }
            }
        }
    }

    fn parse_version_info(&mut self) -> Option<String> {
        if !self.cmp(b"version") {
            return None;
        }
        self.skip(7);
        self.skip_blanks();
        if self.cur() != b'=' {
            self.fatal(XML_ERR_EQUAL_REQUIRED, None);
            return None;
        }
        self.skip(1);
        self.skip_blanks();
        let q = self.cur();
        if q == b'"' || q == b'\'' {
            self.skip(1);
            let v = self.parse_version_num();
            if self.cur() != q {
                self.fatal(XML_ERR_STRING_NOT_CLOSED, None);
            } else {
                self.skip(1);
            }
            v
        } else {
            self.fatal(XML_ERR_STRING_NOT_STARTED, None);
            None
        }
    }

    fn parse_version_num(&mut self) -> Option<String> {
        let start = self.pos();
        if !self.cur().is_ascii_digit() {
            return None;
        }
        self.skip(1);
        if self.cur() != b'.' {
            return None;
        }
        self.skip(1);
        while self.cur().is_ascii_digit() {
            self.skip(1);
        }
        let end = self.pos();
        Some(String::from_utf8_lossy(&self.inp().data[start..end]).into_owned())
    }

    fn parse_encoding_decl(&mut self) {
        self.skip_blanks();
        if !self.cmp(b"encoding") {
            return;
        }
        self.skip(8);
        self.skip_blanks();
        if self.cur() != b'=' {
            self.fatal(XML_ERR_EQUAL_REQUIRED, None);
            return;
        }
        self.skip(1);
        self.skip_blanks();
        let q = self.cur();
        if q == b'"' || q == b'\'' {
            self.skip(1);
            let enc = self.parse_enc_name();
            if self.cur() != q {
                self.fatal(XML_ERR_STRING_NOT_CLOSED, None);
                return;
            }
            self.skip(1);
            if let Some(e) = enc {
                self.set_declared_encoding(&e);
            }
        } else {
            self.fatal(XML_ERR_STRING_NOT_STARTED, None);
        }
    }

    fn parse_enc_name(&mut self) -> Option<String> {
        let start = self.pos();
        let c = self.cur();
        if !c.is_ascii_alphabetic() {
            self.fatal(XML_ERR_ENCODING_NAME, None);
            return None;
        }
        self.skip(1);
        while {
            let c = self.cur();
            c.is_ascii_alphanumeric() || c == b'.' || c == b'_' || c == b'-'
        } {
            self.skip(1);
        }
        let end = self.pos();
        Some(String::from_utf8_lossy(&self.inp().data[start..end]).into_owned())
    }

    fn parse_sd_decl(&mut self) -> i32 {
        let mut standalone = -2;
        self.skip_blanks();
        if self.cmp(b"standalone") {
            self.skip(10);
            self.skip_blanks();
            if self.cur() != b'=' {
                self.fatal(XML_ERR_EQUAL_REQUIRED, None);
                return standalone;
            }
            self.skip(1);
            self.skip_blanks();
            let q = self.cur();
            if q == b'"' || q == b'\'' {
                self.skip(1);
                if self.cmp(b"no") {
                    standalone = 0;
                    self.skip(2);
                } else if self.cmp(b"yes") {
                    standalone = 1;
                    self.skip(3);
                } else {
                    self.fatal(XML_ERR_STANDALONE_VALUE, None);
                }
                if self.cur() != q {
                    self.fatal(XML_ERR_STRING_NOT_CLOSED, None);
                } else {
                    self.skip(1);
                }
            } else {
                self.fatal(XML_ERR_STRING_NOT_STARTED, None);
            }
        }
        standalone
    }

    fn parse_misc(&mut self) {
        while !self.stopped() {
            self.skip_blanks();
            if self.cur() == b'<' && self.nxt(1) == b'?' {
                self.parse_pi();
            } else if self.cmp(b"<!--") {
                self.parse_comment();
            } else {
                break;
            }
        }
    }

    // ------------------------------------------------------------------
    // Comments and PIs

    fn parse_comment(&mut self) {
        if !self.cmp(b"<!--") {
            return;
        }
        self.skip(4);
        let mut buf: Vec<u8> = Vec::new();
        // q, r, cur
        let (mut q, ql) = self.cur_char();
        if ql == 0 {
            self.fatal_msg(
                XML_ERR_COMMENT_NOT_FINISHED,
                "Comment not terminated\n".into(),
            );
            return;
        }
        if !is_char(q) {
            self.fatal_msg(
                XML_ERR_INVALID_CHAR,
                format!("xmlParseComment: invalid xmlChar value {}\n", q),
            );
            return;
        }
        self.skip(ql);
        let (mut r, rl) = self.cur_char();
        if rl == 0 {
            self.fatal_msg(
                XML_ERR_COMMENT_NOT_FINISHED,
                "Comment not terminated\n".into(),
            );
            return;
        }
        if !is_char(r) {
            self.fatal_msg(
                XML_ERR_INVALID_CHAR,
                format!("xmlParseComment: invalid xmlChar value {}\n", r),
            );
            return;
        }
        // special-case "<!---->" and "<!--->" handled by the loop
        if q == b'-' as u32 && r == b'-' as u32 {
            // "<!---" followed by '-'... check for "-->" immediately: "<!---->"
            self.skip(rl);
            let (c, _l) = self.cur_char();
            if c == b'>' as u32 {
                self.skip(1);
                if self.sax_ok() {
                    self.sink.comment(&mut self.st, "");
                }
                return;
            }
            self.fatal(XML_ERR_HYPHEN_IN_COMMENT, None);
            // continue as complex
            let (mut cur, mut l) = self.cur_char();
            loop {
                if l == 0 || !is_char(cur) {
                    break;
                }
                if cur == b'>' as u32 && r == b'-' as u32 && q == b'-' as u32 {
                    break;
                }
                if r == b'-' as u32 && q == b'-' as u32 {
                    self.fatal(XML_ERR_HYPHEN_IN_COMMENT, None);
                }
                push_char(&mut buf, q);
                q = r;
                r = cur;
                self.skip(l);
                let x = self.cur_char();
                cur = x.0;
                l = x.1;
            }
            return self.finish_comment(buf, cur, l);
        }
        self.skip(rl);
        let (mut cur, mut l) = self.cur_char();
        if l == 0 {
            self.fatal_msg(
                XML_ERR_COMMENT_NOT_FINISHED,
                "Comment not terminated\n".into(),
            );
            return;
        }
        loop {
            if l == 0 || !is_char(cur) {
                break;
            }
            if cur == b'>' as u32 && r == b'-' as u32 && q == b'-' as u32 {
                break;
            }
            if r == b'-' as u32 && q == b'-' as u32 {
                let partial = String::from_utf8_lossy(&buf).into_owned();
                let p50: String = partial.chars().take(50).collect();
                self.fatal_msg(
                    XML_ERR_HYPHEN_IN_COMMENT,
                    format!("Double hyphen within comment: <!--{}\n", p50),
                );
            }
            push_char(&mut buf, q);
            q = r;
            r = cur;
            self.skip(l);
            let x = self.cur_char();
            cur = x.0;
            l = x.1;
        }
        self.finish_comment(buf, cur, l);
    }

    fn finish_comment(&mut self, buf: Vec<u8>, cur: u32, l: usize) {
        let s = String::from_utf8_lossy(&buf).into_owned();
        if l == 0 {
            let p50: String = s.chars().take(50).collect();
            self.fatal_msg(
                XML_ERR_COMMENT_NOT_FINISHED,
                format!("Comment not terminated \n<!--{}\n", p50),
            );
        } else if !is_char(cur) {
            self.fatal_msg(
                XML_ERR_INVALID_CHAR,
                format!("xmlParseComment: invalid xmlChar value {}\n", cur),
            );
        } else {
            self.skip(1);
            if self.sax_ok() {
                if self.st.in_subset == 0 || true {
                    self.sink.comment(&mut self.st, &s);
                }
            }
        }
    }

    fn parse_pi_target(&mut self) -> Option<String> {
        let name = self.parse_name()?;
        let lower = name.to_ascii_lowercase();
        if lower.starts_with("xml") {
            if name == "xml" {
                self.fatal_msg(
                    XML_ERR_RESERVED_XML_NAME,
                    "XML declaration allowed only at the start of the document\n".into(),
                );
                return Some(name);
            } else if name.len() == 3 {
                self.fatal(XML_ERR_RESERVED_XML_NAME, None);
                return Some(name);
            }
            if name != "xml-stylesheet" && name != "xml-model" {
                self.warning(
                    XML_ERR_RESERVED_XML_NAME,
                    "xmlParsePITarget: invalid name prefix 'xml'\n".into(),
                );
            }
        }
        if name.contains(':') {
            self.ns_err(
                XML_NS_ERR_COLON,
                format!("colons are forbidden from PI names '{}'\n", name),
            );
        }
        Some(name)
    }

    fn parse_pi(&mut self) {
        if !(self.cur() == b'<' && self.nxt(1) == b'?') {
            return;
        }
        self.skip(2);
        let target = match self.parse_pi_target() {
            Some(t) => t,
            None => {
                self.fatal(XML_ERR_PI_NOT_STARTED, None);
                return;
            }
        };
        if self.cur() == b'?' && self.nxt(1) == b'>' {
            self.skip(2);
            if self.sax_ok() {
                self.sink.pi(&mut self.st, &target, None);
            }
            return;
        }
        if self.skip_blanks() == 0 {
            self.fatal_msg(
                XML_ERR_SPACE_REQUIRED,
                format!("ParsePI: PI {} space expected\n", target),
            );
        }
        let mut buf = Vec::new();
        let (mut cur, mut l) = self.cur_char();
        while l != 0 && is_char(cur) && !(cur == b'?' as u32 && self.nxt(1) == b'>') {
            push_char(&mut buf, cur);
            self.skip(l);
            let x = self.cur_char();
            cur = x.0;
            l = x.1;
        }
        if cur != b'?' as u32 || l == 0 {
            self.fatal_msg(
                XML_ERR_PI_NOT_FINISHED,
                format!("ParsePI: PI {} never end ...\n", target),
            );
        } else {
            self.skip(2);
            if self.sax_ok() {
                let s = String::from_utf8_lossy(&buf).into_owned();
                self.sink.pi(&mut self.st, &target, Some(&s));
            }
        }
    }

    // ------------------------------------------------------------------
    // DTD

    fn parse_doctype_decl(&mut self) {
        self.skip(9);
        if self.skip_blanks() == 0 {
            self.fatal_msg(
                XML_ERR_SPACE_REQUIRED,
                "Space required after 'DOCTYPE'\n".into(),
            );
        }
        let name = self.parse_name();
        if name.is_none() {
            self.fatal_msg(
                XML_ERR_NAME_REQUIRED,
                "xmlParseDocTypeDecl : no DOCTYPE name !\n".into(),
            );
        }
        self.int_sub_name = name.clone();
        self.skip_blanks();
        let (uri, public) = self.parse_external_id(true);
        if uri.is_some() || public.is_some() {
            self.st.has_external_subset = true;
        }
        self.ext_sub_uri = uri.clone();
        self.ext_sub_system = public.clone();
        self.skip_blanks();
        if self.sax_ok() {
            self.sink
                .internal_subset(name.as_deref(), public.as_deref(), uri.as_deref());
        }
        if self.cur() != b'[' && self.cur() != b'>' {
            self.fatal(XML_ERR_DOCTYPE_NOT_FINISHED, None);
        }
    }

    /// Returns (system URI, public id)
    fn parse_external_id(&mut self, strict: bool) -> (Option<String>, Option<String>) {
        let mut public = None;
        let mut uri = None;
        if self.cmp(b"SYSTEM") {
            self.skip(6);
            if self.skip_blanks() == 0 {
                self.fatal_msg(
                    XML_ERR_SPACE_REQUIRED,
                    "Space required after 'SYSTEM'\n".into(),
                );
            }
            uri = self.parse_system_literal();
            if uri.is_none() {
                self.fatal(XML_ERR_URI_REQUIRED, None);
            }
        } else if self.cmp(b"PUBLIC") {
            self.skip(6);
            if self.skip_blanks() == 0 {
                self.fatal_msg(
                    XML_ERR_SPACE_REQUIRED,
                    "Space required after 'PUBLIC'\n".into(),
                );
            }
            public = self.parse_pubid_literal();
            if public.is_none() {
                self.fatal(XML_ERR_PUBID_REQUIRED, None);
            }
            if strict {
                if self.skip_blanks() == 0 {
                    self.fatal_msg(
                        XML_ERR_SPACE_REQUIRED,
                        "Space required after the Public Identifier\n".into(),
                    );
                }
            } else {
                if self.skip_blanks() == 0 {
                    return (None, public);
                }
                if self.cur() != b'\'' && self.cur() != b'"' {
                    return (None, public);
                }
            }
            uri = self.parse_system_literal();
            if uri.is_none() {
                self.fatal(XML_ERR_URI_REQUIRED, None);
            }
        }
        (uri, public)
    }

    fn parse_system_literal(&mut self) -> Option<String> {
        let q = self.cur();
        if q != b'"' && q != b'\'' {
            self.fatal(XML_ERR_LITERAL_NOT_STARTED, None);
            return None;
        }
        self.skip(1);
        let mut buf = Vec::new();
        loop {
            let (c, l) = self.cur_char();
            if l == 0 || !is_char(c) || c == q as u32 {
                break;
            }
            push_char(&mut buf, c);
            self.skip(l);
        }
        if self.cur() != q {
            self.fatal(XML_ERR_LITERAL_NOT_FINISHED, None);
            return None;
        }
        self.skip(1);
        Some(String::from_utf8_lossy(&buf).into_owned())
    }

    fn parse_pubid_literal(&mut self) -> Option<String> {
        let q = self.cur();
        if q != b'"' && q != b'\'' {
            self.fatal(XML_ERR_LITERAL_NOT_STARTED, None);
            return None;
        }
        self.skip(1);
        let start = self.pos();
        loop {
            let c = self.cur();
            let ok = c == 0x20
                || c == 0xD
                || c == 0xA
                || c.is_ascii_alphanumeric()
                || b"-'()+,./:=?;!*#@$_%".contains(&c);
            if !ok || c == q || self.at_end() {
                break;
            }
            self.skip(1);
        }
        let end = self.pos();
        if self.cur() != q {
            self.fatal(XML_ERR_LITERAL_NOT_FINISHED, None);
            return None;
        }
        self.skip(1);
        Some(String::from_utf8_lossy(&self.inp().data[start..end]).into_owned())
    }

    fn parse_internal_subset(&mut self) {
        if self.cur() == b'[' {
            let old_nr = self.st.inputs.len();
            self.skip(1);
            self.skip_blanks();
            loop {
                if self.stopped() {
                    return;
                } else if self.at_end() {
                    if self.st.inputs.len() <= old_nr {
                        self.fatal(XML_ERR_INT_SUBSET_NOT_FINISHED, None);
                        return;
                    }
                    self.st.inputs.pop();
                } else if self.cur() == b']' && self.st.inputs.len() <= old_nr {
                    self.skip(1);
                    self.skip_blanks();
                    break;
                } else if self.cur() == b'<' && (self.nxt(1) == b'!' || self.nxt(1) == b'?') {
                    self.parse_markup_decl();
                } else if self.cur() == b'%' {
                    self.parse_pe_reference(true);
                } else {
                    self.fatal(XML_ERR_INT_SUBSET_NOT_FINISHED, None);
                    while self.st.inputs.len() > old_nr {
                        self.st.inputs.pop();
                    }
                    return;
                }
                self.skip_blanks();
            }
        }
        if self.cur() != b'>' {
            self.fatal(XML_ERR_DOCTYPE_NOT_FINISHED, None);
            return;
        }
        self.skip(1);
    }

    fn parse_markup_decl(&mut self) {
        if self.cur() != b'<' {
            return;
        }
        if self.nxt(1) == b'!' {
            match self.nxt(2) {
                b'E' => {
                    if self.nxt(3) == b'L' {
                        self.parse_element_decl();
                    } else if self.nxt(3) == b'N' {
                        self.parse_entity_decl();
                    } else {
                        self.skip(2);
                    }
                }
                b'A' => self.parse_attribute_list_decl(),
                b'N' => self.parse_notation_decl(),
                b'-' => self.parse_comment(),
                _ => {
                    let code = if self.st.in_subset == 2 {
                        60
                    } else {
                        XML_ERR_INT_SUBSET_NOT_FINISHED
                    };
                    self.fatal(code, None);
                    self.skip(2);
                }
            }
        } else if self.nxt(1) == b'?' {
            self.parse_pi();
        }
    }

    fn parse_pe_reference(&mut self, _markup_decl: bool) {
        if self.cur() != b'%' {
            return;
        }
        self.skip(1);
        let name = match self.parse_name() {
            Some(n) => n,
            None => {
                self.fatal_msg(XML_ERR_PEREF_NO_NAME, "PEReference: no name\n".into());
                return;
            }
        };
        if self.cur() != b';' {
            self.fatal(XML_ERR_PEREF_SEMICOL_MISSING, None);
            return;
        }
        self.skip(1);
        self.st.has_perefs = true;
        let ent = self.pentities.get(&name).cloned();
        match ent {
            None => {
                if self.st.standalone == 1 || (!self.st.has_external_subset && !self.st.has_perefs)
                {
                    self.fatal_msg(
                        XML_ERR_UNDECLARED_ENTITY,
                        format!("PEReference: %{}; not found\n", name),
                    );
                } else {
                    self.warning(
                        XML_WAR_UNDECLARED_ENTITY,
                        format!("PEReference: %{}; not found\n", name),
                    );
                }
            }
            Some(e) => {
                if e.kind == EntKind::InternalParameter {
                    // push the replacement text, surrounded by spaces
                    let mut data = Vec::with_capacity(e.content.len() + 2);
                    data.push(b' ');
                    data.extend_from_slice(e.content.as_bytes());
                    data.push(b' ');
                    let mut inp = Input::new(Rc::new(data), None);
                    inp.entity = Some(name);
                    self.st.inputs.push(inp);
                } else {
                    self.push_external_pe(&e);
                }
            }
        }
    }

    /// Entity value literal with PE references and char refs expanded.
    fn parse_entity_value(&mut self) -> Option<(String, String)> {
        let q = self.cur();
        if q != b'"' && q != b'\'' {
            self.fatal(XML_ERR_ENTITY_NOT_STARTED, None);
            return None;
        }
        self.skip(1);
        let start = self.pos();
        let base_depth = self.st.inputs.len();
        let mut raw = Vec::new();
        loop {
            if self.at_end() {
                break;
            }
            let (c, l) = self.cur_char();
            if l == 0 || c == q as u32 {
                break;
            }
            if !is_char(c) {
                break;
            }
            push_char(&mut raw, c);
            self.skip(l);
        }
        if self.cur() != q || self.st.inputs.len() != base_depth {
            self.fatal(XML_ERR_ENTITY_NOT_FINISHED, None);
            return None;
        }
        let _ = start;
        self.skip(1);
        let orig = String::from_utf8_lossy(&raw).into_owned();
        // expand PE references and char refs
        let mut out = Vec::new();
        let b = raw;
        let mut i = 0;
        while i < b.len() {
            let c = b[i];
            if c == b'%' {
                let mut j = i + 1;
                while j < b.len() && b[j] != b';' && !is_blank(b[j]) {
                    j += 1;
                }
                if j < b.len() && b[j] == b';' {
                    let name = String::from_utf8_lossy(&b[i + 1..j]).into_owned();
                    if self.st.in_subset == 1 && self.st.inputs.len() == 1 {
                        self.fatal(88, None);
                        return None;
                    }
                    if let Some(e) = self.pentities.get(&name).cloned() {
                        if e.kind == EntKind::ExternalParameter && !e.parsed {
                            if self.st.options & XML_PARSE_NO_XXE == 0
                                && (self.st.replace_entities
                                    || self.st.options & XML_PARSE_DTDVALID != 0)
                            {
                                let content = self
                                    .load_entity_content(&e)
                                    .map(|c| c.0)
                                    .unwrap_or_default();
                                if let Some(pe) = self.pentities.get_mut(&name) {
                                    pe.content = content.clone();
                                    pe.parsed = true;
                                }
                                out.extend_from_slice(content.as_bytes());
                            } else {
                                self.warning(
                                    XML_ERR_ENTITY_PROCESSING,
                                    format!(
                                        "not validating will not read content for PE entity {}\n",
                                        name
                                    ),
                                );
                            }
                        } else {
                            out.extend_from_slice(e.content.as_bytes());
                        }
                    }
                    i = j + 1;
                    continue;
                }
                out.push(c);
                i += 1;
            } else if c == b'&' && i + 1 < b.len() && b[i + 1] == b'#' {
                let mut j = i + 2;
                let hex = j < b.len() && b[j] == b'x';
                if hex {
                    j += 1;
                }
                let s = j;
                while j < b.len() && b[j] != b';' {
                    j += 1;
                }
                let digits = String::from_utf8_lossy(&b[s..j.min(b.len())]).into_owned();
                let v = if hex {
                    u32::from_str_radix(&digits, 16).ok()
                } else {
                    digits.parse::<u32>().ok()
                };
                match v {
                    Some(v) if is_char(v) && j < b.len() => {
                        push_char(&mut out, v);
                        i = j + 1;
                    }
                    _ => {
                        let code = if hex {
                            XML_ERR_INVALID_HEX_CHARREF
                        } else {
                            XML_ERR_INVALID_DEC_CHARREF
                        };
                        self.fatal(code, None);
                        return None;
                    }
                }
            } else if c == b'&' {
                // general entity reference: validate syntax, keep as-is
                let mut j = i + 1;
                while j < b.len() && b[j] != b';' && b[j] != b'&' && !is_blank(b[j]) {
                    j += 1;
                }
                if j >= b.len() || b[j] != b';' || j == i + 1 {
                    if j == i + 1 {
                        self.fatal_msg(
                            XML_ERR_NAME_REQUIRED,
                            "xmlParseStringEntityRef: no name\n".into(),
                        );
                    } else {
                        self.fatal(XML_ERR_ENTITYREF_SEMICOL_MISSING, None);
                    }
                    return None;
                }
                out.extend_from_slice(&b[i..=j]);
                i = j + 1;
            } else {
                out.push(c);
                i += 1;
            }
        }
        Some((String::from_utf8_lossy(&out).into_owned(), orig))
    }

    fn parse_entity_decl(&mut self) {
        if !self.cmp(b"<!ENTITY") {
            return;
        }
        self.skip(8);
        if self.skip_blanks_pe() == 0 {
            self.fatal_msg(
                XML_ERR_SPACE_REQUIRED,
                "Space required after '<!ENTITY'\n".into(),
            );
        }
        let mut is_param = false;
        if self.cur() == b'%' {
            self.skip(1);
            if self.skip_blanks_pe() == 0 {
                self.fatal_msg(XML_ERR_SPACE_REQUIRED, "Space required after '%'\n".into());
            }
            is_param = true;
        }
        let name = match self.parse_name() {
            Some(n) => n,
            None => {
                self.fatal_msg(
                    XML_ERR_NAME_REQUIRED,
                    "xmlParseEntityDecl: no name\n".into(),
                );
                return;
            }
        };
        if name.contains(':') {
            self.ns_err(
                XML_NS_ERR_COLON,
                format!("colons are forbidden from entities names '{}'\n", name),
            );
        }
        if self.skip_blanks_pe() == 0 {
            self.fatal_msg(
                XML_ERR_SPACE_REQUIRED,
                "Space required after the entity name\n".into(),
            );
        }
        let mut new_ent: Option<PEntity> = None;
        if is_param {
            if self.cur() == b'"' || self.cur() == b'\'' {
                if let Some((value, orig)) = self.parse_entity_value() {
                    new_ent = Some(PEntity {
                        name: name.clone(),
                        kind: EntKind::InternalParameter,
                        content: value,
                        orig: Some(orig),
                        public: None,
                        system: None,
                        notation: None,
                        parsed: false,
                        expanding: false,
                        uri: None,
                    });
                }
            } else {
                let (uri, literal) = self.parse_external_id(true);
                if uri.is_none() && literal.is_none() {
                    self.fatal(XML_ERR_VALUE_REQUIRED, None);
                }
                if let Some(u) = &uri {
                    if u.contains('#') {
                        self.fatal(XML_ERR_URI_FRAGMENT, None);
                    } else {
                        new_ent = Some(PEntity {
                            name: name.clone(),
                            kind: EntKind::ExternalParameter,
                            content: String::new(),
                            orig: None,
                            public: literal,
                            system: uri,
                            notation: None,
                            parsed: false,
                            expanding: false,
                            uri: None,
                        });
                    }
                }
            }
        } else if self.cur() == b'"' || self.cur() == b'\'' {
            if let Some((value, orig)) = self.parse_entity_value() {
                new_ent = Some(PEntity {
                    name: name.clone(),
                    kind: EntKind::InternalGeneral,
                    content: value,
                    orig: Some(orig),
                    public: None,
                    system: None,
                    notation: None,
                    parsed: false,
                    expanding: false,
                    uri: None,
                });
            }
        } else {
            let (uri, literal) = self.parse_external_id(true);
            if uri.is_none() && literal.is_none() {
                self.fatal(XML_ERR_VALUE_REQUIRED, None);
            }
            if let Some(u) = &uri {
                if u.contains('#') {
                    self.fatal(XML_ERR_URI_FRAGMENT, None);
                }
            }
            if self.cur() != b'>' && self.skip_blanks_pe() == 0 {
                self.fatal_msg(
                    XML_ERR_SPACE_REQUIRED,
                    "Space required before 'NDATA'\n".into(),
                );
            }
            if self.cmp(b"NDATA") {
                self.skip(5);
                if self.skip_blanks_pe() == 0 {
                    self.fatal_msg(
                        XML_ERR_SPACE_REQUIRED,
                        "Space required after 'NDATA'\n".into(),
                    );
                }
                let ndata = self.parse_name();
                new_ent = Some(PEntity {
                    name: name.clone(),
                    kind: EntKind::ExternalGeneralUnparsed,
                    content: String::new(),
                    orig: None,
                    public: literal,
                    system: uri,
                    notation: ndata,
                    parsed: false,
                    expanding: false,
                    uri: None,
                });
            } else {
                new_ent = Some(PEntity {
                    name: name.clone(),
                    kind: EntKind::ExternalGeneralParsed,
                    content: String::new(),
                    orig: None,
                    public: literal,
                    system: uri,
                    notation: None,
                    parsed: false,
                    expanding: false,
                    uri: None,
                });
            }
        }
        self.skip_blanks_pe();
        if self.cur() != b'>' {
            self.fatal_msg(
                XML_ERR_ENTITY_NOT_FINISHED,
                format!("xmlParseEntityDecl: entity {} not terminated\n", name),
            );
            // libxml2 stops here (xmlHaltParser in some versions)
        } else {
            self.skip(1);
        }
        if let Some(e) = new_ent.as_mut() {
            if let Some(sys) = e.system.clone() {
                e.uri = Some(self.resolve_uri(&sys, true));
            }
        }
        if let Some(e) = new_ent {
            if !self.sax_ok() {
                return;
            }
            // predefined entity redeclaration check
            if !is_param && matches!(e.name.as_str(), "lt" | "gt" | "amp" | "apos" | "quot") {
                let valid = match e.name.as_str() {
                    "lt" => e.content == "&#60;" || e.content == "&#x3C;" || e.content == "<",
                    "gt" => e.content == ">" || e.content == "&#62;" || e.content == "&#x3E;",
                    "amp" => e.content == "&#38;" || e.content == "&#x26;",
                    "apos" => e.content == "'" || e.content == "&#39;" || e.content == "&#x27;",
                    "quot" => e.content == "\"" || e.content == "&#34;" || e.content == "&#x22;",
                    _ => true,
                };
                if !valid && e.kind == EntKind::InternalGeneral {
                    self.fatal(XML_ERR_REDECL_PREDEF_ENTITY, None);
                }
                return;
            }
            let table = if is_param {
                &self.pentities
            } else {
                &self.entities
            };
            if table.contains_key(&e.name) {
                // libxml2 warns about redefinition when pedantic
                if self.st.options & XML_PARSE_PEDANTIC != 0 {
                    self.warning(
                        XML_WAR_ENTITY_REDEFINED,
                        format!(
                            "Entity({}) already defined in the internal subset\n",
                            e.name
                        ),
                    );
                }
                return;
            }
            match e.kind {
                EntKind::ExternalGeneralUnparsed => {
                    self.sink.unparsed_entity_decl_p(
                        &mut self.st,
                        &e.name,
                        e.public.as_deref(),
                        e.system.as_deref(),
                        e.notation.as_deref().unwrap_or(""),
                    );
                }
                _ => {
                    let content = if matches!(
                        e.kind,
                        EntKind::InternalGeneral | EntKind::InternalParameter
                    ) {
                        Some(e.content.as_str())
                    } else {
                        None
                    };
                    self.sink.entity_decl_p(
                        &mut self.st,
                        &e.name,
                        e.kind,
                        e.public.as_deref(),
                        e.system.as_deref(),
                        content,
                        e.orig.as_deref(),
                    );
                }
            }
            if is_param {
                self.pentities.insert(e.name.clone(), e);
            } else {
                self.entities.insert(e.name.clone(), e);
            }
        }
    }

    fn parse_notation_decl(&mut self) {
        if !self.cmp(b"<!NOTATION") {
            return;
        }
        self.skip(10);
        if self.skip_blanks_pe() == 0 {
            self.fatal_msg(
                XML_ERR_SPACE_REQUIRED,
                "Space required after '<!NOTATION'\n".into(),
            );
            return;
        }
        let name = match self.parse_name() {
            Some(n) => n,
            None => {
                self.fatal(XML_ERR_NOTATION_NOT_STARTED, None);
                return;
            }
        };
        if self.skip_blanks_pe() == 0 {
            self.fatal_msg(
                XML_ERR_SPACE_REQUIRED,
                "Space required after the NOTATION name'\n".into(),
            );
            return;
        }
        let (sys, public) = self.parse_external_id(false);
        self.skip_blanks_pe();
        if self.cur() == b'>' {
            self.skip(1);
            if self.sax_ok() {
                self.sink
                    .notation_decl_p(&mut self.st, &name, public.as_deref(), sys.as_deref());
            }
        } else {
            self.fatal(XML_ERR_NOTATION_NOT_FINISHED, None);
        }
    }

    fn parse_element_decl(&mut self) {
        if !self.cmp(b"<!ELEMENT") {
            return;
        }
        self.skip(9);
        if self.skip_blanks_pe() == 0 {
            self.fatal_msg(
                XML_ERR_SPACE_REQUIRED,
                "Space required after 'ELEMENT'\n".into(),
            );
            return;
        }
        let name = match self.parse_name() {
            Some(n) => n,
            None => {
                self.fatal_msg(
                    XML_ERR_NAME_REQUIRED,
                    "xmlParseElementDecl: no name for Element\n".into(),
                );
                return;
            }
        };
        if self.skip_blanks_pe() == 0 {
            self.fatal_msg(
                XML_ERR_SPACE_REQUIRED,
                "Space required after the element name\n".into(),
            );
        }
        let (etype, content);
        if self.cmp(b"EMPTY") {
            self.skip(5);
            etype = 1;
            content = None;
        } else if self.cmp(b"ANY") {
            self.skip(3);
            etype = 2;
            content = None;
        } else if self.cur() == b'(' {
            match self.parse_element_content_decl() {
                Some((t, c)) => {
                    etype = t;
                    content = Some(c);
                }
                None => return,
            }
        } else {
            if self.cur() == b'%' && self.st.in_subset == 1 {
                self.fatal_msg(
                    21,
                    "PEReference: forbidden within markup decl in internal subset\n".into(),
                );
            } else {
                self.fatal_msg(
                    XML_ERR_ELEMCONTENT_NOT_STARTED,
                    "xmlParseElementDecl: 'EMPTY', 'ANY' or '(' expected\n".into(),
                );
            }
            return;
        }
        self.skip_blanks_pe();
        if self.cur() != b'>' {
            self.fatal(XML_ERR_GT_REQUIRED, None);
            return;
        }
        self.skip(1);
        if self.sax_ok() {
            self.elem_decls.entry(name.clone()).or_insert(etype);
            let d = ElementDecl {
                name,
                etype,
                content,
            };
            self.sink.element_decl_p(&mut self.st, &d);
        }
    }

    /// xmlParseElementContentDecl, at '('. Returns (etype, model).
    fn parse_element_content_decl(&mut self) -> Option<(u8, ElemContent)> {
        self.skip(1);
        self.skip_blanks_pe();
        let mut ec = ElemContent::default();
        if self.cmp(b"#PCDATA") {
            let r = self.parse_mixed_content(&mut ec)?;
            ec.root = r;
            return Some((3, ec));
        }
        let r = self.parse_children_content(&mut ec, 0)?;
        ec.root = r;
        Some((4, ec))
    }

    /// xmlParseElementMixedContentDecl, at "#PCDATA".
    fn parse_mixed_content(&mut self, ec: &mut ElemContent) -> Option<usize> {
        self.skip(7);
        self.skip_blanks_pe();
        if self.cur() == b')' {
            self.skip(1);
            let r = ec.new_node(CT_PCDATA, None);
            if self.cur() == b'*' {
                ec.nodes[r].ocur = OC_MULT;
                self.skip(1);
            }
            return Some(r);
        }
        let mut ret = None;
        let mut cur = None;
        let mut elem: Option<String> = None;
        if self.cur() == b'(' || self.cur() == b'|' {
            let n = ec.new_node(CT_PCDATA, None);
            ret = Some(n);
            cur = Some(n);
        }
        while self.cur() == b'|' && !self.stopped() {
            self.skip(1);
            let n = ec.new_node(CT_OR, None);
            match &elem {
                None => {
                    if let Some(c) = cur {
                        ec.set_c1(n, c);
                    }
                    ret = Some(n);
                    cur = Some(n);
                }
                Some(e) => {
                    if let Some(c) = cur {
                        ec.set_c2(c, n);
                    }
                    let x = ec.new_node(CT_ELEMENT, Some(e));
                    ec.set_c1(n, x);
                    cur = Some(n);
                }
            }
            self.skip_blanks_pe();
            elem = self.parse_name();
            if elem.is_none() {
                self.fatal_msg(
                    XML_ERR_NAME_REQUIRED,
                    "xmlParseElementMixedContentDecl : Name expected\n".into(),
                );
                return None;
            }
            self.skip_blanks_pe();
        }
        if self.cur() == b')' && self.nxt(1) == b'*' {
            if let (Some(e), Some(c)) = (&elem, cur) {
                let x = ec.new_node(CT_ELEMENT, Some(e));
                ec.set_c2(c, x);
            }
            if let Some(r) = ret {
                ec.nodes[r].ocur = OC_MULT;
            }
            self.skip(2);
        } else {
            self.fatal(XML_ERR_MIXED_NOT_STARTED, None);
            return None;
        }
        ret
    }

    fn parse_occur_code(&mut self) -> u8 {
        match self.cur() {
            b'?' => {
                self.skip(1);
                OC_OPT
            }
            b'*' => {
                self.skip(1);
                OC_MULT
            }
            b'+' => {
                self.skip(1);
                OC_PLUS
            }
            _ => OC_ONCE,
        }
    }

    /// xmlParseElementChildrenContentDeclPriv, after the opening '('.
    fn parse_children_content(&mut self, ec: &mut ElemContent, depth: usize) -> Option<usize> {
        let max_depth = if self.st.options & XML_PARSE_HUGE != 0 {
            2048
        } else {
            256
        };
        if depth > max_depth {
            self.fatal_msg(
                XML_ERR_RESOURCE_LIMIT,
                format!(
                    "xmlParseElementChildrenContentDecl : depth {} too deep, use XML_PARSE_HUGE\n",
                    depth
                ),
            );
            return None;
        }
        self.skip_blanks_pe();
        let mut ret;
        let mut cur;
        let mut last: Option<usize> = None;
        let mut typ: u8 = 0;
        if self.cur() == b'(' {
            self.skip(1);
            self.skip_blanks_pe();
            let r = self.parse_children_content(ec, depth + 1)?;
            ret = r;
            cur = r;
            self.skip_blanks_pe();
        } else {
            let name = match self.parse_name() {
                Some(n) => n,
                None => {
                    self.fatal(XML_ERR_ELEMCONTENT_NOT_STARTED, None);
                    return None;
                }
            };
            let r = ec.new_node(CT_ELEMENT, Some(&name));
            ec.nodes[r].ocur = self.parse_occur_code();
            ret = r;
            cur = r;
        }
        self.skip_blanks_pe();
        while self.cur() != b')' && !self.stopped() {
            let c = self.cur();
            if c == b',' || c == b'|' {
                if typ == 0 {
                    typ = c;
                } else if typ != c {
                    self.fatal_msg(
                        XML_ERR_SEPARATOR_REQUIRED,
                        format!(
                            "xmlParseElementChildrenContentDecl : '{}' expected\n",
                            typ as char
                        ),
                    );
                    return None;
                }
                self.skip(1);
                let op = ec.new_node(if c == b',' { CT_SEQ } else { CT_OR }, None);
                match last {
                    None => {
                        ec.set_c1(op, ret);
                        ret = op;
                        cur = op;
                    }
                    Some(l) => {
                        ec.set_c2(cur, op);
                        ec.set_c1(op, l);
                        cur = op;
                    }
                }
            } else {
                self.fatal(XML_ERR_ELEMCONTENT_NOT_FINISHED, None);
                return None;
            }
            self.skip_blanks_pe();
            if self.cur() == b'(' {
                self.skip(1);
                self.skip_blanks_pe();
                last = Some(self.parse_children_content(ec, depth + 1)?);
                self.skip_blanks_pe();
            } else {
                let name = match self.parse_name() {
                    Some(n) => n,
                    None => {
                        self.fatal(XML_ERR_ELEMCONTENT_NOT_STARTED, None);
                        return None;
                    }
                };
                let l = ec.new_node(CT_ELEMENT, Some(&name));
                ec.nodes[l].ocur = self.parse_occur_code();
                last = Some(l);
            }
            self.skip_blanks_pe();
        }
        if let Some(l) = last {
            ec.set_c2(cur, l);
        }
        self.skip(1);
        match self.cur() {
            b'?' => {
                let o = ec.nodes[ret].ocur;
                ec.nodes[ret].ocur = if o == OC_PLUS || o == OC_MULT {
                    OC_MULT
                } else {
                    OC_OPT
                };
                self.skip(1);
            }
            b'*' => {
                ec.nodes[ret].ocur = OC_MULT;
                // (a | b* | c?)* == (a | b | c)*
                let mut c = Some(ret);
                while let Some(n) = c {
                    if ec.nodes[n].ctype != CT_OR {
                        break;
                    }
                    for k in [ec.nodes[n].c1, ec.nodes[n].c2].into_iter().flatten() {
                        if ec.nodes[k].ocur == OC_OPT || ec.nodes[k].ocur == OC_MULT {
                            ec.nodes[k].ocur = OC_ONCE;
                        }
                    }
                    c = ec.nodes[n].c2;
                }
                self.skip(1);
            }
            b'+' => {
                let o = ec.nodes[ret].ocur;
                ec.nodes[ret].ocur = if o == OC_OPT || o == OC_MULT {
                    OC_MULT
                } else {
                    OC_PLUS
                };
                // (a | b*)+ == (a | b)* -- note: libxml2 walks from the
                // last operator here, not from the top of the group
                let mut found = false;
                let mut c = Some(cur);
                while let Some(n) = c {
                    if ec.nodes[n].ctype != CT_OR {
                        break;
                    }
                    for k in [ec.nodes[n].c1, ec.nodes[n].c2].into_iter().flatten() {
                        if ec.nodes[k].ocur == OC_OPT || ec.nodes[k].ocur == OC_MULT {
                            ec.nodes[k].ocur = OC_ONCE;
                            found = true;
                        }
                    }
                    c = ec.nodes[n].c2;
                }
                if found {
                    ec.nodes[ret].ocur = OC_MULT;
                }
                self.skip(1);
            }
            _ => {}
        }
        Some(ret)
    }

    fn parse_attribute_list_decl(&mut self) {
        if !self.cmp(b"<!ATTLIST") {
            return;
        }
        self.skip(9);
        if self.skip_blanks_pe() == 0 {
            self.fatal_msg(
                XML_ERR_SPACE_REQUIRED,
                "Space required after '<!ATTLIST'\n".into(),
            );
        }
        let elem = match self.parse_name() {
            Some(n) => n,
            None => {
                self.fatal_msg(
                    XML_ERR_NAME_REQUIRED,
                    "ATTLIST: no name for Element\n".into(),
                );
                return;
            }
        };
        self.skip_blanks_pe();
        while self.cur() != b'>' && !self.stopped() && !self.at_end() {
            let attr = match self.parse_name() {
                Some(n) => n,
                None => {
                    self.fatal_msg(
                        XML_ERR_NAME_REQUIRED,
                        "ATTLIST: no name for Attribute\n".into(),
                    );
                    break;
                }
            };
            if self.skip_blanks_pe() == 0 {
                self.fatal_msg(
                    XML_ERR_SPACE_REQUIRED,
                    "Space required after the attribute name\n".into(),
                );
                break;
            }
            let (atype, tree) = match self.parse_attribute_type() {
                Some(x) => x,
                None => break,
            };
            if self.skip_blanks_pe() == 0 {
                self.fatal_msg(
                    XML_ERR_SPACE_REQUIRED,
                    "Space required after the attribute type\n".into(),
                );
                break;
            }
            let (def, mut default) = self.parse_default_decl();
            if def == 0 {
                break;
            }
            if atype != 1 {
                if let Some(d) = default.take() {
                    default = Some(normalize_attr_value(&d));
                }
            }
            if self.cur() != b'>' && self.skip_blanks_pe() == 0 {
                self.fatal_msg(
                    XML_ERR_SPACE_REQUIRED,
                    "Space required after the attribute default value\n".into(),
                );
                break;
            }
            if self.sax_ok() {
                let d = AttrDeclInfo {
                    elem: elem.clone(),
                    name: attr.clone(),
                    atype,
                    def,
                    default: default.clone(),
                    tree,
                };
                self.sink.attribute_decl_p(&mut self.st, &d);
            }
            if let Some(dv) = &default {
                if def != 2 && def != 3 {
                    self.add_def_attr(&elem, &attr, dv);
                }
            }
            if atype != 1 {
                self.atts_special
                    .entry((elem.clone(), attr.clone()))
                    .or_insert(atype);
            }
        }
        if self.cur() == b'>' {
            self.skip(1);
        }
    }

    fn add_def_attr(&mut self, elem: &str, attr: &str, value: &str) {
        let (prefix, local) = match attr.find(':') {
            Some(i) => (Some(attr[..i].to_string()), attr[i + 1..].to_string()),
            None => (None, attr.to_string()),
        };
        let list = self.atts_default.entry(elem.to_string()).or_default();
        if list.iter().any(|(p, l, _)| *p == prefix && *l == local) {
            return;
        }
        list.push((prefix, local, value.to_string()));
    }

    fn parse_attribute_type(&mut self) -> Option<(u8, Vec<String>)> {
        let t = if self.cmp(b"CDATA") {
            self.skip(5);
            1
        } else if self.cmp(b"IDREFS") {
            self.skip(6);
            4
        } else if self.cmp(b"IDREF") {
            self.skip(5);
            3
        } else if self.cmp(b"ID") {
            self.skip(2);
            2
        } else if self.cmp(b"ENTITY") {
            self.skip(6);
            5
        } else if self.cmp(b"ENTITIES") {
            self.skip(8);
            6
        } else if self.cmp(b"NMTOKENS") {
            self.skip(8);
            8
        } else if self.cmp(b"NMTOKEN") {
            self.skip(7);
            7
        } else if self.cmp(b"NOTATION") {
            self.skip(8);
            if self.skip_blanks_pe() == 0 {
                self.fatal_msg(
                    XML_ERR_SPACE_REQUIRED,
                    "Space required after 'NOTATION'\n".into(),
                );
                return None;
            }
            let tree = self.parse_enumeration(true)?;
            return Some((10, tree));
        } else {
            let tree = self.parse_enumeration(false)?;
            return Some((9, tree));
        };
        Some((t, Vec::new()))
    }

    fn parse_enumeration(&mut self, notation: bool) -> Option<Vec<String>> {
        if self.cur() != b'(' {
            self.fatal(
                if notation {
                    XML_ERR_NOTATION_NOT_STARTED
                } else {
                    XML_ERR_ATTLIST_NOT_STARTED
                },
                None,
            );
            return None;
        }
        let mut v = Vec::new();
        loop {
            self.skip(1);
            self.skip_blanks_pe();
            let n = if notation {
                self.parse_name()
            } else {
                self.parse_nmtoken()
            };
            match n {
                Some(n) => v.push(n),
                None => {
                    if notation {
                        self.fatal_msg(
                            XML_ERR_NAME_REQUIRED,
                            "Name expected in NOTATION declaration\n".into(),
                        );
                    } else {
                        self.fatal(XML_ERR_NMTOKEN_REQUIRED, None);
                    }
                    return None;
                }
            }
            self.skip_blanks_pe();
            if self.cur() != b'|' {
                break;
            }
        }
        if self.cur() != b')' {
            self.fatal(
                if notation {
                    XML_ERR_NOTATION_NOT_FINISHED
                } else {
                    XML_ERR_ATTLIST_NOT_FINISHED
                },
                None,
            );
            return None;
        }
        self.skip(1);
        Some(v)
    }

    /// Returns (def, default value); def 0 = error
    fn parse_default_decl(&mut self) -> (u8, Option<String>) {
        if self.cmp(b"#REQUIRED") {
            self.skip(9);
            return (2, None);
        }
        if self.cmp(b"#IMPLIED") {
            self.skip(8);
            return (3, None);
        }
        let mut val = 1;
        if self.cmp(b"#FIXED") {
            self.skip(6);
            val = 4;
            if self.skip_blanks_pe() == 0 {
                self.fatal_msg(
                    XML_ERR_SPACE_REQUIRED,
                    "Space required after '#FIXED'\n".into(),
                );
            }
        }
        match self.parse_att_value_internal(0, false) {
            Some(v) => (val, Some(v)),
            None => {
                self.fatal_msg(
                    XML_ERR_ATTRIBUTE_NOT_STARTED,
                    "Attribute default value declaration error\n".into(),
                );
                (0, None)
            }
        }
    }

    fn clean_special_attr(&mut self) {
        self.atts_special.retain(|_, t| *t != 1);
    }

    // ------------------------------------------------------------------
    // Namespace handling

    fn ns_lookup(&self, prefix: Option<&str>) -> Option<usize> {
        for i in (0..self.ns.len()).rev() {
            if self.ns[i].prefix.as_deref() == prefix {
                if i < self.ns_min {
                    return None;
                }
                return Some(i);
            }
        }
        None
    }

    /// Push a namespace; returns 1 if pushed, 0 if skipped.
    fn ns_push(&mut self, prefix: Option<&str>, uri: &str, def_attr: bool) -> usize {
        if prefix == Some("xml") {
            return 0;
        }
        if let Some(old) = self.ns_lookup(prefix) {
            if self.ns[old].elem == self.elem_id {
                if !def_attr {
                    match prefix {
                        None => self.fatal_msg(
                            XML_ERR_ATTRIBUTE_REDEFINED,
                            "Attribute xmlns redefined\n".into(),
                        ),
                        Some(p) => self.fatal_msg(
                            XML_ERR_ATTRIBUTE_REDEFINED,
                            format!("Attribute xmlns:{} redefined\n", p),
                        ),
                    }
                }
                return 0;
            }
            if self.st.options & XML_PARSE_NSCLEAN != 0 && self.ns[old].uri == uri {
                return 0;
            }
        }
        self.ns.push(NsEntry {
            prefix: prefix.map(|s| s.to_string()),
            uri: uri.to_string(),
            elem: self.elem_id,
        });
        1
    }

    fn ns_pop(&mut self, n: usize) {
        for _ in 0..n {
            self.ns.pop();
        }
    }

    // ------------------------------------------------------------------
    // Attribute values

    fn lookup_general_entity(&mut self, name: &str, in_attr: bool) -> Option<PEntity> {
        let predef = match name {
            "lt" => Some("<"),
            "gt" => Some(">"),
            "amp" => Some("&"),
            "apos" => Some("'"),
            "quot" => Some("\""),
            _ => None,
        };
        let predef = predef.map(|p| PEntity {
            name: name.into(),
            kind: EntKind::Predefined,
            content: p.into(),
            orig: None,
            public: None,
            system: None,
            notation: None,
            parsed: true,
            expanding: false,
            uri: None,
        });
        if self.st.sax_get_entity && self.st.in_subset == 0 {
            // XML_PARSE_OLDSAX: the SAX getEntity callback is asked first,
            // for predefined entities too.
            let ent = predef.or_else(|| self.entities.get(name).cloned());
            self.sink
                .get_entity(&mut self.st, name, ent.as_ref(), !in_attr);
            if let Some(e) = &ent {
                if e.kind == EntKind::Predefined {
                    return ent;
                }
            }
        } else if predef.is_some() {
            return predef;
        }
        let ent = self.entities.get(name).cloned();
        match ent {
            None => {
                self.handle_undeclared_entity(name);
                None
            }
            Some(e) if e.kind == EntKind::ExternalGeneralUnparsed => {
                self.fatal_msg(
                    XML_ERR_UNPARSED_ENTITY,
                    format!("Entity reference to unparsed entity {}\n", name),
                );
                None
            }
            Some(e) if e.kind == EntKind::ExternalGeneralParsed && in_attr => {
                self.fatal_msg(
                    XML_ERR_ENTITY_IS_EXTERNAL,
                    format!("Attribute references external entity '{}'\n", name),
                );
                None
            }
            Some(e) => Some(e),
        }
    }

    fn handle_undeclared_entity(&mut self, name: &str) {
        if self.st.standalone == 1 || (!self.st.has_external_subset && !self.st.has_perefs) {
            self.fatal_msg(
                XML_ERR_UNDECLARED_ENTITY,
                format!("Entity '{}' not defined\n", name),
            );
        } else if self.st.options & XML_PARSE_DTDVALID != 0 {
            self.valid_err(
                XML_ERR_UNDECLARED_ENTITY,
                format!("Entity '{}' not defined\n", name),
            );
        } else if (self.st.options & (XML_PARSE_DTDLOAD | XML_PARSE_DTDATTR)) != 0
            || (self.st.replace_entities && self.st.options & XML_PARSE_NO_XXE == 0)
        {
            self.err_msg(
                XML_WAR_UNDECLARED_ENTITY,
                format!("Entity '{}' not defined\n", name),
            );
        } else {
            self.warning(
                XML_WAR_UNDECLARED_ENTITY,
                format!("Entity '{}' not defined\n", name),
            );
        }
        self.st.valid = false;
    }

    /// xmlParseCharRef: returns the value (0 on error).
    fn parse_char_ref(&mut self) -> u32 {
        let mut val: u32 = 0;
        if self.cur() == b'&' && self.nxt(1) == b'#' && self.nxt(2) == b'x' {
            self.skip(3);
            let mut count = 0;
            while self.cur() != b';' && !self.stopped() {
                let c = self.cur();
                let d = match c {
                    b'0'..=b'9' => (c - b'0') as u32,
                    b'a'..=b'f' if count < 20 => (c - b'a') as u32 + 10,
                    b'A'..=b'F' if count < 20 => (c - b'A') as u32 + 10,
                    _ => {
                        self.fatal(XML_ERR_INVALID_HEX_CHARREF, None);
                        val = 0;
                        break;
                    }
                };
                val = (val * 16 + d).min(0x110000);
                self.skip(1);
                count += 1;
            }
            if self.cur() == b';' {
                self.skip(1);
            }
        } else if self.cur() == b'&' && self.nxt(1) == b'#' {
            self.skip(2);
            while self.cur() != b';' {
                let c = self.cur();
                if c.is_ascii_digit() {
                    val = (val * 10 + (c - b'0') as u32).min(0x110000);
                } else {
                    self.fatal(XML_ERR_INVALID_DEC_CHARREF, None);
                    val = 0;
                    break;
                }
                self.skip(1);
            }
            if self.cur() == b';' {
                self.skip(1);
            }
        } else {
            if self.cur() == b'&' {
                self.skip(1);
            }
            self.fatal(XML_ERR_INVALID_CHARREF, None);
        }
        if val >= 0x110000 {
            self.fatal_msg(
                XML_ERR_INVALID_CHAR,
                format!("xmlParseCharRef: character reference out of bounds\n"),
            );
            return 0;
        } else if !is_char(val) {
            self.fatal_msg(
                XML_ERR_INVALID_CHAR,
                format!("xmlParseCharRef: invalid xmlChar value {}\n", val),
            );
            return 0;
        }
        val
    }

    fn parse_entity_ref_name(&mut self) -> Option<String> {
        if self.cur() != b'&' {
            return None;
        }
        self.skip(1);
        let name = match self.parse_name() {
            Some(n) => n,
            None => {
                self.fatal_msg(XML_ERR_NAME_REQUIRED, "xmlParseEntityRef: no name\n".into());
                return None;
            }
        };
        if self.cur() != b';' {
            self.fatal(XML_ERR_ENTITYREF_SEMICOL_MISSING, None);
            return None;
        }
        self.skip(1);
        Some(name)
    }

    /// Expand entity references within an attribute value (entity content).
    fn expand_entity_in_att(
        &mut self,
        out: &mut Vec<u8>,
        s: &str,
        pent: Option<&str>,
        normalize: bool,
        in_space: &mut bool,
        depth: usize,
    ) {
        if depth > 40 {
            self.fatal_msg(
                XML_ERR_RESOURCE_LIMIT,
                "Maximum entity nesting depth exceeded".into(),
            );
            return;
        }
        if let Some(p) = pent {
            if self.entities.get(p).map(|e| e.expanding).unwrap_or(false) {
                self.fatal(XML_ERR_ENTITY_LOOP, None);
                return;
            }
        }
        let b = s.as_bytes();
        let mut i = 0;
        while i < b.len() && !self.stopped() {
            let c = b[i];
            if c != b'&' {
                if pent.is_some() && c == b'<' {
                    self.fatal_msg(
                        XML_ERR_LT_IN_ATTRIBUTE,
                        format!(
                            "'<' in entity '{}' is not allowed in attributes values\n",
                            pent.unwrap()
                        ),
                    );
                    break;
                }
                if c <= 0x20 {
                    if normalize && *in_space {
                    } else if c < 0x20 {
                        out.push(b' ');
                    } else {
                        out.push(c);
                    }
                    *in_space = true;
                } else {
                    out.push(c);
                    *in_space = false;
                }
                i += 1;
            } else if i + 1 < b.len() && b[i + 1] == b'#' {
                let mut j = i + 2;
                let hex = j < b.len() && b[j] == b'x';
                if hex {
                    j += 1;
                }
                let st = j;
                while j < b.len() && b[j] != b';' {
                    j += 1;
                }
                let digits = std::str::from_utf8(&b[st..j]).unwrap_or("");
                let v = if hex {
                    u32::from_str_radix(digits, 16).unwrap_or(0)
                } else {
                    digits.parse::<u32>().unwrap_or(0)
                };
                i = (j + 1).min(b.len());
                if v == b' ' as u32 {
                    if !(normalize && *in_space) {
                        out.push(b' ');
                    }
                    *in_space = true;
                } else {
                    push_char(out, v);
                    *in_space = false;
                }
            } else {
                let mut j = i + 1;
                while j < b.len() && b[j] != b';' {
                    j += 1;
                }
                let name = String::from_utf8_lossy(&b[i + 1..j]).into_owned();
                i = (j + 1).min(b.len());
                let ent = self.lookup_general_entity(&name, true);
                match ent {
                    Some(e) if e.kind == EntKind::Predefined => {
                        out.extend_from_slice(e.content.as_bytes());
                        *in_space = false;
                    }
                    Some(e) => {
                        if let Some(p) = pent {
                            if let Some(pe) = self.entities.get_mut(p) {
                                pe.expanding = true;
                            }
                        }
                        let content = e.content.clone();
                        self.expand_entity_in_att(
                            out,
                            &content,
                            Some(&name),
                            normalize,
                            in_space,
                            depth + 1,
                        );
                        if let Some(p) = pent {
                            if let Some(pe) = self.entities.get_mut(p) {
                                pe.expanding = false;
                            }
                        }
                    }
                    None => {}
                }
            }
        }
    }

    /// xmlParseAttValueInternal. `special` is the declared type (1 = CDATA).
    fn parse_att_value_internal(&mut self, special: u8, is_namespace: bool) -> Option<String> {
        let normalize = special > 1;
        let replace = self.st.replace_entities || is_namespace;
        let q = self.cur();
        if q != b'"' && q != b'\'' {
            self.fatal(XML_ERR_ATTRIBUTE_NOT_STARTED, None);
            return None;
        }
        self.skip(1);
        let mut buf: Vec<u8> = Vec::new();
        let mut in_space = true;
        loop {
            if self.stopped() {
                return None;
            }
            if self.at_end() {
                self.fatal_msg(
                    XML_ERR_ATTRIBUTE_NOT_FINISHED,
                    "AttValue: ' expected\n".into(),
                );
                return None;
            }
            let c = self.cur();
            if c >= 0x80 {
                let (ch, l) = self.cur_char();
                if !is_char(ch) {
                    self.fatal_msg(
                        XML_ERR_INVALID_CHAR,
                        "invalid character in attribute value\n".into(),
                    );
                    push_char(&mut buf, 0xFFFD);
                } else {
                    push_char(&mut buf, ch);
                }
                self.skip(l.max(1));
                in_space = false;
            } else if c != b'&' {
                if c > 0x20 {
                    if c == q {
                        break;
                    }
                    if c == b'<' {
                        self.fatal(XML_ERR_LT_IN_ATTRIBUTE, None);
                    }
                    buf.push(c);
                    in_space = false;
                } else if !(c == 0x9 || c == 0xA || c == 0xD || c == 0x20) {
                    self.fatal_msg(
                        XML_ERR_INVALID_CHAR,
                        "invalid character in attribute value\n".into(),
                    );
                    push_char(&mut buf, 0xFFFD);
                    in_space = false;
                } else {
                    if normalize && in_space {
                    } else if c < 0x20 {
                        buf.push(b' ');
                    } else {
                        buf.push(c);
                    }
                    in_space = true;
                    if c == 0xD && self.nxt(1) == 0xA {
                        self.skip(1);
                    }
                }
                self.skip(1);
            } else if self.nxt(1) == b'#' {
                let val = self.parse_char_ref();
                if val == 0 {
                    return None;
                }
                if val == b'&' as u32 && !replace {
                    buf.extend_from_slice(b"&#38;");
                    in_space = false;
                } else if val == b' ' as u32 {
                    if !(normalize && in_space) {
                        buf.push(b' ');
                    }
                    in_space = true;
                } else {
                    push_char(&mut buf, val);
                    in_space = false;
                }
            } else {
                let name = match self.parse_entity_ref_name() {
                    Some(n) => n,
                    None => continue,
                };
                let ent = match self.lookup_general_entity(&name, true) {
                    Some(e) => e,
                    None => continue,
                };
                if ent.kind == EntKind::Predefined {
                    if ent.content == "&" && !replace {
                        buf.extend_from_slice(b"&#38;");
                    } else {
                        buf.extend_from_slice(ent.content.as_bytes());
                    }
                    in_space = false;
                } else if replace {
                    let content = ent.content.clone();
                    self.expand_entity_in_att(
                        &mut buf,
                        &content,
                        Some(&name),
                        normalize,
                        &mut in_space,
                        0,
                    );
                } else {
                    // check the entity content for '<'
                    if ent.content.contains('<') {
                        self.fatal_msg(
                            XML_ERR_LT_IN_ATTRIBUTE,
                            format!(
                                "'<' in entity '{}' is not allowed in attributes values\n",
                                name
                            ),
                        );
                    }
                    buf.push(b'&');
                    buf.extend_from_slice(name.as_bytes());
                    buf.push(b';');
                    in_space = false;
                }
            }
        }
        if normalize && in_space && !buf.is_empty() {
            buf.pop();
        }
        self.skip(1);
        Some(String::from_utf8_lossy(&buf).into_owned())
    }

    fn parse_attribute2(
        &mut self,
        elem_qname: &str,
    ) -> Option<(Option<String>, String, Option<String>)> {
        let (prefix, name) = match self.parse_qname() {
            Some(x) => x,
            None => {
                self.fatal_msg(
                    XML_ERR_NAME_REQUIRED,
                    "error parsing attribute name\n".into(),
                );
                return None;
            }
        };
        let aq = match &prefix {
            Some(p) => format!("{}:{}", p, name),
            None => name.clone(),
        };
        let special = self
            .atts_special
            .get(&(elem_qname.to_string(), aq))
            .copied()
            .unwrap_or(0);
        self.skip_blanks();
        if self.cur() != b'=' {
            self.fatal_msg(
                XML_ERR_ATTRIBUTE_WITHOUT_VALUE,
                format!("Specification mandates value for attribute {}\n", name),
            );
            return Some((prefix, name, None));
        }
        self.skip(1);
        self.skip_blanks();
        let is_ns = (prefix.is_none() && name == "xmlns") || prefix.as_deref() == Some("xmlns");
        let val = self.parse_att_value_internal(special, is_ns);
        let val = match val {
            Some(v) => v,
            None => return Some((prefix, name, None)),
        };
        if prefix.as_deref() == Some("xml") && name == "space" {
            if val == "default" {
                if let Some(s) = self.space.last_mut() {
                    *s = 0;
                }
            } else if val == "preserve" {
                if let Some(s) = self.space.last_mut() {
                    *s = 1;
                }
            } else {
                self.warning(XML_WAR_SPACE_VALUE, format!("Invalid value \"{}\" for xml:space : \"default\" or \"preserve\" expected\n", val));
            }
        }
        Some((prefix, name, Some(val)))
    }

    // ------------------------------------------------------------------
    // Elements

    /// xmlParseStartTag2. Returns (localname, prefix, uri, nbns) or None.
    fn parse_start_tag2(
        &mut self,
    ) -> Option<(String, Option<String>, Option<String>, usize, bool)> {
        if self.cur() != b'<' {
            return None;
        }
        self.skip(1);
        self.elem_id += 1;
        let (prefix, localname) = match self.parse_qname() {
            Some(x) => x,
            None => {
                self.fatal_msg(
                    XML_ERR_NAME_REQUIRED,
                    "StartTag: invalid element name\n".into(),
                );
                return None;
            }
        };
        let elem_qname = match &prefix {
            Some(p) => format!("{}:{}", p, localname),
            None => localname.clone(),
        };
        self.skip_blanks();
        let mut nb_ns = 0usize;
        // raw attributes: (localname, prefix, value)
        let mut atts: Vec<(String, Option<String>, String)> = Vec::new();
        while self.cur() != b'>'
            && !(self.cur() == b'/' && self.nxt(1) == b'>')
            && !self.at_end()
            && !self.stopped()
        {
            let r = self.parse_attribute2(&elem_qname);
            let (aprefix, attname, value) = match r {
                Some(x) => x,
                None => break,
            };
            if let Some(value) = value {
                if attname == "xmlns" && aprefix.is_none() {
                    let uri = value;
                    let mut skip = false;
                    if !uri.is_empty() {
                        match crate::uri::parse_uri(&uri) {
                            None => self.ns_err(
                                XML_WAR_NS_URI,
                                format!("xmlns: '{}' is not a valid URI\n", uri),
                            ),
                            Some(u) => {
                                if u.scheme.is_none() {
                                    self.ns_warn(
                                        XML_WAR_NS_URI_RELATIVE,
                                        format!("xmlns: URI {} is not absolute\n", uri),
                                    );
                                }
                            }
                        }
                        if uri == NS_XML {
                            self.ns_err(
                                XML_NS_ERR_XML_NAMESPACE,
                                "xml namespace URI cannot be the default namespace\n".into(),
                            );
                            skip = true;
                        } else if uri == "http://www.w3.org/2000/xmlns/" {
                            self.ns_err(
                                XML_NS_ERR_XML_NAMESPACE,
                                "reuse of the xmlns namespace name is forbidden\n".into(),
                            );
                            skip = true;
                        }
                    }
                    if !skip {
                        nb_ns += self.ns_push(None, &uri, false);
                    }
                } else if aprefix.as_deref() == Some("xmlns") {
                    let uri = value;
                    if attname == "xml" {
                        if uri != NS_XML {
                            self.ns_err(
                                XML_NS_ERR_XML_NAMESPACE,
                                "xml namespace prefix mapped to wrong URI\n".into(),
                            );
                        }
                    } else if uri == NS_XML {
                        self.ns_err(
                            XML_NS_ERR_XML_NAMESPACE,
                            "xml namespace URI mapped to wrong prefix\n".into(),
                        );
                    } else if attname == "xmlns" {
                        self.ns_err(
                            XML_NS_ERR_XML_NAMESPACE,
                            "redefinition of the xmlns prefix is forbidden\n".into(),
                        );
                    } else if uri == "http://www.w3.org/2000/xmlns/" {
                        self.ns_err(
                            XML_NS_ERR_XML_NAMESPACE,
                            "reuse of the xmlns namespace name is forbidden\n".into(),
                        );
                    } else if uri.is_empty() {
                        self.ns_err(
                            XML_NS_ERR_XML_NAMESPACE,
                            format!("xmlns:{}: Empty XML namespace is not allowed\n", attname),
                        );
                    } else {
                        match crate::uri::parse_uri(&uri) {
                            None => self.ns_err(
                                XML_WAR_NS_URI,
                                format!("xmlns:{}: '{}' is not a valid URI\n", attname, uri),
                            ),
                            Some(u) => {
                                if self.st.options & XML_PARSE_PEDANTIC != 0 && u.scheme.is_none() {
                                    self.ns_warn(
                                        XML_WAR_NS_URI_RELATIVE,
                                        format!("xmlns:{}: URI {} is not absolute\n", attname, uri),
                                    );
                                }
                            }
                        }
                        nb_ns += self.ns_push(Some(&attname), &uri, false);
                    }
                } else {
                    atts.push((attname, aprefix, value));
                }
            }
            if self.cur() == b'>' || (self.cur() == b'/' && self.nxt(1) == b'>') {
                break;
            }
            if self.skip_blanks() == 0 {
                self.fatal_msg(
                    XML_ERR_SPACE_REQUIRED,
                    "attributes construct error\n".into(),
                );
                break;
            }
        }
        // namespaces from default attributes
        let defaults = self
            .atts_default
            .get(&elem_qname)
            .cloned()
            .unwrap_or_default();
        for (dp, dl, dv) in &defaults {
            if dl == "xmlns" && dp.is_none() {
                nb_ns += self.ns_push(None, dv, true);
            } else if dp.as_deref() == Some("xmlns") {
                nb_ns += self.ns_push(Some(dl), dv, true);
            }
        }
        // resolve attribute namespaces
        let mut resolved: Vec<SaxAttr> = Vec::new();
        for (name, aprefix, value) in &atts {
            let uri = match aprefix.as_deref() {
                None => None,
                Some("xml") => Some(NS_XML.to_string()),
                Some(p) => match self.ns_lookup(Some(p)) {
                    Some(i) => Some(self.ns[i].uri.clone()),
                    None => {
                        self.ns_err(
                            XML_NS_ERR_UNDEFINED_NAMESPACE,
                            format!(
                                "Namespace prefix {} for {} on {} is not defined\n",
                                p, name, localname
                            ),
                        );
                        None
                    }
                },
            };
            resolved.push(SaxAttr {
                localname: name.clone(),
                prefix: aprefix.clone(),
                uri,
                value: value.clone(),
                defaulted: false,
            });
        }
        // uniqueness
        let mut num_dup = 0;
        let mut num_ns_err = 0;
        for i in 0..resolved.len() {
            if resolved[i].prefix.is_some() && resolved[i].uri.is_none() {
                continue;
            }
            for j in 0..i {
                if resolved[j].prefix.is_some() && resolved[j].uri.is_none() {
                    continue;
                }
                if resolved[j].localname == resolved[i].localname
                    && resolved[j].uri == resolved[i].uri
                {
                    if resolved[j].prefix == resolved[i].prefix {
                        match &resolved[i].prefix {
                            None => self.fatal_msg(
                                XML_ERR_ATTRIBUTE_REDEFINED,
                                format!("Attribute {} redefined\n", resolved[i].localname),
                            ),
                            Some(p) => self.fatal_msg(
                                XML_ERR_ATTRIBUTE_REDEFINED,
                                format!("Attribute {}:{} redefined\n", p, resolved[i].localname),
                            ),
                        }
                        num_dup += 1;
                    } else {
                        let u = resolved[i].uri.clone().unwrap_or_default();
                        self.ns_err(
                            XML_NS_ERR_ATTRIBUTE_REDEFINED,
                            format!(
                                "Namespaced Attribute {} in '{}' redefined\n",
                                resolved[i].localname, u
                            ),
                        );
                        num_ns_err += 1;
                    }
                    break;
                }
            }
        }
        if num_dup == 0 && num_ns_err > 1 {
            for i in 0..resolved.len() {
                if resolved[i].prefix.is_none() {
                    continue;
                }
                for j in 0..i {
                    if resolved[j].prefix == resolved[i].prefix
                        && resolved[j].localname == resolved[i].localname
                    {
                        let p = resolved[i].prefix.clone().unwrap();
                        self.fatal_msg(
                            XML_ERR_ATTRIBUTE_REDEFINED,
                            format!("Attribute {}:{} redefined\n", p, resolved[i].localname),
                        );
                        break;
                    }
                }
            }
        }
        // default attributes
        let mut nb_def = 0;
        for (dp, dl, dv) in &defaults {
            if (dl == "xmlns" && dp.is_none()) || dp.as_deref() == Some("xmlns") {
                continue;
            }
            let uri = match dp.as_deref() {
                None => None,
                Some("xml") => Some(NS_XML.to_string()),
                Some(p) => match self.ns_lookup(Some(p)) {
                    Some(i) => Some(self.ns[i].uri.clone()),
                    None => {
                        self.ns_err(
                            XML_NS_ERR_UNDEFINED_NAMESPACE,
                            format!(
                                "Namespace prefix {} for {} on {} is not defined\n",
                                p, dl, localname
                            ),
                        );
                        None
                    }
                },
            };
            if resolved.iter().any(|a| a.localname == *dl && a.uri == uri) {
                continue;
            }
            resolved.push(SaxAttr {
                localname: dl.clone(),
                prefix: dp.clone(),
                uri,
                value: dv.clone(),
                defaulted: true,
            });
            nb_def += 1;
        }
        let uri = self
            .ns_lookup(prefix.as_deref())
            .map(|i| self.ns[i].uri.clone());
        let uri = if prefix.is_none() {
            uri.filter(|u| !u.is_empty())
        } else {
            uri
        };
        if prefix.is_some() && uri.is_none() {
            self.ns_err(
                XML_NS_ERR_UNDEFINED_NAMESPACE,
                format!(
                    "Namespace prefix {} on {} is not defined\n",
                    prefix.as_deref().unwrap(),
                    localname
                ),
            );
        }
        let called = self.sax_ok();
        if called {
            let nsdecls: Vec<(Option<String>, String)> = self.ns[self.ns.len() - nb_ns..]
                .iter()
                .map(|e| (e.prefix.clone(), e.uri.clone()))
                .collect();
            let line = self.st.cur_line();
            self.sink.start_element(
                &mut self.st,
                &localname,
                prefix.as_deref(),
                uri.as_deref(),
                &nsdecls,
                &resolved,
                nb_def,
                line,
            );
        }
        Some((localname, prefix, uri, nb_ns, called))
    }

    fn parse_element(&mut self) {
        if self.parse_element_start() != 0 {
            return;
        }
        self.parse_content_internal();
        if self.at_end() {
            if self.st.well_formed {
                if let Some(n) = self.names.last() {
                    let msg = format!("Premature end of data in tag {} line {}\n", n.name, n.line);
                    self.fatal_msg(XML_ERR_TAG_NOT_FINISHED, msg);
                }
            }
            return;
        }
        self.parse_element_end();
    }

    /// Returns -1 on error, 0 on start tag, 1 on empty element.
    fn parse_element_start(&mut self) -> i32 {
        if self.names.len() > self.depth_limit {
            let n = self.names.len();
            self.fatal_msg(
                XML_ERR_RESOURCE_LIMIT,
                format!(
                    "Excessive depth in document: {} use XML_PARSE_HUGE option\n",
                    n
                ),
            );
            return -1;
        }
        let sp = match self.space.last() {
            None => -1,
            Some(-2) => -1,
            Some(v) => *v,
        };
        self.space.push(sp);
        let line = self.st.cur_line();
        let r = self.parse_start_tag2();
        let (name, prefix, uri, nb_ns, has_node) = match r {
            Some(x) => x,
            None => {
                self.space.pop();
                return -1;
            }
        };
        self.names.push(NameEntry {
            name: name.clone(),
            prefix: prefix.clone(),
            uri: uri.clone(),
            line,
            ns_nr: nb_ns,
            has_node,
        });
        if self.cur() == b'/' && self.nxt(1) == b'>' {
            self.skip(2);
            if self.sax_ok() {
                self.sink
                    .end_element(&mut self.st, &name, prefix.as_deref(), uri.as_deref());
            } else if has_node {
                self.sink
                    .end_element(&mut self.st, "\u{0}abort", None, None);
            }
            self.names.pop();
            self.space.pop();
            self.ns_pop(nb_ns);
            return 1;
        }
        if self.cur() == b'>' {
            self.skip(1);
        } else {
            let qn = match &prefix {
                Some(p) => format!("{}:{}", p, name),
                None => name.clone(),
            };
            let _ = qn;
            self.fatal_msg(
                XML_ERR_GT_REQUIRED,
                format!("Couldn't find end of Start Tag {} line {}\n", name, line),
            );
            // nodePop: tell the sink to close the element
            if has_node {
                self.sink
                    .end_element(&mut self.st, "\u{0}abort", None, None);
            }
            self.names.pop();
            self.space.pop();
            self.ns_pop(nb_ns);
            return -1;
        }
        0
    }

    fn parse_element_end(&mut self) {
        if self.names.is_empty() {
            if self.cur() == b'<' && self.nxt(1) == b'/' {
                self.skip(2);
            }
            return;
        }
        self.parse_end_tag2();
        self.names.pop();
    }

    fn parse_end_tag2(&mut self) {
        let (tname, tprefix, turi, tline, tns, thas) = {
            let t = self.names.last().unwrap();
            (
                t.name.clone(),
                t.prefix.clone(),
                t.uri.clone(),
                t.line,
                t.ns_nr,
                t.has_node,
            )
        };
        if !(self.cur() == b'<' && self.nxt(1) == b'/') {
            self.fatal(XML_ERR_LTSLASH_REQUIRED, None);
            return;
        }
        self.skip(2);
        // compare name
        let mut matched = false;
        let qn = match &tprefix {
            Some(p) => format!("{}:{}", p, tname),
            None => tname.clone(),
        };
        let mismatch_name: Option<String>;
        if self.cmp(qn.as_bytes()) {
            let nb = self.nxt(qn.len());
            if nb == b'>' || is_blank(nb) {
                self.skip(qn.len());
                matched = true;
            }
        }
        if matched {
            mismatch_name = None;
        } else {
            match self.parse_qname() {
                None => mismatch_name = Some("unparsable".into()),
                Some((p, l)) => {
                    let full = match &p {
                        Some(pp) => format!("{}:{}", pp, l),
                        None => l.clone(),
                    };
                    if full == qn {
                        mismatch_name = None;
                    } else {
                        // libxml2 reports the local name for prefixed tags
                        mismatch_name = Some(if tprefix.is_some() { l } else { full });
                    }
                }
            }
        }
        self.skip_blanks();
        if self.cur() != b'>' {
            self.fatal(XML_ERR_GT_REQUIRED, None);
        } else {
            self.skip(1);
        }
        if let Some(m) = mismatch_name {
            self.fatal_msg(
                XML_ERR_TAG_NAME_MISMATCH,
                format!(
                    "Opening and ending tag mismatch: {} line {} and {}\n",
                    tname, tline, m
                ),
            );
        }
        if self.sax_ok() {
            self.sink
                .end_element(&mut self.st, &tname, tprefix.as_deref(), turi.as_deref());
        } else if thas {
            self.sink
                .end_element(&mut self.st, "\u{0}abort", None, None);
        }
        self.space.pop();
        self.ns_pop(tns);
    }

    fn parse_content_internal(&mut self) {
        let old_name_nr = self.names.len();
        let old_space_nr = self.space.len();
        while !self.at_end() && !self.stopped() {
            let c = self.cur();
            if c == b'<' && self.nxt(1) == b'?' {
                self.parse_pi();
            } else if self.cmp(b"<![CDATA[") {
                self.parse_cdsect();
            } else if self.cmp(b"<!--") {
                self.parse_comment();
            } else if c == b'<' {
                if self.nxt(1) == b'/' {
                    if self.names.len() <= old_name_nr {
                        break;
                    }
                    self.parse_element_end();
                } else {
                    self.parse_element_start();
                }
            } else if c == b'&' {
                self.parse_reference();
            } else {
                self.parse_char_data();
            }
        }
        if self.names.len() > old_name_nr && self.at_end() && self.st.well_formed {
            let n = self.names.last().unwrap();
            let msg = format!("Premature end of data in tag {} line {}\n", n.name, n.line);
            self.fatal_msg(XML_ERR_TAG_NOT_FINISHED, msg);
        }
        while self.names.len() > old_name_nr {
            let t = self.names.pop().unwrap();
            self.ns_pop(t.ns_nr);
            // close the node in the sink (nodePop)
            if t.has_node {
                self.sink
                    .end_element(&mut self.st, "\u{0}abort", None, None);
            }
        }
        while self.space.len() > old_space_nr {
            self.space.pop();
        }
    }

    fn parse_cdsect(&mut self) {
        self.skip(9);
        let (mut r, rl) = self.cur_char();
        if rl == 0 || !is_char(r) {
            self.fatal(XML_ERR_CDATA_NOT_FINISHED, None);
            return;
        }
        self.skip(rl);
        let (mut s, sl) = self.cur_char();
        if sl == 0 || !is_char(s) {
            self.fatal(XML_ERR_CDATA_NOT_FINISHED, None);
            return;
        }
        self.skip(sl);
        let (mut cur, mut l) = self.cur_char();
        let mut buf = Vec::new();
        while l != 0 && is_char(cur) && (r != b']' as u32 || s != b']' as u32 || cur != b'>' as u32)
        {
            push_char(&mut buf, r);
            r = s;
            s = cur;
            self.skip(l);
            let x = self.cur_char();
            cur = x.0;
            l = x.1;
        }
        let text = String::from_utf8_lossy(&buf).into_owned();
        if cur != b'>' as u32 || l == 0 {
            let p50: String = text.chars().take(50).collect();
            self.fatal_msg(
                XML_ERR_CDATA_NOT_FINISHED,
                format!("CData section not finished\n{}\n", p50),
            );
            return;
        }
        self.skip(l);
        if self.sax_ok() {
            if self.st.options & XML_PARSE_NOCDATA == 0 {
                self.sink.cdata_block(&mut self.st, &text);
            } else {
                self.sink.characters(&mut self.st, &text);
            }
        }
    }

    fn are_blanks(&mut self, text: &str, blank_chars: bool) -> bool {
        match self.space.last() {
            None | Some(1) | Some(-2) => return false,
            _ => {}
        }
        if !blank_chars && !text.bytes().all(is_blank) {
            return false;
        }
        let elem = self.sink.current_element();
        let (ename, eprefix) = match elem {
            None => return false,
            Some(x) => x,
        };
        let qn = match eprefix {
            Some(p) => format!("{}:{}", p, ename),
            None => ename,
        };
        let et = self.elem_decls.get(&qn).copied();
        if let Some(t) = et {
            if t == 4 {
                return true;
            }
            if t == 2 || t == 3 {
                return false;
            }
        }
        let c = self.cur();
        if c != b'<' && c != 0xD {
            return false;
        }
        self.sink
            .are_blanks_tree(c == b'<', c == b'<' && self.nxt(1) == b'/', et)
    }

    fn characters(&mut self, text: &str, is_blank: bool) {
        if !self.sax_ok() {
            return;
        }
        let check_blanks = !self.st.keep_blanks;
        if check_blanks && self.are_blanks(text, is_blank) {
            // ignorable whitespace: dropped when !keepBlanks
        } else {
            self.sink.characters(&mut self.st, text);
            if check_blanks {
                if let Some(s) = self.space.last_mut() {
                    if *s == -1 {
                        *s = -2;
                    }
                }
            }
        }
    }

    fn deliver(&mut self, start: usize, end: usize, is_blank: bool) {
        if end <= start {
            return;
        }
        let s = String::from_utf8_lossy(&self.inp().data[start..end]).into_owned();
        // advance before the callback (libxml2 moves cur first)
        self.inp_mut().pos = end;
        self.characters(&s, is_blank);
    }

    /// xmlParseCharDataInternal
    fn parse_char_data(&mut self) {
        let data = self.inp().data.clone();
        let d = &data[..];
        let end = d.len();
        let at = |i: usize| -> u8 { if i < end { d[i] } else { 0 } };
        let mut inp = self.pos();
        loop {
            // get_more_space
            loop {
                while at(inp) == 0x20 {
                    inp += 1;
                }
                if at(inp) == 0xA {
                    while at(inp) == 0xA {
                        inp += 1;
                    }
                    continue;
                }
                break;
            }
            if at(inp) == b'<' {
                let st = self.pos();
                self.deliver(st, inp, true);
                return;
            }
            // get_more
            loop {
                while is_test_char_data(at(inp)) {
                    inp += 1;
                }
                if at(inp) == 0xA {
                    while at(inp) == 0xA {
                        inp += 1;
                    }
                    continue;
                }
                if at(inp) == b']' {
                    if at(inp + 1) == b']' && at(inp + 2) == b'>' {
                        self.inp_mut().pos = inp;
                        self.fatal(XML_ERR_MISPLACED_CDATA_END, None);
                    }
                    inp += 1;
                    continue;
                }
                break;
            }
            let st = self.pos();
            self.deliver(st, inp, false);
            self.inp_mut().pos = inp;
            if at(inp) == 0xD {
                if at(inp + 1) == 0xA {
                    self.inp_mut().pos = inp + 1;
                    inp += 2;
                    // deliver nothing for the \r, continue
                    let c = at(inp);
                    if (0x20..=0x7F).contains(&c) || c == 0x09 || c == 0x0A {
                        // the \n itself starts the next chunk
                        inp = self.pos();
                        continue;
                    }
                    inp = self.pos();
                    continue;
                }
            }
            if at(inp) == b'<' || at(inp) == b'&' {
                return;
            }
            if self.at_end() {
                return;
            }
            inp = self.pos();
            let c = at(inp);
            if !((0x20..=0x7F).contains(&c) || c == 0x09 || c == 0x0A) {
                break;
            }
        }
        self.parse_char_data_complex();
    }

    fn parse_char_data_complex(&mut self) {
        let mut buf: Vec<u8> = Vec::new();
        let (mut cur, mut l) = self.cur_char();
        while l != 0 && cur != b'<' as u32 && cur != b'&' as u32 && is_char(cur) {
            if cur == b']' as u32 && self.nxt(1) == b']' && self.nxt(2) == b'>' {
                self.fatal(XML_ERR_MISPLACED_CDATA_END, None);
            }
            push_char(&mut buf, cur);
            self.skip(l);
            if buf.len() >= 300 {
                let s = String::from_utf8_lossy(&buf).into_owned();
                buf.clear();
                self.characters(&s, false);
            }
            let x = self.cur_char();
            cur = x.0;
            l = x.1;
        }
        if !buf.is_empty() {
            let s = String::from_utf8_lossy(&buf).into_owned();
            self.characters(&s, false);
        }
        if !self.at_end()
            && cur != b'<' as u32
            && cur != b'&' as u32
            && cur != b']' as u32
            && l != 0
        {
            self.fatal_msg(
                XML_ERR_INVALID_CHAR,
                format!("PCDATA invalid Char value {}\n", cur),
            );
            self.skip(l);
        } else if !self.at_end() && l != 0 && cur == b']' as u32 {
            // not reached normally
        }
    }

    /// xmlParseReference
    fn parse_reference(&mut self) {
        if self.cur() != b'&' {
            return;
        }
        if self.nxt(1) == b'#' {
            let v = self.parse_char_ref();
            if v == 0 {
                return;
            }
            let mut out = Vec::new();
            push_char(&mut out, v);
            if self.sax_ok() {
                let s = String::from_utf8_lossy(&out).into_owned();
                self.sink.characters(&mut self.st, &s);
            }
            return;
        }
        let name = match self.parse_entity_ref_name() {
            Some(n) => n,
            None => return,
        };
        let ent = self.lookup_general_entity(&name, false);
        let ent = match ent {
            None => {
                if !self.st.replace_entities && self.sax_ok() {
                    self.sink.reference(&mut self.st, &name);
                }
                return;
            }
            Some(e) => e,
        };
        if !self.st.well_formed {
            return;
        }
        if ent.kind == EntKind::Predefined {
            if self.sax_ok() {
                self.sink.characters(&mut self.st, &ent.content);
            }
            return;
        }
        let mut ent = ent;
        let mut efile: Option<String> = None;
        if ent.kind != EntKind::InternalGeneral {
            // external parsed entity: loaded when substituting or
            // validating (xmlCtxtParseEntity), unless XXE is disabled
            let load = self.st.options & XML_PARSE_NO_XXE == 0
                && (self.st.replace_entities || self.st.options & XML_PARSE_DTDVALID != 0);
            let loaded = if load && ent.orig.is_none() {
                let r = self.load_entity_content(&ent);
                if let Some(pe) = self.entities.get_mut(&name) {
                    // remember the outcome (content, file) in `orig`/`notation`
                    match &r {
                        Some((c, f)) => {
                            pe.content = c.clone();
                            pe.orig = Some(f.clone());
                        }
                        None => pe.orig = Some(String::new()),
                    }
                }
                r.is_some()
            } else {
                load && ent.orig.as_deref().map(|f| !f.is_empty()).unwrap_or(false)
            };
            if !loaded {
                if self.sax_ok() && !self.st.replace_entities {
                    self.sink.reference(&mut self.st, &name);
                }
                return;
            }
            ent = self.entities.get(&name).cloned().unwrap_or(ent);
            efile = ent.orig.clone();
        }
        if self.st.replace_entities {
            // parse the entity content inline
            self.parse_entity_content_file(&name, &ent.content, true, efile);
            return;
        }
        // first reference: parse content into the entity
        if !ent.parsed {
            if let Some(e) = self.entities.get_mut(&name) {
                e.parsed = true;
            }
            let wants = self.sink.builds_tree() && self.sink.begin_entity_content(&name);
            if wants {
                self.parse_entity_content_file(&name, &ent.content, false, efile);
                self.sink.end_entity_content(&name);
            } else {
                // still check well-formedness of the content
                let quiet = self.st.quiet;
                let _ = quiet;
                self.parse_entity_content_check(&name, &ent.content);
            }
        }
        if self.sax_ok() {
            self.sink.reference(&mut self.st, &name);
        }
    }

    fn parse_entity_content_check(&mut self, _name: &str, _content: &str) {}

    /// Parse entity content as a new input (named `file` for external
    /// entities, so diagnostics point into it).
    fn parse_entity_content_file(
        &mut self,
        name: &str,
        content: &str,
        inline: bool,
        file: Option<String>,
    ) {
        if self
            .entities
            .get(name)
            .map(|e| e.expanding)
            .unwrap_or(false)
        {
            self.fatal(XML_ERR_ENTITY_LOOP, None);
            return;
        }
        if self.st.inputs.len() > 40 {
            self.fatal(XML_ERR_ENTITY_LOOP, None);
            return;
        }
        if let Some(e) = self.entities.get_mut(name) {
            e.expanding = true;
        }
        let mut inp = Input::new(Rc::new(content.as_bytes().to_vec()), file);
        inp.entity = Some(name.to_string());
        self.st.inputs.push(inp);
        let old_min = self.ns_min;
        if !inline {
            self.ns_min = self.ns.len();
        }
        self.names.push(NameEntry {
            name: "#root".into(),
            prefix: None,
            uri: None,
            line: 0,
            ns_nr: 0,
            has_node: false,
        });
        self.space.push(-1);
        self.parse_content_internal();
        if !self.at_end() && !self.stopped() {
            self.fatal(XML_ERR_NOT_WELL_BALANCED, None);
        }
        self.names.pop();
        self.space.pop();
        self.ns_min = old_min;
        self.st.inputs.pop();
        if let Some(e) = self.entities.get_mut(name) {
            e.expanding = false;
        }
    }
}

/// Whether `b[i..]` is a truncated (so far valid) UTF-8 sequence.
fn incomplete_utf8_at_end(b: &[u8], i: usize) -> bool {
    let c = b[i];
    let need = if c & 0xE0 == 0xC0 {
        2
    } else if c & 0xF0 == 0xE0 {
        3
    } else if c & 0xF8 == 0xF0 {
        4
    } else {
        return false;
    };
    i + need > b.len() && b[i + 1..].iter().all(|&x| x & 0xC0 == 0x80)
}

fn is_test_char_data(c: u8) -> bool {
    c == 0x09 || ((0x20..0x80).contains(&c) && c != b'&' && c != b'<' && c != b']')
}

fn dummy_state() -> ParserState {
    ParserState {
        inputs: Vec::new(),
        options: 0,
        well_formed: true,
        ns_well_formed: true,
        valid: true,
        disable_sax: 0,
        errors: Vec::new(),
        nb_errors: 0,
        nb_warnings: 0,
        replace_entities: false,
        keep_blanks: true,
        recovery: false,
        in_subset: 0,
        standalone: -1,
        version: None,
        encoding: None,
        input_encoding: None,
        has_external_subset: false,
        has_perefs: false,
        html: false,
        stop: false,
        quiet: false,
        ids: HashMap::new(),
        err_no: 0,
        last_err_level: 0,
        sax1: false,
        sax_get_entity: false,
        directory: None,
    }
}

/// Normalize a non-CDATA attribute value: collapse runs of spaces and strip
/// leading/trailing spaces.
pub fn normalize_attr_value(s: &str) -> String {
    let mut out = String::new();
    let mut in_space = true;
    for c in s.chars() {
        if c == ' ' {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

// ---------------------------------------------------------------------------
// Push parser (xmlParseChunk / xmlParseTryOrFinish)

impl<'a, S: Sink> Parser<'a, S> {
    pub fn push_state(&self) -> PushState {
        self.push_state
    }

    /// Append raw bytes to the main input, converting them like the data
    /// already there.
    fn push_append(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        let conv = match &self.conv_enc {
            None => None,
            Some(enc) => {
                let mut raw = std::mem::take(&mut self.conv_pending);
                raw.extend_from_slice(chunk);
                let up = enc.to_ascii_uppercase();
                if up.starts_with("UTF-16") && raw.len() % 2 == 1 {
                    self.conv_pending.push(raw.pop().unwrap());
                }
                Some(decode_to_utf8(&raw, enc).unwrap_or(raw))
            }
        };
        let inp = &mut self.st.inputs[0];
        let d = Rc::make_mut(&mut inp.data);
        match conv {
            Some(c) => d.extend_from_slice(&c),
            None => d.extend_from_slice(chunk),
        }
    }

    fn avail(&self) -> usize {
        let i = self.inp();
        i.data.len().saturating_sub(i.pos)
    }

    /// xmlParseLookupChar
    fn lookup_char(&mut self, c: u8) -> bool {
        let i = self.inp();
        let start = i.pos
            + if self.check_index == 0 {
                1
            } else {
                self.check_index
            };
        let found = start < i.data.len() && i.data[start..].contains(&c);
        if found {
            self.check_index = 0;
        } else {
            self.check_index = i.data.len() - i.pos;
        }
        found
    }

    /// xmlParseLookupString
    fn lookup_string(&mut self, start_delta: usize, s: &[u8]) -> bool {
        let i = self.inp();
        let cur = (i.pos
            + if self.check_index == 0 {
                start_delta
            } else {
                self.check_index
            })
        .min(i.data.len());
        let hay = &i.data[cur..];
        let found = hay.len() >= s.len() && hay.windows(s.len()).any(|w| w == s);
        if found {
            self.check_index = 0;
        } else {
            let mut end = i.data.len();
            if end - cur < s.len() {
                end = cur;
            } else {
                end -= s.len() - 1;
            }
            self.check_index = end - i.pos;
        }
        found
    }

    /// xmlParseLookupCharData
    fn lookup_char_data(&mut self) -> bool {
        let i = self.inp();
        let start = (i.pos + self.check_index).min(i.data.len());
        if i.data[start..].iter().any(|&b| b == b'<' || b == b'&') {
            self.check_index = 0;
            true
        } else {
            self.check_index = i.data.len() - i.pos;
            false
        }
    }

    /// xmlParseLookupGt: a '>' outside quotes.
    fn lookup_gt(&mut self) -> bool {
        let i = self.inp();
        let d = &i.data;
        let mut cur = i.pos
            + if self.check_index == 0 {
                1
            } else {
                self.check_index
            };
        let mut state = self.end_check_state;
        while cur < d.len() {
            let c = d[cur];
            if state != 0 {
                if c == state {
                    state = 0;
                }
            } else if c == b'\'' || c == b'"' {
                state = c;
            } else if c == b'>' {
                self.check_index = 0;
                self.end_check_state = 0;
                return true;
            }
            cur += 1;
        }
        self.check_index = cur - i.pos;
        self.end_check_state = state;
        false
    }

    /// xmlParseLookupInternalSubset
    fn lookup_internal_subset(&mut self) -> bool {
        let i = self.inp();
        let d = &i.data;
        let at = |k: usize| -> u8 { if k < d.len() { d[k] } else { 0 } };
        let mut cur = i.pos
            + if self.check_index == 0 {
                1
            } else {
                self.check_index
            };
        let mut start = cur;
        let mut state = self.end_check_state;
        while cur < d.len() {
            let c = d[cur];
            if state == b'-' {
                if c == b'-' && at(cur + 1) == b'-' && at(cur + 2) == b'>' {
                    state = 0;
                    cur += 3;
                    start = cur;
                    continue;
                }
            } else if state == b']' {
                if c == b'>' {
                    self.check_index = 0;
                    self.end_check_state = 0;
                    return true;
                }
                if is_blank(c) {
                    state = b' ';
                } else if c != b']' {
                    state = 0;
                    start = cur;
                    continue;
                }
            } else if state == b' ' {
                if c == b'>' {
                    self.check_index = 0;
                    self.end_check_state = 0;
                    return true;
                }
                if !is_blank(c) {
                    state = 0;
                    start = cur;
                    continue;
                }
            } else if state != 0 {
                if c == state {
                    state = 0;
                    start = cur + 1;
                }
            } else if c == b'<' {
                if at(cur + 1) == b'!' && at(cur + 2) == b'-' && at(cur + 3) == b'-' {
                    state = b'-';
                    cur += 4;
                    start = cur;
                    continue;
                }
            } else if c == b'"' || c == b'\'' || c == b']' {
                state = c;
            }
            cur += 1;
        }
        if state == 0 || state == b'-' {
            if cur - start < 3 {
                cur = start;
            } else {
                cur -= 3;
            }
        }
        self.check_index = cur.saturating_sub(i.pos);
        self.end_check_state = state;
        false
    }

    /// xmlFinishDocument
    fn finish_document(&mut self) {
        let enc = self.st.encoding.clone();
        self.sink.end_document_p(&mut self.st, enc.as_deref());
    }

    /// xmlParseChunk: feed `chunk`; parse as far as complete constructs
    /// allow (everything when `terminate`). Returns an xmlParserErrors code
    /// (ctxt->errNo when the document is not well-formed, else 0).
    pub fn parse_chunk(&mut self, chunk: &[u8], terminate: bool) -> i32 {
        if self.st.disable_sax != 0 {
            return self.st.err_no;
        }
        let mut chunk = chunk;
        let mut end_in_cr = false;
        if !chunk.is_empty() && !terminate && chunk[chunk.len() - 1] == b'\r' {
            end_in_cr = true;
            chunk = &chunk[..chunk.len() - 1];
        }
        self.push_append(chunk);
        self.try_or_finish(terminate);
        let max = if self.st.options & XML_PARSE_HUGE != 0 {
            1_000_000_000
        } else {
            10_000_000
        };
        if self.st.inputs.len() == 1 && self.st.inputs[0].pos > max {
            self.fatal_msg(
                XML_ERR_RESOURCE_LIMIT,
                "Buffer size limit exceeded, try XML_PARSE_HUGE\n".into(),
            );
        }
        if self.st.err_no != 0 && self.st.disable_sax != 0 {
            return self.st.err_no;
        }
        if end_in_cr {
            self.push_append(b"\r");
        }
        if terminate {
            if self.push_state != PushState::Eof && self.push_state != PushState::Epilog {
                if let Some(n) = self.names.last() {
                    let msg = format!("Premature end of data in tag {} line {}\n", n.name, n.line);
                    self.fatal_msg(XML_ERR_TAG_NOT_FINISHED, msg);
                } else if self.push_state == PushState::Start {
                    self.fatal(XML_ERR_DOCUMENT_EMPTY, None);
                } else {
                    self.fatal_msg(
                        XML_ERR_DOCUMENT_EMPTY,
                        "Start tag expected, '<' not found\n".into(),
                    );
                }
            } else if self.st.err_no == 0 && self.avail() > 0 {
                self.fatal(XML_ERR_DOCUMENT_END, None);
            }
            if self.push_state != PushState::Eof {
                self.push_state = PushState::Eof;
                self.finish_document();
            }
        }
        if !self.st.well_formed {
            self.st.err_no
        } else {
            0
        }
    }

    /// xmlParseTryOrFinish
    fn try_or_finish(&mut self, terminate: bool) {
        if self.st.inputs.len() == 1 && self.st.inputs[0].pos > 4096 {
            self.st.inputs[0].shrink(80);
        }
        while self.st.disable_sax == 0 && !self.st.stop {
            let avail = self.avail();
            if avail < 1 {
                break;
            }
            match self.push_state {
                PushState::Eof => break,
                PushState::Start => {
                    if !terminate && avail < 4 {
                        break;
                    }
                    self.detect_encoding();
                    self.push_state = PushState::XmlDecl;
                }
                PushState::XmlDecl => {
                    if !terminate && avail < 2 {
                        break;
                    }
                    if self.cur() == b'<' && self.nxt(1) == b'?' {
                        if !terminate && !self.lookup_string(2, b"?>") {
                            break;
                        }
                        if self.cmp(b"<?xml") && is_blank(self.nxt(5)) {
                            self.parse_xml_decl();
                        } else {
                            self.st.version = Some("1.0".into());
                        }
                    } else {
                        self.st.version = Some("1.0".into());
                    }
                    if self.sax_ok() {
                        let v = self.st.version.clone().unwrap_or_else(|| "1.0".into());
                        let e = self.st.encoding.clone();
                        let sa = self.st.standalone;
                        self.sink.start_document(&v, e.as_deref(), sa);
                    }
                    self.push_state = PushState::Misc;
                }
                PushState::StartTag => {
                    if !terminate && avail < 2 {
                        break;
                    }
                    if self.cur() != b'<' {
                        self.fatal_msg(
                            XML_ERR_DOCUMENT_EMPTY,
                            "Start tag expected, '<' not found\n".into(),
                        );
                        self.push_state = PushState::Eof;
                        self.finish_document();
                        break;
                    }
                    if !terminate && !self.lookup_gt() {
                        break;
                    }
                    if !self.push_start_tag() {
                        self.push_state = PushState::Eof;
                        self.finish_document();
                        break;
                    }
                    self.push_state = if self.names.is_empty() {
                        PushState::Epilog
                    } else {
                        PushState::Content
                    };
                }
                PushState::Content => {
                    let c = self.cur();
                    if c == b'<' {
                        if !terminate && avail < 2 {
                            break;
                        }
                        let next = self.nxt(1);
                        if next == b'/' {
                            self.push_state = PushState::EndTag;
                            continue;
                        } else if next == b'?' {
                            if !terminate && !self.lookup_string(2, b"?>") {
                                break;
                            }
                            self.parse_pi();
                            continue;
                        } else if next == b'!' {
                            if !terminate && avail < 3 {
                                break;
                            }
                            let n2 = self.nxt(2);
                            if n2 == b'-' {
                                if !terminate && avail < 4 {
                                    break;
                                }
                                if self.nxt(3) == b'-' {
                                    if !terminate && !self.lookup_string(4, b"-->") {
                                        break;
                                    }
                                    self.parse_comment();
                                    continue;
                                }
                            } else if n2 == b'[' {
                                if !terminate && avail < 9 {
                                    break;
                                }
                                if self.cmp(b"<![CDATA[") {
                                    if !terminate && !self.lookup_string(9, b"]]>") {
                                        break;
                                    }
                                    self.parse_cdsect();
                                    continue;
                                }
                            }
                        }
                        self.push_state = PushState::StartTag;
                    } else if c == b'&' {
                        if !terminate && !self.lookup_char(b';') {
                            break;
                        }
                        self.parse_reference();
                    } else {
                        if avail < 300 && !terminate && !self.lookup_char_data() {
                            break;
                        }
                        self.check_index = 0;
                        self.partial = !terminate;
                        let before = self.pos();
                        self.parse_char_data();
                        self.partial = false;
                        if self.pos() == before {
                            // incomplete multi-byte character: wait for more
                            break;
                        }
                    }
                }
                PushState::EndTag => {
                    if !terminate && !self.lookup_char(b'>') {
                        break;
                    }
                    if self.st.sax1 {
                        self.parse_end_tag1(0);
                    } else {
                        self.parse_element_end();
                    }
                    self.push_state = if self.names.is_empty() {
                        PushState::Epilog
                    } else {
                        PushState::Content
                    };
                }
                PushState::Misc | PushState::Prolog | PushState::Epilog => {
                    self.skip_blanks();
                    let avail = self.avail();
                    if avail < 1 {
                        break;
                    }
                    if self.cur() == b'<' {
                        if !terminate && avail < 2 {
                            break;
                        }
                        let next = self.nxt(1);
                        if next == b'?' {
                            if !terminate && !self.lookup_string(2, b"?>") {
                                break;
                            }
                            self.parse_pi();
                            continue;
                        } else if next == b'!' {
                            if !terminate && avail < 3 {
                                break;
                            }
                            if self.nxt(2) == b'-' {
                                if !terminate && avail < 4 {
                                    break;
                                }
                                if self.nxt(3) == b'-' {
                                    if !terminate && !self.lookup_string(4, b"-->") {
                                        break;
                                    }
                                    self.parse_comment();
                                    continue;
                                }
                            } else if self.push_state == PushState::Misc {
                                if !terminate && avail < 9 {
                                    break;
                                }
                                if self.cmp(b"<!DOCTYPE") {
                                    if !terminate && !self.lookup_gt() {
                                        break;
                                    }
                                    self.st.in_subset = 1;
                                    self.parse_doctype_decl();
                                    if self.cur() == b'[' {
                                        self.push_state = PushState::Dtd;
                                    } else {
                                        if self.cur() == b'>' {
                                            self.skip(1);
                                        }
                                        self.end_doctype();
                                    }
                                    continue;
                                }
                            }
                        }
                    }
                    if self.push_state == PushState::Epilog {
                        if self.st.err_no == 0 {
                            self.fatal(XML_ERR_DOCUMENT_END, None);
                        }
                        self.push_state = PushState::Eof;
                        self.finish_document();
                    } else {
                        self.push_state = PushState::StartTag;
                    }
                }
                PushState::Dtd => {
                    if !terminate && !self.lookup_internal_subset() {
                        break;
                    }
                    self.parse_internal_subset();
                    self.end_doctype();
                }
            }
        }
    }

    fn end_doctype(&mut self) {
        self.st.in_subset = 2;
        if self.sax_ok() {
            self.load_external_subset();
        }
        self.st.in_subset = 0;
        self.clean_special_attr();
        self.push_state = PushState::Prolog;
    }

    /// The XML_PARSER_START_TAG step. Returns false when no element name
    /// could be parsed.
    fn push_start_tag(&mut self) -> bool {
        let sp = match self.space.last() {
            None | Some(-2) => -1,
            Some(v) => *v,
        };
        self.space.push(sp);
        let line = self.st.cur_line();
        let r = if self.st.sax1 {
            self.parse_start_tag1().map(|n| (n, None, None, 0, false))
        } else {
            self.parse_start_tag2()
        };
        let (name, prefix, uri, nb_ns, has_node) = match r {
            Some(x) => x,
            None => {
                self.space.pop();
                return false;
            }
        };
        if self.cur() == b'/' && self.nxt(1) == b'>' {
            self.skip(2);
            if self.sax_ok() {
                self.sink
                    .end_element(&mut self.st, &name, prefix.as_deref(), uri.as_deref());
            } else if has_node {
                self.sink
                    .end_element(&mut self.st, "\u{0}abort", None, None);
            }
            self.ns_pop(nb_ns);
            self.space.pop();
        } else if self.cur() == b'>' {
            self.skip(1);
            self.names.push(NameEntry {
                name,
                prefix,
                uri,
                line,
                ns_nr: nb_ns,
                has_node,
            });
        } else {
            self.fatal_msg(
                XML_ERR_GT_REQUIRED,
                format!("Couldn't find end of Start Tag {}\n", name),
            );
            if has_node {
                self.sink
                    .end_element(&mut self.st, "\u{0}abort", None, None);
            }
            self.space.pop();
            self.ns_pop(nb_ns);
        }
        true
    }

    /// xmlParseStartTag (SAX1): names as written, no namespace processing.
    fn parse_start_tag1(&mut self) -> Option<String> {
        if self.cur() != b'<' {
            return None;
        }
        self.skip(1);
        let name = match self.parse_name() {
            Some(n) => n,
            None => {
                self.fatal_msg(
                    XML_ERR_NAME_REQUIRED,
                    "xmlParseStartTag: invalid element name\n".into(),
                );
                return None;
            }
        };
        self.skip_blanks();
        let mut atts: Vec<SaxAttr> = Vec::new();
        loop {
            let c = self.cur();
            if c == b'>'
                || (c == b'/' && self.nxt(1) == b'>')
                || !(c >= 0x20 || c == 0x9 || c == 0xA || c == 0xD)
                || self.stopped()
            {
                break;
            }
            // xmlParseAttribute
            let attname = match self.parse_name() {
                Some(n) => n,
                None => {
                    self.fatal_msg(
                        XML_ERR_NAME_REQUIRED,
                        "error parsing attribute name\n".into(),
                    );
                    break;
                }
            };
            self.skip_blanks();
            let val = if self.cur() == b'=' {
                self.skip(1);
                self.skip_blanks();
                self.parse_att_value_internal(0, false)
            } else {
                self.fatal_msg(
                    XML_ERR_ATTRIBUTE_WITHOUT_VALUE,
                    format!("Specification mandates value for attribute {}\n", attname),
                );
                None
            };
            if let Some(v) = val {
                if attname == "xml:space" {
                    if v == "default" {
                        if let Some(s) = self.space.last_mut() {
                            *s = 0;
                        }
                    } else if v == "preserve" {
                        if let Some(s) = self.space.last_mut() {
                            *s = 1;
                        }
                    } else {
                        self.warning(XML_WAR_SPACE_VALUE, format!("Invalid value \"{}\" for xml:space : \"default\" or \"preserve\" expected\n", v));
                    }
                }
                if atts.iter().any(|a| a.localname == attname) {
                    self.fatal_msg(
                        XML_ERR_ATTRIBUTE_REDEFINED,
                        format!("Attribute {} redefined\n", attname),
                    );
                } else {
                    atts.push(SaxAttr {
                        localname: attname,
                        prefix: None,
                        uri: None,
                        value: v,
                        defaulted: false,
                    });
                }
            }
            if self.cur() == b'>' || (self.cur() == b'/' && self.nxt(1) == b'>') {
                break;
            }
            if self.skip_blanks() == 0 {
                self.fatal_msg(
                    XML_ERR_SPACE_REQUIRED,
                    "attributes construct error\n".into(),
                );
            }
        }
        if self.sax_ok() {
            let line = self.st.cur_line();
            self.sink
                .start_element(&mut self.st, &name, None, None, &[], &atts, 0, line);
        }
        Some(name)
    }

    /// xmlParseEndTag1 (SAX1)
    fn parse_end_tag1(&mut self, line: u32) {
        if !(self.cur() == b'<' && self.nxt(1) == b'/') {
            self.fatal_msg(
                XML_ERR_LTSLASH_REQUIRED,
                "xmlParseEndTag: '</' not found\n".into(),
            );
            return;
        }
        self.skip(2);
        let tname = self
            .names
            .last()
            .map(|n| n.name.clone())
            .unwrap_or_default();
        // xmlParseNameAndCompare
        let mut mismatch: Option<String> = None;
        let nb = self.nxt(tname.len());
        if self.cmp(tname.as_bytes()) && (nb == b'>' || is_blank(nb)) {
            self.skip(tname.len());
        } else {
            match self.parse_name() {
                Some(n) if n == tname => {}
                Some(n) => mismatch = Some(n),
                None => mismatch = Some("unparsable".into()),
            }
        }
        self.skip_blanks();
        if self.cur() != b'>' {
            self.fatal(XML_ERR_GT_REQUIRED, None);
        } else {
            self.skip(1);
        }
        if let Some(m) = mismatch {
            self.fatal_msg(
                XML_ERR_TAG_NAME_MISMATCH,
                format!(
                    "Opening and ending tag mismatch: {} line {} and {}\n",
                    tname, line, m
                ),
            );
        }
        if self.sax_ok() {
            self.sink.end_element(&mut self.st, &tname, None, None);
        }
        self.names.pop();
        self.space.pop();
    }
}

// ---------------------------------------------------------------------------
// External resources (external subset, external parameter and general
// entities)

pub const XML_IO_UNKNOWN: i32 = 1500;
pub const XML_IO_ENOENT: i32 = 1524;
pub const XML_IO_NETWORK_ATTEMPT: i32 = 1543;
pub const XML_ERR_EXT_SUBSET_NOT_FINISHED: i32 = 60;
pub const XML_ERR_CONDSEC_NOT_FINISHED: i32 = 59;
pub const XML_ERR_CONDSEC_INVALID: i32 = 83;
pub const XML_ERR_CONDSEC_INVALID_KEYWORD: i32 = 95;
pub const XML_ERR_XMLDECL_NOT_STARTED: i32 = 56;
pub const XML_ERR_ENTITY_PROCESSING: i32 = 104;

/// What an external resource load produced: the bytes and optionally the
/// final URL (used as the input's file name), or an error to report as
/// an I/O error (None: libxml2's "failed to load \"URL\": No such file or
/// directory").
pub type ExtLoadResult = Result<(Vec<u8>, Option<String>), Option<(i32, String)>>;

/// External entity loader hook (xmlExternalEntityLoader): called with the
/// public id, the system id as written and the URL resolved against the
/// current base.
pub type ExtLoader = Box<dyn FnMut(Option<&str>, &str, &str) -> ExtLoadResult>;

thread_local! {
    static EXT_LOADER: std::cell::RefCell<Option<ExtLoader>> = const { std::cell::RefCell::new(None) };
}

/// Install (or remove with None) the external entity loader used by the
/// parser; returns the previous one. Without a hook resources are read
/// from the file system.
pub fn set_ext_loader(l: Option<ExtLoader>) -> Option<ExtLoader> {
    EXT_LOADER.with(|c| std::mem::replace(&mut *c.borrow_mut(), l))
}

/// The default loader: local files (plain paths or file:// URLs).
pub fn default_ext_load(url: &str) -> ExtLoadResult {
    let path = if let Some(rest) = url.strip_prefix("file://") {
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        percent_decode_str(rest)
    } else if url.contains("://") {
        return Err(None);
    } else {
        url.to_string()
    };
    match std::fs::read(&path) {
        Ok(d) => Ok((d, None)),
        Err(_) => Err(None),
    }
}

fn percent_decode_str(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) =
                u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16)
            {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Load an external resource through the installed hook (or the default
/// loader).
pub fn load_external(public: Option<&str>, system: &str, url: &str) -> ExtLoadResult {
    let hook = EXT_LOADER.with(|c| c.borrow_mut().take());
    match hook {
        Some(mut h) => {
            let r = h(public, system, url);
            EXT_LOADER.with(|c| {
                let mut b = c.borrow_mut();
                if b.is_none() {
                    *b = Some(h);
                }
            });
            r
        }
        None => default_ext_load(url),
    }
}

/// Convert the raw bytes of an external entity to UTF-8 when a BOM says
/// so (the text declaration's encoding is applied while parsing it).
pub fn prepare_external(data: Vec<u8>) -> (Vec<u8>, bool) {
    if data.len() >= 3 && data[0] == 0xEF && data[1] == 0xBB && data[2] == 0xBF {
        return (data[3..].to_vec(), true);
    }
    if data.len() >= 2
        && ((data[0] == 0xFF && data[1] == 0xFE) || (data[0] == 0xFE && data[1] == 0xFF))
    {
        let enc = if data[0] == 0xFF {
            "UTF-16LE"
        } else {
            "UTF-16BE"
        };
        return (decode_to_utf8(&data[2..], enc).unwrap_or_default(), true);
    }
    if data.len() >= 4 && data[0] == b'<' && data[1] == 0 && data[2] == b'?' && data[3] == 0 {
        return (decode_to_utf8(&data, "UTF-16LE").unwrap_or_default(), true);
    }
    if data.len() >= 4 && data[0] == 0 && data[1] == b'<' && data[2] == 0 && data[3] == b'?' {
        return (decode_to_utf8(&data, "UTF-16BE").unwrap_or_default(), true);
    }
    (data, false)
}

impl<'a, S: Sink> Parser<'a, S> {
    /// ctxt->validate || ctxt->loadsubset, and external entities allowed.
    fn loads_dtd(&self) -> bool {
        self.st.options & XML_PARSE_NO_XXE == 0
            && self.st.options & (XML_PARSE_DTDVALID | XML_PARSE_DTDLOAD | XML_PARSE_DTDATTR) != 0
    }

    /// Base URI: the current input's file name (or, with `nearest`, the
    /// innermost input having one), else ctxt->directory.
    fn resolve_base(&self, nearest: bool) -> Option<String> {
        if nearest {
            for i in self.st.inputs.iter().rev() {
                if let Some(f) = &i.filename {
                    return Some(f.clone());
                }
            }
        } else if let Some(f) = self.st.inputs.last().and_then(|i| i.filename.clone()) {
            return Some(f);
        }
        self.st.directory.clone()
    }

    fn resolve_uri(&self, system: &str, nearest: bool) -> String {
        let base = self.resolve_base(nearest);
        match base {
            Some(b) => {
                crate::uri::build_uri(system, Some(&b)).unwrap_or_else(|| system.to_string())
            }
            None => system.to_string(),
        }
    }

    /// xmlCtxtErrIO for a failed external load.
    fn io_load_error(&mut self, url: &str, e: Option<(i32, String)>) {
        let (code, msg) = e.unwrap_or_else(|| {
            (
                XML_IO_ENOENT,
                format!("failed to load \"{}\": No such file or directory\n", url),
            )
        });
        let level =
            if code == XML_IO_ENOENT || code == XML_IO_NETWORK_ATTEMPT || code == XML_IO_UNKNOWN {
                if self.st.options & XML_PARSE_DTDVALID != 0 {
                    ERR_ERROR
                } else {
                    ERR_WARNING
                }
            } else {
                ERR_FATAL
            };
        self.err_full(FROM_IO, code, level, msg);
    }

    /// Load an external entity's bytes, reporting failures.
    fn fetch_external(
        &mut self,
        public: Option<&str>,
        system: &str,
        url: &str,
    ) -> Option<(Vec<u8>, String)> {
        match load_external(public, system, url) {
            Ok((data, final_url)) => {
                let (data, _) = prepare_external(data);
                Some((data, final_url.unwrap_or_else(|| url.to_string())))
            }
            Err(e) => {
                self.io_load_error(url, e);
                None
            }
        }
    }

    /// xmlParseTextDecl, on the current (external) input. The declared
    /// encoding converts the rest of that input.
    fn parse_text_decl(&mut self) {
        if !(self.cmp(b"<?xml") && is_blank(self.nxt(5))) {
            self.fatal(XML_ERR_XMLDECL_NOT_STARTED, None);
            return;
        }
        let saved = (
            self.st.encoding.take(),
            self.st.input_encoding.take(),
            self.conv_enc.take(),
            self.st.version.clone(),
        );
        self.skip(5);
        if self.skip_blanks() == 0 {
            self.fatal_msg(
                XML_ERR_SPACE_REQUIRED,
                "Space needed after '<?xml'\n".into(),
            );
        }
        if self.parse_version_info().is_some() && self.skip_blanks() == 0 {
            self.fatal_msg(XML_ERR_SPACE_REQUIRED, "Space needed here\n".into());
        }
        self.parse_encoding_decl();
        self.skip_blanks();
        if self.cur() == b'?' && self.nxt(1) == b'>' {
            self.skip(2);
        } else if self.cur() == b'>' {
            self.fatal(XML_ERR_XMLDECL_NOT_FINISHED, None);
            self.skip(1);
        } else {
            self.fatal(XML_ERR_XMLDECL_NOT_FINISHED, None);
            while !self.stopped() && !self.at_end() {
                let c = self.cur();
                self.skip(1);
                if c == b'>' {
                    break;
                }
            }
        }
        self.st.encoding = saved.0;
        self.st.input_encoding = saved.1;
        self.conv_enc = saved.2;
        self.st.version = saved.3;
    }

    /// xmlSAX2ExternalSubset: fetch and parse the external subset when
    /// validating or loading the DTD.
    fn load_external_subset(&mut self) {
        let sys = match self.ext_sub_uri.clone() {
            Some(s) => s,
            None => return,
        };
        if !self.sink.wants_external_subset() || !self.loads_dtd() || !self.st.well_formed {
            return;
        }
        let public = self.ext_sub_system.clone();
        let name = self.int_sub_name.clone();
        let url = self.resolve_uri(&sys, false);
        let (data, fname) = match self.fetch_external(public.as_deref(), &sys, &url) {
            Some(x) => x,
            None => return,
        };
        self.sink
            .external_subset(name.as_deref(), public.as_deref(), Some(&sys));
        let saved = std::mem::take(&mut self.st.inputs);
        self.st.inputs.push(Input::new(Rc::new(data), Some(fname)));
        let old_sub = self.st.in_subset;
        self.st.in_subset = 2;
        self.parse_external_subset();
        self.st.in_subset = old_sub;
        self.st.inputs = saved;
    }

    /// xmlParseExternalSubset on the current input.
    pub fn parse_external_subset(&mut self) {
        if self.cmp(b"<?xml") {
            self.parse_text_decl();
        }
        self.st.in_subset = 2;
        let old_nr = self.st.inputs.len();
        self.skip_blanks_pe();
        while (self.cur() != 0 || self.st.inputs.len() > old_nr) && !self.stopped() {
            if self.at_end() && self.st.inputs.len() > old_nr {
                self.st.inputs.pop();
                self.skip_blanks_pe();
                continue;
            }
            if self.cmp(b"<![") {
                self.parse_conditional_sections();
            } else if self.cur() == b'<' && (self.nxt(1) == b'!' || self.nxt(1) == b'?') {
                self.parse_markup_decl();
            } else {
                self.fatal(XML_ERR_EXT_SUBSET_NOT_FINISHED, None);
                self.st.disable_sax = 2;
                return;
            }
            self.skip_blanks_pe();
        }
        while self.st.inputs.len() > old_nr {
            self.st.inputs.pop();
        }
        if self.cur() != 0 {
            self.fatal(XML_ERR_EXT_SUBSET_NOT_FINISHED, None);
        }
    }

    /// xmlParseConditionalSections
    fn parse_conditional_sections(&mut self) {
        let mut depth = 0usize;
        while !self.stopped() {
            if self.cmp(b"<![") {
                self.skip(3);
                self.skip_blanks_pe();
                if self.cmp(b"INCLUDE") {
                    self.skip(7);
                    self.skip_blanks_pe();
                    if self.cur() != b'[' {
                        self.fatal(XML_ERR_CONDSEC_INVALID, None);
                        self.st.disable_sax = 2;
                        return;
                    }
                    self.skip(1);
                    depth += 1;
                } else if self.cmp(b"IGNORE") {
                    self.skip(6);
                    self.skip_blanks_pe();
                    if self.cur() != b'[' {
                        self.fatal(XML_ERR_CONDSEC_INVALID, None);
                        self.st.disable_sax = 2;
                        return;
                    }
                    self.skip(1);
                    let mut ignore_depth = 0usize;
                    while !self.stopped() {
                        if self.at_end() {
                            self.fatal(XML_ERR_CONDSEC_NOT_FINISHED, None);
                            return;
                        }
                        if self.cmp(b"<![") {
                            self.skip(3);
                            ignore_depth += 1;
                        } else if self.cmp(b"]]>") {
                            self.skip(3);
                            if ignore_depth == 0 {
                                break;
                            }
                            ignore_depth -= 1;
                        } else {
                            self.skip(1);
                        }
                    }
                } else {
                    self.fatal(XML_ERR_CONDSEC_INVALID_KEYWORD, None);
                    self.st.disable_sax = 2;
                    return;
                }
            } else if depth > 0 && self.cmp(b"]]>") {
                depth -= 1;
                self.skip(3);
            } else if self.cur() == b'<' && (self.nxt(1) == b'!' || self.nxt(1) == b'?') {
                self.parse_markup_decl();
            } else {
                self.fatal(XML_ERR_EXT_SUBSET_NOT_FINISHED, None);
                self.st.disable_sax = 2;
                return;
            }
            if depth == 0 {
                break;
            }
            self.skip_blanks_pe();
        }
    }

    /// Push an external parameter entity's content as a new input
    /// (xmlParsePEReference for XML_EXTERNAL_PARAMETER_ENTITY).
    fn push_external_pe(&mut self, e: &PEntity) {
        if self.st.options & XML_PARSE_NO_XXE != 0
            || (!self.loads_dtd()
                && !self.st.replace_entities
                && self.st.options & XML_PARSE_DTDVALID == 0)
        {
            return;
        }
        let sys = match &e.system {
            Some(s) => s.clone(),
            None => return,
        };
        let url = e.uri.clone().unwrap_or_else(|| sys.clone());
        if let Some((data, fname)) = self.fetch_external(e.public.as_deref(), &sys, &url) {
            let mut inp = Input::new(Rc::new(data), Some(fname));
            inp.entity = Some(e.name.clone());
            self.st.inputs.push(inp);
            if self.cmp(b"<?xml") && is_blank(self.nxt(5)) {
                self.parse_text_decl();
            }
        }
    }

    /// xmlLoadEntityContent: the replacement text of an external entity
    /// (text declaration removed). None if it could not be loaded.
    fn load_entity_content(&mut self, e: &PEntity) -> Option<(String, String)> {
        let sys = e.system.clone()?;
        let url = e.uri.clone().unwrap_or_else(|| sys.clone());
        let (data, fname) = self.fetch_external(e.public.as_deref(), &sys, &url)?;
        let mut inp = Input::new(Rc::new(data), Some(fname.clone()));
        inp.entity = Some(e.name.clone());
        self.st.inputs.push(inp);
        if self.cmp(b"<?xml") && is_blank(self.nxt(5)) {
            self.parse_text_decl();
        }
        let pos = self.pos();
        let rest = String::from_utf8_lossy(&self.inp().data[pos..]).into_owned();
        self.st.inputs.pop();
        Some((rest, fname))
    }
}
