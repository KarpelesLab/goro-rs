//! DateTime, DateTimeImmutable, DateTimeZone and DateInterval (PHP manual:
//! the class pages). Each method handler also serves the procedural alias
//! (date_format(), date_modify(), ...), which passes the object first:
//! `this_or_first` picks it.

use goro_ext_date::interval::{self, Interval};
use goro_ext_date::parse;
use goro_ext_date::resolve::{self, Mode};
use goro_ext_date::{DateTime, TimeZone, api};
use goro_runtime::{Key, KeyRef, PhpArray, PhpStr, Value};

use super::funcs::get_last_errors;
use super::period::*;
use super::*;

macro_rules! pair {
    ($m:ident, $i:ident, $imp:ident) => {
        pub(super) fn $m(vm: &mut Vm, a: &mut Args) -> R<Value> {
            $imp(vm, a, false)
        }
        pub(super) fn $i(vm: &mut Vm, a: &mut Args) -> R<Value> {
            $imp(vm, a, true)
        }
    };
}

pub(super) static METHODS: &[(&str, crate::func::NativeFn)] = &[
    ("datetime::__construct", dt_construct),
    ("datetime::__serialize", dt_serialize),
    ("datetime::__unserialize", dt_unserialize),
    ("datetime::__wakeup", dt_wakeup),
    ("datetime::__set_state", dt_set_state),
    ("datetime::createfromimmutable", dt_create_from_other),
    ("datetime::createfrominterface", dt_create_from_other),
    ("datetime::createfromformat", dt_create_from_format),
    ("datetime::createfromtimestamp", dt_create_from_timestamp),
    ("datetime::getlasterrors", get_last_errors),
    ("datetime::format", dt_format),
    ("datetime::modify", dt_modify),
    ("datetime::add", dt_add),
    ("datetime::sub", dt_sub),
    ("datetime::gettimezone", dt_get_timezone),
    ("datetime::settimezone", dt_set_timezone),
    ("datetime::getoffset", dt_get_offset),
    ("datetime::getmicrosecond", dt_get_microsecond),
    ("datetime::settime", dt_set_time),
    ("datetime::setdate", dt_set_date),
    ("datetime::setisodate", dt_set_isodate),
    ("datetime::settimestamp", dt_set_timestamp),
    ("datetime::setmicrosecond", dt_set_microsecond),
    ("datetime::gettimestamp", dt_get_timestamp),
    ("datetime::diff", dt_diff),
    ("datetimeimmutable::__construct", dti_construct),
    ("datetimeimmutable::__serialize", dt_serialize),
    ("datetimeimmutable::__unserialize", dt_unserialize),
    ("datetimeimmutable::__wakeup", dt_wakeup),
    ("datetimeimmutable::__set_state", dt_set_state),
    ("datetimeimmutable::createfrommutable", dt_create_from_other),
    (
        "datetimeimmutable::createfrominterface",
        dt_create_from_other,
    ),
    (
        "datetimeimmutable::createfromformat",
        dti_create_from_format,
    ),
    (
        "datetimeimmutable::createfromtimestamp",
        dt_create_from_timestamp,
    ),
    ("datetimeimmutable::getlasterrors", get_last_errors),
    ("datetimeimmutable::format", dt_format),
    ("datetimeimmutable::modify", dti_modify),
    ("datetimeimmutable::add", dti_add),
    ("datetimeimmutable::sub", dti_sub),
    ("datetimeimmutable::gettimezone", dt_get_timezone),
    ("datetimeimmutable::settimezone", dti_set_timezone),
    ("datetimeimmutable::getoffset", dt_get_offset),
    ("datetimeimmutable::getmicrosecond", dt_get_microsecond),
    ("datetimeimmutable::settime", dti_set_time),
    ("datetimeimmutable::setdate", dti_set_date),
    ("datetimeimmutable::setisodate", dti_set_isodate),
    ("datetimeimmutable::settimestamp", dti_set_timestamp),
    ("datetimeimmutable::setmicrosecond", dti_set_microsecond),
    ("datetimeimmutable::gettimestamp", dt_get_timestamp),
    ("datetimeimmutable::diff", dt_diff),
    ("datetimezone::__construct", tz_construct),
    ("datetimezone::getname", tz_get_name),
    ("datetimezone::getoffset", tz_get_offset),
    ("datetimezone::gettransitions", tz_get_transitions),
    ("datetimezone::getlocation", tz_get_location),
    ("datetimezone::listabbreviations", tz_list_abbreviations),
    ("datetimezone::listidentifiers", tz_list_identifiers),
    ("datetimezone::__serialize", tz_serialize),
    ("datetimezone::__unserialize", tz_unserialize),
    ("datetimezone::__wakeup", tz_wakeup),
    ("datetimezone::__set_state", tz_set_state),
    ("dateinterval::__construct", iv_construct),
    (
        "dateinterval::createfromdatestring",
        iv_create_from_date_string,
    ),
    ("dateinterval::format", iv_format),
    ("dateinterval::__serialize", iv_serialize),
    ("dateinterval::__unserialize", iv_unserialize),
    ("dateinterval::__wakeup", iv_wakeup),
    ("dateinterval::__set_state", iv_set_state),
    ("dateperiod::createfromiso8601string", dp_create_from_iso),
    ("dateperiod::__construct", dp_construct),
    ("dateperiod::getstartdate", dp_get_start),
    ("dateperiod::getenddate", dp_get_end),
    ("dateperiod::getdateinterval", dp_get_interval),
    ("dateperiod::getrecurrences", dp_get_recurrences),
    ("dateperiod::__serialize", dp_serialize),
    ("dateperiod::__unserialize", dp_unserialize),
    ("dateperiod::__wakeup", dp_wakeup),
    ("dateperiod::__set_state", dp_set_state),
    ("dateperiod::getiterator", dp_get_iterator),
    ("dateperioditerator::current", it_current),
    ("dateperioditerator::key", it_key),
    ("dateperioditerator::next", it_next),
    ("dateperioditerator::valid", it_valid),
    ("dateperioditerator::rewind", it_rewind),
];

// ---------------------------------------------------------------- helpers

/// Parses a constructor / date_create() string, recording its warnings
/// and errors; the error is the DateMalformedStringException message.
pub(crate) fn parse_create(vm: &mut Vm, input: &[u8], zone: &TimeZone) -> Result<DateTime, String> {
    // an empty string means "now" here (test DateTime_construct_basic1.phpt)
    let input: &[u8] = if input.is_empty() { b"now" } else { input };
    let p = parse::parse(input);
    set_last_errors(
        vm,
        LastErrors {
            warnings: p.warnings.clone(),
            errors: p.errors.clone(),
        },
    );
    if let Some(msg) = resolve::parse_error_message(input, &p) {
        return Err(msg);
    }
    let (n, us) = now();
    Ok(resolve::resolve(&p, n, us, zone, Mode::Create))
}

/// A DateTimeZone constructor argument, or the warning / exception text
/// (observed on PHP 8.5: offsets beyond ±99:59 are "out of range").
pub(crate) fn parse_zone(name: &[u8]) -> Result<TimeZone, String> {
    let text = String::from_utf8_lossy(name);
    if let Some(z) = TimeZone::parse(&text) {
        return Ok(z);
    }
    let t = text.trim_start_matches([' ', '\t']);
    let t = t.strip_prefix("GMT").unwrap_or(t);
    if t.starts_with(['+', '-'])
        && t.len() > 1
        && t[1..].bytes().all(|c| c.is_ascii_digit() || c == b':')
    {
        return Err(format!("Timezone offset is out of range ({text})"));
    }
    Err(format!("Unknown or bad timezone ({text})"))
}

/// Stores the result of a mutator: in place for DateTime, on a clone for
/// DateTimeImmutable.
fn finish(vm: &mut Vm, o: &ObjRef, imm: bool, dt: DateTime) -> R<Value> {
    if imm {
        let c = vm.clone_object(o)?;
        set_dt(&c, dt);
        Ok(Value::Object(c))
    } else {
        set_dt(o, dt);
        Ok(Value::Object(o.clone()))
    }
}

/// The internal class a method belongs to, for messages.
fn scope_name(a: &Args) -> String {
    a.func
        .scope()
        .map(|c| c.name.to_string_lossy())
        .unwrap_or_default()
}

fn invalid_data<T>(vm: &mut Vm, what: &str) -> R<T> {
    vm.throw(
        "Error",
        &format!("Invalid serialization data for {what} object"),
    )
}

fn in_hierarchy(vm: &Vm, o: &ObjRef, name: &[u8]) -> bool {
    let mut c = Some(vm.class_of(o));
    while let Some(k) = c {
        if k.name.as_bytes().eq_ignore_ascii_case(name) {
            return true;
        }
        c = k.parent.clone();
    }
    false
}

/// Restores the properties of serialized data other than `skip`.
pub(crate) fn restore_props(vm: &mut Vm, o: &ObjRef, data: &PhpArray, skip: &[&str]) -> R<()> {
    for (k, v) in data.iter() {
        let name = match k {
            KeyRef::Str(s) => s.clone(),
            KeyRef::Int(i) => PhpStr::from(i.to_string().as_str()),
        };
        if skip.iter().any(|s| s.as_bytes() == name.as_bytes()) {
            continue;
        }
        let v = v.deref();
        // mangled names: "\0Class\0prop" / "\0*\0prop"
        let plain = match name.as_bytes() {
            [0, rest @ ..] => match rest.iter().position(|&c| c == 0) {
                Some(p) => {
                    // a private property of a class the object is not
                    // (test gh10747-error.phpt) is dropped
                    let owner = &rest[..p];
                    if owner != b"*" && !in_hierarchy(vm, o, owner) {
                        continue;
                    }
                    PhpStr::new(&rest[p + 1..])
                }
                None => name.clone(),
            },
            _ => name.clone(),
        };
        vm.set_prop_raw(o, plain.as_bytes(), v);
    }
    Ok(())
}

// ---------------------------------------------------------------- DateTime

fn construct_impl(vm: &mut Vm, a: &Args) -> R<Value> {
    let o = a.this_obj().clone();
    let input = if a.has(0) {
        a.str(0)
    } else {
        PhpStr::from("now")
    };
    let zone = zone_arg(vm, a, 1)?;
    match parse_create(vm, &input, &zone) {
        Ok(dt) => {
            set_dt(&o, dt);
            Ok(Value::Null)
        }
        Err(msg) => vm.throw("DateMalformedStringException", &msg),
    }
}

fn dt_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    construct_impl(vm, a)
}

fn dti_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    construct_impl(vm, a)
}

fn dt_serialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let dt = get_dt(vm, &o)?;
    let mut out = PhpArray::new();
    date_parts(&dt, &mut out);
    for (k, v) in vm.plain_props_array(&o, true).iter() {
        out.insert(k.to_owned(), v.clone());
    }
    Ok(Value::Array(out))
}

/// The DateTime held by serialized data, if valid.
fn date_from_data(data: &PhpArray) -> Option<DateTime> {
    let date = match data.get_bytes(b"date")?.deref() {
        Value::Str(s) => s,
        _ => return None,
    };
    let ty = match data.get_bytes(b"timezone_type")?.deref() {
        Value::Int(i) => i,
        _ => return None,
    };
    let tz = match data.get_bytes(b"timezone")?.deref() {
        Value::Str(s) => s,
        _ => return None,
    };
    let zone = DateTime::zone_from_parts(ty, &tz.to_string_lossy())?;
    if let Some(d) = dump_date(&date, &zone) {
        return Some(d);
    }
    DateTime::from_parts(&date, ty, &tz.to_string_lossy())
}

/// The "[-]Y-m-d H:i:s.uuuuuu" form var_dump() and serialize() write,
/// read directly so that years beyond four digits round-trip (test
/// bug75035.phpt).
fn dump_date(s: &[u8], zone: &TimeZone) -> Option<DateTime> {
    let t = std::str::from_utf8(s).ok()?;
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let (date, time) = t.split_once(' ')?;
    let mut d = date.split('-');
    let y: i64 = d.next()?.parse().ok()?;
    let m: i64 = d.next()?.parse().ok()?;
    let dd: i64 = d.next()?.parse().ok()?;
    if d.next().is_some() {
        return None;
    }
    let (hms, us) = time.split_once('.')?;
    if us.len() != 6 {
        return None;
    }
    let us: i64 = us.parse().ok()?;
    let mut h = hms.split(':');
    let hh: i64 = h.next()?.parse().ok()?;
    let mi: i64 = h.next()?.parse().ok()?;
    let ss: i64 = h.next()?.parse().ok()?;
    if h.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&dd) {
        return None;
    }
    let y = if neg { -y } else { y };
    Some(DateTime::from_local(y, m, dd, hh, mi, ss, us, zone.clone()))
}

const DATE_KEYS: &[&str] = &["date", "timezone_type", "timezone"];

fn dt_unserialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let data = a.array(0);
    let Some(dt) = date_from_data(&data) else {
        return invalid_data(vm, &scope_name(a));
    };
    set_dt(&o, dt);
    restore_props(vm, &o, &data, DATE_KEYS)?;
    Ok(Value::Null)
}

fn dt_wakeup(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let data = vm.plain_props_array(&o, false);
    let Some(dt) = date_from_data(&data) else {
        return invalid_data(vm, &scope_name(a));
    };
    set_dt(&o, dt);
    Ok(Value::Null)
}

fn dt_set_state(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cls = static_class(vm, a);
    let data = a.array(0);
    let Some(dt) = date_from_data(&data) else {
        return invalid_data(vm, &scope_name(a));
    };
    let o = new_date(vm, &cls, dt)?;
    restore_props(vm, &o, &data, DATE_KEYS)?;
    Ok(Value::Object(o))
}

/// createFromImmutable / createFromMutable / createFromInterface.
fn dt_create_from_other(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let src = a.obj(0).expect("object argument");
    let dt = get_dt(vm, &src)?;
    let cls = static_class(vm, a);
    Ok(Value::Object(new_date(vm, &cls, dt)?))
}

/// "f(): Argument #N ($name) must not contain any null bytes" (test
/// bug72963.phpt).
pub(crate) fn no_nul(vm: &mut Vm, a: &Args, v: &[u8], n: usize, name: &str) -> R<()> {
    if v.contains(&0) {
        return vm.value_error(&format!(
            "{}(): Argument #{n} (${name}) must not contain any null bytes",
            a.fname()
        ));
    }
    Ok(())
}

fn create_from_format_impl(vm: &mut Vm, a: &Args, imm: bool) -> R<Value> {
    let fmt = a.str(0);
    let input = a.str(1);
    no_nul(vm, a, &fmt, 1, "format")?;
    no_nul(vm, a, &input, 2, "datetime")?;
    let zone = zone_arg(vm, a, 2)?;
    let (n, us) = now();
    let cls = if a.func.scope().is_some() {
        static_class(vm, a)
    } else {
        class(vm, if imm { "DateTimeImmutable" } else { "DateTime" })?
    };
    match goro_ext_date::from_format::create_from_format(&fmt, &input, &zone, n, us) {
        Ok((dt, errs)) => {
            set_last_errors(vm, errs);
            Ok(Value::Object(new_date(vm, &cls, dt)?))
        }
        Err(errs) => {
            set_last_errors(vm, errs);
            Ok(Value::Bool(false))
        }
    }
}
pair!(
    dt_create_from_format,
    dti_create_from_format,
    create_from_format_impl
);

fn dt_create_from_timestamp(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let dt = match a.val(0) {
        Value::Float(f) => match DateTime::from_float_timestamp(f) {
            Some(d) => d,
            None => {
                let shown = goro_runtime::convert::float_to_string(f, 17);
                return vm.throw(
                    "DateRangeError",
                    &format!(
                        "{}(): Argument #1 ($timestamp) must be a finite number between {} and {}.999999, {} given",
                        a.fname(),
                        i64::MIN,
                        i64::MAX,
                        shown.to_string_lossy()
                    ),
                );
            }
        },
        v => DateTime::from_ts(vm.to_int(&v)?, 0, TimeZone::Offset(0)),
    };
    let cls = static_class(vm, a);
    Ok(Value::Object(new_date(vm, &cls, dt)?))
}

pub(super) fn dt_format(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, b) = this_or_first(a);
    let fmt = a.str(b);
    let dt = get_dt(vm, &o)?;
    Ok(s(&dt.format(&fmt)))
}

fn modify_impl(vm: &mut Vm, a: &Args, imm: bool) -> R<Value> {
    let (o, b) = this_or_first(a);
    let input = a.str(b);
    let dt = get_dt(vm, &o)?;
    let p = parse::parse(&input);
    set_last_errors(
        vm,
        LastErrors {
            warnings: p.warnings.clone(),
            errors: p.errors.clone(),
        },
    );
    if let Some(msg) = resolve::parse_error_message(&input, &p) {
        if a.this.is_none() {
            vm.fwarn(a, &msg)?;
            return Ok(Value::Bool(false));
        }
        return vm.fthrow(a, "DateMalformedStringException", &msg);
    }
    let new = resolve::resolve(&p, dt.ts, dt.us, &dt.zone, Mode::Modify);
    finish(vm, &o, imm, new)
}
pair!(dt_modify, dti_modify, modify_impl);

fn interval_arg(vm: &mut Vm, a: &Args, i: usize) -> R<Interval> {
    let o = a.obj(i).expect("interval argument");
    get_iv(vm, &o)
}

fn add_impl(vm: &mut Vm, a: &Args, imm: bool) -> R<Value> {
    let (o, b) = this_or_first(a);
    let dt = get_dt(vm, &o)?;
    let iv = interval_arg(vm, a, b)?;
    let new = interval::add(&dt, &iv, 1);
    finish(vm, &o, imm, new)
}
pair!(dt_add, dti_add, add_impl);

fn sub_impl(vm: &mut Vm, a: &Args, imm: bool) -> R<Value> {
    let (o, b) = this_or_first(a);
    let dt = get_dt(vm, &o)?;
    let iv = interval_arg(vm, a, b)?;
    match interval::sub(&dt, &iv) {
        Ok(new) => finish(vm, &o, imm, new),
        Err(msg) => {
            if a.this.is_none() {
                vm.fwarn(a, msg)?;
                return Ok(Value::Object(o));
            }
            vm.fthrow(a, "DateInvalidOperationException", msg)
        }
    }
}
pair!(dt_sub, dti_sub, sub_impl);

pub(super) fn dt_get_timezone(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_or_first(a);
    let dt = get_dt(vm, &o)?;
    Ok(Value::Object(new_tz(vm, dt.zone)?))
}

fn set_timezone_impl(vm: &mut Vm, a: &Args, imm: bool) -> R<Value> {
    let (o, b) = this_or_first(a);
    let dt = get_dt(vm, &o)?;
    let z = a.obj(b).expect("zone argument");
    let z = get_tz(vm, &z)?;
    finish(vm, &o, imm, dt.set_timezone(z))
}
pair!(dt_set_timezone, dti_set_timezone, set_timezone_impl);

pub(super) fn dt_get_offset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_or_first(a);
    let dt = get_dt(vm, &o)?;
    Ok(Value::Int(dt.offset() as i64))
}

fn dt_get_microsecond(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_or_first(a);
    let dt = get_dt(vm, &o)?;
    Ok(Value::Int(dt.us as i64))
}

pub(super) fn dt_get_timestamp(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_or_first(a);
    let dt = get_dt(vm, &o)?;
    Ok(Value::Int(dt.ts))
}

fn set_time_impl(vm: &mut Vm, a: &Args, imm: bool) -> R<Value> {
    let (o, b) = this_or_first(a);
    let dt = get_dt(vm, &o)?;
    let new = dt.set_time(
        a.int(b),
        a.int(b + 1),
        a.int_or(b + 2, 0),
        a.int_or(b + 3, 0),
    );
    finish(vm, &o, imm, new)
}
pair!(dt_set_time, dti_set_time, set_time_impl);

fn set_date_impl(vm: &mut Vm, a: &Args, imm: bool) -> R<Value> {
    let (o, b) = this_or_first(a);
    let dt = get_dt(vm, &o)?;
    let new = dt.set_date(a.int(b), a.int(b + 1), a.int(b + 2));
    finish(vm, &o, imm, new)
}
pair!(dt_set_date, dti_set_date, set_date_impl);

fn set_isodate_impl(vm: &mut Vm, a: &Args, imm: bool) -> R<Value> {
    let (o, b) = this_or_first(a);
    let dt = get_dt(vm, &o)?;
    let new = dt.set_iso_date(a.int(b), a.int(b + 1), a.int_or(b + 2, 1));
    finish(vm, &o, imm, new)
}
pair!(dt_set_isodate, dti_set_isodate, set_isodate_impl);

fn set_timestamp_impl(vm: &mut Vm, a: &Args, imm: bool) -> R<Value> {
    let (o, b) = this_or_first(a);
    let dt = get_dt(vm, &o)?;
    let new = dt.set_timestamp(a.int(b));
    finish(vm, &o, imm, new)
}
pair!(dt_set_timestamp, dti_set_timestamp, set_timestamp_impl);

fn set_microsecond_impl(vm: &mut Vm, a: &Args, imm: bool) -> R<Value> {
    let o = a.this_obj().clone();
    let dt = get_dt(vm, &o)?;
    let us = a.int(0);
    if !(0..=999_999).contains(&us) {
        return vm.fthrow(
            a,
            "DateRangeError",
            &format!("Argument #1 ($microsecond) must be between 0 and 999999, {us} given"),
        );
    }
    let new = DateTime {
        us: us as u32,
        ..dt
    };
    finish(vm, &o, imm, new)
}
pair!(
    dt_set_microsecond,
    dti_set_microsecond,
    set_microsecond_impl
);

pub(super) fn dt_diff(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, b) = this_or_first(a);
    let dt = get_dt(vm, &o)?;
    let other = a.obj(b).expect("date argument");
    let other = get_dt(vm, &other)?;
    let iv = interval::diff(&dt, &other, a.bool_or(b + 1, false));
    Ok(Value::Object(new_iv(vm, &iv)?))
}

// ---------------------------------------------------------------- DateTimeZone

fn tz_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    no_nul(vm, a, &a.str(0), 1, "timezone")?;
    match parse_zone(&a.str(0)) {
        Ok(z) => {
            set_tz(&o, z);
            Ok(Value::Null)
        }
        Err(msg) => vm.fthrow(a, "DateInvalidTimeZoneException", &msg),
    }
}

pub(super) fn tz_get_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_or_first(a);
    let z = get_tz(vm, &o)?;
    Ok(sv(&z.name()))
}

pub(super) fn tz_get_offset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, b) = this_or_first(a);
    let z = get_tz(vm, &o)?;
    let d = a.obj(b).expect("date argument");
    let dt = get_dt(vm, &d)?;
    Ok(Value::Int(z.offset_at(dt.ts) as i64))
}

pub(super) fn tz_get_transitions(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, b) = this_or_first(a);
    let z = get_tz(vm, &o)?;
    let begin = a.int_or(b, i64::MIN);
    let end = if a.has(b + 1) {
        a.int(b + 1)
    } else {
        api::TRANSITIONS_DEFAULT_END
    };
    let Some(list) = api::transitions(&z, begin, end) else {
        return Ok(Value::Bool(false));
    };
    let mut out = PhpArray::new();
    for t in list {
        let mut e = PhpArray::new();
        e.insert(key("ts"), Value::Int(t.ts));
        e.insert(key("time"), sv(&t.time));
        e.insert(key("offset"), Value::Int(t.offset as i64));
        e.insert(key("isdst"), Value::Bool(t.isdst));
        e.insert(key("abbr"), sv(&t.abbr));
        let _ = out.push(Value::Array(e));
    }
    Ok(Value::Array(out))
}

pub(super) fn tz_get_location(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_or_first(a);
    let z = get_tz(vm, &o)?;
    let Some(l) = z.location() else {
        return Ok(Value::Bool(false));
    };
    let mut e = PhpArray::new();
    e.insert(key("country_code"), sv(l.country_code));
    e.insert(key("latitude"), Value::Float(l.latitude));
    e.insert(key("longitude"), Value::Float(l.longitude));
    e.insert(key("comments"), sv(l.comments));
    Ok(Value::Array(e))
}

pub(super) fn tz_list_abbreviations(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let mut out = PhpArray::new();
    for e in goro_ext_date::tz::abbr::entries() {
        let mut row = PhpArray::new();
        row.insert(key("dst"), Value::Bool(e.dst));
        row.insert(key("offset"), Value::Int(e.offset as i64));
        row.insert(
            key("timezone_id"),
            match e.tz {
                Some(t) => sv(t),
                None => Value::Null,
            },
        );
        let k = Key::from(e.abbr);
        if let Some(Value::Array(list)) = out.get(k.as_ref()) {
            let mut list = list.clone();
            let _ = list.push(Value::Array(row));
            out.insert(k, Value::Array(list));
        } else {
            out.insert(k, Value::Array(PhpArray::from_values([Value::Array(row)])));
        }
    }
    Ok(Value::Array(out))
}

pub(super) fn tz_list_identifiers(vm: &mut Vm, a: &mut Args) -> R<Value> {
    use goro_ext_date::api::groups;
    let group = a.int_or(0, groups::ALL);
    let country = a.opt_str(1).map(|c| c.to_string_lossy());
    if !(group == groups::PER_COUNTRY || (0..=groups::ALL_WITH_BC).contains(&group)) {
        return vm.value_error(&format!(
            "{}(): Argument #1 ($timezoneGroup) must be one of the DateTimeZone group constants",
            a.fname()
        ));
    }
    if group == groups::PER_COUNTRY && country.as_ref().is_none_or(|c| c.len() != 2) {
        return vm.value_error(&format!(
            "{}(): Argument #2 ($countryCode) must be a two-letter ISO 3166-1 compatible country code when argument #1 ($timezoneGroup) is DateTimeZone::PER_COUNTRY",
            a.fname()
        ));
    }
    let list = api::list_identifiers(group, country.as_deref());
    Ok(Value::Array(PhpArray::from_values(
        list.into_iter().map(sv),
    )))
}

fn tz_serialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let z = get_tz(vm, &o)?;
    let mut out = PhpArray::new();
    zone_parts(&z, &mut out);
    for (k, v) in vm.plain_props_array(&o, true).iter() {
        out.insert(k.to_owned(), v.clone());
    }
    Ok(Value::Array(out))
}

fn zone_from_data(data: &PhpArray) -> Option<TimeZone> {
    let ty = match data.get_bytes(b"timezone_type")?.deref() {
        Value::Int(i) => i,
        _ => return None,
    };
    let tz = match data.get_bytes(b"timezone")?.deref() {
        Value::Str(s) => s,
        _ => return None,
    };
    DateTime::zone_from_parts(ty, &tz.to_string_lossy())
}

const ZONE_KEYS: &[&str] = &["timezone_type", "timezone"];

fn tz_unserialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let data = a.array(0);
    let Some(z) = zone_from_data(&data) else {
        return invalid_data(vm, "DateTimeZone");
    };
    set_tz(&o, z);
    restore_props(vm, &o, &data, ZONE_KEYS)?;
    Ok(Value::Null)
}

fn tz_wakeup(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let data = vm.plain_props_array(&o, false);
    let Some(z) = zone_from_data(&data) else {
        return invalid_data(vm, "DateTimeZone");
    };
    set_tz(&o, z);
    Ok(Value::Null)
}

fn tz_set_state(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cls = static_class(vm, a);
    let data = a.array(0);
    let Some(z) = zone_from_data(&data) else {
        return invalid_data(vm, "DateTimeZone");
    };
    let o = vm.instantiate(&cls)?;
    set_tz(&o, z);
    restore_props(vm, &o, &data, ZONE_KEYS)?;
    Ok(Value::Object(o))
}

// ---------------------------------------------------------------- DateInterval

fn iv_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let spec = a.str(0);
    let r = Interval::parse_iso(&spec).or_else(|e| two_dates(&spec).unwrap_or(Err(e)));
    match r {
        Ok(iv) => {
            set_iv(vm, &o, &iv);
            Ok(Value::Null)
        }
        Err(msg) => vm.throw("DateMalformedIntervalStringException", &msg),
    }
}

/// "YYYY-MM-DDTHH:MM:SSZ" or "YYYYMMDDTHHMMSSZ" (the ISO 8601 dates of
/// an interval written as two dates).
fn iso_utc_date(s: &[u8]) -> Option<DateTime> {
    let num = |r: &[u8]| -> Option<i64> {
        if r.is_empty() || !r.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(r).ok()?.parse().ok()
    };
    let f = match s.len() {
        20 if s[4] == b'-'
            && s[7] == b'-'
            && s[10] == b'T'
            && s[13] == b':'
            && s[16] == b':'
            && s[19] == b'Z' =>
        {
            [
                &s[0..4],
                &s[5..7],
                &s[8..10],
                &s[11..13],
                &s[14..16],
                &s[17..19],
            ]
        }
        16 if s[8] == b'T' && s[15] == b'Z' => [
            &s[0..4],
            &s[4..6],
            &s[6..8],
            &s[9..11],
            &s[11..13],
            &s[13..15],
        ],
        _ => return None,
    };
    let v: Option<Vec<i64>> = f.iter().map(|p| num(p)).collect();
    let v = v?;
    Some(DateTime::from_local(
        v[0],
        v[1],
        v[2],
        v[3],
        v[4],
        v[5],
        0,
        TimeZone::Offset(0),
    ))
}

/// `new DateInterval("start/end")`: the difference between two ISO
/// dates (test bug52808.phpt); `None` when the spec does not start with
/// a date.
fn two_dates(spec: &[u8]) -> Option<Result<Interval, String>> {
    let cut = spec.iter().position(|&c| c == b'/' || c == b' ');
    let (first, rest) = match cut {
        Some(p) => (&spec[..p], Some(&spec[p + 1..])),
        None => (spec, None),
    };
    let a = iso_utc_date(first)?;
    let failed = || {
        Err(format!(
            "Failed to parse interval ({})",
            String::from_utf8_lossy(spec)
        ))
    };
    match rest {
        None => Some(failed()),
        Some([]) => Some(failed()),
        Some(r) => match iso_utc_date(r) {
            Some(b) => Some(Ok(interval::diff(&a, &b, false))),
            None => Some(Err(interval::bad_format(spec))),
        },
    }
}

pub(super) fn iv_create_from_date_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match Interval::from_date_string(&a.str(0)) {
        Ok(iv) => {
            let cls = if a.func.scope().is_some() {
                static_class(vm, a)
            } else {
                class(vm, "DateInterval")?
            };
            let o = vm.instantiate(&cls)?;
            set_iv(vm, &o, &iv);
            Ok(Value::Object(o))
        }
        Err(msg) => {
            if a.func.scope().is_none() {
                vm.fwarn(a, &msg)?;
                return Ok(Value::Bool(false));
            }
            vm.throw("DateMalformedIntervalStringException", &msg)
        }
    }
}

pub(super) fn iv_format(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, b) = this_or_first(a);
    let iv = get_iv(vm, &o)?;
    Ok(s(&iv.format(&a.str(b))))
}

fn iv_serialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    get_iv(vm, &o)?;
    let all = vm.plain_props_array(&o, true);
    let mut out = PhpArray::new();
    for k in IV_KEYS {
        if let Some(v) = all.get_bytes(k.as_bytes()) {
            out.insert(key(k), v.clone());
        }
    }
    for (k, v) in all.iter() {
        if out.get(k).is_none() {
            out.insert(k.to_owned(), v.clone());
        }
    }
    Ok(Value::Array(out))
}

const IV_KEYS: &[&str] = &[
    "y",
    "m",
    "d",
    "h",
    "i",
    "s",
    "f",
    "invert",
    "days",
    "from_string",
    "date_string",
];

/// The interval described by serialized / __set_state data.
fn iv_from_data(vm: &mut Vm, data: &PhpArray) -> R<Interval> {
    let date_string = match data.get_bytes(b"date_string").map(|v| v.deref()) {
        Some(Value::Str(ds)) => Some(ds),
        _ => None,
    };
    let from_string = match data.get_bytes(b"from_string") {
        Some(v) => goro_runtime::convert::to_bool_quiet(&v.deref()),
        None => date_string.is_some(),
    };
    if from_string && let Some(ds) = date_string {
        // the relative part of the string, absolute parts ignored (test
        // DateInterval_set_state_exception.phpt)
        let p = parse::parse(&ds);
        if let Some((pos, msg)) = p.errors.first() {
            let c = ds.as_bytes().get(*pos).copied().unwrap_or(b' ');
            return vm.throw(
                "Error",
                &format!(
                    "Unknown or bad format ({}) at position {} ({}) while unserializing: {}",
                    ds.to_string_lossy(),
                    pos,
                    String::from_utf8_lossy(&[c]),
                    msg
                ),
            );
        }
        let r = p.relative.unwrap_or_default();
        return Ok(Interval {
            y: r.y,
            m: r.m,
            d: r.d,
            h: r.h,
            i: r.i,
            s: r.s,
            us: r.us,
            from_string: Some(goro_ext_date::interval::FromString {
                date_string: ds.as_bytes().to_vec(),
                relative: r,
            }),
            ..Default::default()
        });
    }
    // fields missing or not numbers read as -1 (test bug73091.phpt)
    let int = |vm: &mut Vm, k: &str| -> R<i64> {
        match data.get_bytes(k.as_bytes()).map(|v| v.deref()) {
            Some(v @ (Value::Int(_) | Value::Float(_) | Value::Bool(_))) => vm.to_int(&v),
            // strings convert quietly, other values are -1 (tests
            // bug53437_var5.phpt, bug73091.phpt)
            Some(Value::Str(t)) => Ok(goro_runtime::convert::str_to_int(&t)),
            Some(_) => Ok(-1),
            None => Ok(if k == "invert" { 0 } else { -1 }),
        }
    };
    let y = int(vm, "y")?;
    let m = int(vm, "m")?;
    let d = int(vm, "d")?;
    let h = int(vm, "h")?;
    let i = int(vm, "i")?;
    let sec = int(vm, "s")?;
    let invert = int(vm, "invert")? != 0;
    let f = match data.get_bytes(b"f") {
        Some(v) => vm.to_float(&v.deref())?,
        None => 0.0,
    };
    let us_f = (f * 1_000_000.0).round();
    if !(-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&us_f) {
        // test bug79015.phpt
        vm.warn(&format!(
            "The float {} is not representable as an int, cast occurred",
            goro_runtime::convert::float_repr(us_f)
        ))?;
    }
    let days = match data.get_bytes(b"days").map(|v| v.deref()) {
        None | Some(Value::Bool(false)) | Some(Value::Null) => None,
        Some(v @ (Value::Int(_) | Value::Float(_) | Value::Bool(_))) => Some(vm.to_int(&v)?),
        Some(Value::Str(t)) => Some(goro_runtime::convert::str_to_int(&t)),
        Some(_) => Some(-1),
    };
    Ok(Interval {
        y,
        m,
        d,
        h,
        i,
        s: sec,
        us: (f * 1_000_000.0).round() as i64,
        invert,
        days,
        from_string: None,
    })
}

fn iv_unserialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let data = a.array(0);
    let iv = iv_from_data(vm, &data)?;
    set_iv(vm, &o, &iv);
    restore_props(vm, &o, &data, IV_KEYS)?;
    Ok(Value::Null)
}

fn iv_wakeup(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let data = vm.plain_props_array(&o, false);
    let iv = iv_from_data(vm, &data)?;
    set_iv(vm, &o, &iv);
    Ok(Value::Null)
}

fn iv_set_state(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cls = static_class(vm, a);
    let data = a.array(0);
    let iv = iv_from_data(vm, &data)?;
    let o = vm.instantiate(&cls)?;
    set_iv(vm, &o, &iv);
    restore_props(vm, &o, &data, IV_KEYS)?;
    Ok(Value::Object(o))
}
