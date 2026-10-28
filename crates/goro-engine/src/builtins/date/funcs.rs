//! The procedural date functions (PHP manual: Date/Time Functions).

use goro_ext_date::parse::DateParse;
use goro_ext_date::sun::{self, SunResult, SunTime};
use goro_ext_date::{TimeZone, api, resolve};
use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::classes::*;
use super::*;

pub(super) const FUNCTIONS: &[(&str, crate::func::NativeFn)] = &[
    (
        "function strtotime(string $datetime, ?int $baseTimestamp = null): int|false",
        strtotime,
    ),
    (
        "function date(string $format, ?int $timestamp = null): string",
        date,
    ),
    (
        "function idate(string $format, ?int $timestamp = null): int|false",
        idate,
    ),
    (
        "function gmdate(string $format, ?int $timestamp = null): string",
        gmdate,
    ),
    (
        "function mktime(int $hour, ?int $minute = null, ?int $second = null, ?int $month = null, ?int $day = null, ?int $year = null): int|false",
        mktime,
    ),
    (
        "function gmmktime(int $hour, ?int $minute = null, ?int $second = null, ?int $month = null, ?int $day = null, ?int $year = null): int|false",
        gmmktime,
    ),
    (
        "function checkdate(int $month, int $day, int $year): bool",
        checkdate,
    ),
    (
        "function strftime(string $format, ?int $timestamp = null): string|false",
        strftime,
    ),
    (
        "function gmstrftime(string $format, ?int $timestamp = null): string|false",
        gmstrftime,
    ),
    ("function time(): int", time),
    (
        "function localtime(?int $timestamp = null, bool $associative = false): array",
        localtime,
    ),
    ("function getdate(?int $timestamp = null): array", getdate),
    (
        "function date_create(string $datetime = \"now\", ?DateTimeZone $timezone = null): DateTime|false",
        date_create,
    ),
    (
        "function date_create_immutable(string $datetime = \"now\", ?DateTimeZone $timezone = null): DateTimeImmutable|false",
        date_create_immutable,
    ),
    (
        "function date_create_from_format(string $format, string $datetime, ?DateTimeZone $timezone = null): DateTime|false",
        dt_create_from_format,
    ),
    (
        "function date_create_immutable_from_format(string $format, string $datetime, ?DateTimeZone $timezone = null): DateTimeImmutable|false",
        dti_create_from_format,
    ),
    ("function date_parse(string $datetime): array", date_parse),
    (
        "function date_parse_from_format(string $format, string $datetime): array",
        date_parse_from_format,
    ),
    (
        "function date_get_last_errors(): array|false",
        get_last_errors,
    ),
    (
        "function date_format(DateTimeInterface $object, string $format): string",
        dt_format,
    ),
    (
        "function date_modify(DateTime $object, string $modifier): DateTime|false",
        dt_modify,
    ),
    (
        "function date_add(DateTime $object, DateInterval $interval): DateTime",
        dt_add,
    ),
    (
        "function date_sub(DateTime $object, DateInterval $interval): DateTime",
        dt_sub,
    ),
    (
        "function date_timezone_get(DateTimeInterface $object): DateTimeZone|false",
        dt_get_timezone,
    ),
    (
        "function date_timezone_set(DateTime $object, DateTimeZone $timezone): DateTime",
        dt_set_timezone,
    ),
    (
        "function date_offset_get(DateTimeInterface $object): int",
        dt_get_offset,
    ),
    (
        "function date_diff(DateTimeInterface $baseObject, DateTimeInterface $targetObject, bool $absolute = false): DateInterval",
        dt_diff,
    ),
    (
        "function date_time_set(DateTime $object, int $hour, int $minute, int $second = 0, int $microsecond = 0): DateTime",
        dt_set_time,
    ),
    (
        "function date_date_set(DateTime $object, int $year, int $month, int $day): DateTime",
        dt_set_date,
    ),
    (
        "function date_isodate_set(DateTime $object, int $year, int $week, int $dayOfWeek = 1): DateTime",
        dt_set_isodate,
    ),
    (
        "function date_timestamp_set(DateTime $object, int $timestamp): DateTime",
        dt_set_timestamp,
    ),
    (
        "function date_timestamp_get(DateTimeInterface $object): int",
        dt_get_timestamp,
    ),
    (
        "function timezone_open(string $timezone): DateTimeZone|false",
        timezone_open,
    ),
    (
        "function timezone_name_get(DateTimeZone $object): string",
        tz_get_name,
    ),
    (
        "function timezone_name_from_abbr(string $abbr, int $utcOffset = -1, int $isDST = -1): string|false",
        timezone_name_from_abbr,
    ),
    (
        "function timezone_offset_get(DateTimeZone $object, DateTimeInterface $datetime): int",
        tz_get_offset,
    ),
    (
        "function timezone_transitions_get(DateTimeZone $object, int $timestampBegin = PHP_INT_MIN, int $timestampEnd = 2147483647): array|false",
        tz_get_transitions,
    ),
    (
        "function timezone_location_get(DateTimeZone $object): array|false",
        tz_get_location,
    ),
    (
        "function timezone_identifiers_list(int $timezoneGroup = DateTimeZone::ALL, ?string $countryCode = null): array",
        tz_list_identifiers,
    ),
    (
        "function timezone_abbreviations_list(): array",
        tz_list_abbreviations,
    ),
    (
        "function timezone_version_get(): string",
        timezone_version_get,
    ),
    (
        "function date_interval_create_from_date_string(string $datetime): DateInterval|false",
        iv_create_from_date_string,
    ),
    (
        "function date_interval_format(DateInterval $object, string $format): string",
        iv_format,
    ),
    (
        "function date_default_timezone_set(string $timezoneId): bool",
        date_default_timezone_set,
    ),
    (
        "function date_default_timezone_get(): string",
        date_default_timezone_get,
    ),
    (
        "function date_sunrise(int $timestamp, int $returnFormat = SUNFUNCS_RET_STRING, ?float $latitude = null, ?float $longitude = null, ?float $zenith = null, ?float $utcOffset = null): string|int|float|false",
        date_sunrise,
    ),
    (
        "function date_sunset(int $timestamp, int $returnFormat = SUNFUNCS_RET_STRING, ?float $latitude = null, ?float $longitude = null, ?float $zenith = null, ?float $utcOffset = null): string|int|float|false",
        date_sunset,
    ),
    (
        "function date_sun_info(int $timestamp, float $latitude, float $longitude): array",
        date_sun_info,
    ),
];

fn ts_arg(a: &Args, i: usize) -> i64 {
    a.opt_int(i).unwrap_or_else(|| now().0)
}

fn strtotime(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let input = a.str(0);
    let base = ts_arg(a, 1);
    let zone = default_tz(vm);
    Ok(match resolve::strtotime(&input, base, &zone) {
        Some(t) => Value::Int(t),
        None => Value::Bool(false),
    })
}

fn date(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let fmt = a.str(0);
    let ts = ts_arg(a, 1);
    let zone = default_tz(vm);
    Ok(s(&goro_ext_date::time::date(&fmt, ts, &zone)))
}

/// gmdate(): UTC, with "GMT" as the abbreviation (observed on PHP 8.5:
/// `gmdate("e T")` is "UTC GMT").
pub(super) fn gmt_format(fmt: &[u8], ts: i64, us: u32) -> Vec<u8> {
    let zone = TimeZone::utc();
    let t = goro_ext_date::LocalTime::new(ts, us, &zone);
    let mut out = Vec::with_capacity(fmt.len() * 2);
    let mut start = 0;
    let mut i = 0;
    while i < fmt.len() {
        match fmt[i] {
            b'\\' => i += 2,
            b'T' => {
                goro_ext_date::format::format_into(&mut out, &fmt[start..i], &t, &zone);
                out.extend_from_slice(b"GMT");
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < fmt.len() {
        goro_ext_date::format::format_into(&mut out, &fmt[start..], &t, &zone);
    }
    out
}

fn gmdate(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let fmt = a.str(0);
    let ts = ts_arg(a, 1);
    Ok(s(&gmt_format(&fmt, ts, 0)))
}

fn idate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let fmt = a.str(0);
    let ts = ts_arg(a, 1);
    let zone = default_tz(vm);
    match api::idate(&fmt, ts, &zone) {
        Ok(n) => Ok(Value::Int(n)),
        Err(e) => {
            vm.fwarn(a, e.message())?;
            Ok(Value::Bool(false))
        }
    }
}

fn mktime_in(vm: &mut Vm, a: &Args, zone: &TimeZone) -> R<Value> {
    let _ = vm;
    let (now, _) = now();
    let t = goro_ext_date::LocalTime::new(now, 0, zone);
    let h = a.int(0);
    let i = a.opt_int(1).unwrap_or(t.i as i64);
    let sec = a.opt_int(2).unwrap_or(t.s as i64);
    let m = a.opt_int(3).unwrap_or(t.m as i64);
    let d = a.opt_int(4).unwrap_or(t.d as i64);
    let y = a.opt_int(5).unwrap_or(t.y);
    Ok(Value::Int(api::mktime(h, i, sec, m, d, y, zone)))
}

fn mktime(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let zone = default_tz(vm);
    mktime_in(vm, a, &zone)
}

fn gmmktime(vm: &mut Vm, a: &mut Args) -> R<Value> {
    mktime_in(vm, a, &TimeZone::Offset(0))
}

fn checkdate(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(api::checkdate(a.int(0), a.int(1), a.int(2))))
}

fn strftime_impl(vm: &mut Vm, a: &Args, gmt: bool) -> R<Value> {
    vm.deprecated(&format!(
        "Function {}() is deprecated since 8.1, use IntlDateFormatter::format() instead",
        if gmt { "gmstrftime" } else { "strftime" }
    ))?;
    let fmt = a.str(0);
    if fmt.is_empty() {
        return Ok(Value::Bool(false));
    }
    let ts = ts_arg(a, 1);
    let zone = if gmt { TimeZone::utc() } else { default_tz(vm) };
    Ok(s(&api::strftime(&fmt, ts, &zone, gmt)))
}

fn strftime(vm: &mut Vm, a: &mut Args) -> R<Value> {
    strftime_impl(vm, a, false)
}

fn gmstrftime(vm: &mut Vm, a: &mut Args) -> R<Value> {
    strftime_impl(vm, a, true)
}

fn time(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(now().0))
}

fn localtime(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let ts = ts_arg(a, 0);
    let assoc = a.bool_or(1, false);
    let zone = default_tz(vm);
    let v = api::localtime(ts, &zone);
    let mut out = PhpArray::new();
    for (k, n) in api::LOCALTIME_KEYS.iter().zip(v) {
        if assoc {
            out.insert(key(k), Value::Int(n));
        } else {
            let _ = out.push(Value::Int(n));
        }
    }
    Ok(Value::Array(out))
}

fn getdate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let ts = ts_arg(a, 0);
    let zone = default_tz(vm);
    let g = api::getdate(ts, &zone);
    let mut out = PhpArray::new();
    out.insert(key("seconds"), Value::Int(g.seconds));
    out.insert(key("minutes"), Value::Int(g.minutes));
    out.insert(key("hours"), Value::Int(g.hours));
    out.insert(key("mday"), Value::Int(g.mday));
    out.insert(key("wday"), Value::Int(g.wday));
    out.insert(key("mon"), Value::Int(g.mon));
    out.insert(key("year"), Value::Int(g.year));
    out.insert(key("yday"), Value::Int(g.yday));
    out.insert(key("weekday"), sv(g.weekday));
    out.insert(key("month"), sv(g.month));
    out.insert(Key::Int(0), Value::Int(g.ts));
    Ok(Value::Array(out))
}

fn date_create_impl(vm: &mut Vm, a: &Args, cls: &str) -> R<Value> {
    let input = if a.has(0) {
        a.str(0)
    } else {
        PhpStr::from("now")
    };
    let zone = zone_arg(vm, a, 1)?;
    match parse_create(vm, &input, &zone) {
        Ok(dt) => {
            let o = new_date_named(vm, cls, dt)?;
            Ok(Value::Object(o))
        }
        Err(_) => Ok(Value::Bool(false)),
    }
}

fn date_create(vm: &mut Vm, a: &mut Args) -> R<Value> {
    date_create_impl(vm, a, "DateTime")
}

fn date_create_immutable(vm: &mut Vm, a: &mut Args) -> R<Value> {
    date_create_impl(vm, a, "DateTimeImmutable")
}

/// The date_parse() / date_parse_from_format() array.
fn date_parse_value(p: &DateParse) -> Value {
    let mut out = PhpArray::new();
    let num = |v: Option<i64>| match v {
        Some(n) => Value::Int(n),
        None => Value::Bool(false),
    };
    out.insert(key("year"), num(p.year));
    out.insert(key("month"), num(p.month));
    out.insert(key("day"), num(p.day));
    out.insert(key("hour"), num(p.hour));
    out.insert(key("minute"), num(p.minute));
    out.insert(key("second"), num(p.second));
    out.insert(
        key("fraction"),
        match p.fraction_us {
            Some(us) => Value::Float(us as f64 / 1_000_000.0),
            None => Value::Bool(false),
        },
    );
    let e = LastErrors {
        warnings: p.warnings.clone(),
        errors: p.errors.clone(),
    };
    if let Value::Array(errs) = last_errors_value(&e) {
        for (k, v) in errs.iter() {
            out.insert(k.to_owned(), v.clone());
        }
    }
    out.insert(key("is_localtime"), Value::Bool(p.is_localtime));
    if p.is_localtime {
        if let Some(t) = p.zone_type {
            out.insert(key("zone_type"), Value::Int(t as i64));
        }
        if let Some(z) = p.zone {
            out.insert(key("zone"), Value::Int(z));
        }
        if let Some(d) = p.is_dst {
            out.insert(key("is_dst"), Value::Bool(d));
        }
        if let Some(t) = &p.tz_abbr {
            out.insert(key("tz_abbr"), sv(t));
        }
        if let Some(t) = &p.tz_id {
            out.insert(key("tz_id"), sv(t));
        }
    }
    if let Some(r) = &p.relative {
        let mut rel = PhpArray::new();
        for (k, v) in ["year", "month", "day", "hour", "minute", "second"]
            .iter()
            .zip(r.ymdhis)
        {
            rel.insert(key(k), Value::Int(v));
        }
        if let Some(w) = r.weekday {
            rel.insert(key("weekday"), Value::Int(w));
        }
        if let Some(w) = r.weekdays {
            rel.insert(key("weekdays"), Value::Int(w));
        }
        if r.first_day_of_month {
            rel.insert(key("first_day_of_month"), Value::Bool(true));
        }
        if r.last_day_of_month {
            rel.insert(key("last_day_of_month"), Value::Bool(true));
        }
        out.insert(key("relative"), Value::Array(rel));
    }
    Value::Array(out)
}

fn date_parse(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p = goro_ext_date::parse::parse(&a.str(0));
    Ok(date_parse_value(&p.date_parse()))
}

fn date_parse_from_format(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p = goro_ext_date::from_format::parse_from_format(&a.str(0), &a.str(1));
    Ok(date_parse_value(&p.date_parse()))
}

pub(super) fn get_last_errors(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(last_errors(vm))
}

fn timezone_open(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let name = a.str(0);
    match parse_zone(&name) {
        Ok(z) => Ok(Value::Object(new_tz(vm, z)?)),
        Err(msg) => {
            vm.fwarn(a, &msg)?;
            Ok(Value::Bool(false))
        }
    }
}

fn timezone_name_from_abbr(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let abbr = a.str(0).to_string_lossy();
    Ok(
        match goro_ext_date::tz::abbr::name_from_abbr(&abbr, a.int_or(1, -1), a.int_or(2, -1)) {
            Some(n) => sv(n),
            None => Value::Bool(false),
        },
    )
}

fn timezone_version_get(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(sv(&goro_ext_date::tz::db::php_version()))
}

fn date_default_timezone_set(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let name = a.str(0).to_string_lossy();
    match api::default_timezone(&name) {
        Some(z) => {
            set_default_tz(vm, z);
            Ok(Value::Bool(true))
        }
        None => {
            vm.fnotice(a, &api::invalid_default_zone_notice(&name))?;
            Ok(Value::Bool(false))
        }
    }
}

fn date_default_timezone_get(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(sv(&default_tz(vm).name()))
}

// ---------------------------------------------------------------- sun

fn ini_float(vm: &mut Vm, k: &[u8], d: f64) -> f64 {
    match vm.get_ini(k) {
        Some(v) => goro_runtime::convert::str_to_float(&v),
        None => d,
    }
}

fn sun_impl(vm: &mut Vm, a: &Args, which: sun::RiseSet) -> R<Value> {
    vm.deprecated(match which {
        sun::RiseSet::Sunrise => sun::DEPRECATED_SUNRISE,
        sun::RiseSet::Sunset => sun::DEPRECATED_SUNSET,
    })?;
    let ts = a.int(0);
    let format = a.int_or(1, 1);
    let opt = |i: usize| match a.get(i) {
        None | Some(Value::Null) => None,
        Some(_) => Some(a.float(i)),
    };
    let lat = match opt(2) {
        Some(v) => v,
        None => ini_float(vm, b"date.default_latitude", sun::DEFAULT_LATITUDE),
    };
    let lon = match opt(3) {
        Some(v) => v,
        None => ini_float(vm, b"date.default_longitude", sun::DEFAULT_LONGITUDE),
    };
    let zenith = match opt(4) {
        Some(v) => v,
        None => {
            let k: &[u8] = match which {
                sun::RiseSet::Sunrise => b"date.sunrise_zenith",
                sun::RiseSet::Sunset => b"date.sunset_zenith",
            };
            ini_float(vm, k, sun::DEFAULT_ZENITH)
        }
    };
    let zone = default_tz(vm);
    let utc_offset = match opt(5) {
        Some(v) => v,
        None => sun::default_utc_offset_hours(zone.offset_at(0) as i64),
    };
    let local_offset = zone.offset_at(ts) as i64;
    let r = sun::sun_rise_set(
        which,
        ts,
        format,
        lat,
        lon,
        zenith,
        utc_offset,
        local_offset,
    );
    match r {
        Err(e) => vm.value_error(&e.0),
        Ok(SunResult::Timestamp(t)) => Ok(Value::Int(t)),
        Ok(SunResult::String(t)) => Ok(sv(&t)),
        Ok(SunResult::Double(f)) => Ok(Value::Float(f)),
        Ok(SunResult::False) => Ok(Value::Bool(false)),
    }
}

fn date_sunrise(vm: &mut Vm, a: &mut Args) -> R<Value> {
    sun_impl(vm, a, sun::RiseSet::Sunrise)
}

fn date_sunset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    sun_impl(vm, a, sun::RiseSet::Sunset)
}

fn date_sun_info(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let ts = a.int(0);
    let zone = default_tz(vm);
    let local_offset = zone.offset_at(ts) as i64;
    match sun::sun_info(ts, a.float(1), a.float(2), local_offset) {
        Err(e) => vm.value_error(&e.0),
        Ok(info) => {
            let mut out = PhpArray::new();
            for (k, v) in info.entries() {
                out.insert(
                    key(k),
                    match v {
                        SunTime::At(t) => Value::Int(t),
                        SunTime::AlwaysAbove => Value::Bool(true),
                        SunTime::AlwaysBelow => Value::Bool(false),
                    },
                );
            }
            Ok(Value::Array(out))
        }
    }
}
