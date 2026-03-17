use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

pub fn register(vm: &mut Vm) {
    vm.register_function(b"gettype", gettype);
    vm.register_function(b"is_null", is_null);
    vm.register_function(b"is_bool", is_bool);
    vm.register_function(b"is_int", is_int);
    vm.register_function(b"is_integer", is_int);
    vm.register_function(b"is_long", is_int);
    vm.register_function(b"is_float", is_float);
    vm.register_function(b"is_double", is_float);
    vm.register_function(b"is_string", is_string);
    vm.register_function(b"is_array", is_array);
    vm.register_function(b"is_numeric", is_numeric);
    vm.register_function(b"intval", intval);
    vm.register_function(b"floatval", floatval);
    vm.register_function(b"doubleval", floatval);
    vm.register_function(b"strval", strval);
    vm.register_function(b"boolval", boolval);
    vm.register_function(b"settype", settype);
    vm.register_function(b"isset", php_isset);
    vm.register_function(b"empty", php_empty);
    vm.register_function(b"count", count);
    vm.register_function(b"sizeof", count);
    vm.register_function(b"is_scalar", is_scalar);
    vm.register_function(b"is_resource", is_resource);
    vm.register_function(b"is_countable", is_countable);
    vm.register_function(b"is_iterable", is_iterable);
    vm.register_function(b"get_debug_type", get_debug_type);
}

fn gettype(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let type_name = match val {
        Value::Undef | Value::Null => "NULL",
        Value::True | Value::False => "boolean",
        Value::Long(_) => "integer",
        Value::Double(_) => "double",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) | Value::Generator(_) => "object",
        Value::Reference(r) => return gettype(_vm, &[r.borrow().clone()]),
    };
    Ok(Value::String(PhpString::from_bytes(type_name.as_bytes())))
}

fn is_null(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(match args.first().unwrap_or(&Value::Null) {
        Value::Null | Value::Undef => Value::True,
        _ => Value::False,
    })
}

fn is_bool(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(match args.first().unwrap_or(&Value::Null) {
        Value::True | Value::False => Value::True,
        _ => Value::False,
    })
}

fn is_int(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(match args.first().unwrap_or(&Value::Null) {
        Value::Long(_) => Value::True,
        _ => Value::False,
    })
}

fn is_float(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(match args.first().unwrap_or(&Value::Null) {
        Value::Double(_) => Value::True,
        _ => Value::False,
    })
}

fn is_string(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(match args.first().unwrap_or(&Value::Null) {
        Value::String(_) => Value::True,
        _ => Value::False,
    })
}

fn is_array(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(match args.first().unwrap_or(&Value::Null) {
        Value::Array(_) => Value::True,
        _ => Value::False,
    })
}

fn is_numeric(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    Ok(match val {
        Value::Long(_) | Value::Double(_) => Value::True,
        Value::String(s) => {
            let s_str = s.to_string_lossy();
            let trimmed = s_str.trim();
            if trimmed.parse::<i64>().is_ok() || trimmed.parse::<f64>().is_ok() {
                Value::True
            } else {
                Value::False
            }
        }
        _ => Value::False,
    })
}

fn intval(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let base = args.get(1).map(|v| v.to_long()).unwrap_or(10);

    if base != 10
        && let Value::String(s) = val
    {
        let s_str = s.to_string_lossy();
        let trimmed = s_str.trim();
        let result = i64::from_str_radix(trimmed, base as u32).unwrap_or(0);
        return Ok(Value::Long(result));
    }

    Ok(Value::Long(val.to_long()))
}

fn floatval(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Double(
        args.first().unwrap_or(&Value::Null).to_double(),
    ))
}

fn strval(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(
        args.first().unwrap_or(&Value::Null).to_php_string(),
    ))
}

fn boolval(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(if args.first().unwrap_or(&Value::Null).is_truthy() {
        Value::True
    } else {
        Value::False
    })
}

fn settype(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // settype() modifies the variable in-place, which requires reference support.
    // For now, return true as a stub.
    Ok(Value::True)
}

fn php_isset(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    for arg in args {
        match arg {
            Value::Undef | Value::Null => return Ok(Value::False),
            _ => {}
        }
    }
    Ok(Value::True)
}

fn php_empty(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    Ok(if val.is_truthy() {
        Value::False
    } else {
        Value::True
    })
}

fn count(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    match val {
        Value::Array(arr) => Ok(Value::Long(arr.borrow().len() as i64)),
        Value::Null | Value::Undef => Ok(Value::Long(0)),
        _ => {
            // PHP 8: count() on non-array/Countable emits warning and returns 0
            // but for simplicity return 1 for scalars (matches common PHP behavior pre-8)
            Ok(Value::Long(1))
        }
    }
}

fn is_scalar(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    Ok(match val {
        Value::Long(_) | Value::Double(_) | Value::String(_) | Value::True | Value::False => {
            Value::True
        }
        Value::Reference(r) => {
            let inner = r.borrow();
            match &*inner {
                Value::Long(_)
                | Value::Double(_)
                | Value::String(_)
                | Value::True
                | Value::False => Value::True,
                _ => Value::False,
            }
        }
        _ => Value::False,
    })
}

fn is_resource(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // We don't have resource type, always false
    Ok(Value::False)
}

fn is_countable(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    Ok(match val {
        Value::Array(_) => Value::True,
        _ => Value::False,
    })
}

fn is_iterable(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    Ok(match val {
        Value::Array(_) | Value::Generator(_) => Value::True,
        _ => Value::False,
    })
}

fn get_debug_type(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    use goro_core::string::PhpString;
    let val = args.first().unwrap_or(&Value::Null);
    let type_name = match val {
        Value::Null | Value::Undef => "null",
        Value::True | Value::False => "bool",
        Value::Long(_) => "int",
        Value::Double(_) => "float",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(obj) => {
            let name = String::from_utf8_lossy(&obj.borrow().class_name).to_string();
            return Ok(Value::String(PhpString::from_string(name)));
        }
        Value::Generator(_) => "Generator",
        Value::Reference(r) => {
            let args = [r.borrow().clone()];
            return get_debug_type(_vm, &args);
        }
    };
    Ok(Value::String(goro_core::string::PhpString::from_bytes(
        type_name.as_bytes(),
    )))
}
