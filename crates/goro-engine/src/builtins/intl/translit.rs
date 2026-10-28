//! `Transliterator` and the `transliterator_*()` functions (PHP manual:
//! Transliterator).

use goro_ext_intl::error as ue;
use goro_ext_intl::translit::{Translit, TranslitError};
use goro_runtime::{ObjRef, PhpArray, Value};

use super::{fail, fail_obj, new_obj, obj_err, reset, reset_obj, store, with_obj};
use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "intl",
    functions: &[
        (
            "function transliterator_create(string $id, int $direction = Transliterator::FORWARD): ?Transliterator",
            t_create,
        ),
        (
            "function transliterator_create_from_rules(string $rules, int $direction = Transliterator::FORWARD): ?Transliterator",
            t_create_from_rules,
        ),
        ("function transliterator_list_ids(): array|false", t_list_ids),
        (
            "function transliterator_create_inverse(Transliterator $transliterator): ?Transliterator",
            t_create_inverse,
        ),
        (
            "function transliterator_transliterate(Transliterator|string $transliterator, string $string, int $start = 0, int $end = -1): string|false",
            t_transliterate,
        ),
        (
            "function transliterator_get_error_code(Transliterator $transliterator): int|false",
            t_error_code,
        ),
        (
            "function transliterator_get_error_message(Transliterator $transliterator): string|false",
            t_error_message,
        ),
    ],
    classes: &[("transliterator", TR_CLASS)],
    methods: &[
        ("transliterator::create", t_create),
        ("transliterator::createfromrules", t_create_from_rules),
        ("transliterator::createinverse", t_create_inverse),
        ("transliterator::listids", t_list_ids),
        ("transliterator::transliterate", t_transliterate),
        ("transliterator::geterrorcode", t_error_code),
        ("transliterator::geterrormessage", t_error_message),
    ],
    hooks: &[("transliterator", hooks)],
    ..Module::EMPTY
};

const TR_CLASS: &str = r#"<?php class Transliterator {
    public const int FORWARD = 0;
    public const int REVERSE = 1;
    public readonly string $id;
    final private function __construct() {}
    public static function create(string $id, int $direction = Transliterator::FORWARD): ?Transliterator {}
    public static function createFromRules(string $rules, int $direction = Transliterator::FORWARD): ?Transliterator {}
    public function createInverse(): ?Transliterator {}
    public static function listIDs(): array|false {}
    public function transliterate(string $string, int $start = 0, int $end = -1): string|false {}
    public function getErrorCode(): int|false {}
    public function getErrorMessage(): string|false {}
}"#;

fn hooks() -> NativeHooks {
    NativeHooks {
        create: Some(super::numfmt::on_create),
        clone: Some(|_, from, to| {
            if let Some(t) = with_obj::<Translit, Translit>(from, |x| x.v.clone()) {
                store(to, t);
            }
        }),
        ..Default::default()
    }
}

fn new_tr(vm: &mut Vm, t: Translit) -> R<Value> {
    let id = t.id.clone();
    let o = new_obj(vm, "Transliterator", t)?;
    vm.set_prop_raw(&o, b"id", Value::from(id.as_str()));
    Ok(Value::Object(o))
}

fn direction(vm: &mut Vm, a: &Args, i: usize) -> R<Option<bool>> {
    match a.int_or(i, 0) {
        0 => Ok(Some(false)),
        1 => Ok(Some(true)),
        _ => vm
            .fthrow(
                a,
                "ValueError",
                &format!(
                    "Argument #{} ($direction) must be either Transliterator::FORWARD or Transliterator::REVERSE",
                    i + 1
                ),
            )
            .map(|()| None),
    }
}

/// Creates by ID, setting the errors.
fn create_by_id(vm: &mut Vm, a: &Args, id: &[u8], reverse: bool) -> R<Option<Translit>> {
    let Ok(id) = std::str::from_utf8(id) else {
        fail(
            vm,
            a,
            ue::U_INVALID_CHAR_FOUND,
            "String conversion of id to UTF-16 failed",
        )?;
        return Ok(None);
    };
    match Translit::create(id, reverse) {
        Ok(t) => Ok(Some(t)),
        Err(_) => {
            fail(
                vm,
                a,
                ue::U_INVALID_ID,
                &format!("unable to open ICU transliterator with id \"{id}\""),
            )?;
            Ok(None)
        }
    }
}

fn t_create(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let Some(rev) = direction(vm, a, 1)? else {
        return Ok(Value::Null);
    };
    match create_by_id(vm, a, &a.bytes(0).to_vec(), rev)? {
        Some(t) => new_tr(vm, t),
        None => Ok(Value::Null),
    }
}

fn t_create_from_rules(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let Some(rev) = direction(vm, a, 1)? else {
        return Ok(Value::Null);
    };
    let Ok(rules) = std::str::from_utf8(a.bytes(0)).map(str::to_string) else {
        fail(
            vm,
            a,
            ue::U_INVALID_CHAR_FOUND,
            "String conversion of rules to UTF-16 failed",
        )?;
        return Ok(Value::Null);
    };
    match Translit::from_rules(&rules, rev) {
        Ok(t) => new_tr(vm, t),
        Err(TranslitError::Rule(code, ctx)) => {
            fail(
                vm,
                a,
                code,
                &format!("unable to create ICU transliterator from rules ({ctx})"),
            )?;
            Ok(Value::Null)
        }
        Err(TranslitError::InvalidId) => {
            fail(
                vm,
                a,
                ue::U_INVALID_ID,
                "unable to create ICU transliterator from rules",
            )?;
            Ok(Value::Null)
        }
    }
}

fn this_tr(vm: &mut Vm, a: &Args) -> R<(ObjRef, usize, Translit)> {
    let (o, off) = super::this_or_first(a);
    match with_obj::<Translit, Translit>(&o, |x| x.v.clone()) {
        Some(t) => Ok((o, off, t)),
        None => vm.throw("Error", "Found unconstructed transliterator"),
    }
}

fn t_create_inverse(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _, t) = this_tr(vm, a)?;
    reset_obj::<Translit>(vm, &o);
    match t.inverse() {
        Ok(i) => new_tr(vm, i),
        Err(_) => {
            fail_obj::<Translit>(
                vm,
                a,
                &o,
                ue::U_INVALID_ID,
                "Could not create inverse ICU transliterator",
            )?;
            Ok(Value::Null)
        }
    }
}

fn t_list_ids(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    reset(vm);
    Ok(Value::Array(PhpArray::from_values(
        goro_ext_intl::translit::list_ids()
            .into_iter()
            .map(|s| Value::from(s.as_str())),
    )))
}

fn t_transliterate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off, t) = if a.this.is_some() {
        let (o, off, t) = this_tr(vm, a)?;
        (Some(o), off, t)
    } else {
        reset(vm);
        match a.val(0) {
            Value::Object(o) => {
                let t = match with_obj::<Translit, Translit>(&o, |x| x.v.clone()) {
                    Some(t) => t,
                    None => return vm.throw("Error", "Found unconstructed transliterator"),
                };
                (Some(o), 1, t)
            }
            v => {
                let id = vm.to_php_str(&v)?;
                match create_by_id(vm, a, id.as_bytes(), false)? {
                    Some(t) => (None, 1, t),
                    None => return Ok(Value::Bool(false)),
                }
            }
        }
    };
    if let Some(o) = &o {
        reset_obj::<Translit>(vm, o);
    }
    let start = a.int_or(off + 1, 0);
    let end = a.int_or(off + 2, -1);
    if start < 0 || (end != -1 && end < 0) {
        let which = if start < 0 { off + 2 } else { off + 3 };
        let name = if start < 0 { "start" } else { "end" };
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #{which} (${name}) must be greater than or equal to 0"),
        );
    }
    if end != -1 && start > end {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} ($string) must be less than or equal to argument #{} ($end)",
                off + 1,
                off + 2
            ),
        );
    }
    let fail_here = |vm: &mut Vm, code: i64, msg: &str| -> R<()> {
        match &o {
            Some(o) => fail_obj::<Translit>(vm, a, o, code, msg),
            None => fail(vm, a, code, msg),
        }
    };
    let Ok(s) = std::str::from_utf8(a.bytes(off)).map(str::to_string) else {
        fail_here(
            vm,
            ue::U_INVALID_CHAR_FOUND,
            "String conversion of string to UTF-16 failed",
        )?;
        return Ok(Value::Bool(false));
    };
    let len = goro_ext_intl::utf16::len16(&s) as i64;
    let end = if end == -1 { len } else { end };
    if start > len || end > len {
        fail_here(
            vm,
            ue::U_ILLEGAL_ARGUMENT_ERROR,
            &format!(
                "Neither \"start\" nor the \"end\" arguments can exceed the number of UTF-16 code units (in this case, {len})"
            ),
        )?;
        return Ok(Value::Bool(false));
    }
    let r = t.apply_range(&s, start as usize, end as usize);
    Ok(Value::from(r.as_str()))
}

fn t_error_code(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = super::this_or_first(a);
    let _ = vm;
    Ok(Value::Int(obj_err::<Translit>(&o).code))
}

fn t_error_message(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = super::this_or_first(a);
    let _ = vm;
    Ok(Value::from(obj_err::<Translit>(&o).full_message().as_str()))
}
