//! `Collator` and the `collator_*()` functions (PHP manual: Collator).

use std::cmp::Ordering;

use goro_ext_intl::collator::{self as co, Collator};
use goro_ext_intl::error as ue;
use goro_runtime::{PhpArray, PhpStr, Value};

use super::locale::MAX_LOCALE_LEN;
use super::{
    IntlState, already_constructed, fail, fail_obj, has_obj, new_obj, obj_err, reset, reset_obj,
    store, this_or_first, with_obj,
};
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "intl",
    functions: &[
        ("function collator_create(string $locale): ?Collator", c_create),
        (
            "function collator_compare(Collator $object, string $string1, string $string2): int|false",
            c_compare,
        ),
        (
            "function collator_get_attribute(Collator $object, int $attribute): int|false",
            c_get_attribute,
        ),
        (
            "function collator_set_attribute(Collator $object, int $attribute, int $value): bool",
            c_set_attribute,
        ),
        (
            "function collator_get_strength(Collator $object): int",
            c_get_strength,
        ),
        (
            "function collator_set_strength(Collator $object, int $strength): true",
            c_set_strength,
        ),
        (
            "function collator_sort(Collator $object, array &$array, int $flags = Collator::SORT_REGULAR): bool",
            c_sort,
        ),
        (
            "function collator_sort_with_sort_keys(Collator $object, array &$array): bool",
            c_sort_with_sort_keys,
        ),
        (
            "function collator_asort(Collator $object, array &$array, int $flags = Collator::SORT_REGULAR): bool",
            c_asort,
        ),
        (
            "function collator_get_locale(Collator $object, int $type): string|false",
            c_get_locale,
        ),
        (
            "function collator_get_error_code(Collator $object): int|false",
            c_get_error_code,
        ),
        (
            "function collator_get_error_message(Collator $object): string|false",
            c_get_error_message,
        ),
        (
            "function collator_get_sort_key(Collator $object, string $string): string|false",
            c_get_sort_key,
        ),
    ],
    classes: &[("collator", COLLATOR_CLASS)],
    methods: &[
        ("collator::__construct", c_construct),
        ("collator::create", c_create),
        ("collator::compare", c_compare),
        ("collator::sort", c_sort),
        ("collator::sortwithsortkeys", c_sort_with_sort_keys),
        ("collator::asort", c_asort),
        ("collator::getattribute", c_get_attribute),
        ("collator::setattribute", c_set_attribute),
        ("collator::getstrength", c_get_strength),
        ("collator::setstrength", c_set_strength),
        ("collator::getlocale", c_get_locale),
        ("collator::geterrorcode", c_get_error_code),
        ("collator::geterrormessage", c_get_error_message),
        ("collator::getsortkey", c_get_sort_key),
    ],
    ..Module::EMPTY
};

const COLLATOR_CLASS: &str = r#"<?php class Collator {
    public const int DEFAULT_VALUE = -1;
    public const int PRIMARY = 0;
    public const int SECONDARY = 1;
    public const int TERTIARY = 2;
    public const int DEFAULT_STRENGTH = 2;
    public const int QUATERNARY = 3;
    public const int IDENTICAL = 15;
    public const int OFF = 16;
    public const int ON = 17;
    public const int SHIFTED = 20;
    public const int NON_IGNORABLE = 21;
    public const int LOWER_FIRST = 24;
    public const int UPPER_FIRST = 25;
    public const int FRENCH_COLLATION = 0;
    public const int ALTERNATE_HANDLING = 1;
    public const int CASE_FIRST = 2;
    public const int CASE_LEVEL = 3;
    public const int NORMALIZATION_MODE = 4;
    public const int STRENGTH = 5;
    public const int HIRAGANA_QUATERNARY_MODE = 6;
    public const int NUMERIC_COLLATION = 7;
    public const int SORT_REGULAR = 0;
    public const int SORT_STRING = 1;
    public const int SORT_NUMERIC = 2;
    public function __construct(string $locale) {}
    public static function create(string $locale): ?Collator {}
    public function compare(string $string1, string $string2): int|false {}
    public function sort(array &$array, int $flags = Collator::SORT_REGULAR): bool {}
    public function sortWithSortKeys(array &$array): bool {}
    public function asort(array &$array, int $flags = Collator::SORT_REGULAR): bool {}
    public function getAttribute(int $attribute): int|false {}
    public function setAttribute(int $attribute, int $value): bool {}
    public function getStrength(): int {}
    public function setStrength(int $strength): true {}
    public function getLocale(int $type): string|false {}
    public function getErrorCode(): int|false {}
    public function getErrorMessage(): string|false {}
    public function getSortKey(string $string): string|false {}
}"#;

/// Builds a collator for `locale`, or the error message.
fn make(vm: &mut Vm, locale: &PhpStr) -> Result<Collator, &'static str> {
    let l = locale.to_string_lossy();
    if l.len() > MAX_LOCALE_LEN {
        return Err("Locale string too long, should be no longer than 156 characters");
    }
    let l = if l.is_empty() {
        super::default_locale(vm)
    } else {
        l
    };
    if goro_ext_intl::locale::parse(&l).bad_keywords {
        return Err("unable to open ICU collator");
    }
    Ok(Collator::new(&l))
}

fn c_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if has_obj::<Collator>(&o) {
        return already_constructed(vm, "Collator");
    }
    reset(vm);
    match make(vm, &a.str(0)) {
        Ok(c) => {
            store(&o, c);
            Ok(Value::Null)
        }
        Err(m) => {
            let msg = format!("{}(): {}", a.fname(), m);
            super::set_error_quiet(vm, ue::U_ILLEGAL_ARGUMENT_ERROR, msg.clone());
            vm.throw("IntlException", &msg)
        }
    }
}

fn c_create(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    match make(vm, &a.str(0)) {
        Ok(c) => Ok(Value::Object(new_obj(vm, "Collator", c)?)),
        Err(m) => {
            fail(vm, a, ue::U_ILLEGAL_ARGUMENT_ERROR, m)?;
            Ok(Value::Null)
        }
    }
}

/// The collator of the call (`$this` or the first argument) with the
/// offset of the other arguments.
fn coll(vm: &mut Vm, a: &Args) -> R<(goro_runtime::ObjRef, usize, Collator)> {
    let (o, off) = this_or_first(a);
    match with_obj::<Collator, Collator>(&o, |x| x.v.clone()) {
        Some(c) => Ok((o, off, c)),
        None => vm.throw("Error", "Object not initialized"),
    }
}

/// Argument `i` as UTF-8 text (None for invalid UTF-8, after setting the
/// error).
fn text(
    vm: &mut Vm,
    a: &Args,
    o: &goro_runtime::ObjRef,
    i: usize,
    which: &str,
) -> R<Option<String>> {
    match std::str::from_utf8(a.bytes(i)) {
        Ok(s) => Ok(Some(s.to_string())),
        Err(_) => {
            fail_obj::<Collator>(
                vm,
                a,
                o,
                ue::U_INVALID_CHAR_FOUND,
                &format!("Error converting {which} argument to UTF-16"),
            )?;
            Ok(None)
        }
    }
}

fn c_compare(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, c) = coll(vm, a)?;
    reset_obj::<Collator>(vm, &o);
    let Some(x) = text(vm, a, &o, off, "first")? else {
        return Ok(Value::Bool(false));
    };
    let Some(y) = text(vm, a, &o, off + 1, "second")? else {
        return Ok(Value::Bool(false));
    };
    Ok(Value::Int(match c.compare(&x, &y) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }))
}

fn c_get_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, c) = coll(vm, a)?;
    reset_obj::<Collator>(vm, &o);
    match c.attribute(a.int(off)) {
        Some(v) => Ok(Value::Int(v)),
        None => {
            fail_obj::<Collator>(
                vm,
                a,
                &o,
                ue::U_ILLEGAL_ARGUMENT_ERROR,
                "Error getting attribute value",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn c_set_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, _) = coll(vm, a)?;
    reset_obj::<Collator>(vm, &o);
    let (attr, val) = (a.int(off), a.int(off + 1));
    let ok = with_obj::<Collator, bool>(&o, |x| x.v.set_attribute(attr, val)).unwrap_or(false);
    if !ok {
        fail_obj::<Collator>(
            vm,
            a,
            &o,
            ue::U_ILLEGAL_ARGUMENT_ERROR,
            "Error setting attribute value",
        )?;
    }
    Ok(Value::Bool(ok))
}

fn c_get_strength(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _, c) = coll(vm, a)?;
    reset_obj::<Collator>(vm, &o);
    Ok(Value::Int(c.strength()))
}

fn c_set_strength(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, _) = coll(vm, a)?;
    reset_obj::<Collator>(vm, &o);
    let s = a.int(off);
    let ok = with_obj::<Collator, bool>(&o, |x| x.v.set_attribute(co::STRENGTH, s)).unwrap_or(false);
    if !ok {
        fail_obj::<Collator>(
            vm,
            a,
            &o,
            ue::U_ILLEGAL_ARGUMENT_ERROR,
            "Error setting attribute value",
        )?;
    }
    Ok(Value::Bool(true))
}

fn c_get_locale(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, c) = coll(vm, a)?;
    reset_obj::<Collator>(vm, &o);
    match a.int(off) {
        0 => Ok(Value::from(c.actual.as_str())),
        1 => Ok(Value::from(c.valid.as_str())),
        _ => {
            fail_obj::<Collator>(
                vm,
                a,
                &o,
                ue::U_ILLEGAL_ARGUMENT_ERROR,
                "Error getting locale by type",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn c_get_error_code(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_or_first(a);
    let _ = vm;
    Ok(Value::Int(obj_err::<Collator>(&o).code))
}

fn c_get_error_message(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = this_or_first(a);
    let _ = vm;
    Ok(Value::from(obj_err::<Collator>(&o).full_message().as_str()))
}

fn c_get_sort_key(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, c) = coll(vm, a)?;
    reset_obj::<Collator>(vm, &o);
    let Some(s) = text(vm, a, &o, off, "first")? else {
        return Ok(Value::Bool(false));
    };
    let mut k = c.sort_key_bytes(&s);
    k.push(0);
    Ok(Value::from(k.as_slice()))
}

// ---------------------------------------------------------------- sorting

/// A value as the comparison sees it.
enum SortVal {
    Num(f64),
    Str(String),
}

fn sort_val(vm: &mut Vm, v: &Value, flags: i64) -> R<SortVal> {
    let v = v.deref();
    if flags == 2 {
        // SORT_NUMERIC
        let f = match &v {
            Value::Int(i) => *i as f64,
            Value::Float(f) => *f,
            Value::Str(s) => goro_runtime::convert::str_to_float(s.as_bytes()),
            Value::Bool(b) => *b as i64 as f64,
            _ => 0.0,
        };
        return Ok(SortVal::Num(f));
    }
    if flags == 0 {
        match &v {
            Value::Int(i) => return Ok(SortVal::Num(*i as f64)),
            Value::Float(f) => return Ok(SortVal::Num(*f)),
            Value::Str(s) if goro_runtime::numeric::is_numeric_str(s.as_bytes()) => {
                return Ok(SortVal::Num(goro_runtime::convert::str_to_float(
                    s.as_bytes(),
                )));
            }
            _ => {}
        }
    }
    let s = vm.to_php_str(&v)?;
    Ok(SortVal::Str(s.to_string_lossy()))
}

fn cmp_vals(c: &Collator, x: &SortVal, y: &SortVal) -> Ordering {
    match (x, y) {
        (SortVal::Num(a), SortVal::Num(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
        (SortVal::Str(a), SortVal::Str(b)) => c.compare(a, b),
        (SortVal::Num(a), SortVal::Str(b)) => c.compare(&num_str(*a), b),
        (SortVal::Str(a), SortVal::Num(b)) => c.compare(a, &num_str(*b)),
    }
}

fn num_str(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        format!("{f}")
    }
}

fn do_sort(vm: &mut Vm, a: &Args, keep_keys: bool, with_keys_flag: bool) -> R<Value> {
    let (o, off, c) = coll(vm, a)?;
    reset_obj::<Collator>(vm, &o);
    let flags = if with_keys_flag {
        1
    } else {
        a.int_or(off + 1, 0)
    };
    let Some(r) = a.ref_at(off) else {
        return Ok(Value::Bool(false));
    };
    let arr = match r.get() {
        Value::Array(x) => x,
        _ => return Ok(Value::Bool(false)),
    };
    let mut items = Vec::with_capacity(arr.len());
    for (k, v) in arr.iter() {
        let sv = sort_val(vm, &v.deref(), flags)?;
        items.push((k.to_owned(), v.deref(), sv));
    }
    // a stable sort (PHP 8's sorts are stable)
    items.sort_by(|x, y| cmp_vals(&c, &x.2, &y.2));
    let mut out = PhpArray::new();
    for (k, v, _) in items {
        if keep_keys {
            out.insert(k, v);
        } else {
            let _ = out.push(v);
        }
    }
    drop(r.set(Value::Array(out)));
    Ok(Value::Bool(true))
}

fn c_sort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    do_sort(vm, a, false, false)
}

fn c_asort(vm: &mut Vm, a: &mut Args) -> R<Value> {
    do_sort(vm, a, true, false)
}

fn c_sort_with_sort_keys(vm: &mut Vm, a: &mut Args) -> R<Value> {
    do_sort(vm, a, false, true)
}

#[allow(dead_code)]
fn state(vm: &mut Vm) -> &mut IntlState {
    vm.ext::<IntlState>()
}
