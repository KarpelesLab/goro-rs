//! Minimal emulation of PHP's zend_parse_parameters for the date extension.

use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

pub type R<T> = Result<T, VmError>;

pub fn throw(vm: &mut Vm, class: &[u8], msg: &str) -> VmError {
    let exc = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError {
        message: msg.to_string(),
        line: vm.current_line,
    }
}

pub fn type_name(v: &Value) -> String {
    Vm::value_type_name(v)
}

/// Is the argument present (not Undef)?
#[allow(dead_code)]
pub fn present(args: &[Value], idx: usize) -> bool {
    !matches!(args.get(idx), None | Some(Value::Undef))
}

pub fn check_count(vm: &mut Vm, fname: &str, args: &[Value], min: usize, max: usize) -> R<()> {
    let n = args
        .iter()
        .rposition(|a| !matches!(a, Value::Undef))
        .map(|p| p + 1)
        .unwrap_or(0);
    if n < min {
        let msg = if min == max {
            format!(
                "{}() expects exactly {} argument{}, {} given",
                fname,
                min,
                if min == 1 { "" } else { "s" },
                n
            )
        } else {
            format!(
                "{}() expects at least {} argument{}, {} given",
                fname,
                min,
                if min == 1 { "" } else { "s" },
                n
            )
        };
        return Err(throw(vm, b"ArgumentCountError", &msg));
    }
    if n > max {
        let msg = if min == max {
            format!(
                "{}() expects exactly {} argument{}, {} given",
                fname,
                max,
                if max == 1 { "" } else { "s" },
                n
            )
        } else {
            format!(
                "{}() expects at most {} argument{}, {} given",
                fname,
                max,
                if max == 1 { "" } else { "s" },
                n
            )
        };
        return Err(throw(vm, b"ArgumentCountError", &msg));
    }
    Ok(())
}

fn arg_err(
    vm: &mut Vm,
    fname: &str,
    idx: usize,
    pname: &str,
    expected: &str,
    v: &Value,
) -> VmError {
    let msg = format!(
        "{}(): Argument #{} (${}) must be of type {}, {} given",
        fname,
        idx + 1,
        pname,
        expected,
        type_name(v)
    );
    throw(vm, b"TypeError", &msg)
}

fn null_deprecation(vm: &mut Vm, fname: &str, idx: usize, pname: &str, expected: &str) {
    let msg = format!(
        "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
        fname,
        idx + 1,
        pname,
        expected
    );
    vm.emit_deprecated(&msg);
}

/// Parse a PHP numeric string (allowing leading/trailing whitespace).
/// Returns Some(Ok(i64)) for integers, Some(Err(f64)) for floats.
pub fn numeric_str(s: &[u8]) -> Option<Result<i64, f64>> {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let mut j = s.len();
    while j > i && matches!(s[j - 1], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        j -= 1;
    }
    let t = &s[i..j];
    if t.is_empty() {
        return None;
    }
    let st = std::str::from_utf8(t).ok()?;
    // integer?
    let digits = st.strip_prefix(['+', '-']).unwrap_or(st);
    if !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()) {
        if let Ok(v) = st.parse::<i64>() {
            return Some(Ok(v));
        }
        return st.parse::<f64>().ok().map(Err);
    }
    // float: digits with optional . and exponent
    let b = digits.as_bytes();
    let mut k = 0;
    let mut nd = 0;
    while k < b.len() && b[k].is_ascii_digit() {
        k += 1;
        nd += 1;
    }
    if k < b.len() && b[k] == b'.' {
        k += 1;
        while k < b.len() && b[k].is_ascii_digit() {
            k += 1;
            nd += 1;
        }
    }
    if nd == 0 {
        return None;
    }
    if k < b.len() && (b[k] == b'e' || b[k] == b'E') {
        k += 1;
        if k < b.len() && (b[k] == b'+' || b[k] == b'-') {
            k += 1;
        }
        let ds = k;
        while k < b.len() && b[k].is_ascii_digit() {
            k += 1;
        }
        if k == ds {
            return None;
        }
    }
    if k != b.len() {
        return None;
    }
    st.parse::<f64>().ok().map(Err)
}

fn double_fits_long(d: f64) -> bool {
    d.is_finite() && (-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&d)
}

fn float_to_long(
    vm: &mut Vm,
    fname: &str,
    idx: usize,
    pname: &str,
    d: f64,
    orig: &Value,
    from_string: Option<&[u8]>,
) -> R<i64> {
    if !double_fits_long(d) {
        return Err(arg_err(vm, fname, idx, pname, "int", orig));
    }
    let l = d as i64;
    if (l as f64) != d {
        match from_string {
            Some(s) => vm.emit_deprecated(&format!(
                "Implicit conversion from float-string \"{}\" to int loses precision",
                goro_core::string::msg_str(s)
            )),
            None => vm.emit_deprecated(&format!(
                "Implicit conversion from float {} to int loses precision",
                goro_core::value::format_php_float_repr(d)
            )),
        }
    }
    Ok(l)
}

/// Z_PARAM_LONG / Z_PARAM_LONG_OR_NULL. Returns None if absent (or null and nullable).
pub fn long_arg(
    vm: &mut Vm,
    fname: &str,
    args: &[Value],
    idx: usize,
    pname: &str,
    nullable: bool,
) -> R<Option<i64>> {
    let v = match args.get(idx) {
        None | Some(Value::Undef) => return Ok(None),
        Some(v) => v.deref(),
    };
    match &v {
        Value::Null => {
            if nullable {
                Ok(None)
            } else {
                null_deprecation(vm, fname, idx, pname, "int");
                Ok(Some(0))
            }
        }
        Value::Long(l) => Ok(Some(*l)),
        Value::Double(d) => float_to_long(vm, fname, idx, pname, *d, &v, None).map(Some),
        Value::True => Ok(Some(1)),
        Value::False => Ok(Some(0)),
        Value::String(s) => match numeric_str(s.as_bytes()) {
            Some(Ok(l)) => Ok(Some(l)),
            Some(Err(d)) => {
                float_to_long(vm, fname, idx, pname, d, &v, Some(s.as_bytes())).map(Some)
            }
            None => Err(arg_err(
                vm,
                fname,
                idx,
                pname,
                if nullable { "?int" } else { "int" },
                &v,
            )),
        },
        _ => Err(arg_err(
            vm,
            fname,
            idx,
            pname,
            if nullable { "?int" } else { "int" },
            &v,
        )),
    }
}

pub fn double_arg(
    vm: &mut Vm,
    fname: &str,
    args: &[Value],
    idx: usize,
    pname: &str,
    nullable: bool,
) -> R<Option<f64>> {
    let v = match args.get(idx) {
        None | Some(Value::Undef) => return Ok(None),
        Some(v) => v.deref(),
    };
    match &v {
        Value::Null => {
            if nullable {
                Ok(None)
            } else {
                null_deprecation(vm, fname, idx, pname, "float");
                Ok(Some(0.0))
            }
        }
        Value::Long(l) => Ok(Some(*l as f64)),
        Value::Double(d) => Ok(Some(*d)),
        Value::True => Ok(Some(1.0)),
        Value::False => Ok(Some(0.0)),
        Value::String(s) => match numeric_str(s.as_bytes()) {
            Some(Ok(l)) => Ok(Some(l as f64)),
            Some(Err(d)) => Ok(Some(d)),
            None => Err(arg_err(
                vm,
                fname,
                idx,
                pname,
                if nullable { "?float" } else { "float" },
                &v,
            )),
        },
        _ => Err(arg_err(
            vm,
            fname,
            idx,
            pname,
            if nullable { "?float" } else { "float" },
            &v,
        )),
    }
}

pub fn bool_arg(
    vm: &mut Vm,
    fname: &str,
    args: &[Value],
    idx: usize,
    pname: &str,
) -> R<Option<bool>> {
    let v = match args.get(idx) {
        None | Some(Value::Undef) => return Ok(None),
        Some(v) => v.deref(),
    };
    match &v {
        Value::Null => {
            null_deprecation(vm, fname, idx, pname, "bool");
            Ok(Some(false))
        }
        Value::True => Ok(Some(true)),
        Value::False => Ok(Some(false)),
        Value::Long(_) | Value::Double(_) | Value::String(_) => Ok(Some(v.is_truthy())),
        _ => Err(arg_err(vm, fname, idx, pname, "bool", &v)),
    }
}

/// Z_PARAM_STRING / Z_PARAM_STR (and _OR_NULL).
pub fn str_arg(
    vm: &mut Vm,
    fname: &str,
    args: &[Value],
    idx: usize,
    pname: &str,
    nullable: bool,
) -> R<Option<Vec<u8>>> {
    let v = match args.get(idx) {
        None | Some(Value::Undef) => return Ok(None),
        Some(v) => v.deref(),
    };
    match &v {
        Value::Null => {
            if nullable {
                Ok(None)
            } else {
                null_deprecation(vm, fname, idx, pname, "string");
                Ok(Some(Vec::new()))
            }
        }
        Value::String(s) => Ok(Some(s.as_bytes().to_vec())),
        Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
            Ok(Some(v.to_php_string().as_bytes().to_vec()))
        }
        Value::Object(o) => {
            let has_tostring = {
                let cl = o.borrow().class_name.to_ascii_lowercase();
                vm.classes
                    .get(&cl)
                    .map(|c| c.find_method_in_chain_has(b"__tostring"))
                    .unwrap_or(false)
            };
            if has_tostring {
                if let Some(r) = vm.call_object_method(&v, b"__toString", &[]) {
                    return Ok(Some(r.to_php_string().as_bytes().to_vec()));
                }
            }
            Err(arg_err(
                vm,
                fname,
                idx,
                pname,
                if nullable { "?string" } else { "string" },
                &v,
            ))
        }
        _ => Err(arg_err(
            vm,
            fname,
            idx,
            pname,
            if nullable { "?string" } else { "string" },
            &v,
        )),
    }
}

/// Z_PARAM_PATH: like a string but must not contain NUL bytes.
pub fn path_arg(
    vm: &mut Vm,
    fname: &str,
    args: &[Value],
    idx: usize,
    pname: &str,
) -> R<Option<Vec<u8>>> {
    let s = str_arg(vm, fname, args, idx, pname, false)?;
    if let Some(ref b) = s {
        if b.contains(&0) {
            let msg = format!(
                "{}(): Argument #{} (${}) must not contain any null bytes",
                fname,
                idx + 1,
                pname
            );
            return Err(throw(vm, b"ValueError", &msg));
        }
    }
    Ok(s)
}

pub fn array_arg(
    vm: &mut Vm,
    fname: &str,
    args: &[Value],
    idx: usize,
    pname: &str,
) -> R<Rc<RefCell<goro_core::array::PhpArray>>> {
    let v = args.get(idx).cloned().unwrap_or(Value::Undef).deref();
    match v {
        Value::Array(a) => Ok(a),
        other => Err(arg_err(vm, fname, idx, pname, "array", &other)),
    }
}

pub fn obj_of(v: &Value) -> Option<Rc<RefCell<PhpObject>>> {
    match v.deref() {
        Value::Object(o) => Some(o),
        _ => None,
    }
}

pub fn s(v: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(v))
}

pub fn ss(v: String) -> Value {
    Value::String(PhpString::from_string(v))
}

pub fn type_err_class(
    vm: &mut Vm,
    fname: &str,
    idx: usize,
    pname: &str,
    expected: &str,
    v: &Value,
) -> VmError {
    arg_err(vm, fname, idx, pname, expected, v)
}

trait ClassExt {
    fn find_method_in_chain_has(&self, m: &[u8]) -> bool;
}
impl ClassExt for goro_core::object::ClassEntry {
    fn find_method_in_chain_has(&self, m: &[u8]) -> bool {
        self.methods.contains_key(m)
    }
}
