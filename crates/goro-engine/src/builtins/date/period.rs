//! DatePeriod (PHP manual: DatePeriod) and its iterator.
//!
//! The properties (start, current, end, interval, recurrences,
//! include_start_date, include_end_date) are computed views: each read
//! makes new objects (observed on PHP 8.5: `$p->start === $p->start` is
//! false).

use std::rc::Rc;

use goro_ext_date::DateTime;
use goro_ext_date::period::{Period, PeriodError};
use goro_runtime::{ObjRef, PhpArray, Value};

use super::*;
use crate::class::Class;

pub(crate) struct PeriodObj {
    pub p: Option<Period>,
    /// The class of the start date (the dates produced have it).
    pub start_cls: Option<Rc<Class>>,
    pub end_cls: Option<Rc<Class>>,
    /// The `current` property: the last date computed by an iteration.
    pub cur: Option<DateTime>,
}
impl goro_runtime::NativeData for PeriodObj {}

type Peeked = (
    Period,
    Option<Rc<Class>>,
    Option<Rc<Class>>,
    Option<DateTime>,
);
type Got = (Period, Rc<Class>, Rc<Class>, Option<DateTime>);

fn peek(o: &ObjRef) -> Option<Peeked> {
    with_native::<PeriodObj, _>(o, |d| {
        d.p.clone()
            .map(|p| (p, d.start_cls.clone(), d.end_cls.clone(), d.cur.clone()))
    })
    .flatten()
}

fn get(vm: &mut Vm, o: &ObjRef) -> R<Got> {
    match peek(o) {
        Some((p, sc, ec, cur)) => {
            let sc = match sc {
                Some(c) => c,
                None => class(vm, "DateTime")?,
            };
            let ec = match ec {
                Some(c) => c,
                None => sc.clone(),
            };
            Ok((p, sc, ec, cur))
        }
        None => uninit(vm, o),
    }
}

fn store(
    vm: &mut Vm,
    o: &ObjRef,
    p: Period,
    sc: Rc<Class>,
    ec: Option<Rc<Class>>,
    cur: Option<DateTime>,
) -> R<()> {
    *o.obj().native.borrow_mut() = Some(Box::new(PeriodObj {
        p: Some(p),
        start_cls: Some(sc),
        end_cls: ec,
        cur,
    }));
    sync_props(vm, o)
}

/// Writes the seven properties (PHP 8.2+ keeps them as properties: they
/// show in get_object_vars() and json_encode(), test
/// date_period_properties.phpt).
fn sync_props(vm: &mut Vm, o: &ObjRef) -> R<()> {
    let mut parts = PhpArray::new();
    period_parts(vm, o, &mut parts)?;
    for (k, v) in parts.iter() {
        if let goro_runtime::KeyRef::Str(n) = k {
            vm.set_prop_raw(o, n.as_bytes(), v.clone());
        }
    }
    Ok(())
}

/// A period's interval as its properties show it: a createFromDateString()
/// interval appears as its relative numbers (test
/// DatePeriod_serialize-004.phpt).
fn plain_interval(iv: &goro_ext_date::interval::Interval) -> goro_ext_date::interval::Interval {
    let mut out = iv.clone();
    out.from_string = None;
    out
}

/// The seven properties, in PHP's order.
fn period_parts(vm: &mut Vm, o: &ObjRef, out: &mut PhpArray) -> R<()> {
    let Some((p, sc, ec, cur)) = peek(o) else {
        return Ok(());
    };
    let sc = match sc {
        Some(c) => c,
        None => class(vm, "DateTime")?,
    };
    let ec = ec.unwrap_or_else(|| sc.clone());
    let start = new_date(vm, &sc, p.start.clone())?;
    out.insert(key("start"), Value::Object(start));
    let cur = match cur {
        Some(d) => Value::Object(new_date(vm, &sc, d)?),
        None => Value::Null,
    };
    out.insert(key("current"), cur);
    let end = match &p.end {
        Some(d) => Value::Object(new_date(vm, &ec, d.clone())?),
        None => Value::Null,
    };
    out.insert(key("end"), end);
    let iv = new_iv(vm, &plain_interval(&p.interval))?;
    out.insert(key("interval"), Value::Object(iv));
    out.insert(key("recurrences"), Value::Int(p.recurrences_property()));
    out.insert(key("include_start_date"), Value::Bool(p.include_start));
    out.insert(key("include_end_date"), Value::Bool(p.include_end));
    Ok(())
}

pub(super) fn period_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|_, o| {
            *o.obj().native.borrow_mut() = Some(Box::new(PeriodObj {
                p: None,
                start_cls: None,
                end_cls: None,
                cur: None,
            }));
        }),
        clone: Some(|_, from, to| {
            let d = with_native::<PeriodObj, _>(from, |d| PeriodObj {
                p: d.p.clone(),
                start_cls: d.start_cls.clone(),
                end_cls: d.end_cls.clone(),
                cur: d.cur.clone(),
            });
            if let Some(d) = d {
                *to.obj().native.borrow_mut() = Some(Box::new(d));
            }
        }),
        dyn_prop: Some(|vm, o, name, op| {
            if !PERIOD_KEYS.iter().any(|k| k.as_bytes() == name.as_bytes()) {
                return None;
            }
            let n = name.to_string_lossy();
            match op {
                crate::class::DynPropOp::Read => None,
                crate::class::DynPropOp::Write => Some(vm.throw(
                    "Error",
                    &format!("Cannot modify readonly property DatePeriod::${n}"),
                )),
                crate::class::DynPropOp::Unset => {
                    let cls = vm.class_of(o).name.to_string_lossy();
                    Some(vm.throw("Error", &format!("Cannot unset {cls}::${n}")))
                }
            }
        }),
        get_iterator: Some(|vm, o, _| {
            let cls = vm.class_of(o);
            match cls.find_method(b"getiterator") {
                Some(f) if !f.is_native() => match vm.call_method(o, b"getiterator", Vec::new())? {
                    Value::Object(it) => Ok(it),
                    _ => vm.throw(
                        "Exception",
                        &format!(
                            "Objects returned by {}::getIterator() must be traversable or implement interface Iterator",
                            cls.name.to_string_lossy()
                        ),
                    ),
                },
                _ => new_iterator(vm, o),
            }
        }),
        ..Default::default()
    }
}

/// The date of a constructor argument; uninitialized ones are reported
/// as "DateTimeInterface" (test bug-gh11416.phpt).
fn get_dt_iface(vm: &mut Vm, o: &ObjRef) -> R<DateTime> {
    match peek_dt(o) {
        Some(d) => Ok(d),
        None => vm.throw(
            "DateObjectError",
            "Object of type DateTimeInterface has not been correctly initialized by calling parent::__construct() in its constructor",
        ),
    }
}

/// Period::from_iso, with an empty specification refused as a bad
/// format (test bug54283.phpt).
fn from_iso(spec: &[u8], options: i64, func: &str) -> Result<Period, PeriodError> {
    if spec.iter().all(|c| c.is_ascii_whitespace()) {
        return Err(PeriodError::MalformedPeriod(format!(
            "Unknown or bad format ({})",
            String::from_utf8_lossy(spec)
        )));
    }
    Period::from_iso(spec, options, func)
}

fn period_error<T>(vm: &mut Vm, e: PeriodError) -> R<T> {
    match e {
        PeriodError::MalformedPeriod(m) => vm.throw("DateMalformedPeriodStringException", &m),
        PeriodError::Malformed(m) => vm.throw("DateMalformedStringException", &m),
    }
}

const BAD_ARGS: &str = "DatePeriod::__construct() accepts (DateTimeInterface, DateInterval, int [, int]), or (DateTimeInterface, DateInterval, DateTime [, int]), or (string [, int]) as arguments";

pub(super) fn dp_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let func = "DatePeriod::__construct";
    if a.is_empty() {
        return vm.type_error(BAD_ARGS);
    }
    match a.val(0) {
        Value::Object(st) if is_date(&st) => {
            let Value::Object(ivo) = a.val(1) else {
                return vm.type_error(BAD_ARGS);
            };
            if with_native::<IvObj, _>(&ivo, |_| ()).is_none() {
                return vm.type_error(BAD_ARGS);
            }
            let options = match a.val(3) {
                Value::Null => 0,
                v => vm.to_int(&v)?,
            };
            let start = get_dt_iface(vm, &st)?;
            let iv = get_iv(vm, &ivo)?;
            let sc = vm.class_of(&st);
            let (end, rec, ec) = match a.val(2) {
                Value::Int(r) => (None, Some(r), None),
                Value::Object(e) if is_date(&e) => {
                    let d = get_dt_iface(vm, &e)?;
                    (Some(d), None, Some(vm.class_of(&e)))
                }
                _ => return vm.type_error(BAD_ARGS),
            };
            match Period::new(start, iv, end, rec, options, func) {
                Ok(p) => store(vm, &o, p, sc, ec, None)?,
                Err(e) => return period_error(vm, e),
            }
        }
        v @ (Value::Str(_) | Value::Null) if a.len() <= 2 => {
            if v.is_null() {
                vm.deprecated("DatePeriod::__construct(): Passing null to parameter #1 ($start) of type string is deprecated")?;
            }
            vm.deprecated("Calling DatePeriod::__construct(string $isostr, int $options = 0) is deprecated, use DatePeriod::createFromISO8601String() instead")?;
            let spec = a.str(0);
            let options = match a.val(1) {
                Value::Null => 0,
                v => vm.to_int(&v)?,
            };
            match from_iso(&spec, options, func) {
                Ok(p) => {
                    let sc = class(vm, "DateTime")?;
                    store(vm, &o, p, sc, None, None)?
                }
                Err(e) => return period_error(vm, e),
            }
        }
        _ => return vm.type_error(BAD_ARGS),
    }
    Ok(Value::Null)
}

pub(super) fn dp_create_from_iso(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cls = static_class(vm, a);
    let o = vm.instantiate(&cls)?;
    let spec = a.str(0);
    let options = a.int_or(1, 0);
    match from_iso(&spec, options, "DatePeriod::createFromISO8601String") {
        Ok(p) => {
            let sc = class(vm, "DateTimeImmutable")?;
            store(vm, &o, p, sc, None, None)?;
            Ok(Value::Object(o))
        }
        Err(e) => period_error(vm, e),
    }
}

pub(super) fn dp_get_start(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (p, sc, _, _) = get(vm, a.this_obj())?;
    Ok(Value::Object(new_date(vm, &sc, p.start)?))
}

pub(super) fn dp_get_end(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if peek(a.this_obj()).is_none() {
        return Ok(Value::Null);
    }
    let (p, _, ec, _) = get(vm, a.this_obj())?;
    match p.end {
        Some(d) => Ok(Value::Object(new_date(vm, &ec, d)?)),
        None => Ok(Value::Null),
    }
}

pub(super) fn dp_get_interval(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (p, _, _, _) = get(vm, a.this_obj())?;
    Ok(Value::Object(new_iv(vm, &plain_interval(&p.interval))?))
}

pub(super) fn dp_get_recurrences(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if peek(a.this_obj()).is_none() {
        return Ok(Value::Null);
    }
    let (p, _, _, _) = get(vm, a.this_obj())?;
    Ok(match p.get_recurrences() {
        Some(n) => Value::Int(n),
        None => Value::Null,
    })
}

pub(super) fn dp_serialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    get(vm, &o)?;
    let mut out = PhpArray::new();
    period_parts(vm, &o, &mut out)?;
    for (k, v) in vm.plain_props_array(&o, true).iter() {
        out.insert(k.to_owned(), v.clone());
    }
    Ok(Value::Array(out))
}

const PERIOD_KEYS: &[&str] = &[
    "start",
    "current",
    "end",
    "interval",
    "recurrences",
    "include_start_date",
    "include_end_date",
];

type Restored = (Period, Rc<Class>, Option<Rc<Class>>, Option<DateTime>);

/// The period described by serialized / __set_state data, if valid.
fn period_from_data(vm: &mut Vm, data: &PhpArray) -> R<Option<Restored>> {
    let date = |v: Option<&Value>| -> Result<Option<ObjRef>, ()> {
        match v.map(|v| v.deref()) {
            Some(Value::Object(o)) if peek_dt(&o).is_some() => Ok(Some(o)),
            Some(Value::Null) => Ok(None),
            _ => Err(()),
        }
    };
    let Ok(start) = date(data.get_bytes(b"start")) else {
        return Ok(None);
    };
    let Ok(cur) = date(data.get_bytes(b"current")) else {
        return Ok(None);
    };
    let Ok(end) = date(data.get_bytes(b"end")) else {
        return Ok(None);
    };
    // a null start only goes with a null end (test gh20936.phpt)
    if start.is_none() && end.is_some() {
        return Ok(None);
    }
    let iv = match data.get_bytes(b"interval").map(|v| v.deref()) {
        Some(Value::Object(o)) if with_native::<IvObj, _>(&o, |d| d.init) == Some(true) => {
            get_iv(vm, &o)?
        }
        _ => return Ok(None),
    };
    let Some(Value::Int(rec)) = data.get_bytes(b"recurrences").map(|v| v.deref()) else {
        return Ok(None);
    };
    if rec < 0 {
        return Ok(None);
    }
    let Some(Value::Bool(is)) = data.get_bytes(b"include_start_date").map(|v| v.deref()) else {
        return Ok(None);
    };
    let Some(Value::Bool(ie)) = data.get_bytes(b"include_end_date").map(|v| v.deref()) else {
        return Ok(None);
    };
    let given = rec - i64::from(is) - i64::from(ie);
    let recurrences = if end.is_some() && given <= 0 {
        None
    } else {
        Some(given)
    };
    // a null start is accepted; the period then starts at the epoch
    let (sc, start_dt) = match &start {
        Some(st) => (vm.class_of(st), peek_dt(st).expect("checked")),
        None => (
            class(vm, "DateTime")?,
            DateTime::from_ts(0, 0, goro_ext_date::TimeZone::utc()),
        ),
    };
    let ec = end.as_ref().map(|e| vm.class_of(e));
    let p = Period {
        start: start_dt,
        interval: iv,
        end: end.as_ref().and_then(peek_dt),
        recurrences,
        include_start: is,
        include_end: ie,
    };
    Ok(Some((p, sc, ec, cur.as_ref().and_then(peek_dt))))
}

fn restore(vm: &mut Vm, a: &Args, o: &ObjRef, data: &PhpArray) -> R<()> {
    let Some((p, sc, ec, cur)) = period_from_data(vm, data)? else {
        partial_restore(vm, o, data)?;
        return vm.throw("Error", "Invalid serialization data for DatePeriod object");
    };
    store(vm, o, p, sc, ec, cur)?;
    let _ = a;
    super::classes::restore_props(vm, o, data, PERIOD_KEYS)?;
    Ok(())
}

/// Invalid data still replaces the fields read before the bad one, in
/// the order start, end, current, interval, recurrences (test
/// date_period_unserialize3.phpt).
fn partial_restore(vm: &mut Vm, o: &ObjRef, data: &PhpArray) -> R<()> {
    let Some((mut p, mut sc, mut ec, mut cur)) = peek(o) else {
        return Ok(());
    };
    let date = |k: &[u8]| -> Option<Option<ObjRef>> {
        match data.get_bytes(k).map(|v| v.deref()) {
            Some(Value::Object(d)) if peek_dt(&d).is_some() => Some(Some(d)),
            Some(Value::Null) => Some(None),
            _ => None,
        }
    };
    'apply: {
        match date(b"start") {
            Some(Some(d)) => {
                p.start = peek_dt(&d).expect("checked");
                sc = Some(vm.class_of(&d));
            }
            _ => break 'apply,
        }
        match date(b"end") {
            Some(Some(d)) => {
                p.end = peek_dt(&d);
                ec = Some(vm.class_of(&d));
            }
            Some(None) => p.end = None,
            None => break 'apply,
        }
        match date(b"current") {
            Some(d) => cur = d.as_ref().and_then(peek_dt),
            None => break 'apply,
        }
        match data.get_bytes(b"interval").map(|v| v.deref()) {
            Some(Value::Object(i)) if with_native::<IvObj, _>(&i, |d| d.init) == Some(true) => {
                p.interval = get_iv(vm, &i)?;
            }
            _ => break 'apply,
        }
        if let Some(Value::Int(r)) = data.get_bytes(b"recurrences").map(|v| v.deref())
            && r >= 0
        {
            let given = r - i64::from(p.include_start) - i64::from(p.include_end);
            p.recurrences = if p.end.is_some() && given <= 0 {
                None
            } else {
                Some(given)
            };
        }
    }
    let sc = match sc {
        Some(c) => c,
        None => class(vm, "DateTime")?,
    };
    store(vm, o, p, sc, ec, cur)
}

pub(super) fn dp_unserialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let data = a.array(0);
    restore(vm, a, &o, &data)?;
    Ok(Value::Null)
}

pub(super) fn dp_wakeup(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let data = vm.plain_props_array(&o, false);
    let Some((p, sc, ec, cur)) = period_from_data(vm, &data)? else {
        return vm.throw("Error", "Invalid serialization data for DatePeriod object");
    };
    store(vm, &o, p, sc, ec, cur)?;
    Ok(Value::Null)
}

pub(super) fn dp_set_state(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cls = static_class(vm, a);
    let data = a.array(0);
    let o = vm.instantiate(&cls)?;
    restore(vm, a, &o, &data)?;
    Ok(Value::Object(o))
}

pub(super) fn dp_get_iterator(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    Ok(Value::Object(new_iterator(vm, &o)?))
}

// ---------------------------------------------------------------- iterator

/// The iterator's state. It holds the period object, whose `current`
/// property it updates.
pub(crate) struct DpIter {
    period: Option<ObjRef>,
    p: Period,
    cls: Rc<Class>,
    cur: Option<DateTime>,
    key: i64,
    yielded: i64,
}

impl goro_runtime::NativeData for DpIter {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        if let Some(p) = &self.period {
            visit(&Value::Object(p.clone()));
        }
    }
    fn clear(&mut self) {
        self.period = None;
    }
}

fn new_iterator(vm: &mut Vm, o: &ObjRef) -> R<ObjRef> {
    if peek(o).is_none() {
        return vm.throw(
            "DateObjectError",
            "Object of type DatePeriod has not been correctly initialized by calling parent::__construct() in its constructor",
        );
    }
    let (p, sc, _, _) = get(vm, o)?;
    let cls = class(vm, "DatePeriodIterator")?;
    let it = vm.instantiate_raw(&cls)?;
    *it.obj().native.borrow_mut() = Some(Box::new(DpIter {
        period: Some(o.clone()),
        p,
        cls: sc,
        cur: None,
        key: 0,
        yielded: 0,
    }));
    iter_rewind(vm, &it)?;
    Ok(it)
}

/// Whether `d` ends the iteration after `yielded` dates.
fn stops(p: &Period, d: &DateTime, yielded: i64) -> bool {
    match &p.end {
        Some(e) => {
            let k = (d.ts, d.us);
            let ke = (e.ts, e.us);
            k > ke || (k == ke && !p.include_end)
        }
        None => yielded >= p.recurrences.unwrap_or(0) + i64::from(p.include_start),
    }
}

/// Records `d` as the period's `current` date.
fn set_period_current(vm: &mut Vm, period: Option<ObjRef>, cls: &Rc<Class>, d: DateTime) -> R<()> {
    let Some(po) = period else {
        return Ok(());
    };
    with_native::<PeriodObj, _>(&po, |x| x.cur = Some(d.clone()));
    let obj = new_date(vm, cls, d)?;
    vm.set_prop_raw(&po, b"current", Value::Object(obj));
    Ok(())
}

fn iter_rewind(vm: &mut Vm, it: &ObjRef) -> R<()> {
    let r = with_native::<DpIter, _>(it, |s| {
        let d = if s.p.include_start {
            s.p.start.clone()
        } else {
            s.p.step(&s.p.start)
        };
        s.key = 0;
        if stops(&s.p, &d, 0) {
            s.cur = None;
            s.yielded = 0;
        } else {
            s.cur = Some(d.clone());
            s.yielded = 1;
        }
        (s.period.clone(), s.cls.clone(), d)
    });
    match r {
        Some((po, cls, d)) => set_period_current(vm, po, &cls, d),
        None => Ok(()),
    }
}

fn iter_next(vm: &mut Vm, it: &ObjRef) -> R<()> {
    let r = with_native::<DpIter, _>(it, |s| {
        let c = s.cur.clone()?;
        let d = s.p.step(&c);
        if stops(&s.p, &d, s.yielded) {
            s.cur = None;
        } else {
            s.cur = Some(d.clone());
            s.key += 1;
            s.yielded += 1;
        }
        Some((s.period.clone(), s.cls.clone(), d))
    });
    match r.flatten() {
        Some((po, cls, d)) => set_period_current(vm, po, &cls, d),
        None => Ok(()),
    }
}

pub(super) fn it_current(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let st = with_native::<DpIter, _>(a.this_obj(), |s| (s.cur.clone(), s.cls.clone()));
    match st {
        Some((Some(d), cls)) => Ok(Value::Object(new_date(vm, &cls, d)?)),
        _ => Ok(Value::Null),
    }
}

pub(super) fn it_key(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let st = with_native::<DpIter, _>(a.this_obj(), |s| s.cur.as_ref().map(|_| s.key));
    Ok(match st.flatten() {
        Some(k) => Value::Int(k),
        None => Value::Null,
    })
}

pub(super) fn it_next(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let it = a.this_obj().clone();
    iter_next(vm, &it)?;
    Ok(Value::Null)
}

pub(super) fn it_valid(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let st = with_native::<DpIter, _>(a.this_obj(), |s| s.cur.is_some());
    Ok(Value::Bool(st == Some(true)))
}

pub(super) fn it_rewind(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let it = a.this_obj().clone();
    iter_rewind(vm, &it)?;
    Ok(Value::Null)
}
