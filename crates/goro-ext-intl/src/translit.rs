//! `Transliterator` (PHP manual: Transliterator): compound transform IDs
//! (`Latin; Title`, `[filter] ID`, `Source-Target/Variant`) over a set of
//! built-in transforms (script romanizations after the ICU user guide's
//! "General Transforms" and the UNGEGN/Hepburn conventions, case
//! mappings, normalization, hex escapes) and rule-based transforms with
//! the core of ICU's transform rule syntax (`a > b;`, `a <> b;`,
//! `x { a } y > b;`, quoted literals).

use crate::uset::{self, USet};

/// One transform step.
#[derive(Clone, Debug)]
pub enum Step {
    Builtin(Builtin),
    Rules(RuleSet),
    Filtered(USet, Box<Step>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Builtin {
    Null,
    Remove,
    Upper,
    Lower,
    Title,
    Nfd,
    Nfc,
    Nfkd,
    Nfkc,
    AnyHex,
    HexAny,
    LatinAscii,
    GreekLatin,
    LatinGreek,
    CyrillicLatin,
    KatakanaLatin,
    LatinKatakana,
    HiraganaLatin,
    LatinHiragana,
    HiraganaKatakana,
    KatakanaHiragana,
    AnyLatin,
    ArmenianLatin,
    GeorgianLatin,
    Accents,
}

/// A transliterator: its ID and steps.
#[derive(Clone, Debug)]
pub struct Translit {
    pub id: String,
    pub steps: Vec<Step>,
    /// The inverse's ID.
    pub inverse_id: Option<String>,
}

fn loose(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The built-in transform of a single ID (and its inverse ID).
fn builtin(id: &str) -> Option<(Builtin, &'static str)> {
    let l = loose(id);
    let l = l.split('/').next().unwrap_or("");
    let (src, dst) = match l.split_once('-') {
        Some((s, d)) => (s.to_string(), d.to_string()),
        None => ("any".to_string(), l.to_string()),
    };
    let s = src.as_str();
    let d = dst.as_str();
    Some(match (s, d) {
        ("any", "null") | ("null", _) => (Builtin::Null, "Any-Null"),
        ("any", "remove") => (Builtin::Remove, "Any-Null"),
        ("any", "upper") | ("upper", _) => (Builtin::Upper, "Any-Lower"),
        ("any", "lower") | ("lower", _) => (Builtin::Lower, "Any-Upper"),
        ("any", "title") | ("title", _) => (Builtin::Title, "Any-Lower"),
        ("any", "nfd") | ("nfd", _) => (Builtin::Nfd, "Any-NFC"),
        ("any", "nfc") | ("nfc", _) => (Builtin::Nfc, "Any-NFD"),
        ("any", "nfkd") | ("nfkd", _) => (Builtin::Nfkd, "Any-NFKC"),
        ("any", "nfkc") | ("nfkc", _) => (Builtin::Nfkc, "Any-NFKD"),
        ("any", "hex") | ("hex", "") => (Builtin::AnyHex, "Hex-Any"),
        ("hex", "any") => (Builtin::HexAny, "Any-Hex"),
        ("latin", "ascii") => (Builtin::LatinAscii, "Any-Null"),
        ("greek", "latin") | ("el", "latin") => (Builtin::GreekLatin, "Latin-Greek"),
        ("latin", "greek") => (Builtin::LatinGreek, "Greek-Latin"),
        ("cyrillic", "latin") | ("russian", "latin") | ("ru", "latin") => (Builtin::CyrillicLatin, "Latin-Cyrillic"),
        ("katakana", "latin") => (Builtin::KatakanaLatin, "Latin-Katakana"),
        ("latin", "katakana") => (Builtin::LatinKatakana, "Katakana-Latin"),
        ("hiragana", "latin") => (Builtin::HiraganaLatin, "Latin-Hiragana"),
        ("latin", "hiragana") => (Builtin::LatinHiragana, "Hiragana-Latin"),
        ("hiragana", "katakana") => (Builtin::HiraganaKatakana, "Katakana-Hiragana"),
        ("katakana", "hiragana") => (Builtin::KatakanaHiragana, "Hiragana-Katakana"),
        ("any", "latin") | ("latin", "") => (Builtin::AnyLatin, "Latin-Any"),
        ("armenian", "latin") => (Builtin::ArmenianLatin, "Latin-Armenian"),
        ("georgian", "latin") => (Builtin::GeorgianLatin, "Latin-Georgian"),
        ("any", "accents") | ("accents", _) => (Builtin::Accents, "Any-Null"),
        ("latin", "latin") | ("any", "any") => (Builtin::Null, "Any-Null"),
        _ => return None,
    })
}

/// The IDs `listIDs()` gives.
pub fn list_ids() -> Vec<String> {
    let mut v: Vec<String> = [
        "ASCII-Latin", "Accents-Any", "Amharic-Latin/BGN", "Any-Accents", "Any-Publishing",
        "Arabic-Latin", "Arabic-Latin/BGN", "Armenian-Latin", "Armenian-Latin/BGN",
        "Azerbaijani-Latin/BGN", "Belarusian-Latin/BGN", "Bengali-Devanagari", "Bengali-Gujarati",
        "Bengali-Gurmukhi", "Bengali-Kannada", "Bengali-Latin", "Bengali-Malayalam",
        "Bengali-Oriya", "Bengali-Tamil", "Bengali-Telugu", "Bopomofo-Latin",
        "Bulgarian-Latin/BGN", "Cyrillic-Latin", "Devanagari-Bengali", "Devanagari-Gujarati",
        "Devanagari-Gurmukhi", "Devanagari-Kannada", "Devanagari-Latin", "Devanagari-Malayalam",
        "Devanagari-Oriya", "Devanagari-Tamil", "Devanagari-Telugu", "Georgian-Latin",
        "Georgian-Latin/BGN", "Greek-Latin", "Greek-Latin/BGN", "Greek-Latin/UNGEGN",
        "Gujarati-Bengali", "Gujarati-Devanagari", "Gujarati-Gurmukhi", "Gujarati-Kannada",
        "Gujarati-Latin", "Gujarati-Malayalam", "Gujarati-Oriya", "Gujarati-Tamil",
        "Gujarati-Telugu", "Gurmukhi-Bengali", "Gurmukhi-Devanagari", "Gurmukhi-Gujarati",
        "Gurmukhi-Kannada", "Gurmukhi-Latin", "Gurmukhi-Malayalam", "Gurmukhi-Oriya",
        "Gurmukhi-Tamil", "Gurmukhi-Telugu", "Han-Latin", "Hangul-Latin", "Hebrew-Latin",
        "Hebrew-Latin/BGN", "Hiragana-Katakana", "Hiragana-Latin", "Jamo-Latin",
        "Kannada-Bengali", "Kannada-Devanagari", "Kannada-Gujarati", "Kannada-Gurmukhi",
        "Kannada-Latin", "Kannada-Malayalam", "Kannada-Oriya", "Kannada-Tamil",
        "Kannada-Telugu", "Katakana-Hiragana", "Katakana-Latin", "Kazakh-Latin/BGN",
        "Kirghiz-Latin/BGN", "Korean-Latin/BGN", "Latin-ASCII", "Latin-Arabic", "Latin-Armenian",
        "Latin-Bengali", "Latin-Bopomofo", "Latin-Cyrillic", "Latin-Devanagari", "Latin-Georgian",
        "Latin-Greek", "Latin-Greek/UNGEGN", "Latin-Gujarati", "Latin-Gurmukhi", "Latin-Hangul",
        "Latin-Hebrew", "Latin-Hiragana", "Latin-Jamo", "Latin-Kannada", "Latin-Katakana",
        "Latin-Malayalam", "Latin-Oriya", "Latin-Russian/BGN", "Latin-Syriac", "Latin-Tamil",
        "Latin-Telugu", "Latin-Thaana", "Latin-Thai", "Macedonian-Latin/BGN", "Malayalam-Bengali",
        "Malayalam-Devanagari", "Malayalam-Gujarati", "Malayalam-Gurmukhi", "Malayalam-Kannada",
        "Malayalam-Latin", "Malayalam-Oriya", "Malayalam-Tamil", "Malayalam-Telugu",
        "Maldivian-Latin/BGN", "Mongolian-Latin/BGN", "Oriya-Bengali", "Oriya-Devanagari",
        "Oriya-Gujarati", "Oriya-Gurmukhi", "Oriya-Kannada", "Oriya-Latin", "Oriya-Malayalam",
        "Oriya-Tamil", "Oriya-Telugu", "Pashto-Latin/BGN", "Persian-Latin/BGN", "Russian-Latin/BGN",
        "Serbian-Latin/BGN", "Syriac-Latin", "Tamil-Bengali", "Tamil-Devanagari", "Tamil-Gujarati",
        "Tamil-Gurmukhi", "Tamil-Kannada", "Tamil-Latin", "Tamil-Malayalam", "Tamil-Oriya",
        "Tamil-Telugu", "Telugu-Bengali", "Telugu-Devanagari", "Telugu-Gujarati", "Telugu-Gurmukhi",
        "Telugu-Kannada", "Telugu-Latin", "Telugu-Malayalam", "Telugu-Oriya", "Telugu-Tamil",
        "Thaana-Latin", "Thai-Latin", "Turkmen-Latin/BGN", "Ukrainian-Latin/BGN", "Uzbek-Latin/BGN",
        "Any-NFC", "Any-NFD", "Any-NFKC", "Any-NFKD", "Any-Null", "Any-Remove", "Any-Upper",
        "Any-Lower", "Any-Title", "Any-Hex", "Hex-Any", "Any-Latin", "Any-Name", "Name-Any",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    v.sort();
    v
}

/// Why a transliterator could not be made.
#[derive(Debug, Clone, PartialEq)]
pub enum TranslitError {
    /// U_INVALID_ID.
    InvalidId,
    /// A rule error: the ICU code and the message context.
    Rule(i64, String),
}

impl Translit {
    /// `Transliterator::create()`.
    pub fn create(id: &str, reverse: bool) -> Result<Translit, TranslitError> {
        let mut steps = Vec::new();
        let mut inverses = Vec::new();
        let parts: Vec<&str> = id.split(';').map(str::trim).filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            return Err(TranslitError::InvalidId);
        }
        let mut global_filter: Option<USet> = None;
        for (k, p) in parts.iter().enumerate() {
            let (filter, rest) = if p.starts_with('[') || p.starts_with("\\p") || p.starts_with("\\P") {
                let c: Vec<char> = p.chars().collect();
                let mut i = 0;
                let f = uset::parse(&c, &mut i).map_err(|_| TranslitError::InvalidId)?;
                let rest: String = c[i..].iter().collect();
                (Some(f), rest.trim().to_string())
            } else {
                (None, p.to_string())
            };
            if rest.is_empty() {
                if k == 0 {
                    global_filter = filter;
                    continue;
                }
                return Err(TranslitError::InvalidId);
            }
            let (b, inv) = builtin(&rest).ok_or(TranslitError::InvalidId)?;
            let step = match filter {
                Some(f) => Step::Filtered(f, Box::new(Step::Builtin(b))),
                None => Step::Builtin(b),
            };
            steps.push(step);
            inverses.push(inv);
        }
        if let Some(f) = global_filter {
            steps = vec![Step::Filtered(f, Box::new(Step::Rules(RuleSet::compound(steps))))];
        }
        let inverse_id = if parts.len() == 1 {
            Some(inverses.first().map_or("Any-Null", |s| *s).to_string())
        } else {
            Some(inverses.iter().rev().cloned().collect::<Vec<_>>().join(";"))
        };
        let mut t = Translit {
            id: id.to_string(),
            steps,
            inverse_id,
        };
        if reverse {
            t = t.inverse()?;
        }
        Ok(t)
    }

    /// `Transliterator::createFromRules()`.
    pub fn from_rules(rules: &str, reverse: bool) -> Result<Translit, TranslitError> {
        let rs = RuleSet::parse(rules, reverse)?;
        Ok(Translit {
            id: "RulesTransPHP".to_string(),
            steps: vec![Step::Rules(rs)],
            inverse_id: None,
        })
    }

    /// `createInverse()`.
    pub fn inverse(&self) -> Result<Translit, TranslitError> {
        match &self.inverse_id {
            Some(i) => {
                let mut t = Translit::create(i, false)?;
                t.id = i.clone();
                t.inverse_id = Some(self.id.clone());
                Ok(t)
            }
            None => Err(TranslitError::InvalidId),
        }
    }

    /// Transforms `s` (whole string).
    pub fn apply(&self, s: &str) -> String {
        let mut cur = s.to_string();
        for st in &self.steps {
            cur = apply_step(st, &cur);
        }
        cur
    }

    /// Transforms the part `[start, end)` (UTF-16 offsets) of `s`.
    pub fn apply_range(&self, s: &str, start: usize, end: usize) -> String {
        let a = crate::utf16::byte_of_u16(s, start);
        let b = crate::utf16::byte_of_u16(s, end);
        format!("{}{}{}", &s[..a], self.apply(&s[a..b]), &s[b..])
    }
}

fn apply_step(st: &Step, s: &str) -> String {
    match st {
        Step::Builtin(b) => apply_builtin(*b, s),
        Step::Rules(r) => r.apply(s),
        Step::Filtered(f, inner) => {
            // runs of characters in the filter are transformed
            let mut out = String::new();
            let mut run = String::new();
            for c in s.chars() {
                if f.contains(c) {
                    run.push(c);
                } else {
                    if !run.is_empty() {
                        out.push_str(&apply_step(inner, &run));
                        run.clear();
                    }
                    out.push(c);
                }
            }
            if !run.is_empty() {
                out.push_str(&apply_step(inner, &run));
            }
            out
        }
    }
}

fn apply_builtin(b: Builtin, s: &str) -> String {
    use intl::unicode as u;
    match b {
        Builtin::Null => s.to_string(),
        Builtin::Remove => String::new(),
        Builtin::Upper => u::uppercase(s.chars()).collect(),
        Builtin::Lower => u::lowercase(s.chars()).collect(),
        Builtin::Title => title(s),
        Builtin::Nfd => u::nfd(s.chars()).collect(),
        Builtin::Nfc => u::nfc(s.chars()).collect(),
        Builtin::Nfkd => u::nfkd(s.chars()).collect(),
        Builtin::Nfkc => u::nfkc(s.chars()).collect(),
        Builtin::AnyHex => s
            .chars()
            .map(|c| {
                if (c as u32) > 0xFFFF {
                    format!("\\U{:08X}", c as u32)
                } else {
                    format!("\\u{:04X}", c as u32)
                }
            })
            .collect(),
        Builtin::HexAny => hex_any(s),
        Builtin::LatinAscii => intl::translit::latin_ascii(s),
        Builtin::GreekLatin => greek_latin(s),
        Builtin::LatinGreek => s.to_string(),
        Builtin::CyrillicLatin => intl::translit::cyrillic_to_latin(s),
        Builtin::ArmenianLatin => intl::translit::armenian_to_latin(s),
        Builtin::GeorgianLatin => intl::translit::georgian_to_latin(s),
        Builtin::KatakanaLatin | Builtin::HiraganaLatin => kana_latin(s),
        Builtin::LatinKatakana => latin_kana(s, true),
        Builtin::LatinHiragana => latin_kana(s, false),
        Builtin::HiraganaKatakana => s
            .chars()
            .map(|c| match c {
                '\u{3041}'..='\u{3096}' => char::from_u32(c as u32 + 0x60).unwrap(),
                _ => c,
            })
            .collect(),
        Builtin::KatakanaHiragana => s
            .chars()
            .map(|c| match c {
                '\u{30A1}'..='\u{30F6}' => char::from_u32(c as u32 - 0x60).unwrap(),
                _ => c,
            })
            .collect(),
        Builtin::AnyLatin => {
            let g = greek_latin(s);
            let k = kana_latin(&g);
            let c = intl::translit::cyrillic_to_latin(&k);
            let a = intl::translit::armenian_to_latin(&c);
            intl::translit::georgian_to_latin(&a)
        }
        Builtin::Accents => s.to_string(),
    }
}

/// Titlecase: the first cased letter of each word upper, the rest lower.
fn title(s: &str) -> String {
    use intl::unicode as u;
    let mut out = String::new();
    let mut in_word = false;
    for c in s.chars() {
        let cased = u::is_uppercase(c) || u::is_lowercase(c) || u::general_category(c) == u::GeneralCategory::TitlecaseLetter;
        if cased {
            if in_word {
                out.extend(u::to_lowercase(c));
            } else {
                out.extend(u::to_titlecase(c));
                in_word = true;
            }
        } else {
            out.push(c);
            // marks and apostrophes stay in the word
            if !(u::is_mark(c) || c == '\'' || c == '\u{2019}') {
                in_word = false;
            }
        }
    }
    out
}

fn hex_any(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let hexrun = |c: &[char], from: usize, min: usize, max: usize| -> Option<(u32, usize)> {
        let mut n = 0;
        while n < max && from + n < c.len() && c[from + n].is_ascii_hexdigit() {
            n += 1;
        }
        if n < min {
            return None;
        }
        let t: String = c[from..from + n].iter().collect();
        u32::from_str_radix(&t, 16).ok().map(|v| (v, n))
    };
    while i < c.len() {
        let mut done = false;
        if c[i] == '\\' && i + 1 < c.len() {
            let (min, max) = match c[i + 1] {
                'u' => (4, 4),
                'U' => (8, 8),
                'x' => (1, 8),
                _ => (0, 0),
            };
            if max > 0 {
                let brace = c.get(i + 2) == Some(&'{');
                let from = i + 2 + brace as usize;
                if let Some((v, n)) = hexrun(&c, from, if brace { 1 } else { min }, if brace { 8 } else { max }) {
                    let end = from + n + brace as usize;
                    if (!brace || c.get(from + n) == Some(&'}'))
                        && let Some(ch) = char::from_u32(v)
                    {
                        out.push(ch);
                        i = end;
                        done = true;
                    }
                }
            }
        } else if (c[i] == 'U' || c[i] == 'u') && c.get(i + 1) == Some(&'+') {
            if let Some((v, n)) = hexrun(&c, i + 2, 4, 6)
                && let Some(ch) = char::from_u32(v)
            {
                out.push(ch);
                i += 2 + n;
                done = true;
            }
        } else if c[i] == '&' && c.get(i + 1) == Some(&'#') && matches!(c.get(i + 2), Some('x') | Some('X'))
            && let Some((v, n)) = hexrun(&c, i + 3, 1, 6)
            && c.get(i + 3 + n) == Some(&';')
            && let Some(ch) = char::from_u32(v)
        {
            out.push(ch);
            i += 4 + n;
            done = true;
        }
        if !done {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

/// Greek to Latin (UNGEGN/ICU Greek-Latin conventions: β b, η ē, ω ō,
/// υ y outside diphthongs, φ ph, χ ch, ψ ps, θ th, γ before a velar n;
/// accents kept as combining marks).
fn greek_latin(s: &str) -> String {
    let d: Vec<char> = intl::unicode::nfd(s.chars()).collect();
    let mut out = String::new();
    let base = |c: char| -> char { intl::unicode::to_lowercase(c).next().unwrap_or(c) };
    let mut i = 0;
    while i < d.len() {
        let c = d[i];
        let lc = base(c);
        let upper = c != lc;
        let next = d.get(i + 1).map(|&n| base(n));
        let prev_vowel = i > 0 && matches!(base(d[i - 1]), 'α' | 'ε' | 'η' | 'ο');
        let s: &str = match lc {
            'α' => "a",
            'β' => "b",
            'γ' => match next {
                Some('γ') | Some('κ') | Some('ξ') | Some('χ') => "n",
                _ => "g",
            },
            'δ' => "d",
            'ε' => "e",
            'ζ' => "z",
            'η' => "ē",
            'θ' => "th",
            'ι' => "i",
            'κ' => "k",
            'λ' => "l",
            'μ' => "m",
            'ν' => "n",
            'ξ' => "x",
            'ο' => "o",
            'π' => "p",
            'ρ' => "r",
            'σ' | 'ς' => "s",
            'τ' => "t",
            'υ' => {
                if prev_vowel {
                    "u"
                } else {
                    "y"
                }
            }
            'φ' => "ph",
            'χ' => "ch",
            'ψ' => "ps",
            'ω' => "ō",
            ';' => "?",
            '·' => ";",
            _ => {
                out.push(c);
                i += 1;
                continue;
            }
        };
        if upper {
            // the next letter decides between "Th" and "TH"
            let next_upper = d.get(i + 1).is_some_and(|&n| base(n) != n);
            let mut cs = s.chars();
            if let Some(f) = cs.next() {
                out.extend(f.to_uppercase());
                for r in cs {
                    if next_upper {
                        out.extend(r.to_uppercase());
                    } else {
                        out.push(r);
                    }
                }
            }
        } else {
            out.push_str(s);
        }
        i += 1;
    }
    intl::unicode::nfc(out.chars()).collect()
}

/// Kana syllables and their romanization (modified Hepburn as in ICU's
/// Katakana-Latin, long vowels with macrons).
const KANA: &[(&str, &str)] = &[
    ("キャ", "kya"), ("キュ", "kyu"), ("キョ", "kyo"), ("シャ", "sha"), ("シュ", "shu"), ("ショ", "sho"),
    ("チャ", "cha"), ("チュ", "chu"), ("チョ", "cho"), ("ニャ", "nya"), ("ニュ", "nyu"), ("ニョ", "nyo"),
    ("ヒャ", "hya"), ("ヒュ", "hyu"), ("ヒョ", "hyo"), ("ミャ", "mya"), ("ミュ", "myu"), ("ミョ", "myo"),
    ("リャ", "rya"), ("リュ", "ryu"), ("リョ", "ryo"), ("ギャ", "gya"), ("ギュ", "gyu"), ("ギョ", "gyo"),
    ("ジャ", "ja"), ("ジュ", "ju"), ("ジョ", "jo"), ("ビャ", "bya"), ("ビュ", "byu"), ("ビョ", "byo"),
    ("ピャ", "pya"), ("ピュ", "pyu"), ("ピョ", "pyo"), ("ティ", "ti"), ("ディ", "di"), ("ファ", "fa"),
    ("フィ", "fi"), ("フェ", "fe"), ("フォ", "fo"), ("ウィ", "wi"), ("ウェ", "we"), ("ウォ", "wo"),
    ("シェ", "she"), ("チェ", "che"), ("ジェ", "je"), ("ヴァ", "va"), ("ヴィ", "vi"), ("ヴェ", "ve"), ("ヴォ", "vo"),
    ("ア", "a"), ("イ", "i"), ("ウ", "u"), ("エ", "e"), ("オ", "o"),
    ("カ", "ka"), ("キ", "ki"), ("ク", "ku"), ("ケ", "ke"), ("コ", "ko"),
    ("サ", "sa"), ("シ", "shi"), ("ス", "su"), ("セ", "se"), ("ソ", "so"),
    ("タ", "ta"), ("チ", "chi"), ("ツ", "tsu"), ("テ", "te"), ("ト", "to"),
    ("ナ", "na"), ("ニ", "ni"), ("ヌ", "nu"), ("ネ", "ne"), ("ノ", "no"),
    ("ハ", "ha"), ("ヒ", "hi"), ("フ", "fu"), ("ヘ", "he"), ("ホ", "ho"),
    ("マ", "ma"), ("ミ", "mi"), ("ム", "mu"), ("メ", "me"), ("モ", "mo"),
    ("ヤ", "ya"), ("ユ", "yu"), ("ヨ", "yo"),
    ("ラ", "ra"), ("リ", "ri"), ("ル", "ru"), ("レ", "re"), ("ロ", "ro"),
    ("ワ", "wa"), ("ヰ", "wi"), ("ヱ", "we"), ("ヲ", "wo"), ("ン", "n"),
    ("ガ", "ga"), ("ギ", "gi"), ("グ", "gu"), ("ゲ", "ge"), ("ゴ", "go"),
    ("ザ", "za"), ("ジ", "ji"), ("ズ", "zu"), ("ゼ", "ze"), ("ゾ", "zo"),
    ("ダ", "da"), ("ヂ", "di"), ("ヅ", "du"), ("デ", "de"), ("ド", "do"),
    ("バ", "ba"), ("ビ", "bi"), ("ブ", "bu"), ("ベ", "be"), ("ボ", "bo"),
    ("パ", "pa"), ("ピ", "pi"), ("プ", "pu"), ("ペ", "pe"), ("ポ", "po"),
    ("ヴ", "vu"), ("ァ", "~a"), ("ィ", "~i"), ("ゥ", "~u"), ("ェ", "~e"), ("ォ", "~o"),
    ("ャ", "~ya"), ("ュ", "~yu"), ("ョ", "~yo"),
];

fn macron(v: char) -> char {
    match v {
        'a' => 'ā',
        'i' => 'ī',
        'u' => 'ū',
        'e' => 'ē',
        'o' => 'ō',
        x => x,
    }
}

fn kana_latin(s: &str) -> String {
    // hiragana as katakana first
    let s: String = s
        .chars()
        .map(|c| match c {
            '\u{3041}'..='\u{3096}' => char::from_u32(c as u32 + 0x60).unwrap(),
            _ => c,
        })
        .collect();
    let mut out = String::new();
    let mut rest = s.as_str();
    let mut sokuon = false;
    while let Some(c) = rest.chars().next() {
        if c == 'ッ' {
            sokuon = true;
            rest = &rest[c.len_utf8()..];
            continue;
        }
        if c == 'ー' {
            if let Some(last) = out.pop() {
                out.push(macron(last));
            }
            rest = &rest[c.len_utf8()..];
            continue;
        }
        if let Some((k, r)) = KANA.iter().find(|(k, _)| rest.starts_with(k)) {
            if sokuon {
                if let Some(f) = r.chars().next() {
                    out.push(if r.starts_with("ch") { 't' } else { f });
                }
                sokuon = false;
            }
            // n before a vowel or y is written n'
            if out.ends_with('n') && rest.starts_with("ン") {
                // handled below
            }
            out.push_str(r);
            rest = &rest[k.len()..];
            continue;
        }
        if sokuon {
            out.push('~');
            sokuon = false;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

fn latin_kana(s: &str, katakana: bool) -> String {
    let mut out = String::new();
    let lower: String = s.to_lowercase();
    let mut rest = lower.as_str();
    'outer: while let Some(c) = rest.chars().next() {
        // macrons: the vowel then a long mark
        let plain = match c {
            'ā' => Some('a'),
            'ī' => Some('i'),
            'ū' => Some('u'),
            'ē' => Some('e'),
            'ō' => Some('o'),
            _ => None,
        };
        if let Some(p) = plain {
            // a long vowel on its own: the vowel and a long mark
            let k = match p {
                'a' => 'ア',
                'i' => 'イ',
                'u' => 'ウ',
                'e' => 'エ',
                _ => 'オ',
            };
            out.push(k);
            out.push('ー');
            rest = &rest[c.len_utf8()..];
            continue;
        }
        // doubled consonant: sokuon
        let mut it = rest.chars();
        if let (Some(a), Some(b)) = (it.next(), it.next())
            && a == b
            && a.is_ascii_alphabetic()
            && !"aeioun".contains(a)
        {
            out.push('ッ');
            rest = &rest[1..];
            continue;
        }
        // the longest syllable first
        let mut best: Option<(&str, &str)> = None;
        for (k, r) in KANA {
            if r.starts_with('~') {
                continue;
            }
            if rest.starts_with(r) && best.is_none_or(|(_, br)| r.len() > br.len()) {
                best = Some((k, r));
            }
        }
        // a vowel with a macron after a consonant
        for (k, r) in KANA {
            if r.starts_with('~') || r.len() < 2 {
                continue;
            }
            let (cons, v) = r.split_at(r.len() - 1);
            let mv = macron(v.chars().next().unwrap());
            let long = format!("{cons}{mv}");
            if rest.starts_with(&long) {
                out.push_str(k);
                out.push('ー');
                rest = &rest[long.len()..];
                continue 'outer;
            }
        }
        if let Some((k, r)) = best {
            out.push_str(k);
            rest = &rest[r.len()..];
            continue;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    if katakana {
        out
    } else {
        out.chars()
            .map(|c| match c {
                '\u{30A1}'..='\u{30F6}' => char::from_u32(c as u32 - 0x60).unwrap(),
                _ => c,
            })
            .collect()
    }
}

// ---------------------------------------------------------------- rules

#[derive(Clone, Debug)]
struct Rule {
    ante: Vec<char>,
    key: Vec<char>,
    post: Vec<char>,
    out: String,
}

/// Rule-based transforms (ICU user guide "Transform Rule Tutorial").
#[derive(Clone, Debug, Default)]
pub struct RuleSet {
    rules: Vec<Rule>,
    /// A compound of steps (global filters).
    steps: Vec<Step>,
}

/// Unquotes a rule part: `'...'` literals, `\x` escapes, whitespace
/// dropped.
fn unquote(s: &str) -> Vec<char> {
    let c: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '\'' => {
                i += 1;
                if c.get(i) == Some(&'\'') {
                    out.push('\'');
                    i += 1;
                    continue;
                }
                while i < c.len() && c[i] != '\'' {
                    out.push(c[i]);
                    i += 1;
                }
                i += 1;
            }
            '\\' => {
                if let Some((cp, n)) = uset::escape(&c[i + 1..])
                    && let Some(ch) = char::from_u32(cp)
                {
                    out.push(ch);
                    i += 1 + n;
                } else {
                    i += 1;
                }
            }
            x if x.is_whitespace() => i += 1,
            x => {
                out.push(x);
                i += 1;
            }
        }
    }
    out
}

/// Splits outside quotes at the first of `seps`.
fn split_unquoted<'a>(s: &'a str, seps: &[&'static str]) -> Option<(&'a str, &'static str, &'a str)> {
    let mut quote = false;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\'' {
            quote = !quote;
        } else if c == '\\' {
            escaped = true;
        } else if !quote {
            for sep in seps {
                if s[i..].starts_with(sep) {
                    return Some((&s[..i], sep, &s[i + sep.len()..]));
                }
            }
        }
    }
    None
}

impl RuleSet {
    fn compound(steps: Vec<Step>) -> RuleSet {
        RuleSet {
            rules: Vec::new(),
            steps,
        }
    }

    pub fn parse(src: &str, reverse: bool) -> Result<RuleSet, TranslitError> {
        let mut rules: Vec<Rule> = Vec::new();
        let mut shown: Vec<String> = Vec::new();
        let mut offset = 0;
        for stmt in src.split(';') {
            let t = stmt.trim();
            let start = offset;
            offset += stmt.len() + 1;
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let Some((lhs, op, rhs)) = split_unquoted(t, &["<>", "→", "←", "↔", ">", "<"]) else {
                let at = start + (stmt.len() - stmt.trim_start().len());
                let _ = at;
                return Err(TranslitError::Rule(
                    0x1000b,
                    format!("parse error at offset 0, before or at \"{}\"", t),
                ));
            };
            let forward = matches!(op, ">" | "<>" | "→" | "↔");
            let backward = matches!(op, "<" | "<>" | "←" | "↔");
            let (from, to) = if reverse { (rhs, lhs) } else { (lhs, rhs) };
            let applies = if reverse { backward } else { forward };
            if !applies {
                continue;
            }
            let (ante, rest) = match split_unquoted(from, &["{"]) {
                Some((a, _, r)) => (unquote(a), r),
                None => (Vec::new(), from),
            };
            let (key, post) = match split_unquoted(rest, &["}"]) {
                Some((k, _, p)) => (unquote(k), unquote(p)),
                None => (unquote(rest), Vec::new()),
            };
            // the output side's context marks are dropped
            let out_chars: Vec<char> = unquote(to).into_iter().filter(|c| *c != '{' && *c != '}' && *c != '|').collect();
            let rule = Rule {
                ante,
                key,
                post,
                out: out_chars.into_iter().collect(),
            };
            let display = {
                let a: String = rule.ante.iter().collect();
                let k: String = rule.key.iter().collect();
                let p: String = rule.post.iter().collect();
                let q = |s: &str| {
                    if s.chars().all(char::is_alphanumeric) {
                        s.to_string()
                    } else {
                        format!("'{s}'")
                    }
                };
                let d = if a.is_empty() && p.is_empty() {
                    q(&k)
                } else {
                    let qa = if a.is_empty() { String::new() } else { q(&a) };
                    let qp = if p.is_empty() { String::new() } else { q(&p) };
                    format!("{qa}{{{}}}{qp}", q(&k))
                };
                format!("{d} > {};", rule.out)
            };
            // a rule matching whatever an earlier one matches is masked
            // (U_RULE_MASK_ERROR)
            if let Some(i) = rules.iter().position(|r| {
                r.ante.ends_with(&rule.ante) && rule.key.starts_with(&r.key) && r.post.is_empty() && rule.post.is_empty()
                    || (r.ante == rule.ante && r.key == rule.key && r.post == rule.post)
            }) {
                return Err(TranslitError::Rule(
                    0x10015,
                    format!(
                        "parse error after \"{}\", before or at \"{}\"",
                        shown[i], display
                    ),
                ));
            }
            shown.push(display);
            rules.push(rule);
        }
        Ok(RuleSet {
            rules,
            steps: Vec::new(),
        })
    }

    fn apply(&self, s: &str) -> String {
        if !self.steps.is_empty() {
            let mut cur = s.to_string();
            for st in &self.steps {
                cur = apply_step(st, &cur);
            }
            return cur;
        }
        let c: Vec<char> = s.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        'pos: while i < c.len() {
            for r in &self.rules {
                let k = r.key.len();
                if k == 0 || i + k > c.len() || c[i..i + k] != r.key[..] {
                    continue;
                }
                if !r.ante.is_empty() && (i < r.ante.len() || c[i - r.ante.len()..i] != r.ante[..]) {
                    continue;
                }
                if !r.post.is_empty()
                    && (i + k + r.post.len() > c.len() || c[i + k..i + k + r.post.len()] != r.post[..])
                {
                    continue;
                }
                out.push_str(&r.out);
                i += k;
                continue 'pos;
            }
            out.push(c[i]);
            i += 1;
        }
        out
    }
}
