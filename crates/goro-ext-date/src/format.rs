//! `date()` / `DateTimeInterface::format()`.
//!
//! PHP manual: DateTimeInterface::format, its table of format characters
//! and the DATE_* constants. Corner cases (negative and large years, the
//! zone characters for each zone kind) observed on PHP 8.5.

use crate::civil;
use crate::time::LocalTime;
use crate::tz::TimeZone;

/// English day names, Sunday first.
pub const DAY_NAMES: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// English month names, January first.
pub const MONTH_NAMES: [&str; 12] = [
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

/// The `DATE_*` / `DateTimeInterface::*` format constants.
pub mod consts {
    /// DATE_ATOM
    pub const ATOM: &str = "Y-m-d\\TH:i:sP";
    /// DATE_COOKIE
    pub const COOKIE: &str = "l, d-M-Y H:i:s T";
    /// DATE_ISO8601
    pub const ISO8601: &str = "Y-m-d\\TH:i:sO";
    /// DATE_ISO8601_EXPANDED
    pub const ISO8601_EXPANDED: &str = "X-m-d\\TH:i:sP";
    /// DATE_RFC822
    pub const RFC822: &str = "D, d M y H:i:s O";
    /// DATE_RFC850
    pub const RFC850: &str = "l, d-M-y H:i:s T";
    /// DATE_RFC1036
    pub const RFC1036: &str = "D, d M y H:i:s O";
    /// DATE_RFC1123
    pub const RFC1123: &str = "D, d M Y H:i:s O";
    /// DATE_RFC7231 (deprecated in PHP 8.5)
    pub const RFC7231: &str = "D, d M Y H:i:s \\G\\M\\T";
    /// DATE_RFC2822
    pub const RFC2822: &str = "D, d M Y H:i:s O";
    /// DATE_RFC3339
    pub const RFC3339: &str = "Y-m-d\\TH:i:sP";
    /// DATE_RFC3339_EXTENDED
    pub const RFC3339_EXTENDED: &str = "Y-m-d\\TH:i:s.vP";
    /// DATE_RSS
    pub const RSS: &str = "D, d M Y H:i:s O";
    /// DATE_W3C
    pub const W3C: &str = "Y-m-d\\TH:i:sP";
}

/// English ordinal suffix for a day of the month (`S`).
pub fn ordinal_suffix(d: u32) -> &'static str {
    match d {
        1 | 21 | 31 => "st",
        2 | 22 => "nd",
        3 | 23 => "rd",
        _ => "th",
    }
}

fn push_num(out: &mut Vec<u8>, v: i64, width: usize) {
    use std::io::Write;
    let _ = write!(out, "{v:0width$}");
}

/// C's `%04lld`: the sign counts towards the width ("-001").
fn push_c_pad4(out: &mut Vec<u8>, y: i64) {
    use std::io::Write;
    if y < 0 {
        let _ = write!(out, "-{:03}", y.unsigned_abs());
    } else {
        let _ = write!(out, "{y:04}");
    }
}

/// "Y": at least four digits, "-" for years before year 0.
fn push_year(out: &mut Vec<u8>, y: i64) {
    use std::io::Write;
    if y < 0 {
        let _ = write!(out, "-{:04}", y.unsigned_abs());
    } else {
        let _ = write!(out, "{y:04}");
    }
}

/// "+hh:mm" (with_colon) or "+hhmm"; seconds are dropped.
pub fn push_offset(out: &mut Vec<u8>, offset: i32, with_colon: bool) {
    use std::io::Write;
    let sign = if offset < 0 { '-' } else { '+' };
    let a = offset.unsigned_abs();
    let (h, m) = (a / 3600, a / 60 % 60);
    if with_colon {
        let _ = write!(out, "{sign}{h:02}:{m:02}");
    } else {
        let _ = write!(out, "{sign}{h:02}{m:02}");
    }
}

/// Formats `t` (in zone `zone`) with a PHP date format string.
pub fn format(fmt: &[u8], t: &LocalTime, zone: &TimeZone) -> Vec<u8> {
    let mut out = Vec::with_capacity(fmt.len() * 3);
    format_into(&mut out, fmt, t, zone);
    out
}

/// Like [`format`], appending to `out`.
pub fn format_into(out: &mut Vec<u8>, fmt: &[u8], t: &LocalTime, zone: &TimeZone) {
    use std::io::Write;
    let mut i = 0;
    while i < fmt.len() {
        let c = fmt[i];
        i += 1;
        match c {
            // day
            b'd' => push_num(out, t.d as i64, 2),
            b'D' => out.extend_from_slice(&DAY_NAMES[t.weekday() as usize].as_bytes()[..3]),
            b'j' => push_num(out, t.d as i64, 0),
            b'l' => out.extend_from_slice(DAY_NAMES[t.weekday() as usize].as_bytes()),
            b'N' => {
                let w = t.weekday();
                push_num(out, if w == 0 { 7 } else { w as i64 }, 0)
            }
            b'S' => out.extend_from_slice(ordinal_suffix(t.d).as_bytes()),
            b'w' => push_num(out, t.weekday() as i64, 0),
            b'z' => push_num(out, civil::day_of_year(t.y, t.m, t.d) as i64, 0),
            // week
            b'W' => push_num(out, civil::iso_week_date(t.y, t.m, t.d).1 as i64, 2),
            // month
            b'F' => out.extend_from_slice(MONTH_NAMES[t.m as usize - 1].as_bytes()),
            b'm' => push_num(out, t.m as i64, 2),
            b'M' => out.extend_from_slice(&MONTH_NAMES[t.m as usize - 1].as_bytes()[..3]),
            b'n' => push_num(out, t.m as i64, 0),
            b't' => push_num(out, civil::days_in_month(t.y, t.m) as i64, 0),
            // year
            b'L' => out.push(if civil::is_leap(t.y) { b'1' } else { b'0' }),
            b'o' => push_num(out, civil::iso_week_date(t.y, t.m, t.d).0, 0),
            b'X' => {
                let _ = if t.y < 0 {
                    write!(out, "-{:04}", t.y.unsigned_abs())
                } else {
                    write!(out, "+{:04}", t.y)
                };
            }
            b'x' => {
                let _ = if t.y < 0 {
                    write!(out, "-{:04}", t.y.unsigned_abs())
                } else if t.y >= 10_000 {
                    write!(out, "+{}", t.y)
                } else {
                    write!(out, "{:04}", t.y)
                };
            }
            b'Y' => push_year(out, t.y),
            b'y' => {
                // C's "%02d" of y % 100: -1 prints "-1"
                let v = t.y % 100;
                if v < 0 {
                    let _ = write!(out, "{v}");
                } else {
                    push_num(out, v, 2);
                }
            }
            // time
            b'a' => out.extend_from_slice(if t.h < 12 { b"am" } else { b"pm" }),
            b'A' => out.extend_from_slice(if t.h < 12 { b"AM" } else { b"PM" }),
            b'B' => {
                let secs = (t.ts.rem_euclid(86_400) + 3600) % 86_400;
                push_num(out, secs * 10 / 864, 3);
            }
            b'g' => push_num(
                out,
                if t.h.is_multiple_of(12) {
                    12
                } else {
                    (t.h % 12) as i64
                },
                0,
            ),
            b'G' => push_num(out, t.h as i64, 0),
            b'h' => push_num(
                out,
                if t.h.is_multiple_of(12) {
                    12
                } else {
                    (t.h % 12) as i64
                },
                2,
            ),
            b'H' => push_num(out, t.h as i64, 2),
            b'i' => push_num(out, t.i as i64, 2),
            b's' => push_num(out, t.s as i64, 2),
            b'u' => push_num(out, t.us as i64, 6),
            b'v' => push_num(out, (t.us / 1000) as i64, 3),
            // zone
            b'e' => match zone {
                TimeZone::Offset(o) => push_offset(out, *o, true),
                _ => out.extend_from_slice(zone.name().as_bytes()),
            },
            b'I' => out.push(if t.is_dst { b'1' } else { b'0' }),
            b'O' => push_offset(out, t.offset, false),
            b'P' => push_offset(out, t.offset, true),
            b'p' => {
                let z = t.offset == 0
                    && match zone {
                        TimeZone::Offset(_) => true,
                        _ => t.abbr.as_str() == "UTC" || t.abbr.as_str() == "Z",
                    };
                if z {
                    out.push(b'Z');
                } else {
                    push_offset(out, t.offset, true);
                }
            }
            b'T' => match zone {
                TimeZone::Offset(o) => {
                    out.extend_from_slice(b"GMT");
                    push_offset(out, *o, false);
                }
                _ => out.extend_from_slice(t.abbr.as_bytes()),
            },
            b'Z' => push_num(out, t.offset as i64, 0),
            // full date/time
            b'c' => {
                push_c_pad4(out, t.y);
                let _ = write!(
                    out,
                    "-{:02}-{:02}T{:02}:{:02}:{:02}",
                    t.m, t.d, t.h, t.i, t.s
                );
                push_offset(out, t.offset, true);
            }
            b'r' => {
                let wd = &DAY_NAMES[t.weekday() as usize][..3];
                let mn = &MONTH_NAMES[t.m as usize - 1][..3];
                let _ = write!(out, "{wd}, {:02} {mn} ", t.d);
                push_c_pad4(out, t.y);
                let _ = write!(out, " {:02}:{:02}:{:02} ", t.h, t.i, t.s);
                push_offset(out, t.offset, false);
            }
            b'U' => push_num(out, t.ts, 0),
            b'\\' => {
                if i < fmt.len() {
                    out.push(fmt[i]);
                    i += 1;
                } else {
                    // a trailing backslash prints a NUL byte (observed on
                    // PHP 8.5)
                    out.push(0);
                }
            }
            other => out.push(other),
        }
    }
}
