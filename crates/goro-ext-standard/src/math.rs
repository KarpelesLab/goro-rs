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
    vm.register_function(b"php_sapi_name", php_sapi_name);
    vm.register_function(b"defined", defined);
    vm.register_function(b"function_exists", function_exists);
}

fn abs(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    Ok(match val {
        Value::Long(n) => Value::Long(n.abs()),
        Value::Double(f) => Value::Double(f.abs()),
        _ => Value::Long(val.to_long().abs()),
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

fn max(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Err(VmError {
            message: "max() expects at least 1 argument".into(),
            line: 0,
        });
    }

    // If single array argument, find max in array
    if args.len() == 1 {
        if let Value::Array(arr) = &args[0] {
            let arr = arr.borrow();
            let mut max_val = Value::Null;
            let mut first = true;
            for (_, v) in arr.iter() {
                if first || v.compare(&max_val) > 0 {
                    max_val = v.clone();
                    first = false;
                }
            }
            return Ok(max_val);
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

fn min(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Err(VmError {
            message: "min() expects at least 1 argument".into(),
            line: 0,
        });
    }

    if args.len() == 1 {
        if let Value::Array(arr) = &args[0] {
            let arr = arr.borrow();
            let mut min_val = Value::Null;
            let mut first = true;
            for (_, v) in arr.iter() {
                if first || v.compare(&min_val) < 0 {
                    min_val = v.clone();
                    first = false;
                }
            }
            return Ok(min_val);
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

fn intdiv(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let a = args.first().unwrap_or(&Value::Null).to_long();
    let b = args.get(1).unwrap_or(&Value::Null).to_long();
    if b == 0 {
        return Err(VmError {
            message: "Division by zero".into(),
            line: 0,
        });
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

fn defined(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // TODO: implement constant table lookup
    Ok(Value::False)
}

fn function_exists(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().unwrap_or(&Value::Null).to_php_string();
    let name_lower: Vec<u8> = name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    // Check if the function is registered (we need access to the function table)
    // For now, return false for unknown functions
    // The VM stores functions in its HashMap, but we can't access it from here directly
    // through the builtin fn signature. This will be fixed when we refactor.
    let _ = name_lower;
    let _ = vm;
    Ok(Value::False)
}
