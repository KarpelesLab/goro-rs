//! ICU's IslamicCalendar arithmetic (islamcal.cpp): the civil / tabular
//! variants, Umm al-Qura (the intl crate's table) and the astronomical
//! "islamic" calendar over a port of ICU's CalendarAstronomer moon age.

use std::cell::RefCell;
use std::collections::HashMap;
use std::f64::consts::PI;

pub const CIVIL_EPOC: i64 = 1_948_440;
pub const ASTRONOMICAL_EPOC: i64 = 1_948_439;

const SYNODIC_MONTH: f64 = 29.530588853;
const HIJRA_MILLIS: f64 = -42_521_587_200_000.0;
const DAY_MS: f64 = 86_400_000.0;
const JULIAN_EPOCH_MS: f64 = -210_866_760_000_000.0;
const JD_EPOCH: f64 = 2_447_891.5;
const TROPICAL_YEAR: f64 = 365.242191;

fn floor_div(a: i64, b: i64) -> i64 {
    a.div_euclid(b)
}

/// IslamicCivilCalendar::yearStart(): days from the epoch to 1 Muharram.
pub fn civil_year_start(year: i64) -> i64 {
    (year - 1) * 354 + floor_div(3 + 11 * year, 30)
}

/// monthStart() of the civil calendar (relative to the epoch).
pub fn civil_month_start(year: i64, month: i64) -> i64 {
    (29.5 * month as f64).ceil() as i64 + civil_year_start(year)
}

/// The (year, month 0-based, day, day of year) of `days` since the epoch.
pub fn civil_fields(days: i64) -> (i64, i64, i64, i64) {
    let year = floor_div(30 * days + 10646, 10631);
    let mut month = ((days - 29 - civil_year_start(year)) as f64 / 29.5).ceil() as i64;
    month = month.min(11);
    let ms = civil_month_start(year, month);
    let dom = days - ms + 1;
    let doy = days - civil_month_start(year, 0) + 1;
    (year, month, dom, doy)
}

fn norm2pi(a: f64) -> f64 {
    a - 2.0 * PI * (a / (2.0 * PI)).floor()
}

fn true_anomaly(mean: f64, e: f64) -> f64 {
    let mut ee = mean;
    loop {
        let delta = ee - e * ee.sin() - mean;
        ee -= delta / (1.0 - e * ee.cos());
        if delta.abs() <= 1e-5 {
            break;
        }
    }
    2.0 * ((ee / 2.0).tan() * ((1.0 + e) / (1.0 - e)).sqrt()).atan()
}

/// CalendarAstronomer::getMoonAge() at UDate `time`, in degrees
/// (-180, 180] (IslamicCalendar::moonAge()).
pub fn moon_age(time: f64) -> f64 {
    let jd = (time - JULIAN_EPOCH_MS) / DAY_MS;
    let day = jd - JD_EPOCH;
    // the sun
    let sun_eta_g = 279.403303 * PI / 180.0;
    let sun_omega_g = 282.768422 * PI / 180.0;
    let sun_e = 0.016713;
    let epoch_angle = norm2pi(2.0 * PI / TROPICAL_YEAR * day);
    let mean_anomaly_sun = norm2pi(epoch_angle + sun_eta_g - sun_omega_g);
    let sun_longitude = norm2pi(true_anomaly(mean_anomaly_sun, sun_e) + sun_omega_g);
    // the moon
    let moon_l0 = 318.351648 * PI / 180.0;
    let moon_p0 = 36.340410 * PI / 180.0;
    let mean_longitude = norm2pi(13.1763966 * PI / 180.0 * day + moon_l0);
    let mut mean_anomaly_moon = norm2pi(mean_longitude - 0.1114041 * PI / 180.0 * day - moon_p0);
    let evection =
        1.2739 * PI / 180.0 * (2.0 * (mean_longitude - sun_longitude) - mean_anomaly_moon).sin();
    let annual = 0.1858 * PI / 180.0 * mean_anomaly_sun.sin();
    let a3 = 0.3700 * PI / 180.0 * mean_anomaly_sun.sin();
    mean_anomaly_moon += evection - annual - a3;
    let center = 6.2886 * PI / 180.0 * mean_anomaly_moon.sin();
    let a4 = 0.2140 * PI / 180.0 * (2.0 * mean_anomaly_moon).sin();
    let mut moon_longitude = mean_longitude + evection + center - annual + a4;
    let variation = 0.6583 * PI / 180.0 * (2.0 * (moon_longitude - sun_longitude)).sin();
    moon_longitude += variation;
    let age = norm2pi(moon_longitude - sun_longitude);
    let mut deg = age * 180.0 / PI;
    if deg > 180.0 {
        deg -= 360.0;
    }
    deg
}

thread_local! {
    static MONTH_CACHE: RefCell<HashMap<i64, i64>> = RefCell::new(HashMap::new());
}

/// IslamicCalendar::trueMonthStart(): days from the epoch to the start of
/// month index `month` (12 * (year - 1) + month).
pub fn true_month_start(month: i64) -> i64 {
    if let Some(v) = MONTH_CACHE.with(|c| c.borrow().get(&month).copied()) {
        return v;
    }
    let mut origin = HIJRA_MILLIS + (month as f64 * SYNODIC_MONTH).floor() * DAY_MS;
    let mut age = moon_age(origin);
    if age >= 0.0 {
        loop {
            origin -= DAY_MS;
            age = moon_age(origin);
            if age < 0.0 {
                break;
            }
        }
    } else {
        loop {
            origin += DAY_MS;
            age = moon_age(origin);
            if age >= 0.0 {
                break;
            }
        }
    }
    let start = ((origin - HIJRA_MILLIS) / DAY_MS).floor() as i64 + 1;
    MONTH_CACHE.with(|c| c.borrow_mut().insert(month, start));
    start
}

/// The astronomical month start (days from the epoch).
pub fn astro_month_start(year: i64, month: i64) -> i64 {
    true_month_start(12 * (year - 1) + month)
}

/// The astronomical calendar's fields of `days` since its epoch; `time` is
/// the calendar's instant (ICU consults the moon age at it).
pub fn astro_fields(days: i64, time: f64) -> (i64, i64, i64, i64) {
    let mut months = (days as f64 / SYNODIC_MONTH).floor() as i64;
    let start_date = (months as f64 * SYNODIC_MONTH).floor() as i64;
    let age = moon_age(time);
    if days - start_date >= 25 && age > 0.0 {
        months += 1;
    }
    while true_month_start(months) > days {
        months -= 1;
    }
    let year = if months >= 0 {
        months / 12 + 1
    } else {
        (months + 1) / 12
    };
    let month = months.rem_euclid(12);
    let dom = days - astro_month_start(year, month) + 1;
    let doy = days - astro_month_start(year, 0) + 1;
    (year, month, dom, doy)
}
