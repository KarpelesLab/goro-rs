//! ext/dom (legacy DOM classes), ported from php-src/ext/dom on top of the
//! tree in `tree.rs`.

pub mod chardata;
pub mod document;
pub mod element;
pub mod lists;
pub mod modern;
pub mod node;
pub mod xpathobj;

use crate::tree::{self, NType, NodeId, Tree};
use goro_core::array::PhpArray;
use goro_core::object::{ClassEntry, NativeData, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, NativeObjectHandlers, Vm};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

pub type Obj = Rc<RefCell<PhpObject>>;

// ---------------------------------------------------------------------------
// Exception codes

pub const INDEX_SIZE_ERR: i64 = 1;
pub const DOMSTRING_SIZE_ERR: i64 = 2;
pub const HIERARCHY_REQUEST_ERR: i64 = 3;
pub const WRONG_DOCUMENT_ERR: i64 = 4;
pub const INVALID_CHARACTER_ERR: i64 = 5;
pub const NO_DATA_ALLOWED_ERR: i64 = 6;
pub const NO_MODIFICATION_ALLOWED_ERR: i64 = 7;
pub const NOT_FOUND_ERR: i64 = 8;
pub const NOT_SUPPORTED_ERR: i64 = 9;
pub const INUSE_ATTRIBUTE_ERR: i64 = 10;
pub const INVALID_STATE_ERR: i64 = 11;
pub const SYNTAX_ERR: i64 = 12;
pub const INVALID_MODIFICATION_ERR: i64 = 13;
pub const NAMESPACE_ERR: i64 = 14;
pub const INVALID_ACCESS_ERR: i64 = 15;
pub const VALIDATION_ERR: i64 = 16;

pub const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";

pub fn error_message(code: i64) -> &'static str {
    match code {
        INDEX_SIZE_ERR => "Index Size Error",
        DOMSTRING_SIZE_ERR => "DOM String Size Error",
        HIERARCHY_REQUEST_ERR => "Hierarchy Request Error",
        WRONG_DOCUMENT_ERR => "Wrong Document Error",
        INVALID_CHARACTER_ERR => "Invalid Character Error",
        NO_DATA_ALLOWED_ERR => "No Data Allowed Error",
        NO_MODIFICATION_ALLOWED_ERR => "No Modification Allowed Error",
        NOT_FOUND_ERR => "Not Found Error",
        NOT_SUPPORTED_ERR => "Not Supported Error",
        INUSE_ATTRIBUTE_ERR => "Inuse Attribute Error",
        INVALID_STATE_ERR => "Invalid State Error",
        SYNTAX_ERR => "Syntax Error",
        INVALID_MODIFICATION_ERR => "Invalid Modification Error",
        NAMESPACE_ERR => "Namespace Error",
        INVALID_ACCESS_ERR => "Invalid Access Error",
        VALIDATION_ERR => "Validation Error",
        _ => "Unhandled Error",
    }
}

/// php_dom_throw_error_with_message
pub fn throw_dom_msg(vm: &mut Vm, code: i64, msg: &str, strict: bool) {
    if strict {
        let e = vm.create_exception(b"DOMException", msg, vm.current_line);
        if let Value::Object(o) = &e {
            o.borrow_mut()
                .set_property(b"code".to_vec(), Value::Long(code));
        }
        vm.current_exception = Some(e);
    } else {
        let f = active_fn(vm);
        crate::libxml::issue_error(vm, &f, msg);
    }
}

pub fn throw_dom(vm: &mut Vm, code: i64, strict: bool) {
    throw_dom_msg(vm, code, error_message(code), strict);
}

pub fn throw_error(vm: &mut Vm, class: &[u8], msg: &str) {
    let e = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(e);
}

/// Active function name as php_error_docref prints it.
pub fn active_fn(vm: &Vm) -> String {
    CURRENT_FN.with(|c| c.borrow().clone()).unwrap_or_else(|| {
        vm.call_stack
            .last()
            .map(|f| f.0.clone())
            .unwrap_or_else(|| "main".to_string())
    })
}

thread_local! {
    static CURRENT_FN: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Emit a warning prefixed with the current DOM method name.
pub fn warn(vm: &mut Vm, msg: &str) {
    let f = active_fn(vm);
    vm.emit_warning(&format!("{}(): {}", f, msg));
}

// ---------------------------------------------------------------------------
// Native data

#[derive(Clone)]
pub enum ListKind {
    /// childNodes of a node (the base object: its node may change, e.g. on
    /// DOMDocument::loadXML)
    Children(Obj),
    /// getElementsByTagName(NS): base, namespace (None = any), localname
    ByTag {
        base: Obj,
        ns: Option<String>,
        local: String,
    },
    /// static list of nodes (XPath results, etc.)
    Static(Vec<Value>),
    /// element children (Dom\ParentNode::$children)
    ChildElements(Obj),
    /// Dom getElementsByTagName: qualified name matching ("*" = any)
    ByQName { base: Obj, qname: String },
    /// getElementsByClassName: every class must be present
    ByClass {
        base: Obj,
        classes: Vec<String>,
        quirks: bool,
    },
    /// entity reference children / empty
    Empty,
}

#[derive(Clone)]
pub enum MapKind {
    Attributes(Obj),
    Entities(Obj),
    Notations(Obj),
}

pub enum Kind {
    /// Not yet initialized (constructor not run)
    None,
    Node(NodeId),
    List(ListKind),
    Map(MapKind),
    XPath(RefCell<xpathobj::XPathState>),
    Implementation,
    /// DOMNodeList / DOMNamedNodeMap iterator
    Iter(RefCell<lists::IterState>),
    /// Dom\TokenList and other modern-only objects
    Modern(modern::MKind),
}

pub struct DomData {
    pub kind: RefCell<Kind>,
    /// Cached property objects ([SameObject] attributes such as
    /// Dom\Element::$classList or Dom\Document::$implementation).
    pub cache: RefCell<Vec<(&'static str, CacheEntry)>>,
}

pub enum CacheEntry {
    Strong(Obj),
    Weak(Weak<RefCell<PhpObject>>),
}

thread_local! {
    /// Nodes whose wrapper died while the tree was borrowed.
    static PENDING_FREE: RefCell<Vec<NodeId>> = const { RefCell::new(Vec::new()) };
}

impl Drop for DomData {
    fn drop(&mut self) {
        if let Kind::Node(n) = self.kind.get_mut() {
            let n = *n;
            if tree::try_with(|t| free_detached(t, n)).is_none() {
                let _ = PENDING_FREE.try_with(|p| p.borrow_mut().push(n));
            }
        }
    }
}

/// php_libxml_node_free_resource: when the last reference to a detached
/// (sub)tree goes away the tree is freed; descendants that still have a PHP
/// object are unlinked from it and survive on their own.
fn free_detached(t: &mut Tree, n: NodeId) {
    let pending: Vec<NodeId> = PENDING_FREE
        .try_with(|p| std::mem::take(&mut *p.borrow_mut()))
        .unwrap_or_default();
    for p in pending.into_iter().chain(std::iter::once(n)) {
        free_detached_one(t, p);
    }
}

fn alive(t: &Tree, n: NodeId) -> bool {
    t.nodes[n]
        .wrapper
        .as_ref()
        .map(|w| w.strong_count() > 0)
        .unwrap_or(false)
}

fn free_detached_one(t: &mut Tree, n: NodeId) {
    if n >= t.nodes.len() || t.nodes[n].parent.is_some() || t.is_doc(n) || alive(t, n) {
        return;
    }
    if matches!(
        t.nodes[n].t,
        NType::NamespaceDecl | NType::EntityDecl | NType::Entity | NType::Notation
    ) {
        return;
    }
    let mut stack: Vec<NodeId> = Vec::new();
    let push_kids = |t: &Tree, x: NodeId, stack: &mut Vec<NodeId>| {
        if t.nodes[x].t != NType::EntityRef {
            stack.extend(t.children_of(x));
        }
        if t.nodes[x].t == NType::Element {
            stack.extend(t.attrs_of(x));
        }
    };
    push_kids(t, n, &mut stack);
    while let Some(x) = stack.pop() {
        if alive(t, x) {
            t.unlink(x);
            if t.nodes[x].t == NType::Element {
                // keep the namespaces it uses declared within the survivor
                node::reconciliate_ns(t, x);
            }
            continue;
        }
        push_kids(t, x, &mut stack);
    }
}

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    read_property: Some(h_read_property),
    write_property: Some(h_write_property),
    has_property: Some(h_has_property),
    unset_property: Some(h_unset_property),
    read_dimension: Some(lists::h_read_dimension),
    write_dimension: None,
    has_dimension: Some(lists::h_has_dimension),
    unset_dimension: None,
    get_properties: Some(h_get_properties),
    count_elements: Some(lists::h_count),
    cast: None,
    compare: None,
    clone_obj: Some(h_clone),
};

pub fn data_of(o: &Obj) -> Option<Rc<DomData>> {
    let b = o.borrow();
    let n = b.native.as_ref()?;
    n.data.clone().downcast::<DomData>().ok()
}

pub fn node_of(o: &Obj) -> Option<NodeId> {
    let d = data_of(o)?;
    let k = d.kind.borrow();
    match &*k {
        Kind::Node(n) => Some(*n),
        _ => None,
    }
}

pub fn attach(o: &Obj, kind: Kind) {
    o.borrow_mut().native = Some(NativeData {
        data: Rc::new(DomData {
            kind: RefCell::new(kind),
            cache: RefCell::new(Vec::new()),
        }) as Rc<dyn Any>,
        handlers: &HANDLERS,
    });
}

/// Attach native data with a specific handler table (modern DOM objects).
pub fn attach_with(o: &Obj, kind: Kind, handlers: &'static NativeObjectHandlers) {
    o.borrow_mut().native = Some(NativeData {
        data: Rc::new(DomData {
            kind: RefCell::new(kind),
            cache: RefCell::new(Vec::new()),
        }) as Rc<dyn Any>,
        handlers,
    });
}

/// Dispatch helpers shared with the modern handler tables.
pub fn h_read_property_pub(vm: &mut Vm, o: &Obj, name: &[u8], mode: u8) -> Option<Value> {
    h_read_property(vm, o, name, mode)
}
pub fn h_write_property_pub(vm: &mut Vm, o: &Obj, name: &[u8], v: &Value) -> bool {
    h_write_property(vm, o, name, v)
}
pub fn h_has_property_pub(vm: &mut Vm, o: &Obj, name: &[u8], check_empty: u8) -> Option<bool> {
    h_has_property(vm, o, name, check_empty)
}
pub fn h_unset_property_pub(vm: &mut Vm, o: &Obj, name: &[u8]) -> bool {
    h_unset_property(vm, o, name)
}
pub fn h_get_properties_pub(vm: &mut Vm, o: &Obj, purpose: u8) -> Option<PhpArray> {
    h_get_properties(vm, o, purpose)
}

// ---------------------------------------------------------------------------
// Classes

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Base {
    Node,
    Document,
    DocumentFragment,
    Element,
    Attr,
    CharacterData,
    Text,
    Comment,
    CdataSection,
    DocumentType,
    Notation,
    Entity,
    EntityReference,
    ProcessingInstruction,
    NameSpaceNode,
    Implementation,
    NodeList,
    NamedNodeMap,
    XPath,
    Iterator,
    // modern (Dom\ namespace) classes
    MNode,
    MDocument,
    MXmlDocument,
    MHtmlDocument,
    MDocumentFragment,
    MElement,
    MHtmlElement,
    MAttr,
    MCharacterData,
    MText,
    MComment,
    MCdataSection,
    MDocumentType,
    MNotation,
    MEntity,
    MEntityReference,
    MProcessingInstruction,
    MImplementation,
    MNodeList,
    MNamedNodeMap,
    MDtdNamedNodeMap,
    MHtmlCollection,
    MXPath,
    MTokenList,
}

impl Base {
    /// Is this one of the Dom\ (spec-compliant) classes?
    pub fn is_modern(self) -> bool {
        (self as u32) >= (Base::MNode as u32)
    }
}

pub const CLASSES: &[(&str, Base, Option<&str>)] = &[
    ("DOMNode", Base::Node, None),
    ("DOMDocument", Base::Document, Some("DOMNode")),
    (
        "DOMDocumentFragment",
        Base::DocumentFragment,
        Some("DOMNode"),
    ),
    ("DOMElement", Base::Element, Some("DOMNode")),
    ("DOMAttr", Base::Attr, Some("DOMNode")),
    ("DOMCharacterData", Base::CharacterData, Some("DOMNode")),
    ("DOMText", Base::Text, Some("DOMCharacterData")),
    ("DOMComment", Base::Comment, Some("DOMCharacterData")),
    ("DOMCdataSection", Base::CdataSection, Some("DOMText")),
    ("DOMDocumentType", Base::DocumentType, Some("DOMNode")),
    ("DOMNotation", Base::Notation, Some("DOMNode")),
    ("DOMEntity", Base::Entity, Some("DOMNode")),
    ("DOMEntityReference", Base::EntityReference, Some("DOMNode")),
    (
        "DOMProcessingInstruction",
        Base::ProcessingInstruction,
        Some("DOMNode"),
    ),
    ("DOMNameSpaceNode", Base::NameSpaceNode, None),
    ("DOMImplementation", Base::Implementation, None),
    ("DOMNodeList", Base::NodeList, None),
    ("DOMNamedNodeMap", Base::NamedNodeMap, None),
    ("DOMXPath", Base::XPath, None),
    ("Dom\\Node", Base::MNode, None),
    ("Dom\\Document", Base::MDocument, Some("Dom\\Node")),
    (
        "Dom\\XMLDocument",
        Base::MXmlDocument,
        Some("Dom\\Document"),
    ),
    (
        "Dom\\HTMLDocument",
        Base::MHtmlDocument,
        Some("Dom\\Document"),
    ),
    (
        "Dom\\DocumentFragment",
        Base::MDocumentFragment,
        Some("Dom\\Node"),
    ),
    ("Dom\\Element", Base::MElement, Some("Dom\\Node")),
    ("Dom\\HTMLElement", Base::MHtmlElement, Some("Dom\\Element")),
    ("Dom\\Attr", Base::MAttr, Some("Dom\\Node")),
    (
        "Dom\\CharacterData",
        Base::MCharacterData,
        Some("Dom\\Node"),
    ),
    ("Dom\\Text", Base::MText, Some("Dom\\CharacterData")),
    ("Dom\\Comment", Base::MComment, Some("Dom\\CharacterData")),
    ("Dom\\CDATASection", Base::MCdataSection, Some("Dom\\Text")),
    ("Dom\\DocumentType", Base::MDocumentType, Some("Dom\\Node")),
    ("Dom\\Notation", Base::MNotation, Some("Dom\\Node")),
    ("Dom\\Entity", Base::MEntity, Some("Dom\\Node")),
    (
        "Dom\\EntityReference",
        Base::MEntityReference,
        Some("Dom\\Node"),
    ),
    (
        "Dom\\ProcessingInstruction",
        Base::MProcessingInstruction,
        Some("Dom\\CharacterData"),
    ),
    ("Dom\\Implementation", Base::MImplementation, None),
    ("Dom\\NodeList", Base::MNodeList, None),
    ("Dom\\NamedNodeMap", Base::MNamedNodeMap, None),
    ("Dom\\DtdNamedNodeMap", Base::MDtdNamedNodeMap, None),
    ("Dom\\HTMLCollection", Base::MHtmlCollection, None),
    ("Dom\\XPath", Base::MXPath, None),
    ("Dom\\TokenList", Base::MTokenList, None),
];

pub fn base_name(b: Base) -> &'static str {
    for (n, bb, _) in CLASSES {
        if *bb == b {
            return n;
        }
    }
    "InternalIterator"
}

fn base_from_lower(l: &[u8]) -> Option<Base> {
    for (n, b, _) in CLASSES {
        if n.as_bytes().eq_ignore_ascii_case(l) {
            return Some(*b);
        }
    }
    None
}

/// The DOM base class of a (possibly user) class.
pub fn base_of_class(vm: &Vm, class_name: &[u8]) -> Option<Base> {
    let mut cur = class_name.to_ascii_lowercase();
    for _ in 0..64 {
        if let Some(b) = base_from_lower(&cur) {
            return Some(b);
        }
        match vm.classes.get(&cur).and_then(|c| c.parent.clone()) {
            Some(p) => cur = p.to_ascii_lowercase(),
            None => return None,
        }
    }
    None
}

pub fn base_of(vm: &Vm, o: &Obj) -> Option<Base> {
    let cn = o.borrow().class_name.clone();
    base_of_class(vm, &cn)
}

/// Is the class `class` (lowercase) the class `target` or a subclass?
pub fn class_is(vm: &Vm, class: &[u8], target: &str) -> bool {
    let t = target.to_ascii_lowercase();
    let mut cur = class.to_ascii_lowercase();
    for _ in 0..64 {
        if cur == t.as_bytes() {
            return true;
        }
        match vm.classes.get(&cur).and_then(|c| c.parent.clone()) {
            Some(p) => cur = p.to_ascii_lowercase(),
            None => return false,
        }
    }
    false
}

pub fn obj_is(vm: &Vm, o: &Obj, target: &str) -> bool {
    let cn = o.borrow().class_name.clone();
    class_is(vm, &cn, target)
}

// ---------------------------------------------------------------------------
// Per-document state (libxml_doc_props)

#[derive(Clone)]
pub struct DocProps {
    pub format_output: bool,
    pub validate_on_parse: bool,
    pub resolve_externals: bool,
    pub preserve_whitespace: bool,
    pub substitute_entities: bool,
    pub strict_error_checking: bool,
    pub recover: bool,
    /// registerNodeClass: base class lower -> user class name
    pub classmap: HashMap<String, Vec<u8>>,
    /// php_libxml_class_type: 0 unset, 1 legacy (DOMNode), 2 modern (Dom\Node)
    pub class_type: u8,
}

impl Default for DocProps {
    fn default() -> Self {
        DocProps {
            format_output: false,
            validate_on_parse: false,
            resolve_externals: false,
            preserve_whitespace: true,
            substitute_entities: false,
            strict_error_checking: true,
            recover: false,
            classmap: HashMap::new(),
            class_type: 0,
        }
    }
}

thread_local! {
    static DOC_PROPS: RefCell<HashMap<NodeId, DocProps>> = RefCell::new(HashMap::new());
}

pub fn doc_props(doc: Option<NodeId>) -> DocProps {
    match doc {
        None => DocProps::default(),
        Some(d) => DOC_PROPS.with(|m| m.borrow().get(&d).cloned().unwrap_or_default()),
    }
}

pub fn with_doc_props<R>(doc: NodeId, f: impl FnOnce(&mut DocProps) -> R) -> R {
    DOC_PROPS.with(|m| f(m.borrow_mut().entry(doc).or_default()))
}

pub fn set_doc_props(doc: NodeId, p: DocProps) {
    DOC_PROPS.with(|m| {
        m.borrow_mut().insert(doc, p);
    });
}

pub fn strict(doc: Option<NodeId>) -> bool {
    doc_props(doc).strict_error_checking
}

/// Document of a node (the node itself for documents).
pub fn doc_of_node(n: NodeId) -> Option<NodeId> {
    tree::with(|t| if t.is_doc(n) { Some(n) } else { t.nodes[n].doc })
}

// ---------------------------------------------------------------------------
// Node -> object

fn class_for_node(t: &Tree, n: NodeId) -> Option<Base> {
    Some(match t.nodes[n].t {
        NType::Document | NType::HtmlDocument => Base::Document,
        NType::Dtd | NType::DocumentType => Base::DocumentType,
        NType::Element => Base::Element,
        NType::Attribute => Base::Attr,
        NType::Text => Base::Text,
        NType::Comment => Base::Comment,
        NType::Pi => Base::ProcessingInstruction,
        NType::EntityRef => Base::EntityReference,
        NType::EntityDecl | NType::Entity | NType::ElementDecl => Base::Entity,
        NType::CData => Base::CdataSection,
        NType::DocumentFrag => Base::DocumentFragment,
        NType::Notation => Base::Notation,
        NType::NamespaceDecl => Base::NameSpaceNode,
        _ => return None,
    })
}

/// php_dom_create_object: the (cached) PHP object for a node.
pub fn node_obj(vm: &mut Vm, n: NodeId) -> Value {
    let cached = tree::with(|t| t.nodes[n].wrapper.as_ref().and_then(|w| w.upgrade()));
    if let Some(o) = cached {
        return Value::Object(o);
    }
    let base = match tree::with(|t| class_for_node(t, n)) {
        Some(b) => b,
        None => {
            let ty = tree::with(|t| t.nodes[n].t.code());
            throw_error(vm, b"Error", &format!("Unsupported node type: {}", ty));
            return Value::Null;
        }
    };
    // a namespace declaration node keeps its parent element object alive
    // ("fake child" of php_dom_create_fake_namespace_decl), created first
    let ns_parent = if base == Base::NameSpaceNode {
        tree::with(|t| t.nodes[n].parent).map(|p| node_obj(vm, p))
    } else {
        None
    };
    let doc = doc_of_node(n);
    let modern_doc = doc
        .map(|d| doc_props(Some(d)).class_type == 2)
        .unwrap_or(false)
        || tree::with(|t| t.nodes[n].flags & tree::F_MODERN != 0);
    let base = if modern_doc {
        modern::modern_base_for(n, base)
    } else {
        base
    };
    let mut class: Vec<u8> = base_name(base).as_bytes().to_vec();
    if let Some(d) = doc {
        let p = doc_props(Some(d));
        if let Some(c) = p.classmap.get(&base_name(base).to_ascii_lowercase()) {
            class = c.clone();
        }
    }
    let o = new_object(vm, &class);
    if base.is_modern() {
        attach_with(&o, Kind::Node(n), &modern::NODE_HANDLERS);
    } else {
        attach(&o, Kind::Node(n));
    }
    if let Some(p) = ns_parent {
        o.borrow_mut().set_property(b"__dom_parent".to_vec(), p);
    }
    tree::with(|t| t.nodes[n].wrapper = Some(Rc::downgrade(&o)));
    Value::Object(o)
}

pub fn node_obj_opt(vm: &mut Vm, n: Option<NodeId>) -> Value {
    match n {
        Some(n) => node_obj(vm, n),
        None => Value::Null,
    }
}

/// Allocate an object of a DOM (or user) class without running its
/// constructor.
pub fn new_object(vm: &mut Vm, class: &[u8]) -> Obj {
    let lower = class.to_ascii_lowercase();
    let disp = match base_from_lower(&lower) {
        Some(b) => base_name(b).as_bytes().to_vec(),
        None => vm
            .classes
            .get(&lower)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| class.to_vec()),
    };
    let id = vm.alloc_object_id();
    let mut o = PhpObject::new(disp, id);
    if vm.classes.contains_key(&lower) {
        vm.apply_class_property_defaults(&mut o, &lower);
    }
    Rc::new(RefCell::new(o))
}

/// Object for a DOMNodeList/DOMNamedNodeMap.
pub fn list_obj(vm: &mut Vm, k: ListKind) -> Value {
    let o = new_object(vm, b"DOMNodeList");
    attach(&o, Kind::List(k));
    Value::Object(o)
}

pub fn map_obj(vm: &mut Vm, k: MapKind) -> Value {
    let o = new_object(vm, b"DOMNamedNodeMap");
    attach(&o, Kind::Map(k));
    Value::Object(o)
}

// ---------------------------------------------------------------------------
// Argument parsing helpers (zpp)

/// Closures may be represented as internal function-name strings.
pub fn is_closure(v: &Value) -> bool {
    match v {
        Value::String(s) => {
            let b = s.as_bytes();
            b.starts_with(b"__closure_")
                || b.starts_with(b"__arrow_")
                || b.starts_with(b"__bound_closure_")
        }
        Value::Object(o) => o.borrow().class_name.eq_ignore_ascii_case(b"closure"),
        Value::Reference(r) => is_closure(&r.borrow()),
        _ => false,
    }
}

pub fn type_name_of(v: &Value) -> String {
    if is_closure(v) {
        return "Closure".into();
    }
    match v {
        Value::Null | Value::Undef => "null".into(),
        Value::True | Value::False => "bool".into(),
        Value::Long(_) => "int".into(),
        Value::Double(_) => "float".into(),
        Value::String(_) => "string".into(),
        Value::Array(_) => "array".into(),
        Value::Object(o) => {
            goro_core::value::display_class_name(&o.borrow().class_name).to_string()
        }
        Value::Resource(_) => "resource".into(),
        Value::Generator(_) => "Generator".into(),
        Value::Reference(r) => type_name_of(&r.borrow()),
    }
}

pub struct Args<'a> {
    pub f: String,
    pub a: &'a [Value],
}

pub type R<T> = Result<T, ()>;

impl<'a> Args<'a> {
    pub fn new(f: String, a: &'a [Value]) -> Self {
        Args { f, a }
    }
    pub fn count(&self, vm: &mut Vm, min: usize, max: usize) -> R<()> {
        let n = self
            .a
            .iter()
            .filter(|v| !matches!(v, Value::Undef))
            .count()
            .max(
                self.a.len().min(
                    self.a
                        .iter()
                        .rposition(|v| !matches!(v, Value::Undef))
                        .map(|p| p + 1)
                        .unwrap_or(0),
                ),
            );
        if n < min || n > max {
            let msg = if min == max {
                format!(
                    "{}() expects exactly {} argument{}, {} given",
                    self.f,
                    min,
                    if min == 1 { "" } else { "s" },
                    n
                )
            } else if n < min {
                format!(
                    "{}() expects at least {} argument{}, {} given",
                    self.f,
                    min,
                    if min == 1 { "" } else { "s" },
                    n
                )
            } else {
                format!(
                    "{}() expects at most {} argument{}, {} given",
                    self.f,
                    max,
                    if max == 1 { "" } else { "s" },
                    n
                )
            };
            throw_error(vm, b"ArgumentCountError", &msg);
            return Err(());
        }
        Ok(())
    }
    fn get(&self, i: usize) -> Option<Value> {
        self.a
            .get(i)
            .map(|v| v.deref())
            .filter(|v| !matches!(v, Value::Undef))
    }
    pub fn type_err(&self, vm: &mut Vm, i: usize, name: &str, ty: &str, v: &Value) {
        let msg = format!(
            "{}(): Argument #{} (${}) must be of type {}, {} given",
            self.f,
            i + 1,
            name,
            ty,
            type_name_of(v)
        );
        throw_error(vm, b"TypeError", &msg);
    }
    pub fn str(&self, vm: &mut Vm, i: usize, name: &str) -> R<String> {
        match self.get(i) {
            None => Ok(String::new()),
            Some(v) => self.str_val(vm, i, name, &v),
        }
    }
    pub fn str_bytes(&self, vm: &mut Vm, i: usize, name: &str) -> R<Vec<u8>> {
        match self.get(i) {
            None => Ok(Vec::new()),
            Some(Value::String(s)) => Ok(s.as_bytes().to_vec()),
            Some(v) => self.str_val(vm, i, name, &v).map(|s| s.into_bytes()),
        }
    }
    fn str_val(&self, vm: &mut Vm, i: usize, name: &str, v: &Value) -> R<String> {
        if is_closure(v) {
            self.type_err(vm, i, name, "string", v);
            return Err(());
        }
        match v {
            Value::String(s) => Ok(s.to_string_lossy()),
            Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
                Ok(v.to_php_string().to_string_lossy())
            }
            Value::Null => {
                vm.emit_deprecated(&format!(
                    "{}(): Passing null to parameter #{} (${}) of type string is deprecated",
                    self.f,
                    i + 1,
                    name
                ));
                Ok(String::new())
            }
            Value::Object(o) => {
                let cl = o.borrow().class_name.to_ascii_lowercase();
                let has = vm
                    .classes
                    .get(&cl)
                    .map(|c| c.methods.contains_key(&b"__tostring".to_vec()))
                    .unwrap_or(false)
                    || goro_core::value::native_cast(o, 3).is_some();
                if has {
                    let s = vm.value_to_string(v);
                    Ok(s.to_string_lossy())
                } else {
                    self.type_err(vm, i, name, "string", v);
                    Err(())
                }
            }
            _ => {
                self.type_err(vm, i, name, "string", v);
                Err(())
            }
        }
    }
    /// "P"/"p": a string that must not contain NUL bytes
    pub fn path_str(&self, vm: &mut Vm, i: usize, name: &str) -> R<String> {
        let s = self.str(vm, i, name)?;
        if s.contains('\0') {
            throw_error(
                vm,
                b"ValueError",
                &format!(
                    "{}(): Argument #{} (${}) must not contain any null bytes",
                    self.f,
                    i + 1,
                    name
                ),
            );
            return Err(());
        }
        Ok(s)
    }
    pub fn opt_path_str(&self, vm: &mut Vm, i: usize, name: &str) -> R<Option<String>> {
        let s = self.opt_str(vm, i, name)?;
        if s.as_deref().map(|s| s.contains('\0')).unwrap_or(false) {
            throw_error(
                vm,
                b"ValueError",
                &format!(
                    "{}(): Argument #{} (${}) must not contain any null bytes",
                    self.f,
                    i + 1,
                    name
                ),
            );
            return Err(());
        }
        Ok(s)
    }
    /// ?string
    pub fn opt_str(&self, vm: &mut Vm, i: usize, name: &str) -> R<Option<String>> {
        match self.get(i) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => self.str_val(vm, i, name, &v).map(Some),
        }
    }
    pub fn bool(&self, vm: &mut Vm, i: usize, name: &str, def: bool) -> R<bool> {
        match self.get(i) {
            None => Ok(def),
            Some(Value::True) => Ok(true),
            Some(Value::False) => Ok(false),
            Some(v @ (Value::Long(_) | Value::Double(_) | Value::String(_))) => Ok(v.is_truthy()),
            Some(Value::Null) => {
                vm.emit_deprecated(&format!(
                    "{}(): Passing null to parameter #{} (${}) of type bool is deprecated",
                    self.f,
                    i + 1,
                    name
                ));
                Ok(false)
            }
            Some(v) => {
                self.type_err(vm, i, name, "bool", &v);
                Err(())
            }
        }
    }
    pub fn long(&self, vm: &mut Vm, i: usize, name: &str, def: i64) -> R<i64> {
        match self.get(i) {
            None => Ok(def),
            Some(Value::Long(n)) => Ok(n),
            Some(Value::True) => Ok(1),
            Some(Value::False) => Ok(0),
            Some(Value::Double(d)) => {
                if d.fract() != 0.0 && d.is_finite() {
                    vm.emit_deprecated(&format!(
                        "Implicit conversion from float {} to int loses precision",
                        goro_core::value::format_php_float(d)
                    ));
                }
                if !d.is_finite() || d >= 9.223372036854776e18 || d < -9.223372036854776e18 {
                    self.type_err(vm, i, name, "int", &Value::Double(d));
                    return Err(());
                }
                Ok(d as i64)
            }
            Some(Value::String(s)) => {
                let st = s.to_string_lossy();
                let t = st.trim();
                if let Ok(n) = t.parse::<i64>() {
                    return Ok(n);
                }
                if let Ok(d) = t.parse::<f64>() {
                    if d.fract() == 0.0 {
                        return Ok(d as i64);
                    }
                }
                self.type_err(vm, i, name, "int", &Value::String(s));
                Err(())
            }
            Some(Value::Null) => {
                vm.emit_deprecated(&format!(
                    "{}(): Passing null to parameter #{} (${}) of type int is deprecated",
                    self.f,
                    i + 1,
                    name
                ));
                Ok(0)
            }
            Some(v) => {
                self.type_err(vm, i, name, "int", &v);
                Err(())
            }
        }
    }
    /// Object argument that must be an instance of `class`; returns the object.
    pub fn obj_of(
        &self,
        vm: &mut Vm,
        i: usize,
        name: &str,
        class: &str,
        nullable: bool,
    ) -> R<Option<Obj>> {
        match self.get(i) {
            None => {
                if nullable {
                    Ok(None)
                } else {
                    self.type_err(vm, i, name, class, &Value::Null);
                    Err(())
                }
            }
            Some(Value::Null) if nullable => Ok(None),
            Some(Value::Object(o)) if obj_is(vm, &o, class) => Ok(Some(o)),
            Some(v) => {
                let ty = if nullable {
                    format!("?{}", class)
                } else {
                    class.to_string()
                };
                self.type_err(vm, i, name, &ty, &v);
                Err(())
            }
        }
    }
    pub fn raw(&self, i: usize) -> Option<Value> {
        self.get(i)
    }
    pub fn len(&self) -> usize {
        self.a.len()
    }
}

/// DOM_GET_OBJ: the node behind an object, or "Couldn't fetch".
pub fn need_node(vm: &mut Vm, o: &Obj) -> R<NodeId> {
    match node_of(o) {
        Some(n) => Ok(n),
        None => {
            let cn = goro_core::value::display_class_name(&o.borrow().class_name).to_string();
            throw_error(vm, b"Error", &format!("Couldn't fetch {}", cn));
            Err(())
        }
    }
}

// ---------------------------------------------------------------------------
// Property handlers

pub type ReadFn = fn(&mut Vm, &Obj, NodeId) -> R<Value>;
pub type WriteFn = fn(&mut Vm, &Obj, NodeId, &Value) -> R<()>;

#[derive(Clone, Copy)]
pub struct Prop {
    pub name: &'static str,
    pub read: ReadFn,
    pub write: Option<WriteFn>,
    /// declared type, for write coercion: "string", "?string", "bool", "int", "mixed"
    pub ty: &'static str,
    /// declaring class (for type errors)
    pub owner: &'static str,
}

pub const fn p(name: &'static str, read: ReadFn, write: Option<WriteFn>, ty: &'static str) -> Prop {
    Prop {
        name,
        read,
        write,
        ty,
        owner: "",
    }
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

thread_local! {
    static PROP_TABLES: RefCell<HashMap<Base, Rc<Vec<Prop>>>> = RefCell::new(HashMap::new());
    static IN_DEBUG: Cell<bool> = const { Cell::new(false) };
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

fn build_tables() {
    let node = owned(node::node_props(), "DOMNode");
    let mut t: HashMap<Base, Rc<Vec<Prop>>> = HashMap::new();
    t.insert(Base::Node, Rc::new(node.clone()));
    t.insert(
        Base::DocumentFragment,
        Rc::new(merge(
            &owned(node::parent_props(), "DOMDocumentFragment"),
            &node,
        )),
    );
    t.insert(
        Base::Document,
        Rc::new(merge(
            &owned(document::document_props(), "DOMDocument"),
            &node,
        )),
    );
    let chardata = merge(
        &owned(chardata::chardata_props(), "DOMCharacterData"),
        &node,
    );
    t.insert(Base::CharacterData, Rc::new(chardata.clone()));
    t.insert(Base::Comment, Rc::new(chardata.clone()));
    let text = merge(&owned(chardata::text_props(), "DOMText"), &chardata);
    t.insert(Base::Text, Rc::new(text.clone()));
    t.insert(Base::CdataSection, Rc::new(text));
    t.insert(
        Base::Attr,
        Rc::new(merge(&owned(element::attr_props(), "DOMAttr"), &node)),
    );
    t.insert(
        Base::Element,
        Rc::new(merge(&owned(element::element_props(), "DOMElement"), &node)),
    );
    t.insert(
        Base::DocumentType,
        Rc::new(merge(
            &owned(chardata::doctype_props(), "DOMDocumentType"),
            &node,
        )),
    );
    t.insert(
        Base::Notation,
        Rc::new(merge(
            &owned(chardata::notation_props(), "DOMNotation"),
            &node,
        )),
    );
    t.insert(
        Base::Entity,
        Rc::new(merge(&owned(chardata::entity_props(), "DOMEntity"), &node)),
    );
    let mut eref = node.clone();
    for np in owned(chardata::entity_ref_overrides(), "DOMNode") {
        overwrite(&mut eref, np);
    }
    t.insert(Base::EntityReference, Rc::new(eref));
    t.insert(
        Base::ProcessingInstruction,
        Rc::new(merge(
            &owned(chardata::pi_props(), "DOMProcessingInstruction"),
            &node,
        )),
    );
    t.insert(
        Base::NameSpaceNode,
        Rc::new(owned(chardata::nsnode_props(), "DOMNameSpaceNode")),
    );
    t.insert(
        Base::NodeList,
        Rc::new(owned(lists::list_props(), "DOMNodeList")),
    );
    t.insert(
        Base::NamedNodeMap,
        Rc::new(owned(lists::map_props(), "DOMNamedNodeMap")),
    );
    t.insert(
        Base::XPath,
        Rc::new(owned(xpathobj::xpath_props(), "DOMXPath")),
    );
    t.insert(Base::Implementation, Rc::new(Vec::new()));
    modern::build_tables(&mut t);
    PROP_TABLES.with(|p| *p.borrow_mut() = t);
}

pub fn props_of(b: Base) -> Rc<Vec<Prop>> {
    PROP_TABLES.with(|p| p.borrow().get(&b).cloned().unwrap_or_default())
}

fn find_prop(vm: &Vm, o: &Obj, name: &[u8]) -> Option<(Base, Prop)> {
    let b = base_of(vm, o)?;
    let tbl = props_of(b);
    let n = std::str::from_utf8(name).ok()?;
    tbl.iter().find(|p| p.name == n).map(|p| (b, *p))
}

/// The node for a property read; DOM_PROP_NODE throws "Invalid State Error".
fn prop_target(vm: &mut Vm, o: &Obj) -> R<NodeId> {
    match data_of(o) {
        Some(d) => match &*d.kind.borrow() {
            Kind::Node(n) => return Ok(*n),
            Kind::List(_) | Kind::Map(_) | Kind::XPath(_) | Kind::Iter(_) | Kind::Modern(_) => {
                return Ok(usize::MAX);
            }
            _ => {}
        },
        None => {}
    }
    // DOMXPath property readers check their own state
    if matches!(base_of(vm, o), Some(Base::XPath) | Some(Base::MXPath)) {
        return Ok(usize::MAX);
    }
    throw_dom(vm, INVALID_STATE_ERR, true);
    Err(())
}

fn h_read_property(vm: &mut Vm, o: &Obj, name: &[u8], _mode: u8) -> Option<Value> {
    let (_b, p) = find_prop(vm, o, name)?;
    let n = match prop_target(vm, o) {
        Ok(n) => n,
        Err(()) => return Some(Value::Null),
    };
    match (p.read)(vm, o, n) {
        Ok(v) => Some(v),
        Err(()) => Some(Value::Null),
    }
}

fn coerce_for_type(vm: &mut Vm, _o: &Obj, p: &Prop, v: &Value) -> R<Value> {
    let cn = p.owner.to_string();
    let err = |vm: &mut Vm, v: &Value| {
        let msg = format!(
            "Cannot assign {} to property {}::${} of type {}",
            type_name_of(v),
            cn,
            p.name,
            p.ty
        );
        throw_error(vm, b"TypeError", &msg);
    };
    match p.ty {
        "bool" => match v {
            Value::True | Value::False => Ok(v.clone()),
            Value::Long(_) | Value::Double(_) | Value::String(_) => Ok(if v.is_truthy() {
                Value::True
            } else {
                Value::False
            }),
            _ => {
                err(vm, v);
                Err(())
            }
        },
        "string" | "?string" => match v {
            Value::String(_) => Ok(v.clone()),
            Value::Null if p.ty == "?string" => Ok(Value::Null),
            Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
                Ok(Value::String(v.to_php_string()))
            }
            Value::Object(ob) => {
                let cl = ob.borrow().class_name.to_ascii_lowercase();
                let has = vm
                    .classes
                    .get(&cl)
                    .map(|c| c.methods.contains_key(&b"__tostring".to_vec()))
                    .unwrap_or(false)
                    || goro_core::value::native_cast(ob, 3).is_some();
                if has {
                    let sv = vm.value_to_string(v);
                    if vm.current_exception.is_some() {
                        return Err(());
                    }
                    Ok(Value::String(sv))
                } else {
                    err(vm, v);
                    Err(())
                }
            }
            _ => {
                err(vm, v);
                Err(())
            }
        },
        "int" => match v {
            Value::Long(_) => Ok(v.clone()),
            Value::True => Ok(Value::Long(1)),
            Value::False => Ok(Value::Long(0)),
            _ => {
                err(vm, v);
                Err(())
            }
        },
        _ => Ok(v.clone()),
    }
}

fn h_write_property(vm: &mut Vm, o: &Obj, name: &[u8], v: &Value) -> bool {
    let (_b, p) = match find_prop(vm, o, name) {
        Some(x) => x,
        None => return false,
    };
    let w = match p.write {
        Some(w) => w,
        None => {
            let cn = goro_core::value::display_class_name(&o.borrow().class_name).to_string();
            throw_error(
                vm,
                b"Error",
                &format!("Cannot modify readonly property {}::${}", cn, p.name),
            );
            return true;
        }
    };
    let cv = match coerce_for_type(vm, o, &p, v) {
        Ok(c) => c,
        Err(()) => return true,
    };
    let n = match prop_target(vm, o) {
        Ok(n) => n,
        Err(()) => return true,
    };
    let _ = w(vm, o, n, &cv);
    true
}

fn h_has_property(vm: &mut Vm, o: &Obj, name: &[u8], check_empty: u8) -> Option<bool> {
    let (_b, p) = find_prop(vm, o, name)?;
    let n = match prop_target(vm, o) {
        Ok(n) => n,
        Err(()) => {
            vm.current_exception = None;
            return Some(false);
        }
    };
    match (p.read)(vm, o, n) {
        Ok(v) => Some(if check_empty == 1 {
            v.is_truthy()
        } else {
            !matches!(v, Value::Null)
        }),
        Err(()) => {
            vm.current_exception = None;
            Some(false)
        }
    }
}

fn h_unset_property(vm: &mut Vm, o: &Obj, name: &[u8]) -> bool {
    if let Some((_b, p)) = find_prop(vm, o, name) {
        let cn = goro_core::value::display_class_name(&o.borrow().class_name).to_string();
        throw_error(vm, b"Error", &format!("Cannot unset {}::${}", cn, p.name));
        return true;
    }
    false
}

/// dom_get_debug_info: std properties plus every property handler value
/// (objects shown as "(object value omitted)").
fn h_get_properties(vm: &mut Vm, o: &Obj, purpose: u8) -> Option<PhpArray> {
    if purpose != 0 {
        return None;
    }
    let b = base_of(vm, o)?;
    let mut arr = PhpArray::new();
    // std props (user-declared/dynamic, skip hidden)
    let std: Vec<(Vec<u8>, Value)> = o
        .borrow()
        .properties
        .iter()
        .filter(|(k, _)| !k.starts_with(b"__"))
        .cloned()
        .collect();
    for (k, v) in std {
        arr.set(
            goro_core::array::ArrayKey::String(PhpString::from_vec(k)),
            v,
        );
    }
    let tbl = props_of(b);
    let target = match data_of(o) {
        Some(d) => match &*d.kind.borrow() {
            Kind::Node(n) => Some(*n),
            Kind::List(_) | Kind::Map(_) | Kind::XPath(_) | Kind::Modern(_) => Some(usize::MAX),
            _ if b == Base::XPath || b == Base::MXPath => Some(usize::MAX),
            _ => None,
        },
        None => None,
    };
    let target = match target {
        Some(t) => t,
        // uninitialized object: every property read fails (and the
        // exception of the failed read is left pending, as in PHP)
        None => {
            if !tbl.is_empty() && vm.current_exception.is_none() {
                throw_dom(vm, INVALID_STATE_ERR, true);
            }
            return Some(arr);
        }
    };
    let saved_exc = vm.current_exception.take();
    let mut first_exc: Option<Value> = None;
    let prev = IN_DEBUG.with(|c| c.replace(true));
    let saved_er = vm.error_reporting;
    vm.error_reporting = 0;
    for p in tbl.iter() {
        let r = (p.read)(vm, o, target);
        if let Some(e) = vm.current_exception.take() {
            first_exc.get_or_insert(e);
            continue;
        }
        if let Ok(v) = r {
            let v = if matches!(v, Value::Object(_)) {
                Value::String(PhpString::from_bytes(b"(object value omitted)"))
            } else {
                v
            };
            arr.set(
                goro_core::array::ArrayKey::String(PhpString::from_bytes(p.name.as_bytes())),
                v,
            );
        }
    }
    vm.error_reporting = saved_er;
    IN_DEBUG.with(|c| c.set(prev));
    vm.current_exception = saved_exc.or(first_exc);
    Some(arr)
}

pub fn in_debug() -> bool {
    IN_DEBUG.with(|c| c.get())
}

fn h_clone(vm: &mut Vm, src: &Obj, dst: &Obj) {
    let d = match data_of(src) {
        Some(d) => d,
        None => return,
    };
    let k = match &*d.kind.borrow() {
        Kind::Node(n) => Some(*n),
        _ => None,
    };
    match k {
        Some(n) => {
            let doc = tree::with(|t| t.nodes[n].doc);
            let cloned = node::clone_node(n, doc, true);
            match cloned {
                Some(c) => {
                    if tree::with(|t| t.is_doc(c)) {
                        let props = doc_props(Some(n));
                        set_doc_props(c, props);
                    }
                    attach(dst, Kind::Node(c));
                    tree::with(|t| t.nodes[c].wrapper = Some(Rc::downgrade(dst)));
                }
                None => attach(dst, Kind::None),
            }
        }
        None => {
            if matches!(
                &*d.kind.borrow(),
                Kind::List(_) | Kind::Map(_) | Kind::XPath(_) | Kind::Iter(_)
            ) {
                let cn = goro_core::value::display_class_name(&src.borrow().class_name).to_string();
                throw_error(
                    vm,
                    b"Error",
                    &format!("Trying to clone an uncloneable object of class {}", cn),
                );
                return;
            }
            let nk = match &*d.kind.borrow() {
                Kind::List(l) => Kind::List(l.clone()),
                Kind::Map(m) => Kind::Map(m.clone()),
                Kind::Implementation => Kind::Implementation,
                _ => Kind::None,
            };
            attach(dst, nk);
        }
    }
}

// ---------------------------------------------------------------------------
// Methods

pub type MethodFn = fn(&mut Vm, &Obj, &[Value]) -> R<Value>;

fn method_table(b: Base) -> Vec<(&'static str, MethodFn)> {
    if b.is_modern() {
        return modern::method_table(b);
    }
    let mut v: Vec<(&'static str, MethodFn)> = Vec::new();
    let node_methods = node::node_methods();
    match b {
        Base::Node => v.extend(node_methods),
        Base::Document => {
            v.extend(document::document_methods());
            v.extend(node::parent_methods());
            v.extend(node_methods);
        }
        Base::DocumentFragment => {
            v.extend(chardata::fragment_methods());
            v.extend(node::parent_methods());
            v.extend(node_methods);
        }
        Base::Element => {
            v.extend(element::element_methods());
            v.extend(node::parent_methods());
            v.extend(node::child_methods());
            v.extend(node_methods);
        }
        Base::Attr => {
            v.extend(element::attr_methods());
            v.extend(node_methods);
        }
        Base::CharacterData | Base::Comment => {
            v.extend(chardata::chardata_methods());
            v.extend(node::child_methods());
            v.extend(node_methods);
        }
        Base::Text | Base::CdataSection => {
            v.extend(chardata::text_methods());
            v.extend(chardata::chardata_methods());
            v.extend(node::child_methods());
            v.extend(node_methods);
        }
        Base::DocumentType
        | Base::Notation
        | Base::Entity
        | Base::EntityReference
        | Base::ProcessingInstruction => v.extend(node_methods),
        Base::NameSpaceNode => v.extend(chardata::nsnode_methods()),
        Base::Implementation => v.extend(chardata::implementation_methods()),
        Base::NodeList | Base::NamedNodeMap | Base::Iterator => v.extend(lists::list_methods()),
        Base::XPath => v.extend(xpathobj::xpath_methods()),
        _ => {}
    }
    v
}

/// Methods of a DOM class family (lowercase name -> fn).
pub fn methods_of_pub(b: Base) -> Rc<HashMap<String, MethodFn>> {
    methods_of(b)
}

thread_local! {
    static METHODS: RefCell<HashMap<Base, Rc<HashMap<String, MethodFn>>>> = RefCell::new(HashMap::new());
    static DATE_ITER_HANDLER: Cell<Option<NativeClassHandler>> = const { Cell::new(None) };
}

fn methods_of(b: Base) -> Rc<HashMap<String, MethodFn>> {
    METHODS.with(|m| {
        if let Some(t) = m.borrow().get(&b) {
            return t.clone();
        }
        let mut h = HashMap::new();
        for (n, f) in method_table(b) {
            h.entry(n.to_ascii_lowercase()).or_insert(f);
        }
        let rc = Rc::new(h);
        m.borrow_mut().insert(b, rc.clone());
        rc
    })
}

fn has_ctor(b: Base) -> bool {
    matches!(
        b,
        Base::Document
            | Base::Element
            | Base::Attr
            | Base::Text
            | Base::Comment
            | Base::CdataSection
            | Base::ProcessingInstruction
            | Base::EntityReference
            | Base::DocumentFragment
            | Base::XPath
            | Base::MXPath
    )
}

fn has_method(class: &[u8], method: &[u8], is_static: bool) -> bool {
    let b = match base_from_lower(class) {
        Some(b) => b,
        None => {
            if class == b"internaliterator" {
                return !is_static
                    && matches!(method, b"current" | b"key" | b"next" | b"valid" | b"rewind");
            }
            return false;
        }
    };
    let m = String::from_utf8_lossy(method).to_ascii_lowercase();
    if is_static && (b == Base::XPath || b == Base::MXPath) && m == "quote" {
        return true;
    }
    if is_static && modern::is_static_method(b, &m) {
        return true;
    }
    if b.is_modern() && m == "__construct" && modern::is_node_base(b) {
        return true;
    }
    // non-static methods called statically error out in call_method
    (m == "__construct" && has_ctor(b)) || methods_of(b).contains_key(&m)
}

fn method_display(b: Base, m: &str) -> String {
    // canonical method names for messages
    let canon = method_table(b)
        .into_iter()
        .map(|(n, _)| n)
        .find(|n| n.eq_ignore_ascii_case(m))
        .unwrap_or(m);
    // methods are declared on the class that defines them; PHP reports the
    // declaring class
    let owner = declaring_class(b, canon);
    format!("{}::{}", owner, canon)
}

fn declaring_class(b: Base, m: &str) -> &'static str {
    if b.is_modern() {
        return modern::declaring_class(b, m);
    }
    let lm = m.to_ascii_lowercase();
    let in_tbl =
        |tbl: Vec<(&'static str, MethodFn)>| tbl.iter().any(|(n, _)| n.eq_ignore_ascii_case(&lm));
    match b {
        Base::Document
            if in_tbl(document::document_methods()) || in_tbl(node::parent_methods()) =>
        {
            "DOMDocument"
        }
        Base::DocumentFragment
            if in_tbl(chardata::fragment_methods()) || in_tbl(node::parent_methods()) =>
        {
            "DOMDocumentFragment"
        }
        Base::Element
            if in_tbl(element::element_methods())
                || in_tbl(node::parent_methods())
                || in_tbl(node::child_methods()) =>
        {
            "DOMElement"
        }
        Base::Attr if in_tbl(element::attr_methods()) => "DOMAttr",
        Base::Text | Base::CdataSection if in_tbl(chardata::text_methods()) => "DOMText",
        Base::Text | Base::CdataSection | Base::CharacterData | Base::Comment
            if in_tbl(chardata::chardata_methods()) || in_tbl(node::child_methods()) =>
        {
            "DOMCharacterData"
        }
        Base::NameSpaceNode => "DOMNameSpaceNode",
        Base::Implementation => "DOMImplementation",
        Base::NodeList => "DOMNodeList",
        Base::NamedNodeMap => "DOMNamedNodeMap",
        Base::XPath => "DOMXPath",
        Base::Iterator => "InternalIterator",
        _ => "DOMNode",
    }
}

fn call_method(
    vm: &mut Vm,
    class: &[u8],
    method: &[u8],
    called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    if class == b"internaliterator" {
        if let Some(Value::Object(o)) = this {
            if let Some(d) = data_of(o) {
                if matches!(&*d.kind.borrow(), Kind::Iter(_)) {
                    drop(d);
                    return lists::iter_method(vm, o, method).unwrap_or(Value::Null);
                }
                if matches!(&*d.kind.borrow(), Kind::Modern(_)) {
                    drop(d);
                    return modern::tokenlist::iter_method(vm, o, method).unwrap_or(Value::Null);
                }
            }
        }
        if let Some(h) = DATE_ITER_HANDLER.with(|c| c.get()) {
            return (h.call)(vm, class, method, called, this, args);
        }
        return Value::Null;
    }
    let b = match base_from_lower(class) {
        Some(b) => b,
        None => return Value::Null,
    };
    let m = String::from_utf8_lossy(method).to_ascii_lowercase();
    if this.is_none() {
        if (b == Base::XPath || b == Base::MXPath) && m == "quote" {
            let prev = CURRENT_FN.with(|c| c.replace(Some(format!("{}::quote", base_name(b)))));
            let r = xpathobj::quote(vm, args).unwrap_or(Value::Null);
            CURRENT_FN.with(|c| *c.borrow_mut() = prev);
            return r;
        }
        if modern::is_static_method(b, &m) {
            let name = format!("{}::{}", base_name(b), modern::static_display(&m));
            let prev = CURRENT_FN.with(|c| c.replace(Some(name)));
            let r = modern::call_static(vm, b, &m, args);
            CURRENT_FN.with(|c| *c.borrow_mut() = prev);
            return r.unwrap_or(Value::Null);
        }
        if methods_of(b).contains_key(&m) {
            let name = method_display(b, &m);
            throw_error(
                vm,
                b"Error",
                &format!("Non-static method {}() cannot be called statically", name),
            );
        }
        return Value::Null;
    }
    let o = match this {
        Some(Value::Object(o)) => o.clone(),
        _ => return Value::Null,
    };
    if m == "__construct" && b.is_modern() && modern::is_node_base(b) {
        let cn = goro_core::value::display_class_name(&o.borrow().class_name).to_string();
        throw_error(
            vm,
            b"Error",
            &format!(
                "Cannot directly construct {}, use document methods instead",
                cn
            ),
        );
        return Value::Null;
    }
    if m == "__construct" && has_ctor(b) {
        // explicit $obj->__construct(...) re-initializes the object
        let mut full = vec![Value::Object(o.clone())];
        full.extend_from_slice(args);
        let line = vm.current_line;
        let _ = construct(vm, &full, line);
        return Value::Null;
    }
    let f = match methods_of(b).get(&m) {
        Some(f) => *f,
        None => return Value::Null,
    };
    let name = method_display(b, &m);
    let prev = CURRENT_FN.with(|c| c.replace(Some(name)));
    let r = f(vm, &o, args);
    CURRENT_FN.with(|c| *c.borrow_mut() = prev);
    r.unwrap_or(Value::Null)
}

/// Name of the running DOM method ("DOMDocument::loadXML").
pub fn cur_method() -> String {
    CURRENT_FN.with(|c| c.borrow().clone()).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Constructors

fn construct(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return Ok(()),
    };
    let b = match base_of(vm, &this) {
        Some(b) => b,
        None => return Ok(()),
    };
    let name = format!("{}::__construct", base_name(b));
    let prev = CURRENT_FN.with(|c| c.replace(Some(name)));
    let r = match b {
        Base::Document => document::construct(vm, &this, &args[1..]),
        Base::Element => element::construct_element(vm, &this, &args[1..]),
        Base::Attr => element::construct_attr(vm, &this, &args[1..]),
        Base::Text
        | Base::Comment
        | Base::CdataSection
        | Base::ProcessingInstruction
        | Base::EntityReference
        | Base::DocumentFragment => chardata::construct(vm, &this, b, &args[1..]),
        Base::XPath | Base::MXPath => xpathobj::construct(vm, &this, &args[1..]),
        _ => Ok(()),
    };
    CURRENT_FN.with(|c| *c.borrow_mut() = prev);
    match r {
        Ok(()) => Ok(()),
        Err(()) => {
            if vm.current_exception.is_none() {
                Ok(())
            } else {
                let msg = match &vm.current_exception {
                    Some(Value::Object(o)) => o
                        .borrow()
                        .get_property(b"message")
                        .to_php_string()
                        .to_string_lossy(),
                    _ => String::new(),
                };
                Err(msg)
            }
        }
    }
}

fn create_object(vm: &mut Vm, o: &Obj) {
    if o.borrow().native.is_none() {
        let modern = base_of(vm, o).map(|b| b.is_modern()).unwrap_or(false);
        if modern {
            attach_with(o, Kind::None, &modern::NODE_HANDLERS);
        } else {
            attach(o, Kind::None);
        }
    }
}

// ---------------------------------------------------------------------------
// Registration

fn class_entry(name: &str, parent: Option<&str>, interfaces: &[&str]) -> ClassEntry {
    let mut ce = ClassEntry::new(name.as_bytes().to_vec());
    ce.parent = parent.map(|p| p.as_bytes().to_vec());
    ce.interfaces = interfaces.iter().map(|s| s.as_bytes().to_vec()).collect();
    ce
}

pub fn register(vm: &mut Vm) {
    build_tables();
    vm.register_extension(b"dom");

    // interfaces
    let mut pn = class_entry("DOMParentNode", None, &[]);
    pn.is_interface = true;
    vm.classes.insert(b"domparentnode".to_vec(), pn);
    let mut cn = class_entry("DOMChildNode", None, &[]);
    cn.is_interface = true;
    vm.classes.insert(b"domchildnode".to_vec(), cn);

    let mut exc = class_entry("DOMException", Some("Exception"), &[]);
    exc.is_final = true;
    vm.classes.insert(b"domexception".to_vec(), exc);

    for (name, b, parent) in CLASSES {
        if b.is_modern() {
            continue;
        }
        let ifaces: &[&str] = match b {
            Base::Document | Base::DocumentFragment => &["DOMParentNode"],
            Base::Element => &["DOMParentNode", "DOMChildNode"],
            Base::CharacterData => &["DOMChildNode"],
            Base::NodeList | Base::NamedNodeMap => {
                &["IteratorAggregate", "Traversable", "Countable"]
            }
            _ => &[],
        };
        let mut ce = class_entry(name, *parent, ifaces);
        if *b == Base::Node {
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
        vm.classes
            .insert(name.to_ascii_lowercase().into_bytes(), ce);
    }

    modern::register_classes(vm);
    let h = NativeClassHandler {
        has_method,
        call: call_method,
        prop_added: None,
        prop_write: None,
        compare_check: None,
        prop_read: None,
        param_names: Some(param_names),
    };
    for (name, b, _) in CLASSES {
        let l = name.to_ascii_lowercase().into_bytes();
        vm.native_class_handlers.insert(l.clone(), h);
        vm.object_create_handlers.insert(l.clone(), create_object);
        if matches!(
            b,
            Base::Document
                | Base::Element
                | Base::Attr
                | Base::Text
                | Base::Comment
                | Base::CdataSection
                | Base::ProcessingInstruction
                | Base::EntityReference
                | Base::DocumentFragment
                | Base::XPath
                | Base::MXPath
        ) {
            vm.object_construct_handlers.insert(l, construct);
        }
    }
    // InternalIterator is shared with ext/date (DatePeriod)
    if let Some(prev) = vm
        .native_class_handlers
        .get(&b"internaliterator".to_vec())
        .copied()
    {
        DATE_ITER_HANDLER.with(|c| c.set(Some(prev)));
    }
    vm.native_class_handlers
        .insert(b"internaliterator".to_vec(), h);

    // constants
    let c = |vm: &mut Vm, n: &str, v: i64| {
        vm.constants.insert(n.as_bytes().to_vec(), Value::Long(v));
    };
    for (n, v) in [
        ("XML_ELEMENT_NODE", 1),
        ("XML_ATTRIBUTE_NODE", 2),
        ("XML_TEXT_NODE", 3),
        ("XML_CDATA_SECTION_NODE", 4),
        ("XML_ENTITY_REF_NODE", 5),
        ("XML_ENTITY_NODE", 6),
        ("XML_PI_NODE", 7),
        ("XML_COMMENT_NODE", 8),
        ("XML_DOCUMENT_NODE", 9),
        ("XML_DOCUMENT_TYPE_NODE", 10),
        ("XML_DOCUMENT_FRAG_NODE", 11),
        ("XML_NOTATION_NODE", 12),
        ("XML_HTML_DOCUMENT_NODE", 13),
        ("XML_DTD_NODE", 14),
        ("XML_ELEMENT_DECL_NODE", 15),
        ("XML_ATTRIBUTE_DECL_NODE", 16),
        ("XML_ENTITY_DECL_NODE", 17),
        ("XML_NAMESPACE_DECL_NODE", 18),
        ("XML_LOCAL_NAMESPACE", 18),
        ("XML_ATTRIBUTE_CDATA", 1),
        ("XML_ATTRIBUTE_ID", 2),
        ("XML_ATTRIBUTE_IDREF", 3),
        ("XML_ATTRIBUTE_IDREFS", 4),
        ("XML_ATTRIBUTE_ENTITY", 5),
        ("XML_ATTRIBUTE_NMTOKEN", 7),
        ("XML_ATTRIBUTE_NMTOKENS", 8),
        ("XML_ATTRIBUTE_ENUMERATION", 9),
        ("XML_ATTRIBUTE_NOTATION", 10),
        ("DOM_PHP_ERR", 0),
        ("DOM_INDEX_SIZE_ERR", 1),
        ("DOMSTRING_SIZE_ERR", 2),
        ("DOM_HIERARCHY_REQUEST_ERR", 3),
        ("DOM_WRONG_DOCUMENT_ERR", 4),
        ("DOM_INVALID_CHARACTER_ERR", 5),
        ("DOM_NO_DATA_ALLOWED_ERR", 6),
        ("DOM_NO_MODIFICATION_ALLOWED_ERR", 7),
        ("DOM_NOT_FOUND_ERR", 8),
        ("DOM_NOT_SUPPORTED_ERR", 9),
        ("DOM_INUSE_ATTRIBUTE_ERR", 10),
        ("DOM_INVALID_STATE_ERR", 11),
        ("DOM_SYNTAX_ERR", 12),
        ("DOM_INVALID_MODIFICATION_ERR", 13),
        ("DOM_NAMESPACE_ERR", 14),
        ("DOM_INVALID_ACCESS_ERR", 15),
        ("DOM_VALIDATION_ERR", 16),
    ] {
        c(vm, n, v);
    }
    vm.register_function_with_params(
        b"dom_import_simplexml",
        chardata::dom_import_simplexml,
        &[b"node"],
    );
    modern::register_rest(vm);
}

fn param_names(class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    let b = base_from_lower(class)?;
    if b.is_modern() {
        if let Some(p) = modern::param_names(b, method) {
            return Some(p);
        }
    }
    Some(match (b, method) {
        (Base::Document, b"__construct") => &["version", "encoding"],
        (Base::Element, b"__construct") => &["qualifiedName", "value", "namespace"],
        (Base::Attr, b"__construct") => &["name", "value"],
        (Base::ProcessingInstruction, b"__construct") => &["name", "value"],
        (Base::XPath, b"__construct") => &["document", "registerNodeNS"],
        (_, b"__construct") => &["data"],
        (_, b"load") | (_, b"loadxml") | (_, b"loadhtml") | (_, b"loadhtmlfile") => {
            &["source", "options"]
        }
        (_, b"savexml") => &["node", "options"],
        (_, b"save") => &["filename", "options"],
        (_, b"evaluate") | (_, b"query") => &["expression", "contextNode", "registerNodeNS"],
        (_, b"registernamespace") => &["prefix", "namespace"],
        (_, b"c14n") => &["exclusive", "withComments", "xpath", "nsPrefixes"],
        (_, b"getattribute")
        | (_, b"hasattribute")
        | (_, b"removeattribute")
        | (_, b"getattributenode") => &["qualifiedName"],
        (_, b"setattribute") => &["qualifiedName", "value"],
        (_, b"toggleattribute") => &["qualifiedName", "force"],
        (_, b"createelement") => &["localName", "value"],
        (_, b"createelementns") => &["namespace", "qualifiedName", "value"],
        (_, b"importnode") => &["node", "deep"],
        (_, b"clonenode") => &["deep"],
        (_, b"insertbefore") => &["node", "child"],
        (_, b"getelementsbytagname") => &["qualifiedName"],
        (_, b"getelementsbytagnamens") => &["namespace", "localName"],
        _ => &[],
    })
}

/// DOMNode from a value (for simplexml_import_dom): the node of any DOM
/// node object.
pub fn dom_node_of_value(o: &Obj) -> Option<NodeId> {
    node_of(o)
}

pub fn set_wrapper(n: NodeId, o: &Obj) {
    tree::with(|t| t.nodes[n].wrapper = Some(Rc::downgrade(o)));
}

pub fn weak(o: &Obj) -> Weak<RefCell<PhpObject>> {
    Rc::downgrade(o)
}
