//! TimeZone::getDisplayName() over the intl crate's CLDR zone names.

use intl::datetime::{DateTime, DateTimeFormatOptions, DateTimePartType, TimeZoneNameStyle};

use super::data;
use super::zone::{self, Zone};

pub const SHORT: i64 = 1;
pub const LONG: i64 = 2;
pub const SHORT_GENERIC: i64 = 3;
pub const LONG_GENERIC: i64 = 4;
pub const SHORT_GMT: i64 = 5;
pub const LONG_GMT: i64 = 6;
pub const SHORT_COMMONLY_USED: i64 = 7;
pub const GENERIC_LOCATION: i64 = 8;

/// The language tag intl's tables take for an ICU locale ID.
fn lang_tag(locale: &str) -> String {
    let b = super::locale_base(locale).replace('_', "-");
    if b.is_empty() { "en".to_string() } else { b }
}

/// The localized GMT format of `offset` ms (formatOffsetLocalizedGMT()).
pub fn long_gmt(lang: &str, offset: i32) -> String {
    let mins = offset / 60_000;
    let mut s = intl::datetime::format_gmt_offset(lang, mins);
    let secs = (offset / 1000).abs() % 60;
    if secs != 0 && offset % 60_000 != 0 {
        s.push_str(&format!(":{:02}", secs));
    }
    s
}

/// The short localized GMT format (formatOffsetShortLocalizedGMT()):
/// "GMT+1", "GMT+5:30".
pub fn short_gmt(lang: &str, offset: i32) -> String {
    let long = long_gmt(lang, offset);
    if offset == 0 {
        return long;
    }
    // shorten the "hh:mm" of the long form
    let chars: Vec<char> = long.chars().collect();
    let mut i = 0;
    while i < chars.len() && !chars[i].is_ascii_digit() {
        i += 1;
    }
    let mut j = i;
    while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == ':') {
        j += 1;
    }
    if i >= j {
        return long;
    }
    let body: String = chars[i..j].iter().collect();
    let mut parts: Vec<String> = body.split(':').map(|p| p.to_string()).collect();
    if let Some(h) = parts.first_mut() {
        if h.len() == 2 && h.starts_with('0') {
            *h = h[1..].to_string();
        }
    }
    while parts.len() > 1 && parts.last().is_some_and(|p| p == "00") {
        parts.pop();
    }
    let pre: String = chars[..i].iter().collect();
    let post: String = chars[j..].iter().collect();
    format!("{}{}{}", pre, parts.join(":"), post)
}

/// formatOffsetISO8601Basic(offset, false, false, false): "+0100".
pub fn iso_basic(offset: i32) -> String {
    let a = (offset / 1000).abs();
    let mut s = format!(
        "{}{:02}{:02}",
        if offset < 0 { '-' } else { '+' },
        a / 3600,
        a / 60 % 60
    );
    if a % 60 != 0 {
        s.push_str(&format!("{:02}", a % 60));
    }
    s
}

thread_local! {
    static REF_TIME: std::cell::Cell<Option<f64>> = const { std::cell::Cell::new(None) };
}

/// The instant names are chosen for: now, or the one display_name_at()
/// was given.
fn ref_now() -> f64 {
    REF_TIME.with(|r| r.get()).unwrap_or_else(zone::now_millis)
}

/// The display name of `z` as of the instant `at` (UDate) rather than now
/// (the metazone and DST state of that date, as a formatter needs).
pub fn display_name_at(z: &Zone, daylight: bool, style: i64, locale: &str, at: f64) -> String {
    let old = REF_TIME.with(|r| r.replace(Some(at)));
    let n = display_name(z, daylight, style, locale);
    REF_TIME.with(|r| r.set(old));
    n
}

fn now_local(z: &Zone) -> DateTime {
    let now = ref_now();
    let (r, d) = z.offsets_utc(now);
    let local = now + (r + d) as f64;
    let days = (local / 86_400_000.0).floor() as i64;
    let ms = (local - days as f64 * 86_400_000.0) as i64;
    let (y, m, dd) = super::civil_from_days(days);
    DateTime {
        year: y as i32,
        month: m as u8,
        day: dd as u8,
        hour: (ms / 3_600_000) as u8,
        minute: (ms / 60_000 % 60) as u8,
        second: (ms / 1000 % 60) as u8,
        millisecond: 0,
    }
}

/// The CLDR zone name in `style` from intl, None when intl fell back to
/// the GMT format.
fn cldr_name(
    z: &Zone,
    lang: &str,
    style: TimeZoneNameStyle,
    daylight: Option<bool>,
) -> Option<String> {
    let idx = zone::system_index(&z.id)?;
    let canon: &'static str = zone::canonical_of(idx);
    let mut o = DateTimeFormatOptions::default();
    o.time_zone = Some(canon);
    o.time_zone_name = Some(style);
    o.tz_is_dst = daylight;
    let dt = now_local(z);
    let parts = intl::datetime::format_to_parts(lang, &dt, &o).ok()?;
    let name: String = parts
        .iter()
        .filter(|p| p.kind == DateTimePartType::TimeZoneName)
        .map(|p| p.value.as_str())
        .collect();
    if name.is_empty() {
        return None;
    }
    // intl's own fallback: the GMT format of the zone's offset
    let now = ref_now();
    let (r, d) = z.offsets_utc(now);
    for off in [r + d, r, r + z.dst_savings()] {
        if name == long_gmt(lang, off) || name == short_gmt(lang, off) {
            return None;
        }
    }
    Some(name)
}

/// The region format ("{0} Time") of a locale: derived from intl's
/// generic location name of Europe/Lisbon (named after its country), or
/// a fallback table.
fn region_format(lang: &str) -> String {
    if let (Some(n), Some(country)) = (
        zone::create("Europe/Lisbon")
            .and_then(|lz| cldr_name(&lz, lang, TimeZoneNameStyle::ShortGeneric, None)),
        intl::display::region_name(lang, "PT"),
    ) {
        if n.contains(country) {
            return n.replacen(country, "{0}", 1);
        }
    }
    match lang.split('-').next().unwrap_or("") {
        "en" => "{0} Time",
        "de" => "{0} (Ortszeit)",
        "fr" => "heure : {0}",
        "es" => "hora de {0}",
        "it" => "Ora {0}",
        "pt" => "Horário {0}",
        "nl" => "tijd in {0}",
        "ja" => "{0}時間",
        "zh" => "{0}时间",
        _ => "{0}",
    }
    .to_string()
}

/// Whether a name is an abbreviation ("PT", "CET").
fn is_abbreviation(n: &str) -> bool {
    n.len() <= 5
        && n.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

const PRIMARY_ZONES: &[(&str, &str)] = &[
    ("CL", "America/Santiago"),
    ("CN", "Asia/Shanghai"),
    ("DE", "Europe/Berlin"),
    ("EC", "America/Guayaquil"),
    ("ES", "Europe/Madrid"),
    ("MH", "Pacific/Majuro"),
    ("MY", "Asia/Kuala_Lumpur"),
    ("NZ", "Pacific/Auckland"),
    ("PT", "Europe/Lisbon"),
    ("UA", "Europe/Kiev"),
    ("UZ", "Asia/Tashkent"),
];

/// The generic location format ("Portugal Time", "Los Angeles Time").
pub fn generic_location(z: &Zone, lang: &str) -> Option<String> {
    let idx = zone::system_index(&z.id)?;
    let canon = zone::canonical_of(idx);
    let ci = zone::system_index(canon)?;
    let region = data::ZONES[ci].2;
    if canon.starts_with("Etc/") || region.is_empty() || region == "001" {
        return None;
    }
    let single = data::ZONES
        .iter()
        .filter(|r| r.2 == region && r.4 & 2 != 0)
        .count()
        == 1
        || PRIMARY_ZONES
            .iter()
            .any(|(r, zid)| *r == region && *zid == canon);
    // intl's short generic name falls back to the generic location
    if let Some(n) = cldr_name(z, lang, TimeZoneNameStyle::ShortGeneric, None) {
        if !is_abbreviation(&n)
            && cldr_name(z, lang, TimeZoneNameStyle::LongGeneric, None).as_deref()
                != Some(n.as_str())
        {
            return Some(n);
        }
    }
    let fmt = region_format(lang);
    if single {
        if let Some(country) = intl::display::region_name(lang, region) {
            return Some(fmt.replace("{0}", country));
        }
    }
    let city = canon.rsplit('/').next().unwrap_or(canon).replace('_', " ");
    Some(fmt.replace("{0}", &city))
}

/// TimeZone::getDisplayName(daylight, style, locale)
pub fn display_name(z: &Zone, daylight: bool, style: i64, locale: &str) -> String {
    let lang = lang_tag(locale);
    let raw = z.offsets_utc(ref_now()).0;
    match style {
        GENERIC_LOCATION | LONG_GENERIC | SHORT_GENERIC => {
            let name = match style {
                GENERIC_LOCATION => generic_location(z, &lang),
                _ => {
                    let long = style == LONG_GENERIC;
                    // no DST around now: the standard name
                    // (zones without a metazone, Etc/*, have no such name)
                    let etc = zone::system_index(&z.id)
                        .is_none_or(|i| zone::canonical_of(i).starts_with("Etc/"));
                    let std = if etc || z.dst_near(ref_now()) {
                        None
                    } else {
                        let st = if long {
                            TimeZoneNameStyle::Long
                        } else {
                            TimeZoneNameStyle::Short
                        };
                        cldr_name(z, &lang, st, Some(false))
                    };
                    let gs = if long {
                        TimeZoneNameStyle::LongGeneric
                    } else {
                        TimeZoneNameStyle::ShortGeneric
                    };
                    std.or_else(|| cldr_name(z, &lang, gs, None))
                }
            };
            match name {
                Some(n) => n,
                None => {
                    let off = if daylight && !z.is_unknown() {
                        raw + z.dst_savings()
                    } else {
                        raw
                    };
                    if style == SHORT_GENERIC {
                        short_gmt(&lang, off)
                    } else {
                        long_gmt(&lang, off)
                    }
                }
            }
        }
        LONG_GMT | SHORT_GMT => {
            let off = if daylight && z.use_daylight_time() {
                raw + z.dst_savings()
            } else {
                raw
            };
            if style == LONG_GMT {
                long_gmt(&lang, off)
            } else {
                iso_basic(off)
            }
        }
        _ => {
            let st = if style == LONG {
                TimeZoneNameStyle::Long
            } else {
                TimeZoneNameStyle::Short
            };
            match cldr_name(z, &lang, st, Some(daylight)) {
                Some(n) => n,
                None => {
                    let off = if daylight && z.use_daylight_time() {
                        raw + z.dst_savings()
                    } else {
                        raw
                    };
                    if style == LONG {
                        long_gmt(&lang, off)
                    } else {
                        short_gmt(&lang, off)
                    }
                }
            }
        }
    }
}
