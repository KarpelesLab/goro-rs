//! Number formatting: `format-number()` (XSLT 1.0 section 12.3, the JDK
//! `DecimalFormat` pattern language) and `xsl:number`'s format strings
//! (section 7.7.1).

use super::compile::DecimalFormat;

struct SubPattern {
    prefix: String,
    suffix: String,
    min_int: usize,
    min_frac: usize,
    max_frac: usize,
    group: usize,
    scale: f64,
}

fn parse_sub(p: &str, df: &DecimalFormat) -> SubPattern {
    let is_body = |c: char| {
        c == df.digit
            || c == df.zero_digit
            || c == df.decimal_separator
            || c == df.grouping_separator
    };
    let chars: Vec<char> = p.chars().collect();
    let start = chars
        .iter()
        .position(|&c| is_body(c))
        .unwrap_or(chars.len());
    let end = chars
        .iter()
        .rposition(|&c| is_body(c))
        .map_or(start, |e| e + 1);
    let prefix: String = chars[..start].iter().collect();
    let suffix: String = chars[end..].iter().collect();
    let (mut min_int, mut min_frac, mut max_frac, mut group) = (0, 0, 0, 0);
    let mut in_frac = false;
    let mut since_group: Option<usize> = None;
    for &c in &chars[start..end] {
        if c == df.decimal_separator {
            in_frac = true;
        } else if c == df.grouping_separator {
            if !in_frac {
                since_group = Some(0);
            }
        } else if in_frac {
            if c == df.zero_digit {
                min_frac += 1;
            }
            max_frac += 1;
        } else {
            if c == df.zero_digit {
                min_int += 1;
            }
            if let Some(n) = &mut since_group {
                *n += 1;
            }
        }
    }
    if let Some(n) = since_group {
        group = n;
    }
    let has = |c: char| prefix.contains(c) || suffix.contains(c);
    let scale = if has(df.percent) {
        100.0
    } else if has(df.per_mille) {
        1000.0
    } else {
        1.0
    };
    SubPattern {
        prefix,
        suffix,
        min_int,
        min_frac,
        max_frac,
        group,
        scale,
    }
}

/// `format-number($n, $pattern)`.
pub fn format_number(n: f64, pattern: &str, df: &DecimalFormat) -> String {
    if n.is_nan() {
        return df.nan.clone();
    }
    let (pos, neg) = match pattern.split_once(df.pattern_separator) {
        Some((a, b)) => (a, Some(b)),
        None => (pattern, None),
    };
    let p = parse_sub(pos, df);
    let negative = n < 0.0 || (n == 0.0 && n.is_sign_negative() && false);
    let (prefix, suffix) = if negative {
        match neg {
            Some(x) => {
                let q = parse_sub(x, df);
                (q.prefix, q.suffix)
            }
            None => (format!("{}{}", df.minus_sign, p.prefix), p.suffix.clone()),
        }
    } else {
        (p.prefix.clone(), p.suffix.clone())
    };
    let abs = n.abs() * p.scale;
    if abs.is_infinite() {
        return format!("{prefix}{}{suffix}", df.infinity);
    }
    let text = format!("{:.*}", p.max_frac, abs);
    let (int, frac) = match text.split_once('.') {
        Some((i, f)) => (i.to_string(), f.to_string()),
        None => (text, String::new()),
    };
    let mut frac = frac;
    while frac.len() > p.min_frac && frac.ends_with('0') {
        frac.pop();
    }
    let mut int = int.trim_start_matches('0').to_string();
    while int.len() < p.min_int {
        int.insert(0, '0');
    }
    if int.is_empty() && frac.is_empty() {
        int.push('0');
    }
    let digit = |c: char| -> char {
        let d = c as u32 - '0' as u32;
        char::from_u32(df.zero_digit as u32 + d).unwrap_or(c)
    };
    let mut out = prefix;
    let len = int.len();
    for (i, c) in int.chars().enumerate() {
        out.push(digit(c));
        let left = len - i - 1;
        if p.group > 0 && left > 0 && left % p.group == 0 {
            out.push(df.grouping_separator);
        }
    }
    if !frac.is_empty() {
        out.push(df.decimal_separator);
        out.extend(frac.chars().map(digit));
    }
    out.push_str(&suffix);
    out
}

fn alpha(mut n: u64, base: char) -> String {
    let mut v = Vec::new();
    while n > 0 {
        n -= 1;
        v.push(char::from_u32(base as u32 + (n % 26) as u32).unwrap_or(base));
        n /= 26;
    }
    v.iter().rev().collect()
}

fn roman(mut n: u64, upper: bool) -> String {
    if n == 0 || n >= 5000 {
        return n.to_string();
    }
    const T: &[(u64, &str)] = &[
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut s = String::new();
    for &(v, r) in T {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    if upper { s.to_uppercase() } else { s }
}

fn decimal(n: u64, width: usize, group: Option<(&str, usize)>) -> String {
    let mut digits = n.to_string();
    while digits.len() < width {
        digits.insert(0, '0');
    }
    let Some((sep, size)) = group else {
        return digits;
    };
    let mut out = String::new();
    let len = digits.len();
    for (i, c) in digits.chars().enumerate() {
        out.push(c);
        let left = len - i - 1;
        if left > 0 && left % size == 0 {
            out.push_str(sep);
        }
    }
    out
}

fn one(n: u64, token: &str, group: Option<(&str, usize)>) -> String {
    match token {
        "a" => alpha(n, 'a'),
        "A" => alpha(n, 'A'),
        "i" => roman(n, false),
        "I" => roman(n, true),
        t if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) => decimal(n, t.len(), group),
        _ => decimal(n, 1, group),
    }
}

/// `xsl:number`: the numbers as `format` says (section 7.7.1).
pub fn format_numbers(nums: &[f64], format: &str, group: Option<(&str, usize)>) -> String {
    // Alternating punctuation and alphanumeric tokens.
    let mut tokens: Vec<(bool, String)> = Vec::new();
    for c in format.chars() {
        let alnum = c.is_alphanumeric();
        match tokens.last_mut() {
            Some((a, s)) if *a == alnum => s.push(c),
            _ => tokens.push((alnum, c.to_string())),
        }
    }
    let mut prefix = String::new();
    let mut suffix = String::new();
    if tokens.first().is_some_and(|t| !t.0) {
        prefix = tokens.remove(0).1;
    }
    if tokens.last().is_some_and(|t| !t.0) {
        suffix = tokens.pop().unwrap().1;
    }
    let mut out = prefix;
    let mut idx = 0;
    let mut last_tok = "1".to_string();
    let mut last_sep = ".".to_string();
    for (i, &n) in nums.iter().enumerate() {
        if let Some((_, t)) = tokens.get(idx) {
            last_tok = t.clone();
        }
        if i > 0 {
            if idx > 0
                && let Some((false, s)) = tokens.get(idx - 1)
            {
                last_sep = s.clone();
            }
            out.push_str(&last_sep);
        }
        if n.is_nan() || n.is_infinite() || n < 0.5 {
            out.push_str(&crate::xpath::number_to_string(n));
        } else {
            out.push_str(&one((n + 0.5).floor() as u64, &last_tok, group));
        }
        idx += 2;
    }
    out.push_str(&suffix);
    out
}
