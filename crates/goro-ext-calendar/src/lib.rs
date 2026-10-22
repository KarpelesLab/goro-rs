//! php-src's ext/calendar: conversions between Julian Day Counts and the
//! Gregorian, Julian, Jewish and French republican calendars, plus
//! easter_date()/easter_days() and unixtojd()/jdtounix().

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

pub mod sdncal;

use sdncal::*;

/// The extension name (`extension_loaded('calendar')`).
pub const NAME: &[u8] = b"calendar";

type R = Result<Value, VmError>;

const CAL_GREGORIAN: i64 = 0;
const CAL_JULIAN: i64 = 1;
const CAL_JEWISH: i64 = 2;
const CAL_FRENCH: i64 = 3;
const CAL_NUM_CALS: i64 = 4;

const CAL_DOW_DAYNO: i64 = 0;
const CAL_DOW_LONG: i64 = 1;
const CAL_DOW_SHORT: i64 = 2;

const CAL_MONTH_GREGORIAN_SHORT: i64 = 0;
const CAL_MONTH_GREGORIAN_LONG: i64 = 1;
const CAL_MONTH_JULIAN_SHORT: i64 = 2;
const CAL_MONTH_JULIAN_LONG: i64 = 3;
const CAL_MONTH_JEWISH: i64 = 4;
const CAL_MONTH_FRENCH: i64 = 5;

const CAL_EASTER_DEFAULT: i64 = 0;
const CAL_EASTER_ROMAN: i64 = 1;
const CAL_EASTER_ALWAYS_GREGORIAN: i64 = 2;
const CAL_EASTER_ALWAYS_JULIAN: i64 = 3;

const CAL_JEWISH_ADD_ALAFIM_GERESH: i64 = 0x2;
const CAL_JEWISH_ADD_ALAFIM: i64 = 0x4;
const CAL_JEWISH_ADD_GERESHAYIM: i64 = 0x8;

const CONSTANTS: &[(&str, i64)] = &[
    ("CAL_GREGORIAN", CAL_GREGORIAN),
    ("CAL_JULIAN", CAL_JULIAN),
    ("CAL_JEWISH", CAL_JEWISH),
    ("CAL_FRENCH", CAL_FRENCH),
    ("CAL_NUM_CALS", CAL_NUM_CALS),
    ("CAL_DOW_DAYNO", CAL_DOW_DAYNO),
    ("CAL_DOW_SHORT", CAL_DOW_SHORT),
    ("CAL_DOW_LONG", CAL_DOW_LONG),
    ("CAL_MONTH_GREGORIAN_SHORT", CAL_MONTH_GREGORIAN_SHORT),
    ("CAL_MONTH_GREGORIAN_LONG", CAL_MONTH_GREGORIAN_LONG),
    ("CAL_MONTH_JULIAN_SHORT", CAL_MONTH_JULIAN_SHORT),
    ("CAL_MONTH_JULIAN_LONG", CAL_MONTH_JULIAN_LONG),
    ("CAL_MONTH_JEWISH", CAL_MONTH_JEWISH),
    ("CAL_MONTH_FRENCH", CAL_MONTH_FRENCH),
    ("CAL_EASTER_DEFAULT", CAL_EASTER_DEFAULT),
    ("CAL_EASTER_ROMAN", CAL_EASTER_ROMAN),
    ("CAL_EASTER_ALWAYS_GREGORIAN", CAL_EASTER_ALWAYS_GREGORIAN),
    ("CAL_EASTER_ALWAYS_JULIAN", CAL_EASTER_ALWAYS_JULIAN),
    ("CAL_JEWISH_ADD_ALAFIM_GERESH", CAL_JEWISH_ADD_ALAFIM_GERESH),
    ("CAL_JEWISH_ADD_ALAFIM", CAL_JEWISH_ADD_ALAFIM),
    ("CAL_JEWISH_ADD_GERESHAYIM", CAL_JEWISH_ADD_GERESHAYIM),
];

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("cal_days_in_month", cal_days_in_month),
    ("cal_from_jd", cal_from_jd),
    ("cal_info", cal_info),
    ("cal_to_jd", cal_to_jd),
    ("easter_date", easter_date),
    ("easter_days", easter_days),
    ("frenchtojd", frenchtojd),
    ("gregoriantojd", gregoriantojd),
    ("jddayofweek", jddayofweek),
    ("jdmonthname", jdmonthname),
    ("jdtofrench", jdtofrench),
    ("jdtogregorian", jdtogregorian),
    ("jdtojewish", jdtojewish),
    ("jdtojulian", jdtojulian),
    ("jdtounix", jdtounix),
    ("jewishtojd", jewishtojd),
    ("juliantojd", juliantojd),
    ("unixtojd", unixtojd),
];

/// MINIT
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    for &(name, f) in FUNCTIONS {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), f, &params);
        vm.zpp_extension_verified.insert(
            name.as_bytes().to_vec(),
            goro_core::arginfo::CHECK_ARITY | goro_core::arginfo::CHECK_TYPES,
        );
    }
    for &(name, v) in CONSTANTS {
        vm.constants
            .insert(name.as_bytes().to_vec(), Value::Long(v));
    }
    vm.module_constants.push((
        NAME.to_vec(),
        CONSTANTS
            .iter()
            .map(|(n, _)| n.as_bytes().to_vec())
            .collect(),
    ));
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn throw(vm: &mut Vm, class: &[u8], msg: &str) -> R {
    let exc = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(exc);
    Ok(Value::Null)
}

fn value_error(vm: &mut Vm, func: &str, n: usize, what: &str) -> R {
    let name = goro_core::stubs::find_function(func.as_bytes())
        .and_then(|f| f.args.get(n - 1))
        .map(|a| a.name)
        .unwrap_or("");
    let msg = format!("{}(): Argument #{} (${}) {}", func, n, name, what);
    throw(vm, b"ValueError", &msg)
}

fn arg(args: &[Value], i: usize) -> Value {
    match args.get(i) {
        Some(v) => v.deref(),
        None => Value::Undef,
    }
}

fn long(args: &[Value], i: usize) -> i64 {
    arg(args, i).to_long()
}

/// An optional `?int` argument.
fn opt_long(args: &[Value], i: usize) -> Option<i64> {
    match arg(args, i) {
        Value::Undef | Value::Null => None,
        v => Some(v.to_long()),
    }
}

fn s(v: &str) -> Value {
    Value::String(PhpString::from_string(v.to_string()))
}

fn b(v: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(v))
}

fn arr(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

fn set(a: &mut PhpArray, k: &str, v: Value) {
    a.set(ArrayKey::String(PhpString::from_string(k.to_string())), v);
}

fn date_str(year: i32, month: i32, day: i32) -> Value {
    s(&format!("{}/{}/{}", month, day, year))
}

// ---------------------------------------------------------------------------
// The calendar table
// ---------------------------------------------------------------------------

struct CalEntry {
    name: &'static str,
    symbol: &'static str,
    to_jd: fn(i32, i32, i32) -> i64,
    from_jd: fn(i64) -> (i32, i32, i32),
    num_months: usize,
    max_days_in_month: i64,
    month_name_short: &'static [&'static str],
    month_name_long: &'static [&'static str],
}

static CALS: [CalEntry; 4] = [
    CalEntry {
        name: "Gregorian",
        symbol: "CAL_GREGORIAN",
        to_jd: gregorian_to_sdn,
        from_jd: sdn_to_gregorian,
        num_months: 12,
        max_days_in_month: 31,
        month_name_short: &MONTH_NAME_SHORT,
        month_name_long: &MONTH_NAME_LONG,
    },
    CalEntry {
        name: "Julian",
        symbol: "CAL_JULIAN",
        to_jd: julian_to_sdn,
        from_jd: sdn_to_julian,
        num_months: 12,
        max_days_in_month: 31,
        month_name_short: &MONTH_NAME_SHORT,
        month_name_long: &MONTH_NAME_LONG,
    },
    CalEntry {
        name: "Jewish",
        symbol: "CAL_JEWISH",
        to_jd: jewish_to_sdn,
        from_jd: sdn_to_jewish,
        num_months: 13,
        max_days_in_month: 30,
        month_name_short: &JEWISH_MONTH_NAME_LEAP,
        month_name_long: &JEWISH_MONTH_NAME_LEAP,
    },
    CalEntry {
        name: "French",
        symbol: "CAL_FRENCH",
        to_jd: french_to_sdn,
        from_jd: sdn_to_french,
        num_months: 13,
        max_days_in_month: 30,
        month_name_short: &FRENCH_MONTH_NAME,
        month_name_long: &FRENCH_MONTH_NAME,
    },
];

fn jewish_month_name(year: i32) -> &'static [&'static str; 14] {
    if jewish_leap(year) {
        &JEWISH_MONTH_NAME_LEAP
    } else {
        &JEWISH_MONTH_NAME
    }
}

fn idx<T: Copy + Default>(t: &[T], i: i32) -> T {
    t.get(i as usize).copied().unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

fn php_cal_info(cal: usize) -> Value {
    let c = &CALS[cal];
    let mut months = PhpArray::new();
    let mut smonths = PhpArray::new();
    for i in 1..=c.num_months {
        months.set(ArrayKey::Int(i as i64), s(c.month_name_long[i]));
        smonths.set(ArrayKey::Int(i as i64), s(c.month_name_short[i]));
    }
    let mut ret = PhpArray::new();
    set(&mut ret, "months", arr(months));
    set(&mut ret, "abbrevmonths", arr(smonths));
    set(&mut ret, "maxdaysinmonth", Value::Long(c.max_days_in_month));
    set(&mut ret, "calname", s(c.name));
    set(&mut ret, "calsymbol", s(c.symbol));
    arr(ret)
}

fn cal_info(vm: &mut Vm, args: &[Value]) -> R {
    let cal = match arg(args, 0) {
        Value::Undef => -1,
        v => v.to_long(),
    };
    if cal == -1 {
        let mut ret = PhpArray::new();
        for i in 0..CAL_NUM_CALS as usize {
            ret.set(ArrayKey::Int(i as i64), php_cal_info(i));
        }
        return Ok(arr(ret));
    }
    if !(0..CAL_NUM_CALS).contains(&cal) {
        return value_error(vm, "cal_info", 1, "must be a valid calendar ID");
    }
    Ok(php_cal_info(cal as usize))
}

fn cal_days_in_month(vm: &mut Vm, args: &[Value]) -> R {
    let f = "cal_days_in_month";
    let (cal, month, year) = (long(args, 0), long(args, 1), long(args, 2));
    if !(0..CAL_NUM_CALS).contains(&cal) {
        return value_error(vm, f, 1, "must be a valid calendar ID");
    }
    let int_max = i32::MAX as i64;
    if month <= 0 || month > int_max - 1 {
        return value_error(vm, f, 2, &format!("must be between 1 and {}", int_max - 1));
    }
    if year > int_max - 1 {
        return value_error(vm, f, 3, &format!("must be less than {}", int_max - 1));
    }
    let c = &CALS[cal as usize];
    let sdn_start = (c.to_jd)(year as i32, month as i32, 1);
    if sdn_start == 0 {
        return throw(vm, b"ValueError", "Invalid date");
    }
    let mut sdn_next = (c.to_jd)(year as i32, (1 + month) as i32, 1);
    if sdn_next == 0 {
        if year == -1 {
            sdn_next = (c.to_jd)(1, 1, 1);
        } else {
            sdn_next = (c.to_jd)((year + 1) as i32, 1, 1);
            if cal == CAL_FRENCH && sdn_next == 0 {
                // The French calendar ends on 0014-13-05.
                sdn_next = 2380953;
            }
        }
    }
    Ok(Value::Long(sdn_next - sdn_start))
}

fn cal_to_jd(vm: &mut Vm, args: &[Value]) -> R {
    let f = "cal_to_jd";
    let (cal, month, day, year) = (long(args, 0), long(args, 1), long(args, 2), long(args, 3));
    if !(0..CAL_NUM_CALS).contains(&cal) {
        return value_error(vm, f, 1, "must be a valid calendar ID");
    }
    let int_max = i32::MAX as i64;
    if month <= 0 || month > int_max - 1 {
        return value_error(vm, f, 2, &format!("must be between 1 and {}", int_max - 1));
    }
    if day < i32::MIN as i64 || day > int_max {
        return value_error(
            vm,
            f,
            3,
            &format!("must be between {} and {}", i32::MIN, i32::MAX),
        );
    }
    if year > int_max - 1 {
        return value_error(vm, f, 4, &format!("must be less than {}", int_max - 1));
    }
    Ok(Value::Long((CALS[cal as usize].to_jd)(
        year as i32,
        month as i32,
        day as i32,
    )))
}

fn cal_from_jd(vm: &mut Vm, args: &[Value]) -> R {
    let (jd, cal) = (long(args, 0), long(args, 1));
    if !(0..CAL_NUM_CALS).contains(&cal) {
        return value_error(vm, "cal_from_jd", 2, "must be a valid calendar ID");
    }
    let c = &CALS[cal as usize];
    let (year, month, day) = (c.from_jd)(jd);
    let mut ret = PhpArray::new();
    set(&mut ret, "date", date_str(year, month, day));
    set(&mut ret, "month", Value::Long(month as i64));
    set(&mut ret, "day", Value::Long(day as i64));
    set(&mut ret, "year", Value::Long(year as i64));
    if cal != CAL_JEWISH || year > 0 {
        let dow = day_of_week(jd);
        set(&mut ret, "dow", Value::Long(dow as i64));
        set(&mut ret, "abbrevdayname", s(DAY_NAME_SHORT[dow as usize]));
        set(&mut ret, "dayname", s(DAY_NAME_LONG[dow as usize]));
    } else {
        set(&mut ret, "dow", Value::Null);
        set(&mut ret, "abbrevdayname", s(""));
        set(&mut ret, "dayname", s(""));
    }
    if cal == CAL_JEWISH {
        let n = if year > 0 {
            idx(jewish_month_name(year), month)
        } else {
            ""
        };
        set(&mut ret, "abbrevmonth", s(n));
        set(&mut ret, "monthname", s(n));
    } else {
        set(&mut ret, "abbrevmonth", s(idx(c.month_name_short, month)));
        set(&mut ret, "monthname", s(idx(c.month_name_long, month)));
    }
    Ok(arr(ret))
}

fn jdtogregorian(_vm: &mut Vm, args: &[Value]) -> R {
    let (y, m, d) = sdn_to_gregorian(long(args, 0));
    Ok(date_str(y, m, d))
}

fn gregoriantojd(_vm: &mut Vm, args: &[Value]) -> R {
    let (m, d, y) = (long(args, 0), long(args, 1), long(args, 2));
    Ok(Value::Long(gregorian_to_sdn(y as i32, m as i32, d as i32)))
}

fn jdtojulian(_vm: &mut Vm, args: &[Value]) -> R {
    let (y, m, d) = sdn_to_julian(long(args, 0));
    Ok(date_str(y, m, d))
}

fn juliantojd(_vm: &mut Vm, args: &[Value]) -> R {
    let (m, d, y) = (long(args, 0), long(args, 1), long(args, 2));
    Ok(Value::Long(julian_to_sdn(y as i32, m as i32, d as i32)))
}

/// ISO-8859-8 אבגדהוזחטיכלמנסעפצקרשת
const ALEF_BET: &[u8; 25] =
    b"0\xE0\xE1\xE2\xE3\xE4\xE5\xE6\xE7\xE8\xE9\xEB\xEC\xEE\xF0\xF1\xF2\xF4\xF6\xF7\xF8\xF9\xFA\0\0";

/// heb_number_to_chars(): None outside 1..=9999.
fn heb_number_to_chars(mut n: i32, fl: i64) -> Option<Vec<u8>> {
    if !(1..=9999).contains(&n) {
        return None;
    }
    let mut p = Vec::new();
    let mut end_of_alafim = 0;
    if n / 1000 != 0 {
        p.push(ALEF_BET[(n / 1000) as usize]);
        if CAL_JEWISH_ADD_ALAFIM_GERESH & fl != 0 {
            p.push(b'\'');
        }
        if CAL_JEWISH_ADD_ALAFIM & fl != 0 {
            p.extend_from_slice(b" \xE0\xEC\xF4\xE9\xED ");
        }
        end_of_alafim = p.len();
        n %= 1000;
    }
    while n >= 400 {
        p.push(ALEF_BET[22]);
        n -= 400;
    }
    if n >= 100 {
        p.push(ALEF_BET[(18 + n / 100) as usize]);
        n %= 100;
    }
    if n == 15 || n == 16 {
        p.push(ALEF_BET[9]);
        p.push(ALEF_BET[(n - 9) as usize]);
    } else {
        if n >= 10 {
            p.push(ALEF_BET[(9 + n / 10) as usize]);
            n %= 10;
        }
        if n > 0 {
            p.push(ALEF_BET[n as usize]);
        }
    }
    if CAL_JEWISH_ADD_GERESHAYIM & fl != 0 {
        match p.len() - end_of_alafim {
            0 => {}
            1 => p.push(b'\''),
            _ => {
                let last = *p.last().unwrap();
                let l = p.len();
                p[l - 1] = b'"';
                p.push(last);
            }
        }
    }
    Some(p)
}

fn jdtojewish(vm: &mut Vm, args: &[Value]) -> R {
    let jd = long(args, 0);
    let heb = arg(args, 1).to_bool();
    let fl = long(args, 2);
    let (year, month, day) = sdn_to_jewish(jd);
    if !heb {
        return Ok(date_str(year, month, day));
    }
    if year <= 0 || year > 9999 {
        return throw(vm, b"ValueError", "Year out of range (0-9999)");
    }
    let names = if jewish_leap(year) {
        &JEWISH_MONTH_HEB_NAME_LEAP
    } else {
        &JEWISH_MONTH_HEB_NAME
    };
    let mut out = heb_number_to_chars(day, fl).unwrap_or_else(|| b"(null)".to_vec());
    out.push(b' ');
    out.extend_from_slice(idx(names, month));
    out.push(b' ');
    out.extend(heb_number_to_chars(year, fl).unwrap_or_else(|| b"(null)".to_vec()));
    Ok(b(&out))
}

fn jewishtojd(vm: &mut Vm, args: &[Value]) -> R {
    let (m, d, y) = (long(args, 0), long(args, 1), long(args, 2));
    if y < i32::MIN as i64 || y > i32::MAX as i64 {
        return value_error(
            vm,
            "jewishtojd",
            3,
            &format!("must be between {} and {}", i32::MIN, i32::MAX),
        );
    }
    Ok(Value::Long(jewish_to_sdn(y as i32, m as i32, d as i32)))
}

fn jdtofrench(_vm: &mut Vm, args: &[Value]) -> R {
    let (y, m, d) = sdn_to_french(long(args, 0));
    Ok(date_str(y, m, d))
}

fn frenchtojd(_vm: &mut Vm, args: &[Value]) -> R {
    let (m, d, y) = (long(args, 0), long(args, 1), long(args, 2));
    Ok(Value::Long(french_to_sdn(y as i32, m as i32, d as i32)))
}

fn jddayofweek(_vm: &mut Vm, args: &[Value]) -> R {
    let jd = long(args, 0);
    let mode = match arg(args, 1) {
        Value::Undef => CAL_DOW_DAYNO,
        v => v.to_long(),
    };
    let day = day_of_week(jd);
    Ok(match mode {
        CAL_DOW_LONG => s(DAY_NAME_LONG[day as usize]),
        CAL_DOW_SHORT => s(DAY_NAME_SHORT[day as usize]),
        _ => Value::Long(day as i64),
    })
}

fn jdmonthname(_vm: &mut Vm, args: &[Value]) -> R {
    let (jd, mode) = (long(args, 0), long(args, 1));
    let name = match mode {
        CAL_MONTH_GREGORIAN_LONG => idx(&MONTH_NAME_LONG, sdn_to_gregorian(jd).1),
        CAL_MONTH_JULIAN_SHORT => idx(&MONTH_NAME_SHORT, sdn_to_julian(jd).1),
        CAL_MONTH_JULIAN_LONG => idx(&MONTH_NAME_LONG, sdn_to_julian(jd).1),
        CAL_MONTH_JEWISH => {
            let (y, m, _) = sdn_to_jewish(jd);
            if y > 0 {
                idx(jewish_month_name(y), m)
            } else {
                ""
            }
        }
        CAL_MONTH_FRENCH => idx(&FRENCH_MONTH_NAME, sdn_to_french(jd).1),
        _ => idx(&MONTH_NAME_SHORT, sdn_to_gregorian(jd).1),
    };
    Ok(s(name))
}

/// php_localtime_r() of the C library (the TZ environment variable).
fn c_localtime(ts: i64) -> Option<libc::tm> {
    unsafe {
        let t = ts as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            None
        } else {
            Some(tm)
        }
    }
}

fn now() -> i64 {
    unsafe { libc::time(std::ptr::null_mut()) as i64 }
}

fn cal_easter(vm: &mut Vm, args: &[Value], gm: bool) -> R {
    let f = if gm { "easter_date" } else { "easter_days" };
    let method = match arg(args, 1) {
        Value::Undef => CAL_EASTER_DEFAULT,
        v => v.to_long(),
    };
    let max_year = (i64::MAX / 5) * 4;
    let year = match opt_long(args, 0) {
        Some(y) => y,
        None => match c_localtime(now()) {
            Some(tm) => 1900 + tm.tm_year as i64,
            None => 1900,
        },
    };
    if year <= 0 || year > max_year {
        return value_error(vm, f, 1, &format!("must be between 1 and {}", max_year));
    }
    if gm && year < 1970 {
        return value_error(vm, f, 1, "must be a year after 1970 (inclusive)");
    }
    if gm && year > 2000000000 {
        return value_error(vm, f, 1, "must be a year before 2.000.000.000 (inclusive)");
    }
    let golden = (year % 19) + 1;
    let mut pfm;
    let mut dom;
    if (year <= 1582 && method != CAL_EASTER_ALWAYS_GREGORIAN)
        || ((1583..=1752).contains(&year)
            && method != CAL_EASTER_ROMAN
            && method != CAL_EASTER_ALWAYS_GREGORIAN)
        || method == CAL_EASTER_ALWAYS_JULIAN
    {
        dom = (year + (year / 4) + 5) % 7;
        if dom < 0 {
            dom += 7;
        }
        pfm = (3 - (11 * golden) - 7) % 30;
        if pfm < 0 {
            pfm += 30;
        }
    } else {
        dom = (year + (year / 4) - (year / 100) + (year / 400)) % 7;
        if dom < 0 {
            dom += 7;
        }
        let solar = (year - 1600) / 100 - (year - 1600) / 400;
        let lunar = (((year - 1400) / 100) * 8) / 25;
        pfm = (3 - (11 * golden) + solar - lunar) % 30;
        if pfm < 0 {
            pfm += 30;
        }
    }
    if pfm == 29 || (pfm == 28 && golden > 11) {
        pfm -= 1;
    }
    let mut tmp = (4 - pfm - dom) % 7;
    if tmp < 0 {
        tmp += 7;
    }
    let easter = pfm + tmp + 1;
    if !gm {
        return Ok(Value::Long(easter));
    }
    let mut te: libc::tm = unsafe { std::mem::zeroed() };
    te.tm_isdst = -1;
    te.tm_year = (year - 1900) as i32;
    if easter < 11 {
        te.tm_mon = 2;
        te.tm_mday = (easter + 21) as i32;
    } else {
        te.tm_mon = 3;
        te.tm_mday = (easter - 10) as i32;
    }
    let r = unsafe { libc::mktime(&mut te) } as i64;
    Ok(Value::Long(r))
}

fn easter_date(vm: &mut Vm, args: &[Value]) -> R {
    cal_easter(vm, args, true)
}

fn easter_days(vm: &mut Vm, args: &[Value]) -> R {
    cal_easter(vm, args, false)
}

fn unixtojd(vm: &mut Vm, args: &[Value]) -> R {
    let ts = match opt_long(args, 0) {
        None => now(),
        Some(t) if t >= 0 => t,
        Some(_) => {
            return value_error(vm, "unixtojd", 1, "must be greater than or equal to 0");
        }
    };
    match c_localtime(ts) {
        Some(tm) => Ok(Value::Long(gregorian_to_sdn(
            tm.tm_year.wrapping_add(1900),
            tm.tm_mon + 1,
            tm.tm_mday,
        ))),
        None => Ok(Value::False),
    }
}

fn jdtounix(vm: &mut Vm, args: &[Value]) -> R {
    const SECS_PER_DAY: i64 = 24 * 3600;
    let uday = long(args, 0);
    if uday < 2440588 || (uday - 2440588) > (i64::MAX / SECS_PER_DAY) {
        let msg = format!(
            "jday must be between 2440588 and {}",
            i64::MAX / SECS_PER_DAY + 2440588
        );
        return throw(vm, b"ValueError", &msg);
    }
    Ok(Value::Long((uday - 2440588) * SECS_PER_DAY))
}
