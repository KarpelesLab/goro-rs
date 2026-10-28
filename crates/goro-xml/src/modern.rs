//! XML serialization for the spec-compliant `Dom\` classes
//! (`Dom\XMLDocument::saveXml`, `Dom\Node` XML serialization): the W3C
//! DOM Parsing and Serialization "XML serialization" algorithm with its
//! namespace prefix map, as amended by the issues PHP follows
//! (w3c/DOM-Parsing#44, #45, #47: tests ext/dom/tests/modern/spec/
//! dom_parsing_gh*.phpt), plus libxml-style `formatOutput` indentation.

use crate::tree::{Arena, HTML_NS, NodeData, NodeId, Str, XML_NS, XMLNS_NS};

/// Serialization settings.
#[derive(Clone, Copy, Debug, Default)]
pub struct ModernOptions {
    /// `formatOutput`.
    pub format: bool,
    /// LIBXML_NOEMPTYTAG.
    pub no_empty_tags: bool,
    /// LIBXML_NOXMLDECL.
    pub no_xml_decl: bool,
    /// Fail on not-well-formed nodes ("require well-formed").
    pub require_well_formed: bool,
}

/// Why serialization failed (with `require_well_formed`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotWellFormed(pub &'static str);

/// The namespace prefix map: per namespace, the prefixes bound to it in
/// order of declaration, and the latest binding of each prefix.
#[derive(Clone, Default)]
struct PrefixMap {
    by_ns: Vec<(Option<Str>, Vec<Str>)>,
    binding: Vec<(Str, Option<Str>)>,
}

impl PrefixMap {
    fn add(&mut self, ns: Option<Str>, prefix: Str) {
        match self.by_ns.iter_mut().find(|(n, _)| *n == ns) {
            Some((_, v)) => v.push(prefix.clone()),
            None => self.by_ns.push((ns.clone(), vec![prefix.clone()])),
        }
        self.binding.retain(|(p, _)| *p != prefix);
        self.binding.push((prefix, ns));
    }
    fn contains(&self, ns: &Option<Str>, prefix: &str) -> bool {
        self.by_ns
            .iter()
            .any(|(n, v)| n == ns && v.iter().any(|p| &**p == prefix))
    }
    /// "Retrieve a preferred prefix string" (with the DOM-Parsing#45 fix:
    /// a prefix rebound to another namespace no longer counts).
    fn preferred(&self, ns: &Option<Str>, preferred: Option<&str>) -> Option<Str> {
        let list = self.by_ns.iter().find(|(n, _)| n == ns).map(|(_, v)| v)?;
        let live: Vec<&Str> = list
            .iter()
            .filter(|p| {
                self.binding
                    .iter()
                    .rev()
                    .find(|(bp, _)| bp == *p)
                    .is_some_and(|(_, b)| b == ns)
            })
            .collect();
        if let Some(pp) = preferred
            && let Some(p) = live.iter().find(|p| &****p == pp)
        {
            return Some((*p).clone());
        }
        live.last().map(|p| (*p).clone())
    }
}

struct W<'a> {
    arena: &'a Arena,
    out: String,
    opts: ModernOptions,
    err: Option<NotWellFormed>,
}

fn esc_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

fn esc_attr(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
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

impl W<'_> {
    fn indent(&mut self, level: usize) {
        for _ in 0..level {
            self.out.push_str("  ");
        }
    }

    fn texty(&self, n: NodeId) -> bool {
        matches!(
            self.arena.data(n),
            NodeData::Text(_) | NodeData::CData(_) | NodeData::EntityRef(_)
        )
    }

    fn node(
        &mut self,
        n: NodeId,
        ctx_ns: Option<Str>,
        map: &mut PrefixMap,
        index: &mut u32,
        level: usize,
        format: bool,
    ) {
        let arena = self.arena;
        match arena.data(n) {
            NodeData::Document(_) => {
                let d = arena.doc(n);
                if !self.opts.no_xml_decl {
                    self.out.push_str("<?xml version=\"");
                    self.out.push_str(d.version.as_deref().unwrap_or("1.0"));
                    self.out.push('"');
                    self.out.push_str(" encoding=\"");
                    self.out.push_str(d.encoding.as_deref().unwrap_or("UTF-8"));
                    self.out.push('"');
                    match d.standalone {
                        1 => self.out.push_str(" standalone=\"yes\""),
                        0 => self.out.push_str(" standalone=\"no\""),
                        _ => {}
                    }
                    self.out.push_str("?>\n");
                }
                let kids: Vec<NodeId> = arena.children(n).collect();
                for (i, c) in kids.iter().enumerate() {
                    let doctype = matches!(arena.data(*c), NodeData::Doctype(_));
                    if i > 0 && !matches!(arena.data(kids[i - 1]), NodeData::Doctype(_)) {
                        self.out.push('\n');
                    }
                    self.node(*c, None, map, index, 0, format);
                    // A doctype is always followed by a newline (tests
                    // ext/dom/tests/modern/spec/Document_implementation_*).
                    if doctype {
                        self.out.push('\n');
                    }
                }
            }
            NodeData::Fragment => {
                for c in arena.children(n).collect::<Vec<_>>() {
                    self.node(c, ctx_ns.clone(), map, index, level, format);
                }
            }
            NodeData::Element(_) => self.element(n, ctx_ns, map, index, level, format),
            NodeData::Text(t) => esc_text(t, &mut self.out),
            NodeData::CData(t) => {
                if self.opts.require_well_formed && t.contains("]]>") {
                    self.err = Some(NotWellFormed("CDATA section contains ]]>"));
                }
                self.out.push_str("<![CDATA[");
                self.out.push_str(t);
                self.out.push_str("]]>");
            }
            NodeData::Comment(t) => {
                if self.opts.require_well_formed && (t.contains("--") || t.ends_with('-')) {
                    self.err = Some(NotWellFormed("comment is not well-formed"));
                }
                self.out.push_str("<!--");
                self.out.push_str(t);
                self.out.push_str("-->");
            }
            NodeData::Pi { target, data } => {
                self.out.push_str("<?");
                self.out.push_str(target);
                self.out.push(' ');
                self.out.push_str(data);
                self.out.push_str("?>");
            }
            NodeData::Doctype(d) => {
                self.out.push_str("<!DOCTYPE ");
                self.out.push_str(&d.name);
                match (&d.public_id, &d.system_id) {
                    (Some(p), s) if !p.is_empty() => {
                        self.out.push_str(" PUBLIC \"");
                        self.out.push_str(p);
                        self.out.push('"');
                        if let Some(s) = s.as_ref().filter(|s| !s.is_empty()) {
                            self.out.push_str(" \"");
                            self.out.push_str(s);
                            self.out.push('"');
                        }
                    }
                    (_, Some(s)) if !s.is_empty() => {
                        self.out.push_str(" SYSTEM \"");
                        self.out.push_str(s);
                        self.out.push('"');
                    }
                    _ => {}
                }
                if let Some(sub) = crate::serialize::internal_subset(d) {
                    self.out.push_str(" [\n");
                    self.out.push_str(&sub);
                    self.out.push(']');
                }
                self.out.push('>');
            }
            NodeData::EntityRef(name) => {
                self.out.push('&');
                self.out.push_str(name);
                self.out.push(';');
            }
            NodeData::Attribute(_) => {
                let v = arena.attr_value(n);
                esc_attr(&v, &mut self.out);
            }
            _ => {}
        }
    }

    fn element(
        &mut self,
        n: NodeId,
        ctx_ns: Option<Str>,
        parent_map: &mut PrefixMap,
        index: &mut u32,
        level: usize,
        format: bool,
    ) {
        let arena = self.arena;
        let e = arena.element(n).unwrap();
        let mut map = parent_map.clone();
        let mut local_prefixes: Vec<(Str, Option<Str>)> = Vec::new();
        // Record the namespace information.
        let mut local_default: Option<Option<Str>> = None;
        for a in arena.attrs(n) {
            let ad = arena.attr(a).unwrap();
            if ad.name.ns.as_deref() != Some(XMLNS_NS) {
                continue;
            }
            let val: Str = arena.attr_value(a).into();
            if ad.name.prefix.is_none() {
                if local_default.is_none() {
                    local_default = Some(if val.is_empty() { None } else { Some(val) });
                }
                continue;
            }
            let pdef = ad.name.local.clone();
            if &*val == XML_NS {
                continue;
            }
            let ndef = if val.is_empty() { None } else { Some(val) };
            if map.contains(&ndef, &pdef) {
                continue;
            }
            map.add(ndef.clone(), pdef.clone());
            local_prefixes.push((pdef, ndef));
        }
        let ns = e.name.ns.clone().filter(|s| !s.is_empty());
        let mut inherited = ctx_ns.clone();
        let mut ignore_default_attr = false;
        let qname: String;
        let mut extra = String::new();
        if inherited == ns {
            // DOM-Parsing#47: a redundant default declaration is kept.
            if ns.as_deref() == Some(XML_NS) {
                qname = format!("xml:{}", e.name.local);
            } else {
                qname = match &e.name.prefix {
                    // Keep the prefix when it is bound to this namespace.
                    Some(p) if map.preferred(&ns, Some(p)).as_deref() == Some(&**p) => {
                        format!("{p}:{}", e.name.local)
                    }
                    _ => e.name.local.to_string(),
                };
            }
        } else {
            let prefix = e.name.prefix.clone();
            let candidate = map.preferred(&ns, prefix.as_deref());
            if prefix.as_deref() == Some("xmlns") && self.opts.require_well_formed {
                self.err = Some(NotWellFormed("element prefix xmlns"));
            }
            if let Some(c) = candidate {
                qname = format!("{c}:{}", e.name.local);
                if let Some(ld) = &local_default
                    && ld.as_deref() != Some(XML_NS)
                {
                    inherited = ld.clone();
                }
            } else if let Some(mut p) = prefix {
                if local_prefixes.iter().any(|(lp, _)| *lp == p) {
                    p = generate_prefix(&mut map, &local_prefixes, ns.clone(), index);
                } else {
                    map.add(ns.clone(), p.clone());
                }
                qname = format!("{p}:{}", e.name.local);
                extra.push_str(&format!(" xmlns:{p}=\""));
                esc_attr(ns.as_deref().unwrap_or(""), &mut extra);
                extra.push('"');
                if let Some(ld) = &local_default {
                    inherited = ld.clone();
                }
            } else if local_default.is_none() || local_default.as_ref() != Some(&ns) {
                ignore_default_attr = true;
                qname = e.name.local.to_string();
                inherited = ns.clone();
                extra.push_str(" xmlns=\"");
                esc_attr(ns.as_deref().unwrap_or(""), &mut extra);
                extra.push('"');
            } else {
                qname = e.name.local.to_string();
                inherited = ns.clone();
            }
        }
        self.out.push('<');
        self.out.push_str(&qname);
        self.out.push_str(&extra);
        // Serialize the attributes.
        for a in arena.attrs(n).collect::<Vec<_>>() {
            let ad = arena.attr(a).unwrap();
            let ans = ad.name.ns.clone().filter(|s| !s.is_empty());
            let mut candidate: Option<Str> = None;
            if let Some(ans_v) = &ans {
                candidate = map.preferred(&ans, ad.name.prefix.as_deref());
                if &**ans_v == XMLNS_NS {
                    let v = arena.attr_value(a);
                    if v == XML_NS {
                        continue;
                    }
                    if ad.name.prefix.is_none() && ignore_default_attr {
                        continue;
                    }
                    if ad.name.prefix.is_some() {
                        let lp = local_prefixes.iter().find(|(p, _)| *p == ad.name.local);
                        let matches = lp.is_some_and(|(_, u)| u.as_deref().unwrap_or("") == v);
                        if !matches {
                            continue;
                        }
                    }
                    candidate = Some("xmlns".into());
                } else if &**ans_v == XML_NS {
                    candidate = Some("xml".into());
                } else if candidate.is_none() {
                    // The attribute's own prefix when nothing binds it here
                    // (PHP keeps it; tests ext/dom/tests/modern/
                    // css_selectors/namespaces.phpt,
                    // modern/extensions/node_renaming.phpt), else a
                    // generated one.
                    let own = ad.name.prefix.clone().filter(|p| {
                        &**p != "xmlns"
                            && !local_prefixes.iter().any(|(lp, _)| lp == p)
                            && !map.binding.iter().any(|(bp, _)| bp == p)
                    });
                    let p = match own {
                        Some(p) => {
                            map.add(ans.clone(), p.clone());
                            p
                        }
                        None => generate_prefix(&mut map, &local_prefixes, ans.clone(), index),
                    };
                    self.out.push_str(&format!(" xmlns:{p}=\""));
                    esc_attr(ans_v, &mut self.out);
                    self.out.push('"');
                    candidate = Some(p);
                }
            }
            self.out.push(' ');
            match &candidate {
                Some(c) if !(c.as_ref() == "xmlns" && ad.name.prefix.is_none()) => {
                    self.out.push_str(c);
                    self.out.push(':');
                }
                _ => {}
            }
            self.out.push_str(&ad.name.local);
            self.out.push_str("=\"");
            esc_attr(&arena.attr_value(a), &mut self.out);
            self.out.push('"');
        }
        let empty = arena.first_child(n).is_none() && e.template_contents.is_none();
        if ns.as_deref() == Some(HTML_NS) && empty && is_void(&e.name.local) {
            self.out.push_str(" />");
            return;
        }
        if ns.as_deref() != Some(HTML_NS) && empty {
            if self.opts.no_empty_tags {
                self.out.push_str("></");
                self.out.push_str(&qname);
                self.out.push('>');
            } else {
                self.out.push_str("/>");
            }
            return;
        }
        self.out.push('>');
        let content = if ns.as_deref() == Some(HTML_NS) && &*e.name.local == "template" {
            e.template_contents.unwrap_or(n)
        } else {
            n
        };
        let fmt = format && !arena.children(content).any(|c| self.texty(c));
        if fmt {
            self.out.push('\n');
        }
        for c in arena.children(content).collect::<Vec<_>>() {
            if fmt {
                self.indent(level + 1);
            }
            self.node(c, inherited.clone(), &mut map, index, level + 1, fmt);
            if fmt {
                self.out.push('\n');
            }
        }
        if fmt {
            self.indent(level);
        }
        self.out.push_str("</");
        self.out.push_str(&qname);
        self.out.push('>');
    }
}

/// "Generate a prefix" (with the DOM-Parsing#44 fix: skip prefixes declared
/// on the element itself).
fn generate_prefix(
    map: &mut PrefixMap,
    local: &[(Str, Option<Str>)],
    ns: Option<Str>,
    index: &mut u32,
) -> Str {
    loop {
        let p: Str = format!("ns{index}").into();
        *index += 1;
        if local.iter().any(|(lp, _)| *lp == p) {
            continue;
        }
        map.add(ns, p.clone());
        return p;
    }
}

/// Serializes a node of a `Dom\` document to UTF-8 text.
pub fn to_string(arena: &Arena, n: NodeId, opts: ModernOptions) -> Result<String, NotWellFormed> {
    let mut w = W {
        arena,
        out: String::new(),
        opts,
        err: None,
    };
    let mut map = PrefixMap::default();
    map.add(Some(XML_NS.into()), "xml".into());
    let mut index = 1;
    w.node(n, None, &mut map, &mut index, 0, opts.format);
    match w.err {
        Some(e) => Err(e),
        None => Ok(w.out),
    }
}

/// `Dom\XMLDocument::saveXml()`: the bytes in the document's encoding.
pub fn save_xml(arena: &Arena, n: NodeId, opts: ModernOptions) -> Result<Vec<u8>, NotWellFormed> {
    let s = to_string(arena, n, opts)?;
    let doc = arena.doc_of(n);
    let enc = arena
        .doc(doc)
        .encoding
        .clone()
        .unwrap_or_else(|| "UTF-8".into());
    Ok(crate::serialize::encode_output(&s, Some(&enc)))
}
