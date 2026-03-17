use std::collections::HashSet;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

pub fn register(vm: &mut Vm) {
    vm.register_function(b"var_dump", var_dump);
    vm.register_function(b"print_r", print_r);
    vm.register_function(b"var_export", var_export);
}

fn var_dump(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let mut seen = HashSet::new();
    for arg in args {
        var_dump_value(vm, arg, 0, &mut seen);
    }
    Ok(Value::Null)
}

fn var_dump_value(vm: &mut Vm, val: &Value, indent: usize, seen: &mut HashSet<u64>) {
    if indent > 40 {
        return;
    }
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
            vm.write_output(
                format!("{}float({})\n", prefix, format_php_float_serialize(*f)).as_bytes(),
            );
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
                var_dump_value(vm, value, indent + 2, seen);
            }
            vm.write_output(format!("{}}}\n", prefix).as_bytes());
        }
        Value::Object(obj) => {
            let obj_borrow = obj.borrow();
            let class_name = String::from_utf8_lossy(&obj_borrow.class_name);
            let prop_count = obj_borrow.properties.len();
            let oid = obj_borrow.object_id;
            vm.write_output(
                format!(
                    "{}object({})#{} ({}) {{\n",
                    prefix, class_name, oid, prop_count
                )
                .as_bytes(),
            );

            // Check for recursion (self-referencing objects)
            if !seen.insert(oid) {
                // Already seen this object — print *RECURSION*
                vm.write_output(format!("{}  *RECURSION*\n", prefix).as_bytes());
                vm.write_output(format!("{}}}\n", prefix).as_bytes());
                return;
            }

            // Look up class to get property visibility info
            let class_lower: Vec<u8> = obj_borrow
                .class_name
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            let class_info = vm.classes.get(&class_lower).cloned();

            // Properties are in declaration order (Vec preserves insertion order)
            let props: Vec<_> = obj_borrow.properties.clone();
            let obj_class_name = obj_borrow.class_name.clone();
            drop(obj_borrow);
            for (name, value) in &props {
                let name_str = String::from_utf8_lossy(name);
                // Determine visibility
                let vis = class_info.as_ref().and_then(|c| {
                    c.properties
                        .iter()
                        .find(|p| p.name == *name)
                        .map(|p| p.visibility)
                });
                let display_name = match vis {
                    Some(goro_core::object::Visibility::Protected) => {
                        format!("\"{}\":protected", name_str)
                    }
                    Some(goro_core::object::Visibility::Private) => {
                        let class_name = String::from_utf8_lossy(&obj_class_name);
                        format!("\"{}\":\"{}\":private", name_str, class_name)
                    }
                    _ => format!("\"{}\"", name_str),
                };
                vm.write_output(format!("{}  [{}]=>\n", prefix, display_name).as_bytes());
                var_dump_value(vm, value, indent + 2, seen);
            }
            vm.write_output(format!("{}}}\n", prefix).as_bytes());
            seen.remove(&oid);
        }
        Value::Generator(_) => {
            vm.write_output(
                format!("{}object(Generator)#0 (0) {{\n{}}}\n", prefix, prefix).as_bytes(),
            );
        }
        Value::Reference(r) => {
            // Show the inner value with & prefix (PHP shows &int(42), &string(...), etc.)
            let inner = r.borrow().clone();
            var_dump_value_ref(vm, &inner, indent, &prefix, seen);
        }
    }
}

fn var_dump_value_ref(vm: &mut Vm, val: &Value, indent: usize, prefix: &str, seen: &mut HashSet<u64>) {
    match val {
        Value::Null | Value::Undef => {
            vm.write_output(format!("{}&NULL\n", prefix).as_bytes());
        }
        Value::True => {
            vm.write_output(format!("{}&bool(true)\n", prefix).as_bytes());
        }
        Value::False => {
            vm.write_output(format!("{}&bool(false)\n", prefix).as_bytes());
        }
        Value::Long(n) => {
            vm.write_output(format!("{}&int({})\n", prefix, n).as_bytes());
        }
        Value::Double(f) => {
            vm.write_output(
                format!("{}&float({})\n", prefix, format_php_float_serialize(*f)).as_bytes(),
            );
        }
        Value::String(s) => {
            vm.write_output(
                format!(
                    "{}&string({}) \"{}\"\n",
                    prefix,
                    s.len(),
                    s.to_string_lossy()
                )
                .as_bytes(),
            );
        }
        Value::Array(arr) => {
            let arr = arr.borrow();
            vm.write_output(format!("{}&array({}) {{\n", prefix, arr.len()).as_bytes());
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
                var_dump_value(vm, value, indent + 2, seen);
            }
            vm.write_output(format!("{}}}\n", prefix).as_bytes());
        }
        _ => {
            // For other types (Object, nested Reference), just dump normally
            var_dump_value(vm, val, indent, seen);
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
    if indent > 20 {
        buf.extend_from_slice(b" *RECURSION*");
        return;
    }
    match val {
        Value::Null | Value::Undef => buf.extend_from_slice(b""),
        Value::True => buf.extend_from_slice(b"1"),
        Value::False => {}
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
        Value::Object(obj) => {
            let obj_borrow = obj.borrow();
            let class_name = String::from_utf8_lossy(&obj_borrow.class_name);
            let prefix = " ".repeat(indent);
            buf.extend_from_slice(format!("{} Object\n", class_name).as_bytes());
            buf.extend_from_slice(format!("{}(\n", prefix).as_bytes());
            for (name, value) in &obj_borrow.properties {
                let name_str = String::from_utf8_lossy(name);
                buf.extend_from_slice(format!("{}    [{}] => ", prefix, name_str).as_bytes());
                print_r_value(value, buf, indent + 8);
                buf.push(b'\n');
            }
            buf.extend_from_slice(format!("{})\n", prefix).as_bytes());
        }
        Value::Generator(_) => {
            buf.extend_from_slice(b"Generator Object\n(\n)\n");
        }
        Value::Reference(r) => {
            print_r_value(&r.borrow(), buf, indent);
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

fn var_export_value(val: &Value, buf: &mut Vec<u8>, indent: usize) {
    let prefix = " ".repeat(indent);
    match val {
        Value::Null | Value::Undef => buf.extend_from_slice(b"NULL"),
        Value::True => buf.extend_from_slice(b"true"),
        Value::False => buf.extend_from_slice(b"false"),
        Value::Long(n) => buf.extend_from_slice(n.to_string().as_bytes()),
        Value::Double(f) => {
            // var_export uses a representation that can be parsed back
            let s = format_float(*f);
            buf.extend_from_slice(s.as_bytes());
            // Ensure there's a decimal point
            if !s.contains('.')
                && !s.contains('E')
                && !s.contains('e')
                && s != "INF"
                && s != "-INF"
                && s != "NAN"
            {
                buf.extend_from_slice(b".0");
            }
        }
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
        Value::Array(arr) => {
            let arr = arr.borrow();
            buf.extend_from_slice(b"array (\n");
            for (key, value) in arr.iter() {
                buf.extend_from_slice(format!("{}  ", prefix).as_bytes());
                match key {
                    goro_core::array::ArrayKey::Int(n) => {
                        buf.extend_from_slice(format!("{} => ", n).as_bytes());
                    }
                    goro_core::array::ArrayKey::String(s) => {
                        buf.extend_from_slice(b"'");
                        buf.extend_from_slice(s.as_bytes());
                        buf.extend_from_slice(b"' => ");
                    }
                }
                var_export_value(value, buf, indent + 2);
                buf.extend_from_slice(b",\n");
            }
            buf.extend_from_slice(format!("{})", prefix).as_bytes());
        }
        Value::Object(obj) => {
            let obj_borrow = obj.borrow();
            let class_name = String::from_utf8_lossy(&obj_borrow.class_name);
            buf.extend_from_slice(format!("{}::__set_state(array(\n", class_name).as_bytes());
            for (name, value) in &obj_borrow.properties {
                let name_str = String::from_utf8_lossy(name);
                buf.extend_from_slice(format!("{}   '{}' => ", prefix, name_str).as_bytes());
                var_export_value(value, buf, indent + 2);
                buf.extend_from_slice(b",\n");
            }
            buf.extend_from_slice(format!("{}))", prefix).as_bytes());
        }
        Value::Generator(_) => {
            buf.extend_from_slice(b"NULL");
        }
        Value::Reference(r) => {
            var_export_value(&r.borrow(), buf, indent);
        }
    }
}

/// Format a float using serialize_precision=-1 (shortest unique representation)
/// This is what PHP 8 uses for var_dump, var_export, json_encode, etc.
fn format_php_float_serialize(f: f64) -> String {
    if f.is_infinite() {
        return if f.is_sign_positive() {
            "INF".to_string()
        } else {
            "-INF".to_string()
        };
    }
    if f.is_nan() {
        return "NAN".to_string();
    }
    // PHP serialize_precision=-1: shortest exact representation
    // Use scientific notation for very large/small numbers
    let abs = f.abs();
    if abs != 0.0 && !(1e-4..1e15).contains(&abs) {
        // Use scientific notation like PHP
        let s = format!("{:e}", f);
        if let Some(pos) = s.find('e') {
            let mut mantissa = s[..pos].to_string();
            let exp: i32 = s[pos + 1..].parse().unwrap_or(0);
            // Ensure at least one decimal digit
            if !mantissa.contains('.') {
                mantissa.push_str(".0");
            } else if mantissa.ends_with('.') {
                mantissa.push('0');
            }
            if exp >= 0 {
                format!("{}E+{}", mantissa, exp)
            } else {
                format!("{}E{}", mantissa, exp)
            }
        } else {
            s
        }
    } else {
        // PHP serialize_precision=-1: shortest roundtrip representation
        // Use ryu-style formatting for exact roundtrip
        let mut buf = ryu_format(f);
        // Ensure no trailing dot
        if buf.ends_with('.') {
            buf.push('0');
        }
        buf
    }
}

/// Format float with shortest roundtrip representation (like PHP serialize_precision=-1)
fn ryu_format(f: f64) -> String {
    // Try increasing precision until roundtrip works
    for prec in 0..20 {
        let s = format!("{:.prec$}", f, prec = prec);
        if let Ok(parsed) = s.parse::<f64>() {
            if parsed == f {
                return s;
            }
        }
    }
    format!("{}", f)
}

fn format_float(f: f64) -> String {
    if f.is_infinite() {
        if f.is_sign_positive() {
            "INF".to_string()
        } else {
            "-INF".to_string()
        }
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
