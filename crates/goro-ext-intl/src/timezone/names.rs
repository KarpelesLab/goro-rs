//! Localized time zone names (UTS #35 Part 4 "Dates", section "Using Time
//! Zone Names"): specific and generic non-location names from CLDR's zone
//! and metazone names, the generic location format, exemplar cities, the
//! localized GMT format and the ISO 8601 offset formats.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::{TimeZone, canonical_ids, region};
use crate::datefmt::data::Loc;
use crate::datefmt::supplemental as sup;

/// A name style (the zone pattern fields of UTS #35, and ICU's
/// TimeZone::EDisplayType).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// `z`: short specific non-location (falls back to short GMT).
    ShortSpecific,
    /// `zzzz`: long specific non-location (falls back to long GMT).
    LongSpecific,
    /// `v`: short generic non-location.
    ShortGeneric,
    /// `vvvv`: long generic non-location.
    LongGeneric,
    /// `O`: short localized GMT.
    ShortGmt,
    /// `OOOO`, `ZZZZ`: long localized GMT.
    LongGmt,
    /// `VVV`: exemplar city.
    ExemplarCity,
    /// `VVVV`: generic location format.
    GenericLocation,
}

/// The metazone of a canonical zone at UTC time `ms`.
pub fn metazone(canon: &str, ms: f64) -> Option<&'static str> {
    let i = sup::METAZONE_PERIODS
        .binary_search_by(|(z, _)| z.as_bytes().cmp(canon.as_bytes()))
        .ok()?;
    let t = ms as i64;
    sup::METAZONE_PERIODS[i]
        .1
        .iter()
        .find(|(_, from, to)| *from <= t && t < *to)
        .map(|(m, _, _)| *m)
}

/// The golden zone of a metazone for a territory (else "001").
pub fn golden_zone(mz: &str, territory: &str) -> Option<&'static str> {
    let find = |t: &str| {
        sup::METAZONE_MAP
            .binary_search_by(|(m, tt, _)| {
                (m.as_bytes(), tt.as_bytes()).cmp(&(mz.as_bytes(), t.as_bytes()))
            })
            .ok()
            .map(|i| sup::METAZONE_MAP[i].2)
    };
    find(territory).or_else(|| find("001"))
}

/// The region of a locale id (its own, else the likely one).
pub fn locale_region(locale: &str) -> String {
    for part in locale.split('_').skip(1) {
        if (part.len() == 2 && part.bytes().all(|b| b.is_ascii_alphabetic()))
            || (part.len() == 3 && part.bytes().all(|b| b.is_ascii_digit()))
        {
            return part.to_ascii_uppercase();
        }
    }
    let lang = locale.split('_').next().unwrap_or("");
    intl::locale::Locale::parse(if lang.is_empty() { "und" } else { lang })
        .ok()
        .map(|l| l.maximize())
        .and_then(|l| l.region)
        .unwrap_or_else(|| "US".into())
}

fn zone_name(loc: Loc, canon: &str, kind: &str) -> Option<&'static str> {
    loc.get("tz", &format!("z:{canon}|{kind}"))
}

fn mz_name(loc: Loc, mz: &str, kind: &str) -> Option<&'static str> {
    loc.get("tz", &format!("m:{mz}|{kind}"))
}

/// The exemplar city of a canonical zone.
pub fn exemplar_city(loc: Loc, canon: &str) -> String {
    if let Some(c) = zone_name(loc, canon, "exemplarCity") {
        return c.to_string();
    }
    // UTS #35: the last field of the id with underscores as spaces
    canon.rsplit('/').next().unwrap_or(canon).replace('_', " ")
}

/// Number of canonical location zones per region.
fn zones_per_region() -> &'static HashMap<String, usize> {
    static M: OnceLock<HashMap<String, usize>> = OnceLock::new();
    M.get_or_init(|| {
        let mut m = HashMap::new();
        for id in canonical_ids() {
            if let Some(r) = region(&id)
                && r != "001"
            {
                *m.entry(r).or_insert(0) += 1;
            }
        }
        m
    })
}

/// The country name for the region format.
pub fn territory_name(loc: Loc, code: &str) -> Option<&'static str> {
    loc.get("territories", code)
}

/// The generic location format ("Portugal Time", "Los Angeles Time").
pub fn generic_location(loc: Loc, canon: &str) -> Option<String> {
    let r = region(canon)?;
    if r == "001" {
        return None;
    }
    let fmt = loc.get("tz", "regionFormat").unwrap_or("{0}");
    let single = zones_per_region().get(&r).copied().unwrap_or(0) <= 1;
    let primary = sup::PRIMARY_ZONES
        .iter()
        .any(|(c, z)| *c == r && *z == canon);
    if (single || primary)
        && let Some(name) = territory_name(loc, &r)
    {
        return Some(fmt.replace("{0}", name));
    }
    Some(fmt.replace("{0}", &exemplar_city(loc, canon)))
}

/// The localized GMT format of an offset (long: "GMT+05:00"; short:
/// "GMT+5", "GMT+5:30"; zero: the locale's "GMT").
pub fn localized_gmt(loc: Loc, offset_ms: i32, short: bool) -> String {
    let gmt = loc.get("tz", "gmtFormat").unwrap_or("GMT{0}");
    if offset_ms == 0 {
        return loc.get("tz", "gmtZeroFormat").unwrap_or("GMT").to_string();
    }
    let hour = loc.get("tz", "hourFormat").unwrap_or("+HH:mm;-HH:mm");
    let (pos, neg) = hour.split_once(';').unwrap_or((hour, hour));
    let pat = if offset_ms < 0 { neg } else { pos };
    let t = offset_ms.unsigned_abs() / 1000;
    let (h, m, s) = (t / 3600, t / 60 % 60, t % 60);
    // tokens: (letter or '\0' for literal text, run length, text)
    let mut toks: Vec<(char, usize, String)> = Vec::new();
    for c in pat.chars() {
        let letter = if c == 'H' || c == 'm' { c } else { '\0' };
        match toks.last_mut() {
            Some((l, n, txt)) if *l == letter => {
                *n += 1;
                txt.push(c);
            }
            _ => toks.push((letter, 1, c.to_string())),
        }
    }
    let drop_minutes = short && m == 0 && s == 0;
    let mut out = String::new();
    let mut i = 0;
    while i < toks.len() {
        let (l, n, txt) = &toks[i];
        match l {
            'H' => {
                if short || *n == 1 {
                    out.push_str(&h.to_string());
                } else {
                    out.push_str(&format!("{h:02}"));
                }
                if drop_minutes {
                    // skip the separator and the minutes
                    while i + 1 < toks.len() && toks[i + 1].0 != 'm' && toks[i + 1].0 != 'H' {
                        i += 1;
                    }
                    if i + 1 < toks.len() && toks[i + 1].0 == 'm' {
                        i += 1;
                    }
                }
            }
            'm' => {
                out.push_str(&format!("{m:02}"));
                if s != 0 {
                    out.push_str(&format!(":{s:02}"));
                }
            }
            _ => out.push_str(txt),
        }
        i += 1;
    }
    gmt.replace("{0}", &out)
}

/// ISO 8601 offsets (the `X`/`x` and `Z` fields): `basic` "+0530",
/// extended "+05:30"; `minutes` optional ("+05" when zero), `seconds`
/// optional; `z` gives "Z" for zero.
pub fn iso_offset(offset_ms: i32, extended: bool, minutes: Opt, seconds: bool, z: bool) -> String {
    if offset_ms == 0 && z {
        return "Z".into();
    }
    let sign = if offset_ms < 0 { '-' } else { '+' };
    let t = offset_ms.unsigned_abs() / 1000;
    let (h, m, s) = (t / 3600, t / 60 % 60, t % 60);
    let sep = if extended { ":" } else { "" };
    let mut out = format!("{sign}{h:02}");
    match minutes {
        Opt::Optional if m == 0 && s == 0 => return out,
        _ => out.push_str(&format!("{sep}{m:02}")),
    }
    if seconds && s != 0 {
        out.push_str(&format!("{sep}{s:02}"));
    }
    out
}

/// Whether a field is required or may be dropped when zero.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Opt {
    Required,
    Optional,
}

/// A zone's name in a style at UTC time `ms` (`daylight`: the DST
/// variant of specific names, None to follow the zone at `ms`).
pub fn display_name(
    loc: Loc,
    locale: &str,
    tz: &TimeZone,
    ms: f64,
    style: Style,
    daylight: Option<bool>,
) -> String {
    let (raw, dst) = tz.offsets(ms);
    let in_dst = daylight.unwrap_or(dst != 0);
    let offset = if daylight.is_some() {
        if in_dst {
            raw + if dst != 0 { dst } else { tz.dst_savings() }
        } else {
            raw
        }
    } else {
        raw + dst
    };
    if tz.is_custom() {
        return match style {
            Style::ShortSpecific | Style::ShortGeneric | Style::ShortGmt => {
                localized_gmt(loc, offset, true)
            }
            Style::ExemplarCity => exemplar_city(loc, super::UNKNOWN_ID),
            _ => localized_gmt(loc, offset, false),
        };
    }
    let canon = tz.canonical_id();
    let mz = metazone(&canon, ms);
    let kind = if in_dst { "daylight" } else { "standard" };
    match style {
        Style::ShortSpecific | Style::LongSpecific => {
            let w = if style == Style::ShortSpecific {
                "short"
            } else {
                "long"
            };
            let key = format!("{w}/{kind}");
            if let Some(n) = zone_name(loc, &canon, &key) {
                return n.to_string();
            }
            if let Some(m) = mz
                && let Some(n) = mz_name(loc, m, &key)
            {
                return n.to_string();
            }
            localized_gmt(loc, offset, style == Style::ShortSpecific)
        }
        Style::ShortGeneric | Style::LongGeneric => {
            let w = if style == Style::ShortGeneric {
                "short"
            } else {
                "long"
            };
            let key = format!("{w}/generic");
            if let Some(n) = zone_name(loc, &canon, &key) {
                return n.to_string();
            }
            if let Some(m) = mz
                && let Some(n) = mz_name(loc, m, &key)
            {
                // the metazone name stands for the zone when the zone keeps
                // the offset of the metazone's golden zone for the region
                let golden = golden_zone(m, &locale_region(locale));
                let same = match golden {
                    Some(g) if g != canon => {
                        TimeZone::create(g).total_offset(ms) == tz.total_offset(ms)
                    }
                    _ => true,
                };
                if same {
                    return n.to_string();
                }
            }
            if let Some(n) = generic_location(loc, &canon) {
                return n;
            }
            localized_gmt(loc, offset, style == Style::ShortGeneric)
        }
        Style::ShortGmt => localized_gmt(loc, offset, true),
        Style::LongGmt => localized_gmt(loc, offset, false),
        Style::ExemplarCity => exemplar_city(loc, &canon),
        Style::GenericLocation => {
            generic_location(loc, &canon).unwrap_or_else(|| localized_gmt(loc, offset, false))
        }
    }
}

/// Every name a zone may be written with in a locale (for parsing):
/// (name, canonical zone or metazone golden zone, is DST).
pub fn parse_candidates(loc: Loc) -> Vec<(&'static str, String, Option<bool>)> {
    let mut out = Vec::new();
    for (k, v) in loc.entries("tz", "m:") {
        let Some((mz, kind)) = k[2..].split_once('|') else {
            continue;
        };
        let Some(g) = golden_zone(mz, "001") else {
            continue;
        };
        let dst = if kind.ends_with("daylight") {
            Some(true)
        } else if kind.ends_with("standard") {
            Some(false)
        } else {
            None
        };
        out.push((v, g.to_string(), dst));
    }
    for (k, v) in loc.entries("tz", "z:") {
        let Some((z, kind)) = k[2..].split_once('|') else {
            continue;
        };
        if kind == "exemplarCity" {
            continue;
        }
        let dst = if kind.ends_with("daylight") {
            Some(true)
        } else if kind.ends_with("standard") {
            Some(false)
        } else {
            None
        };
        out.push((v, z.to_string(), dst));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lisbon_names() {
        let en = Loc::resolve("en_US");
        let tz = TimeZone::create("Europe/Lisbon");
        let jan = 1_325_376_000_000.0;
        assert_eq!(
            display_name(en, "en_US", &tz, jan, Style::LongSpecific, None),
            "Western European Standard Time"
        );
        assert_eq!(
            display_name(en, "en_US", &tz, jan, Style::ShortGeneric, None),
            "Portugal Time"
        );
        assert_eq!(
            display_name(en, "en_US", &tz, jan, Style::LongGeneric, None),
            "Western European Time"
        );
        assert_eq!(
            display_name(en, "en_US", &tz, jan, Style::ShortSpecific, None),
            "GMT"
        );
        assert_eq!(localized_gmt(en, 5 * 3_600_000, true), "GMT+5");
        assert_eq!(
            localized_gmt(en, 5 * 3_600_000 + 12 * 60_000, false),
            "GMT+05:12"
        );
        assert_eq!(localized_gmt(en, -10 * 3_600_000, true), "GMT-10");
        let pt = Loc::resolve("pt_PT");
        assert_eq!(
            display_name(pt, "pt_PT", &tz, jan, Style::LongGeneric, None),
            "Hora da Europa Ocidental"
        );
    }
}
