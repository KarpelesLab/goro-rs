use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

/// Register all ctype extension functions
pub fn register(vm: &mut Vm) {
    vm.register_function(b"ctype_alpha", ctype_alpha);
    vm.register_function(b"ctype_digit", ctype_digit);
    vm.register_function(b"ctype_alnum", ctype_alnum);
    vm.register_function(b"ctype_upper", ctype_upper);
    vm.register_function(b"ctype_lower", ctype_lower);
    vm.register_function(b"ctype_space", ctype_space);
    vm.register_function(b"ctype_cntrl", ctype_cntrl);
    vm.register_function(b"ctype_graph", ctype_graph);
    vm.register_function(b"ctype_print", ctype_print);
    vm.register_function(b"ctype_punct", ctype_punct);
    vm.register_function(b"ctype_xdigit", ctype_xdigit);
}

/// Helper for ctype functions: get bytes to check (handles int as ASCII code)
fn ctype_get_bytes(args: &[Value]) -> Vec<u8> {
    let val = args.first().unwrap_or(&Value::Null);
    match val {
        Value::Long(n) => {
            if *n >= -128 && *n <= 255 {
                let c = if *n < 0 { (*n + 256) as u8 } else { *n as u8 };
                vec![c]
            } else {
                val.to_php_string().as_bytes().to_vec()
            }
        }
        _ => val.to_php_string().as_bytes().to_vec(),
    }
}

fn ctype_alpha(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_alphabetic()) {
            Value::True
        } else {
            Value::False
        },
    )
}
fn ctype_digit(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_digit()) {
            Value::True
        } else {
            Value::False
        },
    )
}
fn ctype_alnum(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_alphanumeric()) {
            Value::True
        } else {
            Value::False
        },
    )
}
fn ctype_upper(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_uppercase()) {
            Value::True
        } else {
            Value::False
        },
    )
}
fn ctype_lower(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_lowercase()) {
            Value::True
        } else {
            Value::False
        },
    )
}
fn ctype_space(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty()
            && bytes
                .iter()
                .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C))
        {
            Value::True
        } else {
            Value::False
        },
    )
}

fn ctype_cntrl(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_control()) {
            Value::True
        } else {
            Value::False
        },
    )
}

fn ctype_graph(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_graphic()) {
            Value::True
        } else {
            Value::False
        },
    )
}

fn ctype_print(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| *b >= 0x20 && *b <= 0x7e) {
            Value::True
        } else {
            Value::False
        },
    )
}

fn ctype_punct(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_punctuation()) {
            Value::True
        } else {
            Value::False
        },
    )
}

fn ctype_xdigit(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(args);
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_hexdigit()) {
            Value::True
        } else {
            Value::False
        },
    )
}
