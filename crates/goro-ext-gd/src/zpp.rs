//! zend_parse_parameters for the gd functions.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::arginfo::{T_BOOL, T_FLOAT, T_INT, T_STRING};
use goro_core::array::PhpArray;
use goro_core::object::PhpObject;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use goro_gd::{Font, Image};

pub type Obj = Rc<RefCell<PhpObject>>;
pub type Img = Rc<RefCell<Image>>;

#[derive(Clone)]
pub enum Arg {
    Absent,
    Null,
    Str(Vec<u8>),
    Long(i64),
    Double(f64),
    Bool(bool),
    Arr(Rc<RefCell<PhpArray>>),
    /// 'O': a GdImage
    Img(Img),
    /// 'F': GdFont|int
    Font(Rc<Font>),
    /// 'z'
    Any(Value),
}

impl Arg {
    pub fn str_or<'a>(&'a self, d: &'a [u8]) -> &'a [u8] {
        match self {
            Arg::Str(s) => s,
            _ => d,
        }
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
    pub fn is_null(&self) -> bool {
        matches!(self, Arg::Null | Arg::Absent)
    }
    pub fn img(&self) -> Img {
        match self {
            Arg::Img(i) => i.clone(),
            _ => unreachable!("not an image argument"),
        }
    }
    pub fn arr(&self) -> Rc<RefCell<PhpArray>> {
        match self {
            Arg::Arr(a) => a.clone(),
            _ => Rc::new(RefCell::new(PhpArray::new())),
        }
    }
}

pub fn throw(vm: &mut Vm, class: &[u8], msg: &str) {
    let e = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(e);
}

/// The VmError returned once an exception is pending.
pub fn err(vm: &Vm) -> VmError {
    VmError {
        message: String::new(),
        line: vm.current_line,
    }
}

/// The function being run, for messages.
#[derive(Clone)]
pub struct Func {
    pub name: &'static str,
    pub params: Vec<&'static str>,
}

impl Func {
    pub fn new(name: &'static str) -> Func {
        let params = goro_core::stubs::find_function(name.as_bytes())
            .map(|f| {
                f.args
                    .iter()
                    .take_while(|a| a.flags & goro_core::stubs::A_VARIADIC == 0)
                    .map(|a| a.name)
                    .collect()
            })
            .unwrap_or_default();
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
    pub fn value_error(&self, vm: &mut Vm, n: usize, msg: &str) -> VmError {
        let m = format!("{} {}", self.arg_prefix(n), msg);
        throw(vm, b"ValueError", &m);
        err(vm)
    }

    /// zend_argument_type_error()
    pub fn arg_type_error(&self, vm: &mut Vm, n: usize, msg: &str) -> VmError {
        let m = format!("{} {}", self.arg_prefix(n), msg);
        throw(vm, b"TypeError", &m);
        err(vm)
    }

    /// zend_value_error()
    pub fn plain_value_error(&self, vm: &mut Vm, msg: &str) -> VmError {
        throw(vm, b"ValueError", msg);
        err(vm)
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

    /// php_error_docref(NULL, E_NOTICE, ...)
    pub fn notice(&self, vm: &mut Vm, msg: &str) {
        let line = vm.current_line;
        vm.emit_notice_at(&format!("{}(): {}", self.name, msg), line);
    }

    /// php_error_docref(NULL, E_DEPRECATED, ...)
    pub fn deprecated(&self, vm: &mut Vm, msg: &str) -> Result<(), VmError> {
        vm.emit_deprecated(&format!("{}(): {}", self.name, msg));
        if vm.current_exception.is_some() {
            return Err(err(vm));
        }
        Ok(())
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// zend_wrong_parameters_count_error()
pub fn arity_error(vm: &mut Vm, f: &Func, given: usize, min: usize, max: usize) -> VmError {
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
    err(vm)
}

/// zend_parse_parameters with the format characters used by gd: O (a
/// GdImage), F (GdFont|int), s, p (path), l, d, b, a, z, with `!`
/// (nullable) and `|`.
pub fn parse(vm: &mut Vm, f: &Func, spec: &str, args: &[Value]) -> Result<Vec<Arg>, VmError> {
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
    let mut given = args.len();
    while given > 0 && matches!(args[given - 1], Value::Undef) {
        given -= 1;
    }
    if given < min || given > max {
        return Err(arity_error(vm, f, given, min, max));
    }
    let mut out = Vec::with_capacity(n);
    for (ai, &(c, nullable)) in toks.iter().enumerate() {
        let a = match args.get(ai) {
            None | Some(Value::Undef) => Arg::Absent,
            Some(v) => parse_one(vm, f, ai + 1, c, nullable, v)?,
        };
        out.push(a);
    }
    Ok(out)
}

pub fn parse_one(
    vm: &mut Vm,
    f: &Func,
    argn: usize,
    c: char,
    nullable: bool,
    v: &Value,
) -> Result<Arg, VmError> {
    let strict = vm.caller_strict_types;
    let line = vm.current_line;
    let raw = v.deref();
    if c == 'z' {
        if nullable && matches!(raw, Value::Null) {
            return Ok(Arg::Null);
        }
        return Ok(Arg::Any(raw));
    }
    if nullable && matches!(raw, Value::Null) {
        return Ok(Arg::Null);
    }
    match c {
        'O' => {
            if let Value::Object(o) = &raw {
                if let Some(img) = crate::image_of(o) {
                    return Ok(Arg::Img(img));
                }
            }
            f.type_error(
                vm,
                argn,
                if nullable { "?GdImage" } else { "GdImage" },
                &raw,
            );
            Err(err(vm))
        }
        'a' => match &raw {
            Value::Array(a) => Ok(Arg::Arr(a.clone())),
            other => {
                f.type_error(vm, argn, if nullable { "?array" } else { "array" }, other);
                Err(err(vm))
            }
        },
        'F' => {
            if let Value::Object(o) = &raw {
                if let Some(font) = crate::font_of(o) {
                    return Ok(Arg::Font(font));
                }
                f.type_error(vm, argn, "GdFont|int", &raw);
                return Err(err(vm));
            }
            match scalar(vm, f, argn, 'l', "GdFont|int", &raw, strict, line)? {
                Arg::Long(n) => Ok(Arg::Long(n)),
                a => Ok(a),
            }
        }
        's' | 'p' | 'l' | 'b' | 'd' => {
            let tyname = match c {
                'l' => "int",
                'b' => "bool",
                'd' => "float",
                _ => "string",
            };
            let ty = if nullable {
                format!("?{}", tyname)
            } else {
                tyname.to_string()
            };
            scalar(vm, f, argn, c, &ty, &raw, strict, line)
        }
        _ => Ok(Arg::Any(raw)),
    }
}

#[allow(clippy::too_many_arguments)]
fn scalar(
    vm: &mut Vm,
    f: &Func,
    argn: usize,
    c: char,
    ty: &str,
    raw: &Value,
    strict: bool,
    line: u32,
) -> Result<Arg, VmError> {
    let mask = match c {
        'l' => T_INT,
        'b' => T_BOOL,
        'd' => T_FLOAT,
        _ => T_STRING,
    };
    if matches!(raw, Value::Null) {
        if strict {
            f.type_error(vm, argn, ty, raw);
            return Err(err(vm));
        }
        let msg = match f.params.get(argn - 1) {
            Some(p) => format!(
                "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
                f.name, argn, p, ty
            ),
            None => format!(
                "{}(): Passing null to parameter #{} of type {} is deprecated",
                f.name, argn, ty
            ),
        };
        vm.emit_deprecated(&msg);
        if vm.current_exception.is_some() {
            return Err(err(vm));
        }
        return Ok(match c {
            'l' => Arg::Long(0),
            'b' => Arg::Bool(false),
            'd' => Arg::Double(0.0),
            _ => Arg::Str(Vec::new()),
        });
    }
    let v = match vm.zpp_coerce_scalar(mask, raw, strict, line) {
        Ok(Some(v)) => v,
        Ok(None) => {
            f.type_error(vm, argn, ty, raw);
            return Err(err(vm));
        }
        Err(()) => return Err(err(vm)),
    };
    if vm.current_exception.is_some() {
        return Err(err(vm));
    }
    Ok(match (c, v) {
        ('l', Value::Long(n)) => Arg::Long(n),
        ('b', Value::True) => Arg::Bool(true),
        ('b', Value::False) => Arg::Bool(false),
        ('d', Value::Double(d)) => Arg::Double(d),
        ('d', Value::Long(n)) => Arg::Double(n as f64),
        (_, Value::String(s)) => {
            if c == 'p' && s.as_bytes().contains(&0) {
                return Err(f.value_error(vm, argn, "must not contain any null bytes"));
            }
            Arg::Str(s.as_bytes().to_vec())
        }
        (_, v) => Arg::Any(v),
    })
}
