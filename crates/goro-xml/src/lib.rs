//! XML core for ext/dom, ext/xml, simplexml, xmlreader and xmlwriter (W3C XML 1.0, Namespaces in XML, XPath 1.0).

pub mod tree;

pub use tree::{Arena, NodeId, NodeType, QName, Str};
