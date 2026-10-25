use goro_core::opcode::ParamType;
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
    vm.register_function(b"clamp", clamp_fn);
    vm.register_function(b"version_compare", version_compare_fn);
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
    vm.register_function(b"random_int", random_int);
    vm.register_function(b"random_bytes", random_bytes_fn);
    vm.register_function(b"sinh", sinh_fn);
    vm.register_function(b"cosh", cosh_fn);
    vm.register_function(b"tanh", tanh_fn);
    vm.register_function(b"asinh", asinh_fn);
    vm.register_function(b"acosh", acosh_fn);
    vm.register_function(b"atanh", atanh_fn);
    vm.register_function(b"fdiv", fdiv_fn);
    vm.register_function(b"log1p", log1p_fn);
    vm.register_function(b"expm1", expm1_fn);
    vm.register_function(b"getrandmax", getrandmax_fn);
    vm.register_function(b"mt_getrandmax", mt_getrandmax_fn);
    vm.register_function(b"intval", intval_fn);
    vm.register_function(b"floatval", floatval_fn);
    vm.register_function(b"doubleval", floatval_fn);
    vm.register_function(b"fpow", fpow_fn);
    vm.register_function(b"srand", srand_fn);
    vm.register_function(b"mt_srand", srand_fn);
    vm.register_function(b"lcg_value", lcg_value_fn);

    // Register parameter types for strict_types enforcement on builtins
    let num = || {
        Some(ParamType::Union(vec![
            ParamType::Simple(b"int".to_vec()),
            ParamType::Simple(b"float".to_vec()),
        ]))
    };
    let i = || Some(ParamType::Simple(b"int".to_vec()));
    let f = || Some(ParamType::Simple(b"float".to_vec()));
    let s = || Some(ParamType::Simple(b"string".to_vec()));
    vm.register_builtin_param_types(b"abs", vec![num()]);
    vm.register_builtin_param_types(b"ceil", vec![num()]);
    vm.register_builtin_param_types(b"floor", vec![num()]);
    vm.register_builtin_param_types(b"round", vec![num(), i(), None]);
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
    // (the generic zpp layer; its arginfo says float but Z_PARAM_NUMBER
    // parses it: see arginfo::zpp_number_param())
    vm.zpp_extension_verified.insert(
        b"number_format".to_vec(),
        goro_core::arginfo::CHECK_ARITY | goro_core::arginfo::CHECK_TYPES,
    );
    vm.register_builtin_param_types(b"fdiv", vec![f(), f()]);
    // These validate their own arguments (check_math_num_arg/check_int_arg) and
    // emit the null / float-to-int deprecations themselves.
    for name in [
        &b"abs"[..],
        b"ceil",
        b"floor",
        b"round",
        b"decbin",
        b"decoct",
        b"dechex",
    ] {
        vm.builtin_self_coercing.insert(name.to_vec());
    }
}

/// Throw a TypeError for a math function argument and set the current exception
fn throw_math_type_error(vm: &mut Vm, message: String) {
    let exc = vm.throw_type_error(message);
    vm.current_exception = Some(exc);
}

/// Check math function argument for null deprecation warning or type error.
/// Returns true if processing should continue (value was acceptable), false if error was thrown.
fn check_math_num_arg(
    vm: &mut Vm,
    val: &Value,
    func_name: &str,
    param_name: &str,
    param_num: u32,
) -> Result<bool, VmError> {
    match val {
        Value::Null => {
            vm.emit_deprecated_at(
                &format!(
                    "{}(): Passing null to parameter #{} (${}) of type int|float is deprecated",
                    func_name, param_num, param_name
                ),
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
                throw_math_type_error(
                    vm,
                    format!(
                        "{}(): Argument #{} (${}) must be of type int|float, {} given",
                        func_name, param_num, param_name, type_name
                    ),
                );
                Ok(false)
            }
        }
        Value::Reference(r) => {
            let inner = r.borrow().clone();
            check_math_num_arg(vm, &inner, func_name, param_name, param_num)
        }
        _ => {
            let type_name = Vm::value_type_name(val);
            throw_math_type_error(
                vm,
                format!(
                    "{}(): Argument #{} (${}) must be of type int|float, {} given",
                    func_name, param_num, param_name, type_name
                ),
            );
            Ok(false)
        }
    }
}

/// Check math function argument for int type only.
/// In PHP 8, floats that fit in int are silently coerced. Floats with fractional parts
/// emit a deprecation. Very large floats that don't fit throw TypeError.
fn check_int_arg(
    vm: &mut Vm,
    val: &Value,
    func_name: &str,
    param_name: &str,
    param_num: u32,
) -> Result<bool, VmError> {
    check_int_arg_as(vm, val, func_name, param_name, param_num, "int")
}

/// check_int_arg() for a parameter whose declared type, as errors name it,
/// is `ty` (e.g. round()'s RoundingMode|int $mode).
fn check_int_arg_as(
    vm: &mut Vm,
    val: &Value,
    func_name: &str,
    param_name: &str,
    param_num: u32,
    ty: &str,
) -> Result<bool, VmError> {
    match val {
        Value::Null => {
            // null is accepted with deprecation in PHP 8.1+
            vm.emit_deprecated_at(
                &format!(
                    "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
                    func_name, param_num, param_name, ty
                ),
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
            let fits_in_int =
                !f.is_nan() && !f.is_infinite() && *f >= lower_bound && *f < upper_bound;
            if !fits_in_int {
                let type_name = Vm::value_type_name(val);
                throw_math_type_error(
                    vm,
                    format!(
                        "{}(): Argument #{} (${}) must be of type {}, {} given",
                        func_name, param_num, param_name, ty, type_name
                    ),
                );
                Ok(false)
            } else {
                if f.fract() != 0.0 {
                    vm.emit_deprecated_at(
                        &format!(
                            "Implicit conversion from float {} to int loses precision",
                            goro_core::value::format_php_float_repr(*f)
                        ),
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
                        &format!(
                            "Implicit conversion from float-string \"{}\" to int loses precision",
                            str_val
                        ),
                        vm.current_line,
                    );
                }
                Ok(true)
            } else {
                let type_name = Vm::value_type_name(val);
                throw_math_type_error(
                    vm,
                    format!(
                        "{}(): Argument #{} (${}) must be of type {}, {} given",
                        func_name, param_num, param_name, ty, type_name
                    ),
                );
                Ok(false)
            }
        }
        Value::Reference(r) => {
            let inner = r.borrow().clone();
            check_int_arg_as(vm, &inner, func_name, param_name, param_num, ty)
        }
        _ => {
            let type_name = Vm::value_type_name(val);
            throw_math_type_error(
                vm,
                format!(
                    "{}(): Argument #{} (${}) must be of type {}, {} given",
                    func_name, param_num, param_name, ty, type_name
                ),
            );
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
    let skipped = std::mem::take(&mut vm.builtin_named_skipped);
    let precision = if skipped.contains(&1) {
        0
    } else if let Some(prec_val) = args.get(1) {
        if !check_int_arg(vm, prec_val, "round", "precision", 2)? {
            return Ok(Value::Null);
        }
        prec_val.to_long_coerced()
    } else {
        0
    };

    let mode = if let Some(mode_val) = args.get(2).filter(|_| !skipped.contains(&2)) {
        let mode_val = mode_val.deref();
        match &mode_val {
            Value::Object(obj) => {
                let obj_ref = obj.borrow();
                if obj_ref.class_name.eq_ignore_ascii_case(b"RoundingMode") {
                    match obj_ref.get_property(b"name").to_php_string().as_bytes() {
                        b"HalfAwayFromZero" => 1,
                        b"HalfTowardsZero" => 2,
                        b"HalfEven" => 3,
                        b"HalfOdd" => 4,
                        b"PositiveInfinity" => 5,
                        b"NegativeInfinity" => 6,
                        b"TowardsZero" => 7,
                        b"AwayFromZero" => 8,
                        _ => 0,
                    }
                } else {
                    let class = goro_core::value::display_class_name(&obj_ref.class_name);
                    drop(obj_ref);
                    throw_math_type_error(
                        vm,
                        format!(
                            "round(): Argument #3 ($mode) must be of type RoundingMode|int, {} given",
                            class
                        ),
                    );
                    return Ok(Value::Null);
                }
            }
            Value::Long(n) => *n,
            Value::Double(_) | Value::String(_) | Value::True | Value::False | Value::Null => {
                if !check_int_arg_as(vm, &mode_val, "round", "mode", 3, "RoundingMode|int")? {
                    return Ok(Value::Null);
                }
                mode_val.to_long_coerced()
            }
            other => {
                let tn = Vm::value_type_name(other);
                throw_math_type_error(
                    vm,
                    format!(
                        "round(): Argument #3 ($mode) must be of type RoundingMode|int, {} given",
                        tn
                    ),
                );
                return Ok(Value::Null);
            }
        }
    } else {
        1 // PHP_ROUND_HALF_UP
    };
    if !(1..=8).contains(&mode) {
        let msg = "round(): Argument #3 ($mode) must be a valid rounding mode (RoundingMode::*)";
        let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Ok(Value::Null);
    }
    let places: i32 = if precision >= 0 {
        if precision > i32::MAX as i64 {
            i32::MAX
        } else {
            precision as i32
        }
    } else if precision < i32::MIN as i64 {
        i32::MIN
    } else {
        precision as i32
    };
    // Integers with non-negative precision need no rounding.
    if matches!(
        val.deref(),
        Value::Long(_) | Value::True | Value::False | Value::Null
    ) && places >= 0
    {
        return Ok(Value::Double(f));
    }
    if let Value::String(s) = val.deref() {
        if places >= 0 {
            if s.to_string_lossy().trim_start().parse::<i64>().is_ok() {
                return Ok(Value::Double(f));
            }
        }
    }
    Ok(Value::Double(php_math_round(f, places, mode)))
}

fn php_intpow10(power: i32) -> f64 {
    if !(0..=22).contains(&power) {
        return 10f64.powf(power as f64);
    }
    const POWERS: [f64; 23] = [
        1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
        1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
    ];
    POWERS[power as usize]
}

fn php_round_helper(integral: f64, value: f64, exponent: f64, places: i32, mode: i64) -> f64 {
    let value_abs = value.abs();
    let basic = || {
        if places > 0 {
            ((integral + 0.5f64.copysign(integral)) / exponent).abs()
        } else {
            ((integral + 0.5f64.copysign(integral)) * exponent).abs()
        }
    };
    let zero = || {
        if places > 0 {
            (integral / exponent).abs()
        } else {
            (integral * exponent).abs()
        }
    };
    match mode {
        1 => {
            if value_abs >= basic() {
                integral + 1f64.copysign(integral)
            } else {
                integral
            }
        }
        2 => {
            if value_abs > basic() {
                integral + 1f64.copysign(integral)
            } else {
                integral
            }
        }
        5 => {
            if value > 0.0 && value_abs > zero() {
                integral + 1.0
            } else {
                integral
            }
        }
        6 => {
            if value < 0.0 && value_abs > zero() {
                integral - 1.0
            } else {
                integral
            }
        }
        7 => integral,
        8 => {
            if value_abs > zero() {
                integral + 1f64.copysign(integral)
            } else {
                integral
            }
        }
        3 | 4 => {
            let edge = basic();
            if value_abs > edge {
                integral + 1f64.copysign(integral)
            } else if value_abs == edge {
                let even = integral % 2.0 == 0.0;
                if (mode == 3) != even {
                    integral + 1f64.copysign(integral)
                } else {
                    integral
                }
            } else {
                integral
            }
        }
        _ => integral,
    }
}

/// Port of _php_math_round() (ext/standard/math.c).
pub fn php_math_round(value: f64, places: i32, mode: i64) -> f64 {
    if !value.is_finite() || value == 0.0 {
        return value;
    }
    let places = if places < i32::MIN + 1 {
        i32::MIN + 1
    } else {
        places
    };
    let exponent = php_intpow10(places.abs());
    let (mut tmp_value, tmp_value2);
    if value >= 0.0 {
        tmp_value = (if places > 0 {
            value * exponent
        } else {
            value / exponent
        })
        .floor();
        tmp_value2 = tmp_value + 1.0;
    } else {
        tmp_value = (if places > 0 {
            value * exponent
        } else {
            value / exponent
        })
        .ceil();
        tmp_value2 = tmp_value - 1.0;
    }
    if (if places > 0 {
        tmp_value2 / exponent
    } else {
        tmp_value2 * exponent
    }) == value
    {
        tmp_value = tmp_value2;
    }
    if tmp_value.abs() >= 1e16 {
        return value;
    }
    tmp_value = php_round_helper(tmp_value, value, exponent, places, mode);
    if places.abs() < 23 {
        if places > 0 {
            tmp_value / exponent
        } else {
            tmp_value * exponent
        }
    } else {
        let s = format!("{:15.6}e{}", tmp_value, -(places as i64));
        match s.trim().parse::<f64>() {
            Ok(v) if v.is_finite() => v,
            _ => value,
        }
    }
}

/// zend_dval_to_lval_silent((double) n) == n: the int converts to a double
/// and back unchanged.
fn long_fits_double(n: i64) -> bool {
    let d = n as f64;
    (-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&d) && d as i64 == n
}

fn max(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        let msg = "max() expects at least 1 argument, 0 given".to_string();
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }

    if args.len() == 1 {
        // Single argument must be an array
        if let Value::Array(arr) = &args[0] {
            let arr = arr.borrow();
            if arr.is_empty() {
                let msg =
                    "max(): Argument #1 ($value) must contain at least one element".to_string();
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg,
                    line: vm.current_line,
                });
            }
            let mut max_val = Value::Null;
            let mut first = true;
            let vals: Vec<Value> = arr.values().cloned().collect();
            drop(arr);
            for v in vals {
                if first || vm.zend_compare(&v, &max_val) > 0 {
                    max_val = v;
                    first = false;
                }
                if vm.current_exception.is_some() {
                    return Ok(Value::Null);
                }
            }
            return Ok(max_val);
        } else {
            let type_name = Vm::value_type_name(&args[0]);
            let msg = format!(
                "max(): Argument #1 ($value) must be of type array, {} given",
                type_name
            );
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    }

    // ZEND_FRAMELESS_FUNCTION(max, 2): what a direct two-argument call runs
    // (the lhs unless it compares below the rhs; uncomparable gives the lhs).
    if args.len() == 2 {
        let c = vm.zend_compare(&args[0], &args[1]);
        if vm.current_exception.is_some() {
            return Ok(Value::Null);
        }
        let lhs_wins = match (args[0].deref(), args[1].deref()) {
            (Value::Long(a), Value::Long(b)) => a >= b,
            (Value::Double(a), Value::Double(b)) => a >= b,
            // (only an int a double holds exactly takes the double path)
            (Value::Long(a), Value::Double(b)) if long_fits_double(a) => a as f64 >= b,
            (Value::Double(a), Value::Long(b)) if long_fits_double(b) => a >= b as f64,
            _ => c >= 0,
        };
        return Ok(if lhs_wins {
            args[0].clone()
        } else {
            args[1].clone()
        });
    }
    let mut max_val = args[0].clone();
    for arg in &args[1..] {
        if vm.zend_compare(arg, &max_val) > 0 {
            max_val = arg.clone();
        }
        if vm.current_exception.is_some() {
            return Ok(Value::Null);
        }
    }
    Ok(max_val)
}

fn min(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        let msg = "min() expects at least 1 argument, 0 given".to_string();
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }

    if args.len() == 1 {
        if let Value::Array(arr) = &args[0] {
            let arr = arr.borrow();
            if arr.is_empty() {
                let msg =
                    "min(): Argument #1 ($value) must contain at least one element".to_string();
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg,
                    line: vm.current_line,
                });
            }
            let mut min_val = Value::Null;
            let mut first = true;
            let vals: Vec<Value> = arr.values().cloned().collect();
            drop(arr);
            for v in vals {
                if first || vm.zend_compare(&v, &min_val) < 0 {
                    min_val = v;
                    first = false;
                }
                if vm.current_exception.is_some() {
                    return Ok(Value::Null);
                }
            }
            return Ok(min_val);
        } else {
            let type_name = Vm::value_type_name(&args[0]);
            let msg = format!(
                "min(): Argument #1 ($value) must be of type array, {} given",
                type_name
            );
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    }

    // ZEND_FRAMELESS_FUNCTION(min, 2): what a direct two-argument call runs
    // (the lhs only when it compares below the rhs).
    if args.len() == 2 {
        let c = vm.zend_compare(&args[0], &args[1]);
        if vm.current_exception.is_some() {
            return Ok(Value::Null);
        }
        let lhs_wins = match (args[0].deref(), args[1].deref()) {
            (Value::Long(a), Value::Long(b)) => a < b,
            (Value::Double(a), Value::Double(b)) => a < b,
            (Value::Long(a), Value::Double(b)) if long_fits_double(a) => (a as f64) < b,
            (Value::Double(a), Value::Long(b)) if long_fits_double(b) => a < b as f64,
            _ => c < 0,
        };
        return Ok(if lhs_wins {
            args[0].clone()
        } else {
            args[1].clone()
        });
    }
    let mut min_val = args[0].clone();
    for arg in &args[1..] {
        if vm.zend_compare(arg, &min_val) < 0 {
            min_val = arg.clone();
        }
        if vm.current_exception.is_some() {
            return Ok(Value::Null);
        }
    }
    Ok(min_val)
}

fn sqrt(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = args.first().unwrap_or(&Value::Null).to_double();
    Ok(Value::Double(f.sqrt()))
}

fn pow(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() != 2 {
        let msg = format!("pow() expects exactly 2 arguments, {} given", args.len());
        let exc = vm.create_exception(b"ArgumentCountError", &msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let base = &args[0];
    let exp = &args[1];
    // If either operand is a GMP object, route through the extension's operator
    // overloading so pow(gmp, n) / pow(n, gmp) returns a GMP object (matching PHP).
    let is_gmp_obj = vm
        .object_is_operator_type
        .map(|f| f(base) || f(exp))
        .unwrap_or(false);
    if is_gmp_obj {
        if let Some(op_fn) = vm.object_do_operation {
            if let Some(result) = op_fn(vm, "**", base, exp) {
                return match result {
                    Ok(v) => Ok(v),
                    Err(e) => {
                        // Map specific error markers (V/D prefixes) used by GMP.
                        if let Some(stripped) = e.strip_prefix("\x01V\x01") {
                            let msg = stripped.to_string();
                            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                            vm.current_exception = Some(exc);
                            Err(VmError {
                                message: msg,
                                line: vm.current_line,
                            })
                        } else if let Some(stripped) = e.strip_prefix("\x01D\x01") {
                            let msg = stripped.to_string();
                            let exc =
                                vm.create_exception(b"DivisionByZeroError", &msg, vm.current_line);
                            vm.current_exception = Some(exc);
                            Err(VmError {
                                message: msg,
                                line: vm.current_line,
                            })
                        } else {
                            let exc = vm.create_exception(b"TypeError", &e, vm.current_line);
                            vm.current_exception = Some(exc);
                            Err(VmError {
                                message: e,
                                line: vm.current_line,
                            })
                        }
                    }
                };
            }
        }
    }
    // pow_function(): op1 then op2 are converted to numbers
    // (zendi_try_convert_scalar_to_number): a leading-numeric string warns,
    // an operand that cannot be converted stops there.
    for v in [base, exp] {
        if Vm::check_unsupported_operand_types(v, &Value::Long(0), "**").is_some() {
            break;
        }
        if let Value::String(s) = v.deref() {
            if matches!(
                goro_core::value::parse_numeric_str(s.as_bytes(), true),
                Some((_, true))
            ) {
                vm.emit_warning_at("A non-numeric value encountered", vm.current_line);
                if vm.current_exception.is_some() {
                    return Err(VmError {
                        message: vm.uncaught_exception_message(),
                        line: vm.current_line,
                    });
                }
            }
        }
    }
    if let Some(msg) = Vm::check_unsupported_operand_types(base, exp, "**") {
        return throw_err(vm, b"TypeError", msg);
    }
    // Emit deprecation warning for pow(0, negative)
    let base_f = base.to_double();
    let exp_f = exp.to_double();
    if base_f == 0.0 && exp_f < 0.0 {
        let line = vm.current_line;
        vm.emit_deprecated_at("Power of base 0 and negative exponent is deprecated", line);
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
        return Err(VmError {
            message: msg.into(),
            line: vm.current_line,
        });
    }
    if a == i64::MIN && b == -1 {
        let msg = "Division of PHP_INT_MIN by -1 is not an integer";
        let exc = vm.create_exception(b"ArithmeticError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg.into(),
            line: vm.current_line,
        });
    }
    Ok(Value::Long(a / b))
}

fn fmod_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = args.first().unwrap_or(&Value::Null).to_double();
    let b = args.get(1).unwrap_or(&Value::Null).to_double();
    Ok(Value::Double(a % b))
}

/// Throw `class` with `msg` from a builtin.
fn throw_err(vm: &mut Vm, class: &[u8], msg: String) -> Result<Value, VmError> {
    let exc = vm.create_exception(class, &msg, vm.current_line);
    vm.current_exception = Some(exc);
    Err(VmError {
        message: msg,
        line: vm.current_line,
    })
}

/// mt_rand()/rand(): no arguments or exactly two.
fn rand_args(vm: &mut Vm, fname: &str, args: &[Value]) -> Result<Option<(i64, i64)>, VmError> {
    match args.len() {
        0 => return Ok(None),
        2 => {}
        n => {
            return throw_err(
                vm,
                b"ArgumentCountError",
                format!("{}() expects exactly 2 arguments, {} given", fname, n),
            )
            .map(|_| None);
        }
    }
    // Z_PARAM_LONG(min) / Z_PARAM_LONG(max): the diagnostics of the
    // accepted conversions (null, fractional floats) are emitted by the
    // builtin parameter types already.
    let fits = |d: f64| !d.is_nan() && (-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&d);
    let mut out = [0i64; 2];
    for (i, name) in ["min", "max"].iter().enumerate() {
        let v = args[i].deref();
        let n = match &v {
            Value::Null | Value::Undef | Value::False => Some(0),
            Value::True => Some(1),
            Value::Long(n) => Some(*n),
            Value::Double(d) if fits(*d) => Some(*d as i64),
            Value::String(s) => match goro_core::value::parse_numeric_str(s.as_bytes(), false) {
                Some((goro_core::value::NumStr::Long(n), _)) => Some(n),
                Some((goro_core::value::NumStr::Double(d), _)) if fits(d) => Some(d as i64),
                _ => None,
            },
            _ => None,
        };
        match n {
            Some(n) => out[i] = n,
            None => {
                return throw_err(
                    vm,
                    b"TypeError",
                    format!(
                        "{}(): Argument #{} (${}) must be of type int, {} given",
                        fname,
                        i + 1,
                        name,
                        Vm::value_type_name(&v)
                    ),
                )
                .map(|_| None);
            }
        }
    }
    Ok(Some((out[0], out[1])))
}

fn rand_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let Some((min, max)) = rand_args(vm, "rand", args)? else {
        return Ok(Value::Long((goro_core::random::mt_rand_u32() >> 1) as i64));
    };
    let v = if max < min {
        goro_core::random::mt_rand_common(vm, max, min)
    } else {
        goro_core::random::mt_rand_common(vm, min, max)
    };
    Ok(Value::Long(v))
}

fn clamp_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() < 3 {
        let msg = format!("clamp() expects exactly 3 arguments, {} given", args.len());
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let value = &args[0];
    let min = &args[1];
    let max = &args[2];

    // Check for NaN in min/max
    if let Value::Double(f) = min {
        if f.is_nan() {
            let msg = "clamp(): Argument #2 ($min) must not be NAN".to_string();
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    }
    if let Value::Double(f) = max {
        if f.is_nan() {
            let msg = "clamp(): Argument #3 ($max) must not be NAN".to_string();
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    }

    // Compare min > max
    let cmp_min_max = Vm::php_compare_values(min, max);
    if cmp_min_max == std::cmp::Ordering::Greater {
        let msg = "clamp(): Argument #2 ($min) must be smaller than or equal to argument #3 ($max)"
            .to_string();
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }

    // clamp: if value < min, return min; if value > max, return max; else return value
    let cmp_val_min = Vm::php_compare_values(value, min);
    if cmp_val_min == std::cmp::Ordering::Less {
        return Ok(min.clone());
    }
    let cmp_val_max = Vm::php_compare_values(value, max);
    if cmp_val_max == std::cmp::Ordering::Greater {
        return Ok(max.clone());
    }
    Ok(value.clone())
}

fn mt_rand(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let Some((min, max)) = rand_args(vm, "mt_rand", args)? else {
        return Ok(Value::Long((goro_core::random::mt_rand_u32() >> 1) as i64));
    };
    if max < min {
        return throw_err(
            vm,
            b"ValueError",
            "mt_rand(): Argument #2 ($max) must be greater than or equal to argument #1 ($min)"
                .to_string(),
        );
    }
    Ok(Value::Long(goro_core::random::mt_rand_common(vm, min, max)))
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
    // PHP_FUNCTION(log): exact log2()/log10() for those bases, NAN for 1.
    match base {
        None => Ok(Value::Double(val.ln())),
        Some(2.0) => Ok(Value::Double(val.log2())),
        Some(10.0) => Ok(Value::Double(val.log10())),
        Some(1.0) => Ok(Value::Double(f64::NAN)),
        Some(b) if b <= 0.0 => {
            let msg = "log(): Argument #2 ($base) must be greater than 0";
            let exc = vm.create_exception(b"ValueError", msg, 0);
            vm.current_exception = Some(exc);
            Err(VmError {
                message: msg.into(),
                line: vm.current_line,
            })
        }
        Some(b) => Ok(Value::Double(val.ln() / b.ln())),
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
        args.first().unwrap_or(&Value::Null).to_double() / 180.0 * std::f64::consts::PI,
    ))
}
fn rad2deg_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double() / std::f64::consts::PI * 180.0,
    ))
}
/// Z_PARAM_STR for the base conversion functions: scalars convert, Stringable
/// objects use __toString, anything else throws a TypeError.
fn base_str_arg(
    vm: &mut Vm,
    val: &Value,
    func: &str,
    param: &str,
) -> Option<goro_core::string::PhpString> {
    let v = val.deref();
    match &v {
        Value::Array(_) | Value::Resource(_) => {}
        Value::Object(obj) => {
            let lower: Vec<u8> = obj.borrow().class_name.to_ascii_lowercase();
            let has = vm
                .classes
                .get(&lower)
                .map(|c| c.get_method(b"__tostring").is_some())
                .unwrap_or(false);
            if has {
                return Some(vm.value_to_string(&v));
            }
        }
        _ => return Some(v.to_php_string()),
    }
    let tn = Vm::value_type_name(&v);
    throw_math_type_error(
        vm,
        format!(
            "{}(): Argument #1 (${}) must be of type string, {} given",
            func, param, tn
        ),
    );
    None
}

/// Port of _php_math_basetozval().
fn php_basetozval(vm: &mut Vm, s: &[u8], base: u32) -> Value {
    let mut st = 0usize;
    let mut e = s.len();
    let is_space = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    while st < e && is_space(s[st]) {
        st += 1;
    }
    while st < e && is_space(s[e - 1]) {
        e -= 1;
    }
    if e - st >= 2 && s[st] == b'0' {
        let c1 = s[st + 1];
        if (base == 16 && (c1 == b'x' || c1 == b'X'))
            || (base == 8 && (c1 == b'o' || c1 == b'O'))
            || (base == 2 && (c1 == b'b' || c1 == b'B'))
        {
            st += 2;
        }
    }
    let cutoff = i64::MAX / base as i64;
    let cutlim = i64::MAX % base as i64;
    let mut num: i64 = 0;
    let mut fnum: f64 = 0.0;
    let mut float_mode = false;
    let mut invalid = 0;
    for &ch in &s[st..e] {
        let c = match ch {
            b'0'..=b'9' => (ch - b'0') as i64,
            b'A'..=b'Z' => (ch - b'A' + 10) as i64,
            b'a'..=b'z' => (ch - b'a' + 10) as i64,
            _ => {
                invalid += 1;
                continue;
            }
        };
        if c >= base as i64 {
            invalid += 1;
            continue;
        }
        if !float_mode {
            if num < cutoff || (num == cutoff && c <= cutlim) {
                num = num * base as i64 + c;
                continue;
            }
            fnum = num as f64;
            float_mode = true;
        }
        fnum = fnum * base as f64 + c as f64;
    }
    if invalid > 0 {
        vm.emit_deprecated_at(
            "Invalid characters passed for attempted conversion, these have been ignored",
            vm.current_line,
        );
    }
    if float_mode {
        Value::Double(fnum)
    } else {
        Value::Long(num)
    }
}

fn php_longtobase(value: i64, base: u32) -> String {
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut v = value as u64;
    let mut out = Vec::new();
    loop {
        out.push(digits[(v % base as u64) as usize]);
        v /= base as u64;
        if v == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

fn base_convert_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let num_str = match base_str_arg(
        vm,
        args.first().unwrap_or(&Value::Null),
        "base_convert",
        "num",
    ) {
        Some(s) => s,
        None => return Ok(Value::Null),
    };
    let from_base = args.get(1).map(|v| v.to_long()).unwrap_or(10);
    let to_base = args.get(2).map(|v| v.to_long()).unwrap_or(10);
    for (n, b, name) in [(2, from_base, "from_base"), (3, to_base, "to_base")] {
        if !(2..=36).contains(&b) {
            let msg = format!(
                "base_convert(): Argument #{} (${}) must be between 2 and 36 (inclusive)",
                n, name
            );
            let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Ok(Value::Null);
        }
    }
    let to_base = to_base as u32;
    let v = php_basetozval(vm, num_str.as_bytes(), from_base as u32);
    let result = match v {
        Value::Double(f) => {
            let mut fvalue = f.floor();
            if fvalue.is_infinite() {
                let msg = format!("An infinite value cannot be converted to base {}", to_base);
                let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Ok(Value::Null);
            }
            let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
            let mut out = Vec::new();
            loop {
                out.push(digits[(fvalue % to_base as f64) as usize]);
                fvalue /= to_base as f64;
                if !(out.len() < 64 && fvalue.abs() >= 1.0) {
                    break;
                }
            }
            out.reverse();
            String::from_utf8(out).unwrap_or_default()
        }
        Value::Long(n) => php_longtobase(n, to_base),
        _ => String::new(),
    };
    Ok(Value::String(goro_core::string::PhpString::from_string(
        result,
    )))
}

fn bindec_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    match base_str_arg(
        vm,
        args.first().unwrap_or(&Value::Null),
        "bindec",
        "binary_string",
    ) {
        Some(s) => Ok(php_basetozval(vm, s.as_bytes(), 2)),
        None => Ok(Value::Null),
    }
}
fn octdec_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    match base_str_arg(
        vm,
        args.first().unwrap_or(&Value::Null),
        "octdec",
        "octal_string",
    ) {
        Some(s) => Ok(php_basetozval(vm, s.as_bytes(), 8)),
        None => Ok(Value::Null),
    }
}
fn hexdec_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    match base_str_arg(
        vm,
        args.first().unwrap_or(&Value::Null),
        "hexdec",
        "hex_string",
    ) {
        Some(s) => Ok(php_basetozval(vm, s.as_bytes(), 16)),
        None => Ok(Value::Null),
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
fn random_int(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let min = args.first().map(|v| v.to_long()).unwrap_or(0);
    let max = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if min > max {
        return throw_err(
            vm,
            b"ValueError",
            "random_int(): Argument #1 ($min) must be less than or equal to argument #2 ($max)"
                .to_string(),
        );
    }
    Ok(Value::Long(goro_core::random::csprng_int(min, max)))
}
fn random_bytes_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let len = args.first().map(|v| v.to_long()).unwrap_or(0);
    if len < 1 {
        return throw_err(
            vm,
            b"ValueError",
            "random_bytes(): Argument #1 ($length) must be greater than 0".to_string(),
        );
    }
    let mut bytes = vec![0u8; len as usize];
    goro_core::random::csprng_bytes(&mut bytes);
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

/// C strtoll(): whitespace, sign, base prefixes, saturating on overflow.
fn c_strtol(s: &[u8], base: i64) -> i64 {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut base = base;
    let has_hex_prefix = |i: usize| {
        i + 1 < s.len()
            && s[i] == b'0'
            && (s[i + 1] == b'x' || s[i + 1] == b'X')
            && s.get(i + 2).map(|c| c.is_ascii_hexdigit()).unwrap_or(false)
    };
    if base == 0 {
        if has_hex_prefix(i) {
            base = 16;
            i += 2;
        } else if i < s.len() && s[i] == b'0' {
            base = 8;
        } else {
            base = 10;
        }
    } else if base == 16 && has_hex_prefix(i) {
        i += 2;
    }
    if !(2..=36).contains(&base) {
        return 0;
    }
    let mut acc: i128 = 0;
    let mut overflow = false;
    while i < s.len() {
        let d = match s[i] {
            c @ b'0'..=b'9' => (c - b'0') as i64,
            c @ b'a'..=b'z' => (c - b'a' + 10) as i64,
            c @ b'A'..=b'Z' => (c - b'A' + 10) as i64,
            _ => break,
        };
        if d >= base {
            break;
        }
        if !overflow {
            acc = acc * base as i128 + d as i128;
            if acc > i64::MAX as i128 + 1 {
                overflow = true;
            }
        }
        i += 1;
    }
    if neg {
        if overflow || acc > i64::MAX as i128 + 1 {
            i64::MIN
        } else {
            (-acc) as i64
        }
    } else if overflow || acc > i64::MAX as i128 {
        i64::MAX
    } else {
        acc as i64
    }
}

fn intval_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    let base = args.get(1).map(|v| v.to_long()).unwrap_or(10);
    let s = match &val {
        Value::String(s) if base != 10 => s.as_bytes().to_vec(),
        Value::Object(obj) if goro_core::value::native_cast(obj, 1).is_some() => {
            return Ok(Value::Long(
                goro_core::value::native_cast(obj, 1).unwrap().to_long(),
            ));
        }
        Value::Object(obj) => {
            let cn = goro_core::value::display_class_name(&obj.borrow().class_name);
            vm.emit_warning(&format!(
                "Object of class {} could not be converted to int",
                cn
            ));
            return Ok(Value::Long(1));
        }
        Value::Double(d) => {
            // zval_get_long(): zend_dval_to_lval() warns for a float out of
            // range (and wraps it)
            let line = vm.current_line;
            return Ok(Value::Long(vm.float_to_int_cast(*d, line)));
        }
        _ => return Ok(Value::Long(val.to_long())),
    };
    if base == 0 || base == 2 {
        let mut t = 0;
        while t < s.len() && matches!(s[t], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
            t += 1;
        }
        let rest = &s[t..];
        if rest.len() > 2 {
            let off = if rest[0] == b'-' || rest[0] == b'+' {
                1
            } else {
                0
            };
            if rest[off] == b'0'
                && (rest.get(off + 1) == Some(&b'b') || rest.get(off + 1) == Some(&b'B'))
            {
                let mut tmp = Vec::new();
                if off == 1 {
                    tmp.push(rest[0]);
                }
                tmp.extend_from_slice(&rest[off + 2..]);
                return Ok(Value::Long(c_strtol(&tmp, 2)));
            }
        }
    }
    // (ZEND_STRTOL() takes the base as a C int)
    Ok(Value::Long(c_strtol(&s, base as i32 as i64)))
}

/// php_canonicalize_version(): separate digit runs from the rest with
/// dots; `-`, `_`, `+` and other non-alphanumerics become a single dot.
fn canonicalize_version(v: &[u8]) -> Vec<u8> {
    let mut q = Vec::with_capacity(v.len() * 2);
    let Some((&first, rest)) = v.split_first() else {
        return q;
    };
    let isdig = |c: u8| c.is_ascii_digit();
    let isndig = |c: u8| !c.is_ascii_digit() && c != b'.';
    let mut lp = first;
    q.push(first);
    for &p in rest {
        let lq = *q.last().unwrap();
        if matches!(p, b'-' | b'_' | b'+') {
            if lq != b'.' {
                q.push(b'.');
            }
        } else if (isndig(lp) && isdig(p)) || (isdig(lp) && isndig(p)) {
            if lq != b'.' {
                q.push(b'.');
            }
            q.push(p);
        } else if !p.is_ascii_alphanumeric() {
            if lq != b'.' {
                q.push(b'.');
            }
        } else {
            q.push(p);
        }
        lp = p;
    }
    q
}

/// compare_special_version_forms()
fn compare_special_version_forms(a: &[u8], b: &[u8]) -> i64 {
    const FORMS: [(&[u8], i64); 10] = [
        (b"dev", 0),
        (b"alpha", 1),
        (b"a", 1),
        (b"beta", 2),
        (b"b", 2),
        (b"RC", 3),
        (b"rc", 3),
        (b"#", 4),
        (b"pl", 5),
        (b"p", 5),
    ];
    let order = |f: &[u8]| {
        FORMS
            .iter()
            .find(|(n, _)| f.starts_with(n))
            .map_or(-1, |&(_, o)| o)
    };
    (order(a) - order(b)).signum()
}

/// strtol(s, NULL, 10) of a leading digit run (saturating like strtol).
fn version_strtol(s: &[u8]) -> i64 {
    let mut n: i64 = 0;
    for &c in s.iter().take_while(|c| c.is_ascii_digit()) {
        n = n.saturating_mul(10).saturating_add((c - b'0') as i64);
    }
    n
}

/// php_version_compare()
pub(crate) fn php_version_compare(v1: &[u8], v2: &[u8]) -> i64 {
    if v1.is_empty() || v2.is_empty() {
        return match (v1.is_empty(), v2.is_empty()) {
            (true, true) => 0,
            (false, _) => 1,
            _ => -1,
        };
    }
    let c1 = if v1[0] == b'#' {
        v1.to_vec()
    } else {
        canonicalize_version(v1)
    };
    let c2 = if v2[0] == b'#' {
        v2.to_vec()
    } else {
        canonicalize_version(v2)
    };
    // The C code walks NUL-terminated buffers, cutting each element at the
    // next '.'; `n1` / `n2` are None once an element had no '.' after it.
    let at = |b: &[u8], p: usize| b.get(p).copied().unwrap_or(0);
    let (mut p1, mut p2) = (0usize, 0usize);
    let (mut n1, mut n2) = (Some(0usize), Some(0usize));
    let mut compare = 0;
    while p1 < c1.len() && p2 < c2.len() && n1.is_some() && n2.is_some() {
        n1 = c1[p1..].iter().position(|&c| c == b'.').map(|k| p1 + k);
        n2 = c2[p2..].iter().position(|&c| c == b'.').map(|k| p2 + k);
        let a = &c1[p1..n1.unwrap_or(c1.len())];
        let b = &c2[p2..n2.unwrap_or(c2.len())];
        let (da, db) = (at(a, 0).is_ascii_digit(), at(b, 0).is_ascii_digit());
        compare = if da && db {
            (version_strtol(a).wrapping_sub(version_strtol(b))).signum()
        } else if !da && !db {
            compare_special_version_forms(a, b)
        } else if da {
            compare_special_version_forms(b"#N#", b)
        } else {
            compare_special_version_forms(a, b"#N#")
        };
        if compare != 0 {
            break;
        }
        if let Some(n) = n1 {
            p1 = n + 1;
        }
        if let Some(n) = n2 {
            p2 = n + 1;
        }
    }
    if compare == 0 {
        if n1.is_some() {
            compare = if at(&c1, p1).is_ascii_digit() {
                1
            } else {
                php_version_compare(&c1[p1.min(c1.len())..], b"#N#")
            };
        } else if n2.is_some() {
            compare = if at(&c2, p2).is_ascii_digit() {
                -1
            } else {
                php_version_compare(b"#N#", &c2[p2.min(c2.len())..])
            };
        }
    }
    compare
}

fn version_compare_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cstr = |v: Option<&Value>| {
        let s = v.unwrap_or(&Value::Null).to_php_string();
        let b = s.as_bytes();
        // The C implementation stops at a NUL byte.
        b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())].to_vec()
    };
    let compare = php_version_compare(&cstr(args.first()), &cstr(args.get(1)));
    let op = match args.get(2).map(|v| v.deref()) {
        None | Some(Value::Null) => return Ok(Value::Long(compare)),
        Some(v) => v.to_php_string(),
    };
    let r = match op.as_bytes() {
        b"<" | b"lt" => compare == -1,
        b"<=" | b"le" => compare != 1,
        b">" | b"gt" => compare == 1,
        b">=" | b"ge" => compare != -1,
        b"==" | b"=" | b"eq" => compare == 0,
        b"!=" | b"<>" | b"ne" => compare != 0,
        _ => {
            let msg =
                "version_compare(): Argument #3 ($operator) must be a valid comparison operator";
            let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg.into(),
                line: vm.current_line,
            });
        }
    };
    Ok(if r { Value::True } else { Value::False })
}

/// floatval(): zval_get_double(); an object without a numeric cast warns
/// and counts as 1.0.
fn floatval_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let v = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    if let Value::Object(obj) = &v {
        if let Some(d) = vm.object_to_double.and_then(|h| h(&v)) {
            return Ok(Value::Double(d));
        }
        if let Some(c) = goro_core::value::native_cast(obj, 2) {
            return Ok(Value::Double(c.to_double()));
        }
        let class_name = goro_core::value::display_class_name(&obj.borrow().class_name);
        vm.emit_warning(&format!(
            "Object of class {} could not be converted to float",
            class_name
        ));
        return Ok(Value::Double(1.0));
    }
    Ok(Value::Double(v.to_double()))
}

fn fpow_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let base = args.first().unwrap_or(&Value::Null).to_double();
    let exp = args.get(1).unwrap_or(&Value::Null).to_double();
    Ok(Value::Double(base.powf(exp)))
}

fn srand_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let seed = match args.first().map(|v| v.deref()) {
        None | Some(Value::Null) | Some(Value::Undef) => None,
        Some(v) => Some(v.to_long()),
    };
    let mode = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if mode == 1 {
        vm.emit_deprecated_at(
            "The MT_RAND_PHP variant of Mt19937 is deprecated",
            vm.current_line,
        );
    }
    goro_core::random::mt_srand(seed, mode);
    Ok(Value::Null)
}

fn lcg_value_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.emit_deprecated_at(
        "Function lcg_value() is deprecated since 8.4, use \\Random\\Randomizer::getFloat() instead",
        vm.current_line,
    );
    if !args.is_empty() && vm.current_exception.is_none() {
        return throw_err(
            vm,
            b"ArgumentCountError",
            format!(
                "lcg_value() expects exactly 0 arguments, {} given",
                args.len()
            ),
        );
    }
    Ok(Value::Double(goro_core::random::combined_lcg()))
}

/// zend_string_alloc(len): the emalloc() size of a string of `len` bytes.
pub(crate) fn zend_string_alloc_size(len: usize) -> usize {
    len.saturating_add(24 + 1 + 7) & !7
}

/// _php_math_number_format_long()
pub(crate) fn number_format_long(
    vm: &mut Vm,
    num: i64,
    dec: i64,
    dec_point: &[u8],
    thousand_sep: &[u8],
) -> Result<Vec<u8>, VmError> {
    let mut is_negative = num < 0;
    let mut tmpnum = num.unsigned_abs();
    if dec < 0 {
        if dec < -19 {
            tmpnum = 0;
        } else {
            let power = 10u64.pow((-dec) as u32);
            let rest = tmpnum % power;
            tmpnum /= power;
            tmpnum = if rest >= power / 2 {
                tmpnum.wrapping_mul(power).wrapping_add(power)
            } else {
                tmpnum.wrapping_mul(power)
            };
        }
        if tmpnum == 0 {
            is_negative = false;
        }
    }
    let digits = tmpnum.to_string().into_bytes();
    // reslen, as allocated
    let mut reslen = digits.len() + (digits.len() - 1) / 3 * thousand_sep.len();
    reslen += is_negative as usize;
    if dec > 0 {
        reslen = reslen
            .saturating_add(dec as usize)
            .saturating_add(dec_point.len());
    }
    vm.memory_check(zend_string_alloc_size(reslen))?;
    let mut out = Vec::with_capacity(reslen);
    if is_negative {
        out.push(b'-');
    }
    for (i, &c) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.extend_from_slice(thousand_sep);
        }
        out.push(c);
    }
    if dec > 0 {
        out.extend_from_slice(dec_point);
        out.resize(out.len() + dec as usize, b'0');
    }
    Ok(out)
}
