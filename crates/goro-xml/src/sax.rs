//! The event API behind ext/xml (`xml_parser_create`, `xml_parse`,
//! `xml_parse_into_struct`), with the behaviour of PHP's libxml-based
//! implementation (`XML_SAX_IMPL === 'libxml'`), observed on PHP 8.5
//! (fixtures `tests/fixtures/sax.php`):
//!
//! - element and attribute names are upper-cased when case folding is on
//!   (the default); `XML_OPTION_SKIP_TAGSTART` drops leading bytes;
//! - with `xml_parser_create_ns`, namespaced names are `URI` + separator +
//!   local name, and namespace declarations are reported before the start
//!   tag;
//! - character data is split at references and carriage returns; internal
//!   entity references go to the default handler when there is one, or
//!   else their replacement text goes, unparsed, to the character data
//!   handler; external ones go to the external entity reference handler;
//! - comments, and markup without its own handler, go to the default
//!   handler as written; the XML and document type declarations do not;
//! - output is converted to the target encoding (UTF-8, ISO-8859-1 or
//!   US-ASCII, with `?` for what does not fit);
//! - `xml_get_error_code()` is the code of the last error; the codes are
//!   libxml's, and [`error_string`] gives the texts `xml_error_string()`
//!   prints.

use crate::error::{LEVEL_ERROR, XmlError};
use crate::tree::{DtdData, DtdItem, EntityDecl, EntityKind, QName};
use crate::xml::{ParseOptions, Parser, Sink, StartTag, Status, opt};

/// Target encodings ext/xml supports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TargetEncoding {
    /// UTF-8 (default).
    #[default]
    Utf8,
    /// ISO-8859-1.
    Latin1,
    /// US-ASCII.
    Ascii,
}

impl TargetEncoding {
    /// Parses an encoding name as `xml_parser_create()` and
    /// `XML_OPTION_TARGET_ENCODING` accept it (case-insensitive).
    pub fn from_name(name: &str) -> Option<TargetEncoding> {
        match name.to_ascii_uppercase().as_str() {
            "UTF-8" => Some(TargetEncoding::Utf8),
            "ISO-8859-1" => Some(TargetEncoding::Latin1),
            "US-ASCII" => Some(TargetEncoding::Ascii),
            _ => None,
        }
    }
    /// The canonical name (`xml_parser_get_option(XML_OPTION_TARGET_ENCODING)`).
    pub fn name(self) -> &'static str {
        match self {
            TargetEncoding::Utf8 => "UTF-8",
            TargetEncoding::Latin1 => "ISO-8859-1",
            TargetEncoding::Ascii => "US-ASCII",
        }
    }
    /// Converts UTF-8 text.
    pub fn convert(self, s: &str) -> Vec<u8> {
        match self {
            TargetEncoding::Utf8 => s.as_bytes().to_vec(),
            TargetEncoding::Latin1 => s
                .chars()
                .map(|c| if (c as u32) < 0x100 { c as u8 } else { b'?' })
                .collect(),
            TargetEncoding::Ascii => s
                .chars()
                .map(|c| if c.is_ascii() { c as u8 } else { b'?' })
                .collect(),
        }
    }
}

/// `xml_parser_set_option` settings.
#[derive(Clone, Debug)]
pub struct SaxOptions {
    /// XML_OPTION_CASE_FOLDING (default on).
    pub case_folding: bool,
    /// XML_OPTION_TARGET_ENCODING.
    pub target: TargetEncoding,
    /// XML_OPTION_SKIP_TAGSTART.
    pub skip_tagstart: usize,
    /// XML_OPTION_SKIP_WHITE (used by `xml_parse_into_struct`).
    pub skip_white: bool,
    /// XML_OPTION_PARSE_HUGE.
    pub parse_huge: bool,
}

impl Default for SaxOptions {
    fn default() -> Self {
        SaxOptions {
            case_folding: true,
            target: TargetEncoding::Utf8,
            skip_tagstart: 0,
            skip_white: false,
            parse_huge: false,
        }
    }
}

/// Which handlers are set (they change how events are routed).
#[derive(Clone, Copy, Debug, Default)]
pub struct Handlers {
    /// Start/end element handlers.
    pub element: bool,
    /// Character data handler.
    pub cdata: bool,
    /// Processing instruction handler.
    pub pi: bool,
    /// Default handler.
    pub default: bool,
    /// External entity reference handler.
    pub external_entity: bool,
    /// Unparsed entity declaration handler.
    pub unparsed_entity: bool,
    /// Notation declaration handler.
    pub notation: bool,
    /// Start/end namespace declaration handlers.
    pub namespace: bool,
}

/// The parser position (`xml_get_current_byte_index`,
/// `xml_get_current_line_number`, `xml_get_current_column_number`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Position {
    /// 0-based byte offset.
    pub byte_index: usize,
    /// 1-based line.
    pub line: u32,
    /// 1-based column.
    pub column: u32,
}

/// Receives ext/xml events. Strings are in the target encoding.
#[allow(unused_variables)]
pub trait SaxHandler {
    /// `xml_set_element_handler` start.
    fn start_element(&mut self, name: &[u8], attrs: &[(Vec<u8>, Vec<u8>)], pos: &Position) {}
    /// `xml_set_element_handler` end.
    fn end_element(&mut self, name: &[u8], pos: &Position) {}
    /// `xml_set_character_data_handler`.
    fn character_data(&mut self, data: &[u8], pos: &Position) {}
    /// `xml_set_processing_instruction_handler`.
    fn processing_instruction(&mut self, target: &[u8], data: &[u8], pos: &Position) {}
    /// `xml_set_default_handler`.
    fn default(&mut self, data: &[u8], pos: &Position) {}
    /// `xml_set_unparsed_entity_decl_handler` (base is always unset).
    fn unparsed_entity_decl(
        &mut self,
        name: &[u8],
        system: &[u8],
        public: Option<&[u8]>,
        notation: &[u8],
    ) {
    }
    /// `xml_set_notation_decl_handler` (base is always unset).
    fn notation_decl(&mut self, name: &[u8], system: Option<&[u8]>, public: Option<&[u8]>) {}
    /// `xml_set_external_entity_ref_handler`: open entity names, base
    /// (empty), system ID, public ID.
    fn external_entity_ref(
        &mut self,
        names: &[u8],
        system: &[u8],
        public: Option<&[u8]>,
        pos: &Position,
    ) -> bool {
        true
    }
    /// `xml_set_start_namespace_decl_handler` (`None` prefix: default namespace).
    fn start_namespace_decl(&mut self, prefix: Option<&[u8]>, uri: &[u8]) {}
    /// `xml_set_end_namespace_decl_handler`.
    fn end_namespace_decl(&mut self, prefix: Option<&[u8]>) {}
}

/// The texts of `xml_error_string()` for libxml error codes (observed on
/// PHP 8.5 with libxml 2.15); other codes print "Unknown".
const ERROR_STRINGS: [&str; 102] = [
    "No error",
    "No memory",
    "Invalid document start",
    "Empty document",
    "Not well-formed (invalid token)",
    "Invalid document end",
    "Invalid hexadecimal character reference",
    "Invalid decimal character reference",
    "Invalid character reference",
    "Invalid character",
    "XML_ERR_CHARREF_AT_EOF",
    "XML_ERR_CHARREF_IN_PROLOG",
    "XML_ERR_CHARREF_IN_EPILOG",
    "XML_ERR_CHARREF_IN_DTD",
    "XML_ERR_ENTITYREF_AT_EOF",
    "XML_ERR_ENTITYREF_IN_PROLOG",
    "XML_ERR_ENTITYREF_IN_EPILOG",
    "XML_ERR_ENTITYREF_IN_DTD",
    "PEReference at end of document",
    "PEReference in prolog",
    "PEReference in epilog",
    "PEReference: forbidden within markup decl in internal subset",
    "XML_ERR_ENTITYREF_NO_NAME",
    "EntityRef: expecting ';'",
    "PEReference: no name",
    "PEReference: expecting ';'",
    "Undeclared entity error",
    "Undeclared entity warning",
    "Unparsed Entity",
    "XML_ERR_ENTITY_IS_EXTERNAL",
    "XML_ERR_ENTITY_IS_PARAMETER",
    "Unknown encoding",
    "Unsupported encoding",
    "String not started expecting ' or \"",
    "String not closed expecting \" or '",
    "Namespace declaration error",
    "EntityValue: \" or ' expected",
    "EntityValue: \" or ' expected",
    "< in attribute",
    "Attribute not started",
    "Attribute not finished",
    "Attribute without value",
    "Attribute redefined",
    "SystemLiteral \" or ' expected",
    "SystemLiteral \" or ' expected",
    "Comment not finished",
    "Processing Instruction not started",
    "Processing Instruction not finished",
    "NOTATION: Name expected here",
    "'>' required to close NOTATION declaration",
    "'(' required to start ATTLIST enumeration",
    "'(' required to start ATTLIST enumeration",
    "MixedContentDecl : '|' or ')*' expected",
    "XML_ERR_MIXED_NOT_FINISHED",
    "ELEMENT in DTD not started",
    "ELEMENT in DTD not finished",
    "XML declaration not started",
    "XML declaration not finished",
    "XML_ERR_CONDSEC_NOT_STARTED",
    "XML conditional section not closed",
    "Content error in the external subset",
    "DOCTYPE not finished",
    "Sequence ']]>' not allowed in content",
    "CDATA not finished",
    "Reserved XML Name",
    "Space required",
    "XML_ERR_SEPARATOR_REQUIRED",
    "NmToken expected in ATTLIST enumeration",
    "XML_ERR_NAME_REQUIRED",
    "MixedContentDecl : '#PCDATA' expected",
    "SYSTEM or PUBLIC, the URI is missing",
    "PUBLIC, the Public Identifier is missing",
    "< required",
    "> required",
    "</ required",
    "= required",
    "Mismatched tag",
    "Tag not finished",
    "standalone accepts only 'yes' or 'no'",
    "Invalid XML encoding name",
    "Comment must not contain '--' (double-hyphen)",
    "Invalid encoding",
    "external parsed entities cannot be standalone",
    "XML conditional section '[' expected",
    "Entity value required",
    "chunk is not well balanced",
    "extra content at the end of well balanced chunk",
    "XML_ERR_ENTITY_CHAR_ERROR",
    "PEReferences forbidden in internal subset",
    "Detected an entity reference loop",
    "XML_ERR_ENTITY_BOUNDARY",
    "Invalid URI",
    "Fragment not allowed",
    "XML_WAR_CATALOG_PI",
    "XML_ERR_NO_DTD",
    "conditional section INCLUDE or IGNORE keyword expected",
    "Version in XML Declaration missing",
    "XML_WAR_UNKNOWN_VERSION",
    "XML_WAR_LANG_VALUE",
    "XML_WAR_NS_URI",
    "XML_WAR_NS_URI_RELATIVE",
    "Missing encoding in text declaration",
];

/// `xml_error_string()`: the text for an error code.
pub fn error_string(code: i64) -> &'static str {
    if (0..ERROR_STRINGS.len() as i64).contains(&code) {
        ERROR_STRINGS[code as usize]
    } else {
        "Unknown"
    }
}

/// An ext/xml parser.
pub struct SaxParser {
    /// Options (may change between `parse` calls).
    pub options: SaxOptions,
    ns_separator: Option<String>,
    parser: Option<Parser<'static>>,
    /// The code of the last error (`xml_get_error_code`), 0 if none.
    pub error_code: u32,
    /// The current position.
    pub position: Position,
    failed: bool,
    done: bool,
    ns_stack: Vec<Vec<Option<String>>>,
    /// All errors seen.
    pub errors: Vec<XmlError>,
}

struct Adaptor<'a> {
    opts: &'a SaxOptions,
    sep: Option<&'a str>,
    handlers: Handlers,
    h: &'a mut dyn SaxHandler,
    pos: Position,
    last_error: Option<XmlError>,
    failed: bool,
    raw: String,
    ns_stack: &'a mut Vec<Vec<Option<String>>>,
    bom: usize,
    fatal: bool,
}

impl Adaptor<'_> {
    fn out(&self, s: &str) -> Vec<u8> {
        self.opts.target.convert(s)
    }
    fn name(&self, q: &QName) -> Vec<u8> {
        let mut s = match (self.sep, &q.ns) {
            (Some(sep), Some(ns)) => format!("{ns}{sep}{}", q.local),
            _ => q.qualified(),
        };
        if self.opts.case_folding {
            s.make_ascii_uppercase();
        }
        let mut v = self.out(&s);
        if self.opts.skip_tagstart > 0 {
            let n = self.opts.skip_tagstart.min(v.len());
            v.drain(..n);
        }
        v
    }
    fn attr_name(&self, q: &QName) -> Vec<u8> {
        let mut s = match (self.sep, &q.ns) {
            (Some(sep), Some(ns)) => format!("{ns}{sep}{}", q.local),
            _ => q.qualified(),
        };
        if self.opts.case_folding {
            s.make_ascii_uppercase();
        }
        self.out(&s)
    }
    fn default_raw(&mut self) {
        if self.handlers.default && !self.raw.is_empty() {
            let r = std::mem::take(&mut self.raw);
            let b = self.out(&r);
            self.h.default(&b, &self.pos);
        }
        self.raw.clear();
    }
}

impl Sink for Adaptor<'_> {
    fn doctype(&mut self, dtd: &DtdData, _external: Option<&DtdData>) {
        for it in &dtd.items {
            match it {
                DtdItem::Notation(n) if self.handlers.notation => {
                    if let Some(nt) = dtd.notations.get(n) {
                        let name = self.out(&nt.name);
                        let sys = nt.system_id.as_deref().map(|s| self.out(s));
                        let public = nt.public_id.as_deref().map(|s| self.out(s));
                        self.h
                            .notation_decl(&name, sys.as_deref(), public.as_deref());
                    }
                }
                DtdItem::Entity(n, false) if self.handlers.unparsed_entity => {
                    if let Some(e) = dtd.entities.get(n)
                        && e.kind == EntityKind::ExternalGeneralUnparsed
                    {
                        let name = self.out(&e.name);
                        let sys = self.out(e.system_id.as_deref().unwrap_or(""));
                        let public = e.public_id.as_deref().map(|s| self.out(s));
                        let notation = self.out(e.notation.as_deref().unwrap_or(""));
                        self.h
                            .unparsed_entity_decl(&name, &sys, public.as_deref(), &notation);
                    }
                }
                _ => {}
            }
        }
    }

    fn position(&mut self, offset: usize, line: u32, column: u32) {
        let _ = self.bom;
        self.pos = Position {
            byte_index: offset,
            line,
            column,
        };
    }

    fn raw(&mut self, text: &str) {
        self.raw.clear();
        self.raw.push_str(text);
    }

    fn start_element(&mut self, tag: &StartTag<'_>) {
        let mut prefixes = Vec::new();
        if self.sep.is_some() {
            for d in tag.ns_decls {
                prefixes.push(d.prefix.as_deref().map(str::to_string));
                if self.handlers.namespace {
                    let p = d.prefix.as_deref().map(|p| self.out(p));
                    let u = self.out(&d.uri);
                    self.h.start_namespace_decl(p.as_deref(), &u);
                }
            }
        }
        self.ns_stack.push(prefixes);
        if self.handlers.element {
            self.raw.clear();
            let name = self.name(tag.name);
            let mut attrs: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(tag.attrs.len());
            if self.sep.is_none() {
                // Without namespace processing xmlns attributes are
                // ordinary attributes.
                for d in tag.ns_decls {
                    let n = match &d.prefix {
                        Some(p) => format!("xmlns:{p}"),
                        None => "xmlns".to_string(),
                    };
                    let mut n = n;
                    if self.opts.case_folding {
                        n.make_ascii_uppercase();
                    }
                    attrs.push((self.out(&n), self.out(&d.uri)));
                }
            }
            for a in tag.attrs {
                attrs.push((self.attr_name(&a.name), self.out(&a.value)));
            }
            self.h.start_element(&name, &attrs, &self.pos);
        } else {
            self.default_raw();
        }
    }

    fn end_element(&mut self, name: &QName) {
        if self.handlers.element {
            self.raw.clear();
            let n = self.name(name);
            self.h.end_element(&n, &self.pos);
        } else {
            self.default_raw();
        }
        // PHP's libxml-based parser never calls the end namespace
        // declaration handler (observed on PHP 8.5).
        self.ns_stack.pop();
    }

    fn text(&mut self, text: &str) {
        let b = self.out(text);
        if self.handlers.cdata {
            self.h.character_data(&b, &self.pos);
        } else if self.handlers.default {
            self.h.default(&b, &self.pos);
        }
    }

    fn ignorable_whitespace(&mut self, text: &str) {
        self.text(text);
    }

    fn cdata(&mut self, text: &str) {
        self.text(text);
    }

    fn comment(&mut self, _text: &str) {
        self.default_raw();
    }

    fn pi(&mut self, target: &str, data: &str) {
        if self.handlers.pi {
            self.raw.clear();
            let t = self.out(target);
            let d = self.out(data);
            self.h.processing_instruction(&t, &d, &self.pos);
        } else {
            self.default_raw();
        }
    }

    fn reference(&mut self, raw: &str, value: &str) {
        if self.handlers.cdata {
            let b = self.out(value);
            self.h.character_data(&b, &self.pos);
        } else if self.handlers.default {
            // Character references give their character, predefined
            // entities their reference (observed on PHP 8.5).
            let b = if raw.starts_with("&#") {
                self.out(value)
            } else {
                self.out(raw)
            };
            self.h.default(&b, &self.pos);
        }
    }

    fn undefined_entity(&mut self, name: &str) {
        if self.handlers.default {
            let b = self.out(&format!("&{name};"));
            self.h.default(&b, &self.pos);
        }
    }

    fn entity_start(&mut self, name: &str, decl: Option<&EntityDecl>) -> bool {
        let Some(d) = decl else {
            return false;
        };
        match d.kind {
            EntityKind::ExternalGeneralParsed => {
                if self.handlers.external_entity {
                    let n = self.out(name);
                    let s = self.out(d.system_id.as_deref().unwrap_or(""));
                    let p = d.public_id.as_deref().map(|p| self.out(p));
                    let ok = self.h.external_entity_ref(&n, &s, p.as_deref(), &self.pos);
                    if !ok {
                        self.failed = true;
                    }
                } else if self.handlers.default {
                    let b = self.out(&format!("&{name};"));
                    self.h.default(&b, &self.pos);
                }
            }
            _ => {
                if self.handlers.default {
                    let b = self.out(&format!("&{name};"));
                    self.h.default(&b, &self.pos);
                } else if self.handlers.cdata {
                    let b = self.out(d.value.as_deref().unwrap_or(""));
                    self.h.character_data(&b, &self.pos);
                }
            }
        }
        false
    }

    fn error(&mut self, e: &XmlError) {
        if e.level >= LEVEL_ERROR {
            self.failed = true;
            self.last_error = Some(e.clone());
        }
        if e.level >= crate::error::LEVEL_FATAL {
            self.fatal = true;
        }
    }
}

impl SaxParser {
    /// `xml_parser_create($encoding)` / `xml_parser_create_ns($encoding,
    /// $separator)`. The source encoding is detected from the document
    /// (PHP passes it to libxml only as a hint; observed on PHP 8.5 that
    /// ISO-8859-1 bytes without a declaration still fail as UTF-8).
    pub fn new(ns_separator: Option<&str>, target: Option<TargetEncoding>) -> SaxParser {
        SaxParser {
            options: SaxOptions {
                target: target.unwrap_or_default(),
                ..Default::default()
            },
            ns_separator: ns_separator.map(str::to_string),
            parser: None,
            error_code: 0,
            position: Position {
                byte_index: 0,
                line: 1,
                column: 1,
            },
            failed: false,
            done: false,
            ns_stack: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// True for a namespace-aware parser.
    pub fn is_ns(&self) -> bool {
        self.ns_separator.is_some()
    }

    /// `xml_parse($parser, $data, $is_final)`. Returns false after an
    /// error.
    pub fn parse(
        &mut self,
        data: &[u8],
        is_final: bool,
        handlers: Handlers,
        h: &mut dyn SaxHandler,
    ) -> bool {
        if self.failed {
            return false;
        }
        if self.done {
            // Data after the end: libxml reports extra content.
            if data.iter().all(|b| crate::chars::is_space(*b)) {
                return true;
            }
            self.error_code = 5;
            self.failed = true;
            return false;
        }
        if self.parser.is_none() {
            let mut po = ParseOptions {
                sax: true,
                split_text_at_refs: true,
                namespaces: self.ns_separator.is_some(),
                ..Default::default()
            };
            if self.options.parse_huge {
                po.flags |= opt::PARSEHUGE;
            }
            self.parser = Some(Parser::new(po, None));
        }
        let parser = self.parser.as_mut().unwrap();
        let bom = parser.bom_len;
        let mut ad = Adaptor {
            opts: &self.options,
            sep: self.ns_separator.as_deref(),
            handlers,
            h,
            pos: self.position,
            last_error: None,
            failed: false,
            raw: String::new(),
            ns_stack: &mut self.ns_stack,
            bom,
            fatal: false,
        };
        let status = parser.feed(data, is_final, &mut ad);
        let failed = ad.failed || status == Status::Failed;
        let fatal = ad.fatal || status == Status::Failed;
        let last = ad.last_error.take();
        let pos = ad.pos;
        drop(ad);
        if let Some(e) = &last {
            self.error_code = e.code;
            self.errors.push(e.clone());
        }
        let parser = self.parser.as_mut().unwrap();
        let _ = pos;
        if failed && fatal {
            self.failed = true;
            if let Some(e) = last {
                let mut off = parser.offset_of(e.line, e.column);
                let mut column = e.column;
                if e.code == 81 {
                    // After an encoding error the position is past the
                    // bad byte (observed on PHP 8.5).
                    off += 1;
                    column += 1;
                }
                self.position = Position {
                    byte_index: off + parser.bom_len,
                    line: e.line,
                    column,
                };
            }
            if self.error_code == 0 {
                self.error_code = 1;
            }
            return false;
        }
        let (off, line, column) = parser.sax_position();
        self.position = Position {
            byte_index: off,
            line,
            column,
        };
        if failed {
            self.failed = true;
            return false;
        }
        if status == Status::Done {
            self.done = true;
        }
        true
    }
}

/// One entry of `xml_parse_into_struct()`'s values array.
///
/// Key order in PHP: `tag, type, level, attributes, value` for open,
/// complete and close entries; `tag, value, type, level` for cdata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructEntry {
    /// The tag name.
    pub tag: Vec<u8>,
    /// "open", "complete", "close" or "cdata".
    pub kind: &'static str,
    /// Nesting level (1 for the root).
    pub level: u32,
    /// Attributes (open/complete entries with attributes).
    pub attributes: Option<Vec<(Vec<u8>, Vec<u8>)>>,
    /// Character data.
    pub value: Option<Vec<u8>>,
}

/// Collects `xml_parse_into_struct()` output (observed on PHP 8.5: levels
/// past 255 are dropped with a warning, but their end tags still produce
/// "close" entries).
#[derive(Debug, Default)]
pub struct IntoStruct {
    /// The values array.
    pub values: Vec<StructEntry>,
    /// The index array: tag name to entry indices, in first-seen order.
    pub index: Vec<(Vec<u8>, Vec<usize>)>,
    /// "Maximum depth exceeded - Results truncated" should be warned.
    pub truncated: bool,
    level: u32,
    last_open: bool,
    skip_white: bool,
}

/// The deepest level `xml_parse_into_struct()` records.
pub const MAX_STRUCT_LEVEL: u32 = 255;

impl IntoStruct {
    /// A collector; `skip_white` is XML_OPTION_SKIP_WHITE.
    pub fn new(skip_white: bool) -> IntoStruct {
        IntoStruct {
            skip_white,
            ..Default::default()
        }
    }
    /// The handlers `xml_parse_into_struct()` installs.
    pub fn handlers() -> Handlers {
        Handlers {
            element: true,
            cdata: true,
            ..Default::default()
        }
    }
    fn add_index(&mut self, tag: &[u8], i: usize) {
        match self.index.iter_mut().find(|(t, _)| t == tag) {
            Some((_, v)) => v.push(i),
            None => self.index.push((tag.to_vec(), vec![i])),
        }
    }
}

impl SaxHandler for IntoStruct {
    fn start_element(&mut self, name: &[u8], attrs: &[(Vec<u8>, Vec<u8>)], _pos: &Position) {
        self.level += 1;
        if self.level > MAX_STRUCT_LEVEL {
            self.truncated = true;
            return;
        }
        let mut merged: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        for (k, v) in attrs {
            match merged.iter_mut().find(|(mk, _)| mk == k) {
                Some(e) => e.1 = v.clone(),
                None => merged.push((k.clone(), v.clone())),
            }
        }
        let i = self.values.len();
        self.values.push(StructEntry {
            tag: name.to_vec(),
            kind: "open",
            level: self.level,
            attributes: if merged.is_empty() {
                None
            } else {
                Some(merged)
            },
            value: None,
        });
        self.add_index(name, i);
        self.last_open = true;
    }

    fn end_element(&mut self, name: &[u8], _pos: &Position) {
        if self.last_open {
            if let Some(last) = self.values.last_mut() {
                last.kind = "complete";
            }
        } else {
            let i = self.values.len();
            self.values.push(StructEntry {
                tag: name.to_vec(),
                kind: "close",
                level: self.level,
                attributes: None,
                value: None,
            });
            self.add_index(name, i);
        }
        self.last_open = false;
        self.level = self.level.saturating_sub(1);
    }

    fn character_data(&mut self, data: &[u8], _pos: &Position) {
        if self.level == 0 || self.level > MAX_STRUCT_LEVEL {
            return;
        }
        if self.last_open {
            let last = self.values.last_mut().unwrap();
            match &mut last.value {
                Some(v) => v.extend_from_slice(data),
                None => {
                    if self.skip_white && data.iter().all(|b| crate::chars::is_space(*b)) {
                        return;
                    }
                    last.value = Some(data.to_vec());
                }
            }
            return;
        }
        if let Some(last) = self.values.last_mut()
            && last.kind == "cdata"
            && last.level == self.level
        {
            last.value
                .get_or_insert_with(Vec::new)
                .extend_from_slice(data);
            return;
        }
        if self.skip_white && data.iter().all(|b| crate::chars::is_space(*b)) {
            return;
        }
        // The tag of the enclosing element.
        let tag = self
            .values
            .iter()
            .rev()
            .find(|e| e.level == self.level && e.kind == "open")
            .map(|e| e.tag.clone())
            .unwrap_or_default();
        let i = self.values.len();
        self.values.push(StructEntry {
            tag: tag.clone(),
            kind: "cdata",
            level: self.level,
            attributes: None,
            value: Some(data.to_vec()),
        });
        self.add_index(&tag, i);
    }
}

/// Runs `xml_parse_into_struct()` on a whole document with the parser's
/// options. Returns the parse result and the collected values.
pub fn parse_into_struct(p: &mut SaxParser, data: &[u8]) -> (bool, IntoStruct) {
    let mut c = IntoStruct::new(p.options.skip_white);
    let ok = p.parse(data, true, IntoStruct::handlers(), &mut c);
    (ok, c)
}
