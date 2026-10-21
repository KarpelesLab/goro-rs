//! The spec-compliant DOM API in the `Dom\` namespace (PHP 8.4):
//! Dom\XMLDocument & co. Ported from php-src/ext/dom (the `Dom_*`
//! PHP_METHODs, the `php_dom_follow_spec_intern` branches, xml_document.c,
//! xml_serializer.c, namespace_compat.c, token_list.c, html_collection.c,
//! inner_outer_html_mixin.c, ...).
//!
//! Nodes of a modern document live in the same arena as legacy ones; the
//! document's `DocProps::class_type` (2 = modern) decides which wrapper
//! classes are used. Namespaces of modern documents are never declared on
//! elements (`ns_def` is empty): every element/attribute points to a shared
//! namespace of the document's namespace mapper (one per (prefix, uri)), and
//! namespace declarations are ordinary attributes in the XMLNS namespace.

pub mod chardata;
pub mod collections;
pub mod document;
pub mod element;
pub mod html_serializer;
pub mod inner_html;
pub mod node;
pub mod serializer;
pub mod tokenlist;

use super::*;
use crate::tree::{NsId, XML_XML_NAMESPACE};

pub const XHTML_NS: &str = "http://www.w3.org/1999/xhtml";
pub const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";
pub const SVG_NS: &str = "http://www.w3.org/2000/svg";
pub const XLINK_NS: &str = "http://www.w3.org/1999/xlink";

/// Modern-only native object state.
pub enum MKind {
    /// Dom\TokenList bound to an element (classList)
    TokenList(tokenlist::TokenList),
    /// InternalIterator over a Dom\TokenList
    TokenIter(tokenlist::TokenIter),
}

// ---------------------------------------------------------------------------
// Class type / document flavour

pub fn is_modern_doc(doc: Option<NodeId>) -> bool {
    doc.map(|d| doc_props(Some(d)).class_type == 2)
        .unwrap_or(false)
}

pub fn set_class_type(doc: NodeId, ct: u8) {
    with_doc_props(doc, |p| p.class_type = ct);
}

/// Is `n` a node of a modern document (or a doc-less modern node)?
pub fn is_modern_node(n: NodeId) -> bool {
    let (doc, flags) = tree::with(|t| {
        (
            if t.is_doc(n) { Some(n) } else { t.nodes[n].doc },
            t.nodes[n].flags,
        )
    });
    flags & tree::F_MODERN != 0 || is_modern_doc(doc)
}

/// The modern class family for a node of a modern document.
pub fn modern_base_for(n: NodeId, base: Base) -> Base {
    match base {
        Base::Document => {
            if tree::with(|t| t.nodes[n].t == NType::HtmlDocument) {
                Base::MHtmlDocument
            } else {
                Base::MXmlDocument
            }
        }
        Base::Element => {
            if tree::with(|t| ns_is_html_and_doc_is_html(t, n)) {
                Base::MHtmlElement
            } else {
                Base::MElement
            }
        }
        Base::DocumentFragment => Base::MDocumentFragment,
        Base::Attr => Base::MAttr,
        Base::Text => Base::MText,
        Base::Comment => Base::MComment,
        Base::CdataSection => Base::MCdataSection,
        Base::DocumentType => Base::MDocumentType,
        Base::Notation => Base::MNotation,
        Base::Entity => Base::MEntity,
        Base::EntityReference => Base::MEntityReference,
        Base::ProcessingInstruction => Base::MProcessingInstruction,
        b => b,
    }
}

/// Classes whose objects are nodes (they have a private final constructor).
pub fn is_node_base(b: Base) -> bool {
    matches!(
        b,
        Base::MNode
            | Base::MDocument
            | Base::MXmlDocument
            | Base::MHtmlDocument
            | Base::MDocumentFragment
            | Base::MElement
            | Base::MHtmlElement
            | Base::MAttr
            | Base::MCharacterData
            | Base::MText
            | Base::MComment
            | Base::MCdataSection
            | Base::MDocumentType
            | Base::MNotation
            | Base::MEntity
            | Base::MEntityReference
            | Base::MProcessingInstruction
    )
}

pub fn ns_is(t: &Tree, ns: Option<NsId>, uri: &str) -> bool {
    ns.map(|x| t.nss[x].href == uri).unwrap_or(false)
}

/// php_dom_ns_is_html_and_document_is_html
pub fn ns_is_html_and_doc_is_html(t: &Tree, n: NodeId) -> bool {
    let doc = t.nodes[n].doc;
    match doc {
        Some(d) if t.nodes[d].t == NType::HtmlDocument => ns_is(t, t.nodes[n].ns, XHTML_NS),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Namespace mapper (namespace_compat.c / private_data.c)

thread_local! {
    /// doc -> (uri, prefix or "") -> namespace
    static MAPPERS: RefCell<HashMap<NodeId, HashMap<(String, String), NsId>>> = RefCell::new(HashMap::new());
}

/// php_dom_libxml_ns_mapper_get_ns: the shared namespace for (prefix, uri);
/// None for the empty namespace without prefix.
pub fn mapper_get_ns(
    t: &mut Tree,
    doc: NodeId,
    prefix: Option<&str>,
    uri: Option<&str>,
) -> Option<NsId> {
    let prefix = prefix.unwrap_or("");
    let uri = uri.unwrap_or("");
    if prefix.is_empty() && uri.is_empty() {
        return None;
    }
    let key = (uri.to_string(), prefix.to_string());
    if let Some(ns) = MAPPERS.with(|m| m.borrow().get(&doc).and_then(|h| h.get(&key).copied())) {
        return Some(ns);
    }
    let ns = t.new_ns_raw(
        uri,
        if prefix.is_empty() {
            None
        } else {
            Some(prefix)
        },
        None,
    );
    MAPPERS.with(|m| {
        m.borrow_mut().entry(doc).or_default().insert(key, ns);
    });
    Some(ns)
}

/// php_dom_libxml_ns_mapper_store_and_normalize_parsed_ns
fn mapper_store_parsed(t: &Tree, doc: NodeId, ns: NsId) {
    let key = (
        t.nss[ns].href.clone(),
        t.nss[ns].prefix.clone().unwrap_or_default(),
    );
    MAPPERS.with(|m| {
        m.borrow_mut()
            .entry(doc)
            .or_default()
            .entry(key)
            .or_insert(ns);
    });
}

pub fn mapper_reset(doc: NodeId) {
    MAPPERS.with(|m| {
        m.borrow_mut().insert(doc, HashMap::new());
    });
}

/// php_dom_ns_compat_mark_attribute: an `xmlns`/`xmlns:prefix` attribute
/// equivalent to the declaration `ns`, unlinked.
fn compat_attribute(t: &mut Tree, doc: NodeId, node: NodeId, ns: NsId) -> NodeId {
    let (prefix, href) = (t.nss[ns].prefix.clone(), t.nss[ns].href.clone());
    let (xmlns_ns, name) = match prefix {
        Some(p) => (mapper_get_ns(t, doc, Some("xmlns"), Some(XMLNS_NS)), p),
        None => (
            mapper_get_ns(t, doc, None, Some(XMLNS_NS)),
            "xmlns".to_string(),
        ),
    };
    let d = t.nodes[node].doc;
    // xmlSetNsProp always creates a text child, even for an empty value
    let a = t.new_attr_node(d, xmlns_ns, &name, None);
    let tx = t.new_text(d, &href);
    t.link_child_raw(a, tx);
    a
}

/// php_dom_ns_compat_mark_attribute_list: turn the namespace declarations
/// of `node` into attributes (prepended, in declaration order).
pub fn compat_mark_attribute_list(t: &mut Tree, doc: NodeId, node: NodeId) {
    let defs = std::mem::take(&mut t.nodes[node].ns_def);
    if defs.is_empty() {
        return;
    }
    let mut new_attrs = Vec::new();
    for ns in defs {
        // xmlSetNsProp replaces an existing attribute with the same name/ns
        let a = compat_attribute(t, doc, node, ns);
        new_attrs.push(a);
        mapper_store_parsed(t, doc, ns);
    }
    prepend_attrs(t, node, &new_attrs);
}

fn prepend_attrs(t: &mut Tree, node: NodeId, attrs: &[NodeId]) {
    let old = t.attrs_of(node);
    for &a in &old {
        t.unlink(a);
    }
    t.nodes[node].properties = None;
    for &a in attrs {
        t.link_child_raw(node, a);
    }
    for &a in &old {
        t.link_child_raw(node, a);
    }
}

/// dom_mark_namespaces_as_attributes_too
pub fn mark_namespaces_as_attributes(t: &mut Tree, doc: NodeId) {
    let mut cur = t.nodes[doc].children;
    while let Some(x) = cur {
        if t.nodes[x].t == NType::Element {
            compat_mark_attribute_list(t, doc, x);
        }
        cur = next_in_tree_order(t, x, None);
    }
}

/// php_dom_next_in_tree_order (elements' children only; attributes and
/// entity references are not descended into).
pub fn next_in_tree_order(t: &Tree, node: NodeId, base: Option<NodeId>) -> Option<NodeId> {
    if t.nodes[node].t == NType::Element || t.nodes[node].t == NType::DocumentFrag || t.is_doc(node)
    {
        if let Some(c) = t.nodes[node].children {
            return Some(c);
        }
    }
    let mut cur = node;
    loop {
        if Some(cur) == base {
            return None;
        }
        if let Some(n) = t.nodes[cur].next {
            return Some(n);
        }
        cur = t.nodes[cur].parent?;
        if Some(cur) == base {
            return None;
        }
    }
}

/// dom_document_convert_to_modern
pub fn convert_to_modern(doc: NodeId) {
    set_class_type(doc, 2);
    mapper_reset(doc);
    tree::with(|t| mark_namespaces_as_attributes(t, doc));
}

/// php_dom_libxml_reconcile_modern: move the namespaces of a subtree into
/// the mapper of `doc`.
pub fn reconcile_modern(t: &mut Tree, doc: NodeId, node: NodeId) {
    let map_one = |t: &mut Tree, cache: &mut HashMap<NsId, NsId>, x: NodeId| {
        if let Some(ns) = t.nodes[x].ns {
            let nn = match cache.get(&ns) {
                Some(n) => *n,
                None => {
                    let (p, h) = (t.nss[ns].prefix.clone(), t.nss[ns].href.clone());
                    let n = mapper_get_ns(t, doc, p.as_deref(), Some(&h));
                    match n {
                        Some(n) => {
                            cache.insert(ns, n);
                            n
                        }
                        None => return,
                    }
                }
            };
            t.nodes[x].ns = Some(nn);
        }
    };
    let mut cache: HashMap<NsId, NsId> = HashMap::new();
    if t.nodes[node].t == NType::Attribute {
        map_one(t, &mut cache, node);
        return;
    }
    if t.nodes[node].t != NType::Element {
        return;
    }
    let mut cur = Some(node);
    while let Some(x) = cur {
        if t.nodes[x].t == NType::Element {
            // declarations of legacy nodes become attributes
            if !t.nodes[x].ns_def.is_empty() {
                compat_mark_attribute_list(t, doc, x);
            }
            map_one(t, &mut cache, x);
            for a in t.attrs_of(x) {
                map_one(t, &mut cache, a);
            }
        }
        cur = next_in_tree_order(t, x, Some(node));
    }
}

/// php_dom_get_in_scope_ns: in-scope namespaces of `node` (prefix -> ns),
/// in libxml order (bottom to top, first seen wins).
pub fn in_scope_ns(t: &mut Tree, doc: NodeId, node: NodeId, ignore_elements: bool) -> Vec<NsId> {
    let mut out: Vec<(String, NsId)> = Vec::new();
    let mut cur = Some(node);
    while let Some(c) = cur {
        if t.nodes[c].t == NType::Element {
            if !ignore_elements {
                if let Some(ns) = t.nodes[c].ns {
                    if let Some(p) = t.nss[ns].prefix.clone() {
                        if !out.iter().any(|(k, _)| *k == p) {
                            out.push((p, ns));
                        }
                    }
                }
            }
            for a in t.attrs_of(c) {
                if let Some(ans) = t.nodes[a].ns {
                    if t.nss[ans].prefix.is_some()
                        && t.nss[ans].href == XMLNS_NS
                        && t.nodes[a].children.is_some()
                    {
                        let prefix = t.nodes[a].name.clone();
                        let val = t.node_get_content(a);
                        if let Some(ns) = mapper_get_ns(t, doc, Some(&prefix), Some(&val)) {
                            if !out.iter().any(|(k, _)| *k == prefix) {
                                out.push((prefix, ns));
                            }
                        }
                    }
                }
            }
        }
        cur = t.nodes[c].parent;
    }
    out.into_iter().map(|(_, n)| n).collect()
}

// ---------------------------------------------------------------------------
// Small helpers

pub fn s(v: &str) -> Value {
    Value::String(PhpString::from_string(v.to_string()))
}

pub fn opt_s(v: Option<&str>) -> Value {
    match v {
        Some(x) => s(x),
        None => Value::Null,
    }
}

pub fn bv(v: bool) -> Value {
    if v { Value::True } else { Value::False }
}

pub fn args<'a>(a: &'a [Value]) -> Args<'a> {
    Args::new(cur_method(), a)
}

/// Document node for a node (the node itself for documents).
pub fn doc_from_node(t: &Tree, n: NodeId) -> Option<NodeId> {
    if t.is_doc(n) { Some(n) } else { t.nodes[n].doc }
}

/// Throw a DOMException (always strict in the modern API).
pub fn dom_err(vm: &mut Vm, code: i64) {
    throw_dom(vm, code, true);
}

pub fn dom_err_msg(vm: &mut Vm, code: i64, msg: &str) {
    throw_dom_msg(vm, code, msg, true);
}

/// A node argument of class `class` (DOM_GET_OBJ included).
pub fn arg_node(
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

/// Get or create a cached property object.
pub fn cached_obj(o: &Obj, key: &'static str, weak: bool, make: impl FnOnce() -> Obj) -> Obj {
    let d = match data_of(o) {
        Some(d) => d,
        None => return make(),
    };
    {
        let c = d.cache.borrow();
        for (k, e) in c.iter() {
            if *k == key {
                match e {
                    CacheEntry::Strong(x) => return x.clone(),
                    CacheEntry::Weak(w) => {
                        if let Some(x) = w.upgrade() {
                            return x;
                        }
                    }
                }
            }
        }
    }
    let obj = make();
    let mut c = d.cache.borrow_mut();
    c.retain(|(k, _)| *k != key);
    c.push((
        key,
        if weak {
            CacheEntry::Weak(Rc::downgrade(&obj))
        } else {
            CacheEntry::Strong(obj.clone())
        },
    ));
    obj
}

/// Modern list object (Dom\NodeList / Dom\HTMLCollection / ...).
pub fn mlist_obj(vm: &mut Vm, class: &[u8], k: ListKind) -> Value {
    let o = new_object(vm, class);
    attach_with(&o, Kind::List(k), &collections::LIST_HANDLERS);
    Value::Object(o)
}

pub fn mmap_obj(vm: &mut Vm, class: &[u8], k: MapKind) -> Value {
    let o = new_object(vm, class);
    attach_with(&o, Kind::Map(k), &collections::MAP_HANDLERS);
    Value::Object(o)
}

/// Object for a node created by the modern API without a document.
pub fn node_obj_detached_modern(vm: &mut Vm, n: NodeId) -> Value {
    tree::with(|t| t.nodes[n].flags |= tree::F_MODERN);
    node_obj(vm, n)
}

// ---------------------------------------------------------------------------
// Handler tables

pub static NODE_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    read_property: Some(h_read_property_pub),
    write_property: Some(h_write_property_pub),
    has_property: Some(h_has_property_pub),
    unset_property: Some(h_unset_property_pub),
    read_dimension: None,
    write_dimension: None,
    has_dimension: None,
    unset_dimension: None,
    get_properties: Some(h_get_properties_pub),
    count_elements: None,
    cast: None,
    compare: None,
    clone_obj: Some(node::h_clone),
};

pub static UNCLONEABLE_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    read_property: Some(h_read_property_pub),
    write_property: Some(h_write_property_pub),
    has_property: Some(h_has_property_pub),
    unset_property: Some(h_unset_property_pub),
    read_dimension: None,
    write_dimension: None,
    has_dimension: None,
    unset_dimension: None,
    get_properties: Some(h_get_properties_pub),
    count_elements: None,
    cast: None,
    compare: None,
    clone_obj: Some(h_uncloneable),
};

pub fn h_uncloneable(vm: &mut Vm, src: &Obj, _dst: &Obj) {
    let cn = goro_core::value::display_class_name(&src.borrow().class_name).to_string();
    throw_error(
        vm,
        b"Error",
        &format!("Trying to clone an uncloneable object of class {}", cn),
    );
}

// ---------------------------------------------------------------------------
// Property and method tables

/// A legacy property handler, reused as is.
fn lp(t: &HashMap<Base, Rc<Vec<Prop>>>, b: Base, name: &str) -> Prop {
    *t.get(&b)
        .and_then(|v| v.iter().find(|p| p.name == name))
        .unwrap_or_else(|| panic!("no legacy prop {}", name))
}

fn owned(v: Vec<Prop>, owner: &'static str) -> Vec<Prop> {
    v.into_iter()
        .map(|mut p| {
            if p.owner.is_empty() {
                p.owner = owner;
            }
            p
        })
        .collect()
}

fn merge(own: &[Prop], parent: &[Prop]) -> Vec<Prop> {
    let mut v: Vec<Prop> = own.to_vec();
    for p in parent {
        if !v.iter().any(|x| x.name == p.name) {
            v.push(*p);
        }
    }
    v
}

fn overwrite(v: &mut [Prop], np: Prop) {
    for x in v.iter_mut() {
        if x.name == np.name {
            *x = np;
        }
    }
}

pub fn build_tables(t: &mut HashMap<Base, Rc<Vec<Prop>>>) {
    let node = owned(node::node_props(t), "Dom\\Node");
    t.insert(Base::MNode, Rc::new(node.clone()));
    // DocumentFragment
    let mut frag = merge(
        &owned(node::parent_props(t), "Dom\\DocumentFragment"),
        &node,
    );
    overwrite(&mut frag, node::text_content_rw("Dom\\Node"));
    t.insert(Base::MDocumentFragment, Rc::new(frag));
    // Document
    let docp = merge(&owned(document::document_props(t), "Dom\\Document"), &node);
    t.insert(Base::MDocument, Rc::new(docp.clone()));
    t.insert(Base::MHtmlDocument, Rc::new(docp.clone()));
    t.insert(
        Base::MXmlDocument,
        Rc::new(merge(
            &owned(document::xml_document_props(t), "Dom\\XMLDocument"),
            &docp,
        )),
    );
    // CharacterData
    let mut cd = merge(
        &owned(chardata::chardata_props(t), "Dom\\CharacterData"),
        &node,
    );
    overwrite(&mut cd, node::node_value_rw("Dom\\Node"));
    overwrite(&mut cd, node::text_content_rw("Dom\\Node"));
    t.insert(Base::MCharacterData, Rc::new(cd.clone()));
    t.insert(Base::MComment, Rc::new(cd.clone()));
    let text = merge(&owned(chardata::text_props(t), "Dom\\Text"), &cd);
    t.insert(Base::MText, Rc::new(text.clone()));
    t.insert(Base::MCdataSection, Rc::new(text));
    t.insert(
        Base::MProcessingInstruction,
        Rc::new(merge(
            &owned(chardata::pi_props(t), "Dom\\ProcessingInstruction"),
            &cd,
        )),
    );
    // Attr
    let mut attr = merge(&owned(element::attr_props(t), "Dom\\Attr"), &node);
    overwrite(&mut attr, node::node_value_rw("Dom\\Node"));
    overwrite(&mut attr, node::text_content_rw("Dom\\Node"));
    t.insert(Base::MAttr, Rc::new(attr));
    // Element
    let mut el = merge(&owned(element::element_props(t), "Dom\\Element"), &node);
    overwrite(&mut el, node::text_content_rw("Dom\\Node"));
    t.insert(Base::MElement, Rc::new(el.clone()));
    t.insert(Base::MHtmlElement, Rc::new(el));
    // DocumentType, Notation (uses the plain node table, as in php_dom.c), Entity
    t.insert(
        Base::MDocumentType,
        Rc::new(merge(
            &owned(chardata::doctype_props(t), "Dom\\DocumentType"),
            &node,
        )),
    );
    t.insert(Base::MNotation, Rc::new(node.clone()));
    t.insert(
        Base::MEntity,
        Rc::new(merge(
            &owned(chardata::entity_props(t), "Dom\\Entity"),
            &node,
        )),
    );
    let mut eref = node.clone();
    for np in owned(chardata::entity_ref_overrides(t), "Dom\\Node") {
        overwrite(&mut eref, np);
    }
    t.insert(Base::MEntityReference, Rc::new(eref));
    // lists
    let len = owned(collections::list_props(), "Dom\\NodeList");
    t.insert(Base::MNodeList, Rc::new(len));
    t.insert(
        Base::MHtmlCollection,
        Rc::new(owned(collections::list_props(), "Dom\\HTMLCollection")),
    );
    t.insert(
        Base::MNamedNodeMap,
        Rc::new(owned(collections::list_props(), "Dom\\NamedNodeMap")),
    );
    t.insert(
        Base::MDtdNamedNodeMap,
        Rc::new(owned(collections::list_props(), "Dom\\DtdNamedNodeMap")),
    );
    let xp = t
        .get(&Base::XPath)
        .map(|v| v.as_ref().clone())
        .unwrap_or_default();
    let xp: Vec<Prop> = xp
        .into_iter()
        .map(|mut p| {
            p.owner = "Dom\\XPath";
            p
        })
        .collect();
    t.insert(Base::MXPath, Rc::new(xp));
    t.insert(
        Base::MTokenList,
        Rc::new(owned(tokenlist::props(), "Dom\\TokenList")),
    );
    t.insert(Base::MImplementation, Rc::new(Vec::new()));
}

/// Look up a legacy method by name.
pub fn legacy_method(b: Base, name: &str) -> MethodFn {
    *methods_of_pub(b)
        .get(&name.to_ascii_lowercase())
        .unwrap_or_else(|| panic!("no legacy method {}", name))
}

pub fn method_table(b: Base) -> Vec<(&'static str, MethodFn)> {
    let mut v: Vec<(&'static str, MethodFn)> = Vec::new();
    let nm = node::node_methods;
    match b {
        Base::MNode => v.extend(nm()),
        Base::MDocument | Base::MXmlDocument | Base::MHtmlDocument => {
            if b == Base::MXmlDocument {
                v.extend(document::xml_document_methods());
            }
            if b == Base::MHtmlDocument {
                v.extend(document::html_document_methods());
            }
            v.extend(document::document_methods());
            v.extend(node::parent_methods());
            v.extend(nm());
        }
        Base::MDocumentFragment => {
            v.extend(chardata::fragment_methods());
            v.extend(node::parent_methods());
            v.extend(nm());
        }
        Base::MElement | Base::MHtmlElement => {
            v.extend(element::element_methods());
            v.extend(node::parent_methods());
            v.extend(node::child_methods());
            v.extend(nm());
        }
        Base::MAttr => {
            v.extend(element::attr_methods());
            v.extend(nm());
        }
        Base::MCharacterData | Base::MComment | Base::MProcessingInstruction => {
            v.extend(chardata::chardata_methods());
            v.extend(node::child_methods());
            v.extend(nm());
        }
        Base::MText | Base::MCdataSection => {
            v.extend(chardata::text_methods());
            v.extend(chardata::chardata_methods());
            v.extend(node::child_methods());
            v.extend(nm());
        }
        Base::MDocumentType => {
            v.extend(node::child_methods());
            v.extend(nm());
        }
        Base::MNotation | Base::MEntity | Base::MEntityReference => v.extend(nm()),
        Base::MImplementation => v.extend(document::implementation_methods()),
        Base::MNodeList | Base::MNamedNodeMap | Base::MDtdNamedNodeMap | Base::MHtmlCollection => {
            v.extend(collections::methods(b))
        }
        Base::MXPath => v.extend(xpath_methods()),
        Base::MTokenList => v.extend(tokenlist::methods()),
        _ => {}
    }
    v
}

fn xpath_methods() -> Vec<(&'static str, MethodFn)> {
    super::xpathobj::xpath_methods()
        .into_iter()
        .filter(|(n, _)| *n != "quote")
        .collect()
}

pub fn declaring_class(b: Base, m: &str) -> &'static str {
    let lm = m.to_ascii_lowercase();
    let in_tbl =
        |tbl: Vec<(&'static str, MethodFn)>| tbl.iter().any(|(n, _)| n.eq_ignore_ascii_case(&lm));
    match b {
        Base::MXmlDocument if in_tbl(document::xml_document_methods()) => "Dom\\XMLDocument",
        Base::MHtmlDocument if in_tbl(document::html_document_methods()) => "Dom\\HTMLDocument",
        Base::MDocument | Base::MXmlDocument | Base::MHtmlDocument
            if in_tbl(document::document_methods()) || in_tbl(node::parent_methods()) =>
        {
            "Dom\\Document"
        }
        Base::MDocumentFragment
            if in_tbl(chardata::fragment_methods()) || in_tbl(node::parent_methods()) =>
        {
            "Dom\\DocumentFragment"
        }
        Base::MElement | Base::MHtmlElement
            if in_tbl(element::element_methods())
                || in_tbl(node::parent_methods())
                || in_tbl(node::child_methods()) =>
        {
            "Dom\\Element"
        }
        Base::MAttr if in_tbl(element::attr_methods()) => "Dom\\Attr",
        Base::MText | Base::MCdataSection if in_tbl(chardata::text_methods()) => "Dom\\Text",
        Base::MText
        | Base::MCdataSection
        | Base::MCharacterData
        | Base::MComment
        | Base::MProcessingInstruction
            if in_tbl(chardata::chardata_methods()) || in_tbl(node::child_methods()) =>
        {
            "Dom\\CharacterData"
        }
        Base::MDocumentType if in_tbl(node::child_methods()) => "Dom\\DocumentType",
        Base::MImplementation => "Dom\\Implementation",
        Base::MNodeList => "Dom\\NodeList",
        Base::MNamedNodeMap => "Dom\\NamedNodeMap",
        Base::MDtdNamedNodeMap => "Dom\\DtdNamedNodeMap",
        Base::MHtmlCollection => "Dom\\HTMLCollection",
        Base::MXPath => "Dom\\XPath",
        Base::MTokenList => "Dom\\TokenList",
        _ => "Dom\\Node",
    }
}

pub fn is_static_method(b: Base, m: &str) -> bool {
    match b {
        Base::MXmlDocument => matches!(m, "createempty" | "createfromstring" | "createfromfile"),
        Base::MHtmlDocument => matches!(m, "createempty" | "createfromstring" | "createfromfile"),
        _ => false,
    }
}

pub fn static_display(m: &str) -> &'static str {
    match m {
        "createempty" => "createEmpty",
        "createfromstring" => "createFromString",
        "createfromfile" => "createFromFile",
        _ => "",
    }
}

pub fn call_static(vm: &mut Vm, b: Base, m: &str, a: &[Value]) -> R<Value> {
    match (b, m) {
        (Base::MXmlDocument, "createempty") => document::xml_create_empty(vm, a),
        (Base::MXmlDocument, "createfromstring") => document::xml_load(vm, a, false),
        (Base::MXmlDocument, "createfromfile") => document::xml_load(vm, a, true),
        (Base::MHtmlDocument, "createempty") => document::html_create_empty(vm, a),
        (Base::MHtmlDocument, _) => {
            throw_error(vm, b"Error", "Dom\\HTMLDocument parsing is not supported");
            Err(())
        }
        _ => Ok(Value::Null),
    }
}

pub fn param_names(b: Base, method: &[u8]) -> Option<&'static [&'static str]> {
    Some(match (b, method) {
        (Base::MXmlDocument, b"createempty") => &["version", "encoding"],
        (Base::MHtmlDocument, b"createempty") => &["encoding"],
        (_, b"createfromstring") => &["source", "options", "overrideEncoding"],
        (_, b"createfromfile") => &["path", "options", "overrideEncoding"],
        (_, b"savexml") => &["node", "options"],
        (_, b"savexmlfile") => &["filename", "options"],
        (_, b"createelement") => &["localName"],
        (_, b"createelementns") => &["namespace", "qualifiedName"],
        (_, b"createattributens") => &["namespace", "qualifiedName"],
        (_, b"createattribute") => &["localName"],
        (_, b"importnode") | (_, b"importlegacynode") => &["node", "deep"],
        (_, b"createdocumenttype") => &["qualifiedName", "publicId", "systemId"],
        (_, b"createdocument") => &["namespace", "qualifiedName", "doctype"],
        (_, b"createhtmldocument") => &["title"],
        (_, b"toggle") => &["token", "force"],
        (_, b"replace") => &["token", "newToken"],
        (_, b"rename") => &["namespaceURI", "qualifiedName"],
        (_, b"insertadjacentelement") => &["where", "element"],
        (_, b"insertadjacenttext") => &["where", "data"],
        (_, b"insertadjacenthtml") => &["where", "string"],
        (_, b"getrootnode") => &["options"],
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Registration

fn class_entry(name: &str, parent: Option<&str>, interfaces: &[&str]) -> ClassEntry {
    let mut ce = ClassEntry::new(name.as_bytes().to_vec());
    ce.parent = parent.map(|p| p.as_bytes().to_vec());
    ce.interfaces = interfaces.iter().map(|s| s.as_bytes().to_vec()).collect();
    ce
}

pub fn register_classes(vm: &mut Vm) {
    // Dom\DOMException is an alias of DOMException
    if let Some(exc) = vm.classes.get(&b"domexception".to_vec()).cloned() {
        vm.classes.insert(b"dom\\domexception".to_vec(), exc);
        vm.class_aliases
            .insert(b"dom\\domexception".to_vec(), b"domexception".to_vec());
        vm.class_aliases
            .insert(b"domexception".to_vec(), b"dom\\domexception".to_vec());
    }
    let mut pn = class_entry("Dom\\ParentNode", None, &[]);
    pn.is_interface = true;
    vm.classes.insert(b"dom\\parentnode".to_vec(), pn);
    let mut cn = class_entry("Dom\\ChildNode", None, &[]);
    cn.is_interface = true;
    vm.classes.insert(b"dom\\childnode".to_vec(), cn);

    for (name, b, parent) in CLASSES {
        if !b.is_modern() {
            continue;
        }
        let ifaces: &[&str] = match b {
            Base::MDocument | Base::MDocumentFragment => &["Dom\\ParentNode"],
            Base::MElement => &["Dom\\ParentNode", "Dom\\ChildNode"],
            Base::MCharacterData | Base::MDocumentType => &["Dom\\ChildNode"],
            Base::MNodeList
            | Base::MNamedNodeMap
            | Base::MDtdNamedNodeMap
            | Base::MHtmlCollection
            | Base::MTokenList => &["IteratorAggregate", "Traversable", "Countable"],
            _ => &[],
        };
        let mut ce = class_entry(name, *parent, ifaces);
        if *b == Base::MNode {
            for (cname, v) in [
                ("DOCUMENT_POSITION_DISCONNECTED", 1),
                ("DOCUMENT_POSITION_PRECEDING", 2),
                ("DOCUMENT_POSITION_FOLLOWING", 4),
                ("DOCUMENT_POSITION_CONTAINS", 8),
                ("DOCUMENT_POSITION_CONTAINED_BY", 16),
                ("DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC", 32),
            ] {
                ce.constants
                    .insert(cname.as_bytes().to_vec(), Value::Long(v));
            }
        }
        if *b == Base::MDocument {
            ce.is_abstract = true;
        }
        if matches!(
            b,
            Base::MXmlDocument | Base::MHtmlDocument | Base::MTokenList | Base::MXPath
        ) {
            ce.is_final = true;
        }
        vm.classes
            .insert(name.to_ascii_lowercase().into_bytes(), ce);
    }

    // Dom\NamespaceInfo: a plain readonly final class
    {
        use goro_core::object::{PropertyDef, Visibility};
        let mut ni = class_entry("Dom\\NamespaceInfo", None, &[]);
        ni.is_final = true;
        ni.is_readonly = true;
        for pn in ["prefix", "namespaceURI", "element"] {
            ni.properties.push(PropertyDef {
                name: pn.as_bytes().to_vec(),
                default: Value::Undef,
                is_static: false,
                is_readonly: true,
                visibility: Visibility::Public,
                set_visibility: None,
                declaring_class: b"dom\\namespaceinfo".to_vec(),
                property_type: None,
                has_get_hook: false,
                has_set_hook: false,
                is_virtual: false,
                is_abstract: false,
                is_final: false,
                attributes: Vec::new(),
                doc_comment: None,
            });
        }
        vm.classes.insert(b"dom\\namespaceinfo".to_vec(), ni);
    }

    // Dom\AdjacentPosition: string-backed enum
    let mut ap = class_entry("Dom\\AdjacentPosition", None, &["UnitEnum", "BackedEnum"]);
    ap.is_enum = true;
    ap.enum_backing_type = Some(b"string".to_vec());
    for (case, val) in [
        ("BeforeBegin", "beforebegin"),
        ("AfterBegin", "afterbegin"),
        ("BeforeEnd", "beforeend"),
        ("AfterEnd", "afterend"),
    ] {
        ap.enum_cases.push((
            case.as_bytes().to_vec(),
            Value::String(PhpString::from_bytes(val.as_bytes())),
        ));
        let mut marker = b"__enum_case__::".to_vec();
        marker.extend_from_slice(case.as_bytes());
        ap.constants.insert(
            case.as_bytes().to_vec(),
            Value::String(PhpString::from_vec(marker)),
        );
    }
    vm.classes.insert(b"dom\\adjacentposition".to_vec(), ap);
}

pub fn register_rest(vm: &mut Vm) {
    for (n, v) in [
        ("INDEX_SIZE_ERR", 1),
        ("STRING_SIZE_ERR", 2),
        ("HIERARCHY_REQUEST_ERR", 3),
        ("WRONG_DOCUMENT_ERR", 4),
        ("INVALID_CHARACTER_ERR", 5),
        ("NO_DATA_ALLOWED_ERR", 6),
        ("NO_MODIFICATION_ALLOWED_ERR", 7),
        ("NOT_FOUND_ERR", 8),
        ("NOT_SUPPORTED_ERR", 9),
        ("INUSE_ATTRIBUTE_ERR", 10),
        ("INVALID_STATE_ERR", 11),
        ("SYNTAX_ERR", 12),
        ("INVALID_MODIFICATION_ERR", 13),
        ("NAMESPACE_ERR", 14),
        ("VALIDATION_ERR", 16),
        ("HTML_NO_DEFAULT_NS", 1 << 31),
    ] {
        vm.constants
            .insert(format!("Dom\\{}", n).into_bytes(), Value::Long(v));
    }
    vm.register_function_with_params(
        b"Dom\\import_simplexml",
        element::import_simplexml,
        &[b"node"],
    );
}

pub fn xml_ns() -> &'static str {
    XML_XML_NAMESPACE
}

/// In-scope namespaces of `n` as (prefix, uri) pairs for XPath evaluation
/// (php_dom_get_in_scope_ns with the modern mapper).
pub fn xpath_ns_list(doc: NodeId, n: NodeId) -> Vec<(String, String)> {
    tree::with(|t| {
        in_scope_ns(t, doc, n, false)
            .into_iter()
            .filter_map(|ns| {
                t.nss[ns]
                    .prefix
                    .clone()
                    .map(|p| (p, t.nss[ns].href.clone()))
            })
            .collect()
    })
}
