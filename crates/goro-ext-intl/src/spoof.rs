//! `Spoofchecker` (PHP manual): UTS #39 security checks — confusable
//! skeletons (the intl crate's confusables data), restriction levels
//! (section 5.2), mixed numbers, invisible characters, and the allowed
//! characters of locales or of a set.

use crate::uset::USet;

// Checks (PHP's Spoofchecker constants, ICU's USpoofChecks).
pub const SINGLE_SCRIPT_CONFUSABLE: i64 = 1;
pub const MIXED_SCRIPT_CONFUSABLE: i64 = 2;
pub const WHOLE_SCRIPT_CONFUSABLE: i64 = 4;
pub const ANY_CASE: i64 = 8;
pub const RESTRICTION_LEVEL: i64 = 16;
pub const INVISIBLE: i64 = 32;
pub const CHAR_LIMIT: i64 = 64;
pub const MIXED_NUMBERS: i64 = 128;
pub const HIDDEN_OVERLAY: i64 = 256;
pub const ALL_CHECKS: i64 = 0xFFFF;

// Restriction levels.
pub const ASCII: i64 = 0x1000_0000;
pub const SINGLE_SCRIPT_RESTRICTIVE: i64 = 0x2000_0000;
pub const HIGHLY_RESTRICTIVE: i64 = 0x3000_0000;
pub const MODERATELY_RESTRICTIVE: i64 = 0x4000_0000;
pub const MINIMALLY_RESTRICTIVE: i64 = 0x5000_0000;
pub const UNRESTRICTIVE: i64 = 0x6000_0000;

/// A spoof checker's settings.
#[derive(Clone, Debug)]
pub struct Checker {
    pub checks: i64,
    pub level: i64,
    /// Allowed characters (from locales or a set).
    pub allowed: Option<Allowed>,
}

#[derive(Clone, Debug)]
pub enum Allowed {
    /// The scripts of the allowed locales (plus Common and Inherited).
    Scripts(Vec<String>),
    Set(USet, bool),
}

impl Default for Checker {
    fn default() -> Self {
        Checker {
            // PHP's defaults: every check but the char limit (which needs
            // allowed characters)
            checks: ALL_CHECKS & !CHAR_LIMIT,
            level: HIGHLY_RESTRICTIVE,
            allowed: None,
        }
    }
}

fn script_name(c: char) -> &'static str {
    intl::unicode::script(c).long_name()
}

/// The scripts of `s`, Common and Inherited left out.
fn scripts(s: &str) -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    for c in s.chars() {
        let n = script_name(c);
        if n != "Common" && n != "Inherited" && n != "Unknown" && !v.contains(&n) {
            v.push(n);
        }
    }
    v
}

/// The restriction level of `s` (UTS #39 section 5.2).
pub fn restriction_level(s: &str) -> i64 {
    if s.is_ascii() {
        return ASCII;
    }
    let sc = scripts(s);
    if sc.len() <= 1 {
        return SINGLE_SCRIPT_RESTRICTIVE;
    }
    let subset = |set: &[&str]| sc.iter().all(|x| set.contains(x));
    if subset(&["Latin", "Han", "Hiragana", "Katakana"])
        || subset(&["Latin", "Han", "Bopomofo"])
        || subset(&["Latin", "Han", "Hangul"])
    {
        return HIGHLY_RESTRICTIVE;
    }
    if sc.len() == 2 && sc.contains(&"Latin") && !sc.contains(&"Cyrillic") && !sc.contains(&"Greek") {
        return MODERATELY_RESTRICTIVE;
    }
    MINIMALLY_RESTRICTIVE
}

impl Checker {
    /// `isSuspicious()`: the failed checks (0: not suspicious).
    pub fn check(&self, s: &str) -> i64 {
        let mut failed = 0;
        if self.checks & RESTRICTION_LEVEL != 0 && restriction_level(s) > self.level {
            failed |= RESTRICTION_LEVEL;
        }
        if self.checks & MIXED_NUMBERS != 0 {
            let mut zeros: Vec<u32> = Vec::new();
            for c in s.chars() {
                if intl::unicode::general_category(c) == intl::unicode::GeneralCategory::DecimalNumber
                    && let Some(v) = intl::unicode::numeric_value(c).and_then(|n| n.to_i64())
                {
                    let z = c as u32 - v as u32;
                    if !zeros.contains(&z) {
                        zeros.push(z);
                    }
                }
            }
            if zeros.len() > 1 {
                failed |= MIXED_NUMBERS;
            }
        }
        if self.checks & INVISIBLE != 0 {
            let d: Vec<char> = intl::unicode::nfd(s.chars()).collect();
            for w in d.windows(2) {
                if w[0] == w[1] && intl::unicode::canonical_combining_class(w[0]) != 0 {
                    failed |= INVISIBLE;
                }
            }
        }
        if let Some(a) = &self.allowed {
            let ok = s.chars().all(|c| match a {
                Allowed::Scripts(list) => {
                    let n = script_name(c);
                    n == "Common" || n == "Inherited" || list.iter().any(|x| x == n)
                }
                Allowed::Set(set, ci) => {
                    set.contains(c)
                        || (*ci && intl::unicode::to_lowercase(c).chain(intl::unicode::to_uppercase(c)).any(|x| set.contains(x)))
                }
            });
            if !ok {
                failed |= CHAR_LIMIT;
            }
        }
        failed
    }

    /// `areConfusable()`: the confusable checks that hold (0: not
    /// confusable).
    pub fn confusable(&self, a: &str, b: &str) -> i64 {
        let skel = |s: &str| {
            if self.checks & ANY_CASE != 0 {
                intl::unicode::spoof::skeleton(&s.to_lowercase())
            } else {
                intl::unicode::spoof::skeleton(s)
            }
        };
        if skel(a) != skel(b) {
            return 0;
        }
        let sa = scripts(a);
        let sb = scripts(b);
        let mut r = 0;
        if sa.len() <= 1 && sb.len() <= 1 {
            r |= if sa == sb { SINGLE_SCRIPT_CONFUSABLE } else { WHOLE_SCRIPT_CONFUSABLE };
        } else {
            r |= MIXED_SCRIPT_CONFUSABLE;
        }
        let mask = self.checks & (SINGLE_SCRIPT_CONFUSABLE | MIXED_SCRIPT_CONFUSABLE | WHOLE_SCRIPT_CONFUSABLE);
        r & mask
    }
}

/// The scripts a locale writes (its likely script, CLDR likely subtags;
/// Japanese and Korean with their companions).
pub fn locale_scripts(id: &str) -> Vec<String> {
    let p = crate::locale::parse(id);
    let script = if !p.script.is_empty() {
        p.script.clone()
    } else {
        match crate::locale::add_likely(&p.base_id()) {
            Some(m) => crate::locale::parse(&m).script,
            None => return Vec::new(),
        }
    };
    let v: Vec<&str> = match script.as_str() {
        "Jpan" => vec!["Han", "Hiragana", "Katakana"],
        "Kore" => vec!["Han", "Hangul"],
        "Hanb" => vec!["Han", "Bopomofo"],
        "Hans" | "Hant" => vec!["Han"],
        other => {
            let name = include_str!("data/scripts.txt")
                .lines()
                .find_map(|l| l.split_once('\t').filter(|(c, _)| *c == other).map(|(_, n)| n));
            match name {
                Some(n) => vec![n],
                None => vec![],
            }
        }
    };
    v.into_iter().map(str::to_string).collect()
}
