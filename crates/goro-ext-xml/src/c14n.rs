//! XML canonicalization (c14n.c). Not implemented yet.

use crate::tree::NodeId;
use goro_core::value::Value;
use goro_core::vm::Vm;

pub fn canonicalize(
    _vm: &mut Vm,
    _node: NodeId,
    _exclusive: bool,
    _with_comments: bool,
    _xpath: Option<&Value>,
    _prefixes: Option<&Value>,
) -> Result<Option<Vec<u8>>, ()> {
    Ok(None)
}
