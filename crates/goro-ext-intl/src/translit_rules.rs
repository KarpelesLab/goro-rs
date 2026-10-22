//! ICU transliteration rules (the RuleBasedTransliterator syntax):
//! `a > b;`, `a < b;`, `a <> b;`, contexts `x { a } y`, the `|` cursor,
//! `^` / `$` anchors, variables, UnicodeSets with `*` `+` `?`, quoting and
//! escapes, `::ID;` steps. Rules are tried in order at each position; the
//! first match is replaced (ICU's semantics, hence its masking check).

use std::collections::HashMap;
use std::rc::Rc;

use crate::uset::{self, USet};

pub const U_MALFORMED_RULE: i32 = 0x10001;
pub const U_MALFORMED_SET: i32 = 0x10002;
pub const U_MALFORMED_VARIABLE_DEFINITION: i32 = 0x10005;
pub const U_MALFORMED_VARIABLE_REFERENCE: i32 = 0x10006;
pub const U_MISSING_OPERATOR: i32 = 0x1000B;
pub const U_UNDEFINED_VARIABLE: i32 = 0x10012;
pub const U_UNQUOTED_SPECIAL: i32 = 0x10013;
pub const U_UNTERMINATED_QUOTE: i32 = 0x10014;
pub const U_RULE_MASK_ERROR: i32 = 0x10015;
pub const U_INVALID_ID: i32 = 0x10021;

#[derive(Clone, Debug)]
pub enum Elem {
    Char(char),
    /// A set (index into the rule set's sets) and its pattern text.
    Set(usize),
    Quant(Box<Elem>, u32, u32),
}

#[derive(Clone, Debug, Default)]
pub struct Rule {
    pub ante: Vec<Elem>,
    pub key: Vec<Elem>,
    pub post: Vec<Elem>,
    pub out: Vec<char>,
    /// Cursor position in the output (chars), None = after it.
    pub cursor: Option<usize>,
    pub anchor_start: bool,
    pub anchor_end: bool,
}

/// One step of a rule-based transliterator.
#[derive(Debug)]
pub enum Step {
    Rules(Vec<Rule>),
    /// A `::ID;` step (the ID text).
    Id(String),
}

#[derive(Debug)]
pub struct RuleSet {
    pub sets: Vec<(USet, String)>,
    pub steps: Vec<Step>,
    /// A global `::[filter];`.
    pub filter: Option<USet>,
}

/// A parse error: status, offset (-1: none), pre/post context.
#[derive(Debug, Clone)]
pub struct ParseError {
    pub code: i32,
    pub offset: i32,
    pub pre: String,
    pub post: String,
}

fn is_ws(c: char) -> bool {
    matches!(
        c as u32,
        0x09..=0x0D | 0x20 | 0x85 | 0x200E | 0x200F | 0x2028 | 0x2029
    )
}

/// UTF-16 length of chars.
fn u16len(c: &[char]) -> usize {
    c.iter().map(|c| c.len_utf16()).sum()
}

/// Up to 15 UTF-16 units of `c`, from the start or (pre) the end.
fn ctx(c: &[char], from_end: bool) -> String {
    let mut out: Vec<char> = Vec::new();
    let mut n = 0;
    if from_end {
        for &ch in c.iter().rev() {
            if n + ch.len_utf16() > 15 {
                break;
            }
            n += ch.len_utf16();
            out.insert(0, ch);
        }
    } else {
        for &ch in c {
            if n + ch.len_utf16() > 15 {
                break;
            }
            n += ch.len_utf16();
            out.push(ch);
        }
    }
    out.into_iter().collect()
}

struct P<'a> {
    c: &'a [char],
    vars: HashMap<String, Vec<Elem>>,
    var_out: HashMap<String, Vec<char>>,
    sets: Vec<(USet, String)>,
}

/// One side of a rule, parsed.
#[derive(Default)]
struct Side {
    ante: Vec<Elem>,
    key: Vec<Elem>,
    post: Vec<Elem>,
    /// the literal text (for output use)
    text: Vec<char>,
    cursor: Option<usize>,
    anchor_start: bool,
    anchor_end: bool,
    has_set: bool,
}

impl P<'_> {
    fn syntax(&self, code: i32, start: usize) -> ParseError {
        // TransliteratorParser::syntaxError(): offset of the rule start,
        // 15 units of context either side
        let off = u16len(&self.c[..start]) as i32;
        ParseError {
            code,
            offset: off,
            pre: ctx(&self.c[..start], true),
            post: ctx(&self.c[start..], false),
        }
    }

    /// Parse one side between `a` and `b`.
    fn side(&mut self, a: usize, b: usize, rule_start: usize) -> Result<Side, ParseError> {
        let mut s = Side::default();
        // 0 = ante/key (before '{'), 1 = key, 2 = post
        let mut cur: Vec<Elem> = Vec::new();
        let mut seen_open = false;
        let mut seen_close = false;
        let mut i = a;
        let mut first = true;
        while i < b {
            let ch = self.c[i];
            if is_ws(ch) {
                i += 1;
                continue;
            }
            match ch {
                '{' => {
                    if seen_open {
                        return Err(self.syntax(U_MALFORMED_RULE, rule_start));
                    }
                    seen_open = true;
                    s.ante = std::mem::take(&mut cur);
                    i += 1;
                }
                '}' => {
                    if seen_close {
                        return Err(self.syntax(U_MALFORMED_RULE, rule_start));
                    }
                    seen_close = true;
                    s.key = std::mem::take(&mut cur);
                    i += 1;
                }
                '|' => {
                    s.cursor = Some(s.text.len());
                    i += 1;
                }
                '^' if first => {
                    s.anchor_start = true;
                    i += 1;
                }
                '\'' => {
                    i += 1;
                    let st = i;
                    if self.c.get(i) == Some(&'\'') {
                        cur.push(Elem::Char('\''));
                        s.text.push('\'');
                        i += 1;
                    } else {
                        loop {
                            if i >= b {
                                return Err(self.syntax(U_UNTERMINATED_QUOTE, rule_start));
                            }
                            if self.c[i] == '\'' {
                                if self.c.get(i + 1) == Some(&'\'') && i + 1 < b {
                                    cur.push(Elem::Char('\''));
                                    s.text.push('\'');
                                    i += 2;
                                    continue;
                                }
                                i += 1;
                                break;
                            }
                            cur.push(Elem::Char(self.c[i]));
                            s.text.push(self.c[i]);
                            i += 1;
                        }
                        let _ = st;
                    }
                }
                '\\' if !uset::starts_set(self.c, i) => {
                    i += 1;
                    let cp = uset::parse_escape(self.c, &mut i)
                        .ok_or_else(|| self.syntax(U_MALFORMED_RULE, rule_start))?;
                    let c = char::from_u32(cp).unwrap_or('\u{FFFD}');
                    cur.push(Elem::Char(c));
                    s.text.push(c);
                }
                '[' | '\\' => {
                    let st = i;
                    let sets = &self.sets;
                    let vars = &self.vars;
                    let lookup = |n: &str| -> Option<USet> {
                        match vars.get(n)?.as_slice() {
                            [Elem::Set(k)] => Some(sets[*k].0.clone()),
                            [Elem::Char(c)] => Some(USet::Ranges(vec![(*c as u32, *c as u32)])),
                            _ => None,
                        }
                    };
                    let set = uset::parse(self.c, &mut i, &lookup)
                        .map_err(|_| self.syntax(U_MALFORMED_SET, rule_start))?;
                    let txt: String = self.c[st..i].iter().collect();
                    self.sets.push((set, txt));
                    cur.push(Elem::Set(self.sets.len() - 1));
                    s.has_set = true;
                }
                '$' => {
                    i += 1;
                    let st = i;
                    while i < b && (self.c[i].is_alphanumeric() || self.c[i] == '_') {
                        i += 1;
                    }
                    if st == i {
                        // end anchor
                        s.anchor_end = true;
                        continue;
                    }
                    let name: String = self.c[st..i].iter().collect();
                    match self.vars.get(&name) {
                        Some(v) => {
                            cur.extend(v.iter().cloned());
                            if let Some(t) = self.var_out.get(&name) {
                                s.text.extend(t.iter().copied());
                            }
                            if v.iter().any(|e| !matches!(e, Elem::Char(_))) {
                                s.has_set = true;
                            }
                        }
                        None => return Err(self.syntax(U_UNDEFINED_VARIABLE, rule_start)),
                    }
                }
                '*' | '+' | '?' => {
                    let Some(last) = cur.pop() else {
                        return Err(self.syntax(U_MALFORMED_RULE, rule_start));
                    };
                    let (mn, mx) = match ch {
                        '*' => (0, u32::MAX),
                        '+' => (1, u32::MAX),
                        _ => (0, 1),
                    };
                    cur.push(Elem::Quant(Box::new(last), mn, mx));
                    s.has_set = true;
                    i += 1;
                }
                '(' | ')' | '&' | '@' | '=' | '<' | '>' | ';' => {
                    return Err(self.syntax(U_UNQUOTED_SPECIAL, rule_start));
                }
                c => {
                    cur.push(Elem::Char(c));
                    s.text.push(c);
                    i += 1;
                }
            }
            first = false;
        }
        // "a } b": the key is everything before '}'
        if seen_close {
            s.post = cur;
        } else {
            s.key = cur;
        }
        Ok(s)
    }
}

/// Split the rules into statements (';' outside quotes, sets, escapes),
/// with comments removed: (start, end) char ranges.
fn statements(c: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut st = 0;
    let mut depth = 0;
    while i < c.len() {
        match c[i] {
            '\\' => i += 1,
            '\'' => {
                i += 1;
                while i < c.len() && c[i] != '\'' {
                    i += 1;
                }
            }
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            ';' if depth == 0 => {
                out.push((st, i));
                st = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if c[st.min(c.len())..].iter().any(|c| !is_ws(*c)) {
        out.push((st, c.len()));
    }
    out
}

/// Find the operator of a rule statement: (position, length, kind) with
/// kind 0 = '>', 1 = '<', 2 = '<>', 3 = '='.
fn find_op(c: &[char], a: usize, b: usize) -> Option<(usize, usize, u8)> {
    let mut i = a;
    let mut depth = 0;
    while i < b {
        match c[i] {
            '\\' => i += 1,
            '\'' => {
                i += 1;
                while i < b && c[i] != '\'' {
                    i += 1;
                }
            }
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            '<' if depth == 0 => {
                if i + 1 < b && c[i + 1] == '>' {
                    return Some((i, 2, 2));
                }
                return Some((i, 1, 1));
            }
            '>' if depth == 0 => return Some((i, 1, 0)),
            '\u{2192}' if depth == 0 => return Some((i, 1, 0)),
            '\u{2190}' if depth == 0 => return Some((i, 1, 1)),
            '\u{2194}' if depth == 0 => return Some((i, 1, 2)),
            '=' if depth == 0 => return Some((i, 1, 3)),
            _ => {}
        }
        i += 1;
    }
    None
}

/// Strip comments (`#` to end of line, outside quotes).
fn strip_comments(src: &str) -> Vec<char> {
    let c: Vec<char> = src.chars().collect();
    let mut out = Vec::with_capacity(c.len());
    let mut i = 0;
    let mut depth = 0;
    while i < c.len() {
        let ch = c[i];
        match ch {
            '\\' => {
                out.push(ch);
                if i + 1 < c.len() {
                    out.push(c[i + 1]);
                }
                i += 2;
                continue;
            }
            '\'' => {
                out.push(ch);
                i += 1;
                while i < c.len() && c[i] != '\'' {
                    out.push(c[i]);
                    i += 1;
                }
                if i < c.len() {
                    out.push(c[i]);
                }
                i += 1;
                continue;
            }
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            '#' if depth == 0 => {
                while i < c.len() && !matches!(c[i], '\n' | '\r') {
                    out.push(' ');
                    i += 1;
                }
                continue;
            }
            _ => {}
        }
        out.push(ch);
        i += 1;
    }
    out
}

impl RuleSet {
    /// Parse rules for `forward` (or the reverse) direction.
    pub fn parse(src: &str, forward: bool) -> Result<RuleSet, ParseError> {
        let chars = strip_comments(src);
        let mut p = P {
            c: &chars,
            vars: HashMap::new(),
            var_out: HashMap::new(),
            sets: Vec::new(),
        };
        let mut steps: Vec<Step> = Vec::new();
        let mut rules: Vec<Rule> = Vec::new();
        let mut filter = None;
        let mut reverse_ids: Vec<String> = Vec::new();
        for (a, b) in statements(&chars) {
            let mut a = a;
            while a < b && is_ws(chars[a]) {
                a += 1;
            }
            if a >= b {
                continue;
            }
            // ::ID
            if chars[a] == ':' && chars.get(a + 1) == Some(&':') {
                let body: String = chars[a + 2..b].iter().collect();
                let body = body.trim().to_string();
                let (fwd, rev) = match body.find('(') {
                    Some(k) => {
                        let close = body.rfind(')').unwrap_or(body.len());
                        (
                            body[..k].trim().to_string(),
                            body[k + 1..close].trim().to_string(),
                        )
                    }
                    None => (body.clone(), String::new()),
                };
                let id = if forward { fwd } else { rev.clone() };
                if steps.is_empty() && rules.is_empty() && id.starts_with('[') && forward {
                    let ch: Vec<char> = id.chars().collect();
                    let mut k = 0;
                    if let Ok(s) = uset::parse(&ch, &mut k, &|_| None) {
                        if ch[k..].iter().all(|c| is_ws(*c)) {
                            filter = Some(s);
                            continue;
                        }
                    }
                }
                if !rules.is_empty() {
                    steps.push(Step::Rules(std::mem::take(&mut rules)));
                }
                if forward {
                    if !id.is_empty() {
                        steps.push(Step::Id(id));
                    }
                } else if !id.is_empty() {
                    reverse_ids.push(id);
                }
                continue;
            }
            let mut quotes = 0;
            let mut k = a;
            while k < b {
                match chars[k] {
                    '\\' => k += 1,
                    '\'' => quotes += 1,
                    _ => {}
                }
                k += 1;
            }
            if quotes % 2 == 1 {
                return Err(p.syntax(U_UNTERMINATED_QUOTE, a));
            }
            let Some((op, oplen, kind)) = find_op(&chars, a, b) else {
                return Err(p.syntax(U_MISSING_OPERATOR, a));
            };
            if kind == 3 {
                // $var = value
                let mut k = a;
                while k < op && is_ws(chars[k]) {
                    k += 1;
                }
                if chars.get(k) != Some(&'$') {
                    return Err(p.syntax(U_MALFORMED_VARIABLE_DEFINITION, a));
                }
                let name: String = chars[k + 1..op].iter().filter(|c| !is_ws(**c)).collect();
                if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    return Err(p.syntax(U_MALFORMED_VARIABLE_DEFINITION, a));
                }
                let side = p.side(op + 1, b, a)?;
                p.vars.insert(name.clone(), side.key);
                p.var_out.insert(name, side.text);
                continue;
            }
            let left = p.side(a, op, a)?;
            let right = p.side(op + oplen, b, a)?;
            let make = |src: &Side, dst: &Side| Rule {
                ante: src.ante.clone(),
                key: src.key.clone(),
                post: src.post.clone(),
                out: dst.text.clone(),
                cursor: dst.cursor,
                anchor_start: src.anchor_start,
                anchor_end: src.anchor_end,
            };
            match (kind, forward) {
                (0, true) | (2, true) => rules.push(make(&left, &right)),
                (1, false) | (2, false) => rules.push(make(&right, &left)),
                _ => {}
            }
        }
        if !rules.is_empty() {
            steps.push(Step::Rules(rules));
        }
        if !forward {
            // reverse: the rule sets apply in the reverse order
            steps.reverse();
            for id in reverse_ids.into_iter().rev() {
                steps.push(Step::Id(id));
            }
        }
        let rs = RuleSet {
            sets: p.sets,
            steps,
            filter,
        };
        rs.check_masking()?;
        Ok(rs)
    }

    fn elem_eq(a: &Elem, b: &Elem) -> bool {
        match (a, b) {
            (Elem::Char(x), Elem::Char(y)) => x == y,
            (Elem::Set(x), Elem::Set(y)) => x == y,
            (Elem::Quant(x, a1, b1), Elem::Quant(y, a2, b2)) => {
                a1 == a2 && b1 == b2 && Self::elem_eq(x, y)
            }
            _ => false,
        }
    }

    /// TransliterationRule::masks()
    fn masks(r1: &Rule, r2: &Rule) -> bool {
        let p1: Vec<&Elem> = r1.ante.iter().chain(&r1.key).chain(&r1.post).collect();
        let p2: Vec<&Elem> = r2.ante.iter().chain(&r2.key).chain(&r2.post).collect();
        let len = p1.len();
        let left = r1.ante.len();
        let left2 = r2.ante.len();
        let right = len - left;
        let right2 = p2.len() - left2;
        let cmp = left2 >= left
            && left2 - left + len <= p2.len()
            && (0..len).all(|k| Self::elem_eq(p1[k], p2[left2 - left + k]));
        let f1 = (r1.anchor_start, r1.anchor_end);
        let f2 = (r2.anchor_start, r2.anchor_end);
        if left == left2 && right == right2 && r1.key.len() <= r2.key.len() && cmp {
            return f1 == f2 || (!f1.0 && !f1.1) || (f2.0 && f2.1);
        }
        left <= left2
            && (right < right2 || (right == right2 && r1.key.len() <= r2.key.len()))
            && cmp
    }

    fn check_masking(&self) -> Result<(), ParseError> {
        for st in &self.steps {
            let Step::Rules(rules) = st else { continue };
            for i in 0..rules.len() {
                for j in i + 1..rules.len() {
                    if Self::masks(&rules[i], &rules[j]) {
                        let a: Vec<char> = self.to_rule(&rules[i]).chars().collect();
                        let b: Vec<char> = self.to_rule(&rules[j]).chars().collect();
                        return Err(ParseError {
                            code: U_RULE_MASK_ERROR,
                            offset: -1,
                            pre: ctx(&a, false),
                            post: ctx(&b, false),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn elems_to_rule(&self, out: &mut String, q: &mut String, elems: &[Elem]) {
        for e in elems {
            match e {
                Elem::Char(c) => append_char(out, q, *c, false),
                Elem::Set(k) => {
                    flush(out, q);
                    out.push_str(&self.sets[*k].1);
                }
                Elem::Quant(x, mn, mx) => {
                    self.elems_to_rule(out, q, std::slice::from_ref(x));
                    flush(out, q);
                    out.push(match (mn, mx) {
                        (0, 1) => '?',
                        (0, _) => '*',
                        _ => '+',
                    });
                }
            }
        }
    }

    /// TransliterationRule::toRule()
    pub fn to_rule(&self, r: &Rule) -> String {
        let mut out = String::new();
        let mut q = String::new();
        let braces = !r.ante.is_empty() || !r.post.is_empty();
        if r.anchor_start {
            out.push('^');
        }
        self.elems_to_rule(&mut out, &mut q, &r.ante);
        if braces {
            append_char(&mut out, &mut q, '{', true);
        }
        self.elems_to_rule(&mut out, &mut q, &r.key);
        if braces {
            append_char(&mut out, &mut q, '}', true);
        }
        self.elems_to_rule(&mut out, &mut q, &r.post);
        if r.anchor_end {
            append_char(&mut out, &mut q, '$', true);
        }
        for c in " > ".chars() {
            append_char(&mut out, &mut q, c, true);
        }
        for (k, &c) in r.out.iter().enumerate() {
            if r.cursor == Some(k) {
                append_char(&mut out, &mut q, '|', true);
            }
            append_char(&mut out, &mut q, c, false);
        }
        if r.cursor == Some(r.out.len()) && !r.out.is_empty() {
            append_char(&mut out, &mut q, '|', true);
        }
        append_char(&mut out, &mut q, ';', true);
        flush(&mut out, &mut q);
        out
    }

    fn match_elem(&self, e: &Elem, c: char) -> bool {
        match e {
            Elem::Char(x) => *x == c,
            Elem::Set(k) => self.sets[*k].0.has(c),
            Elem::Quant(x, _, _) => self.match_elem(x, c),
        }
    }

    /// Match `elems` forward from `pos` (not past `limit`): the end.
    fn match_fwd(&self, elems: &[Elem], t: &[char], mut pos: usize, limit: usize) -> Option<usize> {
        for e in elems {
            match e {
                Elem::Quant(x, mn, mx) => {
                    let mut n = 0;
                    while n < *mx && pos < limit && self.match_elem(x, t[pos]) {
                        pos += 1;
                        n += 1;
                    }
                    if n < *mn {
                        return None;
                    }
                }
                _ => {
                    if pos >= limit || !self.match_elem(e, t[pos]) {
                        return None;
                    }
                    pos += 1;
                }
            }
        }
        Some(pos)
    }

    /// Match `elems` backward ending at `pos` (not before `start`): the
    /// start.
    fn match_back(
        &self,
        elems: &[Elem],
        t: &[char],
        mut pos: usize,
        start: usize,
    ) -> Option<usize> {
        for e in elems.iter().rev() {
            match e {
                Elem::Quant(x, mn, mx) => {
                    let mut n = 0;
                    while n < *mx && pos > start && self.match_elem(x, t[pos - 1]) {
                        pos -= 1;
                        n += 1;
                    }
                    if n < *mn {
                        return None;
                    }
                }
                _ => {
                    if pos <= start || !self.match_elem(e, t[pos - 1]) {
                        return None;
                    }
                    pos -= 1;
                }
            }
        }
        Some(pos)
    }

    /// Apply the rules of one step to `t`.
    pub fn apply_rules(&self, rules: &[Rule], t: &mut Vec<char>) {
        let mut pos = 0;
        while pos < t.len() {
            let limit = t.len();
            let mut done = false;
            for r in rules {
                let Some(kend) = self.match_fwd(&r.key, t, pos, limit) else {
                    continue;
                };
                let Some(pend) = self.match_fwd(&r.post, t, kend, limit) else {
                    continue;
                };
                if r.anchor_end && pend != limit {
                    continue;
                }
                let Some(astart) = self.match_back(&r.ante, t, pos, 0) else {
                    continue;
                };
                if r.anchor_start && astart != 0 {
                    continue;
                }
                if kend == pos && r.out.is_empty() {
                    continue;
                }
                t.splice(pos..kend, r.out.iter().copied());
                let adv = r.cursor.unwrap_or(r.out.len());
                pos += adv;
                if adv == 0 && kend == pos {
                    pos += 1;
                }
                done = true;
                break;
            }
            if !done {
                pos += 1;
            }
        }
    }
}

fn flush(out: &mut String, q: &mut String) {
    if q.is_empty() {
        return;
    }
    out.push('\'');
    out.push_str(q);
    out.push('\'');
    q.clear();
}

/// ICU_Utility::appendToRule() for one character.
fn append_char(out: &mut String, q: &mut String, c: char, literal: bool) {
    if literal {
        flush(out, q);
        out.push(c);
        return;
    }
    if c == '\'' {
        flush(out, q);
        out.push_str("\\'");
        return;
    }
    let cp = c as u32;
    if (0x21..=0x7E).contains(&cp) && !c.is_ascii_alphanumeric() || is_ws(c) {
        q.push(c);
    } else {
        flush(out, q);
        out.push(c);
    }
}

/// A compiled set reference (for callers building rules in code).
pub type Shared = Rc<RuleSet>;
