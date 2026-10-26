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
    vm.register_function(b"strval", strval);
    vm.register_function(b"boolval", boolval);
    vm.register_function(b"settype", settype);
    vm.register_function(b"isset", php_isset);
    vm.register_function(b"empty", php_empty);
    vm.register_function(b"count", count);
    vm.register_function(b"sizeof", sizeof);
    vm.register_function(b"is_scalar", is_scalar);
    vm.register_function(b"is_resource", is_resource);
    vm.register_function(b"is_countable", is_countable);
    vm.register_function(b"is_iterable", is_iterable);
    vm.register_function(b"get_debug_type", get_debug_type);
}

fn gettype(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    if val.is_resource() {
        let label = if goro_core::value::is_open_resource(val.resource_id()) {
            "resource"
        } else {
            "resource (closed)"
        };
        return Ok(Value::String(PhpString::from_bytes(label.as_bytes())));
    }
    let type_name = match val {
        Value::Undef | Value::Null => "NULL",
        Value::True | Value::False => "boolean",
        Value::Long(_) => "integer",
        Value::Double(_) => "double",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) | Value::Generator(_) => "object",
        Value::Reference(r) => return gettype(_vm, &[r.borrow().clone()]),
        Value::Resource(_) => unreachable!(),
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
    let val = args.first().unwrap_or(&Value::Null);
    Ok(match val {
        Value::String(_) => Value::True,
        Value::Reference(r) => {
            let inner = r.borrow();
            match &*inner {
                Value::String(_) => Value::True,
                _ => Value::False,
            }
        }
        _ => Value::False,
    })
}

fn is_array(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    Ok(match args.first().unwrap_or(&Value::Null) {
        Value::Array(_) => Value::True,
        _ => Value::False,
    })
}

fn strval(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    let derefed = val.deref();
    if matches!(&derefed, Value::Array(_)) {
        vm.emit_warning("Array to string conversion");
    }
    if let Value::Object(obj) = &derefed {
        if let Some(v) = goro_core::value::native_cast(obj, 3) {
            return Ok(Value::String(v.to_php_string()));
        }
    }
    // Object without a __toString hook is not convertible — PHP throws Error.
    if let Value::Object(obj) = &derefed {
        let class_name_bytes = obj.borrow().class_name.clone();
        let class_lower: Vec<u8> = class_name_bytes
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect();
        let has_to_string = {
            let mut found = false;
            let mut cur = Some(class_lower);
            while let Some(cname) = cur {
                if let Some(cls) = vm.classes.get(&cname) {
                    if cls.methods.contains_key(b"__tostring".as_slice()) {
                        found = true;
                        break;
                    }
                    cur = cls
                        .parent
                        .as_ref()
                        .map(|p| p.iter().map(|b| b.to_ascii_lowercase()).collect());
                } else {
                    break;
                }
            }
            found
        };
        if !has_to_string {
            let class_name = goro_core::value::display_class_name(&class_name_bytes);
            let msg = format!(
                "Object of class {} could not be converted to string",
                class_name
            );
            let exc = vm.create_exception(b"Error", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
        return Ok(Value::String(vm.value_to_string_checked(&derefed)?));
    }
    if matches!(derefed, Value::Double(d) if d.is_nan()) {
        // zend_double_to_str()
        vm.emit_warning("unexpected NAN value was coerced to string");
    }
    Ok(Value::String(val.to_php_string()))
}

fn boolval(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // (ZEND_BOOL: zend_is_true() warns for NAN)
    let line = vm.current_line;
    vm.warn_nan_coercion(args.first().unwrap_or(&Value::Null), "bool", line);
    Ok(if args.first().unwrap_or(&Value::Null).is_truthy() {
        Value::True
    } else {
        Value::False
    })
}

#[allow(clippy::blocks_in_conditions)] // scrutinee emits the NaN warning before matching
fn settype(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let var_ref = args.first().unwrap_or(&Value::Null);
    let type_name = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let type_str = type_name.to_string_lossy();

    if let Value::Reference(r) = var_ref {
        let current = r.borrow().clone();
        let new_val = match type_str.to_ascii_lowercase().as_str() {
            "int" | "integer" => {
                if let Value::Object(obj) = &current {
                    if goro_core::value::native_cast(obj, 1).is_none() {
                        let class_name =
                            goro_core::value::display_class_name(&obj.borrow().class_name);
                        _vm.emit_warning(&format!(
                            "Object of class {} could not be converted to int",
                            class_name
                        ));
                    }
                }
                let l = current.to_long();
                let line = _vm.current_line;
                _vm.warn_oob_float_to_int(&current, line);
                Value::Long(l)
            }
            "float" | "double" => {
                if let Value::Object(obj) = &current {
                    if goro_core::value::native_cast(obj, 2).is_none() {
                        let class_name =
                            goro_core::value::display_class_name(&obj.borrow().class_name);
                        _vm.emit_warning(&format!(
                            "Object of class {} could not be converted to float",
                            class_name
                        ));
                    }
                }
                Value::Double(current.to_double())
            }
            "string" => {
                if matches!(&current, Value::Array(_)) {
                    _vm.emit_warning("Array to string conversion");
                }
                if let Value::Object(_) = &current {
                    match _vm.value_to_string_checked(&current) {
                        Ok(s) => Value::String(s),
                        Err(e) => {
                            // convert_to_string() leaves an empty string behind on failure.
                            *r.borrow_mut() = Value::String(PhpString::empty());
                            return Err(e);
                        }
                    }
                } else {
                    let line = _vm.current_line;
                    _vm.warn_nan_coercion(&current, "string", line);
                    Value::String(current.to_php_string())
                }
            }
            "bool" | "boolean" => {
                let line = _vm.current_line;
                _vm.warn_nan_coercion(&current, "bool", line);
                if current.is_truthy() {
                    Value::True
                } else {
                    Value::False
                }
            }
            // convert_to_array() of a NAN wraps whatever the error handler
            // left in the variable.
            "array" if matches!(current, Value::Double(d) if d.is_nan()) => {
                let line = _vm.current_line;
                _vm.warn_nan_coercion(&current, "array", line);
                let mut arr = goro_core::array::PhpArray::new();
                arr.push(r.borrow().clone());
                Value::Array(std::rc::Rc::new(std::cell::RefCell::new(arr)))
            }
            "object" if matches!(current, Value::Double(d) if d.is_nan()) => {
                let line = _vm.current_line;
                _vm.warn_nan_coercion(&current, "object", line);
                let mut obj =
                    goro_core::object::PhpObject::new(b"stdClass".to_vec(), _vm.alloc_object_id());
                obj.set_property(b"scalar".to_vec(), r.borrow().clone());
                Value::Object(std::rc::Rc::new(std::cell::RefCell::new(obj)))
            }
            "array" => match {
                let line = _vm.current_line;
                _vm.warn_nan_coercion(&current, "array", line);
                if matches!(current, Value::Double(d) if d.is_nan()) {
                    r.borrow().clone()
                } else {
                    current
                }
            } {
                cur @ Value::Array(_) => cur,
                Value::Null | Value::Undef | Value::Generator(_) => Value::Array(std::rc::Rc::new(
                    std::cell::RefCell::new(goro_core::array::PhpArray::new()),
                )),
                Value::Object(ref o) => Value::Array(_vm.object_to_array(o)),
                other => {
                    let mut arr = goro_core::array::PhpArray::new();
                    arr.push(other);
                    Value::Array(std::rc::Rc::new(std::cell::RefCell::new(arr)))
                }
            },
            "object" => match {
                let line = _vm.current_line;
                _vm.warn_nan_coercion(&current, "object", line);
                if matches!(current, Value::Double(d) if d.is_nan()) {
                    r.borrow().clone()
                } else {
                    current
                }
            } {
                cur @ Value::Object(_) => cur,
                Value::Array(arr) => {
                    let mut obj = goro_core::object::PhpObject::new(
                        b"stdClass".to_vec(),
                        _vm.alloc_object_id(),
                    );
                    {
                        let arr_borrow = arr.borrow();
                        for (k, v) in arr_borrow.iter() {
                            let key_bytes = match k {
                                goro_core::array::ArrayKey::Int(i) => format!("{}", i).into_bytes(),
                                goro_core::array::ArrayKey::String(s) => s.as_bytes().to_vec(),
                            };
                            obj.set_property(key_bytes, v.clone());
                        }
                    }
                    Value::Object(std::rc::Rc::new(std::cell::RefCell::new(obj)))
                }
                cur => {
                    let mut obj = goro_core::object::PhpObject::new(
                        b"stdClass".to_vec(),
                        _vm.alloc_object_id(),
                    );
                    if !matches!(cur, Value::Null | Value::Undef) {
                        obj.set_property(b"scalar".to_vec(), cur);
                    }
                    Value::Object(std::rc::Rc::new(std::cell::RefCell::new(obj)))
                }
            },
            "null" => {
                let line = _vm.current_line;
                _vm.warn_nan_coercion(&current, "null", line);
                Value::Null
            }
            "resource" => {
                let msg = "Cannot convert to resource type";
                let exc = _vm.create_exception(b"ValueError", msg, _vm.current_line);
                _vm.current_exception = Some(exc);
                return Err(VmError {
                    message: format!("Uncaught ValueError: {}", msg),
                    line: _vm.current_line,
                });
            }
            _ => {
                let msg = "settype(): Argument #2 ($type) must be a valid type";
                let exc = _vm.create_exception(b"ValueError", msg, _vm.current_line);
                _vm.current_exception = Some(exc);
                return Err(VmError {
                    message: msg.to_string(),
                    line: _vm.current_line,
                });
            }
        };
        // ZEND_TRY_ASSIGN_REF_TMP(): typed references verify the new value.
        if _vm.try_assign_ref(r, new_val).is_err() {
            return Err(VmError {
                message: String::new(),
                line: _vm.current_line,
            });
        }
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

fn count(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    count_impl(vm, args, "count")
}

fn sizeof(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    count_impl(vm, args, "sizeof")
}

fn count_impl(vm: &mut Vm, args: &[Value], fname: &str) -> Result<Value, VmError> {
    let val_raw = args.first().unwrap_or(&Value::Null);
    // Dereference if a reference
    let val_owned;
    let val: &Value = match val_raw {
        Value::Reference(r) => {
            val_owned = r.borrow().clone();
            &val_owned
        }
        _ => val_raw,
    };
    let mode = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    // Validate mode: PHP 8+ throws ValueError for anything other than 0/1.
    // Note: bool TRUE casts to 1, FALSE casts to 0, so they're valid.
    if mode != 0 && mode != 1 {
        let msg = format!(
            "{}(): Argument #2 ($mode) must be either COUNT_NORMAL or COUNT_RECURSIVE",
            fname
        );
        let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    match val {
        Value::Array(arr) => {
            if mode == 1 {
                // COUNT_RECURSIVE
                Ok(Value::Long(count_recursive(vm, arr, &mut Vec::new())))
            } else {
                Ok(Value::Long(arr.borrow().len() as i64))
            }
        }
        Value::Object(obj) => {
            // Check for Countable interface - try to call count() method
            let class_lower: Vec<u8> = obj
                .borrow()
                .class_name
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();

            // First check if user class has overridden count() method
            let has_user_count = vm
                .classes
                .get(&class_lower)
                .map(|c| c.get_method(b"count").is_some())
                .unwrap_or(false);

            if has_user_count {
                // Call user-defined count() method; the result goes through
                // zval_get_long().
                let result = vm.call_object_method(val, b"count", &[]);
                if vm.current_exception.is_some() {
                    return Err(VmError {
                        message: "Uncaught exception".to_string(),
                        line: vm.current_line,
                    });
                }
                let n = match result.unwrap_or(Value::Null).deref() {
                    Value::Object(o) => {
                        let cn = goro_core::value::display_class_name(&o.borrow().class_name)
                            .to_string();
                        vm.emit_warning(&format!(
                            "Object of class {} could not be converted to int",
                            cn
                        ));
                        1
                    }
                    Value::Array(a) => {
                        if !a.borrow().is_empty() {
                            1
                        } else {
                            0
                        }
                    }
                    other => other.to_long(),
                };
                return Ok(Value::Long(n));
            }

            // Check if this is an SPL class with __spl_array
            let ob = obj.borrow();
            let spl_arr = ob.get_property(b"__spl_array");
            if let Value::Array(a) = spl_arr {
                return Ok(Value::Long(a.borrow().len() as i64));
            }
            drop(ob);

            // Try to call $obj->count() if it implements Countable (built-in SPL)
            if let Some(result) = vm.call_object_method(val, b"count", &[]) {
                return Ok(result);
            }

            // Non-countable object: TypeError in PHP 8+
            let class_name = String::from_utf8_lossy(&obj.borrow().class_name).into_owned();
            let err_msg = format!(
                "{}(): Argument #1 ($value) must be of type Countable|array, {} given",
                fname, class_name
            );
            let exc = vm.throw_type_error(err_msg.clone());
            vm.current_exception = Some(exc);
            Err(VmError {
                message: format!("Uncaught TypeError: {}", err_msg),
                line: vm.current_line,
            })
        }
        Value::Generator(_) => {
            let err_msg = format!(
                "{}(): Argument #1 ($value) must be of type Countable|array, Generator given",
                fname
            );
            let exc = vm.throw_type_error(err_msg.clone());
            vm.current_exception = Some(exc);
            Err(VmError {
                message: format!("Uncaught TypeError: {}", err_msg),
                line: vm.current_line,
            })
        }
        _ => {
            // null, bool, int, float, string -> TypeError in PHP 8+
            let type_name = Vm::value_type_name(val);
            let err_msg = format!(
                "{}(): Argument #1 ($value) must be of type Countable|array, {} given",
                fname, type_name
            );
            let exc = vm.throw_type_error(err_msg.clone());
            vm.current_exception = Some(exc);
            Err(VmError {
                message: format!("Uncaught TypeError: {}", err_msg),
                line: vm.current_line,
            })
        }
    }
}

/// php_count_recursive()
fn count_recursive(
    vm: &mut Vm,
    arr: &std::rc::Rc<std::cell::RefCell<goro_core::array::PhpArray>>,
    stack: &mut Vec<usize>,
) -> i64 {
    let ptr = std::rc::Rc::as_ptr(arr) as usize;
    if stack.contains(&ptr) {
        vm.emit_warning("count(): Recursion detected");
        return 0;
    }
    stack.push(ptr);
    let children: Vec<std::rc::Rc<std::cell::RefCell<goro_core::array::PhpArray>>> = arr
        .borrow()
        .values()
        .filter_map(|v| match v.deref() {
            Value::Array(a) => Some(a),
            _ => None,
        })
        .collect();
    let mut total = arr.borrow().len() as i64;
    for c in children {
        total += count_recursive(vm, &c, stack);
    }
    stack.pop();
    total
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

fn is_resource(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().unwrap_or(&Value::Null);
    // is_resource returns false for closed resources (matches PHP semantics)
    Ok(
        if val.is_resource() && goro_core::value::is_open_resource(val.resource_id()) {
            Value::True
        } else {
            Value::False
        },
    )
}

/// `$obj instanceof $target` for user and builtin classes/interfaces.
fn class_instanceof(vm: &Vm, class_lower: &[u8], target: &[u8]) -> bool {
    let mut cur = class_lower.to_vec();
    for _ in 0..64 {
        if cur == target
            || vm.class_implements_interface(&cur, target)
            || goro_core::vm::is_builtin_subclass(&cur, target)
        {
            return true;
        }
        let next = vm
            .classes
            .get(&cur)
            .and_then(|c| c.parent.as_ref())
            .map(|p| {
                p.iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect::<Vec<u8>>()
            });
        match next {
            Some(n) => cur = n,
            None => return false,
        }
    }
    false
}

fn is_countable(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    Ok(match &val {
        Value::Array(_) => Value::True,
        Value::Object(obj) => {
            let cl: Vec<u8> = obj
                .borrow()
                .class_name
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            if class_instanceof(vm, &cl, b"countable") {
                Value::True
            } else {
                Value::False
            }
        }
        _ => Value::False,
    })
}

fn is_iterable(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    Ok(match &val {
        Value::Array(_) | Value::Generator(_) => Value::True,
        Value::Object(obj) => {
            let cl: Vec<u8> = obj
                .borrow()
                .class_name
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            if class_instanceof(vm, &cl, b"traversable") {
                Value::True
            } else {
                Value::False
            }
        }
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
            // Anonymous classes: "Parent@anonymous" (the part before the NUL byte).
            let name = goro_core::value::display_class_name(&obj.borrow().class_name);
            return Ok(Value::String(PhpString::from_string(name)));
        }
        Value::Generator(_) => "Generator",
        Value::Reference(r) => {
            let args = [r.borrow().clone()];
            return get_debug_type(_vm, &args);
        }
        Value::Resource(id) => {
            let t = goro_core::value::resource_type_of(id.id())
                .map(|v| String::from_utf8_lossy(&v).to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            let s = if goro_core::value::is_open_resource(id.id()) {
                format!("resource ({})", t)
            } else {
                "resource (closed)".to_string()
            };
            return Ok(Value::String(PhpString::from_string(s)));
        }
    };
    Ok(Value::String(goro_core::string::PhpString::from_bytes(
        type_name.as_bytes(),
    )))
}
