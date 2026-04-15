use goro_core::opcode::ParamType;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

/// PHP-compatible rounding using floor(f + 0.5) approach.
/// This matches PHP's _php_math_round() which uses floor(f + 0.5) for positive
/// and ceil(f - 0.5) for negative, which handles edge cases like 0.045*100
/// differently from Rust's f64::round().
fn php_round_value(val: f64, places: i32) -> f64 {
    if val.is_nan() || val.is_infinite() || val == 0.0 {
        return val;
    }
    let factor = 10f64.powi(places);
    let f = val * factor;
    // Beyond our precision, rounding is pointless
    if f.abs() >= 1e15 {
        return val;
    }
    let tmp = if f >= 0.0 {
        (f + 0.5).floor()
    } else {
        (f - 0.5).ceil()
    };
    tmp / factor
}

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
    vm.register_function(b"lcg_value", lcg_value_fn);

    // Register parameter types for strict_types enforcement on builtins
    let num = || Some(ParamType::Union(vec![
        ParamType::Simple(b"int".to_vec()),
        ParamType::Simple(b"float".to_vec()),
    ]));
    let i = || Some(ParamType::Simple(b"int".to_vec()));
    let f = || Some(ParamType::Simple(b"float".to_vec()));
    let s = || Some(ParamType::Simple(b"string".to_vec()));
    vm.register_builtin_param_types(b"abs", vec![num()]);
    vm.register_builtin_param_types(b"ceil", vec![num()]);
    vm.register_builtin_param_types(b"floor", vec![num()]);
    vm.register_builtin_param_types(b"round", vec![num(), i(), i()]);
    vm.register_builtin_param_types(b"sqrt", vec![f()]);
    vm.register_builtin_param_types(b"intdiv", vec![i(), i()]);
    vm.register_builtin_param_types(b"fmod", vec![f(), f()]);
    vm.register_builtin_param_types(b"rand", vec![i(), i()]);
    vm.register_builtin_param_types(b"mt_rand", vec![i(), i()]);
    vm.register_builtin_param_types(b"random_int", vec![i(), i()]);
    vm.register_builtin_param_types(b"sin", vec![f()]);
    vm.register_builtin_param_types(b"cos", vec![f()]);
    vm.register_builtin_param_types(b"tan", vec![f()]);
    vm.register_builtin_param_types(b"asin", vec![f()]);
    vm.register_builtin_param_types(b"acos", vec![f()]);
    vm.register_builtin_param_types(b"atan", vec![f()]);
    vm.register_builtin_param_types(b"atan2", vec![f(), f()]);
    vm.register_builtin_param_types(b"log", vec![f(), f()]);
    vm.register_builtin_param_types(b"log10", vec![f()]);
    vm.register_builtin_param_types(b"log2", vec![f()]);
    vm.register_builtin_param_types(b"exp", vec![f()]);
    vm.register_builtin_param_types(b"hypot", vec![f(), f()]);
    vm.register_builtin_param_types(b"deg2rad", vec![f()]);
    vm.register_builtin_param_types(b"rad2deg", vec![f()]);
    vm.register_builtin_param_types(b"base_convert", vec![s(), i(), i()]);
    vm.register_builtin_param_types(b"bindec", vec![s()]);
    vm.register_builtin_param_types(b"octdec", vec![s()]);
    vm.register_builtin_param_types(b"hexdec", vec![s()]);
    vm.register_builtin_param_types(b"decbin", vec![i()]);
    vm.register_builtin_param_types(b"decoct", vec![i()]);
    vm.register_builtin_param_types(b"dechex", vec![i()]);
    vm.register_builtin_param_types(b"is_nan", vec![f()]);
    vm.register_builtin_param_types(b"is_infinite", vec![f()]);
    vm.register_builtin_param_types(b"is_finite", vec![f()]);
    vm.register_builtin_param_types(b"number_format", vec![f(), i(), s(), s()]);
    vm.register_builtin_param_types(b"fdiv", vec![f(), f()]);
}

/// Throw a TypeError for a math function argument and set the current exception
fn throw_math_type_error(vm: &mut Vm, message: String) {
    let exc = vm.throw_type_error(message);
    vm.current_exception = Some(exc);
}

/// Check math function argument for null deprecation warning or type error.
/// Returns true if processing should continue (value was acceptable), false if error was thrown.
fn check_math_num_arg(vm: &mut Vm, val: &Value, func_name: &str, param_name: &str, param_num: u32) -> Result<bool, VmError> {
    match val {
        Value::Null => {
            vm.emit_deprecated_at(
                &format!("{}(): Passing null to parameter #{} (${}) of type int|float is deprecated", func_name, param_num, param_name),
                vm.current_line,
            );
            Ok(true)
        }
        Value::True | Value::False => Ok(true),
        Value::Long(_) | Value::Double(_) => Ok(true),
        Value::String(s) => {
            // PHP 8: Only numeric strings are accepted for int|float params
            // Non-numeric strings throw TypeError
            let bytes = s.as_bytes();
            if goro_core::value::parse_numeric_string(bytes).is_some() {
                Ok(true) // numeric strings are coerced to numbers
            } else {
                // In PHP 8, typed parameters (int|float) only accept fully numeric strings
                // Non-numeric and leading-numeric strings throw TypeError
                let type_name = Vm::value_type_name(val);
                throw_math_type_error(vm, format!("{}(): Argument #{} (${}) must be of type int|float, {} given", func_name, param_num, param_name, type_name));
                Ok(false)
            }
        }
        Value::Reference(r) => {
            let inner = r.borrow().clone();
            check_math_num_arg(vm, &inner, func_name, param_name, param_num)
        }
        _ => {
            let type_name = Vm::value_type_name(val);
            throw_math_type_error(vm, format!("{}(): Argument #{} (${}) must be of type int|float, {} given", func_name, param_num, param_name, type_name));
            Ok(false)
        }
    }
}

/// Check math function argument for int type only.
/// In PHP 8, floats that fit in int are silently coerced. Floats with fractional parts
/// emit a deprecation. Very large floats that don't fit throw TypeError.
fn check_int_arg(vm: &mut Vm, val: &Value, func_name: &str, param_name: &str, param_num: u32) -> Result<bool, VmError> {
    match val {
        Value::Null => {
            // null is accepted with deprecation in PHP 8.1+
            vm.emit_deprecated_at(
                &format!("{}(): Passing null to parameter #{} (${}) of type int is deprecated", func_name, param_num, param_name),
                vm.current_line,
            );
            Ok(true)
        }
        Value::True | Value::False => Ok(true),
        Value::Long(_) => Ok(true),
        Value::Double(f) => {
            // PHP 8: floats that fit in int range are coerced (with deprecation if fractional)
            // i64 range: -2^63 to 2^63-1
            // i64::MIN as f64 is exact (-9223372036854775808.0)
            // i64::MAX as f64 rounds up to 9223372036854775808.0 (= 2^63)
            // So we reject any float >= 2^63 or < -2^63
            let upper_bound = 9223372036854775808.0_f64; // 2^63
            let lower_bound = -9223372036854775808.0_f64; // -2^63
            let fits_in_int = !f.is_nan() && !f.is_infinite()
                && *f >= lower_bound
                && *f < upper_bound;
            if !fits_in_int {
                let type_name = Vm::value_type_name(val);
                throw_math_type_error(vm, format!("{}(): Argument #{} (${}) must be of type int, {} given", func_name, param_num, param_name, type_name));
                Ok(false)
            } else {
                if f.fract() != 0.0 {
                    vm.emit_deprecated_at(
                        &format!("Implicit conversion from float {} to int loses precision", goro_core::value::format_php_float(*f)),
                        vm.current_line,
                    );
                }
                Ok(true) // coerced to int via to_long()
            }
        }
        Value::String(s) => {
            // Numeric strings accepted, non-numeric rejected
            let bytes = s.as_bytes();
            if let Some(n) = goro_core::value::parse_numeric_string(bytes) {
                if n.fract() != 0.0 {
                    // Float string with fractional part - deprecated
                    let str_val = s.to_string_lossy();
                    vm.emit_deprecated_at(
                        &format!("Implicit conversion from float-string \"{}\" to int loses precision", str_val),
                        vm.current_line,
                    );
                }
                Ok(true)
            } else {
                let type_name = Vm::value_type_name(val);
                throw_math_type_error(vm, format!("{}(): Argument #{} (${}) must be of type int, {} given", func_name, param_num, param_name, type_name));
                Ok(false)
            }
        }
        Value::Reference(r) => {
            let inner = r.borrow().clone();
            check_int_arg(vm, &inner, func_name, param_name, param_num)
        }
        _ => {
            let type_name = Vm::value_type_name(val);
            throw_math_type_error(vm, format!("{}(): Argument #{} (${}) must be of type int, {} given", func_name, param_num, param_name, type_name));
            Ok(false)
        }
    }
}

fn abs(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    if !check_math_num_arg(vm, val, "abs", "num", 1)? {
        return Ok(Value::Null);
    }
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

fn ceil(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    if !check_math_num_arg(vm, val, "ceil", "num", 1)? {
        return Ok(Value::Null);
    }
    let f = val.to_double();
    Ok(Value::Double(f.ceil()))
}

fn floor(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    if !check_math_num_arg(vm, val, "floor", "num", 1)? {
        return Ok(Value::Null);
    }
    let f = val.to_double();
    Ok(Value::Double(f.floor()))
}

fn round(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    if !check_math_num_arg(vm, val, "round", "num", 1)? {
        return Ok(Value::Null);
    }
    let f = val.to_double();
    let precision = if let Some(prec_val) = args.get(1) {
        if !check_int_arg(vm, prec_val, "round", "precision", 2)? {
            return Ok(Value::Null);
        }
        prec_val.to_long_coerced()
    } else {
        0
    };

    // Handle edge cases
    if f.is_nan() || f.is_infinite() || f == 0.0 { return Ok(Value::Double(f)); }
    // Clamp precision to avoid overflow in 10^precision
    if precision > 292 { return Ok(Value::Double(f)); }
    if precision < -292 { return Ok(Value::Double(0.0)); }
    let precision_i32 = precision as i32;

    // mode: 0=HALF_UP(default), 1=HALF_DOWN, 2=HALF_EVEN, 3=HALF_ODD
    // Also support RoundingMode enum backed values
    let mode = if let Some(mode_val) = args.get(2) {
        let mode_val = mode_val.deref();
        match &mode_val {
            Value::Object(obj) => {
                // RoundingMode enum - get the backed value
                let obj_ref = obj.borrow();
                let class = String::from_utf8_lossy(&obj_ref.class_name).to_string();
                if class == "RoundingMode" {
                    let val = obj_ref.get_property(b"value");
                    val.to_long()
                } else {
                    vm.throw_type_error(format!(
                        "round(): Argument #3 ($mode) must be of type RoundingMode|int, {} given",
                        class
                    ));
                    return Ok(Value::Null);
                }
            }
            _ => mode_val.to_long(),
        }
    } else {
        0 // PHP_ROUND_HALF_UP
    };
    let factor = 10f64.powi(precision_i32);
    let scaled = f * factor;
    // Beyond float64 precision, rounding is pointless - return value as-is
    if scaled.abs() >= 1e15 {
        return Ok(Value::Double(f));
    }
    let rounded = match mode {
        0 | 1 => { // HALF_UP / HalfAwayFromZero (PHP default, PHP_ROUND_HALF_UP=1)
            // Use PHP's floor(f+0.5) approach for correct edge case handling
            if scaled >= 0.0 { (scaled + 0.5).floor() } else { (scaled - 0.5).ceil() }
        }
        2 => { // HALF_DOWN / HalfTowardsZero (PHP_ROUND_HALF_DOWN=2)
            let frac = scaled.fract().abs();
            if (frac - 0.5).abs() < 1e-9 {
                scaled.trunc()
            } else {
                scaled.round()
            }
        }
        3 => { // HALF_EVEN / HalfEven (PHP_ROUND_HALF_EVEN=3)
            let frac = scaled.fract().abs();
            if (frac - 0.5).abs() < 1e-9 {
                let t = scaled.trunc();
                if t as i64 % 2 == 0 {
                    t
                } else {
                    if scaled > 0.0 { t + 1.0 } else { t - 1.0 }
                }
            } else {
                scaled.round()
            }
        }
        4 => { // HALF_ODD / HalfOdd (PHP_ROUND_HALF_ODD=4)
            let frac = scaled.fract().abs();
            if (frac - 0.5).abs() < 1e-9 {
                let t = scaled.trunc();
                if t as i64 % 2 != 0 {
                    t
                } else {
                    if scaled > 0.0 { t + 1.0 } else { t - 1.0 }
                }
            } else {
                scaled.round()
            }
        }
        5 => { // PHP_ROUND_CEILING / PositiveInfinity
            scaled.ceil()
        }
        6 => { // PHP_ROUND_FLOOR / NegativeInfinity
            scaled.floor()
        }
        7 => { // PHP_ROUND_TOWARD_ZERO / TowardsZero
            scaled.trunc()
        }
        8 => { // PHP_ROUND_AWAY_FROM_ZERO / AwayFromZero
            if scaled > 0.0 { scaled.ceil() } else { scaled.floor() }
        }
        _ => {
            vm.throw_type_error("round(): Argument #3 ($mode) must be a valid rounding mode (PHP_ROUND_HALF_*)".to_string());
            return Ok(Value::Null);
        }
    };
    Ok(Value::Double(rounded / factor))
}

fn max(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        let msg = "max() expects at least 1 argument, 0 given".to_string();
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    if args.len() == 1 {
        // Single argument must be an array
        if let Value::Array(arr) = &args[0] {
            let arr = arr.borrow();
            if arr.len() == 0 {
                let msg = "max(): Argument #1 ($value) must contain at least one element".to_string();
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
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
            let type_name = args[0].type_name();
            let msg = format!("max(): Argument #1 ($value) must be of type array, {} given", type_name);
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
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
        let msg = "min() expects at least 1 argument, 0 given".to_string();
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg, line: vm.current_line });
    }

    if args.len() == 1 {
        if let Value::Array(arr) = &args[0] {
            let arr = arr.borrow();
            if arr.len() == 0 {
                let msg = "min(): Argument #1 ($value) must contain at least one element".to_string();
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
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
            let type_name = args[0].type_name();
            let msg = format!("min(): Argument #1 ($value) must be of type array, {} given", type_name);
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
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

fn pow(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let base = args.first().unwrap_or(&Value::Null);
    let exp = args.get(1).unwrap_or(&Value::Null);
    // Emit deprecation warning for pow(0, negative)
    let base_f = base.to_double();
    let exp_f = exp.to_double();
    if base_f == 0.0 && exp_f < 0.0 {
        vm.emit_deprecated_at("Power of base 0 and negative exponent is deprecated", 0);
    }
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

fn array_sum(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    if let Value::Array(arr) = val {
        let arr = arr.borrow();
        let mut sum = Value::Long(0);
        for (_, v) in arr.iter() {
            let val = match v {
                Value::Reference(r) => r.borrow().clone(),
                other => other.clone(),
            };
            match &val {
                Value::Long(_) | Value::Double(_) | Value::True | Value::False | Value::Null => {
                    sum = sum.add(&val);
                }
                Value::String(s) => {
                    // Only numeric strings are summed, others emit warning
                    if goro_core::value::parse_numeric_string(s.as_bytes()).is_some() {
                        sum = sum.add(&val);
                    } else {
                        vm.emit_warning_at(
                            "array_sum(): Addition is not supported on type string",
                            vm.current_line,
                        );
                    }
                }
                Value::Array(_) => {
                    vm.emit_warning_at(
                        "array_sum(): Addition is not supported on type array",
                        vm.current_line,
                    );
                }
                Value::Object(obj) => {
                    let class_name = String::from_utf8_lossy(&obj.borrow().class_name).to_string();
                    vm.emit_warning_at(
                        &format!("array_sum(): Addition is not supported on type {}", class_name),
                        vm.current_line,
                    );
                }
                _ => {
                    let type_name = Vm::value_type_name(&val);
                    vm.emit_warning_at(
                        &format!("array_sum(): Addition is not supported on type {}", type_name),
                        vm.current_line,
                    );
                }
            }
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
    let exists = vm.functions.contains_key(&name_lower)
        || vm.user_functions.contains_key(&name_lower);
    Ok(if exists { Value::True } else { Value::False })
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
    let val = args.first().unwrap_or(&Value::Null);
    if !check_int_arg(_vm, val, "decbin", "num", 1)? {
        return Ok(Value::Null);
    }
    Ok(Value::String(goro_core::string::PhpString::from_string(
        format!("{:b}", val.to_long_coerced()),
    )))
}
fn decoct_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    if !check_int_arg(_vm, val, "decoct", "num", 1)? {
        return Ok(Value::Null);
    }
    Ok(Value::String(goro_core::string::PhpString::from_string(
        format!("{:o}", val.to_long_coerced()),
    )))
}
fn dechex_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    if !check_int_arg(_vm, val, "dechex", "num", 1)? {
        return Ok(Value::Null);
    }
    Ok(Value::String(goro_core::string::PhpString::from_string(
        format!("{:x}", val.to_long_coerced()),
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
fn array_product(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr = arr.borrow();
        let mut product = Value::Long(1);
        for (_, v) in arr.iter() {
            let val = match v {
                Value::Reference(r) => r.borrow().clone(),
                other => other.clone(),
            };
            match &val {
                Value::Long(_) | Value::Double(_) | Value::True | Value::False | Value::Null => {
                    product = product.mul(&val);
                }
                Value::String(s) => {
                    // Only numeric strings are multiplied, others emit warning
                    if goro_core::value::parse_numeric_string(s.as_bytes()).is_some() {
                        product = product.mul(&val);
                    } else {
                        vm.emit_warning_at(
                            "array_product(): Multiplication is not supported on type string",
                            vm.current_line,
                        );
                    }
                }
                Value::Array(_) => {
                    vm.emit_warning_at(
                        "array_product(): Multiplication is not supported on type array",
                        vm.current_line,
                    );
                }
                Value::Object(obj) => {
                    let class_name = String::from_utf8_lossy(&obj.borrow().class_name).to_string();
                    vm.emit_warning_at(
                        &format!("array_product(): Multiplication is not supported on type {}", class_name),
                        vm.current_line,
                    );
                }
                _ => {
                    let type_name = Vm::value_type_name(&val);
                    vm.emit_warning_at(
                        &format!("array_product(): Multiplication is not supported on type {}", type_name),
                        vm.current_line,
                    );
                }
            }
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
        let neg_dec = decimals_raw.checked_neg().unwrap_or(i64::MAX) as u64;
        if neg_dec >= 20 {
            (0.0f64, 0usize)
        } else {
            let factor = 10f64.powi(neg_dec as i32);
            let rounded = (num / factor).round() * factor;
            (rounded, 0usize)
        }
    } else {
        // Round using PHP's rounding (half away from zero) before formatting
        let rounded = php_round_value(num, decimals_raw.min(100000) as i32);
        (rounded, decimals_raw.min(100000) as usize)
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
    if base != 10 && base >= 2 && base <= 36 {
        if let Value::String(s) = val {
            let s = s.to_string_lossy();
            let s = s.trim();
            let result = i64::from_str_radix(
                s.trim_start_matches("0x").trim_start_matches("0X").trim_start_matches("0b").trim_start_matches("0B").trim_start_matches("0o").trim_start_matches("0O"),
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

fn srand_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Check for MT_RAND_PHP mode deprecation (second argument = 1)
    if let Some(mode) = args.get(1) {
        if mode.to_long() == 1 {
            vm.emit_deprecated_at("The MT_RAND_PHP variant of Mt19937 is deprecated", vm.current_line);
        }
    }
    // srand/mt_srand is a no-op in our implementation since we use the system RNG
    Ok(Value::Null)
}

fn lcg_value_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // lcg_value() returns a pseudo-random float between 0 and 1
    use std::time::SystemTime;
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as f64;
    Ok(Value::Double(seed / 1_000_000_000.0))
}
