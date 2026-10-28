//! UnicodeSet patterns (ICU user guide "UnicodeSet"; UTS #35 section
//! 3.6.1 / UTS #18 property syntax): `[a-z]`, `[^...]`, nested sets with
//! union, `&` intersection and `-` difference, `[:Name:]`, `[:^Name:]`,
//! `\p{Name}`, `\P{Name}`, `\uXXXX`, `\x{...}` escapes and quoted
//! literals. Properties are General_Category values, scripts
//! (UCD PropertyValueAliases.txt) and common binary properties.

use intl::unicode::GeneralCategory as G;

/// A set of code points.
#[derive(Clone, Debug)]
pub enum USet {
    Ranges(Vec<(u32, u32)>),
    Prop(Prop),
    Not(Box<USet>),
    Union(Vec<USet>),
    Inter(Box<USet>, Box<USet>),
    Diff(Box<USet>, Box<USet>),
}

#[derive(Clone, Copy, Debug)]
pub enum Prop {
    Any,
    Gc(&'static [G]),
    Script(intl::unicode::Script),
    Alphabetic,
    WhiteSpace,
    Uppercase,
    Lowercase,
    DefaultIgnorable,
    Ascii,
    Assigned,
    BidiMirrored,
    Math,
    Dash,
    Diacritic,
    HexDigit,
    QuotationMark,
    JoinControl,
}

impl USet {
    pub fn contains(&self, c: char) -> bool {
        match self {
            USet::Ranges(r) => {
                let cp = c as u32;
                r.iter().any(|(a, b)| *a <= cp && cp <= *b)
            }
            USet::Prop(p) => prop_has(*p, c),
            USet::Not(s) => !s.contains(c),
            USet::Union(v) => v.iter().any(|s| s.contains(c)),
            USet::Inter(a, b) => a.contains(c) && b.contains(c),
            USet::Diff(a, b) => a.contains(c) && !b.contains(c),
        }
    }
}

fn prop_has(p: Prop, c: char) -> bool {
    use intl::unicode as u;
    match p {
        Prop::Any => true,
        Prop::Gc(list) => list.contains(&u::general_category(c)),
        Prop::Script(s) => u::script(c) == s,
        Prop::Alphabetic => u::is_alphabetic(c),
        Prop::WhiteSpace => u::is_whitespace(c),
        Prop::Uppercase => u::is_uppercase(c),
        Prop::Lowercase => u::is_lowercase(c),
        Prop::DefaultIgnorable => u::is_default_ignorable(c),
        Prop::Ascii => (c as u32) < 0x80,
        Prop::Assigned => u::is_assigned(c),
        Prop::BidiMirrored => u::is_bidi_mirrored(c),
        Prop::Math => u::is_math(c),
        Prop::Dash => u::is_dash(c),
        Prop::Diacritic => u::is_diacritic(c),
        Prop::HexDigit => u::is_hex_digit(c),
        Prop::QuotationMark => u::is_quotation_mark(c),
        Prop::JoinControl => u::is_join_control(c),
    }
}

/// Loose matching of property names (UAX #44 LM3: case, spaces,
/// underscores and hyphens ignored).
fn loose(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

const L: &[G] = &[
    G::UppercaseLetter,
    G::LowercaseLetter,
    G::TitlecaseLetter,
    G::ModifierLetter,
    G::OtherLetter,
];
const LC: &[G] = &[G::UppercaseLetter, G::LowercaseLetter, G::TitlecaseLetter];
const M: &[G] = &[G::NonspacingMark, G::SpacingMark, G::EnclosingMark];
const N: &[G] = &[G::DecimalNumber, G::LetterNumber, G::OtherNumber];
const P: &[G] = &[
    G::ConnectorPunctuation,
    G::DashPunctuation,
    G::OpenPunctuation,
    G::ClosePunctuation,
    G::InitialPunctuation,
    G::FinalPunctuation,
    G::OtherPunctuation,
];
const S: &[G] = &[G::MathSymbol, G::CurrencySymbol, G::ModifierSymbol, G::OtherSymbol];
const Z: &[G] = &[G::SpaceSeparator, G::LineSeparator, G::ParagraphSeparator];
const C: &[G] = &[G::Control, G::Format, G::Unassigned, G::PrivateUse, G::Surrogate];

fn gc_value(n: &str) -> Option<&'static [G]> {
    Some(match n {
        "l" | "letter" => L,
        "lc" | "casedletter" => LC,
        "lu" | "uppercaseletter" => &[G::UppercaseLetter],
        "ll" | "lowercaseletter" => &[G::LowercaseLetter],
        "lt" | "titlecaseletter" => &[G::TitlecaseLetter],
        "lm" | "modifierletter" => &[G::ModifierLetter],
        "lo" | "otherletter" => &[G::OtherLetter],
        "m" | "mark" | "combiningmark" => M,
        "mn" | "nonspacingmark" => &[G::NonspacingMark],
        "mc" | "spacingmark" => &[G::SpacingMark],
        "me" | "enclosingmark" => &[G::EnclosingMark],
        "n" | "number" => N,
        "nd" | "decimalnumber" | "digit" => &[G::DecimalNumber],
        "nl" | "letternumber" => &[G::LetterNumber],
        "no" | "othernumber" => &[G::OtherNumber],
        "p" | "punctuation" | "punct" => P,
        "pc" | "connectorpunctuation" => &[G::ConnectorPunctuation],
        "pd" | "dashpunctuation" => &[G::DashPunctuation],
        "ps" | "openpunctuation" => &[G::OpenPunctuation],
        "pe" | "closepunctuation" => &[G::ClosePunctuation],
        "pi" | "initialpunctuation" => &[G::InitialPunctuation],
        "pf" | "finalpunctuation" => &[G::FinalPunctuation],
        "po" | "otherpunctuation" => &[G::OtherPunctuation],
        "s" | "symbol" => S,
        "sm" | "mathsymbol" => &[G::MathSymbol],
        "sc" | "currencysymbol" => &[G::CurrencySymbol],
        "sk" | "modifiersymbol" => &[G::ModifierSymbol],
        "so" | "othersymbol" => &[G::OtherSymbol],
        "z" | "separator" => Z,
        "zs" | "spaceseparator" => &[G::SpaceSeparator],
        "zl" | "lineseparator" => &[G::LineSeparator],
        "zp" | "paragraphseparator" => &[G::ParagraphSeparator],
        "c" | "other" => C,
        "cc" | "control" | "cntrl" => &[G::Control],
        "cf" | "format" => &[G::Format],
        "cn" | "unassigned" => &[G::Unassigned],
        "co" | "privateuse" => &[G::PrivateUse],
        "cs" | "surrogate" => &[G::Surrogate],
        _ => return None,
    })
}

/// A property by name (`[:Name:]`, `\p{Name}`, `\p{gc=Lu}`,
/// `\p{Script=Latin}`).
pub fn property(name: &str) -> Option<USet> {
    let (k, v) = match name.split_once(['=', ':']) {
        Some((k, v)) => (loose(k), loose(v)),
        None => (String::new(), loose(name)),
    };
    let p = match k.as_str() {
        "gc" | "generalcategory" => Prop::Gc(gc_value(&v)?),
        "sc" | "script" | "scx" | "scriptextensions" => Prop::Script(script_cached(&v)?),
        "" => {
            if let Some(g) = gc_value(&v) {
                Prop::Gc(g)
            } else {
                match v.as_str() {
                    "any" => Prop::Any,
                    "ascii" => Prop::Ascii,
                    "assigned" => Prop::Assigned,
                    "alphabetic" | "alpha" => Prop::Alphabetic,
                    "whitespace" | "space" | "wspace" => Prop::WhiteSpace,
                    "uppercase" | "upper" => Prop::Uppercase,
                    "lowercase" | "lower" => Prop::Lowercase,
                    "defaultignorablecodepoint" | "di" => Prop::DefaultIgnorable,
                    "bidimirrored" | "bidim" => Prop::BidiMirrored,
                    "math" => Prop::Math,
                    "dash" => Prop::Dash,
                    "diacritic" | "dia" => Prop::Diacritic,
                    "hexdigit" | "hex" => Prop::HexDigit,
                    "quotationmark" | "qmark" => Prop::QuotationMark,
                    "joincontrol" | "joinc" => Prop::JoinControl,
                    _ => Prop::Script(script_cached(&v)?),
                }
            }
        }
        _ => return None,
    };
    Some(USet::Prop(p))
}

/// Scripts by loose name, from one scan of the code points.
fn script_cached(key: &str) -> Option<intl::unicode::Script> {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    static M: OnceLock<HashMap<String, intl::unicode::Script>> = OnceLock::new();
    let m = M.get_or_init(|| {
        let mut by_long: HashMap<&'static str, intl::unicode::Script> = HashMap::new();
        let mut cp = 0u32;
        while cp < 0x110000 {
            if let Some(c) = char::from_u32(cp) {
                let s = intl::unicode::script(c);
                by_long.entry(s.long_name()).or_insert(s);
            }
            cp += 1;
        }
        let mut m = HashMap::new();
        for l in include_str!("data/scripts.txt").lines() {
            if let Some((code, name)) = l.split_once('\t')
                && let Some(s) = by_long.get(name)
            {
                m.insert(loose(code), *s);
                m.insert(loose(name), *s);
            }
        }
        m
    });
    m.get(key).copied()
}

/// A pattern error at a char index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetError(pub usize);

/// Parses a set at `c[*i]` (which is `[` or `\p`/`\P`).
pub fn parse(c: &[char], i: &mut usize) -> Result<USet, SetError> {
    if c.get(*i) == Some(&'\\') && matches!(c.get(*i + 1), Some('p') | Some('P')) {
        let neg = c[*i + 1] == 'P';
        *i += 2;
        if c.get(*i) != Some(&'{') {
            return Err(SetError(*i));
        }
        let start = *i + 1;
        let end = c[start..].iter().position(|x| *x == '}').ok_or(SetError(*i))? + start;
        let name: String = c[start..end].iter().collect();
        *i = end + 1;
        let (neg2, name) = match name.strip_prefix('^') {
            Some(n) => (true, n.to_string()),
            None => (false, name),
        };
        let p = property(&name).ok_or(SetError(start))?;
        return Ok(if neg ^ neg2 { USet::Not(Box::new(p)) } else { p });
    }
    if c.get(*i) != Some(&'[') {
        return Err(SetError(*i));
    }
    // [:Name:]
    if c.get(*i + 1) == Some(&':') {
        let start = *i + 2;
        let rel = c[start..].windows(2).position(|w| w == [':', ']']).ok_or(SetError(*i))?;
        let name: String = c[start..start + rel].iter().collect();
        *i = start + rel + 2;
        let (neg, name) = match name.strip_prefix('^') {
            Some(n) => (true, n.to_string()),
            None => (false, name),
        };
        let p = property(&name).ok_or(SetError(start))?;
        return Ok(if neg { USet::Not(Box::new(p)) } else { p });
    }
    *i += 1;
    let mut neg = false;
    if c.get(*i) == Some(&'^') {
        neg = true;
        *i += 1;
    }
    let mut items: Vec<USet> = Vec::new();
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    let mut pending_op: Option<char> = None;
    let mut acc: Option<USet> = None;
    let flush = |items: &mut Vec<USet>, ranges: &mut Vec<(u32, u32)>| {
        if !ranges.is_empty() {
            items.push(USet::Ranges(std::mem::take(ranges)));
        }
    };
    loop {
        let Some(&ch) = c.get(*i) else {
            return Err(SetError(*i));
        };
        match ch {
            ']' => {
                *i += 1;
                break;
            }
            ' ' | '\t' | '\n' | '\r' => {
                *i += 1;
            }
            '[' | '\\'
                if ch == '[' || matches!(c.get(*i + 1), Some('p') | Some('P')) =>
            {
                let s = parse(c, i)?;
                match pending_op.take() {
                    Some(op) => {
                        flush(&mut items, &mut ranges);
                        let left = acc.take().unwrap_or_else(|| USet::Union(std::mem::take(&mut items)));
                        acc = Some(if op == '&' {
                            USet::Inter(Box::new(left), Box::new(s))
                        } else {
                            USet::Diff(Box::new(left), Box::new(s))
                        });
                    }
                    None => {
                        if let Some(a) = acc.take() {
                            items.push(a);
                        }
                        items.push(s);
                    }
                }
            }
            '&' | '-' if matches!(c.get(*i + 1), Some('[') | Some('\\')) && (!items.is_empty() || !ranges.is_empty() || acc.is_some()) && (ch == '&' || c.get(*i + 1) == Some(&'[')) => {
                pending_op = Some(ch);
                *i += 1;
            }
            _ => {
                let a = literal(c, i)?;
                // a range a-b
                if c.get(*i) == Some(&'-') && c.get(*i + 1).is_some_and(|x| *x != ']' && *x != '[') {
                    *i += 1;
                    let b = literal(c, i)?;
                    if b < a {
                        return Err(SetError(*i));
                    }
                    ranges.push((a, b));
                } else {
                    ranges.push((a, a));
                }
            }
        }
    }
    flush(&mut items, &mut ranges);
    if let Some(a) = acc {
        items.push(a);
    }
    let s = if items.len() == 1 {
        items.pop().unwrap()
    } else {
        USet::Union(items)
    };
    Ok(if neg { USet::Not(Box::new(s)) } else { s })
}

/// A literal character of a set (escapes and quotes).
fn literal(c: &[char], i: &mut usize) -> Result<u32, SetError> {
    let ch = *c.get(*i).ok_or(SetError(*i))?;
    if ch == '\\' {
        let (cp, n) = escape(&c[*i + 1..]).ok_or(SetError(*i))?;
        *i += 1 + n;
        return Ok(cp);
    }
    if ch == '\'' {
        // 'x' or '' (a quote)
        if c.get(*i + 1) == Some(&'\'') {
            *i += 2;
            return Ok('\'' as u32);
        }
        let v = *c.get(*i + 1).ok_or(SetError(*i))?;
        *i += 2;
        if c.get(*i) == Some(&'\'') {
            *i += 1;
        }
        return Ok(v as u32);
    }
    *i += 1;
    Ok(ch as u32)
}

/// An escape after the backslash: the code point and the chars used.
pub fn escape(c: &[char]) -> Option<(u32, usize)> {
    let hex = |s: &[char]| -> Option<u32> {
        let t: String = s.iter().collect();
        u32::from_str_radix(&t, 16).ok()
    };
    match c.first()? {
        'u' => {
            if c.get(1) == Some(&'{') {
                let end = c.iter().position(|x| *x == '}')?;
                return Some((hex(&c[2..end])?, end + 1));
            }
            if c.len() < 5 {
                return None;
            }
            Some((hex(&c[1..5])?, 5))
        }
        'U' => {
            if c.len() < 9 {
                return None;
            }
            Some((hex(&c[1..9])?, 9))
        }
        'x' => {
            if c.get(1) == Some(&'{') {
                let end = c.iter().position(|x| *x == '}')?;
                return Some((hex(&c[2..end])?, end + 1));
            }
            if c.len() < 3 {
                return None;
            }
            Some((hex(&c[1..3])?, 3))
        }
        'n' => Some((10, 1)),
        't' => Some((9, 1)),
        'r' => Some((13, 1)),
        'f' => Some((12, 1)),
        'v' => Some((11, 1)),
        'a' => Some((7, 1)),
        'e' => Some((0x1b, 1)),
        other => Some((*other as u32, 1)),
    }
}

/// Parses a whole set pattern.
pub fn parse_str(s: &str) -> Result<USet, SetError> {
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    let set = parse(&c, &mut i)?;
    if i != c.len() {
        return Err(SetError(i));
    }
    Ok(set)
}
