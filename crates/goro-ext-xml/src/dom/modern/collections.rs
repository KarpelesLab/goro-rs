//! Dom\NodeList, Dom\NamedNodeMap, Dom\DtdNamedNodeMap, Dom\HTMLCollection
//! (nodelist.c, namednodemap.c, html_collection.c, obj_map.c; modern
//! dimension handlers).

use super::*;
use crate::dom::lists;

fn r_length(_vm: &mut Vm, o: &Obj, _n: NodeId) -> R<Value> {
    Ok(Value::Long(lists::list_length(o)))
}

pub fn list_props() -> Vec<Prop> {
    vec![p("length", r_length, None, "int")]
}

/// Does element `x` match a modern getElementsByTagName() qualified name?
pub fn qname_matches(t: &Tree, x: NodeId, qname: &str) -> bool {
    if t.nodes[x].t != NType::Element {
        return false;
    }
    if qname == "*" {
        return true;
    }
    let html = t.nodes[x]
        .doc
        .map(|d| t.nodes[d].t == NType::HtmlDocument)
        .unwrap_or(false)
        && ns_is(t, t.nodes[x].ns, XHTML_NS);
    if html {
        element::match_qname(t, &qname.to_ascii_lowercase(), x)
    } else {
        element::match_qname(t, qname, x)
    }
}

/// dom_matches_class_name
pub fn class_matches(t: &Tree, x: NodeId, classes: &[String], quirks: bool) -> bool {
    if t.nodes[x].t != NType::Element {
        return false;
    }
    match t.has_ns_prop(x, "class", None) {
        Some(a) => {
            let v = t.node_get_content(a);
            tokenlist::ordered_set_all_contained(classes, &v, quirks)
        }
        None => false,
    }
}

fn first_child_of_container(t: &Tree, n: NodeId) -> Option<NodeId> {
    t.nodes[n].children
}

/// Elements of a tree-order filtered collection rooted at `base`.
pub fn filtered(
    t: &Tree,
    base: NodeId,
    limit: usize,
    pred: &dyn Fn(&Tree, NodeId) -> bool,
) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut cur = first_child_of_container(t, base);
    while let Some(x) = cur {
        if pred(t, x) {
            out.push(x);
            if out.len() >= limit {
                break;
            }
        }
        cur = next_in_tree_order(t, x, Some(base));
    }
    out
}

pub fn child_elements(t: &Tree, base: NodeId) -> Vec<NodeId> {
    t.children_of(base)
        .into_iter()
        .filter(|x| t.nodes[*x].t == NType::Element)
        .collect()
}

// ---------------------------------------------------------------------------
// Dimension handlers

enum Index {
    Long(i64),
    Str(String),
    Illegal,
}

/// zend_dval_to_lval_safe
pub fn dval_to_lval_safe(vm: &mut Vm, d: f64) -> i64 {
    if !d.is_finite() || d < i64::MIN as f64 || d >= i64::MAX as f64 {
        return 0;
    }
    if d.fract() != 0.0 {
        vm.emit_deprecated(&format!(
            "Implicit conversion from float {} to int loses precision",
            goro_core::value::format_php_float(d)
        ));
    }
    d as i64
}

/// dom_modern_nodelist_get_index
fn get_index(vm: &mut Vm, v: &Value) -> Index {
    match v {
        Value::Long(n) => Index::Long(*n),
        Value::Double(d) => Index::Long(dval_to_lval_safe(vm, *d)),
        Value::String(s_) => {
            let b = s_.as_bytes();
            if let Some(n) = handle_numeric(b) {
                Index::Long(n)
            } else {
                Index::Str(s_.to_string_lossy())
            }
        }
        Value::Reference(r) => {
            let inner = r.borrow().clone();
            get_index(vm, &inner)
        }
        _ => Index::Illegal,
    }
}

/// ZEND_HANDLE_NUMERIC_STR: canonical decimal integer strings.
fn handle_numeric(b: &[u8]) -> Option<i64> {
    if b.is_empty() || b.len() > 20 {
        return None;
    }
    let (neg, digits) = if b[0] == b'-' {
        (true, &b[1..])
    } else {
        (false, b)
    };
    if digits.is_empty() || !digits.iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if digits.len() > 1 && digits[0] == b'0' {
        return None;
    }
    if neg && digits == b"0" {
        return None;
    }
    std::str::from_utf8(b).ok()?.parse::<i64>().ok()
}

fn offset_type_name(v: &Value) -> String {
    match v {
        Value::Null | Value::Undef => "null".into(),
        Value::True | Value::False => "bool".into(),
        Value::Array(_) => "array".into(),
        Value::Reference(r) => offset_type_name(&r.borrow()),
        other => type_name_of(other),
    }
}

fn illegal_offset(vm: &mut Vm, o: &Obj, v: &Value, isset: bool) {
    let cn = goro_core::value::display_class_name(&o.borrow().class_name).to_string();
    let msg = if isset {
        format!(
            "Cannot access offset of type {} in isset or empty",
            offset_type_name(v)
        )
    } else {
        format!(
            "Cannot access offset of type {} on {}",
            offset_type_name(v),
            cn
        )
    };
    throw_error(vm, b"TypeError", &msg);
}

fn cannot_append(vm: &mut Vm, o: &Obj) {
    let cn = goro_core::value::display_class_name(&o.borrow().class_name).to_string();
    throw_error(vm, b"Error", &format!("Cannot append to {}", cn));
}

fn is_collection(vm: &Vm, o: &Obj) -> bool {
    base_of(vm, o) == Some(Base::MHtmlCollection)
}

/// HTMLCollection::namedItem
pub fn named_item(o: &Obj, key: &str) -> Option<NodeId> {
    if key.is_empty() {
        return None;
    }
    let len = lists::list_length(o);
    let items = lists::list_nodes(o, len.max(0) as usize);
    tree::with(|t| {
        for x in items {
            if t.nodes[x].t != NType::Element {
                continue;
            }
            if let Some(a) = t.has_ns_prop(x, "id", None) {
                if t.node_get_content(a) == key {
                    return Some(x);
                }
            }
            if ns_is(t, t.nodes[x].ns, XHTML_NS) {
                if let Some(a) = t.has_ns_prop(x, "name", None) {
                    if t.node_get_content(a) == key {
                        return Some(x);
                    }
                }
            }
        }
        None
    })
}

fn list_read_dimension(vm: &mut Vm, o: &Obj, offset: Option<&Value>, _mode: u8) -> Option<Value> {
    let off = match offset {
        Some(v) => v.deref(),
        None => {
            cannot_append(vm, o);
            return Some(Value::Null);
        }
    };
    let collection = is_collection(vm, o);
    match get_index(vm, &off) {
        Index::Long(i) => Some(lists::get_item(vm, o, i)),
        Index::Str(s_) if collection => {
            let n = named_item(o, &s_);
            Some(node_obj_opt(vm, n))
        }
        _ => {
            illegal_offset(vm, o, &off, false);
            Some(Value::Null)
        }
    }
}

fn list_has_dimension(vm: &mut Vm, o: &Obj, offset: &Value, _check_empty: u8) -> Option<bool> {
    let off = offset.deref();
    let collection = is_collection(vm, o);
    match get_index(vm, &off) {
        Index::Long(i) => Some(i >= 0 && i < lists::list_length(o)),
        Index::Str(s_) if collection => Some(named_item(o, &s_).is_some()),
        _ => {
            illegal_offset(vm, o, &off, true);
            Some(false)
        }
    }
}

fn count(_vm: &mut Vm, o: &Obj) -> Option<i64> {
    Some(lists::list_length(o))
}

fn map_read_dimension(vm: &mut Vm, o: &Obj, offset: Option<&Value>, _mode: u8) -> Option<Value> {
    let off = match offset {
        Some(v) => v.deref(),
        None => {
            cannot_append(vm, o);
            return Some(Value::Null);
        }
    };
    match &off {
        Value::String(s_) => match handle_numeric(s_.as_bytes()) {
            Some(i) => Some(lists::get_item(vm, o, i)),
            None => {
                let n = map_named_item(o, &s_.to_string_lossy(), None);
                Some(node_obj_opt(vm, n))
            }
        },
        Value::Long(i) => Some(lists::get_item(vm, o, *i)),
        Value::Double(_) => match get_index(vm, &off) {
            Index::Long(i) => Some(lists::get_item(vm, o, i)),
            _ => Some(Value::Null),
        },
        _ => {
            illegal_offset(vm, o, &off, false);
            Some(Value::Null)
        }
    }
}

fn map_has_dimension(vm: &mut Vm, o: &Obj, offset: &Value, _check_empty: u8) -> Option<bool> {
    let off = offset.deref();
    match &off {
        Value::String(s_) => match handle_numeric(s_.as_bytes()) {
            Some(i) => Some(i >= 0 && i < lists::list_length(o)),
            None => Some(map_named_item(o, &s_.to_string_lossy(), None).is_some()),
        },
        Value::Long(i) => Some(*i >= 0 && *i < lists::list_length(o)),
        Value::Double(_) => match get_index(vm, &off) {
            Index::Long(i) => Some(i >= 0 && i < lists::list_length(o)),
            _ => Some(false),
        },
        _ => {
            illegal_offset(vm, o, &off, true);
            Some(false)
        }
    }
}

/// get_ns_named_item for modern maps: attributes use qualified name
/// matching when no namespace is given.
pub fn map_named_item(o: &Obj, name: &str, ns: Option<&str>) -> Option<NodeId> {
    let d = data_of(o)?;
    let k = d.kind.borrow();
    match &*k {
        Kind::Map(MapKind::Attributes(b)) => {
            let n = node_of(b)?;
            tree::with(|t| {
                if t.nodes[n].t != NType::Element {
                    return None;
                }
                match ns {
                    Some(u) => t.has_ns_prop(n, name, Some(u)),
                    None => element::get_attribute_node(t, n, name),
                }
            })
        }
        _ => {
            drop(k);
            lists::map_named(o, name, ns)
        }
    }
}

pub static LIST_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    read_property: Some(h_read_property_pub),
    write_property: Some(h_write_property_pub),
    has_property: Some(h_has_property_pub),
    unset_property: Some(h_unset_property_pub),
    read_dimension: Some(list_read_dimension),
    write_dimension: None,
    has_dimension: Some(list_has_dimension),
    unset_dimension: None,
    get_properties: Some(h_get_properties_pub),
    count_elements: Some(count),
    cast: None,
    compare: None,
    clone_obj: Some(h_uncloneable),
};

pub static MAP_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    read_property: Some(h_read_property_pub),
    write_property: Some(h_write_property_pub),
    has_property: Some(h_has_property_pub),
    unset_property: Some(h_unset_property_pub),
    read_dimension: Some(map_read_dimension),
    write_dimension: None,
    has_dimension: Some(map_has_dimension),
    unset_dimension: None,
    get_properties: Some(h_get_properties_pub),
    count_elements: Some(count),
    cast: None,
    compare: None,
    clone_obj: Some(h_uncloneable),
};

// ---------------------------------------------------------------------------
// Methods

fn m_get_named_item(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let name = a.str(vm, 0, "qualifiedName")?;
    let n = map_named_item(o, &name, None);
    Ok(node_obj_opt(vm, n))
}

fn m_get_named_item_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let ns = a.opt_str(vm, 0, "namespace")?;
    let name = a.str(vm, 1, "localName")?;
    let n = map_named_item(o, &name, ns.as_deref());
    Ok(node_obj_opt(vm, n))
}

fn m_named_item(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let key = a.str(vm, 0, "key")?;
    let n = named_item(o, &key);
    Ok(node_obj_opt(vm, n))
}

pub fn methods(b: Base) -> Vec<(&'static str, MethodFn)> {
    let lm = |n: &str| legacy_method(Base::NodeList, n);
    let mm = |n: &str| legacy_method(Base::NamedNodeMap, n);
    match b {
        Base::MNodeList => vec![
            ("count", lm("count")),
            ("getIterator", lm("getIterator")),
            ("item", lm("item")),
        ],
        Base::MHtmlCollection => vec![
            ("item", lm("item")),
            ("namedItem", m_named_item as MethodFn),
            ("count", lm("count")),
            ("getIterator", lm("getIterator")),
        ],
        _ => vec![
            ("item", mm("item")),
            ("getNamedItem", m_get_named_item as MethodFn),
            ("getNamedItemNS", m_get_named_item_ns),
            ("count", mm("count")),
            ("getIterator", mm("getIterator")),
        ],
    }
}
