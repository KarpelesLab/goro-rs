//! Comparison (PHP manual: Comparison Operators, "Comparison with Various
//! Types"; RFC "Saner string to number comparisons").
//!
//! [`compare`] implements `<=>` and returns -1, 0 or 1. Pairs PHP calls
//! "uncomparable" (arrays with different keys, NAN) return 1, and the
//! ordering operators are derived as PHP derives them:
//! `$a < $b` is `compare(a, b) < 0` and `$a > $b` is `compare(b, a) < 0`,
//! so an uncomparable pair is neither smaller nor greater.

use std::cmp::Ordering;

use crate::array::PhpArray;
use crate::convert::{float_to_string, int_to_string, to_bool_quiet};
use crate::ctx::Ctx;
use crate::error::{PhpError, PhpResult};
use crate::numeric::{Number, numeric_value};
use crate::value::Value;

#[inline]
fn ord(o: Ordering) -> i32 {
    match o {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

#[inline]
fn cmp_bool(a: bool, b: bool) -> i32 {
    a as i32 - b as i32
}

/// Float comparison: NAN is uncomparable (1).
#[inline]
fn cmp_f64(a: f64, b: f64) -> i32 {
    match a.partial_cmp(&b) {
        Some(o) => ord(o),
        None => 1,
    }
}

#[inline]
fn cmp_int_float(a: i64, b: f64) -> i32 {
    cmp_f64(a as f64, b)
}

fn cmp_num(a: Number, b: Number) -> i32 {
    match (a, b) {
        (Number::Int(x), Number::Int(y)) => ord(x.cmp(&y)),
        (Number::Int(x), Number::Float(y)) => cmp_int_float(x, y),
        (Number::Float(x), Number::Int(y)) => cmp_f64(x, y as f64),
        (Number::Float(x), Number::Float(y)) => cmp_f64(x, y),
    }
}

/// Byte-wise string comparison (`strcmp`, normalised).
#[inline]
pub fn cmp_bytes(a: &[u8], b: &[u8]) -> i32 {
    ord(a.cmp(b))
}

/// Comparison of two strings: numerically if both are numeric strings,
/// otherwise byte-wise.
pub fn cmp_strings(a: &[u8], b: &[u8]) -> i32 {
    if let (Some(x), Some(y)) = (numeric_value(a), numeric_value(b)) {
        // Numeric strings whose values are equal only because of float
        // overflow compare as strings, so distinct big integers stay
        // distinct (observed on PHP 8.5: "9223372036854775807" <
        // "9223372036854775808", "1e1000" != "1e1001").
        if x.to_f64() == y.to_f64() {
            let int_overflow =
                |n: Number, s: &[u8]| matches!(n, Number::Float(_)) && is_int_syntax(s);
            let both_int_syntax = is_int_syntax(a) && is_int_syntax(b);
            if (both_int_syntax && (int_overflow(x, a) || int_overflow(y, b)))
                || x.to_f64().is_infinite()
            {
                return cmp_bytes(a, b);
            }
        }
        return cmp_num(x, y);
    }
    cmp_bytes(a, b)
}

fn is_int_syntax(s: &[u8]) -> bool {
    !s.iter().any(|&c| c == b'.' || c == b'e' || c == b'E')
}

/// Loose equality of two strings.
pub fn eq_strings(a: &[u8], b: &[u8]) -> bool {
    if a == b {
        return true;
    }
    // Only numeric strings can be equal without being identical; they
    // start (after whitespace) with a digit, sign or '.'.
    let starts_numeric = |s: &[u8]| {
        s.iter()
            .find(|c| !crate::numeric::is_php_whitespace(**c))
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.'))
    };
    if !starts_numeric(a) || !starts_numeric(b) {
        return false;
    }
    cmp_strings(a, b) == 0
}

/// Number vs string ("Saner string to number comparisons" RFC): numeric
/// strings compare numerically, others compare with the number cast to a
/// string.
fn cmp_num_str(n: Number, s: &[u8], ctx: &mut dyn Ctx) -> i32 {
    if let Some(m) = numeric_value(s) {
        return cmp_num(n, m);
    }
    let ns = match n {
        Number::Int(i) => int_to_string(i),
        Number::Float(f) => float_to_string(f, ctx.precision()),
    };
    cmp_bytes(&ns, s)
}

/// `$a <=> $b`.
#[inline]
pub fn compare(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<i32> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(ord(x.cmp(y))),
        (Value::Float(x), Value::Float(y)) => Ok(cmp_f64(*x, *y)),
        _ => compare_slow(a, b, ctx),
    }
}

#[inline(never)]
fn compare_slow(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<i32> {
    use Value::*;
    Ok(match (a, b) {
        (Int(x), Int(y)) => ord(x.cmp(y)),
        (Float(x), Float(y)) => cmp_f64(*x, *y),
        (Int(x), Float(y)) => cmp_int_float(*x, *y),
        (Float(x), Int(y)) => cmp_f64(*x, *y as f64),
        (Str(x), Str(y)) => {
            if crate::string::PhpStr::ptr_eq(x, y) {
                0
            } else {
                cmp_strings(x, y)
            }
        }
        (Ref(r), _) => {
            let v = r.get();
            return compare(&v, b, ctx);
        }
        (_, Ref(r)) => {
            let v = r.get();
            return compare(a, &v, ctx);
        }
        (Null, Null) => 0,
        (Bool(x), _) => cmp_bool(*x, to_bool_quiet(b)),
        (_, Bool(y)) => cmp_bool(to_bool_quiet(a), *y),
        (Null, Str(s)) => cmp_bytes(b"", s),
        (Str(s), Null) => cmp_bytes(s, b""),
        (Null, _) => cmp_bool(false, to_bool_quiet(b)),
        (_, Null) => cmp_bool(to_bool_quiet(a), false),
        (Object(_), _) | (_, Object(_)) => {
            return match ctx.object_compare(a, b) {
                Some(r) => r,
                None => default_object_compare(a, b, ctx),
            };
        }
        // NAN is uncomparable with any string (observed on PHP 8.5).
        (Float(x), Str(_)) | (Str(_), Float(x)) if x.is_nan() => 1,
        (Int(x), Str(s)) => cmp_num_str(Number::Int(*x), s, ctx),
        (Float(x), Str(s)) => cmp_num_str(Number::Float(*x), s, ctx),
        (Str(s), Int(y)) => -cmp_num_str(Number::Int(*y), s, ctx),
        (Str(s), Float(y)) => -cmp_num_str(Number::Float(*y), s, ctx),
        (Array(x), Array(y)) => return compare_arrays(x, y, ctx),
        (Array(_), _) => 1,
        (_, Array(_)) => -1,
        (Resource(x), Resource(y)) => ord(x.id().cmp(&y.id())),
        (Resource(x), _) => {
            return compare(&Int(x.id()), b, ctx);
        }
        (_, Resource(y)) => {
            return compare(a, &Int(y.id()), ctx);
        }
    })
}

/// PHP's comparison for objects without comparison handlers: an object
/// equals itself; objects of the same class compare property by property;
/// compared with an int or float the object converts to 1 with a notice
/// (observed on PHP 8.5: "Object of class stdClass could not be converted to
/// int"); anything else is uncomparable.
pub fn default_object_compare(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<i32> {
    use Value::*;
    let num_notice = |o: &crate::object::ObjRef, float: bool, ctx: &mut dyn Ctx| {
        ctx.notice(
            crate::error::Level::Notice,
            format!(
                "Object of class {} could not be converted to {}",
                o.class_name().to_string_lossy(),
                if float { "float" } else { "int" }
            ),
        );
    };
    match (a, b) {
        (Object(x), Object(y)) => {
            if crate::object::ObjRef::ptr_eq(x, y) {
                return Ok(0);
            }
            if !std::rc::Rc::ptr_eq(x.class(), y.class()) {
                return Ok(1);
            }
            let (sx, sy) = (
                x.obj().slots.borrow().clone(),
                y.obj().slots.borrow().clone(),
            );
            for (u, v) in sx.iter().zip(sy.iter()) {
                let c = compare(u, v, ctx)?;
                if c != 0 {
                    return Ok(c);
                }
            }
            let (px, py) = (
                x.obj().props.borrow().clone(),
                y.obj().props.borrow().clone(),
            );
            compare_arrays(&px, &py, ctx)
        }
        (Object(o), Int(i)) => {
            num_notice(o, false, ctx);
            Ok(ord(1.cmp(i)))
        }
        (Int(i), Object(o)) => {
            num_notice(o, false, ctx);
            Ok(ord(i.cmp(&1)))
        }
        (Object(o), Float(f)) => {
            num_notice(o, true, ctx);
            Ok(cmp_f64(1.0, *f))
        }
        (Float(f), Object(o)) => {
            num_notice(o, true, ctx);
            Ok(cmp_f64(*f, 1.0))
        }
        // An object is greater than an uncomparable non-object (observed
        // on PHP 8.5: [] <=> new stdClass is -1, "s" <=> new stdClass -1).
        (Object(_), _) => Ok(1),
        _ => Ok(-1),
    }
}

/// Array `<=>`: fewer elements is smaller; otherwise compared element by
/// element in the order of `a`; a key of `a` missing from `b` makes them
/// uncomparable (PHP manual, Comparison Operators: "Array with fewer
/// members is smaller, if key from operand 1 is not found in operand 2 then
/// arrays are incomparable, otherwise - compare value by value").
pub fn compare_arrays(a: &PhpArray, b: &PhpArray, ctx: &mut dyn Ctx) -> PhpResult<i32> {
    if PhpArray::ptr_eq(a, b) {
        return Ok(0);
    }
    if a.len() != b.len() {
        return Ok(ord(a.len().cmp(&b.len())));
    }
    let _g = NestGuard::enter(a)?;
    for (k, v) in a.iter() {
        let Some(w) = b.get(k) else {
            return Ok(1);
        };
        let c = compare(v, w, ctx)?;
        if c != 0 {
            return Ok(c);
        }
    }
    Ok(0)
}

thread_local! {
    /// Arrays whose elements are being compared.
    static COMPARING: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Recursion protection of array comparisons: comparing an array that
/// contains itself (through a reference) throws "Nesting level too deep -
/// recursive dependency?" (observed on PHP 8.5).
struct NestGuard(usize);

impl NestGuard {
    fn enter(a: &PhpArray) -> PhpResult<NestGuard> {
        let addr = a.data_addr();
        if addr != 0 {
            let seen = COMPARING.with(|c| {
                let mut c = c.borrow_mut();
                if c.contains(&addr) {
                    true
                } else {
                    c.push(addr);
                    false
                }
            });
            if seen {
                return Err(PhpError::error(
                    "Nesting level too deep - recursive dependency?",
                ));
            }
        }
        Ok(NestGuard(addr))
    }
}

impl Drop for NestGuard {
    fn drop(&mut self) {
        if self.0 != 0 {
            COMPARING.with(|c| {
                let mut c = c.borrow_mut();
                if let Some(i) = c.iter().rposition(|&x| x == self.0) {
                    c.remove(i);
                }
            });
        }
    }
}

/// `$a == $b`.
#[inline]
pub fn loose_eq(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<bool> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x == y),
        (Value::Float(x), Value::Float(y)) => Ok(x == y),
        _ => loose_eq_slow(a, b, ctx),
    }
}

#[inline(never)]
fn loose_eq_slow(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<bool> {
    use Value::*;
    Ok(match (a, b) {
        (Int(x), Int(y)) => x == y,
        (Float(x), Float(y)) => x == y,
        (Int(x), Float(y)) => (*x as f64) == *y,
        (Float(x), Int(y)) => *x == (*y as f64),
        (Str(x), Str(y)) => eq_strings(x, y),
        (Null, Null) => true,
        (Ref(r), _) => {
            let v = r.get();
            return loose_eq(&v, b, ctx);
        }
        (_, Ref(r)) => {
            let v = r.get();
            return loose_eq(a, &v, ctx);
        }
        (Array(x), Array(y)) => return loose_eq_arrays(x, y, ctx),
        (Object(_), _) | (_, Object(_)) => {
            if matches!(a, Bool(_)) || matches!(b, Bool(_)) {
                return Ok(to_bool_quiet(a) == to_bool_quiet(b));
            }
            if matches!(a, Null) || matches!(b, Null) {
                return Ok(false);
            }
            if let Some(r) = ctx.object_loose_eq(a, b) {
                return r;
            }
            return Ok(compare(a, b, ctx)? == 0);
        }
        _ => compare(a, b, ctx)? == 0,
    })
}

/// Array `==`: the same key/value pairs, compared loosely, in any order.
pub fn loose_eq_arrays(a: &PhpArray, b: &PhpArray, ctx: &mut dyn Ctx) -> PhpResult<bool> {
    if PhpArray::ptr_eq(a, b) {
        return Ok(true);
    }
    if a.len() != b.len() {
        return Ok(false);
    }
    let _g = NestGuard::enter(a)?;
    for (k, v) in a.iter() {
        let Some(w) = b.get(k) else {
            return Ok(false);
        };
        if !loose_eq(v, w, ctx)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `$a === $b`.
pub fn strict_eq(a: &Value, b: &Value) -> bool {
    use Value::*;
    match (a, b) {
        (Null, Null) => true,
        (Bool(x), Bool(y)) => x == y,
        (Int(x), Int(y)) => x == y,
        (Float(x), Float(y)) => x == y,
        (Str(x), Str(y)) => x == y,
        (Array(x), Array(y)) => strict_eq_arrays(x, y),
        (Object(x), Object(y)) => crate::object::ObjRef::ptr_eq(x, y),
        (Resource(x), Resource(y)) => crate::object::ResRef::ptr_eq(x, y),
        (Ref(r), _) => strict_eq(&r.borrow(), b),
        (_, Ref(r)) => strict_eq(a, &r.borrow()),
        _ => false,
    }
}

/// Array `===`: the same key/value pairs in the same order with identical
/// values (PHP manual, Array Operators).
pub fn strict_eq_arrays(a: &PhpArray, b: &PhpArray) -> bool {
    if PhpArray::ptr_eq(a, b) {
        return true;
    }
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|((ka, va), (kb, vb))| ka == kb && strict_eq(va, vb))
}

/// `$a < $b`.
pub fn less(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<bool> {
    Ok(compare(a, b, ctx)? < 0)
}

/// `$a <= $b`.
pub fn less_eq(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<bool> {
    Ok(compare(a, b, ctx)? <= 0)
}

/// `$a > $b` (evaluated as `$b < $a`).
pub fn greater(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<bool> {
    Ok(compare(b, a, ctx)? < 0)
}

/// `$a >= $b` (evaluated as `$b <= $a`).
pub fn greater_eq(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<bool> {
    Ok(compare(b, a, ctx)? <= 0)
}
