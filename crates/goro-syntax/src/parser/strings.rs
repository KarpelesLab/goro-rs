//! Literal decoding: string escapes, heredoc indentation, numbers.
//!
//! PHP manual: "Strings" (escape sequences, heredoc closing-marker
//! indentation), "Integers", "Floating point numbers".

use std::borrow::Cow;

/// Decodes a single-quoted string body: only `\'` and `\\` are escapes.
pub fn unescape_single(s: &[u8]) -> Cow<'_, [u8]> {
    if !s.contains(&b'\\') {
        return Cow::Borrowed(s);
    }
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'\\' && i + 1 < s.len() && (s[i + 1] == b'\'' || s[i + 1] == b'\\') {
            out.push(s[i + 1]);
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
    }
    Cow::Owned(out)
}

/// Which quote character is escapable in a double-quote-like context.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Quote {
    Double,
    Backtick,
    Heredoc,
}

/// Result of escape decoding: the bytes and an optional warning (octal
/// overflow).
pub struct Decoded<'a> {
    pub bytes: Cow<'a, [u8]>,
    pub warnings: Vec<String>,
}

/// Decodes the escapes of a double-quoted, backtick or heredoc segment.
/// `\u{...}` escapes were validated by the lexer.
pub fn unescape_double(s: &[u8], q: Quote) -> Decoded<'_> {
    if !s.contains(&b'\\') {
        return Decoded {
            bytes: Cow::Borrowed(s),
            warnings: Vec::new(),
        };
    }
    let mut out = Vec::with_capacity(s.len());
    let mut warnings = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c != b'\\' || i + 1 >= s.len() {
            out.push(c);
            i += 1;
            continue;
        }
        let n = s[i + 1];
        i += 2;
        match n {
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'v' => out.push(0x0b),
            b'e' => out.push(0x1b),
            b'f' => out.push(0x0c),
            b'\\' => out.push(b'\\'),
            b'$' => out.push(b'$'),
            b'"' if q == Quote::Double => out.push(b'"'),
            b'`' if q == Quote::Backtick => out.push(b'`'),
            b'0'..=b'7' => {
                let start = i - 1;
                let mut v: u32 = (n - b'0') as u32;
                let mut k = 1;
                while k < 3 && i < s.len() && (b'0'..=b'7').contains(&s[i]) {
                    v = v * 8 + (s[i] - b'0') as u32;
                    i += 1;
                    k += 1;
                }
                if v > 0xff {
                    // Observed on PHP 8.5: a warning, the value wraps.
                    warnings.push(format!(
                        "Octal escape sequence overflow \\{} is greater than \\377",
                        String::from_utf8_lossy(&s[start..i])
                    ));
                }
                out.push(v as u8);
            }
            // `\X41` is a hex escape too (observed on PHP 8.5).
            b'x' | b'X' if i < s.len() && s[i].is_ascii_hexdigit() => {
                let mut v: u32 = 0;
                let mut k = 0;
                while k < 2 && i < s.len() && s[i].is_ascii_hexdigit() {
                    v = v * 16 + (s[i] as char).to_digit(16).unwrap();
                    i += 1;
                    k += 1;
                }
                out.push(v as u8);
            }
            b'u' if i < s.len() && s[i] == b'{' => {
                let mut j = i + 1;
                let mut v: u32 = 0;
                while j < s.len() && s[j].is_ascii_hexdigit() {
                    v = v
                        .saturating_mul(16)
                        .saturating_add((s[j] as char).to_digit(16).unwrap());
                    j += 1;
                }
                if j < s.len() && s[j] == b'}' && j > i + 1 {
                    push_utf8(&mut out, v);
                    i = j + 1;
                } else {
                    // Invalid (the lexer reports it in parser mode).
                    out.extend_from_slice(b"\\u");
                }
            }
            _ => {
                out.push(b'\\');
                out.push(n);
            }
        }
    }
    Decoded {
        bytes: Cow::Owned(out),
        warnings,
    }
}

fn push_utf8(out: &mut Vec<u8>, v: u32) {
    if v < 0x80 {
        out.push(v as u8);
    } else if v < 0x800 {
        out.push(0xC0 | (v >> 6) as u8);
        out.push(0x80 | (v & 0x3F) as u8);
    } else if v < 0x10000 {
        out.push(0xE0 | (v >> 12) as u8);
        out.push(0x80 | ((v >> 6) & 0x3F) as u8);
        out.push(0x80 | (v & 0x3F) as u8);
    } else {
        out.push(0xF0 | (v >> 18) as u8);
        out.push(0x80 | ((v >> 12) & 0x3F) as u8);
        out.push(0x80 | ((v >> 6) & 0x3F) as u8);
        out.push(0x80 | (v & 0x3F) as u8);
    }
}

/// Removes `indent` bytes of leading whitespace from each line of a heredoc
/// segment. `at_line_start`: the segment begins at the start of a line.
pub fn remove_indent(s: &[u8], indent: usize, at_line_start: bool) -> Cow<'_, [u8]> {
    if indent == 0 {
        return Cow::Borrowed(s);
    }
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    let mut line_start = at_line_start;
    while i < s.len() {
        if line_start {
            let mut k = 0;
            while k < indent && i < s.len() && (s[i] == b' ' || s[i] == b'\t') {
                i += 1;
                k += 1;
            }
            line_start = false;
            continue;
        }
        let c = s[i];
        out.push(c);
        i += 1;
        if c == b'\n' {
            line_start = true;
        } else if c == b'\r' {
            if i < s.len() && s[i] == b'\n' {
                out.push(b'\n');
                i += 1;
            }
            line_start = true;
        }
    }
    Cow::Owned(out)
}

/// Removes one trailing newline (`\n`, `\r\n` or `\r`).
pub fn strip_trailing_newline(s: &mut Cow<'_, [u8]>) {
    let n = s.len();
    let cut = if n >= 2 && s[n - 2] == b'\r' && s[n - 1] == b'\n' {
        2
    } else if n >= 1 && (s[n - 1] == b'\n' || s[n - 1] == b'\r') {
        1
    } else {
        0
    };
    if cut > 0 {
        match s {
            Cow::Borrowed(b) => *b = &b[..n - cut],
            Cow::Owned(v) => v.truncate(n - cut),
        }
    }
}

/// A parsed number literal.
pub enum Number {
    Int(i64),
    Float(f64),
}

/// Parses an integer or float literal as written in source (`T_LNUMBER`
/// or `T_DNUMBER` text).
pub fn parse_number(text: &[u8]) -> Number {
    let clean: Cow<'_, [u8]> = if text.contains(&b'_') {
        Cow::Owned(text.iter().copied().filter(|&c| c != b'_').collect())
    } else {
        Cow::Borrowed(text)
    };
    let t = &clean[..];
    if t.len() > 1 && t[0] == b'0' {
        let (radix, digits) = match t[1] {
            b'x' | b'X' => (16, &t[2..]),
            b'b' | b'B' => (2, &t[2..]),
            b'o' | b'O' => (8, &t[2..]),
            b'0'..=b'9' if !t.iter().any(|&c| matches!(c, b'.' | b'e' | b'E')) => (8, &t[1..]),
            _ => (10, t),
        };
        if radix != 10 {
            return int_or_float_radix(digits, radix);
        }
    }
    if t.iter().all(|c| c.is_ascii_digit()) {
        return int_or_float_radix(t, 10);
    }
    let s = std::str::from_utf8(t).unwrap_or("0");
    Number::Float(s.parse::<f64>().unwrap_or(0.0))
}

fn int_or_float_radix(digits: &[u8], radix: u32) -> Number {
    let mut v: u64 = 0;
    let mut overflow = false;
    for &c in digits {
        let d = (c as char).to_digit(radix).unwrap_or(0) as u64;
        match v.checked_mul(radix as u64).and_then(|x| x.checked_add(d)) {
            Some(x) => v = x,
            None => {
                overflow = true;
                break;
            }
        }
    }
    if !overflow && v <= i64::MAX as u64 {
        return Number::Int(v as i64);
    }
    if radix == 10 {
        let s = std::str::from_utf8(digits).unwrap_or("0");
        return Number::Float(s.parse::<f64>().unwrap_or(f64::INFINITY));
    }
    if radix == 2 {
        // Binary literals keep their first 53 significant bits (observed on
        // PHP 8.5: 64 one bits give 0x43efffffffffffff, not 2^64).
        let digits: Vec<u8> = digits.iter().copied().skip_while(|&c| c == b'0').collect();
        let keep = digits.len().min(53);
        let mut m: u64 = 0;
        for &c in &digits[..keep] {
            m = m * 2 + (c - b'0') as u64;
        }
        return Number::Float(m as f64 * 2f64.powi((digits.len() - keep) as i32));
    }
    // Hexadecimal and octal overflow: accumulate as a double (observed on
    // PHP 8.5: 0xFFFFFFFFFFFFFFFF gives 2^64).
    let mut f = 0.0f64;
    for &c in digits {
        let d = (c as char).to_digit(radix).unwrap_or(0) as f64;
        f = f * radix as f64 + d;
    }
    Number::Float(f)
}
