//! zend_parse_parameters for the SOAP methods (adapted from goro-ext-mysqli).

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::arginfo::{T_BOOL, T_FLOAT, T_INT, T_STRING};
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::types::{Bail, Obj, SR};

/// A parsed argument
#[derive(Clone, Debug)]
pub enum Arg {
    Absent,
    Null,
    Obj(Obj),
    Str(Vec<u8>),
    Long(i64),
    Double(f64),
    Bool(bool),
    Arr(Rc<RefCell<PhpArray>>),
    Any(Value),
    /// the variadic rest (as passed)
    Rest(Vec<Value>),
    /// a class name ('C'): the declared name
    Class(Vec<u8>),
}

impl Arg {
    pub fn str(&self) -> Option<&[u8]> {
        match self {
            Arg::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn string(&self) -> Option<String> {
        self.str().map(|s| String::from_utf8_lossy(s).into_owned())
    }
    pub fn long(&self) -> Option<i64> {
        match self {
            Arg::Long(n) => Some(*n),
            _ => None,
        }
    }
    pub fn bool(&self) -> bool {
        matches!(self, Arg::Bool(true))
    }
    pub fn obj(&self) -> Option<Obj> {
        match self {
            Arg::Obj(o) => Some(o.clone()),
            _ => None,
        }
    }
    pub fn arr(&self) -> Option<Rc<RefCell<PhpArray>>> {
        match self {
            Arg::Arr(a) => Some(a.clone()),
            _ => None,
        }
    }
    pub fn any(&self) -> Option<Value> {
        match self {
            Arg::Any(v) => Some(v.clone()),
            _ => None,
        }
    }
    pub fn is_given(&self) -> bool {
        !matches!(self, Arg::Absent | Arg::Null)
    }
}

pub fn throw(vm: &mut Vm, class: &[u8], msg: &str) -> Bail {
    let e = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(e);
    Bail
}

/// The method being run, for messages: "SoapClient::__soapCall", with its
/// parameter names (from the stub).
#[derive(Clone)]
pub struct Func {
    pub name: String,
    pub params: Vec<&'static str>,
}

impl Func {
    pub fn new(full: &str) -> Func {
        let (class, method) = full.split_once("::").unwrap_or(("", full));
        let params =
            goro_core::stubs::find_method(class.to_ascii_lowercase().as_bytes(), method.as_bytes())
                .map(|(_, m)| m.args.iter().map(|a| a.name).collect())
                .or_else(|| {
                    goro_core::stubs::find_function(full.as_bytes())
                        .map(|f| f.args.iter().map(|a| a.name).collect())
                })
                .unwrap_or_default();
        Func {
            name: full.to_string(),
            params,
        }
    }

    fn pname(&self, i: usize) -> Option<&'static str> {
        self.params.get(i).copied()
    }

    /// "func(): Argument #n ($name)"
    pub fn arg_prefix(&self, n: usize) -> String {
        match self.pname(n - 1) {
            Some(p) => format!("{}(): Argument #{} (${})", self.name, n, p),
            None => format!("{}(): Argument #{}", self.name, n),
        }
    }

    /// zend_argument_value_error
    pub fn value_error(&self, vm: &mut Vm, n: usize, msg: &str) -> Bail {
        let m = format!("{} {}", self.arg_prefix(n), msg);
        throw(vm, b"ValueError", &m)
    }

    /// zend_argument_type_error
    pub fn arg_type_error(&self, vm: &mut Vm, n: usize, msg: &str) -> Bail {
        let m = format!("{} {}", self.arg_prefix(n), msg);
        throw(vm, b"TypeError", &m)
    }

    /// zend_argument_must_not_be_empty_error
    pub fn must_not_be_empty(&self, vm: &mut Vm, n: usize) -> Bail {
        self.value_error(vm, n, "must not be empty")
    }

    pub fn type_error(&self, vm: &mut Vm, n: usize, ty: &str, v: &Value) -> Bail {
        let m = format!(
            "{} must be of type {}, {} given",
            self.arg_prefix(n),
            ty,
            Vm::value_type_name(v)
        );
        throw(vm, b"TypeError", &m)
    }

    /// Map the arguments of a variadic forwarding method (named arguments
    /// have string keys) to positions.
    pub fn positional(&self, vm: &mut Vm, args: &Value) -> SR<Vec<Value>> {
        let a = match args.deref() {
            Value::Array(a) => a,
            _ => return Ok(Vec::new()),
        };
        let a = a.borrow();
        let mut out: Vec<Value> = Vec::new();
        for (k, v) in a.iter() {
            match k {
                ArrayKey::Int(_) => out.push(v.clone()),
                ArrayKey::String(s) => {
                    let name = s.to_string_lossy();
                    let Some(pos) = self.params.iter().position(|p| *p == name) else {
                        let m = format!("Unknown named parameter ${}", name);
                        return Err(throw(vm, b"Error", &m));
                    };
                    while out.len() <= pos {
                        out.push(Value::Undef);
                    }
                    if !matches!(out[pos], Value::Undef) {
                        let m = format!("Named parameter ${} overwrites previous argument", name);
                        return Err(throw(vm, b"Error", &m));
                    }
                    out[pos] = v.clone();
                }
            }
        }
        // Skipped positional parameters of internal functions get their
        // default (zpp sees them as not passed only at the end).
        while matches!(out.last(), Some(Value::Undef)) {
            out.pop();
        }
        Ok(out)
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// zend_parse_parameters with the C format characters: O (object of
/// `classes[i]`), o, s/S/p/P, l, b, d, a/h, z, C, *, with `!` and `|`.
pub fn parse(vm: &mut Vm, f: &Func, spec: &str, classes: &[&str], args: &[Value]) -> SR<Vec<Arg>> {
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
    let variadic = toks.iter().any(|t| t.0 == '*' || t.0 == '+');
    let n_fixed = toks.iter().filter(|t| t.0 != '*' && t.0 != '+').count();
    let min = match opt_from {
        Some(o) => o,
        None => {
            if toks.iter().any(|t| t.0 == '+') {
                n_fixed + 1
            } else {
                n_fixed
            }
        }
    };
    let max = if variadic { usize::MAX } else { n_fixed };
    let given = args.len();
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
        return Err(throw(vm, b"ArgumentCountError", &msg));
    }
    let strict = vm.caller_strict_types;
    let line = vm.current_line;
    let mut out: Vec<Arg> = Vec::new();
    let mut class_idx = 0;
    let mut ai = 0usize;
    for &(c, nullable) in toks.iter() {
        let argn = ai + 1;
        if c == '*' || c == '+' {
            out.push(Arg::Rest(args[ai.min(args.len())..].to_vec()));
            ai = args.len();
            continue;
        }
        let raw = match args.get(ai) {
            None | Some(Value::Undef) => {
                if c == 'O' {
                    class_idx += 1;
                }
                out.push(Arg::Absent);
                ai += 1;
                continue;
            }
            Some(v) => v.deref(),
        };
        ai += 1;
        if nullable && matches!(raw, Value::Null) {
            if c == 'O' {
                class_idx += 1;
            }
            out.push(Arg::Null);
            continue;
        }
        match c {
            'O' => {
                let cls = classes.get(class_idx).copied().unwrap_or("stdClass");
                class_idx += 1;
                match &raw {
                    Value::Object(o) if crate::php::instance_of(vm, o, cls) => {
                        out.push(Arg::Obj(o.clone()))
                    }
                    other => {
                        let ty = if nullable {
                            format!("?{}", cls)
                        } else {
                            cls.to_string()
                        };
                        return Err(f.type_error(vm, argn, &ty, other));
                    }
                }
            }
            'o' => match &raw {
                Value::Object(o) => out.push(Arg::Obj(o.clone())),
                other => {
                    let ty = if nullable { "?object" } else { "object" };
                    return Err(f.type_error(vm, argn, ty, other));
                }
            },
            'z' => out.push(Arg::Any(args[ai - 1].clone())),
            'a' | 'h' => match &raw {
                Value::Array(a) => out.push(Arg::Arr(a.clone())),
                other => {
                    let ty = if nullable { "?array" } else { "array" };
                    return Err(f.type_error(vm, argn, ty, other));
                }
            },
            'C' => {
                let name = match &raw {
                    Value::String(s) => s.as_bytes().to_vec(),
                    Value::Object(_) => match vm.value_to_string_checked(&raw) {
                        Ok(s) => s.as_bytes().to_vec(),
                        Err(_) => return Err(Bail),
                    },
                    other => match vm.zpp_coerce_scalar(T_STRING, other, strict, line) {
                        Ok(Some(Value::String(s))) => s.as_bytes().to_vec(),
                        Ok(_) => return Err(f.type_error(vm, argn, "string", other)),
                        Err(()) => return Err(Bail),
                    },
                };
                match crate::php::lookup_class(vm, &name) {
                    Some(n) => out.push(Arg::Class(n)),
                    None => {
                        if vm.current_exception.is_some() {
                            return Err(Bail);
                        }
                        let m = format!(
                            "must be a valid class name, {} given",
                            String::from_utf8_lossy(&name)
                        );
                        return Err(f.arg_type_error(vm, argn, &m));
                    }
                }
            }
            's' | 'S' | 'p' | 'P' | 'l' | 'b' | 'd' => {
                let (mask, tyname) = match c {
                    's' | 'S' | 'p' | 'P' => (T_STRING, "string"),
                    'l' => (T_INT, "int"),
                    'b' => (T_BOOL, "bool"),
                    _ => (T_FLOAT, "float"),
                };
                if matches!(raw, Value::Null) {
                    if strict {
                        return Err(f.type_error(vm, argn, tyname, &raw));
                    }
                    let msg = match f.pname(argn - 1) {
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
                        return Err(Bail);
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
                        return Err(f.type_error(vm, argn, &ty, &raw));
                    }
                    Err(()) => return Err(Bail),
                };
                let a = match (c, v) {
                    ('l', Value::Long(n)) => Arg::Long(n),
                    ('b', Value::True) => Arg::Bool(true),
                    ('b', Value::False) => Arg::Bool(false),
                    ('d', Value::Double(d)) => Arg::Double(d),
                    ('d', Value::Long(n)) => Arg::Double(n as f64),
                    (_, Value::String(s)) => {
                        if matches!(c, 'p' | 'P') && s.as_bytes().contains(&0) {
                            return Err(f.value_error(vm, argn, "must not contain any null bytes"));
                        }
                        Arg::Str(s.as_bytes().to_vec())
                    }
                    (_, v) => Arg::Any(v),
                };
                out.push(a);
            }
            _ => out.push(Arg::Any(raw)),
        }
    }
    Ok(out)
}
