//! zend_parse_parameters for the curl functions: argument count errors,
//! CurlHandle / CurlMultiHandle / CurlShareHandle object parameters and the
//! weak/strict scalar coercions, with php-src's messages.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::arginfo::{T_FLOAT, T_INT, T_STRING};
use goro_core::array::PhpArray;
use goro_core::object::PhpObject;
use goro_core::value::Value;
use goro_core::vm::Vm;

pub type Obj = Rc<RefCell<PhpObject>>;

/// A parsed argument
#[derive(Clone)]
pub enum Arg {
    Absent,
    Null,
    Obj(Obj),
    Str(Vec<u8>),
    Long(i64),
    Double(f64),
    Arr(Rc<RefCell<PhpArray>>),
    /// 'z': the argument as passed (references kept)
    Any(Value),
}

impl Arg {
    pub fn long(&self) -> i64 {
        match self {
            Arg::Long(n) => *n,
            _ => 0,
        }
    }
    pub fn str(&self) -> &[u8] {
        match self {
            Arg::Str(s) => s,
            _ => b"",
        }
    }
    pub fn obj(&self) -> Obj {
        match self {
            Arg::Obj(o) => o.clone(),
            _ => unreachable!("object argument expected"),
        }
    }
    pub fn any(&self) -> Value {
        match self {
            Arg::Any(v) => v.clone(),
            _ => Value::Null,
        }
    }
    pub fn is_given(&self) -> bool {
        !matches!(self, Arg::Absent | Arg::Null)
    }
}

pub fn throw(vm: &mut Vm, class: &[u8], msg: &str) {
    let e = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(e);
}

/// The parameter name of function `fname` (from its stub).
pub fn param_name(fname: &str, n: usize) -> Option<&'static str> {
    if let Some((cls, m)) = fname.split_once("::") {
        let (_, f) = goro_core::stubs::find_method(cls.as_bytes(), m.as_bytes())?;
        return f.args.get(n - 1).map(|a| a.name);
    }
    let f = goro_core::stubs::find_function(fname.as_bytes())?;
    f.args.get(n - 1).map(|a| a.name)
}

/// "func(): Argument #n ($name)"
pub fn arg_prefix(fname: &str, n: usize) -> String {
    match param_name(fname, n) {
        Some(p) => format!("{}(): Argument #{} (${})", fname, n, p),
        None => format!("{}(): Argument #{}", fname, n),
    }
}

/// zend_argument_value_error()
pub fn value_error(vm: &mut Vm, fname: &str, n: usize, msg: &str) {
    let m = format!("{} {}", arg_prefix(fname, n), msg);
    throw(vm, b"ValueError", &m);
}

/// zend_argument_type_error()
pub fn arg_type_error(vm: &mut Vm, fname: &str, n: usize, msg: &str) {
    let m = format!("{} {}", arg_prefix(fname, n), msg);
    throw(vm, b"TypeError", &m);
}

/// zend_value_error()
pub fn plain_value_error(vm: &mut Vm, msg: &str) {
    throw(vm, b"ValueError", msg);
}

fn type_error(vm: &mut Vm, fname: &str, n: usize, ty: &str, v: &Value) {
    let m = format!(
        "{} must be of type {}, {} given",
        arg_prefix(fname, n),
        ty,
        Vm::value_type_name(v)
    );
    throw(vm, b"TypeError", &m);
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// zend_wrong_parameters_count_error()
pub fn count_error(vm: &mut Vm, fname: &str, min: usize, max: usize, given: usize) {
    let msg = if min == max {
        format!(
            "{}() expects exactly {} argument{}, {} given",
            fname,
            min,
            plural(min),
            given
        )
    } else if given < min {
        format!(
            "{}() expects at least {} argument{}, {} given",
            fname,
            min,
            plural(min),
            given
        )
    } else {
        format!(
            "{}() expects at most {} argument{}, {} given",
            fname,
            max,
            plural(max),
            given
        )
    };
    throw(vm, b"ArgumentCountError", &msg);
}

/// The class a format character stands for.
fn class_of(c: char) -> Option<(&'static [u8], &'static str)> {
    match c {
        'C' => Some((b"CurlHandle", "CurlHandle")),
        'M' => Some((b"CurlMultiHandle", "CurlMultiHandle")),
        'H' => Some((b"CurlShareHandle", "CurlShareHandle")),
        _ => None,
    }
}

/// zend_parse_parameters with the format characters used here:
/// C (CurlHandle), M (CurlMultiHandle), H (CurlShareHandle), l, d, s,
/// p (path: no NUL bytes), z, a (array), with `!` (nullable) and `|`
/// (optional from here).
pub fn parse(vm: &mut Vm, fname: &str, spec: &str, args: &[Value]) -> Result<Vec<Arg>, ()> {
    let mut toks: Vec<(char, bool)> = Vec::new();
    let mut opt_from: Option<usize> = None;
    let chars: Vec<char> = spec.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '|' {
            opt_from = Some(toks.len());
            i += 1;
            continue;
        }
        let nullable = chars.get(i + 1) == Some(&'!');
        toks.push((c, nullable));
        i += if nullable { 2 } else { 1 };
    }
    let max = toks.len();
    let min = opt_from.unwrap_or(max);
    // trailing skipped (named) arguments do not count
    let mut given = args.len();
    while given > 0 && matches!(args[given - 1], Value::Undef) {
        given -= 1;
    }
    if given < min || given > max {
        count_error(vm, fname, min, max, given);
        return Err(());
    }
    let strict = vm.caller_strict_types;
    let line = vm.current_line;
    let mut out: Vec<Arg> = Vec::with_capacity(max);
    for (ai, &(c, nullable)) in toks.iter().enumerate() {
        let argn = ai + 1;
        let raw = match args.get(ai) {
            None | Some(Value::Undef) => {
                out.push(Arg::Absent);
                continue;
            }
            Some(v) => v.deref(),
        };
        if nullable && matches!(raw, Value::Null) {
            out.push(Arg::Null);
            continue;
        }
        if let Some((cls, disp)) = class_of(c) {
            match &raw {
                Value::Object(o) if o.borrow().class_name.eq_ignore_ascii_case(cls) => {
                    out.push(Arg::Obj(o.clone()))
                }
                other => {
                    let ty = if nullable {
                        format!("?{}", disp)
                    } else {
                        disp.to_string()
                    };
                    type_error(vm, fname, argn, &ty, other);
                    return Err(());
                }
            }
            continue;
        }
        match c {
            'z' => out.push(Arg::Any(args[ai].clone())),
            'a' => match &raw {
                Value::Array(a) => out.push(Arg::Arr(a.clone())),
                other => {
                    let ty = if nullable { "?array" } else { "array" };
                    type_error(vm, fname, argn, ty, other);
                    return Err(());
                }
            },
            's' | 'p' | 'l' | 'd' => {
                let (mask, tyname) = match c {
                    's' | 'p' => (T_STRING, "string"),
                    'd' => (T_FLOAT, "float"),
                    _ => (T_INT, "int"),
                };
                if matches!(raw, Value::Null) {
                    if strict {
                        type_error(vm, fname, argn, tyname, &raw);
                        return Err(());
                    }
                    let msg = match param_name(fname, argn) {
                        Some(p) => format!(
                            "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
                            fname, argn, p, tyname
                        ),
                        None => format!(
                            "{}(): Passing null to parameter #{} of type {} is deprecated",
                            fname, argn, tyname
                        ),
                    };
                    vm.emit_deprecated(&msg);
                    if vm.current_exception.is_some() {
                        return Err(());
                    }
                    out.push(match c {
                        's' | 'p' => Arg::Str(Vec::new()),
                        'd' => Arg::Double(0.0),
                        _ => Arg::Long(0),
                    });
                    continue;
                }
                let v = match vm.zpp_coerce_scalar(mask, &raw, strict, line) {
                    Ok(Some(v)) => v,
                    Ok(None) => {
                        let ty = if nullable {
                            format!("?{}", tyname)
                        } else {
                            tyname.to_string()
                        };
                        type_error(vm, fname, argn, &ty, &raw);
                        return Err(());
                    }
                    Err(()) => return Err(()),
                };
                if vm.current_exception.is_some() {
                    return Err(());
                }
                match v {
                    Value::String(s) => {
                        if c == 'p' && s.as_bytes().contains(&0) {
                            value_error(vm, fname, argn, "must not contain any null bytes");
                            return Err(());
                        }
                        out.push(Arg::Str(s.as_bytes().to_vec()))
                    }
                    Value::Long(n) => {
                        if c == 'd' {
                            out.push(Arg::Double(n as f64))
                        } else {
                            out.push(Arg::Long(n))
                        }
                    }
                    Value::Double(d) => out.push(Arg::Double(d)),
                    other => out.push(Arg::Any(other)),
                }
            }
            _ => out.push(Arg::Any(raw)),
        }
    }
    Ok(out)
}

/// zval_try_get_long(): None when the value cannot be converted.
pub fn try_get_long(v: &Value) -> Option<i64> {
    match v.deref() {
        Value::Long(n) => Some(n),
        Value::Null | Value::Undef | Value::False => Some(0),
        Value::True => Some(1),
        Value::Double(d) => {
            if (-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&d) {
                Some(d as i64)
            } else {
                None
            }
        }
        Value::String(s) => match goro_core::value::parse_numeric_str(s.as_bytes(), true) {
            Some((goro_core::value::NumStr::Long(n), false)) => Some(n),
            Some((goro_core::value::NumStr::Double(d), false)) => {
                if d.is_finite() {
                    Some(d as i64)
                } else {
                    None
                }
            }
            _ => None,
        },
        _ => None,
    }
}

/// zval_get_long(): objects warn and convert to 1.
pub fn zval_get_long(vm: &mut Vm, v: &Value) -> i64 {
    match v.deref() {
        Value::Object(o) => {
            let cn = goro_core::value::display_class_name(&o.borrow().class_name);
            vm.emit_warning(&format!(
                "Object of class {} could not be converted to int",
                cn
            ));
            1
        }
        other => other.to_long(),
    }
}

/// zval_get_string() (objects with __toString, arrays "Array" with a warning).
pub fn zval_get_string(vm: &mut Vm, v: &Value) -> Vec<u8> {
    match v.deref() {
        Value::Array(_) => {
            vm.emit_warning("Array to string conversion");
            b"Array".to_vec()
        }
        other @ Value::Object(_) => match vm.value_to_string_checked(&other) {
            Ok(s) => s.as_bytes().to_vec(),
            Err(_) => Vec::new(),
        },
        other => other.to_php_string().as_bytes().to_vec(),
    }
}
