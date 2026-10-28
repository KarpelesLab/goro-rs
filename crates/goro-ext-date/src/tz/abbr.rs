//! Time zone abbreviations ("EST", "CEST", military letters).
//!
//! The table is PHP's own list as `DateTimeZone::listAbbreviations()`
//! prints it (observed on PHP 8.5), generated into `abbr_table.rs` by
//! `tools/gen_tz_abbr.py`.

use super::abbr_table::ENTRIES;

/// One row of the abbreviation list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbbrEntry {
    /// Lowercase abbreviation, as the list's keys.
    pub abbr: &'static str,
    /// The uppercase form PHP prints for a zone parsed from it.
    pub upper: &'static str,
    /// Whether the abbreviation denotes daylight saving time.
    pub dst: bool,
    /// The UTC offset in seconds (including the DST shift).
    pub offset: i32,
    /// An identifier using it, if any (the military letters have none).
    pub tz: Option<&'static str>,
}

/// The whole list, in PHP's order.
pub fn entries() -> &'static [AbbrEntry] {
    ENTRIES
}

/// The entries of one abbreviation (ASCII case-insensitive), in order.
pub fn find_all(abbr: &str) -> &'static [AbbrEntry] {
    // Entries of one abbreviation are contiguous.
    let start = ENTRIES
        .iter()
        .position(|e| e.abbr.eq_ignore_ascii_case(abbr));
    match start {
        Some(s) => {
            let n = ENTRIES[s..]
                .iter()
                .take_while(|e| e.abbr.eq_ignore_ascii_case(abbr))
                .count();
            &ENTRIES[s..s + n]
        }
        None => &[],
    }
}

/// The first entry of an abbreviation: what a date string or a
/// `DateTimeZone` constructor means by it.
pub fn find(abbr: &str) -> Option<&'static AbbrEntry> {
    if abbr.is_empty() || abbr.len() > 6 {
        return None;
    }
    find_all(abbr).first()
}

/// Zone for an offset/DST pair when the abbreviation is unknown (observed
/// on PHP 8.5 by probing `timezone_name_from_abbr('', offset, isdst)` for
/// every quarter hour).
static FALLBACK: &[(i32, bool, &str)] = &[
    (-39600, false, "Pacific/Apia"),
    (-36000, false, "Pacific/Honolulu"),
    (-32400, false, "America/Anchorage"),
    (-28800, false, "America/Los_Angeles"),
    (-28800, true, "America/Anchorage"),
    (-25200, false, "America/Denver"),
    (-25200, true, "America/Los_Angeles"),
    (-21600, false, "America/Chicago"),
    (-21600, true, "America/Denver"),
    (-18000, false, "America/New_York"),
    (-18000, true, "America/Chicago"),
    (-16200, false, "America/Caracas"),
    (-14400, false, "America/Halifax"),
    (-14400, true, "America/New_York"),
    (-10800, false, "America/Sao_Paulo"),
    (-10800, true, "America/Halifax"),
    (-7200, true, "America/Sao_Paulo"),
    (-3600, false, "Atlantic/Azores"),
    (0, false, "Europe/London"),
    (0, true, "Atlantic/Azores"),
    (3600, false, "Europe/Paris"),
    (3600, true, "Europe/London"),
    (7200, false, "Europe/Helsinki"),
    (7200, true, "Europe/Paris"),
    (10800, false, "Europe/Moscow"),
    (10800, true, "Europe/Helsinki"),
    (14400, false, "Asia/Dubai"),
    (14400, true, "Europe/Moscow"),
    (18000, false, "Asia/Karachi"),
    (19800, false, "Asia/Kolkata"),
    (20700, false, "Asia/Katmandu"),
    (21600, true, "Asia/Yekaterinburg"),
    (25200, false, "Asia/Krasnoyarsk"),
    (25200, true, "Asia/Novosibirsk"),
    (28800, false, "Asia/Shanghai"),
    (28800, true, "Asia/Krasnoyarsk"),
    (32400, false, "Asia/Tokyo"),
    (36000, false, "Australia/Melbourne"),
    (37800, true, "Australia/Adelaide"),
    (39600, true, "Australia/Melbourne"),
    (43200, false, "Pacific/Auckland"),
    (46800, true, "Pacific/Auckland"),
];

/// `timezone_name_from_abbr()`. `offset` is -1 for "any"; `isdst` is -1
/// for "any", else 0 or 1.
///
/// Behaviour observed on PHP 8.5: "utc" and "gmt" always give "UTC"; a
/// known abbreviation gives its first identifier with the requested
/// offset, or its first identifier at all; an unknown one falls back to a
/// fixed offset/DST table, which needs `isdst` to be 0 or 1.
pub fn name_from_abbr(abbr: &str, offset: i64, isdst: i64) -> Option<&'static str> {
    if abbr.eq_ignore_ascii_case("utc") || abbr.eq_ignore_ascii_case("gmt") {
        return Some("UTC");
    }
    let list = find_all(abbr);
    if let Some(first) = list.first() {
        if offset != -1
            && let Some(e) = list.iter().find(|e| e.offset as i64 == offset)
        {
            return e.tz;
        }
        return first.tz;
    }
    if isdst == -1 {
        return None;
    }
    FALLBACK
        .iter()
        .find(|(o, d, _)| *o as i64 == offset && *d == (isdst == 1))
        .map(|(_, _, n)| *n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookups() {
        let e = find("EsT").unwrap();
        assert_eq!(
            (e.upper, e.offset, e.dst, e.tz),
            ("EST", -18000, false, Some("America/New_York"))
        );
        assert!(find("J").is_none());
        assert_eq!(find("a").unwrap().offset, 3600);
        assert_eq!(find("CHST").unwrap().upper, "CHST");
    }

    #[test]
    fn name_from_abbr_cases() {
        // observed on PHP 8.5
        assert_eq!(name_from_abbr("EST", -1, -1), Some("America/New_York"));
        assert_eq!(name_from_abbr("", 3600, 0), Some("Europe/Paris"));
        assert_eq!(name_from_abbr("", 3600, 1), Some("Europe/London"));
        assert_eq!(name_from_abbr("foo", 3600, 0), Some("Europe/Paris"));
        assert_eq!(name_from_abbr("EST", 3600, 0), Some("America/New_York"));
        assert_eq!(name_from_abbr("CEST", -1, -1), Some("Europe/Berlin"));
        assert_eq!(name_from_abbr("", 0, -1), None);
        assert_eq!(name_from_abbr("IST", 19800, 0), Some("Asia/Calcutta"));
        assert_eq!(name_from_abbr("IST", 2079, 1), Some("Europe/Dublin"));
        assert_eq!(name_from_abbr("cet", 7200, 1), Some("Europe/Kaliningrad"));
        assert_eq!(name_from_abbr("GMT", -1, -1), Some("UTC"));
        assert_eq!(name_from_abbr("Z", -1, -1), None);
        assert_eq!(name_from_abbr("", -36000, 1), None);
        assert_eq!(name_from_abbr("", 20700, 0), Some("Asia/Katmandu"));
    }
}
