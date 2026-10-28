//! The procedural date functions that are not parsers: mktime, checkdate,
//! idate, getdate, localtime, strftime, and the DateTimeZone listings.
//!
//! PHP manual: the pages of each function. Corner cases observed on PHP
//! 8.5 (cited where they matter).

use crate::civil;
use crate::format::{DAY_NAMES, MONTH_NAMES};
use crate::time::LocalTime;
use crate::tz::{TimeZone, ZoneId};

/// `mktime()` / `gmmktime()` (with `zone` UTC): the fields carry; years
/// 0-69 mean 2000-2069 and 70-100 mean 1970-2000 (PHP manual: mktime).
pub fn mktime(h: i64, i: i64, s: i64, m: i64, d: i64, y: i64, zone: &TimeZone) -> i64 {
    let y = match y {
        0..=69 => y + 2000,
        70..=100 => y + 1900,
        _ => y,
    };
    let local = civil::civil_to_ts(y, m, d, h, i, s);
    zone.local_to_ts(local)
}

/// `checkdate()`: years 1-32767 (PHP manual: checkdate).
pub fn checkdate(m: i64, d: i64, y: i64) -> bool {
    (1..=32767).contains(&y)
        && (1..=12).contains(&m)
        && d >= 1
        && d <= civil::days_in_month(y, m as u32) as i64
}

/// Why `idate()` failed: the warning PHP prints before returning false.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdateError {
    /// "idate format is one char"
    NotOneChar,
    /// "Unrecognized date format token"
    Unrecognized,
}

impl IdateError {
    /// The warning text.
    pub fn message(&self) -> &'static str {
        match self {
            IdateError::NotOneChar => "idate format is one char",
            IdateError::Unrecognized => "Unrecognized date format token",
        }
    }
}

/// `idate()`.
pub fn idate(fmt: &[u8], ts: i64, zone: &TimeZone) -> Result<i64, IdateError> {
    if fmt.len() != 1 {
        return Err(IdateError::NotOneChar);
    }
    let t = LocalTime::new(ts, 0, zone);
    Ok(match fmt[0] {
        b'B' => {
            let secs = (ts.rem_euclid(86_400) + 3600) % 86_400;
            secs * 10 / 864
        }
        b'd' => t.d as i64,
        b'h' => {
            let h = t.h % 12;
            if h == 0 { 12 } else { h as i64 }
        }
        b'H' => t.h as i64,
        b'i' => t.i as i64,
        b'I' => t.is_dst as i64,
        b'L' => civil::is_leap(t.y) as i64,
        b'm' => t.m as i64,
        b'N' => match t.weekday() {
            0 => 7,
            w => w as i64,
        },
        b'o' => civil::iso_week_date(t.y, t.m, t.d).0,
        b's' => t.s as i64,
        b't' => civil::days_in_month(t.y, t.m) as i64,
        b'U' => ts,
        b'w' => t.weekday() as i64,
        b'W' => civil::iso_week_date(t.y, t.m, t.d).1 as i64,
        b'y' => t.y % 100,
        b'Y' => t.y,
        b'z' => civil::day_of_year(t.y, t.m, t.d) as i64,
        b'Z' => t.offset as i64,
        _ => return Err(IdateError::Unrecognized),
    })
}

/// `getdate()`'s array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetDate {
    /// seconds
    pub seconds: i64,
    /// minutes
    pub minutes: i64,
    /// hours
    pub hours: i64,
    /// mday
    pub mday: i64,
    /// wday
    pub wday: i64,
    /// mon
    pub mon: i64,
    /// year
    pub year: i64,
    /// yday
    pub yday: i64,
    /// weekday
    pub weekday: &'static str,
    /// month
    pub month: &'static str,
    /// 0 (the timestamp)
    pub ts: i64,
}

/// `getdate()`.
pub fn getdate(ts: i64, zone: &TimeZone) -> GetDate {
    let t = LocalTime::new(ts, 0, zone);
    GetDate {
        seconds: t.s as i64,
        minutes: t.i as i64,
        hours: t.h as i64,
        mday: t.d as i64,
        wday: t.weekday() as i64,
        mon: t.m as i64,
        year: t.y,
        yday: civil::day_of_year(t.y, t.m, t.d) as i64,
        weekday: DAY_NAMES[t.weekday() as usize],
        month: MONTH_NAMES[t.m as usize - 1],
        ts,
    }
}

/// `localtime()`: tm_sec, tm_min, tm_hour, tm_mday, tm_mon (0-11),
/// tm_year (years since 1900), tm_wday, tm_yday, tm_isdst, in that order
/// (the keys of the associative form).
pub fn localtime(ts: i64, zone: &TimeZone) -> [i64; 9] {
    let t = LocalTime::new(ts, 0, zone);
    [
        t.s as i64,
        t.i as i64,
        t.h as i64,
        t.d as i64,
        t.m as i64 - 1,
        t.y - 1900,
        t.weekday() as i64,
        civil::day_of_year(t.y, t.m, t.d) as i64,
        t.is_dst as i64,
    ]
}

/// The keys of `localtime($ts, true)`.
pub const LOCALTIME_KEYS: [&str; 9] = [
    "tm_sec", "tm_min", "tm_hour", "tm_mday", "tm_mon", "tm_year", "tm_wday", "tm_yday", "tm_isdst",
];

/// `strftime()` / `gmstrftime()` in the C locale (both deprecated since
/// PHP 8.1). Unknown conversions are copied ("%q"), as is a trailing '%'
/// (observed on PHP 8.5). `gmt` selects gmstrftime's "GMT" for %Z.
pub fn strftime(fmt: &[u8], ts: i64, zone: &TimeZone, gmt: bool) -> Vec<u8> {
    use std::io::Write;
    let t = LocalTime::new(ts, 0, zone);
    let mut out = Vec::with_capacity(fmt.len() * 2);
    let wd = t.weekday() as usize;
    let h12 = if t.h.is_multiple_of(12) { 12 } else { t.h % 12 };
    let (iso_y, iso_w, iso_d) = civil::iso_week_date(t.y, t.m, t.d);
    let yday = civil::day_of_year(t.y, t.m, t.d) as i64;
    let mut k = 0;
    while k < fmt.len() {
        let c = fmt[k];
        k += 1;
        if c != b'%' {
            out.push(c);
            continue;
        }
        let Some(&f) = fmt.get(k) else {
            out.push(b'%');
            break;
        };
        k += 1;
        let _ = match f {
            b'a' => write!(out, "{}", &DAY_NAMES[wd][..3]),
            b'A' => write!(out, "{}", DAY_NAMES[wd]),
            b'b' | b'h' => write!(out, "{}", &MONTH_NAMES[t.m as usize - 1][..3]),
            b'B' => write!(out, "{}", MONTH_NAMES[t.m as usize - 1]),
            b'c' => write!(
                out,
                "{} {} {:2} {:02}:{:02}:{:02} {}",
                &DAY_NAMES[wd][..3],
                &MONTH_NAMES[t.m as usize - 1][..3],
                t.d,
                t.h,
                t.i,
                t.s,
                t.y
            ),
            b'C' => write!(out, "{:02}", t.y.div_euclid(100)),
            b'd' => write!(out, "{:02}", t.d),
            b'D' | b'x' => write!(out, "{:02}/{:02}/{:02}", t.m, t.d, t.y.rem_euclid(100)),
            b'e' => write!(out, "{:2}", t.d),
            b'F' => write!(out, "{}-{:02}-{:02}", t.y, t.m, t.d),
            b'g' => write!(out, "{:02}", iso_y.rem_euclid(100)),
            b'G' => write!(out, "{iso_y}"),
            b'H' => write!(out, "{:02}", t.h),
            b'I' => write!(out, "{h12:02}"),
            b'j' => write!(out, "{:03}", yday + 1),
            b'k' => write!(out, "{:2}", t.h),
            b'l' => write!(out, "{h12:2}"),
            b'm' => write!(out, "{:02}", t.m),
            b'M' => write!(out, "{:02}", t.i),
            b'n' => writeln!(out),
            b'p' => write!(out, "{}", if t.h < 12 { "AM" } else { "PM" }),
            b'P' => write!(out, "{}", if t.h < 12 { "am" } else { "pm" }),
            b'r' => write!(
                out,
                "{h12:02}:{:02}:{:02} {}",
                t.i,
                t.s,
                if t.h < 12 { "AM" } else { "PM" }
            ),
            b'R' => write!(out, "{:02}:{:02}", t.h, t.i),
            // the C library reads the broken-down time back in the
            // process zone (UTC), taking an hour off for DST (observed on
            // PHP 8.5 on Linux)
            b's' => write!(out, "{}", t.local_secs() - if t.is_dst { 3600 } else { 0 }),
            b'S' => write!(out, "{:02}", t.s),
            b't' => write!(out, "\t"),
            b'T' | b'X' => write!(out, "{:02}:{:02}:{:02}", t.h, t.i, t.s),
            b'u' => write!(out, "{iso_d}"),
            b'U' => write!(out, "{:02}", (yday + 7 - wd as i64) / 7),
            b'V' => write!(out, "{iso_w:02}"),
            b'w' => write!(out, "{wd}"),
            b'W' => write!(out, "{:02}", (yday + 7 - ((wd as i64 + 6) % 7)) / 7),
            b'y' => write!(out, "{:02}", t.y.rem_euclid(100)),
            b'Y' => write!(out, "{}", t.y),
            b'z' => {
                let mut tmp = Vec::new();
                crate::format::push_offset(&mut tmp, t.offset, false);
                out.extend_from_slice(&tmp);
                Ok(())
            }
            b'Z' => {
                if gmt {
                    write!(out, "GMT")
                } else {
                    write!(out, "{}", t.abbr.as_str())
                }
            }
            b'%' => write!(out, "%"),
            other => write!(out, "%{}", other as char),
        };
    }
    out
}

/// `DateTimeZone` group constants.
pub mod groups {
    /// AFRICA
    pub const AFRICA: i64 = 1;
    /// AMERICA
    pub const AMERICA: i64 = 2;
    /// ANTARCTICA
    pub const ANTARCTICA: i64 = 4;
    /// ARCTIC
    pub const ARCTIC: i64 = 8;
    /// ASIA
    pub const ASIA: i64 = 16;
    /// ATLANTIC
    pub const ATLANTIC: i64 = 32;
    /// AUSTRALIA
    pub const AUSTRALIA: i64 = 64;
    /// EUROPE
    pub const EUROPE: i64 = 128;
    /// INDIAN
    pub const INDIAN: i64 = 256;
    /// PACIFIC
    pub const PACIFIC: i64 = 512;
    /// UTC
    pub const UTC: i64 = 1024;
    /// ALL
    pub const ALL: i64 = 2047;
    /// ALL_WITH_BC
    pub const ALL_WITH_BC: i64 = 4095;
    /// PER_COUNTRY
    pub const PER_COUNTRY: i64 = 4096;
}

const PREFIXES: [(&str, i64); 10] = [
    ("Africa/", groups::AFRICA),
    ("America/", groups::AMERICA),
    ("Antarctica/", groups::ANTARCTICA),
    ("Arctic/", groups::ARCTIC),
    ("Asia/", groups::ASIA),
    ("Atlantic/", groups::ATLANTIC),
    ("Australia/", groups::AUSTRALIA),
    ("Europe/", groups::EUROPE),
    ("Indian/", groups::INDIAN),
    ("Pacific/", groups::PACIFIC),
];

/// `DateTimeZone::listIdentifiers()`. `ALL` and the groups list zone.tab's
/// names (plus "UTC"); ALL_WITH_BC lists every name; PER_COUNTRY (exactly)
/// lists a country's zones, matching the code case-sensitively (observed
/// on PHP 8.5: "us" gives nothing). The caller checks that a PER_COUNTRY
/// code has two letters.
pub fn list_identifiers(group: i64, country: Option<&str>) -> Vec<&'static str> {
    let mut out = Vec::new();
    if group == groups::PER_COUNTRY {
        let cc = country.unwrap_or("");
        for z in ZoneId::all() {
            if let Some(l) = z.location()
                && z.in_zone_tab()
                && l.country_code == cc
            {
                out.push(z.name());
            }
        }
        return out;
    }
    if group == groups::ALL_WITH_BC {
        return ZoneId::all().map(|z| z.name()).collect();
    }
    for z in ZoneId::all() {
        let name = z.name();
        if name == "UTC" {
            if group & groups::UTC != 0 {
                out.push(name);
            }
            continue;
        }
        if !z.in_zone_tab() {
            continue;
        }
        if PREFIXES
            .iter()
            .any(|(p, g)| group & g != 0 && name.starts_with(p))
        {
            out.push(name);
        }
    }
    out
}

/// One entry of `DateTimeZone::getTransitions()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionInfo {
    /// ts
    pub ts: i64,
    /// time ("Y-m-d\TH:i:sO" in UTC, as `format('c')` with +00:00)
    pub time: String,
    /// offset
    pub offset: i32,
    /// isdst
    pub isdst: bool,
    /// abbr
    pub abbr: String,
}

/// `DateTimeZone::getTransitions($begin, $end)`: the state at `begin`
/// first, then each change with `begin < ts < end` (observed on PHP 8.5;
/// the default end is 2^31 - 1 there). `None` (PHP's false) for offset and
/// abbreviation zones.
pub fn transitions(zone: &TimeZone, begin: i64, end: i64) -> Option<Vec<TransitionInfo>> {
    let TimeZone::Id(z) = zone else {
        return None;
    };
    let utc = TimeZone::Offset(0);
    let time = |ts: i64| {
        String::from_utf8(crate::time::date(b"Y-m-d\\TH:i:sP", ts, &utc)).unwrap_or_default()
    };
    let first = if begin == i64::MIN {
        z.zone.initial_type()
    } else {
        z.zone.type_at(begin)
    };
    let mut out = vec![TransitionInfo {
        ts: begin,
        time: time(begin),
        offset: first.offset,
        isdst: first.is_dst,
        abbr: first.abbr.to_string(),
    }];
    for t in z.zone.transitions(begin, end) {
        out.push(TransitionInfo {
            ts: t.ts,
            time: time(t.ts),
            offset: t.ty.offset,
            isdst: t.ty.is_dst,
            abbr: t.ty.abbr.to_string(),
        });
    }
    Some(out)
}

/// The default `$timestampEnd` of `getTransitions()` (observed on PHP 8.5:
/// the list stops before 2038-01-19).
pub const TRANSITIONS_DEFAULT_END: i64 = i32::MAX as i64;
