//! XML core for ext/dom, ext/xml, simplexml, xmlreader and xmlwriter (W3C XML 1.0, Namespaces in XML, XPath 1.0).

pub mod chars;
pub mod encoding;
pub mod error;
pub mod serialize;
pub mod tree;
pub mod xml;

pub use tree::{Arena, NodeId, NodeType, QName, Str};
