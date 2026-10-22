//! ext/intl/listformatter: IntlListFormatter (ulistfmt_openForType()), over
//! the `intl` crate's CLDR list patterns.

use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};
use intl::list::{ListFormatOptions, ListType, ListWidth, format_list};

use crate::error::{self, IntlError};
use crate::{Call, Obj, obj, s, sv};

crate::intl_functions! { pub FUNCTIONS: }

pub const METHODS: &[(&str, crate::Imp)] = &[
    ("IntlListFormatter::__construct", construct),
    ("IntlListFormatter::format", format),
    ("IntlListFormatter::getErrorCode", get_error_code),
    ("IntlListFormatter::getErrorMessage", get_error_message),
];

pub const CLASSES: &[(&str, Option<crate::obj::CreateFn>)] =
    &[("IntlListFormatter", Some(create_object))];

#[derive(Clone, Debug, Default)]
struct LfState {
    /// (intl tag, options) once constructed
    fmt: Option<(String, ListFormatOptions)>,
    err: IntlError,
}

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(clone_object),
    ..NativeObjectHandlers::EMPTY
};

fn create_object(_vm: &mut Vm, o: &Obj) {
    obj::attach(o, LfState::default(), &HANDLERS);
}

fn clone_object(vm: &mut Vm, src: &Obj, _dst: &Obj) {
    obj::uncloneable(vm, src);
}

/// IntlListFormatter::__construct()
fn construct(vm: &mut Vm, c: &mut Call) -> Value {
    let o = c.obj();
    let loc = c.str(0);
    let ty = if c.passed(1) { c.long(1) } else { 0 };
    let width = if c.passed(2) { c.long(2) } else { 0 };
    let locale = crate::common::locale_or_default(vm, &loc);
    if loc.len() > crate::common::MAX_LOCALE_LEN {
        let msg = format!(
            "must be less than or equal to {} characters",
            crate::common::MAX_LOCALE_LEN
        );
        crate::zpp::value_error(vm, &c.name, 1, "locale", &msg);
        return Value::Null;
    }
    if !crate::numdata::has_iso3_language(&locale) {
        crate::zpp::value_error(vm, &c.name, 1, "locale", &format!("\"{}\" is invalid", locale));
        return Value::Null;
    }
    let list_type = match ty {
        0 => ListType::Conjunction,
        1 => ListType::Disjunction,
        2 => ListType::Unit,
        _ => {
            crate::zpp::value_error(
                vm,
                &c.name,
                2,
                "type",
                "must be one of IntlListFormatter::TYPE_AND, IntlListFormatter::TYPE_OR, or IntlListFormatter::TYPE_UNITS",
            );
            return Value::Null;
        }
    };
    let width = match width {
        0 => ListWidth::Long,
        1 => ListWidth::Short,
        2 => ListWidth::Narrow,
        _ => {
            crate::zpp::value_error(
                vm,
                &c.name,
                3,
                "width",
                "must be one of IntlListFormatter::WIDTH_WIDE, IntlListFormatter::WIDTH_SHORT, or IntlListFormatter::WIDTH_NARROW",
            );
            return Value::Null;
        }
    };
    let canon = crate::numdata::canonicalize(&locale);
    let valid = crate::numdata::valid_locale(&canon, &crate::common::default_locale(vm));
    let mut opts = ListFormatOptions::default();
    opts.list_type = list_type;
    opts.width = width;
    if let Some(st) = obj::state::<LfState>(&o) {
        st.borrow_mut().fmt = Some((crate::numdata::intl_tag(&valid), opts));
    }
    Value::Null
}

/// IntlListFormatter::format()
fn format(vm: &mut Vm, c: &mut Call) -> Value {
    let Value::Array(arr) = c.arg(0) else {
        return Value::Null;
    };
    let Some(st) = obj::state::<LfState>(&c.obj()) else {
        return Value::Null;
    };
    let vals: Vec<Value> = arr.borrow().iter().map(|(_, v)| v.deref()).collect();
    if vals.is_empty() {
        return s(b"");
    }
    let mut items: Vec<String> = Vec::with_capacity(vals.len());
    for v in &vals {
        let Ok(b) = vm.value_to_string_checked(v) else {
            return Value::Null;
        };
        if vm.current_exception.is_some() {
            return Value::Null;
        }
        match String::from_utf8(b.as_bytes().to_vec()) {
            Ok(t) => items.push(t),
            Err(_) => {
                error::set_global(
                    vm,
                    &c.name,
                    error::U_INVALID_CHAR_FOUND,
                    "Failed to convert string to UTF-16",
                );
                return Value::False;
            }
        }
    }
    let Some((tag, opts)) = st.borrow().fmt.clone() else {
        return Value::False;
    };
    let refs: Vec<&str> = items.iter().map(|x| x.as_str()).collect();
    sv(format_list(&tag, &refs, &opts).into_bytes())
}

/// IntlListFormatter::getErrorCode()
fn get_error_code(_vm: &mut Vm, c: &mut Call) -> Value {
    match obj::state::<LfState>(&c.obj()) {
        Some(st) => Value::Long(st.borrow().err.code as i64),
        None => Value::Long(0),
    }
}

/// IntlListFormatter::getErrorMessage()
fn get_error_message(_vm: &mut Vm, c: &mut Call) -> Value {
    match obj::state::<LfState>(&c.obj()) {
        Some(st) => s(st.borrow().err.message().as_bytes()),
        None => s(b"U_ZERO_ERROR"),
    }
}
