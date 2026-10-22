//! The serial day number conversions of php-src's ext/calendar (gregor.c,
//! julian.c, jewish.c, french.c, dow.c), with C's `int` truncations.

// ---------------------------------------------------------------------------
// Gregorian
// ---------------------------------------------------------------------------

const GREGOR_SDN_OFFSET: i64 = 32045;
const DAYS_PER_5_MONTHS: i64 = 153;
const DAYS_PER_4_YEARS: i64 = 1461;
const DAYS_PER_400_YEARS: i64 = 146097;
const INT_MAX: i64 = i32::MAX as i64;

/// SdnToGregorian(): (year, month, day), all 0 when out of range.
pub fn sdn_to_gregorian(sdn: i64) -> (i32, i32, i32) {
    if sdn <= 0 || sdn > (i64::MAX - 4 * GREGOR_SDN_OFFSET) / 4 {
        return (0, 0, 0);
    }
    let mut temp = (sdn + GREGOR_SDN_OFFSET) * 4 - 1;
    if temp < 0 || (temp / DAYS_PER_400_YEARS) > INT_MAX {
        return (0, 0, 0);
    }
    let century = (temp / DAYS_PER_400_YEARS) as i32;
    temp = ((temp % DAYS_PER_400_YEARS) / 4) * 4 + 3;
    if century as i64 > ((INT_MAX / 100) - (temp / DAYS_PER_4_YEARS)) {
        return (0, 0, 0);
    }
    let mut year = (century as i64 * 100 + temp / DAYS_PER_4_YEARS) as i32;
    let day_of_year = ((temp % DAYS_PER_4_YEARS) / 4 + 1) as i32;
    let temp = day_of_year as i64 * 5 - 3;
    let mut month = (temp / DAYS_PER_5_MONTHS) as i32;
    let day = ((temp % DAYS_PER_5_MONTHS) / 5 + 1) as i32;
    if month < 10 {
        month += 3;
    } else {
        year = year.wrapping_add(1);
        month -= 9;
    }
    year = year.wrapping_sub(4800);
    if year <= 0 {
        year -= 1;
    }
    (year, month, day)
}

/// GregorianToSdn()
pub fn gregorian_to_sdn(input_year: i32, input_month: i32, input_day: i32) -> i64 {
    if input_year == 0
        || input_year < -4714
        || input_month <= 0
        || input_month > 12
        || input_day <= 0
        || input_day > 31
    {
        return 0;
    }
    if input_year == -4714 {
        if input_month < 11 {
            return 0;
        }
        if input_month == 11 && input_day < 25 {
            return 0;
        }
    }
    let mut year: i64 = if input_year < 0 {
        input_year as i64 + 4801
    } else {
        input_year as i64 + 4800
    };
    let month: i64 = if input_month > 2 {
        input_month as i64 - 3
    } else {
        year -= 1;
        input_month as i64 + 9
    };
    ((year / 100) * DAYS_PER_400_YEARS) / 4
        + ((year % 100) * DAYS_PER_4_YEARS) / 4
        + (month * DAYS_PER_5_MONTHS + 2) / 5
        + input_day as i64
        - GREGOR_SDN_OFFSET
}

pub const MONTH_NAME_SHORT: [&str; 13] = [
    "", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

pub const MONTH_NAME_LONG: [&str; 13] = [
    "",
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

// ---------------------------------------------------------------------------
// Julian
// ---------------------------------------------------------------------------

const JULIAN_SDN_OFFSET: i64 = 32083;

/// SdnToJulian()
pub fn sdn_to_julian(sdn: i64) -> (i32, i32, i32) {
    if sdn <= 0 {
        return (0, 0, 0);
    }
    if !(i64::MIN / 4..=(i64::MAX - JULIAN_SDN_OFFSET * 4 + 1) / 4).contains(&sdn) {
        return (0, 0, 0);
    }
    let temp = sdn * 4 + (JULIAN_SDN_OFFSET * 4 - 1);
    let yearl = temp / DAYS_PER_4_YEARS;
    if yearl > INT_MAX || yearl < i32::MIN as i64 {
        return (0, 0, 0);
    }
    let mut year = yearl as i32;
    let day_of_year = ((temp % DAYS_PER_4_YEARS) / 4 + 1) as i32;
    let temp = day_of_year as i64 * 5 - 3;
    let mut month = (temp / DAYS_PER_5_MONTHS) as i32;
    let day = ((temp % DAYS_PER_5_MONTHS) / 5 + 1) as i32;
    if month < 10 {
        month += 3;
    } else {
        year = year.wrapping_add(1);
        month -= 9;
    }
    year = year.wrapping_sub(4800);
    if year <= 0 {
        year = year.wrapping_sub(1);
    }
    (year, month, day)
}

/// JulianToSdn()
pub fn julian_to_sdn(input_year: i32, input_month: i32, input_day: i32) -> i64 {
    if input_year == 0
        || input_year < -4713
        || input_month <= 0
        || input_month > 12
        || input_day <= 0
        || input_day > 31
    {
        return 0;
    }
    if input_year == -4713 && input_month == 1 && input_day == 1 {
        return 0;
    }
    let mut year: i64 = if input_year < 0 {
        input_year as i64 + 4801
    } else {
        input_year as i64 + 4800
    };
    let month: i64 = if input_month > 2 {
        input_month as i64 - 3
    } else {
        year -= 1;
        input_month as i64 + 9
    };
    (year * DAYS_PER_4_YEARS) / 4 + (month * DAYS_PER_5_MONTHS + 2) / 5 + input_day as i64
        - JULIAN_SDN_OFFSET
}

// ---------------------------------------------------------------------------
// French republican
// ---------------------------------------------------------------------------

const FRENCH_SDN_OFFSET: i64 = 2375474;
const DAYS_PER_MONTH: i64 = 30;
const FIRST_VALID: i64 = 2375840;
const LAST_VALID: i64 = 2380952;

/// SdnToFrench()
pub fn sdn_to_french(sdn: i64) -> (i32, i32, i32) {
    if !(FIRST_VALID..=LAST_VALID).contains(&sdn) {
        return (0, 0, 0);
    }
    let temp = (sdn - FRENCH_SDN_OFFSET) * 4 - 1;
    let year = (temp / DAYS_PER_4_YEARS) as i32;
    let day_of_year = (temp % DAYS_PER_4_YEARS) / 4;
    let month = (day_of_year / DAYS_PER_MONTH + 1) as i32;
    let day = (day_of_year % DAYS_PER_MONTH + 1) as i32;
    (year, month, day)
}

/// FrenchToSdn()
pub fn french_to_sdn(year: i32, month: i32, day: i32) -> i64 {
    if !(1..=14).contains(&year) || !(1..=13).contains(&month) || !(1..=30).contains(&day) {
        return 0;
    }
    (year as i64 * DAYS_PER_4_YEARS) / 4
        + (month as i64 - 1) * DAYS_PER_MONTH
        + day as i64
        + FRENCH_SDN_OFFSET
}

pub const FRENCH_MONTH_NAME: [&str; 14] = [
    "",
    "Vendemiaire",
    "Brumaire",
    "Frimaire",
    "Nivose",
    "Pluviose",
    "Ventose",
    "Germinal",
    "Floreal",
    "Prairial",
    "Messidor",
    "Thermidor",
    "Fructidor",
    "Extra",
];

// ---------------------------------------------------------------------------
// Day of week
// ---------------------------------------------------------------------------

/// DayOfWeek(): 0 = Sunday
pub fn day_of_week(sdn: i64) -> i32 {
    ((sdn % 7 + 8) as i32) % 7
}

pub const DAY_NAME_SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
pub const DAY_NAME_LONG: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

// ---------------------------------------------------------------------------
// Jewish
// ---------------------------------------------------------------------------

const HALAKIM_PER_HOUR: i64 = 1080;
const HALAKIM_PER_DAY: i64 = 25920;
const HALAKIM_PER_LUNAR_CYCLE: i64 = (29 * HALAKIM_PER_DAY) + 13753;
const HALAKIM_PER_METONIC_CYCLE: i64 = HALAKIM_PER_LUNAR_CYCLE * (12 * 19 + 7);
const JEWISH_SDN_OFFSET: i64 = 347997;
const JEWISH_SDN_MAX: i64 = 324542846;
const NEW_MOON_OF_CREATION: i64 = 31524;
const SUNDAY: i32 = 0;
const MONDAY: i32 = 1;
const TUESDAY: i32 = 2;
const WEDNESDAY: i32 = 3;
const FRIDAY: i32 = 5;
const NOON: i64 = 18 * HALAKIM_PER_HOUR;
const AM3_11_20: i64 = (9 * HALAKIM_PER_HOUR) + 204;
const AM9_32_43: i64 = (15 * HALAKIM_PER_HOUR) + 589;

pub const MONTHS_PER_YEAR: [i32; 19] = [
    12, 12, 13, 12, 12, 13, 12, 13, 12, 12, 13, 12, 12, 13, 12, 12, 13, 12, 13,
];

const YEAR_OFFSET: [i64; 19] = [
    0, 12, 24, 37, 49, 61, 74, 86, 99, 111, 123, 136, 148, 160, 173, 185, 197, 210, 222,
];

pub const JEWISH_MONTH_NAME_LEAP: [&str; 14] = [
    "", "Tishri", "Heshvan", "Kislev", "Tevet", "Shevat", "Adar I", "Adar II", "Nisan", "Iyyar",
    "Sivan", "Tammuz", "Av", "Elul",
];

pub const JEWISH_MONTH_NAME: [&str; 14] = [
    "", "Tishri", "Heshvan", "Kislev", "Tevet", "Shevat", "", "Adar", "Nisan", "Iyyar", "Sivan",
    "Tammuz", "Av", "Elul",
];

/// ISO-8859-8
pub const JEWISH_MONTH_HEB_NAME_LEAP: [&[u8]; 14] = [
    b"",
    b"\xFA\xF9\xF8\xE9",
    b"\xE7\xF9\xE5\xEF",
    b"\xEB\xF1\xEC\xE5",
    b"\xE8\xE1\xFA",
    b"\xF9\xE1\xE8",
    b"\xE0\xE3\xF8 \xE0'",
    b"\xE0\xE3\xF8 \xE1'",
    b"\xF0\xE9\xF1\xEF",
    b"\xE0\xE9\xE9\xF8",
    b"\xF1\xE9\xe5\xEF",
    b"\xFA\xEE\xE5\xE6",
    b"\xE0\xE1",
    b"\xE0\xEC\xE5\xEC",
];

pub const JEWISH_MONTH_HEB_NAME: [&[u8]; 14] = [
    b"",
    b"\xFA\xF9\xF8\xE9",
    b"\xE7\xF9\xE5\xEF",
    b"\xEB\xF1\xEC\xE5",
    b"\xE8\xE1\xFA",
    b"\xF9\xE1\xE8",
    b"",
    b"\xE0\xE3\xF8",
    b"\xF0\xE9\xF1\xEF",
    b"\xE0\xE9\xE9\xF8",
    b"\xF1\xE9\xE5\xEF",
    b"\xFA\xEE\xE5\xE6",
    b"\xE0\xE1",
    b"\xE0\xEC\xE5\xEC",
];

/// A leap (13 months) Jewish year? (`year` > 0)
pub fn jewish_leap(year: i32) -> bool {
    MONTHS_PER_YEAR[(year - 1).rem_euclid(19) as usize] == 13
}

fn tishri1(metonic_year: i32, molad_day: i64, molad_halakim: i64) -> i64 {
    let mut tishri1 = molad_day;
    let mut dow = (tishri1 % 7) as i32;
    let leap_year = matches!(metonic_year, 2 | 5 | 7 | 10 | 13 | 16 | 18);
    let last_was_leap_year = matches!(metonic_year, 3 | 6 | 8 | 11 | 14 | 17 | 0);
    if molad_halakim >= NOON
        || (!leap_year && dow == TUESDAY && molad_halakim >= AM3_11_20)
        || (last_was_leap_year && dow == MONDAY && molad_halakim >= AM9_32_43)
    {
        tishri1 += 1;
        dow += 1;
        if dow == 7 {
            dow = 0;
        }
    }
    if dow == WEDNESDAY || dow == FRIDAY || dow == SUNDAY {
        tishri1 += 1;
    }
    tishri1
}

/// MoladOfMetonicCycle(): (moladDay, moladHalakim)
fn molad_of_metonic_cycle(metonic_cycle: i32) -> (i64, i64) {
    let mut r1: u64 = NEW_MOON_OF_CREATION as u64;
    let chk = metonic_cycle as i64;
    if chk > (i64::MAX - NEW_MOON_OF_CREATION) / (HALAKIM_PER_METONIC_CYCLE & 0xFFFF) {
        return (0, 0);
    }
    r1 = r1.wrapping_add((chk.wrapping_mul(HALAKIM_PER_METONIC_CYCLE & 0xFFFF)) as u64);
    // (signed / unsigned mix as in C: the comparison is done unsigned)
    let lim = (i64::MAX as u64).wrapping_sub(r1 >> 16)
        / ((HALAKIM_PER_METONIC_CYCLE >> 16) & 0xFFFF) as u64;
    if (chk as u64) > lim {
        return (0, 0);
    }
    let mut r2 = r1 >> 16;
    r2 = r2.wrapping_add(chk.wrapping_mul((HALAKIM_PER_METONIC_CYCLE >> 16) & 0xFFFF) as u64);
    let hpd = HALAKIM_PER_DAY as u64;
    let d2 = r2 / hpd;
    r2 -= d2 * hpd;
    r1 = (r2 << 16) | (r1 & 0xFFFF);
    let d1 = r1 / hpd;
    r1 -= d1 * hpd;
    (((d2 << 16) | d1) as i64, r1 as i64)
}

/// FindTishriMolad(): (metonicCycle, metonicYear, moladDay, moladHalakim)
fn find_tishri_molad(input_day: i64) -> (i32, i32, i64, i64) {
    let mut metonic_cycle = ((input_day + 310) / 6940) as i32;
    let (mut molad_day, mut molad_halakim) = molad_of_metonic_cycle(metonic_cycle);
    while molad_day < input_day - 6940 + 310 {
        metonic_cycle = metonic_cycle.wrapping_add(1);
        molad_halakim += HALAKIM_PER_METONIC_CYCLE;
        molad_day += molad_halakim / HALAKIM_PER_DAY;
        molad_halakim %= HALAKIM_PER_DAY;
    }
    let mut metonic_year = 0;
    while metonic_year < 18 {
        if molad_day > input_day - 74 {
            break;
        }
        molad_halakim += HALAKIM_PER_LUNAR_CYCLE * MONTHS_PER_YEAR[metonic_year as usize] as i64;
        molad_day += molad_halakim / HALAKIM_PER_DAY;
        molad_halakim %= HALAKIM_PER_DAY;
        metonic_year += 1;
    }
    (metonic_cycle, metonic_year, molad_day, molad_halakim)
}

/// FindStartOfYear(): (metonicCycle, metonicYear, moladDay, moladHalakim, tishri1)
fn find_start_of_year(year: i32) -> (i32, i32, i64, i64, i32) {
    let metonic_cycle = (year - 1) / 19;
    let metonic_year = (year - 1) % 19;
    let (mut molad_day, mut molad_halakim) = molad_of_metonic_cycle(metonic_cycle);
    molad_halakim += HALAKIM_PER_LUNAR_CYCLE * YEAR_OFFSET[metonic_year as usize];
    molad_day += molad_halakim / HALAKIM_PER_DAY;
    molad_halakim %= HALAKIM_PER_DAY;
    let t = tishri1(metonic_year, molad_day, molad_halakim) as i32;
    (metonic_cycle, metonic_year, molad_day, molad_halakim, t)
}

/// SdnToJewish()
pub fn sdn_to_jewish(sdn: i64) -> (i32, i32, i32) {
    if sdn <= JEWISH_SDN_OFFSET || sdn > JEWISH_SDN_MAX {
        return (0, 0, 0);
    }
    let input_day = sdn - JEWISH_SDN_OFFSET;
    let (mut metonic_cycle, mut metonic_year, mut day, mut halakim) = find_tishri_molad(input_day);
    let mut tishri1_ = tishri1(metonic_year, day, halakim) as i32 as i64;
    let year: i32;
    let tishri1_after: i64;
    if input_day >= tishri1_ {
        year = metonic_cycle * 19 + metonic_year + 1;
        if input_day < tishri1_ + 59 {
            if input_day < tishri1_ + 30 {
                return (year, 1, (input_day - tishri1_ + 1) as i32);
            } else {
                return (year, 2, (input_day - tishri1_ - 29) as i32);
            }
        }
        halakim += HALAKIM_PER_LUNAR_CYCLE * MONTHS_PER_YEAR[metonic_year as usize] as i64;
        day += halakim / HALAKIM_PER_DAY;
        halakim %= HALAKIM_PER_DAY;
        tishri1_after = tishri1((metonic_year + 1) % 19, day, halakim) as i32 as i64;
    } else {
        year = metonic_cycle * 19 + metonic_year;
        if input_day >= tishri1_ - 177 {
            let (m, d) = if input_day > tishri1_ - 30 {
                (13, input_day - tishri1_ + 30)
            } else if input_day > tishri1_ - 60 {
                (12, input_day - tishri1_ + 60)
            } else if input_day > tishri1_ - 89 {
                (11, input_day - tishri1_ + 89)
            } else if input_day > tishri1_ - 119 {
                (10, input_day - tishri1_ + 119)
            } else if input_day > tishri1_ - 148 {
                (9, input_day - tishri1_ + 148)
            } else {
                (8, input_day - tishri1_ + 178)
            };
            return (year, m, d as i32);
        }
        let mut month;
        let mut pday: i32;
        if MONTHS_PER_YEAR[(year - 1).rem_euclid(19) as usize] == 13 {
            month = 7;
            pday = (input_day - tishri1_ + 207) as i32;
            if pday > 0 {
                return (year, month, pday);
            }
            month -= 1;
            pday += 30;
            if pday > 0 {
                return (year, month, pday);
            }
            month -= 1;
            pday += 30;
        } else {
            month = 7;
            pday = (input_day - tishri1_ + 207) as i32;
            if pday > 0 {
                return (year, month, pday);
            }
            month -= 2;
            pday += 30;
        }
        if pday > 0 {
            return (year, month, pday);
        }
        month -= 1;
        pday += 29;
        if pday > 0 {
            return (year, month, pday);
        }
        tishri1_after = tishri1_;
        (metonic_cycle, metonic_year, day, halakim) = find_tishri_molad(day - 365);
        let _ = metonic_cycle;
        tishri1_ = tishri1(metonic_year, day, halakim) as i32 as i64;
    }
    let year_length = tishri1_after - tishri1_;
    let mut day = input_day - tishri1_ - 29;
    if year_length == 355 || year_length == 385 {
        if day <= 30 {
            return (year, 2, day as i32);
        }
        day -= 30;
    } else {
        if day <= 29 {
            return (year, 2, day as i32);
        }
        day -= 29;
    }
    (year, 3, day as i32)
}

/// JewishToSdn()
pub fn jewish_to_sdn(year: i32, month: i32, day: i32) -> i64 {
    if year <= 0 || year >= 6000 || day <= 0 || day > 30 {
        return 0;
    }
    let day = day as i64;
    let sdn: i64 = match month {
        1 | 2 => {
            let (_, _, _, _, t1) = find_start_of_year(year);
            if month == 1 {
                t1 as i64 + day - 1
            } else {
                t1 as i64 + day + 29
            }
        }
        3 => {
            let (_, metonic_year, mut molad_day, mut molad_halakim, t1) = find_start_of_year(year);
            molad_halakim +=
                HALAKIM_PER_LUNAR_CYCLE * MONTHS_PER_YEAR[metonic_year as usize] as i64;
            molad_day += molad_halakim / HALAKIM_PER_DAY;
            molad_halakim %= HALAKIM_PER_DAY;
            let after = tishri1((metonic_year + 1) % 19, molad_day, molad_halakim) as i32;
            let year_length = after - t1;
            if year_length == 355 || year_length == 385 {
                t1 as i64 + day + 59
            } else {
                t1 as i64 + day + 58
            }
        }
        4..=6 => {
            let (_, _, _, _, after) = find_start_of_year(year + 1);
            let adar = if MONTHS_PER_YEAR[(year - 1).rem_euclid(19) as usize] == 12 {
                29
            } else {
                59
            };
            let after = after as i64;
            match month {
                4 => after + day - adar - 237,
                5 => after + day - adar - 208,
                _ => after + day - adar - 178,
            }
        }
        _ => {
            let (_, _, _, _, after) = find_start_of_year(year + 1);
            let after = after as i64;
            match month {
                7 => after + day - 207,
                8 => after + day - 178,
                9 => after + day - 148,
                10 => after + day - 119,
                11 => after + day - 89,
                12 => after + day - 60,
                13 => after + day - 30,
                _ => return 0,
            }
        }
    };
    sdn + JEWISH_SDN_OFFSET
}
