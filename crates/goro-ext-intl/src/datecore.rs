//! The calendar / time zone core shared by IntlCalendar, IntlTimeZone
//! (calendar.rs, timezone.rs) and IntlDateFormatter, MessageFormatter
//! (dateformat.rs, msgformat.rs): ICU's Calendar / TimeZone services.
//!
//! This is the contract between those modules. The engine lives in the
//! submodules: [`zone`] (TimeZone: system zones over ext/date's timezonedb,
//! custom zones, ICU metadata), [`cal`] (a port of ICU's Calendar /
//! GregorianCalendar field engine and the other calendar systems) and
//! [`names`] (TimeZone::getDisplayName()).

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::PhpArray;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::Obj;
use crate::error::{self, IntlError, U_ILLEGAL_ARGUMENT_ERROR};

pub mod cal;
pub(crate) mod data;
pub mod islamic;
pub mod names;
pub mod zone;

pub use cal::{CalType, Calendar, WeekData};
pub use zone::Zone;

/// ICU calendar field indices (UCalendarDateFields / IntlCalendar::FIELD_*).
pub const ERA: usize = 0;
pub const YEAR: usize = 1;
pub const MONTH: usize = 2;
pub const WEEK_OF_YEAR: usize = 3;
pub const WEEK_OF_MONTH: usize = 4;
pub const DATE: usize = 5;
pub const DAY_OF_YEAR: usize = 6;
pub const DAY_OF_WEEK: usize = 7;
pub const DAY_OF_WEEK_IN_MONTH: usize = 8;
pub const AM_PM: usize = 9;
pub const HOUR: usize = 10;
pub const HOUR_OF_DAY: usize = 11;
pub const MINUTE: usize = 12;
pub const SECOND: usize = 13;
pub const MILLISECOND: usize = 14;
pub const ZONE_OFFSET: usize = 15;
pub const DST_OFFSET: usize = 16;
pub const YEAR_WOY: usize = 17;
pub const DOW_LOCAL: usize = 18;
pub const EXTENDED_YEAR: usize = 19;
pub const JULIAN_DAY: usize = 20;
pub const MILLISECONDS_IN_DAY: usize = 21;
pub const IS_LEAP_MONTH: usize = 22;
pub const FIELD_COUNT: usize = 23;

/// A calendar's settings and instant (what an IntlCalendar object holds,
/// and what IntlDateFormatter formats with).
#[derive(Clone, Debug)]
pub struct CalInfo {
    /// UDate: milliseconds since the epoch.
    pub millis: f64,
    /// Time zone ID ("UTC", "Europe/Lisbon", "GMT+01:00").
    pub tz_id: String,
    /// ICU calendar type ("gregorian", "japanese", "buddhist"...).
    pub cal_type: String,
    /// The locale the calendar was created for.
    pub locale: String,
    /// UCAL_SUNDAY = 1 .. UCAL_SATURDAY = 7
    pub first_day_of_week: i32,
    pub minimal_days_in_first_week: i32,
    pub lenient: bool,
    /// Gregorian change date (UDate), for gregorian calendars.
    pub gregorian_change: f64,
}

impl CalInfo {
    /// A gregorian calendar in `tz_id` for `locale` at `millis`.
    pub fn gregorian(millis: f64, tz_id: &str, locale: &str) -> CalInfo {
        let w = week_data(locale);
        CalInfo {
            millis,
            tz_id: tz_id.to_string(),
            cal_type: "gregorian".to_string(),
            locale: locale.to_string(),
            first_day_of_week: w.first_day,
            minimal_days_in_first_week: w.min_days,
            lenient: true,
            gregorian_change: -12219292800000.0,
        }
    }

    /// Calendar::createInstance(zone, locale) at `millis`: the calendar
    /// type and week data of the locale.
    pub fn for_locale(millis: f64, tz_id: &str, locale: &str) -> CalInfo {
        let mut i = CalInfo::gregorian(millis, tz_id, locale);
        i.cal_type = locale_cal_type(locale);
        if i.cal_type == "iso8601" {
            i.first_day_of_week = 2;
            i.minimal_days_in_first_week = 4;
        }
        i
    }

    /// The ICU calendar these settings describe, its time set.
    pub fn to_calendar(&self) -> Calendar {
        let mut c = new_calendar_raw(
            &self.cal_type,
            zone::create_or_unknown(&self.tz_id),
            &self.locale,
        );
        c.first_day = self.first_day_of_week;
        c.min_days = self.minimal_days_in_first_week;
        c.lenient = self.lenient;
        if self.gregorian_change != c.cutover {
            let _ = c.set_gregorian_change(self.gregorian_change);
        }
        let _ = c.set_time(self.millis);
        c
    }

    /// The settings and instant of a calendar (pending fields resolved).
    pub fn from_calendar(c: &mut Calendar) -> CalInfo {
        let millis = c.get_time().unwrap_or(f64::NAN);
        CalInfo {
            millis,
            tz_id: c.zone.id.clone(),
            cal_type: c.type_name().to_string(),
            locale: c.locale.clone(),
            first_day_of_week: c.first_day,
            minimal_days_in_first_week: c.min_days,
            lenient: c.lenient,
            gregorian_change: c.cutover,
        }
    }
}

/// The fields of `info` at its instant, indexed by the constants above.
pub fn compute_fields(info: &CalInfo) -> [i32; FIELD_COUNT] {
    let mut c = info.to_calendar();
    let mut f = [0i32; FIELD_COUNT];
    if c.complete().is_ok() {
        for (i, v) in f.iter_mut().enumerate() {
            *v = c.raw_field(i);
        }
    }
    f
}

/// The instant of local fields (EXTENDED_YEAR, MONTH, DATE, HOUR_OF_DAY,
/// MINUTE, SECOND, MILLISECOND) in `info`'s calendar and zone.
pub fn fields_to_millis(info: &CalInfo, f: &[i32; FIELD_COUNT]) -> f64 {
    let mut c = info.to_calendar();
    c.clear_all();
    for i in [
        EXTENDED_YEAR,
        MONTH,
        DATE,
        HOUR_OF_DAY,
        MINUTE,
        SECOND,
        MILLISECOND,
    ] {
        c.set(i, f[i]);
    }
    c.get_time().unwrap_or(f64::NAN)
}

/// The raw and DST offsets (ms) of zone `tz_id` at `millis` (UTC).
pub fn tz_offsets(tz_id: &str, millis: f64) -> (i32, i32) {
    zone::create_or_unknown(tz_id)
        .offsets(millis)
        .unwrap_or((0, 0))
}

/// ICU's canonical ID of a time zone ID ("GMT+1" -> "GMT+01:00", links
/// resolved, case fixed), None when ICU does not know it.
pub fn canonical_tz_id(id: &str) -> Option<String> {
    zone::canonical_id(id).map(|(c, _)| c)
}

/// The default time zone ID (date.timezone / date_default_timezone_get()).
pub fn default_tz_id(vm: &Vm) -> String {
    // date_default_timezone_set() (ext/date keeps it there)
    if let Some(v) = vm.constants.get(b"__default_timezone".as_slice()) {
        let s = v.to_php_string();
        if !s.as_bytes().is_empty() {
            return s.to_string_lossy();
        }
    }
    match vm.constants.get(b"date.timezone".as_slice()) {
        Some(v) if !v.to_php_string().as_bytes().is_empty() => v.to_php_string().to_string_lossy(),
        _ => "UTC".to_string(),
    }
}

/// get_timezone_info()->name: PHP's default time zone (date.timezone or
/// date_default_timezone_set()).
pub fn php_default_tz(vm: &mut Vm) -> String {
    let had_exc = vm.current_exception.is_some();
    let r = vm.call_callback(&crate::s("date_default_timezone_get"), &[]);
    if !had_exc && vm.current_exception.is_some() {
        vm.current_exception = None;
    }
    match r {
        Ok(Value::String(s)) if !s.as_bytes().is_empty() => s.to_string_lossy(),
        _ => default_tz_id(vm),
    }
}

/// TimeZone::getDisplayName(): `style` is IntlTimeZone::DISPLAY_*.
pub fn tz_display_name(tz_id: &str, daylight: bool, style: i64, locale: &str) -> String {
    let z = zone::create_or_unknown(tz_id);
    names::display_name(&z, daylight, style, locale)
}

/// timezone_process_timezone_argument(): the time zone ID a `$timezone`
/// argument designates (null: the default; IntlTimeZone; DateTimeZone; a
/// string ID). Err(()) when the argument is invalid: the error has been
/// set (or an exception thrown) as php-src does, with `func` the active
/// function name.
#[allow(clippy::result_unit_err)]
pub fn tz_from_arg(vm: &mut Vm, v: &Value, func: &str) -> Result<String, ()> {
    zone_from_arg(vm, v, func, None).map(|z| z.id.clone())
}

/// A new IntlTimeZone object for `tz_id`.
pub fn new_tz_object(vm: &mut Vm, tz_id: &str) -> Option<Obj> {
    let z = zone::create_or_unknown(tz_id);
    new_tz_object_zone(vm, z)
}

/// The time zone ID of an IntlTimeZone object.
pub fn tz_object_id(o: &Obj) -> Option<String> {
    tz_object_zone(o).map(|z| z.id.clone())
}

/// A new IntlCalendar object (class from `info.cal_type`: IntlGregorianCalendar
/// for gregorian) holding `info`.
pub fn new_cal_object(vm: &mut Vm, info: &CalInfo) -> Option<Obj> {
    new_cal_object_from(vm, info.to_calendar())
}

/// The settings and instant of an IntlCalendar object (None if not an
/// initialized IntlCalendar).
pub fn cal_object_info(o: &Obj) -> Option<CalInfo> {
    let st = crate::obj::state::<CalState>(o)?;
    let mut b = st.borrow_mut();
    let c = b.cal.as_mut()?;
    Some(CalInfo::from_calendar(c))
}

/// The default calendar type of a locale ("gregorian", or the
/// @calendar=... keyword, or the region's preferred calendar).
pub fn locale_cal_type(locale: &str) -> String {
    if let Some(k) = locale_keyword(locale, "calendar") {
        let k = k.to_ascii_lowercase();
        if let Some(t) = CalType::from_name(&k) {
            return t.name().to_string();
        }
    }
    let region = locale_region(locale);
    let prefs = data::CAL_PREFS
        .iter()
        .find(|(r, _)| *r == region)
        .or_else(|| data::CAL_PREFS.iter().find(|(r, _)| *r == "001"));
    match prefs.and_then(|(_, p)| p.first()) {
        Some(p) => p.to_string(),
        None => "gregorian".to_string(),
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The proleptic Gregorian date of days since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ---------------------------------------------------------------------------
// Object states

/// The native state of an IntlTimeZone object (TimeZone_object).
#[derive(Default)]
pub struct TzState {
    pub zone: Option<Rc<Zone>>,
    pub err: IntlError,
}

/// The native state of an IntlCalendar object (Calendar_object).
#[derive(Default)]
pub struct CalState {
    pub cal: Option<Calendar>,
    pub err: IntlError,
}

/// The zone of an IntlTimeZone object (None: not constructed).
pub fn tz_object_zone(o: &Obj) -> Option<Rc<Zone>> {
    crate::obj::state::<TzState>(o)?.borrow().zone.clone()
}

/// timezone_object_construct(): a new IntlTimeZone object for `z`.
pub fn new_tz_object_zone(vm: &mut Vm, z: Rc<Zone>) -> Option<Obj> {
    let o = crate::obj::object_init_ex(vm, b"intltimezone")?;
    if let Some(st) = crate::obj::state::<TzState>(&o) {
        st.borrow_mut().zone = Some(z);
    }
    Some(o)
}

/// calendar_object_create(): a new IntlCalendar (IntlGregorianCalendar
/// for the gregorian type) object holding `c`.
pub fn new_cal_object_from(vm: &mut Vm, c: Calendar) -> Option<Obj> {
    let class: &[u8] = if c.cal_type == CalType::Gregorian {
        b"intlgregoriancalendar"
    } else {
        b"intlcalendar"
    };
    let o = new_object_no_ctor_check(vm, class)?;
    if let Some(st) = crate::obj::state::<CalState>(&o) {
        st.borrow_mut().cal = Some(c);
    }
    Some(o)
}

/// object_init_ex() of an abstract-less intl class (IntlCalendar is not
/// abstract, its constructor is private).
fn new_object_no_ctor_check(vm: &mut Vm, class: &[u8]) -> Option<Obj> {
    crate::obj::object_init_ex(vm, class)
}

// ---------------------------------------------------------------------------
// Locales

/// The value of keyword `key` of an ICU locale ID ("@calendar=...;...").
pub fn locale_keyword(locale: &str, key: &str) -> Option<String> {
    let (_, kw) = locale.split_once('@')?;
    for part in kw.split(';') {
        if let Some((k, v)) = part.split_once('=') {
            if k.trim().eq_ignore_ascii_case(key) {
                return Some(v.trim().to_string());
            }
        }
    }
    // BCP 47 -u-ca-
    None
}

/// The base name of a locale (keywords dropped, "-" -> "_").
pub fn locale_base(locale: &str) -> String {
    let base = locale.split('@').next().unwrap_or("");
    let base = base.split(".").next().unwrap_or("");
    let mut out = Vec::new();
    for (i, p) in base.split(['_', '-']).enumerate() {
        if p.is_empty() {
            continue;
        }
        if i == 0 {
            out.push(p.to_ascii_lowercase());
        } else if p.len() == 4 && p.chars().all(|c| c.is_ascii_alphabetic()) {
            let mut s = p.to_ascii_lowercase();
            s[..1].make_ascii_uppercase();
            out.push(s);
        } else {
            out.push(p.to_ascii_uppercase());
        }
    }
    out.join("_")
}

/// The region of a locale for supplemental data: its region subtag, or
/// the likely region of its language.
pub fn locale_region(locale: &str) -> String {
    if let Some(rg) = locale_keyword(locale, "rg") {
        if rg.len() >= 2 {
            return rg[..2].to_ascii_uppercase();
        }
    }
    let base = locale_base(locale);
    let parts: Vec<&str> = base.split('_').collect();
    for p in parts.iter().skip(1) {
        if p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic())
            || p.len() == 3 && p.chars().all(|c| c.is_ascii_digit())
        {
            return p.to_string();
        }
    }
    let tag = base.replace('_', "-");
    let tag = if tag.is_empty() {
        "und".to_string()
    } else {
        tag
    };
    if let Ok(l) = intl::locale::Locale::parse(&tag) {
        if let Some(r) = l.maximize().region {
            return r;
        }
    }
    "001".to_string()
}

/// The week data of a locale's region.
pub fn week_data(locale: &str) -> WeekData {
    let region = locale_region(locale);
    let row = data::WEEK_DATA
        .iter()
        .find(|r| r.0 == region)
        .or_else(|| data::WEEK_DATA.iter().find(|r| r.0 == "001"))
        .copied()
        .unwrap_or(("001", 1, 1, 7, 0, 1, 86_400_000));
    let mut w = WeekData {
        first_day: row.1 as i32,
        min_days: row.2 as i32,
        onset: row.3 as i32,
        onset_ms: row.4,
        cease: row.5 as i32,
        cease_ms: row.6,
    };
    if let Some(fw) = locale_keyword(locale, "fw") {
        let d = match fw.to_ascii_lowercase().as_str() {
            "sun" => 1,
            "mon" => 2,
            "tue" => 3,
            "wed" => 4,
            "thu" => 5,
            "fri" => 6,
            "sat" => 7,
            _ => 0,
        };
        if d != 0 {
            w.first_day = d;
        }
    }
    w
}

/// Calendar::getLocale(): (actual, valid) locales of a calendar created
/// for `locale`.
pub fn calendar_locales(locale: &str) -> (String, String) {
    let base = locale_base(locale);
    let of = |l: &str| -> Option<(String, String)> {
        if let Some(r) = data::CAL_LOCALES.iter().find(|r| r.0 == l) {
            return Some((r.1.to_string(), r.2.to_string()));
        }
        if data::AVAILABLE_LOCALES.contains(&l) {
            let lang = l.split('_').next().unwrap_or("").to_string();
            return Some((lang, l.to_string()));
        }
        None
    };
    if let Some(r) = of(&base) {
        return r;
    }
    // the longest available parent
    let mut cur = base.as_str();
    while let Some(p) = cur.rfind('_') {
        cur = &cur[..p];
        if let Some((actual, valid)) = of(cur) {
            let has_region = base.split('_').skip(1).any(|p| {
                p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic())
                    || p.len() == 3 && p.chars().all(|c| c.is_ascii_digit())
            });
            return (actual, if has_region { base.clone() } else { valid });
        }
    }
    ("en".to_string(), "en_US".to_string())
}

/// A calendar of type `cal_type` (a CLDR calendar name) for `locale` in
/// zone `z`, fields cleared, time not set.
pub fn new_calendar_raw(cal_type: &str, z: Rc<Zone>, locale: &str) -> Calendar {
    let t = CalType::from_name(cal_type).unwrap_or(CalType::Gregorian);
    Calendar::new(t, z, locale, week_data(locale))
}

/// Calendar::createInstance(zone, locale): the locale's calendar, at now.
pub fn create_calendar(z: Rc<Zone>, locale: &str) -> Calendar {
    let mut c = new_calendar_raw(&locale_cal_type(locale), z, locale);
    let _ = c.set_time(zone::now_millis());
    c
}

// ---------------------------------------------------------------------------
// ext/date interoperation (common_date.cpp)

/// $o->method(...args): None when it threw.
pub fn call_method(vm: &mut Vm, o: &Obj, name: &str, args: &[Value]) -> Option<Value> {
    let mut a = PhpArray::new();
    a.push(Value::Object(o.clone()));
    a.push(crate::s(name));
    let cb = crate::arr(a);
    let r = vm.call_callback(&cb, args);
    if vm.current_exception.is_some() {
        return None;
    }
    r.ok()
}

/// Report an error to `err` (and the global error) or to the global error
/// only.
pub fn report(vm: &mut Vm, func: &str, err: Option<&mut IntlError>, code: i32, msg: &str) {
    match err {
        Some(e) => error::set_both(vm, func, e, code, msg),
        None => error::set_global(vm, func, code, msg),
    }
}

/// The time zone ID of a DateTimeZone name as timezone_convert_datetimezone()
/// sees it: an offset type ("+hh:mm") becomes "GMT+hh:mm".
fn dtz_name_to_icu_id(name: &str) -> Result<String, &'static str> {
    let b = name.as_bytes();
    if !b.is_empty() && (b[0] == b'+' || b[0] == b'-') {
        let neg = b[0] == b'-';
        let parts: Vec<i64> = name[1..]
            .split(':')
            .map(|p| p.parse::<i64>().unwrap_or(0))
            .collect();
        let secs = parts.first().copied().unwrap_or(0) * 3600
            + parts.get(1).copied().unwrap_or(0) * 60
            + parts.get(2).copied().unwrap_or(0);
        let secs = if neg { -secs } else { secs };
        let offset_mins = secs / 60;
        let hours = offset_mins / 60;
        let mut minutes = offset_mins - hours * 60;
        if minutes < 0 {
            minutes = -minutes;
        }
        if offset_mins <= -24 * 60 || offset_mins >= 24 * 60 {
            return Err("object has an time zone offset that's too large");
        }
        return Ok(format!(
            "GMT{}{:02}:{:02}",
            if hours < 0 { '-' } else { '+' },
            hours.abs(),
            minutes
        ));
    }
    Ok(name.to_string())
}

/// timezone_convert_datetimezone() of a DateTimeZone object.
pub fn datetimezone_to_zone(
    vm: &mut Vm,
    dtz: &Obj,
    func: &str,
    err: Option<&mut IntlError>,
) -> Result<Rc<Zone>, ()> {
    let name = match call_method(vm, dtz, "getName", &[]) {
        Some(Value::String(s)) => s.to_string_lossy(),
        _ => return Err(()),
    };
    zone_from_dtz_name(vm, &name, func, err)
}

fn zone_from_dtz_name(
    vm: &mut Vm,
    name: &str,
    func: &str,
    err: Option<&mut IntlError>,
) -> Result<Rc<Zone>, ()> {
    let id = match dtz_name_to_icu_id(name) {
        Ok(id) => id,
        Err(m) => {
            report(vm, func, err, U_ILLEGAL_ARGUMENT_ERROR, m);
            return Err(());
        }
    };
    match zone::create(&id) {
        Some(z) => Ok(z),
        None => {
            let m = format!(
                "time zone id '{}' extracted from ext/date DateTimeZone not recognized",
                id
            );
            report(vm, func, err, U_ILLEGAL_ARGUMENT_ERROR, &m);
            Err(())
        }
    }
}

fn throw_intl_exception(vm: &mut Vm, msg: &str) {
    if vm.current_exception.is_some() {
        return;
    }
    let e = vm.create_exception(b"IntlException", msg, vm.current_line);
    vm.current_exception = Some(e);
}

/// timezone_process_timezone_argument()
pub fn zone_from_arg(
    vm: &mut Vm,
    v: &Value,
    func: &str,
    err: Option<&mut IntlError>,
) -> Result<Rc<Zone>, ()> {
    let id = match v.deref() {
        Value::Null | Value::Undef => php_default_tz(vm),
        Value::Object(o) => {
            if crate::zpp::obj_instanceof(vm, &o, "IntlTimeZone") {
                return match tz_object_zone(&o) {
                    Some(z) => Ok(z),
                    None => {
                        throw_intl_exception(
                            vm,
                            &format!(
                                "{}(): passed IntlTimeZone is not properly constructed",
                                func
                            ),
                        );
                        Err(())
                    }
                };
            }
            if crate::zpp::obj_instanceof(vm, &o, "DateTimeZone") {
                return datetimezone_to_zone(vm, &o, func, err);
            }
            match vm.zpp_coerce_scalar(
                goro_core::arginfo::T_STRING,
                &Value::Object(o.clone()),
                false,
                vm.current_line,
            ) {
                Ok(Some(s)) => s.to_php_string().to_string_lossy(),
                _ => {
                    if vm.current_exception.is_none() {
                        let cn = goro_core::value::display_class_name(&o.borrow().class_name);
                        crate::zpp::throw(
                            vm,
                            b"Error",
                            &format!("Object of class {} could not be converted to string", cn),
                        );
                    }
                    return Err(());
                }
            }
        }
        other => {
            let s = other.to_php_string();
            if !crate::common::utf8_valid(s.as_bytes()) {
                throw_intl_exception(
                    vm,
                    &format!(
                        "{}(): Time zone identifier given is not a valid UTF-8 string",
                        func
                    ),
                );
                return Err(());
            }
            s.to_string_lossy()
        }
    };
    match zone::create(&id) {
        Some(z) => Ok(z),
        None => {
            throw_intl_exception(vm, &format!("{}(): No such time zone: \"{}\"", func, id));
            Err(())
        }
    }
}

/// timezone_convert_to_datetimezone(): a DateTimeZone object for `z`.
/// Err(()) with an exception thrown / error set.
pub fn zone_to_datetimezone(
    vm: &mut Vm,
    z: &Zone,
    func: &str,
    err: Option<&mut IntlError>,
) -> Result<Value, ()> {
    let _ = (func, &err);
    let arg = if z.id.starts_with("GMT") {
        let off = z.raw_offset() / 1000;
        let a = off.abs();
        let mut s = format!(
            "{}{:02}:{:02}",
            if off < 0 { '-' } else { '+' },
            a / 3600,
            a / 60 % 60
        );
        if a % 60 != 0 {
            s.push_str(&format!(":{:02}", a % 60));
        }
        s
    } else {
        z.id.clone()
    };
    match construct_native(vm, b"datetimezone", &[crate::s(arg)]) {
        Ok(o) => Ok(Value::Object(o)),
        Err(()) => {
            if let Some(prev) = vm.current_exception.take() {
                let e = vm.create_exception(
                    b"IntlException",
                    "DateTimeZone constructor threw exception",
                    vm.current_line,
                );
                if let Value::Object(eo) = &e {
                    eo.borrow_mut().set_property(b"previous".to_vec(), prev);
                }
                vm.current_exception = Some(e);
            }
            Err(())
        }
    }
}

/// `new Class(...args)` of a native (ext/date) class.
pub fn construct_native(vm: &mut Vm, class_lower: &[u8], args: &[Value]) -> Result<Obj, ()> {
    let o = crate::obj::object_init_ex(vm, class_lower).ok_or(())?;
    let Some(h) = vm.object_construct_handlers.get(class_lower).copied() else {
        return Ok(o);
    };
    let mut a = Vec::with_capacity(args.len() + 1);
    a.push(Value::Object(o.clone()));
    a.extend_from_slice(args);
    let line = vm.current_line;
    if h(vm, &a, line).is_err() || vm.current_exception.is_some() {
        return Err(());
    }
    Ok(o)
}

fn call_method_keep_exc(vm: &mut Vm, o: &Obj, name: &str, args: &[Value]) -> Option<Value> {
    let mut a = PhpArray::new();
    a.push(Value::Object(o.clone()));
    a.push(crate::s(name));
    vm.call_callback(&crate::arr(a), args).ok()
}

/// intl_datetime_decompose(): the instant (ms) and the time zone of a
/// DateTimeInterface object. Err: (code, message) to report.
pub fn datetime_decompose(
    vm: &mut Vm,
    o: &Obj,
) -> Result<(f64, Option<Rc<Zone>>), Option<(i32, String)>> {
    let ts = match call_method_keep_exc(vm, o, "getTimestamp", &[]) {
        Some(Value::Long(n)) => n,
        _ => return Err(None),
    };
    if vm.current_exception.is_some() {
        return Err(None);
    }
    let us = match call_method(vm, o, "format", &[crate::s("u")]) {
        Some(v) => v
            .to_php_string()
            .to_string_lossy()
            .parse::<i64>()
            .unwrap_or(0),
        None => 0,
    };
    let millis = 1000.0 * ts as f64 + (us / 1000) as f64;
    let tz = match call_method(vm, o, "getTimezone", &[]) {
        Some(Value::Object(t)) => match call_method(vm, &t, "getName", &[]) {
            Some(Value::String(s)) => s.to_string_lossy(),
            _ => return Err(None),
        },
        _ => return Ok((millis, Some(zone::gmt()))),
    };
    let id = match dtz_name_to_icu_id(&tz) {
        Ok(id) => id,
        Err(_) => {
            return Err(Some((
                U_ILLEGAL_ARGUMENT_ERROR,
                "could not convert DateTime's time zone".to_string(),
            )));
        }
    };
    match zone::create(&id) {
        Some(z) => Ok((millis, Some(z))),
        None => Err(Some((
            U_ILLEGAL_ARGUMENT_ERROR,
            "could not convert DateTime's time zone".to_string(),
        ))),
    }
}

/// The time zone of a DateTime object as intlcal_from_date_time() converts
/// it (errors reported with `func`).
pub fn datetime_zone(vm: &mut Vm, o: &Obj, func: &str) -> Result<Rc<Zone>, ()> {
    let name = match call_method(vm, o, "getTimezone", &[]) {
        Some(Value::Object(t)) => match call_method(vm, &t, "getName", &[]) {
            Some(Value::String(s)) => s.to_string_lossy(),
            _ => return Err(()),
        },
        _ => return Ok(zone::gmt()),
    };
    zone_from_dtz_name(vm, &name, func, None)
}

/// A cell holding a calendar state, for the formatters.
pub type CalCell = Rc<RefCell<CalState>>;

/// intl_zval_to_millis(): the UDate a date argument designates (a number
/// of seconds, a numeric string, a DateTimeInterface or an IntlCalendar),
/// NaN with the error reported to `err` (and the global error) otherwise.
pub fn zval_to_millis(vm: &mut Vm, v: &Value, func: &str, err: Option<&mut IntlError>) -> f64 {
    match v.deref() {
        Value::Long(n) => 1000.0 * n as f64,
        Value::Double(d) => 1000.0 * d,
        Value::String(s) => match goro_core::value::parse_numeric_str(s.as_bytes(), false) {
            Some((goro_core::value::NumStr::Long(n), _)) => 1000.0 * n as f64,
            Some((goro_core::value::NumStr::Double(d), _)) => 1000.0 * d,
            None => {
                let m = format!(
                    "string '{}' is not numeric, which would be required for it to be a valid date",
                    s.to_string_lossy()
                );
                report(vm, func, err, U_ILLEGAL_ARGUMENT_ERROR, &m);
                f64::NAN
            }
        },
        Value::Object(o) => {
            if crate::zpp::obj_instanceof(vm, &o, "DateTimeInterface") {
                match datetime_decompose(vm, &o) {
                    Ok((ms, _)) => ms,
                    Err(Some((code, m))) => {
                        report(vm, func, err, code, &m);
                        f64::NAN
                    }
                    Err(None) => f64::NAN,
                }
            } else if crate::zpp::obj_instanceof(vm, &o, "IntlCalendar") {
                let t = crate::obj::state::<CalState>(&o)
                    .and_then(|st| st.borrow_mut().cal.as_mut().map(|c| c.get_time()));
                match t {
                    None => {
                        report(
                            vm,
                            func,
                            err,
                            U_ILLEGAL_ARGUMENT_ERROR,
                            "IntlCalendar object is not properly constructed",
                        );
                        f64::NAN
                    }
                    Some(Ok(t)) => t,
                    Some(Err(())) => {
                        report(
                            vm,
                            func,
                            err,
                            U_ILLEGAL_ARGUMENT_ERROR,
                            "call to internal Calendar::getTime() has failed",
                        );
                        f64::NAN
                    }
                }
            } else {
                report(
                    vm,
                    func,
                    err,
                    U_ILLEGAL_ARGUMENT_ERROR,
                    "invalid object type for date/time (only IntlCalendar and DateTimeInterface permitted)",
                );
                f64::NAN
            }
        }
        _ => {
            report(
                vm,
                func,
                err,
                U_ILLEGAL_ARGUMENT_ERROR,
                "invalid PHP type for date",
            );
            f64::NAN
        }
    }
}

/// "Call to private Class::__construct() from ..." when `new` runs the
/// private constructor of `class` outside of its scope.
pub fn private_ctor_error(vm: &Vm, class: &str) -> Option<String> {
    let scope = vm.current_class_scope();
    let lower = class.to_ascii_lowercase();
    let in_scope = scope.as_ref().is_some_and(|s| {
        let s = s.to_ascii_lowercase();
        s == lower.as_bytes() || vm.class_extends(&s, lower.as_bytes())
    });
    if in_scope {
        return None;
    }
    let from = match scope {
        Some(s) if !s.is_empty() => {
            let l = s.to_ascii_lowercase();
            let name = vm
                .classes
                .get(&l)
                .map(|c| String::from_utf8_lossy(&c.name).into_owned())
                .unwrap_or_else(|| String::from_utf8_lossy(&s).into_owned());
            format!("scope {}", name)
        }
        _ => "global scope".to_string(),
    };
    Some(format!(
        "Call to private {}::__construct() from {}",
        class, from
    ))
}
