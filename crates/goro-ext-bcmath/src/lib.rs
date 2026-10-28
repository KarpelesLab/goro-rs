//! PHP's bcmath extension (PHP manual: BCMath Arbitrary Precision
//! Mathematics) over byte strings: the `bc*()` functions and the
//! `BcMath\Number` class (PHP 8.4+).
//!
//! Behaviour follows the PHP manual, the RFCs "Support object type in
//! BCMath" and "Add bcround, bcfloor, bcceil" / "bcdivmod", the expected
//! output of php-src's `ext/bcmath/tests/*.phpt`, and observations of
//! PHP 8.5 (the fixture in `tests/fixtures` was generated once from the
//! reference binary by `tests/fixtures/bcmath_driver.php`).
//!
//! # Engine wiring
//!
//! * The functions in [`functions`] take the PHP arguments after the
//!   engine's usual coercion to string/int (`bcadd(1.5, 2)` passes `"1.5"`,
//!   null-to-string deprecations are the engine's), the `?int $scale`
//!   argument as `Option<i64>`, and the current `bcmath.scale` value.
//!   They return the result string or a [`BcError`] to throw. The `_into`
//!   forms ([`bcadd_into`], ...) append to a caller buffer: keep one
//!   scratch `Vec` and copy the result into the PHP string, so a call costs
//!   one allocation.
//! * Argument order of errors is PHP's: `$scale` range first, then each
//!   number's well-formedness, then the function's own checks (fraction
//!   and sign of exponents, division by zero).
//! * Operators on `BcMath\Number` ([`Number::binary_op`]) take int, string
//!   or `Number` operands; the engine first converts floats and bools to
//!   int as PHP does (with the "Implicit conversion" deprecation) and
//!   raises "Unsupported operand types" for other types itself. PHP 8.5's
//!   `%` additionally checks a string left operand as an int before the
//!   `Number` handler runs: a non-numeric one throws `TypeError:
//!   Unsupported operand types: string % BcMath\Number`, a float-string
//!   one emits the float-to-int deprecation (observed on PHP 8.5); that
//!   check is the engine's. [`Number::compare_operands`] is `None` for a
//!   malformed string (every ordered comparison false); comparisons with
//!   null, arrays and other objects follow the engine's object rules.
//! * `bcmath.scale` (default `"0"`) is validated with [`parse_scale_ini`];
//!   `bcscale($n)` returns the old default and, through [`bcscale`],
//!   validates the new one, which the engine stores back into the ini
//!   setting (so `ini_get('bcmath.scale')` follows it).
//! * `BcMath\Number` is a `final readonly` class implementing `Stringable`
//!   with properties `string $value` and `int $scale`; the engine's object
//!   wraps a [`Number`] and forwards methods, operators (`+ - * / % **`,
//!   `<=>`, `++`, `--`, unary `-`), casts and serialization to it. See
//!   [`Number`] for the details (error messages, scale rules).
//! * [`set_max_result_len`] lets the engine bound result sizes by its
//!   memory limit: an operation whose result would exceed it fails with
//!   [`BcErrorKind::OutOfMemory`], which the engine reports as PHP's
//!   "Allowed memory size of N bytes exhausted" fatal error (test
//!   ext/bcmath/tests/gh17398.phpt).

mod arith;
mod dec;
pub mod functions;
mod limbs;
mod number;

use std::cell::Cell;

pub use dec::RoundingMode;
pub use functions::{
    MAX_SCALE, bcadd, bcadd_into, bcceil, bcceil_into, bccomp, bcdiv, bcdiv_into, bcdivmod,
    bcfloor, bcfloor_into, bcmod, bcmod_into, bcmul, bcmul_into, bcpow, bcpow_into, bcpowmod,
    bcpowmod_into, bcround, bcround_into, bcscale, bcsqrt, bcsqrt_into, bcsub, bcsub_into,
};
pub use number::{BinOp, CLASS_NAME, Number, NumberArg};

/// The class of a Throwable (or fatal error) raised by bcmath.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BcErrorKind {
    /// `ValueError`
    ValueError,
    /// `DivisionByZeroError`
    DivisionByZero,
    /// `Exception` (`BcMath\Number::__unserialize()` with bad data)
    Exception,
    /// Not a Throwable: the result would exceed [`max_result_len`]; the
    /// engine raises its "Allowed memory size exhausted" fatal error.
    OutOfMemory,
}

/// An error bcmath raises.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BcError {
    pub kind: BcErrorKind,
    pub message: String,
    /// For [`BcErrorKind::OutOfMemory`]: the size that was refused.
    pub bytes: usize,
}

impl BcError {
    #[cold]
    pub(crate) fn value(message: impl Into<String>) -> BcError {
        BcError {
            kind: BcErrorKind::ValueError,
            message: message.into(),
            bytes: 0,
        }
    }
    #[cold]
    pub(crate) fn division_by_zero(message: impl Into<String>) -> BcError {
        BcError {
            kind: BcErrorKind::DivisionByZero,
            message: message.into(),
            bytes: 0,
        }
    }
    #[cold]
    pub(crate) fn exception(message: impl Into<String>) -> BcError {
        BcError {
            kind: BcErrorKind::Exception,
            message: message.into(),
            bytes: 0,
        }
    }
    #[cold]
    pub(crate) fn out_of_memory(bytes: usize) -> BcError {
        BcError {
            kind: BcErrorKind::OutOfMemory,
            message: format!("result of {bytes} bytes exceeds the memory limit"),
            bytes,
        }
    }

    /// The PHP class name (`"Exception"` for the unserialize error; the
    /// out-of-memory case has none and reports `"Error"`).
    pub fn class_name(&self) -> &'static str {
        match self.kind {
            BcErrorKind::ValueError => "ValueError",
            BcErrorKind::DivisionByZero => "DivisionByZeroError",
            BcErrorKind::Exception => "Exception",
            BcErrorKind::OutOfMemory => "Error",
        }
    }
}

impl std::fmt::Display for BcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.class_name(), self.message)
    }
}

impl std::error::Error for BcError {}

thread_local! {
    static MAX_RESULT_LEN: Cell<usize> = const { Cell::new(1 << 31) };
}

/// The largest result, in bytes, an operation may build (default 2 GiB).
pub fn max_result_len() -> usize {
    MAX_RESULT_LEN.with(Cell::get)
}

/// Sets [`max_result_len`] for this thread (the engine derives it from
/// `memory_limit`).
pub fn set_max_result_len(len: usize) {
    MAX_RESULT_LEN.with(|c| c.set(len));
}

/// Whether `s` is a well-formed bcmath number: an optional sign, digits,
/// and an optional `.` with digits; either digit run may be empty, and the
/// string ends at its first NUL byte (observed on PHP 8.5).
pub fn is_well_formed(s: &[u8]) -> bool {
    dec::parse(s).is_some()
}

/// The outcome of setting `bcmath.scale`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IniScale {
    /// The new scale, or `None` when the value is refused (`ini_set()`
    /// returns false and the old value stays).
    pub scale: Option<u32>,
    /// An `E_WARNING` message to emit first, if any.
    pub warning: Option<String>,
}

/// Parses a `bcmath.scale` ini value. PHP reads it as an ini quantity
/// (observed on PHP 8.5): surrounding whitespace, an optional sign, a
/// `0x`/`0o`/`0b` or leading-`0` (octal) base prefix, and an optional
/// `k`/`m`/`g` multiplier (powers of 1024); trailing garbage gives a
/// warning and keeps the leading digits. Values outside 0..=2147483647
/// are refused.
pub fn parse_scale_ini(value: &[u8]) -> IniScale {
    let shown = String::from_utf8_lossy(value);
    let warn = |reason: &str, interp: Option<&str>| {
        let mut w =
            format!("Invalid \"bcmath.scale\" setting. Invalid quantity \"{shown}\": {reason}");
        match interp {
            Some(v) => w.push_str(&format!(
                ", interpreting as \"{v}\" for backwards compatibility"
            )),
            None => w.push_str(", using overflow result for backwards compatibility"),
        }
        w
    };
    let is_ws = |c: &u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c');
    let mut s = value;
    while let Some((f, rest)) = s.split_first() {
        if is_ws(f) {
            s = rest;
        } else {
            break;
        }
    }
    while let Some((l, rest)) = s.split_last() {
        if is_ws(l) {
            s = rest;
        } else {
            break;
        }
    }
    if s.is_empty() {
        return IniScale {
            scale: Some(0),
            warning: None,
        };
    }
    let mut factor: u32 = 0;
    let mut body = s;
    if let Some((&last, rest)) = s.split_last() {
        factor = match last {
            b'k' | b'K' => 10,
            b'm' | b'M' => 20,
            b'g' | b'G' => 30,
            _ => 0,
        };
        if factor > 0 {
            body = rest;
            while let Some((l, r)) = body.split_last() {
                if is_ws(l) {
                    body = r;
                } else {
                    break;
                }
            }
        }
    }
    let (neg, digits_part) = match body.first() {
        Some(b'-') => (true, &body[1..]),
        Some(b'+') => (false, &body[1..]),
        _ => (false, body),
    };
    let (radix, digits, prefixed) = match digits_part {
        [b'0', b'x' | b'X', rest @ ..] => (16, rest, true),
        [b'0', b'o' | b'O', rest @ ..] => (8, rest, true),
        [b'0', b'b' | b'B', rest @ ..] => (2, rest, true),
        [b'0', rest @ ..] if !rest.is_empty() => (8, rest, false),
        _ => (10, digits_part, false),
    };
    let mut value: u128 = 0;
    let mut used = 0;
    let mut overflow = false;
    for &c in digits {
        let d = match c {
            b'0'..=b'9' => (c - b'0') as u32,
            b'a'..=b'f' => (c - b'a' + 10) as u32,
            b'A'..=b'F' => (c - b'A' + 10) as u32,
            _ => break,
        };
        if d >= radix {
            break;
        }
        value = value * radix as u128 + d as u128;
        if value > u64::MAX as u128 {
            overflow = true;
            value = u64::MAX as u128 + 1;
        }
        used += 1;
    }
    let mut warning = None;
    if used == 0 && !(radix == 8 && !prefixed) {
        let reason = if prefixed {
            "no digits after base prefix"
        } else {
            "no valid leading digits"
        };
        return IniScale {
            scale: Some(0),
            warning: Some(warn(reason, Some("0"))),
        };
    }
    if used < digits.len() && factor == 0 {
        // Unknown trailing text: PHP names its last character.
        let last = *s.last().unwrap_or(&b'?') as char;
        let shown_val = format!("{}{}", if neg { "-" } else { "" }, value);
        warning = Some(warn(
            &format!("unknown multiplier \"{last}\""),
            Some(&shown_val),
        ));
    }
    if overflow {
        return IniScale {
            scale: None,
            warning: Some(warn("value is out of range", None)),
        };
    }
    let v = value << factor;
    if neg && v != 0 {
        return IniScale {
            scale: None,
            warning,
        };
    }
    IniScale {
        scale: (v <= MAX_SCALE as u128).then_some(v as u32),
        warning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_quantities() {
        let p = |s: &str| parse_scale_ini(s.as_bytes());
        assert_eq!(p("5").scale, Some(5));
        assert_eq!(p("010").scale, Some(8));
        assert_eq!(p("0x1f").scale, Some(31));
        assert_eq!(p("0b11").scale, Some(3));
        assert_eq!(p("1k").scale, Some(1024));
        assert_eq!(p("1 k").scale, Some(1024));
        assert_eq!(p("2G").scale, None);
        assert_eq!(p("-1").scale, None);
        assert_eq!(p("-0").scale, Some(0));
        assert_eq!(p("2147483648").scale, None);
        let w = p("5abc");
        assert_eq!(w.scale, Some(5));
        assert_eq!(
            w.warning.as_deref(),
            Some(
                "Invalid \"bcmath.scale\" setting. Invalid quantity \"5abc\": unknown multiplier \"c\", interpreting as \"5\" for backwards compatibility"
            )
        );
        let w = p("abc");
        assert_eq!(w.scale, Some(0));
        assert!(w.warning.unwrap().ends_with(
            "no valid leading digits, interpreting as \"0\" for backwards compatibility"
        ));
        assert_eq!(p("0x").scale, Some(0));
        assert_eq!(p("18446744073709551616").scale, None);
        assert!(p("18446744073709551616").warning.is_some());
        assert_eq!(p("9223372036854775807").warning, None);
    }
}
