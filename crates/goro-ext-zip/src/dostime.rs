//! MS-DOS date and time fields (PKWARE APPNOTE.TXT 4.4.6): local time
//! with two-second resolution, years 1980-2107. This crate reads and
//! writes them as UTC; out-of-range fields are normalized the way C's
//! `mktime()` does (month 0 is the previous December, day 0 the last day
//! of the previous month).

/// Days since 1970-01-01 of a proleptic Gregorian date (month 1-12).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// The date `(year, month 1-12, day)` of a day number.
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// A DOS `(time, date)` pair as a Unix timestamp (UTC).
pub fn dos_to_unix(time: u16, date: u16) -> i64 {
    let year = 1980 + (date >> 9) as i64;
    let month = ((date >> 5) & 0x0f) as i64;
    let day = (date & 0x1f) as i64;
    let hour = (time >> 11) as i64;
    let min = ((time >> 5) & 0x3f) as i64;
    let sec = ((time & 0x1f) * 2) as i64;
    // normalize the month like mktime()
    let m0 = month - 1;
    let y = year + m0.div_euclid(12);
    let m = m0.rem_euclid(12) + 1;
    let days = days_from_civil(y, m, 1) + day - 1;
    days * 86400 + hour * 3600 + min * 60 + sec
}

/// A Unix timestamp as a DOS `(time, date)` pair (UTC), clamped to the
/// representable range.
pub fn unix_to_dos(t: i64) -> (u16, u16) {
    let lo = days_from_civil(1980, 1, 1) * 86400;
    let hi = days_from_civil(2107, 12, 31) * 86400 + 86399;
    let t = t.clamp(lo, hi);
    let days = t.div_euclid(86400);
    let secs = t.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    let time = ((secs / 3600) << 11) | (((secs / 60) % 60) << 5) | ((secs % 60) / 2);
    let date = ((y - 1980) << 9) | (m << 5) | d;
    (time as u16, date as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        // 2019-12-25 00:00:00 UTC
        let t = 1_577_232_000;
        let (tm, dt) = unix_to_dos(t);
        assert_eq!(dos_to_unix(tm, dt), t);
        // odd seconds are rounded down
        assert_eq!(dos_to_unix(unix_to_dos(t + 3).0, unix_to_dos(t + 3).1), t + 2);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 1996-12-24 23:32:00, the TorrentZip timestamp
        assert_eq!(dos_to_unix(0xBC00, 0x2198), 851_470_320);
    }
}
