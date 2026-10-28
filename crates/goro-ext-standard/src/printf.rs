//! The printf family's formatting engine: `sprintf`, `printf`, `vsprintf`,
//! `vprintf`, `fprintf` and `vfprintf` all format through [`format`].
//!
//! Behaviour sources: PHP manual `sprintf` page (specifier syntax
//! `%[argnum$][flags][width][.precision]specifier`), tests
//! ext/standard/tests/strings/sprintf_*.phpt, printf_*.phpt, and
//! observations on PHP 8.5 kept in `tests/fixtures/printf*.txt`.

use crate::error::{Diag, StdError};
use crate::float;

/// The arguments of a printf call, converted lazily: each specifier asks for
/// the conversion it needs, in format order, so the wiring layer emits any
/// conversion diagnostics (for example "The float ... is not representable as
/// an int") at the right time.
pub trait FormatArgs {
    /// Number of arguments after the format.
    fn count(&self) -> usize;
    /// The argument converted as by an `(int)` cast (`%d %u %c %o %x %X %b`).
    fn int(&mut self, i: usize) -> i64;
    /// The argument converted as by a `(float)` cast (`%e %E %f %F %g %G %h %H`).
    fn float(&mut self, i: usize) -> f64;
    /// Appends the argument converted to a string (`%s`).
    fn string(&mut self, i: usize, out: &mut Vec<u8>);
    /// For `*` widths and precisions: the argument's value if it is an int,
    /// `None` for any other type (strings and floats are refused).
    fn exact_int(&mut self, i: usize) -> Option<i64>;
}

/// A ready-made argument, for callers holding plain values.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    /// A PHP int.
    Int(i64),
    /// A PHP float.
    Float(f64),
    /// A PHP string (its numeric conversion follows PHP's leading-numeric rule).
    Str(Vec<u8>),
}

/// Leading-numeric prefix of a string as a float, PHP's `(float)` on strings:
/// leading whitespace, then the longest numeric prefix, 0 when there is none.
pub fn str_to_float(s: &[u8]) -> f64 {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c') {
        i += 1;
    }
    let start = i;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        i += 1;
    }
    let digits_start = i;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    let mut ndigits = i - digits_start;
    if i < s.len() && s[i] == b'.' {
        let j = i + 1;
        let mut k = j;
        while k < s.len() && s[k].is_ascii_digit() {
            k += 1;
        }
        if ndigits > 0 || k > j {
            ndigits += k - j;
            i = k;
        }
    }
    if ndigits == 0 {
        return 0.0;
    }
    if i < s.len() && (s[i] == b'e' || s[i] == b'E') {
        let mut k = i + 1;
        if k < s.len() && (s[k] == b'+' || s[k] == b'-') {
            k += 1;
        }
        let es = k;
        while k < s.len() && s[k].is_ascii_digit() {
            k += 1;
        }
        if k > es {
            i = k;
        }
    }
    std::str::from_utf8(&s[start..i])
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or(0.0)
}

/// PHP's `(int)` of a float on 64-bit platforms: truncation, with values out
/// of range wrapping modulo 2^64 (observed on PHP 8.5: `(int)1e19` is
/// `-8446744073709551616`) and NaN/infinities giving 0.
pub fn float_to_int(f: f64) -> i64 {
    if !f.is_finite() {
        return 0;
    }
    let t = f.trunc();
    let two63 = (1u64 << 63) as f64;
    if (-two63..two63).contains(&t) {
        return t as i64;
    }
    // Out of range: reduce modulo 2^64. Every float this large is an integer.
    let m = t % 18_446_744_073_709_551_616.0;
    let m = if m < 0.0 {
        m + 18_446_744_073_709_551_616.0
    } else {
        m
    };
    (m as u64) as i64
}

/// PHP's `(int)` of a string: the leading integer prefix, or the integer
/// value of a leading float (`"1e3"` is 1000), saturating on overflow.
pub fn str_to_int(s: &[u8]) -> i64 {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c') {
        i += 1;
    }
    let mut j = i;
    if j < s.len() && (s[j] == b'+' || s[j] == b'-') {
        j += 1;
    }
    let ds = j;
    while j < s.len() && s[j].is_ascii_digit() {
        j += 1;
    }
    let float_form = j < s.len() && matches!(s[j], b'.' | b'e' | b'E');
    if float_form {
        let f = str_to_float(&s[i..]);
        // `as` saturates, and the prefix may be an integer followed by junk.
        return f as i64;
    }
    if j == ds {
        return 0;
    }
    let neg = s[i] == b'-';
    let mut v: i64 = 0;
    for &c in &s[ds..j] {
        let d = (c - b'0') as i64;
        let next = v.checked_mul(10).and_then(|v| {
            if neg {
                v.checked_sub(d)
            } else {
                v.checked_add(d)
            }
        });
        match next {
            Some(n) => v = n,
            None => return if neg { i64::MIN } else { i64::MAX },
        }
    }
    v
}

/// Shortest PHP float-to-string with `precision = -1`: used by [`Arg`] for
/// `%s` of floats (`1.0E+25`, `0.1`, `-0`, `INF`, `NAN`).
pub fn float_to_string(f: f64) -> Vec<u8> {
    let mut out = Vec::new();
    if f.is_nan() {
        return b"NAN".to_vec();
    }
    if f.is_infinite() {
        return if f < 0.0 {
            b"-INF".to_vec()
        } else {
            b"INF".to_vec()
        };
    }
    if f.is_sign_negative() {
        out.push(b'-');
    }
    float::push_general(&mut out, &float::shortest(f), 15, b'E', b'.');
    out
}

impl FormatArgs for [Arg] {
    fn count(&self) -> usize {
        self.len()
    }
    fn int(&mut self, i: usize) -> i64 {
        match &self[i] {
            Arg::Int(v) => *v,
            Arg::Float(f) => float_to_int(*f),
            Arg::Str(s) => str_to_int(s),
        }
    }
    fn float(&mut self, i: usize) -> f64 {
        match &self[i] {
            Arg::Int(v) => *v as f64,
            Arg::Float(f) => *f,
            Arg::Str(s) => str_to_float(s),
        }
    }
    fn string(&mut self, i: usize, out: &mut Vec<u8>) {
        match &self[i] {
            Arg::Int(v) => out.extend_from_slice(v.to_string().as_bytes()),
            Arg::Float(f) => out.extend_from_slice(&float_to_string(*f)),
            Arg::Str(s) => out.extend_from_slice(s),
        }
    }
    fn exact_int(&mut self, i: usize) -> Option<i64> {
        match &self[i] {
            Arg::Int(v) => Some(*v),
            _ => None,
        }
    }
}

/// How the arguments were passed, which changes the missing-argument error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgsKind {
    /// `sprintf`, `printf`, `fprintf`: the arguments follow the format.
    Variadic,
    /// `vsprintf`, `vprintf`, `vfprintf`: the arguments are an array.
    Array,
}

/// Locale-dependent settings. `%f`, `%g` and `%G` use the locale's decimal
/// point; `%F`, `%e`, `%E`, `%h` and `%H` never do (PHP manual: sprintf;
/// tests sprintf_f_3.phpt, printf_h_H.phpt).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Locale {
    /// The `LC_NUMERIC` decimal point.
    pub decimal_point: u8,
}

impl Default for Locale {
    fn default() -> Self {
        Locale {
            decimal_point: b'.',
        }
    }
}

/// The most digits PHP prints after the decimal point (observed on PHP 8.5:
/// "Requested precision of 60 digits was truncated to PHP maximum of 53
/// digits").
pub const MAX_FLOAT_PRECISION: i64 = 53;

const INT_MAX: i64 = i32::MAX as i64;

fn value_error(m: &str) -> StdError {
    StdError::ValueError(m.to_string())
}

struct Spec {
    left: bool,
    plus: bool,
    pad: u8,
    width: i64,
    precision: Option<i64>,
}

fn missing(kind: ArgsKind, needed: usize, given: usize) -> StdError {
    match kind {
        ArgsKind::Variadic => StdError::ArgumentCountError(format!(
            "{} arguments are required, {} given",
            needed + 1,
            given + 1
        )),
        ArgsKind::Array => StdError::ValueError(format!(
            "The arguments array must contain {needed} items, {given} given"
        )),
    }
}

/// Parses a decimal number at `fmt[*pos..]`; `None` when it reaches
/// `INT_MAX` (PHP refuses widths, precisions and argnums of 2^31-1 and up).
fn parse_num(fmt: &[u8], pos: &mut usize) -> Option<i64> {
    let mut v: i64 = 0;
    let mut overflow = false;
    while *pos < fmt.len() && fmt[*pos].is_ascii_digit() {
        if !overflow {
            v = v * 10 + (fmt[*pos] - b'0') as i64;
            if v >= INT_MAX {
                overflow = true;
            }
        }
        *pos += 1;
    }
    if overflow { None } else { Some(v) }
}

const ARGNUM_ERROR: &str =
    "Argument number specifier must be greater than zero and less than 2147483647";

/// Parses an optional `N$` at `fmt[*pos..]`, returning the zero-based argument
/// number.
fn parse_argnum(fmt: &[u8], pos: &mut usize) -> Result<Option<usize>, StdError> {
    let mut p = *pos;
    // Test bug69751.phpt: "%$s" is an invalid argument number.
    if p < fmt.len() && fmt[p] == b'$' {
        return Err(value_error(ARGNUM_ERROR));
    }
    if p >= fmt.len() || !fmt[p].is_ascii_digit() {
        return Ok(None);
    }
    let n = parse_num(fmt, &mut p);
    if p < fmt.len() && fmt[p] == b'$' {
        *pos = p + 1;
        match n {
            Some(n) if n > 0 => Ok(Some((n - 1) as usize)),
            _ => Err(value_error(ARGNUM_ERROR)),
        }
    } else {
        Ok(None)
    }
}

/// The number of arguments the format refers to, for the missing-argument
/// message. Observed on PHP 8.5: the count covers the whole format, not just
/// the specifiers up to the missing one (`sprintf("%s%s%s", "one")` reports
/// "4 arguments are required, 2 given"), and a `%` conversion after
/// modifiers counts twice (`sprintf("%5%")` reports 3 required although
/// `sprintf("%5%", "a")` works).
fn required_args(fmt: &[u8]) -> usize {
    let mut max = 0usize;
    let mut currarg = 0usize;
    let mut pos = 0;
    let mut take = |argnum: Option<usize>, currarg: &mut usize| {
        let idx = argnum.unwrap_or_else(|| {
            *currarg += 1;
            *currarg - 1
        });
        max = max.max(idx + 1);
    };
    while pos < fmt.len() {
        if fmt[pos] != b'%' {
            pos += 1;
            continue;
        }
        pos += 1;
        if pos < fmt.len() && fmt[pos] == b'%' {
            pos += 1;
            continue;
        }
        let Ok(argnum) = parse_argnum(fmt, &mut pos) else {
            break;
        };
        while pos < fmt.len() && matches!(fmt[pos], b'-' | b'+' | b'0' | b' ' | b'\'') {
            if fmt[pos] == b'\'' {
                pos += 1;
            }
            pos += 1;
        }
        for dot in [false, true] {
            if dot {
                if pos < fmt.len() && fmt[pos] == b'.' {
                    pos += 1;
                } else {
                    break;
                }
            }
            if pos < fmt.len() && fmt[pos] == b'*' {
                pos += 1;
                // Star arguments are not counted (observed: `sprintf("%*s")`
                // reports 2 required).
                if parse_argnum(fmt, &mut pos).is_err() {
                    return max;
                }
            } else {
                while pos < fmt.len() && fmt[pos].is_ascii_digit() {
                    pos += 1;
                }
            }
        }
        if pos < fmt.len() && fmt[pos] == b'l' {
            pos += 1;
        }
        take(argnum, &mut currarg);
        // The conversion character is not consumed here: a `%` there starts
        // the next specifier.
    }
    max
}

/// Formats `fmt` with `args`, appending to `out`. Warnings go to `diags`
/// (without the `sprintf(): ` prefix).
///
/// Errors are those of PHP 8.5: `ValueError` for malformed specifiers,
/// `ArgumentCountError` (or, for [`ArgsKind::Array`], `ValueError`) for
/// missing arguments.
pub fn format<A: FormatArgs + ?Sized>(
    fmt: &[u8],
    args: &mut A,
    kind: ArgsKind,
    locale: Locale,
    out: &mut Vec<u8>,
    diags: &mut Vec<Diag>,
) -> Result<(), StdError> {
    let nargs = args.count();
    let mut currarg: usize = 0;
    let mut pos = 0;
    while pos < fmt.len() {
        let c = fmt[pos];
        if c != b'%' {
            // Copy the literal run in one go.
            let end = fmt[pos..]
                .iter()
                .position(|&b| b == b'%')
                .map_or(fmt.len(), |p| pos + p);
            out.extend_from_slice(&fmt[pos..end]);
            pos = end;
            continue;
        }
        pos += 1;
        if pos < fmt.len() && fmt[pos] == b'%' {
            out.push(b'%');
            pos += 1;
            continue;
        }

        let argnum = parse_argnum(fmt, &mut pos)?;
        let mut spec = Spec {
            left: false,
            plus: false,
            pad: b' ',
            width: 0,
            precision: None,
        };
        // Flags, in any order (observed: "%'x-10d" and "%-'x10d" agree).
        while pos < fmt.len() {
            match fmt[pos] {
                b'-' => spec.left = true,
                b'+' => spec.plus = true,
                b'0' => spec.pad = b'0',
                b' ' => spec.pad = b' ',
                b'\'' => {
                    if pos + 1 >= fmt.len() {
                        return Err(value_error("Missing padding character"));
                    }
                    pos += 1;
                    spec.pad = fmt[pos];
                }
                _ => break,
            }
            pos += 1;
        }

        // Width.
        if pos < fmt.len() && fmt[pos] == b'*' {
            pos += 1;
            let idx = match parse_argnum(fmt, &mut pos)? {
                Some(n) => n,
                None => {
                    currarg += 1;
                    currarg - 1
                }
            };
            if idx >= nargs {
                return Err(missing(kind, required_args(fmt).max(idx + 1), nargs));
            }
            let w = args
                .exact_int(idx)
                .ok_or_else(|| value_error("Width must be an integer"))?;
            if !(0..=INT_MAX).contains(&w) {
                return Err(value_error("Width must be between 0 and 2147483647"));
            }
            spec.width = w;
        } else if pos < fmt.len() && fmt[pos].is_ascii_digit() {
            spec.width = parse_num(fmt, &mut pos)
                .ok_or_else(|| value_error("Width must be between 0 and 2147483647"))?;
        }

        // Precision.
        if pos < fmt.len() && fmt[pos] == b'.' {
            pos += 1;
            if pos < fmt.len() && fmt[pos] == b'*' {
                pos += 1;
                let idx = match parse_argnum(fmt, &mut pos)? {
                    Some(n) => n,
                    None => {
                        currarg += 1;
                        currarg - 1
                    }
                };
                if idx >= nargs {
                    return Err(missing(kind, required_args(fmt).max(idx + 1), nargs));
                }
                let p = args
                    .exact_int(idx)
                    .ok_or_else(|| value_error("Precision must be an integer"))?;
                if !(-1..=INT_MAX).contains(&p) {
                    return Err(value_error("Precision must be between -1 and 2147483647"));
                }
                spec.precision = Some(p);
            } else if pos < fmt.len() && fmt[pos].is_ascii_digit() {
                spec.precision =
                    Some(parse_num(fmt, &mut pos).ok_or_else(|| {
                        value_error("Precision must be between 0 and 2147483647")
                    })?);
            } else {
                spec.precision = Some(0);
            }
        }

        // One `l` length modifier is accepted and ignored (observed: "%ld"
        // works, "%lld" is an unknown specifier "l").
        if pos < fmt.len() && fmt[pos] == b'l' {
            pos += 1;
        }

        let idx = match argnum {
            Some(n) => n,
            None => {
                currarg += 1;
                currarg - 1
            }
        };
        if idx >= nargs {
            return Err(missing(kind, required_args(fmt).max(idx + 1), nargs));
        }
        if pos >= fmt.len() {
            return Err(value_error("Missing format specifier at end of string"));
        }
        let conv = fmt[pos];
        pos += 1;

        if spec.precision == Some(-1) && !matches!(conv, b'g' | b'G' | b'h' | b'H') {
            return Err(value_error(
                "Precision -1 is only supported for %g, %G, %h and %H",
            ));
        }

        match conv {
            b's' => {
                let mut s = Vec::new();
                args.string(idx, &mut s);
                if let Some(p) = spec.precision
                    && (p as usize) < s.len()
                {
                    s.truncate(p as usize);
                }
                pad_into(out, &s, &spec, false)?;
            }
            b'd' => {
                let v = args.int(idx);
                let mut s = Vec::with_capacity(21);
                if spec.plus && v >= 0 {
                    s.push(b'+');
                }
                s.extend_from_slice(v.to_string().as_bytes());
                int_pad(out, &s, &spec, true)?;
            }
            b'u' => {
                let v = args.int(idx) as u64;
                int_pad(out, v.to_string().as_bytes(), &spec, false)?;
            }
            b'c' => {
                out.push(args.int(idx) as u8);
            }
            b'o' | b'x' | b'X' | b'b' => {
                let v = args.int(idx) as u64;
                // Observed on PHP 8.5: any precision empties these conversions.
                let s = if spec.precision.is_some() {
                    String::new()
                } else {
                    match conv {
                        b'o' => format!("{v:o}"),
                        b'x' => format!("{v:x}"),
                        b'X' => format!("{v:X}"),
                        _ => format!("{v:b}"),
                    }
                };
                pad_into(out, s.as_bytes(), &spec, false)?;
            }
            b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'h' | b'H' => {
                let v = args.float(idx);
                let mut prec = spec.precision.unwrap_or(6);
                if prec > MAX_FLOAT_PRECISION {
                    diags.push(Diag::Notice(format!(
                        "Requested precision of {prec} digits was truncated to PHP maximum of {MAX_FLOAT_PRECISION} digits"
                    )));
                    prec = MAX_FLOAT_PRECISION;
                }
                if !v.is_finite() {
                    let mut s: Vec<u8> = if v.is_nan() {
                        b"NaN".to_vec()
                    } else if v < 0.0 {
                        b"-INF".to_vec()
                    } else {
                        b"INF".to_vec()
                    };
                    // Observed on PHP 8.5: with `+` and zero padding the sign
                    // overwrites the first letter ("+NF", "+aN"); no padding.
                    if spec.plus && spec.pad == b'0' && !spec.left && s[0] != b'-' {
                        s[0] = b'+';
                    }
                    out.extend_from_slice(&s);
                    continue;
                }
                let s = format_float(v, conv, prec, locale);
                let s = if spec.plus && s[0] != b'-' {
                    let mut t = Vec::with_capacity(s.len() + 1);
                    t.push(b'+');
                    t.extend_from_slice(&s);
                    t
                } else {
                    s
                };
                pad_into(out, &s, &spec, true)?;
            }
            b'%' => out.push(b'%'),
            other => {
                return Err(StdError::ValueError(format!(
                    "Unknown format specifier \"{}\"",
                    String::from_utf8_lossy(&[other])
                )));
            }
        }
    }
    Ok(())
}

/// Formats a finite float for conversion `conv` with precision `prec`
/// (-1 for the shortest round-trip form, `%g`-family only), sign included.
fn format_float(v: f64, conv: u8, prec: i64, locale: Locale) -> Vec<u8> {
    let mut s = Vec::new();
    match conv {
        b'e' | b'E' => {
            if v < 0.0 {
                s.push(b'-');
            }
            let d = float::significant(v, prec as usize + 1);
            s.push(d.digits[0]);
            if prec > 0 {
                s.push(b'.');
                s.extend_from_slice(&d.digits[1..]);
            }
            float::push_exponent(&mut s, if conv == b'e' { b'e' } else { b'E' }, d.exp);
        }
        b'f' | b'F' => {
            if v < 0.0 {
                s.push(b'-');
            }
            let mut t = float::fixed_abs(v, prec as usize).into_bytes();
            if conv == b'f' && locale.decimal_point != b'.' {
                for b in t.iter_mut() {
                    if *b == b'.' {
                        *b = locale.decimal_point;
                    }
                }
            }
            s.extend_from_slice(&t);
        }
        _ => {
            // %g %G %h %H: the sign follows the sign bit ("-0").
            if v.is_sign_negative() {
                s.push(b'-');
            }
            let (d, threshold) = if prec == -1 {
                (float::shortest(v), 17)
            } else {
                let p = prec.max(1);
                (float::significant(v, p as usize), p as i32)
            };
            let e = if conv == b'g' || conv == b'h' {
                b'e'
            } else {
                b'E'
            };
            let dp = if conv == b'g' || conv == b'G' {
                locale.decimal_point
            } else {
                b'.'
            };
            float::push_general(&mut s, &d, threshold, e, dp);
        }
    }
    s
}

/// `%d` and `%u`: like [`pad_into`], except that zero padding of
/// left-justified integers pads with spaces (observed on PHP 8.5: `%-05d` of
/// 1 is `"1    "`).
fn int_pad(out: &mut Vec<u8>, s: &[u8], spec: &Spec, signed: bool) -> Result<(), StdError> {
    if spec.left && spec.pad == b'0' {
        let sp = Spec {
            pad: b' ',
            left: true,
            plus: spec.plus,
            width: spec.width,
            precision: spec.precision,
        };
        pad_into(out, s, &sp, signed)
    } else {
        pad_into(out, s, spec, signed)
    }
}

/// Pads `s` to the width. Right-justified numbers padded with `0` keep their
/// sign in front (`-0005`); left-justified values are padded on the right
/// with the padding character, even `0` (observed: `%-05.2f` of 3.4 is
/// `3.400`).
fn pad_into(out: &mut Vec<u8>, s: &[u8], spec: &Spec, numeric: bool) -> Result<(), StdError> {
    let width = spec.width as usize;
    if s.len() >= width {
        out.extend_from_slice(s);
        return Ok(());
    }
    if spec.width >= INT_MAX {
        return Err(StdError::Fatal(format!(
            "Field width {} is too long",
            spec.width
        )));
    }
    let n = width - s.len();
    if spec.left {
        out.extend_from_slice(s);
        out.extend(std::iter::repeat_n(spec.pad, n));
    } else if numeric && spec.pad == b'0' && !s.is_empty() && (s[0] == b'-' || s[0] == b'+') {
        out.push(s[0]);
        out.extend(std::iter::repeat_n(b'0', n));
        out.extend_from_slice(&s[1..]);
    } else {
        out.extend(std::iter::repeat_n(spec.pad, n));
        out.extend_from_slice(s);
    }
    Ok(())
}

/// `sprintf()` over plain values with the C locale; returns the string and
/// the warnings.
pub fn sprintf(fmt: &[u8], args: &[Arg]) -> Result<(Vec<u8>, Vec<Diag>), StdError> {
    let mut args = args.to_vec();
    let mut out = Vec::new();
    let mut diags = Vec::new();
    format(
        fmt,
        args.as_mut_slice(),
        ArgsKind::Variadic,
        Locale::default(),
        &mut out,
        &mut diags,
    )?;
    Ok((out, diags))
}

#[cfg(test)]
#[allow(clippy::cloned_ref_to_slice_refs)]
mod tests {
    use super::*;

    fn s(fmt: &str, args: &[Arg]) -> String {
        match sprintf(fmt.as_bytes(), args) {
            Ok((o, _)) => String::from_utf8_lossy(&o).into_owned(),
            Err(StdError::ValueError(m)) | Err(StdError::ArgumentCountError(m)) => m,
            Err(e) => format!("{e:?}"),
        }
    }
    use Arg::*;

    // test ext/standard/tests/strings/sprintf_f.phpt
    #[test]
    fn sprintf_f() {
        assert_eq!(s("%3.2f", &[Float(1.2)]), "1.20");
        assert_eq!(s("%-03.2f", &[Float(1.2)]), "1.20");
        assert_eq!(s("%5.2f", &[Float(3.4)]), " 3.40");
        assert_eq!(s("%-5.2f", &[Float(3.4)]), "3.40 ");
        assert_eq!(s("%05.2f", &[Float(3.4)]), "03.40");
        assert_eq!(s("%-05.2f", &[Float(3.4)]), "3.400");
        assert_eq!(s("%7.2f", &[Float(-5.6)]), "  -5.60");
        assert_eq!(s("%-7.2f", &[Float(-5.6)]), "-5.60  ");
        assert_eq!(s("%07.2f", &[Float(-5.6)]), "-005.60");
        assert_eq!(s("%-07.2f", &[Float(-5.6)]), "-5.6000");
        let big = s("%3.4f", &[Float(1.2345678e99)]);
        assert_eq!(big.len(), 105);
        assert!(big.starts_with("1234567"));
        assert!(big.ends_with(".0000"));
    }

    // test ext/standard/tests/strings/sprintf_error.phpt
    #[test]
    fn sprintf_error() {
        let one = Str(b"one".to_vec());
        let two = Str(b"two".to_vec());
        assert_eq!(s("%s", &[]), "2 arguments are required, 1 given");
        assert_eq!(
            s("%s%s", &[one.clone()]),
            "3 arguments are required, 2 given"
        );
        assert_eq!(
            s("%s%s%s", &[one.clone(), two]),
            "4 arguments are required, 3 given"
        );
        assert_eq!(s("%s%s%s", &[one]), "4 arguments are required, 2 given");
        assert_eq!(s("%100$d %d", &[]), "101 arguments are required, 1 given");
        assert_eq!(
            s("foo %", &[Int(42)]),
            "Missing format specifier at end of string"
        );
    }

    // test ext/standard/tests/strings/sprintf_star.phpt
    #[test]
    fn sprintf_star() {
        let f = Float(1.234_567_890_123_456_7);
        let fx = Float(1.234_567_890_123_456_9e100);
        assert_eq!(s("%.*f", &[Int(10), f.clone()]), "1.2345678901");
        assert_eq!(s("%.*G", &[Int(10), f.clone()]), "1.23456789");
        assert_eq!(s("%.*g", &[Int(-1), fx.clone()]), "1.2345678901234569e+100");
        assert_eq!(s("%.*G", &[Int(-1), fx.clone()]), "1.2345678901234569E+100");
        assert_eq!(s("%.*h", &[Int(-1), fx.clone()]), "1.2345678901234569e+100");
        assert_eq!(s("%.*H", &[Int(-1), fx]), "1.2345678901234569E+100");
        assert_eq!(s("%.*s", &[Int(3), Str(b"foobar".to_vec())]), "foo");
        assert_eq!(s("%*f", &[Int(10), f.clone()]), "  1.234568");
        assert_eq!(s("%*G", &[Int(10), f.clone()]), "   1.23457");
        assert_eq!(s("%*s", &[Int(10), Str(b"foobar".to_vec())]), "    foobar");
        assert_eq!(s("%*.*f", &[Int(10), Int(3), f.clone()]), "     1.235");
        assert_eq!(s("%*.*G", &[Int(10), Int(3), f.clone()]), "      1.23");
        assert_eq!(
            s("%*.*s", &[Int(10), Int(3), Str(b"foobar".to_vec())]),
            "       foo"
        );
        assert_eq!(s("%1$.*2$f", &[f.clone(), Int(10)]), "1.2345678901");
        assert_eq!(s("%.*2$f", &[f.clone(), Int(10)]), "1.2345678901");
        assert_eq!(s("%2$.*f", &[Int(10), f.clone()]), "1.2345678901");
        assert_eq!(s("%1$*2$f", &[f.clone(), Int(10)]), "  1.234568");
        assert_eq!(s("%*2$f", &[f.clone(), Int(10)]), "  1.234568");
        assert_eq!(s("%2$*f", &[Int(10), f.clone()]), "  1.234568");
        assert_eq!(
            s("%1$*2$.*3$f", &[f.clone(), Int(10), Int(3)]),
            "     1.235"
        );
        assert_eq!(s("%*2$.*3$f", &[f.clone(), Int(10), Int(3)]), "     1.235");
        assert_eq!(s("%3$*.*f", &[Int(10), Int(3), f.clone()]), "     1.235");
        assert_eq!(
            s("%.*G", &[Str(b"foo".to_vec()), Float(1.5)]),
            "Precision must be an integer"
        );
        assert_eq!(
            s("%.*G", &[Int(-100), Float(1.5)]),
            "Precision must be between -1 and 2147483647"
        );
        assert_eq!(
            s("%.*s", &[Int(-1), Str(b"Foo".to_vec())]),
            "Precision -1 is only supported for %g, %G, %h and %H"
        );
        assert_eq!(
            s("%*G", &[Int(-1), f.clone()]),
            "Width must be between 0 and 2147483647"
        );
        assert_eq!(
            s("%9999999999999999999999.f", &[f.clone()]),
            "Width must be between 0 and 2147483647"
        );
        assert_eq!(
            s("%.9999999999999999999999f", &[f]),
            "Precision must be between 0 and 2147483647"
        );
    }

    // test ext/standard/tests/strings/sprintf_f_2.phpt (manual examples)
    #[test]
    fn manual_examples() {
        let n = Int(43951789);
        let u = Int(-43951789);
        assert_eq!(s("%b", &[n.clone()]), "10100111101010011010101101");
        assert_eq!(s("%c", &[Int(65)]), "A");
        assert_eq!(s("%e", &[n.clone()]), "4.395179e+7");
        assert_eq!(s("%u", &[u.clone()]), "18446744073665599827");
        assert_eq!(s("%f", &[n.clone()]), "43951789.000000");
        assert_eq!(s("%o", &[n.clone()]), "247523255");
        assert_eq!(s("%x", &[n.clone()]), "29ea6ad");
        assert_eq!(s("%X", &[n.clone()]), "29EA6AD");
        assert_eq!(s("%+d", &[n]), "+43951789");
        assert_eq!(s("%+d", &[u]), "-43951789");
        let m = Str(b"monkey".to_vec());
        assert_eq!(s("[%10s]", &[m.clone()]), "[    monkey]");
        assert_eq!(s("[%-10s]", &[m.clone()]), "[monkey    ]");
        assert_eq!(s("[%010s]", &[m.clone()]), "[0000monkey]");
        assert_eq!(s("[%'#10s]", &[m]), "[####monkey]");
        assert_eq!(
            s("[%10.10s]", &[Str(b"many monkeys".to_vec())]),
            "[many monke]"
        );
        assert_eq!(
            s("%04d-%02d-%02d", &[Int(2006), Int(12), Int(18)]),
            "2006-12-18"
        );
        assert_eq!(s("%01.2f", &[Float(68.75 + 54.35)]), "123.10");
        assert_eq!(s("%.3e", &[Int(362525200)]), "3.625e+8");
        assert_eq!(s("%.3F", &[Float(100.426)]), "100.426");
        assert_eq!(s("%.2F", &[Float(100.426)]), "100.43");
        assert_eq!(s("%d", &[Float(100.9)]), "100");
        assert_eq!(s("%o", &[Float(100.9)]), "144");
        assert_eq!(
            s(
                "The %2$s contains %1$d monkeys",
                &[Float(100.1), Str(b"world".to_vec())]
            ),
            "The world contains 100 monkeys"
        );
    }

    #[test]
    fn locale() {
        let mut out = Vec::new();
        let mut d = Vec::new();
        let mut a = vec![Float(100.426), Float(1.25), Float(1.25), Float(123.1)];
        format(
            b"%.2f %g %h %F %.3e",
            a.as_mut_slice(),
            ArgsKind::Variadic,
            Locale {
                decimal_point: b',',
            },
            &mut out,
            &mut d,
        )
        .unwrap_err();
        out.clear();
        a.push(Float(123.1));
        format(
            b"%.2f %g %h %F %.3e",
            a.as_mut_slice(),
            ArgsKind::Variadic,
            Locale {
                decimal_point: b',',
            },
            &mut out,
            &mut d,
        )
        .unwrap();
        assert_eq!(out, b"100,43 1,25 1.25 123.100000 1.231e+2");
    }

    #[test]
    fn conversions() {
        assert_eq!(str_to_int(b"12abc"), 12);
        assert_eq!(str_to_int(b" 12"), 12);
        assert_eq!(str_to_int(b"1e3"), 1000);
        assert_eq!(str_to_int(b"0x1A"), 0);
        assert_eq!(str_to_int(b"-1.9"), -1);
        assert_eq!(str_to_int(b"9999999999999999999"), i64::MAX);
        assert_eq!(str_to_int(b"-9999999999999999999"), i64::MIN);
        assert_eq!(float_to_int(1e19), -8446744073709551616);
        assert_eq!(float_to_int(-1e19), 8446744073709551616);
        assert_eq!(float_to_string(1e19), b"1.0E+19");
        assert_eq!(float_to_string(-0.0), b"-0");
        assert_eq!(float_to_string(0.1 + 0.2), b"0.30000000000000004");
    }
}
