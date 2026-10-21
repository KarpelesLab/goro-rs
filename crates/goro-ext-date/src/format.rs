//! date()/DateTime::format(), DateInterval::format() and strftime() formatting.

use crate::timelib::dow::*;
use crate::timelib::*;

pub const MON_FULL: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
pub const MON_SHORT: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
pub const DAY_FULL: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
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
    if dow < 0 { "Unknown" } else { DAY_FULL[dow as usize] }
}

pub fn short_day_name(y: i64, m: i64, d: i64) -> &'static str {
    let dow = day_of_week(y, m, d);
    if dow < 0 { "Unknown" } else { DAY_SHORT[dow as usize] }
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
                abbr: format!("GMT{}{:02}{:02}", if off < 0 { '-' } else { '+' }, (off / 3600).abs(), ((off % 3600) / 60).abs()),
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
pub fn date_format(format: &[u8], t: &Time, localtime: bool) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    if format.is_empty() {
        return out;
    }
    let offset = if localtime { local_offset(t) } else { TimeOffset::default() };
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
            b'Y' => format!("{}{:04}", if t.y < 0 { "-" } else { "" }, t.y.unsigned_abs()),
            b'x' => format!("{}{:04}", if t.y < 0 { "-" } else if t.y >= 10000 { "+" } else { "" }, t.y.unsigned_abs()),
            b'X' => format!("{}{:04}", if t.y < 0 { "-" } else { "+" }, t.y.unsigned_abs()),
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
                if c == b'p' && (!localtime || offset.abbr == "UTC" || offset.abbr == "Z" || offset.abbr == "GMT+0000") {
                    "Z".to_string()
                } else {
                    if c != b'O' {
                        rfc_colon = true;
                    }
                    let o = offset.offset;
                    format!(
                        "{}{:02}{}{:02}",
                        if localtime { if o < 0 { '-' } else { '+' } } else { '+' },
                        if localtime { (o / 3600).abs() } else { 0 },
                        if rfc_colon { ":" } else { "" },
                        if localtime { ((o % 3600) / 60).abs() } else { 0 }
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
                        ZONETYPE_ID => t.tz_info.as_ref().map(|z| z.name.clone()).unwrap_or_default(),
                        ZONETYPE_ABBR => offset.abbr.clone(),
                        ZONETYPE_OFFSET => {
                            let o = offset.offset;
                            let seconds = o % 60;
                            if seconds == 0 {
                                format!("{}{:02}:{:02}", if o < 0 { '-' } else { '+' }, (o / 3600).abs(), ((o % 3600) / 60).abs())
                            } else {
                                format!("{}{:02}:{:02}:{:02}", if o < 0 { '-' } else { '+' }, (o / 3600).abs(), ((o % 3600) / 60).abs(), seconds.abs())
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
                    if localtime { if o < 0 { '-' } else { '+' } } else { '+' },
                    if localtime { (o / 3600).abs() } else { 0 },
                    if localtime { ((o % 3600) / 60).abs() } else { 0 }
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
                    if localtime { if o < 0 { '-' } else { '+' } } else { '+' },
                    if localtime { (o / 3600).abs() } else { 0 },
                    if localtime { ((o % 3600) / 60).abs() } else { 0 }
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
    #[allow(dead_code)]
    pub isdst: i64,
    pub gmtoff: i64,
    pub zone: String,
    pub ts: i64,
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

/// glibc-compatible strftime in the C locale
pub fn strftime(format: &[u8], tm: &Tm) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    let h12 = if tm.hour % 12 == 0 { 12 } else { tm.hour % 12 };
    while i < format.len() {
        let c = format[i];
        if c != b'%' {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        // flags / width (minimal support)
        let mut pad: Option<u8> = None;
        while i < format.len() && matches!(format[i], b'_' | b'-' | b'0' | b'^' | b'#') {
            pad = Some(format[i]);
            i += 1;
        }
        while i < format.len() && (format[i] == b'E' || format[i] == b'O') {
            i += 1;
        }
        if i >= format.len() {
            out.push(b'%');
            break;
        }
        let spec = format[i];
        i += 1;
        let num = |v: i64, w: usize, default_pad: u8| -> String {
            let p = pad.unwrap_or(default_pad);
            match p {
                b'-' => format!("{}", v),
                b'_' | b' ' => format!("{:>w$}", v, w = w),
                _ => {
                    if v < 0 {
                        format!("-{:0>w$}", -v, w = w.saturating_sub(1))
                    } else {
                        format!("{:0>w$}", v, w = w)
                    }
                }
            }
        };
        let s: String = match spec {
            b'a' => DAY_SHORT[tm.wday.rem_euclid(7) as usize].to_string(),
            b'A' => DAY_FULL[tm.wday.rem_euclid(7) as usize].to_string(),
            b'b' | b'h' => MON_SHORT[tm.mon.clamp(0, 11) as usize].to_string(),
            b'B' => MON_FULL[tm.mon.clamp(0, 11) as usize].to_string(),
            b'c' => format!(
                "{} {} {:>2} {:02}:{:02}:{:02} {}",
                DAY_SHORT[tm.wday.rem_euclid(7) as usize],
                MON_SHORT[tm.mon.clamp(0, 11) as usize],
                tm.mday,
                tm.hour,
                tm.min,
                tm.sec,
                tm.year + 1900
            ),
            b'C' => num((tm.year + 1900).div_euclid(100), 2, b'0'),
            b'd' => num(tm.mday, 2, b'0'),
            b'D' => format!("{:02}/{:02}/{:02}", tm.mon + 1, tm.mday, (tm.year + 1900).rem_euclid(100)),
            b'e' => num(tm.mday, 2, b'_'),
            b'F' => format!("{}-{:02}-{:02}", num(tm.year + 1900, 1, b'0'), tm.mon + 1, tm.mday),
            b'g' => num(iso_week_based(tm).0.rem_euclid(100), 2, b'0'),
            b'G' => num(iso_week_based(tm).0, 1, b'0'),
            b'H' => num(tm.hour, 2, b'0'),
            b'I' => num(h12, 2, b'0'),
            b'j' => num(tm.yday + 1, 3, b'0'),
            b'k' => num(tm.hour, 2, b'_'),
            b'l' => num(h12, 2, b'_'),
            b'm' => num(tm.mon + 1, 2, b'0'),
            b'M' => num(tm.min, 2, b'0'),
            b'n' => "\n".to_string(),
            b'p' => (if tm.hour >= 12 { "PM" } else { "AM" }).to_string(),
            b'P' => (if tm.hour >= 12 { "pm" } else { "am" }).to_string(),
            b'r' => format!("{:02}:{:02}:{:02} {}", h12, tm.min, tm.sec, if tm.hour >= 12 { "PM" } else { "AM" }),
            b'R' => format!("{:02}:{:02}", tm.hour, tm.min),
            b's' => format!("{}", tm.ts),
            b'S' => num(tm.sec, 2, b'0'),
            b't' => "\t".to_string(),
            b'T' | b'X' => format!("{:02}:{:02}:{:02}", tm.hour, tm.min, tm.sec),
            b'u' => format!("{}", if tm.wday == 0 { 7 } else { tm.wday }),
            b'U' => num((tm.yday - tm.wday + 7) / 7, 2, b'0'),
            b'V' => num(iso_week_based(tm).1, 2, b'0'),
            b'w' => format!("{}", tm.wday),
            b'W' => num((tm.yday - (tm.wday - 1 + 7) % 7 + 7) / 7, 2, b'0'),
            b'x' => format!("{:02}/{:02}/{:02}", tm.mon + 1, tm.mday, (tm.year + 1900).rem_euclid(100)),
            b'y' => num((tm.year + 1900).rem_euclid(100), 2, b'0'),
            b'Y' => num(tm.year + 1900, 1, b'0'),
            b'z' => {
                let o = tm.gmtoff;
                let a = o.abs();
                format!("{}{:02}{:02}", if o < 0 { '-' } else { '+' }, a / 3600, (a % 3600) / 60)
            }
            b'Z' => tm.zone.clone(),
            b'%' => "%".to_string(),
            other => {
                out.push(b'%');
                out.push(other);
                continue;
            }
        };
        out.extend_from_slice(s.as_bytes());
    }
    out
}
