//! Decimal digit generation for floats.
//!
//! PHP prints floats from their exact binary value, correctly rounded, with
//! exact ties going to the even digit (observed on PHP 8.5: `%.0f` of 2.5 is
//! `2`, `%.2f` of 1.005 is `1.00`). Rust's own float formatting has the same
//! contract, so the digits come from it and this module only rearranges
//! them.

use std::fmt::Write;

/// Decimal digits and exponent of a finite float: `|v| = 0.d1d2d3... * 10^(exp+1)`,
/// that is `d1.d2d3... * 10^exp`. Zero gives digits of `0` and exponent 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digits {
    /// ASCII digits, the first one non-zero unless the value is zero.
    pub digits: Vec<u8>,
    /// Decimal exponent of the first digit.
    pub exp: i32,
}

fn parse_sci(s: &str) -> Digits {
    let (mant, exp) = s.split_once('e').expect("exponent");
    let digits: Vec<u8> = mant.bytes().filter(|b| b.is_ascii_digit()).collect();
    Digits {
        digits,
        exp: exp.parse().expect("exponent"),
    }
}

/// `n` significant digits (at least 1) of `|v|`, correctly rounded.
pub fn significant(v: f64, n: usize) -> Digits {
    let n = n.max(1);
    let mut s = String::new();
    write!(s, "{:.*e}", n - 1, v.abs()).unwrap();
    parse_sci(&s)
}

/// The shortest digits that read back as `|v|`.
pub fn shortest(v: f64) -> Digits {
    let mut s = String::new();
    write!(s, "{:e}", v.abs()).unwrap();
    parse_sci(&s)
}

/// `|v|` with `prec` digits after the decimal point, exact and correctly
/// rounded, without a decimal point when `prec` is 0.
pub fn fixed_abs(v: f64, prec: usize) -> String {
    let mut s = String::new();
    write!(s, "{:.*}", prec, v.abs()).unwrap();
    s
}

/// Appends `exp` the way PHP's printf and float-to-string write exponents:
/// a sign and the minimal digits (`e+5`, `e-10`).
pub fn push_exponent(out: &mut Vec<u8>, e: u8, exp: i32) {
    out.push(e);
    out.push(if exp < 0 { b'-' } else { b'+' });
    out.extend_from_slice(exp.unsigned_abs().to_string().as_bytes());
}

/// Formats digits in PHP's `%g` style (also used for float-to-string): fixed
/// notation when `-4 <= exp < threshold`, otherwise `d.ddde+X` with at least
/// one digit after the point (`1.0e+25`). Trailing zeros are dropped.
/// `dec_point` separates the fraction. No sign is written.
pub fn push_general(out: &mut Vec<u8>, d: &Digits, threshold: i32, e: u8, dec_point: u8) {
    let mut digits: &[u8] = &d.digits;
    while digits.len() > 1 && digits[digits.len() - 1] == b'0' {
        digits = &digits[..digits.len() - 1];
    }
    if digits == b"0" {
        out.push(b'0');
        return;
    }
    let exp = d.exp;
    if exp < -4 || exp >= threshold {
        out.push(digits[0]);
        out.push(dec_point);
        if digits.len() > 1 {
            out.extend_from_slice(&digits[1..]);
        } else {
            out.push(b'0');
        }
        push_exponent(out, e, exp);
    } else if exp < 0 {
        out.push(b'0');
        out.push(dec_point);
        out.extend(std::iter::repeat_n(b'0', (-exp - 1) as usize));
        out.extend_from_slice(digits);
    } else {
        let int_len = exp as usize + 1;
        if digits.len() <= int_len {
            out.extend_from_slice(digits);
            out.extend(std::iter::repeat_n(b'0', int_len - digits.len()));
        } else {
            out.extend_from_slice(&digits[..int_len]);
            out.push(dec_point);
            out.extend_from_slice(&digits[int_len..]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits() {
        assert_eq!(
            significant(2.5, 1),
            Digits {
                digits: b"2".to_vec(),
                exp: 0
            }
        );
        assert_eq!(significant(15.0, 1).digits, b"2");
        assert_eq!(significant(0.0, 3).digits, b"000");
        assert_eq!(shortest(0.1).digits, b"1");
        assert_eq!(shortest(0.1).exp, -1);
        assert_eq!(fixed_abs(-2.5, 0), "2");
        assert_eq!(fixed_abs(1.005, 2), "1.00");
    }

    #[test]
    fn general() {
        let g = |v: f64, p: usize| {
            let mut o = Vec::new();
            push_general(&mut o, &significant(v, p), p as i32, b'e', b'.');
            String::from_utf8(o).unwrap()
        };
        assert_eq!(g(1e15, 6), "1.0e+15");
        assert_eq!(g(100000.0, 6), "100000");
        assert_eq!(g(0.0001, 6), "0.0001");
        assert_eq!(g(0.00001, 6), "1.0e-5");
        assert_eq!(g(1234.5678, 6), "1234.57");
        assert_eq!(g(1.5e20, 20), "1.5e+20");
    }
}
