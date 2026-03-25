use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

pub fn register(vm: &mut Vm) {
    vm.register_function(b"abs", abs);
    vm.register_function(b"ceil", ceil);
    vm.register_function(b"floor", floor);
    vm.register_function(b"round", round);
    vm.register_function(b"max", max);
    vm.register_function(b"min", min);
    vm.register_function(b"sqrt", sqrt);
    vm.register_function(b"pow", pow);
    vm.register_function(b"intdiv", intdiv);
    vm.register_function(b"fmod", fmod_fn);
    vm.register_function(b"rand", rand_fn);
    vm.register_function(b"mt_rand", mt_rand);
    vm.register_function(b"array_sum", array_sum);
    vm.register_function(b"php_uname", php_uname);
    vm.register_function(b"phpversion", phpversion);
    vm.register_function(b"extension_loaded", extension_loaded_fn);
    vm.register_function(b"version_compare", version_compare_fn);
    vm.register_function(b"php_sapi_name", php_sapi_name);
    vm.register_function(b"defined", defined);
    vm.register_function(b"function_exists", function_exists);
    vm.register_function(b"sin", sin_fn);
    vm.register_function(b"cos", cos_fn);
    vm.register_function(b"tan", tan_fn);
    vm.register_function(b"asin", asin_fn);
    vm.register_function(b"acos", acos_fn);
    vm.register_function(b"atan", atan_fn);
    vm.register_function(b"atan2", atan2_fn);
    vm.register_function(b"log", log_fn);
    vm.register_function(b"log10", log10_fn);
    vm.register_function(b"log2", log2_fn);
    vm.register_function(b"exp", exp_fn);
    vm.register_function(b"pi", pi_fn);
    vm.register_function(b"hypot", hypot);
    vm.register_function(b"deg2rad", deg2rad_fn);
    vm.register_function(b"rad2deg", rad2deg_fn);
    vm.register_function(b"base_convert", base_convert_fn);
    vm.register_function(b"bindec", bindec_fn);
    vm.register_function(b"octdec", octdec_fn);
    vm.register_function(b"hexdec", hexdec_fn);
    vm.register_function(b"decbin", decbin_fn);
    vm.register_function(b"decoct", decoct_fn);
    vm.register_function(b"dechex", dechex_fn);
    vm.register_function(b"is_nan", is_nan_fn);
    vm.register_function(b"is_infinite", is_infinite_fn);
    vm.register_function(b"is_finite", is_finite_fn);
    vm.register_function(b"array_product", array_product);
    vm.register_function(b"random_int", random_int);
    vm.register_function(b"random_bytes", random_bytes_fn);
    vm.register_function(b"sinh", sinh_fn);
    vm.register_function(b"cosh", cosh_fn);
    vm.register_function(b"tanh", tanh_fn);
    vm.register_function(b"asinh", asinh_fn);
    vm.register_function(b"acosh", acosh_fn);
    vm.register_function(b"atanh", atanh_fn);
    vm.register_function(b"fdiv", fdiv_fn);
    vm.register_function(b"array_sum", array_sum);
    vm.register_function(b"log1p", log1p_fn);
    vm.register_function(b"expm1", expm1_fn);
    vm.register_function(b"getrandmax", getrandmax_fn);
    vm.register_function(b"mt_getrandmax", mt_getrandmax_fn);
    vm.register_function(b"number_format", number_format_math);
    vm.register_function(b"intval", intval_fn);
    vm.register_function(b"floatval", floatval_fn);
    vm.register_function(b"doubleval", floatval_fn);
    vm.register_function(b"fpow", fpow_fn);
    vm.register_function(b"srand", srand_fn);
    vm.register_function(b"mt_srand", srand_fn);
}

fn abs(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    Ok(match val {
        Value::Long(n) => {
            // i64::MIN.abs() would overflow, return as float in that case
            match n.checked_abs() {
                Some(abs_val) => Value::Long(abs_val),
                None => Value::Double((*n as f64).abs()),
            }
        }
        Value::Double(f) => Value::Double(f.abs()),
        Value::String(s) => {
            // Check if string contains a float (has '.', 'e', 'E')
            let bytes = s.as_bytes();
            if bytes.iter().any(|&b| b == b'.' || b == b'e' || b == b'E') {
                Value::Double(val.to_double().abs())
            } else {
                let n = val.to_long();
                match n.checked_abs() {
                    Some(abs_val) => Value::Long(abs_val),
                    None => Value::Double((n as f64).abs()),
                }
            }
        }
        _ => {
            let n = val.to_long();
            match n.checked_abs() {
                Some(abs_val) => Value::Long(abs_val),
                None => Value::Double((n as f64).abs()),
            }
        }
    })
}

fn ceil(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = args.first().unwrap_or(&Value::Null).to_double();
    Ok(Value::Double(f.ceil()))
}

fn floor(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = args.first().unwrap_or(&Value::Null).to_double();
    Ok(Value::Double(f.floor()))
}

fn round(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = args.first().unwrap_or(&Value::Null).to_double();
    let precision = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let factor = 10f64.powi(precision as i32);
    Ok(Value::Double((f * factor).round() / factor))
}

fn max(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Err(VmError {
            message: "max() expects at least 1 argument".into(),
            line: 0,
        });
    }

    if args.len() == 1 {
        // Single argument must be an array
        if let Value::Array(arr) = &args[0] {
            let arr = arr.borrow();
            if arr.len() == 0 {
                // Empty array - throw ValueError
                let msg = "max(): Argument #1 ($value) must contain at least one element".to_string();
                vm.emit_warning_at(&msg, vm.current_line);
                return Err(VmError { message: msg, line: vm.current_line });
            }
            let mut max_val = Value::Null;
            let mut first = true;
            for (_, v) in arr.iter() {
                if first || v.compare(&max_val) > 0 {
                    max_val = v.clone();
                    first = false;
                }
            }
            return Ok(max_val);
        } else {
            // Single non-array - throw TypeError
            let msg = "max(): Argument #1 ($value) must be of type array, int given".to_string();
            vm.emit_warning_at(&msg, vm.current_line);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }

    let mut max_val = args[0].clone();
    for arg in &args[1..] {
        if arg.compare(&max_val) > 0 {
            max_val = arg.clone();
        }
    }
    Ok(max_val)
}

fn min(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Err(VmError {
            message: "min() expects at least 1 argument".into(),
            line: 0,
        });
    }

    if args.len() == 1 {
        if let Value::Array(arr) = &args[0] {
            let arr = arr.borrow();
            if arr.len() == 0 {
                let msg = "min(): Argument #1 ($value) must contain at least one element".to_string();
                vm.emit_warning_at(&msg, vm.current_line);
                return Err(VmError { message: msg, line: vm.current_line });
            }
            let mut min_val = Value::Null;
            let mut first = true;
            for (_, v) in arr.iter() {
                if first || v.compare(&min_val) < 0 {
                    min_val = v.clone();
                    first = false;
                }
            }
            return Ok(min_val);
        } else {
            let msg = "min(): Argument #1 ($value) must be of type array, int given".to_string();
            vm.emit_warning_at(&msg, vm.current_line);
            return Err(VmError { message: msg, line: vm.current_line });
        }
    }

    let mut min_val = args[0].clone();
    for arg in &args[1..] {
        if arg.compare(&min_val) < 0 {
            min_val = arg.clone();
        }
    }
    Ok(min_val)
}

fn sqrt(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = args.first().unwrap_or(&Value::Null).to_double();
    Ok(Value::Double(f.sqrt()))
}

fn pow(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let base = args.first().unwrap_or(&Value::Null);
    let exp = args.get(1).unwrap_or(&Value::Null);
    Ok(base.pow(exp))
}

fn intdiv(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = args.first().unwrap_or(&Value::Null).to_long();
    let b = args.get(1).unwrap_or(&Value::Null).to_long();
    if b == 0 {
        let msg = "Division by zero";
        let exc = vm.create_exception(b"DivisionByZeroError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.into(), line: vm.current_line });
    }
    if a == i64::MIN && b == -1 {
        let msg = "Division of PHP_INT_MIN by -1 is not an integer";
        let exc = vm.create_exception(b"ArithmeticError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.into(), line: vm.current_line });
    }
    Ok(Value::Long(a / b))
}

fn fmod_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = args.first().unwrap_or(&Value::Null).to_double();
    let b = args.get(1).unwrap_or(&Value::Null).to_double();
    Ok(Value::Double(a % b))
}

fn rand_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let min = args.first().map(|v| v.to_long()).unwrap_or(0);
    let max = args.get(1).map(|v| v.to_long()).unwrap_or(i32::MAX as i64);
    // Simple pseudo-random (not cryptographic)
    let val = simple_random(min, max);
    Ok(Value::Long(val))
}

fn mt_rand(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let min = args.first().map(|v| v.to_long()).unwrap_or(0);
    let max = args.get(1).map(|v| v.to_long()).unwrap_or(i32::MAX as i64);
    let val = simple_random(min, max);
    Ok(Value::Long(val))
}

fn simple_random(min: i64, max: i64) -> i64 {
    use std::time::SystemTime;
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let range = (max - min + 1) as u64;
    if range == 0 {
        return min;
    }
    min + (seed % range) as i64
}

fn array_sum(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    if let Value::Array(arr) = val {
        let arr = arr.borrow();
        let mut sum = Value::Long(0);
        for (_, v) in arr.iter() {
            sum = sum.add(v);
        }
        Ok(sum)
    } else {
        Ok(Value::Long(0))
    }
}

fn php_uname(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(goro_core::string::PhpString::from_bytes(
        b"goro-rs 0.1.0",
    )))
}

fn phpversion(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(goro_core::string::PhpString::from_bytes(
        b"8.5.4",
    )))
}

fn php_sapi_name(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(goro_core::string::PhpString::from_bytes(
        b"cli",
    )))
}

fn defined(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(if vm.constants.contains_key(name.as_bytes()) {
        Value::True
    } else {
        Value::False
    })
}

fn function_exists(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let name_lower: Vec<u8> = name
        .as_bytes()
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    // Check if the function is registered (we need access to the function table)
    // For now, return false for unknown functions
    // The VM stores functions in its HashMap, but we can't access it from here directly
    // through the builtin fn signature. This will be fixed when we refactor.
    let _ = name_lower;
    let _ = vm;
    Ok(Value::False)
}

fn sin_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double().sin(),
    ))
}
fn cos_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double().cos(),
    ))
}
fn tan_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double().tan(),
    ))
}
fn asin_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double().asin(),
    ))
}
fn acos_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double().acos(),
    ))
}
fn atan_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double().atan(),
    ))
}
fn atan2_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let y = args.first().unwrap_or(&Value::Null).to_double();
    let x = args.get(1).unwrap_or(&Value::Null).to_double();
    Ok(Value::Double(y.atan2(x)))
}
fn log_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null).to_double();
    let base = args.get(1).map(|v| v.to_double());
    match base {
        Some(b) if b <= 0.0 => {
            let msg = "log(): Argument #2 ($base) must be greater than 0";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            Err(VmError { message: msg.into(), line: vm.current_line })
        }
        Some(b) if b == 1.0 => {
            // Division by zero in log would return NaN/Inf, but PHP throws ValueError
            vm.emit_warning("log(): Base must not be 1");
            Ok(Value::Double(f64::NAN))
        }
        Some(b) => Ok(Value::Double(val.log(b))),
        None => Ok(Value::Double(val.ln())),
    }
}
fn log10_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double().log10(),
    ))
}
fn log2_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double().log2(),
    ))
}
fn exp_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double().exp(),
    ))
}
fn pi_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(std::f64::consts::PI))
}
fn hypot(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let x = args.first().unwrap_or(&Value::Null).to_double();
    let y = args.get(1).unwrap_or(&Value::Null).to_double();
    Ok(Value::Double(x.hypot(y)))
}
fn deg2rad_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first()
            .unwrap_or(&Value::Null)
            .to_double()
            .to_radians(),
    ))
}
fn rad2deg_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first()
            .unwrap_or(&Value::Null)
            .to_double()
            .to_degrees(),
    ))
}
fn base_convert_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let num_str = args.first().unwrap_or(&Value::Null).to_php_string();
    let from_base = args.get(1).map(|v| v.to_long()).unwrap_or(10) as u32;
    let to_base = args.get(2).map(|v| v.to_long()).unwrap_or(10) as u32;
    if from_base < 2 || from_base > 36 {
        let msg = "base_convert(): Argument #2 ($from_base) must be between 2 and 36 (inclusive)".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    if to_base < 2 || to_base > 36 {
        let msg = "base_convert(): Argument #3 ($to_base) must be between 2 and 36 (inclusive)".to_string();
        let exc = vm.throw_type_error(msg.clone());
        if let Value::Object(obj) = &exc {
            obj.borrow_mut().class_name = b"ValueError".to_vec();
        }
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }
    let input = num_str.to_string_lossy().to_lowercase();
    // Strip known prefixes silently (0b, 0o, 0x, 0B, 0O, 0X)
    let input = if input.starts_with("0b") && from_base == 2 {
        input[2..].to_string()
    } else if input.starts_with("0o") && from_base == 8 {
        input[2..].to_string()
    } else if input.starts_with("0x") && from_base == 16 {
        input[2..].to_string()
    } else {
        input
    };
    // Strip invalid characters for the given base, emit deprecation if any were removed
    let mut cleaned = String::new();
    let mut had_invalid = false;
    for c in input.chars() {
        let digit_val = match c {
            '0'..='9' => c as u32 - '0' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 10,
            _ => from_base, // invalid
        };
        if digit_val < from_base {
            cleaned.push(c);
        } else {
            had_invalid = true;
        }
    }
    if had_invalid {
        vm.emit_deprecated_at("Invalid characters passed for attempted conversion, these have been ignored", vm.current_line);
    }
    if cleaned.is_empty() {
        return Ok(Value::String(goro_core::string::PhpString::from_string("0".to_string())));
    }
    // Parse cleaned string in from_base, convert to to_base
    // Use u128 for larger range
    let mut val: u128 = 0;
    for c in cleaned.chars() {
        let digit_val = match c {
            '0'..='9' => c as u32 - '0' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 10,
            _ => 0,
        };
        val = val.wrapping_mul(from_base as u128).wrapping_add(digit_val as u128);
    }
    let result = int_to_base_u128(val, to_base);
    Ok(Value::String(goro_core::string::PhpString::from_string(
        result,
    )))
}

fn int_to_base_u128(mut val: u128, base: u32) -> String {
    if val == 0 {
        return "0".to_string();
    }
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut result = Vec::new();
    while val > 0 {
        result.push(digits[(val % base as u128) as usize]);
        val /= base as u128;
    }
    result.reverse();
    String::from_utf8(result).unwrap_or_default()
}

fn int_to_base(mut val: u64, base: u32) -> String {
    if val == 0 {
        return "0".to_string();
    }
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut result = Vec::new();
    while val > 0 {
        result.push(digits[(val % base as u64) as usize]);
        val /= base as u64;
    }
    result.reverse();
    String::from_utf8(result).unwrap_or_default()
}
fn bindec_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let input = s.to_string_lossy();
    // Strip 0b/0B prefix
    let trimmed = input.trim();
    let stripped = if trimmed.starts_with("0b") || trimmed.starts_with("0B") {
        &trimmed[2..]
    } else {
        trimmed
    };
    Ok(parse_base_string_strip(vm, stripped, 2))
}
fn octdec_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let input = s.to_string_lossy();
    // Strip 0o/0O prefix
    let trimmed = input.trim();
    let stripped = if trimmed.starts_with("0o") || trimmed.starts_with("0O") {
        &trimmed[2..]
    } else {
        trimmed
    };
    Ok(parse_base_string_strip(vm, stripped, 8))
}
fn hexdec_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = args.first().unwrap_or(&Value::Null).to_php_string();
    let input = s.to_string_lossy();
    // Strip 0x/0X prefix
    let trimmed = input.trim();
    let stripped = if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
        &trimmed[2..]
    } else {
        trimmed
    };
    Ok(parse_base_string_strip(vm, stripped, 16))
}

/// Parse a string in the given base, stripping invalid chars with deprecation warning
fn parse_base_string_strip(vm: &mut Vm, s: &str, base: u32) -> Value {
    let trimmed = s.trim();
    // Strip invalid characters
    let mut cleaned = String::new();
    let mut had_invalid = false;
    for c in trimmed.chars() {
        let digit_val = match c {
            '0'..='9' => c as u32 - '0' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 10,
            'A'..='Z' => c as u32 - 'A' as u32 + 10,
            _ => base, // invalid
        };
        if digit_val < base {
            cleaned.push(c);
        } else {
            had_invalid = true;
        }
    }
    if had_invalid {
        vm.emit_deprecated_at("Invalid characters passed for attempted conversion, these have been ignored", vm.current_line);
    }
    if cleaned.is_empty() {
        return Value::Long(0);
    }
    parse_base_string(&cleaned, base)
}

/// Parse a string in the given base, returning Long if it fits, Double otherwise
fn parse_base_string(s: &str, base: u32) -> Value {
    // Try parsing as unsigned first (PHP treats these as unsigned)
    let trimmed = s.trim();
    match u64::from_str_radix(trimmed, base) {
        Ok(n) => {
            if n <= i64::MAX as u64 {
                Value::Long(n as i64)
            } else {
                Value::Double(n as f64)
            }
        }
        Err(_) => {
            // Too large for u64, compute as float
            let mut result: f64 = 0.0;
            let base_f = base as f64;
            for c in trimmed.chars() {
                let digit = match c {
                    '0'..='9' => c as u32 - '0' as u32,
                    'a'..='f' => c as u32 - 'a' as u32 + 10,
                    'A'..='F' => c as u32 - 'A' as u32 + 10,
                    _ => break,
                };
                if digit >= base {
                    break;
                }
                result = result * base_f + digit as f64;
            }
            Value::Double(result)
        }
    }
}
fn decbin_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(goro_core::string::PhpString::from_string(
        format!("{:b}", args.first().unwrap_or(&Value::Null).to_long()),
    )))
}
fn decoct_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(goro_core::string::PhpString::from_string(
        format!("{:o}", args.first().unwrap_or(&Value::Null).to_long()),
    )))
}
fn dechex_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(goro_core::string::PhpString::from_string(
        format!("{:x}", args.first().unwrap_or(&Value::Null).to_long()),
    )))
}
fn is_nan_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(
        if args.first().unwrap_or(&Value::Null).to_double().is_nan() {
            Value::True
        } else {
            Value::False
        },
    )
}
fn is_infinite_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(
        if args
            .first()
            .unwrap_or(&Value::Null)
            .to_double()
            .is_infinite()
        {
            Value::True
        } else {
            Value::False
        },
    )
}
fn is_finite_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(
        if args.first().unwrap_or(&Value::Null).to_double().is_finite() {
            Value::True
        } else {
            Value::False
        },
    )
}
fn array_product(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        let mut product = Value::Long(1);
        for (_, v) in arr.iter() {
            product = product.mul(v);
        }
        Ok(product)
    } else {
        Ok(Value::Long(0))
    }
}
fn random_int(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let min = args.first().map(|v| v.to_long()).unwrap_or(0);
    let max = args.get(1).map(|v| v.to_long()).unwrap_or(i64::MAX);
    use std::time::SystemTime;
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let range = (max - min + 1) as u64;
    if range == 0 {
        return Ok(Value::Long(min));
    }
    Ok(Value::Long(min + (seed % range) as i64))
}
fn random_bytes_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let len = args.first().map(|v| v.to_long()).unwrap_or(16) as usize;
    let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 13) as u8).collect(); // Not cryptographic!
    Ok(Value::String(goro_core::string::PhpString::from_vec(bytes)))
}
fn sinh_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double().sinh(),
    ))
}
fn cosh_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double().cosh(),
    ))
}
fn tanh_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double().tanh(),
    ))
}
fn asinh_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double().asinh(),
    ))
}
fn acosh_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double().acosh(),
    ))
}
fn atanh_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double().atanh(),
    ))
}
fn fdiv_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = args.first().unwrap_or(&Value::Null).to_double();
    let b = args.get(1).unwrap_or(&Value::Null).to_double();
    Ok(Value::Double(a / b))
}

fn log1p_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double().ln_1p(),
    ))
}

fn expm1_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double().exp_m1(),
    ))
}

fn getrandmax_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(2147483647))
}

fn mt_getrandmax_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(2147483647))
}

fn number_format_math(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let decimals_raw = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let dec_point = match args.get(2).map(|v| v.deref()) {
        Some(Value::Null) | Some(Value::Undef) | None => ".".to_string(),
        Some(v) => v.to_php_string().to_string_lossy(),
    };
    let thousands_sep = match args.get(3).map(|v| v.deref()) {
        Some(Value::Null) | Some(Value::Undef) | None => ",".to_string(),
        Some(v) => v.to_php_string().to_string_lossy(),
    };

    let num = val.to_double();

    // Handle negative decimals: round to nearest 10^(-decimals)
    let (num_to_format, decimals) = if decimals_raw < 0 {
        let neg_dec = (-decimals_raw) as u32;
        if neg_dec >= 20 {
            (0.0f64, 0usize)
        } else {
            let factor = 10f64.powi(neg_dec as i32);
            let rounded = (num / factor).round() * factor;
            (rounded, 0usize)
        }
    } else {
        (num, decimals_raw.min(100000) as usize)
    };

    // For integer values with no decimals, format without going through float
    let formatted = if decimals_raw >= 0 && let Value::Long(n) = val {
        if decimals > 0 {
            format!("{}.{}", n, "0".repeat(decimals))
        } else {
            format!("{}", n)
        }
    } else {
        if num_to_format.is_nan() {
            if decimals > 0 {
                format!("NAN{}{}", dec_point, "0".repeat(decimals))
            } else {
                "NAN".to_string()
            }
        } else if num_to_format.is_infinite() {
            if num_to_format > 0.0 {
                if decimals > 0 {
                    format!("INF{}{}", dec_point, "0".repeat(decimals))
                } else {
                    "INF".to_string()
                }
            } else {
                if decimals > 0 {
                    format!("-INF{}{}", dec_point, "0".repeat(decimals))
                } else {
                    "-INF".to_string()
                }
            }
        } else {
            format!("{:.prec$}", num_to_format, prec = decimals)
        }
    };

    let parts: Vec<&str> = formatted.split('.').collect();
    let int_part = parts[0];
    let dec_part = parts.get(1).unwrap_or(&"");

    // Add thousands separator
    let negative = int_part.starts_with('-');
    let digits: &str = if negative { &int_part[1..] } else { int_part };

    // Don't add thousands sep to non-numeric parts (NAN, INF)
    let with_sep = if digits.chars().all(|c| c.is_ascii_digit()) {
        let mut result = String::new();
        for (i, c) in digits.chars().rev().enumerate() {
            if i > 0 && i % 3 == 0 && !thousands_sep.is_empty() {
                result = format!("{}{}{}", c, thousands_sep, result);
            } else {
                result.insert(0, c);
            }
        }
        if negative {
            result.insert(0, '-');
        }
        result
    } else {
        int_part.to_string()
    };

    let result = if decimals > 0 {
        format!("{}{}{}", with_sep, dec_point, dec_part)
    } else {
        with_sep
    };

    Ok(Value::String(goro_core::string::PhpString::from_string(
        result,
    )))
}

fn intval_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let base = args.get(1).map(|v| v.to_long()).unwrap_or(10);
    if base != 10 {
        if let Value::String(s) = val {
            let s = s.to_string_lossy();
            let s = s.trim();
            let result = i64::from_str_radix(
                s.trim_start_matches("0x").trim_start_matches("0X"),
                base as u32,
            )
            .unwrap_or(0);
            return Ok(Value::Long(result));
        }
    }
    Ok(Value::Long(val.to_long()))
}

fn extension_loaded_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use goro_core::string::PhpString;
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let name_lower = name.to_string_lossy().to_ascii_lowercase();
    // Report our built-in extensions as loaded
    let loaded = matches!(
        name_lower.as_str(),
        "standard"
            | "core"
            | "date"
            | "pcre"
            | "json"
            | "ctype"
            | "hash"
            | "spl"
            | "reflection"
            | "mbstring"
            | "tokenizer"
    );
    Ok(if loaded { Value::True } else { Value::False })
}

fn version_compare_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use goro_core::string::PhpString;
    let v1 = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();
    let v2 = args
        .get(1)
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();
    let op = args.get(2).map(|v| v.to_php_string().to_string_lossy());

    // Simple version comparison: split by dots and compare parts
    let parts1: Vec<i64> = v1.split('.').map(|s| s.parse().unwrap_or(0)).collect();
    let parts2: Vec<i64> = v2.split('.').map(|s| s.parse().unwrap_or(0)).collect();
    let max_len = parts1.len().max(parts2.len());
    let mut cmp = 0i64;
    for i in 0..max_len {
        let a = parts1.get(i).copied().unwrap_or(0);
        let b = parts2.get(i).copied().unwrap_or(0);
        if a < b {
            cmp = -1;
            break;
        }
        if a > b {
            cmp = 1;
            break;
        }
    }

    if let Some(operator) = op {
        let result = match operator.as_str() {
            "<" | "lt" => cmp < 0,
            "<=" | "le" => cmp <= 0,
            ">" | "gt" => cmp > 0,
            ">=" | "ge" => cmp >= 0,
            "==" | "eq" => cmp == 0,
            "!=" | "ne" => cmp != 0,
            _ => false,
        };
        Ok(if result { Value::True } else { Value::False })
    } else {
        Ok(Value::Long(cmp))
    }
}

fn floatval_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double(),
    ))
}

fn fpow_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let base = args.first().unwrap_or(&Value::Null).to_double();
    let exp = args.get(1).unwrap_or(&Value::Null).to_double();
    Ok(Value::Double(base.powf(exp)))
}

fn srand_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // srand/mt_srand is a no-op in our implementation since we use the system RNG
    Ok(Value::Null)
}
