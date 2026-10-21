//! zend_parse_parameters / zend_parse_method_parameters for the mysqli
//! functions and methods.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::arginfo::{T_ARRAY, T_BOOL, T_FLOAT, T_INT, T_STRING};
use goro_core::array::PhpArray;
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

pub type Obj = Rc<RefCell<PhpObject>>;
pub type R<T> = Result<T, ()>;

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
    /// the variadic rest (as passed, references kept)
    Rest(Vec<Value>),
    /// a class name ('C'): the lowercase name
    Class(Vec<u8>),
}

impl Arg {
    pub fn str(&self) -> Option<&[u8]> {
        match self {
            Arg::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn long(&self) -> Option<i64> {
        match self {
            Arg::Long(n) => Some(*n),
            _ => None,
        }
    }
    pub fn long_or(&self, d: i64) -> i64 {
        self.long().unwrap_or(d)
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
    pub fn is_given(&self) -> bool {
        !matches!(self, Arg::Absent | Arg::Null)
    }
}

pub fn throw(vm: &mut Vm, class: &[u8], msg: &str) {
    let e = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(e);
}

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

pub fn obj_is(vm: &Vm, o: &Obj, target: &[u8]) -> bool {
    let cn = o.borrow().class_name.clone();
    class_is(vm, &cn, target)
}

/// The function being run, for messages: "mysqli_query" or
/// "mysqli::query", with its parameter names.
#[derive(Clone)]
pub struct Func {
    pub name: String,
    /// whether called as a method (getThis() set)
    pub is_method: bool,
    pub this: Option<Obj>,
    pub params: Vec<&'static str>,
}

impl Func {
    pub fn function(name: &str) -> Func {
        let params = goro_core::stubs::find_function(name.as_bytes())
            .map(|f| f.args.iter().map(|a| a.name).collect())
            .unwrap_or_default();
        Func {
            name: name.to_string(),
            is_method: false,
            this: None,
            params,
        }
    }

    pub fn method(class: &str, method: &str, this: Option<Obj>) -> Func {
        let (cname, mname, params) =
            match goro_core::stubs::find_method(class.as_bytes(), method.as_bytes()) {
                Some((c, m)) => (
                    c.name.to_string(),
                    m.name.to_string(),
                    m.args.iter().map(|a| a.name).collect(),
                ),
                None => (class.to_string(), method.to_string(), Vec::new()),
            };
        Func {
            name: format!("{}::{}", cname, mname),
            is_method: true,
            this,
            params,
        }
    }

    /// ERROR_ARG_POS(arg_num)
    pub fn arg_pos(&self, n: usize) -> usize {
        if self.is_method { n - 1 } else { n }
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
    pub fn value_error(&self, vm: &mut Vm, n: usize, msg: &str) {
        let m = format!("{} {}", self.arg_prefix(n), msg);
        throw(vm, b"ValueError", &m);
    }

    /// zend_argument_must_not_be_empty_error
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

    /// zend_argument_count_error (the message carries no function name)
    pub fn count_error(&self, vm: &mut Vm, msg: &str) {
        throw(vm, b"ArgumentCountError", msg);
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// zend_parse_parameters with a spec of the C format characters this
/// extension uses: O (object of `classes[i]`), s, l, b, d, a, h, z, C, *, +,
/// with `!` (nullable) and `|`. When `f.is_method`, a leading "O" is the
/// object ($this) and is not taken from `args`.
pub fn parse(vm: &mut Vm, f: &Func, spec: &str, classes: &[&[u8]], args: &[Value]) -> R<Vec<Arg>> {
    // tokenize
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
    let mut out: Vec<Arg> = Vec::new();
    let mut tstart = 0;
    let mut class_idx = 0;
    if f.is_method && toks.first().map(|t| t.0) == Some('O') {
        out.push(match &f.this {
            Some(o) => Arg::Obj(o.clone()),
            None => Arg::Absent,
        });
        tstart = 1;
        class_idx = 1;
        if let Some(o) = opt_from.as_mut() {
            *o = o.saturating_sub(1);
        }
    }
    let toks = &toks[tstart..];
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
    }
    .min(n_fixed + toks.iter().filter(|t| t.0 == '+').count());
    let max = if variadic { usize::MAX } else { n_fixed };
    // trailing Undef (skipped optional named args) do not count
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
        throw(vm, b"ArgumentCountError", &msg);
        return Err(());
    }
    let strict = vm.caller_strict_types;
    let line = vm.current_line;
    let mut ai = 0usize;
    for (ti, &(c, nullable)) in toks.iter().enumerate() {
        let argn = ai + 1; // 1-based position in args
        if c == '*' || c == '+' {
            out.push(Arg::Rest(args[ai.min(args.len())..].to_vec()));
            ai = args.len();
            continue;
        }
        let _ = ti;
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
                let cls = classes.get(class_idx).copied().unwrap_or(b"mysqli");
                class_idx += 1;
                match &raw {
                    Value::Object(o) if obj_is(vm, o, cls) => out.push(Arg::Obj(o.clone())),
                    other => {
                        let disp = class_display(cls);
                        let ty = if nullable { format!("?{}", disp) } else { disp };
                        f.type_error(vm, argn, &ty, other);
                        return Err(());
                    }
                }
            }
            'z' => out.push(Arg::Any(args[ai - 1].clone())),
            'a' | 'h' => match &raw {
                Value::Array(a) => out.push(Arg::Arr(a.clone())),
                other => {
                    let ty = if nullable { "?array" } else { "array" };
                    f.type_error(vm, argn, ty, other);
                    return Err(());
                }
            },
            'C' => {
                let name = match &raw {
                    Value::String(s) => s.as_bytes().to_vec(),
                    // try_convert_to_string(): objects throw rather than TypeError
                    Value::Object(_) => match vm.value_to_string_checked(&raw) {
                        Ok(s) => s.as_bytes().to_vec(),
                        Err(_) => return Err(()),
                    },
                    other => match vm.zpp_coerce_scalar(T_STRING, other, strict, line) {
                        Ok(Some(Value::String(s))) => s.as_bytes().to_vec(),
                        Ok(_) => {
                            f.type_error(vm, argn, "string", other);
                            return Err(());
                        }
                        Err(()) => return Err(()),
                    },
                };
                out.push(Arg::Class(name));
            }
            's' | 'l' | 'b' | 'd' => {
                let (mask, tyname) = match c {
                    's' => (T_STRING, "string"),
                    'l' => (T_INT, "int"),
                    'b' => (T_BOOL, "bool"),
                    _ => (T_FLOAT, "float"),
                };
                if matches!(raw, Value::Null) {
                    if strict {
                        f.type_error(vm, argn, tyname, &raw);
                        return Err(());
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
                        return Err(());
                    }
                    out.push(match c {
                        's' => Arg::Str(Vec::new()),
                        'l' => Arg::Long(0),
                        'b' => Arg::Bool(false),
                        _ => Arg::Double(0.0),
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
                out.push(match (c, v) {
                    ('s', Value::String(s)) => Arg::Str(s.as_bytes().to_vec()),
                    ('l', Value::Long(n)) => Arg::Long(n),
                    ('b', Value::True) => Arg::Bool(true),
                    ('b', Value::False) => Arg::Bool(false),
                    ('d', Value::Double(d)) => Arg::Double(d),
                    ('d', Value::Long(n)) => Arg::Double(n as f64),
                    (_, v) => Arg::Any(v),
                });
            }
            _ => out.push(Arg::Any(raw)),
        }
    }
    let _ = T_ARRAY;
    Ok(out)
}

/// The display name of an internal mysqli class
pub fn class_display(lower: &[u8]) -> String {
    match lower {
        b"mysqli" => "mysqli",
        b"mysqli_result" => "mysqli_result",
        b"mysqli_stmt" => "mysqli_stmt",
        b"mysqli_warning" => "mysqli_warning",
        b"mysqli_driver" => "mysqli_driver",
        other => return String::from_utf8_lossy(other).into_owned(),
    }
    .to_string()
}

pub fn vstr(s: impl AsRef<[u8]>) -> Value {
    Value::String(PhpString::from_bytes(s.as_ref()))
}
