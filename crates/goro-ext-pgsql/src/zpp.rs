//! zend_parse_parameters() for the specs the pgsql functions use.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::arginfo::{T_BOOL, T_INT, T_STRING};
use goro_core::array::PhpArray;
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

pub type Obj = Rc<RefCell<PhpObject>>;

#[derive(Clone)]
pub enum Arg {
    Absent,
    Null,
    Obj(Obj),
    Str(Vec<u8>),
    Long(i64),
    Bool(bool),
    Arr(Rc<RefCell<PhpArray>>),
    Res(i64),
    /// 'z': as passed (dereferenced)
    Any(Value),
}

impl Arg {
    pub fn str(&self) -> &[u8] {
        match self {
            Arg::Str(s) => s,
            _ => b"",
        }
    }
    pub fn opt_str(&self) -> Option<&[u8]> {
        match self {
            Arg::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn long_or(&self, d: i64) -> i64 {
        match self {
            Arg::Long(n) => *n,
            _ => d,
        }
    }
    pub fn opt_long(&self) -> Option<i64> {
        match self {
            Arg::Long(n) => Some(*n),
            _ => None,
        }
    }
    pub fn bool_or(&self, d: bool) -> bool {
        match self {
            Arg::Bool(b) => *b,
            _ => d,
        }
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
}

pub fn throw(vm: &mut Vm, class: &[u8], msg: &str) {
    let e = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(e);
}

pub fn param_name(fname: &str, n: usize) -> &'static str {
    goro_core::stubs::find_function(fname.as_bytes())
        .and_then(|f| f.args.get(n - 1))
        .map(|a| a.name)
        .unwrap_or("")
}

/// "func(): Argument #n ($name)"
pub fn arg_prefix(fname: &str, n: usize) -> String {
    let p = param_name(fname, n);
    if p.is_empty() {
        format!("{}(): Argument #{}", fname, n)
    } else {
        format!("{}(): Argument #{} (${})", fname, n, p)
    }
}

/// zend_argument_value_error
pub fn value_error(vm: &mut Vm, fname: &str, n: usize, msg: &str) {
    let m = format!("{} {}", arg_prefix(fname, n), msg);
    throw(vm, b"ValueError", &m);
}

/// zend_argument_type_error
pub fn type_error(vm: &mut Vm, fname: &str, n: usize, msg: &str) {
    let m = format!("{} {}", arg_prefix(fname, n), msg);
    throw(vm, b"TypeError", &m);
}

/// zend_argument_must_not_be_empty_error
pub fn must_not_be_empty(vm: &mut Vm, fname: &str, n: usize) {
    value_error(vm, fname, n, "must not be empty");
}

pub fn type_mismatch(vm: &mut Vm, fname: &str, n: usize, ty: &str, v: &Value) {
    let m = format!(
        "{} must be of type {}, {} given",
        arg_prefix(fname, n),
        ty,
        Vm::value_type_name(v)
    );
    throw(vm, b"TypeError", &m);
}

/// The class an 'O' argument must be
pub fn class_display(c: u8) -> &'static str {
    match c {
        b'C' => "PgSql\\Connection",
        b'R' => "PgSql\\Result",
        _ => "PgSql\\Lob",
    }
}

fn class_matches(o: &Obj, c: u8) -> bool {
    o.borrow()
        .class_name
        .eq_ignore_ascii_case(class_display(c).as_bytes())
}

/// Parse `args` by `spec`:
///   C / R / L: PgSql\Connection / Result / Lob object; `?` before makes it
///              nullable
///   s, p (path: no NUL), l, b, a (array), z (any), r (stream resource),
///   `|` optional from here, `!` after s/l: nullable
/// Err(()) once an exception is pending (with `quiet`, nothing is thrown).
pub fn parse_ex(
    vm: &mut Vm,
    fname: &str,
    spec: &str,
    args: &[Value],
    quiet: bool,
) -> Result<Vec<Arg>, ()> {
    let strict = vm.caller_strict_types;
    let line = vm.current_line;
    let mut out = Vec::new();
    let mut ai = 0;
    let chars: Vec<char> = spec.chars().collect();
    let mut ci = 0;
    let mut optional = false;
    let required = spec
        .split('|')
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| !matches!(c, '?' | '!'))
        .count();
    let max = spec
        .chars()
        .filter(|c| !matches!(c, '?' | '!' | '|'))
        .count();
    let n = args.iter().filter(|a| !matches!(a, Value::Undef)).count();
    if quiet && (n < required || args.len() > max) {
        return Err(());
    }
    while ci < chars.len() {
        let mut c = chars[ci];
        ci += 1;
        if c == '|' {
            optional = true;
            continue;
        }
        let mut nullable = false;
        if c == '?' {
            nullable = true;
            c = chars[ci];
            ci += 1;
        }
        if ci < chars.len() && chars[ci] == '!' {
            nullable = true;
            ci += 1;
        }
        let _ = optional;
        let argn = ai + 1;
        let raw = match args.get(ai) {
            None | Some(Value::Undef) => {
                out.push(Arg::Absent);
                ai += 1;
                continue;
            }
            Some(v) => v.deref(),
        };
        if nullable && matches!(raw, Value::Null) {
            out.push(Arg::Null);
            ai += 1;
            continue;
        }
        match c {
            'C' | 'R' | 'L' => match &raw {
                Value::Object(o) if class_matches(o, c as u8) => out.push(Arg::Obj(o.clone())),
                other => {
                    if !quiet {
                        let ty = if nullable {
                            format!("?{}", class_display(c as u8))
                        } else {
                            class_display(c as u8).to_string()
                        };
                        type_mismatch(vm, fname, argn, &ty, other);
                    }
                    return Err(());
                }
            },
            'z' => out.push(Arg::Any(raw)),
            'a' => match &raw {
                Value::Array(a) => out.push(Arg::Arr(a.clone())),
                other => {
                    if !quiet {
                        type_mismatch(vm, fname, argn, "array", other);
                    }
                    return Err(());
                }
            },
            'r' => match &raw {
                Value::Resource(id) => out.push(Arg::Res(id.id())),
                other => {
                    if !quiet {
                        type_mismatch(vm, fname, argn, "resource", other);
                    }
                    return Err(());
                }
            },
            's' | 'p' | 'l' | 'b' => {
                let (mask, tyname) = match c {
                    's' | 'p' => (T_STRING, "string"),
                    'b' => (T_BOOL, "bool"),
                    _ => (T_INT, "int"),
                };
                let v = if matches!(raw, Value::Null) {
                    if strict || quiet {
                        if !quiet {
                            type_mismatch(vm, fname, argn, tyname, &raw);
                        }
                        return Err(());
                    }
                    vm.emit_deprecated(&format!(
                        "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
                        fname,
                        argn,
                        param_name(fname, argn),
                        tyname
                    ));
                    if vm.current_exception.is_some() {
                        return Err(());
                    }
                    match c {
                        'l' => Value::Long(0),
                        'b' => Value::False,
                        _ => Value::String(PhpString::empty()),
                    }
                } else if quiet {
                    // ZEND_PARSE_PARAMS_QUIET: no conversions that warn
                    match (&raw, c) {
                        (Value::String(_), 's' | 'p') => raw.clone(),
                        (
                            Value::Long(_) | Value::Double(_) | Value::True | Value::False,
                            's' | 'p',
                        ) if !strict => Value::String(raw.to_php_string()),
                        (Value::Long(_), 'l') => raw.clone(),
                        (Value::Double(d), 'l') if !strict && d.fract() == 0.0 && d.is_finite() => {
                            Value::Long(*d as i64)
                        }
                        (Value::String(s), 'l') if !strict => {
                            match goro_core::value::parse_numeric_str(s.as_bytes(), false) {
                                Some((goro_core::value::NumStr::Long(n), _)) => Value::Long(n),
                                _ => return Err(()),
                            }
                        }
                        (Value::True | Value::False, 'l') if !strict => Value::Long(raw.to_long()),
                        (_, 'b') => match vm.zpp_coerce_scalar(mask, &raw, strict, line) {
                            Ok(Some(v)) => v,
                            _ => return Err(()),
                        },
                        _ => return Err(()),
                    }
                } else {
                    match vm.zpp_coerce_scalar(mask, &raw, strict, line) {
                        Ok(Some(v)) => v,
                        Ok(None) => {
                            let ty = if nullable {
                                format!("?{}", tyname)
                            } else {
                                tyname.to_string()
                            };
                            type_mismatch(vm, fname, argn, &ty, &raw);
                            return Err(());
                        }
                        Err(()) => return Err(()),
                    }
                };
                if vm.current_exception.is_some() {
                    return Err(());
                }
                match v {
                    Value::String(s) => {
                        if c == 'p' && s.as_bytes().contains(&0) {
                            if !quiet {
                                let m = format!(
                                    "{} must not contain any null bytes",
                                    arg_prefix(fname, argn)
                                );
                                throw(vm, b"ValueError", &m);
                            }
                            return Err(());
                        }
                        out.push(Arg::Str(s.as_bytes().to_vec()))
                    }
                    Value::Long(n) => out.push(Arg::Long(n)),
                    Value::True => out.push(Arg::Bool(true)),
                    Value::False => out.push(Arg::Bool(false)),
                    other => out.push(Arg::Any(other)),
                }
            }
            _ => out.push(Arg::Any(raw)),
        }
        ai += 1;
    }
    Ok(out)
}

pub fn parse(vm: &mut Vm, fname: &str, spec: &str, args: &[Value]) -> Result<Vec<Arg>, ()> {
    parse_ex(vm, fname, spec, args, false)
}

/// Z_PARAM_STR_OR_LONG: Ok(Str) / Ok(Long)
pub fn str_or_long(vm: &mut Vm, fname: &str, n: usize, v: &Value) -> Result<Arg, ()> {
    let v = v.deref();
    match &v {
        Value::String(s) => return Ok(Arg::Str(s.as_bytes().to_vec())),
        Value::Long(l) => return Ok(Arg::Long(*l)),
        _ => {}
    }
    let strict = vm.caller_strict_types;
    let line = vm.current_line;
    if matches!(v, Value::Null) && !strict {
        vm.emit_deprecated(&format!(
            "{}(): Passing null to parameter #{} (${}) of type string|int is deprecated",
            fname,
            n,
            param_name(fname, n)
        ));
        if vm.current_exception.is_some() {
            return Err(());
        }
        return Ok(Arg::Str(Vec::new()));
    }
    match vm.zpp_coerce_scalar(T_INT | T_STRING, &v, strict, line) {
        Ok(Some(Value::Long(l))) => Ok(Arg::Long(l)),
        Ok(Some(Value::String(s))) => Ok(Arg::Str(s.as_bytes().to_vec())),
        Ok(_) => {
            type_mismatch(vm, fname, n, "string|int", &v);
            Err(())
        }
        Err(()) => Err(()),
    }
}

/// zend_wrong_parameters_count_error
pub fn wrong_count(vm: &mut Vm, fname: &str, min: usize, max: usize, given: usize) {
    let msg = if min == max {
        format!(
            "{}() expects exactly {} argument{}, {} given",
            fname,
            min,
            if min == 1 { "" } else { "s" },
            given
        )
    } else if given < min {
        format!(
            "{}() expects at least {} argument{}, {} given",
            fname,
            min,
            if min == 1 { "" } else { "s" },
            given
        )
    } else {
        format!(
            "{}() expects at most {} argument{}, {} given",
            fname,
            max,
            if max == 1 { "" } else { "s" },
            given
        )
    };
    throw(vm, b"ArgumentCountError", &msg);
}
