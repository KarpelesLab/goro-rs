//! ext/intl/transliterator: Transliterator and the transliterator_*()
//! functions.
//!
//! Transform IDs follow ICU's syntax (`[filter] Source-Target/Variant`,
//! compound `a; b; c`, special inverses); the transforms themselves are
//! implemented here (case mappings, normalization, Hex, Name, Latin-ASCII,
//! script romanizations over intl and translit_data) and rule-based ones
//! run on translit_rules.

use std::rc::Rc;

use goro_core::array::PhpArray;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};
use intl::unicode as un;

use crate::error::{self, IntlError};
use crate::translit_data as td;
use crate::translit_rules::{self, RuleSet, Step};
use crate::uset::{self, USet};
use crate::zpp::Obj;
use crate::{Call, obj, s, sv};

#[derive(Clone)]
enum K {
    Null,
    Remove,
    Upper(String),
    Lower(String),
    Title(String),
    Nf(i64),
    Hex(u8),
    HexAny,
    Name,
    NameAny,
    LatinAscii,
    AnyLatin,
    GreekLatin,
    CyrLatin,
    ArmLatin,
    GeoLatin,
    KanaLatin,
    HangulLatin,
    LatinKana(bool),
    KanaSwap(bool),
    Rules(Rc<RuleSet>),
    Chain(Vec<TElem>),
}

#[derive(Clone)]
struct TElem {
    filter: Option<Rc<USet>>,
    k: K,
}

/// Transliterator_object
#[derive(Clone, Default)]
pub struct Trans {
    t: Option<Rc<Compiled>>,
    pub err: IntlError,
}

struct Compiled {
    id: String,
    /// The ID of the inverse (None: no inverse).
    inv: Option<String>,
    elem: TElem,
}

// ---------------------------------------------------------------------------
// IDs

/// Split a compound ID on ';' outside sets.
fn split_ids(id: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut esc = false;
    for c in id.chars() {
        if esc {
            cur.push(c);
            esc = false;
            continue;
        }
        match c {
            '\\' => {
                esc = true;
                cur.push(c);
            }
            '[' => {
                depth += 1;
                cur.push(c);
            }
            ']' => {
                depth -= 1;
                cur.push(c);
            }
            ';' if depth <= 0 => out.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// An `A(B)` element (outside sets): (A, B), the parenthesized part
/// closing the element.
fn split_inverse(el: &str) -> Option<(String, String)> {
    let t = el.trim();
    let chars: Vec<char> = t.chars().collect();
    let mut depth = 0i32;
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '[' => depth += 1,
            ']' => depth -= 1,
            '(' if depth <= 0 => {
                let close = chars[i + 1..].iter().position(|&c| c == ')')? + i + 1;
                if chars[close + 1..].iter().any(|c| !c.is_whitespace()) {
                    return None;
                }
                return Some((chars[..i].iter().collect(), chars[i + 1..close].iter().collect()));
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The special inverses (TransliteratorIDParser::SPECIAL_INVERSES).
fn special_inverse(target: &str) -> Option<&'static str> {
    Some(match target.to_ascii_lowercase().as_str() {
        "null" => "Null",
        "upper" => "Lower",
        "lower" => "Upper",
        "title" => "Lower",
        "nfc" => "NFD",
        "nfd" => "NFC",
        "nfkc" => "NFKD",
        "nfkd" => "NFKC",
        "fcd" => "FCD",
        "fcc" => "NFD",
        "remove" => "Null",
        "latin" | "greek" | "cyrillic" | "katakana" | "hiragana" | "armenian" | "georgian" => {
            "Null"
        }
        _ => return None,
    })
}

struct Spec {
    filter_text: String,
    filter: Option<USet>,
    source: String,
    saw_source: bool,
    target: String,
    variant: String,
}

fn parse_spec(el: &str) -> Option<Spec> {
    let t = el.trim();
    let chars: Vec<char> = t.chars().collect();
    let mut pos = 0;
    let mut filter = None;
    let mut filter_text = String::new();
    if uset::starts_set(&chars, 0) {
        let set = uset::parse(&chars, &mut pos, &|_| None).ok()?;
        let mut end = pos;
        while end < chars.len() && chars[end].is_whitespace() {
            end += 1;
        }
        filter_text = chars[..end].iter().collect();
        pos = end;
        filter = Some(set);
    }
    let rest: String = chars[pos..].iter().collect();
    let rest = rest.trim();
    let (basic, variant) = match rest.split_once('/') {
        Some((b, v)) => (b.trim(), v.trim().to_string()),
        None => (rest, String::new()),
    };
    let (source, target, saw) = match basic.split_once('-') {
        Some((s, t)) => (s.trim().to_string(), t.trim().to_string(), true),
        None => ("Any".to_string(), basic.to_string(), false),
    };
    Some(Spec {
        filter_text,
        filter,
        source,
        saw_source: saw,
        target,
        variant,
    })
}

fn locale_case(src: &str) -> Option<String> {
    let l = src.to_ascii_lowercase();
    (l.len() == 2 || l.len() == 3).then_some(l)
}

/// The transform of a basic ID.
fn lookup(sp: &Spec) -> Option<K> {
    let src = sp.source.to_ascii_lowercase();
    let tgt = sp.target.to_ascii_lowercase();
    let var = sp.variant.to_ascii_lowercase();
    let any = src == "any";
    if sp.target.is_empty() && sp.filter.is_some() {
        return Some(K::Null);
    }
    let lang = if any {
        Some(String::new())
    } else {
        locale_case(&src)
    };
    Some(match (src.as_str(), tgt.as_str()) {
        (_, "null") if any => K::Null,
        (_, "remove") if any => K::Remove,
        (_, "upper") if lang.is_some() => K::Upper(lang.unwrap()),
        (_, "lower") if lang.is_some() => K::Lower(lang.unwrap()),
        (_, "title") if lang.is_some() => K::Title(lang.unwrap()),
        (_, "nfc") | (_, "fcc") if any => K::Nf(crate::normalizer::FORM_C),
        (_, "nfd") | (_, "fcd") if any => K::Nf(crate::normalizer::FORM_D),
        (_, "nfkc") if any => K::Nf(crate::normalizer::FORM_KC),
        (_, "nfkd") if any => K::Nf(crate::normalizer::FORM_KD),
        (_, "hex") if any => K::Hex(match var.as_str() {
            "" | "java" => 0,
            "c" => 1,
            "perl" => 2,
            "xml" => 3,
            "xml10" => 4,
            "unicode" => 5,
            _ => return None,
        }),
        ("hex", "any") => {
            if !matches!(
                var.as_str(),
                "" | "java" | "c" | "perl" | "xml" | "xml10" | "unicode"
            ) {
                return None;
            }
            K::HexAny
        }
        (_, "name") if any => K::Name,
        ("name", "any") => K::NameAny,
        ("latin", "ascii") => K::LatinAscii,
        ("ascii", "latin") => K::Null,
        (_, "latin") if any => K::AnyLatin,
        ("greek", "latin") | ("el", "latin") => K::GreekLatin,
        ("cyrillic", "latin") | ("ru", "latin") => K::CyrLatin,
        ("armenian", "latin") => K::ArmLatin,
        ("georgian", "latin") => K::GeoLatin,
        ("katakana", "latin") | ("hiragana", "latin") => K::KanaLatin,
        ("hangul", "latin") => K::HangulLatin,
        ("latin", "katakana") => K::LatinKana(false),
        ("latin", "hiragana") => K::LatinKana(true),
        ("hiragana", "katakana") => K::KanaSwap(true),
        ("katakana", "hiragana") => K::KanaSwap(false),
        (_, "katakana") if any => K::Chain(vec![el(K::AnyLatin), el(K::LatinKana(false))]),
        (_, "hiragana") if any => K::Chain(vec![el(K::AnyLatin), el(K::LatinKana(true))]),
        ("greek" | "cyrillic" | "armenian" | "georgian", "katakana" | "hiragana") => {
            let first = lookup(&Spec {
                target: "Latin".into(),
                variant: String::new(),
                filter: None,
                filter_text: String::new(),
                source: sp.source.clone(),
                saw_source: true,
            })?;
            K::Chain(vec![el(first), el(K::LatinKana(tgt == "hiragana"))])
        }
        _ => return None,
    })
}

fn el(k: K) -> TElem {
    TElem { filter: None, k }
}

/// The forward canonical ID and the inverse ID of an element.
fn spec_ids(sp: &Spec) -> (String, String) {
    let var = if sp.variant.is_empty() {
        String::new()
    } else {
        format!("/{}", sp.variant)
    };
    let fwd = format!(
        "{}{}{}{}",
        sp.filter_text,
        if sp.saw_source {
            format!("{}-", sp.source)
        } else {
            String::new()
        },
        sp.target,
        var
    );
    let inv = if sp.source.eq_ignore_ascii_case("any")
        && let Some(i) = special_inverse(&sp.target)
    {
        format!(
            "{}{}{}{}",
            sp.filter_text,
            if sp.saw_source { "Any-" } else { "" },
            i,
            var
        )
    } else {
        format!("{}{}-{}{}", sp.filter_text, sp.target, sp.source, var)
    };
    (fwd, inv)
}

/// Transliterator::createInstance(id): the compiled transform, or the
/// ICU error status.
fn create_instance(id: &str) -> Result<Compiled, i32> {
    let parts = split_ids(id);
    let mut elems = Vec::new();
    let mut fwd_ids = Vec::new();
    let mut inv_ids = Vec::new();
    let mut global: Option<Rc<USet>> = None;
    let n = parts.iter().filter(|p| !p.trim().is_empty()).count();
    for (i, p) in parts.iter().enumerate() {
        if p.trim().is_empty() {
            if n == 0 && i == 0 {
                return Err(translit_rules::U_INVALID_ID);
            }
            continue;
        }
        // `A(B)`, `A()`, `(B)`: A forward, B the inverse
        if let Some((a, b)) = split_inverse(p) {
            let ida = match a.trim() {
                "" => None,
                t => Some(parse_spec(t).ok_or(translit_rules::U_INVALID_ID)?),
            };
            let idb = match b.trim() {
                "" => None,
                t => Some(parse_spec(t).ok_or(translit_rules::U_INVALID_ID)?),
            };
            let canon = |sp: &Option<Spec>| sp.as_ref().map(|s| spec_ids(s).0).unwrap_or_default();
            fwd_ids.push(format!("{}({})", canon(&ida), canon(&idb)));
            inv_ids.push(format!("{}({})", canon(&idb), canon(&ida)));
            let elem = match ida {
                Some(sp) => {
                    let k = lookup(&sp).ok_or(translit_rules::U_INVALID_ID)?;
                    TElem {
                        filter: sp.filter.map(Rc::new),
                        k,
                    }
                }
                None => el(K::Null),
            };
            elems.push(elem);
            continue;
        }
        let sp = parse_spec(p).ok_or(translit_rules::U_INVALID_ID)?;
        // a lone leading filter is the compound's filter
        if sp.target.is_empty() && sp.source == "Any" && !sp.saw_source {
            if sp.filter.is_some() && elems.is_empty() && global.is_none() && n > 1 {
                global = sp.filter.map(Rc::new);
                fwd_ids.push(sp.filter_text.trim().to_string());
                continue;
            }
            return Err(translit_rules::U_INVALID_ID);
        }
        let k = lookup(&sp).ok_or(translit_rules::U_INVALID_ID)?;
        let (f, iv) = spec_ids(&sp);
        fwd_ids.push(f);
        inv_ids.push(iv);
        elems.push(TElem {
            filter: sp.filter.map(Rc::new),
            k,
        });
    }
    if elems.is_empty() {
        return Err(translit_rules::U_INVALID_ID);
    }
    inv_ids.reverse();
    let id_out = if parts.len() == 1 {
        fwd_ids.pop().unwrap_or_default()
    } else {
        fwd_ids.join(";")
    };
    let elem = if elems.len() == 1 && global.is_none() {
        elems.pop().unwrap()
    } else {
        TElem {
            filter: global,
            k: K::Chain(elems),
        }
    };
    Ok(Compiled {
        id: id_out,
        inv: Some(inv_ids.join(";")),
        elem,
    })
}

// ---------------------------------------------------------------------------
// Transforms

thread_local! {
    static LATIN_ASCII_RULES: std::cell::OnceCell<Option<Rc<RuleSet>>> = const { std::cell::OnceCell::new() };
}

/// A CLDR transform's rules, compiled once per thread.
fn cldr_rules(
    cell: &'static std::thread::LocalKey<std::cell::OnceCell<Option<Rc<RuleSet>>>>,
    src: &str,
) -> Option<Rc<RuleSet>> {
    cell.with(|c| c.get_or_init(|| RuleSet::parse(src, true).ok().map(Rc::new)).clone())
}

fn apply(e: &TElem, t: Vec<char>) -> Vec<char> {
    let Some(f) = &e.filter else {
        return apply_k(&e.k, t);
    };
    // transform the runs of filtered characters
    let mut out = Vec::with_capacity(t.len());
    let mut i = 0;
    while i < t.len() {
        if !f.has(t[i]) {
            out.push(t[i]);
            i += 1;
            continue;
        }
        let st = i;
        while i < t.len() && f.has(t[i]) {
            i += 1;
        }
        out.extend(apply_k(&e.k, t[st..i].to_vec()));
    }
    out
}

fn string_of(t: &[char]) -> String {
    t.iter().collect()
}

fn hex_of(c: char, style: u8) -> String {
    let cp = c as u32;
    match style {
        0 => {
            let mut b = [0u16; 2];
            c.encode_utf16(&mut b)
                .iter()
                .map(|u| format!("\\u{:04X}", u))
                .collect()
        }
        1 => {
            if cp > 0xFFFF {
                format!("\\U{:08X}", cp)
            } else {
                format!("\\u{:04X}", cp)
            }
        }
        2 => format!("\\x{{{:X}}}", cp),
        3 => format!("&#x{:X};", cp),
        4 => format!("&#{};", cp),
        _ => format!("U+{:04X}", cp),
    }
}

/// Hex-Any: \uXXXX, \UXXXXXXXX, \x{X..}, &#xX..;, &#D..;, U+XXXX.
fn hex_any(t: &[char]) -> Vec<char> {
    let mut units: Vec<u32> = Vec::new();
    let mut i = 0;
    let hexn = |t: &[char], i: usize, min: usize, max: usize| -> Option<(u32, usize)> {
        let mut v = 0u32;
        let mut n = 0;
        while n < max && i + n < t.len() && t[i + n].is_ascii_hexdigit() {
            v = v * 16 + t[i + n].to_digit(16).unwrap();
            n += 1;
        }
        (n >= min).then_some((v, n))
    };
    while i < t.len() {
        let c = t[i];
        let r = match c {
            '\\' if t.get(i + 1) == Some(&'u') => hexn(t, i + 2, 4, 4).map(|(v, n)| (v, 2 + n)),
            '\\' if t.get(i + 1) == Some(&'U') => hexn(t, i + 2, 8, 8).map(|(v, n)| (v, 2 + n)),
            '\\' if t.get(i + 1) == Some(&'x') && t.get(i + 2) == Some(&'{') => hexn(t, i + 3, 1, 6)
                .filter(|(_, n)| t.get(i + 3 + n) == Some(&'}'))
                .map(|(v, n)| (v, 4 + n)),
            '&' if t.get(i + 1) == Some(&'#') => {
                if matches!(t.get(i + 2), Some('x' | 'X')) {
                    hexn(t, i + 3, 1, 6)
                        .filter(|(_, n)| t.get(i + 3 + n) == Some(&';'))
                        .map(|(v, n)| (v, 4 + n))
                } else {
                    let mut v = 0u32;
                    let mut n = 0;
                    while n < 7 && i + 2 + n < t.len() && t[i + 2 + n].is_ascii_digit() {
                        v = v * 10 + t[i + 2 + n].to_digit(10).unwrap();
                        n += 1;
                    }
                    (n > 0 && t.get(i + 2 + n) == Some(&';')).then_some((v, 3 + n))
                }
            }
            'U' | 'u' if t.get(i + 1) == Some(&'+') => hexn(t, i + 2, 4, 6).map(|(v, n)| (v, 2 + n)),
            _ => None,
        };
        match r {
            Some((v, n)) if v <= 0x10FFFF => {
                units.push(v);
                i += n;
            }
            _ => {
                units.push(c as u32);
                i += 1;
            }
        }
    }
    // combine surrogate pairs
    let mut out = Vec::with_capacity(units.len());
    let mut k = 0;
    while k < units.len() {
        let u = units[k];
        if (0xD800..0xDC00).contains(&u)
            && k + 1 < units.len()
            && (0xDC00..0xE000).contains(&units[k + 1])
        {
            let cp = 0x10000 + ((u - 0xD800) << 10) + (units[k + 1] - 0xDC00);
            out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
            k += 2;
            continue;
        }
        out.push(char::from_u32(u).unwrap_or('\u{FFFD}'));
        k += 1;
    }
    out
}

fn name_any(t: &[char]) -> Vec<char> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if t[i] == '\\' && t.get(i + 1) == Some(&'N') && t.get(i + 2) == Some(&'{') {
            if let Some(close) = (i + 3..t.len()).find(|&k| t[k] == '}') {
                let name: String = t[i + 3..close].iter().collect();
                if let Some(cp) = uset::char_from_name(&name).and_then(char::from_u32) {
                    out.push(cp);
                    i = close + 1;
                    continue;
                }
            }
        }
        out.push(t[i]);
        i += 1;
    }
    out
}

fn is_cased(c: char) -> bool {
    un::is_uppercase(c)
        || un::is_lowercase(c)
        || un::general_category(c) == un::GeneralCategory::TitlecaseLetter
}

fn is_case_ignorable(c: char) -> bool {
    use un::GeneralCategory as G;
    matches!(
        un::general_category(c),
        G::NonspacingMark | G::EnclosingMark | G::Format | G::ModifierLetter | G::ModifierSymbol
    ) || matches!(
        c,
        '\'' | '.' | ':' | '^' | '`' | '\u{B7}' | '\u{2018}' | '\u{2019}' | '\u{2024}' | '\u{FE52}' | '\u{FF07}' | '\u{FF0E}' | '\u{AD}'
    )
}

/// Any-Title (TitlecaseTransliterator).
fn title(t: &[char], lang: &str) -> Vec<char> {
    let mut out = String::new();
    let mut do_title = true;
    for &c in t {
        if is_case_ignorable(c) {
            out.push(c);
            continue;
        }
        if is_cased(c) {
            if do_title {
                out.extend(un::to_titlecase(c));
            } else {
                out.push_str(&un::lowercase_str_lang(&c.to_string(), lang));
            }
            do_title = false;
        } else {
            out.push(c);
            do_title = true;
        }
    }
    out.chars().collect()
}

/// Any-Latin: each script run romanized with its transform.
fn any_latin(t: &[char]) -> Vec<char> {
    #[derive(PartialEq, Clone, Copy)]
    enum S {
        Other,
        Greek,
        Cyr,
        Arm,
        Geo,
        Kana,
        Hang,
    }
    let script_of = |c: char| match un::script(c) {
        un::Script::Greek => S::Greek,
        un::Script::Cyrillic => S::Cyr,
        un::Script::Armenian => S::Arm,
        un::Script::Georgian => S::Geo,
        un::Script::Hiragana | un::Script::Katakana => S::Kana,
        un::Script::Hangul => S::Hang,
        _ if c == 'ー' => S::Kana,
        un::Script::Common | un::Script::Inherited => S::Other,
        _ => S::Other,
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let s = script_of(t[i]);
        let st = i;
        i += 1;
        while i < t.len() {
            let x = script_of(t[i]);
            if x == s || (s != S::Other && un::is_mark(t[i])) {
                i += 1;
            } else {
                break;
            }
        }
        let run = &t[st..i];
        let r: Vec<char> = match s {
            S::Greek => td::greek_latin(run),
            S::Cyr => intl::translit::cyrillic_to_latin(&string_of(run)).chars().collect(),
            S::Arm => intl::translit::armenian_to_latin(&string_of(run)).chars().collect(),
            S::Geo => intl::translit::georgian_to_latin(&string_of(run)).chars().collect(),
            S::Kana => td::kana_latin(run),
            S::Hang => td::hangul_latin(run),
            S::Other => run.to_vec(),
        };
        out.extend(r);
    }
    out
}

fn apply_k(k: &K, t: Vec<char>) -> Vec<char> {
    match k {
        K::Null => t,
        K::Remove => Vec::new(),
        K::Upper(l) => un::uppercase_str_lang(&string_of(&t), l).chars().collect(),
        K::Lower(l) => un::lowercase_str_lang(&string_of(&t), l).chars().collect(),
        K::Title(l) => title(&t, l),
        K::Nf(f) => crate::normalizer::normalize_str(*f, &string_of(&t))
            .chars()
            .collect(),
        K::Hex(st) => t.iter().flat_map(|&c| hex_of(c, *st).chars().collect::<Vec<_>>()).collect(),
        K::HexAny => hex_any(&t),
        K::Name => t
            .iter()
            .flat_map(|&c| match un::name(c) {
                Some(n) => format!("\\N{{{}}}", n).chars().collect::<Vec<_>>(),
                None => vec![c],
            })
            .collect(),
        K::NameAny => name_any(&t),
        // (CLDR's rules, as ICU: Latin, Common and Inherited only; intl's
        // latin_ascii() strips the marks of other scripts and leaves the
        // compatibility characters)
        K::LatinAscii => match cldr_rules(&LATIN_ASCII_RULES, crate::translit_cldr::LATIN_ASCII) {
            Some(rs) => apply_k(&K::Rules(rs), t),
            None => intl::translit::latin_ascii(&string_of(&t)).chars().collect(),
        },
        K::AnyLatin => any_latin(&t),
        K::GreekLatin => td::greek_latin(&t),
        K::CyrLatin => intl::translit::cyrillic_to_latin(&string_of(&t))
            .chars()
            .collect(),
        K::ArmLatin => intl::translit::armenian_to_latin(&string_of(&t))
            .chars()
            .collect(),
        K::GeoLatin => intl::translit::georgian_to_latin(&string_of(&t))
            .chars()
            .collect(),
        K::KanaLatin => td::kana_latin(&t),
        K::HangulLatin => td::hangul_latin(&t),
        K::LatinKana(h) => td::latin_kana(&t, *h),
        K::KanaSwap(to_kata) => td::kana_swap(&t, *to_kata),
        K::Chain(v) => v.iter().fold(t, |acc, e| apply(e, acc)),
        K::Rules(rs) => {
            let run = |mut t: Vec<char>| {
                for st in &rs.steps {
                    match st {
                        Step::Rules(r) => rs.apply_rules(r, &mut t),
                        Step::Id(id) => {
                            if let Ok(c) = create_instance(id) {
                                t = apply(&c.elem, t);
                            }
                        }
                    }
                }
                t
            };
            match &rs.filter {
                Some(f) => {
                    let mut out = Vec::new();
                    let mut i = 0;
                    while i < t.len() {
                        if !f.has(t[i]) {
                            out.push(t[i]);
                            i += 1;
                            continue;
                        }
                        let st = i;
                        while i < t.len() && f.has(t[i]) {
                            i += 1;
                        }
                        out.extend(run(t[st..i].to_vec()));
                    }
                    out
                }
                None => run(t),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The class

pub static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(clone_obj),
    ..NativeObjectHandlers::EMPTY
};

fn create(_vm: &mut Vm, o: &Obj) {
    obj::attach(o, Trans::default(), &HANDLERS);
}

fn clone_obj(vm: &mut Vm, src: &Obj, dst: &Obj) {
    let Some(st) = obj::state::<Trans>(src) else {
        return;
    };
    if st.borrow().t.is_none() {
        crate::zpp::throw(vm, b"Error", "Cannot clone uninitialized Transliterator");
    }
    obj::clone_state::<Trans>(src, dst, |t| Trans {
        t: t.t.clone(),
        err: IntlError::default(),
    });
}

crate::intl_functions! { pub FUNCTIONS:
    "transliterator_create" => tr_create,
    "transliterator_create_from_rules" => tr_create_from_rules,
    "transliterator_create_inverse" => tr_create_inverse,
    "transliterator_list_ids" => tr_list_ids,
    "transliterator_transliterate" => tr_transliterate,
    "transliterator_get_error_code" => tr_get_error_code,
    "transliterator_get_error_message" => tr_get_error_message,
}

pub const METHODS: &[(&str, crate::Imp)] = &[("Transliterator::__construct", tr_ctor)];

pub const CLASSES: &[(&str, Option<crate::obj::CreateFn>)] =
    &[("Transliterator", Some(create))];

fn tr_ctor(vm: &mut Vm, _c: &mut Call) -> Value {
    let scope = match vm.current_class_scope() {
        Some(c) => format!(
            "scope {}",
            goro_core::value::display_class_name(
                &vm.classes.get(&c).map(|d| d.name.clone()).unwrap_or(c)
            )
        ),
        None => "global scope".to_string(),
    };
    let msg = format!("Call to private Transliterator::__construct() from {}", scope);
    crate::zpp::throw(vm, b"Error", &msg);
    Value::Null
}

/// A new Transliterator object for `c`.
fn new_object(vm: &mut Vm, c: Rc<Compiled>) -> Option<Obj> {
    let o = obj::object_init_ex(vm, b"transliterator")?;
    o.borrow_mut()
        .set_property(b"id".to_vec(), s(c.id.clone()));
    if let Some(st) = obj::state::<Trans>(&o) {
        st.borrow_mut().t = Some(c);
    }
    Some(o)
}

fn check_direction(vm: &mut Vm, c: &Call, d: i64) -> bool {
    if d != 0 && d != 1 {
        crate::zpp::value_error(
            vm,
            &c.name,
            2,
            "direction",
            "must be either Transliterator::FORWARD or Transliterator::REVERSE",
        );
        return false;
    }
    true
}

/// create_transliterator(): Err(()) with the global error set.
fn create_from_id(vm: &mut Vm, name: &str, id: &[u8], dir: i64) -> Result<Obj, ()> {
    let Ok(ids) = std::str::from_utf8(id) else {
        error::set_global(
            vm,
            name,
            error::U_INVALID_CHAR_FOUND,
            "String conversion of id to UTF-16 failed",
        );
        return Err(());
    };
    let r = create_instance(ids).and_then(|c| {
        if dir == 1 {
            let inv = c.inv.clone().ok_or(translit_rules::U_INVALID_ID)?;
            let mut ic = create_instance(&inv)?;
            ic.inv = Some(c.id.clone());
            Ok(ic)
        } else {
            Ok(c)
        }
    });
    match r {
        Ok(c) => new_object(vm, Rc::new(c)).ok_or(()),
        Err(code) => {
            let msg = format!("unable to open ICU transliterator with id \"{}\"", ids);
            error::set_global(vm, name, code, &msg);
            Err(())
        }
    }
}

/// transliterator_create()
fn tr_create(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let id = c.str(0);
    let dir = c.long(1);
    if !check_direction(vm, c, dir) {
        return Value::Null;
    }
    match create_from_id(vm, &c.name, &id, dir) {
        Ok(o) => Value::Object(o),
        Err(()) => Value::Null,
    }
}

/// transliterator_create_from_rules()
fn tr_create_from_rules(vm: &mut Vm, c: &mut Call) -> Value {
    let rules = c.str(0);
    let dir = c.long(1);
    if !check_direction(vm, c, dir) {
        return Value::Null;
    }
    let mut err = IntlError::default();
    let Ok(src) = std::str::from_utf8(&rules) else {
        error::set_both(
            vm,
            &c.name,
            &mut err,
            error::U_INVALID_CHAR_FOUND,
            "String conversion of rules to UTF-16 failed",
        );
        return Value::Null;
    };
    match RuleSet::parse(src, dir == 0) {
        Ok(rs) => {
            error::set_global_code(0);
            let comp = Compiled {
                id: "RulesTransPHP".to_string(),
                inv: None,
                elem: el(K::Rules(Rc::new(rs))),
            };
            match new_object(vm, Rc::new(comp)) {
                Some(o) => Value::Object(o),
                None => Value::Null,
            }
        }
        Err(e) => {
            let pe = error::parse_error_string(0, e.offset, &e.pre, &e.post);
            let msg = format!("unable to create ICU transliterator from rules ({})", pe);
            error::set_both(vm, &c.name, &mut err, e.code, &msg);
            Value::Null
        }
    }
}

/// TRANSLITERATOR_METHOD_FETCH_OBJECT
fn fetch(vm: &mut Vm, o: &Obj) -> Option<(std::rc::Rc<std::cell::RefCell<Trans>>, Rc<Compiled>)> {
    let st = obj::state::<Trans>(o)?;
    st.borrow_mut().err.reset();
    let t = st.borrow().t.clone();
    match t {
        Some(t) => Some((st, t)),
        None => {
            crate::zpp::throw(vm, b"Error", "Found unconstructed transliterator");
            None
        }
    }
}

/// transliterator_create_inverse()
fn tr_create_inverse(vm: &mut Vm, c: &mut Call) -> Value {
    let o = c.obj();
    let Some((_, t)) = fetch(vm, &o) else {
        return Value::Null;
    };
    let r = t.inv.clone().ok_or(translit_rules::U_INVALID_ID).and_then(|inv| {
        let mut ic = create_instance(&inv)?;
        ic.inv = Some(t.id.clone());
        Ok(ic)
    });
    match r {
        Ok(ic) => match new_object(vm, Rc::new(ic)) {
            Some(o) => Value::Object(o),
            None => Value::Null,
        },
        Err(code) => {
            let mut err = IntlError::default();
            error::set_both(
                vm,
                &c.name,
                &mut err,
                code,
                "could not create inverse ICU transliterator",
            );
            Value::Null
        }
    }
}

/// The IDs utrans_openIDs() lists.
const IDS: &[&str] = &[
    "ASCII-Latin",
    "Any-Null",
    "Any-Remove",
    "Any-Upper",
    "Any-Lower",
    "Any-Title",
    "Any-NFC",
    "Any-NFD",
    "Any-NFKC",
    "Any-NFKD",
    "Any-FCD",
    "Any-FCC",
    "Any-Hex",
    "Any-Hex/C",
    "Any-Hex/Java",
    "Any-Hex/Perl",
    "Any-Hex/Unicode",
    "Any-Hex/XML",
    "Any-Hex/XML10",
    "Hex-Any",
    "Hex-Any/C",
    "Hex-Any/Java",
    "Hex-Any/Perl",
    "Hex-Any/Unicode",
    "Hex-Any/XML",
    "Hex-Any/XML10",
    "Any-Name",
    "Name-Any",
    "Latin-ASCII",
    "Any-Latin",
    "Any-Katakana",
    "Any-Hiragana",
    "Greek-Latin",
    "Cyrillic-Latin",
    "Armenian-Latin",
    "Georgian-Latin",
    "Hangul-Latin",
    "Katakana-Latin",
    "Hiragana-Latin",
    "Latin-Katakana",
    "Latin-Hiragana",
    "Hiragana-Katakana",
    "Katakana-Hiragana",
    "Greek-Katakana",
    "Greek-Hiragana",
    "Cyrillic-Katakana",
    "Cyrillic-Hiragana",
    "Armenian-Katakana",
    "Armenian-Hiragana",
    "Georgian-Katakana",
    "Georgian-Hiragana",
    "az-Lower",
    "az-Title",
    "az-Upper",
    "el-Lower",
    "el-Title",
    "el-Upper",
    "lt-Lower",
    "lt-Title",
    "lt-Upper",
    "nl-Title",
    "tr-Lower",
    "tr-Title",
    "tr-Upper",
    "el-Latin",
    "ru-Latin",
    "de-Upper",
    "de-Lower",
    "de-Title",
    "en-Upper",
    "en-Lower",
    "en-Title",
    "es-Upper",
    "es-Lower",
    "es-Title",
    "fr-Upper",
    "fr-Lower",
    "fr-Title",
    "it-Upper",
    "it-Lower",
    "it-Title",
    "pt-Upper",
    "pt-Lower",
    "pt-Title",
    "ru-Upper",
    "ru-Lower",
    "ru-Title",
    "ja-Upper",
    "ja-Lower",
    "ja-Title",
    "nl-Upper",
    "nl-Lower",
    "pl-Upper",
    "pl-Lower",
    "pl-Title",
    "sv-Upper",
    "sv-Lower",
    "sv-Title",
    "uk-Upper",
    "uk-Lower",
    "uk-Title",
    "hy-Upper",
    "hy-Lower",
    "hy-Title",
];

/// transliterator_list_ids()
fn tr_list_ids(_vm: &mut Vm, _c: &mut Call) -> Value {
    error::reset_global();
    let mut a = PhpArray::new();
    for id in IDS {
        a.push(s(*id));
    }
    crate::arr(a)
}

/// transliterator_transliterate()
fn tr_transliterate(vm: &mut Vm, c: &mut Call) -> Value {
    let o = match (c.this.clone(), c.first.clone()) {
        (Some(o), _) => o,
        (None, Some(v)) => {
            let id = v.to_php_string().as_bytes().to_vec();
            error::reset_global();
            match create_from_id(vm, &c.name, &id, 0) {
                Ok(o) => o,
                Err(()) => {
                    if vm.current_exception.is_none() {
                        let m = format!(
                            "{}(): Could not create transliterator with ID \"{}\" ({})",
                            c.name,
                            String::from_utf8_lossy(&id),
                            error::global_message()
                        );
                        vm.emit_warning(&m);
                    }
                    return Value::False;
                }
            }
        }
        _ => return Value::Null,
    };
    let text = c.str(0);
    let start = c.long(1);
    let limit = if c.passed(2) { c.long(2) } else { -1 };
    let proc = !c.is_method;
    let pname = |n: usize| -> &'static str {
        if proc {
            ["transliterator", "string", "start", "end"][n - 1]
        } else {
            ["string", "start", "end"][n - 1]
        }
    };
    if limit < -1 {
        crate::zpp::value_error(vm, &c.name, 3, pname(3), "must be greater than or equal to -1");
        return Value::Null;
    }
    if start < 0 {
        crate::zpp::value_error(vm, &c.name, 2, pname(2), "must be greater than or equal to 0");
        return Value::Null;
    }
    if limit != -1 && start > limit {
        crate::zpp::value_error(
            vm,
            &c.name,
            2,
            pname(2),
            "must be less than or equal to argument #3 ($end)",
        );
        return Value::Null;
    }
    let Some((st, t)) = fetch(vm, &o) else {
        return Value::Null;
    };
    let Ok(txt) = std::str::from_utf8(&text) else {
        let mut b = st.borrow_mut();
        error::set_both(
            vm,
            &c.name,
            &mut b.err,
            error::U_INVALID_CHAR_FOUND,
            "String conversion of string to UTF-16 failed",
        );
        return Value::False;
    };
    let ulen = txt.encode_utf16().count() as i64;
    if start > ulen || (limit != -1 && limit > ulen) {
        let msg = format!(
            "Neither \"start\" nor the \"end\" arguments can exceed the number of UTF-16 code units (in this case, {})",
            ulen
        );
        let mut b = st.borrow_mut();
        error::set_both(vm, &c.name, &mut b.err, error::U_ILLEGAL_ARGUMENT_ERROR, &msg);
        return Value::False;
    }
    let limit = if limit == -1 { ulen } else { limit };
    // UTF-16 offsets -> char indexes
    let chars: Vec<char> = txt.chars().collect();
    let to_idx = |u: i64| -> usize {
        let mut n = 0i64;
        for (i, ch) in chars.iter().enumerate() {
            if n >= u {
                return i;
            }
            n += ch.len_utf16() as i64;
        }
        chars.len()
    };
    let (a, b) = (to_idx(start), to_idx(limit));
    let mid = apply(&t.elem, chars[a..b].to_vec());
    let mut out: String = chars[..a].iter().collect();
    out.extend(mid);
    out.extend(chars[b..].iter());
    sv(out.into_bytes())
}

/// transliterator_get_error_code()
fn tr_get_error_code(_vm: &mut Vm, c: &mut Call) -> Value {
    match obj::state::<Trans>(&c.obj()) {
        Some(st) => Value::Long(st.borrow().err.code as i64),
        None => Value::Long(0),
    }
}

/// transliterator_get_error_message()
fn tr_get_error_message(_vm: &mut Vm, c: &mut Call) -> Value {
    match obj::state::<Trans>(&c.obj()) {
        Some(st) => s(st.borrow().err.message()),
        None => s("U_ZERO_ERROR"),
    }
}
