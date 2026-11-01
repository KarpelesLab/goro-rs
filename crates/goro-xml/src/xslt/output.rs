//! Writing a result tree (XSLT 1.0 section 16).

use super::compile::{OutMethod, OutputSpec};
use crate::html::{self, HtmlSaveOptions};
use crate::serialize::{self, SaveOptions};
use crate::tree::{Arena, DocKind, DtdData, NodeData, NodeId, QName};

/// The method in effect: the one asked for, else what the result looks like
/// (section 16: `html` when the first element is `html` without namespace
/// and only whitespace comes before it).
pub fn effective_method(arena: &Arena, doc: NodeId, spec: &OutputSpec) -> OutMethod {
    if let Some(m) = spec.method {
        return m;
    }
    for c in arena.children(doc) {
        match arena.data(c) {
            NodeData::Element(e) => {
                return if e.name.local.eq_ignore_ascii_case("html") && e.name.ns.is_none() {
                    OutMethod::Html
                } else {
                    OutMethod::Xml
                };
            }
            NodeData::Text(t) | NodeData::CData(t) => {
                if !t.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r')) {
                    return OutMethod::Xml;
                }
            }
            _ => {}
        }
    }
    OutMethod::Xml
}

/// Gives the result document what `xsl:output` says it has: its kind and
/// the document type declaration.
pub fn finish_result(arena: &mut Arena, doc: NodeId, spec: &OutputSpec) {
    let method = effective_method(arena, doc, spec);
    if method == OutMethod::Html {
        arena.doc_mut(doc).kind = DocKind::Html;
    }
    if method == OutMethod::Text {
        return;
    }
    if spec.doctype_public.is_none() && spec.doctype_system.is_none() {
        return;
    }
    let root_name = arena
        .children(doc)
        .find_map(|c| arena.element(c).map(|e| e.name.qualified()));
    let Some(root_name) = root_name else {
        return;
    };
    let dt = DtdData {
        name: root_name.as_str().into(),
        public_id: spec.doctype_public.clone(),
        system_id: spec.doctype_system.clone(),
        ..Default::default()
    };
    let n = arena.create(doc, NodeData::Doctype(Box::new(dt)));
    let first = arena.first_child(doc);
    arena.raw_insert_before(doc, n, first);
    arena.doc_mut(doc).doctype = Some(n);
}

fn collect_text(arena: &Arena, n: NodeId, out: &mut String) {
    for c in arena.children(n) {
        match arena.data(c) {
            NodeData::Text(t) | NodeData::CData(t) => out.push_str(t),
            NodeData::Element(_) => collect_text(arena, c, out),
            _ => {}
        }
    }
}

/// Section 16.2: `<meta>` after the `<head>` start tag, unless present.
fn add_html_meta(arena: &mut Arena, doc: NodeId, encoding: &str) {
    let html_el = arena.children(doc).find(|&c| {
        arena
            .element(c)
            .is_some_and(|e| e.name.local.eq_ignore_ascii_case("html"))
    });
    let Some(html_el) = html_el else { return };
    let head = arena.children(html_el).find(|&c| {
        arena
            .element(c)
            .is_some_and(|e| e.name.local.eq_ignore_ascii_case("head"))
    });
    let Some(head) = head else { return };
    let has_meta = arena.children(head).any(|c| {
        arena
            .element(c)
            .is_some_and(|e| e.name.local.eq_ignore_ascii_case("meta"))
            && arena
                .get_attribute(c, "http-equiv")
                .is_some_and(|v| v.eq_ignore_ascii_case("content-type"))
    });
    if has_meta {
        return;
    }
    let meta = arena.new_element(doc, QName::local("meta"));
    let a = arena.new_attribute(doc, QName::local("http-equiv"), "Content-Type");
    arena.raw_append_attr(meta, a);
    let value = format!("text/html; charset={encoding}");
    let b = arena.new_attribute(doc, QName::local("content"), &value);
    arena.raw_append_attr(meta, b);
    let first = arena.first_child(head);
    arena.raw_insert_before(head, meta, first);
}

/// The bytes of a result document.
pub fn serialize(arena: &mut Arena, doc: NodeId, spec: &OutputSpec) -> Vec<u8> {
    match effective_method(arena, doc, spec) {
        OutMethod::Text => {
            let mut s = String::new();
            collect_text(arena, doc, &mut s);
            serialize::encode_output(&s, Some(spec.encoding.as_deref().unwrap_or("UTF-8")))
        }
        OutMethod::Html => {
            let enc = spec.encoding.clone();
            add_html_meta(arena, doc, enc.as_deref().unwrap_or("UTF-8"));
            // html output indents unless told otherwise (section 16.2).
            let format = spec.indent != Some(false);
            html::save_html(arena, doc, HtmlSaveOptions { format })
        }
        OutMethod::Xml | OutMethod::Xhtml => xml_output(arena, doc, spec),
    }
}

fn xml_output(arena: &Arena, doc: NodeId, spec: &OutputSpec) -> Vec<u8> {
    let d = arena.doc(doc);
    let mut s = String::new();
    if spec.omit_xml_declaration != Some(true) {
        s.push_str("<?xml version=\"");
        s.push_str(d.version.as_deref().unwrap_or("1.0"));
        s.push('"');
        if let Some(e) = &spec.encoding {
            s.push_str(" encoding=\"");
            s.push_str(e);
            s.push('"');
        }
        match spec.standalone {
            Some(true) => s.push_str(" standalone=\"yes\""),
            Some(false) => s.push_str(" standalone=\"no\""),
            None => {}
        }
        s.push_str("?>\n");
    }
    let opts = SaveOptions {
        format: spec.indent == Some(true),
        ..SaveOptions::default()
    };
    let mut any = false;
    for c in arena.children(doc) {
        any = true;
        if let NodeData::Doctype(dt) = arena.data(c) {
            s.push_str("<!DOCTYPE ");
            s.push_str(&dt.name);
            match (&dt.public_id, &dt.system_id) {
                (Some(p), Some(u)) => s.push_str(&format!(" PUBLIC \"{p}\" \"{u}\"")),
                (Some(p), None) => s.push_str(&format!(" PUBLIC \"{p}\"")),
                (None, Some(u)) => s.push_str(&format!(" SYSTEM \"{u}\"")),
                (None, None) => {}
            }
            s.push_str(">\n");
            continue;
        }
        s.push_str(&serialize::to_string(arena, c, &opts));
    }
    // A final newline, unless `indent="no"` was asked for.
    if any && spec.indent != Some(false) {
        s.push('\n');
    }
    serialize::encode_output(&s, spec.encoding.as_deref())
}
