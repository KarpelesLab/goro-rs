//! DOMElement (element.c) and DOMAttr (attr.c)

use super::document::{check_qname, get_ns};
use super::node::{IdHelpers, args, b, cstr, s, str_of};
use super::*;
use crate::parser::is_valid_name;
use crate::tree::NsId;

/// The value of a tree node "attribute or namespace declaration".
#[derive(Clone, Copy)]
pub enum AttrOrNs {
    Attr(NodeId),
    Ns(NsId),
}

/// dom_get_nsdecl
pub fn get_nsdecl(t: &Tree, elem: NodeId, local: Option<&str>) -> Option<NsId> {
    if t.nodes[elem].t != NType::Element {
        return None;
    }
    let want = local.filter(|l| !l.is_empty());
    t.nodes[elem].ns_def.iter().copied().find(|&ns| match want {
        None => t.nss[ns].prefix.is_none(),
        Some(l) => t.nss[ns].prefix.as_deref() == Some(l),
    })
}

/// dom_get_attribute_or_nsdecl (legacy)
pub fn get_attribute_or_nsdecl(t: &mut Tree, elem: NodeId, name: &str) -> Option<AttrOrNs> {
    if let Some(i) = name.find(':').filter(|&i| i > 0 && i + 1 < name.len()) {
        let prefix = &name[..i];
        let local = &name[i + 1..];
        if prefix == "xmlns" {
            return t.nodes[elem]
                .ns_def
                .iter()
                .copied()
                .find(|&ns| t.nss[ns].prefix.as_deref() == Some(local))
                .map(AttrOrNs::Ns);
        }
        if let Some(ns) = t.search_ns(elem, Some(prefix)) {
            let href = t.nss[ns].href.clone();
            return t.has_ns_prop(elem, local, Some(&href)).map(AttrOrNs::Attr);
        }
    } else if name == "xmlns" {
        return t.nodes[elem]
            .ns_def
            .iter()
            .copied()
            .find(|&ns| t.nss[ns].prefix.is_none())
            .map(AttrOrNs::Ns);
    }
    t.has_ns_prop(elem, name, None).map(AttrOrNs::Attr)
}

/// Remove all children of an attribute/element and set legacy content
/// (xmlNodeSetContent: entity references are parsed).
pub fn set_content_legacy(t: &mut Tree, n: NodeId, value: &str) {
    super::node::remove_all_children(t, n);
    let doc = t.nodes[n].doc;
    crate::simplexml::string_get_node_list(t, doc, n, value);
}

/// xmlSetProp: "prefix:local" names use the in-scope namespace of the
/// prefix when there is one.
pub fn set_prop(t: &mut Tree, n: NodeId, name: &str, value: &str) -> NodeId {
    if let Some(i) = name.find(':').filter(|&i| i > 0 && i + 1 < name.len()) {
        if let Some(ns) = t.search_ns(n, Some(&name[..i])) {
            return t.set_ns_prop(n, Some(ns), &name[i + 1..], value);
        }
    }
    t.set_ns_prop(n, None, name, value)
}

/// dom_deep_ns_redef
fn deep_ns_redef(t: &mut Tree, node: NodeId, ns: NsId) {
    let mut work: Vec<(NodeId, Option<NsId>)> = vec![(node, None)];
    while let Some((cur, mut defined)) = work.pop() {
        if t.nodes[cur].ns == Some(ns) {
            if defined.is_none() {
                let (h, p) = (t.nss[ns].href.clone(), t.nss[ns].prefix.clone());
                defined = Some(new_ns_force(t, cur, &h, p.as_deref()));
            }
            t.nodes[cur].ns = defined;
        }
        for at in t.attrs_of(cur) {
            if t.nodes[at].ns == Some(ns) {
                if defined.is_none() {
                    let (h, p) = (t.nss[ns].href.clone(), t.nss[ns].prefix.clone());
                    defined = Some(new_ns_force(t, cur, &h, p.as_deref()));
                }
                t.nodes[at].ns = defined;
            }
        }
        for c in t.children_of(cur) {
            if t.nodes[c].t == NType::Element {
                work.push((c, defined));
            }
        }
    }
}

fn new_ns_force(t: &mut Tree, node: NodeId, href: &str, prefix: Option<&str>) -> NsId {
    match t.new_ns(Some(node), href, prefix) {
        Some(n) => n,
        None => t.new_ns_raw(href, prefix, Some(node)),
    }
}

/// dom_remove_attribute
fn remove_attribute(t: &mut Tree, elem: NodeId, a: AttrOrNs) {
    match a {
        AttrOrNs::Attr(at) => {
            t.remove_id_for_attr_if_attr(at);
            t.unlink(at);
        }
        AttrOrNs::Ns(ns) => {
            t.nodes[elem].ns_def.retain(|&x| x != ns);
            deep_ns_redef(t, elem, ns);
        }
    }
}

fn attr_value(t: &Tree, at: NodeId) -> String {
    t.list_get_string(t.nodes[at].children, true)
}

// ---------------------------------------------------------------------------
// Properties

fn r_tag_name(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| s(&t.qname(n))))
}

fn reflected_read(n: NodeId, name: &str) -> Value {
    tree::with(|t| s(&t.get_no_ns_prop(n, name).unwrap_or_default()))
}

fn r_class_name(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(reflected_read(n, "class"))
}

fn w_class_name(_vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let st = str_of(v);
    tree::with(|t| {
        t.set_ns_prop(n, None, "class", cstr(&st));
    });
    Ok(())
}

fn r_id(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(reflected_read(n, "id"))
}

fn w_id(_vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let st = str_of(v);
    tree::with(|t| {
        let at = t.set_ns_prop(n, None, "id", cstr(&st));
        t.nodes[at].atype = 2;
    });
    Ok(())
}

fn r_null(_vm: &mut Vm, _o: &Obj, _n: NodeId) -> R<Value> {
    Ok(Value::Null)
}

pub fn element_props() -> Vec<Prop> {
    let mut v = vec![
        p("tagName", r_tag_name, None, "string"),
        p("className", r_class_name, Some(w_class_name), "string"),
        p("id", r_id, Some(w_id), "string"),
        p("schemaTypeInfo", r_null, None, "mixed"),
    ];
    v.extend(super::node::parent_props());
    v.push(p(
        "previousElementSibling",
        super::node::r_prev_element_sibling,
        None,
        "mixed",
    ));
    v.push(p(
        "nextElementSibling",
        super::node::r_next_element_sibling,
        None,
        "mixed",
    ));
    v
}

fn r_attr_name(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| s(&t.nodes[n].name)))
}

fn r_true(_vm: &mut Vm, _o: &Obj, _n: NodeId) -> R<Value> {
    Ok(Value::True)
}

fn r_attr_value(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| super::node::get_content_value(t, n, false)))
}

fn w_attr_value(_vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let st = str_of(v);
    tree::with(|t| {
        let was_id = t.nodes[n].atype == 2;
        t.remove_id_for_attr_if_attr(n);
        if was_id {
            t.nodes[n].atype = 2;
        }
        set_content_legacy(t, n, &st);
    });
    Ok(())
}

fn r_owner_element(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    let p = tree::with(|t| t.nodes[n].parent);
    Ok(node_obj_opt(vm, p))
}

pub fn attr_props() -> Vec<Prop> {
    vec![
        p("name", r_attr_name, None, "string"),
        p("specified", r_true, None, "bool"),
        p("value", r_attr_value, Some(w_attr_value), "string"),
        p("ownerElement", r_owner_element, None, "mixed"),
        p("schemaTypeInfo", r_null, None, "mixed"),
    ]
}

// ---------------------------------------------------------------------------
// Constructors

pub fn construct_element(vm: &mut Vm, this: &Obj, a: &[Value]) -> R<()> {
    let a = args(a);
    a.count(vm, 1, 3)?;
    let name = a.str(vm, 0, "qualifiedName")?;
    let value = a.opt_str(vm, 1, "value")?;
    let uri = a.str(vm, 2, "namespace")?;
    if !is_valid_name(&name) {
        throw_dom(vm, INVALID_CHARACTER_ERR, true);
        return Err(());
    }
    let n = if !uri.is_empty() {
        let r = tree::with(|t| -> Result<NodeId, i64> {
            let (prefix, local) = check_qname(&name, uri.len())?;
            let e = t.new_element(None, &local);
            let ns = get_ns(t, e, &uri, prefix.as_deref())?;
            t.nodes[e].ns = Some(ns);
            Ok(e)
        });
        match r {
            Ok(e) => e,
            Err(code) => {
                throw_dom(vm, code, true);
                return Err(());
            }
        }
    } else {
        if name
            .find(':')
            .map(|i| i > 0 && i + 1 < name.len())
            .unwrap_or(false)
        {
            throw_dom(vm, NAMESPACE_ERR, true);
            return Err(());
        }
        tree::with(|t| t.new_element(None, &name))
    };
    if let Some(v) = value.filter(|v| !v.is_empty()) {
        tree::with(|t| set_content_legacy(t, n, &v));
    }
    attach(this, Kind::Node(n));
    set_wrapper(n, this);
    Ok(())
}

pub fn construct_attr(vm: &mut Vm, this: &Obj, a: &[Value]) -> R<()> {
    let a = args(a);
    a.count(vm, 1, 2)?;
    let name = a.str(vm, 0, "name")?;
    let value = a.str(vm, 1, "value")?;
    if !is_valid_name(&name) {
        throw_dom(vm, INVALID_CHARACTER_ERR, true);
        return Err(());
    }
    let n = tree::with(|t| {
        let at = t.new_attr_node(None, None, &name, None);
        if !value.is_empty() {
            let tx = t.new_text(None, &value);
            t.link_child_raw(at, tx);
        }
        at
    });
    attach(this, Kind::Node(n));
    set_wrapper(n, this);
    Ok(())
}

// ---------------------------------------------------------------------------
// Methods

fn ns_obj(vm: &mut Vm, elem: NodeId, ns: NsId) -> Value {
    let id = tree::with(|t| crate::xpath::ns_node(t, elem, ns));
    node_obj(vm, id)
}

fn m_get_attribute(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let name = a.str(vm, 0, "qualifiedName")?;
    let n = need_node(vm, o)?;
    Ok(tree::with(|t| match get_attribute_or_nsdecl(t, n, &name) {
        Some(AttrOrNs::Attr(at)) => s(&attr_value(t, at)),
        Some(AttrOrNs::Ns(ns)) => s(&t.nss[ns].href),
        None => s(""),
    }))
}

fn m_get_attribute_names(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let n = need_node(vm, o)?;
    let names: Vec<String> = tree::with(|t| {
        let mut v = Vec::new();
        for &ns in &t.nodes[n].ns_def {
            match &t.nss[ns].prefix {
                None => v.push("xmlns".to_string()),
                Some(p) => v.push(format!("xmlns:{}", p)),
            }
        }
        for at in t.attrs_of(n) {
            v.push(t.qname(at));
        }
        v
    });
    let mut arr = PhpArray::new();
    for nm in names {
        arr.push(s(&nm));
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn m_set_attribute(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let name = a.str(vm, 0, "qualifiedName")?;
    let value = cstr(&a.str(vm, 1, "value")?).to_string();
    if name.is_empty() {
        throw_error(
            vm,
            b"ValueError",
            &format!("{}(): Argument #1 ($qualifiedName) must not be empty", a.f),
        );
        return Err(());
    }
    if !is_valid_name(&name) {
        throw_dom(vm, INVALID_CHARACTER_ERR, true);
        return Err(());
    }
    let n = need_node(vm, o)?;
    let r = tree::with(|t| -> Option<Option<NodeId>> {
        match get_attribute_or_nsdecl(t, n, &name) {
            Some(AttrOrNs::Ns(_)) => return None,
            Some(AttrOrNs::Attr(at)) => {
                let was_id = t.nodes[at].atype == 2;
                t.remove_id_for_attr_if_attr(at);
                if was_id {
                    t.nodes[at].atype = 2;
                }
                super::node::remove_all_children(t, at);
            }
            None => {}
        }
        if name == "xmlns" {
            t.new_ns(Some(n), &value, None);
            return Some(None);
        }
        Some(Some(set_prop(t, n, &name, &value)))
    });
    match r {
        None => Ok(Value::False),
        Some(None) => Ok(Value::True),
        Some(Some(at)) => Ok(node_obj(vm, at)),
    }
}

fn m_remove_attribute(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let name = a.str(vm, 0, "qualifiedName")?;
    let n = need_node(vm, o)?;
    Ok(b(tree::with(|t| {
        match get_attribute_or_nsdecl(t, n, &name) {
            Some(x) => {
                remove_attribute(t, n, x);
                true
            }
            None => false,
        }
    })))
}

fn m_get_attribute_node(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let name = a.str(vm, 0, "qualifiedName")?;
    let n = need_node(vm, o)?;
    match tree::with(|t| get_attribute_or_nsdecl(t, n, &name)) {
        None => Ok(Value::False),
        Some(AttrOrNs::Attr(at)) => Ok(node_obj(vm, at)),
        Some(AttrOrNs::Ns(ns)) => Ok(ns_obj(vm, n, ns)),
    }
}

fn set_attribute_node_common(vm: &mut Vm, o: &Obj, a: &[Value], use_ns: bool) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let ao = a.obj_of(vm, 0, "attr", "DOMAttr", false)?.unwrap();
    let n = need_node(vm, o)?;
    let at = need_node(vm, &ao)?;
    let (adoc, ndoc) = tree::with(|t| (t.nodes[at].doc, t.nodes[n].doc));
    if !(adoc.is_none() || adoc == ndoc) {
        throw_dom(vm, WRONG_DOCUMENT_ERR, strict(doc_of_node(n)));
        return Ok(Value::False);
    }
    let r = tree::with(|t| -> Result<Option<NodeId>, ()> {
        let nsp = t.nodes[at].ns;
        let aname = t.nodes[at].name.clone();
        let exist = match (use_ns, nsp) {
            (true, Some(ns)) => {
                let h = t.nss[ns].href.clone();
                t.has_ns_prop(n, &aname, Some(&h))
            }
            _ => t.has_prop(n, &aname),
        };
        if let Some(e) = exist {
            if e == at {
                return Err(());
            }
            t.unlink(e);
        }
        if t.nodes[at].parent.is_some() {
            t.unlink(at);
        }
        if t.nodes[at].doc.is_none() && ndoc.is_some() {
            t.set_tree_doc(at, ndoc);
        }
        t.link_child_raw(n, at);
        super::node::reconcile_attribute_ns_after_insertion(t, at);
        Ok(exist)
    });
    match r {
        Err(()) => Ok(Value::Null),
        Ok(Some(e)) => Ok(node_obj(vm, e)),
        Ok(None) => Ok(Value::Null),
    }
}

fn m_set_attribute_node(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    set_attribute_node_common(vm, o, a, false)
}

fn m_set_attribute_node_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    set_attribute_node_common(vm, o, a, true)
}

fn m_remove_attribute_node(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let ao = a.obj_of(vm, 0, "attr", "DOMAttr", false)?.unwrap();
    let n = need_node(vm, o)?;
    let at = need_node(vm, &ao)?;
    if tree::with(|t| t.nodes[at].parent) != Some(n) {
        throw_dom(vm, NOT_FOUND_ERR, strict(doc_of_node(n)));
        return Ok(Value::False);
    }
    tree::with(|t| t.unlink(at));
    Ok(node_obj(vm, at))
}

fn m_get_elements_by_tag_name(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let name = a.path_str(vm, 0, "qualifiedName")?;
    need_node(vm, o)?;
    Ok(list_obj(
        vm,
        ListKind::ByTag {
            base: o.clone(),
            ns: None,
            local: name,
        },
    ))
}

fn m_get_elements_by_tag_name_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let ns = a.opt_path_str(vm, 0, "namespace")?;
    let name = a.path_str(vm, 1, "localName")?;
    need_node(vm, o)?;
    Ok(list_obj(
        vm,
        ListKind::ByTag {
            base: o.clone(),
            ns: Some(ns.unwrap_or_default()),
            local: name,
        },
    ))
}

/// dom_get_attribute_ns (legacy)
fn get_attribute_ns(t: &Tree, elem: NodeId, uri: Option<&str>, name: &str) -> Option<String> {
    // (xmlGetNsProp(): "" is a namespace no attribute is in)
    if let Some(at) = t.has_ns_prop(elem, name, uri) {
        return Some(attr_value(t, at));
    }
    if uri == Some(XMLNS_NS) {
        return get_nsdecl(t, elem, Some(name)).map(|ns| t.nss[ns].href.clone());
    }
    None
}

fn m_get_attribute_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let uri = a.opt_str(vm, 0, "namespace")?;
    let name = a.str(vm, 1, "localName")?;
    let n = need_node(vm, o)?;
    Ok(s(&tree::with(|t| {
        get_attribute_ns(t, n, uri.as_deref(), &name)
    })
    .unwrap_or_default()))
}

fn m_has_attribute_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let uri = a.opt_str(vm, 0, "namespace")?;
    let name = a.str(vm, 1, "localName")?;
    let n = need_node(vm, o)?;
    Ok(b(tree::with(|t| {
        get_attribute_ns(t, n, uri.as_deref(), &name)
    })
    .is_some()))
}

fn m_set_attribute_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 3, 3)?;
    let uri = a.opt_str(vm, 0, "namespace")?;
    let name = a.str(vm, 1, "qualifiedName")?;
    let value = cstr(&a.str(vm, 2, "value")?).to_string();
    let n = need_node(vm, o)?;
    if name.is_empty() {
        throw_error(
            vm,
            b"ValueError",
            &format!("{}(): Argument #2 ($qualifiedName) must not be empty", a.f),
        );
        return Err(());
    }
    let uri = uri.unwrap_or_default();
    let mut stricterror = strict(doc_of_node(n));
    let r = tree::with(|t| -> Result<(), i64> {
        let (prefix, local) = check_qname(&name, uri.len())?;
        if !uri.is_empty() {
            if let Some(at) = t.has_ns_prop(n, &local, Some(&uri)) {
                super::node::remove_all_children(t, at);
            }
            let is_xmlns = (prefix.as_deref() == Some("xmlns")
                || (prefix.is_none() && local == "xmlns"))
                && uri == XMLNS_NS;
            let mut nsptr;
            if is_xmlns {
                nsptr = if prefix.is_none() {
                    get_nsdecl(t, n, None)
                } else {
                    get_nsdecl(t, n, Some(&local))
                };
            } else {
                nsptr = t.search_ns_by_href(n, &uri);
                if let Some(np) = nsptr {
                    if t.nss[np].prefix.is_none() {
                        // look for a prefixed declaration of the same URI
                        let alt = t.nodes[n].ns_def.iter().copied().find(|&x| {
                            x != np && t.nss[x].prefix.is_some() && t.nss[x].href == uri
                        });
                        nsptr = match alt {
                            Some(x) => Some(x),
                            None => super::document::resolve_prefix_conflict(t, n, &uri),
                        };
                    }
                }
            }
            let mut err = 0;
            match nsptr {
                None => {
                    if is_xmlns {
                        t.new_ns(
                            Some(n),
                            &value,
                            if prefix.is_none() { None } else { Some(&local) },
                        );
                    } else {
                        match get_ns(t, n, &uri, prefix.as_deref()) {
                            Ok(x) => nsptr = Some(x),
                            Err(c) => err = c,
                        }
                    }
                    super::node::reconciliate_ns(t, n);
                }
                Some(np) => {
                    if is_xmlns {
                        t.nss[np].href = value.clone();
                    }
                }
            }
            if err != 0 {
                return Err(err);
            }
            if !is_xmlns {
                t.set_ns_prop(n, nsptr, &local, &value);
            }
        } else {
            if !is_valid_name(&local) {
                return Err(-INVALID_CHARACTER_ERR);
            }
            if let Some(at) = t.has_prop(n, &local) {
                super::node::remove_all_children(t, at);
            }
            t.set_ns_prop(n, None, &local, &value);
        }
        Ok(())
    });
    if let Err(mut code) = r {
        if code < 0 {
            code = -code;
            stricterror = true;
        }
        throw_dom(vm, code, stricterror);
        if stricterror {
            return Err(());
        }
    }
    Ok(Value::Null)
}

fn eliminate_ns(t: &mut Tree, elem: NodeId, ns: NsId) {
    t.nodes[elem].ns_def.retain(|&x| x != ns);
    let mut nodes = vec![elem];
    nodes.extend(
        t.descendants(elem)
            .into_iter()
            .filter(|&x| t.nodes[x].t == NType::Element),
    );
    for x in nodes {
        if t.nodes[x].ns == Some(ns) {
            t.nodes[x].ns = None;
        }
        for at in t.attrs_of(x) {
            if t.nodes[at].ns == Some(ns) {
                t.nodes[at].ns = None;
            }
        }
    }
}

fn m_remove_attribute_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let uri = a.opt_str(vm, 0, "namespace")?;
    let name = a.str(vm, 1, "localName")?;
    let n = need_node(vm, o)?;
    tree::with(|t| {
        // (legacy: "" stays a namespace name, as in xmlHasNsProp())
        let at = t.has_ns_prop(n, &name, uri.as_deref());
        if let Some(ns) = get_nsdecl(t, n, Some(&name)) {
            if uri.as_deref() == Some(t.nss[ns].href.as_str()) {
                eliminate_ns(t, n, ns);
            } else {
                return;
            }
        }
        if let Some(at) = at {
            t.remove_id_for_attr_if_attr(at);
            t.unlink(at);
        }
    });
    Ok(Value::Null)
}

fn m_get_attribute_node_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let uri = a.opt_str(vm, 0, "namespace")?;
    let name = a.str(vm, 1, "localName")?;
    let n = need_node(vm, o)?;
    // (xmlHasNsProp(): "" is a namespace no attribute is in)
    let at = tree::with(|t| t.has_ns_prop(n, &name, uri.as_deref()));
    match at {
        Some(at) => Ok(node_obj(vm, at)),
        None => {
            if uri.as_deref() == Some(XMLNS_NS) {
                if let Some(ns) = tree::with(|t| get_nsdecl(t, n, Some(&name))) {
                    return Ok(ns_obj(vm, n, ns));
                }
            }
            Ok(Value::Null)
        }
    }
}

fn m_has_attribute(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let name = a.str(vm, 0, "qualifiedName")?;
    let n = need_node(vm, o)?;
    Ok(b(
        tree::with(|t| get_attribute_or_nsdecl(t, n, &name)).is_some()
    ))
}

fn set_attribute_id(t: &mut Tree, at: NodeId, is_id: bool) {
    if is_id && t.nodes[at].atype != 2 {
        t.nodes[at].atype = 2;
        if let Some(d) = t.nodes[at].doc {
            let v = attr_value(t, at);
            if let Some(di) = t.doc_info_mut(d) {
                di.ids.entry(v).or_insert(at);
            }
        }
    } else if !is_id && t.nodes[at].atype == 2 {
        t.remove_id_for_attr(at);
        t.nodes[at].atype = 0;
    }
}

fn m_set_id_attribute(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let name = a.str(vm, 0, "qualifiedName")?;
    let is_id = a.bool(vm, 1, "isId", false)?;
    let n = need_node(vm, o)?;
    let at = tree::with(|t| t.has_ns_prop(n, &name, None));
    match at {
        None => throw_dom(vm, NOT_FOUND_ERR, strict(doc_of_node(n))),
        Some(at) => tree::with(|t| set_attribute_id(t, at, is_id)),
    }
    Ok(Value::Null)
}

fn m_set_id_attribute_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 3, 3)?;
    let uri = a.str(vm, 0, "namespace")?;
    let name = a.str(vm, 1, "qualifiedName")?;
    let is_id = a.bool(vm, 2, "isId", false)?;
    let n = need_node(vm, o)?;
    let at = tree::with(|t| t.has_ns_prop(n, &name, Some(&uri)));
    match at {
        None => throw_dom(vm, NOT_FOUND_ERR, strict(doc_of_node(n))),
        Some(at) => tree::with(|t| set_attribute_id(t, at, is_id)),
    }
    Ok(Value::Null)
}

fn m_set_id_attribute_node(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let ao = a.obj_of(vm, 0, "attr", "DOMAttr", false)?.unwrap();
    let is_id = a.bool(vm, 1, "isId", false)?;
    let n = need_node(vm, o)?;
    let at = need_node(vm, &ao)?;
    if tree::with(|t| t.nodes[at].parent) != Some(n) {
        throw_dom(vm, NOT_FOUND_ERR, strict(doc_of_node(n)));
    } else {
        tree::with(|t| set_attribute_id(t, at, is_id));
    }
    Ok(Value::Null)
}

enum Adj {
    Null,
    Failed,
    Ok,
}

fn insert_adjacent(vm: &mut Vm, wh: &str, thisp: NodeId, other: NodeId) -> Adj {
    let w = wh.to_ascii_lowercase();
    let st = strict(doc_of_node(thisp));
    let (parent, next, first, doc) = tree::with(|t| {
        (
            t.nodes[thisp].parent,
            t.nodes[thisp].next,
            t.nodes[thisp].children,
            t.nodes[thisp].doc,
        )
    });
    let adopt = |other: NodeId| {
        tree::with(|t| {
            if t.nodes[other].doc != doc {
                t.unlink(other);
                t.set_tree_doc(other, doc);
            }
        })
    };
    let (p, ip) = match w.as_str() {
        "beforebegin" => match parent {
            None => return Adj::Null,
            Some(p) => (p, Some(thisp)),
        },
        "afterbegin" => (thisp, first),
        "beforeend" => (thisp, None),
        "afterend" => match parent {
            None => return Adj::Null,
            Some(p) => (p, next),
        },
        _ => {
            throw_dom(vm, SYNTAX_ERR, st);
            return Adj::Failed;
        }
    };
    adopt(other);
    if !super::node::pre_insert(vm, other, p, ip, st) {
        return Adj::Failed;
    }
    Adj::Ok
}

fn m_insert_adjacent_element(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let wh = a.str(vm, 0, "where")?;
    let eo = a.obj_of(vm, 1, "element", "DOMElement", false)?.unwrap();
    let n = need_node(vm, o)?;
    let other = need_node(vm, &eo)?;
    match insert_adjacent(vm, &wh, n, other) {
        Adj::Null => Ok(Value::Null),
        Adj::Ok => Ok(Value::Object(eo)),
        Adj::Failed => {
            if vm.current_exception.is_some() {
                Err(())
            } else {
                Ok(Value::Null)
            }
        }
    }
}

fn m_insert_adjacent_text(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let wh = a.str(vm, 0, "where")?;
    let data = a.str(vm, 1, "data")?;
    let n = need_node(vm, o)?;
    let doc = tree::with(|t| t.nodes[n].doc);
    let tx = tree::with(|t| t.new_text(doc, &data));
    let _ = insert_adjacent(vm, &wh, n, tx);
    Ok(Value::Null)
}

fn m_toggle_attribute(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 2)?;
    let mut name = a.str(vm, 0, "qualifiedName")?;
    let force = match a.raw(1) {
        None | Some(Value::Null) => None,
        Some(_) => Some(a.bool(vm, 1, "force", false)?),
    };
    let n = need_node(vm, o)?;
    if !is_valid_name(&name) {
        throw_dom(vm, INVALID_CHARACTER_ERR, true);
        return Err(());
    }
    let html = tree::with(|t| {
        t.nodes[n]
            .doc
            .map(|d| t.nodes[d].t == NType::HtmlDocument)
            .unwrap_or(false)
            && t.nodes[n].ns.is_none()
    });
    if html {
        name = name.to_ascii_lowercase();
    }
    let r = tree::with(|t| match get_attribute_or_nsdecl(t, n, &name) {
        None => {
            if force.unwrap_or(true) {
                match name.find(':').filter(|&i| i > 0 && i + 1 < name.len()) {
                    Some(i) if &name[..i] == "xmlns" => {
                        t.new_ns(Some(n), "", Some(&name[i + 1..]));
                    }
                    _ => {
                        if name == "xmlns" {
                            t.new_ns(Some(n), "", None);
                        } else {
                            set_prop(t, n, &name, "");
                        }
                    }
                }
                true
            } else {
                false
            }
        }
        Some(x) => {
            if !force.unwrap_or(false) {
                remove_attribute(t, n, x);
                false
            } else {
                true
            }
        }
    });
    Ok(b(r))
}

pub fn element_methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("getAttribute", m_get_attribute),
        ("getAttributeNames", m_get_attribute_names),
        ("setAttribute", m_set_attribute),
        ("removeAttribute", m_remove_attribute),
        ("getAttributeNode", m_get_attribute_node),
        ("setAttributeNode", m_set_attribute_node),
        ("removeAttributeNode", m_remove_attribute_node),
        ("getElementsByTagName", m_get_elements_by_tag_name),
        ("getAttributeNS", m_get_attribute_ns),
        ("setAttributeNS", m_set_attribute_ns),
        ("removeAttributeNS", m_remove_attribute_ns),
        ("getAttributeNodeNS", m_get_attribute_node_ns),
        ("setAttributeNodeNS", m_set_attribute_node_ns),
        ("getElementsByTagNameNS", m_get_elements_by_tag_name_ns),
        ("hasAttribute", m_has_attribute),
        ("hasAttributeNS", m_has_attribute_ns),
        ("setIdAttribute", m_set_id_attribute),
        ("setIdAttributeNS", m_set_id_attribute_ns),
        ("setIdAttributeNode", m_set_id_attribute_node),
        ("insertAdjacentElement", m_insert_adjacent_element),
        ("insertAdjacentText", m_insert_adjacent_text),
        ("toggleAttribute", m_toggle_attribute),
    ]
}

fn m_is_id(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let n = need_node(vm, o)?;
    Ok(b(tree::with(|t| t.nodes[n].atype == 2)))
}

pub fn attr_methods() -> Vec<(&'static str, MethodFn)> {
    vec![("isId", m_is_id)]
}
