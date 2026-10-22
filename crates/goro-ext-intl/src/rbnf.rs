//! A small RuleBasedNumberFormat for NumberFormatter::PATTERN_RULEBASED
//! (custom rule sets): rule sets, base values with radix, `<<`, `>>`,
//! `==` substitutions (to rule sets or DecimalFormat patterns), optional
//! `[...]` text, the `-x`, `x.x` and `0.x` rules.

use crate::decfmt::{DecimalFormat, Dq, ROUND_DOWN, apply_pattern};

#[derive(Clone, Debug)]
struct Rule {
    /// Some(base) for a normal rule; None for special ones.
    base: Option<i64>,
    special: Option<String>,
    radix: i64,
    text: String,
}

#[derive(Clone, Debug)]
struct RuleSet {
    name: String,
    rules: Vec<Rule>,
}

fn parse(rules: &str) -> Vec<RuleSet> {
    let mut sets: Vec<RuleSet> = Vec::new();
    let mut cur = RuleSet {
        name: String::new(),
        rules: Vec::new(),
    };
    let mut prev_base: i64 = -1;
    for raw in rules.split(';') {
        let mut r = raw.trim_start().to_string();
        if r.trim().is_empty() {
            continue;
        }
        // a rule set name
        if r.starts_with('%') {
            if let Some(i) = r.find(':') {
                let name = r[..i].trim().to_string();
                if !cur.rules.is_empty() || !cur.name.is_empty() {
                    sets.push(cur);
                }
                cur = RuleSet {
                    name,
                    rules: Vec::new(),
                };
                prev_base = -1;
                r = r[i + 1..].trim_start().to_string();
                if r.is_empty() {
                    continue;
                }
            }
        }
        let (desc, body) = match r.find(':') {
            Some(i) if !r[..i].contains(['<', '>', '=', '[', '\'']) => (
                Some(r[..i].trim().to_string()),
                r[i + 1..].trim_start().to_string(),
            ),
            _ => (None, r.clone()),
        };
        let body = body
            .strip_prefix('\'')
            .map(|s| s.to_string())
            .unwrap_or(body);
        let mut rule = Rule {
            base: None,
            special: None,
            radix: 10,
            text: body,
        };
        match desc {
            None => {
                prev_base += 1;
                rule.base = Some(prev_base);
            }
            Some(d) => {
                let (b, rad) = match d.split_once('/') {
                    Some((b, r)) => (b.to_string(), r.trim().parse::<i64>().unwrap_or(10).max(2)),
                    None => (d.clone(), 10),
                };
                let b = b.trim_end_matches('>').replace([',', ' ', '.'], "");
                if let Ok(n) = b.parse::<i64>() {
                    rule.base = Some(n);
                    rule.radix = rad;
                    prev_base = n;
                } else {
                    rule.special = Some(d.clone());
                }
            }
        }
        cur.rules.push(rule);
    }
    if !cur.rules.is_empty() || !cur.name.is_empty() {
        sets.push(cur);
    }
    sets
}

fn divisor(base: i64, radix: i64) -> i64 {
    let mut p = 1i64;
    while p <= base / radix {
        p *= radix;
    }
    p
}

struct Ctx<'a> {
    sets: &'a [RuleSet],
    df: &'a DecimalFormat,
    budget: u32,
}

impl Ctx<'_> {
    fn set_index(&self, name: &str) -> Option<usize> {
        self.sets.iter().position(|s| s.name == name)
    }

    fn sub_value(&mut self, tok: &str, set: usize, q: &Dq, depth: u32) -> String {
        // tok is the content between the delimiters
        if tok.is_empty() {
            return self.fmt(set, q, depth + 1);
        }
        if tok.starts_with('%') {
            return match self.set_index(tok) {
                Some(i) => self.fmt(i, q, depth + 1),
                None => String::new(),
            };
        }
        // a DecimalFormat pattern
        let mut df = self.df.clone();
        if apply_pattern(&mut df.props, tok, 0).is_err() {
            return String::new();
        }
        df.format_dq(q.clone(), None).unwrap_or_default()
    }

    fn render(
        &mut self,
        text: &str,
        set: usize,
        q: &Dq,
        quot: &Dq,
        rem: &Dq,
        depth: u32,
        rem_zero: bool,
    ) -> String {
        let cs: Vec<char> = text.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        let mut skipping = false;
        while i < cs.len() {
            let c = cs[i];
            match c {
                '[' => {
                    skipping = rem_zero;
                    i += 1;
                }
                ']' => {
                    skipping = false;
                    i += 1;
                }
                _ if skipping => i += 1,
                '<' | '>' | '=' | '←' | '→' => {
                    let delim = c;
                    let mut j = i + 1;
                    let mut tok = String::new();
                    while j < cs.len() && cs[j] != delim {
                        tok.push(cs[j]);
                        j += 1;
                    }
                    if j >= cs.len() {
                        out.push(c);
                        i += 1;
                        continue;
                    }
                    // `>>>`
                    let mut k = j + 1;
                    if delim == '>' && k < cs.len() && cs[k] == '>' {
                        k += 1;
                    }
                    let v = match delim {
                        '<' | '←' => quot,
                        '>' | '→' => rem,
                        _ => q,
                    };
                    out.push_str(&self.sub_value(&tok, set, v, depth));
                    i = k;
                }
                _ => {
                    out.push(c);
                    i += 1;
                }
            }
        }
        out
    }

    fn fmt(&mut self, set: usize, q: &Dq, depth: u32) -> String {
        if depth > 64 || self.budget == 0 {
            return String::new();
        }
        self.budget -= 1;
        let rs = &self.sets[set];
        if q.neg && !q.is_zero() {
            if let Some(r) = rs.rules.iter().find(|r| r.special.as_deref() == Some("-x")) {
                let mut a = q.clone();
                a.neg = false;
                let text = r.text.clone();
                return self.render(&text, set, q, &a, &a, depth, false);
            }
            let mut a = q.clone();
            a.neg = false;
            return format!("-{}", self.fmt(set, &a, depth + 1));
        }
        if q.lower() < 0 {
            let mut ip = q.clone();
            let _ = ip.round_at(0, ROUND_DOWN);
            let special = if ip.is_zero() {
                ["0.x", "x.x"]
            } else {
                ["x.x", "x.x"]
            };
            for sp in special {
                if let Some(r) = rs.rules.iter().find(|r| r.special.as_deref() == Some(sp)) {
                    let text = r.text.clone();
                    // the fraction part, digit by digit through the rule set
                    let mut frac = String::new();
                    let mut m = -1;
                    while m >= q.lower() {
                        if !frac.is_empty() {
                            frac.push(' ');
                        }
                        frac.push_str(&self.fmt(set, &Dq::from_i64(q.digit(m) as i64), depth + 1));
                        m -= 1;
                    }
                    let ipart = self.fmt(set, &ip, depth + 1);
                    return render_fraction(&text, &ipart, &frac);
                }
            }
        }
        let mut ip = q.clone();
        let _ = ip.round_at(0, ROUND_DOWN);
        let n = ip.to_i64().unwrap_or(i64::MAX);
        let rule = rs
            .rules
            .iter()
            .filter(|r| r.base.is_some_and(|b| b <= n))
            .max_by_key(|r| r.base.unwrap_or(0))
            .cloned();
        let Some(rule) = rule else {
            return self.df.format_dq(q.clone(), None).unwrap_or_default();
        };
        let base = rule.base.unwrap_or(0);
        let div = if base > 0 {
            divisor(base, rule.radix)
        } else {
            1
        };
        let quot = Dq::from_i64(n / div);
        let rem = Dq::from_i64(n % div);
        self.render(&rule.text, set, q, &quot, &rem, depth, n % div == 0)
    }
}

fn render_fraction(text: &str, ipart: &str, frac: &str) -> String {
    let mut out = String::new();
    let cs: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if matches!(c, '<' | '>' | '=') {
            let mut j = i + 1;
            while j < cs.len() && cs[j] != c {
                j += 1;
            }
            out.push_str(if c == '<' { ipart } else { frac });
            i = j + 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Format `q` with custom rules.
pub fn format(rules: &str, q: &Dq, df: &DecimalFormat) -> String {
    let sets = parse(rules);
    if sets.is_empty() {
        return df.format_dq(q.clone(), None).unwrap_or_default();
    }
    // the default rule set: the last public one... ICU uses the first public
    let start = sets
        .iter()
        .position(|s| !s.name.starts_with("%%"))
        .unwrap_or(0);
    let mut ctx = Ctx {
        sets: &sets,
        df,
        budget: 10_000,
    };
    ctx.fmt(start, q, 0)
}
