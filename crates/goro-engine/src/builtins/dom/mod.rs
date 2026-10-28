//! ext/dom (PHP manual: DOM; RFCs "DOM HTML5 parsing and serialization"
//! and "Opt-in DOM spec-compliance"): the legacy `DOM*` classes and the
//! PHP 8.4 `Dom\` namespace, on goro-xml's tree in the shared node store
//! ([`super::xmlstore`]).
//!
//! Node objects: one PHP object per node while PHP refers to it (the
//! store's `dom_objs`), holding the node ([`DomObj`]). Its class comes from
//! the node kind, the document flavour (legacy or `Dom\`) and the classes
//! the document registered (`registerNodeClass()`). Properties are hooked
//! properties of the stubs whose hooks run natively ([`props`]).

pub mod chardata;
pub mod document;
pub mod element;
pub mod lists;
pub mod modern;
pub mod node;
pub mod props;
pub mod stubs;
pub mod xpath;

use std::rc::Rc;

use goro_runtime::{NativeData, ObjRef, PhpStr, Value};
use goro_xml::tree::{DocKind, DomError, NodeData};
use goro_xml::{Arena, NodeId, NodeType};

use super::xmlstore::{self, NodeHold};
use crate::class::{Class, NativeHooks};
use crate::func::{Args, NativeFn};
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "dom",
    functions: &[
        (
            "function dom_import_simplexml(object $node): DOMAttr|DOMElement",
            document::dom_import_simplexml,
        ),
        (
            "function Dom\\import_simplexml(object $node): Dom\\Attr|Dom\\Element",
            document::dom_import_simplexml,
        ),
    ],
    classes: CLASSES,
    methods: &[],
    hooks: &[
        ("domnode", node_hooks),
        ("dom\\node", node_hooks),
        ("domnamespacenode", node_hooks),
        ("domnodelist", lists::list_hooks),
        ("dom\\nodelist", lists::list_hooks),
        ("domnamednodemap", lists::list_hooks),
        ("dom\\namednodemap", lists::list_hooks),
        ("dom\\dtdnamednodemap", lists::list_hooks),
        ("dom\\htmlcollection", lists::list_hooks),
        ("dom\\tokenlist", lists::list_hooks),
        ("domxpath", xpath::xpath_hooks),
        ("dom\\xpath", xpath::xpath_hooks),
        ("domimplementation", plain_hooks),
        ("dom\\implementation", plain_hooks),
        ("dom\\namespaceinfo", plain_hooks),
    ],
    constants: CONSTANTS,
    init: Some(init),
    ..Module::EMPTY
};

const CLASSES: &[(&str, &str)] = &{
    const N: usize = stubs::CLASSES.len() + 1;
    let mut out: [(&str, &str); N] = [("", ""); N];
    let mut i = 0;
    while i < stubs::CLASSES.len() {
        out[i] = stubs::CLASSES[i];
        i += 1;
    }
    out[i] = ("dom\\domexception", "alias:DOMException");
    out
};

fn init(vm: &mut Vm) {
    let tables: [&[(&str, NativeFn)]; 8] = [
        node::METHODS,
        document::METHODS,
        element::METHODS,
        chardata::METHODS,
        lists::METHODS,
        xpath::METHODS,
        modern::METHODS,
        props::METHODS,
    ];
    for t in tables {
        for &(k, h) in t {
            vm.native_methods.insert(PhpStr::intern(k.as_bytes()), h);
        }
    }
}

const CONSTANTS: &[(&str, ConstVal)] = &[
    ("XML_ELEMENT_NODE", ConstVal::Int(1)),
    ("XML_ATTRIBUTE_NODE", ConstVal::Int(2)),
    ("XML_TEXT_NODE", ConstVal::Int(3)),
    ("XML_CDATA_SECTION_NODE", ConstVal::Int(4)),
    ("XML_ENTITY_REF_NODE", ConstVal::Int(5)),
    ("XML_ENTITY_NODE", ConstVal::Int(6)),
    ("XML_PI_NODE", ConstVal::Int(7)),
    ("XML_COMMENT_NODE", ConstVal::Int(8)),
    ("XML_DOCUMENT_NODE", ConstVal::Int(9)),
    ("XML_DOCUMENT_TYPE_NODE", ConstVal::Int(10)),
    ("XML_DOCUMENT_FRAG_NODE", ConstVal::Int(11)),
    ("XML_NOTATION_NODE", ConstVal::Int(12)),
    ("XML_HTML_DOCUMENT_NODE", ConstVal::Int(13)),
    ("XML_DTD_NODE", ConstVal::Int(14)),
    ("XML_ELEMENT_DECL_NODE", ConstVal::Int(15)),
    ("XML_ATTRIBUTE_DECL_NODE", ConstVal::Int(16)),
    ("XML_ENTITY_DECL_NODE", ConstVal::Int(17)),
    ("XML_NAMESPACE_DECL_NODE", ConstVal::Int(18)),
    ("XML_LOCAL_NAMESPACE", ConstVal::Int(18)),
    ("XML_ATTRIBUTE_CDATA", ConstVal::Int(1)),
    ("XML_ATTRIBUTE_ID", ConstVal::Int(2)),
    ("XML_ATTRIBUTE_IDREF", ConstVal::Int(3)),
    ("XML_ATTRIBUTE_IDREFS", ConstVal::Int(4)),
    ("XML_ATTRIBUTE_ENTITY", ConstVal::Int(6)),
    ("XML_ATTRIBUTE_NMTOKEN", ConstVal::Int(7)),
    ("XML_ATTRIBUTE_NMTOKENS", ConstVal::Int(8)),
    ("XML_ATTRIBUTE_ENUMERATION", ConstVal::Int(9)),
    ("XML_ATTRIBUTE_NOTATION", ConstVal::Int(10)),
    ("DOM_PHP_ERR", ConstVal::Int(0)),
    ("DOM_INDEX_SIZE_ERR", ConstVal::Int(1)),
    ("DOMSTRING_SIZE_ERR", ConstVal::Int(2)),
    ("DOM_HIERARCHY_REQUEST_ERR", ConstVal::Int(3)),
    ("DOM_WRONG_DOCUMENT_ERR", ConstVal::Int(4)),
    ("DOM_INVALID_CHARACTER_ERR", ConstVal::Int(5)),
    ("DOM_NO_DATA_ALLOWED_ERR", ConstVal::Int(6)),
    ("DOM_NO_MODIFICATION_ALLOWED_ERR", ConstVal::Int(7)),
    ("DOM_NOT_FOUND_ERR", ConstVal::Int(8)),
    ("DOM_NOT_SUPPORTED_ERR", ConstVal::Int(9)),
    ("DOM_INUSE_ATTRIBUTE_ERR", ConstVal::Int(10)),
    ("DOM_INVALID_STATE_ERR", ConstVal::Int(11)),
    ("DOM_SYNTAX_ERR", ConstVal::Int(12)),
    ("DOM_INVALID_MODIFICATION_ERR", ConstVal::Int(13)),
    ("DOM_NAMESPACE_ERR", ConstVal::Int(14)),
    ("DOM_INVALID_ACCESS_ERR", ConstVal::Int(15)),
    ("DOM_VALIDATION_ERR", ConstVal::Int(16)),
    ("Dom\\INDEX_SIZE_ERR", ConstVal::Int(1)),
    ("Dom\\STRING_SIZE_ERR", ConstVal::Int(2)),
    ("Dom\\HIERARCHY_REQUEST_ERR", ConstVal::Int(3)),
    ("Dom\\WRONG_DOCUMENT_ERR", ConstVal::Int(4)),
    ("Dom\\INVALID_CHARACTER_ERR", ConstVal::Int(5)),
    ("Dom\\NO_DATA_ALLOWED_ERR", ConstVal::Int(6)),
    ("Dom\\NO_MODIFICATION_ALLOWED_ERR", ConstVal::Int(7)),
    ("Dom\\NOT_FOUND_ERR", ConstVal::Int(8)),
    ("Dom\\NOT_SUPPORTED_ERR", ConstVal::Int(9)),
    ("Dom\\INUSE_ATTRIBUTE_ERR", ConstVal::Int(10)),
    ("Dom\\INVALID_STATE_ERR", ConstVal::Int(11)),
    ("Dom\\SYNTAX_ERR", ConstVal::Int(12)),
    ("Dom\\INVALID_MODIFICATION_ERR", ConstVal::Int(13)),
    ("Dom\\NAMESPACE_ERR", ConstVal::Int(14)),
    ("Dom\\VALIDATION_ERR", ConstVal::Int(16)),
    ("Dom\\HTML_NO_DEFAULT_NS", ConstVal::Int(2147483648)),
];

// ------------------------------------------------------------ node objects

/// The native data of a node object.
pub struct DomObj {
    pub hold: NodeHold,
    /// Collections the object keeps (`children` and `classList` are the
    /// same object every time; observed on PHP 8.5).
    pub cached: std::cell::RefCell<Vec<(u8, ObjRef)>>,
}

impl NativeData for DomObj {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        for (_, o) in self.cached.borrow().iter() {
            visit(&Value::Object(o.clone()));
        }
    }
    fn clear(&mut self) {
        self.cached.borrow_mut().clear();
    }
}

/// Runs `f` on the native data of a node object.
pub(crate) fn with_dom_obj<T>(o: &ObjRef, f: impl FnOnce(&DomObj) -> T) -> Option<T> {
    let n = o.obj().native.borrow();
    let any: &dyn std::any::Any = &**n.as_ref()?;
    any.downcast_ref::<DomObj>().map(f)
}

impl Drop for DomObj {
    fn drop(&mut self) {
        let id = self.hold.id();
        if !xmlstore::busy() {
            xmlstore::with(|s| {
                if s.dom_objs.get(&id).is_some_and(|w| w.upgrade().is_none()) {
                    s.dom_objs.remove(&id);
                }
            });
        }
    }
}

/// Runs `f` with the store's arena.
#[inline]
pub(crate) fn with_arena<T>(f: impl FnOnce(&mut Arena) -> T) -> T {
    xmlstore::with(|s| f(&mut s.arena))
}

/// The node of a DOM node object.
pub fn node_of(o: &ObjRef) -> Option<NodeId> {
    let n = o.obj().native.borrow();
    let any: &dyn std::any::Any = &**n.as_ref()?;
    any.downcast_ref::<DomObj>().map(|d| d.hold.id())
}

/// Per-document settings of ext/dom (PHP keeps them with the document:
/// they survive the document object and apply to every object of it).
pub struct DocSettings {
    pub format_output: bool,
    pub validate_on_parse: bool,
    pub resolve_externals: bool,
    pub preserve_white_space: bool,
    pub recover: bool,
    pub substitute_entities: bool,
    pub strict_error_checking: bool,
    /// `registerNodeClass()`: (lower-case base class, class).
    pub class_map: Vec<(PhpStr, Rc<Class>)>,
    /// The class of the document object (a user subclass of DOMDocument).
    pub doc_class: Option<Rc<Class>>,
    /// Created implicitly for a node made without a document (`new
    /// DOMElement(...)`): its nodes report no owner document.
    pub hidden: bool,
}

impl Default for DocSettings {
    fn default() -> Self {
        DocSettings {
            format_output: false,
            validate_on_parse: false,
            resolve_externals: false,
            preserve_white_space: true,
            recover: false,
            substitute_entities: false,
            strict_error_checking: true,
            class_map: Vec::new(),
            doc_class: None,
            hidden: false,
        }
    }
}

impl Clone for DocSettings {
    fn clone(&self) -> Self {
        DocSettings {
            format_output: self.format_output,
            validate_on_parse: self.validate_on_parse,
            resolve_externals: self.resolve_externals,
            preserve_white_space: self.preserve_white_space,
            recover: self.recover,
            substitute_entities: self.substitute_entities,
            strict_error_checking: self.strict_error_checking,
            class_map: self.class_map.clone(),
            doc_class: self.doc_class.clone(),
            hidden: self.hidden,
        }
    }
}

/// The settings of the document owning `n` (a copy).
pub(crate) fn settings(n: NodeId) -> DocSettings {
    xmlstore::with(|s| s.doc_ext(n).get::<DocSettings>().clone())
}

/// Changes the settings of the document owning `n`.
pub(crate) fn with_settings<T>(n: NodeId, f: impl FnOnce(&mut DocSettings) -> T) -> T {
    xmlstore::with(|s| f(s.doc_ext(n).get::<DocSettings>()))
}

/// True if `n` belongs to a `Dom\` (spec-compliant) document.
pub(crate) fn is_modern_node(a: &Arena, n: NodeId) -> bool {
    let d = a.doc_of(n);
    a.doc(d).modern
}

/// The built-in class of a node.
fn builtin_class(a: &Arena, n: NodeId) -> &'static str {
    let modern = is_modern_node(a, n);
    let node = a.node(n);
    if modern {
        return match &node.data {
            NodeData::Document(d) => match d.kind {
                DocKind::Html5 => "Dom\\HTMLDocument",
                _ => "Dom\\XMLDocument",
            },
            NodeData::Fragment => "Dom\\DocumentFragment",
            NodeData::Element(e) => {
                let doc = a.doc_of(n);
                if a.doc(doc).kind == DocKind::Html5
                    && e.name.ns.as_deref() == Some(goro_xml::tree::HTML_NS)
                {
                    "Dom\\HTMLElement"
                } else {
                    "Dom\\Element"
                }
            }
            NodeData::Attribute(_) => "Dom\\Attr",
            NodeData::Text(_) => "Dom\\Text",
            NodeData::CData(_) => "Dom\\CDATASection",
            NodeData::Comment(_) => "Dom\\Comment",
            NodeData::Pi { .. } => "Dom\\ProcessingInstruction",
            NodeData::EntityRef(_) => "Dom\\EntityReference",
            NodeData::Doctype(_) => "Dom\\DocumentType",
            NodeData::EntityDecl(_) => "Dom\\Entity",
            NodeData::Notation(_) => "Dom\\Notation",
            NodeData::ElementDecl(_) | NodeData::AttributeDecl(_) => "Dom\\Node",
            NodeData::Namespace(_) => "Dom\\Node",
        };
    }
    match &node.data {
        NodeData::Document(_) => "DOMDocument",
        NodeData::Fragment => "DOMDocumentFragment",
        NodeData::Element(_) => "DOMElement",
        NodeData::Attribute(_) => "DOMAttr",
        NodeData::Text(_) => "DOMText",
        NodeData::CData(_) => "DOMCdataSection",
        NodeData::Comment(_) => "DOMComment",
        NodeData::Pi { .. } => "DOMProcessingInstruction",
        NodeData::EntityRef(_) => "DOMEntityReference",
        NodeData::Doctype(_) => "DOMDocumentType",
        NodeData::EntityDecl(_) => "DOMEntity",
        NodeData::Notation(_) => "DOMNotation",
        NodeData::ElementDecl(_) | NodeData::AttributeDecl(_) => "DOMNode",
        NodeData::Namespace(_) => "DOMNameSpaceNode",
    }
}

/// The PHP object of node `id` (the existing one while it lives).
pub fn wrap_node(vm: &mut Vm, id: NodeId) -> R<Value> {
    let (existing, base, mapped) = xmlstore::with(|s| {
        let existing = s.dom_objs.get(&id).and_then(|w| w.upgrade());
        if existing.is_some() {
            return (existing, "", None);
        }
        let base = builtin_class(&s.arena, id);
        let is_doc = matches!(s.arena.data(id), NodeData::Document(_));
        let st = s.doc_ext(id).get::<DocSettings>();
        let mapped = if is_doc && st.doc_class.is_some() {
            st.doc_class.clone()
        } else {
            let lb = base.to_ascii_lowercase();
            st.class_map
                .iter()
                .find(|(b, _)| b.as_bytes() == lb.as_bytes())
                .map(|(_, c)| c.clone())
        };
        (None, base, mapped)
    });
    if let Some(o) = existing {
        return Ok(Value::Object(o));
    }
    let cls = match mapped {
        Some(c) => c,
        None => vm
            .find_class(base.as_bytes(), false)?
            .expect("DOM class is declared"),
    };
    let o = vm.instantiate_raw(&cls)?;
    attach(&o, id);
    Ok(Value::Object(o))
}

/// Makes `o` the object of node `id`.
pub(crate) fn attach(o: &ObjRef, id: NodeId) {
    let hold = xmlstore::with(|s| {
        s.dom_objs.insert(id, o.downgrade());
        s.hold(id)
    });
    let old = o.obj().native.borrow_mut().replace(Box::new(DomObj {
        hold,
        cached: Default::default(),
    }));
    drop(old);
}

/// [`wrap_node`] of an optional node (null for none).
pub(crate) fn wrap_opt(vm: &mut Vm, id: Option<NodeId>) -> R<Value> {
    match id {
        Some(n) => wrap_node(vm, n),
        None => Ok(Value::Null),
    }
}

/// The node of `$this`; "Couldn't fetch C" when the object has none (made
/// by a subclass constructor not calling the parent's; observed on PHP
/// 8.5).
pub(crate) fn this_node(vm: &mut Vm, a: &Args) -> R<NodeId> {
    let o = a.this_obj();
    match node_of(o) {
        Some(n) => Ok(n),
        None => {
            let c = vm.class_of(o).name.to_string_lossy();
            vm.throw("Error", &format!("Couldn't fetch {c}"))
        }
    }
}

/// The node of object argument `i`.
pub(crate) fn arg_node(vm: &mut Vm, a: &Args, i: usize) -> R<NodeId> {
    let Some(o) = a.obj(i) else {
        return vm.throw("Error", "Couldn't fetch node");
    };
    match node_of(&o) {
        Some(n) => Ok(n),
        None => {
            let c = vm.class_of(&o).name.to_string_lossy();
            vm.throw("Error", &format!("Couldn't fetch {c}"))
        }
    }
}

/// The node of optional object argument `i` (None when null or absent).
pub(crate) fn opt_arg_node(vm: &mut Vm, a: &Args, i: usize) -> R<Option<NodeId>> {
    match a.val(i) {
        Value::Object(_) => arg_node(vm, a, i).map(Some),
        _ => Ok(None),
    }
}

/// True for a `Dom\` method or property (the declaring class's namespace).
pub(crate) fn modern_fn(a: &Args) -> bool {
    a.func
        .scope()
        .is_some_and(|c| c.name.as_bytes().starts_with(b"Dom\\"))
}

/// Throws a DOMException.
pub(crate) fn throw_dom<T>(vm: &mut Vm, e: DomError) -> R<T> {
    if e == DomError::HierarchyRequest
        && let Some(m) = node::DETAIL.with(|d| d.take())
    {
        return throw_dom_msg(vm, m, e.code() as i64);
    }
    node::DETAIL.with(|d| d.set(None));
    throw_dom_msg(vm, e.message(), e.code() as i64)
}

/// Throws a DOMException with a message and code.
pub(crate) fn throw_dom_msg<T>(vm: &mut Vm, msg: &str, code: i64) -> R<T> {
    let o = vm.create_exception("DOMException", msg, code)?;
    if code == 0 {
        vm.set_prop_raw(&o, b"code", Value::Int(0));
    }
    Err(crate::vm::Unwind::Throw(o))
}

/// A legacy DOM error: a DOMException, or with `strictErrorChecking` off,
/// a warning and `false` (PHP manual: DOMDocument::$strictErrorChecking).
pub(crate) fn legacy_error(vm: &mut Vm, a: &Args, n: NodeId, e: DomError) -> R<Value> {
    if settings(n).strict_error_checking {
        return throw_dom(vm, e);
    }
    vm.fwarn(a, e.message())?;
    Ok(Value::Bool(false))
}

thread_local! {
    /// Attributes made non-ID with `setIdAttribute*(..., false)`, by
    /// document (an `id` attribute of a `Dom\` or HTML document is an ID
    /// otherwise).
    pub(crate) static NOT_ID: std::cell::RefCell<std::collections::HashMap<NodeId, std::collections::HashSet<NodeId>>> = Default::default();
}

/// True if attribute `at` is an ID (DTD-declared or set with
/// `setIdAttribute()`, `xml:id`, or `id` in `Dom\` and HTML documents;
/// tests bug79701/*.phpt).
pub(crate) fn is_id_attr(s: &xmlstore::Store, at: NodeId) -> bool {
    let a = &s.arena;
    let doc = a.doc_of(at);
    if NOT_ID.with(|m| m.borrow().get(&doc).is_some_and(|set| set.contains(&at))) {
        return false;
    }
    let Some(d) = a.attr(at) else {
        return false;
    };
    if d.is_id {
        return true;
    }
    let q = &d.name;
    if q.ns.as_deref() == Some(goro_xml::tree::XML_NS) && &*q.local == "id" {
        return true;
    }
    let dd = a.doc(doc);
    q.ns.is_none() && &*q.local == "id" && (dd.modern || dd.kind == DocKind::Html)
}

/// The node type number PHP reports.
pub(crate) fn node_type_num(a: &Arena, n: NodeId) -> i64 {
    let t = a.node_type(n);
    if t == NodeType::Document && matches!(a.doc(n).kind, DocKind::Html | DocKind::Html5) {
        return 13;
    }
    t as i64
}

/// UTF-8 text of a PHP string (goro-xml works on `str`).
pub(crate) fn s8(s: &PhpStr) -> String {
    String::from_utf8_lossy(s.as_bytes()).into_owned()
}

/// A PHP string of text.
pub(crate) fn sv(s: &str) -> Value {
    Value::from(s.as_bytes())
}

/// A PHP string or null.
pub(crate) fn sv_opt(s: Option<&str>) -> Value {
    s.map_or(Value::Null, sv)
}

fn node_hooks() -> NativeHooks {
    NativeHooks {
        debug_props: Some(props::debug_props),
        props_for: Some(|vm, o| Some(vm.plain_props_array(o, true))),
        clone: Some(node::clone_hook),
        ..Default::default()
    }
}

fn plain_hooks() -> NativeHooks {
    NativeHooks::default()
}
