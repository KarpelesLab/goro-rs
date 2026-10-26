//! Locale functions of ext/standard/string.c: setlocale(), localeconv(),
//! nl_langinfo(), strcoll(), on top of the C library's locale support.

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::rc::Rc;

/// nl_langinfo() items (glibc values), registered as constants.
pub const LANGINFO_ITEMS: &[(&str, i64)] = &[
    ("ABDAY_1", 131072),
    ("ABDAY_2", 131073),
    ("ABDAY_3", 131074),
    ("ABDAY_4", 131075),
    ("ABDAY_5", 131076),
    ("ABDAY_6", 131077),
    ("ABDAY_7", 131078),
    ("DAY_1", 131079),
    ("DAY_2", 131080),
    ("DAY_3", 131081),
    ("DAY_4", 131082),
    ("DAY_5", 131083),
    ("DAY_6", 131084),
    ("DAY_7", 131085),
    ("ABMON_1", 131086),
    ("ABMON_2", 131087),
    ("ABMON_3", 131088),
    ("ABMON_4", 131089),
    ("ABMON_5", 131090),
    ("ABMON_6", 131091),
    ("ABMON_7", 131092),
    ("ABMON_8", 131093),
    ("ABMON_9", 131094),
    ("ABMON_10", 131095),
    ("ABMON_11", 131096),
    ("ABMON_12", 131097),
    ("MON_1", 131098),
    ("MON_2", 131099),
    ("MON_3", 131100),
    ("MON_4", 131101),
    ("MON_5", 131102),
    ("MON_6", 131103),
    ("MON_7", 131104),
    ("MON_8", 131105),
    ("MON_9", 131106),
    ("MON_10", 131107),
    ("MON_11", 131108),
    ("MON_12", 131109),
    ("AM_STR", 131110),
    ("PM_STR", 131111),
    ("D_T_FMT", 131112),
    ("D_FMT", 131113),
    ("T_FMT", 131114),
    ("T_FMT_AMPM", 131115),
    ("ERA", 131116),
    ("ERA_YEAR", 131117),
    ("ERA_D_T_FMT", 131120),
    ("ERA_D_FMT", 131118),
    ("ERA_T_FMT", 131121),
    ("ALT_DIGITS", 131119),
    ("INT_CURR_SYMBOL", 262144),
    ("CURRENCY_SYMBOL", 262145),
    ("CRNCYSTR", 262159),
    ("MON_DECIMAL_POINT", 262146),
    ("MON_THOUSANDS_SEP", 262147),
    ("MON_GROUPING", 262148),
    ("POSITIVE_SIGN", 262149),
    ("NEGATIVE_SIGN", 262150),
    ("INT_FRAC_DIGITS", 262151),
    ("FRAC_DIGITS", 262152),
    ("P_CS_PRECEDES", 262153),
    ("P_SEP_BY_SPACE", 262154),
    ("N_CS_PRECEDES", 262155),
    ("N_SEP_BY_SPACE", 262156),
    ("P_SIGN_POSN", 262157),
    ("N_SIGN_POSN", 262158),
    ("DECIMAL_POINT", 65536),
    ("RADIXCHAR", 65536),
    ("THOUSANDS_SEP", 65537),
    ("THOUSEP", 65537),
    ("GROUPING", 65538),
    ("YESEXPR", 327680),
    ("NOEXPR", 327681),
    ("YESSTR", 327682),
    ("NOSTR", 327683),
    ("CODESET", 14),
];

thread_local! {
    /// BG(ctype_string) is set: setlocale() changed LC_CTYPE to something
    /// other than "C" (PCRE then builds character tables for the locale).
    static CTYPE_SET: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn ctype_locale_active() -> bool {
    CTYPE_SET.with(|c| c.get())
}

/// Bytes >= 0x80 that are alphanumeric in the current LC_CTYPE (for
/// locale-specific PCRE tables); empty unless setlocale() changed LC_CTYPE.
pub fn locale_word_bytes() -> Vec<u8> {
    if !ctype_locale_active() {
        return Vec::new();
    }
    (0x80u16..=0xff)
        .map(|b| b as u8)
        .filter(|&b| unsafe { libc::isalnum(b as i32) != 0 })
        .collect()
}

/// zend_reset_lc_ctype_locale(): LC_CTYPE is C.UTF-8 (or C) at startup.
pub fn startup() {
    unsafe {
        if libc::setlocale(libc::LC_CTYPE, c"C.UTF-8".as_ptr()).is_null() {
            libc::setlocale(libc::LC_CTYPE, c"C".as_ptr());
        }
    }
}

pub fn register(vm: &mut Vm) {
    startup();
    for (name, v) in LANGINFO_ITEMS {
        vm.constants
            .insert(name.as_bytes().to_vec(), Value::Long(*v));
    }
    vm.register_function(b"setlocale", setlocale_fn);
    vm.register_function(b"localeconv", localeconv_fn);
    vm.register_function(b"nl_langinfo", nl_langinfo_fn);
    vm.register_function(b"strcoll", strcoll_fn);
}

/// The LC_NUMERIC decimal point (localeconv()->decimal_point[0]).
pub fn decimal_point() -> u8 {
    unsafe {
        let lc = libc::localeconv();
        if lc.is_null() || (*lc).decimal_point.is_null() {
            return b'.';
        }
        let c = *(*lc).decimal_point as u8;
        if c == 0 { b'.' } else { c }
    }
}

fn throw(vm: &mut Vm, class: &[u8], msg: &str) -> VmError {
    let exc = if class == b"TypeError" {
        vm.throw_type_error(msg.to_string())
    } else {
        vm.create_exception(class, msg, vm.current_line)
    };
    vm.current_exception = Some(exc);
    VmError {
        message: msg.to_string(),
        line: vm.current_line,
    }
}

fn s(b: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(b))
}

fn cstr(p: *const libc::c_char) -> Vec<u8> {
    if p.is_null() {
        Vec::new()
    } else {
        unsafe { CStr::from_ptr(p).to_bytes().to_vec() }
    }
}

/// try_setlocale_str()
fn try_setlocale_str(vm: &mut Vm, cat: i64, loc: &[u8]) -> Option<Vec<u8>> {
    let query = loc == b"0";
    if !query && loc.len() >= 255 {
        vm.emit_warning("setlocale(): Specified locale name is too long");
        return None;
    }
    let retval = if query {
        unsafe { libc::setlocale(cat as libc::c_int, std::ptr::null()) }
    } else {
        // C strings stop at NUL.
        let end = loc.iter().position(|&c| c == 0).unwrap_or(loc.len());
        let c = CString::new(&loc[..end]).ok()?;
        unsafe { libc::setlocale(cat as libc::c_int, c.as_ptr()) }
    };
    if retval.is_null() {
        return None;
    }
    let r = cstr(retval);
    if !query && (cat == libc::LC_CTYPE as i64 || cat == libc::LC_ALL as i64) {
        CTYPE_SET.with(|c| c.set(r != b"C"));
        if r == b"C" {
            return Some(b"C".to_vec());
        }
    }
    if !query && r == loc {
        return Some(loc.to_vec());
    }
    Some(r)
}

/// zend_parse_arg_str() (nullable) of a variadic setlocale() argument.
fn locale_arg(vm: &mut Vm, v: &Value, n: usize) -> Result<Option<Vec<u8>>, VmError> {
    let v = v.deref();
    let bad = |vm: &mut Vm, v: &Value| -> VmError {
        let pname = if n == 2 { " ($locales)" } else { "" };
        throw(
            vm,
            b"TypeError",
            &format!(
                "setlocale(): Argument #{}{} must be of type array|string|null, {} given",
                n,
                pname,
                Vm::value_type_name(v)
            ),
        )
    };
    match &v {
        Value::String(st) => Ok(Some(st.as_bytes().to_vec())),
        Value::Null | Value::Undef => Ok(None),
        Value::Array(_) => Ok(None),
        Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
            if vm.caller_strict_types {
                Err(bad(vm, &v))
            } else {
                Ok(Some(v.to_php_string().as_bytes().to_vec()))
            }
        }
        // (zend_parse_arg_str(): a Stringable converts, others are a type error)
        Value::Object(_) => {
            let (strict, line) = (vm.caller_strict_types, vm.current_line);
            match vm.zpp_coerce_scalar(goro_core::arginfo::T_STRING, &v, strict, line) {
                Ok(Some(st)) => Ok(Some(st.to_php_string().as_bytes().to_vec())),
                Ok(None) => Err(bad(vm, &v)),
                Err(()) => Err(VmError {
                    message: vm.uncaught_exception_message(),
                    line,
                }),
            }
        }
        _ => Err(bad(vm, &v)),
    }
}

/// PHP_FUNCTION(setlocale)
pub fn setlocale_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() < 2 {
        return Err(throw(
            vm,
            b"ArgumentCountError",
            &format!(
                "setlocale() expects at least 2 arguments, {} given",
                args.len()
            ),
        ));
    }
    let cat = args[0].to_long();
    // Validate all arguments first.
    let mut strs = Vec::with_capacity(args.len() - 1);
    for (i, a) in args[1..].iter().enumerate() {
        strs.push(locale_arg(vm, a, i + 2)?);
    }
    for (i, a) in args[1..].iter().enumerate() {
        if let Value::Array(arr) = a.deref() {
            let elems: Vec<Value> = arr.borrow().values().cloned().collect();
            for e in elems {
                let e = e.deref();
                let st = match &e {
                    Value::Object(_) => vm.value_to_string_checked(&e)?.as_bytes().to_vec(),
                    Value::Array(_) => {
                        vm.emit_warning("Array to string conversion");
                        b"Array".to_vec()
                    }
                    _ => e.to_php_string().as_bytes().to_vec(),
                };
                if let Some(r) = try_setlocale_str(vm, cat, &st) {
                    return Ok(Value::String(PhpString::from_vec(r)));
                }
            }
            continue;
        }
        let st = strs[i].clone().unwrap_or_default();
        if let Some(r) = try_setlocale_str(vm, cat, &st) {
            return Ok(Value::String(PhpString::from_vec(r)));
        }
    }
    Ok(Value::False)
}

/// PHP_FUNCTION(localeconv)
pub fn localeconv_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    let mut grouping = PhpArray::new();
    let mut mon_grouping = PhpArray::new();
    unsafe {
        let lc = libc::localeconv();
        let l = &*lc;
        for c in cstr(l.grouping) {
            grouping.push(Value::Long(c as i8 as i64));
        }
        for c in cstr(l.mon_grouping) {
            mon_grouping.push(Value::Long(c as i8 as i64));
        }
        let mut set = |k: &[u8], v: Value| arr.set(ArrayKey::String(PhpString::from_bytes(k)), v);
        set(b"decimal_point", s(&cstr(l.decimal_point)));
        set(b"thousands_sep", s(&cstr(l.thousands_sep)));
        set(b"int_curr_symbol", s(&cstr(l.int_curr_symbol)));
        set(b"currency_symbol", s(&cstr(l.currency_symbol)));
        set(b"mon_decimal_point", s(&cstr(l.mon_decimal_point)));
        set(b"mon_thousands_sep", s(&cstr(l.mon_thousands_sep)));
        set(b"positive_sign", s(&cstr(l.positive_sign)));
        set(b"negative_sign", s(&cstr(l.negative_sign)));
        set(
            b"int_frac_digits",
            Value::Long(l.int_frac_digits as i8 as i64),
        );
        set(b"frac_digits", Value::Long(l.frac_digits as i8 as i64));
        set(b"p_cs_precedes", Value::Long(l.p_cs_precedes as i8 as i64));
        set(
            b"p_sep_by_space",
            Value::Long(l.p_sep_by_space as i8 as i64),
        );
        set(b"n_cs_precedes", Value::Long(l.n_cs_precedes as i8 as i64));
        set(
            b"n_sep_by_space",
            Value::Long(l.n_sep_by_space as i8 as i64),
        );
        set(b"p_sign_posn", Value::Long(l.p_sign_posn as i8 as i64));
        set(b"n_sign_posn", Value::Long(l.n_sign_posn as i8 as i64));
    }
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"grouping")),
        Value::Array(Rc::new(RefCell::new(grouping))),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"mon_grouping")),
        Value::Array(Rc::new(RefCell::new(mon_grouping))),
    );
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

/// PHP_FUNCTION(nl_langinfo)
pub fn nl_langinfo_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let item = args.first().map(|v| v.to_long()).unwrap_or(0);
    if !LANGINFO_ITEMS.iter().any(|(_, v)| *v == item) {
        vm.emit_warning(&format!("nl_langinfo(): Item '{}' is not valid", item));
        return Ok(Value::False);
    }
    let p = unsafe { libc::nl_langinfo(item as libc::nl_item) };
    if p.is_null() {
        return Ok(Value::False);
    }
    Ok(s(&cstr(p)))
}

/// strcoll() on two byte strings (each cut at its first NUL), as
/// string_locale_compare_function does for SORT_LOCALE_STRING.
pub(crate) fn locale_strcoll(a: &[u8], b: &[u8]) -> i32 {
    let cut = |x: &[u8]| -> CString {
        let end = x.iter().position(|&c| c == 0).unwrap_or(x.len());
        CString::new(&x[..end]).unwrap_or_default()
    };
    let (ca, cb) = (cut(a), cut(b));
    let r = unsafe { libc::strcoll(ca.as_ptr(), cb.as_ptr()) };
    r.signum()
}

/// PHP_FUNCTION(strcoll)
pub fn strcoll_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = args
        .first()
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    let b = args
        .get(1)
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    let cut = |x: &[u8]| -> CString {
        let end = x.iter().position(|&c| c == 0).unwrap_or(x.len());
        CString::new(&x[..end]).unwrap_or_default()
    };
    let (ca, cb) = (cut(&a), cut(&b));
    Ok(Value::Long(
        unsafe { libc::strcoll(ca.as_ptr(), cb.as_ptr()) } as i64,
    ))
}
