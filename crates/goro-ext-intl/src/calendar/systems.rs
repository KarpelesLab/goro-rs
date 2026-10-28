//! Calendar arithmetic: Julian day numbers to and from the dates of each
//! calendar system ICU offers (the "calendar" locale keyword of UTS #35 /
//! BCP 47 "ca"). The algorithms are the published arithmetic rules of each
//! calendar (Dershowitz & Reingold, "Calendrical Calculations"; the
//! Gregorian/Julian conversions of the Explanatory Supplement): the
//! Gregorian calendar with its Julian predecessor before a cutover, the
//! tabular Islamic calendars, the Hebrew calendar, the Persian 33-year
//! arithmetic cycle, the Coptic/Ethiopic and Indian national calendars;
//! the Islamic astronomical calendar starts its months the day after the
//! conjunction (new moons from Meeus, "Astronomical Algorithms", ch. 49);
//! Umm al-Qura and the Chinese/Korean lunisolar calendars use the intl
//! crate's tables.

use super::super::datefmt::supplemental as sup;

/// The Julian day number of 1970-01-01.
pub const EPOCH_JD: i64 = 2_440_588;

/// A calendar system.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CalType {
    Gregorian,
    Iso8601,
    Japanese,
    Buddhist,
    Roc,
    Persian,
    Islamic,
    IslamicCivil,
    IslamicTbla,
    IslamicUmalqura,
    IslamicRgsa,
    Hebrew,
    Chinese,
    Dangi,
    Indian,
    Coptic,
    Ethiopic,
    EthiopicAmeteAlem,
}

/// Every calendar type, in the order ICU lists the keyword values.
pub const ALL: &[CalType] = &[
    CalType::Gregorian,
    CalType::Japanese,
    CalType::Buddhist,
    CalType::Roc,
    CalType::Persian,
    CalType::IslamicCivil,
    CalType::Islamic,
    CalType::Hebrew,
    CalType::Chinese,
    CalType::Indian,
    CalType::Coptic,
    CalType::Ethiopic,
    CalType::EthiopicAmeteAlem,
    CalType::Iso8601,
    CalType::Dangi,
    CalType::IslamicUmalqura,
    CalType::IslamicTbla,
    CalType::IslamicRgsa,
];

impl CalType {
    /// The calendar of a keyword value ("gregorian", "islamic-civil", ...;
    /// also the BCP 47 aliases "gregory", "ethioaa", "islamicc").
    pub fn from_name(s: &str) -> Option<CalType> {
        Some(match s.to_ascii_lowercase().as_str() {
            "gregorian" | "gregory" => CalType::Gregorian,
            "iso8601" => CalType::Iso8601,
            "japanese" => CalType::Japanese,
            "buddhist" => CalType::Buddhist,
            "roc" => CalType::Roc,
            "persian" => CalType::Persian,
            "islamic" => CalType::Islamic,
            "islamic-civil" | "islamicc" => CalType::IslamicCivil,
            "islamic-tbla" => CalType::IslamicTbla,
            "islamic-umalqura" => CalType::IslamicUmalqura,
            "islamic-rgsa" => CalType::IslamicRgsa,
            "hebrew" => CalType::Hebrew,
            "chinese" => CalType::Chinese,
            "dangi" => CalType::Dangi,
            "indian" => CalType::Indian,
            "coptic" => CalType::Coptic,
            "ethiopic" => CalType::Ethiopic,
            "ethiopic-amete-alem" | "ethioaa" => CalType::EthiopicAmeteAlem,
            _ => return None,
        })
    }

    /// The keyword value (`IntlCalendar::getType()`).
    pub fn name(self) -> &'static str {
        match self {
            CalType::Gregorian => "gregorian",
            CalType::Iso8601 => "iso8601",
            CalType::Japanese => "japanese",
            CalType::Buddhist => "buddhist",
            CalType::Roc => "roc",
            CalType::Persian => "persian",
            CalType::Islamic => "islamic",
            CalType::IslamicCivil => "islamic-civil",
            CalType::IslamicTbla => "islamic-tbla",
            CalType::IslamicUmalqura => "islamic-umalqura",
            CalType::IslamicRgsa => "islamic-rgsa",
            CalType::Hebrew => "hebrew",
            CalType::Chinese => "chinese",
            CalType::Dangi => "dangi",
            CalType::Indian => "indian",
            CalType::Coptic => "coptic",
            CalType::Ethiopic => "ethiopic",
            CalType::EthiopicAmeteAlem => "ethiopic-amete-alem",
        }
    }

    /// The CLDR data section of the calendar's symbols and patterns.
    pub fn data_section(self) -> &'static str {
        match self {
            CalType::Iso8601 => "gregorian",
            other => other.name(),
        }
    }

    /// Whether the calendar is Gregorian-based (Gregorian months and days).
    pub fn is_gregorian_like(self) -> bool {
        matches!(
            self,
            CalType::Gregorian
                | CalType::Iso8601
                | CalType::Japanese
                | CalType::Buddhist
                | CalType::Roc
        )
    }

    pub fn is_lunisolar(self) -> bool {
        matches!(self, CalType::Chinese | CalType::Dangi)
    }

    /// The highest MONTH value (12 for the 13-month calendars).
    pub fn max_month(self) -> i32 {
        match self {
            CalType::Hebrew | CalType::Coptic | CalType::Ethiopic | CalType::EthiopicAmeteAlem => {
                12
            }
            _ => 11,
        }
    }
}

/// floor(a / b)
pub fn fdiv(a: i64, b: i64) -> i64 {
    a.div_euclid(b)
}

/// The date fields of a calendar day.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ymd {
    pub era: i32,
    pub year: i32,
    pub eyear: i32,
    /// 0-based month (ICU MONTH; Hebrew counts Adar I as 5 in all years).
    pub month: i32,
    /// 0-based month counted in the year (ICU ORDINAL_MONTH).
    pub ordinal_month: i32,
    pub dom: i32,
    pub doy: i32,
    pub leap_month: bool,
}

// ---------------------------------------------------------------- Gregorian / Julian

fn is_greg_leap(y: i64) -> bool {
    y.rem_euclid(4) == 0 && (y.rem_euclid(100) != 0 || y.rem_euclid(400) == 0)
}

fn is_julian_leap(y: i64) -> bool {
    y.rem_euclid(4) == 0
}

const MONTH_DAYS: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

fn days_before_month(m: usize, leap: bool) -> i64 {
    let mut d = 0;
    for (i, md) in MONTH_DAYS.iter().enumerate().take(m) {
        d += *md as i64 + if i == 1 && leap { 1 } else { 0 };
    }
    d
}

/// JDN of a proleptic Gregorian date (month 1-12).
pub fn greg_to_jd(y: i64, m: i64, d: i64) -> i64 {
    // days from 0000-03-01 (era computation with March-based years)
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = fdiv(y, 400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe + 1_721_120
}

/// Proleptic Gregorian (year, month 1-12, day) of a JDN.
pub fn jd_to_greg(jd: i64) -> (i64, i64, i64) {
    let z = jd - 1_721_120;
    let era = fdiv(z, 146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// JDN of a Julian calendar date.
pub fn julian_to_jd(y: i64, m: i64, d: i64) -> i64 {
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = fdiv(y, 4);
    let yoe = y - era * 4;
    let doy = (153 * m + 2) / 5 + d - 1;
    era * 1461 + yoe * 365 + doy + 1_721_118
}

/// Julian calendar (year, month, day) of a JDN.
pub fn jd_to_julian(jd: i64) -> (i64, i64, i64) {
    let z = jd - 1_721_118;
    let era = fdiv(z, 1461);
    let doe = z - era * 1461;
    let yoe = (doe - doe / 1460) / 365;
    let y = yoe + era * 4;
    let doy = doe - 365 * yoe;
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The JDN of the Gregorian cutover (ICU's default: 1582-10-15).
pub fn cutover_jd(cutover_ms: f64) -> i64 {
    EPOCH_JD + (cutover_ms / 86_400_000.0).floor() as i64
}

/// The year in which the Gregorian rules start.
fn cutover_year(cutover: i64) -> i64 {
    jd_to_greg(cutover).0
}

fn hybrid_ymd(jd: i64, cutover: i64) -> (i64, i64, i64, bool) {
    if jd >= cutover {
        let (y, m, d) = jd_to_greg(jd);
        (y, m, d, true)
    } else {
        let (y, m, d) = jd_to_julian(jd);
        (y, m, d, false)
    }
}

fn hybrid_to_jd(y: i64, m: i64, d: i64, cutover: i64) -> i64 {
    if y > cutover_year(cutover) {
        return greg_to_jd(y, m, d);
    }
    if y < cutover_year(cutover) {
        return julian_to_jd(y, m, d);
    }
    let g = greg_to_jd(y, m, d);
    if g >= cutover {
        g
    } else {
        julian_to_jd(y, m, d)
    }
}

fn hybrid_year_start(y: i64, cutover: i64) -> i64 {
    hybrid_to_jd(y, 1, 1, cutover)
}

fn hybrid_is_leap(y: i64, cutover: i64) -> bool {
    if y >= cutover_year(cutover) {
        is_greg_leap(y)
    } else {
        is_julian_leap(y)
    }
}

// ---------------------------------------------------------------- Islamic

fn islamic_epoch(ct: CalType) -> i64 {
    // civil: 16 July 622 Julian (Friday); astronomical epoch: the day before
    match ct {
        CalType::IslamicTbla => 1_948_439,
        _ => 1_948_440,
    }
}

fn tabular_islamic_to_jd(ct: CalType, y: i64, m: i64, d: i64) -> i64 {
    islamic_epoch(ct) - 1 + (y - 1) * 354 + fdiv(3 + 11 * y, 30) + 29 * (m - 1) + m / 2 + d
}

/// New moon number `k` (k = 0 near 2000-01-06) as a Julian date (UT),
/// Meeus "Astronomical Algorithms" ch. 49 (main periodic terms).
fn new_moon_jde(k: f64) -> f64 {
    let t = k / 1236.85;
    let jde = 2_451_550.097_66 + 29.530_588_861 * k + 0.000_154_37 * t * t
        - 0.000_000_150 * t * t * t
        + 0.000_000_000_73 * t * t * t * t;
    let e = 1.0 - 0.002_516 * t - 0.000_007_4 * t * t;
    let rad = |x: f64| x.to_radians();
    let m = rad(2.5534 + 29.105_356_70 * k - 0.000_001_4 * t * t);
    let mp = rad(201.5643 + 385.816_935_28 * k + 0.010_758_2 * t * t + 0.000_012_38 * t * t * t);
    let f = rad(160.7108 + 390.670_502_84 * k - 0.001_611_8 * t * t - 0.000_002_27 * t * t * t);
    let om = rad(124.7746 - 1.563_755_88 * k + 0.002_067_2 * t * t);
    let corr = -0.40720 * mp.sin()
        + 0.17241 * e * m.sin()
        + 0.01608 * (2.0 * mp).sin()
        + 0.01039 * (2.0 * f).sin()
        + 0.00739 * e * (mp - m).sin()
        - 0.00514 * e * (mp + m).sin()
        + 0.00208 * e * e * (2.0 * m).sin()
        - 0.00111 * (mp - 2.0 * f).sin()
        - 0.00057 * (mp + 2.0 * f).sin()
        + 0.00056 * e * (2.0 * mp + m).sin()
        - 0.00042 * (3.0 * mp).sin()
        + 0.00042 * e * (m + 2.0 * f).sin()
        + 0.00038 * e * (m - 2.0 * f).sin()
        - 0.00024 * e * (2.0 * mp - m).sin()
        - 0.00017 * om.sin();
    // dynamical time to UT: ~69 s around 2000, negligible for whole days
    jde + corr - 69.0 / 86_400.0
}

/// Lunations between Meeus' k = 0 and the month of Muharram 1 AH.
const ISLAMIC_LUNATION_OFFSET: i64 = 17_037;

/// JDN of the first day of astronomical Islamic month number `n` (months
/// since Muharram 1 AH, 0-based): the day after the day of the conjunction.
fn astro_month_start(n: i64) -> i64 {
    let k = (n - ISLAMIC_LUNATION_OFFSET) as f64;
    let jd = new_moon_jde(k);
    // the civil day (from midnight UT) containing the conjunction, + 1
    (jd + 0.5).floor() as i64 + 1
}

fn astro_months_before(jd: i64) -> i64 {
    // estimate, then correct
    let mut n = ((jd - 1_948_440) as f64 / 29.530_588_861).floor() as i64;
    while astro_month_start(n) > jd {
        n -= 1;
    }
    while astro_month_start(n + 1) <= jd {
        n += 1;
    }
    n
}

// ---------------------------------------------------------------- Hebrew

fn hebrew_leap(y: i64) -> bool {
    (7 * y + 1).rem_euclid(19) < 7
}

fn hebrew_elapsed(y: i64) -> i64 {
    let months = fdiv(235 * y - 234, 19);
    let parts = 12_084 + 13_753 * months;
    let day = months * 29 + fdiv(parts, 25_920);
    if (3 * (day + 1)).rem_euclid(7) < 3 {
        day + 1
    } else {
        day
    }
}

fn hebrew_new_year(y: i64) -> i64 {
    let ny0 = hebrew_elapsed(y - 1);
    let ny1 = hebrew_elapsed(y);
    let ny2 = hebrew_elapsed(y + 1);
    let corr = if ny2 - ny1 == 356 {
        2
    } else if ny1 - ny0 == 382 {
        1
    } else {
        0
    };
    // the Hebrew epoch (1 Tishri AM 1 = 7 October 3761 BCE, Julian) as
    // the JDN offset of the elapsed-days count
    347_998 + ny1 + corr
}

fn hebrew_year_len(y: i64) -> i64 {
    hebrew_new_year(y + 1) - hebrew_new_year(y)
}

/// Month lengths in ICU order (Tishri = 0 … Elul = 12, Adar I = 5).
fn hebrew_month_len(y: i64, m: i32) -> i32 {
    let len = hebrew_year_len(y);
    match m {
        0 => 30,
        1 => {
            if len % 10 == 5 {
                30
            } else {
                29
            }
        }
        2 => {
            if len % 10 == 3 {
                29
            } else {
                30
            }
        }
        3 => 29,
        4 => 30,
        5 => {
            if hebrew_leap(y) {
                30
            } else {
                0
            }
        }
        6 => 29,
        7 => 30,
        8 => 29,
        9 => 30,
        10 => 29,
        11 => 30,
        _ => 29,
    }
}

// ---------------------------------------------------------------- Persian

/// The 33-year arithmetic leap rule.
fn persian_leap(y: i64) -> bool {
    (25 * y + 11).rem_euclid(33) < 8
}

fn persian_leaps_before(y: i64) -> i64 {
    // leap years in [1, y)
    let n = y - 1;
    let cycles = fdiv(n, 33);
    let mut c = cycles * 8;
    for k in (cycles * 33 + 1)..=n {
        if persian_leap(k) {
            c += 1;
        }
    }
    c
}

fn persian_year_start(y: i64) -> i64 {
    // 1 Farvardin 1 AP = 19 March 622 (Julian) = JDN 1 948 321
    1_948_319 + 365 * (y - 1) + persian_leaps_before(y) + 1
}

// ---------------------------------------------------------------- Coptic / Ethiopic

fn coptic_epoch(ct: CalType) -> i64 {
    match ct {
        CalType::Coptic => 1_825_030,
        _ => 1_724_221,
    }
}

fn coptic_to_jd(ct: CalType, y: i64, m: i64, d: i64) -> i64 {
    coptic_epoch(ct) - 1 + 365 * (y - 1) + fdiv(y, 4) + 30 * (m - 1) + d
}

fn coptic_from_jd(ct: CalType, jd: i64) -> (i64, i64, i64) {
    let y = fdiv(4 * (jd - coptic_epoch(ct)) + 1463, 1461);
    let m = fdiv(jd - coptic_to_jd(ct, y, 1, 1), 30) + 1;
    let d = jd + 1 - coptic_to_jd(ct, y, m, 1);
    (y, m, d)
}

// ---------------------------------------------------------------- Indian

fn indian_year_start(y: i64) -> i64 {
    let g = y + 78;
    greg_to_jd(g, 3, if is_greg_leap(g) { 21 } else { 22 })
}

fn indian_month_len(y: i64, m: i32) -> i32 {
    match m {
        0 => {
            if is_greg_leap(y + 78) {
                31
            } else {
                30
            }
        }
        1..=5 => 31,
        _ => 30,
    }
}

// ---------------------------------------------------------------- lunisolar

fn chinese_offset(ct: CalType) -> i64 {
    match ct {
        CalType::Dangi => 2333,
        _ => 2637,
    }
}

fn lunisolar_from_jd(ct: CalType, jd: i64) -> Option<(i64, i64, i64, bool)> {
    let r = if ct == CalType::Dangi {
        intl::calendar::jdn_to_dangi(jd)
    } else {
        intl::calendar::jdn_to_chinese(jd)
    }?;
    Some(r)
}

fn lunisolar_to_jd(ct: CalType, y: i64, m: i64, d: i64, leap: bool) -> Option<i64> {
    if ct == CalType::Dangi {
        intl::calendar::dangi_to_jdn(y, m, d, leap)
    } else {
        intl::calendar::chinese_to_jdn(y, m, d, leap)
    }
}

/// The months of a lunisolar year: (month 1-12, leap) in order.
fn lunisolar_months(ct: CalType, y: i64) -> Vec<(i64, bool)> {
    let mut v = Vec::new();
    for m in 1..=12 {
        v.push((m, false));
        if lunisolar_to_jd(ct, y, m, 1, true).is_some_and(|jd| {
            lunisolar_from_jd(ct, jd).is_some_and(|(yy, mm, _, l)| yy == y && mm == m && l)
        }) {
            v.push((m, true));
        }
    }
    v
}

// ---------------------------------------------------------------- API

/// Japanese era index of a Gregorian date.
fn japanese_era(y: i64, m: i64, d: i64) -> usize {
    let mut era = 0;
    for (i, (ey, em, ed)) in sup::JAPANESE_ERAS.iter().enumerate() {
        if (y, m, d) >= (*ey as i64, *em as i64, *ed as i64) {
            era = i;
        } else {
            break;
        }
    }
    era
}

fn clamp32(v: i64) -> i32 {
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// The date fields of JDN `jd`.
pub fn fields_from_jd(ct: CalType, jd: i64, cutover: i64) -> Ymd {
    match ct {
        CalType::Gregorian
        | CalType::Iso8601
        | CalType::Japanese
        | CalType::Buddhist
        | CalType::Roc => {
            let (y, m, d, _) = hybrid_ymd(jd, cutover);
            let doy = jd - hybrid_year_start(y, cutover) + 1;
            let (era, year) = match ct {
                CalType::Japanese => {
                    let e = japanese_era(y, m, d);
                    (e as i64, y - sup::JAPANESE_ERAS[e].0 as i64 + 1)
                }
                CalType::Buddhist => (0, y + 543),
                CalType::Roc => {
                    if y > 1911 {
                        (1, y - 1911)
                    } else {
                        (0, 1912 - y)
                    }
                }
                _ => {
                    if y > 0 {
                        (1, y)
                    } else {
                        (0, 1 - y)
                    }
                }
            };
            Ymd {
                era: era as i32,
                year: clamp32(year),
                eyear: clamp32(y),
                month: (m - 1) as i32,
                ordinal_month: (m - 1) as i32,
                dom: d as i32,
                doy: doy as i32,
                leap_month: false,
            }
        }
        CalType::Islamic
        | CalType::IslamicRgsa
        | CalType::IslamicCivil
        | CalType::IslamicTbla
        | CalType::IslamicUmalqura => {
            let (y, m, d, ystart) = match ct {
                CalType::IslamicCivil | CalType::IslamicTbla => {
                    let mut y = fdiv(30 * (jd - islamic_epoch(ct)) + 10_646, 10_631);
                    while tabular_islamic_to_jd(ct, y, 1, 1) > jd {
                        y -= 1;
                    }
                    while tabular_islamic_to_jd(ct, y + 1, 1, 1) <= jd {
                        y += 1;
                    }
                    let mut m = 1;
                    while m < 12 && tabular_islamic_to_jd(ct, y, m + 1, 1) <= jd {
                        m += 1;
                    }
                    let d = jd - tabular_islamic_to_jd(ct, y, m, 1) + 1;
                    (y, m, d, tabular_islamic_to_jd(ct, y, 1, 1))
                }
                CalType::IslamicUmalqura => {
                    let (y, m, d) = intl::calendar::jdn_to_umalqura(jd);
                    (y, m, d, intl::calendar::umalqura_to_jdn(y, 1, 1))
                }
                _ => {
                    let n = astro_months_before(jd);
                    let y = fdiv(n, 12) + 1;
                    let m = n - (y - 1) * 12 + 1;
                    let d = jd - astro_month_start(n) + 1;
                    (y, m, d, astro_month_start((y - 1) * 12))
                }
            };
            Ymd {
                era: 0,
                year: clamp32(y),
                eyear: clamp32(y),
                month: (m - 1) as i32,
                ordinal_month: (m - 1) as i32,
                dom: d as i32,
                doy: (jd - ystart + 1) as i32,
                leap_month: false,
            }
        }
        CalType::Hebrew => {
            // years start in Tishri; the Hebrew year is ~ Gregorian + 3760
            let (gy, _, _) = jd_to_greg(jd);
            let mut y = gy + 3760;
            while hebrew_new_year(y) > jd {
                y -= 1;
            }
            while hebrew_new_year(y + 1) <= jd {
                y += 1;
            }
            let start = hebrew_new_year(y);
            let mut rem = jd - start;
            let mut m = 0;
            while m < 12 && rem >= hebrew_month_len(y, m) as i64 {
                rem -= hebrew_month_len(y, m) as i64;
                m += 1;
            }
            let ordinal = if !hebrew_leap(y) && m > 5 { m - 1 } else { m };
            Ymd {
                era: 0,
                year: clamp32(y),
                eyear: clamp32(y),
                month: m,
                ordinal_month: ordinal,
                dom: rem as i32 + 1,
                doy: (jd - start + 1) as i32,
                leap_month: false,
            }
        }
        CalType::Persian => {
            let (gy, _, _) = jd_to_greg(jd);
            let mut y = gy - 621;
            while persian_year_start(y) > jd {
                y -= 1;
            }
            while persian_year_start(y + 1) <= jd {
                y += 1;
            }
            let doy = jd - persian_year_start(y);
            let (m, d) = if doy < 186 {
                (doy / 31, doy % 31)
            } else {
                (6 + (doy - 186) / 30, (doy - 186) % 30)
            };
            Ymd {
                era: 0,
                year: clamp32(y),
                eyear: clamp32(y),
                month: m as i32,
                ordinal_month: m as i32,
                dom: d as i32 + 1,
                doy: doy as i32 + 1,
                leap_month: false,
            }
        }
        CalType::Coptic | CalType::Ethiopic | CalType::EthiopicAmeteAlem => {
            let (y, m, d) = coptic_from_jd(ct, jd);
            let doy = jd - coptic_to_jd(ct, y, 1, 1) + 1;
            let (era, year) = match ct {
                CalType::Coptic => {
                    if y > 0 {
                        (1, y)
                    } else {
                        (0, 1 - y)
                    }
                }
                CalType::Ethiopic => {
                    if y > 0 {
                        (1, y)
                    } else {
                        (0, y + 5500)
                    }
                }
                _ => (0, y + 5500),
            };
            Ymd {
                era,
                year: clamp32(year),
                eyear: clamp32(y),
                month: (m - 1) as i32,
                ordinal_month: (m - 1) as i32,
                dom: d as i32,
                doy: doy as i32,
                leap_month: false,
            }
        }
        CalType::Indian => {
            let (gy, _, _) = jd_to_greg(jd);
            let mut y = gy - 78;
            while indian_year_start(y) > jd {
                y -= 1;
            }
            while indian_year_start(y + 1) <= jd {
                y += 1;
            }
            let mut rem = jd - indian_year_start(y);
            let doy = rem + 1;
            let mut m = 0;
            while m < 11 && rem >= indian_month_len(y, m) as i64 {
                rem -= indian_month_len(y, m) as i64;
                m += 1;
            }
            Ymd {
                era: 0,
                year: clamp32(y),
                eyear: clamp32(y),
                month: m,
                ordinal_month: m,
                dom: rem as i32 + 1,
                doy: doy as i32,
                leap_month: false,
            }
        }
        CalType::Chinese | CalType::Dangi => match lunisolar_from_jd(ct, jd) {
            Some((ry, m, d, leap)) => {
                let eyear = ry + chinese_offset(ct);
                let months = lunisolar_months(ct, ry);
                let ord = months.iter().position(|x| *x == (m, leap)).unwrap_or(0);
                let ystart = lunisolar_to_jd(ct, ry, 1, 1, false).unwrap_or(jd);
                Ymd {
                    era: clamp32(fdiv(eyear - 1, 60) + 1),
                    year: clamp32((eyear - 1).rem_euclid(60) + 1),
                    eyear: clamp32(eyear),
                    month: (m - 1) as i32,
                    ordinal_month: ord as i32,
                    dom: d as i32,
                    doy: (jd - ystart + 1) as i32,
                    leap_month: leap,
                }
            }
            None => {
                let mut f = fields_from_jd(CalType::Gregorian, jd, cutover);
                f.eyear += chinese_offset(ct) as i32;
                f
            }
        },
    }
}

/// The JDN of day `dom` (1-based, may overflow) of month `month`
/// (0-based, may overflow into other years) of extended year `eyear`.
pub fn jd_from(
    ct: CalType,
    eyear: i64,
    month: i64,
    dom: i64,
    leap_month: bool,
    cutover: i64,
) -> i64 {
    month_start(ct, eyear, month, leap_month, cutover) + dom - 1
}

/// The JDN of the first day of a month (month normalized across years).
pub fn month_start(ct: CalType, eyear: i64, month: i64, leap_month: bool, cutover: i64) -> i64 {
    match ct {
        CalType::Gregorian
        | CalType::Iso8601
        | CalType::Japanese
        | CalType::Buddhist
        | CalType::Roc => {
            let y = eyear + fdiv(month, 12);
            let m = month.rem_euclid(12);
            hybrid_year_start(y, cutover)
                + days_before_month(m as usize, hybrid_is_leap(y, cutover))
        }
        CalType::IslamicCivil | CalType::IslamicTbla => {
            let y = eyear + fdiv(month, 12);
            let m = month.rem_euclid(12);
            tabular_islamic_to_jd(ct, y, m + 1, 1)
        }
        CalType::IslamicUmalqura => {
            let y = eyear + fdiv(month, 12);
            let m = month.rem_euclid(12);
            intl::calendar::umalqura_to_jdn(y, m + 1, 1)
        }
        CalType::Islamic | CalType::IslamicRgsa => astro_month_start((eyear - 1) * 12 + month),
        CalType::Hebrew => {
            let mut y = eyear;
            let mut m = month;
            while m < 0 {
                y -= 1;
                m += 13;
            }
            while m > 12 {
                m -= 13;
                y += 1;
            }
            if !hebrew_leap(y) && m == 5 {
                m = 6;
            }
            let mut jd = hebrew_new_year(y);
            for i in 0..m as i32 {
                jd += hebrew_month_len(y, i) as i64;
            }
            jd
        }
        CalType::Persian => {
            let y = eyear + fdiv(month, 12);
            let m = month.rem_euclid(12);
            persian_year_start(y) + if m < 6 { 31 * m } else { 186 + 30 * (m - 6) }
        }
        CalType::Coptic | CalType::Ethiopic | CalType::EthiopicAmeteAlem => {
            let y = eyear + fdiv(month, 13);
            let m = month.rem_euclid(13);
            coptic_to_jd(ct, y, m + 1, 1)
        }
        CalType::Indian => {
            let y = eyear + fdiv(month, 12);
            let m = month.rem_euclid(12) as i32;
            let mut jd = indian_year_start(y);
            for i in 0..m {
                jd += indian_month_len(y, i) as i64;
            }
            jd
        }
        CalType::Chinese | CalType::Dangi => {
            let ry = eyear - chinese_offset(ct) + fdiv(month, 12);
            let m = month.rem_euclid(12) + 1;
            lunisolar_to_jd(ct, ry, m, 1, leap_month)
                .or_else(|| lunisolar_to_jd(ct, ry, m, 1, false))
                .unwrap_or_else(|| greg_to_jd(ry, m, 1))
        }
    }
}

/// The JDN of the first day of month number `ordinal` (0-based, counted
/// through leap months) of a year.
pub fn ordinal_month_start(ct: CalType, eyear: i64, ordinal: i64, cutover: i64) -> i64 {
    match ct {
        CalType::Hebrew => {
            let mut y = eyear;
            let mut o = ordinal;
            loop {
                let n = if hebrew_leap(y) { 13 } else { 12 };
                if o < 0 {
                    y -= 1;
                    o += if hebrew_leap(y) { 13 } else { 12 };
                } else if o >= n {
                    o -= n;
                    y += 1;
                } else {
                    break;
                }
            }
            let m = if !hebrew_leap(y) && o >= 5 { o + 1 } else { o };
            month_start(ct, y, m, false, cutover)
        }
        CalType::Chinese | CalType::Dangi => {
            let mut ry = eyear - chinese_offset(ct);
            let mut o = ordinal;
            loop {
                let months = lunisolar_months(ct, ry);
                let n = months.len() as i64;
                if o < 0 {
                    ry -= 1;
                    o += lunisolar_months(ct, ry).len() as i64;
                } else if o >= n {
                    o -= n;
                    ry += 1;
                } else {
                    let (m, l) = months[o as usize];
                    return lunisolar_to_jd(ct, ry, m, 1, l).unwrap_or(0);
                }
            }
        }
        _ => month_start(ct, eyear, ordinal, false, cutover),
    }
}

/// Days in a month (month normalized like [`month_start`]).
pub fn month_length(ct: CalType, eyear: i64, month: i64, leap_month: bool, cutover: i64) -> i64 {
    let (ny, nm) = match ct {
        CalType::Hebrew => {
            // the next existing month
            let start = month_start(ct, eyear, month, false, cutover);
            let f = fields_from_jd(ct, start, cutover);
            let next = month_start(ct, f.eyear as i64, f.month as i64 + 1, false, cutover);
            return next - start;
        }
        CalType::Chinese | CalType::Dangi => {
            let start = month_start(ct, eyear, month, leap_month, cutover);
            let f = fields_from_jd(ct, start, cutover);
            let next = ordinal_month_start(ct, f.eyear as i64, f.ordinal_month as i64 + 1, cutover);
            return next - start;
        }
        _ => (eyear, month + 1),
    };
    month_start(ct, ny, nm, false, cutover) - month_start(ct, eyear, month, leap_month, cutover)
}

/// Days in a year.
pub fn year_length(ct: CalType, eyear: i64, cutover: i64) -> i64 {
    match ct {
        CalType::Hebrew => hebrew_year_len(eyear),
        CalType::Chinese | CalType::Dangi => {
            let ry = eyear - chinese_offset(ct);
            match (
                lunisolar_to_jd(ct, ry + 1, 1, 1, false),
                lunisolar_to_jd(ct, ry, 1, 1, false),
            ) {
                (Some(a), Some(b)) => a - b,
                _ => 365,
            }
        }
        CalType::Coptic | CalType::Ethiopic | CalType::EthiopicAmeteAlem => {
            month_start(ct, eyear + 1, 0, false, cutover)
                - month_start(ct, eyear, 0, false, cutover)
        }
        _ => month_start(ct, eyear, 12, false, cutover) - month_start(ct, eyear, 0, false, cutover),
    }
}

/// Months in a year (13 in Hebrew leap years and lunisolar leap years).
pub fn months_in_year(ct: CalType, eyear: i64) -> i64 {
    match ct {
        CalType::Hebrew => {
            if hebrew_leap(eyear) {
                13
            } else {
                12
            }
        }
        CalType::Coptic | CalType::Ethiopic | CalType::EthiopicAmeteAlem => 13,
        CalType::Chinese | CalType::Dangi => {
            lunisolar_months(ct, eyear - chinese_offset(ct)).len() as i64
        }
        _ => 12,
    }
}

/// Whether a Hebrew year is a leap year.
pub fn is_hebrew_leap(y: i64) -> bool {
    hebrew_leap(y)
}

/// The extended year of (era, year).
pub fn eyear_from_era(ct: CalType, era: i64, year: i64) -> i64 {
    match ct {
        CalType::Gregorian | CalType::Iso8601 | CalType::Coptic => {
            if era == 0 {
                1 - year
            } else {
                year
            }
        }
        CalType::Japanese => {
            let e = era.clamp(0, sup::JAPANESE_ERAS.len() as i64 - 1) as usize;
            sup::JAPANESE_ERAS[e].0 as i64 + year - 1
        }
        CalType::Buddhist => year - 543,
        CalType::Roc => {
            if era == 0 {
                1912 - year
            } else {
                year + 1911
            }
        }
        CalType::Ethiopic => {
            if era == 0 {
                year - 5500
            } else {
                year
            }
        }
        CalType::EthiopicAmeteAlem => year - 5500,
        CalType::Chinese | CalType::Dangi => (era - 1) * 60 + year,
        _ => year,
    }
}

/// The era for an unset ERA field.
pub fn default_era(ct: CalType) -> i64 {
    match ct {
        CalType::Gregorian
        | CalType::Iso8601
        | CalType::Coptic
        | CalType::Ethiopic
        | CalType::Roc => 1,
        CalType::Japanese => sup::JAPANESE_ERAS.len() as i64 - 1,
        CalType::Chinese | CalType::Dangi => 1,
        _ => 0,
    }
}

/// Whether a (Gregorian) year is a leap year with this cutover.
pub fn gregorian_leap(year: i64, cutover: i64) -> bool {
    hybrid_is_leap(year, cutover)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUT: i64 = 2_299_161;

    #[test]
    fn gregorian() {
        assert_eq!(greg_to_jd(1970, 1, 1), EPOCH_JD);
        assert_eq!(jd_to_greg(EPOCH_JD), (1970, 1, 1));
        assert_eq!(jd_to_greg(greg_to_jd(-500, 3, 1)), (-500, 3, 1));
        assert_eq!(cutover_jd(-12_219_292_800_000.0), CUT);
        // 1582-10-04 Julian is the day before the cutover
        assert_eq!(julian_to_jd(1582, 10, 4), CUT - 1);
        let f = fields_from_jd(CalType::Gregorian, greg_to_jd(2012, 2, 29), CUT);
        assert_eq!((f.eyear, f.month, f.dom, f.doy), (2012, 1, 29, 60));
    }

    #[test]
    fn hebrew() {
        // 2012-01-01 = 6 Tevet 5772
        let f = fields_from_jd(CalType::Hebrew, greg_to_jd(2012, 1, 1), CUT);
        assert_eq!((f.eyear, f.month, f.dom), (5772, 3, 6));
        assert_eq!(
            jd_from(CalType::Hebrew, 5772, 3, 6, false, CUT),
            greg_to_jd(2012, 1, 1)
        );
    }

    #[test]
    fn islamic() {
        let jd = greg_to_jd(2012, 1, 1);
        let f = fields_from_jd(CalType::IslamicCivil, jd, CUT);
        assert_eq!((f.eyear, f.month, f.dom), (1433, 1, 6));
        let f = fields_from_jd(CalType::Islamic, jd, CUT);
        assert_eq!((f.eyear, f.month, f.dom), (1433, 1, 8));
        assert_eq!(jd_from(CalType::Islamic, 1433, 1, 8, false, CUT), jd);
    }

    #[test]
    fn persian() {
        // Nowruz 1391 = 2012-03-20, 1404 = 2025-03-21
        assert_eq!(persian_year_start(1391), greg_to_jd(2012, 3, 20));
        assert_eq!(persian_year_start(1404), greg_to_jd(2025, 3, 21));
    }

    #[test]
    fn others() {
        // Coptic new year 1728 = 2011-09-12; Ethiopic 2004 = 2011-09-12
        assert_eq!(
            coptic_to_jd(CalType::Coptic, 1728, 1, 1),
            greg_to_jd(2011, 9, 12)
        );
        assert_eq!(
            coptic_to_jd(CalType::Ethiopic, 2004, 1, 1),
            greg_to_jd(2011, 9, 12)
        );
        // Indian 1934-01-01 Saka = 2012-03-21 (leap year)
        assert_eq!(indian_year_start(1934), greg_to_jd(2012, 3, 21));
        let f = fields_from_jd(CalType::Japanese, greg_to_jd(2019, 5, 1), CUT);
        assert_eq!(f.year, 1);
    }
}
