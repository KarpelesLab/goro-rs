//! `MessageFormatter` (PHP manual: MessageFormatter): ICU MessageFormat
//! patterns as the ICU user guide ("Formatting Messages") and the
//! MessagePattern API documentation describe them: arguments
//! `{name[, type[, style]]}`, `plural`/`selectordinal` (with `offset:`,
//! `=n` selectors and `#`), `select`, `choice`, and the apostrophe quoting
//! of ApostropheMode DOUBLE_OPTIONAL.

use crate::locale;
use crate::numfmt::{self as nf, Dec, NumberFormat};

/// An argument name: a number or an identifier.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ArgName {
    Index(i64),
    Name(String),
}

impl std::fmt::Display for ArgName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArgName::Index(i) => write!(f, "{i}"),
            ArgName::Name(n) => f.write_str(n),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Node {
    Text(String),
    /// `#` in a plural sub-message.
    Pound,
    Arg(ArgName, ArgKind),
}

#[derive(Clone, Debug)]
pub enum Selector {
    Exact(f64),
    Keyword(String),
}

#[derive(Clone, Debug)]
pub enum ArgKind {
    None,
    /// `number`, `date`, `time`, `spellout`, `ordinal`, `duration` with
    /// an optional style.
    Simple(String, Option<String>),
    Plural {
        ordinal: bool,
        offset: f64,
        cases: Vec<(Selector, Vec<Node>)>,
    },
    Select(Vec<(String, Vec<Node>)>),
    Choice(Vec<(f64, bool, Vec<Node>)>),
}

/// The type class of an argument (the value conversion PHP applies).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgClass {
    Any,
    Number,
    Date,
    String,
}

/// An argument used with two type classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InconsistentTypes;

/// A pattern error.
#[derive(Debug, Clone, PartialEq)]
pub enum MsgError {
    /// U_PATTERN_SYNTAX_ERROR at a UTF-16 offset.
    Syntax(usize),
    /// U_UNMATCHED_BRACES.
    UnmatchedBraces,
    /// U_ILLEGAL_ARGUMENT_ERROR (an unknown argument type).
    IllegalArgument,
}

#[derive(Clone, Debug)]
pub struct MessagePattern {
    pub nodes: Vec<Node>,
}

struct Parser<'a> {
    c: &'a [char],
    i: usize,
}

fn u16_at(c: &[char], i: usize) -> usize {
    c[..i.min(c.len())].iter().map(|x| x.len_utf16()).sum()
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while self.i < self.c.len() && self.c[self.i].is_whitespace() {
            self.i += 1;
        }
    }

    fn syntax(&self, at: usize) -> MsgError {
        MsgError::Syntax(u16_at(self.c, at))
    }

    /// A message up to an unmatched `}` (when `nested`) or the end.
    fn message(&mut self, nested: bool, in_plural: bool) -> Result<Vec<Node>, MsgError> {
        let mut nodes = Vec::new();
        let mut text = String::new();
        while self.i < self.c.len() {
            let ch = self.c[self.i];
            match ch {
                '\'' => {
                    let next = self.c.get(self.i + 1).copied();
                    if next == Some('\'') {
                        text.push('\'');
                        self.i += 2;
                    } else if matches!(next, Some('{') | Some('}'))
                        || (in_plural && next == Some('#'))
                        || next == Some('|')
                    {
                        // quoted literal up to the next single apostrophe
                        self.i += 1;
                        while self.i < self.c.len() {
                            if self.c[self.i] == '\'' {
                                if self.c.get(self.i + 1) == Some(&'\'') {
                                    text.push('\'');
                                    self.i += 2;
                                    continue;
                                }
                                break;
                            }
                            text.push(self.c[self.i]);
                            self.i += 1;
                        }
                        self.i += 1;
                    } else {
                        text.push('\'');
                        self.i += 1;
                    }
                }
                '{' => {
                    if !text.is_empty() {
                        nodes.push(Node::Text(std::mem::take(&mut text)));
                    }
                    self.i += 1;
                    let (name, kind) = self.argument()?;
                    nodes.push(Node::Arg(name, kind));
                }
                '}' => {
                    if nested {
                        break;
                    }
                    return Err(MsgError::UnmatchedBraces);
                }
                '#' if in_plural => {
                    if !text.is_empty() {
                        nodes.push(Node::Text(std::mem::take(&mut text)));
                    }
                    nodes.push(Node::Pound);
                    self.i += 1;
                }
                _ => {
                    text.push(ch);
                    self.i += 1;
                }
            }
        }
        if !text.is_empty() {
            nodes.push(Node::Text(text));
        }
        Ok(nodes)
    }

    fn word(&mut self) -> String {
        let mut s = String::new();
        while self.i < self.c.len() && (self.c[self.i].is_alphanumeric() || self.c[self.i] == '_') {
            s.push(self.c[self.i]);
            self.i += 1;
        }
        s
    }

    /// After `{`: the argument through its `}`.
    fn argument(&mut self) -> Result<(ArgName, ArgKind), MsgError> {
        let start = self.i;
        self.skip_ws();
        let name_start = self.i;
        let w = self.word();
        if w.is_empty() {
            if self.i >= self.c.len() {
                return Err(MsgError::UnmatchedBraces);
            }
            return Err(self.syntax(start));
        }
        let name = if w.chars().all(|c| c.is_ascii_digit()) {
            if w.len() > 1 && w.starts_with('0') {
                return Err(self.syntax(start));
            }
            ArgName::Index(w.parse().map_err(|_| self.syntax(start))?)
        } else if w.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return Err(self.syntax(start));
        } else {
            ArgName::Name(w)
        };
        let _ = name_start;
        self.skip_ws();
        match self.c.get(self.i) {
            None => return Err(MsgError::UnmatchedBraces),
            Some('}') => {
                self.i += 1;
                return Ok((name, ArgKind::None));
            }
            Some(',') => self.i += 1,
            Some(_) => return Err(self.syntax(start)),
        }
        self.skip_ws();
        let ty = self.word();
        if ty.is_empty() {
            if self.i >= self.c.len() {
                return Err(MsgError::UnmatchedBraces);
            }
            return Err(self.syntax(start));
        }
        self.skip_ws();
        let lty = ty.to_ascii_lowercase();
        match lty.as_str() {
            "plural" | "selectordinal" | "select" | "choice" => {
                match self.c.get(self.i) {
                    Some(',') => self.i += 1,
                    None => return Err(MsgError::UnmatchedBraces),
                    Some(_) => return Err(self.syntax(start)),
                }
                let kind = match lty.as_str() {
                    "choice" => self.choice(start)?,
                    "select" => self.select(start)?,
                    _ => self.plural(start, lty == "selectordinal")?,
                };
                self.skip_ws();
                match self.c.get(self.i) {
                    Some('}') => {
                        self.i += 1;
                        Ok((name, kind))
                    }
                    None => Err(MsgError::UnmatchedBraces),
                    Some(_) => Err(self.syntax(start)),
                }
            }
            "number" | "date" | "time" | "spellout" | "ordinal" | "duration" => {
                let style = match self.c.get(self.i) {
                    Some('}') => {
                        self.i += 1;
                        None
                    }
                    Some(',') => {
                        self.i += 1;
                        // the style text, with quoting and nested braces
                        let mut s = String::new();
                        let mut depth = 0;
                        loop {
                            match self.c.get(self.i) {
                                None => return Err(MsgError::UnmatchedBraces),
                                Some('\'') => {
                                    s.push('\'');
                                    self.i += 1;
                                    while self.i < self.c.len() && self.c[self.i] != '\'' {
                                        s.push(self.c[self.i]);
                                        self.i += 1;
                                    }
                                    if self.i < self.c.len() {
                                        s.push('\'');
                                        self.i += 1;
                                    }
                                }
                                Some('{') => {
                                    depth += 1;
                                    s.push('{');
                                    self.i += 1;
                                }
                                Some('}') => {
                                    if depth == 0 {
                                        self.i += 1;
                                        break;
                                    }
                                    depth -= 1;
                                    s.push('}');
                                    self.i += 1;
                                }
                                Some(c) => {
                                    s.push(*c);
                                    self.i += 1;
                                }
                            }
                        }
                        Some(s.trim().to_string())
                    }
                    None => return Err(MsgError::UnmatchedBraces),
                    Some(_) => return Err(self.syntax(start)),
                };
                Ok((name, ArgKind::Simple(lty, style)))
            }
            _ => {
                // an unknown type: ICU fails with U_ILLEGAL_ARGUMENT_ERROR
                // (ext/intl/tests/msgfmt_errors.phpt); skip to the brace
                // to tell unmatched braces apart
                let mut depth = 0;
                while let Some(c) = self.c.get(self.i) {
                    match c {
                        '{' => depth += 1,
                        '}' if depth == 0 => {
                            return Err(MsgError::IllegalArgument);
                        }
                        '}' => depth -= 1,
                        _ => {}
                    }
                    self.i += 1;
                }
                Err(MsgError::UnmatchedBraces)
            }
        }
    }

    fn sub_message(&mut self, start: usize, in_plural: bool) -> Result<Vec<Node>, MsgError> {
        self.skip_ws();
        match self.c.get(self.i) {
            Some('{') => self.i += 1,
            None => return Err(MsgError::UnmatchedBraces),
            Some(_) => return Err(self.syntax(start)),
        }
        let m = self.message(true, in_plural)?;
        match self.c.get(self.i) {
            Some('}') => {
                self.i += 1;
                Ok(m)
            }
            _ => Err(MsgError::UnmatchedBraces),
        }
    }

    fn plural(&mut self, start: usize, ordinal: bool) -> Result<ArgKind, MsgError> {
        let mut offset = 0.0;
        let mut cases = Vec::new();
        loop {
            self.skip_ws();
            match self.c.get(self.i) {
                None => return Err(MsgError::UnmatchedBraces),
                Some('}') => break,
                Some('=') => {
                    self.i += 1;
                    let mut n = String::new();
                    while let Some(c) = self.c.get(self.i) {
                        if c.is_ascii_digit() || *c == '.' || *c == '-' {
                            n.push(*c);
                            self.i += 1;
                        } else {
                            break;
                        }
                    }
                    let v: f64 = n.parse().map_err(|_| self.syntax(start))?;
                    let m = self.sub_message(start, true)?;
                    cases.push((Selector::Exact(v), m));
                }
                Some(_) => {
                    let w = self.word();
                    if w.is_empty() {
                        return Err(self.syntax(start));
                    }
                    if w == "offset" && self.c.get(self.i) == Some(&':') && cases.is_empty() {
                        self.i += 1;
                        self.skip_ws();
                        let mut n = String::new();
                        while let Some(c) = self.c.get(self.i) {
                            if c.is_ascii_digit() || *c == '.' || *c == '-' {
                                n.push(*c);
                                self.i += 1;
                            } else {
                                break;
                            }
                        }
                        offset = n.parse().map_err(|_| self.syntax(start))?;
                        continue;
                    }
                    let m = self.sub_message(start, true)?;
                    cases.push((Selector::Keyword(w), m));
                }
            }
        }
        if !cases
            .iter()
            .any(|(s, _)| matches!(s, Selector::Keyword(k) if k == "other"))
        {
            return Err(self.syntax(start));
        }
        Ok(ArgKind::Plural {
            ordinal,
            offset,
            cases,
        })
    }

    fn select(&mut self, start: usize) -> Result<ArgKind, MsgError> {
        let mut cases = Vec::new();
        loop {
            self.skip_ws();
            match self.c.get(self.i) {
                None => return Err(MsgError::UnmatchedBraces),
                Some('}') => break,
                Some(_) => {
                    let w = self.word();
                    if w.is_empty() {
                        return Err(self.syntax(start));
                    }
                    let m = self.sub_message(start, false)?;
                    cases.push((w, m));
                }
            }
        }
        if !cases.iter().any(|(k, _)| k == "other") {
            return Err(self.syntax(start));
        }
        Ok(ArgKind::Select(cases))
    }

    fn choice(&mut self, start: usize) -> Result<ArgKind, MsgError> {
        let mut cases = Vec::new();
        loop {
            self.skip_ws();
            let mut n = String::new();
            while let Some(c) = self.c.get(self.i) {
                if c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E' | '∞') {
                    n.push(*c);
                    self.i += 1;
                } else {
                    break;
                }
            }
            if n.is_empty() {
                return Err(self.syntax(start));
            }
            let v: f64 = match n.as_str() {
                "∞" => f64::INFINITY,
                "-∞" => f64::NEG_INFINITY,
                _ => n.parse().map_err(|_| self.syntax(start))?,
            };
            self.skip_ws();
            let strict = match self.c.get(self.i) {
                Some('#') | Some('≤') => false,
                Some('<') => true,
                None => return Err(MsgError::UnmatchedBraces),
                _ => return Err(self.syntax(start)),
            };
            self.i += 1;
            // the sub-message up to '|' or the closing brace
            let mut m = Vec::new();
            let mut text = String::new();
            loop {
                match self.c.get(self.i) {
                    None => return Err(MsgError::UnmatchedBraces),
                    Some('|') | Some('}') => break,
                    Some('\'') => {
                        let next = self.c.get(self.i + 1).copied();
                        if next == Some('\'') {
                            text.push('\'');
                            self.i += 2;
                        } else if matches!(next, Some('{') | Some('}') | Some('|')) {
                            self.i += 1;
                            while self.i < self.c.len() && self.c[self.i] != '\'' {
                                text.push(self.c[self.i]);
                                self.i += 1;
                            }
                            self.i += 1;
                        } else {
                            text.push('\'');
                            self.i += 1;
                        }
                    }
                    Some('{') => {
                        if !text.is_empty() {
                            m.push(Node::Text(std::mem::take(&mut text)));
                        }
                        self.i += 1;
                        let (name, kind) = self.argument()?;
                        m.push(Node::Arg(name, kind));
                    }
                    Some(c) => {
                        text.push(*c);
                        self.i += 1;
                    }
                }
            }
            if !text.is_empty() {
                m.push(Node::Text(text));
            }
            cases.push((v, strict, m));
            if self.c.get(self.i) == Some(&'|') {
                self.i += 1;
                continue;
            }
            break;
        }
        Ok(ArgKind::Choice(cases))
    }
}

impl MessagePattern {
    pub fn parse(pattern: &str) -> Result<MessagePattern, MsgError> {
        let c: Vec<char> = pattern.chars().collect();
        let mut p = Parser { c: &c, i: 0 };
        let nodes = p.message(false, false)?;
        Ok(MessagePattern { nodes })
    }

    /// The arguments with their type classes, in order of appearance;
    /// an error for an argument declared with two classes.
    pub fn arg_types(&self) -> Result<Vec<(ArgName, ArgClass)>, InconsistentTypes> {
        fn walk(
            nodes: &[Node],
            out: &mut Vec<(ArgName, ArgClass)>,
        ) -> Result<(), InconsistentTypes> {
            for n in nodes {
                if let Node::Arg(name, kind) = n {
                    let class = match kind {
                        ArgKind::None => ArgClass::Any,
                        ArgKind::Simple(t, _) => match t.as_str() {
                            "date" | "time" => ArgClass::Date,
                            _ => ArgClass::Number,
                        },
                        ArgKind::Plural { .. } | ArgKind::Choice(_) => ArgClass::Number,
                        ArgKind::Select(_) => ArgClass::String,
                    };
                    match out.iter().find(|(n, _)| n == name) {
                        Some((_, c)) if *c != class => return Err(InconsistentTypes),
                        Some(_) => {}
                        None => out.push((name.clone(), class)),
                    }
                    match kind {
                        ArgKind::Plural { cases, .. } => {
                            for (_, m) in cases {
                                walk(m, out)?;
                            }
                        }
                        ArgKind::Select(cases) => {
                            for (_, m) in cases {
                                walk(m, out)?;
                            }
                        }
                        ArgKind::Choice(cases) => {
                            for (_, _, m) in cases {
                                walk(m, out)?;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(())
        }
        let mut out = Vec::new();
        walk(&self.nodes, &mut out)?;
        Ok(out)
    }

    /// Whether the arguments are numbered (else named).
    pub fn uses_numbers(&self) -> bool {
        self.arg_types()
            .map(|v| v.iter().all(|(n, _)| matches!(n, ArgName::Index(_))))
            .unwrap_or(true)
    }
}

/// An argument value.
#[derive(Clone, Debug)]
pub enum MsgValue {
    Int(i64),
    Double(f64),
    Str(String),
    /// Milliseconds since the epoch.
    Date(f64),
}

impl MsgValue {
    fn as_f64(&self) -> Option<f64> {
        match self {
            MsgValue::Int(i) => Some(*i as f64),
            MsgValue::Double(d) => Some(*d),
            MsgValue::Date(d) => Some(*d),
            MsgValue::Str(s) => s.trim().parse().ok(),
        }
    }
}

/// Formats dates and times for `date`/`time` arguments: (type, style,
/// milliseconds) → text.
pub type DateHook<'a> = &'a dyn Fn(&str, Option<&str>, f64) -> String;

/// Formats `pat` with `args` (looked up by name).
pub fn format(
    locale: &str,
    pat: &MessagePattern,
    args: &dyn Fn(&ArgName) -> Option<MsgValue>,
    date: DateHook<'_>,
) -> String {
    let mut out = String::new();
    format_nodes(locale, &pat.nodes, args, date, None, &mut out);
    out
}

fn number_format(locale: &str, style: Option<&str>) -> Option<NumberFormat> {
    let (st, pattern): (i64, Option<String>) = match style {
        None => (nf::DECIMAL, None),
        Some("integer") => (nf::DECIMAL, None),
        Some("currency") => (nf::CURRENCY, None),
        Some("percent") => (nf::PERCENT, None),
        Some(s) if s.starts_with("::") => return skeleton_format(locale, &s[2..]),
        Some(s) => (nf::PATTERN_DECIMAL, Some(s.to_string())),
    };
    let mut f = NumberFormat::new(locale, st, pattern.as_deref()).ok()?;
    if style == Some("integer") {
        f.set_attribute(nf::MAX_FRACTION_DIGITS, 0.0);
        f.parse_int_only = true;
    }
    Some(f)
}

/// ICU number skeletons (ICU user guide "Number Skeletons"), the common
/// stems.
fn skeleton_format(locale: &str, sk: &str) -> Option<NumberFormat> {
    let mut f = NumberFormat::new(locale, nf::DECIMAL, None).ok()?;
    for stem in sk.split_whitespace() {
        if let Some(cur) = stem.strip_prefix("currency/") {
            let mut g = NumberFormat::new(locale, nf::CURRENCY, None).ok()?;
            g.set_text_attribute(nf::CURRENCY_CODE, cur);
            f = g;
        } else if stem == "percent" || stem == "%" {
            f = NumberFormat::new(locale, nf::PERCENT, None).ok()?;
        } else if stem == "integer" || stem == "precision-integer" || stem == "." {
            f.set_attribute(nf::MAX_FRACTION_DIGITS, 0.0);
        } else if stem == "compact-short" || stem == "K" {
            f = NumberFormat::new(locale, nf::DECIMAL_COMPACT_SHORT, None).ok()?;
        } else if stem == "compact-long" || stem == "KK" {
            f = NumberFormat::new(locale, nf::DECIMAL_COMPACT_LONG, None).ok()?;
        } else if stem == "group-off" || stem == ",_" {
            f.set_attribute(nf::GROUPING_USED, 0.0);
        } else if let Some(fr) = stem.strip_prefix('.') {
            let min = fr.chars().filter(|c| *c == '0').count() as f64;
            let max = min + fr.chars().filter(|c| *c == '#').count() as f64;
            let plus = fr.ends_with('*') || fr.ends_with('+');
            f.set_attribute(nf::MIN_FRACTION_DIGITS, min);
            f.set_attribute(nf::MAX_FRACTION_DIGITS, if plus { 999.0 } else { max });
        } else if stem.starts_with('@') {
            let min = stem.chars().filter(|c| *c == '@').count() as f64;
            let max = min + stem.chars().filter(|c| *c == '#').count() as f64;
            f.set_attribute(nf::MIN_SIGNIFICANT_DIGITS, min);
            f.set_attribute(nf::MAX_SIGNIFICANT_DIGITS, max);
        } else if let Some(m) = stem.strip_prefix("scale/")
            && let Ok(v) = m.parse::<f64>()
        {
            f.set_attribute(nf::MULTIPLIER, v);
        }
    }
    Some(f)
}

fn format_number(f: &NumberFormat, v: &MsgValue) -> String {
    match v {
        MsgValue::Int(i) => f.format_i64(*i).unwrap_or_default(),
        other => f
            .format_f64(other.as_f64().unwrap_or(0.0))
            .unwrap_or_default(),
    }
}

fn rbnf(locale: &str, style: i64, v: &MsgValue) -> String {
    let f = match NumberFormat::new(locale, style, None) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    format_number(&f, v)
}

fn format_nodes(
    locale: &str,
    nodes: &[Node],
    args: &dyn Fn(&ArgName) -> Option<MsgValue>,
    date: DateHook<'_>,
    pound: Option<&str>,
    out: &mut String,
) {
    for n in nodes {
        match n {
            Node::Text(t) => out.push_str(t),
            Node::Pound => out.push_str(pound.unwrap_or("#")),
            Node::Arg(name, kind) => {
                let Some(v) = args(name) else {
                    out.push('{');
                    out.push_str(&name.to_string());
                    out.push('}');
                    continue;
                };
                match kind {
                    ArgKind::None => match &v {
                        MsgValue::Str(s) => out.push_str(s),
                        MsgValue::Date(ms) => out.push_str(&date("datetime", None, *ms)),
                        other => {
                            if let Some(f) = number_format(locale, None) {
                                out.push_str(&format_number(&f, other));
                            }
                        }
                    },
                    ArgKind::Simple(t, style) => match t.as_str() {
                        "number" => {
                            // PHP passes integer-style arguments as int64
                            // (ext/intl/tests/msgfmt_format_simple_types_
                            // numeric_strings_icu72-1.phpt: 965.5 is 965)
                            let v = match (&v, style.as_deref()) {
                                (MsgValue::Double(d), Some("integer")) => {
                                    MsgValue::Int(d.trunc() as i64)
                                }
                                _ => v.clone(),
                            };
                            if let Some(f) = number_format(locale, style.as_deref()) {
                                out.push_str(&format_number(&f, &v));
                            }
                        }
                        "date" | "time" => {
                            let ms = v.as_f64().unwrap_or(0.0);
                            out.push_str(&date(t, style.as_deref(), ms));
                        }
                        "spellout" => out.push_str(&rbnf(locale, nf::SPELLOUT, &v)),
                        "ordinal" => out.push_str(&rbnf(locale, nf::ORDINAL, &v)),
                        "duration" => out.push_str(&rbnf(locale, nf::DURATION, &v)),
                        _ => {}
                    },
                    ArgKind::Plural {
                        ordinal,
                        offset,
                        cases,
                    } => {
                        let x = v.as_f64().unwrap_or(0.0);
                        let chosen = cases
                            .iter()
                            .find(|(s, _)| matches!(s, Selector::Exact(e) if *e == x))
                            .or_else(|| {
                                let y = x - offset;
                                let cat = plural_keyword(locale, y, *ordinal);
                                cases
                                    .iter()
                                    .find(|(s, _)| matches!(s, Selector::Keyword(k) if *k == cat))
                            })
                            .or_else(|| {
                                cases.iter().find(
                                    |(s, _)| matches!(s, Selector::Keyword(k) if k == "other"),
                                )
                            });
                        if let Some((_, m)) = chosen {
                            let y = x - offset;
                            let p = number_format(locale, None)
                                .map(|f| {
                                    if y.fract() == 0.0 && y.abs() < 9e15 {
                                        f.format_i64(y as i64).unwrap_or_default()
                                    } else {
                                        f.format_f64(y).unwrap_or_default()
                                    }
                                })
                                .unwrap_or_default();
                            format_nodes(locale, m, args, date, Some(&p), out);
                        }
                    }
                    ArgKind::Select(cases) => {
                        let key = match &v {
                            MsgValue::Str(s) => s.clone(),
                            MsgValue::Int(i) => i.to_string(),
                            MsgValue::Double(d) => d.to_string(),
                            MsgValue::Date(d) => d.to_string(),
                        };
                        let chosen = cases
                            .iter()
                            .find(|(k, _)| *k == key)
                            .or_else(|| cases.iter().find(|(k, _)| k == "other"));
                        if let Some((_, m)) = chosen {
                            format_nodes(locale, m, args, date, pound, out);
                        }
                    }
                    ArgKind::Choice(cases) => {
                        let x = v.as_f64().unwrap_or(0.0);
                        let mut chosen = &cases[0].2;
                        for (limit, strict, m) in cases {
                            if (*strict && x > *limit) || (!*strict && x >= *limit) {
                                chosen = m;
                            }
                        }
                        format_nodes(locale, chosen, args, date, pound, out);
                    }
                }
            }
        }
    }
}

/// The CLDR plural category keyword of `x`.
fn plural_keyword(locale: &str, x: f64, ordinal: bool) -> String {
    use intl::plural::{PluralCategory as C, PluralOperands, ordinal_category, plural_category};
    let lang = locale::parse(locale).language;
    let s = Dec::from_f64(x).plain();
    let ops = PluralOperands::parse(&s).unwrap_or_else(|| PluralOperands::from_int(x as i64));
    let c = if ordinal {
        ordinal_category(&lang, &ops)
    } else {
        plural_category(&lang, &ops)
    };
    match c {
        C::Zero => "zero",
        C::One => "one",
        C::Two => "two",
        C::Few => "few",
        C::Many => "many",
        C::Other => "other",
    }
    .to_string()
}

/// The date pattern of a `date`/`time` argument (ICU user guide
/// "Formatting Messages": the short/medium/long/full styles, a pattern or
/// a `::` skeleton; "datetime" for a date in a plain argument, which ICU
/// shows in the short styles).
pub fn date_pattern(locale: &str, ty: &str, style: Option<&str>) -> String {
    use crate::datefmt;
    let level = |s: Option<&str>| match s {
        None | Some("medium") => Some(2),
        Some("short") => Some(3),
        Some("long") => Some(1),
        Some("full") => Some(0),
        _ => None,
    };
    let style = style.map(str::trim);
    match ty {
        "datetime" => datefmt::style_pattern(locale, 3, 3),
        _ => match level(style) {
            Some(l) if ty == "time" => datefmt::style_pattern(locale, -1, l),
            Some(l) => datefmt::style_pattern(locale, l, -1),
            None => {
                let s = style.unwrap_or("");
                match s.strip_prefix("::") {
                    Some(sk) => datefmt::skeleton_pattern(locale, sk),
                    None => s.to_string(),
                }
            }
        },
    }
}

/// Parses `text` against `pat` (`MessageFormatter::parse()`): the
/// values of the number, date and string arguments in order of their
/// names (dates read in the time zone `tz_id`).
pub fn parse(
    locale: &str,
    pat: &MessagePattern,
    text: &str,
    tz_id: &str,
) -> Option<Vec<(ArgName, MsgValue)>> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut out: Vec<(ArgName, MsgValue)> = Vec::new();
    let nodes = &pat.nodes;
    for (k, n) in nodes.iter().enumerate() {
        match n {
            Node::Text(t) => {
                let tc: Vec<char> = t.chars().collect();
                if chars.len() < i + tc.len() || chars[i..i + tc.len()] != tc[..] {
                    return None;
                }
                i += tc.len();
            }
            Node::Arg(name, ArgKind::Simple(t, style)) if t == "number" => {
                let f = number_format(locale, style.as_deref())?;
                let rest: String = chars[i..].iter().collect();
                let (v, int, end) = f.parse(&rest, 0)?;
                i += end;
                let val = if int && v.abs() < 9.2e18 {
                    MsgValue::Int(v as i64)
                } else {
                    MsgValue::Double(v)
                };
                out.push((name.clone(), val));
            }
            Node::Arg(name, ArgKind::Simple(t, style)) if t == "date" || t == "time" => {
                let p = date_pattern(locale, t, style.as_deref());
                let rest: String = chars[i..].iter().collect();
                let (ms, end) = crate::datefmt::parse_pattern(locale, &p, &rest, tz_id)?;
                i += rest[..end].chars().count();
                out.push((name.clone(), MsgValue::Date(ms)));
            }
            Node::Arg(name, ArgKind::None) => {
                // up to the next literal text
                let next = nodes.get(k + 1).and_then(|n| match n {
                    Node::Text(t) => Some(t.clone()),
                    _ => None,
                });
                let rest: String = chars[i..].iter().collect();
                let end = match next {
                    Some(t) => rest.find(&t)?,
                    None => rest.len(),
                };
                let s = &rest[..end];
                i += s.chars().count();
                out.push((name.clone(), MsgValue::Str(s.to_string())));
            }
            _ => return None,
        }
    }
    if i != chars.len() {
        return None;
    }
    Some(out)
}
