use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

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
    vm.register_function(b"strtotime", strtotime);
    vm.register_function(b"date_create", date_create_fn);
    vm.register_function(b"getdate", getdate_fn);
    vm.register_function(b"localtime", localtime_fn);
    vm.register_function(b"checkdate", checkdate_fn);
    vm.register_function(b"idate", idate_fn);
}

fn date_default_timezone_set(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}

fn date_default_timezone_get(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"UTC")))
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

    // Simple date formatting - compute components from unix timestamp
    // This is a simplified version, not handling timezones properly
    let days_since_epoch = secs / 86400;
    let time_of_day = ((secs % 86400) + 86400) % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Compute year/month/day from days since epoch (1970-01-01)
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
            b'U' => result.push_str(&format!("{}", secs)),
            b'N' => {
                let dow = ((days_since_epoch % 7) + 4) % 7; // Monday=1
                result.push_str(&format!("{}", if dow == 0 { 7 } else { dow }));
            }
            b'w' => {
                let dow = ((days_since_epoch % 7) + 4) % 7;
                result.push_str(&format!("{}", dow));
            }
            b'g' => {
                let h12 = if hours == 0 {
                    12
                } else if hours > 12 {
                    hours - 12
                } else {
                    hours
                };
                result.push_str(&format!("{}", h12));
            }
            b'A' => result.push_str(if hours < 12 { "AM" } else { "PM" }),
            b'a' => result.push_str(if hours < 12 { "am" } else { "pm" }),
            b't' => {
                let days_in_month = match month {
                    2 => {
                        if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                            29
                        } else {
                            28
                        }
                    }
                    4 | 6 | 9 | 11 => 30,
                    _ => 31,
                };
                result.push_str(&format!("{}", days_in_month));
            }
            b'L' => {
                let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
                result.push(if leap { '1' } else { '0' });
            }
            _ => result.push(c as char),
        }
        i += 1;
    }

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
fn strtotime(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn gmdate_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // For now, same as date_fn since we don't handle timezones
    date_fn(_vm, args)
}

/// gmmktime - UTC version of mktime
fn gmmktime_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // For now, same as mktime since we don't handle timezones
    mktime(_vm, args)
}

/// strftime - format a timestamp using strftime-style format codes
fn strftime_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
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

/// date_create - create a DateTime-like value (returns stdClass with timestamp property)
fn date_create_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let datetime_str = args
        .first()
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();

    // Get current timestamp as default
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let timestamp = if datetime_str.is_empty() || datetime_str == "now" {
        now_secs
    } else {
        // Very basic parsing: try "Y-m-d H:i:s" or "Y-m-d"
        let parts: Vec<&str> = datetime_str.split(|c: char| c == ' ' || c == 'T').collect();
        let date_parts: Vec<&str> = parts.first().unwrap_or(&"").split('-').collect();
        if date_parts.len() == 3 {
            let year = date_parts[0].parse::<i64>().unwrap_or(1970);
            let month = date_parts[1].parse::<u32>().unwrap_or(1);
            let day = date_parts[2].parse::<u32>().unwrap_or(1);
            let mut h = 0i64;
            let mut m = 0i64;
            let mut s = 0i64;
            if let Some(time_str) = parts.get(1) {
                let time_parts: Vec<&str> = time_str.split(':').collect();
                h = time_parts.first().and_then(|v| v.parse().ok()).unwrap_or(0);
                m = time_parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
                s = time_parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            let days = ymd_to_days(year, month, day);
            days * 86400 + h * 3600 + m * 60 + s
        } else {
            now_secs
        }
    };

    // Return a stdClass-like object with a timestamp property
    let obj_id = _vm.next_object_id();
    let mut obj = PhpObject::new(b"stdClass".to_vec(), obj_id);
    obj.set_property(b"timestamp".to_vec(), Value::Long(timestamp));
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
            // ISO week number
            let iso_dow = if dow == 0 { 7 } else { dow }; // Monday=1
            let jan1_days = ymd_to_days(year, 1, 1);
            let jan1_dow = (((jan1_days % 7) + 4) % 7 + 7) % 7;
            let jan1_iso_dow = if jan1_dow == 0 { 7 } else { jan1_dow };
            let iso_week_one_start = jan1_days - (jan1_iso_dow - 1) + if jan1_iso_dow <= 4 { 0 } else { 7 };
            let current_days = ymd_to_days(year, month, day);
            let diff = current_days - iso_week_one_start;
            if diff < 0 {
                // In last week of previous year - compute that year's last week
                52 // simplified
            } else {
                (diff / 7 + 1) as i64
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
