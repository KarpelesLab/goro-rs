//! Port of timelib dow.c

use super::is_leap;

const M_TABLE_COMMON: [i64; 13] = [-1, 0, 3, 3, 6, 1, 4, 6, 2, 5, 0, 3, 5];
const M_TABLE_LEAP: [i64; 13] = [-1, 6, 2, 3, 6, 1, 4, 6, 2, 5, 0, 3, 5];

fn positive_mod(x: i64, y: i64) -> i64 {
    let mut tmp = x % y;
    if tmp < 0 {
        tmp += y;
    }
    tmp
}

fn century_value(j: i64) -> i64 {
    6 - positive_mod(j, 4) * 2
}

fn day_of_week_ex(y: i64, m: i64, d: i64, iso: bool) -> i64 {
    let c1 = century_value(positive_mod(y, 400) / 100);
    let y1 = positive_mod(y, 100);
    let mi = m.clamp(0, 12) as usize;
    let m1 = if is_leap(y) {
        M_TABLE_LEAP[mi]
    } else {
        M_TABLE_COMMON[mi]
    };
    let mut dow = positive_mod(c1 + y1 + m1 + (y1 / 4) + d, 7);
    if iso && dow == 0 {
        dow = 7;
    }
    dow
}

pub fn day_of_week(y: i64, m: i64, d: i64) -> i64 {
    day_of_week_ex(y, m, d, false)
}

pub fn iso_day_of_week(y: i64, m: i64, d: i64) -> i64 {
    day_of_week_ex(y, m, d, true)
}

const D_TABLE_COMMON: [i64; 13] = [0, 0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
const D_TABLE_LEAP: [i64; 13] = [0, 0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];
pub const ML_TABLE_COMMON: [i64; 13] = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
pub const ML_TABLE_LEAP: [i64; 13] = [0, 31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

pub fn day_of_year(y: i64, m: i64, d: i64) -> i64 {
    let mi = m.clamp(0, 12) as usize;
    (if is_leap(y) {
        D_TABLE_LEAP[mi]
    } else {
        D_TABLE_COMMON[mi]
    }) + d
        - 1
}

pub fn days_in_month(y: i64, m: i64) -> i64 {
    let mi = m.clamp(0, 12) as usize;
    if is_leap(y) {
        ML_TABLE_LEAP[mi]
    } else {
        ML_TABLE_COMMON[mi]
    }
}

/// Returns (iw, iy)
pub fn isoweek_from_date(y: i64, m: i64, d: i64) -> (i64, i64) {
    let y_leap = is_leap(y) as i64;
    let prev_y_leap = is_leap(y - 1);
    let mut doy = day_of_year(y, m, d) + 1;
    if y_leap == 1 && m > 2 {
        doy += 1;
    }
    let mut jan1weekday = day_of_week(y, 1, 1);
    let mut weekday = day_of_week(y, m, d);
    if weekday == 0 {
        weekday = 7;
    }
    if jan1weekday == 0 {
        jan1weekday = 7;
    }
    let mut iw = 0;
    let iy;
    if doy <= (8 - jan1weekday) && jan1weekday > 4 {
        iy = y - 1;
        if jan1weekday == 5 || (jan1weekday == 6 && prev_y_leap) {
            iw = 53;
        } else {
            iw = 52;
        }
    } else {
        iy = y;
    }
    if iy == y {
        let i = if y_leap == 1 { 366 } else { 365 };
        if (i - (doy - y_leap)) < (4 - weekday) {
            return (1, y + 1);
        }
    }
    if iy == y {
        let j = doy + (7 - weekday) + (jan1weekday - 1);
        iw = j / 7;
        if jan1weekday > 4 {
            iw -= 1;
        }
    }
    (iw, iy)
}

/// Returns (iy, iw, id)
pub fn isodate_from_date(y: i64, m: i64, d: i64) -> (i64, i64, i64) {
    let (iw, iy) = isoweek_from_date(y, m, d);
    (iy, iw, day_of_week_ex(y, m, d, true))
}

pub fn daynr_from_weeknr(iy: i64, iw: i64, id: i64) -> i64 {
    let dow = day_of_week(iy, 1, 1);
    let day = 0 - if dow > 4 { dow - 7 } else { dow };
    day + ((iw - 1) * 7) + id
}

/// Returns (y, m, d)
pub fn date_from_isodate(iy: i64, iw: i64, id: i64) -> (i64, i64, i64) {
    let mut daynr = daynr_from_weeknr(iy, iw, id) + 1;
    let mut y = iy;
    let mut leap = is_leap(y);
    while daynr <= 0 {
        y -= 1;
        leap = is_leap(y);
        daynr += if leap { 366 } else { 365 };
    }
    while daynr > if leap { 366 } else { 365 } {
        daynr -= if leap { 366 } else { 365 };
        y += 1;
        leap = is_leap(y);
    }
    let table = if leap {
        &ML_TABLE_LEAP
    } else {
        &ML_TABLE_COMMON
    };
    let mut m = 1;
    while daynr > table[m as usize] {
        daynr -= table[m as usize];
        m += 1;
    }
    (y, m, daynr)
}
