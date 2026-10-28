//! Calendars as ICU's Calendar class exposes them to PHP's IntlCalendar
//! (PHP manual: IntlCalendar, IntlGregorianCalendar; ICU API reference:
//! Calendar, GregorianCalendar, "Calendar field resolution").
//!
//! A calendar holds an instant (milliseconds since 1970, UTC) and the
//! fields of that instant in its calendar system and time zone. Setting a
//! field invalidates the instant; reading recomputes the instant from the
//! fields (resolving the date from the most recently set field
//! combination: MONTH + DAY_OF_MONTH, MONTH + WEEK_OF_MONTH + DAY_OF_WEEK,
//! MONTH + DAY_OF_WEEK_IN_MONTH + DAY_OF_WEEK, DAY_OF_YEAR, WEEK_OF_YEAR +
//! DAY_OF_WEEK; the hour from HOUR_OF_DAY or AM_PM + HOUR) and then all
//! fields from the instant. Week fields follow the first day of the week
//! and the minimal days in the first week (CLDR weekData for the locale's
//! region). Lenient calendars accept out-of-range fields and roll them
//! over; non-lenient ones report U_ILLEGAL_ARGUMENT_ERROR.

pub mod systems;

use crate::datefmt::supplemental as sup;
use crate::error::U_ILLEGAL_ARGUMENT_ERROR;
use crate::timezone::{TimeZone, WallTime};
pub use systems::CalType;
use systems::EPOCH_JD;

pub const ERA: usize = 0;
pub const YEAR: usize = 1;
pub const MONTH: usize = 2;
pub const WEEK_OF_YEAR: usize = 3;
pub const WEEK_OF_MONTH: usize = 4;
pub const DATE: usize = 5;
pub const DAY_OF_YEAR: usize = 6;
pub const DAY_OF_WEEK: usize = 7;
pub const DAY_OF_WEEK_IN_MONTH: usize = 8;
pub const AM_PM: usize = 9;
pub const HOUR: usize = 10;
pub const HOUR_OF_DAY: usize = 11;
pub const MINUTE: usize = 12;
pub const SECOND: usize = 13;
pub const MILLISECOND: usize = 14;
pub const ZONE_OFFSET: usize = 15;
pub const DST_OFFSET: usize = 16;
pub const YEAR_WOY: usize = 17;
pub const DOW_LOCAL: usize = 18;
pub const EXTENDED_YEAR: usize = 19;
pub const JULIAN_DAY: usize = 20;
pub const MILLISECONDS_IN_DAY: usize = 21;
pub const IS_LEAP_MONTH: usize = 22;
pub const ORDINAL_MONTH: usize = 23;
/// Number of fields.
pub const FIELD_COUNT: usize = 24;

/// Days of the week (Calendar::DOW_*): 1 = Sunday … 7 = Saturday.
pub const SUNDAY: i32 = 1;

const UNSET: i32 = 0;
const INTERNAL: i32 = 1;
const MIN_USER: i32 = 2;

const DAY: f64 = 86_400_000.0;
const HOUR_MS: f64 = 3_600_000.0;

/// The supported instant range (about ±5.8 million years).
pub const MAX_MILLIS: f64 = 183_882_168_921_600_000.0;
pub const MIN_MILLIS: f64 = -184_303_902_528_000_000.0;

/// ICU's default Gregorian cutover: 1582-10-15 00:00 UTC.
pub const DEFAULT_CUTOVER: f64 = -12_219_292_800_000.0;

const MAX_YEAR: i32 = 5_828_963;

/// Results with an ICU error code.
pub type CalResult<T> = Result<T, i64>;

/// Day of week types (Calendar::DOW_TYPE_*).
pub const WEEKDAY: i32 = 0;
pub const WEEKEND: i32 = 1;
pub const WEEKEND_ONSET: i32 = 2;
pub const WEEKEND_CEASE: i32 = 3;

/// A calendar.
#[derive(Clone, Debug)]
pub struct Calendar {
    pub cal_type: CalType,
    time: f64,
    fields: [i32; FIELD_COUNT],
    stamp: [i32; FIELD_COUNT],
    next_stamp: i32,
    is_time_set: bool,
    are_fields_set: bool,
    virtually_set: bool,
    lenient: bool,
    first_dow: i32,
    min_days: i32,
    repeated: WallTime,
    skipped: WallTime,
    tz: TimeZone,
    cutover_ms: f64,
    cutover_jd: i64,
    weekend: (i32, i32, i32, i32),
    /// The locale the calendar was created for (ICU valid locale).
    pub valid_locale: String,
    /// The locale its data comes from (ICU actual locale).
    pub actual_locale: String,
}

/// CLDR week data for a territory: (first day, minimal days, weekend
/// start, weekend end), with the "001" defaults.
pub fn week_data(region: &str) -> (i32, i32, i32, i32) {
    let get = |t: &str| {
        sup::WEEK_DATA
            .binary_search_by(|e| e.0.cmp(t))
            .ok()
            .map(|i| sup::WEEK_DATA[i])
    };
    let world = get("001").unwrap_or(("001", 2, 1, 7, 1));
    let r = get(region).unwrap_or(world);
    let pick = |a: u8, b: u8| if a == 0 { b as i32 } else { a as i32 };
    (
        pick(r.1, world.1),
        pick(r.2, world.2),
        pick(r.3, world.3),
        pick(r.4, world.4),
    )
}

fn floor_div(a: f64, b: f64) -> f64 {
    (a / b).floor()
}

impl Calendar {
    /// A calendar of type `ct` in zone `tz` with the week data of
    /// `region`, set to `now` (milliseconds).
    pub fn new(ct: CalType, tz: TimeZone, region: &str, now: f64) -> Calendar {
        let (fd, md, ws, we) = week_data(region);
        let (fd, md) = if ct == CalType::Iso8601 {
            (2, 4)
        } else {
            (fd, md)
        };
        let mut c = Calendar {
            cal_type: ct,
            time: now,
            fields: [0; FIELD_COUNT],
            stamp: [UNSET; FIELD_COUNT],
            next_stamp: MIN_USER,
            is_time_set: true,
            are_fields_set: false,
            virtually_set: true,
            lenient: true,
            first_dow: fd,
            min_days: md,
            repeated: WallTime::Last,
            skipped: WallTime::Last,
            tz,
            cutover_ms: DEFAULT_CUTOVER,
            cutover_jd: systems::cutover_jd(DEFAULT_CUTOVER),
            weekend: (ws, 0, we, 86_400_000),
            valid_locale: String::new(),
            actual_locale: String::new(),
        };
        c.compute_fields();
        c.are_fields_set = false;
        c.virtually_set = true;
        c
    }

    // ------------------------------------------------------------ basics

    pub fn time_zone(&self) -> &TimeZone {
        &self.tz
    }

    /// `setTimeZone()`: keeps the instant.
    pub fn set_time_zone(&mut self, tz: TimeZone) {
        let _ = self.get_time();
        self.tz = tz;
        self.are_fields_set = false;
        self.virtually_set = true;
    }

    /// Sets the zone used for the pending fields (parsing).
    pub fn set_time_zone_keep_fields(&mut self, tz: TimeZone) {
        self.tz = tz;
        self.is_time_set = false;
    }

    pub fn is_lenient(&self) -> bool {
        self.lenient
    }

    pub fn set_lenient(&mut self, l: bool) {
        self.lenient = l;
    }

    pub fn first_day_of_week(&self) -> i32 {
        self.first_dow
    }

    pub fn set_first_day_of_week(&mut self, d: i32) {
        if self.first_dow != d {
            self.first_dow = d;
            self.are_fields_set = false;
            if self.is_time_set {
                self.virtually_set = true;
            }
        }
    }

    pub fn minimal_days(&self) -> i32 {
        self.min_days
    }

    pub fn set_minimal_days(&mut self, d: i32) {
        let d = d.clamp(1, 7);
        if self.min_days != d {
            self.min_days = d;
            self.are_fields_set = false;
            if self.is_time_set {
                self.virtually_set = true;
            }
        }
    }

    pub fn repeated_wall_time(&self) -> WallTime {
        self.repeated
    }

    pub fn set_repeated_wall_time(&mut self, w: WallTime) {
        self.repeated = w;
    }

    pub fn skipped_wall_time(&self) -> WallTime {
        self.skipped
    }

    pub fn set_skipped_wall_time(&mut self, w: WallTime) {
        self.skipped = w;
    }

    pub fn gregorian_change(&self) -> f64 {
        self.cutover_ms
    }

    /// `setGregorianChange()`.
    pub fn set_gregorian_change(&mut self, ms: f64) -> CalResult<()> {
        if !ms.is_finite() {
            return Err(U_ILLEGAL_ARGUMENT_ERROR);
        }
        let _ = self.get_time();
        self.cutover_ms = ms;
        self.cutover_jd = systems::cutover_jd(ms);
        self.are_fields_set = false;
        self.virtually_set = true;
        Ok(())
    }

    /// `isLeapYear()` (Gregorian, with the cutover).
    pub fn is_leap_year(&self, year: i64) -> bool {
        systems::gregorian_leap(year, self.cutover_jd)
    }

    // ------------------------------------------------------------ time

    /// `getTime()`.
    pub fn get_time(&mut self) -> CalResult<f64> {
        if !self.is_time_set {
            self.update_time()?;
        }
        Ok(self.time)
    }

    /// `setTime()`.
    pub fn set_time(&mut self, ms: f64) -> CalResult<()> {
        if ms.is_nan() {
            return Err(U_ILLEGAL_ARGUMENT_ERROR);
        }
        let ms = if ms > MAX_MILLIS || ms < MIN_MILLIS {
            if !self.lenient {
                return Err(U_ILLEGAL_ARGUMENT_ERROR);
            }
            ms.clamp(MIN_MILLIS, MAX_MILLIS)
        } else {
            ms
        };
        self.time = ms;
        self.is_time_set = true;
        self.are_fields_set = false;
        self.virtually_set = true;
        self.stamp = [UNSET; FIELD_COUNT];
        self.fields = [0; FIELD_COUNT];
        self.next_stamp = MIN_USER;
        Ok(())
    }

    fn update_time(&mut self) -> CalResult<()> {
        self.compute_time()?;
        self.is_time_set = true;
        self.are_fields_set = false;
        self.virtually_set = false;
        Ok(())
    }

    /// Makes both the instant and the fields valid.
    pub fn complete(&mut self) -> CalResult<()> {
        if !self.is_time_set {
            self.update_time()?;
        }
        if !self.are_fields_set {
            self.compute_fields();
            self.are_fields_set = true;
            self.virtually_set = false;
        }
        Ok(())
    }

    // ------------------------------------------------------------ fields

    /// `get()`.
    pub fn get(&mut self, field: usize) -> CalResult<i32> {
        self.complete()?;
        Ok(self.fields[field])
    }

    fn internal_or(&self, field: usize, d: i32) -> i32 {
        if self.stamp[field] > UNSET {
            self.fields[field]
        } else {
            d
        }
    }

    /// `set()`.
    pub fn set(&mut self, field: usize, value: i32) {
        if self.virtually_set {
            self.compute_fields();
            self.virtually_set = false;
        }
        self.fields[field] = value;
        self.stamp[field] = self.next_stamp;
        self.next_stamp = self.next_stamp.saturating_add(1);
        self.is_time_set = false;
        self.are_fields_set = false;
    }

    /// `isSet()`.
    pub fn is_set(&self, field: usize) -> bool {
        self.virtually_set || self.stamp[field] != UNSET
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        self.fields = [0; FIELD_COUNT];
        self.stamp = [UNSET; FIELD_COUNT];
        self.is_time_set = false;
        self.are_fields_set = false;
        self.virtually_set = false;
    }

    /// `clear($field)`.
    pub fn clear_field(&mut self, field: usize) {
        if self.virtually_set {
            self.compute_fields();
        }
        self.fields[field] = 0;
        self.stamp[field] = UNSET;
        if field == MONTH {
            self.fields[ORDINAL_MONTH] = 0;
            self.stamp[ORDINAL_MONTH] = UNSET;
        } else if field == ORDINAL_MONTH {
            self.fields[MONTH] = 0;
            self.stamp[MONTH] = UNSET;
        }
        self.is_time_set = false;
        self.are_fields_set = false;
        self.virtually_set = false;
    }

    // ------------------------------------------------------------ computation

    fn year_length(&self, eyear: i64) -> i64 {
        systems::year_length(self.cal_type, eyear, self.cutover_jd)
    }

    fn month_length(&self, eyear: i64, month: i64, leap: bool) -> i64 {
        systems::month_length(self.cal_type, eyear, month, leap, self.cutover_jd)
    }

    /// The week number of day `day` (1-based) of a period whose day
    /// `day_of_period` falls on `dow`.
    fn week_number(&self, day_of_period: i32, dow: i32) -> i32 {
        let period_start_dow = (dow - self.first_dow - day_of_period + 1).rem_euclid(7);
        let mut week = (day_of_period + period_start_dow - 1) / 7;
        if 7 - period_start_dow >= self.min_days {
            week += 1;
        }
        week
    }

    fn compute_fields(&mut self) {
        let (raw, dst) = self.tz.offsets(self.time);
        let local = self.time + raw as f64 + dst as f64;
        let days = floor_div(local, DAY);
        let ms_in_day = (local - days * DAY) as i32;
        let jd = days as i64 + EPOCH_JD;
        let ymd = systems::fields_from_jd(self.cal_type, jd, self.cutover_jd);
        let dow = ((jd + 1).rem_euclid(7) + 1) as i32;
        let f = &mut self.fields;
        f[ERA] = ymd.era;
        f[YEAR] = ymd.year;
        f[EXTENDED_YEAR] = ymd.eyear;
        f[MONTH] = ymd.month;
        f[ORDINAL_MONTH] = ymd.ordinal_month;
        f[DATE] = ymd.dom;
        f[DAY_OF_YEAR] = ymd.doy;
        f[DAY_OF_WEEK] = dow;
        f[DAY_OF_WEEK_IN_MONTH] = (ymd.dom - 1) / 7 + 1;
        f[IS_LEAP_MONTH] = ymd.leap_month as i32;
        f[JULIAN_DAY] = jd.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        f[MILLISECONDS_IN_DAY] = ms_in_day;
        f[AM_PM] = ms_in_day / 43_200_000;
        f[HOUR_OF_DAY] = ms_in_day / 3_600_000;
        f[HOUR] = f[HOUR_OF_DAY] % 12;
        f[MINUTE] = ms_in_day / 60_000 % 60;
        f[SECOND] = ms_in_day / 1000 % 60;
        f[MILLISECOND] = ms_in_day % 1000;
        f[ZONE_OFFSET] = raw;
        f[DST_OFFSET] = dst;
        f[DOW_LOCAL] = (dow - self.first_dow).rem_euclid(7) + 1;
        // weeks
        let first_dow = self.first_dow;
        let min_days = self.min_days;
        let eyear = ymd.eyear as i64;
        let doy = ymd.doy;
        let rel_dow = (dow - first_dow).rem_euclid(7);
        let rel_dow_jan1 = (dow - doy + 7001 - first_dow).rem_euclid(7);
        let mut woy = (doy - 1 + rel_dow_jan1) / 7;
        if 7 - rel_dow_jan1 >= min_days {
            woy += 1;
        }
        let mut year_woy = eyear as i32;
        if woy == 0 {
            let prev_doy = doy + self.year_length(eyear - 1) as i32;
            woy = self.week_number(prev_doy, dow);
            year_woy -= 1;
        } else {
            let last_doy = self.year_length(eyear) as i32;
            if doy >= last_doy - 5 {
                let last_rel_dow = (rel_dow + last_doy - doy).rem_euclid(7);
                if 6 - last_rel_dow >= min_days && doy + 7 - rel_dow > last_doy {
                    woy = 1;
                    year_woy += 1;
                }
            }
        }
        self.fields[WEEK_OF_YEAR] = woy;
        self.fields[YEAR_WOY] = year_woy;
        self.fields[WEEK_OF_MONTH] = self.week_number(ymd.dom, dow);
        for s in self.stamp.iter_mut() {
            *s = INTERNAL;
        }
    }

    fn newest(&self, fields: &[usize]) -> i32 {
        fields.iter().map(|&f| self.stamp[f]).max().unwrap_or(UNSET)
    }

    /// The extended year from the year fields.
    fn resolve_eyear(&self, use_woy: bool) -> i64 {
        let ct = self.cal_type;
        if use_woy && self.stamp[YEAR_WOY] > self.newest(&[YEAR, ERA, EXTENDED_YEAR]) {
            return self.fields[YEAR_WOY] as i64;
        }
        if self.stamp[EXTENDED_YEAR] > self.newest(&[YEAR, ERA]) {
            return self.fields[EXTENDED_YEAR] as i64;
        }
        if self.stamp[YEAR] == UNSET && self.stamp[ERA] == UNSET {
            // the year of 1970-01-01
            return systems::fields_from_jd(ct, EPOCH_JD, self.cutover_jd).eyear as i64;
        }
        let era = if self.stamp[ERA] > UNSET {
            self.fields[ERA] as i64
        } else {
            systems::default_era(ct)
        };
        let year = if self.stamp[YEAR] > UNSET {
            self.fields[YEAR] as i64
        } else {
            1
        };
        systems::eyear_from_era(ct, era, year)
    }

    /// (month, is ordinal, leap flag) from the month fields.
    fn resolve_month(&self) -> (i64, bool, bool) {
        if self.stamp[ORDINAL_MONTH] > self.stamp[MONTH] {
            (self.fields[ORDINAL_MONTH] as i64, true, false)
        } else {
            (
                self.internal_or(MONTH, 0) as i64,
                false,
                self.internal_or(IS_LEAP_MONTH, 0) != 0,
            )
        }
    }

    fn month_start_resolved(&self, eyear: i64) -> i64 {
        let (m, ordinal, leap) = self.resolve_month();
        if ordinal {
            systems::ordinal_month_start(self.cal_type, eyear, m, self.cutover_jd)
        } else {
            systems::month_start(self.cal_type, eyear, m, leap, self.cutover_jd)
        }
    }

    fn dow_value(&self) -> i32 {
        // DOW_LOCAL when set more recently than DAY_OF_WEEK
        if self.stamp[DOW_LOCAL] > self.stamp[DAY_OF_WEEK] {
            (self.fields[DOW_LOCAL] - 1 + self.first_dow - 1).rem_euclid(7) + 1
        } else {
            self.internal_or(DAY_OF_WEEK, self.first_dow)
        }
    }

    fn jd_dow(jd: i64) -> i32 {
        ((jd + 1).rem_euclid(7) + 1) as i32
    }

    /// The JDN of the requested day of week in week `week` of a period
    /// starting at JDN `start`.
    fn jd_of_week(&self, start: i64, week: i64, dow: i32) -> i64 {
        let first_rel = (Self::jd_dow(start) - self.first_dow).rem_euclid(7) as i64;
        let rel = (dow - self.first_dow).rem_euclid(7) as i64;
        // the first day of week 1
        let mut week1 = start - first_rel;
        if 7 - first_rel < self.min_days as i64 {
            week1 += 7;
        }
        week1 + 7 * (week - 1) + rel
    }

    fn resolve_jd(&self) -> CalResult<i64> {
        let date_fields = [
            ERA,
            YEAR,
            MONTH,
            ORDINAL_MONTH,
            DATE,
            DAY_OF_YEAR,
            WEEK_OF_YEAR,
            WEEK_OF_MONTH,
            DAY_OF_WEEK,
            DOW_LOCAL,
            DAY_OF_WEEK_IN_MONTH,
            EXTENDED_YEAR,
            YEAR_WOY,
        ];
        if self.stamp[JULIAN_DAY] > self.newest(&date_fields) {
            return Ok(self.fields[JULIAN_DAY] as i64);
        }
        let dow_stamp = self.newest(&[DAY_OF_WEEK, DOW_LOCAL]);
        let month_stamp = self.newest(&[MONTH, ORDINAL_MONTH]);
        // candidate combinations (key field must be set), most recent wins,
        // ties go to the earlier one
        let groups: [(u8, i32, bool); 5] = [
            (0, self.stamp[DATE].max(month_stamp), true),
            (
                1,
                self.stamp[WEEK_OF_MONTH].max(dow_stamp).max(month_stamp),
                self.stamp[WEEK_OF_MONTH] > UNSET,
            ),
            (
                2,
                self.stamp[DAY_OF_WEEK_IN_MONTH]
                    .max(dow_stamp)
                    .max(month_stamp),
                self.stamp[DAY_OF_WEEK_IN_MONTH] > UNSET,
            ),
            (3, self.stamp[DAY_OF_YEAR], self.stamp[DAY_OF_YEAR] > UNSET),
            (
                4,
                self.stamp[WEEK_OF_YEAR]
                    .max(dow_stamp)
                    .max(self.stamp[YEAR_WOY]),
                self.stamp[WEEK_OF_YEAR] > UNSET,
            ),
        ];
        let mut best = (0u8, i32::MIN);
        for (g, s, ok) in groups {
            if ok && s > best.1 {
                best = (g, s);
            }
        }
        // a lone DAY_OF_WEEK newer than everything else keeps the week
        if dow_stamp > best.1 && dow_stamp >= MIN_USER {
            if self.stamp[WEEK_OF_YEAR] > UNSET {
                best.0 = 4;
            } else if self.stamp[WEEK_OF_MONTH] > UNSET {
                best.0 = 1;
            }
        }
        let ct = self.cal_type;
        let jd = match best.0 {
            0 => {
                let eyear = self.resolve_eyear(false);
                self.month_start_resolved(eyear) + self.internal_or(DATE, 1) as i64 - 1
            }
            1 => {
                let eyear = self.resolve_eyear(false);
                let start = self.month_start_resolved(eyear);
                self.jd_of_week(start, self.fields[WEEK_OF_MONTH] as i64, self.dow_value())
            }
            2 => {
                let eyear = self.resolve_eyear(false);
                let start = self.month_start_resolved(eyear);
                let (m, ordinal, leap) = self.resolve_month();
                let dow = self.dow_value();
                let dim = self.fields[DAY_OF_WEEK_IN_MONTH] as i64;
                let first = start + (dow - Self::jd_dow(start)).rem_euclid(7) as i64;
                if dim > 0 {
                    first + 7 * (dim - 1)
                } else {
                    let len = if ordinal {
                        systems::ordinal_month_start(ct, eyear, m + 1, self.cutover_jd) - start
                    } else {
                        self.month_length(eyear, m, leap)
                    };
                    let last = start + len - 1;
                    let last_dow = last - (Self::jd_dow(last) - dow).rem_euclid(7) as i64;
                    last_dow + 7 * (dim + 1)
                }
            }
            3 => {
                let eyear = self.resolve_eyear(false);
                systems::month_start(ct, eyear, 0, false, self.cutover_jd)
                    + self.fields[DAY_OF_YEAR] as i64
                    - 1
            }
            _ => {
                let eyear = self.resolve_eyear(true);
                let start = systems::month_start(ct, eyear, 0, false, self.cutover_jd);
                self.jd_of_week(start, self.fields[WEEK_OF_YEAR] as i64, self.dow_value())
            }
        };
        Ok(jd)
    }

    fn resolve_ms_in_day(&self) -> f64 {
        let hour_stamp = self.newest(&[HOUR_OF_DAY, HOUR, AM_PM]);
        let time_stamp = hour_stamp.max(self.newest(&[MINUTE, SECOND, MILLISECOND]));
        if self.stamp[MILLISECONDS_IN_DAY] > UNSET && self.stamp[MILLISECONDS_IN_DAY] > time_stamp {
            return self.fields[MILLISECONDS_IN_DAY] as f64;
        }
        let hour = if self.stamp[HOUR_OF_DAY] >= self.newest(&[HOUR, AM_PM]) {
            self.internal_or(HOUR_OF_DAY, 0) as f64
        } else {
            self.internal_or(HOUR, 0) as f64 + 12.0 * self.internal_or(AM_PM, 0) as f64
        };
        hour * HOUR_MS
            + self.internal_or(MINUTE, 0) as f64 * 60_000.0
            + self.internal_or(SECOND, 0) as f64 * 1000.0
            + self.internal_or(MILLISECOND, 0) as f64
    }

    fn validate(&self) -> CalResult<()> {
        for f in 0..FIELD_COUNT {
            if self.stamp[f] < MIN_USER {
                continue;
            }
            let v = self.fields[f];
            let ok = match f {
                DATE => {
                    let eyear = self.resolve_eyear(false);
                    let (m, _, leap) = self.resolve_month();
                    v >= 1 && (v as i64) <= self.month_length(eyear, m, leap)
                }
                DAY_OF_YEAR => {
                    let eyear = self.resolve_eyear(false);
                    v >= 1 && (v as i64) <= self.year_length(eyear)
                }
                DAY_OF_WEEK_IN_MONTH => v != 0 && v >= self.minimum(f) && v <= self.maximum(f),
                _ => v >= self.minimum(f) && v <= self.maximum(f),
            };
            if !ok {
                return Err(U_ILLEGAL_ARGUMENT_ERROR);
            }
        }
        Ok(())
    }

    fn compute_time(&mut self) -> CalResult<()> {
        if !self.lenient {
            self.validate()?;
        }
        let jd = self.resolve_jd()?;
        let ms = self.resolve_ms_in_day();
        let local = (jd - EPOCH_JD) as f64 * DAY + ms;
        let t = if self.stamp[ZONE_OFFSET] >= MIN_USER || self.stamp[DST_OFFSET] >= MIN_USER {
            local - self.internal_or(ZONE_OFFSET, 0) as f64 - self.internal_or(DST_OFFSET, 0) as f64
        } else {
            self.tz.local_to_utc_ms(local, self.skipped, self.repeated)
        };
        if (!t.is_finite() || t > MAX_MILLIS || t < MIN_MILLIS) && !self.lenient {
            return Err(U_ILLEGAL_ARGUMENT_ERROR);
        }
        self.time = t;
        Ok(())
    }

    /// Sets the instant from a local day and time of day (wall time
    /// resolution options apply).
    fn set_local(&mut self, jd: i64, ms_in_day: f64) -> CalResult<()> {
        let local = (jd - EPOCH_JD) as f64 * DAY + ms_in_day;
        let t = self.tz.local_to_utc_ms(local, self.skipped, self.repeated);
        if !t.is_finite() || t > MAX_MILLIS || t < MIN_MILLIS {
            return Err(U_ILLEGAL_ARGUMENT_ERROR);
        }
        self.set_time(t)
    }

    // ------------------------------------------------------------ limits

    fn limits(&self, field: usize) -> (i32, i32, i32, i32) {
        let ct = self.cal_type;
        match field {
            ERA => match ct {
                CalType::Japanese => (
                    0,
                    0,
                    sup::JAPANESE_ERAS.len() as i32 - 1,
                    sup::JAPANESE_ERAS.len() as i32 - 1,
                ),
                CalType::Gregorian
                | CalType::Iso8601
                | CalType::Roc
                | CalType::Coptic
                | CalType::Ethiopic => (0, 0, 1, 1),
                CalType::Chinese | CalType::Dangi => (1, 1, 83_333, 83_333),
                _ => (0, 0, 0, 0),
            },
            YEAR => match ct {
                CalType::Chinese | CalType::Dangi => (1, 1, 60, 60),
                CalType::Japanese => (1, 1, 1, MAX_YEAR),
                _ => (1, 1, MAX_YEAR, MAX_YEAR),
            },
            MONTH | ORDINAL_MONTH => (
                0,
                0,
                if ct == CalType::Hebrew {
                    12
                } else {
                    ct.max_month()
                },
                ct.max_month(),
            ),
            WEEK_OF_YEAR => match ct {
                CalType::Hebrew => (1, 1, 50, 56),
                CalType::Chinese | CalType::Dangi => (1, 1, 50, 55),
                CalType::Islamic
                | CalType::IslamicCivil
                | CalType::IslamicTbla
                | CalType::IslamicUmalqura
                | CalType::IslamicRgsa => (1, 1, 50, 51),
                _ => (1, 1, 52, 53),
            },
            WEEK_OF_MONTH => (0, 0, 4, 6),
            DATE => match ct {
                CalType::Coptic | CalType::Ethiopic | CalType::EthiopicAmeteAlem => (1, 1, 5, 30),
                CalType::Persian | CalType::Indian => (1, 1, 29, 31),
                CalType::Islamic
                | CalType::IslamicCivil
                | CalType::IslamicTbla
                | CalType::IslamicUmalqura
                | CalType::IslamicRgsa
                | CalType::Hebrew
                | CalType::Chinese
                | CalType::Dangi => (1, 1, 29, 30),
                _ => (1, 1, 28, 31),
            },
            DAY_OF_YEAR => match ct {
                CalType::Hebrew => (1, 1, 353, 385),
                CalType::Chinese | CalType::Dangi => (1, 1, 353, 385),
                CalType::Islamic
                | CalType::IslamicCivil
                | CalType::IslamicTbla
                | CalType::IslamicUmalqura
                | CalType::IslamicRgsa => (1, 1, 354, 355),
                _ => (1, 1, 365, 366),
            },
            DAY_OF_WEEK | DOW_LOCAL => (1, 1, 7, 7),
            DAY_OF_WEEK_IN_MONTH => (-1, -1, 4, 5),
            AM_PM => (0, 0, 1, 1),
            HOUR => (0, 0, 11, 11),
            HOUR_OF_DAY => (0, 0, 23, 23),
            MINUTE | SECOND => (0, 0, 59, 59),
            MILLISECOND => (0, 0, 999, 999),
            ZONE_OFFSET => (
                -16 * 3_600_000,
                -16 * 3_600_000,
                12 * 3_600_000,
                30 * 3_600_000,
            ),
            DST_OFFSET => (0, 0, 3_600_000, 2 * 3_600_000),
            YEAR_WOY | EXTENDED_YEAR => (-MAX_YEAR, -MAX_YEAR, MAX_YEAR, MAX_YEAR),
            JULIAN_DAY => (-0x7F00_0000, -0x7F00_0000, 0x7F00_0000, 0x7F00_0000),
            MILLISECONDS_IN_DAY => (0, 0, 86_399_999, 86_399_999),
            IS_LEAP_MONTH if ct.is_lunisolar() => (0, 0, 1, 1),
            _ => (0, 0, 0, 0),
        }
    }

    pub fn minimum(&self, field: usize) -> i32 {
        self.limits(field).0
    }

    pub fn greatest_minimum(&self, field: usize) -> i32 {
        self.limits(field).1
    }

    pub fn least_maximum(&self, field: usize) -> i32 {
        self.limits(field).2
    }

    pub fn maximum(&self, field: usize) -> i32 {
        self.limits(field).3
    }

    /// `getActualMinimum()`.
    pub fn actual_minimum(&mut self, field: usize) -> CalResult<i32> {
        self.complete()?;
        Ok(match field {
            WEEK_OF_MONTH => {
                let dom = self.fields[DATE];
                let dow = self.fields[DAY_OF_WEEK];
                let first_dow = (dow - (dom - 1)).rem_euclid(7);
                let first_dow = if first_dow == 0 { 7 } else { first_dow };
                self.week_number(1, first_dow)
            }
            _ => self.minimum(field),
        })
    }

    /// `getActualMaximum()`.
    pub fn actual_maximum(&mut self, field: usize) -> CalResult<i32> {
        self.complete()?;
        let eyear = self.fields[EXTENDED_YEAR] as i64;
        let ct = self.cal_type;
        Ok(match field {
            DATE => self.month_length(
                eyear,
                self.fields[MONTH] as i64,
                self.fields[IS_LEAP_MONTH] != 0,
            ) as i32,
            DAY_OF_YEAR => self.year_length(eyear) as i32,
            MONTH => self.maximum(MONTH),
            ORDINAL_MONTH => systems::months_in_year(ct, eyear) as i32 - 1,
            DAY_OF_WEEK_IN_MONTH => {
                let len = self.month_length(
                    eyear,
                    self.fields[MONTH] as i64,
                    self.fields[IS_LEAP_MONTH] != 0,
                );
                ((len - 1) / 7 + 1) as i32
            }
            WEEK_OF_MONTH => {
                let len = self.month_length(
                    eyear,
                    self.fields[MONTH] as i64,
                    self.fields[IS_LEAP_MONTH] != 0,
                ) as i32;
                let dom = self.fields[DATE];
                let dow = self.fields[DAY_OF_WEEK];
                let last_dow = (dow + (len - dom) - 1).rem_euclid(7) + 1;
                self.week_number(len, last_dow)
            }
            WEEK_OF_YEAR => {
                // the week of the last day of the year that still belongs to it
                let len = self.year_length(eyear) as i32;
                let doy = self.fields[DAY_OF_YEAR];
                let dow = self.fields[DAY_OF_WEEK];
                let mut d = len;
                loop {
                    let ddow = (dow + (d - doy) - 1).rem_euclid(7) + 1;
                    let rel_dow = (ddow - self.first_dow).rem_euclid(7);
                    // does day d belong to week 1 of the next year?
                    let next_year_week1 = 6 - (rel_dow + len - d).rem_euclid(7) >= self.min_days
                        && d + 7 - rel_dow > len;
                    if !next_year_week1 {
                        let rel_dow_jan1 = (ddow - d + 7001 - self.first_dow).rem_euclid(7);
                        let mut woy = (d - 1 + rel_dow_jan1) / 7;
                        if 7 - rel_dow_jan1 >= self.min_days {
                            woy += 1;
                        }
                        break woy.max(1);
                    }
                    d -= 1;
                }
            }
            YEAR | EXTENDED_YEAR | YEAR_WOY => self.maximum(field),
            _ => self.maximum(field),
        })
    }

    // ------------------------------------------------------------ add / roll

    fn ms_in_day_now(&self) -> f64 {
        self.fields[MILLISECONDS_IN_DAY] as f64
    }

    fn cur_jd(&self) -> i64 {
        self.fields[JULIAN_DAY] as i64
    }

    /// Moves to (eyear, month, dom pinned to the month), keeping the time
    /// of day.
    fn set_ymd_pinned(
        &mut self,
        eyear: i64,
        month: i64,
        leap: bool,
        ordinal: bool,
    ) -> CalResult<()> {
        let ct = self.cal_type;
        let cut = self.cutover_jd;
        let start = if ordinal {
            systems::ordinal_month_start(ct, eyear, month, cut)
        } else {
            systems::month_start(ct, eyear, month, leap, cut)
        };
        let f = systems::fields_from_jd(ct, start, cut);
        let len = systems::month_length(ct, f.eyear as i64, f.month as i64, f.leap_month, cut);
        let dom = (self.fields[DATE] as i64).min(len).max(1);
        let ms = self.ms_in_day_now();
        self.set_local(start + dom - 1, ms)
    }

    /// `add()`.
    pub fn add(&mut self, field: usize, amount: i32) -> CalResult<()> {
        if amount == 0 {
            return Ok(());
        }
        self.complete()?;
        let ct = self.cal_type;
        let a = amount as i64;
        match field {
            ERA => {
                let v = self.fields[ERA] + amount;
                self.set(ERA, v);
                self.complete()
            }
            YEAR | EXTENDED_YEAR | YEAR_WOY => {
                let eyear = self.fields[EXTENDED_YEAR] as i64;
                let sign = if field == YEAR
                    && self.fields[ERA] == 0
                    && matches!(
                        ct,
                        CalType::Gregorian | CalType::Iso8601 | CalType::Roc | CalType::Coptic
                    ) {
                    -1
                } else {
                    1
                };
                let ny = eyear + sign * a;
                if ny.abs() > MAX_YEAR as i64 {
                    return Err(U_ILLEGAL_ARGUMENT_ERROR);
                }
                if field == YEAR_WOY {
                    // keep the week and day of week
                    let woy = self.fields[WEEK_OF_YEAR] as i64;
                    let dow = self.fields[DAY_OF_WEEK];
                    let start = systems::month_start(
                        ct,
                        self.fields[YEAR_WOY] as i64 + a,
                        0,
                        false,
                        self.cutover_jd,
                    );
                    let jd = self.jd_of_week(start, woy, dow);
                    let ms = self.ms_in_day_now();
                    return self.set_local(jd, ms);
                }
                if ct == CalType::Hebrew || ct.is_lunisolar() {
                    let m = self.fields[MONTH] as i64;
                    let leap = self.fields[IS_LEAP_MONTH] != 0;
                    return self.set_ymd_pinned(ny, m, leap, false);
                }
                self.set_ymd_pinned(ny, self.fields[MONTH] as i64, false, false)
            }
            MONTH | ORDINAL_MONTH => {
                let eyear = self.fields[EXTENDED_YEAR] as i64;
                if ct == CalType::Hebrew || ct.is_lunisolar() {
                    let o = self.fields[ORDINAL_MONTH] as i64 + a;
                    return self.set_ymd_pinned(eyear, o, false, true);
                }
                let n = if matches!(
                    ct,
                    CalType::Coptic | CalType::Ethiopic | CalType::EthiopicAmeteAlem
                ) {
                    13
                } else {
                    12
                };
                let total = self.fields[MONTH] as i64 + a;
                let ny = eyear + total.div_euclid(n);
                self.set_ymd_pinned(ny, total.rem_euclid(n), false, false)
            }
            WEEK_OF_YEAR | WEEK_OF_MONTH | DAY_OF_WEEK_IN_MONTH => {
                let jd = self.cur_jd() + 7 * a;
                let ms = self.ms_in_day_now();
                self.set_local(jd, ms)
            }
            DATE | DAY_OF_YEAR | DAY_OF_WEEK | DOW_LOCAL | JULIAN_DAY => {
                let jd = self.cur_jd() + a;
                let ms = self.ms_in_day_now();
                self.set_local(jd, ms)
            }
            AM_PM => self.add_ms(a as f64 * 12.0 * HOUR_MS),
            HOUR | HOUR_OF_DAY => self.add_ms(a as f64 * HOUR_MS),
            MINUTE => self.add_ms(a as f64 * 60_000.0),
            SECOND => self.add_ms(a as f64 * 1000.0),
            MILLISECOND | MILLISECONDS_IN_DAY => self.add_ms(a as f64),
            _ => Err(U_ILLEGAL_ARGUMENT_ERROR),
        }
    }

    fn add_ms(&mut self, d: f64) -> CalResult<()> {
        let t = self.time + d;
        if t > MAX_MILLIS || t < MIN_MILLIS {
            return Err(U_ILLEGAL_ARGUMENT_ERROR);
        }
        self.set_time(t)
    }

    /// `roll()`.
    pub fn roll(&mut self, field: usize, amount: i32) -> CalResult<()> {
        if amount == 0 {
            return Ok(());
        }
        self.complete()?;
        let ct = self.cal_type;
        let a = amount as i64;
        let ms = self.ms_in_day_now();
        let wrap = |v: i64, min: i64, max: i64| (v - min).rem_euclid(max - min + 1) + min;
        match field {
            DATE => {
                let eyear = self.fields[EXTENDED_YEAR] as i64;
                let len = self.month_length(
                    eyear,
                    self.fields[MONTH] as i64,
                    self.fields[IS_LEAP_MONTH] != 0,
                );
                let dom = self.fields[DATE] as i64;
                let nd = wrap(dom + a, 1, len);
                let jd = self.cur_jd() + nd - dom;
                self.set_local(jd, ms)
            }
            DAY_OF_YEAR => {
                let eyear = self.fields[EXTENDED_YEAR] as i64;
                let len = self.year_length(eyear);
                let doy = self.fields[DAY_OF_YEAR] as i64;
                let nd = wrap(doy + a, 1, len);
                let jd = self.cur_jd() + nd - doy;
                self.set_local(jd, ms)
            }
            MONTH | ORDINAL_MONTH => {
                let eyear = self.fields[EXTENDED_YEAR] as i64;
                let n = systems::months_in_year(ct, eyear);
                if ct == CalType::Hebrew || ct.is_lunisolar() {
                    let o = wrap(self.fields[ORDINAL_MONTH] as i64 + a, 0, n - 1);
                    return self.set_ymd_pinned(eyear, o, false, true);
                }
                let m = wrap(self.fields[MONTH] as i64 + a, 0, n - 1);
                self.set_ymd_pinned(eyear, m, false, false)
            }
            YEAR | EXTENDED_YEAR | YEAR_WOY => self.add(field, amount),
            ERA => {
                let (min, max) = (self.minimum(ERA) as i64, self.maximum(ERA) as i64);
                let v = wrap(self.fields[ERA] as i64 + a, min, max) as i32;
                self.set(ERA, v);
                self.complete()
            }
            HOUR_OF_DAY => {
                let h = self.fields[HOUR_OF_DAY] as i64;
                let nh = wrap(h + a, 0, 23);
                self.set_local(self.cur_jd(), ms + (nh - h) as f64 * HOUR_MS)
            }
            HOUR => {
                let h = self.fields[HOUR] as i64;
                let nh = wrap(h + a, 0, 11);
                self.set_local(self.cur_jd(), ms + (nh - h) as f64 * HOUR_MS)
            }
            AM_PM => {
                if a.rem_euclid(2) == 0 {
                    return Ok(());
                }
                let d = if self.fields[AM_PM] == 0 { 12.0 } else { -12.0 };
                self.set_local(self.cur_jd(), ms + d * HOUR_MS)
            }
            MINUTE => {
                let v = self.fields[MINUTE] as i64;
                self.set_local(
                    self.cur_jd(),
                    ms + (wrap(v + a, 0, 59) - v) as f64 * 60_000.0,
                )
            }
            SECOND => {
                let v = self.fields[SECOND] as i64;
                self.set_local(self.cur_jd(), ms + (wrap(v + a, 0, 59) - v) as f64 * 1000.0)
            }
            MILLISECOND => {
                let v = self.fields[MILLISECOND] as i64;
                self.set_local(self.cur_jd(), ms + (wrap(v + a, 0, 999) - v) as f64)
            }
            MILLISECONDS_IN_DAY => {
                let v = self.fields[MILLISECONDS_IN_DAY] as i64;
                self.set_local(self.cur_jd(), wrap(v + a, 0, 86_399_999) as f64)
            }
            DAY_OF_WEEK | DOW_LOCAL => {
                let rel = (self.fields[DAY_OF_WEEK] - self.first_dow).rem_euclid(7) as i64;
                let nrel = wrap(rel + a, 0, 6);
                self.set_local(self.cur_jd() + nrel - rel, ms)
            }
            WEEK_OF_MONTH | DAY_OF_WEEK_IN_MONTH => {
                // move by weeks, staying in the month
                let eyear = self.fields[EXTENDED_YEAR] as i64;
                let len = self.month_length(
                    eyear,
                    self.fields[MONTH] as i64,
                    self.fields[IS_LEAP_MONTH] != 0,
                );
                let dom = self.fields[DATE] as i64;
                let first = (dom - 1) % 7 + 1;
                let count = (len - first) / 7 + 1;
                let idx = (dom - first) / 7;
                let nidx = wrap(idx + a, 0, count - 1);
                self.set_local(self.cur_jd() + 7 * (nidx - idx), ms)
            }
            WEEK_OF_YEAR => {
                let eyear = self.fields[EXTENDED_YEAR] as i64;
                let len = self.year_length(eyear);
                let doy = self.fields[DAY_OF_YEAR] as i64;
                let first = (doy - 1) % 7 + 1;
                let count = (len - first) / 7 + 1;
                let idx = (doy - first) / 7;
                let nidx = wrap(idx + a, 0, count - 1);
                self.set_local(self.cur_jd() + 7 * (nidx - idx), ms)
            }
            JULIAN_DAY => self.add(field, amount),
            _ => Err(U_ILLEGAL_ARGUMENT_ERROR),
        }
    }

    /// `fieldDifference()`: the number of `field` units from the current
    /// instant to `when`; the calendar is left advanced by that amount.
    pub fn field_difference(&mut self, when: f64, field: usize) -> CalResult<i32> {
        if !when.is_finite() {
            return Err(U_ILLEGAL_ARGUMENT_ERROR);
        }
        let start = self.get_time()?;
        let probe = |n: i64| -> CalResult<f64> {
            let mut c = self.clone();
            c.set_time(start)?;
            c.add(field, n as i32)?;
            c.get_time()
        };
        let mut min: i64 = 0;
        if start < when {
            let mut max: i64 = 1;
            loop {
                let ms = probe(max)?;
                if ms == when {
                    min = max;
                    break;
                }
                if ms > when {
                    // binary search in (min, max)
                    while max - min > 1 {
                        let t = (min + max) / 2;
                        let ms = probe(t)?;
                        if ms == when {
                            min = t;
                            break;
                        } else if ms > when {
                            max = t;
                        } else {
                            min = t;
                        }
                    }
                    break;
                }
                min = max;
                max <<= 1;
                if max > i32::MAX as i64 {
                    return Err(U_ILLEGAL_ARGUMENT_ERROR);
                }
            }
        } else if start > when {
            let mut max: i64 = -1;
            loop {
                let ms = probe(max)?;
                if ms == when {
                    min = max;
                    break;
                }
                if ms < when {
                    while min - max > 1 {
                        let t = (min + max) / 2;
                        let ms = probe(t)?;
                        if ms == when {
                            min = t;
                            break;
                        } else if ms < when {
                            max = t;
                        } else {
                            min = t;
                        }
                    }
                    break;
                }
                min = max;
                max <<= 1;
                if max < i32::MIN as i64 {
                    return Err(U_ILLEGAL_ARGUMENT_ERROR);
                }
            }
        }
        self.set_time(start)?;
        self.add(field, min as i32)?;
        Ok(min as i32)
    }

    // ------------------------------------------------------------ comparisons, weekend

    /// `isEquivalentTo()`: same behaviour, any instant.
    pub fn is_equivalent_to(&self, o: &Calendar) -> bool {
        self.cal_type == o.cal_type
            && self.lenient == o.lenient
            && self.first_dow == o.first_dow
            && self.min_days == o.min_days
            && self.tz.equals(&o.tz)
            && self.weekend == o.weekend
            && self.repeated == o.repeated
            && self.skipped == o.skipped
            && (!self.cal_type.is_gregorian_like() || self.cutover_ms == o.cutover_ms)
    }

    /// `getDayOfWeekType()`.
    pub fn day_of_week_type(&self, dow: i32) -> i32 {
        let (on, on_ms, off, off_ms) = self.weekend;
        if dow == on {
            return if on_ms == 0 { WEEKEND } else { WEEKEND_ONSET };
        }
        if dow == off {
            return if off_ms >= 86_400_000 {
                WEEKEND
            } else {
                WEEKEND_CEASE
            };
        }
        // strictly between onset and cease (cyclically)
        let span = (off - on).rem_euclid(7);
        let pos = (dow - on).rem_euclid(7);
        if pos > 0 && pos < span {
            WEEKEND
        } else {
            WEEKDAY
        }
    }

    /// `getWeekendTransition()`.
    pub fn weekend_transition(&self, dow: i32) -> CalResult<i32> {
        let (on, on_ms, off, off_ms) = self.weekend;
        if dow == on {
            Ok(on_ms)
        } else if dow == off {
            Ok(off_ms)
        } else {
            Err(U_ILLEGAL_ARGUMENT_ERROR)
        }
    }

    /// `isWeekend()` at an instant (None: the calendar's).
    pub fn is_weekend(&mut self, at: Option<f64>) -> CalResult<bool> {
        let mut c = self.clone();
        if let Some(t) = at {
            c.set_time(t)?;
        }
        let dow = c.get(DAY_OF_WEEK)?;
        let ms = c.get(MILLISECONDS_IN_DAY)?;
        Ok(match self.day_of_week_type(dow) {
            WEEKDAY => false,
            WEEKEND => true,
            WEEKEND_ONSET => ms >= self.weekend.1,
            _ => ms < self.weekend.3,
        })
    }

    /// Whether DST is in effect at the calendar's instant.
    pub fn in_daylight_time(&mut self) -> CalResult<bool> {
        let t = self.get_time()?;
        Ok(self.tz.in_daylight_time(t))
    }

    /// A copy set to instant `ms`, fields computed (for formatting).
    pub fn at(&self, ms: f64) -> CalResult<Calendar> {
        let mut c = self.clone();
        c.set_time(ms)?;
        c.complete()?;
        Ok(c)
    }

    /// The field values (after [`Calendar::complete`]).
    pub fn field(&self, f: usize) -> i32 {
        self.fields[f]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc() -> Calendar {
        Calendar::new(CalType::Gregorian, TimeZone::create("UTC"), "NL", 0.0)
    }

    #[test]
    fn clear_and_set() {
        let mut c = utc();
        c.clear();
        c.set(YEAR, 2012);
        c.set(MONTH, 1);
        c.set(DATE, 29);
        assert_eq!(c.get_time(), Ok(1_330_473_600_000.0));
        let mut c = Calendar::new(
            CalType::Gregorian,
            TimeZone::create("Europe/Amsterdam"),
            "NL",
            0.0,
        );
        c.clear();
        assert_eq!(c.get_time(), Ok(-3_600_000.0));
    }

    #[test]
    fn clear_month() {
        let mut c = utc();
        c.set_time(1_330_491_967_000.0).unwrap(); // 2012-02-29 05:06:07
        assert!(c.is_set(MONTH));
        c.clear_field(MONTH);
        assert!(!c.is_set(MONTH));
        assert_eq!(c.get_time(), Ok(1_327_813_567_000.0));
    }

    #[test]
    fn add_roll_difference() {
        let mut c = utc();
        c.set_time(1_330_473_600_000.0).unwrap();
        c.add(DATE, 1).unwrap();
        c.add(HOUR, 5).unwrap();
        c.add(MINUTE, 6).unwrap();
        c.add(SECOND, 7).unwrap();
        assert_eq!(c.get_time(), Ok(1_330_578_367_000.0));
        let mut c = utc();
        c.set_time(1_330_491_967_000.0).unwrap();
        assert_eq!(c.field_difference(1_330_495_568_000.0, SECOND), Ok(3601));
        assert_eq!(c.get(HOUR_OF_DAY), Ok(6));
        let mut c = utc();
        c.clear();
        c.set(YEAR, 2012);
        c.set(MONTH, 1);
        c.set(DATE, 28);
        c.roll(DATE, 2).unwrap();
        assert_eq!((c.get(MONTH), c.get(DATE)), (Ok(1), Ok(1)));
    }

    #[test]
    fn wall_time() {
        let tz = TimeZone::create("Europe/Amsterdam");
        let mut c = Calendar::new(CalType::Gregorian, tz, "NL", 0.0);
        c.clear();
        c.set(YEAR, 2012);
        c.set(MONTH, 2);
        c.set(DATE, 25);
        c.set(HOUR_OF_DAY, 2);
        c.set(MINUTE, 30);
        assert_eq!((c.get(HOUR_OF_DAY), c.get(MINUTE)), (Ok(3), Ok(30)));
        c.set_skipped_wall_time(WallTime::First);
        c.set(HOUR_OF_DAY, 2);
        c.set(MINUTE, 30);
        assert_eq!(c.get(HOUR_OF_DAY), Ok(1));
        c.set_skipped_wall_time(WallTime::NextValid);
        c.set(HOUR_OF_DAY, 2);
        c.set(MINUTE, 30);
        assert_eq!((c.get(HOUR_OF_DAY), c.get(MINUTE)), (Ok(3), Ok(0)));
        // repeated: 2012-10-28 02:30
        c.clear();
        c.set(YEAR, 2012);
        c.set(MONTH, 9);
        c.set(DATE, 28);
        c.set(HOUR_OF_DAY, 2);
        c.set(MINUTE, 30);
        assert_eq!(c.get_time(), Ok(1_351_387_800_000.0));
        c.set_repeated_wall_time(WallTime::First);
        c.set(MINUTE, 30);
        assert_eq!(c.get_time(), Ok(1_351_384_200_000.0));
    }

    #[test]
    fn weeks() {
        let mut c = utc();
        c.set_time(1_330_473_600_000.0).unwrap(); // Wed 2012-02-29
        assert_eq!(c.get(DAY_OF_WEEK), Ok(4));
        // ISO-like rules (NL): week 9
        assert_eq!(c.get(WEEK_OF_YEAR), Ok(9));
        assert_eq!(c.actual_maximum(DATE), Ok(29));
        assert_eq!(c.day_of_week_type(1), WEEKEND);
        assert_eq!(c.day_of_week_type(7), WEEKEND);
        assert_eq!(c.day_of_week_type(2), WEEKDAY);
        assert_eq!(c.weekend_transition(1), Ok(86_400_000));
    }
}
