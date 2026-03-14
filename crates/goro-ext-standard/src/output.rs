use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{BuiltinFn, Vm, VmError};

pub fn register(vm: &mut Vm) {
    vm.register_function(b"var_dump", var_dump);
    vm.register_function(b"print_r", print_r);
    vm.register_function(b"var_export", var_export);
}

fn var_dump(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    for arg in args {
        var_dump_value(vm, arg, 0);
    }
    Ok(Value::Null)
}

fn var_dump_value(vm: &mut Vm, val: &Value, indent: usize) {
    let prefix = " ".repeat(indent);
    match val {
        Value::Null | Value::Undef => {
            vm.write_output(format!("{}NULL\n", prefix).as_bytes());
        }
        Value::True => {
            vm.write_output(format!("{}bool(true)\n", prefix).as_bytes());
        }
        Value::False => {
            vm.write_output(format!("{}bool(false)\n", prefix).as_bytes());
        }
        Value::Long(n) => {
            vm.write_output(format!("{}int({})\n", prefix, n).as_bytes());
        }
        Value::Double(f) => {
            // var_dump uses serialize_precision (-1 in PHP 8 = shortest representation)
            vm.write_output(format!("{}float({})\n", prefix, format_php_float_serialize(*f)).as_bytes());
        }
        Value::String(s) => {
            vm.write_output(
                format!(
                    "{}string({}) \"{}\"\n",
                    prefix,
                    s.len(),
                    s.to_string_lossy()
                )
                .as_bytes(),
            );
        }
        Value::Array(arr) => {
            let arr = arr.borrow();
            vm.write_output(format!("{}array({}) {{\n", prefix, arr.len()).as_bytes());
            for (key, value) in arr.iter() {
                match key {
                    goro_core::array::ArrayKey::Int(n) => {
                        vm.write_output(format!("{}  [{}]=>\n", prefix, n).as_bytes());
                    }
                    goro_core::array::ArrayKey::String(s) => {
                        vm.write_output(
                            format!("{}  [\"{}\"]=>\n", prefix, s.to_string_lossy()).as_bytes(),
                        );
                    }
                }
                var_dump_value(vm, value, indent + 2);
            }
            vm.write_output(format!("{}}}\n", prefix).as_bytes());
        }
        Value::Object(obj) => {
            let obj_borrow = obj.borrow();
            let class_name = String::from_utf8_lossy(&obj_borrow.class_name);
            let prop_count = obj_borrow.properties.len();
            vm.write_output(
                format!("{}object({})#{} ({}) {{\n", prefix, class_name, obj_borrow.object_id, prop_count).as_bytes(),
            );
            // Sort properties for consistent output
            let mut props: Vec<_> = obj_borrow.properties.iter().collect();
            props.sort_by(|(a, _), (b, _)| a.cmp(b));
            for (name, value) in &props {
                let name_str = String::from_utf8_lossy(name);
                vm.write_output(format!("{}  [\"{}\"]=>", prefix, name_str).as_bytes());
                vm.write_output(b"\n");
                var_dump_value(vm, value, indent + 2);
            }
            vm.write_output(format!("{}}}\n", prefix).as_bytes());
        }
    }
}

fn print_r(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Err(VmError {
            message: "print_r() expects at least 1 argument".into(),
            line: 0,
        });
    }

    let return_output = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);

    if return_output {
        let mut buf = Vec::new();
        print_r_value(&args[0], &mut buf, 0);
        Ok(Value::String(PhpString::from_vec(buf)))
    } else {
        let mut buf = Vec::new();
        print_r_value(&args[0], &mut buf, 0);
        vm.write_output(&buf);
        Ok(Value::True)
    }
}

fn print_r_value(val: &Value, buf: &mut Vec<u8>, indent: usize) {
    match val {
        Value::Null | Value::Undef => buf.extend_from_slice(b""),
        Value::True => buf.extend_from_slice(b"1"),
        Value::False => {},
        Value::Long(n) => buf.extend_from_slice(n.to_string().as_bytes()),
        Value::Double(f) => buf.extend_from_slice(format_float(*f).as_bytes()),
        Value::String(s) => buf.extend_from_slice(s.as_bytes()),
        Value::Array(arr) => {
            let arr = arr.borrow();
            let prefix = " ".repeat(indent);
            buf.extend_from_slice(b"Array\n");
            buf.extend_from_slice(format!("{}(\n", prefix).as_bytes());
            for (key, value) in arr.iter() {
                match key {
                    goro_core::array::ArrayKey::Int(n) => {
                        buf.extend_from_slice(format!("{}    [{}] => ", prefix, n).as_bytes());
                    }
                    goro_core::array::ArrayKey::String(s) => {
                        buf.extend_from_slice(
                            format!("{}    [{}] => ", prefix, s.to_string_lossy()).as_bytes(),
                        );
                    }
                }
                print_r_value(value, buf, indent + 8);
                buf.push(b'\n');
            }
            buf.extend_from_slice(format!("{})\n", prefix).as_bytes());
        }
        Value::Object(_) => {
            buf.extend_from_slice(b"Object"); // TODO: implement object print_r
        }
    }
}

fn var_export(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Err(VmError {
            message: "var_export() expects at least 1 argument".into(),
            line: 0,
        });
    }
    let return_output = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let mut buf = Vec::new();
    var_export_value(&args[0], &mut buf, 0);

    if return_output {
        Ok(Value::String(PhpString::from_vec(buf)))
    } else {
        vm.write_output(&buf);
        Ok(Value::Null)
    }
}

fn var_export_value(val: &Value, buf: &mut Vec<u8>, _indent: usize) {
    match val {
        Value::Null | Value::Undef => buf.extend_from_slice(b"NULL"),
        Value::True => buf.extend_from_slice(b"true"),
        Value::False => buf.extend_from_slice(b"false"),
        Value::Long(n) => buf.extend_from_slice(n.to_string().as_bytes()),
        Value::Double(f) => buf.extend_from_slice(format_float(*f).as_bytes()),
        Value::String(s) => {
            buf.push(b'\'');
            for &byte in s.as_bytes() {
                match byte {
                    b'\'' => buf.extend_from_slice(b"\\'"),
                    b'\\' => buf.extend_from_slice(b"\\\\"),
                    _ => buf.push(byte),
                }
            }
            buf.push(b'\'');
        }
        Value::Array(_) => {
            buf.extend_from_slice(b"array (...)"); // simplified
        }
        Value::Object(_) => {
            buf.extend_from_slice(b"(object) array(...)"); // TODO: implement object var_export
        }
    }
}

/// Format a float using serialize_precision=-1 (shortest unique representation)
/// This is what PHP 8 uses for var_dump, var_export, json_encode, etc.
fn format_php_float_serialize(f: f64) -> String {
    if f.is_infinite() {
        return if f.is_sign_positive() { "INF".to_string() } else { "-INF".to_string() };
    }
    if f.is_nan() {
        return "NAN".to_string();
    }
    // Rust's Display for f64 already produces the shortest exact representation
    format!("{}", f)
}

fn format_float(f: f64) -> String {
    if f.is_infinite() {
        if f.is_sign_positive() { "INF".to_string() } else { "-INF".to_string() }
    } else if f.is_nan() {
        "NAN".to_string()
    } else {
        // PHP default precision is 14
        let s = format!("{:.14}", f);
        // Trim trailing zeros but keep at least one decimal
        let trimmed = s.trim_end_matches('0');
        if trimmed.ends_with('.') {
            trimmed.to_string()
        } else {
            trimmed.to_string()
        }
    }
}
