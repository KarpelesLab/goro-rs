//! What IntlDateFormatter needs beyond the datecore contract: zone names at
//! an instant, zone short IDs and exemplar cities, DateTime decomposition
//! (common_date.cpp), locale canonicalization and validation.

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::Obj;
use crate::datecore as dc;

/// The display name of zone `tz_id` at instant `millis` (the metazone in
/// effect then: Europe/Minsk in 2012 is "Kaliningrad"), for
/// IntlTimeZone::DISPLAY_* `style`.
///
/// PLACEHOLDER over datecore::tz_display_name(): it must select the
/// metazone by the instant (ICU's TimeZoneFormat does, getDisplayName()
/// uses the current date).
pub fn tz_display_name_at(tz_id: &str, millis: f64, daylight: bool, style: i64, locale: &str) -> String {
    let _ = millis;
    dc::tz_display_name(tz_id, daylight, style, locale)
}

/// The BCP 47 short ID of a zone ("America/New_York" -> "usnyc"), "unk"
/// when there is none.
///
/// PLACEHOLDER: needs the CLDR bcp47 timezone table.
pub fn tz_short_id(tz_id: &str) -> String {
    match tz_id {
        "UTC" | "Etc/UTC" | "Etc/GMT" | "GMT" => "utc".to_string(),
        _ => "unk".to_string(),
    }
}

/// The exemplar city of a zone ("America/New_York" -> "New York").
///
/// PLACEHOLDER: should come from the localized CLDR exemplar cities.
pub fn tz_exemplar_city(tz_id: &str, _locale: &str) -> String {
    let id = dc::canonical_tz_id(tz_id).unwrap_or_else(|| tz_id.to_string());
    match id.rsplit_once('/') {
        Some((area, city)) if area != "Etc" => city.replace('_', " "),
        _ => "Unknown City".to_string(),
    }
}

fn call(vm: &mut Vm, o: &Obj, method: &str, args: &[Value]) -> Option<Value> {
    let mut a = goro_core::array::PhpArray::new();
    a.push(Value::Object(o.clone()));
    a.push(Value::String(PhpString::from_bytes(method.as_bytes())));
    let cb = crate::arr(a);
    match vm.call_callback(&cb, args) {
        Ok(v) if vm.current_exception.is_none() => Some(v.deref()),
        _ => None,
    }
}

/// intl_datetime_decompose(): the instant (ms) and the ICU time zone ID of
/// a DateTimeInterface. Err(None) when an exception is pending, Err(Some(
/// (code, msg))) for an intl error.
pub fn datetime_decompose(vm: &mut Vm, o: &Obj) -> Result<(f64, String), Option<(i32, String)>> {
    let ts = call(vm, o, "getTimestamp", &[]).ok_or(None)?;
    let Value::Long(ts) = ts else {
        let cn = goro_core::value::display_class_name(&o.borrow().class_name);
        return Err(Some((
            crate::error::U_INTERNAL_PROGRAM_ERROR,
            format!("{}::getTimestamp() did not return an int", cn),
        )));
    };
    let us = call(vm, o, "format", &[crate::s("u")])
        .map(|v| v.to_php_string().to_string_lossy())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    let millis = ts as f64 * 1000.0 + (us / 1000) as f64;
    let tz = call(vm, o, "getTimezone", &[]).ok_or(None)?;
    let name = match &tz {
        Value::Object(z) => {
            let z = z.clone();
            call(vm, &z, "getName", &[])
                .map(|v| v.to_php_string().to_string_lossy())
                .unwrap_or_default()
        }
        _ => String::new(),
    };
    match zone_from_datetimezone_name(&name) {
        Some(id) => Ok((millis, id)),
        None => Err(Some((
            crate::error::U_ILLEGAL_ARGUMENT_ERROR,
            "could not convert DateTime's time zone".to_string(),
        ))),
    }
}

/// timezone_convert_datetimezone(): the ICU zone ID of an ext/date zone name
/// (an ID, an offset "+05:00", an abbreviation); None when ICU does not know it.
pub fn zone_from_datetimezone_name(name: &str) -> Option<String> {
    let b = name.as_bytes();
    if !b.is_empty() && (b[0] == b'+' || b[0] == b'-') {
        let sign = if b[0] == b'-' { -1 } else { 1 };
        let rest = &name[1..];
        let (h, m) = rest.split_once(':').unwrap_or((rest, "0"));
        let h: i64 = h.parse().ok()?;
        let m: i64 = m.parse().ok()?;
        let mins = sign * (h * 60 + m);
        if mins <= -24 * 60 || mins >= 24 * 60 {
            return None;
        }
        let hours = mins / 60;
        let minutes = (mins - hours * 60).abs();
        return Some(format!("GMT{}{:02}:{:02}", if mins < 0 { '-' } else { '+' }, hours.abs(), minutes));
    }
    dc::canonical_tz_id(name).map(|_| name.to_string())
}

/// uloc_canonicalize() for the common cases: '-' -> '_', the POSIX
/// charset ("pt_PT.utf8") and old variants ("@euro") dropped, letter case.
pub fn uloc_canonicalize(loc: &str) -> String {
    let (base, kw) = match loc.split_once('@') {
        Some((b, k)) => (b, Some(k)),
        None => (loc, None),
    };
    let base = base.split('.').next().unwrap_or("");
    let mut parts: Vec<String> = Vec::new();
    for (i, p) in base.split(['-', '_']).enumerate() {
        let s = if i == 0 {
            p.to_ascii_lowercase()
        } else if p.len() == 4 && p.chars().all(|c| c.is_ascii_alphabetic()) {
            let mut s = p.to_ascii_lowercase();
            s[..1].make_ascii_uppercase();
            s
        } else {
            p.to_ascii_uppercase()
        };
        parts.push(s);
    }
    let mut out = parts.join("_");
    if let Some(k) = kw {
        let kws: Vec<String> = k
            .split(';')
            .filter(|e| e.contains('='))
            .map(|e| {
                let (a, b) = e.split_once('=').unwrap_or((e, ""));
                format!("{}={}", a.trim().to_ascii_lowercase(), b.trim())
            })
            .collect();
        if !kws.is_empty() {
            out.push('@');
            out.push_str(&kws.join(";"));
        }
    }
    out
}

/// ISO 639-1 codes ICU knows (uloc_getISO3Language() not empty).
const ISO639_1: &str = "aa ab ae af ak am an ar as av ay az ba be bg bh bi bm bn bo br bs ca ce ch co cr cs cu cv cy da de dv dz ee el en eo es et eu fa ff fi fj fo fr fy ga gd gl gn gu gv ha he hi ho hr ht hu hy hz ia id ie ig ii ik in io is it iu iw ja ji jv jw ka kg ki kj kk kl km kn ko kr ks ku kv kw ky la lb lg li ln lo lt lu lv mg mh mi mk ml mn mo mr ms mt my na nb nd ne ng nl nn no nr nv ny oc oj om or os pa pi pl ps pt qu rm rn ro ru rw sa sc sd se sg sh si sk sl sm sn so sq sr ss st su sv sw ta te tg th ti tk tl tn to tr ts tt tw ty ug uk ur uz ve vi vo wa wo xh yi yo za zh zu";

/// Locale::getISO3Language() is not empty: a known language.
pub fn has_iso3_language(loc: &str) -> bool {
    let lang: String = loc
        .split(['_', '-', '@', '.'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match lang.len() {
        2 => ISO639_1.split(' ').any(|c| c == lang),
        3 => lang.chars().all(|c| c.is_ascii_alphabetic()),
        _ => lang == "root",
    }
}

/// The current instant (ms), as Calendar::getNow().
pub fn now_millis() -> f64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_millis() as f64,
        Err(_) => 0.0,
    }
}

/// The week data of a locale (UCAL_FIRST_DAY_OF_WEEK, minimal days in the
/// first week), from its region.
///
/// PLACEHOLDER for the calendar module's CLDR week data.
pub fn locale_week_data(locale: &str) -> (i32, i32) {
    let base = locale.split('@').next().unwrap_or("");
    let region = base
        .split(['_', '-'])
        .skip(1)
        .find(|p| p.len() == 2 || p.len() == 3 && p.chars().all(|c| c.is_ascii_digit()))
        .unwrap_or("");
    let lang = base.split(['_', '-']).next().unwrap_or("");
    let sunday = [
        "US", "CA", "MX", "BR", "JP", "KR", "IL", "IN", "PH", "ZA", "AU", "HK", "TW", "SA", "PE",
        "CO", "VE", "GT", "HN", "NI", "PA", "DO", "PR", "SV", "BZ", "BS", "JM", "TH", "KH", "LA",
        "MM", "NP", "PK", "BD", "BT", "MO", "SG", "ID", "KE", "ET", "ZW", "MZ", "YE", "AS", "GU",
        "UM", "VI", "MH", "WS", "DM", "AG", "TT", "PY", "AR",
    ];
    let min4 = [
        "AD", "AN", "AT", "AX", "BE", "BG", "CH", "CZ", "DE", "DK", "EE", "ES", "FI", "FJ", "FO",
        "FR", "GB", "GF", "GG", "GI", "GP", "GR", "HU", "IE", "IM", "IS", "IT", "JE", "LI", "LT",
        "LU", "MC", "MQ", "NL", "NO", "PL", "RE", "RU", "SE", "SJ", "SK", "SM", "VA",
    ];
    let region = if region.is_empty() {
        match lang {
            "en" | "ja" | "ko" | "he" | "hi" | "zh" | "pt" | "th" | "id" => match lang {
                "en" => "US",
                "ja" => "JP",
                "ko" => "KR",
                "he" => "IL",
                "hi" => "IN",
                "zh" => "CN",
                "pt" => "BR",
                "th" => "TH",
                _ => "ID",
            },
            "de" => "DE",
            "fr" => "FR",
            "es" => "ES",
            "it" => "IT",
            "nl" => "NL",
            "ru" => "RU",
            "pl" => "PL",
            "sv" => "SE",
            "da" => "DK",
            "fi" => "FI",
            "nb" | "no" | "nn" => "NO",
            _ => "",
        }
    } else {
        region
    };
    let first = if sunday.contains(&region) { 1 } else { 2 };
    let min = if min4.contains(&region) { 4 } else { 1 };
    (first, min)
}
