//! DOMNode / Dom\Node methods: tree mutation (with PHP's legacy rules for
//! `DOM*` objects and the DOM Standard's for `Dom\` ones), cloning,
//! comparisons, namespace lookups, node paths and canonicalization (PHP
//! manual: DOMNode; WHATWG DOM; observed on PHP 8.5).

use goro_runtime::{ObjRef, PhpArray, Value};
use goro_xml::NodeId;
use goro_xml::tree::{DomError, NodeData, QName};

use super::xmlstore;
use super::{
    arg_node, is_modern_node, legacy_error, modern_fn, node_of, opt_arg_node, s8, settings, sv,
    sv_opt, this_node, throw_dom, with_arena, with_settings, wrap_node, wrap_opt,
};
use crate::func::{Args, NativeFn};
use crate::vm::{R, Vm};

pub const METHODS: &[(&str, NativeFn)] = &[
    ("domnode::appendchild", append_child),
    ("domnode::insertbefore", insert_before),
    ("domnode::removechild", remove_child),
    ("domnode::replacechild", replace_child),
    ("domnode::clonenode", clone_node),
    ("domnode::haschildnodes", has_child_nodes),
    ("domnode::hasattributes", has_attributes),
    ("domnode::normalize", normalize),
    ("domnode::issamenode", is_same_node),
    ("domnode::isequalnode", is_equal_node),
    ("domnode::issupported", is_supported),
    ("domnode::lookupnamespaceuri", lookup_namespace_uri),
    ("domnode::lookupprefix", lookup_prefix),
    ("domnode::isdefaultnamespace", is_default_namespace),
    ("domnode::getnodepath", get_node_path),
    ("domnode::getlineno", get_line_no),
    ("domnode::c14n", c14n),
    ("domnode::c14nfile", c14n_file),
    ("domnode::contains", contains),
    ("domnode::getrootnode", get_root_node),
    ("domnode::comparedocumentposition", compare_document_position),
    ("domnode::__sleep", sleep),
    ("domnode::__wakeup", wakeup),
    ("domnamespacenode::__sleep", sleep),
    ("domnamespacenode::__wakeup", wakeup),
    ("dom\\node::appendchild", append_child),
    ("dom\\node::insertbefore", insert_before),
    ("dom\\node::removechild", remove_child),
    ("dom\\node::replacechild", replace_child),
    ("dom\\node::clonenode", clone_node),
    ("dom\\node::haschildnodes", has_child_nodes),
    ("dom\\node::normalize", normalize),
    ("dom\\node::issamenode", is_same_node),
    ("dom\\node::isequalnode", is_equal_node),
    ("dom\\node::lookupnamespaceuri", lookup_namespace_uri),
    ("dom\\node::lookupprefix", lookup_prefix),
    ("dom\\node::isdefaultnamespace", is_default_namespace),
    ("dom\\node::getnodepath", get_node_path),
    ("dom\\node::getlineno", get_line_no),
    ("dom\\node::c14n", c14n),
    ("dom\\node::c14nfile", c14n_file),
    ("dom\\node::contains", contains),
    ("dom\\node::getrootnode", get_root_node),
    ("dom\\node::comparedocumentposition", compare_document_position),
    ("dom\\node::__sleep", sleep),
    ("dom\\node::__wakeup", wakeup),
];

// ------------------------------------------------------------- insertion

/// How an insertion was asked for (messages and return values differ).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum InsertOp {
    Append,
    InsertBefore,
}

/// Checks and inserts `node` into `parent` before `child` for the legacy
/// classes. Returns the error to raise, if any.
pub(crate) fn legacy_insert(
    parent: NodeId,
    node: NodeId,
    child: Option<NodeId>,
) -> Result<Vec<NodeId>, LegacyInsertError> {
    xmlstore::with(|s| {
        let a = &mut s.arena;
        let pdoc = a.doc_of(parent);
        let ndoc = a.doc_of(node);
        let can_have = matches!(
            a.data(parent),
            NodeData::Element(_) | NodeData::Document(_) | NodeData::Fragment
        );
        if !can_have {
            return Err(LegacyInsertError::Dom(DomError::HierarchyRequest));
        }
        if a.is_inclusive_ancestor(node, parent) {
            return Err(LegacyInsertError::Dom(DomError::HierarchyRequest));
        }
        if matches!(a.data(node), NodeData::Document(_)) {
            return Err(LegacyInsertError::Dom(DomError::HierarchyRequest));
        }
        if pdoc != ndoc {
            let hidden = s
                .docs
                .get(&ndoc)
                .and_then(|d| d.peek::<super::DocSettings>())
                .is_some_and(|d| d.hidden);
            if !hidden {
                return Err(LegacyInsertError::Dom(DomError::WrongDocument));
            }
        }
        let a = &mut s.arena;
        if let Some(c) = child
            && a.parent(c) != Some(parent)
        {
            return Err(LegacyInsertError::Dom(DomError::NotFound));
        }
        if matches!(a.data(node), NodeData::Fragment) && a.first_child(node).is_none() {
            return Err(LegacyInsertError::EmptyFragment);
        }
        if matches!(a.data(node), NodeData::Attribute(_)) {
            return Err(LegacyInsertError::Attr);
        }
        if matches!(a.data(parent), NodeData::Document(_)) {
            // Only one document element, doctype before it.
            if let Err(e) = a.check_pre_insert(parent, node, child)
                && !(e == DomError::HierarchyRequest
                    && matches!(a.data(node), NodeData::Text(_) | NodeData::CData(_)))
            {
                return Err(LegacyInsertError::Dom(e));
            }
        }
        let mut child = child;
        if child == Some(node) {
            child = a.next_sibling(node);
        }
        let old_root = a.root(node);
        let inserted = a.insert_unchecked(parent, node, child);
        if old_root != node && old_root != a.doc_of(old_root) {
            s.free_if_unheld(old_root);
        }
        for &n in &inserted {
            super::element::reconcile_after_insert(&mut s.arena, n);
        }
        s.collect();
        Ok(inserted)
    })
}

pub(crate) enum LegacyInsertError {
    Dom(DomError),
    EmptyFragment,
    Attr,
}

/// The DOM Standard's pre-insert for `Dom\` nodes (adopting the node).
pub(crate) fn modern_insert(
    parent: NodeId,
    node: NodeId,
    child: Option<NodeId>,
) -> Result<Vec<NodeId>, DomError> {
    xmlstore::with(|s| {
        let a = &mut s.arena;
        a.check_pre_insert(parent, node, child)?;
        let mut child = child;
        if child == Some(node) {
            child = a.next_sibling(node);
        }
        let old_root = a.root(node);
        let inserted = a.insert_unchecked(parent, node, child);
        if old_root != node && !matches!(a.data(old_root), NodeData::Document(_)) {
            s.free_if_unheld(old_root);
        }
        s.collect();
        Ok(inserted)
    })
}

fn do_insert(vm: &mut Vm, a: &Args, child: Option<NodeId>, op: InsertOp) -> R<Value> {
    let parent = this_node(vm, a)?;
    let node = arg_node(vm, a, 0)?;
    if modern_fn(a) {
        return match modern_insert(parent, node, child) {
            Ok(_) => Ok(a.val(0)),
            Err(e) => throw_dom(vm, e),
        };
    }
    match legacy_insert(parent, node, child) {
        Ok(_) => Ok(a.val(0)),
        Err(LegacyInsertError::Dom(e)) => legacy_error(vm, a, parent, e),
        Err(LegacyInsertError::EmptyFragment) => {
            vm.fwarn(a, "Document Fragment is empty")?;
            Ok(Value::Bool(false))
        }
        Err(LegacyInsertError::Attr) => {
            let _ = op;
            // An attribute appended to an element becomes one of its
            // attributes (observed on PHP 8.5).
            let is_el = with_arena(|ar| ar.is_element(parent));
            if !is_el {
                return legacy_error(vm, a, parent, DomError::HierarchyRequest);
            }
            super::element::set_attribute_node_raw(vm, a, parent, node, false)?;
            Ok(a.val(0))
        }
    }
}

fn append_child(vm: &mut Vm, a: &mut Args) -> R<Value> {
    do_insert(vm, a, None, InsertOp::Append)
}

fn insert_before(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let child = opt_arg_node(vm, a, 1)?;
    do_insert(vm, a, child, InsertOp::InsertBefore)
}

fn remove_child(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let parent = this_node(vm, a)?;
    let child = arg_node(vm, a, 0)?;
    let modern = modern_fn(a);
    let r = with_arena(|ar| {
        if !matches!(
            ar.data(parent),
            NodeData::Element(_) | NodeData::Document(_) | NodeData::Fragment
        ) && !modern
        {
            return Err(DomError::NotFound);
        }
        ar.remove_child(parent, child)
    });
    match r {
        Ok(_) => Ok(a.val(0)),
        Err(e) if modern => throw_dom(vm, e),
        Err(e) => legacy_error(vm, a, parent, e),
    }
}

fn replace_child(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let parent = this_node(vm, a)?;
    let node = arg_node(vm, a, 0)?;
    let child = arg_node(vm, a, 1)?;
    let modern = modern_fn(a);
    let r = xmlstore::with(|s| {
        let ar = &mut s.arena;
        if !modern {
            let pdoc = ar.doc_of(parent);
            let ndoc = ar.doc_of(node);
            if pdoc != ndoc {
                let hidden = s
                    .docs
                    .get(&ndoc)
                    .and_then(|d| d.peek::<super::DocSettings>())
                    .is_some_and(|d| d.hidden);
                if !hidden {
                    return Err(DomError::WrongDocument);
                }
            }
        }
        let ar = &mut s.arena;
        if ar.parent(child) != Some(parent) {
            return Err(DomError::NotFound);
        }
        if modern && matches!(ar.data(parent), NodeData::Document(_)) {
            // Validity as if `child` were not there (DOM Standard
            // "replace").
        }
        let old_root = ar.root(node);
        let r = ar.replace_child(parent, node, child)?;
        if old_root != node && !matches!(ar.data(old_root), NodeData::Document(_)) {
            s.free_if_unheld(old_root);
        }
        if !modern {
            super::element::reconcile_after_insert(&mut s.arena, node);
        }
        s.collect();
        Ok(r)
    });
    match r {
        Ok(_) => Ok(a.val(1)),
        Err(e) if modern => throw_dom(vm, e),
        Err(e) => legacy_error(vm, a, parent, e),
    }
}

// --------------------------------------------------------------- cloning

/// `cloneNode()`: a copy in the same document (documents copy whole).
pub(crate) fn clone_id(n: NodeId, deep: bool) -> NodeId {
    xmlstore::with(|s| {
        let a = &mut s.arena;
        let doc = a.doc_of(n);
        let c = a.clone_node(n, doc, deep);
        if matches!(a.data(n), NodeData::Document(_)) {
            let st = s.doc_ext(n).get::<super::DocSettings>().clone();
            *s.doc_ext(c).get::<super::DocSettings>() = st;
        }
        c
    })
}

fn clone_node(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let deep = a.bool(0);
    let c = clone_id(n, deep);
    let v = wrap_node(vm, c)?;
    // The copy is held by its object now; an unreferenced one is freed.
    Ok(v)
}

/// `clone $node`: a deep copy (observed on PHP 8.5).
pub(crate) fn clone_hook(_vm: &mut Vm, from: &ObjRef, to: &ObjRef) {
    let Some(n) = node_of(from) else {
        return;
    };
    let c = clone_id(n, true);
    *to.obj().native.borrow_mut() = None;
    super::attach(to, c);
}

// ------------------------------------------------------------- queries

fn has_child_nodes(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    Ok(Value::Bool(with_arena(|ar| ar.first_child(n).is_some())))
}

fn has_attributes(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    Ok(Value::Bool(with_arena(|ar| {
        ar.element(n).is_some_and(|e| e.first_attr.is_some())
    })))
}

fn normalize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    with_arena(|ar| ar.normalize(n));
    Ok(Value::Null)
}

fn is_same_node(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let o = opt_arg_node(vm, a, 0)?;
    Ok(Value::Bool(o == Some(n)))
}

fn is_equal_node(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let Some(o) = opt_arg_node(vm, a, 0)? else {
        return Ok(Value::Bool(false));
    };
    Ok(Value::Bool(with_arena(|ar| ar.is_equal_node(n, o))))
}

fn is_supported(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    // PHP's DOM reports every feature as supported (observed on PHP 8.5).
    Ok(Value::Bool(true))
}

/// The element a namespace lookup starts from (DOM Standard "locate a
/// namespace").
fn lookup_start(ar: &goro_xml::Arena, n: NodeId) -> Option<NodeId> {
    match ar.data(n) {
        NodeData::Element(_) => Some(n),
        NodeData::Document(_) => ar.document_element(n),
        NodeData::Attribute(_) | NodeData::Namespace(_) => ar.parent(n),
        NodeData::Doctype(_) | NodeData::Fragment => None,
        _ => ar.parent(n).filter(|&p| ar.is_element(p)),
    }
}

/// The namespace bound to `prefix` at `n` (modern documents keep their
/// declarations as `xmlns` attributes).
pub(crate) fn locate_ns(ar: &goro_xml::Arena, n: NodeId, prefix: Option<&str>) -> Option<String> {
    let start = lookup_start(ar, n)?;
    if !is_modern_node(ar, n) {
        return ar.lookup_ns(start, prefix).map(|s| s.to_string());
    }
    match prefix {
        Some("xml") => return Some(goro_xml::tree::XML_NS.into()),
        Some("xmlns") => return Some(goro_xml::tree::XMLNS_NS.into()),
        _ => {}
    }
    let mut cur = Some(start);
    while let Some(c) = cur {
        if let Some(e) = ar.element(c) {
            if e.name.ns.is_some() && e.name.prefix.as_deref() == prefix {
                return e.name.ns.as_ref().map(|s| s.to_string());
            }
            for at in ar.attrs(c) {
                let q = ar.qname(at).unwrap();
                if q.ns.as_deref() != Some(goro_xml::tree::XMLNS_NS) {
                    continue;
                }
                let matches = match prefix {
                    None => q.prefix.is_none() && &*q.local == "xmlns",
                    Some(p) => q.prefix.as_deref() == Some("xmlns") && &*q.local == p,
                };
                if matches {
                    let v = ar.attr_value(at);
                    return if v.is_empty() { None } else { Some(v) };
                }
            }
        }
        cur = ar.parent(c).filter(|&p| ar.is_element(p));
    }
    None
}

fn lookup_namespace_uri(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let p = a.opt_str(0).map(|s| s8(&s)).filter(|s| !s.is_empty());
    let r = with_arena(|ar| locate_ns(ar, n, p.as_deref()));
    Ok(sv_opt(r.as_deref()))
}

fn lookup_prefix(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let Some(ns) = a.opt_str(0).map(|s| s8(&s)).filter(|s| !s.is_empty()) else {
        return Ok(Value::Null);
    };
    let r = with_arena(|ar| {
        let start = lookup_start(ar, n)?;
        if is_modern_node(ar, n) {
            let mut cur = Some(start);
            while let Some(c) = cur {
                let e = ar.element(c)?;
                if e.name.ns.as_deref() == Some(&ns) && e.name.prefix.is_some() {
                    return e.name.prefix.as_ref().map(|s| s.to_string());
                }
                for at in ar.attrs(c) {
                    let q = ar.qname(at).unwrap();
                    if q.prefix.as_deref() == Some("xmlns") && ar.attr_value(at) == ns {
                        return Some(q.local.to_string());
                    }
                }
                cur = ar.parent(c).filter(|&p| ar.is_element(p));
            }
            return None;
        }
        ar.lookup_prefix(start, &ns).map(|s| s.to_string())
    });
    Ok(sv_opt(r.as_deref()))
}

fn is_default_namespace(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let ns = a.opt_str(0).map(|s| s8(&s)).filter(|s| !s.is_empty());
    let r = with_arena(|ar| locate_ns(ar, n, None));
    Ok(Value::Bool(r == ns))
}

fn get_node_path(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let p = with_arena(|ar| ar.node_path(n));
    Ok(sv(&p))
}

fn get_line_no(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    Ok(Value::Int(with_arena(|ar| ar.line(n)) as i64))
}

fn contains(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let Some(o) = a.obj(0) else {
        return Ok(Value::Bool(false));
    };
    let Some(other) = node_of(&o) else {
        return Ok(Value::Bool(false));
    };
    Ok(Value::Bool(with_arena(|ar| {
        !matches!(ar.data(other), NodeData::Namespace(_)) && ar.is_inclusive_ancestor(n, other)
    })))
}

fn get_root_node(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let r = with_arena(|ar| {
        if matches!(ar.data(n), NodeData::Attribute(_)) && modern_fn(a) {
            return n;
        }
        ar.root(n)
    });
    wrap_node(vm, r)
}

/// DOM Standard `compareDocumentPosition()`.
fn compare_document_position(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let other = arg_node(vm, a, 0)?;
    const DISCONNECTED: i64 = 1;
    const PRECEDING: i64 = 2;
    const FOLLOWING: i64 = 4;
    const CONTAINS: i64 = 8;
    const CONTAINED_BY: i64 = 16;
    const IMPL: i64 = 32;
    let r = with_arena(|ar| {
        if n == other {
            return 0;
        }
        let mut node1 = Some(other);
        let mut node2 = Some(n);
        let mut attr1 = None;
        let mut attr2 = None;
        if matches!(ar.data(other), NodeData::Attribute(_)) {
            attr1 = Some(other);
            node1 = ar.parent(other);
        }
        if matches!(ar.data(n), NodeData::Attribute(_)) {
            attr2 = Some(n);
            node2 = ar.parent(n);
            if let (Some(a1), Some(e)) = (attr1, node2)
                && node1 == Some(e)
            {
                for at in ar.attrs(e) {
                    if Some(at) == attr1 {
                        return IMPL | PRECEDING;
                    }
                    if Some(at) == attr2 {
                        return IMPL | FOLLOWING;
                    }
                }
                let _ = a1;
            }
        }
        let (Some(x1), Some(x2)) = (node1, node2) else {
            let k = if other.index() < n.index() {
                PRECEDING
            } else {
                FOLLOWING
            };
            return DISCONNECTED | IMPL | k;
        };
        if ar.root(x1) != ar.root(x2) {
            let k = if ar.root(x1).index() < ar.root(x2).index() {
                PRECEDING
            } else {
                FOLLOWING
            };
            return DISCONNECTED | IMPL | k;
        }
        if (attr1.is_none() && ar.is_inclusive_ancestor(x1, x2) && x1 != x2)
            || (attr2.is_some() && x1 == x2 && attr1.is_none())
        {
            return CONTAINS | PRECEDING;
        }
        if (attr2.is_none() && ar.is_inclusive_ancestor(x2, x1) && x1 != x2)
            || (attr1.is_some() && x1 == x2 && attr2.is_none())
        {
            return CONTAINED_BY | FOLLOWING;
        }
        if ar.cmp_order(x1, x2) == std::cmp::Ordering::Less {
            PRECEDING
        } else {
            FOLLOWING
        }
    });
    Ok(Value::Int(r))
}

// ----------------------------------------------------------- C14N

fn c14n_run(vm: &mut Vm, a: &Args, first: usize) -> R<Option<String>> {
    let n = this_node(vm, a)?;
    let exclusive = a.bool(first);
    let with_comments = a.bool(first + 1);
    let xpath = match a.val(first + 2) {
        Value::Array(x) => Some(x),
        _ => None,
    };
    let prefixes = match a.val(first + 3) {
        Value::Array(p) => p
            .iter()
            .map(|(_, v)| s8(&v.as_str().cloned().unwrap_or_default()))
            .collect(),
        _ => Vec::new(),
    };
    let attached = with_arena(|ar| ar.is_connected(n) || ar.parent(n).is_some());
    let is_doc = with_arena(|ar| matches!(ar.data(n), NodeData::Document(_)));
    if !attached && !is_doc {
        return vm.throw("ValueError", "Node must be associated with a document");
    }
    let node_set = match xpath {
        Some(x) => {
            let Some(q) = x.get_bytes(b"query").cloned() else {
                return vm.throw("ValueError", "\"query\" option missing in $xpath");
            };
            let q = vm.to_php_str(&q)?;
            let ns: Vec<(String, String)> = match x.get_bytes(b"namespaces") {
                Some(Value::Array(m)) => m
                    .iter()
                    .map(|(k, v)| {
                        (
                            match k { goro_runtime::KeyRef::Str(s) => s8(s), goro_runtime::KeyRef::Int(i) => i.to_string() },
                            s8(&v.as_str().cloned().unwrap_or_default()),
                        )
                    })
                    .collect(),
                _ => Vec::new(),
            };
            let ids = super::xpath::eval_nodes(vm, n, &s8(&q), &ns)?;
            Some(ids.into_iter().collect())
        }
        None => None,
    };
    let opts = goro_xml::c14n::C14nOptions {
        mode: if exclusive {
            goro_xml::c14n::Mode::Exclusive
        } else {
            goro_xml::c14n::Mode::Inclusive
        },
        with_comments,
        inclusive_prefixes: prefixes,
        node_set,
    };
    Ok(Some(with_arena(|ar| {
        goro_xml::c14n::canonicalize(ar, n, &opts)
    })))
}

fn c14n(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match c14n_run(vm, a, 0)? {
        Some(s) => Ok(sv(&s)),
        None => Ok(Value::Bool(false)),
    }
}

fn c14n_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let path = a.str(0);
    let Some(s) = c14n_run(vm, a, 1)? else {
        return Ok(Value::Bool(false));
    };
    let p = vm.fs_path(path.as_bytes());
    match std::fs::write(&p, s.as_bytes()) {
        Ok(()) => Ok(Value::Int(s.len() as i64)),
        Err(_) => Ok(Value::Bool(false)),
    }
}

// ------------------------------------------------------- serialization

fn sleep(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = vm.class_of(a.this_obj()).name.to_string_lossy();
    vm.throw(
        "Exception",
        &format!(
            "Serialization of '{c}' is not allowed, unless serialization methods are implemented in a subclass"
        ),
    )
}

fn wakeup(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = vm.class_of(a.this_obj()).name.to_string_lossy();
    vm.throw(
        "Exception",
        &format!(
            "Unserialization of '{c}' is not allowed, unless unserialization methods are implemented in a subclass"
        ),
    )
}

// --------------------------------------------------- value and content

/// Decodes the entity and character references of a legacy `nodeValue`
/// assignment to an element (PHP parses them; observed on PHP 8.5).
fn decode_refs(ar: &goro_xml::Arena, doc: NodeId, s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if let Some(end) = tail.find(';') {
            let name = &tail[1..end];
            let rep = if let Some(num) = name.strip_prefix('#') {
                let v = if let Some(h) = num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
                    u32::from_str_radix(h, 16).ok()
                } else {
                    num.parse::<u32>().ok()
                };
                v.and_then(char::from_u32).map(|c| c.to_string())
            } else if !name.is_empty() && !name.contains(|c: char| c.is_whitespace() || c == '&')
            {
                ar.entity_value(doc, name)
            } else {
                None
            };
            if let Some(r) = rep {
                out.push_str(&r);
                rest = &tail[end + 1..];
                continue;
            }
        }
        out.push('&');
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

/// The `nodeValue` setter.
pub(crate) fn set_node_value(vm: &mut Vm, n: NodeId, text: &str, modern: bool) -> R<()> {
    let _ = vm;
    xmlstore::with(|s| {
        let ar = &mut s.arena;
        match ar.data(n) {
            NodeData::Element(_) | NodeData::Fragment if !modern => {
                let doc = ar.doc_of(n);
                let t = decode_refs(ar, doc, text);
                ar.set_text_content(n, &t);
            }
            NodeData::Element(_) | NodeData::Fragment => {
                ar.set_text_content(n, text);
            }
            NodeData::Attribute(_) => ar.set_attr_value(n, text),
            NodeData::Text(_) | NodeData::CData(_) | NodeData::Comment(_) | NodeData::Pi { .. } => {
                ar.set_text_content(n, text)
            }
            _ => {}
        }
    });
    Ok(())
}

/// The `textContent` setter.
pub(crate) fn set_text_content(vm: &mut Vm, n: NodeId, text: &str) -> R<()> {
    let _ = vm;
    with_arena(|ar| {
        if !matches!(ar.data(n), NodeData::Document(_) | NodeData::Doctype(_)) {
            ar.set_text_content(n, text);
        }
    });
    Ok(())
}

/// Wraps a list of nodes as a PHP array of objects.
pub(crate) fn wrap_all(vm: &mut Vm, ids: &[NodeId]) -> R<PhpArray> {
    let mut arr = PhpArray::new();
    for &id in ids {
        let v = wrap_node(vm, id)?;
        let _ = arr.push(v);
    }
    Ok(arr)
}

/// A new element name for `qualified` in namespace `ns` (no checks).
pub(crate) fn qname(ns: Option<&str>, qualified: &str) -> QName {
    match qualified.split_once(':') {
        Some((p, l)) if ns.is_some() => QName {
            prefix: Some(p.into()),
            local: l.into(),
            ns: ns.map(Into::into),
        },
        _ => QName {
            prefix: None,
            local: qualified.into(),
            ns: ns.map(Into::into),
        },
    }
}

/// True when node `n`'s document is the hidden one made for a node created
/// without a document.
pub(crate) fn in_hidden_doc(n: NodeId) -> bool {
    with_settings(n, |s| s.hidden)
}

/// Keeps `settings` linked (used by other modules).
pub(crate) fn strict(n: NodeId) -> bool {
    settings(n).strict_error_checking
}

/// The optional node argument as an object value.
pub(crate) fn opt_wrap(vm: &mut Vm, n: Option<NodeId>) -> R<Value> {
    wrap_opt(vm, n)
}
