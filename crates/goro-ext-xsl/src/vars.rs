//! Variables and parameters (variables.c) and the XPath host interface of
//! the transformation context.

use crate::style::{Kind, PreComp};
use crate::transform::{Elem, StackElem, State, Transform};
use crate::util::*;
use crate::xpath::{self, Host, PCtx, XObj, XpCtx};
use goro_ext_xml::tree::{NType, NodeId};
use std::cell::RefCell;
use std::rc::Rc;

impl<'v> Transform<'v> {
    /// xsltCreateRVT: a new result tree fragment container
    pub fn create_rvt(&mut self) -> NodeId {
        let d = tw(|t| {
            let d = t.new_doc(None);
            t.nodes[d].name = xpath::RTF_NAME.to_string();
            d
        });
        self.rtfs.push(d);
        d
    }

    /// xsltEvalVariable
    pub fn eval_variable(&mut self, variable: &Elem, comp: Option<&Rc<PreComp>>) -> Option<XObj> {
        let old_inst = self.inst;
        let (select, tree, name) = {
            let v = variable.borrow();
            (v.select.clone(), v.tree, v.name.clone())
        };
        let result;
        if let Some(sel) = select {
            let expr = match comp.and_then(|c| c.comp.clone()) {
                Some(c) => c,
                None => match xpath::compile(&sel) {
                    Ok(c) => Rc::new(c),
                    Err(msgs) => {
                        for m in msgs {
                            php_error(self.vm, &format!("{}\n", m));
                        }
                        return None;
                    }
                },
            };
            let saved = (
                self.xp.doc,
                self.xp.node,
                self.xp.pos,
                self.xp.size,
                self.xp.namespaces.clone(),
            );
            let cn = self.node;
            self.xp.node = Some(cn);
            let (ct, cd) = tw(|t| (t.nodes[cn].t, t.nodes[cn].doc));
            if ct != NType::NamespaceDecl {
                if let Some(d) = cd {
                    self.xp.doc = Some(d);
                }
            }
            self.xp.namespaces = match comp {
                Some(c) => c.ns_list.clone(),
                None => Rc::new(Vec::new()),
            };
            let old_var = self.contextual_var.replace(variable.clone());
            result = xpath::eval(self, &expr);
            self.contextual_var = old_var;
            self.xp.doc = saved.0;
            self.xp.node = saved.1;
            self.xp.pos = saved.2;
            self.xp.size = saved.3;
            self.xp.namespaces = saved.4;
            if result.is_none() {
                let inst = comp.map(|c| c.inst);
                self.error(
                    inst,
                    &format!(
                        "Failed to evaluate the expression of variable '{}'.\n",
                        name
                    ),
                );
                self.state = State::Stopped;
            }
        } else {
            match tree {
                None => result = Some(XObj::Str(String::new())),
                Some(tree) => {
                    let container = self.create_rvt();
                    let old_output = self.output;
                    let old_insert = self.insert;
                    self.output = container;
                    self.insert = Some(container);
                    let old_var = self.contextual_var.replace(variable.clone());
                    let cn = self.node;
                    self.apply_one_template(cn, Some(tree));
                    self.contextual_var = old_var;
                    self.insert = old_insert;
                    self.output = old_output;
                    result = Some(XObj::Tree(vec![container]));
                }
            }
        }
        self.inst = old_inst;
        result
    }

    /// xsltEvalGlobalVariable
    pub fn eval_global_variable(&mut self, elem: &Elem) -> Option<XObj> {
        if elem.borrow().computed {
            return elem.borrow().value.clone();
        }
        let old_inst = self.inst;
        elem.borrow_mut().computing = true;
        let old_ctx_var = self.contextual_var.take();
        let (select, tree, comp) = {
            let e = elem.borrow();
            (e.select.clone(), e.tree, e.comp.clone())
        };
        let mut result = None;
        if let Some(sel) = select {
            let expr = match comp.as_ref().and_then(|c| c.comp.clone()) {
                Some(c) => Some(c),
                None => match xpath::compile(&sel) {
                    Ok(c) => Some(Rc::new(c)),
                    Err(msgs) => {
                        for m in msgs {
                            php_error(self.vm, &format!("{}\n", m));
                        }
                        None
                    }
                },
            };
            if let Some(expr) = expr {
                self.inst = comp.as_ref().map(|c| c.inst);
                let saved = (
                    self.xp.doc,
                    self.xp.node,
                    self.xp.pos,
                    self.xp.size,
                    self.xp.namespaces.clone(),
                );
                self.xp.node = Some(self.initial_context_node);
                self.xp.doc = Some(self.initial_context_doc);
                self.xp.size = 1;
                self.xp.pos = 1;
                self.xp.namespaces = match &comp {
                    Some(c) => c.ns_list.clone(),
                    None => Rc::new(Vec::new()),
                };
                result = xpath::eval(self, &expr);
                self.xp.doc = saved.0;
                self.xp.node = saved.1;
                self.xp.pos = saved.2;
                self.xp.size = saved.3;
                self.xp.namespaces = saved.4;
                if result.is_none() {
                    let inst = comp.as_ref().map(|c| c.inst);
                    // elem->name is xsltComputingGlobalVarMarker meanwhile
                    self.error(
                        inst,
                        "Evaluating global variable  var/param being computed failed\n",
                    );
                    self.state = State::Stopped;
                }
            }
        } else {
            match tree {
                None => result = Some(XObj::Str(String::new())),
                Some(tree) => {
                    let container = self.create_rvt();
                    let old_output = self.output;
                    let old_insert = self.insert;
                    let old_doc = self.xp.doc;
                    self.output = container;
                    self.insert = Some(container);
                    self.xp.doc = Some(self.initial_context_doc);
                    let cn = self.node;
                    self.apply_one_template(cn, Some(tree));
                    self.xp.doc = old_doc;
                    self.insert = old_insert;
                    self.output = old_output;
                    result = Some(XObj::Tree(vec![container]));
                }
            }
        }
        self.contextual_var = old_ctx_var;
        elem.borrow_mut().computing = false;
        self.inst = old_inst;
        if let Some(r) = &result {
            let mut e = elem.borrow_mut();
            e.value = Some(r.clone());
            e.computed = true;
        }
        result
    }

    /// xsltEvalGlobalVariables
    pub fn eval_global_variables(&mut self) {
        let mut head: Vec<Elem> = Vec::new();
        let sheet = self.sheet.clone();
        for s in sheet.import_order() {
            // style->variables is a list with the last declared first
            for def in sheet.styles[s].variables.iter().rev() {
                let key = (def.name.clone(), def.name_uri.clone());
                match self.global_vars.get(&key) {
                    None => {
                        let e = Rc::new(RefCell::new(StackElem {
                            name: def.name.clone(),
                            name_uri: def.name_uri.clone(),
                            select: def.select.clone(),
                            tree: def.tree,
                            comp: Some(def.comp.clone()),
                            computed: false,
                            value: None,
                            level: 0,
                            computing: false,
                        }));
                        self.global_vars.insert(key, e.clone());
                        head.insert(0, e);
                    }
                    Some(existing) => {
                        if def.comp.kind == Kind::Variable {
                            let same_doc = {
                                let ex = existing.borrow();
                                match &ex.comp {
                                    Some(ec) => {
                                        tw(|t| t.nodes[ec.inst].doc == t.nodes[def.comp.inst].doc)
                                    }
                                    None => false,
                                }
                            };
                            if same_doc {
                                self.error(
                                    Some(def.comp.inst),
                                    &format!("Global variable {} already defined\n", def.name),
                                );
                            }
                        }
                    }
                }
            }
        }
        for e in head {
            self.eval_global_variable(&e);
        }
    }

    /// xsltProcessUserParamInternal with eval = 0 (xsltQuoteOneUserParam)
    pub fn quote_user_param(&mut self, name: &str, value: &str) -> i32 {
        let mut href: Option<String> = None;
        let mut lname = name.to_string();
        if name.starts_with('{') {
            match name.find('}') {
                None => {
                    self.error(
                        None,
                        &format!("user param : malformed parameter name : {}\n", name),
                    );
                }
                Some(i) => {
                    href = Some(name[1..i].to_string());
                    lname = name[i + 1..].to_string();
                }
            }
        } else {
            let (prefix, local) = split_qname(name);
            lname = local;
            if let Some(p) = prefix {
                let doc = self.sheet.styles[0].doc;
                let ns = tw(|t| {
                    let root = t.root_element(doc)?;
                    t.search_ns(root, Some(&p)).map(|n| t.nss[n].href.clone())
                });
                match ns {
                    None => {
                        self.error(
                            None,
                            &format!("user param : no namespace bound to prefix {}\n", p),
                        );
                        href = None;
                    }
                    Some(h) => href = Some(h),
                }
            }
        }
        let key = (lname.clone(), href.clone());
        if self.global_vars.contains_key(&key) {
            self.error(
                None,
                &format!("Global parameter {} already defined\n", lname),
            );
        }
        // do not overwrite variables with parameters from the command line
        // (libxslt only looks at the principal stylesheet's variables)
        let sheet = self.sheet.clone();
        for _s in sheet.import_order() {
            for def in &sheet.styles[0].variables {
                if def.comp.kind == Kind::Variable && def.name == lname && def.name_uri == href {
                    return 0;
                }
            }
        }
        let e = Rc::new(RefCell::new(StackElem {
            name: lname.clone(),
            name_uri: href,
            select: Some(value.to_string()),
            tree: None,
            comp: None,
            computed: true,
            value: Some(XObj::Str(value.to_string())),
            level: 0,
            computing: false,
        }));
        if self.global_vars.contains_key(&key) {
            self.error(
                None,
                &format!("Global parameter {} already defined\n", lname),
            );
        } else {
            self.global_vars.insert(key, e);
        }
        0
    }

    /// xsltBuildVariable
    fn build_variable(&mut self, comp: &Rc<PreComp>, tree: Option<NodeId>) -> Elem {
        let elem = Rc::new(RefCell::new(StackElem {
            name: comp.name.clone().unwrap_or_default(),
            name_uri: comp.ns.clone(),
            select: comp.select.clone(),
            tree,
            comp: Some(comp.clone()),
            computed: false,
            value: None,
            level: 0,
            computing: false,
        }));
        let v = self.eval_variable(&elem, Some(comp));
        {
            let mut e = elem.borrow_mut();
            e.value = v;
            e.computed = true;
        }
        elem
    }

    /// xsltStackLookup
    fn stack_lookup(&self, name: &str, uri: Option<&str>) -> Option<Elem> {
        let mut i = self.vars.len();
        while i > self.vars_base {
            i -= 1;
            let v = &self.vars[i];
            let b = v.borrow();
            if b.name == name && b.name_uri.as_deref() == uri {
                return Some(v.clone());
            }
        }
        None
    }

    /// xsltCheckStackElem: 1 variable, 2 param, 3 with-param, 0 not found
    fn check_stack_elem(&self, name: &str, uri: Option<&str>) -> i32 {
        match self.stack_lookup(name, uri) {
            None => 0,
            Some(e) => match e.borrow().comp.as_ref().map(|c| c.kind) {
                Some(Kind::WithParam) => 3,
                Some(Kind::Param) => 2,
                _ => 1,
            },
        }
    }

    /// xsltRegisterVariable
    fn register_variable(&mut self, comp: &Rc<PreComp>, tree: Option<NodeId>, is_param: bool) {
        let name = comp.name.clone().unwrap_or_default();
        let present = self.check_stack_elem(&name, comp.ns.as_deref());
        if !is_param {
            if present != 0 && present != 3 {
                self.error(
                    Some(comp.inst),
                    &format!("XSLT-variable: Redefinition of variable '{}'.\n", name),
                );
                return;
            }
        } else if present != 0 {
            if present == 1 || present == 2 {
                self.error(
                    Some(comp.inst),
                    &format!("XSLT-param: Redefinition of parameter '{}'.\n", name),
                );
                return;
            }
            return;
        }
        let v = self.build_variable(comp, tree);
        self.vars.push(v);
    }

    /// xsltParseStylesheetVariable
    pub fn parse_stylesheet_variable(&mut self, inst: NodeId) {
        let comp = match self.sheet.precomp.get(&inst).cloned() {
            Some(c) => c,
            None => {
                self.error(
                    Some(inst),
                    "Internal error in xsltParseStylesheetVariable(): The XSLT 'variable' instruction was not compiled.\n",
                );
                return;
            }
        };
        if comp.name.is_none() {
            self.error(
                Some(inst),
                "Internal error in xsltParseStylesheetVariable(): The attribute 'name' was not compiled.\n",
            );
            return;
        }
        let tree = tw(|t| t.nodes[inst].children);
        self.register_variable(&comp, tree, false);
    }

    /// xsltParseStylesheetParam
    pub fn parse_stylesheet_param(&mut self, cur: NodeId) {
        let comp = match self.sheet.precomp.get(&cur).cloned() {
            Some(c) if c.name.is_some() => c,
            _ => {
                self.error(
                    Some(cur),
                    "Internal error in xsltParseStylesheetParam(): The XSLT 'param' declaration was not compiled correctly.\n",
                );
                return;
            }
        };
        let tree = tw(|t| t.nodes[cur].children);
        let before = self.vars.len();
        self.register_variable(&comp, tree, true);
        // xsltLocalVariablePush(-1) is not used here: params get level 0
        let _ = before;
    }

    /// xsltParseStylesheetCallerParam
    pub fn parse_stylesheet_caller_param(&mut self, inst: NodeId) -> Option<Elem> {
        let comp = match self.sheet.precomp.get(&inst).cloned() {
            Some(c) => c,
            None => {
                self.error(
                    Some(inst),
                    "Internal error in xsltParseStylesheetCallerParam(): The XSLT 'with-param' instruction was not compiled.\n",
                );
                return None;
            }
        };
        if comp.name.is_none() {
            self.error(
                Some(inst),
                "Internal error in xsltParseStylesheetCallerParam(): XSLT 'with-param': The attribute 'name' was not compiled.\n",
            );
            return None;
        }
        let tree = if comp.select.is_none() {
            tw(|t| t.nodes[inst].children)
        } else {
            Some(inst)
        };
        Some(self.build_variable(&comp, tree))
    }

    /// xsltGlobalVariableLookup
    fn global_variable_lookup(&mut self, name: &str, uri: Option<&str>) -> Option<XObj> {
        let elem = self
            .global_vars
            .get(&(name.to_string(), uri.map(|u| u.to_string())))
            .cloned()?;
        let (computed, computing) = {
            let e = elem.borrow();
            (e.computed, e.computing)
        };
        if !computed {
            if computing {
                let inst = elem.borrow().comp.as_ref().map(|c| c.inst);
                self.error(inst, &format!("Recursive definition of {}\n", name));
                return None;
            }
            return self.eval_global_variable(&elem);
        }
        elem.borrow().value.clone()
    }

    /// xsltXPathVariableLookup
    pub fn variable_lookup(&mut self, name: &str, uri: Option<&str>) -> Option<XObj> {
        if !self.vars.is_empty() {
            if let Some(v) = self.stack_lookup(name, uri) {
                let computed = v.borrow().computed;
                if !computed {
                    let val = self.eval_variable(&v, None);
                    let mut b = v.borrow_mut();
                    b.value = val;
                    b.computed = true;
                }
                return v.borrow().value.clone();
            }
        }
        let r = self.global_variable_lookup(name, uri);
        if r.is_none() {
            let inst = self.inst;
            match uri {
                Some(u) => self.error(
                    inst,
                    &format!("Variable '{{{}}}{}' has not been declared.\n", u, name),
                ),
                None => self.error(
                    inst,
                    &format!("Variable '{}' has not been declared.\n", name),
                ),
            }
        }
        r
    }
}

impl<'v> Host for Transform<'v> {
    fn xp(&mut self) -> &mut XpCtx {
        &mut self.xp
    }

    fn lookup_var(&mut self, name: &str, uri: Option<&str>) -> Option<XObj> {
        self.variable_lookup(name, uri)
    }

    fn call_function(&mut self, p: &mut PCtx, name: &str, uri: Option<&str>, nargs: usize) -> bool {
        crate::functions::call(self, p, name, uri, nargs)
    }

    fn xpath_error(&mut self, msg: &str) {
        php_error(self.vm, &format!("{}\n", msg));
    }

    fn ns_hash_lookup(&self, prefix: &str) -> Option<String> {
        self.sheet.styles[0].ns_hash.get(prefix).cloned()
    }
}
