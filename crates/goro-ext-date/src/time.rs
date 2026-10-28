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
