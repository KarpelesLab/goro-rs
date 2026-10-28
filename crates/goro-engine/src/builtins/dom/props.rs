//! DOM properties: the hooks of the stubs' hooked properties, run by one
//! native handler per class (`"class::$"` method entries; the engine binds
//! them, see `vm/classes.rs` `bind_natives`). Values follow the DOM
//! Standard for `Dom\` classes and PHP's legacy rules for `DOM*` ones (PHP
//! manual: DOMNode, DOMElement, DOMDocument...; observed on PHP 8.5).

use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};
use goro_xml::NodeId;
use goro_xml::tree::{DocKind, DomError, NodeData, XML_NS};

use super::lists::{self, ListKind};
use super::{
    DocSettings, is_modern_node, node_of, node_type_num, settings, sv, sv_opt, throw_dom,
    with_arena, with_settings,
};
use crate::func::{Args, NativeFn};
use crate::vm::{R, Vm};

const P: NativeFn = prop;

pub const METHODS: &[(&str, NativeFn)] = &[
    ("domnode::$", P),
    ("dom\\node::$", P),
    ("domnamespacenode::$", P),
    ("domdocumentfragment::$", P),
    ("dom\\documentfragment::$", P),
    ("dom\\document::$", P),
    ("domdocument::$", P),
    ("dom\\xmldocument::$", P),
    ("dom\\htmldocument::$", P),
    ("domcharacterdata::$", P),
    ("dom\\characterdata::$", P),
    ("domattr::$", P),
    ("dom\\attr::$", P),
    ("domelement::$", P),
    ("dom\\element::$", P),
    ("domtext::$", P),
    ("dom\\text::$", P),
    ("domdocumenttype::$", P),
    ("dom\\documenttype::$", P),
    ("domnotation::$", P),
    ("dom\\notation::$", P),
    ("domentity::$", P),
    ("dom\\entity::$", P),
    ("domprocessinginstruction::$", P),
    ("dom\\processinginstruction::$", P),
    ("domnodelist::$", lists::list_prop),
    ("dom\\nodelist::$", lists::list_prop),
    ("domnamednodemap::$", lists::list_prop),
    ("dom\\namednodemap::$", lists::list_prop),
    ("dom\\dtdnamednodemap::$", lists::list_prop),
    ("dom\\htmlcollection::$", lists::list_prop),
    ("dom\\tokenlist::$", lists::list_prop),
    ("domxpath::$", super::xpath::xpath_prop),
    ("dom\\xpath::$", super::xpath::xpath_prop),
];

/// The property name and whether this is the set hook, from the hook's
/// name (`C::$prop::get`).
pub(crate) fn hook_name(a: &Args) -> (String, bool) {
    let n = a.func.name.as_bytes();
    let start = n.iter().position(|&c| c == b'$').map_or(0, |p| p + 1);
    let rest = &n[start.min(n.len())..];
    let end = rest
        .windows(2)
        .position(|w| w == b"::")
        .unwrap_or(rest.len());
    let is_set = rest[end..].ends_with(b"set");
    (String::from_utf8_lossy(&rest[..end]).into_owned(), is_set)
}

fn prop(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (name, is_set) = hook_name(a);
    let o = a.this_obj().clone();
    if is_set {
        let v = a.val(0);
        set_prop(vm, &o, &name, v)?;
        return Ok(Value::Null);
    }
    get_prop(vm, &o, &name)
}

/// An intermediate property value (computed with the store borrowed,
/// turned into a PHP value outside).
pub(crate) enum PV {
    Null,
    S(String),
    I(i64),
    B(bool),
    Node(Option<NodeId>),
    List(ListKind),
    /// A cached collection object of the node (`children`, `classList`).
    Cached(ListKind),
    Impl,
    Missing,
}

impl From<Option<String>> for PV {
    fn from(o: Option<String>) -> PV {
        match o {
            Some(s) => PV::S(s),
            None => PV::Null,
        }
    }
}

fn first_el(a: &goro_xml::Arena, n: NodeId) -> Option<NodeId> {
    a.children(n).find(|&c| a.is_element(c))
}

fn last_el(a: &goro_xml::Arena, n: NodeId) -> Option<NodeId> {
    let mut c = a.last_child(n);
    while let Some(x) = c {
        if a.is_element(x) {
            return Some(x);
        }
        c = a.prev_sibling(x);
    }
    None
}

fn prev_el(a: &goro_xml::Arena, n: NodeId) -> Option<NodeId> {
    let mut c = a.prev_sibling(n);
    while let Some(x) = c {
        if a.is_element(x) {
            return Some(x);
        }
        c = a.prev_sibling(x);
    }
    None
}

fn next_el(a: &goro_xml::Arena, n: NodeId) -> Option<NodeId> {
    let mut c = a.next_sibling(n);
    while let Some(x) = c {
        if a.is_element(x) {
            return Some(x);
        }
        c = a.next_sibling(x);
    }
    None
}

/// True for an element whose names HTML rules uppercase (an HTML-namespace
/// element of an HTML document; DOM Standard "qualified name in uppercase").
pub(crate) fn html_upper(a: &goro_xml::Arena, n: NodeId) -> bool {
    let d = a.doc_of(n);
    a.doc(d).kind == DocKind::Html5
        && a.element(n)
            .is_some_and(|e| e.name.ns.as_deref() == Some(goro_xml::tree::HTML_NS))
}

/// `nodeName` (and `tagName`).
pub(crate) fn node_name(a: &goro_xml::Arena, n: NodeId, modern: bool) -> String {
    if modern {
        match a.data(n) {
            NodeData::Element(e) => {
                let q = e.name.qualified();
                return if html_upper(a, n) {
                    q.to_ascii_uppercase()
                } else {
                    q
                };
            }
            NodeData::Pi { target, .. } => return target.to_string(),
            _ => {}
        }
    }
    a.node_name(n)
}

/// The base URI of a node: `xml:base` attributes resolved against the
/// document URL (XML Base; observed on PHP 8.5).
pub(crate) fn base_uri(a: &goro_xml::Arena, n: NodeId) -> Option<String> {
    let doc = a.doc_of(n);
    let mut bases: Vec<String> = Vec::new();
    let mut cur = match a.data(n) {
        NodeData::Attribute(_) | NodeData::Namespace(_) => a.parent(n),
        _ => Some(n),
    };
    while let Some(c) = cur {
        if a.is_element(c)
            && let Some(at) = a.get_attr_node_ns(c, Some(XML_NS), "base")
        {
            bases.push(a.attr_value(at));
        }
        cur = a.parent(c);
    }
    let mut base = a.doc(doc).url.clone();
    for b in bases.iter().rev() {
        base = Some(resolve_uri(b, base.as_deref()));
    }
    base
}

/// Resolves a URI reference against a base (RFC 3986 section 5.2, the
/// plain-path cases libxml handles).
pub(crate) fn resolve_uri(r: &str, base: Option<&str>) -> String {
    let Some(base) = base else {
        return r.to_string();
    };
    if r.contains("://") || r.starts_with('/') || base.is_empty() {
        if let Some(rest) = r.strip_prefix('/')
            && let Some(p) = base.find("://")
            && !r.starts_with("//")
        {
            let host_end = base[p + 3..].find('/').map_or(base.len(), |i| p + 3 + i);
            return format!("{}/{}", &base[..host_end], rest);
        }
        return r.to_string();
    }
    if r.is_empty() {
        return base.to_string();
    }
    let dir = match base.rfind('/') {
        Some(i) => &base[..=i],
        None => "",
    };
    let mut out = format!("{dir}{r}");
    // Remove dot segments.
    loop {
        if let Some(i) = out.find("/./") {
            out.replace_range(i..i + 2, "");
            continue;
        }
        if let Some(i) = out.find("/../")
            && i > 0
        {
            let start = out[..i].rfind('/').map_or(0, |j| j);
            out.replace_range(start..i + 3, "");
            continue;
        }
        break;
    }
    out
}

fn compute(a: &mut goro_xml::Arena, n: NodeId, name: &str, modern: bool) -> PV {
    let node_kind = |a: &goro_xml::Arena| a.node_type(n);
    let data = a.data(n);
    let is_el = matches!(data, NodeData::Element(_));
    let is_attr = matches!(data, NodeData::Attribute(_));
    let is_doc = matches!(data, NodeData::Document(_));
    let is_ns = matches!(data, NodeData::Namespace(_));
    match name {
        "nodeName" => PV::S(node_name(a, n, modern)),
        "nodeValue" => match a.data(n) {
            NodeData::Element(_) if !modern => PV::S(a.text_content(n)),
            NodeData::Attribute(_) => PV::S(a.attr_value(n)),
            NodeData::Text(s) | NodeData::CData(s) | NodeData::Comment(s) => PV::S(s.clone()),
            NodeData::Pi { data, .. } => PV::S(data.clone()),
            NodeData::Namespace(d) => PV::S(d.uri.to_string()),
            NodeData::EntityDecl(e) if !modern => PV::from(e.value.clone()),
            _ => PV::Null,
        },
        "nodeType" => PV::I(node_type_num(a, n)),
        "parentNode" => {
            if is_attr && modern {
                return PV::Node(None);
            }
            PV::Node(a.parent(n))
        }
        "parentElement" => {
            if is_attr || is_ns && modern {
                return PV::Node(None);
            }
            PV::Node(a.parent(n).filter(|&p| a.is_element(p)))
        }
        "childNodes" => PV::List(ListKind::Children(n)),
        "firstChild" => {
            if is_attr && modern {
                return PV::Node(None);
            }
            PV::Node(a.first_child(n))
        }
        "lastChild" => {
            if is_attr && modern {
                return PV::Node(None);
            }
            PV::Node(a.last_child(n))
        }
        "previousSibling" => {
            if is_attr && modern || is_ns {
                return PV::Node(None);
            }
            PV::Node(a.prev_sibling(n))
        }
        "nextSibling" => {
            if is_attr && modern || is_ns {
                return PV::Node(None);
            }
            PV::Node(a.next_sibling(n))
        }
        "attributes" => {
            if is_el {
                PV::List(ListKind::Attrs(n))
            } else {
                PV::Null
            }
        }
        "isConnected" => {
            if is_ns {
                return PV::B(a.parent(n).is_some_and(|p| a.is_connected(p)));
            }
            PV::B(a.is_connected(n))
        }
        "ownerDocument" => {
            if is_doc {
                return PV::Node(None);
            }
            PV::Node(Some(a.doc_of(n)))
        }
        "namespaceURI" => match a.data(n) {
            NodeData::Element(e) => PV::from(e.name.ns.as_ref().map(|s| s.to_string())),
            NodeData::Attribute(at) => PV::from(at.name.ns.as_ref().map(|s| s.to_string())),
            NodeData::Namespace(d) => PV::S(d.uri.to_string()),
            _ => PV::Null,
        },
        "prefix" => {
            let p = match a.data(n) {
                NodeData::Element(e) => e.name.prefix.as_ref().map(|s| s.to_string()),
                NodeData::Attribute(at) => at.name.prefix.as_ref().map(|s| s.to_string()),
                NodeData::Namespace(d) => d.prefix.as_ref().map(|s| s.to_string()),
                _ => None,
            };
            if modern {
                PV::from(p)
            } else {
                PV::S(p.unwrap_or_default())
            }
        }
        "localName" => match a.data(n) {
            NodeData::Element(e) => PV::S(e.name.local.to_string()),
            NodeData::Attribute(at) => PV::S(at.name.local.to_string()),
            NodeData::Namespace(d) => {
                PV::S(d.prefix.as_ref().map_or("xmlns".to_string(), |p| p.to_string()))
            }
            _ => PV::Null,
        },
        "baseURI" => {
            let b = base_uri(a, n);
            if modern {
                PV::S(b.unwrap_or_else(|| "about:blank".into()))
            } else {
                PV::from(b)
            }
        }
        "textContent" => {
            if modern && matches!(a.data(n), NodeData::Document(_)) {
                return PV::Null;
            }
            if !modern && matches!(a.data(n), NodeData::Document(_)) {
                let mut s = String::new();
                for c in a.children(n) {
                    if a.is_element(c) {
                        s.push_str(&a.text_content(c));
                    }
                }
                return PV::S(s);
            }
            PV::S(a.text_content(n))
        }
        // Element, Document, DocumentFragment (ParentNode, ChildNode).
        "tagName" => PV::S(node_name(a, n, modern)),
        "className" => PV::S(a.get_attribute(n, "class").unwrap_or_default()),
        "id" => PV::S(a.get_attribute(n, "id").unwrap_or_default()),
        "schemaTypeInfo" => PV::Null,
        "firstElementChild" => PV::Node(first_el(a, n)),
        "lastElementChild" => PV::Node(last_el(a, n)),
        "childElementCount" => PV::I(a.children(n).filter(|&c| a.is_element(c)).count() as i64),
        "previousElementSibling" => PV::Node(prev_el(a, n)),
        "nextElementSibling" => PV::Node(next_el(a, n)),
        "children" => PV::Cached(ListKind::ChildElements(n)),
        "classList" => PV::Cached(ListKind::ClassList(n)),
        "innerHTML" => PV::S(super::modern::inner_html(a, n)),
        "outerHTML" => PV::S(super::modern::outer_html(a, n)),
        "substitutedNodeValue" => PV::S(a.text_content(n)),
        // CharacterData, Text, ProcessingInstruction.
        "data" => PV::S(a.char_data(n).unwrap_or("").to_string()),
        "length" => PV::I(a.char_data(n).unwrap_or("").chars().count() as i64),
        "wholeText" => PV::S(super::chardata::whole_text(a, n)),
        "target" => match a.data(n) {
            NodeData::Pi { target, .. } => PV::S(target.to_string()),
            _ => PV::S(String::new()),
        },
        // Attr.
        "name" => match a.data(n) {
            NodeData::Attribute(at) => PV::S(at.name.qualified()),
            NodeData::Doctype(d) => PV::S(d.name.to_string()),
            _ => PV::S(a.node_name(n)),
        },
        "value" => PV::S(a.attr_value(n)),
        "specified" => PV::B(true),
        "ownerElement" => PV::Node(a.parent(n)),
        // DocumentType, Entity, Notation.
        "entities" => PV::List(ListKind::Entities(n)),
        "notations" => PV::List(ListKind::Notations(n)),
        "publicId" => match a.data(n) {
            NodeData::Doctype(d) => {
                if modern {
                    PV::S(d.public_id.clone().unwrap_or_default())
                } else {
                    PV::S(d.public_id.clone().unwrap_or_default())
                }
            }
            NodeData::EntityDecl(e) => PV::from(e.public_id.clone()),
            NodeData::Notation(x) => PV::S(x.public_id.clone().unwrap_or_default()),
            _ => PV::Null,
        },
        "systemId" => match a.data(n) {
            NodeData::Doctype(d) => PV::S(d.system_id.clone().unwrap_or_default()),
            NodeData::EntityDecl(e) => PV::from(e.system_id.clone()),
            NodeData::Notation(x) => PV::S(x.system_id.clone().unwrap_or_default()),
            _ => PV::Null,
        },
        "internalSubset" => match a.data(n) {
            NodeData::Doctype(d) => PV::from(goro_xml::serialize::internal_subset(d)),
            _ => PV::Null,
        },
        "notationName" => match a.data(n) {
            NodeData::EntityDecl(e) => PV::from(e.notation.clone()),
            _ => PV::Null,
        },
        "actualEncoding" | "encoding" | "inputEncoding" | "characterSet" | "charset"
            if is_doc =>
        {
            let e = a.doc(n).encoding.clone();
            if modern {
                PV::S(e.unwrap_or_else(|| "UTF-8".into()))
            } else {
                PV::from(e)
            }
        }
        "actualEncoding" | "encoding" => PV::Null,
        "version" if !is_doc => PV::Null,
        "xmlEncoding" => {
            let e = a.doc(n).encoding.clone();
            if modern {
                PV::S(e.unwrap_or_else(|| "UTF-8".into()))
            } else {
                PV::from(e)
            }
        }
        "standalone" | "xmlStandalone" => PV::B(a.doc(n).standalone == 1),
        "xmlVersion" | "version" if is_doc => {
            let v = a.doc(n).version.clone();
            if modern {
                PV::S(v.unwrap_or_else(|| "1.0".into()))
            } else {
                PV::from(v.or_else(|| Some("1.0".into())))
            }
        }
        "documentURI" | "URL" => {
            let u = a.doc(n).url.clone();
            if modern {
                PV::S(u.unwrap_or_else(|| "about:blank".into()))
            } else {
                PV::from(u)
            }
        }
        "doctype" => PV::Node(
            a.doc(n)
                .doctype
                .filter(|&d| a.is_live(d) && a.parent(d) == Some(n))
                .or_else(|| {
                    a.children(n)
                        .find(|&c| matches!(a.data(c), NodeData::Doctype(_)))
                }),
        ),
        "implementation" => PV::Impl,
        "documentElement" => PV::Node(a.document_element(n)),
        "config" => PV::Null,
        "body" => PV::Node(super::modern::body(a, n)),
        "head" => PV::Node(super::modern::head(a, n)),
        "title" => PV::S(super::modern::title(a, n)),
        _ => {
            let _ = node_kind(a);
            PV::Missing
        }
    }
}

/// Reads a DOM property of `o`.
pub(crate) fn get_prop(vm: &mut Vm, o: &ObjRef, name: &str) -> R<Value> {
    let Some(n) = node_of(o) else {
        if name == "schemaTypeInfo" {
            return Ok(Value::Null);
        }
        return throw_dom(vm, DomError::InvalidState);
    };
    let cls = vm.class_of(o);
    if cls.is_subclass_of(&vm.find_class(b"DOMDocument", false)?.unwrap())
        && matches!(name, "config" | "actualEncoding")
    {
        vm.deprecated(&format!("Property DOMDocument::${name} is deprecated"))?;
    }
    // Document settings.
    if let Some(v) = settings_prop(n, name) {
        return Ok(Value::Bool(v));
    }
    let modern = with_arena(|a| is_modern_node(a, n));
    let pv = xmlstore_compute(n, name, modern);
    pv_value(vm, o, pv, modern)
}

fn xmlstore_compute(n: NodeId, name: &str, modern: bool) -> PV {
    with_arena(|a| {
        let hidden_owner = name == "ownerDocument";
        let pv = compute(a, n, name, modern);
        if hidden_owner && let PV::Node(Some(d)) = pv {
            return PV::Node(Some(d));
        }
        pv
    })
}

fn settings_prop(n: NodeId, name: &str) -> Option<bool> {
    let st = |f: fn(&DocSettings) -> bool| {
        let is_doc = with_arena(|a| matches!(a.data(n), NodeData::Document(_)));
        if !is_doc {
            return None;
        }
        Some(f(&settings(n)))
    };
    match name {
        "formatOutput" => st(|s| s.format_output),
        "validateOnParse" => st(|s| s.validate_on_parse),
        "resolveExternals" => st(|s| s.resolve_externals),
        "preserveWhiteSpace" => st(|s| s.preserve_white_space),
        "recover" => st(|s| s.recover),
        "substituteEntities" => st(|s| s.substitute_entities),
        "strictErrorChecking" => st(|s| s.strict_error_checking),
        _ => None,
    }
}

/// Converts a computed value.
pub(crate) fn pv_value(vm: &mut Vm, o: &ObjRef, pv: PV, modern: bool) -> R<Value> {
    Ok(match pv {
        PV::Null => Value::Null,
        PV::S(s) => sv(&s),
        PV::I(i) => Value::Int(i),
        PV::B(b) => Value::Bool(b),
        PV::Node(Some(id)) => {
            // Nodes of an implicit document report no owner document.
            if with_settings(id, |s| s.hidden)
                && with_arena(|a| matches!(a.data(id), NodeData::Document(_)))
            {
                return Ok(Value::Null);
            }
            super::wrap_node(vm, id)?
        }
        PV::Node(None) => Value::Null,
        PV::List(k) => lists::new_list_owned(vm, k, modern, Some(o.clone()))?,
        PV::Cached(k) => lists::cached_list(vm, o, k)?,
        PV::Impl if modern => {
            let found = super::with_dom_obj(o, |d| {
                d.cached
                    .borrow()
                    .iter()
                    .find(|(t, _)| *t == 3)
                    .map(|(_, x)| x.clone())
            })
            .flatten();
            if let Some(x) = found {
                return Ok(Value::Object(x));
            }
            let cls = vm.find_class(b"Dom\\Implementation", false)?.unwrap();
            let x = vm.instantiate_raw(&cls)?;
            super::with_dom_obj(o, |d| d.cached.borrow_mut().push((3, x.clone())));
            Value::Object(x)
        }
        PV::Impl => {
            let c = if modern {
                "Dom\\Implementation"
            } else {
                "DOMImplementation"
            };
            let cls = vm.find_class(c.as_bytes(), false)?.unwrap();
            Value::Object(vm.instantiate_raw(&cls)?)
        }
        PV::Missing => Value::Null,
    })
}

/// Writes a DOM property of `o`.
pub(crate) fn set_prop(vm: &mut Vm, o: &ObjRef, name: &str, v: Value) -> R<()> {
    let readonly = |vm: &mut Vm| -> R<()> {
        let c = vm.class_of(o).name.to_string_lossy();
        vm.throw(
            "Error",
            &format!("Cannot modify readonly property {c}::${name}"),
        )
    };
    let Some(n) = node_of(o) else {
        // Read-only properties say so first (test property_write_errors).
        const WRITABLE: &[&str] = &[
            "nodeValue", "textContent", "data", "value", "id", "className", "prefix",
            "innerHTML", "outerHTML", "substitutedNodeValue", "formatOutput",
            "validateOnParse", "resolveExternals", "preserveWhiteSpace", "recover",
            "substituteEntities", "strictErrorChecking", "encoding", "version",
            "xmlVersion", "standalone", "xmlStandalone", "documentURI", "title", "body",
            "characterSet", "charset", "inputEncoding",
        ];
        if !WRITABLE.contains(&name) {
            return readonly(vm);
        }
        return throw_dom(vm, DomError::InvalidState);
    };
    let modern = with_arena(|a| is_modern_node(a, n));
    let is_doc = with_arena(|a| matches!(a.data(n), NodeData::Document(_)));
    if is_doc {
        let b = matches!(v, Value::Bool(true));
        let done = match name {
            "formatOutput" => with_settings(n, |s| s.format_output = b),
            "validateOnParse" => with_settings(n, |s| s.validate_on_parse = b),
            "resolveExternals" => with_settings(n, |s| s.resolve_externals = b),
            "preserveWhiteSpace" => with_settings(n, |s| s.preserve_white_space = b),
            "recover" => with_settings(n, |s| s.recover = b),
            "substituteEntities" => with_settings(n, |s| s.substitute_entities = b),
            "strictErrorChecking" => with_settings(n, |s| s.strict_error_checking = b),
            _ => {
                return super::document::set_doc_prop(vm, o, n, name, v, modern);
            }
        };
        let _ = done;
        return Ok(());
    }
    let s = match &v {
        Value::Str(s) => s.clone(),
        Value::Null => PhpStr::empty(),
        other => vm.to_php_str(other)?,
    };
    let mut text = super::s8(&s);
    if !modern && let Some(i) = text.find('\0') {
        // Legacy values are C strings (test DOMElement_className.phpt).
        text.truncate(i);
    }
    match name {
        "nodeValue" => {
            super::node::set_node_value(vm, n, &text, modern)?;
        }
        "textContent" => {
            super::node::set_text_content(vm, n, &text)?;
        }
        "data" => {
            super::chardata::set_data(n, &text);
        }
        "value" => {
            super::element::set_attr_node_value(n, &text);
        }
        "id" => {
            super::element::set_attr_by_name(vm, n, "id", &text)?;
            if !modern {
                with_arena(|a| {
                    if let Some(at) = a.get_attr_node(n, "id") {
                        a.attr_mut(at).unwrap().is_id = true;
                    }
                });
            }
        }
        "className" => super::element::set_attr_by_name(vm, n, "class", &text)?,
        "prefix" if !modern => super::element::set_prefix(vm, n, &text)?,
        "innerHTML" if modern => super::modern::set_inner_html(vm, n, &text)?,
        "outerHTML" if modern => super::modern::set_outer_html(vm, n, &text)?,
        "substitutedNodeValue" if modern => {
            super::node::set_node_value(vm, n, &text, false)?;
        }
        "prefix" => {
            let c = vm.class_of(o).name.to_string_lossy();
            return vm.throw("Error", &format!("Cannot modify readonly property {c}::$prefix"));
        }
        _ => return readonly(vm),
    }
    Ok(())
}

/// The properties var_dump/print_r show: the object's own (declared and
/// dynamic) ones, then the DOM properties of each class from the object's
/// up, objects shown as "(object value omitted)" (observed on PHP 8.5).
pub(crate) fn debug_props(vm: &mut Vm, o: &ObjRef) -> Option<PhpArray> {
    let mut out = vm.plain_props_array(o, true);
    let cls = vm.class_of(o);
    let mut chain = Vec::new();
    let mut c = Some(cls.clone());
    while let Some(k) = c {
        chain.push(k.clone());
        c = k.parent.clone();
    }
    let props = cls.props.borrow().clone();
    for k in &chain {
        for p in props.iter() {
            if !std::ptr::eq(p.class, &**k) || p.get.as_ref().is_none_or(|g| !g.is_native()) {
                continue;
            }
            let name = p.name.to_string_lossy();
            let saved = vm.err.silence.len();
            let r = get_prop_quiet(vm, o, &name);
            let _ = saved;
            match r {
                Ok(Value::Object(_)) => {
                    out.insert(
                        Key::Str(p.name.clone()),
                        Value::from(&b"(object value omitted)"[..]),
                    );
                }
                Ok(v) => {
                    // A node made without a document shows no owner
                    // document (test DOMEntityReference_predefined_free).
                    if v.is_null()
                        && p.name.as_bytes() == b"ownerDocument"
                        && node_of(o).is_some_and(|n| {
                            with_settings(n, |s| s.hidden)
                                && with_arena(|a| !matches!(a.data(n), NodeData::Document(_)))
                        })
                    {
                        continue;
                    }
                    out.insert(Key::Str(p.name.clone()), v);
                }
                Err(_) => {}
            }
        }
    }
    Some(out)
}

/// A property read for var_dump (no deprecations).
fn get_prop_quiet(vm: &mut Vm, o: &ObjRef, name: &str) -> R<Value> {
    if matches!(name, "config" | "actualEncoding") {
        return Ok(Value::Null);
    }
    if node_of(o).is_none() {
        // Only the constant property shows on an object without node.
        return if name == "schemaTypeInfo" {
            Ok(Value::Null)
        } else {
            Err(crate::vm::Unwind::Fatal)
        };
    }
    get_prop(vm, o, name)
}
