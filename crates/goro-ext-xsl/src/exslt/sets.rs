//! EXSLT sets (libexslt/sets.c)

use crate::transform::Transform;
use crate::xpath::{self, PCtx, XObj};
type HashSet<T> = std::collections::HashSet<T, goro_core::fxhash::FxBuildHasher>;

pub fn exists(name: &str) -> bool {
    matches!(
        name,
        "difference" | "intersection" | "distinct" | "has-same-node" | "leading" | "trailing"
    )
}

pub fn call(t: &mut Transform, p: &mut PCtx, name: &str, nargs: usize) -> bool {
    if name == "distinct" {
        if nargs != 1 {
            xpath::x_arity_error(t, p);
            return true;
        }
        let ns = match xpath::x_pop_node_set(t, p) {
            Some(v) => v,
            None => return true,
        };
        let mut seen = HashSet::default();
        let mut ret = Vec::new();
        for n in ns {
            let s = xpath::node_string_id(n);
            if seen.insert(s) {
                ret.push(n);
            }
        }
        p.push(XObj::NodeSet(ret));
        return true;
    }
    if !exists(name) {
        return false;
    }
    if nargs != 2 {
        xpath::x_arity_error(t, p);
        return true;
    }
    let arg2 = match xpath::x_pop_node_set(t, p) {
        Some(v) => v,
        None => return true,
    };
    let arg1 = match xpath::x_pop_node_set(t, p) {
        Some(v) => v,
        None => return true,
    };
    let s2: HashSet<_> = arg2.iter().copied().collect();
    match name {
        "difference" => {
            if arg2.is_empty() {
                p.push(XObj::NodeSet(arg1));
            } else {
                let mut r = Vec::new();
                for n in arg1 {
                    if !s2.contains(&n) {
                        r.push(n);
                    }
                }
                p.push(XObj::NodeSet(r));
            }
        }
        "intersection" => {
            let r: Vec<_> = arg1.into_iter().filter(|n| s2.contains(n)).collect();
            p.push(XObj::NodeSet(r));
        }
        "has-same-node" => {
            let r = !arg1.is_empty() && arg1.iter().any(|n| s2.contains(n));
            p.push(XObj::Bool(r));
        }
        "leading" => {
            if arg2.is_empty() {
                p.push(XObj::NodeSet(arg1));
            } else {
                let node = arg2[0];
                let mut r = Vec::new();
                if arg1.contains(&node) {
                    for n in arg1 {
                        if n == node {
                            break;
                        }
                        r.push(n);
                    }
                }
                p.push(XObj::NodeSet(r));
            }
        }
        _ => {
            if arg2.is_empty() {
                p.push(XObj::NodeSet(arg1));
            } else {
                let node = arg2[0];
                let mut r = Vec::new();
                if arg1.contains(&node) {
                    for &n in arg1.iter().rev() {
                        if n == node {
                            break;
                        }
                        r.push(n);
                    }
                    xpath::sort_nodes(&mut r);
                }
                p.push(XObj::NodeSet(r));
            }
        }
    }
    true
}
