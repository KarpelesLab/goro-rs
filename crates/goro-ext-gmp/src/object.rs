//! The `GMP` class: operators, comparison, casts and serialization.
//!
//! Operands follow PHP 8.5's GMP operator handlers (tests
//! ext/gmp/tests/overloading*.phpt, observed on PHP 8.5):
//!
//! * `+ - * / % & | ^` and comparisons convert the other operand like a
//!   `GMP|string|int` argument in coercive mode, with messages that name no
//!   function: an integer string (any base prefix), an int, a bool (0/1),
//!   or a float (truncated, with the "Implicit conversion" deprecation when
//!   it has a fractional part). A float outside the int range, null,
//!   arrays, other objects (even Stringable ones) and resources throw
//!   `TypeError: Number must be of type GMP|string|int, X given`; a string
//!   that is no integer throws `ValueError: Number is not an integer
//!   string`.
//! * `/` truncates (`gmp_div_q()`), `%` is `gmp_mod()` (non-negative).
//! * `**`, `<<` and `>>` read their right operand as an int: a GMP value
//!   or an integer string through `gmp_intval()`'s conversion, an int, or a
//!   float cast to int (warning when it is not representable, no
//!   deprecation for a fraction); bools, null, arrays, objects and
//!   resources throw `TypeError: Unsupported operand types: GMP ** bool`.
//!   Negative values throw `ValueError: Exponent must be greater than or
//!   equal to 0` / `Shift must be greater than or equal to 0`. `>>`
//!   rounds toward minus infinity.
//! * Comparisons with null and bools are PHP's usual ones (null is smaller
//!   than any object; a bool compares with the GMP value's truthiness).

use std::borrow::Cow;
use std::cmp::Ordering;

use goro_runtime::convert::float_to_int_cast;
use goro_runtime::numeric::numeric_value;
use goro_runtime::ops::BinOp;
use goro_runtime::{Ctx, Number};
use puremp::Int;

use crate::functions::{divide, float_to_int_weak, get_si, mul, pow};
use crate::parse::parse_integer;
use crate::{GMP_ROUND_ZERO, Gmp, GmpError, GmpResult, Num, arith, check_bits};

#[cold]
fn not_integer() -> GmpError {
    GmpError::value("Number is not an integer string")
}

#[cold]
fn operand_type_error(given: &str) -> GmpError {
    GmpError::type_error(format!(
        "Number must be of type GMP|string|int, {given} given"
    ))
}

/// Converts an operand of an arithmetic, bitwise or comparison operator.
#[inline]
fn operand<'a>(ctx: &mut dyn Ctx, v: Num<'a>) -> GmpResult<Cow<'a, Int>> {
    match v {
        Num::Gmp(g) => Ok(Cow::Borrowed(&g.0)),
        Num::Int(i) => Ok(Cow::Owned(Int::from_i64(i))),
        Num::Bool(b) => Ok(Cow::Owned(Int::from_i64(b as i64))),
        Num::Str(s) => parse_integer(s, 0).map(Cow::Owned).ok_or_else(not_integer),
        Num::Float(f) => match float_to_int_weak(ctx, f) {
            Some(i) => Ok(Cow::Owned(Int::from_i64(i))),
            None => Err(operand_type_error("float")),
        },
        Num::Null | Num::Other(_) => Err(operand_type_error(v.given_name())),
    }
}

/// The right operand of `**`, `<<` and `>>`.
fn int_operand(ctx: &mut dyn Ctx, op: BinOp, a: Num, b: Num) -> GmpResult<i64> {
    match b {
        Num::Int(i) => Ok(i),
        Num::Gmp(g) => Ok(get_si(&g.0)),
        Num::Float(f) => Ok(float_to_int_cast(f, ctx)),
        // A PHP integer string ("010" is ten, no base prefixes); anything
        // else, including integers past the int range, is refused.
        Num::Str(s) => match numeric_value(s) {
            Some(Number::Int(i)) => Ok(i),
            _ => Err(not_integer()),
        },
        _ => Err(unsupported(op, a, b)),
    }
}

#[cold]
fn unsupported(op: BinOp, a: Num, b: Num) -> GmpError {
    GmpError::type_error(format!(
        "Unsupported operand types: {} {} {}",
        a.operand_name(),
        op.symbol(),
        b.operand_name()
    ))
}

/// `$a <op> $b` where at least one operand is a GMP object. `None` for
/// operators GMP does not overload (`.`): the engine applies its usual
/// semantics (string conversion).
#[inline]
pub fn binary_op(ctx: &mut dyn Ctx, op: BinOp, a: Num, b: Num) -> Option<GmpResult<Gmp>> {
    Some(match op {
        BinOp::Concat => return None,
        BinOp::Pow | BinOp::Shl | BinOp::Shr => shift_or_pow(ctx, op, a, b),
        _ => (|| {
            let x = operand(ctx, a)?;
            let y = operand(ctx, b)?;
            arith(op, &x, &y).map(Gmp)
        })(),
    })
}

#[inline]
fn arith(op: BinOp, x: &Int, y: &Int) -> GmpResult<Int> {
    Ok(match op {
        BinOp::Add => arith::add(x, y),
        BinOp::Sub => arith::sub(x, y),
        BinOp::Mul => mul(x, y)?,
        BinOp::Div => {
            if y.is_zero() {
                return Err(GmpError::div_zero("Division by zero"));
            }
            divide(x, y, GMP_ROUND_ZERO).0
        }
        BinOp::Mod => {
            if y.is_zero() {
                return Err(GmpError::div_zero("Modulo by zero"));
            }
            arith::rem_euclid(x, y)
        }
        BinOp::BitAnd => x.bitand(y),
        BinOp::BitOr => x.bitor(y),
        BinOp::BitXor => x.bitxor(y),
        _ => unreachable!("not an arithmetic operator"),
    })
}

fn shift_or_pow(ctx: &mut dyn Ctx, op: BinOp, a: Num, b: Num) -> GmpResult<Gmp> {
    // The exponent or shift count is checked first (observed on PHP 8.5:
    // `1e30 ** gmp_init(-1)` reports the negative exponent).
    let n = int_operand(ctx, op, a, b)?;
    if n < 0 {
        return Err(GmpError::value(if op == BinOp::Pow {
            "Exponent must be greater than or equal to 0"
        } else {
            "Shift must be greater than or equal to 0"
        }));
    }
    // A left operand that is not a GMP object and cannot be read as one
    // makes the handler decline, and PHP reports the operand types.
    let x = match a {
        Num::Gmp(g) => Cow::Borrowed(&g.0),
        _ => operand(ctx, a).map_err(|_| unsupported(op, a, b))?,
    };
    let n = n as u64;
    Ok(Gmp(match op {
        BinOp::Pow => pow(&x, n)?,
        BinOp::Shl => {
            if !x.is_zero() {
                check_bits(x.bit_len() as u128 + n as u128)?;
            }
            x.mul_2k(n)
        }
        _ => x.div_2k_floor(n),
    }))
}

/// `$a <=> $b` where at least one operand is a GMP object.
pub fn compare(ctx: &mut dyn Ctx, a: Num, b: Num) -> GmpResult<Ordering> {
    match (a, b) {
        (Num::Null, Num::Null) => Ok(Ordering::Equal),
        (Num::Null, _) => Ok(Ordering::Less),
        (_, Num::Null) => Ok(Ordering::Greater),
        (Num::Bool(x), y) => Ok(x.cmp(&truthy(y))),
        (x, Num::Bool(y)) => Ok(truthy(x).cmp(&y)),
        _ => {
            let x = operand(ctx, a)?;
            let y = operand(ctx, b)?;
            Ok(x.cmp(&y))
        }
    }
}

fn truthy(v: Num) -> bool {
    match v {
        Num::Gmp(g) => to_bool(g),
        Num::Int(i) => i != 0,
        Num::Float(f) => f != 0.0,
        Num::Str(s) => !(s.is_empty() || s == b"0"),
        Num::Bool(b) => b,
        Num::Null => false,
        Num::Other(_) => true,
    }
}

/// `-$a`
pub fn negate(a: &Gmp) -> Gmp {
    Gmp(a.0.neg())
}

/// `~$a`
pub fn bitwise_not(a: &Gmp) -> Gmp {
    Gmp(a.0.not())
}

/// `++$a` / `$a++`: a new GMP object holding `$a + 1`.
pub fn increment(a: &Gmp) -> Gmp {
    Gmp(arith::add(&a.0, &Int::ONE))
}

/// `--$a` / `$a--`: a new GMP object holding `$a - 1`.
pub fn decrement(a: &Gmp) -> Gmp {
    Gmp(arith::sub(&a.0, &Int::ONE))
}

/// `(int)$a` (also `gmp_intval()`): see [`get_si`].
pub fn to_int(a: &Gmp) -> i64 {
    get_si(&a.0)
}

/// `(float)$a`: truncated toward zero (GMP's `mpz_get_d`; observed on PHP
/// 8.5: `(float)gmp_init("18014398509481987")` is 18014398509481984.0),
/// infinite past the float range.
pub fn to_float(a: &Gmp) -> f64 {
    a.0.to_f64_trunc()
}

/// `(bool)$a`: false only for zero.
pub fn to_bool(a: &Gmp) -> bool {
    !a.0.is_zero()
}

/// `(string)$a`, `echo $a` and the `["num"]` entry of `var_dump()` and
/// `print_r()`: the decimal value.
pub fn to_string(a: &Gmp) -> String {
    a.to_string_radix(10)
}

/// The message of the Exception for a bad number in serialized data.
pub const UNSERIALIZE_NUMBER_ERROR: &str = "Could not unserialize number";
/// The message of the Exception for bad properties in serialized data.
pub const UNSERIALIZE_PROPERTIES_ERROR: &str = "Could not unserialize properties";

/// The number in `__serialize()`'s array: element 0 is the value in
/// lower-case hexadecimal (`"-2a"`); element 1, present only when the
/// object has dynamic properties, is the property array (test
/// ext/gmp/tests/serialize.phpt: `O:3:"GMP":1:{i:0;s:2:"2a";}`).
pub fn serialize_value(a: &Gmp) -> String {
    a.to_string_radix(16)
}

/// `__unserialize(array $data)`: reads element 0, which must be a string
/// holding an integer in base 16 (read as `gmp_init($data[0], 16)` reads
/// it: a `0x` prefix and white space are accepted). The engine passes
/// [`Num::Null`] when element 0 is missing; anything else fails with
/// `Exception: Could not unserialize number`. When element 1 exists it
/// must be an array of properties to add, else the engine throws
/// `Exception` with [`UNSERIALIZE_PROPERTIES_ERROR`]; further elements are
/// ignored.
pub fn unserialize_value(data0: Num) -> GmpResult<Gmp> {
    match data0 {
        Num::Str(s) => parse_integer(s, 16)
            .map(Gmp)
            .ok_or_else(|| GmpError::exception(UNSERIALIZE_NUMBER_ERROR)),
        _ => Err(GmpError::exception(UNSERIALIZE_NUMBER_ERROR)),
    }
}

/// The legacy `C:3:"GMP":N:{...}` form (PHP < 8.1 `Serializable`): the
/// payload is a serialized decimal string followed by a serialized
/// property array (`C:3:"GMP":15:{s:2:"42";a:0:{}}`). The engine
/// unserializes the first value and passes it here; a missing or
/// non-array second value throws `Exception` with
/// [`UNSERIALIZE_PROPERTIES_ERROR`] (test ext/gmp/tests/serialize.phpt).
pub fn unserialize_legacy_value(value: Num) -> GmpResult<Gmp> {
    match value {
        Num::Str(s) => parse_integer(s, 10)
            .map(Gmp)
            .ok_or_else(|| GmpError::exception(UNSERIALIZE_NUMBER_ERROR)),
        _ => Err(GmpError::exception(UNSERIALIZE_NUMBER_ERROR)),
    }
}
