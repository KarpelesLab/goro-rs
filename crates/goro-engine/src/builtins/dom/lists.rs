//! DOM collections: DOMNodeList / Dom\NodeList, DOMNamedNodeMap /
//! Dom\NamedNodeMap / Dom\DtdNamedNodeMap, Dom\HTMLCollection and
//! Dom\TokenList. Live collections recompute from the tree, cached on the
//! arena's change counter; iteration is by index (PHP manual: DOMNodeList,
//! DOMNamedNodeMap; WHATWG DOM "collections").

use std::cell::RefCell;

use goro_runtime::{Key, NativeData, ObjRef, PhpArray, PhpStr, Value};
use goro_xml::NodeId;
use goro_xml::tree::NodeData;

use super::xmlstore::{self, NodeHold};
use super::{s8, sv, with_arena, wrap_node};
use crate::builtins::spl::ExtIterOp;
use crate::class::NativeHooks;
use crate::func::{Args, NativeFn};
use crate::vm::{R, Vm};

pub const METHODS: &[(&str, NativeFn)] = &[
    ("domnodelist::count", l_count),
    ("domnodelist::item", l_item),
    ("domnodelist::getiterator", l_iterator),
    ("dom\\nodelist::count", l_count),
    ("dom\\nodelist::item", l_item),
    ("dom\\nodelist::getiterator", l_iterator),
    ("domnamednodemap::count", l_count),
    ("domnamednodemap::item", l_item),
    ("domnamednodemap::getiterator", l_iterator),
    ("domnamednodemap::getnameditem", l_named_item),
    ("domnamednodemap::getnameditemns", l_named_item_ns),
    ("dom\\namednodemap::count", l_count),
    ("dom\\namednodemap::item", l_item),
    ("dom\\namednodemap::getiterator", l_iterator),
    ("dom\\namednodemap::getnameditem", l_named_item),
    ("dom\\namednodemap::getnameditemns", l_named_item_ns),
    ("dom\\dtdnamednodemap::count", l_count),
    ("dom\\dtdnamednodemap::item", l_item),
    ("dom\\dtdnamednodemap::getiterator", l_iterator),
    ("dom\\dtdnamednodemap::getnameditem", l_named_item),
    ("dom\\dtdnamednodemap::getnameditemns", l_named_item_ns),
    ("dom\\htmlcollection::count", l_count),
    ("dom\\htmlcollection::item", l_item),
    ("dom\\htmlcollection::getiterator", l_iterator),
    ("dom\\htmlcollection::nameditem", l_html_named_item),
];

/// What a collection holds.
#[derive(Clone, Debug)]
pub enum ListKind {
    /// `childNodes`.
    Children(NodeId),
    /// `attributes`.
    Attrs(NodeId),
    /// `getElementsByTagName()` below a root (not including it).
    TagName(NodeId, String),
    /// `getElementsByTagNameNS()`.
    TagNameNS(NodeId, Option<String>, String),
    /// `getElementsByClassName()`.
    ClassName(NodeId, Vec<String>),
    /// `children` (element children).
    ChildElements(NodeId),
    /// `DOMDocumentType::$entities`.
    Entities(NodeId),
    /// `DOMDocumentType::$notations`.
    Notations(NodeId),
    /// A fixed list (XPath results, `querySelectorAll()`).
    Static(Vec<NodeId>),
    /// `classList`.
    ClassList(NodeId),
}

/// The native data of a collection object.
pub struct ListObj {
    pub kind: ListKind,
    pub modern: bool,
    /// Holds on the root and on the items of fixed lists.
    _holds: Vec<NodeHold>,
    cache: RefCell<Option<(u64, Vec<NodeId>)>>,
    /// The node object the collection was taken from: a document object
    /// that loads another document takes its collections along (test
    /// DOMDocument_item_cache_invalidation.phpt).
    owner: Option<ObjRef>,
}

impl NativeData for ListObj {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        if let Some(o) = &self.owner {
            visit(&Value::Object(o.clone()));
        }
    }
    fn clear(&mut self) {
        self.owner = None;
    }
}

impl ListKind {
    fn root(&self) -> Option<NodeId> {
        match self {
            ListKind::Static(_) => None,
            ListKind::Children(n)
            | ListKind::Attrs(n)
            | ListKind::TagName(n, _)
            | ListKind::TagNameNS(n, _, _)
            | ListKind::ClassName(n, _)
            | ListKind::ChildElements(n)
            | ListKind::Entities(n)
            | ListKind::Notations(n)
            | ListKind::ClassList(n) => Some(*n),
        }
    }
    fn rerooted(&self, r: NodeId) -> ListKind {
        match self.clone() {
            ListKind::Children(_) => ListKind::Children(r),
            ListKind::Attrs(_) => ListKind::Attrs(r),
            ListKind::TagName(_, a) => ListKind::TagName(r, a),
            ListKind::TagNameNS(_, a, b) => ListKind::TagNameNS(r, a, b),
            ListKind::ClassName(_, a) => ListKind::ClassName(r, a),
            ListKind::ChildElements(_) => ListKind::ChildElements(r),
            ListKind::Entities(_) => ListKind::Entities(r),
            ListKind::Notations(_) => ListKind::Notations(r),
            ListKind::ClassList(_) => ListKind::ClassList(r),
            k => k,
        }
    }
}

fn class_for(kind: &ListKind, modern: bool) -> &'static str {
    match (kind, modern) {
        (ListKind::Children(_) | ListKind::Static(_), false) => "DOMNodeList",
        (ListKind::TagName(..) | ListKind::TagNameNS(..) | ListKind::ClassName(..), false) => {
            "DOMNodeList"
        }
        (ListKind::Attrs(_) | ListKind::Entities(_) | ListKind::Notations(_), false) => {
            "DOMNamedNodeMap"
        }
        (ListKind::Children(_) | ListKind::Static(_), true) => "Dom\\NodeList",
        (
            ListKind::TagName(..)
            | ListKind::TagNameNS(..)
            | ListKind::ClassName(..)
            | ListKind::ChildElements(_),
            _,
        ) => "Dom\\HTMLCollection",
        (ListKind::Attrs(_), true) => "Dom\\NamedNodeMap",
        (ListKind::Entities(_) | ListKind::Notations(_), true) => "Dom\\DtdNamedNodeMap",
        (ListKind::ClassList(_), _) => "Dom\\TokenList",
    }
}

/// A new collection object.
pub fn new_list(vm: &mut Vm, kind: ListKind, modern: bool) -> R<Value> {
    new_list_owned(vm, kind, modern, None)
}

/// A new collection object taken from node object `owner`.
pub fn new_list_owned(vm: &mut Vm, kind: ListKind, modern: bool, owner: Option<ObjRef>) -> R<Value> {
    let cls = vm
        .find_class(class_for(&kind, modern).as_bytes(), false)?
        .unwrap();
    let o = vm.instantiate_raw(&cls)?;
    let holds = xmlstore::with(|s| match &kind {
        ListKind::Static(v) => v.iter().map(|&n| s.hold(n)).collect(),
        ListKind::Children(n)
        | ListKind::Attrs(n)
        | ListKind::TagName(n, _)
        | ListKind::TagNameNS(n, _, _)
        | ListKind::ClassName(n, _)
        | ListKind::ChildElements(n)
        | ListKind::Entities(n)
        | ListKind::Notations(n)
        | ListKind::ClassList(n) => vec![s.hold(*n)],
    });
    *o.obj().native.borrow_mut() = Some(Box::new(ListObj {
        kind,
        modern,
        _holds: holds,
        cache: RefCell::new(None),
        owner,
    }));
    Ok(Value::Object(o))
}

/// The cached collection of kind `k` of node object `owner`.
pub fn cached_list(vm: &mut Vm, owner: &ObjRef, k: ListKind) -> R<Value> {
    let tag = match &k {
        ListKind::ChildElements(_) => 1u8,
        ListKind::ClassList(_) => 2,
        _ => 0,
    };
    let found = super::with_dom_obj(owner, |d| {
        d.cached
            .borrow()
            .iter()
            .find(|(t, _)| *t == tag)
            .map(|(_, o)| o.clone())
    })
    .flatten();
    if let Some(o) = found {
        return Ok(Value::Object(o));
    }
    let v = new_list(vm, k, true)?;
    if let Value::Object(o) = &v {
        super::with_dom_obj(owner, |d| d.cached.borrow_mut().push((tag, o.clone())));
    }
    Ok(v)
}

fn list_of(o: &ObjRef) -> Option<(ListKind, bool)> {
    let n = o.obj().native.borrow();
    let any: &dyn std::any::Any = &**n.as_ref()?;
    any.downcast_ref::<ListObj>()
        .map(|l| (l.kind.clone(), l.modern))
}

/// The current items of a collection (attribute nodes for maps, entity
/// or notation nodes for DTD maps).
pub fn items(o: &ObjRef) -> Vec<NodeId> {
    let n = o.obj().native.borrow();
    let Some(any) = n.as_ref() else {
        return Vec::new();
    };
    let any: &dyn std::any::Any = &**any;
    let Some(l) = any.downcast_ref::<ListObj>() else {
        return Vec::new();
    };
    if let ListKind::Static(v) = &l.kind {
        return v.clone();
    }
    let kind = match (l.owner.as_ref().and_then(super::node_of), l.kind.root()) {
        (Some(now), Some(was)) if now != was => l.kind.rerooted(now),
        _ => l.kind.clone(),
    };
    xmlstore::with(|s| {
        let g = s.arena.generation();
        if let Some((cg, v)) = &*l.cache.borrow()
            && *cg == g
        {
            return v.clone();
        }
        if let Some(r) = kind.root()
            && !s.arena.is_live(r)
        {
            return Vec::new();
        }
        let v = compute_items(s, &kind, l.modern);
        *l.cache.borrow_mut() = Some((s.arena.generation(), v.clone()));
        v
    })
}

fn compute_items(s: &mut xmlstore::Store, kind: &ListKind, modern: bool) -> Vec<NodeId> {
    let a = &mut s.arena;
    match kind {
        ListKind::Children(n) => {
            if !a.is_live(*n) {
                return Vec::new();
            }
            match a.data(*n) {
                NodeData::Element(_)
                | NodeData::Document(_)
                | NodeData::Fragment
                | NodeData::Attribute(_)
                | NodeData::EntityRef(_)
                | NodeData::EntityDecl(_)
                | NodeData::Doctype(_) => a.children(*n).collect(),
                _ => Vec::new(),
            }
        }
        ListKind::ChildElements(n) => a.children(*n).filter(|&c| a.is_element(c)).collect(),
        ListKind::Attrs(n) => {
            if modern {
                a.attrs(*n).collect()
            } else {
                a.attrs(*n).collect()
            }
        }
        ListKind::TagName(root, name) => {
            if modern {
                super::modern::elements_by_qname(a, *root, name)
            } else {
                // Legacy: the qualified or the local name (observed on PHP
                // 8.5).
                a.descendants(*root)
                    .filter(|&n| {
                        a.element(n).is_some_and(|e| {
                            name == "*" || e.name.is(name) || &*e.name.local == name.as_str()
                        })
                    })
                    .collect()
            }
        }
        ListKind::TagNameNS(root, ns, local) => {
            a.elements_by_tag_name_ns(*root, ns.as_deref(), local)
        }
        ListKind::ClassName(root, classes) => super::modern::elements_by_class(a, *root, classes),
        ListKind::Entities(dt) => super::chardata::dtd_nodes(s, *dt, true),
        ListKind::Notations(dt) => super::chardata::dtd_nodes(s, *dt, false),
        ListKind::Static(v) => v.clone(),
        ListKind::ClassList(_) => Vec::new(),
    }
}

fn this_list(vm: &mut Vm, a: &Args) -> R<(ListKind, bool)> {
    let _ = vm;
    // A list made with `new` is empty (test bug38438.phpt).
    Ok(list_of(a.this_obj()).unwrap_or((ListKind::Static(Vec::new()), false)))
}

fn l_count(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if list_of(a.this_obj()).is_none() {
        return Ok(Value::Int(0));
    }
    let _ = vm;
    Ok(Value::Int(items(a.this_obj()).len() as i64))
}

fn l_item(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let _ = this_list(vm, a)?;
    let i = a.int(0);
    if i < 0 {
        return Ok(Value::Null);
    }
    let v = items(a.this_obj());
    match v.get(i as usize) {
        Some(&n) => wrap_node(vm, n),
        None => Ok(Value::Null),
    }
}

fn l_iterator(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    Ok(Value::Object(crate::builtins::spl::new_ext_iter(
        vm, &o, iter_step,
    )?))
}

fn iter_step(vm: &mut Vm, o: &ObjRef, pos: usize, op: ExtIterOp) -> R<Value> {
    let v = items(o);
    let is_map = matches!(
        list_of(o).map(|l| l.0),
        Some(ListKind::Attrs(_) | ListKind::Entities(_) | ListKind::Notations(_))
    );
    match op {
        ExtIterOp::Valid => Ok(Value::Bool(pos < v.len())),
        ExtIterOp::Current => match v.get(pos) {
            Some(&n) => wrap_node(vm, n),
            None => Ok(Value::Null),
        },
        ExtIterOp::Key => {
            if is_map && let Some(&n) = v.get(pos) {
                let name = with_arena(|a| a.node_name(n));
                return Ok(sv(&name));
            }
            Ok(Value::Int(pos as i64))
        }
    }
}

fn l_named_item(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (kind, modern) = this_list(vm, a)?;
    let name = s8(&a.str(0));
    let found = match kind {
        ListKind::Attrs(el) => with_arena(|ar| {
            if modern && super::props::html_upper(ar, el) {
                let lower = name.to_ascii_lowercase();
                ar.attrs(el)
                    .find(|&at| ar.qname(at).is_some_and(|q| q.is(&lower)))
            } else {
                ar.attrs(el)
                    .find(|&at| ar.qname(at).is_some_and(|q| q.is(&name)))
            }
        }),
        _ => {
            let v = items(a.this_obj());
            with_arena(|ar| v.into_iter().find(|&n| ar.node_name(n) == name))
        }
    };
    super::wrap_opt(vm, found)
}

fn l_named_item_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (kind, _) = this_list(vm, a)?;
    let ns = a.opt_str(0).map(|s| s8(&s)).filter(|s| !s.is_empty());
    let local = s8(&a.str(1));
    let found = match kind {
        ListKind::Attrs(el) => with_arena(|ar| ar.get_attr_node_ns(el, ns.as_deref(), &local)),
        _ => {
            let v = items(a.this_obj());
            with_arena(|ar| v.into_iter().find(|&n| ar.node_name(n) == local))
        }
    };
    super::wrap_opt(vm, found)
}

fn l_html_named_item(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let _ = this_list(vm, a)?;
    let key = s8(&a.str(0));
    if key.is_empty() {
        return Ok(Value::Null);
    }
    let v = items(a.this_obj());
    let found = with_arena(|ar| {
        v.iter().copied().find(|&n| {
            ar.get_attribute(n, "id").as_deref() == Some(key.as_str())
                || (ar
                    .element(n)
                    .is_some_and(|e| e.name.ns.as_deref() == Some(goro_xml::tree::HTML_NS))
                    && ar.get_attribute(n, "name").as_deref() == Some(key.as_str()))
        })
    });
    super::wrap_opt(vm, found)
}

/// The `length` property (and TokenList's `value`).
pub fn list_prop(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (name, is_set) = super::props::hook_name(a);
    let o = a.this_obj().clone();
    if let Some((ListKind::ClassList(el), _)) = list_of(&o) {
        return super::modern::token_list_prop(vm, el, &name, is_set, a.val(0));
    }
    if is_set {
        let c = vm.class_of(&o).name.to_string_lossy();
        return vm.throw(
            "Error",
            &format!("Cannot modify readonly property {c}::${name}"),
        );
    }
    if list_of(&o).is_none() {
        return Ok(Value::Int(0));
    }
    Ok(Value::Int(items(&o).len() as i64))
}

fn list_debug(vm: &mut Vm, o: &ObjRef) -> Option<PhpArray> {
    let mut out = vm.plain_props_array(o, true);
    if let Some((ListKind::ClassList(el), _)) = list_of(o) {
        let len = super::modern::token_list_prop(vm, el, "length", false, Value::Null).ok()?;
        let val = super::modern::token_list_prop(vm, el, "value", false, Value::Null).ok()?;
        out.insert(Key::Str(PhpStr::from("length")), len);
        out.insert(Key::Str(PhpStr::from("value")), val);
        return Some(out);
    }
    let n = if list_of(o).is_some() {
        items(o).len()
    } else {
        0
    };
    out.insert(Key::Str(PhpStr::from("length")), Value::Int(n as i64));
    Some(out)
}

/// What a dimension key of a collection selects.
enum DimKey {
    Index(i64),
    Name(String),
    Nothing,
}

/// Dimension access on collections: `$list[0]`, `$attrs['name']`,
/// `$collection['id']` (tests bug67949.phpt, modern/spec/
/// NodeList_dimensions.phpt, NamedNodeMap_dimensions.phpt).
fn list_dim(
    vm: &mut Vm,
    o: &ObjRef,
    key: Option<&Value>,
    op: crate::class::ObjAccess,
) -> Option<R<Value>> {
    use crate::class::ObjAccess as DimOp;
    let (kind, modern) = list_of(o)?;
    if matches!(kind, ListKind::ClassList(_)) {
        return None;
    }
    let cname = vm.class_of(o).name.to_string_lossy();
    let Some(key) = key else {
        let m = if modern {
            format!("Cannot append to {cname}")
        } else {
            format!("Cannot access {cname} without offset")
        };
        return Some(vm.throw("Error", &m));
    };
    if matches!(op, DimOp::Write(_) | DimOp::Unset) {
        return None;
    }
    let is_map = matches!(
        kind,
        ListKind::Attrs(_) | ListKind::Entities(_) | ListKind::Notations(_)
    );
    let is_collection = matches!(
        kind,
        ListKind::TagName(..) | ListKind::TagNameNS(..) | ListKind::ClassName(..) | ListKind::ChildElements(_)
    ) && modern;
    let named_ok = is_map || is_collection;
    let k = match key.deref() {
        Value::Int(i) => DimKey::Index(i),
        Value::Float(f) => {
            if f.fract() != 0.0 && f.is_finite() {
                if let Err(e) = vm.deprecated(&format!(
                    "Implicit conversion from float {} to int loses precision",
                    goro_runtime::convert::float_repr(f)
                )) {
                    return Some(Err(e));
                }
            }
            DimKey::Index(f as i64)
        }
        Value::Str(s) => match goro_runtime::array::canonical_int(s.as_bytes()) {
            Some(i) => DimKey::Index(i),
            None => {
                if named_ok {
                    DimKey::Name(s8(&s))
                } else if modern {
                    return Some(vm.throw(
                        "TypeError",
                        &format!("Cannot access offset of type string on {cname}"),
                    ));
                } else {
                    DimKey::Nothing
                }
            }
        },
        Value::Bool(_) | Value::Null if modern => {
            let t = crate::class::TypeName::tname(&key.deref());
            let m = if matches!(op, DimOp::Isset | DimOp::Empty) {
                format!("Cannot access offset of type {t} in isset or empty")
            } else {
                format!("Cannot access offset of type {t} on {cname}")
            };
            return Some(vm.throw("TypeError", &m));
        }
        Value::Bool(b) => DimKey::Index(b as i64),
        Value::Null => DimKey::Index(0),
        _ => DimKey::Nothing,
    };
    let found = match k {
        DimKey::Index(i) if i < 0 => {
            if is_map && !modern {
                return Some(vm.throw("ValueError", "must be between 0 and 2147483647"));
            }
            None
        }
        DimKey::Index(i) => items(o).get(i as usize).copied(),
        DimKey::Name(name) => {
            let v = items(o);
            with_arena(|ar| {
                if is_collection {
                    v.into_iter().find(|&n| {
                        ar.get_attribute(n, "id").as_deref() == Some(name.as_str())
                            || (ar.element(n).is_some_and(|e| {
                                e.name.ns.as_deref() == Some(goro_xml::tree::HTML_NS)
                            }) && ar.get_attribute(n, "name").as_deref() == Some(name.as_str()))
                    })
                } else {
                    v.into_iter().find(|&n| ar.node_name(n) == name)
                }
            })
        }
        DimKey::Nothing => None,
    };
    Some(match op {
        DimOp::Isset => Ok(Value::Bool(found.is_some())),
        DimOp::Empty => Ok(Value::Bool(found.is_none())),
        _ => match found {
            Some(n) => wrap_node(vm, n),
            None => Ok(Value::Null),
        },
    })
}

pub fn list_hooks() -> NativeHooks {
    NativeHooks {
        dim: Some(list_dim),
        count: Some(|vm, o| {
            if let Some((ListKind::ClassList(el), _)) = list_of(o) {
                return match super::modern::token_list_prop(vm, el, "length", false, Value::Null)?
                {
                    Value::Int(n) => Ok(n),
                    _ => Ok(0),
                };
            }
            Ok(if list_of(o).is_some() {
                items(o).len() as i64
            } else {
                0
            })
        }),
        debug_props: Some(list_debug),
        props_for: Some(|vm, o| Some(vm.plain_props_array(o, true))),
        get_iterator: Some(|vm, o, by_ref| {
            if by_ref {
                return vm.throw(
                    "Error",
                    "An iterator cannot be used with foreach by reference",
                );
            }
            if let Some((ListKind::ClassList(_), _)) = list_of(o) {
                return crate::builtins::spl::new_ext_iter(vm, o, super::modern::token_iter_step);
            }
            crate::builtins::spl::new_ext_iter(vm, o, iter_step)
        }),
        ..Default::default()
    }
}
