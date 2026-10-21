//! DOMNode (node.c), DOMParentNode/DOMChildNode (parentnode/tree.c).

use super::*;
use crate::tree::{NsId, XML_XML_NAMESPACE};

// ---------------------------------------------------------------------------
// Tree helpers

pub fn is_read_only(t: &Tree, n: NodeId) -> bool {
    match t.nodes[n].t {
        NType::EntityRef
        | NType::Entity
        | NType::DocumentType
        | NType::Notation
        | NType::Dtd
        | NType::ElementDecl
        | NType::AttributeDecl
        | NType::EntityDecl
        | NType::NamespaceDecl => true,
        _ => t.nodes[n].doc.is_none(),
    }
}

pub fn children_valid(t: &Tree, n: NodeId) -> bool {
    !matches!(
        t.nodes[n].t,
        NType::DocumentType
            | NType::Dtd
            | NType::Pi
            | NType::Comment
            | NType::Text
            | NType::CData
            | NType::Notation
    )
}

/// dom_hierarchy: SUCCESS (true) when `child` may be inserted under `parent`.
pub fn hierarchy_ok(t: &Tree, parent: NodeId, child: NodeId) -> bool {
    if t.nodes[child].doc != t.nodes[parent].doc {
        return true;
    }
    if t.nodes[child].t == NType::Document {
        return false;
    }
    let mut cur = Some(parent);
    while let Some(c) = cur {
        if c == child {
            return false;
        }
        cur = t.nodes[c].parent;
    }
    true
}

/// Next node in tree order within `base` (php_dom_next_in_tree_order).
pub fn next_in_tree(t: &Tree, node: NodeId, base: NodeId) -> Option<NodeId> {
    if t.nodes[node].t != NType::EntityRef && t.nodes[node].t != NType::Attribute {
        if let Some(c) = t.nodes[node].children {
            return Some(c);
        }
    }
    let mut cur = node;
    loop {
        if cur == base {
            return None;
        }
        if let Some(n) = t.nodes[cur].next {
            return Some(n);
        }
        cur = t.nodes[cur].parent?;
    }
}

/// xmlSetNs + reconciliation helpers

/// xmlNewReconciledNs
fn new_reconciled_ns(t: &mut Tree, tree_root: NodeId, ns: NsId) -> Option<NsId> {
    if t.nodes[tree_root].t != NType::Element {
        return None;
    }
    let href = t.nss[ns].href.clone();
    if let Some(d) = t.search_ns_by_href(tree_root, &href) {
        return Some(d);
    }
    let base = t.nss[ns]
        .prefix
        .clone()
        .unwrap_or_else(|| "default".to_string());
    let mut prefix = base.clone();
    let mut counter = 1;
    while t.search_ns(tree_root, Some(&prefix)).is_some() {
        if counter > 1000 {
            return None;
        }
        prefix = format!("{}{}", base, counter);
        counter += 1;
    }
    t.new_ns(Some(tree_root), &href, Some(&prefix))
}

/// xmlReconciliateNs
pub fn reconciliate_ns(t: &mut Tree, root: NodeId) {
    if t.nodes[root].t != NType::Element {
        return;
    }
    let mut cache: Vec<(NsId, NsId)> = Vec::new();
    let mut fix = |t: &mut Tree, n: NodeId, cache: &mut Vec<(NsId, NsId)>| {
        if let Some(ns) = t.nodes[n].ns {
            if let Some((_, newns)) = cache.iter().find(|(o, _)| *o == ns) {
                t.nodes[n].ns = Some(*newns);
            } else if let Some(newns) = new_reconciled_ns(t, root, ns) {
                cache.push((ns, newns));
                t.nodes[n].ns = Some(newns);
            }
        }
    };
    let mut node = Some(root);
    while let Some(n) = node {
        fix(t, n, &mut cache);
        if t.nodes[n].t == NType::Element {
            let attrs = t.attrs_of(n);
            for a in attrs {
                fix(t, a, &mut cache);
            }
        }
        // next
        if t.nodes[n].children.is_some() && t.nodes[n].t != NType::EntityRef {
            node = t.nodes[n].children;
        } else if n != root && t.nodes[n].next.is_some() {
            node = t.nodes[n].next;
        } else if n != root {
            let mut cur = n;
            node = None;
            loop {
                match t.nodes[cur].parent {
                    Some(p) => cur = p,
                    None => break,
                }
                if cur == root {
                    break;
                }
                if let Some(nx) = t.nodes[cur].next {
                    node = Some(nx);
                    break;
                }
            }
        } else {
            break;
        }
    }
}

/// dom_reconcile_ns_internal: drop nsDef entries already declared in the
/// new context (moving them to the document's old-ns list).
fn reconcile_ns_internal(t: &mut Tree, node: NodeId, search_parent: Option<NodeId>) {
    let defs = t.nodes[node].ns_def.clone();
    if defs.is_empty() {
        return;
    }
    let sp = match search_parent {
        Some(p) => p,
        None => return,
    };
    let mut keep = Vec::new();
    for ns in defs {
        let href = t.nss[ns].href.clone();
        let prefix = t.nss[ns].prefix.clone();
        let found = t.search_ns_by_href(sp, &href);
        let drop = match found {
            Some(f) => prefix.is_none() || t.nss[f].prefix == prefix,
            None => false,
        };
        if drop {
            t.nss[ns].owner = None;
        } else {
            keep.push(ns);
        }
    }
    t.nodes[node].ns_def = keep;
}

pub fn reconcile_ns(t: &mut Tree, node: NodeId) {
    if t.nodes[node].t == NType::Element {
        let p = t.nodes[node].parent;
        reconcile_ns_internal(t, node, p);
        reconciliate_ns(t, node);
    }
}

fn reconcile_ns_list_internal(
    t: &mut Tree,
    first: NodeId,
    last: NodeId,
    search_parent: Option<NodeId>,
) {
    let mut n = first;
    loop {
        if t.nodes[n].t == NType::Element {
            reconcile_ns_internal(t, n, search_parent);
            if let (Some(c), Some(l)) = (t.nodes[n].children, t.nodes[n].last) {
                reconcile_ns_list_internal(t, c, l, search_parent);
            }
        }
        if n == last {
            break;
        }
        match t.nodes[n].next {
            Some(x) => n = x,
            None => break,
        }
    }
}

pub fn reconcile_ns_list(t: &mut Tree, first: NodeId, last: NodeId) {
    let sp = t.nodes[first].parent;
    reconcile_ns_list_internal(t, first, last, sp);
    let mut n = first;
    loop {
        reconciliate_ns(t, n);
        if n == last {
            break;
        }
        match t.nodes[n].next {
            Some(x) => n = x,
            None => break,
        }
    }
}

/// dom_insert_fragment: move all children of `frag` between prev and next.
pub fn insert_fragment(
    t: &mut Tree,
    parent: NodeId,
    prevsib: Option<NodeId>,
    nextsib: Option<NodeId>,
    frag: NodeId,
) -> Option<NodeId> {
    let newchild = t.nodes[frag].children?;
    let flast = t.nodes[frag].last.unwrap();
    match prevsib {
        None => t.nodes[parent].children = Some(newchild),
        Some(p) => t.nodes[p].next = Some(newchild),
    }
    t.nodes[newchild].prev = prevsib;
    match nextsib {
        None => t.nodes[parent].last = Some(flast),
        Some(nx) => {
            t.nodes[flast].next = Some(nx);
            t.nodes[nx].prev = Some(flast);
        }
    }
    let mut n = Some(newchild);
    while let Some(x) = n {
        t.nodes[x].parent = Some(parent);
        if x == flast {
            break;
        }
        n = t.nodes[x].next;
    }
    t.nodes[frag].children = None;
    t.nodes[frag].last = None;
    Some(newchild)
}

/// xmlReplaceNode(old, cur)
pub fn replace_node(t: &mut Tree, old: NodeId, cur: NodeId) {
    if old == cur {
        return;
    }
    t.unlink(cur);
    let doc = t.nodes[old].doc;
    if t.nodes[cur].doc != doc {
        t.set_tree_doc(cur, doc);
    }
    let parent = t.nodes[old].parent;
    let prev = t.nodes[old].prev;
    let next = t.nodes[old].next;
    t.nodes[cur].parent = parent;
    t.nodes[cur].prev = prev;
    t.nodes[cur].next = next;
    if let Some(p) = prev {
        t.nodes[p].next = Some(cur);
    }
    if let Some(n) = next {
        t.nodes[n].prev = Some(cur);
    }
    if let Some(par) = parent {
        if t.nodes[cur].t == NType::Attribute {
            if t.nodes[par].properties == Some(old) {
                t.nodes[par].properties = Some(cur);
            }
        } else {
            if t.nodes[par].children == Some(old) {
                t.nodes[par].children = Some(cur);
            }
            if t.nodes[par].last == Some(old) {
                t.nodes[par].last = Some(cur);
            }
        }
    }
    t.nodes[old].parent = None;
    t.nodes[old].prev = None;
    t.nodes[old].next = None;
}

/// dom_clone_node (legacy): xmlDocCopyNode with DTD handling.
pub fn clone_node(n: NodeId, doc: Option<NodeId>, recursive: bool) -> Option<NodeId> {
    tree::with(|t| {
        if t.nodes[n].t == NType::Dtd {
            let c = t.copy_node(n, doc, 1, None);
            return Some(c);
        }
        if t.is_doc(n) {
            return Some(t.copy_doc(n, recursive));
        }
        let c = t.copy_node(n, doc, if recursive { 1 } else { 2 }, None);
        Some(c)
    })
}

pub fn get_content_value(t: &Tree, n: NodeId, null_on_failure: bool) -> Value {
    let nd = &t.nodes[n];
    match nd.t {
        NType::Text | NType::CData | NType::Pi | NType::Comment => {
            Value::String(PhpString::from_string(nd.content.clone()))
        }
        NType::Attribute => Value::String(PhpString::from_string(t.node_get_content(n))),
        _ => {
            let c = t.node_get_content(n);
            if c.is_empty()
                && null_on_failure
                && !matches!(
                    nd.t,
                    NType::Element
                        | NType::DocumentFrag
                        | NType::Document
                        | NType::HtmlDocument
                        | NType::EntityRef
                        | NType::EntityDecl
                )
            {
                Value::Null
            } else {
                Value::String(PhpString::from_string(c))
            }
        }
    }
}

pub fn s(v: &str) -> Value {
    Value::String(PhpString::from_string(v.to_string()))
}

pub fn opt_s(v: Option<&str>) -> Value {
    match v {
        Some(x) => s(x),
        None => Value::Null,
    }
}

pub fn b(v: bool) -> Value {
    if v { Value::True } else { Value::False }
}

pub fn str_of(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        o => o.to_php_string().to_string_lossy(),
    }
}

// ---------------------------------------------------------------------------
// Properties

fn r_node_name(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| {
        let nd = &t.nodes[n];
        match nd.t {
            NType::Element | NType::Attribute => s(&t.qname(n)),
            NType::NamespaceDecl => {
                let p = t.ns_prefix(nd.ns);
                match p {
                    Some(p) => s(&format!("xmlns:{}", p)),
                    None => s("xmlns"),
                }
            }
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

fn r_node_value(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| match t.nodes[n].t {
        NType::Element
        | NType::Attribute
        | NType::Text
        | NType::Comment
        | NType::CData
        | NType::Pi => get_content_value(t, n, true),
        NType::NamespaceDecl => s(&t.nodes[n].content),
        _ => Value::Null,
    }))
}

fn w_node_value(_vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let st = str_of(v);
    tree::with(|t| match t.nodes[n].t {
        NType::Attribute | NType::Element => {
            t.remove_id_for_attr_if_attr(n);
            remove_all_children(t, n);
            t.node_set_content(n, &st);
            let _ = 0;
        }
        NType::Text | NType::Comment | NType::CData | NType::Pi => t.node_set_content(n, &st),
        _ => {}
    });
    Ok(())
}

pub fn remove_all_children(t: &mut Tree, n: NodeId) {
    for c in t.children_of(n) {
        t.unlink(c);
    }
    t.nodes[n].children = None;
    t.nodes[n].last = None;
}

fn r_node_type(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| {
        let ty = t.nodes[n].t;
        Value::Long(if ty == NType::Dtd { 10 } else { ty.code() })
    }))
}

fn r_parent_node(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    let p = tree::with(|t| t.nodes[n].parent);
    Ok(node_obj_opt(vm, p))
}

fn r_parent_element(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    let p = tree::with(|t| {
        t.nodes[n]
            .parent
            .filter(|p| t.nodes[*p].t == NType::Element)
    });
    Ok(node_obj_opt(vm, p))
}

fn r_child_nodes(vm: &mut Vm, o: &Obj, n: NodeId) -> R<Value> {
    Ok(list_obj(vm, ListKind::Children(o.clone())))
}

fn r_first_child(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    let c = tree::with(|t| {
        if children_valid(t, n) {
            t.nodes[n].children
        } else {
            None
        }
    });
    Ok(node_obj_opt(vm, c))
}

fn r_last_child(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    let c = tree::with(|t| {
        if children_valid(t, n) {
            t.nodes[n].last
        } else {
            None
        }
    });
    Ok(node_obj_opt(vm, c))
}

fn r_prev_sibling(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    let c = tree::with(|t| t.nodes[n].prev);
    Ok(node_obj_opt(vm, c))
}

fn r_next_sibling(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    let c = tree::with(|t| t.nodes[n].next);
    Ok(node_obj_opt(vm, c))
}

pub fn r_prev_element_sibling(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    let c = tree::with(|t| {
        let mut p = t.nodes[n].prev;
        while let Some(x) = p {
            if t.nodes[x].t == NType::Element {
                return Some(x);
            }
            p = t.nodes[x].prev;
        }
        None
    });
    Ok(node_obj_opt(vm, c))
}

pub fn r_next_element_sibling(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    let c = tree::with(|t| {
        let mut p = t.nodes[n].next;
        while let Some(x) = p {
            if t.nodes[x].t == NType::Element {
                return Some(x);
            }
            p = t.nodes[x].next;
        }
        None
    });
    Ok(node_obj_opt(vm, c))
}

fn r_attributes(vm: &mut Vm, o: &Obj, n: NodeId) -> R<Value> {
    if tree::with(|t| t.nodes[n].t == NType::Element) {
        Ok(map_obj(vm, MapKind::Attributes(o.clone())))
    } else {
        Ok(Value::Null)
    }
}

pub fn is_connected(t: &Tree, n: NodeId) -> bool {
    let mut cur = Some(n);
    while let Some(c) = cur {
        if t.is_doc(c) {
            return true;
        }
        cur = t.nodes[c].parent;
    }
    false
}

fn r_is_connected(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(b(tree::with(|t| is_connected(t, n))))
}

fn r_owner_document(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    let (is_doc, doc) = tree::with(|t| (t.is_doc(n), t.nodes[n].doc));
    if is_doc {
        return Ok(Value::Null);
    }
    match doc {
        Some(d) => Ok(node_obj(vm, d)),
        None => Err(()),
    }
}

fn r_namespace_uri(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| match t.nodes[n].t {
        NType::Element | NType::Attribute | NType::NamespaceDecl => opt_s(t.ns_href(t.nodes[n].ns)),
        _ => Value::Null,
    }))
}

fn r_prefix(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| match t.nodes[n].t {
        NType::Element | NType::Attribute | NType::NamespaceDecl => {
            s(t.ns_prefix(t.nodes[n].ns).unwrap_or(""))
        }
        _ => s(""),
    }))
}

fn w_prefix(vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let prefix = str_of(v);
    let doc = doc_of_node(n);
    let r = tree::with(|t| -> Result<(), i64> {
        let ty = t.nodes[n].t;
        if ty != NType::Element && ty != NType::Attribute {
            return Ok(());
        }
        let nsnode = if ty == NType::Element {
            Some(n)
        } else {
            t.nodes[n]
                .parent
                .or_else(|| t.nodes[n].doc.and_then(|d| t.root_element(d)))
        };
        let pfx = if prefix.is_empty() {
            None
        } else {
            Some(prefix.clone())
        };
        let ns = match t.nodes[n].ns {
            Some(ns) => ns,
            None => return Ok(()),
        };
        let nsnode = match nsnode {
            Some(x) => x,
            None => return Ok(()),
        };
        if t.nss[ns].prefix == pfx {
            return Ok(());
        }
        let uri = t.nss[ns].href.clone();
        if (prefix == "xml" && uri != XML_XML_NAMESPACE)
            || (ty == NType::Attribute && prefix == "xmlns" && uri != XMLNS_NS)
            || (ty == NType::Attribute && t.nodes[n].name == "xmlns")
        {
            return Err(NAMESPACE_ERR);
        }
        let mut found = None;
        for &d in &t.nodes[nsnode].ns_def {
            if t.nss[d].prefix == pfx && t.nss[d].href == uri {
                found = Some(d);
                break;
            }
        }
        let newns = match found {
            Some(f) => f,
            None => match t.new_ns(Some(nsnode), &uri, pfx.as_deref()) {
                Some(x) => x,
                None => return Err(-NAMESPACE_ERR),
            },
        };
        t.nodes[n].ns = Some(newns);
        Ok(())
    });
    match r {
        Ok(()) => Ok(()),
        Err(code) if code < 0 => {
            throw_dom(vm, -code, true);
            Err(())
        }
        Err(code) => {
            throw_dom(vm, code, strict(doc));
            Err(())
        }
    }
}

fn r_local_name(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| match t.nodes[n].t {
        NType::Element | NType::Attribute | NType::NamespaceDecl => s(&t.nodes[n].name),
        _ => Value::Null,
    }))
}

/// xmlNodeGetBase
pub fn node_get_base(t: &Tree, n: NodeId) -> Option<String> {
    let doc = t.nodes[n].doc;
    if let Some(d) = doc {
        if t.nodes[d].t == NType::HtmlDocument {
            // look for <base href>
            for x in t.descendants(d) {
                if t.nodes[x].t == NType::Element && t.nodes[x].name.eq_ignore_ascii_case("base") {
                    if let Some(h) = t.get_prop(x, "href") {
                        return Some(h);
                    }
                }
            }
            return t.doc_url(d);
        }
    }
    let mut bases: Vec<String> = Vec::new();
    let mut cur = Some(n);
    while let Some(c) = cur {
        if t.nodes[c].t == NType::EntityDecl {
            break;
        }
        if t.nodes[c].t == NType::Element {
            for a in t.attrs_of(c) {
                if t.nodes[a].name == "base" && t.ns_href(t.nodes[a].ns) == Some(XML_XML_NAMESPACE)
                {
                    let v = t.node_get_content(a);
                    bases.push(v);
                }
            }
            if let Some(last) = bases.last() {
                if last.starts_with("http://")
                    || last.starts_with("ftp://")
                    || last.starts_with("urn:")
                    || last.starts_with('/') && false
                {
                    break;
                }
            }
        }
        cur = t.nodes[c].parent;
    }
    let mut result: Option<String> = doc.and_then(|d| t.doc_url(d));
    for b in bases.iter().rev() {
        result = match &result {
            None => Some(b.clone()),
            Some(r) => crate::uri::build_uri(b, Some(r)).or_else(|| Some(b.clone())),
        };
    }
    result
}

fn r_base_uri(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| opt_s(node_get_base(t, n).as_deref())))
}

fn r_text_content(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| {
        let v = get_content_value(t, n, false);
        if matches!(v, Value::Null) { s("") } else { v }
    }))
}

fn w_text_content(_vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let st = str_of(v);
    tree::with(|t| {
        let ty = t.nodes[n].t;
        if matches!(ty, NType::DocumentFrag | NType::Element | NType::Attribute) {
            t.remove_id_for_attr_if_attr(n);
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

pub fn node_props() -> Vec<Prop> {
    vec![
        p("nodeName", r_node_name, None, "string"),
        p("nodeValue", r_node_value, Some(w_node_value), "?string"),
        p("nodeType", r_node_type, None, "int"),
        p("parentNode", r_parent_node, None, "mixed"),
        p("parentElement", r_parent_element, None, "mixed"),
        p("childNodes", r_child_nodes, None, "mixed"),
        p("firstChild", r_first_child, None, "mixed"),
        p("lastChild", r_last_child, None, "mixed"),
        p("previousSibling", r_prev_sibling, None, "mixed"),
        p("nextSibling", r_next_sibling, None, "mixed"),
        p("attributes", r_attributes, None, "mixed"),
        p("isConnected", r_is_connected, None, "bool"),
        p("ownerDocument", r_owner_document, None, "mixed"),
        p("namespaceURI", r_namespace_uri, None, "?string"),
        p("prefix", r_prefix, Some(w_prefix), "string"),
        p("localName", r_local_name, None, "?string"),
        p("baseURI", r_base_uri, None, "?string"),
        p(
            "textContent",
            r_text_content,
            Some(w_text_content),
            "?string",
        ),
    ]
}

fn r_first_element_child(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    let c = tree::with(|t| {
        let mut x = t.nodes[n].children;
        while let Some(y) = x {
            if t.nodes[y].t == NType::Element {
                return Some(y);
            }
            x = t.nodes[y].next;
        }
        None
    });
    Ok(node_obj_opt(vm, c))
}

fn r_last_element_child(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    let c = tree::with(|t| {
        let mut x = t.nodes[n].last;
        while let Some(y) = x {
            if t.nodes[y].t == NType::Element {
                return Some(y);
            }
            x = t.nodes[y].prev;
        }
        None
    });
    Ok(node_obj_opt(vm, c))
}

fn r_child_element_count(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(Value::Long(tree::with(|t| {
        t.children_of(n)
            .iter()
            .filter(|c| t.nodes[**c].t == NType::Element)
            .count() as i64
    })))
}

pub fn parent_props() -> Vec<Prop> {
    vec![
        p("firstElementChild", r_first_element_child, None, "mixed"),
        p("lastElementChild", r_last_element_child, None, "mixed"),
        p("childElementCount", r_child_element_count, None, "int"),
    ]
}

// ---------------------------------------------------------------------------
// Methods

fn this_node(vm: &mut Vm, o: &Obj) -> R<NodeId> {
    need_node(vm, o)
}

fn arg_node(
    vm: &mut Vm,
    a: &Args,
    i: usize,
    name: &str,
    class: &str,
    nullable: bool,
) -> R<Option<(Obj, NodeId)>> {
    match a.obj_of(vm, i, name, class, nullable)? {
        None => Ok(None),
        Some(o) => {
            let n = need_node(vm, &o)?;
            Ok(Some((o, n)))
        }
    }
}

pub fn args<'a>(a: &'a [Value]) -> Args<'a> {
    Args::new(cur_method(), a)
}

/// dom_node_check_legacy_insertion_validity; Err(code) for errors, Err(-1)
/// when a warning was emitted (empty fragment).
fn legacy_insertion_check(
    t: &Tree,
    parent: NodeId,
    child: NodeId,
    warn_empty_fragment: bool,
) -> Result<(), i64> {
    if is_read_only(t, parent)
        || t.nodes[child]
            .parent
            .map(|p| is_read_only(t, p))
            .unwrap_or(false)
    {
        return Err(NO_MODIFICATION_ALLOWED_ERR);
    }
    if !hierarchy_ok(t, parent, child) {
        return Err(HIERARCHY_REQUEST_ERR);
    }
    if t.nodes[child].doc != t.nodes[parent].doc && t.nodes[child].doc.is_some() {
        return Err(WRONG_DOCUMENT_ERR);
    }
    if warn_empty_fragment
        && t.nodes[child].t == NType::DocumentFrag
        && t.nodes[child].children.is_none()
    {
        return Err(-1);
    }
    if t.nodes[parent].t == NType::Attribute
        && !matches!(t.nodes[child].t, NType::Text | NType::EntityRef)
    {
        return Err(HIERARCHY_REQUEST_ERR);
    }
    if t.nodes[child].t == NType::Attribute && t.nodes[parent].t != NType::Element {
        return Err(HIERARCHY_REQUEST_ERR);
    }
    if t.is_doc(child) {
        return Err(HIERARCHY_REQUEST_ERR);
    }
    Ok(())
}

fn report_check(vm: &mut Vm, code: i64, strict_: bool) {
    if code == -1 {
        warn(vm, "Document Fragment is empty");
    } else {
        throw_dom(vm, code, strict_);
    }
}

fn adopt_doc_if_needed(t: &mut Tree, child: NodeId, parent: NodeId) {
    if t.nodes[child].doc.is_none() && t.nodes[parent].doc.is_some() {
        let d = t.nodes[parent].doc;
        t.set_tree_doc(child, d);
    }
}

fn m_append_child(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let parent = this_node(vm, o)?;
    let (_co, child) = arg_node(vm, &a, 0, "node", "DOMNode", false)?.unwrap();
    append_child_legacy(vm, parent, child)
}

pub fn append_child_legacy(vm: &mut Vm, parent: NodeId, child: NodeId) -> R<Value> {
    let doc = doc_of_node(parent);
    let st = strict(doc);
    if !tree::with(|t| children_valid(t, parent)) {
        return Ok(Value::False);
    }
    if let Err(code) = tree::with(|t| legacy_insertion_check(t, parent, child, true)) {
        report_check(vm, code, st);
        return Ok(Value::False);
    }
    let r: Result<NodeId, i64> = tree::with(|t| {
        adopt_doc_if_needed(t, child, parent);
        if t.nodes[child].parent.is_some() {
            t.unlink(child);
        }
        let ct = t.nodes[child].t;
        if ct == NType::Text
            && t.nodes[parent]
                .last
                .map(|l| t.nodes[l].t == NType::Text)
                .unwrap_or(false)
        {
            t.link_child_raw(parent, child);
            return Ok(child);
        }
        if ct == NType::Attribute {
            let name = t.nodes[child].name.clone();
            let lastattr = match t.nodes[child].ns {
                None => t.has_prop(parent, &name),
                Some(ns) => {
                    let h = t.nss[ns].href.clone();
                    t.has_ns_prop(parent, &name, Some(&h))
                }
            };
            if let Some(la) = lastattr {
                if la != child {
                    t.remove_id_for_attr(la);
                    t.unlink(la);
                }
            }
            let nc = t.add_child(parent, child);
            reconcile_attribute_ns_after_insertion(t, nc);
            return Ok(nc);
        }
        if ct == NType::DocumentFrag {
            let last = t.nodes[child].last.unwrap();
            let plast = t.nodes[parent].last;
            let nc = insert_fragment(t, parent, plast, None, child).unwrap();
            reconcile_ns_list(t, nc, last);
            return Ok(nc);
        }
        if ct == NType::Dtd {
            let d = t.nodes[parent].doc;
            if let Some(d) = d {
                if t.int_subset(d).is_some() {
                    return Err(1000);
                }
            }
            let nc = t.add_child(parent, child);
            if let Some(d) = d {
                if let Some(di) = t.doc_info_mut(d) {
                    di.int_subset = Some(nc);
                }
            }
            return Ok(nc);
        }
        let nc = t.add_child(parent, child);
        reconcile_ns(t, nc);
        Ok(nc)
    });
    match r {
        Ok(nc) => Ok(node_obj(vm, nc)),
        Err(1000) => {
            throw_dom_msg(
                vm,
                HIERARCHY_REQUEST_ERR,
                "A document may only contain one document type",
                st,
            );
            Ok(Value::False)
        }
        Err(_) => {
            throw_dom(vm, INVALID_STATE_ERR, st);
            Ok(Value::False)
        }
    }
}

/// php_dom_reconcile_attribute_namespace_after_insertion
pub fn reconcile_attribute_ns_after_insertion(t: &mut Tree, attr: NodeId) {
    let ns = match t.nodes[attr].ns {
        Some(ns) => ns,
        None => return,
    };
    let parent = match t.nodes[attr].parent {
        Some(p) => p,
        None => return,
    };
    let prefix = t.nss[ns].prefix.clone();
    let href = t.nss[ns].href.clone();
    if let Some(found) = t.search_ns(parent, prefix.as_deref()) {
        if t.nss[found].href == href {
            t.nodes[attr].ns = Some(found);
            return;
        }
    }
    // declare it
    match t.search_ns_by_href(parent, &href) {
        Some(f) if t.nss[f].prefix.is_some() => t.nodes[attr].ns = Some(f),
        _ => {
            if let Some(nn) = t.new_ns(Some(parent), &href, prefix.as_deref()) {
                t.nodes[attr].ns = Some(nn);
            } else if let Some(nn) = new_reconciled_ns(t, parent, ns) {
                t.nodes[attr].ns = Some(nn);
            }
        }
    }
}

fn m_insert_before(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 2)?;
    let parent = this_node(vm, o)?;
    let (_co, child) = arg_node(vm, &a, 0, "node", "DOMNode", false)?.unwrap();
    let refn = arg_node(vm, &a, 1, "child", "DOMNode", true)?.map(|(_, n)| n);
    let doc = doc_of_node(parent);
    let st = strict(doc);
    if !tree::with(|t| children_valid(t, parent)) {
        return Ok(Value::False);
    }
    if let Err(code) = tree::with(|t| legacy_insertion_check(t, parent, child, true)) {
        report_check(vm, code, st);
        return Ok(Value::False);
    }
    if let Some(r) = refn {
        if tree::with(|t| t.nodes[r].parent != Some(parent)) {
            throw_dom(vm, NOT_FOUND_ERR, st);
            return Ok(Value::False);
        }
    }
    let nc: Option<NodeId> = tree::with(|t| {
        adopt_doc_if_needed(t, child, parent);
        let ct = t.nodes[child].t;
        match refn {
            Some(refp) => {
                if t.nodes[child].parent.is_some() {
                    t.unlink(child);
                }
                if ct == NType::Text
                    && (t.nodes[refp].t == NType::Text
                        || t.nodes[refp]
                            .prev
                            .map(|p| t.nodes[p].t == NType::Text)
                            .unwrap_or(false))
                {
                    t.insert_before_raw(refp, child);
                    Some(child)
                } else if ct == NType::Attribute {
                    let rp = t.nodes[refp].parent.unwrap();
                    let name = t.nodes[child].name.clone();
                    let lastattr = match t.nodes[child].ns {
                        None => t.has_prop(rp, &name),
                        Some(ns) => {
                            let h = t.nss[ns].href.clone();
                            t.has_ns_prop(rp, &name, Some(&h))
                        }
                    };
                    if let Some(la) = lastattr {
                        if la != child {
                            t.unlink(la);
                        } else {
                            return Some(child);
                        }
                    }
                    Some(t.add_prev_sibling(refp, child))
                } else if ct == NType::DocumentFrag {
                    let last = t.nodes[child].last?;
                    let prev = t.nodes[refp].prev;
                    let nc = insert_fragment(t, parent, prev, Some(refp), child)?;
                    reconcile_ns_list(t, nc, last);
                    Some(nc)
                } else {
                    let nc = t.add_prev_sibling(refp, child);
                    reconcile_ns(t, nc);
                    Some(nc)
                }
            }
            None => {
                if t.nodes[child].parent.is_some() {
                    t.unlink(child);
                }
                if ct == NType::Text
                    && t.nodes[parent]
                        .last
                        .map(|l| t.nodes[l].t == NType::Text)
                        .unwrap_or(false)
                {
                    t.link_child_raw(parent, child);
                    Some(child)
                } else if ct == NType::Attribute {
                    let name = t.nodes[child].name.clone();
                    let lastattr = match t.nodes[child].ns {
                        None => t.has_prop(parent, &name),
                        Some(ns) => {
                            let h = t.nss[ns].href.clone();
                            t.has_ns_prop(parent, &name, Some(&h))
                        }
                    };
                    if let Some(la) = lastattr {
                        if la != child {
                            t.unlink(la);
                        } else {
                            return Some(child);
                        }
                    }
                    Some(t.add_child(parent, child))
                } else if ct == NType::DocumentFrag {
                    let last = t.nodes[child].last?;
                    let plast = t.nodes[parent].last;
                    let nc = insert_fragment(t, parent, plast, None, child)?;
                    reconcile_ns_list(t, nc, last);
                    Some(nc)
                } else {
                    let nc = t.add_child(parent, child);
                    reconcile_ns(t, nc);
                    Some(nc)
                }
            }
        }
    });
    Ok(node_obj_opt(vm, nc))
}

fn m_replace_child(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let parent = this_node(vm, o)?;
    let (_no, newchild) = arg_node(vm, &a, 0, "node", "DOMNode", false)?.unwrap();
    let (oo, oldchild) = arg_node(vm, &a, 1, "child", "DOMNode", false)?.unwrap();
    let doc = doc_of_node(parent);
    let st = strict(doc);
    let pre = tree::with(|t| -> Result<(), i64> {
        if t.nodes[newchild].doc != t.nodes[parent].doc && t.nodes[newchild].doc.is_some() {
            return Err(WRONG_DOCUMENT_ERR);
        }
        if !children_valid(t, parent) || t.nodes[parent].children.is_none() {
            return Err(0);
        }
        legacy_insertion_check(t, parent, newchild, false)?;
        if (t.nodes[oldchild].t == NType::Attribute) != (t.nodes[newchild].t == NType::Attribute) {
            return Err(HIERARCHY_REQUEST_ERR);
        }
        if t.nodes[oldchild].parent != Some(parent) {
            return Err(NOT_FOUND_ERR);
        }
        Ok(())
    });
    match pre {
        Err(0) => return Ok(Value::False),
        Err(code) => {
            report_check(vm, code, st);
            return Ok(Value::False);
        }
        Ok(()) => {}
    }
    tree::with(|t| {
        adopt_doc_if_needed(t, newchild, parent);
        if t.nodes[newchild].t == NType::DocumentFrag {
            let prev = t.nodes[oldchild].prev;
            let next = t.nodes[oldchild].next;
            t.unlink(oldchild);
            if let Some(last) = t.nodes[newchild].last {
                if let Some(nc) = insert_fragment(t, parent, prev, next, newchild) {
                    reconcile_ns_list(t, nc, last);
                }
            }
        } else if oldchild != newchild {
            let d = t.nodes[parent].doc;
            let replacedoctype = d.and_then(|d| t.int_subset(d)) == Some(oldchild);
            replace_node(t, oldchild, newchild);
            reconcile_ns(t, newchild);
            if replacedoctype {
                if let Some(di) = d.and_then(|d| t.doc_info_mut(d)) {
                    di.int_subset = Some(newchild);
                }
            }
        }
    });
    Ok(Value::Object(oo))
}

fn m_remove_child(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let parent = this_node(vm, o)?;
    let (co, child) = arg_node(vm, &a, 0, "child", "DOMNode", false)?.unwrap();
    let st = strict(doc_of_node(parent));
    let r = tree::with(|t| {
        if t.nodes[parent].children.is_none() && t.nodes[parent].properties.is_none()
            || t.nodes[child].parent != Some(parent)
        {
            return Err(NOT_FOUND_ERR);
        }
        if is_read_only(t, parent)
            || t.nodes[child]
                .parent
                .map(|p| is_read_only(t, p))
                .unwrap_or(false)
        {
            return Err(NO_MODIFICATION_ALLOWED_ERR);
        }
        t.unlink(child);
        Ok(())
    });
    match r {
        Ok(()) => Ok(Value::Object(co)),
        Err(code) => {
            throw_dom(vm, code, st);
            Ok(Value::False)
        }
    }
}

fn m_has_child_nodes(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let n = this_node(vm, o)?;
    Ok(b(tree::with(|t| {
        children_valid(t, n) && t.nodes[n].children.is_some()
    })))
}

fn m_clone_node(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 0, 1)?;
    let deep = a.bool(vm, 0, "deep", false)?;
    let n = this_node(vm, o)?;
    let doc = tree::with(|t| t.nodes[n].doc);
    let is_doc = tree::with(|t| t.is_doc(n));
    let c = match clone_node(n, doc, deep) {
        Some(c) => c,
        None => return Ok(Value::False),
    };
    if is_doc {
        let p = doc_props(Some(n));
        set_doc_props(c, p);
    }
    Ok(node_obj(vm, c))
}

/// php_dom_normalize_legacy
pub fn normalize(t: &mut Tree, n: NodeId) {
    let mut child = t.nodes[n].children;
    while let Some(c) = child {
        match t.nodes[c].t {
            NType::Text => {
                // merge following text nodes
                let mut next = t.nodes[c].next;
                while let Some(nx) = next {
                    if t.nodes[nx].t != NType::Text {
                        break;
                    }
                    let nn = t.nodes[nx].next;
                    let content = t.nodes[nx].content.clone();
                    t.nodes[c].content.push_str(&content);
                    t.unlink(nx);
                    next = nn;
                }
                if t.nodes[c].content.is_empty() {
                    let nx = t.nodes[c].next;
                    t.unlink(c);
                    child = nx;
                    continue;
                }
            }
            NType::Element => {
                normalize(t, c);
                for a in t.attrs_of(c) {
                    normalize(t, a);
                }
            }
            _ => {}
        }
        child = t.nodes[c].next;
    }
}

fn m_normalize(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let n = this_node(vm, o)?;
    tree::with(|t| normalize(t, n));
    Ok(Value::Null)
}

fn has_feature(feature: &str, version: &str) -> bool {
    (version == "1.0" || version == "2.0" || version.is_empty())
        && (feature.eq_ignore_ascii_case("XML")
            || (feature.eq_ignore_ascii_case("Core") && version == "1.0"))
}

fn m_is_supported(vm: &mut Vm, _o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let f = a.str(vm, 0, "feature")?;
    let v = a.str(vm, 1, "version")?;
    Ok(b(has_feature(&f, &v)))
}

fn m_has_attributes(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let n = this_node(vm, o)?;
    Ok(b(tree::with(|t| {
        t.nodes[n].t == NType::Element && t.nodes[n].properties.is_some()
    })))
}

fn m_is_same_node(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let n = this_node(vm, o)?;
    let (_oo, other) = arg_node(vm, &a, 0, "otherNode", "DOMNode", false)?.unwrap();
    Ok(b(n == other))
}

fn content_equal(t: &Tree, a: NodeId, b_: NodeId) -> bool {
    t.node_get_content(a) == t.node_get_content(b_)
}

fn ns_uri_eq(t: &Tree, a: NodeId, b_: NodeId) -> bool {
    t.ns_href(t.nodes[a].ns) == t.ns_href(t.nodes[b_].ns)
}

fn ns_prefix_eq(t: &Tree, a: NodeId, b_: NodeId) -> bool {
    t.ns_prefix(t.nodes[a].ns) == t.ns_prefix(t.nodes[b_].ns)
}

fn list_of(t: &Tree, first: Option<NodeId>) -> Vec<NodeId> {
    let mut v = Vec::new();
    let mut c = first;
    while let Some(x) = c {
        v.push(x);
        c = t.nodes[x].next;
    }
    v
}

fn equal_attr(t: &Tree, a: NodeId, b_: NodeId) -> bool {
    t.nodes[a].name == t.nodes[b_].name && ns_uri_eq(t, a, b_) && content_equal(t, a, b_)
}

pub fn is_equal_node(t: &Tree, a: NodeId, b_: NodeId) -> bool {
    let (na, nb) = (&t.nodes[a], &t.nodes[b_]);
    if na.t != nb.t {
        return false;
    }
    match na.t {
        NType::Element => {
            if na.name != nb.name || !ns_prefix_eq(t, a, b_) || !ns_uri_eq(t, a, b_) {
                return false;
            }
            let aa = list_of(t, na.properties);
            let ab = list_of(t, nb.properties);
            if aa.len() != ab.len()
                || !aa
                    .iter()
                    .all(|x| ab.iter().any(|y| is_equal_node(t, *x, *y)))
            {
                return false;
            }
            let da = &na.ns_def;
            let db = &nb.ns_def;
            if da.len() != db.len()
                || !da.iter().all(|x| {
                    db.iter().any(|y| {
                        t.nss[*x].prefix == t.nss[*y].prefix && t.nss[*x].href == t.nss[*y].href
                    })
                })
            {
                return false;
            }
            let ca = list_of(t, na.children);
            let cb = list_of(t, nb.children);
            ca.len() == cb.len()
                && ca
                    .iter()
                    .zip(cb.iter())
                    .all(|(x, y)| is_equal_node(t, *x, *y))
        }
        NType::Dtd => {
            let (x, y) = (t.dtd_info(a), t.dtd_info(b_));
            na.name == nb.name
                && x.map(|d| d.external_id.clone()) == y.map(|d| d.external_id.clone())
                && x.map(|d| d.system_id.clone()) == y.map(|d| d.system_id.clone())
        }
        NType::Pi => na.name == nb.name && na.content == nb.content,
        NType::Text | NType::Comment | NType::CData => na.content == nb.content,
        NType::Attribute => equal_attr(t, a, b_),
        NType::EntityRef => na.name == nb.name,
        NType::EntityDecl | NType::Notation | NType::Entity => {
            let (x, y) = (t.entity_info(a), t.entity_info(b_));
            na.name == nb.name
                && x.map(|e| (e.etype, e.external_id.clone(), e.system_id.clone()))
                    == y.map(|e| (e.etype, e.external_id.clone(), e.system_id.clone()))
                && content_equal(t, a, b_)
        }
        NType::NamespaceDecl => {
            t.ns_prefix(na.ns) == t.ns_prefix(nb.ns) && t.ns_href(na.ns) == t.ns_href(nb.ns)
        }
        NType::DocumentFrag | NType::Document | NType::HtmlDocument => {
            let ca = list_of(t, na.children);
            let cb = list_of(t, nb.children);
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
    let other = match arg_node(vm, &a, 0, "otherNode", "DOMNode", true)? {
        None => return Ok(Value::False),
        Some((_, n)) => n,
    };
    let n = this_node(vm, o)?;
    if n == other {
        return Ok(Value::True);
    }
    Ok(b(tree::with(|t| is_equal_node(t, n, other))))
}

fn m_lookup_prefix(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let uri = a.str(vm, 0, "namespace")?;
    let n = this_node(vm, o)?;
    if uri.is_empty() {
        return Ok(Value::Null);
    }
    Ok(tree::with(|t| {
        let lookup = match t.nodes[n].t {
            NType::Element => Some(n),
            NType::Document | NType::HtmlDocument => t.root_element(n),
            NType::Entity
            | NType::Notation
            | NType::DocumentFrag
            | NType::DocumentType
            | NType::Dtd => return Value::Null,
            _ => t.nodes[n].parent,
        };
        match lookup {
            Some(l) => match t.search_ns_by_href(l, &uri) {
                Some(ns) if t.nss[ns].prefix.is_some() => s(t.nss[ns].prefix.as_deref().unwrap()),
                _ => Value::Null,
            },
            None => Value::Null,
        }
    }))
}

fn m_is_default_namespace(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let uri = a.str(vm, 0, "namespace")?;
    let n = this_node(vm, o)?;
    if uri.is_empty() {
        return Ok(Value::False);
    }
    Ok(tree::with(|t| {
        let n = if t.is_doc(n) {
            match t.root_element(n) {
                Some(r) => r,
                None => return Value::False,
            }
        } else {
            n
        };
        match t.search_ns(n, None) {
            Some(ns) if t.nss[ns].href == uri => Value::True,
            _ => Value::False,
        }
    }))
}

fn m_lookup_namespace_uri(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let prefix = a.opt_str(vm, 0, "prefix")?;
    let n = this_node(vm, o)?;
    Ok(tree::with(|t| {
        let n = if t.is_doc(n) {
            match t.root_element(n) {
                Some(r) => r,
                None => return Value::Null,
            }
        } else {
            n
        };
        match t.search_ns(n, prefix.as_deref()) {
            Some(ns) => s(&t.nss[ns].href),
            None => Value::Null,
        }
    }))
}

fn m_get_node_path(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let n = this_node(vm, o)?;
    Ok(s(&tree::with(|t| t.node_path(n))))
}

fn m_get_line_no(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let n = this_node(vm, o)?;
    Ok(Value::Long(tree::with(|t| {
        let nd = &t.nodes[n];
        match nd.t {
            NType::Element | NType::Text | NType::Comment | NType::Pi | NType::CData => {
                nd.line as i64
            }
            NType::Attribute => nd.parent.map(|p| t.nodes[p].line as i64).unwrap_or(-1),
            _ => {
                if let Some(p) = nd.prev {
                    t.nodes[p].line as i64
                } else if let Some(p) = nd.parent {
                    t.nodes[p].line as i64
                } else {
                    -1
                }
            }
        }
    })))
}

fn m_contains(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let other = match a.raw(0) {
        None | Some(Value::Null) => return Ok(Value::False),
        Some(Value::Object(oo))
            if obj_is(vm, &oo, "DOMNode") || obj_is(vm, &oo, "DOMNameSpaceNode") =>
        {
            oo
        }
        Some(v) => {
            let msg = format!(
                "{}(): Argument #1 ($other) must be of type DOMNode|DOMNameSpaceNode|null, {} given",
                a.f,
                type_name_of(&v)
            );
            throw_error(vm, b"TypeError", &msg);
            return Err(());
        }
    };
    let otherp = need_node(vm, &other)?;
    let n = this_node(vm, o)?;
    Ok(b(tree::with(|t| {
        let mut cur = Some(otherp);
        while let Some(c) = cur {
            if c == n {
                return true;
            }
            cur = t.nodes[c].parent;
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
        let mut cur = n;
        while let Some(p) = t.nodes[cur].parent {
            cur = p;
        }
        cur
    });
    Ok(node_obj(vm, r))
}

fn m_compare_document_position(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let this = this_node(vm, o)?;
    let (oo, other) = arg_node(vm, &a, 0, "other", "DOMNode", false)?.unwrap();
    if this == other {
        return Ok(Value::Long(0));
    }
    let r = tree::with(|t| -> i64 {
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
            if let (Some(a1), Some(n1), Some(n2)) = (attr1, node1, node2) {
                if n1 == n2 {
                    for at in t.attrs_of(n2) {
                        if equal_attr(t, at, a1) {
                            return 0x20 | 0x02;
                        } else if equal_attr(t, at, this) {
                            return 0x20 | 0x04;
                        }
                    }
                }
            }
        }
        let disconnected = |n1: Option<NodeId>, n2: Option<NodeId>| -> i64 {
            let ordering = if n1 == n2 {
                if Rc::as_ptr(&oo) < Rc::as_ptr(o) {
                    0x02
                } else {
                    0x04
                }
            } else if n1 < n2 {
                0x02
            } else {
                0x04
            };
            0x01 | 0x20 | ordering
        };
        let (mut n1, mut n2) = match (node1, node2) {
            (Some(a), Some(b_)) => (a, b_),
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
            return 0x08 | 0x02;
        }
        if (n2_anc_of_n1 && attr2.is_none()) || (n1 == n2 && attr1.is_some()) {
            return 0x10 | 0x04;
        }
        if n1_anc_of_n2 {
            return 0x02;
        } else if n2_anc_of_n1 {
            return 0x04;
        }
        while d1 > d2 {
            n1 = t.nodes[n1].parent.unwrap();
            d1 -= 1;
        }
        while d2 > d1 {
            n2 = t.nodes[n2].parent.unwrap();
            d2 -= 1;
        }
        while t.nodes[n1].parent != t.nodes[n2].parent {
            n1 = t.nodes[n1].parent.unwrap();
            n2 = t.nodes[n2].parent.unwrap();
        }
        let mut x = t.nodes[n1].next;
        while let Some(y) = x {
            if y == n2 {
                return 0x02;
            }
            x = t.nodes[y].next;
        }
        0x04
    });
    Ok(Value::Long(r))
}

fn m_sleep(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let cn = goro_core::value::display_class_name(&o.borrow().class_name).to_string();
    throw_error(
        vm,
        b"Exception",
        &format!(
            "Serialization of '{}' is not allowed, unless serialization methods are implemented in a subclass",
            cn
        ),
    );
    Err(())
}

fn m_wakeup(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let cn = goro_core::value::display_class_name(&o.borrow().class_name).to_string();
    throw_error(
        vm,
        b"Exception",
        &format!(
            "Unserialization of '{}' is not allowed, unless unserialization methods are implemented in a subclass",
            cn
        ),
    );
    Err(())
}

fn m_c14n(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    crate::dom::document::c14n(vm, o, a, false)
}

fn m_c14n_file(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    crate::dom::document::c14n(vm, o, a, true)
}

pub fn node_methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("appendChild", m_append_child),
        ("C14N", m_c14n),
        ("C14NFile", m_c14n_file),
        ("cloneNode", m_clone_node),
        ("getLineNo", m_get_line_no),
        ("getNodePath", m_get_node_path),
        ("hasAttributes", m_has_attributes),
        ("hasChildNodes", m_has_child_nodes),
        ("insertBefore", m_insert_before),
        ("isDefaultNamespace", m_is_default_namespace),
        ("isSameNode", m_is_same_node),
        ("isEqualNode", m_is_equal_node),
        ("isSupported", m_is_supported),
        ("lookupNamespaceURI", m_lookup_namespace_uri),
        ("lookupPrefix", m_lookup_prefix),
        ("normalize", m_normalize),
        ("removeChild", m_remove_child),
        ("replaceChild", m_replace_child),
        ("contains", m_contains),
        ("getRootNode", m_get_root_node),
        ("compareDocumentPosition", m_compare_document_position),
        ("__sleep", m_sleep),
        ("__wakeup", m_wakeup),
    ]
}

// ---------------------------------------------------------------------------
// ParentNode / ChildNode

/// Validate the (...$nodes) arguments; returns the list of (object|string).
fn node_list_args(vm: &mut Vm, a: &[Value]) -> R<Vec<Value>> {
    let f = cur_method();
    let mut out = Vec::new();
    for (i, v) in a.iter().enumerate() {
        let v = v.deref();
        match &v {
            Value::Object(o) if obj_is(vm, o, "DOMNode") => out.push(v.clone()),
            Value::String(_) => out.push(v.clone()),
            other => {
                let msg = format!(
                    "{}(): Argument #{} must be of type DOMNode|string, {} given",
                    f,
                    i + 1,
                    type_name_of(other)
                );
                throw_error(vm, b"TypeError", &msg);
                return Err(());
            }
        }
    }
    Ok(out)
}

fn doc_from_context(t: &Tree, n: NodeId) -> Option<NodeId> {
    if t.is_doc(n) { Some(n) } else { t.nodes[n].doc }
}

/// dom_is_pre_insert_valid_without_step_1 (legacy flavour)
fn pre_insert_valid(
    vm: &mut Vm,
    parent: NodeId,
    node: NodeId,
    child: Option<NodeId>,
    document: Option<NodeId>,
    st: bool,
) -> bool {
    let r = tree::with(|t| -> Result<(), i64> {
        let pt = t.nodes[parent].t;
        if !matches!(
            pt,
            NType::Document | NType::HtmlDocument | NType::Element | NType::DocumentFrag
        ) {
            return Err(HIERARCHY_REQUEST_ERR);
        }
        if t.nodes[node].doc != document {
            return Err(WRONG_DOCUMENT_ERR);
        }
        if let Some(c) = child {
            if t.nodes[c].parent != Some(parent) {
                return Err(NOT_FOUND_ERR);
            }
        }
        if !hierarchy_ok(t, parent, node) || t.nodes[node].t == NType::Attribute {
            return Err(HIERARCHY_REQUEST_ERR);
        }
        Ok(())
    });
    match r {
        Ok(()) => true,
        Err(code) => {
            throw_dom(vm, code, st);
            false
        }
    }
}

/// dom_zvals_to_single_node
fn zvals_to_single_node(
    vm: &mut Vm,
    context: NodeId,
    nodes: &[Value],
    st: bool,
) -> R<Option<NodeId>> {
    let document = tree::with(|t| doc_from_context(t, context));
    if nodes.len() == 1 {
        return match &nodes[0] {
            Value::Object(o) => match node_of(o) {
                Some(n) => Ok(Some(n)),
                None => {
                    throw_dom(vm, INVALID_STATE_ERR, true);
                    Err(())
                }
            },
            v => {
                let s_ = str_of(v);
                Ok(Some(tree::with(|t| t.new_text(document, &s_))))
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
                        throw_dom(vm, INVALID_STATE_ERR, true);
                        return Err(());
                    }
                };
                if !pre_insert_valid(vm, frag, nn, None, document, st) {
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
                let s_ = str_of(v);
                tree::with(|t| {
                    let tx = t.new_text(document, &s_);
                    t.link_child_raw(frag, tx);
                });
            }
        }
    }
    Ok(Some(frag))
}

fn pre_insert_helper(
    t: &mut Tree,
    insertion_point: Option<NodeId>,
    parent: NodeId,
    newchild: NodeId,
    last: NodeId,
) {
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

fn insert_node_list_unchecked(
    t: &mut Tree,
    node: NodeId,
    parent: NodeId,
    mut insertion_point: Option<NodeId>,
) {
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
            reconcile_ns_list(t, newchild, last);
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
        } else {
            reconcile_ns(t, node);
        }
    }
}

pub fn pre_insert(
    vm: &mut Vm,
    node: NodeId,
    parent: NodeId,
    insertion_point: Option<NodeId>,
    st: bool,
) -> bool {
    let pdoc = tree::with(|t| {
        if t.is_doc(parent) {
            Some(parent)
        } else {
            t.nodes[parent].doc
        }
    });
    if pre_insert_valid(vm, parent, node, insertion_point, pdoc, st) {
        tree::with(|t| insert_node_list_unchecked(t, node, parent, insertion_point));
        true
    } else {
        false
    }
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

fn m_append(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let nodes = node_list_args(vm, a)?;
    let parent = this_node(vm, o)?;
    let st = strict(doc_of_node(parent));
    if let Some(node) = zvals_to_single_node(vm, parent, &nodes, st)? {
        pre_insert(vm, node, parent, None, st);
    }
    Ok(Value::Null)
}

fn m_prepend(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let parent = this_node(vm, o)?;
    if tree::with(|t| t.nodes[parent].children.is_none()) {
        return m_append(vm, o, a);
    }
    let nodes = node_list_args(vm, a)?;
    let st = strict(doc_of_node(parent));
    if let Some(node) = zvals_to_single_node(vm, parent, &nodes, st)? {
        let first = tree::with(|t| t.nodes[parent].children);
        pre_insert(vm, node, parent, first, st);
    }
    Ok(Value::Null)
}

fn m_replace_children(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let nodes = node_list_args(vm, a)?;
    let parent = this_node(vm, o)?;
    let st = strict(doc_of_node(parent));
    if let Some(node) = zvals_to_single_node(vm, parent, &nodes, st)? {
        let pdoc = tree::with(|t| {
            if t.is_doc(parent) {
                Some(parent)
            } else {
                t.nodes[parent].doc
            }
        });
        if pre_insert_valid(vm, parent, node, None, pdoc, st) {
            tree::with(|t| remove_all_children(t, parent));
            pre_insert(vm, node, parent, None, st);
        }
    }
    Ok(Value::Null)
}

pub fn parent_methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("append", m_append),
        ("prepend", m_prepend),
        ("replaceChildren", m_replace_children),
    ]
}

fn m_after(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let nodes = node_list_args(vm, a)?;
    let this = this_node(vm, o)?;
    let parent = match tree::with(|t| t.nodes[this].parent) {
        Some(p) => p,
        None => return Ok(Value::Null),
    };
    let st = strict(doc_of_node(this));
    let mut viable = tree::with(|t| t.nodes[this].next);
    while let Some(v) = viable {
        if !in_list(&nodes, v) {
            break;
        }
        viable = tree::with(|t| t.nodes[v].next);
    }
    if let Some(node) = zvals_to_single_node(vm, parent, &nodes, st)? {
        pre_insert(vm, node, parent, viable, st);
    }
    Ok(Value::Null)
}

fn m_before(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let nodes = node_list_args(vm, a)?;
    let this = this_node(vm, o)?;
    let parent = match tree::with(|t| t.nodes[this].parent) {
        Some(p) => p,
        None => return Ok(Value::Null),
    };
    let st = strict(doc_of_node(this));
    let mut viable = tree::with(|t| t.nodes[this].prev);
    while let Some(v) = viable {
        if !in_list(&nodes, v) {
            break;
        }
        viable = tree::with(|t| t.nodes[v].prev);
    }
    if let Some(node) = zvals_to_single_node(vm, parent, &nodes, st)? {
        let ip = match viable {
            None => tree::with(|t| t.nodes[parent].children),
            Some(v) => tree::with(|t| t.nodes[v].next),
        };
        pre_insert(vm, node, parent, ip, st);
    }
    Ok(Value::Null)
}

fn removal_preconditions(vm: &mut Vm, child: NodeId) -> bool {
    let st = strict(doc_of_node(child));
    let r = tree::with(|t| {
        if is_read_only(t, child)
            || t.nodes[child]
                .parent
                .map(|p| is_read_only(t, p))
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
            throw_dom(vm, c, st);
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
    let st = strict(doc_of_node(child));
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
    if let Some(node) = zvals_to_single_node(vm, parent, &nodes, st)? {
        let pdoc = tree::with(|t| {
            t.nodes[parent]
                .doc
                .or_else(|| if t.is_doc(parent) { Some(parent) } else { None })
        });
        if pre_insert_valid(vm, parent, node, viable, pdoc, st) {
            tree::with(|t| {
                if t.nodes[child].parent != Some(node) {
                    t.unlink(child);
                }
                insert_node_list_unchecked(t, node, parent, viable);
            });
        }
    }
    Ok(Value::Null)
}

pub fn child_methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("remove", m_remove),
        ("before", m_before),
        ("after", m_after),
        ("replaceWith", m_replace_with),
    ]
}

/// Remove an attribute from the ID table if it is one.
pub trait IdHelpers {
    fn remove_id_for_attr_if_attr(&mut self, n: NodeId);
}

impl IdHelpers for Tree {
    fn remove_id_for_attr_if_attr(&mut self, n: NodeId) {
        if self.nodes[n].t == NType::Attribute {
            self.remove_id_for_attr(n);
        }
    }
}
