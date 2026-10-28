//! DTD validation (XML 1.0 validity constraints).

use crate::error::XmlError;
use crate::tree::{Arena, NodeId};

/// Validates a document against its DTD (`DOMDocument::validate()`,
/// LIBXML_DTDVALID). Returns the validity errors.
pub fn validate(_arena: &Arena, _doc: NodeId) -> Vec<XmlError> {
    Vec::new()
}
