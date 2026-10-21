//! The XML serialization algorithm of DOM Parsing (xml_serializer.c),
//! with the spec fixes php-src applies.

use super::*;
use crate::serialize::{self, SaveCtx};

type R2 = Result<(), ()>;

/// namespace -> list of prefixes (the "namespace prefix map")
#[derive(Clone, Default)]
struct PrefixMap {
    m: HashMap<String, Vec<String>>,
}

impl PrefixMap {
    fn add(&mut self, prefix: &str, ns: Option<&str>) {
        self.m
            .entry(ns.unwrap_or("").to_string())
            .or_default()
            .push(prefix.to_string());
    }
    fn candidates(&self, ns: &str) -> Option<&Vec<String>> {
        self.m.get(ns)
    }
    fn found(&self, prefix: &str, ns: &str) -> bool {
        match self.candidates(ns) {
            Some(l) => l.iter().any(|p| p == prefix),
            None => false,
        }
    }
}

/// prefix -> namespace ("local prefixes map"); first insertion wins
#[derive(Default)]
struct LocalMap {
    v: Vec<(String, String)>,
}

impl LocalMap {
    fn add(&mut self, prefix: &str, ns: &str) {
        if !self.v.iter().any(|(p, _)| p == prefix) {
            self.v.push((prefix.to_string(), ns.to_string()));
        }
    }
    fn find(&self, prefix: &str) -> Option<&str> {
        self.v
            .iter()
            .find(|(p, _)| p == prefix)
            .map(|(_, n)| n.as_str())
    }
    fn contains(&self, prefix: &str) -> bool {
        self.find(prefix).is_some()
    }
    fn conflicts(&self, prefix: &str, ns: &str) -> bool {
        match self.find(prefix) {
            None => false,
            Some(r) => r != ns,
        }
    }
}

pub struct Ser<'a> {
    t: &'a Tree,
    pub out: String,
    no_empty_tags: bool,
    no_decl: bool,
    encoding: Option<String>,
}

fn eq_null_as_empty(a: Option<&str>, b: Option<&str>) -> bool {
    a.unwrap_or("") == b.unwrap_or("")
}

fn attr_value(t: &Tree, a: NodeId) -> String {
    match t.nodes[a].children {
        None => String::new(),
        Some(c) => t.nodes[c].content.clone(),
    }
}

fn text_ser(out: &mut String, content: &str, attribute_mode: bool) {
    for c in content.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute_mode => out.push_str("&quot;"),
            '\t' if attribute_mode => out.push_str("&#9;"),
            '\n' if attribute_mode => out.push_str("&#10;"),
            '\r' if attribute_mode => out.push_str("&#13;"),
            '\0' => break,
            c => out.push(c),
        }
    }
}

fn is_xml_char(c: char) -> bool {
    let c = c as u32;
    c == 0x9
        || c == 0xA
        || c == 0xD
        || (0x20..=0xD7FF).contains(&c)
        || (0xE000..=0xFFFD).contains(&c)
        || (0x10000..=0x10FFFF).contains(&c)
}

fn check_char_production(s: &str) -> R2 {
    if s.chars().all(is_xml_char) {
        Ok(())
    } else {
        Err(())
    }
}

/// Retrieving a preferred prefix string (with the php-src fix for issue 45:
/// the nearest declaration gets priority).
fn preferred_prefix(
    map: &PrefixMap,
    local: &LocalMap,
    preferred: Option<&str>,
    ns: Option<&str>,
) -> Option<String> {
    let ns = ns.unwrap_or("");
    let list = map.candidates(ns)?;
    let mut last_non_conflicting: Option<&String> = None;
    for p in list.iter().rev() {
        if !local.conflicts(p, ns) {
            if eq_null_as_empty(preferred, Some(p)) {
                return Some(p.clone());
            }
            if last_non_conflicting.is_none() {
                last_non_conflicting = Some(p);
            }
        }
    }
    last_non_conflicting.cloned()
}

fn generate_prefix(
    map: &mut PrefixMap,
    local: &mut LocalMap,
    new_ns: Option<&str>,
    index: &mut u32,
) -> String {
    let mut g;
    loop {
        g = format!("ns{}", *index);
        *index += 1;
        if !local.contains(&g) {
            break;
        }
    }
    map.add(&g, new_ns);
    local.add(&g, new_ns.unwrap_or(""));
    g
}

fn write_qname(out: &mut String, prefix: Option<&str>, name: &str) {
    if let Some(p) = prefix {
        out.push_str(p);
        out.push(':');
    }
    out.push_str(name);
}

const HTML_VOID: &[&str] = &[
    "area", "base", "basefont", "bgsound", "br", "col", "embed", "frame", "hr", "img", "input",
    "keygen", "link", "menuitem", "meta", "param", "source", "track", "wbr",
];

impl<'a> Ser<'a> {
    pub fn new(t: &'a Tree) -> Self {
        Ser {
            t,
            out: String::new(),
            no_empty_tags: false,
            no_decl: false,
            encoding: None,
        }
    }

    fn ns_href(&self, n: NodeId) -> Option<&'a str> {
        let t = self.t;
        t.nodes[n].ns.map(|x| t.nss[x].href.as_str())
    }

    fn ns_prefix(&self, n: NodeId) -> Option<&'a str> {
        let t = self.t;
        t.nodes[n].ns.and_then(|x| t.nss[x].prefix.as_deref())
    }

    fn is_xmlns_ns(&self, n: NodeId) -> bool {
        self.ns_href(n) == Some(XMLNS_NS)
    }

    /// Recording the namespace information
    fn record_ns_info(
        &self,
        map: &mut PrefixMap,
        local: &mut LocalMap,
        element: NodeId,
    ) -> Option<String> {
        let t = self.t;
        let mut default_ns_attr_value: Option<String> = None;
        let mut a = t.nodes[element].properties;
        while let Some(attr) = a {
            a = t.nodes[attr].next;
            if !self.is_xmlns_ns(attr) {
                continue;
            }
            if self.ns_prefix(attr).is_none() {
                default_ns_attr_value = Some(attr_value(t, attr));
                continue;
            }
            let prefix_def = t.nodes[attr].name.clone();
            let ns_def = attr_value(t, attr);
            if ns_def == XML_XML_NAMESPACE {
                continue;
            }
            if map.found(&prefix_def, &ns_def) {
                continue;
            }
            let nsd = if ns_def.is_empty() {
                None
            } else {
                Some(ns_def.as_str())
            };
            map.add(&prefix_def, nsd);
            local.add(&prefix_def, nsd.unwrap_or(""));
        }
        default_ns_attr_value
    }

    fn attr_value_ser(&mut self, attr: NodeId) {
        let t = self.t;
        self.out.push_str(&t.nodes[attr].name);
        self.out.push_str("=\"");
        let mut c = t.nodes[attr].children;
        while let Some(x) = c {
            match t.nodes[x].t {
                NType::Text => text_ser(&mut self.out, &t.nodes[x].content, true),
                NType::EntityRef => {
                    self.out.push('&');
                    text_ser(&mut self.out, &t.nodes[x].name, true);
                    self.out.push(';');
                }
                _ => {}
            }
            c = t.nodes[x].next;
        }
        self.out.push('"');
    }

    #[allow(clippy::too_many_arguments)]
    fn serialize_attributes(
        &mut self,
        element: NodeId,
        map: &mut PrefixMap,
        local: &mut LocalMap,
        index: &mut u32,
        ignore_ns_def_attr: bool,
        wf: bool,
    ) -> R2 {
        let t = self.t;
        let mut localname_set: Vec<(Option<String>, String)> = Vec::new();
        let mut a = t.nodes[element].properties;
        while let Some(attr) = a {
            a = t.nodes[attr].next;
            if wf {
                let key = (
                    self.ns_href(attr).map(|s| s.to_string()),
                    t.nodes[attr].name.clone(),
                );
                if localname_set.contains(&key) {
                    return Err(());
                }
                localname_set.push(key);
            }
            let attr_ns = self.ns_href(attr);
            let mut candidate: Option<String> = None;
            if let Some(ans) = attr_ns {
                candidate = preferred_prefix(map, local, self.ns_prefix(attr), Some(ans));
                if self.is_xmlns_ns(attr) {
                    let v = attr_value(t, attr);
                    if v == XML_XML_NAMESPACE {
                        continue;
                    }
                    if ignore_ns_def_attr
                        && self.ns_prefix(attr).is_none()
                        && !eq_null_as_empty(self.ns_href(element), Some(&v))
                    {
                        continue;
                    }
                    if self.ns_prefix(attr).is_some() {
                        let lv = local.find(&t.nodes[attr].name);
                        if (lv.is_none() || lv != Some(v.as_str()))
                            && map.found(&t.nodes[attr].name, &v)
                        {
                            continue;
                        }
                    }
                    if self.ns_prefix(attr) == Some("xmlns") {
                        candidate = Some("xmlns".to_string());
                    }
                    if wf {
                        if v == XMLNS_NS {
                            return Err(());
                        }
                        if v.is_empty() && candidate.is_some() {
                            return Err(());
                        }
                    }
                } else if candidate.is_none() {
                    match self.ns_prefix(attr) {
                        Some(ap) if !local.contains(ap) => {
                            let p = ap.to_string();
                            map.add(&p, Some(ans));
                            local.add(&p, ans);
                            candidate = Some(p);
                        }
                        _ => candidate = Some(generate_prefix(map, local, Some(ans), index)),
                    }
                    self.out.push_str(" xmlns:");
                    self.out.push_str(candidate.as_deref().unwrap());
                    self.out.push_str("=\"");
                    text_ser(&mut self.out, ans, true);
                    self.out.push('"');
                }
            }
            self.out.push(' ');
            if let Some(c) = &candidate {
                self.out.push_str(c);
                self.out.push(':');
            }
            if wf {
                let name = &t.nodes[attr].name;
                if !crate::parser::is_valid_ncname(name) || (name == "xmlns" && attr_ns.is_none()) {
                    return Err(());
                }
            }
            self.attr_value_ser(attr);
        }
        Ok(())
    }

    fn should_format(&self, element: NodeId) -> bool {
        let t = self.t;
        let mut c = t.nodes[element].children;
        while let Some(x) = c {
            if matches!(t.nodes[x].t, NType::Text | NType::EntityRef | NType::CData) {
                return false;
            }
            c = t.nodes[x].next;
        }
        true
    }

    fn indents(&mut self, indent: i32) {
        self.out.push('\n');
        for _ in 0..indent {
            self.out.push_str("  ");
        }
    }

    fn element(
        &mut self,
        namespace: Option<&str>,
        prefix_map: &PrefixMap,
        element: NodeId,
        index: &mut u32,
        mut indent: i32,
        wf: bool,
    ) -> R2 {
        let t = self.t;
        let name = t.nodes[element].name.as_str();
        if wf && !crate::parser::is_valid_ncname(name) {
            return Err(());
        }
        let should_format =
            indent >= 0 && t.nodes[element].children.is_some() && self.should_format(element);
        self.out.push('<');
        let mut qprefix: Option<String> = None;
        let mut skip_end_tag = false;
        let mut ignore_ns_def_attr = false;
        let mut map = prefix_map.clone();
        let mut local = LocalMap::default();
        let local_default_ns = self.record_ns_info(&mut map, &mut local, element);
        let mut inherited_ns: Option<String> = namespace.map(|s| s.to_string());
        let ns = self.ns_href(element);
        if inherited_ns.as_deref() == ns {
            if local_default_ns.is_some() {
                ignore_ns_def_attr = true;
            }
            if ns == Some(XML_XML_NAMESPACE) {
                qprefix = Some("xml".to_string());
            }
            write_qname(&mut self.out, qprefix.as_deref(), name);
        } else {
            let mut prefix = self.ns_prefix(element).map(|s| s.to_string());
            let mut candidate =
                if prefix.is_none() && eq_null_as_empty(ns, local_default_ns.as_deref()) {
                    None
                } else {
                    preferred_prefix(&map, &local, prefix.as_deref(), ns)
                };
            if prefix.as_deref() == Some("xmlns") {
                if wf {
                    return Err(());
                }
                candidate = prefix.clone();
            }
            if let Some(c) = candidate {
                qprefix = Some(c);
                if let Some(ld) = &local_default_ns {
                    if ld != XML_XML_NAMESPACE {
                        inherited_ns = if ld.is_empty() {
                            None
                        } else {
                            Some(ld.clone())
                        };
                    }
                }
                write_qname(&mut self.out, qprefix.as_deref(), name);
            } else if let Some(p) = prefix.clone() {
                if local.contains(&p) {
                    prefix = Some(generate_prefix(&mut map, &mut local, ns, index));
                } else {
                    map.add(&p, ns);
                    local.add(&p, ns.unwrap_or(""));
                }
                let p = prefix.unwrap();
                qprefix = Some(p.clone());
                write_qname(&mut self.out, qprefix.as_deref(), name);
                self.out.push_str(" xmlns:");
                self.out.push_str(&p);
                self.out.push_str("=\"");
                text_ser(&mut self.out, ns.unwrap_or(""), true);
                self.out.push('"');
                if let Some(ld) = &local_default_ns {
                    inherited_ns = if ld.is_empty() {
                        None
                    } else {
                        Some(ld.clone())
                    };
                }
            } else if local_default_ns.is_none()
                || !eq_null_as_empty(local_default_ns.as_deref(), ns)
            {
                ignore_ns_def_attr = true;
                inherited_ns = ns.map(|s| s.to_string());
                write_qname(&mut self.out, None, name);
                self.out.push_str(" xmlns=\"");
                text_ser(&mut self.out, ns.unwrap_or(""), true);
                self.out.push('"');
            } else {
                inherited_ns = ns.map(|s| s.to_string());
                write_qname(&mut self.out, None, name);
            }
        }
        self.serialize_attributes(element, &mut map, &mut local, index, ignore_ns_def_attr, wf)?;
        if t.nodes[element].children.is_none() {
            if self.no_empty_tags {
                // <x></x>
            } else if ns == Some(XHTML_NS) {
                if HTML_VOID.contains(&name) {
                    self.out.push_str(" /");
                    skip_end_tag = true;
                }
            } else {
                self.out.push('/');
                skip_end_tag = true;
            }
        }
        self.out.push('>');
        if !skip_end_tag {
            if should_format {
                indent += 1;
            } else {
                indent = -1;
            }
            let mut c = if ns == Some(XHTML_NS) && name == "template" {
                t.nodes[element].template.and_then(|f| t.nodes[f].children)
            } else {
                t.nodes[element].children
            };
            while let Some(x) = c {
                if should_format {
                    self.indents(indent);
                }
                self.node(&map, x, inherited_ns.as_deref(), index, indent, wf)?;
                c = t.nodes[x].next;
            }
            if should_format {
                indent -= 1;
                self.indents(indent);
            }
            self.out.push_str("</");
            write_qname(&mut self.out, qprefix.as_deref(), name);
            self.out.push('>');
        }
        Ok(())
    }

    fn xml_decl(&mut self, doc: NodeId) {
        if self.no_decl {
            return;
        }
        let info = self.t.doc_info(doc).cloned().unwrap_or_default();
        let enc = self.encoding.clone().or(info.encoding.clone());
        self.out.push_str("<?xml version=\"");
        self.out.push_str(info.version.as_deref().unwrap_or("1.0"));
        self.out.push('"');
        if let Some(e) = enc {
            self.out.push_str(" encoding=\"");
            self.out.push_str(&e);
            self.out.push('"');
        }
        match info.standalone {
            0 => self.out.push_str(" standalone=\"no\""),
            1 => self.out.push_str(" standalone=\"yes\""),
            _ => {}
        }
        self.out.push_str("?>\n");
    }

    fn libxml_tree(&mut self, n: NodeId) {
        let mut ctx = SaveCtx::new(self.encoding.as_deref().or(Some("UTF-8")), false);
        if self.t.nodes[n].t == NType::Dtd {
            serialize::dump_dtd(self.t, &mut ctx, n);
        } else {
            serialize::dump_node(self.t, &mut ctx, n);
        }
        self.out.push_str(&ctx.out);
    }

    fn node(
        &mut self,
        map: &PrefixMap,
        n: NodeId,
        namespace: Option<&str>,
        index: &mut u32,
        indent: i32,
        wf: bool,
    ) -> R2 {
        let t = self.t;
        match t.nodes[n].t {
            NType::Element => self.element(namespace, map, n, index, indent, wf),
            NType::DocumentFrag => {
                let mut c = t.nodes[n].children;
                while let Some(x) = c {
                    self.node(map, x, namespace, index, indent, wf)?;
                    c = t.nodes[x].next;
                }
                Ok(())
            }
            NType::Document | NType::HtmlDocument => {
                self.xml_decl(n);
                let mut c = t.nodes[n].children;
                while let Some(x) = c {
                    self.node(map, x, namespace, index, indent, wf)?;
                    c = t.nodes[x].next;
                }
                Ok(())
            }
            NType::Text => {
                let content = &t.nodes[n].content;
                if wf {
                    check_char_production(content)?;
                }
                text_ser(&mut self.out, content, false);
                Ok(())
            }
            NType::Comment => {
                let content = &t.nodes[n].content;
                if wf {
                    check_char_production(content)?;
                    if content.contains("--") || content.ends_with('-') {
                        return Err(());
                    }
                }
                if t.nodes[n].flags & tree::F_NULL_CONTENT != 0 && content.is_empty() {
                    self.out.push_str("<!---->");
                    return Ok(());
                }
                self.out.push_str("<!--");
                self.out.push_str(content);
                self.out.push_str("-->");
                Ok(())
            }
            NType::Pi => {
                let name = &t.nodes[n].name;
                let content = &t.nodes[n].content;
                if wf {
                    if name.contains(':') || name.eq_ignore_ascii_case("xml") {
                        return Err(());
                    }
                    check_char_production(content)?;
                    if content.contains("?>") {
                        return Err(());
                    }
                }
                self.out.push_str("<?");
                self.out.push_str(name);
                self.out.push(' ');
                self.out.push_str(content);
                self.out.push_str("?>");
                Ok(())
            }
            NType::CData => {
                self.out.push_str("<![CDATA[");
                self.out.push_str(&t.nodes[n].content);
                self.out.push_str("]]>");
                Ok(())
            }
            NType::Attribute => {
                if let Some(p) = self.ns_prefix(n) {
                    self.out.push_str(p);
                    self.out.push(':');
                }
                self.attr_value_ser(n);
                Ok(())
            }
            NType::Dtd => {
                self.libxml_tree(n);
                self.out.push('\n');
                Ok(())
            }
            _ => {
                self.libxml_tree(n);
                Ok(())
            }
        }
    }
}

/// dom_xml_serialize: the serialization of `node` (UTF-8). `Err` when the
/// well-formedness requirements are not met.
pub fn serialize(
    t: &Tree,
    node: NodeId,
    format: bool,
    require_well_formed: bool,
    no_empty_tags: bool,
    no_decl: bool,
    encoding: Option<&str>,
) -> Result<String, ()> {
    let mut s = Ser::new(t);
    s.no_empty_tags = no_empty_tags;
    s.no_decl = no_decl;
    s.encoding = encoding.map(|x| x.to_string());
    let mut map = PrefixMap::default();
    map.add("xml", Some(XML_XML_NAMESPACE));
    let mut index = 1u32;
    let indent = if format { 0 } else { -1 };
    s.node(&map, node, None, &mut index, indent, require_well_formed)?;
    Ok(s.out)
}

/// Serialize the children of `node` (innerHTML getter for XML documents).
pub fn serialize_children(t: &Tree, node: NodeId, require_well_formed: bool) -> Result<String, ()> {
    let mut s = Ser::new(t);
    let mut map = PrefixMap::default();
    map.add("xml", Some(XML_XML_NAMESPACE));
    let mut index = 1u32;
    let mut c = t.nodes[node].children;
    while let Some(x) = c {
        s.node(&map, x, None, &mut index, -1, require_well_formed)?;
        c = t.nodes[x].next;
    }
    Ok(s.out)
}

/// php_new_dom_dump_node_to_str / php_new_dom_dump_doc_to_str: what other
/// extensions (SimpleXML) get when serializing nodes of a modern document.
pub fn dump_for_handlers(t: &Tree, node: NodeId, encoding: Option<&str>) -> Vec<u8> {
    match serialize(t, node, false, false, false, false, encoding) {
        Ok(out) => crate::serialize::encode_output(&out, encoding),
        Err(()) => Vec::new(),
    }
}
