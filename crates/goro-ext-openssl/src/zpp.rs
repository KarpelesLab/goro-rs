//! Minimal `zend_parse_parameters` emulation (coercive typing mode) for the
//! openssl builtins: scalar coercions, TypeErrors and null deprecations.

use goro_core::array::PhpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

pub fn throw(vm: &mut Vm, class: &[u8], msg: String) -> VmError {
    let exc = vm.create_exception(class, &msg, vm.current_line);
    vm.current_exception = Some(exc);
    let cls = String::from_utf8_lossy(class).to_string();
    VmError {
        message: format!("Uncaught {}: {}", cls, msg),
        line: vm.current_line,
    }
}

pub fn type_error(vm: &mut Vm, msg: String) -> VmError {
    throw(vm, b"TypeError", msg)
}

pub fn value_error(vm: &mut Vm, msg: String) -> VmError {
    throw(vm, b"ValueError", msg)
}

/// zend_argument_value_error()
pub fn arg_value_error(vm: &mut Vm, f: &str, n: usize, pname: &str, what: &str) -> VmError {
    value_error(
        vm,
        format!("{}(): Argument #{} (${}) {}", f, n, pname, what),
    )
}

/// zend_argument_type_error()-style message for "must be of type X, Y given".
pub fn arg_type_error(
    vm: &mut Vm,
    f: &str,
    n: usize,
    pname: &str,
    expected: &str,
    v: &Value,
) -> VmError {
    let given = given_name(v);
    type_error(
        vm,
        format!(
            "{}(): Argument #{} (${}) must be of type {}, {} given",
            f, n, pname, expected, given
        ),
    )
}

pub fn given_name(v: &Value) -> String {
    match v.deref() {
        Value::Object(o) => goro_core::value::display_class_name(&o.borrow().class_name),
        other => Vm::value_type_name(&other),
    }
}

fn null_deprecation(vm: &mut Vm, f: &str, n: usize, pname: &str, ty: &str) {
    vm.emit_deprecated(&format!(
        "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
        f, n, pname, ty
    ));
}

pub fn arg(args: &[Value], idx: usize) -> Option<Value> {
    if !is_passed(args, idx) {
        return None;
    }
    args.get(idx).map(|v| v.deref())
}

/// Object with __toString?
fn stringable(vm: &mut Vm, v: &Value) -> bool {
    if let Value::Object(o) = v {
        let lc = o.borrow().class_name.to_ascii_lowercase();
        return vm
            .get_class_def(&lc)
            .map(|c| c.get_method(b"__tostring").is_some())
            .unwrap_or(false);
    }
    false
}

/// Z_PARAM_STR / "s".
pub fn str(
    vm: &mut Vm,
    args: &[Value],
    idx: usize,
    f: &str,
    pname: &str,
) -> Result<PhpString, VmError> {
    let v = arg(args, idx).unwrap_or(Value::Null);
    match &v {
        Value::Null | Value::Undef => {
            if is_passed(args, idx) {
                null_deprecation(vm, f, idx + 1, pname, "string");
            }
            Ok(PhpString::empty())
        }
        Value::Array(_) | Value::Resource(_) | Value::Generator(_) => {
            Err(arg_type_error(vm, f, idx + 1, pname, "string", &v))
        }
        Value::Object(_) => {
            if stringable(vm, &v) {
                let s = vm.value_to_string(&v);
                if vm.current_exception.is_some() {
                    return Err(VmError {
                        message: "exception".into(),
                        line: vm.current_line,
                    });
                }
                Ok(s)
            } else {
                Err(arg_type_error(vm, f, idx + 1, pname, "string", &v))
            }
        }
        other => {
            vm.warn_nan_coercion(other, "string", vm.current_line);
            Ok(other.to_php_string())
        }
    }
}

/// "s!" / ?string
pub fn str_nullable(
    vm: &mut Vm,
    args: &[Value],
    idx: usize,
    f: &str,
    pname: &str,
) -> Result<Option<PhpString>, VmError> {
    match arg(args, idx) {
        None | Some(Value::Null) | Some(Value::Undef) => Ok(None),
        Some(_) => str(vm, args, idx, f, pname).map(Some),
    }
}

/// Z_PARAM_PATH: string without NUL bytes.
pub fn path(
    vm: &mut Vm,
    args: &[Value],
    idx: usize,
    f: &str,
    pname: &str,
) -> Result<PhpString, VmError> {
    let s = str(vm, args, idx, f, pname)?;
    if s.as_bytes().contains(&0) {
        return Err(arg_value_error(
            vm,
            f,
            idx + 1,
            pname,
            "must not contain any null bytes",
        ));
    }
    Ok(s)
}

/// "l"
pub fn long(vm: &mut Vm, args: &[Value], idx: usize, f: &str, pname: &str) -> Result<i64, VmError> {
    let v = arg(args, idx).unwrap_or(Value::Null);
    let bad = |vm: &mut Vm, v: &Value| arg_type_error(vm, f, idx + 1, pname, "int", v);
    let from_double = |vm: &mut Vm, d: f64, v: &Value| -> Result<i64, VmError> {
        if !d.is_finite() || !(-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&d) {
            return Err(bad(vm, v));
        }
        if d.fract() != 0.0 {
            vm.emit_deprecated(&format!(
                "Implicit conversion from float {} to int loses precision",
                goro_core::value::format_php_float_repr(d)
            ));
        }
        Ok(d as i64)
    };
    match &v {
        Value::Long(l) => Ok(*l),
        Value::Null | Value::Undef => {
            if is_passed(args, idx) {
                null_deprecation(vm, f, idx + 1, pname, "int");
            }
            Ok(0)
        }
        Value::True => Ok(1),
        Value::False => Ok(0),
        Value::Double(d) => from_double(vm, *d, &v),
        Value::String(s) => match goro_core::value::parse_numeric_str(s.as_bytes(), false) {
            Some((goro_core::value::NumStr::Long(l), _)) => Ok(l),
            Some((goro_core::value::NumStr::Double(d), _)) => from_double(vm, d, &v),
            None => Err(bad(vm, &v)),
        },
        _ => Err(bad(vm, &v)),
    }
}

/// "b"
pub fn bool(
    vm: &mut Vm,
    args: &[Value],
    idx: usize,
    f: &str,
    pname: &str,
) -> Result<bool, VmError> {
    let v = arg(args, idx).unwrap_or(Value::Null);
    match &v {
        Value::Null | Value::Undef => {
            if is_passed(args, idx) {
                null_deprecation(vm, f, idx + 1, pname, "bool");
            }
            Ok(false)
        }
        Value::Array(_) | Value::Object(_) | Value::Resource(_) | Value::Generator(_) => {
            Err(arg_type_error(vm, f, idx + 1, pname, "bool", &v))
        }
        other => {
            vm.warn_nan_coercion(other, "bool", vm.current_line);
            Ok(other.is_truthy())
        }
    }
}

/// "a"
pub fn array(
    vm: &mut Vm,
    args: &[Value],
    idx: usize,
    f: &str,
    pname: &str,
) -> Result<Rc<RefCell<PhpArray>>, VmError> {
    let v = arg(args, idx).unwrap_or(Value::Null);
    match &v {
        Value::Array(a) => Ok(a.clone()),
        _ => Err(arg_type_error(vm, f, idx + 1, pname, "array", &v)),
    }
}

/// "a!" / ?array
pub fn array_nullable(
    vm: &mut Vm,
    args: &[Value],
    idx: usize,
    f: &str,
    pname: &str,
) -> Result<Option<Rc<RefCell<PhpArray>>>, VmError> {
    match arg(args, idx) {
        None | Some(Value::Null) | Some(Value::Undef) => Ok(None),
        Some(_) => array(vm, args, idx, f, pname).map(Some),
    }
}

/// zend_argument_must_not_be_empty_error()
pub fn must_not_be_empty(vm: &mut Vm, f: &str, n: usize, pname: &str) -> VmError {
    arg_value_error(vm, f, n, pname, "must not be empty")
}

/// Assign to a by-reference output argument (ZEND_TRY_ASSIGN_REF_*).
pub fn set_ref(args: &[Value], idx: usize, v: Value) {
    if let Some(Value::Reference(r)) = args.get(idx) {
        *r.borrow_mut() = v;
    }
}

thread_local! {
    static CURRENT: std::cell::RefCell<(&'static str, Vec<usize>)> = const { std::cell::RefCell::new(("", Vec::new())) };
}

/// Record the builtin being called and its named-argument holes.
pub fn enter(fname: &'static str, skipped: &[usize]) {
    CURRENT.with(|c| *c.borrow_mut() = (fname, skipped.to_vec()));
}

/// Name of the openssl builtin currently executing.
pub fn current_fn() -> &'static str {
    CURRENT.with(|c| c.borrow().0)
}

/// php_error_docref(NULL, E_WARNING, ...) from code that doesn't know its caller.
pub fn warn(vm: &mut Vm, msg: &str) {
    let f = current_fn();
    if f.is_empty() {
        vm.emit_warning(msg);
    } else {
        vm.emit_warning(&format!("{}(): {}", f, msg));
    }
}

/// Was argument `idx` passed (and not skipped by named arguments)?
pub fn is_passed(args: &[Value], idx: usize) -> bool {
    idx < args.len() && !CURRENT.with(|c| c.borrow().1.contains(&idx))
}

pub fn bytes(v: Vec<u8>) -> Value {
    Value::String(PhpString::from_vec(v))
}

pub fn sval(s: &str) -> Value {
    Value::String(PhpString::from_bytes(s.as_bytes()))
}
