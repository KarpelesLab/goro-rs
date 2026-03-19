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
            if php_is_numeric_string(s.as_bytes()) {
                Value::True
            } else {
                Value::False
            }
        }
        _ => Value::False,
    })
}

/// Check if a byte string is a valid PHP numeric string.
/// PHP allows leading whitespace but not trailing whitespace.
fn php_is_numeric_string(s: &[u8]) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut i = 0;
    // Skip leading whitespace
    while i < s.len() && (s[i] == b' ' || s[i] == b'\t' || s[i] == b'\n' || s[i] == b'\r' || s[i] == 0x0b || s[i] == 0x0c) {
        i += 1;
    }
    if i >= s.len() {
        return false;
    }
    // Optional sign
    if s[i] == b'+' || s[i] == b'-' {
        i += 1;
    }
    if i >= s.len() {
        return false;
    }
    let mut has_digits = false;
    // Integer part
    while i < s.len() && s[i].is_ascii_digit() {
        has_digits = true;
        i += 1;
    }
    // Decimal point and fractional part
    if i < s.len() && s[i] == b'.' {
        i += 1;
        while i < s.len() && s[i].is_ascii_digit() {
            has_digits = true;
            i += 1;
        }
    }
    if !has_digits {
        return false;
    }
    // Exponent part
    if i < s.len() && (s[i] == b'e' || s[i] == b'E') {
        i += 1;
        if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
            i += 1;
        }
        if i >= s.len() || !s[i].is_ascii_digit() {
            return false;
        }
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
        }
    }
    // Skip trailing whitespace (PHP allows it)
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    i == s.len()
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

fn settype(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let var_ref = args.first().unwrap_or(&Value::Null);
    let type_name = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let type_str = type_name.to_string_lossy();

    if let Value::Reference(r) = var_ref {
        let current = r.borrow().clone();
        let new_val = match type_str.to_ascii_lowercase().as_str() {
            "int" | "integer" => Value::Long(current.to_long()),
            "float" | "double" => Value::Double(current.to_double()),
            "string" => Value::String(current.to_php_string()),
            "bool" | "boolean" => {
                if current.is_truthy() {
                    Value::True
                } else {
                    Value::False
                }
            }
            "array" => match current {
                Value::Array(_) => current,
                Value::Null | Value::Undef => {
                    Value::Array(std::rc::Rc::new(std::cell::RefCell::new(
                        goro_core::array::PhpArray::new(),
                    )))
                }
                other => {
                    let mut arr = goro_core::array::PhpArray::new();
                    arr.push(other);
                    Value::Array(std::rc::Rc::new(std::cell::RefCell::new(arr)))
                }
            },
            "object" => match current {
                Value::Object(_) => current,
                Value::Array(arr) => {
                    let mut obj = goro_core::object::PhpObject::new(b"stdClass".to_vec(), 0);
                    {
                        let arr_borrow = arr.borrow();
                        for (k, v) in arr_borrow.iter() {
                            let key_bytes = match k {
                                goro_core::array::ArrayKey::Int(i) => {
                                    format!("{}", i).into_bytes()
                                }
                                goro_core::array::ArrayKey::String(s) => s.as_bytes().to_vec(),
                            };
                            obj.set_property(key_bytes, v.clone());
                        }
                    }
                    Value::Object(std::rc::Rc::new(std::cell::RefCell::new(obj)))
                }
                _ => {
                    let mut obj = goro_core::object::PhpObject::new(b"stdClass".to_vec(), 0);
                    if !matches!(current, Value::Null | Value::Undef) {
                        obj.set_property(b"scalar".to_vec(), current);
                    }
                    Value::Object(std::rc::Rc::new(std::cell::RefCell::new(obj)))
                }
            },
            "null" => Value::Null,
            _ => return Ok(Value::False),
        };
        *r.borrow_mut() = new_val;
        Ok(Value::True)
    } else {
        // Not a reference - can't modify
        Ok(Value::True)
    }
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
    let mode = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    match val {
        Value::Array(arr) => {
            if mode == 1 {
                // COUNT_RECURSIVE
                Ok(Value::Long(count_recursive(&Value::Array(arr.clone()))))
            } else {
                Ok(Value::Long(arr.borrow().len() as i64))
            }
        }
        Value::Object(obj) => {
            // Check for Countable interface / __spl_array property
            let ob = obj.borrow();
            let spl_arr = ob.get_property(b"__spl_array");
            if let Value::Array(a) = spl_arr {
                Ok(Value::Long(a.borrow().len() as i64))
            } else {
                // Non-countable object - return property count or 1
                Ok(Value::Long(1))
            }
        }
        Value::Null | Value::Undef => Ok(Value::Long(0)),
        _ => {
            Ok(Value::Long(1))
        }
    }
}

fn count_recursive(val: &Value) -> i64 {
    match val {
        Value::Array(arr) => {
            let arr = arr.borrow();
            let mut total = arr.len() as i64;
            for (_, v) in arr.iter() {
                if let Value::Array(_) = v {
                    total += count_recursive(v);
                }
            }
            total
        }
        _ => 1,
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
