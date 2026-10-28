//! HTML serialization (WHATWG HTML "13.3 Serializing HTML fragments"), as
//! `Dom\HTMLDocument::saveHtml()` and the `innerHTML`/`outerHTML` getters
//! use it.
//!
//! PHP 8.5 behaviour (observed, and in the ext/dom serializer tests):
//! attribute values escape `&`, U+00A0 and `"` only (`<` and `>` are kept);
//! text escapes `&`, U+00A0, `<` and `>`; text inside HTML `style`,
//! `script`, `xmp`, `iframe`, `noembed`, `noframes` and `plaintext` is
//! written as is (scripting is disabled, so `noscript` text is escaped);
//! CDATA sections are written as escaped text; entity references as
//! `&name;`; doctypes as `<!DOCTYPE name>`.

use crate::tree::{
    Arena, HTML_NS, MATHML_NS, NodeData, NodeId, SVG_NS, XLINK_NS, XML_NS, XMLNS_NS,
};

/// Serialization options.
#[derive(Clone, Copy, Debug, Default)]
pub struct SerializeOptions {
    /// Treat elements without a namespace as HTML elements (documents
    /// parsed with `Dom\HTML_NO_DEFAULT_NS`).
    pub null_ns_is_html: bool,
}

fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "basefont"
            | "bgsound"
            | "br"
            | "col"
            | "embed"
            | "frame"
            | "hr"
            | "img"
            | "input"
            | "keygen"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn is_raw_text_parent(name: &str) -> bool {
    matches!(
        name,
        "style" | "script" | "xmp" | "iframe" | "noembed" | "noframes" | "plaintext"
    )
}

/// Escapes text (`attr` = attribute mode).
pub fn escape(s: &str, attr: bool, out: &mut String) {
    let bytes = s.as_bytes();
    let mut last = 0;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let rep: Option<&str> = match b {
            b'&' => Some("&amp;"),
            b'"' if attr => Some("&quot;"),
            b'<' if !attr => Some("&lt;"),
            b'>' if !attr => Some("&gt;"),
            0xC2 if bytes.get(i + 1) == Some(&0xA0) => Some("&nbsp;"),
            _ => None,
        };
        if let Some(r) = rep {
            out.push_str(&s[last..i]);
            out.push_str(r);
            i += if b == 0xC2 { 2 } else { 1 };
            last = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&s[last..]);
}

/// The serialization of `node` itself and its subtree (`outerHTML`,
/// `saveHtml($node)`). Documents and fragments serialize their children.
pub fn serialize_node(arena: &Arena, node: NodeId) -> String {
    serialize_node_with(arena, node, SerializeOptions::default())
}

/// [`serialize_node`] with options.
pub fn serialize_node_with(arena: &Arena, node: NodeId, opts: SerializeOptions) -> String {
    let mut out = String::new();
    match arena.data(node) {
        NodeData::Document(_) | NodeData::Fragment => {
            Serializer { arena, opts }.children(node, &mut out)
        }
        _ => Serializer { arena, opts }.node(node, &mut out),
    }
    out
}

/// The serialization of `node`'s children (`innerHTML`; a `template`
/// element serializes its template contents).
pub fn serialize_children(arena: &Arena, node: NodeId) -> String {
    serialize_children_with(arena, node, SerializeOptions::default())
}

/// [`serialize_children`] with options.
pub fn serialize_children_with(arena: &Arena, node: NodeId, opts: SerializeOptions) -> String {
    let mut out = String::new();
    Serializer { arena, opts }.children(node, &mut out);
    out
}

struct Serializer<'a> {
    arena: &'a Arena,
    opts: SerializeOptions,
}

enum Step {
    Enter(NodeId),
    Close(NodeId),
}

impl Serializer<'_> {
    fn is_html(&self, ns: Option<&str>) -> bool {
        match ns {
            Some(HTML_NS) => true,
            None => self.opts.null_ns_is_html,
            _ => false,
        }
    }

    /// The element's children container (template contents for HTML
    /// templates).
    fn content_of(&self, n: NodeId) -> NodeId {
        if let Some(e) = self.arena.element(n)
            && let Some(tc) = e.template_contents
            && &*e.name.local == "template"
            && self.is_html(e.name.ns.as_deref())
        {
            return tc;
        }
        n
    }

    fn children(&self, n: NodeId, out: &mut String) {
        let c = self.content_of(n);
        let mut stack: Vec<Step> = Vec::new();
        let kids: Vec<NodeId> = self.arena.children(c).collect();
        for k in kids.into_iter().rev() {
            stack.push(Step::Enter(k));
        }
        self.run(stack, out);
    }

    fn node(&self, n: NodeId, out: &mut String) {
        self.run(vec![Step::Enter(n)], out);
    }

    fn run(&self, mut stack: Vec<Step>, out: &mut String) {
        while let Some(step) = stack.pop() {
            match step {
                Step::Close(n) => {
                    let e = self.arena.element(n).unwrap();
                    out.push_str("</");
                    self.tag_name(n, out);
                    let _ = e;
                    out.push('>');
                }
                Step::Enter(n) => match self.arena.data(n) {
                    NodeData::Element(e) => {
                        out.push('<');
                        self.tag_name(n, out);
                        for a in self.arena.attrs(n) {
                            out.push(' ');
                            self.attr_name(a, out);
                            out.push_str("=\"");
                            escape(&self.arena.attr_value(a), true, out);
                            out.push('"');
                        }
                        out.push('>');
                        if self.is_html(e.name.ns.as_deref()) && is_void(&e.name.local) {
                            continue;
                        }
                        stack.push(Step::Close(n));
                        let c = self.content_of(n);
                        let kids: Vec<NodeId> = self.arena.children(c).collect();
                        for k in kids.into_iter().rev() {
                            stack.push(Step::Enter(k));
                        }
                    }
                    NodeData::Text(t) | NodeData::CData(t) => {
                        let raw = self.arena.parent(n).is_some_and(|p| {
                            self.arena.element(p).is_some_and(|pe| {
                                self.is_html(pe.name.ns.as_deref())
                                    && is_raw_text_parent(&pe.name.local)
                            })
                        });
                        if raw {
                            out.push_str(t);
                        } else {
                            escape(t, false, out);
                        }
                    }
                    NodeData::Comment(c) => {
                        out.push_str("<!--");
                        out.push_str(c);
                        out.push_str("-->");
                    }
                    NodeData::Pi { target, data } => {
                        out.push_str("<?");
                        out.push_str(target);
                        out.push(' ');
                        out.push_str(data);
                        out.push('>');
                    }
                    NodeData::Doctype(d) => {
                        out.push_str("<!DOCTYPE ");
                        out.push_str(&d.name);
                        out.push('>');
                    }
                    NodeData::EntityRef(name) => {
                        out.push('&');
                        out.push_str(name);
                        out.push(';');
                    }
                    NodeData::Document(_) | NodeData::Fragment => {
                        let kids: Vec<NodeId> = self.arena.children(n).collect();
                        for k in kids.into_iter().rev() {
                            stack.push(Step::Enter(k));
                        }
                    }
                    _ => {}
                },
            }
        }
    }

    fn tag_name(&self, n: NodeId, out: &mut String) {
        let e = self.arena.element(n).unwrap();
        match e.name.ns.as_deref() {
            Some(HTML_NS) | Some(SVG_NS) | Some(MATHML_NS) => out.push_str(&e.name.local),
            _ => {
                if let Some(p) = &e.name.prefix {
                    out.push_str(p);
                    out.push(':');
                }
                out.push_str(&e.name.local);
            }
        }
    }

    fn attr_name(&self, a: NodeId, out: &mut String) {
        let d = self.arena.attr(a).unwrap();
        match d.name.ns.as_deref() {
            None => out.push_str(&d.name.local),
            Some(XML_NS) => {
                out.push_str("xml:");
                out.push_str(&d.name.local);
            }
            Some(XMLNS_NS) => {
                if &*d.name.local != "xmlns" {
                    out.push_str("xmlns:");
                }
                out.push_str(&d.name.local);
            }
            Some(XLINK_NS) => {
                out.push_str("xlink:");
                out.push_str(&d.name.local);
            }
            Some(_) => out.push_str(&d.name.qualified()),
        }
    }
}
