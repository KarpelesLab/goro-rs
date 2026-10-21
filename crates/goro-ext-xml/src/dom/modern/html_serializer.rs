//! The HTML fragment serialization algorithm (html5_serializer.c), used by
//! saveHtml and innerHTML/outerHTML of Dom\HTMLDocument nodes, and the
//! HTML fragment parsing entry point.

use super::*;

/// The serializer output: the string and, when requested, the end offset
/// of every non-empty write call (saveHtmlFile writes them one by one).
#[derive(Default)]
pub struct Out {
    pub s: String,
    pub cuts: Option<Vec<usize>>,
}

impl Out {
    /// write_string_len
    fn w(&mut self, x: &str) {
        if x.is_empty() {
            return;
        }
        self.s.push_str(x);
        if let Some(c) = &mut self.cuts {
            c.push(self.s.len());
        }
    }

    /// write_string: C strings stop at a NUL byte
    fn ws(&mut self, x: &str) {
        self.w(cstr(x));
    }

    /// The pieces in write order.
    pub fn pieces(&self) -> Vec<&str> {
        let mut v = Vec::new();
        let mut last = 0;
        for &c in self.cuts.iter().flatten() {
            v.push(&self.s[last..c]);
            last = c;
        }
        v
    }
}

fn cstr(x: &str) -> &str {
    match x.find('\0') {
        Some(i) => &x[..i],
        None => x,
    }
}

/// dom_html5_escape_string
fn escape(out: &mut Out, s_: &str, attribute_mode: bool) {
    let s_ = cstr(s_);
    let mut last = 0;
    for (i, c) in s_.char_indices() {
        let rep = match c {
            '&' => "&amp;",
            '\u{A0}' => "&nbsp;",
            '"' if attribute_mode => "&quot;",
            '<' if !attribute_mode => "&lt;",
            '>' if !attribute_mode => "&gt;",
            _ => continue,
        };
        out.w(&s_[last..i]);
        out.w(rep);
        last = i + c.len_utf8();
    }
    out.w(&s_[last..]);
}

fn is_html(t: &Tree, n: NodeId) -> bool {
    ns_is(t, t.nodes[n].ns, XHTML_NS)
}

fn serializes_as_void(t: &Tree, n: NodeId) -> bool {
    is_html(t, n)
        && matches!(
            t.nodes[n].name.as_str(),
            "area"
                | "base"
                | "br"
                | "col"
                | "embed"
                | "hr"
                | "img"
                | "input"
                | "link"
                | "meta"
                | "source"
                | "track"
                | "wbr"
                | "basefont"
                | "bgsound"
                | "frame"
                | "keygen"
                | "param"
        )
}

fn tag_name(t: &Tree, out: &mut Out, n: NodeId) {
    if let Some(ns) = t.nodes[n].ns {
        if let Some(p) = &t.nss[ns].prefix {
            let h = &t.nss[ns].href;
            if h != XHTML_NS && h != MATHML_NS && h != SVG_NS {
                out.ws(p);
                out.w(":");
            }
        }
    }
    out.ws(&t.nodes[n].name);
}

fn attr_name(t: &Tree, out: &mut Out, a: NodeId) {
    let an = &t.nodes[a];
    match an.ns {
        None => out.ws(&an.name),
        Some(ns) => {
            let h = t.nss[ns].href.as_str();
            if h == XML_XML_NAMESPACE {
                out.w("xml:");
                out.ws(&an.name);
            } else if h == XMLNS_NS {
                if an.name == "xmlns" {
                    out.w("xmlns");
                } else {
                    out.w("xmlns:");
                    out.ws(&an.name);
                }
            } else if h == XLINK_NS {
                out.w("xlink:");
                out.ws(&an.name);
            } else if let Some(p) = &t.nss[ns].prefix {
                out.ws(p);
                out.w(":");
                out.ws(&an.name);
            } else {
                out.ws(&an.name);
            }
        }
    }
}

fn node(t: &Tree, out: &mut Out, n: NodeId) {
    let nd = &t.nodes[n];
    match nd.t {
        NType::Dtd => {
            out.w("<!DOCTYPE ");
            out.ws(&nd.name);
            out.w(">");
        }
        NType::CData | NType::Text => {
            let raw = nd
                .parent
                .map(|p| {
                    t.nodes[p].t == NType::Element
                        && is_html(t, p)
                        && matches!(
                            t.nodes[p].name.as_str(),
                            "style"
                                | "script"
                                | "xmp"
                                | "iframe"
                                | "noembed"
                                | "noframes"
                                | "plaintext"
                        )
                })
                .unwrap_or(false);
            if raw {
                out.ws(&nd.content);
            } else {
                escape(out, &nd.content, false);
            }
        }
        NType::Pi => {
            out.w("<?");
            out.ws(&nd.name);
            out.w(" ");
            out.ws(&nd.content);
            out.w(">");
        }
        NType::Comment => {
            out.w("<!--");
            out.ws(&nd.content);
            out.w("-->");
        }
        NType::Element => {
            out.w("<");
            tag_name(t, out, n);
            for a in t.attrs_of(n) {
                out.w(" ");
                attr_name(t, out, a);
                out.w("=\"");
                let mut c = t.nodes[a].children;
                while let Some(x) = c {
                    match t.nodes[x].t {
                        NType::Text => escape(out, &t.nodes[x].content, true),
                        NType::EntityRef => {
                            out.w("&");
                            escape(out, &t.nodes[x].name, true);
                            out.w(";");
                        }
                        _ => {}
                    }
                    c = t.nodes[x].next;
                }
                out.w("\"");
            }
            out.w(">");
            // a template element serializes its template contents
            let children = if is_html(t, n) && nd.name == "template" {
                nd.template.and_then(|f| t.nodes[f].children)
            } else {
                nd.children
            };
            if serializes_as_void(t, n) {
                return;
            }
            let mut c = children;
            while let Some(x) = c {
                node(t, out, x);
                c = t.nodes[x].next;
            }
            out.w("</");
            tag_name(t, out, n);
            out.w(">");
        }
        NType::DocumentFrag | NType::Document | NType::HtmlDocument => {
            let mut c = nd.children;
            while let Some(x) = c {
                node(t, out, x);
                c = t.nodes[x].next;
            }
        }
        NType::EntityRef => {
            out.w("&");
            out.ws(&nd.name);
            out.w(";");
        }
        _ => {}
    }
}

/// dom_html5_serialize: the children of `n`.
pub fn serialize_children(n: NodeId) -> String {
    tree::with(|t| {
        let mut out = Out::default();
        if !matches!(
            t.nodes[n].t,
            NType::Element | NType::DocumentFrag | NType::Document | NType::HtmlDocument
        ) {
            return out.s;
        }
        if t.nodes[n].t == NType::Element && serializes_as_void(t, n) {
            return out.s;
        }
        let mut c = match t.nodes[n].template {
            Some(f) => t.nodes[f].children,
            None => t.nodes[n].children,
        };
        while let Some(x) = c {
            node(t, &mut out, x);
            c = t.nodes[x].next;
        }
        out.s
    })
}

/// dom_html5_serialize_outer
pub fn serialize_node(n: NodeId) -> String {
    serialize_node_out(n, false).s
}

/// dom_html5_serialize_outer, optionally recording the write calls.
pub fn serialize_node_out(n: NodeId, pieces: bool) -> Out {
    tree::with(|t| {
        let mut out = Out {
            s: String::new(),
            cuts: if pieces { Some(Vec::new()) } else { None },
        };
        node(t, &mut out, n);
        out
    })
}

/// dom_html_fragment_parsing_algorithm: parse `input` with `context` as
/// the context element; returns a new fragment owned by its document.
pub fn parse_fragment(_vm: &mut Vm, context: NodeId, input: &str) -> Option<NodeId> {
    let (name, ns, doc) = tree::with(|t| {
        let doc = t.nodes[context].doc.unwrap_or(context);
        (
            t.nodes[context].name.clone(),
            t.ns_href(t.nodes[context].ns).map(|s| s.to_string()),
            doc,
        )
    });
    let quirks = doc_props(Some(doc)).quirks_mode;
    let p =
        super::html_document::parse_fragment_input(&name, ns.as_deref(), quirks, input.as_bytes());
    let root = p.fragment_root()?;
    let frag = tree::with(|t| {
        let frag = t.new_fragment(Some(doc));
        super::html_document::convert(t, &p, root, doc, frag, true);
        frag
    });
    Some(frag)
}
