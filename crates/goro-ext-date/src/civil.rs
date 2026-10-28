//! Proleptic Gregorian calendar arithmetic.
//!
//! Days are counted from 1970-01-01 (day 0). Every function works over the
//! whole `i64` timestamp range PHP supports (years roughly ±292 billion),
//! using `i128` where an intermediate value could overflow.

/// Seconds per day.
pub const SECS_PER_DAY: i64 = 86_400;

/// Whether `y` is a leap year in the proleptic Gregorian calendar.
#[inline]
pub const fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

/// Number of days in month `m` (1-12) of year `y`.
#[inline]
pub const fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
    }
}

/// Number of days in year `y`.
#[inline]
pub const fn days_in_year(y: i64) -> u32 {
    if is_leap(y) { 366 } else { 365 }
}

/// Days since 1970-01-01 for a civil date. The month may be outside 1-12
/// and the day outside the month: both are carried into the year/month.
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    // normalise the month first
    let (y, m) = {
        let m0 = m - 1;
        (y as i128 + m0.div_euclid(12) as i128, m0.rem_euclid(12) + 1)
    };
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400); // [0, 399]
    let mp = (m + 9) % 12; // March = 0
    let doy = (153 * mp + 2) / 5; // first day of the month, [0, 306]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy as i128; // [0, 146096]
    let days = era * 146_097 + doe - 719_468 + (d - 1) as i128;
    days.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// Civil date (year, month 1-12, day 1-31) for a day count.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z as i128 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i128::from(m <= 2);
    (y as i64, m, d)
}

/// Day of the week for a day count: 0 = Sunday … 6 = Saturday.
#[inline]
pub fn weekday(days: i64) -> u32 {
    (days + 4).rem_euclid(7) as u32
}

/// Day of the week for a civil date: 0 = Sunday … 6 = Saturday.
pub fn weekday_of(y: i64, m: u32, d: u32) -> u32 {
    weekday(days_from_civil(y, m as i64, d as i64))
}

/// Day of the year, 0-based (January 1st is 0), as `date('z')`.
pub fn day_of_year(y: i64, m: u32, d: u32) -> u32 {
    const CUM: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let mut n = CUM[(m - 1) as usize] + d - 1;
    if m > 2 && is_leap(y) {
        n += 1;
    }
    n
}

/// ISO 8601 week date (ISO year, week 1-53, ISO weekday 1 = Monday … 7).
pub fn iso_week_date(y: i64, m: u32, d: u32) -> (i64, u32, u32) {
    let days = days_from_civil(y, m as i64, d as i64);
    let wd = match weekday(days) {
        0 => 7,
        w => w,
    };
    // The Thursday of this week decides the ISO year.
    let thursday = days - wd as i64 + 4;
    let (iy, _, _) = civil_from_days(thursday);
    let jan1 = days_from_civil(iy, 1, 1);
    let week = ((thursday - jan1) / 7 + 1) as u32;
    (iy, week, wd)
}

/// Day count of the Monday that starts ISO week `week` of ISO year `iy`
/// (weeks outside 1-53 carry into the neighbouring years), plus `wd - 1`
/// days (ISO weekday, 1 = Monday; may be outside 1-7).
pub fn days_from_iso_week(iy: i64, week: i64, wd: i64) -> i64 {
    let jan4 = days_from_civil(iy, 1, 4);
    let jan4_wd = match weekday(jan4) {
        0 => 7,
        w => w as i64,
    };
    let week1_monday = jan4 - (jan4_wd - 1);
    week1_monday + (week - 1) * 7 + (wd - 1)
}

/// Split a Unix timestamp into (days, seconds of day).
#[inline]
pub fn split_ts(ts: i64) -> (i64, u32) {
    (
        ts.div_euclid(SECS_PER_DAY),
        ts.rem_euclid(SECS_PER_DAY) as u32,
    )
}

/// Broken-down civil time of a Unix timestamp (no time zone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    /// Year (proleptic Gregorian, astronomical numbering: 0 is 1 BCE).
    pub y: i64,
    /// Month, 1-12.
    pub m: u32,
    /// Day of the month, 1-31.
    pub d: u32,
    /// Hour, 0-23.
    pub h: u32,
    /// Minute, 0-59.
    pub i: u32,
    /// Second, 0-59.
    pub s: u32,
}

impl Civil {
    /// The civil time of `ts` seconds since the epoch.
    pub fn from_ts(ts: i64) -> Civil {
        let (days, sod) = split_ts(ts);
        let (y, m, d) = civil_from_days(days);
        Civil {
            y,
            m,
            d,
            h: sod / 3600,
            i: sod / 60 % 60,
            s: sod % 60,
        }
    }

    /// Seconds since the epoch, saturating at the `i64` range.
    pub fn to_ts(&self) -> i64 {
        civil_to_ts(
            self.y,
            self.m as i64,
            self.d as i64,
            self.h as i64,
            self.i as i64,
            self.s as i64,
        )
    }
}

/// Seconds since the epoch for possibly out-of-range civil fields (they
/// carry), saturating at the `i64` range.
pub fn civil_to_ts(y: i64, m: i64, d: i64, h: i64, i: i64, s: i64) -> i64 {
    let days = days_from_civil(y, m, d) as i128;
    let secs = days * 86_400 + h as i128 * 3600 + i as i128 * 60 + s as i128;
    secs.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_roundtrip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        for z in (-800_000i64..800_000).step_by(37) {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m as i64, d as i64), z);
        }
    }

    #[test]
    fn carries() {
        assert_eq!(days_from_civil(2021, 13, 1), days_from_civil(2022, 1, 1));
        assert_eq!(days_from_civil(2021, 0, 1), days_from_civil(2020, 12, 1));
        assert_eq!(days_from_civil(2021, 2, 29), days_from_civil(2021, 3, 1));
        assert_eq!(days_from_civil(2021, 3, 0), days_from_civil(2021, 2, 28));
        assert_eq!(days_from_civil(2021, -11, 1), days_from_civil(2020, 1, 1));
    }

    #[test]
    fn extremes() {
        // PHP prints PHP_INT_MIN as -292277022657-01-27T08:29:52+00:00
        // (observed on PHP 8.5).
        let c = Civil::from_ts(i64::MIN);
        assert_eq!(
            (c.y, c.m, c.d, c.h, c.i, c.s),
            (-292_277_022_657, 1, 27, 8, 29, 52)
        );
        assert_eq!(c.to_ts(), i64::MIN);
        let c = Civil::from_ts(i64::MAX);
        assert_eq!(
            (c.y, c.m, c.d, c.h, c.i, c.s),
            (292_277_026_596, 12, 4, 15, 30, 7)
        );
        assert_eq!(c.to_ts(), i64::MAX);
    }

    #[test]
    fn iso_weeks() {
        assert_eq!(iso_week_date(2008, 12, 29), (2009, 1, 1));
        assert_eq!(iso_week_date(2010, 1, 3), (2009, 53, 7));
        assert_eq!(iso_week_date(2021, 1, 4), (2021, 1, 1));
        assert_eq!(iso_week_date(2020, 12, 31), (2020, 53, 4));
        assert_eq!(
            civil_from_days(days_from_iso_week(2009, 53, 7)),
            (2010, 1, 3)
        );
        assert_eq!(
            civil_from_days(days_from_iso_week(2009, 1, 1)),
            (2008, 12, 29)
        );
    }

    #[test]
    fn leap_and_doy() {
        assert!(is_leap(2000) && !is_leap(1900) && is_leap(2024) && is_leap(0) && is_leap(-4));
        assert_eq!(day_of_year(2024, 12, 31), 365);
        assert_eq!(day_of_year(2023, 3, 1), 59);
        assert_eq!(weekday_of(1970, 1, 1), 4);
        assert_eq!(weekday_of(2000, 1, 1), 6);
    }
}
