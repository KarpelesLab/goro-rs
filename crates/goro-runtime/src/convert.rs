//! Type conversions (PHP manual: Type Juggling, and the per-type
//! "Converting to ..." sections).

use crate::array::{Key, PhpArray};
use crate::ctx::Ctx;
use crate::error::{Level, PhpError, PhpResult};
use crate::float_fmt;
use crate::numeric::{Number, Numericness, parse_numeric};
use crate::string::PhpStr;
use crate::value::Value;

// ---------------------------------------------------------------- float text

/// PHP's string form of a float at ini `precision` (14 for echo and string
/// conversion; -1 for the shortest round-trip form used with
/// serialize_precision=-1).
pub fn float_to_string(f: f64, precision: i32) -> PhpStr {
    thread_local! {
        static BUF: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    BUF.with(|b| match b.try_borrow_mut() {
        Ok(mut out) => {
            out.clear();
            float_fmt::format_php(f, precision, &mut out);
            PhpStr::new(&out)
        }
        Err(_) => {
            let mut out = Vec::with_capacity(24);
            float_fmt::format_php(f, precision, &mut out);
            PhpStr::new(&out)
        }
    })
}

/// The shortest round-trip form, as used in diagnostics ("Implicit
/// conversion from float 1.5 to int").
pub fn float_repr(f: f64) -> String {
    let mut out = Vec::with_capacity(24);
    float_fmt::format_php_repr(f, -1, b'E', &mut out);
    String::from_utf8(out).unwrap_or_default()
}

/// Appends the decimal form of an int.
pub fn push_int(out: &mut Vec<u8>, i: i64) {
    let mut buf = [0u8; 20];
    out.extend_from_slice(int_digits(i, &mut buf));
}

/// The decimal form of an int as a string (small values don't allocate).
pub fn int_to_string(i: i64) -> PhpStr {
    if (0..10).contains(&i) {
        return PhpStr::from_byte(b'0' + i as u8);
    }
    let mut buf = [0u8; 20];
    PhpStr::new(int_digits(i, &mut buf))
}

/// Writes the decimal form of `i` at the end of `buf`, returning it.
#[inline]
pub fn int_digits(i: i64, buf: &mut [u8; 20]) -> &[u8] {
    // Two digits per division, from a table of "00".."99".
    const PAIRS: &[u8; 200] = b"0001020304050607080910111213141516171819\
2021222324252627282930313233343536373839\
4041424344454647484950515253545556575859\
6061626364656667686970717273747576777879\
8081828384858687888990919293949596979899";
    let mut n = i.unsigned_abs();
    let mut p = buf.len();
    while n >= 100 {
        let d = (n % 100) as usize * 2;
        n /= 100;
        p -= 2;
        buf[p] = PAIRS[d];
        buf[p + 1] = PAIRS[d + 1];
    }
    if n >= 10 {
        let d = n as usize * 2;
        p -= 2;
        buf[p] = PAIRS[d];
        buf[p + 1] = PAIRS[d + 1];
    } else {
        p -= 1;
        buf[p] = b'0' + n as u8;
    }
    if i < 0 {
        p -= 1;
        buf[p] = b'-';
    }
    &buf[p..]
}

// ---------------------------------------------------------------- bool

/// Boolean conversion (PHP manual, Booleans: "Converting to boolean").
/// NAN converts to true with a warning (observed on PHP 8.5: "unexpected
/// NAN value was coerced to bool").
#[inline]
pub fn to_bool(v: &Value, ctx: &mut dyn Ctx) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Int(i) => *i != 0,
        Value::Float(f) => {
            if f.is_nan() {
                ctx.notice(
                    Level::Warning,
                    "unexpected NAN value was coerced to bool".into(),
                );
            }
            *f != 0.0
        }
        Value::Str(s) => !(s.is_empty() || (s.len() == 1 && s[0] == b'0')),
        Value::Array(a) => !a.is_empty(),
        Value::Object(_) | Value::Resource(_) => true,
        Value::Ref(r) => to_bool(&r.borrow(), ctx),
    }
}

/// Boolean conversion without diagnostics.
pub fn to_bool_quiet(v: &Value) -> bool {
    match v {
        Value::Float(f) => *f != 0.0,
        Value::Ref(r) => to_bool_quiet(&r.borrow()),
        v => to_bool(v, &mut crate::ctx::Diagnostics::new()),
    }
}

// ---------------------------------------------------------------- int

/// 2^63 as a float.
pub const TWO63: f64 = -(i64::MIN as f64);

/// Whether `f` (finite) fits an int without wrapping.
#[inline]
fn float_in_int_range(f: f64) -> bool {
    // [-2^63, 2^63)
    (-TWO63..TWO63).contains(&f)
}

/// Modular float → int conversion used when the float does not fit: the
/// value modulo 2^64, as a two's complement int; NAN and infinities give 0.
/// (Observed on PHP 8.5: `(int)1e19` is -8446744073709551616.)
pub fn float_to_int_wrapping(f: f64) -> i64 {
    if !f.is_finite() {
        return 0;
    }
    if float_in_int_range(f) {
        return f as i64;
    }
    const TWO64: f64 = 18_446_744_073_709_551_616.0;
    let mut m = f.trunc() % TWO64;
    if m < 0.0 {
        m += TWO64;
    }
    // m in [0, 2^64): reinterpret as two's complement.
    (m as u64) as i64
}

/// `(int)$float`: truncates; out-of-range and non-finite values warn and
/// wrap (observed on PHP 8.5: "The float 1.0E+19 is not representable as an
/// int, cast occurred").
pub fn float_to_int_cast(f: f64, ctx: &mut dyn Ctx) -> i64 {
    if f.is_finite() && float_in_int_range(f) {
        return f as i64;
    }
    warn_not_representable(f, ctx);
    float_to_int_wrapping(f)
}

fn warn_not_representable(f: f64, ctx: &mut dyn Ctx) {
    ctx.notice(
        Level::Warning,
        format!(
            "The float {} is not representable as an int, cast occurred",
            float_repr(f)
        ),
    );
}

/// Implicit float → int conversion (array keys, `%`, bitwise operators):
/// like the cast, plus the PHP 8.1 deprecation for a fractional part
/// ("Deprecate implicit non-integer-compatible float to int conversions"
/// RFC).
pub fn float_to_int_implicit(f: f64, ctx: &mut dyn Ctx) -> i64 {
    if f.is_finite() && float_in_int_range(f) {
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
        return i;
    }
    warn_not_representable(f, ctx);
    if f.is_nan() {
        ctx.notice(
            Level::Deprecated,
            "Implicit conversion from float NAN to int loses precision".into(),
        );
    }
    float_to_int_wrapping(f)
}

/// String → int as `(int)` and `intval()` do: the numeric prefix, no
/// diagnostics; float-like prefixes saturate at the int range and
/// non-finite ones give 0 (observed on PHP 8.5: `(int)"1e19"` is
/// PHP_INT_MAX, `(int)"1e1000"` is 0).
pub fn str_to_int(s: &[u8]) -> i64 {
    match parse_numeric(s).num {
        Number::Int(i) => i,
        Number::Float(f) => float_to_int_saturating(f),
    }
}

/// Saturating float → int (non-finite gives 0).
pub fn float_to_int_saturating(f: f64) -> i64 {
    if !f.is_finite() {
        0
    } else if f >= TWO63 {
        i64::MAX
    } else if f < -TWO63 {
        i64::MIN
    } else {
        f as i64
    }
}

/// `(int)$v` (PHP manual, Integers: "Converting to integer").
pub fn to_int(v: &Value, ctx: &mut dyn Ctx) -> i64 {
    match v {
        Value::Null => 0,
        Value::Bool(b) => *b as i64,
        Value::Int(i) => *i,
        Value::Float(f) => float_to_int_cast(*f, ctx),
        Value::Str(s) => str_to_int(s),
        Value::Array(a) => !a.is_empty() as i64,
        Value::Object(o) => match ctx.object_to_number(o, false) {
            Number::Int(i) => i,
            Number::Float(f) => float_to_int_cast(f, ctx),
        },
        Value::Resource(r) => r.id(),
        Value::Ref(r) => to_int(&r.borrow(), ctx),
    }
}

// ---------------------------------------------------------------- float

/// String → float as `(float)` and `floatval()` do: the numeric prefix,
/// keeping the sign of "-0" (observed on PHP 8.5: `(float)"-0"` is -0.0).
pub fn str_to_float(s: &[u8]) -> f64 {
    let p = parse_numeric(s);
    match p.num {
        Number::Int(0) if p.kind != Numericness::NonNumeric => {
            let neg = s
                .iter()
                .find(|c| !crate::numeric::is_php_whitespace(**c))
                .is_some_and(|c| *c == b'-');
            if neg { -0.0 } else { 0.0 }
        }
        Number::Int(i) => i as f64,
        Number::Float(f) => f,
    }
}

/// `(float)$v` (PHP manual, Floating point numbers: "Converting to float").
pub fn to_float(v: &Value, ctx: &mut dyn Ctx) -> f64 {
    match v {
        Value::Null => 0.0,
        Value::Bool(b) => *b as i64 as f64,
        Value::Int(i) => *i as f64,
        Value::Float(f) => *f,
        Value::Str(s) => str_to_float(s),
        Value::Array(a) => !a.is_empty() as i64 as f64,
        Value::Object(o) => ctx.object_to_number(o, true).to_f64(),
        Value::Resource(r) => r.id() as f64,
        Value::Ref(r) => to_float(&r.borrow(), ctx),
    }
}

/// Numeric value for `(int)`/`(float)`-free contexts that accept any
/// number: ints stay ints, numeric strings become int or float.
pub fn to_number_cast(v: &Value, ctx: &mut dyn Ctx) -> Number {
    match v {
        Value::Int(i) => Number::Int(*i),
        Value::Float(f) => Number::Float(*f),
        Value::Str(s) => parse_numeric(s).num,
        Value::Ref(r) => to_number_cast(&r.borrow(), ctx),
        v => Number::Int(to_int(v, ctx)),
    }
}

// ---------------------------------------------------------------- string

/// `(string)$v` (PHP manual, Strings: "Converting to string").
pub fn to_str(v: &Value, ctx: &mut dyn Ctx) -> PhpResult<PhpStr> {
    Ok(match v {
        Value::Null | Value::Bool(false) => PhpStr::empty(),
        Value::Bool(true) => PhpStr::from_byte(b'1'),
        Value::Int(i) => int_to_string(*i),
        Value::Float(f) => {
            if f.is_nan() {
                ctx.notice(
                    Level::Warning,
                    "unexpected NAN value was coerced to string".into(),
                );
            }
            float_to_string(*f, ctx.precision())
        }
        Value::Str(s) => s.clone(),
        Value::Array(_) => {
            ctx.notice(Level::Warning, "Array to string conversion".into());
            PhpStr::intern(b"Array")
        }
        Value::Object(o) => ctx.object_to_string(o)?,
        Value::Resource(r) => PhpStr::from(format!("Resource id #{}", r.id())),
        Value::Ref(r) => {
            let inner = r.get();
            to_str(&inner, ctx)?
        }
    })
}

/// Appends `(string)$v` to `out` (for concatenation and echo without an
/// intermediate string for scalars).
pub fn append_str(out: &mut Vec<u8>, v: &Value, ctx: &mut dyn Ctx) -> PhpResult<()> {
    match v {
        Value::Null | Value::Bool(false) => {}
        Value::Bool(true) => out.push(b'1'),
        Value::Int(i) => push_int(out, *i),
        Value::Float(f) => {
            if f.is_nan() {
                ctx.notice(
                    Level::Warning,
                    "unexpected NAN value was coerced to string".into(),
                );
            }
            float_fmt::format_php(*f, ctx.precision(), out);
        }
        Value::Str(s) => out.extend_from_slice(s),
        v => out.extend_from_slice(&to_str(v, ctx)?),
    }
    Ok(())
}

// ---------------------------------------------------------------- array

/// `(array)$v` (PHP manual, Arrays: "Converting to array"): scalars become a
/// one-element list, null an empty array.
pub fn to_array(v: &Value, ctx: &mut dyn Ctx) -> PhpArray {
    match v {
        Value::Null => PhpArray::new(),
        Value::Array(a) => a.clone(),
        Value::Object(o) => ctx.object_to_array(o),
        Value::Ref(r) => {
            let inner = r.get();
            to_array(&inner, ctx)
        }
        Value::Float(f) if f.is_nan() => {
            ctx.notice(
                Level::Warning,
                "unexpected NAN value was coerced to array".into(),
            );
            PhpArray::from_values([v.clone()])
        }
        v => PhpArray::from_values([v.clone()]),
    }
}

// ---------------------------------------------------------------- keys

/// Converts a value used as an array offset to a key (PHP manual, Arrays:
/// key casts). `container` names the container type in the error message
/// ("Cannot access offset of type array on array").
///
/// - strings holding canonical decimal ints become ints ("8" → 8, "08"
///   stays a string),
/// - floats are truncated, with the fractional-part deprecation,
/// - bools become 0/1,
/// - null becomes "" with PHP 8.5's deprecation ("Using null as an array
///   offset is deprecated, use an empty string instead"),
/// - resources become their id with a warning,
/// - arrays and objects throw TypeError.
pub fn to_key(v: &Value, container: &str, ctx: &mut dyn Ctx) -> PhpResult<Key> {
    match v {
        Value::Int(i) => Ok(Key::Int(*i)),
        Value::Str(s) => Ok(Key::from_php_str(s)),
        Value::Bool(b) => Ok(Key::Int(*b as i64)),
        Value::Float(f) => Ok(Key::Int(float_to_int_implicit(*f, ctx))),
        Value::Null => {
            ctx.notice(
                Level::Deprecated,
                "Using null as an array offset is deprecated, use an empty string instead".into(),
            );
            Ok(Key::Str(PhpStr::empty()))
        }
        Value::Resource(r) => {
            ctx.notice(
                Level::Warning,
                format!(
                    "Resource ID#{} used as offset, casting to integer ({})",
                    r.id(),
                    r.id()
                ),
            );
            Ok(Key::Int(r.id()))
        }
        Value::Array(_) | Value::Object(_) => Err(PhpError::type_error(format!(
            "Cannot access offset of type {} on {}",
            v.type_name_for_error(),
            container
        ))),
        Value::Ref(r) => {
            let inner = r.get();
            to_key(&inner, container, ctx)
        }
    }
}

/// Numeric conversion of a string operand in arithmetic: the numeric value
/// and whether PHP warns ("A non-numeric value encountered" for leading
/// numeric strings). `None` for non-numeric strings, which make arithmetic
/// throw TypeError (observed on PHP 8.5; see
/// Zend/tests/numeric_strings/invalid_numeric_strings_must_generate_warning.phpt).
pub fn str_to_number_operand(s: &[u8], ctx: &mut dyn Ctx) -> Option<(Number, bool)> {
    let p = parse_numeric(s);
    match p.kind {
        Numericness::Numeric => Some((p.num, p.float_syntax)),
        Numericness::Leading => {
            ctx.notice(Level::Warning, "A non-numeric value encountered".into());
            Some((p.num, p.float_syntax))
        }
        Numericness::NonNumeric => None,
    }
}
