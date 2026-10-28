//! Canonical XML: W3C Canonical XML 1.0 and 1.1 (inclusive) and Exclusive
//! XML Canonicalization 1.0, for `DOMNode::C14N()` / `C14NFile()`.

use std::collections::HashSet;

use crate::tree::{Arena, NodeData, NodeId, NsDecl, XML_NS, XMLNS_NS};

/// Canonicalization algorithm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// Canonical XML 1.0.
    #[default]
    Inclusive,
    /// Exclusive XML Canonicalization 1.0.
    Exclusive,
    /// Canonical XML 1.1.
    Inclusive11,
}

/// Options.
#[derive(Clone, Debug, Default)]
pub struct C14nOptions {
    /// The algorithm.
    pub mode: Mode,
    /// Keep comments.
    pub with_comments: bool,
    /// Exclusive C14N InclusiveNamespaces PrefixList (`#default` for the
    /// default namespace).
    pub inclusive_prefixes: Vec<String>,
    /// A document subset (XPath node-set, including attribute and namespace
    /// nodes); `None` canonicalizes the subtree of the given node.
    pub node_set: Option<HashSet<NodeId>>,
}

struct Ctx<'a> {
    arena: &'a Arena,
    opts: &'a C14nOptions,
    out: String,
    /// (element, prefix) of namespace nodes in the node-set.
    ns_in_set: Option<HashSet<(NodeId, Option<String>)>>,
}

fn esc_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#xD;"),
            c => out.push(c),
        }
    }
}

fn esc_attr(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#x9;"),
            '\n' => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            c => out.push(c),
        }
    }
}

impl Ctx<'_> {
    fn in_set(&self, n: NodeId) -> bool {
        self.opts.node_set.as_ref().is_none_or(|s| s.contains(&n))
    }

    /// Namespace nodes of an element: in-scope declarations plus the
    /// element's and attributes' own bindings (nodes built by API calls may
    /// lack declarations).
    fn namespaces(&self, el: NodeId) -> Vec<NsDecl> {
        let mut v: Vec<NsDecl> = self
            .arena
            .in_scope_namespaces(el)
            .into_iter()
            .filter(|d| d.prefix.as_deref() != Some("xml"))
            .collect();
        let e = self.arena.element(el).unwrap();
        let mut add = |prefix: Option<&str>, uri: &str| {
            if !v.iter().any(|d| d.prefix.as_deref() == prefix) {
                v.push(NsDecl {
                    prefix: prefix.map(Into::into),
                    uri: uri.into(),
                });
            }
        };
        if let Some(ns) = &e.name.ns
            && !ns.is_empty()
        {
            add(e.name.prefix.as_deref(), ns);
        }
        for a in self.arena.attrs(el) {
            let ad = self.arena.attr(a).unwrap();
            if let (Some(p), Some(ns)) = (&ad.name.prefix, &ad.name.ns)
                && &**p != "xml"
                && &**ns != XMLNS_NS
            {
                add(Some(p), ns);
            }
        }
        v
    }

    fn ns_node_in_set(&self, el: NodeId, prefix: Option<&str>) -> bool {
        match &self.ns_in_set {
            None => true,
            Some(s) => s.contains(&(el, prefix.map(str::to_string))),
        }
    }

    fn node(&mut self, n: NodeId, rendered: &[NsDecl]) {
        let arena = self.arena;
        match arena.data(n) {
            NodeData::Document(_) => {
                let root = arena.document_element(n);
                let mut after_root = false;
                for c in arena.children(n) {
                    if Some(c) == root {
                        self.node(c, rendered);
                        after_root = true;
                        continue;
                    }
                    match arena.data(c) {
                        NodeData::Comment(_) | NodeData::Pi { .. } => {
                            if !self.in_set(c)
                                || (matches!(arena.data(c), NodeData::Comment(_))
                                    && !self.opts.with_comments)
                            {
                                continue;
                            }
                            if after_root {
                                self.out.push('\n');
                            }
                            self.node(c, rendered);
                            if !after_root {
                                self.out.push('\n');
                            }
                        }
                        _ => {}
                    }
                }
            }
            NodeData::Fragment => {
                for c in arena.children(n) {
                    self.node(c, rendered);
                }
            }
            NodeData::Element(e) => {
                if !self.in_set(n) {
                    for c in arena.children(n) {
                        self.node(c, rendered);
                    }
                    return;
                }
                let qname = e.name.qualified();
                let all_ns = self.namespaces(n);
                let mut out_ns: Vec<NsDecl> = Vec::new();
                let rendered_uri = |p: Option<&str>| -> Option<&str> {
                    rendered
                        .iter()
                        .find(|d| d.prefix.as_deref() == p)
                        .map(|d| &*d.uri)
                };
                match self.opts.mode {
                    Mode::Exclusive => {
                        let mut used: Vec<Option<String>> =
                            vec![e.name.prefix.as_deref().map(str::to_string)];
                        for a in arena.attrs(n) {
                            let ad = arena.attr(a).unwrap();
                            if let Some(p) = &ad.name.prefix
                                && &**p != "xml"
                                && ad.name.ns.as_deref() != Some(XMLNS_NS)
                                && self.in_set(a)
                            {
                                used.push(Some(p.to_string()));
                            }
                        }
                        for p in &self.opts.inclusive_prefixes {
                            used.push(if p == "#default" {
                                None
                            } else {
                                Some(p.clone())
                            });
                        }
                        for d in &all_ns {
                            let p = d.prefix.as_deref().map(str::to_string);
                            if !used.contains(&p) || !self.ns_node_in_set(n, d.prefix.as_deref()) {
                                continue;
                            }
                            if rendered_uri(d.prefix.as_deref()) != Some(&d.uri) {
                                out_ns.push(d.clone());
                            }
                        }
                        // xmlns="" when the default namespace is utilized,
                        // empty, and an ancestor rendered one.
                        if e.name.prefix.is_none()
                            && !all_ns.iter().any(|d| d.prefix.is_none())
                            && rendered_uri(None).is_some_and(|u| !u.is_empty())
                        {
                            out_ns.push(NsDecl {
                                prefix: None,
                                uri: "".into(),
                            });
                        }
                    }
                    _ => {
                        for d in &all_ns {
                            if !self.ns_node_in_set(n, d.prefix.as_deref()) {
                                continue;
                            }
                            if rendered_uri(d.prefix.as_deref()) != Some(&d.uri) {
                                out_ns.push(d.clone());
                            }
                        }
                        if !all_ns.iter().any(|d| d.prefix.is_none())
                            && rendered_uri(None).is_some_and(|u| !u.is_empty())
                        {
                            out_ns.push(NsDecl {
                                prefix: None,
                                uri: "".into(),
                            });
                        }
                    }
                }
                out_ns.sort_by(|a, b| a.prefix.cmp(&b.prefix));
                // Attributes.
                let mut attrs: Vec<(String, String, String)> = Vec::new();
                for a in arena.attrs(n) {
                    let ad = arena.attr(a).unwrap();
                    if ad.name.ns.as_deref() == Some(XMLNS_NS) || !self.in_set(a) {
                        continue;
                    }
                    let ns = ad.name.ns.as_deref().unwrap_or("").to_string();
                    attrs.push((ns, ad.name.local.to_string(), {
                        let mut s = String::new();
                        s.push_str(&ad.name.qualified());
                        s.push_str("=\"");
                        esc_attr(&arena.attr_value(a), &mut s);
                        s.push('"');
                        s
                    }));
                }
                // Canonical XML 1.0: xml:* attributes of ancestors outside
                // the node-set are inherited by the apex element.
                if self.opts.mode != Mode::Exclusive && self.opts.node_set.is_some() {
                    let parent_in = arena
                        .parent(n)
                        .is_some_and(|p| arena.is_element(p) && self.in_set(p));
                    if !parent_in {
                        let mut p = arena.parent(n);
                        while let Some(x) = p {
                            if arena.is_element(x) {
                                for a in arena.attrs(x) {
                                    let ad = arena.attr(a).unwrap();
                                    if ad.name.ns.as_deref() == Some(XML_NS)
                                        && !attrs
                                            .iter()
                                            .any(|(ns, l, _)| ns == XML_NS && *l == *ad.name.local)
                                    {
                                        let mut s = format!("xml:{}=\"", ad.name.local);
                                        esc_attr(&arena.attr_value(a), &mut s);
                                        s.push('"');
                                        attrs.push((
                                            XML_NS.to_string(),
                                            ad.name.local.to_string(),
                                            s,
                                        ));
                                    }
                                }
                            }
                            p = arena.parent(x);
                        }
                    }
                }
                attrs.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
                self.out.push('<');
                self.out.push_str(&qname);
                for d in &out_ns {
                    match &d.prefix {
                        Some(p) => {
                            self.out.push_str(" xmlns:");
                            self.out.push_str(p);
                        }
                        None => self.out.push_str(" xmlns"),
                    }
                    self.out.push_str("=\"");
                    esc_attr(&d.uri, &mut self.out);
                    self.out.push('"');
                }
                for (_, _, s) in &attrs {
                    self.out.push(' ');
                    self.out.push_str(s);
                }
                self.out.push('>');
                let mut ctx: Vec<NsDecl> = rendered.to_vec();
                for d in out_ns {
                    ctx.retain(|x| x.prefix != d.prefix);
                    ctx.push(d);
                }
                for c in arena.children(n) {
                    self.node(c, &ctx);
                }
                self.out.push_str("</");
                self.out.push_str(&qname);
                self.out.push('>');
            }
            NodeData::Text(t) | NodeData::CData(t) => {
                if self.in_set(n) {
                    esc_text(t, &mut self.out);
                }
            }
            NodeData::EntityRef(name) => {
                if arena.first_child(n).is_some() {
                    for c in arena.children(n) {
                        self.node(c, rendered);
                    }
                } else if self.in_set(n)
                    && let Some(v) = arena.entity_value(arena.doc_of(n), name)
                {
                    esc_text(&v, &mut self.out);
                }
            }
            NodeData::Comment(t) => {
                if self.opts.with_comments && self.in_set(n) {
                    self.out.push_str("<!--");
                    self.out.push_str(t);
                    self.out.push_str("-->");
                }
            }
            NodeData::Pi { target, data } => {
                if self.in_set(n) {
                    self.out.push_str("<?");
                    self.out.push_str(target);
                    if !data.is_empty() {
                        self.out.push(' ');
                        self.out.push_str(data);
                    }
                    self.out.push_str("?>");
                }
            }
            _ => {}
        }
    }
}

/// Canonicalizes `node` (or, with a node-set, the document's nodes in the
/// set).
pub fn canonicalize(arena: &Arena, node: NodeId, opts: &C14nOptions) -> String {
    let ns_in_set = opts.node_set.as_ref().map(|s| {
        s.iter()
            .filter_map(|&n| match arena.data(n) {
                NodeData::Namespace(d) => arena
                    .parent(n)
                    .map(|p| (p, d.prefix.as_deref().map(str::to_string))),
                _ => None,
            })
            .collect::<HashSet<_>>()
    });
    // A node-set without namespace nodes keeps all namespace nodes.
    let ns_in_set = ns_in_set.filter(|s| !s.is_empty());
    let mut c = Ctx {
        arena,
        opts,
        out: String::new(),
        ns_in_set,
    };
    let start = if opts.node_set.is_some() {
        arena.doc_of(node)
    } else {
        node
    };
    c.node(start, &[]);
    c.out
}
