//! A Rust port of Derick Rethans' timelib (as bundled with PHP in ext/date/lib).
//!
//! The structure follows the C sources closely so that behaviour (including
//! quirks) matches PHP exactly.

#![allow(clippy::too_many_arguments)]
#![allow(clippy::collapsible_else_if)]
#![allow(clippy::collapsible_if)]

pub mod astro;
pub mod dow;
pub mod interval;
pub mod iso_interval;
pub mod parse_date;
pub mod regex;
pub mod tm2unixtime;
pub mod tz;
pub mod unixtime2tm;

use std::rc::Rc;

pub use tz::TzInfo;

pub const UNSET: i64 = -9999999;

pub const ZONETYPE_NONE: u32 = 0;
pub const ZONETYPE_OFFSET: u32 = 1;
pub const ZONETYPE_ABBR: u32 = 2;
pub const ZONETYPE_ID: u32 = 3;

pub const OVERRIDE_TIME: u32 = 0x01;
pub const NO_CLONE: u32 = 0x02;

pub const SPECIAL_WEEKDAY: u32 = 0x01;
pub const SPECIAL_DAY_OF_WEEK_IN_MONTH: u32 = 0x02;
pub const SPECIAL_LAST_DAY_OF_WEEK_IN_MONTH: u32 = 0x03;

pub const SPECIAL_FIRST_DAY_OF_MONTH: i32 = 0x01;
pub const SPECIAL_LAST_DAY_OF_MONTH: i32 = 0x02;

pub const SECS_PER_DAY: i64 = 86400;
pub const SECS_PER_HOUR: i64 = 3600;
pub const DAYS_PER_ERA: i64 = 146097;
pub const YEARS_PER_ERA: i64 = 400;
pub const DAYS_PER_YEAR: i64 = 365;
pub const HINNANT_EPOCH_SHIFT: i64 = 719468;

#[inline]
pub fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RelTime {
    pub y: i64,
    pub m: i64,
    pub d: i64,
    pub h: i64,
    pub i: i64,
    pub s: i64,
    pub us: i64,
    pub weekday: i32,
    pub weekday_behavior: i32,
    pub first_last_day_of: i32,
    pub invert: i32,
    pub days: i64,
    pub special_type: u32,
    pub special_amount: i64,
    pub have_weekday_relative: u32,
    pub have_special_relative: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Time {
    pub y: i64,
    pub m: i64,
    pub d: i64,
    pub h: i64,
    pub i: i64,
    pub s: i64,
    pub us: i64,
    pub z: i64,
    pub tz_abbr: Option<String>,
    pub tz_info: Option<Rc<TzInfo>>,
    pub dst: i32,
    pub relative: RelTime,
    pub sse: i64,
    pub have_time: u32,
    pub have_date: u32,
    pub have_zone: u32,
    pub have_relative: u32,
    pub have_weeknr_day: u32,
    pub sse_uptodate: u32,
    pub tim_uptodate: u32,
    pub is_localtime: u32,
    pub zone_type: u32,
}

impl Time {
    pub fn new() -> Self {
        Self::default()
    }

    /// timelib_time_tz_abbr_update
    pub fn tz_abbr_update(&mut self, abbr: &str) {
        self.tz_abbr = Some(abbr.to_ascii_uppercase());
    }
}

#[derive(Debug, Clone)]
pub struct ErrorMessage {
    pub error_code: i32,
    pub position: i32,
    pub character: u8,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct ErrorContainer {
    pub error_messages: Vec<ErrorMessage>,
    pub warning_messages: Vec<ErrorMessage>,
}

impl ErrorContainer {
    pub fn error_count(&self) -> usize {
        self.error_messages.len()
    }
    pub fn warning_count(&self) -> usize {
        self.warning_messages.len()
    }
}

#[derive(Debug, Clone, Default)]
pub struct TimeOffset {
    pub offset: i32,
    pub leap_secs: u32,
    pub is_dst: u32,
    pub abbr: String,
    pub transition_time: i64,
}

pub fn time_compare(t1: &Time, t2: &Time) -> i32 {
    if t1.sse == t2.sse {
        if t1.us == t2.us {
            return 0;
        }
        return if t1.us < t2.us { -1 } else { 1 };
    }
    if t1.sse < t2.sse { -1 } else { 1 }
}

pub fn hms_to_seconds(h: i64, m: i64, s: i64) -> i64 {
    h.wrapping_mul(SECS_PER_HOUR).wrapping_add(m.wrapping_mul(60)).wrapping_add(s)
}

pub fn hmsf_to_decimal_hour(hour: i64, min: i64, sec: i64, us: i64) -> f64 {
    if hour >= 0 {
        (hour as f64 + min as f64 / 60.0 + sec as f64 / 3600.0) + us as f64 / 3_600_000_000.0
    } else {
        (hour as f64 - min as f64 / 60.0 - sec as f64 / 3600.0) - us as f64 / 3_600_000_000.0
    }
}

pub fn decimal_hour_to_hms(h: f64) -> (i64, i64, i64) {
    let mut h = h;
    let swap = h < 0.0;
    if swap {
        h = h.abs();
    }
    let mut hour = h.floor() as i64;
    let seconds = ((h - hour as f64) * 3600.0).floor() as i64;
    let min = seconds / 60;
    let sec = seconds % 60;
    if swap {
        hour = -hour;
    }
    (hour, min, sec)
}

/// timelib_strcasecmp equivalent for ASCII.
pub fn strcasecmp(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    let la: Vec<u8> = a.iter().map(|c| c.to_ascii_lowercase()).collect();
    let lb: Vec<u8> = b.iter().map(|c| c.to_ascii_lowercase()).collect();
    la.cmp(&lb)
}

/// timelib_same_timezone
pub fn same_timezone(one: &Time, two: &Time) -> bool {
    if one.zone_type != two.zone_type {
        return false;
    }
    if one.zone_type == ZONETYPE_ABBR || one.zone_type == ZONETYPE_OFFSET {
        return one.z + (one.dst as i64 * 3600) == two.z + (two.dst as i64 * 3600);
    }
    if one.zone_type == ZONETYPE_ID {
        if let (Some(a), Some(b)) = (&one.tz_info, &two.tz_info) {
            return a.name == b.name;
        }
    }
    false
}

/// timelib_get_current_offset
pub fn get_current_offset(t: &Time) -> i64 {
    match t.zone_type {
        ZONETYPE_ABBR | ZONETYPE_OFFSET => t.z + (t.dst as i64 * 3600),
        ZONETYPE_ID => {
            if let Some(tz) = &t.tz_info {
                if let Some((off, _, _)) = tz.get_offset_info(t.sse) {
                    return off as i64;
                }
            }
            0
        }
        _ => 0,
    }
}

pub fn set_timezone_from_offset(t: &mut Time, utc_offset: i64) {
    t.tz_abbr = None;
    t.z = utc_offset;
    t.have_zone = 1;
    t.zone_type = ZONETYPE_OFFSET;
    t.dst = 0;
    t.tz_info = None;
}

pub fn set_timezone_from_abbr(t: &mut Time, abbr: &str, utc_offset: i64, dst: i32) {
    t.tz_abbr = Some(abbr.to_string());
    t.z = utc_offset;
    t.have_zone = 1;
    t.zone_type = ZONETYPE_ABBR;
    t.dst = dst;
    t.tz_info = None;
}

/// timelib_set_timezone
pub fn set_timezone(t: &mut Time, tz: &Rc<TzInfo>) {
    let off = tz.get_time_zone_info(t.sse);
    t.z = off.offset as i64;
    t.dst = off.is_dst as i32;
    t.tz_info = Some(tz.clone());
    t.tz_abbr = Some(off.abbr);
    t.have_zone = 1;
    t.zone_type = ZONETYPE_ID;
}

pub fn valid_time(h: i64, i: i64, s: i64) -> bool {
    !(!(0..=23).contains(&h) || !(0..=59).contains(&i) || !(0..=59).contains(&s))
}

pub fn valid_date(y: i64, m: i64, d: i64) -> bool {
    !(!(1..=12).contains(&m) || d < 1 || d > dow::days_in_month(y, m))
}
