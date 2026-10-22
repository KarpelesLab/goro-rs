//! A RuleBasedBreakIterator rule engine (ICU's rbbi rule syntax): variable
//! definitions, UnicodeSets, concatenation, `|`, `*` `+` `?`, groups, `.`,
//! literals, the `/` look-ahead break point and `{n}` rule statuses. The
//! forward rules run as an NFA from each boundary; the longest match wins
//! and its status values are those of all the rules matching that far.

use std::collections::HashMap;

use crate::uset::{self, USet};

#[derive(Clone, Debug)]
enum Node {
    Set(usize),
    Cat(Vec<Node>),
    Alt(Vec<Node>),
    Star(Box<Node>),
    Plus(Box<Node>),
    Opt(Box<Node>),
    Look,
    Empty,
}

#[derive(Clone, Debug)]
enum St {
    Set(usize, usize),
    Split(usize, usize),
    Look(usize),
    Accept(i32),
}

/// Compiled rules.
#[derive(Debug)]
pub struct Rules {
    /// The source given (as UTF-8).
    pub source: String,
    sets: Vec<USet>,
    states: Vec<St>,
    start: usize,
}

/// A rule syntax error: ICU status, line (1-based) and offset in the line.
#[derive(Debug, Clone, Copy)]
pub struct RuleError {
    pub code: i32,
    pub line: i32,
    pub offset: i32,
}

const U_BRK_HEX_DIGITS_EXPECTED: i32 = 0x10201;
const U_BRK_SEMICOLON_EXPECTED: i32 = 0x10202;
const U_BRK_RULE_SYNTAX: i32 = 0x10203;
const U_BRK_UNCLOSED_SET: i32 = 0x10204;
const U_BRK_MISMATCHED_PAREN: i32 = 0x10207;
const U_BRK_UNDEFINED_VARIABLE: i32 = 0x10209;
const U_BRK_UNRECOGNIZED_OPTION: i32 = 0x1020C;

fn is_ws(c: char) -> bool {
    matches!(
        c as u32,
        0x09..=0x0D | 0x20 | 0x85 | 0x200E | 0x200F | 0x2028 | 0x2029
    )
}

/// RBBIRuleScanner::stripRules(): comments and white space removed.
pub fn strip_rules(rules: &str) -> String {
    let c: Vec<char> = rules.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if is_ws(ch) {
            i += 1;
            continue;
        }
        if ch == '#' {
            while i < c.len() && !matches!(c[i], '\r' | '\n' | '\u{85}') {
                i += 1;
            }
            continue;
        }
        out.push(ch);
        i += 1;
        if ch == '\\' && i < c.len() {
            out.push(c[i]);
            i += 1;
        } else if ch == '\'' {
            while i < c.len() {
                out.push(c[i]);
                i += 1;
                if c[i - 1] == '\'' {
                    break;
                }
            }
        }
    }
    out
}

struct Parser<'a> {
    c: &'a [char],
    p: usize,
    vars: HashMap<String, Node>,
    var_sets: HashMap<String, USet>,
    sets: Vec<USet>,
}

impl Parser<'_> {
    fn error(&self, code: i32, at: usize) -> RuleError {
        let mut line = 1;
        let mut ls = 0;
        for (i, &ch) in self.c.iter().enumerate().take(at) {
            if ch == '\n' {
                line += 1;
                ls = i + 1;
            }
        }
        RuleError {
            code,
            line,
            offset: (at - ls) as i32,
        }
    }

    fn skip_ws(&mut self) {
        loop {
            while self.p < self.c.len() && is_ws(self.c[self.p]) {
                self.p += 1;
            }
            if self.c.get(self.p) == Some(&'#') {
                while self.p < self.c.len() && !matches!(self.c[self.p], '\r' | '\n') {
                    self.p += 1;
                }
                continue;
            }
            break;
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.skip_ws();
        self.c.get(self.p).copied()
    }

    fn var_name(&mut self) -> String {
        let st = self.p;
        while self
            .c
            .get(self.p)
            .is_some_and(|c| c.is_alphanumeric() || *c == '_')
        {
            self.p += 1;
        }
        self.c[st..self.p].iter().collect()
    }

    fn add_set(&mut self, s: USet) -> Node {
        self.sets.push(s);
        Node::Set(self.sets.len() - 1)
    }

    fn lit(&mut self, cp: u32) -> Node {
        self.add_set(USet::Ranges(vec![(cp, cp)]))
    }

    fn alt(&mut self) -> Result<Node, RuleError> {
        let mut alts = vec![self.cat()?];
        while self.peek() == Some('|') {
            self.p += 1;
            alts.push(self.cat()?);
        }
        Ok(if alts.len() == 1 {
            alts.pop().unwrap()
        } else {
            Node::Alt(alts)
        })
    }

    fn cat(&mut self) -> Result<Node, RuleError> {
        let mut items = Vec::new();
        while let Some(ch) = self.peek() {
            if matches!(ch, '|' | ')' | ';' | '{' | '=') {
                break;
            }
            let mut atom = self.atom()?;
            loop {
                match self.peek() {
                    Some('*') => atom = Node::Star(Box::new(atom)),
                    Some('+') => atom = Node::Plus(Box::new(atom)),
                    Some('?') => atom = Node::Opt(Box::new(atom)),
                    _ => break,
                }
                self.p += 1;
            }
            items.push(atom);
        }
        Ok(match items.len() {
            0 => Node::Empty,
            1 => items.pop().unwrap(),
            _ => Node::Cat(items),
        })
    }

    fn atom(&mut self) -> Result<Node, RuleError> {
        let ch = self.c[self.p];
        let at = self.p;
        match ch {
            '(' => {
                self.p += 1;
                let n = self.alt()?;
                if self.peek() != Some(')') {
                    return Err(self.error(U_BRK_MISMATCHED_PAREN, self.p));
                }
                self.p += 1;
                Ok(n)
            }
            '[' => self.set(),
            '\\' if matches!(self.c.get(self.p + 1), Some('p' | 'P' | 'N')) => self.set(),
            '\\' => {
                self.p += 1;
                match uset::parse_escape(self.c, &mut self.p) {
                    Some(cp) => Ok(self.lit(cp)),
                    None => Err(self.error(U_BRK_HEX_DIGITS_EXPECTED, at)),
                }
            }
            '$' => {
                self.p += 1;
                let name = self.var_name();
                match self.vars.get(&name) {
                    Some(n) => Ok(n.clone()),
                    None => Err(self.error(U_BRK_UNDEFINED_VARIABLE, at)),
                }
            }
            '.' => {
                self.p += 1;
                Ok(self.add_set(USet::all()))
            }
            '/' => {
                self.p += 1;
                Ok(Node::Look)
            }
            '^' => {
                self.p += 1;
                Ok(Node::Empty)
            }
            '\'' => {
                self.p += 1;
                let mut items = Vec::new();
                loop {
                    match self.c.get(self.p) {
                        None => return Err(self.error(U_BRK_RULE_SYNTAX, at)),
                        Some('\'') if self.c.get(self.p + 1) == Some(&'\'') => {
                            self.p += 2;
                            items.push(self.lit('\'' as u32));
                        }
                        Some('\'') => {
                            self.p += 1;
                            break;
                        }
                        Some(&c) => {
                            self.p += 1;
                            items.push(self.lit(c as u32));
                        }
                    }
                }
                if items.is_empty() {
                    items.push(self.lit('\'' as u32));
                }
                Ok(Node::Cat(items))
            }
            '*' | '+' | '?' | '}' | ']' | '!' => Err(self.error(U_BRK_RULE_SYNTAX, at)),
            c => {
                self.p += 1;
                Ok(self.lit(c as u32))
            }
        }
    }

    fn set(&mut self) -> Result<Node, RuleError> {
        let at = self.p;
        let vs = &self.var_sets;
        let lookup = |n: &str| vs.get(n).cloned();
        match uset::parse(self.c, &mut self.p, &lookup) {
            Ok(s) => Ok(self.add_set(s)),
            Err(e) => Err(self.error(U_BRK_UNCLOSED_SET, e.pos.max(at))),
        }
    }
}

/// Thompson construction.
struct Builder {
    st: Vec<St>,
}

impl Builder {
    fn push(&mut self, s: St) -> usize {
        self.st.push(s);
        self.st.len() - 1
    }
    /// Compile `n` with continuation `next`; returns the entry state.
    fn build(&mut self, n: &Node, next: usize) -> usize {
        match n {
            Node::Set(i) => self.push(St::Set(*i, next)),
            Node::Empty => next,
            Node::Look => self.push(St::Look(next)),
            Node::Cat(v) => {
                let mut e = next;
                for x in v.iter().rev() {
                    e = self.build(x, e);
                }
                e
            }
            Node::Alt(v) => {
                let entries: Vec<usize> = v.iter().map(|x| self.build(x, next)).collect();
                let mut e = *entries.last().unwrap();
                for &x in entries.iter().rev().skip(1) {
                    e = self.push(St::Split(x, e));
                }
                e
            }
            Node::Opt(x) => {
                let b = self.build(x, next);
                self.push(St::Split(b, next))
            }
            Node::Star(x) => {
                let split = self.push(St::Split(0, next));
                let b = self.build(x, split);
                self.st[split] = St::Split(b, next);
                split
            }
            Node::Plus(x) => {
                let split = self.push(St::Split(0, next));
                let b = self.build(x, split);
                self.st[split] = St::Split(b, next);
                b
            }
        }
    }
}

impl Rules {
    /// Compile `src`.
    pub fn compile(src: &str) -> Result<Rules, RuleError> {
        let chars: Vec<char> = src.chars().collect();
        let mut p = Parser {
            c: &chars,
            p: 0,
            vars: HashMap::new(),
            var_sets: HashMap::new(),
            sets: Vec::new(),
        };
        let mut section = String::from("forward");
        let mut forward: Vec<(Node, i32)> = Vec::new();
        while let Some(ch) = p.peek() {
            let stmt_start = p.p;
            if ch == ';' {
                p.p += 1;
                continue;
            }
            if ch == '!' && p.c.get(p.p + 1) == Some(&'!') {
                p.p += 2;
                let name = p.var_name();
                match name.as_str() {
                    "forward" | "reverse" | "safe_forward" | "safe_reverse" => section = name,
                    "chain" | "LBCMNoChain" | "lookAheadHardBreak" | "quoted_literals_only" => {}
                    _ => return Err(p.error(U_BRK_UNRECOGNIZED_OPTION, stmt_start)),
                }
                if p.peek() != Some(';') {
                    return Err(p.error(U_BRK_SEMICOLON_EXPECTED, p.p));
                }
                p.p += 1;
                continue;
            }
            // $var = expr;
            if ch == '$' {
                let save = p.p;
                p.p += 1;
                let name = p.var_name();
                if p.peek() == Some('=') {
                    p.p += 1;
                    p.skip_ws();
                    let expr_start = p.p;
                    let node = p.alt()?;
                    if p.peek() != Some(';') {
                        return Err(p.error(U_BRK_SEMICOLON_EXPECTED, p.p));
                    }
                    let _ = expr_start;
                    p.p += 1;
                    if let Node::Set(i) = node {
                        let s = p.sets[i].clone();
                        p.var_sets.insert(name.clone(), s);
                    }
                    p.vars.insert(name, node);
                    continue;
                }
                p.p = save;
            }
            let node = p.alt()?;
            let mut tag = 0;
            if p.peek() == Some('{') {
                p.p += 1;
                p.skip_ws();
                let st = p.p;
                let mut neg = false;
                if p.c.get(p.p) == Some(&'-') {
                    neg = true;
                    p.p += 1;
                }
                while p.c.get(p.p).is_some_and(|c| c.is_ascii_digit()) {
                    p.p += 1;
                }
                let digits: String = p.c[st..p.p].iter().filter(|c| **c != '-').collect();
                if digits.is_empty() || p.peek() != Some('}') {
                    return Err(p.error(U_BRK_RULE_SYNTAX, st));
                }
                p.p += 1;
                tag = digits.parse::<i32>().unwrap_or(0) * if neg { -1 } else { 1 };
            }
            match p.peek() {
                Some(';') => p.p += 1,
                None => return Err(p.error(U_BRK_SEMICOLON_EXPECTED, chars.len())),
                Some(_) => return Err(p.error(U_BRK_RULE_SYNTAX, p.p)),
            }
            if section == "forward" {
                forward.push((node, tag));
            }
        }
        let mut b = Builder { st: Vec::new() };
        let mut entries = Vec::new();
        for (n, tag) in &forward {
            let acc = b.push(St::Accept(*tag));
            entries.push(b.build(n, acc));
        }
        let start = if entries.is_empty() {
            b.push(St::Accept(0))
        } else {
            let mut e = *entries.last().unwrap();
            for &x in entries.iter().rev().skip(1) {
                e = b.push(St::Split(x, e));
            }
            e
        };
        Ok(Rules {
            source: src.to_string(),
            sets: p.sets,
            states: b.st,
            start,
        })
    }

    fn closure(
        &self,
        s: usize,
        look: Option<usize>,
        pos: usize,
        out: &mut Vec<(usize, Option<usize>)>,
        seen: &mut [bool],
    ) {
        if seen[s] {
            return;
        }
        seen[s] = true;
        match &self.states[s] {
            St::Split(a, b) => {
                self.closure(*a, look, pos, out, seen);
                self.closure(*b, look, pos, out, seen);
            }
            St::Look(a) => self.closure(*a, Some(pos), pos, out, seen),
            _ => out.push((s, look)),
        }
    }

    /// The boundaries of `chars` (code points with their native offsets;
    /// `len` the native length) and their statuses.
    pub fn boundaries(&self, chars: &[(usize, u32)], len: usize) -> Vec<(usize, Vec<i32>)> {
        let mut out = vec![(0usize, vec![0])];
        let mut i = 0;
        let n = self.states.len();
        let native = |k: usize| if k < chars.len() { chars[k].0 } else { len };
        while i < chars.len() {
            let mut seen = vec![false; n];
            let mut cur = Vec::new();
            self.closure(self.start, None, i, &mut cur, &mut seen);
            // the candidate ends (char index) with their tags
            let mut cands: Vec<(usize, i32)> = Vec::new();
            let mut k = i;
            loop {
                for &(s, look) in &cur {
                    if let St::Accept(t) = self.states[s] {
                        let e = look.unwrap_or(k);
                        if e > i {
                            cands.push((e, t));
                        }
                    }
                }
                if k >= chars.len() || cur.is_empty() {
                    break;
                }
                let cp = chars[k].1;
                let mut seen = vec![false; n];
                let mut next = Vec::new();
                for &(s, look) in &cur {
                    if let St::Set(si, to) = self.states[s] {
                        if self.sets[si].contains(cp) {
                            self.closure(to, look, k + 1, &mut next, &mut seen);
                        }
                    }
                }
                cur = next;
                k += 1;
            }
            let end = cands.iter().map(|c| c.0).max().unwrap_or(i + 1);
            let mut tags: Vec<i32> = cands
                .iter()
                .filter(|c| c.0 == end && c.1 != 0)
                .map(|c| c.1)
                .collect();
            tags.dedup();
            tags.sort();
            tags.dedup();
            if tags.is_empty() {
                tags.push(0);
            }
            i = end;
            out.push((native(i), tags));
        }
        out
    }
}

/// The getBinaryRules() image: a header and the rule source.
const MAGIC: &[u8] = b"\xb1\xa0\x00\x00GORO-RBBI\x00";

pub fn binary(r: &Rules) -> Vec<u8> {
    let mut v = MAGIC.to_vec();
    v.extend_from_slice(r.source.as_bytes());
    v
}

pub fn from_binary(b: &[u8]) -> Option<Rules> {
    let src = b.strip_prefix(MAGIC)?;
    Rules::compile(std::str::from_utf8(src).ok()?).ok()
}
