//! xsltApplyStylesheetInternal: running a transformation.

use crate::transform::{OutType, State, Transform};
use crate::util::*;
use crate::xpath;
use goro_ext_xml::tree::{NType, NodeId};

/// xsltHTMLVersions
pub fn html_ids_pub(version: &str) -> Option<(Option<&'static str>, Option<&'static str>)> {
    html_ids(version)
}

fn html_ids(version: &str) -> Option<(Option<&'static str>, Option<&'static str>)> {
    let table: [(&str, Option<&str>, Option<&str>); 10] = [
        ("5", None, Some("about:legacy-compat")),
        (
            "4.01frame",
            Some("-//W3C//DTD HTML 4.01 Frameset//EN"),
            Some("http://www.w3.org/TR/1999/REC-html401-19991224/frameset.dtd"),
        ),
        (
            "4.01strict",
            Some("-//W3C//DTD HTML 4.01//EN"),
            Some("http://www.w3.org/TR/1999/REC-html401-19991224/strict.dtd"),
        ),
        (
            "4.01trans",
            Some("-//W3C//DTD HTML 4.01 Transitional//EN"),
            Some("http://www.w3.org/TR/1999/REC-html401-19991224/loose.dtd"),
        ),
        (
            "4.01",
            Some("-//W3C//DTD HTML 4.01 Transitional//EN"),
            Some("http://www.w3.org/TR/1999/REC-html401-19991224/loose.dtd"),
        ),
        (
            "4.0strict",
            Some("-//W3C//DTD HTML 4.01//EN"),
            Some("http://www.w3.org/TR/html4/strict.dtd"),
        ),
        (
            "4.0trans",
            Some("-//W3C//DTD HTML 4.01 Transitional//EN"),
            Some("http://www.w3.org/TR/html4/loose.dtd"),
        ),
        (
            "4.0frame",
            Some("-//W3C//DTD HTML 4.01 Frameset//EN"),
            Some("http://www.w3.org/TR/html4/frameset.dtd"),
        ),
        (
            "4.0",
            Some("-//W3C//DTD HTML 4.01 Transitional//EN"),
            Some("http://www.w3.org/TR/html4/loose.dtd"),
        ),
        ("3.2", Some("-//W3C//DTD HTML 3.2//EN"), None),
    ];
    for (v, p, s) in table {
        if v.eq_ignore_ascii_case(version) {
            return Some((p, s));
        }
    }
    None
}

/// htmlNewDoc(URI, publicId)
fn html_new_doc(system: Option<&str>, public: Option<&str>) -> NodeId {
    tw(|t| {
        let d = t.new_html_doc();
        let (sys, publ) = if system.is_none() && public.is_none() {
            (
                Some("http://www.w3.org/TR/REC-html40/loose.dtd"),
                Some("-//W3C//DTD HTML 4.0 Transitional//EN"),
            )
        } else {
            (system, public)
        };
        if sys.is_some() || publ.is_some() {
            t.create_int_subset(Some(d), "html", publ, sys);
        }
        d
    })
}

impl<'v> Transform<'v> {
    /// xsltApplyStylesheetInternal: returns the result document, or None.
    pub fn apply_stylesheet(&mut self, doc: NodeId, params: &[(String, String)]) -> Option<NodeId> {
        // avoid hitting the DTD when scanning nodes (it stays the internal
        // subset)
        let int_subset = tw(|t| t.doc_info(doc).and_then(|i| i.int_subset));
        if let Some(dtd) = int_subset {
            tw(|t| {
                if t.nodes[dtd].parent.is_some() {
                    let parent = t.nodes[dtd].parent;
                    let (prev, next) = (t.nodes[dtd].prev, t.nodes[dtd].next);
                    if let Some(n) = next {
                        t.nodes[n].prev = prev;
                    }
                    if let Some(p) = prev {
                        t.nodes[p].next = next;
                    }
                    if let Some(par) = parent {
                        if t.nodes[par].children == Some(dtd) {
                            t.nodes[par].children = next;
                        }
                        if t.nodes[par].last == Some(dtd) {
                            t.nodes[par].last = prev;
                        }
                    }
                    t.nodes[dtd].prev = None;
                    t.nodes[dtd].next = None;
                }
            });
        }
        xpath::invalidate_order();
        self.initial_context_doc = doc;
        self.initial_context_node = doc;
        let sheet = self.sheet.clone();
        let method = sheet.get_import_str(|s| s.method.as_ref());
        let mut doctype_public = sheet.get_import_str(|s| s.doctype_public.as_ref());
        let mut doctype_system = sheet.get_import_str(|s| s.doctype_system.as_ref());
        let version = sheet.get_import_str(|s| s.version.as_ref());
        let encoding = sheet.get_import_str(|s| s.encoding.as_ref());
        let style_version = sheet.styles[0].version.clone();
        let res: NodeId;
        match method.as_deref() {
            Some(m) if m != "xml" => {
                if m == "html" {
                    self.out_type = OutType::Html;
                    if doctype_public.is_some() || doctype_system.is_some() {
                        res = html_new_doc(doctype_system.as_deref(), doctype_public.as_deref());
                    } else if version.is_none() {
                        res = tw(|t| t.new_html_doc());
                    } else {
                        if let Some((p, s)) = html_ids(version.as_deref().unwrap()) {
                            doctype_public = p.map(|x| x.to_string());
                            doctype_system = s.map(|x| x.to_string());
                        }
                        res = html_new_doc(doctype_system.as_deref(), doctype_public.as_deref());
                    }
                } else if m == "xhtml" {
                    self.error(
                        Some(doc),
                        "xsltApplyStylesheetInternal: unsupported method xhtml, using html\n",
                    );
                    self.out_type = OutType::Html;
                    res = html_new_doc(doctype_system.as_deref(), doctype_public.as_deref());
                } else if m == "text" {
                    self.out_type = OutType::Text;
                    res = tw(|t| t.new_doc(style_version.as_deref()));
                } else {
                    self.error(
                        Some(doc),
                        &format!("xsltApplyStylesheetInternal: unsupported method ({})\n", m),
                    );
                    return None;
                }
            }
            _ => {
                self.out_type = OutType::Xml;
                res = tw(|t| t.new_doc(style_version.as_deref()));
            }
        }
        if let Some(e) = &encoding {
            tw(|t| {
                if let Some(i) = t.doc_info_mut(res) {
                    i.encoding = Some(e.clone());
                }
            });
        }
        self.node = doc;
        self.output = res;
        self.xp.size = 1;
        self.xp.pos = 1;
        self.xp.node = None;
        self.xp.doc = Some(doc);
        if self.need_elem_space_handling() {
            if let Some(root) = tw(|t| t.root_element(doc)) {
                self.apply_strip_spaces(root);
            }
        }
        for (k, v) in params {
            if self.quote_user_param(k, v) < 0 {
                let f = goro_ext_xml::dom::active_fn(self.vm);
                self.vm
                    .emit_warning(&format!("{}(): Could not apply parameter \"{}\"", f, k));
                if self.vm.current_exception.is_some() {
                    return None;
                }
                break;
            }
        }
        // xsltCountKeys
        self.has_templ_key_patterns = sheet
            .import_order()
            .iter()
            .any(|&s| !sheet.styles[s].key_match.is_empty());
        self.nb_keys = sheet
            .import_order()
            .iter()
            .map(|&s| sheet.styles[s].keys.len())
            .sum();
        self.eval_global_variables();
        self.insert = Some(res);
        self.vars_base = self.vars.len().saturating_sub(1);
        if self.vars.is_empty() {
            self.vars_base = 0;
        }
        let n = self.node;
        self.process_one_node(n, &[]);
        self.vars.clear();
        // post processing
        let root = tw(|t| t.root_element(res));
        if let Some(root) = root {
            let doctype = tw(|t| match t.ns_prefix(t.nodes[root].ns) {
                Some(p) if t.nodes[root].ns.is_some() => format!("{}:{}", p, t.nodes[root].name),
                _ => t.nodes[root].name.clone(),
            });
            let root_is_html = tw(|t| {
                t.nodes[root].ns.is_none() && t.nodes[root].name.eq_ignore_ascii_case("html")
            });
            if method.is_none() && root_is_html {
                let only_blank_before = tw(|t| {
                    let mut c = t.nodes[res].children;
                    while let Some(x) = c {
                        if x == root {
                            return true;
                        }
                        if t.nodes[x].t == NType::Element {
                            return false;
                        }
                        if t.nodes[x].t == NType::Text && !is_blank(&t.nodes[x].content) {
                            return false;
                        }
                        c = t.nodes[x].next;
                    }
                    false
                });
                if only_blank_before {
                    self.out_type = OutType::Html;
                    tw(|t| {
                        t.nodes[res].t = NType::HtmlDocument;
                        if let Some(i) = t.doc_info_mut(res) {
                            i.html = true;
                        }
                    });
                    if doctype_public.is_some() || doctype_system.is_some() {
                        let (dp, ds) = (doctype_public.clone(), doctype_system.clone());
                        tw(|t| {
                            t.create_int_subset(Some(res), &doctype, dp.as_deref(), ds.as_deref())
                        });
                    } else if let Some(v) = &version {
                        if let Some((p, s)) = html_ids(v) {
                            if p.is_some() || s.is_some() {
                                tw(|t| t.create_int_subset(Some(res), &doctype, p, s));
                            }
                        }
                    }
                }
            }
            if self.out_type == OutType::Xml {
                let dp = sheet.get_import_str(|s| s.doctype_public.as_ref());
                let ds = sheet.get_import_str(|s| s.doctype_system.as_ref());
                if dp.is_some() || ds.is_some() {
                    tw(|t| {
                        let dtd = t.create_int_subset(None, &doctype, dp.as_deref(), ds.as_deref());
                        t.nodes[dtd].doc = Some(res);
                        if let Some(i) = t.doc_info_mut(res) {
                            i.int_subset = Some(dtd);
                        }
                        // the DTD comes first
                        match t.nodes[res].children {
                            Some(first) => t.insert_before_raw(first, dtd),
                            None => t.link_child_raw(res, dtd),
                        }
                        t.nodes[dtd].parent = Some(res);
                    });
                }
            }
        }
        if self.state != State::Ok {
            return None;
        }
        Some(res)
    }
}
