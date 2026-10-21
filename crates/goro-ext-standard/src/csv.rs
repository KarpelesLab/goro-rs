//! PHP_FUNCTION(fgetcsv), PHP_FUNCTION(fputcsv) (ext/standard/file.c) and
//! PHP_FUNCTION(str_getcsv) (ext/standard/string.c), built on the shared
//! `goro_core::csv` port of `php_fgetcsv` / `php_fputcsv`.

use crate::scanf::zpp_string;
use goro_core::array::PhpArray;
use goro_core::csv::{php_fgetcsv, php_fputcsv_line, CsvEscape};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

pub fn register(vm: &mut Vm) {
    vm.register_function_with_params(b"fputcsv", fputcsv_fn, &[b"stream", b"fields", b"separator", b"enclosure", b"escape", b"eol"]);
    vm.register_function_with_params(b"fgetcsv", fgetcsv_fn, &[b"stream", b"length", b"separator", b"enclosure", b"escape"]);
    vm.register_function_with_params(b"str_getcsv", str_getcsv_fn, &[b"string", b"separator", b"enclosure", b"escape"]);
}

fn value_error(vm: &mut Vm, msg: String) -> VmError {
    let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError { message: msg, line: vm.current_line }
}

fn type_error(vm: &mut Vm, msg: String) -> VmError {
    let exc = vm.throw_type_error(msg.clone());
    vm.current_exception = Some(exc);
    VmError { message: msg, line: vm.current_line }
}

/// Optional `Z_PARAM_STRING`; a missing argument (or a null left by a
/// named-argument gap) counts as "not passed".
fn opt_string(vm: &mut Vm, func: &str, argnum: usize, pname: &str, v: Option<&Value>) -> Result<Option<PhpString>, VmError> {
    match v {
        None => Ok(None),
        Some(v) if matches!(v.deref(), Value::Null | Value::Undef) => Ok(None),
        Some(v) => zpp_string(vm, func, argnum, pname, v).map(Some),
    }
}

/// `PHP_Z_PARAM_STREAM`: returns the handle value (dereferenced).
fn zpp_stream(vm: &mut Vm, func: &str, v: Option<&Value>) -> Result<Value, VmError> {
    let h = v.map(|v| v.deref()).unwrap_or(Value::Null);
    if !matches!(h, Value::Resource(..) | Value::Long(_)) {
        return Err(type_error(
            vm,
            format!("{}(): Argument #1 ($stream) must be of type resource, {} given", func, Vm::value_type_name(&h)),
        ));
    }
    if !crate::misc::has_file_handle(h.to_long()) {
        return Err(type_error(vm, format!("{}(): supplied resource is not a valid stream resource", func)));
    }
    Ok(h)
}

/// Validate a single-character delimiter/enclosure argument.
fn single_char(vm: &mut Vm, func: &str, argnum: usize, pname: &str, s: Option<PhpString>, default: u8) -> Result<u8, VmError> {
    match s {
        None => Ok(default),
        Some(s) => {
            if s.len() != 1 {
                return Err(value_error(
                    vm,
                    format!("{}(): Argument #{} (${}) must be a single character", func, argnum, pname),
                ));
            }
            Ok(s.as_bytes()[0])
        }
    }
}

/// Port of `php_csv_handle_escape_argument`.
pub(crate) fn csv_handle_escape_argument(vm: &mut Vm, func: &str, escape: Option<PhpString>, argnum: usize) -> Result<CsvEscape, VmError> {
    match escape {
        Some(s) => {
            if s.len() > 1 {
                return Err(value_error(
                    vm,
                    format!("{}(): Argument #{} ($escape) must be empty or a single character", func, argnum),
                ));
            }
            Ok(s.as_bytes().first().copied())
        }
        None => {
            let line = vm.current_line;
            vm.emit_deprecated_at(
                &format!("{}(): the $escape parameter must be provided as its default value will change", func),
                line,
            );
            if vm.current_exception.is_some() {
                return Err(VmError { message: String::new(), line });
            }
            Ok(Some(b'\\'))
        }
    }
}

fn fields_to_array(values: Option<Vec<Vec<u8>>>) -> Value {
    let mut arr = PhpArray::new();
    match values {
        Some(vs) => {
            for v in vs {
                arr.push(Value::String(PhpString::from_vec(v)));
            }
        }
        // php_bc_fgetcsv_empty_line()
        None => arr.push(Value::Null),
    }
    Value::Array(Rc::new(RefCell::new(arr)))
}

/// Run `php_fgetcsv` over a stream: `first` is the initial line and more
/// lines are pulled with `php_stream_get_line(stream, NULL, 0)`.
fn fgetcsv_stream(vm: &mut Vm, func: &str, handle: &Value, first: Vec<u8>, delimiter: u8, enclosure: u8, escape: CsvEscape) -> Value {
    let func = func.to_string();
    let handle = handle.clone();
    let mut next = || crate::misc::stream_get_line(vm, &func, &handle, None);
    let values = php_fgetcsv(first, delimiter, enclosure, escape, Some(&mut next));
    fields_to_array(values)
}

/// PHP_FUNCTION(fgetcsv)
pub fn fgetcsv_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "fgetcsv";
    if args.is_empty() {
        let msg = format!("{}() expects at least 1 argument, 0 given", F);
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let handle = zpp_stream(vm, F, args.first())?;
    // Z_PARAM_LONG_OR_NULL(len)
    let len: Option<i64> = match args.get(1).map(|v| v.deref()) {
        None | Some(Value::Null) | Some(Value::Undef) => None,
        Some(Value::Array(_)) => {
            return Err(type_error(vm, format!("{}(): Argument #2 ($length) must be of type ?int, array given", F)));
        }
        Some(v) => Some(v.to_long()),
    };
    let delim_s = opt_string(vm, F, 3, "separator", args.get(2))?;
    let encl_s = opt_string(vm, F, 4, "enclosure", args.get(3))?;
    let esc_s = opt_string(vm, F, 5, "escape", args.get(4))?;

    let delimiter = single_char(vm, F, 3, "separator", delim_s, b',')?;
    let enclosure = single_char(vm, F, 4, "enclosure", encl_s, b'"')?;
    let escape = csv_handle_escape_argument(vm, F, esc_s, 5)?;

    let len: i64 = match len {
        None | Some(0) => -1,
        Some(l) if !(0..=i64::MAX - 1).contains(&l) => {
            return Err(value_error(vm, format!("{}(): Argument #2 ($length) must be between 0 and {}", F, i64::MAX - 1)));
        }
        Some(l) => l,
    };

    let buf = if len < 0 {
        crate::misc::stream_get_line(vm, F, &handle, None)
    } else {
        // emalloc(len + 1): bail out like the Zend allocator would.
        let alloc = (len as u64 + 1).div_ceil(4096).saturating_mul(4096);
        let mem_limit = goro_core::value::memory_get_limit();
        if mem_limit > 0 && (goro_core::value::memory_get_usage() as u64).saturating_add(alloc) > mem_limit as u64 {
            let msg = format!("Allowed memory size of {} bytes exhausted (tried to allocate {} bytes)", mem_limit, alloc);
            let line = vm.current_line;
            return Err(vm.emit_fatal_at(&msg, line));
        }
        crate::misc::stream_get_line(vm, F, &handle, Some(len as usize))
    };
    let buf = match buf {
        Some(b) => b,
        None => return Ok(Value::False),
    };
    Ok(fgetcsv_stream(vm, F, &handle, buf, delimiter, enclosure, escape))
}

/// PHP_FUNCTION(str_getcsv)
pub fn str_getcsv_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "str_getcsv";
    if args.is_empty() {
        let msg = format!("{}() expects at least 1 argument, 0 given", F);
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let s = zpp_string(vm, F, 1, "string", &args[0])?;
    let delim_s = opt_string(vm, F, 2, "separator", args.get(1))?;
    let encl_s = opt_string(vm, F, 3, "enclosure", args.get(2))?;
    let esc_s = opt_string(vm, F, 4, "escape", args.get(3))?;
    let delimiter = single_char(vm, F, 2, "separator", delim_s, b',')?;
    let enclosure = single_char(vm, F, 3, "enclosure", encl_s, b'"')?;
    let escape = csv_handle_escape_argument(vm, F, esc_s, 4)?;
    let values = php_fgetcsv(s.as_bytes().to_vec(), delimiter, enclosure, escape, None);
    Ok(fields_to_array(values))
}

/// `zval_get_tmp_string` for each field of a `fputcsv()` row.
pub(crate) fn csv_fields_to_strings(vm: &mut Vm, fields: &Rc<RefCell<PhpArray>>) -> Result<Vec<Vec<u8>>, VmError> {
    let vals: Vec<Value> = fields.borrow().iter().map(|(_, v)| v.deref()).collect();
    let mut out = Vec::with_capacity(vals.len());
    for v in vals {
        let s = match &v {
            Value::Array(_) => {
                let line = vm.current_line;
                vm.emit_warning_at("Array to string conversion", line);
                b"Array".to_vec()
            }
            Value::Object(_) => vm.value_to_string_checked(&v)?.as_bytes().to_vec(),
            _ => vm.value_to_string(&v).as_bytes().to_vec(),
        };
        out.push(s);
    }
    Ok(out)
}

/// PHP_FUNCTION(fputcsv)
pub fn fputcsv_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "fputcsv";
    if args.len() < 2 {
        let msg = format!("{}() expects at least 2 arguments, {} given", F, args.len());
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let handle = zpp_stream(vm, F, args.first())?;
    let fields = match args[1].deref() {
        Value::Array(a) => a,
        other => {
            return Err(type_error(
                vm,
                format!("{}(): Argument #2 ($fields) must be of type array, {} given", F, Vm::value_type_name(&other)),
            ));
        }
    };
    let delim_s = opt_string(vm, F, 3, "separator", args.get(2))?;
    let encl_s = opt_string(vm, F, 4, "enclosure", args.get(3))?;
    let esc_s = opt_string(vm, F, 5, "escape", args.get(4))?;
    let eol_s = opt_string(vm, F, 6, "eol", args.get(5))?;
    let delimiter = single_char(vm, F, 3, "separator", delim_s, b',')?;
    let enclosure = single_char(vm, F, 4, "enclosure", encl_s, b'"')?;
    let escape = csv_handle_escape_argument(vm, F, esc_s, 5)?;

    let strs = csv_fields_to_strings(vm, &fields)?;
    let line = php_fputcsv_line(&strs, delimiter, enclosure, escape, eol_s.as_ref().map(|s| s.as_bytes()));
    match crate::misc::stream_write(vm, F, &handle, &line) {
        Some(n) if n >= 0 => Ok(Value::Long(n)),
        _ => Ok(Value::False),
    }
}
