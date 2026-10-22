//! EXSLT math (libexslt/math.c)

use crate::transform::Transform;
use crate::xpath::{self, PCtx, XObj};
use goro_ext_xml::tree::NodeId;

pub fn exists(name: &str) -> bool {
    matches!(
        name,
        "min"
            | "max"
            | "highest"
            | "lowest"
            | "constant"
            | "random"
            | "abs"
            | "sqrt"
            | "power"
            | "log"
            | "sin"
            | "cos"
            | "tan"
            | "asin"
            | "acos"
            | "atan"
            | "atan2"
            | "exp"
    )
}

fn node_num(n: NodeId) -> f64 {
    xpath::string_eval_number(&xpath::node_string_id(n))
}

fn min_max(ns: &[NodeId], max: bool) -> f64 {
    if ns.is_empty() {
        return f64::NAN;
    }
    let mut ret = node_num(ns[0]);
    if ret.is_nan() {
        return f64::NAN;
    }
    for &n in &ns[1..] {
        let cur = node_num(n);
        if cur.is_nan() {
            return f64::NAN;
        }
        if (max && cur > ret) || (!max && cur < ret) {
            ret = cur;
        }
    }
    ret
}

fn highest_lowest(ns: &[NodeId], highest: bool) -> Vec<NodeId> {
    let mut ret = Vec::new();
    if ns.is_empty() {
        return ret;
    }
    let mut best = node_num(ns[0]);
    if best.is_nan() {
        return ret;
    }
    ret.push(ns[0]);
    for &n in &ns[1..] {
        let cur = node_num(n);
        if cur.is_nan() {
            ret.clear();
            return ret;
        }
        let worse = if highest { cur < best } else { cur > best };
        if worse {
            continue;
        }
        let better = if highest { cur > best } else { cur < best };
        if better {
            best = cur;
            ret.clear();
            ret.push(n);
            continue;
        }
        ret.push(n);
    }
    ret
}

const PI: &str = "3.1415926535897932384626433832795028841971693993751";
const E: &str = "2.71828182845904523536028747135266249775724709369996";
const SQRRT2: &str = "1.41421356237309504880168872420969807856967187537694";
const LN2: &str = "0.69314718055994530941723212145817656807550013436025";
const LN10: &str = "2.30258509299404568402";
const LOG2E: &str = "1.4426950408889634074";
const SQRT1_2: &str = "0.70710678118654752440";

fn constant(name: &str, precision: f64) -> f64 {
    if precision.is_nan() || precision < 1.0 {
        return f64::NAN;
    }
    let s = match name {
        "PI" => PI,
        "E" => E,
        "SQRRT2" => SQRRT2,
        "LN2" => LN2,
        "LN10" => LN10,
        "LOG2E" => LOG2E,
        "SQRT1_2" => SQRT1_2,
        _ => return f64::NAN,
    };
    let mut len = s.len();
    if precision <= len as f64 {
        len = precision as usize;
    }
    xpath::string_eval_number(&s[..len])
}

thread_local! {
    static SEED: std::cell::Cell<u64> = const { std::cell::Cell::new(0x2545F4914F6CDD1D) };
}

fn random() -> f64 {
    SEED.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        ((x >> 33) as u32 & 0x7fff_ffff) as f64 / 2147483647.0
    })
}

pub fn call(t: &mut Transform, p: &mut PCtx, name: &str, nargs: usize) -> bool {
    match name {
        "min" | "max" | "highest" | "lowest" => {
            if nargs != 1 {
                if name == "min" {
                    t.generic_error("math:min: invalid number of arguments\n");
                    p.error = xpath::XPATH_INVALID_ARITY;
                } else {
                    xpath::x_arity_error(t, p);
                }
                return true;
            }
            let ns = match xpath::x_pop_node_set(t, p) {
                Some(v) => v,
                None => return true,
            };
            if p.error != 0 {
                return true;
            }
            match name {
                "min" => p.push(XObj::Num(min_max(&ns, false))),
                "max" => p.push(XObj::Num(min_max(&ns, true))),
                "highest" => p.push(XObj::NodeSet(highest_lowest(&ns, true))),
                _ => p.push(XObj::NodeSet(highest_lowest(&ns, false))),
            }
        }
        "constant" => {
            if nargs != 2 {
                xpath::x_arity_error(t, p);
                return true;
            }
            let prec = match xpath::x_pop_number(t, p) {
                Some(n) => n,
                None => return true,
            };
            let name = match xpath::x_pop_string(t, p) {
                Some(s) => s,
                None => return true,
            };
            p.push(XObj::Num(constant(&name, prec)));
        }
        "random" => {
            if nargs != 0 {
                xpath::x_arity_error(t, p);
                return true;
            }
            p.push(XObj::Num(random()));
        }
        "power" | "atan2" => {
            if nargs != 2 {
                xpath::x_arity_error(t, p);
                return true;
            }
            let b = match xpath::x_pop_number(t, p) {
                Some(n) => n,
                None => return true,
            };
            let a = match xpath::x_pop_number(t, p) {
                Some(n) => n,
                None => return true,
            };
            let r = if a.is_nan() || b.is_nan() {
                f64::NAN
            } else if name == "power" {
                a.powf(b)
            } else {
                a.atan2(b)
            };
            p.push(XObj::Num(r));
        }
        _ => {
            if nargs != 1 {
                xpath::x_arity_error(t, p);
                return true;
            }
            let n = match xpath::x_pop_number(t, p) {
                Some(n) => n,
                None => return true,
            };
            let r = if n.is_nan() {
                f64::NAN
            } else {
                match name {
                    "abs" => n.abs(),
                    "sqrt" => n.sqrt(),
                    "log" => n.ln(),
                    "sin" => n.sin(),
                    "cos" => n.cos(),
                    "tan" => n.tan(),
                    "asin" => n.asin(),
                    "acos" => n.acos(),
                    "atan" => n.atan(),
                    "exp" => n.exp(),
                    _ => return false,
                }
            };
            p.push(XObj::Num(r));
        }
    }
    true
}
