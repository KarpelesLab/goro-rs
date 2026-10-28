//! XML 1.0 (Fifth Edition) and Namespaces in XML 1.0 parsing.
//!
//! [`Parser`] is an event parser: it reads a document (whole or in chunks)
//! and calls a [`Sink`]. [`build`] runs it with the tree builder and returns
//! a document in an [`Arena`](crate::Arena) — what `DOMDocument::loadXML`,
//! `simplexml_load_string` and `Dom\XMLDocument::createFromString` need.
//!
//! Error codes, messages, levels and positions are those PHP 8.5 reports
//! through libxml2 2.15 (observed on PHP 8.5; fixtures in `tests/fixtures`).

mod builder;
mod dtd;
mod parser;
pub mod validate;

pub use builder::{Built, TreeBuilder, build, build_into, parse_fragment};
pub use dtd::attach_external_subset;
pub use parser::{AttrPiece, Attribute, Parser, StartTag, Status, resolve_system_id};

use crate::error::XmlError;
use crate::tree::{DtdData, EntityDecl, QName};

/// `LIBXML_*` option bits (PHP manual, "libxml constants").
pub mod opt {
    /// LIBXML_RECOVER: keep going after errors.
    pub const RECOVER: u32 = 1 << 0;
    /// LIBXML_NOENT: substitute entities.
    pub const NOENT: u32 = 1 << 1;
    /// LIBXML_DTDLOAD: load the external subset.
    pub const DTDLOAD: u32 = 1 << 2;
    /// LIBXML_DTDATTR: default DTD attributes.
    pub const DTDATTR: u32 = 1 << 3;
    /// LIBXML_DTDVALID: validate with the DTD.
    pub const DTDVALID: u32 = 1 << 4;
    /// LIBXML_NOERROR: suppress error reports.
    pub const NOERROR: u32 = 1 << 5;
    /// LIBXML_NOWARNING: suppress warning reports.
    pub const NOWARNING: u32 = 1 << 6;
    /// LIBXML_PEDANTIC
    pub const PEDANTIC: u32 = 1 << 7;
    /// LIBXML_NOBLANKS: remove blank nodes.
    pub const NOBLANKS: u32 = 1 << 8;
    /// LIBXML_XINCLUDE: implement XInclude substitution.
    pub const XINCLUDE: u32 = 1 << 10;
    /// LIBXML_NONET: forbid network access.
    pub const NONET: u32 = 1 << 11;
    /// LIBXML_NSCLEAN: remove redundant namespace declarations.
    pub const NSCLEAN: u32 = 1 << 13;
    /// LIBXML_NOCDATA: merge CDATA as text nodes.
    pub const NOCDATA: u32 = 1 << 14;
    /// LIBXML_COMPACT: small text nodes compacted (no observable effect).
    pub const COMPACT: u32 = 1 << 16;
    /// LIBXML_PARSEHUGE: relax hardcoded limits.
    pub const PARSEHUGE: u32 = 1 << 19;
    /// LIBXML_BIGLINES: line numbers above 65535.
    pub const BIGLINES: u32 = 1 << 22;
    /// LIBXML_NO_XXE (PHP 8.4): never load external entities.
    pub const NO_XXE: u32 = 1 << 23;
    /// LIBXML_NOXMLDECL (save option): drop the XML declaration.
    pub const NOXMLDECL: u32 = 1 << 1;
    /// LIBXML_NOEMPTYTAG (save option): `<a></a>` instead of `<a/>`.
    pub const NOEMPTYTAG: u32 = 1 << 2;
}

/// Something that loads external entities and subsets (the PHP entity
/// loader: default file/stream loading or `libxml_set_external_entity_loader`).
pub trait EntityLoader {
    /// Loads the resource. `base` is the document URL for resolving
    /// relative system IDs. Returns the bytes or an error message.
    fn load(
        &mut self,
        public_id: Option<&str>,
        system_id: &str,
        base: Option<&str>,
    ) -> Result<Vec<u8>, LoadError>;
    /// [`EntityLoader::load`] with the document's DOCTYPE details (what
    /// PHP passes as the user loader's `$context`).
    fn load_with(
        &mut self,
        public_id: Option<&str>,
        system_id: &str,
        base: Option<&str>,
        ctx: &LoadContext,
    ) -> Result<Vec<u8>, LoadError> {
        let _ = ctx;
        self.load(public_id, system_id, base)
    }
}

/// The DOCTYPE of the document an external resource is loaded for.
#[derive(Clone, Debug, Default)]
pub struct LoadContext {
    /// The DOCTYPE name.
    pub int_sub_name: Option<String>,
    /// The external subset's system ID.
    pub ext_sub_uri: Option<String>,
    /// The external subset's public ID.
    pub ext_sub_system: Option<String>,
}

/// Why an external resource could not be loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// The resource could not be read: libxml's "failed to load external
    /// entity" warning follows.
    NotFound,
    /// The PHP-level resolver returned null: no warning from the parser
    /// (PHP prints its own).
    Refused,
}

/// Parse settings.
#[derive(Clone, Debug)]
pub struct ParseOptions {
    /// `LIBXML_*` bits (see [`opt`]).
    pub flags: u32,
    /// Namespace processing (on for DOM/SimpleXML; `xml_parser_create`
    /// without `_ns` turns it off).
    pub namespaces: bool,
    /// The document URL (error file names, base for relative system IDs).
    pub url: Option<String>,
    /// Encoding to use instead of autodetection.
    pub encoding_override: Option<String>,
    /// Report each entity and character reference as separate text events
    /// (the SAX layer of ext/xml); the tree builder merges text anyway.
    pub split_text_at_refs: bool,
    /// Build the `Dom\XMLDocument` flavour: `xmlns` declarations become
    /// attributes in the XMLNS namespace.
    pub modern: bool,
    /// Load external parsed entities (with a loader).
    pub load_external_entities: bool,
    /// SAX mode (ext/xml): report raw markup, positions and references,
    /// and split character data at references and carriage returns as
    /// PHP's libxml-based parser does (observed on PHP 8.5).
    pub sax: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        ParseOptions {
            flags: 0,
            namespaces: true,
            url: None,
            encoding_override: None,
            split_text_at_refs: false,
            modern: false,
            load_external_entities: false,
            sax: false,
        }
    }
}

impl ParseOptions {
    /// Options from `LIBXML_*` flags.
    pub fn with_flags(flags: u32) -> ParseOptions {
        ParseOptions {
            flags,
            ..Default::default()
        }
    }
    /// True if a flag is set.
    pub fn has(&self, f: u32) -> bool {
        self.flags & f != 0
    }
    /// True if the external subset should be loaded.
    pub fn load_dtd(&self) -> bool {
        !self.has(opt::NO_XXE) && self.flags & (opt::DTDLOAD | opt::DTDVALID | opt::DTDATTR) != 0
    }
    /// True if external general entities should be loaded.
    pub fn load_entities(&self) -> bool {
        !self.has(opt::NO_XXE)
            && (self.load_external_entities || self.has(opt::NOENT) || self.load_dtd())
    }
}

/// What the parser reports.
pub trait Sink {
    /// The XML declaration.
    fn xml_decl(&mut self, _version: &str, _encoding: Option<&str>, _standalone: i8) {}
    /// The document type declaration has been read (internal subset and,
    /// when loaded, external subset).
    fn doctype(&mut self, _dtd: &DtdData, _external: Option<&DtdData>) {}
    /// A start tag.
    fn start_element(&mut self, tag: &mut StartTag<'_>);
    /// An end tag (also after an empty-element tag).
    fn end_element(&mut self, name: &QName);
    /// Character data.
    fn text(&mut self, text: &str);
    /// Character data the sink may keep.
    fn text_owned(&mut self, text: String) {
        self.text(&text)
    }
    /// Whitespace the parser judged ignorable (LIBXML_NOBLANKS).
    fn ignorable_whitespace(&mut self, _text: &str) {}
    /// A CDATA section (with LIBXML_NOCDATA this comes as `text`).
    fn cdata(&mut self, text: &str) {
        self.text(text)
    }
    /// A comment.
    fn comment(&mut self, _text: &str) {}
    /// A processing instruction.
    fn pi(&mut self, _target: &str, _data: &str) {}
    /// A general entity reference in content. `decl` is `None` for
    /// undeclared entities (only reported in recovery and standalone=no
    /// cases). Returns true to have the parser expand the replacement text
    /// (reporting its content between this call and `entity_end`).
    fn entity_start(&mut self, _name: &str, _decl: Option<&EntityDecl>) -> bool {
        true
    }
    /// End of an entity reference's content.
    fn entity_end(&mut self, _name: &str) {}
    /// An error or warning. Fatal errors stop the parse unless recovering.
    fn error(&mut self, _e: &XmlError) {}
    /// The source line of the construct being reported (for start tags it
    /// is in [`StartTag::line`] too).
    fn set_line(&mut self, _line: u32) {}
    /// SAX mode: the raw markup of the start tag, end tag, comment or PI
    /// reported next.
    fn raw(&mut self, _text: &str) {}
    /// SAX mode: the position of the event reported next (byte offset in
    /// the decoded input, 1-based line and column).
    fn position(&mut self, _offset: usize, _line: u32, _column: u32) {}
    /// SAX mode: a character reference or predefined entity reference
    /// (`raw` as written, `value` its character).
    fn reference(&mut self, _raw: &str, value: &str) {
        self.text(value)
    }
    /// SAX mode: a reference to an undeclared entity, before the error.
    fn undefined_entity(&mut self, _name: &str) {}
}
