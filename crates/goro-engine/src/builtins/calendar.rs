//! ext/calendar (PHP manual: Calendar functions): conversions between
//! Julian Day Numbers and the Gregorian, Julian, Jewish and French
//! republican calendars, Easter and Unix time.
//!
//! The day-number arithmetic is the textbook one (Fliegel & Van Flandern
//! for the Gregorian and Julian calendars; Dershowitz & Reingold,
//! "Calendrical Calculations", for the molad and postponement rules of the
//! Hebrew calendar; the Romme leap years 3, 7 and 11 for the French
//! calendar; the Gauss/Meeus computus for Easter). Ranges, argument
//! truncation, month numbering and messages follow the ext/calendar tests
//! and PHP 8.5's observed output.

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "calendar",
    functions: &[
        (
            "function cal_days_in_month(int $calendar, int $month, int $year): int",
            cal_days_in_month,
        ),
        (
            "function cal_from_jd(int $julian_day, int $calendar): array",
            cal_from_jd,
        ),
        ("function cal_info(int $calendar = -1): array", cal_info),
        (
            "function cal_to_jd(int $calendar, int $month, int $day, int $year): int",
            cal_to_jd,
        ),
        (
            "function easter_date(?int $year = null, int $mode = CAL_EASTER_DEFAULT): int",
            easter_date,
        ),
        (
            "function easter_days(?int $year = null, int $mode = CAL_EASTER_DEFAULT): int",
            easter_days,
        ),
        (
            "function frenchtojd(int $month, int $day, int $year): int",
            frenchtojd,
        ),
        (
            "function gregoriantojd(int $month, int $day, int $year): int",
            gregoriantojd,
        ),
        (
            "function jddayofweek(int $julian_day, int $mode = CAL_DOW_DAYNO): int|string",
            jddayofweek,
        ),
        (
            "function jdmonthname(int $julian_day, int $mode): string",
            jdmonthname,
        ),
        ("function jdtofrench(int $julian_day): string", jdtofrench),
        (
            "function jdtogregorian(int $julian_day): string",
            jdtogregorian,
        ),
        (
            "function jdtojewish(int $julian_day, bool $hebrew = false, int $flags = 0): string",
            jdtojewish,
        ),
        ("function jdtojulian(int $julian_day): string", jdtojulian),
        ("function jdtounix(int $julian_day): int", jdtounix),
        (
            "function jewishtojd(int $month, int $day, int $year): int",
            jewishtojd,
        ),
        (
            "function juliantojd(int $month, int $day, int $year): int",
            juliantojd,
        ),
        (
            "function unixtojd(?int $timestamp = null): int|false",
            unixtojd,
        ),
    ],
    constants: &[
        ("CAL_GREGORIAN", ConstVal::Int(0)),
        ("CAL_JULIAN", ConstVal::Int(1)),
        ("CAL_JEWISH", ConstVal::Int(2)),
        ("CAL_FRENCH", ConstVal::Int(3)),
        ("CAL_NUM_CALS", ConstVal::Int(4)),
        ("CAL_DOW_DAYNO", ConstVal::Int(0)),
        ("CAL_DOW_SHORT", ConstVal::Int(2)),
        ("CAL_DOW_LONG", ConstVal::Int(1)),
        ("CAL_MONTH_GREGORIAN_SHORT", ConstVal::Int(0)),
        ("CAL_MONTH_GREGORIAN_LONG", ConstVal::Int(1)),
        ("CAL_MONTH_JULIAN_SHORT", ConstVal::Int(2)),
        ("CAL_MONTH_JULIAN_LONG", ConstVal::Int(3)),
        ("CAL_MONTH_JEWISH", ConstVal::Int(4)),
        ("CAL_MONTH_FRENCH", ConstVal::Int(5)),
        ("CAL_EASTER_DEFAULT", ConstVal::Int(0)),
        ("CAL_EASTER_ROMAN", ConstVal::Int(1)),
        ("CAL_EASTER_ALWAYS_GREGORIAN", ConstVal::Int(2)),
        ("CAL_EASTER_ALWAYS_JULIAN", ConstVal::Int(3)),
        ("CAL_JEWISH_ADD_ALAFIM_GERESH", ConstVal::Int(2)),
        ("CAL_JEWISH_ADD_ALAFIM", ConstVal::Int(4)),
        ("CAL_JEWISH_ADD_GERESHAYIM", ConstVal::Int(8)),
    ],
    listed: true,
    ..Module::EMPTY
};

const GREGORIAN: i64 = 0;
const JULIAN: i64 = 1;
const JEWISH: i64 = 2;
const FRENCH: i64 = 3;

const MONTHS: [&str; 13] = [
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
const MONTHS_SHORT: [&str; 13] = [
    "", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
/// Jewish months in PHP's numbering (1 = Tishri; 6 = Adar I and 7 = Adar
/// II in leap years, 7 = Adar otherwise).
const JEWISH_MONTHS: [&str; 14] = [
    "", "Tishri", "Heshvan", "Kislev", "Tevet", "Shevat", "Adar I", "Adar II", "Nisan", "Iyyar",
    "Sivan", "Tammuz", "Av", "Elul",
];
const FRENCH_MONTHS: [&str; 14] = [
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
const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const DAYS_SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// The C `int` the calendar functions work on (observed on PHP 8.5:
/// `gregoriantojd(1, 1, PHP_INT_MAX)` is the JD of 1/1/-1).
fn c_int(v: i64) -> i64 {
    v as i32 as i64
}

// ---- Gregorian and Julian ----

/// Highest day numbers the conversions accept (observed on PHP 8.5; the
/// years beyond them do not fit a C int).
const GREGORIAN_MAX_JD: i64 = 784_350_656_097;
const JULIAN_MAX_JD: i64 = 784_368_370_349;

fn gregorian_to_jd(year: i64, month: i64, day: i64) -> i64 {
    if year == 0 || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return 0;
    }
    // Astronomical year numbering: 1 BCE is year 0.
    let y = if year < 0 { year + 1 } else { year };
    let a = (14 - month) / 12;
    let yy = y + 4800 - a;
    let mm = month + 12 * a - 3;
    let jd = day + (153 * mm + 2) / 5 + 365 * yy + yy.div_euclid(4) - yy.div_euclid(100)
        + yy.div_euclid(400)
        - 32045;
    jd.max(0)
}

fn julian_to_jd(year: i64, month: i64, day: i64) -> i64 {
    if year == 0 || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return 0;
    }
    let y = if year < 0 { year + 1 } else { year };
    let a = (14 - month) / 12;
    let yy = y + 4800 - a;
    let mm = month + 12 * a - 3;
    let jd = day + (153 * mm + 2) / 5 + 365 * yy + yy.div_euclid(4) - 32083;
    jd.max(0)
}

/// (year, month, day), all 0 outside the supported range.
fn jd_to_gregorian(jd: i64) -> (i64, i64, i64) {
    if jd <= 0 || jd > GREGORIAN_MAX_JD {
        return (0, 0, 0);
    }
    let a = jd + 32044;
    let b = (4 * a + 3) / 146097;
    let c = a - 146097 * b / 4;
    let d = (4 * c + 3) / 1461;
    let e = c - 1461 * d / 4;
    let m = (5 * e + 2) / 153;
    let day = e - (153 * m + 2) / 5 + 1;
    let month = m + 3 - 12 * (m / 10);
    let y = 100 * b + d - 4800 + m / 10;
    (if y <= 0 { y - 1 } else { y }, month, day)
}

fn jd_to_julian(jd: i64) -> (i64, i64, i64) {
    if jd <= 0 || jd > JULIAN_MAX_JD {
        return (0, 0, 0);
    }
    let c = jd + 32082;
    let d = (4 * c + 3) / 1461;
    let e = c - 1461 * d / 4;
    let m = (5 * e + 2) / 153;
    let day = e - (153 * m + 2) / 5 + 1;
    let month = m + 3 - 12 * (m / 10);
    let y = d - 4800 + m / 10;
    (if y <= 0 { y - 1 } else { y }, month, day)
}

// ---- French republican ----

const FRENCH_EPOCH: i64 = 2_375_840;
const FRENCH_LAST: i64 = 2_380_952;

fn french_to_jd(year: i64, month: i64, day: i64) -> i64 {
    if !(1..=14).contains(&year) || !(1..=13).contains(&month) || !(1..=30).contains(&day) {
        return 0;
    }
    FRENCH_EPOCH + (year - 1) * 365 + year / 4 + (month - 1) * 30 + day - 1
}

fn jd_to_french(jd: i64) -> (i64, i64, i64) {
    if !(FRENCH_EPOCH..=FRENCH_LAST).contains(&jd) {
        return (0, 0, 0);
    }
    let mut year = 1;
    while year < 14 && french_to_jd(year + 1, 1, 1) <= jd {
        year += 1;
    }
    let off = jd - french_to_jd(year, 1, 1);
    (year, off / 30 + 1, off % 30 + 1)
}

// ---- Jewish ----

/// JD of 1 Tishri AM 1 (observed: `jewishtojd(1, 1, 1)`).
const JEWISH_EPOCH: i64 = 347_998;
/// Last day jdtojewish() converts (observed on PHP 8.5).
const JEWISH_MAX_JD: i64 = 324_542_846;

fn jewish_leap(y: i64) -> bool {
    (7 * y + 1).rem_euclid(19) < 7
}

/// Days from the epoch's molad day to 1 Tishri of year `y` before the
/// year-length corrections (Dershowitz & Reingold).
fn jewish_elapsed(y: i64) -> i64 {
    let months = (235 * y - 234).div_euclid(19);
    let parts = 12084 + 13753 * months;
    let day = 29 * months + parts.div_euclid(25920);
    if (3 * (day + 1)).rem_euclid(7) < 3 {
        day + 1
    } else {
        day
    }
}

fn jewish_new_year(y: i64) -> i64 {
    let ny0 = jewish_elapsed(y - 1);
    let ny1 = jewish_elapsed(y);
    let ny2 = jewish_elapsed(y + 1);
    let corr = if ny2 - ny1 == 356 {
        2
    } else if ny1 - ny0 == 382 {
        1
    } else {
        0
    };
    JEWISH_EPOCH + ny1 + corr
}

/// Month lengths of year `y`, index 1 = Tishri (Adar I is 0 days long in
/// common years).
fn jewish_month_lengths(y: i64) -> [i64; 14] {
    let len = jewish_new_year(y + 1) - jewish_new_year(y);
    let heshvan = if len % 10 == 5 { 30 } else { 29 };
    let kislev = if len % 10 == 3 { 29 } else { 30 };
    let adar1 = if jewish_leap(y) { 30 } else { 0 };
    [
        0, 30, heshvan, kislev, 29, 30, adar1, 29, 30, 29, 30, 29, 30, 29,
    ]
}

fn jewish_to_jd(year: i64, month: i64, day: i64) -> i64 {
    // Observed on PHP 8.5: years from 6000 on give 0.
    if !(1..=5999).contains(&year) || !(1..=13).contains(&month) || !(1..=30).contains(&day) {
        return 0;
    }
    let lens = jewish_month_lengths(year);
    let mut jd = jewish_new_year(year);
    for l in &lens[1..month as usize] {
        jd += l;
    }
    jd + day - 1
}

fn jd_to_jewish(jd: i64) -> (i64, i64, i64) {
    if !(JEWISH_EPOCH..=JEWISH_MAX_JD).contains(&jd) {
        return (0, 0, 0);
    }
    // Mean year: 235 months of 765433/25920 days in 19 years.
    let mut y = ((jd - JEWISH_EPOCH) as i128 * 19 * 25920 / (235 * 765433)) as i64 + 1;
    while jewish_new_year(y) > jd {
        y -= 1;
    }
    while jewish_new_year(y + 1) <= jd {
        y += 1;
    }
    let lens = jewish_month_lengths(y);
    let mut off = jd - jewish_new_year(y);
    let mut m = 1;
    while m < 13 && off >= lens[m] {
        off -= lens[m];
        m += 1;
    }
    (y, m as i64, off + 1)
}

fn jewish_month_name(year: i64, month: i64) -> &'static str {
    if month == 7 && !jewish_leap(year) {
        "Adar"
    } else {
        JEWISH_MONTHS.get(month as usize).copied().unwrap_or("")
    }
}

/// Hebrew letters (ISO-8859-8).
const HEB_UNITS: [u8; 10] = [0, 0xe0, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8];
const HEB_TENS: [u8; 10] = [0, 0xe9, 0xeb, 0xec, 0xee, 0xf0, 0xf1, 0xf2, 0xf4, 0xf6];
const HEB_HUNDREDS: [u8; 5] = [0, 0xf7, 0xf8, 0xf9, 0xfa];
const HEB_ALAFIM: &[u8] = b"\xe0\xec\xf4\xe9\xed";

const ALAFIM_GERESH: i64 = 2;
const ALAFIM: i64 = 4;
const GERESHAYIM: i64 = 8;

/// A number in Hebrew letters (observed on PHP 8.5): the thousands digit
/// first (with an optional geresh and the word "alafim"), then the rest
/// with 15 and 16 written 9+6 and 9+7; GERESHAYIM marks a lone letter
/// with a geresh and puts a gershayim before the last of several.
fn hebrew_number(mut n: i64, flags: i64, out: &mut Vec<u8>) {
    if n >= 1000 {
        out.push(HEB_UNITS[(n / 1000 % 10) as usize]);
        if flags & ALAFIM_GERESH != 0 {
            out.push(b'\'');
        }
        if flags & ALAFIM != 0 {
            out.push(b' ');
            out.extend_from_slice(HEB_ALAFIM);
            out.push(b' ');
        }
        n %= 1000;
    }
    let mut letters = Vec::new();
    while n >= 400 {
        letters.push(HEB_HUNDREDS[4]);
        n -= 400;
    }
    if n >= 100 {
        letters.push(HEB_HUNDREDS[(n / 100) as usize]);
        n %= 100;
    }
    if n == 15 || n == 16 {
        letters.push(HEB_UNITS[9]);
        letters.push(HEB_UNITS[(n - 9) as usize]);
    } else {
        if n >= 10 {
            letters.push(HEB_TENS[(n / 10) as usize]);
        }
        if n % 10 != 0 {
            letters.push(HEB_UNITS[(n % 10) as usize]);
        }
    }
    if flags & GERESHAYIM != 0 && !letters.is_empty() {
        if letters.len() == 1 {
            letters.push(b'\'');
        } else {
            letters.insert(letters.len() - 1, b'"');
        }
    }
    out.extend_from_slice(&letters);
}

fn hebrew_month_name(year: i64, month: i64) -> &'static [u8] {
    match month {
        1 => b"\xfa\xf9\xf8\xe9",
        2 => b"\xe7\xf9\xe5\xef",
        3 => b"\xeb\xf1\xec\xe5",
        4 => b"\xe8\xe1\xfa",
        5 => b"\xf9\xe1\xe8",
        6 => b"\xe0\xe3\xf8 \xe0'",
        7 if jewish_leap(year) => b"\xe0\xe3\xf8 \xe1'",
        7 => b"\xe0\xe3\xf8",
        8 => b"\xf0\xe9\xf1\xef",
        9 => b"\xe0\xe9\xe9\xf8",
        10 => b"\xf1\xe9\xe5\xef",
        11 => b"\xfa\xee\xe5\xe6",
        12 => b"\xe0\xe1",
        13 => b"\xe0\xec\xe5\xec",
        _ => b"",
    }
}

// ---- dispatch ----

fn to_jd(cal: i64, year: i64, month: i64, day: i64) -> i64 {
    match cal {
        GREGORIAN => gregorian_to_jd(year, month, day),
        JULIAN => julian_to_jd(year, month, day),
        JEWISH => jewish_to_jd(year, month, day),
        _ => french_to_jd(year, month, day),
    }
}

fn from_jd(cal: i64, jd: i64) -> (i64, i64, i64) {
    match cal {
        GREGORIAN => jd_to_gregorian(jd),
        JULIAN => jd_to_julian(jd),
        JEWISH => jd_to_jewish(jd),
        _ => jd_to_french(jd),
    }
}

fn day_of_week(jd: i64) -> i64 {
    (jd % 7 + 1).rem_euclid(7)
}

fn s(v: &str) -> Value {
    Value::Str(PhpStr::from(v))
}

fn date_str(y: i64, m: i64, d: i64) -> Value {
    s(&format!("{}/{}/{}", m, d, y))
}

fn check_cal(vm: &mut Vm, a: &Args, cal: i64, argn: usize) -> R<()> {
    if !(0..4).contains(&cal) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #{} ($calendar) must be a valid calendar ID", argn),
        );
    }
    Ok(())
}

fn month_names(names: &[&str]) -> Value {
    let mut arr = PhpArray::new();
    for (i, n) in names.iter().enumerate().skip(1) {
        arr.insert(Key::Int(i as i64), s(n));
    }
    Value::Array(arr)
}

fn info_for(cal: i64) -> Value {
    let (months, short, max, name, sym): (&[&str], &[&str], i64, &str, &str) = match cal {
        GREGORIAN => (&MONTHS, &MONTHS_SHORT, 31, "Gregorian", "CAL_GREGORIAN"),
        JULIAN => (&MONTHS, &MONTHS_SHORT, 31, "Julian", "CAL_JULIAN"),
        JEWISH => (&JEWISH_MONTHS, &JEWISH_MONTHS, 30, "Jewish", "CAL_JEWISH"),
        _ => (&FRENCH_MONTHS, &FRENCH_MONTHS, 30, "French", "CAL_FRENCH"),
    };
    let mut arr = PhpArray::new();
    arr.insert(Key::Str(PhpStr::from("months")), month_names(months));
    arr.insert(Key::Str(PhpStr::from("abbrevmonths")), month_names(short));
    arr.insert(Key::Str(PhpStr::from("maxdaysinmonth")), Value::Int(max));
    arr.insert(Key::Str(PhpStr::from("calname")), s(name));
    arr.insert(Key::Str(PhpStr::from("calsymbol")), s(sym));
    Value::Array(arr)
}

fn cal_info(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cal = a.int_or(0, -1);
    if cal == -1 {
        let mut arr = PhpArray::new();
        for c in 0..4 {
            arr.insert(Key::Int(c), info_for(c));
        }
        return Ok(Value::Array(arr));
    }
    check_cal(vm, a, cal, 1)?;
    Ok(info_for(cal))
}

/// The month and year checks of cal_days_in_month() and cal_to_jd() (test
/// gh19371.phpt).
fn check_month_year(
    vm: &mut Vm,
    a: &Args,
    month: i64,
    year: i64,
    margn: usize,
    yargn: usize,
) -> R<()> {
    if !(1..=i32::MAX as i64 - 1).contains(&month) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} ($month) must be between 1 and {}",
                margn,
                i32::MAX - 1
            ),
        );
    }
    if year > i32::MAX as i64 - 1 {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} ($year) must be less than {}",
                yargn,
                i32::MAX - 1
            ),
        );
    }
    Ok(())
}

fn cal_days_in_month(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cal = a.int(0);
    let month = a.int(1);
    let year = a.int(2);
    check_cal(vm, a, cal, 1)?;
    check_month_year(vm, a, month, year, 2, 3)?;
    let (month, year) = (c_int(month), c_int(year));
    let start = to_jd(cal, year, month, 1);
    if start == 0 {
        return vm.value_error("Invalid date");
    }
    let mut next = to_jd(cal, year, month + 1, 1);
    if next == 0 {
        let ny = if year == -1 { 1 } else { year + 1 };
        next = to_jd(cal, ny, 1, 1);
        if next == 0 && cal == FRENCH {
            next = FRENCH_LAST + 1;
        }
    }
    Ok(Value::Int(next - start))
}

fn cal_to_jd(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cal = a.int(0);
    let month = a.int(1);
    let day = a.int(2);
    let year = a.int(3);
    check_cal(vm, a, cal, 1)?;
    if !(1..=i32::MAX as i64 - 1).contains(&month) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #2 ($month) must be between 1 and {}",
                i32::MAX - 1
            ),
        );
    }
    if !(i32::MIN as i64..=i32::MAX as i64).contains(&day) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #3 ($day) must be between {} and {}",
                i32::MIN,
                i32::MAX
            ),
        );
    }
    if year > i32::MAX as i64 - 1 {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #4 ($year) must be less than {}", i32::MAX - 1),
        );
    }
    Ok(Value::Int(to_jd(cal, c_int(year), month, day)))
}

fn cal_from_jd(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let jd = a.int(0);
    let cal = a.int(1);
    check_cal(vm, a, cal, 2)?;
    let (y, m, d) = from_jd(cal, jd);
    let mut arr = PhpArray::new();
    let k = |n: &str| Key::Str(PhpStr::from(n));
    arr.insert(k("date"), date_str(y, m, d));
    arr.insert(k("month"), Value::Int(m));
    arr.insert(k("day"), Value::Int(d));
    arr.insert(k("year"), Value::Int(y));
    if cal == JEWISH && y == 0 {
        // Observed on PHP 8.5 (test bug71894.phpt).
        arr.insert(k("dow"), Value::Null);
        arr.insert(k("abbrevdayname"), s(""));
        arr.insert(k("dayname"), s(""));
    } else {
        let dow = day_of_week(jd);
        arr.insert(k("dow"), Value::Int(dow));
        arr.insert(k("abbrevdayname"), s(DAYS_SHORT[dow as usize]));
        arr.insert(k("dayname"), s(DAYS[dow as usize]));
    }
    let (short, long) = match cal {
        GREGORIAN | JULIAN => (MONTHS_SHORT[m as usize], MONTHS[m as usize]),
        JEWISH => {
            let n = if y == 0 { "" } else { jewish_month_name(y, m) };
            (n, n)
        }
        _ => (FRENCH_MONTHS[m as usize], FRENCH_MONTHS[m as usize]),
    };
    arr.insert(k("abbrevmonth"), s(short));
    arr.insert(k("monthname"), s(long));
    Ok(Value::Array(arr))
}

fn gregoriantojd(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(gregorian_to_jd(
        c_int(a.int(2)),
        c_int(a.int(0)),
        c_int(a.int(1)),
    )))
}

fn juliantojd(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(julian_to_jd(
        c_int(a.int(2)),
        c_int(a.int(0)),
        c_int(a.int(1)),
    )))
}

fn frenchtojd(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(french_to_jd(
        c_int(a.int(2)),
        c_int(a.int(0)),
        c_int(a.int(1)),
    )))
}

fn jewishtojd(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let year = a.int(2);
    if !(i32::MIN as i64..=i32::MAX as i64).contains(&year) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #3 ($year) must be between {} and {}",
                i32::MIN,
                i32::MAX
            ),
        );
    }
    Ok(Value::Int(jewish_to_jd(
        year,
        c_int(a.int(0)),
        c_int(a.int(1)),
    )))
}

fn jdtogregorian(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (y, m, d) = jd_to_gregorian(a.int(0));
    Ok(date_str(y, m, d))
}

fn jdtojulian(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (y, m, d) = jd_to_julian(a.int(0));
    Ok(date_str(y, m, d))
}

fn jdtofrench(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (y, m, d) = jd_to_french(a.int(0));
    Ok(date_str(y, m, d))
}

fn jdtojewish(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (y, m, d) = jd_to_jewish(a.int(0));
    if !a.bool_or(1, false) {
        return Ok(date_str(y, m, d));
    }
    if !(1..=9999).contains(&y) {
        return vm.value_error("Year out of range (0-9999)");
    }
    let flags = a.int_or(2, 0);
    let mut out = Vec::new();
    hebrew_number(d, flags, &mut out);
    out.push(b' ');
    out.extend_from_slice(hebrew_month_name(y, m));
    out.push(b' ');
    hebrew_number(y, flags, &mut out);
    Ok(Value::Str(PhpStr::from(out)))
}

fn jddayofweek(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let dow = day_of_week(a.int(0)) as usize;
    Ok(match a.int_or(1, 0) {
        1 => s(DAYS[dow]),
        2 => s(DAYS_SHORT[dow]),
        _ => Value::Int(dow as i64),
    })
}

fn jdmonthname(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let jd = a.int(0);
    let name = match a.int(1) {
        1 => MONTHS[jd_to_gregorian(jd).1 as usize],
        2 => MONTHS_SHORT[jd_to_julian(jd).1 as usize],
        3 => MONTHS[jd_to_julian(jd).1 as usize],
        4 => {
            let (y, m, _) = jd_to_jewish(jd);
            if y == 0 { "" } else { jewish_month_name(y, m) }
        }
        5 => FRENCH_MONTHS[jd_to_french(jd).1 as usize],
        _ => MONTHS_SHORT[jd_to_gregorian(jd).1 as usize],
    };
    Ok(s(name))
}

// ---- Easter and Unix time ----

/// Days from March 21 to Easter Sunday: the Julian computus, or the
/// Gregorian one (Meeus/Butcher) for `gregorian`.
fn easter_offset(year: i64, gregorian: bool) -> i64 {
    let (month, day) = if gregorian {
        let a = year % 19;
        let b = year / 100;
        let c = year % 100;
        let d = b / 4;
        let e = b % 4;
        let f = (b + 8) / 25;
        let g = (b - f + 1) / 3;
        let h = (19 * a + b - d - g + 15) % 30;
        let i = c / 4;
        let k = c % 4;
        let l = (32 + 2 * e + 2 * i - h - k) % 7;
        let m = (a + 11 * h + 22 * l) / 451;
        let n = h + l - 7 * m + 114;
        (n / 31, n % 31 + 1)
    } else {
        let a = year % 4;
        let b = year % 7;
        let c = year % 19;
        let d = (19 * c + 15) % 30;
        let e = (2 * a + 4 * b - d + 34) % 7;
        let n = d + e + 114;
        (n / 31, n % 31 + 1)
    };
    if month == 3 { day - 21 } else { day + 10 }
}

/// Which computus a mode picks (PHP manual: easter_days; CAL_EASTER_DEFAULT
/// switches at the British reform of 1752, CAL_EASTER_ROMAN at 1582).
fn easter_is_gregorian(year: i64, mode: i64) -> bool {
    match mode {
        3 => false,
        2 => true,
        1 => year > 1582,
        _ => year > 1752,
    }
}

/// The time zone of the C library's local time (the TZ variable; UTC when
/// unset), which easter_date() and unixtojd() use (observed on PHP 8.5:
/// date.timezone does not change them, putenv("TZ=...") does).
fn c_local_zone() -> goro_ext_date::TimeZone {
    match std::env::var("TZ") {
        Ok(tz) => {
            let tz = tz.strip_prefix(':').unwrap_or(&tz);
            goro_ext_date::TimeZone::from_id(tz).unwrap_or_else(goro_ext_date::TimeZone::utc)
        }
        Err(_) => goro_ext_date::TimeZone::utc(),
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn current_year() -> i64 {
    let ts = now();
    let local = ts + c_local_zone().offset_at(ts) as i64;
    goro_ext_date::civil::civil_from_days(local.div_euclid(86400)).0
}

/// Upper bound of easter_days()/easter_date() years (observed on PHP 8.5).
const EASTER_MAX: i64 = i64::MAX / 5 * 4;

fn easter_year(vm: &mut Vm, a: &Args) -> R<i64> {
    let year = match a.opt_int(0) {
        Some(y) => y,
        None => current_year(),
    };
    if !(1..=EASTER_MAX).contains(&year) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #1 ($year) must be between 1 and {}", EASTER_MAX),
        );
    }
    Ok(year)
}

fn easter_days(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let year = easter_year(vm, a)?;
    let mode = a.int_or(1, 0);
    Ok(Value::Int(easter_offset(
        year,
        easter_is_gregorian(year, mode),
    )))
}

fn easter_date(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let year = easter_year(vm, a)?;
    if year < 1970 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($year) must be a year after 1970 (inclusive)",
        );
    }
    if year > 2_000_000_000 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($year) must be a year before 2.000.000.000 (inclusive)",
        );
    }
    let mode = a.int_or(1, 0);
    let off = easter_offset(year, easter_is_gregorian(year, mode));
    // The day counts from March 21 of the Gregorian calendar, as mktime()
    // reads it.
    let days = goro_ext_date::civil::days_from_civil(year, 3, 21) + off;
    let ts = c_local_zone().local_to_ts(days * 86400);
    Ok(Value::Int(ts))
}

fn unixtojd(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let ts = match a.opt_int(0) {
        Some(t) => t,
        None => now(),
    };
    if ts < 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($timestamp) must be greater than or equal to 0",
        );
    }
    let local = ts.checked_add(c_local_zone().offset_at(ts) as i64);
    let Some(local) = local else {
        return Ok(Value::Bool(false));
    };
    let (y, m, d) = goro_ext_date::civil::civil_from_days(local.div_euclid(86400));
    // localtime() fails when the year does not fit a C int.
    if y > i32::MAX as i64 - 1900 {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Int(gregorian_to_jd(y, m as i64, d as i64)))
}

fn jdtounix(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let jd = a.int(0);
    let max = i64::MAX / 86400 + 2_440_588;
    if !(2_440_588..=max).contains(&jd) {
        return vm.value_error(&format!("jday must be between 2440588 and {}", max));
    }
    Ok(Value::Int((jd - 2_440_588) * 86400))
}
