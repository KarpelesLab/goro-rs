//! `IntlTimeZone` and the `intltz_*()` functions (PHP manual:
//! IntlTimeZone), plus the `IntlIterator` the enumerations return. The
//! zone logic is goro_ext_intl::timezone's.

use goro_ext_intl::error as ue;
use goro_ext_intl::timezone::names::{self, Style};
use goro_ext_intl::timezone::{self as itz, TimeZone};
use goro_runtime::{Key, ObjRef, PhpArray, Value};

use super::{fail, fail_obj, has_obj, new_obj, obj_err, reset, reset_obj, this_or_first, with_obj};
use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Unwind, Vm};

pub static MODULE: Module = Module {
    name: "intl",
    functions: &[
        (
            "function intltz_count_equivalent_ids(string $timezoneId): int|false",
            tz_count_equivalent,
        ),
        (
            "function intltz_create_default(): IntlTimeZone",
            tz_create_default,
        ),
        (
            "function intltz_create_enumeration(string|int|null $countryOrRawOffset = null): IntlIterator|false",
            tz_create_enumeration,
        ),
        (
            "function intltz_create_time_zone(string $timezoneId): ?IntlTimeZone",
            tz_create,
        ),
        (
            "function intltz_create_time_zone_id_enumeration(int $type, ?string $region = null, ?int $rawOffset = null): IntlIterator|false",
            tz_create_id_enumeration,
        ),
        (
            "function intltz_from_date_time_zone(DateTimeZone $timezone): ?IntlTimeZone",
            tz_from_dtz,
        ),
        (
            "function intltz_get_canonical_id(string $timezoneId, &$isSystemId = null): string|false",
            tz_get_canonical_id,
        ),
        (
            "function intltz_get_display_name(IntlTimeZone $timezone, bool $dst = false, int $style = IntlTimeZone::DISPLAY_LONG, ?string $locale = null): string|false",
            tz_get_display_name,
        ),
        (
            "function intltz_get_dst_savings(IntlTimeZone $timezone): int",
            tz_get_dst_savings,
        ),
        (
            "function intltz_get_equivalent_id(string $timezoneId, int $offset): string|false",
            tz_get_equivalent_id,
        ),
        (
            "function intltz_get_error_code(IntlTimeZone $timezone): int|false",
            tz_get_error_code,
        ),
        (
            "function intltz_get_error_message(IntlTimeZone $timezone): string|false",
            tz_get_error_message,
        ),
        ("function intltz_get_gmt(): IntlTimeZone", tz_get_gmt),
        (
            "function intltz_get_iana_id(string $timezoneId): string|false",
            tz_get_iana_id,
        ),
        (
            "function intltz_get_id(IntlTimeZone $timezone): string|false",
            tz_get_id,
        ),
        (
            "function intltz_get_offset(IntlTimeZone $timezone, float $timestamp, bool $local, &$rawOffset, &$dstOffset): bool",
            tz_get_offset,
        ),
        (
            "function intltz_get_raw_offset(IntlTimeZone $timezone): int",
            tz_get_raw_offset,
        ),
        (
            "function intltz_get_region(string $timezoneId): string|false",
            tz_get_region,
        ),
        (
            "function intltz_get_tz_data_version(): string|false",
            tz_get_tz_data_version,
        ),
        (
            "function intltz_get_unknown(): IntlTimeZone",
            tz_get_unknown,
        ),
        (
            "function intltz_get_windows_id(string $timezoneId): string|false",
            tz_get_windows_id,
        ),
        (
            "function intltz_get_id_for_windows_id(string $timezoneId, ?string $region = null): string|false",
            tz_get_id_for_windows_id,
        ),
        (
            "function intltz_has_same_rules(IntlTimeZone $timezone, IntlTimeZone $other): bool",
            tz_has_same_rules,
        ),
        (
            "function intltz_to_date_time_zone(IntlTimeZone $timezone): DateTimeZone|false",
            tz_to_dtz,
        ),
        (
            "function intltz_use_daylight_time(IntlTimeZone $timezone): bool",
            tz_use_daylight_time,
        ),
    ],
    classes: &[("intltimezone", TZ_CLASS), ("intliterator", ITER_CLASS)],
    methods: &[
        ("intltimezone::countequivalentids", tz_count_equivalent),
        ("intltimezone::createdefault", tz_create_default),
        ("intltimezone::createenumeration", tz_create_enumeration),
        ("intltimezone::createtimezone", tz_create),
        (
            "intltimezone::createtimezoneidenumeration",
            tz_create_id_enumeration,
        ),
        ("intltimezone::fromdatetimezone", tz_from_dtz),
        ("intltimezone::getcanonicalid", tz_get_canonical_id),
        ("intltimezone::getdisplayname", tz_get_display_name),
        ("intltimezone::getdstsavings", tz_get_dst_savings),
        ("intltimezone::getequivalentid", tz_get_equivalent_id),
        ("intltimezone::geterrorcode", tz_get_error_code),
        ("intltimezone::geterrormessage", tz_get_error_message),
        ("intltimezone::getgmt", tz_get_gmt),
        ("intltimezone::getianaid", tz_get_iana_id),
        ("intltimezone::getid", tz_get_id),
        ("intltimezone::getoffset", tz_get_offset),
        ("intltimezone::getrawoffset", tz_get_raw_offset),
        ("intltimezone::getregion", tz_get_region),
        ("intltimezone::gettzdataversion", tz_get_tz_data_version),
        ("intltimezone::getunknown", tz_get_unknown),
        ("intltimezone::getwindowsid", tz_get_windows_id),
        ("intltimezone::getidforwindowsid", tz_get_id_for_windows_id),
        ("intltimezone::hassamerules", tz_has_same_rules),
        ("intltimezone::todatetimezone", tz_to_dtz),
        ("intltimezone::usedaylighttime", tz_use_daylight_time),
        ("intliterator::current", it_current),
        ("intliterator::key", it_key),
        ("intliterator::next", it_next),
        ("intliterator::rewind", it_rewind),
        ("intliterator::valid", it_valid),
    ],
    hooks: &[("intltimezone", tz_hooks), ("intliterator", it_hooks)],
    ..Module::EMPTY
};

const TZ_CLASS: &str = r#"<?php class IntlTimeZone {
    public const int DISPLAY_SHORT = 1;
    public const int DISPLAY_LONG = 2;
    public const int DISPLAY_SHORT_GENERIC = 3;
    public const int DISPLAY_LONG_GENERIC = 4;
    public const int DISPLAY_SHORT_GMT = 5;
    public const int DISPLAY_LONG_GMT = 6;
    public const int DISPLAY_SHORT_COMMONLY_USED = 7;
    public const int DISPLAY_GENERIC_LOCATION = 8;
    public const int TYPE_ANY = 0;
    public const int TYPE_CANONICAL = 1;
    public const int TYPE_CANONICAL_LOCATION = 2;
    private function __construct() {}
    public static function countEquivalentIDs(string $timezoneId): int|false {}
    public static function createDefault(): IntlTimeZone {}
    public static function createEnumeration(string|int|null $countryOrRawOffset = null): IntlIterator|false {}
    public static function createTimeZone(string $timezoneId): ?IntlTimeZone {}
    public static function createTimeZoneIDEnumeration(int $type, ?string $region = null, ?int $rawOffset = null): IntlIterator|false {}
    public static function fromDateTimeZone(DateTimeZone $timezone): ?IntlTimeZone {}
    public static function getCanonicalID(string $timezoneId, &$isSystemId = null): string|false {}
    public function getDisplayName(bool $dst = false, int $style = IntlTimeZone::DISPLAY_LONG, ?string $locale = null): string|false {}
    public function getDSTSavings(): int {}
    public static function getEquivalentID(string $timezoneId, int $offset): string|false {}
    public function getErrorCode(): int|false {}
    public function getErrorMessage(): string|false {}
    public static function getGMT(): IntlTimeZone {}
    public static function getIanaID(string $timezoneId): string|false {}
    public function getID(): string|false {}
    public function getOffset(float $timestamp, bool $local, &$rawOffset, &$dstOffset): bool {}
    public function getRawOffset(): int {}
    public static function getRegion(string $timezoneId): string|false {}
    public static function getTZDataVersion(): string|false {}
    public static function getUnknown(): IntlTimeZone {}
    public static function getWindowsID(string $timezoneId): string|false {}
    public static function getIDForWindowsID(string $timezoneId, ?string $region = null): string|false {}
    public function hasSameRules(IntlTimeZone $other): bool {}
    public function toDateTimeZone(): DateTimeZone|false {}
    public function useDaylightTime(): bool {}
}"#;

const ITER_CLASS: &str = r#"<?php class IntlIterator implements Iterator {
    public function current(): mixed {}
    public function key(): mixed {}
    public function next(): void {}
    public function rewind(): void {}
    public function valid(): bool {}
}"#;

// ---------------------------------------------------------------- helpers

/// The zone of an IntlTimeZone object.
pub(crate) fn peek_itz(o: &ObjRef) -> Option<TimeZone> {
    with_obj::<TimeZone, TimeZone>(o, |x| x.v.clone())
}

/// A new IntlTimeZone object.
pub(crate) fn new_itz(vm: &mut Vm, tz: TimeZone) -> R<Value> {
    Ok(Value::Object(new_obj(vm, "IntlTimeZone", tz)?))
}

fn this_tz(vm: &mut Vm, a: &Args) -> R<(ObjRef, usize, TimeZone)> {
    let (o, off) = this_or_first(a);
    match peek_itz(&o) {
        Some(t) => Ok((o, off, t)),
        None => super::not_constructed(vm, "IntlTimeZone"),
    }
}

/// A zone id argument as UTF-8 (None after setting the error).
fn id_arg(vm: &mut Vm, a: &Args, i: usize) -> R<Option<String>> {
    match std::str::from_utf8(a.bytes(i)) {
        Ok(s) => Ok(Some(s.to_string())),
        Err(_) => {
            fail(
                vm,
                a,
                ue::U_INVALID_CHAR_FOUND,
                "could not convert time zone id to UTF-16",
            )?;
            Ok(None)
        }
    }
}

/// The ICU zone of a PHP DateTimeZone value (as ext/intl converts it:
/// ids by name, abbreviations by their ICU id, offsets as custom zones),
/// or the error message.
pub(crate) fn from_php_zone(z: &goro_ext_date::TimeZone) -> Result<TimeZone, String> {
    use goro_ext_date::TimeZone as P;
    match z {
        P::Offset(secs) => {
            if secs.unsigned_abs() >= 24 * 3600 {
                return Err("object has an time zone offset that's too large".into());
            }
            Ok(TimeZone::fixed(secs * 1000))
        }
        P::Abbr { abbr, .. } => {
            let id = abbr.to_ascii_uppercase();
            TimeZone::try_create(&id).ok_or_else(|| {
                format!("time zone id '{id}' extracted from ext/date DateTimeZone not recognized")
            })
        }
        P::Id(idz) => {
            let name = idz.name();
            TimeZone::try_create(name)
                .or_else(|| TimeZone::try_create(idz.zone.name()))
                .ok_or_else(|| {
                    format!(
                        "time zone id '{name}' extracted from ext/date DateTimeZone not recognized"
                    )
                })
        }
    }
}

/// The default zone (PHP's default time zone, as ext/intl sets ICU's).
pub(crate) fn default_zone(vm: &mut Vm) -> TimeZone {
    let z = crate::builtins::date::default_tz(vm);
    from_php_zone(&z).unwrap_or_else(|_| TimeZone::create(&z.name()))
}

/// A DateTimeZone object for an ICU zone, through the DateTimeZone
/// constructor; its exception becomes the previous of an IntlException
/// "DateTimeZone constructor threw exception".
pub(crate) fn to_php_zone_obj(vm: &mut Vm, tz: &TimeZone) -> R<ObjRef> {
    let name = match tz.fixed_offset() {
        Some(o) => {
            let t = o.unsigned_abs() / 1000;
            format!(
                "{}{:02}:{:02}",
                if o < 0 { '-' } else { '+' },
                t / 3600,
                t / 60 % 60
            )
        }
        None => tz.id().to_string(),
    };
    let cls = super::class(vm, "DateTimeZone")?;
    let o = vm.instantiate_raw(&cls)?;
    match vm.call_method(&o, b"__construct", vec![Value::from(name.as_str())]) {
        Ok(_) => Ok(o),
        Err(Unwind::Throw(e)) => {
            let ie = vm.create_exception(
                "IntlException",
                "DateTimeZone constructor threw exception",
                0,
            )?;
            vm.chain_previous(&ie, e);
            Err(Unwind::Throw(ie))
        }
        Err(e) => Err(e),
    }
}

/// Whether `o` is an instance of the named class.
pub(crate) fn is_instance(vm: &mut Vm, o: &ObjRef, name: &str) -> bool {
    match vm.find_class(name.as_bytes(), false) {
        Ok(Some(c)) => vm.class_of(o).is_subclass_of(&c),
        _ => false,
    }
}

/// Raises an Error from a clone hook (which cannot fail directly).
pub(crate) fn clone_error(vm: &mut Vm, msg: &str) {
    if vm.err.deferred.is_none() {
        vm.err.deferred = Some(vm.throw_error_msg("Error", msg));
    }
}

/// Assigns to a by-reference argument, coercing to the types of typed
/// properties the reference is bound to.
pub(crate) fn set_ref_typed(vm: &mut Vm, a: &Args, i: usize, v: Value) -> R<()> {
    if let Some(r) = a.ref_at(i) {
        let v = vm.typed_ref_value(&r, v)?;
        a.set_ref(i, v);
    }
    Ok(())
}

/// The current time in ms.
fn now() -> f64 {
    itz::now_ms()
}

fn iterator(vm: &mut Vm, items: Vec<String>) -> R<Value> {
    Ok(Value::Object(new_string_iterator(vm, items)?))
}

// ---------------------------------------------------------------- IntlIterator

/// The state of an IntlIterator over strings.
#[derive(Clone)]
pub(crate) struct StrIter {
    pub items: Vec<String>,
    pub pos: usize,
}

/// A new IntlIterator over `items`.
pub(crate) fn new_string_iterator(vm: &mut Vm, items: Vec<String>) -> R<ObjRef> {
    new_obj(vm, "IntlIterator", StrIter { items, pos: 0 })
}

fn it_state<U>(vm: &mut Vm, a: &Args, f: impl FnOnce(&mut StrIter) -> U) -> R<U> {
    let o = a.this_obj().clone();
    match with_obj::<StrIter, U>(&o, |x| f(&mut x.v)) {
        Some(u) => Ok(u),
        None => vm.throw("Error", "The IntlIterator is not properly constructed"),
    }
}

fn it_current(vm: &mut Vm, a: &mut Args) -> R<Value> {
    it_state(vm, a, |s| match s.items.get(s.pos) {
        Some(v) => Value::from(v.as_str()),
        None => Value::Null,
    })
}

fn it_key(vm: &mut Vm, a: &mut Args) -> R<Value> {
    it_state(vm, a, |s| {
        if s.pos < s.items.len() {
            Value::Int(s.pos as i64)
        } else {
            Value::Null
        }
    })
}

fn it_next(vm: &mut Vm, a: &mut Args) -> R<Value> {
    it_state(vm, a, |s| {
        if s.pos < s.items.len() {
            s.pos += 1;
        }
    })?;
    Ok(Value::Null)
}

fn it_rewind(vm: &mut Vm, a: &mut Args) -> R<Value> {
    it_state(vm, a, |s| s.pos = 0)?;
    Ok(Value::Null)
}

fn it_valid(vm: &mut Vm, a: &mut Args) -> R<Value> {
    it_state(vm, a, |s| Value::Bool(s.pos < s.items.len()))
}

fn it_hooks() -> NativeHooks {
    NativeHooks {
        clone: Some(|vm, _from, _to| {
            clone_error(
                vm,
                "Trying to clone an uncloneable object of class IntlIterator",
            );
        }),
        ..Default::default()
    }
}

// ---------------------------------------------------------------- hooks

fn tz_hooks() -> NativeHooks {
    NativeHooks {
        clone: Some(|vm, from, to| match peek_itz(from) {
            Some(t) => super::store(to, t),
            None => clone_error(vm, "Cannot clone uninitialized IntlTimeZone"),
        }),
        debug_props: Some(|_, o| {
            let mut out = PhpArray::new();
            for (k, v) in o.obj().props.borrow().iter() {
                out.insert(k.to_owned(), v.clone());
            }
            match peek_itz(o) {
                None => {
                    out.insert(Key::from("valid"), Value::Bool(false));
                }
                Some(t) => {
                    let (raw, dst) = t.offsets(now());
                    out.insert(Key::from("valid"), Value::Bool(true));
                    out.insert(Key::from("id"), Value::from(t.id()));
                    out.insert(Key::from("rawOffset"), Value::Int(raw as i64));
                    out.insert(Key::from("currentOffset"), Value::Int((raw + dst) as i64));
                }
            }
            Some(out)
        }),
        compare: Some(|vm, x, y| {
            let (Value::Object(a), Value::Object(b)) = (x, y) else {
                return None;
            };
            if !is_instance(vm, a, "IntlTimeZone") || !is_instance(vm, b, "IntlTimeZone") {
                return None;
            }
            match (peek_itz(a), peek_itz(b)) {
                (Some(p), Some(q)) => Some(Ok(if p.equals(&q) { 0 } else { 1 })),
                _ => Some(vm.throw(
                    "Exception",
                    "Comparison with at least one unconstructed IntlTimeZone operand",
                )),
            }
        }),
        ..Default::default()
    }
}

// ---------------------------------------------------------------- functions

fn tz_create(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let Some(id) = id_arg(vm, a, 0)? else {
        return Ok(Value::Null);
    };
    new_itz(vm, TimeZone::create(&id))
}

fn tz_create_default(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    reset(vm);
    let z = default_zone(vm);
    new_itz(vm, z)
}

fn tz_get_gmt(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    reset(vm);
    new_itz(vm, TimeZone::gmt())
}

fn tz_get_unknown(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    reset(vm);
    new_itz(vm, TimeZone::unknown())
}

fn tz_from_dtz(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let o = a.obj(0).expect("DateTimeZone");
    let z = crate::builtins::date::get_tz(vm, &o)?;
    match from_php_zone(&z) {
        Ok(t) => new_itz(vm, t),
        Err(m) => {
            fail(vm, a, ue::U_ILLEGAL_ARGUMENT_ERROR, &m)?;
            Ok(Value::Null)
        }
    }
}

fn tz_create_enumeration(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let ids = match a.val(0) {
        Value::Null => itz::enumerate(0, None, None),
        Value::Int(n) => itz::enumerate(0, None, Some(n as i32)),
        Value::Float(f) => itz::enumerate(0, None, Some(f as i32)),
        Value::Str(s) => {
            let r = s.to_string_lossy();
            itz::enumerate(0, Some(&r), None)
        }
        Value::Object(_) => {
            fail(vm, a, ue::U_ILLEGAL_ARGUMENT_ERROR, "invalid argument type")?;
            return Ok(Value::Bool(false));
        }
        _ => itz::enumerate(0, None, None),
    };
    iterator(vm, ids)
}

fn tz_create_id_enumeration(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let ty = a.int(0);
    if !(0..=2).contains(&ty) {
        return vm.value_error(&format!(
            "{}(): Argument #1 ($type) must be one of IntlTimeZone::TYPE_ANY, IntlTimeZone::TYPE_CANONICAL, or IntlTimeZone::TYPE_CANONICAL_LOCATION",
            a.fname()
        ));
    }
    let region = a.opt_str(1).map(|s| s.to_string_lossy());
    let off = match a.opt_int(2) {
        Some(o) => {
            if o < i32::MIN as i64 || o > i32::MAX as i64 {
                return vm.value_error(&format!(
                    "{}(): Argument #3 ($rawOffset) must be between {} and {}",
                    a.fname(),
                    i32::MIN,
                    i32::MAX
                ));
            }
            Some(o as i32)
        }
        None => None,
    };
    let ids = itz::enumerate(ty, region.as_deref(), off);
    iterator(vm, ids)
}

fn tz_count_equivalent(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let Some(id) = id_arg(vm, a, 0)? else {
        return Ok(Value::Bool(false));
    };
    Ok(Value::Int(itz::equivalent_ids(&id).len() as i64))
}

fn tz_get_equivalent_id(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let Some(id) = id_arg(vm, a, 0)? else {
        return Ok(Value::Bool(false));
    };
    let i = a.int(1);
    let v = itz::equivalent_ids(&id);
    Ok(match usize::try_from(i).ok().and_then(|i| v.get(i)) {
        Some(s) => Value::from(s.as_str()),
        None => Value::from(""),
    })
}

fn tz_get_canonical_id(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let Some(id) = id_arg(vm, a, 0)? else {
        return Ok(Value::Bool(false));
    };
    match itz::canonical_id(&id) {
        Some((c, sys)) => {
            if a.has(1) {
                set_ref_typed(vm, a, 1, Value::Bool(sys))?;
            }
            Ok(Value::from(c.as_str()))
        }
        None => {
            fail(
                vm,
                a,
                ue::U_ILLEGAL_ARGUMENT_ERROR,
                "error obtaining canonical ID",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn tz_get_region(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let Some(id) = id_arg(vm, a, 0)? else {
        return Ok(Value::Bool(false));
    };
    match itz::region(&id) {
        Some(r) => Ok(Value::from(r.as_str())),
        None => {
            fail(
                vm,
                a,
                ue::U_ILLEGAL_ARGUMENT_ERROR,
                "error obtaining region",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn tz_get_iana_id(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let Some(id) = id_arg(vm, a, 0)? else {
        return Ok(Value::Bool(false));
    };
    match itz::iana_id(&id) {
        Some(r) => Ok(Value::from(r.as_str())),
        None => {
            fail(
                vm,
                a,
                ue::U_ILLEGAL_ARGUMENT_ERROR,
                "error obtaining IANA ID",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn tz_get_tz_data_version(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    reset(vm);
    Ok(Value::from(itz::tz_data_version()))
}

fn tz_get_windows_id(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let Some(id) = id_arg(vm, a, 0)? else {
        return Ok(Value::Bool(false));
    };
    match itz::windows_id(&id) {
        Some(w) if !w.is_empty() => Ok(Value::from(w.as_str())),
        Some(_) => {
            fail(
                vm,
                a,
                ue::U_ILLEGAL_ARGUMENT_ERROR,
                "unknown windows timezone",
            )?;
            Ok(Value::Bool(false))
        }
        None => {
            fail(
                vm,
                a,
                ue::U_ILLEGAL_ARGUMENT_ERROR,
                "unknown system timezone",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn tz_get_id_for_windows_id(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let Some(id) = id_arg(vm, a, 0)? else {
        return Ok(Value::Bool(false));
    };
    let region = a.opt_str(1).map(|s| s.to_string_lossy());
    match itz::id_for_windows_id(&id, region.as_deref()) {
        Some(z) => Ok(Value::from(z.as_str())),
        None => {
            fail(
                vm,
                a,
                ue::U_ILLEGAL_ARGUMENT_ERROR,
                "unknown windows timezone",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn tz_get_id(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _, t) = this_tz(vm, a)?;
    reset_obj::<TimeZone>(vm, &o);
    Ok(Value::from(t.id()))
}

fn tz_get_raw_offset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _, t) = this_tz(vm, a)?;
    reset_obj::<TimeZone>(vm, &o);
    Ok(Value::Int(t.raw_offset() as i64))
}

fn tz_get_dst_savings(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _, t) = this_tz(vm, a)?;
    reset_obj::<TimeZone>(vm, &o);
    Ok(Value::Int(t.dst_savings() as i64))
}

fn tz_use_daylight_time(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _, t) = this_tz(vm, a)?;
    reset_obj::<TimeZone>(vm, &o);
    Ok(Value::Bool(t.use_daylight_time()))
}

fn tz_has_same_rules(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, t) = this_tz(vm, a)?;
    reset_obj::<TimeZone>(vm, &o);
    let other = a.obj(off).expect("IntlTimeZone");
    match peek_itz(&other) {
        Some(u) => Ok(Value::Bool(t.has_same_rules(&u))),
        None => {
            fail_obj::<TimeZone>(
                vm,
                a,
                &o,
                ue::U_ILLEGAL_ARGUMENT_ERROR,
                "The second IntlTimeZone is unconstructed",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn tz_get_offset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, t) = this_tz(vm, a)?;
    reset_obj::<TimeZone>(vm, &o);
    let date = a.float(off);
    let local = a.bool(off + 1);
    if !date.is_finite() {
        fail_obj::<TimeZone>(
            vm,
            a,
            &o,
            ue::U_ILLEGAL_ARGUMENT_ERROR,
            "error obtaining offset",
        )?;
        return Ok(Value::Bool(false));
    }
    let (raw, dst) = if local {
        t.offsets_local(date, itz::WallTime::First, itz::WallTime::First)
    } else {
        t.offsets(date)
    };
    set_ref_typed(vm, a, off + 2, Value::Int(raw as i64))?;
    set_ref_typed(vm, a, off + 3, Value::Int(dst as i64))?;
    Ok(Value::Bool(true))
}

fn tz_get_display_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, t) = this_tz(vm, a)?;
    reset_obj::<TimeZone>(vm, &o);
    let dst = a.bool_or(off, false);
    let style = a.int_or(off + 1, 2);
    let st = match style {
        1 | 7 => Style::ShortSpecific,
        2 => Style::LongSpecific,
        3 => Style::ShortGeneric,
        4 => Style::LongGeneric,
        5 => Style::ShortGmt,
        6 => Style::LongGmt,
        8 => Style::GenericLocation,
        _ => {
            fail_obj::<TimeZone>(
                vm,
                a,
                &o,
                ue::U_ILLEGAL_ARGUMENT_ERROR,
                "wrong display type",
            )?;
            return Ok(Value::Bool(false));
        }
    };
    let locale = match a.opt_str(off + 2) {
        Some(s) if !s.as_bytes().is_empty() => s.to_string_lossy(),
        _ => super::default_locale(vm),
    };
    let l = goro_ext_intl::datefmt::LocaleId::parse(&locale);
    let loc = l.data_loc();
    let ms = now();
    let s = if style == 5 {
        // DISPLAY_SHORT_GMT: the RFC 822 form ("+0000")
        let (raw, d) = t.offsets(ms);
        let total = if dst {
            raw + if d != 0 { d } else { t.dst_savings() }
        } else {
            raw
        };
        names::iso_offset(total, false, names::Opt::Required, false, false)
    } else {
        names::display_name(loc, &l.base, &t, ms, st, Some(dst && t.use_daylight_time()))
    };
    Ok(Value::from(s.as_str()))
}

fn tz_to_dtz(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _, t) = this_tz(vm, a)?;
    reset_obj::<TimeZone>(vm, &o);
    Ok(Value::Object(to_php_zone_obj(vm, &t)?))
}

fn tz_get_error_code(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_or_first(a);
    if !has_obj::<TimeZone>(&o) {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Int(obj_err::<TimeZone>(&o).code))
}

fn tz_get_error_message(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_or_first(a);
    if !has_obj::<TimeZone>(&o) {
        return Ok(Value::Bool(false));
    }
    Ok(Value::from(obj_err::<TimeZone>(&o).full_message().as_str()))
}
