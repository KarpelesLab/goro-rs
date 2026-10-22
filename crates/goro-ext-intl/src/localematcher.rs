//! ICU's LocaleMatcher (localematcher.cpp, locdistance.cpp) as
//! uloc_acceptLanguageFromHTTP() uses it over ICU's installed locales:
//! maximized (language, script, region) triples with explicit-subtag flags,
//! CLDR's language/script/region distances (the parts that matter for
//! ICU's locale list), demotion per desired locale and the threshold.
//!
//! The distance data is a distilled subset of CLDR's languageMatching.

use std::cell::OnceCell;
use std::rc::Rc;

use super::uloc;

const FLAG_LANGUAGE: u8 = 4;
const FLAG_SCRIPT: u8 = 2;
const FLAG_REGION: u8 = 1;

/// Distances are in CLDR units; ICU shifts them by 3 bits to add the
/// "micro distance" of differing explicit subtags.
const SHIFT: u32 = 3;
/// The default threshold (scripts differing) and demotion per desired
/// locale (the distance from en to en-GB).
const THRESHOLD: u32 = 50;
const DEMOTION: u32 = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lsr {
    pub language: String,
    pub script: String,
    pub region: String,
    pub flags: u8,
}

/// XLikelySubtags::makeMaximizedLsrFrom(): None for "und" (and unknown
/// languages without likely subtags data are kept as they are).
pub fn lsr(id: &[u8]) -> Option<Lsr> {
    // a private use tag ("x-...", "@x=...") is its own language
    if id.starts_with(b"@x=") {
        return None;
    }
    let st = uloc::get_subtags(id).ok()?;
    let mut language = String::from_utf8_lossy(&st.language).to_ascii_lowercase();
    let mut script = String::from_utf8_lossy(&st.script).into_owned();
    let mut region = String::from_utf8_lossy(&st.region).into_owned();
    // the root locale ("und" alone) matches nothing
    if language.is_empty() && script.is_empty() && region.is_empty() && st.end >= id.len() && st.variant.is_empty() {
        return None;
    }
    if script == "Zzzz" {
        script.clear();
    }
    if region == "ZZ" {
        region.clear();
    }
    let mut flags = 0;
    if !language.is_empty() {
        flags |= FLAG_LANGUAGE;
    }
    if !script.is_empty() {
        flags |= FLAG_SCRIPT;
    }
    if !region.is_empty() {
        flags |= FLAG_REGION;
    }
    // deprecated language / region codes replaced by a plain one; a
    // language whose replacement is more than a language (sh → sr-Latn)
    // has no likely subtags of its own
    let mut known = !language.is_empty();
    if !language.is_empty() {
        if let Some(c) = intl::locale::canonicalize(&language) {
            if c != language {
                if !c.is_empty() && c.bytes().all(|b| b.is_ascii_lowercase()) {
                    language = c;
                } else {
                    known = false;
                }
            }
        }
    }
    if !region.is_empty() {
        if let Some(c) = intl::locale::canonicalize(&format!("und-{}", region)) {
            if let Some(r) = c.strip_prefix("und-") {
                if r.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()) {
                    region = r.to_string();
                }
            }
        }
    }
    let likely = |tag: &str| -> Option<intl::locale::Locale> {
        Some(intl::locale::Locale::parse(tag).ok()?.maximize())
    };
    if known {
        known = likely(&language).is_some_and(|m| m.script.is_some());
    }
    let m = if known {
        let mut tag = language.clone();
        for p in [&script, &region] {
            if !p.is_empty() {
                tag.push('-');
                tag.push_str(p);
            }
        }
        // (subtags that are not well-formed are kept as they are)
        likely(&tag).or_else(|| likely(&language))
    } else if !script.is_empty() {
        // und-Script-Region: its own likely subtags, else the region's
        // when its language uses the script, else the script's
        let by_region = (!region.is_empty())
            .then(|| likely(&format!("und-{}", region)))
            .flatten();
        let full = (!region.is_empty())
            .then(|| likely(&format!("und-{}-{}", script, region)))
            .flatten()
            .filter(|m| by_region.as_ref().is_none_or(|r| r.language != m.language));
        full.or_else(|| by_region.filter(|m| m.script.as_deref() == Some(script.as_str())))
            .or_else(|| likely(&format!("und-{}", script)))
            .or_else(|| likely("und"))
    } else if !region.is_empty() {
        likely(&format!("und-{}", region)).or_else(|| likely("und"))
    } else {
        likely("und")
    };
    let m = m.filter(|m| !m.language.is_empty())?;
    let l = if language.is_empty() { m.language } else { language };
    Some(Lsr {
        language: l,
        script: if script.is_empty() { m.script.unwrap_or_default() } else { script },
        region: if region.is_empty() { m.region.unwrap_or_default() } else { region },
        flags,
    })
}

/// Macroregions: (code, within $enUS, within $americas); None when it
/// contains regions of both sides.
const MACROREGIONS: &[(&str, Option<bool>, Option<bool>)] = &[
    ("001", None, None),
    ("002", Some(false), Some(false)),
    ("005", Some(false), Some(true)),
    ("009", None, Some(false)),
    ("011", Some(false), Some(false)),
    ("013", Some(false), Some(true)),
    ("014", Some(false), Some(false)),
    ("015", Some(false), Some(false)),
    ("017", Some(false), Some(false)),
    ("018", Some(false), Some(false)),
    ("019", None, Some(true)),
    ("021", None, Some(true)),
    ("029", None, Some(true)),
    ("030", Some(false), Some(false)),
    ("034", Some(false), Some(false)),
    ("035", None, Some(false)),
    ("039", Some(false), Some(false)),
    ("053", Some(false), Some(false)),
    ("054", Some(false), Some(false)),
    ("057", None, Some(false)),
    ("061", None, Some(false)),
    ("142", None, Some(false)),
    ("143", Some(false), Some(false)),
    ("145", Some(false), Some(false)),
    ("150", Some(false), Some(false)),
    ("151", Some(false), Some(false)),
    ("154", Some(false), Some(false)),
    ("155", Some(false), Some(false)),
    ("202", Some(false), Some(false)),
    ("419", None, Some(true)),
];

/// Which side of a partition (`set`, the $enUS or $americas regions) a
/// region is on; None for both or unknown (private use, macroregions
/// across it).
fn side(r: &str, set: &[&str], americas: bool) -> Option<bool> {
    if let Some(m) = MACROREGIONS.iter().find(|m| m.0 == r) {
        return if americas { m.2 } else { m.1 };
    }
    let private = r == "AA"
        || r == "ZZ"
        || r.bytes().all(|b| b.is_ascii_digit())
        || (r.len() == 2 && ((r.starts_with('Q') && r.as_bytes()[1] >= b'M') || r.starts_with('X')));
    if private {
        return None;
    }
    Some(set.contains(&r))
}

/// CLDR's $enUS region set.
const EN_US: &[&str] = &["AS", "CA", "GU", "MH", "MP", "PH", "PR", "UM", "US", "VI"];

/// CLDR's $americas (019).
const AMERICAS: &[&str] = &[
    "019", "419", "005", "013", "021", "029", "AG", "AI", "AR", "AW", "BB", "BL", "BM", "BO", "BQ", "BR", "BS",
    "BZ", "CA", "CL", "CO", "CR", "CU", "CW", "DM", "DO", "EC", "FK", "GD", "GF", "GL", "GP", "GT", "GY", "HN",
    "HT", "JM", "KN", "KY", "LC", "MF", "MQ", "MS", "MX", "NI", "PA", "PE", "PM", "PR", "PY", "SR", "SV", "SX",
    "TC", "TT", "UM", "US", "UY", "VC", "VE", "VG", "VI",
];

fn region_distance(lang: &str, d: &str, s: &str) -> u32 {
    if d == s {
        return 0;
    }
    let part = |set: &[&str], americas: bool| -> u32 {
        match (side(d, set, americas), side(s, set, americas)) {
            (Some(a), Some(b)) if a != b => 5,
            _ => 4,
        }
    };
    match lang {
        "en" => part(EN_US, false),
        "es" | "pt" => part(AMERICAS, true),
        _ => 4,
    }
}

/// (language, desired script, supported script, distance), one way.
const SCRIPT_MATCHES: &[(&str, &str, &str, u32)] = &[
    ("ar", "Latn", "Arab", 20),
    ("bn", "Latn", "Beng", 20),
    ("gu", "Latn", "Gujr", 20),
    ("hi", "Latn", "Deva", 20),
    ("kn", "Latn", "Knda", 20),
    ("ml", "Latn", "Mlym", 20),
    ("mr", "Latn", "Deva", 20),
    ("ta", "Latn", "Taml", 20),
    ("te", "Latn", "Telu", 20),
    ("zh", "Latn", "Hans", 20),
    ("zh", "Hani", "Hans", 20),
    ("zh", "Hant", "Hans", 15),
    ("zh", "Hans", "Hant", 19),
    ("ja", "Latn", "Jpan", 5),
    ("ja", "Hani", "Jpan", 5),
    ("ja", "Hira", "Jpan", 5),
    ("ja", "Kana", "Jpan", 5),
    ("ja", "Hrkt", "Jpan", 5),
    ("ja", "Hira", "Hrkt", 5),
    ("ja", "Kana", "Hrkt", 5),
    ("ko", "Hani", "Kore", 5),
    ("ko", "Hang", "Kore", 5),
    ("ko", "Jamo", "Kore", 5),
    ("ko", "Jamo", "Hang", 5),
];

fn script_distance(lang: &str, d: &str, s: &str) -> u32 {
    if d == s {
        return 0;
    }
    SCRIPT_MATCHES
        .iter()
        .find(|(l, ds, ss, _)| *l == lang && *ds == d && *ss == s)
        .map(|m| m.3)
        .unwrap_or(THRESHOLD)
}

fn language_distance(d: &str, s: &str) -> Option<u32> {
    if d == s {
        return Some(0);
    }
    if matches!((d, s), ("sh", "bs" | "hr" | "sr") | ("hr" | "sr", "bs") | ("sr", "hr") | ("no", "nb")) {
        return Some(4);
    }
    if uloc::data::LANGUAGE_FALLBACKS.iter().any(|(a, b)| *a == d && *b == s) {
        return Some(30);
    }
    None
}

/// The distance between a desired and a supported locale (None when the
/// languages do not match).
fn distance(d: &Lsr, s: &Lsr) -> Option<u32> {
    let l = language_distance(&d.language, &s.language)?;
    let sc = script_distance(&d.language, &d.script, &s.script);
    let r = if sc >= THRESHOLD {
        0
    } else {
        region_distance(&d.language, &d.region, &s.region)
    };
    Some(l + sc + r)
}

thread_local! {
    static SUPPORTED: OnceCell<Rc<Vec<(usize, Lsr)>>> = const { OnceCell::new() };
}

/// The paradigm locales of CLDR's languageMatching.
const PARADIGMS: &[(&str, &str, &str)] = &[
    ("en", "Latn", "US"),
    ("en", "Latn", "GB"),
    ("es", "Latn", "ES"),
    ("es", "Latn", "419"),
    ("pt", "Latn", "BR"),
    ("pt", "Latn", "PT"),
];

/// The supported locales in LocaleMatcher's order (the default, the
/// paradigm locales, the others), one per distinct LSR and flags.
fn supported() -> Rc<Vec<(usize, Lsr)>> {
    SUPPORTED.with(|c| {
        c.get_or_init(|| {
            let lsrs: Vec<Option<Lsr>> = uloc::data::AVAILABLE
                .iter()
                .map(|a| lsr(a.as_bytes()))
                .collect();
            let mut order: Vec<(u8, usize)> = Vec::new();
            let def = lsrs.first().cloned().flatten();
            for (i, l) in lsrs.iter().enumerate() {
                let Some(l) = l else {
                    continue;
                };
                let equiv = |a: &Lsr, b: &Lsr| a.language == b.language && a.script == b.script && a.region == b.region;
                let o = if def.as_ref().is_some_and(|d| equiv(d, l)) {
                    1
                } else if PARADIGMS
                    .iter()
                    .any(|(pl, ps, pr)| l.language == *pl && l.script == *ps && l.region == *pr)
                {
                    2
                } else {
                    3
                };
                order.push((o, i));
            }
            order.sort_by_key(|&(o, i)| (o, i));
            let mut out: Vec<(usize, Lsr)> = Vec::new();
            for (_, i) in order {
                let l = lsrs[i].clone().expect("lsr");
                if !out.iter().any(|(_, x)| *x == l) {
                    out.push((i, l));
                }
            }
            Rc::new(out)
        })
        .clone()
    })
}

/// LocaleMatcher::getBestMatch() over ICU's installed locales for the
/// desired locales (ICU IDs, by priority): the matched supported locale.
pub fn best_match(desired: &[Vec<u8>]) -> Option<&'static str> {
    let sup = supported();
    let mut best: Option<usize> = None;
    let mut best_shifted: i64 = (THRESHOLD << SHIFT) as i64;
    for d in desired {
        if let Some(dl) = lsr(d) {
            let mut found: Option<(usize, i64)> = None;
            for (i, s) in sup.iter() {
                let Some(dist) = distance(&dl, s) else {
                    continue;
                };
                let mut shifted = (dist << SHIFT) as i64;
                if dist == 0 {
                    shifted |= (dl.flags ^ s.flags) as i64;
                }
                if shifted < best_shifted && found.is_none_or(|(_, f)| shifted < f) {
                    found = Some((*i, shifted));
                    if shifted == 0 {
                        break;
                    }
                }
            }
            if let Some((i, sh)) = found {
                best = Some(i);
                best_shifted = sh;
            }
        }
        best_shifted -= (DEMOTION << SHIFT) as i64;
        if best_shifted <= 0 {
            break;
        }
    }
    best.map(|i| uloc::data::AVAILABLE[i])
}
