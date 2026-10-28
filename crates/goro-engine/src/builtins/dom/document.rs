//! DOMDocument, Dom\Document, Dom\XMLDocument, Dom\HTMLDocument and
//! DOMImplementation: node factories, loading and saving (PHP manual:
//! DOMDocument; RFC "DOM HTML5 parsing and serialization"; observed on PHP
//! 8.5).

use goro_runtime::{ObjRef, PhpStr, Value};
use goro_xml::NodeId;
use goro_xml::tree::{DocKind, DomError, NodeData, QName, XMLNS_NS, XML_NS};
use goro_xml::xml::{ParseOptions, opt};

use super::super::libxml::{self, PhpLoader, Style};
use super::xmlstore;
use super::{
    DocSettings, arg_node, legacy_error, modern_fn, node_of, opt_arg_node, s8, settings, sv,
    this_node, throw_dom, throw_dom_msg, with_arena, with_settings, wrap_node, wrap_opt,
};
use NameCheck as NC;
use crate::func::{Args, NativeFn};
use crate::vm::{R, Unwind, Vm};

pub const METHODS: &[(&str, NativeFn)] = &[
    ("domdocument::__construct", construct),
    ("domdocument::createelement", create_element),
    ("domdocument::createelementns", create_element_ns),
    ("domdocument::createtextnode", create_text_node),
    ("domdocument::createcomment", create_comment),
    ("domdocument::createcdatasection", create_cdata),
    ("domdocument::createprocessinginstruction", create_pi),
    ("domdocument::createattribute", create_attribute),
    ("domdocument::createattributens", create_attribute_ns),
    ("domdocument::createdocumentfragment", create_fragment),
    ("domdocument::createentityreference", create_entity_ref),
    ("domdocument::getelementbyid", get_element_by_id),
    ("domdocument::getelementsbytagname", super::element::get_elements_by_tag_name),
    ("domdocument::getelementsbytagnamens", super::element::get_elements_by_tag_name_ns),
    ("domdocument::importnode", import_node),
    ("domdocument::adoptnode", adopt_node),
    ("domdocument::load", load),
    ("domdocument::loadxml", load_xml),
    ("domdocument::loadhtml", load_html),
    ("domdocument::loadhtmlfile", load_html_file),
    ("domdocument::save", save),
    ("domdocument::savexml", save_xml),
    ("domdocument::savehtml", save_html),
    ("domdocument::savehtmlfile", save_html_file),
    ("domdocument::normalizedocument", normalize_document),
    ("domdocument::registernodeclass", register_node_class),
    ("domdocument::validate", validate),
    ("domdocument::xinclude", xinclude),
    ("domdocument::schemavalidate", schema_validate),
    ("domdocument::schemavalidatesource", schema_validate),
    ("domdocument::relaxngvalidate", schema_validate),
    ("domdocument::relaxngvalidatesource", schema_validate),
    ("domdocument::append", super::element::append),
    ("domdocument::prepend", super::element::prepend),
    ("domdocument::replacechildren", super::element::replace_children),
    ("dom\\document::createelement", create_element),
    ("dom\\document::createelementns", create_element_ns),
    ("dom\\document::createtextnode", create_text_node),
    ("dom\\document::createcomment", create_comment),
    ("dom\\document::createcdatasection", create_cdata),
    ("dom\\document::createprocessinginstruction", create_pi),
    ("dom\\document::createattribute", create_attribute),
    ("dom\\document::createattributens", create_attribute_ns),
    ("dom\\document::createdocumentfragment", create_fragment),
    ("dom\\document::getelementbyid", get_element_by_id),
    ("dom\\document::getelementsbytagname", super::element::get_elements_by_tag_name),
    ("dom\\document::getelementsbytagnamens", super::element::get_elements_by_tag_name_ns),
    ("dom\\document::getelementsbyclassname", super::modern::get_elements_by_class_name),
    ("dom\\document::importnode", import_node),
    ("dom\\document::adoptnode", adopt_node),
    ("dom\\document::registernodeclass", register_node_class),
    ("dom\\document::schemavalidate", schema_validate),
    ("dom\\document::schemavalidatesource", schema_validate),
    ("dom\\document::relaxngvalidate", schema_validate),
    ("dom\\document::relaxngvalidatesource", schema_validate),
    ("dom\\document::append", super::element::append),
    ("dom\\document::prepend", super::element::prepend),
    ("dom\\document::replacechildren", super::element::replace_children),
    ("dom\\document::importlegacynode", import_legacy_node),
    ("dom\\document::queryselector", super::modern::query_selector),
    ("dom\\document::queryselectorall", super::modern::query_selector_all),
    ("dom\\xmldocument::createempty", xml_create_empty),
    ("dom\\xmldocument::createfromstring", xml_create_from_string),
    ("dom\\xmldocument::createfromfile", xml_create_from_file),
    ("dom\\xmldocument::createentityreference", create_entity_ref),
    ("dom\\xmldocument::validate", validate),
    ("dom\\xmldocument::xinclude", xinclude),
    ("dom\\xmldocument::savexml", modern_save_xml),
    ("dom\\xmldocument::savexmlfile", modern_save_xml_file),
    ("dom\\htmldocument::createempty", html_create_empty),
    ("dom\\htmldocument::createfromstring", html_create_from_string),
    ("dom\\htmldocument::createfromfile", html_create_from_file),
    ("dom\\htmldocument::savexml", modern_save_xml),
    ("dom\\htmldocument::savexmlfile", modern_save_xml_file),
    ("dom\\htmldocument::savehtml", modern_save_html),
    ("dom\\htmldocument::savehtmlfile", modern_save_html_file),
    ("domimplementation::hasfeature", has_feature),
    ("domimplementation::createdocumenttype", impl_create_doctype),
    ("domimplementation::createdocument", impl_create_document),
    ("dom\\implementation::createdocumenttype", impl_create_doctype),
    ("dom\\implementation::createdocument", impl_create_document),
    ("dom\\implementation::createhtmldocument", impl_create_html_document),
    ("domelement::__construct", super::element::construct),
    ("domattr::__construct", attr_construct),
    ("domtext::__construct", text_construct),
    ("domcomment::__construct", comment_construct),
    ("domcdatasection::__construct", cdata_construct),
    ("domprocessinginstruction::__construct", pi_construct),
    ("domentityreference::__construct", entity_ref_construct),
    ("domdocumentfragment::__construct", fragment_construct),
];

// ---------------------------------------------------------- construction

/// A new empty document (legacy or `Dom\`).
pub(crate) fn new_doc(kind: DocKind, modern: bool) -> NodeId {
    with_arena(|a| {
        let d = a.new_document(kind);
        a.doc_mut(d).modern = modern;
        if kind == DocKind::Html5 {
            // HTML documents serialize as standalone XML (observed on PHP
            // 8.5).
            a.doc_mut(d).standalone = 1;
        }
        d
    })
}

/// The hidden document of a node made without one (`new DOMElement()`).
pub(crate) fn hidden_doc() -> NodeId {
    let d = new_doc(DocKind::Xml, false);
    with_settings(d, |s| s.hidden = true);
    d
}

fn construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let version = s8(&a.str(0));
    let encoding = a.opt_str(1).map(|s| s8(&s)).filter(|s| !s.is_empty());
    let d = new_doc(DocKind::Xml, false);
    with_arena(|ar| {
        let dd = ar.doc_mut(d);
        dd.version = Some(if a.has(0) { version } else { "1.0".into() });
        dd.encoding = encoding;
    });
    set_doc_class(vm, &o, d);
    replace_doc(&o, d);
    Ok(Value::Null)
}

/// Records the class of the document object (subclasses of DOMDocument
/// come back as themselves through `ownerDocument`).
fn set_doc_class(vm: &mut Vm, o: &ObjRef, d: NodeId) {
    let cls = vm.class_of(o);
    if !cls.has_flag(crate::class::cflags::INTERNAL) {
        with_settings(d, |s| s.doc_class = Some(cls));
    }
}

/// Points document object `o` at document `d` (load*, constructor),
/// carrying the old document's settings over.
pub(crate) fn replace_doc(o: &ObjRef, d: NodeId) {
    if let Some(old) = node_of(o) {
        if old == d {
            return;
        }
        let st = settings(old);
        with_settings(d, |s| {
            let hidden = s.hidden;
            *s = st;
            s.hidden = hidden;
        });
        xmlstore::with(|s| {
            if s.dom_objs.get(&old).and_then(|w| w.upgrade()).is_some_and(|x| goro_runtime::ObjRef::ptr_eq(&x, o)) {
                s.dom_objs.remove(&old);
            }
        });
    }
    super::attach(o, d);
}

/// The document of a node (`$this` of a document method).
fn this_doc(vm: &mut Vm, a: &Args) -> R<NodeId> {
    let n = this_node(vm, a)?;
    Ok(with_arena(|ar| ar.doc_of(n)))
}

/// Validates an XML name (legacy: Invalid Character Error).
fn check_name(name: &str) -> bool {
    goro_xml::chars::is_name(name)
}

fn create_element(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let name = s8(&a.str(0));
    let modern = modern_fn(a);
    if modern {
        if !check_name(&name) {
            return throw_dom(vm, DomError::InvalidCharacter);
        }
        let html = with_arena(|ar| ar.doc(d).kind == DocKind::Html5);
        let (local, ns) = if html {
            (name.to_ascii_lowercase(), Some(goro_xml::tree::HTML_NS))
        } else {
            (name.clone(), None)
        };
        let e = with_arena(|ar| {
            ar.new_element(
                d,
                QName {
                    prefix: None,
                    local: local.as_str().into(),
                    ns: ns.map(Into::into),
                },
            )
        });
        return wrap_node(vm, e);
    }
    if !check_name(&name) {
        return legacy_error(vm, a, d, DomError::InvalidCharacter);
    }
    let value = a.opt_str(1).map(|s| s8(&s));
    let e = with_arena(|ar| ar.new_element(d, QName::local(&name)));
    if let Some(v) = value.filter(|v| !v.is_empty()) {
        super::node::set_node_value(vm, e, &v, false)?;
    }
    wrap_node(vm, e)
}

/// How names are validated.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NameCheck {
    /// `createElementNS()` & co. of the legacy classes.
    Legacy,
    /// Legacy constructors (`new DOMElement`).
    Ctor,
    /// `Dom\` methods (DOM Standard).
    Modern,
}

/// Validates and extracts a namespace and qualified name (DOM Standard
/// "validate and extract").
pub(crate) fn validate_and_extract(
    ns: Option<&str>,
    qualified: &str,
    mode: NameCheck,
) -> Result<QName, DomError> {
    let has_ns = ns.is_some_and(|s| !s.is_empty());
    match mode {
        // Legacy factories: a namespaced name that is not a QName is a
        // namespace error (test dom_create_element.phpt).
        NameCheck::Legacy if has_ns => {
            if !goro_xml::chars::is_qname(qualified) {
                return Err(DomError::Namespace);
            }
        }
        NameCheck::Modern => {
            if !goro_xml::chars::is_qname(qualified) {
                return Err(DomError::InvalidCharacter);
            }
        }
        _ => {
            if !goro_xml::chars::is_name(qualified) {
                return Err(DomError::InvalidCharacter);
            }
            if !goro_xml::chars::is_qname(qualified) {
                return Err(DomError::Namespace);
            }
        }
    }
    let ns = ns.filter(|s| !s.is_empty());
    let (prefix, local) = match qualified.split_once(':') {
        Some((p, l)) => (Some(p), l),
        None => (None, qualified),
    };
    if prefix.is_some() && ns.is_none() {
        return Err(DomError::Namespace);
    }
    if prefix == Some("xml") && ns != Some(XML_NS) {
        return Err(DomError::Namespace);
    }
    if (qualified == "xmlns" || prefix == Some("xmlns")) && ns != Some(XMLNS_NS) {
        return Err(DomError::Namespace);
    }
    if ns == Some(XMLNS_NS) && qualified != "xmlns" && prefix != Some("xmlns") {
        return Err(DomError::Namespace);
    }
    Ok(QName {
        prefix: prefix.map(Into::into),
        local: local.into(),
        ns: ns.map(Into::into),
    })
}

fn create_element_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let ns = a.opt_str(0).map(|s| s8(&s));
    let qn = s8(&a.str(1));
    let modern = modern_fn(a);
    let q = match validate_and_extract(ns.as_deref(), &qn, if modern { NC::Modern } else { NC::Legacy }) {
        Ok(q) => q,
        Err(e) if modern => return throw_dom(vm, e),
        Err(e) => return legacy_error(vm, a, d, e),
    };
    let value = a.opt_str(2).map(|s| s8(&s));
    let e = with_arena(|ar| {
        let e = ar.new_element(d, q.clone());
        if !modern && let Some(uri) = &q.ns {
            // Legacy elements declare their namespace (libxml-style
            // namespace nodes).
            ar.declare_ns(e, q.prefix.as_deref(), uri);
        }
        e
    });
    if let Some(v) = value.filter(|v| !v.is_empty()) {
        super::node::set_node_value(vm, e, &v, false)?;
    }
    wrap_node(vm, e)
}

fn create_text_node(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let t = s8(&a.str(0));
    let n = with_arena(|ar| ar.new_text(d, t));
    wrap_node(vm, n)
}

fn create_comment(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let t = s8(&a.str(0));
    let n = with_arena(|ar| ar.create(d, NodeData::Comment(t)));
    wrap_node(vm, n)
}

fn create_cdata(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let t = s8(&a.str(0));
    if modern_fn(a) {
        let html = with_arena(|ar| ar.doc(d).kind == DocKind::Html5);
        if html {
            return throw_dom_msg(
                vm,
                "This operation is not supported for HTML documents",
                9,
            );
        }
        if t.contains("]]>") {
            return throw_dom_msg(vm, "Invalid character sequence \"]]>\" in CDATA section", 5);
        }
    }
    let n = with_arena(|ar| ar.create(d, NodeData::CData(t)));
    wrap_node(vm, n)
}

fn create_pi(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let target = s8(&a.str(0));
    let data = s8(&a.str(1));
    let modern = modern_fn(a);
    if !check_name(&target) {
        if modern {
            return throw_dom(vm, DomError::InvalidCharacter);
        }
        return legacy_error(vm, a, d, DomError::InvalidCharacter);
    }
    if modern && data.contains("?>") {
        return throw_dom_msg(vm, "Invalid character sequence \"?>\" in processing instruction", 5);
    }
    let n = with_arena(|ar| {
        ar.create(
            d,
            NodeData::Pi {
                target: target.as_str().into(),
                data,
            },
        )
    });
    wrap_node(vm, n)
}

fn create_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let name = s8(&a.str(0));
    let modern = modern_fn(a);
    if !check_name(&name) {
        if modern {
            return throw_dom(vm, DomError::InvalidCharacter);
        }
        return legacy_error(vm, a, d, DomError::InvalidCharacter);
    }
    let name = if modern && with_arena(|ar| ar.doc(d).kind == DocKind::Html5) {
        name.to_ascii_lowercase()
    } else {
        name
    };
    let n = with_arena(|ar| ar.new_attribute(d, QName::local(&name), ""));
    wrap_node(vm, n)
}

fn create_attribute_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let ns = a.opt_str(0).map(|s| s8(&s));
    let qn = s8(&a.str(1));
    let modern = modern_fn(a);
    let q = match validate_and_extract(ns.as_deref(), &qn, if modern { NC::Modern } else { NC::Legacy }) {
        Ok(q) => q,
        Err(e) if modern => return throw_dom(vm, e),
        Err(e) => return legacy_error(vm, a, d, e),
    };
    if !modern && q.ns.is_some() && q.prefix.is_none() && q.ns.as_deref() != Some(XMLNS_NS) {
        // A namespaced attribute needs a prefix in libxml's model: PHP
        // generates one (test createAttributeNS_prefix_conflicts).
    }
    let n = with_arena(|ar| ar.new_attribute(d, q, ""));
    wrap_node(vm, n)
}

fn create_fragment(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let n = with_arena(|ar| ar.create(d, NodeData::Fragment));
    wrap_node(vm, n)
}

fn create_entity_ref(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let name = s8(&a.str(0));
    let modern = modern_fn(a);
    if !check_name(&name) {
        if modern {
            return throw_dom(vm, DomError::InvalidCharacter);
        }
        return legacy_error(vm, a, d, DomError::InvalidCharacter);
    }
    let n = with_arena(|ar| ar.create(d, NodeData::EntityRef(name.as_str().into())));
    wrap_node(vm, n)
}

fn get_element_by_id(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let id = s8(&a.str(0));
    let modern = modern_fn(a);
    let _ = modern;
    let r = xmlstore::with(|s| {
        let els: Vec<NodeId> = s.arena.descendants(d).filter(|&e| s.arena.is_element(e)).collect();
        els.into_iter().find(|&e| {
            let attrs: Vec<NodeId> = s.arena.attrs(e).collect();
            attrs
                .into_iter()
                .any(|at| super::is_id_attr(s, at) && s.arena.attr_value(at) == id)
        })
    });
    wrap_opt(vm, r)
}

fn import_node(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let modern = modern_fn(a);
    let Some(n) = opt_arg_node(vm, a, 0)? else {
        return vm.throw(
            "TypeError",
            &format!(
                "{}(): Argument #1 ($node) must be of type Dom\\Node, null given",
                a.fname()
            ),
        );
    };
    let deep = a.bool(1);
    let r = with_arena(|ar| match ar.data(n) {
        NodeData::Document(_) => Err(DomError::NotSupported),
        NodeData::Doctype(_) if !modern => Err(DomError::NotSupported),
        _ => Ok(ar.clone_node(n, d, deep)),
    });
    match r {
        Ok(c) => {
            if !modern {
                with_arena(|ar| super::element::reconcile_imported(ar, c, d));
            }
            wrap_node(vm, c)
        }
        Err(e) if modern => throw_dom(vm, e),
        Err(_) => {
            vm.fwarn(a, "Cannot import: Node Type Not Supported")?;
            Ok(Value::Bool(false))
        }
    }
}

fn adopt_node(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let n = arg_node(vm, a, 0)?;
    let modern = modern_fn(a);
    let r = xmlstore::with(|s| {
        let ar = &mut s.arena;
        if matches!(ar.data(n), NodeData::Document(_)) {
            return Err(DomError::NotSupported);
        }
        if ar.parent(n).is_some() {
            if matches!(ar.data(n), NodeData::Attribute(_)) {
                ar.detach(n);
            } else {
                let p = ar.parent(n).unwrap();
                let _ = ar.remove_child(p, n);
            }
        }
        ar.adopt(n, d);
        s.collect();
        Ok(())
    });
    match r {
        Ok(()) => Ok(a.val(0)),
        Err(e) if modern => throw_dom(vm, e),
        Err(e) => legacy_error(vm, a, d, e),
    }
}

fn import_legacy_node(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let n = arg_node(vm, a, 0)?;
    let deep = a.bool(1);
    let c = with_arena(|ar| {
        let c = ar.clone_node(n, d, deep);
        // Legacy namespace declarations become xmlns attributes (test
        // modern/spec/Document_importLegacyNode.phpt).
        let mut els = vec![c];
        els.extend(ar.descendants(c).filter(|&x| ar.is_element(x)));
        for el in els {
            let decls = match ar.element_mut(el) {
                Some(e) => std::mem::take(&mut e.ns_decls),
                None => continue,
            };
            let first = ar.element(el).and_then(|e| e.first_attr);
            for dcl in decls.into_iter().rev() {
                let name = match &dcl.prefix {
                    Some(p) => QName {
                        prefix: Some("xmlns".into()),
                        local: p.clone(),
                        ns: Some(XMLNS_NS.into()),
                    },
                    None => QName {
                        prefix: None,
                        local: "xmlns".into(),
                        ns: Some(XMLNS_NS.into()),
                    },
                };
                let at = ar.new_attribute(d, name, &dcl.uri);
                let cur_first = ar.element(el).and_then(|e| e.first_attr);
                match cur_first {
                    Some(f0) => {
                        let _ = first;
                        ar.raw_append_attr(el, at);
                        // move before the first original attribute
                        ar.detach(at);
                        insert_attr_before(ar, el, at, f0);
                    }
                    None => ar.raw_append_attr(el, at),
                }
            }
        }
        c
    });
    wrap_node(vm, c)
}

/// Inserts attribute `at` (detached) before attribute `before` of `el`.
fn insert_attr_before(ar: &mut goro_xml::Arena, el: NodeId, at: NodeId, before: NodeId) {
    let rest: Vec<NodeId> = {
        let mut v = Vec::new();
        let mut c = Some(before);
        while let Some(x) = c {
            v.push(x);
            c = ar.next_sibling(x);
        }
        v
    };
    for &x in &rest {
        ar.detach(x);
    }
    ar.raw_append_attr(el, at);
    for x in rest {
        ar.raw_append_attr(el, x);
    }
}

fn normalize_document(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    with_arena(|ar| ar.normalize(d));
    Ok(Value::Null)
}

fn register_node_class(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let base = a.str(0);
    let modern = modern_fn(a);
    let Some(bc) = vm.find_class(base.as_bytes(), true)? else {
        return vm.fthrow(
            a,
            "TypeError",
            &format!(
                "Argument #1 ($baseClass) must be a class name derived from {} or null, {} given",
                if modern { "Dom\\Node" } else { "DOMNode" },
                base.to_string_lossy()
            ),
        );
    };
    let root = vm
        .find_class(if modern { b"Dom\\Node" as &[u8] } else { b"DOMNode" }, false)?
        .unwrap();
    if modern && bc.has_flag(crate::class::cflags::ABSTRACT) && bc.is_subclass_of(&root) {
        return vm.fthrow(a, "ValueError", "Argument #1 ($baseClass) must not be an abstract class");
    }
    if !bc.is_subclass_of(&root) || !bc.has_flag(crate::class::cflags::INTERNAL) {
        return vm.fthrow(
            a,
            "TypeError",
            &format!(
                "Argument #1 ($baseClass) must be a class name derived from {}, {} given",
                if modern { "Dom\\Node" } else { "DOMNode" },
                bc.name.to_string_lossy()
            ),
        );
    }
    let ext = match a.val(1) {
        Value::Null => None,
        _ => {
            let en = a.str(1);
            match vm.find_class(en.as_bytes(), true)? {
                Some(c) if c.is_subclass_of(&bc) && !c.has_flag(crate::class::cflags::ABSTRACT) => {
                    Some(c)
                }
                _ => {
                    return vm.fthrow(
                        a,
                        "TypeError",
                        &format!(
                            "Argument #2 ($extendedClass) must be a class name derived from {} or null, {} given",
                            bc.name.to_string_lossy(),
                            en.to_string_lossy()
                        ),
                    );
                }
            }
        }
    };
    let is_doc_class = bc.lname.as_bytes() == b"domdocument";
    with_settings(d, |s| {
        s.class_map.retain(|(b, _)| b != &bc.lname);
        if is_doc_class {
            s.doc_class = ext.clone();
        }
        if let Some(c) = ext {
            s.class_map.push((bc.lname.clone(), c));
        }
    });
    if modern {
        Ok(Value::Null)
    } else {
        Ok(Value::Bool(true))
    }
}

fn schema_validate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let _ = this_doc(vm, a)?;
    vm.fwarn(a, "Schema validation is not supported")?;
    Ok(Value::Bool(false))
}

// ------------------------------------------------------------- loading

/// The parse flags of a document's settings (PHP manual: DOMDocument
/// properties resolveExternals, validateOnParse, substituteEntities,
/// preserveWhiteSpace, recover).
fn settings_flags(st: &DocSettings) -> u32 {
    let mut f = 0;
    if st.resolve_externals {
        f |= opt::DTDLOAD;
    }
    if st.validate_on_parse {
        f |= opt::DTDLOAD | opt::DTDVALID;
    }
    if st.substitute_entities {
        f |= opt::NOENT;
    }
    if !st.preserve_white_space {
        f |= opt::NOBLANKS;
    }
    if st.recover {
        f |= opt::RECOVER;
    }
    f
}

/// The URL of documents loaded from strings: the current directory
/// (observed on PHP 8.5).
fn cwd_url(vm: &Vm) -> String {
    let mut s = vm.cwd.to_string_lossy().into_owned();
    if !s.ends_with('/') {
        s.push('/');
    }
    s
}

/// The absolute URL of a file.
pub(crate) fn file_url(vm: &Vm, path: &str) -> String {
    if path.contains("://") || path.starts_with('/') {
        return path.to_string();
    }
    vm.fs_path(path.as_bytes()).to_string_lossy().into_owned()
}

/// Parses XML into a new document: `Ok(Some(doc))`, or `Ok(None)` after
/// reporting errors of a failed parse.
pub(crate) fn parse_xml(
    vm: &mut Vm,
    fname: &str,
    input: &[u8],
    flags: u32,
    url: Option<String>,
    modern: bool,
    style: Style,
) -> R<Option<NodeId>> {
    let mut opts = ParseOptions::with_flags(flags);
    opts.url = url;
    opts.modern = modern;
    let mut loader = PhpLoader::new(vm, fname);
    loader.nonet = flags & opt::NONET != 0;
    let built = if libxml::has_user_loader(vm) {
        // The loader runs PHP code: parse outside the store.
        let mut tmp = goro_xml::Arena::new();
        let b = goro_xml::xml::build(&mut tmp, input, &opts, Some(&mut loader));
        let doc = xmlstore::with(|s| {
            let d = s.arena.new_document(DocKind::Xml);
            let src = tmp.doc(b.doc).clone();
            *s.arena.doc_mut(d) = goro_xml::tree::DocData { holds: 0, doctype: None, ext_subset: None, ids: Default::default(), ..src };
            let kids: Vec<NodeId> = tmp.children(b.doc).collect();
            for k in kids {
                let c = s.arena.import_from(&tmp, k, d);
                s.arena.raw_append(d, c);
                if matches!(s.arena.data(c), NodeData::Doctype(_)) {
                    s.arena.doc_mut(d).doctype = Some(c);
                }
            }
            if let Some(e) = tmp.doc(b.doc).ext_subset {
                let c = s.arena.import_from(&tmp, e, d);
                s.arena.doc_mut(d).ext_subset = Some(c);
            }
            s.arena.register_ids(d);
            d
        });
        goro_xml::xml::Built {
            doc,
            errors: b.errors,
            ok: b.ok,
        }
    } else {
        xmlstore::with(|s| goro_xml::xml::build(&mut s.arena, input, &opts, Some(&mut loader)))
    };
    loader.finish(vm)?;
    let xinclude = flags & opt::XINCLUDE != 0 && built.ok;
    libxml::report(vm, fname, &built.errors, style, flags, Some(input))?;
    if !built.ok {
        xmlstore::with(|s| s.free_doc_if_unheld(built.doc));
        return Ok(None);
    }
    if xinclude {
        run_xinclude(vm, fname, built.doc, flags)?;
    }
    Ok(Some(built.doc))
}

/// Reads a file for a load method: the bytes, or `None` after PHP's
/// warning.
fn read_for_load(vm: &mut Vm, fname: &str, path: &str) -> R<Option<Vec<u8>>> {
    // Paths longer than PHP_MAXPATHLEN fail silently (test
    // DOMDocument_load_error6.phpt).
    if path.len() > 4096 {
        return Ok(None);
    }
    match libxml::load_document(vm, fname, path)? {
        Ok(b) => Ok(Some(b)),
        Err(Some(msg)) => {
            vm.warn(&format!("{fname}(): {msg}"))?;
            Ok(None)
        }
        Err(None) => {
            let url = file_url(vm, path);
            let e = libxml::io_error(&url);
            libxml::report(vm, fname, &[e], Style::Dom, 0, None)?;
            Ok(None)
        }
    }
}

fn check_path(vm: &mut Vm, a: &Args, i: usize, what: &str) -> R<Option<String>> {
    let s = a.str(i);
    if s.is_empty() {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #{} (${what}) must not be empty", i + 1),
        );
    }
    if s.as_bytes().contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #{} (${what}) must not contain any null bytes", i + 1),
        );
    }
    Ok(Some(s8(&s)))
}

const VALID_FLAGS: i64 = (opt::RECOVER
    | opt::NOENT
    | opt::DTDLOAD
    | opt::DTDATTR
    | opt::DTDVALID
    | opt::NOERROR
    | opt::NOWARNING
    | opt::NOBLANKS
    | opt::XINCLUDE
    | opt::NSCLEAN
    | opt::NOCDATA
    | opt::NONET
    | opt::PEDANTIC
    | opt::COMPACT
    | opt::PARSEHUGE
    | opt::BIGLINES
    | opt::NO_XXE) as i64;

fn check_options(vm: &mut Vm, a: &Args, i: usize) -> R<u32> {
    let o = a.int(i);
    if o < 0 || o > u32::MAX as i64 {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} ($options) is not a valid libxml option",
                i + 1
            ),
        );
    }
    Ok(o as u32)
}

fn load_common(vm: &mut Vm, a: &Args, input: &[u8], flags: u32, url: String, from_string: bool) -> R<Value> {
    let o = a.this_obj().clone();
    let cur = node_of(&o);
    let st = cur.map(settings).unwrap_or_default();
    let flags = flags | settings_flags(&st);
    let fname = a.fname();
    // Strings parse without a URL (errors say "Entity"); the document gets
    // the current directory as its URL (observed on PHP 8.5).
    let purl = if from_string { None } else { Some(url.clone()) };
    let Some(d) = parse_xml(vm, &fname, input, flags, purl, false, Style::Dom)? else {
        return Ok(Value::Bool(false));
    };
    with_arena(|ar| ar.doc_mut(d).url = Some(url));
    if cur.is_none() {
        set_doc_class(vm, &o, d);
    }
    replace_doc(&o, d);
    Ok(Value::Bool(true))
}

fn load_xml(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let src = a.str(0);
    if src.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($source) must not be empty");
    }
    let flags = check_options(vm, a, 1)?;
    let url = cwd_url(vm);
    load_common(vm, a, src.as_bytes(), flags, url, true)
}

fn load(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(path) = check_path(vm, a, 0, "filename")? else {
        return Ok(Value::Bool(false));
    };
    let flags = check_options(vm, a, 1)?;
    let fname = a.fname();
    let Some(bytes) = read_for_load(vm, &fname, &path)? else {
        return Ok(Value::Bool(false));
    };
    let url = file_url(vm, &path);
    load_common(vm, a, &bytes, flags, url, false)
}

fn load_html_common(vm: &mut Vm, a: &Args, input: &[u8], flags: u32, url: Option<String>) -> R<Value> {
    let o = a.this_obj().clone();
    let cur = node_of(&o);
    let fname = a.fname();
    let parsed = xmlstore::with(|s| {
        goro_xml::html::parse_html(&mut s.arena, input, flags, None, url.as_deref())
    });
    libxml::report(vm, &fname, &parsed.errors, Style::Dom, flags, Some(input))?;
    if cur.is_none() {
        set_doc_class(vm, &o, parsed.doc);
    }
    replace_doc(&o, parsed.doc);
    Ok(Value::Bool(true))
}

fn load_html(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let src = a.str(0);
    if src.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($source) must not be empty");
    }
    let flags = check_options(vm, a, 1)?;
    load_html_common(vm, a, src.as_bytes(), flags, None)
}

fn load_html_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(path) = check_path(vm, a, 0, "filename")? else {
        return Ok(Value::Bool(false));
    };
    let flags = check_options(vm, a, 1)?;
    let fname = a.fname();
    let Some(bytes) = read_for_load(vm, &fname, &path)? else {
        return Ok(Value::Bool(false));
    };
    let url = file_url(vm, &path);
    load_html_common(vm, a, &bytes, flags, Some(url))
}

// -------------------------------------------------------------- saving

/// `saveXML()` of a node or the document (legacy).
pub(crate) fn legacy_save(n: NodeId, flags: u32) -> Vec<u8> {
    let format = settings(n).format_output;
    with_arena(|ar| {
        let opts = goro_xml::serialize::SaveOptions::new(format, flags);
        goro_xml::serialize::save_xml(ar, n, &opts)
    })
}

fn save_xml(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let flags = a.int(1) as u32;
    let target = match opt_arg_node(vm, a, 0)? {
        Some(n) => {
            if with_arena(|ar| ar.doc_of(n)) != d {
                return legacy_error(vm, a, d, DomError::WrongDocument);
            }
            n
        }
        None => d,
    };
    let out = if target == d {
        legacy_save(d, flags)
    } else {
        let format = settings(d).format_output;
        with_arena(|ar| {
            let opts = goro_xml::serialize::SaveOptions::new(format, flags);
            let s = goro_xml::serialize::to_string(ar, target, &opts);
            goro_xml::serialize::encode_output(&s, ar.doc(d).encoding.as_deref())
        })
    };
    Ok(Value::Str(PhpStr::from(out)))
}

/// Writes bytes to a file for a save method.
fn write_file(vm: &mut Vm, a: &Args, path: &str, bytes: &[u8]) -> R<Value> {
    let p = vm.fs_path(path.as_bytes());
    let r = if path.contains("://") && !path.starts_with("file://") {
        let f = vm.call_value(
            &Value::from(&b"file_put_contents"[..]),
            vec![sv(path), Value::Str(PhpStr::from(bytes.to_vec()))],
        )?;
        return Ok(f);
    } else {
        std::fs::write(&p, bytes)
    };
    match r {
        Ok(()) => Ok(Value::Int(bytes.len() as i64)),
        Err(_) => {
            let _ = a;
            Ok(Value::Bool(false))
        }
    }
}

fn save(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let Some(path) = check_path(vm, a, 0, "filename")? else {
        return Ok(Value::Bool(false));
    };
    let flags = a.int(1) as u32;
    let out = legacy_save(d, flags);
    write_file(vm, a, &path, &out)
}

pub(crate) fn html_bytes(n: NodeId) -> Vec<u8> {
    let format = settings(n).format_output;
    with_arena(|ar| {
        goro_xml::html::save_html(ar, n, goro_xml::html::HtmlSaveOptions { format })
    })
}

fn save_html(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let target = match opt_arg_node(vm, a, 0)? {
        Some(n) => {
            if with_arena(|ar| ar.doc_of(n)) != d {
                return legacy_error(vm, a, d, DomError::WrongDocument);
            }
            n
        }
        None => d,
    };
    Ok(Value::Str(PhpStr::from(html_bytes(target))))
}

fn save_html_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let Some(path) = check_path(vm, a, 0, "filename")? else {
        return Ok(Value::Bool(false));
    };
    let out = html_bytes(d);
    write_file(vm, a, &path, &out)
}

// ---------------------------------------------------- validation, xinclude

fn validate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let fname = a.fname();
    // Load the external subset when it isn't (validate() needs it).
    let need = with_arena(|ar| {
        let dd = ar.doc(d);
        let dt = dd.doctype?;
        if dd.ext_subset.is_some() {
            return None;
        }
        let dtd = ar.dtd(dt)?;
        dtd.system_id
            .clone()
            .map(|s| (dtd.public_id.clone(), s, dd.url.clone()))
    });
    if let Some((public, system, base)) = need {
        let mut loader = PhpLoader::new(vm, &fname);
        loader.bare = true;
        let r = super::chardata::load_external_subset(vm, d, public.as_deref(), &system, base.as_deref(), &mut loader);
        loader.finish(vm)?;
        if !r {
            let msg = format!("Could not load the external subset \"{system}\"");
            vm.fwarn(a, &msg)?;
            return Ok(Value::Bool(false));
        }
    }
    let errs = with_arena(|ar| goro_xml::xml::validate::validate(ar, d));
    let has_dtd = with_arena(|ar| ar.doc(d).doctype.is_some() || ar.doc(d).ext_subset.is_some());
    if !has_dtd {
        vm.fwarn(a, "no DTD found!")?;
        return Ok(Value::Bool(false));
    }
    let ok = errs.is_empty();
    libxml::report(vm, &fname, &errs, Style::Dom, 0, None)?;
    Ok(Value::Bool(ok))
}

struct XiHost<'a> {
    loader: PhpLoader,
    vm: *mut Vm,
    _p: std::marker::PhantomData<&'a ()>,
}

impl goro_xml::xinclude::XIncludeHost for XiHost<'_> {
    fn load(&mut self, href: &str, base: Option<&str>) -> Result<Vec<u8>, goro_xml::xml::LoadError> {
        goro_xml::xml::EntityLoader::load(&mut self.loader, None, href, base)
    }
    fn xpath(&mut self, arena: &mut goro_xml::Arena, doc: NodeId, expr: &str) -> Option<Vec<NodeId>> {
        let _ = self.vm;
        let e = goro_xml::xpath::compile(expr).ok()?;
        let mut ctx = goro_xml::xpath::Context::new();
        match goro_xml::xpath::evaluate(arena, &e, doc, &mut ctx).ok()? {
            goro_xml::xpath::Value::NodeSet(v) => Some(v),
            _ => None,
        }
    }
}

fn run_xinclude(vm: &mut Vm, fname: &str, d: NodeId, flags: u32) -> R<i64> {
    let loader = PhpLoader::new(vm, fname);
    let mut host = XiHost {
        loader,
        vm: vm as *mut Vm,
        _p: std::marker::PhantomData,
    };
    let (n, errs) = xmlstore::with(|s| goro_xml::xinclude::process(&mut s.arena, d, flags, &mut host));
    host.loader.finish(vm)?;
    libxml::report(vm, fname, &errs, Style::Dom, flags, None)?;
    Ok(n as i64)
}

fn xinclude(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let flags = a.int(0) as u32;
    let fname = a.fname();
    let n = run_xinclude(vm, &fname, d, flags)?;
    // Legacy: false when nothing was substituted (PHP manual:
    // DOMDocument::xinclude).
    if n == 0 && !modern_fn(a) {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Int(n))
}

// ---------------------------------------------------------- Dom\ factories

fn new_modern_doc_object(vm: &mut Vm, a: &Args, d: NodeId) -> R<Value> {
    let _ = a;
    wrap_node(vm, d)
}

fn xml_create_empty(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let version = s8(&a.str(0));
    let enc = s8(&a.str(1));
    let version = if a.has(0) { version } else { "1.0".into() };
    let enc = if a.has(1) { enc } else { "UTF-8".into() };
    if goro_xml::encoding::lookup(&enc).is_none() {
        return vm.fthrow(a, "ValueError", "Argument #2 ($encoding) is not a valid document encoding");
    }
    let d = new_doc(DocKind::Xml, true);
    with_arena(|ar| {
        let dd = ar.doc_mut(d);
        dd.version = Some(version);
        dd.encoding = Some(enc);
    });
    new_modern_doc_object(vm, a, d)
}

fn modern_check_options(vm: &mut Vm, a: &Args, i: usize, html: bool) -> R<u32> {
    let o = a.int(i);
    let allowed: i64 = if html {
        (opt::NOERROR | goro_xml::xml::opt::COMPACT) as i64 | 8192 | 2147483648
    } else {
        VALID_FLAGS & !(opt::NO_XXE as i64)
    };
    if o & !allowed != 0 || o < 0 {
        let list = if html {
            "LIBXML_NOERROR, LIBXML_COMPACT, LIBXML_HTML_NOIMPLIED, Dom\\HTML_NO_DEFAULT_NS"
        } else {
            "LIBXML_RECOVER, LIBXML_NOENT, LIBXML_DTDLOAD, LIBXML_DTDATTR, LIBXML_DTDVALID, LIBXML_NOERROR, LIBXML_NOWARNING, LIBXML_NOBLANKS, LIBXML_XINCLUDE, LIBXML_NSCLEAN, LIBXML_NOCDATA, LIBXML_NONET, LIBXML_PEDANTIC, LIBXML_COMPACT, LIBXML_PARSEHUGE, LIBXML_BIGLINES"
        };
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} ($options) contains invalid flags (allowed flags: {list})",
                i + 1
            ),
        );
    }
    Ok(o as u32)
}

fn modern_parse(vm: &mut Vm, a: &Args, input: &[u8], url: String, from_string: bool) -> R<Value> {
    let flags = modern_check_options(vm, a, 1, false)?;
    let enc = a.opt_str(2).map(|s| s8(&s));
    if let Some(e) = &enc
        && goro_xml::encoding::lookup(e).is_none()
    {
        return vm.fthrow(a, "ValueError", "Argument #3 ($overrideEncoding) is not a valid document encoding");
    }
    let fname = a.fname();
    let mut opts_flags = flags;
    if enc.is_some() {
        opts_flags |= 0;
    }
    let d = {
        let mut opts = ParseOptions::with_flags(opts_flags);
        opts.url = if from_string { None } else { Some(url.clone()) };
        opts.modern = true;
        opts.encoding_override = enc;
        let mut loader = PhpLoader::new(vm, &fname);
        let built = xmlstore::with(|s| goro_xml::xml::build(&mut s.arena, input, &opts, Some(&mut loader)));
        loader.finish(vm)?;
        libxml::report(vm, &fname, &built.errors, Style::Dom, flags, Some(input))?;
        if !built.ok {
            xmlstore::with(|s| s.free_doc_if_unheld(built.doc));
            return throw_dom_msg(vm, "XML fragment is not well-formed", 2);
        }
        if flags & opt::XINCLUDE != 0 {
            run_xinclude(vm, &fname, built.doc, flags)?;
        }
        built.doc
    };
    with_arena(|ar| ar.doc_mut(d).url = Some(url));
    with_arena(|ar| {
        let dd = ar.doc_mut(d);
        if dd.encoding.is_none() {
            dd.encoding = Some("UTF-8".into());
        }
    });
    wrap_node(vm, d)
}

fn xml_create_from_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let src = a.str(0);
    if src.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($source) must not be empty");
    }
    let url = cwd_url(vm);
    modern_parse(vm, a, &src.as_bytes().to_vec(), url, true)
}

fn xml_create_from_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(path) = check_path(vm, a, 0, "path")? else {
        return Ok(Value::Null);
    };
    let fname = a.fname();
    let Some(bytes) = read_for_load(vm, &fname, &path)? else {
        return vm.throw("Exception", &format!("Cannot open file '{path}'"));
    };
    let url = file_url(vm, &path);
    modern_parse(vm, a, &bytes, url, false)
}

fn html_create_empty(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let enc = if a.has(0) { s8(&a.str(0)) } else { "UTF-8".into() };
    let Some(e) = goro_xml::html5::encoding::encoding_for_label(&enc) else {
        return vm.fthrow(a, "ValueError", "Argument #1 ($encoding) is not a valid document encoding");
    };
    let d = new_doc(DocKind::Html5, true);
    with_arena(|ar| ar.doc_mut(d).encoding = Some(e.name().to_string()));
    wrap_node(vm, d)
}

fn html_parse(vm: &mut Vm, a: &Args, input: &[u8], source: &str, url: Option<String>) -> R<Value> {
    let flags = modern_check_options(vm, a, 1, true)? as i64;
    let enc = match a.opt_str(2) {
        Some(e) => match goro_xml::html5::encoding::encoding_for_label(&s8(&e)) {
            Some(x) => Some(x),
            None => {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #3 ($overrideEncoding) must be a valid document encoding",
                );
            }
        },
        None => None,
    };
    let opts = goro_xml::html5::ParseOptions {
        override_encoding: enc,
        transport_encoding: None,
        no_default_ns: flags & 2147483648 != 0,
        no_implied: flags & 8192 != 0,
        collect_errors: flags & opt::NOERROR as i64 == 0,
        standard: false,
    };
    let r = xmlstore::with(|s| goro_xml::html5::parse_document(&mut s.arena, input, &opts));
    let fname = a.fname();
    let text = String::from_utf8_lossy(input);
    // Parse errors are libxml errors of level 2, code 1 (observed on PHP
    // 8.5, test HTMLDocument_fromString_parser_warning_internal_error).
    let _ = &text;
    for e in &r.errors {
        let m = e.message(source);
        let err = goro_xml::error::XmlError {
            domain: goro_xml::error::Domain::Html,
            code: 1,
            level: goro_xml::error::LEVEL_ERROR,
            message: m.clone(),
            file: Some(source.to_string()),
            line: e.line,
            column: e.column,
        };
        if !libxml::record(vm, err) {
            vm.warn(&format!("{fname}(): {m}"))?;
        }
    }
    with_arena(|ar| {
        let dd = ar.doc_mut(r.doc);
        dd.modern = true;
        dd.standalone = 1;
        dd.url = url.clone();
    });
    wrap_node(vm, r.doc)
}

fn html_create_from_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let src = a.str(0);
    html_parse(vm, a, &src.as_bytes().to_vec(), "Entity", None)
}

fn html_create_from_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    // Read through PHP's streams (observed on PHP 8.5, tests
    // modern/html/parser/HTMLDocument_fromFile_*.phpt).
    let p = a.str(0);
    if p.is_empty() {
        return vm.throw("ValueError", "Path must not be empty");
    }
    if p.as_bytes().contains(&0) {
        return vm.fthrow(a, "ValueError", "Argument #1 ($path) must not contain any null bytes");
    }
    if p.as_bytes().windows(3).any(|w| w.eq_ignore_ascii_case(b"%00")) {
        return vm.fthrow(a, "ValueError", "Argument #1 ($path) must not contain percent-encoded NUL bytes");
    }
    let path = s8(&p);
    let bytes = match super::super::file::open_stream(vm, a, p.as_bytes(), b"rb", false)? {
        Some(mut st) => super::super::file::read_rest(vm, &mut st),
        None => return vm.throw("Exception", &format!("Cannot open file '{path}'")),
    };
    vm.check_deferred()?;
    let url = if path.contains("://") {
        path.clone()
    } else {
        let abs = file_url(vm, &path);
        format!("file://{}", abs.replace(' ', "%20"))
    };
    html_parse(vm, a, &bytes, &path, Some(url))
}

/// `Dom\` XML serialization of a node or document.
pub(crate) fn modern_xml_bytes(vm: &mut Vm, n: NodeId, flags: u32) -> R<Option<Vec<u8>>> {
    let format = with_arena(|ar| ar.doc_of(n));
    let format = settings(format).format_output;
    let r = with_arena(|ar| {
        let o = goro_xml::modern::ModernOptions {
            format,
            no_empty_tags: flags & opt::NOEMPTYTAG != 0,
            no_xml_decl: flags & opt::NOXMLDECL != 0,
            require_well_formed: false,
        };
        goro_xml::modern::save_xml(ar, n, o)
    });
    match r {
        Ok(b) => Ok(Some(b)),
        Err(e) => {
            let _ = vm;
            let _ = e;
            Ok(None)
        }
    }
}

fn modern_target(vm: &mut Vm, a: &Args, d: NodeId) -> R<NodeId> {
    match opt_arg_node(vm, a, 0)? {
        Some(n) => {
            if with_arena(|ar| ar.doc_of(n)) != d {
                return throw_dom(vm, DomError::WrongDocument);
            }
            Ok(n)
        }
        None => Ok(d),
    }
}

fn modern_save_xml(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let n = modern_target(vm, a, d)?;
    let flags = a.int(1) as u32;
    if flags & !(opt::NOEMPTYTAG | opt::NOXMLDECL) != 0 {
        // Unknown flags are ignored.
    }
    match modern_xml_bytes(vm, n, flags)? {
        Some(b) => Ok(Value::Str(PhpStr::from(b))),
        None => Ok(Value::Bool(false)),
    }
}

fn modern_save_xml_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let Some(path) = check_path(vm, a, 0, "filename")? else {
        return Ok(Value::Bool(false));
    };
    let flags = a.int(1) as u32;
    let Some(b) = modern_xml_bytes(vm, d, flags)? else {
        return Ok(Value::Bool(false));
    };
    write_file(vm, a, &path, &b)
}

/// HTML5 serialization of a node or document (`Dom\HTMLDocument::saveHtml`).
pub(crate) fn modern_html(n: NodeId) -> Vec<u8> {
    with_arena(|ar| {
        let s = if matches!(ar.data(n), NodeData::Document(_)) {
            goro_xml::html5::serialize::serialize_children(ar, n)
        } else {
            goro_xml::html5::serialize::serialize_node(ar, n)
        };
        let d = ar.doc_of(n);
        let enc = ar.doc(d).encoding.clone().unwrap_or_else(|| "UTF-8".into());
        match goro_xml::html5::encoding::encoding_for_label(&enc) {
            Some(e) => goro_xml::html5::encoding::encode_lossy(&s, e),
            None => s.into_bytes(),
        }
    })
}

fn modern_save_html(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let n = modern_target(vm, a, d)?;
    Ok(Value::Str(PhpStr::from(modern_html(n))))
}

fn modern_save_html_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = this_doc(vm, a)?;
    let Some(path) = check_path(vm, a, 0, "filename")? else {
        return Ok(Value::Bool(false));
    };
    let b = modern_html(d);
    write_file(vm, a, &path, &b)
}

// ------------------------------------------------------- implementation

fn has_feature(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(true))
}

fn impl_create_doctype(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let name = s8(&a.str(0));
    let public = s8(&a.str(1));
    let system = s8(&a.str(2));
    let modern = modern_fn(a);
    if name.is_empty() && !modern {
        return vm.fthrow(a, "ValueError", "Argument #1 ($qualifiedName) must not be empty");
    }
    if !modern && name.to_ascii_lowercase().contains("%00") {
        vm.fwarn(a, "URI must not contain percent-encoded NUL bytes")?;
        return Ok(Value::Bool(false));
    }
    if !goro_xml::chars::is_qname(&name) {
        let e = if modern || goro_xml::chars::is_name(&name) {
            DomError::Namespace
        } else {
            DomError::InvalidCharacter
        };
        return throw_dom(vm, e);
    }
    let d = if modern {
        let d = new_doc(DocKind::Xml, true);
        with_settings(d, |s| s.hidden = true);
        d
    } else {
        hidden_doc()
    };
    let dt = with_arena(|ar| {
        ar.create(
            d,
            NodeData::Doctype(Box::new(goro_xml::tree::DtdData {
                name: name.as_str().into(),
                public_id: Some(public).filter(|s| !s.is_empty()),
                system_id: Some(system).filter(|s| !s.is_empty()),
                ..Default::default()
            })),
        )
    });
    wrap_node(vm, dt)
}

fn impl_create_document(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let modern = modern_fn(a);
    let ns = a.opt_str(0).map(|s| s8(&s));
    let qn = s8(&a.str(1));
    let dt = opt_arg_node(vm, a, 2)?;
    if let Some(t) = dt
        && with_arena(|ar| ar.parent(t).is_some())
    {
        return throw_dom(vm, DomError::WrongDocument);
    }
    let d = new_doc(DocKind::Xml, modern);
    with_arena(|ar| {
        let dd = ar.doc_mut(d);
        dd.version = Some("1.0".into());
        if modern {
            dd.encoding = Some("UTF-8".into());
        }
    });
    if let Some(t) = dt {
        with_arena(|ar| {
            ar.adopt(t, d);
            ar.raw_append(d, t);
            ar.doc_mut(d).doctype = Some(t);
        });
    }
    if !qn.is_empty() {
        let q = match validate_and_extract(ns.as_deref(), &qn, if modern { NC::Modern } else { NC::Legacy }) {
            Ok(q) => q,
            Err(e) => {
                xmlstore::with(|s| s.free_doc_if_unheld(d));
                return throw_dom(vm, e);
            }
        };
        with_arena(|ar| {
            let e = ar.new_element(d, q.clone());
            if !modern && let Some(uri) = &q.ns {
                ar.declare_ns(e, q.prefix.as_deref(), uri);
            }
            ar.raw_append(d, e);
        });
    } else if ns.as_deref().is_some_and(|s| !s.is_empty()) && !modern {
        xmlstore::with(|s| s.free_doc_if_unheld(d));
        return throw_dom(vm, DomError::Namespace);
    }
    with_arena(|ar| ar.collect());
    wrap_node(vm, d)
}

fn impl_create_html_document(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let title = a.opt_str(0).map(|s| s8(&s));
    let d = new_doc(DocKind::Html5, true);
    with_arena(|ar| {
        ar.doc_mut(d).encoding = Some("UTF-8".into());
        let dt = ar.create(
            d,
            NodeData::Doctype(Box::new(goro_xml::tree::DtdData {
                name: "html".into(),
                ..Default::default()
            })),
        );
        ar.raw_append(d, dt);
        ar.doc_mut(d).doctype = Some(dt);
        let h = |l: &str| QName {
            prefix: None,
            local: l.into(),
            ns: Some(goro_xml::tree::HTML_NS.into()),
        };
        let html = ar.new_element(d, h("html"));
        ar.raw_append(d, html);
        let head = ar.new_element(d, h("head"));
        ar.raw_append(html, head);
        if let Some(t) = title {
            let te = ar.new_element(d, h("title"));
            ar.raw_append(head, te);
            let tx = ar.new_text(d, t);
            ar.raw_append(te, tx);
        }
        let body = ar.new_element(d, h("body"));
        ar.raw_append(html, body);
    });
    wrap_node(vm, d)
}

// ------------------------------------------------- legacy constructors

/// Attaches a new node (in a hidden document) to `$this`.
fn construct_node(vm: &mut Vm, a: &Args, data: NodeData) -> R<Value> {
    let _ = vm;
    let d = hidden_doc();
    let n = with_arena(|ar| ar.create(d, data));
    super::attach(a.this_obj(), n);
    Ok(Value::Null)
}

fn attr_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let name = s8(&a.str(0));
    if !check_name(&name) {
        return throw_dom(vm, DomError::InvalidCharacter);
    }
    let value = s8(&a.str(1));
    let d = hidden_doc();
    let n = with_arena(|ar| ar.new_attribute(d, QName::local(&name), &value));
    super::attach(a.this_obj(), n);
    Ok(Value::Null)
}

fn text_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let t = s8(&a.str(0));
    construct_node(vm, a, NodeData::Text(t))
}

fn comment_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let t = s8(&a.str(0));
    construct_node(vm, a, NodeData::Comment(t))
}

fn cdata_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let t = s8(&a.str(0));
    construct_node(vm, a, NodeData::CData(t))
}

fn pi_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let target = s8(&a.str(0));
    if !check_name(&target) {
        return throw_dom(vm, DomError::InvalidCharacter);
    }
    let data = s8(&a.str(1));
    construct_node(
        vm,
        a,
        NodeData::Pi {
            target: target.as_str().into(),
            data,
        },
    )
}

fn entity_ref_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let name = s8(&a.str(0));
    if !check_name(&name) {
        return throw_dom(vm, DomError::InvalidCharacter);
    }
    construct_node(vm, a, NodeData::EntityRef(name.as_str().into()))
}

fn fragment_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    construct_node(vm, a, NodeData::Fragment)
}

/// Document property writes (`encoding`, `version`, `standalone`,
/// `documentURI`; `Dom\` documents' `charset`, `title`, `body`).
pub(crate) fn set_doc_prop(
    vm: &mut Vm,
    o: &ObjRef,
    d: NodeId,
    name: &str,
    v: Value,
    modern: bool,
) -> R<()> {
    let str_of = |vm: &mut Vm, v: &Value| -> R<Option<String>> {
        Ok(match v {
            Value::Null => None,
            other => Some(s8(&vm.to_php_str(other)?)),
        })
    };
    match name {
        "encoding" | "xmlEncoding" if !modern || name == "xmlEncoding" => {
            if name == "xmlEncoding" {
                return readonly(vm, o, name);
            }
            let e = str_of(vm, &v)?;
            match &e {
                Some(x) if goro_xml::encoding::lookup(x).is_some() => {}
                _ => return vm.throw("ValueError", "Invalid document encoding"),
            }
            with_arena(|ar| ar.doc_mut(d).encoding = e);
        }
        "characterSet" | "charset" | "inputEncoding" if modern => {
            let e = str_of(vm, &v)?.unwrap_or_default();
            let ok = if with_arena(|ar| ar.doc(d).kind == DocKind::Html5) {
                goro_xml::html5::encoding::encoding_for_label(&e).map(|x| x.name().to_string())
            } else {
                goro_xml::encoding::lookup(&e).map(|_| e.clone())
            };
            match ok {
                Some(name) => with_arena(|ar| ar.doc_mut(d).encoding = Some(name)),
                None => return vm.throw("ValueError", "Invalid document encoding"),
            }
        }
        "version" | "xmlVersion" => {
            let e = str_of(vm, &v)?;
            with_arena(|ar| ar.doc_mut(d).version = e);
        }
        "standalone" | "xmlStandalone" => {
            let b = vm.to_bool(&v);
            with_arena(|ar| ar.doc_mut(d).standalone = if b { 1 } else { 0 });
        }
        "documentURI" => {
            let e = str_of(vm, &v)?;
            with_arena(|ar| ar.doc_mut(d).url = e);
        }
        "title" if modern => {
            let t = str_of(vm, &v)?.unwrap_or_default();
            super::modern::set_title(d, &t);
        }
        "body" if modern => {
            super::modern::set_body(vm, d, v)?;
        }
        "textContent" | "nodeValue" => {}
        _ => return readonly(vm, o, name),
    }
    Ok(())
}

fn readonly(vm: &mut Vm, o: &ObjRef, name: &str) -> R<()> {
    let c = vm.class_of(o).name.to_string_lossy();
    vm.throw(
        "Error",
        &format!("Cannot modify readonly property {c}::${name}"),
    )
}

/// `dom_import_simplexml()` / `Dom\import_simplexml()`.
pub(crate) fn dom_import_simplexml(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.obj(0).unwrap();
    let n = simplexml_node(&o);
    match n {
        Some(n) => wrap_node(vm, n),
        None => vm.fthrow(
            a,
            "TypeError",
            "Argument #1 ($node) must be of type SimpleXMLElement, object given",
        ),
    }
}

fn simplexml_node(o: &ObjRef) -> Option<NodeId> {
    crate::builtins::simplexml::sxe_node_of(o)
}

/// Throws for a legacy operation's error in a context without `Args`.
pub(crate) fn unwind_dom(vm: &mut Vm, e: DomError) -> Unwind {
    match throw_dom::<()>(vm, e) {
        Err(u) => u,
        Ok(()) => Unwind::Fatal,
    }
}
