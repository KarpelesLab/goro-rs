//! Integer strings: parsing as `gmp_init()` and every `GMP|string|int`
//! argument read them, and writing in a base as `gmp_strval()` does.
//!
//! Parsing has two layers (observed on PHP 8.5 with GMP 6.3, and the GMP
//! manual for `mpz_set_str`):
//!
//! * PHP skips leading white space and, when the base is 0 or matches,
//!   removes a `0x`/`0X` (16), `0b`/`0B` (2) or `0o`/`0O` (8) prefix,
//!   switching base 0 to that base. The prefix is only seen right after the
//!   white space: `"-0o17"` is not recognised by this layer.
//! * GMP then reads the rest: leading white space, an optional `-`, and a
//!   first character that must be a digit (decimal for base 0). Base 0
//!   then looks for `0x`/`0X` (16), `0b`/`0B` (2) or a leading `0` (8),
//!   else decimal. Every following character is a digit of the base or
//!   white space, which is ignored anywhere ("White space is allowed in the
//!   string, and is simply ignored", GMP manual). The digits may run out
//!   right after a base-0 prefix (`"-0x"` is zero). A `+` sign is not
//!   accepted.
//!
//! The string ends at its first NUL byte (PHP hands GMP a C string).
//! White space is the C locale's: space, `\t`, `\n`, `\v`, `\f`, `\r`.
//! Digits: `0-9a-z` (either case) for bases up to 36; `0-9A-Za-z` with
//! upper case first for bases 37 to 62.

use puremp::Int;

#[inline]
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

#[inline]
fn digit_value(c: u8, base: u32) -> Option<u32> {
    let v = match c {
        b'0'..=b'9' => (c - b'0') as u32,
        b'A'..=b'Z' => (c - b'A') as u32 + 10,
        b'a'..=b'z' => {
            if base <= 36 {
                (c - b'a') as u32 + 10
            } else {
                (c - b'a') as u32 + 36
            }
        }
        _ => return None,
    };
    (v < base).then_some(v)
}

/// Parses an integer string in `base` (0 or 2..=62) the way PHP's gmp
/// functions do; `None` when it is not an integer string.
pub fn parse_integer(s: &[u8], base: u32) -> Option<Int> {
    let s = match s.iter().position(|&c| c == 0) {
        Some(n) => &s[..n],
        None => s,
    };
    let mut base = base;
    let mut start = 0;
    let p = s.iter().position(|&c| !is_space(c)).unwrap_or(s.len());
    if s.len() >= p + 2 && s[p] == b'0' {
        let prefix_base = match s[p + 1] {
            b'x' | b'X' => 16,
            b'b' | b'B' => 2,
            b'o' | b'O' => 8,
            _ => 0,
        };
        if prefix_base != 0 && (base == 0 || base == prefix_base) {
            base = prefix_base;
            start = p + 2;
        }
    }
    set_str(&s[start..], base)
}

/// GMP's layer (see the module documentation).
fn set_str(s: &[u8], base: u32) -> Option<Int> {
    let mut i = 0;
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    let neg = s.get(i) == Some(&b'-');
    if neg {
        i += 1;
    }
    let first = *s.get(i)?;
    digit_value(first, if base == 0 { 10 } else { base })?;
    let mut base = base;
    if base == 0 {
        base = 10;
        if first == b'0' {
            match s.get(i + 1) {
                Some(b'x' | b'X') => {
                    base = 16;
                    i += 2;
                }
                Some(b'b' | b'B') => {
                    base = 2;
                    i += 2;
                }
                _ => base = 8,
            }
        }
    }
    let digits = &s[i..];
    // Fast path: the value fits a u64 and there is no white space.
    let mut acc: u64 = 0;
    let mut n = 0;
    while n < digits.len() {
        let c = digits[n];
        let Some(d) = digit_value(c, base) else {
            if is_space(c) {
                break;
            }
            return None;
        };
        match acc
            .checked_mul(base as u64)
            .and_then(|v| v.checked_add(d as u64))
        {
            Some(v) => acc = v,
            None => break,
        }
        n += 1;
    }
    let mag = if n == digits.len() {
        Int::from_u64(acc)
    } else {
        // Slow path: normalise the digits (white space removed, bases up
        // to 36 in lower case) for puremp's parser.
        let mut clean = String::with_capacity(digits.len());
        for &c in digits {
            if is_space(c) {
                continue;
            }
            digit_value(c, base)?;
            clean.push(if base <= 36 {
                c.to_ascii_lowercase() as char
            } else {
                c as char
            });
        }
        if clean.is_empty() {
            Int::ZERO
        } else {
            Int::from_str_radix(&clean, base).ok()?
        }
    };
    Some(if neg { mag.neg() } else { mag })
}

const LOWER: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
const UPPER: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const B62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Writes `n` in `base`: 2..=36 with lower-case digits, -2..=-36 with
/// upper-case digits, 37..=62 with `0-9A-Za-z`. `base` must be valid.
pub fn to_string_radix(n: &Int, base: i32) -> String {
    let b = base.unsigned_abs();
    debug_assert!((2..=62).contains(&b) && (base > 0 || b <= 36));
    let table: &[u8] = if b > 36 {
        B62
    } else if base < 0 {
        UPPER
    } else {
        LOWER
    };
    if n.limbs().len() <= 1 {
        let m = n.least_significant_limb();
        // One limb: write the digits directly.
        let mut buf = [0u8; 65];
        let mut pos = buf.len();
        let mut m = m;
        if b == 10 {
            // Constant divisor: the compiler turns it into multiplications.
            loop {
                pos -= 1;
                buf[pos] = b'0' + (m % 10) as u8;
                m /= 10;
                if m == 0 {
                    break;
                }
            }
        } else {
            loop {
                pos -= 1;
                buf[pos] = table[(m % b as u64) as usize];
                m /= b as u64;
                if m == 0 {
                    break;
                }
            }
        }
        if n.is_negative() {
            pos -= 1;
            buf[pos] = b'-';
        }
        // The bytes are ASCII.
        return std::str::from_utf8(&buf[pos..])
            .unwrap_or_default()
            .to_owned();
    }
    let mut out = String::new();
    let _ = n.write_radix(&mut out, b);
    if base < 0 {
        out.make_ascii_uppercase();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str, base: u32) -> Option<String> {
        parse_integer(s.as_bytes(), base).map(|v| to_string_radix(&v, 10))
    }

    #[test]
    fn prefixes() {
        // tests ext/gmp/tests/gmp_init_integer_notations.phpt, 003.phpt,
        // bug50175.phpt, surprising_integer_literals.phpt
        assert_eq!(p("0x16", 0).as_deref(), Some("22"));
        assert_eq!(p("  0o16", 0).as_deref(), Some("14"));
        assert_eq!(p("016", 0).as_deref(), Some("14"));
        assert_eq!(p("0b11", 0).as_deref(), Some("3"));
        assert_eq!(p("0bcd", 16).as_deref(), Some("3021"));
        assert_eq!(p("0xyz", 36).as_deref(), Some("44027"));
        assert_eq!(p("4d2", 0), None);
        for s in ["0x", "0X", "0b", "0B", "0o", "0O", "", "  "] {
            assert_eq!(p(s, 0), None, "{s:?}");
        }
        // observed on PHP 8.5
        assert_eq!(p("-0x1f", 0).as_deref(), Some("-31"));
        assert_eq!(p("0x-1f", 0).as_deref(), Some("-31"));
        assert_eq!(p("-0x", 0).as_deref(), Some("0"));
        assert_eq!(p("-0o17", 0), None);
        assert_eq!(p("1 2", 0).as_deref(), Some("12"));
        assert_eq!(p("- 12", 0), None);
        assert_eq!(p("+1", 0), None);
        assert_eq!(p("12\0x", 0).as_deref(), Some("12"));
        assert_eq!(p("ZZ", 37).as_deref(), Some("1330"));
        assert_eq!(p("zz", 37), None);
        assert_eq!(p("z", 62).as_deref(), Some("61"));
    }

    #[test]
    fn radix() {
        // test ext/gmp/tests/bug50283.phpt
        let a = parse_integer(b"0x41682179fbf5", 0).unwrap();
        assert_eq!(to_string_radix(&a, -36), "PHPISCOOL");
        assert_eq!(to_string_radix(&a, 36), "phpiscool");
        assert_eq!(to_string_radix(&a, 37), "KHKATELJF");
        assert_eq!(to_string_radix(&a, 62), "KQ6yq741");
        let b = parse_integer(b"gh82179fbf5", 32).unwrap();
        assert_eq!(to_string_radix(&b, 62), "1NHkAcdIiD");
        let big = parse_integer(b"-123456789012345678901234567890", 10).unwrap();
        assert_eq!(to_string_radix(&big, 10), "-123456789012345678901234567890");
        assert_eq!(to_string_radix(&big, -16), "-18EE90FF6C373E0EE4E3F0AD2");
    }
}
