//! The DateTime value: an instant with microseconds and a time zone.

use crate::civil;
use crate::tz::{ShortStr, TimeZone};

/// The broken-down local time of a [`DateTime`] (what `format()` prints).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    /// Year.
    pub y: i64,
    /// Month, 1-12.
    pub m: u32,
    /// Day of the month.
    pub d: u32,
    /// Hour, 0-23.
    pub h: u32,
    /// Minute.
    pub i: u32,
    /// Second.
    pub s: u32,
    /// Microseconds.
    pub us: u32,
    /// Unix time (seconds).
    pub ts: i64,
    /// UTC offset in seconds.
    pub offset: i32,
    /// DST flag.
    pub is_dst: bool,
    /// Zone abbreviation (empty for offset zones).
    pub abbr: ShortStr,
}

impl LocalTime {
    /// Local time of Unix time `ts` (+ `us` microseconds) in `zone`.
    pub fn new(ts: i64, us: u32, zone: &TimeZone) -> LocalTime {
        let st = zone.state_at(ts);
        let c = civil::Civil::from_ts(ts.saturating_add(st.offset as i64));
        LocalTime {
            y: c.y,
            m: c.m,
            d: c.d,
            h: c.h,
            i: c.i,
            s: c.s,
            us,
            ts,
            offset: st.offset,
            is_dst: st.is_dst,
            abbr: st.abbr,
        }
    }

    /// Day of the week, 0 = Sunday.
    pub fn weekday(&self) -> u32 {
        civil::weekday_of(self.y, self.m, self.d)
    }

    /// The local wall clock as seconds counted from the epoch as if UTC.
    pub fn local_secs(&self) -> i64 {
        civil::civil_to_ts(
            self.y,
            self.m as i64,
            self.d as i64,
            self.h as i64,
            self.i as i64,
            self.s as i64,
        )
    }
}

/// An instant with microsecond precision in a time zone: what PHP's
/// DateTime and DateTimeImmutable hold.
#[derive(Debug, Clone, PartialEq)]
pub struct DateTime {
    /// Unix time in seconds.
    pub ts: i64,
    /// Microseconds, 0-999999.
    pub us: u32,
    /// The zone.
    pub zone: TimeZone,
}

impl DateTime {
    /// Builds from a Unix time.
    pub fn from_ts(ts: i64, us: u32, zone: TimeZone) -> DateTime {
        DateTime { ts, us, zone }
    }

    /// Builds from local wall-clock fields (they may be out of range and
    /// carry), resolving the local time in the zone.
    #[allow(clippy::too_many_arguments)]
    pub fn from_local(
        y: i64,
        m: i64,
        d: i64,
        h: i64,
        i: i64,
        s: i64,
        us: i64,
        zone: TimeZone,
    ) -> DateTime {
        let extra_s = us.div_euclid(1_000_000);
        let us = us.rem_euclid(1_000_000) as u32;
        let local = civil::civil_to_ts(y, m, d, h, i, s.saturating_add(extra_s));
        let ts = zone.local_to_ts(local);
        DateTime { ts, us, zone }
    }

    /// The broken-down local time.
    pub fn local(&self) -> LocalTime {
        LocalTime::new(self.ts, self.us, &self.zone)
    }

    /// Formats with a `date()` format string.
    pub fn format(&self, fmt: &[u8]) -> Vec<u8> {
        crate::format::format(fmt, &self.local(), &self.zone)
    }
}

/// `date()` / `gmdate()`: formats Unix time `ts` in `zone`.
pub fn date(fmt: &[u8], ts: i64, zone: &TimeZone) -> Vec<u8> {
    crate::format::format(fmt, &LocalTime::new(ts, 0, zone), zone)
}

impl DateTime {
    /// `setDate()`: new date, same wall clock time; out-of-range values
    /// carry (`setDate(2021, 2, 30)` is March 2nd).
    pub fn set_date(&self, y: i64, m: i64, d: i64) -> DateTime {
        let l = self.local();
        let local = civil::civil_to_ts(y, m, d, l.h as i64, l.i as i64, l.s as i64);
        DateTime {
            ts: self.zone.local_to_ts_with_dst(local, l.is_dst),
            us: self.us,
            zone: self.zone.clone(),
        }
    }

    /// `setISODate()`: ISO year, week and day of the week (1 = Monday;
    /// out-of-range weeks and days carry).
    pub fn set_iso_date(&self, y: i64, week: i64, dow: i64) -> DateTime {
        let days = civil::days_from_iso_week(y, week, dow);
        let (ny, nm, nd) = civil::civil_from_days(days);
        self.set_date(ny, nm as i64, nd as i64)
    }

    /// `setTime()`: new wall clock time on the same date; out-of-range
    /// values carry, microseconds included.
    pub fn set_time(&self, h: i64, i: i64, s: i64, us: i64) -> DateTime {
        let l = self.local();
        let carry = us.div_euclid(1_000_000);
        let local = civil::civil_to_ts(l.y, l.m as i64, l.d as i64, h, i, s.saturating_add(carry));
        DateTime {
            ts: self.zone.local_to_ts_with_dst(local, l.is_dst),
            us: us.rem_euclid(1_000_000) as u32,
            zone: self.zone.clone(),
        }
    }

    /// `setTimestamp()`: same zone, microseconds reset.
    pub fn set_timestamp(&self, ts: i64) -> DateTime {
        DateTime {
            ts,
            us: 0,
            zone: self.zone.clone(),
        }
    }

    /// `setTimezone()`: same instant in another zone.
    pub fn set_timezone(&self, zone: TimeZone) -> DateTime {
        DateTime {
            ts: self.ts,
            us: self.us,
            zone,
        }
    }

    /// `getOffset()`.
    pub fn offset(&self) -> i32 {
        self.zone.offset_at(self.ts)
    }

    /// The "date" member of `var_dump()`, `serialize()` and `json_encode()`
    /// output: "Y-m-d H:i:s.u" (observed on PHP 8.5).
    pub fn dump_date(&self) -> String {
        String::from_utf8(self.format(b"Y-m-d H:i:s.u")).unwrap_or_default()
    }

    /// `DateTime::createFromTimestamp()` with a float: rounded to the
    /// microsecond, at +00:00. `None` for a non-finite or out-of-range
    /// value ("must be a finite number between -9223372036854775808 and
    /// 9223372036854775807.999999", observed on PHP 8.5).
    pub fn from_float_timestamp(t: f64) -> Option<DateTime> {
        if !t.is_finite() || !(-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&t) {
            return None;
        }
        let secs = t.floor();
        let mut us = ((t - secs) * 1_000_000.0).round() as i64;
        let mut secs = secs as i64;
        if us >= 1_000_000 {
            us -= 1_000_000;
            secs = secs.checked_add(1)?;
        }
        Some(DateTime {
            ts: secs,
            us: us as u32,
            zone: TimeZone::Offset(0),
        })
    }
}
