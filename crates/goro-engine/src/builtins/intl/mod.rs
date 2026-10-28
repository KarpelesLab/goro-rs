//! The intl extension (PHP manual: Internationalization Functions):
//! collation, number/message/date formatting, calendars and time zones,
//! locales, normalization, transliteration, text boundaries, IDNA,
//! character properties, spoof checks and charset conversion. The logic is
//! the goro-ext-intl crate's (on the `intl` crate); these modules wire it
//! to PHP functions and classes.
//!
//! Errors follow the manual's "intl error handling": every function resets
//! and sets the global error (`intl_get_error_code()`,
//! `intl_get_error_message()`) and the object's own one; with
//! `intl.use_exceptions` an `IntlException` is thrown, with a non-zero
//! (deprecated) `intl.error_level` a diagnostic of that level is raised.
//! Messages are "function(): text" and `intl_get_error_message()` appends
//! ": U_ERROR_NAME" (expected output of ext/intl/tests/formatter_fail.phpt
//! and ini_use_exceptions_basic.phpt).

use std::rc::Rc;

use goro_ext_intl::error as ue;
use goro_runtime::{ObjRef, PhpStr, Value};

use crate::class::Class;
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

mod breakiter;
mod calendar;
mod collator;
mod datefmt;
mod datehook;
mod dtpg;
mod iter;
mod locale;
mod msgfmt;
mod numfmt;
mod resbundle;
mod spoof;
mod text;
mod timezone;
mod translit;
mod uchar;
mod uconv;

/// Every module of the extension (all named "intl").
pub static MODULES: &[&Module] = &[
    &MODULE,
    &locale::MODULE,
    &collator::MODULE,
    &numfmt::MODULE,
    &msgfmt::MODULE,
    &timezone::MODULE,
    &calendar::MODULE,
    &datefmt::MODULE,
    &dtpg::MODULE,
    &text::MODULE,
    &iter::MODULE,
    &breakiter::MODULE,
    &uconv::MODULE,
    &translit::MODULE,
    &spoof::MODULE,
    &resbundle::MODULE,
    &uchar::MODULE,
];

pub static MODULE: Module = Module {
    name: "intl",
    functions: &[
        ("function intl_get_error_code(): int", intl_get_error_code),
        (
            "function intl_get_error_message(): string",
            intl_get_error_message,
        ),
        (
            "function intl_is_failure(int $errorCode): bool",
            intl_is_failure,
        ),
        (
            "function intl_error_name(int $errorCode): string",
            intl_error_name,
        ),
    ],
    classes: &[(
        "intlexception",
        "<?php class IntlException extends Exception {}",
    )],
    constants: CONSTANTS,
    // PHP manual: intl runtime configuration.
    ini: &[
        ("intl.default_locale", ""),
        ("intl.error_level", "0"),
        ("intl.use_exceptions", "0"),
    ],
    on_ini_set: Some(ini_set_hook),
    init: Some(init),
    ..Module::EMPTY
};

/// The ICU version the behaviour follows: the intl crate carries CLDR 48
/// and Unicode 17 data, ICU 78's.
pub const ICU_VERSION: &str = "78.1";

const CONSTANTS: &[(&str, ConstVal)] = &[
    ("INTL_MAX_LOCALE_LEN", ConstVal::Int(156)),
    ("INTL_ICU_VERSION", ConstVal::Str(ICU_VERSION)),
    ("INTL_ICU_DATA_VERSION", ConstVal::Str(ICU_VERSION)),
];

fn init(vm: &mut Vm) {
    for (name, v) in ue::CODES {
        vm.constants
            .insert(PhpStr::intern(name.as_bytes()), Value::Int(*v));
    }
}

/// `intl.error_level` is deprecated in PHP 8.5 for non-zero values
/// (expected output of ext/intl/tests/gh17469.phpt).
fn ini_set_hook(vm: &mut Vm, k: &[u8], v: &[u8]) -> R<bool> {
    if k == b"intl.error_level" && ini_int(v) != 0 {
        vm.deprecated(
            "ini_set(): Using a value different than 0 for intl.error_level is deprecated, \
             as the intl.error_level INI setting is deprecated. Instead the intl.use_exceptions \
             INI setting should be enabled to throw exceptions on errors or \
             intl_get_error_code()/intl_get_error_message() should be used to manually deal \
             with errors",
        )?;
    }
    if k == b"intl.default_locale" {
        vm.ext::<IntlState>().default_locale = None;
    }
    Ok(true)
}

fn ini_int(v: &[u8]) -> i64 {
    let s = String::from_utf8_lossy(v);
    let s = s.trim();
    match s.to_ascii_lowercase().as_str() {
        "on" | "yes" | "true" => 1,
        _ => {
            let digits: String = s
                .chars()
                .enumerate()
                .take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && *c == '-'))
                .map(|(_, c)| c)
                .collect();
            digits.parse().unwrap_or(0)
        }
    }
}

// ---------------------------------------------------------------- errors

/// The extension's per-request state.
#[derive(Default)]
pub(crate) struct IntlState {
    /// The global error.
    pub err: IntlErr,
    /// `Locale::setDefault()`'s locale (None: the ini setting's).
    pub default_locale: Option<String>,
}

/// An error code with its message ("function(): text").
#[derive(Default, Clone, Debug)]
pub(crate) struct IntlErr {
    pub code: i64,
    pub msg: String,
}

impl IntlErr {
    /// The message as `intl_get_error_message()` and the objects'
    /// `getErrorMessage()` give it.
    pub fn full_message(&self) -> String {
        if self.msg.is_empty() {
            ue::error_name(self.code).to_string()
        } else {
            format!("{}: {}", self.msg, ue::error_name(self.code))
        }
    }
}

/// Resets the global error (start of each intl function).
pub(crate) fn reset(vm: &mut Vm) {
    let st = vm.ext::<IntlState>();
    st.err.code = 0;
    st.err.msg.clear();
}

/// Sets the global error to `code` with "fname(): text" and raises it as
/// the ini settings say. Returns the message (for the object's copy).
pub(crate) fn fail(vm: &mut Vm, a: &Args, code: i64, text: &str) -> R<()> {
    let msg = format!("{}(): {}", a.fname(), text);
    fail_msg(vm, code, msg)
}

/// [`fail`] with the whole message.
pub(crate) fn fail_msg(vm: &mut Vm, code: i64, msg: String) -> R<()> {
    {
        let st = vm.ext::<IntlState>();
        st.err.code = code;
        st.err.msg = msg.clone();
    }
    raise(vm, code, &msg)
}

/// Sets only the global error code and message (no exception or
/// diagnostic): the error state some functions leave silently.
pub(crate) fn set_error_quiet(vm: &mut Vm, code: i64, msg: String) {
    let st = vm.ext::<IntlState>();
    st.err.code = code;
    st.err.msg = msg;
}

/// Throws or raises `msg` according to `intl.use_exceptions` and
/// `intl.error_level`.
pub(crate) fn raise(vm: &mut Vm, code: i64, msg: &str) -> R<()> {
    if use_exceptions(vm) {
        let e = vm.create_exception("IntlException", msg, code)?;
        return Err(crate::vm::Unwind::Throw(e));
    }
    let level = error_level(vm);
    if level != 0 {
        vm.raise(level, msg)?;
    }
    Ok(())
}

pub(crate) fn use_exceptions(vm: &Vm) -> bool {
    vm.get_ini(b"intl.use_exceptions")
        .is_some_and(|v| ini_int(v.as_bytes()) != 0)
}

pub(crate) fn error_level(vm: &Vm) -> i64 {
    vm.get_ini(b"intl.error_level")
        .map_or(0, |v| ini_int(v.as_bytes()))
}

fn intl_get_error_code(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(vm.ext::<IntlState>().err.code))
}

fn intl_get_error_message(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let m = vm.ext::<IntlState>().err.full_message();
    Ok(Value::from(m.as_str()))
}

fn intl_is_failure(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(ue::is_failure(a.int(0))))
}

fn intl_error_name(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::from(ue::error_name(a.int(0))))
}

// ---------------------------------------------------------------- locale

/// The default locale: `Locale::setDefault()`'s, else
/// `intl.default_locale`, else ICU's default ("en-US" in the test
/// environment: ext/intl/tests/locale_get_default.phpt).
pub(crate) fn default_locale(vm: &mut Vm) -> String {
    if let Some(l) = &vm.ext::<IntlState>().default_locale {
        return l.clone();
    }
    match vm.get_ini(b"intl.default_locale") {
        Some(v) if !v.as_bytes().is_empty() => v.to_string_lossy(),
        _ => "en-US".to_string(),
    }
}

// ---------------------------------------------------------------- objects

/// The native data of an intl object: the value and its own error.
pub(crate) struct IntlObj<T> {
    pub v: T,
    pub err: IntlErr,
}

impl<T: 'static> goro_runtime::NativeData for IntlObj<T> {}

/// Runs `f` on the native value of type `T` of `o` (None when the object
/// holds none: not constructed).
pub(crate) fn with_obj<T: 'static, U>(
    o: &ObjRef,
    f: impl FnOnce(&mut IntlObj<T>) -> U,
) -> Option<U> {
    let mut n = o.obj().native.borrow_mut();
    let b = n.as_mut()?;
    let any: &mut dyn std::any::Any = &mut **b;
    any.downcast_mut::<IntlObj<T>>().map(f)
}

/// Whether `o` holds a value of type `T`.
pub(crate) fn has_obj<T: 'static>(o: &ObjRef) -> bool {
    let n = o.obj().native.borrow();
    n.as_ref().is_some_and(|b| {
        let any: &dyn std::any::Any = &**b;
        any.is::<IntlObj<T>>()
    })
}

/// Stores `v` as the native value of `o`.
pub(crate) fn store<T: 'static>(o: &ObjRef, v: T) {
    *o.obj().native.borrow_mut() = Some(Box::new(IntlObj {
        v,
        err: IntlErr::default(),
    }));
}

/// Sets the object's own error (the global one is set separately).
pub(crate) fn set_obj_err<T: 'static>(o: &ObjRef, code: i64, msg: &str) {
    with_obj::<T, ()>(o, |x| {
        x.err.code = code;
        x.err.msg = msg.to_string();
    });
}

/// The object's own error.
pub(crate) fn obj_err<T: 'static>(o: &ObjRef) -> IntlErr {
    with_obj::<T, IntlErr>(o, |x| x.err.clone()).unwrap_or_default()
}

/// The object for a method (`$this`, arguments counted from 0) or a
/// procedural function (the first argument; the others are counted from
/// 1 in messages): `(object, argument offset)`.
pub(crate) fn this_or_first(a: &Args) -> (ObjRef, usize) {
    match &a.this {
        Some(o) => (o.clone(), 0),
        None => (a.obj(0).expect("object argument"), 1),
    }
}

/// Sets the global and the object's error, then raises it.
pub(crate) fn fail_obj<T: 'static>(
    vm: &mut Vm,
    a: &Args,
    o: &ObjRef,
    code: i64,
    text: &str,
) -> R<()> {
    let msg = format!("{}(): {}", a.fname(), text);
    set_obj_err::<T>(o, code, &msg);
    fail_msg(vm, code, msg)
}

/// [`fail_obj`] with a message holding arbitrary bytes (the invalid
/// UTF-8 a message quotes: ext/intl/tests/msgfmt_format_error4.phpt).
pub(crate) fn fail_obj_bytes<T: 'static>(
    vm: &mut Vm,
    a: &Args,
    o: Option<&ObjRef>,
    code: i64,
    text: &[u8],
) -> R<()> {
    let mut msg = format!("{}(): ", a.fname()).into_bytes();
    msg.extend_from_slice(text);
    let lossy = String::from_utf8_lossy(&msg).into_owned();
    if let Some(o) = o {
        set_obj_err::<T>(o, code, &lossy);
    }
    set_error_quiet(vm, code, lossy.clone());
    if use_exceptions(vm) {
        let e = vm.create_exception("IntlException", "", code)?;
        vm.set_prop_raw(&e, b"message", Value::Str(PhpStr::from(msg)));
        return Err(crate::vm::Unwind::Throw(e));
    }
    let level = error_level(vm);
    if level != 0 {
        vm.raise(level, &lossy)?;
    }
    Ok(())
}

/// Resets the global and the object's error.
pub(crate) fn reset_obj<T: 'static>(vm: &mut Vm, o: &ObjRef) {
    reset(vm);
    set_obj_err::<T>(o, 0, "");
}

pub(crate) fn class(vm: &mut Vm, name: &str) -> R<Rc<Class>> {
    match vm.find_class(name.as_bytes(), false)? {
        Some(c) => Ok(c),
        None => Err(vm.fatal(&format!("Class \"{name}\" not found"))),
    }
}

/// A new object of the named class holding `v`.
pub(crate) fn new_obj<T: 'static>(vm: &mut Vm, cls: &str, v: T) -> R<ObjRef> {
    let c = class(vm, cls)?;
    let o = vm.instantiate_raw(&c)?;
    store(&o, v);
    Ok(o)
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

/// "X object is already constructed" (`__construct()` called twice:
/// ext/intl/tests/formatter_fail.phpt).
pub(crate) fn already_constructed<T>(vm: &mut Vm, cls: &str) -> R<T> {
    vm.throw("Error", &format!("{cls} object is already constructed"))
}

/// The error of a method called on an object whose constructor did not
/// run or failed.
pub(crate) fn not_constructed<T>(vm: &mut Vm, cls: &str) -> R<T> {
    vm.throw("Error", &format!("Found unconstructed {cls}"))
}
