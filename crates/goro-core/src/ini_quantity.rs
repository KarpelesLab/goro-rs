//! Port of zend_ini.c `zend_ini_parse_quantity_internal` (memory_limit style
//! "128M" values) including its backwards-compatibility warnings.

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// smart_str_append_escaped()
fn escaped(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &c in bytes {
        match c {
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x0c => out.push_str("\\f"),
            0x0b => out.push_str("\\v"),
            b'\\' => out.push_str("\\\\"),
            0x1b => out.push_str("\\e"),
            c if (0x20..0x7f).contains(&c) => out.push(c as char),
            c => out.push_str(&format!("\\x{:02X}", c)),
        }
    }
    out
}

/// strtoul(digits, &end, base) on a byte slice; returns (value, consumed, overflow).
fn strtoul(s: &[u8], base: u32) -> (u64, usize, bool) {
    let mut i = 0;
    let mut base = base;
    if base == 0 {
        if s.len() >= 2
            && s[0] == b'0'
            && (s[1] == b'x' || s[1] == b'X')
            && s.get(2).is_some_and(|c| c.is_ascii_hexdigit())
        {
            base = 16;
            i = 2;
        } else if s.first() == Some(&b'0') {
            base = 8;
        } else {
            base = 10;
        }
    }
    let start = i;
    let mut v: u64 = 0;
    let mut overflow = false;
    while i < s.len() {
        let d = match (s[i] as char).to_digit(base) {
            Some(d) => d as u64,
            None => break,
        };
        match v.checked_mul(base as u64).and_then(|x| x.checked_add(d)) {
            Some(n) => v = n,
            None => {
                overflow = true;
                v = u64::MAX;
            }
        }
        i += 1;
    }
    if i == start {
        return (0, 0, false);
    }
    (v, i, overflow)
}

/// Returns the parsed quantity (as unsigned bits) and an optional warning.
pub fn parse_quantity(value: &[u8], signed: bool) -> (u64, Option<String>) {
    let mut lo = 0;
    let mut hi = value.len();
    while lo < hi && is_ws(value[lo]) {
        lo += 1;
    }
    while lo < hi && is_ws(value[hi - 1]) {
        hi -= 1;
    }
    if lo == hi {
        return (0, None);
    }
    let s = &value[..hi];
    let mut d = lo;
    let mut is_negative = false;
    if s[d] == b'+' {
        d += 1;
    } else if s[d] == b'-' {
        is_negative = true;
        d += 1;
    }
    let no_digits = || {
        format!(
            "Invalid quantity \"{}\": no valid leading digits, interpreting as \"0\" for backwards compatibility",
            escaped(value)
        )
    };
    if d >= hi || !s[d].is_ascii_digit() {
        return (0, Some(no_digits()));
    }
    let mut base = 0u32;
    if s[d] == b'0' && !(d + 1 < hi && s[d + 1].is_ascii_digit()) {
        if d + 1 == hi {
            return (0, None);
        }
        match s[d + 1] {
            b'g' | b'G' | b'm' | b'M' | b'k' | b'K' => {}
            b'x' | b'X' => base = 16,
            b'o' | b'O' => base = 8,
            b'b' | b'B' => base = 2,
            c => {
                return (
                    0,
                    Some(format!(
                        "Invalid prefix \"0{}\", interpreting as \"0\" for backwards compatibility",
                        c as char
                    )),
                );
            }
        }
        if base != 0 {
            d += 2;
            let ok = d < hi && (s[d] as char).is_digit(base);
            if !ok {
                return (
                    0,
                    Some(format!(
                        "Invalid quantity \"{}\": no digits after base prefix, interpreting as \"0\" for backwards compatibility",
                        escaped(value)
                    )),
                );
            }
        }
    }
    let (mut retval, consumed, mut overflow) = strtoul(&s[d..], base);
    let mut digits_end = d + consumed;
    if !overflow {
        if !signed {
            if is_negative {
                if retval == 1 && digits_end == hi {
                    retval = u64::MAX;
                } else {
                    overflow = true;
                }
            }
        } else if is_negative && retval == (i64::MAX as u64) + 1 {
            retval = 0u64.wrapping_sub(retval);
        } else if (retval as i64) < 0 {
            overflow = true;
        } else if is_negative {
            retval = 0u64.wrapping_sub(retval);
        }
    }
    if consumed == 0 {
        return (0, Some(no_digits()));
    }
    while digits_end < hi && is_ws(s[digits_end]) {
        digits_end += 1;
    }
    if digits_end != hi {
        let factor: u64 = match s[hi - 1] {
            b'g' | b'G' => 1 << 30,
            b'm' | b'M' => 1 << 20,
            b'k' | b'K' => 1 << 10,
            _ => {
                return (
                    retval,
                    Some(format!(
                        "Invalid quantity \"{}\": unknown multiplier \"{}\", interpreting as \"{}\" for backwards compatibility",
                        escaped(value),
                        escaped(&s[hi - 1..hi]),
                        escaped(&value[..digits_end])
                    )),
                );
            }
        };
        if !overflow {
            if signed {
                let sv = retval as i64;
                overflow = if sv > 0 {
                    sv > i64::MAX / factor as i64
                } else {
                    sv < i64::MIN / factor as i64
                };
            } else {
                overflow = retval > u64::MAX / factor;
            }
        }
        retval = retval.wrapping_mul(factor);
        if digits_end != hi - 1 {
            return (
                retval,
                Some(format!(
                    "Invalid quantity \"{}\", interpreting as \"{}{}\" for backwards compatibility",
                    escaped(value),
                    escaped(&value[..digits_end]),
                    escaped(&s[hi - 1..hi])
                )),
            );
        }
    }
    if overflow {
        return (
            retval,
            Some(format!(
                "Invalid quantity \"{}\": value is out of range, using overflow result for backwards compatibility",
                escaped(value)
            )),
        );
    }
    (retval, None)
}
