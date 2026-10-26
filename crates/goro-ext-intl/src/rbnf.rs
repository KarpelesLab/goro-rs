//! RuleBasedNumberFormat (ICU's rbnf.cpp, nfrs.cpp, nfrule.cpp,
//! nfsubs.cpp): the custom rules of NumberFormatter::PATTERN_RULEBASED and
//! CLDR's ordinal and spell-out rules (with CLDR's ← → arrows or ICU's
//! < >). Rule sets, base values with radix and exponent, the -x / x.x /
//! 0.x / x.0 / Inf / NaN rules (the fraction rule matching the decimal
//! separator), bracketed text splitting a rule in two, the roll-back rule,
//! <<, >>, >>>, ==, <%set<, <%set<< (numerator with zeros), =#,##0=
//! substitutions, fraction rule sets and $(cardinal|ordinal,...)$ plurals,
//! with ICU's separate integer and double code paths.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::decfmt::{DecimalFormat, Dq, apply_pattern};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Normal,
    Negative,
    Improper,
    Proper,
    Master,
    Infinity,
    NaN,
}

#[derive(Clone, Debug)]
enum Target {
    /// The rule set owning the rule.
    Own,
    Set(usize),
    /// A DecimalFormat pattern.
    Pattern(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SubType {
    Multiplier,
    Modulus,
    SameValue,
    Absolute,
    Integral,
    Fractional,
    Numerator,
}

#[derive(Clone, Debug)]
struct Sub {
    /// '<', '>' or '='
    ch: char,
    target: Target,
    /// ">>>" (modulus: the preceding rule; fraction: no spaces)
    triple: bool,
    /// "<%set<<": the numerator with its leading zeros
    with_zeros: bool,
    /// ">>" / ">>>" / no rule set name (fraction digit by digit)
    plain: bool,
}

#[derive(Clone, Debug)]
enum Piece {
    Text(String),
    Sub(Sub),
    Plural {
        ordinal: bool,
        forms: Vec<(String, String)>,
    },
}

#[derive(Clone, Debug)]
struct Rule {
    base: i64,
    radix: i64,
    exponent: u32,
    kind: Kind,
    /// The decimal point of an x.x / 0.x / x.0 rule.
    decimal: char,
    pieces: Vec<Piece>,
    /// The rule before this one in its set (">>>").
    pred: Option<Box<Rule>>,
}

impl Rule {
    fn divisor(&self) -> i64 {
        let mut d: i64 = 1;
        for _ in 0..self.exponent {
            d = d.saturating_mul(self.radix);
        }
        d.max(1)
    }

    fn subs(&self) -> impl Iterator<Item = &Sub> {
        self.pieces.iter().filter_map(|p| match p {
            Piece::Sub(s) => Some(s),
            _ => None,
        })
    }

    /// NFRule::shouldRollBack()
    fn should_roll_back(&self, n: i64) -> bool {
        if self.subs().any(|s| s.ch == '>') {
            let re = self.divisor();
            return n % re == 0 && self.base % re != 0;
        }
        false
    }
}

#[derive(Clone, Debug, Default)]
struct RuleSet {
    name: String,
    rules: Vec<Rule>,
    negative: Option<Rule>,
    improper: Vec<Rule>,
    proper: Vec<Rule>,
    master: Vec<Rule>,
    infinity: Option<Rule>,
    nan: Option<Rule>,
    fraction: bool,
}

impl RuleSet {
    fn public(&self) -> bool {
        !self.name.starts_with("%%")
    }
}

#[derive(Debug, Default)]
pub struct Rbnf {
    sets: Vec<RuleSet>,
}

/// NFRule::expectedExponent()
fn expected_exponent(base: i64, radix: i64) -> u32 {
    if radix < 2 || base < 1 {
        return 0;
    }
    let mut e = ((base as f64).ln() / (radix as f64).ln()) as u32;
    let mut p: i64 = 1;
    for _ in 0..=e {
        p = p.saturating_mul(radix);
    }
    if p <= base {
        e += 1;
    }
    e
}

/// Split rule text into pieces: the substitutions (NFRule::
/// extractSubstitution(), RULE_PREFIXES), the plural and the text.
fn pieces(text: &str, set_index: &HashMap<String, usize>) -> Vec<Piece> {
    let c: Vec<char> = text.chars().collect();
    let mut out: Vec<Piece> = Vec::new();
    let mut lit = String::new();
    let mut i = 0;
    let mut nsubs = 0;
    let push_lit = |out: &mut Vec<Piece>, lit: &mut String| {
        if !lit.is_empty() {
            out.push(Piece::Text(std::mem::take(lit)));
        }
    };
    while i < c.len() {
        let ch = c[i];
        if ch == '$' && c.get(i + 1) == Some(&'(') {
            let rest: String = c[i + 2..].iter().collect();
            if let Some(e) = rest.find(")$") {
                let body = &rest[..e];
                if let Some((kind, forms)) = body.split_once(',') {
                    push_lit(&mut out, &mut lit);
                    let mut fs = Vec::new();
                    let mut r = forms;
                    while let Some(o) = r.find('{') {
                        let key = r[..o].trim().to_string();
                        let Some(cl) = r[o..].find('}') else { break };
                        fs.push((key, r[o + 1..o + cl].to_string()));
                        r = &r[o + cl + 1..];
                    }
                    out.push(Piece::Plural {
                        ordinal: kind.trim() == "ordinal",
                        forms: fs,
                    });
                    i += 2 + body.chars().count() + 2;
                    continue;
                }
            }
        }
        let is_prefix = matches!(ch, '<' | '>' | '=')
            && nsubs < 2
            && c.get(i + 1)
                .is_some_and(|n| *n == ch || matches!(n, '%' | '#' | '0'));
        if !is_prefix {
            lit.push(ch);
            i += 1;
            continue;
        }
        // the token ends at the next delimiter (">>>", "<%set<<" special)
        let Some(rel) = c[i + 1..].iter().position(|&x| x == ch) else {
            lit.push(ch);
            i += 1;
            continue;
        };
        let mut end = i + 1 + rel;
        let mut triple = false;
        let mut with_zeros = false;
        if ch == '>' && c.get(i + 1) == Some(&'>') && c.get(i + 2) == Some(&'>') {
            end = i + 2;
            triple = true;
        } else if ch == '<' && end > i + 1 && c.get(end + 1) == Some(&'<') {
            end += 1;
            with_zeros = true;
        }
        let inner: String = c[i + 1..if with_zeros { end - 1 } else { end }]
            .iter()
            .collect();
        let inner = if triple { String::new() } else { inner };
        let target = if inner.is_empty() {
            Target::Own
        } else if inner.starts_with('%') {
            match set_index.get(inner.trim()) {
                Some(&k) => Target::Set(k),
                None => Target::Own,
            }
        } else {
            Target::Pattern(inner.clone())
        };
        push_lit(&mut out, &mut lit);
        out.push(Piece::Sub(Sub {
            ch,
            plain: inner.is_empty(),
            target,
            triple,
            with_zeros,
        }));
        nsubs += 1;
        i = end + 1;
    }
    push_lit(&mut out, &mut lit);
    out
}

impl Rbnf {
    /// Parse rules (RuleBasedNumberFormat::init(), NFRuleSet::parseRules(),
    /// NFRule::makeRules()).
    pub fn parse(src: &str) -> Rbnf {
        // (CLDR writes the substitutions with arrows)
        let src: String = src
            .chars()
            .map(|c| match c {
                '\u{2190}' => '<',
                '\u{2192}' => '>',
                c => c,
            })
            .collect();
        // the rule sets and their rule descriptions
        let mut descs: Vec<(String, Vec<String>)> = Vec::new();
        for raw in src.split(';') {
            let mut r = raw.trim_start().to_string();
            if r.trim().is_empty() {
                continue;
            }
            if r.starts_with('%') {
                if let Some(i) = r.find(':') {
                    let name = r[..i].trim().to_string();
                    descs.push((name, Vec::new()));
                    r = r[i + 1..].trim_start().to_string();
                    if r.trim().is_empty() {
                        continue;
                    }
                }
            }
            if descs.is_empty() {
                descs.push((String::new(), Vec::new()));
            }
            descs.last_mut().unwrap().1.push(r);
        }
        let set_index: HashMap<String, usize> = descs
            .iter()
            .enumerate()
            .filter(|(_, (n, _))| !n.is_empty())
            .map(|(i, (n, _))| (n.clone(), i))
            .collect();
        let mut sets: Vec<RuleSet> = descs
            .iter()
            .map(|(n, _)| RuleSet {
                name: n.clone(),
                ..Default::default()
            })
            .collect();
        for (si, (_, rules)) in descs.iter().enumerate() {
            let mut prev_base: i64 = -1;
            let mut pred: Option<Rule> = None;
            for d in rules {
                let (desc, body) = match d.find(':') {
                    Some(i)
                        if !d[..i].contains(['<', '>', '=', '[', '\'', '$'])
                            || is_descriptor(&d[..i]) =>
                    {
                        (
                            Some(d[..i].trim().to_string()),
                            d[i + 1..].trim_start().to_string(),
                        )
                    }
                    _ => (None, d.clone()),
                };
                let body = body
                    .strip_prefix('\'')
                    .map(|s| s.to_string())
                    .unwrap_or(body);
                let mut rule = Rule {
                    base: 0,
                    radix: 10,
                    exponent: 0,
                    kind: Kind::Normal,
                    decimal: '.',
                    pieces: Vec::new(),
                    pred: None,
                };
                match desc.as_deref() {
                    None => {
                        prev_base += 1;
                        rule.base = prev_base;
                        rule.exponent = expected_exponent(rule.base, rule.radix);
                    }
                    Some("-x") => rule.kind = Kind::Negative,
                    Some("Inf") => rule.kind = Kind::Infinity,
                    Some("NaN") => rule.kind = Kind::NaN,
                    Some(x) if x.len() == 3 && x.starts_with('x') && x.ends_with('x') => {
                        rule.kind = Kind::Improper;
                        rule.decimal = x.chars().nth(1).unwrap();
                    }
                    Some(x) if x.len() == 3 && x.starts_with('0') && x.ends_with('x') => {
                        rule.kind = Kind::Proper;
                        rule.decimal = x.chars().nth(1).unwrap();
                    }
                    Some(x) if x.len() == 3 && x.starts_with('x') && x.ends_with('0') => {
                        rule.kind = Kind::Master;
                        rule.decimal = x.chars().nth(1).unwrap();
                    }
                    Some(x) => {
                        let (b, rest) = match x.split_once('/') {
                            Some((b, r)) => (b, Some(r)),
                            None => (x, None),
                        };
                        let digits: String = b.chars().filter(|c| c.is_ascii_digit()).collect();
                        rule.base = digits.parse().unwrap_or(0);
                        prev_base = rule.base;
                        let mut gts = b.chars().filter(|&c| c == '>').count() as u32;
                        if let Some(r) = rest {
                            let rd: String = r.chars().filter(|c| c.is_ascii_digit()).collect();
                            rule.radix = rd.parse::<i64>().unwrap_or(10).max(2);
                            gts += r.chars().filter(|&c| c == '>').count() as u32;
                        }
                        rule.exponent =
                            expected_exponent(rule.base, rule.radix).saturating_sub(gts);
                    }
                }
                // bracketed text: two rules (NFRule::makeRules())
                let b1 = body.find('[');
                let b2 = body.find(']');
                let mut made: Vec<Rule> = Vec::new();
                match (b1, b2) {
                    (Some(a), Some(b))
                        if a < b
                            && !matches!(
                                rule.kind,
                                Kind::Proper | Kind::Negative | Kind::Infinity | Kind::NaN
                            ) =>
                    {
                        let with: String =
                            format!("{}{}{}", &body[..a], &body[a + 1..b], &body[b + 1..]);
                        let without: String = format!("{}{}", &body[..a], &body[b + 1..]);
                        let split = (rule.kind == Kind::Normal
                            && rule.base > 0
                            && rule.base % rule.divisor() == 0)
                            || matches!(rule.kind, Kind::Improper | Kind::Master);
                        if split {
                            let mut r2 = rule.clone();
                            let mut r1 = rule.clone();
                            match rule.kind {
                                Kind::Normal => r1.base += 1,
                                Kind::Improper => r2.kind = Kind::Proper,
                                Kind::Master => r1.kind = Kind::Improper,
                                _ => {}
                            }
                            r2.pieces = pieces(&without, &set_index);
                            r1.pieces = pieces(&with, &set_index);
                            made.push(r2);
                            made.push(r1);
                        } else {
                            rule.pieces = pieces(&with, &set_index);
                            made.push(rule);
                        }
                    }
                    _ => {
                        rule.pieces = pieces(&body, &set_index);
                        made.push(rule);
                    }
                }
                let p = pred.clone();
                for mut r in made {
                    r.pred = p.clone().map(Box::new);
                    let set = &mut sets[si];
                    match r.kind {
                        Kind::Normal => {
                            pred = Some(Rule {
                                pred: None,
                                ..r.clone()
                            });
                            set.rules.push(r);
                        }
                        Kind::Negative => set.negative = Some(r),
                        Kind::Improper => set.improper.push(r),
                        Kind::Proper => set.proper.push(r),
                        Kind::Master => set.master.push(r),
                        Kind::Infinity => set.infinity = Some(r),
                        Kind::NaN => set.nan = Some(r),
                    }
                }
            }
        }
        // a rule set a fraction rule names becomes a fraction rule set
        // (FractionalPartSubstitution: makeIntoFractionRuleSet())
        let mut frac: Vec<usize> = Vec::new();
        for s in &sets {
            for r in s.improper.iter().chain(&s.proper).chain(&s.master) {
                for sub in r.subs() {
                    if sub.ch == '>' && !sub.plain {
                        if let Target::Set(k) = sub.target {
                            frac.push(k);
                        }
                    }
                }
            }
        }
        for k in frac {
            sets[k].fraction = true;
        }
        Rbnf { sets }
    }

    /// RuleBasedNumberFormat::initDefaultRuleSet()
    fn default_set(&self) -> usize {
        for (i, s) in self.sets.iter().enumerate() {
            if matches!(
                s.name.as_str(),
                "%spellout-numbering" | "%digits-ordinal" | "%duration"
            ) {
                return i;
            }
        }
        let last = self.sets.len().saturating_sub(1);
        if self.sets.get(last).is_some_and(|s| s.public()) {
            return last;
        }
        (0..last)
            .rev()
            .find(|&i| self.sets[i].public())
            .unwrap_or(last)
    }
}

struct Ctx<'a> {
    r: &'a Rbnf,
    df: &'a DecimalFormat,
    lang: &'a str,
    depth: u32,
}

/// uprv_round()
fn round_half_up(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// util64_fromDouble()
fn to_i64(x: f64) -> i64 {
    if x.is_nan() { 0 } else { x as i64 }
}

impl Ctx<'_> {
    fn pick<'b>(&self, list: &'b [Rule]) -> Option<&'b Rule> {
        // (the one with the locale's decimal separator, else the first)
        let dec = self.df.symbols[crate::numdata::SYM_DECIMAL]
            .chars()
            .next()
            .unwrap_or('.');
        list.iter().find(|r| r.decimal == dec).or(list.first())
    }

    /// NFRuleSet::findNormalRule()
    fn find_normal(&self, set: usize, n: i64) -> Option<&Rule> {
        let s = &self.r.sets[set];
        if s.fraction {
            return self.find_fraction(set, n as f64);
        }
        let mut n = n;
        if n < 0 {
            if let Some(r) = &s.negative {
                return Some(r);
            }
            n = n.saturating_neg();
        }
        if !s.rules.is_empty() {
            let hi = s.rules.partition_point(|r| r.base <= n);
            if hi == 0 {
                return s.rules.first();
            }
            let r = &s.rules[hi - 1];
            if r.should_roll_back(n) {
                if hi == 1 {
                    return None;
                }
                return Some(&s.rules[hi - 2]);
            }
            return Some(r);
        }
        self.pick(&s.master)
    }

    /// NFRuleSet::findFractionRuleSetRule(): the closest denominator, in
    /// ICU's (wrapping) 64-bit arithmetic.
    fn find_fraction(&self, set: usize, x: f64) -> Option<&Rule> {
        let rules = &self.r.sets[set].rules;
        let first = rules.first()?;
        let gcd = |mut a: i64, mut b: i64| {
            while b != 0 {
                let t = a % b;
                a = b;
                b = t;
            }
            a
        };
        let mut lcm = first.base.max(1);
        for r in &rules[1..] {
            let b = r.base.max(1);
            lcm = (lcm / gcd(lcm, b)).wrapping_mul(b);
        }
        let numerator = to_i64(x * lcm as f64 + 0.5);
        let mut difference = i64::MAX;
        let mut winner = 0;
        for (i, r) in rules.iter().enumerate() {
            let mut t = numerator.wrapping_mul(r.base) % lcm.max(1);
            if lcm.wrapping_sub(t) < t {
                t = lcm.wrapping_sub(t);
            }
            if t < difference {
                difference = t;
                winner = i;
                if difference == 0 {
                    break;
                }
            }
        }
        if winner + 1 < rules.len() && rules[winner + 1].base == rules[winner].base {
            let n = rules[winner].base as f64 * x;
            if !(0.5..2.0).contains(&n) {
                winner += 1;
            }
        }
        Some(&rules[winner])
    }

    /// NFRuleSet::findDoubleRule()
    fn find_double(&self, set: usize, x: f64) -> Option<&Rule> {
        let s = &self.r.sets[set];
        if s.fraction {
            return self.find_fraction(set, x);
        }
        if x.is_nan() {
            return s.nan.as_ref();
        }
        let mut x = x;
        if x < 0.0 {
            if let Some(r) = &s.negative {
                return Some(r);
            }
            x = -x;
        }
        if x.is_infinite() {
            return s.infinity.as_ref();
        }
        if x != x.floor() {
            if x < 1.0 {
                if let Some(r) = self.pick(&s.proper) {
                    return Some(r);
                }
            }
            if let Some(r) = self.pick(&s.improper) {
                return Some(r);
            }
        }
        if let Some(r) = self.pick(&s.master) {
            return Some(r);
        }
        self.find_normal(set, to_i64(x + 0.5))
    }

    fn format_i64(&mut self, set: usize, n: i64, out: &mut String) {
        if self.depth > 64 {
            return;
        }
        self.depth += 1;
        if let Some(rule) = self.find_normal(set, n) {
            let rule = rule.clone();
            self.do_format_i64(&rule, set, n, out);
        }
        self.depth -= 1;
    }

    fn format_f64(&mut self, set: usize, x: f64, out: &mut String) {
        if self.depth > 64 {
            return;
        }
        self.depth += 1;
        match self.find_double(set, x).cloned() {
            Some(rule) => self.do_format_f64(&rule, set, x, out),
            None if x.is_nan() || x.is_infinite() => {
                // (the default NaN / infinity rules: the symbols)
                if x.is_nan() {
                    out.push_str(&self.df.symbols[crate::numdata::SYM_NAN]);
                } else {
                    out.push_str(&self.df.symbols[crate::numdata::SYM_INFINITY]);
                }
            }
            None => {}
        }
        self.depth -= 1;
    }

    fn plural(&self, ordinal: bool, forms: &[(String, String)], v: i32, out: &mut String) {
        let ops = intl::plural::PluralOperands::from_int(v as i64);
        use intl::plural::PluralCategory as C;
        let cat = if ordinal {
            intl::plural::ordinal_category(self.lang, &ops)
        } else {
            intl::plural::plural_category(self.lang, &ops)
        };
        let want = match cat {
            C::Zero => "zero",
            C::One => "one",
            C::Two => "two",
            C::Few => "few",
            C::Many => "many",
            C::Other => "other",
        };
        let t = forms
            .iter()
            .find(|(k, _)| k == want)
            .or_else(|| forms.iter().find(|(k, _)| k == "other"));
        if let Some((_, t)) = t {
            out.push_str(t);
        }
    }

    fn sub_type(&self, rule: &Rule, set: usize, s: &Sub) -> SubType {
        match s.ch {
            '<' => match rule.kind {
                Kind::Improper | Kind::Proper | Kind::Master => SubType::Integral,
                _ if self.r.sets[set].fraction => SubType::Numerator,
                _ => SubType::Multiplier,
            },
            '>' => match rule.kind {
                Kind::Negative => SubType::Absolute,
                Kind::Improper | Kind::Proper | Kind::Master => SubType::Fractional,
                _ => SubType::Modulus,
            },
            _ => SubType::SameValue,
        }
    }

    fn target_set(&self, s: &Sub, own: usize) -> Option<usize> {
        match s.target {
            Target::Own => Some(own),
            Target::Set(k) => Some(k),
            Target::Pattern(_) => None,
        }
    }

    fn pattern_format(&self, s: &Sub, x: f64, floor_if_int: bool) -> String {
        let Target::Pattern(p) = &s.target else {
            return String::new();
        };
        let mut df = self.df.clone();
        if apply_pattern(&mut df.props, p, 0).is_err() {
            return String::new();
        }
        let x = if floor_if_int && df.props.max_frac == 0 {
            x.floor()
        } else {
            x
        };
        df.format_dq(Dq::from_f64(x), None).unwrap_or_default()
    }

    /// NFRule::doFormat(int64_t)
    fn do_format_i64(&mut self, rule: &Rule, set: usize, n: i64, out: &mut String) {
        for p in &rule.pieces {
            match p {
                Piece::Text(t) => out.push_str(t),
                Piece::Plural { ordinal, forms } => {
                    let v = n / rule.divisor();
                    self.plural(*ordinal, forms, v as i32, out);
                }
                Piece::Sub(s) => self.sub_i64(rule, set, s, n, out),
            }
        }
    }

    /// NFRule::doFormat(double)
    fn do_format_f64(&mut self, rule: &Rule, set: usize, x: f64, out: &mut String) {
        for p in &rule.pieces {
            match p {
                Piece::Text(t) => out.push_str(t),
                Piece::Plural { ordinal, forms } => {
                    let d = rule.divisor() as f64;
                    let v = if (0.0..1.0).contains(&x) {
                        round_half_up(x * d)
                    } else {
                        x / d
                    };
                    self.plural(*ordinal, forms, v as i32, out);
                }
                Piece::Sub(s) => self.sub_f64(rule, set, s, x, out),
            }
        }
    }

    /// NFSubstitution::doSubstitution(int64_t)
    fn sub_i64(&mut self, rule: &Rule, set: usize, s: &Sub, n: i64, out: &mut String) {
        let ty = self.sub_type(rule, set, s);
        let div = rule.divisor();
        let v = match ty {
            SubType::Multiplier => n / div,
            SubType::Modulus => n % div,
            SubType::SameValue | SubType::Integral => n,
            SubType::Absolute => n.saturating_abs(),
            SubType::Fractional => 0,
            SubType::Numerator => return self.sub_f64(rule, set, s, n as f64, out),
        };
        if ty == SubType::Modulus && s.triple {
            if let Some(p) = &rule.pred {
                let p = (**p).clone();
                self.do_format_i64(&p, set, v, out);
            }
            return;
        }
        match self.target_set(s, set) {
            Some(k) => self.format_i64(k, v, out),
            None => {
                // (a DecimalFormat: on the transformed double)
                let x = match ty {
                    SubType::Multiplier => (n as f64 / div as f64).floor(),
                    SubType::Modulus => (n as f64) % div as f64,
                    _ => v as f64,
                };
                out.push_str(&self.pattern_format(s, x, true));
            }
        }
    }

    /// NFSubstitution::doSubstitution(double)
    fn sub_f64(&mut self, rule: &Rule, set: usize, s: &Sub, x: f64, out: &mut String) {
        let ty = self.sub_type(rule, set, s);
        let div = rule.divisor() as f64;
        if ty == SubType::Fractional && (s.plain || matches!(s.target, Target::Own)) {
            // FractionalPartSubstitution by digits
            let k = self.target_set(s, set).unwrap_or(set);
            let q = Dq::from_f64(x);
            let mut first = true;
            let mut m = -1;
            let mut digits: Vec<i64> = Vec::new();
            while m >= q.lower() {
                digits.push(q.digit(m) as i64);
                m -= 1;
            }
            for d in digits {
                if !first && !s.triple {
                    out.push(' ');
                }
                first = false;
                self.format_i64(k, d, out);
            }
            if first {
                self.format_i64(k, 0, out);
            }
            return;
        }
        let v = match ty {
            SubType::Multiplier => (x / div).floor(),
            SubType::Modulus => x % div,
            SubType::SameValue => x,
            SubType::Absolute => x.abs(),
            SubType::Integral => x.floor(),
            SubType::Fractional => x - x.floor(),
            SubType::Numerator => round_half_up(x * rule.base as f64),
        };
        if ty == SubType::Modulus && s.triple {
            if let Some(p) = &rule.pred {
                let p = (**p).clone();
                self.do_format_f64(&p, set, v, out);
            }
            return;
        }
        if ty == SubType::Numerator && s.with_zeros {
            if let Some(k) = self.target_set(s, set) {
                let mut nf = to_i64(v);
                while {
                    nf = nf.saturating_mul(10);
                    nf < rule.base
                } {
                    self.format_i64(k, 0, out);
                    out.push(' ');
                }
            }
        }
        if v.is_infinite() {
            if let Some(k) = self.target_set(s, set) {
                if let Some(r) = self.find_double(k, f64::INFINITY).cloned() {
                    self.do_format_f64(&r, k, v, out);
                }
            }
            return;
        }
        match self.target_set(s, set) {
            Some(k) if v == v.floor() && v.abs() < 9.2e18 => self.format_i64(k, to_i64(v), out),
            Some(k) => self.format_f64(k, v, out),
            None => out.push_str(&self.pattern_format(s, v, false)),
        }
    }
}

thread_local! {
    static CACHE: RefCell<HashMap<String, Rc<Rbnf>>> = RefCell::new(HashMap::new());
}

/// The parsed rules (cached per thread).
fn parsed(rules: &str) -> Rc<Rbnf> {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(r) = c.get(rules) {
            return r.clone();
        }
        let r = Rc::new(Rbnf::parse(rules));
        if c.len() > 64 {
            c.clear();
        }
        c.insert(rules.to_string(), r.clone());
        r
    })
}

/// The public rule set names of `rules`, in order.
pub fn public_sets(rules: &str) -> Vec<String> {
    parsed(rules)
        .sets
        .iter()
        .filter(|s| s.public() && !s.name.is_empty())
        .map(|s| s.name.clone())
        .collect()
}

/// The default rule set's name (RuleBasedNumberFormat::
/// getDefaultRuleSetName(): empty for a private one).
pub fn default_set_name(rules: &str) -> String {
    let r = parsed(rules);
    match r.sets.get(r.default_set()) {
        Some(s) if s.public() => s.name.clone(),
        _ => String::new(),
    }
}

/// Format `q` with custom rules (the default rule set).
pub fn format(rules: &str, q: &Dq, df: &DecimalFormat) -> String {
    format_with(rules, None, q, df, "en")
}

/// Format `q` with `rules`, starting with the rule set named `start` (the
/// default one when None), plural selections in `lang`.
pub fn format_with(
    rules: &str,
    start: Option<&str>,
    q: &Dq,
    df: &DecimalFormat,
    lang: &str,
) -> String {
    let r = parsed(rules);
    if r.sets.is_empty() {
        return df.format_dq(q.clone(), None).unwrap_or_default();
    }
    let set = start
        .and_then(|n| r.sets.iter().position(|s| s.name == n))
        .unwrap_or_else(|| r.default_set());
    let mut ctx = Ctx {
        r: &r,
        df,
        lang,
        depth: 0,
    };
    let mut out = String::new();
    // (an integral value takes the int64 path, as format(int64_t))
    match q.to_i64() {
        Some(n) if q.is_finite() && q.lower() >= 0 => ctx.format_i64(set, n, &mut out),
        _ => ctx.format_f64(set, q.to_f64(), &mut out),
    }
    out
}

/// Whether `d` is a rule descriptor (a base value, -x, x.x...).
fn is_descriptor(d: &str) -> bool {
    let d = d.trim();
    matches!(
        d,
        "-x" | "x.x" | "x,x" | "0.x" | "0,x" | "x.0" | "x,0" | "Inf" | "NaN"
    ) || (!d.is_empty()
        && d.chars()
            .all(|c| c.is_ascii_digit() || matches!(c, ',' | '.' | ' ' | '/' | '>')))
}

/// The locale's CLDR rules in `table`, by locale fallback.
fn locale_rules(table: &'static [(&'static str, &'static str)], id: &str) -> Option<&'static str> {
    let find = |l: &str| {
        table
            .binary_search_by(|(k, _)| (*k).cmp(l))
            .ok()
            .map(|i| table[i].1)
    };
    let mut cur = crate::numdata::base_name(id).replace('-', "_");
    // (the zh_TW alias)
    let lang = crate::numdata::language(&cur);
    let region = cur.rsplit('_').next().unwrap_or("").to_string();
    if lang == "zh" && matches!(region.as_str(), "TW" | "HK" | "MO") && !cur.contains("Hant") {
        cur = "zh_Hant".to_string();
    }
    for _ in 0..16 {
        if let Some(r) = find(&cur) {
            return Some(r);
        }
        // CLDR's parentLocales, else the truncation
        use crate::parent_locales::PARENT_LOCALES;
        if let Ok(i) = PARENT_LOCALES.binary_search_by(|(k, _)| (*k).cmp(cur.as_str())) {
            cur = PARENT_LOCALES[i].1.to_string();
            if cur == "root" {
                break;
            }
            continue;
        }
        match cur.rfind('_') {
            Some(p) => {
                cur.truncate(p);
                while cur.ends_with('_') {
                    cur.pop();
                }
            }
            None => break,
        }
    }
    find("root")
}

/// CLDR's %digits-ordinal rules for an ICU locale ID, by locale fallback
/// (root's when no ancestor has any).
pub fn digits_ordinal_rules(id: &str) -> &'static str {
    locale_rules(crate::rbnf_ordinal::DIGITS_ORDINAL, id)
        .unwrap_or("%digits-ordinal: -x: \u{2212}>>; 0: =#,##0=.;")
}

/// CLDR's spell-out rules for an ICU locale ID, by locale fallback.
pub fn spellout_rules(id: &str) -> Option<&'static str> {
    locale_rules(crate::rbnf_spellout::SPELLOUT, id)
}
