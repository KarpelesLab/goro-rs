//! ext/intl/dateformat: IntlDateFormatter and the datefmt_*() functions,
//! over ICU-style SimpleDateFormat patterns (datefmt_pattern.rs,
//! datefmt_parse.rs) and the CLDR data of the `intl` crate
//! (datefmt_data.rs).

#[path = "datefmt_data.rs"]
pub mod datefmt_data;
#[path = "datefmt_ext.rs"]
pub mod datefmt_ext;
#[path = "datefmt_parse.rs"]
pub mod datefmt_parse;
#[path = "datefmt_pattern.rs"]
pub mod datefmt_pattern;

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::{NumStr, Value};
use goro_core::vm::{NativeObjectHandlers, Vm};

use crate::datecore::{self as dc, CalInfo};
use crate::dateformat::datefmt_data::{self as data, locale_data};
use crate::dateformat::datefmt_ext as ext;
use crate::dateformat::datefmt_parse::ParseCtx;
use crate::dateformat::datefmt_pattern::{self as pat, Fmt, Item};
use crate::error::{self, IntlError, U_ILLEGAL_ARGUMENT_ERROR};
use crate::{Call, Obj, arr, bool_v, obj, s, zpp};

pub const UDAT_FULL: i64 = 0;
pub const UDAT_LONG: i64 = 1;
pub const UDAT_MEDIUM: i64 = 2;
pub const UDAT_SHORT: i64 = 3;
pub const UDAT_NONE: i64 = -1;
pub const UDAT_PATTERN: i64 = -2;
pub const UDAT_RELATIVE: i64 = 128;
pub const UCAL_GREGORIAN: i64 = 1;
pub const UCAL_TRADITIONAL: i64 = 0;

crate::intl_functions! { pub FUNCTIONS:
    "datefmt_create" => datefmt_create,
    "datefmt_get_datetype" => get_datetype,
    "datefmt_get_timetype" => get_timetype,
    "datefmt_get_calendar" => get_calendar,
    "datefmt_set_calendar" => set_calendar,
    "datefmt_get_timezone_id" => get_timezone_id,
    "datefmt_get_calendar_object" => get_calendar_object,
    "datefmt_get_timezone" => get_timezone,
    "datefmt_set_timezone" => set_timezone,
    "datefmt_set_pattern" => set_pattern,
    "datefmt_get_pattern" => get_pattern,
    "datefmt_get_locale" => get_locale,
    "datefmt_set_lenient" => set_lenient,
    "datefmt_is_lenient" => is_lenient,
    "datefmt_format" => format,
    "datefmt_format_object" => format_object,
    "datefmt_parse" => parse,
    "datefmt_localtime" => localtime,
    "datefmt_get_error_code" => get_error_code,
    "datefmt_get_error_message" => get_error_message,
}

pub const METHODS: &[(&str, crate::Imp)] = &[
    ("IntlDateFormatter::__construct", construct),
    ("IntlDateFormatter::setTimeZone", set_timezone),
    ("IntlDateFormatter::parseToCalendar", parse_to_calendar),
];

pub const CLASSES: &[(&str, Option<crate::obj::CreateFn>)] =
    &[("IntlDateFormatter", Some(create_object))];

/// The state of an IntlDateFormatter (IntlDateFormatter_object).
#[derive(Clone, Debug)]
pub struct DateFmt {
    /// The ICU formatter exists (constructed).
    pub ready: bool,
    pub err: IntlError,
    pub date_type: i64,
    pub time_type: i64,
    /// The calendar type given (UCAL_GREGORIAN, UCAL_TRADITIONAL), -1 for
    /// an IntlCalendar object.
    pub calendar: i64,
    /// The requested locale (canonicalized), with its keywords.
    pub requested_locale: String,
    /// The pattern (toPattern()).
    pub pattern: String,
    pub items: Vec<Item>,
    /// The calendar: its type, zone, settings.
    pub cal: CalInfo,
    pub lenient: bool,
}

impl Default for DateFmt {
    fn default() -> Self {
        DateFmt {
            ready: false,
            err: IntlError::default(),
            date_type: 0,
            time_type: 0,
            calendar: -1,
            requested_locale: String::new(),
            pattern: String::new(),
            items: Vec::new(),
            cal: CalInfo::gregorian(0.0, "UTC", ""),
            lenient: true,
        }
    }
}

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(clone_obj),
    ..NativeObjectHandlers::EMPTY
};

fn create_object(_vm: &mut Vm, o: &Obj) {
    obj::attach(o, DateFmt::default(), &HANDLERS);
}

/// IntlDateFormatter_object_clone
fn clone_obj(vm: &mut Vm, src: &Obj, dst: &Obj) {
    let ready = obj::state::<DateFmt>(src).is_some_and(|s| s.borrow().ready);
    obj::clone_state::<DateFmt>(src, dst, |s| s.clone());
    if !ready {
        zpp::throw(vm, b"Error", "Cannot clone uninitialized IntlDateFormatter");
    }
}

// ---------------------------------------------------------------------------
// Patterns of the styles

fn style_idx(st: i64) -> Option<usize> {
    match st & !UDAT_RELATIVE {
        0..=3 => Some((st & !UDAT_RELATIVE) as usize),
        _ => None,
    }
}

/// The date pattern of a style for the calendar of a locale's `@calendar`
/// keyword (non-gregorian calendars use CLDR's generic patterns, with the
/// era).
fn date_pattern(ld: &data::LocaleData, locale: &str, idx: usize) -> String {
    let g = ld.date[idx].clone();
    let cal = dc::locale_cal_type(locale);
    if matches!(cal.as_str(), "gregorian" | "gregory" | "iso8601") {
        return g;
    }
    if cal == "hebrew" && ld.lang.starts_with("en") {
        return ["EEEE, d MMMM y", "d MMMM y", "d MMM y", "d MMM y"][idx].to_string();
    }
    // the generic patterns: the era after the year
    let items = pat::compile(&g);
    let mut out = String::new();
    let mut done = false;
    for it in items {
        match it {
            Item::Lit(l) => data::quote_literal(&l, &mut out),
            Item::Field(c, n) => {
                if c == 'y' && !done {
                    out.push('y');
                    out.push_str(if idx == 3 { " GGGGG" } else { " G" });
                    done = true;
                } else {
                    for _ in 0..n {
                        out.push(c);
                    }
                }
            }
        }
    }
    out
}

/// The pattern of a date style and a time style (UDAT_FULL..UDAT_SHORT,
/// UDAT_NONE, the relative styles) for `locale`, as DateFormat's
/// createDateTimeInstance() builds it.
pub fn style_pattern(locale: &str, date_style: i64, time_style: i64) -> String {
    let ld = locale_data(locale);
    let d = style_idx(date_style).filter(|_| date_style != UDAT_NONE);
    let t = if time_style >= UDAT_RELATIVE {
        None
    } else {
        style_idx(time_style).filter(|_| time_style != UDAT_NONE)
    };
    match (d, t) {
        (None, None) => "yMMdd hh:mm a".to_string(),
        (Some(d), None) => date_pattern(&ld, locale, d),
        (None, Some(t)) => ld.time[t].clone(),
        (Some(d), Some(t)) => {
            let glue = if date_style & UDAT_RELATIVE != 0 {
                "{1}, {0}".to_string()
            } else {
                ld.glue[d].clone()
            };
            glue.replace("{1}", &date_pattern(&ld, locale, d))
                .replace("{0}", &ld.time[t])
        }
    }
}

/// Format the instant of `info` with an ICU pattern for `locale` (an
/// invalid pattern letter formats as nothing).
pub fn format_date_pattern(_vm: &mut Vm, locale: &str, pattern: &str, info: &CalInfo) -> String {
    pat::format_pattern(locale, pattern, info).unwrap_or_default()
}

/// The relative-day word of `info`'s day, compared to today in its zone
/// ("yesterday", "today", "tomorrow", "vorgestern").
fn relative_day(locale: &str, info: &CalInfo) -> Option<String> {
    let f = dc::compute_fields(info);
    let now = CalInfo {
        millis: ext::now_millis(),
        ..info.clone()
    };
    let nf = dc::compute_fields(&now);
    let diff = f[dc::JULIAN_DAY] as i64 - nf[dc::JULIAN_DAY] as i64;
    if !(-2..=2).contains(&diff) {
        return None;
    }
    let opts: intl::relative::RelativeTimeFormatOptions =
        intl::relative::RelativeNumeric::Auto.into();
    let w = intl::relative::format_relative(
        &data::intl_lang(locale),
        diff as f64,
        intl::relative::RelativeUnit::Day,
        &opts,
    );
    if w.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(w)
}

/// Format with the formatter's settings: Err(()) for an invalid pattern.
fn do_format(st: &DateFmt, millis: f64) -> Result<String, ()> {
    let info = CalInfo {
        millis,
        ..st.cal.clone()
    };
    let locale = st.requested_locale.as_str();
    let ld = locale_data(locale);
    if st.date_type >= UDAT_RELATIVE && st.date_type != UDAT_PATTERN {
        if let Some(word) = relative_day(locale, &info) {
            let t = if st.time_type >= UDAT_RELATIVE {
                None
            } else {
                style_idx(st.time_type).filter(|_| st.time_type != UDAT_NONE)
            };
            return Ok(match t {
                None => word,
                Some(t) => {
                    let time = Fmt::new(&ld, &info, locale).format(&pat::compile(&ld.time[t]))?;
                    format!("{}, {}", word, time)
                }
            });
        }
    }
    Fmt::new(&ld, &info, locale).format(&st.items)
}

// ---------------------------------------------------------------------------
// Construction

fn valid_style(t: i64) -> bool {
    matches!(t, -2..=3 | 128..=131)
}

/// Run `f` with intl.use_exceptions on and intl.error_level 0 (what the
/// constructors do).
pub fn with_exceptions<R>(vm: &mut Vm, f: impl FnOnce(&mut Vm) -> R) -> R {
    let ue = vm.constants.get(b"intl.use_exceptions".as_slice()).cloned();
    let el = vm.constants.get(b"intl.error_level".as_slice()).cloned();
    vm.constants.insert(b"intl.use_exceptions".to_vec(), s("1"));
    vm.constants.insert(b"intl.error_level".to_vec(), s("0"));
    let r = f(vm);
    match ue {
        Some(v) => vm.constants.insert(b"intl.use_exceptions".to_vec(), v),
        None => vm.constants.remove(b"intl.use_exceptions".as_slice()),
    };
    match el {
        Some(v) => vm.constants.insert(b"intl.error_level".to_vec(), v),
        None => vm.constants.remove(b"intl.error_level".as_slice()),
    };
    r
}

/// The calendar argument of the constructor / setCalendar().
enum CalArg {
    /// A calendar the formatter owns (from null or a calendar type).
    Owned(String, i64),
    /// An IntlCalendar object's calendar.
    Object(CalInfo),
}

/// datefmt_process_calendar_arg()
fn process_calendar_arg(
    vm: &mut Vm,
    c: &Call,
    v: &Value,
    locale: &str,
    err: &mut IntlError,
) -> Result<CalArg, ()> {
    match v {
        Value::Null | Value::Undef => Ok(CalArg::Owned("gregorian".to_string(), UCAL_GREGORIAN)),
        Value::Object(o) => match dc::cal_object_info(o) {
            Some(info) => Ok(CalArg::Object(info)),
            None => {
                error::set_both(
                    vm,
                    &c.name,
                    err,
                    U_ILLEGAL_ARGUMENT_ERROR,
                    "Found unconstructed IntlCalendar object",
                );
                Err(())
            }
        },
        v => {
            let n = v.to_long();
            if n != UCAL_TRADITIONAL && n != UCAL_GREGORIAN {
                error::set_both(
                    vm,
                    &c.name,
                    err,
                    U_ILLEGAL_ARGUMENT_ERROR,
                    "Invalid value for calendar type; it must be one of IntlDateFormatter::TRADITIONAL (locale's default calendar) or IntlDateFormatter::GREGORIAN. Alternatively, it can be an IntlCalendar object",
                );
                return Err(());
            }
            let t = if n == UCAL_TRADITIONAL {
                dc::locale_cal_type(locale)
            } else {
                "gregorian".to_string()
            };
            Ok(CalArg::Owned(t, n))
        }
    }
}

/// A new calendar of `cal_type` for `locale` in `tz` (now).
fn new_calendar(cal_type: &str, tz: &str, locale: &str) -> CalInfo {
    let mut info = CalInfo::gregorian(ext::now_millis(), tz, locale);
    info.cal_type = cal_type.to_string();
    let (fd, md) = ext::locale_week_data(locale);
    info.first_day_of_week = fd;
    info.minimal_days_in_first_week = md;
    info
}

/// datefmt_ctor(): Err(()) on failure (errors set / exception thrown).
fn ctor(vm: &mut Vm, c: &mut Call, o: &Obj) -> Result<(), ()> {
    error::reset_global();
    let Some(st) = obj::state::<DateFmt>(o) else {
        return Err(());
    };
    let locale_arg = c.opt_str(0);
    let date_type = if c.passed(1) { c.long(1) } else { UDAT_FULL };
    let time_type = if c.passed(2) { c.long(2) } else { UDAT_FULL };
    let tz_arg = c.arg(3);
    let cal_arg = c.arg(4);
    let pattern_arg = c.opt_str(5);

    if st.borrow().ready {
        let mut e = st.borrow().err.clone();
        error::set_both(vm, &c.name, &mut e, U_ILLEGAL_ARGUMENT_ERROR, "cannot call constructor twice");
        st.borrow_mut().err = e;
        return Err(());
    }
    if !valid_style(date_type) {
        error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, "invalid date format style");
        return Err(());
    }
    if !valid_style(time_type) {
        error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, "invalid time format style");
        return Err(());
    }
    if date_type == UDAT_PATTERN && time_type != UDAT_PATTERN {
        error::set_global(
            vm,
            &c.name,
            U_ILLEGAL_ARGUMENT_ERROR,
            "time format must be UDAT_PATTERN if date format is UDAT_PATTERN",
        );
        return Err(());
    }
    let locale_bytes = locale_arg.unwrap_or_default();
    if locale_bytes.len() > crate::common::MAX_LOCALE_LEN {
        let m = format!(
            "Locale string too long, should be no longer than {} characters",
            crate::common::MAX_LOCALE_LEN
        );
        error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, &m);
        return Err(());
    }
    let locale_str = crate::common::locale_or_default(vm, &locale_bytes);
    let locale = ext::uloc_canonicalize(&locale_str);
    let len = locale_str.len();
    if (len == 1 && locale_str != "C") || (len > 1 && !ext::has_iso3_language(&locale)) {
        let m = format!("\"{}\" is invalid", locale_str);
        zpp::value_error(vm, &c.name, 1, "locale", &m);
        return Err(());
    }

    let mut err = IntlError::default();
    let cal = match process_calendar_arg(vm, c, &cal_arg, &locale, &mut err) {
        Ok(cal) => cal,
        Err(()) => {
            st.borrow_mut().err = err;
            return Err(());
        }
    };
    let explicit_tz = !matches!(tz_arg, Value::Null | Value::Undef);
    let owned = matches!(cal, CalArg::Owned(..));
    let tz = if explicit_tz || owned {
        match dc::tz_from_arg(vm, &tz_arg, &c.name) {
            Ok(t) => Some(t),
            Err(()) => return Err(()),
        }
    } else {
        None
    };
    let pattern = match pattern_arg {
        Some(p) if !p.is_empty() => match String::from_utf8(p) {
            Ok(p) => Some(p),
            Err(_) => {
                error::set_global(
                    vm,
                    &c.name,
                    error::U_INVALID_CHAR_FOUND,
                    "error converting pattern to UTF-16",
                );
                return Err(());
            }
        },
        _ => None,
    };
    let (info, cal_int) = match cal {
        CalArg::Owned(t, n) => (
            new_calendar(&t, tz.as_deref().unwrap_or("UTC"), &locale),
            n,
        ),
        CalArg::Object(mut info) => {
            if let Some(tz) = tz {
                info.tz_id = tz;
            }
            (info, -1)
        }
    };
    let pattern = match pattern {
        Some(p) => p,
        None if date_type == UDAT_PATTERN => String::new(),
        None => style_pattern(&locale, date_type, time_type),
    };
    let mut s = st.borrow_mut();
    s.ready = true;
    s.err = IntlError::default();
    s.date_type = date_type;
    s.time_type = time_type;
    s.calendar = cal_int;
    s.requested_locale = locale;
    s.items = pat::compile(&pattern);
    s.pattern = pattern;
    s.cal = info;
    s.lenient = true;
    Ok(())
}

/// datefmt_create(), IntlDateFormatter::create()
fn datefmt_create(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(o) = obj::object_init_ex(vm, b"intldateformatter") else {
        return Value::Null;
    };
    if ctor(vm, c, &o).is_err() || vm.current_exception.is_some() {
        return Value::Null;
    }
    Value::Object(o)
}

/// IntlDateFormatter::__construct()
fn construct(vm: &mut Vm, c: &mut Call) -> Value {
    let o = c.obj();
    with_exceptions(vm, |vm| {
        let _ = ctor(vm, c, &o);
    });
    Value::Null
}

// ---------------------------------------------------------------------------
// Accessors

/// DATE_FORMAT_METHOD_FETCH_OBJECT: the state, its error reset; throws
/// for an unconstructed formatter.
fn fetch(vm: &mut Vm, c: &Call) -> Option<Rc<RefCell<DateFmt>>> {
    let o = c.this.clone()?;
    let st = obj::state::<DateFmt>(&o)?;
    st.borrow_mut().err.reset();
    if !st.borrow().ready {
        zpp::throw(vm, b"Error", "Found unconstructed IntlDateFormatter");
        return None;
    }
    Some(st)
}

/// INTL_METHOD_CHECK_STATUS with a failure code: false.
fn method_fail(vm: &mut Vm, c: &Call, st: &Rc<RefCell<DateFmt>>, code: i32, msg: &str) -> Value {
    let mut e = st.borrow().err.clone();
    error::set_both(vm, &c.name, &mut e, code, msg);
    st.borrow_mut().err = e;
    Value::False
}

/// INTL_METHOD_CHECK_STATUS on success: the global code reset.
fn method_ok() {
    error::set_global_code(error::U_ZERO_ERROR);
}

fn get_datetype(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    method_ok();
    Value::Long(st.borrow().date_type)
}

fn get_timetype(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    method_ok();
    Value::Long(st.borrow().time_type)
}

fn get_calendar(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    match st.borrow().calendar {
        -1 => Value::False,
        n => Value::Long(n),
    }
}

fn get_calendar_object(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let info = st.borrow().cal.clone();
    match dc::new_cal_object(vm, &info) {
        Some(o) => Value::Object(o),
        None => Value::Null,
    }
}

fn set_calendar(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let v = c.arg(0);
    let locale = st.borrow().requested_locale.clone();
    let mut err = st.borrow().err.clone();
    let r = process_calendar_arg(vm, c, &v, &locale, &mut err);
    st.borrow_mut().err = err;
    let Ok(cal) = r else {
        return Value::False;
    };
    let mut s = st.borrow_mut();
    match cal {
        CalArg::Owned(t, n) => {
            let tz = s.cal.tz_id.clone();
            s.cal = new_calendar(&t, &tz, &locale);
            s.calendar = n;
        }
        CalArg::Object(info) => {
            s.cal = info;
            s.calendar = -1;
        }
    }
    Value::True
}

fn get_timezone_id(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    method_ok();
    s(st.borrow().cal.tz_id.clone())
}

fn get_timezone(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let tz = st.borrow().cal.tz_id.clone();
    match dc::new_tz_object(vm, &tz) {
        Some(o) => Value::Object(o),
        None => Value::False,
    }
}

/// datefmt_set_timezone(), IntlDateFormatter::setTimeZone()
fn set_timezone(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let v = c.arg(0);
    match dc::tz_from_arg(vm, &v, &c.name) {
        Ok(tz) => {
            st.borrow_mut().cal.tz_id = tz;
            Value::True
        }
        Err(()) => Value::False,
    }
}

fn get_pattern(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    method_ok();
    s(st.borrow().pattern.clone())
}

fn set_pattern(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let p = c.str(0);
    let Ok(p) = String::from_utf8(p) else {
        return method_fail(
            vm,
            c,
            &st,
            error::U_INVALID_CHAR_FOUND,
            "Error converting pattern to UTF-16",
        );
    };
    method_ok();
    let mut s = st.borrow_mut();
    s.items = pat::compile(&p);
    s.pattern = p;
    Value::True
}

fn get_locale(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let t = if c.passed(0) { c.long(0) } else { 0 };
    let (valid, actual) = data::valid_actual(&st.borrow().requested_locale);
    match t {
        0 => {
            method_ok();
            s(actual)
        }
        1 => {
            method_ok();
            s(valid)
        }
        _ => method_fail(vm, c, &st, U_ILLEGAL_ARGUMENT_ERROR, "Error getting locale"),
    }
}

fn is_lenient(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    bool_v(st.borrow().lenient)
}

fn set_lenient(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let l = c.bool(0);
    let mut s = st.borrow_mut();
    s.lenient = l;
    s.cal.lenient = l;
    Value::Null
}

fn get_error_code(_vm: &mut Vm, c: &mut Call) -> Value {
    let o = c.obj();
    match obj::state::<DateFmt>(&o) {
        Some(st) => Value::Long(st.borrow().err.code as i64),
        None => Value::Long(0),
    }
}

fn get_error_message(_vm: &mut Vm, c: &mut Call) -> Value {
    let o = c.obj();
    match obj::state::<DateFmt>(&o) {
        Some(st) => s(st.borrow().err.message()),
        None => s("U_ZERO_ERROR"),
    }
}

// ---------------------------------------------------------------------------
// Formatting

/// intl_zval_to_millis(): Err(None) with an exception pending,
/// Err(Some((code, msg))) for an intl error.
fn zval_to_millis(vm: &mut Vm, v: &Value) -> Result<f64, Option<(i32, String)>> {
    match v.deref() {
        Value::String(s) => match numeric(s.as_bytes()) {
            Some(NumStr::Long(n)) => Ok(n as f64 * 1000.0),
            Some(NumStr::Double(d)) => Ok(d * 1000.0),
            _ => Err(Some((
                U_ILLEGAL_ARGUMENT_ERROR,
                format!(
                    "string '{}' is not numeric, which would be required for it to be a valid date",
                    s.to_string_lossy()
                ),
            ))),
        },
        Value::Long(n) => Ok(n as f64 * 1000.0),
        Value::Double(d) => Ok(d * 1000.0),
        Value::Object(o) => {
            if zpp::obj_instanceof(vm, &o, "DateTimeInterface") {
                ext::datetime_decompose(vm, &o).map(|(m, _)| m)
            } else if zpp::obj_instanceof(vm, &o, "IntlCalendar") {
                match dc::cal_object_info(&o) {
                    Some(i) => Ok(i.millis),
                    None => Err(Some((
                        U_ILLEGAL_ARGUMENT_ERROR,
                        "IntlCalendar object is not properly constructed".to_string(),
                    ))),
                }
            } else {
                Err(Some((
                    U_ILLEGAL_ARGUMENT_ERROR,
                    "invalid object type for date/time (only IntlCalendar and DateTimeInterface permitted)"
                        .to_string(),
                )))
            }
        }
        _ => Err(Some((
            U_ILLEGAL_ARGUMENT_ERROR,
            "invalid PHP type for date".to_string(),
        ))),
    }
}

/// internal_get_arr_ele()
fn arr_ele(a: &PhpArray, key: &str) -> Result<i32, String> {
    match a.get_str(key.as_bytes()).map(|v| v.deref()) {
        None => Ok(0),
        Some(Value::Long(n)) => {
            if n > i32::MAX as i64 || n < i32::MIN as i64 {
                Err(format!(
                    "value {} is out of bounds for a 32-bit integer in key '{}'",
                    n, key
                ))
            } else {
                Ok(n as i32)
            }
        }
        Some(_) => Err(format!(
            "parameter array contains a non-integer element for key '{}'",
            key
        )),
    }
}

/// datefmt_format()
fn format(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let v = c.arg(0);
    let millis = if let Value::Array(a) = &v {
        let a = a.borrow();
        if a.is_empty() {
            return Value::False;
        }
        let mut vals = [0i32; 6];
        for (i, k) in ["tm_year", "tm_mon", "tm_hour", "tm_min", "tm_sec", "tm_mday"]
            .iter()
            .enumerate()
        {
            match arr_ele(&a, k) {
                Ok(n) => vals[i] = n,
                Err(m) => {
                    drop(a);
                    method_fail(vm, c, &st, U_ILLEGAL_ARGUMENT_ERROR, &m);
                    return method_fail(vm, c, &st, U_ILLEGAL_ARGUMENT_ERROR, "date formatting failed");
                }
            }
        }
        let info = st.borrow().cal.clone();
        let mut f = dc::compute_fields(&info);
        f[dc::EXTENDED_YEAR] = vals[0] + 1900;
        f[dc::YEAR] = vals[0] + 1900;
        f[dc::ERA] = 1;
        f[dc::MONTH] = vals[1];
        f[dc::HOUR_OF_DAY] = vals[2];
        f[dc::MINUTE] = vals[3];
        f[dc::SECOND] = vals[4];
        f[dc::DATE] = vals[5];
        method_ok();
        dc::fields_to_millis(&info, &f)
    } else {
        match zval_to_millis(vm, &v) {
            Ok(m) => m,
            Err(None) => return Value::False,
            Err(Some((code, msg))) => return method_fail(vm, c, &st, code, &msg),
        }
    };
    let r = do_format(&st.borrow(), millis);
    match r {
        Ok(out) => {
            method_ok();
            s(out)
        }
        Err(()) => method_fail(vm, c, &st, error::U_INVALID_FORMAT_ERROR, "Date formatting failed"),
    }
}

fn valid_object_style(n: i64) -> bool {
    matches!(n, -1..=3 | 128..=131)
}

/// datefmt_format_object(), IntlDateFormatter::formatObject()
fn format_object(vm: &mut Vm, c: &mut Call) -> Value {
    let o = match c.arg(0) {
        Value::Object(o) => o,
        v => {
            let m = format!("must be of type object, {} given", Vm::value_type_name(&v));
            zpp::arg_type_error(vm, &c.name, 1, "datetime", &m);
            return Value::Null;
        }
    };
    let fmt = c.arg(1);
    let locale_str = match c.opt_str(2) {
        Some(l) => String::from_utf8_lossy(&l).into_owned(),
        None => crate::common::default_locale(vm),
    };
    let locale = ext::uloc_canonicalize(&locale_str);
    let fail = |vm: &mut Vm, code: i32, msg: &str| {
        error::set_global(vm, &c.name, code, msg);
        Value::False
    };
    let (mut date_style, mut time_style) = (UDAT_MEDIUM, UDAT_MEDIUM);
    let mut pattern: Option<String> = None;
    match &fmt {
        Value::Null | Value::Undef => {}
        Value::Array(a) => {
            let vals: Vec<Value> = a.borrow().values().map(|v| v.deref()).collect();
            if vals.len() != 2 {
                return fail(
                    vm,
                    U_ILLEGAL_ARGUMENT_ERROR,
                    "bad format; if array, it must have two elements",
                );
            }
            for (i, z) in vals.iter().enumerate() {
                let ok = matches!(z, Value::Long(n) if valid_object_style(*n));
                if !ok {
                    return fail(
                        vm,
                        U_ILLEGAL_ARGUMENT_ERROR,
                        if i == 0 {
                            "bad format; the date format (first element of the array) is not valid"
                        } else {
                            "bad format; the time format (second element of the array) is not valid"
                        },
                    );
                }
                let n = z.to_long();
                if i == 0 {
                    date_style = n;
                } else {
                    time_style = n;
                }
            }
        }
        Value::Long(n) => {
            if !valid_object_style(*n) {
                return fail(
                    vm,
                    U_ILLEGAL_ARGUMENT_ERROR,
                    "the date/time format type is invalid",
                );
            }
            date_style = *n;
            time_style = *n;
        }
        v => {
            let sv = v.to_php_string();
            if vm.current_exception.is_some() {
                return Value::Null;
            }
            if sv.as_bytes().is_empty() {
                return fail(vm, U_ILLEGAL_ARGUMENT_ERROR, "the format is empty");
            }
            pattern = Some(sv.to_string_lossy());
        }
    }
    if time_style != UDAT_NONE {
        time_style &= !UDAT_RELATIVE;
    }
    let info = if zpp::obj_instanceof(vm, &o, "IntlCalendar") {
        match dc::cal_object_info(&o) {
            Some(i) => i,
            None => {
                return fail(
                    vm,
                    U_ILLEGAL_ARGUMENT_ERROR,
                    "bad IntlCalendar instance: not initialized properly",
                );
            }
        }
    } else if zpp::obj_instanceof(vm, &o, "DateTimeInterface") {
        match ext::datetime_decompose(vm, &o) {
            Ok((m, tz)) => {
                let mut i = new_calendar("gregorian", &tz, &locale);
                i.millis = m;
                i
            }
            Err(None) => return Value::False,
            Err(Some((code, msg))) => return fail(vm, code, &msg),
        }
    } else {
        return fail(
            vm,
            error::U_ZERO_ERROR,
            "the passed object must be an instance of either IntlCalendar or DateTimeInterface",
        );
    };
    let st = DateFmt {
        ready: true,
        date_type: if pattern.is_some() { UDAT_PATTERN } else { date_style },
        time_type: if pattern.is_some() { UDAT_PATTERN } else { time_style },
        calendar: -1,
        items: pat::compile(
            &pattern
                .clone()
                .unwrap_or_else(|| style_pattern(&locale, date_style, time_style)),
        ),
        pattern: pattern.unwrap_or_default(),
        requested_locale: locale,
        cal: info.clone(),
        ..DateFmt::default()
    };
    match do_format(&st, info.millis) {
        Ok(out) => s(out),
        Err(()) => fail(vm, error::U_INVALID_FORMAT_ERROR, "error converting result to UTF-8"),
    }
}

// ---------------------------------------------------------------------------
// Parsing

/// The parse position argument: Ok(None) when not given, Err(()) when the
/// function must return false.
fn parse_pos_arg(vm: &mut Vm, c: &Call, text_len: usize, try_long: bool) -> Result<Option<i64>, Value> {
    if !c.passed(1) {
        return Ok(None);
    }
    let v = c.arg(1);
    let n = if try_long {
        match &v {
            Value::Long(n) => *n,
            Value::Null | Value::Undef => 0,
            Value::False => 0,
            Value::True => 1,
            Value::Double(d) => {
                let line = vm.current_line;
                vm.float_to_int_cast(*d, line)
            }
            Value::String(s) => match numeric(s.as_bytes()) {
                Some(NumStr::Long(n)) => n,
                Some(NumStr::Double(d)) => d as i64,
                _ => {
                    let m = format!("must be of type int, {} given", Vm::value_type_name(&v));
                    zpp::arg_type_error(vm, &c.name, 2, "offset", &m);
                    return Err(Value::Null);
                }
            },
            _ => {
                let m = format!("must be of type int, {} given", Vm::value_type_name(&v));
                zpp::arg_type_error(vm, &c.name, 2, "offset", &m);
                return Err(Value::Null);
            }
        }
    } else {
        v.to_long()
    };
    if n > i32::MAX as i64 || n < i32::MIN as i64 {
        error::set_global_code(U_ILLEGAL_ARGUMENT_ERROR);
        error::set_global(vm, &c.name, U_ILLEGAL_ARGUMENT_ERROR, "String index is out of valid range.");
        return Err(Value::False);
    }
    // (size_t)parse_pos > text_len; parseToCalendar() lets -1 through
    if !(try_long && n == -1) && (n < 0 || n as usize > text_len) {
        return Err(Value::False);
    }
    Ok(Some(n))
}

/// is_numeric_string()
fn numeric(b: &[u8]) -> Option<NumStr> {
    match goro_core::value::parse_numeric_str(b, false) {
        Some((n, false)) => Some(n),
        _ => None,
    }
}

/// The text as chars, with the UTF-16 offset of each char boundary.
fn utf16_map(text: &str) -> (Vec<char>, Vec<usize>) {
    let chars: Vec<char> = text.chars().collect();
    let mut offs = Vec::with_capacity(chars.len() + 1);
    let mut o = 0;
    for ch in &chars {
        offs.push(o);
        o += ch.len_utf16();
    }
    offs.push(o);
    (chars, offs)
}

/// Parse `text` from UTF-16 position `pos`: Ok((millis, zone, fields, end))
/// or Err(error position), both in UTF-16 units.
fn do_parse(st: &DateFmt, text: &str, pos: i64, keep: bool) -> Result<(f64, String, usize), usize> {
    let (chars, offs) = utf16_map(text);
    let start = offs.iter().position(|&o| o as i64 >= pos.max(0)).unwrap_or(chars.len());
    let ld = locale_data(&st.requested_locale);
    let cx = ParseCtx {
        ld: &ld,
        info: &st.cal,
        locale: &st.requested_locale,
        lenient: st.lenient,
    };
    match cx.parse(&st.items, &chars, start) {
        Ok((p, end)) => {
            let (millis, tz) = cx.resolve(&p, keep);
            Ok((millis, tz, offs[end.min(chars.len())]))
        }
        Err(e) => Err(offs[e.min(chars.len())]),
    }
}

fn millis_to_ret(millis: f64) -> Value {
    let r = millis / 1000.0;
    if r > i64::MAX as f64 || r < i64::MIN as f64 {
        Value::Double(if r < 0.0 { r.ceil() } else { r.floor() })
    } else {
        Value::Long(r as i64)
    }
}

/// internal_parse_to_timestamp(): the return value and the new position.
fn parse_common(vm: &mut Vm, c: &mut Call, update_calendar: bool) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let text = c.str(0);
    let pos = match parse_pos_arg(vm, c, text.len(), update_calendar) {
        Ok(p) => p,
        Err(v) => return v,
    };
    let mut new_pos = pos.unwrap_or(0);
    let ret = match std::str::from_utf8(&text) {
        Err(_) => method_fail(
            vm,
            c,
            &st,
            error::U_INVALID_CHAR_FOUND,
            "Error converting timezone to UTF-16",
        ),
        Ok(t) => {
            let r = do_parse(&st.borrow(), t, pos.unwrap_or(0), update_calendar);
            match r {
                Ok((millis, tz, end)) => {
                    new_pos = end as i64;
                    if update_calendar {
                        let mut s = st.borrow_mut();
                        s.cal.millis = millis;
                        s.cal.tz_id = tz;
                    }
                    method_ok();
                    millis_to_ret(millis)
                }
                Err(e) => {
                    new_pos = e as i64;
                    method_fail(vm, c, &st, error::U_PARSE_ERROR, "Date parsing failed")
                }
            }
        }
    };
    if pos.is_some() {
        c.set_ref(vm, 1, Value::Long(new_pos));
    }
    ret
}

/// datefmt_parse()
fn parse(vm: &mut Vm, c: &mut Call) -> Value {
    parse_common(vm, c, false)
}

/// IntlDateFormatter::parseToCalendar()
fn parse_to_calendar(vm: &mut Vm, c: &mut Call) -> Value {
    parse_common(vm, c, true)
}

/// datefmt_localtime()
fn localtime(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    let text = c.str(0);
    let pos = match parse_pos_arg(vm, c, text.len(), false) {
        Ok(p) => p,
        Err(v) => return v,
    };
    let mut new_pos = pos.unwrap_or(0);
    let ret = match std::str::from_utf8(&text) {
        Err(_) => method_fail(
            vm,
            c,
            &st,
            error::U_INVALID_CHAR_FOUND,
            "Error converting timezone to UTF-16",
        ),
        Ok(t) => {
            let r = do_parse(&st.borrow(), t, pos.unwrap_or(0), true);
            match r {
                Ok((millis, tz, end)) => {
                    new_pos = end as i64;
                    let info = {
                        let mut s = st.borrow_mut();
                        s.cal.millis = millis;
                        s.cal.tz_id = tz;
                        s.cal.clone()
                    };
                    method_ok();
                    let f = dc::compute_fields(&info);
                    let mut a = PhpArray::new();
                    let mut put = |k: &str, v: i64| {
                        a.set(
                            ArrayKey::String(PhpString::from_bytes(k.as_bytes())),
                            Value::Long(v),
                        )
                    };
                    put("tm_sec", f[dc::SECOND] as i64);
                    put("tm_min", f[dc::MINUTE] as i64);
                    put("tm_hour", f[dc::HOUR_OF_DAY] as i64);
                    put("tm_year", f[dc::YEAR] as i64 - 1900);
                    put("tm_mday", f[dc::DATE] as i64);
                    put("tm_wday", f[dc::DAY_OF_WEEK] as i64 - 1);
                    put("tm_yday", f[dc::DAY_OF_YEAR] as i64);
                    put("tm_mon", f[dc::MONTH] as i64);
                    put("tm_isdst", (f[dc::DST_OFFSET] != 0) as i64);
                    arr(a)
                }
                Err(e) => {
                    new_pos = e as i64;
                    method_fail(vm, c, &st, error::U_PARSE_ERROR, "Date parsing failed")
                }
            }
        }
    };
    if pos.is_some() {
        c.set_ref(vm, 1, Value::Long(new_pos));
    }
    ret
}
