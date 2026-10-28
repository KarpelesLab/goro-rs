//! DOMElement / Dom\Element: attributes (legacy libxml-style namespace
//! handling for `DOM*`, the DOM Standard's for `Dom\`), element queries,
//! and the ParentNode / ChildNode mixins shared by elements, documents,
//! fragments and character data (PHP manual: DOMElement, DOMParentNode,
//! DOMChildNode; WHATWG DOM; observed on PHP 8.5).

use goro_runtime::{PhpArray, Value};
use goro_xml::tree::{DocKind, DomError, NodeData, NsDecl, QName, XMLNS_NS};
use goro_xml::{Arena, NodeId};

use super::lists::ListKind;
use super::node::{LegacyInsertError, legacy_insert, modern_insert};
use super::xmlstore;
use super::{
    arg_node, legacy_error, modern_fn, node_of, s8, sv, sv_opt, this_node, throw_dom,
    throw_dom_msg, with_arena, wrap_node, wrap_opt,
};
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
        Some(_) => match super::document::validate_and_extract(ns.as_deref(), &name) {
            Ok(q) => q,
            Err(e) => return throw_dom(vm, e),
        },
        None => {
            if !goro_xml::chars::is_name(&name) {
                return throw_dom(vm, DomError::InvalidCharacter);
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
                return e.ns_decls.len() != before;
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
    let q = match super::document::validate_and_extract(ns.as_deref(), &qn) {
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
        let prefix = match &q.prefix {
            Some(p) => {
                match ar.lookup_ns(el, Some(p)) {
                    Some(bound) if bound == uri => {}
                    Some(_) => {
                        // The prefix is bound elsewhere: redeclare here.
                        let e = ar.element_mut(el).unwrap();
                        if e.ns_decls.iter().any(|d| d.prefix.as_deref() == Some(p)) {
                            return Err(DomError::Namespace);
                        }
                        e.ns_decls.push(NsDecl {
                            prefix: Some(p.clone()),
                            uri: uri.clone(),
                        });
                    }
                    None => {
                        ar.declare_ns(el, Some(p), &uri);
                    }
                }
                Some(p.clone())
            }
            None => {
                // libxml binds unprefixed namespaced attributes to a
                // default declaration on the element (observed on PHP
                // 8.5, fixture domops.out "createElementNS").
                let has_default = ar
                    .element(el)
                    .is_some_and(|e| e.ns_decls.iter().any(|d| d.prefix.is_none()));
                if !has_default && ar.lookup_ns(el, None).is_none() {
                    ar.declare_ns(el, None, &uri);
                    None
                } else if ar.lookup_ns(el, None).as_deref() == Some(&*uri) {
                    None
                } else {
                    match ar.lookup_prefix(el, &uri) {
                        Some(p) => Some(p),
                        None => {
                            let p = generated_prefix(ar, el);
                            ar.declare_ns(el, Some(&p), &uri);
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
) -> R<Value> {
    let r = xmlstore::with(|s| {
        let ar = &mut s.arena;
        if let Some(p) = ar.parent(at) {
            if p == el {
                return Ok(Some(at));
            }
            return Err(DomError::InUseAttribute);
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
        let old = if q.ns.is_some() {
            ar.get_attr_node_ns(el, q.ns.as_deref(), &q.local)
        } else {
            ar.attrs(el).find(|&x| ar.qname(x).is_some_and(|k| k.ns.is_none() && k.local == q.local && k.prefix == q.prefix))
        };
        if let Some(o) = old {
            if o == at {
                return Ok(Some(at));
            }
            ar.detach(o);
        }
        if let Some(uri) = &q.ns
            && !modern
            && &**uri != XMLNS_NS
            && q.prefix.as_deref() != Some("xml")
        {
            let p = q.prefix.as_deref();
            if ar.lookup_ns(el, p).as_deref() != Some(&**uri) {
                ar.declare_ns(el, p, uri);
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
    set_attribute_node_raw(vm, a, el, at, modern)
}

fn set_id(ar: &mut Arena, at: NodeId, is_id: bool) {
    if let Some(d) = ar.attr_mut(at) {
        d.is_id = is_id;
    }
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
        let n = if modern && lower_names(ar, el) {
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

/// After a legacy insertion: namespace declarations of the inserted node
/// that its new parent already provides go away (libxml-style
/// reconciliation; observed on PHP 8.5, fixture domops.out).
pub(crate) fn reconcile_after_insert(ar: &mut Arena, n: NodeId) {
    if !ar.is_element(n) || ar.is_modern(ar.doc_of(n)) {
        return;
    }
    let Some(parent) = ar.parent(n) else {
        return;
    };
    if !ar.is_element(parent) {
        return;
    }
    let decls = ar.element(n).map(|e| e.ns_decls.clone()).unwrap_or_default();
    let mut keep = Vec::new();
    for d in decls {
        let bound = ar.lookup_ns(parent, d.prefix.as_deref());
        if bound.as_deref() == Some(&*d.uri) {
            continue;
        }
        keep.push(d);
    }
    if let Some(e) = ar.element_mut(n) {
        e.ns_decls = keep;
    }
}

/// After a legacy import: namespaces used in the copy but declared outside
/// it are declared on the copy's root.
pub(crate) fn reconcile_imported(ar: &mut Arena, root: NodeId) {
    if !ar.is_element(root) {
        return;
    }
    let mut stack = vec![root];
    let mut needed: Vec<(Option<goro_xml::Str>, goro_xml::Str)> = Vec::new();
    while let Some(x) = stack.pop() {
        if let Some(e) = ar.element(x) {
            if let Some(ns) = &e.name.ns {
                needed.push((e.name.prefix.clone(), ns.clone()));
            }
            for at in ar.attrs(x) {
                let q = ar.qname(at).unwrap();
                if let (Some(ns), Some(p)) = (&q.ns, &q.prefix)
                    && &**p != "xml"
                {
                    needed.push((Some(p.clone()), ns.clone()));
                }
            }
            stack.extend(ar.children(x));
        }
    }
    for (p, ns) in needed {
        // Declared on the path from the node to the root of the copy?
        if ar.lookup_ns(root, p.as_deref()).as_deref() == Some(&*ns) {
            continue;
        }
        let declared_inside = {
            let mut found = false;
            let mut st = vec![root];
            while let Some(x) = st.pop() {
                if let Some(e) = ar.element(x) {
                    if e.ns_decls.iter().any(|d| d.prefix == p && d.uri == ns) {
                        found = true;
                        break;
                    }
                    st.extend(ar.children(x));
                }
            }
            found
        };
        if !declared_inside {
            ar.declare_ns(root, p.as_deref(), &ns);
        }
    }
}

// ---------------------------------------------------------------- queries

pub(crate) fn get_elements_by_tag_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let name = s8(&a.str(0));
    let modern = modern_fn(a);
    super::lists::new_list(vm, ListKind::TagName(n, name), modern)
}

pub(crate) fn get_elements_by_tag_name_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let ns = a.opt_str(0).map(|s| s8(&s));
    let local = s8(&a.str(1));
    let modern = modern_fn(a);
    super::lists::new_list(vm, ListKind::TagNameNS(n, ns, local), modern)
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
                    let c = vm.class_of(&o).name.to_string_lossy();
                    return vm.throw("Error", &format!("Couldn't fetch {c}"));
                };
                if !modern {
                    let wrong = xmlstore::with(|s| {
                        let nd = s.arena.doc_of(n);
                        nd != doc
                            && !s
                                .docs
                                .get(&nd)
                                .and_then(|d| d.peek::<super::DocSettings>())
                                .is_some_and(|d| d.hidden)
                    });
                    if wrong {
                        return throw_dom(vm, DomError::WrongDocument);
                    }
                }
                let is_attr = with_arena(|ar| matches!(ar.data(n), NodeData::Attribute(_) | NodeData::Namespace(_)));
                if is_attr {
                    return throw_dom(vm, DomError::HierarchyRequest);
                }
                out.push(n);
            }
            v => {
                let s = vm.to_php_str(&v)?;
                let t = with_arena(|ar| ar.new_text(doc, s8(&s)));
                holds.push(xmlstore::with(|st| st.hold(t)));
                out.push(t);
            }
        }
    }
    Ok((out, holds))
}

/// Inserts nodes before `child` of `parent`, as one operation.
fn insert_nodes(
    vm: &mut Vm,
    a: &Args,
    parent: NodeId,
    nodes: &[NodeId],
    child: Option<NodeId>,
) -> R<()> {
    let modern = modern_fn(a);
    // Validate first: none may be an inclusive ancestor of the parent.
    let bad = with_arena(|ar| {
        nodes.iter().any(|&n| ar.is_inclusive_ancestor(n, parent))
    });
    if bad {
        return throw_dom(vm, DomError::HierarchyRequest);
    }
    if modern || with_arena(|ar| matches!(ar.data(parent), NodeData::Document(_))) {
        // Gather into a fragment so validity is checked once.
        let doc = with_arena(|ar| ar.doc_of(parent));
        let frag = with_arena(|ar| ar.create(doc, NodeData::Fragment));
        let fh = xmlstore::with(|s| s.hold(frag));
        for &n in nodes {
            let r = with_arena(|ar| {
                if let Some(p) = ar.parent(n) {
                    let _ = ar.remove_child(p, n);
                }
                ar.adopt(n, doc);
                ar.raw_append(frag, n);
            });
            let _ = r;
        }
        let r = if modern {
            modern_insert(parent, frag, child).map(|_| ())
        } else {
            legacy_insert(parent, frag, child).map(|_| ()).map_err(|e| match e {
                LegacyInsertError::Dom(d) => d,
                _ => DomError::HierarchyRequest,
            })
        };
        drop(fh);
        xmlstore::with(|s| s.collect());
        return match r {
            Ok(()) => Ok(()),
            Err(LegacyErrorEmpty) if false => Ok(()),
            Err(e) => {
                let _ = LegacyErrorEmpty;
                throw_dom(vm, e)
            }
        };
    }
    for &n in nodes {
        match legacy_insert(parent, n, child) {
            Ok(_) => {}
            Err(LegacyInsertError::Dom(e)) => return throw_dom(vm, e),
            Err(LegacyInsertError::EmptyFragment) => {}
            Err(LegacyInsertError::Attr) => return throw_dom(vm, DomError::HierarchyRequest),
        }
    }
    Ok(())
}

#[allow(non_upper_case_globals)]
const LegacyErrorEmpty: DomError = DomError::HierarchyRequest;

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
    let (_, errs, ok) = with_arena(|ar| {
        let opts = goro_xml::xml::ParseOptions {
            modern,
            ..Default::default()
        };
        goro_xml::xml::parse_fragment(ar, f, data.as_bytes(), &opts)
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
