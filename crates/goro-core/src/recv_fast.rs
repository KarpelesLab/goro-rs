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
    f.variadic_param.is_none() && positional_args_match_exactly(vm, f, args)
}

/// args_match_exactly() of a call with positional arguments only: those a
/// variadic parameter collects too (of its type).
#[inline]
pub(crate) fn positional_args_match_exactly(vm: &Vm, f: &OpArray, args: &[Value]) -> bool {
    let variadic = f.variadic_param.map_or(usize::MAX, |v| v as usize);
    let masks = f.type_kind_masks();
    for (i, a) in args.iter().enumerate() {
        let i = i.min(variadic);
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
        let ok = match masks.get(i) {
            Some(&m) if m != 0 => m & kind_bit(a) != 0,
            _ => value_is_exactly(vm, a, &t.param_type),
        };
        if !ok {
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
        Some(Some(t)) => {
            matches!(v, Value::Undef)
                || match f.type_kind_masks().get(i) {
                    Some(&m) if m != 0 => m & kind_bit(v) != 0,
                    _ => value_is_exactly(vm, v, &t.param_type),
                }
        }
        _ => true,
    }
}

/// The kind of `v` among those type_kind_masks() names: null 1, true 2,
/// false 4, int 8, float 16, string 32, array 64 (else 0: no such mask
/// takes it).
#[inline(always)]
fn kind_bit(v: &Value) -> u16 {
    match v {
        Value::Null => 1,
        Value::True => 2,
        Value::False => 4,
        Value::Long(_) => 8,
        Value::Double(_) => 16,
        Value::String(_) => 32,
        Value::Array(_) => 64,
        _ => 0,
    }
}

/// OpArray::type_kind_masks() of `f`.
pub(crate) fn type_kind_masks_of(f: &OpArray) -> Box<[u16]> {
    f.param_types
        .iter()
        .map(|t| t.as_ref().map_or(0, |t| type_kind_mask(&t.param_type)))
        .chain(std::iter::once(f.return_type.as_ref().map_or(0, type_kind_mask)))
        .collect()
}

/// The value kinds (see kind_bit) `t` takes as they are, as
/// value_is_exactly() judges them; 0 when it names a class or anything
/// else.
fn type_kind_mask(t: &ParamType) -> u16 {
    match t {
        ParamType::Nullable(inner) => match type_kind_mask(inner) {
            0 => 0,
            m => m | 1,
        },
        ParamType::Union(members) => {
            let mut m = 0;
            for t in members {
                match type_kind_mask(t) {
                    0 => return 0,
                    k => m |= k,
                }
            }
            m
        }
        ParamType::Simple(n) => {
            let n = n.strip_prefix(b"\\").unwrap_or(n);
            match n.len() {
                3 if n.eq_ignore_ascii_case(b"int") => 8,
                5 if n.eq_ignore_ascii_case(b"float") => 16,
                6 if n.eq_ignore_ascii_case(b"string") => 32,
                4 if n.eq_ignore_ascii_case(b"bool") => 6,
                5 if n.eq_ignore_ascii_case(b"array") => 64,
                4 if n.eq_ignore_ascii_case(b"null") => 1,
                4 if n.eq_ignore_ascii_case(b"true") => 2,
                5 if n.eq_ignore_ascii_case(b"false") => 4,
                _ => 0,
            }
        }
        ParamType::Intersection(_) => 0,
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
        t => match f.type_kind_masks().last() {
            Some(&m) if m != 0 => m & kind_bit(v) != 0,
            _ => value_is_exactly(vm, v, t),
        },
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
/// (Remembered while the classes in the table do not change: the answer
/// is a function of them when `n` names one.)
#[inline(never)]
fn class_is_a(vm: &Vm, class: &[u8], n: &[u8]) -> bool {
    let stamp = vm.classes.link_generation();
    if let Some(r) = vm.isa_memo.borrow_mut().get(stamp, class, n) {
        return r;
    }
    let lower = n.to_ascii_lowercase();
    if !vm.classes.contains_key(&lower) {
        return false;
    }
    let r = class_is_a_walk(vm, class, &lower);
    vm.isa_memo.borrow_mut().set(stamp, class, n, r);
    r
}

/// class_is_a() of the class `n` (lowercase) in the table.
fn class_is_a_walk(vm: &Vm, class: &[u8], n: &[u8]) -> bool {
    let Some(target) = vm.classes.get(n) else {
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

    /// recv_has_nothing_to_do() of a call without named arguments (a
    /// variadic parameter's arguments checked too).
    #[inline]
    pub(crate) fn recv_positional_nothing_to_do(&self, f: &OpArray, args: &[Value]) -> bool {
        self.pending_callable_deprecations.borrow().is_empty()
            && positional_args_match_exactly(self, f, args)
    }
}
