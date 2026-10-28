//! Numeric strings.
//!
//! PHP manual (Numeric strings):
//!
//! ```text
//! WHITESPACES      \s*
//! LNUM             [0-9]+
//! DNUM             ({LNUM}?"."{LNUM}) | ({LNUM}"."{LNUM}?)
//! EXPONENT_DNUM    (({LNUM} | {DNUM}) [eE][+-]? {LNUM})
//! INT_NUM_STRING   {WHITESPACES} [+-]? {LNUM} {WHITESPACES}
//! FLOAT_NUM_STRING {WHITESPACES} [+-]? ({DNUM} | {EXPONENT_DNUM}) {WHITESPACES}
//! NUM_STRING       ({INT_NUM_STRING} | {FLOAT_NUM_STRING})
//! ```
//!
//! "PHP also has a concept of leading numeric strings. This is simply a
//! string which starts like a numeric string followed by any characters."
//! Whitespace is `" \t\n\r\v\f"` (observed on PHP 8.5). Hex, octal and
//! binary notations are not numeric. An integer string that overflows
//! `int` is a float ("Saner numeric strings" RFC; observed on PHP 8.5:
//! `"9999999999999999999" + 0` is `float(1.0E+19)`).

/// A PHP number.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Number {
    Int(i64),
    Float(f64),
}

impl Number {
    pub fn to_f64(self) -> f64 {
        match self {
            Number::Int(i) => i as f64,
            Number::Float(f) => f,
        }
    }
}

/// How much of a string is numeric.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Numericness {
    /// The whole string is numeric (surrounding whitespace allowed).
    Numeric,
    /// A numeric prefix followed by other characters.
    Leading,
    /// No numeric prefix.
    NonNumeric,
}

/// Result of [`parse_numeric`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumParse {
    /// The value of the numeric prefix (0 when there is none).
    pub num: Number,
    pub kind: Numericness,
    /// Whether the prefix used float syntax (`.` or an exponent), as
    /// opposed to an integer string that may have overflowed to float.
    pub float_syntax: bool,
}

#[inline]
pub fn is_php_whitespace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// Parses the numeric prefix of `s`.
pub fn parse_numeric(s: &[u8]) -> NumParse {
    // Fast path: short plain decimal integers.
    if !s.is_empty() && s.len() <= 18 && s.iter().all(u8::is_ascii_digit) {
        let mut v: i64 = 0;
        for &c in s {
            v = v * 10 + (c - b'0') as i64;
        }
        return NumParse {
            num: Number::Int(v),
            kind: Numericness::Numeric,
            float_syntax: false,
        };
    }
    let n = s.len();
    let mut i = 0;
    while i < n && is_php_whitespace(s[i]) {
        i += 1;
    }
    let start = i;
    let mut neg = false;
    if i < n && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let int_start = i;
    while i < n && s[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i - int_start;
    let mut frac_digits = 0;
    let mut float_syntax = false;
    if i < n && s[i] == b'.' {
        let mut j = i + 1;
        while j < n && s[j].is_ascii_digit() {
            j += 1;
        }
        frac_digits = j - i - 1;
        if int_digits > 0 || frac_digits > 0 {
            float_syntax = true;
            i = j;
        }
    }
    if int_digits == 0 && frac_digits == 0 {
        return NumParse {
            num: Number::Int(0),
            kind: Numericness::NonNumeric,
            float_syntax: false,
        };
    }
    if i < n && (s[i] == b'e' || s[i] == b'E') {
        let mut j = i + 1;
        if j < n && (s[j] == b'+' || s[j] == b'-') {
            j += 1;
        }
        let exp_start = j;
        while j < n && s[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            float_syntax = true;
            i = j;
        }
    }
    let end = i;
    while i < n && is_php_whitespace(s[i]) {
        i += 1;
    }
    let kind = if i == n {
        Numericness::Numeric
    } else {
        Numericness::Leading
    };
    let num = if float_syntax {
        Number::Float(parse_float_ascii(&s[start..end]))
    } else {
        // Integer syntax: accumulate, falling back to float on overflow.
        let digits = &s[int_start..end];
        let mut v: u64 = 0;
        let mut overflow = false;
        for &c in digits {
            match v
                .checked_mul(10)
                .and_then(|v| v.checked_add((c - b'0') as u64))
            {
                Some(x) => v = x,
                None => {
                    overflow = true;
                    break;
                }
            }
        }
        if !overflow && neg && v <= i64::MAX as u64 + 1 {
            Number::Int((v as i64).wrapping_neg())
        } else if !overflow && !neg && v <= i64::MAX as u64 {
            Number::Int(v as i64)
        } else {
            Number::Float(parse_float_ascii(&s[start..end]))
        }
    };
    NumParse {
        num,
        kind,
        float_syntax,
    }
}

/// Correctly rounded decimal → binary conversion of an ASCII number that
/// matches the numeric-string grammar (sign, digits, '.', exponent).
pub fn parse_float_ascii(b: &[u8]) -> f64 {
    // The grammar is a subset of what Rust's f64 parser accepts, and Rust's
    // parser rounds correctly (round-half-even), as IEEE 754 requires.
    std::str::from_utf8(b)
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0)
}

/// `is_numeric()` for strings: the whole string is numeric.
pub fn is_numeric_str(s: &[u8]) -> bool {
    parse_numeric(s).kind == Numericness::Numeric
}

/// The value of a fully numeric string, or `None`.
pub fn numeric_value(s: &[u8]) -> Option<Number> {
    let p = parse_numeric(s);
    (p.kind == Numericness::Numeric).then_some(p.num)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> (Number, Numericness) {
        let r = parse_numeric(s.as_bytes());
        (r.num, r.kind)
    }

    #[test]
    fn grammar() {
        use Number::*;
        use Numericness::*;
        assert_eq!(p("123"), (Int(123), Numeric));
        assert_eq!(p(" 123 "), (Int(123), Numeric));
        assert_eq!(p("-0"), (Int(0), Numeric));
        assert_eq!(p("1e3"), (Float(1000.0), Numeric));
        assert_eq!(p("1."), (Float(1.0), Numeric));
        assert_eq!(p(".5"), (Float(0.5), Numeric));
        assert_eq!(p("."), (Int(0), NonNumeric));
        assert_eq!(p("1e"), (Int(1), Leading));
        assert_eq!(p("1e+"), (Int(1), Leading));
        assert_eq!(p("0x1A"), (Int(0), Leading));
        assert_eq!(p("abc"), (Int(0), NonNumeric));
        assert_eq!(p(""), (Int(0), NonNumeric));
        assert_eq!(p("   "), (Int(0), NonNumeric));
        assert_eq!(p("1 1"), (Int(1), Leading));
        assert_eq!(p("9223372036854775807"), (Int(i64::MAX), Numeric));
        assert_eq!(p("-9223372036854775808"), (Int(i64::MIN), Numeric));
        assert_eq!(
            p("9223372036854775808"),
            (Float(crate::convert::TWO63), Numeric)
        );
        assert_eq!(p("1e1000"), (Float(f64::INFINITY), Numeric));
        assert_eq!(p("\x0b\x0c1\t"), (Int(1), Numeric));
        assert_eq!(p("1\0"), (Int(1), Leading));
        assert_eq!(p("+.5e-3"), (Float(0.0005), Numeric));
        assert_eq!(p(".e3"), (Int(0), NonNumeric));
        assert_eq!(p("1e3.5"), (Float(1000.0), Leading));
    }
}
