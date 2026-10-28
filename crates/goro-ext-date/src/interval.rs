//! DateInterval: ISO 8601 durations, `format()`, `diff()`, `add()`/`sub()`.
//!
//! PHP manual: DateInterval (the constructor's ISO 8601 duration format
//! and `format()`'s characters), DateTime::add/sub/diff. Details checked
//! against the ext/date tests (DateTime_{diff,days,add,sub}-*.phpt) and
//! observed on PHP 8.5.

use crate::civil;
use crate::parse::{self, Relative};
use crate::time::{DateTime, LocalTime};
use crate::tz::TimeZone;

/// A DateInterval.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Interval {
    /// Years.
    pub y: i64,
    /// Months.
    pub m: i64,
    /// Days.
    pub d: i64,
    /// Hours.
    pub h: i64,
    /// Minutes.
    pub i: i64,
    /// Seconds.
    pub s: i64,
    /// Microseconds (`f` × 1e6).
    pub us: i64,
    /// `invert`: the interval goes backwards.
    pub invert: bool,
    /// `days`: total days, for intervals made by `diff()`.
    pub days: Option<i64>,
    /// For `createFromDateString()`: the string and its relative part.
    pub from_string: Option<FromString>,
}

/// The relative specification of a `createFromDateString()` interval.
#[derive(Debug, Clone, PartialEq)]
pub struct FromString {
    /// `date_string`.
    pub date_string: Vec<u8>,
    /// The parsed relative part.
    pub relative: Relative,
}

/// "Unknown or bad format (%s)" (DateMalformedIntervalStringException).
pub fn bad_format(spec: &[u8]) -> String {
    format!("Unknown or bad format ({})", String::from_utf8_lossy(spec))
}

impl Interval {
    /// `new DateInterval($spec)`: an ISO 8601 duration ("P1Y2M3DT4H5M6S",
    /// "P2W", "P1Y2M3W4D") or the "P0001-02-03T04:05:06" form; a leading
    /// "Rn/" recurrence count is ignored and surrounding blanks are allowed
    /// (observed on PHP 8.5). Two ISO dates "start/end" give their
    /// difference (test bug52808.phpt). Errors give the exception message.
    pub fn parse_iso(spec: &[u8]) -> Result<Interval, String> {
        Interval::parse_duration(spec).or_else(|e| two_dates(spec).unwrap_or(Err(e)))
    }

    /// The duration forms of [`Interval::parse_iso`].
    pub(crate) fn parse_duration(spec: &[u8]) -> Result<Interval, String> {
        let err = || bad_format(spec);
        let s = trim(spec);
        let mut s = s;
        if s.first() == Some(&b'R') {
            let slash = s.iter().position(|&c| c == b'/').ok_or_else(err)?;
            if !s[1..slash].iter().all(u8::is_ascii_digit) {
                return Err(err());
            }
            s = &s[slash + 1..];
        }
        if s.first() != Some(&b'P') || s.len() < 2 {
            return Err(err());
        }
        let body = &s[1..];
        if body.len() == 19 && body[4] == b'-' {
            return parse_alt(body).ok_or_else(err);
        }
        let mut iv = Interval::default();
        let mut p = 0;
        let mut in_time = false;
        // designators in order: Y M W D, then T, then H M S
        let mut stage = 0u8;
        let mut any = false;
        let mut any_time = false;
        while p < body.len() {
            if body[p] == b'T' {
                if in_time {
                    return Err(err());
                }
                in_time = true;
                stage = 0;
                p += 1;
                continue;
            }
            let start = p;
            while p < body.len() && body[p].is_ascii_digit() {
                p += 1;
            }
            if p == start || p >= body.len() || p - start > 18 {
                return Err(err());
            }
            let n: i64 = std::str::from_utf8(&body[start..p])
                .unwrap()
                .parse()
                .map_err(|_| err())?;
            let unit = body[p];
            p += 1;
            let order = if in_time {
                match unit {
                    b'H' => 1,
                    b'M' => 2,
                    b'S' => 3,
                    _ => return Err(err()),
                }
            } else {
                match unit {
                    b'Y' => 1,
                    b'M' => 2,
                    b'W' => 3,
                    b'D' => 4,
                    _ => return Err(err()),
                }
            };
            if order <= stage {
                return Err(err());
            }
            stage = order;
            if in_time {
                any_time = true;
                match unit {
                    b'H' => iv.h = n,
                    b'M' => iv.i = n,
                    _ => iv.s = n,
                }
            } else {
                any = true;
                match unit {
                    b'Y' => iv.y = n,
                    b'M' => iv.m = n,
                    b'W' => iv.d = n.checked_mul(7).ok_or_else(err)?,
                    _ => iv.d = iv.d.checked_add(n).ok_or_else(err)?,
                }
            }
        }
        if (in_time && !any_time) || !(any || any_time) {
            return Err(err());
        }
        Ok(iv)
    }

    /// `DateInterval::createFromDateString()`: the relative part of a
    /// date/time string ("3 days", "next monday"). Errors give the message
    /// "Unknown or bad format (S) at position N (C): MESSAGE", or "String
    /// 'S' contains non-relative elements" for a date, time or zone
    /// ("noon", "10:00", "UTC"; observed on PHP 8.5), both
    /// DateMalformedIntervalStringException.
    pub fn from_date_string(s: &[u8]) -> Result<Interval, String> {
        let p = parse::parse(s);
        let absolute = p.have_date
            || p.have_time
            || p.zone.is_some()
            || p.y.is_some()
            || p.m.is_some()
            || p.d.is_some();
        if p.errors.is_empty() && absolute {
            return Err(format!(
                "String '{}' contains non-relative elements",
                String::from_utf8_lossy(s)
            ));
        }
        if let Some((pos, msg)) = p.errors.first() {
            let t = trim(s);
            let c = t.get(*pos).copied().unwrap_or(b' ');
            return Err(format!(
                "Unknown or bad format ({}) at position {} ({}): {}",
                String::from_utf8_lossy(s),
                pos,
                String::from_utf8_lossy(&[c]),
                msg
            ));
        }
        let r = p.relative.unwrap_or_default();
        Ok(Interval {
            y: r.y,
            m: r.m,
            d: r.d,
            h: r.h,
            i: r.i,
            s: r.s,
            us: r.us,
            invert: false,
            days: None,
            from_string: Some(FromString {
                date_string: s.to_vec(),
                relative: r,
            }),
        })
    }

    /// `DateInterval::format()`.
    pub fn format(&self, fmt: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut out = Vec::with_capacity(fmt.len() * 2);
        let mut k = 0;
        while k < fmt.len() {
            let c = fmt[k];
            k += 1;
            if c != b'%' {
                out.push(c);
                continue;
            }
            let Some(&f) = fmt.get(k) else {
                // a trailing '%' prints nothing (observed on PHP 8.5)
                break;
            };
            k += 1;
            let _ = match f {
                b'Y' => write!(out, "{:02}", self.y),
                b'y' => write!(out, "{}", self.y),
                b'M' => write!(out, "{:02}", self.m),
                b'm' => write!(out, "{}", self.m),
                b'D' => write!(out, "{:02}", self.d),
                b'd' => write!(out, "{}", self.d),
                b'H' => write!(out, "{:02}", self.h),
                b'h' => write!(out, "{}", self.h),
                b'I' => write!(out, "{:02}", self.i),
                b'i' => write!(out, "{}", self.i),
                b'S' => write!(out, "{:02}", self.s),
                b's' => write!(out, "{}", self.s),
                b'F' => write!(out, "{:06}", self.us),
                b'f' => write!(out, "{}", self.us),
                b'R' => write!(out, "{}", if self.invert { '-' } else { '+' }),
                b'r' => write!(out, "{}", if self.invert { "-" } else { "" }),
                b'a' => match self.days {
                    Some(d) => write!(out, "{d}"),
                    None => write!(out, "(unknown)"),
                },
                b'%' => write!(out, "%"),
                other => write!(out, "%{}", other as char),
            };
        }
        out
    }
}

/// "YYYY-MM-DDTHH:MM:SSZ" or "YYYYMMDDTHHMMSSZ" (the ISO 8601 dates of
/// an interval written as two dates).
fn iso_utc_date(s: &[u8]) -> Option<DateTime> {
    let num = |r: &[u8]| -> Option<i64> {
        if r.is_empty() || !r.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(r).ok()?.parse().ok()
    };
    let f = match s.len() {
        20 if s[4] == b'-'
            && s[7] == b'-'
            && s[10] == b'T'
            && s[13] == b':'
            && s[16] == b':'
            && s[19] == b'Z' =>
        {
            [
                &s[0..4],
                &s[5..7],
                &s[8..10],
                &s[11..13],
                &s[14..16],
                &s[17..19],
            ]
        }
        16 if s[8] == b'T' && s[15] == b'Z' => [
            &s[0..4],
            &s[4..6],
            &s[6..8],
            &s[9..11],
            &s[11..13],
            &s[13..15],
        ],
        _ => return None,
    };
    let v: Option<Vec<i64>> = f.iter().map(|p| num(p)).collect();
    let v = v?;
    Some(DateTime::from_local(
        v[0],
        v[1],
        v[2],
        v[3],
        v[4],
        v[5],
        0,
        TimeZone::Offset(0),
    ))
}

/// `new DateInterval("start/end")`: the difference between two ISO
/// dates (test bug52808.phpt: a missing end is "Failed to parse interval
/// (S)", a bad one "Unknown or bad format (S)"); `None` when the spec
/// does not start with a date.
fn two_dates(spec: &[u8]) -> Option<Result<Interval, String>> {
    let cut = spec.iter().position(|&c| c == b'/' || c == b' ');
    let (first, rest) = match cut {
        Some(p) => (&spec[..p], Some(&spec[p + 1..])),
        None => (spec, None),
    };
    let a = iso_utc_date(first)?;
    let failed = || {
        Err(format!(
            "Failed to parse interval ({})",
            String::from_utf8_lossy(spec)
        ))
    };
    match rest {
        None | Some([]) => Some(failed()),
        Some(r) => match iso_utc_date(r) {
            Some(b) => Some(Ok(diff(&a, &b, false))),
            None => Some(Err(bad_format(spec))),
        },
    }
}

fn trim(s: &[u8]) -> &[u8] {
    let ws = |c: &u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    let a = s.iter().position(|c| !ws(c)).unwrap_or(s.len());
    let b = s.iter().rposition(|c| !ws(c)).map_or(a, |e| e + 1);
    &s[a..b.max(a)]
}

/// "YYYY-MM-DDTHH:MM:SS" after the P (PHP manual: DateInterval, the
/// alternative format; a month above 12 is refused, observed on PHP 8.5).
fn parse_alt(b: &[u8]) -> Option<Interval> {
    if b.len() != 19
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let n = |r: std::ops::Range<usize>| -> Option<i64> {
        let t = &b[r];
        if !t.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(t).ok()?.parse().ok()
    };
    let iv = Interval {
        y: n(0..4)?,
        m: n(5..7)?,
        d: n(8..10)?,
        h: n(11..13)?,
        i: n(14..16)?,
        s: n(17..19)?,
        ..Default::default()
    };
    if iv.m > 12 || iv.d > 31 || iv.h > 24 || iv.i > 59 || iv.s > 59 {
        return None;
    }
    Some(iv)
}

impl Interval {
    /// The interval with every field negated by `invert`.
    fn signed(&self) -> (i64, i64, i64, i64, i64, i64, i64) {
        let k = if self.invert { -1 } else { 1 };
        (
            k * self.y,
            k * self.m,
            k * self.d,
            k * self.h,
            k * self.i,
            k * self.s,
            k * self.us,
        )
    }
}

/// "Only non-special relative time specifications are supported for
/// subtraction" (DateInvalidOperationException, prefixed with the method).
pub const SUB_SPECIAL: &str =
    "Only non-special relative time specifications are supported for subtraction";

/// `DateTime::sub()`: [`add`] backwards; an interval from
/// `createFromDateString()` with a weekday or "X of" part is refused
/// (observed on PHP 8.5).
pub fn sub(dt: &DateTime, iv: &Interval) -> Result<DateTime, &'static str> {
    if let Some(fs) = &iv.from_string
        && (fs.relative.weekday.is_some() || fs.relative.special != parse::Special::None)
    {
        return Err(SUB_SPECIAL);
    }
    Ok(add(dt, iv, -1))
}

/// `DateTime::add()` (`sign` 1) and the arithmetic of `sub()` (-1): years,
/// months and days on the wall clock, then hours and below as elapsed time
/// (tests DateTime_add-*.phpt, rfc-datetime_and_daylight_saving_time-
/// type3-fa.phpt). Intervals from `createFromDateString()` apply their
/// relative part like `modify()`, keeping the time and ignoring "first/last
/// day of" (observed on PHP 8.5: adding "last day of next month" to June
/// 15th gives July 15th).
pub fn add(dt: &DateTime, iv: &Interval, sign: i64) -> DateTime {
    if let Some(fs) = &iv.from_string {
        let mut r = fs.relative;
        r.first_last_day_of = 0;
        if sign < 0 {
            r.y = -r.y;
            r.m = -r.m;
            r.d = -r.d;
            r.h = -r.h;
            r.i = -r.i;
            r.s = -r.s;
            r.us = -r.us;
        }
        let p = parse::Parsed::from_relative(r);
        return crate::resolve::resolve(&p, dt.ts, dt.us, &dt.zone, crate::resolve::Mode::Modify);
    }
    let (y, m, d, h, i, s, us) = iv.signed();
    let (y, m, d, h, i, s, us) = (
        y * sign,
        m * sign,
        d * sign,
        h * sign,
        i * sign,
        s * sign,
        us * sign,
    );
    let l = dt.local();
    let local = civil::civil_to_ts(
        l.y.saturating_add(y),
        l.m as i64 + m,
        l.d as i64 + d,
        l.h as i64,
        l.i as i64,
        l.s as i64,
    );
    let ts = if y == 0 && m == 0 && d == 0 {
        dt.ts
    } else {
        dt.zone.local_to_ts_with_dst(local, l.is_dst)
    };
    let total_us = dt.us as i128 + us as i128;
    let secs = h as i128 * 3600 + i as i128 * 60 + s as i128 + total_us.div_euclid(1_000_000);
    let ts = (ts as i128 + secs).clamp(i64::MIN as i128, i64::MAX as i128) as i64;
    DateTime {
        ts,
        us: total_us.rem_euclid(1_000_000) as u32,
        zone: dt.zone.clone(),
    }
}

/// `DateTime::diff()`: the interval from `a` to `b`.
///
/// Observed (tests DateTime_diff-*.phpt): the dates and times compare on
/// the wall clock, except that when they are less than a day apart the
/// hours, minutes and seconds are the elapsed time (so a DST change in
/// between counts); `days` is the number of whole days.
pub fn diff(a: &DateTime, b: &DateTime, absolute: bool) -> Interval {
    let invert = (b.ts, b.us) < (a.ts, a.us);
    let (lo, hi) = if invert { (b, a) } else { (a, b) };
    let same_zone = match (&lo.zone, &hi.zone) {
        (TimeZone::Id(x), TimeZone::Id(y)) => x.zone == y.zone,
        (x, y) => x == y,
    };
    let (l1, l2) = if same_zone {
        (lo.local(), hi.local())
    } else {
        let utc = TimeZone::Offset(0);
        (
            LocalTime::new(lo.ts, lo.us, &utc),
            LocalTime::new(hi.ts, hi.us, &utc),
        )
    };
    let el =
        (hi.ts as i128 * 1_000_000 + hi.us as i128) - (lo.ts as i128 * 1_000_000 + lo.us as i128);
    let el = el.clamp(i64::MIN as i128, i64::MAX as i128) as i64;
    let elapsed = |iv: &mut Interval| {
        iv.us = el % 1_000_000;
        let secs = el / 1_000_000;
        iv.s = secs % 60;
        iv.i = secs / 60 % 60;
        iv.h = secs / 3600;
    };
    let wall_lo = (l1.local_secs(), l1.us);
    let wall_hi = (l2.local_secs(), l2.us);
    let mut iv;
    if wall_hi < wall_lo {
        // a fall-back overlap reverses the wall clock: elapsed time only
        // (test DateTime_diff-fall-type3-type3.phpt, dtsec/stsec)
        iv = Interval::default();
        elapsed(&mut iv);
        iv.invert = invert && !absolute;
        iv.days = Some(el / 86_400_000_000);
        return iv;
    }
    iv = wall_diff(&l1, &l2, invert);
    if iv.y == 0 && iv.m == 0 && iv.d == 0 {
        elapsed(&mut iv);
    }
    iv.invert = invert && !absolute;
    iv.days = Some(full_days(&l1, &l2));
    iv
}

/// Field-wise wall clock difference `b - a` (b not earlier), borrowing.
///
/// Days are borrowed from the months before `b`'s, or, when the interval
/// is inverted, from `a`'s month on (tests bug49081, DateTime_diff-dates:
/// January 31st to March 1st is 29 days, March 1st back to January 31st is
/// one month and one day).
fn wall_diff(a: &LocalTime, b: &LocalTime, inverted: bool) -> Interval {
    let mut us = b.us as i64 - a.us as i64;
    let mut s = b.s as i64 - a.s as i64;
    let mut i = b.i as i64 - a.i as i64;
    let mut h = b.h as i64 - a.h as i64;
    let mut d = b.d as i64 - a.d as i64;
    let mut m = b.m as i64 - a.m as i64;
    let mut y = b.y - a.y;
    if us < 0 {
        us += 1_000_000;
        s -= 1;
    }
    if s < 0 {
        s += 60;
        i -= 1;
    }
    if i < 0 {
        i += 60;
        h -= 1;
    }
    if h < 0 {
        h += 24;
        d -= 1;
    }
    let (mut by, mut bm) = if inverted {
        (a.y, a.m as i64)
    } else if b.m == 1 {
        (b.y - 1, 12)
    } else {
        (b.y, b.m as i64 - 1)
    };
    while d < 0 {
        d += civil::days_in_month(by, bm as u32) as i64;
        m -= 1;
        if inverted {
            bm += 1;
            if bm > 12 {
                bm = 1;
                by += 1;
            }
        } else {
            bm -= 1;
            if bm < 1 {
                bm = 12;
                by -= 1;
            }
        }
    }
    if m < 0 {
        m += 12;
        y -= 1;
    }
    Interval {
        y,
        m,
        d,
        h,
        i,
        s,
        us,
        ..Default::default()
    }
}

fn full_days(a: &LocalTime, b: &LocalTime) -> i64 {
    let da = civil::days_from_civil(a.y, a.m as i64, a.d as i64);
    let db = civil::days_from_civil(b.y, b.m as i64, b.d as i64);
    let mut n = db - da;
    let ta = (a.h as i64 * 3600 + a.i as i64 * 60 + a.s as i64) * 1_000_000 + a.us as i64;
    let tb = (b.h as i64 * 3600 + b.i as i64 * 60 + b.s as i64) * 1_000_000 + b.us as i64;
    if tb < ta {
        n -= 1;
    }
    n
}
