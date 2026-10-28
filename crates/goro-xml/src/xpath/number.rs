//! XPath number conversions (W3C XPath 1.0 sections 4.2 and 4.4) with the
//! formatting PHP's XPath engine uses (observed on PHP 8.5).
//!
//! `string(number)`: NaN, Infinity, -Infinity; zero (either sign) is `0`;
//! integers strictly between -2147483648 and 2147483647 are written as
//! integers; other numbers with an absolute value in [1e-5, 1e9) are
//! written in fixed notation with 15 significant digits (trailing zeros
//! removed); everything else in exponent notation with up to 15
//! significant digits and an exponent of at least two digits
//! (`1e+20`, `1.5e-07`, `1.23456789012345e+14`).

/// The string value of a number.
pub fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if x == 0.0 || x.abs() < 1e-323 {
        // The smallest subnormal is written as 0 (observed on PHP 8.5).
        return "0".into();
    }
    if x.fract() == 0.0 && x > -2147483648.0 && x < 2147483647.0 {
        return format!("{}", x as i64);
    }
    // 15 significant digits.
    let e = format!("{:.14e}", x);
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let neg = mant.starts_with('-');
    let digits: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let ax = x.abs();
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    if (1e-5..1e9).contains(&ax) {
        // Fixed notation.
        if exp >= 0 {
            let int_len = exp as usize + 1;
            if digits.len() <= int_len {
                out.push_str(digits);
                for _ in digits.len()..int_len {
                    out.push('0');
                }
            } else {
                out.push_str(&digits[..int_len]);
                out.push('.');
                out.push_str(&digits[int_len..]);
            }
        } else {
            out.push_str("0.");
            for _ in 0..(-exp - 1) {
                out.push('0');
            }
            out.push_str(digits);
        }
        return out;
    }
    out.push_str(&digits[..1]);
    if digits.len() > 1 {
        out.push('.');
        out.push_str(&digits[1..]);
    }
    out.push('e');
    out.push(if exp < 0 { '-' } else { '+' });
    let a = exp.unsigned_abs();
    if a < 10 {
        out.push('0');
    }
    out.push_str(&a.to_string());
    out
}

/// The number value of a string (XPath 1.0 `number()`): optional
/// whitespace, an optional minus sign, digits with an optional fraction
/// (or a fraction alone), and, as PHP accepts, an optional exponent
/// (observed on PHP 8.5: `number("1e3")` is 1000). Anything else is NaN.
pub fn string_to_number(s: &str) -> f64 {
    let t = s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r'));
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && b[i] == b'-' {
        i += 1;
    }
    let ds = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut digits = i - ds;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let fs = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        digits += i - fs;
    }
    if digits == 0 {
        return f64::NAN;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let save = i;
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let es = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == es {
            i = save;
        }
    }
    if i != b.len() {
        return f64::NAN;
    }
    let mut txt = t.to_string();
    if txt.ends_with('.') {
        txt.push('0');
    }
    if txt.starts_with('.') || txt.starts_with("-.") {
        txt = txt.replacen('.', "0.", 1);
    }
    txt.parse::<f64>().unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting() {
        // Observed on PHP 8.5.
        for (x, s) in [
            (1.0 / 3.0, "0.333333333333333"),
            (2.0 / 3.0, "0.666666666666667"),
            (0.1 + 0.2, "0.3"),
            (1e21, "1e+21"),
            (1e20, "1e+20"),
            (1000000.0, "1000000"),
            (0.000001, "1e-06"),
            (0.00001, "0.00001"),
            (1.23e-5, "0.0000123"),
            (1.5e-7, "1.5e-07"),
            (123456789012345.0, "1.23456789012345e+14"),
            (9007199254740993.0, "9.00719925474099e+15"),
            (100.0 / 3.0, "33.3333333333333"),
            (999999999.5, "999999999.5"),
            (1000000000.5, "1.0000000005e+09"),
            (1e9, "1000000000"),
            (2147483647.0, "2.147483647e+09"),
            (-2147483648.0, "-2.147483648e+09"),
            (123456789.123456789, "123456789.123457"),
            (0.000123456789012345678, "0.000123456789012346"),
            (-0.0, "0"),
            (5e-324, "0"),
            (1.5e300, "1.5e+300"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (-1.5, "-1.5"),
            (10.0, "10"),
        ] {
            assert_eq!(number_to_string(x), s, "{x:e}");
        }
    }

    #[test]
    fn parsing() {
        assert_eq!(string_to_number("  12  "), 12.0);
        assert_eq!(string_to_number("1e3"), 1000.0);
        assert_eq!(string_to_number(".5"), 0.5);
        assert_eq!(string_to_number("5."), 5.0);
        assert_eq!(string_to_number("-.5"), -0.5);
        assert!(string_to_number("+5").is_nan());
        assert!(string_to_number("-").is_nan());
        assert!(string_to_number("abc").is_nan());
        assert!(string_to_number("").is_nan());
        assert!(string_to_number("1 2").is_nan());
    }
}
