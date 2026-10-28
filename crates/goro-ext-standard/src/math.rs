//! Math functions: `round` and its modes, `floor`/`ceil`/`fmod`/`intdiv`,
//! base conversions and `number_format`.
//!
//! Behaviour sources: PHP manual pages (round, number_format, base_convert,
//! bindec, hexdec, octdec, intdiv), the RoundingMode RFC
//! (wiki.php.net/rfc/new_rounding_modes_to_round_function), the
//! base_convert improvements RFC (wiki.php.net/rfc/base_convert_improvements),
//! tests ext/standard/tests/math/*.phpt, and observations on PHP 8.5 kept in
//! `tests/fixtures/math.txt`.

use crate::error::{Diag, StdError};

/// Rounding modes of `round()`: the `PHP_ROUND_*` constants and the
/// `RoundingMode` enum cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundMode {
    /// `PHP_ROUND_HALF_UP`, `RoundingMode::HalfAwayFromZero`.
    HalfAwayFromZero,
    /// `PHP_ROUND_HALF_DOWN`, `RoundingMode::HalfTowardsZero`.
    HalfTowardsZero,
    /// `PHP_ROUND_HALF_EVEN`, `RoundingMode::HalfEven`.
    HalfEven,
    /// `PHP_ROUND_HALF_ODD`, `RoundingMode::HalfOdd`.
    HalfOdd,
    /// `RoundingMode::TowardsZero`.
    TowardsZero,
    /// `RoundingMode::AwayFromZero`.
    AwayFromZero,
    /// `RoundingMode::NegativeInfinity`.
    NegativeInfinity,
    /// `RoundingMode::PositiveInfinity`.
    PositiveInfinity,
}

impl RoundMode {
    /// The mode for a `PHP_ROUND_*` integer constant (1 to 4).
    pub fn from_int(mode: i64) -> Result<RoundMode, StdError> {
        Ok(match mode {
            1 => RoundMode::HalfAwayFromZero,
            2 => RoundMode::HalfTowardsZero,
            3 => RoundMode::HalfEven,
            4 => RoundMode::HalfOdd,
            _ => {
                return Err(StdError::ValueError(
                    "round(): Argument #3 ($mode) must be a valid rounding mode (RoundingMode::*)"
                        .into(),
                ));
            }
        })
    }

    /// The mode for a `RoundingMode` case name.
    pub fn from_case_name(name: &str) -> Option<RoundMode> {
        Some(match name {
            "HalfAwayFromZero" => RoundMode::HalfAwayFromZero,
            "HalfTowardsZero" => RoundMode::HalfTowardsZero,
            "HalfEven" => RoundMode::HalfEven,
            "HalfOdd" => RoundMode::HalfOdd,
            "TowardsZero" => RoundMode::TowardsZero,
            "AwayFromZero" => RoundMode::AwayFromZero,
            "NegativeInfinity" => RoundMode::NegativeInfinity,
            "PositiveInfinity" => RoundMode::PositiveInfinity,
            _ => return None,
        })
    }
}

const TWO53: f64 = 9_007_199_254_740_992.0;

/// Exact powers of ten representable as doubles.
const POW10: [f64; 23] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
];

/// `10^n` for `n >= 0`: exact up to 10^22, `pow` beyond (infinite past
/// 10^308).
fn pow10(n: u32) -> f64 {
    if n <= 22 {
        POW10[n as usize]
    } else {
        10f64.powf(n as f64)
    }
}

/// `round($num, $precision, $mode)` for floats (ints are rounded as floats).
///
/// Observed on PHP 8.5 (tests round_prerounding.phpt, round_gh12143_*.phpt
/// and the `round` cases of tests/fixtures/math.txt):
/// - the value is scaled by `10^precision` in double arithmetic and
///   returned unchanged when the scaled value reaches 2^53 (no digits left
///   to round) or the scale overflows;
/// - for the half modes, a value counts as exactly half way when it equals
///   the double computed for the half-way point `(floor + 0.5) / 10^p`, so
///   `round(0.285, 2)` is 0.29 while `round(0.49999999999999994)` is 0;
///   between 2^52 and 2^53 that half-way point collapses onto the value
///   itself, so `round(7.0, 15)` is 7.000000000000001;
/// - the directed modes round the scaled double itself;
/// - the result is `k / 10^p` in double arithmetic, a zero result keeps the
///   value's sign except for precisions below -22.
pub fn round(value: f64, places: i64, mode: RoundMode) -> f64 {
    if !value.is_finite() || value == 0.0 {
        return value;
    }
    let neg = value < 0.0;
    let a = value.abs();
    let mode = match (mode, neg) {
        (RoundMode::NegativeInfinity, false) | (RoundMode::PositiveInfinity, true) => {
            RoundMode::TowardsZero
        }
        (RoundMode::NegativeInfinity, true) | (RoundMode::PositiveInfinity, false) => {
            RoundMode::AwayFromZero
        }
        (m, _) => m,
    };
    let p = places.clamp(-1000, 1000);
    let pow = pow10(p.unsigned_abs() as u32);
    let (tmp, scale_up) = if p >= 0 {
        if pow.is_infinite() {
            return value;
        }
        (a * pow, true)
    } else {
        (a / pow, false)
    };
    if !tmp.is_finite() || tmp >= TWO53 {
        return value;
    }
    // The double for the decimal `k * 10^-p`: one correctly rounded
    // operation while the power of ten is exact, a decimal parse beyond.
    let unscale = |k: f64| {
        if p.unsigned_abs() <= 22 || k == 0.0 {
            if scale_up { k / pow } else { k * pow }
        } else {
            format!("{}e{}", k as u64, -p).parse::<f64>().unwrap_or(0.0)
        }
    };
    let mut lo = tmp.floor();
    // The scaled double may have crossed an integer: settle `lo` so that
    // unscale(lo) <= a < unscale(lo + 1).
    if unscale(lo + 1.0) <= a {
        lo += 1.0;
    } else if lo > 0.0 && unscale(lo) > a {
        lo -= 1.0;
    }
    let k = if unscale(lo) == a && matches!(mode, RoundMode::TowardsZero | RoundMode::AwayFromZero)
    {
        lo
    } else {
        match mode {
            RoundMode::TowardsZero => lo,
            RoundMode::AwayFromZero => lo + 1.0,
            _ => {
                let half = if scale_up {
                    (lo + 0.5) / pow
                } else {
                    (lo + 0.5) * pow
                };
                if a > half {
                    lo + 1.0
                } else if a < half {
                    lo
                } else {
                    let up = match mode {
                        RoundMode::HalfAwayFromZero => true,
                        RoundMode::HalfTowardsZero => false,
                        RoundMode::HalfEven => lo % 2.0 != 0.0,
                        _ => lo % 2.0 == 0.0,
                    };
                    if up { lo + 1.0 } else { lo }
                }
            }
        }
    };
    if k == 0.0 && p < -22 {
        return 0.0;
    }
    let r = unscale(k);
    if neg { -r } else { r }
}

/// `intdiv()`.
pub fn intdiv(a: i64, b: i64) -> Result<i64, StdError> {
    if b == 0 {
        return Err(StdError::DivisionByZeroError("Division by zero".into()));
    }
    if a == i64::MIN && b == -1 {
        return Err(StdError::ArithmeticError(
            "Division of PHP_INT_MIN by -1 is not an integer".into(),
        ));
    }
    Ok(a / b)
}

/// `fmod()`: the C `fmod`, the remainder with the sign of the dividend.
pub fn fmod(a: f64, b: f64) -> f64 {
    a % b
}

/// An int or a float result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Number {
    /// A PHP int.
    Int(i64),
    /// A PHP float.
    Float(f64),
}

/// The deprecation `bindec()`, `octdec()`, `hexdec()` and `base_convert()`
/// raise for characters that are not digits of the base.
pub const INVALID_CHARS: &str =
    "Invalid characters passed for attempted conversion, these have been ignored";

/// Parses `s` as a number in `base` (2 to 36), as `bindec()`, `octdec()`,
/// `hexdec()` and `base_convert()` do: surrounding whitespace and a `0b`,
/// `0o` or `0x` prefix matching the base are allowed (base_convert
/// improvements RFC), other non-digits are skipped with a deprecation, and
/// values beyond the int range continue as a float.
pub fn parse_base(s: &[u8], base: u32, diags: &mut Vec<Diag>) -> Number {
    let is_ws = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c');
    let mut start = 0;
    let mut end = s.len();
    while start < end && is_ws(s[start]) {
        start += 1;
    }
    while end > start && is_ws(s[end - 1]) {
        end -= 1;
    }
    let mut t = &s[start..end];
    if t.len() >= 2 && t[0] == b'0' {
        let p = t[1].to_ascii_lowercase();
        if (p == b'x' && base == 16) || (p == b'o' && base == 8) || (p == b'b' && base == 2) {
            t = &t[2..];
        }
    }
    let mut n: i64 = 0;
    let mut f: Option<f64> = None;
    let mut invalid = false;
    for &c in t {
        let d = match c {
            b'0'..=b'9' => (c - b'0') as u32,
            b'a'..=b'z' => (c - b'a') as u32 + 10,
            b'A'..=b'Z' => (c - b'A') as u32 + 10,
            _ => 99,
        };
        if d >= base {
            invalid = true;
            continue;
        }
        match f {
            Some(ref mut f) => *f = *f * base as f64 + d as f64,
            None => match n
                .checked_mul(base as i64)
                .and_then(|v| v.checked_add(d as i64))
            {
                Some(v) => n = v,
                None => f = Some(n as f64 * base as f64 + d as f64),
            },
        }
    }
    if invalid {
        diags.push(Diag::Deprecated(INVALID_CHARS.into()));
    }
    match f {
        Some(f) => Number::Float(f),
        None => Number::Int(n),
    }
}

const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// Writes a non-negative integer in `base`, lowercase.
pub fn to_base(mut v: u64, base: u32) -> Vec<u8> {
    if v == 0 {
        return b"0".to_vec();
    }
    let mut out = Vec::new();
    while v > 0 {
        out.push(DIGITS[(v % base as u64) as usize]);
        v /= base as u64;
    }
    out.reverse();
    out
}

/// `decbin()`, `decoct()`, `dechex()`: the two's complement bits of
/// negative numbers (manual: "treated as unsigned").
pub fn dec_to_base(v: i64, base: u32) -> Vec<u8> {
    to_base(v as u64, base)
}

/// Writes a float beyond the int range in `base`. Observed on PHP 8.5: the
/// digits are peeled with `fmod` and float division, so they drift from the
/// exact value ( `base_convert("99999999999999999999999", 10, 36)` is
/// `ga894a06abs40sw`, not the exact `ga894a06ac1q2gw`).
fn float_to_base(f: f64, base: u32) -> Vec<u8> {
    let f = f.abs();
    if !f.is_finite() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut v = f;
    loop {
        out.push(DIGITS[(v % base as f64) as usize]);
        v /= base as f64;
        if v < 1.0 {
            break;
        }
    }
    out.reverse();
    out
}

/// `base_convert()`.
pub fn base_convert(
    num: &[u8],
    from_base: i64,
    to_base_: i64,
    diags: &mut Vec<Diag>,
) -> Result<Vec<u8>, StdError> {
    if !(2..=36).contains(&from_base) {
        return Err(StdError::ValueError(
            "base_convert(): Argument #2 ($from_base) must be between 2 and 36 (inclusive)".into(),
        ));
    }
    if !(2..=36).contains(&to_base_) {
        return Err(StdError::ValueError(
            "base_convert(): Argument #3 ($to_base) must be between 2 and 36 (inclusive)".into(),
        ));
    }
    Ok(match parse_base(num, from_base as u32, diags) {
        Number::Int(n) => to_base(n as u64, to_base_ as u32),
        Number::Float(f) => float_to_base(f, to_base_ as u32),
    })
}

/// Inserts `sep` between groups of three digits of `int_digits`.
fn group_thousands(out: &mut Vec<u8>, int_digits: &[u8], sep: &[u8]) {
    let n = int_digits.len();
    for (i, &d) in int_digits.iter().enumerate() {
        if i > 0 && (n - i).is_multiple_of(3) {
            out.extend_from_slice(sep);
        }
        out.push(d);
    }
}

fn assemble(neg: bool, digits: &str, dec_point: &[u8], thousands_sep: &[u8]) -> Vec<u8> {
    let (int_part, frac) = match digits.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (digits, None),
    };
    let mut out = Vec::with_capacity(digits.len() + 8);
    if neg {
        out.push(b'-');
    }
    group_thousands(&mut out, int_part.as_bytes(), thousands_sep);
    if let Some(f) = frac {
        out.extend_from_slice(dec_point);
        out.extend_from_slice(f.as_bytes());
    }
    out
}

/// `number_format()` of a float. The value is rounded as by `round()` (half
/// away from zero), negative `decimals` round to tens, hundreds and so on,
/// and a result that is zero never shows a minus sign (PHP manual:
/// number_format changelog; tests math/number_format_*.phpt).
pub fn number_format(num: f64, decimals: i64, dec_point: &[u8], thousands_sep: &[u8]) -> Vec<u8> {
    if num.is_nan() {
        return b"nan".to_vec();
    }
    if num.is_infinite() {
        // Observed on PHP 8.5: both infinities print as "inf".
        return b"inf".to_vec();
    }
    let r = round(num, decimals, RoundMode::HalfAwayFromZero);
    let dec = decimals.clamp(0, 1000) as usize;
    let digits = crate::float::fixed_abs(r, dec);
    let neg = r < 0.0 && digits.bytes().any(|b| (b'1'..=b'9').contains(&b));
    assemble(neg, &digits, dec_point, thousands_sep)
}

/// `number_format()` of an int: exact, including negative `decimals`
/// rounding past the int range (observed on PHP 8.5:
/// `number_format(PHP_INT_MAX, -3)` is `9,223,372,036,854,776,000`).
pub fn number_format_int(
    num: i64,
    decimals: i64,
    dec_point: &[u8],
    thousands_sep: &[u8],
) -> Vec<u8> {
    let mut v = num as i128;
    if decimals < 0 {
        let p = decimals.saturating_neg().min(40) as u32;
        let m = 10i128.pow(p.min(38));
        if p > 19 {
            v = 0;
        } else {
            let rem = v % m;
            v -= rem;
            if rem.abs() * 2 >= m {
                v += if rem < 0 { -m } else { m };
            }
        }
    }
    let mut digits = v.unsigned_abs().to_string();
    if decimals > 0 {
        digits.push('.');
        digits.extend(std::iter::repeat_n('0', decimals.min(1000) as usize));
    }
    assemble(v < 0, &digits, dec_point, thousands_sep)
}

#[cfg(test)]
mod tests {
    use super::*;

    // test ext/standard/tests/math/round_modes.phpt (sample)
    #[test]
    fn round_basics() {
        use RoundMode::*;
        assert_eq!(round(2.5, 0, HalfAwayFromZero), 3.0);
        assert_eq!(round(-2.5, 0, HalfAwayFromZero), -3.0);
        assert_eq!(round(2.5, 0, HalfTowardsZero), 2.0);
        assert_eq!(round(2.5, 0, HalfEven), 2.0);
        assert_eq!(round(3.5, 0, HalfEven), 4.0);
        assert_eq!(round(2.5, 0, HalfOdd), 3.0);
        assert_eq!(round(0.65, 1, HalfAwayFromZero), 0.7);
        assert_eq!(round(0.65, 1, HalfTowardsZero), 0.6);
        assert_eq!(round(15.51, -1, HalfAwayFromZero), 20.0);
        assert_eq!(
            round(-0.0001, 0, HalfAwayFromZero).to_bits(),
            (-0.0f64).to_bits()
        );
        assert_eq!(round(0.285, 2, HalfAwayFromZero), 0.29);
        assert_eq!(round(0.49999999999999994, 0, HalfAwayFromZero), 0.0);
        assert_eq!(round(1.0, -2147483648, HalfAwayFromZero), 0.0);
        assert_eq!(round(1.0, i64::MAX, HalfAwayFromZero), 1.0);
    }

    // test ext/standard/tests/math/round_gh12143_expand_rounding_target.phpt
    #[test]
    fn round_expand_target() {
        use RoundMode::*;
        assert_eq!(
            round(0.12345678901234565, 16, HalfAwayFromZero),
            0.1234567890123457
        );
        assert_eq!(
            round(12345678901234565.0, -1, HalfAwayFromZero),
            12345678901234570.0
        );
        assert_eq!(
            round(12345678901234565.0, -1, HalfTowardsZero),
            12345678901234560.0
        );
        assert_eq!(round(4503599627370495.5, 0, HalfEven), 4503599627370496.0);
        assert_eq!(round(4503599627370495.5, 0, HalfOdd), 4503599627370495.0);
        assert_eq!(
            round(0.1234567890123456, 16, AwayFromZero),
            0.1234567890123456
        );
        assert_eq!(
            round(12345678901234567.0, -1, AwayFromZero),
            12345678901234570.0
        );
        assert_eq!(
            round(-12345678901234564.0, -1, PositiveInfinity),
            -12345678901234560.0
        );
        assert_eq!(
            round(-12345678901234564.0, -1, NegativeInfinity),
            -12345678901234570.0
        );
    }

    #[test]
    fn base() {
        let mut d = Vec::new();
        assert_eq!(parse_base(b"0xFf", 16, &mut d), Number::Int(255));
        assert!(d.is_empty());
        assert_eq!(parse_base(b"1xff ", 16, &mut d), Number::Int(511));
        assert_eq!(d.len(), 1);
        assert_eq!(
            base_convert(b"99999999999999999999999", 10, 36, &mut d).unwrap(),
            b"ga894a06abs40sw"
        );
        assert_eq!(
            base_convert(b"ffffffffffffffffffff", 16, 10, &mut d).unwrap(),
            b"1208925819614629446642046"
        );
        assert_eq!(dec_to_base(-1, 16), b"ffffffffffffffff");
    }

    // test ext/standard/tests/math/number_format_decimals.phpt (sample)
    #[test]
    fn number_formats() {
        assert_eq!(number_format(1515.1, -3, b".", b","), b"2,000");
        assert_eq!(number_format(-1.5151, -1, b".", b","), b"0");
        assert_eq!(number_format(-0.4, 0, b".", b","), b"0");
        assert_eq!(number_format(0.5, 2, b"", b""), b"050");
        assert_eq!(number_format_int(15151, -2, b".", b","), b"15,200");
        assert_eq!(number_format_int(-15151, -4, b".", b","), b"-20,000");
        assert_eq!(
            number_format_int(i64::MAX, -3, b".", b","),
            b"9,223,372,036,854,776,000"
        );
        assert_eq!(number_format_int(999, i64::MIN, b".", b","), b"0");
    }
}
