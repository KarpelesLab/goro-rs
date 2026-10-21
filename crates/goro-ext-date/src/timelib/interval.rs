//! Port of timelib interval.c

use super::tm2unixtime::{do_normalize, do_rel_normalize, epoch_days_from_time, update_ts};
use super::unixtime2tm::update_from_sse;
use super::*;

fn same_tzid(one: &Time, two: &Time) -> bool {
    one.zone_type == ZONETYPE_ID
        && two.zone_type == ZONETYPE_ID
        && match (&one.tz_info, &two.tz_info) {
            (Some(a), Some(b)) => a.name == b.name,
            _ => false,
        }
}

/// Returns true if swapped
fn sort_old_to_new(one: &Time, two: &Time) -> bool {
    if same_tzid(one, two) {
        let a = (one.y, one.m, one.d, one.h, one.i, one.s, one.us);
        let b = (two.y, two.m, two.d, two.h, two.i, two.s, two.us);
        return a > b;
    }
    one.sse > two.sse || (one.sse == two.sse && one.us > two.us)
}

pub fn diff_days(one: &Time, two: &Time) -> i64 {
    if same_timezone(one, two) {
        let (earliest, latest) = if time_compare(one, two) < 0 { (one, two) } else { (two, one) };
        let earliest_time = hmsf_to_decimal_hour(earliest.h, earliest.i, earliest.s, earliest.us);
        let latest_time = hmsf_to_decimal_hour(latest.h, latest.i, latest.s, latest.us);
        let mut days = (epoch_days_from_time(one) - epoch_days_from_time(two)).abs();
        if latest_time < earliest_time && days > 0 {
            days -= 1;
        }
        // C stores into an int
        days as i32 as i64
    } else {
        (((one.sse - two.sse) as f64).floor() / 86400.0).abs() as i32 as i64
    }
}

fn diff_with_tzid(one_in: &Time, two_in: &Time) -> RelTime {
    let mut rt = RelTime::default();
    let swapped = sort_old_to_new(one_in, two_in);
    let (mut one, mut two) = if swapped { (two_in.clone(), one_in.clone()) } else { (one_in.clone(), two_in.clone()) };
    if swapped {
        rt.invert = 1;
    }
    let mut dst_corr = two.z - one.z;
    let dst_h_corr = dst_corr / 3600;
    let dst_m_corr = (dst_corr % 3600) / 60;

    rt.y = two.y - one.y;
    rt.m = two.m - one.m;
    rt.d = two.d - one.d;
    rt.h = two.h - one.h;
    rt.i = two.i - one.i;
    rt.s = two.s - one.s;
    rt.us = two.us - one.us;

    rt.days = diff_days(&one, &two);

    if two.sse < one.sse {
        let flipped = ((rt.i * 60) + rt.s - dst_corr).abs();
        rt.h = flipped / SECS_PER_HOUR;
        rt.i = (flipped - rt.h * SECS_PER_HOUR) / 60;
        rt.s = flipped % 60;
        rt.invert = 1 - rt.invert;
    }

    if rt.invert != 0 {
        let (mut by, mut bm) = (one.y, one.m);
        do_rel_normalize(&mut by, &mut bm, &mut rt);
        one.y = by;
        one.m = bm;
    } else {
        let (mut by, mut bm) = (two.y, two.m);
        do_rel_normalize(&mut by, &mut bm, &mut rt);
        two.y = by;
        two.m = bm;
    }

    if one.dst == 1 && two.dst == 0 {
        if two.tz_info.is_some() && (two.sse - one.sse + dst_corr) < SECS_PER_DAY {
            rt.h -= dst_h_corr;
            rt.i -= dst_m_corr;
        }
    } else if one.dst == 0 && two.dst == 1 {
        if let Some(tzi) = &two.tz_info {
            if let Some((_trans_offset, trans_transition_time, _)) = tzi.get_offset_info(two.sse) {
                if !((one.sse + SECS_PER_DAY > trans_transition_time) && (one.sse + SECS_PER_DAY <= (trans_transition_time + dst_corr)))
                    && two.sse >= trans_transition_time
                    && ((two.sse - one.sse + dst_corr) % SECS_PER_DAY) > (two.sse - trans_transition_time)
                {
                    rt.h -= dst_h_corr;
                    rt.i -= dst_m_corr;
                }
            }
        }
    } else if two.sse - one.sse >= SECS_PER_DAY {
        if let Some(tzi) = &two.tz_info {
            if let Some((trans_offset, trans_transition_time, _)) = tzi.get_offset_info(two.sse - two.z) {
                dst_corr = one.z - trans_offset as i64;
                if two.sse >= trans_transition_time - dst_corr && two.sse < trans_transition_time {
                    rt.d -= 1;
                    rt.h = 24;
                }
            }
        }
    }
    rt
}

/// timelib_diff
pub fn diff(one_in: &Time, two_in: &Time) -> RelTime {
    if same_tzid(one_in, two_in) {
        return diff_with_tzid(one_in, two_in);
    }
    let mut rt = RelTime::default();
    let swapped = sort_old_to_new(one_in, two_in);
    let (mut one, mut two) = if swapped { (two_in.clone(), one_in.clone()) } else { (one_in.clone(), two_in.clone()) };
    if swapped {
        rt.invert = 1;
    }
    rt.y = two.y - one.y;
    rt.m = two.m - one.m;
    rt.d = two.d - one.d;
    rt.h = two.h - one.h;
    if one.zone_type != ZONETYPE_ID {
        rt.h += one.dst as i64;
    }
    if two.zone_type != ZONETYPE_ID {
        rt.h -= two.dst as i64;
    }
    rt.i = two.i - one.i;
    rt.s = two.s - one.s - two.z + one.z;
    rt.us = two.us - one.us;

    rt.days = diff_days(&one, &two);

    if rt.invert != 0 {
        let (mut by, mut bm) = (one.y, one.m);
        do_rel_normalize(&mut by, &mut bm, &mut rt);
        one.y = by;
        one.m = bm;
    } else {
        let (mut by, mut bm) = (two.y, two.m);
        do_rel_normalize(&mut by, &mut bm, &mut rt);
        two.y = by;
        two.m = bm;
    }
    rt
}

/// timelib_add
pub fn add(old_time: &Time, interval: &RelTime) -> Time {
    let mut t = old_time.clone();
    if interval.have_weekday_relative != 0 || interval.have_special_relative != 0 {
        t.relative = interval.clone();
    } else {
        let bias = if interval.invert != 0 { -1 } else { 1 };
        t.relative = RelTime::default();
        t.relative.y = interval.y * bias;
        t.relative.m = interval.m * bias;
        t.relative.d = interval.d * bias;
        t.relative.h = interval.h * bias;
        t.relative.i = interval.i * bias;
        t.relative.s = interval.s * bias;
        t.relative.us = interval.us * bias;
    }
    t.have_relative = 1;
    t.sse_uptodate = 0;
    update_ts(&mut t, None);
    update_from_sse(&mut t);
    t.have_relative = 0;
    t
}

/// timelib_sub
pub fn sub(old_time: &Time, interval: &RelTime) -> Time {
    let mut t = old_time.clone();
    let bias = if interval.invert != 0 { -1 } else { 1 };
    t.relative = RelTime::default();
    t.relative.y = 0 - (interval.y * bias);
    t.relative.m = 0 - (interval.m * bias);
    t.relative.d = 0 - (interval.d * bias);
    t.relative.h = 0 - (interval.h * bias);
    t.relative.i = 0 - (interval.i * bias);
    t.relative.s = 0 - (interval.s * bias);
    t.relative.us = 0 - (interval.us * bias);
    t.have_relative = 1;
    t.sse_uptodate = 0;
    update_ts(&mut t, None);
    update_from_sse(&mut t);
    t.have_relative = 0;
    t
}

fn do_range_limit_simple(start: i64, end: i64, adj: i64, a: &mut i64, b: &mut i64) {
    if *a < start {
        *b -= (start - *a - 1) / adj + 1;
        *a += adj * ((start - *a - 1) / adj + 1);
    }
    if *a >= end {
        *b += *a / adj;
        *a -= adj * (*a / adj);
    }
}

fn add_sub_wall(old_time: &Time, interval: &RelTime, sign: i64) -> Time {
    let mut t = old_time.clone();
    t.have_relative = 1;
    t.sse_uptodate = 0;
    if interval.have_weekday_relative != 0 || interval.have_special_relative != 0 {
        t.relative = interval.clone();
        update_ts(&mut t, None);
        update_from_sse(&mut t);
    } else {
        let bias = if interval.invert != 0 { -1 } else { 1 };
        t.relative = RelTime::default();
        t.relative.y = sign * (interval.y * bias);
        t.relative.m = sign * (interval.m * bias);
        t.relative.d = sign * (interval.d * bias);
        if t.relative.y != 0 || t.relative.m != 0 || t.relative.d != 0 {
            update_ts(&mut t, None);
        }
        if interval.us == 0 {
            t.sse += sign * bias * hms_to_seconds(interval.h, interval.i, interval.s);
            update_from_sse(&mut t);
        } else {
            let mut temp = interval.clone();
            do_range_limit_simple(0, 1000000, 1000000, &mut temp.us, &mut temp.s);
            t.sse += sign * bias * hms_to_seconds(temp.h, temp.i, temp.s);
            update_from_sse(&mut t);
            t.us += sign * temp.us * bias;
            do_normalize(&mut t);
            update_ts(&mut t, None);
        }
        do_normalize(&mut t);
    }
    if t.zone_type == ZONETYPE_ID {
        if let Some(tz) = t.tz_info.clone() {
            set_timezone(&mut t, &tz);
        }
    }
    t.have_relative = 0;
    t
}

/// timelib_add_wall
pub fn add_wall(old_time: &Time, interval: &RelTime) -> Time {
    add_sub_wall(old_time, interval, 1)
}

/// timelib_sub_wall
pub fn sub_wall(old_time: &Time, interval: &RelTime) -> Time {
    add_sub_wall(old_time, interval, -1)
}
