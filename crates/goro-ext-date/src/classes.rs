//! DateTime, DateTimeImmutable, DateTimeZone, DateInterval and DatePeriod.

use crate::format::*;
use crate::funcs::*;
use crate::state::*;
use crate::timelib::interval::{add, add_wall, diff, sub, sub_wall};
use crate::timelib::iso_interval::strtointerval;
use crate::timelib::parse_date::{Ptr, fill_holes, parse_from_format, parse_zone, strtotime};
use crate::timelib::tm2unixtime::update_ts;
use crate::timelib::tz::parse_tzfile;
use crate::timelib::unixtime2tm::{unixtime2date, unixtime2gmt, unixtime2local, update_from_sse};
use crate::timelib::*;
use crate::zpp::*;
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

type Obj = Rc<RefCell<PhpObject>>;

const INIT_CTOR: u32 = 1;
const INIT_FORMAT: u32 = 2;

// ---------------------------------------------------------------------------
// Helpers

pub fn uninitialized_error(vm: &mut Vm, class_name: &[u8]) -> VmError {
    let lower = class_name.to_ascii_lowercase();
    let display = class_display(vm, class_name);
    let msg = if Base::from_lower(&lower).is_some() || lower == b"datetimeinterface" {
        format!(
            "Object of type {} has not been correctly initialized by calling parent::__construct() in its constructor",
            display
        )
    } else {
        match base_of(vm, class_name) {
            Some(b) => format!(
                "Object of type {} (inheriting {}) has not been correctly initialized by calling parent::__construct() in its constructor",
                display,
                b.name()
            ),
            None => format!(
                "Object of type {} not been correctly initialized by calling parent::__construct() in its constructor",
                display
            ),
        }
    };
    throw(vm, b"DateObjectError", &msg)
}

fn class_of(o: &Obj) -> Vec<u8> {
    o.borrow().class_name.clone()
}

/// Fetch an initialized DateTime state or throw.
fn need_date(vm: &mut Vm, o: &Obj) -> R<Time> {
    match date_get(o) {
        Some(t) => Ok(t),
        None => Err(uninitialized_error(vm, &class_of(o))),
    }
}

fn need_tz(vm: &mut Vm, o: &Obj) -> R<TzObj> {
    match tz_get(o) {
        Some(t) => Ok(t),
        None => Err(uninitialized_error(vm, &class_of(o))),
    }
}

fn need_interval(vm: &mut Vm, o: &Obj) -> R<IntervalObj> {
    match interval_get(o) {
        Some(t) => Ok(t),
        None => Err(uninitialized_error(vm, &class_of(o))),
    }
}

fn obj_arg(
    vm: &mut Vm,
    fname: &str,
    args: &[Value],
    idx: usize,
    pname: &str,
    tname: &str,
    ok: &[Base],
    nullable: bool,
) -> R<Option<Obj>> {
    let v = match args.get(idx) {
        None | Some(Value::Undef) => return Ok(None),
        Some(v) => v.deref(),
    };
    if nullable && matches!(v, Value::Null) {
        return Ok(None);
    }
    if let Value::Object(o) = &v {
        if let Some(b) = obj_base(vm, o) {
            if ok.contains(&b) {
                return Ok(Some(o.clone()));
            }
        }
    }
    let expected = if nullable {
        format!("?{}", tname)
    } else {
        tname.to_string()
    };
    Err(type_err_class(vm, fname, idx, pname, &expected, &v))
}

const DATE_BASES: &[Base] = &[Base::DateTime, Base::Immutable];

fn clone_obj(vm: &mut Vm, o: &Obj) -> Obj {
    let id = vm.alloc_object_id();
    let ob = o.borrow();
    let mut n = PhpObject::new(ob.class_name.clone(), id);
    n.properties = ob.properties.clone();
    Rc::new(RefCell::new(n))
}

/// object_init_ex() refuses abstract classes
fn check_instantiable(vm: &mut Vm, cls: &[u8]) -> R<()> {
    let abstract_name = vm
        .classes
        .get(&cls.to_ascii_lowercase())
        .filter(|c| c.is_abstract)
        .map(|c| c.name.clone());
    if let Some(n) = abstract_name {
        return Err(throw(
            vm,
            b"Error",
            &format!(
                "Cannot instantiate abstract class {}",
                goro_core::string::msg_str(&n)
            ),
        ));
    }
    Ok(())
}

fn cstr(b: &[u8]) -> String {
    let z = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..z]).into_owned()
}

fn first_error_char(e: &ErrorMessage) -> char {
    if e.character != 0 {
        e.character as char
    } else {
        ' '
    }
}

/// php_date_initialize. Returns Ok(Some(time)) on success, Ok(None) on parse
/// failure (exception already thrown when INIT_CTOR is set).
pub fn date_initialize(
    vm: &mut Vm,
    time_str: &[u8],
    format: Option<&[u8]>,
    tzobj: Option<&TzObj>,
    flags: u32,
) -> R<Option<Time>> {
    let (mut t, err) = match format {
        Some(f) => parse_from_format(f, time_str),
        None => {
            if time_str.is_empty() {
                strtotime(b"now")
            } else {
                strtotime(time_str)
            }
        }
    };
    update_errors_warnings(Some(&err));
    if flags & INIT_CTOR != 0 && err.error_count() > 0 {
        let e = &err.error_messages[0];
        let shown = if format.is_none() && time_str.is_empty() {
            "now".to_string()
        } else {
            cstr(time_str)
        };
        let msg = format!(
            "Failed to parse time string ({}) at position {} ({}): {}",
            shown,
            e.position,
            first_error_char(e),
            e.message
        );
        return Err(throw(vm, b"DateMalformedStringException", &msg));
    }
    if err.error_count() > 0 {
        return Ok(None);
    }
    let mut ztype = ZONETYPE_ID;
    let mut new_offset = 0;
    let mut new_dst = 0;
    let mut new_abbr = None;
    let tzi: Option<Rc<TzInfo>>;
    if let Some(z) = tzobj {
        tzi = z.tz.clone();
        match z.ztype {
            ZONETYPE_ID => {}
            ZONETYPE_OFFSET => new_offset = z.utc_offset,
            ZONETYPE_ABBR => {
                new_offset = z.utc_offset;
                new_dst = z.dst;
                new_abbr = Some(z.abbr.clone());
            }
            _ => {
                return Err(throw(
                    vm,
                    b"Error",
                    "The DateTimeZone object has not been correctly initialized by its constructor",
                ));
            }
        }
        ztype = z.ztype;
    } else if let Some(z) = &t.tz_info {
        tzi = Some(z.clone());
    } else {
        tzi = get_timezone_info(vm);
        if tzi.is_none() {
            return Ok(None);
        }
    }
    let mut now = Time::new();
    now.zone_type = ztype;
    match ztype {
        ZONETYPE_ID => now.tz_info = tzi.clone(),
        ZONETYPE_OFFSET => now.z = new_offset,
        ZONETYPE_ABBR => {
            now.z = new_offset;
            now.dst = new_dst;
            now.tz_abbr = new_abbr;
        }
        _ => {}
    }
    let (sec, usec) = now_with_fraction();
    unixtime2local(&mut now, sec);
    now.us = usec;
    if format.is_none() && (time_str.is_empty() || time_str == b"now") {
        return Ok(Some(now));
    }
    let mut options = NO_CLONE;
    if flags & INIT_FORMAT != 0 {
        options |= OVERRIDE_TIME;
    }
    fill_holes(&mut t, &now, options);
    update_ts(&mut t, tzi.as_ref());
    update_from_sse(&mut t);
    t.have_relative = 0;
    Ok(Some(t))
}

fn initialize_from_ts_long(sec: i64, usec: i64) -> Time {
    let mut t = Time::new();
    t.zone_type = ZONETYPE_OFFSET;
    unixtime2gmt(&mut t, sec);
    update_ts(&mut t, None);
    t.us = usec;
    t
}

fn initialize_from_ts_double(vm: &mut Vm, fname: &str, ts: f64) -> R<Time> {
    let sec_dval = ts.trunc();
    let range_err = |vm: &mut Vm| {
        let msg = format!(
            "{}(): Argument #1 ($timestamp) must be a finite number between {} and {}.999999, {} given",
            fname,
            i64::MIN,
            i64::MAX,
            fmt_g(ts)
        );
        throw(vm, b"DateRangeError", &msg)
    };
    if sec_dval.is_nan()
        || !(-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&sec_dval)
    {
        return Err(range_err(vm));
    }
    let mut sec = sec_dval as i64;
    let mut usec = ((ts % 1.0) * 1000000.0).round() as i64;
    if usec.abs() == 1000000 {
        sec += if usec > 0 { 1 } else { -1 };
        usec = 0;
    }
    if usec < 0 {
        if sec == i64::MIN {
            return Err(range_err(vm));
        }
        sec -= 1;
        usec += 1000000;
    }
    Ok(initialize_from_ts_long(sec, usec))
}

/// C's "%g" formatting
pub fn fmt_g(v: f64) -> String {
    if v.is_nan() {
        return "NAN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "INF".into() } else { "-INF".into() };
    }
    if v == 0.0 {
        return if v.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
    }
    let exp = v.abs().log10().floor() as i32;
    let p = 6;
    if exp < -4 || exp >= p {
        let s = format!("{:.*e}", (p - 1) as usize, v);
        // trim mantissa zeros
        let (m, e) = s.split_once('e').unwrap();
        let m = if m.contains('.') {
            m.trim_end_matches('0').trim_end_matches('.')
        } else {
            m
        };
        let ev: i32 = e.parse().unwrap();
        format!("{}e{}{:02}", m, if ev < 0 { '-' } else { '+' }, ev.abs())
    } else {
        let decimals = (p - 1 - exp).max(0) as usize;
        let s = format!("{:.*}", decimals, v);
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        }
    }
}

/// php_date_modify
fn date_modify(vm: &mut Vm, t: &mut Time, modify: &[u8], fname: &str, throw_exc: bool) -> R<bool> {
    let (tmp, err) = strtotime(modify);
    update_errors_warnings(Some(&err));
    if err.error_count() > 0 {
        let e = &err.error_messages[0];
        let msg = format!(
            "{}(): Failed to parse time string ({}) at position {} ({}): {}",
            fname,
            cstr(modify),
            e.position,
            first_error_char(e),
            e.message
        );
        if throw_exc {
            return Err(throw(vm, b"DateMalformedStringException", &msg));
        }
        vm.emit_warning(&msg);
        return Ok(false);
    }
    t.relative = tmp.relative.clone();
    t.have_relative = tmp.have_relative;
    t.sse_uptodate = 0;
    if tmp.y != UNSET {
        t.y = tmp.y;
    }
    if tmp.m != UNSET {
        t.m = tmp.m;
    }
    if tmp.d != UNSET {
        t.d = tmp.d;
    }
    if tmp.h != UNSET {
        t.h = tmp.h;
        if tmp.i != UNSET {
            t.i = tmp.i;
            if tmp.s != UNSET {
                t.s = tmp.s;
            } else {
                t.s = 0;
            }
        } else {
            t.i = 0;
            t.s = 0;
        }
    }
    if tmp.us != UNSET {
        t.us = tmp.us;
    }
    if tmp.y == 1970
        && tmp.m == 1
        && tmp.d == 1
        && tmp.h == 0
        && tmp.i == 0
        && tmp.s == 0
        && tmp.us == 0
        && tmp.have_zone != 0
        && tmp.zone_type == ZONETYPE_OFFSET
        && tmp.z == 0
        && tmp.dst == 0
    {
        set_timezone_from_offset(t, 0);
    }
    update_ts(t, None);
    update_from_sse(t);
    t.have_relative = 0;
    t.relative = RelTime::default();
    Ok(true)
}

fn date_add_impl(t: &Time, iv: &IntervalObj) -> Time {
    if iv.civil_or_wall == WALL {
        add_wall(t, &iv.diff)
    } else {
        add(t, &iv.diff)
    }
}

fn date_sub_impl(
    vm: &mut Vm,
    t: &Time,
    iv: &IntervalObj,
    fname: &str,
    throw_exc: bool,
) -> R<Option<Time>> {
    if iv.diff.have_weekday_relative != 0 || iv.diff.have_special_relative != 0 {
        let msg = format!(
            "{}(): Only non-special relative time specifications are supported for subtraction",
            fname
        );
        if throw_exc {
            return Err(throw(vm, b"DateInvalidOperationException", &msg));
        }
        vm.emit_warning(&msg);
        return Ok(None);
    }
    Ok(Some(if iv.civil_or_wall == WALL {
        sub_wall(t, &iv.diff)
    } else {
        sub(t, &iv.diff)
    }))
}

fn date_timezone_set_impl(t: &mut Time, z: &TzObj) {
    match z.ztype {
        ZONETYPE_OFFSET => set_timezone_from_offset(t, z.utc_offset),
        ZONETYPE_ABBR => set_timezone_from_abbr(t, &z.abbr, z.utc_offset, z.dst),
        ZONETYPE_ID => {
            if let Some(tz) = &z.tz {
                set_timezone(t, tz);
            }
        }
        _ => {}
    }
    let sse = t.sse;
    unixtime2local(t, sse);
}

fn offset_of(t: &Time) -> i64 {
    if t.is_localtime == 0 {
        return 0;
    }
    match t.zone_type {
        ZONETYPE_ID => t
            .tz_info
            .as_ref()
            .map(|tz| tz.get_time_zone_info(t.sse).offset as i64)
            .unwrap_or(0),
        ZONETYPE_OFFSET => t.z,
        ZONETYPE_ABBR => t.z + 3600 * t.dst as i64,
        _ => 0,
    }
}

fn make_tz_value(vm: &mut Vm, t: &Time) -> Value {
    if t.is_localtime == 0 {
        return Value::False;
    }
    let o = new_object(vm, b"DateTimeZone");
    tz_set(&o, &TzObj::from_time(t));
    Value::Object(o)
}

/// timezone_initialize: Err(message) on failure
pub fn timezone_initialize(tz: &[u8]) -> Result<TzObj, String> {
    let shown = goro_core::string::msg_str(tz).into_owned();
    let mut dummy = Time::new();
    let mut ptr = Ptr::new(tz);
    let mut dst = 0;
    let mut not_found = false;
    dummy.z = parse_zone(&mut ptr, &mut dst, &mut dummy, &mut not_found);
    if dummy.z >= 100 * 60 * 60 || dummy.z <= -100 * 60 * 60 {
        return Err(format!("Timezone offset is out of range ({})", shown));
    }
    dummy.dst = dst;
    if !not_found && ptr.c() != 0 {
        return Err(format!("Unknown or bad timezone ({})", shown));
    }
    if not_found {
        return Err(format!("Unknown or bad timezone ({})", shown));
    }
    Ok(TzObj::from_time(&dummy))
}

fn tz_from_hash(a: &PhpArray) -> Option<TzObj> {
    let zt = arr_get(a, b"timezone_type")?;
    let z = arr_get(a, b"timezone")?;
    let zt = match zt {
        Value::Long(l) => *l,
        _ => return None,
    };
    if !(1..=3).contains(&zt) {
        return None;
    }
    let z = match z {
        Value::String(s) => s.as_bytes().to_vec(),
        _ => return None,
    };
    if z.contains(&0) {
        return None;
    }
    timezone_initialize(&z).ok()
}

/// php_date_initialize_from_hash
fn date_from_hash(vm: &mut Vm, a: &PhpArray) -> R<Option<Time>> {
    let date = match arr_get(a, b"date") {
        Some(Value::String(s)) => s.as_bytes().to_vec(),
        _ => return Ok(None),
    };
    let zt = match arr_get(a, b"timezone_type") {
        Some(Value::Long(l)) => *l,
        _ => return Ok(None),
    };
    let zone = match arr_get(a, b"timezone") {
        Some(Value::String(s)) => s.as_bytes().to_vec(),
        _ => return Ok(None),
    };
    match zt as u32 {
        ZONETYPE_OFFSET | ZONETYPE_ABBR => {
            let mut s = date.clone();
            s.push(b' ');
            s.extend_from_slice(&zone);
            date_initialize(vm, &s, None, None, 0)
        }
        ZONETYPE_ID => {
            let name = String::from_utf8_lossy(&zone).into_owned();
            if zone.contains(&0) {
                return Ok(None);
            }
            let tzi = match parse_tzfile(&name) {
                Some(t) => t,
                None => return Ok(None),
            };
            let z = TzObj {
                ztype: ZONETYPE_ID,
                utc_offset: 0,
                dst: 0,
                abbr: String::new(),
                tz: Some(tzi),
            };
            date_initialize(vm, &date, None, Some(&z), 0)
        }
        _ => Ok(None),
    }
}

/// Find the declared visibility of a property: (is_private, is_protected, declaring class display name)
fn prop_visibility(vm: &Vm, class_name: &[u8], prop: &[u8]) -> Option<(bool, bool, Vec<u8>)> {
    let mut cur = class_name.to_ascii_lowercase();
    for _ in 0..64 {
        let ce = vm.classes.get(&cur)?;
        if let Some(pd) = ce
            .properties
            .iter()
            .find(|p| p.name == prop && !p.is_static)
        {
            use goro_core::object::Visibility;
            let decl = vm
                .classes
                .get(&pd.declaring_class)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| ce.name.clone());
            return Some((
                pd.visibility == Visibility::Private,
                pd.visibility == Visibility::Protected,
                decl,
            ));
        }
        cur = ce.parent.clone()?.to_ascii_lowercase();
    }
    None
}

/// restore_custom_*_properties + update_property
fn restore_custom_props(vm: &mut Vm, o: &Obj, a: &PhpArray, internal: &[&[u8]]) {
    // zend_update_property() creating a public property the class does not
    // declare: "Creation of dynamic property" (unless it allows them).
    let cl = o.borrow().class_name.to_ascii_lowercase();
    let mut dynamic: Vec<Vec<u8>> = Vec::new();
    {
        let ob = o.borrow();
        for (k, v) in a.iter() {
            if let ArrayKey::String(name) = k {
                let nb = name.as_bytes();
                if matches!(v, Value::Reference(_))
                    || internal.contains(&nb)
                    || nb.first() == Some(&0)
                    || ob.has_property(nb)
                {
                    continue;
                }
                let mut declared = false;
                let mut allows = false;
                let mut cur = Some(cl.clone());
                while let Some(c) = cur.take() {
                    let Some(ce) = vm.classes.get(&c) else { break };
                    declared |= ce.properties.iter().any(|p| p.name == nb && !p.is_static);
                    allows |= ce.allow_dynamic_properties;
                    cur = ce.parent.as_ref().map(|p| p.to_ascii_lowercase());
                }
                if !declared && !allows {
                    dynamic.push(nb.to_vec());
                }
            }
        }
    }
    if !dynamic.is_empty() {
        let disp = goro_core::value::display_class_name(&o.borrow().class_name).to_string();
        for n in dynamic {
            vm.emit_deprecated(&format!(
                "Creation of dynamic property {}::${} is deprecated",
                disp,
                goro_core::string::msg_str(&n)
            ));
        }
    }
    let mut ob = o.borrow_mut();
    for (k, v) in a.iter() {
        if let ArrayKey::String(name) = k {
            if matches!(v, Value::Reference(_)) || internal.contains(&name.as_bytes()) {
                continue;
            }
            let nb = name.as_bytes();
            if nb.first() == Some(&0) {
                // Mangled (non-public) names
                let p = match nb[1..].iter().position(|&c| c == 0) {
                    Some(p) => p + 1,
                    None => continue,
                };
                let cls = &nb[1..p];
                let prop = nb[p + 1..].to_vec();
                if cls != b"*" && !vm.classes.contains_key(&cls.to_ascii_lowercase()) {
                    // update_property: unknown class -> ignored
                    continue;
                }
                ob.set_property(prop, v.clone());
            } else {
                ob.set_property(nb.to_vec(), v.clone());
            }
        }
    }
}

/// add_common_properties: user properties of the object appended to an array
/// (with mangled names for non-public declared properties).
fn add_common_props(vm: &Vm, o: &Obj, a: &mut PhpArray, internal: &[&[u8]]) {
    let ob = o.borrow();
    for (k, v) in &ob.properties {
        if k.starts_with(b"__") || internal.contains(&k.as_slice()) {
            continue;
        }
        let mangled = match prop_visibility(vm, &ob.class_name, k) {
            Some((true, _, decl)) => {
                let mut m = vec![0u8];
                m.extend_from_slice(&decl);
                m.push(0);
                m.extend_from_slice(k);
                m
            }
            Some((false, true, _)) => {
                let mut m = b"\0*\0".to_vec();
                m.extend_from_slice(k);
                m
            }
            _ => k.clone(),
        };
        let key = ArrayKey::String(PhpString::from_vec(mangled));
        if a.get(&key).is_none() {
            a.set(key, v.clone());
        }
    }
}

const DT_INTERNAL: &[&[u8]] = &[b"date", b"timezone_type", b"timezone"];
const TZ_INTERNAL: &[&[u8]] = &[b"timezone_type", b"timezone"];
const IV_INTERNAL: &[&[u8]] = &[
    b"date_string",
    b"from_string",
    b"y",
    b"m",
    b"d",
    b"h",
    b"i",
    b"s",
    b"f",
    b"invert",
    b"days",
];
const DP_INTERNAL: &[&[u8]] = &[
    b"start",
    b"current",
    b"end",
    b"interval",
    b"recurrences",
    b"include_start_date",
    b"include_end_date",
];

fn ret_obj(o: &Obj) -> Value {
    Value::Object(o.clone())
}

// ---------------------------------------------------------------------------
// Constructors

fn push_frame(vm: &mut Vm, name: String, args: &[Value], method: bool) {
    let line = vm.current_line;
    let file = vm.current_file.clone();
    vm.call_stack
        .push((name, file, line, args.to_vec(), method));
}

fn ctor_result(r: R<()>) -> Result<(), String> {
    r.map_err(|e| e.message)
}

pub fn construct_datetime(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return Ok(()),
    };
    let base = obj_base(vm, &this).unwrap_or(Base::DateTime);
    let fname = format!("{}::__construct", base.name());
    let uargs: Vec<Value> = args[1..].to_vec();
    push_frame(vm, format!("{}->__construct", base.name()), &uargs, true);
    let r = (|| -> R<()> {
        check_count(vm, &fname, &uargs, 0, 2)?;
        let time_str = str_arg(vm, &fname, &uargs, 0, "datetime", false)?.unwrap_or_default();
        let tz = obj_arg(
            vm,
            &fname,
            &uargs,
            1,
            "timezone",
            "DateTimeZone",
            &[Base::TimeZone],
            true,
        )?;
        let tzobj = tz.as_ref().map(|o| {
            tz_get(o).unwrap_or(TzObj {
                ztype: 0,
                utc_offset: 0,
                dst: 0,
                abbr: String::new(),
                tz: None,
            })
        });
        if let Some(t) = date_initialize(vm, &time_str, None, tzobj.as_ref(), INIT_CTOR)? {
            date_set(&this, &t);
        }
        Ok(())
    })();
    vm.call_stack.pop();
    ctor_result(r)
}

pub fn construct_timezone(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return Ok(()),
    };
    let uargs: Vec<Value> = args[1..].to_vec();
    push_frame(vm, "DateTimeZone->__construct".into(), &uargs, true);
    let r = (|| -> R<()> {
        let fname = "DateTimeZone::__construct";
        check_count(vm, fname, &uargs, 1, 1)?;
        let tz = path_arg(vm, fname, &uargs, 0, "timezone")?.unwrap_or_default();
        match timezone_initialize(&tz) {
            Ok(z) => {
                tz_set(&this, &z);
                Ok(())
            }
            Err(m) => Err(throw(
                vm,
                b"DateInvalidTimeZoneException",
                &format!("DateTimeZone::__construct(): {}", m),
            )),
        }
    })();
    vm.call_stack.pop();
    ctor_result(r)
}

fn date_interval_initialize(vm: &mut Vm, format: &[u8]) -> R<RelTime> {
    let res = strtointerval(format);
    let shown = cstr(format);
    if res.errors.error_count() > 0 {
        return Err(throw(
            vm,
            b"DateMalformedIntervalStringException",
            &format!("Unknown or bad format ({})", shown),
        ));
    }
    if let Some(p) = res.period {
        return Ok(p);
    }
    if let (Some(mut b), Some(mut e)) = (res.begin, res.end) {
        update_ts(&mut b, None);
        update_ts(&mut e, None);
        return Ok(diff(&b, &e));
    }
    Err(throw(
        vm,
        b"DateMalformedIntervalStringException",
        &format!("Failed to parse interval ({})", shown),
    ))
}

pub fn construct_interval(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return Ok(()),
    };
    let uargs: Vec<Value> = args[1..].to_vec();
    push_frame(vm, "DateInterval->__construct".into(), &uargs, true);
    let r = (|| -> R<()> {
        let fname = "DateInterval::__construct";
        check_count(vm, fname, &uargs, 1, 1)?;
        let spec = str_arg(vm, fname, &uargs, 0, "duration", false)?.unwrap_or_default();
        let rt = date_interval_initialize(vm, &spec)?;
        interval_set(
            &this,
            &IntervalObj {
                diff: rt,
                civil_or_wall: WALL,
                from_string: false,
                date_string: Vec::new(),
            },
        );
        Ok(())
    })();
    vm.call_stack.pop();
    ctor_result(r)
}

const MAX_RECURRENCES: i64 = i32::MAX as i64 - 8;

fn period_init_iso(
    vm: &mut Vm,
    p: &mut PeriodObj,
    base_ce: &[u8],
    isostr: &[u8],
    fname: &str,
) -> R<i64> {
    let res = strtointerval(isostr);
    if res.errors.error_count() > 0 {
        return Err(throw(
            vm,
            b"DateMalformedPeriodStringException",
            &format!("Unknown or bad format ({})", cstr(isostr)),
        ));
    }
    let recurrences = res.recurrences.unwrap_or(0);
    p.start = res.begin;
    p.end = res.end;
    p.interval = res.period;
    let shown = cstr(isostr);
    if p.start.is_none() {
        return Err(throw(
            vm,
            b"DateMalformedPeriodStringException",
            &format!(
                "{}(): ISO interval must contain a start date, \"{}\" given",
                fname, shown
            ),
        ));
    }
    if p.interval.is_none() {
        return Err(throw(
            vm,
            b"DateMalformedPeriodStringException",
            &format!(
                "{}(): ISO interval must contain an interval, \"{}\" given",
                fname, shown
            ),
        ));
    }
    if let Some(s) = p.start.as_mut() {
        update_ts(s, None);
    }
    if let Some(e) = p.end.as_mut() {
        update_ts(e, None);
    }
    p.start_ce = base_ce.to_vec();
    Ok(recurrences)
}

fn period_init_finish(
    vm: &mut Vm,
    p: &mut PeriodObj,
    options: i64,
    recurrences: i64,
    fname: &str,
) -> R<()> {
    if p.end.is_none() && !(1..=MAX_RECURRENCES).contains(&recurrences) {
        return Err(throw(
            vm,
            b"DateMalformedPeriodStringException",
            &format!(
                "{}(): Recurrence count must be greater or equal to 1 and lower than {}",
                fname,
                MAX_RECURRENCES + 1
            ),
        ));
    }
    p.include_start_date = options & 1 == 0;
    p.include_end_date = options & 2 != 0;
    let recurrences = recurrences + p.include_start_date as i64 + p.include_end_date as i64;
    if recurrences > MAX_RECURRENCES {
        return Err(throw(
            vm,
            b"DateMalformedStringException",
            &format!(
                "{}(): Recurrence count must be greater or equal to 1 and lower than {} (including options)",
                fname,
                MAX_RECURRENCES + 1
            ),
        ));
    }
    p.recurrences = recurrences;
    p.initialized = true;
    Ok(())
}

pub fn construct_period(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return Ok(()),
    };
    let uargs: Vec<Value> = args[1..].to_vec();
    push_frame(vm, "DatePeriod->__construct".into(), &uargs, true);
    let r = (|| -> R<()> {
        let fname = "DatePeriod::__construct";
        let n = uargs
            .iter()
            .rposition(|a| !matches!(a, Value::Undef))
            .map(|p| p + 1)
            .unwrap_or(0);
        let a0 = uargs.first().map(|v| v.deref());
        let a1 = uargs.get(1).map(|v| v.deref());
        let a2 = uargs.get(2).map(|v| v.deref());
        let is_date = |vm: &Vm, v: &Option<Value>| matches!(v, Some(Value::Object(o)) if is_date_interface(vm, o));
        let is_iv = |vm: &Vm, v: &Option<Value>| matches!(v, Some(Value::Object(o)) if obj_base(vm, o) == Some(Base::Interval));
        let is_long_like = |v: &Option<Value>| match v {
            Some(Value::Long(_))
            | Some(Value::Double(_))
            | Some(Value::True)
            | Some(Value::False) => true,
            Some(Value::String(s)) => numeric_str(s.as_bytes()).is_some(),
            _ => false,
        };
        let mut p = PeriodObj::default();
        let mut recurrences = 0;
        let options;
        enum Form {
            Recur,
            End,
            Iso,
        }
        let form = if (3..=4).contains(&n)
            && is_date(vm, &a0)
            && is_iv(vm, &a1)
            && is_long_like(&a2)
        {
            Form::Recur
        } else if (3..=4).contains(&n) && is_date(vm, &a0) && is_iv(vm, &a1) && is_date(vm, &a2) {
            Form::End
        } else if (1..=2).contains(&n)
            && matches!(
                a0,
                Some(Value::String(_))
                    | Some(Value::Long(_))
                    | Some(Value::Double(_))
                    | Some(Value::Null)
                    | Some(Value::True)
                    | Some(Value::False)
            )
        {
            Form::Iso
        } else {
            return Err(throw(
                vm,
                b"TypeError",
                "DatePeriod::__construct() accepts (DateTimeInterface, DateInterval, int [, int]), or (DateTimeInterface, DateInterval, DateTime [, int]), or (string [, int]) as arguments",
            ));
        };
        match form {
            Form::Iso => {
                let iso = str_arg(vm, fname, &uargs, 0, "start", false)?.unwrap_or_default();
                options = if n == 2 {
                    long_arg(vm, fname, &uargs, 1, "options", false)?.unwrap_or(0)
                } else {
                    0
                };
                vm.emit_deprecated("Calling DatePeriod::__construct(string $isostr, int $options = 0) is deprecated, use DatePeriod::createFromISO8601String() instead");
                if vm.current_exception.is_some() {
                    return Err(VmError {
                        message: String::new(),
                        line: vm.current_line,
                    });
                }
                recurrences = period_init_iso(vm, &mut p, b"DateTime", &iso, fname)?;
            }
            Form::Recur | Form::End => {
                let start = match &a0 {
                    Some(Value::Object(o)) => o.clone(),
                    _ => unreachable!(),
                };
                let st = match date_get(&start) {
                    Some(t) => t,
                    None => return Err(uninitialized_error(vm, b"DateTimeInterface")),
                };
                let end_obj = if let Form::End = form {
                    match &a2 {
                        Some(Value::Object(o)) => Some(o.clone()),
                        _ => None,
                    }
                } else {
                    None
                };
                let et = match &end_obj {
                    Some(o) => match date_get(o) {
                        Some(t) => Some(t),
                        None => return Err(uninitialized_error(vm, b"DateTimeInterface")),
                    },
                    None => None,
                };
                let iv = match &a1 {
                    Some(Value::Object(o)) => need_interval(vm, o)?,
                    _ => unreachable!(),
                };
                if let Form::Recur = form {
                    recurrences =
                        long_arg(vm, fname, &uargs, 2, "recurrences", false)?.unwrap_or(0);
                }
                options = long_arg(vm, fname, &uargs, 3, "options", false)?.unwrap_or(0);
                p.start = Some(st);
                p.start_ce = class_of(&start);
                p.interval = Some(iv.diff.clone());
                p.end = et;
            }
        }
        period_init_finish(vm, &mut p, options, recurrences, fname)?;
        period_set(vm, &this, &p);
        Ok(())
    })();
    vm.call_stack.pop();
    ctor_result(r)
}

// ---------------------------------------------------------------------------
// Method dispatch

/// Parameter names (for named arguments), from php_date.stub.php
pub fn param_names(class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    Some(match (class, method) {
        (b"datetime" | b"datetimeimmutable", b"__construct") => &["datetime", "timezone"],
        (b"datetime" | b"datetimeimmutable", m) => match m {
            b"format" => &["format"],
            b"modify" => &["modifier"],
            b"add" | b"sub" => &["interval"],
            b"settimezone" => &["timezone"],
            b"settime" => &["hour", "minute", "second", "microsecond"],
            b"setdate" => &["year", "month", "day"],
            b"setisodate" => &["year", "week", "dayOfWeek"],
            b"settimestamp" => &["timestamp"],
            b"setmicrosecond" => &["microsecond"],
            b"diff" => &["targetObject", "absolute"],
            b"__unserialize" => &["data"],
            b"__set_state" => &["array"],
            b"createfromformat" => &["format", "datetime", "timezone"],
            b"createfromtimestamp" => &["timestamp"],
            b"createfromimmutable" | b"createfrommutable" | b"createfrominterface" => &["object"],
            b"gettimezone" | b"getoffset" | b"getmicrosecond" | b"gettimestamp"
            | b"getlasterrors" | b"__serialize" | b"__wakeup" => &[],
            _ => return None,
        },
        (b"datetimezone", m) => match m {
            b"__construct" => &["timezone"],
            b"getoffset" => &["datetime"],
            b"gettransitions" => &["timestampBegin", "timestampEnd"],
            b"listidentifiers" => &["timezoneGroup", "countryCode"],
            b"__unserialize" => &["data"],
            b"__set_state" => &["array"],
            b"getname" | b"getlocation" | b"listabbreviations" | b"__serialize" | b"__wakeup" => {
                &[]
            }
            _ => return None,
        },
        (b"dateinterval", m) => match m {
            b"__construct" => &["duration"],
            b"createfromdatestring" => &["datetime"],
            b"format" => &["format"],
            b"__unserialize" => &["data"],
            b"__set_state" => &["array"],
            b"__serialize" | b"__wakeup" => &[],
            _ => return None,
        },
        (b"dateperiod", m) => match m {
            b"createfromiso8601string" => &["specification", "options"],
            b"__unserialize" => &["data"],
            b"__set_state" => &["array"],
            _ => return None,
        },
        _ => return None,
    })
}

pub fn has_method(class: &[u8], method: &[u8], is_static: bool) -> bool {
    match class {
        b"datetime" | b"datetimeimmutable" => {
            if is_static {
                matches!(
                    method,
                    b"createfromformat"
                        | b"createfromtimestamp"
                        | b"getlasterrors"
                        | b"__set_state"
                        | b"createfrominterface"
                ) || (class == b"datetime" && method == b"createfromimmutable")
                    || (class == b"datetimeimmutable" && method == b"createfrommutable")
            } else {
                matches!(
                    method,
                    b"format"
                        | b"modify"
                        | b"add"
                        | b"sub"
                        | b"gettimezone"
                        | b"settimezone"
                        | b"getoffset"
                        | b"getmicrosecond"
                        | b"settime"
                        | b"setdate"
                        | b"setisodate"
                        | b"settimestamp"
                        | b"setmicrosecond"
                        | b"gettimestamp"
                        | b"diff"
                        | b"__serialize"
                        | b"__unserialize"
                        | b"__wakeup"
                )
            }
        }
        b"datetimezone" => {
            if is_static {
                matches!(
                    method,
                    b"listabbreviations" | b"listidentifiers" | b"__set_state"
                )
            } else {
                matches!(
                    method,
                    b"getname"
                        | b"getoffset"
                        | b"gettransitions"
                        | b"getlocation"
                        | b"__serialize"
                        | b"__unserialize"
                        | b"__wakeup"
                )
            }
        }
        b"dateinterval" => {
            if is_static {
                matches!(method, b"createfromdatestring" | b"__set_state")
            } else {
                matches!(
                    method,
                    b"format" | b"__serialize" | b"__unserialize" | b"__wakeup"
                )
            }
        }
        b"dateperiod" => {
            if is_static {
                matches!(method, b"createfromiso8601string" | b"__set_state")
            } else {
                matches!(
                    method,
                    b"getstartdate"
                        | b"getenddate"
                        | b"getdateinterval"
                        | b"getrecurrences"
                        | b"getiterator"
                        | b"__serialize"
                        | b"__unserialize"
                        | b"__wakeup"
                )
            }
        }
        b"internaliterator" => {
            !is_static && matches!(method, b"current" | b"key" | b"next" | b"valid" | b"rewind")
        }
        _ => false,
    }
}

const METHOD_NAMES: &[&str] = &[
    "format",
    "modify",
    "add",
    "sub",
    "getTimezone",
    "setTimezone",
    "getOffset",
    "getMicrosecond",
    "setTime",
    "setDate",
    "setISODate",
    "setTimestamp",
    "setMicrosecond",
    "getTimestamp",
    "diff",
    "__serialize",
    "__unserialize",
    "__wakeup",
    "__set_state",
    "createFromFormat",
    "createFromTimestamp",
    "getLastErrors",
    "createFromInterface",
    "createFromImmutable",
    "createFromMutable",
    "getName",
    "getTransitions",
    "getLocation",
    "listAbbreviations",
    "listIdentifiers",
    "createFromDateString",
    "createFromISO8601String",
    "getStartDate",
    "getEndDate",
    "getDateInterval",
    "getRecurrences",
    "getIterator",
    "current",
    "key",
    "next",
    "valid",
    "rewind",
];

fn canonical_method(m: &[u8]) -> String {
    METHOD_NAMES
        .iter()
        .find(|n| n.as_bytes().eq_ignore_ascii_case(m))
        .map(|s| s.to_string())
        .unwrap_or_else(|| String::from_utf8_lossy(m).into_owned())
}

fn class_canonical(class: &[u8]) -> &'static str {
    match Base::from_lower(class) {
        Some(b) => b.name(),
        None => "InternalIterator",
    }
}

pub fn call_method(
    vm: &mut Vm,
    class: &[u8],
    method: &[u8],
    called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    // When invoked from the engine (e.g. unserialize() calling __unserialize),
    // there is no call frame yet: add an "[internal function]" one.
    let mut pushed = false;
    // Iterator methods driven by foreach run without a frame, as in PHP.
    if this.is_some() && class != b"internaliterator" {
        let frame = format!("{}->{}", class_canonical(class), canonical_method(method));
        let has = vm
            .call_stack
            .last()
            .map(|f| f.0.eq_ignore_ascii_case(&frame))
            .unwrap_or(false);
        if !has {
            vm.call_stack
                .push((frame, "[internal function]".into(), 0, args.to_vec(), true));
            pushed = true;
        }
    }
    let r = call_method_inner(vm, class, method, called, this, args);
    if pushed {
        vm.call_stack.pop();
    }
    r
}

fn call_method_inner(
    vm: &mut Vm,
    class: &[u8],
    method: &[u8],
    called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    let this_obj = this.and_then(obj_of);
    let r = match class {
        b"datetime" | b"datetimeimmutable" => {
            let immutable = class == b"datetimeimmutable";
            match this_obj {
                Some(o) => datetime_method(vm, immutable, method, &o, args),
                None => datetime_static(vm, immutable, method, called, args),
            }
        }
        b"datetimezone" => match this_obj {
            Some(o) => timezone_method(vm, method, &o, args),
            None => timezone_static(vm, method, args),
        },
        b"dateinterval" => match this_obj {
            Some(o) => interval_method(vm, method, &o, args),
            None => interval_static(vm, method, args),
        },
        b"dateperiod" => match this_obj {
            Some(o) => period_method(vm, method, &o, args),
            None => period_static(vm, method, called, args),
        },
        b"internaliterator" => match this_obj {
            Some(o) => iterator_method(vm, method, &o),
            None => Ok(Value::Null),
        },
        _ => Ok(Value::Null),
    };
    r.unwrap_or(Value::Null)
}

fn mname(immutable: bool, m: &str) -> String {
    format!(
        "{}::{}",
        if immutable {
            "DateTimeImmutable"
        } else {
            "DateTime"
        },
        m
    )
}

fn datetime_method(
    vm: &mut Vm,
    immutable: bool,
    method: &[u8],
    o: &Obj,
    args: &[Value],
) -> R<Value> {
    let m = |n: &str| mname(immutable, n);
    match method {
        b"format" => {
            let f = m("format");
            check_count(vm, &f, args, 1, 1)?;
            let fmt = str_arg(vm, &f, args, 0, "format", false)?.unwrap_or_default();
            let t = need_date(vm, o)?;
            Ok(Value::String(PhpString::from_vec(date_format(
                &fmt,
                &t,
                t.is_localtime != 0,
            ))))
        }
        b"modify" => {
            let f = m("modify");
            check_count(vm, &f, args, 1, 1)?;
            let modify = str_arg(vm, &f, args, 0, "modifier", false)?.unwrap_or_default();
            let target = if immutable {
                clone_obj(vm, o)
            } else {
                o.clone()
            };
            let mut t = need_date(vm, &target)?;
            date_modify(vm, &mut t, &modify, &f, true)?;
            date_set(&target, &t);
            Ok(ret_obj(&target))
        }
        b"add" => {
            let f = m("add");
            check_count(vm, &f, args, 1, 1)?;
            let iv = obj_arg(
                vm,
                &f,
                args,
                0,
                "interval",
                "DateInterval",
                &[Base::Interval],
                false,
            )?
            .unwrap();
            let target = if immutable {
                clone_obj(vm, o)
            } else {
                o.clone()
            };
            let t = need_date(vm, &target)?;
            let ivs = need_interval(vm, &iv)?;
            let nt = date_add_impl(&t, &ivs);
            date_set(&target, &nt);
            Ok(ret_obj(&target))
        }
        b"sub" => {
            let f = m("sub");
            check_count(vm, &f, args, 1, 1)?;
            let iv = obj_arg(
                vm,
                &f,
                args,
                0,
                "interval",
                "DateInterval",
                &[Base::Interval],
                false,
            )?
            .unwrap();
            let target = if immutable {
                clone_obj(vm, o)
            } else {
                o.clone()
            };
            let t = need_date(vm, &target)?;
            let ivs = need_interval(vm, &iv)?;
            if let Some(nt) = date_sub_impl(vm, &t, &ivs, &f, true)? {
                date_set(&target, &nt);
            }
            Ok(ret_obj(&target))
        }
        b"gettimezone" => {
            check_count(vm, &m("getTimezone"), args, 0, 0)?;
            let t = need_date(vm, o)?;
            Ok(make_tz_value(vm, &t))
        }
        b"settimezone" => {
            let f = m("setTimezone");
            check_count(vm, &f, args, 1, 1)?;
            let tz = obj_arg(
                vm,
                &f,
                args,
                0,
                "timezone",
                "DateTimeZone",
                &[Base::TimeZone],
                false,
            )?
            .unwrap();
            let target = if immutable {
                clone_obj(vm, o)
            } else {
                o.clone()
            };
            let mut t = need_date(vm, &target)?;
            let z = need_tz(vm, &tz)?;
            date_timezone_set_impl(&mut t, &z);
            date_set(&target, &t);
            Ok(ret_obj(&target))
        }
        b"getoffset" => {
            check_count(vm, &m("getOffset"), args, 0, 0)?;
            let t = need_date(vm, o)?;
            Ok(Value::Long(offset_of(&t)))
        }
        b"getmicrosecond" => {
            check_count(vm, &m("getMicrosecond"), args, 0, 0)?;
            let t = need_date(vm, o)?;
            Ok(Value::Long(t.us))
        }
        b"settime" => {
            let f = m("setTime");
            check_count(vm, &f, args, 2, 4)?;
            let h = long_arg(vm, &f, args, 0, "hour", false)?.unwrap_or(0);
            let i = long_arg(vm, &f, args, 1, "minute", false)?.unwrap_or(0);
            let s = long_arg(vm, &f, args, 2, "second", false)?.unwrap_or(0);
            let ms = long_arg(vm, &f, args, 3, "microsecond", false)?.unwrap_or(0);
            let target = if immutable {
                clone_obj(vm, o)
            } else {
                o.clone()
            };
            let mut t = need_date(vm, &target)?;
            t.h = h;
            t.i = i;
            t.s = s;
            t.us = ms;
            update_ts(&mut t, None);
            update_from_sse(&mut t);
            date_set(&target, &t);
            Ok(ret_obj(&target))
        }
        b"setdate" => {
            let f = m("setDate");
            check_count(vm, &f, args, 3, 3)?;
            let y = long_arg(vm, &f, args, 0, "year", false)?.unwrap_or(0);
            let mo = long_arg(vm, &f, args, 1, "month", false)?.unwrap_or(0);
            let d = long_arg(vm, &f, args, 2, "day", false)?.unwrap_or(0);
            let target = if immutable {
                clone_obj(vm, o)
            } else {
                o.clone()
            };
            let mut t = need_date(vm, &target)?;
            t.y = y;
            t.m = mo;
            t.d = d;
            update_ts(&mut t, None);
            date_set(&target, &t);
            Ok(ret_obj(&target))
        }
        b"setisodate" => {
            let f = m("setISODate");
            check_count(vm, &f, args, 2, 3)?;
            let y = long_arg(vm, &f, args, 0, "year", false)?.unwrap_or(0);
            let w = long_arg(vm, &f, args, 1, "week", false)?.unwrap_or(0);
            let d = long_arg(vm, &f, args, 2, "dayOfWeek", false)?.unwrap_or(1);
            let target = if immutable {
                clone_obj(vm, o)
            } else {
                o.clone()
            };
            let mut t = need_date(vm, &target)?;
            isodate_set(&mut t, y, w, d);
            date_set(&target, &t);
            Ok(ret_obj(&target))
        }
        b"settimestamp" => {
            let f = m("setTimestamp");
            check_count(vm, &f, args, 1, 1)?;
            let ts = long_arg(vm, &f, args, 0, "timestamp", false)?.unwrap_or(0);
            let target = if immutable {
                clone_obj(vm, o)
            } else {
                o.clone()
            };
            let mut t = need_date(vm, &target)?;
            timestamp_set(&mut t, ts);
            date_set(&target, &t);
            Ok(ret_obj(&target))
        }
        b"setmicrosecond" => {
            let f = m("setMicrosecond");
            check_count(vm, &f, args, 1, 1)?;
            let us = long_arg(vm, &f, args, 0, "microsecond", false)?.unwrap_or(0);
            if !(0..=999999).contains(&us) {
                return Err(throw(
                    vm,
                    b"DateRangeError",
                    &format!(
                        "{}(): Argument #1 ($microsecond) must be between 0 and 999999, {} given",
                        f, us
                    ),
                ));
            }
            let _ = need_date(vm, o)?;
            let target = if immutable {
                clone_obj(vm, o)
            } else {
                o.clone()
            };
            let mut t = need_date(vm, &target)?;
            t.us = us;
            date_set(&target, &t);
            Ok(ret_obj(&target))
        }
        b"gettimestamp" => {
            check_count(vm, &m("getTimestamp"), args, 0, 0)?;
            let mut t = need_date(vm, o)?;
            if t.sse_uptodate == 0 {
                update_ts(&mut t, None);
            }
            Ok(Value::Long(t.sse))
        }
        b"diff" => {
            let f = m("diff");
            check_count(vm, &f, args, 1, 2)?;
            let other = obj_arg(
                vm,
                &f,
                args,
                0,
                "targetObject",
                "DateTimeInterface",
                DATE_BASES,
                false,
            )?
            .unwrap();
            let absolute = bool_arg(vm, &f, args, 1, "absolute")?.unwrap_or(false);
            let t1 = need_date(vm, o)?;
            let t2 = need_date(vm, &other)?;
            Ok(make_diff(vm, &t1, &t2, absolute))
        }
        b"__serialize" => {
            check_count(vm, &m("__serialize"), args, 0, 0)?;
            let t = need_date(vm, o)?;
            let mut a = new_arr();
            for (k, v) in date_props(&t) {
                a.set(ArrayKey::String(PhpString::from_bytes(k)), v);
            }
            add_common_props(vm, o, &mut a, DT_INTERNAL);
            Ok(arr_val(a))
        }
        b"__unserialize" => {
            let f = m("__unserialize");
            check_count(vm, &f, args, 1, 1)?;
            let a = array_arg(vm, &f, args, 0, "data")?;
            let ab = a.borrow().clone();
            match date_from_hash(vm, &ab)? {
                Some(t) => {
                    date_set(o, &t);
                    restore_custom_props(vm, o, &ab, DT_INTERNAL);
                    Ok(Value::Null)
                }
                None => Err(throw(
                    vm,
                    b"Error",
                    &format!(
                        "Invalid serialization data for {} object",
                        if immutable {
                            "DateTimeImmutable"
                        } else {
                            "DateTime"
                        }
                    ),
                )),
            }
        }
        b"__wakeup" => {
            let mut a = PhpArray::new();
            for (k, v) in o.borrow().properties.iter() {
                a.set(ArrayKey::String(PhpString::from_vec(k.clone())), v.clone());
            }
            match date_from_hash(vm, &a)? {
                Some(t) => {
                    date_set(o, &t);
                    Ok(Value::Null)
                }
                None => Err(throw(
                    vm,
                    b"Error",
                    &format!(
                        "Invalid serialization data for {} object",
                        if immutable {
                            "DateTimeImmutable"
                        } else {
                            "DateTime"
                        }
                    ),
                )),
            }
        }
        _ => Ok(Value::Null),
    }
}

pub fn isodate_set(t: &mut Time, y: i64, w: i64, d: i64) {
    t.y = y;
    t.m = 1;
    t.d = 1;
    t.relative = RelTime::default();
    t.relative.d = dow::daynr_from_weeknr(y, w, d);
    t.have_relative = 1;
    update_ts(t, None);
}

pub fn timestamp_set(t: &mut Time, ts: i64) {
    unixtime2local(t, ts);
    update_ts(t, None);
    t.us = 0;
}

pub fn make_diff(vm: &mut Vm, t1: &Time, t2: &Time, absolute: bool) -> Value {
    let mut d = diff(t1, t2);
    if absolute {
        d.invert = 0;
    }
    let o = new_object(vm, b"DateInterval");
    interval_set(
        &o,
        &IntervalObj {
            diff: d,
            civil_or_wall: CIVIL,
            from_string: false,
            date_string: Vec::new(),
        },
    );
    Value::Object(o)
}

fn datetime_static(
    vm: &mut Vm,
    immutable: bool,
    method: &[u8],
    called: &[u8],
    args: &[Value],
) -> R<Value> {
    let m = |n: &str| mname(immutable, n);
    let called_or_base = |vm: &Vm| -> Vec<u8> {
        if base_of(vm, called).is_some() {
            called.to_vec()
        } else if immutable {
            b"DateTimeImmutable".to_vec()
        } else {
            b"DateTime".to_vec()
        }
    };
    match method {
        b"createfromformat" => {
            let f = m("createFromFormat");
            check_count(vm, &f, args, 2, 3)?;
            let format = str_arg(vm, &f, args, 0, "format", false)?.unwrap_or_default();
            let ts = path_arg(vm, &f, args, 1, "datetime")?.unwrap_or_default();
            let tz = obj_arg(
                vm,
                &f,
                args,
                2,
                "timezone",
                "DateTimeZone",
                &[Base::TimeZone],
                true,
            )?;
            let tzobj = tz.as_ref().map(|o| {
                tz_get(o).unwrap_or(TzObj {
                    ztype: 0,
                    utc_offset: 0,
                    dst: 0,
                    abbr: String::new(),
                    tz: None,
                })
            });
            let cls = called_or_base(vm);
            check_instantiable(vm, &cls)?;
            match date_initialize(vm, &ts, Some(&format), tzobj.as_ref(), INIT_FORMAT)? {
                Some(t) => {
                    let o = new_object(vm, &cls);
                    date_set(&o, &t);
                    Ok(Value::Object(o))
                }
                None => Ok(Value::False),
            }
        }
        b"createfromtimestamp" => {
            let f = m("createFromTimestamp");
            check_count(vm, &f, args, 1, 1)?;
            let v = args[0].deref();
            let t = match &v {
                Value::Long(l) => initialize_from_ts_long(*l, 0),
                Value::Double(d) => initialize_from_ts_double(vm, &f, *d)?,
                Value::String(sv) => match numeric_str(sv.as_bytes()) {
                    Some(Ok(l)) => initialize_from_ts_long(l, 0),
                    Some(Err(d)) => initialize_from_ts_double(vm, &f, d)?,
                    None => return Err(type_err_class(vm, &f, 0, "timestamp", "int|float", &v)),
                },
                Value::True => initialize_from_ts_long(1, 0),
                Value::False => initialize_from_ts_long(0, 0),
                _ => return Err(type_err_class(vm, &f, 0, "timestamp", "int|float", &v)),
            };
            let cls = called_or_base(vm);
            check_instantiable(vm, &cls)?;
            let o = new_object(vm, &cls);
            date_set(&o, &t);
            Ok(Value::Object(o))
        }
        b"getlasterrors" => {
            check_count(vm, &m("getLastErrors"), args, 0, 0)?;
            Ok(last_errors_value())
        }
        b"createfromimmutable" | b"createfrommutable" | b"createfrominterface" => {
            let mn = match method {
                b"createfromimmutable" => "createFromImmutable",
                b"createfrommutable" => "createFromMutable",
                _ => "createFromInterface",
            };
            let f = m(mn);
            check_count(vm, &f, args, 1, 1)?;
            let (tn, ok): (&str, &[Base]) = match method {
                b"createfromimmutable" => ("DateTimeImmutable", &[Base::Immutable]),
                b"createfrommutable" => ("DateTime", &[Base::DateTime]),
                _ => ("DateTimeInterface", DATE_BASES),
            };
            let src = obj_arg(vm, &f, args, 0, "object", tn, ok, false)?.unwrap();
            let t = need_date(vm, &src)?;
            let cls = called_or_base(vm);
            check_instantiable(vm, &cls)?;
            let o = new_object(vm, &cls);
            date_set(&o, &t);
            Ok(Value::Object(o))
        }
        b"__set_state" => {
            let f = m("__set_state");
            check_count(vm, &f, args, 1, 1)?;
            let a = array_arg(vm, &f, args, 0, "array")?;
            let ab = a.borrow().clone();
            let cls: &[u8] = if immutable {
                b"DateTimeImmutable"
            } else {
                b"DateTime"
            };
            match date_from_hash(vm, &ab)? {
                Some(t) => {
                    let o = new_object(vm, cls);
                    date_set(&o, &t);
                    Ok(Value::Object(o))
                }
                None => Err(throw(
                    vm,
                    b"Error",
                    &format!(
                        "Invalid serialization data for {} object",
                        if immutable {
                            "DateTimeImmutable"
                        } else {
                            "DateTime"
                        }
                    ),
                )),
            }
        }
        _ => Ok(Value::Null),
    }
}

// ---------------------------------------------------------------------------
// DateTimeZone

pub fn tz_offset_for(z: &TzObj, t: &Time) -> i64 {
    match z.ztype {
        ZONETYPE_ID => {
            z.tz.as_ref()
                .map(|tz| tz.get_time_zone_info(t.sse).offset as i64)
                .unwrap_or(0)
        }
        ZONETYPE_OFFSET => z.utc_offset,
        ZONETYPE_ABBR => z.utc_offset + z.dst as i64 * 3600,
        _ => 0,
    }
}

pub fn transitions(vm: &mut Vm, z: &TzObj, begin: i64, end: i64) -> Value {
    if z.ztype != ZONETYPE_ID {
        return Value::False;
    }
    let tz = match &z.tz {
        Some(t) => t.clone(),
        None => return Value::False,
    };
    let mut out = new_arr();
    let fmt = b"x-m-d\\TH:i:sP";
    let add_el =
        |vm: &mut Vm, out: &mut PhpArray, ts: i64, offset: i64, isdst: bool, abbr: String| {
            let mut el = new_arr();
            arr_set(&mut el, "ts", Value::Long(ts));
            arr_set(
                &mut el,
                "time",
                Value::String(PhpString::from_vec(php_format_date(vm, fmt, ts, false))),
            );
            arr_set(&mut el, "offset", Value::Long(offset));
            arr_set(
                &mut el,
                "isdst",
                if isdst { Value::True } else { Value::False },
            );
            arr_set(&mut el, "abbr", ss(abbr));
            out.push(arr_val(el));
        };
    let ty = |i: usize| tz.types[i];
    let add_idx = |vm: &mut Vm, out: &mut PhpArray, i: usize, ts: i64| {
        let t = ty(tz.trans_idx[i] as usize);
        add_el(
            vm,
            out,
            ts,
            t.offset as i64,
            t.isdst != 0,
            tz.abbr_at(t.abbr_idx),
        );
    };
    let add_nominal = |vm: &mut Vm, out: &mut PhpArray| {
        let t = ty(0);
        add_el(
            vm,
            out,
            begin,
            t.offset as i64,
            t.isdst != 0,
            tz.abbr_at(t.abbr_idx),
        );
    };
    let mut beg = 0usize;

    let found = if begin == i64::MIN {
        add_nominal(vm, &mut out);
        true
    } else {
        let mut f = false;
        if tz.timecnt > 0 {
            while beg < tz.timecnt {
                if tz.trans[beg] > begin {
                    if beg > 0 {
                        add_idx(vm, &mut out, beg - 1, begin);
                    } else {
                        add_nominal(vm, &mut out);
                    }
                    f = true;
                    break;
                }
                beg += 1;
            }
        }
        f
    };
    let has_posix_end = tz
        .posix_info
        .as_ref()
        .map(|p| p.dst_end.is_some())
        .unwrap_or(false);
    if !found {
        if tz.timecnt > 0 {
            if has_posix_end {
                let tto = tz.get_time_zone_info(begin);
                add_el(
                    vm,
                    &mut out,
                    begin,
                    tto.offset as i64,
                    tto.is_dst != 0,
                    tto.abbr,
                );
            } else {
                add_idx(vm, &mut out, tz.timecnt - 1, begin);
            }
        } else {
            add_nominal(vm, &mut out);
        }
    } else {
        for i in beg..tz.timecnt {
            if tz.trans[i] < end {
                add_idx(vm, &mut out, i, tz.trans[i]);
            } else {
                return arr_val(out);
            }
        }
    }
    if has_posix_end && tz.timecnt > 0 {
        let last = tz.trans[tz.timecnt - 1];
        let (start_y, _, _) = unixtime2date(last);
        let (end_y, _, _) = unixtime2date(end);
        let mut y = start_y;
        while y <= end_y {
            let mut tr = Vec::new();
            tz.get_transitions_for_year(y, &mut tr);
            for (time, typ) in tr {
                if time <= last {
                    continue;
                }
                if time < begin {
                    continue;
                }
                if time > end {
                    return arr_val(out);
                }
                let t = ty(typ);
                add_el(
                    vm,
                    &mut out,
                    time,
                    t.offset as i64,
                    t.isdst != 0,
                    tz.abbr_at(t.abbr_idx),
                );
            }
            y += 1;
        }
    }
    arr_val(out)
}

pub fn location(z: &TzObj) -> Value {
    if z.ztype != ZONETYPE_ID {
        return Value::False;
    }
    let tz = match &z.tz {
        Some(t) => t,
        None => return Value::False,
    };
    let mut a = new_arr();
    arr_set(&mut a, "country_code", ss(tz.country_code.clone()));
    arr_set(&mut a, "latitude", Value::Double(tz.latitude));
    arr_set(&mut a, "longitude", Value::Double(tz.longitude));
    arr_set(&mut a, "comments", ss(tz.comments.clone()));
    arr_val(a)
}

fn timezone_method(vm: &mut Vm, method: &[u8], o: &Obj, args: &[Value]) -> R<Value> {
    match method {
        b"getname" => {
            check_count(vm, "DateTimeZone::getName", args, 0, 0)?;
            let z = need_tz(vm, o)?;
            Ok(ss(z.to_string()))
        }
        b"getoffset" => {
            let f = "DateTimeZone::getOffset";
            check_count(vm, f, args, 1, 1)?;
            let d = obj_arg(
                vm,
                f,
                args,
                0,
                "datetime",
                "DateTimeInterface",
                DATE_BASES,
                false,
            )?
            .unwrap();
            let z = need_tz(vm, o)?;
            let t = need_date(vm, &d)?;
            Ok(Value::Long(tz_offset_for(&z, &t)))
        }
        b"gettransitions" => {
            let f = "DateTimeZone::getTransitions";
            check_count(vm, f, args, 0, 2)?;
            let b = long_arg(vm, f, args, 0, "timestampBegin", false)?.unwrap_or(i64::MIN);
            let e = long_arg(vm, f, args, 1, "timestampEnd", false)?.unwrap_or(i32::MAX as i64);
            let z = need_tz(vm, o)?;
            Ok(transitions(vm, &z, b, e))
        }
        b"getlocation" => {
            check_count(vm, "DateTimeZone::getLocation", args, 0, 0)?;
            let z = need_tz(vm, o)?;
            Ok(location(&z))
        }
        b"__serialize" => {
            check_count(vm, "DateTimeZone::__serialize", args, 0, 0)?;
            let z = need_tz(vm, o)?;
            let mut a = new_arr();
            for (k, v) in tz_props(&z) {
                a.set(ArrayKey::String(PhpString::from_bytes(k)), v);
            }
            add_common_props(vm, o, &mut a, TZ_INTERNAL);
            Ok(arr_val(a))
        }
        b"__unserialize" => {
            let f = "DateTimeZone::__unserialize";
            check_count(vm, f, args, 1, 1)?;
            let a = array_arg(vm, f, args, 0, "data")?;
            let ab = a.borrow().clone();
            match tz_from_hash(&ab) {
                Some(z) => {
                    tz_set(o, &z);
                    restore_custom_props(vm, o, &ab, TZ_INTERNAL);
                    Ok(Value::Null)
                }
                None => Err(throw(
                    vm,
                    b"Error",
                    "Invalid serialization data for DateTimeZone object",
                )),
            }
        }
        b"__wakeup" => {
            let mut a = PhpArray::new();
            for (k, v) in o.borrow().properties.iter() {
                a.set(ArrayKey::String(PhpString::from_vec(k.clone())), v.clone());
            }
            match tz_from_hash(&a) {
                Some(z) => {
                    tz_set(o, &z);
                    Ok(Value::Null)
                }
                None => Err(throw(
                    vm,
                    b"Error",
                    "Invalid serialization data for DateTimeZone object",
                )),
            }
        }
        _ => Ok(Value::Null),
    }
}

fn timezone_static(vm: &mut Vm, method: &[u8], args: &[Value]) -> R<Value> {
    match method {
        b"listabbreviations" => abbreviations_list(vm, "DateTimeZone::listAbbreviations", args),
        b"listidentifiers" => identifiers_list(vm, "DateTimeZone::listIdentifiers", args),
        b"__set_state" => {
            let f = "DateTimeZone::__set_state";
            check_count(vm, f, args, 1, 1)?;
            let a = array_arg(vm, f, args, 0, "array")?;
            let ab = a.borrow().clone();
            match tz_from_hash(&ab) {
                Some(z) => {
                    let o = new_object(vm, b"DateTimeZone");
                    tz_set(&o, &z);
                    Ok(Value::Object(o))
                }
                None => Err(throw(
                    vm,
                    b"Error",
                    "Invalid serialization data for DateTimeZone object",
                )),
            }
        }
        _ => Ok(Value::Null),
    }
}

// ---------------------------------------------------------------------------
// DateInterval

fn interval_from_hash(vm: &mut Vm, a: &PhpArray) -> R<Option<IntervalObj>> {
    if let Some(Value::String(ds)) = arr_get(a, b"date_string") {
        let dsb = ds.as_bytes().to_vec();
        let (t, err) = strtotime(&dsb);
        if err.error_count() > 0 {
            let e = &err.error_messages[0];
            let msg = format!(
                "Unknown or bad format ({}) at position {} ({}) while unserializing: {}",
                cstr(&dsb),
                e.position,
                first_error_char(e),
                e.message
            );
            return Err(throw(vm, b"Error", &msg));
        }
        return Ok(Some(IntervalObj {
            diff: t.relative.clone(),
            civil_or_wall: CIVIL,
            from_string: true,
            date_string: dsb,
        }));
    }
    let mut d = RelTime::default();
    let scalar = |v: &Value| {
        matches!(
            v,
            Value::Null
                | Value::True
                | Value::False
                | Value::Long(_)
                | Value::Double(_)
                | Value::String(_)
                | Value::Undef
        )
    };
    let rd = |k: &[u8], def: i64| -> i64 {
        match arr_get(a, k) {
            Some(v) if scalar(v) => v.to_long(),
            _ => def,
        }
    };
    d.y = rd(b"y", -1);
    d.m = rd(b"m", -1);
    d.d = rd(b"d", -1);
    d.h = rd(b"h", -1);
    d.i = rd(b"i", -1);
    d.s = rd(b"s", -1);
    if let Some(v) = arr_get(a, b"f") {
        d.us = dval_to_lval(vm, v.to_double() * 1000000.0);
    }
    d.weekday = rd(b"weekday", -1) as i32;
    d.weekday_behavior = rd(b"weekday_behavior", -1) as i32;
    d.first_last_day_of = rd(b"first_last_day_of", -1) as i32;
    d.invert = rd(b"invert", 0) as i32;
    d.days = match arr_get(a, b"days") {
        Some(Value::False) => UNSET,
        Some(v) if scalar(v) => a64i(&v.to_php_string().to_string_lossy()),
        _ => -1,
    };
    d.special_type = rd(b"special_type", 0) as u32;
    d.special_amount = match arr_get(a, b"special_amount") {
        Some(v) if scalar(v) => a64i(&v.to_php_string().to_string_lossy()),
        _ => -1,
    };
    d.have_weekday_relative = rd(b"have_weekday_relative", 0) as u32;
    d.have_special_relative = rd(b"have_special_relative", 0) as u32;
    let civil_or_wall = match arr_get(a, b"civil_or_wall") {
        Some(v) => v.to_long(),
        None => CIVIL,
    };
    Ok(Some(IntervalObj {
        diff: d,
        civil_or_wall,
        from_string: false,
        date_string: Vec::new(),
    }))
}

/// zend_dval_to_lval (PHP 8.5: warns when out of range)
pub fn dval_to_lval(vm: &mut Vm, d: f64) -> i64 {
    if !d.is_finite() {
        vm.emit_warning(&format!(
            "The float {} is not representable as an int, cast occurred",
            goro_core::value::format_php_float(d)
        ));
        return 0;
    }
    if !(-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&d) {
        vm.emit_warning(&format!(
            "The float {} is not representable as an int, cast occurred",
            goro_core::value::format_php_float(d)
        ));
        let two64 = 18446744073709551616.0f64;
        let mut dmod = d % two64;
        if dmod < 0.0 {
            dmod += two64;
        }
        return dmod as u64 as i64;
    }
    d as i64
}

/// DATE_A64I (strtoll)
fn a64i(s: &str) -> i64 {
    let t = s.trim_start();
    let mut end = 0;
    let b = t.as_bytes();
    if end < b.len() && (b[end] == b'+' || b[end] == b'-') {
        end += 1;
    }
    while end < b.len() && b[end].is_ascii_digit() {
        end += 1;
    }
    t[..end].parse::<i64>().unwrap_or_else(|_| {
        if t.starts_with('-') && end > 1 {
            i64::MIN
        } else if end > 0 && b[0] != b'-' && end > (b[0] == b'+') as usize {
            i64::MAX
        } else {
            0
        }
    })
}

fn interval_method(vm: &mut Vm, method: &[u8], o: &Obj, args: &[Value]) -> R<Value> {
    match method {
        b"format" => {
            let f = "DateInterval::format";
            check_count(vm, f, args, 1, 1)?;
            let fmt = str_arg(vm, f, args, 0, "format", false)?.unwrap_or_default();
            let iv = need_interval(vm, o)?;
            Ok(Value::String(PhpString::from_vec(interval_format(
                &fmt, &iv.diff,
            ))))
        }
        b"__serialize" => {
            check_count(vm, "DateInterval::__serialize", args, 0, 0)?;
            let iv = need_interval(vm, o)?;
            let mut a = new_arr();
            for (k, v) in interval_props(&iv) {
                a.set(ArrayKey::String(PhpString::from_bytes(k)), v);
            }
            add_common_props(vm, o, &mut a, IV_INTERNAL);
            Ok(arr_val(a))
        }
        b"__unserialize" => {
            let f = "DateInterval::__unserialize";
            check_count(vm, f, args, 1, 1)?;
            let a = array_arg(vm, f, args, 0, "data")?;
            let ab = a.borrow().clone();
            if let Some(iv) = interval_from_hash(vm, &ab)? {
                interval_set(o, &iv);
            }
            restore_custom_props(vm, o, &ab, IV_INTERNAL);
            Ok(Value::Null)
        }
        b"__wakeup" => {
            let mut a = PhpArray::new();
            for (k, v) in o.borrow().properties.iter() {
                a.set(ArrayKey::String(PhpString::from_vec(k.clone())), v.clone());
            }
            if let Some(iv) = interval_from_hash(vm, &a)? {
                interval_set(o, &iv);
            }
            Ok(Value::Null)
        }
        _ => Ok(Value::Null),
    }
}

pub fn interval_from_date_string(vm: &mut Vm, s: &[u8], throw_exc: bool, fname: &str) -> R<Value> {
    let (t, err) = strtotime(s);
    if err.error_count() > 0 {
        let e = &err.error_messages[0];
        let msg = format!(
            "Unknown or bad format ({}) at position {} ({}): {}",
            cstr(s),
            e.position,
            first_error_char(e),
            e.message
        );
        if throw_exc {
            return Err(throw(vm, b"DateMalformedIntervalStringException", &msg));
        }
        vm.emit_warning(&format!("{}(): {}", fname, msg));
        return Ok(Value::False);
    }
    if t.have_date != 0 || t.have_time != 0 || t.have_zone != 0 {
        let msg = format!("String '{}' contains non-relative elements", cstr(s));
        if throw_exc {
            return Err(throw(vm, b"DateMalformedIntervalStringException", &msg));
        }
        vm.emit_warning(&format!("{}(): {}", fname, msg));
        return Ok(Value::False);
    }
    let o = new_object(vm, b"DateInterval");
    interval_set(
        &o,
        &IntervalObj {
            diff: t.relative.clone(),
            civil_or_wall: CIVIL,
            from_string: true,
            date_string: s.to_vec(),
        },
    );
    Ok(Value::Object(o))
}

fn interval_static(vm: &mut Vm, method: &[u8], args: &[Value]) -> R<Value> {
    match method {
        b"createfromdatestring" => {
            let f = "DateInterval::createFromDateString";
            check_count(vm, f, args, 1, 1)?;
            let s = str_arg(vm, f, args, 0, "datetime", false)?.unwrap_or_default();
            interval_from_date_string(vm, &s, true, f)
        }
        b"__set_state" => {
            let f = "DateInterval::__set_state";
            check_count(vm, f, args, 1, 1)?;
            let a = array_arg(vm, f, args, 0, "array")?;
            let ab = a.borrow().clone();
            let o = new_object(vm, b"DateInterval");
            if let Some(iv) = interval_from_hash(vm, &ab)? {
                interval_set(&o, &iv);
            }
            Ok(Value::Object(o))
        }
        _ => Ok(Value::Null),
    }
}

// ---------------------------------------------------------------------------
// DatePeriod

fn need_period(vm: &mut Vm, o: &Obj) -> PeriodObj {
    let _ = vm;
    period_get(o).unwrap_or_default()
}

fn copy_date_obj(vm: &mut Vm, t: &Time, ce: &[u8]) -> Value {
    let o = new_object(vm, ce);
    date_set(&o, t);
    Value::Object(o)
}

fn period_method(vm: &mut Vm, method: &[u8], o: &Obj, args: &[Value]) -> R<Value> {
    match method {
        b"getstartdate" => {
            check_count(vm, "DatePeriod::getStartDate", args, 0, 0)?;
            let p = need_period(vm, o);
            match &p.start {
                Some(t) => Ok(copy_date_obj(vm, t, &p.start_ce)),
                None => Err(uninitialized_error(vm, &class_of(o))),
            }
        }
        b"getenddate" => {
            check_count(vm, "DatePeriod::getEndDate", args, 0, 0)?;
            let p = need_period(vm, o);
            match &p.end {
                Some(t) => Ok(copy_date_obj(vm, t, &p.start_ce)),
                None => Ok(Value::Null),
            }
        }
        b"getdateinterval" => {
            check_count(vm, "DatePeriod::getDateInterval", args, 0, 0)?;
            let p = need_period(vm, o);
            match &p.interval {
                Some(r) => Ok(make_interval_obj(vm, &Some(r.clone()), 0)),
                None => Err(uninitialized_error(vm, &class_of(o))),
            }
        }
        b"getrecurrences" => {
            check_count(vm, "DatePeriod::getRecurrences", args, 0, 0)?;
            let p = need_period(vm, o);
            let r = p.recurrences - p.include_start_date as i64 - p.include_end_date as i64;
            if r == 0 {
                Ok(Value::Null)
            } else {
                Ok(Value::Long(r))
            }
        }
        b"getiterator" => {
            check_count(vm, "DatePeriod::getIterator", args, 0, 0)?;
            let it = new_object(vm, b"InternalIterator");
            {
                let mut ib = it.borrow_mut();
                ib.set_property(
                    b"__timestamp_iter_period".to_vec(),
                    Value::Object(o.clone()),
                );
                ib.set_property(b"__timestamp_iter_index".to_vec(), Value::Long(0));
                ib.set_property(b"__timestamp_iter_started".to_vec(), Value::False);
            }
            Ok(Value::Object(it))
        }
        b"__serialize" => {
            check_count(vm, "DatePeriod::__serialize", args, 0, 0)?;
            let p = need_period(vm, o);
            if p.start.is_none() {
                return Err(uninitialized_error(vm, &class_of(o)));
            }
            let mut a = new_arr();
            for (k, v) in period_props(vm, &p) {
                a.set(ArrayKey::String(PhpString::from_bytes(k)), v);
            }
            add_common_props(vm, o, &mut a, DP_INTERNAL);
            Ok(arr_val(a))
        }
        b"__unserialize" => {
            let f = "DatePeriod::__unserialize";
            check_count(vm, f, args, 1, 1)?;
            let a = array_arg(vm, f, args, 0, "data")?;
            let ab = a.borrow().clone();
            let mut p = need_period(vm, o);
            let ok = period_from_hash(vm, &mut p, &ab);
            // php_date_period_initialize_from_hash does no rollback on error
            period_set(vm, o, &p);
            if !ok {
                return Err(throw(
                    vm,
                    b"Error",
                    "Invalid serialization data for DatePeriod object",
                ));
            }
            restore_custom_props(vm, o, &ab, DP_INTERNAL);
            Ok(Value::Null)
        }
        b"__wakeup" => {
            let mut a = PhpArray::new();
            for (k, v) in o.borrow().properties.iter() {
                a.set(ArrayKey::String(PhpString::from_vec(k.clone())), v.clone());
            }
            let mut p = need_period(vm, o);
            if !period_from_hash(vm, &mut p, &a) {
                return Err(throw(
                    vm,
                    b"Error",
                    "Invalid serialization data for DatePeriod object",
                ));
            }
            period_set(vm, o, &p);
            Ok(Value::Null)
        }
        _ => Ok(Value::Null),
    }
}

fn period_from_hash(vm: &mut Vm, p: &mut PeriodObj, a: &PhpArray) -> bool {
    let date_entry = |vm: &Vm, v: Option<&Value>| -> Result<Option<(Time, Vec<u8>)>, ()> {
        match v {
            None => Err(()),
            Some(Value::Null) => Ok(None),
            Some(Value::Object(o)) if is_date_interface(vm, o) => match date_get(o) {
                Some(t) => Ok(Some((t, o.borrow().class_name.clone()))),
                None => Err(()),
            },
            Some(_) => Err(()),
        }
    };
    match date_entry(vm, arr_get(a, b"start")) {
        Err(_) => return false,
        Ok(Some((t, ce))) => {
            p.start = Some(t);
            p.start_ce = ce;
        }
        Ok(None) => {}
    }
    match date_entry(vm, arr_get(a, b"end")) {
        Err(_) => return false,
        Ok(Some((t, _))) => {
            if p.start_ce.is_empty() {
                return false;
            }
            p.end = Some(t);
        }
        Ok(None) => {}
    }
    match date_entry(vm, arr_get(a, b"current")) {
        Err(_) => return false,
        Ok(Some((t, _))) => {
            if p.start_ce.is_empty() {
                return false;
            }
            p.current = Some(t);
        }
        Ok(None) => {}
    }
    match arr_get(a, b"interval") {
        Some(Value::Object(o)) if o.borrow().class_name.eq_ignore_ascii_case(b"DateInterval") => {
            match interval_get(o) {
                Some(iv) => p.interval = Some(iv.diff),
                None => return false,
            }
        }
        _ => return false,
    }
    match arr_get(a, b"recurrences") {
        Some(Value::Long(r)) if *r >= 0 && *r <= i32::MAX as i64 => p.recurrences = *r,
        _ => return false,
    }
    match arr_get(a, b"include_start_date") {
        Some(Value::True) => p.include_start_date = true,
        Some(Value::False) => p.include_start_date = false,
        _ => return false,
    }
    match arr_get(a, b"include_end_date") {
        Some(Value::True) => p.include_end_date = true,
        Some(Value::False) => p.include_end_date = false,
        _ => return false,
    }
    p.initialized = true;
    true
}

fn period_static(vm: &mut Vm, method: &[u8], called: &[u8], args: &[Value]) -> R<Value> {
    match method {
        b"createfromiso8601string" => {
            let f = "DatePeriod::createFromISO8601String";
            check_count(vm, f, args, 1, 2)?;
            let iso = str_arg(vm, f, args, 0, "specification", false)?.unwrap_or_default();
            let options = long_arg(vm, f, args, 1, "options", false)?.unwrap_or(0);
            let cls: Vec<u8> = if base_of(vm, called) == Some(Base::Period) {
                called.to_vec()
            } else {
                b"DatePeriod".to_vec()
            };
            check_instantiable(vm, &cls)?;
            let o = new_object(vm, &cls);
            let mut p = PeriodObj::default();
            let rec = period_init_iso(vm, &mut p, b"DateTimeImmutable", &iso, f)?;
            period_init_finish(vm, &mut p, options, rec, f)?;
            period_set(vm, &o, &p);
            Ok(Value::Object(o))
        }
        b"__set_state" => {
            let f = "DatePeriod::__set_state";
            check_count(vm, f, args, 1, 1)?;
            let a = array_arg(vm, f, args, 0, "array")?;
            let ab = a.borrow().clone();
            let o = new_object(vm, b"DatePeriod");
            let mut p = PeriodObj::default();
            if !period_from_hash(vm, &mut p, &ab) {
                return Err(throw(
                    vm,
                    b"Error",
                    "Invalid serialization data for DatePeriod object",
                ));
            }
            period_set(vm, &o, &p);
            Ok(Value::Object(o))
        }
        _ => Ok(Value::Null),
    }
}

fn period_advance(t: &mut Time, iv: &RelTime) {
    t.have_relative = 1;
    t.relative = iv.clone();
    t.sse_uptodate = 0;
    update_ts(t, None);
    update_from_sse(t);
}

fn base_date_class(vm: &Vm, ce: &[u8]) -> &'static [u8] {
    match base_of(vm, ce) {
        Some(Base::Immutable) => b"DateTimeImmutable",
        _ => b"DateTime",
    }
}

fn iterator_method(vm: &mut Vm, method: &[u8], it: &Obj) -> R<Value> {
    let period = match it.borrow().get_property(b"__timestamp_iter_period") {
        Value::Object(o) => o,
        _ => return Ok(Value::Null),
    };
    // InternalIterator implicitly rewinds before first use.
    if method != b"rewind"
        && !it
            .borrow()
            .get_property(b"__timestamp_iter_started")
            .is_truthy()
    {
        iterator_method(vm, b"rewind", it)?;
    }
    let index = it
        .borrow()
        .get_property(b"__timestamp_iter_index")
        .to_long();
    let mut p = period_get(&period).unwrap_or_default();
    match method {
        b"rewind" => {
            let start = match &p.start {
                Some(s) => s.clone(),
                None => return Err(uninitialized_error(vm, b"DatePeriod")),
            };
            let mut cur = start;
            if !p.include_start_date {
                if let Some(iv) = &p.interval {
                    period_advance(&mut cur, iv);
                }
            }
            p.current = Some(cur);
            period_set(vm, &period, &p);
            let mut ib = it.borrow_mut();
            ib.set_property(b"__timestamp_iter_index".to_vec(), Value::Long(0));
            ib.set_property(b"__timestamp_iter_started".to_vec(), Value::True);
            Ok(Value::Null)
        }
        b"valid" => {
            let cur = match &p.current {
                Some(c) => c,
                None => return Ok(Value::False),
            };
            let r = if let Some(end) = &p.end {
                if cur.sse == end.sse {
                    if p.include_end_date {
                        cur.us <= end.us
                    } else {
                        cur.us < end.us
                    }
                } else {
                    cur.sse < end.sse
                }
            } else {
                index < p.recurrences
            };
            Ok(if r { Value::True } else { Value::False })
        }
        b"current" => match &p.current {
            Some(c) => {
                let cls = base_date_class(vm, &p.start_ce);
                Ok(copy_date_obj(vm, c, cls))
            }
            None => Ok(Value::Null),
        },
        b"key" => Ok(Value::Long(index)),
        b"next" => {
            if let (Some(cur), Some(iv)) = (p.current.as_mut(), p.interval.clone()) {
                period_advance(cur, &iv);
            }
            period_set(vm, &period, &p);
            it.borrow_mut()
                .set_property(b"__timestamp_iter_index".to_vec(), Value::Long(index + 1));
            Ok(Value::Null)
        }
        _ => Ok(Value::Null),
    }
}

// ---------------------------------------------------------------------------
// Procedural object functions

fn date_create_common(vm: &mut Vm, args: &[Value], fname: &str, cls: &[u8]) -> R<Value> {
    check_count(vm, fname, args, 0, 2)?;
    let time_str = str_arg(vm, fname, args, 0, "datetime", false)?.unwrap_or_default();
    let tz = obj_arg(
        vm,
        fname,
        args,
        1,
        "timezone",
        "DateTimeZone",
        &[Base::TimeZone],
        true,
    )?;
    let tzobj = tz.as_ref().map(|o| {
        tz_get(o).unwrap_or(TzObj {
            ztype: 0,
            utc_offset: 0,
            dst: 0,
            abbr: String::new(),
            tz: None,
        })
    });
    match date_initialize(vm, &time_str, None, tzobj.as_ref(), 0)? {
        Some(t) => {
            let o = new_object(vm, cls);
            date_set(&o, &t);
            Ok(Value::Object(o))
        }
        None => Ok(Value::False),
    }
}

pub fn date_create_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    date_create_common(vm, args, "date_create", b"DateTime")
}

pub fn date_create_immutable_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    date_create_common(vm, args, "date_create_immutable", b"DateTimeImmutable")
}

fn date_create_from_format_common(
    vm: &mut Vm,
    args: &[Value],
    fname: &str,
    cls: &[u8],
) -> R<Value> {
    check_count(vm, fname, args, 2, 3)?;
    let format = str_arg(vm, fname, args, 0, "format", false)?.unwrap_or_default();
    let ts = path_arg(vm, fname, args, 1, "datetime")?.unwrap_or_default();
    let tz = obj_arg(
        vm,
        fname,
        args,
        2,
        "timezone",
        "DateTimeZone",
        &[Base::TimeZone],
        true,
    )?;
    let tzobj = tz.as_ref().map(|o| {
        tz_get(o).unwrap_or(TzObj {
            ztype: 0,
            utc_offset: 0,
            dst: 0,
            abbr: String::new(),
            tz: None,
        })
    });
    match date_initialize(vm, &ts, Some(&format), tzobj.as_ref(), INIT_FORMAT)? {
        Some(t) => {
            let o = new_object(vm, cls);
            date_set(&o, &t);
            Ok(Value::Object(o))
        }
        None => Ok(Value::False),
    }
}

pub fn date_create_from_format_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    date_create_from_format_common(vm, args, "date_create_from_format", b"DateTime")
}

pub fn date_create_immutable_from_format_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    date_create_from_format_common(
        vm,
        args,
        "date_create_immutable_from_format",
        b"DateTimeImmutable",
    )
}

fn date_obj_arg(
    vm: &mut Vm,
    fname: &str,
    args: &[Value],
    idx: usize,
    mutable_only: bool,
) -> R<Obj> {
    let (tn, ok): (&str, &[Base]) = if mutable_only {
        ("DateTime", &[Base::DateTime])
    } else {
        ("DateTimeInterface", DATE_BASES)
    };
    Ok(obj_arg(vm, fname, args, idx, "object", tn, ok, false)?
        .unwrap_or_else(|| Rc::new(RefCell::new(PhpObject::new(Vec::new(), 0)))))
}

fn need_args(vm: &mut Vm, fname: &str, args: &[Value], min: usize, max: usize) -> R<()> {
    check_count(vm, fname, args, min, max)
}

pub fn date_format_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    need_args(vm, "date_format", args, 2, 2)?;
    let o = date_obj_arg(vm, "date_format", args, 0, false)?;
    let fmt = str_arg(vm, "date_format", args, 1, "format", false)?.unwrap_or_default();
    let t = need_date(vm, &o)?;
    Ok(Value::String(PhpString::from_vec(date_format(
        &fmt,
        &t,
        t.is_localtime != 0,
    ))))
}

pub fn date_modify_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    need_args(vm, "date_modify", args, 2, 2)?;
    let o = date_obj_arg(vm, "date_modify", args, 0, true)?;
    let modify = str_arg(vm, "date_modify", args, 1, "modifier", false)?.unwrap_or_default();
    let mut t = need_date(vm, &o)?;
    if !date_modify(vm, &mut t, &modify, "date_modify", false)? {
        return Ok(Value::False);
    }
    date_set(&o, &t);
    Ok(ret_obj(&o))
}

pub fn date_add_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    need_args(vm, "date_add", args, 2, 2)?;
    let o = date_obj_arg(vm, "date_add", args, 0, true)?;
    let iv = obj_arg(
        vm,
        "date_add",
        args,
        1,
        "interval",
        "DateInterval",
        &[Base::Interval],
        false,
    )?
    .unwrap();
    let t = need_date(vm, &o)?;
    let ivs = need_interval(vm, &iv)?;
    date_set(&o, &date_add_impl(&t, &ivs));
    Ok(ret_obj(&o))
}

pub fn date_sub_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    need_args(vm, "date_sub", args, 2, 2)?;
    let o = date_obj_arg(vm, "date_sub", args, 0, true)?;
    let iv = obj_arg(
        vm,
        "date_sub",
        args,
        1,
        "interval",
        "DateInterval",
        &[Base::Interval],
        false,
    )?
    .unwrap();
    let t = need_date(vm, &o)?;
    let ivs = need_interval(vm, &iv)?;
    if let Some(nt) = date_sub_impl(vm, &t, &ivs, "date_sub", false)? {
        date_set(&o, &nt);
    }
    Ok(ret_obj(&o))
}

pub fn date_timezone_get_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    need_args(vm, "date_timezone_get", args, 1, 1)?;
    let o = date_obj_arg(vm, "date_timezone_get", args, 0, false)?;
    let t = need_date(vm, &o)?;
    Ok(make_tz_value(vm, &t))
}

pub fn date_timezone_set_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    need_args(vm, "date_timezone_set", args, 2, 2)?;
    let o = date_obj_arg(vm, "date_timezone_set", args, 0, true)?;
    let tz = obj_arg(
        vm,
        "date_timezone_set",
        args,
        1,
        "timezone",
        "DateTimeZone",
        &[Base::TimeZone],
        false,
    )?
    .unwrap();
    let mut t = need_date(vm, &o)?;
    let z = need_tz(vm, &tz)?;
    date_timezone_set_impl(&mut t, &z);
    date_set(&o, &t);
    Ok(ret_obj(&o))
}

pub fn date_offset_get_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    need_args(vm, "date_offset_get", args, 1, 1)?;
    let o = date_obj_arg(vm, "date_offset_get", args, 0, false)?;
    let t = need_date(vm, &o)?;
    Ok(Value::Long(offset_of(&t)))
}

pub fn date_diff_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    need_args(vm, "date_diff", args, 2, 3)?;
    let o1 = obj_arg(
        vm,
        "date_diff",
        args,
        0,
        "baseObject",
        "DateTimeInterface",
        DATE_BASES,
        false,
    )?
    .unwrap();
    let o2 = obj_arg(
        vm,
        "date_diff",
        args,
        1,
        "targetObject",
        "DateTimeInterface",
        DATE_BASES,
        false,
    )?
    .unwrap();
    let absolute = bool_arg(vm, "date_diff", args, 2, "absolute")?.unwrap_or(false);
    let t1 = need_date(vm, &o1)?;
    let t2 = need_date(vm, &o2)?;
    Ok(make_diff(vm, &t1, &t2, absolute))
}

pub fn date_time_set_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let f = "date_time_set";
    need_args(vm, f, args, 3, 5)?;
    let o = date_obj_arg(vm, f, args, 0, true)?;
    let h = long_arg(vm, f, args, 1, "hour", false)?.unwrap_or(0);
    let i = long_arg(vm, f, args, 2, "minute", false)?.unwrap_or(0);
    let s = long_arg(vm, f, args, 3, "second", false)?.unwrap_or(0);
    let ms = long_arg(vm, f, args, 4, "microsecond", false)?.unwrap_or(0);
    let mut t = need_date(vm, &o)?;
    t.h = h;
    t.i = i;
    t.s = s;
    t.us = ms;
    update_ts(&mut t, None);
    update_from_sse(&mut t);
    date_set(&o, &t);
    Ok(ret_obj(&o))
}

pub fn date_date_set_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let f = "date_date_set";
    need_args(vm, f, args, 4, 4)?;
    let o = date_obj_arg(vm, f, args, 0, true)?;
    let y = long_arg(vm, f, args, 1, "year", false)?.unwrap_or(0);
    let m = long_arg(vm, f, args, 2, "month", false)?.unwrap_or(0);
    let d = long_arg(vm, f, args, 3, "day", false)?.unwrap_or(0);
    let mut t = need_date(vm, &o)?;
    t.y = y;
    t.m = m;
    t.d = d;
    update_ts(&mut t, None);
    date_set(&o, &t);
    Ok(ret_obj(&o))
}

pub fn date_isodate_set_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let f = "date_isodate_set";
    need_args(vm, f, args, 3, 4)?;
    let o = date_obj_arg(vm, f, args, 0, true)?;
    let y = long_arg(vm, f, args, 1, "year", false)?.unwrap_or(0);
    let w = long_arg(vm, f, args, 2, "week", false)?.unwrap_or(0);
    let d = long_arg(vm, f, args, 3, "dayOfWeek", false)?.unwrap_or(1);
    let mut t = need_date(vm, &o)?;
    isodate_set(&mut t, y, w, d);
    date_set(&o, &t);
    Ok(ret_obj(&o))
}

pub fn date_timestamp_set_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let f = "date_timestamp_set";
    need_args(vm, f, args, 2, 2)?;
    let o = date_obj_arg(vm, f, args, 0, true)?;
    let ts = long_arg(vm, f, args, 1, "timestamp", false)?.unwrap_or(0);
    let mut t = need_date(vm, &o)?;
    timestamp_set(&mut t, ts);
    date_set(&o, &t);
    Ok(ret_obj(&o))
}

pub fn date_timestamp_get_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    need_args(vm, "date_timestamp_get", args, 1, 1)?;
    let o = date_obj_arg(vm, "date_timestamp_get", args, 0, false)?;
    let mut t = need_date(vm, &o)?;
    if t.sse_uptodate == 0 {
        update_ts(&mut t, None);
    }
    Ok(Value::Long(t.sse))
}

pub fn timezone_open_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    need_args(vm, "timezone_open", args, 1, 1)?;
    let tz = path_arg(vm, "timezone_open", args, 0, "timezone")?.unwrap_or_default();
    match timezone_initialize(&tz) {
        Ok(z) => {
            let o = new_object(vm, b"DateTimeZone");
            tz_set(&o, &z);
            Ok(Value::Object(o))
        }
        Err(m) => {
            vm.emit_warning(&format!("timezone_open(): {}", m));
            Ok(Value::False)
        }
    }
}

fn tz_obj_arg(vm: &mut Vm, fname: &str, args: &[Value]) -> R<Obj> {
    Ok(obj_arg(
        vm,
        fname,
        args,
        0,
        "object",
        "DateTimeZone",
        &[Base::TimeZone],
        false,
    )?
    .unwrap())
}

pub fn timezone_name_get_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    need_args(vm, "timezone_name_get", args, 1, 1)?;
    let o = tz_obj_arg(vm, "timezone_name_get", args)?;
    let z = need_tz(vm, &o)?;
    Ok(ss(z.to_string()))
}

pub fn timezone_offset_get_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    need_args(vm, "timezone_offset_get", args, 2, 2)?;
    let o = tz_obj_arg(vm, "timezone_offset_get", args)?;
    let d = obj_arg(
        vm,
        "timezone_offset_get",
        args,
        1,
        "datetime",
        "DateTimeInterface",
        DATE_BASES,
        false,
    )?
    .unwrap();
    let z = need_tz(vm, &o)?;
    let t = need_date(vm, &d)?;
    Ok(Value::Long(tz_offset_for(&z, &t)))
}

pub fn timezone_transitions_get_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let f = "timezone_transitions_get";
    need_args(vm, f, args, 1, 3)?;
    let o = tz_obj_arg(vm, f, args)?;
    let b = long_arg(vm, f, args, 1, "timestampBegin", false)?.unwrap_or(i64::MIN);
    let e = long_arg(vm, f, args, 2, "timestampEnd", false)?.unwrap_or(i32::MAX as i64);
    let z = need_tz(vm, &o)?;
    Ok(transitions(vm, &z, b, e))
}

pub fn timezone_location_get_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    need_args(vm, "timezone_location_get", args, 1, 1)?;
    let o = tz_obj_arg(vm, "timezone_location_get", args)?;
    let z = need_tz(vm, &o)?;
    Ok(location(&z))
}

pub fn date_interval_create_from_date_string_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let f = "date_interval_create_from_date_string";
    need_args(vm, f, args, 1, 1)?;
    let s = str_arg(vm, f, args, 0, "datetime", false)?.unwrap_or_default();
    interval_from_date_string(vm, &s, false, f)
}

pub fn date_interval_format_fn(vm: &mut Vm, args: &[Value]) -> R<Value> {
    let f = "date_interval_format";
    need_args(vm, f, args, 2, 2)?;
    let o = obj_arg(
        vm,
        f,
        args,
        0,
        "object",
        "DateInterval",
        &[Base::Interval],
        false,
    )?
    .unwrap();
    let fmt = str_arg(vm, f, args, 1, "format", false)?.unwrap_or_default();
    let iv = need_interval(vm, &o)?;
    Ok(Value::String(PhpString::from_vec(interval_format(
        &fmt, &iv.diff,
    ))))
}
