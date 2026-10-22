//! EXSLT common (libexslt/common.c)

use crate::transform::Transform;
use crate::xpath::{self, PCtx, XObj};

pub fn call(t: &mut Transform, p: &mut PCtx, name: &str, nargs: usize) -> bool {
    match name {
        "node-set" => {
            if nargs != 1 {
                xpath::x_arity_error(t, p);
                return true;
            }
            match p.top() {
                Some(XObj::NodeSet(_)) | Some(XObj::Tree(_)) => {
                    if let Some(top) = p.top_mut() {
                        if let XObj::Tree(v) = top {
                            *top = XObj::NodeSet(std::mem::take(v));
                        }
                    }
                }
                _ => {
                    let frag = t.create_rvt();
                    let s = xpath::x_pop_string(t, p).unwrap_or_default();
                    let txt = goro_ext_xml::tree::with(|tr| {
                        let n = tr.new_text(Some(frag), &s);
                        tr.link_child_raw(frag, n);
                        n
                    });
                    p.push(XObj::NodeSet(vec![txt]));
                }
            }
        }
        "object-type" => {
            if nargs != 1 {
                xpath::x_arity_error(t, p);
                return true;
            }
            let r = match p.pop() {
                Some(XObj::Str(_)) => "string",
                Some(XObj::Num(_)) => "number",
                Some(XObj::Bool(_)) => "boolean",
                Some(XObj::NodeSet(_)) => "node-set",
                Some(XObj::Tree(_)) => "RTF",
                None => {
                    t.generic_error("object-type() invalid arg\n");
                    p.error = xpath::XPATH_INVALID_TYPE;
                    return true;
                }
            };
            p.push(XObj::Str(r.to_string()));
        }
        _ => return false,
    }
    true
}
