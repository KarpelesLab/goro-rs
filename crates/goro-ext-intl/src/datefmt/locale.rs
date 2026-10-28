//! Locale ids as ICU's uloc functions canonicalize them for the date
//! classes (ICU User Guide "Locale"): "pt-PT" → "pt_PT", charset suffixes
//! dropped ("pt_PT.utf8"), POSIX "@euro" variant, BCP 47 "-u-ca-" calendar
//! extensions turned into the "@calendar=" keyword; plus ICU's valid and
//! actual locale of the CLDR data (`getLocale()`).

use super::data::{self, Loc};
use super::supplemental as sup;
use crate::calendar::CalType;

/// A canonical ICU locale: base id and keywords.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocaleId {
    /// "lang[_Script][_REGION][_VARIANT]".
    pub base: String,
    /// Keywords, lower-case names ("calendar", "currency"), sorted.
    pub keywords: Vec<(String, String)>,
}

impl LocaleId {
    /// Canonicalizes an ICU or BCP 47 locale id.
    pub fn parse(s: &str) -> LocaleId {
        let (main, kw) = match s.split_once('@') {
            Some((a, b)) => (a, Some(b)),
            None => (s, None),
        };
        // a POSIX charset ("pt_PT.utf8")
        let main = main.split('.').next().unwrap_or("");
        let mut keywords: Vec<(String, String)> = Vec::new();
        let mut parts: Vec<String> = Vec::new();
        let mut subtags = main.split(['-', '_']).filter(|p| !p.is_empty()).peekable();
        let mut i = 0;
        while let Some(p) = subtags.next() {
            if p.len() == 1 && i > 0 {
                // an extension: "-u-ca-islamic-nu-latn"
                let singleton = p.to_ascii_lowercase();
                let mut rest: Vec<String> = Vec::new();
                while let Some(q) = subtags.peek() {
                    if q.len() == 1 {
                        break;
                    }
                    rest.push(q.to_ascii_lowercase());
                    subtags.next();
                }
                if singleton == "u" {
                    let mut j = 0;
                    while j < rest.len() {
                        if rest[j].len() == 2 {
                            let key = rest[j].clone();
                            let mut vals = Vec::new();
                            j += 1;
                            while j < rest.len() && rest[j].len() != 2 {
                                vals.push(rest[j].clone());
                                j += 1;
                            }
                            let name = match key.as_str() {
                                "ca" => "calendar",
                                "nu" => "numbers",
                                "co" => "collation",
                                "cu" => "currency",
                                "hc" => "hours",
                                "fw" => "fw",
                                other => other,
                            };
                            let mut v = vals.join("-");
                            if name == "calendar" && v == "gregory" {
                                v = "gregorian".into();
                            }
                            keywords.push((name.to_string(), v));
                        } else {
                            j += 1;
                        }
                    }
                }
                continue;
            }
            let t = match i {
                0 => p.to_ascii_lowercase(),
                _ if p.len() == 4 && p.bytes().all(|b| b.is_ascii_alphabetic()) && parts.len() == 1 => {
                    let mut c = p.to_ascii_lowercase();
                    c[..1].make_ascii_uppercase();
                    c
                }
                _ => p.to_ascii_uppercase(),
            };
            parts.push(t);
            i += 1;
        }
        if parts.first().is_some_and(|l| l == "und" || l == "root") {
            parts[0] = String::new();
        }
        if let Some(kw) = kw {
            for item in kw.split(';') {
                let item = item.trim();
                if item.is_empty() {
                    continue;
                }
                match item.split_once('=') {
                    Some((k, v)) => keywords.push((k.trim().to_ascii_lowercase(), v.trim().to_ascii_lowercase())),
                    None => {
                        // "@euro": the POSIX variant ICU maps to a currency
                        if item.eq_ignore_ascii_case("euro") {
                            keywords.push(("currency".into(), "EUR".into()));
                        }
                    }
                }
            }
        }
        keywords.sort();
        keywords.dedup_by(|a, b| a.0 == b.0);
        let mut base = parts.join("_");
        if base.starts_with('_') && base.len() == 1 {
            base.clear();
        }
        LocaleId { base, keywords }
    }

    pub fn keyword(&self, k: &str) -> Option<&str> {
        self.keywords.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    /// The language subtag.
    pub fn language(&self) -> &str {
        self.base.split('_').next().unwrap_or("")
    }

    /// The region subtag, if any.
    pub fn region(&self) -> Option<&str> {
        self.base.split('_').skip(1).find(|p| {
            (p.len() == 2 && p.bytes().all(|b| b.is_ascii_alphabetic()))
                || (p.len() == 3 && p.bytes().all(|b| b.is_ascii_digit()))
        })
    }

    /// The region, else the likely region of the language.
    pub fn region_or_likely(&self) -> String {
        crate::timezone::names::locale_region(&self.base)
    }

    /// The full id with keywords ("es_ES@calendar=islamic").
    pub fn full(&self) -> String {
        if self.keywords.is_empty() {
            return self.base.clone();
        }
        let kw: Vec<String> = self.keywords.iter().map(|(k, v)| format!("{k}={v}")).collect();
        format!("{}@{}", self.base, kw.join(";"))
    }

    /// The CLDR data locale.
    pub fn data_loc(&self) -> Loc {
        Loc::resolve(&self.base)
    }

    /// ICU's valid locale: the most specific known locale on the fallback
    /// chain ("root" when none).
    pub fn valid_locale(&self) -> String {
        let mut cur = self.base.clone();
        loop {
            if cur.is_empty() {
                return "root".into();
            }
            if data::is_known_locale(&cur) {
                return cur;
            }
            match data::parent_locale(&cur) {
                Some(p) if p != "und" => cur = p,
                _ => return "root".into(),
            }
        }
    }

    /// ICU's actual locale: the valid one without default-content levels.
    pub fn actual_locale(&self) -> String {
        let mut cur = self.valid_locale();
        while data::is_default_content(&cur) {
            match data::parent_locale(&cur) {
                Some(p) if p != "und" => cur = p,
                _ => return "root".into(),
            }
        }
        cur
    }

    /// The calendar a locale selects: its "calendar" keyword, else its
    /// region's preferred calendar (CLDR calendarPreferenceData).
    pub fn calendar(&self) -> CalType {
        if let Some(c) = self.keyword("calendar") {
            return CalType::from_name(c).unwrap_or(CalType::Gregorian);
        }
        let r = self.region_or_likely();
        let pref = sup::CALENDAR_PREFS
            .iter()
            .find(|(t, _)| *t == r)
            .or_else(|| sup::CALENDAR_PREFS.iter().find(|(t, _)| *t == "001"))
            .map(|(_, v)| *v)
            .unwrap_or("gregorian");
        CalType::from_name(pref.split(' ').next().unwrap_or("gregorian")).unwrap_or(CalType::Gregorian)
    }
}

/// The calendar keyword values a region uses (`getKeywordValuesForLocale`
/// with commonly used values).
pub fn preferred_calendars(region: &str) -> Vec<&'static str> {
    let pref = sup::CALENDAR_PREFS
        .iter()
        .find(|(t, _)| *t == region)
        .or_else(|| sup::CALENDAR_PREFS.iter().find(|(t, _)| *t == "001"))
        .map(|(_, v)| *v)
        .unwrap_or("gregorian");
    pref.split(' ').collect()
}

/// The ICU ids of all CLDR locales (available locales lists).
pub fn available_locales() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = sup::LOCALES.iter().copied().filter(|l| *l != "und").collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical() {
        assert_eq!(LocaleId::parse("pt-PT").base, "pt_PT");
        assert_eq!(LocaleId::parse("pt_PT.utf8").base, "pt_PT");
        let l = LocaleId::parse("fr_CA@euro");
        assert_eq!(l.base, "fr_CA");
        assert_eq!(l.keyword("currency"), Some("EUR"));
        let l = LocaleId::parse("en-US-u-ca-islamic-civil");
        assert_eq!(l.keyword("calendar"), Some("islamic-civil"));
        assert_eq!(LocaleId::parse("sl-IT-nedis").base, "sl_IT_NEDIS");
        assert_eq!(LocaleId::parse("zh-hant-tw").base, "zh_Hant_TW");
    }

    #[test]
    fn valid_actual() {
        let l = LocaleId::parse("de-DE");
        assert_eq!(l.valid_locale(), "de_DE");
        assert_eq!(l.actual_locale(), "de");
        assert_eq!(LocaleId::parse("sl-IT-nedis").valid_locale(), "sl");
        assert_eq!(LocaleId::parse("en_UK").valid_locale(), "en");
        assert_eq!(LocaleId::parse("ru_RU").actual_locale(), "ru");
        assert_eq!(LocaleId::parse("pt_PT").actual_locale(), "pt_PT");
        assert_eq!(LocaleId::parse("xx").valid_locale(), "root");
        assert_eq!(LocaleId::parse("fa_IR").calendar(), CalType::Persian);
    }
}
