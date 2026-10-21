//! DTD validation (valid.c). Not implemented yet: documents are reported
//! as valid.

use crate::parser::XmlError;
use crate::tree::NodeId;

pub fn validate_document(_doc: NodeId) -> Vec<XmlError> {
    Vec::new()
}
