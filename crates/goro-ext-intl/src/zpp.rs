//! zend_parse_parameters for the intl functions and methods, driven by
//! their stubs (the C arginfo is generated from them and php-src keeps the
//! two in sync): argument count, scalar coercion (weak and strict mode),
//! null deprecations, object parameters (instanceof the declared classes)
//! and the defaults of omitted optional parameters.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::arginfo::{
    T_ARRAY, T_BOOL, T_CALLABLE, T_FALSE, T_FLOAT, T_INT, T_MIXED, T_NULL, T_OBJECT, T_STRING,
    T_TRUE, type_display, type_mask,
};
use goro_core::object::PhpObject;
use goro_core::stubs::{A_BY_REF, A_VARIADIC, StubFunc};
use goro_core::value::Value;
use goro_core::vm::Vm;

pub type Obj = Rc<RefCell<PhpObject>>;

/// Throw `class` with `msg` (pending in current_exception).
pub fn throw(vm: &mut Vm, class: &[u8], msg: &str) {
    if vm.current_exception.is_some() {
        return;
    }
    let e = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(e);
}

/// instanceof for an object and a class name (any case).
pub fn obj_instanceof(vm: &Vm, o: &Obj, class: &str) -> bool {
    let cn = o.borrow().class_name.to_ascii_lowercase();
    class_instanceof(vm, &cn, class)
}

/// instanceof for a lowercase class name and a class name (any case).
pub fn class_instanceof(vm: &Vm, class_lower: &[u8], target: &str) -> bool {
    let t = target.trim_start_matches('\\').to_ascii_lowercase();
    if class_lower == t.as_bytes() {
        return true;
    }
    vm.class_extends(class_lower, t.as_bytes())
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// zend_wrong_parameters_count_error()
pub fn count_error(vm: &mut Vm, name: &str, min: usize, max: Option<usize>, given: usize) {
    let msg = if max == Some(min) {
        format!(
            "{}() expects exactly {} argument{}, {} given",
            name,
            min,
            plural(min),
            given
        )
    } else if given < min {
        format!(
            "{}() expects at least {} argument{}, {} given",
            name,
            min,
            plural(min),
            given
        )
    } else {
        let m = max.unwrap_or(min);
        format!(
            "{}() expects at most {} argument{}, {} given",
            name,
            m,
            plural(m),
            given
        )
    };
    throw(vm, b"ArgumentCountError", &msg);
}

/// The class names of a stub type.
fn type_classes(ty: &str) -> Vec<&str> {
    let t = ty.trim().trim_start_matches('?');
    t.split('|')
        .map(|p| p.trim().trim_matches(|c| c == '(' || c == ')'))
        .filter(|p| {
            !p.is_empty()
                && !matches!(
                    p.to_ascii_lowercase().as_str(),
                    "null"
                        | "false"
                        | "true"
                        | "bool"
                        | "int"
                        | "float"
                        | "string"
                        | "array"
                        | "object"
                        | "callable"
                        | "mixed"
                        | "void"
                        | "never"
                        | "static"
                        | "self"
                        | "iterable"
                )
        })
        .collect()
}

/// Parsed arguments of a call.
pub struct Args {
    /// One value per declared parameter (variadic ones excluded): the
    /// coerced argument, the stub default of an omitted one, or Undef when
    /// omitted without a known default. By-reference parameters keep the
    /// reference.
    pub v: Vec<Value>,
    /// The variadic rest, as passed.
    pub rest: Vec<Value>,
    /// Number of arguments passed.
    pub given: usize,
}

impl Args {
    pub fn get(&self, i: usize) -> Value {
        self.v.get(i).cloned().unwrap_or(Value::Undef)
    }
    /// Whether argument `i` was passed (not omitted).
    pub fn passed(&self, i: usize) -> bool {
        i < self.given
    }
}

/// Parse `args` for the function described by `stub`, shown as `name` in
/// messages. None when an exception was thrown.
pub fn parse(vm: &mut Vm, name: &str, stub: &StubFunc, args: &[Value]) -> Option<Args> {
    let variadic = stub.args.last().is_some_and(|a| a.flags & A_VARIADIC != 0);
    let n_decl = stub.args.len() - variadic as usize;
    let min = stub.required_args();
    // trailing Undef (omitted named arguments) do not count
    let mut given = args.len();
    while given > 0 && matches!(args[given - 1], Value::Undef) {
        given -= 1;
    }
    if given < min || (!variadic && given > n_decl) {
        count_error(
            vm,
            name,
            min,
            if variadic { None } else { Some(n_decl) },
            given,
        );
        return None;
    }
    let strict = vm.caller_strict_types;
    let line = vm.current_line;
    let mut out = Vec::with_capacity(n_decl);
    for (i, p) in stub.args.iter().enumerate().take(n_decl) {
        let raw = args.get(i).cloned().unwrap_or(Value::Undef);
        if matches!(raw, Value::Undef) {
            if i < min {
                let msg = format!("{}(): Argument #{} (${}) not passed", name, i + 1, p.name);
                throw(vm, b"ArgumentCountError", &msg);
                return None;
            }
            let d = p.default.and_then(|d| vm.arginfo_default_value(d));
            let d = d.unwrap_or(Value::Undef);
            out.push(if p.flags & A_BY_REF != 0 {
                Value::Reference(Rc::new(RefCell::new(d)))
            } else {
                d
            });
            continue;
        }
        if p.flags & A_BY_REF != 0 {
            out.push(raw);
            continue;
        }
        out.push(check_arg(
            vm,
            name,
            i,
            p.name,
            override_type(name, p.name).unwrap_or(p.ty),
            &raw.deref(),
            strict,
            line,
        )?);
    }
    let rest = if variadic && args.len() > n_decl {
        args[n_decl..].to_vec()
    } else {
        Vec::new()
    };
    Some(Args {
        v: out,
        rest,
        given,
    })
}

fn type_error(vm: &mut Vm, name: &str, i: usize, pname: &str, ty: &str, v: &Value) {
    let msg = format!(
        "{}(): Argument #{} (${}) must be of type {}, {} given",
        name,
        i + 1,
        pname,
        type_display(ty),
        Vm::value_type_name(v)
    );
    throw(vm, b"TypeError", &msg);
}

/// Check / coerce one argument against its declared type.
#[allow(clippy::too_many_arguments)]
pub fn check_arg(
    vm: &mut Vm,
    name: &str,
    i: usize,
    pname: &str,
    ty: &str,
    v: &Value,
    strict: bool,
    line: u32,
) -> Option<Value> {
    if ty.is_empty() {
        return Some(v.clone());
    }
    let mask = type_mask(ty);
    if mask & T_MIXED != 0 {
        return Some(v.clone());
    }
    // Z_PARAM_FUNC / Z_PARAM_FUNC_OR_NULL
    if mask & T_CALLABLE != 0 && mask & !(T_CALLABLE | T_NULL) == 0 {
        if mask & T_NULL != 0 && matches!(v, Value::Null | Value::Undef) {
            return Some(Value::Null);
        }
        let r = vm.zend_is_callable_ex(v, 0);
        if vm.current_exception.is_some() {
            return None;
        }
        if !r.ok {
            let msg = format!(
                "{}(): Argument #{} (${}) must be a valid callback{}, {}",
                name,
                i + 1,
                pname,
                if mask & T_NULL != 0 { " or null" } else { "" },
                r.error.unwrap_or_default()
            );
            throw(vm, b"TypeError", &msg);
            return None;
        }
        return Some(v.clone());
    }
    let classes = type_classes(ty);
    let scalar = mask & (T_BOOL | T_INT | T_FLOAT | T_STRING);
    match v {
        Value::Null | Value::Undef => {
            if mask & T_NULL != 0 {
                return Some(Value::Null);
            }
            if scalar == 0 || strict {
                type_error(vm, name, i, pname, ty, v);
                return None;
            }
            let msg = format!(
                "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
                name,
                i + 1,
                pname,
                type_display(ty)
            );
            vm.emit_deprecated_at(&msg, line);
            if vm.current_exception.is_some() {
                return None;
            }
            Some(if mask & T_STRING != 0 {
                Value::String(goro_core::string::PhpString::empty())
            } else if mask & T_INT != 0 {
                Value::Long(0)
            } else if mask & T_FLOAT != 0 {
                Value::Double(0.0)
            } else {
                Value::False
            })
        }
        Value::Object(o) => {
            if mask & T_OBJECT != 0 {
                return Some(v.clone());
            }
            if classes.iter().any(|c| obj_instanceof(vm, o, c)) {
                return Some(v.clone());
            }
            if mask & T_CALLABLE != 0 {
                return Some(v.clone());
            }
            if mask & T_STRING != 0 && !strict {
                if let Ok(Some(s)) = vm.zpp_coerce_scalar(T_STRING, v, strict, line) {
                    return Some(s);
                }
                if vm.current_exception.is_some() {
                    return None;
                }
            }
            type_error(vm, name, i, pname, ty, v);
            None
        }
        Value::Array(_) => {
            if mask & (T_ARRAY | T_CALLABLE) != 0 {
                return Some(v.clone());
            }
            type_error(vm, name, i, pname, ty, v);
            None
        }
        _ => {
            if mask & T_CALLABLE != 0 && matches!(v, Value::String(_)) {
                return Some(v.clone());
            }
            if scalar == 0 {
                type_error(vm, name, i, pname, ty, v);
                return None;
            }
            // false/true pseudo types count as bool here
            let m = scalar | (mask & (T_FALSE | T_TRUE));
            match vm.zpp_coerce_scalar(m, v, strict, line) {
                Ok(Some(c)) => Some(c),
                Ok(None) => {
                    type_error(vm, name, i, pname, ty, v);
                    None
                }
                Err(()) => None,
            }
        }
    }
}

/// zend_argument_value_error(): "name(): Argument #n ($p) msg"
pub fn value_error(vm: &mut Vm, name: &str, n: usize, pname: &str, msg: &str) {
    let m = format!("{}(): Argument #{} (${}) {}", name, n, pname, msg);
    throw(vm, b"ValueError", &m);
}

/// zend_argument_type_error()
pub fn arg_type_error(vm: &mut Vm, name: &str, n: usize, pname: &str, msg: &str) {
    let m = format!("{}(): Argument #{} (${}) {}", name, n, pname, msg);
    throw(vm, b"TypeError", &m);
}

/// Parameters whose C parsing differs from their stub type (the stub
/// documents what the function accepts, ZPP takes more and rejects later):
/// (function, parameter, type ZPP checks).
const TYPE_OVERRIDES: &[(&str, &str, &str)] = &[
    // Z_PARAM_OBJ_OR_STR_OR_NULL
    ("datefmt_create", "timezone", "object|string|null"),
    (
        "IntlDateFormatter::create",
        "timezone",
        "object|string|null",
    ),
    (
        "IntlDateFormatter::__construct",
        "timezone",
        "object|string|null",
    ),
    ("datefmt_set_timezone", "timezone", "object|string|null"),
    (
        "IntlDateFormatter::setTimeZone",
        "timezone",
        "object|string|null",
    ),
    // Z_PARAM_OBJ_OF_CLASS_OR_LONG_OR_NULL (untyped in the stub)
    (
        "IntlDateFormatter::__construct",
        "calendar",
        "IntlCalendar|int|null",
    ),
];

fn override_type(name: &str, pname: &str) -> Option<&'static str> {
    TYPE_OVERRIDES
        .iter()
        .find(|(f, p, _)| *f == name && *p == pname)
        .map(|(_, _, t)| *t)
}
