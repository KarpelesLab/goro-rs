//! DOMCharacterData, DOMText, DOMComment, DOMCdataSection, DOMDocumentType,
//! DOMNotation, DOMEntity, DOMEntityReference, DOMProcessingInstruction,
//! DOMNameSpaceNode, DOMDocumentFragment, DOMImplementation.

use super::node::{args, b, opt_s, s, str_of};
use super::*;
use crate::parser::is_valid_name;
use goro_core::vm::VmError;

fn utf8_len(s: &str) -> usize {
    s.chars().count()
}

/// xmlUTF8Strsub semantics (character offsets, clamped to the end)
fn utf8_sub(s: &str, start: usize, len: usize) -> String {
    s.chars().skip(start).take(len).collect()
}

// ---------------------------------------------------------------------------
// CharacterData

fn r_data(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| super::node::get_content_value(t, n, false)))
}

fn w_data(_vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let st = str_of(v);
    tree::with(|t| t.node_set_content(n, &st));
    Ok(())
}

fn r_length(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(Value::Long(
        tree::with(|t| utf8_len(&t.nodes[n].content)) as i64
    ))
}

pub fn chardata_props() -> Vec<Prop> {
    vec![
        p("data", r_data, Some(w_data), "string"),
        p("length", r_length, None, "int"),
        p(
            "previousElementSibling",
            super::node::r_prev_element_sibling,
            None,
            "mixed",
        ),
        p(
            "nextElementSibling",
            super::node::r_next_element_sibling,
            None,
            "mixed",
        ),
    ]
}

fn content_of(n: NodeId) -> String {
    tree::with(|t| t.nodes[n].content.clone())
}

fn int_ovfl(v: i64) -> bool {
    v > i32::MAX as i64 || v < i32::MIN as i64
}

fn index_err(vm: &mut Vm, n: NodeId) -> R<Value> {
    throw_dom(vm, INDEX_SIZE_ERR, strict(doc_of_node(n)));
    Ok(Value::False)
}

fn m_substring_data(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let offset = a.long(vm, 0, "offset", 0)?;
    let count = a.long(vm, 1, "count", 0)?;
    let n = need_node(vm, o)?;
    let cur = content_of(n);
    let length = utf8_len(&cur) as i64;
    if int_ovfl(offset) || int_ovfl(count) || offset < 0 || count < 0 {
        return index_err(vm, n);
    }
    if offset > length {
        return index_err(vm, n);
    }
    let count = count.min(length - offset);
    Ok(s(&utf8_sub(&cur, offset as usize, count as usize)))
}

fn m_append_data(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let data = a.str(vm, 0, "data")?;
    let n = need_node(vm, o)?;
    tree::with(|t| {
        if matches!(
            t.nodes[n].t,
            NType::Text | NType::CData | NType::Comment | NType::Pi
        ) {
            t.nodes[n].content.push_str(&data);
        }
    });
    Ok(Value::True)
}

fn m_insert_data(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let offset = a.long(vm, 0, "offset", 0)?;
    let data = a.str(vm, 1, "data")?;
    let n = need_node(vm, o)?;
    let cur = content_of(n);
    let length = utf8_len(&cur) as i64;
    if int_ovfl(offset) || offset < 0 || offset > length {
        return index_err(vm, n);
    }
    let first = utf8_sub(&cur, 0, offset as usize);
    let second = utf8_sub(&cur, offset as usize, (length - offset) as usize);
    tree::with(|t| t.node_set_content(n, &format!("{}{}{}", first, data, second)));
    Ok(Value::True)
}

fn m_delete_data(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let offset = a.long(vm, 0, "offset", 0)?;
    let count = a.long(vm, 1, "count", 0)?;
    let n = need_node(vm, o)?;
    let cur = content_of(n);
    let length = utf8_len(&cur) as i64;
    if offset < 0 || int_ovfl(offset) || int_ovfl(count) || offset > length || count < 0 {
        return index_err(vm, n);
    }
    let count = count.min(length - offset);
    let first = utf8_sub(&cur, 0, offset as usize);
    let second = utf8_sub(&cur, (offset + count) as usize, usize::MAX);
    tree::with(|t| t.node_set_content(n, &format!("{}{}", first, second)));
    Ok(Value::True)
}

fn m_replace_data(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 3, 3)?;
    let offset = a.long(vm, 0, "offset", 0)?;
    let count = a.long(vm, 1, "count", 0)?;
    let data = a.str(vm, 2, "data")?;
    let n = need_node(vm, o)?;
    let cur = content_of(n);
    let length = utf8_len(&cur) as i64;
    if offset < 0 || int_ovfl(offset) || int_ovfl(count) || offset > length || count < 0 {
        return index_err(vm, n);
    }
    let count = count.min(length - offset);
    let first = utf8_sub(&cur, 0, offset as usize);
    let second = if offset < length {
        utf8_sub(&cur, (offset + count) as usize, usize::MAX)
    } else {
        String::new()
    };
    tree::with(|t| t.node_set_content(n, &format!("{}{}{}", first, data, second)));
    Ok(Value::True)
}

pub fn chardata_methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("substringData", m_substring_data),
        ("appendData", m_append_data),
        ("insertData", m_insert_data),
        ("deleteData", m_delete_data),
        ("replaceData", m_replace_data),
    ]
}

// ---------------------------------------------------------------------------
// Text

fn r_whole_text(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| {
        let is_text = |t: &Tree, x: NodeId| matches!(t.nodes[x].t, NType::Text | NType::CData);
        let mut cur = n;
        while let Some(p) = t.nodes[cur].prev {
            if is_text(t, p) {
                cur = p;
            } else {
                break;
            }
        }
        let mut out = String::new();
        let mut c = Some(cur);
        while let Some(x) = c {
            if !is_text(t, x) {
                break;
            }
            out.push_str(&t.nodes[x].content);
            c = t.nodes[x].next;
        }
        s(&out)
    }))
}

pub fn text_props() -> Vec<Prop> {
    vec![p("wholeText", r_whole_text, None, "string")]
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
        return Ok(Value::False);
    }
    let first = utf8_sub(&cur, 0, offset as usize);
    let second = utf8_sub(&cur, offset as usize, (length - offset) as usize);
    let nn = tree::with(|t| {
        t.node_set_content(n, &first);
        let doc = t.nodes[n].doc;
        let nn = t.new_text(doc, &second);
        if t.nodes[n].parent.is_some() {
            // xmlAddNextSibling without text merging
            t.insert_after_raw(n, nn);
        }
        nn
    });
    Ok(node_obj(vm, nn))
}

fn m_is_whitespace(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    args(a).count(vm, 0, 0)?;
    let n = need_node(vm, o)?;
    Ok(b(tree::with(|t| {
        matches!(t.nodes[n].t, NType::Text | NType::CData)
            && tree::is_blank_str(&t.nodes[n].content)
    })))
}

pub fn text_methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("splitText", m_split_text),
        ("isWhitespaceInElementContent", m_is_whitespace),
        ("isElementContentWhitespace", m_is_whitespace),
    ]
}

// ---------------------------------------------------------------------------
// DocumentType

fn r_dt_name(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| s(&t.nodes[n].name)))
}

fn r_dt_entities(vm: &mut Vm, o: &Obj, n: NodeId) -> R<Value> {
    Ok(map_obj(vm, MapKind::Entities(o.clone())))
}

fn r_dt_notations(vm: &mut Vm, o: &Obj, n: NodeId) -> R<Value> {
    Ok(map_obj(vm, MapKind::Notations(o.clone())))
}

fn r_dt_public(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| {
        s(t.dtd_info(n)
            .and_then(|d| d.external_id.as_deref())
            .unwrap_or(""))
    }))
}

fn r_dt_system(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| {
        s(t.dtd_info(n)
            .and_then(|d| d.system_id.as_deref())
            .unwrap_or(""))
    }))
}

fn r_dt_internal_subset(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| {
        let doc = match t.nodes[n].doc {
            Some(d) => d,
            None => return Value::Null,
        };
        let sub = match t.int_subset(doc) {
            Some(s) => s,
            None => return Value::Null,
        };
        let mut out: Vec<u8> = Vec::new();
        for c in t.children_of(sub) {
            out.extend(crate::serialize::node_to_bytes(t, c, None, false, false));
        }
        if out.is_empty() {
            Value::Null
        } else {
            Value::String(PhpString::from_vec(out))
        }
    }))
}

pub fn doctype_props() -> Vec<Prop> {
    vec![
        p("name", r_dt_name, None, "string"),
        p("entities", r_dt_entities, None, "mixed"),
        p("notations", r_dt_notations, None, "mixed"),
        p("publicId", r_dt_public, None, "string"),
        p("systemId", r_dt_system, None, "string"),
        p("internalSubset", r_dt_internal_subset, None, "?string"),
    ]
}

// ---------------------------------------------------------------------------
// Notation

fn notation_ids(t: &Tree, n: NodeId) -> (Option<String>, Option<String>) {
    match &t.nodes[n].extra {
        tree::Extra::Notation(p, s) => (p.clone(), s.clone()),
        _ => (None, None),
    }
}

fn r_not_public(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| {
        s(notation_ids(t, n).0.as_deref().unwrap_or(""))
    }))
}

fn r_not_system(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| {
        s(notation_ids(t, n).1.as_deref().unwrap_or(""))
    }))
}

pub fn notation_props() -> Vec<Prop> {
    vec![
        p("publicId", r_not_public, None, "string"),
        p("systemId", r_not_system, None, "string"),
    ]
}

// ---------------------------------------------------------------------------
// Entity

fn unparsed(t: &Tree, n: NodeId) -> Option<&tree::EntityInfo> {
    t.entity_info(n)
        .filter(|e| e.etype == tree::EntityType::ExternalGeneralUnparsed)
}

fn r_ent_public(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| {
        match unparsed(t, n).and_then(|e| e.external_id.as_deref()) {
            Some(x) => s(x),
            None => Value::Null,
        }
    }))
}

fn r_ent_system(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| match unparsed(t, n) {
        Some(e) => opt_s(e.system_id.as_deref()),
        None => Value::Null,
    }))
}

fn r_ent_notation(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| match unparsed(t, n) {
        Some(e) => s(e.notation.as_deref().unwrap_or("")),
        None => Value::Null,
    }))
}

fn r_ent_actual_encoding(vm: &mut Vm, _o: &Obj, _n: NodeId) -> R<Value> {
    if !in_debug() {
        vm.emit_deprecated("Property DOMEntity::$actualEncoding is deprecated");
    }
    Ok(Value::Null)
}

fn r_ent_encoding(vm: &mut Vm, _o: &Obj, _n: NodeId) -> R<Value> {
    if !in_debug() {
        vm.emit_deprecated("Property DOMEntity::$encoding is deprecated");
    }
    Ok(Value::Null)
}

fn r_ent_version(vm: &mut Vm, _o: &Obj, _n: NodeId) -> R<Value> {
    if !in_debug() {
        vm.emit_deprecated("Property DOMEntity::$version is deprecated");
    }
    Ok(Value::Null)
}

pub fn entity_props() -> Vec<Prop> {
    vec![
        p("publicId", r_ent_public, None, "?string"),
        p("systemId", r_ent_system, None, "?string"),
        p("notationName", r_ent_notation, None, "?string"),
        p("actualEncoding", r_ent_actual_encoding, None, "?string"),
        p("encoding", r_ent_encoding, None, "?string"),
        p("version", r_ent_version, None, "?string"),
    ]
}

// ---------------------------------------------------------------------------
// EntityReference

pub fn sync_entity_ref(n: NodeId) -> Option<NodeId> {
    tree::with(|t| {
        if t.nodes[n].t != NType::EntityRef {
            return t.nodes[n].children;
        }
        let ent = t.nodes[n]
            .doc
            .and_then(|d| t.get_doc_entity(d, &t.nodes[n].name.clone()));
        t.nodes[n].children = ent;
        t.nodes[n].last = ent;
        ent
    })
}

fn r_eref_child(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    let e = sync_entity_ref(n);
    Ok(node_obj_opt(vm, e))
}

fn r_eref_text_content(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    sync_entity_ref(n);
    Ok(tree::with(|t| s(&t.node_get_content(n))))
}

fn r_eref_child_nodes(vm: &mut Vm, o: &Obj, n: NodeId) -> R<Value> {
    sync_entity_ref(n);
    Ok(list_obj(vm, ListKind::Children(o.clone())))
}

pub fn entity_ref_overrides() -> Vec<Prop> {
    vec![
        p("firstChild", r_eref_child, None, "mixed"),
        p("lastChild", r_eref_child, None, "mixed"),
        p("textContent", r_eref_text_content, None, "string"),
        p("childNodes", r_eref_child_nodes, None, "mixed"),
    ]
}

// ---------------------------------------------------------------------------
// ProcessingInstruction

fn r_pi_target(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| s(&t.nodes[n].name)))
}

pub fn pi_props() -> Vec<Prop> {
    vec![
        p("target", r_pi_target, None, "string"),
        p("data", r_data, Some(w_data), "string"),
    ]
}

// ---------------------------------------------------------------------------
// DOMNameSpaceNode

pub fn nsnode_props() -> Vec<Prop> {
    let all = super::node::node_props();
    let pick = [
        "nodeName",
        "nodeValue",
        "nodeType",
        "prefix",
        "localName",
        "namespaceURI",
        "isConnected",
        "ownerDocument",
        "parentNode",
        "parentElement",
    ];
    let mut v = Vec::new();
    for name in pick {
        if let Some(pp) = all.iter().find(|x| x.name == name) {
            let mut pp = *pp;
            pp.write = None;
            v.push(pp);
        }
    }
    v
}

fn m_ns_sleep(vm: &mut Vm, o: &Obj, _a: &[Value]) -> R<Value> {
    let cn = goro_core::value::display_class_name(&o.borrow().class_name).to_string();
    throw_error(
        vm,
        b"Exception",
        &format!(
            "Serialization of '{}' is not allowed, unless serialization methods are implemented in a subclass",
            cn
        ),
    );
    Err(())
}

fn m_ns_wakeup(vm: &mut Vm, o: &Obj, _a: &[Value]) -> R<Value> {
    let cn = goro_core::value::display_class_name(&o.borrow().class_name).to_string();
    throw_error(
        vm,
        b"Exception",
        &format!(
            "Unserialization of '{}' is not allowed, unless unserialization methods are implemented in a subclass",
            cn
        ),
    );
    Err(())
}

pub fn nsnode_methods() -> Vec<(&'static str, MethodFn)> {
    vec![("__sleep", m_ns_sleep), ("__wakeup", m_ns_wakeup)]
}

// ---------------------------------------------------------------------------
// DocumentFragment

fn m_append_xml(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let data = a.str_bytes(vm, 0, "data")?;
    let n = need_node(vm, o)?;
    if tree::with(|t| super::node::is_read_only(t, n)) {
        throw_dom(vm, NO_MODIFICATION_ALLOWED_ERR, strict(doc_of_node(n)));
        return Ok(Value::False);
    }
    if data.is_empty() {
        return Ok(Value::True);
    }
    let (list, errors) = crate::sax2::parse_in_context(&data, n, 0);
    crate::libxml::report(vm, &a.f, &errors, crate::libxml::Style::Generic);
    match list {
        None => Ok(Value::False),
        Some(list) => {
            tree::with(|t| {
                for c in list {
                    t.add_child(n, c);
                }
            });
            Ok(Value::True)
        }
    }
}

pub fn fragment_methods() -> Vec<(&'static str, MethodFn)> {
    vec![("appendXML", m_append_xml)]
}

// ---------------------------------------------------------------------------
// DOMImplementation

fn m_has_feature(vm: &mut Vm, _o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let _f = a.str(vm, 0, "feature")?;
    let _v = a.str(vm, 1, "version")?;
    Ok(Value::True)
}

fn m_create_document_type(vm: &mut Vm, _o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 3)?;
    let name = a.str(vm, 0, "qualifiedName")?;
    let public = a.str(vm, 1, "publicId")?;
    let system = a.str(vm, 2, "systemId")?;
    if name.is_empty() {
        throw_error(
            vm,
            b"ValueError",
            &format!("{}(): Argument #1 ($qualifiedName) must not be empty", a.f),
        );
        return Err(());
    }
    if name.contains("%00") {
        warn(vm, "URI must not contain percent-encoded NUL bytes");
        return Ok(Value::False);
    }
    // xmlParseURI never fills uri->opaque: the name is used as is
    let local = name.clone();
    let p1 = if public.is_empty() {
        None
    } else {
        Some(public.as_str())
    };
    let p2 = if system.is_empty() {
        None
    } else {
        Some(system.as_str())
    };
    let d = tree::with(|t| t.create_int_subset(None, &local, p1, p2));
    Ok(node_obj(vm, d))
}

fn m_create_document(vm: &mut Vm, _o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 0, 3)?;
    let uri = a.opt_str(vm, 0, "namespace")?;
    let name = a.str(vm, 1, "qualifiedName")?;
    let dt = a.obj_of(vm, 2, "doctype", "DOMDocumentType", true)?;
    let mut doctype = None;
    if let Some(dto) = &dt {
        let dn = need_node(vm, dto)?;
        if tree::with(|t| t.nodes[dn].t == NType::DocumentType) {
            throw_error(
                vm,
                b"ValueError",
                &format!(
                    "{}(): Argument #3 ($doctype) is an invalid DocumentType object",
                    a.f
                ),
            );
            return Err(());
        }
        if tree::with(|t| t.nodes[dn].doc.is_some()) {
            throw_dom(vm, WRONG_DOCUMENT_ERR, true);
            return Err(());
        }
        doctype = Some(dn);
    }
    let mut local = None;
    let mut prefix = None;
    if !name.is_empty() {
        match super::document::check_qname(&name, 1) {
            Ok((p, l)) => {
                prefix = p;
                local = Some(l);
            }
            Err(code) => {
                throw_dom(vm, code, true);
                return Err(());
            }
        }
        if let Some(u) = uri.as_deref() {
            if !u.is_empty() && prefix.as_deref() == Some("xml") {
                // xmlNewNs refuses to create the "xml" prefix
                throw_dom(vm, NAMESPACE_ERR, true);
                return Err(());
            }
        }
    }
    let d = tree::with(|t| {
        let d = t.new_doc(Some("1.0"));
        if let Some(dt) = doctype {
            t.set_tree_doc(dt, Some(d));
            t.link_child_raw(d, dt);
            if let Some(di) = t.doc_info_mut(d) {
                di.int_subset = Some(dt);
            }
        }
        if let Some(l) = &local {
            let e = t.new_element(Some(d), l);
            if let Some(u) = uri.as_deref().filter(|u| !u.is_empty()) {
                let ns = t.new_ns_raw(u, prefix.as_deref(), Some(e));
                t.nodes[e].ns_def.push(ns);
                t.nodes[e].ns = Some(ns);
            }
            t.link_child_raw(d, e);
        }
        d
    });
    set_doc_props(d, DocProps::default());
    Ok(node_obj(vm, d))
}

pub fn implementation_methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("hasFeature", m_has_feature),
        ("createDocumentType", m_create_document_type),
        ("createDocument", m_create_document),
    ]
}

// ---------------------------------------------------------------------------
// Constructors (Text, Comment, CDATASection, PI, EntityReference, Fragment)

pub fn construct(vm: &mut Vm, this: &Obj, base: Base, a: &[Value]) -> R<()> {
    let a = args(a);
    let n = match base {
        Base::Text | Base::Comment => {
            a.count(vm, 0, 1)?;
            let v = a.str(vm, 0, "data")?;
            tree::with(|t| {
                if base == Base::Text {
                    t.new_text(None, &v)
                } else {
                    t.new_comment(None, &v)
                }
            })
        }
        Base::CdataSection => {
            a.count(vm, 1, 1)?;
            let v = a.str(vm, 0, "data")?;
            tree::with(|t| t.new_cdata(None, &v))
        }
        Base::ProcessingInstruction => {
            a.count(vm, 1, 2)?;
            let name = a.str(vm, 0, "name")?;
            let v = a.str(vm, 1, "value")?;
            if !is_valid_name(&name) {
                throw_dom(vm, INVALID_CHARACTER_ERR, true);
                return Err(());
            }
            tree::with(|t| t.new_pi(None, &name, &v))
        }
        Base::EntityReference => {
            a.count(vm, 1, 1)?;
            let name = a.str(vm, 0, "name")?;
            if !is_valid_name(&name) {
                throw_dom(vm, INVALID_CHARACTER_ERR, true);
                return Err(());
            }
            tree::with(|t| t.new_entity_ref(None, &name))
        }
        Base::DocumentFragment => {
            a.count(vm, 0, 0)?;
            tree::with(|t| t.new_fragment(None))
        }
        _ => return Ok(()),
    };
    attach(this, Kind::Node(n));
    set_wrapper(n, this);
    Ok(())
}

// ---------------------------------------------------------------------------
// dom_import_simplexml

pub fn dom_import_simplexml(vm: &mut Vm, a: &[Value]) -> Result<Value, VmError> {
    let f = "dom_import_simplexml";
    let v = a.first().map(|v| v.deref()).unwrap_or(Value::Undef);
    let o = match v {
        Value::Object(o) => o,
        Value::Undef => {
            throw_error(
                vm,
                b"ArgumentCountError",
                &format!("{}() expects exactly 1 argument, 0 given", f),
            );
            return Ok(Value::Null);
        }
        other => {
            throw_error(
                vm,
                b"TypeError",
                &format!(
                    "{}(): Argument #1 ($node) must be of type object, {} given",
                    f,
                    type_name_of(&other)
                ),
            );
            return Ok(Value::Null);
        }
    };
    let n = match crate::simplexml::sxe_of(&o) {
        Some(sx) => sx.node.get(),
        None => node_of(&o),
    };
    let ok = n
        .map(|n| {
            tree::with(|t| {
                t.nodes[n].doc.is_some()
                    && matches!(t.nodes[n].t, NType::Element | NType::Attribute)
            })
        })
        .unwrap_or(false);
    if !ok {
        throw_error(
            vm,
            b"TypeError",
            &format!("{}(): Argument #1 ($node) is not a valid node type", f),
        );
        return Ok(Value::Null);
    }
    Ok(node_obj(vm, n.unwrap()))
}

#[allow(dead_code)]
fn unused() -> Value {
    b(false)
}
