//! The transformation engine: a port of libxslt's transform.c (sequence
//! constructors, templates, instructions, copying), variables.c,
//! templates.c (AVTs), the runtime half of pattern.c and namespaces.c.

use crate::pattern::{CompMatch, POp};
use crate::style::{Kind, PreComp, Sheet, XSLT_PAT_NO_PRIORITY};
use crate::util::*;
use crate::xpath::{self, CompExpr, PCtx, XObj, XpCtx};
use goro_core::vm::Vm;
use goro_ext_xml::tree::{NType, NodeId, NsId, Tree};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Ok,
    Error,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutType {
    Xml,
    Html,
    Text,
}

/// xsltStackElem: a variable / parameter value
#[derive(Debug)]
pub struct StackElem {
    pub name: String,
    pub name_uri: Option<String>,
    pub select: Option<String>,
    pub tree: Option<NodeId>,
    pub comp: Option<Rc<PreComp>>,
    pub computed: bool,
    pub value: Option<XObj>,
    pub level: i32,
    /// a global being computed (xsltComputingGlobalVarMarker)
    pub computing: bool,
}

pub type Elem = Rc<RefCell<StackElem>>;

/// Per-document information (xsltDocument): keys.
#[derive(Debug, Default)]
pub struct DocInfo {
    pub doc: NodeId,
    pub nb_keys_computed: usize,
    /// key tables: (name, uri) -> value -> nodes
    pub keys: Vec<((String, Option<String>), HashMap<String, Vec<NodeId>>)>,
    pub main: bool,
}

/// Security preferences (XSL_SECPREF_*)
pub const SEC_READ_FILE: i64 = 2;
pub const SEC_WRITE_FILE: i64 = 4;
pub const SEC_CREATE_DIRECTORY: i64 = 8;
pub const SEC_READ_NETWORK: i64 = 16;
pub const SEC_WRITE_NETWORK: i64 = 32;

/// PHP-side hooks of the transformation (functions registered through
/// XSLTProcessor).
pub trait PhpHost {
    /// php:function / php:functionString, registered namespaced functions.
    /// Returns None when no such function exists (lookup only).
    fn has_function(&self, name: &str, uri: &str) -> bool;
    fn call(&mut self, t: &mut Transform, p: &mut PCtx, name: &str, uri: &str, nargs: usize);
}

pub struct Transform<'v> {
    pub vm: &'v mut Vm,
    pub sheet: Rc<Sheet>,
    pub xp: XpCtx,
    /// the current node
    pub node: NodeId,
    pub insert: Option<NodeId>,
    pub output: NodeId,
    pub inst: Option<NodeId>,
    pub mode: Option<String>,
    pub mode_uri: Option<String>,
    /// the template stack (indices into sheet.templates)
    pub templ_tab: Vec<usize>,
    pub current_template_rule: Option<usize>,
    pub vars: Vec<Elem>,
    pub vars_base: usize,
    pub depth: i32,
    pub max_template_depth: i32,
    pub max_template_vars: i32,
    pub state: State,
    pub global_vars: HashMap<(String, Option<String>), Elem>,
    pub docs: Vec<DocInfo>,
    /// index into docs of the current document (ctxt->document)
    pub document: Option<usize>,
    pub key_init_level: usize,
    pub nb_keys: usize,
    pub has_templ_key_patterns: bool,
    pub key_nodes: HashSet<NodeId>,
    pub ids: HashMap<NodeId, u64>,
    pub current_id: u64,
    pub out_type: OutType,
    pub initial_context_doc: NodeId,
    pub initial_context_node: NodeId,
    pub sec_prefs: i64,
    pub xinclude: bool,
    pub parser_options: i64,
    pub node_list: Vec<NodeId>,
    pub php: Option<*mut dyn PhpHost>,
    /// the source document of the transformation (ctxt->document->doc)
    pub main_doc: NodeId,
    /// func:function call stack (results)
    pub func_results: Vec<Option<XObj>>,
    /// exsltFuncData: result, error and saved context variable
    pub func_result: Option<XObj>,
    pub func_error: bool,
    pub func_ctxt_var: Option<Elem>,
    /// nodes created in RTFs (result value trees), for cleanup bookkeeping
    pub rtfs: Vec<NodeId>,
    /// the user parameters (name, value) set from PHP
    pub contextual_var: Option<Elem>,
}

pub const XSLT_MAX_SORT: usize = 15;

impl<'v> Transform<'v> {
    pub fn new(vm: &'v mut Vm, sheet: Rc<Sheet>, doc: NodeId) -> Self {
        let nb_keys = 0;
        Transform {
            vm,
            sheet,
            xp: XpCtx::default(),
            node: doc,
            insert: None,
            output: doc,
            inst: None,
            mode: None,
            mode_uri: None,
            templ_tab: Vec::new(),
            current_template_rule: None,
            vars: Vec::new(),
            vars_base: 0,
            depth: 0,
            max_template_depth: 3000,
            max_template_vars: 15000,
            state: State::Ok,
            global_vars: HashMap::new(),
            docs: vec![DocInfo {
                doc,
                main: true,
                ..Default::default()
            }],
            document: Some(0),
            key_init_level: 0,
            nb_keys,
            has_templ_key_patterns: false,
            key_nodes: HashSet::new(),
            ids: HashMap::new(),
            current_id: 0,
            out_type: OutType::Xml,
            initial_context_doc: doc,
            initial_context_node: doc,
            sec_prefs: 0,
            xinclude: false,
            parser_options: crate::style::XSLT_PARSE_OPTIONS,
            node_list: Vec::new(),
            php: None,
            main_doc: doc,
            func_results: Vec::new(),
            func_result: None,
            func_error: false,
            func_ctxt_var: None,
            rtfs: Vec::new(),
            contextual_var: None,
        }
    }

    // ------------------------------------------------------------------
    // Errors

    /// xsltTransformError(ctxt, NULL, node, msg)
    pub fn error(&mut self, node: Option<NodeId>, msg: &str) {
        if self.state == State::Ok {
            self.state = State::Error;
        }
        let node = node.or(self.inst);
        transform_error(self.vm, true, false, node, msg);
    }

    /// xsltTransformError(ctxt, style, node, msg) during transformation
    pub fn error_style(&mut self, node: Option<NodeId>, msg: &str) {
        self.error(node, msg);
    }

    /// xsltGenericError
    pub fn generic_error(&mut self, msg: &str) {
        php_error(self.vm, msg);
    }

    pub fn stopped(&self) -> bool {
        self.state == State::Stopped
    }

    // ------------------------------------------------------------------
    // XPath evaluation helpers

    /// Evaluate a compiled expression with the given namespaces, the
    /// context node `node` (xsltPreCompEval).
    pub fn eval_comp(
        &mut self,
        node: NodeId,
        comp: &CompExpr,
        ns: &Rc<Vec<(String, String)>>,
    ) -> Option<XObj> {
        let (old_node, old_ns) = (self.xp.node, self.xp.namespaces.clone());
        self.xp.node = Some(node);
        self.xp.namespaces = ns.clone();
        let r = xpath::eval(self, comp);
        self.xp.node = old_node;
        self.xp.namespaces = old_ns;
        r
    }

    /// xsltPreCompEvalToBoolean: 1, 0, or -1 on error
    pub fn eval_comp_bool(
        &mut self,
        node: NodeId,
        comp: &CompExpr,
        ns: &Rc<Vec<(String, String)>>,
    ) -> i32 {
        let (old_node, old_ns) = (self.xp.node, self.xp.namespaces.clone());
        self.xp.node = Some(node);
        self.xp.namespaces = ns.clone();
        let r = xpath::eval_to_boolean(self, comp);
        self.xp.node = old_node;
        self.xp.namespaces = old_ns;
        match r {
            Some(true) => 1,
            Some(false) => 0,
            None => -1,
        }
    }

    /// xsltEvalXPathStringNs
    pub fn eval_xpath_string_ns(
        &mut self,
        comp: &CompExpr,
        ns: &Rc<Vec<(String, String)>>,
    ) -> Option<String> {
        let old_inst = self.inst;
        let (old_node, old_pos, old_size, old_ns) = (
            self.xp.node,
            self.xp.pos,
            self.xp.size,
            self.xp.namespaces.clone(),
        );
        self.xp.node = Some(self.node);
        self.xp.namespaces = ns.clone();
        let res = xpath::eval(self, comp);
        let ret = match res {
            Some(XObj::Str(s)) => Some(s),
            Some(o) => Some(xpath::cast_to_string(&o)),
            None => {
                self.state = State::Stopped;
                None
            }
        };
        self.inst = old_inst;
        self.xp.node = old_node;
        self.xp.size = old_size;
        self.xp.pos = old_pos;
        self.xp.namespaces = old_ns;
        ret
    }

    /// xsltAttrTemplateValueProcessNode
    pub fn attr_template_value_process(&mut self, s: &str, inst: Option<NodeId>) -> String {
        let b = s.as_bytes();
        let mut ret = String::new();
        let mut start = 0usize;
        let mut cur = 0usize;
        let mut ns_list: Option<Rc<Vec<(String, String)>>> = None;
        while cur < b.len() {
            if b[cur] == b'{' {
                if cur + 1 < b.len() && b[cur + 1] == b'{' {
                    cur += 1;
                    ret.push_str(&s[start..cur]);
                    cur += 1;
                    start = cur;
                    continue;
                }
                ret.push_str(&s[start..cur]);
                start = cur;
                cur += 1;
                while cur < b.len() && b[cur] != b'}' {
                    if b[cur] == b'\'' || b[cur] == b'"' {
                        let d = b[cur];
                        cur += 1;
                        while cur < b.len() && b[cur] != d {
                            cur += 1;
                        }
                        if cur < b.len() {
                            cur += 1;
                        }
                    } else {
                        cur += 1;
                    }
                }
                if cur >= b.len() {
                    self.error(inst, "xsltAttrTemplateValueProcessNode: unmatched '{'\n");
                    ret.push_str(&s[start..]);
                    return ret;
                }
                start += 1;
                let expr = &s[start..cur];
                if expr.starts_with('{') {
                    ret.push_str(expr);
                } else {
                    if ns_list.is_none() {
                        ns_list = Some(Rc::new(match inst {
                            Some(i) => tw(|t| xpath_ns_list(t, i)),
                            None => Vec::new(),
                        }));
                    }
                    match xpath::compile(expr) {
                        Ok(comp) => {
                            let nsl = ns_list.clone().unwrap();
                            if let Some(v) = self.eval_xpath_string_ns(&comp, &nsl) {
                                ret.push_str(&v);
                            }
                        }
                        Err(msgs) => {
                            for m in msgs {
                                php_error(self.vm, &format!("{}\n", m));
                            }
                            // xsltEvalXPathStringNs(NULL) does nothing
                        }
                    }
                }
                cur += 1;
                start = cur;
            } else if b[cur] == b'}' {
                cur += 1;
                if cur < b.len() && b[cur] == b'}' {
                    ret.push_str(&s[start..cur]);
                    cur += 1;
                    start = cur;
                    continue;
                } else {
                    self.error(inst, "xsltAttrTemplateValueProcessNode: unmatched '}'\n");
                }
            } else {
                cur += 1;
            }
        }
        if cur != start {
            ret.push_str(&s[start..cur.min(b.len())]);
        }
        ret
    }

    /// xsltEvalAttrValueTemplate
    pub fn eval_attr_value_template(
        &mut self,
        inst: NodeId,
        name: &str,
        ns: Option<&str>,
    ) -> Option<String> {
        let expr = tw(|t| xslt_ns_prop(t, inst, name, ns))?;
        Some(self.attr_template_value_process(&expr, Some(inst)))
    }

    /// xsltEvalAVT
    pub fn eval_avt(&mut self, avt: &crate::style::Avt) -> Option<String> {
        let mut ret: Option<String> = None;
        let mut is_str = avt.strstart;
        for seg in &avt.segments {
            match seg {
                crate::style::AvtSeg::Str(s) => {
                    ret.get_or_insert_with(String::new).push_str(s);
                }
                crate::style::AvtSeg::Expr(c) => {
                    if let Some(c) = c {
                        let c = c.clone();
                        if let Some(v) = self.eval_xpath_string_ns(&c, &avt.ns_list) {
                            ret.get_or_insert_with(String::new).push_str(&v);
                        }
                    }
                }
            }
            is_str = !is_str;
        }
        let _ = is_str;
        ret
    }

    // ------------------------------------------------------------------
    // Tree output helpers

    fn is_cdata_section_target(&self, target: NodeId) -> bool {
        if self.out_type != OutType::Xml {
            return false;
        }
        let cds = match &self.sheet.styles[0].cdata_section {
            Some(c) => c,
            None => return false,
        };
        tw(|t| {
            let nd = &t.nodes[target];
            if nd.t != NType::Element {
                return false;
            }
            let href = t.ns_href(nd.ns).map(|s| s.to_string());
            cds.contains(&(nd.name.clone(), href))
        })
    }

    /// xsltAddChild (xmlAddChild semantics; text merging)
    pub fn add_child(&mut self, parent: Option<NodeId>, cur: NodeId) -> Option<NodeId> {
        let parent = parent?;
        Some(tw(|t| t.add_child(parent, cur)))
    }

    /// xsltCopyTextString
    pub fn copy_text_string(
        &mut self,
        target: Option<NodeId>,
        s: &str,
        noescape: bool,
    ) -> Option<NodeId> {
        let target = target?;
        let cdata = self.is_cdata_section_target(target);
        let out = self.output;
        let r = tw(|t| {
            let last = t.nodes[target].last;
            if cdata {
                if let Some(l) = last {
                    if t.nodes[l].t == NType::CData {
                        t.nodes[l].content.push_str(s);
                        return l;
                    }
                }
                let c = t.new_cdata(Some(out), s);
                t.link_child_raw(target, c);
                return c;
            }
            if noescape {
                if let Some(l) = last {
                    if t.nodes[l].t == NType::Text && t.nodes[l].name == "textnoenc" {
                        t.nodes[l].content.push_str(s);
                        return l;
                    }
                }
                let c = t.new_text(Some(out), s);
                t.nodes[c].name = "textnoenc".into();
                t.nodes[c].flags |= 1;
                t.link_child_raw(target, c);
                return c;
            }
            if let Some(l) = last {
                if t.nodes[l].t == NType::Text && t.nodes[l].name == "text" {
                    t.nodes[l].content.push_str(s);
                    return l;
                }
            }
            let c = t.new_text(Some(out), s);
            t.link_child_raw(target, c);
            c
        });
        Some(r)
    }

    /// xsltCopyText: copy a text / CDATA node
    pub fn copy_text(&mut self, target: Option<NodeId>, cur: NodeId) -> Option<NodeId> {
        let target = target?;
        let (ctype, content, cname) = tw(|t| {
            let n = &t.nodes[cur];
            (n.t, n.content.clone(), n.name.clone())
        });
        if ctype != NType::Text && ctype != NType::CData {
            return None;
        }
        let noenc = cname == "textnoenc";
        let cdata = self.is_cdata_section_target(target);
        let out = self.output;
        Some(tw(|t| {
            let last = t.nodes[target].last;
            if cdata {
                if let Some(l) = last {
                    if t.nodes[l].t == NType::CData {
                        t.nodes[l].content.push_str(&content);
                        return l;
                    }
                }
                let c = t.new_cdata(Some(out), &content);
                t.link_child_raw(target, c);
                return c;
            }
            if let Some(l) = last {
                let ln = &t.nodes[l];
                if (ln.t == NType::Text && ln.name == cname)
                    || (ln.t == NType::CData && noenc)
                    || (ln.t == NType::Text && ctype == NType::CData && ln.name == "text")
                {
                    if ln.t == NType::Text && ctype == NType::CData && ln.name == "text" {
                        // a CDATA node is copied as normal text
                    }
                    t.nodes[l].content.push_str(&content);
                    return l;
                }
            }
            let c = t.new_text(Some(out), &content);
            if noenc {
                t.nodes[c].name = "textnoenc".into();
                t.nodes[c].flags |= 1;
            }
            t.link_child_raw(target, c);
            c
        }))
    }

    // ------------------------------------------------------------------
    // Namespaces (namespaces.c)

    /// xsltGetSpecialNamespace
    pub fn get_special_namespace(
        &mut self,
        invoc: Option<NodeId>,
        ns_name: Option<&str>,
        ns_prefix: Option<&str>,
        target: NodeId,
    ) -> Option<NsId> {
        enum R {
            Ret(Option<NsId>),
            Err(String),
            DeclareNewPrefix,
        }
        let r = tw(|t| {
            if t.nodes[target].t != NType::Element {
                return R::Ret(None);
            }
            if ns_prefix.is_none() && ns_name.map(|n| n.is_empty()).unwrap_or(true) {
                for &ns in &t.nodes[target].ns_def {
                    if t.nss[ns].prefix.is_none() {
                        if !t.nss[ns].href.is_empty() {
                            return R::Err(format!(
                                "Namespace normalization error: Cannot undeclare the default namespace, since the default namespace '{}' is already declared on the result element '{}'.\n",
                                t.nss[ns].href, t.nodes[target].name
                            ));
                        }
                        return R::Ret(None);
                    }
                }
                if let Some(p) = t.nodes[target].parent {
                    if t.nodes[p].t == NType::Element {
                        if t.nodes[p].ns.is_none() {
                            return R::Ret(None);
                        }
                        let ns = t.search_ns(p, None);
                        match ns {
                            None => return R::Ret(None),
                            Some(n) if t.nss[n].href.is_empty() => return R::Ret(None),
                            _ => {}
                        }
                        t.new_ns(Some(target), "", None);
                        return R::Ret(None);
                    }
                }
                return R::Ret(None);
            }
            let ns_name = ns_name.unwrap_or("");
            if ns_prefix == Some("xml") {
                return R::Ret(t.search_ns(target, Some("xml")));
            }
            let mut prefix_occupied = false;
            for &ns in &t.nodes[target].ns_def {
                if t.nss[ns].prefix.as_deref() == ns_prefix {
                    if t.nss[ns].href == ns_name {
                        return R::Ret(Some(ns));
                    }
                    prefix_occupied = true;
                    break;
                }
            }
            if prefix_occupied {
                if let Some(ns) = t.search_ns_by_href(target, ns_name) {
                    return R::Ret(Some(ns));
                }
                return R::DeclareNewPrefix;
            }
            let parent = t.nodes[target].parent;
            if let Some(p) = parent.filter(|&p| t.nodes[p].t == NType::Element) {
                if let Some(pns) = t.nodes[p].ns {
                    let pp = t.nss[pns].prefix.clone();
                    if pp.is_some() == ns_prefix.is_some() {
                        if ns_prefix.is_none() {
                            if t.nss[pns].href == ns_name {
                                return R::Ret(Some(pns));
                            }
                        } else if pp.as_deref() == ns_prefix && t.nss[pns].href == ns_name {
                            return R::Ret(Some(pns));
                        }
                    }
                }
                if let Some(ns) = t.search_ns(p, ns_prefix) {
                    if t.nss[ns].href == ns_name {
                        return R::Ret(Some(ns));
                    }
                    let mut a = t.nodes[target].properties;
                    while let Some(x) = a {
                        if let Some(ans) = t.nodes[x].ns {
                            if t.nss[ans].prefix.as_deref() == ns_prefix {
                                if let Some(ns) = t.search_ns_by_href(target, ns_name) {
                                    return R::Ret(Some(ns));
                                }
                                return R::DeclareNewPrefix;
                            }
                        }
                        a = t.nodes[x].next;
                    }
                }
                return R::Ret(t.new_ns(Some(target), ns_name, ns_prefix));
            }
            R::Ret(t.new_ns(Some(target), ns_name, ns_prefix))
        });
        match r {
            R::Ret(x) => x,
            R::Err(m) => {
                self.error(invoc, &m);
                None
            }
            R::DeclareNewPrefix => {
                let base = ns_prefix.unwrap_or("ns").to_string();
                let ns_name = ns_name.unwrap_or("").to_string();
                let mut counter = 1;
                loop {
                    let pref = format!("{}_{}", base, counter);
                    counter += 1;
                    let exists = tw(|t| t.search_ns(target, Some(&pref)).is_some());
                    if counter > 1000 {
                        self.error(
                            invoc,
                            "Internal error in xsltAcquireResultInScopeNs(): Failed to compute a unique ns-prefix for the generated element",
                        );
                        return None;
                    }
                    if !exists {
                        return tw(|t| t.new_ns(Some(target), &ns_name, Some(&pref)));
                    }
                }
            }
        }
    }

    /// The namespace alias of `href` (cascading over imports): Some(None)
    /// for UNDEFINED_DEFAULT_NS.
    fn ns_alias(&self, href: &str) -> Option<Option<String>> {
        for s in self.sheet.import_order() {
            for (k, v) in &self.sheet.styles[s].ns_aliases {
                if k == href {
                    return Some(v.clone());
                }
            }
        }
        None
    }

    /// xsltGetNamespace
    pub fn get_namespace(&mut self, cur: NodeId, ns: Option<NsId>, out: NodeId) -> Option<NsId> {
        let ns = ns?;
        let (href, prefix) = tw(|t| (t.nss[ns].href.clone(), t.nss[ns].prefix.clone()));
        match self.ns_alias(&href) {
            Some(None) => self.get_special_namespace(Some(cur), None, None, out),
            Some(Some(u)) => {
                self.get_special_namespace(Some(cur), Some(&u), prefix.as_deref(), out)
            }
            None => self.get_special_namespace(Some(cur), Some(&href), prefix.as_deref(), out),
        }
    }

    /// xsltCopyNamespaceList (with aliasing)
    pub fn copy_namespace_list(&mut self, node: Option<NodeId>, list: &[NsId]) {
        let node = node.filter(|&n| tw(|t| t.nodes[n].t == NType::Element));
        for &cur in list {
            let (href, prefix) = tw(|t| (t.nss[cur].href.clone(), t.nss[cur].prefix.clone()));
            if let Some(n) = node {
                let skip = tw(|t| {
                    if let Some(nns) = t.nodes[n].ns {
                        if t.nss[nns].prefix == prefix && t.nss[nns].href == href {
                            return true;
                        }
                    }
                    if let Some(tmp) = t.search_ns(n, prefix.as_deref()) {
                        if t.nss[tmp].href == href {
                            return true;
                        }
                    }
                    false
                });
                if skip {
                    continue;
                }
            }
            if href != XSLT_NAMESPACE {
                // TODO apply cascading: only the principal stylesheet
                let alias = self.sheet.styles[0]
                    .ns_aliases
                    .iter()
                    .find(|(k, _)| *k == href)
                    .map(|(_, v)| v.clone());
                match alias {
                    Some(None) => continue,
                    Some(Some(u)) => {
                        tw(|t| t.new_ns(node, &u, prefix.as_deref()));
                    }
                    None => {
                        tw(|t| t.new_ns(node, &href, prefix.as_deref()));
                    }
                }
            }
        }
    }

    /// xsltCopyNamespaceListInternal
    fn copy_namespace_list_internal(&mut self, elem: NodeId, list: &[NsId]) {
        tw(|t| {
            if t.nodes[elem].t != NType::Element {
                return;
            }
            for &ns in list {
                let (href, prefix) = (t.nss[ns].href.clone(), t.nss[ns].prefix.clone());
                if let Some(ens) = t.nodes[elem].ns {
                    if t.nss[ens].prefix == prefix && t.nss[ens].href == href {
                        continue;
                    }
                }
                if let Some(lu) = t.search_ns(elem, prefix.as_deref()) {
                    if t.nss[lu].href == href {
                        continue;
                    }
                }
                t.new_ns(Some(elem), &href, prefix.as_deref());
            }
        });
    }

    // ------------------------------------------------------------------
    // Copying (transform.c)

    /// xmlDocCopyNode(node, doc, 0) for elements / PIs / comments
    fn shallow_node_copy(&mut self, node: NodeId) -> NodeId {
        let out = self.output;
        tw(|t| {
            let src = t.nodes[node].clone();
            let n = t.new_node(Some(out), src.t, &src.name);
            t.nodes[n].content = src.content.clone();
            t.nodes[n].flags = src.flags & 1;
            if src.t == NType::Element {
                t.nodes[n].line = src.line;
            }
            n
        })
    }

    /// xsltShallowCopyElem
    pub fn shallow_copy_elem(
        &mut self,
        node: NodeId,
        insert: Option<NodeId>,
        is_lre: bool,
    ) -> Option<NodeId> {
        let insert = insert?;
        let ntype = tw(|t| t.nodes[node].t);
        if ntype == NType::Dtd {
            return None;
        }
        if ntype == NType::Text || ntype == NType::CData {
            return self.copy_text(Some(insert), node);
        }
        let copy = self.shallow_node_copy(node);
        let copy = self.add_child(Some(insert), copy)?;
        if ntype == NType::Element {
            let (ns_def, nns) = tw(|t| (t.nodes[node].ns_def.clone(), t.nodes[node].ns));
            if !ns_def.is_empty() {
                if is_lre {
                    self.copy_namespace_list(Some(copy), &ns_def);
                } else {
                    self.copy_namespace_list_internal(copy, &ns_def);
                }
            }
            if let Some(ns) = nns {
                let r = if is_lre {
                    self.get_namespace(node, Some(ns), copy)
                } else {
                    let (h, p) = tw(|t| (t.nss[ns].href.clone(), t.nss[ns].prefix.clone()));
                    self.get_special_namespace(Some(node), Some(&h), p.as_deref(), copy)
                };
                tw(|t| t.nodes[copy].ns = r);
            } else {
                let undeclare =
                    tw(|t| t.nodes[insert].t == NType::Element && t.nodes[insert].ns.is_some());
                if undeclare {
                    self.get_special_namespace(Some(node), None, None, copy);
                }
            }
        }
        Some(copy)
    }

    /// xsltShallowCopyAttr
    pub fn shallow_copy_attr(
        &mut self,
        invoc: Option<NodeId>,
        target: Option<NodeId>,
        attr: NodeId,
    ) -> Option<NodeId> {
        let target = target?;
        let (ttype, has_children) = tw(|t| (t.nodes[target].t, t.nodes[target].children.is_some()));
        if ttype != NType::Element {
            self.error(
                invoc,
                "Cannot add an attribute node to a non-element node.\n",
            );
            return None;
        }
        if has_children {
            self.error(
                invoc,
                "Attribute nodes must be added before any child nodes to an element.\n",
            );
            return None;
        }
        let (value, ans, name) = tw(|t| {
            let a = &t.nodes[attr];
            (t.node_get_content(attr), a.ns, a.name.clone())
        });
        let ns = match ans {
            Some(ans) => {
                let (h, p) = tw(|t| (t.nss[ans].href.clone(), t.nss[ans].prefix.clone()));
                let ns = self.get_special_namespace(invoc, Some(&h), p.as_deref(), target);
                if ns.is_none() {
                    self.error(
                        invoc,
                        &format!(
                            "Namespace fixup error: Failed to acquire an in-scope namespace binding of the copied attribute '{{{}}}{}'.\n",
                            h, name
                        ),
                    );
                }
                ns
            }
            None => None,
        };
        Some(tw(|t| t.set_ns_prop(target, ns, &name, &value)))
    }

    /// xsltCopyAttrListNoOverwrite
    fn copy_attr_list_no_overwrite(
        &mut self,
        invoc: Option<NodeId>,
        target: NodeId,
        attrs: &[NodeId],
    ) -> Result<(), ()> {
        let mut orig_ns: Option<Option<NsId>> = None;
        let mut copy_ns: Option<NsId> = None;
        for &attr in attrs {
            let (ans, name, value) = tw(|t| {
                (
                    t.nodes[attr].ns,
                    t.nodes[attr].name.clone(),
                    t.node_get_content(attr),
                )
            });
            if orig_ns != Some(ans) {
                orig_ns = Some(ans);
                match ans {
                    Some(a) => {
                        let (h, p) = tw(|t| (t.nss[a].href.clone(), t.nss[a].prefix.clone()));
                        copy_ns = self.get_special_namespace(invoc, Some(&h), p.as_deref(), target);
                        if copy_ns.is_none() {
                            return Err(());
                        }
                    }
                    None => copy_ns = None,
                }
            }
            let out = self.output;
            tw(|t| {
                let a = t.new_attr_node(Some(out), copy_ns, &name, Some(&value));
                t.link_child_raw(target, a);
            });
        }
        Ok(())
    }

    /// xsltShallowCopyNsNode
    fn shallow_copy_ns_node(
        &mut self,
        invoc: Option<NodeId>,
        insert: Option<NodeId>,
        nsnode: NodeId,
    ) -> Option<NsId> {
        let insert = insert?;
        let (itype, ichildren) = tw(|t| (t.nodes[insert].t, t.nodes[insert].children.is_some()));
        if itype != NType::Element {
            return None;
        }
        if ichildren {
            self.error(
                invoc,
                "Namespace nodes must be added before any child nodes are added to an element.\n",
            );
            return None;
        }
        tw(|t| {
            let nsid = t.nodes[nsnode].ns?;
            let prefix = t.nss[nsid].prefix.clone();
            let href = t.nss[nsid].href.clone();
            match &prefix {
                None => {
                    if t.nodes[insert].ns.is_none() {
                        return None;
                    }
                }
                Some(p) if p == "xml" => return None,
                _ => {}
            }
            for &d in &t.nodes[insert].ns_def {
                if t.nss[d].prefix == prefix {
                    if t.nss[d].href == href {
                        return None;
                    }
                    return None;
                }
            }
            if let Some(tmp) = t.search_ns(insert, prefix.as_deref()) {
                if t.nss[tmp].href == href {
                    return None;
                }
            }
            t.new_ns(Some(insert), &href, prefix.as_deref())
        })
    }

    /// xsltCopyTreeList
    fn copy_tree_list(
        &mut self,
        invoc: Option<NodeId>,
        list: Option<NodeId>,
        insert: Option<NodeId>,
        is_lre: bool,
        top_elem_visited: bool,
    ) {
        let mut cur = list;
        while let Some(c) = cur {
            let next = tw(|t| t.nodes[c].next);
            self.copy_tree(invoc, c, insert, is_lre, top_elem_visited);
            cur = next;
        }
    }

    /// xsltCopyTree
    pub fn copy_tree(
        &mut self,
        invoc: Option<NodeId>,
        node: NodeId,
        insert: Option<NodeId>,
        is_lre: bool,
        mut top_elem_visited: bool,
    ) -> Option<NodeId> {
        let insert = insert?;
        let (ntype, content, name, is_rtf) = tw(|t| {
            let n = &t.nodes[node];
            (
                n.t,
                n.content.clone(),
                n.name.clone(),
                xpath::is_rtf(t, node),
            )
        });
        match ntype {
            NType::Element
            | NType::EntityRef
            | NType::Entity
            | NType::Pi
            | NType::Comment
            | NType::Document
            | NType::HtmlDocument => {}
            NType::Text => {
                let noenc = name == "textnoenc";
                return self.copy_text_string(Some(insert), &content, noenc);
            }
            NType::CData => return self.copy_text_string(Some(insert), &content, false),
            NType::Attribute => return self.shallow_copy_attr(invoc, Some(insert), node),
            NType::NamespaceDecl => {
                self.shallow_copy_ns_node(invoc, Some(insert), node);
                return None;
            }
            _ => return None,
        }
        if is_rtf || matches!(ntype, NType::Document | NType::HtmlDocument) {
            if is_rtf {
                let ch = tw(|t| t.nodes[node].children);
                if ch.is_some() {
                    self.copy_tree_list(invoc, ch, Some(insert), false, false);
                }
                return None;
            }
        }
        let copy = match ntype {
            NType::Document | NType::HtmlDocument => {
                // xmlDocCopyNode on a document: a document copy (not useful
                // in a result tree); libxslt only gets here for documents
                // selected through namespace-less paths
                return None;
            }
            NType::EntityRef => {
                let out = self.output;
                tw(|t| t.new_entity_ref(Some(out), &name))
            }
            _ => self.shallow_node_copy(node),
        };
        let copy = match self.add_child(Some(insert), copy) {
            Some(c) => c,
            None => {
                self.error(
                    invoc,
                    &format!("xsltCopyTree: Copying of '{}' failed.\n", name),
                );
                return None;
            }
        };
        let last = tw(|t| t.nodes[insert].last);
        if last != Some(copy) {
            return last;
        }
        if ntype == NType::Element {
            let (parent_is_elem, node_ns, ns_def) = tw(|t| {
                let p = t.nodes[node].parent;
                (
                    p.map(|p| !matches!(t.nodes[p].t, NType::Document | NType::HtmlDocument))
                        .unwrap_or(false),
                    t.nodes[node].ns,
                    t.nodes[node].ns_def.clone(),
                )
            });
            if !top_elem_visited && parent_is_elem {
                // copy all in-scope namespaces
                let ns_list = tw(|t| ns_list(t, node));
                for cns in ns_list {
                    let (cp, ch) = tw(|t| (t.nss[cns].prefix.clone(), t.nss[cns].href.clone()));
                    let ns = tw(|t| {
                        let found = t.search_ns(insert, cp.as_deref());
                        match found {
                            Some(f) if t.nss[f].href == ch => Some(f),
                            _ => None,
                        }
                    });
                    let ns = match ns {
                        Some(n) => Some(n),
                        None => tw(|t| t.new_ns(Some(copy), &ch, cp.as_deref())),
                    };
                    if node_ns == Some(cns) {
                        tw(|t| t.nodes[copy].ns = ns);
                    }
                }
            } else if !ns_def.is_empty() {
                if is_lre {
                    self.copy_namespace_list(Some(copy), &ns_def);
                } else {
                    self.copy_namespace_list_internal(copy, &ns_def);
                }
            }
            if let Some(nns) = node_ns {
                if tw(|t| t.nodes[copy].ns.is_none()) {
                    let (h, p) = tw(|t| (t.nss[nns].href.clone(), t.nss[nns].prefix.clone()));
                    let r = self.get_special_namespace(invoc, Some(&h), p.as_deref(), copy);
                    tw(|t| t.nodes[copy].ns = r);
                }
            } else if tw(|t| t.nodes[insert].t == NType::Element && t.nodes[insert].ns.is_some()) {
                self.get_special_namespace(invoc, None, None, copy);
            }
            let attrs = tw(|t| t.attrs_of(node));
            if !attrs.is_empty() {
                let _ = self.copy_attr_list_no_overwrite(invoc, copy, &attrs);
            }
            if !top_elem_visited {
                top_elem_visited = true;
            }
        }
        let ch = tw(|t| {
            if t.nodes[node].t == NType::EntityRef {
                None
            } else {
                t.nodes[node].children
            }
        });
        if ch.is_some() {
            self.copy_tree_list(invoc, ch, Some(copy), is_lre, top_elem_visited);
        }
        Some(copy)
    }

    // ------------------------------------------------------------------
    // Templates

    /// Template stack push (templPush)
    fn templ_push(&mut self, t: usize) {
        self.templ_tab.push(t);
    }

    fn templ_pop(&mut self) {
        self.templ_tab.pop();
    }

    pub fn templ(&self) -> Option<usize> {
        self.templ_tab.last().copied()
    }

    /// xsltProcessOneNode
    pub fn process_one_node(&mut self, context_node: NodeId, with_params: &[Elem]) {
        let templ = self.get_template(context_node, None);
        match templ {
            None => {
                let old = self.node;
                self.node = context_node;
                self.default_process_one_node(context_node, with_params);
                self.node = old;
            }
            Some(t) => {
                let old_rule = self.current_template_rule;
                self.current_template_rule = Some(t);
                let content = self.sheet.templates[t].content;
                self.apply_xslt_template(context_node, content, Some(t), with_params);
                self.current_template_rule = old_rule;
            }
        }
    }

    /// xsltDefaultProcessOneNode
    pub fn default_process_one_node(&mut self, node: NodeId, params: &[Elem]) {
        if self.stopped() {
            return;
        }
        let ntype = tw(|t| t.nodes[node].t);
        match ntype {
            NType::Document | NType::HtmlDocument | NType::Element => {}
            NType::CData | NType::Text => {
                let ins = self.insert;
                if self.copy_text(ins, node).is_none() {
                    let m = if ntype == NType::CData {
                        "xsltDefaultProcessOneNode: cdata copy failed\n"
                    } else {
                        "xsltDefaultProcessOneNode: text copy failed\n"
                    };
                    self.error(Some(node), m);
                }
                return;
            }
            NType::Attribute => {
                let cur = tw(|t| {
                    let mut c = t.nodes[node].children;
                    while let Some(x) = c {
                        if t.nodes[x].t == NType::Text {
                            return Some(x);
                        }
                        c = t.nodes[x].next;
                    }
                    None
                });
                match cur {
                    None => self.error(
                        Some(node),
                        "xsltDefaultProcessOneNode: no text for attribute\n",
                    ),
                    Some(c) => {
                        let ins = self.insert;
                        if self.copy_text(ins, c).is_none() {
                            self.error(Some(node), "xsltDefaultProcessOneNode: text copy failed\n");
                        }
                    }
                }
                return;
            }
            _ => return,
        }
        let children = tw(|t| t.children_of(node));
        let nbchild = tw(|t| children.iter().filter(|&&c| is_real_node(t, c)).count());
        let (old_size, old_pos) = (self.xp.size, self.xp.pos);
        let mut childno = 0;
        for cur in children {
            childno += 1;
            let ctype = tw(|t| t.nodes[cur].t);
            match ctype {
                NType::Document | NType::HtmlDocument | NType::Element => {
                    self.xp.size = nbchild as i32;
                    self.xp.pos = childno;
                    if self.depth >= self.max_template_depth {
                        let m = format!(
                            "xsltDefaultProcessOneNode: Maximum template depth exceeded.\nYou can adjust xsltMaxDepth (--maxdepth) in order to raise the maximum number of nested template calls and variables/params (currently set to {}).\n",
                            self.max_template_depth
                        );
                        self.error(Some(cur), &m);
                        self.state = State::Stopped;
                        return;
                    }
                    self.depth += 1;
                    self.process_one_node(cur, params);
                    self.depth -= 1;
                }
                NType::CData | NType::Text => {
                    let templ = self.get_template(cur, None);
                    match templ {
                        Some(t) => {
                            if ctype == NType::Text {
                                self.xp.size = nbchild as i32;
                                self.xp.pos = childno;
                            }
                            let content = self.sheet.templates[t].content;
                            self.apply_xslt_template(cur, content, Some(t), params);
                        }
                        None => {
                            let ins = self.insert;
                            if self.copy_text(ins, cur).is_none() {
                                let m = if ctype == NType::CData {
                                    "xsltDefaultProcessOneNode: cdata copy failed\n"
                                } else {
                                    "xsltDefaultProcessOneNode: text copy failed\n"
                                };
                                self.error(Some(cur), m);
                            }
                        }
                    }
                }
                NType::Pi | NType::Comment => {
                    if let Some(t) = self.get_template(cur, None) {
                        self.xp.size = nbchild as i32;
                        self.xp.pos = childno;
                        let content = self.sheet.templates[t].content;
                        self.apply_xslt_template(cur, content, Some(t), params);
                    }
                }
                _ => {}
            }
            if self.stopped() {
                break;
            }
        }
        self.xp.size = old_size;
        self.xp.pos = old_pos;
    }

    /// xsltApplyXSLTTemplate
    pub fn apply_xslt_template(
        &mut self,
        context_node: NodeId,
        list: Option<NodeId>,
        templ: Option<usize>,
        with_params: &[Elem],
    ) {
        let templ = match templ {
            Some(t) => t,
            None => {
                self.error(
                    list,
                    "xsltApplyXSLTTemplate: Bad arguments; @templ is mandatory.\n",
                );
                return;
            }
        };
        let list = match list {
            Some(l) => l,
            None => return,
        };
        if self.stopped() {
            return;
        }
        if self.vars.len() as i32 >= self.max_template_vars {
            let m = format!(
                "xsltApplyXSLTTemplate: A potential infinite template recursion was detected.\nYou can adjust maxTemplateVars (--maxvars) in order to raise the maximum number of variables/params (currently set to {}).\n",
                self.max_template_vars
            );
            self.error(Some(list), &m);
            self.debug_dump(context_node, list);
            self.state = State::Stopped;
            return;
        }
        let old_vars_base = self.vars_base;
        self.vars_base = self.vars.len();
        self.node = context_node;
        self.templ_push(templ);
        // xsl:param handling
        let mut body = Some(list);
        let mut cur = Some(list);
        while let Some(c) = cur {
            let (ctype, is_param, next) = tw(|t| {
                (
                    t.nodes[c].t,
                    is_xslt_elem_named(t, c, "param"),
                    t.nodes[c].next,
                )
            });
            if ctype == NType::Text {
                cur = next;
                continue;
            }
            let pc = self.sheet.precomp.get(&c).cloned();
            let pc = match pc {
                Some(pc) if ctype == NType::Element && is_param => pc,
                _ => break,
            };
            body = next;
            self.push_param_or_withparam(c, &pc, with_params);
            cur = next;
        }
        self.apply_sequence_constructor(context_node, body, Some(templ));
        // xsltTemplateParamsCleanup
        if self.vars.len() > self.vars_base {
            self.vars.truncate(self.vars_base);
        }
        self.vars_base = old_vars_base;
        self.templ_pop();
    }

    fn push_param_or_withparam(&mut self, c: NodeId, pc: &Rc<PreComp>, with_params: &[Elem]) {
        let mut found = None;
        for wp in with_params {
            let b = wp.borrow();
            if Some(&b.name) == pc.name.as_ref() && b.name_uri == pc.ns {
                found = Some(wp.clone());
                break;
            }
        }
        match found {
            Some(wp) => {
                wp.borrow_mut().level = -1;
                self.vars.push(wp);
            }
            None => self.parse_stylesheet_param(c),
        }
    }

    /// xsltDebug: dump templates / variables stacks
    fn debug_dump(&mut self, _node: NodeId, _inst: NodeId) {
        let mut s = String::from("Templates:\n");
        let mut j = 0;
        for i in (0..self.templ_tab.len()).rev() {
            if j >= 15 {
                break;
            }
            s.push_str(&format!("#{} ", j));
            let t = &self.sheet.templates[self.templ_tab[i]];
            if let Some(n) = &t.name {
                s.push_str(&format!("name {} ", n));
            }
            if let Some(m) = &t.match_ {
                s.push_str(&format!("name {} ", m));
            }
            if let Some(m) = &t.mode {
                s.push_str(&format!("name {} ", m));
            }
            s.push('\n');
            j += 1;
        }
        for line in s.lines() {
            php_error(self.vm, &format!("{}\n", line));
        }
        let mut s = String::from("Variables:\n");
        let mut j = 0;
        let mut i = self.vars.len();
        while i > 0 && j < 15 {
            i -= 1;
            let v = self.vars[i].borrow();
            s.push_str(&format!("#{}\n", j));
            match v.comp.as_ref().map(|c| c.kind) {
                Some(Kind::Param) => s.push_str("param "),
                Some(Kind::Variable) => s.push_str("var "),
                _ => {}
            }
            s.push_str(&format!("{} ", v.name));
            s.push('\n');
            j += 1;
        }
        for line in s.lines() {
            php_error(self.vm, &format!("{}\n", line));
        }
    }

    /// xsltApplyOneTemplate (no new variable scope)
    pub fn apply_one_template(&mut self, context_node: NodeId, list: Option<NodeId>) {
        if list.is_none() || self.stopped() {
            return;
        }
        self.apply_sequence_constructor(context_node, list, None);
    }

    /// xsltLocalVariablePop
    pub fn local_variable_pop(&mut self, limit_nr: usize, level: i32) {
        while self.vars.len() > limit_nr {
            let lv = self.vars.last().unwrap().borrow().level;
            if lv <= level {
                break;
            }
            self.vars.pop();
        }
    }

    /// xsltApplySequenceConstructor
    pub fn apply_sequence_constructor(
        &mut self,
        context_node: NodeId,
        list: Option<NodeId>,
        templ: Option<usize>,
    ) {
        let list = match list {
            Some(l) => l,
            None => return,
        };
        if self.stopped() {
            return;
        }
        if self.depth >= self.max_template_depth {
            let m = format!(
                "xsltApplySequenceConstructor: A potential infinite template recursion was detected.\nYou can adjust xsltMaxDepth (--maxdepth) in order to raise the maximum number of nested template calls and variables/params (currently set to {}).\n",
                self.max_template_depth
            );
            self.error(Some(list), &m);
            self.debug_dump(context_node, list);
            self.state = State::Stopped;
            return;
        }
        self.depth += 1;
        let old_insert = self.insert;
        let mut insert = self.insert;
        let old_inst = self.inst;
        let old_context_node = self.node;
        let old_vars_nr = self.vars.len();
        let list_parent = tw(|t| t.nodes[list].parent);
        let mut level: i32 = 0;
        let mut cur = Some(list);
        'outer: while let Some(c) = cur {
            self.inst = Some(c);
            if insert.is_none() {
                break;
            }
            let mut copy: Option<NodeId> = None;
            let mut skip_children = false;
            let (ctype, is_x, has_ns, name) = tw(|t| {
                let n = &t.nodes[c];
                (n.t, is_xslt_elem(t, c), n.ns.is_some(), n.name.clone())
            });
            if is_x {
                let info = self.sheet.precomp.get(&c).cloned();
                match info {
                    None => {
                        if name == "message" {
                            self.message(context_node, c);
                        } else {
                            self.insert = insert;
                            if self.apply_fallbacks(context_node, c) == 0 {
                                self.generic_error(&format!(
                                    "xsltApplySequenceConstructor: {} was not compiled\n",
                                    name
                                ));
                            }
                            self.insert = old_insert;
                        }
                    }
                    Some(info) => {
                        if self.instruction_has_func(&info) {
                            let old_cur_inst = self.inst;
                            self.inst = Some(c);
                            self.insert = insert;
                            self.exec_instruction(context_node, c, &info);
                            self.insert = old_insert;
                            self.inst = old_cur_inst;
                        } else if name == "variable" {
                            let old_cur_inst = self.inst;
                            self.inst = Some(c);
                            let before = self.vars.len();
                            self.parse_stylesheet_variable(c);
                            self.inst = old_cur_inst;
                            if self.vars.len() != before {
                                self.vars.last().unwrap().borrow_mut().level = level;
                            }
                        } else if name == "message" {
                            self.message(context_node, c);
                        } else {
                            self.error(Some(c), &format!("Unexpected XSLT element '{}'.\n", name));
                        }
                    }
                }
                skip_children = true;
            } else if ctype == NType::Text || ctype == NType::CData {
                if self.copy_text(insert, c).is_none() {
                    break;
                }
            } else if ctype == NType::Element
                && has_ns
                && (self.sheet.precomp.contains_key(&c) || self.sheet.ext_marker.contains(&c))
            {
                let old_cur_inst = self.inst;
                self.inst = Some(c);
                let info = self.sheet.precomp.get(&c).cloned();
                let handled = match &info {
                    Some(i) => {
                        self.insert = insert;
                        self.exec_extension_element(context_node, c, i);
                        self.insert = old_insert;
                        true
                    }
                    None => {
                        // xsltExtMarker: look up a context-level element
                        let uri = tw(|t| t.ns_href(t.nodes[c].ns).map(|s| s.to_string()));
                        let known = uri
                            .as_deref()
                            .map(|u| crate::style::ext_module_element(&name, u))
                            .unwrap_or(false);
                        if known {
                            self.insert = insert;
                            let pc = Rc::new(PreComp {
                                kind: Kind::Extension,
                                inst: c,
                                ext: Some((uri.clone().unwrap(), name.clone())),
                                ..Default::default()
                            });
                            self.exec_extension_element(context_node, c, &pc);
                            self.insert = old_insert;
                            true
                        } else {
                            false
                        }
                    }
                };
                if !handled {
                    self.insert = insert;
                    let mut found = false;
                    let kids = tw(|t| t.children_of(c));
                    for k in kids {
                        if tw(|t| is_xslt_elem_named(t, k, "fallback")) {
                            found = true;
                            let ch = tw(|t| t.nodes[k].children);
                            self.apply_sequence_constructor(context_node, ch, None);
                        }
                    }
                    self.insert = old_insert;
                    if !found {
                        self.error(
                            Some(c),
                            &format!(
                                "xsltApplySequenceConstructor: failed to find extension {}\n",
                                name
                            ),
                        );
                    }
                }
                self.inst = old_cur_inst;
                skip_children = true;
            } else if ctype == NType::Element {
                let old_cur_inst = self.inst;
                self.inst = Some(c);
                copy = self.shallow_copy_elem(c, insert, true);
                let cp = match copy {
                    Some(x) => x,
                    None => break 'outer,
                };
                // inherited namespaces of the template
                if templ.is_some() && old_insert == insert {
                    if let Some(ct) = self.templ() {
                        let inherited = self.sheet.templates[ct].inherited_ns.clone();
                        if !inherited.is_empty() {
                            for (prefix, href) in inherited {
                                let mut uri: Option<Option<String>> = None;
                                for s in self.sheet.import_order() {
                                    if let Some((_, v)) = self.sheet.styles[s]
                                        .ns_aliases
                                        .iter()
                                        .find(|(k, _)| *k == href)
                                    {
                                        uri = Some(v.clone());
                                        break;
                                    }
                                }
                                let u = match uri {
                                    Some(None) => continue,
                                    Some(Some(u)) => u,
                                    None => href.clone(),
                                };
                                tw(|t| {
                                    let r = t.search_ns(cp, prefix.as_deref());
                                    let need = match r {
                                        None => true,
                                        Some(r) => t.nss[r].href != u,
                                    };
                                    if need {
                                        t.new_ns(Some(cp), &u, prefix.as_deref());
                                    }
                                });
                            }
                            let cns = tw(|t| t.nodes[cp].ns);
                            if cns.is_some() {
                                let r = self.get_namespace(c, cns, cp);
                                tw(|t| t.nodes[cp].ns = r);
                            }
                        }
                    }
                }
                let attrs = tw(|t| t.attrs_of(c));
                if !attrs.is_empty() {
                    self.attr_list_template_process(cp, &attrs);
                }
                self.inst = old_cur_inst;
            }
            // descend into content
            if !skip_children {
                let ch = tw(|t| {
                    t.nodes[c]
                        .children
                        .filter(|&x| t.nodes[x].t != NType::EntityDecl)
                });
                if let Some(ch) = ch {
                    cur = Some(ch);
                    level += 1;
                    if copy.is_some() {
                        insert = copy;
                    }
                    continue;
                }
            }
            // skip_children:
            if self.state == State::Stopped {
                break;
            }
            let mut c2 = c;
            if let Some(n) = tw(|t| t.nodes[c2].next) {
                cur = Some(n);
                continue;
            }
            loop {
                let p = tw(|t| t.nodes[c2].parent);
                level -= 1;
                if self.vars.len() > old_vars_nr {
                    if let Some(lv) = self.vars.last().map(|v| v.borrow().level) {
                        if lv > level {
                            self.local_variable_pop(old_vars_nr, level);
                        }
                    }
                }
                insert = insert.and_then(|i| tw(|t| t.nodes[i].parent));
                match p {
                    None => {
                        cur = None;
                        break;
                    }
                    Some(p) => {
                        if Some(p) == list_parent {
                            cur = None;
                            break;
                        }
                        c2 = p;
                        if let Some(n) = tw(|t| t.nodes[p].next) {
                            cur = Some(n);
                            break;
                        }
                    }
                }
            }
        }
        if self.vars.len() > old_vars_nr {
            self.local_variable_pop(old_vars_nr, -1);
        }
        self.node = old_context_node;
        self.inst = old_inst;
        self.insert = old_insert;
        self.depth -= 1;
    }

    fn instruction_has_func(&self, info: &PreComp) -> bool {
        !matches!(
            info.kind,
            Kind::WithParam | Kind::Param | Kind::Variable | Kind::When
        )
    }

    /// xsltApplyFallbacks
    fn apply_fallbacks(&mut self, node: NodeId, inst: NodeId) -> i32 {
        let kids = tw(|t| t.children_of(inst));
        let mut ret = 0;
        for k in kids {
            if tw(|t| is_xslt_elem_named(t, k, "fallback")) {
                ret += 1;
                let ch = tw(|t| t.nodes[k].children);
                self.apply_sequence_constructor(node, ch, None);
            }
        }
        ret
    }

    /// Run a compiled XSLT instruction (info->func)
    fn exec_instruction(&mut self, node: NodeId, inst: NodeId, comp: &Rc<PreComp>) {
        match comp.kind {
            Kind::Copy => self.xsl_copy(node, inst, comp),
            Kind::Sort => {
                self.error(
                    Some(inst),
                    "xsl:sort : improper use this should not be reached\n",
                );
            }
            Kind::Text => self.xsl_text(inst),
            Kind::Element => self.xsl_element(node, inst, comp),
            Kind::Attribute => self.xsl_attribute(node, inst, comp),
            Kind::Comment => self.xsl_comment(node, inst),
            Kind::Pi => self.xsl_processing_instruction(node, inst, comp),
            Kind::CopyOf => self.xsl_copy_of(node, inst, comp),
            Kind::ValueOf => self.xsl_value_of(node, inst, comp),
            Kind::Number => self.xsl_number(node, inst, comp),
            Kind::ApplyImports => self.xsl_apply_imports(node, inst),
            Kind::CallTemplate => self.xsl_call_template(node, inst, comp),
            Kind::ApplyTemplates => self.xsl_apply_templates(node, inst, comp),
            Kind::Choose => self.xsl_choose(node, inst),
            Kind::If => self.xsl_if(node, inst, comp),
            Kind::ForEach => self.xsl_for_each(node, inst, comp),
            Kind::Document => self.document_elem(node, inst, comp),
            Kind::Extension => self.exec_extension_element(node, inst, comp),
            Kind::WithParam | Kind::Param | Kind::Variable | Kind::When => {}
        }
    }

    /// An extension element with a handler
    fn exec_extension_element(&mut self, node: NodeId, inst: NodeId, comp: &Rc<PreComp>) {
        let (uri, name) = match &comp.ext {
            Some(x) => x.clone(),
            None => {
                if comp.kind == Kind::Document {
                    return self.document_elem(node, inst, comp);
                }
                return;
            }
        };
        match (uri.as_str(), name.as_str()) {
            (crate::style::EXSLT_FUNCTIONS_NAMESPACE, "result") => {
                crate::exslt::func_result_elem(self, node, inst, comp);
            }
            (crate::style::XSLT_LIBXSLT_NAMESPACE, "debug") => {
                self.debug_dump(node, inst);
            }
            _ => self.document_elem(node, inst, comp),
        }
    }

    // ------------------------------------------------------------------
    // Instructions

    /// xsltCopy
    fn xsl_copy(&mut self, node: NodeId, inst: NodeId, comp: &Rc<PreComp>) {
        let old_insert = self.insert;
        let ntype = tw(|t| t.nodes[node].t);
        if self.insert.is_some() {
            match ntype {
                NType::Text | NType::CData => {
                    let ins = self.insert;
                    self.copy_text(ins, node);
                }
                NType::Document | NType::HtmlDocument => {}
                NType::Element => {
                    let ins = self.insert;
                    let copy = self.shallow_copy_elem(node, ins, false);
                    self.insert = copy;
                    if let Some(u) = &comp.use_ {
                        let u = u.clone();
                        self.apply_attribute_set(node, Some(inst), Some(&u));
                    }
                }
                NType::Attribute => {
                    let ins = self.insert;
                    self.shallow_copy_attr(Some(inst), ins, node);
                }
                NType::Pi => {
                    let (name, content) =
                        tw(|t| (t.nodes[node].name.clone(), t.nodes[node].content.clone()));
                    let out = self.output;
                    let pi = tw(|t| t.new_pi(Some(out), &name, &content));
                    let ins = self.insert;
                    self.add_child(ins, pi);
                }
                NType::Comment => {
                    let content = tw(|t| t.nodes[node].content.clone());
                    let out = self.output;
                    let c = tw(|t| t.new_comment(Some(out), &content));
                    let ins = self.insert;
                    self.add_child(ins, c);
                }
                NType::NamespaceDecl => {
                    let ins = self.insert;
                    self.shallow_copy_ns_node(Some(inst), ins, node);
                }
                _ => {}
            }
        }
        match ntype {
            NType::Document | NType::HtmlDocument | NType::Element => {
                let ch = tw(|t| t.nodes[inst].children);
                let cn = self.node;
                self.apply_sequence_constructor(cn, ch, None);
            }
            _ => {}
        }
        self.insert = old_insert;
    }

    /// xsltText (disable-output-escaping xsl:text kept as instruction)
    fn xsl_text(&mut self, inst: NodeId) {
        let kids = tw(|t| t.children_of(inst));
        for k in kids {
            let (kt, content) = tw(|t| (t.nodes[k].t, t.nodes[k].content.clone()));
            if kt != NType::Text && kt != NType::CData {
                self.error(Some(inst), "xsl:text content problem\n");
                break;
            }
            let out = self.output;
            let copy = tw(|t| {
                let c = t.new_text(Some(out), &content);
                if kt != NType::CData {
                    t.nodes[c].name = "textnoenc".into();
                    t.nodes[c].flags |= 1;
                }
                c
            });
            let ins = self.insert;
            self.add_child(ins, copy);
        }
    }

    /// xsltElement
    fn xsl_element(&mut self, node: NodeId, inst: NodeId, comp: &Rc<PreComp>) {
        if self.insert.is_none() || !comp.has_name {
            return;
        }
        let old_insert = self.insert;
        let (prefix, name) = match &comp.name {
            None => {
                let prop = match self.eval_attr_value_template(inst, "name", Some(XSLT_NAMESPACE)) {
                    Some(p) => p,
                    None => {
                        self.error(
                            Some(inst),
                            "xsl:element: The attribute 'name' is missing.\n",
                        );
                        self.insert = old_insert;
                        return;
                    }
                };
                if !valid_qname(&prop) {
                    self.error(
                        Some(inst),
                        &format!(
                            "xsl:element: The effective name '{}' is not a valid QName.\n",
                            prop
                        ),
                    );
                }
                split_qname(&prop)
            }
            Some(n) => split_qname(n),
        };
        let mut prefix = prefix;
        let out = self.output;
        let copy = tw(|t| t.new_element(Some(out), &name));
        let copy = match self.add_child(old_insert, copy) {
            Some(c) => c,
            None => {
                self.error(Some(inst), "xsl:element : xsltAddChild failed\n");
                return;
            }
        };
        let mut ns_name: Option<String> = None;
        if comp.has_ns {
            if let Some(n) = &comp.ns {
                if !n.is_empty() {
                    ns_name = Some(n.clone());
                }
            } else {
                let tmp = self.eval_attr_value_template(inst, "namespace", Some(XSLT_NAMESPACE));
                if let Some(t) = tmp {
                    if !t.is_empty() {
                        ns_name = Some(t);
                    }
                }
            }
            if ns_name.as_deref() == Some("http://www.w3.org/2000/xmlns/") {
                self.error(
                    Some(inst),
                    "xsl:attribute: Namespace http://www.w3.org/2000/xmlns/ forbidden.\n",
                );
                self.insert = old_insert;
                return;
            }
            if ns_name.as_deref() == Some(goro_ext_xml::tree::XML_XML_NAMESPACE) {
                prefix = Some("xml".into());
            } else if prefix.as_deref() == Some("xml") {
                prefix = None;
            }
        } else {
            let ns = tw(|t| {
                t.search_ns(inst, prefix.as_deref())
                    .map(|n| t.nss[n].href.clone())
            });
            match ns {
                None => {
                    if let Some(p) = &prefix {
                        self.error(
                            Some(inst),
                            &format!(
                                "xsl:element: The QName '{}:{}' has no namespace binding in scope in the stylesheet; this is an error, since the namespace was not specified by the instruction itself.\n",
                                p, name
                            ),
                        );
                    }
                }
                Some(h) => ns_name = Some(h),
            }
        }
        if let Some(nsn) = &ns_name {
            let r = if prefix.as_deref() == Some("xmlns") {
                self.get_special_namespace(Some(inst), Some(nsn), Some("ns_1"), copy)
            } else {
                self.get_special_namespace(Some(inst), Some(nsn), prefix.as_deref(), copy)
            };
            tw(|t| t.nodes[copy].ns = r);
        } else {
            let undecl = tw(|t| {
                t.nodes[copy]
                    .parent
                    .map(|p| t.nodes[p].t == NType::Element && t.nodes[p].ns.is_some())
                    .unwrap_or(false)
            });
            if undecl {
                self.get_special_namespace(Some(inst), None, None, copy);
            }
        }
        self.insert = Some(copy);
        if comp.has_use {
            if let Some(u) = &comp.use_ {
                let u = u.clone();
                self.apply_attribute_set(node, Some(inst), Some(&u));
            } else if let Some(sets) =
                self.eval_attr_value_template(inst, "use-attribute-sets", None)
            {
                self.apply_attribute_set(node, Some(inst), Some(&sets));
            }
        }
        let ch = tw(|t| t.nodes[inst].children);
        if ch.is_some() {
            let cn = self.node;
            self.apply_sequence_constructor(cn, ch, None);
        }
        self.insert = old_insert;
    }

    /// xsltEvalTemplateString
    pub fn eval_template_string(&mut self, context_node: NodeId, inst: NodeId) -> Option<String> {
        let ch = tw(|t| {
            if t.nodes[inst].t != NType::Element {
                None
            } else {
                t.nodes[inst].children
            }
        })?;
        let out = self.output;
        let fake = tw(|t| t.new_element(Some(out), "fake"));
        let old_insert = self.insert;
        self.insert = Some(fake);
        self.apply_one_template(context_node, Some(ch));
        self.insert = old_insert;
        Some(tw(|t| t.node_get_content(fake)))
    }

    /// xsltComment
    fn xsl_comment(&mut self, node: NodeId, inst: NodeId) {
        let value = self.eval_template_string(node, inst);
        if let Some(v) = &value {
            if !v.is_empty() && (v.ends_with('-') || v.contains("--")) {
                self.error(
                    Some(inst),
                    "xsl:comment : '--' or ending '-' not allowed in comment\n",
                );
            }
        }
        let out = self.output;
        let c = tw(|t| {
            let c = t.new_comment(Some(out), value.as_deref().unwrap_or(""));
            if value.is_none() {
                t.nodes[c].flags |= goro_ext_xml::tree::F_NULL_CONTENT;
            }
            c
        });
        let ins = self.insert;
        self.add_child(ins, c);
    }

    /// xsltProcessingInstruction
    fn xsl_processing_instruction(&mut self, node: NodeId, inst: NodeId, comp: &Rc<PreComp>) {
        if self.insert.is_none() || !comp.has_name {
            return;
        }
        let name = match &comp.name {
            None => match self.eval_attr_value_template(inst, "name", None) {
                Some(n) => n,
                None => {
                    self.error(Some(inst), "xsl:processing-instruction : name is missing\n");
                    return;
                }
            },
            Some(n) => n.clone(),
        };
        let value = self.eval_template_string(node, inst);
        if value.as_deref().map(|v| v.contains("?>")).unwrap_or(false) {
            self.error(
                Some(inst),
                "xsl:processing-instruction: '?>' not allowed within PI content\n",
            );
            return;
        }
        let out = self.output;
        let pi = tw(|t| t.new_pi(Some(out), &name, value.as_deref().unwrap_or("")));
        let ins = self.insert;
        self.add_child(ins, pi);
    }

    /// xsltCopyOf
    fn xsl_copy_of(&mut self, node: NodeId, inst: NodeId, comp: &Rc<PreComp>) {
        let c = match &comp.comp {
            Some(c) => c.clone(),
            None => {
                self.error(Some(inst), "xsl:copy-of : compilation failed\n");
                return;
            }
        };
        let res = self.eval_comp(node, &c, &comp.ns_list);
        match res {
            Some(XObj::NodeSet(list)) => {
                for cur in list {
                    let ct = tw(|t| t.nodes[cur].t);
                    let ins = self.insert;
                    if matches!(ct, NType::Document | NType::HtmlDocument) {
                        let ch = tw(|t| t.nodes[cur].children);
                        self.copy_tree_list(Some(inst), ch, ins, false, false);
                    } else if ct == NType::Attribute {
                        self.shallow_copy_attr(Some(inst), ins, cur);
                    } else {
                        self.copy_tree(Some(inst), cur, ins, false, false);
                    }
                }
            }
            Some(XObj::Tree(list)) => {
                if let Some(&first) = list.first() {
                    let ch = tw(|t| t.nodes[first].children);
                    let ins = self.insert;
                    self.copy_tree_list(Some(inst), ch, ins, false, false);
                }
            }
            Some(o) => {
                let v = xpath::cast_to_string(&o);
                if !v.is_empty() {
                    let ins = self.insert;
                    self.copy_text_string(ins, &v, false);
                }
            }
            None => {
                self.state = State::Stopped;
            }
        }
    }

    /// xsltValueOf
    fn xsl_value_of(&mut self, node: NodeId, inst: NodeId, comp: &Rc<PreComp>) {
        let c = match &comp.comp {
            Some(c) => c.clone(),
            None => {
                self.error(
                    Some(inst),
                    "Internal error in xsltValueOf(): The XSLT 'value-of' instruction was not compiled.\n",
                );
                return;
            }
        };
        let res = self.eval_comp(node, &c, &comp.ns_list);
        match res {
            Some(o) => {
                let v = xpath::cast_to_string(&o);
                if !v.is_empty() {
                    let ins = self.insert;
                    self.copy_text_string(ins, &v, comp.noescape);
                }
            }
            None => {
                self.error(Some(inst), "XPath evaluation returned no result.\n");
                self.state = State::Stopped;
            }
        }
    }

    /// xsltNumber
    fn xsl_number(&mut self, node: NodeId, inst: NodeId, comp: &Rc<PreComp>) {
        let old_ns = self.xp.namespaces.clone();
        self.xp.namespaces = comp.ns_list.clone();
        crate::numbers::number_format(self, comp, inst, node);
        self.xp.namespaces = old_ns;
    }

    /// xsltApplyImports
    fn xsl_apply_imports(&mut self, context_node: NodeId, inst: NodeId) {
        let rule = match self.current_template_rule {
            None => {
                self.error(
                    Some(inst),
                    "It is an error to call 'apply-imports' when there's no current template rule.\n",
                );
                return;
            }
            Some(r) => r,
        };
        let style = self.sheet.templates[rule].style;
        let templ = self.get_template(context_node, Some(style));
        match templ {
            Some(t) => {
                let old = self.current_template_rule;
                self.current_template_rule = Some(t);
                let content = self.sheet.templates[t].content;
                self.apply_xslt_template(context_node, content, Some(t), &[]);
                self.current_template_rule = old;
            }
            None => self.default_process_one_node(context_node, &[]),
        }
    }

    /// xsltFindTemplate
    pub fn find_template(&self, name: &str, uri: Option<&str>) -> Option<usize> {
        for s in self.sheet.import_order() {
            if let Some(&t) = self.sheet.styles[s]
                .named_templates
                .get(&(name.to_string(), uri.map(|u| u.to_string())))
            {
                return Some(t);
            }
        }
        None
    }

    /// xsltCallTemplate
    fn xsl_call_template(&mut self, node: NodeId, inst: NodeId, comp: &Rc<PreComp>) {
        if self.insert.is_none() {
            return;
        }
        let name = comp.name.clone().unwrap_or_default();
        let templ = match self.find_template(&name, comp.ns.as_deref()) {
            Some(t) => t,
            None => {
                match &comp.ns {
                    Some(ns) => self.error(
                        Some(inst),
                        &format!("The called template '{{{}}}{}' was not found.\n", ns, name),
                    ),
                    None => self.error(
                        Some(inst),
                        &format!("The called template '{}' was not found.\n", name),
                    ),
                }
                return;
            }
        };
        let mut with_params: Vec<Elem> = Vec::new();
        let kids = tw(|t| t.children_of(inst));
        for cur in kids {
            if self.stopped() {
                break;
            }
            let (is_x, cname) = tw(|t| (is_xslt_elem(t, cur), t.nodes[cur].name.clone()));
            if is_x {
                if cname == "with-param" {
                    if let Some(p) = self.parse_stylesheet_caller_param(cur) {
                        with_params.insert(0, p);
                    }
                } else {
                    self.generic_error(&format!("xsl:call-template: misplaced xsl:{}\n", cname));
                }
            } else {
                self.generic_error(&format!("xsl:call-template: misplaced {} element\n", cname));
            }
        }
        let content = self.sheet.templates[templ].content;
        self.apply_xslt_template(node, content, Some(templ), &with_params);
    }

    /// xsltApplyTemplates
    fn xsl_apply_templates(&mut self, node: NodeId, inst: NodeId, comp: &Rc<PreComp>) {
        let old_context_node = self.node;
        let old_mode = self.mode.clone();
        let old_mode_uri = self.mode_uri.clone();
        let old_doc_info = self.document;
        let old_list = std::mem::take(&mut self.node_list);
        let (old_size, old_pos, old_doc) = (self.xp.size, self.xp.pos, self.xp.doc);
        self.mode = comp.mode.clone();
        self.mode_uri = comp.mode_uri.clone();
        let mut with_params: Vec<Elem> = Vec::new();
        let mut list: Vec<NodeId>;
        'body: {
            if comp.select.is_some() {
                let c = match &comp.comp {
                    Some(c) => c.clone(),
                    None => {
                        self.error(Some(inst), "xsl:apply-templates : compilation failed\n");
                        break 'body;
                    }
                };
                match self.eval_comp(node, &c, &comp.ns_list) {
                    Some(XObj::NodeSet(v)) => list = v,
                    Some(_) => {
                        self.error(
                            Some(inst),
                            "The 'select' expression did not evaluate to a node set.\n",
                        );
                        self.state = State::Stopped;
                        break 'body;
                    }
                    None => {
                        self.error(Some(inst), "Failed to evaluate the 'select' expression.\n");
                        self.state = State::Stopped;
                        break 'body;
                    }
                }
            } else {
                list = tw(|t| {
                    if t.nodes[node].t == NType::NamespaceDecl {
                        return Vec::new();
                    }
                    t.children_of(node)
                        .into_iter()
                        .filter(|&c| is_real_node(t, c))
                        .collect()
                });
            }
            if list.is_empty() {
                break 'body;
            }
            self.node_list = list.clone();
            let kids = tw(|t| t.children_of(inst));
            let mut i = 0;
            while i < kids.len() {
                let cur = kids[i];
                if self.stopped() {
                    break;
                }
                let (ctype, is_x, cname) = tw(|t| {
                    (
                        t.nodes[cur].t,
                        is_xslt_elem(t, cur),
                        t.nodes[cur].name.clone(),
                    )
                });
                if ctype == NType::Text {
                    i += 1;
                    continue;
                }
                if !is_x {
                    break;
                }
                if cname == "with-param" {
                    if let Some(p) = self.parse_stylesheet_caller_param(cur) {
                        with_params.insert(0, p);
                    }
                }
                if cname == "sort" {
                    let old_rule = self.current_template_rule;
                    let mut sorts = vec![cur];
                    i += 1;
                    while i < kids.len() {
                        let c2 = kids[i];
                        if self.stopped() {
                            break;
                        }
                        let (ct2, is_x2, n2) =
                            tw(|t| (t.nodes[c2].t, is_xslt_elem(t, c2), t.nodes[c2].name.clone()));
                        if ct2 == NType::Text {
                            i += 1;
                            continue;
                        }
                        if !is_x2 {
                            break;
                        }
                        if n2 == "with-param" {
                            if let Some(p) = self.parse_stylesheet_caller_param(c2) {
                                with_params.insert(0, p);
                            }
                        }
                        if n2 == "sort" {
                            if sorts.len() >= XSLT_MAX_SORT {
                                self.error(
                                    Some(c2),
                                    &format!(
                                        "The number ({}) of xsl:sort instructions exceeds the maximum allowed by this processor's settings.\n",
                                        sorts.len()
                                    ),
                                );
                                self.state = State::Stopped;
                                break;
                            } else {
                                sorts.push(c2);
                            }
                        }
                        i += 1;
                    }
                    self.current_template_rule = None;
                    crate::sort::do_sort(self, &sorts, &mut list);
                    self.current_template_rule = old_rule;
                    break;
                }
                i += 1;
            }
            self.xp.size = list.len() as i32;
            for (i, &cur) in list.iter().enumerate() {
                self.node = cur;
                let (ct, d) = tw(|t| (t.nodes[cur].t, t.nodes[cur].doc));
                if ct != NType::NamespaceDecl {
                    if let Some(d) = d {
                        self.xp.doc = Some(d);
                    }
                }
                self.xp.pos = i as i32 + 1;
                self.process_one_node(cur, &with_params);
            }
        }
        self.xp.doc = old_doc;
        self.xp.size = old_size;
        self.xp.pos = old_pos;
        self.document = old_doc_info;
        self.node_list = old_list;
        self.node = old_context_node;
        self.mode = old_mode;
        self.mode_uri = old_mode_uri;
    }

    /// xsltChoose
    fn xsl_choose(&mut self, context_node: NodeId, inst: NodeId) {
        let first = tw(|t| t.nodes[inst].children);
        let mut cur = match first {
            None => {
                self.error(Some(inst), "xsl:choose: The instruction has no content.\n");
                return;
            }
            Some(c) => c,
        };
        if !tw(|t| is_xslt_elem_named(t, cur, "when")) {
            self.error(Some(inst), "xsl:choose: xsl:when expected first\n");
            return;
        }
        loop {
            if !tw(|t| is_xslt_elem_named(t, cur, "when")) {
                break;
            }
            let wcomp = self.sheet.precomp.get(&cur).cloned();
            let wcomp = match wcomp.filter(|w| w.test.is_some() && w.comp.is_some()) {
                Some(w) => w,
                None => {
                    self.error(
                        Some(cur),
                        "Internal error in xsltChoose(): The XSLT 'when' instruction was not compiled.\n",
                    );
                    return;
                }
            };
            let c = wcomp.comp.clone().unwrap();
            let res = self.eval_comp_bool(context_node, &c, &wcomp.ns_list);
            if res == -1 {
                self.state = State::Stopped;
                return;
            }
            if res == 1 {
                let ch = tw(|t| t.nodes[cur].children);
                let cn = self.node;
                self.apply_sequence_constructor(cn, ch, None);
                return;
            }
            match tw(|t| t.nodes[cur].next) {
                Some(n) => cur = n,
                None => return,
            }
        }
        if tw(|t| is_xslt_elem_named(t, cur, "otherwise")) {
            let ch = tw(|t| t.nodes[cur].children);
            let cn = self.node;
            self.apply_sequence_constructor(cn, ch, None);
        }
    }

    /// xsltIf
    fn xsl_if(&mut self, context_node: NodeId, inst: NodeId, comp: &Rc<PreComp>) {
        let c = match (&comp.test, &comp.comp) {
            (Some(_), Some(c)) => c.clone(),
            _ => {
                self.error(
                    Some(inst),
                    "Internal error in xsltIf(): The XSLT 'if' instruction was not compiled.\n",
                );
                return;
            }
        };
        let res = self.eval_comp_bool(context_node, &c, &comp.ns_list);
        if res == -1 {
            self.state = State::Stopped;
            return;
        }
        if res == 1 {
            let ch = tw(|t| t.nodes[inst].children);
            self.apply_sequence_constructor(context_node, ch, None);
        }
    }

    /// xsltForEach
    fn xsl_for_each(&mut self, context_node: NodeId, inst: NodeId, comp: &Rc<PreComp>) {
        let c = match (&comp.select, &comp.comp) {
            (Some(_), Some(c)) => c.clone(),
            _ => {
                self.error(
                    Some(inst),
                    "Internal error in xsltForEach(): The selecting expression of the XSLT 'for-each' instruction was not compiled correctly.\n",
                );
                return;
            }
        };
        let old_doc_info = self.document;
        let old_list = std::mem::take(&mut self.node_list);
        let old_context_node = self.node;
        let old_rule = self.current_template_rule;
        self.current_template_rule = None;
        let (old_doc, old_pos, old_size) = (self.xp.doc, self.xp.pos, self.xp.size);
        'body: {
            let mut list = match self.eval_comp(context_node, &c, &comp.ns_list) {
                Some(XObj::NodeSet(v)) => v,
                Some(_) => {
                    self.error(
                        Some(inst),
                        "The 'select' expression does not evaluate to a node set.\n",
                    );
                    break 'body;
                }
                None => {
                    self.error(Some(inst), "Failed to evaluate the 'select' expression.\n");
                    self.state = State::Stopped;
                    break 'body;
                }
            };
            if list.is_empty() {
                break 'body;
            }
            self.node_list = list.clone();
            let mut cur_inst = tw(|t| t.nodes[inst].children);
            if let Some(ci) = cur_inst {
                if tw(|t| is_xslt_elem_named(t, ci, "sort")) {
                    let mut sorts = vec![ci];
                    cur_inst = tw(|t| t.nodes[ci].next);
                    while let Some(c2) = cur_inst {
                        if !tw(|t| is_xslt_elem_named(t, c2, "sort")) {
                            break;
                        }
                        if sorts.len() >= XSLT_MAX_SORT {
                            self.error(
                                Some(c2),
                                &format!(
                                    "The number of xsl:sort instructions exceeds the maximum ({}) allowed by this processor.\n",
                                    XSLT_MAX_SORT
                                ),
                            );
                            break 'body;
                        }
                        sorts.push(c2);
                        cur_inst = tw(|t| t.nodes[c2].next);
                    }
                    crate::sort::do_sort(self, &sorts, &mut list);
                }
            }
            self.xp.size = list.len() as i32;
            for (i, &cur) in list.iter().enumerate() {
                self.node = cur;
                let (ct, d) = tw(|t| (t.nodes[cur].t, t.nodes[cur].doc));
                if ct != NType::NamespaceDecl {
                    if let Some(d) = d {
                        self.xp.doc = Some(d);
                    }
                }
                self.xp.pos = i as i32 + 1;
                self.apply_sequence_constructor(cur, cur_inst, None);
            }
        }
        self.document = old_doc_info;
        self.node_list = old_list;
        self.node = old_context_node;
        self.current_template_rule = old_rule;
        self.xp.doc = old_doc;
        self.xp.size = old_size;
        self.xp.pos = old_pos;
    }

    /// xsltMessage
    pub fn message(&mut self, node: NodeId, inst: NodeId) {
        let mut terminate = false;
        if let Some(prop) = tw(|t| get_ns_prop(t, inst, "terminate", None)) {
            if prop == "yes" {
                terminate = true;
            } else if prop != "no" {
                self.error(
                    Some(inst),
                    "xsl:message : terminate expecting 'yes' or 'no'\n",
                );
            }
        }
        let message = self.eval_template_string(node, inst);
        if let Some(m) = message {
            let len = m.len();
            php_error(self.vm, &m);
            if len > 0 && !m.ends_with('\n') {
                php_error(self.vm, "\n");
            }
        }
        if terminate {
            self.state = State::Stopped;
        }
    }

    // ------------------------------------------------------------------
    // Attributes (attributes.c, templates.c)

    /// xsltAttribute
    pub fn xsl_attribute(&mut self, context_node: NodeId, inst: NodeId, comp: &Rc<PreComp>) {
        if !comp.has_name {
            return;
        }
        let target = match self.insert {
            Some(t) => t,
            None => return,
        };
        let (ttype, tchildren) = tw(|t| (t.nodes[target].t, t.nodes[target].children.is_some()));
        if ttype != NType::Element {
            return;
        }
        if tchildren {
            self.error(
                Some(inst),
                "xsl:attribute: Cannot add attributes to an element if children have been already added to the element.\n",
            );
            return;
        }
        let (mut prefix, name) = match &comp.name {
            None => {
                let prop = match self.eval_attr_value_template(inst, "name", Some(XSLT_NAMESPACE)) {
                    Some(p) => p,
                    None => {
                        self.error(
                            Some(inst),
                            "xsl:attribute: The attribute 'name' is missing.\n",
                        );
                        return;
                    }
                };
                if !valid_qname(&prop) {
                    self.error(
                        Some(inst),
                        &format!(
                            "xsl:attribute: The effective name '{}' is not a valid QName.\n",
                            prop
                        ),
                    );
                }
                if prop == "xmlns" {
                    self.error(
                        Some(inst),
                        "xsl:attribute: The effective name 'xmlns' is not allowed.\n",
                    );
                    return;
                }
                split_qname(&prop)
            }
            Some(n) => split_qname(n),
        };
        let mut ns_name: Option<String> = None;
        if comp.has_ns {
            if let Some(n) = &comp.ns {
                if !n.is_empty() {
                    ns_name = Some(n.clone());
                }
            } else if let Some(tmp) =
                self.eval_attr_value_template(inst, "namespace", Some(XSLT_NAMESPACE))
            {
                if !tmp.is_empty() {
                    ns_name = Some(tmp);
                }
            }
            if ns_name.as_deref() == Some("http://www.w3.org/2000/xmlns/") {
                self.error(
                    Some(inst),
                    "xsl:attribute: Namespace http://www.w3.org/2000/xmlns/ forbidden.\n",
                );
                return;
            }
            if ns_name.as_deref() == Some(goro_ext_xml::tree::XML_XML_NAMESPACE) {
                prefix = Some("xml".into());
            } else if prefix.as_deref() == Some("xml") {
                prefix = None;
            }
        } else if let Some(p) = &prefix {
            match tw(|t| t.search_ns(inst, Some(p)).map(|n| t.nss[n].href.clone())) {
                None => {
                    self.error(
                        Some(inst),
                        &format!(
                            "xsl:attribute: The QName '{}:{}' has no namespace binding in scope in the stylesheet; this is an error, since the namespace was not specified by the instruction itself.\n",
                            p, name
                        ),
                    );
                }
                Some(h) => ns_name = Some(h),
            }
        }
        let mut ns = None;
        if let Some(nsn) = &ns_name {
            ns = if prefix.is_none() || prefix.as_deref() == Some("xmlns") {
                self.get_special_namespace(Some(inst), Some(nsn), Some("ns_1"), target)
            } else {
                self.get_special_namespace(Some(inst), Some(nsn), prefix.as_deref(), target)
            };
            if ns.is_none() {
                self.error(
                    Some(inst),
                    &format!(
                        "Namespace fixup error: Failed to acquire an in-scope namespace binding for the generated attribute '{{{}}}{}'.\n",
                        nsn, name
                    ),
                );
                return;
            }
        }
        let (children, single_text) = tw(|t| {
            let ch = t.nodes[inst].children;
            let single = ch.and_then(|c| {
                if t.nodes[c].next.is_none() && matches!(t.nodes[c].t, NType::Text | NType::CData) {
                    Some((t.nodes[c].content.clone(), t.nodes[c].name == "textnoenc"))
                } else {
                    None
                }
            });
            (ch, single)
        });
        let ins = self.insert.unwrap();
        if children.is_none() {
            tw(|t| t.set_ns_prop(ins, ns, &name, ""));
        } else if let Some((text, noenc)) = single_text {
            tw(|t| {
                let a = t.set_ns_prop(ins, ns, &name, "");
                // replace the value by a copy of the text node
                let kids = t.children_of(a);
                for k in kids {
                    t.unlink(k);
                }
                let doc = t.nodes[a].doc;
                let tx = t.new_text(doc, &text);
                if noenc {
                    t.nodes[tx].name = "textnoenc".into();
                }
                t.link_child_raw(a, tx);
                if t.is_id(ins, a) {
                    t.add_id_safe(a, &text);
                }
            });
        } else {
            let value = self.eval_template_string(context_node, inst);
            let v = value.unwrap_or_default();
            tw(|t| t.set_ns_prop(ins, ns, &name, &v));
        }
    }

    /// xsltApplyAttributeSet
    pub fn apply_attribute_set(
        &mut self,
        node: NodeId,
        inst: Option<NodeId>,
        attr_sets: Option<&str>,
    ) {
        let sets: String = match attr_sets {
            Some(s) => s.to_string(),
            None => {
                let inst = match inst {
                    Some(i) => i,
                    None => return,
                };
                match tw(|t| {
                    if t.nodes[inst].t == NType::Attribute {
                        t.nodes[inst].children.map(|c| t.nodes[c].content.clone())
                    } else {
                        None
                    }
                }) {
                    Some(s) => s,
                    None => return,
                }
            }
        };
        for cur in sets
            .split([' ', '\t', '\n', '\r'])
            .filter(|x| !x.is_empty())
        {
            if !valid_qname(cur) {
                self.error(
                    inst,
                    &format!(
                        "The name '{}' in use-attribute-sets is not a valid QName.\n",
                        cur
                    ),
                );
                return;
            }
            let (prefix, ncname) = split_qname(cur);
            let mut ns_uri = None;
            if let Some(p) = &prefix {
                let i = match inst {
                    Some(i) => i,
                    None => return,
                };
                match tw(|t| t.search_ns(i, Some(p)).map(|n| t.nss[n].href.clone())) {
                    None => {
                        self.error(
                            inst,
                            &format!(
                                "use-attribute-set : No namespace found for QName '{}:{}'\n",
                                p, ncname
                            ),
                        );
                        return;
                    }
                    Some(h) => ns_uri = Some(h),
                }
            }
            let set = self.sheet.attr_sets.get(&(ncname, ns_uri)).cloned();
            if let Some(set) = set {
                for a in set.attrs {
                    if let Some(pc) = self.sheet.precomp.get(&a).cloned() {
                        self.xsl_attribute(node, a, &pc);
                    }
                }
            }
        }
    }

    /// xsltAttrListTemplateProcess
    fn attr_list_template_process(&mut self, target: NodeId, attrs: &[NodeId]) {
        let old_insert = self.insert;
        self.insert = Some(target);
        for &attr in attrs {
            let is_use = tw(|t| {
                let a = &t.nodes[attr];
                a.ns.is_some()
                    && a.name == "use-attribute-sets"
                    && t.ns_href(a.ns) == Some(XSLT_NAMESPACE)
            });
            if is_use {
                let cn = self.node;
                self.apply_attribute_set(cn, Some(attr), None);
            }
        }
        let has_attr = tw(|t| t.nodes[target].properties.is_some());
        let mut orig_ns: Option<Option<NsId>> = None;
        let mut copy_ns: Option<NsId> = None;
        for &attr in attrs {
            let (ans, is_xslt, name, value, ok) = tw(|t| {
                let a = &t.nodes[attr];
                let ok = match a.children {
                    None => true,
                    Some(c) => t.nodes[c].t == NType::Text && t.nodes[c].next.is_none(),
                };
                (
                    a.ns,
                    a.ns.is_some() && t.ns_href(a.ns) == Some(XSLT_NAMESPACE),
                    a.name.clone(),
                    a.children
                        .map(|c| t.nodes[c].content.clone())
                        .unwrap_or_default(),
                    ok,
                )
            });
            if is_xslt {
                continue;
            }
            let parent = tw(|t| t.nodes[attr].parent);
            if !ok {
                self.error(
                    parent,
                    "Internal error: The children of an attribute node of a literal result element are not in the expected form.\n",
                );
                self.insert = old_insert;
                return;
            }
            if orig_ns != Some(ans) {
                orig_ns = Some(ans);
                match ans {
                    Some(_) => {
                        copy_ns = self.get_namespace(parent.unwrap_or(target), ans, target);
                        if copy_ns.is_none() {
                            self.insert = old_insert;
                            return;
                        }
                    }
                    None => copy_ns = None,
                }
            }
            let avt = self.sheet.avts.get(&attr).cloned();
            let v = match avt {
                Some(avt) => match self.eval_avt(&avt) {
                    Some(v) => Some(v),
                    None => {
                        let m = match ans {
                            Some(a) => {
                                let h = tw(|t| t.nss[a].href.clone());
                                format!(
                                    "Internal error: Failed to evaluate the AVT of attribute '{{{}}}{}'.\n",
                                    h, name
                                )
                            }
                            None => format!(
                                "Internal error: Failed to evaluate the AVT of attribute '{}'.\n",
                                name
                            ),
                        };
                        self.error(parent, &m);
                        None
                    }
                },
                None => Some(value),
            };
            let out = self.output;
            let failed = v.is_none();
            let val = v.unwrap_or_default();
            tw(|t| {
                let copy = if has_attr {
                    t.set_ns_prop(target, copy_ns, &name, "")
                } else {
                    let a = t.new_attr_node(Some(out), copy_ns, &name, None);
                    t.link_child_raw(target, a);
                    a
                };
                let kids = t.children_of(copy);
                for k in kids {
                    t.unlink(k);
                }
                let tx = t.new_text(Some(out), &val);
                t.link_child_raw(copy, tx);
                if t.is_id(target, copy) {
                    t.add_id_safe(copy, &val);
                }
            });
            if failed {
                self.insert = old_insert;
                return;
            }
        }
        self.insert = old_insert;
    }

    // ------------------------------------------------------------------
    // Pattern matching (pattern.c)

    /// xsltGetTemplate
    pub fn get_template(&mut self, node: NodeId, style: Option<usize>) -> Option<usize> {
        let order = self.sheet.import_order();
        let start_idx = match style {
            None => 0,
            Some(s) => {
                let next = self.sheet.next_import(s)?;
                order.iter().position(|&x| x == next)?
            }
        };
        let (ntype, nname) = tw(|t| (t.nodes[node].t, t.nodes[node].name.clone()));
        for &cs in &order[start_idx..] {
            if Some(cs) == style {
                break;
            }
            let mut ret: Option<usize> = None;
            let mut priority = XSLT_PAT_NO_PRIORITY;
            let name: Option<String> = match ntype {
                NType::Element => {
                    if nname.starts_with(' ') {
                        None
                    } else {
                        Some(nname.clone())
                    }
                }
                NType::Attribute | NType::Pi => Some(nname.clone()),
                _ => None,
            };
            if let Some(n) = name {
                let list = self.sheet.styles[cs]
                    .templates_hash
                    .get(&(n, self.mode.clone(), self.mode_uri.clone()))
                    .cloned();
                if let Some(list) = list {
                    for pi in list {
                        if self.test_comp_match(pi, node, true) == 1 {
                            ret = self.sheet.pats[pi].template;
                            priority = self.sheet.pats[pi].priority;
                            break;
                        }
                    }
                }
            }
            let generic: Vec<usize> = {
                let st = &self.sheet.styles[cs];
                match ntype {
                    NType::Element => {
                        if nname.starts_with(' ') {
                            st.root_match.clone()
                        } else {
                            st.elem_match.clone()
                        }
                    }
                    NType::Attribute => st.attr_match.clone(),
                    NType::Pi => st.pi_match.clone(),
                    NType::Document | NType::HtmlDocument => st.root_match.clone(),
                    NType::Text | NType::CData => st.text_match.clone(),
                    NType::Comment => st.comment_match.clone(),
                    _ => Vec::new(),
                }
            };
            self.scan_list(&generic, node, &mut ret, &mut priority);
            if matches!(ntype, NType::Document | NType::HtmlDocument | NType::Text)
                || matches!(ntype, NType::Pi | NType::Comment)
            {
                let em = self.sheet.styles[cs].elem_match.clone();
                self.scan_list(&em, node, &mut ret, &mut priority);
            }
            // keyed_match
            loop {
                if self.key_nodes.contains(&node) {
                    let km = self.sheet.styles[cs].key_match.clone();
                    self.scan_list(&km, node, &mut ret, &mut priority);
                    break;
                } else if self.has_templ_key_patterns && {
                    let d = self.document_for_node(node);
                    d.map(|di| self.docs[di].nb_keys_computed < self.nb_keys)
                        .unwrap_or(true)
                } {
                    if self.compute_all_keys(node).is_err() {
                        return None;
                    }
                    if self.key_nodes.contains(&node) {
                        continue;
                    }
                    break;
                } else {
                    break;
                }
            }
            if ret.is_some() {
                return ret;
            }
        }
        None
    }

    fn scan_list(
        &mut self,
        list: &[usize],
        node: NodeId,
        ret: &mut Option<usize>,
        priority: &mut f32,
    ) {
        for &pi in list {
            let (pprio, ptempl) = (self.sheet.pats[pi].priority, self.sheet.pats[pi].template);
            let better = match *ret {
                None => true,
                Some(r) => {
                    pprio > *priority
                        || (pprio == *priority
                            && ptempl
                                .map(|pt| {
                                    self.sheet.templates[pt].position
                                        > self.sheet.templates[r].position
                                })
                                .unwrap_or(false))
                }
            };
            if !better {
                break;
            }
            if self.test_comp_match(pi, node, true) == 1 {
                *ret = ptempl;
                *priority = pprio;
                break;
            }
        }
    }

    /// xsltTestCompMatchList (count/from patterns of xsl:number)
    pub fn test_comp_match_list(&mut self, node: NodeId, list: &[usize]) -> bool {
        for &pi in list {
            if self.test_comp_match(pi, node, false) == 1 {
                return true;
            }
        }
        false
    }

    /// xsltTestStepMatch
    fn test_step_match(&mut self, node: NodeId, step: &crate::pattern::PStep) -> i32 {
        let r = tw(|t| {
            let nd = &t.nodes[node];
            match step.op {
                POp::Root => Some(
                    matches!(nd.t, NType::Document | NType::HtmlDocument)
                        || (nd.t == NType::Element && nd.name.starts_with(' ')),
                ),
                POp::Elem => {
                    if nd.t != NType::Element {
                        return Some(false);
                    }
                    if let Some(v) = &step.value {
                        if *v != nd.name {
                            return Some(false);
                        }
                    } else {
                        return Some(true);
                    }
                    match nd.ns {
                        None => Some(step.value2.is_none()),
                        Some(_) => Some(step.value2.as_deref() == t.ns_href(nd.ns)),
                    }
                }
                POp::Attr => {
                    if nd.t != NType::Attribute {
                        return Some(false);
                    }
                    if let Some(v) = &step.value {
                        if *v != nd.name {
                            return Some(false);
                        }
                    }
                    match nd.ns {
                        None => Some(step.value2.is_none()),
                        Some(_) => match &step.value2 {
                            Some(v2) => Some(Some(v2.as_str()) == t.ns_href(nd.ns)),
                            None => Some(true),
                        },
                    }
                }
                POp::Id => {
                    if nd.t != NType::Element {
                        return Some(false);
                    }
                    let id = step.value.clone().unwrap_or_default();
                    let a = nd
                        .doc
                        .and_then(|d| t.doc_info(d))
                        .and_then(|i| i.ids.get(&id).copied());
                    Some(matches!(a, Some(a) if t.nodes[a].parent == Some(node)))
                }
                POp::Key => None,
                POp::Ns => {
                    if nd.t != NType::Element {
                        return Some(false);
                    }
                    match nd.ns {
                        None => Some(step.value.is_none()),
                        Some(_) => Some(step.value.as_deref() == t.ns_href(nd.ns)),
                    }
                }
                POp::All => Some(nd.t == NType::Element),
                POp::Pi => {
                    if nd.t != NType::Pi {
                        return Some(false);
                    }
                    Some(step.value.as_ref().map(|v| *v == nd.name).unwrap_or(true))
                }
                POp::Comment => Some(nd.t == NType::Comment),
                POp::Text => Some(matches!(nd.t, NType::Text | NType::CData)),
                POp::Node => Some(matches!(
                    nd.t,
                    NType::Element | NType::CData | NType::Pi | NType::Comment | NType::Text
                )),
                _ => Some(false),
            }
        });
        match r {
            Some(b) => b as i32,
            None => {
                // XSLT_OP_KEY
                let list = self.get_key(
                    step.value.as_deref().unwrap_or(""),
                    step.value3.as_deref(),
                    step.value2.as_deref().unwrap_or(""),
                );
                match list {
                    Some(l) if l.contains(&node) => 1,
                    _ => 0,
                }
            }
        }
    }

    /// xsltTestPredicateMatch
    fn test_predicate_match(
        &mut self,
        comp: &CompMatch,
        node: NodeId,
        step: &crate::pattern::PStep,
        sel: Option<&crate::pattern::PStep>,
    ) -> bool {
        let c = match &step.comp {
            Some(c) => c.clone(),
            None => return false,
        };
        let sel = match sel {
            Some(s) => s.clone(),
            None => return false,
        };
        let (old_cs, old_cp) = (self.xp.size, self.xp.pos);
        // recompute the position among matching siblings
        let parent = tw(|t| t.nodes[node].parent);
        let mut pos = 0;
        let mut len = 0;
        if let Some(p) = parent {
            let sibs = tw(|t| t.children_of(p));
            for s in sibs {
                if s == node {
                    len += 1;
                    pos = len;
                } else if self.test_step_match(s, &sel) == 1 {
                    len += 1;
                }
            }
        }
        if pos != 0 {
            self.xp.size = len;
            self.xp.pos = pos;
        }
        let old_node = self.node;
        self.node = node;
        let m = self.eval_xpath_predicate(&c, &comp.ns_list);
        if pos != 0 {
            self.xp.size = old_cs;
            self.xp.pos = old_cp;
        }
        self.node = old_node;
        m
    }

    /// xsltEvalXPathPredicate
    fn eval_xpath_predicate(&mut self, comp: &CompExpr, ns: &Rc<Vec<(String, String)>>) -> bool {
        let old_node = self.xp.node;
        let (old_cs, old_cp) = (self.xp.size, self.xp.pos);
        let old_ns = self.xp.namespaces.clone();
        let old_inst = self.inst;
        self.xp.node = Some(self.node);
        self.xp.namespaces = ns.clone();
        let res = xpath::eval(self, comp);
        let ret = match res {
            Some(r) => match r {
                XObj::Bool(b) => b,
                XObj::Num(n) => n == self.xp.pos as f64,
                XObj::NodeSet(v) | XObj::Tree(v) => !v.is_empty(),
                XObj::Str(s) => !s.is_empty(),
                XObj::External(_) => false,
            },
            None => {
                self.state = State::Stopped;
                false
            }
        };
        self.xp.node = old_node;
        self.xp.namespaces = old_ns;
        self.inst = old_inst;
        self.xp.size = old_cs;
        self.xp.pos = old_cp;
        ret
    }

    /// xsltTestCompMatchDirect
    fn test_comp_match_direct(&mut self, comp: &CompMatch, node: NodeId) -> i32 {
        let doc = tw(|t| t.nodes[node].doc.unwrap_or(node));
        let expr = match xpath::compile(&comp.pattern) {
            Ok(e) => e,
            Err(msgs) => {
                for m in msgs {
                    php_error(self.vm, &format!("{}\n", m));
                }
                return -1;
            }
        };
        let saved = self.xp.clone();
        self.xp.node = Some(node);
        self.xp.doc = Some(doc);
        self.xp.namespaces = comp.ns_list.clone();
        let r = xpath::eval(self, &expr);
        let (d, s, p) = (self.xp.depth, saved.size, saved.pos);
        self.xp = saved;
        self.xp.depth = d;
        self.xp.size = s;
        self.xp.pos = p;
        match r {
            Some(XObj::NodeSet(v)) => v.contains(&node) as i32,
            _ => -1,
        }
    }

    /// xsltTestCompMatch
    pub fn test_comp_match(&mut self, pi: usize, match_node: NodeId, check_mode: bool) -> i32 {
        let comp = self.sheet.pats[pi].clone();
        if check_mode {
            if comp.mode != self.mode || comp.mode_uri != self.mode_uri {
                return 0;
            }
        } else if comp.mode.is_some() || comp.mode_uri.is_some() {
            return 0;
        }
        let old_inst = self.inst;
        self.inst = Some(comp.node);
        let mut states: Vec<(usize, Option<NodeId>)> = Vec::new();
        let mut node: Option<NodeId> = Some(match_node);
        let mut i = 0usize;
        let mut sel: Option<usize> = None;
        let found: i32 = 'outer: loop {
            // restart:
            let mut rollback = false;
            while i < comp.steps.len() {
                let step = &comp.steps[i];
                if step.op != POp::Predicate {
                    sel = Some(i);
                }
                let n = match node {
                    Some(n) => n,
                    None => {
                        rollback = true;
                        break;
                    }
                };
                match step.op {
                    POp::End => break 'outer 1,
                    POp::Parent => {
                        let (nt, parent) = tw(|t| (t.nodes[n].t, t.nodes[n].parent));
                        if matches!(
                            nt,
                            NType::Document | NType::HtmlDocument | NType::NamespaceDecl
                        ) {
                            rollback = true;
                            break;
                        }
                        node = parent;
                        let p = match parent {
                            None => {
                                rollback = true;
                                break;
                            }
                            Some(p) => p,
                        };
                        if let Some(v) = &step.value {
                            let ok = tw(|t| {
                                let pn = &t.nodes[p];
                                if *v != pn.name {
                                    return false;
                                }
                                match pn.ns {
                                    None => step.value2.is_none(),
                                    Some(_) => step.value2.as_deref() == t.ns_href(pn.ns),
                                }
                            });
                            if !ok {
                                rollback = true;
                                break;
                            }
                        }
                        i += 1;
                        continue;
                    }
                    POp::Ancestor => {
                        if step.value.is_none() {
                            let nstep = &comp.steps[i + 1];
                            if nstep.op == POp::Root {
                                break 'outer 1;
                            }
                            if !matches!(
                                nstep.op,
                                POp::Elem | POp::All | POp::Ns | POp::Id | POp::Key
                            ) {
                                rollback = true;
                                break;
                            }
                        }
                        let (nt, parent) = tw(|t| (t.nodes[n].t, t.nodes[n].parent));
                        if matches!(
                            nt,
                            NType::Document | NType::HtmlDocument | NType::NamespaceDecl
                        ) {
                            rollback = true;
                            break;
                        }
                        node = parent;
                        let nstep_op = comp.steps[i + 1].op;
                        if nstep_op != POp::Elem && nstep_op != POp::All {
                            states.push((i, node));
                            i += 1;
                            continue;
                        }
                        i += 1;
                        sel = Some(i);
                        let st = &comp.steps[i];
                        let v = match &st.value {
                            None => {
                                states.push((i - 1, node));
                                i += 1;
                                continue;
                            }
                            Some(v) => v.clone(),
                        };
                        let mut x = node;
                        while let Some(c) = x {
                            let hit = tw(|t| {
                                let cn = &t.nodes[c];
                                if cn.t == NType::Element && cn.name == v {
                                    match cn.ns {
                                        None => st.value2.is_none(),
                                        Some(_) => {
                                            st.value2.is_some()
                                                && st.value2.as_deref() == t.ns_href(cn.ns)
                                        }
                                    }
                                } else {
                                    false
                                }
                            });
                            if hit {
                                break;
                            }
                            x = tw(|t| t.nodes[c].parent);
                        }
                        node = x;
                        if node.is_none() {
                            rollback = true;
                            break;
                        }
                        states.push((i - 1, node));
                        i += 1;
                        continue;
                    }
                    POp::Predicate => {
                        if comp.direct {
                            break 'outer self.test_comp_match_direct(&comp, match_node);
                        }
                        let selstep = sel.map(|s| comp.steps[s].clone());
                        if !self.test_predicate_match(&comp, n, step, selstep.as_ref()) {
                            rollback = true;
                            break;
                        }
                    }
                    _ => {
                        if self.test_step_match(n, step) != 1 {
                            rollback = true;
                            break;
                        }
                    }
                }
                i += 1;
            }
            if !rollback {
                break 'outer 1;
            }
            // rollback:
            match states.pop() {
                None => break 'outer 0,
                Some((si, sn)) => {
                    i = si;
                    node = sn;
                    continue 'outer;
                }
            }
        };
        self.inst = old_inst;
        found
    }
}

// ---------------------------------------------------------------------------
// Document loading

/// xsltDocDefaultLoader: load and parse a document through PHP streams.
pub fn load_xml_document(vm: &mut Vm, uri: &str, options: i64) -> Option<NodeId> {
    let path = goro_ext_xml::io::strip_file_scheme(uri);
    let Some(data) = goro_ext_xml::io::read_resource(vm, &path) else {
        // xmlLoadExternalEntity's error
        if !goro_ext_xml::io::last_failure_reported() {
            let f = goro_ext_xml::dom::active_fn(vm);
            // (the name as PHP's stream opener saw it: unescaped)
            let shown = match goro_ext_xml::uri::parse_uri(uri) {
                Some(u) if u.scheme.as_deref().is_none_or(|s| s.starts_with("file")) => {
                    goro_ext_xml::io::uri_unescape(uri)
                }
                _ => uri.to_string(),
            };
            let msg = format!(
                "I/O warning : failed to load \"{}\": No such file or directory",
                shown
            );
            goro_ext_xml::libxml::issue_error(vm, &f, &msg);
        }
        return None;
    };
    let out = goro_ext_xml::sax2::parse_doc_ex(&data, Some(uri), options, None);
    let f = goro_ext_xml::dom::active_fn(vm);
    goro_ext_xml::libxml::report(vm, &f, &out.errors, goro_ext_xml::libxml::Style::Ctx);
    if out.well_formed { out.doc } else { None }
}

pub fn tree_has(_t: &Tree) {}
