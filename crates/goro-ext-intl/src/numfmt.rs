//! `NumberFormatter` (PHP manual: NumberFormatter): ICU's DecimalFormat
//! with the pattern syntax of UTS #35 section 3 ("Number Format
//! Patterns") and the ICU user guide ("Formatting Numbers"), CLDR 48
//! locale data (tools/gen_intl_numbers.py), and the rule-based styles
//! (spell-out, ordinal, duration) on the intl crate.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::locale;

// Styles (NumberFormatter constants).
pub const PATTERN_DECIMAL: i64 = 0;
pub const DECIMAL: i64 = 1;
pub const CURRENCY: i64 = 2;
pub const PERCENT: i64 = 3;
pub const SCIENTIFIC: i64 = 4;
pub const SPELLOUT: i64 = 5;
pub const ORDINAL: i64 = 6;
pub const DURATION: i64 = 7;
pub const PATTERN_RULEBASED: i64 = 9;
pub const CURRENCY_ISO: i64 = 10;
pub const CURRENCY_PLURAL: i64 = 11;
pub const CURRENCY_ACCOUNTING: i64 = 12;
pub const CASH_CURRENCY: i64 = 13;
pub const DECIMAL_COMPACT_SHORT: i64 = 14;
pub const DECIMAL_COMPACT_LONG: i64 = 15;
pub const CURRENCY_STANDARD: i64 = 16;

// Attributes.
pub const PARSE_INT_ONLY: i64 = 0;
pub const GROUPING_USED: i64 = 1;
pub const DECIMAL_ALWAYS_SHOWN: i64 = 2;
pub const MAX_INTEGER_DIGITS: i64 = 3;
pub const MIN_INTEGER_DIGITS: i64 = 4;
pub const INTEGER_DIGITS: i64 = 5;
pub const MAX_FRACTION_DIGITS: i64 = 6;
pub const MIN_FRACTION_DIGITS: i64 = 7;
pub const FRACTION_DIGITS: i64 = 8;
pub const MULTIPLIER: i64 = 9;
pub const GROUPING_SIZE: i64 = 10;
pub const ROUNDING_MODE: i64 = 11;
pub const ROUNDING_INCREMENT: i64 = 12;
pub const FORMAT_WIDTH: i64 = 13;
pub const PADDING_POSITION: i64 = 14;
pub const SECONDARY_GROUPING_SIZE: i64 = 15;
pub const SIGNIFICANT_DIGITS_USED: i64 = 16;
pub const MIN_SIGNIFICANT_DIGITS: i64 = 17;
pub const MAX_SIGNIFICANT_DIGITS: i64 = 18;
pub const LENIENT_PARSE: i64 = 19;

// Text attributes.
pub const POSITIVE_PREFIX: i64 = 0;
pub const POSITIVE_SUFFIX: i64 = 1;
pub const NEGATIVE_PREFIX: i64 = 2;
pub const NEGATIVE_SUFFIX: i64 = 3;
pub const PADDING_CHARACTER: i64 = 4;
pub const CURRENCY_CODE: i64 = 5;
pub const DEFAULT_RULESET: i64 = 6;
pub const PUBLIC_RULESETS: i64 = 7;

// Symbols.
pub const DECIMAL_SEPARATOR_SYMBOL: i64 = 0;
pub const GROUPING_SEPARATOR_SYMBOL: i64 = 1;
pub const PATTERN_SEPARATOR_SYMBOL: i64 = 2;
pub const PERCENT_SYMBOL: i64 = 3;
pub const ZERO_DIGIT_SYMBOL: i64 = 4;
pub const DIGIT_SYMBOL: i64 = 5;
pub const MINUS_SIGN_SYMBOL: i64 = 6;
pub const PLUS_SIGN_SYMBOL: i64 = 7;
pub const CURRENCY_SYMBOL: i64 = 8;
pub const INTL_CURRENCY_SYMBOL: i64 = 9;
pub const MONETARY_SEPARATOR_SYMBOL: i64 = 10;
pub const EXPONENTIAL_SYMBOL: i64 = 11;
pub const PERMILL_SYMBOL: i64 = 12;
pub const PAD_ESCAPE_SYMBOL: i64 = 13;
pub const INFINITY_SYMBOL: i64 = 14;
pub const NAN_SYMBOL: i64 = 15;
pub const SIGNIFICANT_DIGIT_SYMBOL: i64 = 16;
pub const MONETARY_GROUPING_SEPARATOR_SYMBOL: i64 = 17;
pub const SYMBOL_COUNT: usize = 18;

// Rounding modes (ICU's UNumberFormatRoundingMode).
pub const ROUND_CEILING: i64 = 0;
pub const ROUND_FLOOR: i64 = 1;
pub const ROUND_DOWN: i64 = 2;
pub const ROUND_UP: i64 = 3;
pub const ROUND_HALFEVEN: i64 = 4;
pub const ROUND_HALFDOWN: i64 = 5;
pub const ROUND_HALFUP: i64 = 6;
pub const ROUND_UNNECESSARY: i64 = 7;
pub const ROUND_HALFODD: i64 = 8;
pub const ROUND_HALFCEILING: i64 = 9;
pub const ROUND_HALFFLOOR: i64 = 10;

// Padding positions.
pub const PAD_BEFORE_PREFIX: i64 = 0;
pub const PAD_AFTER_PREFIX: i64 = 1;
pub const PAD_BEFORE_SUFFIX: i64 = 2;
pub const PAD_AFTER_SUFFIX: i64 = 3;

// ---------------------------------------------------------------- data

struct NumData {
    /// (kind, locale, key) → fields.
    map: HashMap<String, Vec<&'static str>>,
    parents: HashMap<&'static str, &'static str>,
}

fn k(kind: u8, loc: &str, key: &str) -> String {
    format!("{}\u{1}{loc}\u{1}{key}", kind as char)
}

fn data() -> &'static NumData {
    static D: OnceLock<NumData> = OnceLock::new();
    D.get_or_init(|| {
        let mut map = HashMap::new();
        let mut parents = HashMap::new();
        for line in crate::inflate_text(include_bytes!("../data/numbers.txt.z")).lines() {
            if line.starts_with('#') {
                continue;
            }
            let f: Vec<&'static str> = line.split('\t').collect();
            match f[0] {
                "S" => {
                    map.insert(k(b'S', f[1], ""), f[2..].to_vec());
                }
                "F" => {
                    map.insert(k(b'F', f[1], f[2]), f[3..].to_vec());
                }
                "C" | "N" => {
                    map.insert(k(f[0].as_bytes()[0], f[1], f[2]), f[3..].to_vec());
                }
                "D" | "T" => {
                    map.insert(k(f[0].as_bytes()[0], f[1], ""), f[2..].to_vec());
                }
                "A" => {
                    parents.insert(f[1], f[2]);
                }
                _ => {}
            }
        }
        NumData { map, parents }
    })
}

/// The CLDR inheritance chain of a locale (BCP 47 tags as in the data),
/// root last ("root").
pub fn chain(id: &str) -> Vec<String> {
    let mut p = locale::parse(id);
    // a region implying another script than the language's (zh_TW is
    // zh_Hant_TW: UTS #35 section 4.3 likely subtags)
    if p.script.is_empty() && !p.region.is_empty() && !p.language.is_empty() {
        let with_region = locale::add_likely(&format!("{}_{}", p.language, p.region))
            .map(|m| locale::parse(&m).script);
        let plain = locale::add_likely(&p.language).map(|m| locale::parse(&m).script);
        if let (Some(s), Some(d)) = (with_region, plain)
            && s != d
        {
            p.script = s;
        }
    }
    let mut cur = if p.language.is_empty() {
        "root".to_string()
    } else {
        let mut s = p.language.clone();
        if !p.script.is_empty() {
            s.push('-');
            s.push_str(&p.script);
        }
        if !p.region.is_empty() {
            s.push('-');
            s.push_str(&p.region);
        }
        s
    };
    let d = data();
    let mut out = Vec::new();
    loop {
        out.push(cur.clone());
        if cur == "root" {
            break;
        }
        cur = match d.parents.get(cur.as_str()) {
            Some(p) => p.to_string(),
            None => match cur.rfind('-') {
                Some(i) => cur[..i].to_string(),
                None => "root".to_string(),
            },
        };
        if out.len() > 8 {
            break;
        }
    }
    out
}

fn lookup(kind: u8, chain: &[String], key: &str) -> Option<&'static Vec<&'static str>> {
    let d = data();
    for l in chain {
        if let Some(v) = d.map.get(&k(kind, l, key)) {
            return Some(v);
        }
    }
    None
}

/// A locale's symbol (index into the S line), inherited.
fn symbol_data(chain: &[String], idx: usize) -> Option<&'static str> {
    let d = data();
    for l in chain {
        if let Some(v) = d.map.get(&k(b'S', l, ""))
            && let Some(s) = v.get(idx)
            && !s.is_empty()
        {
            return Some(s);
        }
    }
    None
}

fn format_data(chain: &[String], key: &str) -> Option<&'static str> {
    lookup(b'F', chain, key).map(|v| v[0])
}

/// The fraction digits of a currency (ISO 4217 / CLDR currencyData).
pub fn currency_digits(code: &str) -> u32 {
    let d = data();
    match d.map.get(&k(b'D', code, "")) {
        Some(v) => v[0].parse().unwrap_or(2),
        None => d
            .map
            .get(&k(b'D', "DEFAULT", ""))
            .and_then(|v| v[0].parse().ok())
            .unwrap_or(2),
    }
}

/// The cash fraction digits of a currency (CLDR currencyData
/// `_cashDigits`, else its digits).
pub fn currency_cash_digits(code: &str) -> u32 {
    match data().map.get(&k(b'D', code, "")) {
        Some(v) => v
            .get(2)
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| currency_digits(code)),
        None => currency_digits(code),
    }
}

/// The currency of a symbol in a locale's data (`$` is USD in en).
pub fn currency_for_symbol(chain: &[String], sym: &str) -> Option<String> {
    let d = data();
    let mut locs: Vec<&str> = chain.iter().map(String::as_str).collect();
    locs.push("en");
    for l in locs {
        let prefix = k(b'C', l, "");
        for (key, v) in &d.map {
            if key.starts_with(&prefix) && (v[0] == sym || v.get(1) == Some(&sym)) {
                return Some(key[prefix.len()..].to_string());
            }
        }
    }
    None
}

/// The current currency of a region.
pub fn region_currency(region: &str) -> Option<&'static str> {
    data().map.get(&k(b'T', region, "")).map(|v| v[0])
}

/// The locale's currency symbol of `code` (`$` for USD in en), else the
/// code.
pub fn currency_symbol(chain: &[String], code: &str) -> String {
    match lookup(b'C', chain, code) {
        Some(v) => v[0].to_string(),
        None => code.to_string(),
    }
}

/// The display name of a currency (`one`/`other` plural forms when
/// `count` is given).
pub fn currency_name(chain: &[String], code: &str, count: Option<&str>) -> String {
    match lookup(b'N', chain, code) {
        Some(v) => {
            let i = match count {
                Some("one") => 1,
                Some(_) => 2,
                None => 0,
            };
            let s = v.get(i).copied().unwrap_or("");
            if s.is_empty() {
                v[0].to_string()
            } else {
                s.to_string()
            }
        }
        None => code.to_string(),
    }
}

// ---------------------------------------------------------------- patterns

/// A piece of a pattern affix.
#[derive(Clone, Debug, PartialEq)]
pub enum AffixPart {
    Lit(String),
    /// `¤` repeated n times.
    Currency(usize),
    Percent,
    Permill,
    Minus,
    Plus,
}

/// A parsed pattern (UTS #35 section 3.2 / ICU DecimalFormat).
#[derive(Clone, Debug)]
pub struct Pattern {
    pub pos_prefix: Vec<AffixPart>,
    pub pos_suffix: Vec<AffixPart>,
    pub neg: Option<(Vec<AffixPart>, Vec<AffixPart>)>,
    pub min_int: i32,
    pub max_int: i32,
    pub min_frac: i32,
    pub max_frac: i32,
    pub min_sig: i32,
    pub max_sig: i32,
    pub sig_used: bool,
    pub grouping: i32,
    pub grouping2: i32,
    pub grouping_used: bool,
    pub decimal_shown: bool,
    /// Minimum exponent digits, exponent sign always shown.
    pub exp: Option<(i32, bool)>,
    pub multiplier: i32,
    /// The rounding increment's digits ("05" for 0.05) and its scale.
    pub increment: Option<f64>,
    pub pad: Option<(char, i64, i64)>,
}

impl Pattern {
    fn empty() -> Pattern {
        Pattern {
            pos_prefix: Vec::new(),
            pos_suffix: Vec::new(),
            neg: None,
            min_int: 1,
            max_int: 2_000_000_000,
            min_frac: 0,
            max_frac: 0,
            min_sig: 0,
            max_sig: 0,
            sig_used: false,
            grouping: 0,
            grouping2: 0,
            grouping_used: false,
            decimal_shown: false,
            exp: None,
            multiplier: 1,
            increment: None,
            pad: None,
        }
    }

    pub fn has_currency(&self) -> bool {
        let f = |v: &Vec<AffixPart>| v.iter().any(|p| matches!(p, AffixPart::Currency(_)));
        f(&self.pos_prefix) || f(&self.pos_suffix)
    }
}

/// A pattern syntax error.
#[derive(Debug)]
pub struct PatternError;

/// A rounding that ROUND_UNNECESSARY forbids (U_FORMAT_INEXACT_ERROR).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inexact;

/// `max_frac` of a pattern showing all fraction digits.
pub const UNLIMITED_FRAC: i32 = 100_000;

fn parse_affix(
    chars: &[char],
    i: &mut usize,
    stop_at_number: bool,
) -> Result<(Vec<AffixPart>, Option<char>), PatternError> {
    let mut parts = Vec::new();
    let mut lit = String::new();
    let mut pad = None;
    let flush = |lit: &mut String, parts: &mut Vec<AffixPart>| {
        if !lit.is_empty() {
            parts.push(AffixPart::Lit(std::mem::take(lit)));
        }
    };
    while *i < chars.len() {
        let c = chars[*i];
        if matches!(c, '#' | '0'..='9' | ',' | '.' | '@') {
            if stop_at_number {
                break;
            }
            return Err(PatternError);
        }
        if c == ';' {
            break;
        }
        match c {
            '\'' => {
                *i += 1;
                if *i < chars.len() && chars[*i] == '\'' {
                    lit.push('\'');
                    *i += 1;
                    continue;
                }
                while *i < chars.len() {
                    if chars[*i] == '\'' {
                        if *i + 1 < chars.len() && chars[*i + 1] == '\'' {
                            lit.push('\'');
                            *i += 2;
                            continue;
                        }
                        break;
                    }
                    lit.push(chars[*i]);
                    *i += 1;
                }
                if *i >= chars.len() {
                    return Err(PatternError);
                }
                *i += 1;
                continue;
            }
            '¤' => {
                flush(&mut lit, &mut parts);
                let mut n = 0;
                while *i < chars.len() && chars[*i] == '¤' {
                    n += 1;
                    *i += 1;
                }
                parts.push(AffixPart::Currency(n));
                continue;
            }
            '%' => {
                flush(&mut lit, &mut parts);
                parts.push(AffixPart::Percent);
            }
            '‰' => {
                flush(&mut lit, &mut parts);
                parts.push(AffixPart::Permill);
            }
            '-' => {
                flush(&mut lit, &mut parts);
                parts.push(AffixPart::Minus);
            }
            '+' => {
                flush(&mut lit, &mut parts);
                parts.push(AffixPart::Plus);
            }
            '*' => {
                *i += 1;
                if *i < chars.len() {
                    pad = Some(chars[*i]);
                } else {
                    return Err(PatternError);
                }
            }
            _ => lit.push(c),
        }
        *i += 1;
    }
    flush(&mut lit, &mut parts);
    Ok((parts, pad))
}

/// Parses an ICU decimal pattern.
pub fn parse_pattern(pat: &str) -> Result<Pattern, PatternError> {
    let chars: Vec<char> = pat.chars().collect();
    let mut p = Pattern::empty();
    let mut i = 0;
    // positive prefix (may hold a pad spec)
    let mut pad_pos = None;
    let (prefix, pad1) = parse_affix(&chars, &mut i, true)?;
    if let Some(c) = pad1 {
        // "*x" before or after the prefix
        let at_start = pat.starts_with('*');
        pad_pos = Some((
            c,
            if at_start {
                PAD_BEFORE_PREFIX
            } else {
                PAD_AFTER_PREFIX
            },
        ));
    }
    p.pos_prefix = prefix;
    // number part
    let mut int_hash = 0;
    let mut int_zero = 0;
    let mut sig = 0;
    let mut sig_hash = 0;
    let mut frac_zero = 0;
    let mut frac_hash = 0;
    let mut in_frac = false;
    let mut last_group = None;
    let mut groups: Vec<i32> = Vec::new();
    let mut count_since_group = 0;
    let mut inc_digits = String::new();
    let mut inc_has = false;
    let mut seen_number = false;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '#' => {
                seen_number = true;
                if in_frac {
                    frac_hash += 1;
                } else if sig > 0 {
                    sig_hash += 1;
                    count_since_group += 1;
                } else {
                    if int_zero > 0 {
                        return Err(PatternError);
                    }
                    int_hash += 1;
                    count_since_group += 1;
                }
                if in_frac || int_zero > 0 || inc_has {
                    inc_digits.push('0');
                }
            }
            '0'..='9' => {
                seen_number = true;
                if sig > 0 {
                    return Err(PatternError);
                }
                if c != '0' {
                    inc_has = true;
                }
                inc_digits.push(c);
                if in_frac {
                    if frac_hash > 0 {
                        return Err(PatternError);
                    }
                    frac_zero += 1;
                } else {
                    int_zero += 1;
                    count_since_group += 1;
                }
            }
            '@' => {
                seen_number = true;
                if in_frac || int_zero > 0 || sig_hash > 0 {
                    return Err(PatternError);
                }
                sig += 1;
                count_since_group += 1;
            }
            ',' => {
                if in_frac {
                    return Err(PatternError);
                }
                if let Some(_g) = last_group {
                    groups.push(count_since_group);
                }
                last_group = Some(i);
                count_since_group = 0;
            }
            '.' => {
                if in_frac {
                    return Err(PatternError);
                }
                in_frac = true;
                seen_number = true;
                p.decimal_shown = true;
                inc_digits.push('.');
            }
            'E' => {
                if !seen_number {
                    break;
                }
                i += 1;
                let mut sign = false;
                if i < chars.len() && chars[i] == '+' {
                    sign = true;
                    i += 1;
                }
                let mut n = 0;
                while i < chars.len() && chars[i] == '0' {
                    n += 1;
                    i += 1;
                }
                if n == 0 {
                    return Err(PatternError);
                }
                p.exp = Some((n, sign));
                continue;
            }
            _ => break,
        }
        i += 1;
    }
    if !seen_number {
        return Err(PatternError);
    }
    if last_group.is_some() {
        p.grouping_used = true;
        p.grouping = count_since_group;
        p.grouping2 = groups.last().copied().unwrap_or(count_since_group);
        if p.grouping <= 0 {
            p.grouping_used = false;
        }
    }
    // a decimal point with only integer digits after reads "decimal shown"
    // only when it is the last character of the number part
    p.decimal_shown = in_frac && frac_zero == 0 && frac_hash == 0;
    if sig > 0 {
        p.sig_used = true;
        p.min_sig = sig;
        p.max_sig = sig + sig_hash;
        p.min_int = 1;
        p.max_frac = 0;
    } else {
        p.min_int = int_zero;
        p.min_frac = frac_zero;
        p.max_frac = frac_zero + frac_hash;
        if p.exp.is_some() {
            p.max_int = int_hash + int_zero;
        }
    }
    if inc_has {
        let v: f64 = inc_digits.parse().unwrap_or(0.0);
        if v > 0.0 {
            p.increment = Some(v);
        }
    }
    // positive suffix
    let (suffix, pad2) = parse_affix(&chars, &mut i, false)?;
    if let Some(c) = pad2 {
        let at_end = i >= chars.len() || chars[i] == ';';
        let _ = at_end;
        // "*x" before the suffix when the suffix follows it, else after
        let before = suffix.is_empty() || !pat.trim_end_matches(|ch| ch != '*').is_empty();
        pad_pos = Some((
            c,
            if before {
                PAD_BEFORE_SUFFIX
            } else {
                PAD_AFTER_SUFFIX
            },
        ));
    }
    p.pos_suffix = suffix;
    if i < chars.len() && chars[i] == ';' {
        i += 1;
        let (np, _) = parse_affix(&chars, &mut i, true)?;
        // skip the negative number part
        while i < chars.len() && matches!(chars[i], '#' | '0'..='9' | ',' | '.' | '@' | 'E' | '+') {
            i += 1;
        }
        let (ns, _) = parse_affix(&chars, &mut i, false)?;
        p.neg = Some((np, ns));
    }
    if let Some((c, pos)) = pad_pos {
        // the format width is the pattern's length without the pad spec
        let width = pattern_width(&p);
        p.pad = Some((c, width as i64, pos));
    }
    for part in p.pos_prefix.iter().chain(p.pos_suffix.iter()) {
        match part {
            AffixPart::Percent => p.multiplier = 100,
            AffixPart::Permill => p.multiplier = 1000,
            _ => {}
        }
    }
    Ok(p)
}

fn pattern_width(p: &Pattern) -> usize {
    let alen = |v: &Vec<AffixPart>| -> usize {
        v.iter()
            .map(|a| match a {
                AffixPart::Lit(s) => s.chars().count(),
                AffixPart::Currency(n) => *n,
                _ => 1,
            })
            .sum()
    };
    let mut n = alen(&p.pos_prefix) + alen(&p.pos_suffix);
    n += p.min_int.max(1) as usize;
    if p.max_frac > 0 {
        n += 1 + p.max_frac as usize;
    }
    if p.grouping_used {
        n += 1 + p.grouping as usize - p.min_int.min(p.grouping) as usize;
    }
    n
}

// ---------------------------------------------------------------- decimals

/// A non-negative decimal: `digits` (no leading zeros) × 10^(point -
/// digits.len()), i.e. `point` digits before the decimal point.
#[derive(Clone, Debug)]
pub struct Dec {
    pub digits: Vec<u8>,
    pub point: i64,
}

impl Dec {
    pub fn zero() -> Dec {
        Dec {
            digits: Vec::new(),
            point: 0,
        }
    }

    pub fn is_zero(&self) -> bool {
        self.digits.iter().all(|d| *d == 0)
    }

    /// The shortest decimal that reads back as `x` (|x|).
    pub fn from_f64(x: f64) -> Dec {
        let x = x.abs();
        if x == 0.0 {
            return Dec::zero();
        }
        let s = format!("{x:e}");
        let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
        let e: i64 = e.parse().unwrap_or(0);
        let digits: Vec<u8> = m
            .bytes()
            .filter(u8::is_ascii_digit)
            .map(|b| b - b'0')
            .collect();
        let mut d = Dec {
            digits,
            point: e + 1,
        };
        d.trim();
        d
    }

    pub fn from_i128(x: i128) -> Dec {
        let s = x.unsigned_abs().to_string();
        let mut d = Dec {
            point: s.len() as i64,
            digits: s.bytes().map(|b| b - b'0').collect(),
        };
        d.trim();
        d
    }

    /// From a plain decimal string ("123.45", "1e5").
    pub fn parse_str(s: &str) -> Option<Dec> {
        let s = s.trim().trim_start_matches(['-', '+']);
        let (mant, exp) = match s.find(['e', 'E']) {
            Some(i) => (&s[..i], s[i + 1..].parse::<i64>().ok()?),
            None => (s, 0),
        };
        let (ip, fp) = mant.split_once('.').unwrap_or((mant, ""));
        if !ip.bytes().chain(fp.bytes()).all(|b| b.is_ascii_digit())
            || (ip.is_empty() && fp.is_empty())
        {
            return None;
        }
        let mut d = Dec {
            digits: ip.bytes().chain(fp.bytes()).map(|b| b - b'0').collect(),
            point: ip.len() as i64 + exp,
        };
        d.trim();
        Some(d)
    }

    fn trim(&mut self) {
        let lead = self.digits.iter().take_while(|d| **d == 0).count();
        self.digits.drain(..lead);
        self.point -= lead as i64;
        while self.digits.last() == Some(&0) {
            self.digits.pop();
        }
        if self.digits.is_empty() {
            self.point = 0;
        }
    }

    /// The digit at power-of-ten `p` (0 = units).
    fn digit_at(&self, p: i64) -> u8 {
        let idx = self.point - 1 - p;
        if idx < 0 || idx >= self.digits.len() as i64 {
            0
        } else {
            self.digits[idx as usize]
        }
    }

    /// Rounds to `frac` fraction digits with `mode` (`neg`: the value's
    /// sign). Returns false for ROUND_UNNECESSARY when inexact.
    pub fn round_frac(&mut self, frac: i64, mode: i64, neg: bool) -> bool {
        let keep = self.point + frac; // digits kept
        if keep >= self.digits.len() as i64 {
            return true;
        }
        let (kept, rest): (Vec<u8>, Vec<u8>) = if keep <= 0 {
            (Vec::new(), {
                let mut r = vec![0u8; (-keep) as usize];
                r.extend_from_slice(&self.digits);
                r
            })
        } else {
            (
                self.digits[..keep as usize].to_vec(),
                self.digits[keep as usize..].to_vec(),
            )
        };
        let first = rest.first().copied().unwrap_or(0);
        let rest_nonzero = rest.iter().any(|d| *d != 0);
        let beyond_half = rest.iter().skip(1).any(|d| *d != 0);
        let last_odd = kept.last().is_some_and(|d| d % 2 == 1);
        let up = match mode {
            ROUND_CEILING => rest_nonzero && !neg,
            ROUND_FLOOR => rest_nonzero && neg,
            ROUND_DOWN => false,
            ROUND_UP => rest_nonzero,
            ROUND_HALFDOWN => first > 5 || (first == 5 && beyond_half),
            ROUND_HALFUP => first >= 5,
            ROUND_HALFODD => first > 5 || (first == 5 && (beyond_half || !last_odd)),
            ROUND_HALFCEILING => first > 5 || (first == 5 && (beyond_half || !neg)),
            ROUND_HALFFLOOR => first > 5 || (first == 5 && (beyond_half || neg)),
            ROUND_UNNECESSARY => {
                if rest_nonzero {
                    return false;
                }
                false
            }
            _ => first > 5 || (first == 5 && (beyond_half || last_odd)),
        };
        let mut digits = kept;
        let mut point = self.point;
        if keep <= 0 {
            // everything after the kept position dropped
            digits = Vec::new();
            point = -frac;
            if up {
                digits.push(1);
                point = 1 - frac;
            }
        } else if up {
            let mut i = digits.len();
            loop {
                if i == 0 {
                    digits.insert(0, 1);
                    point += 1;
                    break;
                }
                i -= 1;
                if digits[i] == 9 {
                    digits[i] = 0;
                } else {
                    digits[i] += 1;
                    break;
                }
            }
        }
        self.digits = digits;
        self.point = point;
        self.trim();
        true
    }

    /// Rounds to `sig` significant digits.
    pub fn round_sig(&mut self, sig: i64, mode: i64, neg: bool) -> bool {
        if self.digits.is_empty() {
            return true;
        }
        let frac = sig - self.point;
        self.round_frac(frac, mode, neg)
    }

    pub fn to_f64(&self) -> f64 {
        let s = self.plain();
        s.parse().unwrap_or(0.0)
    }

    /// "123.45".
    pub fn plain(&self) -> String {
        if self.digits.is_empty() {
            return "0".into();
        }
        let mut s = String::new();
        if self.point <= 0 {
            s.push_str("0.");
            for _ in 0..(-self.point) {
                s.push('0');
            }
            for d in &self.digits {
                s.push((b'0' + d) as char);
            }
        } else {
            for i in 0..self.point.max(self.digits.len() as i64) {
                if i == self.point {
                    s.push('.');
                }
                s.push((b'0' + self.digits.get(i as usize).copied().unwrap_or(0)) as char);
            }
        }
        s
    }

    fn mul_pow10(&mut self, n: i64) {
        if !self.digits.is_empty() {
            self.point += n;
        }
    }

    fn mul_int(&mut self, m: i64) {
        if m == 1 || self.digits.is_empty() {
            return;
        }
        // exact multiplication on the digit string
        let mut res: Vec<u32> = vec![0; self.digits.len() + 20];
        let mut mm = m.unsigned_abs();
        let mut mdigits = Vec::new();
        while mm > 0 {
            mdigits.push((mm % 10) as u32);
            mm /= 10;
        }
        let n = self.digits.len();
        for (i, d) in self.digits.iter().rev().enumerate() {
            for (j, md) in mdigits.iter().enumerate() {
                res[i + j] += *d as u32 * md;
            }
        }
        let mut carry = 0;
        for r in res.iter_mut() {
            let v = *r + carry;
            *r = v % 10;
            carry = v / 10;
        }
        while res.len() > 1 && *res.last().unwrap() == 0 {
            res.pop();
        }
        let added = res.len() as i64 - n as i64;
        self.digits = res.iter().rev().map(|d| *d as u8).collect();
        self.point += added;
        self.trim();
    }
}

// ---------------------------------------------------------------- formatter

/// A number formatter.
#[derive(Clone, Debug)]
pub struct NumberFormat {
    pub locale: String,
    pub style: i64,
    pub pattern: Pattern,
    /// The pattern as set (for rule-based styles: the rules).
    pub pattern_text: String,
    pub symbols: Vec<String>,
    pub currency: String,
    pub rounding_mode: i64,
    pub lenient: bool,
    pub parse_int_only: bool,
    /// Explicit affixes (setTextAttribute).
    pub text_affix: [Option<String>; 4],
    pub pad_char: char,
    pub pad_width: i64,
    pub pad_pos: i64,
    pub min_grouping: i32,
    chain: Vec<String>,
    /// Rule-based: the default rule set name.
    pub ruleset: Option<String>,
    /// The currency spacing (inserted between an alphabetic currency
    /// symbol and the digits).
    spacing: String,
}

/// Why a formatter could not be created.
#[derive(Debug, PartialEq)]
pub enum CreateError {
    /// U_ILLEGAL_ARGUMENT_ERROR / U_UNSUPPORTED_ERROR...
    Unsupported,
    Pattern,
    BadStyle,
}

impl NumberFormat {
    pub fn new(id: &str, style: i64, pattern: Option<&str>) -> Result<NumberFormat, CreateError> {
        let chain = chain(id);
        let p = locale::parse(id);
        let sym = |i: usize, d: &str| symbol_data(&chain, i).unwrap_or(d).to_string();
        let decimal = sym(0, ".");
        let group = sym(1, ",");
        let mut symbols = vec![String::new(); SYMBOL_COUNT];
        symbols[DECIMAL_SEPARATOR_SYMBOL as usize] = decimal.clone();
        symbols[GROUPING_SEPARATOR_SYMBOL as usize] = group.clone();
        symbols[PATTERN_SEPARATOR_SYMBOL as usize] = ";".into();
        symbols[PERCENT_SYMBOL as usize] = sym(2, "%");
        symbols[ZERO_DIGIT_SYMBOL as usize] = "0".into();
        symbols[DIGIT_SYMBOL as usize] = "#".into();
        symbols[MINUS_SIGN_SYMBOL as usize] = sym(4, "-");
        symbols[PLUS_SIGN_SYMBOL as usize] = sym(3, "+");
        symbols[EXPONENTIAL_SYMBOL as usize] = sym(5, "E");
        symbols[PERMILL_SYMBOL as usize] = sym(6, "‰");
        symbols[PAD_ESCAPE_SYMBOL as usize] = "*".into();
        symbols[INFINITY_SYMBOL as usize] = sym(7, "∞");
        symbols[NAN_SYMBOL as usize] = sym(8, "NaN");
        symbols[SIGNIFICANT_DIGIT_SYMBOL as usize] = "@".into();
        symbols[MONETARY_SEPARATOR_SYMBOL as usize] =
            symbol_data(&chain, 9).map_or(decimal, str::to_string);
        symbols[MONETARY_GROUPING_SEPARATOR_SYMBOL as usize] =
            symbol_data(&chain, 10).map_or(group, str::to_string);
        let currency = p
            .keywords
            .get("currency")
            .map(|c| c.to_ascii_uppercase())
            // a locale without a region has no currency (ICU's XXX:
            // ext/intl/tests/formatter_format8.phpt, "de")
            .or_else(|| region_currency(&p.region).map(str::to_string))
            .unwrap_or_else(|| "XXX".to_string());
        // the unknown currency shows its code (ICU 62+:
        // ext/intl/tests/formatter_format8.phpt)
        symbols[CURRENCY_SYMBOL as usize] = currency_symbol(&chain, &currency);
        symbols[INTL_CURRENCY_SYMBOL as usize] = if currency == "XXX" {
            "XXX".to_string()
        } else {
            currency.clone()
        };
        // DecimalFormat groups from 2 digits whatever CLDR's
        // minimumGroupingDigits (ext/intl/tests/gh11942_numfmt_locale_
        // canonicalization.phpt: pt_PT "1 234,56")
        let min_grouping = 1;
        let spacing = format_data(&chain, "spacing")
            .unwrap_or("\u{a0}")
            .to_string();
        let (pattern_text, ruleset) = match style {
            PATTERN_DECIMAL => (pattern.unwrap_or("#").to_string(), None),
            DECIMAL => (
                format_data(&chain, "decimal")
                    .unwrap_or("#,##0.###")
                    .to_string(),
                None,
            ),
            PERCENT => (
                format_data(&chain, "percent")
                    .unwrap_or("#,##0%")
                    .to_string(),
                None,
            ),
            SCIENTIFIC => (
                format_data(&chain, "scientific")
                    .unwrap_or("#E0")
                    .to_string(),
                None,
            ),
            CURRENCY | CURRENCY_STANDARD | CASH_CURRENCY => {
                let alpha = symbols[CURRENCY_SYMBOL as usize]
                    .chars()
                    .last()
                    .is_some_and(char::is_alphabetic);
                let key = if alpha { "currency-alpha" } else { "currency" };
                (
                    format_data(&chain, key)
                        .or_else(|| format_data(&chain, "currency"))
                        .unwrap_or("¤#,##0.00")
                        .to_string(),
                    None,
                )
            }
            CURRENCY_ACCOUNTING => {
                let alpha = symbols[CURRENCY_SYMBOL as usize]
                    .chars()
                    .last()
                    .is_some_and(char::is_alphabetic);
                let key = if alpha {
                    "accounting-alpha"
                } else {
                    "accounting"
                };
                (
                    format_data(&chain, key)
                        .or_else(|| format_data(&chain, "accounting"))
                        .unwrap_or("¤#,##0.00")
                        .to_string(),
                    None,
                )
            }
            CURRENCY_ISO => {
                let base = format_data(&chain, "currency-alpha")
                    .or_else(|| format_data(&chain, "currency"))
                    .unwrap_or("¤#,##0.00");
                (base.replace('¤', "¤¤"), None)
            }
            CURRENCY_PLURAL => {
                // CLDR's unitPattern "{0} {1}" around the decimal pattern
                let dec = format_data(&chain, "decimal").unwrap_or("#,##0.###");
                let int = dec.split('.').next().unwrap_or("#,##0");
                (format!("{int}.00 ¤¤¤"), None)
            }
            DECIMAL_COMPACT_SHORT | DECIMAL_COMPACT_LONG => (
                format_data(&chain, "decimal")
                    .unwrap_or("#,##0.###")
                    .to_string(),
                None,
            ),
            SPELLOUT => (String::new(), Some("%spellout-numbering".to_string())),
            ORDINAL => (String::new(), Some("%digits-ordinal".to_string())),
            DURATION => (String::new(), Some("%duration".to_string())),
            PATTERN_RULEBASED => match pattern {
                Some(r) => (r.to_string(), Some(String::new())),
                None => return Err(CreateError::Unsupported),
            },
            _ => return Err(CreateError::BadStyle),
        };
        let pat = if ruleset.is_some() {
            parse_pattern("#,##0.###").map_err(|_| CreateError::Pattern)?
        } else {
            parse_pattern(&pattern_text).map_err(|_| CreateError::Pattern)?
        };
        let mut nf = NumberFormat {
            locale: id.to_string(),
            style,
            pattern: pat,
            pattern_text,
            symbols,
            currency,
            rounding_mode: ROUND_HALFEVEN,
            lenient: false,
            parse_int_only: false,
            text_affix: [None, None, None, None],
            pad_char: ' ',
            pad_width: 0,
            pad_pos: PAD_BEFORE_PREFIX,
            min_grouping,
            chain,
            ruleset,
            spacing,
        };
        if style == PATTERN_DECIMAL && pattern.is_none() {
            // ICU's pattern for a null PATTERN_DECIMAL pattern: no grouping,
            // all fraction digits
            // ICU's empty pattern: "#" showing every fraction digit
            // (ext/intl/tests/formatter_get_set_pattern2.phpt)
            nf.pattern.max_frac = UNLIMITED_FRAC;
        }
        nf.apply_currency_digits();
        nf.sync_pad();
        Ok(nf)
    }

    fn sync_pad(&mut self) {
        if let Some((c, w, pos)) = self.pattern.pad {
            self.pad_char = c;
            self.pad_width = w;
            self.pad_pos = pos;
        }
    }

    fn is_currency_style(&self) -> bool {
        matches!(
            self.style,
            CURRENCY
                | CURRENCY_ISO
                | CURRENCY_PLURAL
                | CURRENCY_ACCOUNTING
                | CASH_CURRENCY
                | CURRENCY_STANDARD
        )
    }

    fn apply_currency_digits(&mut self) {
        if self.pattern.has_currency() && !self.pattern.sig_used && self.currency != "XXX" {
            let d = if self.style == CASH_CURRENCY {
                currency_cash_digits(&self.currency)
            } else {
                currency_digits(&self.currency)
            } as i32;
            self.pattern.min_frac = d;
            self.pattern.max_frac = d;
        }
    }

    /// Sets the pattern (`setPattern()`).
    pub fn set_pattern(&mut self, pat: &str) -> Result<(), PatternError> {
        if self.ruleset.is_some() {
            self.pattern_text = pat.to_string();
            return Ok(());
        }
        let p = parse_pattern(pat)?;
        self.pattern = p;
        self.pattern_text = pat.to_string();
        self.text_affix = [None, None, None, None];
        self.apply_currency_digits();
        self.pad_width = 0;
        self.sync_pad();
        Ok(())
    }

    pub fn is_rule_based(&self) -> bool {
        self.ruleset.is_some()
    }

    /// The pattern as ICU writes it back (`getPattern()`).
    pub fn to_pattern(&self) -> String {
        if self.ruleset.is_some() {
            return self.pattern_text.clone();
        }
        let p = &self.pattern;
        let mut s = String::new();
        let affix = |v: &Vec<AffixPart>| -> String {
            let mut o = String::new();
            for a in v {
                match a {
                    AffixPart::Lit(l) => {
                        for c in l.chars() {
                            if matches!(
                                c,
                                '#' | '0'
                                    ..='9'
                                        | ','
                                        | '.'
                                        | '@'
                                        | ';'
                                        | '%'
                                        | '‰'
                                        | '¤'
                                        | '-'
                                        | '+'
                                        | '*'
                                        | 'E'
                            ) {
                                o.push('\'');
                                o.push(c);
                                o.push('\'');
                            } else if c == '\'' {
                                o.push_str("''");
                            } else {
                                o.push(c);
                            }
                        }
                    }
                    AffixPart::Currency(n) => {
                        for _ in 0..*n {
                            o.push('¤');
                        }
                    }
                    AffixPart::Percent => o.push('%'),
                    AffixPart::Permill => o.push('‰'),
                    AffixPart::Minus => o.push('-'),
                    AffixPart::Plus => o.push('+'),
                }
            }
            o
        };
        let mut pos_prefix = affix(&p.pos_prefix);
        let mut pos_suffix = affix(&p.pos_suffix);
        if let Some(t) = &self.text_affix[0] {
            pos_prefix = t.clone();
        }
        if let Some(t) = &self.text_affix[1] {
            pos_suffix = t.clone();
        }
        let number = self.number_pattern();
        if self.pad_width > 0 && self.pad_pos == PAD_BEFORE_PREFIX {
            s.push('*');
            s.push(self.pad_char);
        }
        s.push_str(&pos_prefix);
        if self.pad_width > 0 && self.pad_pos == PAD_AFTER_PREFIX {
            s.push('*');
            s.push(self.pad_char);
        }
        s.push_str(&number);
        if self.pad_width > 0 && self.pad_pos == PAD_BEFORE_SUFFIX {
            s.push('*');
            s.push(self.pad_char);
        }
        s.push_str(&pos_suffix);
        if self.pad_width > 0 && self.pad_pos == PAD_AFTER_SUFFIX {
            s.push('*');
            s.push(self.pad_char);
        }
        if let Some((np, ns)) = &p.neg {
            let np = affix(np);
            let ns = affix(ns);
            let implied = np == format!("-{pos_prefix}") && ns == pos_suffix;
            if !implied {
                s.push(';');
                s.push_str(&np);
                s.push_str(&number);
                s.push_str(&ns);
            }
        }
        s
    }

    fn number_pattern(&self) -> String {
        let p = &self.pattern;
        let mut s = String::new();
        if p.sig_used {
            let mut int = String::new();
            // ICU writes at most 100 significant digits back
            // (ext/intl/tests/formatter_get_set_pattern2.phpt)
            for _ in 0..p.min_sig.min(100) {
                int.push('@');
            }
            for _ in p.min_sig.min(100)..p.max_sig.min(100) {
                int.push('#');
            }
            if p.grouping_used && p.grouping > 0 {
                let need = (p.grouping + 1) as usize;
                while int.len() < need {
                    int.insert(0, '#');
                }
                s = group_pattern(&int, p.grouping as usize, p.grouping2 as usize);
            } else {
                s = int;
            }
        } else {
            let mut int = String::new();
            for _ in 0..p.min_int {
                int.push('0');
            }
            if p.exp.is_some() {
                let max = p.max_int.min(100);
                while (int.len() as i32) < max {
                    int.insert(0, '#');
                }
            }
            if p.grouping_used && p.grouping > 0 {
                let need = if p.grouping2 != p.grouping && p.grouping2 > 0 {
                    (p.grouping + p.grouping2 + 1) as usize
                } else {
                    (p.grouping + 1) as usize
                };
                while int.len() < need {
                    int.insert(0, '#');
                }
                s.push_str(&group_pattern(
                    &int,
                    p.grouping as usize,
                    p.grouping2 as usize,
                ));
            } else {
                if int.is_empty() {
                    int.insert(0, '#');
                }
                s.push_str(&int);
            }
            if let Some(inc) = p.increment {
                // the increment's digits replace the zeros
                let txt = Dec::from_f64(inc).plain();
                let (ip, fp) = txt.split_once('.').unwrap_or((&txt, ""));
                let ilen = s.len();
                if ip != "0" {
                    let ip = ip.to_string();
                    s.truncate(ilen.saturating_sub(ip.len()));
                    s.push_str(&ip);
                }
                if !fp.is_empty() || p.max_frac > 0 {
                    s.push('.');
                    let mut f = fp.to_string();
                    while (f.len() as i32) < p.min_frac {
                        f.push('0');
                    }
                    s.push_str(&f);
                }
            } else if (p.max_frac > 0 && p.max_frac < UNLIMITED_FRAC) || p.decimal_shown {
                s.push('.');
                for _ in 0..p.min_frac {
                    s.push('0');
                }
                for _ in p.min_frac..p.max_frac {
                    s.push('#');
                }
            }
            if let Some((n, sign)) = p.exp {
                s.push('E');
                if sign {
                    s.push('+');
                }
                for _ in 0..n {
                    s.push('0');
                }
            }
        }
        s
    }

    // ------------------------------------------------------------ attributes

    pub fn attribute(&self, attr: i64) -> Option<f64> {
        let p = &self.pattern;
        Some(match attr {
            PARSE_INT_ONLY => self.parse_int_only as i64 as f64,
            GROUPING_USED => p.grouping_used as i64 as f64,
            DECIMAL_ALWAYS_SHOWN => p.decimal_shown as i64 as f64,
            MAX_INTEGER_DIGITS => p.max_int as f64,
            MIN_INTEGER_DIGITS | INTEGER_DIGITS => p.min_int as f64,
            MAX_FRACTION_DIGITS => p.max_frac as f64,
            MIN_FRACTION_DIGITS | FRACTION_DIGITS => p.min_frac as f64,
            MULTIPLIER => p.multiplier as f64,
            GROUPING_SIZE => p.grouping as f64,
            ROUNDING_MODE => self.rounding_mode as f64,
            ROUNDING_INCREMENT => p.increment.unwrap_or(0.0),
            FORMAT_WIDTH => self.pad_width as f64,
            PADDING_POSITION => self.pad_pos as f64,
            SECONDARY_GROUPING_SIZE => {
                if p.grouping2 == p.grouping {
                    0.0
                } else {
                    p.grouping2 as f64
                }
            }
            SIGNIFICANT_DIGITS_USED => p.sig_used as i64 as f64,
            MIN_SIGNIFICANT_DIGITS => p.min_sig.max(1) as f64,
            MAX_SIGNIFICANT_DIGITS => p.max_sig.max(6) as f64,
            LENIENT_PARSE => self.lenient as i64 as f64,
            _ => return None,
        })
    }

    pub fn set_attribute(&mut self, attr: i64, v: f64) -> bool {
        let iv = v as i64;
        let i32v = iv.clamp(0, 2_000_000_000) as i32;
        let p = &mut self.pattern;
        match attr {
            PARSE_INT_ONLY => self.parse_int_only = iv != 0,
            GROUPING_USED => p.grouping_used = iv != 0,
            DECIMAL_ALWAYS_SHOWN => p.decimal_shown = iv != 0,
            MAX_INTEGER_DIGITS => {
                p.max_int = i32v;
                if p.min_int > p.max_int {
                    p.min_int = p.max_int;
                }
            }
            MIN_INTEGER_DIGITS => {
                p.min_int = i32v;
                if p.max_int < p.min_int {
                    p.max_int = p.min_int;
                }
            }
            INTEGER_DIGITS => {
                p.min_int = i32v;
                p.max_int = i32v;
            }
            MAX_FRACTION_DIGITS => {
                p.max_frac = i32v.min(999);
                if p.min_frac > p.max_frac {
                    p.min_frac = p.max_frac;
                }
            }
            MIN_FRACTION_DIGITS => {
                p.min_frac = i32v.min(999);
                if p.max_frac < p.min_frac {
                    p.max_frac = p.min_frac;
                }
            }
            FRACTION_DIGITS => {
                p.min_frac = i32v.min(999);
                p.max_frac = i32v.min(999);
            }
            MULTIPLIER => p.multiplier = iv as i32,
            GROUPING_SIZE => {
                if p.grouping2 == p.grouping {
                    p.grouping2 = iv as i32;
                }
                p.grouping = iv as i32;
            }
            ROUNDING_MODE => {
                if !(0..=10).contains(&iv) {
                    return false;
                }
                self.rounding_mode = iv;
            }
            ROUNDING_INCREMENT => p.increment = if v > 0.0 { Some(v) } else { None },
            FORMAT_WIDTH => self.pad_width = iv.max(0),
            PADDING_POSITION => {
                if !(0..=3).contains(&iv) {
                    return false;
                }
                self.pad_pos = iv;
            }
            SECONDARY_GROUPING_SIZE => p.grouping2 = iv as i32,
            SIGNIFICANT_DIGITS_USED => p.sig_used = iv != 0,
            MIN_SIGNIFICANT_DIGITS => {
                p.sig_used = true;
                p.min_sig = i32v.max(1);
                if p.max_sig < p.min_sig {
                    p.max_sig = p.min_sig;
                }
            }
            MAX_SIGNIFICANT_DIGITS => {
                p.sig_used = true;
                p.max_sig = i32v.max(1);
                if p.min_sig > p.max_sig || p.min_sig == 0 {
                    p.min_sig = p.min_sig.clamp(1, p.max_sig);
                }
            }
            LENIENT_PARSE => self.lenient = iv != 0,
            _ => return false,
        }
        true
    }

    pub fn text_attribute(&self, attr: i64) -> Option<String> {
        let render = |v: &Vec<AffixPart>| self.render_affix(v, false);
        Some(match attr {
            POSITIVE_PREFIX => self.text_affix[0]
                .clone()
                .unwrap_or_else(|| render(&self.pattern.pos_prefix)),
            POSITIVE_SUFFIX => self.text_affix[1]
                .clone()
                .unwrap_or_else(|| render(&self.pattern.pos_suffix)),
            NEGATIVE_PREFIX => self.text_affix[2]
                .clone()
                .unwrap_or_else(|| self.neg_affixes().0),
            NEGATIVE_SUFFIX => self.text_affix[3]
                .clone()
                .unwrap_or_else(|| self.neg_affixes().1),
            PADDING_CHARACTER => self.pad_char.to_string(),
            CURRENCY_CODE => self.currency.clone(),
            DEFAULT_RULESET => self.ruleset.clone()?,
            PUBLIC_RULESETS => {
                self.ruleset.as_ref()?;
                match self.style {
                    SPELLOUT => "%spellout-numbering-year;%spellout-numbering;%spellout-numbering-verbose;%spellout-cardinal;%spellout-ordinal;%spellout-ordinal-verbose;".into(),
                    ORDINAL => "%digits-ordinal;".into(),
                    DURATION => "%duration;".into(),
                    _ => String::new(),
                }
            }
            _ => return None,
        })
    }

    pub fn set_text_attribute(&mut self, attr: i64, v: &str) -> bool {
        match attr {
            POSITIVE_PREFIX..=NEGATIVE_SUFFIX => {
                self.text_affix[attr as usize] = Some(v.to_string())
            }
            PADDING_CHARACTER => match v.chars().next() {
                Some(c) => self.pad_char = c,
                None => return false,
            },
            CURRENCY_CODE => {
                if v.chars().count() != 3 {
                    return false;
                }
                self.currency = v.to_ascii_uppercase();
                self.symbols[CURRENCY_SYMBOL as usize] =
                    currency_symbol(&self.chain, &self.currency);
                self.symbols[INTL_CURRENCY_SYMBOL as usize] = self.currency.clone();
                self.apply_currency_digits();
            }
            DEFAULT_RULESET => {
                if self.ruleset.is_none() {
                    return false;
                }
                self.ruleset = Some(v.to_string());
            }
            _ => return false,
        }
        true
    }

    pub fn symbol(&self, s: i64) -> Option<&str> {
        self.symbols
            .get(usize::try_from(s).ok()?)
            .map(String::as_str)
    }

    pub fn set_symbol(&mut self, s: i64, v: &str) -> bool {
        match usize::try_from(s).ok().filter(|i| *i < SYMBOL_COUNT) {
            Some(i) => {
                self.symbols[i] = v.to_string();
                true
            }
            None => false,
        }
    }

    // ------------------------------------------------------------ formatting

    fn render_affix(&self, v: &[AffixPart], plural_one: bool) -> String {
        let mut o = String::new();
        for a in v {
            match a {
                AffixPart::Lit(s) => o.push_str(s),
                AffixPart::Currency(1) => o.push_str(&self.symbols[CURRENCY_SYMBOL as usize]),
                AffixPart::Currency(2) => o.push_str(&self.symbols[INTL_CURRENCY_SYMBOL as usize]),
                AffixPart::Currency(3) => o.push_str(&currency_name(
                    &self.chain,
                    &self.currency,
                    Some(if plural_one { "one" } else { "other" }),
                )),
                AffixPart::Currency(_) => o.push_str(&self.symbols[CURRENCY_SYMBOL as usize]),
                AffixPart::Percent => o.push_str(&self.symbols[PERCENT_SYMBOL as usize]),
                AffixPart::Permill => o.push_str(&self.symbols[PERMILL_SYMBOL as usize]),
                AffixPart::Minus => o.push_str(&self.symbols[MINUS_SIGN_SYMBOL as usize]),
                AffixPart::Plus => o.push_str(&self.symbols[PLUS_SIGN_SYMBOL as usize]),
            }
        }
        o
    }

    fn neg_affixes(&self) -> (String, String) {
        match &self.pattern.neg {
            Some((np, ns)) => (self.render_affix(np, false), self.render_affix(ns, false)),
            None => {
                let pp = self.text_affix[0]
                    .clone()
                    .unwrap_or_else(|| self.render_affix(&self.pattern.pos_prefix, false));
                let ps = self.text_affix[1]
                    .clone()
                    .unwrap_or_else(|| self.render_affix(&self.pattern.pos_suffix, false));
                (
                    format!("{}{}", self.symbols[MINUS_SIGN_SYMBOL as usize], pp),
                    ps,
                )
            }
        }
    }

    /// Formats a double (`format()` with TYPE_DEFAULT/TYPE_DOUBLE).
    pub fn format_f64(&self, x: f64) -> Result<String, Inexact> {
        if let Some(r) = &self.ruleset {
            return Ok(self.format_rbnf(r, x));
        }
        if self.style == DECIMAL_COMPACT_SHORT || self.style == DECIMAL_COMPACT_LONG {
            return Ok(self.format_compact(x));
        }
        if x.is_nan() {
            return Ok(self.symbols[NAN_SYMBOL as usize].clone());
        }
        let neg = x.is_sign_negative();
        if x.is_infinite() {
            return Ok(self.affix_wrap(neg, self.symbols[INFINITY_SYMBOL as usize].clone(), false));
        }
        let d = Dec::from_f64(x);
        self.format_dec(d, neg)
    }

    /// Formats an integer exactly.
    pub fn format_i64(&self, x: i64) -> Result<String, Inexact> {
        if let Some(r) = &self.ruleset {
            return Ok(self.format_rbnf(r, x as f64));
        }
        if self.style == DECIMAL_COMPACT_SHORT || self.style == DECIMAL_COMPACT_LONG {
            return Ok(self.format_compact(x as f64));
        }
        self.format_dec(Dec::from_i128(x as i128), x < 0)
    }

    /// Formats a decimal (|value| with its sign).
    pub fn format_dec(&self, mut d: Dec, neg: bool) -> Result<String, Inexact> {
        let p = &self.pattern;
        let mult = p.multiplier;
        if mult != 1 {
            match mult {
                100 => d.mul_pow10(2),
                1000 => d.mul_pow10(3),
                m => d.mul_int(m as i64),
            }
        }
        let neg = neg ^ (mult < 0);
        let body = if let Some((min_exp, exp_sign)) = p.exp {
            self.format_scientific(d, neg, min_exp, exp_sign)?
        } else {
            self.format_fixed(d, neg)?
        };
        let (body, _is_zero) = body;
        let one = self.plural_one(&body);
        Ok(self.affix_wrap(neg, body, one))
    }

    /// Whether the formatted digits take CLDR's "one" plural form (for
    /// the currency plural names).
    fn plural_one(&self, body: &str) -> bool {
        if !self
            .pattern
            .pos_suffix
            .iter()
            .chain(self.pattern.pos_prefix.iter())
            .any(|a| *a == AffixPart::Currency(3))
        {
            return false;
        }
        let dsep = &self.symbols[MONETARY_SEPARATOR_SYMBOL as usize];
        let mut t = String::new();
        let mut rest = body;
        while let Some(c) = rest.chars().next() {
            if rest.starts_with(dsep.as_str()) && !dsep.is_empty() {
                t.push('.');
                rest = &rest[dsep.len()..];
                continue;
            }
            if c.is_ascii_digit() {
                t.push(c);
            }
            rest = &rest[c.len_utf8()..];
        }
        let lang = locale::parse(&self.locale).language;
        intl::plural::PluralOperands::parse(&t).is_some_and(|ops| {
            intl::plural::plural_category(&lang, &ops) == intl::plural::PluralCategory::One
        })
    }

    fn currency_in_affix(&self) -> bool {
        self.pattern.has_currency()
    }

    fn affix_wrap(&self, neg: bool, body: String, one: bool) -> String {
        let (mut prefix, mut suffix) = if neg {
            let (np, ns) = match (&self.text_affix[2], &self.text_affix[3]) {
                (None, None) => match &self.pattern.neg {
                    Some((np, ns)) => (self.render_affix(np, one), self.render_affix(ns, one)),
                    None => (
                        format!(
                            "{}{}",
                            self.symbols[MINUS_SIGN_SYMBOL as usize],
                            self.text_affix[0]
                                .clone()
                                .unwrap_or_else(|| self.render_affix(&self.pattern.pos_prefix, one))
                        ),
                        self.text_affix[1]
                            .clone()
                            .unwrap_or_else(|| self.render_affix(&self.pattern.pos_suffix, one)),
                    ),
                },
                _ => {
                    let (dp, ds) = self.neg_affixes();
                    (
                        self.text_affix[2].clone().unwrap_or(dp),
                        self.text_affix[3].clone().unwrap_or(ds),
                    )
                }
            };
            (np, ns)
        } else {
            (
                self.text_affix[0]
                    .clone()
                    .unwrap_or_else(|| self.render_affix(&self.pattern.pos_prefix, one)),
                self.text_affix[1]
                    .clone()
                    .unwrap_or_else(|| self.render_affix(&self.pattern.pos_suffix, one)),
            )
        };
        // currency spacing (UTS #35 section 3.10 currencySpacing): an
        // alphabetic currency symbol next to the digits gets a space
        if self.currency_in_affix() {
            let first_digit = body.chars().next().is_some_and(|c| c.is_ascii_digit());
            let last_digit = body.chars().last().is_some_and(|c| c.is_ascii_digit());
            if first_digit && prefix.chars().last().is_some_and(char::is_alphabetic) {
                prefix.push_str(&self.spacing);
            }
            if last_digit && suffix.chars().next().is_some_and(char::is_alphabetic) {
                suffix.insert_str(0, &self.spacing);
            }
        }
        let len = |s: &str| s.chars().count() as i64;
        let total = len(&prefix) + len(&body) + len(&suffix);
        let mut pad = String::new();
        if self.pad_width > total {
            for _ in 0..(self.pad_width - total) {
                pad.push(self.pad_char);
            }
        }
        match self.pad_pos {
            PAD_AFTER_PREFIX => format!("{prefix}{pad}{body}{suffix}"),
            PAD_BEFORE_SUFFIX => format!("{prefix}{body}{pad}{suffix}"),
            PAD_AFTER_SUFFIX => format!("{prefix}{body}{suffix}{pad}"),
            _ => format!("{pad}{prefix}{body}{suffix}"),
        }
    }

    fn digit_char(&self, d: u8) -> String {
        let zero = &self.symbols[ZERO_DIGIT_SYMBOL as usize];
        let mut zc = zero.chars();
        match (zc.next(), zc.next()) {
            (Some(z), None) => char::from_u32(z as u32 + d as u32)
                .unwrap_or('0')
                .to_string(),
            _ => ((b'0' + d) as char).to_string(),
        }
    }

    fn format_fixed(&self, mut d: Dec, neg: bool) -> Result<(String, bool), Inexact> {
        let p = &self.pattern;
        if let Some(inc) = p.increment {
            // round to a multiple of the increment
            let v = d.to_f64() / inc;
            let mut q = Dec::from_f64(v);
            if !q.round_frac(0, self.rounding_mode, neg) {
                return Err(Inexact);
            }
            let r = q.to_f64() * inc;
            d = Dec::from_f64(r);
            let frac = Dec::from_f64(inc)
                .plain()
                .split_once('.')
                .map_or(0, |(_, f)| f.len() as i64);
            d.round_frac(frac.max(p.max_frac as i64), ROUND_HALFEVEN, neg);
        }
        let (min_frac, max_frac) = if p.sig_used {
            if !d.round_sig(p.max_sig as i64, self.rounding_mode, neg) {
                return Err(Inexact);
            }
            let sig_digits = d.digits.len() as i64;
            let min_frac = (p.min_sig as i64
                - d.point.max(if d.digits.is_empty() { 1 } else { d.point }))
            .max(0);
            let _ = sig_digits;
            (min_frac, i64::MAX)
        } else {
            if !d.round_frac(p.max_frac as i64, self.rounding_mode, neg) {
                return Err(Inexact);
            }
            (p.min_frac as i64, p.max_frac as i64)
        };
        let zero = d.is_zero();
        // integer digits
        let int_len = d.point.max(0);
        let max_int = p.max_int as i64;
        let mut int_digits: Vec<u8> = (0..int_len).rev().map(|i| d.digit_at(i)).collect();
        if int_digits.len() as i64 > max_int {
            let cut = int_digits.len() - max_int as usize;
            int_digits.drain(..cut);
        }
        let min_int = if p.sig_used { 1 } else { p.min_int as i64 };
        while (int_digits.len() as i64) < min_int {
            int_digits.insert(0, 0);
        }
        // "#" alone still shows a zero
        let no_frac = d.digits.len() as i64 <= d.point || max_frac == 0;
        if int_digits.is_empty() && (no_frac || d.is_zero()) && !p.decimal_shown {
            int_digits.push(0);
        }
        let mut s = String::new();
        let grouping = if self.is_currency_style() {
            &self.symbols[MONETARY_GROUPING_SEPARATOR_SYMBOL as usize]
        } else {
            &self.symbols[GROUPING_SEPARATOR_SYMBOL as usize]
        };
        let n = int_digits.len();
        let g1 = p.grouping.max(0) as usize;
        let g2 = if p.grouping2 > 0 {
            p.grouping2 as usize
        } else {
            g1
        };
        let use_group = p.grouping_used && g1 > 0 && n >= g1 + self.min_grouping.max(1) as usize;
        for (i, dg) in int_digits.iter().enumerate() {
            s.push_str(&self.digit_char(*dg));
            let left = n - i - 1;
            if use_group
                && left > 0
                && (left == g1 || (left > g1 && (left - g1).is_multiple_of(g2)))
            {
                s.push_str(grouping);
            }
        }
        // fraction digits
        let mut frac: Vec<u8> = Vec::new();
        let last = d.digits.len() as i64 - d.point; // fraction digits present
        for i in 0..last.max(0) {
            frac.push(d.digit_at(-1 - i));
        }
        while (frac.len() as i64) < min_frac {
            frac.push(0);
        }
        if max_frac != i64::MAX {
            frac.truncate(max_frac.max(min_frac) as usize);
        }
        let dsep = if self.is_currency_style() {
            &self.symbols[MONETARY_SEPARATOR_SYMBOL as usize]
        } else {
            &self.symbols[DECIMAL_SEPARATOR_SYMBOL as usize]
        };
        if !frac.is_empty() || p.decimal_shown {
            s.push_str(dsep);
            for dg in frac {
                s.push_str(&self.digit_char(dg));
            }
        }
        Ok((s, zero))
    }

    fn format_scientific(
        &self,
        mut d: Dec,
        neg: bool,
        min_exp: i32,
        exp_sign: bool,
    ) -> Result<(String, bool), Inexact> {
        let p = &self.pattern;
        let max_int = p.max_int.max(1) as i64;
        let min_int = p.min_int as i64;
        // significant digits: min integer + max fraction (all digits when
        // the pattern gives none: "#E0")
        let sig = if p.sig_used {
            p.max_sig as i64
        } else {
            min_int.max(1) + p.max_frac as i64
        };
        let all = !p.sig_used && min_int == 0 && p.max_frac == 0;
        if !all && !d.round_sig(sig.max(1), self.rounding_mode, neg) {
            return Err(Inexact);
        }
        let zero = d.is_zero();
        let mut exp = if zero { 0 } else { d.point - 1 };
        let engineering = max_int > min_int && max_int > 1;
        let int_digits = if engineering {
            let e = exp.rem_euclid(max_int);
            exp -= e;
            e + 1
        } else {
            let k = min_int.max(1);
            exp -= k - 1;
            k
        };
        // mantissa digits
        let mut s = String::new();
        let digits = &d.digits;
        for i in 0..int_digits {
            s.push_str(&self.digit_char(digits.get(i as usize).copied().unwrap_or(0)));
        }
        let mut frac: Vec<u8> = digits.iter().skip(int_digits as usize).copied().collect();
        let min_frac = if p.sig_used {
            (p.min_sig as i64 - int_digits).max(0)
        } else {
            p.min_frac as i64
        };
        while (frac.len() as i64) < min_frac {
            frac.push(0);
        }
        if !frac.is_empty() || p.decimal_shown {
            s.push_str(&self.symbols[DECIMAL_SEPARATOR_SYMBOL as usize]);
            for dg in frac {
                s.push_str(&self.digit_char(dg));
            }
        }
        s.push_str(&self.symbols[EXPONENTIAL_SYMBOL as usize]);
        if exp < 0 {
            s.push_str(&self.symbols[MINUS_SIGN_SYMBOL as usize]);
        } else if exp_sign {
            s.push_str(&self.symbols[PLUS_SIGN_SYMBOL as usize]);
        }
        let es = exp.unsigned_abs().to_string();
        for _ in es.len()..min_exp as usize {
            s.push_str(&self.digit_char(0));
        }
        for c in es.bytes() {
            s.push_str(&self.digit_char(c - b'0'));
        }
        Ok((s, zero))
    }

    fn format_compact(&self, x: f64) -> String {
        use intl::number::{CompactDisplay, Notation, NumberFormatOptions};
        let mut opts = NumberFormatOptions::default();
        opts.notation = Notation::Compact;
        opts.compact_display = if self.style == DECIMAL_COMPACT_LONG {
            CompactDisplay::Long
        } else {
            CompactDisplay::Short
        };
        intl::number::format(&locale::to_bcp47(&self.locale), x, &opts)
    }

    // ------------------------------------------------------------ rule-based

    fn format_rbnf(&self, ruleset: &str, x: f64) -> String {
        let lang = locale::parse(&self.locale).language;
        if self.style == PATTERN_RULEBASED {
            return format_user_rules(&self.pattern_text, x);
        }
        if x.is_nan() {
            return "NaN".into();
        }
        let neg = x < 0.0;
        let ax = x.abs();
        let decimal = |v: f64| {
            let f = NumberFormat::new(&self.locale, DECIMAL, None).ok();
            f.and_then(|f| f.format_f64(v).ok()).unwrap_or_default()
        };
        match self.style {
            ORDINAL => {
                // the value rounds half-even to an integer
                let mut d = Dec::from_f64(ax);
                d.round_frac(0, ROUND_HALFEVEN, false);
                let n = d.to_f64() as i64;
                let s = intl::number::format_ordinal(&locale::to_bcp47(&self.locale), n);
                // the digits use the locale's decimal grouping
                let plain = intl::number::format_decimal(&locale::to_bcp47(&self.locale), n as f64);
                let rest = s.get(plain.len()..).unwrap_or("");
                let mut out = decimal(n as f64);
                if lang != "en" && rest.is_empty() {
                    out.push('.');
                } else {
                    out.push_str(rest);
                }
                if neg { format!("-{out}") } else { out }
            }
            DURATION => {
                if lang != "en" {
                    return decimal(x);
                }
                let total = ax.round() as i64;
                let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
                let body = if h > 0 {
                    format!("{}:{:02}:{:02}", decimal(h as f64), m, s)
                } else if m > 0 {
                    format!("{m}:{s:02}")
                } else {
                    format!("{s} sec.")
                };
                if neg { format!("-{body}") } else { body }
            }
            _ => {
                let tag = locale::to_bcp47(&self.locale);
                let int = ax.trunc();
                let spell_int = |n: i64| -> String {
                    if ruleset.is_empty() || ruleset == "%spellout-numbering" {
                        intl::spellout::spell_cardinal(&tag, n)
                    } else {
                        intl::spellout::spell_rule_set(&tag, ruleset, n)
                            .or_else(|| intl::spellout::spell_cardinal(&tag, n))
                    }
                    .unwrap_or_else(|| n.to_string())
                };
                let mut out = if int >= 9.2e18 {
                    decimal(int)
                } else {
                    spell_int(int as i64)
                };
                let frac = ax - int;
                if frac > 0.0 {
                    // "x.x: << point >>": the fraction digit by digit
                    let d = Dec::from_f64(ax);
                    let digits: Vec<u8> = (0..(d.digits.len() as i64 - d.point).max(0))
                        .map(|i| d.digit_at(-1 - i))
                        .collect();
                    let point = match lang.as_str() {
                        "de" => "Komma",
                        "fr" => "virgule",
                        "es" => "punto",
                        "it" => "virgola",
                        "nl" => "komma",
                        "pt" => "vírgula",
                        "sv" => "komma",
                        _ => "point",
                    };
                    out.push(' ');
                    out.push_str(point);
                    for dg in digits {
                        out.push(' ');
                        out.push_str(&spell_int(dg as i64));
                    }
                }
                if neg {
                    let minus = match lang.as_str() {
                        "de" => "minus ",
                        "fr" => "moins ",
                        "es" => "menos ",
                        _ => "minus ",
                    };
                    format!("{minus}{out}")
                } else {
                    out
                }
            }
        }
    }

    // ------------------------------------------------------------ parsing

    /// Parses an amount with a currency (`parseCurrency()`): a currency
    /// symbol or ISO code before or after the number. Returns the value,
    /// the ISO code and the end (chars).
    pub fn parse_currency(&self, text: &str, start: usize) -> Option<(f64, String, usize)> {
        let chars: Vec<char> = text.chars().collect();
        let rest: String = chars[start.min(chars.len())..].iter().collect();
        let is_space = |c: char| c.is_whitespace() || c == '\u{a0}' || c == '\u{202f}';
        // a currency text and what it stands for
        let ident = |t: &str| -> Option<String> {
            let t = t.trim_matches(is_space);
            if t.is_empty() {
                return None;
            }
            if t.len() == 3 && t.bytes().all(|b| b.is_ascii_uppercase()) {
                return Some(t.to_string());
            }
            if t == self.symbols[CURRENCY_SYMBOL as usize] {
                return Some(self.currency.clone());
            }
            currency_for_symbol(&self.chain, t)
        };
        // the number: the first digit run
        let first = rest.find(|c: char| c.is_ascii_digit() || c == '-')?;
        let before = &rest[..first];
        let mut plain = self.clone();
        plain.text_affix = [
            Some(String::new()),
            Some(String::new()),
            None,
            Some(String::new()),
        ];
        let tail: String = rest[first..].to_string();
        let (v, _, end) = plain.parse(&tail, 0)?;
        let after: String = tail.chars().skip(end).collect();
        let (code, used) = if let Some(c) = ident(before) {
            (c, 0)
        } else {
            let a = after.trim_matches(is_space).to_string();
            {
                let c = ident(&a)?;
                (c, after.chars().count())
            }
        };
        let total = start + before.chars().count() + end + used;
        Some((v, code, total))
    }

    /// Parses `text` from UTF-16 offset... here: from char index `start`.
    /// Returns the value (and whether it is an integer) with the end
    /// position (chars), or None.
    pub fn parse(&self, text: &str, start: usize) -> Option<(f64, bool, usize)> {
        let chars: Vec<char> = text.chars().collect();
        let mut i = start;
        let minus = &self.symbols[MINUS_SIGN_SYMBOL as usize];
        let plus = &self.symbols[PLUS_SIGN_SYMBOL as usize];
        let dsep: Vec<char> = if self.is_currency_style() {
            self.symbols[MONETARY_SEPARATOR_SYMBOL as usize]
                .chars()
                .collect()
        } else {
            self.symbols[DECIMAL_SEPARATOR_SYMBOL as usize]
                .chars()
                .collect()
        };
        let gsep: Vec<char> = self.symbols[GROUPING_SEPARATOR_SYMBOL as usize]
            .chars()
            .collect();
        let starts = |i: usize, s: &[char]| {
            !s.is_empty() && chars.len() >= i + s.len() && chars[i..i + s.len()] == *s
        };
        let mut neg = false;
        // prefixes
        let pp: Vec<char> = self.text_affix[0]
            .clone()
            .unwrap_or_else(|| self.render_affix(&self.pattern.pos_prefix, false))
            .chars()
            .collect();
        let (np, ns) = self.neg_affixes();
        let np: Vec<char> = np.chars().collect();
        let ns: Vec<char> = ns.chars().collect();
        if np.len() > pp.len() && starts(i, &np) {
            neg = true;
            i += np.len();
        } else if !pp.is_empty() && starts(i, &pp) {
            i += pp.len();
        } else if starts(i, &minus.chars().collect::<Vec<_>>()) {
            neg = true;
            i += minus.chars().count();
        } else if starts(i, &plus.chars().collect::<Vec<_>>()) && self.lenient {
            i += plus.chars().count();
        }
        // currency symbol in the prefix
        let mut num = String::new();
        let mut seen_digit = false;
        let mut seen_point = false;
        let mut int_only = true;
        let begin = i;
        while i < chars.len() {
            let c = chars[i];
            if let Some(dg) = c.to_digit(10) {
                num.push(char::from(b'0' + dg as u8));
                seen_digit = true;
                i += 1;
            } else if !seen_point && !self.parse_int_only && starts(i, &dsep) {
                // a decimal separator needs digits around it
                seen_point = true;
                int_only = false;
                num.push('.');
                i += dsep.len();
            } else if !seen_point && self.pattern.grouping_used && starts(i, &gsep) && seen_digit {
                if i + gsep.len() < chars.len() && chars[i + gsep.len()].is_ascii_digit() {
                    i += gsep.len();
                } else {
                    break;
                }
            } else if seen_digit && (c == 'E' || c == 'e') {
                let mut j = i + 1;
                let mut e = String::from("e");
                if j < chars.len() && (chars[j] == '-' || chars[j] == '+') {
                    e.push(chars[j]);
                    j += 1;
                }
                let ds = j;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    e.push(chars[j]);
                    j += 1;
                }
                if j == ds {
                    break;
                }
                num.push_str(&e);
                int_only = false;
                i = j;
            } else {
                break;
            }
        }
        if !seen_digit {
            let inf: Vec<char> = self.symbols[INFINITY_SYMBOL as usize].chars().collect();
            if starts(begin, &inf) {
                let v = if neg {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                };
                return Some((v, false, begin + inf.len()));
            }
            return None;
        }
        if neg && !ns.is_empty() && starts(i, &ns) {
            i += ns.len();
        } else {
            let ps: Vec<char> = self.text_affix[1]
                .clone()
                .unwrap_or_else(|| self.render_affix(&self.pattern.pos_suffix, false))
                .chars()
                .collect();
            if !ps.is_empty() && starts(i, &ps) {
                i += ps.len();
            }
        }
        let mut v: f64 = num.parse().unwrap_or(0.0);
        let m = self.pattern.multiplier;
        if m != 1 && m != 0 {
            v /= m as f64;
            int_only = false;
        }
        if neg {
            v = -v;
        }
        Some((v, int_only && v.fract() == 0.0, i))
    }
}

/// `"#,##0"` from integer digits and grouping sizes.
fn group_pattern(int: &str, g1: usize, g2: usize) -> String {
    let chars: Vec<char> = int.chars().collect();
    let n = chars.len();
    let mut s = String::new();
    let g2 = if g2 == 0 { g1 } else { g2 };
    for (i, c) in chars.iter().enumerate() {
        s.push(*c);
        let left = n - i - 1;
        if left > 0 && (left == g1 || (left > g1 && (left - g1).is_multiple_of(g2))) {
            s.push(',');
        }
    }
    s
}

/// Minimal user rules (`PATTERN_RULEBASED`): a rule set whose rules are
/// literal text formats as that text (ICU RBNF without substitutions).
fn format_user_rules(rules: &str, x: f64) -> String {
    let body = rules.trim().trim_end_matches(';');
    if !body.contains(['<', '>', '=', ':']) {
        return body.to_string();
    }
    // "x: text;" rule lines: the last rule whose base value is <= x
    let mut out = String::new();
    for r in body.split(';') {
        let r = r.trim();
        if let Some((base, text)) = r.split_once(':') {
            if let Ok(b) = base.trim().parse::<f64>()
                && b <= x
            {
                out = text.trim().to_string();
            }
        } else if out.is_empty() {
            out = r.to_string();
        }
    }
    out
}
