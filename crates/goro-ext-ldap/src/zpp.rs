//! zend_parse_parameters for ext/ldap, with the exact error messages
//! php-src produces.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::arginfo::{T_BOOL, T_INT, T_STRING};
use goro_core::object::PhpObject;
use goro_core::value::Value;
use goro_core::vm::Vm;

pub type Obj = Rc<RefCell<PhpObject>>;

pub const CONN_CLASS: &[u8] = b"LDAP\\Connection";
pub const RESULT_CLASS: &[u8] = b"LDAP\\Result";
pub const ENTRY_CLASS: &[u8] = b"LDAP\\ResultEntry";

pub enum Arg {
    Absent,
    Null,
    Obj(Obj),
    Str(Vec<u8>),
    Long(i64),
    Bool(bool),
    /// 'z': the argument as passed (references kept)
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
    pub fn any(&self) -> Option<Value> {
        match self {
            Arg::Any(v) => Some(v.clone()),
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

pub fn class_display(class: &[u8]) -> &'static str {
    match class {
        CONN_CLASS => "LDAP\\Connection",
        RESULT_CLASS => "LDAP\\Result",
        ENTRY_CLASS => "LDAP\\ResultEntry",
        _ => "object",
    }
}

fn obj_of_class(v: &Value, class: &[u8]) -> Option<Obj> {
    match v.deref() {
        Value::Object(o) if o.borrow().class_name.eq_ignore_ascii_case(class) => Some(o),
        _ => None,
    }
}

/// Parse by `spec`. Chars:
///   C = LDAP\Connection, c = ?LDAP\Connection (O!),
///   R = LDAP\Result, E = LDAP\ResultEntry,
///   s = string, p = path string (no NUL), l = int, b = bool,
///   z = zval (as passed), h = ?array (controls),
///   `|` optional from here, `!` after s/p makes it nullable.
/// Argument count is assumed already checked by the generic layer.
pub fn parse(vm: &mut Vm, fname: &str, spec: &str, args: &[Value]) -> Result<Vec<Arg>, ()> {
    let strict = vm.caller_strict_types;
    let line = vm.current_line;
    let mut out = Vec::new();
    let chars: Vec<char> = spec.chars().collect();
    let mut ci = 0;
    let mut ai = 0;
    while ci < chars.len() {
        let c = chars[ci];
        if c == '|' {
            ci += 1;
            continue;
        }
        let nullable = chars.get(ci + 1) == Some(&'!');
        ci += if nullable { 2 } else { 1 };
        let argn = ai + 1;
        let raw = match args.get(ai) {
            None | Some(Value::Undef) => {
                out.push(Arg::Absent);
                ai += 1;
                continue;
            }
            Some(v) => v.deref(),
        };
        ai += 1;
        match c {
            'z' => out.push(Arg::Any(args[argn - 1].clone())),
            'C' | 'c' | 'R' | 'E' => {
                if nullable || c == 'c' {
                    if matches!(raw, Value::Null) {
                        out.push(Arg::Null);
                        continue;
                    }
                }
                let class: &[u8] = match c {
                    'C' | 'c' => CONN_CLASS,
                    'R' => RESULT_CLASS,
                    _ => ENTRY_CLASS,
                };
                match obj_of_class(&raw, class) {
                    Some(o) => out.push(Arg::Obj(o)),
                    None => {
                        let ty = if c == 'c' {
                            format!("?{}", class_display(class))
                        } else {
                            class_display(class).to_string()
                        };
                        type_mismatch(vm, fname, argn, &ty, &raw);
                        return Err(());
                    }
                }
            }
            's' | 'p' | 'l' | 'b' => {
                let (mask, tyname) = match c {
                    's' | 'p' => (T_STRING, "string"),
                    'b' => (T_BOOL, "bool"),
                    _ => (T_INT, "int"),
                };
                if matches!(raw, Value::Null) {
                    if nullable {
                        out.push(Arg::Null);
                        continue;
                    }
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
                        type_mismatch(vm, fname, argn, tyname, &raw);
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
                        out.push(Arg::Str(s.as_bytes().to_vec()));
                    }
                    Value::Long(n) => out.push(Arg::Long(n)),
                    Value::True => out.push(Arg::Bool(true)),
                    Value::False => out.push(Arg::Bool(false)),
                    other => out.push(Arg::Any(other)),
                }
            }
            'h' => {
                // ?array (server controls)
                match &raw {
                    Value::Null => out.push(Arg::Null),
                    Value::Array(_) => out.push(Arg::Any(raw)),
                    other => {
                        type_mismatch(vm, fname, argn, "?array", other);
                        return Err(());
                    }
                }
            }
            'a' => match &raw {
                Value::Array(_) => out.push(Arg::Any(raw)),
                other => {
                    type_mismatch(vm, fname, argn, "array", other);
                    return Err(());
                }
            },
            _ => out.push(Arg::Any(raw)),
        }
    }
    Ok(out)
}
