use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

/// Register all ctype extension functions
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"ctype");
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
    // ctype_impl() parses exactly one zval (Z_PARAM_ZVAL): the arginfo's
    // argument count holds.
    for name in [
        "ctype_alpha",
        "ctype_digit",
        "ctype_alnum",
        "ctype_upper",
        "ctype_lower",
        "ctype_space",
        "ctype_cntrl",
        "ctype_graph",
        "ctype_print",
        "ctype_punct",
        "ctype_xdigit",
    ] {
        vm.zpp_extension_verified
            .insert(name.as_bytes().to_vec(), goro_core::arginfo::CHECK_ARITY);
    }
}

/// Returns the PHP type name used in error messages
fn php_type_name(val: &Value) -> String {
    match val {
        Value::Null | Value::Undef => "null".to_string(),
        Value::True | Value::False => "bool".to_string(),
        Value::Long(_) => "int".to_string(),
        Value::Double(_) => "float".to_string(),
        Value::String(_) => "string".to_string(),
        Value::Array(_) => "array".to_string(),
        Value::Resource(_) => "resource".to_string(),
        Value::Object(obj) => String::from_utf8_lossy(&obj.borrow().class_name).to_string(),
        Value::Generator(_) => "Generator".to_string(),
        Value::Reference(r) => php_type_name(&r.borrow()),
    }
}

/// Run a ctype check on a value matching PHP semantics.
/// `iswhat` is a byte predicate (taking the byte and returning bool).
/// `allow_digits` is returned for ints > 255.
/// `allow_minus` is returned for ints < -128.
fn ctype_check<F: Fn(u8) -> bool>(
    vm: &mut Vm,
    args: &[Value],
    func_name: &str,
    iswhat: F,
    allow_digits: bool,
    allow_minus: bool,
) -> Value {
    let val = match args.first() {
        Some(v) => v.clone(),
        None => return Value::False,
    };
    // Dereference if needed
    let val = match &val {
        Value::Reference(r) => r.borrow().clone(),
        _ => val,
    };

    match &val {
        Value::String(s) => {
            let bytes = s.as_bytes();
            if bytes.is_empty() {
                Value::False
            } else if bytes.iter().all(|&b| iswhat(b)) {
                Value::True
            } else {
                Value::False
            }
        }
        Value::Long(n) => {
            // Emit deprecation
            vm.emit_deprecated_at(
                &format!(
                    "{func_name}(): Argument of type int will be interpreted as string in the future"
                ),
                vm.current_line,
            );
            let n = *n;
            if (0..=255).contains(&n) {
                if iswhat(n as u8) {
                    Value::True
                } else {
                    Value::False
                }
            } else if (-128..0).contains(&n) {
                let b = (n + 256) as u8;
                if iswhat(b) { Value::True } else { Value::False }
            } else if n > 255 {
                if allow_digits {
                    Value::True
                } else {
                    Value::False
                }
            } else {
                // n < -128
                if allow_minus {
                    Value::True
                } else {
                    Value::False
                }
            }
        }
        _ => {
            // Emit deprecation for non-string, non-int types
            let type_name = php_type_name(&val);
            vm.emit_deprecated_at(
                &format!(
                    "{func_name}(): Argument of type {type_name} will be interpreted as string in the future"
                ),
                vm.current_line,
            );
            Value::False
        }
    }
}

fn ctype_alpha(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(ctype_check(
        vm,
        args,
        "ctype_alpha",
        |b| unsafe { libc::isalpha(b as i32) != 0 },
        false,
        false,
    ))
}
fn ctype_digit(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(ctype_check(
        vm,
        args,
        "ctype_digit",
        |b| unsafe { libc::isdigit(b as i32) != 0 },
        true,
        false,
    ))
}
fn ctype_alnum(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(ctype_check(
        vm,
        args,
        "ctype_alnum",
        |b| unsafe { libc::isalnum(b as i32) != 0 },
        true,
        false,
    ))
}
fn ctype_upper(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(ctype_check(
        vm,
        args,
        "ctype_upper",
        |b| unsafe { libc::isupper(b as i32) != 0 },
        false,
        false,
    ))
}
fn ctype_lower(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(ctype_check(
        vm,
        args,
        "ctype_lower",
        |b| unsafe { libc::islower(b as i32) != 0 },
        false,
        false,
    ))
}
fn ctype_space(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(ctype_check(
        vm,
        args,
        "ctype_space",
        |b| unsafe { libc::isspace(b as i32) != 0 },
        false,
        false,
    ))
}
fn ctype_cntrl(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(ctype_check(
        vm,
        args,
        "ctype_cntrl",
        |b| unsafe { libc::iscntrl(b as i32) != 0 },
        false,
        false,
    ))
}
fn ctype_graph(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(ctype_check(
        vm,
        args,
        "ctype_graph",
        |b| unsafe { libc::isgraph(b as i32) != 0 },
        true,
        true,
    ))
}
fn ctype_print(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(ctype_check(
        vm,
        args,
        "ctype_print",
        |b| unsafe { libc::isprint(b as i32) != 0 },
        true,
        true,
    ))
}
fn ctype_punct(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(ctype_check(
        vm,
        args,
        "ctype_punct",
        |b| unsafe { libc::ispunct(b as i32) != 0 },
        false,
        false,
    ))
}
fn ctype_xdigit(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(ctype_check(
        vm,
        args,
        "ctype_xdigit",
        |b| unsafe { libc::isxdigit(b as i32) != 0 },
        true,
        false,
    ))
}
