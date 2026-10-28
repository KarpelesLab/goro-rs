//! `IntlCalendar`, `IntlGregorianCalendar` and the `intlcal_*()` /
//! `intlgregcal_*()` functions (PHP manual: IntlCalendar,
//! IntlGregorianCalendar). The calendar logic is
//! goro_ext_intl::calendar's.

use goro_ext_intl::calendar::{self as ic, CalType, Calendar};
use goro_ext_intl::datefmt::LocaleId;
use goro_ext_intl::datefmt::locale as iloc;
use goro_ext_intl::error as ue;
use goro_ext_intl::timezone::{TimeZone, WallTime};
use goro_runtime::{Key, ObjRef, PhpArray, Value};

use super::timezone::{
    clone_error, default_zone, from_php_zone, is_instance, new_itz, new_string_iterator, peek_itz,
    to_php_zone_obj,
};
use super::{
    already_constructed, fail, fail_obj, has_obj, new_obj, obj_err, reset, reset_obj,
    this_or_first, with_obj,
};
use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "intl",
    functions: &[
        (
            "function intlcal_create_instance(IntlTimeZone|DateTimeZone|string|null $timezone = null, ?string $locale = null): ?IntlCalendar",
            cal_create_instance,
        ),
        (
            "function intlcal_get_keyword_values_for_locale(string $keyword, string $locale, bool $onlyCommon): IntlIterator|false",
            cal_keyword_values,
        ),
        ("function intlcal_get_now(): float", cal_get_now),
        (
            "function intlcal_get_available_locales(): array",
            cal_available_locales,
        ),
        (
            "function intlcal_get(IntlCalendar $calendar, int $field): int|false",
            cal_get,
        ),
        (
            "function intlcal_get_time(IntlCalendar $calendar): float|false",
            cal_get_time,
        ),
        (
            "function intlcal_set_time(IntlCalendar $calendar, float $timestamp): bool",
            cal_set_time,
        ),
        (
            "function intlcal_add(IntlCalendar $calendar, int $field, int $value): bool",
            cal_add,
        ),
        (
            "function intlcal_set_time_zone(IntlCalendar $calendar, IntlTimeZone|DateTimeZone|string|null $timezone): bool",
            cal_set_time_zone,
        ),
        (
            "function intlcal_after(IntlCalendar $calendar, IntlCalendar $other): bool",
            cal_after,
        ),
        (
            "function intlcal_before(IntlCalendar $calendar, IntlCalendar $other): bool",
            cal_before,
        ),
        (
            "function intlcal_set(IntlCalendar $calendar, int $year, int $month, int ...$rest): true",
            cal_set,
        ),
        (
            "function intlcal_roll(IntlCalendar $calendar, int $field, int|bool $value = 0): bool",
            cal_roll,
        ),
        (
            "function intlcal_clear(IntlCalendar $calendar, ?int $field = null): bool",
            cal_clear,
        ),
        (
            "function intlcal_field_difference(IntlCalendar $calendar, float $timestamp, int $field): int|false",
            cal_field_difference,
        ),
        (
            "function intlcal_get_actual_maximum(IntlCalendar $calendar, int $field): int|false",
            cal_actual_max,
        ),
        (
            "function intlcal_get_actual_minimum(IntlCalendar $calendar, int $field): int|false",
            cal_actual_min,
        ),
        (
            "function intlcal_get_day_of_week_type(IntlCalendar $calendar, int $dayOfWeek): int|false",
            cal_dow_type,
        ),
        (
            "function intlcal_get_first_day_of_week(IntlCalendar $calendar): int|false",
            cal_get_first_dow,
        ),
        (
            "function intlcal_get_least_maximum(IntlCalendar $calendar, int $field): int|false",
            cal_least_max,
        ),
        (
            "function intlcal_get_greatest_minimum(IntlCalendar $calendar, int $field): int|false",
            cal_greatest_min,
        ),
        (
            "function intlcal_get_locale(IntlCalendar $calendar, int $type): string|false",
            cal_get_locale,
        ),
        (
            "function intlcal_get_maximum(IntlCalendar $calendar, int $field): int|false",
            cal_max,
        ),
        (
            "function intlcal_get_minimal_days_in_first_week(IntlCalendar $calendar): int|false",
            cal_get_min_days,
        ),
        (
            "function intlcal_set_minimal_days_in_first_week(IntlCalendar $calendar, int $days): bool",
            cal_set_min_days,
        ),
        (
            "function intlcal_get_minimum(IntlCalendar $calendar, int $field): int|false",
            cal_min,
        ),
        (
            "function intlcal_get_time_zone(IntlCalendar $calendar): IntlTimeZone|false",
            cal_get_time_zone,
        ),
        (
            "function intlcal_get_type(IntlCalendar $calendar): string",
            cal_get_type,
        ),
        (
            "function intlcal_get_weekend_transition(IntlCalendar $calendar, int $dayOfWeek): int|false",
            cal_weekend_transition,
        ),
        (
            "function intlcal_in_daylight_time(IntlCalendar $calendar): bool",
            cal_in_daylight,
        ),
        (
            "function intlcal_is_lenient(IntlCalendar $calendar): bool",
            cal_is_lenient,
        ),
        (
            "function intlcal_is_set(IntlCalendar $calendar, int $field): bool",
            cal_is_set,
        ),
        (
            "function intlcal_is_equivalent_to(IntlCalendar $calendar, IntlCalendar $other): bool",
            cal_is_equivalent,
        ),
        (
            "function intlcal_is_weekend(IntlCalendar $calendar, ?float $timestamp = null): bool",
            cal_is_weekend,
        ),
        (
            "function intlcal_set_first_day_of_week(IntlCalendar $calendar, int $dayOfWeek): bool",
            cal_set_first_dow,
        ),
        (
            "function intlcal_set_lenient(IntlCalendar $calendar, bool $lenient): bool",
            cal_set_lenient,
        ),
        (
            "function intlcal_get_repeated_wall_time_option(IntlCalendar $calendar): int",
            cal_get_repeated,
        ),
        (
            "function intlcal_equals(IntlCalendar $calendar, IntlCalendar $other): bool",
            cal_equals,
        ),
        (
            "function intlcal_get_skipped_wall_time_option(IntlCalendar $calendar): int",
            cal_get_skipped,
        ),
        (
            "function intlcal_set_repeated_wall_time_option(IntlCalendar $calendar, int $option): bool",
            cal_set_repeated,
        ),
        (
            "function intlcal_set_skipped_wall_time_option(IntlCalendar $calendar, int $option): bool",
            cal_set_skipped,
        ),
        (
            "function intlcal_from_date_time(DateTime|string $datetime, ?string $locale = null): ?IntlCalendar",
            cal_from_date_time,
        ),
        (
            "function intlcal_to_date_time(IntlCalendar $calendar): DateTime|false",
            cal_to_date_time,
        ),
        (
            "function intlcal_get_error_code(IntlCalendar $calendar): int|false",
            cal_error_code,
        ),
        (
            "function intlcal_get_error_message(IntlCalendar $calendar): string|false",
            cal_error_message,
        ),
        (
            "function intlgregcal_create_instance(...$args): ?IntlGregorianCalendar",
            greg_create_instance,
        ),
        (
            "function intlgregcal_set_gregorian_change(IntlGregorianCalendar $calendar, float $timestamp): bool",
            greg_set_change,
        ),
        (
            "function intlgregcal_get_gregorian_change(IntlGregorianCalendar $calendar): float",
            greg_get_change,
        ),
        (
            "function intlgregcal_is_leap_year(IntlGregorianCalendar $calendar, int $year): bool",
            greg_is_leap,
        ),
    ],
    classes: &[
        ("intlcalendar", CAL_CLASS),
        ("intlgregoriancalendar", GREG_CLASS),
    ],
    methods: &[
        ("intlcalendar::createinstance", cal_create_instance),
        ("intlcalendar::equals", cal_equals),
        ("intlcalendar::fielddifference", cal_field_difference),
        ("intlcalendar::add", cal_add),
        ("intlcalendar::after", cal_after),
        ("intlcalendar::before", cal_before),
        ("intlcalendar::clear", cal_clear),
        ("intlcalendar::fromdatetime", cal_from_date_time),
        ("intlcalendar::get", cal_get),
        ("intlcalendar::getactualmaximum", cal_actual_max),
        ("intlcalendar::getactualminimum", cal_actual_min),
        ("intlcalendar::getavailablelocales", cal_available_locales),
        ("intlcalendar::getdayofweektype", cal_dow_type),
        ("intlcalendar::geterrorcode", cal_error_code),
        ("intlcalendar::geterrormessage", cal_error_message),
        ("intlcalendar::getfirstdayofweek", cal_get_first_dow),
        ("intlcalendar::getgreatestminimum", cal_greatest_min),
        (
            "intlcalendar::getkeywordvaluesforlocale",
            cal_keyword_values,
        ),
        ("intlcalendar::getleastmaximum", cal_least_max),
        ("intlcalendar::getlocale", cal_get_locale),
        ("intlcalendar::getmaximum", cal_max),
        ("intlcalendar::getminimaldaysinfirstweek", cal_get_min_days),
        ("intlcalendar::setminimaldaysinfirstweek", cal_set_min_days),
        ("intlcalendar::getminimum", cal_min),
        ("intlcalendar::getnow", cal_get_now),
        ("intlcalendar::getrepeatedwalltimeoption", cal_get_repeated),
        ("intlcalendar::getskippedwalltimeoption", cal_get_skipped),
        ("intlcalendar::gettime", cal_get_time),
        ("intlcalendar::gettimezone", cal_get_time_zone),
        ("intlcalendar::gettype", cal_get_type),
        ("intlcalendar::getweekendtransition", cal_weekend_transition),
        ("intlcalendar::indaylighttime", cal_in_daylight),
        ("intlcalendar::isequivalentto", cal_is_equivalent),
        ("intlcalendar::islenient", cal_is_lenient),
        ("intlcalendar::isweekend", cal_is_weekend),
        ("intlcalendar::roll", cal_roll),
        ("intlcalendar::isset", cal_is_set),
        ("intlcalendar::set", cal_set),
        ("intlcalendar::setdate", cal_set_date),
        ("intlcalendar::setdatetime", cal_set_date_time),
        ("intlcalendar::setfirstdayofweek", cal_set_first_dow),
        ("intlcalendar::setlenient", cal_set_lenient),
        ("intlcalendar::setrepeatedwalltimeoption", cal_set_repeated),
        ("intlcalendar::setskippedwalltimeoption", cal_set_skipped),
        ("intlcalendar::settime", cal_set_time),
        ("intlcalendar::settimezone", cal_set_time_zone),
        ("intlcalendar::todatetime", cal_to_date_time),
        ("intlgregoriancalendar::__construct", greg_construct),
        (
            "intlgregoriancalendar::createfromdate",
            greg_create_from_date,
        ),
        (
            "intlgregoriancalendar::createfromdatetime",
            greg_create_from_date_time,
        ),
        ("intlgregoriancalendar::setgregorianchange", greg_set_change),
        ("intlgregoriancalendar::getgregorianchange", greg_get_change),
        ("intlgregoriancalendar::isleapyear", greg_is_leap),
    ],
    hooks: &[("intlcalendar", cal_hooks)],
    ..Module::EMPTY
};

const CAL_CLASS: &str = r#"<?php class IntlCalendar {
    public const int FIELD_ERA = 0;
    public const int FIELD_YEAR = 1;
    public const int FIELD_MONTH = 2;
    public const int FIELD_WEEK_OF_YEAR = 3;
    public const int FIELD_WEEK_OF_MONTH = 4;
    public const int FIELD_DATE = 5;
    public const int FIELD_DAY_OF_YEAR = 6;
    public const int FIELD_DAY_OF_WEEK = 7;
    public const int FIELD_DAY_OF_WEEK_IN_MONTH = 8;
    public const int FIELD_AM_PM = 9;
    public const int FIELD_HOUR = 10;
    public const int FIELD_HOUR_OF_DAY = 11;
    public const int FIELD_MINUTE = 12;
    public const int FIELD_SECOND = 13;
    public const int FIELD_MILLISECOND = 14;
    public const int FIELD_ZONE_OFFSET = 15;
    public const int FIELD_DST_OFFSET = 16;
    public const int FIELD_YEAR_WOY = 17;
    public const int FIELD_DOW_LOCAL = 18;
    public const int FIELD_EXTENDED_YEAR = 19;
    public const int FIELD_JULIAN_DAY = 20;
    public const int FIELD_MILLISECONDS_IN_DAY = 21;
    public const int FIELD_IS_LEAP_MONTH = 22;
    public const int FIELD_FIELD_COUNT = 24;
    public const int FIELD_DAY_OF_MONTH = 5;
    public const int DOW_SUNDAY = 1;
    public const int DOW_MONDAY = 2;
    public const int DOW_TUESDAY = 3;
    public const int DOW_WEDNESDAY = 4;
    public const int DOW_THURSDAY = 5;
    public const int DOW_FRIDAY = 6;
    public const int DOW_SATURDAY = 7;
    public const int DOW_TYPE_WEEKDAY = 0;
    public const int DOW_TYPE_WEEKEND = 1;
    public const int DOW_TYPE_WEEKEND_OFFSET = 2;
    public const int DOW_TYPE_WEEKEND_CEASE = 3;
    public const int WALLTIME_FIRST = 1;
    public const int WALLTIME_LAST = 0;
    public const int WALLTIME_NEXT_VALID = 2;
    private function __construct() {}
    public static function createInstance(IntlTimeZone|DateTimeZone|string|null $timezone = null, ?string $locale = null): ?IntlCalendar {}
    public function equals(IntlCalendar $other): bool {}
    public function fieldDifference(float $timestamp, int $field): int|false {}
    public function add(int $field, int $value): true {}
    public function after(IntlCalendar $other): bool {}
    public function before(IntlCalendar $other): bool {}
    public function clear(?int $field = null): true {}
    public static function fromDateTime(DateTime|string $datetime, ?string $locale = null): ?IntlCalendar {}
    public function get(int $field): int|false {}
    public function getActualMaximum(int $field): int|false {}
    public function getActualMinimum(int $field): int|false {}
    public static function getAvailableLocales(): array {}
    public function getDayOfWeekType(int $dayOfWeek): int|false {}
    public function getErrorCode(): int|false {}
    public function getErrorMessage(): string|false {}
    public function getFirstDayOfWeek(): int|false {}
    public function getGreatestMinimum(int $field): int|false {}
    public static function getKeywordValuesForLocale(string $keyword, string $locale, bool $onlyCommon): IntlIterator|false {}
    public function getLeastMaximum(int $field): int|false {}
    public function getLocale(int $type): string|false {}
    public function getMaximum(int $field): int|false {}
    public function getMinimalDaysInFirstWeek(): int|false {}
    public function setMinimalDaysInFirstWeek(int $days): true {}
    public function getMinimum(int $field): int|false {}
    public static function getNow(): float {}
    public function getRepeatedWallTimeOption(): int {}
    public function getSkippedWallTimeOption(): int {}
    public function getTime(): float|false {}
    public function getTimeZone(): IntlTimeZone|false {}
    public function getType(): string {}
    public function getWeekendTransition(int $dayOfWeek): int|false {}
    public function inDaylightTime(): bool {}
    public function isEquivalentTo(IntlCalendar $other): bool {}
    public function isLenient(): bool {}
    public function isWeekend(?float $timestamp = null): bool {}
    public function roll(int $field, int|bool $value = 0): true {}
    public function isSet(int $field): bool {}
    public function set(int $year, int $month, int ...$rest): true {}
    public function setDate(int $year, int $month, int $dayOfMonth): void {}
    public function setDateTime(int $year, int $month, int $dayOfMonth, int $hour, int $minute, ?int $second = null): void {}
    public function setFirstDayOfWeek(int $dayOfWeek): true {}
    public function setLenient(bool $lenient): true {}
    public function setRepeatedWallTimeOption(int $option): true {}
    public function setSkippedWallTimeOption(int $option): true {}
    public function setTime(float $timestamp): true {}
    public function setTimeZone(IntlTimeZone|DateTimeZone|string|null $timezone): bool {}
    public function toDateTime(): DateTime|false {}
}"#;

const GREG_CLASS: &str = r#"<?php class IntlGregorianCalendar extends IntlCalendar {
    public function __construct(...$args) {}
    public static function createFromDate(int $year, int $month, int $dayOfMonth): static {}
    public static function createFromDateTime(int $year, int $month, int $dayOfMonth, int $hour, int $minute, ?int $second = null): static {}
    public function setGregorianChange(float $timestamp): bool {}
    public function getGregorianChange(): float {}
    public function isLeapYear(int $year): bool {}
}"#;

// ---------------------------------------------------------------- helpers

pub(crate) fn peek_cal(o: &ObjRef) -> Option<Calendar> {
    with_obj::<Calendar, Calendar>(o, |x| x.v.clone())
}

fn with_cal<U>(o: &ObjRef, f: impl FnOnce(&mut Calendar) -> U) -> Option<U> {
    with_obj::<Calendar, U>(o, |x| f(&mut x.v))
}

fn this_cal(vm: &mut Vm, a: &Args) -> R<(ObjRef, usize)> {
    let (o, off) = this_or_first(a);
    if !has_obj::<Calendar>(&o) {
        return vm.throw("Error", "Found unconstructed IntlCalendar");
    }
    reset_obj::<Calendar>(vm, &o);
    Ok((o, off))
}

/// The calendar of locale `locale` (ICU style id, keywords allowed) in
/// zone `tz`, of type `forced` or the locale's.
pub(crate) fn make_calendar(
    vm: &mut Vm,
    forced: Option<CalType>,
    tz: TimeZone,
    locale: &str,
) -> Calendar {
    let mut l = LocaleId::parse(locale);
    if l.base.is_empty() {
        l.base = LocaleId::parse(&super::default_locale(vm)).base;
    }
    let ct = forced.unwrap_or_else(|| l.calendar());
    let region = l.region_or_likely();
    let mut c = Calendar::new(ct, tz, &region, goro_ext_intl::timezone::now_ms());
    c.valid_locale = if l.region().is_none() && !l.base.is_empty() {
        let v = l.valid_locale();
        if v == "root" {
            v
        } else {
            format!("{}_{}", v, region)
        }
    } else {
        l.valid_locale()
    };
    c.actual_locale = l.actual_locale();
    c
}

/// A new IntlCalendar / IntlGregorianCalendar object.
pub(crate) fn new_cal_obj(vm: &mut Vm, c: Calendar) -> R<Value> {
    let cls = if c.cal_type == CalType::Gregorian {
        "IntlGregorianCalendar"
    } else {
        "IntlCalendar"
    };
    Ok(Value::Object(new_obj(vm, cls, c)?))
}

/// A time zone argument (IntlTimeZone, DateTimeZone, id, null for the
/// default): Ok(Err(message)) for unusable values.
pub(crate) fn zone_arg(vm: &mut Vm, a: &Args, i: usize) -> R<Result<TimeZone, String>> {
    match a.val(i) {
        Value::Null => Ok(Ok(default_zone(vm))),
        Value::Object(o) => {
            if is_instance(vm, &o, "IntlTimeZone") {
                match peek_itz(&o) {
                    Some(t) => Ok(Ok(t)),
                    None => {
                        let m = format!(
                            "{}(): passed IntlTimeZone is not properly constructed",
                            a.fname()
                        );
                        super::set_error_quiet(vm, ue::U_ILLEGAL_ARGUMENT_ERROR, m.clone());
                        vm.throw("IntlException", &m)
                    }
                }
            } else {
                let z = crate::builtins::date::get_tz(vm, &o)?;
                Ok(from_php_zone(&z))
            }
        }
        v => {
            let s = vm.to_php_str(&v)?.to_string_lossy();
            match TimeZone::try_create(&s) {
                Some(t) => Ok(Ok(t)),
                None => Ok(Err(format!("No such time zone: '{s}'"))),
            }
        }
    }
}

fn field_err<T>(vm: &mut Vm, a: &Args, argno: usize, name: &str, msg: &str) -> R<T> {
    vm.value_error(&format!(
        "{}(): Argument #{} (${}) {}",
        a.fname(),
        argno,
        name,
        msg
    ))
}

/// A field argument at index `i` (checked).
fn field_arg(vm: &mut Vm, a: &Args, i: usize, name: &str) -> R<usize> {
    let f = a.int(i);
    if !(0..ic::FIELD_COUNT as i64).contains(&f) {
        return field_err(vm, a, i + 1, name, "must be a valid field");
    }
    Ok(f as usize)
}

fn icu_fail(vm: &mut Vm, a: &Args, o: &ObjRef, code: i64) -> R<Value> {
    fail_obj::<Calendar>(vm, a, o, code, "Call to ICU method has failed")?;
    Ok(Value::Bool(false))
}

fn other_cal(vm: &mut Vm, a: &Args, i: usize) -> R<Option<Calendar>> {
    let o = a.obj(i).expect("IntlCalendar");
    match peek_cal(&o) {
        Some(c) => Ok(Some(c)),
        None => vm.throw("Error", "Found unconstructed IntlCalendar"),
    }
}

fn wall(v: i64) -> WallTime {
    match v {
        1 => WallTime::First,
        2 => WallTime::NextValid,
        _ => WallTime::Last,
    }
}

fn wall_int(w: WallTime) -> i64 {
    match w {
        WallTime::First => 1,
        WallTime::NextValid => 2,
        WallTime::Last => 0,
    }
}

const FIELD_NAMES: [&str; 24] = [
    "era",
    "year",
    "month",
    "week of year",
    "week of month",
    "day of year",
    "day of month",
    "day of week",
    "day of week in month",
    "AM/PM",
    "hour",
    "hour of day",
    "minute",
    "second",
    "millisecond",
    "zone offset",
    "DST offset",
    "year for week of year",
    "localized day of week",
    "extended year",
    "julian day",
    "milliseconds in day",
    "is leap month",
    "ordinal month",
];

fn cal_hooks() -> NativeHooks {
    NativeHooks {
        clone: Some(|vm, from, to| match peek_cal(from) {
            Some(c) => super::store(to, c),
            None => clone_error(vm, "Cannot clone uninitialized IntlCalendar"),
        }),
        debug_props: Some(|_, o| {
            let mut out = PhpArray::new();
            for (k, v) in o.obj().props.borrow().iter() {
                out.insert(k.to_owned(), v.clone());
            }
            let Some(mut c) = peek_cal(o) else {
                out.insert(Key::from("valid"), Value::Bool(false));
                return Some(out);
            };
            out.insert(Key::from("valid"), Value::Bool(true));
            out.insert(Key::from("type"), Value::from(c.cal_type.name()));
            let tz = c.time_zone().clone();
            let mut tza = PhpArray::new();
            let (raw, dst) = tz.offsets(goro_ext_intl::timezone::now_ms());
            tza.insert(Key::from("valid"), Value::Bool(true));
            tza.insert(Key::from("id"), Value::from(tz.id()));
            tza.insert(Key::from("rawOffset"), Value::Int(raw as i64));
            tza.insert(Key::from("currentOffset"), Value::Int((raw + dst) as i64));
            out.insert(Key::from("timeZone"), Value::Array(tza));
            out.insert(Key::from("locale"), Value::from(c.valid_locale.as_str()));
            let mut fa = PhpArray::new();
            if c.complete().is_ok() {
                for (i, n) in FIELD_NAMES.iter().enumerate().take(23) {
                    fa.insert(Key::from(*n), Value::Int(c.field(i) as i64));
                }
            }
            out.insert(Key::from("fields"), Value::Array(fa));
            Some(out)
        }),
        ..Default::default()
    }
}

// ---------------------------------------------------------------- creation

fn cal_create_instance(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let tz = match zone_arg(vm, a, 0)? {
        Ok(t) => t,
        Err(m) => {
            fail(vm, a, ue::U_ILLEGAL_ARGUMENT_ERROR, &m)?;
            return Ok(Value::Null);
        }
    };
    let locale = match a.opt_str(1) {
        Some(s) if !s.as_bytes().is_empty() => s.to_string_lossy(),
        _ => super::default_locale(vm),
    };
    let c = make_calendar(vm, None, tz, &locale);
    new_cal_obj(vm, c)
}

fn cal_from_date_time(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let o = match a.val(0) {
        Value::Object(o) => o,
        v => {
            // a string: through the DateTime constructor
            let cls = super::class(vm, "DateTime")?;
            let o = vm.instantiate_raw(&cls)?;
            vm.call_method(&o, b"__construct", vec![v])?;
            o
        }
    };
    let Some(dt) = crate::builtins::date::peek_dt(&o) else {
        fail(
            vm,
            a,
            ue::U_ILLEGAL_ARGUMENT_ERROR,
            "DateTime object is unconstructed",
        )?;
        return Ok(Value::Null);
    };
    let tz = match from_php_zone(&dt.zone) {
        Ok(t) => t,
        Err(m) => {
            fail(vm, a, ue::U_ILLEGAL_ARGUMENT_ERROR, &m)?;
            return Ok(Value::Null);
        }
    };
    let locale = match a.opt_str(1) {
        Some(s) if !s.as_bytes().is_empty() => s.to_string_lossy(),
        _ => super::default_locale(vm),
    };
    let mut c = make_calendar(vm, None, tz, &locale);
    let ms = dt.ts as f64 * 1000.0 + (dt.us / 1000) as f64;
    if c.set_time(ms).is_err() {
        fail(
            vm,
            a,
            ue::U_ILLEGAL_ARGUMENT_ERROR,
            "error creating ICU Calendar object",
        )?;
        return Ok(Value::Null);
    }
    new_cal_obj(vm, c)
}

fn cal_to_date_time(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_cal(vm, a)?;
    let mut c = peek_cal(&o).expect("calendar");
    let Ok(ms) = c.get_time() else {
        return icu_fail(vm, a, &o, ue::U_ILLEGAL_ARGUMENT_ERROR);
    };
    let zo = to_php_zone_obj(vm, c.time_zone())?;
    let zone = crate::builtins::date::get_tz(vm, &zo)?;
    let secs = (ms / 1000.0).floor();
    let us = ((ms - secs * 1000.0) * 1000.0).round() as u32;
    let dt = goro_ext_date::DateTime::from_ts(secs as i64, us, zone);
    Ok(Value::Object(crate::builtins::date::new_date_named(
        vm, "DateTime", dt,
    )?))
}

fn cal_keyword_values(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let kw = a.str(0).to_string_lossy();
    let locale = a.str(1).to_string_lossy();
    if kw != "calendar" {
        fail(
            vm,
            a,
            ue::U_ILLEGAL_ARGUMENT_ERROR,
            "error calling underlying method",
        )?;
        return Ok(Value::Bool(false));
    }
    let items: Vec<String> = if a.bool(2) {
        let l = LocaleId::parse(&locale);
        iloc::preferred_calendars(&l.region_or_likely())
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        ic::systems::ALL
            .iter()
            .map(|c| c.name().to_string())
            .collect()
    };
    Ok(Value::Object(new_string_iterator(vm, items)?))
}

fn cal_get_now(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    reset(vm);
    Ok(Value::Float(goro_ext_intl::timezone::now_ms()))
}

fn cal_available_locales(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    reset(vm);
    let mut arr = PhpArray::new();
    for l in iloc::available_locales() {
        let _ = arr.push(Value::from(l));
    }
    Ok(Value::Array(arr))
}

// ---------------------------------------------------------------- fields

fn cal_get(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let f = field_arg(vm, a, off, "field")?;
    match with_cal(&o, |c| c.get(f)).expect("calendar") {
        Ok(v) => Ok(Value::Int(v as i64)),
        Err(e) => icu_fail(vm, a, &o, e),
    }
}

fn cal_is_set(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let f = field_arg(vm, a, off, "field")?;
    Ok(Value::Bool(with_cal(&o, |c| c.is_set(f)).unwrap_or(false)))
}

fn limit(vm: &mut Vm, a: &mut Args, which: u8) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let f = field_arg(vm, a, off, "field")?;
    let r = with_cal(&o, |c| match which {
        0 => Ok(c.minimum(f)),
        1 => Ok(c.greatest_minimum(f)),
        2 => Ok(c.least_maximum(f)),
        3 => Ok(c.maximum(f)),
        4 => c.actual_minimum(f),
        _ => c.actual_maximum(f),
    })
    .expect("calendar");
    match r {
        Ok(v) => Ok(Value::Int(v as i64)),
        Err(e) => icu_fail(vm, a, &o, e),
    }
}

fn cal_min(vm: &mut Vm, a: &mut Args) -> R<Value> {
    limit(vm, a, 0)
}
fn cal_greatest_min(vm: &mut Vm, a: &mut Args) -> R<Value> {
    limit(vm, a, 1)
}
fn cal_least_max(vm: &mut Vm, a: &mut Args) -> R<Value> {
    limit(vm, a, 2)
}
fn cal_max(vm: &mut Vm, a: &mut Args) -> R<Value> {
    limit(vm, a, 3)
}
fn cal_actual_min(vm: &mut Vm, a: &mut Args) -> R<Value> {
    limit(vm, a, 4)
}
fn cal_actual_max(vm: &mut Vm, a: &mut Args) -> R<Value> {
    limit(vm, a, 5)
}

fn i32_arg(vm: &mut Vm, a: &Args, i: usize, name: &str) -> R<i32> {
    let v = a.int(i);
    if v < i32::MIN as i64 || v > i32::MAX as i64 {
        return field_err(
            vm,
            a,
            i + 1,
            name,
            &format!("must be between {} and {}", i32::MIN, i32::MAX),
        );
    }
    Ok(v as i32)
}

fn cal_set(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if a.this.is_none() {
        vm.deprecated(
            "Function intlcal_set() is deprecated since 8.4, use IntlCalendar::set(), IntlCalendar::setDate(), or IntlCalendar::setDateTime() instead",
        )?;
    }
    let (o, off) = this_cal(vm, a)?;
    let n = a.len() - off;
    if n > 2 {
        vm.deprecated(&format!(
            "Calling {}() with more than 2 arguments is deprecated, use either IntlCalendar::setDate() or IntlCalendar::setDateTime() instead",
            a.fname()
        ))?;
    }
    if n > 6 {
        return vm.throw(
            "ArgumentCountError",
            &format!(
                "{}() expects at most {} arguments, {} given",
                a.fname(),
                6 + off,
                a.len()
            ),
        );
    }
    if n == 4 {
        return vm.throw(
            "ArgumentCountError",
            &format!("{}() has no variant with exactly 4 parameters", a.fname()),
        );
    }
    if n == 2 {
        let f = a.int(off);
        if !(0..ic::FIELD_COUNT as i64).contains(&f) {
            return field_err(vm, a, off + 1, "year", "must be a valid field");
        }
        let v = i32_arg(vm, a, off + 1, "month")?;
        with_cal(&o, |c| c.set(f as usize, v));
        return Ok(Value::Bool(true));
    }
    let names = ["year", "month", "dayOfMonth", "hour", "minute", "second"];
    let mut vals = Vec::new();
    for (k, name) in names.iter().enumerate().take(n) {
        vals.push(i32_arg(vm, a, off + k, name)?);
    }
    set_fields(&o, &vals);
    Ok(Value::Bool(true))
}

fn set_fields(o: &ObjRef, vals: &[i32]) {
    let fields = [
        ic::YEAR,
        ic::MONTH,
        ic::DATE,
        ic::HOUR_OF_DAY,
        ic::MINUTE,
        ic::SECOND,
    ];
    with_cal(o, |c| {
        for (f, v) in fields.iter().zip(vals) {
            c.set(*f, *v);
        }
    });
}

fn cal_set_date(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let y = i32_arg(vm, a, off, "year")?;
    let m = i32_arg(vm, a, off + 1, "month")?;
    let d = i32_arg(vm, a, off + 2, "dayOfMonth")?;
    set_fields(&o, &[y, m, d]);
    Ok(Value::Null)
}

fn cal_set_date_time(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let names = ["year", "month", "dayOfMonth", "hour", "minute"];
    let mut vals = Vec::new();
    for (k, name) in names.iter().enumerate() {
        vals.push(i32_arg(vm, a, off + k, name)?);
    }
    if a.opt_int(off + 5).is_some() {
        vals.push(i32_arg(vm, a, off + 5, "second")?);
    }
    set_fields(&o, &vals);
    Ok(Value::Null)
}

fn cal_clear(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    match a.opt_int(off) {
        None => {
            with_cal(&o, |c| c.clear());
        }
        Some(_) => {
            let f = field_arg(vm, a, off, "field")?;
            with_cal(&o, |c| c.clear_field(f));
        }
    }
    Ok(Value::Bool(true))
}

fn cal_add(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let f = field_arg(vm, a, off, "field")?;
    let v = i32_arg(vm, a, off + 1, "value")?;
    match with_cal(&o, |c| c.add(f, v)).expect("calendar") {
        Ok(()) => Ok(Value::Bool(true)),
        Err(e) => icu_fail(vm, a, &o, e),
    }
}

fn cal_roll(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let f = field_arg(vm, a, off, "field")?;
    let v = match a.val(off + 1) {
        Value::Bool(b) => {
            vm.deprecated(&format!(
                "{}(): Passing bool is deprecated, use 1 or -1 instead",
                a.fname()
            ))?;
            if b { 1 } else { -1 }
        }
        _ => i32_arg(vm, a, off + 1, "value")?,
    };
    match with_cal(&o, |c| c.roll(f, v)).expect("calendar") {
        Ok(()) => Ok(Value::Bool(true)),
        Err(e) => icu_fail(vm, a, &o, e),
    }
}

fn cal_field_difference(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let when = a.float(off);
    let f = field_arg(vm, a, off + 1, "field")?;
    match with_cal(&o, |c| c.field_difference(when, f)).expect("calendar") {
        Ok(v) => Ok(Value::Int(v as i64)),
        Err(e) => icu_fail(vm, a, &o, e),
    }
}

// ---------------------------------------------------------------- time, zone

fn cal_get_time(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_cal(vm, a)?;
    match with_cal(&o, |c| c.get_time()).expect("calendar") {
        Ok(v) => Ok(Value::Float(v)),
        Err(e) => icu_fail(vm, a, &o, e),
    }
}

fn cal_set_time(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let t = a.float(off);
    match with_cal(&o, |c| c.set_time(t)).expect("calendar") {
        Ok(()) => Ok(Value::Bool(true)),
        Err(e) => icu_fail(vm, a, &o, e),
    }
}

fn cal_get_time_zone(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_cal(vm, a)?;
    let tz = peek_cal(&o).expect("calendar").time_zone().clone();
    new_itz(vm, tz)
}

fn cal_set_time_zone(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    if matches!(a.val(off), Value::Null) {
        return Ok(Value::Bool(true));
    }
    match zone_arg(vm, a, off)? {
        Ok(t) => {
            with_cal(&o, |c| c.set_time_zone(t));
            Ok(Value::Bool(true))
        }
        Err(m) => {
            fail_obj::<Calendar>(vm, a, &o, ue::U_ILLEGAL_ARGUMENT_ERROR, &m)?;
            Ok(Value::Bool(false))
        }
    }
}

fn cal_in_daylight(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_cal(vm, a)?;
    match with_cal(&o, |c| c.in_daylight_time()).expect("calendar") {
        Ok(v) => Ok(Value::Bool(v)),
        Err(e) => icu_fail(vm, a, &o, e),
    }
}

// ---------------------------------------------------------------- comparisons

fn compare(vm: &mut Vm, a: &mut Args, which: u8) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let Some(mut other) = other_cal(vm, a, off)? else {
        return Ok(Value::Bool(false));
    };
    let mut me = peek_cal(&o).expect("calendar");
    let (Ok(x), Ok(y)) = (me.get_time(), other.get_time()) else {
        return icu_fail(vm, a, &o, ue::U_ILLEGAL_ARGUMENT_ERROR);
    };
    Ok(Value::Bool(match which {
        0 => x == y,
        1 => x < y,
        _ => x > y,
    }))
}

fn cal_equals(vm: &mut Vm, a: &mut Args) -> R<Value> {
    compare(vm, a, 0)
}
fn cal_before(vm: &mut Vm, a: &mut Args) -> R<Value> {
    compare(vm, a, 1)
}
fn cal_after(vm: &mut Vm, a: &mut Args) -> R<Value> {
    compare(vm, a, 2)
}

fn cal_is_equivalent(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let Some(other) = other_cal(vm, a, off)? else {
        return Ok(Value::Bool(false));
    };
    let me = peek_cal(&o).expect("calendar");
    Ok(Value::Bool(me.is_equivalent_to(&other)))
}

// ---------------------------------------------------------------- settings

fn dow_arg(vm: &mut Vm, a: &Args, i: usize) -> R<i32> {
    let d = a.int(i);
    if !(1..=7).contains(&d) {
        return field_err(vm, a, i + 1, "dayOfWeek", "must be a valid day of the week");
    }
    Ok(d as i32)
}

fn cal_get_first_dow(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_cal(vm, a)?;
    Ok(Value::Int(
        peek_cal(&o).expect("calendar").first_day_of_week() as i64,
    ))
}

fn cal_set_first_dow(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let d = dow_arg(vm, a, off)?;
    with_cal(&o, |c| c.set_first_day_of_week(d));
    Ok(Value::Bool(true))
}

fn cal_get_min_days(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_cal(vm, a)?;
    Ok(Value::Int(
        peek_cal(&o).expect("calendar").minimal_days() as i64
    ))
}

fn cal_set_min_days(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let d = a.int(off);
    if !(1..=7).contains(&d) {
        return field_err(vm, a, off + 1, "days", "must be between 1 and 7");
    }
    with_cal(&o, |c| c.set_minimal_days(d as i32));
    Ok(Value::Bool(true))
}

fn cal_is_lenient(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_cal(vm, a)?;
    Ok(Value::Bool(peek_cal(&o).expect("calendar").is_lenient()))
}

fn cal_set_lenient(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let l = a.bool(off);
    with_cal(&o, |c| c.set_lenient(l));
    Ok(Value::Bool(true))
}

fn cal_get_repeated(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_cal(vm, a)?;
    Ok(Value::Int(wall_int(
        peek_cal(&o).expect("calendar").repeated_wall_time(),
    )))
}

fn cal_get_skipped(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_cal(vm, a)?;
    Ok(Value::Int(wall_int(
        peek_cal(&o).expect("calendar").skipped_wall_time(),
    )))
}

fn cal_set_repeated(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let v = a.int(off);
    if !(0..=1).contains(&v) {
        return field_err(
            vm,
            a,
            off + 1,
            "option",
            "must be either IntlCalendar::WALLTIME_FIRST or IntlCalendar::WALLTIME_LAST",
        );
    }
    with_cal(&o, |c| c.set_repeated_wall_time(wall(v)));
    Ok(Value::Bool(true))
}

fn cal_set_skipped(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let v = a.int(off);
    if !(0..=2).contains(&v) {
        return field_err(
            vm,
            a,
            off + 1,
            "option",
            "must be one of IntlCalendar::WALLTIME_FIRST, IntlCalendar::WALLTIME_LAST, or IntlCalendar::WALLTIME_NEXT_VALID",
        );
    }
    with_cal(&o, |c| c.set_skipped_wall_time(wall(v)));
    Ok(Value::Bool(true))
}

fn cal_get_type(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_cal(vm, a)?;
    Ok(Value::from(peek_cal(&o).expect("calendar").cal_type.name()))
}

fn cal_get_locale(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let c = peek_cal(&o).expect("calendar");
    match a.int(off) {
        0 => Ok(Value::from(c.actual_locale.as_str())),
        1 => Ok(Value::from(c.valid_locale.as_str())),
        _ => {
            fail_obj::<Calendar>(
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

fn cal_dow_type(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let d = dow_arg(vm, a, off)?;
    Ok(Value::Int(
        peek_cal(&o).expect("calendar").day_of_week_type(d) as i64,
    ))
}

fn cal_weekend_transition(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let d = dow_arg(vm, a, off)?;
    match peek_cal(&o).expect("calendar").weekend_transition(d) {
        Ok(v) => Ok(Value::Int(v as i64)),
        Err(e) => icu_fail(vm, a, &o, e),
    }
}

fn cal_is_weekend(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let at = match a.val(off) {
        Value::Null => None,
        _ => Some(a.float(off)),
    };
    match with_cal(&o, |c| c.is_weekend(at)).expect("calendar") {
        Ok(v) => Ok(Value::Bool(v)),
        Err(e) => icu_fail(vm, a, &o, e),
    }
}

fn cal_error_code(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_or_first(a);
    if !has_obj::<Calendar>(&o) {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Int(obj_err::<Calendar>(&o).code))
}

fn cal_error_message(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_or_first(a);
    if !has_obj::<Calendar>(&o) {
        return Ok(Value::Bool(false));
    }
    Ok(Value::from(obj_err::<Calendar>(&o).full_message().as_str()))
}

// ---------------------------------------------------------------- IntlGregorianCalendar

const GREG_NAMES: [&str; 6] = [
    "timezoneOrYear",
    "localeOrMonth",
    "dayOfMonth",
    "hour",
    "minute",
    "second",
];

/// The calendar of the constructor arguments (shared with
/// intlgregcal_create_instance).
fn greg_from_args(vm: &mut Vm, a: &Args, ctor: bool) -> R<Option<Calendar>> {
    let n = a.len();
    if ctor && n > 2 {
        vm.deprecated(&format!(
            "Calling {}() with more than 2 arguments is deprecated, use either IntlGregorianCalendar::createFromDate() or IntlGregorianCalendar::createFromDateTime() instead",
            a.fname()
        ))?;
    }
    if n > 6 {
        return vm.throw("ArgumentCountError", "Too many arguments");
    }
    let mut k = n;
    while k > 0 && matches!(a.val(k - 1), Value::Null) {
        k -= 1;
    }
    if k <= 2 {
        let tz = match zone_arg(vm, a, 0)? {
            Ok(t) => t,
            Err(m) => {
                fail(vm, a, ue::U_ILLEGAL_ARGUMENT_ERROR, &m)?;
                return Ok(None);
            }
        };
        let locale = match a.val(1) {
            Value::Null => super::default_locale(vm),
            v => {
                let s = vm.to_php_str(&v)?.to_string_lossy();
                if s.is_empty() {
                    super::default_locale(vm)
                } else {
                    s
                }
            }
        };
        return Ok(Some(make_calendar(
            vm,
            Some(CalType::Gregorian),
            tz,
            &locale,
        )));
    }
    if k == 4 {
        return vm.throw(
            "ArgumentCountError",
            "No variant with 4 arguments (excluding trailing NULLs)",
        );
    }
    let mut vals = Vec::new();
    for (i, name) in GREG_NAMES.iter().enumerate().take(k) {
        match a.val(i) {
            Value::Int(v) => {
                if v < i32::MIN as i64 || v > i32::MAX as i64 {
                    return field_err(
                        vm,
                        a,
                        i + 1,
                        name,
                        &format!("must be between {} and {}", i32::MIN, i32::MAX),
                    );
                }
                vals.push(v as i32)
            }
            v => {
                let t = Vm::type_name_given(&v);
                return vm.throw(
                    "TypeError",
                    &format!(
                        "{}(): Argument #{} (${}) must be of type int, {} given",
                        a.fname(),
                        i + 1,
                        name,
                        t
                    ),
                );
            }
        }
    }
    let tz = default_zone(vm);
    let locale = super::default_locale(vm);
    let mut c = make_calendar(vm, Some(CalType::Gregorian), tz, &locale);
    c.clear();
    let fields = [
        ic::YEAR,
        ic::MONTH,
        ic::DATE,
        ic::HOUR_OF_DAY,
        ic::MINUTE,
        ic::SECOND,
    ];
    for (f, v) in fields.iter().zip(&vals) {
        c.set(*f, *v);
    }
    Ok(Some(c))
}

fn greg_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if has_obj::<Calendar>(&o) {
        return already_constructed(vm, "IntlGregorianCalendar");
    }
    reset(vm);
    if let Some(c) = greg_from_args(vm, a, true)? {
        super::store(&o, c);
    }
    Ok(Value::Null)
}

fn greg_create_instance(vm: &mut Vm, a: &mut Args) -> R<Value> {
    vm.deprecated(
        "Function intlgregcal_create_instance() is deprecated since 8.4, use IntlGregorianCalendar::__construct(), IntlGregorianCalendar::createFromDate(), or IntlGregorianCalendar::createFromDateTime() instead",
    )?;
    reset(vm);
    match greg_from_args(vm, a, false)? {
        Some(c) => Ok(Value::Object(new_obj(vm, "IntlGregorianCalendar", c)?)),
        None => Ok(Value::Null),
    }
}

fn greg_create(vm: &mut Vm, a: &Args, n: usize) -> R<Value> {
    reset(vm);
    let mut vals = Vec::new();
    for (i, name) in GREG_NAMES.iter().enumerate().take(n) {
        let name = match i {
            0 => "year",
            1 => "month",
            _ => name,
        };
        if i == 5 && a.opt_int(5).is_none() {
            break;
        }
        vals.push(i32_arg(vm, a, i, name)?);
    }
    let tz = default_zone(vm);
    let locale = super::default_locale(vm);
    let mut c = make_calendar(vm, Some(CalType::Gregorian), tz, &locale);
    c.clear();
    let fields = [
        ic::YEAR,
        ic::MONTH,
        ic::DATE,
        ic::HOUR_OF_DAY,
        ic::MINUTE,
        ic::SECOND,
    ];
    for (f, v) in fields.iter().zip(&vals) {
        c.set(*f, *v);
    }
    let cls = super::static_class(vm, a);
    let o = vm.instantiate_raw(&cls)?;
    super::store(&o, c);
    Ok(Value::Object(o))
}

fn greg_create_from_date(vm: &mut Vm, a: &mut Args) -> R<Value> {
    greg_create(vm, a, 3)
}

fn greg_create_from_date_time(vm: &mut Vm, a: &mut Args) -> R<Value> {
    greg_create(vm, a, 6)
}

fn greg_set_change(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let t = a.float(off);
    match with_cal(&o, |c| c.set_gregorian_change(t)).expect("calendar") {
        Ok(()) => Ok(Value::Bool(true)),
        Err(e) => icu_fail(vm, a, &o, e),
    }
}

fn greg_get_change(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_cal(vm, a)?;
    Ok(Value::Float(
        peek_cal(&o).expect("calendar").gregorian_change(),
    ))
}

fn greg_is_leap(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_cal(vm, a)?;
    let y = a.int(off);
    Ok(Value::Bool(peek_cal(&o).expect("calendar").is_leap_year(y)))
}
