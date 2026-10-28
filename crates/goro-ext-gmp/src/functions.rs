//! The `gmp_*()` functions and `GMP::__construct()`.
//!
//! Every function takes the [`Env`] of the call, its `GMP|string|int`
//! parameters as [`Num`] and its `int`/`bool` parameters already coerced
//! by the engine (see the crate documentation). Arguments are checked in
//! PHP's order: the `GMP|string|int` arguments are converted left to right
//! (type errors, deprecations, "is not an integer string"), then each
//! function checks its values (observed on PHP 8.5, e.g. `gmp_div_r(7, 0,
//! 5)` reports the division by zero before the bad rounding mode).

use std::borrow::Cow;

use goro_runtime::Level;
use goro_runtime::convert::float_repr;
use puremp::Int;

use crate::parse::{parse_integer, to_string_radix};
use crate::{
    Env, GMP_BIG_ENDIAN, GMP_LITTLE_ENDIAN, GMP_LSW_FIRST, GMP_MSW_FIRST, GMP_NATIVE_ENDIAN,
    GMP_ROUND_MINUSINF, GMP_ROUND_PLUSINF, GMP_ROUND_ZERO, Gmp, GmpError, GmpResult, Num, arith,
    check_bits, random, small,
};

// ------------------------------------------------------------- arguments

/// The float → int conversion of weak-mode integer parameters (PHP manual,
/// "Type juggling"; the PHP 8.1 deprecation of fractional floats): `None`
/// when the float is not finite or outside the int range (a TypeError).
pub(crate) fn float_to_int_weak(ctx: &mut dyn goro_runtime::Ctx, f: f64) -> Option<i64> {
    const TWO63: f64 = 9_223_372_036_854_775_808.0;
    if !(f.is_finite() && (-TWO63..TWO63).contains(&f)) {
        return None;
    }
    let i = f as i64;
    if i as f64 != f {
        ctx.notice(
            Level::Deprecated,
            format!(
                "Implicit conversion from float {} to int loses precision",
                float_repr(f)
            ),
        );
    }
    Some(i)
}

#[cold]
fn not_integer(func: &str, argno: u32, name: &str) -> GmpError {
    GmpError::value(format!(
        "{func}(): Argument #{argno} (${name}) is not an integer string"
    ))
}

#[cold]
fn arg_type_error(func: &str, argno: u32, name: &str, types: &str, given: &str) -> GmpError {
    GmpError::type_error(format!(
        "{func}(): Argument #{argno} (${name}) must be of type {types}, {given} given"
    ))
}

#[cold]
fn arg_value_error(func: &str, argno: u32, name: &str, what: &str) -> GmpError {
    GmpError::value(format!("{func}(): Argument #{argno} (${name}) {what}"))
}

/// The TypeError for a non-GMP `$num` of `gmp_setbit()`/`gmp_clrbit()`
/// (`given` is the type name, e.g. `"string"`, `"array"`).
pub fn gmp_object_type_error(func: &str, given: &str) -> GmpError {
    arg_type_error(func, 1, "num", "GMP", given)
}

/// Converts a `GMP|string|int` argument.
#[inline]
pub(crate) fn arg<'a>(
    env: &mut Env,
    func: &str,
    argno: u32,
    name: &str,
    v: Num<'a>,
) -> GmpResult<Cow<'a, Int>> {
    match v {
        Num::Gmp(g) => Ok(Cow::Borrowed(&g.0)),
        Num::Int(i) => Ok(Cow::Owned(Int::from_i64(i))),
        Num::Str(s) => match parse_integer(s, 0) {
            Some(n) => Ok(Cow::Owned(n)),
            None => Err(not_integer(func, argno, name)),
        },
        Num::Float(f) if !env.strict => match float_to_int_weak(env.ctx, f) {
            Some(i) => Ok(Cow::Owned(Int::from_i64(i))),
            None => Err(arg_type_error(func, argno, name, "GMP|string|int", "float")),
        },
        Num::Bool(b) if !env.strict => Ok(Cow::Owned(Int::from_i64(b as i64))),
        Num::Null if !env.strict => {
            env.ctx.notice(
                Level::Deprecated,
                format!(
                    "{func}(): Passing null to parameter #{argno} (${name}) of type GMP|string|int is deprecated"
                ),
            );
            Ok(Cow::Owned(Int::ZERO))
        }
        _ => Err(arg_type_error(
            func,
            argno,
            name,
            "GMP|string|int",
            v.given_name(),
        )),
    }
}

/// `string|int $num` of `gmp_init()`/`GMP::__construct()`.
enum IntOrStr<'a> {
    Int(i64),
    Str(Cow<'a, [u8]>),
}

fn int_or_str<'a>(env: &mut Env, func: &str, v: Num<'a>) -> GmpResult<IntOrStr<'a>> {
    match v {
        Num::Int(i) => Ok(IntOrStr::Int(i)),
        Num::Str(s) => Ok(IntOrStr::Str(Cow::Borrowed(s))),
        Num::Gmp(g) if !env.strict => Ok(IntOrStr::Str(Cow::Owned(g.to_string().into_bytes()))),
        Num::Float(f) if !env.strict => match float_to_int_weak(env.ctx, f) {
            Some(i) => Ok(IntOrStr::Int(i)),
            // A float that is no int is passed as its string form.
            None if f.is_nan() => {
                env.ctx.notice(
                    Level::Warning,
                    "unexpected NAN value was coerced to string".into(),
                );
                Ok(IntOrStr::Str(Cow::Borrowed(b"NAN")))
            }
            None => Ok(IntOrStr::Str(Cow::Owned(
                goro_runtime::float_fmt::to_php_string(f, 14).into_bytes(),
            ))),
        },
        Num::Bool(b) if !env.strict => Ok(IntOrStr::Int(b as i64)),
        Num::Null if !env.strict => {
            env.ctx.notice(
                Level::Deprecated,
                format!(
                    "{func}(): Passing null to parameter #1 ($num) of type string|int is deprecated"
                ),
            );
            Ok(IntOrStr::Int(0))
        }
        _ => Err(arg_type_error(func, 1, "num", "string|int", v.given_name())),
    }
}

fn check_rounding(func: &str, mode: i64) -> GmpResult<()> {
    if matches!(
        mode,
        GMP_ROUND_ZERO | GMP_ROUND_PLUSINF | GMP_ROUND_MINUSINF
    ) {
        Ok(())
    } else {
        Err(arg_value_error(
            func,
            3,
            "rounding_mode",
            "must be one of GMP_ROUND_ZERO, GMP_ROUND_PLUSINF, or GMP_ROUND_MINUSINF",
        ))
    }
}

/// The largest bit index `gmp_setbit()` and friends accept (observed on
/// PHP 8.5: "must be between 0 and 2147483647 * 64").
pub const MAX_BIT_INDEX: i64 = 2_147_483_647 * 64;

fn check_index(func: &str, name: &str, index: i64) -> GmpResult<u64> {
    if (0..MAX_BIT_INDEX).contains(&index) {
        Ok(index as u64)
    } else {
        Err(arg_value_error(
            func,
            2,
            name,
            "must be between 0 and 2147483647 * 64",
        ))
    }
}

#[inline]
fn bits(n: &Int) -> u128 {
    n.bit_len() as u128
}

// ----------------------------------------------------------- construction

/// `gmp_init(int|string $num, int $base = 0): GMP`
pub fn gmp_init(env: &mut Env, num: Num, base: i64) -> GmpResult<Gmp> {
    init_impl(env, "gmp_init", num, base)
}

/// `GMP::__construct(int|string $num = 0, int $base = 0)`: `None` when
/// `$num` is omitted.
pub fn gmp_construct(env: &mut Env, num: Option<Num>, base: i64) -> GmpResult<Gmp> {
    init_impl(env, "GMP::__construct", num.unwrap_or(Num::Int(0)), base)
}

fn init_impl(env: &mut Env, func: &str, num: Num, base: i64) -> GmpResult<Gmp> {
    let v = int_or_str(env, func, num)?;
    if base != 0 && !(2..=62).contains(&base) {
        return Err(arg_value_error(
            func,
            2,
            "base",
            "must be 0 or between 2 and 62",
        ));
    }
    match v {
        IntOrStr::Int(i) => Ok(Gmp::from_i64(i)),
        IntOrStr::Str(s) => parse_integer(&s, base as u32)
            .map(Gmp)
            .ok_or_else(|| not_integer(func, 1, "num")),
    }
}

/// Word order and endianness from `$flags` (PHP manual: gmp_import):
/// `(msw_first, big_endian_words)`.
fn word_flags(func: &str, flags: i64) -> GmpResult<(bool, bool)> {
    let msw = match flags & (GMP_MSW_FIRST | GMP_LSW_FIRST) {
        GMP_LSW_FIRST => false,
        0 | GMP_MSW_FIRST => true,
        _ => {
            return Err(arg_value_error(
                func,
                3,
                "flags",
                "cannot use multiple word order options",
            ));
        }
    };
    let big = match flags & (GMP_LITTLE_ENDIAN | GMP_BIG_ENDIAN | GMP_NATIVE_ENDIAN) {
        GMP_BIG_ENDIAN => true,
        GMP_LITTLE_ENDIAN => false,
        0 | GMP_NATIVE_ENDIAN => cfg!(target_endian = "big"),
        _ => {
            return Err(arg_value_error(
                func,
                3,
                "flags",
                "cannot use multiple endian options",
            ));
        }
    };
    Ok((msw, big))
}

/// `gmp_import(string $data, int $word_size = 1, int $flags =
/// GMP_MSW_FIRST | GMP_NATIVE_ENDIAN): GMP`
pub fn gmp_import(data: &[u8], word_size: i64, flags: i64) -> GmpResult<Gmp> {
    const F: &str = "gmp_import";
    if word_size < 1 {
        return Err(arg_value_error(
            F,
            2,
            "word_size",
            "must be greater than or equal to 1",
        ));
    }
    let (msw, big) = word_flags(F, flags)?;
    let size = word_size as u64;
    if !(data.len() as u64).is_multiple_of(size) {
        return Err(GmpError::value(
            "gmp_import(): Argument #1 ($data) must be a multiple of argument #2 ($word_size)",
        ));
    }
    if data.is_empty() {
        return Ok(Gmp::default());
    }
    let size = size as usize;
    if msw && big {
        return Ok(Gmp(Int::from_bytes_be(data)));
    }
    if !msw && !big {
        return Ok(Gmp(Int::from_bytes_le(data)));
    }
    // Reorder into one big-endian byte string.
    let mut be = Vec::with_capacity(data.len());
    let words = data.chunks(size);
    let mut push = |w: &[u8]| {
        if big {
            be.extend_from_slice(w);
        } else {
            be.extend(w.iter().rev());
        }
    };
    if msw {
        words.for_each(&mut push);
    } else {
        words.rev().for_each(&mut push);
    }
    Ok(Gmp(Int::from_bytes_be(&be)))
}

/// `gmp_export(GMP|int|string $num, int $word_size = 1, int $flags =
/// GMP_MSW_FIRST | GMP_NATIVE_ENDIAN): string` (the magnitude; the sign is
/// dropped).
pub fn gmp_export(env: &mut Env, num: Num, word_size: i64, flags: i64) -> GmpResult<Vec<u8>> {
    const F: &str = "gmp_export";
    let n = arg(env, F, 1, "num", num)?;
    if word_size < 1 {
        return Err(arg_value_error(
            F,
            2,
            "word_size",
            "must be greater than or equal to 1",
        ));
    }
    let (msw, big) = word_flags(F, flags)?;
    if n.is_zero() {
        return Ok(Vec::new());
    }
    let nbits = n.bit_len() as u64;
    // Observed on PHP 8.5: the word size is refused when `8·size + bits - 1`
    // (the rounding up of the word count) overflows 64 bits.
    let size = word_size as u64;
    if 8 * size as u128 + nbits as u128 > 1 << 64 {
        return Err(GmpError::value(
            "gmp_export(): Argument #2 ($word_size) is too large for argument #1 ($num)",
        ));
    }
    let count = nbits.div_ceil(8 * size);
    let total = count * size;
    check_bits(total as u128 * 8)?;
    let mag = n.to_bytes_be();
    let mut be = vec![0u8; (total as usize) - mag.len()];
    be.extend_from_slice(&mag);
    if msw && big {
        return Ok(be);
    }
    if !msw && !big {
        be.reverse();
        return Ok(be);
    }
    let size = size as usize;
    let mut out = Vec::with_capacity(be.len());
    let mut push = |w: &[u8]| {
        if big {
            out.extend_from_slice(w);
        } else {
            out.extend(w.iter().rev());
        }
    };
    let words = be.chunks(size);
    if msw {
        words.for_each(&mut push);
    } else {
        words.rev().for_each(&mut push);
    }
    Ok(out)
}

/// GMP's conversion to a C long (`mpz_get_si`, observed on PHP 8.5): the
/// value when it fits, otherwise the low 63 bits of the magnitude with
/// the sign applied in one's-complement fashion (`-2^64` gives
/// `PHP_INT_MIN`).
pub fn get_si(n: &Int) -> i64 {
    if let Some(v) = n.to_i64() {
        return v;
    }
    let low = n.least_significant_limb();
    if n.is_negative() {
        -1 - (low.wrapping_sub(1) & i64::MAX as u64) as i64
    } else {
        (low & i64::MAX as u64) as i64
    }
}

/// `gmp_intval(GMP|int|string $num): int`
pub fn gmp_intval(env: &mut Env, num: Num) -> GmpResult<i64> {
    Ok(get_si(&*arg(env, "gmp_intval", 1, "num", num)?))
}

/// `gmp_strval(GMP|int|string $num, int $base = 10): string`
pub fn gmp_strval(env: &mut Env, num: Num, base: i64) -> GmpResult<String> {
    const F: &str = "gmp_strval";
    let n = arg(env, F, 1, "num", num)?;
    if !((2..=62).contains(&base) || (-36..=-2).contains(&base)) {
        return Err(arg_value_error(
            F,
            2,
            "base",
            "must be between 2 and 62, or -2 and -36",
        ));
    }
    Ok(to_string_radix(&n, base as i32))
}

// ------------------------------------------------------------- arithmetic

#[inline]
fn two<'a, 'b>(
    env: &mut Env,
    func: &str,
    a: Num<'a>,
    b: Num<'b>,
) -> GmpResult<(Cow<'a, Int>, Cow<'b, Int>)> {
    let x = arg(env, func, 1, "num1", a)?;
    let y = arg(env, func, 2, "num2", b)?;
    Ok((x, y))
}

/// `gmp_add(GMP|int|string $num1, GMP|int|string $num2): GMP`
pub fn gmp_add(env: &mut Env, a: Num, b: Num) -> GmpResult<Gmp> {
    let (a, b) = two(env, "gmp_add", a, b)?;
    Ok(Gmp(arith::add(&a, &b)))
}

/// `gmp_sub(GMP|int|string $num1, GMP|int|string $num2): GMP`
pub fn gmp_sub(env: &mut Env, a: Num, b: Num) -> GmpResult<Gmp> {
    let (a, b) = two(env, "gmp_sub", a, b)?;
    Ok(Gmp(arith::sub(&a, &b)))
}

pub(crate) fn mul(a: &Int, b: &Int) -> GmpResult<Int> {
    check_bits(bits(a) + bits(b))?;
    Ok(arith::mul(a, b))
}

/// `gmp_mul(GMP|int|string $num1, GMP|int|string $num2): GMP`
pub fn gmp_mul(env: &mut Env, a: Num, b: Num) -> GmpResult<Gmp> {
    let (a, b) = two(env, "gmp_mul", a, b)?;
    Ok(Gmp(mul(&a, &b)?))
}

/// Division with GMP's rounding modes: `(quotient, remainder)` with
/// `a = q·b + r`; `b` must not be zero.
pub(crate) fn divide(a: &Int, b: &Int, mode: i64) -> (Int, Int) {
    let (q, r) = arith::div_rem_trunc(a, b);
    // Move toward minus or plus infinity when there is a remainder of the
    // wrong sign.
    let adjust = !r.is_zero()
        && match mode {
            GMP_ROUND_MINUSINF => r.is_negative() != b.is_negative(),
            GMP_ROUND_PLUSINF => r.is_negative() == b.is_negative(),
            _ => false,
        };
    if !adjust {
        return (q, r);
    }
    if mode == GMP_ROUND_MINUSINF {
        (arith::sub(&q, &Int::ONE), arith::add(&r, b))
    } else {
        (arith::add(&q, &Int::ONE), arith::sub(&r, b))
    }
}

fn div_args<'a, 'b>(
    env: &mut Env,
    func: &str,
    a: Num<'a>,
    b: Num<'b>,
    mode: i64,
) -> GmpResult<(Cow<'a, Int>, Cow<'b, Int>)> {
    let (a, b) = two(env, func, a, b)?;
    if b.is_zero() {
        return Err(GmpError::div_zero(format!(
            "{func}(): Argument #2 ($num2) Division by zero"
        )));
    }
    check_rounding(func, mode)?;
    Ok((a, b))
}

/// `gmp_div_qr(GMP|int|string $num1, GMP|int|string $num2, int
/// $rounding_mode = GMP_ROUND_ZERO): array` (`[quotient, remainder]`)
pub fn gmp_div_qr(env: &mut Env, a: Num, b: Num, mode: i64) -> GmpResult<(Gmp, Gmp)> {
    let (a, b) = div_args(env, "gmp_div_qr", a, b, mode)?;
    let (q, r) = divide(&a, &b, mode);
    Ok((Gmp(q), Gmp(r)))
}

/// `gmp_div_q(GMP|int|string $num1, GMP|int|string $num2, int
/// $rounding_mode = GMP_ROUND_ZERO): GMP`
pub fn gmp_div_q(env: &mut Env, a: Num, b: Num, mode: i64) -> GmpResult<Gmp> {
    div_q_impl(env, "gmp_div_q", a, b, mode)
}

/// `gmp_div()`: alias of [`gmp_div_q`] (messages name `gmp_div()`).
pub fn gmp_div(env: &mut Env, a: Num, b: Num, mode: i64) -> GmpResult<Gmp> {
    div_q_impl(env, "gmp_div", a, b, mode)
}

fn div_q_impl(env: &mut Env, func: &str, a: Num, b: Num, mode: i64) -> GmpResult<Gmp> {
    let (a, b) = div_args(env, func, a, b, mode)?;
    Ok(Gmp(divide(&a, &b, mode).0))
}

/// `gmp_div_r(GMP|int|string $num1, GMP|int|string $num2, int
/// $rounding_mode = GMP_ROUND_ZERO): GMP`
pub fn gmp_div_r(env: &mut Env, a: Num, b: Num, mode: i64) -> GmpResult<Gmp> {
    let (a, b) = div_args(env, "gmp_div_r", a, b, mode)?;
    Ok(Gmp(divide(&a, &b, mode).1))
}

/// `gmp_mod(GMP|int|string $num1, GMP|int|string $num2): GMP`: the
/// non-negative remainder, in `[0, |num2|)`.
pub fn gmp_mod(env: &mut Env, a: Num, b: Num) -> GmpResult<Gmp> {
    let (a, b) = two(env, "gmp_mod", a, b)?;
    if b.is_zero() {
        return Err(GmpError::div_zero(
            "gmp_mod(): Argument #2 ($num2) Modulo by zero",
        ));
    }
    Ok(Gmp(arith::rem_euclid(&a, &b)))
}

/// Exact division. When `d` does not divide `a` the result is what GMP's
/// `mpz_divexact` computes (the GMP manual leaves it undefined; observed
/// with GMP 6.3): with `2^t` the power of two in `d`, the 2-adic quotient
/// `(|a| >> t)·(|d| >> t)^-1 mod 2^(64·k)`, `k` = limbs of `|a|` minus
/// limbs of `|d|` plus one (zero when `|a|` has fewer limbs), signed as
/// `a·d`.
pub(crate) fn divexact(a: &Int, d: &Int) -> Int {
    let (q, r) = a.div_rem_trunc(d);
    if r.is_zero() {
        return q;
    }
    let na = a.limbs().len();
    let nd = d.limbs().len();
    if na < nd {
        return Int::ZERO;
    }
    let k = 64 * (na - nd + 1) as u64;
    let t = d.trailing_zeros() as u64;
    let a2 = a.abs().div_2k_trunc(t);
    let d2 = d.abs().div_2k_trunc(t);
    let m = Int::ONE.mul_2k(k);
    let inv = d2.modinv(&m).unwrap_or(Int::ZERO);
    let q = a2.mul(&inv).mod_2k(k);
    if a.is_negative() != d.is_negative() {
        q.neg()
    } else {
        q
    }
}

/// `gmp_divexact(GMP|int|string $num1, GMP|int|string $num2): GMP`
pub fn gmp_divexact(env: &mut Env, a: Num, b: Num) -> GmpResult<Gmp> {
    let (a, b) = two(env, "gmp_divexact", a, b)?;
    if b.is_zero() {
        return Err(GmpError::div_zero(
            "gmp_divexact(): Argument #2 ($num2) Division by zero",
        ));
    }
    Ok(Gmp(divexact(&a, &b)))
}

/// `gmp_neg(GMP|int|string $num): GMP`
pub fn gmp_neg(env: &mut Env, num: Num) -> GmpResult<Gmp> {
    Ok(Gmp(arg(env, "gmp_neg", 1, "num", num)?.neg()))
}

/// `gmp_abs(GMP|int|string $num): GMP`
pub fn gmp_abs(env: &mut Env, num: Num) -> GmpResult<Gmp> {
    Ok(Gmp(arg(env, "gmp_abs", 1, "num", num)?.abs()))
}

/// `gmp_fact(GMP|int|string $num): GMP`
pub fn gmp_fact(env: &mut Env, num: Num) -> GmpResult<Gmp> {
    const F: &str = "gmp_fact";
    let n = arg(env, F, 1, "num", num)?;
    if n.is_negative() {
        return Err(arg_value_error(
            F,
            1,
            "num",
            "must be greater than or equal to 0",
        ));
    }
    let Some(n) = n.to_u64() else {
        return Err(GmpError::out_of_memory(u128::MAX));
    };
    // log2(n!) <= n·log2(n)
    check_bits(n as u128 * (64 - n.leading_zeros()) as u128)?;
    Ok(Gmp(Int::factorial(n)))
}

/// `gmp_sqrt(GMP|int|string $num): GMP`
pub fn gmp_sqrt(env: &mut Env, num: Num) -> GmpResult<Gmp> {
    let n = non_negative(env, "gmp_sqrt", num)?;
    Ok(Gmp(n.isqrt()))
}

fn non_negative<'a>(env: &mut Env, func: &str, num: Num<'a>) -> GmpResult<Cow<'a, Int>> {
    let n = arg(env, func, 1, "num", num)?;
    if n.is_negative() {
        return Err(arg_value_error(
            func,
            1,
            "num",
            "must be greater than or equal to 0",
        ));
    }
    Ok(n)
}

/// `gmp_sqrtrem(GMP|int|string $num): array` (`[root, remainder]`)
pub fn gmp_sqrtrem(env: &mut Env, num: Num) -> GmpResult<(Gmp, Gmp)> {
    let n = non_negative(env, "gmp_sqrtrem", num)?;
    let (r, rem) = n.isqrt_rem();
    Ok((Gmp(r), Gmp(rem)))
}

fn root_args<'a>(
    env: &mut Env,
    func: &str,
    num: Num<'a>,
    nth: i64,
    nth_msg: &str,
) -> GmpResult<Cow<'a, Int>> {
    let n = arg(env, func, 1, "num", num)?;
    if nth <= 0 {
        return Err(arg_value_error(func, 2, "nth", nth_msg));
    }
    if nth % 2 == 0 && n.is_negative() {
        return Err(arg_value_error(
            func,
            2,
            "nth",
            "must be odd if argument #1 ($a) is negative",
        ));
    }
    Ok(n)
}

/// `gmp_root(GMP|int|string $num, int $nth): GMP` (truncated toward zero)
pub fn gmp_root(env: &mut Env, num: Num, nth: i64) -> GmpResult<Gmp> {
    let n = root_args(env, "gmp_root", num, nth, "must be greater than 0")?;
    Ok(Gmp(n.root(nth as u64)))
}

/// `gmp_rootrem(GMP|int|string $num, int $nth): array` (`[root,
/// remainder]`, the remainder with the sign of `$num`)
pub fn gmp_rootrem(env: &mut Env, num: Num, nth: i64) -> GmpResult<(Gmp, Gmp)> {
    let n = root_args(
        env,
        "gmp_rootrem",
        num,
        nth,
        "must be greater than or equal to 1",
    )?;
    let (r, rem) = n.root_rem(nth as u64);
    Ok((Gmp(r), Gmp(rem)))
}

/// `base ** exp` with the size guard.
pub(crate) fn pow(base: &Int, exp: u64) -> GmpResult<Int> {
    if base.bit_len() <= 1 {
        // 0, 1 and -1
        let one = exp == 0 || (base.is_negative() && exp.is_multiple_of(2));
        return Ok(if one { Int::ONE } else { base.clone() });
    }
    check_bits(bits(base) * exp as u128)?;
    Ok(base.pow(exp))
}

/// `gmp_pow(GMP|int|string $num, int $exponent): GMP`
pub fn gmp_pow(env: &mut Env, num: Num, exponent: i64) -> GmpResult<Gmp> {
    const F: &str = "gmp_pow";
    let n = arg(env, F, 1, "num", num)?;
    if exponent < 0 {
        return Err(arg_value_error(
            F,
            2,
            "exponent",
            "must be greater than or equal to 0",
        ));
    }
    Ok(Gmp(pow(&n, exponent as u64)?))
}

/// `gmp_powm(GMP|int|string $num, GMP|int|string $exponent,
/// GMP|int|string $modulus): GMP`: the result is in `[0, |modulus|)`.
pub fn gmp_powm(env: &mut Env, num: Num, exponent: Num, modulus: Num) -> GmpResult<Gmp> {
    const F: &str = "gmp_powm";
    let b = arg(env, F, 1, "num", num)?;
    let e = arg(env, F, 2, "exponent", exponent)?;
    let m = arg(env, F, 3, "modulus", modulus)?;
    if e.is_negative() {
        return Err(arg_value_error(
            F,
            2,
            "exponent",
            "must be greater than or equal to 0",
        ));
    }
    if m.is_zero() {
        return Err(GmpError::div_zero("Modulo by zero"));
    }
    if let Some(mm) = m.abs().to_u64() {
        let base = arith::rem_euclid(&b, &m).to_u64().unwrap_or(0);
        return Ok(Gmp(Int::from_u64(small::powm(base, e.limbs(), mm))));
    }
    Ok(Gmp(b.modpow(&e, &m)))
}

/// `gmp_perfect_square(GMP|int|string $num): bool`
pub fn gmp_perfect_square(env: &mut Env, num: Num) -> GmpResult<bool> {
    Ok(arg(env, "gmp_perfect_square", 1, "num", num)?.is_perfect_square())
}

/// `gmp_perfect_power(GMP|int|string $num): bool` (0, 1 and -1 are
/// perfect powers; a negative number only through an odd exponent)
pub fn gmp_perfect_power(env: &mut Env, num: Num) -> GmpResult<bool> {
    Ok(arg(env, "gmp_perfect_power", 1, "num", num)?.is_perfect_power())
}

/// Below this bound GMP 6.3 reports primes as definitely prime (2);
/// above it as probably prime (1). Observed on PHP 8.5: 2462906046218231
/// gives 2 and the next prime, 2462906046218251, gives 1.
const DEFINITELY_PRIME_BELOW: u64 = 2_462_906_046_218_251;

/// GMP's primality answer for `|n|`: 0 composite, 1 probably prime, 2
/// definitely prime. The Baillie-PSW test has no known counterexample
/// (and none below 2^64), so composites are always 0, as observed with
/// GMP; `$repetitions` only adds Miller-Rabin rounds there, which cannot
/// change that answer.
pub(crate) fn prob_prime(n: &Int) -> i64 {
    if n.limbs().len() <= 1 {
        let v = n.least_significant_limb();
        return match small::is_prime(v) {
            false => 0,
            true if v < DEFINITELY_PRIME_BELOW => 2,
            true => 1,
        };
    }
    if n.abs().is_prime_bpsw() { 1 } else { 0 }
}

/// `gmp_prob_prime(GMP|int|string $num, int $repetitions = 10): int`
pub fn gmp_prob_prime(env: &mut Env, num: Num, _repetitions: i64) -> GmpResult<i64> {
    Ok(prob_prime(&*arg(env, "gmp_prob_prime", 1, "num", num)?))
}

/// `gmp_gcd(GMP|int|string $num1, GMP|int|string $num2): GMP`
pub fn gmp_gcd(env: &mut Env, a: Num, b: Num) -> GmpResult<Gmp> {
    let (a, b) = two(env, "gmp_gcd", a, b)?;
    if a.limbs().len() <= 1 && b.limbs().len() <= 1 {
        let g = small::gcd(a.least_significant_limb(), b.least_significant_limb());
        return Ok(Gmp(Int::from_u64(g)));
    }
    Ok(Gmp(a.gcd(&b)))
}

/// The result of `gmp_gcdext()`: `['g' => ..., 's' => ..., 't' => ...]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GcdExt {
    pub g: Gmp,
    pub s: Gmp,
    pub t: Gmp,
}

/// `gmp_gcdext(GMP|int|string $num1, GMP|int|string $num2): array`:
/// `g = num1·s + num2·t` with GMP's choice of cofactors (`|s| <
/// |num2|/(2g)`, `|t| < |num1|/(2g)`, `s = 0` when `|num1| = |num2|`).
pub fn gmp_gcdext(env: &mut Env, a: Num, b: Num) -> GmpResult<GcdExt> {
    let (a, b) = two(env, "gmp_gcdext", a, b)?;
    let (g, s, t) = a.extended_gcd(&b);
    Ok(GcdExt {
        g: Gmp(g),
        s: Gmp(s),
        t: Gmp(t),
    })
}

/// `gmp_lcm(GMP|int|string $num1, GMP|int|string $num2): GMP`
pub fn gmp_lcm(env: &mut Env, a: Num, b: Num) -> GmpResult<Gmp> {
    let (a, b) = two(env, "gmp_lcm", a, b)?;
    Ok(Gmp(a.lcm(&b)))
}

/// `gmp_invert(GMP|int|string $num1, GMP|int|string $num2): GMP|false`:
/// the inverse in `[0, |num2|)`, `None` (false) when there is none.
pub fn gmp_invert(env: &mut Env, a: Num, b: Num) -> GmpResult<Option<Gmp>> {
    let (a, b) = two(env, "gmp_invert", a, b)?;
    if b.is_zero() {
        return Err(GmpError::div_zero("Division by zero"));
    }
    Ok(a.modinv(&b).map(Gmp))
}

/// `gmp_jacobi(GMP|int|string $num1, GMP|int|string $num2): int`
pub fn gmp_jacobi(env: &mut Env, a: Num, b: Num) -> GmpResult<i64> {
    let (a, b) = two(env, "gmp_jacobi", a, b)?;
    Ok(a.jacobi(&b) as i64)
}

/// `gmp_legendre(GMP|int|string $num1, GMP|int|string $num2): int`
pub fn gmp_legendre(env: &mut Env, a: Num, b: Num) -> GmpResult<i64> {
    let (a, b) = two(env, "gmp_legendre", a, b)?;
    Ok(a.legendre(&b) as i64)
}

/// `gmp_kronecker(GMP|int|string $num1, GMP|int|string $num2): int`
pub fn gmp_kronecker(env: &mut Env, a: Num, b: Num) -> GmpResult<i64> {
    let (a, b) = two(env, "gmp_kronecker", a, b)?;
    Ok(a.kronecker(&b) as i64)
}

/// `gmp_cmp(GMP|int|string $num1, GMP|int|string $num2): int` (-1, 0, 1)
pub fn gmp_cmp(env: &mut Env, a: Num, b: Num) -> GmpResult<i64> {
    let (a, b) = two(env, "gmp_cmp", a, b)?;
    Ok(a.cmp(&b) as i64)
}

/// `gmp_sign(GMP|int|string $num): int`
pub fn gmp_sign(env: &mut Env, num: Num) -> GmpResult<i64> {
    Ok(arg(env, "gmp_sign", 1, "num", num)?.signum() as i64)
}

// ----------------------------------------------------------------- random

/// `gmp_random_seed(GMP|int|string $seed): void`
pub fn gmp_random_seed(env: &mut Env, seed: Num) -> GmpResult<()> {
    let s = arg(env, "gmp_random_seed", 1, "seed", seed)?;
    random::seed(&s);
    Ok(())
}

/// `gmp_random_bits(int $bits): GMP`: uniform in `[0, 2^bits)`.
pub fn gmp_random_bits(bits: i64) -> GmpResult<Gmp> {
    if !(1..=i32::MAX as i64).contains(&bits) {
        return Err(GmpError::value(
            "gmp_random_bits(): Argument #1 ($bits) must be between 1 and 2147483647",
        ));
    }
    check_bits(bits as u128)?;
    Ok(Gmp(random::bits(bits as u64)))
}

/// `gmp_random_range(GMP|int|string $min, GMP|int|string $max): GMP`:
/// uniform in `[min, max]`.
pub fn gmp_random_range(env: &mut Env, min: Num, max: Num) -> GmpResult<Gmp> {
    const F: &str = "gmp_random_range";
    let lo = arg(env, F, 1, "min", min)?;
    let hi = arg(env, F, 2, "max", max)?;
    if *lo >= *hi {
        return Err(GmpError::value(
            "gmp_random_range(): Argument #1 ($min) must be less than argument #2 ($maximum)",
        ));
    }
    Ok(Gmp(random::range(&lo, &hi)))
}

// ------------------------------------------------------------------- bits

/// `gmp_and(GMP|int|string $num1, GMP|int|string $num2): GMP`
/// (two's complement)
pub fn gmp_and(env: &mut Env, a: Num, b: Num) -> GmpResult<Gmp> {
    let (a, b) = two(env, "gmp_and", a, b)?;
    Ok(Gmp(a.bitand(&b)))
}

/// `gmp_or(GMP|int|string $num1, GMP|int|string $num2): GMP`
pub fn gmp_or(env: &mut Env, a: Num, b: Num) -> GmpResult<Gmp> {
    let (a, b) = two(env, "gmp_or", a, b)?;
    Ok(Gmp(a.bitor(&b)))
}

/// `gmp_xor(GMP|int|string $num1, GMP|int|string $num2): GMP`
pub fn gmp_xor(env: &mut Env, a: Num, b: Num) -> GmpResult<Gmp> {
    let (a, b) = two(env, "gmp_xor", a, b)?;
    Ok(Gmp(a.bitxor(&b)))
}

/// `gmp_com(GMP|int|string $num): GMP` (`-num - 1`)
pub fn gmp_com(env: &mut Env, num: Num) -> GmpResult<Gmp> {
    Ok(Gmp(arg(env, "gmp_com", 1, "num", num)?.not()))
}

/// `gmp_setbit(GMP $num, int $index, bool $value = true): void`
/// (two's complement)
pub fn gmp_setbit(num: &mut Gmp, index: i64, value: bool) -> GmpResult<()> {
    let i = check_index("gmp_setbit", "index", index)?;
    if value {
        if !num.0.is_negative() {
            check_bits(i as u128 + 1)?;
        }
        num.0.set_bit(i);
    } else {
        if num.0.is_negative() {
            check_bits(i as u128 + 1)?;
        }
        num.0.clear_bit(i);
    }
    Ok(())
}

/// `gmp_clrbit(GMP $num, int $index): void`
pub fn gmp_clrbit(num: &mut Gmp, index: i64) -> GmpResult<()> {
    let i = check_index("gmp_clrbit", "index", index)?;
    if num.0.is_negative() {
        check_bits(i as u128 + 1)?;
    }
    num.0.clear_bit(i);
    Ok(())
}

/// Bit `i` of the two's complement form.
pub(crate) fn test_bit(n: &Int, i: u64) -> bool {
    if n.is_negative() {
        // -x = ~(x - 1)
        !n.abs().sub(&Int::ONE).bit(i)
    } else {
        n.bit(i)
    }
}

/// `gmp_testbit(GMP|int|string $num, int $index): bool`
pub fn gmp_testbit(env: &mut Env, num: Num, index: i64) -> GmpResult<bool> {
    const F: &str = "gmp_testbit";
    let n = arg(env, F, 1, "num", num)?;
    let i = check_index(F, "index", index)?;
    Ok(test_bit(&n, i))
}

/// `gmp_scan0(GMP|int|string $num1, int $start): int` (-1 when there is
/// no zero bit)
pub fn gmp_scan0(env: &mut Env, num: Num, start: i64) -> GmpResult<i64> {
    const F: &str = "gmp_scan0";
    let n = arg(env, F, 1, "num1", num)?;
    let i = check_index(F, "start", start)?;
    Ok(n.scan0(i).map_or(-1, |v| v as i64))
}

/// `gmp_scan1(GMP|int|string $num1, int $start): int` (-1 when there is
/// no one bit)
pub fn gmp_scan1(env: &mut Env, num: Num, start: i64) -> GmpResult<i64> {
    const F: &str = "gmp_scan1";
    let n = arg(env, F, 1, "num1", num)?;
    let i = check_index(F, "start", start)?;
    Ok(n.scan1(i).map_or(-1, |v| v as i64))
}

/// `gmp_popcount(GMP|int|string $num): int` (-1 for a negative number)
pub fn gmp_popcount(env: &mut Env, num: Num) -> GmpResult<i64> {
    let n = arg(env, "gmp_popcount", 1, "num", num)?;
    Ok(n.count_ones().map_or(-1, |v| v as i64))
}

/// `gmp_hamdist(GMP|int|string $num1, GMP|int|string $num2): int` (-1
/// when the signs differ)
pub fn gmp_hamdist(env: &mut Env, a: Num, b: Num) -> GmpResult<i64> {
    let (a, b) = two(env, "gmp_hamdist", a, b)?;
    Ok(a.hamming_distance(&b).map_or(-1, |v| v as i64))
}

/// `gmp_nextprime(GMP|int|string $num): GMP` (the smallest prime greater
/// than `$num`; 2 for anything below 2)
pub fn gmp_nextprime(env: &mut Env, num: Num) -> GmpResult<Gmp> {
    let n = arg(env, "gmp_nextprime", 1, "num", num)?;
    if n.is_negative() {
        return Ok(Gmp(Int::from_u64(2)));
    }
    if let Some(p) = n.to_u64().and_then(small::next_prime) {
        return Ok(Gmp(Int::from_u64(p)));
    }
    Ok(Gmp(n.next_prime()))
}

/// `gmp_binomial(GMP|int|string $n, int $k): GMP` (GMP's `mpz_bin_ui`:
/// `C(n, k) = (-1)^k·C(k-n-1, k)` for negative `n`)
pub fn gmp_binomial(env: &mut Env, n: Num, k: i64) -> GmpResult<Gmp> {
    const F: &str = "gmp_binomial";
    let n = arg(env, F, 1, "n", n)?;
    if k < 0 {
        return Err(arg_value_error(
            F,
            2,
            "k",
            "must be greater than or equal to 0",
        ));
    }
    let k = k as u64;
    if let Some(m) = n.to_u64() {
        if k > m {
            return Ok(Gmp::default());
        }
        let k = k.min(m - k);
        check_bits(k as u128 * (64 - m.leading_zeros()) as u128)?;
    } else {
        // |C(n, k)| <= max(|n|, k)^k
        let top = if n.is_negative() {
            n.abs().add(&Int::from_u64(k)).bit_len()
        } else {
            n.bit_len()
        };
        check_bits(k as u128 * top as u128)?;
    }
    Ok(Gmp(Int::binomial_int(&n, k)))
}
