//! ICU's DecimalFormat: the pattern language (PatternParser /
//! PatternStringUtils), the property bag (DecimalFormatProperties) and its
//! mapping to formatting settings (NumberPropertyMapper), number formatting
//! over a decimal quantity (DecimalQuantity) and parsing.
//!
//! Locale data (symbols, patterns, currency names) comes from [`numdata`].

use intl::number::CurrencyDisplay;

use crate::error;
use crate::numdata::{self, *};

// ---------------------------------------------------------------------------
// Rounding modes (UNumberFormatRoundingMode)

pub const ROUND_CEILING: i32 = 0;
pub const ROUND_FLOOR: i32 = 1;
pub const ROUND_DOWN: i32 = 2;
pub const ROUND_UP: i32 = 3;
pub const ROUND_HALFEVEN: i32 = 4;
pub const ROUND_HALFDOWN: i32 = 5;
pub const ROUND_HALFUP: i32 = 6;
pub const ROUND_UNNECESSARY: i32 = 7;
pub const ROUND_HALF_ODD: i32 = 8;
pub const ROUND_HALF_CEILING: i32 = 9;
pub const ROUND_HALF_FLOOR: i32 = 10;

// ---------------------------------------------------------------------------
// DecimalQuantity

/// A decimal number: `d` × 10^`e` (digits most significant first, without
/// leading or trailing zeros; empty for zero).
#[derive(Clone, Debug, Default)]
pub struct Dq {
    pub neg: bool,
    pub d: Vec<u8>,
    pub e: i32,
    pub nan: bool,
    pub inf: bool,
}

impl Dq {
    pub fn from_f64(x: f64) -> Dq {
        let mut q = Dq {
            neg: x.is_sign_negative() && !x.is_nan(),
            ..Default::default()
        };
        if x.is_nan() {
            q.nan = true;
            q.neg = false;
            return q;
        }
        if x.is_infinite() {
            q.inf = true;
            return q;
        }
        if x == 0.0 {
            return q;
        }
        let s = format!("{:e}", x.abs());
        let (m, ex) = s.split_once('e').unwrap_or((&s, "0"));
        let ex: i32 = ex.parse().unwrap_or(0);
        let digits: Vec<u8> = m
            .bytes()
            .filter(|b| b.is_ascii_digit())
            .map(|b| b - b'0')
            .collect();
        q.e = ex - (digits.len() as i32 - 1);
        q.d = digits;
        q.normalize();
        q
    }

    pub fn from_i64(x: i64) -> Dq {
        let mut q = Dq {
            neg: x < 0,
            ..Default::default()
        };
        let s = x.unsigned_abs().to_string();
        q.d = s.bytes().map(|b| b - b'0').collect();
        q.normalize();
        q
    }

    /// From a decimal string ("-12.5", "1e3").
    pub fn parse_decimal(s: &str) -> Option<Dq> {
        let s = s.trim();
        let (neg, s) = match s.strip_prefix('-') {
            Some(r) => (true, r),
            None => (false, s.strip_prefix('+').unwrap_or(s)),
        };
        let (m, ex) = match s.find(['e', 'E']) {
            Some(i) => (&s[..i], s[i + 1..].parse::<i32>().ok()?),
            None => (s, 0),
        };
        let (ip, fp) = match m.find('.') {
            Some(i) => (&m[..i], &m[i + 1..]),
            None => (m, ""),
        };
        if ip.is_empty() && fp.is_empty() {
            return None;
        }
        if !ip.bytes().chain(fp.bytes()).all(|b| b.is_ascii_digit()) {
            return None;
        }
        let mut q = Dq {
            neg,
            ..Default::default()
        };
        q.d = ip.bytes().chain(fp.bytes()).map(|b| b - b'0').collect();
        q.e = ex - fp.len() as i32;
        q.normalize();
        Some(q)
    }

    pub fn normalize(&mut self) {
        let lead = self.d.iter().take_while(|&&x| x == 0).count();
        self.d.drain(..lead);
        while let Some(&0) = self.d.last() {
            self.d.pop();
            self.e += 1;
        }
        if self.d.is_empty() {
            self.e = 0;
        }
    }

    pub fn is_zero(&self) -> bool {
        self.d.is_empty() && !self.nan && !self.inf
    }

    pub fn is_finite(&self) -> bool {
        !self.nan && !self.inf
    }

    /// The magnitude of the most significant digit (0 for zero).
    pub fn upper(&self) -> i32 {
        if self.d.is_empty() {
            0
        } else {
            self.e + self.d.len() as i32 - 1
        }
    }

    /// The magnitude of the least significant digit (0 for zero).
    pub fn lower(&self) -> i32 {
        if self.d.is_empty() { 0 } else { self.e }
    }

    pub fn digit(&self, mag: i32) -> u8 {
        if self.d.is_empty() || mag < self.e || mag > self.upper() {
            return 0;
        }
        self.d[(self.upper() - mag) as usize]
    }

    pub fn shift(&mut self, k: i32) {
        if !self.d.is_empty() {
            self.e += k;
        }
    }

    /// Multiply by an integer.
    pub fn mul_int(&mut self, m: i64) {
        if m < 0 {
            self.neg = !self.neg;
        }
        let m = m.unsigned_abs();
        if self.d.is_empty() || m == 1 {
            return;
        }
        if m == 0 {
            self.d.clear();
            self.e = 0;
            return;
        }
        let mut out: Vec<u8> = Vec::new();
        let mut carry: u128 = 0;
        for &x in self.d.iter().rev() {
            let v = x as u128 * m as u128 + carry;
            out.push((v % 10) as u8);
            carry = v / 10;
        }
        while carry > 0 {
            out.push((carry % 10) as u8);
            carry /= 10;
        }
        out.reverse();
        self.d = out;
        self.normalize();
    }

    /// Drop the digits of magnitude >= `n` (IntegerWidth truncation).
    pub fn truncate_int(&mut self, n: i32) {
        if self.d.is_empty() || self.upper() < n {
            return;
        }
        let keep_from = (self.upper() - n + 1).max(0) as usize;
        if keep_from >= self.d.len() {
            self.d.clear();
            self.e = 0;
            return;
        }
        self.d.drain(..keep_from);
        self.normalize();
    }

    /// Round to magnitude `mag` with `mode`. Err(()) for ROUND_UNNECESSARY
    /// when inexact.
    pub fn round_at(&mut self, mag: i32, mode: i32) -> Result<(), ()> {
        if self.d.is_empty() || !self.is_finite() || self.e >= mag {
            return Ok(());
        }
        let upper = self.upper();
        // kept digits: magnitudes upper..=mag
        let mut kept: Vec<u8> = Vec::new();
        let mut m = upper;
        while m >= mag {
            kept.push(self.digit(m));
            m -= 1;
        }
        let first = self.digit(mag - 1);
        let rest_nonzero = self.e < mag - 1;
        let inexact = first != 0 || rest_nonzero;
        if !inexact {
            return Ok(());
        }
        let cmp_half = if first > 5 || (first == 5 && rest_nonzero) {
            1
        } else if first == 5 {
            0
        } else {
            -1
        };
        let last_odd = kept.last().is_some_and(|&x| x % 2 == 1);
        let up = match mode {
            ROUND_CEILING => !self.neg,
            ROUND_FLOOR => self.neg,
            ROUND_DOWN => false,
            ROUND_UP => true,
            ROUND_HALFDOWN => cmp_half > 0,
            ROUND_HALFUP => cmp_half >= 0,
            ROUND_UNNECESSARY => return Err(()),
            ROUND_HALF_ODD => cmp_half > 0 || (cmp_half == 0 && !last_odd),
            ROUND_HALF_CEILING => cmp_half > 0 || (cmp_half == 0 && !self.neg),
            ROUND_HALF_FLOOR => cmp_half > 0 || (cmp_half == 0 && self.neg),
            _ => cmp_half > 0 || (cmp_half == 0 && last_odd),
        };
        if up {
            let mut i = kept.len();
            loop {
                if i == 0 {
                    kept.insert(0, 1);
                    break;
                }
                i -= 1;
                if kept[i] == 9 {
                    kept[i] = 0;
                } else {
                    kept[i] += 1;
                    break;
                }
            }
        }
        self.d = kept;
        self.e = mag;
        self.normalize();
        Ok(())
    }

    /// Round to a multiple of `inc` (> 0).
    pub fn round_increment(&mut self, inc: &Dq, mode: i32) -> Result<(), ()> {
        if self.d.is_empty() || !self.is_finite() || inc.d.is_empty() {
            return Ok(());
        }
        let k = inc.e;
        let m: u64 = inc
            .d
            .iter()
            .fold(0u64, |a, &x| a.saturating_mul(10).saturating_add(x as u64));
        if m == 1 {
            return self.round_at(k, mode);
        }
        // value / 10^k = I + F
        let mut v = self.clone();
        v.neg = false;
        v.shift(-k);
        let mut int_digits: Vec<u8> = Vec::new();
        let mut mg = v.upper();
        while mg >= 0 {
            int_digits.push(v.digit(mg));
            mg -= 1;
        }
        let frac_zero = v.e >= 0;
        let frac_half = v.e == -1 && v.digit(-1) == 5;
        let frac_f: f64 = {
            let mut f = 0.0;
            let mut p = 0.1;
            let mut mg = -1;
            while mg >= v.e && mg > -20 {
                f += v.digit(mg) as f64 * p;
                p /= 10.0;
                mg -= 1;
            }
            f
        };
        // I mod m, I div m
        let mut rem: u128 = 0;
        let mut quot: Vec<u8> = Vec::new();
        for &dg in &int_digits {
            rem = rem * 10 + dg as u128;
            quot.push((rem / m as u128) as u8);
            rem %= m as u128;
        }
        let r = rem as u64;
        let exact = r == 0 && frac_zero;
        if exact {
            return Ok(());
        }
        let twice = 2 * r as u128;
        let tie = (frac_zero && twice == m as u128) || (frac_half && twice + 1 == m as u128);
        let above = !tie && (2.0 * r as f64 + 2.0 * frac_f) > m as f64;
        let q_odd = quot.last().is_some_and(|&x| x % 2 == 1);
        let up = match mode {
            ROUND_CEILING => !self.neg,
            ROUND_FLOOR => self.neg,
            ROUND_DOWN => false,
            ROUND_UP => true,
            ROUND_HALFDOWN => above,
            ROUND_HALFUP => above || tie,
            ROUND_UNNECESSARY => return Err(()),
            ROUND_HALF_ODD => above || (tie && !q_odd),
            ROUND_HALF_CEILING => above || (tie && !self.neg),
            ROUND_HALF_FLOOR => above || (tie && self.neg),
            _ => above || (tie && q_odd),
        };
        // result = (quot + up) * m * 10^k
        let mut qd = Dq {
            neg: self.neg,
            d: quot,
            e: 0,
            ..Default::default()
        };
        qd.normalize();
        if up {
            let one = Dq::from_i64(1);
            qd = add_abs(&qd, &one);
            qd.neg = self.neg;
        }
        qd.mul_int(m as i64);
        qd.shift(k);
        qd.neg = self.neg;
        *self = qd;
        Ok(())
    }

    pub fn to_f64(&self) -> f64 {
        if self.nan {
            return f64::NAN;
        }
        if self.inf {
            return if self.neg {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            };
        }
        if self.d.is_empty() {
            return if self.neg { -0.0 } else { 0.0 };
        }
        let s: String = self.d.iter().map(|&x| (b'0' + x) as char).collect();
        let v: f64 = format!("{}e{}", s, self.e).parse().unwrap_or(0.0);
        if self.neg { -v } else { v }
    }

    /// The value as an i64 when integral and in range.
    pub fn to_i64(&self) -> Option<i64> {
        if !self.is_finite() || self.e < 0 {
            return None;
        }
        let mut v: i128 = 0;
        for &x in &self.d {
            v = v * 10 + x as i128;
            if v > i64::MAX as i128 + 1 {
                return None;
            }
        }
        for _ in 0..self.e {
            v *= 10;
            if v > i64::MAX as i128 + 1 {
                return None;
            }
        }
        let v = if self.neg { -v } else { v };
        i64::try_from(v).ok()
    }

    /// Plain decimal string (for plural operands): "-12.50" style with
    /// at least `min_frac` fraction digits.
    pub fn plain(&self, min_frac: i32) -> String {
        let mut s = String::new();
        if self.neg && !self.d.is_empty() {
            s.push('-');
        }
        let up = self.upper().max(0);
        let low = self.lower().min(0).min(-min_frac);
        let mut m = up;
        while m >= 0 {
            s.push((b'0' + self.digit(m)) as char);
            m -= 1;
        }
        if low < 0 {
            s.push('.');
            let mut m = -1;
            while m >= low {
                s.push((b'0' + self.digit(m)) as char);
                m -= 1;
            }
        }
        s
    }
}

fn add_abs(a: &Dq, b: &Dq) -> Dq {
    if a.d.is_empty() {
        return Dq {
            neg: false,
            ..b.clone()
        };
    }
    if b.d.is_empty() {
        return Dq {
            neg: false,
            ..a.clone()
        };
    }
    let lo = a.e.min(b.e);
    let hi = a.upper().max(b.upper());
    let mut out: Vec<u8> = Vec::new();
    let mut carry = 0;
    let mut m = lo;
    while m <= hi {
        let v = a.digit(m) + b.digit(m) + carry;
        out.push(v % 10);
        carry = v / 10;
        m += 1;
    }
    if carry > 0 {
        out.push(carry);
    }
    out.reverse();
    let mut r = Dq {
        d: out,
        e: lo,
        ..Default::default()
    };
    r.normalize();
    r
}

// ---------------------------------------------------------------------------
// Properties

/// DecimalFormatProperties (the subset DecimalFormat uses). -1 = unset.
#[derive(Clone, Debug)]
pub struct Props {
    pub pos_prefix: Option<String>,
    pub pos_suffix: Option<String>,
    pub neg_prefix: Option<String>,
    pub neg_suffix: Option<String>,
    pub pos_prefix_pat: Option<String>,
    pub pos_suffix_pat: Option<String>,
    pub neg_prefix_pat: Option<String>,
    pub neg_suffix_pat: Option<String>,
    pub min_int: i32,
    pub max_int: i32,
    pub min_frac: i32,
    pub max_frac: i32,
    pub min_sig: i32,
    pub max_sig: i32,
    pub grouping_size: i32,
    pub secondary_grouping: i32,
    pub grouping_used: bool,
    pub rounding_increment: f64,
    pub rounding_mode: Option<i32>,
    pub decimal_always: bool,
    pub currency_as_decimal: bool,
    pub min_exp_digits: i32,
    pub exp_sign_always: bool,
    pub format_width: i32,
    pub pad_string: Option<String>,
    pub pad_position: Option<i32>,
    pub magnitude_multiplier: i32,
    pub multiplier: i64,
    pub parse_int_only: bool,
    pub lenient: bool,
    /// setLenient(false): PARSE_MODE_STRICT
    pub parse_strict: bool,
    pub currency: Option<String>,
    pub cash: bool,
}

impl Default for Props {
    fn default() -> Props {
        Props {
            pos_prefix: None,
            pos_suffix: None,
            neg_prefix: None,
            neg_suffix: None,
            pos_prefix_pat: None,
            pos_suffix_pat: None,
            neg_prefix_pat: None,
            neg_suffix_pat: None,
            min_int: -1,
            max_int: -1,
            min_frac: -1,
            max_frac: -1,
            min_sig: -1,
            max_sig: -1,
            grouping_size: -1,
            secondary_grouping: -1,
            grouping_used: true,
            rounding_increment: 0.0,
            rounding_mode: None,
            decimal_always: false,
            currency_as_decimal: false,
            min_exp_digits: -1,
            exp_sign_always: false,
            format_width: -1,
            pad_string: None,
            pad_position: None,
            magnitude_multiplier: 0,
            multiplier: 1,
            parse_int_only: false,
            lenient: false,
            parse_strict: false,
            currency: None,
            cash: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Pattern parsing

#[derive(Default, Clone, Debug)]
struct Sub {
    grouping_sizes: u64,
    integer_leading_hash: i32,
    integer_trailing_hash: i32,
    integer_numerals: i32,
    integer_at_signs: i32,
    integer_total: i32,
    fraction_numerals: i32,
    fraction_hash: i32,
    fraction_total: i32,
    has_decimal: bool,
    width_except_affixes: i32,
    padding_location: Option<i32>,
    exponent_has_plus: bool,
    exponent_zeros: i32,
    has_percent: bool,
    has_permille: bool,
    has_currency: bool,
    has_currency_decimal: bool,
    rounding: Option<Dq>,
    prefix: (usize, usize),
    suffix: (usize, usize),
    padding: (usize, usize),
}

struct Parser<'a> {
    p: &'a [char],
    i: usize,
    err: i32,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.p.get(self.i).copied()
    }
    fn peek2(&self) -> Option<char> {
        self.p.get(self.i + 1).copied()
    }

    fn literal(&mut self) {
        match self.peek() {
            None => self.err = error::U_PATTERN_SYNTAX_ERROR,
            Some('\'') => {
                self.i += 1;
                loop {
                    match self.peek() {
                        None => {
                            self.err = error::U_PATTERN_SYNTAX_ERROR;
                            return;
                        }
                        Some('\'') => break,
                        _ => self.i += 1,
                    }
                }
                self.i += 1;
            }
            _ => self.i += 1,
        }
    }

    fn affix(&mut self, s: &mut Sub) -> (usize, usize) {
        let start = self.i;
        loop {
            match self.peek() {
                None | Some('#' | '@' | ';' | '*' | '.' | ',' | '0'..='9') => break,
                Some('%') => s.has_percent = true,
                Some('‰') => s.has_permille = true,
                Some('¤') => s.has_currency = true,
                _ => {}
            }
            self.literal();
            if self.err != 0 {
                break;
            }
        }
        (start, self.i)
    }

    fn padding(&mut self, s: &mut Sub, loc: i32) {
        if self.peek() != Some('*') || self.err != 0 {
            return;
        }
        if s.padding_location.is_some() {
            self.err = 65798; // U_MULTIPLE_PAD_SPECIFIERS
            return;
        }
        s.padding_location = Some(loc);
        self.i += 1;
        let st = self.i;
        self.literal();
        s.padding = (st, self.i);
    }

    fn integer(&mut self, s: &mut Sub) {
        loop {
            match self.peek() {
                Some(',') => {
                    s.width_except_affixes += 1;
                    s.grouping_sizes <<= 16;
                }
                Some('#') => {
                    if s.integer_numerals > 0 {
                        self.err = error::U_UNEXPECTED_TOKEN;
                        return;
                    }
                    s.width_except_affixes += 1;
                    s.grouping_sizes += 1;
                    if s.integer_at_signs > 0 {
                        s.integer_trailing_hash += 1;
                    } else {
                        s.integer_leading_hash += 1;
                    }
                    s.integer_total += 1;
                }
                Some('@') => {
                    if s.integer_numerals > 0 || s.integer_trailing_hash > 0 {
                        self.err = error::U_UNEXPECTED_TOKEN;
                        return;
                    }
                    s.width_except_affixes += 1;
                    s.grouping_sizes += 1;
                    s.integer_at_signs += 1;
                    s.integer_total += 1;
                }
                Some(c @ '0'..='9') => {
                    if s.integer_at_signs > 0 {
                        self.err = error::U_UNEXPECTED_TOKEN;
                        return;
                    }
                    s.width_except_affixes += 1;
                    s.grouping_sizes += 1;
                    s.integer_numerals += 1;
                    s.integer_total += 1;
                    let zeroish = s.rounding.as_ref().is_none_or(|r| r.d.is_empty());
                    if !zeroish || c != '0' {
                        let r = s.rounding.get_or_insert_with(Dq::default);
                        // append an integer digit
                        let mut v = r.clone();
                        v.shift(1);
                        let add = Dq::from_i64((c as u8 - b'0') as i64);
                        *r = add_abs(&v, &add);
                    }
                }
                _ => break,
            }
            self.i += 1;
        }
        let g1 = (s.grouping_sizes & 0xffff) as u16 as i16;
        let g2 = ((s.grouping_sizes >> 16) & 0xffff) as u16 as i16;
        let g3 = ((s.grouping_sizes >> 32) & 0xffff) as u16 as i16;
        if g1 == 0 && g2 != -1 {
            self.err = error::U_UNEXPECTED_TOKEN;
            return;
        }
        if g2 == 0 && g3 != -1 {
            self.err = error::U_PATTERN_SYNTAX_ERROR;
        }
    }

    fn fraction(&mut self, s: &mut Sub) {
        let mut zero_counter = 0;
        loop {
            match self.peek() {
                Some('#') => {
                    s.width_except_affixes += 1;
                    s.fraction_hash += 1;
                    s.fraction_total += 1;
                    zero_counter += 1;
                }
                Some(c @ '0'..='9') => {
                    if s.fraction_hash > 0 {
                        self.err = error::U_UNEXPECTED_TOKEN;
                        return;
                    }
                    s.width_except_affixes += 1;
                    s.fraction_numerals += 1;
                    s.fraction_total += 1;
                    if c == '0' {
                        zero_counter += 1;
                    } else {
                        let r = s.rounding.get_or_insert_with(Dq::default);
                        let mut add = Dq::from_i64((c as u8 - b'0') as i64);
                        add.shift(-(s.fraction_total));
                        *r = add_abs(r, &add);
                        zero_counter = 0;
                    }
                }
                _ => break,
            }
            self.i += 1;
        }
        let _ = zero_counter;
    }

    fn format(&mut self, s: &mut Sub) {
        self.integer(s);
        if self.err != 0 {
            return;
        }
        if self.peek() == Some('.') {
            self.i += 1;
            s.has_decimal = true;
            s.width_except_affixes += 1;
            self.fraction(s);
        } else if self.peek() == Some('¤') {
            match self.peek2() {
                Some('#' | '0'..='9') => {}
                _ => return,
            }
            s.has_currency = true;
            s.has_currency_decimal = true;
            s.has_decimal = true;
            s.width_except_affixes += 1;
            self.i += 1;
            self.fraction(s);
        }
    }

    fn exponent(&mut self, s: &mut Sub) {
        if self.peek() != Some('E') || self.err != 0 {
            return;
        }
        if (s.grouping_sizes & 0xffff0000) != 0xffff0000 {
            self.err = 65795; // U_MALFORMED_EXPONENTIAL_PATTERN
            return;
        }
        self.i += 1;
        s.width_except_affixes += 1;
        if self.peek() == Some('+') {
            self.i += 1;
            s.exponent_has_plus = true;
            s.width_except_affixes += 1;
        }
        while self.peek() == Some('0') {
            self.i += 1;
            s.exponent_zeros += 1;
            s.width_except_affixes += 1;
        }
    }

    fn subpattern(&mut self) -> Sub {
        let mut s = Sub {
            grouping_sizes: 0x0000_ffff_ffff_0000,
            ..Default::default()
        };
        self.padding(&mut s, 0);
        if self.err != 0 {
            return s;
        }
        s.prefix = self.affix(&mut s);
        if self.err != 0 {
            return s;
        }
        self.padding(&mut s, 1);
        if self.err != 0 {
            return s;
        }
        self.format(&mut s);
        if self.err != 0 {
            return s;
        }
        self.exponent(&mut s);
        if self.err != 0 {
            return s;
        }
        self.padding(&mut s, 2);
        if self.err != 0 {
            return s;
        }
        s.suffix = self.affix(&mut s);
        if self.err != 0 {
            return s;
        }
        self.padding(&mut s, 3);
        s
    }
}

/// AffixUtils::estimateLength()
fn affix_estimate_len(p: &str) -> i32 {
    let mut n = 0;
    let mut inq = false;
    let cs: Vec<char> = p.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c == '\'' {
            if i + 1 < cs.len() && cs[i + 1] == '\'' {
                n += 1;
                i += 2;
                continue;
            }
            inq = !inq;
            i += 1;
            continue;
        }
        n += 1;
        i += 1;
    }
    let _ = inq;
    n
}

/// Parse `pattern` into `props` (PatternParser::parseToExistingProperties).
/// `ignore_rounding`: 0 never, 1 if currency, 2 always.
pub fn apply_pattern(props: &mut Props, pattern: &str, ignore_rounding: i32) -> Result<(), i32> {
    if pattern.is_empty() {
        // backwards compatibility: reset to the default values
        *props = Props::default();
        return Ok(());
    }
    let chars: Vec<char> = pattern.chars().collect();
    let mut ps = Parser {
        p: &chars,
        i: 0,
        err: 0,
    };
    let pos = ps.subpattern();
    if ps.err != 0 {
        return Err(ps.err);
    }
    let mut neg: Option<Sub> = None;
    if ps.peek() == Some(';') {
        ps.i += 1;
        if ps.peek().is_some() {
            let n = ps.subpattern();
            if ps.err != 0 {
                return Err(ps.err);
            }
            neg = Some(n);
        }
    }
    if ps.peek().is_some() {
        return Err(65555); // U_UNQUOTED_SPECIAL
    }
    let sub = |r: (usize, usize)| -> String { chars[r.0..r.1].iter().collect() };
    let ignore = match ignore_rounding {
        0 => false,
        1 => pos.has_currency,
        _ => true,
    };
    let g1 = (pos.grouping_sizes & 0xffff) as u16 as i16;
    let g2 = ((pos.grouping_sizes >> 16) & 0xffff) as u16 as i16;
    let g3 = ((pos.grouping_sizes >> 32) & 0xffff) as u16 as i16;
    if g2 != -1 {
        props.grouping_size = g1 as i32;
        props.grouping_used = true;
    } else {
        props.grouping_size = -1;
        props.grouping_used = false;
    }
    props.secondary_grouping = if g3 != -1 { g2 as i32 } else { -1 };
    let (min_int, min_frac) = if pos.integer_total == 0 && pos.fraction_total > 0 {
        (0, pos.fraction_numerals.max(1))
    } else if pos.integer_numerals == 0 && pos.fraction_numerals == 0 {
        (1, 0)
    } else {
        (pos.integer_numerals, pos.fraction_numerals)
    };
    let rounding_zero = pos.rounding.as_ref().is_none_or(|r| r.d.is_empty());
    if pos.integer_at_signs > 0 {
        props.min_frac = -1;
        props.max_frac = -1;
        props.rounding_increment = 0.0;
        props.min_sig = pos.integer_at_signs;
        props.max_sig = pos.integer_at_signs + pos.integer_trailing_hash;
    } else if !rounding_zero {
        if !ignore {
            props.min_frac = min_frac;
            props.max_frac = pos.fraction_total;
            props.rounding_increment = pos.rounding.as_ref().map(|r| r.to_f64()).unwrap_or(0.0);
        } else {
            props.min_frac = -1;
            props.max_frac = -1;
            props.rounding_increment = 0.0;
        }
        props.min_sig = -1;
        props.max_sig = -1;
    } else {
        if !ignore {
            props.min_frac = min_frac;
            props.max_frac = pos.fraction_total;
        } else {
            props.min_frac = -1;
            props.max_frac = -1;
        }
        props.rounding_increment = 0.0;
        props.min_sig = -1;
        props.max_sig = -1;
    }
    props.decimal_always = pos.has_decimal && pos.fraction_total == 0;
    props.currency_as_decimal = pos.has_currency_decimal;
    if pos.exponent_zeros > 0 {
        props.exp_sign_always = pos.exponent_has_plus;
        props.min_exp_digits = pos.exponent_zeros;
        if pos.integer_at_signs == 0 {
            props.min_int = pos.integer_numerals;
            props.max_int = pos.integer_total;
        } else {
            props.min_int = 1;
            props.max_int = -1;
        }
    } else {
        props.exp_sign_always = false;
        props.min_exp_digits = -1;
        props.min_int = min_int;
        props.max_int = -1;
    }
    let pp = sub(pos.prefix);
    let psx = sub(pos.suffix);
    if let Some(loc) = pos.padding_location {
        let w = pos.width_except_affixes + affix_estimate_len(&pp) + affix_estimate_len(&psx);
        props.format_width = w;
        let raw: Vec<char> = chars[pos.padding.0..pos.padding.1].to_vec();
        let ps: String = if raw.len() == 1 {
            raw.iter().collect()
        } else if raw.len() == 2 {
            if raw[0] == '\'' {
                "'".to_string()
            } else {
                raw.iter().collect()
            }
        } else {
            raw[1..raw.len() - 1].iter().collect()
        };
        props.pad_string = Some(ps);
        props.pad_position = Some(loc);
    } else {
        props.format_width = -1;
        props.pad_string = None;
        props.pad_position = None;
    }
    props.pos_prefix_pat = Some(pp);
    props.pos_suffix_pat = Some(psx);
    if let Some(n) = &neg {
        props.neg_prefix_pat = Some(sub(n.prefix));
        props.neg_suffix_pat = Some(sub(n.suffix));
    } else {
        props.neg_prefix_pat = None;
        props.neg_suffix_pat = None;
    }
    props.magnitude_multiplier = if pos.has_percent {
        2
    } else if pos.has_permille {
        3
    } else {
        0
    };
    Ok(())
}

// ---------------------------------------------------------------------------
// Affixes

/// AffixUtils::escape()
pub fn affix_escape(s: &str) -> String {
    let mut out = String::new();
    let mut inq = false;
    for c in s.chars() {
        match c {
            '\'' => out.push_str("''"),
            '-' | '+' | '%' | '‰' | '¤' => {
                if !inq {
                    out.push('\'');
                    inq = true;
                }
                out.push(c);
            }
            _ => {
                if inq {
                    out.push('\'');
                    inq = false;
                }
                out.push(c);
            }
        }
    }
    if inq {
        out.push('\'');
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Lit(char),
    Minus,
    Plus,
    Percent,
    Permille,
    Currency(usize),
}

pub fn affix_tokens(p: &str) -> Vec<Tok> {
    let cs: Vec<char> = p.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut inq = false;
    while i < cs.len() {
        let c = cs[i];
        if c == '\'' {
            if i + 1 < cs.len() && cs[i + 1] == '\'' {
                out.push(Tok::Lit('\''));
                i += 2;
                continue;
            }
            inq = !inq;
            i += 1;
            continue;
        }
        if inq {
            out.push(Tok::Lit(c));
            i += 1;
            continue;
        }
        match c {
            '-' => out.push(Tok::Minus),
            '+' => out.push(Tok::Plus),
            '%' => out.push(Tok::Percent),
            '‰' => out.push(Tok::Permille),
            '¤' => {
                let mut n = 0;
                while i < cs.len() && cs[i] == '¤' {
                    n += 1;
                    i += 1;
                }
                out.push(Tok::Currency(n));
                continue;
            }
            _ => out.push(Tok::Lit(c)),
        }
        i += 1;
    }
    out
}

pub fn has_currency(p: &Option<String>) -> bool {
    p.as_ref().is_some_and(|s| {
        affix_tokens(s)
            .iter()
            .any(|t| matches!(t, Tok::Currency(_)))
    })
}

/// The four resolved affix patterns (PropertiesAffixPatternProvider).
#[derive(Clone, Debug)]
pub struct Affixes {
    pub pos_prefix: String,
    pub pos_suffix: String,
    pub neg_prefix: String,
    pub neg_suffix: String,
    pub is_currency: bool,
}

impl Affixes {
    pub fn from_props(p: &Props) -> Affixes {
        let pos_prefix = match (&p.pos_prefix, &p.pos_prefix_pat) {
            (Some(o), _) => affix_escape(o),
            (None, Some(pp)) => pp.clone(),
            _ => String::new(),
        };
        let pos_suffix = match (&p.pos_suffix, &p.pos_suffix_pat) {
            (Some(o), _) => affix_escape(o),
            (None, Some(pp)) => pp.clone(),
            _ => String::new(),
        };
        let neg_prefix = match (&p.neg_prefix, &p.neg_prefix_pat) {
            (Some(o), _) => affix_escape(o),
            (None, Some(pp)) => pp.clone(),
            _ => match &p.pos_prefix_pat {
                Some(pp) => format!("-{}", pp),
                None => "-".to_string(),
            },
        };
        let neg_suffix = match (&p.neg_suffix, &p.neg_suffix_pat) {
            (Some(o), _) => affix_escape(o),
            (None, Some(pp)) => pp.clone(),
            _ => p.pos_suffix_pat.clone().unwrap_or_default(),
        };
        let is_currency = has_currency(&p.pos_prefix_pat)
            || has_currency(&p.pos_suffix_pat)
            || has_currency(&p.neg_prefix_pat)
            || has_currency(&p.neg_suffix_pat)
            || p.currency_as_decimal;
        Affixes {
            pos_prefix,
            pos_suffix,
            neg_prefix,
            neg_suffix,
            is_currency,
        }
    }

    pub fn has_negative_subpattern(&self) -> bool {
        let np: Vec<char> = self.neg_prefix.chars().collect();
        let rest: String = np.iter().skip(1).collect();
        self.neg_suffix != self.pos_suffix || rest != self.pos_prefix || np.first() != Some(&'-')
    }
}

// ---------------------------------------------------------------------------
// The mapped settings

#[derive(Clone, Debug)]
pub enum Prec {
    Unlimited,
    Fraction(i32, i32),
    Sig(i32, i32),
    Increment(Dq, i32),
}

#[derive(Clone, Debug)]
pub struct Macros {
    pub prec: Prec,
    pub mode: i32,
    pub min_int: i32,
    pub max_int: i32,
    pub g1: i32,
    pub g2: i32,
    pub grouping: bool,
    pub decimal_always: bool,
    /// (engineering interval, require min int, min exponent digits, sign always)
    pub sci: Option<(i32, bool, i32, bool)>,
    pub mag_mult: i32,
    pub mult: i64,
    pub width: i32,
    pub pad: char,
    pub pad_pos: i32,
    pub affixes: Affixes,
    pub currency: Option<String>,
    /// exported properties
    pub ex_min_int: i32,
    pub ex_max_int: i32,
    pub ex_min_frac: i32,
    pub ex_max_frac: i32,
    pub ex_min_sig: i32,
    pub ex_max_sig: i32,
    pub ex_increment: f64,
}

const K_MAX: i32 = 999;

/// PatternStringUtils::ignoreRoundingIncrement()
fn ignore_rounding_increment(inc: f64, max_frac: i32) -> bool {
    if max_frac < 0 {
        return false;
    }
    let mut r = inc * 2.0;
    let mut frac = 0;
    while frac <= max_frac && r <= 1.0 {
        frac += 1;
        r *= 10.0;
    }
    frac > max_frac
}

/// NumberPropertyMapper::oldToNew()
pub fn map_props(p: &Props, locale_currency: Option<&str>) -> Macros {
    let affixes = Affixes::from_props(p);
    let use_currency = affixes.is_currency;
    let currency: Option<String> = p
        .currency
        .clone()
        .or_else(|| locale_currency.map(|s| s.to_string()));
    let mut max_int = p.max_int;
    let mut min_int = p.min_int;
    let mut max_frac = p.max_frac;
    let mut min_frac = p.min_frac;
    let mut min_sig = p.min_sig;
    let mut max_sig = p.max_sig;
    let inc = p.rounding_increment;
    let mode = p.rounding_mode.unwrap_or(ROUND_HALFEVEN);
    let explicit_frac = min_frac != -1 || max_frac != -1;
    let explicit_sig = min_sig != -1 || max_sig != -1;
    if use_currency && (min_frac == -1 || max_frac == -1) {
        let digits = numdata::currency_digits(currency.as_deref().unwrap_or("XXX"));
        if min_frac == -1 && max_frac == -1 {
            min_frac = digits;
            max_frac = digits;
        } else if min_frac == -1 {
            min_frac = max_frac.min(digits);
        } else {
            max_frac = min_frac.max(digits);
        }
    }
    if min_int == 0 && max_frac != 0 {
        min_frac = if min_frac < 0 || (min_frac == 0 && max_int == 0) {
            1
        } else {
            min_frac
        };
        max_frac = if max_frac < 0 {
            -1
        } else if max_frac < min_frac {
            min_frac
        } else {
            max_frac
        };
        min_int = 0;
        max_int = if !(0..=K_MAX).contains(&max_int) {
            -1
        } else {
            max_int
        };
    } else {
        min_frac = min_frac.max(0);
        max_frac = if max_frac < 0 {
            -1
        } else if max_frac < min_frac {
            min_frac
        } else {
            max_frac
        };
        min_int = if min_int <= 0 || min_int > K_MAX {
            1
        } else {
            min_int
        };
        max_int = if !(0..=K_MAX).contains(&max_int) {
            -1
        } else if max_int < min_int {
            min_int
        } else {
            max_int
        };
    }
    let mut prec: Option<Prec> = None;
    if inc != 0.0 {
        if ignore_rounding_increment(inc, max_frac) {
            prec = Some(Prec::Fraction(min_frac, max_frac));
        } else {
            if min_frac > max_frac {
                min_frac = max_frac;
            }
            prec = Some(Prec::Increment(Dq::from_f64(inc), min_frac));
        }
    } else if explicit_sig {
        min_sig = min_sig.clamp(1, K_MAX);
        max_sig = if max_sig < 0 {
            K_MAX
        } else if max_sig < min_sig {
            min_sig
        } else if max_sig > K_MAX {
            K_MAX
        } else {
            max_sig
        };
        prec = Some(Prec::Sig(min_sig, max_sig));
    } else if explicit_frac || use_currency {
        prec = Some(Prec::Fraction(min_frac, max_frac));
    }
    let mut sci = None;
    let mut iw_min = min_int;
    let mut iw_max = max_int;
    if p.min_exp_digits != -1 {
        if max_int > 8 {
            max_int = min_int;
            iw_min = min_int;
            iw_max = max_int;
        } else if max_int > min_int && min_int > 1 {
            min_int = 1;
            iw_min = 1;
            iw_max = max_int;
        }
        let engineering = if max_int < 0 { -1 } else { max_int };
        sci = Some((
            engineering,
            engineering == min_int,
            p.min_exp_digits,
            p.exp_sign_always,
        ));
        if let Some(Prec::Fraction(..)) = prec {
            let max_int_ = p.max_int;
            let mut min_int_ = p.min_int;
            let min_frac_ = p.min_frac;
            let max_frac_ = p.max_frac;
            if min_int_ == 0 && max_frac_ == 0 {
                prec = Some(Prec::Unlimited);
            } else if min_int_ == 0 && min_frac_ == 0 {
                prec = Some(Prec::Sig(1, max_frac_ + 1));
            } else {
                let max_sig_ = min_int_ + max_frac_;
                if max_int_ > min_int_ && min_int_ > 1 {
                    min_int_ = 1;
                }
                let min_sig_ = min_int_ + min_frac_;
                prec = Some(Prec::Sig(min_sig_, max_sig_));
            }
        }
    }
    // exported
    let (mut ex_min_frac, mut ex_max_frac, mut ex_min_sig, mut ex_max_sig) =
        (min_frac, max_frac, min_sig, max_sig);
    let mut ex_inc = 0.0;
    match &prec {
        Some(Prec::Fraction(a, b)) => {
            ex_min_frac = *a;
            ex_max_frac = *b;
        }
        Some(Prec::Increment(q, mf)) => {
            ex_inc = q.to_f64();
            ex_min_frac = *mf;
            ex_max_frac = *mf;
        }
        Some(Prec::Sig(a, b)) => {
            ex_min_sig = *a;
            ex_max_sig = *b;
        }
        _ => {}
    }
    let prec = prec.unwrap_or(Prec::Fraction(0, 6));
    // grouping
    let (g1, g2) = if !p.grouping_used {
        (0, 0)
    } else {
        let mut g1 = p.grouping_size;
        let g2s = p.secondary_grouping;
        g1 = if g1 > 0 {
            g1
        } else if g2s > 0 {
            g2s
        } else {
            g1
        };
        let g2 = if g2s > 0 { g2s } else { g1 };
        (g1, g2)
    };
    let pad = p
        .pad_string
        .as_ref()
        .and_then(|s| s.chars().next())
        .unwrap_or(' ');
    Macros {
        prec,
        mode,
        min_int: iw_min,
        max_int: iw_max,
        g1,
        g2,
        grouping: p.grouping_used && g1 > 0,
        decimal_always: p.decimal_always,
        sci,
        mag_mult: p.magnitude_multiplier,
        mult: p.multiplier,
        width: p.format_width,
        pad,
        pad_pos: p.pad_position.unwrap_or(0),
        affixes,
        currency,
        ex_min_int: min_int,
        ex_max_int: max_int,
        ex_min_frac,
        ex_max_frac,
        ex_min_sig,
        ex_max_sig,
        ex_increment: ex_inc,
    }
}

// ---------------------------------------------------------------------------
// The formatter

#[derive(Clone, Debug)]
pub struct DecimalFormat {
    pub props: Props,
    pub symbols: Vec<String>,
    pub custom_cur_sym: bool,
    pub custom_intl_sym: bool,
    /// intl tag of the locale.
    pub tag: String,
    /// The currency of the locale (ucurr_forLocale).
    pub locale_currency: Option<String>,
    /// NumberFormat::getCurrency() (the CURRENCY_CODE text attribute).
    pub currency_attr: Option<String>,
}

/// One piece of formatted output with its field (for currency spacing).
#[derive(Clone, Copy, PartialEq)]
enum Field {
    Other,
    Currency,
}

impl DecimalFormat {
    pub fn new(data: &LocaleData, locale_currency: Option<String>) -> DecimalFormat {
        let mut symbols = data.symbols.clone();
        let mut code = locale_currency.clone();
        if let Some(c) = &locale_currency {
            symbols[SYM_CURRENCY] = numdata::currency_symbol(&data.tag, c, CurrencyDisplay::Symbol);
            symbols[SYM_INTL_CURRENCY] = c.clone();
        } else {
            code = None;
        }
        let _ = code;
        DecimalFormat {
            props: Props::default(),
            symbols,
            custom_cur_sym: false,
            custom_intl_sym: false,
            tag: data.tag.clone(),
            locale_currency,
            currency_attr: None,
        }
    }

    pub fn macros(&self) -> Macros {
        map_props(&self.props, self.locale_currency.as_deref())
    }

    fn currency_code(&self, m: &Macros) -> String {
        m.currency.clone().unwrap_or_else(|| "XXX".to_string())
    }

    fn digit_str(&self, dg: u8) -> &str {
        if dg == 0 {
            &self.symbols[SYM_ZERO_DIGIT]
        } else {
            &self.symbols[SYM_ONE_DIGIT + dg as usize - 1]
        }
    }

    fn render_affix(
        &self,
        p: &str,
        m: &Macros,
        q: &Dq,
        cur_override: Option<&str>,
    ) -> Vec<(String, Field)> {
        let mut out = Vec::new();
        for t in affix_tokens(p) {
            match t {
                Tok::Lit(c) => out.push((c.to_string(), Field::Other)),
                Tok::Minus => out.push((self.symbols[SYM_MINUS].clone(), Field::Other)),
                Tok::Plus => out.push((self.symbols[SYM_PLUS].clone(), Field::Other)),
                Tok::Percent => out.push((self.symbols[SYM_PERCENT].clone(), Field::Other)),
                Tok::Permille => out.push((self.symbols[SYM_PERMILL].clone(), Field::Other)),
                Tok::Currency(n) => {
                    let code = cur_override
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| self.currency_code(m));
                    let same = cur_override.is_none()
                        || self.locale_currency.as_deref() == cur_override
                        || self.props.currency.as_deref() == cur_override;
                    let s = match n {
                        1 => {
                            if self.custom_cur_sym && same {
                                self.symbols[SYM_CURRENCY].clone()
                            } else if code == "XXX" && self.locale_currency.is_none() {
                                numdata::currency_symbol(&self.tag, "XXX", CurrencyDisplay::Symbol)
                            } else {
                                numdata::currency_symbol(&self.tag, &code, CurrencyDisplay::Symbol)
                            }
                        }
                        2 => {
                            if self.custom_intl_sym && same {
                                self.symbols[SYM_INTL_CURRENCY].clone()
                            } else {
                                code.clone()
                            }
                        }
                        3 => numdata::currency_name(&self.tag, &code, q.to_f64().abs()),
                        4 => numdata::currency_symbol(
                            &self.tag,
                            &code,
                            CurrencyDisplay::NarrowSymbol,
                        ),
                        _ => numdata::currency_symbol(&self.tag, &code, CurrencyDisplay::Symbol),
                    };
                    out.push((s, Field::Currency));
                }
            }
        }
        out
    }

    /// Format a quantity. Err(code) on failure.
    pub fn format_dq(&self, mut q: Dq, cur_override: Option<&str>) -> Result<String, i32> {
        let m = self.macros();
        let currency_fmt = m.affixes.is_currency;
        // scale
        if q.is_finite() {
            if m.mag_mult != 0 {
                q.shift(m.mag_mult);
            }
            if m.mult != 1 {
                q.mul_int(m.mult);
            }
        }
        let prec = if let (Some(code), true) = (cur_override, currency_fmt) {
            // a currency amount: the currency's digits when the pattern did not fix them
            if self.props.min_frac == -1 && self.props.max_frac == -1 && self.props.max_sig == -1 {
                let d = numdata::currency_digits(code);
                Prec::Fraction(d, d)
            } else {
                m.prec.clone()
            }
        } else {
            m.prec.clone()
        };
        let mut min_frac_disp = 0;
        let mut exponent: Option<i32> = None;
        if q.is_finite() {
            if let Some((interval, req_min_int, _, _)) = m.sci {
                let get_mult = |mag: i32| -> i32 {
                    let shown = if req_min_int {
                        interval
                    } else if interval <= 1 {
                        1
                    } else {
                        (mag % interval + interval) % interval + 1
                    };
                    shown - mag - 1
                };
                if q.is_zero() {
                    min_frac_disp = apply_prec(&mut q, &prec, m.mode)?;
                    if let (true, Prec::Sig(min_sig, _)) = (req_min_int, &prec) {
                        // the zero shows `interval` integer digits
                        min_frac_disp = (min_sig - interval).max(0);
                    }
                    exponent = Some(0);
                } else {
                    let mag = q.upper();
                    let mult = get_mult(mag);
                    let mut t = q.clone();
                    t.shift(mult);
                    let mut mfd = apply_prec(&mut t, &prec, m.mode)?;
                    let mut used = mult;
                    if !t.is_zero() && t.upper() != mag + mult {
                        let mult2 = get_mult(mag + 1);
                        if mult2 != mult {
                            t = q.clone();
                            t.shift(mult2);
                            mfd = apply_prec(&mut t, &prec, m.mode)?;
                            used = mult2;
                        }
                    }
                    q = t;
                    min_frac_disp = mfd;
                    exponent = Some(-used);
                }
            } else {
                min_frac_disp = apply_prec(&mut q, &prec, m.mode)?;
            }
            if m.max_int >= 0 {
                q.truncate_int(m.max_int);
            }
        }
        let decimal_sym = if currency_fmt {
            &self.symbols[SYM_MONETARY_SEPARATOR]
        } else {
            &self.symbols[SYM_DECIMAL]
        };
        let group_sym = if currency_fmt {
            &self.symbols[SYM_MONETARY_GROUPING]
        } else {
            &self.symbols[SYM_GROUPING]
        };
        // the number
        let mut num = String::new();
        if q.nan {
            num.push_str(&self.symbols[SYM_NAN]);
        } else if q.inf {
            num.push_str(&self.symbols[SYM_INFINITY]);
        } else {
            let top = q.upper().max(m.min_int - 1);
            // (significant digits show a digit for zero)
            let zero_min = if matches!(prec, Prec::Sig(..)) {
                m.min_int.max(1)
            } else {
                m.min_int
            };
            let int_top = if q.is_zero() { zero_min - 1 } else { top };
            let mut mag = int_top;
            while mag >= 0 {
                num.push_str(self.digit_str(q.digit(mag)));
                if m.grouping && mag > 0 {
                    let pos = mag - m.g1;
                    if pos >= 0 && m.g2 > 0 && pos % m.g2 == 0 {
                        num.push_str(group_sym);
                    }
                }
                mag -= 1;
            }
            let low = q.lower().min(0).min(-min_frac_disp);
            if low < 0 || m.decimal_always {
                if m.affixes.is_currency && self.props.currency_as_decimal {
                    num.push_str(&self.symbols[SYM_CURRENCY]);
                } else {
                    num.push_str(decimal_sym);
                }
            }
            let mut mag = -1;
            while mag >= low {
                num.push_str(self.digit_str(q.digit(mag)));
                mag -= 1;
            }
            if let (Some(ex), Some((_, _, min_exp, sign_always))) = (exponent, m.sci) {
                num.push_str(&self.symbols[SYM_EXPONENTIAL]);
                if ex < 0 {
                    num.push_str(&self.symbols[SYM_MINUS]);
                } else if sign_always {
                    num.push_str(&self.symbols[SYM_PLUS]);
                }
                let ds = ex.unsigned_abs().to_string();
                for _ in ds.len()..(min_exp.max(0) as usize) {
                    num.push_str(self.digit_str(0));
                }
                for b in ds.bytes() {
                    num.push_str(self.digit_str(b - b'0'));
                }
            }
        }
        // affixes
        let negative = q.neg && !q.nan;
        let (pre_p, suf_p) = if negative {
            (&m.affixes.neg_prefix, &m.affixes.neg_suffix)
        } else {
            (&m.affixes.pos_prefix, &m.affixes.pos_suffix)
        };
        let pre = self.render_affix(pre_p, &m, &q, cur_override);
        let suf = self.render_affix(suf_p, &m, &q, cur_override);
        let mut pre_s: String = pre.iter().map(|x| x.0.as_str()).collect();
        let mut suf_s: String = suf.iter().map(|x| x.0.as_str()).collect();
        // currency spacing
        let in_currency = |c: char| {
            let gc = intl::unicode::general_category(c);
            !gc.is_symbol() && !gc.is_separator()
        };
        let is_digit = |c: char| intl::unicode::is_decimal_digit(c);
        if let Some(last) = pre.iter().rev().find(|x| !x.0.is_empty()) {
            if last.1 == Field::Currency
                && last.0.chars().last().is_some_and(in_currency)
                && num.chars().next().is_some_and(is_digit)
            {
                pre_s.push('\u{a0}');
            }
        }
        if let Some(first) = suf.iter().find(|x| !x.0.is_empty()) {
            if first.1 == Field::Currency
                && first.0.chars().next().is_some_and(in_currency)
                && num.chars().last().is_some_and(is_digit)
            {
                suf_s.insert(0, '\u{a0}');
            }
        }
        // padding
        let total = pre_s.chars().count() + num.chars().count() + suf_s.chars().count();
        let mut out = String::new();
        if m.width > 0 && (total as i32) < m.width {
            let padn = (m.width as usize) - total;
            let pad: String = std::iter::repeat_n(m.pad, padn).collect();
            match m.pad_pos {
                1 => {
                    out.push_str(&pre_s);
                    out.push_str(&pad);
                    out.push_str(&num);
                    out.push_str(&suf_s);
                }
                2 => {
                    out.push_str(&pre_s);
                    out.push_str(&num);
                    out.push_str(&pad);
                    out.push_str(&suf_s);
                }
                3 => {
                    out.push_str(&pre_s);
                    out.push_str(&num);
                    out.push_str(&suf_s);
                    out.push_str(&pad);
                }
                _ => {
                    out.push_str(&pad);
                    out.push_str(&pre_s);
                    out.push_str(&num);
                    out.push_str(&suf_s);
                }
            }
        } else {
            out.push_str(&pre_s);
            out.push_str(&num);
            out.push_str(&suf_s);
        }
        Ok(out)
    }

    pub fn format_f64(&self, v: f64) -> Result<String, i32> {
        self.format_dq(Dq::from_f64(v), None)
    }

    /// The quantity as formatting rounds it (scale and precision; not in
    /// scientific notation), with the fraction digits shown: what plural
    /// rules select on (formatToDecimalQuantity()).
    pub fn rounded(&self, mut q: Dq) -> (Dq, i32) {
        let m = self.macros();
        if !q.is_finite() {
            return (q, 0);
        }
        if m.mag_mult != 0 {
            q.shift(m.mag_mult);
        }
        if m.mult != 1 {
            q.mul_int(m.mult);
        }
        if m.sci.is_some() {
            return (q, 0);
        }
        let mfd = apply_prec(&mut q, &m.prec, m.mode).unwrap_or(0);
        if m.max_int >= 0 {
            q.truncate_int(m.max_int);
        }
        (q, mfd)
    }

    pub fn format_i64(&self, v: i64) -> Result<String, i32> {
        self.format_dq(Dq::from_i64(v), None)
    }

    /// The rendered affix strings (getPositivePrefix() and friends).
    pub fn affix_text(&self, which: usize) -> String {
        let m = self.macros();
        let p = match which {
            0 => &m.affixes.pos_prefix,
            1 => &m.affixes.pos_suffix,
            2 => &m.affixes.neg_prefix,
            _ => &m.affixes.neg_suffix,
        };
        let q = Dq::from_i64(if which >= 2 { -1 } else { 1 });
        self.render_affix(p, &m, &q, None)
            .into_iter()
            .map(|x| x.0)
            .collect()
    }

    // -----------------------------------------------------------------------
    // toPattern

    /// PatternStringUtils::propertiesToPatternString()
    pub fn to_pattern(&self) -> String {
        let p = &self.props;
        let m = self.macros();
        let dos = 100;
        let mut g1 = p.grouping_size.min(dos).max(0);
        let mut g2 = p.secondary_grouping.min(dos).max(0);
        let use_grouping = p.grouping_used;
        let padding_width = p.format_width.min(dos);
        // raw properties, with the exported rounding of a currency format
        let use_currency = p.currency.is_some() || m.affixes.is_currency;
        let (rmin_frac, rmax_frac, rinc) = if use_currency {
            (m.ex_min_frac, m.ex_max_frac, m.ex_increment)
        } else {
            (p.min_frac, p.max_frac, p.rounding_increment)
        };
        let min_int = p.min_int.min(dos).max(0);
        // (the maximum integer digits only show in a scientific pattern)
        let max_int = if p.max_int < 0 || p.min_exp_digits == -1 {
            dos
        } else {
            p.max_int.min(dos)
        };
        let min_frac = rmin_frac.min(dos).max(0);
        let max_frac = if rmax_frac < 0 {
            dos
        } else {
            rmax_frac.min(dos)
        };
        let min_sig = p.min_sig.min(dos);
        let max_sig = p.max_sig.min(dos);
        let always = p.decimal_always;
        let exp_digits = p.min_exp_digits.min(dos);
        let exp_plus = p.exp_sign_always;
        let aff = &m.affixes;
        let mut sb: Vec<char> = aff.pos_prefix.chars().collect();
        let mut after_prefix = sb.len();
        if !use_grouping {
            g1 = 0;
            g2 = 0;
        } else if g1 == g2 {
            g1 = 0;
        }
        let grouping_len = g1 + g2 + 1;
        let increment = rinc;
        let mut digits: Vec<char> = Vec::new();
        let mut scale: i32 = 0;
        if max_sig != -1 {
            while (digits.len() as i32) < min_sig {
                digits.push('@');
            }
            while (digits.len() as i32) < max_sig {
                digits.push('#');
            }
        } else if increment != 0.0 {
            let q = Dq::from_f64(increment);
            scale = q.lower();
            let mut s: Vec<char> = q.d.iter().map(|&x| (b'0' + x) as char).collect();
            while (s.len() as i32) < min_int - scale {
                s.insert(0, '0');
            }
            digits = s;
        }
        while (digits.len() as i32) + scale < min_int {
            digits.insert(0, '0');
        }
        while -scale < min_frac {
            digits.push('0');
            scale -= 1;
        }
        let mut m0 = grouping_len.max(digits.len() as i32 + scale);
        m0 = if max_int != dos {
            max_int.max(min_int) - 1
        } else {
            m0 - 1
        };
        let mn = if max_frac != dos {
            (-max_frac).min(scale)
        } else {
            scale
        };
        let mut mag = m0;
        while mag >= mn {
            let di = digits.len() as i32 + scale - mag - 1;
            if di < 0 || di >= digits.len() as i32 {
                sb.push('#');
            } else {
                sb.push(digits[di as usize]);
            }
            if mag == 0 && (always || mn < 0) {
                sb.push(if p.currency_as_decimal { '¤' } else { '.' });
            }
            if use_grouping {
                if mag > 0 && mag == g1 {
                    sb.push(',');
                }
                if mag > g1 && g2 > 0 && (mag - g1) % g2 == 0 {
                    sb.push(',');
                }
            }
            mag -= 1;
        }
        if exp_digits != -1 {
            sb.push('E');
            if exp_plus {
                sb.push('+');
            }
            sb.extend(std::iter::repeat_n('0', exp_digits.max(0) as usize));
        }
        let mut before_suffix = sb.len();
        sb.extend(aff.pos_suffix.chars());
        if padding_width > 0 {
            if let Some(loc) = p.pad_position {
                while (padding_width as usize) > sb.len() {
                    sb.insert(after_prefix, '#');
                    before_suffix += 1;
                }
                let ps = p.pad_string.clone().unwrap_or_else(|| " ".to_string());
                let esc = escape_padding(&ps);
                let n = esc.len();
                match loc {
                    0 => {
                        for (k, c) in esc.iter().enumerate() {
                            sb.insert(k, *c);
                        }
                        sb.insert(0, '*');
                        after_prefix += n + 1;
                        before_suffix += n + 1;
                    }
                    1 => {
                        for (k, c) in esc.iter().enumerate() {
                            sb.insert(after_prefix + k, *c);
                        }
                        sb.insert(after_prefix, '*');
                        after_prefix += n + 1;
                        before_suffix += n + 1;
                    }
                    2 => {
                        for (k, c) in esc.iter().enumerate() {
                            sb.insert(before_suffix + k, *c);
                        }
                        sb.insert(before_suffix, '*');
                    }
                    _ => {
                        sb.push('*');
                        sb.extend(esc.iter());
                    }
                }
            }
        }
        if aff.has_negative_subpattern() {
            sb.push(';');
            sb.extend(aff.neg_prefix.chars());
            let copy: Vec<char> = sb[after_prefix..before_suffix].to_vec();
            sb.extend(copy);
            sb.extend(aff.neg_suffix.chars());
        }
        sb.into_iter().collect()
    }

    // -----------------------------------------------------------------------
    // Attributes (unum_getAttribute / unum_setAttribute)

    pub fn get_attribute(&self, attr: i64) -> i64 {
        let m = self.macros();
        let p = &self.props;
        match attr {
            0 => p.parse_int_only as i64,
            1 => p.grouping_used as i64,
            2 => p.decimal_always as i64,
            3 => {
                if m.ex_max_int < 0 {
                    2_000_000_000
                } else {
                    m.ex_max_int as i64
                }
            }
            4 | 5 => m.ex_min_int as i64,
            6 => m.ex_max_frac.max(0) as i64,
            7 | 8 => m.ex_min_frac.max(0) as i64,
            9 => {
                if p.multiplier != 1 {
                    p.multiplier
                } else if p.magnitude_multiplier != 0 {
                    10i64.saturating_pow(p.magnitude_multiplier.max(0) as u32)
                } else {
                    1
                }
            }
            10 => p.grouping_size.max(0) as i64,
            11 => p.rounding_mode.unwrap_or(ROUND_HALFEVEN) as i64,
            13 => p.format_width as i64,
            14 => p.pad_position.unwrap_or(0) as i64,
            15 => p.secondary_grouping.max(0) as i64,
            16 => (p.min_sig != -1 || p.max_sig != -1) as i64,
            17 => m.ex_min_sig as i64,
            18 => m.ex_max_sig as i64,
            19 => p.lenient as i64,
            _ => -1,
        }
    }

    pub fn rounding_increment(&self) -> f64 {
        self.macros().ex_increment
    }

    pub fn set_attribute(&mut self, attr: i64, v: i64) {
        let n = v.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let p = &mut self.props;
        match attr {
            0 => p.parse_int_only = v != 0,
            1 => p.grouping_used = v != 0,
            2 => p.decimal_always = v != 0,
            3 => set_max(&mut p.min_int, &mut p.max_int, n),
            4 => set_min(&mut p.min_int, &mut p.max_int, n),
            5 => {
                set_min(&mut p.min_int, &mut p.max_int, n);
                set_max(&mut p.min_int, &mut p.max_int, n);
            }
            6 => set_max(&mut p.min_frac, &mut p.max_frac, n),
            7 => set_min(&mut p.min_frac, &mut p.max_frac, n),
            8 => {
                set_min(&mut p.min_frac, &mut p.max_frac, n);
                set_max(&mut p.min_frac, &mut p.max_frac, n);
            }
            9 => {
                let mut mult = v;
                if mult == 0 {
                    mult = 1;
                }
                let mut delta = 0;
                let mut value = mult;
                while value != 1 && value % 10 == 0 {
                    value /= 10;
                    delta += 1;
                }
                if value == 1 && mult > 0 {
                    p.magnitude_multiplier = delta;
                    p.multiplier = 1;
                } else {
                    p.magnitude_multiplier = 0;
                    p.multiplier = mult;
                }
            }
            10 => p.grouping_size = n,
            11 => p.rounding_mode = Some(n),
            13 => p.format_width = n,
            14 => p.pad_position = Some(n),
            15 => p.secondary_grouping = n,
            16 => {
                let used = v != 0;
                if used {
                    if p.min_sig != -1 || p.max_sig != -1 {
                        return;
                    }
                    p.min_sig = 1;
                    p.max_sig = 6;
                } else {
                    p.min_sig = -1;
                    p.max_sig = -1;
                }
            }
            17 => set_min(&mut p.min_sig, &mut p.max_sig, n),
            18 => set_max(&mut p.min_sig, &mut p.max_sig, n),
            19 => {
                p.lenient = v != 0;
                p.parse_strict = v == 0;
            }
            _ => {}
        }
    }

    pub fn set_rounding_increment(&mut self, v: f64) {
        self.props.rounding_increment = v;
    }

    pub fn set_currency(&mut self, code: &str) {
        let c = code.to_ascii_uppercase();
        self.props.currency = Some(c.clone());
        self.currency_attr = Some(c.clone());
        if !self.custom_cur_sym {
            self.symbols[SYM_CURRENCY] =
                numdata::currency_symbol(&self.tag, &c, CurrencyDisplay::Symbol);
        }
        if !self.custom_intl_sym {
            self.symbols[SYM_INTL_CURRENCY] = c;
        }
    }

    pub fn set_symbol(&mut self, sym: usize, v: &str) {
        if sym >= SYM_COUNT {
            return;
        }
        self.symbols[sym] = v.to_string();
        if sym == SYM_CURRENCY {
            self.custom_cur_sym = true;
        } else if sym == SYM_INTL_CURRENCY {
            self.custom_intl_sym = true;
        } else if sym == SYM_ZERO_DIGIT {
            let cs: Vec<char> = v.chars().collect();
            if cs.len() == 1 && intl::unicode::is_decimal_digit(cs[0]) {
                for i in 1..10 {
                    if let Some(c) = char::from_u32(cs[0] as u32 + i as u32) {
                        self.symbols[SYM_ONE_DIGIT + i - 1] = c.to_string();
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Parsing (NumberParserImpl, lenient mode)

    /// Parse `text` (code points) from `pos`: (value, end position, the
    /// currency matched) or Err(error position). With `want_currency`, a
    /// currency must have been matched.
    pub fn parse(
        &self,
        text: &[char],
        pos: usize,
        want_currency: bool,
    ) -> Result<(Dq, usize, Option<String>), usize> {
        let m = self.macros();
        let mut best: Option<(Dq, usize, Option<String>)> = None;
        let mut furthest = pos;
        for neg in [true, false] {
            let (pre_p, suf_p) = if neg {
                (&m.affixes.neg_prefix, &m.affixes.neg_suffix)
            } else {
                (&m.affixes.pos_prefix, &m.affixes.pos_suffix)
            };
            let mut cur: Option<String> = None;
            let Some(i) = self.match_affix(pre_p, &m, text, pos, &mut cur, true) else {
                // (the error position is past the number, as ICU's
                // lenient matchers get there)
                if let Ok((_, j)) = self.parse_number(text, pos, &m) {
                    furthest = furthest.max(j);
                }
                continue;
            };
            furthest = furthest.max(i);
            let (mut q, j) = match self.parse_number(text, i, &m) {
                Ok(x) => x,
                Err(e) => {
                    furthest = furthest.max(e);
                    continue;
                }
            };
            furthest = furthest.max(j);
            let Some(k) = self.match_affix(suf_p, &m, text, j, &mut cur, false) else {
                continue;
            };
            furthest = furthest.max(k);
            if want_currency && cur.is_none() {
                continue;
            }
            q.neg = neg && !q.nan;
            if best.as_ref().is_none_or(|b| k > b.1) {
                best = Some((q, k, cur));
            }
        }
        let Some((mut q, end, cur)) = best else {
            return Err(furthest);
        };
        // undo the multiplier
        if q.is_finite() {
            if m.mag_mult != 0 {
                q.shift(-m.mag_mult);
            }
            if m.mult != 1 {
                let v = q.to_f64() / m.mult as f64;
                let neg = q.neg;
                q = Dq::from_f64(v);
                q.neg = neg;
            }
        }
        Ok((q, end, cur))
    }

    fn match_affix(
        &self,
        p: &str,
        m: &Macros,
        text: &[char],
        mut i: usize,
        cur: &mut Option<String>,
        prefix: bool,
    ) -> Option<usize> {
        let toks = affix_tokens(p);
        let n = toks.len();
        for (ti, t) in toks.into_iter().enumerate() {
            i = skip_ignorables(text, i);
            let s: String = match t {
                Tok::Lit(c) => c.to_string(),
                Tok::Minus => {
                    if let Some(j) = match_str(text, i, &self.symbols[SYM_MINUS]) {
                        i = j;
                        continue;
                    }
                    if i < text.len() && is_minus_sign(text[i]) {
                        i += 1;
                        continue;
                    }
                    return None;
                }
                Tok::Plus => self.symbols[SYM_PLUS].clone(),
                Tok::Percent => self.symbols[SYM_PERCENT].clone(),
                Tok::Permille => self.symbols[SYM_PERMILL].clone(),
                Tok::Currency(_) => {
                    // the currency spacing insert before a suffix currency
                    if !prefix && ti == 0 && i < text.len() && text[i] == '\u{a0}' {
                        i += 1;
                    }
                    let (code, mut j) = self.match_currency(text, i, m)?;
                    *cur = Some(code);
                    // ... and after a prefix currency
                    if prefix && ti + 1 == n && j < text.len() && text[j] == '\u{a0}' {
                        j += 1;
                    }
                    i = j;
                    continue;
                }
            };
            if matches!(s.as_str(), "\u{200e}" | "\u{200f}" | "\u{61c}") {
                continue;
            }
            i = match_str(text, i, &s)?;
        }
        Some(skip_ignorables(text, i))
    }

    fn match_currency(&self, text: &[char], i: usize, m: &Macros) -> Option<(String, usize)> {
        let mut cands: Vec<(String, String)> = Vec::new();
        let own = self.currency_code(m);
        if self.custom_cur_sym {
            cands.push((self.symbols[SYM_CURRENCY].clone(), own.clone()));
        }
        let mut codes: Vec<&str> = vec![own.as_str()];
        codes.extend(numdata::COMMON_CURRENCIES.iter());
        for c in codes {
            cands.push((
                numdata::currency_symbol(&self.tag, c, CurrencyDisplay::Symbol),
                c.to_string(),
            ));
            cands.push((
                numdata::currency_symbol(&self.tag, c, CurrencyDisplay::NarrowSymbol),
                c.to_string(),
            ));
            cands.push((c.to_string(), c.to_string()));
        }
        // the longest match
        let mut best: Option<(String, usize)> = None;
        for (sym, code) in cands {
            if sym.is_empty() || sym == "¤" {
                continue;
            }
            if let Some(j) = match_str(text, i, &sym) {
                if best.as_ref().is_none_or(|b| j > b.1) {
                    best = Some((code, j));
                }
            }
        }
        if best.is_none()
            && i + 3 <= text.len()
            && text[i..i + 3].iter().all(|c| c.is_ascii_uppercase())
        {
            // any ISO code
            let code: String = text[i..i + 3].iter().collect();
            best = Some((code, i + 3));
        }
        best
    }

    /// The number (DecimalMatcher + ScientificMatcher): Err(start) when
    /// there is none or its grouping is invalid.
    fn parse_number(&self, text: &[char], start: usize, m: &Macros) -> Result<(Dq, usize), usize> {
        let i = skip_ignorables(text, start);
        let currency_fmt = m.affixes.is_currency;
        let dec_sym = if currency_fmt {
            &self.symbols[SYM_MONETARY_SEPARATOR]
        } else {
            &self.symbols[SYM_DECIMAL]
        };
        let grp_sym = if currency_fmt {
            &self.symbols[SYM_MONETARY_GROUPING]
        } else {
            &self.symbols[SYM_GROUPING]
        };
        if let Some(j) = match_str(text, i, &self.symbols[SYM_NAN]) {
            let q = Dq {
                nan: true,
                ..Default::default()
            };
            return Ok((q, j));
        }
        if let Some(j) = match_str(text, i, &self.symbols[SYM_INFINITY]) {
            let q = Dq {
                inf: true,
                ..Default::default()
            };
            return Ok((q, j));
        }
        let strict = self.props.parse_strict;
        let dec_first = dec_sym.chars().next();
        let (comma, period) = if strict {
            (STRICT_COMMA_SET, STRICT_PERIOD_SET)
        } else {
            (COMMA_SET, PERIOD_SET)
        };
        let dec_set: &[char] = match dec_first {
            Some(c) if comma.contains(&c) => comma,
            Some(c) if period.contains(&c) => period,
            _ => &[],
        };
        let is_dec = |k: usize| -> Option<usize> {
            if let Some(j) = match_str(text, k, dec_sym) {
                return Some(j);
            }
            if k < text.len() && dec_set.contains(&text[k]) {
                return Some(k + 1);
            }
            None
        };
        let grouping_disabled = !self.props.grouping_used || m.g1 <= 0;
        let is_grp = |k: usize| -> Option<usize> {
            if let Some(j) = match_str(text, k, grp_sym) {
                return Some(j);
            }
            if k < text.len() {
                let c = text[k];
                if comma.contains(&c) || period.contains(&c) || OTHER_GROUPING.contains(&c) {
                    return Some(k + 1);
                }
            }
            None
        };
        let mut int_digits = String::new();
        let mut frac_digits = String::new();
        let mut groups: Vec<usize> = vec![0];
        let mut seen_sep = false;
        let mut seen_dec = false;
        let mut k = i;
        // where the number ends if the scan stops here
        let mut end = i;
        let mut last_was_sep = false;
        while k < text.len() {
            if let Some(d) = self.digit_of(text[k]) {
                if seen_dec {
                    frac_digits.push((b'0' + d) as char);
                } else {
                    int_digits.push((b'0' + d) as char);
                    *groups.last_mut().unwrap() += 1;
                }
                k += 1;
                end = k;
                last_was_sep = false;
                continue;
            }
            if !seen_dec {
                if let Some(j) = is_dec(k) {
                    if last_was_sep || self.props.parse_int_only {
                        break;
                    }
                    seen_dec = true;
                    k = j;
                    if !int_digits.is_empty() {
                        end = k;
                    }
                    continue;
                }
            }
            if !seen_dec && !grouping_disabled && !int_digits.is_empty() {
                if let Some(j) = is_grp(k) {
                    if last_was_sep {
                        // two separators: the number ends before the first
                        groups.pop();
                        break;
                    }
                    last_was_sep = true;
                    seen_sep = true;
                    groups.push(0);
                    k = j;
                    continue;
                }
            }
            break;
        }
        if last_was_sep {
            groups.pop();
        }
        if int_digits.is_empty() && frac_digits.is_empty() {
            return Err(start);
        }
        // grouping validation
        let seps = groups.len() > 1;
        if seen_sep && seps {
            let g1 = m.g1.max(1) as usize;
            let g2 = m.g2.max(1) as usize;
            let n = groups.len();
            let ok = groups[n - 1] == g1
                && groups[1..n - 1].iter().all(|&x| x == g2)
                && groups[0] >= 1
                && groups[0] <= g2;
            if !ok {
                return Err(start);
            }
        }
        let mut k = end;
        // exponent
        let mut exp: i64 = 0;
        if !self.props.parse_int_only {
            let es = &self.symbols[SYM_EXPONENTIAL];
            let ej = match_str(text, k, es).or_else(|| {
                if k < text.len() && (text[k] == 'E' || text[k] == 'e') {
                    Some(k + 1)
                } else {
                    None
                }
            });
            if let Some(mut j) = ej {
                let mut eneg = false;
                if let Some(x) = match_str(text, j, &self.symbols[SYM_MINUS]) {
                    eneg = true;
                    j = x;
                } else if j < text.len() && is_minus_sign(text[j]) {
                    eneg = true;
                    j += 1;
                } else if let Some(x) =
                    match_str(text, j, &self.symbols[SYM_PLUS]).or_else(|| match_str(text, j, "+"))
                {
                    j = x;
                }
                let mut ed = String::new();
                while j < text.len() {
                    match self.digit_of(text[j]) {
                        Some(d) => ed.push((b'0' + d) as char),
                        None => break,
                    }
                    j += 1;
                }
                if !ed.is_empty() {
                    exp = ed.parse::<i64>().unwrap_or(9999).min(9999);
                    if eneg {
                        exp = -exp;
                    }
                    k = j;
                }
            }
        }
        let s = format!(
            "{}{}{}e{}",
            if int_digits.is_empty() {
                "0"
            } else {
                &int_digits
            },
            if frac_digits.is_empty() { "" } else { "." },
            frac_digits,
            exp
        );
        let q = Dq::parse_decimal(&s).ok_or(start)?;
        Ok((q, k))
    }

    fn digit_of(&self, c: char) -> Option<u8> {
        for k in 0..10 {
            let s = if k == 0 {
                &self.symbols[SYM_ZERO_DIGIT]
            } else {
                &self.symbols[SYM_ONE_DIGIT + k - 1]
            };
            let mut it = s.chars();
            if it.next() == Some(c) && it.next().is_none() {
                return Some(k as u8);
            }
        }
        if intl::unicode::is_decimal_digit(c) {
            let d = intl::unicode::numeric_value(c)?.to_i64()?;
            if (0..10).contains(&d) {
                return Some(d as u8);
            }
        }
        None
    }
}

/// unisets COMMA / PERIOD / OTHER_GROUPING_SEPARATORS
const COMMA_SET: &[char] = &[',', '،', '٫', '、', '︐', '︑', '﹐', '﹑', '，', '､'];
const PERIOD_SET: &[char] = &['.', '․', '。', '︒', '﹒', '．', '｡'];
/// unisets STRICT_COMMA / STRICT_PERIOD
const STRICT_COMMA_SET: &[char] = &[',', '٫', '︐', '﹐', '，'];
const STRICT_PERIOD_SET: &[char] = &['.', '․', '﹒', '．', '｡'];
const OTHER_GROUPING: &[char] = &[
    ' ', '\'', '\u{a0}', '\u{66c}', '\u{2000}', '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}',
    '\u{2005}', '\u{2006}', '\u{2007}', '\u{2008}', '\u{2009}', '\u{200a}', '\u{2018}', '\u{2019}',
    '\u{202f}', '\u{205f}', '\u{3000}', '\u{ff07}',
];

/// unisets MINUS_SIGN
fn is_minus_sign(c: char) -> bool {
    matches!(c, '-' | '⁻' | '₋' | '−' | '➖' | '﹣' | '－')
}

/// Skip the parse ignorables (bidi controls, variation selectors).
fn skip_ignorables(text: &[char], mut i: usize) -> usize {
    while i < text.len()
        && matches!(text[i], '\u{200e}' | '\u{200f}' | '\u{61c}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{fe00}'..='\u{fe0f}')
    {
        i += 1;
    }
    i
}

fn set_min(min: &mut i32, max: &mut i32, v: i32) {
    if *max >= 0 && *max < v {
        *max = v;
    }
    *min = v;
}

fn set_max(min: &mut i32, max: &mut i32, v: i32) {
    if *min >= 0 && *min > v {
        *min = v;
    }
    *max = v;
}

fn escape_padding(s: &str) -> Vec<char> {
    if s.is_empty() {
        return " ".chars().collect();
    }
    let cs: Vec<char> = s.chars().collect();
    if cs.len() == 1 {
        if cs[0] == '\'' {
            return vec!['\'', '\''];
        }
        return cs;
    }
    let mut out = vec!['\''];
    for c in cs {
        if c == '\'' {
            out.push('\'');
            out.push('\'');
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Whether `s` occurs in `text` at `i`; the position after it.
pub fn match_str(text: &[char], i: usize, s: &str) -> Option<usize> {
    let mut j = i;
    for c in s.chars() {
        if j >= text.len() || text[j] != c {
            return None;
        }
        j += 1;
    }
    if j == i {
        return None;
    }
    Some(j)
}

/// Apply a precision; the minimum fraction digits to display.
fn apply_prec(q: &mut Dq, p: &Prec, mode: i32) -> Result<i32, i32> {
    let inexact = |_: ()| error::U_FORMAT_INEXACT_ERROR;
    match p {
        Prec::Unlimited => Ok(0),
        Prec::Fraction(min, max) => {
            if *max >= 0 {
                q.round_at(-*max, mode).map_err(inexact)?;
            }
            Ok(*min)
        }
        Prec::Sig(min, max) => {
            if !q.is_zero() {
                let mag = q.upper() - *max + 1;
                q.round_at(mag, mode).map_err(inexact)?;
            }
            let low = q.upper() - *min + 1;
            Ok((-low).max(0))
        }
        Prec::Increment(inc, min_frac) => {
            q.round_increment(inc, mode).map_err(inexact)?;
            Ok(*min_frac)
        }
    }
}

/// U_NUMBER_SKELETON_SYNTAX_ERROR
pub const U_NUMBER_SKELETON_SYNTAX_ERROR: i32 = 65811;

/// What a number skeleton asks for beyond the DecimalFormat properties.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SkeletonExtra {
    /// compact notation: Some(long)
    pub compact: Option<bool>,
}

/// Apply ICU number skeleton tokens (NumberFormatter::forSkeleton()) over
/// a formatter opened with the locale's decimal pattern; the unit tokens
/// switch to the locale's percent / currency patterns (`percent`,
/// `currency` from LocaleData).
pub fn apply_skeleton(
    df: &mut DecimalFormat,
    data: &numdata::LocaleData,
    skeleton: &str,
) -> Result<SkeletonExtra, i32> {
    let mut extra = SkeletonExtra::default();
    let bad = Err(U_NUMBER_SKELETON_SYNTAX_ERROR);
    // the NumberFormatter defaults: maximum 6 fraction digits
    df.props.min_frac = -1;
    df.props.max_frac = -1;
    let mut sign: Option<&str> = None;
    for tok in skeleton.split_whitespace() {
        let (stem, opt) = match tok.split_once('/') {
            Some((a, b)) => (a, Some(b)),
            None => (tok, None),
        };
        match stem {
            // (a unit: the value is not scaled, unlike %x100)
            "percent" | "%" | "%x100" | "permille" => {
                let p = if stem == "permille" {
                    "#,##0‰"
                } else {
                    &data.percent
                };
                apply_pattern(&mut df.props, p, 2)?;
                if stem != "%x100" {
                    df.props.magnitude_multiplier = 0;
                }
                df.props.min_frac = -1;
                df.props.max_frac = -1;
            }
            "base-unit" => {}
            "currency" => {
                let Some(code) = opt.filter(|c| c.len() == 3) else {
                    return bad;
                };
                apply_pattern(&mut df.props, &data.currency, 1)?;
                df.set_currency(code);
            }
            "measure-unit"
            | "unit"
            | "per-measure-unit"
            | "unit-width-narrow"
            | "unit-width-short"
            | "unit-width-full-name"
            | "unit-width-iso-code"
            | "unit-width-hidden"
            | "numbering-system"
            | "latin"
            | "usage" => {}
            "group-off" | ",_" => df.props.grouping_used = false,
            "group-min2" | ",?" | "group-auto" | "group-on-aligned" | ",!" | "group-thousands"
            | ",=" => df.props.grouping_used = true,
            "sign-always" | "+!" => sign = Some("+"),
            "sign-except-zero" | "+?" => sign = Some("+"),
            "sign-auto" | "sign-never" | "+_" | "sign-negative" | "+-" => {}
            "sign-accounting" | "()" => {
                let pre = df.props.pos_prefix_pat.clone().unwrap_or_default();
                let suf = df.props.pos_suffix_pat.clone().unwrap_or_default();
                df.props.neg_prefix_pat = Some(format!("({}", pre));
                df.props.neg_suffix_pat = Some(format!("{})", suf));
            }
            "decimal-always" => df.props.decimal_always = true,
            "decimal-auto" => df.props.decimal_always = false,
            "compact-short" | "K" => extra.compact = Some(false),
            "compact-long" | "KK" => extra.compact = Some(true),
            "notation-simple" => {}
            "scientific" | "engineering" | "E0" | "EE0" => {
                df.props.min_exp_digits = 1;
                df.props.min_int = 1;
                df.props.max_int = if stem.starts_with("engineering") || stem == "EE0" {
                    3
                } else {
                    1
                };
            }
            "precision-integer" | "." => {
                df.props.min_frac = 0;
                df.props.max_frac = 0;
            }
            "precision-unlimited" => {
                df.props.min_frac = 0;
                df.props.max_frac = -1;
            }
            "precision-currency-standard" | "precision-currency-cash" => {}
            "precision-increment" => {
                let Some(v) = opt.and_then(|o| o.parse::<f64>().ok()) else {
                    return bad;
                };
                df.props.rounding_increment = v;
                let fd = opt
                    .unwrap_or("")
                    .split('.')
                    .nth(1)
                    .map(|f| f.len())
                    .unwrap_or(0) as i32;
                df.props.min_frac = fd;
                df.props.max_frac = fd;
            }
            "scale" => {
                let Some(v) = opt.and_then(|o| o.parse::<f64>().ok()) else {
                    return bad;
                };
                if v.fract() == 0.0 {
                    df.set_attribute(9, v as i64);
                }
            }
            "integer-width" | "integer-width-trunc" => {
                let o = opt.unwrap_or("");
                let (plus, body) = match o.strip_prefix(['+', '*']) {
                    Some(b) => (true, b),
                    None => (false, o),
                };
                let zeros = body.chars().filter(|&c| c == '0').count() as i32;
                let hashes = body.chars().filter(|&c| c == '#').count() as i32;
                df.props.min_int = zeros;
                df.props.max_int = if plus { -1 } else { zeros + hashes };
            }
            s if s.starts_with("rounding-mode-") => {
                let m = match &s[14..] {
                    "ceiling" => ROUND_CEILING,
                    "floor" => ROUND_FLOOR,
                    "down" => ROUND_DOWN,
                    "up" => ROUND_UP,
                    "half-even" => ROUND_HALFEVEN,
                    "half-down" => ROUND_HALFDOWN,
                    "half-up" => ROUND_HALFUP,
                    "unnecessary" => ROUND_UNNECESSARY,
                    "half-odd" => ROUND_HALF_ODD,
                    "half-ceiling" => ROUND_HALF_CEILING,
                    "half-floor" => ROUND_HALF_FLOOR,
                    _ => return bad,
                };
                df.props.rounding_mode = Some(m);
            }
            s if s.starts_with('.') => {
                // fraction precision: .00, .0#, .##, .00*, .00+
                let body = &s[1..];
                let (body, unlimited) = match body.strip_suffix(['*', '+']) {
                    Some(b) => (b, true),
                    None => (body, false),
                };
                if !body.chars().all(|c| c == '0' || c == '#') {
                    return bad;
                }
                let zeros = body.chars().take_while(|&c| c == '0').count() as i32;
                df.props.min_frac = zeros;
                df.props.max_frac = if unlimited { -1 } else { body.len() as i32 };
            }
            s if s.starts_with('@') => {
                let (body, unlimited) = match s.strip_suffix(['*', '+']) {
                    Some(b) => (b, true),
                    None => (s, false),
                };
                if !body.chars().all(|c| c == '@' || c == '#') {
                    return bad;
                }
                let at = body.chars().take_while(|&c| c == '@').count() as i32;
                df.props.min_sig = at;
                df.props.max_sig = if unlimited { -1 } else { body.len() as i32 };
            }
            s if s.starts_with('0') && s.chars().all(|c| c == '0') => {
                df.props.min_int = s.len() as i32;
            }
            _ => return bad,
        }
    }
    if let Some(sg) = sign {
        let pre = df.props.pos_prefix_pat.clone().unwrap_or_default();
        df.props.pos_prefix_pat = Some(format!("{}{}", sg, pre));
        if df.props.neg_prefix_pat.is_none() {
            df.props.neg_prefix_pat = Some(format!("-{}", pre));
            df.props.neg_suffix_pat = df.props.pos_suffix_pat.clone();
        }
    }
    Ok(extra)
}
