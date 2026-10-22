//! zend_parse_parameters() for the specs the ftp functions use.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::arginfo::{T_BOOL, T_INT, T_STRING};
use goro_core::object::PhpObject;
use goro_core::value::Value;
use goro_core::vm::Vm;

pub type Obj = Rc<RefCell<PhpObject>>;

pub enum Arg {
    Absent,
    Obj(Obj),
    Str(Vec<u8>),
    Long(i64),
    Bool(bool),
    Res(i64),
    /// 'z': as passed (references kept)
    Any(Value),
}

impl Arg {
    pub fn str(&self) -> &[u8] {
        match self {
            Arg::Str(s) => s,
            _ => b"",
        }
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
    format!("{}(): Argument #{} (${})", fname, n, param_name(fname, n))
}

pub fn value_error(vm: &mut Vm, fname: &str, n: usize, msg: &str) {
    let m = format!("{} {}", arg_prefix(fname, n), msg);
    throw(vm, b"ValueError", &m);
}

pub fn type_error(vm: &mut Vm, fname: &str, n: usize, msg: &str) {
    let m = format!("{} {}", arg_prefix(fname, n), msg);
    throw(vm, b"TypeError", &m);
}

fn type_mismatch(vm: &mut Vm, fname: &str, n: usize, ty: &str, v: &Value) {
    let m = format!(
        "{} must be of type {}, {} given",
        arg_prefix(fname, n),
        ty,
        Vm::value_type_name(v)
    );
    throw(vm, b"TypeError", &m);
}

/// Parse `args` by `spec`: O (FTP\Connection), s, p (path: no NUL), l, b,
/// z, r (stream), `|` (optional from here). Err(()) once an exception is
/// pending. (The argument count was checked by the generic layer.)
pub fn parse(vm: &mut Vm, fname: &str, spec: &str, args: &[Value]) -> Result<Vec<Arg>, ()> {
    let strict = vm.caller_strict_types;
    let line = vm.current_line;
    let mut out = Vec::new();
    let mut ai = 0;
    for c in spec.chars() {
        if c == '|' {
            continue;
        }
        let argn = ai + 1;
        let raw = match args.get(ai) {
            None | Some(Value::Undef) => {
                out.push(Arg::Absent);
                ai += 1;
                continue;
            }
            Some(v) => v.deref(),
        };
        match c {
            'O' => match &raw {
                Value::Object(o)
                    if o.borrow()
                        .class_name
                        .eq_ignore_ascii_case(crate::CLASS_NAME) =>
                {
                    out.push(Arg::Obj(o.clone()))
                }
                other => {
                    type_mismatch(vm, fname, argn, "FTP\\Connection", other);
                    return Err(());
                }
            },
            'z' => out.push(Arg::Any(args[ai].clone())),
            'r' => match &raw {
                Value::Resource(id) => {
                    let ok = goro_core::value::is_open_resource(*id)
                        && matches!(
                            goro_core::value::resource_type_of(*id).as_deref(),
                            Some(b"stream") | Some(b"persistent stream")
                        );
                    if !ok && !(1..=3).contains(id) {
                        throw(
                            vm,
                            b"TypeError",
                            &format!(
                                "{}(): supplied resource is not a valid stream resource",
                                fname
                            ),
                        );
                        return Err(());
                    }
                    out.push(Arg::Res(*id));
                }
                other => {
                    type_mismatch(vm, fname, argn, "resource", other);
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
                    if strict {
                        type_mismatch(vm, fname, argn, tyname, &raw);
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
                        _ => Value::String(goro_core::string::PhpString::empty()),
                    }
                } else {
                    match vm.zpp_coerce_scalar(mask, &raw, strict, line) {
                        Ok(Some(v)) => v,
                        Ok(None) => {
                            type_mismatch(vm, fname, argn, tyname, &raw);
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
                            let m = format!(
                                "{} must not contain any null bytes",
                                arg_prefix(fname, argn)
                            );
                            throw(vm, b"ValueError", &m);
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
