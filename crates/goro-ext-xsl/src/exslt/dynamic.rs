//! EXSLT dynamic (libexslt/dynamic.c) and the Saxon extension functions
//! (libexslt/saxon.c).

use crate::transform::Transform;
use crate::util::*;
use crate::xpath::{self, CompExpr, PCtx, XObj};
use goro_ext_xml::tree::{NType, NodeId};

const EXSLT_COMMON: &str = "http://exslt.org/common";

/// xmlXPathCtxtCompile: errors are reported like libxml2's handler does.
fn compile(t: &mut Transform, expr: &str) -> Option<CompExpr> {
    match xpath::compile(expr) {
        Ok(c) => Some(c),
        Err(msgs) => {
            for m in msgs {
                php_error(t.vm, &format!("{}\n", m));
            }
            None
        }
    }
}

/// xmlXPathCompiledEval in the current XPath context
fn eval_in_ctx(t: &mut Transform, comp: &CompExpr) -> Option<XObj> {
    let saved = t.xp.clone();
    let r = xpath::eval(t, comp);
    let d = t.xp.depth;
    t.xp = saved;
    t.xp.depth = d;
    r
}

pub fn call(t: &mut Transform, p: &mut PCtx, name: &str, nargs: usize) -> bool {
    match name {
        "evaluate" => evaluate(t, p, nargs),
        "map" => map(t, p, nargs),
        _ => return false,
    }
    true
}

/// exsltDynEvaluateFunction
fn evaluate(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    if nargs != 1 {
        php_error(
            t.vm,
            &format!("dyn:evalute() : invalid number of args {}\n", nargs),
        );
        p.error = xpath::XPATH_INVALID_ARITY;
        return;
    }
    let s = xpath::x_pop_string(t, p).unwrap_or_default();
    if s.is_empty() {
        p.push(XObj::NodeSet(Vec::new()));
        return;
    }
    t.xp.depth += 5;
    let r = compile(t, &s).and_then(|c| eval_in_ctx(t, &c));
    t.xp.depth -= 5;
    match r {
        Some(v) => p.push(v),
        None => {
            php_error(
                t.vm,
                &format!("dyn:evaluate() : unable to evaluate expression '{}'\n", s),
            );
            p.push(XObj::NodeSet(Vec::new()));
        }
    }
}

/// An exsl:boolean / exsl:number / exsl:string element of dyn:map()
fn map_item(container: NodeId, name: &str, value: &str) -> NodeId {
    tw(|tr| {
        let e = tr.new_element(Some(container), name);
        // xmlNewTextChild: a text child even for an empty value
        let txt = tr.new_text(Some(container), value);
        tr.add_child(e, txt);
        tr.add_child(container, e);
        if let Some(ns) = tr.new_ns(Some(e), EXSLT_COMMON, Some("exsl")) {
            tr.nodes[e].ns = Some(ns);
        }
        e
    })
}

/// exsltDynMapFunction
fn map(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    if nargs != 2 {
        xpath::x_arity_error(t, p);
        return;
    }
    let s = xpath::x_pop_string(t, p);
    if p.error != 0 {
        return;
    }
    let nodes = xpath::x_pop_node_set(t, p);
    if p.error != 0 {
        return;
    }
    let mut ret: Vec<NodeId> = Vec::new();
    let s = s.unwrap_or_default();
    let comp = if s.is_empty() { None } else { compile(t, &s) };
    let Some(comp) = comp else {
        p.push(XObj::NodeSet(ret));
        return;
    };
    let saved = t.xp.clone();
    let container = t.create_rvt();
    let mut nodes = nodes.unwrap_or_default();
    if !nodes.is_empty() {
        xpath::sort_nodes(&mut nodes);
        t.xp.size = nodes.len() as i32;
        t.xp.pos = 0;
        for &cur in &nodes {
            t.xp.pos += 1;
            t.xp.node = Some(cur);
            let (is_ns, owner) = tw(|tr| {
                (
                    tr.nodes[cur].t == NType::NamespaceDecl,
                    tr.nodes[cur].parent,
                )
            });
            let doc_of = |n: NodeId| {
                tw(|tr| {
                    if tr.is_doc(n) {
                        Some(n)
                    } else {
                        tr.nodes[n].doc
                    }
                })
            };
            if is_ns {
                match owner {
                    Some(o) if tw(|tr| tr.nodes[o].t == NType::Element) => {
                        t.xp.doc = doc_of(o);
                    }
                    _ => {
                        php_error(
                            t.vm,
                            "Internal error in exsltDynMapFunction: Cannot retrieve the doc of a namespace node.\n",
                        );
                        continue;
                    }
                }
            } else {
                t.xp.doc = doc_of(cur);
            }
            let r = xpath::eval(t, &comp);
            match r {
                Some(XObj::NodeSet(v)) => {
                    for n in v {
                        if !ret.contains(&n) {
                            ret.push(n);
                        }
                    }
                }
                Some(XObj::Bool(b)) => {
                    ret.push(map_item(container, "boolean", if b { "true" } else { "" }));
                }
                Some(XObj::Num(n)) => {
                    ret.push(map_item(container, "number", &xpath::number_to_string(n)));
                }
                Some(XObj::Str(sv)) => {
                    ret.push(map_item(container, "string", &sv));
                }
                _ => {}
            }
        }
    }
    let d = t.xp.depth;
    t.xp = saved;
    t.xp.depth = d;
    p.push(XObj::NodeSet(ret));
}

/// The Saxon functions. saxon:expression() returns the expression string
/// (there are no "external" XPath objects); saxon:eval() compiles it.
pub fn saxon_call(t: &mut Transform, p: &mut PCtx, name: &str, nargs: usize) -> bool {
    match name {
        "expression" => {
            if nargs != 1 {
                xpath::x_arity_error(t, p);
                return true;
            }
            let Some(s) = xpath::x_pop_string(t, p) else {
                xpath::xp_err(t, p, xpath::XPATH_INVALID_TYPE);
                return true;
            };
            if compile(t, &s).is_none() {
                xpath::xp_err(t, p, xpath::XPATH_EXPR_ERROR);
                return true;
            }
            p.push(XObj::Str(s));
        }
        "eval" | "evaluate" => {
            if nargs != 1 {
                xpath::x_arity_error(t, p);
                return true;
            }
            if name == "eval" && !matches!(p.top(), Some(XObj::Str(_))) {
                xpath::xp_err(t, p, xpath::XPATH_INVALID_TYPE);
                return true;
            }
            let Some(s) = xpath::x_pop_string(t, p) else {
                return true;
            };
            let Some(c) = compile(t, &s) else {
                xpath::xp_err(t, p, xpath::XPATH_EXPR_ERROR);
                return true;
            };
            match eval_in_ctx(t, &c) {
                Some(v) => p.push(v),
                None => xpath::xp_err(t, p, xpath::XPATH_EXPR_ERROR),
            }
        }
        "systemId" => {
            if nargs != 0 {
                xpath::x_arity_error(t, p);
                return true;
            }
            let url =
                t.xp.doc
                    .and_then(|d| tw(|tr| tr.doc_info(d).and_then(|i| i.url.clone())));
            p.push(XObj::Str(url.unwrap_or_default()));
        }
        "line-number" => {
            let mut cur: Option<NodeId> = None;
            if nargs == 0 {
                cur = t.xp.node;
            } else if nargs == 1 {
                match p.top() {
                    Some(XObj::NodeSet(_)) => {}
                    _ => {
                        t.error(
                            None,
                            "saxon:line-number() : invalid arg expecting a node-set\n",
                        );
                        p.error = xpath::XPATH_INVALID_TYPE;
                        return true;
                    }
                }
                if let Some(XObj::NodeSet(mut v)) = p.pop() {
                    xpath::sort_nodes(&mut v);
                    cur = v.first().copied();
                }
            } else {
                t.error(
                    None,
                    &format!("saxon:line-number() : invalid number of args {}\n", nargs),
                );
                p.error = xpath::XPATH_INVALID_ARITY;
                return true;
            }
            if let Some(c) = cur {
                if tw(|tr| tr.nodes[c].t == NType::NamespaceDecl) {
                    let owner = tw(|tr| {
                        tr.nodes[c]
                            .parent
                            .filter(|&o| tr.nodes[o].t == NType::Element)
                    });
                    if owner.is_none() {
                        php_error(
                            t.vm,
                            "Internal error in exsltSaxonLineNumberFunction: Cannot retrieve the doc of a namespace node.\n",
                        );
                    }
                    cur = owner;
                }
            }
            let line = match cur {
                Some(c) => tw(|tr| get_line_no(tr, c)) as f64,
                None => -1.0,
            };
            p.push(XObj::Num(line));
        }
        _ => return false,
    }
    true
}
