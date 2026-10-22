//! A port of ICU4C's Calendar / GregorianCalendar field engine
//! (calendar.cpp, gregocal.cpp): field stamps and resolution, lenient
//! normalization, add / roll / fieldDifference, actual limits, week fields,
//! wall time options; with the Gregorian calendar (Julian cutover), its
//! era-based variants (japanese, buddhist, roc, iso8601) and the arithmetic
//! of the other ICU calendar systems.

use std::rc::Rc;

use super::islamic;
use super::zone::Zone;
use super::*;

pub const ORDINAL_MONTH: usize = 23;
/// UCAL_FIELD_COUNT of ICU >= 73 (IntlCalendar::FIELD_FIELD_COUNT).
pub const NF: usize = 24;

const UNSET: i32 = 0;
const INTERNALLY_SET: i32 = 1;
const MIN_USER_STAMP: i32 = 2;

const ONE_DAY: f64 = 86_400_000.0;
const ONE_HOUR: f64 = 3_600_000.0;
const ONE_WEEK: f64 = 7.0 * ONE_DAY;
const EPOCH_JD: i32 = 2_440_588;
const JAN1_1_JD: i64 = 1_721_426;

pub const MIN_MILLIS: f64 = -184_303_902_528_000_000.0;
pub const MAX_MILLIS: f64 = 183_882_168_921_600_000.0;

pub const WALLTIME_LAST: i32 = 0;
pub const WALLTIME_FIRST: i32 = 1;
pub const WALLTIME_NEXT_VALID: i32 = 2;

/// The calendar systems.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalType {
    Gregorian,
    Japanese,
    Buddhist,
    Roc,
    Iso8601,
    Islamic,
    IslamicCivil,
    IslamicTbla,
    IslamicUmalqura,
    IslamicRgsa,
    Hebrew,
    Persian,
    Indian,
    Coptic,
    Ethiopic,
    EthiopicAmeteAlem,
    Chinese,
    Dangi,
}

impl CalType {
    pub fn from_name(n: &str) -> Option<CalType> {
        Some(match n {
            "gregorian" | "gregory" => CalType::Gregorian,
            "japanese" => CalType::Japanese,
            "buddhist" => CalType::Buddhist,
            "roc" => CalType::Roc,
            "iso8601" => CalType::Iso8601,
            "islamic" => CalType::Islamic,
            "islamic-civil" | "islamicc" => CalType::IslamicCivil,
            "islamic-tbla" => CalType::IslamicTbla,
            "islamic-umalqura" => CalType::IslamicUmalqura,
            "islamic-rgsa" => CalType::IslamicRgsa,
            "hebrew" => CalType::Hebrew,
            "persian" => CalType::Persian,
            "indian" => CalType::Indian,
            "coptic" => CalType::Coptic,
            "ethiopic" => CalType::Ethiopic,
            "ethiopic-amete-alem" | "ethioaa" => CalType::EthiopicAmeteAlem,
            "chinese" => CalType::Chinese,
            "dangi" => CalType::Dangi,
            _ => return None,
        })
    }

    /// Calendar::getType()
    pub fn name(self) -> &'static str {
        match self {
            CalType::Gregorian => "gregorian",
            CalType::Japanese => "japanese",
            CalType::Buddhist => "buddhist",
            CalType::Roc => "roc",
            CalType::Iso8601 => "iso8601",
            CalType::Islamic | CalType::IslamicRgsa => "islamic",
            CalType::IslamicCivil => "islamic-civil",
            CalType::IslamicTbla => "islamic-tbla",
            CalType::IslamicUmalqura => "islamic-umalqura",
            CalType::Hebrew => "hebrew",
            CalType::Persian => "persian",
            CalType::Indian => "indian",
            CalType::Coptic => "coptic",
            CalType::Ethiopic => "ethiopic",
            CalType::EthiopicAmeteAlem => "ethiopic-amete-alem",
            CalType::Chinese => "chinese",
            CalType::Dangi => "dangi",
        }
    }

    /// A GregorianCalendar subclass (the Julian / Gregorian machinery).
    pub fn is_gregorian_family(self) -> bool {
        matches!(
            self,
            CalType::Gregorian
                | CalType::Japanese
                | CalType::Buddhist
                | CalType::Roc
                | CalType::Iso8601
        )
    }
}

/// The week data of a region: first day, minimal days, weekend onset /
/// cease days and times.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeekData {
    pub first_day: i32,
    pub min_days: i32,
    pub onset: i32,
    pub onset_ms: i32,
    pub cease: i32,
    pub cease_ms: i32,
}

/// An ICU Calendar.
#[derive(Clone, Debug)]
pub struct Calendar {
    pub cal_type: CalType,
    pub zone: Rc<Zone>,
    pub locale: String,
    fields: [i32; NF],
    stamp: [i32; NF],
    time: f64,
    is_time_set: bool,
    are_fields_set: bool,
    are_all_fields_set: bool,
    are_fields_virtually_set: bool,
    next_stamp: i32,
    pub lenient: bool,
    pub first_day: i32,
    pub min_days: i32,
    pub weekend: (i32, i32, i32, i32),
    pub repeated_wall: i32,
    pub skipped_wall: i32,
    // GregorianCalendar
    pub cutover: f64,
    cutover_year: i32,
    cutover_jd: i32,
    is_gregorian: bool,
    invert_gregorian: bool,
    // computeGregorianFields() results
    g_year: i32,
    g_month: i32,
    g_dom: i32,
    g_doy: i32,
}

pub type R<T> = Result<T, ()>;

fn floor_div(a: i64, b: i64) -> i64 {
    a.div_euclid(b)
}

fn floor_div_f(a: f64, b: f64) -> (f64, f64) {
    let q = (a / b).floor();
    let mut r = a - q * b;
    if r < 0.0 {
        r += b;
    }
    (q, r)
}

const NUM_DAYS: [i32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
const LEAP_NUM_DAYS: [i32; 12] = [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];
const MONTH_LEN: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const LEAP_MONTH_LEN: [i32; 12] = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

fn greg_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

/// Grego::gregorianShift()
fn gregorian_shift(eyear: i32) -> i32 {
    let y = eyear as i64 - 1;
    (floor_div(y, 400) - floor_div(y, 100) + 2) as i32
}

/// JD 0 is a Monday: 1 = Sunday .. 7 = Saturday
fn jd_to_dow(jd: i64) -> i32 {
    ((jd + 1).rem_euclid(7) + 1) as i32
}

fn japanese_era_of(y: i32, m: i32, d: i32) -> i32 {
    let eras = super::data::JAPANESE_ERA_STARTS;
    for (i, &(sy, sm, sd)) in eras.iter().enumerate().rev() {
        if (y, m, d) >= (sy as i32, sm as i32, sd as i32) {
            return i as i32;
        }
    }
    0
}

fn japanese_era_start_year(era: i32) -> i32 {
    let eras = super::data::JAPANESE_ERA_STARTS;
    let i = era.clamp(0, eras.len() as i32 - 1);
    eras[i as usize].0 as i32
}

fn current_japanese_era() -> i32 {
    super::data::JAPANESE_ERA_STARTS.len() as i32 - 1
}

impl Calendar {
    /// Calendar(zone, locale): cleared fields, time not set.
    pub fn new(cal_type: CalType, zone: Rc<Zone>, locale: &str, week: WeekData) -> Calendar {
        let mut c = Calendar {
            cal_type,
            zone,
            locale: locale.to_string(),
            fields: [0; NF],
            stamp: [UNSET; NF],
            time: 0.0,
            is_time_set: false,
            are_fields_set: false,
            are_all_fields_set: false,
            are_fields_virtually_set: false,
            next_stamp: MIN_USER_STAMP,
            lenient: true,
            first_day: week.first_day,
            min_days: week.min_days,
            weekend: (week.onset, week.onset_ms, week.cease, week.cease_ms),
            repeated_wall: WALLTIME_LAST,
            skipped_wall: WALLTIME_LAST,
            cutover: -12_219_292_800_000.0,
            cutover_year: 1582,
            cutover_jd: 2_299_161,
            is_gregorian: true,
            invert_gregorian: false,
            g_year: 0,
            g_month: 0,
            g_dom: 0,
            g_doy: 0,
        };
        if cal_type == CalType::Iso8601 {
            c.first_day = 2;
            c.min_days = 4;
        }
        c.clear_all();
        c
    }

    pub fn type_name(&self) -> &'static str {
        self.cal_type.name()
    }

    // ---- basic state ------------------------------------------------------

    fn internal_get(&self, f: usize) -> i32 {
        self.fields[f]
    }

    fn internal_get_def(&self, f: usize, def: i32) -> i32 {
        if self.stamp[f] > UNSET {
            self.fields[f]
        } else {
            def
        }
    }

    fn internal_set(&mut self, f: usize, v: i32) {
        self.fields[f] = v;
        self.stamp[f] = INTERNALLY_SET;
    }

    pub fn is_set(&self, f: usize) -> bool {
        self.are_fields_virtually_set || self.stamp[f] != UNSET
    }

    /// The raw value of a field (no computation), for debug output.
    pub fn raw_field(&self, f: usize) -> i32 {
        self.fields[f]
    }

    fn recalculate_stamp(&mut self) {
        let mut current = INTERNALLY_SET;
        self.next_stamp = 1;
        for _ in 0..NF {
            let mut best = i32::MAX;
            let mut idx = None;
            for (i, &s) in self.stamp.iter().enumerate() {
                if s > current && s < best {
                    best = s;
                    idx = Some(i);
                }
            }
            match idx {
                Some(i) => {
                    self.next_stamp += 1;
                    self.stamp[i] = self.next_stamp;
                    current = best;
                }
                None => break,
            }
        }
        self.next_stamp += 1;
    }

    pub fn set(&mut self, f: usize, v: i32) {
        if f >= NF {
            return;
        }
        if self.are_fields_virtually_set {
            let _ = self.compute_fields();
        }
        self.fields[f] = v;
        if self.next_stamp == i32::MAX {
            self.recalculate_stamp();
        }
        self.stamp[f] = self.next_stamp;
        self.next_stamp += 1;
        self.is_time_set = false;
        self.are_fields_set = false;
        self.are_fields_virtually_set = false;
    }

    pub fn set_ymd(&mut self, y: i32, m: i32, d: i32) {
        self.set(YEAR, y);
        self.set(MONTH, m);
        self.set(DATE, d);
    }

    pub fn clear_all(&mut self) {
        self.fields = [0; NF];
        self.stamp = [UNSET; NF];
        self.is_time_set = false;
        self.are_fields_set = false;
        self.are_all_fields_set = false;
        self.are_fields_virtually_set = false;
    }

    pub fn clear(&mut self, f: usize) {
        if self.are_fields_virtually_set {
            let _ = self.compute_fields();
        }
        self.fields[f] = 0;
        self.stamp[f] = UNSET;
        if f == MONTH {
            self.fields[ORDINAL_MONTH] = 0;
            self.stamp[ORDINAL_MONTH] = UNSET;
        }
        if f == ORDINAL_MONTH {
            self.fields[MONTH] = 0;
            self.stamp[MONTH] = UNSET;
        }
        self.is_time_set = false;
        self.are_fields_set = false;
        self.are_all_fields_set = false;
        self.are_fields_virtually_set = false;
    }

    pub fn set_time(&mut self, ms: f64) -> R<()> {
        let ms = if ms > MAX_MILLIS {
            if self.lenient {
                MAX_MILLIS
            } else {
                return Err(());
            }
        } else if ms < MIN_MILLIS {
            if self.lenient {
                MIN_MILLIS
            } else {
                return Err(());
            }
        } else if ms.is_nan() {
            return Err(());
        } else {
            ms
        };
        self.time = ms;
        self.are_fields_set = false;
        self.are_all_fields_set = false;
        self.is_time_set = true;
        self.are_fields_virtually_set = true;
        self.fields = [0; NF];
        self.stamp = [UNSET; NF];
        Ok(())
    }

    pub fn get_time(&mut self) -> R<f64> {
        if !self.is_time_set {
            self.update_time()?;
        }
        Ok(self.time)
    }

    fn internal_get_time(&self) -> f64 {
        self.time
    }

    pub fn get(&mut self, f: usize) -> R<i32> {
        if f >= NF {
            return Err(());
        }
        self.complete()?;
        Ok(self.fields[f])
    }

    pub fn complete(&mut self) -> R<()> {
        if !self.is_time_set {
            self.update_time()?;
        }
        if !self.are_fields_set {
            self.compute_fields()?;
            self.are_fields_set = true;
            self.are_all_fields_set = true;
        }
        Ok(())
    }

    fn update_time(&mut self) -> R<()> {
        self.compute_time()?;
        if self.lenient || !self.are_all_fields_set {
            self.are_fields_set = false;
        }
        self.is_time_set = true;
        self.are_fields_virtually_set = false;
        Ok(())
    }

    pub fn set_zone(&mut self, z: Rc<Zone>) {
        self.zone = z;
        self.are_fields_set = false;
    }

    pub fn set_lenient(&mut self, l: bool) {
        self.lenient = l;
    }

    pub fn set_first_day(&mut self, d: i32) {
        if self.first_day != d && (1..=7).contains(&d) {
            self.first_day = d;
            self.are_fields_set = false;
        }
    }

    pub fn set_min_days(&mut self, d: i32) {
        let d = d.clamp(1, 7);
        if self.min_days != d {
            self.min_days = d;
            self.are_fields_set = false;
        }
    }

    // ---- time -> fields ---------------------------------------------------

    fn compute_fields(&mut self) -> R<()> {
        let mut local = self.internal_get_time();
        let (raw, dst) = self.zone.offsets(local)?;
        local += (raw + dst) as f64;
        let mask: u32 = (1 << ERA)
            | (1 << YEAR)
            | (1 << MONTH)
            | (1 << DATE)
            | (1 << DAY_OF_YEAR)
            | (1 << EXTENDED_YEAR)
            | (1 << ORDINAL_MONTH);
        for i in 0..NF {
            if mask & (1 << i) == 0 {
                self.stamp[i] = INTERNALLY_SET;
            } else {
                self.stamp[i] = UNSET;
            }
        }
        let (days, mid) = floor_div_f(local, ONE_DAY);
        let mut mid = mid as i64;
        let jd = days as i64 + EPOCH_JD as i64;
        if jd < i32::MIN as i64 || jd > i32::MAX as i64 {
            return Err(());
        }
        let jd = jd as i32;
        self.internal_set(JULIAN_DAY, jd);
        self.compute_gregorian_and_dow_fields(jd);
        self.handle_compute_fields(jd);
        self.compute_week_fields();
        self.fields[MILLISECONDS_IN_DAY] = mid as i32;
        self.fields[MILLISECOND] = (mid % 1000) as i32;
        mid /= 1000;
        self.fields[SECOND] = (mid % 60) as i32;
        mid /= 60;
        self.fields[MINUTE] = (mid % 60) as i32;
        mid /= 60;
        self.fields[HOUR_OF_DAY] = mid as i32;
        self.fields[AM_PM] = (mid / 12) as i32;
        self.fields[HOUR] = (mid % 12) as i32;
        self.fields[ZONE_OFFSET] = raw;
        self.fields[DST_OFFSET] = dst;
        Ok(())
    }

    fn compute_gregorian_and_dow_fields(&mut self, jd: i32) {
        let days = jd as i64 - EPOCH_JD as i64;
        let (y, m, d) = civil_from_days(days);
        let doy = days - days_from_civil(y, 1, 1) + 1;
        self.g_year = y as i32;
        self.g_month = (m - 1) as i32;
        self.g_dom = d as i32;
        self.g_doy = doy as i32;
        let dow = jd_to_dow(jd as i64);
        self.internal_set(DAY_OF_WEEK, dow);
        let mut dl = dow - self.first_day + 1;
        if dl < 1 {
            dl += 7;
        }
        self.internal_set(DOW_LOCAL, dl);
    }

    /// GregorianCalendar::handleComputeFields(), and the other systems'.
    fn handle_compute_fields(&mut self, jd: i32) {
        if self.cal_type.is_gregorian_family() {
            let (eyear, month, dom, mut doy);
            if jd >= self.cutover_jd {
                month = self.g_month;
                dom = self.g_dom;
                doy = self.g_doy;
                eyear = self.g_year;
            } else {
                let jed = jd as i64 - (JAN1_1_JD - 2);
                let ey = floor_div(4 * jed + 1464, 1461);
                let jan1 = 365 * (ey - 1) + floor_div(ey - 1, 4);
                let d0 = jed - jan1;
                let leap = ey & 3 == 0;
                let mut corr = 0;
                let march1 = if leap { 60 } else { 59 };
                if d0 >= march1 {
                    corr = if leap { 1 } else { 2 };
                }
                let m = ((12 * (d0 + corr) + 6) / 367) as i32;
                let nd = if leap { LEAP_NUM_DAYS } else { NUM_DAYS };
                month = m;
                dom = (d0 - nd[m as usize] as i64 + 1) as i32;
                doy = (d0 + 1) as i32;
                eyear = ey as i32;
            }
            if eyear == self.cutover_year && jd >= self.cutover_jd {
                doy += gregorian_shift(eyear);
            }
            self.internal_set(MONTH, month);
            self.internal_set(ORDINAL_MONTH, month);
            self.internal_set(DATE, dom);
            self.internal_set(DAY_OF_YEAR, doy);
            self.internal_set(EXTENDED_YEAR, eyear);
            let (era, year) = self.gregorian_family_era_year(eyear, month, dom);
            self.internal_set(ERA, era);
            self.internal_set(YEAR, year);
            self.internal_set(IS_LEAP_MONTH, 0);
            return;
        }
        let f = sys_fields(self.cal_type, jd as i64, self.time);
        self.internal_set(MONTH, f.month);
        self.internal_set(ORDINAL_MONTH, f.ordinal_month);
        self.internal_set(DATE, f.dom);
        self.internal_set(DAY_OF_YEAR, f.doy);
        self.internal_set(EXTENDED_YEAR, f.eyear);
        self.internal_set(ERA, f.era);
        self.internal_set(YEAR, f.year);
        self.internal_set(IS_LEAP_MONTH, f.leap_month as i32);
    }

    fn gregorian_family_era_year(&self, eyear: i32, month: i32, dom: i32) -> (i32, i32) {
        match self.cal_type {
            CalType::Japanese => {
                let era = japanese_era_of(eyear, month + 1, dom);
                (era, eyear - japanese_era_start_year(era) + 1)
            }
            CalType::Buddhist => (0, eyear + 543),
            CalType::Roc => {
                if eyear > 1911 {
                    (1, eyear - 1911)
                } else {
                    (0, 1912 - eyear)
                }
            }
            _ => {
                if eyear < 1 {
                    (0, 1 - eyear)
                } else {
                    (1, eyear)
                }
            }
        }
    }

    fn compute_week_fields(&mut self) {
        let eyear = self.fields[EXTENDED_YEAR];
        let dow = self.fields[DAY_OF_WEEK];
        let doy = self.fields[DAY_OF_YEAR];
        let mut ywoy = eyear;
        let rel_dow = (dow + 7 - self.first_day) % 7;
        let rel_dow_jan1 = (dow - doy + 7001 - self.first_day).rem_euclid(7);
        let mut woy = (doy - 1 + rel_dow_jan1) / 7;
        if (7 - rel_dow_jan1) >= self.min_days {
            woy += 1;
        }
        if woy == 0 {
            let prev_doy = doy + self.year_length(eyear - 1);
            woy = self.week_number(prev_doy, prev_doy, dow);
            ywoy -= 1;
        } else {
            let last_doy = self.year_length(eyear);
            if doy >= last_doy - 5 {
                let mut last_rel = (rel_dow + last_doy - doy) % 7;
                if last_rel < 0 {
                    last_rel += 7;
                }
                if (6 - last_rel) >= self.min_days && (doy + 7 - rel_dow) > last_doy {
                    woy = 1;
                    ywoy += 1;
                }
            }
        }
        self.fields[WEEK_OF_YEAR] = woy;
        self.fields[YEAR_WOY] = ywoy;
        let dom = self.fields[DATE];
        self.fields[WEEK_OF_MONTH] = self.week_number(dom, dom, dow);
        self.fields[DAY_OF_WEEK_IN_MONTH] = (dom - 1) / 7 + 1;
    }

    fn week_number(&self, desired: i32, day_of_period: i32, dow: i32) -> i32 {
        let mut psd = (dow - self.first_day - day_of_period + 1) % 7;
        if psd < 0 {
            psd += 7;
        }
        let mut w = (desired + psd - 1) / 7;
        if (7 - psd) >= self.min_days {
            w += 1;
        }
        w
    }

    // ---- fields -> time ---------------------------------------------------

    fn newest_stamp(&self, first: usize, last: usize, best: i32) -> i32 {
        let mut b = best;
        for i in first..=last {
            if self.stamp[i] > b {
                b = self.stamp[i];
            }
        }
        b
    }

    fn newer_field(&self, a: usize, b: usize) -> usize {
        if self.stamp[b] > self.stamp[a] { b } else { a }
    }

    fn resolve_fields(&self, table: &[&[&[usize]]]) -> usize {
        let mut best_field = NF;
        for group in table {
            if best_field != NF {
                break;
            }
            let mut best_stamp = UNSET;
            'lines: for line in group.iter() {
                let mut line_stamp = UNSET;
                let start = if line[0] >= REMAP { 1 } else { 0 };
                for &f in &line[start..] {
                    let s = self.stamp[f];
                    if s == UNSET {
                        continue 'lines;
                    }
                    if s > line_stamp {
                        line_stamp = s;
                    }
                }
                if line_stamp > best_stamp {
                    let mut temp = line[0];
                    if temp >= REMAP {
                        temp &= REMAP - 1;
                        if temp != DATE || self.stamp[WEEK_OF_MONTH] < self.stamp[temp] {
                            best_field = temp;
                        }
                    } else {
                        best_field = temp;
                    }
                    if best_field == temp {
                        best_stamp = line_stamp;
                    }
                }
            }
        }
        best_field
    }

    fn compute_time(&mut self) -> R<()> {
        if !self.lenient {
            self.validate_fields()?;
        }
        let jd = self.compute_julian_day()?;
        let millis = (jd as f64 - EPOCH_JD as f64) * ONE_DAY;
        let mid = if self.stamp[MILLISECONDS_IN_DAY] >= MIN_USER_STAMP
            && self.newest_stamp(AM_PM, MILLISECOND, UNSET) <= self.stamp[MILLISECONDS_IN_DAY]
        {
            self.internal_get(MILLISECONDS_IN_DAY) as f64
        } else {
            self.compute_millis_in_day()
        };
        let t;
        if self.stamp[ZONE_OFFSET] >= MIN_USER_STAMP || self.stamp[DST_OFFSET] >= MIN_USER_STAMP {
            t = millis + mid
                - (self.internal_get(ZONE_OFFSET) as f64 + self.internal_get(DST_OFFSET) as f64);
        } else if !self.lenient || self.skipped_wall == WALLTIME_NEXT_VALID {
            let zo = self.compute_zone_offset(millis, mid);
            let tmp = millis + mid - zo as f64;
            let (raw, dst) = self.zone.offsets(tmp)?;
            if zo != raw + dst {
                if !self.lenient {
                    return Err(());
                }
                match self.zone.prev_transition(tmp, true) {
                    Some(p) => t = p,
                    None => t = tmp,
                }
            } else {
                t = tmp;
            }
        } else {
            t = millis + mid - self.compute_zone_offset(millis, mid) as f64;
        }
        self.time = t;
        Ok(())
    }

    fn compute_zone_offset(&self, millis: f64, mid: f64) -> i32 {
        let wall = millis + mid;
        let dup_former = self.repeated_wall == WALLTIME_FIRST;
        let nonexist_latter = self.skipped_wall == WALLTIME_FIRST;
        let (raw, dst) = self.zone.offsets_local(wall, nonexist_latter, dup_former);
        raw + dst
    }

    fn compute_millis_in_day(&self) -> f64 {
        let mut m = 0.0;
        let hod = self.stamp[HOUR_OF_DAY];
        let hs = self.stamp[HOUR].max(self.stamp[AM_PM]);
        let best = hs.max(hod);
        if best != UNSET {
            if best == hod {
                m += self.internal_get(HOUR_OF_DAY) as f64;
            } else {
                m += self.internal_get(HOUR) as f64;
                m += 12.0 * self.internal_get(AM_PM) as f64;
            }
        }
        m *= 60.0;
        m += self.internal_get(MINUTE) as f64;
        m *= 60.0;
        m += self.internal_get(SECOND) as f64;
        m *= 1000.0;
        m += self.internal_get(MILLISECOND) as f64;
        m
    }

    fn validate_fields(&mut self) -> R<()> {
        for f in 0..NF {
            if self.stamp[f] >= MIN_USER_STAMP {
                self.validate_field(f)?;
            }
        }
        Ok(())
    }

    fn validate_field(&mut self, f: usize) -> R<()> {
        let v = self.fields[f];
        let (min, max) = match f {
            DATE => {
                let y = self.handle_get_extended_year()?;
                let m = self.internal_get_month();
                (1, self.month_length(y, m))
            }
            DAY_OF_YEAR => {
                let y = self.handle_get_extended_year()?;
                (1, self.year_length(y))
            }
            DAY_OF_WEEK_IN_MONTH => {
                if v == 0 {
                    return Err(());
                }
                (self.get_minimum(f), self.get_maximum(f))
            }
            _ => (self.get_minimum(f), self.get_maximum(f)),
        };
        if v < min || v > max {
            return Err(());
        }
        Ok(())
    }

    fn internal_get_month(&self) -> i32 {
        if self.resolve_fields(MONTH_PRECEDENCE) == ORDINAL_MONTH {
            let o = self.internal_get(ORDINAL_MONTH);
            if self.cal_type == CalType::Hebrew {
                let y = self.fields[EXTENDED_YEAR];
                if !hebrew_is_leap(y as i64) && o >= 5 {
                    return o + 1;
                }
            }
            return o;
        }
        self.internal_get(MONTH)
    }

    fn compute_julian_day(&mut self) -> R<i64> {
        if self.stamp[JULIAN_DAY] >= MIN_USER_STAMP {
            let mut best = self.newest_stamp(ERA, DAY_OF_WEEK_IN_MONTH, UNSET);
            best = self.newest_stamp(YEAR_WOY, EXTENDED_YEAR, best);
            best = self.newest_stamp(ORDINAL_MONTH, ORDINAL_MONTH, best);
            if best <= self.stamp[JULIAN_DAY] {
                return Ok(self.internal_get(JULIAN_DAY) as i64);
            }
        }
        let mut best = self.resolve_fields(DATE_PRECEDENCE);
        if best == NF {
            best = DATE;
        }
        if self.cal_type.is_gregorian_family() {
            self.invert_gregorian = false;
            let mut jd = self.base_compute_julian_day(best)?;
            if best == WEEK_OF_YEAR
                && self.internal_get(EXTENDED_YEAR) == self.cutover_year
                && jd >= self.cutover_jd as i64
            {
                self.invert_gregorian = true;
                return self.base_compute_julian_day(best);
            }
            if self.is_gregorian != (jd >= self.cutover_jd as i64) {
                self.invert_gregorian = true;
                jd = self.base_compute_julian_day(best)?;
            }
            if self.is_gregorian && self.internal_get(EXTENDED_YEAR) == self.cutover_year {
                let shift = gregorian_shift(self.internal_get(EXTENDED_YEAR));
                if best == DAY_OF_YEAR {
                    jd -= shift as i64;
                } else if best == WEEK_OF_MONTH {
                    jd += 14;
                }
            }
            return Ok(jd);
        }
        self.base_compute_julian_day(best)
    }

    fn get_local_dow(&self) -> i32 {
        let mut d = match self.resolve_fields(DOW_PRECEDENCE) {
            DAY_OF_WEEK => self.internal_get(DAY_OF_WEEK) - self.first_day,
            DOW_LOCAL => self.internal_get(DOW_LOCAL) - 1,
            _ => 0,
        };
        d %= 7;
        if d < 0 {
            d += 7;
        }
        d
    }

    /// Calendar::handleComputeJulianDay()
    fn base_compute_julian_day(&mut self, best: usize) -> R<i64> {
        let use_month = best == DATE || best == WEEK_OF_MONTH || best == DAY_OF_WEEK_IN_MONTH;
        let year = if best == WEEK_OF_YEAR && self.newer_field(YEAR_WOY, YEAR) == YEAR_WOY {
            self.internal_get(YEAR_WOY)
        } else {
            self.handle_get_extended_year()?
        };
        self.internal_set(EXTENDED_YEAR, year);
        let month = if self.is_set(MONTH) || self.is_set(ORDINAL_MONTH) {
            self.internal_get_month()
        } else {
            0
        };
        let mut jd = self.month_start(year, if use_month { month } else { 0 })?;
        if best == DATE {
            let dom = if self.is_set(DATE) {
                self.internal_get_def(DATE, 1)
            } else {
                1
            };
            return Ok(jd + dom as i64);
        }
        if best == DAY_OF_YEAR {
            return Ok(jd + self.internal_get(DAY_OF_YEAR) as i64);
        }
        let fdw = self.first_day;
        let mut first = jd_to_dow(jd + 1) - fdw;
        if first < 0 {
            first += 7;
        }
        let dow_local = self.get_local_dow();
        let mut date = 1 - first + dow_local;
        if best == DAY_OF_WEEK_IN_MONTH {
            if date < 1 {
                date += 7;
            }
            let dim = self.internal_get_def(DAY_OF_WEEK_IN_MONTH, 1);
            if dim >= 0 {
                date += 7 * (dim - 1);
            } else {
                let m = self.internal_get_month();
                let ml = self.month_length(year, m);
                date += ((ml - date) / 7 + dim + 1) * 7;
            }
        } else {
            if best == WEEK_OF_YEAR
                && (!self.is_set(YEAR_WOY)
                    || (self.resolve_fields(YEAR_PRECEDENCE) != YEAR_WOY
                        && self.stamp[YEAR_WOY] != INTERNALLY_SET))
            {
                let woy = self.internal_get(best);
                let next_jd = self.month_start(year + 1, 0)?;
                let mut next_first = jd_to_dow(next_jd + 1) - fdw;
                if next_first < 0 {
                    next_first += 7;
                }
                if woy == 1 {
                    if next_first > 0 && (7 - next_first) >= self.min_days {
                        jd = next_jd;
                        first = jd_to_dow(jd + 1) - fdw;
                        if first < 0 {
                            first += 7;
                        }
                        date = 1 - first + dow_local;
                    }
                } else if woy >= self.get_least_maximum(best) {
                    let mut test = date;
                    if (7 - first) < self.min_days {
                        test += 7;
                    }
                    test += 7 * (woy - 1);
                    if jd + test as i64 > next_jd {
                        jd = self.month_start(year - 1, 0)?;
                        first = jd_to_dow(jd + 1) - fdw;
                        if first < 0 {
                            first += 7;
                        }
                        date = 1 - first + dow_local;
                    }
                }
            }
            if (7 - first) < self.min_days {
                date += 7;
            }
            date += 7 * (self.internal_get(best) - 1);
        }
        Ok(jd + date as i64)
    }

    fn handle_get_extended_year(&mut self) -> R<i32> {
        if matches!(self.cal_type, CalType::Gregorian | CalType::Iso8601) {
            let mut yf = EXTENDED_YEAR;
            if self.stamp[yf] < self.stamp[YEAR] {
                yf = YEAR;
            }
            if self.stamp[yf] < self.stamp[YEAR_WOY] {
                yf = YEAR_WOY;
            }
            return match yf {
                EXTENDED_YEAR => Ok(self.internal_get_def(EXTENDED_YEAR, 1970)),
                YEAR => self.year_from_era(),
                _ => {
                    let y = self.internal_get(YEAR_WOY);
                    let w = self.internal_get(WEEK_OF_YEAR);
                    self.ext_year_from_week_fields(y, w)
                }
            };
        }
        let ext_newer = self.newer_field(EXTENDED_YEAR, YEAR) == EXTENDED_YEAR;
        match self.cal_type {
            CalType::Japanese => {
                if ext_newer && self.newer_field(EXTENDED_YEAR, ERA) == EXTENDED_YEAR {
                    Ok(self.internal_get_def(EXTENDED_YEAR, 1970))
                } else {
                    self.year_from_era()
                }
            }
            CalType::Buddhist if ext_newer => Ok(self.internal_get_def(EXTENDED_YEAR, 1970)),
            _ if ext_newer => Ok(self.internal_get_def(EXTENDED_YEAR, 1)),
            _ => self.year_from_era(),
        }
    }

    /// The extended year of the ERA and YEAR fields.
    fn year_from_era(&self) -> R<i32> {
        match self.cal_type {
            CalType::Gregorian | CalType::Iso8601 => {
                let era = self.internal_get_def(ERA, 1);
                match era {
                    0 => Ok(1 - self.internal_get_def(YEAR, 1)),
                    1 => Ok(self.internal_get_def(YEAR, 1970)),
                    _ => Err(()),
                }
            }
            CalType::Japanese => {
                let era = self.internal_get_def(ERA, current_japanese_era());
                Ok(self.internal_get_def(YEAR, 1) + japanese_era_start_year(era) - 1)
            }
            CalType::Buddhist => Ok(self.internal_get_def(YEAR, 1970 + 543) - 543),
            CalType::Roc => {
                let era = self.internal_get_def(ERA, 1);
                let y = self.internal_get_def(YEAR, 1);
                Ok(if era == 1 { y + 1911 } else { 1912 - y })
            }
            CalType::EthiopicAmeteAlem => Ok(self.internal_get_def(YEAR, 1 + 5500) - 5500),
            CalType::Coptic | CalType::Ethiopic => {
                let era = self.internal_get_def(ERA, 1);
                let y = self.internal_get_def(YEAR, 1);
                if self.cal_type == CalType::Ethiopic && era == 0 {
                    Ok(y - 5500)
                } else if self.cal_type == CalType::Coptic && era == 0 {
                    Ok(1 - y)
                } else {
                    Ok(y)
                }
            }
            CalType::Chinese | CalType::Dangi => {
                let cycle = self.internal_get_def(ERA, 1);
                let y = self.internal_get_def(YEAR, 1);
                let off = if self.cal_type == CalType::Dangi {
                    2637 - 2333
                } else {
                    0
                };
                Ok((cycle - 1) * 60 + y - off)
            }
            CalType::Persian | CalType::Indian | CalType::Hebrew => {
                Ok(self.internal_get_def(YEAR, 1))
            }
            _ => Ok(self.internal_get_def(YEAR, 1)),
        }
    }

    fn ext_year_from_week_fields(&mut self, ywoy: i32, woy: i32) -> R<i32> {
        let best = self.resolve_fields(DATE_PRECEDENCE);
        let dow_local = self.get_local_dow();
        let fdw = self.first_day;
        let jan1 = self.month_start(ywoy, 0)?;
        let next_jan1 = self.month_start(ywoy + 1, 0)?;
        let mut first = jd_to_dow(jan1 + 1) - fdw;
        if first < 0 {
            first += 7;
        }
        let jan1_in_prev = (7 - first) < self.min_days;
        match best {
            WEEK_OF_YEAR => {
                if woy == 1 {
                    if jan1_in_prev {
                        Ok(ywoy)
                    } else if dow_local < first {
                        Ok(ywoy - 1)
                    } else {
                        Ok(ywoy)
                    }
                } else if woy >= self.get_least_maximum(WEEK_OF_YEAR) {
                    let mut jd =
                        jan1 + (7 - first) as i64 + (woy as i64 - 1) * 7 + dow_local as i64;
                    if !jan1_in_prev {
                        jd -= 7;
                    }
                    if jd + 1 >= next_jan1 {
                        Ok(ywoy + 1)
                    } else {
                        Ok(ywoy)
                    }
                } else {
                    Ok(ywoy)
                }
            }
            DATE => {
                let m = self.internal_get_month();
                if m == 0 && woy >= self.get_least_maximum(WEEK_OF_YEAR) {
                    Ok(ywoy + 1)
                } else if woy == 1 {
                    if m == 0 { Ok(ywoy) } else { Ok(ywoy - 1) }
                } else {
                    Ok(ywoy)
                }
            }
            _ => Ok(ywoy),
        }
    }

    /// handleComputeMonthStart(): the JD of the day before the first of
    /// `month` (0-based, may be out of range) of extended year `eyear`.
    fn month_start(&mut self, eyear: i32, month: i32) -> R<i64> {
        if self.cal_type.is_gregorian_family() {
            let mut eyear = eyear as i64;
            let mut month = month as i64;
            if !(0..=11).contains(&month) {
                eyear += floor_div(month, 12);
                month = month.rem_euclid(12);
            }
            if eyear < i32::MIN as i64 || eyear > i32::MAX as i64 {
                return Err(());
            }
            let mut leap = eyear % 4 == 0;
            let y = eyear - 1;
            let mut jd = 365 * y + floor_div(y, 4) + (JAN1_1_JD - 3);
            self.is_gregorian = eyear >= self.cutover_year as i64;
            if self.invert_gregorian {
                self.is_gregorian = !self.is_gregorian;
            }
            if self.is_gregorian {
                leap = leap && (eyear % 100 != 0 || eyear % 400 == 0);
                jd += gregorian_shift(eyear as i32) as i64;
            }
            if month != 0 {
                jd += if leap {
                    LEAP_NUM_DAYS[month as usize]
                } else {
                    NUM_DAYS[month as usize]
                } as i64;
            }
            return Ok(jd);
        }
        Ok(sys_month_start(self.cal_type, eyear as i64, month as i64))
    }

    pub fn month_length(&self, eyear: i32, month: i32) -> i32 {
        if self.cal_type.is_gregorian_family() {
            let mut eyear = eyear as i64;
            let mut month = month as i64;
            if !(0..=11).contains(&month) {
                eyear += floor_div(month, 12);
                month = month.rem_euclid(12);
            }
            return if self.is_leap_year(eyear as i32) {
                LEAP_MONTH_LEN[month as usize]
            } else {
                MONTH_LEN[month as usize]
            };
        }
        if self.cal_type == CalType::Hebrew {
            return hebrew_month_length(eyear as i64, month as i64);
        }
        let a = sys_month_start(self.cal_type, eyear as i64, month as i64);
        let b = sys_month_start(self.cal_type, eyear as i64, month as i64 + 1);
        (b - a) as i32
    }

    pub fn year_length(&self, eyear: i32) -> i32 {
        if self.cal_type.is_gregorian_family() {
            return if self.is_leap_year(eyear) { 366 } else { 365 };
        }
        if self.cal_type == CalType::Hebrew {
            let a = sys_month_start(self.cal_type, eyear as i64, 0);
            let b = sys_month_start(self.cal_type, eyear as i64 + 1, 0);
            return (b - a) as i32;
        }
        let n = sys_months_in_year(self.cal_type, eyear as i64);
        let a = sys_month_start(self.cal_type, eyear as i64, 0);
        let b = sys_month_start(self.cal_type, eyear as i64, n);
        (b - a) as i32
    }

    /// GregorianCalendar::isLeapYear()
    pub fn is_leap_year(&self, y: i32) -> bool {
        if y >= self.cutover_year {
            y & 3 == 0 && (y % 100 != 0 || y % 400 == 0)
        } else {
            y & 3 == 0
        }
    }

    /// setGregorianChange()
    pub fn set_gregorian_change(&mut self, date: f64) -> R<()> {
        if !self.cal_type.is_gregorian_family() {
            return Ok(());
        }
        if date.is_nan() {
            return Err(());
        }
        let mut cutover = date;
        let (cutover_jd, cutover_year);
        if date <= MIN_MILLIS {
            cutover = MIN_MILLIS;
            cutover_jd = i32::MIN;
            cutover_year = i32::MIN;
        } else if date >= MAX_MILLIS {
            cutover = MAX_MILLIS;
            cutover_jd = i32::MAX;
            cutover_year = i32::MAX;
        } else {
            let days = (date / ONE_DAY).floor();
            cutover_jd = (days + EPOCH_JD as f64) as i32;
            // the Gregorian year of the cutover
            let mut tmp = self.clone();
            tmp.zone = Rc::new(Zone::fixed("GMT", 0));
            tmp.set_time(date)?;
            tmp.complete()?;
            let mut y = tmp.fields[EXTENDED_YEAR];
            // ICU computes this year with the new cutover in effect
            let (gy, _, _) = civil_from_days(days as i64);
            if (days as i64 + EPOCH_JD as i64) >= cutover_jd as i64 {
                y = gy as i32;
            }
            cutover_year = y;
        }
        self.cutover = cutover;
        self.cutover_jd = cutover_jd;
        self.cutover_year = cutover_year;
        Ok(())
    }

    // ---- limits -----------------------------------------------------------

    fn limit(&self, f: usize, kind: usize) -> i32 {
        match f {
            DAY_OF_WEEK | AM_PM | HOUR | HOUR_OF_DAY | MINUTE | SECOND | MILLISECOND
            | ZONE_OFFSET | DST_OFFSET | DOW_LOCAL | JULIAN_DAY | MILLISECONDS_IN_DAY
            | IS_LEAP_MONTH => CALENDAR_LIMITS[f][kind],
            WEEK_OF_MONTH => {
                if kind == 0 {
                    if self.min_days == 1 { 1 } else { 0 }
                } else if kind == 1 {
                    1
                } else {
                    let dim = self.handle_limit(DATE, kind);
                    if kind == 2 {
                        (dim + (7 - self.min_days)) / 7
                    } else {
                        (dim + 6 + (7 - self.min_days)) / 7
                    }
                }
            }
            _ => self.handle_limit(f, kind),
        }
    }

    fn handle_limit(&self, f: usize, kind: usize) -> i32 {
        type_limits(self.cal_type, f)[kind]
    }

    pub fn get_minimum(&self, f: usize) -> i32 {
        self.limit(f, 0)
    }
    pub fn get_greatest_minimum(&self, f: usize) -> i32 {
        self.limit(f, 1)
    }
    pub fn get_least_maximum(&self, f: usize) -> i32 {
        self.limit(f, 2)
    }
    pub fn get_maximum(&self, f: usize) -> i32 {
        self.limit(f, 3)
    }

    pub fn get_actual_minimum(&mut self, f: usize) -> R<i32> {
        if self.cal_type.is_gregorian_family() {
            return Ok(self.get_minimum(f));
        }
        let mut fv = self.get_greatest_minimum(f);
        let end = self.get_minimum(f);
        if fv == end {
            return Ok(fv);
        }
        let mut work = self.clone();
        work.set_lenient(true);
        let mut result = fv;
        loop {
            work.set(f, fv);
            if work.get(f)? != fv {
                break;
            }
            result = fv;
            fv -= 1;
            if fv < end {
                break;
            }
        }
        Ok(result)
    }

    pub fn get_actual_maximum(&mut self, f: usize) -> R<i32> {
        if f == YEAR && self.cal_type == CalType::Japanese {
            let era = self.get(ERA)?;
            let eras = super::data::JAPANESE_ERA_STARTS;
            if era >= current_japanese_era() {
                return Ok(type_limits(self.cal_type, YEAR)[3]);
            }
            let (ny, nm, nd) = eras[(era + 1) as usize];
            let mut max = ny as i32 - japanese_era_start_year(era) + 1;
            if nm == 1 && nd == 1 {
                max -= 1;
            }
            return Ok(max);
        }
        if f == YEAR && self.cal_type.is_gregorian_family() {
            let mut cal = self.clone();
            cal.set_lenient(true);
            let era = cal.get(ERA)?;
            let d = cal.get_time()?;
            let mut low = type_limits(self.cal_type, YEAR)[1];
            let mut high = type_limits(self.cal_type, YEAR)[2] + 1;
            while low + 1 < high {
                let y = (low + high) / 2;
                cal.set(YEAR, y);
                if cal.get(YEAR)? == y && cal.get(ERA)? == era {
                    low = y;
                } else {
                    high = y;
                    cal.set_time(d)?;
                }
            }
            return Ok(low);
        }
        match f {
            DATE => {
                let mut cal = self.clone();
                cal.set_lenient(true);
                cal.prepare_get_actual(f, false)?;
                let y = cal.get(EXTENDED_YEAR)?;
                let m = cal.get(MONTH)?;
                Ok(self.month_length(y, m))
            }
            DAY_OF_YEAR => {
                let mut cal = self.clone();
                cal.set_lenient(true);
                cal.prepare_get_actual(f, false)?;
                let y = cal.get(EXTENDED_YEAR)?;
                Ok(self.year_length(y))
            }
            DAY_OF_WEEK | AM_PM | HOUR | HOUR_OF_DAY | MINUTE | SECOND | MILLISECOND
            | ZONE_OFFSET | DST_OFFSET | DOW_LOCAL | JULIAN_DAY | MILLISECONDS_IN_DAY => {
                Ok(self.get_maximum(f))
            }
            ORDINAL_MONTH => {
                if self.cal_type == CalType::Hebrew {
                    let y = self.get(EXTENDED_YEAR)?;
                    Ok(if hebrew_is_leap(y as i64) { 12 } else { 11 })
                } else {
                    Ok(self.get_least_maximum(ORDINAL_MONTH))
                }
            }
            _ => {
                let lm = self.get_least_maximum(f);
                let mx = self.get_maximum(f);
                self.get_actual_helper(f, lm, mx)
            }
        }
    }

    fn prepare_get_actual(&mut self, f: usize, is_min: bool) -> R<()> {
        self.set(MILLISECONDS_IN_DAY, 0);
        match f {
            YEAR | EXTENDED_YEAR => {
                let v = self.get_greatest_minimum(DAY_OF_YEAR);
                self.set(DAY_OF_YEAR, v);
            }
            YEAR_WOY | MONTH => {
                if f == YEAR_WOY {
                    let v = self.get_greatest_minimum(WEEK_OF_YEAR);
                    self.set(WEEK_OF_YEAR, v);
                }
                let v = self.get_greatest_minimum(DATE);
                self.set(DATE, v);
            }
            DAY_OF_WEEK_IN_MONTH => {
                self.set(DATE, 1);
                let d = self.get(DAY_OF_WEEK)?;
                self.set(DAY_OF_WEEK, d);
            }
            WEEK_OF_MONTH | WEEK_OF_YEAR => {
                let mut dow = self.first_day;
                if is_min {
                    dow = (dow + 6) % 7;
                    if dow < 1 {
                        dow += 7;
                    }
                }
                self.set(DAY_OF_WEEK, dow);
            }
            _ => {}
        }
        let v = self.get_greatest_minimum(f);
        self.set(f, v);
        Ok(())
    }

    fn get_actual_helper(&mut self, f: usize, start: i32, end: i32) -> R<i32> {
        if start == end {
            return Ok(start);
        }
        let delta = if end > start { 1 } else { -1 };
        let mut work = self.clone();
        work.complete()?;
        work.set_lenient(true);
        work.prepare_get_actual(f, delta < 0)?;
        work.set(f, start);
        let mut result = start;
        let mut sv = start;
        let g = work.get(f);
        if (g.map(|v| v != start).unwrap_or(true) && f != WEEK_OF_MONTH && delta > 0) || g.is_err()
        {
        } else {
            loop {
                sv += delta;
                if work.add(f, delta).is_err() {
                    break;
                }
                match work.get(f) {
                    Ok(v) if v == sv => {}
                    _ => break,
                }
                result = sv;
                if sv == end {
                    break;
                }
            }
        }
        Ok(result)
    }

    fn pin_field(&mut self, f: usize) -> R<()> {
        let max = self.get_actual_maximum(f)?;
        let min = self.get_actual_minimum(f)?;
        if self.fields[f] > max {
            self.set(f, max);
        } else if self.fields[f] < min {
            self.set(f, min);
        }
        Ok(())
    }

    // ---- arithmetic -------------------------------------------------------

    fn era0_years_go_backwards(&self) -> bool {
        matches!(
            self.cal_type,
            CalType::Gregorian | CalType::Iso8601 | CalType::Roc | CalType::Coptic
        )
    }

    pub fn add(&mut self, f: usize, amount: i32) -> R<()> {
        if amount == 0 {
            return Ok(());
        }
        let mut amount = amount;
        let mut delta = amount as f64;
        let mut keep_wall = true;
        match f {
            ERA => {
                let v = self.get(ERA)?.checked_add(amount).ok_or(())?;
                self.set(ERA, v);
                return self.pin_field(ERA);
            }
            MONTH | ORDINAL_MONTH if self.cal_type == CalType::Hebrew => {
                let mut month = self.get(MONTH)?;
                let mut year = self.get(YEAR)?;
                if amount > 0 {
                    let mut across = month < 5;
                    month = month.checked_add(amount).ok_or(())?;
                    loop {
                        if across && month >= 5 && !hebrew_is_leap(year as i64) {
                            month += 1;
                        }
                        if month <= 12 {
                            break;
                        }
                        month -= 13;
                        year += 1;
                        across = true;
                    }
                } else {
                    let mut across = month > 5;
                    month = month.checked_add(amount).ok_or(())?;
                    loop {
                        if across && month <= 5 && !hebrew_is_leap(year as i64) {
                            month -= 1;
                        }
                        if month >= 0 {
                            break;
                        }
                        month += 13;
                        year -= 1;
                        across = true;
                    }
                }
                self.set(MONTH, month);
                self.set(YEAR, year);
                return self.pin_field(DATE);
            }
            YEAR | YEAR_WOY | EXTENDED_YEAR | MONTH | ORDINAL_MONTH => {
                if f == YEAR || f == YEAR_WOY {
                    let era = self.get(ERA)?;
                    if era == 0 && self.era0_years_go_backwards() {
                        amount = -amount;
                    }
                }
                let old = self.lenient;
                self.set_lenient(true);
                let v = self.get(f)?.checked_add(amount).ok_or(())?;
                self.set(f, v);
                let r = self.pin_field(DATE);
                if !old {
                    let r2 = self.complete();
                    self.set_lenient(old);
                    r2?;
                }
                return r;
            }
            WEEK_OF_YEAR | WEEK_OF_MONTH | DAY_OF_WEEK_IN_MONTH => delta *= ONE_WEEK,
            AM_PM => delta *= 12.0 * ONE_HOUR,
            DATE | DAY_OF_YEAR | DAY_OF_WEEK | DOW_LOCAL | JULIAN_DAY => delta *= ONE_DAY,
            HOUR_OF_DAY | HOUR => {
                delta *= ONE_HOUR;
                keep_wall = false;
            }
            MINUTE => {
                delta *= 60_000.0;
                keep_wall = false;
            }
            SECOND => {
                delta *= 1000.0;
                keep_wall = false;
            }
            MILLISECOND | MILLISECONDS_IN_DAY => keep_wall = false,
            _ => return Err(()),
        }
        let mut prev_offset = 0;
        let mut prev_wall = 0;
        if keep_wall {
            prev_offset = self.get(DST_OFFSET)? + self.get(ZONE_OFFSET)?;
            prev_wall = self.get(MILLISECONDS_IN_DAY)?;
        }
        let t0 = self.get_time()?;
        self.set_time(t0 + delta)?;
        if keep_wall {
            let mut new_wall = self.get(MILLISECONDS_IN_DAY)?;
            if new_wall != prev_wall {
                let t = self.internal_get_time();
                let new_offset = self.get(DST_OFFSET)? + self.get(ZONE_OFFSET)?;
                if new_offset != prev_offset {
                    let adj = prev_offset - new_offset;
                    let adj = if adj >= 0 {
                        adj % 86_400_000
                    } else {
                        -(-adj % 86_400_000)
                    };
                    if adj != 0 {
                        self.set_time(t + adj as f64)?;
                        new_wall = self.get(MILLISECONDS_IN_DAY)?;
                    }
                    if new_wall != prev_wall {
                        match self.skipped_wall {
                            WALLTIME_FIRST => {
                                if adj > 0 {
                                    self.set_time(t)?;
                                }
                            }
                            WALLTIME_LAST => {
                                if adj < 0 {
                                    self.set_time(t)?;
                                }
                            }
                            _ => {
                                let tmp = if adj > 0 { self.internal_get_time() } else { t };
                                if let Some(p) = self.zone.prev_transition(tmp, true) {
                                    self.set_time(p)?;
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn roll(&mut self, f: usize, amount: i32) -> R<()> {
        if amount == 0 {
            return Ok(());
        }
        self.complete()?;
        // GregorianCalendar::roll(): the month containing the cutover
        if (f == DATE || f == WEEK_OF_MONTH)
            && self.cal_type.is_gregorian_family()
            && self.get(EXTENDED_YEAR)? == self.cutover_year
        {
            let max = self.month_length(self.internal_get(EXTENDED_YEAR), self.internal_get(MONTH));
            let t = self.internal_get_time();
            let c_dom = self.internal_get(DATE) - if t >= self.cutover { 10 } else { 0 };
            let c_month_start = t - (c_dom as f64 - 1.0) * ONE_DAY;
            let c_month_len = max - 10;
            if c_month_start < self.cutover
                && c_month_start + c_month_len as f64 * ONE_DAY >= self.cutover
            {
                if f == DATE {
                    let month_len = c_month_len as f64 * ONE_DAY;
                    let mut ms = (t - c_month_start + amount as f64 * ONE_DAY) % month_len;
                    if ms < 0.0 {
                        ms += month_len;
                    }
                    return self.set_time(c_month_start + ms);
                }
                let mut dow = self.internal_get(DAY_OF_WEEK) - self.first_day;
                if dow < 0 {
                    dow += 7;
                }
                let dom = c_dom;
                let mut fdm = (dow - dom + 1) % 7;
                if fdm < 0 {
                    fdm += 7;
                }
                let start = if (7 - fdm) < self.min_days {
                    8 - fdm
                } else {
                    1 - fdm
                };
                let month_len = c_month_len;
                let ldm = (month_len - dom + dow) % 7;
                let limit = month_len + 7 - ldm;
                let gap = (limit - start) as i64;
                let mut nd = (dom as i64 + amount as i64 * 7 - start as i64) % gap;
                if nd < 0 {
                    nd += gap;
                }
                nd += start as i64;
                let nd = nd.clamp(1, month_len as i64);
                return self.set_time(c_month_start + (nd as f64 - 1.0) * ONE_DAY);
            }
        }
        if f == WEEK_OF_YEAR && self.cal_type.is_gregorian_family() {
            let mut woy = self.get(WEEK_OF_YEAR)?;
            let iso_year = self.get(YEAR_WOY)?;
            let mut iso_doy = self.internal_get(DAY_OF_YEAR);
            if self.internal_get(MONTH) == 0 {
                if woy >= 52 {
                    iso_doy += self.year_length(iso_year);
                }
            } else if woy == 1 {
                iso_doy -= self.year_length(iso_year - 1);
            }
            woy += amount;
            if !(1..=52).contains(&woy) {
                let mut last_doy = self.year_length(iso_year);
                let mut last_rel =
                    (last_doy - iso_doy + self.internal_get(DAY_OF_WEEK) - self.first_day) % 7;
                if last_rel < 0 {
                    last_rel += 7;
                }
                if (6 - last_rel) >= self.min_days {
                    last_doy -= 7;
                }
                let last_woy = self.week_number(last_doy, last_doy, last_rel + 1);
                woy = ((woy + last_woy - 1).rem_euclid(last_woy)) + 1;
            }
            self.set(WEEK_OF_YEAR, woy);
            self.set(YEAR_WOY, iso_year);
            return Ok(());
        }
        match f {
            DATE | AM_PM | MINUTE | SECOND | MILLISECOND | MILLISECONDS_IN_DAY | ERA => {
                let min = self.get_actual_minimum(f)?;
                let max = self.get_actual_maximum(f)?;
                let gap = (max - min + 1) as i64;
                let mut v = (self.internal_get(f) as i64 + amount as i64 - min as i64) % gap;
                if v < 0 {
                    v += gap;
                }
                v += min as i64;
                self.set(f, v as i32);
                Ok(())
            }
            HOUR | HOUR_OF_DAY => {
                let start = self.get_time()?;
                let old = self.internal_get(f) as i64;
                let max = self.get_maximum(f) as i64;
                let mut nh = (old + amount as i64) % (max + 1);
                if nh < 0 {
                    nh += max + 1;
                }
                self.set_time(start + ONE_HOUR * (nh - old) as f64)
            }
            MONTH | ORDINAL_MONTH if self.cal_type == CalType::Hebrew => {
                let month = self.get(MONTH)?;
                let year = self.get(YEAR)?;
                let leap = hebrew_is_leap(year as i64);
                let ylen = if leap { 13 } else { 12 };
                let mut nm = month + (amount % ylen);
                if !leap {
                    if amount > 0 && month < 5 && nm >= 5 {
                        nm += 1;
                    } else if amount < 0 && month > 5 && nm <= 5 {
                        nm -= 1;
                    }
                }
                self.set(MONTH, (nm + 13) % 13);
                self.pin_field(DATE)
            }
            MONTH | ORDINAL_MONTH => {
                let max = self.get_actual_maximum(MONTH)? as i64 + 1;
                let mut mon = (self.internal_get(MONTH) as i64 + amount as i64) % max;
                if mon < 0 {
                    mon += max;
                }
                self.set(MONTH, mon as i32);
                self.pin_field(DATE)
            }
            YEAR | YEAR_WOY => {
                let mut amount = amount;
                let mut era0_back = false;
                let era = self.get(ERA)?;
                if era == 0 && self.era0_years_go_backwards() {
                    amount = -amount;
                    era0_back = true;
                }
                let mut ny = self.internal_get(f).wrapping_add(amount);
                if era > 0 || ny >= 1 {
                    let max_year = self.get_actual_maximum(f)?;
                    if max_year < 32768 {
                        if ny < 1 {
                            ny = max_year - ((-ny) % max_year);
                        } else if ny > max_year {
                            ny = ((ny - 1) % max_year) + 1;
                        }
                    } else if ny < 1 {
                        ny = 1;
                    }
                } else if era0_back {
                    ny = 1;
                }
                self.set(f, ny);
                self.pin_field(MONTH)?;
                self.pin_field(DATE)
            }
            EXTENDED_YEAR => {
                let v = self.internal_get(f).wrapping_add(amount);
                self.set(f, v);
                self.pin_field(MONTH)?;
                self.pin_field(DATE)
            }
            WEEK_OF_MONTH => {
                let mut dow = self.internal_get(DAY_OF_WEEK) - self.first_day;
                if dow < 0 {
                    dow += 7;
                }
                let mut fdm = (dow - self.internal_get(DATE) + 1) % 7;
                if fdm < 0 {
                    fdm += 7;
                }
                let start = if (7 - fdm) < self.min_days {
                    8 - fdm
                } else {
                    1 - fdm
                };
                let month_len = self.get_actual_maximum(DATE)?;
                let ldm = (month_len - self.internal_get(DATE) + dow) % 7;
                let limit = month_len + 7 - ldm;
                let gap = (limit - start) as i64;
                let mut dom =
                    (self.internal_get(DATE) as i64 + amount as i64 * 7 - start as i64) % gap;
                if dom < 0 {
                    dom += gap;
                }
                dom += start as i64;
                let dom = dom.clamp(1, month_len as i64);
                self.set(DATE, dom as i32);
                Ok(())
            }
            WEEK_OF_YEAR => {
                let mut dow = self.internal_get(DAY_OF_WEEK) - self.first_day;
                if dow < 0 {
                    dow += 7;
                }
                let mut fdy = (dow - self.internal_get(DAY_OF_YEAR) + 1) % 7;
                if fdy < 0 {
                    fdy += 7;
                }
                let start = if (7 - fdy) < self.min_days {
                    8 - fdy
                } else {
                    1 - fdy
                };
                let year_len = self.get_actual_maximum(DAY_OF_YEAR)?;
                let ldy = (year_len - self.internal_get(DAY_OF_YEAR) + dow) % 7;
                let limit = year_len + 7 - ldy;
                let gap = (limit - start) as i64;
                let mut doy = (self.internal_get(DAY_OF_YEAR) as i64 + amount as i64 * 7
                    - start as i64)
                    % gap;
                if doy < 0 {
                    doy += gap;
                }
                doy += start as i64;
                let doy = doy.clamp(1, year_len as i64);
                self.set(DAY_OF_YEAR, doy as i32);
                self.clear(MONTH);
                Ok(())
            }
            DAY_OF_YEAR => {
                let delta = amount as f64 * ONE_DAY;
                let min2 = self.internal_get_time()
                    - (self.internal_get(DAY_OF_YEAR) as f64 - 1.0) * ONE_DAY;
                let one_year = self.get_actual_maximum(DAY_OF_YEAR)? as f64 * ONE_DAY;
                let mut nt = (self.internal_get_time() + delta - min2) % one_year;
                if nt < 0.0 {
                    nt += one_year;
                }
                self.set_time(nt + min2)
            }
            DAY_OF_WEEK | DOW_LOCAL => {
                let delta = amount as f64 * ONE_DAY;
                let mut lead = self.internal_get(f);
                lead -= if f == DAY_OF_WEEK { self.first_day } else { 1 };
                if lead < 0 {
                    lead += 7;
                }
                let min2 = self.internal_get_time() - lead as f64 * ONE_DAY;
                let mut nt = (self.internal_get_time() + delta - min2) % ONE_WEEK;
                if nt < 0.0 {
                    nt += ONE_WEEK;
                }
                self.set_time(nt + min2)
            }
            DAY_OF_WEEK_IN_MONTH => {
                let delta = amount as f64 * ONE_WEEK;
                let pre = (self.internal_get(DATE) - 1) / 7;
                let post = (self.get_actual_maximum(DATE)? - self.internal_get(DATE)) / 7;
                let min2 = self.internal_get_time() - pre as f64 * ONE_WEEK;
                let gap2 = ONE_WEEK * (pre + post + 1) as f64;
                let mut nt = (self.internal_get_time() + delta - min2) % gap2;
                if nt < 0.0 {
                    nt += gap2;
                }
                self.set_time(nt + min2)
            }
            JULIAN_DAY => {
                let v = self.internal_get(f).wrapping_add(amount);
                self.set(f, v);
                Ok(())
            }
            _ => Err(()),
        }
    }

    pub fn field_difference(&mut self, target: f64, f: usize) -> R<i32> {
        let mut min: i32 = 0;
        let start = self.get_time()?;
        if start < target {
            let mut max: i32 = 1;
            loop {
                self.set_time(start)?;
                self.add(f, max)?;
                let ms = self.get_time()?;
                if ms == target {
                    return Ok(max);
                } else if ms > target {
                    break;
                } else if max < i32::MAX {
                    min = max;
                    max = max.wrapping_shl(1);
                    if max < 0 {
                        max = i32::MAX;
                    }
                } else {
                    return Err(());
                }
            }
            while max - min > 1 {
                let t = min + (max - min) / 2;
                self.set_time(start)?;
                self.add(f, t)?;
                let ms = self.get_time()?;
                if ms == target {
                    return Ok(t);
                } else if ms > target {
                    max = t;
                } else {
                    min = t;
                }
            }
        } else if start > target {
            let mut max: i32 = -1;
            loop {
                self.set_time(start)?;
                self.add(f, max)?;
                let ms = self.get_time()?;
                if ms == target {
                    return Ok(max);
                } else if ms < target {
                    break;
                } else if max > i32::MIN {
                    min = max;
                    max = max.wrapping_shl(1);
                    if max >= 0 {
                        max = i32::MIN;
                    }
                } else {
                    return Err(());
                }
            }
            while min - max > 1 {
                let t = min + (max - min) / 2;
                self.set_time(start)?;
                self.add(f, t)?;
                let ms = self.get_time()?;
                if ms == target {
                    return Ok(t);
                } else if ms < target {
                    max = t;
                } else {
                    min = t;
                }
            }
        }
        self.set_time(start)?;
        self.add(f, min)?;
        Ok(min)
    }

    // ---- misc -------------------------------------------------------------

    pub fn in_daylight_time(&mut self) -> R<bool> {
        let t = self.get_time()?;
        let (_, dst) = self.zone.offsets(t)?;
        Ok(dst != 0)
    }

    pub fn day_of_week_type(&self, dow: i32) -> R<i32> {
        if !(1..=7).contains(&dow) {
            return Err(());
        }
        let (on, onms, ce, cems) = self.weekend;
        if on == ce {
            if dow != on {
                return Ok(0);
            }
            return Ok(if onms == 0 { 2 } else { 3 });
        }
        if on < ce {
            if dow < on || dow > ce {
                return Ok(0);
            }
        } else if dow > ce && dow < on {
            return Ok(0);
        }
        if dow == on {
            return Ok(if onms == 0 { 1 } else { 2 });
        }
        if dow == ce {
            return Ok(if cems >= 86_400_000 { 1 } else { 3 });
        }
        Ok(1)
    }

    pub fn weekend_transition(&self, dow: i32) -> R<i32> {
        let (on, onms, ce, cems) = self.weekend;
        if dow == on {
            Ok(onms)
        } else if dow == ce {
            Ok(cems)
        } else {
            Err(())
        }
    }

    /// isWeekend(date)
    pub fn is_weekend_at(&self, date: f64) -> R<bool> {
        let mut work = self.clone();
        work.set_time(date)?;
        let dow = work.get(DAY_OF_WEEK)?;
        let ty = self.day_of_week_type(dow)?;
        match ty {
            0 => Ok(false),
            1 => Ok(true),
            _ => {
                let mid = work.get(MILLISECONDS_IN_DAY)?;
                let tr = self.weekend_transition(dow)?;
                Ok(if ty == 2 { mid >= tr } else { mid < tr })
            }
        }
    }

    /// isWeekend()
    pub fn is_weekend(&mut self) -> bool {
        let Ok(dow) = self.get(DAY_OF_WEEK) else {
            return false;
        };
        let Ok(ty) = self.day_of_week_type(dow) else {
            return false;
        };
        match ty {
            0 => false,
            1 => true,
            _ => {
                let mid = self.get(MILLISECONDS_IN_DAY).unwrap_or(0);
                let tr = self.weekend_transition(dow).unwrap_or(0);
                if ty == 2 { mid >= tr } else { mid < tr }
            }
        }
    }

    pub fn is_equivalent_to(&self, o: &Calendar) -> bool {
        self.cal_type == o.cal_type
            && self.lenient == o.lenient
            && self.repeated_wall == o.repeated_wall
            && self.skipped_wall == o.skipped_wall
            && self.first_day == o.first_day
            && self.min_days == o.min_days
            && self.weekend == o.weekend
            && self.zone.equals(&o.zone)
            && (!self.cal_type.is_gregorian_family() || self.cutover == o.cutover)
    }
}

const REMAP: usize = 32;

static DATE_PRECEDENCE: &[&[&[usize]]] = &[
    &[
        &[DATE],
        &[WEEK_OF_YEAR, DAY_OF_WEEK],
        &[WEEK_OF_MONTH, DAY_OF_WEEK],
        &[DAY_OF_WEEK_IN_MONTH, DAY_OF_WEEK],
        &[WEEK_OF_YEAR, DOW_LOCAL],
        &[WEEK_OF_MONTH, DOW_LOCAL],
        &[DAY_OF_WEEK_IN_MONTH, DOW_LOCAL],
        &[DAY_OF_YEAR],
        &[REMAP | DATE, YEAR],
        &[REMAP | WEEK_OF_YEAR, YEAR_WOY],
    ],
    &[
        &[WEEK_OF_YEAR],
        &[WEEK_OF_MONTH],
        &[DAY_OF_WEEK_IN_MONTH],
        &[REMAP | DAY_OF_WEEK_IN_MONTH, DAY_OF_WEEK],
        &[REMAP | DAY_OF_WEEK_IN_MONTH, DOW_LOCAL],
    ],
];

static DOW_PRECEDENCE: &[&[&[usize]]] = &[&[&[DAY_OF_WEEK], &[DOW_LOCAL]]];

static YEAR_PRECEDENCE: &[&[&[usize]]] = &[&[&[YEAR], &[EXTENDED_YEAR], &[YEAR_WOY, WEEK_OF_YEAR]]];

static MONTH_PRECEDENCE: &[&[&[usize]]] = &[&[&[MONTH], &[ORDINAL_MONTH]]];

const NA: [i32; 4] = [-1, -1, -1, -1];

static CALENDAR_LIMITS: [[i32; 4]; NF] = [
    NA,
    NA,
    NA,
    NA,
    NA,
    NA,
    NA,
    [1, 1, 7, 7],
    NA,
    [0, 0, 1, 1],
    [0, 0, 11, 11],
    [0, 0, 23, 23],
    [0, 0, 59, 59],
    [0, 0, 59, 59],
    [0, 0, 999, 999],
    [
        -24 * 3_600_000,
        -16 * 3_600_000,
        12 * 3_600_000,
        30 * 3_600_000,
    ],
    [-3_600_000, -3_600_000, 2 * 3_600_000, 2 * 3_600_000],
    NA,
    [1, 1, 7, 7],
    NA,
    [-0x7F00_0000, -0x7F00_0000, 0x7F00_0000, 0x7F00_0000],
    [0, 0, 24 * 3_600_000 - 1, 24 * 3_600_000 - 1],
    [0, 0, 1, 1],
    NA,
];

// The limits ICU reports for each calendar system (getMinimum(),
// getGreatestMinimum(), getLeastMaximum(), getMaximum()), WEEK_OF_MONTH
// being computed from the minimal days in the first week.
static LIM_GREGORIAN: [[i32; 4]; NF] = [
    [0, 0, 1, 1],
    [1, 1, 140742, 144683],
    [0, 0, 11, 11],
    [1, 1, 52, 53],
    [1, 1, 4, 6],
    [1, 1, 28, 31],
    [1, 1, 365, 366],
    [1, 1, 7, 7],
    [-1, -1, 4, 5],
    [0, 0, 1, 1],
    [0, 0, 11, 11],
    [0, 0, 23, 23],
    [0, 0, 59, 59],
    [0, 0, 59, 59],
    [0, 0, 999, 999],
    [-86400000, -57600000, 43200000, 108000000],
    [-3600000, -3600000, 7200000, 7200000],
    [-140742, -140742, 140742, 144683],
    [1, 1, 7, 7],
    [-140742, -140742, 140742, 144683],
    [-2130706432, -2130706432, 2130706432, 2130706432],
    [0, 0, 86399999, 86399999],
    [0, 0, 1, 1],
    [0, 0, 11, 11],
];
static LIM_JAPANESE: [[i32; 4]; NF] = [
    [0, 0, 236, 236],
    [1, 1, 1, 142664],
    [0, 0, 11, 11],
    [1, 1, 52, 53],
    [1, 1, 4, 6],
    [1, 1, 28, 31],
    [1, 1, 365, 366],
    [1, 1, 7, 7],
    [-1, -1, 4, 5],
    [0, 0, 1, 1],
    [0, 0, 11, 11],
    [0, 0, 23, 23],
    [0, 0, 59, 59],
    [0, 0, 59, 59],
    [0, 0, 999, 999],
    [-86400000, -57600000, 43200000, 108000000],
    [-3600000, -3600000, 7200000, 7200000],
    [-140742, -140742, 140742, 144683],
    [1, 1, 7, 7],
    [-140742, -140742, 140742, 144683],
    [-2130706432, -2130706432, 2130706432, 2130706432],
    [0, 0, 86399999, 86399999],
    [0, 0, 1, 1],
    [0, 0, 11, 11],
];
static LIM_BUDDHIST: [[i32; 4]; NF] = [
    [0, 0, 0, 0],
    [1, 1, 140742, 144683],
    [0, 0, 11, 11],
    [1, 1, 52, 53],
    [1, 1, 4, 6],
    [1, 1, 28, 31],
    [1, 1, 365, 366],
    [1, 1, 7, 7],
    [-1, -1, 4, 5],
    [0, 0, 1, 1],
    [0, 0, 11, 11],
    [0, 0, 23, 23],
    [0, 0, 59, 59],
    [0, 0, 59, 59],
    [0, 0, 999, 999],
    [-86400000, -57600000, 43200000, 108000000],
    [-3600000, -3600000, 7200000, 7200000],
    [-140742, -140742, 140742, 144683],
    [1, 1, 7, 7],
    [-140742, -140742, 140742, 144683],
    [-2130706432, -2130706432, 2130706432, 2130706432],
    [0, 0, 86399999, 86399999],
    [0, 0, 1, 1],
    [0, 0, 11, 11],
];
static LIM_ROC: [[i32; 4]; NF] = [
    [0, 0, 1, 1],
    [1, 1, 140742, 144683],
    [0, 0, 11, 11],
    [1, 1, 52, 53],
    [1, 1, 4, 6],
    [1, 1, 28, 31],
    [1, 1, 365, 366],
    [1, 1, 7, 7],
    [-1, -1, 4, 5],
    [0, 0, 1, 1],
    [0, 0, 11, 11],
    [0, 0, 23, 23],
    [0, 0, 59, 59],
    [0, 0, 59, 59],
    [0, 0, 999, 999],
    [-86400000, -57600000, 43200000, 108000000],
    [-3600000, -3600000, 7200000, 7200000],
    [-140742, -140742, 140742, 144683],
    [1, 1, 7, 7],
    [-140742, -140742, 140742, 144683],
    [-2130706432, -2130706432, 2130706432, 2130706432],
    [0, 0, 86399999, 86399999],
    [0, 0, 1, 1],
    [0, 0, 11, 11],
];
static LIM_HEBREW: [[i32; 4]; NF] = [
    [0, 0, 0, 0],
    [-5000000, -5000000, 5000000, 5000000],
    [0, 0, 12, 12],
    [1, 1, 51, 56],
    [1, 1, 5, 6],
    [1, 1, 29, 30],
    [1, 1, 353, 385],
    [1, 1, 7, 7],
    [-1, -1, 5, 5],
    [0, 0, 1, 1],
    [0, 0, 11, 11],
    [0, 0, 23, 23],
    [0, 0, 59, 59],
    [0, 0, 59, 59],
    [0, 0, 999, 999],
    [-86400000, -57600000, 43200000, 108000000],
    [-3600000, -3600000, 7200000, 7200000],
    [-5000000, -5000000, 5000000, 5000000],
    [1, 1, 7, 7],
    [-5000000, -5000000, 5000000, 5000000],
    [-2130706432, -2130706432, 2130706432, 2130706432],
    [0, 0, 86399999, 86399999],
    [0, 0, 1, 1],
    [0, 0, 11, 12],
];
static LIM_ISLAMIC: [[i32; 4]; NF] = [
    [0, 0, 0, 0],
    [1, 1, 5000000, 5000000],
    [0, 0, 11, 11],
    [1, 1, 50, 51],
    [1, 1, 5, 6],
    [1, 1, 29, 31],
    [1, 1, 354, 355],
    [1, 1, 7, 7],
    [-1, -1, 5, 5],
    [0, 0, 1, 1],
    [0, 0, 11, 11],
    [0, 0, 23, 23],
    [0, 0, 59, 59],
    [0, 0, 59, 59],
    [0, 0, 999, 999],
    [-86400000, -57600000, 43200000, 108000000],
    [-3600000, -3600000, 7200000, 7200000],
    [1, 1, 5000000, 5000000],
    [1, 1, 7, 7],
    [1, 1, 5000000, 5000000],
    [-2130706432, -2130706432, 2130706432, 2130706432],
    [0, 0, 86399999, 86399999],
    [0, 0, 1, 1],
    [0, 0, 11, 11],
];
static LIM_PERSIAN: [[i32; 4]; NF] = [
    [0, 0, 0, 0],
    [-5000000, -5000000, 5000000, 5000000],
    [0, 0, 11, 11],
    [1, 1, 52, 53],
    [1, 1, 5, 6],
    [1, 1, 29, 31],
    [1, 1, 365, 366],
    [1, 1, 7, 7],
    [1, 1, 5, 5],
    [0, 0, 1, 1],
    [0, 0, 11, 11],
    [0, 0, 23, 23],
    [0, 0, 59, 59],
    [0, 0, 59, 59],
    [0, 0, 999, 999],
    [-86400000, -57600000, 43200000, 108000000],
    [-3600000, -3600000, 7200000, 7200000],
    [-5000000, -5000000, 5000000, 5000000],
    [1, 1, 7, 7],
    [-5000000, -5000000, 5000000, 5000000],
    [-2130706432, -2130706432, 2130706432, 2130706432],
    [0, 0, 86399999, 86399999],
    [0, 0, 1, 1],
    [0, 0, 11, 11],
];
static LIM_INDIAN: [[i32; 4]; NF] = [
    [0, 0, 0, 0],
    [-5000000, -5000000, 5000000, 5000000],
    [0, 0, 11, 11],
    [1, 1, 52, 53],
    [1, 1, 5, 6],
    [1, 1, 30, 31],
    [1, 1, 365, 366],
    [1, 1, 7, 7],
    [-1, -1, 5, 5],
    [0, 0, 1, 1],
    [0, 0, 11, 11],
    [0, 0, 23, 23],
    [0, 0, 59, 59],
    [0, 0, 59, 59],
    [0, 0, 999, 999],
    [-86400000, -57600000, 43200000, 108000000],
    [-3600000, -3600000, 7200000, 7200000],
    [-5000000, -5000000, 5000000, 5000000],
    [1, 1, 7, 7],
    [-5000000, -5000000, 5000000, 5000000],
    [-2130706432, -2130706432, 2130706432, 2130706432],
    [0, 0, 86399999, 86399999],
    [0, 0, 1, 1],
    [0, 0, 11, 11],
];
static LIM_COPTIC: [[i32; 4]; NF] = [
    [0, 0, 1, 1],
    [1, 1, 5000000, 5000000],
    [0, 0, 12, 12],
    [1, 1, 52, 53],
    [1, 1, 1, 6],
    [1, 1, 5, 30],
    [1, 1, 365, 366],
    [1, 1, 7, 7],
    [-1, -1, 1, 5],
    [0, 0, 1, 1],
    [0, 0, 11, 11],
    [0, 0, 23, 23],
    [0, 0, 59, 59],
    [0, 0, 59, 59],
    [0, 0, 999, 999],
    [-86400000, -57600000, 43200000, 108000000],
    [-3600000, -3600000, 7200000, 7200000],
    [-5000000, -5000000, 5000000, 5000000],
    [1, 1, 7, 7],
    [-5000000, -5000000, 5000000, 5000000],
    [-2130706432, -2130706432, 2130706432, 2130706432],
    [0, 0, 86399999, 86399999],
    [0, 0, 1, 1],
    [0, 0, 12, 12],
];
static LIM_ETHIOPIC: [[i32; 4]; NF] = [
    [0, 0, 1, 1],
    [1, 1, 5000000, 5000000],
    [0, 0, 12, 12],
    [1, 1, 52, 53],
    [1, 1, 1, 6],
    [1, 1, 5, 30],
    [1, 1, 365, 366],
    [1, 1, 7, 7],
    [-1, -1, 1, 5],
    [0, 0, 1, 1],
    [0, 0, 11, 11],
    [0, 0, 23, 23],
    [0, 0, 59, 59],
    [0, 0, 59, 59],
    [0, 0, 999, 999],
    [-86400000, -57600000, 43200000, 108000000],
    [-3600000, -3600000, 7200000, 7200000],
    [-5000000, -5000000, 5000000, 5000000],
    [1, 1, 7, 7],
    [-5000000, -5000000, 5000000, 5000000],
    [-2130706432, -2130706432, 2130706432, 2130706432],
    [0, 0, 86399999, 86399999],
    [0, 0, 1, 1],
    [0, 0, 12, 12],
];
static LIM_ETHIOAA: [[i32; 4]; NF] = [
    [0, 0, 0, 0],
    [1, 1, 5000000, 5000000],
    [0, 0, 12, 12],
    [1, 1, 52, 53],
    [1, 1, 1, 6],
    [1, 1, 5, 30],
    [1, 1, 365, 366],
    [1, 1, 7, 7],
    [-1, -1, 1, 5],
    [0, 0, 1, 1],
    [0, 0, 11, 11],
    [0, 0, 23, 23],
    [0, 0, 59, 59],
    [0, 0, 59, 59],
    [0, 0, 999, 999],
    [-86400000, -57600000, 43200000, 108000000],
    [-3600000, -3600000, 7200000, 7200000],
    [-5000000, -5000000, 5000000, 5000000],
    [1, 1, 7, 7],
    [-5000000, -5000000, 5000000, 5000000],
    [-2130706432, -2130706432, 2130706432, 2130706432],
    [0, 0, 86399999, 86399999],
    [0, 0, 1, 1],
    [0, 0, 12, 12],
];
static LIM_CHINESE: [[i32; 4]; NF] = [
    [1, 1, 83333, 83333],
    [1, 1, 60, 60],
    [0, 0, 11, 11],
    [1, 1, 50, 55],
    [1, 1, 5, 6],
    [1, 1, 29, 30],
    [1, 1, 353, 385],
    [1, 1, 7, 7],
    [-1, -1, 5, 5],
    [0, 0, 1, 1],
    [0, 0, 11, 11],
    [0, 0, 23, 23],
    [0, 0, 59, 59],
    [0, 0, 59, 59],
    [0, 0, 999, 999],
    [-86400000, -57600000, 43200000, 108000000],
    [-3600000, -3600000, 7200000, 7200000],
    [-5000000, -5000000, 5000000, 5000000],
    [1, 1, 7, 7],
    [-5000000, -5000000, 5000000, 5000000],
    [-2130706432, -2130706432, 2130706432, 2130706432],
    [0, 0, 86399999, 86399999],
    [0, 0, 1, 1],
    [0, 0, 11, 12],
];

/// handleGetLimit() of each calendar system.
fn type_limits(t: CalType, f: usize) -> [i32; 4] {
    let table = match t {
        CalType::Gregorian | CalType::Iso8601 => &LIM_GREGORIAN,
        CalType::Japanese => &LIM_JAPANESE,
        CalType::Buddhist => &LIM_BUDDHIST,
        CalType::Roc => &LIM_ROC,
        CalType::Hebrew => &LIM_HEBREW,
        CalType::Persian => &LIM_PERSIAN,
        CalType::Indian => &LIM_INDIAN,
        CalType::Coptic => &LIM_COPTIC,
        CalType::Ethiopic => &LIM_ETHIOPIC,
        CalType::EthiopicAmeteAlem => &LIM_ETHIOAA,
        CalType::Chinese | CalType::Dangi => &LIM_CHINESE,
        _ => &LIM_ISLAMIC,
    };
    table[f]
}

// ---------------------------------------------------------------------------
// The non-Gregorian systems

pub struct SysFields {
    pub eyear: i32,
    pub month: i32,
    pub ordinal_month: i32,
    pub dom: i32,
    pub doy: i32,
    pub era: i32,
    pub year: i32,
    pub leap_month: bool,
}

fn hebrew_is_leap(y: i64) -> bool {
    (7 * y + 1).rem_euclid(19) < 7
}

fn hebrew_months_in_year(y: i64) -> i64 {
    if hebrew_is_leap(y) { 13 } else { 12 }
}

/// HebrewCalendar::handleGetMonthLength()
fn hebrew_month_length(y: i64, m: i64) -> i32 {
    let (mut y, mut m) = (y, m);
    while m < 0 {
        y -= 1;
        m += hebrew_months_in_year(y);
    }
    while m > 12 {
        m -= hebrew_months_in_year(y);
        y += 1;
    }
    let len = |yy: i64| {
        sys_month_start(CalType::Hebrew, yy + 1, 0) - sys_month_start(CalType::Hebrew, yy, 0)
    };
    let ylen = len(y) % 30; // 353/383 -> 23, 354/384 -> 24, 355/385 -> 25
    let ty = match ylen {
        23 => 0,
        24 => 1,
        _ => 2,
    };
    match m {
        1 => {
            if ty == 2 {
                30
            } else {
                29
            }
        }
        2 => {
            if ty == 0 {
                29
            } else {
                30
            }
        }
        0 | 4 | 5 | 7 | 9 | 11 => 30,
        _ => 29,
    }
}

const COPTIC_EPOCH: i64 = 1_824_665;
const ETHIOPIC_EPOCH: i64 = 1_723_856;

fn ce_to_jd(year: i64, month: i64, date: i64, epoch: i64) -> i64 {
    // CECalendar::ceToJD()
    let mut year = year;
    let mut month = month;
    if month >= 0 {
        year += month / 13;
        month %= 13;
    } else {
        month += 1;
        year += month / 13 - 1;
        month = month % 13 + 12;
    }
    epoch + 365 * year + floor_div(year, 4) + 30 * month + date - 1
}

fn jd_to_ce(jd: i64, epoch: i64) -> (i64, i64, i64, i64) {
    let c4 = floor_div(jd - epoch, 1461);
    let r4 = (jd - epoch).rem_euclid(1461);
    let year = 4 * c4 + (r4 / 365 - r4 / 1460);
    let doy = if r4 == 1460 { 365 } else { r4 % 365 };
    (year, doy / 30, doy % 30 + 1, doy + 1)
}

fn islamic_epoc(t: CalType) -> i64 {
    match t {
        CalType::IslamicTbla => islamic::ASTRONOMICAL_EPOC,
        _ => islamic::CIVIL_EPOC,
    }
}

fn umalqura_range() -> (i64, i64) {
    (
        intl::calendar::umalqura_to_jdn(1300, 1, 1),
        intl::calendar::umalqura_to_jdn(1601, 1, 1),
    )
}

/// Days from the epoch to the first of month `m` (0-based, in range).
fn islamic_month_start(t: CalType, y: i64, m: i64) -> i64 {
    match t {
        CalType::IslamicCivil | CalType::IslamicTbla => islamic::civil_month_start(y, m),
        CalType::IslamicUmalqura => {
            if (1300..=1600).contains(&y) {
                intl::calendar::umalqura_to_jdn(y, m + 1, 1) - islamic::CIVIL_EPOC
            } else {
                islamic::civil_month_start(y, m)
            }
        }
        _ => islamic::astro_month_start(y, m),
    }
}

/// (eyear, month 0-based, dom, doy) of JD `jd` in an Islamic calendar.
fn islamic_fields(t: CalType, jd: i64, time: f64) -> (i64, i64, i64, i64) {
    let days = jd - islamic_epoc(t);
    match t {
        CalType::IslamicCivil | CalType::IslamicTbla => islamic::civil_fields(days),
        CalType::IslamicUmalqura => {
            let (lo, hi) = umalqura_range();
            if jd >= lo && jd < hi {
                let (y, m, d) = intl::calendar::jdn_to_umalqura(jd);
                let doy = jd - intl::calendar::umalqura_to_jdn(y, 1, 1) + 1;
                (y, m - 1, d, doy)
            } else {
                islamic::civil_fields(days)
            }
        }
        _ => islamic::astro_fields(days, time),
    }
}

/// ICU Hebrew month (0 Tishri .. 5 Adar I, 6 Adar .. 12 Elul) from the
/// intl crate's (1 Nisan .. 7 Tishrei .. 12 Adar (I), 13 Adar II).
fn hebrew_month_to_icu(y: i64, m: i64) -> i64 {
    match m {
        7..=11 => m - 7,
        12 => {
            if hebrew_is_leap(y) {
                5
            } else {
                6
            }
        }
        13 => 6,
        _ => m + 6,
    }
}

fn hebrew_month_from_icu(y: i64, m: i64) -> i64 {
    match m {
        0..=4 => m + 7,
        5 => 12,
        6 => {
            if hebrew_is_leap(y) {
                13
            } else {
                12
            }
        }
        _ => m - 6,
    }
}

fn indian_start(gy: i64) -> i64 {
    // 1 Chaitra: March 22 (March 21 in Gregorian leap years)
    let d = if greg_leap(gy) { 21 } else { 22 };
    days_from_civil(gy, 3, d) + EPOCH_JD as i64
}

fn indian_month_len(sy: i64, m: i64) -> i64 {
    let leap = greg_leap(sy + 78);
    match m {
        0 => {
            if leap {
                31
            } else {
                30
            }
        }
        1..=5 => 31,
        _ => 30,
    }
}

/// The fields of JD `jd` in a non-Gregorian system.
pub fn sys_fields(t: CalType, jd: i64, time: f64) -> SysFields {
    let mut leap_month = false;
    let mut chinese_ordinal_shift = 0;
    let (eyear, month, dom, doy, era, year);
    match t {
        CalType::Hebrew => {
            let (y, m, d) = intl::calendar::jdn_to_hebrew(jd);
            let im = hebrew_month_to_icu(y, m);
            eyear = y;
            month = im;
            dom = d;
            doy = jd - sys_month_start(t, y, 0);
            era = 0;
            year = y;
        }
        CalType::Persian => {
            let (y, m, d) = intl::calendar::jdn_to_persian(jd);
            eyear = y;
            month = m - 1;
            dom = d;
            doy = jd - intl::calendar::persian_to_jdn(y, 1, 1) + 1;
            era = 0;
            year = y;
        }
        CalType::Indian => {
            let (gy, _, _) = civil_from_days(jd - EPOCH_JD as i64);
            let mut sy = gy - 78;
            let mut start = indian_start(gy);
            if jd < start {
                sy -= 1;
                start = indian_start(gy - 1);
            }
            let mut d0 = jd - start;
            let mut m = 0;
            while m < 11 && d0 >= indian_month_len(sy, m) {
                d0 -= indian_month_len(sy, m);
                m += 1;
            }
            eyear = sy;
            month = m;
            dom = d0 + 1;
            doy = jd - start + 1;
            era = 0;
            year = sy;
        }
        CalType::Coptic | CalType::Ethiopic | CalType::EthiopicAmeteAlem => {
            let epoch = if t == CalType::Coptic {
                COPTIC_EPOCH
            } else {
                ETHIOPIC_EPOCH
            };
            let (y, m, d, dy) = jd_to_ce(jd, epoch);
            match t {
                CalType::Coptic => {
                    if y <= 0 {
                        era = 0;
                        year = 1 - y;
                    } else {
                        era = 1;
                        year = y;
                    }
                    eyear = y;
                }
                CalType::Ethiopic => {
                    if y <= 0 {
                        era = 0;
                        year = y + 5500;
                    } else {
                        era = 1;
                        year = y;
                    }
                    eyear = y;
                }
                _ => {
                    era = 0;
                    year = y + 5500;
                    eyear = y;
                }
            }
            month = m;
            dom = d;
            doy = dy;
        }
        CalType::Chinese | CalType::Dangi => {
            let r = if t == CalType::Chinese {
                intl::calendar::jdn_to_chinese(jd)
            } else {
                intl::calendar::jdn_to_dangi(jd)
            };
            match r {
                Some((gy, m, d, leap)) => {
                    // ICU's extended years count from 2637 BCE (chinese)
                    // or 2333 BCE (dangi); the cycles from 2637 BCE
                    let cy = gy + 2637;
                    eyear = gy + chinese_epoch_offset(t);
                    month = m - 1;
                    dom = d;
                    leap_month = leap;
                    let ys = sys_month_start(t, eyear, 0);
                    doy = jd - ys;
                    era = (cy - 1).div_euclid(60) + 1;
                    year = (cy - 1).rem_euclid(60) + 1;
                    if let Some(lm) = chinese_leap_month(t, gy) {
                        if m > lm || (m == lm && leap) {
                            chinese_ordinal_shift = 1;
                        }
                    }
                }
                None => {
                    let (gy, gm, gd) = civil_from_days(jd - EPOCH_JD as i64);
                    let cy = gy + 2637;
                    eyear = gy + chinese_epoch_offset(t);
                    month = gm - 1;
                    dom = gd;
                    doy = jd - (days_from_civil(gy, 1, 1) + EPOCH_JD as i64) + 1;
                    era = (cy - 1).div_euclid(60) + 1;
                    year = (cy - 1).rem_euclid(60) + 1;
                }
            }
        }
        _ => {
            let (y, m, d, dy) = islamic_fields(t, jd, time);
            eyear = y;
            month = m;
            dom = d;
            doy = dy;
            era = 0;
            year = y;
        }
    }
    let ordinal_month = if t == CalType::Hebrew && !hebrew_is_leap(eyear) && month >= 6 {
        month - 1
    } else {
        month + chinese_ordinal_shift
    };
    SysFields {
        eyear: eyear as i32,
        month: month as i32,
        ordinal_month: ordinal_month as i32,
        dom: dom as i32,
        doy: doy as i32,
        era: era as i32,
        year: year as i32,
        leap_month,
    }
}

/// The extended year of the Gregorian year a Chinese / Dangi year
/// begins in.
fn chinese_epoch_offset(t: CalType) -> i64 {
    if t == CalType::Dangi { 2333 } else { 2637 }
}

/// The leap month (1-based, the month it repeats) of a Chinese / Dangi
/// year (identified by its related Gregorian year).
fn chinese_leap_month(t: CalType, gy: i64) -> Option<i64> {
    (1..=12).find(|&k| {
        if t == CalType::Chinese {
            intl::calendar::chinese_to_jdn(gy, k, 1, true).is_some()
        } else {
            intl::calendar::dangi_to_jdn(gy, k, 1, true).is_some()
        }
    })
}

pub fn sys_months_in_year(t: CalType, _y: i64) -> i64 {
    match t {
        CalType::Hebrew | CalType::Coptic | CalType::Ethiopic | CalType::EthiopicAmeteAlem => 13,
        _ => 12,
    }
}

/// The JD of the day before the first of month `m` (0-based, normalized)
/// of extended year `y`.
pub fn sys_month_start(t: CalType, y: i64, m: i64) -> i64 {
    let n = sys_months_in_year(t, y);
    let (mut y, mut m) = (y, m);
    if t != CalType::Hebrew && !(0..n).contains(&m) {
        y += floor_div(m, n);
        m = m.rem_euclid(n);
    }
    match t {
        CalType::Hebrew => {
            while m < 0 {
                y -= 1;
                m += hebrew_months_in_year(y);
            }
            while m > 12 {
                m -= hebrew_months_in_year(y);
                y += 1;
            }
            if m == 5 && !hebrew_is_leap(y) {
                // ADAR_1 in a non-leap year: the start of ADAR
                m = 6;
            }
            intl::calendar::hebrew_to_jdn(y, hebrew_month_from_icu(y, m), 1) - 1
        }
        CalType::Persian => intl::calendar::persian_to_jdn(y, m + 1, 1) - 1,
        CalType::Indian => {
            let mut jd = indian_start(y + 78);
            for k in 0..m {
                jd += indian_month_len(y, k);
            }
            jd - 1
        }
        CalType::Coptic => ce_to_jd(y, m, 0, COPTIC_EPOCH),
        CalType::Ethiopic => ce_to_jd(y, m, 0, ETHIOPIC_EPOCH),
        CalType::EthiopicAmeteAlem => ce_to_jd(y, m, 0, ETHIOPIC_EPOCH),
        CalType::Chinese | CalType::Dangi => {
            let gy = y - chinese_epoch_offset(t);
            let r = if t == CalType::Chinese {
                intl::calendar::chinese_to_jdn(gy, m + 1, 1, false)
            } else {
                intl::calendar::dangi_to_jdn(gy, m + 1, 1, false)
            };
            match r {
                Some(j) => j - 1,
                None => days_from_civil(gy, m + 1, 1) + EPOCH_JD as i64 - 1,
            }
        }
        _ => islamic_month_start(t, y, m) + islamic_epoc(t) - 1,
    }
}
