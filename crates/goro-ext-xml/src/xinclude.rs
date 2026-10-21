//! XInclude processing (xinclude.c). Not implemented yet.

use crate::tree::NodeId;
use goro_core::vm::Vm;

/// Returns the number of substitutions, or -1 on error.
pub fn process(_vm: &mut Vm, _doc: NodeId, _flags: i64) -> i64 {
    0
}
