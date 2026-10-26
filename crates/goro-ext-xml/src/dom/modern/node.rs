//! Dom\Node, Dom\ParentNode, Dom\ChildNode (node.c and parentnode/tree.c,
//! modern branches).

use super::*;
use crate::dom::node::{
    children_valid, get_content_value, hierarchy_ok, remove_all_children, str_of,
};
use crate::tree::NsId;

// ---------------------------------------------------------------------------
// Properties

/// dom_node_get_node_name_attribute_or_element
pub fn qname_of(t: &Tree, n: NodeId, uppercase: bool) -> String {
    let q = t.qname(n);
    if uppercase { q.to_ascii_uppercase() } else { q }
}

fn r_node_name(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| {
        let nd = &t.nodes[n];
        match nd.t {
            NType::Element => {
                let upper = ns_is_html_and_doc_is_html(t, n);
                s(&qname_of(t, n, upper))
            }
            NType::Attribute => s(&t.qname(n)),
            NType::NamespaceDecl => match t.ns_prefix(nd.ns) {
                Some(p) => s(&format!("xmlns:{}", p)),
                None => s(&nd.name),
            },
            NType::DocumentType
            | NType::Dtd
            | NType::Pi
            | NType::EntityDecl
            | NType::Entity
            | NType::EntityRef
            | NType::Notation => s(&nd.name),
            NType::CData => s("#cdata-section"),
            NType::Comment => s("#comment"),
            NType::Document | NType::HtmlDocument => s("#document"),
            NType::DocumentFrag => s("#document-fragment"),
            NType::Text => s("#text"),
            _ => s(&nd.name),
        }
    }))
}

/// dom_modern_node_base_uri_read
fn r_base_uri(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    // xmlNodeGetBase on an entity declaration returns its resolved URI
    let ent_uri = tree::with(|t| {
        if matches!(t.nodes[n].t, NType::EntityDecl | NType::Entity) {
            let sys = t.entity_info(n).and_then(|e| e.system_id.clone())?;
            let doc = t.nodes[n].doc;
            let base = doc.and_then(|d| t.doc_url(d));
            Some(crate::uri::build_uri(&sys, base.as_deref()).unwrap_or(sys))
        } else {
            None
        }
    });
    if let Some(u) = ent_uri {
        return Ok(s(&u));
    }
    Ok(tree::with(|t| {
        match crate::dom::node::node_get_base(t, n) {
            Some(b) => s(&b),
            None => {
                let doc = doc_from_node(t, n);
                match doc.and_then(|d| t.doc_url(d)) {
                    Some(u) => s(&u),
                    None => s("about:blank"),
                }
            }
        }
    }))
}

fn r_child_nodes(vm: &mut Vm, o: &Obj, _n: NodeId) -> R<Value> {
    Ok(mlist_obj(
        vm,
        b"Dom\\NodeList",
        ListKind::Children(o.clone()),
    ))
}

fn r_node_value(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| match t.nodes[n].t {
        NType::Attribute | NType::Text | NType::Comment | NType::CData | NType::Pi => {
            get_content_value(t, n, true)
        }
        NType::NamespaceDecl => s(&t.nodes[n].content),
        _ => Value::Null,
    }))
}

/// dom_node_node_value_write (modern)
fn w_node_value(_vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let st = str_of(v);
    tree::with(|t| match t.nodes[n].t {
        NType::Attribute => {
            element::attr_value_will_change(t, n);
            remove_all_children(t, n);
            let doc = t.nodes[n].doc;
            let tx = t.new_text(doc, &st);
            t.link_child_raw(n, tx);
        }
        NType::Element => {
            remove_all_children(t, n);
            t.node_set_content(n, &st);
        }
        NType::Text | NType::Comment | NType::CData | NType::Pi => t.node_set_content(n, &st),
        _ => {}
    });
    Ok(())
}

fn skip_text_content(t: &Tree, n: NodeId) -> bool {
    // document-less nodes keep the legacy behaviour (php_dom_follow_spec_intern)
    if t.nodes[n].doc.is_none() && !t.is_doc(n) {
        return false;
    }
    !matches!(
        t.nodes[n].t,
        NType::DocumentFrag
            | NType::Element
            | NType::Attribute
            | NType::Text
            | NType::CData
            | NType::Comment
            | NType::Pi
    )
}

fn r_text_content(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| {
        if skip_text_content(t, n) {
            Value::Null
        } else {
            get_content_value(t, n, false)
        }
    }))
}

fn w_text_content(_vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let st = str_of(v);
    tree::with(|t| {
        let ty = t.nodes[n].t;
        if matches!(ty, NType::DocumentFrag | NType::Element | NType::Attribute) {
            if ty == NType::Attribute {
                element::attr_value_will_change(t, n);
            }
            remove_all_children(t, n);
            let doc = t.nodes[n].doc;
            let tx = t.new_text(doc, &st);
            t.add_child(n, tx);
        } else {
            t.node_set_content(n, &st);
        }
    });
    Ok(())
}

pub fn node_value_rw(owner: &'static str) -> Prop {
    Prop {
        name: "nodeValue",
        read: r_node_value,
        write: Some(w_node_value),
        ty: "?string",
        owner,
    }
}

pub fn text_content_rw(owner: &'static str) -> Prop {
    Prop {
        name: "textContent",
        read: r_text_content,
        write: Some(w_text_content),
        ty: "?string",
        owner,
    }
}

pub fn node_props(t: &HashMap<Base, Rc<Vec<Prop>>>) -> Vec<Prop> {
    let l = |n: &str| {
        let mut p = lp(t, Base::Node, n);
        p.owner = "";
        p
    };
    vec![
        l("nodeType"),
        p("nodeName", r_node_name, None, "string"),
        p("baseURI", r_base_uri, None, "string"),
        l("isConnected"),
        l("ownerDocument"),
        l("parentNode"),
        l("parentElement"),
        p("childNodes", r_child_nodes, None, "mixed"),
        l("firstChild"),
        l("lastChild"),
        l("previousSibling"),
        l("nextSibling"),
        p("nodeValue", r_node_value, None, "?string"),
        p("textContent", r_text_content, None, "?string"),
    ]
}

fn r_children(vm: &mut Vm, o: &Obj, _n: NodeId) -> R<Value> {
    let base = o.clone();
    let c = cached_obj(o, "children", true, || {
        let v = mlist_obj(vm, b"Dom\\HTMLCollection", ListKind::ChildElements(base));
        match v {
            Value::Object(x) => x,
            _ => unreachable!(),
        }
    });
    Ok(Value::Object(c))
}

pub fn parent_props(t: &HashMap<Base, Rc<Vec<Prop>>>) -> Vec<Prop> {
    let l = |n: &str| {
        let mut p = lp(t, Base::DocumentFragment, n);
        p.owner = "";
        p
    };
    vec![
        p("children", r_children, None, "mixed"),
        l("firstElementChild"),
        l("lastElementChild"),
        l("childElementCount"),
    ]
}

// ---------------------------------------------------------------------------
// Clone

/// xmlDocCopyNode(src, doc, 0)-like shallow copy for the modern cloning
/// helper: names, content and namespace pointer are kept as is.
fn shallow_copy(t: &mut Tree, src: NodeId, doc: Option<NodeId>) -> NodeId {
    let sn = t.nodes[src].clone();
    let id = t.new_node(doc, sn.t, &sn.name);
    t.nodes[id].content = sn.content.clone();
    t.nodes[id].line = sn.line;
    t.nodes[id].ns = sn.ns;
    t.nodes[id].flags = sn.flags & tree::F_NULL_CONTENT;
    t.nodes[id].extra = match &sn.extra {
        tree::Extra::Doc(_) | tree::Extra::Dtd(_) => tree::Extra::None,
        e => e.clone(),
    };
    match sn.t {
        NType::EntityRef => {
            let ent = doc
                .and_then(|d| t.get_doc_entity(d, &sn.name))
                .or(sn.children);
            t.nodes[id].children = ent;
            t.nodes[id].last = ent;
        }
        NType::Attribute => {
            // xmlCopyProp copies the value
            let mut c = sn.children;
            while let Some(x) = c {
                let cc = deep_copy_leaf(t, x, doc);
                t.link_child_raw(id, cc);
                c = t.nodes[x].next;
            }
        }
        _ => {}
    }
    id
}

/// xmlDocCopyNode(src, doc, 1) for non-element nodes (text, comment, PI,
/// entity references, ...).
fn deep_copy_leaf(t: &mut Tree, src: NodeId, doc: Option<NodeId>) -> NodeId {
    if t.nodes[src].t == NType::Element {
        return t.copy_node(src, doc, 1, None);
    }
    shallow_copy(t, src, doc)
}

/// dom_clone_container_helper
fn container_copy(t: &mut Tree, mapper_doc: NodeId, src: NodeId, doc: Option<NodeId>) -> NodeId {
    if t.is_doc(src) {
        return t.copy_doc(src, false);
    }
    let c = shallow_copy(t, src, doc);
    if t.nodes[src].t == NType::Element {
        let defs = t.nodes[src].ns_def.clone();
        for ns in defs {
            let a = super::compat_attribute(t, mapper_doc, c, ns);
            t.link_child_raw(c, a);
        }
        for a in t.attrs_of(src) {
            let na = shallow_copy(t, a, doc);
            t.link_child_raw(c, na);
        }
    }
    c
}

/// dom_clone_node with a namespace mapper (modern documents). `mapper_doc`
/// is the document whose mapper receives the namespaces; for document
/// clones it is the new document.
pub fn clone_node(
    t: &mut Tree,
    src: NodeId,
    doc: Option<NodeId>,
    recursive: bool,
    mapper_doc: Option<NodeId>,
) -> NodeId {
    if t.nodes[src].t == NType::Dtd {
        return t.copy_node(src, doc, 1, None);
    }
    let src_doc = doc_from_node(t, src);
    let is_doc = t.is_doc(src);
    let outer = container_copy(t, mapper_doc.unwrap_or(src), src, doc);
    let dst_doc = if is_doc { Some(outer) } else { doc };
    let mdoc = if is_doc {
        outer
    } else {
        mapper_doc.or(doc).unwrap_or(outer)
    };
    if recursive
        && matches!(
            t.nodes[src].t,
            NType::Element | NType::DocumentFrag | NType::Document | NType::HtmlDocument
        )
    {
        if is_doc {
            if let Some(sub) = t.int_subset(src) {
                let c = t.copy_node(sub, Some(outer), 1, None);
                t.link_child_raw(outer, c);
                if let Some(di) = t.doc_info_mut(outer) {
                    di.int_subset = Some(c);
                }
            }
        }
        // iterative tree copy
        let mut stack: Vec<(NodeId, NodeId)> = vec![(src, outer)];
        while let Some((s_parent, c_parent)) = stack.pop() {
            let mut c = t.nodes[s_parent].children;
            let mut pending: Vec<(NodeId, NodeId)> = Vec::new();
            while let Some(x) = c {
                let cloned = match t.nodes[x].t {
                    NType::Element => Some(container_copy(t, mdoc, x, dst_doc)),
                    NType::Dtd => None,
                    _ => Some(deep_copy_leaf(t, x, dst_doc)),
                };
                if let Some(cl) = cloned {
                    t.link_child_raw(c_parent, cl);
                    if t.nodes[x].t == NType::Element && t.nodes[x].children.is_some() {
                        pending.push((x, cl));
                    }
                }
                c = t.nodes[x].next;
            }
            for p in pending.into_iter().rev() {
                stack.push(p);
            }
        }
    }
    let clone_doc = doc_from_node(t, outer);
    if clone_doc != src_doc {
        if let Some(md) = clone_doc.or(Some(mdoc)) {
            if matches!(
                t.nodes[outer].t,
                NType::Document | NType::HtmlDocument | NType::DocumentFrag
            ) {
                for c in t.children_of(outer) {
                    reconcile_modern(t, md, c);
                }
            } else {
                reconcile_modern(t, md, outer);
            }
        }
    }
    outer
}

/// Clone handler of modern node objects (dom_objects_store_clone_obj).
pub fn h_clone(vm: &mut Vm, src: &Obj, dst: &Obj) {
    let n = match node_of(src) {
        Some(n) => n,
        None => {
            if let Some(d) = data_of(src) {
                if !matches!(&*d.kind.borrow(), Kind::None) {
                    h_uncloneable(vm, src, dst);
                    return;
                }
            }
            attach_with(dst, Kind::None, &NODE_HANDLERS);
            return;
        }
    };
    let c = clone_document_or_node(n, true);
    attach_with(dst, Kind::Node(c), &NODE_HANDLERS);
    tree::with(|t| t.nodes[c].wrapper = Some(Rc::downgrade(dst)));
}

/// Clone a node (deep or not); clones of documents get new document state.
pub fn clone_document_or_node(n: NodeId, deep: bool) -> NodeId {
    let is_doc = tree::with(|t| t.is_doc(n));
    if is_doc {
        let c = tree::with(|t| clone_node(t, n, None, deep, None));
        let mut props = doc_props(Some(n));
        props.class_type = 2;
        set_doc_props(c, props);
        c
    } else {
        tree::with(|t| {
            let doc = t.nodes[n].doc;
            let c = clone_node(t, n, doc, deep, doc);
            if t.nodes[n].flags & tree::F_MODERN != 0 {
                t.nodes[c].flags |= tree::F_MODERN;
            }
            c
        })
    }
}

// ---------------------------------------------------------------------------
// Methods

fn this_node(vm: &mut Vm, o: &Obj) -> R<NodeId> {
    need_node(vm, o)
}

const NODE: &str = "Dom\\Node";

fn m_append_child(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let (co, child) = arg_node(vm, &a, 0, "node", NODE, false)?.unwrap();
    let parent = this_node(vm, o)?;
    if tree::with(|t| pre_insert_parent_invalid(t, parent)) {
        dom_err(vm, HIERARCHY_REQUEST_ERR);
        return Err(());
    }
    pre_insert(vm, Some(child), parent, None);
    if vm.current_exception.is_some() {
        return Err(());
    }
    Ok(Value::Object(co))
}

fn m_insert_before(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 2)?;
    let (co, child) = arg_node(vm, &a, 0, "node", NODE, false)?.unwrap();
    let rf = a.obj_of(vm, 1, "child", NODE, true)?;
    let parent = this_node(vm, o)?;
    if tree::with(|t| pre_insert_parent_invalid(t, parent)) {
        dom_err(vm, HIERARCHY_REQUEST_ERR);
        return Err(());
    }
    let refp = match rf {
        Some(r) => Some(need_node(vm, &r)?),
        None => None,
    };
    pre_insert(vm, Some(child), parent, refp);
    if vm.current_exception.is_some() {
        return Err(());
    }
    Ok(Value::Object(co))
}

fn has_sibling_following(t: &Tree, n: NodeId, ty: NType) -> bool {
    let mut x = t.nodes[n].next;
    while let Some(y) = x {
        if t.nodes[y].t == ty {
            return true;
        }
        x = t.nodes[y].next;
    }
    false
}

fn has_sibling_preceding(t: &Tree, n: NodeId, ty: NType) -> bool {
    let mut x = t.nodes[n].prev;
    while let Some(y) = x {
        if t.nodes[y].t == ty {
            return true;
        }
        x = t.nodes[y].prev;
    }
    false
}

fn has_child_of_type(t: &Tree, n: NodeId, ty: NType) -> bool {
    let mut x = t.nodes[n].children;
    while let Some(y) = x {
        if t.nodes[y].t == ty {
            return true;
        }
        x = t.nodes[y].next;
    }
    false
}

/// Err((code, Some(message)))
type HErr = (i64, Option<&'static str>);

const MSG_MORE_THAN_ONE_ELEMENT: &str = "Cannot have more than one element child in a document";
const MSG_TEXT_IN_DOC: &str = "Cannot insert text as a child of a document";
const MSG_DOCTYPE_FIRST: &str = "Document types must be the first child in a document";
const MSG_DOCTYPE_OUTSIDE: &str =
    "Cannot insert a document type into anything other than a document";

fn fragment_common_check(t: &Tree, node: NodeId) -> Result<bool, HErr> {
    let mut seen = false;
    let mut it = t.nodes[node].children;
    while let Some(x) = it {
        match t.nodes[x].t {
            NType::Element => {
                if seen {
                    return Err((HIERARCHY_REQUEST_ERR, Some(MSG_MORE_THAN_ONE_ELEMENT)));
                }
                seen = true;
            }
            NType::Text | NType::CData => {
                return Err((HIERARCHY_REQUEST_ERR, Some(MSG_TEXT_IN_DOC)));
            }
            _ => {}
        }
        it = t.nodes[x].next;
    }
    Ok(seen)
}

fn fragment_check_pre_insertion(
    t: &Tree,
    parent: NodeId,
    node: NodeId,
    child: Option<NodeId>,
) -> Result<(), HErr> {
    if fragment_common_check(t, node)? {
        if has_child_of_type(t, parent, NType::Element) {
            return Err((HIERARCHY_REQUEST_ERR, Some(MSG_MORE_THAN_ONE_ELEMENT)));
        }
        if let Some(c) = child {
            if t.nodes[c].t == NType::Dtd || has_sibling_following(t, c, NType::Dtd) {
                return Err((HIERARCHY_REQUEST_ERR, Some(MSG_DOCTYPE_FIRST)));
            }
        }
    }
    Ok(())
}

fn fragment_check_replace(
    t: &Tree,
    parent: NodeId,
    node: NodeId,
    child: NodeId,
) -> Result<(), HErr> {
    if fragment_common_check(t, node)? {
        let mut it = t.nodes[parent].children;
        while let Some(x) = it {
            if t.nodes[x].t == NType::Element && x != child {
                return Err((HIERARCHY_REQUEST_ERR, Some(MSG_MORE_THAN_ONE_ELEMENT)));
            }
            it = t.nodes[x].next;
        }
        if has_sibling_following(t, child, NType::Dtd) {
            return Err((HIERARCHY_REQUEST_ERR, Some(MSG_DOCTYPE_FIRST)));
        }
    }
    Ok(())
}

pub fn pre_insert_parent_invalid(t: &Tree, parent: NodeId) -> bool {
    !matches!(
        t.nodes[parent].t,
        NType::Document | NType::HtmlDocument | NType::Element | NType::DocumentFrag
    )
}

/// dom_is_pre_insert_valid_without_step_1 (modern flavour)
fn pre_insert_valid(
    t: &Tree,
    parent: NodeId,
    node: NodeId,
    child: Option<NodeId>,
    document: Option<NodeId>,
) -> Result<(), HErr> {
    if pre_insert_parent_invalid(t, parent) {
        return Err((HIERARCHY_REQUEST_ERR, None));
    }
    if t.nodes[node].doc != document {
        return Err((WRONG_DOCUMENT_ERR, None));
    }
    if let Some(c) = child {
        if t.nodes[c].parent != Some(parent) {
            return Err((NOT_FOUND_ERR, None));
        }
    }
    let nt = t.nodes[node].t;
    let parent_is_doc = t.is_doc(parent);
    if !hierarchy_ok(t, parent, node)
        || nt == NType::Attribute
        || matches!(
            nt,
            NType::EntityRef
                | NType::Entity
                | NType::Notation
                | NType::Document
                | NType::HtmlDocument
        )
        || (nt as u32) >= (NType::ElementDecl as u32)
    {
        return Err((HIERARCHY_REQUEST_ERR, None));
    }
    if parent_is_doc && matches!(nt, NType::Text | NType::CData) {
        return Err((HIERARCHY_REQUEST_ERR, Some(MSG_TEXT_IN_DOC)));
    }
    if !parent_is_doc && nt == NType::Dtd {
        return Err((HIERARCHY_REQUEST_ERR, Some(MSG_DOCTYPE_OUTSIDE)));
    }
    if parent_is_doc {
        match nt {
            NType::DocumentFrag => fragment_check_pre_insertion(t, parent, node, child)?,
            NType::Element => {
                if has_child_of_type(t, parent, NType::Element) {
                    return Err((HIERARCHY_REQUEST_ERR, Some(MSG_MORE_THAN_ONE_ELEMENT)));
                }
                if let Some(c) = child {
                    if t.nodes[c].t == NType::Dtd || has_sibling_following(t, c, NType::Dtd) {
                        return Err((HIERARCHY_REQUEST_ERR, Some(MSG_DOCTYPE_FIRST)));
                    }
                }
            }
            NType::Dtd => {
                if has_child_of_type(t, parent, NType::Dtd) {
                    return Err((
                        HIERARCHY_REQUEST_ERR,
                        Some("Cannot have more than one document type"),
                    ));
                }
                if (child.is_some() && has_sibling_preceding(t, child.unwrap(), NType::Element))
                    || (child.is_none() && has_child_of_type(t, parent, NType::Element))
                {
                    return Err((HIERARCHY_REQUEST_ERR, Some(MSG_DOCTYPE_FIRST)));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn report(vm: &mut Vm, e: HErr) {
    match e.1 {
        Some(m) => dom_err_msg(vm, e.0, m),
        None => dom_err(vm, e.0),
    }
}

fn pre_insert_helper(
    t: &mut Tree,
    insertion_point: Option<NodeId>,
    parent: NodeId,
    newchild: NodeId,
    last: NodeId,
) {
    t.touch();
    match insertion_point {
        None => {
            match t.nodes[parent].children {
                Some(_) => {
                    let pl = t.nodes[parent].last.unwrap();
                    t.nodes[newchild].prev = Some(pl);
                    t.nodes[pl].next = Some(newchild);
                }
                None => t.nodes[parent].children = Some(newchild),
            }
            t.nodes[parent].last = Some(last);
        }
        Some(ip) => {
            t.nodes[last].next = Some(ip);
            if let Some(pp) = t.nodes[ip].prev {
                t.nodes[pp].next = Some(newchild);
                t.nodes[newchild].prev = Some(pp);
            }
            t.nodes[ip].prev = Some(last);
            if t.nodes[parent].children == Some(ip) {
                t.nodes[parent].children = Some(newchild);
            }
        }
    }
}

/// dom_insert_node_list_unchecked (no namespace reconciliation)
fn insert_node_list_unchecked(
    t: &mut Tree,
    node: NodeId,
    parent: NodeId,
    mut insertion_point: Option<NodeId>,
) {
    t.touch();
    if t.nodes[node].t == NType::DocumentFrag {
        if let Some(newchild) = t.nodes[node].children {
            let last = t.nodes[node].last.unwrap();
            pre_insert_helper(t, insertion_point, parent, newchild, last);
            let mut n = Some(newchild);
            while let Some(x) = n {
                t.nodes[x].parent = Some(parent);
                if x == last {
                    break;
                }
                n = t.nodes[x].next;
            }
            if t.nodes[newchild].t == NType::Dtd {
                if let Some(d) = t.nodes[parent].doc {
                    if let Some(di) = t.doc_info_mut(d) {
                        di.int_subset = Some(newchild);
                    }
                }
            }
        }
        t.nodes[node].children = None;
        t.nodes[node].last = None;
    } else {
        if insertion_point == Some(node) {
            insertion_point = t.nodes[node].next;
        }
        t.unlink(node);
        pre_insert_helper(t, insertion_point, parent, node, node);
        t.nodes[node].parent = Some(parent);
        if t.nodes[node].t == NType::Dtd {
            if let Some(d) = t.nodes[parent].doc {
                if let Some(di) = t.doc_info_mut(d) {
                    di.int_subset = Some(node);
                }
            }
        }
    }
}

/// php_dom_pre_insert
pub fn pre_insert(
    vm: &mut Vm,
    node: Option<NodeId>,
    parent: NodeId,
    insertion_point: Option<NodeId>,
) -> bool {
    let node = match node {
        Some(n) => n,
        None => return false,
    };
    let r = tree::with(|t| {
        let pdoc = t.nodes[parent].doc;
        pre_insert_valid(t, parent, node, insertion_point, pdoc)
    });
    match r {
        Ok(()) => {
            tree::with(|t| insert_node_list_unchecked(t, node, parent, insertion_point));
            true
        }
        Err(e) => {
            report(vm, e);
            false
        }
    }
}

/// dom_replace_node_validity_checks
fn replace_validity(t: &Tree, parent: NodeId, node: NodeId, child: NodeId) -> Result<(), HErr> {
    if pre_insert_parent_invalid(t, parent) {
        return Err((HIERARCHY_REQUEST_ERR, None));
    }
    if !hierarchy_ok(t, parent, node) {
        return Err((HIERARCHY_REQUEST_ERR, None));
    }
    if t.nodes[child].parent != Some(parent) {
        return Err((NOT_FOUND_ERR, None));
    }
    let nt = t.nodes[node].t;
    if !matches!(
        nt,
        NType::DocumentFrag
            | NType::Dtd
            | NType::Element
            | NType::Text
            | NType::CData
            | NType::Comment
            | NType::Pi
    ) {
        return Err((HIERARCHY_REQUEST_ERR, None));
    }
    let parent_is_doc = t.is_doc(parent);
    if parent_is_doc && matches!(nt, NType::Text | NType::CData) {
        return Err((HIERARCHY_REQUEST_ERR, Some(MSG_TEXT_IN_DOC)));
    }
    if !parent_is_doc && nt == NType::Dtd {
        return Err((HIERARCHY_REQUEST_ERR, Some(MSG_DOCTYPE_OUTSIDE)));
    }
    if parent_is_doc {
        match nt {
            NType::DocumentFrag => fragment_check_replace(t, parent, node, child)?,
            NType::Element => {
                if t.root_element(parent) != Some(child) {
                    return Err((HIERARCHY_REQUEST_ERR, Some(MSG_MORE_THAN_ONE_ELEMENT)));
                }
                if has_sibling_following(t, child, NType::Dtd) {
                    return Err((HIERARCHY_REQUEST_ERR, Some(MSG_DOCTYPE_FIRST)));
                }
            }
            NType::Dtd => {
                if t.int_subset(parent) != Some(child)
                    || has_sibling_preceding(t, child, NType::Element)
                {
                    return Err((HIERARCHY_REQUEST_ERR, Some(MSG_DOCTYPE_FIRST)));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// xmlReplaceNode
fn replace_node(t: &mut Tree, old: NodeId, cur: NodeId) {
    if old == cur {
        return;
    }
    t.unlink(cur);
    t.insert_before_raw(old, cur);
    t.unlink(old);
}

fn m_replace_child(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let (_no, newchild) = arg_node(vm, &a, 0, "node", NODE, false)?.unwrap();
    let (oo, oldchild) = arg_node(vm, &a, 1, "child", NODE, false)?.unwrap();
    let parent = this_node(vm, o)?;
    let wrong = tree::with(|t| {
        t.nodes[newchild].doc != t.nodes[parent].doc && t.nodes[newchild].doc.is_some()
    });
    if wrong {
        dom_err(vm, WRONG_DOCUMENT_ERR);
        return Err(());
    }
    if let Err(e) = tree::with(|t| replace_validity(t, parent, newchild, oldchild)) {
        report(vm, e);
        return Err(());
    }
    tree::with(|t| {
        if t.nodes[newchild].doc.is_none() && t.nodes[parent].doc.is_some() {
            let d = t.nodes[parent].doc;
            t.set_tree_doc(newchild, d);
        }
        if t.nodes[newchild].t == NType::DocumentFrag {
            let prev = t.nodes[oldchild].prev;
            let next = t.nodes[oldchild].next;
            t.unlink(oldchild);
            insert_fragment(t, parent, prev, next, newchild);
        } else if oldchild != newchild {
            let doc = t.nodes[parent].doc;
            let replaced_doctype = doc.and_then(|d| t.int_subset(d)) == Some(oldchild);
            replace_node(t, oldchild, newchild);
            if replaced_doctype {
                if let Some(d) = doc {
                    if let Some(di) = t.doc_info_mut(d) {
                        di.int_subset = Some(newchild);
                    }
                }
            }
        }
    });
    Ok(Value::Object(oo))
}

/// dom_insert_fragment (without reconciliation)
fn insert_fragment(
    t: &mut Tree,
    parent: NodeId,
    prevsib: Option<NodeId>,
    nextsib: Option<NodeId>,
    frag: NodeId,
) {
    t.touch();
    let first = match t.nodes[frag].children {
        Some(f) => f,
        None => return,
    };
    let last = t.nodes[frag].last.unwrap();
    let mut n = Some(first);
    while let Some(x) = n {
        t.nodes[x].parent = Some(parent);
        if x == last {
            break;
        }
        n = t.nodes[x].next;
    }
    match prevsib {
        Some(p) => {
            t.nodes[p].next = Some(first);
            t.nodes[first].prev = Some(p);
        }
        None => {
            t.nodes[parent].children = Some(first);
            t.nodes[first].prev = None;
        }
    }
    match nextsib {
        Some(nx) => {
            t.nodes[nx].prev = Some(last);
            t.nodes[last].next = Some(nx);
        }
        None => {
            t.nodes[parent].last = Some(last);
            t.nodes[last].next = None;
        }
    }
    t.nodes[frag].children = None;
    t.nodes[frag].last = None;
}

fn m_remove_child(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let (co, child) = arg_node(vm, &a, 0, "child", NODE, false)?.unwrap();
    let parent = this_node(vm, o)?;
    let r = tree::with(|t| {
        if t.nodes[parent].children.is_none() || t.nodes[child].parent != Some(parent) {
            return Err(NOT_FOUND_ERR);
        }
        if crate::dom::node::is_read_only(t, parent)
            || t.nodes[child]
                .parent
                .map(|p| crate::dom::node::is_read_only(t, p))
                .unwrap_or(false)
        {
            return Err(NO_MODIFICATION_ALLOWED_ERR);
        }
        t.unlink(child);
        Ok(())
    });
    match r {
        Ok(()) => Ok(Value::Object(co)),
        Err(c) => {
            dom_err(vm, c);
            Err(())
        }
    }
}

fn m_has_child_nodes(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let n = this_node(vm, o)?;
    Ok(bv(tree::with(|t| {
        children_valid(t, n) && t.nodes[n].children.is_some()
    })))
}

fn m_clone_node(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 0, 1)?;
    let deep = a.bool(vm, 0, "deep", false)?;
    let n = this_node(vm, o)?;
    let c = clone_document_or_node(n, deep);
    Ok(node_obj(vm, c))
}

/// php_dom_normalize_modern
pub fn normalize(t: &mut Tree, this: NodeId) {
    let mut node = t.nodes[this].children;
    while let Some(x) = node {
        if t.nodes[x].t == NType::Text {
            if t.nodes[x].content.is_empty() {
                let next = t.nodes[x].next;
                t.unlink(x);
                node = next;
                continue;
            }
            let mut nx = t.nodes[x].next;
            while let Some(y) = nx {
                if t.nodes[y].t != NType::Text {
                    break;
                }
                let nn = t.nodes[y].next;
                let c = t.nodes[y].content.clone();
                t.nodes[x].content.push_str(&c);
                t.unlink(y);
                nx = nn;
            }
        } else if t.nodes[x].t == NType::Element {
            normalize(t, x);
        }
        node = t.nodes[x].next;
    }
}

fn m_normalize(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let n = this_node(vm, o)?;
    tree::with(|t| normalize(t, n));
    Ok(Value::Null)
}

fn m_is_same_node(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let other = a.obj_of(vm, 0, "otherNode", NODE, true)?;
    let other = match other {
        Some(x) => x,
        None => return Ok(Value::False),
    };
    let n = this_node(vm, o)?;
    let on = need_node(vm, &other)?;
    Ok(bv(n == on))
}

fn content_equal(t: &Tree, a: NodeId, b: NodeId) -> bool {
    t.node_get_content(a) == t.node_get_content(b)
}

fn ns_uri_eq(t: &Tree, a: NodeId, b: NodeId) -> bool {
    t.ns_href(t.nodes[a].ns) == t.ns_href(t.nodes[b].ns)
}

fn ns_prefix_eq(t: &Tree, a: NodeId, b: NodeId) -> bool {
    t.ns_prefix(t.nodes[a].ns) == t.ns_prefix(t.nodes[b].ns)
}

pub fn equal_attr(t: &Tree, a: NodeId, b: NodeId) -> bool {
    t.nodes[a].name == t.nodes[b].name && ns_uri_eq(t, a, b) && content_equal(t, a, b)
}

fn list(t: &Tree, first: Option<NodeId>) -> Vec<NodeId> {
    let mut v = Vec::new();
    let mut c = first;
    while let Some(x) = c {
        v.push(x);
        c = t.nodes[x].next;
    }
    v
}

/// php_dom_node_is_equal_node (spec compliant)
pub fn is_equal_node(t: &Tree, a: NodeId, b: NodeId) -> bool {
    let (na, nb) = (&t.nodes[a], &t.nodes[b]);
    if na.t != nb.t {
        return false;
    }
    match na.t {
        NType::Element => {
            if na.name != nb.name || !ns_prefix_eq(t, a, b) || !ns_uri_eq(t, a, b) {
                return false;
            }
            let (pa, pb) = (list(t, na.properties), list(t, nb.properties));
            if pa.len() != pb.len() {
                return false;
            }
            for x in &pa {
                if !pb.iter().any(|y| is_equal_node(t, *x, *y)) {
                    return false;
                }
            }
            let (ca, cb) = (list(t, na.children), list(t, nb.children));
            ca.len() == cb.len()
                && ca
                    .iter()
                    .zip(cb.iter())
                    .all(|(x, y)| is_equal_node(t, *x, *y))
        }
        NType::Dtd => {
            let (da, db) = (t.dtd_info(a), t.dtd_info(b));
            na.name == nb.name
                && da.and_then(|d| d.external_id.clone()) == db.and_then(|d| d.external_id.clone())
                && da.and_then(|d| d.system_id.clone()) == db.and_then(|d| d.system_id.clone())
        }
        NType::Pi => na.name == nb.name && na.content == nb.content,
        NType::Text | NType::Comment | NType::CData => na.content == nb.content,
        NType::Attribute => equal_attr(t, a, b),
        NType::EntityRef => na.name == nb.name,
        NType::EntityDecl | NType::Notation | NType::Entity => {
            let (ea, eb) = (t.entity_info(a), t.entity_info(b));
            let et = |e: Option<&tree::EntityInfo>| e.map(|x| x.etype as i32);
            et(ea) == et(eb)
                && na.name == nb.name
                && ea.and_then(|e| e.external_id.clone()) == eb.and_then(|e| e.external_id.clone())
                && ea.and_then(|e| e.system_id.clone()) == eb.and_then(|e| e.system_id.clone())
                && content_equal(t, a, b)
        }
        NType::NamespaceDecl => {
            t.ns_prefix(na.ns) == t.ns_prefix(nb.ns) && t.ns_href(na.ns) == t.ns_href(nb.ns)
        }
        NType::DocumentFrag | NType::HtmlDocument | NType::Document => {
            let (ca, cb) = (list(t, na.children), list(t, nb.children));
            ca.len() == cb.len()
                && ca
                    .iter()
                    .zip(cb.iter())
                    .all(|(x, y)| is_equal_node(t, *x, *y))
        }
        _ => false,
    }
}

fn m_is_equal_node(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let other = match a.obj_of(vm, 0, "otherNode", NODE, true)? {
        Some(x) => x,
        None => return Ok(Value::False),
    };
    let on = need_node(vm, &other)?;
    let n = this_node(vm, o)?;
    if n == on {
        return Ok(Value::True);
    }
    Ok(bv(tree::with(|t| is_equal_node(t, n, on))))
}

/// dom_locate_a_namespace_prefix
fn locate_prefix(t: &Tree, mut elem: NodeId, uri: &str) -> Option<String> {
    loop {
        if let Some(ns) = t.nodes[elem].ns {
            if t.nss[ns].prefix.is_some() && t.nss[ns].href == uri {
                return t.nss[ns].prefix.clone();
            }
        }
        for a in t.attrs_of(elem) {
            if let Some(ans) = t.nodes[a].ns {
                if t.nodes[a].children.is_some()
                    && t.nss[ans].prefix.as_deref() == Some("xmlns")
                    && t.node_get_content(a) == uri
                {
                    return Some(t.nodes[a].name.clone());
                }
            }
        }
        match t.nodes[elem].parent {
            Some(p) if t.nodes[p].t == NType::Element => elem = p,
            _ => return None,
        }
    }
}

fn m_lookup_prefix(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let uri = a.opt_str(vm, 0, "namespace")?;
    let n = this_node(vm, o)?;
    let uri = match uri {
        Some(u) if !u.is_empty() => u,
        _ => return Ok(Value::Null),
    };
    let r = tree::with(|t| {
        let lookup = match t.nodes[n].t {
            NType::Element => Some(n),
            NType::Document | NType::HtmlDocument => t.root_element(n),
            NType::Entity
            | NType::Notation
            | NType::DocumentFrag
            | NType::DocumentType
            | NType::Dtd => return None,
            _ => t.nodes[n].parent,
        };
        let l = lookup?;
        if t.nodes[l].t != NType::Element {
            return None;
        }
        locate_prefix(t, l, &uri)
    });
    Ok(opt_s(r.as_deref()))
}

/// dom_locate_a_namespace
pub fn locate_namespace(t: &Tree, node: NodeId, prefix: Option<&str>) -> Option<String> {
    match t.nodes[node].t {
        NType::Element => {
            if let Some(p) = prefix {
                if p.eq_ignore_ascii_case("xml") {
                    return Some(XML_XML_NAMESPACE.to_string());
                }
                if p.eq_ignore_ascii_case("xmlns") {
                    return Some(XMLNS_NS.to_string());
                }
            }
            let mut cur = node;
            loop {
                if let Some(ns) = t.nodes[cur].ns {
                    if t.nss[ns].prefix.as_deref() == prefix {
                        return Some(t.nss[ns].href.clone());
                    }
                }
                for a in t.attrs_of(cur) {
                    let ans = match t.nodes[a].ns {
                        Some(x) if t.nss[x].href == XMLNS_NS => x,
                        _ => continue,
                    };
                    let ap = t.nss[ans].prefix.as_deref();
                    let hit = match prefix {
                        Some(p) => ap == Some("xmlns") && t.nodes[a].name == p,
                        None => ap.is_none() && t.nodes[a].name == "xmlns",
                    };
                    if hit {
                        let v = t.node_get_content(a);
                        return if t.nodes[a].children.is_some() && !v.is_empty() {
                            Some(v)
                        } else {
                            None
                        };
                    }
                }
                match t.nodes[cur].parent {
                    Some(p) if t.nodes[p].t == NType::Element => cur = p,
                    _ => return None,
                }
            }
        }
        NType::Document | NType::HtmlDocument => {
            let r = t.root_element(node)?;
            locate_namespace(t, r, prefix)
        }
        NType::Dtd | NType::DocumentFrag => None,
        _ => match t.nodes[node].parent {
            Some(p) if t.nodes[p].t == NType::Element => locate_namespace(t, p, prefix),
            _ => None,
        },
    }
}

fn m_lookup_namespace_uri(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let prefix = a.opt_str(vm, 0, "prefix")?;
    let n = this_node(vm, o)?;
    let prefix = prefix.filter(|p| !p.is_empty());
    let r = tree::with(|t| locate_namespace(t, n, prefix.as_deref()));
    Ok(opt_s(r.as_deref()))
}

fn m_is_default_namespace(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let uri = a.opt_str(vm, 0, "namespace")?;
    let n = this_node(vm, o)?;
    let uri = uri.filter(|u| !u.is_empty());
    let r = tree::with(|t| locate_namespace(t, n, None));
    Ok(bv(uri == r))
}

fn m_contains(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let other = match a.obj_of(vm, 0, "other", NODE, true)? {
        Some(x) => x,
        None => return Ok(Value::False),
    };
    let on = need_node(vm, &other)?;
    let n = this_node(vm, o)?;
    Ok(bv(tree::with(|t| {
        let mut c = Some(on);
        while let Some(x) = c {
            if x == n {
                return true;
            }
            c = t.nodes[x].parent;
        }
        false
    })))
}

fn m_get_root_node(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 0, 1)?;
    if let Some(v) = a.raw(0) {
        if !matches!(v, Value::Array(_) | Value::Null) {
            a.type_err(vm, 0, "options", "?array", &v);
            return Err(());
        }
    }
    let n = this_node(vm, o)?;
    let r = tree::with(|t| {
        let mut x = n;
        while let Some(p) = t.nodes[x].parent {
            x = p;
        }
        x
    });
    Ok(node_obj(vm, r))
}

fn m_compare_document_position(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    const DISCONNECTED: i64 = 1;
    const PRECEDING: i64 = 2;
    const FOLLOWING: i64 = 4;
    const CONTAINS: i64 = 8;
    const CONTAINED_BY: i64 = 16;
    const IMPL: i64 = 32;
    let a = args(a);
    a.count(vm, 1, 1)?;
    let (oo, other) = arg_node(vm, &a, 0, "other", NODE, false)?.unwrap();
    let this = this_node(vm, o)?;
    if this == other {
        return Ok(Value::Long(0));
    }
    let this_id = o.borrow().object_id;
    let other_id = oo.borrow().object_id;
    let r = tree::with(|t| {
        let mut node1 = Some(other);
        let mut node2 = Some(this);
        let mut attr1 = None;
        let mut attr2 = None;
        if t.nodes[other].t == NType::Attribute {
            attr1 = Some(other);
            node1 = t.nodes[other].parent;
        }
        if t.nodes[this].t == NType::Attribute {
            attr2 = Some(this);
            node2 = t.nodes[this].parent;
            if let (Some(a1), Some(n1)) = (attr1, node1) {
                if node2 == Some(n1) {
                    for at in t.attrs_of(n1) {
                        if equal_attr(t, at, a1) {
                            return IMPL | PRECEDING;
                        } else if equal_attr(t, at, this) {
                            return IMPL | FOLLOWING;
                        }
                    }
                }
            }
        }
        let disconnected = |n1: Option<NodeId>, n2: Option<NodeId>| -> i64 {
            let ord = if n1 == n2 {
                if other_id < this_id {
                    PRECEDING
                } else {
                    FOLLOWING
                }
            } else if n1 < n2 {
                PRECEDING
            } else {
                FOLLOWING
            };
            DISCONNECTED | IMPL | ord
        };
        let (n1, n2) = match (node1, node2) {
            (Some(a), Some(b)) => (a, b),
            _ => return disconnected(node1, node2),
        };
        let mut n2_anc_of_n1 = false;
        let mut d1 = 0usize;
        let mut r1 = n1;
        while let Some(p) = t.nodes[r1].parent {
            r1 = p;
            if r1 == n2 {
                n2_anc_of_n1 = true;
            }
            d1 += 1;
        }
        let mut n1_anc_of_n2 = false;
        let mut d2 = 0usize;
        let mut r2 = n2;
        while let Some(p) = t.nodes[r2].parent {
            r2 = p;
            if r2 == n1 {
                n1_anc_of_n2 = true;
            }
            d2 += 1;
        }
        if r1 != r2 {
            return disconnected(Some(n1), Some(n2));
        }
        if (n1_anc_of_n2 && attr1.is_none()) || (n1 == n2 && attr2.is_some()) {
            return CONTAINS | PRECEDING;
        }
        if (n2_anc_of_n1 && attr2.is_none()) || (n1 == n2 && attr1.is_some()) {
            return CONTAINED_BY | FOLLOWING;
        }
        if n1_anc_of_n2 {
            return PRECEDING;
        } else if n2_anc_of_n1 {
            return FOLLOWING;
        }
        let (mut x1, mut x2) = (n1, n2);
        while d1 > d2 {
            x1 = t.nodes[x1].parent.unwrap();
            d1 -= 1;
        }
        while d2 > d1 {
            x2 = t.nodes[x2].parent.unwrap();
            d2 -= 1;
        }
        while t.nodes[x1].parent != t.nodes[x2].parent {
            x1 = t.nodes[x1].parent.unwrap();
            x2 = t.nodes[x2].parent.unwrap();
        }
        let mut c = t.nodes[x1].next;
        while let Some(x) = c {
            if x == x2 {
                return PRECEDING;
            }
            c = t.nodes[x].next;
        }
        FOLLOWING
    });
    Ok(Value::Long(r))
}

fn c14n_attached(vm: &mut Vm, o: &Obj) -> R<()> {
    let n = need_node(vm, o)?;
    let attached = tree::with(|t| {
        let mut x = n;
        while let Some(p) = t.nodes[x].parent {
            x = p;
        }
        t.is_doc(x)
    });
    if !attached {
        dom_err_msg(
            vm,
            HIERARCHY_REQUEST_ERR,
            "Canonicalization can only happen on nodes attached to a document.",
        );
        return Err(());
    }
    Ok(())
}

/// What dom_relink_ns_decls changed on one element.
struct Relinked {
    elem: NodeId,
    attrs: Vec<NodeId>,
    ns: Option<NsId>,
}

/// dom_relink_ns_decls: make the xmlns attributes of a modern document
/// visible as namespace declarations for the canonicalizer.
fn relink_ns_decls(t: &mut Tree, root: NodeId) -> Vec<Relinked> {
    t.touch();
    let mut out = Vec::new();
    let mut cur = Some(root);
    while let Some(x) = cur {
        if t.nodes[x].t == NType::Element {
            let attrs = t.attrs_of(x);
            let mut keep = Vec::new();
            for &at in &attrs {
                let is_xmlns = t.nodes[at]
                    .ns
                    .map(|n| t.nss[n].href == XMLNS_NS)
                    .unwrap_or(false);
                if is_xmlns {
                    let href = t.node_get_content(at);
                    let prefix = if t.nodes[at]
                        .ns
                        .and_then(|n| t.nss[n].prefix.clone())
                        .is_some()
                    {
                        Some(t.nodes[at].name.clone())
                    } else {
                        None
                    };
                    let ns = t.new_ns_raw(&href, prefix.as_deref(), Some(x));
                    t.nodes[x].ns_def.insert(0, ns);
                } else {
                    keep.push(at);
                }
            }
            if keep.len() != attrs.len() {
                for &at in &attrs {
                    t.unlink(at);
                }
                t.nodes[x].properties = None;
                for &at in &keep {
                    t.link_child_raw(x, at);
                }
            }
            let old_ns = t.nodes[x].ns;
            if old_ns.map(|n| t.nss[n].prefix.is_none()).unwrap_or(false) {
                t.nodes[x].ns = t.search_ns(x, None);
            }
            out.push(Relinked {
                elem: x,
                attrs,
                ns: old_ns,
            });
        }
        cur = next_in_tree_order(t, x, Some(root));
    }
    out
}

/// dom_unlink_ns_decls
fn unlink_ns_decls(t: &mut Tree, saved: Vec<Relinked>) {
    t.touch();
    for r in saved {
        t.nodes[r.elem].ns_def.clear();
        t.nodes[r.elem].ns = r.ns;
        for at in t.attrs_of(r.elem) {
            t.unlink(at);
        }
        t.nodes[r.elem].properties = None;
        for at in r.attrs {
            t.link_child_raw(r.elem, at);
        }
    }
}

fn c14n_common(vm: &mut Vm, o: &Obj, a: &[Value], file: bool) -> R<Value> {
    c14n_attached(vm, o)?;
    let n = need_node(vm, o)?;
    let root = tree::with(|t| doc_from_node(t, n).and_then(|d| t.root_element(d)));
    let saved = match root {
        Some(r) => tree::with(|t| relink_ns_decls(t, r)),
        None => Vec::new(),
    };
    let r = crate::dom::document::c14n(vm, o, a, file);
    tree::with(|t| unlink_ns_decls(t, saved));
    r
}

fn m_c14n(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    c14n_common(vm, o, a, false)
}

fn m_c14n_file(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    c14n_common(vm, o, a, true)
}

pub fn node_methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("getRootNode", m_get_root_node as MethodFn),
        ("hasChildNodes", m_has_child_nodes),
        ("normalize", m_normalize),
        ("cloneNode", m_clone_node),
        ("isEqualNode", m_is_equal_node),
        ("isSameNode", m_is_same_node),
        ("compareDocumentPosition", m_compare_document_position),
        ("contains", m_contains),
        ("lookupPrefix", m_lookup_prefix),
        ("lookupNamespaceURI", m_lookup_namespace_uri),
        ("isDefaultNamespace", m_is_default_namespace),
        ("insertBefore", m_insert_before),
        ("appendChild", m_append_child),
        ("replaceChild", m_replace_child),
        ("removeChild", m_remove_child),
        ("getLineNo", legacy_method(Base::Node, "getLineNo")),
        ("getNodePath", legacy_method(Base::Node, "getNodePath")),
        ("C14N", m_c14n),
        ("C14NFile", m_c14n_file),
        ("__sleep", legacy_method(Base::Node, "__sleep")),
        ("__wakeup", legacy_method(Base::Node, "__wakeup")),
    ]
}

// ---------------------------------------------------------------------------
// ParentNode / ChildNode

/// dom_sanity_check_node_list_types
fn node_list_args(vm: &mut Vm, a: &[Value]) -> R<Vec<Value>> {
    let f = cur_method();
    let mut out = Vec::new();
    for (i, v) in a.iter().enumerate() {
        let v = v.deref();
        match &v {
            Value::Object(o) if obj_is(vm, o, NODE) => out.push(v.clone()),
            Value::String(_) => out.push(v.clone()),
            other => {
                let msg = format!(
                    "{}(): Argument #{} must be of type Dom\\Node|string, {} given",
                    f,
                    i + 1,
                    crate::dom::type_name_of(other)
                );
                throw_error(vm, b"TypeError", &msg);
                return Err(());
            }
        }
    }
    Ok(out)
}

fn in_list(nodes: &[Value], n: NodeId) -> bool {
    nodes.iter().any(|v| {
        if let Value::Object(o) = v {
            node_of(o) == Some(n)
        } else {
            false
        }
    })
}

/// dom_zvals_to_single_node
fn zvals_to_single_node(vm: &mut Vm, context: NodeId, nodes: &[Value]) -> R<Option<NodeId>> {
    let document = tree::with(|t| doc_from_node(t, context));
    if nodes.len() == 1 {
        return match &nodes[0] {
            Value::Object(o) => match node_of(o) {
                Some(n) => Ok(Some(n)),
                None => {
                    dom_err(vm, INVALID_STATE_ERR);
                    Err(())
                }
            },
            v => {
                let st = str_of(v);
                Ok(Some(tree::with(|t| t.new_text(document, &st))))
            }
        };
    }
    let frag = tree::with(|t| t.new_fragment(document));
    for v in nodes {
        match v {
            Value::Object(o) => {
                let nn = match node_of(o) {
                    Some(n) => n,
                    None => {
                        dom_err(vm, INVALID_STATE_ERR);
                        return Err(());
                    }
                };
                if let Err(e) = tree::with(|t| pre_insert_valid(t, frag, nn, None, document)) {
                    report(vm, e);
                    return Err(());
                }
                tree::with(|t| {
                    if t.nodes[nn].parent.is_some() {
                        t.unlink(nn);
                    }
                    if t.nodes[nn].t == NType::DocumentFrag {
                        for c in t.children_of(nn) {
                            t.unlink(c);
                            t.link_child_raw(frag, c);
                        }
                    } else {
                        t.link_child_raw(frag, nn);
                    }
                });
            }
            v => {
                let st = str_of(v);
                tree::with(|t| {
                    let tx = t.new_text(document, &st);
                    t.link_child_raw(frag, tx);
                });
            }
        }
    }
    Ok(Some(frag))
}

pub fn parent_append(vm: &mut Vm, parent: NodeId, a: &[Value]) -> R<()> {
    let nodes = node_list_args(vm, a)?;
    if let Some(node) = zvals_to_single_node(vm, parent, &nodes)? {
        pre_insert(vm, Some(node), parent, None);
    }
    Ok(())
}

fn m_append(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let parent = this_node(vm, o)?;
    parent_append(vm, parent, a)?;
    Ok(Value::Null)
}

fn m_prepend(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let parent = this_node(vm, o)?;
    if tree::with(|t| t.nodes[parent].children.is_none()) {
        return m_append(vm, o, a);
    }
    let nodes = node_list_args(vm, a)?;
    if let Some(node) = zvals_to_single_node(vm, parent, &nodes)? {
        let first = tree::with(|t| t.nodes[parent].children);
        pre_insert(vm, Some(node), parent, first);
    }
    Ok(Value::Null)
}

fn m_replace_children(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let nodes = node_list_args(vm, a)?;
    let parent = this_node(vm, o)?;
    if let Some(node) = zvals_to_single_node(vm, parent, &nodes)? {
        let r = tree::with(|t| {
            let pdoc = t.nodes[parent].doc;
            pre_insert_valid(t, parent, node, None, pdoc)
        });
        match r {
            Ok(()) => {
                tree::with(|t| remove_all_children(t, parent));
                pre_insert(vm, Some(node), parent, None);
            }
            Err(e) => report(vm, e),
        }
    }
    Ok(Value::Null)
}

fn m_query_selector(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let sel = a.str(vm, 0, "selectors")?;
    let n = this_node(vm, o)?;
    let r = super::css::select(vm, n, &sel, true)?;
    Ok(node_obj_opt(vm, r.first().copied()))
}

fn m_query_selector_all(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let sel = a.str(vm, 0, "selectors")?;
    let n = this_node(vm, o)?;
    let r = super::css::select(vm, n, &sel, false)?;
    let mut items = Vec::new();
    for x in r {
        items.push(node_obj(vm, x));
    }
    Ok(mlist_obj(vm, b"Dom\\NodeList", ListKind::Static(items)))
}

pub fn parent_methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("append", m_append as MethodFn),
        ("prepend", m_prepend),
        ("replaceChildren", m_replace_children),
        ("querySelector", m_query_selector),
        ("querySelectorAll", m_query_selector_all),
    ]
}

fn m_after(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let nodes = node_list_args(vm, a)?;
    let this = this_node(vm, o)?;
    let parent = match tree::with(|t| t.nodes[this].parent) {
        Some(p) => p,
        None => return Ok(Value::Null),
    };
    let mut viable = tree::with(|t| t.nodes[this].next);
    while let Some(v) = viable {
        if !in_list(&nodes, v) {
            break;
        }
        viable = tree::with(|t| t.nodes[v].next);
    }
    let node = zvals_to_single_node(vm, parent, &nodes)?;
    pre_insert(vm, node, parent, viable);
    Ok(Value::Null)
}

fn m_before(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let nodes = node_list_args(vm, a)?;
    let this = this_node(vm, o)?;
    let parent = match tree::with(|t| t.nodes[this].parent) {
        Some(p) => p,
        None => return Ok(Value::Null),
    };
    let mut viable = tree::with(|t| t.nodes[this].prev);
    while let Some(v) = viable {
        if !in_list(&nodes, v) {
            break;
        }
        viable = tree::with(|t| t.nodes[v].prev);
    }
    let node = zvals_to_single_node(vm, parent, &nodes)?;
    let ip = match viable {
        None => tree::with(|t| t.nodes[parent].children),
        Some(v) => tree::with(|t| t.nodes[v].next),
    };
    pre_insert(vm, node, parent, ip);
    Ok(Value::Null)
}

fn removal_preconditions(vm: &mut Vm, child: NodeId) -> bool {
    let r = tree::with(|t| {
        if crate::dom::node::is_read_only(t, child)
            || t.nodes[child]
                .parent
                .map(|p| crate::dom::node::is_read_only(t, p))
                .unwrap_or(false)
        {
            return Err(NO_MODIFICATION_ALLOWED_ERR);
        }
        if t.nodes[child].parent.is_none() {
            return Err(NOT_FOUND_ERR);
        }
        Ok(())
    });
    match r {
        Ok(()) => true,
        Err(c) => {
            dom_err(vm, c);
            false
        }
    }
}

fn m_remove(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let this = this_node(vm, o)?;
    if !removal_preconditions(vm, this) {
        return Ok(Value::Null);
    }
    tree::with(|t| t.unlink(this));
    Ok(Value::Null)
}

fn m_replace_with(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let nodes = node_list_args(vm, a)?;
    let child = this_node(vm, o)?;
    let parent = match tree::with(|t| t.nodes[child].parent) {
        Some(p) => p,
        None => return Ok(Value::Null),
    };
    let mut viable = tree::with(|t| t.nodes[child].next);
    while let Some(v) = viable {
        if !in_list(&nodes, v) {
            break;
        }
        viable = tree::with(|t| t.nodes[v].next);
    }
    if !removal_preconditions(vm, child) {
        return Ok(Value::Null);
    }
    if let Some(node) = zvals_to_single_node(vm, parent, &nodes)? {
        let r = tree::with(|t| {
            let pdoc = t.nodes[parent].doc;
            pre_insert_valid(t, parent, node, viable, pdoc)
        });
        match r {
            Ok(()) => tree::with(|t| {
                if t.nodes[child].parent != Some(node) {
                    t.unlink(child);
                }
                insert_node_list_unchecked(t, node, parent, viable);
            }),
            Err(e) => report(vm, e),
        }
    }
    Ok(Value::Null)
}

pub fn child_methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("remove", m_remove as MethodFn),
        ("before", m_before),
        ("after", m_after),
        ("replaceWith", m_replace_with),
    ]
}
