//! `Spoofchecker` (PHP manual: Spoofchecker).

use goro_ext_intl::spoof::{self as sp, Allowed, Checker};
use goro_runtime::Value;

use super::{store, with_obj};
use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "intl",
    classes: &[("spoofchecker", SPOOF_CLASS)],
    methods: &[
        ("spoofchecker::__construct", s_construct),
        ("spoofchecker::issuspicious", s_is_suspicious),
        ("spoofchecker::areconfusable", s_are_confusable),
        ("spoofchecker::setallowedlocales", s_set_locales),
        ("spoofchecker::setchecks", s_set_checks),
        ("spoofchecker::setrestrictionlevel", s_set_level),
        ("spoofchecker::setallowedchars", s_set_chars),
    ],
    hooks: &[("spoofchecker", hooks)],
    ..Module::EMPTY
};

const SPOOF_CLASS: &str = r#"<?php class Spoofchecker {
    public const int SINGLE_SCRIPT_CONFUSABLE = 1;
    public const int MIXED_SCRIPT_CONFUSABLE = 2;
    public const int WHOLE_SCRIPT_CONFUSABLE = 4;
    public const int ANY_CASE = 8;
    public const int SINGLE_SCRIPT = 16;
    public const int INVISIBLE = 32;
    public const int CHAR_LIMIT = 64;
    public const int ASCII = 268435456;
    public const int HIGHLY_RESTRICTIVE = 805306368;
    public const int MODERATELY_RESTRICTIVE = 1073741824;
    public const int MINIMALLY_RESTRICTIVE = 1342177280;
    public const int UNRESTRICTIVE = 1610612736;
    public const int SINGLE_SCRIPT_RESTRICTIVE = 536870912;
    public const int MIXED_NUMBERS = 128;
    public const int HIDDEN_OVERLAY = 256;
    public const int IGNORE_SPACE = 1;
    public const int CASE_INSENSITIVE = 2;
    public const int ADD_CASE_MAPPINGS = 4;
    public const int SIMPLE_CASE_INSENSITIVE = 6;
    public function __construct() {}
    public function isSuspicious(string $string, &$errorCode = null): bool {}
    public function areConfusable(string $string1, string $string2, &$errorCode = null): bool {}
    public function setAllowedLocales(string $locales): void {}
    public function setChecks(int $checks): void {}
    public function setRestrictionLevel(int $level): void {}
    public function setAllowedChars(string $pattern, int $patternOptions = 0): void {}
}"#;

fn hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|vm, o| {
            super::numfmt::on_create(vm, o);
            store(o, Checker::default());
        }),
        clone: Some(|_, from, to| {
            if let Some(c) = with_obj::<Checker, Checker>(from, |x| x.v.clone()) {
                store(to, c);
            }
        }),
        ..Default::default()
    }
}

fn s_construct(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    store(a.this_obj(), Checker::default());
    Ok(Value::Null)
}

fn checker(a: &Args) -> Checker {
    with_obj::<Checker, Checker>(a.this_obj(), |x| x.v.clone()).unwrap_or_default()
}

fn set_out(vm: &mut Vm, a: &Args, i: usize, v: i64) -> R<()> {
    if a.has(i) {
        super::timezone::set_ref_typed(vm, a, i, Value::Int(v))?;
    }
    Ok(())
}

fn s_is_suspicious(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = checker(a);
    let s = String::from_utf8_lossy(a.bytes(0)).into_owned();
    let r = c.check(&s);
    set_out(vm, a, 1, r)?;
    Ok(Value::Bool(r != 0))
}

fn s_are_confusable(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = checker(a);
    let x = String::from_utf8_lossy(a.bytes(0)).into_owned();
    let y = String::from_utf8_lossy(a.bytes(1)).into_owned();
    let r = c.confusable(&x, &y);
    set_out(vm, a, 2, r)?;
    Ok(Value::Bool(r != 0))
}

fn s_set_locales(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let list = a.str(0).to_string_lossy();
    let mut scripts = Vec::new();
    for l in list.split(',') {
        for s in sp::locale_scripts(l.trim()) {
            if !scripts.contains(&s) {
                scripts.push(s);
            }
        }
    }
    with_obj::<Checker, ()>(a.this_obj(), |x| {
        x.v.allowed = Some(Allowed::Scripts(scripts));
        x.v.checks |= sp::CHAR_LIMIT;
    });
    Ok(Value::Null)
}

fn s_set_checks(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = a.int(0);
    with_obj::<Checker, ()>(a.this_obj(), |x| x.v.checks = c);
    Ok(Value::Null)
}

fn s_set_level(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let l = a.int(0);
    if ![
        sp::ASCII,
        sp::SINGLE_SCRIPT_RESTRICTIVE,
        sp::HIGHLY_RESTRICTIVE,
        sp::MODERATELY_RESTRICTIVE,
        sp::MINIMALLY_RESTRICTIVE,
        sp::UNRESTRICTIVE,
    ]
    .contains(&l)
    {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($level) must be one of Spoofchecker::ASCII, Spoofchecker::SINGLE_SCRIPT_RESTRICTIVE, Spoofchecker::HIGHLY_RESTRICTIVE, Spoofchecker::MODERATELY_RESTRICTIVE, Spoofchecker::MINIMALLY_RESTRICTIVE, or Spoofchecker::UNRESTRICTIVE",
        );
    }
    with_obj::<Checker, ()>(a.this_obj(), |x| {
        x.v.level = l;
        x.v.checks |= sp::RESTRICTION_LEVEL;
    });
    Ok(Value::Null)
}

fn s_set_chars(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let opts = a.int_or(1, 0);
    // 0, IGNORE_SPACE, and one of the case options
    let case = opts & !1;
    if !matches!(case, 0 | 2 | 4 | 6) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($patternOptions) must be a valid pattern option, 0 or (SpoofChecker::IGNORE_SPACE|(<none> or SpoofChecker::USET_CASE_INSENSITIVE or SpoofChecker::USET_ADD_CASE_MAPPINGS or SpoofChecker::USET_SIMPLE_CASE_INSENSITIVE))",
        );
    }
    let p = String::from_utf8_lossy(a.bytes(0)).into_owned();
    let set = match goro_ext_intl::uset::parse_str(&p) {
        Ok(s) => s,
        Err(_) => {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #1 ($pattern) must be a valid regular expression character set pattern",
            );
        }
    };
    with_obj::<Checker, ()>(a.this_obj(), |x| {
        x.v.allowed = Some(Allowed::Set(set, case != 0));
        x.v.checks |= sp::CHAR_LIMIT;
    });
    Ok(Value::Null)
}
