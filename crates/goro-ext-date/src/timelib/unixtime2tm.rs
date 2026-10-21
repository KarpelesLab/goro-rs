//! Port of timelib unixtime2tm.c

use super::*;

/// Returns (y, m, d)
pub fn unixtime2date(ts: i64) -> (i64, i64, i64) {
    let mut days = ts / SECS_PER_DAY + HINNANT_EPOCH_SHIFT;
    let t = ts % SECS_PER_DAY;
    if t < 0 {
        days -= 1;
    }
    let era = (if days >= 0 {
        days
    } else {
        days - DAYS_PER_ERA + 1
    }) / DAYS_PER_ERA;
    let day_of_era = (days - era * DAYS_PER_ERA) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let mut y = year_of_era as i64 + era * YEARS_PER_ERA;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_portion = (5 * day_of_year + 2) / 153;
    let d = (day_of_year - (153 * month_portion + 2) / 5 + 1) as i64;
    let m = month_portion as i64 + if month_portion < 10 { 3 } else { -9 };
    if m <= 2 {
        y += 1;
    }
    (y, m, d)
}

/// timelib_unixtime2gmt
pub fn unixtime2gmt(tm: &mut Time, ts: i64) {
    let (y, m, d) = unixtime2date(ts);
    tm.y = y;
    tm.m = m;
    tm.d = d;
    let mut remainder = ts % SECS_PER_DAY;
    if remainder < 0 {
        remainder += SECS_PER_DAY;
    }
    let hours = remainder / 3600;
    let minutes = (remainder - hours * 3600) / 60;
    let seconds = remainder % 60;
    tm.h = hours;
    tm.i = minutes;
    tm.s = seconds;
    tm.z = 0;
    tm.dst = 0;
    tm.sse = ts;
    tm.sse_uptodate = 1;
    tm.tim_uptodate = 1;
    tm.is_localtime = 0;
}

/// timelib_update_from_sse
pub fn update_from_sse(tm: &mut Time) {
    let z = tm.z;
    let dst = tm.dst;
    let sse = tm.sse;
    match tm.zone_type {
        ZONETYPE_ABBR | ZONETYPE_OFFSET => {
            let v = tm.sse + tm.z + (tm.dst as i64 * 3600);
            unixtime2gmt(tm, v);
        }
        ZONETYPE_ID => {
            let offset = tm
                .tz_info
                .as_ref()
                .and_then(|t| t.get_offset_info(tm.sse))
                .map(|x| x.0)
                .unwrap_or(0);
            let v = tm.sse + offset as i64;
            unixtime2gmt(tm, v);
        }
        _ => {
            let v = tm.sse;
            unixtime2gmt(tm, v);
        }
    }
    tm.sse = sse;
    tm.is_localtime = 1;
    tm.have_zone = 1;
    tm.z = z;
    tm.dst = dst;
}

/// timelib_unixtime2local
pub fn unixtime2local(tm: &mut Time, ts: i64) {
    match tm.zone_type {
        ZONETYPE_ABBR | ZONETYPE_OFFSET => {
            let z = tm.z;
            let dst = tm.dst;
            unixtime2gmt(tm, ts + tm.z + (tm.dst as i64 * 3600));
            tm.sse = ts;
            tm.z = z;
            tm.dst = dst;
        }
        ZONETYPE_ID => {
            let tz = tm.tz_info.clone();
            let gmt_offset = tz
                .as_ref()
                .map(|t| t.get_time_zone_info(ts))
                .unwrap_or_default();
            unixtime2gmt(tm, ts + gmt_offset.offset as i64);
            tm.sse = ts;
            tm.dst = gmt_offset.is_dst as i32;
            tm.z = gmt_offset.offset as i64;
            tm.tz_info = tz;
            tm.tz_abbr_update(&gmt_offset.abbr);
        }
        _ => {
            tm.is_localtime = 0;
            tm.have_zone = 0;
            return;
        }
    }
    tm.is_localtime = 1;
    tm.have_zone = 1;
}
