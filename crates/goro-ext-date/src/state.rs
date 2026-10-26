//! Object state storage for the date classes.
//!
//! goro objects are property bags, so the timelib state of a DateTime,
//! DateTimeZone, DateInterval or DatePeriod is serialised into hidden
//! `__timestamp*` properties (which every dumper/serializer in goro skips),
//! while the PHP-visible properties (`date`, `timezone_type`, ...) are kept in
//! sync after every mutation.

use crate::timelib::*;
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use std::cell::RefCell;
use std::rc::Rc;

pub const P_SSE: &[u8] = b"__timestamp";
pub const P_STATE: &[u8] = b"__timestamp_state";
pub const P_TZ: &[u8] = b"__timestamp_tzobj";
pub const P_IVAL: &[u8] = b"__timestamp_ival";
pub const P_PERIOD: &[u8] = b"__timestamp_period";

pub const CIVIL: i64 = 1;
pub const WALL: i64 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Base {
    DateTime,
    Immutable,
    TimeZone,
    Interval,
    Period,
}

impl Base {
    pub fn name(self) -> &'static str {
        match self {
            Base::DateTime => "DateTime",
            Base::Immutable => "DateTimeImmutable",
            Base::TimeZone => "DateTimeZone",
            Base::Interval => "DateInterval",
            Base::Period => "DatePeriod",
        }
    }
    pub fn from_lower(c: &[u8]) -> Option<Base> {
        Some(match c {
            b"datetime" => Base::DateTime,
            b"datetimeimmutable" => Base::Immutable,
            b"datetimezone" => Base::TimeZone,
            b"dateinterval" => Base::Interval,
            b"dateperiod" => Base::Period,
            _ => return None,
        })
    }
}

/// Find the built-in date base class of a (possibly user) class.
pub fn base_of(vm: &Vm, class_name: &[u8]) -> Option<Base> {
    let mut cur = class_name.to_ascii_lowercase();
    for _ in 0..64 {
        if let Some(b) = Base::from_lower(&cur) {
            return Some(b);
        }
        {
            let p = vm.classes.get(&cur).and_then(|c| c.parent.clone())?;
            cur = p.to_ascii_lowercase()
        }
    }
    None
}

pub fn obj_base(vm: &Vm, o: &Rc<RefCell<PhpObject>>) -> Option<Base> {
    let cn = o.borrow().class_name.clone();
    base_of(vm, &cn)
}

pub fn is_date_interface(vm: &Vm, o: &Rc<RefCell<PhpObject>>) -> bool {
    matches!(
        obj_base(vm, o),
        Some(Base::DateTime) | Some(Base::Immutable)
    )
}

/// Canonical (declared) name of a class.
pub fn class_display(vm: &Vm, class_name: &[u8]) -> String {
    let lower = class_name.to_ascii_lowercase();
    if let Some(b) = Base::from_lower(&lower) {
        return b.name().to_string();
    }
    if let Some(c) = vm.classes.get(&lower) {
        return String::from_utf8_lossy(&c.name).into_owned();
    }
    String::from_utf8_lossy(class_name).into_owned()
}

/// Allocate a new object of the given class (user or built-in) with property
/// defaults applied, without calling its constructor (object_init_ex).
pub fn new_object(vm: &mut Vm, class_name: &[u8]) -> Rc<RefCell<PhpObject>> {
    let lower = class_name.to_ascii_lowercase();
    let display = class_display(vm, class_name);
    let id = vm.alloc_object_id();
    let mut obj = PhpObject::new(display.into_bytes(), id);
    let mut has_dtor = false;
    if let Some(c) = vm.classes.get(&lower) {
        has_dtor = c.get_method(b"__destruct").is_some();
        vm.apply_class_property_defaults(&mut obj, &lower);
    }
    let o = Rc::new(RefCell::new(obj));
    goro_core::gc::track_object(&o);
    // (a user subclass's destructor runs for the objects made internally)
    if has_dtor {
        vm.track_destructible(Value::Object(o.clone()));
    }
    o
}

// ---------------------------------------------------------------------------
// Binary encoding helpers

struct Enc(Vec<u8>);
impl Enc {
    fn i(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn s(&mut self, v: Option<&str>) {
        match v {
            None => self.i(-1),
            Some(s) => {
                self.i(s.len() as i64);
                self.0.extend_from_slice(s.as_bytes());
            }
        }
    }
}

struct Dec<'a> {
    b: &'a [u8],
    p: usize,
}
impl Dec<'_> {
    fn i(&mut self) -> Option<i64> {
        let v = i64::from_le_bytes(self.b.get(self.p..self.p + 8)?.try_into().ok()?);
        self.p += 8;
        Some(v)
    }
    fn s(&mut self) -> Option<Option<String>> {
        let l = self.i()?;
        if l < 0 {
            return Some(None);
        }
        let l = l as usize;
        let v = String::from_utf8_lossy(self.b.get(self.p..self.p + l)?).into_owned();
        self.p += l;
        Some(Some(v))
    }
}

fn enc_rel(e: &mut Enc, r: &RelTime) {
    for v in [r.y, r.m, r.d, r.h, r.i, r.s, r.us] {
        e.i(v);
    }
    e.i(r.weekday as i64);
    e.i(r.weekday_behavior as i64);
    e.i(r.first_last_day_of as i64);
    e.i(r.invert as i64);
    e.i(r.days);
    e.i(r.special_type as i64);
    e.i(r.special_amount);
    e.i(r.have_weekday_relative as i64);
    e.i(r.have_special_relative as i64);
}

fn dec_rel(d: &mut Dec) -> Option<RelTime> {
    Some(RelTime {
        y: d.i()?,
        m: d.i()?,
        d: d.i()?,
        h: d.i()?,
        i: d.i()?,
        s: d.i()?,
        us: d.i()?,
        weekday: d.i()? as i32,
        weekday_behavior: d.i()? as i32,
        first_last_day_of: d.i()? as i32,
        invert: d.i()? as i32,
        days: d.i()?,
        special_type: d.i()? as u32,
        special_amount: d.i()?,
        have_weekday_relative: d.i()? as u32,
        have_special_relative: d.i()? as u32,
    })
}

fn enc_time(e: &mut Enc, t: &Time) {
    for v in [t.y, t.m, t.d, t.h, t.i, t.s, t.us, t.z, t.dst as i64, t.sse] {
        e.i(v);
    }
    for v in [
        t.have_time,
        t.have_date,
        t.have_zone,
        t.have_relative,
        t.have_weeknr_day,
        t.sse_uptodate,
        t.tim_uptodate,
        t.is_localtime,
        t.zone_type,
    ] {
        e.i(v as i64);
    }
    e.s(t.tz_abbr.as_deref());
    e.s(t.tz_info.as_ref().map(|z| z.name.as_str()));
    enc_rel(e, &t.relative);
}

fn dec_time(d: &mut Dec) -> Option<Time> {
    let mut t = Time::new();
    t.y = d.i()?;
    t.m = d.i()?;
    t.d = d.i()?;
    t.h = d.i()?;
    t.i = d.i()?;
    t.s = d.i()?;
    t.us = d.i()?;
    t.z = d.i()?;
    t.dst = d.i()? as i32;
    t.sse = d.i()?;
    t.have_time = d.i()? as u32;
    t.have_date = d.i()? as u32;
    t.have_zone = d.i()? as u32;
    t.have_relative = d.i()? as u32;
    t.have_weeknr_day = d.i()? as u32;
    t.sse_uptodate = d.i()? as u32;
    t.tim_uptodate = d.i()? as u32;
    t.is_localtime = d.i()? as u32;
    t.zone_type = d.i()? as u32;
    t.tz_abbr = d.s()?;
    if let Some(name) = d.s()? {
        t.tz_info = tz::parse_tzfile(&name);
    }
    t.relative = dec_rel(d)?;
    Some(t)
}

pub fn encode_time(t: &Time) -> Vec<u8> {
    let mut e = Enc(Vec::with_capacity(256));
    enc_time(&mut e, t);
    e.0
}

pub fn decode_time(b: &[u8]) -> Option<Time> {
    dec_time(&mut Dec { b, p: 0 })
}

fn bytes_prop(o: &PhpObject, name: &[u8]) -> Option<Vec<u8>> {
    match o.get_property(name) {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// DateTime

pub fn date_get(o: &Rc<RefCell<PhpObject>>) -> Option<Time> {
    let b = bytes_prop(&o.borrow(), P_STATE)?;
    decode_time(&b)
}

/// "+05:00" style offset string (date_create_tz_offset_str)
pub fn tz_offset_str(offset: i64) -> String {
    let seconds = offset % 60;
    let sign = if offset < 0 { '-' } else { '+' };
    if seconds == 0 {
        format!(
            "{}{:02}:{:02}",
            sign,
            (offset / 3600).abs(),
            ((offset % 3600) / 60).abs()
        )
    } else {
        format!(
            "{}{:02}:{:02}:{:02}",
            sign,
            (offset / 3600).abs(),
            ((offset % 3600) / 60).abs(),
            seconds.abs()
        )
    }
}

/// The (date, timezone_type, timezone) property values for a time.
pub fn date_props(t: &Time) -> Vec<(&'static [u8], Value)> {
    let mut out = Vec::new();
    out.push((
        b"date" as &[u8],
        Value::String(PhpString::from_vec(crate::format::date_format(
            b"x-m-d H:i:s.u",
            t,
            t.is_localtime != 0,
        ))),
    ));
    if t.is_localtime != 0 {
        out.push((b"timezone_type", Value::Long(t.zone_type as i64)));
        let tzv = match t.zone_type {
            ZONETYPE_ID => t
                .tz_info
                .as_ref()
                .map(|z| z.name.clone())
                .unwrap_or_default(),
            ZONETYPE_OFFSET => tz_offset_str(t.z),
            ZONETYPE_ABBR => t.tz_abbr.clone().unwrap_or_default(),
            _ => String::new(),
        };
        out.push((b"timezone", Value::String(PhpString::from_string(tzv))));
    }
    out
}

fn reappend(o: &mut PhpObject, name: &[u8], v: Value) {
    o.remove_property(name);
    o.set_property(name.to_vec(), v);
}

pub fn date_set(o: &Rc<RefCell<PhpObject>>, t: &Time) {
    let props = date_props(t);
    let mut ob = o.borrow_mut();
    ob.set_property(P_SSE.to_vec(), Value::Long(t.sse));
    ob.set_property(b"__timestamp_us".to_vec(), Value::Long(t.us));
    ob.set_property(
        P_STATE.to_vec(),
        Value::String(PhpString::from_vec(encode_time(t))),
    );
    ob.remove_property(b"timezone_type");
    ob.remove_property(b"timezone");
    for (k, v) in props {
        reappend(&mut ob, k, v);
    }
}

// ---------------------------------------------------------------------------
// DateTimeZone

#[derive(Clone, Debug)]
pub struct TzObj {
    pub ztype: u32,
    pub utc_offset: i64,
    pub dst: i32,
    pub abbr: String,
    pub tz: Option<Rc<TzInfo>>,
}

impl TzObj {
    #[allow(clippy::inherent_to_string)] // not a Display impl; callers use it as a plain getter
    pub fn to_string(&self) -> String {
        match self.ztype {
            ZONETYPE_ID => self.tz.as_ref().map(|t| t.name.clone()).unwrap_or_default(),
            ZONETYPE_OFFSET => tz_offset_str(self.utc_offset),
            ZONETYPE_ABBR => self.abbr.clone(),
            _ => String::new(),
        }
    }
    /// set_timezone_from_timelib_time
    pub fn from_time(t: &Time) -> TzObj {
        match t.zone_type {
            ZONETYPE_ID => TzObj {
                ztype: ZONETYPE_ID,
                utc_offset: 0,
                dst: 0,
                abbr: String::new(),
                tz: t.tz_info.clone(),
            },
            ZONETYPE_OFFSET => TzObj {
                ztype: ZONETYPE_OFFSET,
                utc_offset: t.z,
                dst: 0,
                abbr: String::new(),
                tz: None,
            },
            _ => TzObj {
                ztype: t.zone_type,
                utc_offset: t.z,
                dst: t.dst,
                abbr: t.tz_abbr.clone().unwrap_or_default(),
                tz: None,
            },
        }
    }
}

pub fn tz_get(o: &Rc<RefCell<PhpObject>>) -> Option<TzObj> {
    let b = bytes_prop(&o.borrow(), P_TZ)?;
    let mut d = Dec { b: &b, p: 0 };
    let ztype = d.i()? as u32;
    let utc_offset = d.i()?;
    let dst = d.i()? as i32;
    let abbr = d.s()?.unwrap_or_default();
    let tz = match d.s()? {
        Some(n) => tz::parse_tzfile(&n),
        None => None,
    };
    Some(TzObj {
        ztype,
        utc_offset,
        dst,
        abbr,
        tz,
    })
}

pub fn tz_props(z: &TzObj) -> Vec<(&'static [u8], Value)> {
    vec![
        (b"timezone_type" as &[u8], Value::Long(z.ztype as i64)),
        (
            b"timezone",
            Value::String(PhpString::from_string(z.to_string())),
        ),
    ]
}

pub fn tz_set(o: &Rc<RefCell<PhpObject>>, z: &TzObj) {
    let mut e = Enc(Vec::new());
    e.i(z.ztype as i64);
    e.i(z.utc_offset);
    e.i(z.dst as i64);
    e.s(Some(&z.abbr));
    e.s(z.tz.as_ref().map(|t| t.name.as_str()));
    let mut ob = o.borrow_mut();
    ob.set_property(P_TZ.to_vec(), Value::String(PhpString::from_vec(e.0)));
    for (k, v) in tz_props(z) {
        reappend(&mut ob, k, v);
    }
}

// ---------------------------------------------------------------------------
// DateInterval

#[derive(Clone, Debug, Default)]
pub struct IntervalObj {
    pub diff: RelTime,
    pub civil_or_wall: i64,
    pub from_string: bool,
    pub date_string: Vec<u8>,
}

const IVAL_VISIBLE: [&[u8]; 10] = [
    b"y",
    b"m",
    b"d",
    b"h",
    b"i",
    b"s",
    b"f",
    b"invert",
    b"days",
    b"from_string",
];

pub fn interval_get(o: &Rc<RefCell<PhpObject>>) -> Option<IntervalObj> {
    let ob = o.borrow();
    let b = bytes_prop(&ob, P_IVAL)?;
    let mut d = Dec { b: &b, p: 0 };
    let mut diff = dec_rel(&mut d)?;
    let civil_or_wall = d.i()?;
    let from_string = d.i()? != 0;
    let date_string = d.s()?.unwrap_or_default().into_bytes();
    if !from_string {
        // Visible properties are writable and act on the interval.
        let gl = |name: &[u8], cur: i64| -> i64 {
            match ob.get_property(name) {
                Value::Undef => cur,
                v => v.to_long(),
            }
        };
        diff.y = gl(b"y", diff.y);
        diff.m = gl(b"m", diff.m);
        diff.d = gl(b"d", diff.d);
        diff.h = gl(b"h", diff.h);
        diff.i = gl(b"i", diff.i);
        diff.s = gl(b"s", diff.s);
        diff.invert = gl(b"invert", diff.invert as i64) as i32;
        match ob.get_property(b"f") {
            Value::Undef => {}
            v => {
                let fv = v.to_double();
                let f = fv * 1000000.0;
                diff.us = if f.is_finite() && f.abs() < 9.2e18 {
                    // The property holds us / 1e6 of the interval's integer
                    // microseconds: recover them exactly; a value written
                    // otherwise truncates (date_interval_write_property()).
                    let r = f.round();
                    if r / 1000000.0 == fv {
                        r as i64
                    } else {
                        f as i64
                    }
                } else {
                    0
                };
            }
        }
    }
    Some(IntervalObj {
        diff,
        civil_or_wall,
        from_string,
        date_string,
    })
}

pub fn interval_props(iv: &IntervalObj) -> Vec<(&'static [u8], Value)> {
    if iv.from_string {
        return vec![
            (b"from_string" as &[u8], Value::True),
            (
                b"date_string",
                Value::String(PhpString::from_bytes(&iv.date_string)),
            ),
        ];
    }
    let d = &iv.diff;
    vec![
        (b"y" as &[u8], Value::Long(d.y)),
        (b"m", Value::Long(d.m)),
        (b"d", Value::Long(d.d)),
        (b"h", Value::Long(d.h)),
        (b"i", Value::Long(d.i)),
        (b"s", Value::Long(d.s)),
        (b"f", Value::Double(d.us as f64 / 1000000.0)),
        (b"invert", Value::Long(d.invert as i64)),
        (
            b"days",
            if d.days != UNSET {
                Value::Long(d.days)
            } else {
                Value::False
            },
        ),
        (b"from_string", Value::False),
    ]
}

pub fn interval_set(o: &Rc<RefCell<PhpObject>>, iv: &IntervalObj) {
    let mut e = Enc(Vec::new());
    enc_rel(&mut e, &iv.diff);
    e.i(iv.civil_or_wall);
    e.i(iv.from_string as i64);
    e.s(Some(&String::from_utf8_lossy(&iv.date_string)));
    let mut ob = o.borrow_mut();
    ob.set_property(P_IVAL.to_vec(), Value::String(PhpString::from_vec(e.0)));
    for k in IVAL_VISIBLE.iter().chain([b"date_string" as &[u8]].iter()) {
        ob.remove_property(k);
    }
    for (k, v) in interval_props(iv) {
        ob.set_property(k.to_vec(), v);
    }
    if ob.native.is_none() {
        ob.native = Some(goro_core::object::NativeData {
            data: Rc::new(()),
            handlers: &IVAL_HANDLERS,
        });
    }
}

/// Set once the engine asked for the interval's property table
/// (date_object_get_properties_interval() fills the standard one): only
/// from then on are from_string / date_string readable as properties.
const P_IVAL_SEEN: &[u8] = b"__timestamp_ival_seen";

static IVAL_HANDLERS: goro_core::vm::NativeObjectHandlers = goro_core::vm::NativeObjectHandlers {
    get_properties: Some(interval_get_properties),
    clone_obj: Some(native_clone_obj),
    ..goro_core::vm::NativeObjectHandlers::EMPTY
};

fn interval_get_properties(
    _vm: &mut Vm,
    o: &Rc<RefCell<PhpObject>>,
    _purpose: u8,
) -> Option<PhpArray> {
    if o.borrow().has_property(P_IVAL) {
        o.borrow_mut().set_property(P_IVAL_SEEN.to_vec(), Value::True);
    }
    None
}

fn native_clone_obj(_vm: &mut Vm, src: &Rc<RefCell<PhpObject>>, dst: &Rc<RefCell<PhpObject>>) {
    let n = src.borrow().native.clone();
    dst.borrow_mut().native = n;
}

// ---------------------------------------------------------------------------
// DatePeriod

#[derive(Clone, Debug, Default)]
pub struct PeriodObj {
    pub start: Option<Time>,
    pub current: Option<Time>,
    pub end: Option<Time>,
    pub interval: Option<RelTime>,
    pub recurrences: i64,
    pub include_start_date: bool,
    pub include_end_date: bool,
    pub start_ce: Vec<u8>,
    pub initialized: bool,
}

fn enc_opt_time(e: &mut Enc, t: &Option<Time>) {
    match t {
        None => e.i(0),
        Some(t) => {
            e.i(1);
            enc_time(e, t);
        }
    }
}
fn dec_opt_time(d: &mut Dec) -> Option<Option<Time>> {
    if d.i()? == 0 {
        Some(None)
    } else {
        Some(Some(dec_time(d)?))
    }
}

pub fn period_get(o: &Rc<RefCell<PhpObject>>) -> Option<PeriodObj> {
    let b = bytes_prop(&o.borrow(), P_PERIOD)?;
    let mut d = Dec { b: &b, p: 0 };
    let start = dec_opt_time(&mut d)?;
    let current = dec_opt_time(&mut d)?;
    let end = dec_opt_time(&mut d)?;
    let interval = if d.i()? == 0 {
        None
    } else {
        Some(dec_rel(&mut d)?)
    };
    let recurrences = d.i()?;
    let include_start_date = d.i()? != 0;
    let include_end_date = d.i()? != 0;
    let start_ce = d.s()?.unwrap_or_default().into_bytes();
    let initialized = d.i()? != 0;
    Some(PeriodObj {
        start,
        current,
        end,
        interval,
        recurrences,
        include_start_date,
        include_end_date,
        start_ce,
        initialized,
    })
}

pub fn period_store(o: &Rc<RefCell<PhpObject>>, p: &PeriodObj) {
    let mut e = Enc(Vec::new());
    enc_opt_time(&mut e, &p.start);
    enc_opt_time(&mut e, &p.current);
    enc_opt_time(&mut e, &p.end);
    match &p.interval {
        None => e.i(0),
        Some(r) => {
            e.i(1);
            enc_rel(&mut e, r);
        }
    }
    e.i(p.recurrences);
    e.i(p.include_start_date as i64);
    e.i(p.include_end_date as i64);
    e.s(Some(&String::from_utf8_lossy(&p.start_ce)));
    e.i(p.initialized as i64);
    o.borrow_mut()
        .set_property(P_PERIOD.to_vec(), Value::String(PhpString::from_vec(e.0)));
}

/// create_date_period_datetime
pub fn make_date_obj(vm: &mut Vm, t: &Option<Time>, ce: &[u8]) -> Value {
    match t {
        None => Value::Null,
        Some(t) => {
            let o = new_object(vm, ce);
            date_set(&o, t);
            Value::Object(o)
        }
    }
}

pub fn make_interval_obj(vm: &mut Vm, r: &Option<RelTime>, civil_or_wall: i64) -> Value {
    match r {
        None => Value::Null,
        Some(r) => {
            let o = new_object(vm, b"DateInterval");
            interval_set(
                &o,
                &IntervalObj {
                    diff: r.clone(),
                    civil_or_wall,
                    from_string: false,
                    date_string: Vec::new(),
                },
            );
            Value::Object(o)
        }
    }
}

pub fn period_props(vm: &mut Vm, p: &PeriodObj) -> Vec<(&'static [u8], Value)> {
    PERIOD_PROPS
        .iter()
        .map(|k| (*k, period_prop(vm, p, k)))
        .collect()
}

/// One property of a DatePeriod (a fresh object for the object-valued ones).
pub fn period_prop(vm: &mut Vm, p: &PeriodObj, name: &[u8]) -> Value {
    let ce = || {
        if p.start_ce.is_empty() {
            b"DateTime".to_vec()
        } else {
            p.start_ce.clone()
        }
    };
    match name {
        b"start" => make_date_obj(vm, &p.start, &ce()),
        b"current" => make_date_obj(vm, &p.current, &ce()),
        b"end" => make_date_obj(vm, &p.end, &ce()),
        b"interval" => make_interval_obj(vm, &p.interval, 0),
        b"recurrences" => Value::Long(p.recurrences),
        b"include_start_date" => if p.include_start_date { Value::True } else { Value::False },
        _ => if p.include_end_date { Value::True } else { Value::False },
    }
}

/// The properties of a DatePeriod are no real ones: its property tables
/// (date_period_get_properties_for) get fresh start/current/end/interval
/// objects each time. The object only keeps placeholders, for the table's
/// order (they compare equal, as the uninitialized slots of php-src do).
pub fn period_set(_vm: &mut Vm, o: &Rc<RefCell<PhpObject>>, p: &PeriodObj) {
    period_store(o, p);
    let mut ob = o.borrow_mut();
    for k in PERIOD_PROPS {
        ob.set_property(k.to_vec(), Value::Null);
    }
    ob.native = Some(goro_core::object::NativeData {
        data: Rc::new(()),
        handlers: &PERIOD_HANDLERS,
    });
}

/// create_object of DatePeriod: its handlers from the start.
pub fn period_create(_vm: &mut Vm, o: &Rc<RefCell<PhpObject>>) {
    let mut ob = o.borrow_mut();
    if ob.native.is_none() {
        ob.native = Some(goro_core::object::NativeData {
            data: Rc::new(()),
            handlers: &PERIOD_HANDLERS,
        });
    }
}

/// date_period_has_property(): the fields of an uninitialized period are
/// not set (though they read as zero).
fn period_has_property(
    _vm: &mut Vm,
    o: &Rc<RefCell<PhpObject>>,
    name: &[u8],
    _check_empty: u8,
) -> Option<bool> {
    if PERIOD_PROPS.contains(&name) && !period_get(o).is_some_and(|p| p.initialized) {
        return Some(false);
    }
    None
}

static PERIOD_HANDLERS: goro_core::vm::NativeObjectHandlers = goro_core::vm::NativeObjectHandlers {
    get_properties: Some(period_get_properties),
    has_property: Some(period_has_property),
    clone_obj: Some(native_clone_obj),
    ..goro_core::vm::NativeObjectHandlers::EMPTY
};

fn period_get_properties(vm: &mut Vm, o: &Rc<RefCell<PhpObject>>, _purpose: u8) -> Option<PhpArray> {
    let mut a = vm.object_to_array(o).borrow().clone();
    if let Some(p) = period_get(o).filter(|p| p.initialized) {
        for (k, v) in period_props(vm, &p) {
            a.set(ArrayKey::String(PhpString::from_bytes(k)), v);
        }
    }
    Some(a)
}


// ---------------------------------------------------------------------------
// Misc helpers

pub fn arr_get<'a>(a: &'a PhpArray, k: &[u8]) -> Option<&'a Value> {
    a.get(&ArrayKey::String(PhpString::from_bytes(k)))
}

pub fn arr_set(a: &mut PhpArray, k: &str, v: Value) {
    a.set(ArrayKey::String(PhpString::from_string(k.to_string())), v);
}

pub fn new_arr() -> PhpArray {
    PhpArray::new()
}

pub fn arr_val(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

pub(crate) const PERIOD_PROPS: [&[u8]; 7] = [
    b"start",
    b"current",
    b"end",
    b"interval",
    b"recurrences",
    b"include_start_date",
    b"include_end_date",
];

/// DatePeriod's properties are read-only and cannot be unset.
pub fn prop_write(vm: &mut Vm, o: &Rc<RefCell<PhpObject>>, name: &[u8], kind: u8) -> bool {
    if base_of(vm, &o.borrow().class_name.clone()) != Some(Base::Period)
        || !PERIOD_PROPS.contains(&name)
    {
        return true;
    }
    let msg = if kind == 1 {
        format!(
            "Cannot unset {}::${}",
            class_display(vm, &o.borrow().class_name.clone()),
            goro_core::string::msg_str(name)
        )
    } else {
        format!(
            "Cannot modify readonly property DatePeriod::${}",
            goro_core::string::msg_str(name)
        )
    };
    let exc = vm.create_exception(b"Error", &msg, vm.current_line);
    vm.current_exception = Some(exc);
    false
}

/// Property reads: date fields of DateTime/DateTimeZone are not real
/// properties; DateInterval fields map onto the interval; DatePeriod fields
/// return fresh copies.
pub fn prop_read(vm: &mut Vm, o: &Rc<RefCell<PhpObject>>, name: &[u8]) -> Option<Option<Value>> {
    let cn = o.borrow().class_name.clone();
    match base_of(vm, &cn)? {
        Base::DateTime | Base::Immutable => {
            if matches!(name, b"date" | b"timezone_type" | b"timezone")
                && o.borrow().has_property(P_STATE)
            {
                return Some(None);
            }
            None
        }
        Base::TimeZone => {
            if matches!(name, b"timezone_type" | b"timezone") && o.borrow().has_property(P_TZ) {
                return Some(None);
            }
            None
        }
        Base::Interval => {
            let iv = interval_get(o)?;
            if matches!(name, b"from_string" | b"date_string")
                && !o.borrow().has_property(P_IVAL_SEEN)
            {
                return Some(None);
            }
            let d = &iv.diff;
            Some(Some(match name {
                b"y" => Value::Long(d.y),
                b"m" => Value::Long(d.m),
                b"d" => Value::Long(d.d),
                b"h" => Value::Long(d.h),
                b"i" => Value::Long(d.i),
                b"s" => Value::Long(d.s),
                b"f" => Value::Double(d.us as f64 / 1000000.0),
                b"invert" => Value::Long(d.invert as i64),
                b"days" => {
                    if d.days != UNSET {
                        Value::Long(d.days)
                    } else {
                        Value::False
                    }
                }
                _ => return None,
            }))
        }
        Base::Period => {
            if !PERIOD_PROPS.contains(&name) {
                return None;
            }
            // (an uninitialized period's fields are zero)
            let p = period_get(o).unwrap_or_default();
            Some(Some(period_prop(vm, &p, name)))
        }
    }
}

/// Side effects of the date classes' compare handlers.
pub fn compare_check(vm: &mut Vm, a: &Value, b: &Value) {
    let (oa, ob) = match (a, b) {
        (Value::Object(x), Value::Object(y)) => (x.clone(), y.clone()),
        _ => return,
    };
    let ba = obj_base(vm, &oa);
    let bb = obj_base(vm, &ob);
    let is_date = |b: Option<Base>| matches!(b, Some(Base::DateTime) | Some(Base::Immutable));
    let err = |vm: &mut Vm, class: &[u8], msg: &str| {
        let exc = vm.create_exception(class, msg, vm.current_line);
        vm.current_exception = Some(exc);
    };
    if is_date(ba) && is_date(bb) {
        if date_get(&oa).is_none() || date_get(&ob).is_none() {
            err(
                vm,
                b"DateObjectError",
                "Trying to compare an incomplete DateTime or DateTimeImmutable object",
            );
        }
    } else if ba == Some(Base::TimeZone) && bb == Some(Base::TimeZone) {
        match (tz_get(&oa), tz_get(&ob)) {
            (Some(x), Some(y)) => {
                if x.ztype != y.ztype {
                    err(
                        vm,
                        b"DateException",
                        "Cannot compare two different kinds of DateTimeZone objects",
                    );
                }
            }
            _ => err(
                vm,
                b"DateObjectError",
                "Trying to compare uninitialized DateTimeZone objects",
            ),
        }
    } else if ba == Some(Base::Interval) && bb == Some(Base::Interval) {
        vm.emit_warning("Cannot compare DateInterval objects");
    }
}

/// Keep the engine-provided properties after user-added ones, mirroring the
/// order PHP's get_properties_for handlers produce.
pub fn prop_added(o: &Rc<RefCell<PhpObject>>) {
    // date_interval_write_property(): y/m/d/h/i/s/invert are stored as
    // integers, f as a float; days (and from_string) keep the interval's.
    let is_interval = o.borrow().has_property(P_IVAL);
    if is_interval {
        if let Some(iv) = interval_get(o) {
            if !iv.from_string {
                interval_set(o, &iv);
                return;
            }
        }
    }
    let mut ob = o.borrow_mut();
    let names: &[&[u8]] = if ob.has_property(P_STATE) {
        &[b"date", b"timezone_type", b"timezone"]
    } else if ob.has_property(P_TZ) {
        &[b"timezone_type", b"timezone"]
    } else if ob.has_property(P_IVAL) {
        &[
            b"y",
            b"m",
            b"d",
            b"h",
            b"i",
            b"s",
            b"f",
            b"invert",
            b"days",
            b"from_string",
            b"date_string",
        ]
    } else if ob.has_property(P_PERIOD) {
        &[
            b"start",
            b"current",
            b"end",
            b"interval",
            b"recurrences",
            b"include_start_date",
            b"include_end_date",
        ]
    } else {
        return;
    };
    for n in names {
        if let Some(pos) = ob.properties.iter().position(|(k, _)| k.as_slice() == *n) {
            let e = ob.properties.remove(pos);
            ob.properties.push(e);
        }
    }
}

/// The clone's state while a user __clone() runs: date_object_clone_*()
/// copy it after zend_objects_clone_members(), so the clone looks
/// uninitialized to __clone(). `before`: set it aside; else put it back.
pub fn clone_hold(o: &Rc<RefCell<PhpObject>>, before: bool) {
    const P_HELD: &[u8] = b"__timestamp_clone_held";
    let mut ob = o.borrow_mut();
    if before {
        let is_ival = ob.has_property(P_IVAL);
        let keys: Vec<Vec<u8>> = ob
            .properties
            .iter()
            .map(|(k, _)| k.clone())
            .filter(|k| {
                k.starts_with(b"__timestamp")
                    || goro_core::foreach_obj::is_date_view_prop(&ob, k)
                    || is_ival
                        && (IVAL_VISIBLE.contains(&k.as_slice()) || k.as_slice() == b"date_string")
            })
            .collect();
        let mut held = PhpArray::new();
        for k in keys {
            let v = ob.get_property(&k);
            ob.remove_property(&k);
            held.set(ArrayKey::String(PhpString::from_vec(k)), v);
        }
        ob.set_property(P_HELD.to_vec(), arr_val(held));
    } else if let Value::Array(a) = ob.get_property(P_HELD) {
        ob.remove_property(P_HELD);
        for (k, v) in a.borrow().iter() {
            if let ArrayKey::String(k) = k {
                ob.set_property(k.as_bytes().to_vec(), v.clone());
            }
        }
    }
}
