//! Tree builder (libxml2 SAX2.c equivalent) and parse entry points.

use crate::parser::*;
use crate::tree::{self, AttrDecl, EntityType, Extra, NType, NodeId, Tree};
use crate::valid::{self, Cx, Out, Validator};

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
    /// ctxt->validate (XML_PARSE_DTDVALID; cleared when there is no DTD).
    pub validate: bool,
    /// Validation state (vctxt) for parse-time validation.
    pub validator: Validator,
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
            validate: options & XML_PARSE_DTDVALID != 0,
            validator: Validator::default(),
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

    /// The subset declarations currently go to (ctxt->inSubset).
    fn subset_for(&self, t: &Tree, in_subset: u8) -> Option<NodeId> {
        let d = self.doc?;
        if in_subset == 2 {
            t.doc_info(d).and_then(|i| i.ext_subset)
        } else {
            t.int_subset(d)
        }
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

    fn end_document_p(&mut self, p: &mut ParserState, encoding: Option<&str>) {
        if let Some(d) = self.doc {
            if self.validate && p.well_formed && self.chunk_root.is_none() {
                let v = &mut self.validator;
                tree::with(|t| {
                    if t.int_subset(d).is_some() {
                        let mut cx = Cx::new(t, v, Out::Parser(p), d);
                        if !cx.validate_document_final() {
                            p.valid = false;
                        }
                    }
                });
            }
        }
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

    fn external_subset(&mut self, name: Option<&str>, ext_id: Option<&str>, sys_id: Option<&str>) {
        // xmlNewDtd(doc, ...): the external subset is not linked in the tree
        let doc = match self.doc {
            Some(d) => d,
            None => return,
        };
        tree::with(|t| {
            let id = t.new_node(Some(doc), NType::Dtd, name.unwrap_or(""));
            t.nodes[id].extra = Extra::Dtd(Box::new(tree::DtdInfo {
                external_id: ext_id.map(|s| s.to_string()),
                system_id: sys_id.map(|s| s.to_string()),
                ..Default::default()
            }));
            if let Some(di) = t.doc_info_mut(doc) {
                di.ext_subset = Some(id);
            }
        });
    }

    fn wants_external_subset(&self) -> bool {
        true
    }

    fn entity_decl_p(
        &mut self,
        p: &mut ParserState,
        name: &str,
        kind: EntKind,
        public: Option<&str>,
        system: Option<&str>,
        content: Option<&str>,
        orig: Option<&str>,
    ) {
        let dtd = match tree::with(|t| self.subset_for(t, p.in_subset)) {
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

    fn unparsed_entity_decl_p(
        &mut self,
        p: &mut ParserState,
        name: &str,
        public: Option<&str>,
        system: Option<&str>,
        notation: &str,
    ) {
        let dtd = match tree::with(|t| self.subset_for(t, p.in_subset)) {
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

    fn notation_decl_p(
        &mut self,
        p: &mut ParserState,
        name: &str,
        public: Option<&str>,
        system: Option<&str>,
    ) {
        if public.is_none() && system.is_none() {
            return;
        }
        let in_subset = p.in_subset;
        let dup = tree::with(|t| {
            let dtd = self.subset_for(t, in_subset)?;
            let d = t.dtd_info_mut(dtd)?;
            if d.notations.iter().any(|(n, _, _)| n == name) {
                return Some(true);
            }
            d.notations.push((
                name.to_string(),
                public.map(|s| s.to_string()),
                system.map(|s| s.to_string()),
            ));
            Some(false)
        });
        if dup == Some(true) {
            let _ = p.report(
                FROM_VALID,
                valid::XML_DTD_NOTATION_REDEFINED,
                ERR_ERROR,
                format!("xmlAddNotationDecl: {} already defined\n", name),
            );
            p.valid = false;
        }
    }

    fn element_decl_p(&mut self, p: &mut ParserState, decl: &ElementDecl) {
        let doc = match self.doc {
            Some(d) => d,
            None => return,
        };
        let in_subset = p.in_subset;
        let validate = self.validate;
        let v = &mut self.validator;
        tree::with(|t| {
            let (int, ext) = valid::subsets(t, doc);
            let dtd = match if in_subset == 2 { ext } else { int } {
                Some(d) => d,
                None => return,
            };
            let (prefix, local) = match decl.name.find(':') {
                Some(i) if i > 0 => (
                    Some(decl.name[..i].to_string()),
                    decl.name[i + 1..].to_string(),
                ),
                _ => (None, decl.name.clone()),
            };
            // attributes declared (in the internal subset) before the
            // element was declared in the external one
            let mut old_attrs = Vec::new();
            if let Some(isub) = int.filter(|i| *i != dtd) {
                if let Some(i) = valid::find_elem(t, Some(isub), &local, prefix.as_deref()) {
                    let d = t.dtd_info_mut(isub).unwrap();
                    if d.elements[i].etype == valid::ET_UNDEFINED {
                        old_attrs = std::mem::take(&mut d.elements[i].attrs);
                        d.elements.remove(i);
                    }
                }
            }
            let idx = match valid::find_elem(t, Some(dtd), &local, prefix.as_deref()) {
                Some(i) => {
                    if t.dtd_info(dtd).unwrap().elements[i].etype != valid::ET_UNDEFINED {
                        let mut cx = Cx::new(t, v, Out::Parser(p), doc);
                        cx.err(
                            Some(dtd),
                            valid::XML_DTD_ELEM_REDEFINED,
                            ERR_ERROR,
                            format!("Redefinition of element {}\n", decl.name),
                        );
                        p.valid = false;
                        return;
                    }
                    i
                }
                None => {
                    let d = t.dtd_info_mut(dtd).unwrap();
                    d.elements.push(tree::ElemDecl {
                        name: local.clone(),
                        prefix: prefix.clone(),
                        attrs: old_attrs,
                        ..Default::default()
                    });
                    d.elements.len() - 1
                }
            };
            let n = t.new_node(Some(doc), NType::ElementDecl, &decl.name);
            let content = match decl.etype {
                1 => "EMPTY".to_string(),
                2 => "ANY".to_string(),
                _ => decl.content.as_ref().map(|c| c.dump()).unwrap_or_default(),
            };
            t.nodes[n].atype = decl.etype;
            t.nodes[n].extra = Extra::ElementDecl(content);
            t.link_child_raw(dtd, n);
            {
                let e = &mut t.dtd_info_mut(dtd).unwrap().elements[idx];
                e.etype = decl.etype;
                e.content = decl.content.clone();
                e.node = Some(n);
                e.model = None;
            }
            if validate && p.well_formed && int.is_some() {
                let mut cx = Cx::new(t, v, Out::Parser(p), doc);
                if !cx.validate_element_decl((dtd, idx)) {
                    p.valid = false;
                }
            }
        });
    }

    fn attribute_decl_p(&mut self, p: &mut ParserState, decl: &AttrDeclInfo) {
        let doc = match self.doc {
            Some(d) => d,
            None => return,
        };
        if decl.name == "xml:id" && decl.atype != valid::AT_ID {
            let ok = p.valid;
            let _ = p.report(
                FROM_DTD,
                valid::XML_DTD_XMLID_TYPE,
                ERR_ERROR,
                "xml:id : attribute type should be ID\n".into(),
            );
            p.valid = ok;
        }
        let in_subset = p.in_subset;
        let validate = self.validate;
        let v = &mut self.validator;
        tree::with(|t| {
            let (int, ext) = valid::subsets(t, doc);
            let dtd = match if in_subset == 2 { ext } else { int } {
                Some(d) => d,
                None => return,
            };
            let (prefix, name) = match decl.name.find(':') {
                Some(i) => (
                    Some(decl.name[..i].to_string()),
                    decl.name[i + 1..].to_string(),
                ),
                None => (None, decl.name.clone()),
            };
            let mut default = decl.default.clone();
            if let Some(dv) = &default {
                if !valid::validate_attribute_value(decl.atype, dv) {
                    let mut cx = Cx::new(t, v, Out::Parser(p), doc);
                    cx.err(
                        Some(dtd),
                        valid::XML_DTD_ATTRIBUTE_DEFAULT,
                        ERR_ERROR,
                        format!(
                            "Attribute {} of {}: invalid default value\n",
                            decl.elem, name
                        ),
                    );
                    default = None;
                    p.valid = false;
                }
            }
            if int.is_some()
                && Some(dtd) == ext
                && valid::find_attr(t, int, &decl.elem, &name, prefix.as_deref()).is_some()
            {
                return;
            }
            // the element entry (possibly XML_ELEMENT_TYPE_UNDEFINED)
            let (eprefix, elocal) = match decl.elem.find(':') {
                Some(i) if i > 0 => (
                    Some(decl.elem[..i].to_string()),
                    decl.elem[i + 1..].to_string(),
                ),
                _ => (None, decl.elem.clone()),
            };
            let eidx = match valid::find_elem(t, Some(dtd), &elocal, eprefix.as_deref()) {
                Some(i) => i,
                None => {
                    let d = t.dtd_info_mut(dtd).unwrap();
                    d.elements.push(tree::ElemDecl {
                        name: elocal.clone(),
                        prefix: eprefix.clone(),
                        ..Default::default()
                    });
                    d.elements.len() - 1
                }
            };
            if valid::find_attr(t, Some(dtd), &decl.elem, &name, prefix.as_deref()).is_some() {
                let mut cx = Cx::new(t, v, Out::Parser(p), doc);
                cx.err(
                    Some(dtd),
                    valid::XML_DTD_ATTRIBUTE_REDEFINED,
                    ERR_WARNING,
                    format!(
                        "Attribute {} of element {}: already defined\n",
                        name, decl.elem
                    ),
                );
                return;
            }
            if decl.atype == valid::AT_ID {
                // xmlScanIDAttributeDecl(ctxt, elemDef, 1)
                let ids: Vec<String> = t.dtd_info(dtd).unwrap().elements[eidx]
                    .attrs
                    .iter()
                    .filter_map(|(d, i)| t.dtd_info(*d).map(|x| &x.attr_defaults[*i]))
                    .filter(|a| a.atype_n == valid::AT_ID)
                    .map(|a| a.name.clone())
                    .collect();
                if !ids.is_empty() {
                    let mut cx = Cx::new(t, v, Out::Parser(p), doc);
                    for (k, an) in ids.iter().enumerate() {
                        if k >= 1 {
                            cx.err(
                                Some(dtd),
                                valid::XML_DTD_MULTIPLE_ID,
                                ERR_ERROR,
                                format!(
                                    "Element {} has too many ID attributes defined : {}\n",
                                    elocal, an
                                ),
                            );
                        }
                    }
                    cx.err(
                        Some(dtd),
                        valid::XML_DTD_MULTIPLE_ID,
                        ERR_ERROR,
                        format!(
                            "Element {} has too may ID attributes defined : {}\n",
                            decl.elem, name
                        ),
                    );
                    p.valid = false;
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
            let n = t.new_node(Some(doc), NType::AttributeDecl, &name);
            let ad = AttrDecl {
                elem: decl.elem.clone(),
                name: name.clone(),
                prefix: prefix.clone(),
                atype,
                def,
                default,
                atype_n: decl.atype,
                def_n: decl.def,
                tree: decl.tree.clone(),
                node: Some(n),
            };
            t.nodes[n].atype = decl.atype;
            t.nodes[n].extra = Extra::AttributeDecl(Box::new(ad.clone()));
            t.link_child_raw(dtd, n);
            let aidx = {
                let d = t.dtd_info_mut(dtd).unwrap();
                d.attr_defaults.push(ad);
                d.attr_defaults.len() - 1
            };
            // elem->attributes: namespace declarations first
            let is_xmlns = name == "xmlns" || prefix.as_deref() == Some("xmlns");
            let names: Vec<String> = t.dtd_info(dtd).unwrap().elements[eidx]
                .attrs
                .iter()
                .map(|(d, i)| {
                    t.dtd_info(*d)
                        .map(|x| x.attr_defaults[*i].name.clone())
                        .unwrap_or_default()
                })
                .collect();
            let list = &mut t.dtd_info_mut(dtd).unwrap().elements[eidx].attrs;
            if is_xmlns || list.is_empty() {
                list.insert(0, (dtd, aidx));
            } else {
                let mut k = 0;
                while names[k] == "xmlns" && k + 1 < names.len() {
                    k += 1;
                }
                list.insert(k + 1, (dtd, aidx));
            }
            if validate && p.well_formed && int.is_some() {
                let mut cx = Cx::new(t, v, Out::Parser(p), doc);
                if !cx.validate_attribute_decl((dtd, aidx)) {
                    p.valid = false;
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
        if self.validate {
            // "First check on validity"
            let no_dtd = tree::with(|t| {
                let d = match doc {
                    Some(d) => d,
                    None => return false,
                };
                let (int, ext) = valid::subsets(t, d);
                if ext.is_some() {
                    return false;
                }
                match int.and_then(|i| t.dtd_info(i)) {
                    None => true,
                    Some(di) => {
                        di.notations.is_empty()
                            && di.elements.is_empty()
                            && di.attr_defaults.is_empty()
                            && di.entities.is_empty()
                    }
                }
            });
            if no_dtd {
                let _ = p.report(
                    FROM_DTD,
                    valid::XML_DTD_NO_DTD,
                    ERR_ERROR,
                    "Validation failed: no DTD found !".into(),
                );
                p.valid = false;
                self.validate = false;
            }
        }
        let in_entity = p.inputs.last().map(|i| i.entity.is_some()).unwrap_or(false);
        // Entity content substituted into the tree (XML_PARSE_NOENT): libxml2
        // copies the entity's nodes, and xmlStaticCopyNode() registers their
        // IDs (quietly; an ID defined first wins).
        let copied_entity = in_entity && self.in_entity.is_empty();
        let mut warn_undef: Option<String> = None;
        let validate = self.validate && !html;
        let v = &mut self.validator;
        let el = tree::with(|t| {
            let name = match (prefix, uri) {
                (Some(p), None) => format!("{}:{}", p, localname),
                _ => localname.to_string(),
            };
            let el = t.new_element(doc, &name);
            t.nodes[el].line = line;
            let has_int = doc.and_then(|d| t.int_subset(d)).is_some();
            for (pref, u) in nsdecls {
                let ns = t.new_ns_raw(u, pref.as_deref(), Some(el));
                t.nodes[el].ns_def.push(ns);
                if uri.is_some() && pref.as_deref() == prefix {
                    t.nodes[el].ns = Some(ns);
                }
                if validate && p.well_formed && has_int {
                    let mut cx = Cx::new(t, v, Out::Parser(p), doc.unwrap());
                    if !cx.validate_one_namespace(el, prefix, ns, u) {
                        p.valid = false;
                    }
                }
            }
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
            if let (Some(u), None) = (uri, t.nodes[el].ns) {
                let ns = t.search_ns(el, prefix);
                match ns {
                    Some(ns) => t.nodes[el].ns = Some(ns),
                    None => {
                        let ns = t.new_ns_raw(u, prefix, Some(el));
                        t.nodes[el].ns_def.push(ns);
                        t.nodes[el].ns = Some(ns);
                        warn_undef = Some(prefix.unwrap_or("").to_string());
                    }
                }
            }
            if let Some(pfx) = warn_undef.take() {
                let _ = p.report(
                    FROM_NAMESPACE,
                    XML_NS_ERR_UNDEFINED_NAMESPACE,
                    ERR_WARNING,
                    format!("Namespace prefix {} was not found\n", pfx),
                );
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
                let d = match doc {
                    Some(d) => d,
                    None => continue,
                };
                if validate && p.well_formed && has_int {
                    // the value with entities substituted (and normalized
                    // again if declared non-CDATA)
                    let mut value = a.value.clone();
                    let mut cx = Cx::new(t, v, Out::Parser(p), d);
                    if !replace && a.value.contains('&') {
                        value = cx.t.node_get_content(at);
                        let full = match &a.prefix {
                            Some(pr) => format!("{}:{}", pr, a.localname),
                            None => a.localname.clone(),
                        };
                        if let Some(nv) = cx.normalize_attribute_value(el, &full, &value) {
                            value = nv;
                        }
                    }
                    let ok = cx.validate_one_attribute(el, at, &value) && cx.valid;
                    if !ok {
                        p.valid = false;
                    }
                    continue;
                }
                // IDs (non-validating)
                let single_text = {
                    let c = t.nodes[at].children;
                    c.is_some()
                        && t.nodes[c.unwrap()].t == NType::Text
                        && t.nodes[c.unwrap()].next.is_none()
                };
                if single_text && copied_entity {
                    let content = t.node_get_content(at);
                    let is_xml_id = a.prefix.as_deref() == Some("xml") && a.localname == "id";
                    if is_xml_id || is_id_attr(t, doc, el, at, html) {
                        t.add_id_safe(at, &content);
                    }
                } else if single_text && !in_entity {
                    let content = t.node_get_content(at);
                    let is_xml_id = a.prefix.as_deref() == Some("xml") && a.localname == "id";
                    if is_xml_id && !is_valid_ncname(&content) {
                        let _ = p.report(
                            FROM_DTD,
                            valid::XML_DTD_XMLID_VALUE,
                            ERR_ERROR,
                            format!("xml:id : attribute value {} is not an NCName\n", content),
                        );
                        p.valid = false;
                    }
                    let is_id = is_xml_id || is_id_attr(t, doc, el, at, html);
                    if is_id {
                        let mut cx = Cx::new(t, v, Out::Parser(p), d);
                        cx.add_id(at, &content);
                    }
                }
            }
            el
        });
        if self.validate && !self.validator.dtd_validated {
            if let Some(d) = doc {
                let v = &mut self.validator;
                tree::with(|t| {
                    let mut cx = Cx::new(t, v, Out::Parser(p), d);
                    let chk = cx.validate_dtd_final();
                    let root_ok = cx.validate_root();
                    if !chk || !root_ok {
                        p.valid = false;
                    }
                });
            }
            self.validator.dtd_validated = true;
        }
        self.stack.push(el);
    }

    fn end_element(
        &mut self,
        p: &mut ParserState,
        _localname: &str,
        _prefix: Option<&str>,
        _uri: Option<&str>,
    ) {
        if self.validate && p.well_formed {
            if let (Some(d), Some(cur)) = (self.doc, self.node()) {
                let v = &mut self.validator;
                tree::with(|t| {
                    if t.int_subset(d).is_some() {
                        let mut cx = Cx::new(t, v, Out::Parser(p), d);
                        if !cx.validate_one_element(cur) {
                            p.valid = false;
                        }
                    }
                });
            }
        }
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
    let attr_q = t.qname(at);
    for sub in [info.int_subset, info.ext_subset].into_iter().flatten() {
        if let Some(dd) = t.dtd_info(sub) {
            if dd.is_id_decl(&elem_q, &attr_q) {
                return true;
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
            // the entity content is parsed (as attribute content) on first use
            if let Some(d) = doc {
                if let Some(ent) = t.get_doc_entity(d, &name) {
                    let pending = match &t.nodes[ent].extra {
                        Extra::Entity(info) => !info.checked && t.nodes[ent].children.is_none(),
                        _ => false,
                    };
                    if pending {
                        if let Extra::Entity(info) = &mut t.nodes[ent].extra {
                            info.checked = true;
                        }
                        let content = t.nodes[ent].content.clone();
                        if !content.is_empty() {
                            parse_att_value_children(t, doc, ent, &content);
                        }
                    }
                }
            }
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
    parse_doc_ex(data, url, options, None)
}

/// `parse_doc` with a base directory for resolving external identifiers
/// when there is no URL (ctxt->directory).
pub fn parse_doc_ex(
    data: &[u8],
    url: Option<&str>,
    options: i64,
    directory: Option<&str>,
) -> ParseOutcome {
    parse_doc_enc(data, url, options, directory, None)
}

/// `parse_doc_ex` of an input whose encoding the transport gave (it
/// overrides what the document declares).
pub fn parse_doc_enc(
    data: &[u8],
    url: Option<&str>,
    options: i64,
    directory: Option<&str>,
    encoding: Option<&str>,
) -> ParseOutcome {
    let mut b = TreeBuilder::new(options);
    b.url = url.map(|s| s.to_string());
    let (wf, nswf, errors) = {
        let mut p = Parser::new(&mut b, data.to_vec(), url.map(|s| s.to_string()), options);
        if let Some(e) = encoding {
            p.switch_encoding(e);
        }
        p.st.directory = directory.map(|s| s.to_string());
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
                                    uri: None,
                                    content_start: 0,
                                    content_base: (1, 1),
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

/// xmlParseDTD(ExternalID, SystemID): load and parse a DTD as an external
/// subset. Returns the (unlinked) DTD node, owned by a scratch document,
/// and the diagnostics (formatted as libxml2's generic handler prints
/// them).
pub fn parse_dtd(public: Option<&str>, system: Option<&str>) -> (Option<NodeId>, Vec<XmlError>) {
    let sys = match system {
        Some(s) => s,
        None => return (None, Vec::new()),
    };
    let (data, fname) = match load_external(public, sys, sys) {
        Ok((d, f)) => (prepare_external(d).0, f.unwrap_or_else(|| sys.to_string())),
        Err(Some((crate::parser::EXT_LOAD_REPORTED, _))) => return (None, Vec::new()),
        Err(e) => {
            let (code, msg) = e.unwrap_or_else(|| {
                (
                    XML_IO_ENOENT,
                    format!("failed to load \"{}\": No such file or directory\n", sys),
                )
            });
            let mut err = XmlError::simple(FROM_IO, code, ERR_WARNING, msg.clone());
            err.formatted = vec![format_no_input(None, 0, None, FROM_IO, ERR_WARNING, &msg)];
            return (None, vec![err]);
        }
    };
    let mut b = TreeBuilder::new(0);
    let doc = tree::with(|t| {
        let d = t.new_doc(Some("1.0"));
        t.create_int_subset(Some(d), "none", public, system);
        d
    });
    b.doc = Some(doc);
    b.external_subset(Some("none"), public, system);
    let (wf, errors) = {
        let mut p = Parser::new(&mut b, data, Some(fname), 0);
        p.st.in_subset = 2;
        p.parse_external_subset();
        (p.st.well_formed, std::mem::take(&mut p.st.errors))
    };
    let dtd = if wf {
        tree::with(|t| t.doc_info(doc).and_then(|d| d.ext_subset))
    } else {
        None
    };
    (dtd, errors)
}
