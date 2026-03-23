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
/// Emits deprecation warning for non-string types
fn ctype_get_bytes(vm: &mut Vm, args: &[Value], func_name: &str) -> Vec<u8> {
    let val = args.first().unwrap_or(&Value::Null);
    match val {
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Long(n) => {
            vm.emit_deprecated_at(
                &format!("{func_name}(): Argument of type int will be interpreted as string in the future"),
                vm.current_line,
            );
            if *n >= -128 && *n <= 255 {
                let c = if *n < 0 { (*n + 256) as u8 } else { *n as u8 };
                vec![c]
            } else {
                val.to_php_string().as_bytes().to_vec()
            }
        }
        Value::Double(f) => {
            vm.emit_deprecated_at(
                &format!("{func_name}(): Argument of type float will be interpreted as string in the future"),
                vm.current_line,
            );
            // PHP treats float as (int)$float for ctype functions
            let n = *f as i64;
            if n >= -128 && n <= 255 {
                let c = if n < 0 { (n + 256) as u8 } else { n as u8 };
                vec![c]
            } else {
                val.to_php_string().as_bytes().to_vec()
            }
        }
        Value::Null | Value::Undef => {
            vm.emit_deprecated_at(
                &format!("{func_name}(): Argument of type null will be interpreted as string in the future"),
                vm.current_line,
            );
            val.to_php_string().as_bytes().to_vec()
        }
        Value::True => {
            vm.emit_deprecated_at(
                &format!("{func_name}(): Argument of type bool will be interpreted as string in the future"),
                vm.current_line,
            );
            // PHP treats bool as int (1/0) then as ASCII code
            vec![1u8]
        }
        Value::False => {
            vm.emit_deprecated_at(
                &format!("{func_name}(): Argument of type bool will be interpreted as string in the future"),
                vm.current_line,
            );
            // false = 0 (NUL byte)
            vec![0u8]
        }
        Value::Array(_) => {
            vm.emit_deprecated_at(
                &format!("{func_name}(): Argument of type array will be interpreted as string in the future"),
                vm.current_line,
            );
            // PHP returns false for array arguments
            vec![]
        }
        Value::Object(obj) => {
            let class_name = String::from_utf8_lossy(&obj.borrow().class_name).to_string();
            vm.emit_deprecated_at(
                &format!("{func_name}(): Argument of type {class_name} will be interpreted as string in the future"),
                vm.current_line,
            );
            // PHP returns false for object arguments
            vec![]
        }
        _ => val.to_php_string().as_bytes().to_vec(),
    }
}

fn ctype_alpha(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(vm, args, "ctype_alpha");
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_alphabetic()) {
            Value::True
        } else {
            Value::False
        },
    )
}
fn ctype_digit(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(vm, args, "ctype_digit");
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_digit()) {
            Value::True
        } else {
            Value::False
        },
    )
}
fn ctype_alnum(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(vm, args, "ctype_alnum");
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_alphanumeric()) {
            Value::True
        } else {
            Value::False
        },
    )
}
fn ctype_upper(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(vm, args, "ctype_upper");
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_uppercase()) {
            Value::True
        } else {
            Value::False
        },
    )
}
fn ctype_lower(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(vm, args, "ctype_lower");
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_lowercase()) {
            Value::True
        } else {
            Value::False
        },
    )
}
fn ctype_space(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(vm, args, "ctype_space");
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

fn ctype_cntrl(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(vm, args, "ctype_cntrl");
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_control()) {
            Value::True
        } else {
            Value::False
        },
    )
}

fn ctype_graph(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(vm, args, "ctype_graph");
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_graphic()) {
            Value::True
        } else {
            Value::False
        },
    )
}

fn ctype_print(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(vm, args, "ctype_print");
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| *b >= 0x20 && *b <= 0x7e) {
            Value::True
        } else {
            Value::False
        },
    )
}

fn ctype_punct(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(vm, args, "ctype_punct");
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_punctuation()) {
            Value::True
        } else {
            Value::False
        },
    )
}

fn ctype_xdigit(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let bytes = ctype_get_bytes(vm, args, "ctype_xdigit");
    Ok(
        if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_hexdigit()) {
            Value::True
        } else {
            Value::False
        },
    )
}
