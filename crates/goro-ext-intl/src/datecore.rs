//! The calendar / time zone core shared by IntlCalendar, IntlTimeZone
//! (calendar.rs, timezone.rs) and IntlDateFormatter, MessageFormatter
//! (dateformat.rs, msgformat.rs): ICU's Calendar / TimeZone services.
//!
//! This is the contract between those modules. It is OWNED by the
//! calendar/timezone implementation, which replaces the placeholder bodies
//! (UTC-only proleptic Gregorian) with real ones; the signatures are kept
//! (extend them additively) so that the formatters can rely on them.

use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::Obj;

/// ICU calendar field indices (UCalendarDateFields / IntlCalendar::FIELD_*).
pub const ERA: usize = 0;
pub const YEAR: usize = 1;
pub const MONTH: usize = 2;
pub const WEEK_OF_YEAR: usize = 3;
pub const WEEK_OF_MONTH: usize = 4;
pub const DATE: usize = 5;
pub const DAY_OF_YEAR: usize = 6;
pub const DAY_OF_WEEK: usize = 7;
pub const DAY_OF_WEEK_IN_MONTH: usize = 8;
pub const AM_PM: usize = 9;
pub const HOUR: usize = 10;
pub const HOUR_OF_DAY: usize = 11;
pub const MINUTE: usize = 12;
pub const SECOND: usize = 13;
pub const MILLISECOND: usize = 14;
pub const ZONE_OFFSET: usize = 15;
pub const DST_OFFSET: usize = 16;
pub const YEAR_WOY: usize = 17;
pub const DOW_LOCAL: usize = 18;
pub const EXTENDED_YEAR: usize = 19;
pub const JULIAN_DAY: usize = 20;
pub const MILLISECONDS_IN_DAY: usize = 21;
pub const IS_LEAP_MONTH: usize = 22;
pub const FIELD_COUNT: usize = 23;

/// A calendar's settings and instant (what an IntlCalendar object holds,
/// and what IntlDateFormatter formats with).
#[derive(Clone, Debug)]
pub struct CalInfo {
    /// UDate: milliseconds since the epoch.
    pub millis: f64,
    /// Time zone ID ("UTC", "Europe/Lisbon", "GMT+01:00").
    pub tz_id: String,
    /// ICU calendar type ("gregorian", "japanese", "buddhist"...).
    pub cal_type: String,
    /// The locale the calendar was created for.
    pub locale: String,
    /// UCAL_SUNDAY = 1 .. UCAL_SATURDAY = 7
    pub first_day_of_week: i32,
    pub minimal_days_in_first_week: i32,
    pub lenient: bool,
    /// Gregorian change date (UDate), for gregorian calendars.
    pub gregorian_change: f64,
}

impl CalInfo {
    /// A gregorian calendar in `tz_id` for `locale` at `millis`.
    pub fn gregorian(millis: f64, tz_id: &str, locale: &str) -> CalInfo {
        CalInfo {
            millis,
            tz_id: tz_id.to_string(),
            cal_type: "gregorian".to_string(),
            locale: locale.to_string(),
            first_day_of_week: 1,
            minimal_days_in_first_week: 1,
            lenient: true,
            gregorian_change: -12219292800000.0,
        }
    }
}

/// The fields of `info` at its instant, indexed by the constants above.
pub fn compute_fields(info: &CalInfo) -> [i32; FIELD_COUNT] {
    let (raw, dst) = tz_offsets(&info.tz_id, info.millis);
    let local = info.millis + (raw + dst) as f64;
    let days = (local / 86_400_000.0).floor() as i64;
    let ms_in_day = (local - days as f64 * 86_400_000.0) as i64;
    let (y, m, d) = civil_from_days(days);
    let mut f = [0i32; FIELD_COUNT];
    f[ERA] = if y > 0 { 1 } else { 0 };
    f[YEAR] = if y > 0 { y as i32 } else { (1 - y) as i32 };
    f[EXTENDED_YEAR] = y as i32;
    f[MONTH] = (m - 1) as i32;
    f[DATE] = d as i32;
    let doy = days - days_from_civil(y, 1, 1) + 1;
    f[DAY_OF_YEAR] = doy as i32;
    f[DAY_OF_WEEK] = ((days + 4).rem_euclid(7) + 1) as i32;
    f[DOW_LOCAL] = (f[DAY_OF_WEEK] - info.first_day_of_week).rem_euclid(7) + 1;
    f[DAY_OF_WEEK_IN_MONTH] = ((d - 1) / 7 + 1) as i32;
    f[HOUR_OF_DAY] = (ms_in_day / 3_600_000) as i32;
    f[AM_PM] = if f[HOUR_OF_DAY] >= 12 { 1 } else { 0 };
    f[HOUR] = f[HOUR_OF_DAY] % 12;
    f[MINUTE] = ((ms_in_day / 60_000) % 60) as i32;
    f[SECOND] = ((ms_in_day / 1000) % 60) as i32;
    f[MILLISECOND] = (ms_in_day % 1000) as i32;
    f[MILLISECONDS_IN_DAY] = ms_in_day as i32;
    f[ZONE_OFFSET] = raw;
    f[DST_OFFSET] = dst;
    f[JULIAN_DAY] = (days + 2_440_588) as i32;
    f[WEEK_OF_YEAR] = ((doy - 1) / 7 + 1) as i32;
    f[YEAR_WOY] = y as i32;
    f[WEEK_OF_MONTH] = ((d - 1) / 7 + 1) as i32;
    f
}

/// The instant of local fields (YEAR/EXTENDED_YEAR, MONTH, DATE, HOUR_OF_DAY,
/// MINUTE, SECOND, MILLISECOND) in `info`'s calendar and zone.
pub fn fields_to_millis(info: &CalInfo, f: &[i32; FIELD_COUNT]) -> f64 {
    let days = days_from_civil(f[EXTENDED_YEAR] as i64, f[MONTH] as i64 + 1, f[DATE] as i64);
    let local = days as f64 * 86_400_000.0
        + f[HOUR_OF_DAY] as f64 * 3_600_000.0
        + f[MINUTE] as f64 * 60_000.0
        + f[SECOND] as f64 * 1000.0
        + f[MILLISECOND] as f64;
    let (raw, dst) = tz_offsets(&info.tz_id, local);
    local - (raw + dst) as f64
}

/// The raw and DST offsets (ms) of zone `tz_id` at `millis` (UTC).
pub fn tz_offsets(_tz_id: &str, _millis: f64) -> (i32, i32) {
    (0, 0)
}

/// ICU's canonical ID of a time zone ID ("GMT+1" -> "GMT+01:00", links
/// resolved, case fixed), None when ICU does not know it.
pub fn canonical_tz_id(id: &str) -> Option<String> {
    Some(id.to_string())
}

/// The default time zone ID (date.timezone / date_default_timezone_get()).
pub fn default_tz_id(vm: &Vm) -> String {
    match vm.constants.get(b"date.timezone".as_slice()) {
        Some(v) if !v.to_php_string().as_bytes().is_empty() => v.to_php_string().to_string_lossy(),
        _ => "UTC".to_string(),
    }
}

/// TimeZone::getDisplayName(): `style` is IntlTimeZone::DISPLAY_*.
pub fn tz_display_name(tz_id: &str, _daylight: bool, _style: i64, _locale: &str) -> String {
    tz_id.to_string()
}

/// timezone_process_timezone_argument(): the time zone ID a `$timezone`
/// argument designates (null: the default; IntlTimeZone; DateTimeZone; a
/// string ID). Err(()) when the argument is invalid: the error has been
/// set (or an exception thrown) as php-src does, with `func` the active
/// function name.
#[allow(clippy::result_unit_err)]
pub fn tz_from_arg(vm: &mut Vm, v: &Value, func: &str) -> Result<String, ()> {
    let _ = func;
    match v {
        Value::Null | Value::Undef => Ok(default_tz_id(vm)),
        Value::String(s) => Ok(s.to_string_lossy()),
        _ => Ok("UTC".to_string()),
    }
}

/// A new IntlTimeZone object for `tz_id`.
pub fn new_tz_object(vm: &mut Vm, tz_id: &str) -> Option<Obj> {
    let _ = (vm, tz_id);
    None
}

/// The time zone ID of an IntlTimeZone object.
pub fn tz_object_id(o: &Obj) -> Option<String> {
    let _ = o;
    None
}

/// A new IntlCalendar object (class from `info.cal_type`: IntlGregorianCalendar
/// for gregorian) holding `info`.
pub fn new_cal_object(vm: &mut Vm, info: &CalInfo) -> Option<Obj> {
    let _ = (vm, info);
    None
}

/// The settings and instant of an IntlCalendar object (None if not an
/// initialized IntlCalendar).
pub fn cal_object_info(o: &Obj) -> Option<CalInfo> {
    let _ = o;
    None
}

/// The default calendar type of a locale ("gregorian", or the
/// @calendar=... keyword).
pub fn locale_cal_type(locale: &str) -> String {
    match locale.split_once("@calendar=") {
        Some((_, c)) => c.split(';').next().unwrap_or("gregorian").to_string(),
        None => "gregorian".to_string(),
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The proleptic Gregorian date of days since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}
