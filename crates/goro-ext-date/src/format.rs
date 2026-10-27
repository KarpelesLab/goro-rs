//! date()/DateTime::format(), DateInterval::format() and strftime() formatting.

use crate::timelib::dow::*;
use crate::timelib::*;

pub const MON_FULL: [&str; 12] = [
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
pub const MON_SHORT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
pub const DAY_FULL: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
pub const DAY_SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

fn english_suffix(n: i64) -> &'static str {
    if (10..=19).contains(&n) {
        return "th";
    }
    match n % 10 {
        1 => "st",
        2 => "nd",
        3 => "rd",
        _ => "th",
    }
}

pub fn full_day_name(y: i64, m: i64, d: i64) -> &'static str {
    let dow = day_of_week(y, m, d);
    if dow < 0 {
        "Unknown"
    } else {
        DAY_FULL[dow as usize]
    }
}

pub fn short_day_name(y: i64, m: i64, d: i64) -> &'static str {
    let dow = day_of_week(y, m, d);
    if dow < 0 {
        "Unknown"
    } else {
        DAY_SHORT[dow as usize]
    }
}

fn mon_idx(m: i64) -> usize {
    (m - 1).clamp(0, 11) as usize
}

/// Compute the offset info date_format() uses for local time.
pub fn local_offset(t: &Time) -> TimeOffset {
    match t.zone_type {
        ZONETYPE_ABBR => TimeOffset {
            offset: (t.z + t.dst as i64 * 3600) as i32,
            leap_secs: 0,
            is_dst: t.dst as u32,
            abbr: t.tz_abbr.clone().unwrap_or_default(),
            transition_time: 0,
        },
        ZONETYPE_OFFSET => {
            let off = t.z as i32;
            TimeOffset {
                offset: off,
                leap_secs: 0,
                is_dst: 0,
                abbr: format!(
                    "GMT{}{:02}{:02}",
                    if off < 0 { '-' } else { '+' },
                    (off / 3600).abs(),
                    ((off % 3600) / 60).abs()
                ),
                transition_time: 0,
            }
        }
        ZONETYPE_ID => match &t.tz_info {
            Some(tz) => tz.get_time_zone_info(t.sse),
            None => TimeOffset::default(),
        },
        _ => TimeOffset::default(),
    }
}

/// php_date.c date_format()
/// date_format() of "x-m-d H:i:s.u" (the `date` property of every
/// DateTime) for a year of 0..=9999 and fields in range: what the general
/// loop makes, without its formatting machinery. None otherwise.
#[inline(never)]
fn date_prop_format(t: &Time) -> Option<Vec<u8>> {
    let two = |v: i64| (0..100).contains(&v).then_some(v as u8);
    let (m, d, h, i, s) = (two(t.m)?, two(t.d)?, two(t.h)?, two(t.i)?, two(t.s)?);
    if !(0..=9999).contains(&t.y) || !(0..1_000_000).contains(&t.us) {
        return None;
    }
    let mut out = Vec::with_capacity(26);
    let y = t.y as u32;
    out.extend_from_slice(&[
        b'0' + (y / 1000) as u8,
        b'0' + (y / 100 % 10) as u8,
        b'0' + (y / 10 % 10) as u8,
        b'0' + (y % 10) as u8,
    ]);
    for (sep, v) in [(b'-', m), (b'-', d), (b' ', h), (b':', i), (b':', s)] {
        out.extend_from_slice(&[sep, b'0' + v / 10, b'0' + v % 10]);
    }
    out.push(b'.');
    let us = t.us as u32;
    let mut div = 100_000;
    while div > 0 {
        out.push(b'0' + (us / div % 10) as u8);
        div /= 10;
    }
    Some(out)
}

pub fn date_format(format: &[u8], t: &Time, localtime: bool) -> Vec<u8> {
    if format == b"x-m-d H:i:s.u" {
        if let Some(s) = date_prop_format(t) {
            return s;
        }
    }
    let mut out: Vec<u8> = Vec::new();
    if format.is_empty() {
        return out;
    }
    let offset = if localtime {
        local_offset(t)
    } else {
        TimeOffset::default()
    };
    let mut iso: Option<(i64, i64)> = None;
    let mut i = 0;
    while i < format.len() {
        let c = format[i];
        let mut rfc_colon = false;
        let s: String = match c {
            b'd' => format!("{:02}", t.d as i32),
            b'D' => short_day_name(t.y, t.m, t.d).to_string(),
            b'j' => format!("{}", t.d as i32),
            b'l' => full_day_name(t.y, t.m, t.d).to_string(),
            b'S' => english_suffix(t.d).to_string(),
            b'w' => format!("{}", day_of_week(t.y, t.m, t.d) as i32),
            b'N' => format!("{}", iso_day_of_week(t.y, t.m, t.d) as i32),
            b'z' => format!("{}", day_of_year(t.y, t.m, t.d) as i32),
            b'W' => {
                let (w, _) = *iso.get_or_insert_with(|| isoweek_from_date(t.y, t.m, t.d));
                format!("{:02}", w as i32)
            }
            b'o' => {
                let (_, y) = *iso.get_or_insert_with(|| isoweek_from_date(t.y, t.m, t.d));
                format!("{}", y)
            }
            b'F' => MON_FULL[mon_idx(t.m)].to_string(),
            b'm' => format!("{:02}", t.m as i32),
            b'M' => MON_SHORT[mon_idx(t.m)].to_string(),
            b'n' => format!("{}", t.m as i32),
            b't' => format!("{}", days_in_month(t.y, t.m) as i32),
            b'L' => format!("{}", is_leap(t.y as i32 as i64) as i32),
            b'y' => format!("{:02}", (t.y % 100) as i32),
            b'Y' => format!(
                "{}{:04}",
                if t.y < 0 { "-" } else { "" },
                t.y.wrapping_abs() /* php_date_llabs() */
            ),
            b'x' => format!(
                "{}{:04}",
                if t.y < 0 {
                    "-"
                } else if t.y >= 10000 {
                    "+"
                } else {
                    ""
                },
                t.y.wrapping_abs() /* php_date_llabs() */
            ),
            b'X' => format!(
                "{}{:04}",
                if t.y < 0 { "-" } else { "+" },
                t.y.wrapping_abs() /* php_date_llabs() */
            ),
            b'a' => (if t.h >= 12 { "pm" } else { "am" }).to_string(),
            b'A' => (if t.h >= 12 { "PM" } else { "AM" }).to_string(),
            b'B' => {
                let sse = t.sse;
                let mut retval = ((sse - (sse - ((sse % 86400) + 3600))) * 10) as i32;
                if retval < 0 {
                    retval += 864000;
                }
                retval = (retval / 864) % 1000;
                format!("{:03}", retval)
            }
            b'g' => format!("{}", if t.h % 12 != 0 { (t.h as i32) % 12 } else { 12 }),
            b'G' => format!("{}", t.h as i32),
            b'h' => format!("{:02}", if t.h % 12 != 0 { (t.h as i32) % 12 } else { 12 }),
            b'H' => format!("{:02}", t.h as i32),
            b'i' => format!("{:02}", t.i as i32),
            b's' => format!("{:02}", t.s as i32),
            b'u' => format!("{:06}", t.us as i32),
            b'v' => format!("{:03}", (t.us / 1000) as i32),
            b'I' => format!("{}", if localtime { offset.is_dst } else { 0 }),
            b'p' | b'P' | b'O' => {
                if c == b'p'
                    && (!localtime
                        || offset.abbr == "UTC"
                        || offset.abbr == "Z"
                        || offset.abbr == "GMT+0000")
                {
                    "Z".to_string()
                } else {
                    if c != b'O' {
                        rfc_colon = true;
                    }
                    let o = offset.offset;
                    format!(
                        "{}{:02}{}{:02}",
                        if localtime {
                            if o < 0 { '-' } else { '+' }
                        } else {
                            '+'
                        },
                        if localtime { (o / 3600).abs() } else { 0 },
                        if rfc_colon { ":" } else { "" },
                        if localtime {
                            ((o % 3600) / 60).abs()
                        } else {
                            0
                        }
                    )
                }
            }
            b'T' => {
                if localtime {
                    offset.abbr.clone()
                } else {
                    "GMT".to_string()
                }
            }
            b'e' => {
                if !localtime {
                    "UTC".to_string()
                } else {
                    match t.zone_type {
                        ZONETYPE_ID => t
                            .tz_info
                            .as_ref()
                            .map(|z| z.name.clone())
                            .unwrap_or_default(),
                        ZONETYPE_ABBR => offset.abbr.clone(),
                        ZONETYPE_OFFSET => {
                            let o = offset.offset;
                            let seconds = o % 60;
                            if seconds == 0 {
                                format!(
                                    "{}{:02}:{:02}",
                                    if o < 0 { '-' } else { '+' },
                                    (o / 3600).abs(),
                                    ((o % 3600) / 60).abs()
                                )
                            } else {
                                format!(
                                    "{}{:02}:{:02}:{:02}",
                                    if o < 0 { '-' } else { '+' },
                                    (o / 3600).abs(),
                                    ((o % 3600) / 60).abs(),
                                    seconds.abs()
                                )
                            }
                        }
                        _ => String::new(),
                    }
                }
            }
            b'Z' => format!("{}", if localtime { offset.offset } else { 0 }),
            b'c' => {
                let o = offset.offset;
                format!(
                    "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
                    t.y,
                    t.m as i32,
                    t.d as i32,
                    t.h as i32,
                    t.i as i32,
                    t.s as i32,
                    if localtime {
                        if o < 0 { '-' } else { '+' }
                    } else {
                        '+'
                    },
                    if localtime { (o / 3600).abs() } else { 0 },
                    if localtime {
                        ((o % 3600) / 60).abs()
                    } else {
                        0
                    }
                )
            }
            b'r' => {
                let o = offset.offset;
                format!(
                    "{:>3}, {:02} {:>3} {:04} {:02}:{:02}:{:02} {}{:02}{:02}",
                    short_day_name(t.y, t.m, t.d),
                    t.d as i32,
                    MON_SHORT[mon_idx(t.m)],
                    t.y,
                    t.h as i32,
                    t.i as i32,
                    t.s as i32,
                    if localtime {
                        if o < 0 { '-' } else { '+' }
                    } else {
                        '+'
                    },
                    if localtime { (o / 3600).abs() } else { 0 },
                    if localtime {
                        ((o % 3600) / 60).abs()
                    } else {
                        0
                    }
                )
            }
            b'U' => format!("{}", t.sse),
            b'\\' => {
                if i < format.len() {
                    i += 1;
                }
                if i < format.len() {
                    out.push(format[i]);
                } else {
                    // C reads the NUL terminator
                    out.push(0);
                }
                i += 1;
                continue;
            }
            other => {
                out.push(other);
                i += 1;
                continue;
            }
        };
        out.extend_from_slice(s.as_bytes());
        i += 1;
    }
    out
}

/// php_date.c date_interval_format()
pub fn interval_format(format: &[u8], t: &RelTime) -> Vec<u8> {
    let mut out = Vec::new();
    let mut have_spec = false;
    for &c in format {
        if have_spec {
            let s = match c {
                b'Y' => format!("{:02}", t.y as i32),
                b'y' => format!("{}", t.y as i32),
                b'M' => format!("{:02}", t.m as i32),
                b'm' => format!("{}", t.m as i32),
                b'D' => format!("{:02}", t.d as i32),
                b'd' => format!("{}", t.d as i32),
                b'H' => format!("{:02}", t.h as i32),
                b'h' => format!("{}", t.h as i32),
                b'I' => format!("{:02}", t.i as i32),
                b'i' => format!("{}", t.i as i32),
                b'S' => format!("{:02}", t.s),
                b's' => format!("{}", t.s),
                b'F' => format!("{:06}", t.us),
                b'f' => format!("{}", t.us),
                b'a' => {
                    if t.days as i32 as i64 != UNSET {
                        format!("{}", t.days as i32)
                    } else {
                        "(unknown)".to_string()
                    }
                }
                b'r' => (if t.invert != 0 { "-" } else { "" }).to_string(),
                b'R' => (if t.invert != 0 { "-" } else { "+" }).to_string(),
                b'%' => "%".to_string(),
                other => {
                    out.push(b'%');
                    out.push(other);
                    have_spec = false;
                    continue;
                }
            };
            out.extend_from_slice(s.as_bytes());
            have_spec = false;
        } else if c == b'%' {
            have_spec = true;
        } else {
            out.push(c);
        }
    }
    out
}

/// A struct tm equivalent for strftime.
pub struct Tm {
    pub sec: i64,
    pub min: i64,
    pub hour: i64,
    pub mday: i64,
    pub mon: i64,
    pub year: i64,
    pub wday: i64,
    pub yday: i64,
    pub isdst: i64,
    pub gmtoff: i64,
    pub zone: String,
}

fn iso_week_based(tm: &Tm) -> (i64, i64) {
    // glibc iso_week_days
    fn iso_week_days(yday: i64, wday: i64) -> i64 {
        let big_enough_multiple_of_7 = (366 / 7 + 2) * 7;
        yday - (yday - wday + 4 + big_enough_multiple_of_7) % 7 + 3
    }
    let mut year = tm.year + 1900;
    let mut days = iso_week_days(tm.yday, tm.wday);
    if days < 0 {
        year -= 1;
        let ylen = if is_leap(year) { 366 } else { 365 };
        days = iso_week_days(tm.yday + ylen, tm.wday);
    } else {
        let ylen = if is_leap(year) { 366 } else { 365 };
        let d = iso_week_days(tm.yday - ylen, tm.wday);
        if d >= 0 {
            year += 1;
            days = d;
        }
    }
    (year, days / 7 + 1)
}

/// glibc-compatible strftime in the C locale (a port of glibc's
/// __strftime_internal: flags `_ - 0 ^ #`, a field width, `E`/`O`
/// modifiers falling back to the C locale's representations).
///
/// `maxsize` is the size of the buffer: None when the result (and its NUL
/// terminator) does not fit, as strftime() returning 0.
pub fn strftime(format: &[u8], tm: &Tm, maxsize: usize) -> Option<Vec<u8>> {
    // strftime() sees a C string
    let format = match format.iter().position(|&b| b == 0) {
        Some(n) => &format[..n],
        None => format,
    };
    let mut out = Vec::new();
    strftime_internal(&mut out, maxsize, format, tm, false)?;
    Some(out)
}

/// mktime() of a copy of the broken-down time (the C library interprets it
/// in the TZ environment's zone, not PHP's).
fn c_mktime(tm: &Tm) -> i64 {
    // SAFETY: a zeroed struct tm is valid; mktime() only reads/normalises
    // the struct it is given.
    unsafe {
        let mut t: libc::tm = std::mem::zeroed();
        t.tm_sec = tm.sec as libc::c_int;
        t.tm_min = tm.min as libc::c_int;
        t.tm_hour = tm.hour as libc::c_int;
        t.tm_mday = tm.mday as libc::c_int;
        t.tm_mon = tm.mon as libc::c_int;
        t.tm_year = tm.year as libc::c_int;
        t.tm_wday = tm.wday as libc::c_int;
        t.tm_yday = tm.yday as libc::c_int;
        t.tm_isdst = tm.isdst as libc::c_int;
        libc::mktime(&mut t) as i64
    }
}

/// glibc's add(): `n` bytes produced by `f`, right-aligned in `width`
/// (padded with zeros under the `0` flag, spaces otherwise).
fn strftime_add(
    out: &mut Vec<u8>,
    max: usize,
    width: i64,
    pad: u8,
    n: usize,
    f: impl FnOnce(&mut Vec<u8>),
) -> Option<()> {
    let delta = width - n as i64;
    let incr = n as i64 + delta.max(0);
    if incr as u64 >= max.saturating_sub(out.len()) as u64 {
        return None;
    }
    if delta > 0 {
        let c = if pad == b'0' { b'0' } else { b' ' };
        out.extend(std::iter::repeat_n(c, delta as usize));
    }
    f(out);
    Some(())
}

/// glibc's cpy(): add() of a text, with the case conversions.
fn strftime_cpy(
    out: &mut Vec<u8>,
    max: usize,
    s: &[u8],
    width: i64,
    pad: u8,
    low: bool,
    up: bool,
) -> Option<()> {
    strftime_add(out, max, width, pad, s.len(), |o| {
        if low {
            o.extend(s.iter().map(|c| c.to_ascii_lowercase()));
        } else if up {
            o.extend(s.iter().map(|c| c.to_ascii_uppercase()));
        } else {
            o.extend_from_slice(s);
        }
    })
}

enum StrftimeAct {
    Text(Vec<u8>),
    Sub(&'static [u8]),
    /// DO_NUMBER(digits, value)
    Num(i64, i32),
    /// DO_NUMBER_SPACEPAD(digits, value)
    NumSpace(i64, i32),
    Bad,
}

fn strftime_internal(
    out: &mut Vec<u8>,
    max: usize,
    format: &[u8],
    tm: &Tm,
    upcase: bool,
) -> Option<()> {
    use StrftimeAct::*;
    let hour12 = match tm.hour {
        h if h > 12 => h - 12,
        0 => 12,
        h => h,
    };
    // tm_year + TM_YEAR_BASE, as a C int
    let year = (tm.year + 1900) as i32;
    let at = |i: usize| -> u8 { format.get(i).copied().unwrap_or(0) };
    let mut f = 0usize;
    while f < format.len() {
        if format[f] != b'%' {
            strftime_add(out, max, -1, 0, 1, |_| ())?;
            out.push(format[f]);
            f += 1;
            continue;
        }
        let pct = f;
        let mut pad = 0u8;
        let mut width: i64 = -1;
        let mut to_lowcase = false;
        let mut to_uppcase = upcase;
        let mut change_case = false;
        // flags
        loop {
            f += 1;
            match at(f) {
                b'_' | b'-' | b'0' => pad = at(f),
                b'^' => to_uppcase = true,
                b'#' => change_case = true,
                _ => break,
            }
        }
        // field width (a GNU extension)
        if at(f).is_ascii_digit() {
            width = 0;
            while at(f).is_ascii_digit() {
                width = (width * 10 + (at(f) - b'0') as i64).min(i32::MAX as i64);
                f += 1;
            }
        }
        let modifier = match at(f) {
            c @ (b'E' | b'O') => {
                f += 1;
                c
            }
            _ => 0,
        };
        let fc = at(f);
        let wday = tm.wday.rem_euclid(7) as usize;
        let mon = tm.mon.clamp(0, 11) as usize;
        let act = match fc {
            b'%' => Text(b"%".to_vec()),
            b'a' | b'A' if modifier == 0 => {
                if change_case {
                    to_uppcase = true;
                    to_lowcase = false;
                }
                Text(
                    if fc == b'a' {
                        DAY_SHORT[wday]
                    } else {
                        DAY_FULL[wday]
                    }
                    .into(),
                )
            }
            b'b' | b'h' | b'B' => {
                if change_case {
                    to_uppcase = true;
                    to_lowcase = false;
                }
                if modifier == b'E' {
                    Bad
                } else {
                    Text(
                        if fc == b'B' {
                            MON_FULL[mon]
                        } else {
                            MON_SHORT[mon]
                        }
                        .into(),
                    )
                }
            }
            b'c' if modifier != b'O' => Sub(b"%a %b %e %H:%M:%S %Y"),
            b'C' => Num(1, year / 100 - (year % 100 < 0) as i32),
            b'x' if modifier != b'O' => Sub(b"%m/%d/%y"),
            b'D' if modifier == 0 => Sub(b"%m/%d/%y"),
            b'd' if modifier != b'E' => Num(2, tm.mday as i32),
            b'e' if modifier != b'E' => NumSpace(2, tm.mday as i32),
            b'F' if modifier == 0 => Sub(b"%Y-%m-%d"),
            b'H' if modifier != b'E' => Num(2, tm.hour as i32),
            b'I' if modifier != b'E' => Num(2, hour12 as i32),
            b'k' if modifier != b'E' => NumSpace(2, tm.hour as i32),
            b'l' if modifier != b'E' => NumSpace(2, hour12 as i32),
            b'j' if modifier != b'E' => Num(3, (tm.yday + 1) as i32),
            b'M' if modifier != b'E' => Num(2, tm.min as i32),
            b'm' if modifier != b'E' => Num(2, (tm.mon + 1) as i32),
            b'n' => Text(b"\n".to_vec()),
            b't' => Text(b"\t".to_vec()),
            b'P' | b'p' => {
                if fc == b'P' {
                    to_lowcase = true;
                }
                if change_case {
                    to_uppcase = false;
                    to_lowcase = true;
                }
                Text(if tm.hour >= 12 { b"PM" } else { b"AM" }.to_vec())
            }
            b'R' => Sub(b"%H:%M"),
            b'r' => Sub(b"%I:%M:%S %p"),
            b'S' if modifier != b'E' => Num(2, tm.sec as i32),
            // mktime() of the fields, in the process's time zone (digits =
            // 1: only the width pads it)
            b's' => Text(c_mktime(tm).to_string().into_bytes()),
            b'X' if modifier != b'O' => Sub(b"%H:%M:%S"),
            b'T' => Sub(b"%H:%M:%S"),
            b'u' => Num(1, ((tm.wday - 1 + 7) % 7 + 1) as i32),
            b'U' if modifier != b'E' => Num(2, ((tm.yday - tm.wday + 7) / 7) as i32),
            b'V' | b'g' | b'G' if modifier != b'E' => {
                let (y, w) = iso_week_based(tm);
                let y = y as i32;
                match fc {
                    b'g' => Num(2, (y % 100 + 100) % 100),
                    b'G' => Num(1, y),
                    _ => Num(2, w as i32),
                }
            }
            b'W' if modifier != b'E' => Num(2, ((tm.yday - (tm.wday - 1 + 7) % 7 + 7) / 7) as i32),
            b'w' if modifier != b'E' => Num(1, tm.wday as i32),
            b'Y' if modifier != b'O' => Num(1, year),
            b'y' => Num(2, (year % 100 + 100) % 100),
            b'Z' => {
                if change_case {
                    to_uppcase = false;
                    to_lowcase = true;
                }
                Text(tm.zone.as_bytes().to_vec())
            }
            b'z' => {
                let mut diff = tm.gmtoff as i32;
                strftime_add(out, max, width, pad, 1, |o| {
                    o.push(if diff < 0 { b'-' } else { b'+' })
                })?;
                diff = diff.wrapping_abs() / 60;
                Num(4, (diff / 60) * 100 + diff % 60)
            }
            0 => {
                // a '%' at the end of the format
                f -= 1;
                Bad
            }
            _ => Bad,
        };
        match act {
            Text(t) => strftime_cpy(out, max, &t, width, pad, to_lowcase, to_uppcase)?,
            Sub(sub) => {
                let mut inner = Vec::new();
                strftime_internal(&mut inner, usize::MAX, sub, tm, to_uppcase)?;
                let old = out.len();
                strftime_add(out, max, width, pad, inner.len(), |o| {
                    o.extend_from_slice(&inner)
                })?;
                if to_uppcase {
                    out[old..].make_ascii_uppercase();
                }
            }
            Num(d, v) | NumSpace(d, v) => {
                let mut pad = pad;
                if matches!(act, NumSpace(..)) && pad != b'0' && pad != b'-' {
                    pad = b'_';
                }
                let digits = d.max(width);
                let negative = v < 0;
                let mut buf = (v as i64).unsigned_abs().to_string().into_bytes();
                if negative {
                    buf.insert(0, b'-');
                }
                let mut body: &[u8] = &buf;
                if pad != b'-' {
                    let padding = digits - buf.len() as i64;
                    if padding > 0 {
                        let room = max.saturating_sub(out.len()) as u64;
                        if (if pad == b'_' { padding } else { digits }) as u64 >= room {
                            return None;
                        }
                        if pad == b'_' {
                            out.extend(std::iter::repeat_n(b' ', padding as usize));
                        } else {
                            if negative {
                                body = &buf[1..];
                                out.push(b'-');
                            }
                            out.extend(std::iter::repeat_n(b'0', padding as usize));
                        }
                        // (the sign written ahead counts too)
                        let used = padding + (buf.len() - body.len()) as i64;
                        width = (width - used).max(0);
                    }
                }
                strftime_cpy(out, max, body, width, pad, to_lowcase, to_uppcase)?;
            }
            // Unknown conversion: the whole directive, as written.
            Bad => strftime_cpy(
                out,
                max,
                &format[pct..=f],
                width,
                pad,
                to_lowcase,
                to_uppcase,
            )?,
        }
        f += 1;
    }
    Some(())
}
