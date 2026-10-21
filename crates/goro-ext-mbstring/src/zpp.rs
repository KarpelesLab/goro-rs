//! Small helpers for parsing builtin function arguments PHP-style
//! (a subset of Zend's ZEND_PARSE_PARAMETERS).

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

pub fn throw(vm: &mut Vm, class: &[u8], msg: String) -> VmError {
    let exc = vm.create_exception(class, &msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError { message: msg, line: vm.current_line }
}

pub fn value_error(vm: &mut Vm, msg: String) -> VmError {
    throw(vm, b"ValueError", msg)
}

pub fn type_error(vm: &mut Vm, msg: String) -> VmError {
    throw(vm, b"TypeError", msg)
}

/// `zend_argument_value_error(arg_num, ...)`
pub fn arg_value_error(vm: &mut Vm, func: &str, arg_num: usize, pname: &str, msg: &str) -> VmError {
    value_error(vm, format!("{}(): Argument #{} (${}) {}", func, arg_num, pname, msg))
}

pub fn type_name(v: &Value) -> String {
    Vm::value_type_name(v)
}

fn is_strict(_vm: &Vm) -> bool {
    false
}

/// Z_PARAM_STR: returns the bytes of argument `idx` (0-based), converting
/// scalars to strings; arrays and non-stringable objects raise a TypeError.
pub fn str_arg(vm: &mut Vm, func: &str, args: &[Value], idx: usize, pname: &str) -> Result<Vec<u8>, VmError> {
    let v = args.get(idx).map(|v| v.deref()).unwrap_or(Value::Null);
    to_str(vm, func, &v, idx, pname, "string")
}

fn to_str(vm: &mut Vm, func: &str, v: &Value, idx: usize, pname: &str, tname: &str) -> Result<Vec<u8>, VmError> {
    let _ = is_strict(vm);
    match v {
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        Value::Array(_) => Err(type_error(
            vm,
            format!("{}(): Argument #{} (${}) must be of type {}, array given", func, idx + 1, pname, tname),
        )),
        Value::Object(_) => {
            let has_tostring = {
                if let Value::Object(o) = v {
                    let cls: Vec<u8> = o.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                    vm.classes.get(&cls).map(|c| c.methods.contains_key(&b"__tostring".to_vec())).unwrap_or(false)
                } else {
                    false
                }
            };
            if has_tostring {
                Ok(vm.value_to_string(v).as_bytes().to_vec())
            } else {
                Err(type_error(
                    vm,
                    format!("{}(): Argument #{} (${}) must be of type {}, {} given", func, idx + 1, pname, tname, type_name(v)),
                ))
            }
        }
        _ => Ok(v.to_php_string().as_bytes().to_vec()),
    }
}

/// Z_PARAM_STR_OR_NULL (missing or null => None)
pub fn str_or_null_arg(vm: &mut Vm, func: &str, args: &[Value], idx: usize, pname: &str) -> Result<Option<Vec<u8>>, VmError> {
    match args.get(idx).map(|v| v.deref()) {
        None | Some(Value::Null) | Some(Value::Undef) => Ok(None),
        Some(v) => to_str(vm, func, &v, idx, pname, "?string").map(Some),
    }
}

/// Z_PARAM_LONG
pub fn long_arg(vm: &mut Vm, func: &str, args: &[Value], idx: usize, pname: &str) -> Result<i64, VmError> {
    let v = args.get(idx).map(|v| v.deref()).unwrap_or(Value::Null);
    to_long(vm, func, &v, idx, pname, "int")
}

fn to_long(vm: &mut Vm, func: &str, v: &Value, idx: usize, pname: &str, tname: &str) -> Result<i64, VmError> {
    match v {
        Value::Long(n) => Ok(*n),
        Value::Double(f) => {
            if !f.is_finite() || f.fract() != 0.0 || *f >= 9.2233720368547758e18 || *f < -9.2233720368547758e18 {
                if f.is_finite() && f.fract() != 0.0 && *f < 9.2233720368547758e18 && *f >= -9.2233720368547758e18 {
                    // Fractional part: deprecated implicit conversion (handled by VM) — truncate
                    return Ok(*f as i64);
                }
                return Err(type_error(
                    vm,
                    format!("{}(): Argument #{} (${}) must be of type {}, float given", func, idx + 1, pname, tname),
                ));
            }
            Ok(*f as i64)
        }
        Value::True => Ok(1),
        Value::False | Value::Null | Value::Undef => Ok(0),
        Value::String(s) => {
            let b = s.as_bytes();
            match parse_numeric_str(b) {
                Some(Num::Int(n)) => Ok(n),
                Some(Num::Float(f)) => {
                    if f.is_finite() && f < 9.2233720368547758e18 && f >= -9.2233720368547758e18 {
                        Ok(f as i64)
                    } else {
                        Err(type_error(
                            vm,
                            format!("{}(): Argument #{} (${}) must be of type {}, string given", func, idx + 1, pname, tname),
                        ))
                    }
                }
                None => Err(type_error(
                    vm,
                    format!("{}(): Argument #{} (${}) must be of type {}, string given", func, idx + 1, pname, tname),
                )),
            }
        }
        other => Err(type_error(
            vm,
            format!("{}(): Argument #{} (${}) must be of type {}, {} given", func, idx + 1, pname, tname, type_name(other)),
        )),
    }
}

/// Z_PARAM_LONG_OR_NULL
pub fn long_or_null_arg(vm: &mut Vm, func: &str, args: &[Value], idx: usize, pname: &str) -> Result<Option<i64>, VmError> {
    match args.get(idx).map(|v| v.deref()) {
        None | Some(Value::Null) | Some(Value::Undef) => Ok(None),
        Some(v) => to_long(vm, func, &v, idx, pname, "?int").map(Some),
    }
}

/// Z_PARAM_BOOL
pub fn bool_arg(args: &[Value], idx: usize) -> bool {
    args.get(idx).map(|v| v.deref().is_truthy()).unwrap_or(false)
}

enum Num {
    Int(i64),
    Float(f64),
}

fn parse_numeric_str(b: &[u8]) -> Option<Num> {
    let s = std::str::from_utf8(b).ok()?;
    let t = s.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let t = t.trim_end_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    if t.is_empty() {
        return None;
    }
    if let Ok(n) = t.parse::<i64>() {
        return Some(Num::Int(n));
    }
    let valid = {
        let bytes = t.as_bytes();
        let mut i = 0;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        let mut digits = 0;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
        if i < bytes.len() && bytes[i] == b'.' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
                digits += 1;
            }
        }
        if digits > 0 && i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
            let save = i;
            i += 1;
            if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
                i += 1;
            }
            let mut ed = 0;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
                ed += 1;
            }
            if ed == 0 {
                i = save;
            }
        }
        digits > 0 && i == bytes.len()
    };
    if valid {
        t.parse::<f64>().ok().map(Num::Float)
    } else {
        None
    }
}

pub fn pstr(b: &[u8]) -> Value {
    Value::String(PhpString::from_vec(b.to_vec()))
}

pub fn pstring(b: Vec<u8>) -> Value {
    Value::String(PhpString::from_vec(b))
}

pub fn vbool(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}
