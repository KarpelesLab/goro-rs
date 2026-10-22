//! libexslt: EXSLT extension functions and elements.

use crate::transform::Transform;
use crate::xpath::{self, PCtx, XObj, XPATH_INVALID_TYPE};

pub mod common;
pub mod crypto;
pub mod date;
pub mod dynamic;
pub mod func;
pub mod math;
pub mod sets;
pub mod strings;

pub const EXSLT_MATH_NAMESPACE: &str = "http://exslt.org/math";
pub const EXSLT_SETS_NAMESPACE: &str = "http://exslt.org/sets";
pub const EXSLT_STRINGS_NAMESPACE: &str = "http://exslt.org/strings";
pub const EXSLT_DATE_NAMESPACE: &str = "http://exslt.org/dates-and-times";
pub const EXSLT_DYNAMIC_NAMESPACE: &str = "http://exslt.org/dynamic";
pub const EXSLT_CRYPTO_NAMESPACE: &str = "http://exslt.org/crypto";
pub const SAXON_NAMESPACE: &str = "http://icl.com/saxon";

pub use func::{func_function_call, func_function_comp, func_result_elem};

/// Call an EXSLT module function. False when unknown.
pub fn call(t: &mut Transform, p: &mut PCtx, name: &str, uri: &str, nargs: usize) -> bool {
    match uri {
        crate::style::EXSLT_COMMON_NAMESPACE => common::call(t, p, name, nargs),
        EXSLT_MATH_NAMESPACE => math::call(t, p, name, nargs),
        EXSLT_SETS_NAMESPACE => sets::call(t, p, name, nargs),
        EXSLT_STRINGS_NAMESPACE => strings::call(t, p, name, nargs),
        EXSLT_DATE_NAMESPACE => date::call(t, p, name, nargs),
        EXSLT_DYNAMIC_NAMESPACE => dynamic::call(t, p, name, nargs),
        EXSLT_CRYPTO_NAMESPACE => crypto::call(t, p, name, nargs),
        SAXON_NAMESPACE => dynamic::saxon_call(t, p, name, nargs),
        _ => false,
    }
}

pub fn exists(name: &str, uri: &str) -> bool {
    match uri {
        crate::style::EXSLT_COMMON_NAMESPACE => matches!(name, "node-set" | "object-type"),
        EXSLT_MATH_NAMESPACE => math::exists(name),
        EXSLT_SETS_NAMESPACE => sets::exists(name),
        EXSLT_STRINGS_NAMESPACE => strings::exists(name),
        EXSLT_DATE_NAMESPACE => date::exists(name),
        EXSLT_DYNAMIC_NAMESPACE => matches!(name, "evaluate" | "map"),
        EXSLT_CRYPTO_NAMESPACE => crypto::exists(name),
        SAXON_NAMESPACE => matches!(
            name,
            "expression" | "eval" | "evaluate" | "line-number" | "systemId"
        ),
        _ => false,
    }
}

/// xsltFunctionNodeSet: libxslt's node-set() in its own namespaces
pub fn node_set_function(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    if nargs != 1 {
        t.error(None, "node-set() : expects one result-tree arg\n");
        p.error = xpath::XPATH_INVALID_ARITY;
        return;
    }
    match p.top_mut() {
        Some(top @ XObj::Tree(_)) => {
            if let XObj::Tree(v) = top {
                *top = XObj::NodeSet(std::mem::take(v));
            }
        }
        Some(XObj::NodeSet(_)) => {}
        _ => {
            t.error(None, "node-set() invalid arg expecting a result tree\n");
            p.error = XPATH_INVALID_TYPE;
        }
    }
}
