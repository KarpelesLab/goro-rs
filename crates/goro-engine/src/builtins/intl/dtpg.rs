//! `IntlDatePatternGenerator` (PHP manual: IntlDatePatternGenerator).
//! Skeleton matching is goro_ext_intl::dtpg's.

use goro_ext_intl::datefmt::LocaleId;
use goro_ext_intl::error as ue;
use goro_runtime::Value;

use super::timezone::clone_error;
use super::{already_constructed, fail_obj, has_obj, reset, reset_obj, this_or_first, with_obj};
use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "intl",
    classes: &[("intldatepatterngenerator", DTPG_CLASS)],
    methods: &[
        ("intldatepatterngenerator::__construct", g_construct),
        ("intldatepatterngenerator::create", g_create),
        ("intldatepatterngenerator::getbestpattern", g_best),
    ],
    hooks: &[("intldatepatterngenerator", g_hooks)],
    ..Module::EMPTY
};

const DTPG_CLASS: &str = r#"<?php class IntlDatePatternGenerator {
    public function __construct(?string $locale = null) {}
    public static function create(?string $locale = null): ?IntlDatePatternGenerator {}
    public function getBestPattern(string $skeleton): string|false {}
}"#;

/// The generator's locale (ICU base id).
#[derive(Clone)]
struct Gen(String);

fn g_hooks() -> NativeHooks {
    NativeHooks {
        clone: Some(
            |vm, from, to| match with_obj::<Gen, Gen>(from, |x| x.v.clone()) {
                Some(g) => super::store(to, g),
                None => clone_error(vm, "Cannot clone uninitialized IntlDatePatternGenerator"),
            },
        ),
        ..Default::default()
    }
}

fn locale_of(vm: &mut Vm, a: &Args) -> String {
    let l = match a.opt_str(0) {
        Some(s) if !s.as_bytes().is_empty() => s.to_string_lossy(),
        _ => super::default_locale(vm),
    };
    LocaleId::parse(&l).base
}

fn g_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if has_obj::<Gen>(&o) {
        return already_constructed(vm, "IntlDatePatternGenerator");
    }
    reset(vm);
    let l = locale_of(vm, a);
    super::store(&o, Gen(l));
    Ok(Value::Null)
}

fn g_create(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let l = locale_of(vm, a);
    let cls = super::static_class(vm, a);
    let o = vm.instantiate_raw(&cls)?;
    super::store(&o, Gen(l));
    Ok(Value::Object(o))
}

fn g_best(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, off) = this_or_first(a);
    let Some(g) = with_obj::<Gen, Gen>(&o, |x| x.v.clone()) else {
        return vm.throw("Error", "Found unconstructed IntlDatePatternGenerator");
    };
    reset_obj::<Gen>(vm, &o);
    let Ok(sk) = std::str::from_utf8(a.bytes(off)) else {
        fail_obj::<Gen>(
            vm,
            a,
            &o,
            ue::U_INVALID_CHAR_FOUND,
            "Skeleton is not a valid UTF-8 string",
        )?;
        return Ok(Value::Bool(false));
    };
    let p = goro_ext_intl::dtpg::best_pattern(&g.0, sk);
    Ok(Value::from(p.as_str()))
}
