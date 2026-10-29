//! The date extension (PHP manual: Date/Time): the date functions, the
//! DateTime, DateTimeImmutable, DateTimeZone, DateInterval and DatePeriod
//! classes, the DATE_* constants, the date.* ini settings and the date
//! exceptions. The calendar, parsing, formatting and zone work is done by
//! the goro-ext-date crate; this module wires it to PHP values.
//!
//! Object layouts and messages follow the ext/date tests' expected output
//! and observations on PHP 8.5.

mod classes;
mod funcs;
mod period;

use std::rc::Rc;

use goro_ext_date::from_format::LastErrors;
use goro_ext_date::interval::{FromString, Interval};
use goro_ext_date::{DateTime, TimeZone};
use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};

use crate::class::{Class, NativeHooks, cflags};
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

use classes::*;
use funcs::*;
use period::*;

pub static MODULE: Module = Module {
    name: "date",
    functions: FUNCTIONS,
    classes: CLASSES,
    methods: METHODS,
    hooks: &[
        ("datetime", date_hooks),
        ("datetimeimmutable", date_hooks),
        ("datetimezone", zone_hooks),
        ("dateinterval", interval_hooks),
        ("dateperiod", period_hooks),
    ],
    constants: &[
        ("DATE_ATOM", ConstVal::Str("Y-m-d\\TH:i:sP")),
        ("DATE_COOKIE", ConstVal::Str("l, d-M-Y H:i:s T")),
        ("DATE_ISO8601", ConstVal::Str("Y-m-d\\TH:i:sO")),
        ("DATE_ISO8601_EXPANDED", ConstVal::Str("X-m-d\\TH:i:sP")),
        ("DATE_RFC822", ConstVal::Str("D, d M y H:i:s O")),
        ("DATE_RFC850", ConstVal::Str("l, d-M-y H:i:s T")),
        ("DATE_RFC1036", ConstVal::Str("D, d M y H:i:s O")),
        ("DATE_RFC1123", ConstVal::Str("D, d M Y H:i:s O")),
        ("DATE_RFC7231", ConstVal::Str("D, d M Y H:i:s \\G\\M\\T")),
        ("DATE_RFC2822", ConstVal::Str("D, d M Y H:i:s O")),
        ("DATE_RFC3339", ConstVal::Str("Y-m-d\\TH:i:sP")),
        ("DATE_RFC3339_EXTENDED", ConstVal::Str("Y-m-d\\TH:i:s.vP")),
        ("DATE_RSS", ConstVal::Str("D, d M Y H:i:s O")),
        ("DATE_W3C", ConstVal::Str("Y-m-d\\TH:i:sP")),
        ("SUNFUNCS_RET_TIMESTAMP", ConstVal::Int(0)),
        ("SUNFUNCS_RET_STRING", ConstVal::Int(1)),
        ("SUNFUNCS_RET_DOUBLE", ConstVal::Int(2)),
    ],
    // PHP manual: Date/Time runtime configuration; defaults observed on
    // PHP 8.5 (`php -n`).
    ini: &[
        ("date.timezone", "UTC"),
        ("date.default_latitude", "31.7667"),
        ("date.default_longitude", "35.2333"),
        ("date.sunset_zenith", "90.833333"),
        ("date.sunrise_zenith", "90.833333"),
    ],
    ..Module::EMPTY
};

const CLASSES: &[(&str, &str)] = &[
    (
        "datetimeinterface",
        r#"<?php interface DateTimeInterface {
    const string ATOM = 'Y-m-d\TH:i:sP';
    const string COOKIE = 'l, d-M-Y H:i:s T';
    const string ISO8601 = 'Y-m-d\TH:i:sO';
    const string ISO8601_EXPANDED = 'X-m-d\TH:i:sP';
    const string RFC822 = 'D, d M y H:i:s O';
    const string RFC850 = 'l, d-M-y H:i:s T';
    const string RFC1036 = 'D, d M y H:i:s O';
    const string RFC1123 = 'D, d M Y H:i:s O';
    #[\Deprecated(message: "as this format ignores the associated timezone and always uses GMT", since: "8.5")]
    const string RFC7231 = 'D, d M Y H:i:s \G\M\T';
    const string RFC2822 = 'D, d M Y H:i:s O';
    const string RFC3339 = 'Y-m-d\TH:i:sP';
    const string RFC3339_EXTENDED = 'Y-m-d\TH:i:s.vP';
    const string RSS = 'D, d M Y H:i:s O';
    const string W3C = 'Y-m-d\TH:i:sP';
    public function format(string $format): string;
    public function getTimezone(): DateTimeZone|false;
    public function getOffset(): int;
    public function getTimestamp(): int;
    public function getMicrosecond(): int;
    public function diff(DateTimeInterface $targetObject, bool $absolute = false): DateInterval;
    #[\Deprecated(message: "this method is obsolete, as serialization hooks are provided by __unserialize() and __serialize()", since: "8.5")]
    public function __wakeup(): void;
    public function __serialize(): array;
    public function __unserialize(array $data): void;
}"#,
    ),
    (
        "datetime",
        r#"<?php class DateTime implements DateTimeInterface {
    public function __construct(string $datetime = "now", ?DateTimeZone $timezone = null) {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
    #[\Deprecated(message: "this method is obsolete, as serialization hooks are provided by __unserialize() and __serialize()", since: "8.5")]
    public function __wakeup(): void {}
    public static function __set_state(array $array): DateTime {}
    public static function createFromImmutable(DateTimeImmutable $object): static {}
    public static function createFromInterface(DateTimeInterface $object): DateTime {}
    public static function createFromFormat(string $format, string $datetime, ?DateTimeZone $timezone = null): DateTime|false {}
    public static function createFromTimestamp(int|float $timestamp): static {}
    public static function getLastErrors(): array|false {}
    public function format(string $format): string {}
    public function modify(string $modifier): DateTime {}
    public function add(DateInterval $interval): DateTime {}
    public function sub(DateInterval $interval): DateTime {}
    public function getTimezone(): DateTimeZone|false {}
    public function setTimezone(DateTimeZone $timezone): DateTime {}
    public function getOffset(): int {}
    public function getMicrosecond(): int {}
    public function setTime(int $hour, int $minute, int $second = 0, int $microsecond = 0): DateTime {}
    public function setDate(int $year, int $month, int $day): DateTime {}
    public function setISODate(int $year, int $week, int $dayOfWeek = 1): DateTime {}
    public function setTimestamp(int $timestamp): DateTime {}
    public function setMicrosecond(int $microsecond): static {}
    public function getTimestamp(): int {}
    public function diff(DateTimeInterface $targetObject, bool $absolute = false): DateInterval {}
}"#,
    ),
    (
        "datetimeimmutable",
        r#"<?php class DateTimeImmutable implements DateTimeInterface {
    public function __construct(string $datetime = "now", ?DateTimeZone $timezone = null) {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
    #[\Deprecated(message: "this method is obsolete, as serialization hooks are provided by __unserialize() and __serialize()", since: "8.5")]
    public function __wakeup(): void {}
    public static function __set_state(array $array): DateTimeImmutable {}
    public static function createFromFormat(string $format, string $datetime, ?DateTimeZone $timezone = null): DateTimeImmutable|false {}
    public static function createFromTimestamp(int|float $timestamp): static {}
    public static function getLastErrors(): array|false {}
    public function format(string $format): string {}
    public function getTimezone(): DateTimeZone|false {}
    public function getOffset(): int {}
    public function getTimestamp(): int {}
    public function getMicrosecond(): int {}
    public function diff(DateTimeInterface $targetObject, bool $absolute = false): DateInterval {}
    public function modify(string $modifier): DateTimeImmutable {}
    public function add(DateInterval $interval): DateTimeImmutable {}
    public function sub(DateInterval $interval): DateTimeImmutable {}
    public function setTimezone(DateTimeZone $timezone): DateTimeImmutable {}
    public function setTime(int $hour, int $minute, int $second = 0, int $microsecond = 0): DateTimeImmutable {}
    public function setDate(int $year, int $month, int $day): DateTimeImmutable {}
    public function setISODate(int $year, int $week, int $dayOfWeek = 1): DateTimeImmutable {}
    public function setTimestamp(int $timestamp): DateTimeImmutable {}
    public function setMicrosecond(int $microsecond): static {}
    public static function createFromMutable(DateTime $object): static {}
    public static function createFromInterface(DateTimeInterface $object): DateTimeImmutable {}
}"#,
    ),
    (
        "datetimezone",
        r#"<?php class DateTimeZone {
    const int AFRICA = 1;
    const int AMERICA = 2;
    const int ANTARCTICA = 4;
    const int ARCTIC = 8;
    const int ASIA = 16;
    const int ATLANTIC = 32;
    const int AUSTRALIA = 64;
    const int EUROPE = 128;
    const int INDIAN = 256;
    const int PACIFIC = 512;
    const int UTC = 1024;
    const int ALL = 2047;
    const int ALL_WITH_BC = 4095;
    const int PER_COUNTRY = 4096;
    public function __construct(string $timezone) {}
    public function getName(): string {}
    public function getOffset(DateTimeInterface $datetime): int {}
    public function getTransitions(int $timestampBegin = PHP_INT_MIN, int $timestampEnd = 2147483647): array|false {}
    public function getLocation(): array|false {}
    public static function listAbbreviations(): array {}
    public static function listIdentifiers(int $timezoneGroup = DateTimeZone::ALL, ?string $countryCode = null): array {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
    #[\Deprecated(message: "this method is obsolete, as serialization hooks are provided by __unserialize() and __serialize()", since: "8.5")]
    public function __wakeup(): void {}
    public static function __set_state(array $array): DateTimeZone {}
}"#,
    ),
    (
        "dateinterval",
        r#"<?php class DateInterval {
    public function __construct(string $duration) {}
    public static function createFromDateString(string $datetime): DateInterval {}
    public function format(string $format): string {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
    #[\Deprecated(message: "this method is obsolete, as serialization hooks are provided by __unserialize() and __serialize()", since: "8.5")]
    public function __wakeup(): void {}
    public static function __set_state(array $array): DateInterval {}
}"#,
    ),
    (
        "dateperiod",
        r#"<?php class DatePeriod implements IteratorAggregate {
    const int EXCLUDE_START_DATE = 1;
    const int INCLUDE_END_DATE = 2;
    public static function createFromISO8601String(string $specification, int $options = 0): static {}
    public function __construct($start = null, $interval = null, $end = null, $options = null) {}
    public function getStartDate(): DateTimeInterface {}
    public function getEndDate(): ?DateTimeInterface {}
    public function getDateInterval(): DateInterval {}
    public function getRecurrences(): ?int {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
    #[\Deprecated(message: "this method is obsolete, as serialization hooks are provided by __unserialize() and __serialize()", since: "8.5")]
    public function __wakeup(): void {}
    public static function __set_state(array $array): DatePeriod {}
    public function getIterator(): Iterator {}
}"#,
    ),
    (
        "dateperioditerator",
        "<?php final class DatePeriodIterator implements Iterator {
    private function __construct() {}
    public function current(): mixed {}
    public function key(): mixed {}
    public function next(): void {}
    public function valid(): bool {}
    public function rewind(): void {}
}",
    ),
    ("dateerror", "<?php class DateError extends Error {}"),
    (
        "dateobjecterror",
        "<?php class DateObjectError extends DateError {}",
    ),
    (
        "daterangeerror",
        "<?php class DateRangeError extends DateError {}",
    ),
    (
        "dateexception",
        "<?php class DateException extends Exception {}",
    ),
    (
        "dateinvalidtimezoneexception",
        "<?php class DateInvalidTimeZoneException extends DateException {}",
    ),
    (
        "dateinvalidoperationexception",
        "<?php class DateInvalidOperationException extends DateException {}",
    ),
    (
        "datemalformedstringexception",
        "<?php class DateMalformedStringException extends DateException {}",
    ),
    (
        "datemalformedintervalstringexception",
        "<?php class DateMalformedIntervalStringException extends DateException {}",
    ),
    (
        "datemalformedperiodstringexception",
        "<?php class DateMalformedPeriodStringException extends DateException {}",
    ),
];

// ---------------------------------------------------------------- state

/// Per-request state.
#[derive(Default)]
pub(crate) struct DateState {
    /// The zone set by date_default_timezone_set().
    default_tz: Option<TimeZone>,
    /// The warnings and errors of the last parse (None: getLastErrors()
    /// returns false).
    last_errors: Option<LastErrors>,
}

/// The current time (seconds, microseconds).
pub(crate) fn now() -> (i64, u32) {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    (d.as_secs() as i64, d.subsec_micros())
}

/// The default zone: date_default_timezone_set()'s, else the
/// date.timezone setting, else UTC (PHP manual:
/// date_default_timezone_get).
pub(crate) fn default_tz(vm: &mut Vm) -> TimeZone {
    if let Some(z) = &vm.ext::<DateState>().default_tz {
        return z.clone();
    }
    let ini = vm.get_ini(b"date.timezone").unwrap_or_default();
    goro_ext_date::api::default_timezone(&ini.to_string_lossy()).unwrap_or_else(TimeZone::utc)
}

/// The time stamp of error log lines, "d-M-Y H:i:s e" in the default zone
/// (the zone named as the ini setting spells it; test
/// general_functions/bug60723.phpt).
pub fn log_timestamp(vm: &mut Vm) -> String {
    let tz = default_tz(vm);
    let set = vm.ext::<DateState>().default_tz.is_some();
    let ini = vm.get_ini(b"date.timezone").unwrap_or_default();
    let name = if !set
        && !ini.is_empty()
        && goro_ext_date::api::default_timezone(&ini.to_string_lossy()).is_some()
    {
        ini.to_string_lossy()
    } else {
        tz.name()
    };
    let t = now().0;
    let d = goro_ext_date::api::strftime(b"%d-%b-%Y %H:%M:%S", t, &tz, false);
    format!("{} {}", String::from_utf8_lossy(&d), name)
}

/// Stores the warnings and errors of a parse for getLastErrors().
pub(crate) fn set_last_errors(vm: &mut Vm, e: LastErrors) {
    vm.ext::<DateState>().last_errors = if e.is_empty() { None } else { Some(e) };
}

/// `ini_set("date.timezone", v)`: whether the value is accepted. A bad
/// zone keeps the old value with "Invalid date.timezone value 'v', using
/// 'old' instead" (test ini_set_incorrect-002.phpt).
pub(crate) fn ini_timezone_check(vm: &mut Vm, a: &Args, v: &[u8], old: &[u8]) -> R<bool> {
    let name = String::from_utf8_lossy(v);
    if goro_ext_date::api::default_timezone(&name).is_some() {
        return Ok(true);
    }
    vm.fwarn(
        a,
        &format!(
            "Invalid date.timezone value '{}', using '{}' instead",
            name,
            String::from_utf8_lossy(old)
        ),
    )?;
    Ok(false)
}

/// The startup warning for a bad `-d date.timezone` (test
/// date_default_timezone_get-2.phpt).
pub(crate) fn startup_timezone_check(vm: &mut Vm, ini: &[(Vec<u8>, Vec<u8>)]) {
    let Some((_, v)) = ini
        .iter()
        .rev()
        .find(|(n, _)| n.as_slice() == b"date.timezone")
    else {
        return;
    };
    let name = String::from_utf8_lossy(v).into_owned();
    // an ini line's comment (";...") is not part of the value
    let name = name.split(';').next().unwrap_or("").trim().to_string();
    if goro_ext_date::api::default_timezone(&name).is_none() {
        let _ = vm.raise_at(
            crate::vm::errors::E_WARNING,
            &format!(
                "PHP Startup: {}",
                goro_ext_date::api::invalid_ini_zone_warning(&name)
            ),
            PhpStr::from("Unknown"),
            0,
        );
    }
}

// ---------------------------------------------------------------- native data

/// DateTime / DateTimeImmutable state (None until constructed).
pub(crate) struct DateObj(pub Option<DateTime>);
impl goro_runtime::NativeData for DateObj {}

/// DateTimeZone state.
pub(crate) struct TzObj(pub Option<TimeZone>);
impl goro_runtime::NativeData for TzObj {}

/// DateInterval state: the numbers live in the object's properties (PHP
/// shows and updates them as properties); this keeps whether the object
/// was constructed and the relative part of createFromDateString().
pub(crate) struct IvObj {
    pub init: bool,
    pub from_string: Option<FromString>,
}
impl goro_runtime::NativeData for IvObj {}

pub(crate) fn with_native<T: 'static, U>(o: &ObjRef, f: impl FnOnce(&mut T) -> U) -> Option<U> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<T>().map(f)
}

/// The class `name` (an internal class of this module).
pub(crate) fn class(vm: &mut Vm, name: &str) -> R<Rc<Class>> {
    match vm.find_class(name.as_bytes(), false)? {
        Some(c) => Ok(c),
        None => Err(vm.fatal(&format!("Class \"{name}\" not found"))),
    }
}

/// The class a static method was called on (`static`).
pub(crate) fn static_class(vm: &Vm, a: &Args) -> Rc<Class> {
    let p = if a.static_class.is_null() {
        a.func.scope.get()
    } else {
        a.static_class
    };
    vm.rc_class(p)
}

/// "Object of type X (inheriting Y) has not been correctly initialized
/// by calling parent::__construct() in its constructor" (DateObjectError).
pub(crate) fn uninit<T>(vm: &mut Vm, o: &ObjRef) -> R<T> {
    let cls = vm.class_of(o);
    let mut base = Some(cls.clone());
    while let Some(c) = &base {
        if c.has_flag(cflags::INTERNAL) {
            break;
        }
        base = c.parent.clone();
    }
    let name = cls.name.to_string_lossy();
    let msg = match base {
        Some(b) if !Rc::ptr_eq(&b, &cls) => format!(
            "Object of type {} (inheriting {}) has not been correctly initialized by calling parent::__construct() in its constructor",
            name,
            b.name.to_string_lossy()
        ),
        _ => format!(
            "Object of type {name} has not been correctly initialized by calling parent::__construct() in its constructor"
        ),
    };
    vm.throw("DateObjectError", &msg)
}

pub(crate) fn peek_dt(o: &ObjRef) -> Option<DateTime> {
    with_native::<DateObj, _>(o, |d| d.0.clone()).flatten()
}

pub(crate) fn is_date(o: &ObjRef) -> bool {
    with_native::<DateObj, _>(o, |_| ()).is_some()
}

pub(crate) fn get_dt(vm: &mut Vm, o: &ObjRef) -> R<DateTime> {
    match peek_dt(o) {
        Some(d) => Ok(d),
        None => uninit(vm, o),
    }
}

pub(crate) fn set_dt(o: &ObjRef, dt: DateTime) {
    let done = with_native::<DateObj, _>(o, |d| d.0 = Some(dt.clone()));
    if done.is_none() {
        *o.obj().native.borrow_mut() = Some(Box::new(DateObj(Some(dt))));
    }
}

pub(crate) fn peek_tz(o: &ObjRef) -> Option<TimeZone> {
    with_native::<TzObj, _>(o, |d| d.0.clone()).flatten()
}

pub(crate) fn get_tz(vm: &mut Vm, o: &ObjRef) -> R<TimeZone> {
    match peek_tz(o) {
        Some(d) => Ok(d),
        None => uninit(vm, o),
    }
}

pub(crate) fn set_tz(o: &ObjRef, tz: TimeZone) {
    let done = with_native::<TzObj, _>(o, |d| d.0 = Some(tz.clone()));
    if done.is_none() {
        *o.obj().native.borrow_mut() = Some(Box::new(TzObj(Some(tz))));
    }
}

/// A new object of `cls` (DateTime, DateTimeImmutable or a subclass)
/// holding `dt`.
pub(crate) fn new_date(vm: &mut Vm, cls: &Rc<Class>, dt: DateTime) -> R<ObjRef> {
    let o = vm.instantiate(cls)?;
    set_dt(&o, dt);
    Ok(o)
}

pub(crate) fn new_date_named(vm: &mut Vm, name: &str, dt: DateTime) -> R<ObjRef> {
    let cls = class(vm, name)?;
    new_date(vm, &cls, dt)
}

pub(crate) fn new_tz(vm: &mut Vm, tz: TimeZone) -> R<ObjRef> {
    let cls = class(vm, "DateTimeZone")?;
    let o = vm.instantiate(&cls)?;
    set_tz(&o, tz);
    Ok(o)
}

/// The zone of an optional `?DateTimeZone` argument, else the default.
pub(crate) fn zone_arg(vm: &mut Vm, a: &Args, i: usize) -> R<TimeZone> {
    match a.obj(i) {
        Some(o) => match peek_tz(&o) {
            Some(z) => Ok(z),
            // test bug-gh15582.phpt
            None => vm.throw(
                "Error",
                "The DateTimeZone object has not been correctly initialized by its constructor",
            ),
        },
        None => Ok(default_tz(vm)),
    }
}

/// `$this` for a method, else the first argument (the procedural forms),
/// and the index of the next argument.
pub(crate) fn this_or_first(a: &Args) -> (ObjRef, usize) {
    match &a.this {
        Some(o) => (o.clone(), 0),
        None => (a.obj(0).expect("object argument"), 1),
    }
}

pub(crate) fn s(b: &[u8]) -> Value {
    Value::Str(PhpStr::new(b))
}

pub(crate) fn sv(t: &str) -> Value {
    Value::Str(PhpStr::from(t))
}

pub(crate) fn key(k: &str) -> Key {
    Key::from(k)
}

// ---------------------------------------------------------------- property views

/// "date", "timezone_type", "timezone" (observed on PHP 8.5).
pub(crate) fn date_parts(dt: &DateTime, out: &mut PhpArray) {
    out.insert(key("date"), sv(&dt.dump_date()));
    zone_parts(&dt.zone, out);
}

pub(crate) fn zone_parts(z: &TimeZone, out: &mut PhpArray) {
    out.insert(key("timezone_type"), Value::Int(z.kind() as i64));
    out.insert(key("timezone"), sv(&z.name()));
}

/// The var_dump / (array) view: the object's own properties, then the
/// date's (observed on PHP 8.5).
fn date_props(vm: &mut Vm, o: &ObjRef) -> Option<PhpArray> {
    let mut out = vm.plain_props_array(o, true);
    if let Some(dt) = peek_dt(o) {
        date_parts(&dt, &mut out);
    }
    Some(out)
}

fn zone_props(vm: &mut Vm, o: &ObjRef) -> Option<PhpArray> {
    let mut out = vm.plain_props_array(o, true);
    if let Some(z) = peek_tz(o) {
        zone_parts(&z, &mut out);
    }
    Some(out)
}

/// Reads of a missing property warn as undefined instead of finding the
/// var_dump view's members (test bug75232.phpt).
fn no_view_read(
    vm: &mut Vm,
    o: &ObjRef,
    name: &PhpStr,
    op: crate::class::DynPropOp,
) -> Option<R<Value>> {
    if op != crate::class::DynPropOp::Read {
        return None;
    }
    let cls = vm.class_of(o).name.to_string_lossy();
    Some(
        vm.warn(&format!(
            "Undefined property: {}::${}",
            cls,
            name.to_string_lossy()
        ))
        .map(|_| Value::Null),
    )
}

fn date_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|_, o| {
            *o.obj().native.borrow_mut() = Some(Box::new(DateObj(None)));
        }),
        clone: Some(|_, from, to| {
            let d = peek_dt(from);
            *to.obj().native.borrow_mut() = Some(Box::new(DateObj(d)));
        }),
        compare: Some(date_compare),
        dyn_prop: Some(no_view_read),
        debug_props: Some(date_props),
        props_for: Some(date_props),
        ..Default::default()
    }
}

/// DateTime comparison: by instant (PHP manual: DateTime, "Comparison").
fn date_compare(vm: &mut Vm, x: &Value, y: &Value) -> Option<R<i32>> {
    let (Value::Object(a), Value::Object(b)) = (x, y) else {
        return None;
    };
    if !is_date(a) || !is_date(b) {
        return None;
    }
    let (Some(da), Some(db)) = (peek_dt(a), peek_dt(b)) else {
        return Some(vm.throw(
            "DateObjectError",
            "Trying to compare an incomplete DateTime or DateTimeImmutable object",
        ));
    };
    Some(Ok(match (da.ts, da.us).cmp(&(db.ts, db.us)) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }))
}

fn zone_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|_, o| {
            *o.obj().native.borrow_mut() = Some(Box::new(TzObj(None)));
        }),
        clone: Some(|_, from, to| {
            let d = peek_tz(from);
            *to.obj().native.borrow_mut() = Some(Box::new(TzObj(d)));
        }),
        compare: Some(zone_compare),
        dyn_prop: Some(no_view_read),
        debug_props: Some(zone_props),
        props_for: Some(zone_props),
        ..Default::default()
    }
}

/// Zones compare equal when they are the same kind with the same name;
/// different kinds throw (tests DateTimeZone_compare_basic1.phpt).
fn zone_compare(vm: &mut Vm, x: &Value, y: &Value) -> Option<R<i32>> {
    let (Value::Object(a), Value::Object(b)) = (x, y) else {
        return None;
    };
    let ka = with_native::<TzObj, _>(a, |d| d.0.clone())?;
    let kb = with_native::<TzObj, _>(b, |d| d.0.clone())?;
    let (Some(za), Some(zb)) = (ka, kb) else {
        return Some(vm.throw(
            "DateObjectError",
            "Trying to compare uninitialized DateTimeZone objects",
        ));
    };
    if za.kind() != zb.kind() {
        return Some(vm.throw(
            "DateException",
            "Cannot compare two different kinds of DateTimeZone objects",
        ));
    }
    let same = match (&za, &zb) {
        (TimeZone::Offset(p), TimeZone::Offset(q)) => p == q,
        _ => za.name() == zb.name(),
    };
    Some(Ok(if same { 0 } else { 1 }))
}

fn interval_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|_, o| {
            *o.obj().native.borrow_mut() = Some(Box::new(IvObj {
                init: false,
                from_string: None,
            }));
        }),
        clone: Some(|_, from, to| {
            let d = with_native::<IvObj, _>(from, |d| (d.init, d.from_string.clone()));
            let (init, from_string) = d.unwrap_or((false, None));
            *to.obj().native.borrow_mut() = Some(Box::new(IvObj { init, from_string }));
        }),
        compare: Some(|vm, x, y| {
            let (Value::Object(a), Value::Object(b)) = (x, y) else {
                return None;
            };
            with_native::<IvObj, _>(a, |_| ())?;
            with_native::<IvObj, _>(b, |_| ())?;
            Some(vm.warn("Cannot compare DateInterval objects").map(|_| 1))
        }),
        // an interval from createFromDateString() reads its numbers as
        // zero and `days` as false (test bug70153.phpt)
        dyn_prop: Some(|_, o, name, op| {
            if op != crate::class::DynPropOp::Read
                || with_native::<IvObj, _>(o, |d| d.from_string.is_some()) != Some(true)
            {
                return None;
            }
            let r = with_native::<IvObj, _>(o, |d| d.from_string.as_ref().map(|f| f.relative))
                .flatten()?;
            Some(Ok(match name.as_bytes() {
                b"y" => Value::Int(r.y),
                b"m" => Value::Int(r.m),
                b"d" => Value::Int(r.d),
                b"h" => Value::Int(r.h),
                b"i" => Value::Int(r.i),
                b"s" => Value::Int(r.s),
                b"f" => Value::Float(r.us as f64 / 1_000_000.0),
                b"invert" => Value::Int(0),
                b"days" => Value::Bool(false),
                _ => return None,
            }))
        }),
        ..Default::default()
    }
}

// ---------------------------------------------------------------- intervals

/// The interval held by a DateInterval object (its properties).
pub(crate) fn get_iv(vm: &mut Vm, o: &ObjRef) -> R<Interval> {
    let st = with_native::<IvObj, _>(o, |d| (d.init, d.from_string.clone()));
    let Some((true, from_string)) = st else {
        return uninit(vm, o);
    };
    if let Some(fs) = from_string {
        let r = fs.relative;
        return Ok(Interval {
            y: r.y,
            m: r.m,
            d: r.d,
            h: r.h,
            i: r.i,
            s: r.s,
            us: r.us,
            from_string: Some(fs),
            ..Default::default()
        });
    }
    let int = |vm: &mut Vm, n: &str| -> R<i64> {
        let v = vm.get_prop_raw(o, n.as_bytes());
        vm.to_int(&v)
    };
    let y = int(vm, "y")?;
    let m = int(vm, "m")?;
    let d = int(vm, "d")?;
    let h = int(vm, "h")?;
    let i = int(vm, "i")?;
    let s = int(vm, "s")?;
    let invert = int(vm, "invert")? != 0;
    let f = vm.get_prop_raw(o, b"f");
    let f = vm.to_float(&f)?;
    let days = match vm.get_prop_raw(o, b"days") {
        Value::Bool(false) | Value::Null => None,
        v => Some(vm.to_int(&v)?),
    };
    Ok(Interval {
        y,
        m,
        d,
        h,
        i,
        s,
        us: (f * 1_000_000.0).round() as i64,
        invert,
        days,
        from_string: None,
    })
}

/// Writes `iv` into a DateInterval object's properties.
pub(crate) fn set_iv(vm: &mut Vm, o: &ObjRef, iv: &Interval) {
    with_native::<IvObj, _>(o, |d| {
        d.init = true;
        d.from_string = iv.from_string.clone();
    });
    {
        let mut props = o.obj().props.borrow_mut();
        for k in [
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
        ] {
            let old = props.remove(Key::from(k).as_ref());
            drop(old);
        }
    }
    if let Some(fs) = &iv.from_string {
        vm.set_prop_raw(o, b"from_string", Value::Bool(true));
        vm.set_prop_raw(o, b"date_string", s(&fs.date_string));
        return;
    }
    vm.set_prop_raw(o, b"y", Value::Int(iv.y));
    vm.set_prop_raw(o, b"m", Value::Int(iv.m));
    vm.set_prop_raw(o, b"d", Value::Int(iv.d));
    vm.set_prop_raw(o, b"h", Value::Int(iv.h));
    vm.set_prop_raw(o, b"i", Value::Int(iv.i));
    vm.set_prop_raw(o, b"s", Value::Int(iv.s));
    vm.set_prop_raw(o, b"f", Value::Float(iv.us as f64 / 1_000_000.0));
    vm.set_prop_raw(o, b"invert", Value::Int(iv.invert as i64));
    vm.set_prop_raw(
        o,
        b"days",
        match iv.days {
            Some(d) => Value::Int(d),
            None => Value::Bool(false),
        },
    );
    vm.set_prop_raw(o, b"from_string", Value::Bool(false));
}

pub(crate) fn new_iv(vm: &mut Vm, iv: &Interval) -> R<ObjRef> {
    let cls = class(vm, "DateInterval")?;
    let o = vm.instantiate(&cls)?;
    set_iv(vm, &o, iv);
    Ok(o)
}

/// The getLastErrors() array.
pub(crate) fn last_errors_value(e: &LastErrors) -> Value {
    let list = |l: &[(usize, &str)]| {
        let mut a = PhpArray::new();
        for (p, m) in l {
            a.insert(Key::Int(*p as i64), sv(m));
        }
        Value::Array(a)
    };
    let mut out = PhpArray::new();
    out.insert(key("warning_count"), Value::Int(e.warnings.len() as i64));
    out.insert(key("warnings"), list(&e.warnings_array()));
    out.insert(key("error_count"), Value::Int(e.errors.len() as i64));
    out.insert(key("errors"), list(&e.errors_array()));
    Value::Array(out)
}

pub(crate) fn last_errors(vm: &mut Vm) -> Value {
    match &vm.ext::<DateState>().last_errors {
        Some(e) => last_errors_value(e),
        None => Value::Bool(false),
    }
}

pub(crate) fn set_default_tz(vm: &mut Vm, z: TimeZone) {
    vm.ext::<DateState>().default_tz = Some(z);
}
