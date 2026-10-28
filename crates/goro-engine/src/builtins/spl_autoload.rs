//! SPL autoloading (PHP manual: spl_autoload_register(),
//! spl_autoload_unregister(), spl_autoload_functions(), spl_autoload_call(),
//! spl_autoload(), spl_autoload_extensions()). Autoloaders are resolved
//! when they are registered: the called scope of a static method is the
//! registration's (test ext/spl/tests/spl_autoload_called_scope).

use goro_runtime::{ObjRef, PhpArray, PhpStr, Value};

use crate::func::Args;
use crate::vm::calls::Callee;
use crate::vm::{R, Vm};

/// SPL's autoload state: how each registered autoloader is listed by
/// spl_autoload_functions() (parallel to `Vm::autoloaders`), and the
/// extensions spl_autoload() tries.
#[derive(Default)]
struct AutoloadState {
    shown: Vec<Value>,
    extensions: Option<PhpStr>,
}

const DEFAULT_EXTENSIONS: &str = ".inc,.php";

/// What identifies a registered autoloader (the same callable is
/// registered once).
fn same(a: &Callee, b: &Callee) -> bool {
    let obj_eq = |x: &Option<ObjRef>, y: &Option<ObjRef>| match (x, y) {
        (Some(x), Some(y)) => ObjRef::ptr_eq(x, y),
        (None, None) => true,
        _ => false,
    };
    std::rc::Rc::ptr_eq(&a.func, &b.func)
        && obj_eq(&a.this, &b.this)
        && obj_eq(&a.closure, &b.closure)
        && (a.this.is_some() || a.closure.is_some() || std::ptr::eq(a.static_class, b.static_class))
        && a.magic_name == b.magic_name
}

/// How spl_autoload_functions() shows an autoloader: closures as
/// themselves, methods as [object or class, name], functions by name.
fn shown(c: &Callee) -> Value {
    if let Some(cl) = &c.closure
        && c.magic_name.is_none()
    {
        return Value::Object(cl.clone());
    }
    let name = c.magic_name.clone().unwrap_or_else(|| c.func.name.clone());
    if let Some(t) = &c.this {
        return Value::Array(PhpArray::from_values([
            Value::Object(t.clone()),
            Value::Str(name),
        ]));
    }
    let cls = if c.static_class.is_null() {
        c.func.scope.get()
    } else {
        c.static_class
    };
    if cls.is_null() {
        return Value::Str(name);
    }
    let cname = unsafe { (*cls).name.clone() };
    Value::Array(PhpArray::from_values([Value::Str(cname), Value::Str(name)]))
}

fn is_named(v: &Value, name: &[u8]) -> bool {
    matches!(v.deref(), Value::Str(s) if s.trim_ascii().eq_ignore_ascii_case(name)
        || s.strip_prefix(b"\\").is_some_and(|s| s.eq_ignore_ascii_case(name)))
}

fn spl_autoload_register(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = if a.has(0) { a.val(0) } else { Value::Null };
    if a.has(1) && !a.bool_or(1, true) {
        vm.notice_msg("spl_autoload_register(): Argument #2 ($do_throw) has been ignored, spl_autoload_register() will always throw")?;
    }
    let prepend = a.bool_or(2, false);
    let f = if f.is_null() {
        Value::Str(PhpStr::from("spl_autoload"))
    } else {
        f
    };
    if is_named(&f, b"spl_autoload_call") {
        // test spl_autoload_throw_with_spl_autoloader_call_as_autoloader
        return vm.value_error(
            "spl_autoload_register(): Argument #1 ($callback) must not be the spl_autoload_call() function",
        );
    }
    let c = vm.resolve_callable(&f, false)?;
    if vm.autoloaders.iter().any(|x| same(x, &c)) {
        return Ok(Value::Bool(true));
    }
    // Invokable objects are listed as themselves (test spl_autoload_013).
    let s = match f.deref() {
        o @ Value::Object(_) => o,
        _ => shown(&c),
    };
    if prepend {
        vm.autoloaders.insert(0, c);
        vm.ext::<AutoloadState>().shown.insert(0, s);
    } else {
        vm.autoloaders.push(c);
        vm.ext::<AutoloadState>().shown.push(s);
    }
    Ok(Value::Bool(true))
}

fn spl_autoload_unregister(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.val(0);
    if is_named(&f, b"spl_autoload_call") {
        // test spl_autoload_unregister_without_registrations
        vm.deprecated("spl_autoload_unregister(): Using spl_autoload_call() as a callback for spl_autoload_unregister() is deprecated, to remove all registered autoloaders, call spl_autoload_unregister() for all values returned from spl_autoload_functions()")?;
        vm.autoloaders.clear();
        vm.ext::<AutoloadState>().shown.clear();
        return Ok(Value::Bool(true));
    }
    let c = vm.resolve_callable(&f, false)?;
    match vm.autoloaders.iter().position(|x| same(x, &c)) {
        Some(i) => {
            let old = vm.autoloaders.remove(i);
            drop(old);
            let st = vm.ext::<AutoloadState>();
            if i < st.shown.len() {
                let old = st.shown.remove(i);
                drop(old);
            }
            Ok(Value::Bool(true))
        }
        None => Ok(Value::Bool(false)),
    }
}

fn spl_autoload_functions(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let shown = vm.ext::<AutoloadState>().shown.clone();
    Ok(Value::Array(PhpArray::from_values(shown)))
}

fn spl_autoload_call(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = a.str(0);
    vm.find_class(&n, true)?;
    Ok(Value::Null)
}

fn spl_autoload_extensions(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if let Some(e) = a.opt_str(0) {
        vm.ext::<AutoloadState>().extensions = Some(e);
    }
    let st = vm.ext::<AutoloadState>();
    Ok(Value::Str(
        st.extensions
            .clone()
            .unwrap_or_else(|| PhpStr::from(DEFAULT_EXTENSIONS)),
    ))
}

/// The default autoloader: the lower-cased class name (namespace
/// separators as directories) plus each extension, found through
/// include_path and run once, until the class exists (PHP manual:
/// spl_autoload(); test spl_autoload_001).
fn spl_autoload(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let class = a.str(0);
    let exts = match a.opt_str(1) {
        Some(e) => e,
        None => {
            let st = vm.ext::<AutoloadState>();
            st.extensions
                .clone()
                .unwrap_or_else(|| PhpStr::from(DEFAULT_EXTENSIONS))
        }
    };
    let base: Vec<u8> = class
        .iter()
        .map(|&c| {
            if c == b'\\' {
                b'/'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect();
    let lname = crate::class::lower(&class);
    for ext in exts.split(|&c| c == b',') {
        let mut file = base.clone();
        file.extend_from_slice(ext);
        if vm.include_once_quiet(&file)? && vm.classes.contains_key(&lname) {
            break;
        }
    }
    Ok(Value::Null)
}

pub(crate) const FUNCTIONS: &[(&str, crate::func::NativeFn)] = &[
    (
        "function spl_autoload_register(?callable $callback = null, bool $throw = true, bool $prepend = false): bool",
        spl_autoload_register,
    ),
    (
        "function spl_autoload_unregister(callable $callback): bool",
        spl_autoload_unregister,
    ),
    (
        "function spl_autoload_functions(): array",
        spl_autoload_functions,
    ),
    (
        "function spl_autoload_call(string $class): void",
        spl_autoload_call,
    ),
    (
        "function spl_autoload_extensions(?string $file_extensions = null): string",
        spl_autoload_extensions,
    ),
    (
        "function spl_autoload(string $class, ?string $file_extensions = null): void",
        spl_autoload,
    ),
];
