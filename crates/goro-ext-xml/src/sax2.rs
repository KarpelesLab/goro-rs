//! Tree builder (libxml2 SAX2.c equivalent) and parse entry points.

use crate::parser::*;
use crate::tree::{self, AttrDecl, EntityType, Extra, NType, NodeId, Tree};

pub struct TreeBuilder {
    pub doc: Option<NodeId>,
    pub stack: Vec<NodeId>,
    pub options: i64,
    pub html: bool,
    /// Root used when parsing a content chunk.
    pub chunk_root: Option<NodeId>,
    /// Errors collected (in order).
    pub errors: Vec<XmlError>,
    pub in_entity: Vec<NodeId>,
    pub url: Option<String>,
}

impl TreeBuilder {
    pub fn new(options: i64) -> Self {
        TreeBuilder {
            doc: None,
            stack: Vec::new(),
            options,
            html: false,
            chunk_root: None,
            errors: Vec::new(),
            in_entity: Vec::new(),
            url: None,
        }
    }

    fn node(&self) -> Option<NodeId> {
        self.stack.last().copied()
    }

    fn add_text(&self, t: &mut Tree, text: &str, cdata: bool, line: u32) {
        let parent = match self.node() {
            Some(p) => p,
            None => return,
        };
        if let Some(last) = t.nodes[parent].last {
            if !cdata && t.nodes[last].t == NType::Text && t.nodes[last].name == "text" {
                t.nodes[last].content.push_str(text);
                return;
            }
        }
        let doc = self.doc;
        let n = if cdata {
            t.new_cdata(doc, text)
        } else {
            t.new_text(doc, text)
        };
        t.nodes[n].line = line;
        t.link_child_raw(parent, n);
    }

    fn add_misc(&self, t: &mut Tree, n: NodeId, in_subset: u8) {
        let doc = match self.doc {
            Some(d) => d,
            None => return,
        };
        if in_subset == 1 {
            if let Some(dtd) = t.int_subset(doc) {
                t.link_child_raw(dtd, n);
            }
            return;
        } else if in_subset == 2 {
            if let Some(dtd) = t.doc_info(doc).and_then(|d| d.ext_subset) {
                t.link_child_raw(dtd, n);
            }
            return;
        }
        match self.node() {
            None => t.link_child_raw(doc, n),
            Some(p) => {
                if t.nodes[p].t == NType::Element
                    || t.nodes[p].t == NType::EntityDecl
                    || t.nodes[p].t == NType::DocumentFrag
                {
                    t.add_child(p, n);
                } else {
                    t.link_child_raw(p, n);
                }
            }
        }
    }

    fn dtd(&self, t: &Tree) -> Option<NodeId> {
        self.doc.and_then(|d| t.int_subset(d))
    }
}

impl Sink for TreeBuilder {
    fn builds_tree(&self) -> bool {
        true
    }

    fn start_document(&mut self, version: &str, _encoding: Option<&str>, standalone: i32) {
        if self.chunk_root.is_some() {
            return;
        }
        let html = self.html;
        let url = self.url.clone();
        let opts = self.options;
        let d = tree::with(|t| {
            let d = if html {
                t.new_html_doc()
            } else {
                t.new_doc(Some(version))
            };
            if let Some(di) = t.doc_info_mut(d) {
                di.standalone = standalone;
                di.url = url;
                di.parse_options = opts;
            }
            d
        });
        self.doc = Some(d);
    }

    fn end_document(&mut self, encoding: Option<&str>) {
        if let (Some(d), Some(e)) = (self.doc, encoding) {
            tree::with(|t| {
                if let Some(di) = t.doc_info_mut(d) {
                    if di.encoding.is_none() {
                        di.encoding = Some(e.to_string());
                    }
                }
            });
        }
    }

    fn internal_subset(&mut self, name: Option<&str>, ext_id: Option<&str>, sys_id: Option<&str>) {
        let doc = self.doc;
        tree::with(|t| {
            if let Some(d) = doc {
                if let Some(old) = t.int_subset(d) {
                    t.unlink(old);
                }
            }
            t.create_int_subset(doc, name.unwrap_or(""), ext_id, sys_id);
        });
    }

    fn entity_decl(
        &mut self,
        name: &str,
        kind: EntKind,
        public: Option<&str>,
        system: Option<&str>,
        content: Option<&str>,
        orig: Option<&str>,
    ) {
        let dtd = match tree::with(|t| self.dtd(t)) {
            Some(d) => d,
            None => return,
        };
        let et = match kind {
            EntKind::InternalGeneral => EntityType::InternalGeneral,
            EntKind::ExternalGeneralParsed => EntityType::ExternalGeneralParsed,
            EntKind::ExternalGeneralUnparsed => EntityType::ExternalGeneralUnparsed,
            EntKind::InternalParameter => EntityType::InternalParameter,
            EntKind::ExternalParameter => EntityType::ExternalParameter,
            EntKind::Predefined => EntityType::InternalPredefined,
        };
        tree::with(|t| {
            if let Some(e) = t.add_entity(dtd, name, et, public, system, content, None) {
                if let Extra::Entity(info) = &mut t.nodes[e].extra {
                    info.orig = orig.map(|s| s.to_string());
                }
            }
        });
    }

    fn unparsed_entity_decl(
        &mut self,
        name: &str,
        public: Option<&str>,
        system: Option<&str>,
        notation: &str,
    ) {
        let dtd = match tree::with(|t| self.dtd(t)) {
            Some(d) => d,
            None => return,
        };
        tree::with(|t| {
            if let Some(e) = t.add_entity(
                dtd,
                name,
                EntityType::ExternalGeneralUnparsed,
                public,
                system,
                None,
                Some(notation),
            ) {
                t.nodes[e].content = notation.to_string();
            }
        });
    }

    fn notation_decl(&mut self, name: &str, public: Option<&str>, system: Option<&str>) {
        tree::with(|t| {
            if let Some(dtd) = self.dtd(t) {
                if let Some(d) = t.dtd_info_mut(dtd) {
                    if !d.notations.iter().any(|(n, _, _)| n == name) {
                        d.notations.push((
                            name.to_string(),
                            public.map(|s| s.to_string()),
                            system.map(|s| s.to_string()),
                        ));
                    }
                }
            }
        });
    }

    fn element_decl(&mut self, decl: &ElementDecl) {
        tree::with(|t| {
            if let Some(dtd) = self.dtd(t) {
                // duplicates are ignored
                for c in t.children_of(dtd) {
                    if t.nodes[c].t == NType::ElementDecl && t.nodes[c].name == decl.name {
                        return;
                    }
                }
                let doc = self.doc;
                let n = t.new_node(doc, NType::ElementDecl, &decl.name);
                let content = match decl.etype {
                    1 => "EMPTY".to_string(),
                    2 => "ANY".to_string(),
                    _ => decl
                        .content
                        .as_ref()
                        .map(|c| c.to_decl_string())
                        .unwrap_or_default(),
                };
                t.nodes[n].atype = decl.etype;
                t.nodes[n].extra = Extra::ElementDecl(content);
                t.link_child_raw(dtd, n);
            }
        });
    }

    fn attribute_decl(&mut self, decl: &AttrDeclInfo) {
        tree::with(|t| {
            if let Some(dtd) = self.dtd(t) {
                let (prefix, name) = match decl.name.find(':') {
                    Some(i) => (
                        Some(decl.name[..i].to_string()),
                        decl.name[i + 1..].to_string(),
                    ),
                    None => (None, decl.name.clone()),
                };
                if let Some(d) = t.dtd_info(dtd) {
                    if d.attr_defaults
                        .iter()
                        .any(|a| a.elem == decl.elem && a.name == name && a.prefix == prefix)
                    {
                        return;
                    }
                }
                let atype = match decl.atype {
                    1 => "CDATA".to_string(),
                    2 => "ID".to_string(),
                    3 => "IDREF".to_string(),
                    4 => "IDREFS".to_string(),
                    5 => "ENTITY".to_string(),
                    6 => "ENTITIES".to_string(),
                    7 => "NMTOKEN".to_string(),
                    8 => "NMTOKENS".to_string(),
                    9 => format!("({})", decl.tree.join(" | ")),
                    _ => format!("NOTATION ({})", decl.tree.join(" | ")),
                };
                let def = match decl.def {
                    2 => "#REQUIRED",
                    3 => "#IMPLIED",
                    4 => "#FIXED",
                    _ => "",
                }
                .to_string();
                let ad = AttrDecl {
                    elem: decl.elem.clone(),
                    name: name.clone(),
                    prefix: prefix.clone(),
                    atype,
                    def,
                    default: decl.default.clone(),
                };
                let doc = self.doc;
                let n = t.new_node(doc, NType::AttributeDecl, &name);
                t.nodes[n].atype = decl.atype;
                t.nodes[n].extra = Extra::AttributeDecl(Box::new(ad.clone()));
                t.link_child_raw(dtd, n);
                if let Some(d) = t.dtd_info_mut(dtd) {
                    d.attr_defaults.push(ad);
                }
            }
        });
    }

    fn start_element(
        &mut self,
        p: &mut ParserState,
        localname: &str,
        prefix: Option<&str>,
        uri: Option<&str>,
        nsdecls: &[(Option<String>, String)],
        attrs: &[SaxAttr],
        nb_defaulted: usize,
        line: u32,
    ) {
        let doc = self.doc;
        let parent = self.node();
        let replace = p.replace_entities;
        let opts = self.options;
        let html = self.html;
        let mut id_errors: Vec<String> = Vec::new();
        let mut warn_undef: Option<String> = None;
        let el = tree::with(|t| {
            let name = if prefix.is_some() && uri.is_none() {
                format!("{}:{}", prefix.unwrap(), localname)
            } else {
                localname.to_string()
            };
            let el = t.new_element(doc, &name);
            t.nodes[el].line = line;
            match parent {
                None => {
                    if let Some(d) = doc {
                        t.link_child_raw(d, el);
                    }
                }
                Some(pn) => {
                    t.link_child_raw(pn, el);
                }
            }
            for (pref, u) in nsdecls {
                let ns = t.new_ns_raw(u, pref.as_deref(), Some(el));
                t.nodes[el].ns_def.push(ns);
                if uri.is_some() && pref.as_deref() == prefix {
                    t.nodes[el].ns = Some(ns);
                }
            }
            if uri.is_some() && t.nodes[el].ns.is_none() {
                let ns = t.search_ns(el, prefix);
                match ns {
                    Some(ns) => t.nodes[el].ns = Some(ns),
                    None => {
                        let ns = t.new_ns_raw(uri.unwrap(), prefix, Some(el));
                        t.nodes[el].ns_def.push(ns);
                        t.nodes[el].ns = Some(ns);
                        warn_undef = Some(prefix.unwrap_or("").to_string());
                    }
                }
            }
            let nb = if nb_defaulted != 0 && opts & XML_PARSE_DTDATTR == 0 {
                attrs.len() - nb_defaulted
            } else {
                attrs.len()
            };
            for a in &attrs[..nb] {
                let (aname, ns) = if a.prefix.is_some() && a.uri.is_none() {
                    (
                        format!("{}:{}", a.prefix.as_deref().unwrap(), a.localname),
                        None,
                    )
                } else if let Some(pr) = &a.prefix {
                    (a.localname.clone(), t.search_ns(el, Some(pr)))
                } else {
                    (a.localname.clone(), None)
                };
                let at = t.new_node(doc, NType::Attribute, &aname);
                t.nodes[at].ns = ns;
                t.link_child_raw(el, at);
                if !replace && !html && a.value.contains('&') {
                    parse_att_value_children(t, doc, at, &a.value);
                } else {
                    let tx = t.new_text(doc, &a.value);
                    t.link_child_raw(at, tx);
                }
                // IDs
                let single_text = {
                    let c = t.nodes[at].children;
                    c.is_some()
                        && t.nodes[c.unwrap()].t == NType::Text
                        && t.nodes[c.unwrap()].next.is_none()
                };
                if single_text {
                    let content = t.node_get_content(at);
                    let is_xml_id = a.prefix.as_deref() == Some("xml") && a.localname == "id";
                    let is_id = is_xml_id || is_id_attr(t, doc, el, at, html);
                    if is_id {
                        if let Some(d) = doc {
                            t.nodes[at].atype = 2;
                            let exists = t
                                .doc_info(d)
                                .map(|di| di.ids.contains_key(&content))
                                .unwrap_or(false);
                            if exists {
                                id_errors.push(content.clone());
                            } else if let Some(di) = t.doc_info_mut(d) {
                                di.ids.insert(content, at);
                            }
                        }
                    }
                }
            }
            el
        });
        if let Some(pfx) = warn_undef {
            let _ = p
                .report(
                    FROM_NAMESPACE,
                    XML_NS_ERR_UNDEFINED_NAMESPACE,
                    ERR_WARNING,
                    format!("Namespace prefix {} was not found\n", pfx),
                )
                .map(|e| self.errors.push(e));
        }
        for v in id_errors {
            if let Some(e) = p.report(
                FROM_VALID,
                XML_DTD_ID_REDEFINED,
                ERR_ERROR,
                format!("ID {} already defined\n", v),
            ) {
                self.errors.push(e);
            }
        }
        self.stack.push(el);
    }

    fn end_element(
        &mut self,
        _p: &mut ParserState,
        _localname: &str,
        _prefix: Option<&str>,
        _uri: Option<&str>,
    ) {
        self.stack.pop();
    }

    fn characters(&mut self, p: &mut ParserState, text: &str) {
        let line = p.cur_line();
        tree::with(|t| self.add_text(t, text, false, line));
    }

    fn ignorable_whitespace(&mut self, _p: &mut ParserState, _text: &str) {}

    fn cdata_block(&mut self, p: &mut ParserState, text: &str) {
        let line = p.cur_line();
        tree::with(|t| self.add_text(t, text, true, line));
    }

    fn comment(&mut self, p: &mut ParserState, text: &str) {
        let line = p.cur_line();
        let in_subset = p.in_subset;
        let doc = self.doc;
        tree::with(|t| {
            let n = t.new_comment(doc, text);
            t.nodes[n].line = line;
            self.add_misc(t, n, in_subset);
        });
    }

    fn pi(&mut self, p: &mut ParserState, target: &str, data: Option<&str>) {
        let line = p.cur_line();
        let in_subset = p.in_subset;
        let doc = self.doc;
        tree::with(|t| {
            let n = t.new_pi(doc, target, data.unwrap_or(""));
            t.nodes[n].line = line;
            self.add_misc(t, n, in_subset);
        });
    }

    fn reference(&mut self, p: &mut ParserState, name: &str) {
        let parent = match self.node() {
            Some(n) => n,
            None => return,
        };
        let line = p.cur_line();
        let doc = self.doc;
        tree::with(|t| {
            let r = t.new_entity_ref(doc, name);
            t.nodes[r].line = line;
            t.link_child_raw(parent, r);
        });
    }

    fn begin_entity_content(&mut self, name: &str) -> bool {
        let doc = match self.doc {
            Some(d) => d,
            None => return false,
        };
        let ent = tree::with(|t| t.get_doc_entity(doc, name));
        match ent {
            Some(e) => {
                let empty = tree::with(|t| t.nodes[e].children.is_none());
                if empty && self.node().is_some() {
                    self.stack.push(e);
                    self.in_entity.push(e);
                    true
                } else {
                    false
                }
            }
            None => false,
        }
    }

    fn end_entity_content(&mut self, _name: &str) {
        if let Some(e) = self.in_entity.pop() {
            while let Some(top) = self.stack.pop() {
                if top == e {
                    break;
                }
            }
            tree::with(|t| {
                if let Some(c) = t.nodes[e].children {
                    let _ = c;
                }
            });
        }
    }

    fn are_blanks_tree(&self, next_lt: bool, next_lt_slash: bool, _elem_type: Option<u8>) -> bool {
        let node = match self.node() {
            Some(n) => n,
            None => return false,
        };
        tree::with(|t| {
            let n = &t.nodes[node];
            if n.children.is_none() && next_lt && next_lt_slash {
                return false;
            }
            match n.last {
                None => {
                    if n.t != NType::Element && !n.content.is_empty() {
                        return false;
                    }
                }
                Some(last) => {
                    if t.nodes[last].t == NType::Text {
                        return false;
                    }
                    if let Some(first) = n.children {
                        if t.nodes[first].t == NType::Text {
                            return false;
                        }
                    }
                }
            }
            true
        })
    }

    fn current_element(&self) -> Option<(String, Option<String>)> {
        let n = self.node()?;
        tree::with(|t| {
            let nd = &t.nodes[n];
            Some((nd.name.clone(), t.ns_prefix(nd.ns).map(|s| s.to_string())))
        })
    }

    fn error(&mut self, err: &XmlError) {
        self.errors.push(err.clone());
    }
}

/// xmlIsID: is the attribute declared as ID in the DTD (or "id" in HTML)?
fn is_id_attr(t: &Tree, doc: Option<NodeId>, el: NodeId, at: NodeId, html: bool) -> bool {
    let aname = &t.nodes[at].name;
    if html {
        return aname.eq_ignore_ascii_case("id")
            || (aname.eq_ignore_ascii_case("name") && t.nodes[el].name.eq_ignore_ascii_case("a"));
    }
    let d = match doc {
        Some(d) => d,
        None => return false,
    };
    let info = match t.doc_info(d) {
        Some(i) => i,
        None => return false,
    };
    let elem_q = t.qname(el);
    for sub in [info.int_subset, info.ext_subset].into_iter().flatten() {
        if let Some(dd) = t.dtd_info(sub) {
            for ad in &dd.attr_defaults {
                let full = match &ad.prefix {
                    Some(p) => format!("{}:{}", p, ad.name),
                    None => ad.name.clone(),
                };
                if ad.elem == elem_q && full == t.qname(at) && ad.atype == "ID" {
                    return true;
                }
            }
        }
    }
    false
}

/// xmlNodeParseAttValue: split "a&ent;b&#38;" into text and entity ref
/// children.
pub fn parse_att_value_children(t: &mut Tree, doc: Option<NodeId>, attr: NodeId, value: &str) {
    let b = value.as_bytes();
    let mut buf: Vec<u8> = Vec::new();
    let mut i = 0;
    let flush = |t: &mut Tree, buf: &mut Vec<u8>| {
        if !buf.is_empty() {
            let s = String::from_utf8_lossy(buf).into_owned();
            buf.clear();
            // merge into previous text
            if let Some(last) = t.nodes[attr].last {
                if t.nodes[last].t == NType::Text {
                    t.nodes[last].content.push_str(&s);
                    return;
                }
            }
            let tx = t.new_text(doc, &s);
            t.link_child_raw(attr, tx);
        }
    };
    while i < b.len() {
        if b[i] == b'&' {
            if i + 1 < b.len() && b[i + 1] == b'#' {
                let mut j = i + 2;
                let hex = j < b.len() && b[j] == b'x';
                if hex {
                    j += 1;
                }
                let s = j;
                while j < b.len() && b[j] != b';' {
                    j += 1;
                }
                let digits = std::str::from_utf8(&b[s..j]).unwrap_or("");
                let v = if hex {
                    u32::from_str_radix(digits, 16).unwrap_or(0)
                } else {
                    digits.parse().unwrap_or(0)
                };
                if let Some(c) = char::from_u32(v) {
                    let mut tmp = [0u8; 4];
                    buf.extend_from_slice(c.encode_utf8(&mut tmp).as_bytes());
                }
                i = j + 1;
                continue;
            }
            let mut j = i + 1;
            while j < b.len() && b[j] != b';' {
                j += 1;
            }
            let name = String::from_utf8_lossy(&b[i + 1..j]).into_owned();
            i = j + 1;
            let predef = match name.as_str() {
                "lt" => Some("<"),
                "gt" => Some(">"),
                "amp" => Some("&"),
                "apos" => Some("'"),
                "quot" => Some("\""),
                _ => None,
            };
            if let Some(p) = predef {
                buf.extend_from_slice(p.as_bytes());
                continue;
            }
            flush(t, &mut buf);
            let r = t.new_entity_ref(doc, &name);
            t.link_child_raw(attr, r);
            continue;
        }
        buf.push(b[i]);
        i += 1;
    }
    flush(t, &mut buf);
    if t.nodes[attr].children.is_none() {
        let tx = t.new_text(doc, "");
        t.link_child_raw(attr, tx);
    }
}

// ---------------------------------------------------------------------------
// Entry points

pub struct ParseOutcome {
    pub doc: Option<NodeId>,
    pub well_formed: bool,
    pub ns_well_formed: bool,
    pub errors: Vec<XmlError>,
}

/// Parse a whole document into the tree. The document is returned even if
/// not well-formed (callers decide; with RECOVER it is kept).
pub fn parse_doc(data: &[u8], url: Option<&str>, options: i64) -> ParseOutcome {
    let mut b = TreeBuilder::new(options);
    b.url = url.map(|s| s.to_string());
    let (wf, nswf, errors) = {
        let mut p = Parser::new(&mut b, data.to_vec(), url.map(|s| s.to_string()), options);
        let ok = p.parse_document();
        (ok, p.st.ns_well_formed, std::mem::take(&mut p.st.errors))
    };
    let _ = &b.errors;
    ParseOutcome {
        doc: b.doc,
        well_formed: wf,
        ns_well_formed: nswf,
        errors,
    }
}

/// Parse a balanced chunk in the context of `context` (an element or
/// document). Returns the list of top-level nodes (unlinked) or None if not
/// well-formed.
pub fn parse_in_context(
    data: &[u8],
    context: NodeId,
    options: i64,
) -> (Option<Vec<NodeId>>, Vec<XmlError>) {
    let (doc, nsscope, entities, has_dtd) = tree::with(|t| {
        let doc = t.nodes[context].doc;
        // in-scope namespaces
        let mut scope: Vec<(Option<String>, String)> = Vec::new();
        let mut cur = Some(context);
        let mut chain = Vec::new();
        while let Some(c) = cur {
            if t.nodes[c].t == NType::Element {
                chain.push(c);
            }
            cur = t.nodes[c].parent;
        }
        for c in chain.into_iter().rev() {
            for &ns in &t.nodes[c].ns_def {
                scope.push((t.nss[ns].prefix.clone(), t.nss[ns].href.clone()));
            }
        }
        let mut ents = Vec::new();
        let mut has_dtd = false;
        if let Some(d) = doc {
            if let Some(info) = t.doc_info(d) {
                for sub in [info.int_subset, info.ext_subset].into_iter().flatten() {
                    has_dtd = true;
                    if let Some(dd) = t.dtd_info(sub) {
                        for (n, e) in &dd.entities {
                            if let Some(ei) = t.entity_info(*e) {
                                let kind = match ei.etype {
                                    EntityType::InternalGeneral => EntKind::InternalGeneral,
                                    EntityType::ExternalGeneralParsed => {
                                        EntKind::ExternalGeneralParsed
                                    }
                                    EntityType::ExternalGeneralUnparsed => {
                                        EntKind::ExternalGeneralUnparsed
                                    }
                                    EntityType::InternalParameter => EntKind::InternalParameter,
                                    EntityType::ExternalParameter => EntKind::ExternalParameter,
                                    EntityType::InternalPredefined => EntKind::Predefined,
                                };
                                ents.push(PEntity {
                                    name: n.clone(),
                                    kind,
                                    content: t.nodes[*e].content.clone(),
                                    orig: None,
                                    public: None,
                                    system: None,
                                    notation: None,
                                    parsed: t.nodes[*e].children.is_some(),
                                    expanding: false,
                                });
                            }
                        }
                    }
                }
            }
        }
        (doc, scope, ents, has_dtd)
    });
    let mut b = TreeBuilder::new(options);
    b.doc = doc;
    let root = tree::with(|t| {
        let r = t.new_element(doc, "#root");
        // attach temporarily for namespace lookups
        t.nodes[r].parent = Some(context);
        r
    });
    b.chunk_root = Some(root);
    b.stack.push(root);
    let (wf, errors) = {
        let mut p = Parser::new(&mut b, data.to_vec(), None, options);
        for e in entities {
            p.add_known_entity(e);
        }
        if has_dtd {
            p.set_has_dtd();
        }
        let ok = p.parse_content_chunk(&nsscope);
        (ok, std::mem::take(&mut p.st.errors))
    };
    let list = tree::with(|t| {
        let kids = t.children_of(root);
        for k in &kids {
            t.unlink(*k);
        }
        t.nodes[root].parent = None;
        kids
    });
    if wf || options & XML_PARSE_RECOVER != 0 {
        (Some(list), errors)
    } else {
        (None, errors)
    }
}
