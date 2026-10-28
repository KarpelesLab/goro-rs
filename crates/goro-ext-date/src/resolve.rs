//! Turning a parsed string into an instant: `strtotime()`, the DateTime
//! constructor and `modify()`.
//!
//! PHP manual ("Relative Formats" notes): relative statements are applied
//! after the absolute ones; a day name moves to the next such day unless
//! today is that day; "first/last day of" set the day after the month
//! arithmetic; units of days and above count on the wall clock, while hours
//! and below count elapsed time. The finer points (which base fields fill
//! which holes, the week rules) were checked against PHP 8.5.

use crate::civil;
use crate::parse::{Parsed, ParsedZone, Special, WeekdayBehavior};
use crate::time::{DateTime, LocalTime};
use crate::tz::TimeZone;

/// Which operation is resolving the string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `strtotime()` and the DateTime constructor: a date without a time
    /// means midnight, and a zone in the string is used.
    Create,
    /// `DateTime::modify()`: the object's time and zone are kept (only an
    /// "@timestamp" switches the zone, observed on PHP 8.5).
    Modify,
}

/// Resolves `p` against the base instant (`now` for strtotime and the
/// constructor, the object for modify) in zone `zone`.
pub fn resolve(p: &Parsed, base_ts: i64, base_us: u32, zone: &TimeZone, mode: Mode) -> DateTime {
    let base = LocalTime::new(base_ts, base_us, zone);
    let target_zone = match (mode, &p.zone) {
        (Mode::Create, Some(ParsedZone::Zone(z))) => z.clone(),
        // modify() keeps its zone, except for what reads as an
        // "@timestamp": 1970-01-01 00:00:00 at a UTC offset (observed on
        // PHP 8.5: "@123" switches to +00:00, "@123 10:00" does not)
        (Mode::Modify, Some(ParsedZone::Zone(TimeZone::Offset(o))))
            if (p.y, p.m, p.d, p.h, p.i, p.s)
                == (Some(1970), Some(1), Some(1), Some(0), Some(0), Some(0)) =>
        {
            TimeZone::Offset(*o)
        }
        _ => zone.clone(),
    };

    let mut y = p.y.unwrap_or(base.y);
    let mut m = p.m.unwrap_or(base.m as i64);
    let mut d = p.d.unwrap_or(base.d as i64);
    let midnight = mode == Mode::Create && p.have_date && !p.have_time;
    let (mut h, mut i, mut s, mut us) = if midnight {
        (
            p.h.unwrap_or(0),
            p.i.unwrap_or(0),
            p.s.unwrap_or(0),
            p.us.unwrap_or(0),
        )
    } else {
        (
            p.h.unwrap_or(base.h as i64),
            p.i.unwrap_or(base.i as i64),
            p.s.unwrap_or(base.s as i64),
            p.us.unwrap_or(base.us as i64),
        )
    };

    if let Some(r) = &p.relative {
        let set = |days: i64, y: &mut i64, m: &mut i64, d: &mut i64| {
            let (ay, am, ad) = civil::civil_from_days(days);
            *y = ay;
            *m = am as i64;
            *d = ad as i64;
        };
        let month_of = matches!(
            r.special,
            Special::DayOfWeekInMonth | Special::LastDayOfWeekInMonth
        );
        if r.first_last_day_of != 0 {
            // "first/last day of" replace the day (relative days are
            // dropped); a weekday then moves from that day, and the day is
            // set again in the month finally reached, and "last X of"
            // counts from the next month (observed on PHP 8.5: "last day of
            // monday" from June 15th is July 31st)
            y = y.saturating_add(r.y);
            m = m.saturating_add(r.m);
            if r.special == Special::LastDayOfWeekInMonth {
                m += 1;
            }
            let fl = r.first_last_day_of;
            let pick = move |y: i64, m: i64| -> i64 {
                let (ny, nm) = norm_month(y, m);
                if fl == 1 {
                    1
                } else {
                    civil::days_in_month(ny, nm as u32) as i64
                }
            };
            d = pick(y, m);
            if let Some(w) = r.weekday {
                let behavior = if month_of {
                    WeekdayBehavior::Forward
                } else {
                    r.weekday_behavior
                };
                let days = resolve_weekday(civil::days_from_civil(y, m, d), w, behavior);
                set(days, &mut y, &mut m, &mut d);
                d = pick(y, m);
            }
            // a time past midnight moves to the next month's first/last
            // day ("last day of 2006.331 24:55" is February 28th)
            let local = civil::civil_to_ts(y, m, d, h, i, s);
            let c = civil::Civil::from_ts(local);
            if (c.y, c.m as i64) != norm_month(y, m) {
                y = c.y;
                m = c.m as i64;
                d = pick(y, m) + (c.d as i64 - 1).min(0);
                h = c.h as i64;
                i = c.i as i64;
                s = c.s as i64;
            }
        } else {
            if !month_of {
                // the absolute fields are normalised before the relative
                // part ("24:33 eight months" from February 29th is November
                // 1st), but "X of" uses the month as given ("last fri of
                // FEB 31, 2090" is in February; observed on PHP 8.5)
                let c = civil::Civil::from_ts(civil::civil_to_ts(y, m, d, h, i, s));
                y = c.y;
                m = c.m as i64;
                d = c.d as i64;
                h = c.h as i64;
                i = c.i as i64;
                s = c.s as i64;
            }
            if month_of {
                // the weekday on or after the 1st of the month (of the next
                // month for "last/this X of"), then the stored offset; the
                // relative years come after ("next sunday of, 2029 Aug,
                // previous years" is August 5th 2028, observed on PHP 8.5)
                m = m.saturating_add(r.m);
                let next = i64::from(r.special == Special::LastDayOfWeekInMonth);
                let (ny, nm) = norm_month(y, m + next);
                let first = civil::days_from_civil(ny, nm, 1);
                let w = r.weekday.unwrap_or(0);
                let days = resolve_weekday(first, w, r.weekday_behavior) + r.d;
                set(days, &mut y, &mut m, &mut d);
                y = y.saturating_add(r.y);
            } else {
                // a relative weekday is resolved on the date before the
                // relative part is added ("+1 day next monday" is a
                // Tuesday, observed on PHP 8.5)
                if let Some(w) = r.weekday {
                    let days =
                        resolve_weekday(civil::days_from_civil(y, m, d), w, r.weekday_behavior);
                    set(days, &mut y, &mut m, &mut d);
                }
                y = y.saturating_add(r.y);
                m = m.saturating_add(r.m);
                d = d.saturating_add(r.d);
            }
        }
        if let Special::Weekdays(n) = r.special {
            let days = add_weekdays(civil::days_from_civil(y, m, d), n);
            set(days, &mut y, &mut m, &mut d);
        }
        // relative hours and below last, on the wall clock ("2004-04-07
        // +1 hour" in Jerusalem lands in the gap; test bug33415-2.phpt)
        h = h.saturating_add(r.h);
        i = i.saturating_add(r.i);
        s = s.saturating_add(r.s);
        us = us.saturating_add(r.us);
    }

    let extra_s = us.div_euclid(1_000_000);
    let us = us.rem_euclid(1_000_000) as u32;
    let local = civil::civil_to_ts(y, m, d, h, i, s.saturating_add(extra_s));
    let ts = match mode {
        Mode::Create => target_zone.local_to_ts(local),
        Mode::Modify => target_zone.local_to_ts_with_dst(local, base.is_dst),
    };
    DateTime::from_ts(ts, us, target_zone)
}

fn norm_month(y: i64, m: i64) -> (i64, i64) {
    let m0 = m - 1;
    (y.saturating_add(m0.div_euclid(12)), m0.rem_euclid(12) + 1)
}

/// Moves `days` to weekday `w` (0 = Sunday; negative after "ago").
fn resolve_weekday(days: i64, w: i64, behavior: WeekdayBehavior) -> i64 {
    let cur = civil::weekday(days) as i64;
    match behavior {
        WeekdayBehavior::Week => {
            // within the ISO week (Monday first, Sunday last)
            let cur_iso = if cur == 0 { 7 } else { cur };
            let w_iso = if w == 0 { 7 } else { w };
            days - cur_iso + w_iso
        }
        _ if w < 0 => {
            // "monday ago" (weekday -1, Sunday is -7): the Monday of the
            // week before (observed on PHP 8.5 with "next monday ago" from
            // a Tuesday, a Saturday and a Sunday)
            days + (-w - cur) - 7
        }
        WeekdayBehavior::Strict => {
            let diff = (w - cur).rem_euclid(7);
            days + if diff == 0 { 7 } else { diff }
        }
        WeekdayBehavior::Forward => days + (w - cur).rem_euclid(7),
    }
}

/// Adds `n` business days (Monday to Friday). Zero moves a weekend day
/// to the Monday ("this weekday" on a Saturday, observed on PHP 8.5).
fn add_weekdays(days: i64, n: i64) -> i64 {
    let mut d = days;
    if n == 0 {
        return match civil::weekday(d) {
            6 => d + 2,
            0 => d + 1,
            _ => d,
        };
    }
    let step = if n > 0 { 1 } else { -1 };
    let mut left = n.abs();
    while left > 0 {
        d += step;
        let w = civil::weekday(d);
        if w != 0 && w != 6 {
            left -= 1;
        }
    }
    d
}

/// `strtotime()`: the Unix time, or `None` (PHP's `false`) when the string
/// has errors.
pub fn strtotime(input: &[u8], now: i64, zone: &TimeZone) -> Option<i64> {
    let p = crate::parse::parse(input);
    if !p.errors.is_empty() {
        return None;
    }
    Some(resolve(&p, now, 0, zone, Mode::Create).ts)
}

/// The message PHP gives for the first parse error: "Failed to parse time
/// string (STR) at position N (C): MESSAGE" (observed on PHP 8.5).
pub fn parse_error_message(input: &[u8], p: &Parsed) -> Option<String> {
    let (pos, msg) = p.errors.first()?;
    let trimmed = trim_ws(input);
    // past the end (the empty string) PHP prints a space
    let c = trimmed.get(*pos).copied().unwrap_or(b' ');
    let mut out = String::from("Failed to parse time string (");
    out.push_str(&String::from_utf8_lossy(input));
    out.push_str(&format!(") at position {pos} ("));
    out.push_str(&String::from_utf8_lossy(&[c]));
    out.push_str(&format!("): {msg}"));
    Some(out)
}

fn trim_ws(s: &[u8]) -> &[u8] {
    let s = match s.iter().position(|&c| c == 0) {
        Some(n) => &s[..n],
        None => s,
    };
    let ws = |c: &u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    let a = s.iter().position(|c| !ws(c)).unwrap_or(s.len());
    &s[a..]
}

/// `DateTime::modify()` on (`ts`, `us`, `zone`): the new value, or the
/// parse error message (without the "DateTime::modify(): " prefix).
pub fn modify(input: &[u8], ts: i64, us: u32, zone: &TimeZone) -> Result<DateTime, String> {
    let p = crate::parse::parse(input);
    if let Some(msg) = parse_error_message(input, &p) {
        return Err(msg);
    }
    Ok(resolve(&p, ts, us, zone, Mode::Modify))
}
