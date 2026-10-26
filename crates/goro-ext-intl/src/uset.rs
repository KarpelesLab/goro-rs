//! A UnicodeSet pattern subset (ICU's `[...]`, `[:prop:]`, `\p{prop}`),
//! shared by the break iterator rules, the transliterator rules and
//! filters: sets are kept as expression trees evaluated per code point.

use std::cell::RefCell;
use std::collections::HashMap;

use intl::unicode as un;
use intl::unicode::{GeneralCategory, Script};

/// A set of code points.
#[derive(Clone, Debug)]
pub enum USet {
    Ranges(Vec<(u32, u32)>),
    /// A General_Category mask (bit = GeneralCategory as u8).
    Gc(u32),
    Script(Script),
    Scx(Script),
    Bin(fn(char) -> bool),
    /// A Canonical_Combining_Class value.
    Ccc(u8),
    /// A Block (its name, loosely matched).
    Block(String),
    Not(Box<USet>),
    Union(Vec<USet>),
    Inter(Box<USet>, Box<USet>),
    Diff(Box<USet>, Box<USet>),
}

impl USet {
    pub fn empty() -> USet {
        USet::Ranges(Vec::new())
    }
    pub fn all() -> USet {
        USet::Ranges(vec![(0, 0x10FFFF)])
    }
    pub fn contains(&self, cp: u32) -> bool {
        match self {
            USet::Ranges(r) => r.iter().any(|&(a, b)| a <= cp && cp <= b),
            USet::Gc(m) => m & (1 << gc_of(cp) as u32) != 0,
            USet::Script(s) => un::script_u32(cp) == *s,
            USet::Scx(s) => un::script_extensions_u32(cp).contains(*s),
            USet::Bin(f) => char::from_u32(cp).is_some_and(f),
            USet::Ccc(v) => un::canonical_combining_class_u32(cp) == *v,
            USet::Block(b) => char::from_u32(cp).is_some_and(|c| loose(un::block(c)) == *b),
            USet::Not(s) => !s.contains(cp),
            USet::Union(v) => v.iter().any(|s| s.contains(cp)),
            USet::Inter(a, b) => a.contains(cp) && b.contains(cp),
            USet::Diff(a, b) => a.contains(cp) && !b.contains(cp),
        }
    }
    pub fn has(&self, c: char) -> bool {
        self.contains(c as u32)
    }
    /// The set as sorted ranges (scans the code space for property sets).
    pub fn ranges(&self) -> Vec<(u32, u32)> {
        if let USet::Ranges(r) = self {
            let mut r = r.clone();
            r.sort();
            return merge(r);
        }
        let mut out: Vec<(u32, u32)> = Vec::new();
        let mut cp = 0u32;
        while cp <= 0x10FFFF {
            if self.contains(cp) {
                match out.last_mut() {
                    Some(l) if l.1 + 1 == cp => l.1 = cp,
                    _ => out.push((cp, cp)),
                }
            }
            cp += 1;
        }
        out
    }
}

fn merge(r: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for (a, b) in r {
        match out.last_mut() {
            Some(l) if a <= l.1.saturating_add(1) => l.1 = l.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

pub fn gc_of(cp: u32) -> GeneralCategory {
    un::general_category_u32(cp)
}

const fn bit(g: GeneralCategory) -> u32 {
    1 << g as u32
}

use GeneralCategory as G;
const M_L: u32 = bit(G::UppercaseLetter)
    | bit(G::LowercaseLetter)
    | bit(G::TitlecaseLetter)
    | bit(G::ModifierLetter)
    | bit(G::OtherLetter);
const M_LC: u32 = bit(G::UppercaseLetter) | bit(G::LowercaseLetter) | bit(G::TitlecaseLetter);
const M_M: u32 = bit(G::NonspacingMark) | bit(G::SpacingMark) | bit(G::EnclosingMark);
const M_N: u32 = bit(G::DecimalNumber) | bit(G::LetterNumber) | bit(G::OtherNumber);
const M_P: u32 = bit(G::ConnectorPunctuation)
    | bit(G::DashPunctuation)
    | bit(G::OpenPunctuation)
    | bit(G::ClosePunctuation)
    | bit(G::InitialPunctuation)
    | bit(G::FinalPunctuation)
    | bit(G::OtherPunctuation);
const M_S: u32 =
    bit(G::MathSymbol) | bit(G::CurrencySymbol) | bit(G::ModifierSymbol) | bit(G::OtherSymbol);
const M_Z: u32 = bit(G::SpaceSeparator) | bit(G::LineSeparator) | bit(G::ParagraphSeparator);
const M_C: u32 =
    bit(G::Control) | bit(G::Format) | bit(G::Surrogate) | bit(G::PrivateUse) | bit(G::Unassigned);

/// General_Category value names (short, long, other aliases) and masks.
pub const GC_NAMES: &[(&str, &str, &str, u32)] = &[
    ("L", "Letter", "", M_L),
    ("LC", "Cased_Letter", "L&", M_LC),
    ("Lu", "Uppercase_Letter", "", bit(G::UppercaseLetter)),
    ("Ll", "Lowercase_Letter", "", bit(G::LowercaseLetter)),
    ("Lt", "Titlecase_Letter", "", bit(G::TitlecaseLetter)),
    ("Lm", "Modifier_Letter", "", bit(G::ModifierLetter)),
    ("Lo", "Other_Letter", "", bit(G::OtherLetter)),
    ("M", "Mark", "Combining_Mark", M_M),
    ("Mn", "Nonspacing_Mark", "", bit(G::NonspacingMark)),
    ("Mc", "Spacing_Mark", "", bit(G::SpacingMark)),
    ("Me", "Enclosing_Mark", "", bit(G::EnclosingMark)),
    ("N", "Number", "", M_N),
    ("Nd", "Decimal_Number", "digit", bit(G::DecimalNumber)),
    ("Nl", "Letter_Number", "", bit(G::LetterNumber)),
    ("No", "Other_Number", "", bit(G::OtherNumber)),
    ("P", "Punctuation", "punct", M_P),
    (
        "Pc",
        "Connector_Punctuation",
        "",
        bit(G::ConnectorPunctuation),
    ),
    ("Pd", "Dash_Punctuation", "", bit(G::DashPunctuation)),
    ("Ps", "Open_Punctuation", "", bit(G::OpenPunctuation)),
    ("Pe", "Close_Punctuation", "", bit(G::ClosePunctuation)),
    ("Pi", "Initial_Punctuation", "", bit(G::InitialPunctuation)),
    ("Pf", "Final_Punctuation", "", bit(G::FinalPunctuation)),
    ("Po", "Other_Punctuation", "", bit(G::OtherPunctuation)),
    ("S", "Symbol", "", M_S),
    ("Sm", "Math_Symbol", "", bit(G::MathSymbol)),
    ("Sc", "Currency_Symbol", "", bit(G::CurrencySymbol)),
    ("Sk", "Modifier_Symbol", "", bit(G::ModifierSymbol)),
    ("So", "Other_Symbol", "", bit(G::OtherSymbol)),
    ("Z", "Separator", "", M_Z),
    ("Zs", "Space_Separator", "", bit(G::SpaceSeparator)),
    ("Zl", "Line_Separator", "", bit(G::LineSeparator)),
    ("Zp", "Paragraph_Separator", "", bit(G::ParagraphSeparator)),
    ("C", "Other", "", M_C),
    ("Cc", "Control", "cntrl", bit(G::Control)),
    ("Cf", "Format", "", bit(G::Format)),
    ("Cs", "Surrogate", "", bit(G::Surrogate)),
    ("Co", "Private_Use", "", bit(G::PrivateUse)),
    ("Cn", "Unassigned", "", bit(G::Unassigned)),
];

/// Loose property name matching (UAX #44 LM3): case, spaces, '_' and '-'
/// are ignored.
pub fn loose(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn gc_lookup(v: &str) -> Option<u32> {
    let l = loose(v);
    GC_NAMES
        .iter()
        .find(|(s, lg, o, _)| loose(s) == l || loose(lg) == l || (!o.is_empty() && loose(o) == l))
        .map(|e| e.3)
}

thread_local! {
    static SCRIPTS: RefCell<Option<HashMap<String, Script>>> = const { RefCell::new(None) };
}

/// All the Script values (with their long names), found by scanning the
/// code space once (intl has no list of them).
pub fn script_lookup(v: &str) -> Option<Script> {
    let l = loose(v);
    SCRIPTS.with(|t| {
        let mut t = t.borrow_mut();
        let map = t.get_or_insert_with(|| {
            let mut m = HashMap::new();
            let mut last = None;
            for cp in 0..=0x10FFFFu32 {
                let s = un::script_u32(cp);
                if Some(s) != last {
                    m.entry(loose(s.long_name())).or_insert(s);
                    last = Some(s);
                }
            }
            for (code, name) in SCRIPT_CODES {
                if let Some(&s) = m.get(&loose(name)) {
                    m.insert(loose(code), s);
                }
            }
            m
        });
        map.get(&l).copied()
    })
}

/// ISO 15924 codes of the common scripts.
const SCRIPT_CODES: &[(&str, &str)] = &[
    ("Zyyy", "Common"),
    ("Zinh", "Inherited"),
    ("Qaai", "Inherited"),
    ("Arab", "Arabic"),
    ("Armn", "Armenian"),
    ("Beng", "Bengali"),
    ("Bopo", "Bopomofo"),
    ("Cher", "Cherokee"),
    ("Copt", "Coptic"),
    ("Qaac", "Coptic"),
    ("Cyrl", "Cyrillic"),
    ("Deva", "Devanagari"),
    ("Ethi", "Ethiopic"),
    ("Geor", "Georgian"),
    ("Grek", "Greek"),
    ("Gujr", "Gujarati"),
    ("Guru", "Gurmukhi"),
    ("Hani", "Han"),
    ("Hang", "Hangul"),
    ("Hebr", "Hebrew"),
    ("Hira", "Hiragana"),
    ("Kana", "Katakana"),
    ("Khmr", "Khmer"),
    ("Knda", "Kannada"),
    ("Laoo", "Lao"),
    ("Latn", "Latin"),
    ("Mlym", "Malayalam"),
    ("Mong", "Mongolian"),
    ("Mymr", "Myanmar"),
    ("Orya", "Oriya"),
    ("Sinh", "Sinhala"),
    ("Syrc", "Syriac"),
    ("Taml", "Tamil"),
    ("Telu", "Telugu"),
    ("Thaa", "Thaana"),
    ("Thai", "Thai"),
    ("Tibt", "Tibetan"),
    ("Zzzz", "Unknown"),
];

pub fn is_ideographic(c: char) -> bool {
    let cp = c as u32;
    matches!(cp, 0x3006 | 0x3007 | 0x3021..=0x3029 | 0x3038..=0x303A | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFA6D | 0xFA70..=0xFAD9 | 0x16FE4 | 0x17000..=0x187FF | 0x18800..=0x18CD5 | 0x18CFF..=0x18D1E | 0x1B170..=0x1B2FB | 0x20000..=0x323AF)
        || (un::script(c) == Script::Han
            && un::general_category(c) == GeneralCategory::OtherLetter
            && cp >= 0x3400)
}

pub fn is_noncharacter(c: char) -> bool {
    let cp = c as u32;
    (0xFDD0..=0xFDEF).contains(&cp) || cp & 0xFFFE == 0xFFFE
}

fn is_pattern_ws(c: char) -> bool {
    matches!(
        c as u32,
        0x09..=0x0D | 0x20 | 0x85 | 0x200E | 0x200F | 0x2028 | 0x2029
    )
}

/// Binary properties by (loose) name.
pub fn binary_lookup(v: &str) -> Option<fn(char) -> bool> {
    let f: fn(char) -> bool = match loose(v).as_str() {
        "alphabetic" | "alpha" => un::is_alphabetic,
        "whitespace" | "wspace" | "space" => un::is_whitespace,
        "uppercase" | "upper" => un::is_uppercase,
        "lowercase" | "lower" => un::is_lowercase,
        "math" => un::is_math,
        "dash" => un::is_dash,
        "diacritic" | "dia" => un::is_diacritic,
        "hexdigit" | "hex" => un::is_hex_digit,
        "asciihexdigit" | "ahex" => |c| c.is_ascii_hexdigit(),
        "quotationmark" | "qmark" => un::is_quotation_mark,
        "joincontrol" | "joinc" => un::is_join_control,
        "defaultignorablecodepoint" | "di" => un::is_default_ignorable,
        "ideographic" | "ideo" => is_ideographic,
        "noncharactercodepoint" | "nchar" => is_noncharacter,
        "bidimirrored" | "bidim" => un::is_bidi_mirrored,
        "xidstart" | "xids" | "idstart" | "ids" => un::is_xid_start,
        "xidcontinue" | "xidc" | "idcontinue" | "idc" => un::is_xid_continue,
        "changeswhenlowercased" | "cwl" => un::changes_when_lowercased,
        "changeswhenuppercased" | "cwu" => un::changes_when_uppercased,
        "changeswhentitlecased" | "cwt" => un::changes_when_titlecased,
        "changeswhencasefolded" | "cwcf" => un::changes_when_casefolded,
        "changeswhencasemapped" | "cwcm" => un::changes_when_casemapped,
        "patternwhitespace" | "patws" => is_pattern_ws,
        "ascii" => |c| c.is_ascii(),
        "any" => |_| true,
        "assigned" => un::is_assigned,
        "cased" => crate::uchar::is_cased,
        "caseignorable" | "ci" => crate::uchar::is_case_ignorable,
        "softdotted" | "sd" => crate::spoofchecker::is_soft_dotted,
        "logicalorderexception" | "loe" => {
            |c| matches!(c as u32, 0x0E40..=0x0E44 | 0x0EC0..=0x0EC4 | 0x19B5..=0x19B7 | 0x19BA | 0xAAB5 | 0xAAB6 | 0xAAB9 | 0xAABB | 0xAABC)
        }
        _ => return None,
    };
    Some(f)
}

/// A property expression (`L`, `Letter`, `gc=Lu`, `Latin`, `sc=Grek`,
/// `Alphabetic`...).
pub fn property(expr: &str) -> Option<USet> {
    let expr = expr.trim();
    if let Some((k, v)) = expr.split_once(['=', ':']) {
        let (k, v) = (loose(k), v.trim());
        return match k.as_str() {
            "gc" | "generalcategory" => gc_lookup(v).map(USet::Gc),
            "sc" | "script" => script_lookup(v).map(USet::Script),
            "scx" | "scriptextensions" => script_lookup(v).map(USet::Scx),
            "ccc" | "canonicalcombiningclass" => ccc_lookup(v).map(USet::Ccc),
            "blk" | "block" => Some(USet::Block(loose(v))),
            _ => {
                let f = binary_lookup(&k)?;
                match loose(v).as_str() {
                    "y" | "yes" | "t" | "true" => Some(USet::Bin(f)),
                    "n" | "no" | "f" | "false" => Some(USet::Not(Box::new(USet::Bin(f)))),
                    _ => None,
                }
            }
        };
    }
    if let Some(m) = gc_lookup(expr) {
        return Some(USet::Gc(m));
    }
    if let Some(f) = binary_lookup(expr) {
        return Some(USet::Bin(f));
    }
    script_lookup(expr).map(USet::Script)
}

/// A Canonical_Combining_Class value by number or (loose) name.
fn ccc_lookup(v: &str) -> Option<u8> {
    if let Ok(n) = v.trim().parse::<u8>() {
        return Some(n);
    }
    Some(match loose(v).as_str() {
        "notreordered" | "nr" => 0,
        "overlay" | "ov" => 1,
        "hanreading" | "hanr" => 6,
        "nukta" | "nk" => 7,
        "kanavoicing" | "kv" => 8,
        "virama" | "vr" => 9,
        "attachedbelowleft" | "atbl" => 200,
        "attachedbelow" | "atb" => 202,
        "attachedabove" | "ata" => 214,
        "attachedaboveright" | "atar" => 216,
        "belowleft" | "bl" => 218,
        "below" | "b" => 220,
        "belowright" | "br" => 222,
        "left" | "l" => 224,
        "right" | "r" => 226,
        "aboveleft" | "al" => 228,
        "above" | "a" => 230,
        "aboveright" | "ar" => 232,
        "doublebelow" | "db" => 233,
        "doubleabove" | "da" => 234,
        "iotasubscript" | "is" => 240,
        _ => return None,
    })
}

/// A parse failure: the ICU error code and the offset (in chars).
#[derive(Debug, Clone, Copy)]
pub struct SetError {
    pub pos: usize,
}

/// Resolves `$name` variables inside a set.
pub type VarFn<'a> = &'a dyn Fn(&str) -> Option<USet>;

/// Whether a set pattern starts at `pos`.
pub fn starts_set(s: &[char], pos: usize) -> bool {
    match s.get(pos) {
        Some('[') => true,
        Some('\\') => matches!(s.get(pos + 1), Some('p' | 'P' | 'N')),
        _ => false,
    }
}

/// Parse an escape after a backslash at `s[*pos]` (pointing past the
/// backslash): `\uXXXX`, `\U00XXXXXX`, `\x{...}`, `\xXX`, `\t`, `\n`... or
/// the escaped character itself.
pub fn parse_escape(s: &[char], pos: &mut usize) -> Option<u32> {
    let c = *s.get(*pos)?;
    *pos += 1;
    let hex = |s: &[char], pos: &mut usize, min: usize, max: usize| -> Option<u32> {
        let mut v = 0u32;
        let mut n = 0;
        while n < max {
            match s.get(*pos).and_then(|c| c.to_digit(16)) {
                Some(d) => {
                    v = v * 16 + d;
                    *pos += 1;
                    n += 1;
                }
                None => break,
            }
        }
        (n >= min).then_some(v)
    };
    Some(match c {
        'u' => hex(s, pos, 4, 4)?,
        'U' => hex(s, pos, 8, 8)?,
        'x' => {
            if s.get(*pos) == Some(&'{') {
                *pos += 1;
                let v = hex(s, pos, 1, 6)?;
                if s.get(*pos) != Some(&'}') {
                    return None;
                }
                *pos += 1;
                v
            } else {
                hex(s, pos, 1, 2)?
            }
        }
        't' => 9,
        'n' => 10,
        'r' => 13,
        'f' => 12,
        'v' => 11,
        'a' => 7,
        'e' => 0x1B,
        c => c as u32,
    })
}

/// Parse a set pattern at `s[*pos]`: `[...]`, `[:prop:]`, `\p{...}`,
/// `\P{...}`, `\N{name}`.
pub fn parse(s: &[char], pos: &mut usize, vars: VarFn) -> Result<USet, SetError> {
    let start = *pos;
    let err = |p: usize| SetError { pos: p };
    if s.get(start) == Some(&'\\') {
        let neg = s.get(start + 1) == Some(&'P');
        let named = s.get(start + 1) == Some(&'N');
        if s.get(start + 2) != Some(&'{') {
            return Err(err(start));
        }
        let close = (start + 3..s.len())
            .find(|&i| s[i] == '}')
            .ok_or(err(start))?;
        let body: String = s[start + 3..close].iter().collect();
        *pos = close + 1;
        if named {
            let cp = char_from_name(&body).ok_or(err(start))?;
            return Ok(USet::Ranges(vec![(cp, cp)]));
        }
        let p = property(&body).ok_or(err(start))?;
        return Ok(if neg { USet::Not(Box::new(p)) } else { p });
    }
    if s.get(start) != Some(&'[') {
        return Err(err(start));
    }
    if s.get(start + 1) == Some(&':') {
        let close = (start + 2..s.len().saturating_sub(1))
            .find(|&i| s[i] == ':' && s[i + 1] == ']')
            .ok_or(err(start))?;
        let mut body: String = s[start + 2..close].iter().collect();
        let neg = body.starts_with('^');
        if neg {
            body.remove(0);
        }
        *pos = close + 2;
        let p = property(&body).ok_or(err(start))?;
        return Ok(if neg { USet::Not(Box::new(p)) } else { p });
    }
    // [ ... ]
    *pos += 1;
    let mut neg = false;
    if s.get(*pos) == Some(&'^') {
        neg = true;
        *pos += 1;
    }
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    let mut parts: Vec<USet> = Vec::new();
    // A pending binary operation on the accumulated set.
    let mut acc: Option<USet> = None;
    let mut op: Option<char> = None;
    let mut last: Option<u32> = None;
    let flush = |ranges: &mut Vec<(u32, u32)>, parts: &mut Vec<USet>| -> USet {
        let mut v: Vec<USet> = std::mem::take(parts);
        if !ranges.is_empty() {
            v.push(USet::Ranges(std::mem::take(ranges)));
        }
        if v.len() == 1 {
            v.pop().unwrap()
        } else {
            USet::Union(v)
        }
    };
    loop {
        let Some(&c) = s.get(*pos) else {
            return Err(err(*pos));
        };
        if is_pattern_ws(c) {
            *pos += 1;
            continue;
        }
        match c {
            ']' => {
                *pos += 1;
                break;
            }
            '[' | '\\' if starts_set(s, *pos) => {
                let sub = parse(s, pos, vars)?;
                if let Some(o) = op.take() {
                    let a = acc.take().unwrap_or_else(|| flush(&mut ranges, &mut parts));
                    acc = Some(match o {
                        '&' => USet::Inter(Box::new(a), Box::new(sub)),
                        _ => USet::Diff(Box::new(a), Box::new(sub)),
                    });
                } else if let Some(a) = acc.take() {
                    parts.push(a);
                    parts.push(sub);
                } else {
                    parts.push(sub);
                }
                last = None;
            }
            '$' => {
                let st = *pos;
                *pos += 1;
                while s
                    .get(*pos)
                    .is_some_and(|c| c.is_alphanumeric() || *c == '_')
                {
                    *pos += 1;
                }
                let name: String = s[st + 1..*pos].iter().collect();
                if name.is_empty() {
                    // (before the ']': the anchor, U+FFFF, matching the
                    // start / end of the text in transliterator contexts)
                    if s.get(*pos) == Some(&']') {
                        ranges.push((0xFFFF, 0xFFFF));
                        last = None;
                        continue;
                    }
                    ranges.push(('$' as u32, '$' as u32));
                    last = Some('$' as u32);
                    continue;
                }
                let sub = vars(&name).ok_or(err(st))?;
                if let Some(o) = op.take() {
                    let a = acc.take().unwrap_or_else(|| flush(&mut ranges, &mut parts));
                    acc = Some(match o {
                        '&' => USet::Inter(Box::new(a), Box::new(sub)),
                        _ => USet::Diff(Box::new(a), Box::new(sub)),
                    });
                } else {
                    parts.push(sub);
                }
                last = None;
            }
            '&' | '-'
                if (last.is_none() && (!parts.is_empty() || acc.is_some()))
                    && s.get(*pos + 1).is_some_and(|_| {
                        let mut q = *pos + 1;
                        while s.get(q).is_some_and(|&c| is_pattern_ws(c)) {
                            q += 1;
                        }
                        starts_set(s, q) || s.get(q) == Some(&'$')
                    }) =>
            {
                if acc.is_none() {
                    acc = Some(flush(&mut ranges, &mut parts));
                }
                op = Some(c);
                *pos += 1;
            }
            '-' if last.is_some() && s.get(*pos + 1).is_some_and(|&n| n != ']') => {
                *pos += 1;
                let hi = set_char(s, pos).ok_or(err(*pos))?;
                let lo = last.take().unwrap();
                if hi < lo {
                    return Err(err(*pos));
                }
                if let Some(l) = ranges.last_mut() {
                    *l = (lo, hi);
                }
            }
            '{' => {
                // multi-character strings: only single characters kept
                let close = (*pos..s.len()).find(|&i| s[i] == '}').ok_or(err(*pos))?;
                let body: Vec<char> = s[*pos + 1..close].to_vec();
                if body.len() == 1 {
                    ranges.push((body[0] as u32, body[0] as u32));
                }
                *pos = close + 1;
                last = None;
            }
            _ => {
                let cp = set_char(s, pos).ok_or(err(*pos))?;
                ranges.push((cp, cp));
                last = Some(cp);
            }
        }
    }
    let mut set = match acc {
        Some(a) => {
            if parts.is_empty() && ranges.is_empty() {
                a
            } else {
                let mut v = vec![a];
                v.push(flush(&mut ranges, &mut parts));
                USet::Union(v)
            }
        }
        None => flush(&mut ranges, &mut parts),
    };
    if neg {
        set = USet::Not(Box::new(set));
    }
    Ok(set)
}

/// One (possibly escaped or quoted) character of a set.
fn set_char(s: &[char], pos: &mut usize) -> Option<u32> {
    let c = *s.get(*pos)?;
    if c == '\\' {
        *pos += 1;
        return parse_escape(s, pos);
    }
    if c == '\'' {
        // 'x' or '' (a quote)
        if s.get(*pos + 1) == Some(&'\'') {
            *pos += 2;
            return Some('\'' as u32);
        }
        let ch = *s.get(*pos + 1)?;
        if s.get(*pos + 2) == Some(&'\'') {
            *pos += 3;
            return Some(ch as u32);
        }
        return None;
    }
    *pos += 1;
    Some(c as u32)
}

/// u_charFromName() over intl's name table (U_UNICODE_CHAR_NAME), with
/// the algorithmic names.
pub fn char_from_name(name: &str) -> Option<u32> {
    let n = name.trim().to_ascii_uppercase();
    names_index().with(|m| m.get(&n).copied())
}

/// name -> code point, built on first use.
fn names_index() -> &'static std::thread::LocalKey<HashMap<String, u32>> {
    thread_local! {
        static IDX: HashMap<String, u32> = {
            let mut m = HashMap::new();
            for cp in 0..=0x10FFFFu32 {
                let Some(c) = char::from_u32(cp) else { continue };
                if let Some(n) = un::name(c) {
                    m.entry(n).or_insert(cp);
                }
            }
            m
        };
    }
    &IDX
}
