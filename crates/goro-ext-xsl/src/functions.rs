//! XSLT functions (functions.c), keys (keys.c runtime) and documents
//! (documents.c runtime), plus the dispatch to extension functions.

use crate::transform::{DocInfo, State, Transform};
use crate::util::*;
use crate::xpath::{self, PCtx, XObj, XPATH_INVALID_ARITY, XPATH_INVALID_TYPE};
use goro_ext_xml::tree::{NType, NodeId};
use std::collections::HashMap;

/// Function call dispatch (xsltXPathFunctionLookup + the XSLT functions
/// registered in the XPath context). Returns false if unknown.
pub fn call(t: &mut Transform, p: &mut PCtx, name: &str, uri: Option<&str>, nargs: usize) -> bool {
    match uri {
        None => match name {
            "current" => current_function(t, p, nargs),
            "document" => document_function(t, p, nargs),
            "key" => key_function(t, p, nargs),
            "unparsed-entity-uri" => unparsed_entity_uri_function(t, p, nargs),
            "format-number" => format_number_function(t, p, nargs),
            "generate-id" => generate_id_function(t, p, nargs),
            "system-property" => system_property_function(t, p, nargs),
            "element-available" => element_available_function(t, p, nargs),
            "function-available" => function_available_function(t, p, nargs),
            _ => return false,
        },
        Some(u) => {
            // functions registered on the context (PHP registerPHPFunctionNS)
            if let Some(php) = t.php {
                // SAFETY: the host outlives the transformation it drives
                let host = unsafe { &mut *php };
                if host.has_function(name, u) {
                    host.call(t, p, name, u, nargs);
                    return true;
                }
            }
            // EXSLT func:function definitions
            if let Some(&f) = t
                .sheet
                .ext_functions
                .get(&(u.to_string(), name.to_string()))
            {
                crate::exslt::func_function_call(t, p, f, u, name, nargs);
                return true;
            }
            if crate::exslt::call(t, p, name, u, nargs) {
                return true;
            }
            match (u, name) {
                (crate::style::XSLT_LIBXSLT_NAMESPACE, "node-set")
                | (crate::style::XSLT_SAXON_NAMESPACE, "node-set")
                | (crate::style::XSLT_XT_NAMESPACE, "node-set") => {
                    crate::exslt::node_set_function(t, p, nargs);
                }
                _ => return false,
            }
        }
    }
    true
}

/// Is the function (name, uri) available (xmlXPathFunctionLookupNS)?
pub fn function_exists(t: &Transform, name: &str, uri: Option<&str>) -> bool {
    match uri {
        None => {
            matches!(
                name,
                "current"
                    | "document"
                    | "key"
                    | "unparsed-entity-uri"
                    | "format-number"
                    | "generate-id"
                    | "system-property"
                    | "element-available"
                    | "function-available"
            ) || xpath::is_core_function(name)
        }
        Some(u) => {
            if let Some(php) = t.php {
                let host = unsafe { &*php };
                if host.has_function(name, u) {
                    return true;
                }
            }
            if t.sheet
                .ext_functions
                .contains_key(&(u.to_string(), name.to_string()))
            {
                return true;
            }
            crate::exslt::exists(name, u)
                || matches!(
                    (u, name),
                    (crate::style::XSLT_LIBXSLT_NAMESPACE, "node-set")
                        | (crate::style::XSLT_SAXON_NAMESPACE, "node-set")
                        | (crate::style::XSLT_XT_NAMESPACE, "node-set")
                )
        }
    }
}

fn current_function(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    if nargs != 0 {
        t.error(None, "current() : function uses no argument\n");
        p.error = XPATH_INVALID_ARITY;
        return;
    }
    p.push(XObj::NodeSet(vec![t.node]));
}

/// xsltLoadDocument
pub fn load_document(t: &mut Transform, uri: &str) -> Option<usize> {
    if t.sec_prefs != 0 {
        let r = crate::security::check_read(t, uri);
        if r <= 0 {
            if r == 0 {
                t.error(
                    None,
                    &format!("xsltLoadDocument: read rights for {} denied\n", uri),
                );
            }
            return None;
        }
    }
    for (i, d) in t.docs.iter().enumerate() {
        if tw(|tr| doc_url(tr, d.doc)).as_deref() == Some(uri) {
            return Some(i);
        }
    }
    let opts = t.parser_options;
    let doc = crate::transform::load_xml_document(t.vm, uri, opts)?;
    if t.xinclude {
        let f = goro_ext_xml::dom::active_fn(t.vm);
        goro_ext_xml::xinclude::process(t.vm, &f, doc, opts);
    }
    if t.need_elem_space_handling() {
        let root = tw(|tr| tr.root_element(doc));
        if let Some(r) = root {
            t.apply_strip_spaces(r);
        }
    }
    t.docs.push(DocInfo {
        doc,
        ..Default::default()
    });
    Some(t.docs.len() - 1)
}

fn document_function_load_document(
    t: &mut Transform,
    p: &mut PCtx,
    uri: Option<&str>,
    fragment: Option<&str>,
) {
    let idoc = uri.and_then(|u| load_document(t, u));
    let doc = match idoc {
        Some(i) => t.docs[i].doc,
        None => {
            let style_doc = t.sheet.styles[0].doc;
            let style_url = tw(|tr| doc_url(tr, style_doc));
            if uri.is_none()
                || uri.map(|u| u.starts_with('#')).unwrap_or(false)
                || (style_url.is_some() && style_url.as_deref() == uri)
            {
                let copy = tw(|tr| tr.copy_doc(style_doc, true));
                t.docs.push(DocInfo {
                    doc: copy,
                    ..Default::default()
                });
                copy
            } else {
                p.push(XObj::NodeSet(Vec::new()));
                return;
            }
        }
    };
    match fragment {
        None => p.push(XObj::NodeSet(vec![doc])),
        Some(frag) => {
            // XPointer: only the xpointer()/element() basics and bare names
            let r = crate::xpointer::eval(t, doc, frag);
            match r {
                Some(XObj::NodeSet(v)) => p.push(XObj::NodeSet(v)),
                Some(_) => {
                    t.error(
                        None,
                        &format!(
                            "document() : XPointer does not select a node set: #{}\n",
                            frag
                        ),
                    );
                    p.push(XObj::NodeSet(Vec::new()));
                }
                None => p.push(XObj::NodeSet(Vec::new())),
            }
        }
    }
}

/// xsltDocumentFunction
pub fn document_function(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    if !(1..=2).contains(&nargs) {
        t.error(
            None,
            &format!("document() : invalid number of args {}\n", nargs),
        );
        p.error = XPATH_INVALID_ARITY;
        return;
    }
    if p.top().is_none() {
        t.error(None, "document() : invalid arg value\n");
        p.error = XPATH_INVALID_TYPE;
        return;
    }
    let mut obj2: Option<Vec<NodeId>> = None;
    if nargs == 2 {
        match p.top() {
            Some(XObj::NodeSet(_)) => {}
            _ => {
                t.error(None, "document() : invalid arg expecting a nodeset\n");
                p.error = XPATH_INVALID_TYPE;
                return;
            }
        }
        if let Some(XObj::NodeSet(v)) = p.pop() {
            obj2 = Some(v);
        }
    }
    if let Some(XObj::NodeSet(_)) = p.top() {
        let obj = match p.pop() {
            Some(XObj::NodeSet(v)) => v,
            _ => Vec::new(),
        };
        let mut ret: Vec<NodeId> = Vec::new();
        for n in obj {
            p.push(XObj::NodeSet(vec![n]));
            xpath::cast_top_string(p);
            match &obj2 {
                Some(o2) => p.push(XObj::NodeSet(o2.clone())),
                None => p.push(XObj::NodeSet(vec![n])),
            }
            if p.error != 0 {
                break;
            }
            document_function(t, p, 2);
            if let Some(newobj) = p.pop() {
                if let Some(v) = newobj.nodes() {
                    xpath::merge(&mut ret, v);
                }
            }
        }
        p.push(XObj::NodeSet(ret));
        return;
    }
    xpath::cast_top_string(p);
    let url = match p.pop() {
        Some(XObj::Str(s)) => s,
        _ => {
            t.error(None, "document() : invalid arg expecting a string\n");
            p.error = XPATH_INVALID_TYPE;
            return;
        }
    };
    let (url_nofrag, fragment) = match url.find('#') {
        Some(i) => {
            let frag = &url[i + 1..];
            (
                url[..i].to_string(),
                if frag.is_empty() {
                    None
                } else {
                    Some(frag.to_string())
                },
            )
        }
        None => (url.clone(), None),
    };
    if goro_ext_xml::uri::parse_uri(&url).is_none() {
        t.error(
            None,
            &format!("document() : failed to parse URI '{}'\n", url),
        );
        p.push(XObj::NodeSet(Vec::new()));
        return;
    }
    let base = match obj2.as_ref().and_then(|v| v.first().copied()) {
        Some(target) if tw(|tr| is_real_node(tr, target)) => tw(|tr| {
            let mut tg = target;
            if matches!(tr.nodes[tg].t, NType::Attribute | NType::Pi) {
                if let Some(pp) = tr.nodes[tg].parent {
                    tg = pp;
                }
            }
            node_get_base(tr, tg)
        }),
        _ => match t.inst {
            Some(i) => tw(|tr| node_get_base(tr, i)),
            None => {
                let sd = t.sheet.styles[0].doc;
                tw(|tr| node_get_base(tr, sd))
            }
        },
    };
    let uri = build_uri(&url_nofrag, base.as_deref());
    match uri {
        None => p.push(XObj::NodeSet(Vec::new())),
        Some(u) => document_function_load_document(t, p, Some(&u), fragment.as_deref()),
    }
}

impl<'v> Transform<'v> {
    /// The document info index for a node's document (xsltFindDocument,
    /// creating it for result tree fragments).
    pub fn document_for_node(&mut self, node: NodeId) -> Option<usize> {
        let doc = tw(|tr| {
            let n = if tr.nodes[node].t == NType::NamespaceDecl {
                tr.nodes[node].parent?
            } else {
                node
            };
            tr.nodes[n].doc
        })?;
        if let Some(i) = self.docs.iter().position(|d| d.doc == doc) {
            return Some(i);
        }
        let is_rtf = tw(|tr| xpath::is_rtf(tr, doc));
        if is_rtf {
            self.docs.push(DocInfo {
                doc,
                ..Default::default()
            });
            return Some(self.docs.len() - 1);
        }
        if doc == self.sheet.styles[0].doc {
            return self.document;
        }
        None
    }

    /// xsltComputeAllKeys
    pub fn compute_all_keys(&mut self, node: NodeId) -> Result<(), ()> {
        if self.document.is_none() {
            let d = self.document_for_node(node);
            self.document = d;
        }
        let doc_idx = match self.document_for_node(node) {
            Some(d) => d,
            None => return Err(()),
        };
        let old = self.document;
        self.document = Some(doc_idx);
        self.init_all_doc_keys();
        self.document = old;
        Ok(())
    }

    /// xsltInitAllDocKeys
    pub fn init_all_doc_keys(&mut self) {
        let di = match self.document {
            Some(d) => d,
            None => return,
        };
        if self.docs[di].nb_keys_computed == self.nb_keys {
            return;
        }
        let sheet = self.sheet.clone();
        for s in sheet.import_order() {
            for keyd in &sheet.styles[s].keys {
                let exists = self.docs[di]
                    .keys
                    .iter()
                    .any(|(k, _)| k.0 == keyd.name && k.1 == keyd.name_uri);
                if !exists {
                    self.init_doc_key_table(&keyd.name, keyd.name_uri.as_deref());
                }
            }
        }
    }

    /// xsltInitDocKeyTable
    fn init_doc_key_table(&mut self, name: &str, uri: Option<&str>) -> i32 {
        let sheet = self.sheet.clone();
        let mut found = false;
        let mut last_inst = None;
        for s in sheet.import_order() {
            for keyd in &sheet.styles[s].keys {
                last_inst = Some(keyd.inst);
                if keyd.name == name && keyd.name_uri.as_deref() == uri {
                    let di = self.document.unwrap();
                    self.init_ctxt_key(di, keyd);
                    if self.docs[di].nb_keys_computed == self.nb_keys {
                        return 0;
                    }
                    found = true;
                }
            }
        }
        if !found {
            self.error(
                last_inst,
                &format!("Failed to find key definition for {}\n", name),
            );
            self.state = State::Stopped;
            return -1;
        }
        0
    }

    /// xsltInitCtxtKey
    fn init_ctxt_key(&mut self, di: usize, keydef: &crate::style::KeyDef) -> i32 {
        let (comp, usecomp) = match (&keydef.comp, &keydef.usecomp) {
            (Some(c), Some(u)) => (c.clone(), u.clone()),
            _ => return -1,
        };
        if self.key_init_level > self.nb_keys {
            self.error(
                Some(keydef.inst),
                &format!("Key definition for {} is recursive\n", keydef.name),
            );
            self.state = State::Stopped;
            return -1;
        }
        self.key_init_level += 1;
        self.docs[di].nb_keys_computed += 1;
        let old_inst = self.inst;
        let old_doc_info = self.document;
        let old_node = self.node;
        let saved = self.xp.clone();
        let doc = self.docs[di].doc;
        self.document = Some(di);
        self.node = doc;
        self.inst = Some(keydef.inst);
        self.xp.doc = Some(doc);
        self.xp.node = Some(doc);
        self.xp.namespaces = keydef.ns_list.clone();
        'body: {
            let match_list = match xpath::eval(self, &comp) {
                None => {
                    self.error(
                        Some(keydef.inst),
                        "Failed to evaluate the 'match' expression.\n",
                    );
                    self.state = State::Stopped;
                    break 'body;
                }
                Some(XObj::NodeSet(v)) => v,
                Some(_) => {
                    self.error(
                        Some(keydef.inst),
                        "The 'match' expression did not evaluate to a node set.\n",
                    );
                    self.state = State::Stopped;
                    break 'body;
                }
            };
            if match_list.is_empty() {
                break 'body;
            }
            let key = (keydef.name.clone(), keydef.name_uri.clone());
            let ti = match self.docs[di].keys.iter().position(|(k, _)| *k == key) {
                Some(i) => i,
                None => {
                    self.docs[di].keys.insert(0, (key, HashMap::new()));
                    0
                }
            };
            self.xp.size = 1;
            self.xp.pos = 1;
            for cur in match_list {
                if !tw(|tr| is_real_node(tr, cur)) {
                    continue;
                }
                self.node = cur;
                self.xp.node = Some(cur);
                let use_res = match xpath::eval(self, &usecomp) {
                    Some(r) => r,
                    None => {
                        self.error(
                            Some(keydef.inst),
                            "Failed to evaluate the 'use' expression.\n",
                        );
                        self.state = State::Stopped;
                        break;
                    }
                };
                let strs: Vec<String> = match &use_res {
                    XObj::NodeSet(v) => {
                        tw(|tr| v.iter().map(|&n| xpath::node_string(tr, n)).collect())
                    }
                    XObj::Str(s) => vec![s.clone()],
                    o => vec![xpath::cast_to_string(o)],
                };
                for s in strs {
                    let table = &mut self.docs[di].keys[ti].1;
                    let list = table.entry(s).or_default();
                    if !list.contains(&cur) {
                        list.push(cur);
                    }
                    self.key_nodes.insert(cur);
                }
            }
        }
        self.key_init_level -= 1;
        let depth = self.xp.depth;
        self.xp = saved;
        self.xp.depth = depth;
        self.node = old_node;
        self.document = old_doc_info;
        self.inst = old_inst;
        0
    }

    /// xsltGetKey
    pub fn get_key(&mut self, name: &str, uri: Option<&str>, value: &str) -> Option<Vec<NodeId>> {
        let di = self.document?;
        if self.docs[di].nb_keys_computed < self.nb_keys && self.key_init_level == 0 {
            self.init_all_doc_keys();
            if self.stopped() && self.docs[di].nb_keys_computed < self.nb_keys {
                // xsltInitAllDocKeys failed
            }
        }
        let mut init_table = false;
        loop {
            for (k, table) in &self.docs[di].keys {
                if k.0 == name && k.1.as_deref() == uri {
                    return table.get(value).cloned();
                }
            }
            if self.key_init_level != 0 && !init_table {
                self.init_doc_key_table(name, uri);
                init_table = true;
                continue;
            }
            return None;
        }
    }

    /// xsltNeedElemSpaceHandling
    pub fn need_elem_space_handling(&self) -> bool {
        self.sheet
            .import_order()
            .iter()
            .any(|&s| self.sheet.styles[s].strip_spaces.is_some())
    }

    /// xsltFindElemSpaceHandling: should blank children of `node` be
    /// stripped?
    pub fn find_elem_space_handling(&self, node: NodeId) -> bool {
        let (name, href) = tw(|tr| {
            (
                tr.nodes[node].name.clone(),
                tr.ns_href(tr.nodes[node].ns).map(|s| s.to_string()),
            )
        });
        for s in self.sheet.import_order() {
            let st = &self.sheet.styles[s];
            let val = st.strip_spaces.as_ref().and_then(|h| {
                if href.is_some() {
                    h.get(&(name.clone(), href.clone()))
                        .or_else(|| h.get(&("*".to_string(), href.clone())))
                        .copied()
                } else {
                    h.get(&(name.clone(), None)).copied()
                }
            });
            if let Some(v) = val {
                return v;
            }
            if st.strip_all == 1 {
                return true;
            }
            if st.strip_all == -1 {
                return false;
            }
        }
        false
    }

    /// xsltApplyStripSpaces
    pub fn apply_strip_spaces(&mut self, node: NodeId) {
        let mut current = Some(node);
        while let Some(c) = current {
            let (real, has_children) =
                tw(|tr| (is_real_node(tr, c), tr.nodes[c].children.is_some()));
            if real && has_children && self.find_elem_space_handling(c) {
                tw(|tr| {
                    let kids = tr.children_of(c);
                    for k in kids {
                        if is_blank_node(tr, k) {
                            tr.unlink(k);
                        }
                    }
                });
            }
            current = tw(|tr| {
                let nd = &tr.nodes[c];
                if nd.children.is_some() && nd.t != NType::EntityRef {
                    return nd.children;
                }
                if let Some(n) = nd.next {
                    return Some(n);
                }
                let mut x = c;
                loop {
                    x = tr.nodes[x].parent?;
                    if x == node {
                        return None;
                    }
                    if let Some(n) = tr.nodes[x].next {
                        return Some(n);
                    }
                }
            });
        }
        xpath::invalidate_order();
    }
}

/// xsltKeyFunction
pub fn key_function(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    if nargs != 2 {
        t.error(None, "key() : expects two arguments\n");
        p.error = XPATH_INVALID_ARITY;
        return;
    }
    let obj2 = p.pop();
    xpath::cast_top_string(p);
    let obj2 = match (obj2, p.top()) {
        (Some(o), Some(XObj::Str(_))) => o,
        _ => {
            t.error(None, "key() : invalid arg expecting a string\n");
            p.error = XPATH_INVALID_TYPE;
            return;
        }
    };
    let obj1 = match p.pop() {
        Some(XObj::Str(s)) => s,
        _ => String::new(),
    };
    if let XObj::NodeSet(v) | XObj::Tree(v) = &obj2 {
        let mut ret: Vec<NodeId> = Vec::new();
        for &n in v {
            p.push(XObj::Str(obj1.clone()));
            p.push(XObj::NodeSet(vec![n]));
            xpath::cast_top_string(p);
            key_function(t, p, 2);
            if let Some(newobj) = p.pop() {
                if let Some(nv) = newobj.nodes() {
                    xpath::merge(&mut ret, nv);
                }
            }
        }
        p.push(XObj::NodeSet(ret));
        return;
    }
    let old_doc_info = t.document;
    let mut nodelist: Option<Vec<NodeId>> = None;
    'body: {
        let ctx_node = match t.xp.node {
            Some(n) => n,
            None => {
                let inst = t.inst;
                t.error(
                    inst,
                    "Internal error in xsltKeyFunction(): The context node is not set on the XPath context.\n",
                );
                t.state = State::Stopped;
                break 'body;
            }
        };
        let (key_uri, key) = match split_qname2(&obj1) {
            None => (None, obj1.clone()),
            Some((prefix, local)) => {
                let u = if prefix == "xml" {
                    Some(goro_ext_xml::tree::XML_XML_NAMESPACE.to_string())
                } else {
                    t.xp.namespaces
                        .iter()
                        .find(|(pp, _)| *pp == prefix)
                        .map(|(_, u)| u.clone())
                };
                if u.is_none() {
                    let inst = t.inst;
                    t.error(inst, &format!("key() : prefix {} is not bound\n", prefix));
                }
                (u, local)
            }
        };
        let value = xpath::cast_to_string(&obj2);
        let tmp_node = tw(|tr| {
            if tr.nodes[ctx_node].t == NType::NamespaceDecl {
                tr.nodes[ctx_node]
                    .parent
                    .filter(|&pp| tr.nodes[pp].t == NType::Element)
            } else {
                Some(ctx_node)
            }
        });
        let tmp_node = match tmp_node.filter(|&n| tw(|tr| tr.nodes[n].doc.is_some())) {
            Some(n) => n,
            None => {
                let inst = t.inst;
                t.error(
                    inst,
                    "Internal error in xsltKeyFunction(): Couldn't get the doc of the XPath context node.\n",
                );
                break 'body;
            }
        };
        let tdoc = tw(|tr| tr.nodes[tmp_node].doc.unwrap());
        let need = match t.document {
            None => true,
            Some(d) => t.docs[d].doc != tdoc,
        };
        if need {
            let d = t.document_for_node(tmp_node);
            match d {
                None => {
                    let inst = t.inst;
                    t.error(
                        inst,
                        "Internal error in xsltKeyFunction(): Could not get the document info of a context doc.\n",
                    );
                    t.state = State::Stopped;
                    break 'body;
                }
                Some(d) => t.document = Some(d),
            }
        }
        nodelist = t.get_key(&key, key_uri.as_deref(), &value);
    }
    t.document = old_doc_info;
    let mut v = Vec::new();
    if let Some(l) = nodelist {
        xpath::merge(&mut v, &l);
    }
    p.push(XObj::NodeSet(v));
}

/// xsltUnparsedEntityURIFunction
fn unparsed_entity_uri_function(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    if nargs != 1 || p.top().is_none() {
        t.generic_error("unparsed-entity-uri() : expects one string arg\n");
        p.error = XPATH_INVALID_ARITY;
        return;
    }
    let s = xpath::pop_string(p);
    let doc = t.xp.doc;
    let r = doc
        .and_then(|d| {
            tw(|tr| {
                let e = tr.get_doc_entity(d, &s)?;
                tr.entity_info(e).and_then(|i| {
                    i.system_id.clone().map(|sys| {
                        let base = tr
                            .int_subset(d)
                            .and_then(|ds| tr.nodes[ds].doc)
                            .and_then(|dd| doc_url(tr, dd));
                        build_uri(&sys, base.as_deref()).unwrap_or(sys)
                    })
                })
            })
        })
        .unwrap_or_default();
    p.push(XObj::Str(r));
}

/// xsltFormatNumberFunction
fn format_number_function(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    let inst = match t.inst {
        Some(i) => i,
        None => return,
    };
    let sheet = t.sheet.clone();
    let mut format_values = Some(sheet.styles[0].decimal_formats[0].clone());
    let format_obj: String;
    let number_obj: f64;
    match nargs {
        3 | 2 => {
            if nargs == 3 {
                xpath::cast_top_string(p);
                let dname = xpath::pop_string(p);
                let (prefix, ncname) = split_qname(&dname);
                let mut ns_uri = None;
                let mut ncname = Some(ncname);
                if let Some(pr) = prefix {
                    match tw(|tr| {
                        tr.search_ns(inst, Some(&pr))
                            .map(|n| tr.nss[n].href.clone())
                    }) {
                        None => {
                            t.error(
                                None,
                                &format!(
                                    "format-number : No namespace found for QName '{}:{}'\n",
                                    pr,
                                    ncname.clone().unwrap()
                                ),
                            );
                            ncname = None;
                        }
                        Some(h) => ns_uri = Some(h),
                    }
                }
                if let Some(nc) = ncname {
                    format_values = sheet
                        .decimal_format_by_qname(ns_uri.as_deref(), &nc)
                        .cloned();
                }
                if format_values.is_none() {
                    t.error(
                        None,
                        &format!("format-number() : undeclared decimal format '{}'\n", dname),
                    );
                }
            }
            if !matches!(p.top(), Some(XObj::Str(_))) && p.top().is_some() {
                xpath::cast_top_string(p);
            }
            format_obj = xpath::pop_string(p);
            if !matches!(p.top(), Some(XObj::Num(_))) && p.top().is_some() {
                xpath::cast_top_number(p);
            }
            number_obj = xpath::pop_number(p);
        }
        _ => {
            xpath::xp_err(t, p, XPATH_INVALID_ARITY);
            return;
        }
    }
    if p.error == 0 {
        if let Some(fv) = format_values {
            let r = crate::numbers::format_number_conversion(t, &fv, &format_obj, number_obj);
            p.push(XObj::Str(r));
        }
    }
}

/// xsltGenerateIdFunction
fn generate_id_function(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    let mut cur = if nargs == 0 {
        match t.xp.node {
            Some(n) => n,
            None => return,
        }
    } else if nargs == 1 {
        let v = match p.top() {
            Some(XObj::NodeSet(_)) => match p.pop() {
                Some(XObj::NodeSet(v)) => v,
                _ => Vec::new(),
            },
            _ => {
                p.error = XPATH_INVALID_TYPE;
                t.error(None, "generate-id() : invalid arg expecting a node-set\n");
                return;
            }
        };
        if v.is_empty() {
            p.push(XObj::Str(String::new()));
            return;
        }
        let mut c = v[0];
        for &n in &v[1..] {
            if xpath::cmp_nodes(c, n) == std::cmp::Ordering::Greater {
                c = n;
            }
        }
        c
    } else {
        t.error(
            None,
            &format!("generate-id() : invalid number of args {}\n", nargs),
        );
        p.error = XPATH_INVALID_ARITY;
        return;
    };
    let mut ns_prefix: Option<String> = None;
    let is_ns = tw(|tr| tr.nodes[cur].t == NType::NamespaceDecl);
    if is_ns {
        let (pref, parent) = tw(|tr| {
            let n = &tr.nodes[cur];
            let pref =
                n.ns.and_then(|ns| tr.nss[ns].prefix.clone())
                    .unwrap_or_default();
            (pref, n.parent)
        });
        ns_prefix = Some(pref);
        if let Some(pp) = parent {
            cur = pp;
        }
    }
    let valid = tw(|tr| {
        matches!(
            tr.nodes[cur].t,
            NType::Element
                | NType::Attribute
                | NType::Text
                | NType::CData
                | NType::Pi
                | NType::Comment
                | NType::Document
                | NType::HtmlDocument
        )
    });
    if !valid {
        let ty = tw(|tr| tr.nodes[cur].t.code());
        t.error(None, &format!("generate-id(): invalid node type {}\n", ty));
        p.error = XPATH_INVALID_TYPE;
        return;
    }
    let id = match t.ids.get(&cur) {
        Some(&i) => i,
        None => {
            t.current_id += 1;
            let i = t.current_id;
            t.ids.insert(cur, i);
            i
        }
    };
    let s = match ns_prefix {
        None => format!("id{}", id),
        Some(pref) => {
            let mut s = format!("id{}ns", id);
            for b in pref.bytes() {
                let hi = b >> 4;
                let lo = b & 15;
                s.push(if hi < 10 {
                    (b'0' + hi) as char
                } else {
                    (b'A' + hi - 10) as char
                });
                s.push(if lo < 10 {
                    (b'0' + lo) as char
                } else {
                    (b'A' + lo - 10) as char
                });
            }
            s
        }
    };
    p.push(XObj::Str(s));
}

/// xsltSystemPropertyFunction
fn system_property_function(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    if nargs != 1 {
        t.error(None, "system-property() : expects one string arg\n");
        p.error = XPATH_INVALID_ARITY;
        return;
    }
    let s = match p.top() {
        Some(XObj::Str(_)) => xpath::pop_string(p),
        _ => {
            t.error(None, "system-property() : invalid arg expecting a string\n");
            p.error = XPATH_INVALID_TYPE;
            return;
        }
    };
    let (name, ns_uri) = match split_qname2(&s) {
        None => (s.clone(), None),
        Some((prefix, local)) => {
            let u = lookup_prefix(t, &prefix);
            if u.is_none() {
                t.error(
                    None,
                    &format!("system-property() : prefix {} is not bound\n", prefix),
                );
            }
            (local, u)
        }
    };
    // DOCBOOK_XSL_HACK: a template's xsl:variable in a stylesheet whose
    // URL contains "chunk" sees a Saxon 6.2 compatible vendor
    let docbook_hack = name == "vendor" && {
        let principal = t.sheet.styles[0].doc;
        tw(|tr| {
            t.inst.is_some_and(|i| {
                tr.nodes[i].name == "variable"
                    && tr.nodes[i]
                        .parent
                        .is_some_and(|p| tr.nodes[p].name == "template")
            }) && tr.doc_url(principal).is_some_and(|u| u.contains("chunk"))
        })
    };
    let r = if ns_uri.as_deref() == Some(XSLT_NAMESPACE) {
        match name.as_str() {
            "vendor" if docbook_hack => "libxslt (SAXON 6.2 compatible)",
            "vendor" => XSLT_DEFAULT_VENDOR,
            "version" => XSLT_DEFAULT_VERSION,
            "vendor-url" => XSLT_DEFAULT_URL,
            _ => "",
        }
    } else {
        ""
    };
    p.push(XObj::Str(r.to_string()));
}

fn lookup_prefix(t: &Transform, prefix: &str) -> Option<String> {
    if prefix == "xml" {
        return Some(goro_ext_xml::tree::XML_XML_NAMESPACE.to_string());
    }
    t.xp.namespaces
        .iter()
        .find(|(pp, _)| pp == prefix)
        .map(|(_, u)| u.clone())
}

/// Is (name, uri) an available element (xsltExtElementLookup)?
fn element_lookup(name: &str, uri: Option<&str>) -> bool {
    let uri = match uri {
        Some(u) => u,
        None => return false,
    };
    if uri == XSLT_NAMESPACE {
        return matches!(
            name,
            "apply-templates"
                | "apply-imports"
                | "call-template"
                | "element"
                | "attribute"
                | "text"
                | "processing-instruction"
                | "comment"
                | "copy"
                | "value-of"
                | "number"
                | "for-each"
                | "if"
                | "choose"
                | "sort"
                | "copy-of"
                | "message"
                | "variable"
                | "param"
                | "with-param"
                | "decimal-format"
                | "when"
                | "otherwise"
                | "fallback"
                | "document"
        );
    }
    crate::style::ext_module_element(name, uri)
}

/// xsltElementAvailableFunction
fn element_available_function(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    if nargs != 1 {
        t.error(None, "element-available() : expects one string arg\n");
        p.error = XPATH_INVALID_ARITY;
        return;
    }
    xpath::cast_top_string(p);
    let s = match p.top() {
        Some(XObj::Str(_)) => xpath::pop_string(p),
        _ => {
            t.error(
                None,
                "element-available() : invalid arg expecting a string\n",
            );
            p.error = XPATH_INVALID_TYPE;
            return;
        }
    };
    let inst = match t.inst {
        Some(i) => i,
        None => {
            t.error(None, "element-available() : internal error tctxt == NULL\n");
            p.push(XObj::Bool(false));
            return;
        }
    };
    let (name, ns_uri) = match split_qname2(&s) {
        None => {
            let u = tw(|tr| tr.search_ns(inst, None).map(|n| tr.nss[n].href.clone()));
            (s.clone(), u)
        }
        Some((prefix, local)) => {
            let u = lookup_prefix(t, &prefix);
            if u.is_none() {
                t.error(
                    None,
                    &format!("element-available() : prefix {} is not bound\n", prefix),
                );
            }
            (local, u)
        }
    };
    p.push(XObj::Bool(element_lookup(&name, ns_uri.as_deref())));
}

/// xsltFunctionAvailableFunction
fn function_available_function(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    if nargs != 1 {
        t.error(None, "function-available() : expects one string arg\n");
        p.error = XPATH_INVALID_ARITY;
        return;
    }
    xpath::cast_top_string(p);
    let s = match p.top() {
        Some(XObj::Str(_)) => xpath::pop_string(p),
        _ => {
            t.error(
                None,
                "function-available() : invalid arg expecting a string\n",
            );
            p.error = XPATH_INVALID_TYPE;
            return;
        }
    };
    let (name, ns_uri) = match split_qname2(&s) {
        None => (s.clone(), None),
        Some((prefix, local)) => {
            let u = lookup_prefix(t, &prefix);
            if u.is_none() {
                t.error(
                    None,
                    &format!("function-available() : prefix {} is not bound\n", prefix),
                );
            }
            (local, u)
        }
    };
    let r = function_exists(t, &name, ns_uri.as_deref());
    p.push(XObj::Bool(r));
}
