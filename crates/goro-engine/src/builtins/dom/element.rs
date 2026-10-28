//! DOMElement / Dom\Element: attributes (legacy libxml-style namespace
//! handling for `DOM*`, the DOM Standard's for `Dom\`), element queries,
//! and the ParentNode / ChildNode mixins shared by elements, documents,
//! fragments and character data (PHP manual: DOMElement, DOMParentNode,
//! DOMChildNode; WHATWG DOM; observed on PHP 8.5).

use goro_runtime::{PhpArray, Value};
use goro_xml::tree::{DocKind, DomError, NodeData, NsDecl, QName, XMLNS_NS};
use goro_xml::{Arena, NodeId};

use super::NOT_ID;
use super::lists::ListKind;
use super::node::{LegacyInsertError, legacy_insert, modern_insert};
use super::xmlstore;
use super::{
    arg_node, legacy_error, modern_fn, node_of, s8, sv, sv_opt, this_node, throw_dom,
    throw_dom_msg, with_arena, wrap_node, wrap_opt,
};
use super::document::NameCheck as NC;
use crate::func::{Args, NativeFn};
use crate::vm::{R, Vm};

pub const METHODS: &[(&str, NativeFn)] = &[
    ("domelement::getattribute", get_attribute),
    ("domelement::getattributens", get_attribute_ns),
    ("domelement::getattributenode", get_attribute_node),
    ("domelement::getattributenodens", get_attribute_node_ns),
    ("domelement::getattributenames", get_attribute_names),
    ("domelement::hasattribute", has_attribute),
    ("domelement::hasattributens", has_attribute_ns),
    ("domelement::removeattribute", remove_attribute),
    ("domelement::removeattributens", remove_attribute_ns),
    ("domelement::removeattributenode", remove_attribute_node),
    ("domelement::setattribute", set_attribute),
    ("domelement::setattributens", set_attribute_ns),
    ("domelement::setattributenode", set_attribute_node),
    ("domelement::setattributenodens", set_attribute_node),
    ("domelement::setidattribute", set_id_attribute),
    ("domelement::setidattributens", set_id_attribute_ns),
    ("domelement::setidattributenode", set_id_attribute_node),
    ("domelement::toggleattribute", toggle_attribute),
    ("domelement::getelementsbytagname", get_elements_by_tag_name),
    ("domelement::getelementsbytagnamens", get_elements_by_tag_name_ns),
    ("domelement::insertadjacentelement", insert_adjacent_element),
    ("domelement::insertadjacenttext", insert_adjacent_text),
    ("domelement::append", append),
    ("domelement::prepend", prepend),
    ("domelement::replacechildren", replace_children),
    ("domelement::before", before),
    ("domelement::after", after),
    ("domelement::replacewith", replace_with),
    ("domelement::remove", remove),
    ("domdocumentfragment::appendxml", append_xml),
    ("domdocumentfragment::append", append),
    ("domdocumentfragment::prepend", prepend),
    ("domdocumentfragment::replacechildren", replace_children),
    ("domcharacterdata::before", before),
    ("domcharacterdata::after", after),
    ("domcharacterdata::replacewith", replace_with),
    ("domcharacterdata::remove", remove),
    ("dom\\element::hasattributes", has_attributes),
    ("dom\\element::getattribute", get_attribute),
    ("dom\\element::getattributens", get_attribute_ns),
    ("dom\\element::getattributenode", get_attribute_node),
    ("dom\\element::getattributenodens", get_attribute_node_ns),
    ("dom\\element::getattributenames", get_attribute_names),
    ("dom\\element::hasattribute", has_attribute),
    ("dom\\element::hasattributens", has_attribute_ns),
    ("dom\\element::removeattribute", remove_attribute),
    ("dom\\element::removeattributens", remove_attribute_ns),
    ("dom\\element::removeattributenode", remove_attribute_node),
    ("dom\\element::setattribute", set_attribute),
    ("dom\\element::setattributens", set_attribute_ns),
    ("dom\\element::setattributenode", set_attribute_node),
    ("dom\\element::setattributenodens", set_attribute_node),
    ("dom\\element::setidattribute", set_id_attribute),
    ("dom\\element::setidattributens", set_id_attribute_ns),
    ("dom\\element::setidattributenode", set_id_attribute_node),
    ("dom\\element::toggleattribute", toggle_attribute),
    ("dom\\element::getelementsbytagname", get_elements_by_tag_name),
    ("dom\\element::getelementsbytagnamens", get_elements_by_tag_name_ns),
    ("dom\\element::getelementsbyclassname", super::modern::get_elements_by_class_name),
    ("dom\\element::insertadjacentelement", insert_adjacent_element),
    ("dom\\element::insertadjacenttext", insert_adjacent_text),
    ("dom\\element::append", append),
    ("dom\\element::prepend", prepend),
    ("dom\\element::replacechildren", replace_children),
    ("dom\\element::before", before),
    ("dom\\element::after", after),
    ("dom\\element::replacewith", replace_with),
    ("dom\\element::remove", remove),
    ("dom\\documentfragment::appendxml", append_xml),
    ("dom\\documentfragment::append", append),
    ("dom\\documentfragment::prepend", prepend),
    ("dom\\documentfragment::replacechildren", replace_children),
    ("dom\\documentfragment::queryselector", super::modern::query_selector),
    ("dom\\documentfragment::queryselectorall", super::modern::query_selector_all),
    ("dom\\characterdata::before", before),
    ("dom\\characterdata::after", after),
    ("dom\\characterdata::replacewith", replace_with),
    ("dom\\characterdata::remove", remove),
    ("dom\\documenttype::before", before),
    ("dom\\documenttype::after", after),
    ("dom\\documenttype::replacewith", replace_with),
    ("dom\\documenttype::remove", remove),
];

// ------------------------------------------------------------ construction

/// `new DOMElement($qualifiedName, $value, $namespace)`.
pub(crate) fn construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let name = s8(&a.str(0));
    let value = a.opt_str(1).map(|s| s8(&s));
    let ns = a.opt_str(2).map(|s| s8(&s)).filter(|s| !s.is_empty());
    let q = match &ns {
        Some(_) => match super::document::validate_and_extract(ns.as_deref(), &name, NC::Ctor) {
            Ok(q) => q,
            Err(e) => return throw_dom(vm, e),
        },
        None => {
            if !goro_xml::chars::is_name(&name) {
                return throw_dom(vm, DomError::InvalidCharacter);
            }
            if name.contains(':') {
                return throw_dom(vm, DomError::Namespace);
            }
            QName::local(&name)
        }
    };
    let d = super::document::hidden_doc();
    let e = with_arena(|ar| {
        let e = ar.new_element(d, q.clone());
        if let Some(uri) = &q.ns {
            ar.declare_ns(e, q.prefix.as_deref(), uri);
        }
        e
    });
    if let Some(v) = value.filter(|v| !v.is_empty()) {
        super::node::set_node_value(vm, e, &v, false)?;
    }
    super::attach(a.this_obj(), e);
    Ok(Value::Null)
}

// ------------------------------------------------------- attribute lookup

/// True if the element is an HTML element of an HTML document (attribute
/// names are matched in lower case; DOM Standard).
fn lower_names(ar: &Arena, el: NodeId) -> bool {
    super::props::html_upper(ar, el)
}

/// The attribute named `name` (qualified name match).
fn find_attr(ar: &Arena, el: NodeId, name: &str, modern: bool) -> Option<NodeId> {
    if modern && lower_names(ar, el) {
        let l = name.to_ascii_lowercase();
        return ar.attrs(el).find(|&at| ar.qname(at).is_some_and(|q| q.is(&l)));
    }
    ar.get_attr_node(el, name)
}

/// A legacy namespace declaration addressed as an attribute (`xmlns`,
/// `xmlns:p`): its URI.
fn ns_decl_value(ar: &Arena, el: NodeId, name: &str) -> Option<String> {
    let prefix = if name == "xmlns" {
        None
    } else {
        Some(name.strip_prefix("xmlns:")?)
    };
    ar.element(el)?
        .ns_decls
        .iter()
        .find(|d| d.prefix.as_deref() == prefix)
        .map(|d| d.uri.to_string())
}

fn get_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let name = s8(&a.str(0));
    let modern = modern_fn(a);
    let r = with_arena(|ar| {
        if !ar.is_element(el) {
            return None;
        }
        if let Some(at) = find_attr(ar, el, &name, modern) {
            return Some(ar.attr_value(at));
        }
        if !modern {
            return ns_decl_value(ar, el, &name);
        }
        None
    });
    match r {
        Some(v) => Ok(sv(&v)),
        None if modern => Ok(Value::Null),
        None => Ok(sv("")),
    }
}

fn get_attribute_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let ns = a.opt_str(0).map(|s| s8(&s)).filter(|s| !s.is_empty());
    let local = s8(&a.str(1));
    let modern = modern_fn(a);
    let r = with_arena(|ar| {
        if !modern && ns.as_deref() == Some(XMLNS_NS) {
            let p = if local == "xmlns" {
                None
            } else {
                Some(local.as_str())
            };
            return ar
                .element(el)?
                .ns_decls
                .iter()
                .find(|d| d.prefix.as_deref() == p)
                .map(|d| d.uri.to_string());
        }
        ar.get_attr_node_ns(el, ns.as_deref(), &local)
            .map(|at| ar.attr_value(at))
    });
    match r {
        Some(v) => Ok(sv(&v)),
        None if modern => Ok(Value::Null),
        None => Ok(sv("")),
    }
}

/// A namespace declaration of `el` as a DOMNameSpaceNode (created on
/// demand, one per element and prefix).
pub(crate) fn ns_node(ar: &mut Arena, el: NodeId, prefix: Option<&str>, uri: &str) -> NodeId {
    let doc = ar.doc_of(el);
    let n = ar.create(
        doc,
        NodeData::Namespace(NsDecl {
            prefix: prefix.map(Into::into),
            uri: uri.into(),
        }),
    );
    ar.node_mut(n).parent = Some(el);
    n
}

fn get_attribute_node(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let name = s8(&a.str(0));
    let modern = modern_fn(a);
    let r = with_arena(|ar| {
        if let Some(at) = find_attr(ar, el, &name, modern) {
            return Some(at);
        }
        if !modern && let Some(uri) = ns_decl_value(ar, el, &name) {
            let p = name.strip_prefix("xmlns:");
            return Some(ns_node(ar, el, p, &uri));
        }
        None
    });
    match r {
        Some(n) => {
            let v = wrap_node(vm, n)?;
            with_arena(|ar| ar.free_namespace_node(n));
            Ok(v)
        }
        None if modern => Ok(Value::Null),
        None => Ok(Value::Bool(false)),
    }
}

fn get_attribute_node_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let ns = a.opt_str(0).map(|s| s8(&s)).filter(|s| !s.is_empty());
    let local = s8(&a.str(1));
    let modern = modern_fn(a);
    let r = with_arena(|ar| {
        if !modern && ns.as_deref() == Some(XMLNS_NS) {
            let p = if local == "xmlns" {
                None
            } else {
                Some(local.clone())
            };
            let uri = ar
                .element(el)?
                .ns_decls
                .iter()
                .find(|d| d.prefix.as_deref() == p.as_deref())
                .map(|d| d.uri.to_string())?;
            return Some(ns_node(ar, el, p.as_deref(), &uri));
        }
        ar.get_attr_node_ns(el, ns.as_deref(), &local)
    });
    match r {
        Some(n) => wrap_node(vm, n),
        None => Ok(Value::Null),
    }
}

fn get_attribute_names(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let modern = modern_fn(a);
    let names = with_arena(|ar| {
        let mut v = Vec::new();
        if !modern && let Some(e) = ar.element(el) {
            for d in &e.ns_decls {
                v.push(match &d.prefix {
                    Some(p) => format!("xmlns:{p}"),
                    None => "xmlns".to_string(),
                });
            }
        }
        for at in ar.attrs(el) {
            v.push(ar.qname(at).unwrap().qualified());
        }
        v
    });
    let mut arr = PhpArray::new();
    for n in names {
        let _ = arr.push(sv(&n));
    }
    Ok(Value::Array(arr))
}

fn has_attributes(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    Ok(Value::Bool(with_arena(|ar| ar.attrs(el).next().is_some())))
}

fn has_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let name = s8(&a.str(0));
    let modern = modern_fn(a);
    Ok(Value::Bool(with_arena(|ar| {
        find_attr(ar, el, &name, modern).is_some()
            || (!modern && ns_decl_value(ar, el, &name).is_some())
    })))
}

fn has_attribute_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let ns = a.opt_str(0).map(|s| s8(&s)).filter(|s| !s.is_empty());
    let local = s8(&a.str(1));
    let modern = modern_fn(a);
    Ok(Value::Bool(with_arena(|ar| {
        if !modern && ns.as_deref() == Some(XMLNS_NS) {
            let p = if local == "xmlns" {
                None
            } else {
                Some(local.as_str())
            };
            return ar
                .element(el)
                .is_some_and(|e| e.ns_decls.iter().any(|d| d.prefix.as_deref() == p));
        }
        ar.get_attr_node_ns(el, ns.as_deref(), &local).is_some()
    })))
}

/// Detaches attribute `at` from its element (freeing it if unreferenced).
fn drop_attr(s: &mut xmlstore::Store, at: NodeId) {
    s.arena.detach(at);
    s.free_if_unheld(at);
}

fn remove_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let name = s8(&a.str(0));
    let modern = modern_fn(a);
    let r = xmlstore::with(|s| {
        if let Some(at) = find_attr(&s.arena, el, &name, modern) {
            drop_attr(s, at);
            return true;
        }
        if !modern {
            let prefix = if name == "xmlns" {
                Some(None)
            } else {
                name.strip_prefix("xmlns:").map(Some)
            };
            if let Some(p) = prefix
                && let Some(e) = s.arena.element_mut(el)
            {
                let before = e.ns_decls.len();
                e.ns_decls.retain(|d| d.prefix.as_deref() != p);
                let changed = e.ns_decls.len() != before;
                if changed {
                    // Names using it get their own declaration (test
                    // bug47531_a.phpt).
                    declare_dangling(&mut s.arena, el);
                }
                return changed;
            }
        }
        false
    });
    if modern {
        return Ok(Value::Null);
    }
    Ok(Value::Bool(r))
}

fn remove_attribute_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let ns = a.opt_str(0).map(|s| s8(&s)).filter(|s| !s.is_empty());
    let local = s8(&a.str(1));
    let modern = modern_fn(a);
    xmlstore::with(|s| {
        if !modern && ns.as_deref() == Some(XMLNS_NS) {
            let p = if local == "xmlns" {
                None
            } else {
                Some(local.as_str())
            };
            if let Some(e) = s.arena.element_mut(el) {
                e.ns_decls.retain(|d| d.prefix.as_deref() != p);
            }
            declare_dangling(&mut s.arena, el);
            return;
        }
        if let Some(at) = s.arena.get_attr_node_ns(el, ns.as_deref(), &local) {
            drop_attr(s, at);
        }
    });
    Ok(Value::Null)
}

fn remove_attribute_node(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let at = arg_node(vm, a, 0)?;
    let modern = modern_fn(a);
    let ok = with_arena(|ar| ar.parent(at) == Some(el) && matches!(ar.data(at), NodeData::Attribute(_)));
    if !ok {
        if modern {
            return throw_dom(vm, DomError::NotFound);
        }
        return legacy_error(vm, a, el, DomError::NotFound);
    }
    with_arena(|ar| ar.detach(at));
    Ok(a.val(0))
}

/// Sets attribute `name` (legacy: also namespace declarations by their
/// `xmlns:p` name).
pub(crate) fn set_attr_by_name(vm: &mut Vm, el: NodeId, name: &str, value: &str) -> R<()> {
    let _ = vm;
    with_arena(|ar| {
        ar.set_attribute(el, name, value);
    });
    Ok(())
}

fn set_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let name = s8(&a.str(0));
    let value = s8(&a.str(1));
    let modern = modern_fn(a);
    if !goro_xml::chars::is_name(&name) {
        if modern {
            return throw_dom(vm, DomError::InvalidCharacter);
        }
        return legacy_error(vm, a, el, DomError::InvalidCharacter);
    }
    if modern {
        with_arena(|ar| {
            let n = if lower_names(ar, el) {
                name.to_ascii_lowercase()
            } else {
                name.clone()
            };
            match ar.attrs(el).find(|&at| ar.qname(at).is_some_and(|q| q.is(&n))) {
                Some(at) => ar.set_attr_value(at, &value),
                None => {
                    let doc = ar.doc_of(el);
                    let at = ar.new_attribute(doc, QName::local(&n), &value);
                    ar.raw_append_attr(el, at);
                }
            }
        });
        return Ok(Value::Null);
    }
    let at = with_arena(|ar| {
        if !ar.is_element(el) {
            return None;
        }
        Some(ar.set_attribute(el, &name, &value))
    });
    match at {
        Some(at) => wrap_node(vm, at),
        None => Ok(Value::Bool(false)),
    }
}

/// A prefix for namespace `ns` usable on `el` without clashing with the
/// declarations in scope ("default", "default1", ...; observed on PHP
/// 8.5).
pub(crate) fn generated_prefix(ar: &Arena, el: NodeId) -> String {
    let mut i = 0;
    loop {
        let p = if i == 0 {
            "default".to_string()
        } else {
            format!("default{i}")
        };
        if ar.lookup_ns(el, Some(&p)).is_none() {
            return p;
        }
        i += 1;
    }
}

fn set_attribute_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let ns = a.opt_str(0).map(|s| s8(&s)).filter(|s| !s.is_empty());
    let qn = s8(&a.str(1));
    let value = s8(&a.str(2));
    let modern = modern_fn(a);
    let q = match super::document::validate_and_extract(ns.as_deref(), &qn, if modern { NC::Modern } else { NC::Legacy }) {
        Ok(q) => q,
        Err(e) if modern => return throw_dom(vm, e),
        Err(e) => return legacy_error(vm, a, el, e),
    };
    if modern {
        with_arena(|ar| {
            match ar.get_attr_node_ns(el, q.ns.as_deref(), &q.local) {
                Some(at) => ar.set_attr_value(at, &value),
                None => {
                    let doc = ar.doc_of(el);
                    let at = ar.new_attribute(doc, q.clone(), &value);
                    ar.raw_append_attr(el, at);
                }
            }
        });
        return Ok(Value::Null);
    }
    let r = with_arena(|ar| -> Result<(), DomError> {
        let Some(uri) = q.ns.clone() else {
            ar.set_attribute(el, &q.local, &value);
            return Ok(());
        };
        if &*uri == XMLNS_NS {
            // A namespace declaration.
            let p = if q.prefix.is_none() {
                None
            } else {
                Some(q.local.to_string())
            };
            let e = ar.element_mut(el).ok_or(DomError::NotSupported)?;
            match e.ns_decls.iter_mut().find(|d| d.prefix.as_deref() == p.as_deref()) {
                Some(d) => {
                    if d.uri.is_empty() || p.is_none() {
                        d.uri = value.as_str().into();
                    } else {
                        return Err(DomError::Namespace);
                    }
                }
                None => e.ns_decls.push(NsDecl {
                    prefix: p.as_deref().map(Into::into),
                    uri: value.as_str().into(),
                }),
            }
            return Ok(());
        }
        // An existing attribute with this namespace and local name.
        if let Some(at) = ar.get_attr_node_ns(el, Some(&uri), &q.local) {
            ar.set_attr_value(at, &value);
            if let Some(p) = &q.prefix
                && ar.lookup_ns(el, Some(p)).as_deref() == Some(&*uri)
            {
                ar.attr_mut(at).unwrap().name.prefix = Some(p.clone());
            }
            return Ok(());
        }
        let mut declared = false;
        let mut prefix_out: Option<goro_xml::Str> = None;
        let prefix = match &q.prefix {
            Some(p) => {
                match ar.lookup_ns(el, Some(p)) {
                    Some(bound) if bound == uri => {}
                    Some(_) => {
                        let e = ar.element_mut(el).unwrap();
                        if e.ns_decls.iter().any(|d| d.prefix.as_deref() == Some(p)) {
                            // Bound to another namespace on this element: a
                            // generated prefix (observed on PHP 8.5).
                            let g = match ar.lookup_prefix(el, &uri) {
                                Some(x) => x.to_string(),
                                None => {
                                    let g = generated_prefix(ar, el);
                                    ar.declare_ns(el, Some(&g), &uri);
                                    declared = true;
                                    g
                                }
                            };
                            prefix_out = Some(g.as_str().into());
                        } else {
                            // Bound on an ancestor: redeclared here.
                            e.ns_decls.push(NsDecl {
                                prefix: Some(p.clone()),
                                uri: uri.clone(),
                            });
                            declared = true;
                        }
                    }
                    None => {
                        ar.declare_ns(el, Some(p), &uri);
                        declared = true;
                    }
                }
                prefix_out.or(Some(p.clone()))
            }
            None => {
                // A prefix bound to the namespace in scope; else libxml binds
                // the attribute to a new default declaration when there is
                // none in scope, or a generated prefix (observed on PHP 8.5,
                // fixture domops.out, test bug34276.phpt).
                match ar.lookup_prefix(el, &uri) {
                    Some(p) => Some(p),
                    None => {
                        if ar.lookup_ns(el, None).is_none() {
                            ar.declare_ns(el, None, &uri);
                            declared = true;
                            None
                        } else {
                            let p = generated_prefix(ar, el);
                            ar.declare_ns(el, Some(&p), &uri);
                            declared = true;
                            Some(p.as_str().into())
                        }
                    }
                }
            }
        };
        let doc = ar.doc_of(el);
        let at = ar.new_attribute(
            doc,
            QName {
                prefix,
                local: q.local.clone(),
                ns: Some(uri),
            },
            &value,
        );
        ar.raw_append_attr(el, at);
        if declared {
            reconcile_tree(ar, el);
        }
        Ok(())
    });
    match r {
        Ok(()) => Ok(Value::Null),
        Err(e) => legacy_error(vm, a, el, e).map(|_| Value::Null),
    }
}

/// `setAttributeNode()` / appending an attribute: replaces an attribute of
/// the same name. Returns the old one's object (or null).
pub(crate) fn set_attribute_node_raw(
    vm: &mut Vm,
    a: &Args,
    el: NodeId,
    at: NodeId,
    modern: bool,
    ns_variant: bool,
) -> R<Value> {
    let r = xmlstore::with(|s| {
        let ar = &mut s.arena;
        if let Some(p) = ar.parent(at) {
            if p == el {
                return Ok(Some(at));
            }
            if modern {
                return Err(DomError::InUseAttribute);
            }
            // Legacy: the attribute moves (test bug45251.phpt).
            ar.detach(at);
        }
        let elem_doc = ar.doc_of(el);
        if ar.doc_of(at) != elem_doc {
            let hidden = s
                .docs
                .get(&s.arena.doc_of(at))
                .and_then(|d| d.peek::<super::DocSettings>())
                .is_some_and(|d| d.hidden);
            if !hidden && !modern {
                return Err(DomError::WrongDocument);
            }
            s.arena.adopt(at, elem_doc);
        }
        let ar = &mut s.arena;
        let q = ar.qname(at).unwrap().clone();
        // setAttributeNode() replaces an attribute of the same local name
        // (any namespace), setAttributeNodeNS() one of the same namespace
        // and local name (observed on PHP 8.5, tests
        // createAttributeNS_prefix_conflicts/*.phpt).
        let old = if ns_variant || modern {
            ar.get_attr_node_ns(el, q.ns.as_deref(), &q.local)
        } else {
            ar.attrs(el).find(|&x| ar.qname(x).is_some_and(|k| k.local == q.local))
        };
        if let Some(o) = old {
            if o == at {
                return Ok(Some(at));
            }
            ar.detach(o);
        }
        if let Some(uri) = q.ns.clone()
            && !modern
            && &*uri != XMLNS_NS
            && q.prefix.as_deref() != Some("xml")
        {
            // A prefixed binding of the namespace in scope, the attribute's
            // own prefix when free, else a generated one.
            let p = match search_by_href(ar, el, &uri, true) {
                Some(p) => p,
                None => match &q.prefix {
                    Some(p) if ar.lookup_ns(el, Some(p)).is_none() => {
                        ar.declare_ns(el, Some(p), &uri);
                        Some(p.clone())
                    }
                    _ => {
                        let g = generated_prefix(ar, el);
                        ar.declare_ns(el, Some(&g), &uri);
                        Some(g.as_str().into())
                    }
                },
            };
            if let Some(qq) = ar.data_mut_qname(at) {
                qq.prefix = p;
            }
        }
        ar.raw_append_attr(el, at);
        s.collect();
        Ok(old)
    });
    match r {
        Ok(Some(o)) if o == at => Ok(Value::Null),
        Ok(Some(o)) => {
            let v = wrap_node(vm, o)?;
            Ok(v)
        }
        Ok(None) => Ok(Value::Null),
        Err(e) if modern => throw_dom(vm, e),
        Err(e) => legacy_error(vm, a, el, e),
    }
}

fn set_attribute_node(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let at = arg_node(vm, a, 0)?;
    let modern = modern_fn(a);
    let ns_variant = a.func.lname.as_bytes().ends_with(b"ns");
    set_attribute_node_raw(vm, a, el, at, modern, ns_variant)
}

fn set_id(ar: &mut Arena, at: NodeId, is_id: bool) {
    if let Some(d) = ar.attr_mut(at) {
        d.is_id = is_id;
    }
    let doc = ar.doc_of(at);
    NOT_ID.with(|m| {
        let mut m = m.borrow_mut();
        let set = m.entry(doc).or_default();
        if is_id {
            set.remove(&at);
        } else {
            set.insert(at);
        }
    });
    let v = ar.attr_value(at);
    let doc = ar.doc_of(at);
    let el = ar.parent(at);
    if is_id {
        if let Some(e) = el {
            ar.doc_mut(doc).ids.insert(v, e);
        }
    } else if ar.doc(doc).ids.get(&v).copied() == el {
        ar.doc_mut(doc).ids.remove(&v);
    }
}

fn set_id_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let name = s8(&a.str(0));
    let is_id = a.bool(1);
    let modern = modern_fn(a);
    let found = with_arena(|ar| match find_attr(ar, el, &name, modern) {
        Some(at) => {
            set_id(ar, at, is_id);
            true
        }
        None => false,
    });
    if !found {
        if modern {
            return throw_dom(vm, DomError::NotFound);
        }
        legacy_error(vm, a, el, DomError::NotFound)?;
    }
    Ok(Value::Null)
}

fn set_id_attribute_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let ns = a.opt_str(0).map(|s| s8(&s)).filter(|s| !s.is_empty());
    let local = s8(&a.str(1));
    let is_id = a.bool(2);
    let found = with_arena(|ar| match ar.get_attr_node_ns(el, ns.as_deref(), &local) {
        Some(at) => {
            set_id(ar, at, is_id);
            true
        }
        None => false,
    });
    if !found {
        if modern_fn(a) {
            return throw_dom(vm, DomError::NotFound);
        }
        legacy_error(vm, a, el, DomError::NotFound)?;
    }
    Ok(Value::Null)
}

fn set_id_attribute_node(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let at = arg_node(vm, a, 0)?;
    let is_id = a.bool(1);
    let ok = with_arena(|ar| {
        if ar.parent(at) != Some(el) {
            return false;
        }
        set_id(ar, at, is_id);
        true
    });
    if !ok {
        if modern_fn(a) {
            return throw_dom(vm, DomError::NotFound);
        }
        legacy_error(vm, a, el, DomError::NotFound)?;
    }
    Ok(Value::Null)
}

fn toggle_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let name = s8(&a.str(0));
    let force = a.opt_str(1).is_some() || a.has(1) && !matches!(a.val(1), Value::Null);
    let force_v = a.bool(1);
    let modern = modern_fn(a);
    if !goro_xml::chars::is_name(&name) {
        if modern {
            return throw_dom(vm, DomError::InvalidCharacter);
        }
        return throw_dom(vm, DomError::InvalidCharacter);
    }
    let r = xmlstore::with(|s| {
        let ar = &mut s.arena;
        let html = ar.doc(ar.doc_of(el)).kind == DocKind::Html;
        let n = if (modern && lower_names(ar, el)) || html {
            name.to_ascii_lowercase()
        } else {
            name.clone()
        };
        match find_attr(ar, el, &n, modern) {
            Some(at) => {
                if !force || !force_v {
                    drop_attr(s, at);
                    false
                } else {
                    true
                }
            }
            None => {
                if !force || force_v {
                    let doc = ar.doc_of(el);
                    let at = ar.new_attribute(doc, QName::local(&n), "");
                    ar.raw_append_attr(el, at);
                    true
                } else {
                    false
                }
            }
        }
    });
    Ok(Value::Bool(r))
}

/// DOMAttr::$value setter.
pub(crate) fn set_attr_node_value(n: NodeId, text: &str) {
    with_arena(|ar| {
        if matches!(ar.data(n), NodeData::Attribute(_)) {
            ar.set_attr_value(n, text);
        } else {
            ar.set_text_content(n, text);
        }
    });
}

/// DOMNode::$prefix setter (legacy elements and attributes).
pub(crate) fn set_prefix(vm: &mut Vm, n: NodeId, prefix: &str) -> R<()> {
    let r = with_arena(|ar| {
        let Some(q) = ar.qname(n).cloned() else {
            return Ok(());
        };
        let Some(uri) = q.ns.clone() else {
            return Err(DomError::Namespace);
        };
        if !prefix.is_empty() && !goro_xml::chars::is_ncname(prefix) {
            return Err(DomError::InvalidCharacter);
        }
        let new_p = if prefix.is_empty() {
            None
        } else {
            Some(prefix)
        };
        let el = if ar.is_element(n) {
            n
        } else {
            match ar.parent(n) {
                Some(p) => p,
                None => {
                    match ar.data_mut_qname(n) {
                        Some(qq) => qq.prefix = new_p.map(Into::into),
                        None => {}
                    }
                    return Ok(());
                }
            }
        };
        if ar.lookup_ns(el, new_p).as_deref() != Some(&*uri) {
            ar.declare_ns(el, new_p, &uri);
        }
        if let Some(qq) = ar.data_mut_qname(n) {
            qq.prefix = new_p.map(Into::into);
        }
        ar.touch();
        Ok(())
    });
    match r {
        Ok(()) => Ok(()),
        Err(e) => throw_dom(vm, e),
    }
}

trait QNameMut {
    fn data_mut_qname(&mut self, n: NodeId) -> Option<&mut QName>;
}

impl QNameMut for Arena {
    fn data_mut_qname(&mut self, n: NodeId) -> Option<&mut QName> {
        match &mut self.node_mut(n).data {
            NodeData::Element(e) => Some(&mut e.name),
            NodeData::Attribute(a) => Some(&mut a.name),
            _ => None,
        }
    }
}

// --------------------------------------------------- namespace reconciling

/// The elements of the subtree at `root` in document order.
fn subtree_elements(ar: &Arena, root: NodeId) -> Vec<NodeId> {
    let mut v = Vec::new();
    if ar.is_element(root) {
        v.push(root);
    }
    v.extend(ar.descendants(root).filter(|&n| ar.is_element(n)));
    v
}

/// The names (element first, then attributes) of element `el` that refer
/// to a namespace.
fn ns_users(ar: &Arena, el: NodeId) -> Vec<NodeId> {
    let mut v = vec![el];
    v.extend(ar.attrs(el));
    v
}

/// True if the namespace of name-holder `x` (an element or attribute) is
/// bound by the declarations in scope at `at`.
fn bound_at(ar: &Arena, at: NodeId, q: &QName) -> bool {
    match &q.ns {
        None => true,
        Some(ns) => {
            if q.prefix.as_deref() == Some("xml") || &**ns == XMLNS_NS {
                return true;
            }
            ar.lookup_ns(at, q.prefix.as_deref()).as_deref() == Some(&**ns)
        }
    }
}

/// After a legacy insertion (libxml-style reconciliation, observed on
/// PHP 8.5; tests bug67440.phpt, bug47531_*.phpt): declarations in the
/// inserted subtree that the new context already provides go away, and
/// names whose declaration is no longer in scope get one on their element.
pub(crate) fn reconcile_after_insert(ar: &mut Arena, n: NodeId) {
    if !ar.is_element(n) || ar.is_modern(ar.doc_of(n)) {
        return;
    }
    let Some(parent) = ar.parent(n) else {
        return;
    };
    let ctx_ok = ar.is_element(parent);
    if ctx_ok {
        for el in subtree_elements(ar, n) {
            let p = ar.parent(el).unwrap_or(parent);
            let decls = ar.element(el).map(|e| e.ns_decls.clone()).unwrap_or_default();
            let mut keep: Vec<NsDecl> = Vec::new();
            for d in decls {
                if ar.lookup_ns(p, d.prefix.as_deref()).as_deref() == Some(&*d.uri) {
                    continue;
                }
                // The same namespace under another prefix in scope: names
                // switch to it (an unprefixed binding only serves elements).
                let other = search_by_href(ar, p, &d.uri, false);
                let usable = d.prefix.is_none() && match &other {
                    Some(Some(_)) => true,
                    Some(None) => !subtree_attr_uses(ar, el, &d),
                    None => false,
                };
                if usable {
                    let to = other.unwrap();
                    repoint(ar, el, &d, to);
                    continue;
                }
                keep.push(d);
            }
            if let Some(e) = ar.element_mut(el) {
                e.ns_decls = keep;
            }
        }
    }
    declare_dangling(ar, n);
}

/// True if an attribute in the subtree at `el` uses declaration `d`.
fn subtree_attr_uses(ar: &Arena, el: NodeId, d: &NsDecl) -> bool {
    subtree_elements(ar, el).into_iter().any(|e| {
        ar.attrs(e).any(|at| {
            ar.qname(at)
                .is_some_and(|q| q.prefix == d.prefix && q.ns.as_deref() == Some(&*d.uri))
        })
    })
}

/// Renames the names of the subtree at `el` bound by declaration `d` (on
/// `el`) to prefix `to`, stopping where `d`'s prefix is redeclared.
fn repoint(ar: &mut Arena, el: NodeId, d: &NsDecl, to: Option<goro_xml::Str>) {
    let mut stack = vec![el];
    while let Some(x) = stack.pop() {
        if x != el
            && ar
                .element(x)
                .is_some_and(|e| e.ns_decls.iter().any(|dd| dd.prefix == d.prefix))
        {
            continue;
        }
        for y in ns_users(ar, x) {
            if let Some(q) = ar.data_mut_qname(y)
                && q.prefix == d.prefix
                && q.ns.as_deref() == Some(&*d.uri)
            {
                q.prefix = to.clone();
            }
        }
        stack.extend(ar.children(x).filter(|&c| ar.is_element(c)).collect::<Vec<_>>());
    }
    ar.touch();
}

/// Declares, on their own element, the namespaces of names in the subtree
/// at `root` that nothing in scope binds (top-down, so descendants reuse
/// the new declarations).
pub(crate) fn declare_dangling(ar: &mut Arena, root: NodeId) {
    if ar.is_modern(ar.doc_of(root)) {
        return;
    }
    for el in subtree_elements(ar, root) {
        for x in ns_users(ar, el) {
            let q = ar.qname(x).unwrap().clone();
            if bound_at(ar, el, &q) {
                continue;
            }
            let uri = q.ns.clone().unwrap();
            match q.prefix.as_deref() {
                Some(p) if ar.lookup_ns(el, Some(p)).is_none()
                    || !ar
                        .element(el)
                        .is_some_and(|e| e.ns_decls.iter().any(|d| d.prefix.as_deref() == Some(p))) =>
                {
                    let e = ar.element_mut(el).unwrap();
                    e.ns_decls.retain(|d| d.prefix.as_deref() != Some(p));
                    e.ns_decls.push(NsDecl {
                        prefix: Some(p.into()),
                        uri,
                    });
                }
                None if x == el => {
                    ar.declare_ns(el, None, &uri);
                }
                _ => {
                    let p = generated_prefix(ar, el);
                    ar.declare_ns(el, Some(&p), &uri);
                    if let Some(qq) = ar.data_mut_qname(x) {
                        qq.prefix = Some(p.as_str().into());
                    }
                }
            }
        }
    }
}

/// A prefix bound to `href` in scope at `at` (for attributes, only a
/// prefixed binding counts).
fn search_by_href(ar: &Arena, at: NodeId, href: &str, attr: bool) -> Option<Option<goro_xml::Str>> {
    let mut cur = Some(at);
    while let Some(c) = cur {
        if let Some(e) = ar.element(c) {
            for d in &e.ns_decls {
                if &*d.uri == href
                    && (d.prefix.is_some() || !attr)
                    && ar.lookup_ns(at, d.prefix.as_deref()).as_deref() == Some(href)
                {
                    return Some(d.prefix.clone());
                }
            }
        }
        cur = ar.parent(c).filter(|&p| ar.is_element(p));
    }
    None
}

/// libxml's reconciliation of a subtree after `setAttributeNS()` declared
/// a namespace on `tree` (observed on PHP 8.5, fixture domops.out): every
/// namespaced name below is looked up from `tree` upwards; those not found
/// get a declaration on `tree` (with their prefix when free, else
/// "default", "default1"...; default namespaces always get one).
pub(crate) fn reconcile_tree(ar: &mut Arena, tree: NodeId) {
    if !ar.is_element(tree) || ar.is_modern(ar.doc_of(tree)) {
        return;
    }
    let mut cache: Vec<((Option<goro_xml::Str>, goro_xml::Str), Option<goro_xml::Str>)> = Vec::new();
    for el in subtree_elements(ar, tree) {
        for x in ns_users(ar, el) {
            let q = ar.qname(x).unwrap().clone();
            let Some(ns) = q.ns.clone() else {
                continue;
            };
            if q.prefix.as_deref() == Some("xml") || &*ns == XMLNS_NS {
                continue;
            }
            // Bound from the top of the tree already: nothing to do.
            if ar.lookup_ns(tree, q.prefix.as_deref()).as_deref() == Some(&*ns) {
                continue;
            }
            let is_attr = x != el;
            let key = (q.prefix.clone(), ns.clone());
            let newp = if let Some((_, p)) = cache.iter().find(|(k, _)| *k == key) {
                p.clone()
            } else if let Some(p) = search_by_href(ar, tree, &ns, is_attr) {
                cache.push((key, p.clone()));
                p
            } else {
                let want = q.prefix.as_deref();
                let p = match want {
                    Some(w) if ar.lookup_ns(tree, Some(w)).is_none() => w.to_string(),
                    _ => generated_prefix(ar, tree),
                };
                ar.declare_ns(tree, Some(&p), &ns);
                let p: goro_xml::Str = p.as_str().into();
                cache.push((key, Some(p.clone())));
                Some(p)
            };
            if newp != q.prefix
                && let Some(qq) = ar.data_mut_qname(x)
            {
                qq.prefix = newp;
            }
        }
    }
    ar.touch();
}

/// After a legacy import: namespaces used in the copy but declared outside
/// it are declared where they are used.
pub(crate) fn reconcile_imported(ar: &mut Arena, root: NodeId, doc: NodeId) {
    // An imported attribute whose prefix the destination's document
    // element binds to another namespace gets a generated prefix
    // (observed on PHP 8.5, test
    // DOMDocument_importNode_attribute_prefix_conflict.phpt).
    if let Some(q) = ar.attr(root).map(|d| d.name.clone())
        && let (Some(p), Some(ns)) = (&q.prefix, &q.ns)
        && let Some(de) = ar.document_element(doc)
        && ar.lookup_ns(de, Some(p)).is_some_and(|u| u != *ns)
    {
        let g = match search_by_href(ar, de, ns, true) {
            Some(Some(x)) => x,
            _ => generated_prefix(ar, de).as_str().into(),
        };
        ar.attr_mut(root).unwrap().name.prefix = Some(g);
        return;
    }
    declare_dangling(ar, root);
}

// ---------------------------------------------------------------- queries

/// Arguments of the lookup methods must not contain NUL bytes (test
/// gh16465.phpt).
fn no_nul(vm: &mut Vm, a: &Args, i: usize, name: &str) -> R<()> {
    if a.str(i).as_bytes().contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #{} (${name}) must not contain any null bytes", i + 1),
        );
    }
    Ok(())
}

pub(crate) fn get_elements_by_tag_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    no_nul(vm, a, 0, "qualifiedName")?;
    let n = this_node(vm, a)?;
    let name = s8(&a.str(0));
    let modern = modern_fn(a);
    super::lists::new_list_owned(vm, ListKind::TagName(n, name), modern, Some(a.this_obj().clone()))
}

pub(crate) fn get_elements_by_tag_name_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    no_nul(vm, a, 1, "localName")?;
    no_nul(vm, a, 0, "namespace")?;
    let n = this_node(vm, a)?;
    let ns = a.opt_str(0).map(|s| s8(&s));
    let local = s8(&a.str(1));
    let modern = modern_fn(a);
    super::lists::new_list_owned(vm, ListKind::TagNameNS(n, ns, local), modern, Some(a.this_obj().clone()))
}

/// `insertAdjacent*` positions.
fn adjacent_where(vm: &mut Vm, a: &Args) -> R<Option<u8>> {
    let w = match a.val(0) {
        Value::Object(o) => {
            // Dom\AdjacentPosition: its backing value.
            let v = vm.get_prop_raw(&o, b"value");
            v.as_str().map(s8).unwrap_or_default()
        }
        v => s8(&vm.to_php_str(&v)?),
    };
    Ok(match w.to_ascii_lowercase().as_str() {
        "beforebegin" => Some(0),
        "afterbegin" => Some(1),
        "beforeend" => Some(2),
        "afterend" => Some(3),
        _ => None,
    })
}

/// Inserts `node` at an adjacent position of `el` (None: no parent).
fn insert_adjacent(el: NodeId, node: NodeId, w: u8, modern: bool) -> Result<bool, DomError> {
    let (parent, before) = with_arena(|ar| match w {
        0 => (ar.parent(el), Some(el)),
        1 => (Some(el), ar.first_child(el)),
        2 => (Some(el), None),
        _ => (ar.parent(el), ar.next_sibling(el)),
    });
    let Some(p) = parent else {
        return Ok(false);
    };
    if matches!(w, 0 | 3) && with_arena(|ar| matches!(ar.data(p), NodeData::Document(_))) && !modern
    {
        return Ok(false);
    }
    let r = if modern {
        modern_insert(p, node, before).map(|_| ())
    } else {
        legacy_insert(p, node, before).map(|_| ()).map_err(|e| match e {
            LegacyInsertError::Dom(d) => d,
            _ => DomError::HierarchyRequest,
        })
    };
    r.map(|_| true)
}

fn insert_adjacent_element(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let node = arg_node(vm, a, 1)?;
    let modern = modern_fn(a);
    let Some(w) = adjacent_where(vm, a)? else {
        return throw_dom(vm, DomError::Syntax);
    };
    match insert_adjacent(el, node, w, modern) {
        Ok(true) => Ok(a.val(1)),
        Ok(false) => Ok(Value::Null),
        Err(e) => throw_dom(vm, e),
    }
}

fn insert_adjacent_text(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let el = this_node(vm, a)?;
    let modern = modern_fn(a);
    let Some(w) = adjacent_where(vm, a)? else {
        return throw_dom(vm, DomError::Syntax);
    };
    let text = s8(&a.str(1));
    let t = with_arena(|ar| {
        let d = ar.doc_of(el);
        ar.new_text(d, text)
    });
    let hold = xmlstore::with(|s| s.hold(t));
    let r = insert_adjacent(el, t, w, modern);
    drop(hold);
    match r {
        Ok(_) => Ok(Value::Null),
        Err(e) => throw_dom(vm, e),
    }
}

// ------------------------------------------------ ParentNode / ChildNode

/// The nodes of a variadic `Node|string ...$nodes` argument: strings
/// become text nodes. With `into_fragment`, several nodes are gathered in
/// a fragment (DOM Standard "convert nodes into a node").
fn convert_nodes(vm: &mut Vm, a: &Args, ctx: NodeId) -> R<(Vec<NodeId>, Vec<super::xmlstore::NodeHold>)> {
    let mut out = Vec::new();
    let mut holds = Vec::new();
    let doc = with_arena(|ar| ar.doc_of(ctx));
    let modern = modern_fn(a);
    for i in 0..a.len() {
        match a.val(i) {
            Value::Object(o) => {
                let Some(n) = node_of(&o) else {
                    let base = if modern { "Dom\\Node" } else { "DOMNode" };
                    let root = vm.find_class(base.as_bytes(), false)?.unwrap();
                    let cls = vm.class_of(&o);
                    if cls.is_subclass_of(&root) {
                        return throw_dom(vm, DomError::InvalidState);
                    }
                    return vm.fthrow(
                        a,
                        "TypeError",
                        &format!(
                            "Argument #{} must be of type {base}|string, {} given",
                            i + 1,
                            cls.name.to_string_lossy()
                        ),
                    );
                };
                let is_attr = with_arena(|ar| matches!(ar.data(n), NodeData::Attribute(_) | NodeData::Namespace(_)));
                if is_attr {
                    return throw_dom(vm, DomError::HierarchyRequest);
                }
                out.push(n);
            }
            v => {
                if !matches!(v, Value::Str(_)) {
                    let base = if modern { "Dom\\Node" } else { "DOMNode" };
                    return vm.fthrow(
                        a,
                        "TypeError",
                        &format!(
                            "Argument #{} must be of type {base}|string, {} given",
                            i + 1,
                            crate::class::TypeName::tname(&v)
                        ),
                    );
                }
                let s = vm.to_php_str(&v)?;
                let t = with_arena(|ar| ar.new_text(doc, s8(&s)));
                holds.push(xmlstore::with(|st| st.hold(t)));
                out.push(t);
            }
        }
    }
    Ok((out, holds))
}

/// True if node `n` may enter document `doc` from a legacy method (same
/// document, or made without one).
fn same_doc(n: NodeId, doc: NodeId) -> bool {
    xmlstore::with(|s| {
        let nd = s.arena.doc_of(n);
        nd == doc
            || s
                .docs
                .get(&nd)
                .and_then(|d| d.peek::<super::DocSettings>())
                .is_some_and(|d| d.hidden)
    })
}

/// Inserts nodes before `child` of `parent` (DOM Standard "convert nodes
/// into a node": several nodes first move into a fragment, one by one, so
/// an error on a later one leaves the earlier ones there; tests
/// gh11830/*.phpt, DOMElement_append_hierarchy_test.phpt).
fn insert_nodes(
    vm: &mut Vm,
    a: &Args,
    parent: NodeId,
    nodes: &[NodeId],
    child: Option<NodeId>,
) -> R<()> {
    let modern = modern_fn(a);
    if nodes.is_empty() {
        return Ok(());
    }
    let doc = with_arena(|ar| ar.doc_of(parent));
    let (node, _fh) = if nodes.len() == 1 {
        if !modern && !same_doc(nodes[0], doc) {
            return throw_dom(vm, DomError::WrongDocument);
        }
        (nodes[0], None)
    } else {
        let frag = with_arena(|ar| ar.create(doc, NodeData::Fragment));
        let fh = xmlstore::with(|s| s.hold(frag));
        for &n in nodes {
            if !modern && !same_doc(n, doc) {
                return throw_dom(vm, DomError::WrongDocument);
            }
            let r = xmlstore::with(|s| {
                let ar = &mut s.arena;
                // A document can't go into the fragment (test
                // modern/spec/append_text_nodes_invalid_hierarchy.phpt).
                if ar.is_inclusive_ancestor(n, frag)
                    || matches!(ar.data(n), NodeData::Document(_) | NodeData::Attribute(_))
                {
                    return Err(DomError::HierarchyRequest);
                }
                let old_root = ar.root(n);
                ar.detach(n);
                ar.adopt(n, doc);
                ar.raw_append(frag, n);
                if old_root != n && !matches!(ar.data(old_root), NodeData::Document(_)) {
                    s.free_if_unheld(old_root);
                }
                s.collect();
                Ok(())
            });
            if let Err(e) = r {
                return throw_dom(vm, e);
            }
        }
        (frag, Some(fh))
    };
    let r = if modern {
        modern_insert(parent, node, child).map(|_| ())
    } else {
        match legacy_insert(parent, node, child) {
            Ok(_) => Ok(()),
            Err(LegacyInsertError::Dom(d)) => Err(d),
            Err(LegacyInsertError::EmptyFragment) => Ok(()),
            Err(LegacyInsertError::Attr) => Err(DomError::HierarchyRequest),
        }
    };
    match r {
        Ok(()) => Ok(()),
        Err(e) => throw_dom(vm, e),
    }
}

pub(crate) fn append(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p = this_node(vm, a)?;
    let (nodes, _h) = convert_nodes(vm, a, p)?;
    insert_nodes(vm, a, p, &nodes, None)?;
    Ok(Value::Null)
}

pub(crate) fn prepend(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p = this_node(vm, a)?;
    let (nodes, _h) = convert_nodes(vm, a, p)?;
    let first = with_arena(|ar| ar.first_child(p));
    // Nodes that are the first child move: insert before the first child
    // that is not one of them.
    let first = with_arena(|ar| {
        let mut f = first;
        while let Some(x) = f {
            if !nodes.contains(&x) {
                break;
            }
            f = ar.next_sibling(x);
        }
        f
    });
    insert_nodes(vm, a, p, &nodes, first)?;
    Ok(Value::Null)
}

pub(crate) fn replace_children(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p = this_node(vm, a)?;
    let (nodes, _h) = convert_nodes(vm, a, p)?;
    let bad = with_arena(|ar| nodes.iter().any(|&n| ar.is_inclusive_ancestor(n, p)));
    if bad {
        return throw_dom(vm, DomError::HierarchyRequest);
    }
    xmlstore::with(|s| {
        let kids: Vec<NodeId> = s.arena.children(p).filter(|k| !nodes.contains(k)).collect();
        for k in kids {
            s.arena.detach(k);
            s.free_if_unheld(k);
        }
    });
    insert_nodes(vm, a, p, &nodes, None)?;
    Ok(Value::Null)
}

/// The first following sibling of `n` that is not in `nodes`.
fn viable_next(ar: &Arena, n: NodeId, nodes: &[NodeId]) -> Option<NodeId> {
    let mut c = ar.next_sibling(n);
    while let Some(x) = c {
        if !nodes.contains(&x) {
            return Some(x);
        }
        c = ar.next_sibling(x);
    }
    None
}

fn viable_prev(ar: &Arena, n: NodeId, nodes: &[NodeId]) -> Option<NodeId> {
    let mut c = ar.prev_sibling(n);
    while let Some(x) = c {
        if !nodes.contains(&x) {
            return Some(x);
        }
        c = ar.prev_sibling(x);
    }
    None
}

pub(crate) fn before(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let Some(parent) = with_arena(|ar| ar.parent(n)) else {
        return Ok(Value::Null);
    };
    let (nodes, _h) = convert_nodes(vm, a, n)?;
    let prev = with_arena(|ar| viable_prev(ar, n, &nodes));
    let at = match prev {
        Some(p) => with_arena(|ar| ar.next_sibling(p)),
        None => with_arena(|ar| ar.first_child(parent)),
    };
    insert_nodes(vm, a, parent, &nodes, at)?;
    Ok(Value::Null)
}

pub(crate) fn after(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let Some(parent) = with_arena(|ar| ar.parent(n)) else {
        return Ok(Value::Null);
    };
    let (nodes, _h) = convert_nodes(vm, a, n)?;
    let next = with_arena(|ar| viable_next(ar, n, &nodes));
    insert_nodes(vm, a, parent, &nodes, next)?;
    Ok(Value::Null)
}

pub(crate) fn replace_with(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let Some(parent) = with_arena(|ar| ar.parent(n)) else {
        return Ok(Value::Null);
    };
    let (nodes, _h) = convert_nodes(vm, a, n)?;
    let next = with_arena(|ar| viable_next(ar, n, &nodes));
    let bad = with_arena(|ar| nodes.iter().any(|&x| ar.is_inclusive_ancestor(x, parent)));
    if bad {
        return throw_dom(vm, DomError::HierarchyRequest);
    }
    if !nodes.contains(&n) {
        xmlstore::with(|s| {
            s.arena.detach(n);
        });
    }
    insert_nodes(vm, a, parent, &nodes, next)?;
    Ok(Value::Null)
}

pub(crate) fn remove(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    if !modern_fn(a) && with_arena(|ar| ar.parent(n).is_none()) {
        return legacy_error(vm, a, n, DomError::NotFound);
    }
    with_arena(|ar| {
        if ar.parent(n).is_some() && !matches!(ar.data(n), NodeData::Attribute(_)) {
            ar.detach(n);
        }
    });
    Ok(Value::Null)
}

// ------------------------------------------------------------ fragments

fn append_xml(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = this_node(vm, a)?;
    let data = a.str(0);
    if data.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($data) must not be empty");
    }
    let modern = modern_fn(a);
    if !modern && super::with_settings(f, |s| s.hidden) {
        return throw_dom(vm, DomError::NoModificationAllowed);
    }
    let (errs, ok) = xmlstore::with(|s| {
        let opts = goro_xml::xml::ParseOptions {
            modern,
            ..Default::default()
        };
        let (top, errs, ok) = goro_xml::xml::parse_fragment(&mut s.arena, f, data.as_bytes(), &opts);
        if !ok {
            // Nothing is added from content that is not well-formed.
            for n in top {
                if s.arena.is_live(n) {
                    s.arena.detach(n);
                    s.free_if_unheld(n);
                }
            }
        }
        (errs, ok)
    });
    let fname = a.fname();
    super::super::libxml::report(
        vm,
        &fname,
        &errs,
        super::super::libxml::Style::Dom,
        0,
        Some(data.as_bytes()),
    )?;
    Ok(Value::Bool(ok))
}

/// Kept for callers that expect namespace-free names.
pub(crate) fn html_doc(ar: &Arena, n: NodeId) -> bool {
    ar.doc(ar.doc_of(n)).kind == DocKind::Html5
}

/// Throws a DOMException message (used by the `Dom\` setters).
pub(crate) fn dom_msg<T>(vm: &mut Vm, msg: &str, code: i64) -> R<T> {
    throw_dom_msg(vm, msg, code)
}

/// The value of an optional string as a PHP value.
pub(crate) fn opt_sv(v: Option<String>) -> Value {
    sv_opt(v.as_deref())
}

/// Wraps an optional node.
pub(crate) fn wrap_maybe(vm: &mut Vm, n: Option<NodeId>) -> R<Value> {
    wrap_opt(vm, n)
}
