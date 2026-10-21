//! Dom\Element, Dom\Attr, Dom\NamespaceInfo (element.c and attr.c, modern
//! branches).

use super::*;
use crate::dom::node::{get_content_value, remove_all_children, str_of};
use crate::parser::{is_valid_name, is_valid_qname};

const ELEMENT: &str = "Dom\\Element";
const ATTR: &str = "Dom\\Attr";

/// dom_attr_value_will_change: the attribute leaves the ID table (but keeps
/// its ID type).
pub fn attr_value_will_change(t: &mut Tree, at: NodeId) {
    if t.nodes[at].atype == 2 {
        t.remove_id_for_attr(at);
        t.nodes[at].atype = 2;
    }
}

/// dom_match_qualified_name_according_to_spec
pub fn match_qname(t: &Tree, qname: &str, n: NodeId) -> bool {
    let local = &t.nodes[n].name;
    match t.nodes[n].ns.and_then(|x| t.nss[x].prefix.as_deref()) {
        Some(p) => {
            qname.len() > p.len()
                && qname.starts_with(p)
                && qname.as_bytes()[p.len()] == b':'
                && &qname[p.len() + 1..] == local
        }
        None => local == qname,
    }
}

/// php_dom_get_attribute_node: attribute by qualified name.
pub fn get_attribute_node(t: &Tree, elem: NodeId, name: &str) -> Option<NodeId> {
    let lowered;
    let name = if ns_is_html_and_doc_is_html(t, elem) {
        lowered = name.to_ascii_lowercase();
        lowered.as_str()
    } else {
        name
    };
    let mut a = t.nodes[elem].properties;
    while let Some(x) = a {
        if match_qname(t, name, x) {
            return Some(x);
        }
        a = t.nodes[x].next;
    }
    None
}

/// dom_check_register_attribute_id
fn check_register_id(t: &mut Tree, at: NodeId) {
    let html_doc = t.nodes[at]
        .doc
        .map(|d| t.nodes[d].t == NType::HtmlDocument)
        .unwrap_or(false);
    if t.nodes[at].atype != 2 && html_doc && t.nodes[at].ns.is_none() && t.nodes[at].name == "id" {
        t.nodes[at].atype = 2;
    }
}

fn this_node(vm: &mut Vm, o: &Obj) -> R<NodeId> {
    need_node(vm, o)
}

fn doc_of(t: &Tree, n: NodeId) -> NodeId {
    doc_from_node(t, n).unwrap_or(n)
}

// ---------------------------------------------------------------------------
// Properties

fn r_prefix(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| opt_s(t.ns_prefix(t.nodes[n].ns))))
}

fn r_tag_name(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| {
        let up = ns_is_html_and_doc_is_html(t, n);
        s(&node::qname_of(t, n, up))
    }))
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
        t.set_ns_prop(n, None, "class", &st);
    });
    Ok(())
}

fn r_id(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(reflected_read(n, "id"))
}

fn w_id(_vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let st = str_of(v);
    tree::with(|t| {
        let at = t.set_ns_prop(n, None, "id", &st);
        t.nodes[at].atype = 2;
    });
    Ok(())
}

fn r_class_list(vm: &mut Vm, o: &Obj, _n: NodeId) -> R<Value> {
    let el = o.clone();
    let c = cached_obj(o, "classList", true, || tokenlist::new_token_list(vm, el));
    Ok(Value::Object(c))
}

fn r_attributes(vm: &mut Vm, o: &Obj, _n: NodeId) -> R<Value> {
    Ok(mmap_obj(
        vm,
        b"Dom\\NamedNodeMap",
        MapKind::Attributes(o.clone()),
    ))
}

fn r_substituted(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    let _ = vm;
    Ok(tree::with(|t| s(&t.node_get_content(n))))
}

fn w_substituted(_vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let st = str_of(v);
    tree::with(|t| {
        remove_all_children(t, n);
        t.node_set_content(n, &st);
    });
    Ok(())
}

pub fn element_props(t: &HashMap<Base, Rc<Vec<Prop>>>) -> Vec<Prop> {
    let l = |n: &str| {
        let mut p = lp(t, Base::Node, n);
        p.owner = "";
        p
    };
    let mut v = vec![
        l("namespaceURI"),
        p("prefix", r_prefix, None, "?string"),
        l("localName"),
        p("tagName", r_tag_name, None, "string"),
        p("id", r_id, Some(w_id), "string"),
        p("className", r_class_name, Some(w_class_name), "string"),
        p("classList", r_class_list, None, "mixed"),
        p("attributes", r_attributes, None, "mixed"),
    ];
    v.extend(node::parent_props(t));
    v.push(p(
        "previousElementSibling",
        crate::dom::node::r_prev_element_sibling,
        None,
        "mixed",
    ));
    v.push(p(
        "nextElementSibling",
        crate::dom::node::r_next_element_sibling,
        None,
        "mixed",
    ));
    v.push(p(
        "innerHTML",
        inner_html::r_inner_html,
        Some(inner_html::w_inner_html),
        "string",
    ));
    v.push(p(
        "outerHTML",
        inner_html::r_outer_html,
        Some(inner_html::w_outer_html),
        "string",
    ));
    v.push(p(
        "substitutedNodeValue",
        r_substituted,
        Some(w_substituted),
        "string",
    ));
    v
}

fn r_attr_name(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| s(&t.qname(n))))
}

fn r_attr_value(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| get_content_value(t, n, false)))
}

fn w_attr_value(_vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let st = str_of(v);
    tree::with(|t| {
        attr_value_will_change(t, n);
        remove_all_children(t, n);
        let doc = t.nodes[n].doc;
        let tx = t.new_text(doc, &st);
        t.link_child_raw(n, tx);
    });
    Ok(())
}

fn r_owner_element(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    let p = tree::with(|t| t.nodes[n].parent);
    Ok(node_obj_opt(vm, p))
}

fn r_true(_vm: &mut Vm, _o: &Obj, _n: NodeId) -> R<Value> {
    Ok(Value::True)
}

pub fn attr_props(t: &HashMap<Base, Rc<Vec<Prop>>>) -> Vec<Prop> {
    let l = |n: &str| {
        let mut p = lp(t, Base::Node, n);
        p.owner = "";
        p
    };
    vec![
        l("namespaceURI"),
        p("prefix", r_prefix, None, "?string"),
        l("localName"),
        p("name", r_attr_name, None, "string"),
        p("value", r_attr_value, Some(w_attr_value), "string"),
        p("ownerElement", r_owner_element, None, "mixed"),
        p("specified", r_true, None, "bool"),
    ]
}

// ---------------------------------------------------------------------------
// Attribute methods

fn m_has_attributes(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let n = this_node(vm, o)?;
    Ok(bv(tree::with(|t| {
        t.nodes[n].t == NType::Element && t.nodes[n].properties.is_some()
    })))
}

fn m_get_attribute_names(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let n = this_node(vm, o)?;
    let names = tree::with(|t| {
        t.attrs_of(n)
            .into_iter()
            .map(|x| t.qname(x))
            .collect::<Vec<_>>()
    });
    let mut arr = PhpArray::new();
    for nm in names {
        arr.push(s(&nm));
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn m_get_attribute(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let name = a.str(vm, 0, "qualifiedName")?;
    let n = this_node(vm, o)?;
    Ok(tree::with(|t| match get_attribute_node(t, n, &name) {
        Some(at) => s(&t.list_get_string(t.nodes[at].children, true)),
        None => Value::Null,
    }))
}

fn get_attribute_ns(t: &Tree, elem: NodeId, uri: Option<&str>, name: &str) -> Option<NodeId> {
    let uri = uri.filter(|u| !u.is_empty());
    t.has_ns_prop(elem, name, uri)
}

fn m_get_attribute_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let uri = a.opt_str(vm, 0, "namespace")?;
    let name = a.str(vm, 1, "localName")?;
    let n = this_node(vm, o)?;
    Ok(tree::with(|t| {
        match get_attribute_ns(t, n, uri.as_deref(), &name) {
            Some(at) => s(&t.node_get_content(at)),
            None => Value::Null,
        }
    }))
}

fn m_has_attribute_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let uri = a.opt_str(vm, 0, "namespace")?;
    let name = a.str(vm, 1, "localName")?;
    let n = this_node(vm, o)?;
    Ok(bv(tree::with(|t| {
        get_attribute_ns(t, n, uri.as_deref(), &name).is_some()
    })))
}

fn m_get_attribute_node_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let uri = a.opt_str(vm, 0, "namespace")?;
    let name = a.str(vm, 1, "localName")?;
    let n = this_node(vm, o)?;
    let at = tree::with(|t| get_attribute_ns(t, n, uri.as_deref(), &name));
    Ok(node_obj_opt(vm, at))
}

fn m_set_attribute(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let name = a.str(vm, 0, "qualifiedName")?;
    let value = a.str(vm, 1, "value")?;
    if name.is_empty() {
        throw_error(
            vm,
            b"ValueError",
            &format!("{}(): Argument #1 ($qualifiedName) must not be empty", a.f),
        );
        return Err(());
    }
    if !is_valid_name(crate::dom::node::cstr(&name)) {
        dom_err(vm, INVALID_CHARACTER_ERR);
        return Err(());
    }
    let n = this_node(vm, o)?;
    tree::with(|t| {
        let processed = if ns_is_html_and_doc_is_html(t, n) {
            name.to_ascii_lowercase()
        } else {
            name.clone()
        };
        match get_attribute_node(t, n, &name) {
            Some(at) => {
                attr_value_will_change(t, at);
                remove_all_children(t, at);
                let doc = t.nodes[at].doc;
                let tx = t.new_text(doc, crate::dom::node::cstr(&value));
                t.add_child(at, tx);
            }
            None => {
                let at = t.set_ns_prop(n, None, &processed, crate::dom::node::cstr(&value));
                check_register_id(t, at);
            }
        }
    });
    Ok(Value::Null)
}

fn m_set_attribute_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 3, 3)?;
    let uri = a.opt_str(vm, 0, "namespace")?;
    let name = a.str(vm, 1, "qualifiedName")?;
    let value = a.str(vm, 2, "value")?;
    let n = this_node(vm, o)?;
    match crate::dom::document::validate_and_extract(uri.as_deref(), &name) {
        Ok((prefix, local)) => {
            tree::with(|t| {
                let d = doc_of(t, n);
                let ns = mapper_get_ns(t, d, prefix.as_deref(), uri.as_deref());
                let at = t.set_ns_prop(n, ns, &local, crate::dom::node::cstr(&value));
                check_register_id(t, at);
            });
            Ok(Value::Null)
        }
        Err(code) => {
            dom_err(vm, code);
            Err(())
        }
    }
}

fn m_remove_attribute(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let name = a.str(vm, 0, "qualifiedName")?;
    let n = this_node(vm, o)?;
    tree::with(|t| {
        if let Some(at) = get_attribute_node(t, n, &name) {
            t.remove_id_for_attr(at);
            t.unlink(at);
        }
    });
    Ok(Value::Null)
}

fn m_remove_attribute_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let uri = a.opt_str(vm, 0, "namespace")?;
    let name = a.str(vm, 1, "localName")?;
    let n = this_node(vm, o)?;
    tree::with(|t| {
        if let Some(at) = get_attribute_ns(t, n, uri.as_deref(), &name) {
            t.remove_id_for_attr(at);
            t.unlink(at);
        }
    });
    Ok(Value::Null)
}

fn m_toggle_attribute(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 2)?;
    let qname = a.str(vm, 0, "qualifiedName")?;
    let force = match a.raw(1) {
        None | Some(Value::Null) => None,
        Some(_) => Some(a.bool(vm, 1, "force", false)?),
    };
    let n = this_node(vm, o)?;
    if !is_valid_name(crate::dom::node::cstr(&qname)) {
        dom_err(vm, INVALID_CHARACTER_ERR);
        return Err(());
    }
    let r = tree::with(|t| {
        let html_doc = t.nodes[n]
            .doc
            .map(|d| t.nodes[d].t == NType::HtmlDocument)
            .unwrap_or(false);
        let q = if html_doc && ns_is(t, t.nodes[n].ns, XHTML_NS) {
            qname.to_ascii_lowercase()
        } else {
            qname.clone()
        };
        match get_attribute_node(t, n, &q) {
            None => {
                if force.unwrap_or(true) {
                    let doc = t.nodes[n].doc;
                    let at = t.new_attr_node(doc, None, &q, None);
                    t.link_child_raw(n, at);
                    true
                } else {
                    false
                }
            }
            Some(at) => {
                if !force.unwrap_or(false) {
                    t.remove_id_for_attr(at);
                    t.unlink(at);
                    false
                } else {
                    true
                }
            }
        }
    });
    Ok(bv(r))
}

fn m_has_attribute(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let name = a.str(vm, 0, "qualifiedName")?;
    let n = this_node(vm, o)?;
    Ok(bv(tree::with(|t| {
        get_attribute_node(t, n, &name).is_some()
    })))
}

fn m_get_attribute_node(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let name = a.str(vm, 0, "qualifiedName")?;
    let n = this_node(vm, o)?;
    let at = tree::with(|t| get_attribute_node(t, n, &name));
    Ok(node_obj_opt(vm, at))
}

fn set_attribute_node_common(vm: &mut Vm, o: &Obj, a: &[Value], use_ns: bool) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let (_ao, at) = arg_node(vm, &a, 0, "attr", ATTR, false)?.unwrap();
    let n = this_node(vm, o)?;
    let inuse = tree::with(|t| t.nodes[at].parent.is_some() && t.nodes[at].parent != Some(n));
    if inuse {
        dom_err(vm, INUSE_ATTRIBUTE_ERR);
        return Err(());
    }
    let r = tree::with(|t| {
        let ndoc = t.nodes[n].doc;
        if t.nodes[at].doc.is_some() && t.nodes[at].doc != ndoc {
            if let Some(d) = ndoc {
                document::adopt_into(t, at, d);
            }
        }
        let name = t.nodes[at].name.clone();
        let existing = match t.nodes[at].ns {
            Some(ns) if use_ns => {
                let h = t.nss[ns].href.clone();
                t.has_ns_prop(n, &name, Some(&h))
            }
            _ => t.has_prop(n, &name),
        };
        if existing == Some(at) {
            return Err(());
        }
        if let Some(e) = existing {
            t.unlink(e);
        }
        if t.nodes[at].parent.is_some() {
            t.unlink(at);
        }
        if t.nodes[at].doc.is_none() && ndoc.is_some() {
            t.set_tree_doc(at, ndoc);
        }
        t.link_child_raw(n, at);
        check_register_id(t, at);
        Ok(existing)
    });
    match r {
        Err(()) => Ok(Value::Null),
        Ok(e) => Ok(node_obj_opt(vm, e)),
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
    let (ao, at) = arg_node(vm, &a, 0, "attr", ATTR, false)?.unwrap();
    let n = this_node(vm, o)?;
    if tree::with(|t| t.nodes[at].parent != Some(n)) {
        dom_err(vm, NOT_FOUND_ERR);
        return Err(());
    }
    tree::with(|t| t.unlink(at));
    Ok(Value::Object(ao))
}

fn m_set_id_attribute_node(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let (_ao, at) = arg_node(vm, &a, 0, "attr", ATTR, false)?.unwrap();
    let is_id = a.bool(vm, 1, "isId", false)?;
    let n = this_node(vm, o)?;
    if tree::with(|t| t.nodes[at].parent != Some(n)) {
        dom_err(vm, NOT_FOUND_ERR);
        return Ok(Value::Null);
    }
    tree::with(|t| {
        if is_id {
            t.nodes[at].atype = 2;
        } else if t.nodes[at].atype == 2 {
            t.remove_id_for_attr(at);
            t.nodes[at].atype = 0;
        }
    });
    Ok(Value::Null)
}

fn m_get_elements_by_tag_name(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let name = a.path_str(vm, 0, "qualifiedName")?;
    need_node(vm, o)?;
    Ok(mlist_obj(
        vm,
        b"Dom\\HTMLCollection",
        ListKind::ByQName {
            base: o.clone(),
            qname: name,
        },
    ))
}

fn m_get_elements_by_tag_name_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let ns = a.opt_path_str(vm, 0, "namespace")?;
    let name = a.path_str(vm, 1, "localName")?;
    need_node(vm, o)?;
    Ok(mlist_obj(
        vm,
        b"Dom\\HTMLCollection",
        ListKind::ByTag {
            base: o.clone(),
            ns: Some(ns.unwrap_or_default()),
            local: name,
        },
    ))
}

/// Case name of a Dom\AdjacentPosition argument.
fn adjacent_arg(vm: &mut Vm, a: &Args, i: usize) -> R<String> {
    match a.obj_of(vm, i, "where", "Dom\\AdjacentPosition", false)? {
        Some(o) => Ok(o
            .borrow()
            .get_property(b"name")
            .to_php_string()
            .to_string_lossy()),
        None => Err(()),
    }
}

/// dom_insert_adjacent: Ok(Some(node)) inserted, Ok(None) nothing done
fn insert_adjacent(vm: &mut Vm, wh: &str, thisp: NodeId, other: NodeId) -> R<Option<NodeId>> {
    let w = wh.to_ascii_lowercase();
    let (parent, d) = tree::with(|t| (t.nodes[thisp].parent, t.nodes[thisp].doc));
    let adopt = |other: NodeId| {
        if let Some(d) = d {
            tree::with(|t| document::adopt_into(t, other, d));
        }
    };
    let ok = match w.as_str() {
        "beforebegin" => {
            let p = match parent {
                Some(p) => p,
                None => return Ok(None),
            };
            adopt(other);
            node::pre_insert(vm, Some(other), p, Some(thisp))
        }
        "afterbegin" => {
            adopt(other);
            let first = tree::with(|t| t.nodes[thisp].children);
            node::pre_insert(vm, Some(other), thisp, first)
        }
        "beforeend" => {
            adopt(other);
            node::pre_insert(vm, Some(other), thisp, None)
        }
        "afterend" => {
            let p = match parent {
                Some(p) => p,
                None => return Ok(None),
            };
            adopt(other);
            let nx = tree::with(|t| t.nodes[thisp].next);
            node::pre_insert(vm, Some(other), p, nx)
        }
        _ => {
            dom_err(vm, SYNTAX_ERR);
            return Err(());
        }
    };
    if ok { Ok(Some(other)) } else { Err(()) }
}

fn m_insert_adjacent_element(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let wh = adjacent_arg(vm, &a, 0)?;
    let (eo, other) = arg_node(vm, &a, 1, "element", ELEMENT, false)?.unwrap();
    let thisp = this_node(vm, o)?;
    match insert_adjacent(vm, &wh, thisp, other)? {
        Some(_) => Ok(Value::Object(eo)),
        None => Ok(Value::Null),
    }
}

fn m_insert_adjacent_text(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let wh = adjacent_arg(vm, &a, 0)?;
    let data = a.str(vm, 1, "data")?;
    let thisp = this_node(vm, o)?;
    let tx = tree::with(|t| {
        let d = t.nodes[thisp].doc;
        t.new_text(d, &data)
    });
    let _ = insert_adjacent(vm, &wh, thisp, tx);
    Ok(Value::Null)
}

fn m_insert_adjacent_html(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let wh = adjacent_arg(vm, &a, 0)?;
    let string = a.str(vm, 1, "string")?;
    let thisp = this_node(vm, o)?;
    let w = wh.to_ascii_lowercase();
    let mut context = match w.as_str() {
        "beforebegin" | "afterend" => {
            let p = tree::with(|t| t.nodes[thisp].parent);
            match p {
                Some(p) if !tree::with(|t| t.is_doc(p)) => p,
                _ => {
                    dom_err(vm, NO_MODIFICATION_ALLOWED_ERR);
                    return Err(());
                }
            }
        }
        _ => thisp,
    };
    let need_body = tree::with(|t| {
        t.nodes[context].t != NType::Element
            || (ns_is_html_and_doc_is_html(t, context) && t.nodes[context].name == "html")
    });
    if need_body {
        context = tree::with(|t| {
            let d = doc_of(t, thisp);
            let ns = mapper_get_ns(t, d, None, Some(XHTML_NS));
            let e = t.new_element(Some(d), "body");
            t.nodes[e].ns = ns;
            e
        });
    }
    let frag = match inner_html::parse_fragment(vm, context, &string) {
        Some(f) => f,
        None => return Err(()),
    };
    let (parent, next, first) = tree::with(|t| {
        (
            t.nodes[thisp].parent,
            t.nodes[thisp].next,
            t.nodes[thisp].children,
        )
    });
    match w.as_str() {
        "beforebegin" => {
            node::pre_insert(vm, Some(frag), parent.unwrap(), Some(thisp));
        }
        "afterend" => {
            node::pre_insert(vm, Some(frag), parent.unwrap(), next);
        }
        "afterbegin" => {
            node::pre_insert(vm, Some(frag), thisp, first);
        }
        _ => {
            node::pre_insert(vm, Some(frag), thisp, None);
        }
    }
    Ok(Value::Null)
}

fn m_closest(vm: &mut Vm, _o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let _sel = a.str(vm, 0, "selectors")?;
    dom_err_msg(vm, SYNTAX_ERR, "CSS selectors are not supported");
    Err(())
}

/// dom_element_get_in_scope_namespace_info
fn in_scope_namespace_info(vm: &mut Vm, out: &mut PhpArray, node_: NodeId) {
    let entries: Vec<(String, Option<String>)> = tree::with(|t| {
        let d = doc_of(t, node_);
        let mut table: Vec<(String, Option<NsId>)> = Vec::new();
        let mut cur = Some(node_);
        while let Some(c) = cur {
            if t.nodes[c].t == NType::Element {
                let attrs = t.attrs_of(c);
                for &at in attrs.iter().rev() {
                    if let Some(ans) = t.nodes[at].ns {
                        if t.nss[ans].href == XMLNS_NS && t.nodes[at].children.is_some() {
                            let prefix = if t.nss[ans].prefix.is_none() {
                                None
                            } else {
                                Some(t.nodes[at].name.clone())
                            };
                            let key = prefix.clone().unwrap_or_default();
                            let val = t.node_get_content(at);
                            let ns = mapper_get_ns(t, d, prefix.as_deref(), Some(&val));
                            if !table.iter().any(|(k, _)| *k == key) {
                                table.push((key, ns));
                            }
                        }
                    }
                }
            }
            cur = t.nodes[c].parent;
        }
        let mut res = Vec::new();
        for (prefix, ns) in table.into_iter().rev() {
            let href = ns.map(|x| t.nss[x].href.clone()).filter(|h| !h.is_empty());
            if prefix.is_empty() && href.is_none() {
                continue;
            }
            res.push((prefix, href));
        }
        res
    });
    for (prefix, href) in entries {
        let obj = new_object(vm, b"Dom\\NamespaceInfo");
        {
            let mut b = obj.borrow_mut();
            b.set_property(
                b"prefix".to_vec(),
                if prefix.is_empty() {
                    Value::Null
                } else {
                    s(&prefix)
                },
            );
            b.set_property(b"namespaceURI".to_vec(), opt_s(href.as_deref()));
        }
        let el = node_obj(vm, node_);
        obj.borrow_mut().set_property(b"element".to_vec(), el);
        out.push(Value::Object(obj));
    }
}

fn m_get_in_scope_namespaces(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let n = this_node(vm, o)?;
    let mut arr = PhpArray::new();
    in_scope_namespace_info(vm, &mut arr, n);
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn m_get_descendant_namespaces(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let n = this_node(vm, o)?;
    let mut arr = PhpArray::new();
    in_scope_namespace_info(vm, &mut arr, n);
    let mut cur = tree::with(|t| t.nodes[n].children);
    while let Some(c) = cur {
        if tree::with(|t| t.nodes[c].t == NType::Element) {
            in_scope_namespace_info(vm, &mut arr, c);
        }
        cur = tree::with(|t| next_in_tree_order(t, c, Some(n)));
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

/// Dom\Element::rename / Dom\Attr::rename
fn m_rename(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let uri = a.opt_str(vm, 0, "namespaceURI")?;
    let qname = a.str(vm, 1, "qualifiedName")?;
    let n = this_node(vm, o)?;
    let (prefix, local) = match crate::dom::document::validate_and_extract(uri.as_deref(), &qname) {
        Ok(x) => x,
        Err(code) => {
            dom_err(vm, code);
            return Err(());
        }
    };
    let err = tree::with(|t| {
        if t.nodes[n].t == NType::Attribute {
            if let Some(p) = t.nodes[n].parent {
                let u = uri.as_deref().filter(|u| !u.is_empty());
                if let Some(e) = t.has_ns_prop(p, &local, u) {
                    if e != n {
                        return Some(
                            "An attribute with the given name in the given namespace already exists",
                        );
                    }
                }
            }
        } else {
            let cur_html = ns_is(t, t.nodes[n].ns, XHTML_NS);
            let will_html = uri.as_deref() == Some(XHTML_NS);
            if cur_html != will_html {
                return Some(if cur_html {
                    "It is not possible to move an element out of the HTML namespace because the HTML namespace is tied to the HTMLElement class"
                } else {
                    "It is not possible to move an element into the HTML namespace because the HTML namespace is tied to the HTMLElement class"
                });
            }
            if cur_html && t.nodes[n].name == "template" && local != "template" {
                return Some(
                    "It is not possible to rename the template element because it hosts a document fragment",
                );
            }
        }
        None
    });
    if let Some(m) = err {
        dom_err_msg(vm, INVALID_MODIFICATION_ERR, m);
        return Err(());
    }
    tree::with(|t| {
        let d = doc_of(t, n);
        t.nodes[n].ns = mapper_get_ns(t, d, prefix.as_deref(), uri.as_deref());
        t.nodes[n].name = local;
    });
    Ok(Value::Null)
}

pub fn element_methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("hasAttributes", m_has_attributes as MethodFn),
        ("getAttributeNames", m_get_attribute_names),
        ("getAttribute", m_get_attribute),
        ("getAttributeNS", m_get_attribute_ns),
        ("setAttribute", m_set_attribute),
        ("setAttributeNS", m_set_attribute_ns),
        ("removeAttribute", m_remove_attribute),
        ("removeAttributeNS", m_remove_attribute_ns),
        ("toggleAttribute", m_toggle_attribute),
        ("hasAttribute", m_has_attribute),
        ("hasAttributeNS", m_has_attribute_ns),
        ("getAttributeNode", m_get_attribute_node),
        ("getAttributeNodeNS", m_get_attribute_node_ns),
        ("setAttributeNode", m_set_attribute_node),
        ("setAttributeNodeNS", m_set_attribute_node_ns),
        ("removeAttributeNode", m_remove_attribute_node),
        ("getElementsByTagName", m_get_elements_by_tag_name),
        ("getElementsByTagNameNS", m_get_elements_by_tag_name_ns),
        (
            "getElementsByClassName",
            document::get_elements_by_class_name,
        ),
        ("insertAdjacentElement", m_insert_adjacent_element),
        ("insertAdjacentText", m_insert_adjacent_text),
        ("insertAdjacentHTML", m_insert_adjacent_html),
        (
            "setIdAttribute",
            legacy_method(Base::Element, "setIdAttribute"),
        ),
        (
            "setIdAttributeNS",
            legacy_method(Base::Element, "setIdAttributeNS"),
        ),
        ("setIdAttributeNode", m_set_id_attribute_node),
        ("closest", m_closest),
        ("matches", m_closest),
        ("getInScopeNamespaces", m_get_in_scope_namespaces),
        ("getDescendantNamespaces", m_get_descendant_namespaces),
        ("rename", m_rename),
    ]
}

fn m_is_id(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let n = this_node(vm, o)?;
    Ok(bv(tree::with(|t| t.nodes[n].atype == 2)))
}

pub fn attr_methods() -> Vec<(&'static str, MethodFn)> {
    vec![("isId", m_is_id as MethodFn), ("rename", m_rename)]
}

// ---------------------------------------------------------------------------
// Dom\import_simplexml

pub fn import_simplexml(vm: &mut Vm, a: &[Value]) -> Result<Value, goro_core::vm::VmError> {
    let f = "Dom\\import_simplexml";
    let v = a.first().map(|v| v.deref()).unwrap_or(Value::Undef);
    let o = match v {
        Value::Object(o) => o,
        Value::Undef => {
            throw_error(
                vm,
                b"ArgumentCountError",
                &format!("{}() expects exactly 1 argument, 0 given", f),
            );
            return Ok(Value::Null);
        }
        other => {
            throw_error(
                vm,
                b"TypeError",
                &format!(
                    "{}(): Argument #1 ($node) must be of type object, {} given",
                    f,
                    type_name_of(&other)
                ),
            );
            return Ok(Value::Null);
        }
    };
    let n = match crate::simplexml::sxe_of(&o) {
        Some(sx) => tree::with(|t| crate::simplexml::first_node(t, &sx, sx.node.get())),
        None => node_of(&o),
    };
    let ok = n
        .map(|n| {
            tree::with(|t| {
                t.nodes[n].doc.is_some()
                    && matches!(t.nodes[n].t, NType::Element | NType::Attribute)
            })
        })
        .unwrap_or(false);
    if !ok {
        throw_error(
            vm,
            b"TypeError",
            &format!("{}(): Argument #1 ($node) is not a valid node type", f),
        );
        return Ok(Value::Null);
    }
    let n = n.unwrap();
    let d = tree::with(|t| t.nodes[n].doc.unwrap());
    let old = doc_props(Some(d)).class_type;
    if old == 1 {
        throw_error(
            vm,
            b"TypeError",
            &format!(
                "{}(): Argument #1 ($node) must not be already imported as a DOMNode",
                f
            ),
        );
        return Ok(Value::Null);
    }
    if old != 2 {
        convert_to_modern(d);
    }
    Ok(node_obj(vm, n))
}

pub fn is_qname(s_: &str) -> bool {
    is_valid_qname(s_)
}
