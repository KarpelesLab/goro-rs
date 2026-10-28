//! Text boundaries (`IntlBreakIterator`, PHP manual): the UAX #29
//! grapheme, word and sentence boundaries and UAX #14 line breaks of the
//! intl crate, code points, and rule-based iterators with a subset of
//! ICU's break rule syntax (ICU user guide "Boundary Analysis: Break
//! Rules": `$var = expr;`, `!!forward;` sections, rules of sets,
//! literals, `$vars`, groups, `|`, `* + ?` and `{status}` tags; the
//! longest match wins at each position).

use crate::uset::{self, USet};

/// The kind of an iterator.
#[derive(Clone, Debug)]
pub enum Kind {
    Character,
    Word,
    Line,
    Sentence,
    Title,
    CodePoint,
    Rules(Box<RuleSet>),
}

/// ICU's rule status ranges (UBreakIteratorWord / Line / Sentence).
pub const WORD_NONE: i32 = 0;
pub const WORD_NUMBER: i32 = 100;
pub const WORD_LETTER: i32 = 200;
pub const WORD_KANA: i32 = 300;
pub const WORD_IDEO: i32 = 400;
pub const LINE_SOFT: i32 = 0;
pub const LINE_HARD: i32 = 100;
pub const SENTENCE_TERM: i32 = 0;
pub const SENTENCE_SEP: i32 = 100;

/// The boundaries of `text` (byte offsets, 0 first) with the rule
/// statuses of the segment each one ends.
pub fn boundaries(kind: &Kind, text: &str) -> Vec<(usize, Vec<i32>)> {
    let mut out = vec![(0usize, vec![0])];
    let mut pos = 0;
    match kind {
        Kind::Character => {
            for g in intl::unicode::graphemes(text) {
                pos += g.len();
                out.push((pos, vec![0]));
            }
        }
        Kind::CodePoint => {
            for c in text.chars() {
                pos += c.len_utf8();
                out.push((pos, vec![0]));
            }
        }
        Kind::Word | Kind::Title => {
            for w in intl::unicode::words(text) {
                pos += w.len();
                out.push((pos, vec![word_status(w)]));
            }
        }
        Kind::Sentence => {
            for s in intl::unicode::sentences(text) {
                pos += s.len();
                let sep = s.trim_end_matches([' ', '\t']).ends_with(['\n', '\r', '\u{2029}', '\u{85}']);
                out.push((pos, vec![if sep { SENTENCE_SEP } else { SENTENCE_TERM }]));
            }
        }
        Kind::Line => {
            for b in intl::unicode::line_breaks(text) {
                pos += b.text.len();
                out.push((pos, vec![if b.mandatory { LINE_HARD } else { LINE_SOFT }]));
            }
        }
        Kind::Rules(r) => return r.boundaries(text),
    }
    out
}

/// The word status of a segment (ICU: numbers, letters, kana,
/// ideographs, else none).
fn word_status(w: &str) -> i32 {
    use intl::unicode as u;
    let mut st = WORD_NONE;
    for c in w.chars() {
        let s = match u::script(c).long_name() {
            "Hiragana" | "Katakana" => WORD_KANA,
            "Han" => WORD_IDEO,
            _ if u::is_alphabetic(c) => WORD_LETTER,
            _ if u::is_numeric(c) => WORD_NUMBER,
            _ => continue,
        };
        st = st.max(s);
    }
    st
}

// ---------------------------------------------------------------- rules

#[derive(Clone, Debug)]
enum Node {
    Set(USet),
    Lit(char),
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    Star(Box<Node>),
    Plus(Box<Node>),
    Opt(Box<Node>),
}

#[derive(Clone, Debug)]
struct Rule {
    node: Node,
    status: i32,
}

/// Compiled break rules.
#[derive(Clone, Debug)]
pub struct RuleSet {
    /// The rules as given.
    pub source: String,
    forward: Vec<Rule>,
}

/// A rule syntax error: (line, offset in the line).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RuleError {
    pub line: usize,
    pub offset: usize,
}

impl RuleSet {
    /// The rules ICU 68+ gives back: the source without whitespace and
    /// comments (ext/intl/tests/rbbiter_getRules_basic3.phpt).
    pub fn rules_text(&self) -> String {
        let mut out = String::new();
        let mut in_set = 0;
        let mut quote = false;
        let mut chars = self.source.chars().peekable();
        while let Some(c) = chars.next() {
            if quote {
                out.push(c);
                if c == '\'' {
                    quote = false;
                }
                continue;
            }
            match c {
                '\'' => {
                    quote = true;
                    out.push(c);
                }
                '\\' => {
                    out.push(c);
                    if let Some(n) = chars.next() {
                        out.push(n);
                    }
                }
                '#' if in_set == 0 => {
                    for n in chars.by_ref() {
                        if n == '\n' {
                            break;
                        }
                    }
                }
                '[' => {
                    in_set += 1;
                    out.push(c);
                }
                ']' => {
                    in_set -= 1;
                    out.push(c);
                }
                c if c.is_whitespace() => {}
                c => out.push(c),
            }
        }
        out
    }

    pub fn parse(src: &str) -> Result<RuleSet, RuleError> {
        let chars: Vec<char> = src.chars().collect();
        // statements: ';'-terminated, outside sets and quotes
        let mut stmts: Vec<(usize, String)> = Vec::new();
        let mut cur = String::new();
        let mut start = 0;
        let mut depth = 0;
        let mut quote = false;
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if cur.trim().is_empty() && cur.is_empty() {
                start = i;
            }
            if quote {
                cur.push(c);
                if c == '\'' {
                    quote = false;
                }
                i += 1;
                continue;
            }
            match c {
                '\\' => {
                    cur.push(c);
                    if let Some(n) = chars.get(i + 1) {
                        cur.push(*n);
                    }
                    i += 2;
                    continue;
                }
                '\'' => quote = true,
                '[' => depth += 1,
                ']' => depth -= 1,
                '#' if depth == 0 => {
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                    }
                    continue;
                }
                ';' if depth == 0 => {
                    stmts.push((start, std::mem::take(&mut cur)));
                    i += 1;
                    continue;
                }
                _ => {}
            }
            cur.push(c);
            i += 1;
        }
        let err_at = |pos: usize| {
            let line = chars[..pos.min(chars.len())].iter().filter(|c| **c == '\n').count() + 1;
            let line_start = chars[..pos.min(chars.len())]
                .iter()
                .rposition(|c| *c == '\n')
                .map_or(0, |p| p + 1);
            RuleError {
                line,
                offset: pos - line_start,
            }
        };
        if !cur.trim().is_empty() {
            // a statement without its ';'
            return Err(err_at(chars.len()));
        }
        let mut vars: Vec<(String, String)> = Vec::new();
        let mut forward = Vec::new();
        let mut section = "forward".to_string();
        for (pos, st) in stmts {
            let t = st.trim();
            if t.is_empty() {
                continue;
            }
            if let Some(s) = t.strip_prefix("!!") {
                let s = s.trim().to_ascii_lowercase();
                if matches!(s.as_str(), "forward" | "reverse" | "safe_forward" | "safe_reverse") {
                    section = s;
                }
                continue;
            }
            if t.starts_with('$')
                && let Some(eq) = t.find('=')
                && t[1..eq].trim().chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                let name = t[..eq].trim().to_string();
                let mut body = t[eq + 1..].trim().to_string();
                for (n, v) in &vars {
                    body = substitute(&body, n, v);
                }
                vars.push((name, format!("({body})")));
                continue;
            }
            if section != "forward" {
                continue;
            }
            let mut body = t.to_string();
            let mut status = 0;
            if let Some(open) = body.rfind('{')
                && body.ends_with('}')
            {
                status = body[open + 1..body.len() - 1].trim().parse().map_err(|_| err_at(pos))?;
                body.truncate(open);
            }
            for (n, v) in vars.iter().rev() {
                body = substitute(&body, n, v);
            }
            let bc: Vec<char> = body.chars().collect();
            let mut j = 0;
            let node = parse_alt(&bc, &mut j).map_err(|_| err_at(pos + j))?;
            skip_ws(&bc, &mut j);
            if j != bc.len() {
                return Err(err_at(pos + j));
            }
            forward.push(Rule { node, status });
        }
        Ok(RuleSet {
            source: src.to_string(),
            forward,
        })
    }

    fn boundaries(&self, text: &str) -> Vec<(usize, Vec<i32>)> {
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let mut out = vec![(0usize, vec![0])];
        let mut i = 0;
        while i < chars.len() {
            let mut best = 0;
            let mut statuses: Vec<i32> = Vec::new();
            for r in &self.forward {
                let ends = match_node(&r.node, &chars, i);
                if let Some(&e) = ends.iter().max()
                    && e > i
                {
                    let len = e - i;
                    if len > best {
                        best = len;
                        statuses = vec![r.status];
                    } else if len == best && !statuses.contains(&r.status) {
                        statuses.push(r.status);
                    }
                }
            }
            if best == 0 {
                best = 1;
                statuses = vec![0];
            }
            statuses.sort_unstable();
            i += best;
            let pos = chars.get(i).map_or(text.len(), |(p, _)| *p);
            out.push((pos, statuses));
        }
        out
    }
}

fn substitute(body: &str, name: &str, value: &str) -> String {
    // replace $name not followed by a name character
    let mut out = String::new();
    let mut rest = body;
    while let Some(p) = rest.find(name) {
        let after = &rest[p + name.len()..];
        out.push_str(&rest[..p]);
        if after.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_') {
            out.push_str(name);
        } else {
            out.push_str(value);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

fn skip_ws(c: &[char], i: &mut usize) {
    while *i < c.len() && c[*i].is_whitespace() {
        *i += 1;
    }
}

fn parse_alt(c: &[char], i: &mut usize) -> Result<Node, ()> {
    let mut alts = vec![parse_seq(c, i)?];
    while c.get(*i) == Some(&'|') {
        *i += 1;
        alts.push(parse_seq(c, i)?);
    }
    Ok(if alts.len() == 1 { alts.pop().unwrap() } else { Node::Alt(alts) })
}

fn parse_seq(c: &[char], i: &mut usize) -> Result<Node, ()> {
    let mut items = Vec::new();
    loop {
        skip_ws(c, i);
        let Some(&ch) = c.get(*i) else { break };
        let atom = match ch {
            '|' | ')' => break,
            '(' => {
                *i += 1;
                let n = parse_alt(c, i)?;
                skip_ws(c, i);
                if c.get(*i) != Some(&')') {
                    return Err(());
                }
                *i += 1;
                n
            }
            '[' => Node::Set(uset::parse(c, i).map_err(|_| ())?),
            '\\' if matches!(c.get(*i + 1), Some('p') | Some('P')) => {
                Node::Set(uset::parse(c, i).map_err(|_| ())?)
            }
            '\\' => {
                let (cp, n) = uset::escape(&c[*i + 1..]).ok_or(())?;
                *i += 1 + n;
                Node::Lit(char::from_u32(cp).ok_or(())?)
            }
            '\'' => {
                *i += 1;
                let mut lits = Vec::new();
                while let Some(&x) = c.get(*i) {
                    *i += 1;
                    if x == '\'' {
                        break;
                    }
                    lits.push(Node::Lit(x));
                }
                Node::Seq(lits)
            }
            '.' => {
                *i += 1;
                Node::Set(USet::Prop(uset::Prop::Any))
            }
            '$' => return Err(()),
            '*' | '+' | '?' | '{' | '}' => return Err(()),
            _ => {
                *i += 1;
                Node::Lit(ch)
            }
        };
        skip_ws(c, i);
        let atom = match c.get(*i) {
            Some('*') => {
                *i += 1;
                Node::Star(Box::new(atom))
            }
            Some('+') => {
                *i += 1;
                Node::Plus(Box::new(atom))
            }
            Some('?') => {
                *i += 1;
                Node::Opt(Box::new(atom))
            }
            _ => atom,
        };
        items.push(atom);
    }
    if items.is_empty() {
        return Err(());
    }
    Ok(if items.len() == 1 { items.pop().unwrap() } else { Node::Seq(items) })
}

/// The end positions (char indices) of the matches of `n` at `at`.
fn match_node(n: &Node, s: &[(usize, char)], at: usize) -> Vec<usize> {
    match n {
        Node::Set(set) => match s.get(at) {
            Some((_, c)) if set.contains(*c) => vec![at + 1],
            _ => vec![],
        },
        Node::Lit(l) => match s.get(at) {
            Some((_, c)) if c == l => vec![at + 1],
            _ => vec![],
        },
        Node::Seq(items) => {
            let mut cur = vec![at];
            for it in items {
                let mut next = Vec::new();
                for p in cur {
                    for e in match_node(it, s, p) {
                        if !next.contains(&e) {
                            next.push(e);
                        }
                    }
                }
                if next.is_empty() {
                    return next;
                }
                cur = next;
            }
            cur
        }
        Node::Alt(alts) => {
            let mut v = Vec::new();
            for a in alts {
                for e in match_node(a, s, at) {
                    if !v.contains(&e) {
                        v.push(e);
                    }
                }
            }
            v
        }
        Node::Opt(inner) => {
            let mut v = vec![at];
            for e in match_node(inner, s, at) {
                if !v.contains(&e) {
                    v.push(e);
                }
            }
            v
        }
        Node::Star(inner) | Node::Plus(inner) => {
            let mut all: Vec<usize> = if matches!(n, Node::Star(_)) { vec![at] } else { vec![] };
            let mut frontier = vec![at];
            let mut seen = vec![at];
            while !frontier.is_empty() {
                let mut next = Vec::new();
                for p in frontier {
                    for e in match_node(inner, s, p) {
                        if e > p && !seen.contains(&e) {
                            seen.push(e);
                            next.push(e);
                            all.push(e);
                        }
                    }
                }
                frontier = next;
            }
            all
        }
    }
}
