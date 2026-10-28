//! Locale identifiers as ext/intl handles them: ICU locale IDs
//! (`language[_Script][_REGION][_VARIANT][@key=value;...]`, ICU user guide
//! "Locale") that also accept BCP 47 language tags (RFC 5646), plus the
//! PHP-level `Locale` class operations (PHP manual: Locale).
//!
//! The parsing rules here are those the expected output of the
//! ext/intl/tests/locale_*.phpt tests shows:
//! - `-` and `_` both separate subtags; the language is the first subtag
//!   (lowercased, any length), with `i-`/`x-` kept as a prefix of it;
//! - a following 4-letter subtag is the script (titlecased), then a 2 or
//!   3 character subtag is the region (uppercased), the rest is the
//!   variant (uppercased, `_`-joined);
//! - a hyphen-only tag with a singleton subtag is read as a BCP 47 tag
//!   for its extensions: `-u-` attributes and keys, other singletons and
//!   private use become keywords (`en-US-u-islamCal` has keyword
//!   `attribute=islamcal`, `de-CH-x-phonebk` has `x=phonebk`);
//! - the RFC 5646 grandfathered tags are kept whole by the PHP functions.

use std::collections::BTreeMap;

/// RFC 5646 section 2.2.8 grandfathered tags with their preferred values
/// (None: deprecated without replacement), in the IANA registry's case.
pub const GRANDFATHERED: &[(&str, Option<&str>)] = &[
    ("art-lojban", Some("jbo")),
    ("cel-gaulish", None),
    ("en-GB-oed", Some("en-GB-oxendict")),
    ("i-ami", Some("ami")),
    ("i-bnn", Some("bnn")),
    ("i-default", None),
    ("i-enochian", None),
    ("i-hak", Some("hak")),
    ("i-klingon", Some("tlh")),
    ("i-lux", Some("lb")),
    ("i-mingo", None),
    ("i-navajo", Some("nv")),
    ("i-pwn", Some("pwn")),
    ("i-tao", Some("tao")),
    ("i-tay", Some("tay")),
    ("i-tsu", Some("tsu")),
    ("no-bok", Some("nb")),
    ("no-nyn", Some("nn")),
    ("sgn-BE-FR", Some("sfb")),
    ("sgn-BE-NL", Some("vgt")),
    ("sgn-CH-DE", Some("sgg")),
    ("zh-guoyu", Some("cmn")),
    ("zh-hakka", Some("hak")),
    ("zh-min", None),
    ("zh-min-nan", Some("nan")),
    ("zh-xiang", Some("hsn")),
];

/// The grandfathered tag `tag` is (case-insensitive, `_` as `-`).
pub fn grandfathered(tag: &str) -> Option<(&'static str, Option<&'static str>)> {
    let t = tag.replace('_', "-");
    GRANDFATHERED
        .iter()
        .find(|(g, _)| g.eq_ignore_ascii_case(&t))
        .copied()
}

/// A locale ID split into its parts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    pub language: String,
    pub script: String,
    pub region: String,
    /// `_`-joined, uppercase.
    pub variant: String,
    /// Sorted by key.
    pub keywords: BTreeMap<String, String>,
    /// The keyword section was malformed (an empty value): ICU's
    /// U_ILLEGAL_ARGUMENT_ERROR for display names.
    pub bad_keywords: bool,
}

fn is_sep(c: char) -> bool {
    c == '-' || c == '_'
}

fn titlecase(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.chars().enumerate() {
        if i == 0 {
            out.push(c.to_ascii_uppercase());
        } else {
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

/// The part of an ID before its keywords (and before a POSIX `.charset`).
fn main_part(id: &str) -> &str {
    let end = id.find(['@', '.']).unwrap_or(id.len());
    &id[..end]
}

/// Splits the main part into subtags.
fn subtags(main: &str) -> Vec<&str> {
    main.split(is_sep).collect()
}

/// Reads language, script, region and variant from the subtags of the
/// main part (ICU's locale ID parse, as the tests show it).
fn parse_main(tags: &[&str]) -> (String, String, String, String) {
    let mut i = 0;
    let mut lang = String::new();
    if let Some(first) = tags.first() {
        if (first.eq_ignore_ascii_case("i") || first.eq_ignore_ascii_case("x")) && tags.len() > 1 {
            lang = format!("{}-{}", first.to_ascii_lowercase(), tags[1].to_ascii_lowercase());
            i = 2;
        } else {
            lang = first.to_ascii_lowercase();
            i = 1;
        }
    }
    let mut script = String::new();
    if i < tags.len() && tags[i].len() == 4 && tags[i].bytes().all(|b| b.is_ascii_alphabetic()) {
        script = titlecase(tags[i]);
        i += 1;
    }
    let mut region = String::new();
    if i < tags.len() && (tags[i].len() == 2 || tags[i].len() == 3) {
        region = tags[i].to_ascii_uppercase();
        i += 1;
    } else if i < tags.len() && tags[i].is_empty() && i + 1 < tags.len() {
        // "en__POSIX": an empty region
        i += 1;
    }
    let variant = tags[i.min(tags.len())..]
        .iter()
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join("_");
    (lang, script, region, variant)
}

/// Whether `id` is read as a BCP 47 tag with extensions: hyphens only, no
/// keywords, and a singleton subtag.
fn is_extended_tag(id: &str) -> bool {
    let first_ok = id
        .split('-')
        .next()
        .is_some_and(|t| t.len() != 1 || t.eq_ignore_ascii_case("x") || t.eq_ignore_ascii_case("i"));
    first_ok
        && !id.contains('_')
        && !id.contains('@')
        && id.contains('-')
        && id.split('-').any(|t| t.len() == 1)
}

/// The legacy key and type of a `-u-` key/type pair (UTS #35 / CLDR
/// bcp47 data: `cu` is `currency`, `co` `collation`, `ca` `calendar`, ...).
fn u_keyword(key: &str, ty: &str) -> (String, String) {
    let k = match key {
        "ca" => "calendar",
        "co" => "collation",
        "cu" => "currency",
        "nu" => "numbers",
        "tz" => "timezone",
        "ka" => "colalternate",
        "kb" => "colbackwards",
        "kc" => "colcaselevel",
        "kf" => "colcasefirst",
        "kk" => "colnormalization",
        "kn" => "colnumeric",
        "kr" => "colreorder",
        "ks" => "colstrength",
        "kv" => "kv",
        "va" => "va",
        "lb" => "lb",
        "ms" => "measure",
        other => other,
    };
    let t = match (key, ty) {
        ("tz", t) => tz_from_bcp47(t).unwrap_or(t).to_string(),
        ("ca", "gregory") => "gregorian".to_string(),
        ("ca", "ethioaa") => "ethiopic-amete-alem".to_string(),
        ("co", "dict") => "dictionary".to_string(),
        ("co", "phonebk") => "phonebook".to_string(),
        ("co", "trad") => "traditional".to_string(),
        ("co", "gb2312") => "gb2312han".to_string(),
        ("ks", "level1") => "primary".to_string(),
        ("ks", "level2") => "secondary".to_string(),
        ("ks", "level3") => "tertiary".to_string(),
        ("ks", "level4") => "quaternary".to_string(),
        ("ks", "identic") => "identical".to_string(),
        (_, "") => "yes".to_string(),
        (_, t) => t.to_string(),
    };
    (k.to_string(), t)
}

/// A few CLDR bcp47 time zone ids (timezone.xml) with their IANA names.
fn tz_from_bcp47(t: &str) -> Option<&'static str> {
    Some(match t {
        "deber" => "Europe/Berlin",
        "frpar" => "Europe/Paris",
        "gblon" => "Europe/London",
        "usnyc" => "America/New_York",
        "uslax" => "America/Los_Angeles",
        "uschi" => "America/Chicago",
        "jptyo" => "Asia/Tokyo",
        "utc" => "Etc/UTC",
        "gmt" => "Etc/GMT",
        _ => return None,
    })
}

/// The keywords of a BCP 47 tag's extensions and private use; None when
/// the tag isn't read as one.
fn tag_keywords(id: &str) -> Option<BTreeMap<String, String>> {
    let mut kw = BTreeMap::new();
    if let Some((g, pref)) = grandfathered(id) {
        if pref.is_none() {
            kw.insert("x".to_string(), g.to_ascii_lowercase());
        }
        return Some(kw);
    }
    let tags: Vec<&str> = id.split('-').collect();
    let mut i = 0;
    if tags[0].len() == 1 {
        if !tags[0].eq_ignore_ascii_case("x") {
            return None;
        }
    } else {
        i = 1;
        while i < tags.len() && tags[i].len() != 1 {
            i += 1;
        }
    }
    let mut seen: Vec<char> = Vec::new();
    while i < tags.len() {
        let s = tags[i].to_ascii_lowercase();
        let Some(c) = s.chars().next() else { break };
        if s.len() != 1 || seen.contains(&c) {
            break;
        }
        seen.push(c);
        let mut j = i + 1;
        if c == 'x' {
            let v: Vec<String> = tags[j..].iter().map(|t| t.to_ascii_lowercase()).collect();
            if !v.is_empty() {
                kw.insert("x".to_string(), v.join("-"));
            }
            break;
        }
        let start = j;
        while j < tags.len() && tags[j].len() != 1 {
            j += 1;
        }
        let parts: Vec<String> = tags[start..j].iter().map(|t| t.to_ascii_lowercase()).collect();
        if c == 'u' {
            let mut k = 0;
            let mut attrs = Vec::new();
            while k < parts.len() && parts[k].len() != 2 {
                attrs.push(parts[k].clone());
                k += 1;
            }
            if !attrs.is_empty() {
                kw.insert("attribute".to_string(), attrs.join("-"));
            }
            while k < parts.len() {
                let key = parts[k].clone();
                k += 1;
                let mut ty = Vec::new();
                while k < parts.len() && parts[k].len() != 2 {
                    ty.push(parts[k].clone());
                    k += 1;
                }
                let (lk, lt) = u_keyword(&key, &ty.join("-"));
                kw.entry(lk).or_insert(lt);
            }
        } else if !parts.is_empty() {
            kw.insert(c.to_string(), parts.join("-"));
        }
        i = j;
    }
    Some(kw)
}

/// Parses `key=value;key=value` (keys lowercased, sorted).
fn parse_keywords(s: &str) -> (BTreeMap<String, String>, bool) {
    let mut kw = BTreeMap::new();
    let mut bad = false;
    for item in s.split(';') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        match item.split_once('=') {
            Some((k, v)) => {
                let k = k.trim().to_ascii_lowercase();
                let v = v.trim();
                if k.is_empty() || v.is_empty() {
                    bad = true;
                    continue;
                }
                kw.entry(k).or_insert_with(|| v.to_string());
            }
            None => bad = true,
        }
    }
    (kw, bad)
}

/// Parses a locale ID the way ICU's locale functions read it.
pub fn parse(id: &str) -> Parsed {
    parse_with(id, false)
}

/// [`parse`]; with `mixed`, a tag mixing `_` and `-` is read as BCP 47
/// from its first singleton too (ICU 70+'s canonicalization:
/// `sl_IT_nedis-a-kirti-x-xyz` is `sl_IT_NEDIS@a=kirti;x=xyz`,
/// ext/intl/tests/locale_filter_matches_icu70.phpt).
fn parse_with(id: &str, mixed: bool) -> Parsed {
    let main = main_part(id);
    let extended = !id.contains('@')
        && (is_extended_tag(main)
            || (mixed && main.contains('-') && main.split(is_sep).skip(1).any(|t| t.len() == 1)));
    let tags = subtags(main);
    let (language, script, region, variant) = if extended {
        // the core subtags before the first singleton (keeping an i-/x-
        // prefix as part of the language)
        let mut end = tags.len();
        for (k, t) in tags.iter().enumerate() {
            if t.len() == 1 && !(k == 0 && (t.eq_ignore_ascii_case("x") || t.eq_ignore_ascii_case("i"))) {
                end = k;
                break;
            }
        }
        parse_main(&tags[..end])
    } else {
        parse_main(&tags)
    };
    let (keywords, bad_keywords) = if let Some(at) = id.find('@') {
        parse_keywords(&id[at + 1..])
    } else if extended {
        let t = main.replace('_', "-");
        let kw = if main.contains('_') {
            // keywords of the hyphenated tail
            let first = t.split('-').position(|x| x.len() == 1).unwrap_or(0);
            let tail: Vec<&str> = t.split('-').skip(first).collect();
            tag_keywords(&format!("und-{}", tail.join("-")))
        } else {
            tag_keywords(main)
        };
        (kw.unwrap_or_default(), false)
    } else {
        (BTreeMap::new(), false)
    };
    Parsed {
        language,
        script,
        region,
        variant,
        keywords,
        bad_keywords,
    }
}

impl Parsed {
    /// The ICU locale ID (`de_DE_1996@currency=EUR`).
    pub fn to_id(&self) -> String {
        let mut s = self.language.clone();
        if !self.script.is_empty() {
            s.push('_');
            s.push_str(&self.script);
        }
        if !self.region.is_empty() {
            s.push('_');
            s.push_str(&self.region);
        }
        if !self.variant.is_empty() {
            if self.region.is_empty() {
                s.push('_');
            }
            s.push('_');
            s.push_str(&self.variant);
        }
        if !self.keywords.is_empty() {
            s.push('@');
            let kw: Vec<String> = self.keywords.iter().map(|(k, v)| format!("{k}={v}")).collect();
            s.push_str(&kw.join(";"));
        }
        s
    }

    /// The ID without keywords.
    pub fn base_id(&self) -> String {
        let mut p = self.clone();
        p.keywords.clear();
        p.to_id()
    }
}

/// `uloc_getLanguage`.
pub fn language(id: &str) -> String {
    parse(id).language
}

/// ICU's canonical form of a locale ID (`Locale::canonicalize()`): case
/// normalized, `-` read as `_`, BCP 47 extensions as keywords and the
/// CLDR language aliases applied (`art-lojban` is `jbo`, `i-lux` is
/// `lb`: ext/intl/tests/locale_filter_matches4.phpt).
pub fn canonicalize(id: &str) -> String {
    let main = main_part(id);
    if let Some((g, pref)) = grandfathered(main)
        && !id.contains('@')
    {
        // regular grandfathered tags without an i- prefix read as ICU IDs
        let is_irregular = g.starts_with("i-") || g == "en-GB-oed" || g.starts_with("sgn-");
        match pref {
            Some(p) if is_irregular || g == "art-lojban" || g.starts_with("no-") || g == "zh-guoyu" || g == "zh-hakka" || g == "zh-xiang" || g == "zh-min-nan" => {
                if !g.starts_with("sgn-") {
                    return parse(p).to_id();
                }
            }
            None if g.starts_with("i-") => {
                return format!("@x={}", g.to_ascii_lowercase());
            }
            _ => {}
        }
    }
    let mut p = parse_with(id, true);
    if is_extended_tag(main) && !id.contains('@') {
        if p.language.starts_with("x-") {
            p.language.clear();
        }
    }
    if let Some(l) = language_alias(&p.language) {
        p.language = l.to_string();
    }
    p.to_id()
}

/// CLDR's deprecated language codes and their replacements (supplemental
/// metadata languageAlias, the common ones).
fn language_alias(l: &str) -> Option<&'static str> {
    Some(match l {
        "iw" => "he",
        "in" => "id",
        "ji" => "yi",
        "jw" => "jv",
        "mo" => "ro",
        "tl" => "fil",
        "no" => "nb",
        _ => return None,
    })
}

/// The variants as the PHP functions list them: the ICU variant split at
/// `_`, up to the first singleton (`sl_IT_nedis-a-kirti-x-xyz` has the
/// variant NEDIS: ext/intl/tests/locale_get_all_variants.phpt).
pub fn variants(id: &str) -> Vec<String> {
    if grandfathered(main_part(id)).is_some() {
        return Vec::new();
    }
    let p = parse(id);
    let mut out = Vec::new();
    for v in p.variant.split('_') {
        if v.is_empty() {
            continue;
        }
        if v.len() == 1 {
            break;
        }
        out.push(v.to_string());
    }
    out
}

/// The private-use subtags of `id` (after an `x` singleton, original
/// case).
pub fn private_subtags(id: &str) -> Vec<String> {
    let main = main_part(id);
    let tags: Vec<&str> = main.split(is_sep).collect();
    match tags.iter().position(|t| t.eq_ignore_ascii_case("x")) {
        Some(p) => tags[p + 1..].iter().map(|s| s.to_string()).collect(),
        None => Vec::new(),
    }
}

/// `Locale::parseLocale()`: `(key, value)` pairs in PHP's order
/// (language, script, region, variantN, privateN), or the grandfathered
/// tag alone.
pub fn parse_locale(id: &str) -> Vec<(String, String)> {
    let main = main_part(id);
    if let Some((_, _)) = grandfathered(main) {
        return vec![("grandfathered".to_string(), main.to_string())];
    }
    let mut out = Vec::new();
    let p = parse(id);
    let starts_singleton = main.split(is_sep).next().is_some_and(|t| t.len() == 1);
    if !starts_singleton {
        if !p.language.is_empty() {
            out.push(("language".to_string(), p.language.clone()));
        }
        if !p.script.is_empty() {
            out.push(("script".to_string(), p.script.clone()));
        }
        if !p.region.is_empty() {
            out.push(("region".to_string(), p.region.clone()));
        }
        for (i, v) in variants(id).into_iter().enumerate() {
            out.push((format!("variant{i}"), v));
        }
    }
    if starts_singleton && !main.split(is_sep).next().unwrap().eq_ignore_ascii_case("x") {
        return out;
    }
    for (i, v) in private_subtags(id).into_iter().enumerate() {
        out.push((format!("private{i}"), v));
    }
    out
}

/// RFC 4647 basic filtering (`Locale::filterMatches()`): `range` is a
/// prefix of `tag` ending at a subtag boundary (case-insensitive, `-` and
/// `_` alike).
pub fn filter_matches(tag: &str, range: &str) -> bool {
    if range == "*" {
        return true;
    }
    let t = tag.replace('-', "_").to_ascii_lowercase();
    let r = range.replace('-', "_").to_ascii_lowercase();
    if r.is_empty() {
        return false;
    }
    t == r || (t.starts_with(&r) && t.as_bytes().get(r.len()) == Some(&b'_'))
}

/// RFC 4647 lookup (`Locale::lookup()`): the first of `tags` equal to
/// `range` truncated subtag by subtag; `tags` are compared lowercased with
/// `_` separators. Returns the index.
pub fn lookup(tags: &[String], range: &str) -> Option<usize> {
    let norm = |s: &str| s.replace('-', "_").to_ascii_lowercase();
    let cands: Vec<String> = tags.iter().map(|t| norm(t)).collect();
    let mut r = norm(range);
    if let Some(at) = r.find('@') {
        r.truncate(at);
    }
    loop {
        if let Some(i) = cands.iter().position(|c| *c == r) {
            return Some(i);
        }
        let Some(cut) = r.rfind('_') else { return None };
        r.truncate(cut);
        // drop a trailing singleton too (RFC 4647 section 3.4)
        if let Some(c2) = r.rfind('_')
            && r.len() - c2 == 2
        {
            r.truncate(c2);
        }
    }
}

/// Likely subtags (CLDR, UTS #35 "Add Likely Subtags"): `en` is
/// `en_Latn_US`. None for IDs that aren't locale-shaped.
pub fn add_likely(id: &str) -> Option<String> {
    let p = parse(id);
    if !valid_shape(&p) {
        return None;
    }
    let tag = to_bcp47_core(&p);
    let l = intl::locale::Locale::parse(&tag).ok()?;
    let m = l.maximize();
    Some(core_to_id(&m, &p))
}

/// UTS #35 "Remove Likely Subtags": `en_Latn_US` is `en`.
pub fn minimize(id: &str) -> Option<String> {
    let p = parse(id);
    if !valid_shape(&p) {
        return None;
    }
    let tag = to_bcp47_core(&p);
    let l = intl::locale::Locale::parse(&tag).ok()?;
    let m = l.minimize();
    Some(core_to_id(&m, &p))
}

fn valid_shape(p: &Parsed) -> bool {
    let l = &p.language;
    (l.is_empty() || (2..=8).contains(&l.len()) && l.bytes().all(|b| b.is_ascii_alphabetic()))
        && p.region.bytes().all(|b| b.is_ascii_alphanumeric())
        && p.variant.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn to_bcp47_core(p: &Parsed) -> String {
    let mut s = if p.language.is_empty() {
        "und".to_string()
    } else {
        p.language.clone()
    };
    if !p.script.is_empty() {
        s.push('-');
        s.push_str(&p.script);
    }
    if !p.region.is_empty() {
        s.push('-');
        s.push_str(&p.region);
    }
    s
}

fn core_to_id(l: &intl::locale::Locale, p: &Parsed) -> String {
    let mut q = p.clone();
    q.language = l.language.clone();
    q.script = l.script.clone().unwrap_or_default();
    q.region = l.region.clone().unwrap_or_default();
    q.to_id()
}

/// BCP 47 form of an ICU locale ID (`Locale::toLanguageTag`-like, used
/// for display and the formatters): `de_DE@collation=phonebook` is
/// `de-DE-u-co-phonebk`.
pub fn to_bcp47(id: &str) -> String {
    let p = parse(id);
    let mut s = if p.language.is_empty() {
        "und".to_string()
    } else {
        p.language.clone()
    };
    if !p.script.is_empty() {
        s.push('-');
        s.push_str(&p.script);
    }
    if !p.region.is_empty() {
        s.push('-');
        s.push_str(&p.region);
    }
    for v in p.variant.split('_').filter(|v| !v.is_empty()) {
        s.push('-');
        s.push_str(&v.to_ascii_lowercase());
    }
    let mut u = Vec::new();
    for (k, v) in &p.keywords {
        let (bk, bv) = match (k.as_str(), v.as_str()) {
            ("calendar", "gregorian") => ("ca", "gregory".to_string()),
            ("calendar", v) => ("ca", v.to_string()),
            ("collation", "phonebook") => ("co", "phonebk".to_string()),
            ("collation", "traditional") => ("co", "trad".to_string()),
            ("collation", "dictionary") => ("co", "dict".to_string()),
            ("collation", v) => ("co", v.to_string()),
            ("currency", v) => ("cu", v.to_ascii_lowercase()),
            ("numbers", v) => ("nu", v.to_string()),
            ("colnumeric", v) => ("kn", if v == "yes" { "true".into() } else { v.to_string() }),
            _ => continue,
        };
        u.push(format!("{bk}-{bv}"));
    }
    if !u.is_empty() {
        s.push_str("-u-");
        s.push_str(&u.join("-"));
    }
    s
}

/// The value of keyword `key` (lowercase) in `id`.
pub fn keyword(id: &str, key: &str) -> Option<String> {
    parse(id).keywords.get(key).cloned()
}

/// Scripts written right to left (UAX #9 / CLDR layout data: the scripts
/// whose characters are strong R/AL).
const RTL_SCRIPTS: &[&str] = &[
    "Adlm", "Arab", "Aran", "Armi", "Avst", "Chrs", "Cprt", "Elym", "Hatr", "Hebr", "Hung", "Khar",
    "Lydi", "Mand", "Mani", "Mend", "Merc", "Mero", "Narb", "Nbat", "Nkoo", "Orkh", "Ougr", "Palm",
    "Phli", "Phlp", "Phnx", "Prti", "Rohg", "Samr", "Sarb", "Sogd", "Sogo", "Syrc", "Thaa", "Yezi",
];

/// `Locale::isRightToLeft()`: the locale's (likely) script is RTL.
pub fn is_right_to_left(id: &str) -> bool {
    let p = parse(id);
    let script = if !p.script.is_empty() {
        p.script
    } else {
        match add_likely(&p.base_id()) {
            Some(m) => parse(&m).script,
            None => return false,
        }
    };
    RTL_SCRIPTS.contains(&script.as_str())
}

// ---------------------------------------------------------------- display names

static DISPLAY: &str = include_str!("data/display.txt");

struct DisplayData {
    map: std::collections::HashMap<(u8, &'static str, &'static str), &'static str>,
}

fn display_data() -> &'static DisplayData {
    static D: std::sync::OnceLock<DisplayData> = std::sync::OnceLock::new();
    D.get_or_init(|| {
        let mut map = std::collections::HashMap::new();
        for line in DISPLAY.lines() {
            if line.starts_with('#') {
                continue;
            }
            let mut it = line.splitn(4, '\t');
            let (Some(k), Some(l), Some(c), Some(n)) = (it.next(), it.next(), it.next(), it.next())
            else {
                continue;
            };
            map.insert((k.as_bytes()[0], l, c), n);
        }
        DisplayData { map }
    })
}

/// The CLDR parent chain of a display locale (`fr_CA` → `fr`), as BCP 47
/// tags of the data, ending before root.
fn display_chain(display: &str) -> Vec<String> {
    let p = parse(display);
    let mut out = Vec::new();
    let lang = if p.language.is_empty() { "en".to_string() } else { p.language.clone() };
    if !p.script.is_empty() && !p.region.is_empty() {
        out.push(format!("{lang}-{}-{}", p.script, p.region));
    }
    if !p.script.is_empty() {
        out.push(format!("{lang}-{}", p.script));
    }
    if !p.region.is_empty() {
        out.push(format!("{lang}-{}", p.region));
    }
    out.push(lang);
    out
}

fn display_lookup(kind: u8, display: &str, code: &str) -> Option<&'static str> {
    let d = display_data();
    for l in display_chain(display) {
        if let Some(n) = d.map.get(&(kind, l.as_str(), code)) {
            return Some(n);
        }
    }
    None
}

fn display_pattern(display: &str, which: &str) -> &'static str {
    display_lookup(b'P', display, which).unwrap_or(match which {
        "pattern" => "{0} ({1})",
        "separator" => "{0}, {1}",
        _ => "{0}: {1}",
    })
}

/// CLDR's language aliases (`sgn-GB` is `bfi`, `i-klingon` is `tlh`).
fn lang_alias(key: &str) -> Option<&'static str> {
    static M: std::sync::OnceLock<std::collections::HashMap<&'static str, &'static str>> =
        std::sync::OnceLock::new();
    M.get_or_init(|| {
        include_str!("data/lang_alias.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split_once('\t'))
            .collect()
    })
    .get(key)
    .copied()
}

/// The language ICU displays for `id`: its CLDR alias (whole tag, then
/// language and region, then language) when it has one with a language
/// (ext/intl/tests/locale_get_display_language3.phpt: `i-hak` is Hakka
/// Chinese, `sgn-GB` shows `bfi`), else the parsed language.
pub fn display_language_code(id: &str) -> String {
    let p = parse(id);
    let main = main_part(id).replace('_', "-").to_ascii_lowercase();
    let mut keys = vec![main];
    if !p.region.is_empty() {
        keys.push(format!("{}-{}", p.language, p.region.to_ascii_lowercase()));
    }
    keys.push(p.language.clone());
    for k in keys {
        if let Some(r) = lang_alias(&k)
            && !r.contains("-x-")
        {
            let lang = r.split(['-', '_']).next().unwrap_or("");
            if !lang.is_empty() && lang != "und" && lang != "x" {
                return lang.to_string();
            }
        }
    }
    p.language
}

/// A language's display name; the code itself when CLDR has none.
pub fn display_language(code: &str, display: &str) -> String {
    if code == "root" {
        // ICU 68+ shows root as the unknown language (CLDR "und"):
        // ext/intl/tests/locale_get_display_name8.phpt
        return display_lookup(b'L', display, "und").unwrap_or("und").to_string();
    }
    display_lookup(b'L', display, code).map_or_else(|| code.to_string(), str::to_string)
}

/// Whether CLDR names the language (`NumberFormatter` and friends reject
/// unknown languages: ext/intl/tests/gh12727.phpt).
pub fn is_known_language(code: &str) -> bool {
    code == "root" || display_data().map.contains_key(&(b'L', "en", code))
}

/// A script's display name (the stand-alone form when `standalone`).
pub fn display_script(code: &str, display: &str, standalone: bool) -> String {
    if standalone && let Some(n) = display_lookup(b's', display, code) {
        return n.to_string();
    }
    display_lookup(b'S', display, code).map_or_else(|| code.to_string(), str::to_string)
}

/// CLDR's replacements of deprecated region codes (supplemental
/// aliases.json territoryAlias, first replacement): ICU shows `CS` as
/// Serbia (ext/intl/tests/locale_get_display_name8.phpt).
fn region_alias(code: &str) -> &str {
    match code {
        "CS" | "YU" => "RS",
        "DD" => "DE",
        "FX" => "FR",
        "TP" => "TL",
        "UK" => "GB",
        "ZR" => "CD",
        "BU" => "MM",
        "AN" => "CW",
        "NT" => "SA",
        "SU" => "RU",
        "YD" => "YE",
        "VD" => "VN",
        "RH" => "ZW",
        "HV" => "BF",
        "DY" => "BJ",
        "NH" => "VU",
        "QU" => "EU",
        "CT" => "KI",
        "JT" | "MI" | "PU" | "WK" => "UM",
        "PC" => "FM",
        "PZ" => "PA",
        "NQ" | "FQ" => "AQ",
        c => c,
    }
}

pub fn display_region(code: &str, display: &str) -> String {
    display_lookup(b'R', display, region_alias(code)).map_or_else(|| code.to_string(), str::to_string)
}

pub fn display_variant(code: &str, display: &str) -> String {
    display_lookup(b'V', display, code).map_or_else(|| code.to_string(), str::to_string)
}

/// A keyword's display: "key=value" with CLDR's names of both.
pub fn display_keyword(key: &str, value: &str, display: &str) -> String {
    let cldr_key = match key {
        "colstrength" => "colStrength",
        "colalternate" => "colAlternate",
        "colbackwards" => "colBackwards",
        "colcasefirst" => "colCaseFirst",
        "colcaselevel" => "colCaseLevel",
        "colnormalization" => "colNormalization",
        "colnumeric" => "colNumeric",
        "colreorder" => "colReorder",
        k => k,
    };
    let kname = display_lookup(b'K', display, cldr_key).unwrap_or(key);
    let vname = if key == "currency" {
        display_lookup(b'C', display, &value.to_ascii_uppercase())
    } else {
        display_lookup(b'T', display, &format!("{cldr_key}/{value}"))
    };
    format!("{}={}", kname, vname.unwrap_or(value))
}

/// The full display name (`Locale::getDisplayName()`); None for IDs with
/// malformed keywords.
pub fn display_name(id: &str, display: &str) -> Option<String> {
    let p = parse(id);
    if p.bad_keywords {
        return None;
    }
    let lang = if p.language.is_empty() {
        String::new()
    } else {
        display_language(&p.language, display)
    };
    let mut parts = Vec::new();
    if !p.script.is_empty() {
        parts.push(display_script(&p.script, display, false));
    }
    if !p.region.is_empty() {
        parts.push(display_region(&p.region, display));
    }
    if !p.variant.is_empty() {
        parts.push(display_variant(&p.variant, display));
    }
    for (k, v) in &p.keywords {
        parts.push(display_keyword(k, v, display));
    }
    if parts.is_empty() {
        return Some(lang);
    }
    let sep = display_pattern(display, "separator");
    let mut rest = parts[0].clone();
    for x in &parts[1..] {
        rest = sep.replace("{0}", &rest).replace("{1}", x);
    }
    if lang.is_empty() {
        return Some(rest);
    }
    let pat = display_pattern(display, "pattern");
    Some(pat.replace("{0}", &lang).replace("{1}", &rest))
}

/// The locales ICU has data for (CLDR's), ICU-style.
pub fn available_locales() -> &'static [&'static str] {
    static L: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    L.get_or_init(|| include_str!("data/locales.txt").lines().filter(|l| !l.is_empty()).collect())
}

/// `Locale::acceptFromHttp()`: the best available locale for an
/// Accept-Language header (RFC 9110 section 12.5.4 q-values, then lookup
/// among the available locales).
pub fn accept_from_http(header: &str) -> Option<String> {
    let mut items: Vec<(f64, usize, String)> = Vec::new();
    for (i, part) in header.split(',').enumerate() {
        let mut it = part.split(';');
        let tag = it.next().unwrap_or("").trim();
        if tag.is_empty() {
            continue;
        }
        let mut q = 1.0;
        for p in it {
            let p = p.trim();
            if let Some(v) = p.strip_prefix("q=") {
                q = v.trim().parse().unwrap_or(0.0);
            }
        }
        items.push((q, i, tag.to_string()));
    }
    items.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
    let avail = available_locales();
    for (q, _, tag) in items {
        if q <= 0.0 {
            continue;
        }
        let id = canonicalize(&tag);
        let mut r = id.to_ascii_lowercase();
        loop {
            if let Some(a) = avail.iter().find(|a| a.eq_ignore_ascii_case(&r)) {
                return Some(a.to_string());
            }
            match r.rfind('_') {
                Some(c) => r.truncate(c),
                None => break,
            }
        }
    }
    None
}
