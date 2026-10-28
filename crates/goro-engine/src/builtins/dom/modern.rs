//! The `Dom\` (PHP 8.4) extras: innerHTML/outerHTML, CSS selectors
//! (querySelector, matches, closest), getElementsByClassName, the HTML
//! document accessors (body, head, title), Dom\TokenList (classList) and
//! namespace info (RFC "DOM HTML5 parsing and serialization"; WHATWG DOM
//! and HTML; observed on PHP 8.5).

use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};
use goro_xml::tree::{DocKind, DomError, HTML_NS, NodeData, QName};
use goro_xml::{Arena, NodeId};

use super::lists::ListKind;
use super::xmlstore;
use super::{
    arg_node, modern_fn, s8, sv, sv_opt, this_node, throw_dom, throw_dom_msg, with_arena,
    wrap_node, wrap_opt,
};
use crate::builtins::spl::ExtIterOp;
use crate::func::{Args, NativeFn};
use crate::vm::{R, Vm};

pub const METHODS: &[(&str, NativeFn)] = &[
    ("dom\\element::queryselector", query_selector),
    ("dom\\element::queryselectorall", query_selector_all),
    ("dom\\element::closest", closest),
    ("dom\\element::matches", matches),
    ("dom\\element::insertadjacenthtml", insert_adjacent_html),
    ("dom\\element::getinscopenamespaces", get_in_scope_namespaces),
    ("dom\\element::getdescendantnamespaces", get_descendant_namespaces),
    ("dom\\tokenlist::item", tl_item),
    ("dom\\tokenlist::contains", tl_contains),
    ("dom\\tokenlist::add", tl_add),
    ("dom\\tokenlist::remove", tl_remove),
    ("dom\\tokenlist::toggle", tl_toggle),
    ("dom\\tokenlist::replace", tl_replace),
    ("dom\\tokenlist::supports", tl_supports),
    ("dom\\tokenlist::count", tl_count),
    ("dom\\tokenlist::getiterator", tl_iterator),
];

fn is_html_doc(a: &Arena, n: NodeId) -> bool {
    a.doc(a.doc_of(n)).kind == DocKind::Html5
}

fn is_html_el(a: &Arena, n: NodeId, name: &str) -> bool {
    a.element(n)
        .is_some_and(|e| e.name.ns.as_deref() == Some(HTML_NS) && &*e.name.local == name)
}

// ----------------------------------------------------- inner/outer HTML

/// Serializes the children of `n` (HTML or XML rules by document).
pub(crate) fn inner_html(a: &Arena, n: NodeId) -> Result<String, ()> {
    if is_html_doc(a, n) {
        let target = a
            .element(n)
            .and_then(|e| e.template_contents)
            .unwrap_or(n);
        return Ok(goro_xml::html5::serialize::serialize_children(a, target));
    }
    goro_xml::modern::children_to_string(a, n, well_formed()).map_err(|_| ())
}

/// XML serialization requiring well-formedness (DOM Parsing: innerHTML).
fn well_formed() -> goro_xml::modern::ModernOptions {
    goro_xml::modern::ModernOptions {
        require_well_formed: true,
        ..Default::default()
    }
}

/// Serializes `n` itself.
pub(crate) fn outer_html(a: &Arena, n: NodeId) -> Result<String, ()> {
    if is_html_doc(a, n) {
        return Ok(goro_xml::html5::serialize::serialize_node(a, n));
    }
    goro_xml::modern::to_string(a, n, well_formed()).map_err(|_| ())
}

/// Parses `text` as a fragment in the context of `ctx` (HTML fragment
/// parsing for HTML documents, XML for XML ones): a detached fragment.
fn parse_in_context(vm: &mut Vm, ctx: NodeId, text: &str) -> R<NodeId> {
    let html = with_arena(|a| is_html_doc(a, ctx));
    if html {
        let opts = goro_xml::html5::FragmentOptions::default();
        let f = with_arena(|a| goro_xml::html5::parse_fragment(a, ctx, text, &opts));
        return Ok(f.fragment);
    }
    let r = with_arena(|a| {
        let doc = a.doc_of(ctx);
        let f = a.create(doc, NodeData::Fragment);
        // In-scope namespaces of the context apply, the document's DTD
        // does not (test modern/xml/Element_innerHTML_writing_errors).
        let tmp = a.new_document(DocKind::Xml);
        a.doc_mut(tmp).modern = true;
        let holder = a.clone_node(ctx, tmp, false);
        if a.is_element(ctx) {
            let scope = in_scope(a, ctx);
            if let Some(e) = a.element_mut(holder) {
                e.ns_decls = scope
                    .into_iter()
                    .map(|(p, u)| goro_xml::tree::NsDecl {
                        prefix: p.as_deref().map(Into::into),
                        uri: u.as_str().into(),
                    })
                    .collect();
            }
        }
        let opts = goro_xml::xml::ParseOptions {
            modern: true,
            ..Default::default()
        };
        let (_, _errs, ok) = goro_xml::xml::parse_fragment(a, holder, text.as_bytes(), &opts);
        if ok {
            let kids: Vec<NodeId> = a.children(holder).collect();
            for k in kids {
                a.detach(k);
                a.adopt(k, doc);
                a.raw_append(f, k);
            }
        }
        a.free_subtree(holder);
        a.free_document_if_unheld(tmp);
        if ok { Ok(f) } else {
            a.free_subtree(f);
            Err(())
        }
    });
    match r {
        Ok(f) => Ok(f),
        Err(()) => throw_dom_msg(vm, "XML fragment is not well-formed", 12),
    }
}

pub(crate) fn set_inner_html(vm: &mut Vm, n: NodeId, text: &str) -> R<()> {
    let f = parse_in_context(vm, n, text)?;
    xmlstore::with(|s| {
        let target = s
            .arena
            .element(n)
            .and_then(|e| e.template_contents)
            .unwrap_or(n);
        let kids: Vec<NodeId> = s.arena.children(target).collect();
        for k in kids {
            s.arena.detach(k);
            s.free_if_unheld(k);
        }
        let new: Vec<NodeId> = s.arena.children(f).collect();
        for k in new {
            s.arena.detach(k);
            s.arena.raw_append(target, k);
        }
        s.arena.free_subtree(f);
    });
    Ok(())
}

pub(crate) fn set_outer_html(vm: &mut Vm, n: NodeId, text: &str) -> R<()> {
    let Some(parent) = with_arena(|a| a.parent(n)) else {
        return Ok(());
    };
    if with_arena(|a| matches!(a.data(parent), NodeData::Document(_))) {
        return throw_dom_msg(vm, "Invalid Modification Error", 13);
    }
    let ctx = if with_arena(|a| matches!(a.data(parent), NodeData::Fragment)) {
        // A body element in the HTML namespace as context.
        with_arena(|a| {
            let d = a.doc_of(n);
            a.new_element(
                d,
                QName {
                    prefix: None,
                    local: "body".into(),
                    ns: Some(HTML_NS.into()),
                },
            )
        })
    } else {
        parent
    };
    let f = parse_in_context(vm, ctx, text)?;
    xmlstore::with(|s| {
        let next = s.arena.next_sibling(n);
        s.arena.detach(n);
        let new: Vec<NodeId> = s.arena.children(f).collect();
        for k in new {
            s.arena.detach(k);
            s.arena.raw_insert_before(parent, k, next);
        }
        s.arena.free_subtree(f);
        s.free_if_unheld(n);
        if ctx != parent {
            s.free_if_unheld(ctx);
        }
    });
    Ok(())
}

fn insert_adjacent_html(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let w = match a.val(0) {
        Value::Object(o) => s8(&vm.get_prop_raw(&o, b"value").as_str().cloned().unwrap_or_default()),
        v => s8(&vm.to_php_str(&v)?),
    };
    let text = s8(&a.str(1));
    let (ctx, parent, before) = with_arena(|ar| match w.as_str() {
        "beforebegin" => (ar.parent(el), ar.parent(el), Some(el)),
        "afterend" => (ar.parent(el), ar.parent(el), ar.next_sibling(el)),
        "afterbegin" => (Some(el), Some(el), ar.first_child(el)),
        _ => (Some(el), Some(el), None),
    });
    let (Some(ctx), Some(parent)) = (ctx, parent) else {
        return throw_dom_msg(vm, "No Modification Allowed Error", 7);
    };
    if with_arena(|ar| matches!(ar.data(parent), NodeData::Document(_))) {
        return throw_dom_msg(vm, "No Modification Allowed Error", 7);
    }
    let f = parse_in_context(vm, ctx, &text)?;
    with_arena(|ar| {
        let new: Vec<NodeId> = ar.children(f).collect();
        for k in new {
            ar.detach(k);
            ar.raw_insert_before(parent, k, before);
        }
        ar.free_subtree(f);
    });
    Ok(Value::Null)
}

// ------------------------------------------------- document accessors

fn html_root(a: &Arena, d: NodeId) -> Option<NodeId> {
    a.document_element(d).filter(|&e| is_html_el(a, e, "html"))
}

pub(crate) fn body(a: &Arena, d: NodeId) -> Option<NodeId> {
    if !matches!(a.data(d), NodeData::Document(_)) {
        return None;
    }
    let html = html_root(a, d)?;
    a.children(html)
        .find(|&c| is_html_el(a, c, "body") || is_html_el(a, c, "frameset"))
}

pub(crate) fn head(a: &Arena, d: NodeId) -> Option<NodeId> {
    if !matches!(a.data(d), NodeData::Document(_)) {
        return None;
    }
    let html = html_root(a, d)?;
    a.children(html).find(|&c| is_html_el(a, c, "head"))
}

fn title_element(a: &Arena, d: NodeId) -> Option<NodeId> {
    let root = a.document_element(d)?;
    if a.element(root).is_some_and(|e| {
        e.name.ns.as_deref() == Some(goro_xml::tree::SVG_NS) && &*e.name.local == "svg"
    }) {
        return a.children(root).find(|&c| {
            a.element(c).is_some_and(|e| {
                e.name.ns.as_deref() == Some(goro_xml::tree::SVG_NS) && &*e.name.local == "title"
            })
        });
    }
    a.descendants(d).find(|&c| is_html_el(a, c, "title"))
}

/// `document.title` (WHATWG HTML: strip and collapse ASCII whitespace).
pub(crate) fn title(a: &Arena, d: NodeId) -> String {
    if !matches!(a.data(d), NodeData::Document(_)) {
        return String::new();
    }
    let Some(t) = title_element(a, d) else {
        return String::new();
    };
    let mut s = String::new();
    for c in a.children(t) {
        if let NodeData::Text(x) | NodeData::CData(x) = a.data(c) {
            s.push_str(x);
        }
    }
    s.split(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c'))
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn set_title(d: NodeId, t: &str) {
    with_arena(|a| {
        let Some(root) = a.document_element(d) else {
            return;
        };
        let svg = a.element(root).is_some_and(|e| {
            e.name.ns.as_deref() == Some(goro_xml::tree::SVG_NS) && &*e.name.local == "svg"
        });
        let el = match title_element(a, d) {
            Some(x) => x,
            None => {
                if svg {
                    let x = a.new_element(
                        d,
                        QName {
                            prefix: None,
                            local: "title".into(),
                            ns: Some(goro_xml::tree::SVG_NS.into()),
                        },
                    );
                    let first = a.first_child(root);
                    a.raw_insert_before(root, x, first);
                    x
                } else if is_html_el(a, root, "html") {
                    let Some(h) = head(a, d) else {
                        return;
                    };
                    let x = a.new_element(
                        d,
                        QName {
                            prefix: None,
                            local: "title".into(),
                            ns: Some(HTML_NS.into()),
                        },
                    );
                    a.raw_append(h, x);
                    x
                } else {
                    return;
                }
            }
        };
        a.set_text_content(el, t);
        if t.is_empty() {
            // An empty text node stays (observed on PHP 8.5, test
            // Document_title_setter.phpt).
            let tx = a.new_text(d, "");
            a.raw_append(el, tx);
        }
    });
}

pub(crate) fn set_body(vm: &mut Vm, d: NodeId, v: Value) -> R<()> {
    let new = match &v {
        Value::Object(o) => super::node_of(o),
        _ => None,
    };
    let Some(new) = new else {
        return throw_dom_msg(vm, "The new body must either be a body or a frameset tag", 3);
    };
    let ok = with_arena(|a| is_html_el(a, new, "body") || is_html_el(a, new, "frameset"));
    if !ok {
        return throw_dom_msg(vm, "The new body must either be a body or a frameset tag", 3);
    }
    let cur = with_arena(|a| body(a, d));
    if cur == Some(new) {
        return Ok(());
    }
    if with_arena(|a| a.document_element(d).is_none()) {
        return throw_dom_msg(vm, "A body can only be set if there is a document element", 3);
    }
    // Only documents with an html root get a body (DOM Standard note).
    if with_arena(|a| html_root(a, d).is_none()) {
        return Ok(());
    }
    let r = match cur {
        Some(old) => {
            let p = with_arena(|a| a.parent(old)).unwrap();
            with_arena(|a| a.replace_child(p, new, old)).map(|_| ())
        }
        None => match with_arena(|a| a.document_element(d)) {
            Some(root) => super::node::modern_insert(root, new, None).map(|_| ()),
            None => Err(DomError::HierarchyRequest),
        },
    };
    match r {
        Ok(()) => Ok(()),
        Err(_) => throw_dom_msg(vm, "A body can only be set if there is a document element", 3),
    }
}

// --------------------------------------------------- element queries

/// `getElementsByTagName()` for `Dom\` documents: HTML elements match
/// the name in lower case (DOM Standard "list of elements with qualified
/// name").
pub(crate) fn elements_by_qname(a: &Arena, root: NodeId, name: &str) -> Vec<NodeId> {
    let html = is_html_doc(a, root);
    let lower = name.to_ascii_lowercase();
    a.descendants(root)
        .filter(|&n| match a.element(n) {
            Some(e) => {
                if name == "*" {
                    return true;
                }
                if html && e.name.ns.as_deref() == Some(HTML_NS) {
                    e.name.is(&lower)
                } else {
                    e.name.is(name)
                }
            }
            None => false,
        })
        .collect()
}

fn split_ws(s: &str) -> Vec<String> {
    s.split(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c'))
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

/// `getElementsByClassName()`: elements having all the classes (ASCII
/// case-insensitive in quirks mode).
pub(crate) fn elements_by_class(a: &Arena, root: NodeId, classes: &[String]) -> Vec<NodeId> {
    if classes.is_empty() {
        return Vec::new();
    }
    let doc = a.doc_of(root);
    let quirks = a.doc(doc).quirks == goro_xml::tree::QuirksMode::Quirks;
    a.descendants(root)
        .filter(|&n| {
            let Some(v) = a.element(n).and(a.get_attr_node_ns(n, None, "class")) else {
                return false;
            };
            let have = split_ws(&a.attr_value(v));
            classes.iter().all(|c| {
                have.iter().any(|h| {
                    if quirks {
                        h.eq_ignore_ascii_case(c)
                    } else {
                        h == c
                    }
                })
            })
        })
        .collect()
}

pub(crate) fn get_elements_by_class_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let classes = split_ws(&s8(&a.str(0)));
    super::lists::new_list(vm, ListKind::ClassName(n, classes), true)
}

fn parse_selectors(vm: &mut Vm, s: &str) -> R<goro_xml::css::SelectorList> {
    match goro_xml::css::parse(s) {
        Ok(l) => Ok(l),
        Err(e) => throw_dom_msg(vm, &e.exception_message(), 12),
    }
}

pub(crate) fn query_selector(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let sel = parse_selectors(vm, &s8(&a.str(0)))?;
    let r = with_arena(|ar| goro_xml::css::query_selector(ar, n, &sel));
    wrap_opt(vm, r)
}

pub(crate) fn query_selector_all(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let sel = parse_selectors(vm, &s8(&a.str(0)))?;
    let r = with_arena(|ar| goro_xml::css::query_selector_all(ar, n, &sel));
    super::lists::new_list(vm, ListKind::Static(r), true)
}

fn closest(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let sel = parse_selectors(vm, &s8(&a.str(0)))?;
    let r = with_arena(|ar| goro_xml::css::closest(ar, n, &sel));
    wrap_opt(vm, r)
}

fn matches(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let sel = parse_selectors(vm, &s8(&a.str(0)))?;
    Ok(Value::Bool(with_arena(|ar| sel.matches(ar, n))))
}

// ------------------------------------------------------ namespace info

fn namespace_info(vm: &mut Vm, prefix: Option<String>, uri: Option<String>, el: NodeId) -> R<Value> {
    let cls = vm.find_class(b"Dom\\NamespaceInfo", false)?.unwrap();
    let o = vm.instantiate_raw(&cls)?;
    vm.set_prop_raw(&o, b"prefix", sv_opt(prefix.as_deref()));
    vm.set_prop_raw(&o, b"namespaceURI", sv_opt(uri.as_deref()));
    let e = wrap_node(vm, el)?;
    vm.set_prop_raw(&o, b"element", e);
    Ok(Value::Object(o))
}

/// The namespaces in scope at `el`: (prefix, uri) in declaration order
/// from the outermost, innermost bindings winning (`xmlns` attributes and
/// element names).
fn in_scope(a: &Arena, el: NodeId) -> Vec<(Option<String>, String)> {
    let mut chain = Vec::new();
    let mut c = Some(el);
    while let Some(x) = c {
        if a.is_element(x) {
            chain.push(x);
        }
        c = a.parent(x).filter(|&p| a.is_element(p));
    }
    let mut out: Vec<(Option<String>, String)> = Vec::new();
    for &x in chain.iter().rev() {
        for at in a.attrs(x) {
            let q = a.qname(at).unwrap();
            if q.ns.as_deref() != Some(goro_xml::tree::XMLNS_NS) {
                continue;
            }
            let p = if q.prefix.is_some() {
                Some(q.local.to_string())
            } else {
                None
            };
            let v = a.attr_value(at);
            out.retain(|(op, _)| *op != p);
            if !v.is_empty() {
                out.push((p, v));
            }
        }
        if let Some(e) = a.element(x)
            && let Some(ns) = &e.name.ns
        {
            let p = e.name.prefix.as_ref().map(|s| s.to_string());
            if !out.iter().any(|(op, u)| *op == p && u == &**ns) {
                out.retain(|(op, _)| *op != p);
                out.push((p, ns.to_string()));
            }
        }
    }
    out.sort_by(|x, y| x.0.cmp(&y.0));
    out
}

fn get_in_scope_namespaces(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let list = with_arena(|ar| in_scope(ar, el));
    let mut arr = PhpArray::new();
    for (p, u) in list {
        let v = namespace_info(vm, p, Some(u), el)?;
        let _ = arr.push(v);
    }
    Ok(Value::Array(arr))
}

fn get_descendant_namespaces(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let mut items: Vec<(Option<String>, String, NodeId)> = Vec::new();
    let all = with_arena(|ar| {
        let mut v = vec![el];
        v.extend(ar.descendants(el).filter(|&n| ar.is_element(n)));
        let mut out = Vec::new();
        let mut parent_scope: Vec<(Option<String>, String)> = Vec::new();
        for (i, &x) in v.iter().enumerate() {
            let scope = in_scope(ar, x);
            let parent = if i == 0 {
                Vec::new()
            } else {
                ar.parent(x)
                    .filter(|&p| ar.is_element(p))
                    .map(|p| in_scope(ar, p))
                    .unwrap_or_default()
            };
            for (p, u) in scope {
                if i == 0 || !parent.iter().any(|(pp, uu)| *pp == p && *uu == u) {
                    out.push((p, u, x));
                }
            }
            parent_scope.clear();
        }
        out
    });
    items.extend(all);
    let mut arr = PhpArray::new();
    for (p, u, n) in items {
        let v = namespace_info(vm, p, Some(u), n)?;
        let _ = arr.push(v);
    }
    Ok(Value::Array(arr))
}

// ------------------------------------------------------------ TokenList

fn tokens(a: &Arena, el: NodeId) -> Vec<String> {
    let Some(at) = a.get_attr_node_ns(el, None, "class") else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for t in split_ws(&a.attr_value(at)) {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

fn set_tokens(el: NodeId, v: &[String]) {
    with_arena(|a| {
        let has = a.get_attr_node_ns(el, None, "class").is_some();
        if !has && v.is_empty() {
            return;
        }
        let s = v.join(" ");
        match a.get_attr_node_ns(el, None, "class") {
            Some(at) => a.set_attr_value(at, &s),
            None => {
                let d = a.doc_of(el);
                let at = a.new_attribute(d, QName::local("class"), &s);
                a.raw_append_attr(el, at);
            }
        }
    });
}

fn tl_el(vm: &mut Vm, a: &Args) -> R<NodeId> {
    let o = a.this_obj();
    let n = o.obj().native.borrow();
    let el = n.as_ref().and_then(|b| {
        let any: &dyn std::any::Any = &**b;
        any.downcast_ref::<super::lists::ListObj>().and_then(|l| match l.kind {
            ListKind::ClassList(e) => Some(e),
            _ => None,
        })
    });
    drop(n);
    match el {
        Some(e) => Ok(e),
        None => vm.throw("Error", "Couldn't fetch Dom\\TokenList"),
    }
}

fn check_token(vm: &mut Vm, t: &str) -> R<()> {
    if t.is_empty() {
        return throw_dom_msg(vm, "The empty string is not a valid token", 12);
    }
    if t.contains([' ', '\t', '\n', '\r', '\x0c']) {
        return throw_dom_msg(vm, "The token must not contain any ASCII whitespace", 5);
    }
    Ok(())
}

/// A named token argument without NUL bytes.
fn named_token(vm: &mut Vm, a: &Args, i: usize, name: &str) -> R<String> {
    let s = a.str(i);
    if s.as_bytes().contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #{} (${name}) must not contain any null bytes", i + 1),
        );
    }
    Ok(s8(&s))
}

/// Dimension access on a TokenList (tests modern/token_list/dimensions*.phpt).
pub(crate) fn token_dim(
    vm: &mut Vm,
    el: NodeId,
    key: Option<&Value>,
    op: crate::class::ObjAccess,
) -> Option<R<Value>> {
    use crate::class::ObjAccess;
    let isset = matches!(op, ObjAccess::Isset | ObjAccess::Empty);
    let Some(key) = key else {
        return Some(vm.throw("Error", "Cannot append to Dom\\TokenList"));
    };
    if matches!(op, ObjAccess::Write(_) | ObjAccess::Unset) {
        return None;
    }
    let bad = |vm: &mut Vm, t: String| -> Option<R<Value>> {
        let m = if isset {
            format!("Cannot access offset of type {t} in isset or empty")
        } else {
            format!("Cannot access offset of type {t} on Dom\\TokenList")
        };
        Some(vm.throw("TypeError", &m))
    };
    let idx = match key.deref() {
        Value::Int(i) => i,
        Value::Bool(b) => b as i64,
        Value::Null => 0,
        Value::Float(f) => {
            if f.fract() != 0.0 && f.is_finite()
                && let Err(e) = vm.deprecated(&format!(
                    "Implicit conversion from float {} to int loses precision",
                    goro_runtime::convert::float_repr(f)
                ))
            {
                return Some(Err(e));
            }
            f as i64
        }
        Value::Str(s) => match goro_runtime::array::canonical_int(s.as_bytes()) {
            Some(i) => i,
            None => return bad(vm, "string".into()),
        },
        Value::Resource(r) => {
            let id = r.id();
            if let Err(e) = vm.warn(&format!(
                "Resource ID#{id} used as offset, casting to integer ({id})"
            )) {
                return Some(Err(e));
            }
            id
        }
        other => return bad(vm, crate::class::TypeName::tname(&other)),
    };
    let t = with_arena(|ar| tokens(ar, el));
    let found = if idx < 0 { None } else { t.get(idx as usize).cloned() };
    Some(Ok(match op {
        ObjAccess::Isset => Value::Bool(found.is_some()),
        ObjAccess::Empty => Value::Bool(found.as_deref().is_none_or(|s| s.is_empty() || s == "0")),
        _ => found.map_or(Value::Null, |s| sv(&s)),
    }))
}

fn tl_item(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = tl_el(vm, a)?;
    let i = a.int(0);
    let t = with_arena(|ar| tokens(ar, el));
    if i < 0 {
        return Ok(Value::Null);
    }
    Ok(t.get(i as usize).map_or(Value::Null, |s| sv(s)))
}

fn tl_contains(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = tl_el(vm, a)?;
    let tok = named_token(vm, a, 0, "token")?;
    Ok(Value::Bool(with_arena(|ar| tokens(ar, el)).contains(&tok)))
}

fn arg_tokens(vm: &mut Vm, a: &Args) -> R<Vec<String>> {
    let mut v = Vec::new();
    for i in 0..a.len() {
        let s = a.val(i);
        let s = vm.to_php_str(&s)?;
        if s.as_bytes().contains(&0) {
            return vm.fthrow(
                a,
                "ValueError",
                &format!("Argument #{} must not contain any null bytes", i + 1),
            );
        }
        v.push(s8(&s));
    }
    for t in &v {
        check_token(vm, t)?;
    }
    Ok(v)
}

fn tl_add(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = tl_el(vm, a)?;
    let add = arg_tokens(vm, a)?;
    let mut t = with_arena(|ar| tokens(ar, el));
    for x in add {
        if !t.contains(&x) {
            t.push(x);
        }
    }
    set_tokens(el, &t);
    Ok(Value::Null)
}

fn tl_remove(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = tl_el(vm, a)?;
    let rm = arg_tokens(vm, a)?;
    let mut t = with_arena(|ar| tokens(ar, el));
    t.retain(|x| !rm.contains(x));
    set_tokens(el, &t);
    Ok(Value::Null)
}

fn tl_toggle(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = tl_el(vm, a)?;
    let tok = named_token(vm, a, 0, "token")?;
    check_token(vm, &tok)?;
    let force = match a.val(1) {
        Value::Null => None,
        v => Some(vm.to_bool(&v)),
    };
    let mut t = with_arena(|ar| tokens(ar, el));
    let r = if t.contains(&tok) {
        if force == Some(true) {
            true
        } else {
            t.retain(|x| *x != tok);
            set_tokens(el, &t);
            false
        }
    } else if force == Some(false) {
        false
    } else {
        t.push(tok);
        set_tokens(el, &t);
        true
    };
    Ok(Value::Bool(r))
}

fn tl_replace(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = tl_el(vm, a)?;
    let old = named_token(vm, a, 0, "token")?;
    let new = named_token(vm, a, 1, "newToken")?;
    check_token(vm, &old)?;
    check_token(vm, &new)?;
    let mut t = with_arena(|ar| tokens(ar, el));
    let Some(i) = t.iter().position(|x| *x == old) else {
        return Ok(Value::Bool(false));
    };
    if t.contains(&new) {
        t.remove(i);
        let j = t.iter().position(|x| *x == new).unwrap();
        if j > i {
            let v = t.remove(j);
            t.insert(i, v);
        }
    } else {
        t[i] = new;
    }
    set_tokens(el, &t);
    Ok(Value::Bool(true))
}

fn tl_supports(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.throw(
        "TypeError",
        "Attribute \"class\" does not define any supported tokens",
    )
}

fn tl_count(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = tl_el(vm, a)?;
    Ok(Value::Int(with_arena(|ar| tokens(ar, el)).len() as i64))
}

fn tl_iterator(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    Ok(Value::Object(crate::builtins::spl::new_ext_iter(
        vm,
        &o,
        token_iter_step,
    )?))
}

pub(crate) fn token_iter_step(vm: &mut Vm, o: &ObjRef, pos: usize, op: ExtIterOp, state: &mut Value) -> R<Value> {
    let _ = vm;
    let el = {
        let n = o.obj().native.borrow();
        n.as_ref().and_then(|b| {
            let any: &dyn std::any::Any = &**b;
            any.downcast_ref::<super::lists::ListObj>().and_then(|l| match l.kind {
                ListKind::ClassList(e) => Some(e),
                _ => None,
            })
        })
    };
    let Some(el) = el else {
        return Ok(Value::Null);
    };
    let t = with_arena(|ar| tokens(ar, el));
    // A modification of the list since the last step keeps the position
    // (test modern/token_list/iteration_01.phpt).
    let now = Value::from(t.join(" ").as_bytes());
    let changed = !state.is_null() && state.as_str() != now.as_str();
    if !matches!(op, ExtIterOp::Next) {
        *state = now;
    }
    Ok(match op {
        ExtIterOp::Next => {
            if changed {
                Value::Int(pos as i64)
            } else {
                Value::Null
            }
        }
        ExtIterOp::Valid => Value::Bool(pos < t.len()),
        ExtIterOp::Current => t.get(pos).map_or(Value::Null, |s| sv(s)),
        ExtIterOp::Key => Value::Int(pos as i64),
    })
}

/// TokenList `length` / `value`.
pub(crate) fn token_list_prop(vm: &mut Vm, el: NodeId, name: &str, is_set: bool, v: Value) -> R<Value> {
    match (name, is_set) {
        ("length", false) => Ok(Value::Int(with_arena(|ar| tokens(ar, el)).len() as i64)),
        ("value", false) => Ok(sv(&with_arena(|ar| {
            ar.get_attr_node_ns(el, None, "class")
                .map(|at| ar.attr_value(at))
                .unwrap_or_default()
        }))),
        ("value", true) => {
            let s = vm.to_php_str(&v)?;
            if s.as_bytes().contains(&0) {
                return vm.throw("ValueError", "Value must not contain any null bytes");
            }
            let s = s8(&s);
            with_arena(|ar| match ar.get_attr_node_ns(el, None, "class") {
                Some(at) => ar.set_attr_value(at, &s),
                None => {
                    let d = ar.doc_of(el);
                    let at = ar.new_attribute(d, QName::local("class"), &s);
                    ar.raw_append_attr(el, at);
                }
            });
            Ok(Value::Null)
        }
        _ => vm.throw(
            "Error",
            &format!("Cannot modify readonly property Dom\\TokenList::${name}"),
        ),
    }
}

/// Unused helpers kept for API symmetry.
pub(crate) fn _unused(vm: &mut Vm, a: &Args) -> R<Value> {
    let _ = (arg_node(vm, a, 0)?, modern_fn(a));
    let _ = throw_dom::<()>;
    let _ = Key::from_bytes(b"");
    let _ = PhpStr::empty();
    Ok(Value::Null)
}
