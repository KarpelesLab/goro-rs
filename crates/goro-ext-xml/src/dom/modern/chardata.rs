//! Dom\CharacterData, Dom\Text, Dom\ProcessingInstruction,
//! Dom\DocumentType, Dom\Entity, Dom\EntityReference, Dom\DocumentFragment
//! (characterdata.c, text.c, documenttype.c, entityreference.c,
//! documentfragment.c; modern branches).

use super::*;

fn utf8_len(s_: &str) -> usize {
    s_.chars().count()
}

/// xmlUTF8Strsub
fn utf8_sub(s_: &str, start: usize, len: usize) -> String {
    s_.chars().skip(start).take(len).collect()
}

fn content_of(n: NodeId) -> String {
    tree::with(|t| t.nodes[n].content.clone())
}

fn set_content(n: NodeId, v: &str) {
    tree::with(|t| t.node_set_content(n, v));
}

fn int_ovfl(v: i64) -> bool {
    v > i32::MAX as i64 || v < i32::MIN as i64
}

/// dom_convert_number_unsigned (modern: negative numbers wrap around)
fn unsigned(v: i64) -> u32 {
    v as u32
}

fn index_err(vm: &mut Vm) -> R<Value> {
    dom_err(vm, INDEX_SIZE_ERR);
    Err(())
}

fn m_substring_data(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let off = a.long(vm, 0, "offset", 0)?;
    let cnt = a.long(vm, 1, "count", 0)?;
    let n = need_node(vm, o)?;
    let cur = content_of(n);
    let length = utf8_len(&cur) as u32;
    if int_ovfl(off) || int_ovfl(cnt) {
        return index_err(vm);
    }
    let (off, mut cnt) = (unsigned(off), unsigned(cnt));
    if off > length {
        return index_err(vm);
    }
    if cnt > length - off {
        cnt = length - off;
    }
    Ok(s(&utf8_sub(&cur, off as usize, cnt as usize)))
}

fn m_append_data(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let d = a.str(vm, 0, "data")?;
    let n = need_node(vm, o)?;
    tree::with(|t| {
        t.nodes[n].content.push_str(&d);
        t.nodes[n].flags &= !tree::F_NULL_CONTENT;
    });
    Ok(Value::Null)
}

fn m_insert_data(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let off = a.long(vm, 0, "offset", 0)?;
    let d = a.str(vm, 1, "data")?;
    let n = need_node(vm, o)?;
    let cur = content_of(n);
    let length = utf8_len(&cur) as u32;
    if int_ovfl(off) {
        return index_err(vm);
    }
    let off = unsigned(off);
    if off > length {
        return index_err(vm);
    }
    let first = utf8_sub(&cur, 0, off as usize);
    let second = utf8_sub(&cur, off as usize, (length - off) as usize);
    set_content(n, &format!("{}{}{}", first, d, second));
    Ok(Value::Null)
}

fn m_delete_data(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let off = a.long(vm, 0, "offset", 0)?;
    let cnt = a.long(vm, 1, "count", 0)?;
    let n = need_node(vm, o)?;
    let cur = content_of(n);
    let length = utf8_len(&cur) as i64;
    if off < 0 || int_ovfl(off) || int_ovfl(cnt) || off > length {
        return index_err(vm);
    }
    let mut cnt = unsigned(cnt) as i64;
    if cnt > length - off {
        cnt = length - off;
    }
    let first = utf8_sub(&cur, 0, off as usize);
    let second = utf8_sub(&cur, (off + cnt) as usize, (length - off) as usize);
    set_content(n, &format!("{}{}", first, second));
    Ok(Value::Null)
}

fn m_replace_data(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 3, 3)?;
    let off = a.long(vm, 0, "offset", 0)?;
    let cnt = a.long(vm, 1, "count", 0)?;
    let d = a.str(vm, 2, "data")?;
    let n = need_node(vm, o)?;
    let cur = content_of(n);
    let length = utf8_len(&cur) as i64;
    if off < 0 || int_ovfl(off) || int_ovfl(cnt) || off > length {
        return index_err(vm);
    }
    let mut cnt = unsigned(cnt) as i64;
    if cnt > length - off {
        cnt = length - off;
    }
    let first = utf8_sub(&cur, 0, off as usize);
    let second = if off < length {
        utf8_sub(&cur, (off + cnt) as usize, (length - off) as usize)
    } else {
        String::new()
    };
    set_content(n, &format!("{}{}{}", first, d, second));
    Ok(Value::Null)
}

pub fn chardata_methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("substringData", m_substring_data as MethodFn),
        ("appendData", m_append_data),
        ("insertData", m_insert_data),
        ("deleteData", m_delete_data),
        ("replaceData", m_replace_data),
    ]
}

pub fn chardata_props(t: &HashMap<Base, Rc<Vec<Prop>>>) -> Vec<Prop> {
    let l = |n: &str| {
        let mut p = lp(t, Base::CharacterData, n);
        p.owner = "";
        p
    };
    vec![
        l("data"),
        l("length"),
        p(
            "previousElementSibling",
            crate::dom::node::r_prev_element_sibling,
            None,
            "mixed",
        ),
        p(
            "nextElementSibling",
            crate::dom::node::r_next_element_sibling,
            None,
            "mixed",
        ),
    ]
}

pub fn text_props(t: &HashMap<Base, Rc<Vec<Prop>>>) -> Vec<Prop> {
    let mut p = lp(t, Base::Text, "wholeText");
    p.owner = "";
    vec![p]
}

fn m_split_text(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let offset = a.long(vm, 0, "offset", 0)?;
    let n = need_node(vm, o)?;
    if offset < 0 {
        throw_error(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #1 ($offset) must be greater than or equal to 0",
                a.f
            ),
        );
        return Err(());
    }
    let cur = content_of(n);
    let length = utf8_len(&cur) as i64;
    if int_ovfl(offset) || offset > length {
        return index_err(vm);
    }
    let first = utf8_sub(&cur, 0, offset as usize);
    let second = utf8_sub(&cur, offset as usize, (length - offset) as usize);
    let nn = tree::with(|t| {
        t.node_set_content(n, &first);
        let doc = t.nodes[n].doc;
        let nn = t.new_text(doc, &second);
        if t.nodes[n].parent.is_some() {
            t.insert_after_raw(n, nn);
        }
        nn
    });
    Ok(node_obj(vm, nn))
}

pub fn text_methods() -> Vec<(&'static str, MethodFn)> {
    vec![("splitText", m_split_text as MethodFn)]
}

pub fn pi_props(t: &HashMap<Base, Rc<Vec<Prop>>>) -> Vec<Prop> {
    let l = |n: &str| {
        let mut p = lp(t, Base::ProcessingInstruction, n);
        p.owner = "";
        p
    };
    vec![l("target"), l("data")]
}

fn r_entities(vm: &mut Vm, o: &Obj, _n: NodeId) -> R<Value> {
    Ok(mmap_obj(
        vm,
        b"Dom\\DtdNamedNodeMap",
        MapKind::Entities(o.clone()),
    ))
}

fn r_notations(vm: &mut Vm, o: &Obj, _n: NodeId) -> R<Value> {
    Ok(mmap_obj(
        vm,
        b"Dom\\DtdNamedNodeMap",
        MapKind::Notations(o.clone()),
    ))
}

pub fn doctype_props(t: &HashMap<Base, Rc<Vec<Prop>>>) -> Vec<Prop> {
    let l = |n: &str| {
        let mut p = lp(t, Base::DocumentType, n);
        p.owner = "";
        p
    };
    vec![
        l("name"),
        p("entities", r_entities, None, "mixed"),
        p("notations", r_notations, None, "mixed"),
        l("publicId"),
        l("systemId"),
        l("internalSubset"),
    ]
}

pub fn entity_props(t: &HashMap<Base, Rc<Vec<Prop>>>) -> Vec<Prop> {
    let l = |n: &str| {
        let mut p = lp(t, Base::Entity, n);
        p.owner = "";
        p
    };
    vec![l("publicId"), l("systemId"), l("notationName")]
}

fn r_eref_text_content(vm: &mut Vm, o: &Obj, n: NodeId) -> R<Value> {
    crate::dom::chardata::sync_entity_ref(n);
    (node::text_content_rw("").read)(vm, o, n)
}

fn r_eref_child_nodes(vm: &mut Vm, o: &Obj, n: NodeId) -> R<Value> {
    crate::dom::chardata::sync_entity_ref(n);
    Ok(mlist_obj(
        vm,
        b"Dom\\NodeList",
        ListKind::Children(o.clone()),
    ))
}

pub fn entity_ref_overrides(t: &HashMap<Base, Rc<Vec<Prop>>>) -> Vec<Prop> {
    let l = |n: &str| {
        let mut p = lp(t, Base::EntityReference, n);
        p.owner = "";
        p
    };
    let mut tc = node::text_content_rw("");
    tc.read = r_eref_text_content;
    tc.write = None;
    vec![
        l("firstChild"),
        l("lastChild"),
        tc,
        p("childNodes", r_eref_child_nodes, None, "mixed"),
    ]
}

fn m_append_xml(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let f = legacy_method(Base::DocumentFragment, "appendXML");
    let r = f(vm, o, a)?;
    if let Some(n) = node_of(o) {
        tree::with(|t| {
            if let Some(d) = doc_from_node(t, n) {
                let mut cur = t.nodes[n].children;
                while let Some(x) = cur {
                    if t.nodes[x].t == NType::Element {
                        compat_mark_attribute_list(t, d, x);
                    }
                    cur = next_in_tree_order(t, x, Some(n));
                }
                for c in t.children_of(n) {
                    reconcile_modern(t, d, c);
                }
            }
        });
    }
    Ok(r)
}

pub fn fragment_methods() -> Vec<(&'static str, MethodFn)> {
    vec![("appendXml", m_append_xml as MethodFn)]
}
