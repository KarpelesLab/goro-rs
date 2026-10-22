//! zend_parse_parameters for the sqlite3 methods.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::arginfo::{T_BOOL, T_INT, T_STRING};
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

pub type Obj = Rc<RefCell<PhpObject>>;

/// A parsed argument
#[derive(Clone, Debug)]
pub enum Arg {
    Absent,
    Null,
    Str(Vec<u8>),
    Long(i64),
    Bool(bool),
    Obj(Obj),
    /// 'z' (as passed: references kept) and callables ('f'/'F')
    Any(Value),
}

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
    pub fn bool_or(&self, d: bool) -> bool {
        match self {
            Arg::Bool(b) => *b,
            _ => d,
        }
    }
    pub fn value(&self) -> Option<Value> {
        match self {
            Arg::Any(v) => Some(v.clone()),
            _ => None,
        }
    }
    pub fn obj(&self) -> Option<Obj> {
        match self {
            Arg::Obj(o) => Some(o.clone()),
            _ => None,
        }
    }
}

/// zend_throw_exception(): an exception thrown while another one is
/// pending gets the pending one as its previous exception.
pub fn throw_with_code(vm: &mut Vm, class: &[u8], msg: &str, code: i64) {
    let e = vm.create_exception(class, msg, vm.current_line);
    if let Value::Object(o) = &e {
        if code != 0 {
            o.borrow_mut()
                .set_property(b"code".to_vec(), Value::Long(code));
        }
        if let Some(prev) = vm.current_exception.take() {
            o.borrow_mut().set_property(b"previous".to_vec(), prev);
        }
    }
    vm.current_exception = Some(e);
}

pub fn throw(vm: &mut Vm, class: &[u8], msg: &str) {
    throw_with_code(vm, class, msg, 0);
}

pub fn vstr(b: impl AsRef<[u8]>) -> Value {
    Value::String(PhpString::from_bytes(b.as_ref()))
}

pub fn vbool(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

/// Whether class `class` (any case) is `target` (lowercase) or extends it.
pub fn class_is(vm: &Vm, class: &[u8], target: &[u8]) -> bool {
    let mut cur = class.to_ascii_lowercase();
    for _ in 0..64 {
        if cur == target {
            return true;
        }
        match vm.classes.get(&cur).and_then(|c| c.parent.clone()) {
            Some(p) => cur = p.to_ascii_lowercase(),
            None => return false,
        }
    }
    false
}

/// The method being run, for messages.
#[derive(Clone)]
pub struct Func {
    /// "SQLite3::exec"
    pub name: String,
    pub params: Vec<&'static str>,
}

impl Func {
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

    /// zend_argument_value_error()
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
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// zend_parse_parameters with the format characters used by sqlite3.c:
/// s, p, S, l, b, z, f/F (callable), O (object of class, given as
/// `O:classname` is not supported: the class is passed in `class`), with
/// `!` (nullable) and `|`. Err: an exception was thrown.
pub fn parse(
    vm: &mut Vm,
    f: &Func,
    spec: &str,
    class: Option<(&[u8], &str)>,
    args: &[Value],
) -> Result<Vec<Arg>, ()> {
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
    let min = opt_from.unwrap_or(n);
    let max = n;
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
    let strict = vm.caller_strict_types;
    let line = vm.current_line;
    let mut out = Vec::with_capacity(n);
    for (ai, &(c, nullable)) in toks.iter().enumerate() {
        let argn = ai + 1;
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
            'f' | 'F' => {
                let r = vm.zend_is_callable_ex(&raw, 0);
                if vm.current_exception.is_some() {
                    return Err(());
                }
                if !r.ok {
                    let m = format!(
                        "{} must be a valid callback{}, {}",
                        f.arg_prefix(argn),
                        if nullable { " or null" } else { "" },
                        r.error.unwrap_or_default()
                    );
                    throw(vm, b"TypeError", &m);
                    return Err(());
                }
                out.push(Arg::Any(raw));
            }
            'O' => {
                let (cls_lower, cls_name) = class.unwrap_or((b"", ""));
                match &raw {
                    Value::Object(o) if class_is(vm, &o.borrow().class_name.clone(), cls_lower) => {
                        out.push(Arg::Obj(o.clone()));
                    }
                    other => {
                        let ty = if nullable {
                            format!("?{}", cls_name)
                        } else {
                            cls_name.to_string()
                        };
                        f.type_error(vm, argn, &ty, other);
                        return Err(());
                    }
                }
            }
            's' | 'p' | 'S' | 'l' | 'b' => {
                let (mask, tyname) = match c {
                    'l' => (T_INT, "int"),
                    'b' => (T_BOOL, "bool"),
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
                    (_, Value::String(s)) => {
                        if c == 'p' && s.as_bytes().contains(&0) {
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

/// Z_PARAM_STR_OR_LONG: Ok(Err(name)) / Ok(Ok(number)).
pub fn str_or_long(vm: &mut Vm, f: &Func, n: usize, v: &Value) -> Result<Result<i64, Vec<u8>>, ()> {
    let raw = v.deref();
    let strict = vm.caller_strict_types;
    let line = vm.current_line;
    match &raw {
        Value::Long(n) => return Ok(Ok(*n)),
        Value::String(s) => return Ok(Err(s.as_bytes().to_vec())),
        Value::Null if !strict => {
            let msg = match f.params.get(n - 1) {
                Some(p) => format!(
                    "{}(): Passing null to parameter #{} (${}) of type string|int is deprecated",
                    f.name, n, p
                ),
                None => format!(
                    "{}(): Passing null to parameter #{} of type string|int is deprecated",
                    f.name, n
                ),
            };
            vm.emit_deprecated(&msg);
            if vm.current_exception.is_some() {
                return Err(());
            }
            return Ok(Err(Vec::new()));
        }
        _ => {}
    }
    match vm.zpp_coerce_scalar(T_INT | T_STRING, &raw, strict, line) {
        Ok(Some(Value::Long(n))) => Ok(Ok(n)),
        Ok(Some(Value::String(s))) => Ok(Err(s.as_bytes().to_vec())),
        Ok(_) => {
            f.type_error(vm, n, "string|int", &raw);
            Err(())
        }
        Err(()) => Err(()),
    }
}

/// Z_PARAM_LONG for argument `n`
pub fn long_arg(vm: &mut Vm, f: &Func, n: usize, v: &Value) -> Result<i64, ()> {
    let raw = v.deref();
    let strict = vm.caller_strict_types;
    let line = vm.current_line;
    if matches!(raw, Value::Null) && !strict {
        let msg = match f.params.get(n - 1) {
            Some(p) => format!(
                "{}(): Passing null to parameter #{} (${}) of type int is deprecated",
                f.name, n, p
            ),
            None => format!(
                "{}(): Passing null to parameter #{} of type int is deprecated",
                f.name, n
            ),
        };
        vm.emit_deprecated(&msg);
        if vm.current_exception.is_some() {
            return Err(());
        }
        return Ok(0);
    }
    match vm.zpp_coerce_scalar(T_INT, &raw, strict, line) {
        Ok(Some(Value::Long(n))) => Ok(n),
        Ok(_) => {
            f.type_error(vm, n, "int", &raw);
            Err(())
        }
        Err(()) => Err(()),
    }
}

/// The bytes of a C string: up to the first NUL.
pub fn cstr(b: &[u8]) -> &[u8] {
    match b.iter().position(|&c| c == 0) {
        Some(p) => &b[..p],
        None => b,
    }
}
