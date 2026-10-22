//! gdoc.c: clean up HTML exported from Google Docs.

use crate::attrs;
use crate::doc::*;
use crate::enums::TagId;
use crate::lexer;
use crate::parser;
use crate::tags;

/// DiscardContainer(): extricate "element", replace it by its content and
/// delete it; returns the node to continue with.
fn discard_container(doc: &mut Doc, element: NodeId) -> Option<NodeId> {
    if let Some(content) = doc[element].content {
        let parent = doc[element].parent;
        let last = doc[element].last;
        let enext = doc[element].next;
        if let Some(l) = last {
            doc[l].next = enext;
        }
        match enext {
            Some(n) => doc[n].prev = last,
            None => {
                if let Some(p) = parent {
                    doc[p].last = last;
                }
            }
        }
        match doc[element].prev {
            Some(prev) => {
                doc[content].prev = Some(prev);
                doc[prev].next = Some(content);
            }
            None => {
                if let Some(p) = parent {
                    doc[p].content = Some(content);
                }
            }
        }
        let mut n = Some(content);
        while let Some(x) = n {
            doc[x].parent = parent;
            n = doc[x].next;
        }
        doc[element].next = None;
        doc[element].content = None;
        lexer::free_node(doc, Some(element));
        Some(content)
    } else {
        parser::discard_element(doc, element)
    }
}

/// CleanNode()
fn clean_node(doc: &mut Doc, node: NodeId) {
    let mut child = doc[node].content;
    while let Some(c) = child {
        let mut next = doc[c].next;
        if tags::node_is_element(doc, Some(c)) {
            if doc.is(c, TagId::STYLE) {
                parser::discard_element(doc, c);
            }
            if doc.is(c, TagId::P) && doc[c].content.is_none() {
                parser::discard_element(doc, c);
            } else if doc.is(c, TagId::SPAN) {
                next = discard_container(doc, c);
            } else if doc.is(c, TagId::A) && doc[c].content.is_none() {
                let mut id = attrs::get_attr_by_name(doc, c, b"name");
                // Recent Google Docs is using "id" instead of "name"
                if id.is_none() {
                    id = attrs::get_attr_by_name(doc, c, b"id");
                }
                if let Some(id) = id {
                    let v = doc[id].value.clone();
                    if let Some(parent) = doc[c].parent {
                        attrs::repair_attr_value(doc, parent, b"id", v.as_deref());
                    }
                }
                parser::discard_element(doc, c);
            } else {
                if doc[c].attributes.is_some() {
                    attrs::drop_attr_by_name(doc, c, b"class");
                }
                clean_node(doc, c);
            }
        }
        child = next;
    }
}

/// SetUTF8(): insert meta element to force browser to recognize doc as UTF8
fn set_utf8(doc: &mut Doc) {
    if let Some(head) = lexer::find_head(doc) {
        let node = lexer::inferred_tag(doc, TagId::META);
        attrs::add_attribute(doc, node, b"http-equiv", b"Content-Type");
        attrs::add_attribute(doc, node, b"content", b"text/html; charset=UTF-8");
        parser::insert_node_at_start(doc, head, node);
    }
}

/// CleanGoogleDocument(): clean html exported by Google Docs
pub fn clean_google_document(doc: &mut Doc) {
    let root = doc.root;
    clean_node(doc, root);
    set_utf8(doc);
}
