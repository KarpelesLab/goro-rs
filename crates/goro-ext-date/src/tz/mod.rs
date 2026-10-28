//! Time zones: the three kinds `DateTimeZone` distinguishes.
//!
//! PHP manual (DateTimeZone::__construct, DateTime::format "e"/"T"):
//! a zone is a UTC offset (type 1, "+05:00"), an abbreviation (type 2,
//! "EST", with its DST flag) or an identifier from the time zone database
//! (type 3, "Europe/Paris").

pub mod abbr;
mod abbr_table;
pub mod db;

use std::sync::Arc;

pub use db::{LocalType, Location, Transition, ZoneId};

/// A short ASCII string kept inline (abbreviations, offsets).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShortStr {
    len: u8,
    buf: [u8; 15],
}

impl ShortStr {
    /// Builds from `s`, truncated to 15 bytes.
    pub fn new(s: &str) -> ShortStr {
        let mut buf = [0u8; 15];
        let n = s.len().min(15);
        buf[..n].copy_from_slice(&s.as_bytes()[..n]);
        ShortStr { len: n as u8, buf }
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.buf[..self.len as usize]).unwrap_or("")
    }
}

impl std::fmt::Debug for ShortStr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl std::ops::Deref for ShortStr {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

/// A time zone as PHP's DateTimeZone holds it.
#[derive(Clone, Debug, PartialEq)]
pub enum TimeZone {
    /// Type 1: a fixed offset in seconds east of UTC.
    Offset(i32),
    /// Type 2: an abbreviation with its offset (DST included) and DST
    /// flag.
    Abbr {
        /// Uppercase abbreviation as PHP prints it.
        abbr: ShortStr,
        /// UTC offset in seconds, DST shift included.
        offset: i32,
        /// Whether the abbreviation is a DST one.
        dst: bool,
    },
    /// Type 3: a database identifier.
    Id(IdZone),
}

/// A type 3 zone: the database entry and the name as the script spelled
/// it (PHP keeps the spelling: `new DateTimeZone("europe/paris")` reports
/// "europe/paris", observed on PHP 8.5).
#[derive(Clone, Debug, PartialEq)]
pub struct IdZone {
    /// The database entry.
    pub zone: ZoneId,
    spelling: Option<Arc<str>>,
}

impl IdZone {
    /// A zone named exactly as the database spells it.
    pub fn new(zone: ZoneId) -> IdZone {
        IdZone {
            zone,
            spelling: None,
        }
    }

    /// The name PHP reports.
    pub fn name(&self) -> &str {
        match &self.spelling {
            Some(s) => s,
            None => self.zone.name(),
        }
    }
}

/// The zone's state at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneState {
    /// UTC offset in seconds.
    pub offset: i32,
    /// Daylight saving time flag.
    pub is_dst: bool,
    /// Abbreviation as `format("T")` prints it (type 1 zones print their
    /// offset there instead; see [`crate::format`]).
    pub abbr: ShortStr,
}

impl TimeZone {
    /// UTC.
    pub fn utc() -> TimeZone {
        TimeZone::Id(IdZone::new(
            ZoneId::find("UTC").expect("UTC in the database"),
        ))
    }

    /// A database zone by name (ASCII case-insensitive), keeping the
    /// given spelling.
    pub fn from_id(name: &str) -> Option<TimeZone> {
        let zone = ZoneId::find(name)?;
        let spelling = if zone.name() == name {
            None
        } else {
            Some(Arc::from(name))
        };
        Some(TimeZone::Id(IdZone { zone, spelling }))
    }

    /// A zone from an abbreviation entry.
    pub fn from_abbr(e: &abbr::AbbrEntry) -> TimeZone {
        TimeZone::Abbr {
            abbr: ShortStr::new(e.upper),
            offset: e.offset,
            dst: e.dst,
        }
    }

    /// Parses a `DateTimeZone` constructor / `date_default_timezone_set`
    /// argument. Returns `None` for "Unknown or bad timezone".
    ///
    /// Observed on PHP 8.5: leading blanks are skipped but trailing ones
    /// are not; "UTC" (exactly) is the identifier while "utc" is the
    /// abbreviation; "+05:30", "-5", "GMT+5" are offsets; abbreviations
    /// win over identifiers ("EST", "CET" are type 2); identifiers match
    /// case-insensitively.
    pub fn parse(name: &str) -> Option<TimeZone> {
        let s = name.trim_start_matches([' ', '\t']);
        if s == "UTC" {
            return TimeZone::from_id("UTC");
        }
        if s.starts_with(['+', '-']) {
            return parse_offset_zone(s).map(TimeZone::Offset);
        }
        if let Some(rest) = s.strip_prefix("GMT")
            && rest.starts_with(['+', '-'])
        {
            return parse_offset_zone(rest).map(TimeZone::Offset);
        }
        if let Some(e) = abbr::find(s) {
            return Some(TimeZone::from_abbr(e));
        }
        TimeZone::from_id(s)
    }

    /// PHP's zone type number: 1 offset, 2 abbreviation, 3 identifier.
    pub fn kind(&self) -> u8 {
        match self {
            TimeZone::Offset(_) => 1,
            TimeZone::Abbr { .. } => 2,
            TimeZone::Id(_) => 3,
        }
    }

    /// `DateTimeZone::getName()` / `format("e")`.
    pub fn name(&self) -> String {
        match self {
            TimeZone::Offset(o) => format_offset_name(*o),
            TimeZone::Abbr { abbr, .. } => abbr.as_str().to_string(),
            TimeZone::Id(z) => z.name().to_string(),
        }
    }

    /// The zone's state at Unix time `ts`.
    pub fn state_at(&self, ts: i64) -> ZoneState {
        match self {
            TimeZone::Offset(o) => ZoneState {
                offset: *o,
                is_dst: false,
                abbr: ShortStr::new(""),
            },
            TimeZone::Abbr { abbr, offset, dst } => ZoneState {
                offset: *offset,
                is_dst: *dst,
                abbr: *abbr,
            },
            TimeZone::Id(z) => {
                let t = z.zone.type_at(ts);
                ZoneState {
                    offset: t.offset,
                    is_dst: t.is_dst,
                    abbr: ShortStr::new(t.abbr),
                }
            }
        }
    }

    /// UTC offset at Unix time `ts`.
    pub fn offset_at(&self, ts: i64) -> i32 {
        match self {
            TimeZone::Offset(o) => *o,
            TimeZone::Abbr { offset, .. } => *offset,
            TimeZone::Id(z) => z.zone.type_at(ts).offset,
        }
    }

    /// The Unix time of a local wall-clock time (`local` = the civil time
    /// counted as if it were UTC).
    ///
    /// Observed on PHP 8.5: a time in a spring-forward gap moves forward
    /// by the gap (New York 02:30 → 03:30 EDT, Lord Howe 02:15 → 02:45);
    /// a time in a fall-back overlap takes the offset in effect at the
    /// instant `local` read as UTC, which picks EDT in New York but the
    /// later +10:30 on Lord Howe and GMT in London.
    pub fn local_to_ts(&self, local: i64) -> i64 {
        match self {
            TimeZone::Offset(o) => local.saturating_sub(*o as i64),
            TimeZone::Abbr { offset, .. } => local.saturating_sub(*offset as i64),
            TimeZone::Id(z) => {
                let off1 = z.zone.type_at(local).offset as i64;
                let t1 = local.saturating_sub(off1);
                let off2 = z.zone.type_at(t1).offset as i64;
                if off1 == off2 {
                    return t1;
                }
                let t2 = local.saturating_sub(off2);
                let off3 = z.zone.type_at(t2).offset as i64;
                if off3 == off2 {
                    return t2;
                }
                // in a gap: the earlier offset, which lands after it
                local.saturating_sub(off1.min(off2))
            }
        }
    }

    /// `DateTimeZone::getLocation()` data. Zones without a zone.tab entry
    /// (and type 3 "UTC") report country "??" (observed on PHP 8.5);
    /// offset and abbreviation zones have none (`None` here, `false` in
    /// PHP).
    pub fn location(&self) -> Option<Location> {
        match self {
            TimeZone::Id(z) => Some(z.zone.location().unwrap_or(Location {
                country_code: "??",
                latitude: 0.0,
                longitude: 0.0,
                comments: "?",
            })),
            _ => None,
        }
    }
}

/// "+05:00", "-05:30", "+05:30:15" for an offset in seconds (the name of
/// a type 1 zone; observed on PHP 8.5).
pub fn format_offset_name(o: i32) -> String {
    let sign = if o < 0 { '-' } else { '+' };
    let a = o.unsigned_abs();
    let (h, m, s) = (a / 3600, a / 60 % 60, a % 60);
    if s != 0 {
        format!("{sign}{h:02}:{m:02}:{s:02}")
    } else {
        format!("{sign}{h:02}:{m:02}")
    }
}

/// Parses "+H", "+HH", "+HMM", "+HHMM", "+HHMMSS", "+H:MM", "+HH:MM",
/// "+HH:MM:SS" (`s` starts with the sign) as a `DateTimeZone` offset.
/// Minutes and seconds are not range checked: "+01:99" is 02:39
/// (observed on PHP 8.5).
fn parse_offset_zone(s: &str) -> Option<i32> {
    let neg = s.starts_with('-');
    let body = &s[1..];
    if body.is_empty() || !body.bytes().all(|c| c.is_ascii_digit() || c == b':') {
        return None;
    }
    let num = |t: &str| -> i64 { t.bytes().fold(0i64, |a, c| a * 10 + (c - b'0') as i64) };
    let b = body.as_bytes();
    let colons = b.iter().filter(|&&c| c == b':').count();
    let (h, m, sec) = if colons == 0 {
        match b.len() {
            1 | 2 => (num(body), 0, 0),
            3 => (num(&body[..1]), num(&body[1..]), 0),
            4 => (num(&body[..2]), num(&body[2..]), 0),
            6 => (num(&body[..2]), num(&body[2..4]), num(&body[4..])),
            _ => return None,
        }
    } else {
        let parts: Vec<&str> = body.split(':').collect();
        match parts.as_slice() {
            // "+:30" is +00:00 on PHP 8.5
            ["", _] => (0, 0, 0),
            [h, m] if h.len() <= 2 && m.len() <= 2 => (num(h), num(m), 0),
            // "+100:00" is one minute on PHP 8.5 (an approximation of
            // that quirk: the leading digit counts as minutes)
            [h, m] if h.len() == 3 && m.len() == 2 => (0, num(&h[..1]), 0),
            [h, m, sec] if h.len() == 2 && m.len() == 2 && sec.len() == 2 => {
                (num(h), num(m), num(sec))
            }
            _ => return None,
        }
    };
    let total = h * 3600 + m * 60 + sec;
    if total > 99 * 3600 + 59 * 60 + 59 {
        return None;
    }
    let total = total as i32;
    Some(if neg { -total } else { total })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name_of(s: &str) -> Option<(u8, String)> {
        TimeZone::parse(s).map(|z| (z.kind(), z.name()))
    }

    #[test]
    fn constructor_forms() {
        // observed on PHP 8.5
        assert_eq!(name_of("EST"), Some((2, "EST".into())));
        assert_eq!(name_of("est"), Some((2, "EST".into())));
        assert_eq!(name_of("UTC"), Some((3, "UTC".into())));
        assert_eq!(name_of("utc"), Some((2, "UTC".into())));
        assert_eq!(name_of("z"), Some((2, "Z".into())));
        assert_eq!(name_of("+5"), Some((1, "+05:00".into())));
        assert_eq!(name_of("-0530"), Some((1, "-05:30".into())));
        assert_eq!(name_of("+05:30:15"), Some((1, "+05:30:15".into())));
        assert_eq!(name_of("+1:2"), Some((1, "+01:02".into())));
        assert_eq!(name_of("+123"), Some((1, "+01:23".into())));
        assert_eq!(name_of("+12345"), None);
        assert_eq!(name_of("+123456"), Some((1, "+12:34:56".into())));
        assert_eq!(name_of("+01:99"), Some((1, "+02:39".into())));
        assert_eq!(name_of("+999"), Some((1, "+10:39".into())));
        assert_eq!(name_of("-9999"), None);
        assert_eq!(name_of("+100:00"), Some((1, "+00:01".into())));
        assert_eq!(name_of("-0"), Some((1, "+00:00".into())));
        assert_eq!(name_of("+5:"), Some((1, "+05:00".into())));
        assert_eq!(name_of("+:30"), Some((1, "+00:00".into())));
        assert_eq!(name_of("+1:0:0"), None);
        assert_eq!(name_of("GMT+5"), Some((1, "+05:00".into())));
        assert_eq!(name_of("GMT-5"), Some((1, "-05:00".into())));
        assert_eq!(name_of("gmt+5"), None);
        assert_eq!(name_of("UTC+5"), None);
        assert_eq!(name_of("europe/paris"), Some((3, "europe/paris".into())));
        assert_eq!(name_of("  UTC"), Some((3, "UTC".into())));
        assert_eq!(name_of("UTC "), None);
        assert_eq!(name_of(""), None);
        assert_eq!(name_of("J"), None);
        assert_eq!(name_of("CET"), Some((2, "CET".into())));
        assert_eq!(name_of("Factory"), Some((3, "Factory".into())));
        assert_eq!(name_of("US/Eastern"), Some((3, "US/Eastern".into())));
    }

    #[test]
    fn local_resolution() {
        let ny = TimeZone::from_id("America/New_York").unwrap();
        // 2021-03-14 02:30 local is in the gap: 03:30 EDT
        let l = crate::civil::civil_to_ts(2021, 3, 14, 2, 30, 0);
        assert_eq!(ny.local_to_ts(l), 1_615_707_000);
        // 2021-11-07 01:30 local is ambiguous: EDT
        let l = crate::civil::civil_to_ts(2021, 11, 7, 1, 30, 0);
        assert_eq!(ny.local_to_ts(l), 1_636_263_000);
        let lh = TimeZone::from_id("Australia/Lord_Howe").unwrap();
        let l = crate::civil::civil_to_ts(2021, 4, 4, 1, 30, 0);
        assert_eq!(lh.local_to_ts(l), 1_617_462_000);
        let l = crate::civil::civil_to_ts(2021, 4, 4, 1, 29, 59);
        assert_eq!(lh.local_to_ts(l), 1_617_460_199);
        let l = crate::civil::civil_to_ts(2021, 10, 3, 2, 15, 0);
        assert_eq!(lh.local_to_ts(l), 1_633_189_500);
        let lon = TimeZone::from_id("Europe/London").unwrap();
        let l = crate::civil::civil_to_ts(2021, 10, 31, 1, 30, 0);
        assert_eq!(lon.local_to_ts(l), 1_635_643_800);
        let l = crate::civil::civil_to_ts(2021, 3, 28, 1, 30, 0);
        assert_eq!(lon.local_to_ts(l), 1_616_895_000);
    }
}
