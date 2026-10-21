//! The HTML fragment serialization algorithm (html5_serializer.c), used by
//! innerHTML/outerHTML of Dom\HTMLDocument nodes. There is no HTML5 parser:
//! HTML fragment parsing is not supported.

use super::*;

fn escape(out: &mut String, s_: &str, attribute_mode: bool) {
    for c in s_.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '\u{A0}' => out.push_str("&nbsp;"),
            '"' if attribute_mode => out.push_str("&quot;"),
            '<' if !attribute_mode => out.push_str("&lt;"),
            '>' if !attribute_mode => out.push_str("&gt;"),
            '\0' => break,
            c => out.push(c),
        }
    }
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

fn tag_name(t: &Tree, out: &mut String, n: NodeId) {
    if let Some(ns) = t.nodes[n].ns {
        if let Some(p) = &t.nss[ns].prefix {
            let h = &t.nss[ns].href;
            if h != XHTML_NS && h != MATHML_NS && h != SVG_NS {
                out.push_str(p);
                out.push(':');
            }
        }
    }
    out.push_str(&t.nodes[n].name);
}

fn node(t: &Tree, out: &mut String, n: NodeId) {
    let nd = &t.nodes[n];
    match nd.t {
        NType::Dtd => {
            out.push_str("<!DOCTYPE ");
            out.push_str(&nd.name);
            out.push('>');
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
                out.push_str(&nd.content);
            } else {
                escape(out, &nd.content, false);
            }
        }
        NType::Pi => {
            out.push_str("<?");
            out.push_str(&nd.name);
            out.push(' ');
            out.push_str(&nd.content);
            out.push('>');
        }
        NType::Comment => {
            out.push_str("<!--");
            out.push_str(&nd.content);
            out.push_str("-->");
        }
        NType::Element => {
            out.push('<');
            tag_name(t, out, n);
            for a in t.attrs_of(n) {
                out.push(' ');
                let an = &t.nodes[a];
                match an.ns {
                    None => out.push_str(&an.name),
                    Some(ns) => {
                        let h = t.nss[ns].href.as_str();
                        if h == XML_XML_NAMESPACE {
                            out.push_str("xml:");
                            out.push_str(&an.name);
                        } else if h == XMLNS_NS {
                            if an.name == "xmlns" {
                                out.push_str("xmlns");
                            } else {
                                out.push_str("xmlns:");
                                out.push_str(&an.name);
                            }
                        } else if h == XLINK_NS {
                            out.push_str("xlink:");
                            out.push_str(&an.name);
                        } else if let Some(p) = &t.nss[ns].prefix {
                            out.push_str(p);
                            out.push(':');
                            out.push_str(&an.name);
                        } else {
                            out.push_str(&an.name);
                        }
                    }
                }
                out.push_str("=\"");
                let mut c = an.children;
                while let Some(x) = c {
                    match t.nodes[x].t {
                        NType::Text => escape(out, &t.nodes[x].content, true),
                        NType::EntityRef => {
                            out.push('&');
                            escape(out, &t.nodes[x].name, true);
                            out.push(';');
                        }
                        _ => {}
                    }
                    c = t.nodes[x].next;
                }
                out.push('"');
            }
            out.push('>');
            if serializes_as_void(t, n) {
                return;
            }
            let mut c = if is_html(t, n) && nd.name == "template" {
                nd.template.and_then(|f| t.nodes[f].children)
            } else {
                nd.children
            };
            while let Some(x) = c {
                node(t, out, x);
                c = t.nodes[x].next;
            }
            out.push_str("</");
            tag_name(t, out, n);
            out.push('>');
        }
        NType::DocumentFrag | NType::Document | NType::HtmlDocument => {
            let mut c = nd.children;
            while let Some(x) = c {
                node(t, out, x);
                c = t.nodes[x].next;
            }
        }
        NType::EntityRef => {
            out.push('&');
            out.push_str(&nd.name);
            out.push(';');
        }
        _ => {}
    }
}

/// dom_html5_serialize: the children of `n`.
pub fn serialize_children(n: NodeId) -> String {
    tree::with(|t| {
        let mut out = String::new();
        if !matches!(
            t.nodes[n].t,
            NType::Element | NType::DocumentFrag | NType::Document | NType::HtmlDocument
        ) {
            return out;
        }
        if t.nodes[n].t == NType::Element && serializes_as_void(t, n) {
            return out;
        }
        let mut c = match t.nodes[n].template {
            Some(f) => t.nodes[f].children,
            None => t.nodes[n].children,
        };
        while let Some(x) = c {
            node(t, &mut out, x);
            c = t.nodes[x].next;
        }
        out
    })
}

/// dom_html5_serialize_outer
pub fn serialize_node(n: NodeId) -> String {
    tree::with(|t| {
        let mut out = String::new();
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
