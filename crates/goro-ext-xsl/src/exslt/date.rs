//! EXSLT dates-and-times (libexslt/date.c)

use crate::transform::Transform;
use crate::xpath::{self, PCtx, XObj};

pub fn exists(name: &str) -> bool {
    matches!(
        name,
        "add"
            | "add-duration"
            | "date"
            | "date-time"
            | "day-abbreviation"
            | "day-in-month"
            | "day-in-week"
            | "day-in-year"
            | "day-name"
            | "day-of-week-in-month"
            | "difference"
            | "duration"
            | "hour-in-day"
            | "leap-year"
            | "minute-in-hour"
            | "month-abbreviation"
            | "month-in-year"
            | "month-name"
            | "second-in-minute"
            | "seconds"
            | "sum"
            | "time"
            | "week-in-month"
            | "week-in-year"
            | "year"
    )
}

// ---------------------------------------------------------------------------
// Types

const EXSLT_UNKNOWN: u32 = 0;
const XS_TIME: u32 = 1;
const XS_GDAY: u32 = XS_TIME << 1;
const XS_GMONTH: u32 = XS_GDAY << 1;
const XS_GMONTHDAY: u32 = XS_GMONTH | XS_GDAY;
const XS_GYEAR: u32 = XS_GMONTH << 1;
const XS_GYEARMONTH: u32 = XS_GYEAR | XS_GMONTH;
const XS_DATE: u32 = XS_GYEAR | XS_GMONTH | XS_GDAY;
const XS_DATETIME: u32 = XS_DATE | XS_TIME;

/// exsltDateVal
#[derive(Clone, Copy, Debug, Default)]
struct DateVal {
    ty: u32,
    year: i64,
    mon: u32,
    day: u32,
    hour: u32,
    min: u32,
    sec: f64,
    tz_flag: bool,
    tzo: i32,
}

/// exsltDateDurVal
#[derive(Clone, Copy, Debug, Default)]
struct DurVal {
    /// months (years too)
    mon: i64,
    day: i64,
    /// seconds (minutes and hours too), 0 <= sec < SECS_PER_DAY
    sec: f64,
}

const SECS_PER_MIN: i64 = 60;
const MINS_PER_HOUR: i64 = 60;
const HOURS_PER_DAY: i64 = 24;
const SECS_PER_HOUR: i64 = MINS_PER_HOUR * SECS_PER_MIN;
const SECS_PER_DAY: i64 = HOURS_PER_DAY * SECS_PER_HOUR;
const MINS_PER_DAY: i64 = HOURS_PER_DAY * MINS_PER_HOUR;
const DAYS_PER_EPOCH: i64 = 400 * 365 + 100 - 4 + 1;
const YEARS_PER_EPOCH: i64 = 400;

const YEAR_MAX: i64 = i64::MAX;
const YEAR_MIN: i64 = -i64::MAX + 1;

const DAYS_IN_MONTH: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const DAYS_IN_MONTH_LEAP: [i64; 12] = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const DAY_IN_YEAR_BY_MONTH: [i64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
const DAY_IN_LEAP_YEAR_BY_MONTH: [i64; 12] =
    [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];

fn is_leap(y: i64) -> bool {
    ((y & 3) == 0) && ((y % 25 != 0) || ((y & 15) == 0))
}

fn max_day_in_month(yr: i64, mon: u32) -> i64 {
    if is_leap(yr) {
        DAYS_IN_MONTH_LEAP[mon as usize - 1]
    } else {
        DAYS_IN_MONTH[mon as usize - 1]
    }
}

fn day_in_year(day: i64, month: u32, year: i64) -> i64 {
    (if is_leap(year) {
        DAY_IN_LEAP_YEAR_BY_MONTH[month as usize - 1]
    } else {
        DAY_IN_YEAR_BY_MONTH[month as usize - 1]
    }) + day
}

fn valid_month(m: i32) -> bool {
    (1..=12).contains(&m)
}
fn valid_day(d: i32) -> bool {
    (1..=31).contains(&d)
}
fn valid_hour(h: i32) -> bool {
    (0..=23).contains(&h)
}
fn valid_min(m: i32) -> bool {
    (0..=59).contains(&m)
}
fn valid_always(n: i32) -> bool {
    n >= 0
}
fn valid_sec(s: f64) -> bool {
    (0.0..60.0).contains(&s)
}
fn valid_tzo(t: i32) -> bool {
    t > -1440 && t < 1440
}

fn valid_mday(dt: &DateVal) -> bool {
    (dt.day as i64) <= max_day_in_month(dt.year, dt.mon)
}

fn valid_date(dt: &DateVal) -> bool {
    valid_month(dt.mon as i32) && valid_mday(dt)
}

fn valid_time(dt: &DateVal) -> bool {
    dt.hour <= 23 && dt.min <= 59 && valid_sec(dt.sec) && valid_tzo(dt.tzo)
}

fn valid_datetime(dt: &DateVal) -> bool {
    valid_date(dt) && valid_time(dt)
}

/// `*cur` with the C string terminator past the end
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

fn is_tzo_char(c: u8) -> bool {
    c == 0 || c == b'Z' || c == b'+' || c == b'-'
}

/// exsltDateCreateDate
fn create_date(ty: u32) -> DateVal {
    DateVal {
        ty,
        mon: 1,
        day: 1,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Bounded output buffer (xmlChar buf[100], *end = buf + 99)

struct Buf(Vec<u8>);

impl Buf {
    fn new() -> Self {
        Buf(Vec::with_capacity(100))
    }
    fn put(&mut self, c: u8) {
        if self.0.len() < 99 {
            self.0.push(c);
        }
    }
    fn into_string(self) -> String {
        String::from_utf8_lossy(&self.0).into_owned()
    }
}

// ---------------------------------------------------------------------------
// Parsing

/// _exsltDateParseGYear
fn parse_gyear(dt: &mut DateVal, s: &[u8], str_: &mut usize) -> i32 {
    let mut cur = *str_;
    let mut isneg = false;
    let mut digcnt = 0;
    let c = at(s, cur);
    if !is_digit(c) && c != b'-' && c != b'+' {
        return -1;
    }
    if c == b'-' {
        isneg = true;
        cur += 1;
    }
    let first_char = at(s, cur);
    while is_digit(at(s, cur)) {
        if dt.year >= YEAR_MAX / 10 {
            return -1;
        }
        dt.year = dt.year * 10 + (at(s, cur) - b'0') as i64;
        cur += 1;
        digcnt += 1;
    }
    // year must be at least 4 digits (CCYY); over 4 digits cannot have a
    // leading zero.
    if digcnt < 4 || (digcnt > 4 && first_char == b'0') {
        return 1;
    }
    if dt.year == 0 {
        return 2;
    }
    // The internal representation of negative years is continuous.
    if isneg {
        dt.year = -dt.year + 1;
    }
    *str_ = cur;
    0
}

/// exsltFormatGYear
fn format_gyear(b: &mut Buf, yr: i64) {
    if yr <= 0 {
        b.put(b'-');
    }
    let mut year = if yr <= 0 {
        yr.wrapping_neg().wrapping_add(1)
    } else {
        yr
    };
    let mut tmp: Vec<u8> = Vec::new();
    while year > 0 && tmp.len() < 99 {
        tmp.push(b'0' + (year % 10) as u8);
        year /= 10;
    }
    while tmp.len() < 4 {
        tmp.push(b'0');
    }
    while let Some(c) = tmp.pop() {
        b.put(c);
    }
}

/// PARSE_2_DIGITS: Ok(value) or Err(invalid); `cur` always advances by 2.
fn parse_2_digits(s: &[u8], cur: &mut usize, valid: fn(i32) -> bool) -> Result<i32, i32> {
    let c0 = at(s, *cur);
    let r = if !is_digit(c0) || !is_digit(at(s, *cur + 1)) {
        Err(1)
    } else {
        let val = (c0 - b'0') as i32 * 10 + (at(s, *cur + 1) - b'0') as i32;
        if !valid(val) { Err(2) } else { Ok(val) }
    };
    *cur += 2;
    r
}

/// exsltFormat2Digits
fn format_2digits(b: &mut Buf, num: u32) {
    b.put(b'0' + ((num / 10) % 10) as u8);
    b.put(b'0' + (num % 10) as u8);
}

/// _exsltDateParseGMonth
fn parse_gmonth(dt: &mut DateVal, s: &[u8], str_: &mut usize) -> i32 {
    let mut cur = *str_;
    match parse_2_digits(s, &mut cur, valid_month) {
        Ok(v) => dt.mon = v as u32,
        Err(e) => return e,
    }
    *str_ = cur;
    0
}

/// _exsltDateParseGDay
fn parse_gday(dt: &mut DateVal, s: &[u8], str_: &mut usize) -> i32 {
    let mut cur = *str_;
    match parse_2_digits(s, &mut cur, valid_day) {
        Ok(v) => dt.day = v as u32,
        Err(e) => return e,
    }
    *str_ = cur;
    0
}

/// exsltFormatYearMonthDay
fn format_year_month_day(b: &mut Buf, dt: &DateVal) {
    format_gyear(b, dt.year);
    b.put(b'-');
    format_2digits(b, dt.mon);
    b.put(b'-');
    format_2digits(b, dt.day);
}

/// _exsltDateParseTime
fn parse_time(dt: &mut DateVal, s: &[u8], str_: &mut usize) -> i32 {
    let mut cur = *str_;
    let hour = match parse_2_digits(s, &mut cur, valid_hour) {
        Ok(v) => v as u32,
        Err(e) => return e,
    };
    if at(s, cur) != b':' {
        return 1;
    }
    cur += 1;
    // the ':' insures this string is xs:time
    dt.hour = hour;

    match parse_2_digits(s, &mut cur, valid_min) {
        Ok(v) => dt.min = v as u32,
        Err(e) => return e,
    }
    if at(s, cur) != b':' {
        return 1;
    }
    cur += 1;

    // PARSE_FLOAT
    match parse_2_digits(s, &mut cur, valid_always) {
        Ok(v) => dt.sec = v as f64,
        Err(e) => return e,
    }
    if at(s, cur) == b'.' {
        let mut mult = 1.0f64;
        cur += 1;
        if !is_digit(at(s, cur)) {
            return 1;
        }
        while is_digit(at(s, cur)) {
            mult /= 10.0;
            dt.sec += (at(s, cur) - b'0') as f64 * mult;
            cur += 1;
        }
    }

    if !valid_time(dt) {
        return 2;
    }
    *str_ = cur;
    0
}

/// _exsltDateParseTimeZone
fn parse_time_zone(dt: &mut DateVal, s: &[u8], str_: &mut usize) -> i32 {
    let mut cur = *str_;
    match at(s, cur) {
        0 => {
            dt.tz_flag = false;
            dt.tzo = 0;
        }
        b'Z' => {
            dt.tz_flag = true;
            dt.tzo = 0;
            cur += 1;
        }
        c @ (b'+' | b'-') => {
            let isneg = c == b'-';
            cur += 1;
            let tmp = match parse_2_digits(s, &mut cur, valid_hour) {
                Ok(v) => v,
                Err(e) => return e,
            };
            if at(s, cur) != b':' {
                return 1;
            }
            cur += 1;
            dt.tzo = tmp * 60;
            let tmp = match parse_2_digits(s, &mut cur, valid_min) {
                Ok(v) => v,
                Err(e) => return e,
            };
            dt.tzo += tmp;
            if isneg {
                dt.tzo = -dt.tzo;
            }
            if !valid_tzo(dt.tzo) {
                return 2;
            }
        }
        _ => return 1,
    }
    *str_ = cur;
    0
}

/// exsltFormatTimeZone
fn format_time_zone(b: &mut Buf, tzo: i32) {
    if tzo == 0 {
        b.put(b'Z');
    } else {
        let a = tzo.unsigned_abs();
        b.put(if tzo < 0 { b'-' } else { b'+' });
        format_2digits(b, a / 60);
        b.put(b':');
        format_2digits(b, a % 60);
    }
}

// ---------------------------------------------------------------------------
// Current time

#[derive(Clone, Copy, Default)]
struct BrokenTime {
    sec: i64,
    min: i64,
    hour: i64,
    mday: i64,
    /// 0-11
    mon: i64,
    /// years since 1900
    year: i64,
}

/// gmtime_r(): None when the year does not fit a C int
fn gmtime(secs: i64) -> Option<BrokenTime> {
    let days = secs.div_euclid(SECS_PER_DAY);
    let rem = secs.rem_euclid(SECS_PER_DAY);
    // civil_from_days (H. Hinnant)
    let z = days as i128 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    let tm_year = y - 1900;
    if tm_year < i32::MIN as i128 || tm_year > i32::MAX as i128 {
        return None;
    }
    Some(BrokenTime {
        sec: rem % 60,
        min: (rem / 60) % 60,
        hour: rem / 3600,
        mday: d as i64,
        mon: (m - 1) as i64,
        year: tm_year as i64,
    })
}

#[cfg(all(unix, target_pointer_width = "64"))]
fn localtime(secs: i64) -> Option<BrokenTime> {
    use std::os::raw::{c_char, c_int, c_long};
    #[repr(C)]
    struct Tm {
        tm_sec: c_int,
        tm_min: c_int,
        tm_hour: c_int,
        tm_mday: c_int,
        tm_mon: c_int,
        tm_year: c_int,
        tm_wday: c_int,
        tm_yday: c_int,
        tm_isdst: c_int,
        tm_gmtoff: c_long,
        tm_zone: *const c_char,
        _pad: [u64; 4],
    }
    unsafe extern "C" {
        fn localtime_r(t: *const i64, tm: *mut Tm) -> *mut Tm;
    }
    let mut tm = Tm {
        tm_sec: 0,
        tm_min: 0,
        tm_hour: 0,
        tm_mday: 0,
        tm_mon: 0,
        tm_year: 0,
        tm_wday: 0,
        tm_yday: 0,
        tm_isdst: 0,
        tm_gmtoff: 0,
        tm_zone: std::ptr::null(),
        _pad: [0; 4],
    };
    // SAFETY: localtime_r only writes the struct tm we pass in (our layout
    // has trailing padding beyond the platform's struct tm).
    let r = unsafe { localtime_r(&secs, &mut tm) };
    if r.is_null() {
        return None;
    }
    Some(BrokenTime {
        sec: tm.tm_sec as i64,
        min: tm.tm_min as i64,
        hour: tm.tm_hour as i64,
        mday: tm.tm_mday as i64,
        mon: tm.tm_mon as i64,
        year: tm.tm_year as i64,
    })
}

/// Without a usable localtime_r() the local time zone is taken as UTC.
#[cfg(not(all(unix, target_pointer_width = "64")))]
fn localtime(secs: i64) -> Option<BrokenTime> {
    gmtime(secs)
}

/// strtol(s, NULL, 10); None when errno would be set (ERANGE)
fn strtol(s: &[u8]) -> Option<i64> {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut v: i128 = 0;
    let mut overflow = false;
    while i < s.len() && s[i].is_ascii_digit() {
        v = v * 10 + (s[i] - b'0') as i128;
        if v > i64::MAX as i128 + 1 {
            overflow = true;
            v = i64::MAX as i128 + 1;
        }
        i += 1;
    }
    if neg {
        v = -v;
    }
    if overflow || v > i64::MAX as i128 || v < i64::MIN as i128 {
        return None;
    }
    Some(v as i64)
}

/// exsltDateCurrent
fn current() -> Option<DateVal> {
    let mut ret = create_date(XS_DATETIME);
    let mut local: Option<BrokenTime> = None;
    let mut secs: i64 = 0;

    // Allow the date and time to be set externally by an exported
    // environment variable to enable reproducible builds.
    if let Some(sde) = std::env::var_os("SOURCE_DATE_EPOCH") {
        #[cfg(unix)]
        let bytes = {
            use std::os::unix::ffi::OsStrExt;
            sde.as_bytes().to_vec()
        };
        #[cfg(not(unix))]
        let bytes = sde.to_string_lossy().into_owned().into_bytes();
        if let Some(s) = strtol(&bytes) {
            secs = s;
            local = gmtime(secs);
        }
    }

    let local_tm = match local {
        Some(l) => l,
        None => {
            // get current time
            secs = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                Ok(d) => d.as_secs() as i64,
                Err(e) => -(e.duration().as_secs() as i64),
            };
            localtime(secs).or_else(|| gmtime(secs)).unwrap_or_default()
        }
    };

    // get real year, not years since 1900
    ret.year = local_tm.year + 1900;
    ret.mon = (local_tm.mon + 1) as u32;
    ret.day = local_tm.mday as u32;
    ret.hour = local_tm.hour as u32;
    ret.min = local_tm.min as u32;
    // floating point seconds
    ret.sec = local_tm.sec as f64;

    // determine the time zone offset from local to gm time
    let gm_tm = gmtime(secs)?;
    ret.tz_flag = false;
    let local_s = local_tm.hour * SECS_PER_HOUR + local_tm.min * SECS_PER_MIN + local_tm.sec;
    let gm_s = gm_tm.hour * SECS_PER_HOUR + gm_tm.min * SECS_PER_MIN + gm_tm.sec;
    let tzo = if local_tm.year < gm_tm.year {
        -((SECS_PER_DAY - local_s) + gm_s) / 60
    } else if local_tm.year > gm_tm.year {
        ((SECS_PER_DAY - gm_s) + local_s) / 60
    } else if local_tm.mon < gm_tm.mon {
        -((SECS_PER_DAY - local_s) + gm_s) / 60
    } else if local_tm.mon > gm_tm.mon {
        ((SECS_PER_DAY - gm_s) + local_s) / 60
    } else if local_tm.mday < gm_tm.mday {
        -((SECS_PER_DAY - local_s) + gm_s) / 60
    } else if local_tm.mday > gm_tm.mday {
        ((SECS_PER_DAY - gm_s) + local_s) / 60
    } else {
        (local_s - gm_s) / 60
    };
    // signed int tzo:12
    ret.tzo = ((tzo as i32) << 20) >> 20;
    Some(ret)
}

/// exsltDateParse
fn parse(date_time: &[u8]) -> Option<DateVal> {
    let s = date_time;
    let mut cur = 0usize;
    let mut dt = create_date(EXSLT_UNKNOWN);

    macro_rules! return_type_if_valid {
        ($t:expr) => {
            if is_tzo_char(at(s, cur)) {
                let ret = parse_time_zone(&mut dt, s, &mut cur);
                if ret == 0 {
                    if at(s, cur) != 0 {
                        return None;
                    }
                    dt.ty = $t;
                    return Some(dt);
                }
            }
        };
    }

    if at(s, 0) == b'-' && at(s, 1) == b'-' {
        // It's an incomplete date (xs:gMonthDay, xs:gMonth or xs:gDay)
        cur += 2;

        // is it an xs:gDay?
        if at(s, cur) == b'-' {
            cur += 1;
            if parse_gday(&mut dt, s, &mut cur) != 0 {
                return None;
            }
            return_type_if_valid!(XS_GDAY);
            return None;
        }

        // it should be an xs:gMonthDay or xs:gMonth
        if parse_gmonth(&mut dt, s, &mut cur) != 0 {
            return None;
        }
        if at(s, cur) != b'-' {
            return None;
        }
        cur += 1;

        // is it an xs:gMonth?
        if at(s, cur) == b'-' {
            cur += 1;
            return_type_if_valid!(XS_GMONTH);
            return None;
        }

        // it should be an xs:gMonthDay
        if parse_gday(&mut dt, s, &mut cur) != 0 {
            return None;
        }
        return_type_if_valid!(XS_GMONTHDAY);
        return None;
    }

    // It's a right-truncated date or an xs:time.
    // Try to parse an xs:time then fallback on right-truncated dates.
    if is_digit(at(s, cur)) && parse_time(&mut dt, s, &mut cur) == 0 {
        // it's an xs:time
        return_type_if_valid!(XS_TIME);
    }

    // fallback on date parsing
    cur = 0;

    if parse_gyear(&mut dt, s, &mut cur) != 0 {
        return None;
    }
    // is it an xs:gYear?
    return_type_if_valid!(XS_GYEAR);

    if at(s, cur) != b'-' {
        return None;
    }
    cur += 1;

    if parse_gmonth(&mut dt, s, &mut cur) != 0 {
        return None;
    }
    // is it an xs:gYearMonth?
    return_type_if_valid!(XS_GYEARMONTH);

    if at(s, cur) != b'-' {
        return None;
    }
    cur += 1;

    if parse_gday(&mut dt, s, &mut cur) != 0 || !valid_date(&dt) {
        return None;
    }
    // is it an xs:date?
    return_type_if_valid!(XS_DATE);

    if at(s, cur) != b'T' {
        return None;
    }
    cur += 1;

    // it should be an xs:dateTime
    if parse_time(&mut dt, s, &mut cur) != 0 {
        return None;
    }
    let ret = parse_time_zone(&mut dt, s, &mut cur);
    if ret != 0 || at(s, cur) != 0 || !valid_datetime(&dt) {
        return None;
    }
    dt.ty = XS_DATETIME;
    Some(dt)
}

/// exsltDateParseDuration
fn parse_duration(duration: &[u8]) -> Option<DurVal> {
    let s = duration;
    let mut cur = 0usize;
    let mut isneg = false;
    let mut seq: usize = 0;
    let mut secs: i64 = 0;
    let mut sec_frac = 0.0f64;
    const DESIG: [u8; 6] = *b"YMDHMS";

    if at(s, cur) == b'-' {
        isneg = true;
        cur += 1;
    }
    // duration must start with 'P' (after sign)
    let c = at(s, cur);
    cur += 1;
    if c != b'P' {
        return None;
    }
    if at(s, cur) == 0 {
        return None;
    }

    let mut dur = DurVal::default();

    while at(s, cur) != 0 {
        let mut num: i64 = 0;
        let mut has_digits = false;
        let mut has_frac = false;

        // input string should be empty or invalid date/time item
        if seq >= DESIG.len() {
            return None;
        }

        // T designator must be present for time items
        if at(s, cur) == b'T' {
            if seq > 3 {
                return None;
            }
            cur += 1;
            seq = 3;
        } else if seq == 3 {
            return None;
        }

        // Parse integral part.
        while is_digit(at(s, cur)) {
            let digit = (at(s, cur) - b'0') as i64;
            if num > i64::MAX / 10 {
                return None;
            }
            num *= 10;
            if num > i64::MAX - digit {
                return None;
            }
            num += digit;
            has_digits = true;
            cur += 1;
        }

        if at(s, cur) == b'.' {
            // Parse fractional part.
            let mut mult = 1.0f64;
            cur += 1;
            has_frac = true;
            while is_digit(at(s, cur)) {
                mult /= 10.0;
                sec_frac += (at(s, cur) - b'0') as f64 * mult;
                has_digits = true;
                cur += 1;
            }
        }

        while at(s, cur) != DESIG[seq] {
            seq += 1;
            // No T designator or invalid char.
            if seq == 3 || seq == DESIG.len() {
                return None;
            }
        }
        cur += 1;

        if !has_digits || (has_frac && seq != 5) {
            return None;
        }

        match seq {
            0 => {
                // Year
                if num > i64::MAX / 12 {
                    return None;
                }
                dur.mon = num * 12;
            }
            1 => {
                // Month
                if dur.mon > i64::MAX - num {
                    return None;
                }
                dur.mon += num;
            }
            2 => {
                // Day
                dur.day = num;
            }
            3 => {
                // Hour
                let days = num / HOURS_PER_DAY;
                if dur.day > i64::MAX - days {
                    return None;
                }
                dur.day += days;
                secs = (num % HOURS_PER_DAY) * SECS_PER_HOUR;
            }
            4 => {
                // Minute
                let days = num / MINS_PER_DAY;
                if dur.day > i64::MAX - days {
                    return None;
                }
                dur.day += days;
                secs += (num % MINS_PER_DAY) * SECS_PER_MIN;
            }
            5 => {
                // Second
                let days = num / SECS_PER_DAY;
                if dur.day > i64::MAX - days {
                    return None;
                }
                dur.day += days;
                secs += num % SECS_PER_DAY;
            }
            _ => {}
        }

        seq += 1;
    }

    let days = secs / SECS_PER_DAY;
    if dur.day > i64::MAX - days {
        return None;
    }
    dur.day += days;
    dur.sec = (secs % SECS_PER_DAY) as f64 + sec_frac;

    if isneg {
        dur.mon = -dur.mon;
        dur.day = -dur.day;
        if dur.sec != 0.0 {
            dur.sec = SECS_PER_DAY as f64 - dur.sec;
            dur.day = dur.day.wrapping_sub(1);
        }
    }

    Some(dur)
}

/// exsltFormatLong
fn format_long(b: &mut Buf, mut num: i64) {
    let mut buf: Vec<u8> = Vec::with_capacity(20);
    while buf.len() < 20 {
        buf.push((b'0' as i64).wrapping_add(num % 10) as u8);
        num /= 10;
        if num == 0 {
            break;
        }
    }
    while let Some(c) = buf.pop() {
        b.put(c);
    }
}

/// exsltFormatNanoseconds
fn format_nanoseconds(b: &mut Buf, mut nsecs: i64) {
    if nsecs > 0 {
        b.put(b'.');
        let mut p10: i64 = 100000000;
        while nsecs > 0 && p10 > 0 {
            let digit = nsecs / p10;
            b.put((b'0' as i64 + digit) as u8);
            nsecs -= digit * p10;
            p10 /= 10;
        }
    }
}

/// exsltDateFormatDuration
fn format_duration(dur: &DurVal) -> Option<String> {
    // quick and dirty check
    if dur.sec == 0.0 && dur.day == 0 && dur.mon == 0 {
        return Some("P0D".to_string());
    }
    let mut secs = dur.sec;
    let mut days = dur.day;
    let mut months = dur.mon;
    let mut b = Buf::new();
    let mut neg = false;

    if days < 0 {
        if secs != 0.0 {
            secs = SECS_PER_DAY as f64 - secs;
            days += 1;
        }
        days = days.wrapping_neg();
        neg = true;
    }
    if months < 0 {
        months = months.wrapping_neg();
        neg = true;
    }
    if neg {
        b.put(b'-');
    }
    b.put(b'P');

    if months >= 12 {
        let years = months / 12;
        months -= years * 12;
        format_long(&mut b, years);
        b.put(b'Y');
    }
    if months != 0 {
        format_long(&mut b, months);
        b.put(b'M');
    }
    if days != 0 {
        format_long(&mut b, days);
        b.put(b'D');
    }

    let tmp = secs.floor();
    let mut int_secs = tmp as i64;
    // Round to nearest to avoid issues with floating point precision
    let mut nsecs = ((secs - tmp) * 1000000000.0 + 0.5).floor() as i64;
    if nsecs >= 1000000000 {
        nsecs -= 1000000000;
        int_secs += 1;
    }

    if int_secs > 0 || nsecs > 0 {
        b.put(b'T');
        if int_secs >= SECS_PER_HOUR {
            let hours = int_secs / SECS_PER_HOUR;
            int_secs -= hours * SECS_PER_HOUR;
            format_long(&mut b, hours);
            b.put(b'H');
        }
        if int_secs >= SECS_PER_MIN {
            let mins = int_secs / SECS_PER_MIN;
            int_secs -= mins * SECS_PER_MIN;
            format_long(&mut b, mins);
            b.put(b'M');
        }
        if int_secs > 0 || nsecs > 0 {
            format_long(&mut b, int_secs);
            format_nanoseconds(&mut b, nsecs);
            b.put(b'S');
        }
    }
    Some(b.into_string())
}

/// exsltFormatTwoDigits
fn format_two_digits(b: &mut Buf, num: i32) {
    if !(0..100).contains(&num) {
        return;
    }
    b.put(b'0' + (num / 10) as u8);
    b.put(b'0' + (num % 10) as u8);
}

/// exsltFormatTime
fn format_time_part(b: &mut Buf, dt: &DateVal) {
    format_two_digits(b, dt.hour as i32);
    b.put(b':');
    format_two_digits(b, dt.min as i32);
    b.put(b':');
    let tmp = dt.sec.floor();
    let int_secs = tmp as i64;
    // Round to nearest to avoid issues with floating point precision, but
    // don't carry over so seconds stay below 60.
    let mut nsecs = ((dt.sec - tmp) * 1000000000.0 + 0.5).floor() as i64;
    if nsecs > 999999999 {
        nsecs = 999999999;
    }
    format_two_digits(b, int_secs as i32);
    format_nanoseconds(b, nsecs);
}

/// exsltDateFormatDateTime
fn format_date_time(dt: &DateVal) -> Option<String> {
    if !valid_datetime(dt) {
        return None;
    }
    let mut b = Buf::new();
    format_year_month_day(&mut b, dt);
    b.put(b'T');
    format_time_part(&mut b, dt);
    format_time_zone(&mut b, dt.tzo);
    Some(b.into_string())
}

/// exsltDateFormatDate
fn format_date(dt: &DateVal) -> Option<String> {
    if !valid_datetime(dt) {
        return None;
    }
    let mut b = Buf::new();
    format_year_month_day(&mut b, dt);
    if dt.tz_flag || dt.tzo != 0 {
        format_time_zone(&mut b, dt.tzo);
    }
    Some(b.into_string())
}

/// exsltDateFormatTime
fn format_time(dt: &DateVal) -> Option<String> {
    if !valid_time(dt) {
        return None;
    }
    let mut b = Buf::new();
    format_time_part(&mut b, dt);
    if dt.tz_flag || dt.tzo != 0 {
        format_time_zone(&mut b, dt.tzo);
    }
    Some(b.into_string())
}

/// exsltDateFormat
fn format(dt: &DateVal) -> Option<String> {
    match dt.ty {
        XS_DATETIME => return format_date_time(dt),
        XS_DATE => return format_date(dt),
        XS_TIME => return format_time(dt),
        _ => {}
    }
    if dt.ty & XS_GYEAR != 0 {
        let mut b = Buf::new();
        format_gyear(&mut b, dt.year);
        if dt.ty == XS_GYEARMONTH {
            b.put(b'-');
            format_2digits(&mut b, dt.mon);
        }
        if dt.tz_flag || dt.tzo != 0 {
            format_time_zone(&mut b, dt.tzo);
        }
        return Some(b.into_string());
    }
    None
}

/// _exsltDateCastYMToDays
fn cast_ym_to_days(dt: &DateVal) -> i64 {
    let y = dt.year;
    if y <= 0 {
        ((y - 1) * 365) + ((y / 4) - (y / 100) + (y / 400)) + day_in_year(0, dt.mon, y) - 1
    } else {
        ((y - 1) * 365)
            + (((y - 1) / 4) - ((y - 1) / 100) + ((y - 1) / 400))
            + day_in_year(0, dt.mon, y)
    }
}

/// TIME_TO_NUMBER
fn time_to_number(dt: &DateVal) -> f64 {
    ((dt.hour as i64 * SECS_PER_HOUR) + (dt.min as i64 * SECS_PER_MIN)) as f64 + dt.sec
}

/// _exsltDateTruncateDate
fn truncate_date(dt: &mut DateVal, ty: u32) {
    if ty & XS_TIME != XS_TIME {
        dt.hour = 0;
        dt.min = 0;
        dt.sec = 0.0;
    }
    if ty & XS_GDAY != XS_GDAY {
        dt.day = 1;
    }
    if ty & XS_GMONTH != XS_GMONTH {
        dt.mon = 1;
    }
    if ty & XS_GYEAR != XS_GYEAR {
        dt.year = 0;
    }
    dt.ty = ty;
}

/// _exsltDateDayInWeek: day in week (Sunday = 0)
fn day_in_week_of(yday: i64, yr: i64) -> i64 {
    if yr <= 0 {
        // Compute modulus twice to avoid integer overflow
        let mut ret = (yr % 7 - 2 + ((yr / 4) - (yr / 100) + (yr / 400)) + yday) % 7;
        if ret < 0 {
            ret += 7;
        }
        ret
    } else {
        ((yr % 7 - 1) + (((yr - 1) / 4) - ((yr - 1) / 100) + ((yr - 1) / 400)) + yday) % 7
    }
}

/// _exsltDateAdd
fn date_add(dt: &DateVal, dur: &DurVal) -> Option<DateVal> {
    let mut ret = create_date(dt.ty);

    // month
    let mut temp = dt.mon as i64 + dur.mon % 12;
    let mut carry = dur.mon / 12;
    if temp < 1 {
        temp += 12;
        carry -= 1;
    } else if temp > 12 {
        temp -= 12;
        carry += 1;
    }
    ret.mon = temp as u32;

    // year (may be modified later)
    //
    // Add epochs from dur->day now to avoid overflow later and to speed up
    // pathological cases.
    carry += (dur.day / DAYS_PER_EPOCH) * YEARS_PER_EPOCH;
    if (carry > 0 && dt.year > YEAR_MAX - carry) || (carry < 0 && dt.year < YEAR_MIN - carry) {
        // Overflow
        return None;
    }
    ret.year = dt.year + carry;

    // time zone
    ret.tzo = dt.tzo;
    ret.tz_flag = dt.tz_flag;

    // seconds
    let sum = dt.sec + dur.sec;
    ret.sec = sum % 60.0;
    carry = (sum / 60.0) as i64;

    // minute
    temp = dt.min as i64 + carry % 60;
    carry /= 60;
    if temp >= 60 {
        temp -= 60;
        carry += 1;
    }
    ret.min = temp as u32;

    // hours
    temp = dt.hour as i64 + carry % 24;
    carry /= 24;
    if temp >= 24 {
        temp -= 24;
        carry += 1;
    }
    ret.hour = temp as u32;

    // days
    let mdim = max_day_in_month(ret.year, ret.mon);
    temp = if dt.day as i64 > mdim {
        mdim
    } else if dt.day < 1 {
        1
    } else {
        dt.day as i64
    };

    temp += dur.day % DAYS_PER_EPOCH + carry;

    loop {
        if temp < 1 {
            if ret.mon > 1 {
                ret.mon -= 1;
            } else {
                if ret.year == YEAR_MIN {
                    return None;
                }
                ret.mon = 12;
                ret.year -= 1;
            }
            temp += max_day_in_month(ret.year, ret.mon);
        } else if temp > max_day_in_month(ret.year, ret.mon) {
            temp -= max_day_in_month(ret.year, ret.mon);
            if ret.mon < 12 {
                ret.mon += 1;
            } else {
                if ret.year == YEAR_MAX {
                    return None;
                }
                ret.mon = 1;
                ret.year += 1;
            }
        } else {
            break;
        }
    }

    ret.day = temp as u32;

    // adjust the date/time type to the date values
    if ret.ty != XS_DATETIME {
        if ret.hour != 0 || ret.min != 0 || ret.sec != 0.0 {
            ret.ty = XS_DATETIME;
        } else if ret.ty != XS_DATE {
            if ret.day != 1 {
                ret.ty = XS_DATE;
            } else if ret.ty != XS_GYEARMONTH && ret.mon != 1 {
                ret.ty = XS_GYEARMONTH;
            }
        }
    }

    Some(ret)
}

/// _exsltDateDifference: y - x
fn date_difference(x: &mut DateVal, y: &mut DateVal, flag: bool) -> Option<DurVal> {
    if !(XS_GYEAR..=XS_DATETIME).contains(&x.ty) || !(XS_GYEAR..=XS_DATETIME).contains(&y.ty) {
        return None;
    }

    // the operand with the most specific format must be converted to the
    // same type as the operand with the least specific format.
    if x.ty != y.ty {
        if x.ty < y.ty {
            truncate_date(y, x.ty);
        } else {
            truncate_date(x, y.ty);
        }
    }

    let mut ret = DurVal::default();

    if (x.ty == XS_GYEAR || x.ty == XS_GYEARMONTH) && !flag {
        // compute the difference in months
        if x.year >= i64::MAX / 24
            || x.year <= i64::MIN / 24
            || y.year >= i64::MAX / 24
            || y.year <= i64::MIN / 24
        {
            // Possible overflow.
            return None;
        }
        ret.mon = (y.year - x.year) * 12 + (y.mon as i64 - x.mon as i64);
    } else {
        if x.year > i64::MAX / 731
            || x.year < i64::MIN / 731
            || y.year > i64::MAX / 731
            || y.year < i64::MIN / 731
        {
            // Possible overflow.
            return None;
        }

        ret.sec = time_to_number(y) - time_to_number(x);
        ret.sec += ((x.tzo - y.tzo) as i64 * SECS_PER_MIN) as f64;
        let carry = (ret.sec / SECS_PER_DAY as f64).floor() as i64;
        ret.sec -= (carry * SECS_PER_DAY) as f64;

        ret.day = cast_ym_to_days(y) - cast_ym_to_days(x);
        ret.day += y.day as i64 - x.day as i64;
        ret.day += carry;
    }

    Some(ret)
}

/// _exsltDateAddDurCalc: false on error
fn add_dur_calc(x: &DurVal, y: &DurVal) -> Option<DurVal> {
    let mut ret = DurVal::default();
    // months
    if (x.mon > 0 && y.mon > i64::MAX - x.mon) || (x.mon < 0 && y.mon <= i64::MIN - x.mon) {
        return None;
    }
    ret.mon = x.mon + y.mon;

    // days
    if (x.day > 0 && y.day > i64::MAX - x.day) || (x.day < 0 && y.day <= i64::MIN - x.day) {
        return None;
    }
    ret.day = x.day + y.day;

    // seconds
    ret.sec = x.sec + y.sec;
    if ret.sec >= SECS_PER_DAY as f64 {
        if ret.day == i64::MAX {
            return None;
        }
        ret.sec -= SECS_PER_DAY as f64;
        ret.day += 1;
    }

    // are the results indeterminate? i.e. how do you subtract days from
    // months or years?
    if ret.day >= 0 {
        if (ret.day > 0 || ret.sec > 0.0) && ret.mon < 0 {
            return None;
        }
    } else if ret.mon > 0 {
        return None;
    }
    Some(ret)
}

// ---------------------------------------------------------------------------
// EXSLT - Dates and Times functions

/// the date argument, or the current date-time when absent
fn get_date(date_time: Option<&[u8]>, allowed: &[u32]) -> Option<DateVal> {
    match date_time {
        None => current(),
        Some(s) => {
            let dt = parse(s)?;
            if !allowed.contains(&dt.ty) {
                return None;
            }
            Some(dt)
        }
    }
}

const YEAR_TYPES: [u32; 4] = [XS_DATETIME, XS_DATE, XS_GYEARMONTH, XS_GYEAR];
const DATE_TYPES: [u32; 2] = [XS_DATETIME, XS_DATE];
const TIME_TYPES: [u32; 2] = [XS_DATETIME, XS_TIME];

/// exsltDateDate
fn date_fn(s: Option<&[u8]>) -> Option<String> {
    format_date(&get_date(s, &DATE_TYPES)?)
}

/// exsltDateTime
fn time_fn(s: Option<&[u8]>) -> Option<String> {
    format_time(&get_date(s, &TIME_TYPES)?)
}

/// exsltDateYear
fn year_fn(s: Option<&[u8]>) -> f64 {
    let Some(dt) = get_date(s, &YEAR_TYPES) else {
        return f64::NAN;
    };
    let mut year = dt.year;
    if year <= 0 {
        // Adjust for missing year 0.
        year -= 1;
    }
    year as f64
}

/// exsltDateLeapYear
fn leap_year_fn(s: Option<&[u8]>) -> XObj {
    match get_date(s, &YEAR_TYPES) {
        None => XObj::Num(f64::NAN),
        Some(dt) => XObj::Bool(is_leap(dt.year)),
    }
}

/// exsltDateMonthInYear
fn month_in_year_fn(s: Option<&[u8]>) -> f64 {
    match get_date(
        s,
        &[XS_DATETIME, XS_DATE, XS_GYEARMONTH, XS_GMONTH, XS_GMONTHDAY],
    ) {
        None => f64::NAN,
        Some(dt) => dt.mon as f64,
    }
}

const MONTH_NAMES: [&str; 13] = [
    "",
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTH_ABBREVIATIONS: [&str; 13] = [
    "", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const DAY_NAMES: [&str; 8] = [
    "",
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const DAY_ABBREVIATIONS: [&str; 8] = ["", "Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// exsltDateMonthName / exsltDateMonthAbbreviation
fn month_name_fn(s: Option<&[u8]>, abbrev: bool) -> &'static str {
    let month = month_in_year_fn(s);
    let mut index = 0;
    if !month.is_nan() && (1.0..=12.0).contains(&month) {
        index = month as usize;
    }
    if abbrev {
        MONTH_ABBREVIATIONS[index]
    } else {
        MONTH_NAMES[index]
    }
}

/// exsltDateWeekInYear
fn week_in_year_fn(s: Option<&[u8]>) -> f64 {
    let Some(dt) = get_date(s, &DATE_TYPES) else {
        return f64::NAN;
    };
    let mut diy = day_in_year(dt.day as i64, dt.mon, dt.year);

    // Determine day-in-week (0=Sun, 1=Mon, etc.) then adjust so Monday is
    // the first day-in-week
    let diw = (day_in_week_of(diy, dt.year) + 6) % 7;

    // ISO 8601 adjustment, 3 is Thu
    diy += 3 - diw;
    if diy < 1 {
        let mut year = dt.year - 1;
        if year == 0 {
            year -= 1;
        }
        diy += day_in_year(31, 12, year);
    } else if diy > day_in_year(31, 12, dt.year) {
        diy -= day_in_year(31, 12, dt.year);
    }

    (((diy - 1) / 7) + 1) as f64
}

/// exsltDateWeekInMonth
fn week_in_month_fn(s: Option<&[u8]>) -> f64 {
    let Some(dt) = get_date(s, &DATE_TYPES) else {
        return f64::NAN;
    };
    let fdiy = day_in_year(1, dt.mon, dt.year);
    // Determine day-in-week (0=Sun, 1=Mon, etc.) then adjust so Monday is
    // the first day-in-week
    let fdiw = (day_in_week_of(fdiy, dt.year) + 6) % 7;
    (((dt.day as i64 + fdiw - 1) / 7) + 1) as f64
}

/// exsltDateDayInYear
fn day_in_year_fn(s: Option<&[u8]>) -> f64 {
    match get_date(s, &DATE_TYPES) {
        None => f64::NAN,
        Some(dt) => day_in_year(dt.day as i64, dt.mon, dt.year) as f64,
    }
}

/// exsltDateDayInMonth
fn day_in_month_fn(s: Option<&[u8]>) -> f64 {
    match get_date(s, &[XS_DATETIME, XS_DATE, XS_GMONTHDAY, XS_GDAY]) {
        None => f64::NAN,
        Some(dt) => dt.day as f64,
    }
}

/// exsltDateDayOfWeekInMonth
fn day_of_week_in_month_fn(s: Option<&[u8]>) -> f64 {
    match get_date(s, &DATE_TYPES) {
        None => f64::NAN,
        Some(dt) => (((dt.day as i64 - 1) / 7) + 1) as f64,
    }
}

/// exsltDateDayInWeek
fn day_in_week_fn(s: Option<&[u8]>) -> f64 {
    match get_date(s, &DATE_TYPES) {
        None => f64::NAN,
        Some(dt) => {
            let diy = day_in_year(dt.day as i64, dt.mon, dt.year);
            (day_in_week_of(diy, dt.year) + 1) as f64
        }
    }
}

/// exsltDateDayName / exsltDateDayAbbreviation
fn day_name_fn(s: Option<&[u8]>, abbrev: bool) -> &'static str {
    let day = day_in_week_fn(s);
    let mut index = 0;
    if !day.is_nan() && (1.0..=7.0).contains(&day) {
        index = day as usize;
    }
    if abbrev {
        DAY_ABBREVIATIONS[index]
    } else {
        DAY_NAMES[index]
    }
}

/// exsltDateHourInDay
fn hour_in_day_fn(s: Option<&[u8]>) -> f64 {
    match get_date(s, &TIME_TYPES) {
        None => f64::NAN,
        Some(dt) => dt.hour as f64,
    }
}

/// exsltDateMinuteInHour
fn minute_in_hour_fn(s: Option<&[u8]>) -> f64 {
    match get_date(s, &TIME_TYPES) {
        None => f64::NAN,
        Some(dt) => dt.min as f64,
    }
}

/// exsltDateSecondInMinute
fn second_in_minute_fn(s: Option<&[u8]>) -> f64 {
    match get_date(s, &TIME_TYPES) {
        None => f64::NAN,
        Some(dt) => dt.sec,
    }
}

/// exsltDateAdd
fn add_fn(xstr: &[u8], ystr: &[u8]) -> Option<String> {
    let dt = parse(xstr)?;
    if !(XS_GYEAR..=XS_DATETIME).contains(&dt.ty) {
        return None;
    }
    let dur = parse_duration(ystr)?;
    let res = date_add(&dt, &dur)?;
    format(&res)
}

/// exsltDateAddDuration
fn add_duration_fn(xstr: &[u8], ystr: &[u8]) -> Option<String> {
    let x = parse_duration(xstr)?;
    let y = parse_duration(ystr)?;
    let res = add_dur_calc(&x, &y)?;
    format_duration(&res)
}

/// exsltDateSeconds
fn seconds_fn(date_time: Option<&[u8]>) -> f64 {
    let mut dur = None;
    let dt = match date_time {
        None => match current() {
            None => return f64::NAN,
            Some(d) => Some(d),
        },
        Some(s) => {
            let d = parse(s);
            if d.is_none() {
                dur = parse_duration(s);
            }
            d
        }
    };

    if let Some(mut dt) = dt.filter(|d| d.ty >= XS_GYEAR) {
        // compute the difference between the given (or current) date and
        // epoch date
        let mut y = create_date(XS_DATETIME);
        y.year = 1970;
        y.mon = 1;
        y.day = 1;
        y.tz_flag = true;
        if let Some(diff) = date_difference(&mut y, &mut dt, true) {
            return diff.day as f64 * SECS_PER_DAY as f64 + diff.sec;
        }
        f64::NAN
    } else if let Some(dur) = dur.filter(|d| d.mon == 0) {
        dur.day as f64 * SECS_PER_DAY as f64 + dur.sec
    } else {
        f64::NAN
    }
}

/// exsltDateDifference
fn difference_fn(xstr: &[u8], ystr: &[u8]) -> Option<String> {
    let mut x = parse(xstr)?;
    let mut y = parse(ystr)?;
    if !(XS_GYEAR..=XS_DATETIME).contains(&x.ty) || !(XS_GYEAR..=XS_DATETIME).contains(&y.ty) {
        return None;
    }
    let dur = date_difference(&mut x, &mut y, false)?;
    format_duration(&dur)
}

/// exsltDateDuration
fn duration_fn(number: Option<&[u8]>) -> Option<String> {
    let secs = match number {
        None => seconds_fn(None),
        Some(n) => xpath::string_eval_number(&String::from_utf8_lossy(n)),
    };
    if secs.is_nan() {
        return None;
    }
    let days = (secs / SECS_PER_DAY as f64).floor();
    if days <= i64::MIN as f64 || days >= i64::MAX as f64 {
        return None;
    }
    let dur = DurVal {
        mon: 0,
        day: days as i64,
        sec: secs - days * SECS_PER_DAY as f64,
    };
    format_duration(&dur)
}

// ---------------------------------------------------------------------------
// Wrappers for use by the XPath engine

/// The optional string argument: Err(()) when an error was raised.
fn opt_string_arg(t: &mut Transform, p: &mut PCtx, nargs: usize) -> Result<Option<String>, ()> {
    if nargs > 1 {
        xpath::x_arity_error(t, p);
        return Err(());
    }
    if nargs == 1 {
        match xpath::x_pop_string(t, p) {
            Some(s) if p.error == 0 => return Ok(Some(s)),
            _ => {
                xpath::xp_err(t, p, xpath::XPATH_INVALID_TYPE);
                return Err(());
            }
        }
    }
    Ok(None)
}

/// Two string arguments (ystr popped first)
fn two_string_args(t: &mut Transform, p: &mut PCtx, nargs: usize) -> Option<(String, String)> {
    if nargs != 2 {
        xpath::x_arity_error(t, p);
        return None;
    }
    let y = xpath::x_pop_string(t, p)?;
    if p.error != 0 {
        return None;
    }
    let x = xpath::x_pop_string(t, p)?;
    if p.error != 0 {
        return None;
    }
    Some((x, y))
}

fn push_opt_str(p: &mut PCtx, s: Option<String>) {
    p.push(XObj::Str(s.unwrap_or_default()));
}

/// exsltDateSumFunction
fn sum_function(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    if nargs != 1 {
        xpath::x_arity_error(t, p);
        return;
    }
    let Some(ns) = xpath::x_pop_node_set(t, p) else {
        return;
    };
    if p.error != 0 {
        return;
    }
    if ns.is_empty() {
        p.push(XObj::Str(String::new()));
        return;
    }
    let mut total = DurVal::default();
    for &n in &ns {
        let tmp = xpath::node_string_id(n);
        let Some(x) = parse_duration(tmp.as_bytes()) else {
            p.push(XObj::Str(String::new()));
            return;
        };
        match add_dur_calc(&total, &x) {
            Some(r) => total = r,
            None => {
                p.push(XObj::Str(String::new()));
                return;
            }
        }
    }
    push_opt_str(p, format_duration(&total));
}

pub fn call(t: &mut Transform, p: &mut PCtx, name: &str, nargs: usize) -> bool {
    match name {
        "date-time" => {
            if nargs != 0 {
                xpath::x_arity_error(t, p);
                return true;
            }
            push_opt_str(p, current().and_then(|d| format_date_time(&d)));
        }
        "add" | "add-duration" | "difference" => {
            let Some((x, y)) = two_string_args(t, p, nargs) else {
                return true;
            };
            let (x, y) = (x.as_bytes(), y.as_bytes());
            let r = match name {
                "add" => add_fn(x, y),
                "add-duration" => add_duration_fn(x, y),
                _ => difference_fn(x, y),
            };
            push_opt_str(p, r);
        }
        "sum" => sum_function(t, p, nargs),
        "date" | "time" | "duration" | "month-name" | "month-abbreviation" | "day-name"
        | "day-abbreviation" => {
            let Ok(arg) = opt_string_arg(t, p, nargs) else {
                return true;
            };
            let a = arg.as_deref().map(str::as_bytes);
            let r = match name {
                "date" => date_fn(a),
                "time" => time_fn(a),
                "duration" => duration_fn(a),
                "month-name" => Some(month_name_fn(a, false).to_string()),
                "month-abbreviation" => Some(month_name_fn(a, true).to_string()),
                "day-name" => Some(day_name_fn(a, false).to_string()),
                _ => Some(day_name_fn(a, true).to_string()),
            };
            push_opt_str(p, r);
        }
        "leap-year" => {
            let Ok(arg) = opt_string_arg(t, p, nargs) else {
                return true;
            };
            p.push(leap_year_fn(arg.as_deref().map(str::as_bytes)));
        }
        "year"
        | "month-in-year"
        | "week-in-year"
        | "week-in-month"
        | "day-in-year"
        | "day-in-month"
        | "day-of-week-in-month"
        | "day-in-week"
        | "hour-in-day"
        | "minute-in-hour"
        | "second-in-minute"
        | "seconds" => {
            let Ok(arg) = opt_string_arg(t, p, nargs) else {
                return true;
            };
            let a = arg.as_deref().map(str::as_bytes);
            let r = match name {
                "year" => year_fn(a),
                "month-in-year" => month_in_year_fn(a),
                "week-in-year" => week_in_year_fn(a),
                "week-in-month" => week_in_month_fn(a),
                "day-in-year" => day_in_year_fn(a),
                "day-in-month" => day_in_month_fn(a),
                "day-of-week-in-month" => day_of_week_in_month_fn(a),
                "day-in-week" => day_in_week_fn(a),
                "hour-in-day" => hour_in_day_fn(a),
                "minute-in-hour" => minute_in_hour_fn(a),
                "second-in-minute" => second_in_minute_fn(a),
                _ => seconds_fn(a),
            };
            p.push(XObj::Num(r));
        }
        _ => return false,
    }
    true
}
