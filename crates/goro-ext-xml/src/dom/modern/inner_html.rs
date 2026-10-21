//! Element::$innerHTML / $outerHTML for XML documents
//! (inner_outer_html_mixin.c): XML serialization with the well-formed flag
//! and the XML fragment parsing algorithm.

use super::*;
use crate::dom::node::str_of;

fn is_html_doc(n: NodeId) -> bool {
    tree::with(|t| {
        t.nodes[n]
            .doc
            .map(|d| t.nodes[d].t == NType::HtmlDocument)
            .unwrap_or(false)
    })
}

fn not_well_formed(vm: &mut Vm) {
    dom_err_msg(
        vm,
        SYNTAX_ERR,
        "The resulting XML serialization is not well-formed",
    );
}

/// dom_element_html_fragment_serialize for the children of `n`.
fn fragment_serialize(vm: &mut Vm, children: &[NodeId]) -> R<String> {
    let r = tree::with(|t| {
        let mut out = String::new();
        for &c in children {
            out.push_str(&serializer::serialize(
                t,
                c,
                false,
                true,
                false,
                false,
                Some("UTF-8"),
            )?);
        }
        Ok::<String, ()>(out)
    });
    match r {
        Ok(s_) => Ok(s_),
        Err(()) => {
            not_well_formed(vm);
            Err(())
        }
    }
}

pub fn r_inner_html(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    if is_html_doc(n) {
        return Ok(s(&super::html_serializer::serialize_children(n)));
    }
    let kids = tree::with(|t| t.children_of(n));
    Ok(s(&fragment_serialize(vm, &kids)?))
}

pub fn r_outer_html(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    if is_html_doc(n) {
        return Ok(s(&super::html_serializer::serialize_node(n)));
    }
    Ok(s(&fragment_serialize(vm, &[n])?))
}

/// The XML fragment parsing algorithm: parse `input` in the context of
/// `context` and return a new document fragment (owned by the context's
/// document) with the result. Throws a SyntaxError if not well-formed.
pub fn parse_fragment(vm: &mut Vm, context: NodeId, input: &str) -> Option<NodeId> {
    if is_html_doc(context) {
        return super::html_serializer::parse_fragment(vm, context, input);
    }
    let (doc, wrapper) = tree::with(|t| {
        let doc = doc_of_ctx(t, context);
        let mut w = String::new();
        let tag = t.qname(context);
        w.push('<');
        w.push_str(&tag);
        for ns in in_scope_ns(t, doc, context, true) {
            w.push_str(" xmlns:");
            w.push_str(t.nss[ns].prefix.as_deref().unwrap_or(""));
            w.push_str("=\"");
            w.push_str(&t.nss[ns].href);
            w.push('"');
        }
        if let Some(def) = node::locate_namespace(t, context, None) {
            w.push_str(" xmlns=\"");
            w.push_str(&def);
            w.push('"');
        }
        w.push('>');
        w.push_str(input);
        w.push_str("</");
        w.push_str(&tag);
        w.push('>');
        (doc, w)
    });
    let opts = crate::parser::XML_PARSE_IGNORE_ENC
        | crate::parser::XML_PARSE_NOERROR
        | crate::parser::XML_PARSE_NOWARNING;
    let out = crate::sax2::parse_doc(wrapper.as_bytes(), None, opts);
    let pdoc = match out.doc {
        Some(d) if out.well_formed && out.ns_well_formed => d,
        _ => {
            dom_err_msg(vm, SYNTAX_ERR, "XML fragment is not well-formed");
            return None;
        }
    };
    let r = tree::with(|t| {
        let de = t.nodes[pdoc].children?;
        if t.nodes[de].next.is_some() || t.nodes[de].t != NType::Element {
            return None;
        }
        // the document element's declarations become attributes too
        // (dom_mark_namespaces_as_attributes_too on the parsed document)
        compat_mark_attribute_list(t, doc, de);
        let frag = t.new_fragment(Some(doc));
        let kids = t.children_of(de);
        for c in kids {
            t.unlink(c);
            t.set_tree_doc(c, Some(doc));
            t.link_child_raw(frag, c);
        }
        let mut cur = t.nodes[frag].children;
        while let Some(x) = cur {
            if t.nodes[x].t == NType::Element {
                compat_mark_attribute_list(t, doc, x);
            }
            cur = next_in_tree_order(t, x, Some(frag));
        }
        Some(frag)
    });
    if r.is_none() {
        dom_err_msg(vm, SYNTAX_ERR, "XML fragment is not well-formed");
    }
    r
}

fn doc_of_ctx(t: &Tree, n: NodeId) -> NodeId {
    doc_from_node(t, n).unwrap_or(n)
}

pub fn w_inner_html(vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let st = str_of(v);
    let frag = match parse_fragment(vm, n, &st) {
        Some(f) => f,
        None => return Err(()),
    };
    tree::with(|t| crate::dom::node::remove_all_children(t, n));
    if node::pre_insert(vm, Some(frag), n, None) {
        Ok(())
    } else {
        Err(())
    }
}

pub fn w_outer_html(vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let st = str_of(v);
    let parent = match tree::with(|t| t.nodes[n].parent) {
        Some(p) => p,
        None => return Ok(()),
    };
    if tree::with(|t| t.is_doc(parent)) {
        dom_err(vm, INVALID_MODIFICATION_ERR);
        return Err(());
    }
    let ctx = if tree::with(|t| t.nodes[parent].t == NType::DocumentFrag) {
        tree::with(|t| {
            let d = doc_of_ctx(t, n);
            let ns = mapper_get_ns(t, d, None, Some(XHTML_NS));
            let e = t.new_element(Some(d), "body");
            t.nodes[e].ns = ns;
            e
        })
    } else {
        parent
    };
    let frag = match parse_fragment(vm, ctx, &st) {
        Some(f) => f,
        None => return Err(()),
    };
    if !node::pre_insert(vm, Some(frag), parent, Some(n)) {
        return Err(());
    }
    tree::with(|t| t.unlink(n));
    Ok(())
}
