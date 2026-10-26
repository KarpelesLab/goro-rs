//! RECV of typed parameters whose arguments already have the declared
//! types.
//!
//! Checking a call's typed parameters (Vm::recv_typed_params) resolves the
//! method's scope, builds the function's display name, and walks the
//! arguments several times for the checks, the deprecations and the
//! coercions of weak mode. When every argument a typed parameter receives
//! is already of its type as is (an int for `int`, a string for `?string`,
//! an object of the very class named...), none of that has anything to
//! do: the check passes, nothing is coerced or reported.

use crate::opcode::{OpArray, ParamType};
use crate::value::Value;
use crate::vm::Vm;

/// Whether every argument of `args` a typed parameter of `f` receives is
/// exactly of the declared type (see the module's doc). False: the full
/// RECV checks run (they may still pass).
#[inline]
pub(crate) fn args_match_exactly(vm: &Vm, f: &OpArray, args: &[Value]) -> bool {
    if f.variadic_param.is_some() {
        return false;
    }
    for (i, a) in args.iter().enumerate() {
        let Some(t) = f.param_types.get(i) else {
            break;
        };
        let Some(t) = t else {
            continue;
        };
        // (a parameter not passed gets its default later)
        if matches!(a, Value::Undef) {
            continue;
        }
        if !value_is_exactly(vm, a, &t.param_type) {
            return false;
        }
    }
    true
}

/// args_match_exactly() of the argument `v` parameter `i` of `f` (its CV
/// index) receives.
#[inline]
pub(crate) fn arg_matches_exactly(vm: &Vm, f: &OpArray, i: usize, v: &Value) -> bool {
    match f.param_types.get(i) {
        Some(Some(t)) => matches!(v, Value::Undef) || value_is_exactly(vm, v, &t.param_type),
        _ => true,
    }
}

/// Whether `v`, returned by `f` (`implicit`: by falling off its end), is
/// exactly of the declared return type: VERIFY_RETURN_TYPE would neither
/// coerce nor report anything.
#[inline]
pub(crate) fn return_matches_exactly(vm: &Vm, f: &OpArray, v: &Value, implicit: bool) -> bool {
    let Some(t) = &f.return_type else {
        return true;
    };
    match t {
        ParamType::Simple(n) if n.eq_ignore_ascii_case(b"void") => {
            matches!(v, Value::Null | Value::Undef)
        }
        // ("none returned" for any other type)
        _ if implicit => false,
        t => value_is_exactly(vm, v, t),
    }
}

fn value_is_exactly(vm: &Vm, v: &Value, t: &ParamType) -> bool {
    match t {
        ParamType::Simple(n) => simple_exactly(vm, v, n),
        ParamType::Nullable(inner) => matches!(v, Value::Null) || value_is_exactly(vm, v, inner),
        ParamType::Union(ts) => ts.iter().any(|t| value_is_exactly(vm, v, t)),
        ParamType::Intersection(_) => false,
    }
}

fn simple_exactly(vm: &Vm, v: &Value, n: &[u8]) -> bool {
    let n = n.strip_prefix(b"\\").unwrap_or(n);
    match n.len() {
        3 if n.eq_ignore_ascii_case(b"int") => matches!(v, Value::Long(_)),
        5 if n.eq_ignore_ascii_case(b"float") => matches!(v, Value::Double(_)),
        6 if n.eq_ignore_ascii_case(b"string") => matches!(v, Value::String(_)),
        4 if n.eq_ignore_ascii_case(b"bool") => matches!(v, Value::True | Value::False),
        5 if n.eq_ignore_ascii_case(b"array") => matches!(v, Value::Array(_)),
        5 if n.eq_ignore_ascii_case(b"mixed") => !matches!(v, Value::Reference(_)),
        4 if n.eq_ignore_ascii_case(b"null") => matches!(v, Value::Null),
        4 if n.eq_ignore_ascii_case(b"true") => matches!(v, Value::True),
        5 if n.eq_ignore_ascii_case(b"false") => matches!(v, Value::False),
        6 if n.eq_ignore_ascii_case(b"object") => matches!(v, Value::Object(_)),
        8 if n.eq_ignore_ascii_case(b"iterable") => matches!(v, Value::Array(_)),
        _ => match v {
            // An object of the very class the type names (no lookup, no
            // autoload: the class is the object's), or of a declared
            // class extending it or implementing it (loaded classes only).
            Value::Object(o) => {
                !is_reserved_type(n)
                    && o.try_borrow().is_ok_and(|b| {
                        b.class_name.eq_ignore_ascii_case(n)
                            || (b.native.is_none() && class_is_a(vm, &b.class_name, n))
                    })
            }
            _ => false,
        },
    }
}

/// Whether the declared class `class` extends or implements the declared
/// class or interface `n` (as instanceof answers: no autoload).
#[inline(never)]
fn class_is_a(vm: &Vm, class: &[u8], n: &[u8]) -> bool {
    let n = n.to_ascii_lowercase();
    let Some(target) = vm.classes.get(&n) else {
        return false;
    };
    // (a class_alias() names the aliased class)
    let n = target.name.to_ascii_lowercase();
    let mut cur = class.to_ascii_lowercase();
    if target.is_interface {
        return vm.classes.contains_key(&cur) && vm.class_implements_interface(&cur, &n);
    }
    for _ in 0..64 {
        let Some(ce) = vm.classes.get(&cur) else {
            return false;
        };
        let Some(p) = &ce.parent else {
            return false;
        };
        if p.eq_ignore_ascii_case(&n) {
            return true;
        }
        cur = p.to_ascii_lowercase();
    }
    false
}

/// The type names that are not class names.
fn is_reserved_type(n: &[u8]) -> bool {
    const R: [&[u8]; 18] = [
        b"int",
        b"float",
        b"string",
        b"bool",
        b"array",
        b"mixed",
        b"null",
        b"true",
        b"false",
        b"object",
        b"iterable",
        b"callable",
        b"void",
        b"never",
        b"self",
        b"parent",
        b"static",
        b"closure",
    ];
    R.iter().any(|r| n.eq_ignore_ascii_case(r))
}

impl crate::vm::Vm {
    /// RECV of `f`'s typed parameters has nothing to check, coerce or
    /// report for `args` (see args_match_exactly), and no deprecation a
    /// callable check queued waits for it to be reported.
    #[inline]
    pub(crate) fn recv_has_nothing_to_do(&self, f: &OpArray, args: &[Value]) -> bool {
        self.pending_callable_deprecations.borrow().is_empty() && args_match_exactly(self, f, args)
    }
}
