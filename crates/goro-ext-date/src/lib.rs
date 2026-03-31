use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

// Public API for use by VM
pub fn ymd_to_days_pub(year: i64, month: u32, day: u32) -> i64 {
    ymd_to_days(year, month, day)
}

pub fn days_to_ymd_pub(days: i64) -> (i64, u32, u32) {
    days_to_ymd(days)
}

/// Create a DateInterval object from the difference between two timestamps
pub fn create_date_interval(vm: &mut Vm, ts1: i64, ts2: i64, absolute: bool) -> Value {
    create_date_interval_from_diff(vm, ts1, ts2, absolute)
}

/// Parse an ISO 8601 duration string like "P1Y2M3DT4H5M6S"
pub fn parse_iso8601_duration(spec: &str) -> (i64, i64, i64, i64, i64, i64) {
    parse_iso8601_duration_impl(spec)
}

fn parse_iso8601_duration_impl(spec: &str) -> (i64, i64, i64, i64, i64, i64) {
    let mut y = 0i64;
    let mut m = 0i64;
    let mut d = 0i64;
    let mut h = 0i64;
    let mut i = 0i64;
    let mut s = 0i64;

    let s_bytes = spec.as_bytes();
    let mut idx = 0;
    let mut in_time = false;

    // Skip leading P
    if idx < s_bytes.len() && s_bytes[idx] == b'P' {
        idx += 1;
    }

    while idx < s_bytes.len() {
        if s_bytes[idx] == b'T' {
            in_time = true;
            idx += 1;
            continue;
        }

        // Parse number
        let start = idx;
        while idx < s_bytes.len() && s_bytes[idx].is_ascii_digit() {
            idx += 1;
        }
        if idx == start || idx >= s_bytes.len() {
            break;
        }
        let num: i64 = std::str::from_utf8(&s_bytes[start..idx])
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        match s_bytes[idx] {
            b'Y' => y = num,
            b'M' => {
                if in_time {
                    i = num;
                } else {
                    m = num;
                }
            }
            b'D' => d = num,
            b'H' => h = num,
            b'S' => s = num,
            b'W' => d = num * 7,
            _ => {}
        }
        idx += 1;
    }

    (y, m, d, h, i, s)
}

/// Register all date/time extension functions
pub fn register(vm: &mut Vm) {
    vm.register_function(b"date_default_timezone_set", date_default_timezone_set);
    vm.register_function(b"date_default_timezone_get", date_default_timezone_get);
    vm.register_function(b"time", time_fn);
    vm.register_function(b"microtime", microtime);
    vm.register_function(b"date", date_fn);
    vm.register_function(b"gmdate", gmdate_fn);
    vm.register_function(b"mktime", mktime);
    vm.register_function(b"gmmktime", gmmktime_fn);
    vm.register_function(b"strftime", strftime_fn);
    vm.register_function(b"gmstrftime", gmstrftime_fn);
    vm.register_function(b"strtotime", strtotime);
    vm.register_function(b"date_create", date_create_fn);
    vm.register_function(b"getdate", getdate_fn);
    vm.register_function(b"localtime", localtime_fn);
    vm.register_function(b"checkdate", checkdate_fn);
    vm.register_function(b"idate", idate_fn);
    vm.register_function(b"date_format", date_format_fn);
    vm.register_function(b"date_create_immutable", date_create_immutable_fn);
    vm.register_function(b"date_parse", date_parse_fn);
    vm.register_function(b"date_parse_from_format", date_parse_from_format_fn);
    vm.register_function(b"date_modify", date_modify_fn);
    vm.register_function(b"date_timestamp_get", date_timestamp_get_fn);
    vm.register_function(b"date_timestamp_set", date_timestamp_set_fn);
    vm.register_function(b"date_diff", date_diff_fn);
    vm.register_function(b"date_create_from_format", date_create_from_format_fn);
    vm.register_function(b"date_date_set", date_date_set_fn);
    vm.register_function(b"date_time_set", date_time_set_fn);
    vm.register_function(b"date_interval_create_from_date_string", date_interval_create_from_date_string_fn);
    vm.register_function(b"timezone_open", timezone_open_fn);
    vm.register_function(b"gettimeofday", gettimeofday_fn);
    vm.register_function(b"date_timezone_get", date_timezone_get_fn);
    vm.register_function(b"date_timezone_set", date_timezone_set_fn);
    vm.register_function(b"date_isodate_set", date_isodate_set_fn);
    vm.register_function(b"timezone_abbreviations_list", timezone_abbreviations_list_fn);
    vm.register_function(b"timezone_name_from_abbr", timezone_name_from_abbr_fn);
    vm.register_function(b"timezone_offset_get", timezone_offset_get_fn);
    vm.register_function(b"timezone_identifiers_list", timezone_identifiers_list_fn);
    vm.register_function(b"timezone_name_get", timezone_name_get_fn);
    vm.register_function(b"timezone_version_get", timezone_version_get_fn);
    vm.register_function(b"date_offset_get", date_offset_get_fn);
    vm.register_function(b"date_sun_info", date_sun_info_fn);
    vm.register_function(b"date_sunrise", date_sunrise_fn);
    vm.register_function(b"date_sunset", date_sunset_fn);
}

fn date_default_timezone_set(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let tz = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    vm.constants.insert(b"__default_timezone".to_vec(), Value::String(PhpString::from_string(tz)));
    Ok(Value::True)
}

fn date_default_timezone_get(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let tz = vm.constants.get(b"__default_timezone".as_ref())
        .map(|v| v.to_php_string())
        .unwrap_or_else(|| PhpString::from_bytes(b"UTC"));
    Ok(Value::String(tz))
}

fn time_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    use std::time::SystemTime;
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Ok(Value::Long(secs as i64))
}

fn microtime(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let as_float = args.first().map(|v| v.is_truthy()).unwrap_or(false);
    if as_float {
        Ok(Value::Double(dur.as_secs_f64()))
    } else {
        Ok(Value::String(PhpString::from_string(format!(
            "{:.8} {}",
            dur.subsec_nanos() as f64 / 1e9,
            dur.as_secs()
        ))))
    }
}

fn date_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let format = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();
    let timestamp = args.get(1).map(|v| v.to_long());

    // Get current time or use provided timestamp
    let secs = timestamp.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });

    // Apply timezone offset
    let tz_name = get_default_tz(_vm);
    let (offset_secs, tz_abbrev) = timezone_offset_and_abbrev(&tz_name, secs);
    let local_secs = secs + offset_secs;

    let result = format_timestamp_with_tz(&format, local_secs, &tz_abbrev, offset_secs);
    Ok(Value::String(PhpString::from_string(result)))
}


/// Convert days since epoch (1970-01-01) to (year, month, day)
fn days_to_ymd(days: i64) -> (i64, u32, u32) {
    // Civil date from day count algorithm
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as u32, d as u32)
}
/// Convert (year, month, day) to days since epoch (1970-01-01)
/// Inverse of days_to_ymd
fn ymd_to_days(year: i64, month: u32, day: u32) -> i64 {
    // Civil date to day count algorithm (inverse of days_to_ymd)
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let m = month;
    let doy = if m > 2 {
        (153 * (m as u64 - 3) + 2) / 5 + day as u64 - 1
    } else {
        (153 * (m as u64 + 9) + 2) / 5 + day as u64 - 1
    };
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

fn mktime(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // mktime(hour, minute, second, month, day, year)
    // Get current time as defaults
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let now_days = now_secs / 86400;
    let now_time = ((now_secs % 86400) + 86400) % 86400;
    let (now_year, now_month, now_day) = days_to_ymd(now_days);
    let now_hours = now_time / 3600;
    let now_minutes = (now_time % 3600) / 60;
    let now_seconds = now_time % 60;

    let hour = args.first().map(|v| v.to_long()).unwrap_or(now_hours);
    let minute = args.get(1).map(|v| v.to_long()).unwrap_or(now_minutes);
    let second = args.get(2).map(|v| v.to_long()).unwrap_or(now_seconds);
    let month = args.get(3).map(|v| v.to_long()).unwrap_or(now_month as i64);
    let day = args.get(4).map(|v| v.to_long()).unwrap_or(now_day as i64);
    let year = args.get(5).map(|v| v.to_long()).unwrap_or(now_year);

    // Handle year values 0-69 => 2000-2069, 70-100 => 1970-2000
    let year = if (0..70).contains(&year) {
        year + 2000
    } else if (70..=100).contains(&year) {
        year + 1900
    } else {
        year
    };

    let days = ymd_to_days(year, month as u32, day as u32);
    let timestamp = days * 86400 + hour * 3600 + minute * 60 + second;
    Ok(Value::Long(timestamp))
}
fn strtotime(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let datetime_str = args
        .first()
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let base_time = args.get(1).map(|v| v.to_long());

    let now_secs = base_time.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });

    match parse_datetime_string(&datetime_str, now_secs) {
        Some(ts) => Ok(Value::Long(ts)),
        None => Ok(Value::False),
    }
}

fn gmdate_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let format = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();
    let timestamp = args.get(1).map(|v| v.to_long());

    let secs = timestamp.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });

    // Always use UTC for gmdate
    let result = format_timestamp_with_tz(&format, secs, "GMT", 0);
    Ok(Value::String(PhpString::from_string(result)))
}

/// gmmktime - UTC version of mktime
fn gmmktime_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // For now, same as mktime since we don't handle timezones
    mktime(_vm, args)
}

/// strftime - format a timestamp using strftime-style format codes
fn strftime_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // PHP 8.1: strftime() is deprecated
    _vm.emit_deprecated("Function strftime() is deprecated since 8.1, use IntlDateFormatter::format() instead");
    let format = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();
    let timestamp = args.get(1).map(|v| v.to_long());

    let secs = timestamp.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });

    let days_since_epoch = secs / 86400;
    let time_of_day = ((secs % 86400) + 86400) % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    let (year, month, day) = days_to_ymd(days_since_epoch);

    let dow = (((days_since_epoch % 7) + 4) % 7 + 7) % 7; // 0=Sunday

    let day_names_full = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    let day_names_short = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let month_names_full = [
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
    let month_names_short = [
        "", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let mut result = String::new();
    let fmt_bytes = format.as_bytes();
    let mut i = 0;
    while i < fmt_bytes.len() {
        if fmt_bytes[i] == b'%' && i + 1 < fmt_bytes.len() {
            i += 1;
            match fmt_bytes[i] {
                b'Y' => result.push_str(&format!("{:04}", year)),
                b'm' => result.push_str(&format!("{:02}", month)),
                b'd' => result.push_str(&format!("{:02}", day)),
                b'H' => result.push_str(&format!("{:02}", hours)),
                b'M' => result.push_str(&format!("{:02}", minutes)),
                b'S' => result.push_str(&format!("{:02}", seconds)),
                b'A' => {
                    result.push_str(day_names_full[dow as usize % 7]);
                }
                b'a' => {
                    result.push_str(day_names_short[dow as usize % 7]);
                }
                b'B' => {
                    result.push_str(month_names_full[month as usize]);
                }
                b'b' => {
                    result.push_str(month_names_short[month as usize]);
                }
                b'Z' => {
                    result.push_str("UTC");
                }
                b'%' => {
                    result.push('%');
                }
                other => {
                    result.push('%');
                    result.push(other as char);
                }
            }
        } else {
            result.push(fmt_bytes[i] as char);
        }
        i += 1;
    }

    Ok(Value::String(PhpString::from_string(result)))
}

/// gmstrftime - format a timestamp using strftime-style format codes (UTC)
fn gmstrftime_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // PHP 8.1: gmstrftime() is deprecated
    _vm.emit_deprecated("Function gmstrftime() is deprecated since 8.1, use IntlDateFormatter::format() instead");
    strftime_fn(_vm, args)
}

/// date_create - create a DateTime-like value (returns DateTime object with timestamp property)
fn date_create_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let datetime_str = args
        .first()
        .map(|v| {
            if matches!(v, Value::Null) {
                String::new()
            } else {
                v.to_php_string().to_string_lossy()
            }
        })
        .unwrap_or_default();

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let timestamp = if datetime_str.is_empty() || datetime_str.eq_ignore_ascii_case("now") {
        now_secs
    } else {
        match parse_datetime_string(&datetime_str, now_secs) {
            Some(ts) => ts,
            None => return Ok(Value::False),
        }
    };

    let obj_id = _vm.next_object_id();
    let mut obj = PhpObject::new(b"DateTime".to_vec(), obj_id);
    // Store timestamp internally (used by date functions)
    obj.set_property(b"__timestamp".to_vec(), Value::Long(timestamp));
    // Format the date for var_dump display (PHP format)
    let date_str = format_timestamp("Y-m-d H:i:s", timestamp) + ".000000";
    obj.set_property(b"date".to_vec(), Value::String(PhpString::from_string(date_str)));
    obj.set_property(b"timezone_type".to_vec(), Value::Long(3));
    obj.set_property(b"timezone".to_vec(), Value::String(PhpString::from_bytes(b"UTC")));
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

/// getdate - return associative array with date components
fn getdate_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let timestamp = args.get(0).map(|v| v.to_long());

    let secs = timestamp.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });

    let days_since_epoch = secs / 86400;
    let time_of_day = ((secs % 86400) + 86400) % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds_val = time_of_day % 60;
    let (year, month, day) = days_to_ymd(days_since_epoch);

    let dow = (((days_since_epoch % 7) + 4) % 7 + 7) % 7; // 0=Sunday

    let day_names = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    let month_names = [
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

    // Compute day of year (0-based)
    let days_in_months = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let is_leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let mut yday = 0i64;
    for m in 1..month {
        yday += days_in_months[m as usize] as i64;
        if m == 2 && is_leap {
            yday += 1;
        }
    }
    yday += (day as i64) - 1;

    let mut result = PhpArray::new();
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"seconds")),
        Value::Long(seconds_val),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"minutes")),
        Value::Long(minutes),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"hours")),
        Value::Long(hours),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"mday")),
        Value::Long(day as i64),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"wday")),
        Value::Long(dow),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"mon")),
        Value::Long(month as i64),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"year")),
        Value::Long(year),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"yday")),
        Value::Long(yday),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"weekday")),
        Value::String(PhpString::from_string(
            day_names[dow as usize % 7].to_string(),
        )),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"month")),
        Value::String(PhpString::from_string(
            month_names[month as usize].to_string(),
        )),
    );
    result.set(ArrayKey::Int(0), Value::Long(secs));

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

/// localtime - return array of date components
fn localtime_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let timestamp = args.get(0).map(|v| v.to_long());
    let is_assoc = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);

    let secs = timestamp.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });

    let days_since_epoch = secs / 86400;
    let time_of_day = ((secs % 86400) + 86400) % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds_val = time_of_day % 60;
    let (year, month, day) = days_to_ymd(days_since_epoch);

    let dow = (((days_since_epoch % 7) + 4) % 7 + 7) % 7; // 0=Sunday

    // Compute day of year (0-based)
    let days_in_months = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let is_leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let mut yday = 0i64;
    for m in 1..month {
        yday += days_in_months[m as usize] as i64;
        if m == 2 && is_leap {
            yday += 1;
        }
    }
    yday += (day as i64) - 1;

    let mut result = PhpArray::new();

    if is_assoc {
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"tm_sec")),
            Value::Long(seconds_val),
        );
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"tm_min")),
            Value::Long(minutes),
        );
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"tm_hour")),
            Value::Long(hours),
        );
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"tm_mday")),
            Value::Long(day as i64),
        );
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"tm_mon")),
            Value::Long((month as i64) - 1),
        ); // 0-based month
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"tm_year")),
            Value::Long(year - 1900),
        ); // years since 1900
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"tm_wday")),
            Value::Long(dow),
        );
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"tm_yday")),
            Value::Long(yday),
        );
        result.set(
            ArrayKey::String(PhpString::from_bytes(b"tm_isdst")),
            Value::Long(0),
        ); // no DST support
    } else {
        result.push(Value::Long(seconds_val)); // 0: tm_sec
        result.push(Value::Long(minutes)); // 1: tm_min
        result.push(Value::Long(hours)); // 2: tm_hour
        result.push(Value::Long(day as i64)); // 3: tm_mday
        result.push(Value::Long((month as i64) - 1)); // 4: tm_mon (0-based)
        result.push(Value::Long(year - 1900)); // 5: tm_year (years since 1900)
        result.push(Value::Long(dow)); // 6: tm_wday
        result.push(Value::Long(yday)); // 7: tm_yday
        result.push(Value::Long(0)); // 8: tm_isdst
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

/// checkdate - validate a date
fn checkdate_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let month = args.first().map(|v| v.to_long()).unwrap_or(0);
    let day = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let year = args.get(2).map(|v| v.to_long()).unwrap_or(0);

    if year < 1 || year > 32767 || month < 1 || month > 12 || day < 1 {
        return Ok(Value::False);
    }

    let is_leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        2 => {
            if is_leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };

    if day > max_day {
        Ok(Value::False)
    } else {
        Ok(Value::True)
    }
}

/// idate - return a single date component as integer
fn idate_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let format = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();
    let timestamp = args.get(1).map(|v| v.to_long());

    let secs = timestamp.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });

    let days_since_epoch = secs / 86400;
    let time_of_day = ((secs % 86400) + 86400) % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds_val = time_of_day % 60;
    let (year, month, day) = days_to_ymd(days_since_epoch);
    let dow = (((days_since_epoch % 7) + 4) % 7 + 7) % 7;

    // Compute day of year (0-based)
    let days_in_months_arr = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let is_leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let mut yday = 0i64;
    for m in 1..month {
        yday += days_in_months_arr[m as usize] as i64;
        if m == 2 && is_leap {
            yday += 1;
        }
    }
    yday += (day as i64) - 1;

    let days_in_month = match month {
        2 => {
            if is_leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };

    let c = format.as_bytes().first().copied().unwrap_or(b'U');
    let val = match c {
        b'B' => {
            // Swatch internet time
            let beat = ((secs + 3600) % 86400) as f64 / 86.4;
            beat as i64
        }
        b'd' => day as i64,
        b'g' => {
            // 12-hour format without leading zero (1-12)
            let h12 = hours % 12;
            if h12 == 0 { 12 } else { h12 }
        }
        b'G' => hours, // 24-hour format without leading zero (0-23)
        b'h' => {
            let h12 = hours % 12;
            if h12 == 0 { 12 } else { h12 }
        }
        b'H' => hours,
        b'i' => minutes,
        b'I' => 0, // no DST support
        b'j' => day as i64, // day without leading zero
        b'L' => {
            if is_leap {
                1
            } else {
                0
            }
        }
        b'm' => month as i64,
        b'n' => month as i64, // month without leading zero
        b'N' => {
            // ISO-8601 day of week (Monday=1, Sunday=7)
            if dow == 0 { 7 } else { dow }
        }
        b'o' => {
            // ISO-8601 year number (for ISO week)
            // Simplified: same as year for most cases
            // The ISO year can differ from calendar year for dates near Jan 1
            let iso_dow = if dow == 0 { 7 } else { dow }; // Monday=1
            let jan1_days = ymd_to_days(year, 1, 1);
            let jan1_dow = (((jan1_days % 7) + 4) % 7 + 7) % 7;
            let jan1_iso_dow = if jan1_dow == 0 { 7 } else { jan1_dow };
            // ISO week 1 starts on the Monday of the week containing Jan 4
            let iso_week_one_start = jan1_days - (jan1_iso_dow - 1) + if jan1_iso_dow <= 4 { 0 } else { 7 };
            let current_days = ymd_to_days(year, month, day);
            if current_days < iso_week_one_start {
                year - 1 // belongs to previous year's ISO week
            } else {
                // Check if we're in the last week that might belong to next year
                let dec31_days = ymd_to_days(year, 12, 31);
                let dec31_dow = (((dec31_days % 7) + 4) % 7 + 7) % 7;
                let dec31_iso_dow = if dec31_dow == 0 { 7 } else { dec31_dow };
                if dec31_iso_dow < 4 && (dec31_days - current_days) < dec31_iso_dow {
                    year + 1
                } else {
                    year
                }
            }
        }
        b's' => seconds_val,
        b't' => days_in_month,
        b'U' => secs,
        b'w' => dow,
        b'W' => {
            // ISO week number - use the ISO year to compute correctly
            let current_days = ymd_to_days(year, month, day);
            // Try computing with the current year first
            let jan1_days = ymd_to_days(year, 1, 1);
            let jan1_dow = (((jan1_days % 7) + 4) % 7 + 7) % 7;
            let jan1_iso_dow = if jan1_dow == 0 { 7 } else { jan1_dow };
            let iso_week_one_start = jan1_days - (jan1_iso_dow - 1) + if jan1_iso_dow <= 4 { 0 } else { 7 };
            let diff = current_days - iso_week_one_start;
            if diff < 0 {
                // Before ISO week 1 of this year - belongs to last week of previous year
                let prev_jan1_days = ymd_to_days(year - 1, 1, 1);
                let prev_jan1_dow = (((prev_jan1_days % 7) + 4) % 7 + 7) % 7;
                let prev_jan1_iso_dow = if prev_jan1_dow == 0 { 7 } else { prev_jan1_dow };
                let prev_iso_week_one_start = prev_jan1_days - (prev_jan1_iso_dow - 1) + if prev_jan1_iso_dow <= 4 { 0 } else { 7 };
                let prev_diff = current_days - prev_iso_week_one_start;
                (prev_diff / 7 + 1) as i64
            } else {
                let week = (diff / 7 + 1) as i64;
                // Check if this is actually week 1 of next year
                let next_jan1_days = ymd_to_days(year + 1, 1, 1);
                let next_jan1_dow = (((next_jan1_days % 7) + 4) % 7 + 7) % 7;
                let next_jan1_iso_dow = if next_jan1_dow == 0 { 7 } else { next_jan1_dow };
                let next_iso_week_one_start = next_jan1_days - (next_jan1_iso_dow - 1) + if next_jan1_iso_dow <= 4 { 0 } else { 7 };
                if current_days >= next_iso_week_one_start {
                    let next_diff = current_days - next_iso_week_one_start;
                    (next_diff / 7 + 1) as i64
                } else {
                    week
                }
            }
        }
        b'y' => year % 100,
        b'Y' => year,
        b'z' => yday,
        b'Z' => 0, // timezone offset, 0 for UTC
        _ => 0,
    };

    Ok(Value::Long(val))
}

/// date_format($object, $format) - format a DateTime object
fn date_format_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let obj = args.first().unwrap_or(&Value::Null);
    let format = args.get(1).unwrap_or(&Value::Null).to_php_string().to_string_lossy();

    let (timestamp, tz_name) = if let Value::Object(o) = obj {
        let obj_borrow = o.borrow();
        let ts = obj_borrow.get_property(b"__timestamp").to_long();
        let tz = obj_borrow.get_property(b"timezone").to_php_string().to_string_lossy();
        (ts, if tz.is_empty() { get_default_tz(_vm) } else { tz })
    } else {
        (0, get_default_tz(_vm))
    };

    let (offset_secs, tz_abbrev) = timezone_offset_and_abbrev(&tz_name, timestamp);
    let result = format_timestamp_with_tz(&format, timestamp + offset_secs, &tz_abbrev, offset_secs);
    Ok(Value::String(PhpString::from_string(result)))
}

fn get_default_tz(vm: &Vm) -> String {
    vm.constants.get(b"__default_timezone".as_ref())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| "UTC".to_string())
}

/// Get the UTC offset (in seconds) and abbreviation for a timezone name at a given timestamp
pub fn timezone_offset_and_abbrev(tz_name: &str, _timestamp: i64) -> (i64, String) {
    match tz_name {
        "UTC" | "utc" => (0, "UTC".to_string()),
        "America/New_York" | "US/Eastern" => (-5 * 3600, "EST".to_string()),
        "America/Chicago" | "US/Central" => (-6 * 3600, "CST".to_string()),
        "America/Denver" | "US/Mountain" => (-7 * 3600, "MST".to_string()),
        "America/Los_Angeles" | "US/Pacific" => (-8 * 3600, "PST".to_string()),
        "Europe/London" => (0, "GMT".to_string()),
        "Europe/Paris" | "Europe/Berlin" | "Europe/Amsterdam" | "Europe/Brussels" | "Europe/Rome" | "CET" => (1 * 3600, "CET".to_string()),
        "Europe/Helsinki" | "Europe/Athens" | "EET" => (2 * 3600, "EET".to_string()),
        "Europe/Moscow" => (3 * 3600, "MSK".to_string()),
        "Asia/Tokyo" | "Japan" => (9 * 3600, "JST".to_string()),
        "Asia/Shanghai" | "Asia/Hong_Kong" | "PRC" => (8 * 3600, "CST".to_string()),
        "Asia/Kolkata" | "Asia/Calcutta" => (5 * 3600 + 1800, "IST".to_string()),
        "Australia/Sydney" => (10 * 3600, "AEST".to_string()),
        "Pacific/Auckland" | "NZ" => (12 * 3600, "NZST".to_string()),
        _ => {
            // Try to parse timezone offset like "+05:30"
            if let Some(offset) = parse_tz_offset(tz_name) {
                let abbrev = format_tz_offset(offset);
                (offset, abbrev)
            } else {
                (0, "UTC".to_string())
            }
        }
    }
}

fn parse_tz_offset(s: &str) -> Option<i64> {
    if s.len() >= 5 && (s.starts_with('+') || s.starts_with('-')) {
        let sign = if s.starts_with('-') { -1i64 } else { 1 };
        let rest = &s[1..];
        if let Some(colon) = rest.find(':') {
            let hours: i64 = rest[..colon].parse().ok()?;
            let mins: i64 = rest[colon+1..].parse().ok()?;
            Some(sign * (hours * 3600 + mins * 60))
        } else if rest.len() == 4 {
            let hours: i64 = rest[..2].parse().ok()?;
            let mins: i64 = rest[2..].parse().ok()?;
            Some(sign * (hours * 3600 + mins * 60))
        } else {
            None
        }
    } else {
        None
    }
}

fn format_tz_offset(offset_secs: i64) -> String {
    let sign = if offset_secs < 0 { '-' } else { '+' };
    let abs_offset = offset_secs.unsigned_abs();
    let hours = abs_offset / 3600;
    let mins = (abs_offset % 3600) / 60;
    format!("{}{:02}:{:02}", sign, hours, mins)
}

/// Format timestamp with timezone-aware output
pub fn format_timestamp_with_tz(format: &str, local_secs: i64, tz_abbrev: &str, offset_secs: i64) -> String {
    // This is format_timestamp but using the local time and timezone info
    let days_since_epoch = if local_secs >= 0 {
        local_secs / 86400
    } else {
        (local_secs - 86399) / 86400
    };
    let time_of_day = ((local_secs % 86400) + 86400) % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = days_to_ymd(days_since_epoch);

    let mut result = String::new();
    let fmt_bytes = format.as_bytes();
    let mut i = 0;
    while i < fmt_bytes.len() {
        let c = fmt_bytes[i];
        if c == b'\\' && i + 1 < fmt_bytes.len() {
            result.push(fmt_bytes[i + 1] as char);
            i += 2;
            continue;
        }
        match c {
            b'Y' => result.push_str(&format!("{:04}", year)),
            b'y' => result.push_str(&format!("{:02}", year % 100)),
            b'm' => result.push_str(&format!("{:02}", month)),
            b'n' => result.push_str(&format!("{}", month)),
            b'd' => result.push_str(&format!("{:02}", day)),
            b'j' => result.push_str(&format!("{}", day)),
            b'H' => result.push_str(&format!("{:02}", hours)),
            b'G' => result.push_str(&format!("{}", hours)),
            b'i' => result.push_str(&format!("{:02}", minutes)),
            b's' => result.push_str(&format!("{:02}", seconds)),
            b'A' => result.push_str(if hours >= 12 { "PM" } else { "AM" }),
            b'a' => result.push_str(if hours >= 12 { "pm" } else { "am" }),
            b'g' => result.push_str(&format!("{}", if hours == 0 { 12 } else if hours > 12 { hours - 12 } else { hours })),
            b'h' => result.push_str(&format!("{:02}", if hours == 0 { 12 } else if hours > 12 { hours - 12 } else { hours })),
            b'e' => result.push_str(tz_abbrev),
            b'T' => result.push_str(tz_abbrev),
            b'O' => {
                let sign = if offset_secs < 0 { '-' } else { '+' };
                let abs = offset_secs.unsigned_abs();
                result.push_str(&format!("{}{:02}{:02}", sign, abs / 3600, (abs % 3600) / 60));
            }
            b'P' => {
                let sign = if offset_secs < 0 { '-' } else { '+' };
                let abs = offset_secs.unsigned_abs();
                result.push_str(&format!("{}{:02}:{:02}", sign, abs / 3600, (abs % 3600) / 60));
            }
            b'p' => {
                if offset_secs == 0 { result.push('Z'); }
                else {
                    let sign = if offset_secs < 0 { '-' } else { '+' };
                    let abs = offset_secs.unsigned_abs();
                    result.push_str(&format!("{}{:02}:{:02}", sign, abs / 3600, (abs % 3600) / 60));
                }
            }
            b'Z' => result.push_str(&format!("{}", offset_secs)),
            b'U' => result.push_str(&format!("{}", local_secs - offset_secs)),
            b'N' => {
                let dow = ((days_since_epoch % 7 + 7) % 7) + 1;
                let iso_dow = if dow == 0 { 7 } else { dow };
                result.push_str(&format!("{}", iso_dow));
            }
            b'w' => {
                let dow = ((days_since_epoch + 4) % 7 + 7) % 7;
                result.push_str(&format!("{}", dow));
            }
            b'D' => {
                let dow = ((days_since_epoch + 4) % 7 + 7) % 7;
                let names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
                result.push_str(names[dow as usize % 7]);
            }
            b'l' => {
                let dow = ((days_since_epoch + 4) % 7 + 7) % 7;
                let names = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
                result.push_str(names[dow as usize % 7]);
            }
            b'F' => {
                let names = ["January","February","March","April","May","June","July","August","September","October","November","December"];
                if month >= 1 && month <= 12 { result.push_str(names[(month - 1) as usize]); }
            }
            b'M' => {
                let names = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
                if month >= 1 && month <= 12 { result.push_str(names[(month - 1) as usize]); }
            }
            b't' => {
                let days_in_month = match month {
                    1|3|5|7|8|10|12 => 31, 4|6|9|11 => 30,
                    2 => if (year%4==0 && year%100!=0) || year%400==0 { 29 } else { 28 },
                    _ => 30,
                };
                result.push_str(&format!("{}", days_in_month));
            }
            b'L' => {
                let leap = if (year%4==0 && year%100!=0) || year%400==0 { 1 } else { 0 };
                result.push_str(&format!("{}", leap));
            }
            b'S' => {
                // English ordinal suffix
                let suffix = match day % 10 {
                    1 if day != 11 => "st",
                    2 if day != 12 => "nd",
                    3 if day != 13 => "rd",
                    _ => "th",
                };
                result.push_str(suffix);
            }
            b'z' => {
                // Day of the year (0..365)
                let days_in_months = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
                let leap = if (year%4==0 && year%100!=0) || year%400==0 { 1 } else { 0 };
                let mut doy = days_in_months[((month - 1).max(0) as usize).min(11)] + day - 1;
                if month > 2 { doy += leap; }
                result.push_str(&format!("{}", doy));
            }
            b'W' => {
                // ISO 8601 week number
                let jan1_days = ymd_to_days(year, 1, 1);
                let jan1_dow = ((jan1_days + 4) % 7 + 7) % 7; // 0=Sun
                let iso_jan1_dow = if jan1_dow == 0 { 7 } else { jan1_dow }; // 1=Mon..7=Sun
                let ordinal_day = {
                    let days_in_months = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
                    let leap = if (year%4==0 && year%100!=0) || year%400==0 { 1 } else { 0 };
                    let mut d = days_in_months[((month - 1).max(0) as usize).min(11)] + day;
                    if month > 2 { d += leap; }
                    d
                };
                let wk = (ordinal_day as i64 - 1 + (iso_jan1_dow - 1) as i64) / 7;
                let iso_week = if iso_jan1_dow <= 4 { wk + 1 } else { wk };
                let iso_week = if iso_week == 0 { 52 } else { iso_week };
                result.push_str(&format!("{:02}", iso_week));
            }
            b'B' => {
                // Swatch Internet Time
                let utc_secs = local_secs - offset_secs;
                let bmt_secs = utc_secs + 3600; // BMT = UTC+1
                let day_secs = ((bmt_secs % 86400) + 86400) % 86400;
                let beats = day_secs as f64 / 86.4;
                result.push_str(&format!("{:03}", beats as i64 % 1000));
            }
            b'c' => {
                let sign = if offset_secs < 0 { '-' } else { '+' };
                let abs = offset_secs.unsigned_abs();
                result.push_str(&format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
                    year, month, day, hours, minutes, seconds, sign, abs/3600, (abs%3600)/60));
            }
            b'r' => {
                let dow = ((days_since_epoch + 4) % 7 + 7) % 7;
                let day_names = ["Sun","Mon","Tue","Wed","Thu","Fri","Sat"];
                let month_names = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
                let sign = if offset_secs < 0 { '-' } else { '+' };
                let abs = offset_secs.unsigned_abs();
                result.push_str(&format!("{}, {:02} {} {:04} {:02}:{:02}:{:02} {}{:02}{:02}",
                    day_names[dow as usize % 7], day, month_names[(month-1).max(0) as usize % 12],
                    year, hours, minutes, seconds, sign, abs/3600, (abs%3600)/60));
            }
            _ => result.push(c as char),
        }
        i += 1;
    }
    result
}

/// date_create_immutable - same as date_create but returns DateTimeImmutable
fn date_create_immutable_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let datetime_str = args
        .first()
        .map(|v| {
            if matches!(v, Value::Null) {
                String::new()
            } else {
                v.to_php_string().to_string_lossy()
            }
        })
        .unwrap_or_default();

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let timestamp = if datetime_str.is_empty() || datetime_str.eq_ignore_ascii_case("now") {
        now_secs
    } else {
        match parse_datetime_string(&datetime_str, now_secs) {
            Some(ts) => ts,
            None => return Ok(Value::False),
        }
    };

    let obj_id = _vm.next_object_id();
    let mut obj = PhpObject::new(b"DateTimeImmutable".to_vec(), obj_id);
    obj.set_property(b"__timestamp".to_vec(), Value::Long(timestamp));
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

/// Helper: format a timestamp using a format string (like PHP's date() function)
pub fn format_timestamp(format: &str, secs: i64) -> String {
    let days_since_epoch = secs / 86400;
    let time_of_day = ((secs % 86400) + 86400) % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = days_to_ymd(days_since_epoch);

    let mut result = String::new();
    let fmt_bytes = format.as_bytes();
    let mut i = 0;
    while i < fmt_bytes.len() {
        let c = fmt_bytes[i];
        if c == b'\\' && i + 1 < fmt_bytes.len() {
            result.push(fmt_bytes[i + 1] as char);
            i += 2;
            continue;
        }
        match c {
            b'Y' => result.push_str(&format!("{:04}", year)),
            b'y' => result.push_str(&format!("{:02}", year % 100)),
            b'm' => result.push_str(&format!("{:02}", month)),
            b'n' => result.push_str(&format!("{}", month)),
            b'd' => result.push_str(&format!("{:02}", day)),
            b'j' => result.push_str(&format!("{}", day)),
            b'H' => result.push_str(&format!("{:02}", hours)),
            b'G' => result.push_str(&format!("{}", hours)),
            b'i' => result.push_str(&format!("{:02}", minutes)),
            b's' => result.push_str(&format!("{:02}", seconds)),
            b'A' => result.push_str(if hours >= 12 { "PM" } else { "AM" }),
            b'a' => result.push_str(if hours >= 12 { "pm" } else { "am" }),
            b'g' => {
                let h12 = if hours == 0 { 12 } else if hours > 12 { hours - 12 } else { hours };
                result.push_str(&format!("{}", h12));
            }
            b'h' => {
                let h12 = if hours == 0 { 12 } else if hours > 12 { hours - 12 } else { hours };
                result.push_str(&format!("{:02}", h12));
            }
            b'U' => result.push_str(&format!("{}", secs)),
            b'N' => {
                let dow = ((days_since_epoch % 7 + 4 + 7) % 7) as i64;
                result.push_str(&format!("{}", if dow == 0 { 7 } else { dow }));
            }
            b'w' => {
                let dow = ((days_since_epoch % 7 + 4 + 7) % 7) as i64;
                result.push_str(&format!("{}", dow));
            }
            b'D' => {
                let dow = ((days_since_epoch % 7 + 4 + 7) % 7) as usize;
                let names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
                result.push_str(names[dow]);
            }
            b'l' => {
                let dow = ((days_since_epoch % 7 + 4 + 7) % 7) as usize;
                let names = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
                result.push_str(names[dow]);
            }
            b'F' => {
                let names = ["January", "February", "March", "April", "May", "June",
                    "July", "August", "September", "October", "November", "December"];
                result.push_str(names[(month - 1) as usize]);
            }
            b'M' => {
                let names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
                    "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
                result.push_str(names[(month - 1) as usize]);
            }
            b't' => {
                let is_leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
                let dim = match month {
                    1 => 31, 2 => if is_leap { 29 } else { 28 }, 3 => 31, 4 => 30,
                    5 => 31, 6 => 30, 7 => 31, 8 => 31, 9 => 30, 10 => 31, 11 => 30, 12 => 31,
                    _ => 30,
                };
                result.push_str(&format!("{}", dim));
            }
            b'L' => {
                let leap = if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 { 1 } else { 0 };
                result.push_str(&format!("{}", leap));
            }
            b'S' => {
                let suffix = match day % 10 {
                    1 if day != 11 => "st",
                    2 if day != 12 => "nd",
                    3 if day != 13 => "rd",
                    _ => "th",
                };
                result.push_str(suffix);
            }
            b'z' => {
                let days_in_months = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
                let leap = if (year%4==0 && year%100!=0) || year%400==0 { 1 } else { 0 };
                let mut doy = days_in_months[((month - 1).max(0) as usize).min(11)] + day - 1;
                if month > 2 { doy += leap; }
                result.push_str(&format!("{}", doy));
            }
            b'W' => {
                let jan1_days = ymd_to_days(year, 1, 1);
                let jan1_dow = ((jan1_days + 4) % 7 + 7) % 7;
                let iso_jan1_dow = if jan1_dow == 0 { 7 } else { jan1_dow };
                let ordinal_day = {
                    let days_in_months = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
                    let leap = if (year%4==0 && year%100!=0) || year%400==0 { 1 } else { 0 };
                    let mut d = days_in_months[((month - 1).max(0) as usize).min(11)] + day;
                    if month > 2 { d += leap; }
                    d
                };
                let wk = (ordinal_day as i64 - 1 + (iso_jan1_dow - 1) as i64) / 7;
                let iso_week = if iso_jan1_dow <= 4 { wk + 1 } else { wk };
                let iso_week = if iso_week == 0 { 52 } else { iso_week };
                result.push_str(&format!("{:02}", iso_week));
            }
            b'B' => {
                let bmt_secs = secs + 3600; // BMT = UTC+1
                let day_secs = ((bmt_secs % 86400) + 86400) % 86400;
                let beats = day_secs as f64 / 86.4;
                result.push_str(&format!("{:03}", beats as i64 % 1000));
            }
            b'e' | b'T' => result.push_str("UTC"),
            b'O' => result.push_str("+0000"),
            b'P' => result.push_str("+00:00"),
            b'p' => result.push_str("Z"),
            b'Z' => result.push_str("0"),
            b'c' => {
                result.push_str(&format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}+00:00",
                    year, month, day, hours, minutes, seconds));
            }
            b'r' => {
                let dow = ((days_since_epoch % 7 + 4 + 7) % 7) as usize;
                let day_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
                let month_names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
                    "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
                result.push_str(&format!("{}, {:02} {} {:04} {:02}:{:02}:{:02} +0000",
                    day_names[dow], day, month_names[(month-1) as usize], year, hours, minutes, seconds));
            }
            _ => result.push(c as char),
        }
        i += 1;
    }
    result
}

// ==================== Comprehensive date string parser ====================

/// Parse a datetime string into a unix timestamp.
/// Supports: "now", "@timestamp", "Y-m-d", "Y-m-d H:i:s", "Y-m-dTH:i:s",
/// "yesterday", "today", "tomorrow", relative expressions like "+1 day", "-2 hours",
/// month names, "first day of", "last day of", etc.
pub fn parse_datetime_string(input: &str, now: i64) -> Option<i64> {
    let s = input.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("now") {
        return Some(now);
    }

    // @timestamp
    if s.starts_with('@') {
        return s[1..].trim().parse::<i64>().ok();
    }

    // Try absolute date formats first
    if let Some(ts) = try_parse_absolute(s) {
        return Some(ts);
    }

    // Try relative date expressions
    if let Some(ts) = try_parse_relative(s, now) {
        return Some(ts);
    }

    None
}

/// Try to parse an absolute date/time string
fn try_parse_absolute(s: &str) -> Option<i64> {
    // "Y-m-d H:i:s" or "Y-m-dTH:i:s"
    let parts: Vec<&str> = s.splitn(2, |c: char| c == ' ' || c == 'T').collect();
    let date_str = parts.first()?;

    // Try Y-m-d format
    let date_parts: Vec<&str> = date_str.split('-').collect();
    if date_parts.len() == 3 {
        let year = date_parts[0].parse::<i64>().ok()?;
        let month = date_parts[1].parse::<u32>().ok()?;
        let day = date_parts[2].parse::<u32>().ok()?;
        if month < 1 || month > 12 || day < 1 || day > 31 {
            return None;
        }
        let mut h = 0i64;
        let mut m = 0i64;
        let mut sec = 0i64;
        if let Some(time_str) = parts.get(1) {
            // Strip timezone suffix if present (e.g., "+00:00", "Z", " UTC")
            let time_clean = time_str
                .trim_end_matches(|c: char| c == 'Z' || c == 'z')
                .split('+').next().unwrap_or(time_str)
                .split('-').next().unwrap_or(time_str)
                .trim();
            // Handle "H:i:s.u" (with microseconds)
            let time_no_micro = time_clean.split('.').next().unwrap_or(time_clean);
            let time_parts: Vec<&str> = time_no_micro.split(':').collect();
            h = time_parts.first().and_then(|v| v.parse().ok()).unwrap_or(0);
            m = time_parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
            sec = time_parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);
        }
        let days = ymd_to_days(year, month, day);
        return Some(days * 86400 + h * 3600 + m * 60 + sec);
    }

    // Try m/d/Y format (US format)
    let slash_parts: Vec<&str> = date_str.split('/').collect();
    if slash_parts.len() == 3 {
        let month = slash_parts[0].parse::<u32>().ok()?;
        let day = slash_parts[1].parse::<u32>().ok()?;
        let year = slash_parts[2].parse::<i64>().ok()?;
        if month >= 1 && month <= 12 && day >= 1 && day <= 31 {
            let mut h = 0i64;
            let mut m = 0i64;
            let mut sec = 0i64;
            if let Some(time_str) = parts.get(1) {
                let time_parts: Vec<&str> = time_str.split(':').collect();
                h = time_parts.first().and_then(|v| v.parse().ok()).unwrap_or(0);
                m = time_parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
                sec = time_parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            let days = ymd_to_days(year, month, day);
            return Some(days * 86400 + h * 3600 + m * 60 + sec);
        }
    }

    // Try "d Mon Y" or "Mon d, Y" format
    // "15 Jan 2024" or "Jan 15, 2024"
    if let Some(ts) = try_parse_textual_date(s) {
        return Some(ts);
    }

    None
}

/// Parse textual date like "15 Jan 2024", "Jan 15 2024", "January 15, 2024"
fn try_parse_textual_date(s: &str) -> Option<i64> {
    let month_names = [
        ("jan", 1), ("january", 1), ("feb", 2), ("february", 2),
        ("mar", 3), ("march", 3), ("apr", 4), ("april", 4),
        ("may", 5), ("jun", 6), ("june", 6), ("jul", 7), ("july", 7),
        ("aug", 8), ("august", 8), ("sep", 9), ("september", 9),
        ("oct", 10), ("october", 10), ("nov", 11), ("november", 11),
        ("dec", 12), ("december", 12),
    ];

    let lower = s.to_lowercase();
    let tokens: Vec<&str> = lower.split(|c: char| c == ' ' || c == ',' || c == '-').filter(|t| !t.is_empty()).collect();

    if tokens.len() >= 3 {
        // Try "Mon d Y" pattern
        if let Some(&(_, mon)) = month_names.iter().find(|(name, _)| *name == tokens[0]) {
            if let (Ok(day), Ok(year)) = (tokens[1].parse::<u32>(), tokens[2].parse::<i64>()) {
                let days = ymd_to_days(year, mon, day);
                return Some(days * 86400);
            }
        }
        // Try "d Mon Y" pattern
        if let Some(&(_, mon)) = month_names.iter().find(|(name, _)| *name == tokens[1]) {
            if let (Ok(day), Ok(year)) = (tokens[0].parse::<u32>(), tokens[2].parse::<i64>()) {
                let days = ymd_to_days(year, mon, day);
                return Some(days * 86400);
            }
        }
    }
    None
}

/// Try to parse relative date expressions
fn try_parse_relative(s: &str, now: i64) -> Option<i64> {
    let lower = s.to_lowercase().trim().to_string();

    // Simple keywords
    match lower.as_str() {
        "yesterday" => {
            let days = now / 86400;
            return Some((days - 1) * 86400);
        }
        "today" | "midnight" => {
            let days = now / 86400;
            return Some(days * 86400);
        }
        "tomorrow" => {
            let days = now / 86400;
            return Some((days + 1) * 86400);
        }
        "noon" => {
            let days = now / 86400;
            return Some(days * 86400 + 12 * 3600);
        }
        _ => {}
    }

    // "first day of January 2024", etc. - extract month/year
    if lower.starts_with("first day of") || lower.starts_with("last day of") {
        let is_first = lower.starts_with("first");
        let rest = if is_first { &lower[13..] } else { &lower[12..] };
        let rest = rest.trim();

        // Try to parse as "Month Year" or relative month
        if let Some(ts) = parse_month_year_expression(rest, now, is_first) {
            return Some(ts);
        }
    }

    // Apply relative modifications to the current timestamp
    apply_relative_modification(&lower, now)
}

fn parse_month_year_expression(s: &str, now: i64, is_first: bool) -> Option<i64> {
    let month_names = [
        ("jan", 1u32), ("january", 1), ("feb", 2), ("february", 2),
        ("mar", 3), ("march", 3), ("apr", 4), ("april", 4),
        ("may", 5), ("jun", 6), ("june", 6), ("jul", 7), ("july", 7),
        ("aug", 8), ("august", 8), ("sep", 9), ("september", 9),
        ("oct", 10), ("october", 10), ("nov", 11), ("november", 11),
        ("dec", 12), ("december", 12),
    ];

    let lower = s.to_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();

    // "next month", "this month", "+1 month" etc. relative to now
    if lower.contains("month") || lower.contains("year") {
        return apply_relative_modification(s, now);
    }

    // "January 2024" or "January"
    for &(name, mon) in &month_names {
        if tokens.first().map(|t| *t == name).unwrap_or(false) {
            let now_days = now / 86400;
            let (now_year, _, _) = days_to_ymd(now_days);
            let year = tokens.get(1).and_then(|t| t.parse::<i64>().ok()).unwrap_or(now_year);
            if is_first {
                let days = ymd_to_days(year, mon, 1);
                return Some(days * 86400);
            } else {
                let is_leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
                let last = match mon {
                    2 => if is_leap { 29 } else { 28 },
                    4 | 6 | 9 | 11 => 30,
                    _ => 31,
                };
                let days = ymd_to_days(year, mon, last);
                return Some(days * 86400);
            }
        }
    }
    None
}

/// Apply a relative modification string to a timestamp.
/// Handles: "+1 day", "-2 hours", "next month", "last year", compound like "+1 day +2 hours"
pub fn apply_relative_modification(s: &str, ts: i64) -> Option<i64> {
    let lower = s.to_lowercase().trim().to_string();
    let mut result = ts;

    // Split compound expressions and handle each
    // First handle "yesterday/today/tomorrow" prefixes
    let working = if lower.starts_with("yesterday") {
        let days = ts / 86400;
        result = (days - 1) * 86400 + (ts % 86400 + 86400) % 86400;
        lower[9..].trim().to_string()
    } else if lower.starts_with("tomorrow") {
        let days = ts / 86400;
        result = (days + 1) * 86400 + (ts % 86400 + 86400) % 86400;
        lower[8..].trim().to_string()
    } else if lower.starts_with("today") || lower.starts_with("now") {
        let skip = if lower.starts_with("today") { 5 } else { 3 };
        lower[skip..].trim().to_string()
    } else {
        lower.clone()
    };

    if working.is_empty() {
        return Some(result);
    }

    // Tokenize and process
    let tokens: Vec<&str> = working.split_whitespace().collect();
    let mut i = 0;
    let mut any_match = false;

    while i < tokens.len() {
        let token = tokens[i];

        // Handle "next", "last", "this" prefix
        if token == "next" || token == "last" || token == "this" {
            if i + 1 < tokens.len() {
                let unit = tokens[i + 1];
                let amount: i64 = if token == "next" { 1 } else if token == "last" { -1 } else { 0 };
                if let Some(new_ts) = apply_unit_modification(result, amount, unit) {
                    result = new_ts;
                    any_match = true;
                }
                i += 2;
                continue;
            }
        }

        // Handle "+N unit" or "-N unit" or "N unit" or "N units ago"
        if let Some(amount) = parse_amount(token) {
            if i + 1 < tokens.len() {
                let unit = tokens[i + 1];
                let actual_amount = if i + 2 < tokens.len() && tokens[i + 2] == "ago" {
                    i += 1; // skip "ago"
                    -amount
                } else {
                    amount
                };
                if let Some(new_ts) = apply_unit_modification(result, actual_amount, unit) {
                    result = new_ts;
                    any_match = true;
                }
                i += 2;
                continue;
            }
        }

        // Handle day names for "next Monday", etc.
        let day_names = [
            ("sunday", 0), ("monday", 1), ("tuesday", 2), ("wednesday", 3),
            ("thursday", 4), ("friday", 5), ("saturday", 6),
            ("sun", 0), ("mon", 1), ("tue", 2), ("wed", 3),
            ("thu", 4), ("fri", 5), ("sat", 6),
        ];
        if let Some(&(_, target_dow)) = day_names.iter().find(|(name, _)| *name == token) {
            let current_days = result / 86400;
            let current_dow = (((current_days % 7) + 4) % 7 + 7) % 7; // 0=Sun
            let mut diff = target_dow - current_dow;
            if diff <= 0 {
                diff += 7;
            }
            result = (current_days + diff) * 86400 + (result % 86400 + 86400) % 86400;
            any_match = true;
            i += 1;
            continue;
        }

        i += 1;
    }

    if any_match { Some(result) } else { None }
}

fn parse_amount(s: &str) -> Option<i64> {
    if s.starts_with('+') {
        s[1..].parse::<i64>().ok()
    } else if s.starts_with('-') {
        s.parse::<i64>().ok()
    } else {
        s.parse::<i64>().ok()
    }
}

fn apply_unit_modification(ts: i64, amount: i64, unit: &str) -> Option<i64> {
    let unit = unit.trim_end_matches('s'); // "days" -> "day"
    match unit {
        "second" | "sec" => Some(ts + amount),
        "minute" | "min" => Some(ts + amount * 60),
        "hour" => Some(ts + amount * 3600),
        "day" => Some(ts + amount * 86400),
        "week" => Some(ts + amount * 7 * 86400),
        "fortnight" => Some(ts + amount * 14 * 86400),
        "month" => {
            let days = ts / 86400;
            let time_of_day = ((ts % 86400) + 86400) % 86400;
            let (year, month, day) = days_to_ymd(days);
            let new_month = month as i64 + amount;
            let (adj_year, adj_month) = if new_month > 0 {
                let y = year + (new_month - 1) / 12;
                let m = ((new_month - 1) % 12 + 1) as u32;
                (y, m)
            } else {
                let y = year + (new_month - 12) / 12;
                let m = (12 - ((-new_month) % 12)) as u32;
                let m = if m == 0 { 12 } else { m };
                (y, m)
            };
            // Clamp day to max days in target month
            let is_leap = adj_year % 4 == 0 && (adj_year % 100 != 0 || adj_year % 400 == 0);
            let max_day = match adj_month {
                2 => if is_leap { 29 } else { 28 },
                4 | 6 | 9 | 11 => 30,
                _ => 31,
            };
            let adj_day = day.min(max_day);
            let new_days = ymd_to_days(adj_year, adj_month, adj_day);
            Some(new_days * 86400 + time_of_day)
        }
        "year" => {
            let days = ts / 86400;
            let time_of_day = ((ts % 86400) + 86400) % 86400;
            let (year, month, day) = days_to_ymd(days);
            let new_year = year + amount;
            // Handle Feb 29 on non-leap year
            let is_leap = new_year % 4 == 0 && (new_year % 100 != 0 || new_year % 400 == 0);
            let adj_day = if month == 2 && day == 29 && !is_leap { 28 } else { day };
            let new_days = ymd_to_days(new_year, month, adj_day);
            Some(new_days * 86400 + time_of_day)
        }
        _ => None,
    }
}

// ==================== New date functions ====================

/// date_parse($string) - Parse about any English textual datetime description into an array
fn date_parse_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let datetime_str = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut result = PhpArray::new();

    // Try to parse the date string
    let parsed = parse_datetime_string(&datetime_str, now_secs);

    if let Some(ts) = parsed {
        let days = ts / 86400;
        let time_of_day = ((ts % 86400) + 86400) % 86400;
        let (year, month, day) = days_to_ymd(days);
        let hours = time_of_day / 3600;
        let minutes = (time_of_day % 3600) / 60;
        let seconds = time_of_day % 60;

        result.set(ArrayKey::String(PhpString::from_bytes(b"year")), Value::Long(year));
        result.set(ArrayKey::String(PhpString::from_bytes(b"month")), Value::Long(month as i64));
        result.set(ArrayKey::String(PhpString::from_bytes(b"day")), Value::Long(day as i64));
        result.set(ArrayKey::String(PhpString::from_bytes(b"hour")), Value::Long(hours));
        result.set(ArrayKey::String(PhpString::from_bytes(b"minute")), Value::Long(minutes));
        result.set(ArrayKey::String(PhpString::from_bytes(b"second")), Value::Long(seconds));
        result.set(ArrayKey::String(PhpString::from_bytes(b"fraction")), Value::Double(0.0));
        result.set(ArrayKey::String(PhpString::from_bytes(b"warning_count")), Value::Long(0));
        result.set(ArrayKey::String(PhpString::from_bytes(b"warnings")), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
        result.set(ArrayKey::String(PhpString::from_bytes(b"error_count")), Value::Long(0));
        result.set(ArrayKey::String(PhpString::from_bytes(b"errors")), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
        result.set(ArrayKey::String(PhpString::from_bytes(b"is_localtime")), Value::False);
    } else {
        result.set(ArrayKey::String(PhpString::from_bytes(b"year")), Value::False);
        result.set(ArrayKey::String(PhpString::from_bytes(b"month")), Value::False);
        result.set(ArrayKey::String(PhpString::from_bytes(b"day")), Value::False);
        result.set(ArrayKey::String(PhpString::from_bytes(b"hour")), Value::False);
        result.set(ArrayKey::String(PhpString::from_bytes(b"minute")), Value::False);
        result.set(ArrayKey::String(PhpString::from_bytes(b"second")), Value::False);
        result.set(ArrayKey::String(PhpString::from_bytes(b"fraction")), Value::Double(0.0));
        result.set(ArrayKey::String(PhpString::from_bytes(b"warning_count")), Value::Long(0));
        result.set(ArrayKey::String(PhpString::from_bytes(b"warnings")), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
        result.set(ArrayKey::String(PhpString::from_bytes(b"error_count")), Value::Long(1));
        let mut errors = PhpArray::new();
        errors.set(ArrayKey::Int(0), Value::String(PhpString::from_string(format!("The string \"{}\" could not be parsed", datetime_str))));
        result.set(ArrayKey::String(PhpString::from_bytes(b"errors")), Value::Array(Rc::new(RefCell::new(errors))));
        result.set(ArrayKey::String(PhpString::from_bytes(b"is_localtime")), Value::False);
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

/// date_parse_from_format($format, $datetime)
fn date_parse_from_format_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Simplified version - just use regular parsing
    let _format = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let datetime_str = args.get(1).unwrap_or(&Value::Null).to_php_string().to_string_lossy();

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let parsed = parse_datetime_string(&datetime_str, now_secs);
    let mut result = PhpArray::new();

    if let Some(ts) = parsed {
        let days = ts / 86400;
        let time_of_day = ((ts % 86400) + 86400) % 86400;
        let (year, month, day) = days_to_ymd(days);
        let hours = time_of_day / 3600;
        let minutes = (time_of_day % 3600) / 60;
        let seconds = time_of_day % 60;

        result.set(ArrayKey::String(PhpString::from_bytes(b"year")), Value::Long(year));
        result.set(ArrayKey::String(PhpString::from_bytes(b"month")), Value::Long(month as i64));
        result.set(ArrayKey::String(PhpString::from_bytes(b"day")), Value::Long(day as i64));
        result.set(ArrayKey::String(PhpString::from_bytes(b"hour")), Value::Long(hours));
        result.set(ArrayKey::String(PhpString::from_bytes(b"minute")), Value::Long(minutes));
        result.set(ArrayKey::String(PhpString::from_bytes(b"second")), Value::Long(seconds));
        result.set(ArrayKey::String(PhpString::from_bytes(b"fraction")), Value::Double(0.0));
        result.set(ArrayKey::String(PhpString::from_bytes(b"warning_count")), Value::Long(0));
        result.set(ArrayKey::String(PhpString::from_bytes(b"warnings")), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
        result.set(ArrayKey::String(PhpString::from_bytes(b"error_count")), Value::Long(0));
        result.set(ArrayKey::String(PhpString::from_bytes(b"errors")), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
        result.set(ArrayKey::String(PhpString::from_bytes(b"is_localtime")), Value::False);
    } else {
        result.set(ArrayKey::String(PhpString::from_bytes(b"year")), Value::False);
        result.set(ArrayKey::String(PhpString::from_bytes(b"month")), Value::False);
        result.set(ArrayKey::String(PhpString::from_bytes(b"day")), Value::False);
        result.set(ArrayKey::String(PhpString::from_bytes(b"hour")), Value::False);
        result.set(ArrayKey::String(PhpString::from_bytes(b"minute")), Value::False);
        result.set(ArrayKey::String(PhpString::from_bytes(b"second")), Value::False);
        result.set(ArrayKey::String(PhpString::from_bytes(b"fraction")), Value::Double(0.0));
        result.set(ArrayKey::String(PhpString::from_bytes(b"warning_count")), Value::Long(0));
        result.set(ArrayKey::String(PhpString::from_bytes(b"warnings")), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
        result.set(ArrayKey::String(PhpString::from_bytes(b"error_count")), Value::Long(1));
        let mut errors = PhpArray::new();
        errors.set(ArrayKey::Int(0), Value::String(PhpString::from_string("Could not be parsed".to_string())));
        result.set(ArrayKey::String(PhpString::from_bytes(b"errors")), Value::Array(Rc::new(RefCell::new(errors))));
        result.set(ArrayKey::String(PhpString::from_bytes(b"is_localtime")), Value::False);
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

/// date_modify($object, $modifier) - Modify a DateTime object
fn date_modify_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let obj = args.first().unwrap_or(&Value::Null);
    let modifier = args.get(1).unwrap_or(&Value::Null).to_php_string().to_string_lossy();

    if let Value::Object(o) = obj {
        let ts = o.borrow().get_property(b"__timestamp").to_long();
        if let Some(new_ts) = apply_relative_modification(&modifier, ts) {
            o.borrow_mut().set_property(b"__timestamp".to_vec(), Value::Long(new_ts));
            Ok(obj.clone())
        } else {
            Ok(Value::False)
        }
    } else {
        Ok(Value::False)
    }
}

/// date_timestamp_get($object) - Get timestamp from DateTime
fn date_timestamp_get_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let obj = args.first().unwrap_or(&Value::Null);
    if let Value::Object(o) = obj {
        Ok(o.borrow().get_property(b"__timestamp"))
    } else {
        Ok(Value::False)
    }
}

/// date_timestamp_set($object, $timestamp) - Set timestamp on DateTime
fn date_timestamp_set_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let obj = args.first().unwrap_or(&Value::Null);
    let ts = args.get(1).unwrap_or(&Value::Null).to_long();
    if let Value::Object(o) = obj {
        o.borrow_mut().set_property(b"__timestamp".to_vec(), Value::Long(ts));
        Ok(obj.clone())
    } else {
        Ok(Value::False)
    }
}

/// date_diff($datetime1, $datetime2, $absolute = false)
fn date_diff_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let obj1 = args.first().unwrap_or(&Value::Null);
    let obj2 = args.get(1).unwrap_or(&Value::Null);
    let absolute = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);

    let ts1 = if let Value::Object(o) = obj1 {
        o.borrow().get_property(b"__timestamp").to_long()
    } else {
        return Ok(Value::False);
    };
    let ts2 = if let Value::Object(o) = obj2 {
        o.borrow().get_property(b"__timestamp").to_long()
    } else {
        return Ok(Value::False);
    };

    Ok(create_date_interval_from_diff(_vm, ts1, ts2, absolute))
}

/// Create a DateInterval from the difference between two timestamps
fn create_date_interval_from_diff(vm: &mut Vm, ts1: i64, ts2: i64, absolute: bool) -> Value {
    let diff = ts2 - ts1;
    let invert = if diff < 0 && !absolute { 1 } else { 0 };
    let abs_diff = diff.unsigned_abs() as i64;

    let days1 = ts1 / 86400;
    let days2 = ts2 / 86400;
    let (y1, m1, d1) = days_to_ymd(days1);
    let (y2, m2, d2) = days_to_ymd(days2);

    let time1 = ((ts1 % 86400) + 86400) % 86400;
    let time2 = ((ts2 % 86400) + 86400) % 86400;
    let h1 = time1 / 3600;
    let i1 = (time1 % 3600) / 60;
    let s1 = time1 % 60;
    let h2 = time2 / 3600;
    let i2 = (time2 % 3600) / 60;
    let s2 = time2 % 60;

    // Calculate calendar diff
    let (mut years, mut months, mut days_val, mut hours, mut minutes, mut seconds);
    if invert == 1 {
        // swap for calculation
        let (sy, sm, sd, sh, si, ss) = (y2, m2, d2, h2, i2, s2);
        let (ey, em, ed, eh, ei, es) = (y1, m1, d1, h1, i1, s1);
        let r = calc_calendar_diff(sy, sm as i64, sd as i64, sh, si, ss, ey, em as i64, ed as i64, eh, ei, es);
        years = r.0; months = r.1; days_val = r.2; hours = r.3; minutes = r.4; seconds = r.5;
    } else {
        let r = calc_calendar_diff(y1, m1 as i64, d1 as i64, h1, i1, s1, y2, m2 as i64, d2 as i64, h2, i2, s2);
        years = r.0; months = r.1; days_val = r.2; hours = r.3; minutes = r.4; seconds = r.5;
    }

    let total_days = abs_diff / 86400;

    let obj_id = vm.next_object_id();
    let mut obj = PhpObject::new(b"DateInterval".to_vec(), obj_id);
    obj.set_property(b"y".to_vec(), Value::Long(years));
    obj.set_property(b"m".to_vec(), Value::Long(months));
    obj.set_property(b"d".to_vec(), Value::Long(days_val));
    obj.set_property(b"h".to_vec(), Value::Long(hours));
    obj.set_property(b"i".to_vec(), Value::Long(minutes));
    obj.set_property(b"s".to_vec(), Value::Long(seconds));
    obj.set_property(b"f".to_vec(), Value::Double(0.0));
    obj.set_property(b"days".to_vec(), Value::Long(total_days));
    obj.set_property(b"invert".to_vec(), Value::Long(if absolute { 0 } else { invert }));

    Value::Object(Rc::new(RefCell::new(obj)))
}

fn calc_calendar_diff(sy: i64, sm: i64, sd: i64, sh: i64, si: i64, ss: i64,
                      ey: i64, em: i64, ed: i64, eh: i64, ei: i64, es: i64) -> (i64, i64, i64, i64, i64, i64) {
    let mut seconds = es - ss;
    let mut minutes = ei - si;
    let mut hours = eh - sh;
    let mut days_val = ed - sd;
    let mut months = em - sm;
    let mut years = ey - sy;

    if seconds < 0 { seconds += 60; minutes -= 1; }
    if minutes < 0 { minutes += 60; hours -= 1; }
    if hours < 0 { hours += 24; days_val -= 1; }
    if days_val < 0 {
        // Use the days in the month before the end month
        let prev_m = if em == 1 { 12 } else { em - 1 };
        let prev_y = if em == 1 { ey - 1 } else { ey };
        let dim = match prev_m as u32 {
            2 => if prev_y % 4 == 0 && (prev_y % 100 != 0 || prev_y % 400 == 0) { 29 } else { 28 },
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
        days_val += dim;
        months -= 1;
    }
    if months < 0 { months += 12; years -= 1; }

    (years, months, days_val, hours, minutes, seconds)
}

/// date_create_from_format($format, $datetime, $timezone = null)
fn date_create_from_format_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let format = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let datetime_str = args.get(1).unwrap_or(&Value::Null).to_php_string().to_string_lossy();

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let timestamp = parse_with_format(&format, &datetime_str, now_secs);

    match timestamp {
        Some(ts) => {
            let obj_id = _vm.next_object_id();
            let mut obj = PhpObject::new(b"DateTime".to_vec(), obj_id);
            obj.set_property(b"__timestamp".to_vec(), Value::Long(ts));
            Ok(Value::Object(Rc::new(RefCell::new(obj))))
        }
        None => Ok(Value::False),
    }
}

/// Parse a datetime string using a format string (like PHP's DateTime::createFromFormat)
fn parse_with_format(format: &str, datetime: &str, now: i64) -> Option<i64> {
    let now_days = now / 86400;
    let now_time = ((now % 86400) + 86400) % 86400;
    let (now_year, now_month, now_day) = days_to_ymd(now_days);

    let mut year = now_year;
    let mut month = now_month;
    let mut day = now_day;
    let mut hour = 0i64;
    let mut minute = 0i64;
    let mut second = 0i64;
    let mut has_time = false;

    let fmt_bytes = format.as_bytes();
    let dt_bytes = datetime.as_bytes();
    let mut fi = 0;
    let mut di = 0;

    while fi < fmt_bytes.len() && di <= dt_bytes.len() {
        let fc = fmt_bytes[fi];
        match fc {
            b'Y' => {
                // 4-digit year
                let end = (di + 4).min(dt_bytes.len());
                let s = std::str::from_utf8(&dt_bytes[di..end]).ok()?;
                year = s.parse().ok()?;
                di = end;
            }
            b'y' => {
                // 2-digit year
                let end = (di + 2).min(dt_bytes.len());
                let s = std::str::from_utf8(&dt_bytes[di..end]).ok()?;
                let y: i64 = s.parse().ok()?;
                year = if y >= 70 { 1900 + y } else { 2000 + y };
                di = end;
            }
            b'm' | b'n' => {
                let (val, consumed) = parse_num(&dt_bytes[di..], if fc == b'm' { 2 } else { 1 })?;
                month = val as u32;
                di += consumed;
            }
            b'd' | b'j' => {
                let (val, consumed) = parse_num(&dt_bytes[di..], if fc == b'd' { 2 } else { 1 })?;
                day = val as u32;
                di += consumed;
            }
            b'H' | b'G' => {
                let (val, consumed) = parse_num(&dt_bytes[di..], if fc == b'H' { 2 } else { 1 })?;
                hour = val;
                has_time = true;
                di += consumed;
            }
            b'h' | b'g' => {
                let (val, consumed) = parse_num(&dt_bytes[di..], if fc == b'h' { 2 } else { 1 })?;
                hour = val;
                has_time = true;
                di += consumed;
            }
            b'i' => {
                let (val, consumed) = parse_num(&dt_bytes[di..], 2)?;
                minute = val;
                di += consumed;
            }
            b's' => {
                let (val, consumed) = parse_num(&dt_bytes[di..], 2)?;
                second = val;
                di += consumed;
            }
            b'U' => {
                // Unix timestamp - consume all remaining digits
                let start = di;
                let neg = di < dt_bytes.len() && dt_bytes[di] == b'-';
                if neg { di += 1; }
                while di < dt_bytes.len() && dt_bytes[di].is_ascii_digit() {
                    di += 1;
                }
                let s = std::str::from_utf8(&dt_bytes[start..di]).ok()?;
                return s.parse().ok();
            }
            b'A' | b'a' => {
                // AM/PM
                if di + 2 <= dt_bytes.len() {
                    let ampm = std::str::from_utf8(&dt_bytes[di..di+2]).ok()?.to_lowercase();
                    if ampm == "pm" && hour < 12 {
                        hour += 12;
                    } else if ampm == "am" && hour == 12 {
                        hour = 0;
                    }
                    di += 2;
                }
            }
            b'u' | b'v' => {
                // Microseconds/milliseconds - skip digits
                while di < dt_bytes.len() && dt_bytes[di].is_ascii_digit() {
                    di += 1;
                }
            }
            b'e' | b'T' | b'O' | b'P' | b'p' => {
                // Timezone - skip
                while di < dt_bytes.len() && !dt_bytes[di].is_ascii_whitespace() {
                    di += 1;
                }
            }
            b'\\' => {
                // Escape next character
                fi += 1;
                if fi < fmt_bytes.len() && di < dt_bytes.len() {
                    di += 1;
                }
            }
            b' ' | b'-' | b'/' | b':' | b'.' | b'T' => {
                // Literal separator
                if di < dt_bytes.len() {
                    di += 1;
                }
            }
            b'!' => {
                // Reset all fields to Unix epoch
                year = 1970;
                month = 1;
                day = 1;
                hour = 0;
                minute = 0;
                second = 0;
            }
            b'|' => {
                // Reset fields that haven't been parsed yet
                if !has_time {
                    hour = 0;
                    minute = 0;
                    second = 0;
                }
            }
            _ => {
                // Skip literal character
                if di < dt_bytes.len() {
                    di += 1;
                }
            }
        }
        fi += 1;
    }

    let days = ymd_to_days(year, month, day);
    Some(days * 86400 + hour * 3600 + minute * 60 + second)
}

fn parse_num(bytes: &[u8], max_digits: usize) -> Option<(i64, usize)> {
    let mut i = 0;
    // Skip leading whitespace
    while i < bytes.len() && bytes[i] == b' ' {
        i += 1;
    }
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() && (i - start) < max_digits {
        i += 1;
    }
    if i == start {
        return None;
    }
    let s = std::str::from_utf8(&bytes[start..i]).ok()?;
    Some((s.parse().ok()?, i))
}

/// date_date_set($object, $year, $month, $day)
fn date_date_set_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let obj = args.first().unwrap_or(&Value::Null);
    let year = args.get(1).unwrap_or(&Value::Null).to_long();
    let month = args.get(2).unwrap_or(&Value::Null).to_long() as u32;
    let day = args.get(3).unwrap_or(&Value::Null).to_long() as u32;

    if let Value::Object(o) = obj {
        let ts = o.borrow().get_property(b"__timestamp").to_long();
        let time_of_day = ((ts % 86400) + 86400) % 86400;
        let new_days = ymd_to_days(year, month, day);
        let new_ts = new_days * 86400 + time_of_day;
        o.borrow_mut().set_property(b"__timestamp".to_vec(), Value::Long(new_ts));
        Ok(obj.clone())
    } else {
        Ok(Value::False)
    }
}

/// date_time_set($object, $hour, $minute, $second = 0, $microsecond = 0)
fn date_time_set_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let obj = args.first().unwrap_or(&Value::Null);
    let hour = args.get(1).unwrap_or(&Value::Null).to_long();
    let minute = args.get(2).unwrap_or(&Value::Null).to_long();
    let second = args.get(3).map(|v| v.to_long()).unwrap_or(0);

    if let Value::Object(o) = obj {
        let ts = o.borrow().get_property(b"__timestamp").to_long();
        let days = ts / 86400;
        let new_ts = days * 86400 + hour * 3600 + minute * 60 + second;
        o.borrow_mut().set_property(b"__timestamp".to_vec(), Value::Long(new_ts));
        Ok(obj.clone())
    } else {
        Ok(Value::False)
    }
}

/// date_interval_create_from_date_string($datetime) - Creates DateInterval from date string
fn date_interval_create_from_date_string_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let lower = s.to_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();

    let mut years = 0i64;
    let mut months = 0i64;
    let mut days = 0i64;
    let mut hours = 0i64;
    let mut minutes = 0i64;
    let mut seconds = 0i64;

    let mut i = 0;
    while i < tokens.len() {
        if let Ok(val) = tokens[i].parse::<i64>() {
            if i + 1 < tokens.len() {
                let unit = tokens[i + 1].trim_end_matches('s');
                match unit {
                    "year" => years = val,
                    "month" => months = val,
                    "day" => days = val,
                    "hour" => hours = val,
                    "minute" | "min" => minutes = val,
                    "second" | "sec" => seconds = val,
                    "week" => days += val * 7,
                    _ => {}
                }
                i += 2;
                continue;
            }
        }
        i += 1;
    }

    let obj_id = _vm.next_object_id();
    let mut obj = PhpObject::new(b"DateInterval".to_vec(), obj_id);
    obj.set_property(b"y".to_vec(), Value::Long(years));
    obj.set_property(b"m".to_vec(), Value::Long(months));
    obj.set_property(b"d".to_vec(), Value::Long(days));
    obj.set_property(b"h".to_vec(), Value::Long(hours));
    obj.set_property(b"i".to_vec(), Value::Long(minutes));
    obj.set_property(b"s".to_vec(), Value::Long(seconds));
    obj.set_property(b"f".to_vec(), Value::Double(0.0));
    obj.set_property(b"days".to_vec(), Value::False);
    obj.set_property(b"invert".to_vec(), Value::Long(0));

    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

/// timezone_open() - alias for new DateTimeZone($timezone)
fn timezone_open_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let tz_str = args.first().unwrap_or(&Value::Null).to_php_string();
    let tz_bytes = tz_str.as_bytes();
    let tz_name = String::from_utf8_lossy(tz_bytes);

    // Basic validation - check if it resolves to a known timezone or offset
    let (offset, _) = timezone_offset_and_abbrev(&tz_name, 0);
    let is_known = offset != 0 || tz_name == "UTC" || tz_name == "utc"
        || tz_name == "GMT" || tz_name.starts_with('+') || tz_name.starts_with('-')
        || tz_name.contains('/');

    if !is_known && tz_bytes.is_empty() {
        vm.emit_warning_at("timezone_open(): Unknown or bad timezone ()", vm.current_line);
        return Ok(Value::False);
    }

    let obj_id = vm.next_object_id();
    let mut obj = PhpObject::new(b"DateTimeZone".to_vec(), obj_id);
    obj.set_property(b"timezone_type".to_vec(), Value::Long(3));
    obj.set_property(
        b"timezone".to_vec(),
        Value::String(PhpString::from_vec(tz_bytes.to_vec())),
    );
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

/// date_timezone_get() - Return the timezone of a DateTime object
fn date_timezone_get_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let dt_obj = args.first().unwrap_or(&Value::Null);
    if let Value::Object(obj) = dt_obj {
        let obj_borrow = obj.borrow();
        let tz_prop = obj_borrow.get_property(b"timezone");
        if let Value::String(tz_str) = &tz_prop {
            let tz_bytes = tz_str.as_bytes().to_vec();
            drop(obj_borrow);
            let obj_id = vm.next_object_id();
            let mut tz_obj = PhpObject::new(b"DateTimeZone".to_vec(), obj_id);
            tz_obj.set_property(b"timezone_type".to_vec(), Value::Long(3));
            tz_obj.set_property(b"timezone".to_vec(), Value::String(PhpString::from_vec(tz_bytes)));
            return Ok(Value::Object(Rc::new(RefCell::new(tz_obj))));
        }
        drop(obj_borrow);
    }
    Ok(Value::False)
}

/// date_timezone_set() - Set the timezone for a DateTime object
fn date_timezone_set_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let dt_obj = args.first().unwrap_or(&Value::Null);
    let tz_obj = args.get(1).unwrap_or(&Value::Null);
    if let (Value::Object(dt), Value::Object(tz)) = (dt_obj, tz_obj) {
        let tz_name = {
            let tz_borrow = tz.borrow();
            if let Value::String(s) = tz_borrow.get_property(b"timezone") {
                s.as_bytes().to_vec()
            } else {
                b"UTC".to_vec()
            }
        };
        let mut dt_borrow = dt.borrow_mut();
        dt_borrow.set_property(b"timezone".to_vec(), Value::String(PhpString::from_vec(tz_name)));
        drop(dt_borrow);
        return Ok(dt_obj.clone());
    }
    Ok(Value::False)
}

/// gettimeofday() - Get current time
fn gettimeofday_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let as_float = args.first().map(|v| v.is_truthy()).unwrap_or(false);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();

    if as_float {
        Ok(Value::Double(now.as_secs_f64()))
    } else {
        let mut arr = goro_core::array::PhpArray::new();
        let secs = now.as_secs() as i64;
        let usecs = now.subsec_micros() as i64;
        arr.set(goro_core::array::ArrayKey::String(PhpString::from_bytes(b"sec")), Value::Long(secs));
        arr.set(goro_core::array::ArrayKey::String(PhpString::from_bytes(b"usec")), Value::Long(usecs));
        arr.set(goro_core::array::ArrayKey::String(PhpString::from_bytes(b"minuteswest")), Value::Long(0));
        arr.set(goro_core::array::ArrayKey::String(PhpString::from_bytes(b"dsttime")), Value::Long(0));
        Ok(Value::Array(Rc::new(RefCell::new(arr))))
    }
}

/// date_isodate_set($object, $year, $week, $dayOfWeek = 1)
fn date_isodate_set_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let obj = args.first().unwrap_or(&Value::Null);
    let year = args.get(1).unwrap_or(&Value::Null).to_long();
    let week = args.get(2).unwrap_or(&Value::Null).to_long();
    let day_of_week = args.get(3).map(|v| v.to_long()).unwrap_or(1);

    if let Value::Object(o) = obj {
        let ts = o.borrow().get_property(b"__timestamp").to_long();
        let tod = ((ts % 86400) + 86400) % 86400;
        // ISO week 1 contains January 4th
        let jan4 = ymd_to_days(year, 1, 4);
        let jan4_dow = (((jan4 % 7) + 4) % 7 + 7) % 7; // 0=Sunday
        let jan4_iso_dow = if jan4_dow == 0 { 7 } else { jan4_dow };
        let week1_monday = jan4 - (jan4_iso_dow - 1);
        let target_days = week1_monday + (week - 1) * 7 + (day_of_week - 1);
        let new_ts = target_days * 86400 + tod;
        o.borrow_mut().set_property(b"__timestamp".to_vec(), Value::Long(new_ts));
        Ok(obj.clone())
    } else {
        Ok(Value::False)
    }
}

/// timezone_abbreviations_list()
fn timezone_abbreviations_list_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // Return a simplified version of timezone abbreviations
    let mut result = PhpArray::new();

    let abbrevs = [
        ("acst", 34200, 0, "Australia/Adelaide"),
        ("aest", 36000, 0, "Australia/Brisbane"),
        ("akst", -32400, 0, "America/Anchorage"),
        ("ast", -14400, 0, "America/Halifax"),
        ("brt", -10800, 0, "America/Sao_Paulo"),
        ("cdt", -18000, 1, "America/Chicago"),
        ("cest", 7200, 1, "Europe/Berlin"),
        ("cet", 3600, 0, "Europe/Berlin"),
        ("cst", -21600, 0, "America/Chicago"),
        ("eat", 10800, 0, "Africa/Nairobi"),
        ("edt", -14400, 1, "America/New_York"),
        ("eest", 10800, 1, "Europe/Helsinki"),
        ("eet", 7200, 0, "Europe/Helsinki"),
        ("est", -18000, 0, "America/New_York"),
        ("gmt", 0, 0, "UTC"),
        ("hst", -36000, 0, "Pacific/Honolulu"),
        ("ist", 19800, 0, "Asia/Kolkata"),
        ("jst", 32400, 0, "Asia/Tokyo"),
        ("kst", 32400, 0, "Asia/Seoul"),
        ("mdt", -21600, 1, "America/Denver"),
        ("msk", 10800, 0, "Europe/Moscow"),
        ("mst", -25200, 0, "America/Denver"),
        ("nzdt", 46800, 1, "Pacific/Auckland"),
        ("nzst", 43200, 0, "Pacific/Auckland"),
        ("pdt", -25200, 1, "America/Los_Angeles"),
        ("pst", -28800, 0, "America/Los_Angeles"),
        ("sast", 7200, 0, "Africa/Johannesburg"),
        ("utc", 0, 0, "UTC"),
        ("wat", 3600, 0, "Africa/Lagos"),
        ("wet", 0, 0, "Europe/London"),
    ];

    for (abbr, offset, dst, tz_id) in &abbrevs {
        let mut entry = PhpArray::new();
        entry.set(ArrayKey::String(PhpString::from_bytes(b"dst")), if *dst != 0 { Value::True } else { Value::False });
        entry.set(ArrayKey::String(PhpString::from_bytes(b"offset")), Value::Long(*offset));
        entry.set(ArrayKey::String(PhpString::from_bytes(b"timezone_id")), Value::String(PhpString::from_string(tz_id.to_string())));

        let mut arr = PhpArray::new();
        arr.push(Value::Array(Rc::new(RefCell::new(entry))));

        result.set(ArrayKey::String(PhpString::from_string(abbr.to_string())), Value::Array(Rc::new(RefCell::new(arr))));
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

/// timezone_name_from_abbr($abbr, $utcOffset = -1, $isDST = -1)
fn timezone_name_from_abbr_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let abbr = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy().to_lowercase();
    let _utc_offset = args.get(1).map(|v| v.to_long()).unwrap_or(-1);
    let _is_dst = args.get(2).map(|v| v.to_long()).unwrap_or(-1);

    let tz = match abbr.as_str() {
        "est" => "America/New_York",
        "edt" => "America/New_York",
        "cst" => "America/Chicago",
        "cdt" => "America/Chicago",
        "mst" => "America/Denver",
        "mdt" => "America/Denver",
        "pst" => "America/Los_Angeles",
        "pdt" => "America/Los_Angeles",
        "gmt" => "Europe/London",
        "utc" => "UTC",
        "cet" => "Europe/Berlin",
        "cest" => "Europe/Berlin",
        "eet" => "Europe/Helsinki",
        "eest" => "Europe/Helsinki",
        "msk" => "Europe/Moscow",
        "jst" => "Asia/Tokyo",
        "kst" => "Asia/Seoul",
        "ist" => "Asia/Kolkata",
        "aest" => "Australia/Sydney",
        "acst" => "Australia/Adelaide",
        "nzst" => "Pacific/Auckland",
        "nzdt" => "Pacific/Auckland",
        "hst" => "Pacific/Honolulu",
        "akst" => "America/Anchorage",
        "brt" => "America/Sao_Paulo",
        "sast" => "Africa/Johannesburg",
        "eat" => "Africa/Nairobi",
        "wat" => "Africa/Lagos",
        "wet" => "Europe/London",
        _ => return Ok(Value::False),
    };
    Ok(Value::String(PhpString::from_string(tz.to_string())))
}

/// timezone_offset_get($timezone, $datetime)
fn timezone_offset_get_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let tz_obj = args.first().unwrap_or(&Value::Null);
    let dt_obj = args.get(1).unwrap_or(&Value::Null);

    let tz_name = if let Value::Object(o) = tz_obj {
        let ob = o.borrow();
        let tz = ob.get_property(b"timezone").to_php_string().to_string_lossy();
        if tz.is_empty() { "UTC".to_string() } else { tz }
    } else {
        return Ok(Value::False);
    };

    let ts = if let Value::Object(o) = dt_obj {
        o.borrow().get_property(b"__timestamp").to_long()
    } else {
        0
    };

    let (offset, _) = timezone_offset_and_abbrev(&tz_name, ts);
    Ok(Value::Long(offset))
}

/// timezone_identifiers_list()
fn timezone_identifiers_list_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    let timezones = [
        "Africa/Abidjan", "Africa/Accra", "Africa/Addis_Ababa", "Africa/Algiers",
        "Africa/Cairo", "Africa/Casablanca", "Africa/Johannesburg", "Africa/Lagos", "Africa/Nairobi",
        "America/Anchorage", "America/Argentina/Buenos_Aires", "America/Chicago",
        "America/Denver", "America/Halifax", "America/Los_Angeles", "America/New_York",
        "America/Phoenix", "America/Sao_Paulo", "America/Toronto", "America/Vancouver",
        "Asia/Bangkok", "Asia/Calcutta", "Asia/Dhaka", "Asia/Dubai", "Asia/Hong_Kong",
        "Asia/Jakarta", "Asia/Karachi", "Asia/Kolkata", "Asia/Seoul", "Asia/Shanghai",
        "Asia/Singapore", "Asia/Taipei", "Asia/Tokyo",
        "Atlantic/Reykjavik",
        "Australia/Adelaide", "Australia/Brisbane", "Australia/Darwin",
        "Australia/Hobart", "Australia/Melbourne", "Australia/Perth", "Australia/Sydney",
        "Europe/Amsterdam", "Europe/Athens", "Europe/Berlin", "Europe/Brussels",
        "Europe/Budapest", "Europe/Copenhagen", "Europe/Helsinki", "Europe/Istanbul",
        "Europe/Kiev", "Europe/London", "Europe/Madrid", "Europe/Moscow",
        "Europe/Oslo", "Europe/Paris", "Europe/Prague", "Europe/Rome",
        "Europe/Stockholm", "Europe/Vienna", "Europe/Warsaw", "Europe/Zurich",
        "Pacific/Auckland", "Pacific/Fiji", "Pacific/Honolulu",
        "UTC",
    ];
    for tz in &timezones {
        arr.push(Value::String(PhpString::from_string(tz.to_string())));
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

/// timezone_name_get($timezone)
fn timezone_name_get_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let tz_obj = args.first().unwrap_or(&Value::Null);
    if let Value::Object(o) = tz_obj {
        let ob = o.borrow();
        let tz = ob.get_property(b"timezone");
        if matches!(tz, Value::Null) {
            Ok(Value::String(PhpString::from_bytes(b"UTC")))
        } else {
            Ok(tz)
        }
    } else {
        Ok(Value::False)
    }
}

/// timezone_version_get()
fn timezone_version_get_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"2024.1")))
}

/// date_offset_get($datetime)
fn date_offset_get_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let dt_obj = args.first().unwrap_or(&Value::Null);
    if let Value::Object(o) = dt_obj {
        let ob = o.borrow();
        let ts = ob.get_property(b"__timestamp").to_long();
        let tz = ob.get_property(b"timezone").to_php_string().to_string_lossy();
        let tz = if tz.is_empty() { "UTC".to_string() } else { tz };
        drop(ob);
        let (offset, _) = timezone_offset_and_abbrev(&tz, ts);
        Ok(Value::Long(offset))
    } else {
        Ok(Value::False)
    }
}

/// date_sun_info($timestamp, $latitude, $longitude) - stub
fn date_sun_info_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    arr.set(ArrayKey::String(PhpString::from_bytes(b"sunrise")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"sunset")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"transit")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"civil_twilight_begin")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"civil_twilight_end")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"nautical_twilight_begin")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"nautical_twilight_end")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"astronomical_twilight_begin")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"astronomical_twilight_end")), Value::Long(0));
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

/// date_sunrise - stub (deprecated)
fn date_sunrise_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    _vm.emit_deprecated("Function date_sunrise() is deprecated since 8.1");
    Ok(Value::False)
}

/// date_sunset - stub (deprecated)
fn date_sunset_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    _vm.emit_deprecated("Function date_sunset() is deprecated since 8.1");
    Ok(Value::False)
}
