//! DOMNodeList / DOMNamedNodeMap (nodelist.c, namednodemap.c, obj_map.c,
//! dom_iterators.c)

use super::node::{args, next_in_tree};
use super::*;

pub struct IterState {
    /// the list/map object being iterated
    pub map: Obj,
    pub index: i64,
    pub cur: Value,
    /// The zend_object_iterator the InternalIterator wraps (it takes an
    /// object handle of its own; None when the InternalIterator itself
    /// stands for it, under foreach)
    pub _handle: Option<Obj>,
}

/// zend_iterator_init(): an internal class's iterator is an object too, and
/// takes the next object handle while it lives.
pub fn iterator_handle(vm: &mut Vm) -> Obj {
    let id = vm.alloc_object_id();
    Rc::new(RefCell::new(goro_core::object::PhpObject::new(
        b"\0iterator".to_vec(),
        id,
    )))
}

/// A list/map with its base objects resolved to their current nodes.
enum MapRef {
    List(LK),
    Map(MK),
}

enum LK {
    Children(NodeId),
    ByTag {
        base: NodeId,
        ns: Option<String>,
        local: String,
    },
    Static(Vec<Value>),
    Empty,
    /// modern collections (Dom\HTMLCollection)
    ChildElements(NodeId),
    ByQName {
        base: NodeId,
        qname: String,
    },
    ByClass {
        base: NodeId,
        classes: Vec<String>,
        quirks: bool,
    },
}

#[derive(Clone)]
enum MK {
    Attributes(NodeId),
    Entities(NodeId),
    Notations(NodeId),
    Empty,
}

fn map_ref(o: &Obj) -> Option<MapRef> {
    let d = data_of(o)?;
    let k = d.kind.borrow();
    match &*k {
        Kind::List(l) => Some(MapRef::List(match l {
            ListKind::Children(b) => match node_of(b) {
                Some(n) => LK::Children(n),
                None => LK::Empty,
            },
            ListKind::ByTag { base, ns, local } => match node_of(base) {
                Some(n) => LK::ByTag {
                    base: n,
                    ns: ns.clone(),
                    local: local.clone(),
                },
                None => LK::Empty,
            },
            ListKind::Static(v) => LK::Static(v.clone()),
            ListKind::Empty => LK::Empty,
            ListKind::ChildElements(b) => node_of(b).map(LK::ChildElements).unwrap_or(LK::Empty),
            ListKind::ByQName { base, qname } => match node_of(base) {
                Some(n) => LK::ByQName {
                    base: n,
                    qname: qname.clone(),
                },
                None => LK::Empty,
            },
            ListKind::ByClass {
                base,
                classes,
                quirks,
            } => match node_of(base) {
                Some(n) => LK::ByClass {
                    base: n,
                    classes: classes.clone(),
                    quirks: *quirks,
                },
                None => LK::Empty,
            },
        })),
        Kind::Map(m) => Some(MapRef::Map(match m {
            MapKind::Attributes(b) => node_of(b).map(MK::Attributes).unwrap_or(MK::Empty),
            MapKind::Entities(b) => node_of(b).map(MK::Entities).unwrap_or(MK::Empty),
            MapKind::Notations(b) => node_of(b).map(MK::Notations).unwrap_or(MK::Empty),
        })),
        _ => None,
    }
}

fn first_child_of_container(t: &Tree, n: NodeId) -> Option<NodeId> {
    if t.is_doc(n) {
        t.root_element(n)
    } else {
        t.nodes[n].children
    }
}

fn tag_matches(t: &Tree, x: NodeId, ns: Option<&str>, local: &str) -> bool {
    if t.nodes[x].t != NType::Element {
        return false;
    }
    if local != "*" && t.nodes[x].name != local {
        return false;
    }
    match ns {
        None | Some("*") => true,
        Some("") => t.nodes[x].ns.is_none(),
        Some(u) => t.nodes[x].ns.map(|n| t.nss[n].href == u).unwrap_or(false),
    }
}

/// Elements matching a ByTag list, in tree order (up to `limit` items).
fn by_tag(t: &Tree, base: NodeId, ns: Option<&str>, local: &str, limit: usize) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut cur = first_child_of_container(t, base);
    while let Some(x) = cur {
        if tag_matches(t, x, ns, local) {
            out.push(x);
            if out.len() >= limit {
                break;
            }
        }
        cur = next_in_tree(t, x, base);
    }
    out
}

fn children_start(n: NodeId) -> Option<NodeId> {
    let is_ref = tree::with(|t| t.nodes[n].t == NType::EntityRef);
    if is_ref {
        super::chardata::sync_entity_ref(n);
    }
    tree::with(|t| t.nodes[n].children)
}

pub fn list_length(o: &Obj) -> i64 {
    match map_ref(o) {
        Some(MapRef::List(l)) => match l {
            LK::Children(n) => {
                let first = children_start(n);
                tree::with(|t| {
                    let mut c = first;
                    let mut cnt = 0;
                    while let Some(x) = c {
                        cnt += 1;
                        c = t.nodes[x].next;
                    }
                    cnt
                })
            }
            LK::ByTag { base, ns, local } => {
                tree::with(|t| by_tag(t, base, ns.as_deref(), &local, usize::MAX).len() as i64)
            }
            LK::Static(v) => v.len() as i64,
            LK::Empty => 0,
            l => tree::with(|t| modern_nodes(t, &l, usize::MAX).len() as i64),
        },
        Some(MapRef::Map(m)) => tree::with(|t| match m {
            MK::Attributes(n) => {
                if t.nodes[n].t == NType::Element {
                    t.attrs_of(n).len() as i64
                } else {
                    0
                }
            }
            MK::Entities(d) => t.dtd_info(d).map(|i| i.entities.len() as i64).unwrap_or(0),
            MK::Notations(d) => t.dtd_info(d).map(|i| i.notations.len() as i64).unwrap_or(0),
            MK::Empty => 0,
        }),
        None => 0,
    }
}

fn notation_node(
    t: &mut Tree,
    name: &str,
    public: Option<String>,
    system: Option<String>,
) -> NodeId {
    let n = t.new_node(None, NType::Notation, name);
    t.nodes[n].extra = tree::Extra::Notation(public, system);
    n
}

/// get_item: the item at `index` (Null when out of range).
pub fn get_item(vm: &mut Vm, o: &Obj, index: i64) -> Value {
    if index < 0 {
        return Value::Null;
    }
    let idx = index as usize;
    let node = match map_ref(o) {
        Some(MapRef::List(l)) => match l {
            LK::Children(n) => {
                let first = children_start(n);
                tree::with(|t| {
                    let mut c = first;
                    let mut i = 0;
                    while let Some(x) = c {
                        if i == idx {
                            return Some(x);
                        }
                        i += 1;
                        c = t.nodes[x].next;
                    }
                    None
                })
            }
            LK::ByTag { base, ns, local } => tree::with(|t| {
                by_tag(t, base, ns.as_deref(), &local, idx + 1)
                    .get(idx)
                    .copied()
            }),
            LK::Static(v) => return v.get(idx).cloned().unwrap_or(Value::Null),
            LK::Empty => None,
            l => tree::with(|t| modern_nodes(t, &l, idx + 1).get(idx).copied()),
        },
        Some(MapRef::Map(m)) => tree::with(|t| match m {
            MK::Attributes(n) => {
                if t.nodes[n].t == NType::Element {
                    t.attrs_of(n).get(idx).copied()
                } else {
                    None
                }
            }
            MK::Entities(d) => t.dtd_info(d).and_then(|i| i.entities.get(idx).map(|e| e.1)),
            MK::Notations(d) => {
                let nt = t.dtd_info(d).and_then(|i| i.notations.get(idx).cloned());
                nt.map(|(name, p, s)| notation_node(t, &name, p, s))
            }
            MK::Empty => None,
        }),
        None => None,
    };
    mark_modern_notation(vm, o, node);
    node_obj_opt(vm, node)
}

/// Notation nodes are created on the fly without a document: give them the
/// class flavour of the map they come from.
fn mark_modern_notation(vm: &Vm, o: &Obj, node: Option<NodeId>) {
    if let Some(n) = node {
        if base_of(vm, o).map(|b| b.is_modern()).unwrap_or(false) {
            tree::with(|t| {
                if t.nodes[n].t == NType::Notation {
                    t.nodes[n].flags |= tree::F_MODERN;
                }
            });
        }
    }
}

/// Items of a modern collection (up to `limit`).
fn modern_nodes(t: &Tree, l: &LK, limit: usize) -> Vec<NodeId> {
    use super::modern::collections as mc;
    match l {
        LK::ChildElements(n) => {
            let mut v = mc::child_elements(t, *n);
            v.truncate(limit);
            v
        }
        LK::ByQName { base, qname } => {
            mc::filtered(t, *base, limit, &|t, x| mc::qname_matches(t, x, qname))
        }
        LK::ByClass {
            base,
            classes,
            quirks,
        } => {
            if classes.is_empty() {
                return Vec::new();
            }
            mc::filtered(t, *base, limit, &|t, x| {
                mc::class_matches(t, x, classes, *quirks)
            })
        }
        _ => Vec::new(),
    }
}

/// The nodes of a list (up to `limit`), for named lookups.
pub fn list_nodes(o: &Obj, limit: usize) -> Vec<NodeId> {
    match map_ref(o) {
        Some(MapRef::List(LK::ByTag { base, ns, local })) => {
            tree::with(|t| by_tag(t, base, ns.as_deref(), &local, limit))
        }
        Some(MapRef::List(LK::Children(n))) => tree::with(|t| {
            let mut v = t.children_of(n);
            v.truncate(limit);
            v
        }),
        Some(MapRef::List(l)) => tree::with(|t| modern_nodes(t, &l, limit)),
        _ => Vec::new(),
    }
}

/// getNamedItem(NS) of a map object (modern maps: notations get the
/// modern class flavour).
pub fn map_named(o: &Obj, name: &str, ns: Option<&str>) -> Option<NodeId> {
    let r = match map_ref(o) {
        Some(MapRef::Map(m)) => tree::with(|t| named_item(t, &m, name, ns)),
        _ => None,
    };
    if let Some(n) = r {
        tree::with(|t| {
            if t.nodes[n].t == NType::Notation {
                t.nodes[n].flags |= tree::F_MODERN;
            }
        });
    }
    r
}

/// getNamedItem(NS) for maps.
fn named_item(t: &mut Tree, m: &MK, name: &str, ns: Option<&str>) -> Option<NodeId> {
    match m {
        MK::Attributes(n) => {
            if t.nodes[*n].t != NType::Element {
                return None;
            }
            match ns {
                Some(u) => t.has_ns_prop(*n, name, if u.is_empty() { None } else { Some(u) }),
                None => t.has_prop(*n, name),
            }
        }
        MK::Entities(d) => t
            .dtd_info(*d)
            .and_then(|i| i.entities.iter().find(|e| e.0 == name).map(|e| e.1)),
        MK::Notations(d) => {
            let nt = t
                .dtd_info(*d)
                .and_then(|i| i.notations.iter().find(|e| e.0 == name).cloned());
            nt.map(|(name, p, s)| notation_node(t, &name, p, s))
        }
        MK::Empty => None,
    }
}

/// dom_nodemap_or_nodelist_process_offset_as_named: Err(()) means "named".
fn offset_as_long(v: &Value) -> Result<i64, ()> {
    use goro_core::value::{NumStr, parse_numeric_str};
    match v {
        Value::String(s) => match parse_numeric_str(s.as_bytes(), false) {
            Some((NumStr::Long(n), _)) => Ok(n),
            Some((NumStr::Double(d), _)) => Ok(if d.is_finite() {
                d as i64
            } else if d > 0.0 {
                i64::MAX
            } else {
                i64::MIN
            }),
            None => Err(()),
        },
        other => Ok(other.to_long()),
    }
}

pub fn h_read_dimension(vm: &mut Vm, o: &Obj, offset: Option<&Value>, _mode: u8) -> Option<Value> {
    let mr = map_ref(o)?;
    let cn = goro_core::value::display_class_name(&o.borrow().class_name).to_string();
    let off = match offset {
        Some(v) => v.deref(),
        None => {
            throw_error(
                vm,
                b"Error",
                &format!("Cannot access {} without offset", cn),
            );
            return Some(Value::Null);
        }
    };
    match mr {
        MapRef::List(_) => match offset_as_long(&off) {
            Err(()) => Some(Value::Null),
            Ok(i) => Some(get_item(vm, o, i)),
        },
        MapRef::Map(m) => match offset_as_long(&off) {
            Err(()) => {
                let name = off.to_php_string().to_string_lossy();
                let n = tree::with(|t| named_item(t, &m, &name, None));
                Some(node_obj_opt(vm, n))
            }
            Ok(i) => {
                if i < 0 || i > i32::MAX as i64 {
                    throw_error(
                        vm,
                        b"ValueError",
                        &format!("must be between 0 and {}", i32::MAX),
                    );
                    return Some(Value::Null);
                }
                Some(get_item(vm, o, i))
            }
        },
    }
}

pub fn h_has_dimension(_vm: &mut Vm, o: &Obj, offset: &Value, _check_empty: u8) -> Option<bool> {
    let mr = map_ref(o)?;
    let off = offset.deref();
    match offset_as_long(&off) {
        Err(()) => match mr {
            MapRef::List(_) => Some(false),
            MapRef::Map(m) => {
                let name = off.to_php_string().to_string_lossy();
                Some(tree::with(|t| named_item(t, &m, &name, None)).is_some())
            }
        },
        Ok(i) => Some(i >= 0 && i < list_length(o)),
    }
}

pub fn h_count(_vm: &mut Vm, o: &Obj) -> Option<i64> {
    map_ref(o)?;
    Some(list_length(o))
}

// ---------------------------------------------------------------------------
// Properties

fn r_length(_vm: &mut Vm, o: &Obj, _n: NodeId) -> R<Value> {
    Ok(Value::Long(list_length(o)))
}

pub fn list_props() -> Vec<Prop> {
    vec![p("length", r_length, None, "int")]
}

pub fn map_props() -> Vec<Prop> {
    vec![p("length", r_length, None, "int")]
}

// ---------------------------------------------------------------------------
// Methods

fn m_count(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    Ok(Value::Long(list_length(o)))
}

fn is_map(o: &Obj) -> bool {
    matches!(map_ref(o), Some(MapRef::Map(_)))
}

fn m_item(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let i = a.long(vm, 0, "index", 0)?;
    if is_map(o) && (i < 0 || i > i32::MAX as i64) {
        throw_error(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #1 ($index) must be between 0 and {}",
                a.f,
                i32::MAX
            ),
        );
        return Err(());
    }
    Ok(get_item(vm, o, i))
}

fn m_get_named_item(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let name = a.str(vm, 0, "qualifiedName")?;
    let n = match map_ref(o) {
        Some(MapRef::Map(m)) => tree::with(|t| named_item(t, &m, &name, None)),
        _ => None,
    };
    Ok(node_obj_opt(vm, n))
}

fn m_get_named_item_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let ns = a.opt_str(vm, 0, "namespace")?;
    let name = a.str(vm, 1, "localName")?;
    let n = match map_ref(o) {
        Some(MapRef::Map(m)) => {
            tree::with(|t| named_item(t, &m, &name, Some(ns.as_deref().unwrap_or(""))))
        }
        _ => None,
    };
    Ok(node_obj_opt(vm, n))
}

fn m_get_iterator(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    // foreach uses php_dom_get_iterator() directly: the iterator takes one
    // handle, before the first item is fetched.
    let (handle, it, first) = if vm.foreach_native_iter == 1 {
        vm.foreach_native_iter = 2;
        let it = new_object(vm, b"InternalIterator");
        let first = get_item(vm, o, 0);
        (None, it, first)
    } else {
        let handle = iterator_handle(vm);
        let first = get_item(vm, o, 0);
        (Some(handle), new_object(vm, b"InternalIterator"), first)
    };
    attach(
        &it,
        Kind::Iter(RefCell::new(IterState {
            map: o.clone(),
            index: 0,
            cur: first,
            _handle: handle,
        })),
    );
    Ok(Value::Object(it))
}

pub fn list_methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("count", m_count),
        ("item", m_item),
        ("getIterator", m_get_iterator),
        ("getNamedItem", m_get_named_item),
        ("getNamedItemNS", m_get_named_item_ns),
    ]
}

fn nameless(o: &Obj) -> bool {
    !is_map(o)
}

fn uses_next_pointer(o: &Obj) -> bool {
    matches!(
        map_ref(o),
        Some(MapRef::List(LK::Children(_))) | Some(MapRef::Map(MK::Attributes(_)))
    )
}

/// InternalIterator methods for DOM lists.
pub fn iter_method(vm: &mut Vm, it: &Obj, method: &[u8]) -> Option<Value> {
    let d = data_of(it)?;
    let m = String::from_utf8_lossy(method).to_ascii_lowercase();
    let (map, index, cur) = match &*d.kind.borrow() {
        Kind::Iter(st) => {
            let st = st.borrow();
            (st.map.clone(), st.index, st.cur.clone())
        }
        _ => return None,
    };
    let modern = base_of(vm, &map).map(|b| b.is_modern()).unwrap_or(false);
    let set = |index: i64, cur: Value| {
        if let Kind::Iter(st) = &*d.kind.borrow() {
            let mut st = st.borrow_mut();
            st.index = index;
            st.cur = cur;
        }
    };
    match m.as_str() {
        "current" => Some(cur),
        "valid" => Some(if matches!(cur, Value::Null) {
            Value::False
        } else {
            Value::True
        }),
        "key" => {
            if matches!(cur, Value::Null) {
                return Some(Value::Null);
            }
            if nameless(&map) {
                Some(Value::Long(index))
            } else {
                match &cur {
                    Value::Object(co) => match node_of(co) {
                        Some(n) => Some(Value::String(PhpString::from_string(tree::with(|t| {
                            if modern && t.nodes[n].t == NType::Attribute {
                                t.qname(n)
                            } else {
                                t.nodes[n].name.clone()
                            }
                        })))),
                        None => Some(Value::Null),
                    },
                    _ => Some(Value::Null),
                }
            }
        }
        "next" => {
            if matches!(cur, Value::Null) {
                return Some(Value::Null);
            }
            let ni = index + 1;
            let next = if !modern && uses_next_pointer(&map) {
                let cn = match &cur {
                    Value::Object(co) => node_of(co),
                    _ => None,
                };
                let nx = cn.and_then(|n| tree::with(|t| t.nodes[n].next));
                node_obj_opt(vm, nx)
            } else {
                get_item(vm, &map, ni)
            };
            set(ni, next);
            Some(Value::Null)
        }
        "rewind" => {
            // DOM iterators cannot rewind: InternalIterator::rewind() is
            // only allowed before the iteration moved.
            if index != 0 {
                throw_error(vm, b"Error", "Iterator does not support rewinding");
            }
            Some(Value::Null)
        }
        _ => None,
    }
}
