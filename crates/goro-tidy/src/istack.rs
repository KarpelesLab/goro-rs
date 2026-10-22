//! istack.c: the inline stack (Mosaic-compatible recovery of inline markup).

use crate::attrs;
use crate::doc::*;
use crate::enums::TagId;
use crate::lexer;

/// DupAttrs(): duplicate attributes
pub fn dup_attrs(doc: &mut Doc, attrs: Option<AttId>) -> Option<AttId> {
    let a = attrs?;
    let newattrs = lexer::new_attribute(doc);
    let src = doc[a].clone();
    doc[newattrs] = src.clone();
    let next = dup_attrs(doc, src.next);
    doc[newattrs].next = next;
    let d = attrs::find_attribute(doc, newattrs);
    doc[newattrs].dict = d;
    let asp = src.asp.map(|n| lexer::clone_node(doc, Some(n)));
    let php = src.php.map(|n| lexer::clone_node(doc, Some(n)));
    doc[newattrs].asp = asp;
    doc[newattrs].php = php;
    Some(newattrs)
}

fn is_node_pushable(doc: &Doc, node: NodeId) -> bool {
    let Some(t) = doc[node].tag else {
        return false;
    };
    let model = doc.dict_model(t);
    if model & CM_INLINE == 0 {
        return false;
    }
    if model & CM_OBJECT != 0 {
        return false;
    }
    if doc.is(node, TagId::INS) || doc.is(node, TagId::DEL) {
        return false;
    }
    true
}

/// PushInline(): push a copy of an inline node onto stack
pub fn push_inline(doc: &mut Doc, node: NodeId) {
    if doc[node].implicit {
        return;
    }
    if !is_node_pushable(doc, node) {
        return;
    }
    if !doc.is(node, TagId::FONT) && is_pushed(doc, node) {
        return;
    }
    let tag = doc[node].tag;
    let element = doc[node].element.clone();
    let src_attrs = doc[node].attributes;
    let attributes = dup_attrs(doc, src_attrs);
    doc.lx().istack.push(IStack {
        tag,
        element,
        attributes,
    });
}

/// PopIStack(): returns the popped entry's tag
fn pop_istack(doc: &mut Doc) -> Option<DictRef> {
    let e = doc.lx().istack.pop()?;
    let mut a = e.attributes;
    while let Some(av) = a {
        a = doc[av].next;
        lexer::free_attribute(doc, av);
    }
    e.tag
}

fn pop_istack_until(doc: &mut Doc, tid: TagId) {
    while !doc.lxr().istack.is_empty() {
        let tag = pop_istack(doc);
        if tag.is_some_and(|t| doc.dict_id(t) == tid) {
            break;
        }
    }
}

/// PopInline(): pop inline stack
pub fn pop_inline(doc: &mut Doc, node: Option<NodeId>) {
    if let Some(n) = node {
        if !is_node_pushable(doc, n) {
            return;
        }
        if doc.is(n, TagId::A) {
            pop_istack_until(doc, TagId::A);
            return;
        }
    }
    if !doc.lxr().istack.is_empty() {
        pop_istack(doc);
        let lx = doc.lx();
        if lx.insert.is_some_and(|i| i >= lx.istack.len()) {
            lx.insert = None;
        }
    }
}

pub fn is_pushed(doc: &mut Doc, node: NodeId) -> bool {
    let tag = doc[node].tag;
    doc.lxr().istack.iter().rev().any(|e| e.tag == tag)
}

/// IsPushedLast(): whether the last element on the stack has the same
/// type than "node"
pub fn is_pushed_last(doc: &mut Doc, element: NodeId, node: NodeId) -> bool {
    if !is_node_pushable(doc, element) {
        return false;
    }
    let tag = doc[node].tag;
    doc.lxr().istack.last().is_some_and(|e| e.tag == tag)
}

/// InlineDup(): insert "missing" inline elements around the contents of
/// blocklevel elements
pub fn inline_dup(doc: &mut Doc, node: Option<NodeId>) -> i32 {
    let lx = doc.lx();
    let n = (lx.istack.len() as u32).wrapping_sub(lx.istackbase as u32) as i32;
    if n > 0 {
        lx.insert = Some(lx.istackbase);
        lx.inode = node;
    }
    n
}

/// DeferDup(): defer duplicates when entering a table or other element
/// where the inlines shouldn't be duplicated
pub fn defer_dup(doc: &mut Doc) {
    let lx = doc.lx();
    lx.insert = None;
    lx.inode = None;
}

pub fn inserted_token(doc: &mut Doc) -> NodeId {
    let Some(ins) = doc.lxr().insert else {
        let node = doc.lx().inode.take();
        return node.expect("tidy: InsertedToken without inode");
    };

    if doc.lxr().inode.is_none() {
        let (l, c) = {
            let i = doc.doc_in.as_ref().expect("tidy: no input stream");
            (i.curline, i.curcol)
        };
        let lx = doc.lx();
        lx.lines = l as u32;
        lx.columns = c as u32;
    }

    let node = lexer::new_node(doc);
    let (s, e) = (doc.lxr().txtstart, doc.lxr().txtend);
    let entry = doc.lxr().istack[ins].clone();
    doc[node].type_ = NodeType::StartTag;
    doc[node].implicit = true;
    doc[node].start = s;
    doc[node].end = e;
    doc[node].element = entry.element;
    doc[node].tag = entry.tag;
    let a = dup_attrs(doc, entry.attributes);
    doc[node].attributes = a;

    let n = ins + 1;
    let lx = doc.lx();
    lx.insert = if n < lx.istack.len() { Some(n) } else { None };
    node
}

/// SwitchInline(): two CM_INLINE elements are pushed, the first is
/// closing; switch their positions on the stack
pub fn switch_inline(doc: &mut Doc, element: NodeId, node: NodeId) -> bool {
    if doc.lexer.is_none() || doc[element].tag.is_none() || doc[node].tag.is_none() {
        return false;
    }
    if !is_pushed(doc, element) || !is_pushed(doc, node) {
        return false;
    }
    let (etag, ntag) = (doc[element].tag, doc[node].tag);
    let lx = doc.lx();
    let span = (lx.istack.len() as u32).wrapping_sub(lx.istackbase as u32);
    if span < 2 {
        return false;
    }
    let mut i = span.wrapping_sub(1) as i32 as i64;
    while i >= 0 {
        if lx.istack[i as usize].tag == etag {
            let i1 = i as usize;
            i -= 1;
            let mut i2 = None;
            while i >= 0 {
                if lx.istack[i as usize].tag == ntag {
                    i2 = Some(i as usize);
                    break;
                }
                i -= 1;
            }
            if let Some(i2) = i2 {
                lx.istack.swap(i1, i2);
                return true;
            }
        }
        i -= 1;
    }
    false
}

/// InlineDup1(): push a specific element on the stack (it may not be the
/// last element)
pub fn inline_dup1(doc: &mut Doc, node: Option<NodeId>, element: NodeId) -> bool {
    let etag = doc[element].tag;
    if etag.is_none() {
        return false;
    }
    let lx = doc.lx();
    let n = (lx.istack.len() as u32).wrapping_sub(lx.istackbase as u32) as i32;
    if n > 0 {
        let mut i = n - 1;
        while i >= 0 {
            if lx.istack[i as usize].tag == etag {
                lx.insert = Some(i as usize);
                lx.inode = node;
                return true;
            }
            i -= 1;
        }
    }
    false
}
