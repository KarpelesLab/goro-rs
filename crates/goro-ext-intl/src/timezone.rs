//! ext/intl/timezone: IntlTimeZone and the intltz_*() functions.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};

use crate::datecore::{self, TzState, Zone, data, zone};
use crate::error::{self, U_ILLEGAL_ARGUMENT_ERROR, U_INVALID_CHAR_FOUND};
use crate::{Call, Obj, bool_v, s};

crate::intl_functions! { pub FUNCTIONS:
    "intltz_create_time_zone" => create_time_zone,
    "intltz_from_date_time_zone" => from_date_time_zone,
    "intltz_create_default" => create_default,
    "intltz_get_gmt" => get_gmt,
    "intltz_get_unknown" => get_unknown,
    "intltz_create_enumeration" => create_enumeration,
    "intltz_count_equivalent_ids" => count_equivalent_ids,
    "intltz_create_time_zone_id_enumeration" => create_time_zone_id_enumeration,
    "intltz_get_canonical_id" => get_canonical_id,
    "intltz_get_region" => get_region,
    "intltz_get_tz_data_version" => get_tz_data_version,
    "intltz_get_equivalent_id" => get_equivalent_id,
    "intltz_get_iana_id" => get_iana_id,
    "intltz_get_id" => get_id,
    "intltz_use_daylight_time" => use_daylight_time,
    "intltz_get_offset" => get_offset,
    "intltz_get_raw_offset" => get_raw_offset,
    "intltz_has_same_rules" => has_same_rules,
    "intltz_get_display_name" => get_display_name,
    "intltz_get_dst_savings" => get_dst_savings,
    "intltz_to_date_time_zone" => to_date_time_zone,
    "intltz_get_error_code" => get_error_code,
    "intltz_get_error_message" => get_error_message,
    "intltz_get_windows_id" => get_windows_id,
    "intltz_get_id_for_windows_id" => get_id_for_windows_id,
}

pub const METHODS: &[(&str, crate::Imp)] = &[
    ("IntlTimeZone::__construct", construct),
    // IntlIterator over a string enumeration (see string_enum_iterator())
    ("IntlIterator::current", it_current),
    ("IntlIterator::key", it_key),
    ("IntlIterator::next", it_next),
    ("IntlIterator::rewind", it_rewind),
    ("IntlIterator::valid", it_valid),
];

pub const CLASSES: &[(&str, Option<crate::obj::CreateFn>)] = &[
    ("IntlTimeZone", Some(create_object)),
    ("IntlIterator", None),
];

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    get_properties: Some(debug_info),
    compare: Some(compare),
    clone_obj: Some(clone_obj),
    ..NativeObjectHandlers::EMPTY
};

fn create_object(_vm: &mut Vm, o: &Obj) {
    crate::obj::attach(o, TzState::default(), &HANDLERS);
}

fn state(o: &Obj) -> Option<Rc<RefCell<TzState>>> {
    crate::obj::state::<TzState>(o)
}

/// TimeZone_get_debug_info
fn debug_info(_vm: &mut Vm, o: &Obj, purpose: u8) -> Option<PhpArray> {
    if purpose != 0 {
        return None;
    }
    let mut a = PhpArray::new();
    let z = state(o).and_then(|s| s.borrow().zone.clone());
    let key = |k: &str| ArrayKey::String(PhpString::from_bytes(k.as_bytes()));
    match z {
        None => a.set(key("valid"), Value::False),
        Some(z) => {
            a.set(key("valid"), Value::True);
            a.set(key("id"), s(&z.id));
            let (r, d) = z.offsets_utc(zone::now_millis());
            a.set(key("rawOffset"), Value::Long(r as i64));
            a.set(key("currentOffset"), Value::Long((r + d) as i64));
        }
    }
    Some(a)
}

/// TimeZone_compare_objects
fn compare(a: &Value, b: &Value) -> Option<i64> {
    let (Value::Object(x), Value::Object(y)) = (a, b) else {
        return None;
    };
    let zx = state(x)?.borrow().zone.clone();
    let zy = state(y)?.borrow().zone.clone();
    match (zx, zy) {
        (Some(p), Some(q)) => Some(if p.equals(&q) { 0 } else { 1 }),
        _ => Some(1),
    }
}

/// The exception TimeZone_compare_objects throws for an unconstructed
/// operand (called before the comparison).
pub fn compare_check(vm: &mut Vm, a: &Value, b: &Value) {
    let (Value::Object(x), Value::Object(y)) = (a, b) else {
        return;
    };
    let (Some(sx), Some(sy)) = (state(x), state(y)) else {
        return;
    };
    if sx.borrow().zone.is_none() || sy.borrow().zone.is_none() {
        crate::zpp::throw(
            vm,
            b"Exception",
            "Comparison with at least one unconstructed IntlTimeZone operand",
        );
    }
}

/// TimeZone_clone_obj
fn clone_obj(vm: &mut Vm, src: &Obj, dst: &Obj) {
    let z = state(src).and_then(|s| s.borrow().zone.clone());
    crate::obj::attach(
        dst,
        TzState {
            zone: z.clone(),
            err: Default::default(),
        },
        &HANDLERS,
    );
    if z.is_none() {
        crate::zpp::throw(vm, b"Error", "Cannot clone uninitialized IntlTimeZone");
    }
}

/// IntlTimeZone::__construct()
fn construct(vm: &mut Vm, _c: &mut Call) -> Value {
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

/// The id argument as UTF-8, or the "could not convert" error.
fn id_arg(vm: &mut Vm, c: &Call, i: usize) -> Option<String> {
    let b = c.str(i);
    match String::from_utf8(b) {
        Ok(s) => Some(s),
        Err(_) => {
            error::set_global(
                vm,
                &c.name,
                U_INVALID_CHAR_FOUND,
                "could not convert time zone id to UTF-16",
            );
            None
        }
    }
}

fn create_time_zone(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Some(id) = id_arg(vm, c, 0) else {
        return Value::Null;
    };
    obj_value(datecore::new_tz_object_zone(vm, zone::create_or_unknown(&id)))
}

fn from_date_time_zone(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Value::Object(o) = c.arg(0) else {
        return Value::Null;
    };
    let had = vm.current_exception.is_some();
    let name = datecore::call_method(vm, &o, "getName", &[]);
    if name.is_none() {
        if !had {
            vm.current_exception = None;
        }
        crate::zpp::throw(vm, b"Error", "DateTimeZone object is unconstructed");
        return Value::Null;
    }
    match datecore::datetimezone_to_zone(vm, &o, &c.name, None) {
        Ok(z) => obj_value(datecore::new_tz_object_zone(vm, z)),
        Err(()) => Value::Null,
    }
}

fn create_default(vm: &mut Vm, _c: &mut Call) -> Value {
    error::reset_global();
    let id = datecore::php_default_tz(vm);
    obj_value(datecore::new_tz_object_zone(vm, zone::create_or_unknown(&id)))
}

fn get_gmt(vm: &mut Vm, _c: &mut Call) -> Value {
    error::reset_global();
    obj_value(datecore::new_tz_object_zone(vm, zone::gmt()))
}

fn get_unknown(vm: &mut Vm, _c: &mut Call) -> Value {
    error::reset_global();
    obj_value(datecore::new_tz_object_zone(vm, zone::unknown()))
}

/// The current raw offset of system zone `id`.
fn raw_offset_of(id: &str) -> i32 {
    zone::create_or_unknown(id).raw_offset()
}

fn create_enumeration(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let ids: Vec<&'static str> = match c.arg(0) {
        Value::Null | Value::Undef => data::ZONES.iter().map(|z| z.0).collect(),
        Value::Long(n) => {
            if n < i32::MIN as i64 || n > i32::MAX as i64 {
                crate::zpp::value_error(
                    vm,
                    &c.name,
                    1,
                    "countryOrRawOffset",
                    "must be between -2147483648 and 2147483647",
                );
                return Value::Null;
            }
            data::ZONES
                .iter()
                .filter(|z| raw_offset_of(z.0) as i64 == n)
                .map(|z| z.0)
                .collect()
        }
        v => {
            let country = v.to_php_string().to_string_lossy();
            data::ZONES
                .iter()
                .filter(|z| z.2 == country)
                .map(|z| z.0)
                .collect()
        }
    };
    string_enum_iterator(vm, ids.into_iter().map(|s| s.to_string()).collect())
}

fn count_equivalent_ids(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Some(id) = id_arg(vm, c, 0) else {
        return Value::False;
    };
    let n = match zone::system_index(&id) {
        Some(i) => data::EQUIV_GROUPS[data::ZONES[i].5 as usize].len(),
        None => 0,
    };
    Value::Long(n as i64)
}

fn create_time_zone_id_enumeration(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let ty = c.long(0);
    if !(0..=2).contains(&ty) {
        crate::zpp::value_error(
            vm,
            &c.name,
            1,
            "type",
            "must be one of IntlTimeZone::TYPE_ANY, IntlTimeZone::TYPE_CANONICAL, or IntlTimeZone::TYPE_CANONICAL_LOCATION",
        );
        return Value::Null;
    }
    let region = c.opt_str(1).map(|r| String::from_utf8_lossy(&r).into_owned());
    let offset = c.opt_long(2);
    if let Some(o) = offset {
        if o < i32::MIN as i64 || o > i32::MAX as i64 {
            crate::zpp::value_error(
                vm,
                &c.name,
                3,
                "rawOffset",
                "must be between -2147483648 and 2147483647",
            );
            return Value::Null;
        }
    }
    let ids: Vec<String> = data::ZONES
        .iter()
        .filter(|z| match ty {
            1 => z.4 & 1 != 0,
            2 => z.4 & 2 != 0,
            _ => true,
        })
        .filter(|z| region.as_ref().is_none_or(|r| z.2 == r))
        .filter(|z| offset.is_none_or(|o| raw_offset_of(z.0) as i64 == o))
        .map(|z| z.0.to_string())
        .collect();
    string_enum_iterator(vm, ids)
}

fn get_canonical_id(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Some(id) = id_arg(vm, c, 0) else {
        return Value::False;
    };
    match zone::canonical_id(&id) {
        Some((canon, sys)) => {
            if c.passed(1) {
                c.set_ref(vm, 1, bool_v(sys));
            }
            s(canon)
        }
        None => {
            error::set_global(
                vm,
                &c.name,
                U_ILLEGAL_ARGUMENT_ERROR,
                "error obtaining canonical ID",
            );
            Value::False
        }
    }
}

fn get_region(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Some(id) = id_arg(vm, c, 0) else {
        return Value::False;
    };
    match zone::system_index(&id) {
        Some(i) if !data::ZONES[i].2.is_empty() => s(data::ZONES[i].2),
        _ => {
            error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, "error obtaining region");
            Value::False
        }
    }
}

fn get_tz_data_version(_vm: &mut Vm, _c: &mut Call) -> Value {
    error::reset_global();
    s("2025b")
}

fn get_equivalent_id(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let idx = c.long(1);
    if idx < i32::MIN as i64 || idx > i32::MAX as i64 {
        crate::zpp::value_error(
            vm,
            &c.name,
            2,
            "offset",
            "must be between -2147483648 and 2147483647",
        );
        return Value::Null;
    }
    let Some(id) = id_arg(vm, c, 0) else {
        return Value::False;
    };
    let r = zone::system_index(&id)
        .and_then(|i| {
            let g = data::EQUIV_GROUPS[data::ZONES[i].5 as usize];
            if idx >= 0 {
                g.get(idx as usize).copied()
            } else {
                None
            }
        })
        .map(|zi| data::ZONES[zi as usize].0)
        .unwrap_or("");
    s(r)
}

fn get_iana_id(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let Some(id) = id_arg(vm, c, 0) else {
        return Value::False;
    };
    match zone::system_index(&id) {
        Some(i) if data::ZONES[i].4 & 4 == 0 => s(zone::iana_of(zone::canonical_of(i))),
        _ => {
            error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, "error obtaining IANA ID");
            Value::False
        }
    }
}

/// TIMEZONE_METHOD_FETCH_OBJECT: the object's state (its error reset) and
/// zone; throws on an unconstructed object.
fn fetch(vm: &mut Vm, c: &Call) -> Option<(Rc<RefCell<TzState>>, Rc<Zone>)> {
    error::reset_global();
    let o = c.this.clone()?;
    let st = state(&o)?;
    st.borrow_mut().err.reset();
    let z = st.borrow().zone.clone();
    match z {
        Some(z) => Some((st, z)),
        None => {
            crate::zpp::throw(vm, b"Error", "Found unconstructed IntlTimeZone");
            None
        }
    }
}

fn get_id(vm: &mut Vm, c: &mut Call) -> Value {
    let Some((_, z)) = fetch(vm, c) else {
        return Value::Null;
    };
    s(&z.id)
}

fn use_daylight_time(vm: &mut Vm, c: &mut Call) -> Value {
    let Some((_, z)) = fetch(vm, c) else {
        return Value::Null;
    };
    bool_v(z.use_daylight_time())
}

fn get_offset(vm: &mut Vm, c: &mut Call) -> Value {
    let Some((st, z)) = fetch(vm, c) else {
        return Value::Null;
    };
    let date = c.double(0);
    let local = c.bool(1);
    let r = if local {
        if !date.is_finite() || (date / 86_400_000.0).abs() > 2_147_483_647.0 {
            Err(())
        } else {
            Ok(z.offsets_wall(date))
        }
    } else {
        z.offsets(date)
    };
    match r {
        Ok((raw, dst)) => {
            if !c.set_ref(vm, 2, Value::Long(raw as i64)) {
                return Value::Null;
            }
            if !c.set_ref(vm, 3, Value::Long(dst as i64)) {
                return Value::Null;
            }
            Value::True
        }
        Err(()) => {
            let name = c.name.clone();
            error::set_both(
                vm,
                &name,
                &mut st.borrow_mut().err,
                U_ILLEGAL_ARGUMENT_ERROR,
                "error obtaining offset",
            );
            Value::False
        }
    }
}

fn get_raw_offset(vm: &mut Vm, c: &mut Call) -> Value {
    let Some((_, z)) = fetch(vm, c) else {
        return Value::Null;
    };
    Value::Long(z.raw_offset() as i64)
}

fn has_same_rules(vm: &mut Vm, c: &mut Call) -> Value {
    let Some((st, z)) = fetch(vm, c) else {
        return Value::Null;
    };
    let Value::Object(other) = c.arg(0) else {
        return Value::Null;
    };
    match datecore::tz_object_zone(&other) {
        Some(oz) => bool_v(z.has_same_rules(&oz)),
        None => {
            let name = c.name.clone();
            error::set_both(
                vm,
                &name,
                &mut st.borrow_mut().err,
                U_ILLEGAL_ARGUMENT_ERROR,
                "The second IntlTimeZone is unconstructed",
            );
            Value::False
        }
    }
}

fn get_display_name(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let daylight = c.bool(0);
    let style = if c.passed(1) { c.long(1) } else { 2 };
    if !(1..=8).contains(&style) {
        error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, "wrong display type");
        return Value::False;
    }
    let locale = match c.opt_str(2) {
        Some(l) => String::from_utf8_lossy(&l).into_owned(),
        None => crate::common::default_locale(vm),
    };
    let Some((_, z)) = fetch(vm, c) else {
        return Value::Null;
    };
    s(datecore::names::display_name(&z, daylight, style, &locale))
}

fn get_dst_savings(vm: &mut Vm, c: &mut Call) -> Value {
    let Some((_, z)) = fetch(vm, c) else {
        return Value::Null;
    };
    Value::Long(z.dst_savings() as i64)
}

fn to_date_time_zone(vm: &mut Vm, c: &mut Call) -> Value {
    let Some((st, z)) = fetch(vm, c) else {
        return Value::Null;
    };
    let name = c.name.clone();
    let mut err = std::mem::take(&mut st.borrow_mut().err);
    let r = datecore::zone_to_datetimezone(vm, &z, &name, Some(&mut err));
    st.borrow_mut().err = err;
    match r {
        Ok(v) => v,
        Err(()) => Value::False,
    }
}

fn get_error_code(_vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = c.this.as_ref().and_then(state) else {
        return Value::False;
    };
    Value::Long(st.borrow().err.code as i64)
}

fn get_error_message(_vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = c.this.as_ref().and_then(state) else {
        return Value::False;
    };
    s(st.borrow().err.message())
}

fn get_windows_id(vm: &mut Vm, c: &mut Call) -> Value {
    let b = c.str(0);
    let Ok(id) = String::from_utf8(b) else {
        error::set_global(
            vm,
            &c.name,
            U_INVALID_CHAR_FOUND,
            "could not convert time zone id to UTF-16",
        );
        return Value::False;
    };
    error::set_global_code(0);
    match zone::system_index(&id) {
        Some(i) if !data::ZONES[i].3.is_empty() => s(data::ZONES[i].3),
        _ => {
            error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, "unknown system timezone");
            Value::False
        }
    }
}

fn get_id_for_windows_id(vm: &mut Vm, c: &mut Call) -> Value {
    let b = c.str(0);
    let Ok(win) = String::from_utf8(b) else {
        error::set_global(
            vm,
            &c.name,
            U_INVALID_CHAR_FOUND,
            "could not convert time zone id to UTF-16",
        );
        return Value::False;
    };
    error::set_global_code(0);
    let region = c.opt_str(1).map(|r| String::from_utf8_lossy(&r).into_owned());
    let found = data::WINDOWS.iter().find(|w| w.0 == win).map(|w| {
        region
            .as_ref()
            .and_then(|r| w.2.iter().find(|(rr, _)| rr == r).map(|(_, id)| *id))
            .unwrap_or(w.1)
    });
    match found {
        Some(id) if !id.is_empty() => s(id),
        _ => {
            error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, "unknown windows timezone");
            Value::False
        }
    }
}

// ---------------------------------------------------------------------------
// IntlIterator over a StringEnumeration (IntlIterator_from_StringEnumeration)

/// The state of an IntlIterator over a list of strings.
pub struct StringEnum {
    pub items: Vec<Value>,
    pub pos: usize,
}

static ITER_HANDLERS: NativeObjectHandlers = NativeObjectHandlers::EMPTY;

/// IntlIterator_from_StringEnumeration(): a new IntlIterator object over
/// `items`.
pub fn string_enum_iterator(vm: &mut Vm, items: Vec<String>) -> Value {
    let Some(o) = crate::obj::object_init_ex(vm, b"intliterator") else {
        return Value::Null;
    };
    crate::obj::attach(
        &o,
        StringEnum {
            items: items.into_iter().map(s).collect(),
            pos: 0,
        },
        &ITER_HANDLERS,
    );
    Value::Object(o)
}

fn it_state(c: &Call) -> Option<Rc<RefCell<StringEnum>>> {
    crate::obj::state::<StringEnum>(c.this.as_ref()?)
}

fn it_current(_vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = it_state(c) else {
        return Value::Null;
    };
    let b = st.borrow();
    b.items.get(b.pos).cloned().unwrap_or(Value::Null)
}

fn it_key(_vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = it_state(c) else {
        return Value::Null;
    };
    Value::Long(st.borrow().pos as i64)
}

fn it_next(_vm: &mut Vm, c: &mut Call) -> Value {
    if let Some(st) = it_state(c) {
        st.borrow_mut().pos += 1;
    }
    Value::Null
}

fn it_rewind(_vm: &mut Vm, c: &mut Call) -> Value {
    if let Some(st) = it_state(c) {
        st.borrow_mut().pos = 0;
    }
    Value::Null
}

fn it_valid(_vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = it_state(c) else {
        return Value::False;
    };
    let b = st.borrow();
    bool_v(b.pos < b.items.len())
}
