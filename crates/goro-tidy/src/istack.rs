//! istack.c: the inline stack (Mosaic-compatible recovery of inline markup).
#![allow(unused_variables)]

use crate::doc::*;

/// DupAttrs(): duplicate attributes
pub fn dup_attrs(doc: &mut Doc, attrs: Option<AttId>) -> Option<AttId> {
    todo!("DupAttrs")
}

/// PushInline(): push a copy of an inline node onto stack
pub fn push_inline(doc: &mut Doc, node: NodeId) {
    todo!("PushInline")
}

/// PopInline(): pop inline stack
pub fn pop_inline(doc: &mut Doc, node: Option<NodeId>) {
    todo!("PopInline")
}

pub fn is_pushed(doc: &mut Doc, node: NodeId) -> bool {
    todo!("IsPushed")
}

pub fn is_pushed_last(doc: &mut Doc, element: NodeId, node: NodeId) -> bool {
    todo!("IsPushedLast")
}

/// InlineDup(): insert "missing" inline elements around the contents of
/// blocklevel elements
pub fn inline_dup(doc: &mut Doc, node: Option<NodeId>) -> i32 {
    todo!("InlineDup")
}

/// DeferDup(): defer duplicates when entering a table or other element
/// where the inlines shouldn't be duplicated
pub fn defer_dup(doc: &mut Doc) {
    todo!("DeferDup")
}

pub fn inserted_token(doc: &mut Doc) -> NodeId {
    todo!("InsertedToken")
}

/// SwitchInline(): stack manipulation for inline elements
pub fn switch_inline(doc: &mut Doc, element: NodeId, node: NodeId) -> bool {
    todo!("SwitchInline")
}

pub fn inline_dup1(doc: &mut Doc, node: NodeId, element: NodeId) -> bool {
    todo!("InlineDup1")
}
