//! zend_parse_parameters for the mailparse functions and methods.

use goro_core::arginfo::{T_INT, T_STRING};
use goro_core::value::Value;
use goro_core::vm::Vm;

#[derive(Clone, Debug)]
pub enum Arg {
    Absent,
    Null,
    Str(Vec<u8>),
    Long(i64),
    /// 'z' and 'r' (as passed: references kept for 'z')
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
        let params = goro_core::stubs::find_method(class.as_bytes(), method.as_bytes())
            .map(|(_, m)| m.args.iter().map(|a| a.name).collect())
            .unwrap_or_default();
        Func {
            name: format!("{}::{}", class, method),
            params,
        }
    }

    /// "func(): Argument #n ($name)"
    pub fn arg_prefix(&self, n: usize) -> String {
        match self.params.get(n - 1) {
            Some(p) => format!("{}(): Argument #{} (${})", self.name, n, p),
            None => format!("{}(): Argument #{}", self.name, n),
        }
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

/// zend_parse_parameters with the format characters used here: s, S, l,
/// z and r, with `!` (nullable) and `|`. Err: an exception was thrown.
pub fn parse(vm: &mut Vm, f: &Func, spec: &str, args: &[Value]) -> Result<Vec<Arg>, ()> {
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
        let (what, count) = if min == max {
            ("exactly", min)
        } else if given < min {
            ("at least", min)
        } else {
            ("at most", max)
        };
        let msg = format!(
            "{}() expects {} {} argument{}, {} given",
            f.name,
            what,
            count,
            plural(count),
            given
        );
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
        if c == 'r' {
            if !matches!(raw, Value::Resource(_)) {
                f.type_error(vm, argn, "resource", &raw);
                return Err(());
            }
            out.push(Arg::Any(raw));
            continue;
        }
        let (mask, tyname) = if c == 'l' {
            (T_INT, "int")
        } else {
            (T_STRING, "string")
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
            out.push(if c == 'l' {
                Arg::Long(0)
            } else {
                Arg::Str(Vec::new())
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
        out.push(match v {
            Value::Long(n) => Arg::Long(n),
            Value::String(s) => Arg::Str(s.as_bytes().to_vec()),
            v => Arg::Any(v),
        });
    }
    Ok(out)
}
