//! zend_parse_parameters for the fileinfo functions and methods.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::arginfo::{T_BOOL, T_FLOAT, T_INT, T_STRING};
use goro_core::array::PhpArray;
use goro_core::object::PhpObject;
use goro_core::value::Value;
use goro_core::vm::Vm;

pub type Obj = Rc<RefCell<PhpObject>>;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum Arg {
    Absent,
    Null,
    Str(Vec<u8>),
    Long(i64),
    Double(f64),
    Bool(bool),
    Arr(Rc<RefCell<PhpArray>>),
    /// 'z' (as passed: references kept)
    Any(Value),
}

#[allow(dead_code)]
impl Arg {
    pub fn str(&self) -> Option<&[u8]> {
        match self {
            Arg::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn str_or<'a>(&'a self, d: &'a [u8]) -> &'a [u8] {
        self.str().unwrap_or(d)
    }
    pub fn long_or(&self, d: i64) -> i64 {
        match self {
            Arg::Long(n) => *n,
            _ => d,
        }
    }
    pub fn double_or(&self, d: f64) -> f64 {
        match self {
            Arg::Double(n) => *n,
            _ => d,
        }
    }
    pub fn bool_or(&self, d: bool) -> bool {
        match self {
            Arg::Bool(b) => *b,
            _ => d,
        }
    }
}

pub fn throw(vm: &mut Vm, class: &[u8], msg: &str) {
    let e = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(e);
}

/// The function being run, for messages.
#[derive(Clone)]
pub struct Func {
    pub name: String,
    pub params: Vec<&'static str>,
}

#[allow(dead_code)]
impl Func {
    pub fn function(name: &str) -> Func {
        let params = goro_core::stubs::find_function(name.as_bytes())
            .map(|f| f.args.iter().map(|a| a.name).collect())
            .unwrap_or_default();
        Func {
            name: name.to_string(),
            params,
        }
    }

    pub fn method(class: &str, method: &str) -> Func {
        let (name, params) =
            match goro_core::stubs::find_method(class.as_bytes(), method.as_bytes()) {
                Some((c, m)) => (
                    format!("{}::{}", c.name, m.name),
                    m.args.iter().map(|a| a.name).collect(),
                ),
                None => (format!("{}::{}", class, method), Vec::new()),
            };
        Func { name, params }
    }

    /// "func(): Argument #n ($name)"
    pub fn arg_prefix(&self, n: usize) -> String {
        match self.params.get(n - 1) {
            Some(p) => format!("{}(): Argument #{} (${})", self.name, n, p),
            None => format!("{}(): Argument #{}", self.name, n),
        }
    }

    pub fn value_error(&self, vm: &mut Vm, n: usize, msg: &str) {
        let m = format!("{} {}", self.arg_prefix(n), msg);
        throw(vm, b"ValueError", &m);
    }

    /// zend_argument_must_not_be_empty_error()
    pub fn must_not_be_empty(&self, vm: &mut Vm, n: usize) {
        self.value_error(vm, n, "must not be empty");
    }

    pub fn type_error(&self, vm: &mut Vm, n: usize, ty: &str, v: &Value) {
        let m = format!(
            "{} must be of type {}, {} given",
            self.arg_prefix(n),
            ty,
            Vm::value_type_name(v)
        );
        throw(vm, b"TypeError", &m);
    }

    /// php_error_docref(NULL, E_WARNING, ...)
    pub fn warn(&self, vm: &mut Vm, msg: &str) {
        let line = vm.current_line;
        vm.emit_warning_at(&format!("{}(): {}", self.name, msg), line);
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn tokens(spec: &str) -> (Vec<(char, bool)>, usize) {
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
    let n = toks.len();
    (toks, opt_from.unwrap_or(n))
}

/// The argument count check of zend_parse_parameters()
pub fn arity_check(vm: &mut Vm, f: &Func, spec: &str, args: &[Value]) -> Result<(), ()> {
    let (toks, min) = tokens(spec);
    let max = toks.len();
    // trailing undefined (skipped named) arguments do not count
    let mut given = args.len();
    while given > 0 && matches!(args[given - 1], Value::Undef) {
        given -= 1;
    }
    if given < min || given > max {
        let msg = if min == max {
            format!(
                "{}() expects exactly {} argument{}, {} given",
                f.name,
                min,
                plural(min),
                given
            )
        } else if given < min {
            format!(
                "{}() expects at least {} argument{}, {} given",
                f.name,
                min,
                plural(min),
                given
            )
        } else {
            format!(
                "{}() expects at most {} argument{}, {} given",
                f.name,
                max,
                plural(max),
                given
            )
        };
        throw(vm, b"ArgumentCountError", &msg);
        return Err(());
    }
    Ok(())
}

/// zend_parse_parameters with the format characters used here: s, p, P,
/// S, l, b, d, a, h, z, f, with `!` (nullable) and `|`. Err: an exception
/// was thrown.
pub fn parse(vm: &mut Vm, f: &Func, spec: &str, args: &[Value]) -> Result<Vec<Arg>, ()> {
    arity_check(vm, f, spec, args)?;
    parse_offset(vm, f, spec, args, 0)
}

/// `parse()` (no arity check) of the arguments after the first `off`
pub fn parse_offset(
    vm: &mut Vm,
    f: &Func,
    spec: &str,
    args: &[Value],
    off: usize,
) -> Result<Vec<Arg>, ()> {
    let (toks, _) = tokens(spec);
    let n = toks.len();
    let strict = vm.caller_strict_types;
    let line = vm.current_line;
    let mut out = Vec::with_capacity(n);
    for (ai, &(c, nullable)) in toks.iter().enumerate() {
        let argn = ai + 1 + off;
        let raw = match args.get(ai) {
            None | Some(Value::Undef) => {
                out.push(Arg::Absent);
                continue;
            }
            Some(v) => v.deref(),
        };
        if c == 'z' {
            out.push(Arg::Any(args[ai].clone()));
            continue;
        }
        if nullable && matches!(raw, Value::Null) {
            out.push(Arg::Null);
            continue;
        }
        match c {
            'a' | 'h' => match &raw {
                Value::Array(a) => out.push(Arg::Arr(a.clone())),
                other => {
                    f.type_error(vm, argn, if nullable { "?array" } else { "array" }, other);
                    return Err(());
                }
            },
            'r' => match &raw {
                Value::Resource(_) => out.push(Arg::Any(raw)),
                other => {
                    let ty = if nullable {
                        "resource or null"
                    } else {
                        "resource"
                    };
                    f.type_error(vm, argn, ty, other);
                    return Err(());
                }
            },
            'f' => {
                let r = vm.zend_is_callable_ex(&raw, 0);
                if vm.current_exception.is_some() {
                    return Err(());
                }
                if !r.ok {
                    let m = format!(
                        "{} must be a valid callback, {}",
                        f.arg_prefix(argn),
                        r.error.unwrap_or_default()
                    );
                    throw(vm, b"TypeError", &m);
                    return Err(());
                }
                out.push(Arg::Any(raw));
            }
            's' | 'p' | 'P' | 'S' | 'l' | 'b' | 'd' => {
                let (mask, tyname) = match c {
                    'l' => (T_INT, "int"),
                    'b' => (T_BOOL, "bool"),
                    'd' => (T_FLOAT, "float"),
                    _ => (T_STRING, "string"),
                };
                if matches!(raw, Value::Null) {
                    if strict {
                        f.type_error(vm, argn, tyname, &raw);
                        return Err(());
                    }
                    let msg = match f.params.get(argn - 1) {
                        Some(p) => format!(
                            "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
                            f.name, argn, p, tyname
                        ),
                        None => format!(
                            "{}(): Passing null to parameter #{} of type {} is deprecated",
                            f.name, argn, tyname
                        ),
                    };
                    vm.emit_deprecated(&msg);
                    if vm.current_exception.is_some() {
                        return Err(());
                    }
                    out.push(match c {
                        'l' => Arg::Long(0),
                        'b' => Arg::Bool(false),
                        'd' => Arg::Double(0.0),
                        _ => Arg::Str(Vec::new()),
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
                        f.type_error(vm, argn, &ty, &raw);
                        return Err(());
                    }
                    Err(()) => return Err(()),
                };
                if vm.current_exception.is_some() {
                    return Err(());
                }
                out.push(match (c, v) {
                    ('l', Value::Long(n)) => Arg::Long(n),
                    ('b', Value::True) => Arg::Bool(true),
                    ('b', Value::False) => Arg::Bool(false),
                    ('d', Value::Double(d)) => Arg::Double(d),
                    ('d', Value::Long(n)) => Arg::Double(n as f64),
                    (_, Value::String(s)) => {
                        if (c == 'p' || c == 'P') && s.as_bytes().contains(&0) {
                            f.value_error(vm, argn, "must not contain any null bytes");
                            return Err(());
                        }
                        Arg::Str(s.as_bytes().to_vec())
                    }
                    (_, v) => Arg::Any(v),
                });
            }
            _ => out.push(Arg::Any(raw)),
        }
    }
    Ok(out)
}
