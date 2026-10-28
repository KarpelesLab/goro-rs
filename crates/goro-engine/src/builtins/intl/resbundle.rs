//! `ResourceBundle` and the `resourcebundle_*()` functions (PHP manual:
//! ResourceBundle), over ICU `.res` files in a bundle directory. ICU's
//! own data (a null bundle, ICUDATA) is not available.

use goro_ext_intl::error as ue;
use goro_ext_intl::resbundle::{self as rb, Res};
use goro_runtime::{Key, ObjRef, PhpArray, Value};

use super::{fail, has_obj, new_obj, obj_err, reset, set_error_quiet, store, with_obj};
use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "intl",
    functions: &[
        (
            "function resourcebundle_create(?string $locale, ?string $bundle, bool $fallback = true): ?ResourceBundle",
            r_create,
        ),
        (
            "function resourcebundle_get(ResourceBundle $bundle, string|int $index, bool $fallback = true): mixed",
            r_get,
        ),
        ("function resourcebundle_count(ResourceBundle $bundle): int", r_count),
        (
            "function resourcebundle_locales(string $bundle): array|false",
            r_locales,
        ),
        (
            "function resourcebundle_get_error_code(ResourceBundle $bundle): int",
            r_error_code,
        ),
        (
            "function resourcebundle_get_error_message(ResourceBundle $bundle): string",
            r_error_message,
        ),
    ],
    classes: &[("resourcebundle", RB_CLASS)],
    methods: &[
        ("resourcebundle::__construct", r_construct),
        ("resourcebundle::create", r_create),
        ("resourcebundle::get", r_get),
        ("resourcebundle::count", r_count),
        ("resourcebundle::getlocales", r_locales),
        ("resourcebundle::geterrorcode", r_error_code),
        ("resourcebundle::geterrormessage", r_error_message),
        ("resourcebundle::getiterator", r_iterator),
        ("resourcebundle::offsetget", r_offset_get),
        ("resourcebundle::offsetexists", r_offset_exists),
        ("resourcebundle::offsetset", r_offset_set),
        ("resourcebundle::offsetunset", r_offset_set),
    ],
    hooks: &[("resourcebundle", hooks)],
    ..Module::EMPTY
};

const RB_CLASS: &str = r#"<?php class ResourceBundle implements IteratorAggregate, Countable, ArrayAccess {
    public function __construct(?string $locale, ?string $bundle, bool $fallback = true) {}
    public static function create(?string $locale, ?string $bundle, bool $fallback = true): ?ResourceBundle {}
    public function get(string|int $index, bool $fallback = true): mixed {}
    public function count(): int {}
    public static function getLocales(string $bundle): array|false {}
    public function getErrorCode(): int {}
    public function getErrorMessage(): string {}
    public function getIterator(): Iterator {}
    public function offsetGet(mixed $offset): mixed {}
    public function offsetExists(mixed $offset): bool {}
    public function offsetSet(mixed $offset, mixed $value): void {}
    public function offsetUnset(mixed $offset): void {}
}"#;

/// A bundle or one of its tables or arrays.
#[derive(Clone)]
struct Rb {
    res: Res,
}

fn hooks() -> NativeHooks {
    NativeHooks {
        create: Some(super::numfmt::on_create),
        clone: Some(|_, from, to| {
            if let Some(r) = with_obj::<Rb, Rb>(from, |x| x.v.clone()) {
                store(to, r);
            }
        }),
        count: Some(|_, o| Ok(with_obj::<Rb, i64>(o, |x| len(&x.v.res)).unwrap_or(0))),
        ..Default::default()
    }
}

fn len(r: &Res) -> i64 {
    match r {
        Res::Table(t) => t.len() as i64,
        Res::Array(a) => a.len() as i64,
        Res::IntVector(v) => v.len() as i64,
        _ => 1,
    }
}

/// Opens a bundle: Ok(None) with the error set when it can't be loaded.
fn open(vm: &mut Vm, a: &Args) -> R<Option<(Res, i64)>> {
    let locale = match a.opt_str(0) {
        Some(s) if !s.as_bytes().is_empty() => s.to_string_lossy(),
        _ => super::default_locale(vm),
    };
    let fallback = a.bool_or(2, true);
    let Some(dir) = a.opt_str(1).map(|s| s.to_string_lossy()) else {
        return Ok(None);
    };
    match rb::load(&dir, &locale, fallback) {
        Some(l) => Ok(Some((l.bundle.root.clone(), l.warning))),
        None => Ok(None),
    }
}

fn r_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if has_obj::<Rb>(&o) {
        return vm.throw("Error", "ResourceBundle object is already constructed");
    }
    reset(vm);
    match open(vm, a)? {
        Some((res, w)) => {
            store(&o, Rb { res });
            if w != 0 {
                set_error_quiet(vm, w, String::new());
            }
            Ok(Value::Null)
        }
        None => {
            let msg = format!("{}(): Cannot load libICU resource bundle", a.fname());
            set_error_quiet(vm, ue::U_MISSING_RESOURCE_ERROR, msg.clone());
            vm.throw("IntlException", &msg)
        }
    }
}

fn r_create(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    match open(vm, a)? {
        Some((res, w)) => {
            let o = new_obj(vm, "ResourceBundle", Rb { res })?;
            if w != 0 {
                set_error_quiet(vm, w, String::new());
            }
            Ok(Value::Object(o))
        }
        None => {
            fail(
                vm,
                a,
                ue::U_MISSING_RESOURCE_ERROR,
                "Cannot load libICU resource bundle",
            )?;
            Ok(Value::Null)
        }
    }
}

fn to_php(vm: &mut Vm, r: &Res) -> R<Value> {
    Ok(match r {
        Res::Str(s) => Value::from(s.as_str()),
        Res::Bin(b) => Value::from(b.as_slice()),
        Res::Int(i) => Value::Int(*i),
        Res::IntVector(v) => Value::Array(PhpArray::from_values(v.iter().map(|i| Value::Int(*i)))),
        Res::Table(_) | Res::Array(_) => {
            Value::Object(new_obj(vm, "ResourceBundle", Rb { res: r.clone() })?)
        }
    })
}

fn this_rb(vm: &mut Vm, a: &Args) -> R<(ObjRef, usize, Res)> {
    let (o, off) = super::this_or_first(a);
    match with_obj::<Rb, Res>(&o, |x| x.v.res.clone()) {
        Some(r) => Ok((o, off, r)),
        None => vm.throw("Error", "Found unconstructed ResourceBundle"),
    }
}

/// The element of `res` at `idx` (a key or an index).
fn element(res: &Res, idx: &Value) -> Option<Res> {
    match (res, idx) {
        (Res::Table(t), Value::Str(k)) => {
            let k = k.to_string_lossy();
            t.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone())
        }
        (Res::Table(t), Value::Int(i)) => t.get(usize::try_from(*i).ok()?).map(|(_, v)| v.clone()),
        (Res::Array(v), Value::Int(i)) => v.get(usize::try_from(*i).ok()?).cloned(),
        (Res::IntVector(v), Value::Int(i)) => v.get(usize::try_from(*i).ok()?).map(|x| Res::Int(*x)),
        _ => None,
    }
}

fn get_impl(vm: &mut Vm, a: &Args, prefix: Option<&str>) -> R<Value> {
    let (o, off, res) = this_rb(vm, a)?;
    reset(vm);
    super::set_obj_err::<Rb>(&o, 0, "");
    let idx = a.val(off);
    match element(&res, &idx) {
        Some(e) => to_php(vm, &e),
        None => {
            let shown = match &idx {
                Value::Int(i) => i.to_string(),
                v => vm.to_php_str(v)?.to_string_lossy(),
            };
            let text = format!("Cannot load resource element '{shown}'");
            match prefix {
                Some(p) => {
                    let msg = format!("{p}(): {text}");
                    super::set_obj_err::<Rb>(&o, ue::U_MISSING_RESOURCE_ERROR, &msg);
                    super::fail_msg(vm, ue::U_MISSING_RESOURCE_ERROR, msg)?;
                }
                None => super::fail_obj::<Rb>(vm, a, &o, ue::U_MISSING_RESOURCE_ERROR, &text)?,
            }
            Ok(Value::Null)
        }
    }
}

fn r_get(vm: &mut Vm, a: &mut Args) -> R<Value> {
    get_impl(vm, a, None)
}

fn r_offset_get(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match a.val(0) {
        Value::Null => vm.throw("Error", "Cannot apply [] to ResourceBundle object"),
        Value::Float(_) => vm.type_error("Cannot access offset of type float on ResourceBundle"),
        Value::Object(o) => {
            let n = vm.class_of(&o).name.to_string_lossy();
            vm.type_error(&format!("Cannot access offset of type {n} on ResourceBundle"))
        }
        Value::Str(s) if s.as_bytes().is_empty() => vm.value_error("Offset must not be empty"),
        Value::Int(i) if i32::try_from(i).is_err() => {
            vm.value_error("Index must be between -2147483648 and 2147483647")
        }
        // the dimension read of PHP's handler reports as main()
        _ => get_impl(vm, a, Some("main")),
    }
}

fn r_offset_exists(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.throw("Error", "Cannot use object of type ResourceBundle as array")
}

fn r_offset_set(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.throw("Error", "Cannot use object of type ResourceBundle as array")
}

fn r_count(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, res) = this_rb(vm, a)?;
    Ok(Value::Int(len(&res)))
}

fn r_locales(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset(vm);
    let dir = a.str(0).to_string_lossy();
    match rb::locales(&dir) {
        Some(l) => Ok(Value::Array(PhpArray::from_values(
            l.iter().map(|s| Value::from(s.as_str())),
        ))),
        None => {
            fail(
                vm,
                a,
                ue::U_MISSING_RESOURCE_ERROR,
                "Cannot fetch locales list",
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn r_error_code(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = super::this_or_first(a);
    let _ = vm;
    Ok(Value::Int(obj_err::<Rb>(&o).code))
}

fn r_error_message(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, _) = super::this_or_first(a);
    let _ = vm;
    Ok(Value::from(obj_err::<Rb>(&o).full_message().as_str()))
}

fn r_iterator(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, res) = this_rb(vm, a)?;
    let mut items = Vec::new();
    match &res {
        Res::Table(t) => {
            for (k, v) in t {
                let val = to_php(vm, v)?;
                items.push((Value::from(k.as_str()), val));
            }
        }
        Res::Array(v) => {
            for (i, x) in v.iter().enumerate() {
                let val = to_php(vm, x)?;
                items.push((Value::Int(i as i64), val));
            }
        }
        _ => {}
    }
    let _ = Key::Int(0);
    let it = super::iter::new_iterator(vm, "IntlIterator", items, None, Vec::new())?;
    Ok(Value::Object(it))
}
