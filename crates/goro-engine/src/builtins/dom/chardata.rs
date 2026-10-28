//! DOMCharacterData / Dom\CharacterData, DOMText, DOMAttr extras, and the
//! DTD views (DOMDocumentType's entities and notations, external subset
//! loading for `validate()`). Offsets count characters (code points), as
//! PHP does for both APIs (observed on PHP 8.5).

use goro_runtime::Value;
use goro_xml::NodeId;
use goro_xml::tree::{DomError, NodeData, QName};

use super::super::libxml::PhpLoader;
use super::xmlstore;
use super::{legacy_error, modern_fn, s8, sv, this_node, throw_dom, with_arena, wrap_node};
use super::document::NameCheck as NC;
use crate::func::{Args, NativeFn};
use crate::vm::{R, Vm};

pub const METHODS: &[(&str, NativeFn)] = &[
    ("domcharacterdata::substringdata", substring_data),
    ("domcharacterdata::appenddata", append_data),
    ("domcharacterdata::insertdata", insert_data),
    ("domcharacterdata::deletedata", delete_data),
    ("domcharacterdata::replacedata", replace_data),
    ("dom\\characterdata::substringdata", substring_data),
    ("dom\\characterdata::appenddata", append_data),
    ("dom\\characterdata::insertdata", insert_data),
    ("dom\\characterdata::deletedata", delete_data),
    ("dom\\characterdata::replacedata", replace_data),
    ("domtext::splittext", split_text),
    ("dom\\text::splittext", split_text),
    ("domtext::iswhitespaceinelementcontent", is_ws),
    ("domtext::iselementcontentwhitespace", is_ws),
    ("domattr::isid", is_id),
    ("dom\\attr::isid", is_id),
    ("dom\\attr::rename", attr_rename),
    ("dom\\element::rename", attr_rename),
];

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

fn data_of(n: NodeId) -> String {
    with_arena(|ar| ar.char_data(n).unwrap_or("").to_string())
}

/// Sets character data.
pub(crate) fn set_data(n: NodeId, text: &str) {
    with_arena(|ar| {
        if let Some(d) = ar.char_data_mut(n) {
            *d = text.to_string();
        }
    });
}

/// Checks `offset` against the length: IndexSizeError when past the end
/// or negative.
fn check_offset(vm: &mut Vm, a: &Args, n: NodeId, off: i64, len: usize) -> R<Option<usize>> {
    if off < 0 || off as usize > len {
        if modern_fn(a) {
            return throw_dom(vm, DomError::IndexSize);
        }
        legacy_error(vm, a, n, DomError::IndexSize)?;
        return Ok(None);
    }
    Ok(Some(off as usize))
}

fn substring_data(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let c = chars(&data_of(n));
    let Some(off) = check_offset(vm, a, n, a.int(0), c.len())? else {
        return Ok(Value::Bool(false));
    };
    let count = a.int(1);
    if count < 0 && !modern_fn(a) {
        legacy_error(vm, a, n, DomError::IndexSize)?;
        return Ok(Value::Bool(false));
    }
    let end = if count < 0 {
        c.len()
    } else {
        (off + count as usize).min(c.len())
    };
    Ok(sv(&c[off..end].iter().collect::<String>()))
}

fn append_data(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let mut d = data_of(n);
    d.push_str(&s8(&a.str(0)));
    set_data(n, &d);
    if modern_fn(a) {
        return Ok(Value::Null);
    }
    Ok(Value::Bool(true))
}

fn edit(vm: &mut Vm, a: &Args, off: i64, count: i64, insert: &str) -> R<Value> {
    let n = this_node(vm, a)?;
    let mut c = chars(&data_of(n));
    let Some(off) = check_offset(vm, a, n, off, c.len())? else {
        return Ok(Value::Bool(false));
    };
    if count < 0 && !modern_fn(a) {
        legacy_error(vm, a, n, DomError::IndexSize)?;
        return Ok(Value::Bool(false));
    }
    let end = if count < 0 {
        c.len()
    } else {
        (off + count as usize).min(c.len())
    };
    c.splice(off..end, insert.chars());
    set_data(n, &c.iter().collect::<String>());
    if modern_fn(a) {
        return Ok(Value::Null);
    }
    Ok(Value::Bool(true))
}

fn insert_data(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = s8(&a.str(1));
    edit(vm, a, a.int(0), 0, &s)
}

fn delete_data(vm: &mut Vm, a: &mut Args) -> R<Value> {
    edit(vm, a, a.int(0), a.int(1), "")
}

fn replace_data(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = s8(&a.str(2));
    edit(vm, a, a.int(0), a.int(1), &s)
}

fn split_text(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let c = chars(&data_of(n));
    let off = a.int(0);
    if off < 0 || off as usize > c.len() {
        if modern_fn(a) {
            return throw_dom(vm, DomError::IndexSize);
        }
        return legacy_error(vm, a, n, DomError::IndexSize);
    }
    let off = off as usize;
    let head: String = c[..off].iter().collect();
    let tail: String = c[off..].iter().collect();
    let t = with_arena(|ar| {
        let doc = ar.doc_of(n);
        // Even a CDATA section splits off a text node (test bug52656.phpt).
        let data = NodeData::Text(tail);
        let t = ar.create(doc, data);
        if let Some(d) = ar.char_data_mut(n) {
            *d = head;
        }
        if let Some(p) = ar.parent(n) {
            let next = ar.next_sibling(n);
            ar.raw_insert_before(p, t, next);
        }
        t
    });
    wrap_node(vm, t)
}

fn is_ws(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let d = data_of(n);
    Ok(Value::Bool(
        d.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r')),
    ))
}

/// `wholeText`: the text of the contiguous text nodes around `n`.
pub(crate) fn whole_text(ar: &goro_xml::Arena, n: NodeId) -> String {
    let mut first = n;
    while let Some(p) = ar.prev_sibling(first) {
        if !ar.is_text(p) {
            break;
        }
        first = p;
    }
    let mut s = String::new();
    let mut c = Some(first);
    while let Some(x) = c {
        if !ar.is_text(x) {
            break;
        }
        s.push_str(ar.char_data(x).unwrap_or(""));
        c = ar.next_sibling(x);
    }
    s
}

fn is_id(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    Ok(Value::Bool(with_arena(|ar| {
        ar.attr(n).is_some_and(|d| d.is_id)
            || (ar.qname(n).is_some_and(|q| q.is("xml:id")))
            || (super::is_modern_node(ar, n)
                && ar.qname(n).is_some_and(|q| q.ns.is_none() && &*q.local == "id"))
    })))
}

/// `Dom\Element::rename()` / `Dom\Attr::rename()`.
fn attr_rename(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = this_node(vm, a)?;
    let ns = a.opt_str(0).map(|s| s8(&s));
    let qn = s8(&a.str(1));
    let q = match super::document::validate_and_extract(ns.as_deref(), &qn, NC::Modern) {
        Ok(q) => q,
        Err(e) => return throw_dom(vm, e),
    };
    let r = with_arena(|ar| -> Result<(), String> {
        let is_attr = matches!(ar.data(n), NodeData::Attribute(_));
        if is_attr {
            if let Some(el) = ar.parent(n)
                && let Some(other) = ar.get_attr_node_ns(el, q.ns.as_deref(), &q.local)
                && other != n
            {
                return Err("An attribute with the given name in the given namespace already exists".into());
            }
            ar.attr_mut(n).unwrap().name = q;
        } else {
            let html = super::props::html_upper(ar, n);
            let is_template = ar.element(n).is_some_and(|e| e.template_contents.is_some());
            if html && is_template {
                return Err("It is not possible to rename the template element because it hosts a document fragment".into());
            }
            ar.element_mut(n).unwrap().name = QName { ..q };
        }
        ar.touch();
        Ok(())
    });
    match r {
        Ok(()) => Ok(Value::Null),
        Err(m) => super::throw_dom_msg(vm, &m, 11),
    }
}

// ---------------------------------------------------------------- DTDs

/// Per-document cache of the entity and notation nodes made for
/// DOMDocumentType::$entities / $notations.
#[derive(Default)]
pub struct DtdNodes {
    pub map: Vec<((NodeId, bool, String), NodeId)>,
}

/// The entity (or notation) nodes of a doctype, in declaration order.
pub(crate) fn dtd_nodes(s: &mut xmlstore::Store, dt: NodeId, entities: bool) -> Vec<NodeId> {
    let doc = s.arena.doc_of(dt);
    let subsets: Vec<NodeId> = {
        let mut v = vec![dt];
        if let Some(e) = s.arena.doc(doc).ext_subset
            && e != dt
        {
            v.push(e);
        }
        v
    };
    let mut names: Vec<(String, NodeData)> = Vec::new();
    for sub in subsets {
        let Some(d) = s.arena.dtd(sub) else {
            continue;
        };
        if entities {
            let mut seen: Vec<String> = Vec::new();
            for it in &d.items {
                if let goro_xml::tree::DtdItem::Entity(n, false) = it
                    && let Some(e) = d.entities.get(n)
                    && !seen.contains(&n.to_string())
                {
                    seen.push(n.to_string());
                    names.push((n.to_string(), NodeData::EntityDecl(Box::new(e.clone()))));
                }
            }
            for (n, e) in &d.entities {
                if !seen.contains(&n.to_string()) {
                    names.push((n.to_string(), NodeData::EntityDecl(Box::new(e.clone()))));
                }
            }
        } else {
            for it in &d.items {
                if let goro_xml::tree::DtdItem::Notation(n) = it
                    && let Some(x) = d.notations.get(n)
                {
                    names.push((n.to_string(), NodeData::Notation(Box::new(x.clone()))));
                }
            }
        }
    }
    let mut out = Vec::new();
    for (name, data) in names {
        let key = (dt, entities, name);
        let existing = s
            .doc_ext(dt)
            .get::<DtdNodes>()
            .map
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| *v)
            .filter(|&v| s.arena.is_live(v));
        let id = match existing {
            Some(v) => v,
            None => {
                let id = s.arena.create(doc, data);
                s.arena.node_mut(id).parent = Some(dt);
                s.doc_ext(dt).get::<DtdNodes>().map.push((key, id));
                id
            }
        };
        out.push(id);
    }
    out
}

/// Loads and attaches the external subset of document `d` (for
/// `validate()` on a document parsed without it). False on failure.
pub(crate) fn load_external_subset(
    vm: &mut Vm,
    d: NodeId,
    public: Option<&str>,
    system: &str,
    base: Option<&str>,
    loader: &mut PhpLoader,
) -> bool {
    let _ = vm;
    let ctx = with_arena(|ar| {
        let dt = ar.doc(d).doctype;
        goro_xml::xml::LoadContext {
            int_sub_name: dt.and_then(|t| ar.dtd(t)).map(|x| x.name.to_string()),
            ext_sub_uri: Some(system.to_string()),
            ext_sub_system: public.map(str::to_string),
        }
    });
    let bytes = match goro_xml::xml::EntityLoader::load_with(loader, public, system, base, &ctx) {
        Ok(b) => b,
        Err(_) => return false,
    };
    with_arena(|ar| goro_xml::xml::attach_external_subset(ar, d, &bytes, public, system))
}
