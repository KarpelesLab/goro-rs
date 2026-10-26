//! Dom\TokenList (token_list.c): Element::$classList.

use super::*;
use std::cell::Cell;

const ASCII_WS: &[char] = &['\t', '\n', '\x0C', '\r', ' '];

/// dom_ordered_set_parser
pub fn ordered_set_parse(s_: &str, to_lowercase: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tok in s_.split(ASCII_WS).filter(|x| !x.is_empty()) {
        let tok = if to_lowercase {
            tok.to_ascii_lowercase()
        } else {
            tok.to_string()
        };
        if !out.contains(&tok) {
            out.push(tok);
        }
    }
    out
}

/// dom_ordered_set_all_contained
pub fn ordered_set_all_contained(set: &[String], value: &str, to_lowercase: bool) -> bool {
    let mut needed: Vec<&String> = set.iter().collect();
    for tok in value.split(ASCII_WS).filter(|x| !x.is_empty()) {
        let tok = if to_lowercase {
            tok.to_ascii_lowercase()
        } else {
            tok.to_string()
        };
        needed.retain(|x| **x != tok);
        if needed.is_empty() {
            break;
        }
    }
    needed.is_empty()
}

static TAG_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn new_tag() -> u64 {
    TAG_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

pub struct TokenList {
    pub element: Obj,
    /// hash buckets in insertion order (None = deleted)
    set: RefCell<Vec<Option<String>>>,
    cached: RefCell<Option<String>>,
    tag: Cell<u64>,
}

pub struct TokenIter {
    pub list: Obj,
    pos: Cell<usize>,
    index: Cell<i64>,
    tag: u64,
    rewound: Cell<bool>,
    /// The wrapped zend_object_iterator's handle
    _handle: Option<Obj>,
}

fn element_node(tl: &TokenList) -> Option<NodeId> {
    node_of(&tl.element)
}

fn class_value(tl: &TokenList) -> Option<String> {
    let n = element_node(tl)?;
    tree::with(|t| {
        let a = t.has_ns_prop(n, "class", None)?;
        t.nodes[a].children?;
        Some(t.node_get_content(a))
    })
}

fn rebuild(tl: &TokenList) {
    let v = class_value(tl);
    let set: Vec<Option<String>> = match &v {
        Some(s_) => ordered_set_parse(s_, false).into_iter().map(Some).collect(),
        None => Vec::new(),
    };
    *tl.set.borrow_mut() = set;
    *tl.cached.borrow_mut() = v;
}

/// dom_token_list_ensure_set_up_to_date
fn ensure(tl: &TokenList) {
    let v = class_value(tl);
    if v != *tl.cached.borrow() {
        tl.tag.set(new_tag());
        rebuild(tl);
    }
}

fn live(tl: &TokenList) -> Vec<String> {
    tl.set.borrow().iter().flatten().cloned().collect()
}

fn find(tl: &TokenList, tok: &str) -> Option<usize> {
    tl.set
        .borrow()
        .iter()
        .position(|x| x.as_deref() == Some(tok))
}

/// dom_token_list_update: write the serialized set back to the attribute.
fn update(tl: &TokenList) {
    tl.tag.set(new_tag());
    let has_attr = element_node(tl)
        .map(|n| tree::with(|t| t.has_ns_prop(n, "class", None).is_some()))
        .unwrap_or(false);
    let toks = live(tl);
    if !has_attr && toks.is_empty() {
        return;
    }
    let value = toks.join(" ");
    if let Some(n) = element_node(tl) {
        tree::with(|t| {
            t.set_ns_prop(n, None, "class", &value);
        });
    }
    *tl.cached.borrow_mut() = Some(value);
}

pub fn new_token_list(vm: &mut Vm, element: Obj) -> Obj {
    let o = new_object(vm, b"Dom\\TokenList");
    let tl = TokenList {
        element,
        set: RefCell::new(Vec::new()),
        cached: RefCell::new(None),
        tag: Cell::new(0),
    };
    rebuild(&tl);
    attach_with(&o, Kind::Modern(MKind::TokenList(tl)), &HANDLERS);
    o
}

fn with_tl<R>(o: &Obj, f: impl FnOnce(&TokenList) -> R) -> Option<R> {
    let d = data_of(o)?;
    let k = d.kind.borrow();
    match &*k {
        Kind::Modern(MKind::TokenList(tl)) => Some(f(tl)),
        _ => None,
    }
}

fn item(o: &Obj, index: i64) -> Value {
    with_tl(o, |tl| {
        ensure(tl);
        let toks = live(tl);
        if index >= 0 && (index as usize) < toks.len() {
            s(&toks[index as usize])
        } else {
            Value::Null
        }
    })
    .unwrap_or(Value::Null)
}

fn length(o: &Obj) -> i64 {
    with_tl(o, |tl| {
        ensure(tl);
        live(tl).len() as i64
    })
    .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Handlers

fn offset_to_long(vm: &mut Vm, v: &Value) -> Option<i64> {
    match v {
        Value::Resource(id) => {
            vm.emit_warning(&format!(
                "Resource ID#{} used as offset, casting to integer ({})",
                id, id
            ));
            Some(id.id())
        }
        Value::String(s_) => {
            let b = s_.as_bytes();
            let st = std::str::from_utf8(b).ok()?;
            let canonical = st.parse::<i64>().ok().filter(|n| n.to_string() == st)?;
            Some(canonical)
        }
        Value::Double(d) => Some(super::collections::dval_to_lval_safe(vm, *d)),
        Value::Long(n) => Some(*n),
        Value::False => Some(0),
        Value::True => Some(1),
        Value::Reference(r) => {
            let inner = r.borrow().clone();
            offset_to_long(vm, &inner)
        }
        _ => None,
    }
}

fn offset_type(v: &Value) -> String {
    match v {
        Value::Null | Value::Undef => "null".into(),
        Value::Array(_) => "array".into(),
        other => type_name_of(other),
    }
}

fn read_dimension(vm: &mut Vm, o: &Obj, offset: Option<&Value>, _mode: u8) -> Option<Value> {
    let off = match offset {
        Some(v) => v.deref(),
        None => {
            throw_error(vm, b"Error", "Cannot append to Dom\\TokenList");
            return Some(Value::Null);
        }
    };
    match offset_to_long(vm, &off) {
        Some(i) => Some(item(o, i)),
        None => {
            throw_error(
                vm,
                b"TypeError",
                &format!(
                    "Cannot access offset of type {} on Dom\\TokenList",
                    offset_type(&off)
                ),
            );
            Some(Value::Null)
        }
    }
}

fn has_dimension(vm: &mut Vm, o: &Obj, offset: &Value, check_empty: u8) -> Option<bool> {
    let off = offset.deref();
    match offset_to_long(vm, &off) {
        Some(i) => {
            if check_empty == 1 {
                Some(item(o, i).is_truthy())
            } else {
                Some(i >= 0 && i < length(o))
            }
        }
        None => {
            throw_error(
                vm,
                b"TypeError",
                &format!(
                    "Cannot access offset of type {} in isset or empty",
                    offset_type(&off)
                ),
            );
            Some(false)
        }
    }
}

fn count(_vm: &mut Vm, o: &Obj) -> Option<i64> {
    Some(length(o))
}

pub static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    read_property: Some(h_read_property_pub),
    write_property: Some(h_write_property_pub),
    has_property: Some(h_has_property_pub),
    unset_property: Some(h_unset_property_pub),
    read_dimension: Some(read_dimension),
    write_dimension: None,
    has_dimension: Some(has_dimension),
    unset_dimension: None,
    get_properties: Some(h_get_properties_pub),
    count_elements: Some(count),
    cast: None,
    compare: None,
    clone_obj: Some(h_uncloneable),
};

// ---------------------------------------------------------------------------
// Properties

fn r_length(_vm: &mut Vm, o: &Obj, _n: NodeId) -> R<Value> {
    Ok(Value::Long(length(o)))
}

fn r_value(_vm: &mut Vm, o: &Obj, _n: NodeId) -> R<Value> {
    Ok(s(&with_tl(o, class_value).flatten().unwrap_or_default()))
}

fn w_value(vm: &mut Vm, o: &Obj, _n: NodeId, v: &Value) -> R<()> {
    let st = crate::dom::node::str_of(v);
    if st.contains('\0') {
        throw_error(vm, b"ValueError", "Value must not contain any null bytes");
        return Err(());
    }
    with_tl(o, |tl| {
        if let Some(n) = element_node(tl) {
            tree::with(|t| {
                t.set_ns_prop(n, None, "class", &st);
            });
        }
    });
    Ok(())
}

pub fn props() -> Vec<Prop> {
    vec![
        p("length", r_length, None, "int"),
        p("value", r_value, Some(w_value), "string"),
    ]
}

// ---------------------------------------------------------------------------
// Methods

fn validate_token(vm: &mut Vm, tok: &str) -> bool {
    if tok.is_empty() {
        dom_err_msg(vm, SYNTAX_ERR, "The empty string is not a valid token");
        return false;
    }
    if tok.contains(ASCII_WS) {
        dom_err_msg(
            vm,
            INVALID_CHARACTER_ERR,
            "The token must not contain any ASCII whitespace",
        );
        return false;
    }
    true
}

fn tokens_varargs(vm: &mut Vm, a: &[Value]) -> R<Vec<String>> {
    let f = cur_method();
    let mut out = Vec::new();
    for (i, v) in a.iter().enumerate() {
        let v = v.deref();
        match &v {
            Value::String(st) => {
                let st = st.to_string_lossy();
                if st.contains('\0') {
                    throw_error(
                        vm,
                        b"ValueError",
                        &format!(
                            "{}(): Argument #{} must not contain any null bytes",
                            f,
                            i + 1
                        ),
                    );
                    return Err(());
                }
                if !validate_token(vm, &st) {
                    return Err(());
                }
                out.push(st);
            }
            other => {
                throw_error(
                    vm,
                    b"TypeError",
                    &format!(
                        "{}(): Argument #{} must be of type string, {} given",
                        f,
                        i + 1,
                        type_name_of(other)
                    ),
                );
                return Err(());
            }
        }
    }
    Ok(out)
}

fn m_item(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let i = a.long(vm, 0, "index", 0)?;
    Ok(item(o, i))
}

fn m_contains(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let tok = a.path_str(vm, 0, "token")?;
    Ok(bv(with_tl(o, |tl| {
        ensure(tl);
        find(tl, &tok).is_some()
    })
    .unwrap_or(false)))
}

fn m_add(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let toks = tokens_varargs(vm, a)?;
    with_tl(o, |tl| {
        ensure(tl);
        for t_ in toks {
            if find(tl, &t_).is_none() {
                tl.set.borrow_mut().push(Some(t_));
            }
        }
        update(tl);
    });
    Ok(Value::Null)
}

fn m_remove(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let toks = tokens_varargs(vm, a)?;
    with_tl(o, |tl| {
        ensure(tl);
        for t_ in toks {
            if let Some(i) = find(tl, &t_) {
                tl.set.borrow_mut()[i] = None;
            }
        }
        update(tl);
    });
    Ok(Value::Null)
}

fn m_toggle(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 2)?;
    let tok = a.path_str(vm, 0, "token")?;
    let force = match a.raw(1) {
        None | Some(Value::Null) => None,
        Some(_) => a.opt_bool(vm, 1, "force")?,
    };
    if !validate_token(vm, &tok) {
        return Err(());
    }
    let r = with_tl(o, |tl| {
        ensure(tl);
        match find(tl, &tok) {
            Some(i) => {
                if !force.unwrap_or(false) {
                    tl.set.borrow_mut()[i] = None;
                    update(tl);
                    false
                } else {
                    true
                }
            }
            None => {
                if force.unwrap_or(true) {
                    tl.set.borrow_mut().push(Some(tok.clone()));
                    update(tl);
                    true
                } else {
                    false
                }
            }
        }
    });
    Ok(bv(r.unwrap_or(false)))
}

fn m_replace(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let tok = a.path_str(vm, 0, "token")?;
    let new_tok = a.path_str(vm, 1, "newToken")?;
    if !validate_token(vm, &tok) || !validate_token(vm, &new_tok) {
        return Err(());
    }
    let r = with_tl(o, |tl| {
        ensure(tl);
        let i = match find(tl, &tok) {
            Some(i) => i,
            None => return false,
        };
        let exists = find(tl, &new_tok);
        {
            let mut set = tl.set.borrow_mut();
            match exists {
                Some(j) if j != i => set[i] = None,
                _ => set[i] = Some(new_tok.clone()),
            }
        }
        update(tl);
        true
    });
    Ok(bv(r.unwrap_or(false)))
}

fn m_supports(vm: &mut Vm, _o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let _tok = a.path_str(vm, 0, "token")?;
    throw_error(
        vm,
        b"TypeError",
        "Attribute \"class\" does not define any supported tokens",
    );
    Err(())
}

fn m_count(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    Ok(Value::Long(length(o)))
}

fn m_get_iterator(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let tag = with_tl(o, |tl| {
        ensure(tl);
        tl.tag.get()
    })
    .unwrap_or(0);
    let handle = if vm.foreach_native_iter == 1 {
        vm.foreach_native_iter = 2;
        None
    } else {
        Some(crate::dom::lists::iterator_handle(vm))
    };
    let it = new_object(vm, b"InternalIterator");
    let st = TokenIter {
        list: o.clone(),
        pos: Cell::new(0),
        index: Cell::new(0),
        tag,
        rewound: Cell::new(false),
        _handle: handle,
    };
    attach(&it, Kind::Modern(MKind::TokenIter(st)));
    Ok(Value::Object(it))
}

pub fn methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("item", m_item as MethodFn),
        ("contains", m_contains),
        ("add", m_add),
        ("remove", m_remove),
        ("toggle", m_toggle),
        ("replace", m_replace),
        ("supports", m_supports),
        ("count", m_count),
        ("getIterator", m_get_iterator),
    ]
}

// ---------------------------------------------------------------------------
// Iterator (dom_token_list_it_*, through InternalIterator)

/// zend_hash_get_current_pos_ex: skip deleted buckets.
fn valid_pos(tl: &TokenList, pos: usize) -> usize {
    let set = tl.set.borrow();
    let mut p = pos;
    while p < set.len() && set[p].is_none() {
        p += 1;
    }
    p
}

pub fn iter_method(vm: &mut Vm, it: &Obj, method: &[u8]) -> Option<Value> {
    let _ = vm;
    let d = data_of(it)?;
    let k = d.kind.borrow();
    let st = match &*k {
        Kind::Modern(MKind::TokenIter(st)) => st,
        _ => return None,
    };
    let m = String::from_utf8_lossy(method).to_ascii_lowercase();
    let list = st.list.clone();
    let ld = data_of(&list)?;
    let lk = ld.kind.borrow();
    let tl = match &*lk {
        Kind::Modern(MKind::TokenList(tl)) => tl,
        _ => return None,
    };
    let rewind = |st: &TokenIter| {
        st.rewound.set(true);
        st.pos.set(0);
        st.index.set(0);
    };
    if m != "rewind" && !st.rewound.get() {
        rewind(st);
    }
    match m.as_str() {
        "rewind" => {
            rewind(st);
            Some(Value::Null)
        }
        "valid" => {
            ensure(tl);
            let p = valid_pos(tl, st.pos.get());
            st.pos.set(p);
            Some(bv(p < tl.set.borrow().len()))
        }
        "current" => {
            ensure(tl);
            let p = valid_pos(tl, st.pos.get());
            let set = tl.set.borrow();
            Some(match set.get(p) {
                Some(Some(x)) => s(x),
                _ => Value::Null,
            })
        }
        "key" => {
            ensure(tl);
            if tl.tag.get() != st.tag {
                let set = tl.set.borrow();
                let pos = st.pos.get();
                let mut idx = 0;
                let mut p = 0;
                while p < pos && p < set.len() {
                    if set[p].is_some() {
                        idx += 1;
                    }
                    p += 1;
                }
                st.index.set(idx);
            }
            Some(Value::Long(st.index.get()))
        }
        "next" => {
            ensure(tl);
            let cur = st.pos.get();
            let validated = valid_pos(tl, cur);
            if validated != cur {
                st.pos.set(validated);
            } else {
                st.pos.set(valid_pos(tl, cur + 1));
            }
            st.index.set(st.index.get() + 1);
            Some(Value::Null)
        }
        _ => None,
    }
}
