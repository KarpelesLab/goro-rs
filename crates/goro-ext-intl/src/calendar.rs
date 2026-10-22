//! ext/intl/calendar: IntlCalendar, IntlGregorianCalendar and the intlcal_*() / intlgregcal_*() functions.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{BuiltinFn, NativeObjectHandlers, Vm, VmError};

use crate::datecore::cal::{NF, WALLTIME_FIRST, WALLTIME_LAST, WALLTIME_NEXT_VALID};
use crate::datecore::{self, CalState, CalType, Calendar, data, zone};
use crate::error::{self, U_ILLEGAL_ARGUMENT_ERROR};
use crate::{Call, Obj, bool_v, s};

/// The procedural functions; intlcal_set() and intlgregcal_create_instance()
/// have their own wrappers (deprecation before argument parsing, the
/// latter's own argument count check).
macro_rules! cal_functions {
    ($( $name:literal => $imp:path ),* $(,)? ; $( $rname:literal => $rw:path, $rimp:path ),* $(,)?) => {
        pub const FUNCTIONS: &[(&str, BuiltinFn, crate::Imp)] = &[
            $( ($name, {
                fn w(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
                    crate::call_function(vm, $name, $imp, args)
                }
                w
            }, $imp), )*
            $( ($rname, $rw, $rimp), )*
        ];
    };
}

cal_functions! {
    "intlcal_create_instance" => create_instance,
    "intlcal_get_keyword_values_for_locale" => get_keyword_values_for_locale,
    "intlcal_get_now" => get_now,
    "intlcal_get_available_locales" => get_available_locales,
    "intlcal_get" => get,
    "intlcal_get_time" => get_time,
    "intlcal_set_time" => set_time,
    "intlcal_add" => add,
    "intlcal_set_time_zone" => set_time_zone,
    "intlcal_after" => after,
    "intlcal_before" => before,
    "intlcal_roll" => roll,
    "intlcal_clear" => clear,
    "intlcal_field_difference" => field_difference,
    "intlcal_get_actual_maximum" => get_actual_maximum,
    "intlcal_get_actual_minimum" => get_actual_minimum,
    "intlcal_get_day_of_week_type" => get_day_of_week_type,
    "intlcal_get_first_day_of_week" => get_first_day_of_week,
    "intlcal_get_greatest_minimum" => get_greatest_minimum,
    "intlcal_get_least_maximum" => get_least_maximum,
    "intlcal_get_locale" => get_locale,
    "intlcal_get_maximum" => get_maximum,
    "intlcal_get_minimal_days_in_first_week" => get_minimal_days_in_first_week,
    "intlcal_set_minimal_days_in_first_week" => set_minimal_days_in_first_week,
    "intlcal_get_minimum" => get_minimum,
    "intlcal_get_time_zone" => get_time_zone,
    "intlcal_get_type" => get_type,
    "intlcal_get_weekend_transition" => get_weekend_transition,
    "intlcal_in_daylight_time" => in_daylight_time,
    "intlcal_is_lenient" => is_lenient,
    "intlcal_is_set" => is_set,
    "intlcal_is_equivalent_to" => is_equivalent_to,
    "intlcal_is_weekend" => is_weekend,
    "intlcal_set_first_day_of_week" => set_first_day_of_week,
    "intlcal_set_lenient" => set_lenient,
    "intlcal_get_repeated_wall_time_option" => get_repeated_wall_time_option,
    "intlcal_equals" => equals,
    "intlcal_get_skipped_wall_time_option" => get_skipped_wall_time_option,
    "intlcal_set_repeated_wall_time_option" => set_repeated_wall_time_option,
    "intlcal_set_skipped_wall_time_option" => set_skipped_wall_time_option,
    "intlcal_from_date_time" => from_date_time,
    "intlcal_to_date_time" => to_date_time,
    "intlcal_get_error_code" => get_error_code,
    "intlcal_get_error_message" => get_error_message,
    "intlgregcal_set_gregorian_change" => set_gregorian_change,
    "intlgregcal_get_gregorian_change" => get_gregorian_change,
    "intlgregcal_is_leap_year" => is_leap_year;
    "intlcal_set" => intlcal_set_wrapper, set,
    "intlgregcal_create_instance" => gregcal_create_instance_wrapper, gregcal_create_instance,
}

pub const METHODS: &[(&str, crate::Imp)] = &[
    ("IntlCalendar::__construct", construct),
    ("IntlCalendar::setTimeZone", set_time_zone),
    ("IntlCalendar::setDate", set_date),
    ("IntlCalendar::setDateTime", set_date_time),
    ("IntlGregorianCalendar::__construct", gregcal_construct),
    ("IntlGregorianCalendar::createFromDate", create_from_date),
    ("IntlGregorianCalendar::createFromDateTime", create_from_date_time),
];

pub const CLASSES: &[(&str, Option<crate::obj::CreateFn>)] = &[
    ("IntlCalendar", Some(create_object)),
    ("IntlGregorianCalendar", Some(create_object)),
];

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    get_properties: Some(debug_info),
    clone_obj: Some(clone_obj),
    ..NativeObjectHandlers::EMPTY
};

fn create_object(_vm: &mut Vm, o: &Obj) {
    crate::obj::attach(o, CalState::default(), &HANDLERS);
}

fn state(o: &Obj) -> Option<Rc<RefCell<CalState>>> {
    crate::obj::state::<CalState>(o)
}

const FIELD_NAMES: [&str; 23] = [
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
];
const FIELD_ORDER: [usize; 23] = [
    0, 1, 2, 3, 4, 6, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
];

fn key(k: &str) -> ArrayKey {
    ArrayKey::String(PhpString::from_bytes(k.as_bytes()))
}

/// Calendar_get_debug_info
fn debug_info(vm: &mut Vm, o: &Obj, purpose: u8) -> Option<PhpArray> {
    if purpose != 0 {
        return None;
    }
    let mut a = PhpArray::new();
    let Some(st) = state(o) else {
        a.set(key("valid"), Value::False);
        return Some(a);
    };
    let mut b = st.borrow_mut();
    let Some(cal) = b.cal.as_mut() else {
        a.set(key("valid"), Value::False);
        return Some(a);
    };
    a.set(key("valid"), Value::True);
    a.set(key("type"), s(cal.type_name()));
    // php-src builds (and never frees) an IntlTimeZone object here
    let _ = vm.alloc_object_id();
    let mut tz = PhpArray::new();
    tz.set(key("valid"), Value::True);
    tz.set(key("id"), s(&cal.zone.id));
    let (r, d) = cal.zone.offsets_utc(zone::now_millis());
    tz.set(key("rawOffset"), Value::Long(r as i64));
    tz.set(key("currentOffset"), Value::Long((r + d) as i64));
    a.set(key("timeZone"), crate::arr(tz));
    let (_, valid) = datecore::calendar_locales(&cal.locale);
    a.set(key("locale"), s(valid));
    let mut f = PhpArray::new();
    for (i, &fi) in FIELD_ORDER.iter().enumerate() {
        match cal.get(fi) {
            Ok(v) => f.set(key(FIELD_NAMES[i]), Value::Long(v as i64)),
            Err(()) => f.set(key(FIELD_NAMES[i]), s("U_ILLEGAL_ARGUMENT_ERROR")),
        }
    }
    a.set(key("fields"), crate::arr(f));
    Some(a)
}

/// Calendar_clone_obj
fn clone_obj(vm: &mut Vm, src: &Obj, dst: &Obj) {
    let cal = state(src).and_then(|s| s.borrow().cal.clone());
    let none = cal.is_none();
    crate::obj::attach(
        dst,
        CalState {
            cal,
            err: Default::default(),
        },
        &HANDLERS,
    );
    if none {
        crate::zpp::throw(vm, b"Error", "Cannot clone uninitialized IntlCalendar");
    }
}

fn construct(vm: &mut Vm, _c: &mut Call) -> Value {
    // the constructor is private
    if let Some(m) = crate::datecore::private_ctor_error(vm, "IntlCalendar") {
        crate::zpp::throw(vm, b"Error", &m);
        return Value::Null;
    }
    crate::zpp::throw(
        vm,
        b"Exception",
        "An object of this type cannot be created with the new operator",
    );
    Value::Null
}

fn obj_value(o: Option<Obj>) -> Value {
    match o {
        Some(o) => Value::Object(o),
        None => Value::Null,
    }
}

/// CALENDAR_METHOD_FETCH_OBJECT
fn fetch(vm: &mut Vm, c: &Call) -> Option<Rc<RefCell<CalState>>> {
    error::reset_global();
    let o = c.this.clone()?;
    let st = state(&o)?;
    st.borrow_mut().err.reset();
    if st.borrow().cal.is_none() {
        crate::zpp::throw(vm, b"Error", "Found unconstructed IntlCalendar");
        return None;
    }
    Some(st)
}

/// INTL_METHOD_CHECK_STATUS on failure
fn fail(vm: &mut Vm, c: &Call, st: &Rc<RefCell<CalState>>, msg: &str) -> Value {
    let name = c.name.clone();
    error::set_both(
        vm,
        &name,
        &mut st.borrow_mut().err,
        U_ILLEGAL_ARGUMENT_ERROR,
        msg,
    );
    Value::False
}

/// Run `f` on the calendar of a fetched object.
fn with_cal<T>(st: &Rc<RefCell<CalState>>, f: impl FnOnce(&mut Calendar) -> T) -> T {
    let mut b = st.borrow_mut();
    f(b.cal.as_mut().expect("constructed"))
}

fn param_name(c: &Call, i: usize) -> &'static str {
    let stub = if c.is_method {
        c.name
            .split_once("::")
            .and_then(|(cl, m)| goro_core::stubs::find_method(cl.as_bytes(), m.as_bytes()))
            .map(|(_, m)| m)
    } else {
        goro_core::stubs::find_function(c.name.as_bytes())
    };
    let off = if c.is_method || c.this.is_none() { 0 } else { 1 };
    stub.and_then(|s| s.args.get(i + off))
        .map(|a| a.name)
        .unwrap_or("")
}

/// ZEND_VALUE_ERROR_INVALID_FIELD
fn check_field(vm: &mut Vm, c: &Call, i: usize) -> Option<usize> {
    let f = c.long(i);
    if !(0..NF as i64).contains(&f) {
        crate::zpp::value_error(vm, &c.name, c.argn(i), param_name(c, i), "must be a valid field");
        return None;
    }
    Some(f as usize)
}

/// ZEND_VALUE_ERROR_INVALID_DAY_OF_WEEK
fn check_dow(vm: &mut Vm, c: &Call, i: usize) -> Option<i32> {
    let d = c.long(i);
    if !(1..=7).contains(&d) {
        crate::zpp::value_error(
            vm,
            &c.name,
            c.argn(i),
            param_name(c, i),
            "must be a valid day of the week",
        );
        return None;
    }
    Some(d as i32)
}

fn out_of_bound(vm: &mut Vm, name: &str, pos: usize, pname: &str) {
    crate::zpp::value_error(vm, name, pos, pname, "must be between -2147483648 and 2147483647");
}

fn in_i32(v: i64) -> bool {
    v >= i32::MIN as i64 && v <= i32::MAX as i64
}

// ---------------------------------------------------------------------------

fn create_instance(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let name = c.name.clone();
    let Ok(z) = datecore::zone_from_arg(vm, &c.arg(0), &name, None) else {
        return Value::Null;
    };
    let locale = match c.opt_str(1) {
        Some(l) => String::from_utf8_lossy(&l).into_owned(),
        None => crate::common::default_locale(vm),
    };
    let cal = datecore::create_calendar(z, &locale);
    obj_value(datecore::new_cal_object_from(vm, cal))
}

fn get_keyword_values_for_locale(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let locale = String::from_utf8_lossy(&c.str(1)).into_owned();
    let common = c.bool(2);
    let region = datecore::locale_region(&locale);
    let prefs: &[&str] = data::CAL_PREFS
        .iter()
        .find(|(r, _)| *r == region)
        .or_else(|| data::CAL_PREFS.iter().find(|(r, _)| *r == "001"))
        .map(|(_, p)| *p)
        .unwrap_or(&["gregorian"]);
    let mut out: Vec<String> = prefs.iter().map(|s| s.to_string()).collect();
    if !common {
        for t in data::ALL_CALENDARS {
            if !out.iter().any(|o| o == t) {
                out.push(t.to_string());
            }
        }
    }
    crate::timezone::string_enum_iterator(vm, out)
}

fn get_now(_vm: &mut Vm, _c: &mut Call) -> Value {
    error::reset_global();
    Value::Double(zone::now_millis())
}

fn get_available_locales(_vm: &mut Vm, _c: &mut Call) -> Value {
    error::reset_global();
    let mut a = PhpArray::new();
    for l in data::AVAILABLE_LOCALES {
        a.push(s(l));
    }
    crate::arr(a)
}

fn field_op(
    vm: &mut Vm,
    c: &mut Call,
    f: impl FnOnce(&mut Calendar, usize) -> Result<i32, ()>,
) -> Value {
    error::reset_global();
    let Some(field) = check_field(vm, c, 0) else {
        return Value::Null;
    };
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    match with_cal(&st, |cal| f(cal, field)) {
        Ok(v) => Value::Long(v as i64),
        Err(()) => fail(vm, c, &st, "Call to ICU method has failed"),
    }
}

fn get(vm: &mut Vm, c: &mut Call) -> Value {
    field_op(vm, c, |cal, f| cal.get(f))
}

fn get_actual_maximum(vm: &mut Vm, c: &mut Call) -> Value {
    field_op(vm, c, |cal, f| cal.get_actual_maximum(f))
}

fn get_actual_minimum(vm: &mut Vm, c: &mut Call) -> Value {
    field_op(vm, c, |cal, f| cal.get_actual_minimum(f))
}

fn get_greatest_minimum(vm: &mut Vm, c: &mut Call) -> Value {
    field_op(vm, c, |cal, f| Ok(cal.get_greatest_minimum(f)))
}

fn get_least_maximum(vm: &mut Vm, c: &mut Call) -> Value {
    field_op(vm, c, |cal, f| Ok(cal.get_least_maximum(f)))
}

fn get_maximum(vm: &mut Vm, c: &mut Call) -> Value {
    field_op(vm, c, |cal, f| Ok(cal.get_maximum(f)))
}

fn get_minimum(vm: &mut Vm, c: &mut Call) -> Value {
    field_op(vm, c, |cal, f| Ok(cal.get_minimum(f)))
}

fn get_time(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    match with_cal(&st, |cal| cal.get_time()) {
        Ok(t) => Value::Double(t),
        Err(()) => fail(vm, c, &st, "error calling ICU Calendar::getTime"),
    }
}

fn set_time(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let t = c.double(0);
    match with_cal(&st, |cal| cal.set_time(t)) {
        Ok(()) => Value::True,
        Err(()) => fail(vm, c, &st, "Call to underlying method failed"),
    }
}

fn add(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Some(field) = check_field(vm, c, 0) else {
        return Value::Null;
    };
    let amount = c.long(1);
    if !in_i32(amount) {
        out_of_bound(vm, &c.name, c.argn(1), param_name(c, 1));
        return Value::Null;
    }
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    match with_cal(&st, |cal| cal.add(field, amount as i32)) {
        Ok(()) => Value::True,
        Err(()) => fail(vm, c, &st, "Call to underlying method failed"),
    }
}

/// intlcal_set_time_zone() / IntlCalendar::setTimeZone()
fn set_time_zone(vm: &mut Vm, c: &mut Call) -> Value {
    // intlcal_set_time_zone() is no @alias: its object comes first
    if !c.is_method && c.this.is_none() {
        if let Value::Object(o) = c.arg(0) {
            c.this = Some(o);
            c.a.v.remove(0);
            c.a.given = c.a.given.saturating_sub(1);
        }
    }
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let v = c.arg(0);
    if matches!(v, Value::Null | Value::Undef) {
        return Value::True;
    }
    let name = c.name.clone();
    let mut err = std::mem::take(&mut st.borrow_mut().err);
    let r = datecore::zone_from_arg(vm, &v, &name, Some(&mut err));
    st.borrow_mut().err = err;
    match r {
        Ok(z) => {
            with_cal(&st, |cal| cal.set_zone(z));
            Value::True
        }
        Err(()) => Value::False,
    }
}

fn other_cal(vm: &mut Vm, c: &Call, i: usize) -> Option<Calendar> {
    let Value::Object(o) = c.arg(i) else {
        return None;
    };
    let cal = state(&o).and_then(|s| s.borrow().cal.clone());
    if cal.is_none() {
        let m = format!(
            "{}(): Argument #{} (${}) is uninitialized",
            c.name,
            c.argn(i),
            param_name(c, i)
        );
        crate::zpp::throw(vm, b"Error", &m);
    }
    cal
}

fn compare_time(vm: &mut Vm, c: &mut Call, f: fn(f64, f64) -> bool, msg: &str) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let Some(mut other) = other_cal(vm, c, 0) else {
        return Value::Null;
    };
    let r = with_cal(&st, |cal| -> Result<bool, ()> {
        let a = cal.get_time()?;
        let b = other.get_time()?;
        Ok(f(a, b))
    });
    match r {
        Ok(b) => bool_v(b),
        Err(()) => fail(vm, c, &st, msg),
    }
}

fn after(vm: &mut Vm, c: &mut Call) -> Value {
    compare_time(vm, c, |a, b| a > b, "Error calling ICU method")
}

fn before(vm: &mut Vm, c: &mut Call) -> Value {
    compare_time(vm, c, |a, b| a < b, "Error calling ICU method")
}

fn equals(vm: &mut Vm, c: &mut Call) -> Value {
    compare_time(vm, c, |a, b| a == b, "error calling ICU Calendar::equals")
}

fn intlcal_set_wrapper(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.emit_deprecated(
        "Function intlcal_set() is deprecated since 8.4, use IntlCalendar::set(), IntlCalendar::setDate(), or IntlCalendar::setDateTime() instead",
    );
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    crate::call_function(vm, "intlcal_set", set, args)
}

/// IntlCalendar::set()'s deprecation, before its arguments are parsed.
pub fn pre_set(vm: &mut Vm, args: &[Value]) -> bool {
    let n = args
        .iter()
        .rposition(|a| !matches!(a, Value::Undef))
        .map_or(0, |p| p + 1);
    if n > 2 {
        vm.emit_deprecated(
            "Calling IntlCalendar::set() with more than 2 arguments is deprecated, use either IntlCalendar::setDate() or IntlCalendar::setDateTime() instead",
        );
        if vm.current_exception.is_some() {
            return false;
        }
    }
    true
}

/// intlcal_set() / IntlCalendar::set()
fn set(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let arg_num = c.a.given;
    let args: Vec<i64> = (0..6).map(|i| c.long(i)).collect();
    for (i, &a) in args.iter().enumerate().take(arg_num) {
        if !in_i32(a) {
            let pos = if c.is_method { i } else { i + 1 };
            out_of_bound(vm, &c.name, pos, param_name(c, i));
            return Value::Null;
        }
    }
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let a: Vec<i32> = args.iter().map(|&v| v as i32).collect();
    match arg_num {
        2 => {
            let f = args[0];
            if !(0..NF as i64).contains(&f) {
                crate::zpp::value_error(
                    vm,
                    &c.name,
                    c.argn(0),
                    param_name(c, 0),
                    "must be a valid field",
                );
                return Value::Null;
            }
            with_cal(&st, |cal| cal.set(f as usize, a[1]));
        }
        3 => with_cal(&st, |cal| cal.set_ymd(a[0], a[1], a[2])),
        4 => {
            crate::zpp::throw(
                vm,
                b"ArgumentCountError",
                "IntlCalendar::set() has no variant with exactly 4 parameters",
            );
            return Value::Null;
        }
        5 => with_cal(&st, |cal| {
            cal.set_ymd(a[0], a[1], a[2]);
            cal.set(datecore::HOUR_OF_DAY, a[3]);
            cal.set(datecore::MINUTE, a[4]);
        }),
        _ => with_cal(&st, |cal| {
            cal.set_ymd(a[0], a[1], a[2]);
            cal.set(datecore::HOUR_OF_DAY, a[3]);
            cal.set(datecore::MINUTE, a[4]);
            cal.set(datecore::SECOND, a[5]);
        }),
    }
    Value::True
}

fn set_date(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    for i in 0..3 {
        if !in_i32(c.long(i)) {
            out_of_bound(vm, &c.name, i, param_name(c, i));
            return Value::Null;
        }
    }
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let (y, m, d) = (c.long(0) as i32, c.long(1) as i32, c.long(2) as i32);
    with_cal(&st, |cal| cal.set_ymd(y, m, d));
    Value::Null
}

fn set_date_time(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    for i in 0..5 {
        if !in_i32(c.long(i)) {
            out_of_bound(vm, &c.name, i, param_name(c, i));
            return Value::Null;
        }
    }
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let v: Vec<i32> = (0..5).map(|i| c.long(i) as i32).collect();
    let sec = c.opt_long(5);
    if let Some(sv) = sec {
        if !in_i32(sv) {
            out_of_bound(vm, &c.name, 5, param_name(c, 5));
            return Value::Null;
        }
    }
    with_cal(&st, |cal| {
        cal.set_ymd(v[0], v[1], v[2]);
        cal.set(datecore::HOUR_OF_DAY, v[3]);
        cal.set(datecore::MINUTE, v[4]);
        if let Some(sv) = sec {
            cal.set(datecore::SECOND, sv as i32);
        }
    });
    Value::Null
}

fn roll(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let Some(field) = check_field(vm, c, 0) else {
        return Value::Null;
    };
    let value = match c.arg(1) {
        Value::True | Value::False => {
            let v = if matches!(c.arg(1), Value::True) { 1 } else { -1 };
            vm.emit_deprecated(&format!(
                "{}(): Passing bool is deprecated, use 1 or -1 instead",
                c.name
            ));
            if vm.current_exception.is_some() {
                return Value::Null;
            }
            v
        }
        v => {
            let n = v.to_long();
            if !in_i32(n) {
                out_of_bound(vm, &c.name, c.argn(1), param_name(c, 1));
                return Value::Null;
            }
            n
        }
    };
    match with_cal(&st, |cal| cal.roll(field, value as i32)) {
        Ok(()) => Value::True,
        Err(()) => fail(vm, c, &st, "Error calling ICU Calendar::roll"),
    }
}

fn clear(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    match c.opt_long(0) {
        None => with_cal(&st, |cal| cal.clear_all()),
        Some(_) => {
            let Some(f) = check_field(vm, c, 0) else {
                return Value::Null;
            };
            with_cal(&st, |cal| cal.clear(f));
        }
    }
    Value::True
}

fn field_difference(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Some(field) = check_field(vm, c, 1) else {
        return Value::Null;
    };
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let when = c.double(0);
    match with_cal(&st, |cal| cal.field_difference(when, field)) {
        Ok(v) => Value::Long(v as i64),
        Err(()) => fail(vm, c, &st, "Call to ICU method has failed"),
    }
}

fn get_day_of_week_type(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Some(dow) = check_dow(vm, c, 0) else {
        return Value::Null;
    };
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    match with_cal(&st, |cal| cal.day_of_week_type(dow)) {
        Ok(v) => Value::Long(v as i64),
        Err(()) => fail(vm, c, &st, "Call to ICU method has failed"),
    }
}

fn get_weekend_transition(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Some(dow) = check_dow(vm, c, 0) else {
        return Value::Null;
    };
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    match with_cal(&st, |cal| cal.weekend_transition(dow)) {
        Ok(v) => Value::Long(v as i64),
        Err(()) => fail(vm, c, &st, "Error calling ICU method"),
    }
}

fn get_first_day_of_week(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    Value::Long(with_cal(&st, |cal| cal.first_day) as i64)
}

fn get_minimal_days_in_first_week(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    Value::Long(with_cal(&st, |cal| cal.min_days) as i64)
}

fn set_first_day_of_week(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Some(dow) = check_dow(vm, c, 0) else {
        return Value::Null;
    };
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    with_cal(&st, |cal| cal.set_first_day(dow));
    Value::True
}

fn set_minimal_days_in_first_week(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let n = c.long(0);
    if !(1..=7).contains(&n) {
        crate::zpp::value_error(
            vm,
            &c.name,
            if c.is_method { 1 } else { 2 },
            param_name(c, 0),
            "must be between 1 and 7",
        );
        return Value::Null;
    }
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    with_cal(&st, |cal| cal.set_min_days(n as i32));
    Value::True
}

fn get_locale(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let t = c.long(0);
    if t != 0 && t != 1 {
        crate::zpp::value_error(
            vm,
            &c.name,
            if c.is_method { 1 } else { 2 },
            param_name(c, 0),
            "must be either Locale::ACTUAL_LOCALE or Locale::VALID_LOCALE",
        );
        return Value::Null;
    }
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let (actual, valid) = with_cal(&st, |cal| datecore::calendar_locales(&cal.locale));
    s(if t == 0 { actual } else { valid })
}

fn get_time_zone(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let z = with_cal(&st, |cal| cal.zone.clone());
    obj_value(datecore::new_tz_object_zone(vm, z))
}

fn get_type(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    s(with_cal(&st, |cal| cal.type_name()))
}

fn in_daylight_time(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    match with_cal(&st, |cal| cal.in_daylight_time()) {
        Ok(b) => bool_v(b),
        Err(()) => fail(vm, c, &st, "Error calling ICU method"),
    }
}

fn is_equivalent_to(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Some(other) = other_cal(vm, c, 0) else {
        return Value::Null;
    };
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    bool_v(with_cal(&st, |cal| cal.is_equivalent_to(&other)))
}

fn is_lenient(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    bool_v(with_cal(&st, |cal| cal.lenient))
}

fn set_lenient(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let l = c.bool(0);
    with_cal(&st, |cal| cal.set_lenient(l));
    Value::True
}

fn is_set(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Some(f) = check_field(vm, c, 0) else {
        return Value::Null;
    };
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    bool_v(with_cal(&st, |cal| cal.is_set(f)))
}

fn is_weekend(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    match c.arg(0) {
        Value::Null | Value::Undef => bool_v(with_cal(&st, |cal| cal.is_weekend())),
        _ => {
            let d = c.double(0);
            match with_cal(&st, |cal| cal.is_weekend_at(d)) {
                Ok(b) => bool_v(b),
                Err(()) => fail(vm, c, &st, "Error calling ICU method"),
            }
        }
    }
}

fn get_repeated_wall_time_option(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    Value::Long(with_cal(&st, |cal| cal.repeated_wall) as i64)
}

fn get_skipped_wall_time_option(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    Value::Long(with_cal(&st, |cal| cal.skipped_wall) as i64)
}

fn set_repeated_wall_time_option(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let o = c.long(0);
    if o != WALLTIME_FIRST as i64 && o != WALLTIME_LAST as i64 {
        crate::zpp::value_error(
            vm,
            &c.name,
            if c.is_method { 1 } else { 2 },
            param_name(c, 0),
            "must be either IntlCalendar::WALLTIME_FIRST or IntlCalendar::WALLTIME_LAST",
        );
        return Value::Null;
    }
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    with_cal(&st, |cal| cal.repeated_wall = o as i32);
    Value::True
}

fn set_skipped_wall_time_option(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let o = c.long(0);
    if o != WALLTIME_FIRST as i64 && o != WALLTIME_LAST as i64 && o != WALLTIME_NEXT_VALID as i64 {
        crate::zpp::value_error(
            vm,
            &c.name,
            if c.is_method { 1 } else { 2 },
            param_name(c, 0),
            "must be one of IntlCalendar::WALLTIME_FIRST, IntlCalendar::WALLTIME_LAST, or IntlCalendar::WALLTIME_NEXT_VALID",
        );
        return Value::Null;
    }
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    with_cal(&st, |cal| cal.skipped_wall = o as i32);
    Value::True
}

fn from_date_time(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let name = c.name.clone();
    let dt = match c.arg(0) {
        Value::Object(o) => o,
        v => {
            let sv = s(v.to_php_string().as_bytes());
            match datecore::construct_native(vm, b"datetime", &[sv]) {
                Ok(o) => o,
                Err(()) => return Value::Null,
            }
        }
    };
    let had = vm.current_exception.is_some();
    let ts = datecore::call_method(vm, &dt, "getTimestamp", &[]);
    let ts = match ts {
        Some(Value::Long(n)) => n,
        None => {
            if !had {
                vm.current_exception = None;
            }
            error::set_global(
                vm,
                &name,
                U_ILLEGAL_ARGUMENT_ERROR,
                "DateTime object is unconstructed",
            );
            return Value::Null;
        }
        Some(_) => {
            error::set_global(
                vm,
                &name,
                U_ILLEGAL_ARGUMENT_ERROR,
                "bad DateTime; call to DateTime::getTimestamp() failed",
            );
            return Value::Null;
        }
    };
    let Ok(z) = datecore::datetime_zone(vm, &dt, &name) else {
        return Value::Null;
    };
    let locale = match c.opt_str(1) {
        Some(l) => String::from_utf8_lossy(&l).into_owned(),
        None => crate::common::default_locale(vm),
    };
    let mut cal = datecore::create_calendar(z, &locale);
    if cal.set_time(ts as f64 * 1000.0).is_err() {
        error::set_global(
            vm,
            &name,
            U_ILLEGAL_ARGUMENT_ERROR,
            "error creating ICU Calendar::setTime()",
        );
        return Value::Null;
    }
    obj_value(datecore::new_cal_object_from(vm, cal))
}

fn to_date_time(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let t = match with_cal(&st, |cal| cal.get_time()) {
        Ok(t) => t / 1000.0,
        Err(()) => return fail(vm, c, &st, "Call to ICU method has failed"),
    };
    if t > i64::MAX as f64 || t < i64::MIN as f64 {
        return fail(
            vm,
            c,
            &st,
            "The calendar date is out of the range for a 64-bit integer",
        );
    }
    let ts = t as i64;
    let z = with_cal(&st, |cal| cal.zone.clone());
    let name = c.name.clone();
    let mut err = std::mem::take(&mut st.borrow_mut().err);
    let tzv = datecore::zone_to_datetimezone(vm, &z, &name, Some(&mut err));
    st.borrow_mut().err = err;
    let Ok(tzv) = tzv else {
        return Value::False;
    };
    let tsv = s(format!("@{}", ts));
    let Ok(dt) = datecore::construct_native(vm, b"datetime", &[tsv, tzv.clone()]) else {
        return Value::False;
    };
    match datecore::call_method(vm, &dt, "setTimezone", &[tzv]) {
        Some(Value::False) | None => {
            if vm.current_exception.is_none() {
                return fail(vm, c, &st, "call to DateTime::setTimeZone has failed");
            }
            Value::False
        }
        _ => Value::Object(dt),
    }
}

fn get_error_code(_vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Some(st) = c.this.as_ref().and_then(state) else {
        return Value::False;
    };
    Value::Long(st.borrow().err.code as i64)
}

fn get_error_message(_vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Some(st) = c.this.as_ref().and_then(state) else {
        return Value::False;
    };
    s(st.borrow().err.message())
}

// ---------------------------------------------------------------------------
// IntlGregorianCalendar

fn set_gregorian_change(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let d = c.double(0);
    match with_cal(&st, |cal| cal.set_gregorian_change(d)) {
        Ok(()) => Value::True,
        Err(()) => fail(vm, c, &st, "error calling ICU method"),
    }
}

fn get_gregorian_change(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    Value::Double(with_cal(&st, |cal| cal.cutover))
}

fn is_leap_year(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let y = c.long(0);
    if !in_i32(y) {
        out_of_bound(vm, &c.name, if c.is_method { 1 } else { 2 }, param_name(c, 0));
        return Value::Null;
    }
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    bool_v(with_cal(&st, |cal| cal.is_leap_year(y as i32)))
}

/// A GregorianCalendar(y, m, d[, h, mi[, s]]) in PHP's default zone.
fn gregorian_from_fields(vm: &mut Vm, v: &[i32]) -> Calendar {
    let tz = datecore::php_default_tz(vm);
    // ICU's default locale (Locale::getDefault()), not intl.default_locale
    let locale = crate::common::icu_default_locale();
    let mut cal = Calendar::new(
        CalType::Gregorian,
        zone::create_or_unknown(&tz),
        &locale,
        datecore::week_data(&locale),
    );
    cal.set(datecore::ERA, 1);
    cal.set_ymd(v[0], v[1], v[2]);
    if v.len() >= 5 {
        cal.set(datecore::HOUR_OF_DAY, v[3]);
        cal.set(datecore::MINUTE, v[4]);
    }
    if v.len() >= 6 {
        cal.set(datecore::SECOND, v[5]);
    }
    cal
}

fn gregcal_create_instance_wrapper(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.emit_deprecated(
        "Function intlgregcal_create_instance() is deprecated since 8.4, use IntlGregorianCalendar::__construct(), IntlGregorianCalendar::createFromDate(), or IntlGregorianCalendar::createFromDateTime() instead",
    );
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    let mut given = args.len();
    while given > 0 && matches!(args[given - 1], Value::Undef) {
        given -= 1;
    }
    let mut call = Call {
        name: "intlgregcal_create_instance".to_string(),
        this: None,
        a: crate::zpp::Args {
            v: args.iter().map(|a| a.deref()).collect(),
            rest: Vec::new(),
            given,
        },
        called: Vec::new(),
        is_method: false,
        first: None,
    };
    Ok(gregcal_create_instance(vm, &mut call))
}

fn gregcal_create_instance(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(o) = crate::obj::object_init_ex(vm, b"intlgregoriancalendar") else {
        return Value::Null;
    };
    if gregcal_body(vm, c, &o, false) {
        Value::Object(o)
    } else {
        Value::Null
    }
}

fn gregcal_construct(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(o) = c.this.clone() else {
        return Value::Null;
    };
    let saved_exc = vm.constants.get(b"intl.use_exceptions".as_slice()).cloned();
    let saved_lvl = vm.constants.get(b"intl.error_level".as_slice()).cloned();
    vm.constants
        .insert(b"intl.use_exceptions".to_vec(), s("1"));
    vm.constants.insert(b"intl.error_level".to_vec(), s("0"));
    gregcal_body(vm, c, &o, true);
    if let Some(v) = saved_exc {
        vm.constants.insert(b"intl.use_exceptions".to_vec(), v);
    }
    if let Some(v) = saved_lvl {
        vm.constants.insert(b"intl.error_level".to_vec(), v);
    }
    Value::Null
}

/// _php_intlgregcal_constructor_body(): true when the calendar was set.
fn gregcal_body(vm: &mut Vm, c: &mut Call, o: &Obj, is_ctor: bool) -> bool {
    error::reset_global();
    let n = c.a.given;
    if is_ctor && n > 2 {
        vm.emit_deprecated(
            "Calling IntlGregorianCalendar::__construct() with more than 2 arguments is deprecated, use either IntlGregorianCalendar::createFromDate() or IntlGregorianCalendar::createFromDateTime() instead",
        );
        if vm.current_exception.is_some() {
            return false;
        }
    }
    if n > 6 {
        crate::zpp::throw(vm, b"ArgumentCountError", "Too many arguments");
        return false;
    }
    let mut variant = n;
    while variant > 0 && matches!(c.arg(variant - 1), Value::Null) {
        variant -= 1;
    }
    if variant == 4 {
        crate::zpp::throw(
            vm,
            b"ArgumentCountError",
            "No variant with 4 arguments (excluding trailing NULLs)",
        );
        return false;
    }
    let mut largs = [0i64; 6];
    if variant > 2 {
        // zend_parse_parameters("lll|lll")
        let pnames = ["timezoneOrYear", "localeOrMonth", "day", "hour", "minute", "second"];
        let strict = vm.caller_strict_types;
        let line = vm.current_line;
        for (i, slot) in largs.iter_mut().enumerate().take(n) {
            let v = c.arg(i);
            match crate::zpp::check_arg(vm, &c.name, i, pnames[i], "int", &v, strict, line) {
                Some(x) => *slot = x.to_long(),
                None => return false,
            }
        }
    } else {
        // Z_PARAM_OBJ_OR_STR_OR_NULL, Z_PARAM_STRING_OR_NULL
        let strict = vm.caller_strict_types;
        let line = vm.current_line;
        if n >= 1 {
            let v = c.arg(0);
            if !matches!(v, Value::Null | Value::Object(_)) {
                let Some(x) = crate::zpp::check_arg(
                    vm,
                    &c.name,
                    0,
                    "timezoneOrYear",
                    "?string",
                    &v,
                    strict,
                    line,
                ) else {
                    return false;
                };
                c.a.v[0] = x;
            }
        }
        if n >= 2 {
            let v = c.arg(1);
            if !matches!(v, Value::Null) {
                let Some(x) = crate::zpp::check_arg(
                    vm,
                    &c.name,
                    1,
                    "localeOrMonth",
                    "?string",
                    &v,
                    strict,
                    line,
                ) else {
                    return false;
                };
                c.a.v[1] = x;
            }
        }
    }
    let Some(st) = state(o) else {
        return false;
    };
    if st.borrow().cal.is_some() {
        crate::zpp::throw(
            vm,
            b"Error",
            "IntlGregorianCalendar object is already constructed",
        );
        return false;
    }
    let cal = if variant <= 2 {
        let name = c.name.clone();
        let tzv = if n >= 1 { c.arg(0) } else { Value::Null };
        let z = match datecore::zone_from_arg(vm, &tzv, &name, None) {
            Ok(z) => z,
            Err(()) => {
                if vm.current_exception.is_none() {
                    let e = vm.create_exception(b"IntlException", "Constructor failed", vm.current_line);
                    vm.current_exception = Some(e);
                }
                return false;
            }
        };
        let locale = match if n >= 2 { c.opt_str(1) } else { None } {
            Some(l) => String::from_utf8_lossy(&l).into_owned(),
            None => crate::common::default_locale(vm),
        };
        let mut cal = Calendar::new(CalType::Gregorian, z, &locale, datecore::week_data(&locale));
        let _ = cal.set_time(zone::now_millis());
        cal
    } else {
        for (i, &v) in largs.iter().enumerate().take(variant) {
            if !in_i32(v) {
                let pos = if c.this.is_some() && is_ctor { i as isize - 1 } else { i as isize };
                let m = format!(
                    "{}(): Argument #{} (${}) must be between -2147483648 and 2147483647",
                    c.name,
                    pos,
                    ["timezoneOrYear", "localeOrMonth", "day", "hour", "minute", "second"][i]
                );
                crate::zpp::throw(vm, b"ValueError", &m);
                return false;
            }
        }
        let v: Vec<i32> = largs[..variant].iter().map(|&x| x as i32).collect();
        gregorian_from_fields(vm, &v)
    };
    st.borrow_mut().cal = Some(cal);
    true
}

fn create_from_date(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    for i in 0..3 {
        if !in_i32(c.long(i)) {
            out_of_bound(vm, &c.name, i + 1, param_name(c, i));
            return Value::Null;
        }
    }
    let v: Vec<i32> = (0..3).map(|i| c.long(i) as i32).collect();
    let cal = gregorian_from_fields(vm, &v);
    obj_value(datecore::new_cal_object_from(vm, cal))
}

fn create_from_date_time(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    for i in 0..5 {
        if !in_i32(c.long(i)) {
            out_of_bound(vm, &c.name, i + 1, param_name(c, i));
            return Value::Null;
        }
    }
    let mut v: Vec<i32> = (0..5).map(|i| c.long(i) as i32).collect();
    if let Some(sv) = c.opt_long(5) {
        if !in_i32(sv) {
            out_of_bound(vm, &c.name, 6, param_name(c, 5));
            return Value::Null;
        }
        v.push(sv as i32);
    }
    let cal = gregorian_from_fields(vm, &v);
    obj_value(datecore::new_cal_object_from(vm, cal))
}
