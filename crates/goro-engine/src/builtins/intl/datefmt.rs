//! `IntlDateFormatter` and the `datefmt_*()` functions (PHP manual:
//! IntlDateFormatter). Formatting and parsing are
//! goro_ext_intl::datefmt's.

use goro_ext_intl::calendar::{self as ic, CalType, Calendar};
use goro_ext_intl::datefmt::{self as idf, DateFormatter, LocaleId};
use goro_ext_intl::error as ue;
use goro_ext_intl::timezone::TimeZone;
use goro_runtime::{Key, ObjRef, PhpArray, Value};

use super::calendar::{make_calendar, new_cal_obj, peek_cal};
use super::timezone::{
    clone_error, default_zone, from_php_zone, is_instance, new_itz, peek_itz, set_ref_typed,
};
use super::{fail, fail_obj, has_obj, obj_err, reset, reset_obj, this_or_first, with_obj};
use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

const SIG_ARGS: &str = "?string $locale, int $dateType = IntlDateFormatter::FULL, int $timeType = IntlDateFormatter::FULL, object|string|null $timezone = null, IntlCalendar|int|null $calendar = null, ?string $pattern = null";

pub static MODULE: Module = Module {
    name: "intl",
    functions: &[
        (
            "function datefmt_create(?string $locale, int $dateType = IntlDateFormatter::FULL, int $timeType = IntlDateFormatter::FULL, object|string|null $timezone = null, IntlCalendar|int|null $calendar = null, ?string $pattern = null): ?IntlDateFormatter",
            df_create,
        ),
        (
            "function datefmt_get_datetype(IntlDateFormatter $formatter): int|false",
            df_get_datetype,
        ),
        (
            "function datefmt_get_timetype(IntlDateFormatter $formatter): int|false",
            df_get_timetype,
        ),
        (
            "function datefmt_get_calendar(IntlDateFormatter $formatter): int|false",
            df_get_calendar,
        ),
        (
            "function datefmt_set_calendar(IntlDateFormatter $formatter, IntlCalendar|int|null $calendar): bool",
            df_set_calendar,
        ),
        (
            "function datefmt_get_timezone_id(IntlDateFormatter $formatter): string|false",
            df_get_timezone_id,
        ),
        (
            "function datefmt_get_calendar_object(IntlDateFormatter $formatter): IntlCalendar|false|null",
            df_get_calendar_object,
        ),
        (
            "function datefmt_get_timezone(IntlDateFormatter $formatter): IntlTimeZone|false",
            df_get_timezone,
        ),
        (
            "function datefmt_set_timezone(IntlDateFormatter $formatter, object|string|null $timezone): bool",
            df_set_timezone,
        ),
        (
            "function datefmt_set_pattern(IntlDateFormatter $formatter, string $pattern): bool",
            df_set_pattern,
        ),
        (
            "function datefmt_get_pattern(IntlDateFormatter $formatter): string|false",
            df_get_pattern,
        ),
        (
            "function datefmt_get_locale(IntlDateFormatter $formatter, int $type = 0): string|false",
            df_get_locale,
        ),
        (
            "function datefmt_set_lenient(IntlDateFormatter $formatter, bool $lenient): void",
            df_set_lenient,
        ),
        (
            "function datefmt_is_lenient(IntlDateFormatter $formatter): bool",
            df_is_lenient,
        ),
        (
            "function datefmt_format(IntlDateFormatter $formatter, $datetime): string|false",
            df_format,
        ),
        (
            "function datefmt_format_object($datetime, $format = null, ?string $locale = null): string|false",
            df_format_object,
        ),
        (
            "function datefmt_parse(IntlDateFormatter $formatter, string $string, &$offset = null): int|float|false",
            df_parse,
        ),
        (
            "function datefmt_localtime(IntlDateFormatter $formatter, string $string, &$offset = null): array|false",
            df_localtime,
        ),
        (
            "function datefmt_get_error_code(IntlDateFormatter $formatter): int",
            df_error_code,
        ),
        (
            "function datefmt_get_error_message(IntlDateFormatter $formatter): string",
            df_error_message,
        ),
    ],
    classes: &[("intldateformatter", DF_CLASS)],
    methods: &[
        ("intldateformatter::__construct", df_construct),
        ("intldateformatter::create", df_create),
        ("intldateformatter::getdatetype", df_get_datetype),
        ("intldateformatter::gettimetype", df_get_timetype),
        ("intldateformatter::getcalendar", df_get_calendar),
        ("intldateformatter::setcalendar", df_set_calendar),
        ("intldateformatter::gettimezoneid", df_get_timezone_id),
        (
            "intldateformatter::getcalendarobject",
            df_get_calendar_object,
        ),
        ("intldateformatter::gettimezone", df_get_timezone),
        ("intldateformatter::settimezone", df_set_timezone),
        ("intldateformatter::setpattern", df_set_pattern),
        ("intldateformatter::getpattern", df_get_pattern),
        ("intldateformatter::getlocale", df_get_locale),
        ("intldateformatter::setlenient", df_set_lenient),
        ("intldateformatter::islenient", df_is_lenient),
        ("intldateformatter::format", df_format),
        ("intldateformatter::formatobject", df_format_object),
        ("intldateformatter::parse", df_parse),
        ("intldateformatter::parsetocalendar", df_parse_to_calendar),
        ("intldateformatter::localtime", df_localtime),
        ("intldateformatter::geterrorcode", df_error_code),
        ("intldateformatter::geterrormessage", df_error_message),
    ],
    hooks: &[("intldateformatter", df_hooks)],
    ..Module::EMPTY
};

const DF_CLASS: &str = r#"<?php class IntlDateFormatter {
    public const int FULL = 0;
    public const int LONG = 1;
    public const int MEDIUM = 2;
    public const int SHORT = 3;
    public const int NONE = -1;
    public const int RELATIVE_FULL = 128;
    public const int RELATIVE_LONG = 129;
    public const int RELATIVE_MEDIUM = 130;
    public const int RELATIVE_SHORT = 131;
    public const int PATTERN = -2;
    public const int GREGORIAN = 1;
    public const int TRADITIONAL = 0;
    public function __construct(?string $locale, int $dateType = IntlDateFormatter::FULL, int $timeType = IntlDateFormatter::FULL, object|string|null $timezone = null, IntlCalendar|int|null $calendar = null, ?string $pattern = null) {}
    public static function create(?string $locale, int $dateType = IntlDateFormatter::FULL, int $timeType = IntlDateFormatter::FULL, object|string|null $timezone = null, IntlCalendar|int|null $calendar = null, ?string $pattern = null): ?IntlDateFormatter {}
    public function getDateType(): int|false {}
    public function getTimeType(): int|false {}
    public function getCalendar(): int|false {}
    public function setCalendar(IntlCalendar|int|null $calendar): bool {}
    public function getTimeZoneId(): string|false {}
    public function getCalendarObject(): IntlCalendar|false|null {}
    public function getTimeZone(): IntlTimeZone|false {}
    public function setTimeZone(object|string|null $timezone): bool {}
    public function setPattern(string $pattern): bool {}
    public function getPattern(): string|false {}
    public function getLocale(int $type = 0): string|false {}
    public function setLenient(bool $lenient): void {}
    public function isLenient(): bool {}
    public function format($datetime): string|false {}
    public static function formatObject($datetime, $format = null, ?string $locale = null): string|false {}
    public function parse(string $string, &$offset = null): int|float|false {}
    public function parseToCalendar(string $string, &$offset = null): int|float|false {}
    public function localtime(string $string, &$offset = null): array|false {}
    public function getErrorCode(): int {}
    public function getErrorMessage(): string {}
}"#;

/// The native state: the formatter and the calendar kind it was given
/// (GREGORIAN/TRADITIONAL, None for a calendar object).
#[derive(Clone)]
pub(crate) struct Df {
    pub f: DateFormatter,
    pub kind: Option<i64>,
}

const BAD_CAL: &str = "Invalid value for calendar type; it must be one of IntlDateFormatter::TRADITIONAL (locale's default calendar) or IntlDateFormatter::GREGORIAN. Alternatively, it can be an IntlCalendar object";

fn peek_df(o: &ObjRef) -> Option<Df> {
    with_obj::<Df, Df>(o, |x| x.v.clone())
}

fn with_df<U>(o: &ObjRef, f: impl FnOnce(&mut Df) -> U) -> Option<U> {
    with_obj::<Df, U>(o, |x| f(&mut x.v))
}

fn this_df(vm: &mut Vm, a: &Args) -> R<(ObjRef, usize, Df)> {
    let (o, off) = this_or_first(a);
    match peek_df(&o) {
        Some(d) => {
            reset_obj::<Df>(vm, &o);
            Ok((o, off, d))
        }
        None => vm.throw("Error", "Found unconstructed IntlDateFormatter"),
    }
}

fn df_hooks() -> NativeHooks {
    NativeHooks {
        clone: Some(|vm, from, to| match peek_df(from) {
            Some(d) => super::store(to, d),
            None => clone_error(vm, "Cannot clone uninitialized IntlDateFormatter"),
        }),
        ..Default::default()
    }
}

/// A construction failure: (message, code, throw even from create()).
struct CtorErr(String, i64, bool);

/// The zone of a timezone argument (None: not given / null).
fn zone_of(vm: &mut Vm, v: &Value) -> R<Result<Option<TimeZone>, CtorErr>> {
    match v {
        Value::Null => Ok(Ok(None)),
        Value::Object(o) => {
            if is_instance(vm, o, "IntlTimeZone") {
                return Ok(match peek_itz(o) {
                    Some(t) => Ok(Some(t)),
                    None => Err(CtorErr(
                        "passed IntlTimeZone is not properly constructed".into(),
                        ue::U_ILLEGAL_ARGUMENT_ERROR,
                        true,
                    )),
                });
            }
            if is_instance(vm, o, "DateTimeZone") {
                let z = crate::builtins::date::get_tz(vm, o)?;
                return Ok(from_php_zone(&z)
                    .map(Some)
                    .map_err(|m| CtorErr(m, ue::U_ILLEGAL_ARGUMENT_ERROR, true)));
            }
            let s = vm.to_php_str(v)?;
            zone_of(vm, &Value::Str(s))
        }
        v => {
            let s = vm.to_php_str(v)?;
            let Ok(id) = std::str::from_utf8(s.as_bytes()) else {
                return Ok(Err(CtorErr(
                    "Time zone identifier given is not a valid UTF-8 string".into(),
                    ue::U_INVALID_CHAR_FOUND,
                    true,
                )));
            };
            Ok(match TimeZone::try_create(id) {
                Some(t) => Ok(Some(t)),
                None => Err(CtorErr(
                    format!("No such time zone: \"{id}\""),
                    ue::U_ILLEGAL_ARGUMENT_ERROR,
                    true,
                )),
            })
        }
    }
}

fn build(vm: &mut Vm, a: &Args) -> R<Result<Df, CtorErr>> {
    let locale = match a.opt_str(0) {
        Some(s) if !s.as_bytes().is_empty() => s.to_string_lossy(),
        _ => super::default_locale(vm),
    };
    let l = LocaleId::parse(&locale);
    if !l.base.is_empty() && l.valid_locale() == "root" {
        return vm.value_error(&format!(
            "{}(): Argument #1 ($locale) \"{}\" is invalid",
            a.fname(),
            locale
        ));
    }
    let ds = a.int_or(1, idf::FULL);
    let ts = a.int_or(2, idf::FULL);
    if ds == idf::PATTERN || ts == idf::PATTERN {
        if ds != ts {
            let which = if ds == idf::PATTERN {
                "time format must be UDAT_PATTERN if date format is UDAT_PATTERN"
            } else {
                "date format must be UDAT_PATTERN if time format is UDAT_PATTERN"
            };
            return Ok(Err(CtorErr(
                which.into(),
                ue::U_ILLEGAL_ARGUMENT_ERROR,
                true,
            )));
        }
    } else {
        if !idf::valid_style(ds, true) {
            return Ok(Err(CtorErr(
                "invalid date format style".into(),
                ue::U_ILLEGAL_ARGUMENT_ERROR,
                false,
            )));
        }
        if !idf::valid_style(ts, false) {
            return Ok(Err(CtorErr(
                "invalid time format style".into(),
                ue::U_ILLEGAL_ARGUMENT_ERROR,
                false,
            )));
        }
    }
    let zone = match zone_of(vm, &a.val(3))? {
        Ok(z) => z,
        Err(e) => return Ok(Err(e)),
    };
    let pattern = match a.opt_str(5) {
        Some(p) => match std::str::from_utf8(p.as_bytes()) {
            Ok(s) => Some(s.to_string()),
            Err(_) => {
                return Ok(Err(CtorErr(
                    "error converting pattern to UTF-16".into(),
                    ue::U_INVALID_CHAR_FOUND,
                    true,
                )));
            }
        },
        None => None,
    };
    let (cal, kind) = match a.val(4) {
        Value::Object(o) => match peek_cal(&o) {
            Some(mut c) => {
                if let Some(z) = zone.clone() {
                    c.set_time_zone(z);
                }
                (c, None)
            }
            None => {
                return Ok(Err(CtorErr(
                    "IntlCalendar object is not properly constructed".into(),
                    ue::U_ILLEGAL_ARGUMENT_ERROR,
                    false,
                )));
            }
        },
        v => {
            let k = match v {
                Value::Null => 1,
                _ => a.int(4),
            };
            if k != 0 && k != 1 {
                return Ok(Err(CtorErr(
                    BAD_CAL.into(),
                    ue::U_ILLEGAL_ARGUMENT_ERROR,
                    false,
                )));
            }
            let tz = zone.clone().unwrap_or_else(|| default_zone(vm));
            let forced = if k == 1 {
                Some(CalType::Gregorian)
            } else {
                None
            };
            (make_calendar(vm, forced, tz, &l.full()), Some(k))
        }
    };
    let f = DateFormatter::new(&l, ds, ts, pattern.as_deref(), l.calendar(), cal);
    Ok(Ok(Df { f, kind }))
}

fn df_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if has_obj::<Df>(&o) {
        return vm.throw(
            "IntlException",
            &format!("{}(): cannot call constructor twice", a.fname()),
        );
    }
    reset(vm);
    match build(vm, a)? {
        Ok(d) => {
            super::store(&o, d);
            Ok(Value::Null)
        }
        Err(CtorErr(m, code, _)) => {
            let msg = format!("{}(): {}", a.fname(), m);
            super::set_error_quiet(vm, code, msg.clone());
            vm.throw("IntlException", &msg)
        }
    }
}

fn df_create(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    match build(vm, a)? {
        Ok(d) => {
            let cls = if a.this.is_none() && a.static_class.is_null() {
                super::class(vm, "IntlDateFormatter")?
            } else {
                super::static_class(vm, a)
            };
            let o = vm.instantiate_raw(&cls)?;
            super::store(&o, d);
            Ok(Value::Object(o))
        }
        Err(CtorErr(m, code, throw)) => {
            if throw {
                let msg = format!("{}(): {}", a.fname(), m);
                super::set_error_quiet(vm, code, msg.clone());
                return vm.throw("IntlException", &msg);
            }
            fail(vm, a, code, &m)?;
            Ok(Value::Null)
        }
    }
}

// ---------------------------------------------------------------- getters/setters

fn df_get_datetype(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, d) = this_df(vm, a)?;
    Ok(Value::Int(d.f.date_style))
}

fn df_get_timetype(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, d) = this_df(vm, a)?;
    Ok(Value::Int(d.f.time_style))
}

fn df_get_calendar(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, d) = this_df(vm, a)?;
    Ok(match d.kind {
        Some(k) => Value::Int(k),
        None => Value::Bool(false),
    })
}

fn df_set_calendar(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, d) = this_df(vm, a)?;
    match a.val(off) {
        Value::Object(co) => {
            let Some(c) = peek_cal(&co) else {
                fail_obj::<Df>(
                    vm,
                    a,
                    &o,
                    ue::U_ILLEGAL_ARGUMENT_ERROR,
                    "IntlCalendar object is not properly constructed",
                )?;
                return Ok(Value::Bool(false));
            };
            with_df(&o, |x| {
                x.f.calendar = c;
                x.kind = None;
            });
        }
        v => {
            let k = match v {
                Value::Null => 1,
                _ => a.int(off),
            };
            if k != 0 && k != 1 {
                fail_obj::<Df>(vm, a, &o, ue::U_ILLEGAL_ARGUMENT_ERROR, BAD_CAL)?;
                return Ok(Value::Bool(false));
            }
            let tz = d.f.calendar.time_zone().clone();
            let forced = if k == 1 {
                Some(CalType::Gregorian)
            } else {
                None
            };
            let c = make_calendar(vm, forced, tz, &d.f.locale.full());
            with_df(&o, |x| {
                x.f.calendar = c;
                x.kind = Some(k);
            });
        }
    }
    Ok(Value::Bool(true))
}

fn df_get_timezone_id(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, d) = this_df(vm, a)?;
    Ok(Value::from(d.f.time_zone().id()))
}

fn df_get_timezone(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, d) = this_df(vm, a)?;
    new_itz(vm, d.f.time_zone().clone())
}

fn df_get_calendar_object(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, d) = this_df(vm, a)?;
    new_cal_obj(vm, d.f.calendar.clone())
}

fn df_set_timezone(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, _) = this_df(vm, a)?;
    let z = match zone_of(vm, &a.val(off))? {
        Ok(Some(z)) => z,
        Ok(None) => default_zone(vm),
        Err(CtorErr(m, code, _)) => {
            let msg = format!("{}(): {}", a.fname(), m);
            super::set_error_quiet(vm, code, msg.clone());
            return vm.throw("IntlException", &msg);
        }
    };
    with_df(&o, |x| x.f.calendar.set_time_zone(z));
    Ok(Value::Bool(true))
}

fn df_set_pattern(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, _) = this_df(vm, a)?;
    let Ok(p) = std::str::from_utf8(a.bytes(off)) else {
        fail_obj::<Df>(
            vm,
            a,
            &o,
            ue::U_INVALID_CHAR_FOUND,
            "Error converting pattern to UTF-16",
        )?;
        return Ok(Value::Bool(false));
    };
    let p = p.to_string();
    with_df(&o, |x| x.f.set_pattern(&p));
    Ok(Value::Bool(true))
}

fn df_get_pattern(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, d) = this_df(vm, a)?;
    Ok(Value::from(d.f.pattern()))
}

fn df_get_locale(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, d) = this_df(vm, a)?;
    match a.int_or(off, 0) {
        0 => Ok(Value::from(d.f.locale.actual_locale().as_str())),
        1 => Ok(Value::from(d.f.locale.valid_locale().as_str())),
        _ => {
            fail_obj::<Df>(
                vm,
                a,
                &o,
                ue::U_ILLEGAL_ARGUMENT_ERROR,
                "invalid locale type",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn df_set_lenient(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, _) = this_df(vm, a)?;
    let l = a.bool(off);
    with_df(&o, |x| {
        x.f.lenient = l;
        x.f.calendar.set_lenient(l);
    });
    Ok(Value::Null)
}

fn df_is_lenient(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, d) = this_df(vm, a)?;
    Ok(Value::Bool(d.f.lenient))
}

fn df_error_code(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_or_first(a);
    Ok(Value::Int(obj_err::<Df>(&o).code))
}

fn df_error_message(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_or_first(a);
    Ok(Value::from(obj_err::<Df>(&o).full_message().as_str()))
}

// ---------------------------------------------------------------- formatting

/// The instant of a DateTimeInterface object (ms).
fn date_ms(vm: &mut Vm, o: &ObjRef) -> R<(f64, goro_ext_date::TimeZone)> {
    let dt = crate::builtins::date::get_dt(vm, o)?;
    Ok((dt.ts as f64 * 1000.0 + (dt.us / 1000) as f64, dt.zone))
}

fn localtime_cal(vm: &mut Vm, d: &Df, arr: &PhpArray) -> R<Calendar> {
    let _ = vm;
    let mut c = d.f.calendar.clone();
    c.clear();
    let get = |k: &str| match arr.get_str(&goro_runtime::PhpStr::from(k)) {
        Some(v) => match v.deref() {
            Value::Int(i) => i,
            Value::Float(f) => f as i64,
            Value::Bool(b) => b as i64,
            Value::Str(s) => s.to_string_lossy().trim().parse().unwrap_or(0),
            _ => 0,
        },
        None => 0,
    };
    let clamp = |v: i64| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    c.set(ic::YEAR, clamp(get("tm_year") + 1900));
    c.set(ic::MONTH, clamp(get("tm_mon")));
    c.set(ic::DATE, clamp(get("tm_mday")));
    c.set(ic::HOUR_OF_DAY, clamp(get("tm_hour")));
    c.set(ic::MINUTE, clamp(get("tm_min")));
    c.set(ic::SECOND, clamp(get("tm_sec")));
    Ok(c)
}

fn df_format(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, d) = this_df(vm, a)?;
    let v = a.val(off);
    let ms = match &v {
        Value::Int(i) => *i as f64 * 1000.0,
        Value::Float(f) => *f * 1000.0,
        Value::Array(arr) => {
            let mut c = localtime_cal(vm, &d, arr)?;
            if c.complete().is_err() {
                fail_obj::<Df>(
                    vm,
                    a,
                    &o,
                    ue::U_ILLEGAL_ARGUMENT_ERROR,
                    "Date formatting failed",
                )?;
                return Ok(Value::Bool(false));
            }
            return Ok(Value::from(d.f.format_cal(&c).as_str()));
        }
        Value::Object(obj) => {
            if is_instance(vm, obj, "IntlCalendar") {
                match peek_cal(obj) {
                    Some(mut c) => match c.get_time() {
                        Ok(t) => t,
                        Err(_) => {
                            fail_obj::<Df>(
                                vm,
                                a,
                                &o,
                                ue::U_ILLEGAL_ARGUMENT_ERROR,
                                "error retrieving time from IntlCalendar",
                            )?;
                            return Ok(Value::Bool(false));
                        }
                    },
                    None => {
                        fail_obj::<Df>(
                            vm,
                            a,
                            &o,
                            ue::U_ILLEGAL_ARGUMENT_ERROR,
                            "bad IntlCalendar instance: not initialized properly",
                        )?;
                        return Ok(Value::Bool(false));
                    }
                }
            } else if is_instance(vm, obj, "DateTimeInterface") {
                date_ms(vm, obj)?.0
            } else {
                fail_obj::<Df>(
                    vm,
                    a,
                    &o,
                    ue::U_ILLEGAL_ARGUMENT_ERROR,
                    "invalid object type for date/time (only IntlCalendar and DateTimeInterface permitted)",
                )?;
                return Ok(Value::Bool(false));
            }
        }
        Value::Str(s) => match s.to_string_lossy().trim().parse::<f64>() {
            Ok(f) => f * 1000.0,
            Err(_) => {
                fail_obj::<Df>(
                    vm,
                    a,
                    &o,
                    ue::U_ILLEGAL_ARGUMENT_ERROR,
                    "invalid argument type",
                )?;
                return Ok(Value::Bool(false));
            }
        },
        Value::Bool(b) => *b as i64 as f64 * 1000.0,
        _ => 0.0,
    };
    match d.f.format_ms(ms) {
        Some(s) => Ok(Value::from(s.as_str())),
        None => {
            fail_obj::<Df>(
                vm,
                a,
                &o,
                ue::U_ILLEGAL_ARGUMENT_ERROR,
                "Date formatting failed",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn df_format_object(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let Value::Object(obj) = a.val(0) else {
        fail(
            vm,
            a,
            ue::U_ILLEGAL_ARGUMENT_ERROR,
            "the passed object must be an instance of either IntlCalendar or DateTimeInterface",
        )?;
        return Ok(Value::Bool(false));
    };
    let locale = match a.opt_str(2) {
        Some(s) if !s.as_bytes().is_empty() => s.to_string_lossy(),
        _ => super::default_locale(vm),
    };
    let l = LocaleId::parse(&locale);
    let cal = if is_instance(vm, &obj, "IntlCalendar") {
        match peek_cal(&obj) {
            Some(c) => c,
            None => {
                fail(
                    vm,
                    a,
                    ue::U_ILLEGAL_ARGUMENT_ERROR,
                    "bad IntlCalendar instance: not initialized properly",
                )?;
                return Ok(Value::Bool(false));
            }
        }
    } else if is_instance(vm, &obj, "DateTimeInterface") {
        let (ms, z) = date_ms(vm, &obj)?;
        let tz = match from_php_zone(&z) {
            Ok(t) => t,
            Err(m) => {
                fail(vm, a, ue::U_ILLEGAL_ARGUMENT_ERROR, &m)?;
                return Ok(Value::Bool(false));
            }
        };
        let mut c = make_calendar(vm, Some(CalType::Gregorian), tz, &l.base);
        let _ = c.set_time(ms);
        c
    } else {
        let msg = format!(
            "{}(): the passed object must be an instance of either IntlCalendar or DateTimeInterface",
            a.fname()
        );
        super::set_error_quiet(vm, ue::U_ZERO_ERROR, msg);
        return Ok(Value::Bool(false));
    };
    let valid = |v: &Value| match v.deref() {
        Value::Int(i) => idf::valid_style(i, true).then_some(i),
        _ => None,
    };
    let (ds, ts, pattern) = match a.val(1) {
        Value::Null => (idf::MEDIUM, idf::MEDIUM, None),
        Value::Int(i) => {
            if !idf::valid_style(i, true) || i >= idf::RELATIVE {
                fail(
                    vm,
                    a,
                    ue::U_ILLEGAL_ARGUMENT_ERROR,
                    "the date/time format type is invalid",
                )?;
                return Ok(Value::Bool(false));
            }
            (i, i, None)
        }
        Value::Array(arr) => {
            if arr.len() != 2 {
                fail(
                    vm,
                    a,
                    ue::U_ILLEGAL_ARGUMENT_ERROR,
                    "bad format; if array, it must have two elements",
                )?;
                return Ok(Value::Bool(false));
            }
            let vals: Vec<Value> = arr.iter().map(|(_, v)| v.clone()).collect();
            let Some(d) = valid(&vals[0]) else {
                fail(
                    vm,
                    a,
                    ue::U_ILLEGAL_ARGUMENT_ERROR,
                    "bad format; the date format (first element of the array) is not valid",
                )?;
                return Ok(Value::Bool(false));
            };
            let t = match valid(&vals[1]) {
                Some(t) if t < idf::RELATIVE => t,
                _ => {
                    fail(
                        vm,
                        a,
                        ue::U_ILLEGAL_ARGUMENT_ERROR,
                        "bad format; the time format (second element of the array) is not valid",
                    )?;
                    return Ok(Value::Bool(false));
                }
            };
            (d, t, None)
        }
        v => {
            let s = vm.to_php_str(&v)?.to_string_lossy();
            if s.is_empty() {
                fail(vm, a, ue::U_ILLEGAL_ARGUMENT_ERROR, "the format is empty")?;
                return Ok(Value::Bool(false));
            }
            (idf::NONE, idf::NONE, Some(s))
        }
    };
    let pattern_cal = LocaleId::parse(&l.base).calendar();
    let mut c = cal;
    let f = DateFormatter::new(&l, ds, ts, pattern.as_deref(), pattern_cal, c.clone());
    if c.complete().is_err() {
        fail(vm, a, ue::U_ILLEGAL_ARGUMENT_ERROR, "error formatting date")?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::from(f.format_cal(&c).as_str()))
}

// ---------------------------------------------------------------- parsing

/// The start offset of a parse (from the by-reference argument).
fn start_offset(vm: &mut Vm, a: &Args, i: usize, strict_int: bool) -> R<usize> {
    let v = a.val(i);
    Ok(match v {
        Value::Null => 0,
        Value::Int(n) => n.max(0) as usize,
        Value::Float(f) => {
            if strict_int && !(f >= i64::MIN as f64 && f < i64::MAX as f64) {
                let s = format!("{f:E}").replace("E", "E+").replace("E+-", "E-");
                vm.warn(&format!(
                    "The float {s} is not representable as an int, cast occurred"
                ))?;
            }
            (f as i64).max(0) as usize
        }
        Value::Bool(b) => b as usize,
        Value::Str(s) if strict_int => {
            let _ = s;
            return vm.throw(
                "TypeError",
                &format!(
                    "{}(): Argument #{} ($offset) must be of type int, string given",
                    a.fname(),
                    i + 1
                ),
            );
        }
        other => vm
            .to_php_str(&other)?
            .to_string_lossy()
            .trim()
            .parse::<i64>()
            .unwrap_or(0)
            .max(0) as usize,
    })
}

/// Byte offset of UTF-16 position `u` in `s`, and back.
fn byte_of_u16(s: &str, u: usize) -> usize {
    let mut n = 0;
    for (b, c) in s.char_indices() {
        if n >= u {
            return b;
        }
        n += c.len_utf16();
    }
    s.len()
}

fn u16_of_byte(s: &str, b: usize) -> usize {
    s[..b.min(s.len())].chars().map(|c| c.len_utf16()).sum()
}

/// Runs a parse: Ok(calendar) or Ok(None) after setting the error.
fn run_parse(
    vm: &mut Vm,
    a: &Args,
    o: &ObjRef,
    d: &Df,
    off: usize,
    strict: bool,
) -> R<Option<(Calendar, bool)>> {
    let text = String::from_utf8_lossy(a.bytes(off)).into_owned();
    let has_off = a.has(off + 1);
    let start = if has_off {
        start_offset(vm, a, off + 1, strict)?
    } else {
        0
    };
    let mut pos = byte_of_u16(&text, start);
    match d.f.parse(&text, &mut pos) {
        Ok(r) => {
            if has_off {
                set_ref_typed(vm, a, off + 1, Value::Int(u16_of_byte(&text, pos) as i64))?;
            }
            Ok(Some(r))
        }
        Err(e) => {
            if has_off {
                set_ref_typed(vm, a, off + 1, Value::Int(u16_of_byte(&text, e) as i64))?;
            }
            fail_obj::<Df>(vm, a, o, ue::U_PARSE_ERROR, "Date parsing failed")?;
            Ok(None)
        }
    }
}

fn ts_value(ms: f64) -> Value {
    let s = ms / 1000.0;
    if s.fract() == 0.0 && s.abs() < 9.0e18 {
        Value::Int(s as i64)
    } else {
        Value::Float(s)
    }
}

fn df_parse(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, d) = this_df(vm, a)?;
    match run_parse(vm, a, &o, &d, off, false)? {
        Some((mut c, _)) => match c.get_time() {
            Ok(ms) => Ok(ts_value(ms)),
            Err(_) => Ok(Value::Bool(false)),
        },
        None => Ok(Value::Bool(false)),
    }
}

fn df_parse_to_calendar(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, d) = this_df(vm, a)?;
    match run_parse(vm, a, &o, &d, off, true)? {
        Some((mut c, _)) => match c.get_time() {
            Ok(ms) => {
                with_df(&o, |x| x.f.calendar = c);
                Ok(ts_value(ms))
            }
            Err(_) => Ok(Value::Bool(false)),
        },
        None => Ok(Value::Bool(false)),
    }
}

fn df_localtime(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, d) = this_df(vm, a)?;
    let Some((mut c, _)) = run_parse(vm, a, &o, &d, off, false)? else {
        return Ok(Value::Bool(false));
    };
    if c.complete().is_err() {
        return Ok(Value::Bool(false));
    }
    let mut out = PhpArray::new();
    let mut put = |k: &str, v: i64| {
        out.insert(Key::from(k), Value::Int(v));
    };
    put("tm_sec", c.field(ic::SECOND) as i64);
    put("tm_min", c.field(ic::MINUTE) as i64);
    put("tm_hour", c.field(ic::HOUR_OF_DAY) as i64);
    put("tm_year", c.field(ic::YEAR) as i64 - 1900);
    put("tm_mday", c.field(ic::DATE) as i64);
    put("tm_wday", c.field(ic::DAY_OF_WEEK) as i64 - 1);
    // the day of the year as ICU counts it, from 1 (expected output of
    // ext/intl/tests/dateformat_format_parse_version_icu72-1.phpt)
    put("tm_yday", c.field(ic::DAY_OF_YEAR) as i64);
    put("tm_mon", c.field(ic::MONTH) as i64);
    put("tm_isdst", (c.field(ic::DST_OFFSET) != 0) as i64);
    Ok(Value::Array(out))
}

#[allow(dead_code)]
const _SIG: &str = SIG_ARGS;
