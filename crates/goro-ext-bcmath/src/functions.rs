//! The `bc*()` functions (PHP manual: BCMath Functions).
//!
//! Each function comes in two forms: `bcadd()` returns a new `Vec<u8>`,
//! `bcadd_into()` appends the result to a caller buffer (an engine keeps
//! one scratch buffer and copies the result into its string type, so a call
//! costs a single allocation).
//!
//! Argument validation follows PHP 8.5's order (observed on PHP 8.5 and
//! tests ext/bcmath/tests/negative_scale.phpt, bcpowmod_*.phpt, bug72093.phpt):
//! the `$scale` argument first, then each number's well-formedness in
//! argument order, then the function's own conditions.

use crate::dec::{self, Dec, Num, RoundingMode};
use crate::{BcError, arith, max_result_len};
use std::cmp::Ordering;

/// The largest accepted scale (`2147483647`).
pub const MAX_SCALE: i64 = i32::MAX as i64;

/// Resolves an optional `$scale` argument against the `bcmath.scale` default.
pub(crate) fn resolve_scale(
    func: &str,
    arg: u32,
    scale: Option<i64>,
    default_scale: u32,
) -> Result<usize, BcError> {
    match scale {
        None => Ok(default_scale as usize),
        Some(s) if (0..=MAX_SCALE).contains(&s) => Ok(s as usize),
        Some(_) => Err(BcError::value(format!(
            "{func}(): Argument #{arg} ($scale) must be between 0 and 2147483647"
        ))),
    }
}

#[inline]
pub(crate) fn parse_arg<'a>(
    func: &str,
    arg: u32,
    name: &str,
    s: &'a [u8],
) -> Result<Num<'a>, BcError> {
    match dec::parse(s) {
        Some(n) => Ok(n),
        None => Err(not_well_formed(func, arg, name)),
    }
}

#[cold]
fn not_well_formed(func: &str, arg: u32, name: &str) -> BcError {
    BcError::value(format!(
        "{func}(): Argument #{arg} (${name}) is not well-formed"
    ))
}

/// Appends `n` at `scale`, refusing a result longer than [`max_result_len`].
pub(crate) fn output_into(out: &mut Vec<u8>, n: Num, scale: usize) -> Result<(), BcError> {
    let len = dec::formatted_len(&n, scale);
    if len > max_result_len() {
        return Err(BcError::out_of_memory(len));
    }
    dec::write_num(out, n, scale);
    Ok(())
}

/// Formats a result, refusing one longer than [`max_result_len`].
pub(crate) fn output(n: Num, scale: usize) -> Result<Vec<u8>, BcError> {
    let mut out = Vec::new();
    output_into(&mut out, n, scale)?;
    Ok(out)
}

pub(crate) fn check_len(len: u128) -> Result<(), BcError> {
    if len > max_result_len() as u128 {
        Err(BcError::out_of_memory(
            usize::try_from(len).unwrap_or(usize::MAX),
        ))
    } else {
        Ok(())
    }
}

/// Upper bound of the length of a two-operand result at `scale`, before
/// the operation allocates it (a product's integer part is at most the sum
/// of both, a quotient's at most the dividend's plus the divisor's scale).
#[inline]
pub(crate) fn result_bound(a: &Num, b: &Num, scale: usize) -> u128 {
    a.int.len() as u128 + b.int.len() as u128 + b.frac.len() as u128 + scale as u128 + 4
}

/// Runs a two-operand operation that appends its result at `scale`.
#[inline]
pub(crate) fn run_into(
    out: &mut Vec<u8>,
    a: &Num,
    b: &Num,
    scale: usize,
    f: fn(&mut Vec<u8>, &Num, &Num, usize),
) -> Result<(), BcError> {
    check_len(result_bound(a, b, scale))?;
    f(out, a, b, scale);
    Ok(())
}

/// Generates the `Vec`-returning form of an `_into` function.
macro_rules! returning {
    ($(#[$m:meta])* $name:ident => $into:ident ( $($arg:ident : $ty:ty),* )) => {
        $(#[$m])*
        pub fn $name($($arg: $ty),*) -> Result<Vec<u8>, BcError> {
            let mut out = Vec::new();
            $into(&mut out, $($arg),*)?;
            Ok(out)
        }
    };
}

/// `bcadd()` appending to `out`.
pub fn bcadd_into(
    out: &mut Vec<u8>,
    num1: &[u8],
    num2: &[u8],
    scale: Option<i64>,
    default_scale: u32,
) -> Result<(), BcError> {
    let scale = resolve_scale("bcadd", 3, scale, default_scale)?;
    let a = parse_arg("bcadd", 1, "num1", num1)?;
    let b = parse_arg("bcadd", 2, "num2", num2)?;
    run_into(out, &a, &b, scale, dec::add_into)
}

returning!(
    /// `bcadd(string $num1, string $num2, ?int $scale = null): string`
    bcadd => bcadd_into(num1: &[u8], num2: &[u8], scale: Option<i64>, default_scale: u32)
);

/// `bcsub()` appending to `out`.
pub fn bcsub_into(
    out: &mut Vec<u8>,
    num1: &[u8],
    num2: &[u8],
    scale: Option<i64>,
    default_scale: u32,
) -> Result<(), BcError> {
    let scale = resolve_scale("bcsub", 3, scale, default_scale)?;
    let a = parse_arg("bcsub", 1, "num1", num1)?;
    let b = parse_arg("bcsub", 2, "num2", num2)?;
    run_into(out, &a, &b, scale, dec::sub_into)
}

returning!(
    /// `bcsub(string $num1, string $num2, ?int $scale = null): string`
    bcsub => bcsub_into(num1: &[u8], num2: &[u8], scale: Option<i64>, default_scale: u32)
);

/// `bcmul()` appending to `out`.
pub fn bcmul_into(
    out: &mut Vec<u8>,
    num1: &[u8],
    num2: &[u8],
    scale: Option<i64>,
    default_scale: u32,
) -> Result<(), BcError> {
    let scale = resolve_scale("bcmul", 3, scale, default_scale)?;
    let a = parse_arg("bcmul", 1, "num1", num1)?;
    let b = parse_arg("bcmul", 2, "num2", num2)?;
    run_into(out, &a, &b, scale, arith::mul_into)
}

returning!(
    /// `bcmul(string $num1, string $num2, ?int $scale = null): string`
    bcmul => bcmul_into(num1: &[u8], num2: &[u8], scale: Option<i64>, default_scale: u32)
);

pub(crate) const DIV_ZERO: &str = "Division by zero";
pub(crate) const MOD_ZERO: &str = "Modulo by zero";

/// `bcdiv()` appending to `out`.
pub fn bcdiv_into(
    out: &mut Vec<u8>,
    num1: &[u8],
    num2: &[u8],
    scale: Option<i64>,
    default_scale: u32,
) -> Result<(), BcError> {
    let scale = resolve_scale("bcdiv", 3, scale, default_scale)?;
    let a = parse_arg("bcdiv", 1, "num1", num1)?;
    let b = parse_arg("bcdiv", 2, "num2", num2)?;
    if b.is_zero() {
        return Err(BcError::division_by_zero(DIV_ZERO));
    }
    run_into(out, &a, &b, scale, arith::div_into)
}

returning!(
    /// `bcdiv(string $num1, string $num2, ?int $scale = null): string`
    bcdiv => bcdiv_into(num1: &[u8], num2: &[u8], scale: Option<i64>, default_scale: u32)
);

/// `bcmod()` appending to `out`.
pub fn bcmod_into(
    out: &mut Vec<u8>,
    num1: &[u8],
    num2: &[u8],
    scale: Option<i64>,
    default_scale: u32,
) -> Result<(), BcError> {
    let scale = resolve_scale("bcmod", 3, scale, default_scale)?;
    let a = parse_arg("bcmod", 1, "num1", num1)?;
    let b = parse_arg("bcmod", 2, "num2", num2)?;
    if b.is_zero() {
        return Err(BcError::division_by_zero(MOD_ZERO));
    }
    let (_, r) = arith::divmod(&a, &b);
    output_into(out, r.view(), scale)
}

returning!(
    /// `bcmod(string $num1, string $num2, ?int $scale = null): string`
    bcmod => bcmod_into(num1: &[u8], num2: &[u8], scale: Option<i64>, default_scale: u32)
);

/// `bcdivmod(string $num1, string $num2, ?int $scale = null): array`,
/// returning `[quotient, remainder]`; the quotient always has scale 0
/// (PHP manual: bcdivmod).
pub fn bcdivmod(
    num1: &[u8],
    num2: &[u8],
    scale: Option<i64>,
    default_scale: u32,
) -> Result<(Vec<u8>, Vec<u8>), BcError> {
    let scale = resolve_scale("bcdivmod", 3, scale, default_scale)?;
    let a = parse_arg("bcdivmod", 1, "num1", num1)?;
    let b = parse_arg("bcdivmod", 2, "num2", num2)?;
    if b.is_zero() {
        return Err(BcError::division_by_zero(DIV_ZERO));
    }
    let (q, r) = arith::divmod(&a, &b);
    Ok((output(q.view(), 0)?, output(r.view(), scale)?))
}

/// Why an exponent was refused.
pub(crate) enum ExpError {
    Fractional,
    TooLarge,
}

/// An exponent's value: integral, with `|e| <= i64::MAX` (test
/// ext/bcmath/tests/bcpow_error2.phpt refuses `-9223372036854775808` too).
pub(crate) fn exponent(e: &Num) -> Result<i64, ExpError> {
    if e.has_fraction() {
        return Err(ExpError::Fractional);
    }
    match e.int_u64() {
        Some(v) if v <= i64::MAX as u64 => Ok(if e.neg { -(v as i64) } else { v as i64 }),
        _ => Err(ExpError::TooLarge),
    }
}

/// log10 bounds of `|b|` from its leading 17 significant digits:
/// `(lower, upper)`.
fn log10_bounds(b: &Num) -> (f64, f64) {
    let (p1, p2) = b.unscaled_parts();
    let lead: Vec<u8> = p1.iter().chain(p2).take(17).copied().collect();
    let k = lead.len() as f64;
    let m = lead.iter().fold(0f64, |v, &c| v * 10.0 + (c - b'0') as f64);
    // |b| = 0.d1d2... * 10^exp10 with exp10 the position of the first digit.
    let exp10 = if b.int.is_empty() {
        -((b.frac.len() - p2.len()) as f64)
    } else {
        b.int.len() as f64
    };
    // |b| lies in [m, m + 1) * 10^(exp10 - k).
    ((m.log10() + exp10 - k), ((m + 1.0).log10() + exp10 - k))
}

/// `base^e`, exact for `e >= 0` (or zero when `trunc_scale` says the result
/// is cut there and is certainly below one unit of it); for `e < 0`,
/// `1 / base^-e` truncated at `neg_scale` digits. `Ok(None)` is a zero base
/// with a negative exponent.
pub(crate) fn power(
    base: &Num,
    e: i64,
    neg_scale: usize,
    trunc_scale: Option<usize>,
) -> Result<Option<Dec>, BcError> {
    let b = base.trimmed();
    if b.is_zero() {
        return Ok(match e.cmp(&0) {
            Ordering::Less => None,
            Ordering::Equal => Some(Dec::from_digits(false, b"1".to_vec(), 0)),
            Ordering::Greater => Some(Dec::zero()),
        });
    }
    let mag = e.unsigned_abs();
    if e == 0 {
        return Ok(Some(Dec::from_digits(false, b"1".to_vec(), 0)));
    }
    // |base| == 1: the result is ±1 whatever the exponent's size.
    if b.int == b"1" && b.frac.is_empty() {
        let neg = b.neg && mag % 2 == 1;
        return Ok(Some(Dec::from_digits(neg, b"1".to_vec(), 0)));
    }
    // Results that truncate to zero, without the (possibly huge) exact
    // power: |b|^e < 10^-(scale + 1), with a safety margin.
    if mag > 64 {
        let (lo, hi) = log10_bounds(&b);
        let zero = if e > 0 {
            trunc_scale.is_some_and(|s| hi < 0.0 && mag as f64 * hi < -(s as f64) - 2.0)
        } else {
            lo > 0.0 && mag as f64 * lo > neg_scale as f64 + 2.0
        };
        if zero {
            return Ok(Some(Dec::zero()));
        }
    }
    check_len(arith::pow_len_estimate(&b, mag))?;
    let p = arith::pow(&b, mag);
    if e > 0 {
        return Ok(Some(p));
    }
    let one = Num {
        neg: false,
        int: b"1",
        frac: b"",
    };
    Ok(Some(arith::div(&one, &p.view(), neg_scale)))
}

/// `bcpow()` appending to `out`.
pub fn bcpow_into(
    out: &mut Vec<u8>,
    num: &[u8],
    exponent: &[u8],
    scale: Option<i64>,
    default_scale: u32,
) -> Result<(), BcError> {
    let scale = resolve_scale("bcpow", 3, scale, default_scale)?;
    let a = parse_arg("bcpow", 1, "num", num)?;
    let e = parse_arg("bcpow", 2, "exponent", exponent)?;
    let e = self::exponent(&e).map_err(|err| {
        BcError::value(match err {
            ExpError::Fractional => {
                "bcpow(): Argument #2 ($exponent) cannot have a fractional part"
            }
            ExpError::TooLarge => "bcpow(): Argument #2 ($exponent) is too large",
        })
    })?;
    match power(&a, e, scale, Some(scale))? {
        Some(r) => output_into(out, r.view(), scale),
        None => Err(BcError::division_by_zero("Negative power of zero")),
    }
}

returning!(
    /// `bcpow(string $num, string $exponent, ?int $scale = null): string`
    bcpow => bcpow_into(num: &[u8], exponent: &[u8], scale: Option<i64>, default_scale: u32)
);

/// `bcpowmod()` appending to `out`.
pub fn bcpowmod_into(
    out: &mut Vec<u8>,
    num: &[u8],
    exponent: &[u8],
    modulus: &[u8],
    scale: Option<i64>,
    default_scale: u32,
) -> Result<(), BcError> {
    const F: &str = "bcpowmod";
    let scale = resolve_scale(F, 4, scale, default_scale)?;
    let a = parse_arg(F, 1, "num", num)?;
    let e = parse_arg(F, 2, "exponent", exponent)?;
    let m = parse_arg(F, 3, "modulus", modulus)?;
    let r = powmod_checked(&a, &e, &m, |which| {
        BcError::value(match which {
            PowmodError::BaseFraction => {
                format!("{F}(): Argument #1 ($num) cannot have a fractional part")
            }
            PowmodError::ExpFraction => {
                format!("{F}(): Argument #2 ($exponent) cannot have a fractional part")
            }
            PowmodError::ExpNegative => {
                format!("{F}(): Argument #2 ($exponent) must be greater than or equal to 0")
            }
            PowmodError::ModFraction => {
                format!("{F}(): Argument #3 ($modulus) cannot have a fractional part")
            }
        })
    })?;
    output_into(out, r.view(), scale)
}

returning!(
    /// `bcpowmod(string $num, string $exponent, string $modulus, ?int $scale = null): string`
    bcpowmod => bcpowmod_into(
        num: &[u8],
        exponent: &[u8],
        modulus: &[u8],
        scale: Option<i64>,
        default_scale: u32
    )
);

pub(crate) enum PowmodError {
    BaseFraction,
    ExpFraction,
    ExpNegative,
    ModFraction,
}

/// The checks of `bcpowmod` in PHP's order, then the computation.
pub(crate) fn powmod_checked(
    a: &Num,
    e: &Num,
    m: &Num,
    err: impl Fn(PowmodError) -> BcError,
) -> Result<Dec, BcError> {
    if a.has_fraction() {
        return Err(err(PowmodError::BaseFraction));
    }
    if e.has_fraction() {
        return Err(err(PowmodError::ExpFraction));
    }
    if e.neg && !e.is_zero() {
        return Err(err(PowmodError::ExpNegative));
    }
    if m.has_fraction() {
        return Err(err(PowmodError::ModFraction));
    }
    if m.is_zero() {
        return Err(BcError::division_by_zero(MOD_ZERO));
    }
    Ok(arith::powmod(a, e, m))
}

/// `bcsqrt()` appending to `out`.
pub fn bcsqrt_into(
    out: &mut Vec<u8>,
    num: &[u8],
    scale: Option<i64>,
    default_scale: u32,
) -> Result<(), BcError> {
    let scale = resolve_scale("bcsqrt", 2, scale, default_scale)?;
    let a = parse_arg("bcsqrt", 1, "num", num)?;
    if a.neg && !a.is_zero() {
        return Err(BcError::value(
            "bcsqrt(): Argument #1 ($num) must be greater than or equal to 0",
        ));
    }
    check_len(a.int.len() as u128 / 2 + scale as u128 + 3)?;
    output_into(out, arith::sqrt(&a, scale).view(), scale)
}

returning!(
    /// `bcsqrt(string $num, ?int $scale = null): string`
    bcsqrt => bcsqrt_into(num: &[u8], scale: Option<i64>, default_scale: u32)
);

/// `bccomp(string $num1, string $num2, ?int $scale = null): int`: compares
/// the numbers cut to `$scale` digits.
pub fn bccomp(
    num1: &[u8],
    num2: &[u8],
    scale: Option<i64>,
    default_scale: u32,
) -> Result<i64, BcError> {
    let scale = resolve_scale("bccomp", 3, scale, default_scale)?;
    let a = parse_arg("bccomp", 1, "num1", num1)?;
    let b = parse_arg("bccomp", 2, "num2", num2)?;
    Ok(ordering_int(dec::cmp(
        &a.truncated(scale),
        &b.truncated(scale),
    )))
}

pub(crate) fn ordering_int(o: Ordering) -> i64 {
    match o {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// `bcscale(?int $scale = null): int`: validates a new default scale.
/// Returns the scale to store in `bcmath.scale` (`None` when only reading);
/// the function returns the previous default either way.
pub fn bcscale(scale: Option<i64>) -> Result<Option<u32>, BcError> {
    match scale {
        None => Ok(None),
        Some(s) if (0..=MAX_SCALE).contains(&s) => Ok(Some(s as u32)),
        Some(_) => Err(BcError::value(
            "bcscale(): Argument #1 ($scale) must be between 0 and 2147483647",
        )),
    }
}

pub(crate) fn round_num(n: &Num, precision: i64, mode: RoundingMode) -> Result<Dec, BcError> {
    let max = max_result_len();
    dec::round(n, precision, mode, max).ok_or_else(|| {
        BcError::out_of_memory(precision.unsigned_abs().min(usize::MAX as u64) as usize)
    })
}

/// `bcround()` appending to `out`.
pub fn bcround_into(
    out: &mut Vec<u8>,
    num: &[u8],
    precision: i64,
    mode: RoundingMode,
) -> Result<(), BcError> {
    let a = parse_arg("bcround", 1, "num", num)?;
    let r = round_num(&a, precision, mode)?;
    output_into(out, r.view(), precision.max(0) as usize)
}

returning!(
    /// `bcround(string $num, int $precision = 0, RoundingMode $mode = RoundingMode::HalfAwayFromZero): string`
    bcround => bcround_into(num: &[u8], precision: i64, mode: RoundingMode)
);

/// `bcfloor()` appending to `out`.
pub fn bcfloor_into(out: &mut Vec<u8>, num: &[u8]) -> Result<(), BcError> {
    let a = parse_arg("bcfloor", 1, "num", num)?;
    output_into(
        out,
        round_num(&a, 0, RoundingMode::NegativeInfinity)?.view(),
        0,
    )
}

returning!(
    /// `bcfloor(string $num): string`
    bcfloor => bcfloor_into(num: &[u8])
);

/// `bcceil()` appending to `out`.
pub fn bcceil_into(out: &mut Vec<u8>, num: &[u8]) -> Result<(), BcError> {
    let a = parse_arg("bcceil", 1, "num", num)?;
    output_into(
        out,
        round_num(&a, 0, RoundingMode::PositiveInfinity)?.view(),
        0,
    )
}

returning!(
    /// `bcceil(string $num): string`
    bcceil => bcceil_into(num: &[u8])
);
