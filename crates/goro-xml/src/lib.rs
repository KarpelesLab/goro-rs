//! XML core for PHP's ext/dom, ext/xml, simplexml, xmlreader and
//! xmlwriter: parsing (W3C XML 1.0 Fifth Edition, Namespaces in XML 1.0,
//! the legacy libxml-style HTML parser), an arena node tree, serializers,
//! XPath 1.0, C14N and the event/pull/writer APIs.
//!
//! Map of the crate (for the extension wiring):
//!
//! - [`tree`] — [`Arena`] of nodes shared by any number of documents
//!   ([`NodeId`] handles stay valid across `adoptNode`); DOM operations
//!   (`insert_before`, `append_child`, `replace_child`, `remove_child`,
//!   `clone_node`, `adopt`, `import_from`, `normalize`, `text_content`,
//!   `elements_by_tag_name(_ns)`, `get_element_by_id`, `lookup_ns`,
//!   `lookup_prefix`, `node_path`, `cmp_order`) with the DOM Standard's
//!   pre-insertion validity checks ([`tree::DomError`]).
//! - [`xml`] — the XML parser ([`xml::Parser`], event [`xml::Sink`]),
//!   [`xml::build`] (loadXML / createFromString), [`xml::parse_fragment`]
//!   (appendXML), LIBXML_* options ([`xml::opt`], [`xml::ParseOptions`]),
//!   external entity loading ([`xml::EntityLoader`]) and DTD validation
//!   ([`xml::validate`]).
//! - [`error`] — errors as `LibXMLError` exposes them, with PHP's warning
//!   formats.
//! - [`serialize`] — legacy `saveXML` (formatOutput, NOEMPTYTAG, encodings).
//! - [`modern`] — `Dom\` XML serialization (DOM Parsing algorithm).
//! - [`html`] — `DOMDocument::loadHTML` / `saveHTML`.
//! - [`c14n`] — Canonical XML 1.0/1.1 and Exclusive C14N.
//! - [`sax`] — ext/xml (`xml_parse`, `xml_parse_into_struct`, error strings).
//! - [`reader`] — XMLReader.
//! - [`writer`] — XMLWriter.

pub mod c14n;
pub mod chars;
pub mod css;
pub mod encoding;
pub mod error;
pub mod html;
pub mod html5;
mod html4_entities;
pub mod modern;
pub mod reader;
pub mod sax;
pub mod serialize;
pub mod tree;
pub mod writer;
pub mod xinclude;
pub mod xml;
pub mod xpath;

pub use tree::{Arena, NodeId, NodeType, QName, Str};
