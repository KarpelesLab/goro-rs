//! Procedural date functions (port of php_date.c PHP_FUNCTIONs that do not
//! operate on objects).

use crate::format::*;
use crate::state::*;
use crate::timelib::parse_date::{
    TIMEZONEMAP, abbr_search, fill_holes, parse_from_format, strtotime, timezone_id_from_abbr,
};
use crate::timelib::tm2unixtime::update_ts;
use crate::timelib::tz::{TZDB_INDEX, TZDB_VERSION, parse_tzfile, timezone_id_is_valid};
use crate::timelib::unixtime2tm::{unixtime2date, unixtime2gmt, unixtime2local};
use crate::timelib::*;
use crate::zpp::*;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

thread_local! {
    static LAST_ERRORS: RefCell<Option<ErrorContainer>> = const { RefCell::new(None) };
}

/// update_errors_warnings
pub fn update_errors_warnings(err: Option<&ErrorContainer>) {
    LAST_ERRORS.with(|l| {
        let mut l = l.borrow_mut();
        *l = None;
        if let Some(e) = err {
            if e.warning_count() > 0 || e.error_count() > 0 {
                *l = Some(e.clone());
            }
        }
    });
}

pub fn now_with_fraction() -> (i64, i64) {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    (d.as_secs() as i64, d.subsec_micros() as i64)
}

pub fn php_time() -> i64 {
    now_with_fraction().0
}

/// guess_timezone()
pub fn guess_timezone(vm: &Vm) -> String {
    if let Some(v) = vm.constants.get(b"__default_timezone".as_ref()) {
        let s = v.to_php_string().to_string_lossy();
        if !s.is_empty() {
            return s;
        }
    }
    if let Some(v) = vm.constants.get(b"date.timezone".as_ref()) {
        let s = v.to_php_string().to_string_lossy();
        if !s.is_empty() && timezone_id_is_valid(&s) {
            return s;
        }
    }
    "UTC".to_string()
}

/// get_timezone_info()
pub fn get_timezone_info(vm: &mut Vm) -> Option<Rc<TzInfo>> {
    let name = guess_timezone(vm);
    match parse_tzfile(&name) {
        Some(t) => Some(t),
        None => {
            throw(
                vm,
                b"DateError",
                "Timezone database is corrupt. Please file a bug report as this should never happen",
            );
            None
        }
    }
}

fn bytes(v: Vec<u8>) -> Value {
    Value::String(PhpString::from_vec(v))
}

/// php_format_date
pub fn php_format_date(vm: &mut Vm, format: &[u8], ts: i64, localtime: bool) -> Vec<u8> {
    let mut t = Time::new();
    if localtime {
        let tzi = get_timezone_info(vm);
        t.tz_info = tzi;
        t.zone_type = ZONETYPE_ID;
        unixtime2local(&mut t, ts);
    } else {
        unixtime2gmt(&mut t, ts);
    }
    date_format(format, &t, localtime)
}

fn php_date(vm: &mut Vm, args: &[Value], fname: &str, localtime: bool) -> R<Value> {
    check_count(vm, fname, args, 1, 2)?;
    let format = str_arg(vm, fname, args, 0, "format", false)?.unwrap_or_default();
    let ts = match long_arg(vm, fname, args, 1, "timestamp", true)? {
        Some(t) => t,
        None => php_time(),
    };
    Ok(bytes(php_format_date(vm, &format, ts, localtime)))
}

pub fn date_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    php_date(vm, args, "date", true)
}

pub fn gmdate_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    php_date(vm, args, "gmdate", false)
}

pub fn idate_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    check_count(vm, "idate", args, 1, 2)?;
    let format = str_arg(vm, "idate", args, 0, "format", false)?.unwrap_or_default();
    let ts = match long_arg(vm, "idate", args, 1, "timestamp", true)? {
        Some(t) => t,
        None => php_time(),
    };
    if format.len() != 1 {
        vm.emit_warning("idate(): idate format is one char");
        return Ok(Value::False);
    }
    let mut t = Time::new();
    let tzi = get_timezone_info(vm);
    t.tz_info = tzi;
    t.zone_type = ZONETYPE_ID;
    unixtime2local(&mut t, ts);
    let offset = match &t.tz_info {
        Some(tz) => tz.get_time_zone_info(t.sse),
        None => TimeOffset::default(),
    };
    let (isoweek, isoyear) = dow::isoweek_from_date(t.y, t.m, t.d);
    let r: i64 = match format[0] {
        b'd' | b'j' => t.d as i32 as i64,
        b'N' => dow::iso_day_of_week(t.y, t.m, t.d),
        b'w' => dow::day_of_week(t.y, t.m, t.d),
        b'z' => dow::day_of_year(t.y, t.m, t.d),
        b'W' => isoweek,
        b'm' | b'n' => t.m,
        b't' => dow::days_in_month(t.y, t.m),
        b'L' => is_leap(t.y as i32 as i64) as i64,
        b'y' => (t.y % 100) as i32 as i64,
        b'Y' => t.y as i32 as i64,
        b'o' => isoyear as i32 as i64,
        b'B' => {
            let sse = t.sse;
            let mut retval = ((sse - (sse - ((sse % 86400) + 3600))) * 10) as i32;
            if retval < 0 {
                retval += 864000;
            }
            ((retval / 864) % 1000) as i64
        }
        b'g' | b'h' => {
            if t.h % 12 != 0 {
                (t.h as i32 % 12) as i64
            } else {
                12
            }
        }
        b'H' | b'G' => t.h,
        b'i' => t.i,
        b's' => t.s,
        b'I' => offset.is_dst as i64,
        b'Z' => offset.offset as i64,
        b'U' => t.sse as i32 as i64,
        _ => -1,
    };
    // php_idate() returns -1 for an unknown token: any -1 result is one.
    if r == -1 {
        vm.emit_warning("idate(): Unrecognized date format token");
        return Ok(Value::False);
    }
    Ok(Value::Long(r))
}

pub fn strtotime_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    check_count(vm, "strtotime", args, 1, 2)?;
    let times = str_arg(vm, "strtotime", args, 0, "datetime", false)?.unwrap_or_default();
    let preset = long_arg(vm, "strtotime", args, 1, "baseTimestamp", true)?;
    if times.is_empty() {
        return Ok(Value::False);
    }
    let tzi = match get_timezone_info(vm) {
        Some(t) => t,
        None => return Ok(Value::Null),
    };
    let mut now = Time::new();
    now.tz_info = Some(tzi.clone());
    now.zone_type = ZONETYPE_ID;
    unixtime2local(&mut now, preset.unwrap_or_else(php_time));
    let (mut t, err) = strtotime(&times);
    if err.error_count() > 0 {
        return Ok(Value::False);
    }
    fill_holes(&mut t, &now, NO_CLONE);
    update_ts(&mut t, Some(&tzi));
    Ok(Value::Long(t.sse))
}

fn php_mktime(vm: &mut Vm, args: &[Value], fname: &str, gmt: bool) -> R<Value> {
    check_count(vm, fname, args, 1, 6)?;
    let hou = long_arg(vm, fname, args, 0, "hour", false)?.unwrap_or(0);
    let min = long_arg(vm, fname, args, 1, "minute", true)?;
    let sec = long_arg(vm, fname, args, 2, "second", true)?;
    let mon = long_arg(vm, fname, args, 3, "month", true)?;
    let day = long_arg(vm, fname, args, 4, "day", true)?;
    let yea = long_arg(vm, fname, args, 5, "year", true)?;
    let mut now = Time::new();
    let mut tzi = None;
    if gmt {
        unixtime2gmt(&mut now, php_time());
    } else {
        tzi = get_timezone_info(vm);
        now.tz_info = tzi.clone();
        now.zone_type = ZONETYPE_ID;
        unixtime2local(&mut now, php_time());
    }
    now.h = hou;
    if let Some(v) = min {
        now.i = v;
    }
    if let Some(v) = sec {
        now.s = v;
    }
    if let Some(v) = mon {
        now.m = v;
    }
    if let Some(v) = day {
        now.d = v;
    }
    if let Some(mut y) = yea {
        if (0..70).contains(&y) {
            y += 2000;
        } else if (70..=100).contains(&y) {
            y += 1900;
        }
        now.y = y;
    }
    update_ts(&mut now, if gmt { None } else { tzi.as_ref() });
    Ok(Value::Long(now.sse))
}

pub fn mktime_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    php_mktime(vm, args, "mktime", false)
}

pub fn gmmktime_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    php_mktime(vm, args, "gmmktime", true)
}

pub fn checkdate_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    check_count(vm, "checkdate", args, 3, 3)?;
    let m = long_arg(vm, "checkdate", args, 0, "month", false)?.unwrap_or(0);
    let d = long_arg(vm, "checkdate", args, 1, "day", false)?.unwrap_or(0);
    let y = long_arg(vm, "checkdate", args, 2, "year", false)?.unwrap_or(0);
    if !(1..=32767).contains(&y) || !valid_date(y, m, d) {
        return Ok(Value::False);
    }
    Ok(Value::True)
}

fn php_strftime(vm: &mut Vm, args: &[Value], fname: &str, gmt: bool) -> R<Value> {
    vm.emit_deprecated(&format!(
        "Function {}() is deprecated since 8.1, use IntlDateFormatter::format() instead",
        fname
    ));
    check_count(vm, fname, args, 1, 2)?;
    let format = str_arg(vm, fname, args, 0, "format", false)?.unwrap_or_default();
    let ts = long_arg(vm, fname, args, 1, "timestamp", true)?.unwrap_or_else(php_time);
    if format.is_empty() {
        return Ok(Value::False);
    }
    let mut t = Time::new();
    let (isdst, gmtoff, zone);
    if gmt {
        unixtime2gmt(&mut t, ts);
        isdst = 0;
        gmtoff = 0;
        zone = "GMT".to_string();
    } else {
        let tzi = get_timezone_info(vm);
        t.tz_info = tzi.clone();
        t.zone_type = ZONETYPE_ID;
        unixtime2local(&mut t, ts);
        let off = tzi.map(|z| z.get_time_zone_info(ts)).unwrap_or_default();
        isdst = off.is_dst as i64;
        gmtoff = off.offset as i64;
        zone = off.abbr;
    }
    let tm = Tm {
        sec: t.s,
        min: t.i,
        hour: t.h,
        mday: t.d,
        mon: t.m - 1,
        year: t.y - 1900,
        wday: dow::day_of_week(t.y, t.m, t.d),
        yday: dow::day_of_year(t.y, t.m, t.d),
        isdst,
        gmtoff,
        zone,
    };
    // The buffer starts at 256 bytes and is doubled while strftime() returns
    // 0, up to 4096 bytes; an empty result is a failure too.
    match strftime(&format, &tm, 4096) {
        Some(out) if !out.is_empty() => Ok(bytes(out)),
        _ => Ok(Value::False),
    }
}

pub fn strftime_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    php_strftime(vm, args, "strftime", false)
}

pub fn gmstrftime_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    php_strftime(vm, args, "gmstrftime", true)
}

pub fn time_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    check_count(vm, "time", args, 0, 0)?;
    Ok(Value::Long(php_time()))
}

pub fn localtime_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    check_count(vm, "localtime", args, 0, 2)?;
    let ts = long_arg(vm, "localtime", args, 0, "timestamp", true)?.unwrap_or_else(php_time);
    let assoc = bool_arg(vm, "localtime", args, 1, "associative")?.unwrap_or(false);
    let tzi = get_timezone_info(vm);
    let mut t = Time::new();
    t.tz_info = tzi;
    t.zone_type = ZONETYPE_ID;
    unixtime2local(&mut t, ts);
    let vals = [
        ("tm_sec", t.s),
        ("tm_min", t.i),
        ("tm_hour", t.h),
        ("tm_mday", t.d),
        ("tm_mon", t.m - 1),
        ("tm_year", t.y - 1900),
        ("tm_wday", dow::day_of_week(t.y, t.m, t.d)),
        ("tm_yday", dow::day_of_year(t.y, t.m, t.d)),
        ("tm_isdst", t.dst as i64),
    ];
    let mut a = new_arr();
    for (k, v) in vals {
        if assoc {
            arr_set(&mut a, k, Value::Long(v));
        } else {
            a.push(Value::Long(v));
        }
    }
    Ok(arr_val(a))
}

pub fn getdate_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    check_count(vm, "getdate", args, 0, 1)?;
    let ts = long_arg(vm, "getdate", args, 0, "timestamp", true)?.unwrap_or_else(php_time);
    let tzi = get_timezone_info(vm);
    let mut t = Time::new();
    t.tz_info = tzi;
    t.zone_type = ZONETYPE_ID;
    unixtime2local(&mut t, ts);
    let mut a = new_arr();
    arr_set(&mut a, "seconds", Value::Long(t.s));
    arr_set(&mut a, "minutes", Value::Long(t.i));
    arr_set(&mut a, "hours", Value::Long(t.h));
    arr_set(&mut a, "mday", Value::Long(t.d));
    arr_set(&mut a, "wday", Value::Long(dow::day_of_week(t.y, t.m, t.d)));
    arr_set(&mut a, "mon", Value::Long(t.m));
    arr_set(&mut a, "year", Value::Long(t.y));
    arr_set(&mut a, "yday", Value::Long(dow::day_of_year(t.y, t.m, t.d)));
    arr_set(
        &mut a,
        "weekday",
        s(full_day_name(t.y, t.m, t.d).as_bytes()),
    );
    arr_set(
        &mut a,
        "month",
        s(MON_FULL[(t.m - 1).clamp(0, 11) as usize].as_bytes()),
    );
    a.set(goro_core::array::ArrayKey::Int(0), Value::Long(ts));
    Ok(arr_val(a))
}

pub fn microtime_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    check_count(vm, "microtime", args, 0, 1)?;
    let as_float = bool_arg(vm, "microtime", args, 0, "as_float")?.unwrap_or(false);
    let (sec, usec) = now_with_fraction();
    if as_float {
        Ok(Value::Double(sec as f64 + usec as f64 / 1e6))
    } else {
        Ok(ss(format!("{:.8} {}", usec as f64 / 1e6, sec)))
    }
}

pub fn gettimeofday_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    check_count(vm, "gettimeofday", args, 0, 1)?;
    let as_float = bool_arg(vm, "gettimeofday", args, 0, "as_float")?.unwrap_or(false);
    let (sec, usec) = now_with_fraction();
    if as_float {
        return Ok(Value::Double(sec as f64 + usec as f64 / 1e6));
    }
    let tzi = get_timezone_info(vm);
    let off = tzi.map(|z| z.get_time_zone_info(sec)).unwrap_or_default();
    let mut a = new_arr();
    arr_set(&mut a, "sec", Value::Long(sec));
    arr_set(&mut a, "usec", Value::Long(usec));
    arr_set(
        &mut a,
        "minuteswest",
        Value::Long(-(off.offset as i64) / 60),
    );
    arr_set(&mut a, "dsttime", Value::Long(off.is_dst as i64));
    Ok(arr_val(a))
}

/// zval_from_error_container
pub fn errors_to_array(a: &mut goro_core::array::PhpArray, e: &ErrorContainer) {
    arr_set(a, "warning_count", Value::Long(e.warning_count() as i64));
    let mut w = new_arr();
    for m in &e.warning_messages {
        w.set(
            goro_core::array::ArrayKey::Int(m.position as i64),
            ss(m.message.clone()),
        );
    }
    arr_set(a, "warnings", arr_val(w));
    arr_set(a, "error_count", Value::Long(e.error_count() as i64));
    let mut er = new_arr();
    for m in &e.error_messages {
        er.set(
            goro_core::array::ArrayKey::Int(m.position as i64),
            ss(m.message.clone()),
        );
    }
    arr_set(a, "errors", arr_val(er));
}

pub fn date_get_last_errors_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    check_count(vm, "date_get_last_errors", args, 0, 0)?;
    Ok(last_errors_value())
}

pub fn last_errors_value() -> Value {
    LAST_ERRORS.with(|l| match &*l.borrow() {
        Some(e) => {
            let mut a = new_arr();
            errors_to_array(&mut a, e);
            arr_val(a)
        }
        None => Value::False,
    })
}

fn return_parsed_time(t: &Time, err: &ErrorContainer) -> Value {
    let mut a = new_arr();
    let el = |a: &mut goro_core::array::PhpArray, k: &str, v: i64| {
        if v == UNSET {
            arr_set(a, k, Value::False);
        } else {
            arr_set(a, k, Value::Long(v));
        }
    };
    el(&mut a, "year", t.y);
    el(&mut a, "month", t.m);
    el(&mut a, "day", t.d);
    el(&mut a, "hour", t.h);
    el(&mut a, "minute", t.i);
    el(&mut a, "second", t.s);
    if t.us == UNSET {
        arr_set(&mut a, "fraction", Value::False);
    } else {
        arr_set(&mut a, "fraction", Value::Double(t.us as f64 / 1000000.0));
    }
    errors_to_array(&mut a, err);
    arr_set(
        &mut a,
        "is_localtime",
        if t.is_localtime != 0 {
            Value::True
        } else {
            Value::False
        },
    );
    if t.is_localtime != 0 {
        el(&mut a, "zone_type", t.zone_type as i64);
        match t.zone_type {
            ZONETYPE_OFFSET => {
                el(&mut a, "zone", t.z);
                arr_set(
                    &mut a,
                    "is_dst",
                    if t.dst != 0 {
                        Value::True
                    } else {
                        Value::False
                    },
                );
            }
            ZONETYPE_ID => {
                if let Some(ab) = &t.tz_abbr {
                    arr_set(&mut a, "tz_abbr", ss(ab.clone()));
                }
                if let Some(tz) = &t.tz_info {
                    arr_set(&mut a, "tz_id", ss(tz.name.clone()));
                }
            }
            ZONETYPE_ABBR => {
                el(&mut a, "zone", t.z);
                arr_set(
                    &mut a,
                    "is_dst",
                    if t.dst != 0 {
                        Value::True
                    } else {
                        Value::False
                    },
                );
                arr_set(&mut a, "tz_abbr", ss(t.tz_abbr.clone().unwrap_or_default()));
            }
            _ => {}
        }
    }
    if t.have_relative != 0 {
        let r = &t.relative;
        let mut e = new_arr();
        arr_set(&mut e, "year", Value::Long(r.y));
        arr_set(&mut e, "month", Value::Long(r.m));
        arr_set(&mut e, "day", Value::Long(r.d));
        arr_set(&mut e, "hour", Value::Long(r.h));
        arr_set(&mut e, "minute", Value::Long(r.i));
        arr_set(&mut e, "second", Value::Long(r.s));
        if r.have_weekday_relative != 0 {
            arr_set(&mut e, "weekday", Value::Long(r.weekday as i64));
        }
        if r.have_special_relative != 0 && r.special_type == SPECIAL_WEEKDAY {
            arr_set(&mut e, "weekdays", Value::Long(r.special_amount));
        }
        if r.first_last_day_of != 0 {
            arr_set(
                &mut e,
                if r.first_last_day_of == SPECIAL_FIRST_DAY_OF_MONTH {
                    "first_day_of_month"
                } else {
                    "last_day_of_month"
                },
                Value::True,
            );
        }
        arr_set(&mut a, "relative", arr_val(e));
    }
    arr_val(a)
}

pub fn date_parse_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    check_count(vm, "date_parse", args, 1, 1)?;
    let date = str_arg(vm, "date_parse", args, 0, "datetime", false)?.unwrap_or_default();
    let (t, err) = strtotime(&date);
    Ok(return_parsed_time(&t, &err))
}

pub fn date_parse_from_format_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    check_count(vm, "date_parse_from_format", args, 2, 2)?;
    let format =
        str_arg(vm, "date_parse_from_format", args, 0, "format", false)?.unwrap_or_default();
    let date = path_arg(vm, "date_parse_from_format", args, 1, "datetime")?.unwrap_or_default();
    let (t, err) = parse_from_format(&format, &date);
    Ok(return_parsed_time(&t, &err))
}

pub fn date_default_timezone_set_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    check_count(vm, "date_default_timezone_set", args, 1, 1)?;
    let zone = str_arg(
        vm,
        "date_default_timezone_set",
        args,
        0,
        "timezoneId",
        false,
    )?
    .unwrap_or_default();
    // The zone is used as a C string: everything after a NUL byte is ignored.
    let zone = match zone.iter().position(|&b| b == 0) {
        Some(n) => zone[..n].to_vec(),
        None => zone.to_vec(),
    };
    let zs = goro_core::string::msg_str(&zone).into_owned();
    if !timezone_id_is_valid(&zs) {
        vm.emit_notice_at(
            &format!(
                "date_default_timezone_set(): Timezone ID '{}' is invalid",
                zs
            ),
            vm.current_line,
        );
        return Ok(Value::False);
    }
    vm.constants.insert(b"__default_timezone".to_vec(), ss(zs));
    Ok(Value::True)
}

/// OnUpdate_date_timezone
pub fn validate_date_timezone(vm: &mut Vm, value: &[u8], startup: bool) -> bool {
    let s = String::from_utf8_lossy(value).into_owned();
    if !value.contains(&0) && timezone_id_is_valid(&s) {
        return true;
    }
    if startup {
        vm.write_output(format!("\nWarning: PHP Startup: Invalid date.timezone value '{}', using 'UTC' instead in Unknown on line 0\n", s).as_bytes());
    } else {
        let current = vm
            .constants
            .get(b"date.timezone".as_ref())
            .map(|v| v.to_php_string().to_string_lossy())
            .unwrap_or_default();
        let using = if current.is_empty() {
            "UTC".to_string()
        } else {
            current
        };
        vm.emit_warning(&format!(
            "ini_set(): Invalid date.timezone value '{}', using '{}' instead",
            s, using
        ));
    }
    false
}

pub fn date_default_timezone_get_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    check_count(vm, "date_default_timezone_get", args, 0, 0)?;
    match get_timezone_info(vm) {
        Some(t) => Ok(ss(t.name.clone())),
        None => Err(VmError {
            message: "tz".into(),
            line: vm.current_line,
        }),
    }
}

pub fn timezone_name_from_abbr_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    check_count(vm, "timezone_name_from_abbr", args, 1, 3)?;
    let abbr = str_arg(vm, "timezone_name_from_abbr", args, 0, "abbr", false)?.unwrap_or_default();
    let off = long_arg(vm, "timezone_name_from_abbr", args, 1, "utcOffset", false)?.unwrap_or(-1);
    let isdst = long_arg(vm, "timezone_name_from_abbr", args, 2, "isDST", false)?.unwrap_or(-1);
    let z = abbr.iter().position(|&c| c == 0).unwrap_or(abbr.len());
    match timezone_id_from_abbr(&abbr[..z], off, isdst as i32) {
        Some(id) => Ok(s(id.as_bytes())),
        None => Ok(Value::False),
    }
}

fn check_id_allowed(id: &str, what: i64) -> bool {
    let groups: [(i64, &str); 11] = [
        (0x0001, "Africa/"),
        (0x0002, "America/"),
        (0x0004, "Antarctica/"),
        (0x0008, "Arctic/"),
        (0x0010, "Asia/"),
        (0x0020, "Atlantic/"),
        (0x0040, "Australia/"),
        (0x0080, "Europe/"),
        (0x0100, "Indian/"),
        (0x0200, "Pacific/"),
        (0x0400, "UTC"),
    ];
    groups.iter().any(|(bit, pfx)| {
        what & bit != 0 && id.len() >= pfx.len() && id[..pfx.len()].eq_ignore_ascii_case(pfx)
    })
}

static TZDB_RAW: &[u8] = include_bytes!("data/tzdb.bin");

pub fn identifiers_list(vm: &mut Vm, fname: &str, args: &[Value]) -> R<Value> {
    check_count(vm, fname, args, 0, 2)?;
    let what = long_arg(vm, fname, args, 0, "timezoneGroup", false)?.unwrap_or(0x07FF);
    let option = str_arg(vm, fname, args, 1, "countryCode", true)?;
    if what == 0x1000 && option.as_ref().map(|o| o.len()).unwrap_or(0) != 2 {
        return Err(throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #2 ($countryCode) must be a two-letter ISO 3166-1 compatible country code when argument #1 ($timezoneGroup) is DateTimeZone::PER_COUNTRY",
                fname
            ),
        ));
    }
    let mut a = new_arr();
    for (id, pos) in TZDB_INDEX {
        let pos = *pos as usize;
        if what == 0x1000 {
            let o = option.as_ref().unwrap();
            if TZDB_RAW[pos + 5] == o[0] && TZDB_RAW[pos + 6] == o[1] {
                a.push(s(id.as_bytes()));
            }
        } else if what == 0x0FFF || (check_id_allowed(id, what) && TZDB_RAW[pos + 4] == 1) {
            a.push(s(id.as_bytes()));
        }
    }
    Ok(arr_val(a))
}

pub fn timezone_identifiers_list_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    identifiers_list(vm, "timezone_identifiers_list", args)
}

pub fn timezone_version_get_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    check_count(vm, "timezone_version_get", args, 0, 0)?;
    Ok(s(TZDB_VERSION.as_bytes()))
}

pub fn abbreviations_list(vm: &mut Vm, fname: &str, args: &[Value]) -> R<Value> {
    check_count(vm, fname, args, 0, 0)?;
    let mut out = new_arr();
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, goro_core::array::PhpArray> =
        std::collections::HashMap::new();
    for (name, dst, off, tzid) in TIMEZONEMAP {
        let mut el = new_arr();
        arr_set(
            &mut el,
            "dst",
            if *dst != 0 { Value::True } else { Value::False },
        );
        arr_set(&mut el, "offset", Value::Long(*off as i64));
        arr_set(
            &mut el,
            "timezone_id",
            match tzid {
                Some(t) => s(t.as_bytes()),
                None => Value::Null,
            },
        );
        let key = name.to_string();
        if !groups.contains_key(&key) {
            order.push(key.clone());
            groups.insert(key.clone(), new_arr());
        }
        groups.get_mut(&key).unwrap().push(arr_val(el));
    }
    for k in order {
        let g = groups.remove(&k).unwrap();
        arr_set(&mut out, &k, arr_val(g));
    }
    Ok(arr_val(out))
}

pub fn timezone_abbreviations_list_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    abbreviations_list(vm, "timezone_abbreviations_list", args)
}

fn ini_float(vm: &Vm, name: &[u8], default: f64) -> f64 {
    match vm.constants.get(name) {
        Some(v) => v.to_double(),
        None => default,
    }
}

fn sunrise_sunset(vm: &mut Vm, args: &[Value], fname: &str, calc_sunset: bool) -> R<Value> {
    vm.emit_deprecated(&format!(
        "Function {}() is deprecated since 8.1, use date_sun_info() instead",
        fname
    ));
    check_count(vm, fname, args, 1, 6)?;
    let time = long_arg(vm, fname, args, 0, "timestamp", false)?.unwrap_or(0);
    let retformat = long_arg(vm, fname, args, 1, "returnFormat", false)?.unwrap_or(1);
    let latitude = double_arg(vm, fname, args, 2, "latitude", true)?
        .unwrap_or_else(|| ini_float(vm, b"date.default_latitude", 31.7667));
    let longitude = double_arg(vm, fname, args, 3, "longitude", true)?
        .unwrap_or_else(|| ini_float(vm, b"date.default_longitude", 35.2333));
    let zenith = match double_arg(vm, fname, args, 4, "zenith", true)? {
        Some(z) => z,
        None => {
            if calc_sunset {
                ini_float(vm, b"date.sunset_zenith", 90.833333)
            } else {
                ini_float(vm, b"date.sunrise_zenith", 90.833333)
            }
        }
    };
    let gmt_offset = double_arg(vm, fname, args, 5, "utcOffset", true)?;
    if !(0..=2).contains(&retformat) {
        return Err(throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #2 ($returnFormat) must be one of SUNFUNCS_RET_TIMESTAMP, SUNFUNCS_RET_STRING, or SUNFUNCS_RET_DOUBLE",
                fname
            ),
        ));
    }
    let altitude = 90.0 - zenith;
    if !latitude.is_finite() || !longitude.is_finite() {
        return Ok(Value::False);
    }
    let tzi = get_timezone_info(vm);
    let mut t = Time::new();
    t.tz_info = tzi;
    t.zone_type = ZONETYPE_ID;
    let gmt_offset = match gmt_offset {
        Some(g) => g,
        None => get_current_offset(&t) as f64 / 3600.0,
    };
    unixtime2local(&mut t, time);
    let rs = astro::rise_set_altitude(&mut t, longitude, latitude, altitude, true);
    if rs.rc != 0 {
        return Ok(Value::False);
    }
    if retformat == 0 {
        return Ok(Value::Long(if calc_sunset {
            rs.ts_set
        } else {
            rs.ts_rise
        }));
    }
    let mut n = (if calc_sunset { rs.h_set } else { rs.h_rise }) + gmt_offset;
    if !(0.0..=24.0).contains(&n) {
        n -= (n / 24.0).floor() * 24.0;
    }
    if !(0.0..=24.0).contains(&n) {
        return Ok(Value::False);
    }
    if retformat == 1 {
        Ok(ss(format!(
            "{:02}:{:02}",
            n as i32,
            (60.0 * (n - (n as i32) as f64)) as i32
        )))
    } else {
        Ok(Value::Double(n))
    }
}

pub fn date_sunrise_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    sunrise_sunset(vm, args, "date_sunrise", false)
}

pub fn date_sunset_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    sunrise_sunset(vm, args, "date_sunset", true)
}

pub fn date_sun_info_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    check_count(vm, "date_sun_info", args, 3, 3)?;
    let time = long_arg(vm, "date_sun_info", args, 0, "timestamp", false)?.unwrap_or(0);
    let latitude = double_arg(vm, "date_sun_info", args, 1, "latitude", false)?.unwrap_or(0.0);
    let longitude = double_arg(vm, "date_sun_info", args, 2, "longitude", false)?.unwrap_or(0.0);
    if !latitude.is_finite() {
        return Err(throw(
            vm,
            b"ValueError",
            "date_sun_info(): Argument #2 ($latitude) must be finite",
        ));
    }
    if !longitude.is_finite() {
        return Err(throw(
            vm,
            b"ValueError",
            "date_sun_info(): Argument #3 ($longitude) must be finite",
        ));
    }
    let tzi = get_timezone_info(vm);
    let mut t = Time::new();
    t.tz_info = tzi;
    t.zone_type = ZONETYPE_ID;
    unixtime2local(&mut t, time);
    let mut a = new_arr();
    let pairs: [(f64, bool, &str, &str); 4] = [
        (-35.0 / 60.0, true, "sunrise", "sunset"),
        (-6.0, false, "civil_twilight_begin", "civil_twilight_end"),
        (
            -12.0,
            false,
            "nautical_twilight_begin",
            "nautical_twilight_end",
        ),
        (
            -18.0,
            false,
            "astronomical_twilight_begin",
            "astronomical_twilight_end",
        ),
    ];
    for (idx, (alt, upper, k1, k2)) in pairs.iter().enumerate() {
        let rs = astro::rise_set_altitude(&mut t, longitude, latitude, *alt, *upper);
        match rs.rc {
            -1 => {
                arr_set(&mut a, k1, Value::False);
                arr_set(&mut a, k2, Value::False);
            }
            1 => {
                arr_set(&mut a, k1, Value::True);
                arr_set(&mut a, k2, Value::True);
            }
            _ => {
                arr_set(&mut a, k1, Value::Long(rs.ts_rise));
                arr_set(&mut a, k2, Value::Long(rs.ts_set));
            }
        }
        if idx == 0 {
            arr_set(&mut a, "transit", Value::Long(rs.ts_transit));
        }
    }
    Ok(arr_val(a))
}

// Keep helpers referenced
#[allow(dead_code)]
fn _unused() {
    let _ = abbr_search;
    let _ = unixtime2date;
}
