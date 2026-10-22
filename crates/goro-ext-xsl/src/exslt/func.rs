//! EXSLT functions: func:function / func:result (libexslt/functions.c)

use crate::style::{Compiler, EXSLT_FUNCTIONS_NAMESPACE, Kind, PreComp};
use crate::transform::{State, Transform};
use crate::util::*;
use crate::xpath::{self, PCtx, XObj};
use goro_ext_xml::tree::{NType, NodeId};
use std::rc::Rc;

/// exsltFuncFunctionComp: a top-level func:function element
pub fn func_function_comp(c: &mut Compiler, s: usize, inst: NodeId) {
    let qname = tw(|t| get_prop(t, inst, "name"));
    let Some((prefix, name)) = qname.as_deref().and_then(split_qname2) else {
        php_error(c.vm, "func:function: not a QName\n");
        return;
    };
    let href = tw(|t| {
        t.search_ns(inst, Some(&prefix))
            .map(|n| t.nss[n].href.clone())
    });
    let Some(href) = href else {
        php_error(
            c.vm,
            &format!("func:function: undeclared prefix {}\n", prefix),
        );
        return;
    };
    c.parse_template_content(s, inst);
    let key = (href.clone(), name.clone());
    if c.sheet.ext_function_style.get(&key) == Some(&s) {
        transform_error(
            c.vm,
            false,
            true,
            Some(inst),
            &format!("Failed to register function {{{}}}{}\n", href, name),
        );
        c.sheet.styles[s].errors += 1;
        return;
    }
    // functions of higher import precedence are registered later
    c.sheet.ext_functions.insert(key.clone(), inst);
    c.sheet.ext_function_style.insert(key, s);
}

/// exsltFuncResultComp: None when the element is invalid (a plain
/// precomputation is used then).
pub fn func_result_comp(c: &mut Compiler, s: usize, inst: NodeId) -> Option<PreComp> {
    // only xsl:fallback may follow func:result
    let bad_follow = tw(|t| {
        let mut n = t.nodes[inst].next;
        while let Some(x) = n {
            if t.nodes[x].t == NType::Element
                && !(is_xslt_elem(t, x) && is_xslt_name(t, x, "fallback"))
            {
                return true;
            }
            n = t.nodes[x].next;
        }
        false
    });
    if bad_follow {
        php_error(
            c.vm,
            "exsltFuncResultElem: only xsl:fallback is allowed to follow func:result\n",
        );
        c.sheet.styles[s].errors += 1;
        return None;
    }
    // 0: ok, 1: not in func:function, 2: in func:result, 3: in a variable
    let r = tw(|t| {
        let mut p = t.nodes[inst].parent;
        while let Some(x) = p {
            if t.is_doc(x) || (is_xslt_elem(t, x) && is_xslt_name(t, x, "stylesheet")) {
                return 1;
            }
            if t.ns_href(t.nodes[x].ns) == Some(EXSLT_FUNCTIONS_NAMESPACE) {
                if t.nodes[x].name == "function" {
                    return 0;
                }
                if t.nodes[x].name == "result" {
                    return 2;
                }
            }
            if is_xslt_elem(t, x) && (is_xslt_name(t, x, "variable") || is_xslt_name(t, x, "param"))
            {
                return 3;
            }
            p = t.nodes[x].parent;
        }
        0
    });
    let msg = match r {
        1 => Some("func:result element not a descendant of a func:function\n"),
        2 => Some("func:result element not allowed within another func:result element\n"),
        3 => Some("func:result element not allowed within a variable binding element\n"),
        _ => None,
    };
    if let Some(m) = msg {
        php_error(c.vm, m);
        c.sheet.styles[s].errors += 1;
        return None;
    }
    let sel = tw(|t| get_ns_prop(t, inst, "select", None));
    let comp = match &sel {
        Some(e) => c.xpath_compile(e),
        None => None,
    };
    let ns_list = Rc::new(tw(|t| xpath_ns_list(t, inst)));
    Some(PreComp {
        kind: Kind::Extension,
        inst,
        select: sel,
        comp,
        ns_list,
        ..Default::default()
    })
}

/// exsltFuncFunctionFunction: a call of a func:function
pub fn func_function_call(
    t: &mut Transform,
    p: &mut PCtx,
    f: NodeId,
    uri: &str,
    name: &str,
    nargs: usize,
) {
    let old_result = t.func_result.take();
    // the parameters and the body
    let (fnargs, content, params) = tw(|tr| {
        let mut params = Vec::new();
        let mut c = tr.nodes[f].children;
        while let Some(x) = c {
            if is_xslt_elem(tr, x) && is_xslt_name(tr, x, "param") {
                params.push(x);
                c = tr.nodes[x].next;
            } else {
                break;
            }
        }
        (params.len(), c, params)
    });
    if nargs > fnargs {
        php_error(
            t.vm,
            &format!("{{{}}}{}: called with too many arguments\n", uri, name),
        );
        p.error = xpath::XPATH_INVALID_ARITY;
        t.func_result = old_result;
        return;
    }
    if t.depth >= t.max_template_depth {
        t.error(
            None,
            &format!(
                "exsltFuncFunctionFunction: Potentially infinite recursion detected in function {{{}}}{}.\n",
                uri, name
            ),
        );
        t.state = State::Stopped;
        t.func_result = old_result;
        return;
    }
    t.depth += 1;
    let old_xp_node = t.xp.node;
    let fake = tw(|tr| tr.new_element(Some(t.output), "fake"));
    let new_base = t.vars.len();
    let mut args: Vec<XObj> = Vec::new();
    if !params.is_empty() {
        for _ in 0..nargs {
            args.push(p.pop().unwrap_or(XObj::Str(String::new())));
        }
        args.reverse();
        for (i, &pn) in params.iter().enumerate() {
            let Some(param) = t.parse_stylesheet_caller_param(pn) else {
                t.local_variable_pop(new_base, -2);
                tw(|tr| tr.unlink(fake));
                t.depth -= 1;
                t.func_result = old_result;
                return;
            };
            if i < args.len() {
                let mut b = param.borrow_mut();
                b.computed = true;
                b.value = Some(args[i].clone());
            }
            param.borrow_mut().level = -1;
            t.vars.push(param);
        }
    }
    let old_base = t.vars_base;
    let old_insert = t.insert;
    let old_ctxt_var = t.func_ctxt_var.take();
    t.func_ctxt_var = t.contextual_var.take();
    t.vars_base = new_base;
    t.insert = Some(fake);
    let node = t.node;
    t.apply_one_template(node, content);
    let base = t.vars_base;
    t.local_variable_pop(base, -2);
    t.vars.truncate(new_base);
    t.insert = old_insert;
    t.contextual_var = t.func_ctxt_var.take();
    t.vars_base = old_base;
    t.func_ctxt_var = old_ctxt_var;
    t.xp.node = old_xp_node;
    let error = std::mem::replace(&mut t.func_error, false);
    let result = t.func_result.take();
    t.func_result = old_result;
    let wrote = tw(|tr| tr.nodes[fake].children.is_some());
    t.depth -= 1;
    if error {
        return;
    }
    let ret = result.unwrap_or(XObj::Str(String::new()));
    if wrote {
        php_error(
            t.vm,
            &format!(
                "{{{}}}{}: cannot write to result tree while executing a function\n",
                uri, name
            ),
        );
        return;
    }
    p.push(ret);
}

/// exsltFuncResultElem
pub fn func_result_elem(t: &mut Transform, _node: NodeId, inst: NodeId, comp: &Rc<PreComp>) {
    if t.func_result.is_some() {
        php_error(t.vm, "func:result already instanciated\n");
        t.func_error = true;
        return;
    }
    t.contextual_var = t.func_ctxt_var.clone();
    let has_children = tw(|tr| tr.nodes[inst].children.is_some());
    let ret = if let Some(sel) = &comp.comp {
        if has_children {
            php_error(
                t.vm,
                "func:result content must be empty if the function has a select attribute\n",
            );
            t.func_error = true;
            return;
        }
        let node = t.node;
        match t.eval_comp(node, sel, &comp.ns_list) {
            Some(r) => r,
            None => {
                php_error(t.vm, "exsltFuncResultElem: ret == NULL\n");
                return;
            }
        }
    } else if has_children {
        let container = t.create_rvt();
        let old_insert = t.insert;
        t.insert = Some(container);
        let node = t.node;
        let children = tw(|tr| tr.nodes[inst].children);
        t.apply_one_template(node, children);
        t.insert = old_insert;
        XObj::Tree(vec![container])
    } else {
        XObj::Str(String::new())
    };
    t.func_result = Some(ret);
}
