//! ext/simplexml (PHP manual: SimpleXML): `SimpleXMLElement`,
//! `SimpleXMLIterator` and the `simplexml_*` functions, on goro-xml's tree
//! in the shared node store ([`super::xmlstore`]), so documents are shared
//! with ext/dom (`simplexml_import_dom()`, `dom_import_simplexml()`).
//!
//! An object stands for a node and how it is iterated, as the manual
//! describes SimpleXML's access: `$x->name` is the list of `x`'s children
//! named `name` (it acts as its first element), `$x->children()` the
//! children, `$x->attributes()` the attributes, and elements reached by
//! iteration or `[n]` are themselves. The namespace filter
//! (`children($ns)`, the `namespace_or_prefix` arguments) is carried along.
//! Property tables (var_dump, print_r, `(array)`, json_encode), casts,
//! comparisons and write semantics follow observations on PHP 8.5 and the
//! ext/simplexml tests' expected output.

use std::rc::Rc;

use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};
use goro_xml::tree::{DocKind, NodeData, QName};
use goro_xml::{Arena, NodeId, NodeType};

use super::libxml::{self, Style};
use super::xmlstore::{self, NodeHold};
use crate::bytecode::CastTo;
use crate::class::{Class, NativeHooks, ObjAccess};
use crate::func::{Args, NativeFn};
use crate::registry::Module;
use crate::vm::{R, Vm};

const SXE_CLASS: &str = r#"<?php
class SimpleXMLElement implements Stringable, Countable, RecursiveIterator {
    public function xpath(string $expression): array|null|false {}
    public function registerXPathNamespace(string $prefix, string $namespace): bool {}
    public function asXML(?string $filename = null): string|bool {}
    public function saveXML(?string $filename = null): string|bool {}
    public function getNamespaces(bool $recursive = false): array {}
    public function getDocNamespaces(bool $recursive = false, bool $fromRoot = true): array|false {}
    public function children(?string $namespaceOrPrefix = null, bool $isPrefix = false): ?SimpleXMLElement {}
    public function attributes(?string $namespaceOrPrefix = null, bool $isPrefix = false): ?SimpleXMLElement {}
    public function __construct(string $data, int $options = 0, bool $dataIsURL = false, string $namespaceOrPrefix = "", bool $isPrefix = false) {}
    public function addChild(string $qualifiedName, ?string $value = null, ?string $namespace = null): ?SimpleXMLElement {}
    public function addAttribute(string $qualifiedName, string $value, ?string $namespace = null): void {}
    public function getName(): string {}
    public function __toString(): string {}
    public function __debugInfo(): ?array {}
    public function count(): int {}
    public function rewind(): void {}
    public function valid(): bool {}
    public function current(): SimpleXMLElement {}
    public function key(): string {}
    public function next(): void {}
    public function hasChildren(): bool {}
    public function getChildren(): ?SimpleXMLElement {}
}"#;

static METHODS: &[(&str, NativeFn)] = &[
    ("simplexmlelement::xpath", m_xpath),
    (
        "simplexmlelement::registerxpathnamespace",
        m_register_xpath_ns,
    ),
    ("simplexmlelement::asxml", m_as_xml),
    ("simplexmlelement::savexml", m_as_xml),
    ("simplexmlelement::getnamespaces", m_get_namespaces),
    ("simplexmlelement::getdocnamespaces", m_get_doc_namespaces),
    ("simplexmlelement::children", m_children),
    ("simplexmlelement::attributes", m_attributes),
    ("simplexmlelement::__construct", m_construct),
    ("simplexmlelement::addchild", m_add_child),
    ("simplexmlelement::addattribute", m_add_attribute),
    ("simplexmlelement::getname", m_get_name),
    ("simplexmlelement::__tostring", m_to_string),
    ("simplexmlelement::__debuginfo", m_debug_info),
    ("simplexmlelement::count", m_count),
    ("simplexmlelement::rewind", m_rewind),
    ("simplexmlelement::valid", m_valid),
    ("simplexmlelement::current", m_current),
    ("simplexmlelement::key", m_key),
    ("simplexmlelement::next", m_next),
    ("simplexmlelement::haschildren", m_has_children),
    ("simplexmlelement::getchildren", m_get_children),
];

pub static MODULE: Module = Module {
    name: "SimpleXML",
    functions: &[
        (
            "function simplexml_load_file(string $filename, ?string $class_name = SimpleXMLElement::class, int $options = 0, string $namespace_or_prefix = \"\", bool $is_prefix = false): SimpleXMLElement|false",
            simplexml_load_file,
        ),
        (
            "function simplexml_load_string(string $data, ?string $class_name = SimpleXMLElement::class, int $options = 0, string $namespace_or_prefix = \"\", bool $is_prefix = false): SimpleXMLElement|false",
            simplexml_load_string,
        ),
        (
            "function simplexml_import_dom(object $node, ?string $class_name = SimpleXMLElement::class): ?SimpleXMLElement",
            simplexml_import_dom,
        ),
    ],
    classes: &[
        ("simplexmlelement", SXE_CLASS),
        (
            "simplexmliterator",
            "<?php class SimpleXMLIterator extends SimpleXMLElement {}",
        ),
    ],
    methods: METHODS,
    hooks: &[("simplexmlelement", hooks)],
    ..Module::EMPTY
};

// ------------------------------------------------------------------ model

/// How an object iterates its node.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Iter {
    /// The node itself (an element or an attribute).
    None,
    /// The node's children with this name (`$x->name`).
    Elem(String),
    /// The node's children (`children()`).
    Child,
    /// The node's attributes (`attributes()`), or those with a name (an
    /// attribute found by `xpath()`).
    Attrs(Option<String>),
}

/// The namespace filter.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Filter {
    ns: Option<String>,
    is_prefix: bool,
}

/// The native data of a SimpleXML object.
struct Sxe {
    node: NodeHold,
    iter: Iter,
    filter: Filter,
    /// Elements to create under `node` before `iter` applies (a write
    /// through `$x->missing->child = ...`).
    pending: Vec<String>,
    /// The iteration cursor.
    cursor: Option<NodeHold>,
}

impl goro_runtime::NativeData for Sxe {}

/// A plain description of an object (to build one).
#[derive(Clone)]
struct Spec {
    node: NodeId,
    iter: Iter,
    filter: Filter,
    pending: Vec<String>,
}

impl Spec {
    fn of(node: NodeId, filter: &Filter) -> Spec {
        Spec {
            node,
            iter: Iter::None,
            filter: filter.clone(),
            pending: Vec::new(),
        }
    }
}

/// The node of a SimpleXML object (its first element for lists), for
/// `dom_import_simplexml()`.
pub fn sxe_node_of(o: &ObjRef) -> Option<NodeId> {
    let spec = spec_of(o)?;
    if xmlstore::busy() {
        return Some(spec.node);
    }
    xmlstore::with(|s| first(&s.arena, &spec))
}

fn spec_of(o: &ObjRef) -> Option<Spec> {
    let n = o.obj().native.borrow();
    let any: &dyn std::any::Any = &**n.as_ref()?;
    any.downcast_ref::<Sxe>().map(|d| Spec {
        node: d.node.id(),
        iter: d.iter.clone(),
        filter: d.filter.clone(),
        pending: d.pending.clone(),
    })
}

fn with_sxe<T>(o: &ObjRef, f: impl FnOnce(&mut Sxe) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<Sxe>().map(f)
}

/// The object's spec, or the "not initialized" error.
fn spec(vm: &mut Vm, o: &ObjRef) -> R<Spec> {
    match spec_of(o) {
        Some(s) => Ok(s),
        None => vm.throw("Error", "SimpleXMLElement is not properly initialized"),
    }
}

/// A new object of class `cls` for `spec`.
fn make(vm: &mut Vm, cls: &Rc<Class>, spec: Spec) -> R<Value> {
    let o = vm.instantiate_raw(cls)?;
    let hold = xmlstore::with(|s| s.hold(spec.node));
    *o.obj().native.borrow_mut() = Some(Box::new(Sxe {
        node: hold,
        iter: spec.iter,
        filter: spec.filter,
        pending: spec.pending,
        cursor: None,
    }));
    Ok(Value::Object(o))
}

fn el_matches(a: &Arena, n: NodeId, f: &Filter) -> bool {
    let Some(q) = (match a.data(n) {
        NodeData::Element(e) => Some(&e.name),
        _ => None,
    }) else {
        return false;
    };
    name_matches(q, f)
}

fn name_matches(q: &QName, f: &Filter) -> bool {
    match &f.ns {
        None => q.prefix.is_none(),
        Some(p) if f.is_prefix => q.prefix.as_deref().unwrap_or("") == p,
        Some(u) => q.ns_str() == u,
    }
}

fn attr_matches(a: &Arena, n: NodeId, f: &Filter) -> bool {
    match a.attr(n) {
        Some(ad) => match &f.ns {
            None => ad.name.prefix.is_none(),
            Some(p) if f.is_prefix => ad.name.prefix.as_deref().unwrap_or("") == p,
            Some(u) => ad.name.ns_str() == u,
        },
        None => false,
    }
}

fn local_name(a: &Arena, n: NodeId) -> String {
    match a.data(n) {
        NodeData::Element(e) => e.name.local.to_string(),
        NodeData::Attribute(at) => at.name.local.to_string(),
        _ => a.node_name(n),
    }
}

/// Child elements of `n` matching the filter (and `name` if given).
fn child_elems(a: &Arena, n: NodeId, f: &Filter, name: Option<&str>) -> Vec<NodeId> {
    if !matches!(a.data(n), NodeData::Element(_) | NodeData::Document(_)) {
        return Vec::new();
    }
    a.children(n)
        .filter(|&c| el_matches(a, c, f) && name.is_none_or(|nm| local_name(a, c) == nm))
        .collect()
}

fn attrs_of(a: &Arena, n: NodeId, f: &Filter, name: Option<&str>) -> Vec<NodeId> {
    if !a.is_element(n) {
        return Vec::new();
    }
    a.attrs(n)
        .filter(|&c| attr_matches(a, c, f) && name.is_none_or(|nm| local_name(a, c) == nm))
        .collect()
}

/// The nodes an object stands for.
fn members(a: &Arena, s: &Spec) -> Vec<NodeId> {
    if !s.pending.is_empty() || !a.is_live(s.node) {
        return Vec::new();
    }
    match &s.iter {
        Iter::None => vec![s.node],
        Iter::Elem(name) => child_elems(a, s.node, &s.filter, Some(name)),
        Iter::Child => child_elems(a, s.node, &s.filter, None),
        Iter::Attrs(n) => attrs_of(a, s.node, &s.filter, n.as_deref()),
    }
}

/// The node the object acts as (the first of a list).
fn first(a: &Arena, s: &Spec) -> Option<NodeId> {
    members(a, s).first().copied()
}

/// The element property and attribute names are looked up on.
fn base(a: &Arena, s: &Spec) -> Option<NodeId> {
    if !s.pending.is_empty() || !a.is_live(s.node) {
        return None;
    }
    match &s.iter {
        Iter::Elem(_) => first(a, s),
        _ => Some(s.node),
    }
}

/// What iteration (`foreach`, the Iterator methods) walks.
fn iteration(a: &Arena, s: &Spec) -> Vec<NodeId> {
    if !s.pending.is_empty() || !a.is_live(s.node) {
        return Vec::new();
    }
    match &s.iter {
        Iter::None => {
            if a.is_element(s.node) {
                child_elems(a, s.node, &s.filter, None)
            } else {
                Vec::new()
            }
        }
        _ => members(a, s),
    }
}

/// Text of an element's direct text children (the string value).
fn direct_text(a: &Arena, n: NodeId) -> String {
    match a.data(n) {
        NodeData::Attribute(_) => return a.attr_value(n),
        NodeData::Element(_) => {}
        NodeData::EntityRef(name) => {
            return a.entity_value(a.doc_of(n), name).unwrap_or_default();
        }
        _ => return a.char_data(n).map(str::to_string).unwrap_or_default(),
    }
    let mut s = String::new();
    for c in a.children(n) {
        match a.data(c) {
            NodeData::Text(t) | NodeData::CData(t) => s.push_str(t),
            NodeData::EntityRef(name) => {
                if a.first_child(c).is_some() {
                    s.push_str(&a.text_content(c));
                } else if let Some(v) = a.entity_value(a.doc_of(c), name) {
                    s.push_str(&v);
                }
            }
            _ => {}
        }
    }
    s
}

fn is_blank(t: &str) -> bool {
    t.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
}

fn string_of(a: &Arena, s: &Spec) -> String {
    first(a, s).map(|n| direct_text(a, n)).unwrap_or_default()
}

// ------------------------------------------------------------------ properties

/// A property value before objects are made.
enum Pv {
    Str(String),
    Obj(Spec),
    List(Vec<Pv>),
    Attrs(Vec<(String, String)>),
}

/// The value an element child shows as: its text when it starts with
/// non-blank text, else an object (observed on PHP 8.5).
fn child_value(a: &Arena, c: NodeId, f: &Filter) -> Pv {
    if a.is_element(c)
        && let Some(fc) = a.first_child(c)
        && let NodeData::Text(t) = a.data(fc)
        && !is_blank(t)
    {
        return Pv::Str(direct_text(a, c));
    }
    Pv::Obj(Spec::of(c, f))
}

fn push_prop(out: &mut Vec<(Key, Pv)>, k: Key, v: Pv) {
    if let Some(e) = out.iter_mut().find(|(ek, _)| *ek == k) {
        match &mut e.1 {
            Pv::List(l) => l.push(v),
            other => {
                let prev = std::mem::replace(other, Pv::List(Vec::new()));
                *other = Pv::List(vec![prev, v]);
            }
        }
    } else {
        out.push((k, v));
    }
}

fn attr_list(a: &Arena, n: NodeId, f: &Filter) -> Vec<(String, String)> {
    attrs_of(a, n, f, None)
        .into_iter()
        .map(|at| (local_name(a, at), a.attr_value(at)))
        .collect()
}

/// The property table of an element.
fn element_props(a: &Arena, n: NodeId, f: &Filter, out: &mut Vec<(Key, Pv)>) {
    let at = attr_list(a, n, f);
    if !at.is_empty() {
        out.push((Key::from_bytes(b"@attributes"), Pv::Attrs(at)));
    }
    content_props(a, n, f, out);
}

/// The content part of an element's property table: a single text child
/// shows as the next integer key, other children by name.
fn content_props(a: &Arena, n: NodeId, f: &Filter, out: &mut Vec<(Key, Pv)>) {
    if let Some(c) = a.first_child(n)
        && a.next_sibling(c).is_none()
        && let NodeData::Text(t) = a.data(c)
    {
        if !is_blank(t) {
            let i = out.iter().filter(|(k, _)| matches!(k, Key::Int(_))).count();
            out.push((Key::Int(i as i64), Pv::Str(t.clone())));
        }
        return;
    }
    for c in a.children(n) {
        match a.data(c) {
            NodeData::Element(e) => {
                if name_matches(&e.name, f) {
                    let k = Key::from_bytes(e.name.local.as_bytes());
                    push_prop(out, k, child_value(a, c, f));
                }
            }
            NodeData::Comment(_) => {
                push_prop(out, Key::from_bytes(b"comment"), Pv::Obj(Spec::of(c, f)));
            }
            NodeData::Pi { target, .. } => {
                let k = Key::from_bytes(target.as_bytes());
                push_prop(out, k, Pv::Obj(Spec::of(c, f)));
            }
            NodeData::EntityRef(name) => {
                let k = Key::from_bytes(name.as_bytes());
                push_prop(out, k, Pv::Obj(Spec::of(c, f)));
            }
            _ => {}
        }
    }
}

fn props(a: &Arena, s: &Spec) -> Vec<(Key, Pv)> {
    let mut out = Vec::new();
    if !s.pending.is_empty() || !a.is_live(s.node) {
        return out;
    }
    match &s.iter {
        Iter::None | Iter::Child => match a.data(s.node) {
            NodeData::Element(_) => element_props(a, s.node, &s.filter, &mut out),
            NodeData::Attribute(_) => out.push((Key::Int(0), Pv::Str(a.attr_value(s.node)))),
            NodeData::EntityRef(name) => {
                if let Some(v) = a.entity_value(a.doc_of(s.node), name) {
                    out.push((Key::from_bytes(name.as_bytes()), Pv::Str(v)));
                }
            }
            _ => {}
        },
        Iter::Elem(_) => {
            let m = members(a, s);
            if let Some(&f0) = m.first() {
                let at = attr_list(a, f0, &s.filter);
                if !at.is_empty() {
                    out.push((Key::from_bytes(b"@attributes"), Pv::Attrs(at)));
                }
            }
            // The members' contents, merged (observed on PHP 8.5: the
            // first's attributes, then each member's text or children).
            for n in m {
                content_props(a, n, &s.filter, &mut out);
            }
        }
        Iter::Attrs(n) => {
            let at: Vec<(String, String)> = attrs_of(a, s.node, &s.filter, n.as_deref())
                .into_iter()
                .map(|at| (local_name(a, at), a.attr_value(at)))
                .collect();
            if !at.is_empty() {
                out.push((Key::from_bytes(b"@attributes"), Pv::Attrs(at)));
            }
        }
    }
    out
}

fn pv_value(vm: &mut Vm, cls: &Rc<Class>, v: Pv) -> R<Value> {
    Ok(match v {
        Pv::Str(s) => Value::from(s.as_bytes()),
        Pv::Obj(sp) => make(vm, cls, sp)?,
        Pv::List(l) => {
            let mut arr = PhpArray::new();
            for x in l {
                let v = pv_value(vm, cls, x)?;
                let _ = arr.push(v);
            }
            Value::Array(arr)
        }
        Pv::Attrs(at) => {
            let mut arr = PhpArray::new();
            for (k, v) in at {
                arr.insert(Key::from_bytes(k.as_bytes()), Value::from(v.as_bytes()));
            }
            Value::Array(arr)
        }
    })
}

fn props_array(vm: &mut Vm, o: &ObjRef) -> Option<PhpArray> {
    let s = spec_of(o)?;
    let list = xmlstore::with(|st| props(&st.arena, &s));
    let cls = vm.class_of(o);
    let mut arr = PhpArray::new();
    for (k, v) in list {
        let v = pv_value(vm, &cls, v).ok()?;
        arr.insert(k, v);
    }
    Some(arr)
}

// ------------------------------------------------------------------ hooks

fn hooks() -> NativeHooks {
    NativeHooks {
        clone: Some(|_, from, to| {
            let Some(s) = spec_of(from) else { return };
            // A clone gets its own copy of the document (observed on PHP
            // 8.5: changes to a clone don't show in the original).
            let copy = xmlstore::with(|st| {
                let a = &mut st.arena;
                let d = a.doc_of(s.node);
                let nd = a.clone_node(d, d, true);
                // Map the node to its copy by path.
                let path = node_path(a, s.node);
                let n = follow_path(a, nd, &path).unwrap_or(nd);
                let hold = st.hold(n);
                (hold, nd)
            });
            *to.obj().native.borrow_mut() = Some(Box::new(Sxe {
                node: copy.0,
                iter: s.iter,
                filter: s.filter,
                pending: s.pending,
                cursor: None,
            }));
        }),
        cast: Some(|vm, o, to| {
            let s = spec_of(o)?;
            Some(match to {
                CastTo::Bool => Ok(Value::Bool(truthy(&s))),
                CastTo::Int | CastTo::Float => {
                    let t = xmlstore::with(|st| string_of(&st.arena, &s));
                    let v = Value::from(t.as_bytes());
                    if to == CastTo::Int {
                        Ok(Value::Int(goro_runtime::convert::to_int(&v, vm)))
                    } else {
                        Ok(Value::Float(goro_runtime::convert::to_float(&v, vm)))
                    }
                }
                CastTo::String => {
                    let t = xmlstore::with(|st| string_of(&st.arena, &s));
                    Ok(Value::from(t.as_bytes()))
                }
                _ => return None,
            })
        }),
        count: Some(|_, o| {
            Ok(match spec_of(o) {
                Some(s) => xmlstore::with(|st| count_of(&st.arena, &s)),
                None => 0,
            })
        }),
        debug_props: Some(props_array),
        props_for: Some(props_array),
        compare: Some(compare),
        // Arithmetic uses the value as a number (observed on PHP 8.5).
        do_operation: Some(|vm, op, x, y| {
            let conv = |v: &Value| -> Option<Value> {
                let Value::Object(o) = v else { return None };
                let s = spec_of(o)?;
                let t = xmlstore::with(|st| string_of(&st.arena, &s));
                Some(
                    match goro_runtime::numeric::parse_numeric(t.as_bytes()).num {
                        goro_runtime::Number::Int(i) => Value::Int(i),
                        goro_runtime::Number::Float(f) => Value::Float(f),
                    },
                )
            };
            let (cx, cy) = (conv(x), conv(y));
            if cx.is_none() && cy.is_none() {
                return None;
            }
            let a = cx.unwrap_or_else(|| x.clone());
            let b = cy.unwrap_or_else(|| y.clone());
            let bin = match op {
                goro_runtime::ops::BinOp::Add => crate::bytecode::Bin::Add,
                goro_runtime::ops::BinOp::Sub => crate::bytecode::Bin::Sub,
                goro_runtime::ops::BinOp::Mul => crate::bytecode::Bin::Mul,
                goro_runtime::ops::BinOp::Div => crate::bytecode::Bin::Div,
                goro_runtime::ops::BinOp::Mod => crate::bytecode::Bin::Mod,
                goro_runtime::ops::BinOp::Pow => crate::bytecode::Bin::Pow,
                _ => return None,
            };
            Some(vm.binary_op(bin, &a, &b))
        }),
        // `++`, `--` and `-` work on the value as a number (observed on PHP
        // 8.5).
        do_unary: Some(|vm, op, o| {
            let s = spec_of(o)?;
            let t = xmlstore::with(|st| string_of(&st.arena, &s));
            let n = goro_runtime::numeric::parse_numeric(t.as_bytes()).num;
            let v = match n {
                goro_runtime::Number::Int(i) => Value::Int(i),
                goro_runtime::Number::Float(f) => Value::Float(f),
            };
            let one = Value::Int(1);
            Some(match op {
                crate::class::UnaryOp::Inc => vm.binary_op(crate::bytecode::Bin::Add, &v, &one),
                crate::class::UnaryOp::Dec => vm.binary_op(crate::bytecode::Bin::Sub, &v, &one),
                crate::class::UnaryOp::Neg => {
                    vm.binary_op(crate::bytecode::Bin::Mul, &v, &Value::Int(-1))
                }
                crate::class::UnaryOp::BitNot => return None,
            })
        }),
        prop: Some(prop_hook),
        dim: Some(dim_hook),
        ..Default::default()
    }
}

fn node_path(a: &Arena, mut n: NodeId) -> Vec<(bool, usize)> {
    let mut p = Vec::new();
    while let Some(par) = a.parent(n) {
        if matches!(a.data(n), NodeData::Attribute(_)) {
            let i = a.attrs(par).position(|x| x == n).unwrap_or(0);
            p.push((true, i));
        } else {
            let i = a.children(par).position(|x| x == n).unwrap_or(0);
            p.push((false, i));
        }
        n = par;
    }
    p.reverse();
    p
}

fn follow_path(a: &Arena, mut n: NodeId, p: &[(bool, usize)]) -> Option<NodeId> {
    for &(attr, i) in p {
        n = if attr {
            a.attrs(n).nth(i)?
        } else {
            a.children(n).nth(i)?
        };
    }
    Some(n)
}

fn count_of(a: &Arena, s: &Spec) -> i64 {
    match &s.iter {
        Iter::None => {
            if !s.pending.is_empty() || !a.is_live(s.node) {
                0
            } else if a.is_element(s.node) {
                child_elems(a, s.node, &s.filter, None).len() as i64
            } else {
                0
            }
        }
        _ => members(a, s).len() as i64,
    }
}

fn truthy(s: &Spec) -> bool {
    xmlstore::with(|st| {
        let a = &st.arena;
        match &s.iter {
            Iter::None => {
                if !s.pending.is_empty() || !a.is_live(s.node) {
                    return false;
                }
                if a.is_element(s.node) {
                    !props(a, s).is_empty()
                } else {
                    true
                }
            }
            _ => !members(a, s).is_empty(),
        }
    })
}

fn cmp(vm: &mut Vm, a: &Value, b: &Value) -> R<i32> {
    goro_runtime::compare::compare(a, b, vm).map_err(|e| vm.php_error(e))
}

fn compare(vm: &mut Vm, x: &Value, y: &Value) -> Option<R<i32>> {
    match (x, y) {
        (Value::Object(a), Value::Object(b)) => {
            let (sa, sb) = (spec_of(a), spec_of(b));
            match (sa, sb) {
                (Some(sa), Some(sb)) => {
                    if sa.node == sb.node && sa.iter == sb.iter && sa.pending == sb.pending {
                        Some(Ok(0))
                    } else {
                        Some(Ok(1))
                    }
                }
                (Some(sa), None) => {
                    let t = xmlstore::with(|st| string_of(&st.arena, &sa));
                    Some(cmp(vm, &Value::from(t.as_bytes()), y))
                }
                (None, Some(sb)) => {
                    let t = xmlstore::with(|st| string_of(&st.arena, &sb));
                    Some(cmp(vm, x, &Value::from(t.as_bytes())))
                }
                _ => None,
            }
        }
        (Value::Object(a), other) => {
            let sa = spec_of(a)?;
            let left = scalar_like(&sa, other);
            Some(cmp(vm, &left, other))
        }
        (other, Value::Object(b)) => {
            let sb = spec_of(b)?;
            let right = scalar_like(&sb, other);
            Some(cmp(vm, other, &right))
        }
        _ => None,
    }
}

/// The object as the scalar it is compared with (its string, or its
/// truth for booleans and null).
fn scalar_like(s: &Spec, other: &Value) -> Value {
    match other {
        Value::Bool(_) | Value::Null => Value::Bool(truthy(s)),
        _ => {
            let t = xmlstore::with(|st| string_of(&st.arena, s));
            Value::from(t.as_bytes())
        }
    }
}

// ------------------------------------------------------------------ access

fn value_text(vm: &mut Vm, v: &Value) -> R<String> {
    Ok(match v {
        Value::Null => String::new(),
        Value::Object(o) if spec_of(o).is_some() => {
            let s = spec_of(o).unwrap();
            xmlstore::with(|st| string_of(&st.arena, &s))
        }
        other => {
            let s = vm.to_php_str(other)?;
            String::from_utf8_lossy(s.as_bytes()).into_owned()
        }
    })
}

/// Creates the pending elements of `s`; the element they end at.
fn vivify(a: &mut Arena, s: &Spec) -> Option<NodeId> {
    if !a.is_live(s.node) {
        return None;
    }
    let mut n = s.node;
    for name in &s.pending {
        let existing = child_elems(a, n, &s.filter, Some(name)).first().copied();
        n = match existing {
            Some(e) => e,
            None => new_child(a, n, name, &s.filter, None),
        };
    }
    Some(n)
}

/// Appends a new element `name` (in the filter's namespace when it names
/// one) with optional text.
fn new_child(a: &mut Arena, parent: NodeId, name: &str, f: &Filter, text: Option<&str>) -> NodeId {
    let doc = a.doc_of(parent);
    let mut q = QName::local(name);
    if let Some(ns) = &f.ns {
        if f.is_prefix {
            if !ns.is_empty()
                && let Some(u) = a.lookup_ns(parent, Some(ns))
            {
                q.prefix = Some(ns.as_str().into());
                q.ns = Some(u);
            }
        } else if !ns.is_empty() {
            q.ns = Some(ns.as_str().into());
            if let Some(p) = a.lookup_prefix(parent, ns) {
                q.prefix = Some(p);
            }
        }
    } else if a.is_element(parent)
        && let Some(e) = a.element(parent)
        && e.name.prefix.is_none()
    {
        // Unprefixed children of an element in the default namespace stay
        // in it.
        q.ns = e.name.ns.clone();
    }
    let el = a.new_element(doc, q);
    if let Some(t) = text
        && !t.is_empty()
    {
        let tn = a.new_text(doc, t);
        a.raw_append(el, tn);
    }
    a.raw_append(parent, el);
    el
}

fn set_content(a: &mut Arena, n: NodeId, text: &str) {
    if matches!(a.data(n), NodeData::Attribute(_)) {
        a.set_attr_value(n, text);
        return;
    }
    let kids: Vec<NodeId> = a.children(n).collect();
    for k in kids {
        a.detach(k);
        if !a.subtree_held(k) {
            a.free_subtree(k);
        }
    }
    if !text.is_empty() {
        let doc = a.doc_of(n);
        let t = a.new_text(doc, text);
        a.raw_append(n, t);
    }
}

fn set_attr(a: &mut Arena, el: NodeId, name: &str, f: &Filter, value: &str) {
    if let Some(at) = attrs_of(a, el, f, Some(name)).first() {
        a.set_attr_value(*at, value);
        return;
    }
    let doc = a.doc_of(el);
    let mut q = QName::local(name);
    if let Some(ns) = &f.ns
        && !ns.is_empty()
    {
        if f.is_prefix {
            if let Some(u) = a.lookup_ns(el, Some(ns)) {
                q.prefix = Some(ns.as_str().into());
                q.ns = Some(u);
            }
        } else if let Some(p) = a.lookup_prefix(el, ns) {
            q.prefix = Some(p);
            q.ns = Some(ns.as_str().into());
        }
    }
    let at = a.new_attribute(doc, q, value);
    a.raw_append_attr(el, at);
}

fn remove_node(st: &mut xmlstore::Store, n: NodeId) {
    st.arena.detach(n);
    st.free_if_unheld(n);
}

fn prop_hook(vm: &mut Vm, o: &ObjRef, name: &PhpStr, op: ObjAccess) -> Option<R<Value>> {
    let s = spec_of(o)?;
    let name = name.to_string_lossy();
    Some(prop_access(vm, o, s, &name, op))
}

fn prop_access(vm: &mut Vm, o: &ObjRef, s: Spec, name: &str, op: ObjAccess) -> R<Value> {
    let cls = vm.class_of(o);
    match op {
        ObjAccess::Read | ObjAccess::Fetch => {
            if matches!(s.iter, Iter::Attrs(_)) {
                let at = xmlstore::with(|st| {
                    base(&st.arena, &s).and_then(|b| {
                        attrs_of(&st.arena, b, &s.filter, Some(name))
                            .first()
                            .copied()
                    })
                });
                return match at {
                    Some(at) => make(vm, &cls, Spec::of(at, &s.filter)),
                    None => Ok(Value::Null),
                };
            }
            let target = xmlstore::with(|st| base(&st.arena, &s));
            let spec = match target {
                Some(b) => Spec {
                    node: b,
                    iter: Iter::Elem(name.to_string()),
                    filter: s.filter.clone(),
                    pending: Vec::new(),
                },
                // Below a missing element: null (observed on PHP 8.5).
                None => return Ok(Value::Null),
            };
            make(vm, &cls, spec)
        }
        ObjAccess::Isset | ObjAccess::Empty => {
            let empty = matches!(op, ObjAccess::Empty);
            let found = xmlstore::with(|st| {
                let a = &st.arena;
                let b = base(a, &s)?;
                if matches!(s.iter, Iter::Attrs(_)) {
                    return attrs_of(a, b, &s.filter, Some(name)).first().copied();
                }
                child_elems(a, b, &s.filter, Some(name)).first().copied()
            });
            Ok(Value::Bool(match found {
                None => empty,
                Some(n) => {
                    if empty {
                        let sp = Spec::of(n, &s.filter);
                        !truthy(&sp)
                    } else {
                        true
                    }
                }
            }))
        }
        ObjAccess::Write(v) => {
            if let Value::Array(_) = v {
                return vm.throw(
                    "Error",
                    "It's not possible to assign a complex type to properties, array given",
                );
            }
            let text = value_text(vm, &v)?;
            let warn = xmlstore::with(|st| {
                let a = &mut st.arena;
                let b = if s.pending.is_empty() {
                    match &s.iter {
                        Iter::Elem(_) => match first(a, &s) {
                            Some(f) => Some(f),
                            None => {
                                let mut sp = s.clone();
                                if let Iter::Elem(n) = &s.iter {
                                    sp.pending.push(n.clone());
                                }
                                sp.iter = Iter::None;
                                vivify(a, &sp)
                            }
                        },
                        _ => Some(s.node),
                    }
                } else {
                    let mut sp = s.clone();
                    if let Iter::Elem(n) = &s.iter {
                        sp.pending.push(n.clone());
                    }
                    vivify(a, &sp)
                };
                let Some(b) = b else { return false };
                if matches!(s.iter, Iter::Attrs(_)) {
                    set_attr(a, b, name, &s.filter, &text);
                    return false;
                }
                let m = child_elems(a, b, &s.filter, Some(name));
                match m.len() {
                    0 => {
                        new_child(a, b, name, &s.filter, Some(&text));
                        false
                    }
                    1 => {
                        set_content(a, m[0], &text);
                        false
                    }
                    _ => true,
                }
            });
            if warn {
                vm.warn("main(): Cannot assign to an array of nodes (duplicate subnodes or attr detected)")?;
            }
            Ok(Value::Null)
        }
        ObjAccess::Unset => {
            xmlstore::with(|st| {
                let Some(b) = base(&st.arena, &s) else { return };
                let list = if matches!(s.iter, Iter::Attrs(_)) {
                    attrs_of(&st.arena, b, &s.filter, Some(name))
                } else {
                    child_elems(&st.arena, b, &s.filter, Some(name))
                };
                for n in list {
                    remove_node(st, n);
                }
            });
            Ok(Value::Null)
        }
    }
}

fn dim_hook(vm: &mut Vm, o: &ObjRef, k: Option<&Value>, op: ObjAccess) -> Option<R<Value>> {
    let s = spec_of(o)?;
    Some(dim_access(vm, o, s, k, op))
}

enum DKey {
    Index(i64),
    Name(String),
}

fn dim_key(vm: &mut Vm, k: &Value) -> R<DKey> {
    Ok(match k {
        Value::Int(i) => DKey::Index(*i),
        Value::Bool(b) => DKey::Index(*b as i64),
        Value::Float(f) => DKey::Index(*f as i64),
        Value::Null => DKey::Name(String::new()),
        Value::Str(s) => match goro_runtime::array::canonical_int(s) {
            Some(i) => DKey::Index(i),
            None => DKey::Name(s.to_string_lossy()),
        },
        Value::Object(o) if spec_of(o).is_some() => {
            let t = value_text(vm, k)?;
            DKey::Name(t)
        }
        other => {
            return vm.throw(
                "TypeError",
                &format!(
                    "Cannot access offset of type {} on SimpleXMLElement",
                    crate::class::TypeName::tname(other)
                ),
            );
        }
    })
}

fn dim_access(vm: &mut Vm, o: &ObjRef, s: Spec, k: Option<&Value>, op: ObjAccess) -> R<Value> {
    let cls = vm.class_of(o);
    let key = match k {
        Some(k) => Some(dim_key(vm, k)?),
        None => None,
    };
    let op = match (op, &key) {
        // A missing member fetched for a write is created (observed on
        // PHP 8.5; test 027).
        (ObjAccess::Fetch, Some(DKey::Index(i))) => {
            let (len, name) = xmlstore::with(|st| (members(&st.arena, &s).len(), s.iter.clone()));
            if *i >= 0
                && (*i as usize) >= len
                && let Iter::Elem(nm) = name
            {
                if *i as usize > len {
                    vm.warn(&format!(
                        "main(): Cannot add element {nm} number {i} when only {len} such elements exist"
                    ))?;
                }
                let el = xmlstore::with(|st| {
                    let a = &mut st.arena;
                    let mut sp = s.clone();
                    sp.iter = Iter::None;
                    vivify(a, &sp).map(|p| new_child(a, p, &nm, &s.filter, None))
                });
                return match el {
                    Some(el) => make(vm, &cls, Spec::of(el, &s.filter)),
                    None => Ok(Value::Null),
                };
            }
            ObjAccess::Read
        }
        (ObjAccess::Fetch, _) => ObjAccess::Read,
        (op, _) => op,
    };
    match (key, op) {
        // ---- attributes by name
        (Some(DKey::Name(n)), ObjAccess::Read) => {
            let at = xmlstore::with(|st| {
                let a = &st.arena;
                let b = if matches!(s.iter, Iter::Attrs(_)) {
                    Some(s.node)
                } else {
                    first(a, &s).filter(|&f| a.is_element(f))
                }?;
                attrs_of(a, b, &s.filter, Some(&n)).first().copied()
            });
            match at {
                Some(at) => make(vm, &cls, Spec::of(at, &s.filter)),
                None => Ok(Value::Null),
            }
        }
        (Some(DKey::Name(n)), op @ (ObjAccess::Isset | ObjAccess::Empty)) => {
            let at = xmlstore::with(|st| {
                let a = &st.arena;
                let b = if matches!(s.iter, Iter::Attrs(_)) {
                    Some(s.node)
                } else {
                    first(a, &s).filter(|&f| a.is_element(f))
                }?;
                attrs_of(a, b, &s.filter, Some(&n))
                    .first()
                    .map(|&x| a.attr_value(x))
            });
            Ok(Value::Bool(match (at, matches!(op, ObjAccess::Empty)) {
                (None, e) => e,
                (Some(_), false) => true,
                (Some(v), true) => v.is_empty() || v == "0",
            }))
        }
        (Some(DKey::Name(n)), ObjAccess::Write(v)) => {
            if n.is_empty() {
                return vm.throw("ValueError", "Cannot create attribute with an empty name");
            }
            let text = value_text(vm, &v)?;
            xmlstore::with(|st| {
                let a = &mut st.arena;
                let b = if matches!(s.iter, Iter::Attrs(_)) {
                    Some(s.node)
                } else if s.pending.is_empty() {
                    match first(a, &s) {
                        Some(f) => Some(f),
                        None => {
                            let mut sp = s.clone();
                            if let Iter::Elem(nm) = &s.iter {
                                sp.pending.push(nm.clone());
                            }
                            vivify(a, &sp)
                        }
                    }
                } else {
                    let mut sp = s.clone();
                    if let Iter::Elem(nm) = &s.iter {
                        sp.pending.push(nm.clone());
                    }
                    vivify(a, &sp)
                };
                if let Some(b) = b
                    && a.is_element(b)
                {
                    set_attr(a, b, &n, &s.filter, &text);
                }
            });
            Ok(Value::Null)
        }
        (Some(DKey::Name(n)), ObjAccess::Unset) => {
            xmlstore::with(|st| {
                let a = &st.arena;
                let b = if matches!(s.iter, Iter::Attrs(_)) {
                    Some(s.node)
                } else {
                    first(a, &s)
                };
                if let Some(b) = b {
                    for at in attrs_of(a, b, &s.filter, Some(&n)) {
                        remove_node(st, at);
                    }
                }
            });
            Ok(Value::Null)
        }
        // ---- members by index
        (Some(DKey::Index(i)), ObjAccess::Read) => {
            let n = xmlstore::with(|st| {
                let m = members(&st.arena, &s);
                if i < 0 {
                    None
                } else {
                    m.get(i as usize).copied()
                }
            });
            match n {
                Some(n) => make(vm, &cls, Spec::of(n, &s.filter)),
                None => Ok(Value::Null),
            }
        }
        (Some(DKey::Index(i)), op @ (ObjAccess::Isset | ObjAccess::Empty)) => {
            let n = xmlstore::with(|st| {
                let m = members(&st.arena, &s);
                if i < 0 {
                    None
                } else {
                    m.get(i as usize).copied()
                }
            });
            Ok(Value::Bool(match n {
                None => matches!(op, ObjAccess::Empty),
                Some(n) => {
                    if matches!(op, ObjAccess::Empty) {
                        !truthy(&Spec::of(n, &s.filter))
                    } else {
                        true
                    }
                }
            }))
        }
        (Some(DKey::Index(i)), ObjAccess::Write(v)) => {
            let text = value_text(vm, &v)?;
            xmlstore::with(|st| {
                let a = &mut st.arena;
                let m = members(a, &s);
                if i >= 0 && (i as usize) < m.len() {
                    set_content(a, m[i as usize], &text);
                    None
                } else if let Iter::Elem(name) = &s.iter {
                    // Past the end: a new element.
                    let mut sp = s.clone();
                    sp.iter = Iter::None;
                    if let Some(p) = vivify(a, &sp) {
                        new_child(a, p, name, &s.filter, Some(&text));
                    }
                    (i as usize > m.len()).then(|| (name.clone(), m.len()))
                } else {
                    None
                }
            })
            .map_or(Ok(()), |(nm, len)| {
                vm.warn(&format!(
                    "main(): Cannot add element {nm} number {i} when only {len} such elements exist"
                ))
            })?;
            Ok(Value::Null)
        }
        (Some(DKey::Index(i)), ObjAccess::Unset) => {
            xmlstore::with(|st| {
                let m = members(&st.arena, &s);
                if i >= 0
                    && let Some(&n) = m.get(i as usize)
                {
                    remove_node(st, n);
                }
            });
            Ok(Value::Null)
        }
        // ---- append
        (None, ObjAccess::Write(v)) => {
            let Iter::Elem(name) = s.iter.clone() else {
                return vm.throw("ValueError", "Cannot append to an attribute list");
            };
            let text = value_text(vm, &v)?;
            xmlstore::with(|st| {
                let a = &mut st.arena;
                let mut sp = s.clone();
                sp.iter = Iter::None;
                if let Some(p) = vivify(a, &sp) {
                    new_child(a, p, &name, &s.filter, Some(&text));
                }
            });
            Ok(Value::Null)
        }
        (Some(_), ObjAccess::Fetch) => Ok(Value::Null),
        (None, _) => vm.throw("Error", "Cannot use [] for reading"),
    }
}

// ------------------------------------------------------------------ loading

fn class_arg(vm: &mut Vm, a: &Args, i: usize) -> R<Option<Rc<Class>>> {
    match a.opt_str(i) {
        None => Ok(Some(
            vm.find_class(b"SimpleXMLElement", false)?
                .expect("SimpleXMLElement is declared"),
        )),
        Some(n) => match vm.find_class(n.as_bytes(), true)? {
            Some(c) => {
                let base = vm
                    .find_class(b"SimpleXMLElement", false)?
                    .expect("SimpleXMLElement is declared");
                if !c.is_subclass_of(&base) {
                    return vm.fthrow(
                        a,
                        "TypeError",
                        &format!(
                            "Argument #{} ($class_name) must be a class name derived from SimpleXMLElement, {} given",
                            i + 1,
                            n.to_string_lossy()
                        ),
                    );
                }
                Ok(Some(c))
            }
            None => vm.fthrow(
                a,
                "TypeError",
                &format!(
                    "Argument #{} ($class_name) must be a class name derived from SimpleXMLElement, {} given",
                    i + 1,
                    n.to_string_lossy()
                ),
            ),
        },
    }
}

/// Parses a document into the store. The root element, or None (errors
/// reported).
fn parse_doc(
    vm: &mut Vm,
    fname: &str,
    input: &[u8],
    flags: u32,
    url: Option<String>,
) -> R<Option<NodeId>> {
    let mut opts = goro_xml::xml::ParseOptions::with_flags(flags);
    opts.url = url;
    opts.load_external_entities = flags & goro_xml::xml::opt::NOENT != 0;
    let mut loader = libxml::PhpLoader::new(vm, fname);
    loader.nonet = flags & goro_xml::xml::opt::NONET != 0;
    let (built, root) = if libxml::has_user_loader(vm) {
        // The user loader runs PHP code: parse outside the store.
        let mut tmp = Arena::new();
        let b = goro_xml::xml::build(&mut tmp, input, &opts, Some(&mut loader));
        // Import the whole document.
        let doc = xmlstore::with(|st| {
            let nd = st.arena.new_document(DocKind::Xml);
            for c in tmp.children(b.doc).collect::<Vec<_>>() {
                let cc = st.arena.import_from(&tmp, c, nd);
                st.arena.raw_append(nd, cc);
            }
            let src = tmp.doc(b.doc);
            let (enc, ver, sa, url) = (
                src.encoding.clone(),
                src.version.clone(),
                src.standalone,
                src.url.clone(),
            );
            let dd = st.arena.doc_mut(nd);
            dd.encoding = enc;
            dd.version = ver;
            dd.standalone = sa;
            dd.url = url;
            nd
        });
        let root = xmlstore::with(|st| st.arena.document_element(doc));
        (
            goro_xml::xml::Built {
                doc,
                errors: b.errors,
                ok: b.ok,
            },
            root,
        )
    } else {
        let b = xmlstore::with(|st| {
            goro_xml::xml::build(&mut st.arena, input, &opts, Some(&mut loader))
        });
        let root = xmlstore::with(|st| st.arena.document_element(b.doc));
        (b, root)
    };
    loader.finish(vm)?;
    libxml::report(vm, fname, &built.errors, Style::Generic, flags, Some(input))?;
    if !built.ok {
        xmlstore::with(|st| st.free_doc_if_unheld(built.doc));
        return Ok(None);
    }
    // A recovered document without an element stands for itself (observed
    // on PHP 8.5; test SimpleXMLElement_getDocNamespaces).
    let root = root.or(Some(built.doc));
    if flags & goro_xml::xml::opt::XINCLUDE != 0 {
        // XInclude processing is DOM's; SimpleXML documents rarely use it.
    }
    Ok(root)
}

fn ns_arg(a: &Args, i: usize, j: usize) -> Filter {
    let ns = a.opt_str(i).map(|s| s.to_string_lossy());
    Filter {
        ns: ns.filter(|s| !s.is_empty()),
        is_prefix: a.bool(j),
    }
}

fn load_common(vm: &mut Vm, a: &Args, input: &[u8], url: Option<String>) -> R<Value> {
    let Some(cls) = class_arg(vm, a, 1)? else {
        return Ok(Value::Bool(false));
    };
    let flags = a.int(2) as u32;
    let filter = ns_arg(a, 3, 4);
    let fname = a.fname();
    let Some(root) = parse_doc(vm, &fname, input, flags, url)? else {
        return Ok(Value::Bool(false));
    };
    let v = make(vm, &cls, Spec::of(root, &filter))?;
    xmlstore::with(|st| {
        let d = st.arena.doc_of(root);
        st.free_doc_if_unheld(d)
    });
    Ok(v)
}

fn simplexml_load_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.str(0);
    load_common(vm, a, data.as_bytes(), None)
}

fn simplexml_load_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let path = a.str(0).to_string_lossy();
    let fname = a.fname();
    let url = libxml::resolve(vm, &path, None);
    match libxml::load_document(vm, &fname, &path)? {
        Ok(bytes) => load_common(vm, a, &bytes, Some(url)),
        Err(msg) => {
            if let Some(m) = msg {
                vm.fwarn(a, &m)?;
            }
            let e = libxml::io_error(&path);
            libxml::report(vm, &fname, &[e], Style::Generic, 0, None)?;
            Ok(Value::Bool(false))
        }
    }
}

fn simplexml_import_dom(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.obj(0).expect("object argument");
    // A SimpleXML object imports as itself (observed on PHP 8.5).
    let node = match super::dom::node_of(&o) {
        Some(n) => n,
        None => match sxe_node_of(&o) {
            Some(n) => n,
            None => {
                return vm.fthrow(
                    a,
                    "TypeError",
                    "Argument #1 ($node) must be of type SimpleXMLElement|DOMNode, invalid given",
                );
            }
        },
    };
    let node = xmlstore::with(|st| {
        let ar = &st.arena;
        match ar.node_type(node) {
            NodeType::Document | NodeType::HtmlDocument => ar.document_element(node),
            NodeType::Element | NodeType::Attribute => Some(node),
            _ => None,
        }
    });
    let Some(node) = node else {
        return vm.fthrow(
            a,
            "TypeError",
            "Argument #1 ($node) must be of type SimpleXMLElement|DOMNode, invalid given",
        );
    };
    let Some(cls) = class_arg(vm, a, 1)? else {
        return Ok(Value::Null);
    };
    make(vm, &cls, Spec::of(node, &Filter::default()))
}

fn m_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let data = a.str(0);
    let flags = a.int(1) as u32;
    let is_url = a.bool(2);
    let filter = ns_arg(a, 3, 4);
    let (input, url) = if is_url {
        let path = data.to_string_lossy();
        let url = libxml::resolve(vm, &path, None);
        match libxml::load_document(vm, "SimpleXMLElement::__construct", &path)? {
            Ok(b) => (b, Some(url)),
            Err(_) => {
                let e = libxml::io_error(&path);
                libxml::report(
                    vm,
                    "SimpleXMLElement::__construct",
                    &[e],
                    Style::Generic,
                    0,
                    None,
                )?;
                return vm.throw("Exception", "String could not be parsed as XML");
            }
        }
    } else {
        (data.as_bytes().to_vec(), None)
    };
    let Some(root) = parse_doc(vm, "SimpleXMLElement::__construct", &input, flags, url)? else {
        return vm.throw("Exception", "String could not be parsed as XML");
    };
    let hold = xmlstore::with(|st| st.hold(root));
    *o.obj().native.borrow_mut() = Some(Box::new(Sxe {
        node: hold,
        iter: Iter::None,
        filter,
        pending: Vec::new(),
        cursor: None,
    }));
    Ok(Value::Null)
}

// ------------------------------------------------------------------ methods

fn this_spec(vm: &mut Vm, a: &Args) -> R<(ObjRef, Spec)> {
    let o = a.this_obj().clone();
    let s = spec(vm, &o)?;
    Ok((o, s))
}

fn m_to_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, s) = this_spec(vm, a)?;
    let t = xmlstore::with(|st| string_of(&st.arena, &s));
    Ok(Value::from(t.as_bytes()))
}

fn m_debug_info(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    Ok(props_array(vm, &o).map_or(Value::Null, Value::Array))
}

fn m_count(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, s) = this_spec(vm, a)?;
    Ok(Value::Int(xmlstore::with(|st| count_of(&st.arena, &s))))
}

fn m_get_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, s) = this_spec(vm, a)?;
    let n = xmlstore::with(|st| first(&st.arena, &s).map(|n| local_name(&st.arena, n)));
    Ok(Value::from(n.unwrap_or_default().as_bytes()))
}

fn m_children(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, s) = this_spec(vm, a)?;
    let filter = Filter {
        ns: a.opt_str(0).map(|x| x.to_string_lossy()),
        is_prefix: a.bool(1),
    };
    let target = xmlstore::with(|st| {
        let ar = &st.arena;
        first(ar, &s).filter(|&n| ar.is_element(n))
    });
    let Some(t) = target else {
        return Ok(Value::Null);
    };
    let cls = vm.class_of(&o);
    make(
        vm,
        &cls,
        Spec {
            node: t,
            iter: Iter::Child,
            filter,
            pending: Vec::new(),
        },
    )
}

fn m_attributes(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, s) = this_spec(vm, a)?;
    let filter = Filter {
        ns: a.opt_str(0).map(|x| x.to_string_lossy()),
        is_prefix: a.bool(1),
    };
    if matches!(s.iter, Iter::Attrs(_)) {
        return Ok(Value::Null);
    }
    let target = xmlstore::with(|st| {
        let ar = &st.arena;
        first(ar, &s).filter(|&n| ar.is_element(n))
    });
    let Some(t) = target else {
        return Ok(Value::Null);
    };
    let cls = vm.class_of(&o);
    make(
        vm,
        &cls,
        Spec {
            node: t,
            iter: Iter::Attrs(None),
            filter,
            pending: Vec::new(),
        },
    )
}

/// Parses addChild()'s value: entity and character references are
/// resolved, a stray `&` is dropped (observed on PHP 8.5).
fn ref_text(a: &Arena, doc: NodeId, v: &str) -> String {
    let mut out = String::new();
    let mut rest = v;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find(';') {
            Some(j)
                if j > 0
                    && after[..j].chars().all(|c| {
                        c.is_alphanumeric() || matches!(c, '#' | '_' | '-' | '.' | ':')
                    }) =>
            {
                let name = &after[..j];
                if let Some(num) = name.strip_prefix('#') {
                    let cp = if let Some(h) = num.strip_prefix('x') {
                        u32::from_str_radix(h, 16).ok()
                    } else {
                        num.parse().ok()
                    };
                    if let Some(c) = cp.and_then(char::from_u32) {
                        out.push(c);
                    }
                } else if let Some(c) = goro_xml::tree::predefined_entity(name) {
                    out.push(c);
                } else if let Some(v) = a.entity_value(doc, name) {
                    out.push_str(&v);
                }
                rest = &after[j + 1..];
            }
            _ => rest = after,
        }
    }
    out.push_str(rest);
    out
}

fn m_add_child(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, s) = this_spec(vm, a)?;
    let qn = a.str(0).to_string_lossy();
    if qn.is_empty() {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($qualifiedName) must not be empty",
        );
    }
    let value = a.opt_str(1).map(|v| v.to_string_lossy());
    let ns = a.opt_str(2).map(|v| v.to_string_lossy());
    if matches!(s.iter, Iter::Attrs(_)) {
        vm.fwarn(a, "Cannot add element to attributes")?;
        return Ok(Value::Null);
    }
    let r = xmlstore::with(|st| {
        let ar = &mut st.arena;
        let parent = first(ar, &s).filter(|&n| ar.is_element(n))?;
        let doc = ar.doc_of(parent);
        let (prefix, local) = match qn.split_once(':') {
            Some((p, l)) if !p.is_empty() && !l.is_empty() => (Some(p.to_string()), l.to_string()),
            _ => (None, qn.clone()),
        };
        let mut q = QName::local(&local);
        let mut declare = None;
        match &ns {
            Some(u) if !u.is_empty() => {
                q.ns = Some(u.as_str().into());
                q.prefix = prefix.as_deref().map(Into::into);
                let in_scope = ar.lookup_ns(parent, prefix.as_deref());
                if in_scope.as_deref() != Some(u.as_str()) {
                    declare = Some((prefix.clone(), u.clone()));
                }
            }
            Some(_) => {
                // An empty namespace: no namespace (and the default one
                // undeclared when in scope).
                if ar.lookup_ns(parent, None).is_some_and(|d| !d.is_empty()) {
                    declare = Some((None, String::new()));
                }
            }
            None => {
                // Without a namespace the child takes its parent's
                // (observed on PHP 8.5).
                if let Some(pq) = ar.element(parent).map(|e| e.name.clone()) {
                    q.ns = pq.ns.clone();
                    q.prefix = pq.prefix.clone();
                }
                if prefix.is_some() {
                    q = QName::local(&qn);
                }
            }
        }
        let el = ar.new_element(doc, q);
        if let Some((p, u)) = declare {
            ar.declare_ns(el, p.as_deref(), &u);
        }
        if let Some(v) = &value {
            let t = ref_text(ar, doc, v);
            if !t.is_empty() {
                let tn = ar.new_text(doc, t);
                ar.raw_append(el, tn);
            }
        }
        ar.raw_append(parent, el);
        Some(el)
    });
    let Some(el) = r else {
        vm.fwarn(
            a,
            "Cannot add child. Parent is not a permanent member of the XML tree",
        )?;
        return Ok(Value::Null);
    };
    let cls = vm.class_of(&o);
    make(vm, &cls, Spec::of(el, &s.filter))
}

fn m_add_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, s) = this_spec(vm, a)?;
    let qn = a.str(0).to_string_lossy();
    if qn.is_empty() {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($qualifiedName) must not be empty",
        );
    }
    let value = a.str(1).to_string_lossy();
    let ns = a.opt_str(2).map(|v| v.to_string_lossy());
    let (prefix, local) = match qn.split_once(':') {
        Some((p, l)) if !p.is_empty() && !l.is_empty() => (Some(p.to_string()), l.to_string()),
        _ => (None, qn.clone()),
    };
    if prefix.is_some() && ns.as_deref().is_none_or(str::is_empty) {
        vm.fwarn(a, "Attribute requires prefix for namespace")?;
        return Ok(Value::Null);
    }
    enum Out {
        Ok,
        Exists,
        NotElement,
    }
    let r = xmlstore::with(|st| {
        let ar = &mut st.arena;
        let el = match s.iter {
            Iter::Attrs(_) => Some(s.node),
            _ => first(ar, &s),
        };
        let Some(el) = el.filter(|&n| ar.is_element(n)) else {
            return Out::NotElement;
        };
        let doc = ar.doc_of(el);
        let exists = ar.attrs(el).any(|at| {
            ar.attr(at).is_some_and(|d| {
                *d.name.local == *local
                    && match &ns {
                        Some(u) if !u.is_empty() => d.name.ns_str() == u,
                        _ => d.name.ns.is_none(),
                    }
            })
        });
        if exists {
            return Out::Exists;
        }
        let mut q = QName::local(&local);
        if let Some(u) = ns.as_ref().filter(|u| !u.is_empty()) {
            q.ns = Some(u.as_str().into());
            q.prefix = prefix.as_deref().map(Into::into);
            if ar.lookup_ns(el, prefix.as_deref()).as_deref() != Some(u.as_str()) {
                ar.declare_ns(el, prefix.as_deref(), u);
            }
        }
        let at = ar.new_attribute(doc, q, &value);
        ar.raw_append_attr(el, at);
        Out::Ok
    });
    match r {
        Out::Ok => {}
        Out::Exists => vm.fwarn(a, "Attribute already exists")?,
        Out::NotElement => {
            vm.fwarn(a, "Unable to locate parent Element")?;
        }
    }
    Ok(Value::Null)
}

fn m_as_xml(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, s) = this_spec(vm, a)?;
    let out = xmlstore::with(|st| {
        let ar = &st.arena;
        let n = first(ar, &s)?;
        let doc = ar.doc_of(n);
        // The document element of a document: the whole document.
        let whole = s.iter == Iter::None && ar.parent(n) == Some(doc);
        let opts = goro_xml::serialize::SaveOptions::default();
        Some(if whole {
            goro_xml::serialize::save_xml(ar, doc, &opts)
        } else if matches!(ar.data(n), NodeData::Attribute(_)) {
            // An attribute: ` name="value"` (observed on PHP 8.5).
            let mut t = String::from(" ");
            t.push_str(&ar.node_name(n));
            t.push_str("=\"");
            goro_xml::serialize::escape_attr(&ar.attr_value(n), &mut t);
            t.push('"');
            goro_xml::serialize::encode_output(&t, ar.doc(doc).encoding.as_deref())
        } else {
            goro_xml::serialize::save_xml(ar, n, &opts)
        })
    });
    let Some(bytes) = out else {
        return Ok(Value::Bool(false));
    };
    match a.opt_str(0) {
        None => Ok(Value::from(bytes.as_slice())),
        Some(path) => {
            let r = vm.call_value(
                &Value::from(&b"file_put_contents"[..]),
                vec![Value::Str(path), Value::from(bytes.as_slice())],
            )?;
            Ok(Value::Bool(!matches!(r, Value::Bool(false))))
        }
    }
}

fn ns_array(list: Vec<(String, String)>) -> Value {
    let mut arr = PhpArray::new();
    for (p, u) in list {
        if arr.get_bytes(p.as_bytes()).is_none() {
            arr.insert(Key::from_bytes(p.as_bytes()), Value::from(u.as_bytes()));
        }
    }
    Value::Array(arr)
}

fn used_ns(a: &Arena, n: NodeId, recursive: bool, out: &mut Vec<(String, String)>) {
    if let Some(e) = a.element(n) {
        if let Some(u) = &e.name.ns {
            out.push((
                e.name.prefix.as_deref().unwrap_or("").to_string(),
                u.to_string(),
            ));
        }
        for at in a.attrs(n) {
            if let Some(d) = a.attr(at)
                && let Some(u) = &d.name.ns
                && d.name.prefix.as_deref() != Some("xml")
            {
                out.push((
                    d.name.prefix.as_deref().unwrap_or("").to_string(),
                    u.to_string(),
                ));
            }
        }
        if recursive {
            for c in a.children(n) {
                if a.is_element(c) {
                    used_ns(a, c, true, out);
                }
            }
        }
    }
}

fn m_get_namespaces(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, s) = this_spec(vm, a)?;
    let rec = a.bool(0);
    let list = xmlstore::with(|st| {
        let ar = &st.arena;
        let mut out = Vec::new();
        for n in members(ar, &s) {
            match ar.data(n) {
                NodeData::Attribute(d) => {
                    if let Some(u) = &d.name.ns {
                        out.push((
                            d.name.prefix.as_deref().unwrap_or("").to_string(),
                            u.to_string(),
                        ));
                    }
                }
                _ => used_ns(ar, n, rec, &mut out),
            }
            if s.iter == Iter::None {
                break;
            }
        }
        out
    });
    Ok(ns_array(list))
}

fn declared_ns(a: &Arena, n: NodeId, recursive: bool, out: &mut Vec<(String, String)>) {
    if let Some(e) = a.element(n) {
        for d in &e.ns_decls {
            out.push((
                d.prefix.as_deref().unwrap_or("").to_string(),
                d.uri.to_string(),
            ));
        }
        if recursive {
            for c in a.children(n) {
                if a.is_element(c) {
                    declared_ns(a, c, true, out);
                }
            }
        }
    }
}

fn m_get_doc_namespaces(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, s) = this_spec(vm, a)?;
    let rec = a.bool(0);
    let from_root = a.bool_or(1, true);
    let list = xmlstore::with(|st| {
        let ar = &st.arena;
        let n = if from_root {
            ar.document_element(ar.doc_of(s.node))
        } else {
            first(ar, &s)
        }?;
        let mut out = Vec::new();
        declared_ns(ar, n, rec, &mut out);
        Some(out)
    });
    match list {
        Some(l) => Ok(ns_array(l)),
        None => Ok(Value::Bool(false)),
    }
}

/// Namespaces registered with registerXPathNamespace(), per document.
#[derive(Default)]
struct XPathNs(Vec<(String, String)>);

fn m_register_xpath_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, s) = this_spec(vm, a)?;
    let p = a.str(0).to_string_lossy();
    let u = a.str(1).to_string_lossy();
    xmlstore::with(|st| {
        let l = &mut st.doc_ext(s.node).get::<XPathNs>().0;
        l.retain(|(x, _)| *x != p);
        l.push((p, u));
    });
    Ok(Value::Bool(true))
}

fn m_xpath(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, s) = this_spec(vm, a)?;
    let expr = a.str(0).to_string_lossy();
    let compiled = match goro_xml::xpath::compile(&expr) {
        Ok(c) => c,
        Err(e) => {
            vm.fwarn(a, &e.message)?;
            return Ok(Value::Bool(false));
        }
    };
    let r = xmlstore::with(|st| {
        let ctx_node = first(&st.arena, &s).unwrap_or(s.node);
        let regs = st.doc_ext(s.node).get::<XPathNs>().0.clone();
        let mut ctx = goro_xml::xpath::Context::new();
        // The document element's namespaces are known (observed on PHP 8.5).
        let root = st.arena.document_element(st.arena.doc_of(ctx_node));
        if let Some(r) = root {
            for d in st.arena.in_scope_namespaces(r) {
                if let Some(p) = &d.prefix {
                    ctx.register_namespace(p, &d.uri);
                }
            }
        }
        for (p, u) in &regs {
            ctx.register_namespace(p, u);
        }
        let v = goro_xml::xpath::evaluate(&mut st.arena, &compiled, ctx_node, &mut ctx);
        let ns_nodes = std::mem::take(&mut ctx.namespace_nodes);
        (v, ns_nodes)
    });
    let (v, _ns_nodes) = r;
    let v = match v {
        Ok(v) => v,
        Err(e) => {
            vm.fwarn(a, &e.message)?;
            return Ok(Value::Bool(false));
        }
    };
    let nodes = match v {
        goro_xml::xpath::Value::NodeSet(n) => n,
        other => {
            let what = match other {
                goro_xml::xpath::Value::Number(_) => "number",
                goro_xml::xpath::Value::Boolean(_) => "boolean",
                _ => "string",
            };
            vm.fwarn(
                a,
                &format!("XPath expression must return a node set, {what} returned"),
            )?;
            return Ok(Value::Bool(false));
        }
    };
    let cls = vm.class_of(&o);
    let mut arr = PhpArray::new();
    for n in nodes {
        // Attributes come as their element's attribute list of that name
        // (observed on PHP 8.5; test 024), text as its element.
        let target = xmlstore::with(|st| {
            let ar = &st.arena;
            match ar.data(n) {
                NodeData::Element(_) => Some(Spec::of(n, &Filter::default())),
                NodeData::Attribute(d) => Some(Spec {
                    node: ar.parent(n)?,
                    iter: Iter::Attrs(Some(d.name.local.to_string())),
                    filter: Filter {
                        ns: d.name.ns.as_ref().map(|u| u.to_string()),
                        is_prefix: false,
                    },
                    pending: Vec::new(),
                }),
                NodeData::Text(_) | NodeData::CData(_) => {
                    Some(Spec::of(ar.parent(n)?, &Filter::default()))
                }
                _ => None,
            }
        });
        if let Some(t) = target {
            let v = make(vm, &cls, t)?;
            let _ = arr.push(v);
        }
    }
    Ok(Value::Array(arr))
}

// ------------------------------------------------------------------ iteration

fn cursor_of(o: &ObjRef) -> Option<NodeId> {
    with_sxe(o, |d| d.cursor.as_ref().map(|h| h.id())).flatten()
}

fn set_cursor(o: &ObjRef, n: Option<NodeId>) {
    let hold = n.map(|n| xmlstore::with(|st| st.hold(n)));
    let old = with_sxe(o, |d| std::mem::replace(&mut d.cursor, hold));
    drop(old);
}

fn m_rewind(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, s) = this_spec(vm, a)?;
    let f = xmlstore::with(|st| iteration(&st.arena, &s).first().copied());
    set_cursor(&o, f);
    Ok(Value::Null)
}

fn m_valid(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_spec(vm, a)?;
    Ok(Value::Bool(cursor_of(&o).is_some()))
}

fn m_current(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, s) = this_spec(vm, a)?;
    let Some(c) = cursor_of(&o) else {
        return vm.throw("Error", "Iterator not initialized or already consumed");
    };
    let cls = vm.class_of(&o);
    make(vm, &cls, Spec::of(c, &s.filter))
}

fn m_key(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_spec(vm, a)?;
    let Some(c) = cursor_of(&o) else {
        return vm.throw("Error", "Iterator not initialized or already consumed");
    };
    let n = xmlstore::with(|st| local_name(&st.arena, c));
    Ok(Value::from(n.as_bytes()))
}

fn m_next(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, s) = this_spec(vm, a)?;
    let Some(c) = cursor_of(&o) else {
        return Ok(Value::Null);
    };
    let nx = xmlstore::with(|st| {
        let list = iteration(&st.arena, &s);
        let i = list.iter().position(|&x| x == c)?;
        list.get(i + 1).copied()
    });
    set_cursor(&o, nx);
    Ok(Value::Null)
}

fn m_has_children(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, s) = this_spec(vm, a)?;
    let Some(c) = cursor_of(&o) else {
        return Ok(Value::Bool(false));
    };
    let r = xmlstore::with(|st| {
        st.arena.is_element(c) && !child_elems(&st.arena, c, &s.filter, None).is_empty()
    });
    Ok(Value::Bool(r))
}

fn m_get_children(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, s) = this_spec(vm, a)?;
    let Some(c) = cursor_of(&o) else {
        return Ok(Value::Null);
    };
    let cls = vm.class_of(&o);
    make(vm, &cls, Spec::of(c, &s.filter))
}
