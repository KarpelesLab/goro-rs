//! Runs a compiled stylesheet (XSLT 1.0 sections 5-13). Diagnostics follow
//! what the ext/xsl tests expect (tests bug54446.phpt, bug71571_a.phpt,
//! bug71571_b.phpt, maxTemplateDepth_errors.phpt).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::compile::*;
use super::format;
use super::{Host, exslt, output};
use crate::tree::{Arena, DocKind, NodeData, NodeId, QName, Str, XML_NS, XMLNS_NS};
use crate::xpath::pattern::Pattern;
use crate::xpath::{
    self, CallContext, FunctionHandler, NsMap, OrderCache, Value, XPathError, XPathErrorKind,
};

/// Settings of one transformation.
pub struct Options {
    /// Global parameters by expanded name (`{uri}local` or `local`).
    pub params: Vec<(String, String)>,
    /// `XSLTProcessor::$maxTemplateDepth`.
    pub max_depth: usize,
    /// `XSLTProcessor::$maxTemplateVars`.
    pub max_vars: usize,
}

/// The transformation was abandoned.
pub(super) struct Stop;

type F<T> = Result<T, Stop>;

#[derive(Clone)]
struct Var {
    ns: Str,
    local: Str,
    value: Value,
    is_param: bool,
    /// Visible to expressions (a with-param is once its `xsl:param` ran).
    claimed: bool,
    /// The result tree fragment this variable owns.
    rtf: Option<NodeId>,
    /// Only keeps an RTF alive.
    hidden: bool,
}

enum GlobalState {
    Pending,
    Computing,
    Done(Value),
}

enum Sink {
    Tree {
        doc: NodeId,
        parent: NodeId,
        cdata: bool,
    },
    Capture(String),
}

pub(super) struct Tx<'a> {
    pub(super) ss: &'a Stylesheet,
    pub(super) host: &'a mut dyn Host,
    max_depth: usize,
    max_vars: usize,
    vars: Vec<Var>,
    frame_base: usize,
    frames: Vec<usize>,
    globals: Vec<GlobalState>,
    params: HashMap<String, String>,
    tstack: Vec<&'a Template>,
    out: Vec<Sink>,
    current: NodeId,
    cur_ns: Option<NsMap>,
    cur_module: u32,
    nsnodes: Vec<NodeId>,
    orders: OrderCache,
    stopped: bool,
    rtfs: Vec<NodeId>,
    late_free: u32,
    docs: HashMap<String, NodeId>,
    keys: HashMap<(Str, NodeId), Rc<HashMap<String, Vec<NodeId>>>>,
    building_keys: HashSet<(Str, NodeId)>,
    cdata_names: HashSet<&'a str>,
    func_result: Option<Value>,
    prefix_counter: u32,
    pub(super) rtf_roots: HashSet<NodeId>,
    func_depth: u32,
    source_root: NodeId,
}

/// The outcome of [`transform`].
pub struct Outcome {
    /// The result document, unless the transformation stopped.
    pub doc: Option<NodeId>,
}

/// Runs the stylesheet on the document `source`.
pub fn transform(
    arena: &mut Arena,
    ss: &Stylesheet,
    source: NodeId,
    opts: Options,
    host: &mut dyn Host,
) -> Outcome {
    let result = arena.new_document(DocKind::Xml);
    apply_output_props(arena, result, &ss.output);
    let mut tx = Tx {
        ss,
        host,
        max_depth: opts.max_depth,
        max_vars: opts.max_vars,
        vars: Vec::new(),
        frame_base: 0,
        frames: Vec::new(),
        globals: ss.globals.iter().map(|_| GlobalState::Pending).collect(),
        params: opts.params.into_iter().collect(),
        tstack: Vec::new(),
        out: vec![Sink::Tree {
            doc: result,
            parent: result,
            cdata: !ss.output.cdata_elements.is_empty(),
        }],
        current: source,
        cur_ns: None,
        cur_module: 0,
        nsnodes: Vec::new(),
        orders: OrderCache::default(),
        stopped: false,
        rtfs: Vec::new(),
        late_free: 0,
        docs: HashMap::new(),
        keys: HashMap::new(),
        building_keys: HashSet::new(),
        cdata_names: ss
            .output
            .cdata_elements
            .iter()
            .map(String::as_str)
            .collect(),
        func_result: None,
        prefix_counter: 0,
        rtf_roots: HashSet::new(),
        func_depth: 0,
        source_root: source,
    };
    // xsl:strip-space works on a copy: the caller's tree stays as it is.
    let mut stripped = None;
    let mut root = source;
    if ss.space_rules.iter().any(|r| r.strip) {
        let c = arena.clone_node(source, source, true);
        strip_spaces(arena, c, ss);
        stripped = Some(c);
        root = c;
        tx.source_root = c;
    }
    let c = CallContext {
        node: root,
        pos: 1,
        size: 1,
    };
    let r = tx.run(arena, c);
    tx.cleanup(arena);
    if let Some(s) = stripped {
        arena.free_document_if_unheld(s);
    }
    if r.is_err() || tx.stopped {
        arena.free_document_if_unheld(result);
        return Outcome { doc: None };
    }
    output::finish_result(arena, result, &ss.output);
    Outcome { doc: Some(result) }
}

pub(super) fn apply_output_props(arena: &mut Arena, doc: NodeId, o: &OutputSpec) {
    let d = arena.doc_mut(doc);
    d.version = Some(o.version.clone().unwrap_or_else(|| "1.0".into()));
    d.encoding = o.encoding.clone();
    d.standalone = match o.standalone {
        Some(true) => 1,
        Some(false) => 0,
        None => -1,
    };
}

fn strip_spaces(arena: &mut Arena, doc: NodeId, ss: &Stylesheet) {
    let all: Vec<NodeId> = arena.descendants(doc).collect();
    let mut dead = Vec::new();
    for n in all {
        let NodeData::Text(t) = arena.data(n) else {
            continue;
        };
        if !t.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r')) {
            continue;
        }
        let Some(p) = arena.parent(n) else { continue };
        let Some(q) = arena.qname(p) else { continue };
        // The best rule wins: precedence, then the more specific test, then
        // the later one (section 3.4).
        let mut best: Option<(u32, f64, bool)> = None;
        for r in &ss.space_rules {
            let pr = match &r.test {
                SpaceTest::Any => -0.5,
                SpaceTest::Ns(u) if q.ns_str() == &**u => -0.25,
                SpaceTest::Name(u, l) if q.ns_str() == &**u && q.local == *l => 0.0,
                _ => continue,
            };
            let better = match best {
                None => true,
                Some((bp, bpr, _)) => (r.prec, pr) >= (bp, bpr),
            };
            if better {
                best = Some((r.prec, pr, r.strip));
            }
        }
        if !best.is_some_and(|b| b.2) {
            continue;
        }
        // xml:space on an ancestor keeps it.
        let mut keep = false;
        let mut cur = Some(p);
        while let Some(c) = cur {
            if arena.is_element(c)
                && let Some(a) = arena.get_attr_node_ns(c, Some(XML_NS), "space")
            {
                keep = arena.attr_value(a) == "preserve";
                break;
            }
            cur = arena.parent(c);
        }
        if !keep {
            dead.push(n);
        }
    }
    for n in dead {
        arena.detach(n);
        arena.free_subtree(n);
    }
}

fn in_model(arena: &Arena, n: NodeId) -> bool {
    !matches!(
        arena.data(n),
        NodeData::Doctype(_)
            | NodeData::EntityDecl(_)
            | NodeData::Notation(_)
            | NodeData::ElementDecl(_)
            | NodeData::AttributeDecl(_)
    )
}

fn model_children(arena: &Arena, n: NodeId) -> Vec<NodeId> {
    arena.children(n).filter(|&c| in_model(arena, c)).collect()
}

impl<'a> Tx<'a> {
    // ----- diagnostics ---------------------------------------------------------

    fn runtime_error(&mut self, ar: &mut Arena, i: &Instr, msg: &str) {
        let file = self
            .ss
            .modules
            .get(i.module as usize)
            .and_then(|m| m.url.clone())
            .unwrap_or_else(|| "(null)".into());
        let loc = format!(
            "runtime error: file {file} line {} element {}",
            i.line, i.elem
        );
        self.host.report(ar, &loc);
        self.host.report(ar, msg);
    }

    /// The stack dump of a runaway recursion (test bug71571_b.phpt).
    fn dump_stacks(&mut self, ar: &mut Arena) {
        self.host.report(ar, "Templates:");
        let labels: Vec<String> = self
            .tstack
            .iter()
            .rev()
            .take(15)
            .enumerate()
            .map(|(i, t)| format!("#{i} name {} ", t.label))
            .collect();
        for l in labels {
            self.host.report(ar, &l);
        }
        self.host.report(ar, "Variables:");
        let mut lines = Vec::new();
        let mut n = 0;
        let ends: Vec<usize> = {
            let mut e: Vec<usize> = self.frames.iter().skip(1).copied().collect();
            e.push(self.vars.len());
            e
        };
        for (fi, &start) in self.frames.iter().enumerate().rev() {
            let vars = &self.vars[start..ends[fi]];
            if vars.iter().all(|v| v.hidden) {
                continue;
            }
            if n >= 15 {
                break;
            }
            lines.push(format!("#{n}"));
            n += 1;
            for v in vars.iter().rev().filter(|v| !v.hidden) {
                let name = clark(&v.ns, &v.local);
                if v.is_param {
                    lines.push(format!("param {name} "));
                } else {
                    lines.push(format!("{name} "));
                }
            }
        }
        for l in lines {
            self.host.report(ar, &l);
        }
    }

    fn stop(&mut self) -> Stop {
        self.stopped = true;
        Stop
    }

    fn check_stop(&mut self) -> F<()> {
        if self.host.stop_requested() {
            return Err(self.stop());
        }
        Ok(())
    }

    // ----- top level -----------------------------------------------------------

    fn run(&mut self, ar: &mut Arena, c: CallContext) -> F<()> {
        self.frames.push(0);
        for i in 0..self.ss.globals.len() {
            self.global_value(ar, i, c)?;
        }
        self.apply_one(ar, 0, c, &[], None)
    }

    fn cleanup(&mut self, ar: &mut Arena) {
        for v in std::mem::take(&mut self.vars) {
            if let Some(d) = v.rtf {
                ar.free_document_if_unheld(d);
            }
        }
        for d in std::mem::take(&mut self.rtfs) {
            ar.free_document_if_unheld(d);
        }
        for (_, d) in std::mem::take(&mut self.docs) {
            ar.free_document_if_unheld(d);
        }
        for n in std::mem::take(&mut self.nsnodes) {
            ar.free_namespace_node(n);
        }
    }

    fn global_value(&mut self, ar: &mut Arena, i: usize, c: CallContext) -> F<Value> {
        match &self.globals[i] {
            GlobalState::Done(v) => return Ok(v.clone()),
            GlobalState::Computing => {
                let name = self.ss.globals[i].name.local.to_string();
                self.host
                    .report(ar, &format!("Recursive definition of {name}"));
                return Ok(Value::String(String::new()));
            }
            GlobalState::Pending => {}
        }
        let ss: &'a Stylesheet = self.ss;
        let g = &ss.globals[i];
        // User parameters override global xsl:param only (section 11.4;
        // test req30622.phpt).
        if g.is_param
            && let Some(v) = self.params.get(&clark(&g.name.ns, &g.name.local))
        {
            let v = Value::String(v.clone());
            self.globals[i] = GlobalState::Done(v.clone());
            return Ok(v);
        }
        self.globals[i] = GlobalState::Computing;
        // Global variables see no local ones.
        let save_base = self.frame_base;
        self.frame_base = self.vars.len();
        let root = CallContext {
            node: self.source_root,
            pos: 1,
            size: 1,
        };
        let _ = c;
        let r = self.var_value(ar, &g.value, root);
        self.frame_base = save_base;
        let (v, rtf) = r?;
        if let Some(d) = rtf {
            self.rtfs.push(d);
        }
        self.globals[i] = GlobalState::Done(v.clone());
        Ok(v)
    }

    // ----- expressions ---------------------------------------------------------

    /// Evaluates; `Ok(None)` after an XPath error (reported).
    fn eval(
        &mut self,
        ar: &mut Arena,
        i: &Instr,
        e: &'a XExpr,
        c: CallContext,
    ) -> F<Option<Value>> {
        match self.eval_raw(ar, e, c) {
            Ok(v) => Ok(Some(v)),
            Err(err) => {
                self.check_stop()?;
                self.xpath_failed(ar, i, &err);
                self.check_stop()?;
                Ok(None)
            }
        }
    }

    fn eval_raw(
        &mut self,
        ar: &mut Arena,
        e: &'a XExpr,
        c: CallContext,
    ) -> Result<Value, XPathError> {
        let old_current = self.current;
        let old_ns = self.cur_ns.replace(e.b.namespaces().clone());
        let old_module = self.cur_module;
        self.current = c.node;
        self.cur_module = e.module;
        let mut nsn = std::mem::take(&mut self.nsnodes);
        let r = xpath::evaluate_bound(ar, &e.b, c, self, &mut nsn);
        nsn.append(&mut self.nsnodes);
        self.nsnodes = nsn;
        self.current = old_current;
        self.cur_ns = old_ns;
        self.cur_module = old_module;
        r
    }

    fn xpath_failed(&mut self, ar: &mut Arena, i: &Instr, e: &XPathError) {
        match e.kind {
            XPathErrorKind::UnknownFunction => {
                let name = e.message.split(": ").nth(1).unwrap_or("").to_string();
                self.host.report(
                    ar,
                    &format!("xmlXPathCompOpEval: function {name} not found"),
                );
                self.host.report(ar, "Unregistered function");
            }
            _ => {
                if !e.message.is_empty() {
                    let m = e.message.clone();
                    self.host.report(ar, &m);
                }
            }
        }
        self.runtime_error(ar, i, "XPath evaluation returned no result.");
    }

    fn eval_string(
        &mut self,
        ar: &mut Arena,
        i: &Instr,
        e: &'a XExpr,
        c: CallContext,
    ) -> F<Option<String>> {
        Ok(self.eval(ar, i, e, c)?.map(|v| v.to_string_value(ar)))
    }

    fn avt(&mut self, ar: &mut Arena, i: &Instr, t: &'a Avt, c: CallContext) -> F<String> {
        if let [AvtPart::Lit(s)] = t.0.as_slice() {
            return Ok(s.clone());
        }
        let mut out = String::new();
        for p in &t.0 {
            match p {
                AvtPart::Lit(s) => out.push_str(s),
                AvtPart::Expr(e) => {
                    if let Some(s) = self.eval_string(ar, i, e, c)? {
                        out.push_str(&s);
                    }
                }
            }
        }
        Ok(out)
    }

    fn opt_avt(
        &mut self,
        ar: &mut Arena,
        i: &Instr,
        t: &'a Option<Avt>,
        c: CallContext,
    ) -> F<Option<String>> {
        match t {
            Some(a) => Ok(Some(self.avt(ar, i, a, c)?)),
            None => Ok(None),
        }
    }

    // ----- output --------------------------------------------------------------

    fn parent(&self) -> Option<(NodeId, NodeId)> {
        match self.out.last() {
            Some(Sink::Tree { doc, parent, .. }) => Some((*doc, *parent)),
            _ => None,
        }
    }

    fn set_parent(&mut self, p: NodeId) -> NodeId {
        if let Some(Sink::Tree { parent, .. }) = self.out.last_mut() {
            std::mem::replace(parent, p)
        } else {
            p
        }
    }

    pub(super) fn text(&mut self, ar: &mut Arena, s: &str, doe: bool) {
        if s.is_empty() {
            return;
        }
        let (doc, parent, cdata) = match self.out.last_mut() {
            Some(Sink::Capture(b)) => {
                b.push_str(s);
                return;
            }
            Some(Sink::Tree { doc, parent, cdata }) => (*doc, *parent, *cdata),
            None => return,
        };
        // cdata-section-elements (section 16.1).
        if cdata
            && let Some(q) = ar.qname(parent)
            && self
                .cdata_names
                .contains(clark(q.ns_str(), &q.local).as_str())
        {
            if let Some(l) = ar.last_child(parent)
                && let NodeData::CData(t) = &mut ar.node_mut(l).data
            {
                t.push_str(s);
                return;
            }
            let n = ar.create(doc, NodeData::CData(s.to_string()));
            ar.raw_append(parent, n);
            return;
        }
        if let Some(l) = ar.last_child(parent)
            && ar.is_no_escape(l) == doe
            && let NodeData::Text(t) = &mut ar.node_mut(l).data
        {
            t.push_str(s);
            return;
        }
        let n = ar.new_text(doc, s);
        if doe {
            ar.mark_no_escape(n);
        }
        ar.raw_append(parent, n);
    }

    /// Keeps a document made by an extension function until the end.
    pub(super) fn keep_fragment(&mut self, doc: NodeId) {
        self.rtfs.push(doc);
    }

    /// The URI bound to `prefix` for children of `parent` in the result.
    fn scope_ns(ar: &Arena, parent: NodeId, prefix: Option<&str>) -> Option<Str> {
        if !ar.is_element(parent) {
            return None;
        }
        ar.lookup_ns(parent, prefix)
    }

    fn new_element(&mut self, ar: &mut Arena, doc: NodeId, parent: NodeId, name: QName) -> NodeId {
        let el = ar.new_element(doc, name.clone());
        ar.raw_append(parent, el);
        let prefix = name.prefix.as_deref();
        if prefix != Some("xml") {
            let cur = Self::scope_ns(ar, parent, prefix);
            match (&name.ns, cur) {
                (Some(u), Some(c)) if **u == *c => {}
                (Some(u), _) => {
                    ar.declare_ns(el, prefix, u);
                }
                // No namespace under a default one (section 7.1.2).
                (None, Some(_)) if prefix.is_none() => {
                    ar.declare_ns(el, None, "");
                }
                _ => {}
            }
        }
        el
    }

    /// Declares `uri` on element `el` for an attribute, picking a prefix.
    fn attr_prefix(&mut self, ar: &mut Arena, el: NodeId, prefix: Option<&str>, uri: &str) -> Str {
        if uri == XML_NS {
            return "xml".into();
        }
        if let Some(p) = prefix
            && p != "xmlns"
        {
            match ar.lookup_ns(el, Some(p)) {
                Some(c) if &*c == uri => return p.into(),
                Some(_)
                    if ar.element(el).is_some_and(|e| {
                        e.ns_decls.iter().any(|d| d.prefix.as_deref() == Some(p))
                    }) => {}
                _ => {
                    ar.declare_ns(el, Some(p), uri);
                    return p.into();
                }
            }
        }
        // A prefix already bound to the URI, or a new one.
        for d in ar.in_scope_namespaces(el) {
            if &*d.uri == uri
                && let Some(p) = &d.prefix
            {
                return p.clone();
            }
        }
        loop {
            self.prefix_counter += 1;
            let p = format!("ns{}", self.prefix_counter);
            if ar.lookup_ns(el, Some(&p)).is_none() {
                ar.declare_ns(el, Some(&p), uri);
                return p.as_str().into();
            }
        }
    }

    /// Adds an attribute to the current element (section 7.1.3: a later one
    /// of the same name replaces).
    fn add_attr(&mut self, ar: &mut Arena, i: &Instr, mut name: QName, value: &str) {
        let Some((doc, el)) = self.parent() else {
            return;
        };
        if !ar.is_element(el) {
            self.runtime_error(
                ar,
                i,
                "xsl:attribute: Cannot add attributes to an element if children have been already added to the element.",
            );
            return;
        }
        if ar.first_child(el).is_some() {
            self.runtime_error(
                ar,
                i,
                "xsl:attribute: Cannot add attributes to an element if children have been already added to the element.",
            );
            return;
        }
        if let Some(u) = name.ns.clone() {
            let p = self.attr_prefix(ar, el, name.prefix.as_deref(), &u);
            name.prefix = Some(p);
        }
        if let Some(a) = ar.get_attr_node_ns(el, name.ns.as_deref(), &name.local) {
            ar.set_attr_value(a, value);
            return;
        }
        let a = ar.new_attribute(doc, name, value);
        ar.raw_append_attr(el, a);
    }

    fn capture(&mut self, ar: &mut Arena, body: &'a [Instr], c: CallContext) -> F<String> {
        self.out.push(Sink::Capture(String::new()));
        let r = self.exec_body(ar, body, c);
        let s = match self.out.pop() {
            Some(Sink::Capture(s)) => s,
            _ => String::new(),
        };
        r?;
        Ok(s)
    }

    // ----- variables -----------------------------------------------------------

    fn var_value(
        &mut self,
        ar: &mut Arena,
        src: &'a ValueSrc,
        c: CallContext,
    ) -> F<(Value, Option<NodeId>)> {
        match src {
            ValueSrc::Empty => Ok((Value::String(String::new()), None)),
            ValueSrc::Select(e) => match self.eval_raw(ar, e, c) {
                Ok(v) => Ok((v, None)),
                Err(err) => {
                    self.check_stop()?;
                    let m = err.message.clone();
                    if !m.is_empty() {
                        self.host.report(ar, &m);
                    }
                    self.check_stop()?;
                    Ok((Value::String(String::new()), None))
                }
            },
            ValueSrc::Body(body) => {
                let doc = ar.new_document(DocKind::Xml);
                self.rtf_roots.insert(doc);
                self.out.push(Sink::Tree {
                    doc,
                    parent: doc,
                    cdata: false,
                });
                let r = self.exec_body(ar, body, c);
                self.out.pop();
                if let Err(e) = r {
                    self.rtfs.push(doc);
                    return Err(e);
                }
                Ok((Value::NodeSet(vec![doc]), Some(doc)))
            }
        }
    }

    fn pop_vars(&mut self, ar: &mut Arena, base: usize) {
        while self.vars.len() > base {
            let v = self.vars.pop().unwrap();
            if let Some(d) = v.rtf {
                self.free_rtf(ar, d);
            }
        }
    }

    fn free_rtf(&mut self, ar: &mut Arena, d: NodeId) {
        // A function's result can refer to its local fragments.
        if self.late_free > 0 {
            self.rtfs.push(d);
            return;
        }
        self.rtf_roots.remove(&d);
        self.orders.forget(d);
        if !self.keys.is_empty() {
            self.keys.retain(|(_, r), _| *r != d);
        }
        // Namespace nodes of it must go first.
        if !self.nsnodes.is_empty() {
            let list = std::mem::take(&mut self.nsnodes);
            for n in list {
                if ar.is_live(n) {
                    let owner = ar.parent(n);
                    if owner.is_some_and(|o| ar.is_live(o) && ar.doc_of(o) == d) {
                        ar.free_namespace_node(n);
                        continue;
                    }
                    self.nsnodes.push(n);
                }
            }
        }
        ar.free_document_if_unheld(d);
    }

    /// With-params evaluated in the caller's scope. Fragments stay owned by
    /// hidden variables of the caller, which outlive the call(s).
    fn with_params(
        &mut self,
        ar: &mut Arena,
        params: &'a [WithParam],
        c: CallContext,
    ) -> F<Vec<Var>> {
        let mut out = Vec::with_capacity(params.len());
        for p in params {
            let (value, rtf) = self.var_value(ar, &p.value, c)?;
            if let Some(d) = rtf {
                self.vars.push(Var {
                    ns: "".into(),
                    local: "".into(),
                    value: Value::Boolean(false),
                    is_param: false,
                    claimed: false,
                    rtf: Some(d),
                    hidden: true,
                });
            }
            out.push(Var {
                ns: p.name.ns.clone(),
                local: p.name.local.clone(),
                value,
                is_param: false,
                claimed: false,
                rtf: None,
                hidden: false,
            });
        }
        Ok(out)
    }

    // ----- templates -----------------------------------------------------------

    fn run_template(
        &mut self,
        ar: &mut Arena,
        t: &'a Template,
        c: CallContext,
        params: &[Var],
    ) -> F<()> {
        if t.body.is_empty() {
            return Ok(());
        }
        self.tstack.push(t);
        let save_base = self.frame_base;
        let base = self.vars.len();
        self.frame_base = base;
        self.frames.push(base);
        self.vars.extend_from_slice(params);
        if self.tstack.len() > self.max_depth {
            let msg = format!(
                "xsltApplySequenceConstructor: A potential infinite template recursion was detected.\nYou can adjust $maxTemplateDepth in order to raise the maximum number of nested template calls and variables/params (currently set to {}).",
                self.max_depth
            );
            self.runtime_error(ar, &t.body[0], &msg);
            self.dump_stacks(ar);
            return Err(self.stop());
        }
        let r = self.exec_body(ar, &t.body, c);
        if r.is_ok() {
            self.pop_vars(ar, base);
            self.frames.pop();
            self.frame_base = save_base;
            self.tstack.pop();
        }
        r
    }

    fn find_rule(
        &mut self,
        ar: &mut Arena,
        mode: u32,
        n: NodeId,
        range: Option<(u32, u32)>,
    ) -> Option<&'a Template> {
        let ss: &'a Stylesheet = self.ss;
        let table = &ss.modes[mode as usize];
        let empty: &[Entry] = &[];
        let lists: [&'a [Entry]; 3] = match ar.data(n) {
            NodeData::Element(e) => [
                table.elements.get(&e.name.local).map_or(empty, |v| &v[..]),
                &table.any_element,
                &table.any,
            ],
            NodeData::Attribute(a) => [
                table
                    .attributes
                    .get(&a.name.local)
                    .map_or(empty, |v| &v[..]),
                &table.any_attribute,
                &table.any,
            ],
            NodeData::Text(_) | NodeData::CData(_) | NodeData::EntityRef(_) => {
                [&table.text, &table.any, empty]
            }
            NodeData::Comment(_) => [&table.comment, &table.any, empty],
            NodeData::Pi { .. } => [&table.pi, &table.any, empty],
            NodeData::Document(_) | NodeData::Fragment => [&table.root, &table.any, empty],
            _ => [&table.any, empty, empty],
        };
        let mut idx = [0usize; 3];
        loop {
            // The best remaining entry of the three lists.
            let mut best: Option<(usize, &'a Entry)> = None;
            for (li, list) in lists.iter().enumerate() {
                if let Some(e) = list.get(idx[li]) {
                    let better = match best {
                        None => true,
                        Some((_, b)) => (e.prec, e.priority, e.pos) > (b.prec, b.priority, b.pos),
                    };
                    if better {
                        best = Some((li, e));
                    }
                }
            }
            let (li, e) = best?;
            idx[li] += 1;
            if let Some((min, max)) = range
                && (e.prec < min || e.prec >= max)
            {
                continue;
            }
            if self.pattern_matches(ar, &e.pat, n) {
                return Some(&ss.templates[e.tpl as usize]);
            }
        }
    }

    pub(super) fn pattern_matches(&mut self, ar: &mut Arena, p: &Pattern, n: NodeId) -> bool {
        let old = self.current;
        self.current = n;
        let mut nsn = std::mem::take(&mut self.nsnodes);
        let r = p.matches(ar, n, self, &mut nsn);
        nsn.append(&mut self.nsnodes);
        self.nsnodes = nsn;
        self.current = old;
        r.unwrap_or(false)
    }

    fn apply_one(
        &mut self,
        ar: &mut Arena,
        mode: u32,
        c: CallContext,
        params: &[Var],
        range: Option<(u32, u32)>,
    ) -> F<()> {
        if let Some(t) = self.find_rule(ar, mode, c.node, range) {
            return self.run_template(ar, t, c, params);
        }
        self.check_stop()?;
        // The built-in rules (section 5.8).
        match ar.data(c.node) {
            NodeData::Document(_) | NodeData::Fragment | NodeData::Element(_) => {
                let kids = model_children(ar, c.node);
                self.apply_list(ar, mode, &kids, &[])
            }
            NodeData::Text(_) | NodeData::CData(_) | NodeData::Attribute(_) => {
                let s = xpath::string_value(ar, c.node);
                self.text(ar, &s, false);
                Ok(())
            }
            NodeData::EntityRef(_) => {
                let s = ar.text_content(c.node);
                self.text(ar, &s, false);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn apply_list(&mut self, ar: &mut Arena, mode: u32, nodes: &[NodeId], params: &[Var]) -> F<()> {
        let size = nodes.len();
        for (i, &n) in nodes.iter().enumerate() {
            let c = CallContext {
                node: n,
                pos: i + 1,
                size,
            };
            self.apply_one(ar, mode, c, params, None)?;
        }
        Ok(())
    }

    fn sort(
        &mut self,
        ar: &mut Arena,
        i: &Instr,
        nodes: Vec<NodeId>,
        sorts: &'a [SortKey],
        c: CallContext,
    ) -> F<Vec<NodeId>> {
        if sorts.is_empty() || nodes.len() < 2 {
            return Ok(nodes);
        }
        #[derive(Clone)]
        enum K {
            S(String),
            N(f64),
        }
        struct Spec {
            desc: bool,
            upper_first: Option<bool>,
        }
        let mut specs = Vec::with_capacity(sorts.len());
        let mut keys: Vec<Vec<K>> = vec![Vec::with_capacity(sorts.len()); nodes.len()];
        let size = nodes.len();
        for s in sorts {
            let order = self.opt_avt(ar, i, &s.order, c)?;
            let dtype = self.opt_avt(ar, i, &s.data_type, c)?;
            let case = self.opt_avt(ar, i, &s.case_order, c)?;
            specs.push(Spec {
                desc: order.as_deref() == Some("descending"),
                upper_first: case.as_deref().map(|x| x == "upper-first"),
            });
            let numeric = dtype.as_deref() == Some("number");
            for (j, &n) in nodes.iter().enumerate() {
                let cc = CallContext {
                    node: n,
                    pos: j + 1,
                    size,
                };
                let v = self.eval(ar, i, &s.select, cc)?;
                keys[j].push(if numeric {
                    K::N(v.map_or(f64::NAN, |v| v.to_number(ar)))
                } else {
                    K::S(v.map_or(String::new(), |v| v.to_string_value(ar)))
                });
            }
        }
        let mut order: Vec<usize> = (0..nodes.len()).collect();
        order.sort_by(|&a, &b| {
            use std::cmp::Ordering::*;
            for (k, sp) in specs.iter().enumerate() {
                let o = match (&keys[a][k], &keys[b][k]) {
                    (K::N(x), K::N(y)) => match (x.is_nan(), y.is_nan()) {
                        (true, true) => Equal,
                        (true, false) => Less,
                        (false, true) => Greater,
                        _ => x.partial_cmp(y).unwrap_or(Equal),
                    },
                    (K::S(x), K::S(y)) => match sp.upper_first {
                        None => x.cmp(y),
                        Some(upper) => x.to_lowercase().cmp(&y.to_lowercase()).then_with(|| {
                            let o = x.cmp(y);
                            if upper { o } else { o.reverse() }
                        }),
                    },
                    _ => Equal,
                };
                let o = if sp.desc { o.reverse() } else { o };
                if o != Equal {
                    return o;
                }
            }
            Equal
        });
        Ok(order.into_iter().map(|j| nodes[j]).collect())
    }

    // ----- instructions --------------------------------------------------------

    pub(super) fn exec_body(&mut self, ar: &mut Arena, body: &'a [Instr], c: CallContext) -> F<()> {
        let base = self.vars.len();
        for i in body {
            self.exec(ar, i, c)?;
        }
        self.pop_vars(ar, base);
        Ok(())
    }

    fn exec(&mut self, ar: &mut Arena, i: &'a Instr, c: CallContext) -> F<()> {
        match &i.kind {
            Kind::Text { text, doe } => {
                self.text(ar, text, *doe);
                Ok(())
            }
            Kind::ValueOf { select, doe } => {
                if let Some(s) = self.eval_string(ar, i, select, c)? {
                    self.text(ar, &s, *doe);
                }
                Ok(())
            }
            Kind::Lre(l) => self.exec_lre(ar, i, l, c),
            Kind::ApplyTemplates {
                select,
                mode,
                sorts,
                params,
            } => self.exec_apply(ar, i, select.as_ref(), *mode, sorts, params, c),
            Kind::CallTemplate { name, params } => self.exec_call(ar, i, name, params, c),
            Kind::ApplyImports => self.exec_apply_imports(ar, i, c),
            Kind::ForEach {
                select,
                sorts,
                body,
            } => self.exec_for_each(ar, i, select, sorts, body, c),
            Kind::If { test, body } => {
                if self.eval(ar, i, test, c)?.is_some_and(|v| v.to_boolean()) {
                    self.exec_body(ar, body, c)?;
                }
                Ok(())
            }
            Kind::Choose { whens, otherwise } => {
                for (t, b) in whens {
                    if self.eval(ar, i, t, c)?.is_some_and(|v| v.to_boolean()) {
                        return self.exec_body(ar, b, c);
                    }
                }
                self.exec_body(ar, otherwise, c)
            }
            Kind::Var {
                name,
                value,
                is_param,
            } => self.exec_var(ar, i, name, value, *is_param, c),
            _ => self.exec_rare(ar, i, c),
        }
    }

    #[inline(never)]
    fn exec_rare(&mut self, ar: &mut Arena, i: &'a Instr, c: CallContext) -> F<()> {
        match &i.kind {
            Kind::Attribute {
                name,
                ns,
                body,
                scope,
            } => self.exec_attribute(ar, i, name, ns, body, scope, c),
            Kind::Element {
                name,
                ns,
                sets,
                body,
                scope,
            } => self.exec_element(ar, i, name, ns, sets, body, scope, c),
            Kind::Comment { body } => {
                let s = self.capture(ar, body, c)?;
                let mut t = s.replace("--", "- -");
                if t.ends_with('-') {
                    t.push(' ');
                }
                if let Some((doc, parent)) = self.parent() {
                    let n = ar.create(doc, NodeData::Comment(t));
                    ar.raw_append(parent, n);
                }
                Ok(())
            }
            Kind::Pi { name, body } => {
                let target = self.avt(ar, i, name, c)?;
                let s = self.capture(ar, body, c)?;
                if !crate::chars::is_ncname(&target) || target.eq_ignore_ascii_case("xml") {
                    self.runtime_error(
                        ar,
                        i,
                        &format!("xsl:processing-instruction : invalid name '{target}'"),
                    );
                    return Ok(());
                }
                let data = s.replace("?>", "? >");
                if let Some((doc, parent)) = self.parent() {
                    let n = ar.create(
                        doc,
                        NodeData::Pi {
                            target: target.as_str().into(),
                            data,
                        },
                    );
                    ar.raw_append(parent, n);
                }
                Ok(())
            }
            Kind::Copy { sets, body } => self.exec_copy(ar, i, sets, body, c),
            Kind::CopyOf { select } => {
                match self.eval(ar, i, select, c)? {
                    Some(Value::NodeSet(v)) => {
                        for n in v {
                            self.copy_node(ar, i, n);
                        }
                    }
                    Some(v) => {
                        let s = v.to_string_value(ar);
                        self.text(ar, &s, false);
                    }
                    None => {}
                }
                Ok(())
            }
            Kind::Number(spec) => self.exec_number(ar, i, spec, c),
            Kind::Message { body, terminate } => {
                let s = self.capture(ar, body, c)?;
                let term = self.opt_avt(ar, i, terminate, c)?;
                self.host.report(ar, s.trim_end_matches('\n'));
                self.check_stop()?;
                if term.as_deref() == Some("yes") {
                    self.runtime_error(ar, i, "xsl:message : terminating");
                    return Err(self.stop());
                }
                Ok(())
            }
            Kind::Document(d) => self.exec_document(ar, i, d, c),
            Kind::FuncResult { value } => {
                let (v, rtf) = self.var_value(ar, value, c)?;
                if let Some(d) = rtf {
                    self.rtfs.push(d);
                }
                if self.func_depth == 0 {
                    self.runtime_error(ar, i, "func:result outside of func:function");
                    return Ok(());
                }
                self.func_result = Some(v);
                Ok(())
            }
            Kind::Unknown { name, fallback } => match fallback {
                Some(b) => self.exec_body(ar, b, c),
                None => {
                    self.runtime_error(
                        ar,
                        i,
                        &format!("xsltApplySequenceConstructor: failed to find extension {name}"),
                    );
                    Ok(())
                }
            },
            _ => Ok(()),
        }
    }

    fn exec_var(
        &mut self,
        ar: &mut Arena,
        i: &'a Instr,
        name: &'a VarName,
        value: &'a ValueSrc,
        is_param: bool,
        c: CallContext,
    ) -> F<()> {
        if self.vars.len() >= self.max_vars {
            let msg = format!(
                "xsltApplyXSLTTemplate: A potential infinite template recursion was detected.\nYou can adjust $maxTemplateVars in order to raise the maximum number of variables/params (currently set to {}).",
                self.max_vars
            );
            self.runtime_error(ar, i, &msg);
            self.dump_stacks(ar);
            return Err(self.stop());
        }
        if is_param {
            // A value passed by the caller.
            let base = self.frame_base;
            if let Some(v) = self.vars[base..]
                .iter_mut()
                .find(|v| !v.claimed && !v.hidden && v.local == name.local && v.ns == name.ns)
            {
                v.claimed = true;
                return Ok(());
            }
        }
        let (v, rtf) = self.var_value(ar, value, c)?;
        self.vars.push(Var {
            ns: name.ns.clone(),
            local: name.local.clone(),
            value: v,
            is_param,
            claimed: true,
            rtf,
            hidden: false,
        });
        Ok(())
    }

    fn exec_apply(
        &mut self,
        ar: &mut Arena,
        i: &'a Instr,
        select: Option<&'a XExpr>,
        mode: u32,
        sorts: &'a [SortKey],
        params: &'a [WithParam],
        c: CallContext,
    ) -> F<()> {
        let nodes = match select {
            Some(e) => match self.eval(ar, i, e, c)? {
                Some(Value::NodeSet(v)) => v,
                Some(_) => {
                    self.runtime_error(
                        ar,
                        i,
                        "xsl:apply-templates : select expression does not evaluate to a node-set",
                    );
                    return Ok(());
                }
                None => return Ok(()),
            },
            None => model_children(ar, c.node),
        };
        let base = self.vars.len();
        let vars = self.with_params(ar, params, c)?;
        let nodes = self.sort(ar, i, nodes, sorts, c)?;
        self.apply_list(ar, mode, &nodes, &vars)?;
        self.pop_vars(ar, base);
        Ok(())
    }

    fn exec_call(
        &mut self,
        ar: &mut Arena,
        i: &'a Instr,
        name: &'a Str,
        params: &'a [WithParam],
        c: CallContext,
    ) -> F<()> {
        let ss: &'a Stylesheet = self.ss;
        let Some(&t) = ss.named.get(name) else {
            self.runtime_error(
                ar,
                i,
                &format!("xsl:call-template : template {name} not found"),
            );
            return Ok(());
        };
        let base = self.vars.len();
        let vars = self.with_params(ar, params, c)?;
        self.run_template(ar, &ss.templates[t as usize], c, &vars)?;
        self.pop_vars(ar, base);
        Ok(())
    }

    fn exec_apply_imports(&mut self, ar: &mut Arena, i: &'a Instr, c: CallContext) -> F<()> {
        let Some(t) = self.tstack.last().copied() else {
            self.runtime_error(ar, i, "xsl:apply-imports : no current template rule");
            return Ok(());
        };
        self.apply_one(ar, t.mode, c, &[], Some((t.import_min, t.prec)))
    }

    fn exec_for_each(
        &mut self,
        ar: &mut Arena,
        i: &'a Instr,
        select: &'a XExpr,
        sorts: &'a [SortKey],
        body: &'a [Instr],
        c: CallContext,
    ) -> F<()> {
        let nodes = match self.eval(ar, i, select, c)? {
            Some(Value::NodeSet(v)) => v,
            Some(_) => {
                self.runtime_error(
                    ar,
                    i,
                    "xsl:for-each : select expression does not evaluate to a node-set",
                );
                return Ok(());
            }
            None => return Ok(()),
        };
        let nodes = self.sort(ar, i, nodes, sorts, c)?;
        let size = nodes.len();
        for (j, &n) in nodes.iter().enumerate() {
            self.exec_body(
                ar,
                body,
                CallContext {
                    node: n,
                    pos: j + 1,
                    size,
                },
            )?;
        }
        Ok(())
    }

    fn exec_lre(&mut self, ar: &mut Arena, i: &'a Instr, l: &'a Lre, c: CallContext) -> F<()> {
        let Some((doc, parent)) = self.parent() else {
            // An element inside xsl:attribute and the like is an error the
            // processor may recover from by ignoring it (section 7.1.3).
            return Ok(());
        };
        let el = self.new_element(ar, doc, parent, l.name.clone());
        for (p, u) in &l.decls {
            if p.as_deref() == l.name.prefix.as_deref() {
                continue;
            }
            let cur = Self::scope_ns(ar, parent, p.as_deref());
            if cur.as_deref() != Some(&**u) {
                ar.declare_ns(el, p.as_deref(), u);
            }
        }
        let old = self.set_parent(el);
        let r = self.lre_inner(ar, i, l, c);
        self.set_parent(old);
        r
    }

    fn lre_inner(&mut self, ar: &mut Arena, i: &'a Instr, l: &'a Lre, c: CallContext) -> F<()> {
        for s in &l.sets {
            self.use_set(ar, i, s, c, 0)?;
        }
        for (name, avt) in &l.attrs {
            let v = self.avt(ar, i, avt, c)?;
            self.add_attr(ar, i, name.clone(), &v);
        }
        self.exec_body(ar, &l.body, c)
    }

    fn use_set(
        &mut self,
        ar: &mut Arena,
        i: &'a Instr,
        name: &'a Str,
        c: CallContext,
        depth: u32,
    ) -> F<()> {
        let ss: &'a Stylesheet = self.ss;
        let Some(defs) = ss.attr_sets.get(name) else {
            self.runtime_error(
                ar,
                i,
                &format!("xsl:attribute-set : The attribute set '{name}' is not defined"),
            );
            return Ok(());
        };
        if depth > 100 {
            self.runtime_error(ar, i, "xsl:attribute-set : recursive use");
            return Ok(());
        }
        for d in defs {
            for u in &d.uses {
                self.use_set(ar, i, u, c, depth + 1)?;
            }
            self.exec_body(ar, &d.attrs, c)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn exec_attribute(
        &mut self,
        ar: &mut Arena,
        i: &'a Instr,
        name: &'a Avt,
        ns: &'a Option<Avt>,
        body: &'a [Instr],
        scope: &'a NsMap,
        c: CallContext,
    ) -> F<()> {
        let n = self.avt(ar, i, name, c)?;
        let nsv = self.opt_avt(ar, i, ns, c)?;
        let value = self.capture(ar, body, c)?;
        let Some(q) = self.dynamic_name(ar, i, &n, nsv, scope, false) else {
            return Ok(());
        };
        if q.qualified() == "xmlns" || q.prefix.as_deref() == Some("xmlns") {
            self.runtime_error(ar, i, "xsl:attribute: xmlns prefix forbidden");
            return Ok(());
        }
        self.add_attr(ar, i, q, &value);
        Ok(())
    }

    /// The expanded name of `xsl:attribute` / `xsl:element`.
    fn dynamic_name(
        &mut self,
        ar: &mut Arena,
        i: &Instr,
        name: &str,
        ns: Option<String>,
        scope: &NsMap,
        element: bool,
    ) -> Option<QName> {
        let (prefix, local) = match name.split_once(':') {
            Some((p, l)) => (Some(p), l),
            None => (None, name),
        };
        let bad =
            !crate::chars::is_ncname(local) || prefix.is_some_and(|p| !crate::chars::is_ncname(p));
        if bad {
            let what = if element { "element" } else { "attribute" };
            self.runtime_error(
                ar,
                i,
                &format!("xsl:{what}: The effective name '{name}' is not a valid QName."),
            );
            return None;
        }
        let uri = match ns {
            Some(u) => {
                if u.is_empty() {
                    None
                } else {
                    Some(u)
                }
            }
            None => match prefix {
                Some("xml") => Some(XML_NS.to_string()),
                Some(p) => match scope.get(p) {
                    Some(u) => Some(u.clone()),
                    None => {
                        self.runtime_error(
                            ar,
                            i,
                            &format!("xsl:attribute: The QName '{name}' has no namespace binding in scope in the stylesheet; this is an error, since the namespace was not specified by the instruction itself."),
                        );
                        return None;
                    }
                },
                None if element => scope.get("").cloned(),
                None => None,
            },
        };
        Some(QName {
            prefix: if uri.is_some() {
                prefix.map(Into::into)
            } else {
                None
            },
            local: local.into(),
            ns: uri.map(|u| u.as_str().into()),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn exec_element(
        &mut self,
        ar: &mut Arena,
        i: &'a Instr,
        name: &'a Avt,
        ns: &'a Option<Avt>,
        sets: &'a [Str],
        body: &'a [Instr],
        scope: &'a NsMap,
        c: CallContext,
    ) -> F<()> {
        let n = self.avt(ar, i, name, c)?;
        let nsv = self.opt_avt(ar, i, ns, c)?;
        let q = self.dynamic_name(ar, i, &n, nsv, scope, true);
        let Some(q) = q else {
            // Section 7.1.2: the content is still instantiated.
            return self.exec_body(ar, body, c);
        };
        let Some((doc, parent)) = self.parent() else {
            return Ok(());
        };
        let el = self.new_element(ar, doc, parent, q);
        let old = self.set_parent(el);
        let mut r = Ok(());
        for s in sets {
            r = self.use_set(ar, i, s, c, 0);
            if r.is_err() {
                break;
            }
        }
        if r.is_ok() {
            r = self.exec_body(ar, body, c);
        }
        self.set_parent(old);
        r
    }

    fn exec_copy(
        &mut self,
        ar: &mut Arena,
        i: &'a Instr,
        sets: &'a [Str],
        body: &'a [Instr],
        c: CallContext,
    ) -> F<()> {
        // Section 7.5.
        match ar.data(c.node) {
            NodeData::Document(_) | NodeData::Fragment => self.exec_body(ar, body, c),
            NodeData::Element(e) => {
                let name = e.name.clone();
                let Some((doc, parent)) = self.parent() else {
                    return Ok(());
                };
                let el = self.new_element(ar, doc, parent, name);
                self.copy_ns_nodes(ar, c.node, el, parent);
                let old = self.set_parent(el);
                let mut r = Ok(());
                for s in sets {
                    r = self.use_set(ar, i, s, c, 0);
                    if r.is_err() {
                        break;
                    }
                }
                if r.is_ok() {
                    r = self.exec_body(ar, body, c);
                }
                self.set_parent(old);
                r
            }
            _ => {
                self.copy_node(ar, i, c.node);
                Ok(())
            }
        }
    }

    /// Copies the namespaces in scope at `src` to the new element `el`.
    fn copy_ns_nodes(&mut self, ar: &mut Arena, src: NodeId, el: NodeId, parent: NodeId) {
        for d in ar.in_scope_namespaces(src) {
            if d.prefix.as_deref() == Some("xml") {
                continue;
            }
            let cur = Self::scope_ns(ar, parent, d.prefix.as_deref());
            if cur.as_deref() != Some(&*d.uri) {
                ar.declare_ns(el, d.prefix.as_deref(), &d.uri);
            }
        }
    }

    /// `xsl:copy-of` of one node (section 11.3).
    pub(super) fn copy_node(&mut self, ar: &mut Arena, i: &Instr, n: NodeId) {
        match ar.data(n) {
            NodeData::Document(_) | NodeData::Fragment => {
                for k in model_children(ar, n) {
                    self.copy_node(ar, i, k);
                }
            }
            NodeData::Attribute(a) => {
                if a.name.ns.as_deref() == Some(XMLNS_NS) {
                    return;
                }
                let name = a.name.clone();
                let v = ar.attr_value(n);
                self.add_attr(ar, i, name, &v);
            }
            NodeData::Namespace(d) => {
                let d = d.clone();
                if let Some((_, el)) = self.parent()
                    && ar.is_element(el)
                    && d.prefix.as_deref() != Some("xml")
                {
                    ar.declare_ns(el, d.prefix.as_deref(), &d.uri);
                }
            }
            NodeData::Text(t) => {
                let t = t.clone();
                let raw = ar.is_no_escape(n);
                self.text(ar, &t, raw);
            }
            NodeData::CData(t) => {
                let t = t.clone();
                self.text(ar, &t, false);
            }
            NodeData::EntityRef(_) => {
                let t = ar.text_content(n);
                self.text(ar, &t, false);
            }
            NodeData::Element(_) | NodeData::Comment(_) | NodeData::Pi { .. } => {
                let Some((doc, parent)) = self.parent() else {
                    return;
                };
                let copy = ar.clone_node(n, doc, true);
                if ar.is_element(n) {
                    self.copy_ns_nodes(ar, n, copy, parent);
                }
                ar.raw_append(parent, copy);
            }
            _ => {}
        }
    }

    fn exec_number(
        &mut self,
        ar: &mut Arena,
        i: &'a Instr,
        spec: &'a NumberSpec,
        c: CallContext,
    ) -> F<()> {
        let nums: Vec<f64> = match &spec.value {
            Some(e) => {
                let Some(v) = self.eval(ar, i, e, c)? else {
                    return Ok(());
                };
                vec![v.to_number(ar)]
            }
            None => self
                .number_values(ar, spec, c.node)
                .into_iter()
                .map(|n| n as f64)
                .collect(),
        };
        let fmt = self.avt(ar, i, &spec.format, c)?;
        let sep = self.opt_avt(ar, i, &spec.group_sep, c)?;
        let size = self.opt_avt(ar, i, &spec.group_size, c)?;
        let group = match (sep, size) {
            (Some(s), Some(n)) => {
                let n = n.trim().parse::<usize>().unwrap_or(0);
                if n > 0 { Some((s, n)) } else { None }
            }
            _ => None,
        };
        let s = format::format_numbers(&nums, &fmt, group.as_ref().map(|(s, n)| (s.as_str(), *n)));
        self.text(ar, &s, false);
        Ok(())
    }

    fn matches_any(&mut self, ar: &mut Arena, p: &[Pattern], n: NodeId) -> bool {
        p.iter().any(|x| self.pattern_matches(ar, x, n))
    }

    /// The default `count` of `xsl:number`: same type and name.
    fn same_kind(ar: &Arena, a: NodeId, b: NodeId) -> bool {
        match (ar.data(a), ar.data(b)) {
            (NodeData::Element(x), NodeData::Element(y)) => {
                x.name.local == y.name.local && x.name.ns_str() == y.name.ns_str()
            }
            (NodeData::Attribute(x), NodeData::Attribute(y)) => {
                x.name.local == y.name.local && x.name.ns_str() == y.name.ns_str()
            }
            (NodeData::Pi { target: x, .. }, NodeData::Pi { target: y, .. }) => x == y,
            (NodeData::Text(_) | NodeData::CData(_), NodeData::Text(_) | NodeData::CData(_)) => {
                true
            }
            (NodeData::Comment(_), NodeData::Comment(_)) => true,
            (NodeData::Document(_), NodeData::Document(_)) => true,
            _ => false,
        }
    }

    fn counted(&mut self, ar: &mut Arena, spec: &NumberSpec, start: NodeId, n: NodeId) -> bool {
        match &spec.count {
            Some(p) => self.matches_any(ar, p, n),
            None => Self::same_kind(ar, start, n),
        }
    }

    fn is_from(&mut self, ar: &mut Arena, spec: &NumberSpec, n: NodeId) -> bool {
        match &spec.from {
            Some(p) => self.matches_any(ar, p, n),
            None => false,
        }
    }

    /// Section 7.7's numbering levels.
    fn number_values(&mut self, ar: &mut Arena, spec: &NumberSpec, start: NodeId) -> Vec<u64> {
        let sibling_number = |tx: &mut Self, ar: &mut Arena, n: NodeId| -> u64 {
            let mut count = 1;
            let mut cur = ar.prev_sibling(n);
            while let Some(p) = cur {
                if tx.counted(ar, spec, start, p) {
                    count += 1;
                }
                cur = ar.prev_sibling(p);
            }
            count
        };
        match spec.level {
            0 => {
                let mut cur = Some(start);
                while let Some(n) = cur {
                    if self.counted(ar, spec, start, n) {
                        return vec![sibling_number(self, ar, n)];
                    }
                    if self.is_from(ar, spec, n) {
                        return Vec::new();
                    }
                    cur = ar.parent(n);
                }
                Vec::new()
            }
            1 => {
                let mut out = Vec::new();
                let mut cur = Some(start);
                while let Some(n) = cur {
                    if self.counted(ar, spec, start, n) {
                        out.push(sibling_number(self, ar, n));
                    }
                    if self.is_from(ar, spec, n) {
                        break;
                    }
                    cur = ar.parent(n);
                }
                out.reverse();
                out
            }
            _ => {
                // Backwards in document order up to a `from` node.
                let mut count = 0;
                let mut cur = Some(start);
                while let Some(n) = cur {
                    if self.counted(ar, spec, start, n) {
                        count += 1;
                    }
                    if self.is_from(ar, spec, n) {
                        break;
                    }
                    cur = match ar.prev_sibling(n) {
                        Some(mut p) if !matches!(ar.data(n), NodeData::Attribute(_)) => {
                            while let Some(l) = ar.last_child(p) {
                                if !ar.is_element(p) {
                                    break;
                                }
                                p = l;
                            }
                            Some(p)
                        }
                        _ => ar.parent(n),
                    };
                }
                if count == 0 { Vec::new() } else { vec![count] }
            }
        }
    }

    fn exec_document(
        &mut self,
        ar: &mut Arena,
        i: &'a Instr,
        d: &'a DocumentSpec,
        c: CallContext,
    ) -> F<()> {
        let href = self.avt(ar, i, &d.href, c)?;
        let base = self
            .ss
            .modules
            .get(i.module as usize)
            .and_then(|m| m.url.clone());
        let is_absolute = href.starts_with('/') || href.contains("://");
        let url = if is_absolute {
            href.clone()
        } else {
            self.host.resolve(&href, base.as_deref())
        };
        if let Some(why) = self.host.write_refusal(&url) {
            self.runtime_error(ar, i, &why);
            self.check_stop()?;
            self.runtime_error(
                ar,
                i,
                &format!("xsltDocumentElem: write rights for {url} denied"),
            );
            self.check_stop()?;
            return Ok(());
        }
        // The stylesheet's xsl:output, overridden by the instruction's
        // attributes (XSLT 1.1 section 16).
        let mut spec = self.ss.output.clone();
        merge_output(&mut spec, &d.output);
        let doc = ar.new_document(DocKind::Xml);
        apply_output_props(ar, doc, &spec);
        self.out.push(Sink::Tree {
            doc,
            parent: doc,
            cdata: false,
        });
        let r = self.exec_body(ar, &d.body, c);
        self.out.pop();
        if let Err(e) = r {
            ar.free_document_if_unheld(doc);
            return Err(e);
        }
        output::finish_result(ar, doc, &spec);
        let bytes = output::serialize(ar, doc, &spec);
        ar.free_document_if_unheld(doc);
        if !self.host.write(ar, &url, &bytes) {
            self.runtime_error(ar, i, &format!("xsltDocumentElem: unable to save to {url}"));
        }
        self.check_stop()
    }

    // ----- functions -----------------------------------------------------------

    /// Expands a QName argument with the namespaces of the expression.
    fn expand_arg(&self, name: &str) -> String {
        let name = name.trim();
        match name.split_once(':') {
            Some((p, l)) => {
                let uri = self
                    .cur_ns
                    .as_ref()
                    .and_then(|m| m.get(p).cloned())
                    .unwrap_or_default();
                clark(&uri, l)
            }
            None => name.to_string(),
        }
    }

    fn fn_key(
        &mut self,
        ar: &mut Arena,
        args: Vec<Value>,
        cc: CallContext,
    ) -> Result<Value, XPathError> {
        let [name, value] =
            <[Value; 2]>::try_from(args).map_err(|_| XPathError::new(XPathErrorKind::Arity))?;
        let name = self.expand_arg(&name.to_string_value(ar));
        let doc = ar.root(cc.node);
        let name: Str = name.as_str().into();
        let table = self.key_table(ar, &name, doc);
        let mut out = Vec::new();
        match &value {
            Value::NodeSet(v) => {
                for &n in v {
                    if let Some(list) = table.get(&xpath::string_value(ar, n)) {
                        out.extend_from_slice(list);
                    }
                }
            }
            v => {
                if let Some(list) = table.get(&v.to_string_value(ar)) {
                    out.extend_from_slice(list);
                }
            }
        }
        Ok(Value::NodeSet(out))
    }

    fn key_table(
        &mut self,
        ar: &mut Arena,
        name: &Str,
        doc: NodeId,
    ) -> Rc<HashMap<String, Vec<NodeId>>> {
        let id = (name.clone(), doc);
        if let Some(t) = self.keys.get(&id) {
            return t.clone();
        }
        let ss: &'a Stylesheet = self.ss;
        let mut table: HashMap<String, Vec<NodeId>> = HashMap::new();
        let Some(defs) = ss.keys.get(name) else {
            return Rc::new(table);
        };
        if !self.building_keys.insert(id.clone()) {
            return Rc::new(table);
        }
        let mut order = vec![doc];
        order.extend(ar.descendants(doc));
        for n in order {
            let attrs: Vec<NodeId> = if ar.is_element(n) {
                ar.attrs(n).collect()
            } else {
                Vec::new()
            };
            for x in std::iter::once(n).chain(attrs) {
                for d in defs {
                    if !self.matches_any(ar, &d.pats, x) {
                        continue;
                    }
                    let c = CallContext {
                        node: x,
                        pos: 1,
                        size: 1,
                    };
                    let v = match self.eval_raw(ar, &d.use_expr, c) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    match v {
                        Value::NodeSet(u) => {
                            for k in u {
                                let list = table.entry(xpath::string_value(ar, k)).or_default();
                                if list.last() != Some(&x) {
                                    list.push(x);
                                }
                            }
                        }
                        v => {
                            let list = table.entry(v.to_string_value(ar)).or_default();
                            if list.last() != Some(&x) {
                                list.push(x);
                            }
                        }
                    }
                }
            }
        }
        self.building_keys.remove(&id);
        let t = Rc::new(table);
        self.keys.insert(id, t.clone());
        t
    }

    fn load_doc(&mut self, ar: &mut Arena, href: &str, base: Option<&str>) -> Option<NodeId> {
        let href = href.split('#').next().unwrap_or("");
        if href.is_empty() {
            // The stylesheet itself (section 12.1).
            return self.ss.modules.get(self.cur_module as usize).map(|m| m.doc);
        }
        let url = self.host.resolve(href, base);
        if let Some(&d) = self.docs.get(&url) {
            return Some(d);
        }
        if let Some(why) = self.host.read_refusal(&url) {
            self.host.report(ar, &why);
            self.host.report(
                ar,
                &format!("xsltLoadDocument: read rights for {url} denied"),
            );
            return None;
        }
        let d = self.host.load(ar, &url, false)?;
        self.docs.insert(url, d);
        Some(d)
    }

    fn module_base(&self) -> Option<String> {
        self.ss
            .modules
            .get(self.cur_module as usize)
            .and_then(|m| m.url.clone())
    }

    fn fn_document(&mut self, ar: &mut Arena, args: Vec<Value>) -> Result<Value, XPathError> {
        if args.is_empty() || args.len() > 2 {
            return Err(XPathError::new(XPathErrorKind::Arity));
        }
        let second = match args.get(1) {
            Some(Value::NodeSet(v)) => match v.first() {
                Some(&n) => Some(ar.doc(ar.doc_of(n)).url.clone()),
                None => return Ok(Value::NodeSet(Vec::new())),
            },
            Some(_) => return Err(XPathError::new(XPathErrorKind::InvalidType)),
            None => None,
        };
        let mut out = Vec::new();
        match &args[0] {
            Value::NodeSet(v) => {
                for &n in v {
                    let s = xpath::string_value(ar, n);
                    let base = match &second {
                        Some(b) => b.clone(),
                        None => ar.doc(ar.doc_of(n)).url.clone(),
                    };
                    if let Some(d) = self.load_doc(ar, &s, base.as_deref()) {
                        out.push(d);
                    }
                }
            }
            v => {
                let s = v.to_string_value(ar);
                let base = match second {
                    Some(b) => b,
                    None => self.module_base(),
                };
                if let Some(d) = self.load_doc(ar, &s, base.as_deref()) {
                    out.push(d);
                }
            }
        }
        out.dedup();
        Ok(Value::NodeSet(out))
    }

    fn fn_format_number(&mut self, ar: &Arena, args: Vec<Value>) -> Result<Value, XPathError> {
        if args.len() < 2 || args.len() > 3 {
            return Err(XPathError::new(XPathErrorKind::Arity));
        }
        let n = args[0].to_number(ar);
        let pat = args[1].to_string_value(ar);
        let key: Option<Str> = args
            .get(2)
            .map(|a| self.expand_arg(&a.to_string_value(ar)).as_str().into());
        let def = DecimalFormat::default();
        let df = self.ss.decimal_formats.get(&key).unwrap_or(&def);
        Ok(Value::String(format::format_number(n, &pat, df)))
    }

    fn call_func(
        &mut self,
        ar: &mut Arena,
        f: &'a FuncDef,
        args: Vec<Value>,
        cc: CallContext,
    ) -> Result<Value, XPathError> {
        let save_base = self.frame_base;
        let base = self.vars.len();
        self.frame_base = base;
        self.frames.push(base);
        let mut args = args.into_iter();
        for i in &f.body {
            if let Kind::Var {
                name,
                is_param: true,
                ..
            } = &i.kind
                && let Some(v) = args.next()
            {
                self.vars.push(Var {
                    ns: name.ns.clone(),
                    local: name.local.clone(),
                    value: v,
                    is_param: false,
                    claimed: false,
                    rtf: None,
                    hidden: false,
                });
            }
        }
        let old_result = self.func_result.take();
        self.out.push(Sink::Capture(String::new()));
        self.func_depth += 1;
        self.late_free += 1;
        let r = self.exec_body(ar, &f.body, cc);
        self.late_free -= 1;
        self.func_depth -= 1;
        self.out.pop();
        if r.is_err() {
            return Err(XPathError::callback("", None));
        }
        self.pop_vars(ar, base);
        self.frames.pop();
        self.frame_base = save_base;
        let v = self.func_result.take();
        self.func_result = old_result;
        Ok(v.unwrap_or(Value::String(String::new())))
    }

    /// `element-available()` (section 15).
    fn element_available(&self, name: &str) -> bool {
        let Some((p, l)) = name.split_once(':') else {
            return false;
        };
        let uri = self
            .cur_ns
            .as_ref()
            .and_then(|m| m.get(p))
            .map_or("", String::as_str);
        if uri == super::XSL_NS {
            return matches!(
                l,
                "apply-imports"
                    | "apply-templates"
                    | "attribute"
                    | "call-template"
                    | "choose"
                    | "comment"
                    | "copy"
                    | "copy-of"
                    | "document"
                    | "element"
                    | "fallback"
                    | "for-each"
                    | "if"
                    | "message"
                    | "number"
                    | "processing-instruction"
                    | "text"
                    | "value-of"
                    | "variable"
            );
        }
        is_document_element(uri, l) || (uri == EXSLT_FUNCTIONS && l == "result")
    }

    fn function_available(&self, name: &str) -> bool {
        match name.split_once(':') {
            None => matches!(
                name,
                "last"
                    | "position"
                    | "count"
                    | "id"
                    | "local-name"
                    | "namespace-uri"
                    | "name"
                    | "string"
                    | "concat"
                    | "starts-with"
                    | "contains"
                    | "substring-before"
                    | "substring-after"
                    | "substring"
                    | "string-length"
                    | "normalize-space"
                    | "translate"
                    | "boolean"
                    | "not"
                    | "true"
                    | "false"
                    | "lang"
                    | "number"
                    | "sum"
                    | "floor"
                    | "ceiling"
                    | "round"
                    | "document"
                    | "key"
                    | "format-number"
                    | "current"
                    | "unparsed-entity-uri"
                    | "generate-id"
                    | "system-property"
                    | "element-available"
                    | "function-available"
            ),
            Some((p, l)) => {
                let uri = self
                    .cur_ns
                    .as_ref()
                    .and_then(|m| m.get(p))
                    .map_or("", String::as_str);
                self.host.has_function(uri, l)
                    || exslt::is_known(uri, l)
                    || self
                        .ss
                        .funcs
                        .contains_key(&(uri.to_string(), l.to_string()))
            }
        }
    }
}

/// Later attributes override earlier ones.
fn merge_output(to: &mut OutputSpec, from: &OutputSpec) {
    if from.method.is_some() {
        to.method = from.method;
    }
    if from.version.is_some() {
        to.version = from.version.clone();
    }
    if from.encoding.is_some() {
        to.encoding = from.encoding.clone();
    }
    if from.omit_xml_declaration.is_some() {
        to.omit_xml_declaration = from.omit_xml_declaration;
    }
    if from.standalone.is_some() {
        to.standalone = from.standalone;
    }
    if from.doctype_public.is_some() {
        to.doctype_public = from.doctype_public.clone();
    }
    if from.doctype_system.is_some() {
        to.doctype_system = from.doctype_system.clone();
    }
    if from.indent.is_some() {
        to.indent = from.indent;
    }
    if from.media_type.is_some() {
        to.media_type = from.media_type.clone();
    }
}

impl<'a> FunctionHandler for Tx<'a> {
    fn has_function(&self, _ns: &str, _local: &str) -> bool {
        true
    }

    fn call(
        &mut self,
        arena: &mut Arena,
        ns: &str,
        local: &str,
        args: Vec<Value>,
    ) -> Result<Value, XPathError> {
        let cc = CallContext {
            node: self.current,
            pos: 1,
            size: 1,
        };
        self.call_ctx(arena, ns, local, args, cc)
    }

    fn call_ctx(
        &mut self,
        arena: &mut Arena,
        ns: &str,
        local: &str,
        args: Vec<Value>,
        context: CallContext,
    ) -> Result<Value, XPathError> {
        // Functions the user registers override the EXSLT ones (test
        // xsltprocessor_exsl_registerPhpFunctionNs.phpt).
        if self.host.has_function(ns, local) {
            let r = self.host.call_function(arena, ns, local, args);
            // PHP code may have changed the trees.
            self.orders.clear();
            if self.host.stop_requested() {
                self.stopped = true;
                return Err(XPathError::callback("", None));
            }
            return r;
        }
        let ss: &'a Stylesheet = self.ss;
        if let Some(f) = ss.funcs.get(&(ns.to_string(), local.to_string())) {
            return self.call_func(arena, f, args, context);
        }
        if let Some(r) = exslt::call(self, arena, ns, local, args, context) {
            return r;
        }
        Err(XPathError::with_name(
            XPathErrorKind::UnknownFunction,
            &clark(ns, local),
        ))
    }

    fn call_host(
        &mut self,
        arena: &mut Arena,
        name: &str,
        args: Vec<Value>,
        context: CallContext,
    ) -> Result<Value, XPathError> {
        let arity = |ok: bool| -> Result<(), XPathError> {
            if ok {
                Ok(())
            } else {
                Err(XPathError::new(XPathErrorKind::Arity))
            }
        };
        match name {
            "current" => {
                arity(args.is_empty())?;
                Ok(Value::NodeSet(vec![self.current]))
            }
            "key" => self.fn_key(arena, args, context),
            "document" => self.fn_document(arena, args),
            "format-number" => self.fn_format_number(arena, args),
            "generate-id" => {
                arity(args.len() <= 1)?;
                let n = match args.first() {
                    Some(Value::NodeSet(v)) => match v.first() {
                        Some(&n) => n,
                        None => return Ok(Value::String(String::new())),
                    },
                    Some(_) => return Err(XPathError::new(XPathErrorKind::InvalidType)),
                    None => context.node,
                };
                Ok(Value::String(format!("id{}", n.index())))
            }
            "unparsed-entity-uri" => {
                arity(args.len() == 1)?;
                let name = args[0].to_string_value(arena);
                let doc = arena.doc_of(context.node);
                let uri = arena
                    .find_entity(doc, &name)
                    .and_then(|e| e.system_id.clone())
                    .unwrap_or_default();
                Ok(Value::String(uri))
            }
            "system-property" => {
                arity(args.len() == 1)?;
                let name = self.expand_arg(&args[0].to_string_value(arena));
                let xsl = format!("{{{}}}", super::XSL_NS);
                Ok(Value::String(match name.strip_prefix(xsl.as_str()) {
                    Some("version") => "1.0".into(),
                    Some("vendor") => "libxslt".into(),
                    Some("vendor-url") => "http://xmlsoft.org/XSLT/".into(),
                    _ => String::new(),
                }))
            }
            "element-available" => {
                arity(args.len() == 1)?;
                let n = args[0].to_string_value(arena);
                Ok(Value::Boolean(self.element_available(n.trim())))
            }
            "function-available" => {
                arity(args.len() == 1)?;
                let n = args[0].to_string_value(arena);
                Ok(Value::Boolean(self.function_available(n.trim())))
            }
            _ => Err(XPathError::with_name(XPathErrorKind::UnknownFunction, name)),
        }
    }

    fn variable(&mut self, arena: &mut Arena, ns: &str, local: &str) -> Result<Value, XPathError> {
        for v in self.vars[self.frame_base..].iter().rev() {
            if v.claimed && &*v.local == local && &*v.ns == ns {
                return Ok(v.value.clone());
            }
        }
        let ss: &'a Stylesheet = self.ss;
        if let Some(i) = ss
            .globals
            .iter()
            .position(|g| &*g.name.local == local && &*g.name.ns == ns)
        {
            let c = CallContext {
                node: self.source_root,
                pos: 1,
                size: 1,
            };
            return self
                .global_value(arena, i, c)
                .map_err(|_| XPathError::callback("", None));
        }
        Err(XPathError::with_name(XPathErrorKind::UndefVariable, local))
    }

    fn order_cache(&mut self) -> Option<&mut OrderCache> {
        Some(&mut self.orders)
    }
}
