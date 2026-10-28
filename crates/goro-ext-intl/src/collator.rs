//! `Collator` (PHP manual: Collator): UTS #10 collation with ICU's
//! attributes (ICU user guide "Collation: Customization"), on the intl
//! crate's DUCET collator and CLDR tailorings.

use std::cmp::Ordering;
use std::rc::Rc;

use intl::unicode::collate::{AlternateHandling, Collator as Root, Strength as RootStrength, Tailoring};

use crate::locale;

// Attribute ids and values (Collator class constants, PHP manual).
pub const FRENCH_COLLATION: i64 = 0;
pub const ALTERNATE_HANDLING: i64 = 1;
pub const CASE_FIRST: i64 = 2;
pub const CASE_LEVEL: i64 = 3;
pub const NORMALIZATION_MODE: i64 = 4;
pub const STRENGTH: i64 = 5;
pub const HIRAGANA_QUATERNARY_MODE: i64 = 6;
pub const NUMERIC_COLLATION: i64 = 7;

pub const DEFAULT_VALUE: i64 = -1;
pub const PRIMARY: i64 = 0;
pub const SECONDARY: i64 = 1;
pub const TERTIARY: i64 = 2;
pub const QUATERNARY: i64 = 3;
pub const IDENTICAL: i64 = 15;
pub const OFF: i64 = 16;
pub const ON: i64 = 17;
pub const SHIFTED: i64 = 20;
pub const NON_IGNORABLE: i64 = 21;
pub const LOWER_FIRST: i64 = 24;
pub const UPPER_FIRST: i64 = 25;

/// A collator: the locales it was resolved to and its attributes.
#[derive(Clone)]
pub struct Collator {
    /// The locale ICU's collation data was found for ("root" for the
    /// root order).
    pub actual: String,
    /// The most specific available locale of the request.
    pub valid: String,
    tailoring: Option<Rc<Tailoring>>,
    /// The values of the 8 attributes.
    attrs: [i64; 8],
}

impl std::fmt::Debug for Collator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Collator")
            .field("actual", &self.actual)
            .field("valid", &self.valid)
            .field("attrs", &self.attrs)
            .finish()
    }
}

const DEFAULT_ATTRS: [i64; 8] = [OFF, NON_IGNORABLE, OFF, OFF, OFF, TERTIARY, OFF, OFF];

/// The ICU collation tree's locale for a request (as the tests show:
/// `en_US` is valid with the root order, `uk_UA_ODESSA` resolves to `uk`
/// for both: ext/intl/tests/collator_create.phpt).
fn resolve(id: &str) -> (String, String, Option<Tailoring>) {
    let p = locale::parse(id);
    if p.language.is_empty() || p.language == "root" {
        return ("root".into(), "root".into(), None);
    }
    let mut tag = p.language.clone();
    if !p.script.is_empty() {
        tag.push('-');
        tag.push_str(&p.script);
    }
    let co = p.keywords.get("collation").map(|c| match c.as_str() {
        "phonebook" => "phonebk",
        "traditional" => "trad",
        "dictionary" => "dict",
        other => other,
    });
    let query = match co {
        Some(c) => format!("{tag}-u-co-{c}"),
        None => tag.clone(),
    };
    let avail = locale::available_locales();
    let has = |s: &str| avail.iter().any(|a| a.eq_ignore_ascii_case(s));
    let t = rule_override(&p.language)
        .and_then(Tailoring::parse)
        .or_else(|| Tailoring::for_locale(&query));
    if let Some(t) = t {
        let actual = if !p.script.is_empty() && has(&format!("{}_{}", p.language, p.script)) {
            format!("{}_{}", p.language, p.script)
        } else {
            p.language.clone()
        };
        return (actual.clone(), actual, Some(t));
    }
    // the most specific available locale
    let mut cands = Vec::new();
    if !p.script.is_empty() && !p.region.is_empty() {
        cands.push(format!("{}_{}_{}", p.language, p.script, p.region));
    }
    if !p.region.is_empty() {
        cands.push(format!("{}_{}", p.language, p.region));
    }
    if !p.script.is_empty() {
        cands.push(format!("{}_{}", p.language, p.script));
    }
    cands.push(p.language.clone());
    let valid = cands
        .into_iter()
        .find(|c| has(c))
        .unwrap_or_else(|| "root".to_string());
    ("root".into(), valid, None)
}

/// CLDR standard rules the intl crate's table leaves out (CLDR
/// common/collation/lt.xml: y sorts as a secondary variant of i).
fn rule_override(lang: &str) -> Option<&'static str> {
    Some(match lang {
        "lt" => "&c < č <<< Č &i << y <<< Y &s < š <<< Š &z < ž <<< Ž",
        _ => return None,
    })
}

impl Collator {
    /// A collator for the locale; None when the locale isn't usable.
    pub fn new(id: &str) -> Collator {
        let (actual, valid, t) = resolve(id);
        let mut c = Collator {
            actual,
            valid,
            tailoring: t.map(Rc::new),
            attrs: DEFAULT_ATTRS,
        };
        // locale keywords for the attributes (UTS #35 collation settings)
        let p = locale::parse(id);
        if let Some(v) = p.keywords.get("colnumeric") {
            c.attrs[NUMERIC_COLLATION as usize] = if v == "yes" || v == "true" { ON } else { OFF };
        }
        if let Some(v) = p.keywords.get("colstrength") {
            c.attrs[STRENGTH as usize] = match v.as_str() {
                "primary" | "level1" => PRIMARY,
                "secondary" | "level2" => SECONDARY,
                "quaternary" | "level4" => QUATERNARY,
                "identical" | "identic" => IDENTICAL,
                _ => TERTIARY,
            };
        }
        c
    }

    /// The value of an attribute (None: invalid attribute).
    pub fn attribute(&self, attr: i64) -> Option<i64> {
        self.attrs.get(usize::try_from(attr).ok()?).copied()
    }

    /// Sets an attribute; false when the attribute or value is invalid
    /// (U_ILLEGAL_ARGUMENT_ERROR).
    pub fn set_attribute(&mut self, attr: i64, mut value: i64) -> bool {
        let Some(i) = usize::try_from(attr).ok().filter(|i| *i < 8) else {
            return false;
        };
        if value == DEFAULT_VALUE {
            value = DEFAULT_ATTRS[i];
        }
        let ok = match attr {
            FRENCH_COLLATION | CASE_LEVEL | NORMALIZATION_MODE | HIRAGANA_QUATERNARY_MODE
            | NUMERIC_COLLATION => value == ON || value == OFF,
            ALTERNATE_HANDLING => value == SHIFTED || value == NON_IGNORABLE,
            CASE_FIRST => value == OFF || value == LOWER_FIRST || value == UPPER_FIRST,
            STRENGTH => matches!(value, PRIMARY | SECONDARY | TERTIARY | QUATERNARY | IDENTICAL),
            _ => false,
        };
        if ok {
            self.attrs[i] = value;
        }
        ok
    }

    pub fn strength(&self) -> i64 {
        self.attrs[STRENGTH as usize]
    }

    /// The sort key as 16-bit weights, levels separated by 0 (comparing
    /// keys compares the strings).
    pub fn key(&self, s: &str) -> Vec<u16> {
        let strength = self.strength();
        let shifted = self.attrs[ALTERNATE_HANDLING as usize] == SHIFTED;
        let levels = match strength {
            PRIMARY => 1,
            SECONDARY => 2,
            TERTIARY => 3,
            _ => 4,
        };
        let mut key = match &self.tailoring {
            Some(t) => {
                let k = t.sort_key(s);
                truncate_levels(k, if shifted { levels } else { levels.min(3) })
            }
            None => {
                let alt = if shifted {
                    AlternateHandling::Shifted
                } else {
                    AlternateHandling::NonIgnorable
                };
                let st = match levels {
                    1 => RootStrength::Primary,
                    2 => RootStrength::Secondary,
                    3 => RootStrength::Tertiary,
                    _ => RootStrength::Quaternary,
                };
                let case_level = self.attrs[CASE_LEVEL as usize] == ON;
                let c = Root::new(alt)
                    .with_strength(st)
                    .with_case_level(case_level)
                    .with_numeric(self.attrs[NUMERIC_COLLATION as usize] == ON);
                let mut k = c.sort_key(s);
                if self.attrs[FRENCH_COLLATION as usize] == ON {
                    reverse_level(&mut k, 1);
                }
                if self.attrs[CASE_FIRST as usize] == UPPER_FIRST && levels >= 3 {
                    let lvl = if case_level { 3 } else { 2 };
                    swap_case_band(&mut k, lvl);
                }
                k
            }
        };
        if strength == IDENTICAL {
            key.push(0);
            let nfd: String = intl::unicode::nfd(s.chars()).collect();
            for u in nfd.encode_utf16() {
                // keep the separator unique: code units are shifted by one
                key.push(u.saturating_add(1));
            }
        }
        key
    }

    pub fn compare(&self, a: &str, b: &str) -> Ordering {
        if a == b {
            return Ordering::Equal;
        }
        self.key(a).cmp(&self.key(b))
    }

    /// A byte sort key (`Collator::getSortKey()`): the weights big-endian,
    /// without zero bytes inside a level (ICU keys contain no 0x00 but the
    /// terminator; here each weight byte pair is offset to stay non-zero),
    /// levels separated by 0x01.
    pub fn sort_key_bytes(&self, s: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for w in self.key(s) {
            if w == 0 {
                out.push(0x01);
            } else {
                let hi = (w >> 8) as u8;
                let lo = w as u8;
                if hi != 0 {
                    out.push(hi.max(2));
                }
                out.push(lo.max(2));
            }
        }
        out
    }
}

/// Keeps the first `levels` levels of a 0-separated key.
fn truncate_levels(mut k: Vec<u16>, levels: usize) -> Vec<u16> {
    let mut seen = 0;
    for (i, w) in k.iter().enumerate() {
        if *w == 0 {
            seen += 1;
            if seen == levels {
                k.truncate(i);
                return k;
            }
        }
    }
    k
}

/// The range of level `n` (0-based) in a 0-separated key.
fn level_range(k: &[u16], n: usize) -> Option<std::ops::Range<usize>> {
    let mut start = 0;
    let mut level = 0;
    for (i, w) in k.iter().enumerate() {
        if *w == 0 {
            if level == n {
                return Some(start..i);
            }
            level += 1;
            start = i + 1;
        }
    }
    (level == n).then_some(start..k.len())
}

/// French (backwards) secondary ordering: the secondary weights in reverse
/// (UTS #10 section 3.6 "backwards").
fn reverse_level(k: &mut [u16], n: usize) {
    if let Some(r) = level_range(k, n) {
        k[r].reverse();
    }
}

/// Upper-case first: DUCET's upper-case tertiary band (0x08..=0x0C) sorts
/// before the lower-case one (0x02..=0x07).
fn swap_case_band(k: &mut [u16], n: usize) {
    if let Some(r) = level_range(k, n) {
        for w in &mut k[r] {
            *w = match *w {
                0x08..=0x0C => *w - 6,
                0x02..=0x07 => *w + 6,
                x => x,
            };
        }
    }
}
