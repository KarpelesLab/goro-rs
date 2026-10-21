//! DOMDocument (document.c)

use super::node::{args, b, cstr, opt_s, str_of};
use super::*;
use crate::parser::*;
use crate::serialize;

fn doc_node(vm: &mut Vm, o: &Obj) -> R<NodeId> {
    need_node(vm, o)
}

// ---------------------------------------------------------------------------
// Properties

fn r_doctype(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    let d = tree::with(|t| t.int_subset(n));
    Ok(node_obj_opt(vm, d))
}

fn r_implementation(vm: &mut Vm, _o: &Obj, _n: NodeId) -> R<Value> {
    let o = new_object(vm, b"DOMImplementation");
    attach(&o, Kind::Implementation);
    Ok(Value::Object(o))
}

fn r_document_element(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    let r = tree::with(|t| t.root_element(n));
    Ok(node_obj_opt(vm, r))
}

fn r_encoding(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| {
        opt_s(t.doc_info(n).and_then(|d| d.encoding.as_deref()))
    }))
}

fn r_actual_encoding(vm: &mut Vm, o: &Obj, n: NodeId) -> R<Value> {
    if !in_debug() {
        vm.emit_deprecated("Property DOMDocument::$actualEncoding is deprecated");
    }
    r_encoding(vm, o, n)
}

fn w_encoding(vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    match v {
        Value::Null => {
            throw_error(vm, b"ValueError", "Invalid document encoding");
            Err(())
        }
        v => {
            let e = str_of(v);
            if encoding_supported(&e) || e.eq_ignore_ascii_case("UTF-16") {
                tree::with(|t| {
                    if let Some(di) = t.doc_info_mut(n) {
                        di.encoding = Some(e);
                    }
                });
                Ok(())
            } else {
                throw_error(vm, b"ValueError", "Invalid document encoding");
                Err(())
            }
        }
    }
}

fn r_standalone(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(b(tree::with(|t| {
        t.doc_info(n).map(|d| d.standalone > 0).unwrap_or(false)
    })))
}

fn w_standalone(_vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let on = v.is_truthy();
    tree::with(|t| {
        if let Some(di) = t.doc_info_mut(n) {
            di.standalone = if on { 1 } else { 0 };
        }
    });
    Ok(())
}

fn r_version(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| {
        opt_s(t.doc_info(n).and_then(|d| d.version.as_deref()))
    }))
}

fn w_version(_vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let st = str_of(v);
    tree::with(|t| {
        if let Some(di) = t.doc_info_mut(n) {
            di.version = Some(st);
        }
    });
    Ok(())
}

macro_rules! docprop {
    ($r:ident, $w:ident, $field:ident) => {
        fn $r(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
            Ok(b(doc_props(Some(n)).$field))
        }
        fn $w(_vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
            let on = v.is_truthy();
            with_doc_props(n, |p| p.$field = on);
            Ok(())
        }
    };
}

docprop!(r_strict, w_strict, strict_error_checking);
docprop!(r_format_output, w_format_output, format_output);
docprop!(r_validate_on_parse, w_validate_on_parse, validate_on_parse);
docprop!(r_resolve_externals, w_resolve_externals, resolve_externals);
docprop!(r_preserve_ws, w_preserve_ws, preserve_whitespace);
docprop!(r_recover, w_recover, recover);
docprop!(r_substitute, w_substitute, substitute_entities);

fn r_document_uri(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| opt_s(t.doc_url(n).as_deref())))
}

fn w_document_uri(_vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let st = str_of(v);
    tree::with(|t| {
        if let Some(di) = t.doc_info_mut(n) {
            di.url = Some(st);
        }
    });
    Ok(())
}

fn r_config(vm: &mut Vm, _o: &Obj, _n: NodeId) -> R<Value> {
    if !in_debug() {
        vm.emit_deprecated("Property DOMDocument::$config is deprecated");
    }
    Ok(Value::Null)
}

pub fn document_props() -> Vec<Prop> {
    let mut v = vec![
        p("doctype", r_doctype, None, "mixed"),
        p("implementation", r_implementation, None, "mixed"),
        p("documentElement", r_document_element, None, "mixed"),
        p("actualEncoding", r_actual_encoding, None, "?string"),
        p("encoding", r_encoding, Some(w_encoding), "?string"),
        p("xmlEncoding", r_encoding, None, "?string"),
        p("standalone", r_standalone, Some(w_standalone), "bool"),
        p("xmlStandalone", r_standalone, Some(w_standalone), "bool"),
        p("version", r_version, Some(w_version), "?string"),
        p("xmlVersion", r_version, Some(w_version), "?string"),
        p("strictErrorChecking", r_strict, Some(w_strict), "bool"),
        p(
            "documentURI",
            r_document_uri,
            Some(w_document_uri),
            "?string",
        ),
        p("config", r_config, None, "mixed"),
        p(
            "formatOutput",
            r_format_output,
            Some(w_format_output),
            "bool",
        ),
        p(
            "validateOnParse",
            r_validate_on_parse,
            Some(w_validate_on_parse),
            "bool",
        ),
        p(
            "resolveExternals",
            r_resolve_externals,
            Some(w_resolve_externals),
            "bool",
        ),
        p(
            "preserveWhiteSpace",
            r_preserve_ws,
            Some(w_preserve_ws),
            "bool",
        ),
        p("recover", r_recover, Some(w_recover), "bool"),
        p(
            "substituteEntities",
            r_substitute,
            Some(w_substitute),
            "bool",
        ),
    ];
    v.extend(super::node::parent_props());
    v
}

// ---------------------------------------------------------------------------
// Constructor

pub fn construct(vm: &mut Vm, this: &Obj, a: &[Value]) -> R<()> {
    let a = args(a);
    a.count(vm, 0, 2)?;
    let version = a.str(vm, 0, "version")?;
    let encoding = a.str(vm, 1, "encoding")?;
    let version = if a.raw(0).is_none() {
        "1.0".to_string()
    } else {
        version
    };
    let d = tree::with(|t| {
        let d = t.new_doc(Some(&version));
        if !encoding.is_empty() {
            if let Some(di) = t.doc_info_mut(d) {
                di.encoding = Some(encoding.clone());
            }
        }
        d
    });
    // keep the doc props of a previously loaded document
    let old = node_of(this);
    let props = old.map(|o| doc_props(Some(o))).unwrap_or_default();
    set_doc_props(d, props);
    attach(this, Kind::Node(d));
    set_wrapper(d, this);
    Ok(())
}

// ---------------------------------------------------------------------------
// Methods

fn m_create_element(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 2)?;
    let name = a.str(vm, 0, "localName")?;
    let value = cstr(&a.str(vm, 1, "value")?).to_string();
    let d = doc_node(vm, o)?;
    if !is_valid_name(&name) {
        throw_dom(vm, INVALID_CHARACTER_ERR, strict(Some(d)));
        return Ok(Value::False);
    }
    let n = tree::with(|t| {
        let e = t.new_element(Some(d), &name);
        if a.raw(1).is_some() && !value.is_empty() {
            crate::simplexml::string_get_node_list(t, Some(d), e, &value);
        }
        e
    });
    Ok(node_obj(vm, n))
}

fn m_create_document_fragment(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let d = doc_node(vm, o)?;
    let n = tree::with(|t| t.new_fragment(Some(d)));
    Ok(node_obj(vm, n))
}

fn m_create_text_node(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let v = cstr(&a.str(vm, 0, "data")?).to_string();
    let d = doc_node(vm, o)?;
    let n = tree::with(|t| t.new_text(Some(d), &v));
    Ok(node_obj(vm, n))
}

fn m_create_comment(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let v = cstr(&a.str(vm, 0, "data")?).to_string();
    let d = doc_node(vm, o)?;
    let n = tree::with(|t| t.new_comment(Some(d), &v));
    Ok(node_obj(vm, n))
}

fn m_create_cdata_section(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let v = a.str(vm, 0, "data")?;
    let d = doc_node(vm, o)?;
    let n = tree::with(|t| t.new_cdata(Some(d), &v));
    Ok(node_obj(vm, n))
}

fn m_create_pi(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 2)?;
    let target = a.str(vm, 0, "target")?;
    let data = cstr(&a.str(vm, 1, "data")?).to_string();
    let d = doc_node(vm, o)?;
    if !is_valid_name(&target) {
        throw_dom(vm, INVALID_CHARACTER_ERR, strict(Some(d)));
        return Ok(Value::False);
    }
    let n = tree::with(|t| t.new_pi(Some(d), &target, &data));
    Ok(node_obj(vm, n))
}

fn m_create_attribute(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let name = a.str(vm, 0, "localName")?;
    if name.contains('\0') {
        throw_error(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #1 ($localName) must not contain any null bytes",
                a.f
            ),
        );
        return Err(());
    }
    let d = doc_node(vm, o)?;
    if !is_valid_name(&name) {
        throw_dom(vm, INVALID_CHARACTER_ERR, strict(Some(d)));
        return Ok(Value::False);
    }
    let n = tree::with(|t| t.new_attr_node(Some(d), None, &name, None));
    Ok(node_obj(vm, n))
}

fn m_create_entity_reference(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let name = a.str(vm, 0, "name")?;
    let d = doc_node(vm, o)?;
    if !is_valid_name(&name) {
        throw_dom(vm, INVALID_CHARACTER_ERR, strict(Some(d)));
        return Ok(Value::False);
    }
    let n = tree::with(|t| t.new_entity_ref(Some(d), &name));
    Ok(node_obj(vm, n))
}

/// dom_check_qname: Ok((prefix, local)) or Err(code)
pub fn check_qname(qname: &str, uri_len: usize) -> Result<(Option<String>, String), i64> {
    if qname.is_empty() {
        return Err(NAMESPACE_ERR);
    }
    let (prefix, local) = match qname.find(':') {
        Some(i) if i > 0 && i + 1 < qname.len() => {
            (Some(qname[..i].to_string()), qname[i + 1..].to_string())
        }
        _ => (None, qname.to_string()),
    };
    if prefix.is_none() && uri_len == 0 && !qname.contains(':') {
        return Ok((None, local));
    }
    if !is_valid_qname(qname) {
        return Err(NAMESPACE_ERR);
    }
    if prefix.is_some() && uri_len == 0 {
        return Err(NAMESPACE_ERR);
    }
    Ok((prefix, local))
}

/// dom_get_ns
pub fn get_ns(
    t: &mut Tree,
    node: NodeId,
    uri: &str,
    prefix: Option<&str>,
) -> Result<tree::NsId, i64> {
    let bad = (prefix == Some("xml") && uri != tree::XML_XML_NAMESPACE)
        || (prefix == Some("xmlns") && uri != XMLNS_NS)
        || (prefix.is_some() && uri == XMLNS_NS && prefix != Some("xmlns"));
    if bad {
        return Err(NAMESPACE_ERR);
    }
    get_ns_unchecked(t, node, uri, prefix).ok_or(NAMESPACE_ERR)
}

pub fn get_ns_unchecked(
    t: &mut Tree,
    node: NodeId,
    uri: &str,
    prefix: Option<&str>,
) -> Option<tree::NsId> {
    if let Some(ns) = t.new_ns(Some(node), uri, prefix) {
        return Some(ns);
    }
    resolve_prefix_conflict(t, node, uri)
}

/// dom_get_ns_resolve_prefix_conflict
pub fn resolve_prefix_conflict(t: &mut Tree, node: NodeId, uri: &str) -> Option<tree::NsId> {
    t.nodes[node].doc?;
    let mut prefix = "default".to_string();
    let mut counter = 1;
    while t.search_ns(node, Some(&prefix)).is_some() {
        if counter > 1000 {
            return None;
        }
        prefix = format!("default{}", counter);
        counter += 1;
    }
    t.new_ns(Some(node), uri, Some(&prefix))
}

fn m_create_element_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 3)?;
    let uri = a.opt_str(vm, 0, "namespace")?;
    let name = a.str(vm, 1, "qualifiedName")?;
    let value = a.str(vm, 2, "value")?;
    let d = doc_node(vm, o)?;
    let ulen = uri.as_deref().map(|u| u.len()).unwrap_or(0);
    let r = tree::with(|t| -> Result<NodeId, i64> {
        let (prefix, local) = check_qname(&name, ulen)?;
        if !is_valid_name(&local) {
            return Err(INVALID_CHARACTER_ERR);
        }
        let e = t.new_element(Some(d), &local);
        if !value.is_empty() {
            crate::simplexml::string_get_node_list(t, Some(d), e, &value);
        }
        if let Some(u) = &uri {
            let ns = match t.search_ns_by_href(e, u) {
                Some(n) => Some(n),
                None => Some(get_ns(t, e, u, prefix.as_deref())?),
            };
            t.nodes[e].ns = ns;
        }
        Ok(e)
    });
    match r {
        Ok(e) => Ok(node_obj(vm, e)),
        Err(code) => {
            throw_dom(vm, code, strict(Some(d)));
            Ok(Value::False)
        }
    }
}

/// dom_validate_and_extract (legacy createAttributeNS uses it)
pub fn validate_and_extract(
    uri: Option<&str>,
    qname: &str,
) -> Result<(Option<String>, String), i64> {
    let ns = uri.unwrap_or("");
    if !is_valid_qname(qname) {
        return Err(INVALID_CHARACTER_ERR);
    }
    let (prefix, local) = match qname.find(':') {
        Some(i) if i > 0 && i + 1 < qname.len() => {
            (Some(qname[..i].to_string()), qname[i + 1..].to_string())
        }
        _ => (None, qname.to_string()),
    };
    if prefix.is_some() && ns.is_empty() {
        return Err(NAMESPACE_ERR);
    }
    if prefix.as_deref() == Some("xml") && ns != tree::XML_XML_NAMESPACE {
        return Err(NAMESPACE_ERR);
    }
    if (qname == "xmlns" || prefix.as_deref() == Some("xmlns")) && ns != XMLNS_NS {
        return Err(NAMESPACE_ERR);
    }
    if ns == XMLNS_NS && qname != "xmlns" && prefix.as_deref() != Some("xmlns") {
        return Err(NAMESPACE_ERR);
    }
    Ok((prefix, local))
}

fn m_create_attribute_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let uri = a.opt_str(vm, 0, "namespace")?;
    let name = a.str(vm, 1, "qualifiedName")?;
    let d = doc_node(vm, o)?;
    let root = tree::with(|t| t.root_element(d));
    let root = match root {
        Some(r) => r,
        None => {
            warn(vm, "Document Missing Root Element");
            return Ok(Value::False);
        }
    };
    let r = tree::with(|t| -> Result<NodeId, i64> {
        let (prefix, local) =
            validate_and_extract(uri.as_deref(), &name).map_err(|_| NAMESPACE_ERR)?;
        let at = t.new_attr_node(Some(d), None, &local, None);
        if let Some(u) = uri.as_deref().filter(|u| !u.is_empty()) {
            let mut nsptr = t.search_ns_by_href(root, u);
            if name == "xmlns" || prefix.as_deref() == Some("xml") {
                if nsptr.is_none() {
                    nsptr = Some(t.new_ns_raw(u, prefix.as_deref(), None));
                }
            } else if nsptr.is_none() || t.nss[nsptr.unwrap()].prefix.is_none() {
                nsptr = get_ns_unchecked(t, root, u, Some(prefix.as_deref().unwrap_or("default")));
                if nsptr.is_none() {
                    return Err(NAMESPACE_ERR);
                }
            }
            t.nodes[at].ns = nsptr;
        }
        Ok(at)
    });
    match r {
        Ok(at) => Ok(node_obj(vm, at)),
        Err(code) => {
            throw_dom(vm, code, strict(Some(d)));
            Ok(Value::False)
        }
    }
}

fn m_get_element_by_id(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let id = a.str(vm, 0, "elementId")?;
    let d = doc_node(vm, o)?;
    let r = tree::with(|t| {
        let mut cur = t.root_element(d);
        while let Some(x) = cur {
            if t.nodes[x].t == NType::Element {
                for at in t.attrs_of(x) {
                    if t.nodes[at].atype == 2 && t.node_get_content(at) == id {
                        return Some(x);
                    }
                }
            }
            cur = super::node::next_in_tree(t, x, d);
        }
        None
    });
    Ok(node_obj_opt(vm, r))
}

fn m_get_elements_by_tag_name(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let name = a.path_str(vm, 0, "qualifiedName")?;
    let n = need_node(vm, o)?;
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
    let n = need_node(vm, o)?;
    Ok(list_obj(
        vm,
        ListKind::ByTag {
            base: o.clone(),
            ns: Some(ns.unwrap_or_default()),
            local: name,
        },
    ))
}

fn m_import_node(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 2)?;
    let no = a.obj_of(vm, 0, "node", "DOMNode", false)?.unwrap();
    let deep = a.bool(vm, 1, "deep", false)?;
    let d = doc_node(vm, o)?;
    let n = need_node(vm, &no)?;
    if tree::with(|t| t.is_doc(n)) {
        warn(vm, "Cannot import: Node Type Not Supported");
        return Ok(Value::False);
    }
    if tree::with(|t| t.nodes[n].doc) == Some(d) {
        return Ok(Value::Object(no));
    }
    let c = match super::node::clone_node(n, Some(d), deep) {
        Some(c) => c,
        None => return Ok(Value::False),
    };
    tree::with(|t| {
        if t.nodes[c].t == NType::Attribute {
            if let (Some(ons), None) = (t.nodes[n].ns, t.nodes[c].ns) {
                let href = t.nss[ons].href.clone();
                let prefix = t.nss[ons].prefix.clone();
                let root = t.root_element(d);
                let mut nsptr = root.and_then(|r| t.search_ns_by_href(r, &href));
                if nsptr.is_none() || t.nss[nsptr.unwrap()].prefix.is_none() {
                    nsptr = match root {
                        Some(r) => get_ns(t, r, &href, prefix.as_deref()).ok(),
                        None => Some(t.new_ns_raw(&href, prefix.as_deref(), None)),
                    };
                }
                t.nodes[c].ns = nsptr;
            }
        }
    });
    Ok(node_obj(vm, c))
}

fn m_adopt_node(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let no = a.obj_of(vm, 0, "node", "DOMNode", false)?.unwrap();
    let n = need_node(vm, &no)?;
    let bad = tree::with(|t| {
        matches!(
            t.nodes[n].t,
            NType::Document
                | NType::HtmlDocument
                | NType::DocumentType
                | NType::Dtd
                | NType::Entity
                | NType::Notation
        )
    });
    if bad {
        throw_dom(vm, NOT_SUPPORTED_ERR, strict(doc_of_node(n)));
        return Ok(Value::False);
    }
    let d = doc_node(vm, o)?;
    tree::with(|t| {
        let old = t.nodes[n].doc;
        if old != Some(d) {
            // keep the namespaces in scope available after unlinking
            let mut inscope: Vec<(Option<String>, String)> = Vec::new();
            if let Some(p) = t.nodes[n].parent {
                let mut cur = Some(p);
                while let Some(c) = cur {
                    if t.nodes[c].t == NType::Element {
                        for &ns in &t.nodes[c].ns_def {
                            let pr = t.nss[ns].prefix.clone();
                            if !inscope.iter().any(|(x, _)| *x == pr) {
                                inscope.push((pr, t.nss[ns].href.clone()));
                            }
                        }
                    }
                    cur = t.nodes[c].parent;
                }
            }
            t.unlink(n);
            t.set_tree_doc(n, Some(d));
            if t.nodes[n].t == NType::Element {
                // declare namespaces used but no longer in scope
                let mut nodes = vec![n];
                nodes.extend(
                    t.descendants(n)
                        .into_iter()
                        .filter(|x| t.nodes[*x].t == NType::Element),
                );
                for x in nodes {
                    let mut targets = vec![x];
                    targets.extend(t.attrs_of(x));
                    for y in targets {
                        if let Some(ns) = t.nodes[y].ns {
                            let pr = t.nss[ns].prefix.clone();
                            if pr.as_deref() == Some("xml") {
                                continue;
                            }
                            let visible = t.search_ns_ro(y, pr.as_deref()) == Some(ns)
                                || t.nodes[y].ns_def.contains(&ns);
                            if !visible {
                                let href = t.nss[ns].href.clone();
                                let found = t
                                    .search_ns(y, pr.as_deref())
                                    .filter(|f| t.nss[*f].href == href);
                                let newns = match found {
                                    Some(f) => f,
                                    None => match t.new_ns(Some(n), &href, pr.as_deref()) {
                                        Some(nn) => nn,
                                        None => ns,
                                    },
                                };
                                t.nodes[y].ns = Some(newns);
                            }
                        }
                    }
                }
            }
        } else {
            t.unlink(n);
        }
    });
    Ok(Value::Object(no))
}

fn m_normalize_document(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let d = doc_node(vm, o)?;
    tree::with(|t| super::node::normalize(t, d));
    Ok(Value::Null)
}

fn cwd_url() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let mut s = cwd.to_string_lossy().into_owned();
    if !s.ends_with('/') {
        s.push('/');
    }
    Some(s)
}

/// Resolve a load() path like dom_get_valid_file_path.
fn valid_file_path(source: &str) -> Option<String> {
    if source.len() >= 4096 {
        return None;
    }
    let lower = source.to_ascii_lowercase();
    let has_scheme = crate::uri::parse_uri(source)
        .map(|u| u.scheme.is_some())
        .unwrap_or(false)
        && !source.starts_with('/');
    let mut src = source.to_string();
    let mut is_file = false;
    if has_scheme {
        if lower.starts_with("file:///") {
            src = source[7..].to_string();
            is_file = true;
        } else if lower.starts_with("file://localhost/") {
            src = source[16..].to_string();
            is_file = true;
        }
    }
    if !has_scheme || is_file {
        let p = std::path::Path::new(&src);
        let abs = if p.is_absolute() {
            p.to_path_buf()
        } else {
            std::env::current_dir().ok()?.join(p)
        };
        return Some(match std::fs::canonicalize(&abs) {
            Ok(c) => c.to_string_lossy().into_owned(),
            Err(_) => abs.to_string_lossy().into_owned(),
        });
    }
    Some(src)
}

/// Parse options derived from document properties (dom_document_parser).
fn effective_options(d: Option<NodeId>, mut options: i64) -> i64 {
    let p = doc_props(d);
    if p.validate_on_parse {
        options |= XML_PARSE_DTDVALID;
    }
    if p.resolve_externals {
        options |= XML_PARSE_DTDATTR;
    }
    if p.substitute_entities {
        options |= XML_PARSE_NOENT;
    }
    if !p.preserve_whitespace {
        options |= XML_PARSE_NOBLANKS;
    }
    if p.recover {
        options |= XML_PARSE_RECOVER;
    }
    options
}

fn finish_loading(vm: &mut Vm, this: &Obj, newdoc: NodeId) {
    let old = node_of(this);
    let props = old.map(|o| doc_props(Some(o))).unwrap_or_default();
    set_doc_props(newdoc, props);
    if let Some(o) = old {
        tree::with(|t| t.nodes[o].wrapper = None);
    }
    attach(this, Kind::Node(newdoc));
    set_wrapper(newdoc, this);
    let _ = vm;
}

fn load_common(vm: &mut Vm, o: &Obj, a: &[Value], file: bool) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 2)?;
    let argname = if file { "filename" } else { "source" };
    let source = a.str_bytes(vm, 0, argname)?;
    let options = a.long(vm, 1, "options", 0)?;
    if source.is_empty() {
        throw_error(
            vm,
            b"ValueError",
            &format!("{}(): Argument #1 (${}) must not be empty", a.f, argname),
        );
        return Err(());
    }
    if options > i32::MAX as i64 || options < i32::MIN as i64 {
        warn(vm, "Invalid options");
        return Ok(Value::False);
    }
    let cur = node_of(o);
    let opts = effective_options(cur, options);
    let recover = opts & XML_PARSE_RECOVER != 0;
    let fname = a.f.clone();
    let (data, url) = if file {
        if source.contains(&0) {
            throw_error(
                vm,
                b"ValueError",
                &format!(
                    "{}(): Argument #1 ($filename) must not contain any null bytes",
                    a.f
                ),
            );
            return Err(());
        }
        let src = String::from_utf8_lossy(&source).into_owned();
        let path = match valid_file_path(&src) {
            Some(p) => p,
            None => return Ok(Value::False),
        };
        match crate::io::read_resource(vm, &path) {
            Some(d) => (d, Some(path)),
            None if crate::io::last_failure_reported() => return Ok(Value::False),
            None => {
                let msg = format!(
                    "I/O warning : failed to load \"{}\": No such file or directory",
                    path
                );
                let mut e = XmlError::simple(
                    FROM_IO,
                    1549,
                    ERR_WARNING,
                    format!("failed to load \"{}\": No such file or directory\n", path),
                );
                e.formatted = vec![msg];
                e.file = None;
                crate::libxml::report(vm, &fname, &[e], crate::libxml::Style::Generic);
                return Ok(Value::False);
            }
        }
    } else {
        (source, None)
    };
    let saved_er = vm.error_reporting;
    if recover {
        vm.error_reporting |= 2;
    }
    let out = crate::sax2::parse_doc(&data, url.as_deref(), opts);
    crate::libxml::report(vm, &fname, &out.errors, crate::libxml::Style::Ctx);
    if recover {
        vm.error_reporting = saved_er;
    }
    let doc = if out.well_formed || recover {
        out.doc
    } else {
        None
    };
    match doc {
        Some(d) => {
            tree::with(|t| {
                if let Some(di) = t.doc_info_mut(d) {
                    if di.url.is_none() {
                        di.url = cwd_url();
                    }
                }
            });
            if opts & XML_PARSE_DTDVALID != 0 {
                validate_doc(vm, d, &fname);
            }
            finish_loading(vm, o, d);
            Ok(Value::True)
        }
        None => Ok(Value::False),
    }
}

fn m_load(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    load_common(vm, o, a, true)
}

fn m_load_xml(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    load_common(vm, o, a, false)
}

fn html_load(vm: &mut Vm, o: &Obj, a: &[Value], file: bool) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 2)?;
    let argname = if file { "filename" } else { "source" };
    let source = a.str_bytes(vm, 0, argname)?;
    let options = a.long(vm, 1, "options", 0)?;
    if source.is_empty() {
        throw_error(
            vm,
            b"ValueError",
            &format!("{}(): Argument #1 (${}) must not be empty", a.f, argname),
        );
        return Err(());
    }
    if options > i32::MAX as i64 || options < i32::MIN as i64 {
        warn(vm, "Invalid options");
        return Ok(Value::False);
    }
    let fname = a.f.clone();
    let (data, url) = if file {
        if source.contains(&0) {
            throw_error(
                vm,
                b"ValueError",
                &format!(
                    "{}(): Argument #1 ($filename) must not contain any null bytes",
                    a.f
                ),
            );
            return Err(());
        }
        let src = String::from_utf8_lossy(&source).into_owned();
        match crate::io::read_resource(vm, &src) {
            Some(d) => (d, Some(src)),
            None if crate::io::last_failure_reported() => return Ok(Value::False),
            None => {
                let msg = format!(
                    "I/O warning : failed to load \"{}\": No such file or directory",
                    src
                );
                let mut e = XmlError::simple(
                    FROM_IO,
                    1549,
                    ERR_WARNING,
                    format!("failed to load \"{}\": No such file or directory\n", src),
                );
                e.formatted = vec![msg];
                crate::libxml::report(vm, &fname, &[e], crate::libxml::Style::Generic);
                return Ok(Value::False);
            }
        }
    } else {
        (source, None)
    };
    let out = crate::html::parse_html(&data, url.as_deref(), options, None);
    crate::libxml::report_ex(vm, &fname, &out.errors, crate::libxml::Style::Ctx, &|e| {
        crate::html::error_is_silenced(e, options)
    });
    match out.doc {
        Some(d) => {
            finish_loading(vm, o, d);
            Ok(Value::True)
        }
        None => Ok(Value::False),
    }
}

fn m_load_html(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    html_load(vm, o, a, false)
}

fn m_load_html_file(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    html_load(vm, o, a, true)
}

fn m_save_xml(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 0, 2)?;
    let node = a.obj_of(vm, 0, "node", "DOMNode", true)?;
    let options = a.long(vm, 1, "options", 0)?;
    let d = doc_node(vm, o)?;
    let format = doc_props(Some(d)).format_output;
    let no_empty = options & 4 != 0;
    let bytes = match node {
        Some(no) => {
            let n = need_node(vm, &no)?;
            if tree::with(|t| t.nodes[n].doc) != Some(d) {
                throw_dom(vm, WRONG_DOCUMENT_ERR, strict(Some(d)));
                return Ok(Value::False);
            }
            tree::with(|t| {
                let enc = t.doc_info(d).and_then(|i| i.encoding.clone());
                serialize::node_to_bytes(t, n, enc.as_deref(), format, no_empty)
            })
        }
        None => {
            tree::with(|t| serialize::doc_to_bytes(t, d, None, format, options & 2 != 0, no_empty))
        }
    };
    Ok(Value::String(PhpString::from_vec(bytes)))
}

fn m_save(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 2)?;
    let file = a.str(vm, 0, "filename")?;
    let options = a.long(vm, 1, "options", 0)?;
    if file.is_empty() {
        throw_error(
            vm,
            b"ValueError",
            &format!("{}(): Argument #1 ($filename) must not be empty", a.f),
        );
        return Err(());
    }
    if file.contains('\0') {
        throw_error(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #1 ($filename) must not contain any null bytes",
                a.f
            ),
        );
        return Err(());
    }
    let d = doc_node(vm, o)?;
    let format = doc_props(Some(d)).format_output;
    let bytes =
        tree::with(|t| serialize::doc_to_bytes(t, d, None, format, false, options & 4 != 0));
    let n = crate::io::write_resource_len(vm, &file, &bytes);
    if n < 0 {
        return Ok(Value::False);
    }
    Ok(Value::Long(n))
}

fn m_save_html(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 0, 1)?;
    let node = a.obj_of(vm, 0, "node", "DOMNode", true)?;
    let d = doc_node(vm, o)?;
    let format = doc_props(Some(d)).format_output;
    match node {
        Some(no) => {
            let n = need_node(vm, &no)?;
            if tree::with(|t| t.nodes[n].doc) != Some(d) {
                throw_dom(vm, WRONG_DOCUMENT_ERR, strict(Some(d)));
                return Ok(Value::False);
            }
            let bytes = tree::with(|t| crate::html::html_node_to_bytes(t, n, None, format));
            Ok(Value::String(PhpString::from_vec(bytes)))
        }
        None => {
            let bytes = tree::with(|t| crate::html::html_doc_to_bytes(t, d, None, format));
            if bytes.is_empty() {
                return Ok(Value::False);
            }
            Ok(Value::String(PhpString::from_vec(bytes)))
        }
    }
}

fn m_save_html_file(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let file = a.str(vm, 0, "filename")?;
    if file.is_empty() {
        throw_error(
            vm,
            b"ValueError",
            &format!("{}(): Argument #1 ($filename) must not be empty", a.f),
        );
        return Err(());
    }
    let d = doc_node(vm, o)?;
    let format = doc_props(Some(d)).format_output;
    let bytes = tree::with(|t| crate::html::html_save_file_bytes(t, d, format));
    let n = crate::io::write_resource_len(vm, &file, &bytes);
    if n < 0 {
        return Ok(Value::False);
    }
    Ok(Value::Long(n))
}

fn m_register_node_class(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let base = a.str(vm, 0, "baseClass")?;
    let ext = a.opt_str(vm, 1, "extendedClass")?;
    let bl = base.trim_start_matches('\\').to_ascii_lowercase();
    let base_known = vm.classes.contains_key(bl.as_bytes());
    if !base_known
        || base_of_class(vm, bl.as_bytes()).is_none()
        || !class_is(vm, bl.as_bytes(), "DOMNode")
    {
        let msg = format!(
            "{}(): Argument #1 ($baseClass) must be a valid class name, {} given",
            a.f, base
        );
        if !base_known {
            throw_error(
                vm,
                b"TypeError",
                &format!(
                    "{}(): Argument #1 ($baseClass) must be a valid class name, {} given",
                    a.f, base
                ),
            );
            return Err(());
        }
        let _ = msg;
    }
    let base_canon = vm
        .classes
        .get(bl.as_bytes())
        .map(|c| String::from_utf8_lossy(&c.name).into_owned())
        .unwrap_or(base.clone());
    let d = doc_node(vm, o)?;
    match ext {
        None => {
            with_doc_props(d, |p| {
                p.classmap.remove(&bl);
            });
            Ok(Value::True)
        }
        Some(e) => {
            let el = e.trim_start_matches('\\').to_ascii_lowercase();
            if !vm.classes.contains_key(el.as_bytes()) {
                throw_error(
                    vm,
                    b"TypeError",
                    &format!(
                        "{}(): Argument #2 ($extendedClass) must be a valid class name or null, {} given",
                        a.f, e
                    ),
                );
                return Err(());
            }
            if !class_is(vm, el.as_bytes(), &bl) {
                let ecanon = vm
                    .classes
                    .get(el.as_bytes())
                    .map(|c| String::from_utf8_lossy(&c.name).into_owned())
                    .unwrap_or(e.clone());
                throw_error(
                    vm,
                    b"ValueError",
                    &format!(
                        "{}(): Argument #2 ($extendedClass) must be a class name derived from {} or null, {} given",
                        a.f, base_canon, ecanon
                    ),
                );
                return Err(());
            }
            if vm
                .classes
                .get(el.as_bytes())
                .map(|c| c.is_abstract)
                .unwrap_or(false)
            {
                throw_error(
                    vm,
                    b"ValueError",
                    &format!(
                        "{}(): Argument #2 ($extendedClass) must not be an abstract class",
                        a.f
                    ),
                );
                return Err(());
            }
            let ecanon = vm
                .classes
                .get(el.as_bytes())
                .map(|c| c.name.clone())
                .unwrap_or_else(|| e.clone().into_bytes());
            with_doc_props(d, |p| {
                p.classmap.insert(bl.clone(), ecanon);
            });
            Ok(Value::True)
        }
    }
}

/// DTD validation (DOMDocument::validate)
pub fn validate_doc(vm: &mut Vm, d: NodeId, fname: &str) -> bool {
    let errors = crate::valid::validate_document(d);
    let ok = errors.iter().all(|e| e.level < ERR_ERROR);
    crate::libxml::report(vm, fname, &errors, crate::libxml::Style::Ctx);
    ok
}

fn m_validate(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let d = doc_node(vm, o)?;
    let f = cur_method();
    Ok(b(validate_doc(vm, d, &f)))
}

fn m_xinclude(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 0, 1)?;
    let flags = a.long(vm, 0, "options", 0)?;
    if flags > i32::MAX as i64 || flags < 0 && flags < i32::MIN as i64 {
        throw_error(
            vm,
            b"ValueError",
            &format!("{}(): Argument #1 ($options) is too large", a.f),
        );
        return Err(());
    }
    let d = doc_node(vm, o)?;
    let n = crate::xinclude::process(vm, d, flags);
    if n < 0 {
        Ok(Value::False)
    } else {
        Ok(Value::Long(n))
    }
}

fn m_schema_validate(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 2)?;
    let _src = a.str(vm, 0, "filename")?;
    let _d = doc_node(vm, o)?;
    warn(vm, "Invalid Schema");
    Ok(Value::False)
}

fn m_relaxng_validate(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let _src = a.str(vm, 0, "filename")?;
    let _d = doc_node(vm, o)?;
    warn(vm, "Invalid RelaxNG");
    Ok(Value::False)
}

pub fn c14n(vm: &mut Vm, o: &Obj, a: &[Value], file: bool) -> R<Value> {
    let a = args(a);
    let off = if file { 1 } else { 0 };
    a.count(vm, off, 4 + off)?;
    let uri = if file {
        a.str(vm, 0, "uri")?
    } else {
        String::new()
    };
    let exclusive = a.bool(vm, off, "exclusive", false)?;
    let with_comments = a.bool(vm, off + 1, "withComments", false)?;
    let xpath = a.raw(off + 2);
    let prefixes = a.raw(off + 3);
    let n = need_node(vm, o)?;
    let doc = doc_of_node(n);
    if doc.is_none() {
        throw_error(vm, b"ValueError", "Node must be associated with a document");
        return Err(());
    }
    let r = crate::c14n::canonicalize(
        vm,
        n,
        exclusive,
        with_comments,
        xpath.as_ref(),
        prefixes.as_ref(),
    )?;
    match r {
        None => Ok(Value::False),
        Some(bytes) => {
            if file {
                let n = crate::io::write_resource_len(vm, &uri, &bytes);
                if n < 0 {
                    Ok(Value::False)
                } else {
                    Ok(Value::Long(n))
                }
            } else {
                Ok(Value::String(PhpString::from_vec(bytes)))
            }
        }
    }
}

pub fn document_methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("createAttribute", m_create_attribute),
        ("createAttributeNS", m_create_attribute_ns),
        ("createCDATASection", m_create_cdata_section),
        ("createComment", m_create_comment),
        ("createDocumentFragment", m_create_document_fragment),
        ("createElement", m_create_element),
        ("createElementNS", m_create_element_ns),
        ("createEntityReference", m_create_entity_reference),
        ("createProcessingInstruction", m_create_pi),
        ("createTextNode", m_create_text_node),
        ("getElementById", m_get_element_by_id),
        ("getElementsByTagName", m_get_elements_by_tag_name),
        ("getElementsByTagNameNS", m_get_elements_by_tag_name_ns),
        ("importNode", m_import_node),
        ("load", m_load),
        ("loadXML", m_load_xml),
        ("normalizeDocument", m_normalize_document),
        ("registerNodeClass", m_register_node_class),
        ("save", m_save),
        ("loadHTML", m_load_html),
        ("loadHTMLFile", m_load_html_file),
        ("saveHTML", m_save_html),
        ("saveHTMLFile", m_save_html_file),
        ("saveXML", m_save_xml),
        ("schemaValidate", m_schema_validate),
        ("schemaValidateSource", m_schema_validate),
        ("relaxNGValidate", m_relaxng_validate),
        ("relaxNGValidateSource", m_relaxng_validate),
        ("validate", m_validate),
        ("xinclude", m_xinclude),
        ("adoptNode", m_adopt_node),
    ]
}
