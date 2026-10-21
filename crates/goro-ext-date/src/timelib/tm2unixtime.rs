//! Port of timelib tm2unixtime.c

use super::dow::day_of_week;
use super::*;

const DAYS_IN_MONTH_LEAP: [i64; 13] = [31, 31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const DAYS_IN_MONTH: [i64; 13] = [31, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

pub fn do_range_limit(start: i64, end: i64, adj: i64, a: &mut i64, b: &mut i64) {
    if *a < start {
        let a_plus_1 = a.wrapping_add(1);
        *b = b.wrapping_sub(start.wrapping_sub(a_plus_1) / adj + 1);
        *a = a.wrapping_add(adj.wrapping_mul(start.wrapping_sub(a_plus_1) / adj));
        *a = a.wrapping_add(adj);
    }
    if *a >= end {
        *b = b.wrapping_add(*a / adj);
        *a = a.wrapping_sub(adj.wrapping_mul(*a / adj));
    }
}

fn inc_month(y: &mut i64, m: &mut i64) {
    *m += 1;
    if *m > 12 {
        *m -= 12;
        *y += 1;
    }
}

fn dec_month(y: &mut i64, m: &mut i64) {
    *m -= 1;
    if *m < 1 {
        *m += 12;
        *y -= 1;
    }
}

fn dim(leap: bool, month: i64) -> i64 {
    let idx = month.clamp(0, 12) as usize;
    if leap { DAYS_IN_MONTH_LEAP[idx] } else { DAYS_IN_MONTH[idx] }
}

fn do_range_limit_days_relative(base_y: &mut i64, base_m: &mut i64, _y: &mut i64, m: &mut i64, d: &mut i64, invert: i32) {
    do_range_limit(1, 13, 12, base_m, base_y);
    let mut year = *base_y;
    let mut month = *base_m;
    if invert == 0 {
        while *d < 0 {
            dec_month(&mut year, &mut month);
            let days = dim(is_leap(year), month);
            *d += days;
            *m -= 1;
        }
    } else {
        while *d < 0 {
            let days = dim(is_leap(year), month);
            *d += days;
            *m -= 1;
            inc_month(&mut year, &mut month);
        }
    }
}

fn do_range_limit_days(y: &mut i64, m: &mut i64, d: &mut i64) -> bool {
    let mut retval = false;
    if *d >= DAYS_PER_ERA || *d <= -DAYS_PER_ERA {
        *y = y.wrapping_add(YEARS_PER_ERA * (*d / DAYS_PER_ERA));
        *d -= DAYS_PER_ERA * (*d / DAYS_PER_ERA);
    }
    do_range_limit(1, 13, 12, m, y);
    let leapyear = is_leap(*y);
    while *d <= 0 && *m > 0 {
        let mut previous_month = *m - 1;
        let previous_year;
        if previous_month < 1 {
            previous_month += 12;
            previous_year = *y - 1;
        } else {
            previous_year = *y;
        }
        let days_in_previous_month = dim(is_leap(previous_year), previous_month);
        *d += days_in_previous_month;
        *m -= 1;
        retval = true;
    }
    while *d > 0 && *m <= 12 && *d > dim(leapyear, *m) {
        *d -= dim(leapyear, *m);
        *m += 1;
        retval = true;
    }
    retval
}

fn do_adjust_for_weekday(time: &mut Time) {
    let current_dow = day_of_week(time.y, time.m, time.d);
    if time.relative.weekday_behavior == 2 {
        if current_dow == 0 && time.relative.weekday != 0 {
            time.relative.weekday -= 7;
        }
        if time.relative.weekday == 0 && current_dow != 0 {
            time.relative.weekday = 7;
        }
        time.d -= current_dow;
        time.d += time.relative.weekday as i64;
        return;
    }
    let mut difference = time.relative.weekday as i64 - current_dow;
    if (time.relative.d < 0 && difference < 0) || (time.relative.d >= 0 && difference <= -(time.relative.weekday_behavior as i64)) {
        difference += 7;
    }
    if time.relative.weekday >= 0 {
        time.d += difference;
    } else {
        time.d -= 7 - ((time.relative.weekday as i64).abs() - current_dow);
    }
    time.relative.have_weekday_relative = 0;
}

pub fn do_rel_normalize(base_y: &mut i64, base_m: &mut i64, rt: &mut RelTime) {
    do_range_limit(0, 1000000, 1000000, &mut rt.us, &mut rt.s);
    do_range_limit(0, 60, 60, &mut rt.s, &mut rt.i);
    do_range_limit(0, 60, 60, &mut rt.i, &mut rt.h);
    do_range_limit(0, 24, 24, &mut rt.h, &mut rt.d);
    do_range_limit(0, 12, 12, &mut rt.m, &mut rt.y);
    let invert = rt.invert;
    let (mut ry, mut rm, mut rd) = (rt.y, rt.m, rt.d);
    do_range_limit_days_relative(base_y, base_m, &mut ry, &mut rm, &mut rd, invert);
    rt.y = ry;
    rt.m = rm;
    rt.d = rd;
    do_range_limit(0, 12, 12, &mut rt.m, &mut rt.y);
}

fn magic_date_calc(time: &mut Time) {
    if time.d < -719498 {
        return;
    }
    let g = time.d + HINNANT_EPOCH_SHIFT - 1;
    let mut y = (10000 * g + 14780) / 3652425;
    let mut ddd = g - ((365 * y) + (y / 4) - (y / 100) + (y / 400));
    if ddd < 0 {
        y -= 1;
        ddd = g - ((365 * y) + (y / 4) - (y / 100) + (y / 400));
    }
    let mi = (100 * ddd + 52) / 3060;
    let mm = ((mi + 2) % 12) + 1;
    y += (mi + 2) / 12;
    let dd = ddd - ((mi * 306 + 5) / 10) + 1;
    time.y = y;
    time.m = mm;
    time.d = dd;
}

pub fn do_normalize(time: &mut Time) {
    if time.us != UNSET {
        do_range_limit(0, 1000000, 1000000, &mut time.us, &mut time.s);
    }
    if time.s != UNSET {
        do_range_limit(0, 60, 60, &mut time.s, &mut time.i);
    }
    if time.s != UNSET {
        do_range_limit(0, 60, 60, &mut time.i, &mut time.h);
    }
    if time.s != UNSET {
        do_range_limit(0, 24, 24, &mut time.h, &mut time.d);
    }
    do_range_limit(1, 13, 12, &mut time.m, &mut time.y);
    if time.y == 1970 && time.m == 1 && time.d != 1 {
        magic_date_calc(time);
    }
    loop {
        let (mut y, mut m, mut d) = (time.y, time.m, time.d);
        let r = do_range_limit_days(&mut y, &mut m, &mut d);
        time.y = y;
        time.m = m;
        time.d = d;
        if !r {
            break;
        }
    }
    do_range_limit(1, 13, 12, &mut time.m, &mut time.y);
}

fn do_adjust_relative(time: &mut Time) {
    if time.relative.have_weekday_relative != 0 {
        do_adjust_for_weekday(time);
    }
    do_normalize(time);
    if time.have_relative != 0 {
        time.us = time.us.wrapping_add(time.relative.us);
        time.s = time.s.wrapping_add(time.relative.s);
        time.i = time.i.wrapping_add(time.relative.i);
        time.h = time.h.wrapping_add(time.relative.h);
        time.d = time.d.wrapping_add(time.relative.d);
        time.m = time.m.wrapping_add(time.relative.m);
        time.y = time.y.wrapping_add(time.relative.y);
    }
    match time.relative.first_last_day_of {
        SPECIAL_FIRST_DAY_OF_MONTH => time.d = 1,
        SPECIAL_LAST_DAY_OF_MONTH => {
            time.d = 0;
            time.m += 1;
        }
        _ => {}
    }
    do_normalize(time);
}

fn do_adjust_special_weekday(time: &mut Time) {
    let count = time.relative.special_amount;
    let dow = day_of_week(time.y, time.m, time.d);
    time.d += (count / 5) * 7;
    let rem = count % 5;
    if count > 0 {
        if rem == 0 {
            if dow == 0 {
                time.d -= 2;
            } else if dow == 6 {
                time.d -= 1;
            }
        } else if dow == 6 {
            time.d += 1;
        } else if dow + rem > 5 {
            time.d += 2;
        }
    } else {
        if rem == 0 {
            if dow == 6 {
                time.d += 2;
            } else if dow == 0 {
                time.d += 1;
            }
        } else if dow == 0 {
            time.d -= 1;
        } else if dow + rem < 1 {
            time.d -= 2;
        }
    }
    time.d += rem;
}

fn do_adjust_special(time: &mut Time) {
    if time.relative.have_special_relative != 0 && time.relative.special_type == SPECIAL_WEEKDAY {
        do_adjust_special_weekday(time);
    }
    do_normalize(time);
    time.relative.special_type = 0;
    time.relative.special_amount = 0;
}

fn do_adjust_special_early(time: &mut Time) {
    if time.relative.have_special_relative != 0 {
        match time.relative.special_type {
            SPECIAL_DAY_OF_WEEK_IN_MONTH => {
                time.d = 1;
                time.m += time.relative.m;
                time.relative.m = 0;
            }
            SPECIAL_LAST_DAY_OF_WEEK_IN_MONTH => {
                time.d = 1;
                time.m += time.relative.m + 1;
                time.relative.m = 0;
            }
            _ => {}
        }
    }
    match time.relative.first_last_day_of {
        SPECIAL_FIRST_DAY_OF_MONTH => time.d = 1,
        SPECIAL_LAST_DAY_OF_MONTH => {
            time.d = 0;
            time.m += 1;
        }
        _ => {}
    }
    do_normalize(time);
}

fn do_adjust_timezone(tz: &mut Time, tzi: Option<&Rc<TzInfo>>) {
    match tz.zone_type {
        ZONETYPE_OFFSET => {
            tz.is_localtime = 1;
            tz.sse = tz.sse.wrapping_add(-tz.z);
        }
        ZONETYPE_ABBR => {
            tz.is_localtime = 1;
            tz.sse = tz.sse.wrapping_add(-tz.z - tz.dst as i64 * SECS_PER_HOUR);
        }
        _ => {
            let tzi: Option<Rc<TzInfo>> = if tz.zone_type == ZONETYPE_ID { tz.tz_info.clone() } else { tzi.cloned() };
            let tzi = match tzi {
                Some(t) => t,
                None => return,
            };
            let (current_offset, current_transition_time, current_is_dst) = tzi.get_offset_info(tz.sse).unwrap_or((0, 0, 0));
            let (after_offset, after_transition_time, _) =
                tzi.get_offset_info(tz.sse - current_offset as i64).unwrap_or((0, 0, 0));
            let _ = current_transition_time;
            let mut actual_offset = after_offset;
            let mut actual_transition_time = after_transition_time;
            if current_offset == after_offset && tz.have_zone != 0 {
                if current_offset >= 0 && tz.dst != 0 && current_is_dst == 0 {
                    let (earlier_offset, earlier_transition_time, _) =
                        tzi.get_offset_info(tz.sse - current_offset as i64 - 7200).unwrap_or((0, 0, 0));
                    if earlier_offset != after_offset && tz.sse - (earlier_offset as i64) < after_transition_time {
                        actual_offset = earlier_offset;
                        actual_transition_time = earlier_transition_time;
                    }
                } else if current_offset <= 0 && current_is_dst != 0 && tz.dst == 0 {
                    let (later_offset, later_transition_time, _) =
                        tzi.get_offset_info(tz.sse - current_offset as i64 + 7200).unwrap_or((0, 0, 0));
                    if later_offset != after_offset && tz.sse - (later_offset as i64) >= later_transition_time {
                        actual_offset = later_offset;
                        actual_transition_time = later_transition_time;
                    }
                }
            }
            tz.is_localtime = 1;
            let in_transition = actual_transition_time != i64::MIN
                && (tz.sse - actual_offset as i64) >= (actual_transition_time + (current_offset as i64 - actual_offset as i64))
                && (tz.sse - actual_offset as i64) < actual_transition_time;
            let adjustment = if current_offset != actual_offset && !in_transition {
                -(actual_offset as i64)
            } else {
                -(current_offset as i64)
            };
            tz.sse = tz.sse.wrapping_add(adjustment);
            set_timezone(tz, &tzi);
        }
    }
}

pub fn epoch_days_from_time(time: &Time) -> i64 {
    let mut y = time.y;
    if time.m <= 2 {
        y -= 1;
    }
    let era = (if y >= 0 { y } else { y - 399 }) / YEARS_PER_ERA;
    let year_of_era = y - era * YEARS_PER_ERA;
    let day_of_year = (153 * (time.m + if time.m > 2 { -3 } else { 9 }) + 2) / 5 + time.d - 1;
    let day_of_era = year_of_era * DAYS_PER_YEAR + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era.wrapping_mul(DAYS_PER_ERA).wrapping_add(day_of_era).wrapping_sub(HINNANT_EPOCH_SHIFT)
}

/// timelib_update_ts
pub fn update_ts(time: &mut Time, tzi: Option<&Rc<TzInfo>>) {
    do_adjust_special_early(time);
    do_adjust_relative(time);
    do_adjust_special(time);
    time.sse = hms_to_seconds(time.h, time.i, time.s);
    let ed = epoch_days_from_time(time);
    time.sse = time.sse.wrapping_add(ed.wrapping_mul(SECS_PER_DAY / 2));
    time.sse = time.sse.wrapping_add(ed.wrapping_mul(SECS_PER_DAY / 2));
    do_adjust_timezone(time, tzi);
    time.sse_uptodate = 1;
    time.have_relative = 0;
    time.relative.have_weekday_relative = 0;
    time.relative.have_special_relative = 0;
    time.relative.first_last_day_of = 0;
}
