// Reflection API implementation for goro-rs
// Extracted from vm.rs - provides PHP Reflection classes

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::{PhpObject, RuntimeAttribute, Visibility};
use crate::opcode::{OpArray, ParamType};
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::Vm;

/// Validate that the user-supplied argument count is `expected`.
/// `args` is the raw arg slice (includes $this at args[0]).
/// Throws ArgumentCountError on mismatch and returns false.
/// Returns true if the count is correct.
pub(crate) fn check_arg_count(
    vm: &mut Vm,
    args: &[Value],
    expected: usize,
    method_label: &str,
) -> bool {
    let user_arg_count = args.len().saturating_sub(1);
    if user_arg_count != expected {
        let plural = if expected == 1 { "argument" } else { "arguments" };
        let err = format!(
            "{}() expects exactly {} {}, {} given",
            method_label, expected, plural, user_arg_count
        );
        let exc = vm.create_exception(b"ArgumentCountError", &err, vm.current_line);
        vm.current_exception = Some(exc);
        return false;
    }
    true
}

/// Validate user-supplied arg count is in [min, max] range.
/// Returns true if the count is within bounds.
pub(crate) fn check_arg_count_range(
    vm: &mut Vm,
    args: &[Value],
    min: usize,
    max: usize,
    method_label: &str,
) -> bool {
    let user_arg_count = args.len().saturating_sub(1);
    if user_arg_count < min || user_arg_count > max {
        let err = if min == max {
            let plural = if min == 1 { "argument" } else { "arguments" };
            format!("{}() expects exactly {} {}, {} given", method_label, min, plural, user_arg_count)
        } else if user_arg_count < min {
            let plural = if min == 1 { "argument" } else { "arguments" };
            format!("{}() expects at least {} {}, {} given", method_label, min, plural, user_arg_count)
        } else {
            let plural = if max == 1 { "argument" } else { "arguments" };
            format!("{}() expects at most {} {}, {} given", method_label, max, plural, user_arg_count)
        };
        let exc = vm.create_exception(b"ArgumentCountError", &err, vm.current_line);
        vm.current_exception = Some(exc);
        return false;
    }
    true
}

/// Validate that arg at `arg_index` (0-based, where 0 is the first user arg)
/// is a string (or stringable), or null with deprecation.
/// Returns Some(stringified) on success, None if exception was thrown.
pub(crate) fn require_string_arg(
    vm: &mut Vm,
    args: &[Value],
    arg_index: usize,
    method_label: &str,
    param_name: &str,
    type_descriptor: &str,
    allow_null_deprecated: bool,
) -> Option<String> {
    let arg = args.get(arg_index + 1)?;
    match arg {
        Value::String(s) => Some(s.to_string_lossy()),
        Value::Long(n) => Some(n.to_string()),
        Value::Double(d) => {
            // Match PHP's number formatting for floats
            Some(arg.to_php_string().to_string_lossy())
        }
        Value::True => Some("1".to_string()),
        Value::False => Some("".to_string()),
        Value::Null => {
            if allow_null_deprecated {
                vm.emit_deprecated_at(
                    &format!(
                        "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
                        method_label, arg_index + 1, param_name, type_descriptor
                    ),
                    vm.current_line,
                );
                Some("".to_string())
            } else {
                let err = format!(
                    "{}(): Argument #{} (${}) must be of type {}, null given",
                    method_label, arg_index + 1, param_name, type_descriptor
                );
                let exc = vm.create_exception(b"TypeError", &err, vm.current_line);
                vm.current_exception = Some(exc);
                None
            }
        }
        Value::Array(_) => {
            let err = format!(
                "{}(): Argument #{} (${}) must be of type {}, array given",
                method_label, arg_index + 1, param_name, type_descriptor
            );
            let exc = vm.create_exception(b"TypeError", &err, vm.current_line);
            vm.current_exception = Some(exc);
            None
        }
        Value::Object(o) => {
            let class_name = String::from_utf8_lossy(&o.borrow().class_name).to_string();
            let err = format!(
                "{}(): Argument #{} (${}) must be of type {}, {} given",
                method_label, arg_index + 1, param_name, type_descriptor, class_name
            );
            let exc = vm.create_exception(b"TypeError", &err, vm.current_line);
            vm.current_exception = Some(exc);
            None
        }
        _ => Some(arg.to_php_string().to_string_lossy()),
    }
}


/// ReflectionClass constructor: sets up __reflection_target and name properties
pub fn reflection_class_construct(vm: &mut Vm, args: &[Value], line: u32) -> bool {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return true,
    };
    // Determine which reflector class we're constructing for error messages.
    let reflector_class_name: String = {
        let ob = this.borrow();
        String::from_utf8_lossy(&ob.class_name).to_string()
    };

    // User-provided args (args[0] is $this)
    let user_arg_count = args.len().saturating_sub(1);
    if user_arg_count == 0 {
        let err_msg = format!(
            "{}::__construct() expects exactly 1 argument, 0 given",
            reflector_class_name
        );
        let exc = vm.create_exception(b"ArgumentCountError", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }
    if user_arg_count > 1 {
        let err_msg = format!(
            "{}::__construct() expects exactly 1 argument, {} given",
            reflector_class_name, user_arg_count
        );
        let exc = vm.create_exception(b"ArgumentCountError", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }

    let arg = args.get(1).cloned().unwrap_or(Value::Null);

    // Reject array/non-stringable arguments before we try to stringify.
    match &arg {
        Value::Array(_) => {
            let err_msg = format!(
                "{}::__construct(): Argument #1 ($objectOrClass) must be of type object|string, array given",
                reflector_class_name
            );
            let exc = vm.create_exception(b"TypeError", &err_msg, line);
            vm.current_exception = Some(exc);
            return false;
        }
        Value::Null => {
            // Deprecated warning, then proceed with null -> empty string (-> "Class \"\" does not exist")
            vm.emit_deprecated_at(
                &format!("{}::__construct(): Passing null to parameter #1 ($objectOrClass) of type object|string is deprecated",
                    reflector_class_name), line);
        }
        _ => {}
    }

    let class_name = match &arg {
        Value::Object(obj) => {
            let ob = obj.borrow();
            String::from_utf8_lossy(&ob.class_name).to_string()
        }
        Value::String(s) => {
            let name = s.to_string_lossy();
            // Closure strings should map to "Closure" class
            if name.starts_with("__closure_") || name.starts_with("__arrow_") || name.starts_with("__bound_closure_") || name.starts_with("__closure_fcc_") {
                "Closure".to_string()
            } else {
                name
            }
        }
        _ => arg.to_php_string().to_string_lossy(),
    };

    // Strip leading backslash
    let class_name = if class_name.starts_with('\\') {
        class_name[1..].to_string()
    } else {
        class_name
    };
    let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

    // Check if class exists
    let mut class_exists = vm.classes.contains_key(&class_lower)
        || vm.is_known_builtin_class(&class_lower);

    // If class is unknown, try autoloading
    if !class_exists {
        vm.try_autoload_class(class_name.as_bytes());
        class_exists = vm.classes.contains_key(&class_lower)
            || vm.is_known_builtin_class(&class_lower);
    }

    if !class_exists {
        let err_msg = format!("Class \"{}\" does not exist", class_name);
        let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }

    // ReflectionEnum requires the target class actually be an enum.
    if reflector_class_name.eq_ignore_ascii_case("ReflectionEnum") {
        let is_enum = vm.classes.get(&class_lower).map(|c| c.is_enum).unwrap_or(false);
        if !is_enum {
            let canonical = vm.classes.get(&class_lower)
                .map(|c| String::from_utf8_lossy(&c.name).to_string())
                .unwrap_or(class_name.clone());
            let err_msg = format!("Class \"{}\" is not an enum", canonical);
            let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
            vm.current_exception = Some(exc);
            return false;
        }
    }

    // Get canonical name
    let canonical = if let Some(ce) = vm.classes.get(&class_lower) {
        String::from_utf8_lossy(&ce.name).to_string()
    } else {
        // Built-in class - use proper casing
        vm.builtin_canonical_name(&class_lower)
    };

    let mut obj = this.borrow_mut();
    obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(canonical.clone())));
    obj.set_property(b"__reflection_target".to_vec(), Value::String(PhpString::from_string(canonical)));
    // If constructed from an object, store the object reference
    if let Value::Object(_) = &arg {
        obj.set_property(b"__reflection_object".to_vec(), arg);
    } else if let Value::String(s) = &arg {
        // Closure represented as a string: remember the underlying function.
        let name = s.to_string_lossy();
        if name.starts_with("__closure_") || name.starts_with("__arrow_")
            || name.starts_with("__bound_closure_") || name.starts_with("__closure_fcc_")
        {
            obj.set_property(b"__reflection_closure_fn".to_vec(),
                Value::String(s.clone()));
        }
    } else if let Value::Array(arr) = &arg {
        // Closure stored as [func_name, $this, ...]
        let first = arr.borrow().iter().next().map(|(_, v)| v.clone());
        if let Some(Value::String(s)) = first {
            let name = s.to_string_lossy();
            if name.starts_with("__closure_") || name.starts_with("__arrow_")
                || name.starts_with("__bound_closure_") || name.starts_with("__closure_fcc_")
            {
                obj.set_property(b"__reflection_closure_fn".to_vec(),
                    Value::String(s));
            }
        }
    }
    true
}

/// ReflectionMethod constructor
pub fn reflection_method_construct(vm: &mut Vm, args: &[Value], line: u32) -> bool {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return true,
    };

    // Validate arg count (1 or 2 user args expected).
    let user_arg_count = args.len().saturating_sub(1);
    if user_arg_count == 0 {
        let err = "ReflectionMethod::__construct() expects at least 1 argument, 0 given".to_string();
        let exc = vm.create_exception(b"ArgumentCountError", &err, line);
        vm.current_exception = Some(exc);
        return false;
    }
    if user_arg_count > 2 {
        let err = format!("ReflectionMethod::__construct() expects at most 2 arguments, {} given", user_arg_count);
        let exc = vm.create_exception(b"ArgumentCountError", &err, line);
        vm.current_exception = Some(exc);
        return false;
    }

    // Extract the closure's underlying function name, if any, from the
    // first argument. Supports string-form closures and array callables of
    // the form [closure_obj, 'method'] that flatten to a string closure.
    let mut closure_fn: Option<PhpString> = None;
    if let Some(first) = args.get(1) {
        match first {
            Value::String(s) => {
                let name = s.to_string_lossy();
                if name.starts_with("__closure_") || name.starts_with("__arrow_")
                    || name.starts_with("__bound_closure_") || name.starts_with("__closure_fcc_")
                {
                    closure_fn = Some(s.clone());
                }
            }
            Value::Array(arr) => {
                let af = arr.borrow().iter().next().map(|(_, v)| v.clone());
                if let Some(Value::String(s)) = af {
                    let name = s.to_string_lossy();
                    if name.starts_with("__closure_") || name.starts_with("__arrow_")
                        || name.starts_with("__bound_closure_") || name.starts_with("__closure_fcc_")
                    {
                        closure_fn = Some(s);
                    }
                }
            }
            _ => {}
        }
    }

    let (class_name, method_name) = if args.len() >= 3 {
        // new ReflectionMethod($class, $method)
        let class_arg = &args[1];
        let method_arg = args[2].to_php_string().to_string_lossy();
        let class_str = match class_arg {
            Value::Object(obj) => {
                let ob = obj.borrow();
                String::from_utf8_lossy(&ob.class_name).to_string()
            }
            Value::String(s) => {
                let name = s.to_string_lossy();
                // Closure strings map to "Closure" class
                if name.starts_with("__closure_") || name.starts_with("__arrow_") || name.starts_with("__bound_closure_") || name.starts_with("__closure_fcc_") {
                    "Closure".to_string()
                } else {
                    name
                }
            }
            Value::Array(arr) => {
                // Array callable: ['Class', 'method'] or [$obj, 'method']
                let first = arr.borrow().iter().next().map(|(_, v)| v.clone());
                match first {
                    Some(Value::Object(o)) => String::from_utf8_lossy(&o.borrow().class_name).to_string(),
                    Some(Value::String(s)) => {
                        let name = s.to_string_lossy();
                        if name.starts_with("__closure_") || name.starts_with("__arrow_")
                            || name.starts_with("__bound_closure_") || name.starts_with("__closure_fcc_")
                        {
                            "Closure".to_string()
                        } else { name }
                    }
                    _ => class_arg.to_php_string().to_string_lossy(),
                }
            }
            _ => class_arg.to_php_string().to_string_lossy(),
        };
        (class_str, method_arg)
    } else if args.len() >= 2 {
        // new ReflectionMethod('Class::method')
        // 1-arg form is deprecated in PHP 8.4 in favour of createFromMethodName.
        let dep_msg = "Calling ReflectionMethod::__construct() with 1 argument is deprecated, use ReflectionMethod::createFromMethodName() instead";
        vm.emit_deprecated_raw(dep_msg, line);
        let arg = args[1].to_php_string().to_string_lossy();
        if let Some(pos) = arg.find("::") {
            (arg[..pos].to_string(), arg[pos + 2..].to_string())
        } else {
            let err_msg = format!(
                "ReflectionMethod::__construct(): Argument #1 ($objectOrMethod) must be a valid method name"
            );
            let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
            vm.current_exception = Some(exc);
            return false;
        }
    } else {
        return true;
    };

    let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    let method_lower: Vec<u8> = method_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

    // Check if class exists (user-defined or built-in)
    let is_user_class = vm.classes.contains_key(&class_lower);
    let is_builtin_class = vm.is_known_builtin_class(&class_lower);
    if !is_user_class && !is_builtin_class {
        let err_msg = format!("Class \"{}\" does not exist", class_name);
        let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }

    if is_user_class {
        // Check method exists in user class
        let method_exists = vm.classes.get(&class_lower)
            .map(|c| c.get_method(&method_lower).is_some())
            .unwrap_or(false);

        if !method_exists {
            let canonical_class = vm.classes.get(&class_lower)
                .map(|c| String::from_utf8_lossy(&c.name).to_string())
                .unwrap_or(class_name.clone());
            let err_msg = format!("Method {}::{}() does not exist", canonical_class, method_name);
            let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
            vm.current_exception = Some(exc);
            return false;
        }
    }
    // For built-in classes, we accept all method names without checking

    let canonical_class = if is_user_class {
        vm.classes.get(&class_lower)
            .map(|c| String::from_utf8_lossy(&c.name).to_string())
            .unwrap_or(class_name.clone())
    } else {
        vm.builtin_canonical_name(&class_lower)
    };

    let canonical_method = if is_user_class {
        vm.classes.get(&class_lower)
            .and_then(|c| c.get_method(&method_lower).map(|m| String::from_utf8_lossy(&m.name).to_string()))
            .unwrap_or(method_name.clone())
    } else {
        method_name.clone()
    };

    let mut obj = this.borrow_mut();
    obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(canonical_method)));
    obj.set_property(b"class".to_vec(), Value::String(PhpString::from_string(canonical_class.clone())));
    obj.set_property(b"__reflection_class".to_vec(), Value::String(PhpString::from_string(canonical_class)));
    obj.set_property(b"__reflection_method".to_vec(), Value::String(PhpString::from_vec(method_lower)));
    if let Some(fn_str) = closure_fn {
        obj.set_property(b"__reflection_closure_fn".to_vec(), Value::String(fn_str));
    }
    true
}

/// ReflectionFunction constructor
pub fn reflection_function_construct(vm: &mut Vm, args: &[Value], line: u32) -> bool {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return true,
    };

    // Validate user arg count
    let user_arg_count = args.len().saturating_sub(1);
    if user_arg_count == 0 {
        let err = "ReflectionFunction::__construct() expects exactly 1 argument, 0 given".to_string();
        let exc = vm.create_exception(b"ArgumentCountError", &err, line);
        vm.current_exception = Some(exc);
        return false;
    }
    if user_arg_count > 1 {
        let err = format!("ReflectionFunction::__construct() expects exactly 1 argument, {} given", user_arg_count);
        let exc = vm.create_exception(b"ArgumentCountError", &err, line);
        vm.current_exception = Some(exc);
        return false;
    }
    let arg = args.get(1).cloned().unwrap_or(Value::Null);

    // Handle Closure objects
    if let Value::Object(closure_obj) = &arg {
        let class_lower: Vec<u8> = closure_obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
        if class_lower == b"closure" {
            let mut obj = this.borrow_mut();
            obj.set_property(b"name".to_vec(), Value::String(PhpString::from_bytes(b"{closure}")));
            obj.set_property(b"__reflection_target".to_vec(), Value::String(PhpString::from_bytes(b"")));
            obj.set_property(b"__reflection_is_closure".to_vec(), Value::True);
            return true;
        }
        // Non-Closure object: throw TypeError
        let cls = String::from_utf8_lossy(&closure_obj.borrow().class_name).to_string();
        let err = format!("ReflectionFunction::__construct(): Argument #1 ($function) must be of type Closure|string, {} given", cls);
        let exc = vm.create_exception(b"TypeError", &err, line);
        vm.current_exception = Some(exc);
        return false;
    }

    // Handle array callables like [ClassName, method] - these should throw for ReflectionFunction
    if let Value::Array(arr) = &arg {
        // Closures with captured `use ($var)` are internally represented
        // as an array whose first element is a `__closure_*` name. Treat
        // those as Closure objects.
        let first_is_closure_str = arr.borrow().values().next().and_then(|v| {
            if let Value::String(s) = v {
                let b = s.as_bytes();
                if b.starts_with(b"__closure_") || b.starts_with(b"__arrow_")
                    || b.starts_with(b"__bound_closure_") || b.starts_with(b"__closure_fcc_")
                {
                    Some(s.clone())
                } else { None }
            } else { None }
        });
        if let Some(s) = first_is_closure_str {
            // Capture the trailing values (use-var bindings).
            let captured: Vec<Value> = arr.borrow().values().skip(1).cloned().collect();
            let mut obj = this.borrow_mut();
            obj.set_property(b"name".to_vec(), Value::String(PhpString::from_bytes(b"{closure}")));
            obj.set_property(b"__reflection_target".to_vec(), Value::String(s));
            obj.set_property(b"__reflection_is_closure".to_vec(), Value::True);
            if !captured.is_empty() {
                let mut arr_captured = PhpArray::new();
                for v in captured {
                    arr_captured.push(v);
                }
                obj.set_property(b"__reflection_closure_captured".to_vec(),
                    Value::Array(Rc::new(RefCell::new(arr_captured))));
            }
            return true;
        }
        let err_msg = "ReflectionFunction::__construct(): Argument #1 ($function) must be of type Closure|string, array given".to_string();
        let exc = vm.create_exception(b"TypeError", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }

    let func_name = match &arg {
        Value::String(s) => {
            let name = s.to_string_lossy();
            // Check for closures
            if name.starts_with("__closure_") || name.starts_with("__arrow_") || name.starts_with("__bound_closure_") || name.starts_with("__closure_fcc_") {
                let mut obj = this.borrow_mut();
                obj.set_property(b"name".to_vec(), Value::String(PhpString::from_bytes(b"{closure}")));
                obj.set_property(b"__reflection_target".to_vec(), Value::String(s.clone()));
                obj.set_property(b"__reflection_is_closure".to_vec(), Value::True);
                return true;
            }
            name
        }
        _ => arg.to_php_string().to_string_lossy(),
    };

    // Strip leading backslash
    let func_name = if func_name.starts_with('\\') {
        func_name[1..].to_string()
    } else {
        func_name
    };
    let func_lower: Vec<u8> = func_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

    // Check if function exists
    let exists = vm.user_functions.contains_key(&func_lower) || vm.functions.contains_key(&func_lower);
    if !exists {
        let err_msg = format!("Function {}() does not exist", func_name);
        let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }

    let mut obj = this.borrow_mut();
    obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(func_name.clone())));
    obj.set_property(b"__reflection_target".to_vec(), Value::String(PhpString::from_vec(func_lower)));
    true
}

/// ReflectionProperty constructor
pub fn reflection_property_construct(vm: &mut Vm, args: &[Value], line: u32) -> bool {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return true,
    };

    // Validate user arg count (must be exactly 2)
    let user_arg_count = args.len().saturating_sub(1);
    if user_arg_count != 2 {
        let err = format!("ReflectionProperty::__construct() expects exactly 2 arguments, {} given", user_arg_count);
        let exc = vm.create_exception(b"ArgumentCountError", &err, line);
        vm.current_exception = Some(exc);
        return false;
    }

    let class_arg = args.get(1).cloned().unwrap_or(Value::Null);
    let prop_arg = args.get(2).cloned().unwrap_or(Value::Null);

    let class_name_raw = match &class_arg {
        Value::Object(obj) => {
            let ob = obj.borrow();
            String::from_utf8_lossy(&ob.class_name).to_string()
        }
        _ => class_arg.to_php_string().to_string_lossy(),
    };
    // Strip a leading backslash from absolute class names (e.g. "\X").
    let class_name = class_name_raw.trim_start_matches('\\').to_string();
    let prop_name = prop_arg.to_php_string().to_string_lossy();

    let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

    let is_user_class = vm.classes.contains_key(&class_lower);
    let is_builtin = vm.is_known_builtin_class(&class_lower);

    // PHP triggers autoload when ReflectionProperty resolves an unknown class.
    let (is_user_class, is_builtin) = if !is_user_class && !is_builtin {
        vm.try_autoload_class(class_name.as_bytes());
        (vm.classes.contains_key(&class_lower), vm.is_known_builtin_class(&class_lower))
    } else {
        (is_user_class, is_builtin)
    };

    if !is_user_class && !is_builtin {
        let err_msg = format!("Class \"{}\" does not exist", class_name);
        let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }

    let canonical_class = if is_user_class {
        vm.classes.get(&class_lower)
            .map(|c| String::from_utf8_lossy(&c.name).to_string())
            .unwrap_or(class_name.clone())
    } else {
        vm.builtin_canonical_name(&class_lower)
    };

    // For user classes, check property exists; for built-in classes, accept any property name.
    // When constructed from an object instance, accept dynamic properties even if the class
    // doesn't declare them (matches PHP behavior for #[AllowDynamicProperties] and stdClass).
    let mut is_dynamic_prop = false;
    if is_user_class {
        let prop_exists_decl = vm.classes.get(&class_lower)
            .map(|c| c.properties.iter().any(|p| p.name == prop_name.as_bytes()))
            .unwrap_or(false);

        if !prop_exists_decl {
            // Check if it exists as a dynamic property on the passed-in object.
            let dyn_exists = if let Value::Object(o) = &class_arg {
                o.borrow().has_property(prop_name.as_bytes())
            } else { false };
            if dyn_exists {
                is_dynamic_prop = true;
            } else {
                let err_msg = format!("Property {}::${} does not exist", canonical_class, prop_name);
                let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
                vm.current_exception = Some(exc);
                return false;
            }
        }
    }

    let mut obj = this.borrow_mut();
    obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(prop_name.clone())));
    obj.set_property(b"class".to_vec(), Value::String(PhpString::from_string(canonical_class.clone())));
    obj.set_property(b"__reflection_class".to_vec(), Value::String(PhpString::from_string(canonical_class)));
    obj.set_property(b"__reflection_prop".to_vec(), Value::String(PhpString::from_string(prop_name)));
    if is_dynamic_prop {
        obj.set_property(b"__reflection_prop_dynamic".to_vec(), Value::True);
        // Store the originating object so getValue/setValue and related can act on it.
        if let Value::Object(o) = &class_arg {
            obj.set_property(b"__reflection_prop_obj".to_vec(), Value::Object(o.clone()));
        }
    }
    true
}

/// ReflectionParameter constructor
pub fn reflection_parameter_construct(vm: &mut Vm, args: &[Value], _line: u32) -> bool {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return true,
    };

    // PHP requires exactly 2 arguments to __construct (function + param)
    if args.len() < 3 {
        let provided = args.len().saturating_sub(1);
        let err_msg = format!(
            "ReflectionParameter::__construct() expects exactly 2 arguments, {} given",
            provided
        );
        let exc = vm.create_exception(b"ArgumentCountError", &err_msg, _line);
        vm.current_exception = Some(exc);
        return false;
    }

    let func_arg = args.get(1).cloned().unwrap_or(Value::Null);
    let param_arg = args.get(2).cloned().unwrap_or(Value::Null);

    // Validate the function argument type: must be string, array, or callable object (closure/etc).
    match &func_arg {
        Value::String(_) | Value::Array(_) | Value::Object(_) => {}
        _ => {
            let tn = Vm::value_type_name(&func_arg);
            let err_msg = format!(
                "ReflectionParameter::__construct(): Argument #1 ($function) must be a string, an array(class, method), or a callable object, {} given",
                tn
            );
            let exc = vm.create_exception(b"ReflectionException", &err_msg, _line);
            vm.current_exception = Some(exc);
            return false;
        }
    }

    // If the function arg is an array, validate class/method existence and raise
    // appropriate ReflectionException messages.
    if let Value::Array(arr) = &func_arg {
        let items: Vec<Value> = arr.borrow().iter().map(|(_, v)| v.clone()).collect();
        if items.len() == 2 {
            let class_name = match &items[0] {
                Value::Object(o) => o.borrow().class_name.clone(),
                Value::String(s) => s.as_bytes().to_vec(),
                _ => Vec::new(),
            };
            // For objects with __toString, invoke it to obtain the method name.
            let method_name = vm.value_to_string(&items[1]).as_bytes().to_vec();
            // Detect closure (either as an object or as the internal string
            // representation used by goro-rs for closures stored in arrays).
            let is_closure_obj = class_name.starts_with(b"__closure_")
                || class_name.starts_with(b"__arrow_")
                || class_name.starts_with(b"__bound_closure_")
                || class_name.eq_ignore_ascii_case(b"Closure");
            if !class_name.is_empty() && !is_closure_obj {
                let class_lower: Vec<u8> = class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                let method_lower: Vec<u8> = method_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                let class_str = String::from_utf8_lossy(&class_name).to_string();
                let method_str = String::from_utf8_lossy(&method_name).to_string();

                // If the class arg was a string (not an object) and class isn't known, raise.
                let class_known = vm.classes.contains_key(class_lower.as_slice())
                    || vm.is_known_builtin_class(class_lower.as_slice());
                let is_string_class = matches!(&items[0], Value::String(_));
                if is_string_class && !class_known {
                    let err_msg = format!("Class \"{}\" does not exist", class_str);
                    let exc = vm.create_exception(b"ReflectionException", &err_msg, _line);
                    vm.current_exception = Some(exc);
                    return false;
                }

                if class_known {
                    let method_exists = vm.classes.get(class_lower.as_slice())
                        .map(|c| c.methods.contains_key(method_lower.as_slice()))
                        .unwrap_or(false);
                    if !method_exists {
                        let err_msg = format!("Method {}::{}() does not exist", class_str, method_str);
                        let exc = vm.create_exception(b"ReflectionException", &err_msg, _line);
                        vm.current_exception = Some(exc);
                        return false;
                    }
                }
            }
        }
    }

    // If the function arg is a plain string referring to an unknown function, raise.
    if let Value::String(s) = &func_arg {
        let raw = s.as_bytes();
        // Skip closures and method ('Class::method') forms; they go through other paths.
        let has_sep = raw.windows(2).any(|w| w == b"::");
        if !has_sep {
            let lower: Vec<u8> = raw.iter().map(|b| b.to_ascii_lowercase()).collect();
            let is_closure = lower.starts_with(b"__closure_")
                || lower.starts_with(b"__arrow_")
                || lower.starts_with(b"__bound_closure_")
                || lower.starts_with(b"__closure_fcc_");
            if !is_closure && !vm.user_functions.contains_key(&lower)
                && !vm.functions.contains_key(&lower) {
                let err_msg = format!("Function {}() does not exist", String::from_utf8_lossy(raw));
                let exc = vm.create_exception(b"ReflectionException", &err_msg, _line);
                vm.current_exception = Some(exc);
                return false;
            }
        }
    }

    // Resolve the function/method reference to a lowercase key in
    // vm.user_functions. Supports strings, closures (string form), arrays
    // ([class, method] or [closure_str]), and Closure objects.
    let func_lower: Vec<u8> = match &func_arg {
        Value::String(s) => s.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect(),
        Value::Array(arr) => {
            let items: Vec<Value> = arr.borrow().iter().map(|(_, v)| v.clone()).collect();
            if items.len() == 2 {
                let class_part = match &items[0] {
                    Value::Object(o) => String::from_utf8_lossy(&o.borrow().class_name).to_string(),
                    Value::String(s) => s.to_string_lossy(),
                    _ => String::new(),
                };
                let method_part = vm.value_to_string(&items[1]).to_string_lossy();
                // If class is closure, use the closure string directly.
                let cl_low = class_part.to_ascii_lowercase();
                if cl_low == "closure" || items[0].clone().to_php_string().to_string_lossy().starts_with("__closure_")
                    || items[0].clone().to_php_string().to_string_lossy().starts_with("__arrow_")
                    || items[0].clone().to_php_string().to_string_lossy().starts_with("__bound_closure_")
                {
                    if let Value::String(s) = &items[0] {
                        let n = s.to_string_lossy();
                        if n.starts_with("__closure_") || n.starts_with("__arrow_")
                            || n.starts_with("__bound_closure_") || n.starts_with("__closure_fcc_")
                        {
                            n.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect()
                        } else {
                            format!("{}::{}", class_part, method_part).as_bytes()
                                .iter().map(|b| b.to_ascii_lowercase()).collect()
                        }
                    } else {
                        format!("{}::{}", class_part, method_part).as_bytes()
                            .iter().map(|b| b.to_ascii_lowercase()).collect()
                    }
                } else {
                    format!("{}::{}", class_part, method_part).as_bytes()
                        .iter().map(|b| b.to_ascii_lowercase()).collect()
                }
            } else if items.len() == 1 {
                items[0].to_php_string().as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect()
            } else {
                Vec::new()
            }
        }
        _ => func_arg.to_php_string().as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect(),
    };

    // For methods, resolve to the class.method MethodDef so we can use cv_names
    // and skip the synthetic $this slot at index 0.
    let (method_class_lower, method_name_lower): (Option<Vec<u8>>, Option<Vec<u8>>) =
        if let Some(sep) = func_lower.windows(2).position(|w| w == b"::") {
            (Some(func_lower[..sep].to_vec()), Some(func_lower[sep+2..].to_vec()))
        } else { (None, None) };

    let method_def = match (&method_class_lower, &method_name_lower) {
        (Some(cl), Some(mn)) => {
            vm.classes.get(cl).and_then(|c| c.methods.get(mn).cloned())
        }
        _ => None,
    };
    let method_cv_offset: usize = method_def.as_ref().map(|m| if m.is_static { 0 } else { 1 }).unwrap_or(0);

    // Parameter may be specified by index or by name.
    // For named lookup, search cv_names skipping the implicit `$this` for methods.
    let param_idx_opt: Option<usize> = match &param_arg {
        Value::String(s) => {
            let name = s.as_bytes();
            if let Some(m) = method_def.as_ref() {
                // Look up among visible parameter names (cv_names[cv_offset..param_count])
                let pc = m.op_array.param_count as usize;
                let names = &m.op_array.cv_names;
                let mut found: Option<usize> = None;
                for i in 0..(pc.saturating_sub(method_cv_offset)) {
                    let cv_idx = i + method_cv_offset;
                    if cv_idx < names.len() && names[cv_idx] == name {
                        found = Some(i);
                        break;
                    }
                }
                found
            } else {
                vm.user_functions.get(&func_lower)
                    .and_then(|op| op.cv_names.iter().position(|n| n == name))
            }
        }
        Value::Long(i) => {
            if *i < 0 {
                let err_msg = "ReflectionParameter::__construct(): Argument #2 ($param) must be greater than or equal to 0";
                let exc = vm.create_exception(b"ValueError", err_msg, _line);
                vm.current_exception = Some(exc);
                return false;
            }
            Some(*i as usize)
        }
        _ => {
            let val = param_arg.to_long();
            if val < 0 {
                let err_msg = "ReflectionParameter::__construct(): Argument #2 ($param) must be greater than or equal to 0";
                let exc = vm.create_exception(b"ValueError", err_msg, _line);
                vm.current_exception = Some(exc);
                return false;
            }
            Some(val as usize)
        }
    };

    // If a string-name lookup was requested but not found, raise.
    if matches!(&param_arg, Value::String(_)) && param_idx_opt.is_none() {
        let err_msg = "The parameter specified by its name could not be found";
        let exc = vm.create_exception(b"ReflectionException", err_msg, _line);
        vm.current_exception = Some(exc);
        return false;
    }

    // Look up the function or method
    if let Some(m) = method_def.as_ref() {
        let param_idx = param_idx_opt.unwrap_or(0);
        // Validate index range (for integer index).
        let pc = m.op_array.param_count as usize;
        let visible_count = pc.saturating_sub(method_cv_offset);
        if param_idx >= visible_count {
            let err_msg = format!("The parameter specified by its offset could not be found");
            let exc = vm.create_exception(b"ReflectionException", &err_msg, _line);
            vm.current_exception = Some(exc);
            return false;
        }
        let cv_idx = param_idx + method_cv_offset;
        let param_name = if cv_idx < m.op_array.cv_names.len() {
            String::from_utf8_lossy(&m.op_array.cv_names[cv_idx]).to_string()
        } else {
            format!("param{}", param_idx)
        };
        let mut obj = this.borrow_mut();
        obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(param_name)));
        obj.set_property(b"__reflection_func".to_vec(), Value::String(PhpString::from_vec(func_lower)));
        obj.set_property(b"__reflection_param_idx".to_vec(), Value::Long(param_idx as i64));
        obj.set_property(b"__reflection_param_cv_offset".to_vec(), Value::Long(method_cv_offset as i64));
        if let (Some(cl), Some(mn)) = (method_class_lower.as_ref(), method_name_lower.as_ref()) {
            obj.set_property(b"__reflection_param_class".to_vec(),
                Value::String(PhpString::from_vec(cl.clone())));
            obj.set_property(b"__reflection_param_method".to_vec(),
                Value::String(PhpString::from_vec(mn.clone())));
        }
    } else if let Some(op_array) = vm.user_functions.get(&func_lower).cloned() {
        let param_idx = param_idx_opt.unwrap_or(0);
        if param_idx >= op_array.param_count as usize {
            let err_msg = format!("The parameter specified by its offset could not be found");
            let exc = vm.create_exception(b"ReflectionException", &err_msg, _line);
            vm.current_exception = Some(exc);
            return false;
        }
        let param_name = if param_idx < op_array.cv_names.len() {
            String::from_utf8_lossy(&op_array.cv_names[param_idx]).to_string()
        } else {
            format!("param{}", param_idx)
        };

        let mut obj = this.borrow_mut();
        obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(param_name)));
        obj.set_property(b"__reflection_func".to_vec(), Value::String(PhpString::from_vec(func_lower)));
        obj.set_property(b"__reflection_param_idx".to_vec(), Value::Long(param_idx as i64));
    } else if let Some(idx) = param_idx_opt {
        // No function found - still set enough for later calls.
        let mut obj = this.borrow_mut();
        obj.set_property(b"__reflection_func".to_vec(), Value::String(PhpString::from_vec(func_lower)));
        obj.set_property(b"__reflection_param_idx".to_vec(), Value::Long(idx as i64));
    }
    true
}

/// ReflectionClassConstant constructor
pub fn reflection_class_constant_construct(vm: &mut Vm, args: &[Value], line: u32) -> bool {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return true,
    };

    // Determine which reflector class we're constructing - for error messages.
    let reflector_name: String = {
        let ob = this.borrow();
        String::from_utf8_lossy(&ob.class_name).to_string()
    };

    let class_arg = args.get(1).cloned().unwrap_or(Value::Null);
    let const_arg = args.get(2).cloned().unwrap_or(Value::Null);

    // Reject array first argument explicitly.
    if matches!(&class_arg, Value::Array(_)) {
        let err_msg = format!(
            "{}::__construct(): Argument #1 ($class) must be of type object|string, array given",
            reflector_name
        );
        let exc = vm.create_exception(b"TypeError", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }

    let class_name_raw = match &class_arg {
        Value::Object(obj) => {
            let ob = obj.borrow();
            String::from_utf8_lossy(&ob.class_name).to_string()
        }
        _ => class_arg.to_php_string().to_string_lossy(),
    };
    // Strip a leading backslash from absolute class names (e.g. "\X").
    let class_name = class_name_raw.trim_start_matches('\\').to_string();
    let const_name = const_arg.to_php_string().to_string_lossy();

    let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

    // Check class exists
    if !vm.classes.contains_key(&class_lower) && !vm.is_known_builtin_class(&class_lower) {
        let err_msg = format!("Class \"{}\" does not exist", class_name);
        let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }

    let canonical_class = vm.classes.get(&class_lower)
        .map(|c| String::from_utf8_lossy(&c.name).to_string())
        .unwrap_or_else(|| vm.builtin_canonical_name(&class_lower));

    // Check constant exists. PHP says "Constant {}::{} does not exist" in
    // both the ReflectionClassConstant and ReflectionEnumUnit/BackedCase paths.
    let const_val = reflection_class_get_constant(vm, &class_lower, const_name.as_bytes());
    if const_val.is_none() {
        let err_msg = format!("Constant {}::{} does not exist", canonical_class, const_name);
        let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }

    let unwrapped = const_val.unwrap();

    // ReflectionEnumUnitCase / ReflectionEnumBackedCase: validate the
    // referenced constant is actually declared as an enum case (regular
    // constants whose value happens to be an enum case are not cases).
    if reflector_name.eq_ignore_ascii_case("ReflectionEnumUnitCase")
        || reflector_name.eq_ignore_ascii_case("ReflectionEnumBackedCase")
    {
        let is_declared_case = vm.classes.get(&class_lower)
            .map(|c| c.enum_cases.iter().any(|(n, _)| n.eq_ignore_ascii_case(const_name.as_bytes())))
            .unwrap_or(false);
        if !is_declared_case {
            let err_msg = format!("Constant {}::{} is not a case", canonical_class, const_name);
            let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
            vm.current_exception = Some(exc);
            return false;
        }
        if reflector_name.eq_ignore_ascii_case("ReflectionEnumBackedCase") {
            let is_backed = if let Value::Object(o) = &unwrapped {
                o.borrow().has_property(b"value")
            } else { false };
            if !is_backed {
                let err_msg = format!("Enum case {}::{} is not a backed case", canonical_class, const_name);
                let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
                vm.current_exception = Some(exc);
                return false;
            }
        }
    }

    // The visible `class` property reflects the constant's declaring class,
    // not the class the reflection was constructed on. Walk up the parent
    // chain to find it (regular constants only — enum cases stay on the
    // enum they were declared on, which is also the class we were given).
    let declaring_class = if reflector_name.eq_ignore_ascii_case("ReflectionEnumUnitCase")
        || reflector_name.eq_ignore_ascii_case("ReflectionEnumBackedCase")
    {
        canonical_class.clone()
    } else {
        find_constant_declaring_class(vm, &class_lower, const_name.as_bytes())
            .unwrap_or_else(|| canonical_class.clone())
    };

    let mut obj = this.borrow_mut();
    obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(const_name.clone())));
    obj.set_property(b"class".to_vec(), Value::String(PhpString::from_string(declaring_class)));
    obj.set_property(b"__reflection_value".to_vec(), unwrapped);
    true
}


/// ReflectionClass no-arg method dispatch
pub fn reflection_class_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    let target = ob.get_property(b"__reflection_target").to_php_string().to_string_lossy();
    let class_lower: Vec<u8> = target.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    drop(ob);

    match method {
        b"getname" => {
            let ob = obj.borrow();
            Some(ob.get_property(b"name"))
        }
        b"getparentclass" => {
            if let Some(ce) = vm.classes.get(&class_lower) {
                if let Some(ref parent) = ce.parent {
                    // Create a ReflectionClass for the parent
                    let parent_name = String::from_utf8_lossy(parent).to_string();
                    Some(create_reflection_class(vm, &parent_name))
                } else {
                    Some(Value::False)
                }
            } else {
                // Check built-in parent chain
                let parents = super::vm::builtin_parent_chain(&class_lower);
                if let Some(first_parent) = parents.first() {
                    let parent_name = vm.builtin_canonical_name(first_parent);
                    Some(create_reflection_class(vm, &parent_name))
                } else {
                    Some(Value::False)
                }
            }
        }
        b"isabstract" => {
            let is_abstract = vm.classes.get(&class_lower)
                .map(|c| c.is_abstract)
                .unwrap_or(false);
            Some(if is_abstract { Value::True } else { Value::False })
        }
        b"isfinal" => {
            let is_final = vm.classes.get(&class_lower)
                .map(|c| c.is_final || c.is_enum) // Enums are implicitly final
                .unwrap_or(false);
            Some(if is_final { Value::True } else { Value::False })
        }
        b"isinterface" => {
            let is_interface = vm.classes.get(&class_lower)
                .map(|c| c.is_interface)
                .unwrap_or(false);
            Some(if is_interface { Value::True } else { Value::False })
        }
        b"istrait" => {
            let is_trait = vm.classes.get(&class_lower)
                .map(|c| c.is_trait)
                .unwrap_or(false);
            Some(if is_trait { Value::True } else { Value::False })
        }
        b"isenum" => {
            let is_enum = vm.classes.get(&class_lower)
                .map(|c| c.is_enum)
                .unwrap_or(false);
            Some(if is_enum { Value::True } else { Value::False })
        }
        b"isreadonly" => {
            let is_readonly = vm.classes.get(&class_lower)
                .map(|c| c.is_readonly)
                .unwrap_or(false);
            Some(if is_readonly { Value::True } else { Value::False })
        }
        b"isinstantiable" => {
            if let Some(ce) = vm.classes.get(&class_lower) {
                let mut instantiable = !ce.is_abstract && !ce.is_interface && !ce.is_trait && !ce.is_enum;
                // A class with a non-public constructor isn't instantiable
                // from outside, so isInstantiable() reports false.
                if instantiable {
                    if let Some(ctor) = ce.get_method(b"__construct") {
                        if ctor.visibility != Visibility::Public {
                            instantiable = false;
                        }
                    }
                }
                Some(if instantiable { Value::True } else { Value::False })
            } else {
                // Built-in classes are generally instantiable
                Some(Value::True)
            }
        }
        b"iscloneable" => {
            // Traits, interfaces, abstract classes, and enums are not cloneable.
            let cloneable = if let Some(ce) = vm.classes.get(&class_lower) {
                !ce.is_interface && !ce.is_trait && !ce.is_abstract && !ce.is_enum
            } else {
                true
            };
            Some(if cloneable { Value::True } else { Value::False })
        }
        b"isinternal" => {
            // User-defined classes are not internal
            let is_internal = !vm.classes.contains_key(&class_lower);
            Some(if is_internal { Value::True } else { Value::False })
        }
        b"isuserdefined" => {
            let is_user = vm.classes.contains_key(&class_lower);
            Some(if is_user { Value::True } else { Value::False })
        }
        b"isanonymous" => {
            // Anonymous class names contain a NUL byte separator
            let is_anon = if let Some(ce) = vm.classes.get(&class_lower) {
                ce.name.contains(&b'\x00')
            } else {
                false
            };
            Some(if is_anon { Value::True } else { Value::False })
        }
        b"isiterable" | b"isiterateable" => {
            // Per PHP: interfaces themselves and abstract classes are not iterable
            // even if they extend Iterator. Only concrete (or non-interface,
            // non-abstract) classes that implement Iterator/IteratorAggregate are.
            let (is_interface, is_abstract) = vm.classes.get(&class_lower)
                .map(|c| (c.is_interface, c.is_abstract))
                .unwrap_or((vm.builtin_is_interface(&class_lower), false));
            if is_interface || is_abstract {
                return Some(Value::False);
            }
            let is_iterable = vm.class_implements_interface(&class_lower, b"iterator")
                || vm.class_implements_interface(&class_lower, b"iteratoraggregate")
                || vm.builtin_implements_interface(&class_lower, b"iterator")
                || vm.builtin_implements_interface(&class_lower, b"iteratoraggregate");
            Some(if is_iterable { Value::True } else { Value::False })
        }
        b"getinterfacenames" => {
            // Collect all interfaces reachable via implements/extends chains.
            let mut collected: Vec<Vec<u8>> = Vec::new();
            collect_all_interfaces(vm, &class_lower, &mut collected);
            let mut names = PhpArray::new();
            for iface in collected {
                names.push(Value::String(PhpString::from_vec(iface)));
            }
            // Add implicit Stringable interface for classes with __toString
            if vm.class_implements_interface(&class_lower, b"stringable") {
                let already_has = names.iter().any(|(_, v)| {
                    v.to_php_string().as_bytes().eq_ignore_ascii_case(b"Stringable")
                });
                if !already_has {
                    names.push(Value::String(PhpString::from_vec(b"Stringable".to_vec())));
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(names))))
        }
        b"getinterfaces" => {
            let mut collected: Vec<Vec<u8>> = Vec::new();
            collect_all_interfaces(vm, &class_lower, &mut collected);
            let mut result = PhpArray::new();
            for iface in collected {
                let iface_name = String::from_utf8_lossy(&iface).to_string();
                let rc = create_reflection_class(vm, &iface_name);
                result.set(ArrayKey::String(PhpString::from_vec(iface.clone())), rc);
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"gettraitnames" => {
            let mut names = PhpArray::new();
            if let Some(ce) = vm.classes.get(&class_lower) {
                for t in &ce.traits {
                    names.push(Value::String(PhpString::from_vec(t.clone())));
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(names))))
        }
        b"gettraits" => {
            let mut result = PhpArray::new();
            if let Some(ce) = vm.classes.get(&class_lower) {
                for t in ce.traits.clone() {
                    let trait_name = String::from_utf8_lossy(&t).to_string();
                    let rc = create_reflection_class(vm, &trait_name);
                    result.set(ArrayKey::String(PhpString::from_vec(t.clone())), rc);
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"gettraitaliases" => {
            let mut result = PhpArray::new();
            if let Some(ce) = vm.classes.get(&class_lower) {
                for adapt in &ce.trait_adaptations {
                    if let crate::object::TraitAdaptation::Alias { trait_name, method, new_name, .. } = adapt {
                        if let Some(alias) = new_name {
                            let trait_str = trait_name.as_ref()
                                .map(|t| String::from_utf8_lossy(t).to_string())
                                .unwrap_or_default();
                            let method_str = String::from_utf8_lossy(method).to_string();
                            let alias_str = String::from_utf8_lossy(alias).to_string();
                            let source = if trait_str.is_empty() {
                                method_str
                            } else {
                                format!("{}::{}", trait_str, method_str)
                            };
                            result.set(
                                ArrayKey::String(PhpString::from_string(alias_str)),
                                Value::String(PhpString::from_string(source)),
                            );
                        }
                    }
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"getconstructor" => {
            if let Some(ce) = vm.classes.get(&class_lower) {
                if ce.get_method(b"__construct").is_some() {
                    Some(create_reflection_method(vm, &target, "__construct"))
                } else {
                    Some(Value::Null)
                }
            } else {
                Some(Value::Null)
            }
        }
        b"getmodifiers" => {
            let mut mods = 0i64;
            if let Some(ce) = vm.classes.get(&class_lower) {
                if ce.is_abstract { mods |= 0x40; } // IS_EXPLICIT_ABSTRACT
                if ce.is_final { mods |= 0x20; } // IS_FINAL
                if ce.is_readonly { mods |= 0x10000; } // IS_READONLY
            }
            Some(Value::Long(mods))
        }
        b"getdefaultproperties" => {
            // Order: this class's statics (own + inherited non-private), then
            // this class's instance properties (own + inherited non-private).
            let mut result = PhpArray::new();
            // Build the chain from this class up to root.
            let mut chain: Vec<Vec<u8>> = Vec::new();
            chain.push(class_lower.clone());
            if let Some(ce) = vm.classes.get(&class_lower) {
                let mut parent = ce.parent.clone();
                while let Some(ref p) = parent {
                    let p_lower: Vec<u8> = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                    chain.push(p_lower.clone());
                    if let Some(pce) = vm.classes.get(&p_lower) {
                        parent = pce.parent.clone();
                    } else {
                        break;
                    }
                }
            }

            // Static properties pass (child first, inherited non-private).
            // Skip props whose declaring_class is an ancestor *and* are private —
            // those are not visible to subclasses.
            for (idx, link) in chain.iter().enumerate() {
                if let Some(lce) = vm.classes.get(link) {
                    for prop in &lce.properties {
                        if !prop.is_static { continue; }
                        // For the runtime class entry, properties[] also includes
                        // inherited slots — filter out inherited privates by
                        // comparing declaring_class to the current link.
                        if prop.visibility == Visibility::Private && prop.declaring_class.as_slice() != link.as_slice() {
                            continue;
                        }
                        if idx > 0 && prop.visibility == Visibility::Private { continue; }
                        let key = ArrayKey::String(PhpString::from_vec(prop.name.clone()));
                        if result.get(&key).is_none() {
                            // Lookup resolved value (static_properties is the
                            // source of truth at runtime).
                            let val = lce.static_properties.get(&prop.name)
                                .cloned()
                                .unwrap_or_else(|| prop.default.clone());
                            result.set(key, val);
                        }
                    }
                }
            }

            // Instance properties pass
            for (idx, link) in chain.iter().enumerate() {
                if let Some(lce) = vm.classes.get(link) {
                    for prop in &lce.properties {
                        if prop.is_static { continue; }
                        // Skip inherited private props (declaring_class != link).
                        if prop.visibility == Visibility::Private && prop.declaring_class.as_slice() != link.as_slice() {
                            continue;
                        }
                        if idx > 0 && prop.visibility == Visibility::Private { continue; }
                        let key = ArrayKey::String(PhpString::from_vec(prop.name.clone()));
                        if result.get(&key).is_none() {
                            result.set(key, prop.default.clone());
                        }
                    }
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"getstaticproperties" => {
            // Include own static props (including own private) and inherited
            // non-private statics. Properties stored on this class with
            // declaring_class != self are inherited; private inherited ones
            // must be excluded.
            let mut result = PhpArray::new();
            if let Some(ce) = vm.classes.get(&class_lower) {
                for prop in &ce.properties {
                    if !prop.is_static { continue; }
                    let decl_lower: Vec<u8> = prop.declaring_class.iter().map(|b| b.to_ascii_lowercase()).collect();
                    let is_inherited = decl_lower != class_lower;
                    if is_inherited && prop.visibility == Visibility::Private { continue; }
                    let key = ArrayKey::String(PhpString::from_vec(prop.name.clone()));
                    if result.get(&key).is_none() {
                        // Static value is stored on the declaring class
                        let val = if is_inherited {
                            vm.classes.get(&decl_lower)
                                .and_then(|d| d.static_properties.get(&prop.name).cloned())
                                .unwrap_or_else(|| prop.default.clone())
                        } else {
                            ce.static_properties.get(&prop.name).cloned()
                                .unwrap_or_else(|| prop.default.clone())
                        };
                        result.set(key, val);
                    }
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"getfilename" => {
            if let Some(ce) = vm.classes.get(&class_lower) {
                if let Some(ref filename) = ce.filename {
                    Some(Value::String(PhpString::from_string(filename.clone())))
                } else {
                    Some(Value::String(PhpString::from_string(vm.current_file.clone())))
                }
            } else {
                Some(Value::False)
            }
        }
        b"getstartline" => {
            if let Some(ce) = vm.classes.get(&class_lower) {
                if ce.start_line > 0 {
                    Some(Value::Long(ce.start_line as i64))
                } else {
                    Some(Value::False)
                }
            } else {
                Some(Value::False)
            }
        }
        b"getendline" => {
            if let Some(ce) = vm.classes.get(&class_lower) {
                if ce.end_line > 0 {
                    Some(Value::Long(ce.end_line as i64))
                } else {
                    Some(Value::False)
                }
            } else {
                Some(Value::False)
            }
        }
        b"getdoccomment" => {
            if let Some(ce) = vm.classes.get(&class_lower) {
                if let Some(ref doc) = ce.doc_comment {
                    Some(Value::String(PhpString::from_string(doc.clone())))
                } else {
                    Some(Value::False)
                }
            } else {
                Some(Value::False)
            }
        }
        b"newinstancewithoutconstructor" => {
            // Internal final classes cannot be created without their ctor.
            let no_ctorless: &[&[u8]] = &[
                b"generator", b"fiber", b"closure",
            ];
            if no_ctorless.iter().any(|c| *c == class_lower.as_slice()) {
                let canonical = vm.classes.get(&class_lower)
                    .map(|c| String::from_utf8_lossy(&c.name).to_string())
                    .unwrap_or_else(|| vm.builtin_canonical_name(&class_lower));
                let err_msg = format!(
                    "Class {} is an internal class marked as final that cannot be instantiated without invoking its constructor",
                    canonical
                );
                let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
            // Create an instance without calling the constructor
            let obj_id = vm.next_object_id();
            let canonical = vm.classes.get(&class_lower)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| vm.builtin_canonical_name(&class_lower).into_bytes());
            let mut new_obj = PhpObject::new(canonical, obj_id);
            // Set default property values
            if let Some(ce) = vm.classes.get(&class_lower) {
                let props: Vec<_> = ce.properties.iter()
                    .filter(|p| !p.is_static)
                    .map(|p| (p.name.clone(), p.default.clone()))
                    .collect();
                for (name, default) in props {
                    let resolved = vm.resolve_deferred_value(&default);
                    new_obj.set_property(name, resolved);
                }
            }
            Some(Value::Object(Rc::new(RefCell::new(new_obj))))
        }
        b"getextension" => {
            Some(Value::Null)
        }
        b"getextensionname" => {
            Some(Value::False)
        }
        b"innamespace" => {
            Some(if target.contains('\\') { Value::True } else { Value::False })
        }
        b"getnamespacename" => {
            if let Some(pos) = target.rfind('\\') {
                Some(Value::String(PhpString::from_string(target[..pos].to_string())))
            } else {
                Some(Value::String(PhpString::empty()))
            }
        }
        b"getshortname" => {
            if let Some(pos) = target.rfind('\\') {
                Some(Value::String(PhpString::from_string(target[pos + 1..].to_string())))
            } else {
                Some(Value::String(PhpString::from_string(target)))
            }
        }
        // ReflectionEnum-specific methods
        b"isbacked" => {
            let is_backed = vm.classes.get(&class_lower)
                .map(|c| c.is_enum && c.enum_backing_type.is_some())
                .unwrap_or(false);
            Some(if is_backed { Value::True } else { Value::False })
        }
        b"getbackingtype" => {
            if let Some(ce) = vm.classes.get(&class_lower) {
                if let Some(ref bt) = ce.enum_backing_type {
                    Some(create_reflection_type(vm, &crate::opcode::ParamType::Simple(bt.clone())))
                } else {
                    Some(Value::Null)
                }
            } else {
                Some(Value::Null)
            }
        }
        b"getcases" => {
            let mut result = PhpArray::new();
            // Collect case info first, then create objects
            let case_info: Vec<(String, bool, String)> = if let Some(ce) = vm.classes.get(&class_lower) {
                let canonical = String::from_utf8_lossy(&ce.name).to_string();
                let is_backed = ce.enum_backing_type.is_some();
                ce.enum_cases.iter().map(|(case_name, _)| {
                    let case_str = String::from_utf8_lossy(case_name).to_string();
                    (case_str, is_backed, canonical.clone())
                }).collect()
            } else {
                vec![]
            };
            for (case_str, is_backed, canonical) in case_info {
                let class_type = if is_backed { "ReflectionEnumBackedCase" } else { "ReflectionEnumUnitCase" };
                let obj_id = vm.next_object_id();
                let mut case_obj = PhpObject::new(class_type.as_bytes().to_vec(), obj_id);
                case_obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(case_str.clone())));
                case_obj.set_property(b"class".to_vec(), Value::String(PhpString::from_string(canonical)));
                let case_val = reflection_class_get_constant(vm, &class_lower, case_str.as_bytes());
                if let Some(cv) = case_val {
                    case_obj.set_property(b"__reflection_value".to_vec(), cv);
                }
                result.push(Value::Object(Rc::new(RefCell::new(case_obj))));
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        // hasCase/getCase need args; let docall handle them.
        b"__tostring" => {
            // Build a detailed __toString representation for ReflectionClass
            let ob = obj.borrow();
            let name = ob.get_property(b"name").to_php_string().to_string_lossy();
            // Detect which reflector this is: ReflectionClass, ReflectionObject,
            // or ReflectionEnum. Each has a slightly different header line.
            let reflector_class: Vec<u8> = ob.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            let wrapped = if reflector_class == b"reflectionobject" {
                match ob.get_property(b"__reflection_object") {
                    Value::Object(o) => Some(o.clone()),
                    _ => None,
                }
            } else { None };
            drop(ob);
            let kind = if reflector_class == b"reflectionobject" {
                ReflectionStringKind::Object
            } else if reflector_class == b"reflectionenum" {
                ReflectionStringKind::Enum
            } else {
                ReflectionStringKind::Class
            };
            Some(Value::String(PhpString::from_string(
                reflection_class_to_string(vm, &name, &class_lower, kind, wrapped.as_ref())
            )))
        }
        _ => None,
    }
}

/// ReflectionClass methods that need args (dispatched via handle_spl_docall)
pub fn reflection_class_docall(
    vm: &mut Vm,
    method: &[u8],
    args: &[Value],
) -> Option<Value> {
    let this = args.first()?;
    if let Value::Object(obj) = this {
        let ob = obj.borrow();
        let target = ob.get_property(b"__reflection_target").to_php_string().to_string_lossy();
        let class_lower: Vec<u8> = target.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
        drop(ob);

        match method {
            b"hasmethod" => {
                if !check_arg_count(vm, args, 1, "ReflectionClass::hasMethod") {
                    return Some(Value::Null);
                }
                let method_name = match require_string_arg(vm, args, 0, "ReflectionClass::hasMethod", "name", "string", true) {
                    Some(s) => s,
                    None => return Some(Value::Null),
                };
                let method_lower: Vec<u8> = method_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                let has = vm.classes.get(&class_lower)
                    .map(|c| c.get_method(&method_lower).is_some())
                    .unwrap_or(false);
                // Closure::__invoke is implicitly defined.
                let closure_has = class_lower == b"closure"
                    && matches!(method_lower.as_slice(), b"__invoke" | b"bindto" | b"call" | b"bind" | b"fromcallable");
                Some(if has || closure_has { Value::True } else { Value::False })
            }
            b"hasproperty" => {
                if !check_arg_count(vm, args, 1, "ReflectionClass::hasProperty") {
                    return Some(Value::Null);
                }
                let prop_name_s = match require_string_arg(vm, args, 0, "ReflectionClass::hasProperty", "name", "string", true) {
                    Some(s) => s,
                    None => return Some(Value::Null),
                };
                let prop_name = PhpString::from_string(prop_name_s);
                let has = vm.classes.get(&class_lower)
                    .map(|c| c.properties.iter().any(|p| {
                        if p.name != prop_name.as_bytes() { return false; }
                        // Inherited private (instance) props are not visible.
                        if p.visibility == Visibility::Private && p.declaring_class != class_lower && !p.is_static {
                            return false;
                        }
                        true
                    }))
                    .unwrap_or(false);
                // Also check dynamic properties on the reflected object,
                // but exclude properties that ARE declared on the class
                // hierarchy (including inherited private). Those are not
                // "dynamic" for purposes of hasProperty visibility.
                let has_dynamic = if !has {
                    let ob = obj.borrow();
                    let self_obj = ob.get_property(b"__reflection_object");
                    drop(ob);
                    if let Value::Object(inst) = &self_obj {
                        let io = inst.borrow();
                        let on_instance = io.has_property(prop_name.as_bytes());
                        if !on_instance { false } else {
                            // If any class in the hierarchy declares this property,
                            // it is not a true dynamic property.
                            let mut declared_anywhere = false;
                            let mut current = Some(class_lower.clone());
                            while let Some(cl) = current {
                                if let Some(ce) = vm.classes.get(&cl) {
                                    if ce.properties.iter().any(|p| p.name == prop_name.as_bytes()) {
                                        declared_anywhere = true;
                                        break;
                                    }
                                    current = ce.parent.as_ref().map(|p| p.iter().map(|b| b.to_ascii_lowercase()).collect());
                                } else { break; }
                            }
                            !declared_anywhere
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };
                Some(if has || has_dynamic { Value::True } else { Value::False })
            }
            b"hasconstant" => {
                if !check_arg_count(vm, args, 1, "ReflectionClass::hasConstant") {
                    return Some(Value::Null);
                }
                let const_name_s = match require_string_arg(vm, args, 0, "ReflectionClass::hasConstant", "name", "string", true) {
                    Some(s) => s,
                    None => return Some(Value::Null),
                };
                let const_name = PhpString::from_string(const_name_s);
                let has = reflection_class_has_constant(vm, &class_lower, const_name.as_bytes());
                Some(if has { Value::True } else { Value::False })
            }
            b"getconstant" => {
                if !check_arg_count(vm, args, 1, "ReflectionClass::getConstant") {
                    return Some(Value::Null);
                }
                let const_name_s = match require_string_arg(vm, args, 0, "ReflectionClass::getConstant", "name", "string", true) {
                    Some(s) => s,
                    None => return Some(Value::Null),
                };
                let const_name = PhpString::from_string(const_name_s);
                let val = reflection_class_get_constant(vm, &class_lower, const_name.as_bytes());
                if let Some(v) = val {
                    Some(v)
                } else {
                    // Emit deprecated warning
                    vm.emit_deprecated_at(
                        "ReflectionClass::getConstant() for a non-existent constant is deprecated, use ReflectionClass::hasConstant() to check if the constant exists",
                        vm.current_line,
                    );
                    Some(Value::False)
                }
            }
            b"getconstants" => {
                // Filter: IS_PUBLIC=1, IS_PROTECTED=2, IS_PRIVATE=4, IS_FINAL=0x20
                let filter = args.get(1).map(|v| if matches!(v, Value::Null) { -1 } else { v.to_long() }).unwrap_or(-1);
                let mut result = PhpArray::new();
                if let Some(ce) = vm.classes.get(&class_lower) {
                    for (name, val) in &ce.constants {
                        if filter != -1 {
                            let vis_flag = ce.constants_meta.get(name)
                                .map(|m| match m.visibility {
                                    Visibility::Public => 1i64,
                                    Visibility::Protected => 2,
                                    Visibility::Private => 4,
                                })
                                .unwrap_or(1);
                            let final_flag = ce.constants_meta.get(name)
                                .map(|m| if m.is_final { 0x20 } else { 0 })
                                .unwrap_or(0);
                            if (filter & vis_flag) == 0 && (filter & final_flag) == 0 {
                                continue;
                            }
                        }
                        result.set(ArrayKey::String(PhpString::from_vec(name.clone())), val.clone());
                    }
                }
                // Also check parent constants (no filter applied to inherited
                // for simplicity; PHP does filter these too though).
                if filter == -1 {
                    reflection_collect_parent_constants(vm, &class_lower, &mut result);
                }
                Some(Value::Array(Rc::new(RefCell::new(result))))
            }
            b"getmethod" => {
                if !check_arg_count(vm, args, 1, "ReflectionClass::getMethod") {
                    return Some(Value::Null);
                }
                let method_name = match require_string_arg(vm, args, 0, "ReflectionClass::getMethod", "name", "string", true) {
                    Some(s) => s,
                    None => return Some(Value::Null),
                };
                let method_lower: Vec<u8> = method_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                // Look up the method definition and, if found, use its
                // `declaring_class` for the ReflectionMethod->$class field.
                let method_meta = vm.classes.get(&class_lower)
                    .and_then(|c| c.get_method(&method_lower))
                    .map(|m| (String::from_utf8_lossy(&m.name).to_string(),
                              m.declaring_class.clone()));
                if let Some((canonical_method, declaring_lower)) = method_meta {
                    let declaring_canonical = vm.classes.get(declaring_lower.as_slice())
                        .map(|c| String::from_utf8_lossy(&c.name).to_string())
                        .unwrap_or_else(|| String::from_utf8_lossy(&declaring_lower).to_string());
                    // Pass the lookup class so __toString can show
                    // "<user, inherits Parent>" when the method was fetched
                    // from a child class but is declared on a parent.
                    let lookup_canonical = vm.classes.get(&class_lower)
                        .map(|c| String::from_utf8_lossy(&c.name).to_string())
                        .unwrap_or_else(|| target.clone());
                    Some(create_reflection_method_with_lookup(vm, &declaring_canonical, &canonical_method, Some(&lookup_canonical)))
                } else if class_lower == b"closure" && method_lower == b"__invoke" {
                    // Synthesise ReflectionMethod for Closure::__invoke by
                    // pointing at the underlying function name captured when
                    // we constructed the ReflectionObject/Class from a closure.
                    let closure_fn: Option<String> = {
                        let ob = obj.borrow();
                        let direct = ob.get_property(b"__reflection_closure_fn");
                        match direct {
                            Value::String(s) => Some(s.to_string_lossy()),
                            _ => None,
                        }
                    };
                    let obj_id = vm.next_object_id();
                    let mut rm_obj = PhpObject::new(b"ReflectionMethod".to_vec(), obj_id);
                    rm_obj.set_property(b"name".to_vec(), Value::String(PhpString::from_bytes(b"__invoke")));
                    rm_obj.set_property(b"class".to_vec(), Value::String(PhpString::from_bytes(b"Closure")));
                    rm_obj.set_property(b"__reflection_class".to_vec(), Value::String(PhpString::from_bytes(b"Closure")));
                    rm_obj.set_property(b"__reflection_method".to_vec(), Value::String(PhpString::from_bytes(b"__invoke")));
                    if let Some(fn_n) = closure_fn {
                        rm_obj.set_property(b"__reflection_closure_fn".to_vec(),
                            Value::String(PhpString::from_string(fn_n)));
                    }
                    Some(Value::Object(Rc::new(RefCell::new(rm_obj))))
                } else {
                    let canonical_class = vm.classes.get(&class_lower)
                        .map(|c| String::from_utf8_lossy(&c.name).to_string())
                        .unwrap_or(target.clone());
                    let err_msg = format!("Method {}::{}() does not exist", canonical_class, method_name);
                    let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    Some(Value::Null)
                }
            }
            b"getmethods" => {
                let filter = args.get(1).map(|v| if matches!(v, Value::Null) { -1 } else { v.to_long() }).unwrap_or(-1);
                let mut result = PhpArray::new();
                // Collect method info first to avoid borrow issues
                let method_info: Vec<(String, String)> = if let Some(ce) = vm.classes.get(&class_lower) {
                    ce.methods.values().filter_map(|method_def| {
                        if filter != -1 {
                            let method_mod = reflection_method_modifiers_static(method_def);
                            if method_mod & filter == 0 {
                                return None;
                            }
                        }
                        // Inherited private methods are NOT visible.
                        if method_def.visibility == Visibility::Private
                            && method_def.declaring_class != class_lower
                        {
                            return None;
                        }
                        // Property hooks (compiler-synthesised __property_get_/set_
                        // methods) are not real methods.
                        if method_def.name.starts_with(b"__property_get_")
                            || method_def.name.starts_with(b"__property_set_")
                        {
                            return None;
                        }
                        let method_name = String::from_utf8_lossy(&method_def.name).to_string();
                        let declaring = String::from_utf8_lossy(&method_def.declaring_class).to_string();
                        Some((declaring, method_name))
                    }).collect()
                } else {
                    vec![]
                };
                for (declaring, method_name) in method_info {
                    let declaring_lower: Vec<u8> = declaring.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                    let declaring_canonical = vm.classes.get(&declaring_lower)
                        .map(|c| String::from_utf8_lossy(&c.name).to_string())
                        .unwrap_or(declaring);
                    result.push(create_reflection_method(
                        vm,
                        &declaring_canonical,
                        &method_name,
                    ));
                }
                // For Closure include an implicit __invoke method.
                if class_lower == b"closure" {
                    let closure_fn: Option<String> = {
                        let ob = obj.borrow();
                        let direct = ob.get_property(b"__reflection_closure_fn");
                        match direct {
                            Value::String(s) => Some(s.to_string_lossy()),
                            _ => None,
                        }
                    };
                    let obj_id = vm.next_object_id();
                    let mut rm_obj = PhpObject::new(b"ReflectionMethod".to_vec(), obj_id);
                    rm_obj.set_property(b"name".to_vec(), Value::String(PhpString::from_bytes(b"__invoke")));
                    rm_obj.set_property(b"class".to_vec(), Value::String(PhpString::from_bytes(b"Closure")));
                    rm_obj.set_property(b"__reflection_class".to_vec(), Value::String(PhpString::from_bytes(b"Closure")));
                    rm_obj.set_property(b"__reflection_method".to_vec(), Value::String(PhpString::from_bytes(b"__invoke")));
                    if let Some(fn_n) = closure_fn {
                        rm_obj.set_property(b"__reflection_closure_fn".to_vec(),
                            Value::String(PhpString::from_string(fn_n)));
                    }
                    result.push(Value::Object(Rc::new(RefCell::new(rm_obj))));
                }
                Some(Value::Array(Rc::new(RefCell::new(result))))
            }
            b"getproperty" => {
                if !check_arg_count(vm, args, 1, "ReflectionClass::getProperty") {
                    return Some(Value::Null);
                }
                let prop_input = match require_string_arg(vm, args, 0, "ReflectionClass::getProperty", "name", "string", true) {
                    Some(s) => s,
                    None => return Some(Value::Null),
                };
                // Support fully qualified "ClassName::propName" syntax.
                let (qualified_class_lower, prop_name): (Option<Vec<u8>>, String) =
                    if let Some(pos) = prop_input.find("::") {
                        let qc = &prop_input[..pos];
                        let pn = &prop_input[pos + 2..];
                        (Some(qc.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect()), pn.to_string())
                    } else {
                        (None, prop_input.clone())
                    };
                if let Some(ref qc_lower) = qualified_class_lower {
                    let is_self = *qc_lower == class_lower;
                    let is_ancestor = vm.class_extends(&class_lower, qc_lower);
                    if !is_self && !is_ancestor {
                        let qc_exists = vm.classes.contains_key(qc_lower) || vm.is_known_builtin_class(qc_lower);
                        if !qc_exists {
                            // Give a registered autoloader a chance to load this class
                            // before reporting it as missing — PHP triggers autoload
                            // when resolving "Class::prop" qualifiers in getProperty().
                            vm.try_autoload_class(qc_lower);
                        }
                        let qc_exists = vm.classes.contains_key(qc_lower) || vm.is_known_builtin_class(qc_lower);
                        if !qc_exists {
                            let qc_canonical = String::from_utf8_lossy(qc_lower).to_string();
                            let err = format!("Class \"{}\" does not exist", qc_canonical);
                            let exc = vm.create_exception(b"ReflectionException", &err, vm.current_line);
                            vm.current_exception = Some(exc);
                            return Some(Value::Null);
                        }
                        let qc_canonical = vm.classes.get(qc_lower)
                            .map(|c| String::from_utf8_lossy(&c.name).to_string())
                            .unwrap_or_else(|| String::from_utf8_lossy(qc_lower).to_string());
                        let err = format!(
                            "Fully qualified property name {}::${} does not specify a base class of {}",
                            qc_canonical, prop_name, target
                        );
                        let exc = vm.create_exception(b"ReflectionException", &err, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                }
                let lookup_lower: Vec<u8> = qualified_class_lower
                    .clone()
                    .unwrap_or_else(|| class_lower.clone());
                // Inherited private (instance) props are NOT visible via
                // getProperty without an explicit qualifier; static private
                // props remain accessible.
                let prop_info = vm.classes.get(&lookup_lower)
                    .and_then(|c| c.properties.iter().find(|p| {
                        if p.name != prop_name.as_bytes() { return false; }
                        if qualified_class_lower.is_none()
                            && p.visibility == Visibility::Private
                            && p.declaring_class != lookup_lower
                            && !p.is_static
                        {
                            return false;
                        }
                        true
                    }))
                    .map(|p| p.declaring_class.clone());
                // For ReflectionObject, check the actual object for dynamic
                // properties. Inherited private props leave entries on the
                // instance, but those are NOT dynamic — only treat a property
                // as dynamic when it's not declared anywhere on the hierarchy.
                let has_dynamic = if prop_info.is_none() {
                    let ob = obj.borrow();
                    let self_obj = ob.get_property(b"__reflection_object");
                    drop(ob);
                    if let Value::Object(inst) = &self_obj {
                        let io = inst.borrow();
                        let on_inst = io.has_property(prop_name.as_bytes());
                        drop(io);
                        if !on_inst { false } else {
                            // Check whether declared anywhere in hierarchy
                            let mut declared_anywhere = false;
                            let mut current = Some(class_lower.clone());
                            while let Some(cl) = current {
                                if let Some(ce) = vm.classes.get(&cl) {
                                    if ce.properties.iter().any(|p| p.name == prop_name.as_bytes()) {
                                        declared_anywhere = true;
                                        break;
                                    }
                                    current = ce.parent.as_ref().map(|p| p.iter().map(|b| b.to_ascii_lowercase()).collect());
                                } else { break; }
                            }
                            !declared_anywhere
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };
                if let Some(declaring_lower) = prop_info {
                    let declaring_canonical = vm.classes.get(declaring_lower.as_slice())
                        .map(|c| String::from_utf8_lossy(&c.name).to_string())
                        .unwrap_or_else(|| String::from_utf8_lossy(&declaring_lower).to_string());
                    Some(create_reflection_property(vm, &declaring_canonical, &prop_name))
                } else if has_dynamic {
                    Some(create_reflection_property(vm, &target, &prop_name))
                } else {
                    let err_msg = format!("Property {}::${} does not exist", target, prop_name);
                    let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    Some(Value::Null)
                }
            }
            b"getproperties" => {
                let filter = args.get(1).map(|v| if matches!(v, Value::Null) { -1 } else { v.to_long() }).unwrap_or(-1);
                let mut result = PhpArray::new();
                let mut declared_names: Vec<Vec<u8>> = Vec::new();
                // Collect declared properties (both static and instance) along
                // with their declaring class. Private props from a parent are
                // not visible on subclasses.
                let declared: Vec<(String, Vec<u8>)> = if let Some(ce) = vm.classes.get(&class_lower) {
                    let mut v = Vec::new();
                    for prop in ce.properties.iter() {
                        if filter != -1 {
                            let prop_mod = reflection_property_modifiers_static(prop);
                            if prop_mod & filter == 0 { continue; }
                        }
                        if prop.visibility == Visibility::Private && prop.declaring_class != class_lower {
                            continue;
                        }
                        let name_str = String::from_utf8_lossy(&prop.name).to_string();
                        v.push((name_str, prop.declaring_class.clone()));
                        declared_names.push(prop.name.clone());
                    }
                    v
                } else { Vec::new() };
                for (name_str, declaring_lower) in declared {
                    let declaring_canonical = vm.classes.get(declaring_lower.as_slice())
                        .map(|c| String::from_utf8_lossy(&c.name).to_string())
                        .unwrap_or_else(|| String::from_utf8_lossy(&declaring_lower).to_string());
                    result.push(create_reflection_property(vm, &declaring_canonical, &name_str));
                }
                // For ReflectionObject, append dynamic instance properties
                // (these are considered IS_PUBLIC). Collect names first.
                let include_dynamic = filter == -1 || (filter & 1) != 0;
                if include_dynamic {
                    // Collect props on the instance that are NOT declared on the
                    // runtime class. For inherited private props, the class.properties
                    // list will already include them — so we filter them by name AND by
                    // checking if there's a *visible* (non-private-from-parent) declared
                    // prop with that name.
                    let visible_declared_names: Vec<Vec<u8>> = if let Some(ce) = vm.classes.get(&class_lower) {
                        ce.properties.iter()
                            .filter(|p| !(p.visibility == Visibility::Private && p.declaring_class != class_lower))
                            .map(|p| p.name.clone())
                            .collect()
                    } else {
                        Vec::new()
                    };
                    let dyn_names: Vec<Vec<u8>> = {
                        let ob = obj.borrow();
                        let self_obj = ob.get_property(b"__reflection_object");
                        drop(ob);
                        if let Value::Object(inst) = &self_obj {
                            let io = inst.borrow();
                            io.properties.iter()
                                .filter(|(name, _)| !name.starts_with(b"__"))
                                .filter(|(name, _)| !visible_declared_names.iter().any(|n| n == name))
                                .map(|(n, _)| n.clone())
                                .collect()
                        } else {
                            Vec::new()
                        }
                    };
                    for dn in dyn_names {
                        let name_str = String::from_utf8_lossy(&dn).to_string();
                        let rp = create_reflection_property(vm, &target, &name_str);
                        // Mark this ReflectionProperty as referring to a dynamic
                        // (instance-only) property so visibility queries return
                        // public and declaring class reports the runtime class.
                        if let Value::Object(rp_obj) = &rp {
                            rp_obj.borrow_mut().set_property(b"__reflection_prop_dynamic".to_vec(), Value::True);
                        }
                        result.push(rp);
                    }
                }
                Some(Value::Array(Rc::new(RefCell::new(result))))
            }
            b"issubclassof" => {
                let user_arg_count = args.len().saturating_sub(1);
                if user_arg_count == 0 {
                    let err_msg = "ReflectionClass::isSubclassOf() expects exactly 1 argument, 0 given".to_string();
                    let exc = vm.create_exception(b"ArgumentCountError", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                if user_arg_count > 1 {
                    let err_msg = format!("ReflectionClass::isSubclassOf() expects exactly 1 argument, {} given", user_arg_count);
                    let exc = vm.create_exception(b"ArgumentCountError", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                let parent_arg = args.get(1)?;
                if matches!(parent_arg, Value::Null) {
                    vm.emit_deprecated_at(
                        "ReflectionClass::isSubclassOf(): Passing null to parameter #1 ($class) of type ReflectionClass|string is deprecated",
                        vm.current_line,
                    );
                }
                let parent_name = match parent_arg {
                    Value::Object(o) => {
                        let ob = o.borrow();
                        // ReflectionClass/ReflectionObject (and other reflectors
                        // that subclass them) carry the target class in `name`.
                        let cn = &ob.class_name;
                        if cn.eq_ignore_ascii_case(b"ReflectionClass")
                            || cn.eq_ignore_ascii_case(b"ReflectionObject")
                            || cn.eq_ignore_ascii_case(b"ReflectionEnum")
                        {
                            ob.get_property(b"name").to_php_string().to_string_lossy()
                        } else {
                            String::from_utf8_lossy(&ob.class_name).to_string()
                        }
                    }
                    _ => parent_arg.to_php_string().to_string_lossy(),
                };
                let parent_lower: Vec<u8> = parent_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                // Validate that the parent class exists (PHP throws ReflectionException otherwise)
                let parent_exists = vm.classes.contains_key(&parent_lower)
                    || vm.is_known_builtin_class(&parent_lower);
                if !parent_exists {
                    let err_msg = format!("Class \"{}\" does not exist", parent_name);
                    let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                // A class is not a subclass of itself
                if class_lower == parent_lower {
                    return Some(Value::False);
                }
                let result = vm.class_extends(&class_lower, &parent_lower)
                    || vm.class_implements_interface(&class_lower, &parent_lower)
                    || super::vm::is_builtin_subclass(&class_lower, &parent_lower);
                Some(if result { Value::True } else { Value::False })
            }
            b"implementsinterface" => {
                let user_arg_count = args.len().saturating_sub(1);
                if user_arg_count == 0 {
                    let err_msg = "ReflectionClass::implementsInterface() expects exactly 1 argument, 0 given".to_string();
                    let exc = vm.create_exception(b"ArgumentCountError", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                if user_arg_count > 1 {
                    let err_msg = format!("ReflectionClass::implementsInterface() expects exactly 1 argument, {} given", user_arg_count);
                    let exc = vm.create_exception(b"ArgumentCountError", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                let iface_arg = args.get(1)?;
                // Null triggers deprecation, then becomes ""
                if matches!(iface_arg, Value::Null) {
                    vm.emit_deprecated_at(
                        "ReflectionClass::implementsInterface(): Passing null to parameter #1 ($interface) of type ReflectionClass|string is deprecated",
                        vm.current_line,
                    );
                }
                let iface_name = match iface_arg {
                    Value::Object(o) => {
                        let ob = o.borrow();
                        if ob.class_name.eq_ignore_ascii_case(b"ReflectionClass") {
                            ob.get_property(b"name").to_php_string().to_string_lossy()
                        } else {
                            String::from_utf8_lossy(&ob.class_name).to_string()
                        }
                    }
                    _ => iface_arg.to_php_string().to_string_lossy(),
                };
                let iface_lower: Vec<u8> = iface_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

                // PHP triggers autoload before reporting an interface as
                // missing from implementsInterface().
                if !vm.classes.contains_key(&iface_lower)
                    && !vm.is_known_builtin_class(&iface_lower)
                {
                    vm.try_autoload_class(iface_name.as_bytes());
                }

                // Check if the target is actually an interface
                let is_interface = vm.classes.get(&iface_lower)
                    .map(|c| c.is_interface)
                    .unwrap_or_else(|| vm.builtin_is_interface(&iface_lower));

                if !is_interface {
                    // Check if class exists
                    let exists = vm.classes.contains_key(&iface_lower) || vm.is_known_builtin_class(&iface_lower);
                    if exists {
                        let err_msg = format!("{} is not an interface", iface_name);
                        let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    } else {
                        let err_msg = format!("Interface \"{}\" does not exist", iface_name);
                        let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                }

                // An interface "implements" itself per PHP semantics.
                let result = class_lower == iface_lower
                    || vm.class_implements_interface(&class_lower, &iface_lower)
                    || vm.builtin_implements_interface(&class_lower, &iface_lower);
                Some(if result { Value::True } else { Value::False })
            }
            b"isinstance" => {
                let instance = args.get(1)?;
                if let Value::Object(inst_obj) = instance {
                    let inst_class: Vec<u8> = inst_obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                    let result = inst_class == class_lower
                        || vm.class_extends(&inst_class, &class_lower)
                        || vm.class_implements_interface(&inst_class, &class_lower)
                        || super::vm::is_builtin_subclass(&inst_class, &class_lower);
                    Some(if result { Value::True } else { Value::False })
                } else {
                    Some(Value::False)
                }
            }
            b"newinstance" => {
                // Internal types that cannot be created via reflection at all.
                let no_new: &[(&[u8], &str)] = &[
                    (b"generator", "Generator"),
                    (b"fiber", "Fiber"),
                ];
                if let Some((_, name)) = no_new.iter().find(|(c, _)| *c == class_lower.as_slice()) {
                    let err_msg = format!(
                        "The \"{}\" class is reserved for internal use and cannot be manually instantiated",
                        name
                    );
                    let exc = vm.create_exception(b"Error", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                // Create instance and call constructor with remaining args
                let obj_id = vm.next_object_id();
                let canonical = vm.classes.get(&class_lower)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| vm.builtin_canonical_name(&class_lower).into_bytes());

                // Validate abstract/interface
                let (is_abstract, is_interface, is_trait) = vm.classes.get(&class_lower)
                    .map(|c| (c.is_abstract, c.is_interface, c.is_trait))
                    .unwrap_or((false, false, false));
                if is_interface || is_abstract || is_trait {
                    let canonical_name = String::from_utf8_lossy(&canonical).to_string();
                    let err_msg = format!("Cannot instantiate {} {}",
                        if is_interface { "interface" }
                        else if is_trait { "trait" }
                        else { "abstract class" },
                        canonical_name);
                    let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }

                let has_constructor = vm.classes.get(&class_lower)
                    .map(|c| c.get_method(b"__construct").is_some())
                    .unwrap_or(false);

                // If no constructor and args were passed, throw
                if !has_constructor && args.len() > 1 {
                    let canonical_name = String::from_utf8_lossy(&canonical).to_string();
                    let err_msg = format!("Class {} does not have a constructor, so you cannot pass any constructor arguments", canonical_name);
                    let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }

                // Check constructor visibility
                if has_constructor {
                    let ctor_visibility = vm.classes.get(&class_lower)
                        .and_then(|c| c.get_method(b"__construct"))
                        .map(|m| m.visibility.clone());
                    if matches!(ctor_visibility, Some(Visibility::Protected) | Some(Visibility::Private)) {
                        let canonical_name = String::from_utf8_lossy(&canonical).to_string();
                        let err_msg = format!("Access to non-public constructor of class {}", canonical_name);
                        let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                }

                let mut new_obj = PhpObject::new(canonical.clone(), obj_id);
                // Set default property values
                if let Some(ce) = vm.classes.get(&class_lower) {
                    let props: Vec<_> = ce.properties.iter()
                        .filter(|p| !p.is_static)
                        .map(|p| (p.name.clone(), p.default.clone()))
                        .collect();
                    for (name, default) in props {
                        let resolved = vm.resolve_deferred_value(&default);
                        new_obj.set_property(name, resolved);
                    }
                }
                let new_val = Value::Object(Rc::new(RefCell::new(new_obj)));

                // Call constructor if it exists
                if has_constructor {
                    let ctor = vm.classes.get(&class_lower)
                        .and_then(|c| c.get_method(b"__construct"))
                        .cloned();
                    if let Some(ctor_method) = ctor {
                        // Validate argument count: PHP throws ArgumentCountError
                        // when too few constructor args are provided.
                        let provided = args.len().saturating_sub(1);
                        // For non-static methods param_count counts $this too,
                        // so subtract 1; required_param_count already excludes $this.
                        let required = ctor_method.op_array.required_param_count as usize;
                        if provided < required {
                            let canonical_name = String::from_utf8_lossy(&canonical).to_string();
                            let raw_param_count = ctor_method.op_array.param_count as usize;
                            let total_decl = raw_param_count
                                .saturating_sub(if ctor_method.is_static { 0 } else { 1 });
                            let quantifier = if total_decl == required { "exactly" } else { "at least" };
                            let err_msg = format!(
                                "Too few arguments to function {}::__construct(), {} passed and {} {} expected",
                                canonical_name, provided, quantifier, required
                            );
                            let exc = vm.create_exception(b"ArgumentCountError", &err_msg, vm.current_line);
                            vm.current_exception = Some(exc);
                            return Some(Value::Null);
                        }
                        let mut ctor_args = vec![new_val.clone()];
                        for arg in args.iter().skip(1) {
                            ctor_args.push(arg.clone());
                        }
                        let ctor_key = {
                            let mut key = class_lower.clone();
                            key.extend_from_slice(b"::__construct");
                            key
                        };
                        vm.user_functions.insert(ctor_key.clone(), ctor_method.op_array.clone());
                        let mut cvs = vec![Value::Undef; ctor_method.op_array.cv_names.len()];
                        for (i, arg) in ctor_args.iter().enumerate() {
                            if i < cvs.len() {
                                cvs[i] = arg.clone();
                            }
                        }
                        let _ = vm.execute_op_array_pub(&ctor_method.op_array, cvs);
                    }
                }

                Some(new_val)
            }
            b"newinstanceargs" => {
                // Same as newInstance but takes an array of args
                // Validate the argument is an array (or null/missing)
                let args_arr = match args.get(1) {
                    Some(Value::Array(a)) => Value::Array(a.clone()),
                    Some(Value::Null) | None => Value::Array(Rc::new(RefCell::new(PhpArray::new()))),
                    Some(other) => {
                        let got = match other {
                            Value::Long(_) => "int",
                            Value::Double(_) => "float",
                            Value::String(_) => "string",
                            Value::True | Value::False => "bool",
                            Value::Object(o) => {
                                let _ob = o.borrow();
                                "object"
                            }
                            _ => "unknown",
                        };
                        let err_msg = format!(
                            "ReflectionClass::newInstanceArgs(): Argument #1 ($args) must be of type array, {} given",
                            got
                        );
                        let exc = vm.create_exception(b"TypeError", &err_msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                };
                let ctor_args: Vec<Value> = if let Value::Array(arr) = &args_arr {
                    arr.borrow().iter().map(|(_, v)| v.clone()).collect()
                } else {
                    vec![]
                };

                let obj_id = vm.next_object_id();
                let canonical = vm.classes.get(&class_lower)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| vm.builtin_canonical_name(&class_lower).into_bytes());

                // Validate abstract/interface
                let (is_abstract, is_interface, is_trait) = vm.classes.get(&class_lower)
                    .map(|c| (c.is_abstract, c.is_interface, c.is_trait))
                    .unwrap_or((false, false, false));
                if is_interface || is_abstract || is_trait {
                    let canonical_name = String::from_utf8_lossy(&canonical).to_string();
                    let err_msg = format!("Cannot instantiate {} {}",
                        if is_interface { "interface" }
                        else if is_trait { "trait" }
                        else { "abstract class" },
                        canonical_name);
                    let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }

                let has_constructor = vm.classes.get(&class_lower)
                    .map(|c| c.get_method(b"__construct").is_some())
                    .unwrap_or(false);

                // If no constructor and args were passed, throw
                if !has_constructor && !ctor_args.is_empty() {
                    let canonical_name = String::from_utf8_lossy(&canonical).to_string();
                    let err_msg = format!("Class {} does not have a constructor, so you cannot pass any constructor arguments", canonical_name);
                    let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }

                // Check constructor visibility
                if has_constructor {
                    let ctor_visibility = vm.classes.get(&class_lower)
                        .and_then(|c| c.get_method(b"__construct"))
                        .map(|m| m.visibility.clone());
                    if matches!(ctor_visibility, Some(Visibility::Protected) | Some(Visibility::Private)) {
                        let canonical_name = String::from_utf8_lossy(&canonical).to_string();
                        let err_msg = format!("Access to non-public constructor of class {}", canonical_name);
                        let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                }

                let mut new_obj = PhpObject::new(canonical.clone(), obj_id);
                if let Some(ce) = vm.classes.get(&class_lower) {
                    let props: Vec<_> = ce.properties.iter()
                        .filter(|p| !p.is_static)
                        .map(|p| (p.name.clone(), p.default.clone()))
                        .collect();
                    for (name, default) in props {
                        let resolved = vm.resolve_deferred_value(&default);
                        new_obj.set_property(name, resolved);
                    }
                }
                let new_val = Value::Object(Rc::new(RefCell::new(new_obj)));

                if has_constructor {
                    let ctor = vm.classes.get(&class_lower)
                        .and_then(|c| c.get_method(b"__construct"))
                        .cloned();
                    if let Some(ctor_method) = ctor {
                        // Validate argument count.
                        let provided = ctor_args.len();
                        let required = ctor_method.op_array.required_param_count as usize;
                        if provided < required {
                            let canonical_name = String::from_utf8_lossy(&canonical).to_string();
                            let raw_param_count = ctor_method.op_array.param_count as usize;
                            let total_decl = raw_param_count
                                .saturating_sub(if ctor_method.is_static { 0 } else { 1 });
                            let quantifier = if total_decl == required { "exactly" } else { "at least" };
                            let err_msg = format!(
                                "Too few arguments to function {}::__construct(), {} passed and {} {} expected",
                                canonical_name, provided, quantifier, required
                            );
                            let exc = vm.create_exception(b"ArgumentCountError", &err_msg, vm.current_line);
                            vm.current_exception = Some(exc);
                            return Some(Value::Null);
                        }
                        let mut all_args = vec![new_val.clone()];
                        all_args.extend(ctor_args);
                        let mut cvs = vec![Value::Undef; ctor_method.op_array.cv_names.len()];
                        for (i, arg) in all_args.iter().enumerate() {
                            if i < cvs.len() {
                                cvs[i] = arg.clone();
                            }
                        }
                        let _ = vm.execute_op_array_pub(&ctor_method.op_array, cvs);
                    }
                }

                Some(new_val)
            }
            b"resetaslazyghost" | b"resetaslazyproxy" => {
                // Reset an existing object to the uninitialized lazy state.
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                let initializer = args.get(2).cloned().unwrap_or(Value::Null);
                let options = args.get(3).map(|v| v.to_long()).unwrap_or(0);
                let is_proxy = method == b"resetaslazyproxy";
                // Validate options:
                //   - resetAsLazyGhost: only SKIP_INITIALIZATION_ON_SERIALIZE (1) allowed
                //   - resetAsLazyProxy: SKIP_INITIALIZATION_ON_SERIALIZE (1) | SKIP_DESTRUCTOR (2)
                let valid_mask = if is_proxy { 3 } else { 1 };
                if options & !valid_mask != 0 {
                    let method_name = if is_proxy { "resetAsLazyProxy" } else { "resetAsLazyGhost" };
                    let err_msg = format!(
                        "ReflectionClass::{}(): Argument #3 ($options) contains invalid flags",
                        method_name
                    );
                    let exc = vm.create_exception(b"ReflectionException", &err_msg, 0);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                if let Value::Object(obj) = &target {
                    // PHP: classes with no declared instance properties are never lazy
                    // — resetAsLazy*() is a no-op (object remains non-lazy).
                    let decl_prop_count = vm.classes.get(&class_lower)
                        .map(|ce| ce.properties.iter().filter(|p| !p.is_static).count())
                        .unwrap_or(0);
                    if decl_prop_count == 0 {
                        // Still wipe dynamic properties to match expected behavior
                        let mut ob = obj.borrow_mut();
                        let names: Vec<Vec<u8>> = ob.properties.iter()
                            .filter(|(n, _)| !n.starts_with(b"__lazy_"))
                            .map(|(n, _)| n.clone())
                            .collect();
                        for n in names {
                            ob.remove_property(&n);
                        }
                        ob.remove_property(b"__lazy_state");
                        ob.remove_property(b"__lazy_initializer");
                        ob.remove_property(b"__lazy_real");
                        return Some(Value::Null);
                    }
                    let mut ob = obj.borrow_mut();
                    // Clear existing state
                    ob.remove_property(b"__lazy_real");
                    // Wipe all user properties and re-seed as Undef in declaration order
                    let names: Vec<Vec<u8>> = ob.properties.iter()
                        .filter(|(n, _)| !n.starts_with(b"__lazy_"))
                        .map(|(n, _)| n.clone())
                        .collect();
                    for n in names {
                        ob.remove_property(&n);
                    }
                    if let Some(ce) = vm.classes.get(&class_lower) {
                        let decl_props: Vec<_> = ce.properties.iter()
                            .filter(|p| !p.is_static)
                            .map(|p| p.name.clone())
                            .collect();
                        for n in decl_props {
                            ob.set_property(n, Value::Undef);
                        }
                    }
                    ob.set_property(
                        b"__lazy_state".to_vec(),
                        Value::String(PhpString::from_bytes(
                            if is_proxy { b"proxy" } else { b"ghost" },
                        )),
                    );
                    ob.set_property(b"__lazy_initializer".to_vec(), initializer);
                }
                Some(Value::Null)
            }
            b"newlazyghost" | b"newlazyproxy" => {
                let initializer = args.get(1).cloned().unwrap_or(Value::Null);
                let options = args.get(2).map(|v| v.to_long()).unwrap_or(0);
                let is_proxy = method == b"newlazyproxy";
                // PHP: internal classes cannot be initialized lazily.
                // stdClass is whitelisted as it is considered safe.
                if !vm.classes.contains_key(&class_lower) && class_lower != b"stdclass" {
                    let canonical = vm.builtin_canonical_name(&class_lower);
                    let err_msg = format!(
                        "Cannot make instance of internal class lazy: {} is internal",
                        canonical
                    );
                    let exc = vm.create_exception(b"Error", &err_msg, 0);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                // PHP: sub-classes of internal classes (other than stdClass) cannot
                // be initialized lazily either.
                {
                    let mut cur = class_lower.clone();
                    let canonical_self = vm.classes.get(&class_lower)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| vm.builtin_canonical_name(&class_lower).into_bytes());
                    let self_disp = String::from_utf8_lossy(&canonical_self).to_string();
                    let mut bad_internal_ancestor: Option<String> = None;
                    while let Some(ce) = vm.classes.get(&cur) {
                        if let Some(parent_lower) = &ce.parent {
                            let pl: Vec<u8> = parent_lower.iter().map(|b| b.to_ascii_lowercase()).collect();
                            if pl == b"stdclass" {
                                break;
                            }
                            if !vm.classes.contains_key(&pl) {
                                // Parent is internal
                                let parent_disp = vm.builtin_canonical_name(&pl);
                                bad_internal_ancestor = Some(parent_disp);
                                break;
                            }
                            cur = pl;
                        } else {
                            break;
                        }
                    }
                    if let Some(parent_name) = bad_internal_ancestor {
                        let err_msg = format!(
                            "Cannot make instance of internal class lazy: {} inherits internal class {}",
                            self_disp, parent_name
                        );
                        let exc = vm.create_exception(b"Error", &err_msg, 0);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                }
                // Validate options: only SKIP_INITIALIZATION_ON_SERIALIZE (1)
                // is allowed for newLazyGhost/newLazyProxy. SKIP_DESTRUCTOR (2)
                // is only allowed on resetAsLazyProxy.
                if options & !1 != 0 {
                    let method_name = if is_proxy { "newLazyProxy" } else { "newLazyGhost" };
                    let err_msg = if options & 2 != 0 && options & !3 == 0 {
                        format!(
                            "ReflectionClass::{}(): Argument #2 ($options) does not accept ReflectionClass::SKIP_DESTRUCTOR",
                            method_name
                        )
                    } else {
                        format!(
                            "ReflectionClass::{}(): Argument #2 ($options) contains invalid flags",
                            method_name
                        )
                    };
                    let exc = vm.create_exception(b"ReflectionException", &err_msg, 0);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                let obj_id = vm.next_object_id();
                let canonical = vm.classes.get(&class_lower)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| vm.builtin_canonical_name(&class_lower).into_bytes());
                let mut new_obj = PhpObject::new(canonical, obj_id);
                // PHP: classes with no declared instance properties are never lazy
                // — they are returned as already-initialized objects (no initializer).
                let decl_prop_count = vm.classes.get(&class_lower)
                    .map(|ce| ce.properties.iter().filter(|p| !p.is_static).count())
                    .unwrap_or(0);
                if decl_prop_count == 0 {
                    return Some(Value::Object(Rc::new(RefCell::new(new_obj))));
                }
                // For lazy objects, all declared properties are seeded as Undef
                // in declaration order so later assignments preserve that order.
                // Typed properties will render as uninitialized() in var_dump
                // until the initializer runs.
                if let Some(ce) = vm.classes.get(&class_lower) {
                    let decl_props: Vec<_> = ce.properties.iter()
                        .filter(|p| !p.is_static)
                        .map(|p| p.name.clone())
                        .collect();
                    for name in decl_props {
                        new_obj.set_property(name, Value::Undef);
                    }
                }
                new_obj.set_property(
                    b"__lazy_state".to_vec(),
                    Value::String(PhpString::from_bytes(
                        if is_proxy { b"proxy" } else { b"ghost" },
                    )),
                );
                new_obj.set_property(b"__lazy_initializer".to_vec(), initializer);
                Some(Value::Object(Rc::new(RefCell::new(new_obj))))
            }
            b"initializelazyobject" => {
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                if let Value::Object(inner) = &target {
                    vm.maybe_run_lazy_init(inner);
                    // For proxies, return the real backing instance.
                    let ob = inner.borrow();
                    let state = ob.get_property(b"__lazy_state");
                    let is_proxy = matches!(&state, Value::String(s) if s.as_bytes() == b"proxy");
                    if is_proxy {
                        if let Value::Object(real) = ob.get_property(b"__lazy_real") {
                            return Some(Value::Object(real));
                        }
                    }
                }
                Some(target)
            }
            b"marklazyobjectasinitialized" => {
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                if let Value::Object(inner) = &target {
                    // Initialize properties to their default values from the class
                    // definition, skipping the initializer.
                    let defaults: Vec<(Vec<u8>, Value)> = vm.classes.get(&class_lower)
                        .map(|ce| ce.properties.iter()
                            .filter(|p| !p.is_static && !matches!(p.default, Value::Undef))
                            .map(|p| (p.name.clone(), p.default.clone()))
                            .collect())
                        .unwrap_or_default();
                    let mut ob = inner.borrow_mut();
                    for (name, val) in defaults {
                        ob.set_property(name, val);
                    }
                    ob.remove_property(b"__lazy_state");
                    ob.remove_property(b"__lazy_initializer");
                }
                Some(target)
            }
            b"isuninitializedlazyobject" => {
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                if let Value::Object(inner) = &target {
                    let ob = inner.borrow();
                    let state = ob.get_property(b"__lazy_state");
                    let is_ghost = matches!(state, Value::String(ref s) if s.as_bytes() == b"ghost");
                    let is_uninit_proxy = matches!(state, Value::String(ref s) if s.as_bytes() == b"proxy")
                        && !ob.has_property(b"__lazy_real");
                    Some(if is_ghost || is_uninit_proxy { Value::True } else { Value::False })
                } else {
                    Some(Value::False)
                }
            }
            b"getlazyinitializer" => {
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                if let Value::Object(inner) = &target {
                    let ob = inner.borrow();
                    let state = ob.get_property(b"__lazy_state");
                    let is_uninit = match &state {
                        Value::String(s) if s.as_bytes() == b"ghost" => true,
                        Value::String(s) if s.as_bytes() == b"proxy" => !ob.has_property(b"__lazy_real"),
                        _ => false,
                    };
                    if is_uninit {
                        Some(ob.get_property(b"__lazy_initializer"))
                    } else {
                        Some(Value::Null)
                    }
                } else {
                    Some(Value::Null)
                }
            }
            b"getlazyproxyinstance" => {
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                if let Value::Object(inner) = &target {
                    let ob = inner.borrow();
                    let real = ob.get_property(b"__lazy_real");
                    if matches!(real, Value::Object(_)) {
                        Some(real)
                    } else {
                        Some(Value::Null)
                    }
                } else {
                    Some(Value::Null)
                }
            }
            b"getstaticpropertyvalue" => {
                if !check_arg_count_range(vm, args, 1, 2, "ReflectionClass::getStaticPropertyValue") {
                    return Some(Value::Null);
                }
                let prop_name_s = match require_string_arg(vm, args, 0, "ReflectionClass::getStaticPropertyValue", "name", "string", true) {
                    Some(s) => s,
                    None => return Some(Value::Null),
                };
                let prop_name = PhpString::from_string(prop_name_s);
                let default = args.get(2);
                // First check if the property is declared as a static prop
                // on this class (or an ancestor).
                let declared_static = vm.classes.get(&class_lower)
                    .map(|ce| ce.properties.iter().any(|p| p.is_static && p.name == prop_name.as_bytes()))
                    .unwrap_or(false);
                if let Some(ce) = vm.classes.get(&class_lower) {
                    if let Some(val) = ce.static_properties.get(prop_name.as_bytes()) {
                        return Some(val.clone());
                    }
                    if declared_static {
                        // Property exists but has no value: return Null.
                        return Some(Value::Null);
                    }
                }
                // Property doesn't exist on the class. PHP: if a default
                // was provided, return it; otherwise throw.
                if let Some(d) = default {
                    return Some(d.clone());
                }
                let err_msg = format!("Property {}::${} does not exist", target, prop_name.to_string_lossy());
                let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                vm.current_exception = Some(exc);
                Some(Value::Null)
            }
            b"setstaticpropertyvalue" => {
                if !check_arg_count(vm, args, 2, "ReflectionClass::setStaticPropertyValue") {
                    return Some(Value::Null);
                }
                let prop_name_s = match require_string_arg(vm, args, 0, "ReflectionClass::setStaticPropertyValue", "name", "string", true) {
                    Some(s) => s,
                    None => return Some(Value::Null),
                };
                let prop_name = PhpString::from_string(prop_name_s.clone());
                let value = args.get(2).cloned().unwrap_or(Value::Null);
                // Validate that this static prop exists on the class (or an ancestor)
                let declared = vm.classes.get(&class_lower)
                    .map(|ce| ce.properties.iter().any(|p| p.is_static && p.name == prop_name.as_bytes()))
                    .unwrap_or(false);
                if !declared {
                    let canonical = vm.classes.get(&class_lower)
                        .map(|c| String::from_utf8_lossy(&c.name).to_string())
                        .unwrap_or(target.clone());
                    let err_msg = format!("Class {} does not have a property named {}", canonical, prop_name_s);
                    let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                // Enforce property type if declared
                let prop_type = vm.classes.get(&class_lower)
                    .and_then(|ce| ce.properties.iter().find(|p| p.is_static && p.name == prop_name.as_bytes()))
                    .and_then(|p| p.property_type.clone());
                let mut coerced = value;
                if let Some(pt) = prop_type.as_ref() {
                    let strict = false;
                    let original = coerced.clone();
                    crate::vm::Vm::coerce_value_to_type(&mut coerced, pt, strict);
                    if !vm.value_matches_type(&coerced, pt) {
                        let canonical = vm.classes.get(&class_lower)
                            .map(|c| String::from_utf8_lossy(&c.name).to_string())
                            .unwrap_or(target.clone());
                        let type_str = param_type_name(pt);
                        let value_type = match &original {
                            Value::Null => "null",
                            Value::True | Value::False => "bool",
                            Value::Long(_) => "int",
                            Value::Double(_) => "float",
                            Value::String(_) => "string",
                            Value::Array(_) => "array",
                            Value::Object(_) => "object",
                            _ => "unknown",
                        };
                        let err_msg = format!("Cannot assign {} to property {}::${} of type {}",
                            value_type, canonical, prop_name_s, type_str);
                        let exc = vm.create_exception(b"TypeError", &err_msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                }
                if let Some(ce) = vm.classes.get_mut(&class_lower) {
                    ce.static_properties.insert(prop_name.as_bytes().to_vec(), coerced);
                }
                Some(Value::Null)
            }
            b"getreflectionconstant" | b"getreflectionconstants" => {
                // Return ReflectionClassConstant objects
                let mut result = PhpArray::new();
                if method == b"getreflectionconstant" {
                    let const_name = args.get(1)?.to_php_string();
                    let mut val = reflection_class_get_constant(vm, &class_lower, const_name.as_bytes());
                    if val.is_none() {
                        // Fall back to built-in class constants.
                        val = vm.get_builtin_class_constant(&class_lower, const_name.as_bytes());
                    }
                    if let Some(v) = val {
                        return Some(create_reflection_class_constant(vm, &target, &const_name.to_string_lossy(), v));
                    } else {
                        return Some(Value::False);
                    }
                }
                // getReflectionConstants
                if let Some(ce) = vm.classes.get(&class_lower) {
                    for (name, val) in ce.constants.clone() {
                        let const_name = String::from_utf8_lossy(&name).to_string();
                        result.push(create_reflection_class_constant(vm, &target, &const_name, val));
                    }
                }
                Some(Value::Array(Rc::new(RefCell::new(result))))
            }
            b"hascase" => {
                let case_name = args.get(1)?.to_php_string();
                let has = vm.classes.get(&class_lower)
                    .map(|ce| ce.enum_cases.iter().any(|(cn, _)| cn == case_name.as_bytes()))
                    .unwrap_or(false);
                Some(if has { Value::True } else { Value::False })
            }
            b"getcase" => {
                let case_name = args.get(1)?.to_php_string().to_string_lossy();
                if let Some(ce) = vm.classes.get(&class_lower) {
                    let is_case = ce.enum_cases.iter().any(|(cn, _)| cn == case_name.as_bytes());
                    if is_case {
                        let canonical = String::from_utf8_lossy(&ce.name).to_string();
                        let is_backed = ce.enum_backing_type.is_some();
                        let class_type = if is_backed { "ReflectionEnumBackedCase" } else { "ReflectionEnumUnitCase" };
                        let obj_id = vm.next_object_id();
                        let mut case_obj = PhpObject::new(class_type.as_bytes().to_vec(), obj_id);
                        case_obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(case_name.clone())));
                        case_obj.set_property(b"class".to_vec(), Value::String(PhpString::from_string(canonical)));
                        let case_val = reflection_class_get_constant(vm, &class_lower, case_name.as_bytes());
                        if let Some(cv) = case_val {
                            case_obj.set_property(b"__reflection_value".to_vec(), cv);
                        }
                        Some(Value::Object(Rc::new(RefCell::new(case_obj))))
                    } else {
                        // Check if it's a constant but not a case
                        let is_const = ce.constants.contains_key(case_name.as_bytes());
                        let canonical = String::from_utf8_lossy(&ce.name).to_string();
                        if is_const {
                            let err_msg = format!("{}::{} is not a case", canonical, case_name);
                            let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                            vm.current_exception = Some(exc);
                        } else {
                            let err_msg = format!("Case {}::{} does not exist", canonical, case_name);
                            let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                            vm.current_exception = Some(exc);
                        }
                        Some(Value::Null)
                    }
                } else {
                    Some(Value::Null)
                }
            }
            b"getattributes" => {
                let attrs = vm.classes.get(&class_lower)
                    .map(|c| c.attributes.clone())
                    .unwrap_or_default();
                let filter_name = args.get(1).and_then(|v| if matches!(v, Value::Null) { None } else { Some(v.to_php_string().as_bytes().to_vec()) });
                let filter_flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);
                Some(create_reflection_attributes(vm, &attrs, filter_name.as_deref(), filter_flags, 1))
            }
            _ => None,
        }
    } else {
        None
    }
}

/// ReflectionMethod no-arg method dispatch
pub fn reflection_method_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    let class_name = ob.get_property(b"__reflection_class").to_php_string().to_string_lossy();
    let method_lower_val = ob.get_property(b"__reflection_method");
    let method_lower = method_lower_val.to_php_string();
    let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    // Optional closure back-reference, set when this ReflectionMethod came
    // from a Closure::__invoke lookup.
    let closure_fn: Option<Vec<u8>> = match ob.get_property(b"__reflection_closure_fn") {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        _ => None,
    };
    drop(ob);

    // For Closure::__invoke: delegate param/return/doc queries to the
    // underlying user-defined function by rewriting the class/method keys
    // to look up in vm.user_functions.
    if let Some(ref fn_lower) = closure_fn {
        match method {
            b"getnumberofparameters" => {
                let count = vm.user_functions.get(fn_lower.as_slice())
                    .map(|op| op.param_count as i64).unwrap_or(0);
                return Some(Value::Long(count));
            }
            b"getnumberofrequiredparameters" => {
                let count = vm.user_functions.get(fn_lower.as_slice())
                    .map(|op| op.required_param_count as i64).unwrap_or(0);
                return Some(Value::Long(count));
            }
            b"getparameters" => {
                if let Some(op_array) = vm.user_functions.get(fn_lower.as_slice()).cloned() {
                    return Some(create_reflection_parameters(vm, &op_array));
                }
                return Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
            }
            b"getreturntype" => {
                let rt = vm.user_functions.get(fn_lower.as_slice())
                    .and_then(|op| op.return_type.as_ref()).cloned();
                return Some(match rt {
                    Some(rt) => create_reflection_type(vm, &rt),
                    None => Value::Null,
                });
            }
            b"hasreturntype" => {
                let has = vm.user_functions.get(fn_lower.as_slice())
                    .and_then(|op| op.return_type.as_ref()).is_some();
                return Some(if has { Value::True } else { Value::False });
            }
            b"isstatic" => {
                return Some(Value::False);
            }
            b"isvariadic" => {
                let is_var = vm.user_functions.get(fn_lower.as_slice())
                    .and_then(|op| op.variadic_param).is_some();
                return Some(if is_var { Value::True } else { Value::False });
            }
            b"getfilename" => {
                let fname = vm.user_functions.get(fn_lower.as_slice())
                    .map(|op| String::from_utf8_lossy(&op.filename).to_string())
                    .filter(|f| !f.is_empty())
                    .unwrap_or_else(|| vm.current_file.clone());
                return Some(Value::String(PhpString::from_string(fname)));
            }
            b"getstartline" => {
                let line = vm.user_functions.get(fn_lower.as_slice())
                    .map(|op| op.decl_line as i64).unwrap_or(0);
                return Some(Value::Long(line));
            }
            _ => {}
        }
    }

    match method {
        b"getname" => {
            let ob = obj.borrow();
            Some(ob.get_property(b"name"))
        }
        b"getdeclaringclass" => {
            let class = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes())
                    .map(|m| String::from_utf8_lossy(&m.declaring_class).to_string()))
                .unwrap_or(class_name.clone());
            let declaring_canonical = vm.classes.get(class.as_bytes())
                .map(|c| String::from_utf8_lossy(&c.name).to_string())
                .unwrap_or(class.clone());
            Some(create_reflection_class(vm, &declaring_canonical))
        }
        b"ispublic" => {
            let vis = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| m.visibility))
                .unwrap_or(Visibility::Public);
            Some(if vis == Visibility::Public { Value::True } else { Value::False })
        }
        b"isprotected" => {
            let vis = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| m.visibility))
                .unwrap_or(Visibility::Public);
            Some(if vis == Visibility::Protected { Value::True } else { Value::False })
        }
        b"isprivate" => {
            let vis = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| m.visibility))
                .unwrap_or(Visibility::Public);
            Some(if vis == Visibility::Private { Value::True } else { Value::False })
        }
        b"isstatic" => {
            let is_static = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| m.is_static))
                .unwrap_or(false);
            Some(if is_static { Value::True } else { Value::False })
        }
        b"isabstract" => {
            let is_abstract = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| m.is_abstract))
                .unwrap_or(false);
            Some(if is_abstract { Value::True } else { Value::False })
        }
        b"isfinal" => {
            let is_final = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| m.is_final))
                .unwrap_or(false);
            Some(if is_final { Value::True } else { Value::False })
        }
        b"isconstructor" => {
            Some(if method_lower.as_bytes() == b"__construct" { Value::True } else { Value::False })
        }
        b"isdestructor" => {
            Some(if method_lower.as_bytes() == b"__destruct" { Value::True } else { Value::False })
        }
        b"getmodifiers" => {
            let mods = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| reflection_method_modifiers(m)))
                .unwrap_or(1); // default public
            Some(Value::Long(mods))
        }
        b"getnumberofparameters" => {
            let method_def = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()));
            let count = method_def.map(|m| {
                let skip = if !m.is_static { 1u32 } else { 0u32 };
                m.op_array.param_count.saturating_sub(skip)
            }).unwrap_or(0);
            Some(Value::Long(count as i64))
        }
        b"getnumberofrequiredparameters" => {
            let method_def = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()));
            // The compiler stores `required_param_count` as the count of
            // user-declared parameters without defaults — it does NOT include
            // the implicit `$this` slot. So we should NOT subtract for
            // instance methods.
            let count = method_def.map(|m| m.op_array.required_param_count).unwrap_or(0);
            Some(Value::Long(count as i64))
        }
        b"getparameters" => {
            let method_data = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()))
                .map(|m| (m.op_array.clone(), m.is_static));
            let params = if let Some((oa, is_static)) = method_data {
                create_reflection_parameters_method(vm, &oa, is_static)
            } else {
                Value::Array(Rc::new(RefCell::new(PhpArray::new())))
            };
            Some(params)
        }
        b"getreturntype" => {
            let ret = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()))
                .and_then(|m| m.op_array.return_type.as_ref())
                .cloned();
            // Resolve `self` to the declaring class of the method, falling
            // back to the lookup class if declaring info is unavailable.
            let scope: Vec<u8> = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()))
                .map(|m| {
                    let decl_lower: Vec<u8> = m.declaring_class.iter().map(|b| b.to_ascii_lowercase()).collect();
                    vm.classes.get(&decl_lower)
                        .map(|dc| dc.name.clone())
                        .unwrap_or_else(|| m.declaring_class.clone())
                })
                .or_else(|| vm.classes.get(&class_lower).map(|c| c.name.clone()))
                .unwrap_or_else(|| class_lower.clone());
            if let Some(rt) = ret {
                Some(create_reflection_type_scoped(vm, &rt, Some(&scope)))
            } else {
                Some(Value::Null)
            }
        }
        b"hasreturntype" => {
            let has = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()))
                .and_then(|m| m.op_array.return_type.as_ref())
                .is_some();
            Some(if has { Value::True } else { Value::False })
        }
        b"isuserdefined" => {
            // A method is user-defined if its declaring class is user-defined.
            // For builtin classes that have no user definition, return false.
            let is_user = vm.classes.contains_key(class_lower.as_slice());
            Some(if is_user { Value::True } else { Value::False })
        }
        b"isinternal" => {
            let is_user = vm.classes.contains_key(class_lower.as_slice());
            let is_builtin = vm.is_known_builtin_class(class_lower.as_slice());
            Some(if !is_user && is_builtin { Value::True } else { Value::False })
        }
        b"returnsreference" => {
            let by_ref = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()))
                .map(|m| m.op_array.returns_by_ref)
                .unwrap_or(false);
            Some(if by_ref { Value::True } else { Value::False })
        }
        b"getfilename" => {
            // For methods on internal/built-in classes, return false.
            let is_user = vm.classes.contains_key(class_lower.as_slice());
            if !is_user {
                return Some(Value::False);
            }
            // Use the class's filename if available
            let filename = vm.classes.get(&class_lower)
                .and_then(|c| c.filename.clone())
                .unwrap_or_else(|| vm.current_file.clone());
            Some(Value::String(PhpString::from_string(filename)))
        }
        b"getstartline" => {
            let is_user = vm.classes.contains_key(class_lower.as_slice());
            if !is_user {
                return Some(Value::False);
            }
            let line = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| m.op_array.decl_line))
                .unwrap_or(0);
            Some(Value::Long(line as i64))
        }
        b"getendline" => {
            let is_user = vm.classes.contains_key(class_lower.as_slice());
            if !is_user {
                return Some(Value::False);
            }
            let line = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()))
                .map(|m| if m.op_array.end_line > 0 { m.op_array.end_line as i64 } else { m.op_array.decl_line as i64 })
                .unwrap_or(0);
            if line > 0 { Some(Value::Long(line)) } else { Some(Value::False) }
        }
        b"getdoccomment" => {
            let doc = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()))
                .and_then(|m| m.doc_comment.as_ref())
                .cloned();
            if let Some(doc) = doc {
                Some(Value::String(PhpString::from_string(doc)))
            } else {
                Some(Value::False)
            }
        }
        b"isvariadic" => {
            let is_variadic = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()))
                .and_then(|m| m.op_array.variadic_param)
                .is_some();
            Some(if is_variadic { Value::True } else { Value::False })
        }
        b"isdeprecated" => {
            // Check for the #[\Deprecated] attribute on the method.
            let has_dep = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()))
                .map(|m| {
                    m.attributes.iter().any(|a| {
                        let n = String::from_utf8_lossy(&a.name);
                        n.eq_ignore_ascii_case("deprecated") || n.eq_ignore_ascii_case("\\deprecated")
                    })
                })
                .unwrap_or(false);
            Some(if has_dep { Value::True } else { Value::False })
        }
        b"getstaticvariables" => {
            // Static variables on user methods are keyed by op_array.name (which
            // is `Class::method` for methods). Mirror reflection_function_method.
            let mut result = PhpArray::new();
            // Build the lookup prefix using the method's op_array name. We need
            // the op_array's name to match what `StaticVarInit` keyed against.
            let prefix_opt = vm.classes.get(&class_lower)
                .and_then(|ce| ce.methods.get(method_lower.as_bytes()))
                .map(|m| {
                    let mut k = m.op_array.name.clone();
                    k.extend_from_slice(b"::");
                    k
                });
            // Also extract declared static variable names in source order by
            // scanning the op_array for `StaticVarInit` opcodes.
            if let Some(prefix) = prefix_opt {
                if let Some(m) = vm.classes.get(&class_lower).and_then(|ce| ce.methods.get(method_lower.as_bytes())) {
                    use crate::opcode::{OpCode, OperandType};
                    let op_array = m.op_array.clone();
                    for op in op_array.ops.iter() {
                        if op.opcode == OpCode::StaticVarInit {
                            if let OperandType::Const(key_idx) = op.result {
                                if let Some(Value::String(key_s)) = op_array.literals.get(key_idx as usize) {
                                    let key_bytes = key_s.as_bytes();
                                    if key_bytes.len() > prefix.len() && key_bytes.starts_with(&prefix) {
                                        let var_name = &key_bytes[prefix.len()..];
                                        // Get current value (or default)
                                        let val = if let Some(v) = vm.static_vars().get(key_bytes) {
                                            v.clone()
                                        } else {
                                            // No invocation yet -> use the default literal.
                                            if let OperandType::Const(d_idx) = op.op2 {
                                                op_array.literals.get(d_idx as usize).cloned().unwrap_or(Value::Null)
                                            } else { Value::Null }
                                        };
                                        result.set(ArrayKey::String(PhpString::from_bytes(var_name)), val);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"setaccessible" => {
            vm.emit_deprecated(
                "Method ReflectionMethod::setAccessible() is deprecated since 8.5, as it has no effect since PHP 8.1",
            );
            Some(Value::Null)
        }
        b"hastentativereturntype" => {
            Some(Value::False)
        }
        b"gettentativereturntype" => {
            Some(Value::Null)
        }
        b"hasprototype" => {
            let method_lower_bytes = method_lower.as_bytes();
            let proto_exists = vm.classes.get(&class_lower)
                .map(|ce| {
                    let mut check = ce.parent.clone();
                    while let Some(ref p) = check {
                        let p_lower: Vec<u8> = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                        if let Some(pce) = vm.classes.get(&p_lower) {
                            if pce.get_method(method_lower_bytes).is_some() {
                                return true;
                            }
                            check = pce.parent.clone();
                        } else {
                            break;
                        }
                    }
                    for iface in &ce.interfaces {
                        let iface_lower: Vec<u8> = iface.iter().map(|b| b.to_ascii_lowercase()).collect();
                        if let Some(ice) = vm.classes.get(&iface_lower) {
                            if ice.get_method(method_lower_bytes).is_some() {
                                return true;
                            }
                        }
                    }
                    false
                })
                .unwrap_or(false);
            Some(if proto_exists { Value::True } else { Value::False })
        }
        b"isgenerator" => {
            let is_gen = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()))
                .map(|m| m.op_array.is_generator)
                .unwrap_or(false);
            Some(if is_gen { Value::True } else { Value::False })
        }
        b"getprototype" => {
            // Look for the method in parent classes/interfaces
            let method_lower_bytes = method_lower.as_bytes();
            let proto_class = vm.classes.get(&class_lower)
                .and_then(|ce| {
                    // Walk parent chain
                    let mut check = ce.parent.clone();
                    while let Some(ref p) = check {
                        let p_lower: Vec<u8> = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                        if let Some(pce) = vm.classes.get(&p_lower) {
                            if pce.get_method(method_lower_bytes).is_some() {
                                return Some(String::from_utf8_lossy(&pce.name).to_string());
                            }
                            check = pce.parent.clone();
                        } else {
                            break;
                        }
                    }
                    // Check interfaces
                    for iface in &ce.interfaces {
                        let iface_lower: Vec<u8> = iface.iter().map(|b| b.to_ascii_lowercase()).collect();
                        if let Some(ice) = vm.classes.get(&iface_lower) {
                            if ice.get_method(method_lower_bytes).is_some() {
                                return Some(String::from_utf8_lossy(&ice.name).to_string());
                            }
                        }
                    }
                    None
                });
            if let Some(proto) = proto_class {
                let method_name = vm.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower_bytes).map(|m| String::from_utf8_lossy(&m.name).to_string()))
                    .unwrap_or_default();
                Some(create_reflection_method(vm, &proto, &method_name))
            } else {
                // No prototype found - throw
                let ob2 = obj.borrow();
                let method_display = ob2.get_property(b"name").to_php_string().to_string_lossy();
                drop(ob2);
                let err_msg = format!("Method {}::{}() does not have a prototype", class_name, method_display);
                let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                vm.current_exception = Some(exc);
                Some(Value::Null)
            }
        }
        b"__tostring" => {
            let ob = obj.borrow();
            let name = ob.get_property(b"name").to_php_string().to_string_lossy();
            let class = ob.get_property(b"class").to_php_string().to_string_lossy();
            // If the method was looked up via ReflectionClass::getMethod on a
            // child, the lookup class is recorded here. Use it for the
            // inheritance check; otherwise fall back to __reflection_class.
            let effective_class_lower: Vec<u8> = match ob.get_property(b"__reflection_lookup_class") {
                Value::String(s) => s.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect(),
                _ => class_lower.clone(),
            };
            drop(ob);

            let method_def = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()))
                .cloned();

            if let Some(m) = method_def {
                let vis = match m.visibility {
                    Visibility::Public => "public",
                    Visibility::Protected => "protected",
                    Visibility::Private => "private",
                };
                let static_str = if m.is_static { "static " } else { "" };
                let modifiers = format!("{}{}{}{}",
                    if m.is_abstract { "abstract " } else { "" },
                    if m.is_final { "final " } else { "" },
                    static_str,
                    vis,
                );
                // Determine if this method is inherited from a different
                // declaring class — for "<user, inherits Parent>" header.
                let declaring_lower = &m.declaring_class;
                let owner_display = if declaring_lower != &effective_class_lower {
                    vm.classes.get(declaring_lower.as_slice())
                        .map(|c| String::from_utf8_lossy(&c.name).to_string())
                } else {
                    None
                };
                // Find prototype: walk parent chain looking for a method with
                // the same name. The first ancestor that has it is the
                // "overwrites" target, the deepest one is the "prototype".
                // Private methods are not inherited, so don't report
                // overwrites/prototype for them.
                let mut overwrites_name: Option<String> = None;
                let mut prototype_name: Option<String> = None;
                if owner_display.is_none() && m.visibility != Visibility::Private {
                    if let Some(ce) = vm.classes.get(&class_lower) {
                        let method_bytes = method_lower.as_bytes();
                        let mut cur_parent = ce.parent.clone();
                        while let Some(ref p) = cur_parent {
                            let p_lower: Vec<u8> = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                            if let Some(pce) = vm.classes.get(&p_lower) {
                                if let Some(pm) = pce.get_method(method_bytes) {
                                    // Skip ancestor private methods — they
                                    // aren't part of the visible chain.
                                    if pm.visibility != Visibility::Private {
                                        let canonical = String::from_utf8_lossy(&pce.name).to_string();
                                        if overwrites_name.is_none() {
                                            overwrites_name = Some(canonical.clone());
                                        }
                                        prototype_name = Some(canonical);
                                    }
                                }
                                cur_parent = pce.parent.clone();
                            } else {
                                break;
                            }
                        }
                    }
                }
                let scope_str = if let Some(owner) = &owner_display {
                    format!("<user, inherits {}>", owner)
                } else if let (Some(ow), Some(pt)) = (&overwrites_name, &prototype_name) {
                    format!("<user, overwrites {}, prototype {}>", ow, pt)
                } else {
                    "<user>".to_string()
                };
                let mut s = format!("Method [ {} {} method {} ] {{\n", scope_str, modifiers, name);
                let m_end = if m.op_array.end_line > 0 { m.op_array.end_line } else { m.op_array.decl_line };
                let filename_str = if m.op_array.filename.is_empty() {
                    String::from_utf8_lossy(vm.current_file.as_bytes()).into_owned()
                } else {
                    String::from_utf8_lossy(&m.op_array.filename).into_owned()
                };
                s.push_str(&format!("  @@ {} {} - {}\n", filename_str, m.op_array.decl_line, m_end));
                // Skip the implicit `$this` parameter for instance methods —
                // when counting visible parameters (param_count includes $this).
                let skip = if !m.is_static { 1usize } else { 0usize };
                let pcount_total = m.op_array.param_count as usize;
                let pcount = pcount_total.saturating_sub(skip);
                // required_param_count does NOT include $this (compiler stores
                // raw user-declared count), so do not subtract `skip` here.
                let rcount = m.op_array.required_param_count as usize;
                if pcount > 0 {
                    s.push_str(&format!("\n  - Parameters [{}] {{\n", pcount));
                    let variadic_cv = m.op_array.variadic_param;
                    for i in 0..pcount {
                        let cv_idx = i + skip;
                        if cv_idx < m.op_array.cv_names.len() {
                            let pname = String::from_utf8_lossy(&m.op_array.cv_names[cv_idx]);
                            let is_variadic = variadic_cv.map(|v| v as usize == cv_idx).unwrap_or(false);
                            let required = !is_variadic && i < rcount;
                            let default_suffix = if !required && !is_variadic {
                                if let Some(const_ref) = extract_param_default_constant(&m.op_array, cv_idx as u32) {
                                    let display = if let Some(rest) = const_ref.strip_prefix("global:") {
                                        rest.trim_start_matches('\\').to_string()
                                    } else if let Some(rest) = const_ref.strip_prefix("class:") {
                                        rest.to_string()
                                    } else {
                                        const_ref
                                    };
                                    format!(" = {}", display)
                                } else if let Some(v) = extract_param_default(&m.op_array, cv_idx as u32) {
                                    format!(" = {}", format_reflection_default(&v))
                                } else {
                                    String::new()
                                }
                            } else { String::new() };
                            let variadic_prefix = if is_variadic { "..." } else { "" };
                            s.push_str(&format!("    Parameter #{} [ <{}> {}${}{} ]\n", i,
                                if required { "required" } else { "optional" }, variadic_prefix, pname, default_suffix));
                        }
                    }
                    s.push_str("  }\n");
                }
                s.push_str("}\n");
                Some(Value::String(PhpString::from_string(s)))
            } else {
                Some(Value::String(PhpString::from_string(format!("Method [ {} {} ]", class, name))))
            }
        }
        _ => None,
    }
}

/// ReflectionMethod methods that need args
pub fn reflection_method_docall(
    vm: &mut Vm,
    method: &[u8],
    args: &[Value],
) -> Option<Value> {
    let this = args.first()?;
    if let Value::Object(obj) = this {
        let ob = obj.borrow();
        let class_name = ob.get_property(b"__reflection_class").to_php_string().to_string_lossy();
        let method_lower_val = ob.get_property(b"__reflection_method");
        let method_lower_bytes = method_lower_val.to_php_string();
        let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
        drop(ob);

        match method {
            b"invoke" => {
                // invoke($object, ...$args)
                // Require at least 1 user argument ($object).
                if args.len() < 2 {
                    let err = "ReflectionMethod::invoke() expects at least 1 argument, 0 given".to_string();
                    let exc = vm.create_exception(b"ArgumentCountError", &err, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                let target_obj = args.get(1)?.clone();
                let invoke_args: Vec<Value> = args.iter().skip(2).cloned().collect();

                // Block direct instantiation of internal classes via invoke($obj, __construct)
                if method_lower_bytes.as_bytes() == b"__construct" {
                    let no_direct: &[&[u8]] = &[
                        b"reflectionattribute", b"reflectiongenerator",
                        b"reflectionfiber", b"reflectionreference",
                    ];
                    if no_direct.iter().any(|c| *c == class_lower.as_slice()) {
                        let canonical = match class_lower.as_slice() {
                            b"reflectionattribute" => "ReflectionAttribute",
                            b"reflectiongenerator" => "ReflectionGenerator",
                            b"reflectionfiber" => "ReflectionFiber",
                            b"reflectionreference" => "ReflectionReference",
                            _ => "ReflectionAttribute",
                        };
                        let msg = format!("Cannot directly instantiate {}", canonical);
                        let exc = vm.create_exception(b"ReflectionException", &msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                }

                let method_def = vm.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower_bytes.as_bytes()))
                    .cloned();

                if let Some(m) = method_def {
                    // Abstract methods cannot be invoked. Check this before
                    // the ?object type validation to match PHP's ordering.
                    if m.is_abstract {
                        let canonical = vm.classes.get(&class_lower).map(|c| String::from_utf8_lossy(&c.name).to_string()).unwrap_or_else(|| String::from_utf8_lossy(&class_lower).to_string());
                        let method_disp = String::from_utf8_lossy(&m.name).to_string();
                        let msg = format!("Trying to invoke abstract method {}::{}()", canonical, method_disp);
                        let exc = vm.create_exception(b"ReflectionException", &msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                    // Type-check the first arg: must be ?object (null or object).
                    if !matches!(&target_obj, Value::Object(_) | Value::Null) {
                        let type_name = Vm::value_type_name(&target_obj);
                        let msg = format!(
                            "ReflectionMethod::invoke(): Argument #1 ($object) must be of type ?object, {} given",
                            type_name
                        );
                        let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                    // Validate target_obj for non-static methods. PHP uses
                    // the method's declaring class for the type check, not
                    // the class the reflection was created on. This lets
                    // (new ReflectionMethod(B::class, 'method'))->invoke(A)
                    // work when B inherits the method from A.
                    if !m.is_static {
                        match &target_obj {
                            Value::Object(o) => {
                                let obj_class_lower: Vec<u8> = o.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                let decl_lower: Vec<u8> = m.declaring_class.iter().map(|b| b.to_ascii_lowercase()).collect();
                                let target_lower = if !decl_lower.is_empty() { decl_lower } else { class_lower.clone() };
                                let ok = obj_class_lower == target_lower
                                    || vm.class_extends(&obj_class_lower, &target_lower);
                                if !ok {
                                    let msg = "Given object is not an instance of the class this method was declared in";
                                    let exc = vm.create_exception(b"ReflectionException", msg, vm.current_line);
                                    vm.current_exception = Some(exc);
                                    return Some(Value::Null);
                                }
                            }
                            Value::Null => {
                                let canonical = vm.classes.get(&class_lower).map(|c| String::from_utf8_lossy(&c.name).to_string()).unwrap_or_else(|| String::from_utf8_lossy(&class_lower).to_string());
                                let method_disp = String::from_utf8_lossy(&m.name).to_string();
                                let msg = format!("Trying to invoke non static method {}::{}() without an object", canonical, method_disp);
                                let exc = vm.create_exception(b"ReflectionException", &msg, vm.current_line);
                                vm.current_exception = Some(exc);
                                return Some(Value::Null);
                            }
                            _ => {
                                let msg = "Given object is not an instance of the class this method was declared in";
                                let exc = vm.create_exception(b"ReflectionException", msg, vm.current_line);
                                vm.current_exception = Some(exc);
                                return Some(Value::Null);
                            }
                        }
                    }
                    let mut func_key = class_lower.clone();
                    func_key.extend_from_slice(b"::");
                    func_key.extend_from_slice(method_lower_bytes.as_bytes());
                    vm.user_functions.insert(func_key.clone(), m.op_array.clone());

                    let mut cvs = vec![Value::Undef; m.op_array.cv_names.len()];
                    if !m.is_static {
                        if !cvs.is_empty() {
                            cvs[0] = target_obj;
                        }
                        for (i, arg) in invoke_args.iter().enumerate() {
                            if i + 1 < cvs.len() {
                                cvs[i + 1] = arg.clone();
                            }
                        }
                    } else {
                        for (i, arg) in invoke_args.iter().enumerate() {
                            if i < cvs.len() {
                                cvs[i] = arg.clone();
                            }
                        }
                    }

                    // Push class scope
                    vm.called_class_stack.push(class_lower.clone());
                    vm.push_class_scope(class_lower.clone());

                    let result = vm.execute_op_array_pub(&m.op_array, cvs).unwrap_or(Value::Null);

                    vm.called_class_stack.pop();
                    vm.pop_class_scope();

                    Some(result)
                } else {
                    Some(Value::Null)
                }
            }
            b"invokeargs" => {
                let target_obj = args.get(1)?.clone();
                let args_arr_val = args.get(2).cloned().unwrap_or(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                // Type-check: args must be array (PHP throws TypeError otherwise).
                if !matches!(&args_arr_val, Value::Array(_)) {
                    let type_name = Vm::value_type_name(&args_arr_val);
                    let msg = format!(
                        "ReflectionMethod::invokeArgs(): Argument #2 ($args) must be of type array, {} given",
                        type_name
                    );
                    let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                let invoke_args: Vec<Value> = if let Value::Array(arr) = &args_arr_val {
                    arr.borrow().iter().map(|(_, v)| v.clone()).collect()
                } else {
                    vec![]
                };

                let method_def = vm.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower_bytes.as_bytes()))
                    .cloned();

                if let Some(m) = method_def {
                    // Abstract methods cannot be invoked.
                    if m.is_abstract {
                        let canonical = vm.classes.get(&class_lower).map(|c| String::from_utf8_lossy(&c.name).to_string()).unwrap_or_else(|| String::from_utf8_lossy(&class_lower).to_string());
                        let method_disp = String::from_utf8_lossy(&m.name).to_string();
                        let msg = format!("Trying to invoke abstract method {}::{}()", canonical, method_disp);
                        let exc = vm.create_exception(b"ReflectionException", &msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                    // For non-static methods, validate that target_obj is an instance of the
                    // declaring class (or subclass).
                    if !m.is_static {
                        match &target_obj {
                            Value::Object(o) => {
                                let obj_class_lower: Vec<u8> = o.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                let ok = obj_class_lower == class_lower
                                    || vm.class_extends(&obj_class_lower, &class_lower);
                                if !ok {
                                    let msg = "Given object is not an instance of the class this method was declared in";
                                    let exc = vm.create_exception(b"ReflectionException", msg, vm.current_line);
                                    vm.current_exception = Some(exc);
                                    return Some(Value::Null);
                                }
                            }
                            _ => {
                                let msg = "Given object is not an instance of the class this method was declared in";
                                let exc = vm.create_exception(b"ReflectionException", msg, vm.current_line);
                                vm.current_exception = Some(exc);
                                return Some(Value::Null);
                            }
                        }
                    }
                    // Check that enough args were provided (ArgumentCountError).
                    // `required_param_count` is the user-declared count
                    // (no $this), so it works for both static and non-static.
                    let req = m.op_array.required_param_count as usize;
                    if invoke_args.len() < req {
                        let canonical = vm.classes.get(&class_lower).map(|c| String::from_utf8_lossy(&c.name).to_string()).unwrap_or_else(|| String::from_utf8_lossy(&class_lower).to_string());
                        let method_disp = String::from_utf8_lossy(&m.name).to_string();
                        // Determine `exactly` vs `at least`.
                        let skip = if !m.is_static { 1usize } else { 0usize };
                        let total = (m.op_array.param_count as usize).saturating_sub(skip);
                        let descriptor = if total == req { "exactly" } else { "at least" };
                        let plural = if req == 1 { "" } else { "s" };
                        let msg = format!(
                            "Too few arguments to function {}::{}(), {} passed and {} {} expected",
                            canonical, method_disp, invoke_args.len(), descriptor, req
                        );
                        let _ = plural;
                        let line = m.op_array.decl_line;
                        let exc = vm.create_exception(b"ArgumentCountError", &msg, line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                    let mut cvs = vec![Value::Undef; m.op_array.cv_names.len()];
                    if !m.is_static {
                        if !cvs.is_empty() {
                            cvs[0] = target_obj;
                        }
                        for (i, arg) in invoke_args.iter().enumerate() {
                            if i + 1 < cvs.len() {
                                cvs[i + 1] = arg.clone();
                            }
                        }
                    } else {
                        for (i, arg) in invoke_args.iter().enumerate() {
                            if i < cvs.len() {
                                cvs[i] = arg.clone();
                            }
                        }
                    }

                    vm.called_class_stack.push(class_lower.clone());
                    vm.push_class_scope(class_lower.clone());

                    let result = vm.execute_op_array_pub(&m.op_array, cvs).unwrap_or(Value::Null);

                    vm.called_class_stack.pop();
                    vm.pop_class_scope();

                    Some(result)
                } else {
                    Some(Value::Null)
                }
            }
            b"getattributes" => {
                let attrs = vm.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower_bytes.as_bytes()).map(|m| m.attributes.clone()))
                    .unwrap_or_default();
                let filter_name = args.get(1).and_then(|v| if matches!(v, Value::Null) { None } else { Some(v.to_php_string().as_bytes().to_vec()) });
                let filter_flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);
                Some(create_reflection_attributes(vm, &attrs, filter_name.as_deref(), filter_flags, 4))
            }
            b"getclosure" => {
                // Build a Closure value referring to this method. In this VM,
                // closures are represented as strings ("Class::method") or
                // arrays ([callable, $this]). For static methods return the
                // callable name; for instance methods bind to the given $obj.
                let is_static = vm.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower_bytes.as_bytes()))
                    .map(|m| m.is_static)
                    .unwrap_or(false);
                // Canonicalize name and class for display
                let method_name = vm.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower_bytes.as_bytes())
                        .map(|m| String::from_utf8_lossy(&m.name).to_string()))
                    .unwrap_or_else(|| method_lower_bytes.to_string_lossy());
                let class_canonical = vm.classes.get(&class_lower)
                    .map(|c| String::from_utf8_lossy(&c.name).to_string())
                    .unwrap_or(class_name.clone());
                let callable = format!("{}::{}", class_canonical, method_name);
                if is_static {
                    Some(Value::String(PhpString::from_string(callable)))
                } else {
                    let target_obj = args.get(1).cloned().unwrap_or(Value::Null);
                    if matches!(target_obj, Value::Null | Value::Undef) {
                        // No object: cannot bind; return Null.
                        Some(Value::Null)
                    } else {
                        let mut arr = PhpArray::new();
                        arr.push(Value::String(PhpString::from_string(callable)));
                        arr.push(target_obj);
                        Some(Value::Array(Rc::new(RefCell::new(arr))))
                    }
                }
            }
            _ => None,
        }
    } else {
        None
    }
}

/// ReflectionFunction no-arg method dispatch
pub fn reflection_function_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    let target = ob.get_property(b"__reflection_target").to_php_string();
    let func_lower = target.as_bytes().to_vec();
    drop(ob);

    match method {
        b"getname" => {
            let ob = obj.borrow();
            Some(ob.get_property(b"name"))
        }
        b"isinternal" => {
            let is_internal = vm.functions.contains_key(func_lower.as_slice());
            Some(if is_internal { Value::True } else { Value::False })
        }
        b"isuserdefined" => {
            let is_user = vm.user_functions.contains_key(func_lower.as_slice());
            Some(if is_user { Value::True } else { Value::False })
        }
        b"getfilename" => {
            if let Some(op_array) = vm.user_functions.get(func_lower.as_slice()) {
                if !op_array.filename.is_empty() {
                    Some(Value::String(PhpString::from_vec(op_array.filename.clone())))
                } else {
                    Some(Value::String(PhpString::from_string(vm.current_file.clone())))
                }
            } else {
                Some(Value::False)
            }
        }
        b"getstartline" => {
            if let Some(op_array) = vm.user_functions.get(func_lower.as_slice()) {
                Some(Value::Long(op_array.decl_line as i64))
            } else {
                Some(Value::False)
            }
        }
        b"getendline" => {
            if let Some(op_array) = vm.user_functions.get(func_lower.as_slice()) {
                let l = if op_array.end_line > 0 { op_array.end_line as i64 } else { op_array.decl_line as i64 };
                if l > 0 { Some(Value::Long(l)) } else { Some(Value::False) }
            } else {
                Some(Value::False)
            }
        }
        b"getdoccomment" => {
            if let Some(op_array) = vm.user_functions.get(func_lower.as_slice()) {
                if let Some(doc) = op_array.doc_comment.as_ref() {
                    Some(Value::String(PhpString::from_bytes(doc)))
                } else {
                    Some(Value::False)
                }
            } else {
                Some(Value::False)
            }
        }
        b"getstaticvariables" => {
            // Return static variables of the function. PHP returns them even
            // when the function hasn't yet been called, in which case the
            // values are the compile-time defaults. Walk the op_array looking
            // for `StaticVarInit` opcodes and merge with any runtime values.
            let mut result = PhpArray::new();
            if let Some(op_array) = vm.user_functions.get(func_lower.as_slice()).cloned() {
                let func_prefix = {
                    let mut p = op_array.name.clone();
                    p.extend_from_slice(b"::");
                    p
                };
                for op in &op_array.ops {
                    if !matches!(op.opcode, crate::opcode::OpCode::StaticVarInit) {
                        continue;
                    }
                    let key_bytes = match op.result {
                        crate::opcode::OperandType::Const(idx) => {
                            match op_array.literals.get(idx as usize) {
                                Some(Value::String(s)) => s.as_bytes().to_vec(),
                                _ => continue,
                            }
                        }
                        _ => continue,
                    };
                    // Pull the variable name (after the "name::" prefix).
                    let var_name: Vec<u8> = if key_bytes.starts_with(&func_prefix) {
                        key_bytes[func_prefix.len()..].to_vec()
                    } else { continue; };
                    // Prefer runtime value; fall back to the literal default.
                    let val = if let Some(v) = vm.static_vars().get(&key_bytes) {
                        v.clone()
                    } else {
                        match op.op2 {
                            crate::opcode::OperandType::Const(idx) => {
                                op_array.literals.get(idx as usize).cloned().unwrap_or(Value::Null)
                            }
                            _ => Value::Null,
                        }
                    };
                    result.set(ArrayKey::String(PhpString::from_vec(var_name)), val);
                }
            }
            // Also include any runtime statics created later that we missed.
            let prefix = format!("{}::", String::from_utf8_lossy(&func_lower));
            for (key, val) in vm.static_vars() {
                let key_str = String::from_utf8_lossy(key);
                if key_str.starts_with(&prefix) {
                    let var_name = &key_str[prefix.len()..];
                    let key = ArrayKey::String(PhpString::from_string(var_name.to_string()));
                    if result.get(&key).is_none() {
                        result.set(key, val.clone());
                    }
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"getnumberofparameters" => {
            if let Some(op_array) = vm.user_functions.get(func_lower.as_slice()) {
                Some(Value::Long(op_array.param_count as i64))
            } else if let Some(_) = vm.functions.get(func_lower.as_slice()) {
                // For built-in functions, check param names
                let count = vm.builtin_param_names.get(func_lower.as_slice())
                    .map(|p| p.len() as i64)
                    .unwrap_or(0);
                Some(Value::Long(count))
            } else {
                Some(Value::Long(0))
            }
        }
        b"getnumberofrequiredparameters" => {
            if let Some(op_array) = vm.user_functions.get(func_lower.as_slice()) {
                Some(Value::Long(op_array.required_param_count as i64))
            } else {
                Some(Value::Long(0))
            }
        }
        b"returnsreference" => {
            let by_ref = vm.user_functions.get(func_lower.as_slice())
                .map(|op| op.returns_by_ref)
                .unwrap_or(false);
            Some(if by_ref { Value::True } else { Value::False })
        }
        b"getparameters" => {
            if let Some(op_array) = vm.user_functions.get(func_lower.as_slice()).cloned() {
                Some(create_reflection_parameters(vm, &op_array))
            } else {
                Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
            }
        }
        b"getreturntype" => {
            if let Some(op_array) = vm.user_functions.get(func_lower.as_slice()) {
                if let Some(ref rt) = op_array.return_type {
                    let rt = rt.clone();
                    Some(create_reflection_type(vm, &rt))
                } else {
                    Some(Value::Null)
                }
            } else {
                Some(Value::Null)
            }
        }
        b"hasreturntype" => {
            let has = vm.user_functions.get(func_lower.as_slice())
                .and_then(|op| op.return_type.as_ref())
                .is_some();
            Some(if has { Value::True } else { Value::False })
        }
        b"isclosure" => {
            let ob = obj.borrow();
            let is_closure = ob.has_property(b"__reflection_is_closure");
            Some(if is_closure { Value::True } else { Value::False })
        }
        b"isvariadic" => {
            let is_variadic = vm.user_functions.get(func_lower.as_slice())
                .and_then(|op| op.variadic_param)
                .is_some();
            Some(if is_variadic { Value::True } else { Value::False })
        }
        b"isdeprecated" => {
            // Check for the #[\Deprecated] attribute on the function.
            let has_dep = vm.user_functions.get(func_lower.as_slice())
                .map(|op| {
                    op.attributes.iter().any(|a| {
                        let n = String::from_utf8_lossy(&a.name);
                        n.eq_ignore_ascii_case("deprecated") || n.eq_ignore_ascii_case("\\deprecated")
                    })
                })
                .unwrap_or(false);
            Some(if has_dep { Value::True } else { Value::False })
        }
        b"isgenerator" => {
            let is_gen = vm.user_functions.get(func_lower.as_slice())
                .map(|op| op.is_generator)
                .unwrap_or(false);
            Some(if is_gen { Value::True } else { Value::False })
        }
        b"getextension" => {
            let func_str = String::from_utf8_lossy(&func_lower);
            let ext_name = get_function_extension(&func_str);
            if let Some(ext) = ext_name {
                let obj_id = vm.next_object_id();
                let mut ext_obj = PhpObject::new(b"ReflectionExtension".to_vec(), obj_id);
                ext_obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(ext.to_string())));
                Some(Value::Object(Rc::new(RefCell::new(ext_obj))))
            } else {
                Some(Value::Null)
            }
        }
        b"getextensionname" => {
            let func_str = String::from_utf8_lossy(&func_lower);
            let ext_name = get_function_extension(&func_str);
            if let Some(ext) = ext_name {
                Some(Value::String(PhpString::from_string(ext.to_string())))
            } else {
                Some(Value::False)
            }
        }
        b"isanonymous" => {
            let ob = obj.borrow();
            let is_closure = ob.has_property(b"__reflection_is_closure");
            // FCC closures (`strlen(...)`) wrap a named function — they are
            // NOT anonymous, even though the closure carries an internal
            // `__closure_fcc_*` name. Detect via the underlying op_array's
            // fcc_target_name.
            let is_fcc = if is_closure {
                vm.user_functions.get(func_lower.as_slice())
                    .map(|op| op.fcc_target_name.is_some())
                    .unwrap_or(false)
            } else { false };
            Some(if is_closure && !is_fcc { Value::True } else { Value::False })
        }
        b"isstatic" => {
            // Closures declared `static function()` carry is_static_closure on
            // their OpArray. For free functions this is always false.
            if let Some(op_array) = vm.user_functions.get(func_lower.as_slice()) {
                Some(if op_array.is_static_closure { Value::True } else { Value::False })
            } else {
                Some(Value::False)
            }
        }
        b"isdisabled" => {
            Some(Value::False)
        }
        b"innamespace" => {
            let ob = obj.borrow();
            let name = ob.get_property(b"name").to_php_string().to_string_lossy();
            Some(if name.contains('\\') { Value::True } else { Value::False })
        }
        b"getnamespacename" => {
            let ob = obj.borrow();
            let name = ob.get_property(b"name").to_php_string().to_string_lossy();
            if let Some(pos) = name.rfind('\\') {
                Some(Value::String(PhpString::from_string(name[..pos].to_string())))
            } else {
                Some(Value::String(PhpString::from_bytes(b"")))
            }
        }
        b"getshortname" => {
            let ob = obj.borrow();
            let name = ob.get_property(b"name").to_php_string().to_string_lossy();
            if let Some(pos) = name.rfind('\\') {
                Some(Value::String(PhpString::from_string(name[pos+1..].to_string())))
            } else {
                Some(Value::String(PhpString::from_string(name)))
            }
        }
        b"getclosurethis" => {
            let ob = obj.borrow();
            if ob.has_property(b"__reflection_closure_this") {
                Some(ob.get_property(b"__reflection_closure_this"))
            } else {
                Some(Value::Null)
            }
        }
        b"getclosure" => {
            // Return a Closure wrapping the function. The function name itself
            // is a valid string callable in our VM.
            let ob = obj.borrow();
            if ob.has_property(b"__reflection_is_closure") {
                return Some(ob.get_property(b"__reflection_target"));
            }
            let name = ob.get_property(b"name").to_php_string();
            drop(ob);
            let func_lower: Vec<u8> = name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
            if vm.user_functions.contains_key(func_lower.as_slice())
                || vm.functions.contains_key(func_lower.as_slice())
            {
                Some(Value::String(name))
            } else {
                Some(Value::Null)
            }
        }
        b"getclosurescopeclass" => {
            let ob = obj.borrow();
            if ob.has_property(b"__reflection_closure_scope") {
                let name = ob.get_property(b"__reflection_closure_scope").to_php_string().to_string_lossy();
                drop(ob);
                Some(create_reflection_class(vm, &name))
            } else {
                Some(Value::Null)
            }
        }
        b"getclosurecalledclass" => {
            let ob = obj.borrow();
            if ob.has_property(b"__reflection_closure_called") {
                let name = ob.get_property(b"__reflection_closure_called").to_php_string().to_string_lossy();
                drop(ob);
                Some(create_reflection_class(vm, &name))
            } else {
                Some(Value::Null)
            }
        }
        b"getclosureusedvariables" => {
            let ob = obj.borrow();
            let captured = match ob.get_property(b"__reflection_closure_captured") {
                Value::Array(a) => Some(a),
                _ => None,
            };
            drop(ob);
            if let Some(captured_arr) = captured {
                // Map captured values onto the closure's first N cv_names —
                // those are the lexical `use` variables (in source order).
                let cv_names: Vec<Vec<u8>> = vm.user_functions.get(func_lower.as_slice())
                    .map(|op| op.cv_names.clone())
                    .unwrap_or_default();
                let values: Vec<Value> = captured_arr.borrow().values().cloned().collect();
                let mut out = PhpArray::new();
                // Skip leading `$this` slot if present.
                let mut name_offset = 0usize;
                if cv_names.first().map(|n| n.as_slice() == b"this").unwrap_or(false) {
                    name_offset = 1;
                }
                for (i, v) in values.iter().enumerate() {
                    if let Some(n) = cv_names.get(name_offset + i) {
                        let key = ArrayKey::String(PhpString::from_vec(n.clone()));
                        out.set(key, v.clone());
                    }
                }
                Some(Value::Array(Rc::new(RefCell::new(out))))
            } else {
                Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
            }
        }
        b"hastentativereturntype" => {
            Some(Value::False)
        }
        b"gettentativereturntype" => {
            Some(Value::Null)
        }
        b"__tostring" => {
            let ob = obj.borrow();
            let name = ob.get_property(b"name").to_php_string().to_string_lossy();
            drop(ob);
            if let Some(op_array) = vm.user_functions.get(func_lower.as_slice()).cloned() {
                let mut s = String::new();
                if let Some(doc) = op_array.doc_comment.as_ref() {
                    s.push_str(&String::from_utf8_lossy(doc));
                    s.push('\n');
                }
                s.push_str(&format!("Function [ <user> function {} ] {{\n", name));
                let f_end = if op_array.end_line > 0 { op_array.end_line } else { op_array.decl_line };
                s.push_str(&format!("  @@ {} {} - {}\n", vm.current_file, op_array.decl_line, f_end));
                if op_array.param_count > 0 {
                    s.push_str(&format!("\n  - Parameters [{}] {{\n", op_array.param_count));
                    let variadic_cv = op_array.variadic_param;
                    for i in 0..op_array.param_count as usize {
                        if i < op_array.cv_names.len() {
                            let pname = String::from_utf8_lossy(&op_array.cv_names[i]);
                            let is_variadic = variadic_cv.map(|v| v as usize == i).unwrap_or(false);
                            let required = !is_variadic && i < op_array.required_param_count as usize;
                            let default_suffix = if !required && !is_variadic {
                                if let Some(const_ref) = extract_param_default_constant(&op_array, i as u32) {
                                    let display = if let Some(rest) = const_ref.strip_prefix("global:") {
                                        rest.trim_start_matches('\\').to_string()
                                    } else if let Some(rest) = const_ref.strip_prefix("class:") {
                                        rest.to_string()
                                    } else {
                                        const_ref
                                    };
                                    format!(" = {}", display)
                                } else if let Some(v) = extract_param_default(&op_array, i as u32) {
                                    format!(" = {}", format_reflection_default(&v))
                                } else {
                                    String::new()
                                }
                            } else { String::new() };
                            let variadic_prefix = if is_variadic { "..." } else { "" };
                            s.push_str(&format!("    Parameter #{} [ <{}> {}${}{} ]\n", i,
                                if required { "required" } else { "optional" }, variadic_prefix, pname, default_suffix));
                        }
                    }
                    s.push_str("  }\n");
                }
                s.push_str("}\n");
                Some(Value::String(PhpString::from_string(s)))
            } else {
                Some(Value::String(PhpString::from_string(format!("Function [ <internal> function {} ]", name))))
            }
        }
        _ => None,
    }
}

/// ReflectionFunction methods with args
pub fn reflection_function_docall(
    vm: &mut Vm,
    method: &[u8],
    args: &[Value],
) -> Option<Value> {
    let this = args.first()?;
    if let Value::Object(obj) = this {
        let ob = obj.borrow();
        let target = ob.get_property(b"__reflection_target").to_php_string();
        let func_lower = target.as_bytes().to_vec();
        drop(ob);

        match method {
            b"invoke" => {
                let invoke_args: Vec<Value> = args.iter().skip(1).cloned().collect();
                if let Some(op_array) = vm.user_functions.get(&func_lower).cloned() {
                    let mut cvs = vec![Value::Undef; op_array.cv_names.len()];
                    for (i, arg) in invoke_args.iter().enumerate() {
                        if i < cvs.len() {
                            cvs[i] = arg.clone();
                        }
                    }
                    let result = vm.execute_op_array_pub(&op_array, cvs).unwrap_or(Value::Null);
                    Some(result)
                } else if let Some(func) = vm.functions.get(&func_lower).cloned() {
                    let result = func(vm, &invoke_args).unwrap_or(Value::Null);
                    Some(result)
                } else {
                    Some(Value::Null)
                }
            }
            b"invokeargs" => {
                let args_arr = args.get(1).cloned().unwrap_or(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                let invoke_args: Vec<Value> = if let Value::Array(arr) = &args_arr {
                    arr.borrow().iter().map(|(_, v)| v.clone()).collect()
                } else {
                    vec![]
                };
                if let Some(op_array) = vm.user_functions.get(&func_lower).cloned() {
                    let mut cvs = vec![Value::Undef; op_array.cv_names.len()];
                    for (i, arg) in invoke_args.iter().enumerate() {
                        if i < cvs.len() {
                            cvs[i] = arg.clone();
                        }
                    }
                    let result = vm.execute_op_array_pub(&op_array, cvs).unwrap_or(Value::Null);
                    Some(result)
                } else if let Some(func) = vm.functions.get(&func_lower).cloned() {
                    let result = func(vm, &invoke_args).unwrap_or(Value::Null);
                    Some(result)
                } else {
                    Some(Value::Null)
                }
            }
            b"getattributes" => {
                let attrs = vm.user_functions.get(func_lower.as_slice())
                    .map(|op| op.attributes.clone())
                    .unwrap_or_default();
                let filter_name = args.get(1).and_then(|v| if matches!(v, Value::Null) { None } else { Some(v.to_php_string().as_bytes().to_vec()) });
                let filter_flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);
                Some(create_reflection_attributes(vm, &attrs, filter_name.as_deref(), filter_flags, 2))
            }
            b"getclosure" => {
                // Return the function name as a callable string (treated as a
                // closure by Closure::__invoke).
                let name = {
                    let obj_r = obj.borrow();
                    obj_r.get_property(b"name").to_php_string().to_string_lossy()
                };
                // If the underlying target is already a closure-ish string,
                // return it as-is.
                let target_str = String::from_utf8_lossy(&func_lower).to_string();
                if target_str.starts_with("__closure_") || target_str.starts_with("__arrow_")
                    || target_str.starts_with("__bound_closure_") || target_str.starts_with("__closure_fcc_") {
                    Some(Value::String(PhpString::from_string(target_str)))
                } else {
                    Some(Value::String(PhpString::from_string(name)))
                }
            }
            _ => None,
        }
    } else {
        None
    }
}

/// ReflectionProperty no-arg method dispatch
pub fn reflection_property_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    let class_name = ob.get_property(b"__reflection_class").to_php_string().to_string_lossy();
    let prop_name = ob.get_property(b"__reflection_prop").to_php_string().to_string_lossy();
    let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    // Dynamic-property flag: when set, the ReflectionProperty refers to a
    // runtime-only instance property that doesn't exist in the class's
    // PropertyDef list. It is always considered public and declared by the
    // runtime class (PHP behavior for dynamic props with AllowDynamicProperties).
    let is_dynamic = matches!(ob.get_property(b"__reflection_prop_dynamic"), Value::True);
    drop(ob);

    match method {
        b"getname" => {
            let ob = obj.borrow();
            Some(ob.get_property(b"name"))
        }
        b"getdeclaringclass" => {
            if is_dynamic {
                // Dynamic props are owned by the runtime class.
                let canonical = vm.classes.get(&class_lower)
                    .map(|c| String::from_utf8_lossy(&c.name).to_string())
                    .unwrap_or(class_name.clone());
                return Some(create_reflection_class(vm, &canonical));
            }
            let declaring = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes())
                    .map(|p| String::from_utf8_lossy(&p.declaring_class).to_string()))
                .unwrap_or(class_name.clone());
            let declaring_canonical = vm.classes.get(declaring.as_bytes().to_ascii_lowercase().as_slice())
                .map(|c| String::from_utf8_lossy(&c.name).to_string())
                .unwrap_or(declaring.clone());
            Some(create_reflection_class(vm, &declaring_canonical))
        }
        b"ispublic" => {
            if is_dynamic { return Some(Value::True); }
            let vis = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| p.visibility))
                .unwrap_or(Visibility::Public);
            Some(if vis == Visibility::Public { Value::True } else { Value::False })
        }
        b"isprotected" => {
            if is_dynamic { return Some(Value::False); }
            let vis = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| p.visibility))
                .unwrap_or(Visibility::Public);
            Some(if vis == Visibility::Protected { Value::True } else { Value::False })
        }
        b"isprivate" => {
            if is_dynamic { return Some(Value::False); }
            let vis = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| p.visibility))
                .unwrap_or(Visibility::Public);
            Some(if vis == Visibility::Private { Value::True } else { Value::False })
        }
        b"isstatic" => {
            let is_static = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| p.is_static))
                .unwrap_or(false);
            Some(if is_static { Value::True } else { Value::False })
        }
        b"isreadonly" => {
            let is_readonly = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| p.is_readonly))
                .unwrap_or(false);
            Some(if is_readonly { Value::True } else { Value::False })
        }
        b"isvirtual" => {
            let is_virtual = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| p.is_virtual))
                .unwrap_or(false);
            Some(if is_virtual { Value::True } else { Value::False })
        }
        b"isabstract" => {
            let is_abstract = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| p.is_abstract))
                .unwrap_or(false);
            Some(if is_abstract { Value::True } else { Value::False })
        }
        b"isfinal" => {
            // Either explicitly final OR `private(set)` (implicit final
            // because the setter cannot be overridden).
            let is_final = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()))
                .map(|p| p.is_final || p.set_visibility == Some(Visibility::Private))
                .unwrap_or(false);
            Some(if is_final { Value::True } else { Value::False })
        }
        b"hashooks" => {
            let has_hooks = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()))
                .map(|p| p.has_get_hook || p.has_set_hook)
                .unwrap_or(false);
            Some(if has_hooks { Value::True } else { Value::False })
        }
        b"isdefault" => {
            let is_default = vm.classes.get(&class_lower)
                .map(|c| c.properties.iter().any(|p| p.name == prop_name.as_bytes()))
                .unwrap_or(false);
            Some(if is_default { Value::True } else { Value::False })
        }
        b"getdefaultvalue" => {
            // Per PHP 8.5: emit deprecation when called for a property that
            // has no default (Undef) — but only for *typed* properties.
            // Untyped properties implicitly default to NULL, so no deprecation.
            // Dynamic properties (looked up by object) also trigger the warning.
            let prop_info = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).cloned());
            let (default_val, has_default, has_type) = match &prop_info {
                Some(p) => {
                    let has = !matches!(p.default, Value::Undef);
                    (p.default.clone(), has, p.property_type.is_some())
                }
                None => {
                    // Dynamic property: counts as no-default + typed (warns)
                    (Value::Null, false, true)
                }
            };
            if !has_default && has_type {
                vm.emit_deprecated(
                    "ReflectionProperty::getDefaultValue() for a property without a default value is deprecated, use ReflectionProperty::hasDefaultValue() to check if the default value exists",
                );
            }
            let result = match default_val {
                Value::Undef => Value::Null,
                v => v,
            };
            Some(result)
        }
        b"hasdefaultvalue" => {
            let has = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()))
                .map(|p| !matches!(p.default, Value::Undef))
                .unwrap_or(false);
            Some(if has { Value::True } else { Value::False })
        }
        b"getmodifiers" => {
            let mods = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| reflection_property_modifiers(p)))
                .unwrap_or(1);
            Some(Value::Long(mods))
        }
        b"getdoccomment" => {
            let doc = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()))
                .and_then(|p| p.doc_comment.as_ref())
                .cloned();
            if let Some(d) = doc {
                Some(Value::String(PhpString::from_string(d)))
            } else {
                Some(Value::False)
            }
        }
        b"hastype" => {
            let has = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()))
                .map(|p| p.property_type.is_some())
                .unwrap_or(false);
            Some(if has { Value::True } else { Value::False })
        }
        b"gettype" => {
            let pt = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()))
                .and_then(|p| p.property_type.as_ref())
                .cloned();
            if let Some(pt) = pt {
                Some(create_reflection_type(vm, &pt))
            } else {
                Some(Value::Null)
            }
        }
        b"ispromoted" => {
            Some(Value::False)
        }
        b"setaccessible" => {
            vm.emit_deprecated(
                "Method ReflectionProperty::setAccessible() is deprecated since 8.5, as it has no effect since PHP 8.1",
            );
            Some(Value::Null)
        }
        b"getmangledname" => {
            // PHP: private properties -> "\0ClassName\0name",
            // protected properties -> "\0*\0name", public -> "name".
            let (vis, declaring_lower) = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()))
                .map(|p| (p.visibility, p.declaring_class.clone()))
                .unwrap_or((Visibility::Public, class_lower.clone()));
            let declaring_canonical = vm.classes.get(&declaring_lower)
                .map(|c| String::from_utf8_lossy(&c.name).to_string())
                .unwrap_or_else(|| String::from_utf8_lossy(&declaring_lower).to_string());
            let mangled = match vis {
                Visibility::Public => prop_name.clone(),
                Visibility::Protected => format!("\0*\0{}", prop_name),
                Visibility::Private => format!("\0{}\0{}", declaring_canonical, prop_name),
            };
            Some(Value::String(PhpString::from_string(mangled)))
        }
        b"isdynamic" => {
            // Dynamic properties are those not declared on the class.
            let is_declared = vm.classes.get(&class_lower)
                .map(|c| c.properties.iter().any(|p| p.name == prop_name.as_bytes()))
                .unwrap_or(false);
            Some(if is_declared { Value::False } else { Value::True })
        }
        b"isprivateset" => {
            let is_priv_set = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()))
                .and_then(|p| p.set_visibility)
                .map(|v| v == Visibility::Private)
                .unwrap_or(false);
            Some(if is_priv_set { Value::True } else { Value::False })
        }
        b"isprotectedset" => {
            let is_prot_set = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()))
                .and_then(|p| p.set_visibility)
                .map(|v| v == Visibility::Protected)
                .unwrap_or(false);
            Some(if is_prot_set { Value::True } else { Value::False })
        }
        b"gethooks" => {
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
        }
        b"gethook" => {
            Some(Value::Null)
        }
        b"hashook" => {
            Some(Value::False)
        }
        b"getsettabletype" => {
            // Default behavior: same as getType(); asymmetric setter types
            // aren't modeled yet.
            let pt = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()))
                .and_then(|p| p.property_type.as_ref())
                .cloned();
            if let Some(pt) = pt {
                Some(create_reflection_type(vm, &pt))
            } else {
                Some(Value::Null)
            }
        }
        b"__tostring" => {
            let prop = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()))
                .cloned();
            if let Some(p) = prop {
                let vis = match p.visibility {
                    Visibility::Public => "public",
                    Visibility::Protected => "protected",
                    Visibility::Private => "private",
                };
                // PHP order: visibility modifier first, then 'static'.
                let static_str = if p.is_static { " static" } else { "" };
                let default_repr = reflection_repr_for_property_default(&p.default);
                let s = format!("Property [ {}{} ${} = {} ]\n", vis, static_str, prop_name, default_repr);
                Some(Value::String(PhpString::from_string(s)))
            } else {
                Some(Value::String(PhpString::from_string(format!("Property [ ${} ]\n", prop_name))))
            }
        }
        _ => None,
    }
}

/// ReflectionProperty methods with args
/// Shared logic for `skipLazyInitialization` and
/// `setRawValueWithoutLazyInitialization`: adds the property name to the
/// object's `__lazy_skipped` list, optionally sets the property to a value,
/// and clears the lazy state when all declared non-static properties have
/// been skipped.
fn mark_prop_skipped(
    vm: &mut Vm,
    target: &Value,
    class_lower: &[u8],
    prop_name: &[u8],
    value: Option<Value>,
) {
    let target_obj = match target {
        Value::Object(o) => o,
        _ => return,
    };
    let all_props: Vec<Vec<u8>> = vm.classes.get(class_lower)
        .map(|c| c.properties.iter()
            .filter(|p| !p.is_static)
            .map(|p| p.name.clone())
            .collect())
        .unwrap_or_default();
    let mut ob = target_obj.borrow_mut();
    // Append to __lazy_skipped (NUL-separated list of property names).
    let prev = ob.get_property(b"__lazy_skipped");
    let mut names: Vec<u8> = match prev {
        Value::String(s) => s.as_bytes().to_vec(),
        _ => Vec::new(),
    };
    // De-dupe
    if !names.split(|b| *b == 0).any(|n| n == prop_name) {
        if !names.is_empty() {
            names.push(b'\0');
        }
        names.extend_from_slice(prop_name);
    }
    ob.set_property(
        b"__lazy_skipped".to_vec(),
        Value::String(PhpString::from_vec(names.clone())),
    );
    if let Some(v) = value {
        ob.set_property(prop_name.to_vec(), v);
    }
    // If every declared non-static property is now in the skipped list,
    // the object is effectively initialized — clear the lazy state.
    let all_skipped = all_props.iter().all(|p| {
        names.split(|b| *b == 0).any(|n| n == p.as_slice())
    });
    if all_skipped && !all_props.is_empty() {
        ob.remove_property(b"__lazy_state");
        ob.remove_property(b"__lazy_initializer");
        ob.remove_property(b"__lazy_skipped");
    }
}

pub fn reflection_property_docall(
    vm: &mut Vm,
    method: &[u8],
    args: &[Value],
) -> Option<Value> {
    let this = args.first()?;
    if let Value::Object(obj) = this {
        let ob = obj.borrow();
        let prop_name = ob.get_property(b"__reflection_prop").to_php_string().to_string_lossy();
        let class_name = ob.get_property(b"__reflection_class").to_php_string().to_string_lossy();
        let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
        drop(ob);

        // Check if this is a static property
        let is_static = vm.classes.get(&class_lower)
            .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| p.is_static))
            .unwrap_or(false);

        match method {
            b"getvalue" => {
                if is_static {
                    // Static properties are stored on the class that declared
                    // them, even when reflected via a subclass. Walk up the
                    // parent chain to find the storage slot.
                    let val = find_static_property_value(vm, &class_lower, prop_name.as_bytes())
                        .unwrap_or(Value::Null);
                    Some(val)
                } else if let Some(target) = args.get(1) {
                    if let Value::Object(target_obj) = target {
                        let target_ob = target_obj.borrow();
                        Some(target_ob.get_property(prop_name.as_bytes()))
                    } else {
                        Some(Value::Null)
                    }
                } else {
                    Some(Value::Null)
                }
            }
            b"setvalue" => {
                if is_static {
                    // For static properties, ReflectionProperty::setValue accepts:
                    //   setValue($obj_or_null, $value)  — two args (canonical)
                    //   setValue($value)                — one arg (deprecated)
                    // (args[0] is the ReflectionProperty $this slot.)
                    let value = if args.len() >= 3 {
                        args[2].clone()
                    } else if args.len() >= 2 {
                        // Deprecated single-arg form on a static property.
                        vm.emit_deprecated(
                            "Calling ReflectionProperty::setValue() with a single argument is deprecated",
                        );
                        args[1].clone()
                    } else {
                        Value::Null
                    };
                    // Store on the declaring class, not the reflected one.
                    let declaring = find_static_property_owner(vm, &class_lower, prop_name.as_bytes())
                        .unwrap_or_else(|| class_lower.clone());
                    if let Some(ce) = vm.classes.get_mut(declaring.as_slice()) {
                        ce.static_properties.insert(prop_name.as_bytes().to_vec(), value);
                    }
                } else if args.len() >= 3 {
                    let target = args[1].clone();
                    let value = args[2].clone();
                    if let Value::Object(target_obj) = &target {
                        // For typed properties, coerce the incoming value to
                        // satisfy the declared type when possible. In particular
                        // an object that is __toString()-able coerces to string
                        // when assigned to a string-typed property — bug #79683.
                        let prop_type = vm.classes.get(&class_lower)
                            .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()))
                            .and_then(|p| p.property_type.clone());
                        let mut coerced = value.clone();
                        if let Some(pt) = &prop_type {
                            // Walk through Nullable wrapper
                            let inner = match pt {
                                crate::opcode::ParamType::Nullable(inner) => inner.as_ref(),
                                other => other,
                            };
                            if let crate::opcode::ParamType::Simple(name) = inner {
                                if name.as_slice() == b"string" {
                                    if let Value::Object(_) = &coerced {
                                        let s = vm.value_to_string(&coerced);
                                        if vm.current_exception.is_none() {
                                            coerced = Value::String(s);
                                        }
                                    }
                                }
                            }
                        }
                        let mut target_ob = target_obj.borrow_mut();
                        target_ob.set_property(prop_name.as_bytes().to_vec(), coerced);
                    }
                }
                Some(Value::Null)
            }
            b"getattributes" => {
                let attrs = vm.classes.get(&class_lower)
                    .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| p.attributes.clone()))
                    .unwrap_or_default();
                let filter_name = args.get(1).and_then(|v| if matches!(v, Value::Null) { None } else { Some(v.to_php_string().as_bytes().to_vec()) });
                let filter_flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);
                Some(create_reflection_attributes(vm, &attrs, filter_name.as_deref(), filter_flags, 8))
            }
            b"getrawvalue" => {
                // getRawValue($obj): read the property without triggering lazy init.
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                if let Value::Object(target_obj) = &target {
                    let ob = target_obj.borrow();
                    Some(ob.get_property(prop_name.as_bytes()))
                } else {
                    Some(Value::Null)
                }
            }
            b"setrawvaluewithoutlazyinitialization" => {
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                let value = args.get(2).cloned().unwrap_or(Value::Null);
                // If the target is an initialized lazy proxy, write directly
                // to the backing real object.
                if let Value::Object(target_obj) = &target {
                    let is_init_proxy = {
                        let ob = target_obj.borrow();
                        matches!(ob.get_property(b"__lazy_state"),
                            Value::String(ref s) if s.as_bytes() == b"proxy")
                            && ob.has_property(b"__lazy_real")
                    };
                    if is_init_proxy {
                        if let Value::Object(real) = target_obj.borrow().get_property(b"__lazy_real") {
                            real.borrow_mut().set_property(prop_name.as_bytes().to_vec(), value);
                            return Some(Value::Null);
                        }
                    }
                }
                mark_prop_skipped(vm, &target, &class_lower, prop_name.as_bytes(), Some(value));
                Some(Value::Null)
            }
            b"isinitialized" => {
                // isInitialized($obj): returns true if the property has been
                // assigned a value (or has a default) on the object.
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                if is_static {
                    let initialized = vm.classes.get(&class_lower)
                        .map(|c| c.static_properties.contains_key(prop_name.as_bytes()))
                        .unwrap_or(false);
                    Some(if initialized { Value::True } else { Value::False })
                } else if let Value::Object(target_obj) = &target {
                    let ob = target_obj.borrow();
                    let val = ob.get_property(prop_name.as_bytes());
                    let has = !matches!(val, Value::Undef);
                    Some(if has { Value::True } else { Value::False })
                } else {
                    Some(Value::False)
                }
            }
            b"setrawvalue" => {
                // setRawValue($obj, $value): writes the property bypassing
                // hooks and type checks (best-effort in our VM).
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                let value = args.get(2).cloned().unwrap_or(Value::Null);
                if is_static {
                    if let Some(ce) = vm.classes.get_mut(&class_lower) {
                        ce.static_properties.insert(prop_name.as_bytes().to_vec(), value);
                    }
                } else if let Value::Object(target_obj) = &target {
                    let mut ob = target_obj.borrow_mut();
                    ob.set_property(prop_name.as_bytes().to_vec(), value);
                }
                Some(Value::Null)
            }
            b"skiplazyinitialization" => {
                // skipLazyInitialization($obj): mark this property as already
                // initialized on the given lazy object. We store the skipped
                // names in __lazy_skipped and apply the property's default
                // value. Once all declared props are skipped, we clear the
                // lazy state so var_dump shows a normal object.
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                let default = vm.classes.get(&class_lower)
                    .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()))
                    .map(|p| p.default.clone());
                let resolved_default = default.map(|d| vm.resolve_deferred_value(&d));
                mark_prop_skipped(vm, &target, &class_lower, prop_name.as_bytes(), resolved_default);
                Some(Value::Null)
            }
            _ => None,
        }
    } else {
        None
    }
}

/// Extract the literal default value for a given CV index by scanning the
/// function prologue opcodes the compiler emits for parameter defaults.
///
/// Pattern emitted by the compiler for each defaulted parameter:
///   Identical CV(cv), Const(Undef) -> Tmp(t)
///   JmpZ Tmp(t), JmpTarget(after)
///   Assign CV(cv), <default_val>
///
/// We look for an `Assign` targeting the requested CV whose op2 is a
/// `Const(idx)` and return that literal.  If the default is a constant
/// expression or `new ExprName()` initializer it may not be a simple
/// constant operand — in that case we currently give up and return None.
/// Format a Value for display in Reflection __toString output. Matches PHP's
/// format used in `Function [ ... ]` and `Method [ ... ]` representations:
///   string -> 'foo' (single quotes)
///   bool -> true|false (lowercase)
///   null -> NULL
///   array -> [a, b, c]
///   numeric -> as-is
fn format_reflection_default(v: &Value) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        Value::True => "true".to_string(),
        Value::False => "false".to_string(),
        Value::Long(n) => n.to_string(),
        Value::Double(f) => crate::value::format_php_float(*f),
        Value::String(s) => {
            let raw = s.to_string_lossy();
            format!("'{}'", raw.replace('\\', "\\\\").replace('\'', "\\'"))
        }
        Value::Array(a) => {
            let arr = a.borrow();
            let mut parts: Vec<String> = Vec::new();
            let mut last_key: i64 = -1;
            let mut all_packed = true;
            for (k, _) in arr.iter() {
                match k {
                    crate::array::ArrayKey::Int(n) => {
                        if *n == last_key + 1 {
                            last_key = *n;
                        } else {
                            all_packed = false;
                            break;
                        }
                    }
                    _ => { all_packed = false; break; }
                }
            }
            for (k, v) in arr.iter() {
                if all_packed {
                    parts.push(format_reflection_default(v));
                } else {
                    let key_s = match k {
                        crate::array::ArrayKey::Int(n) => n.to_string(),
                        crate::array::ArrayKey::String(s) => format!("'{}'", String::from_utf8_lossy(s.as_bytes())),
                    };
                    parts.push(format!("{} => {}", key_s, format_reflection_default(v)));
                }
            }
            format!("[{}]", parts.join(", "))
        }
        _ => "?".to_string(),
    }
}

fn extract_param_default(op_array: &crate::opcode::OpArray, cv_idx: u32) -> Option<Value> {
    use crate::opcode::{OpCode, OperandType};
    // Scan only the first portion of the function (the default-value prelude
    // sits before any real body opcode that reads/writes other CVs in non-
    // trivial ways). We stop once we encounter an opcode outside the prelude
    // pattern that doesn't touch parameter defaults.
    for op in op_array.ops.iter() {
        if op.opcode == OpCode::Assign {
            if let OperandType::Cv(target_cv) = op.op1 {
                if target_cv == cv_idx {
                    if let OperandType::Const(const_idx) = op.op2 {
                        if let Some(v) = op_array.literals.get(const_idx as usize) {
                            return Some(v.clone());
                        }
                    }
                }
            }
        }
    }
    None
}

/// Identify the kind of default value for the parameter at `cv_idx`.
/// Returns Some("global:<NAME>") for a global constant default
/// (emitted by ConstLookup), Some("class:<Class>::<CONST>") for a class
/// constant default (StaticPropGet with class+name), or None.
fn extract_param_default_constant(
    op_array: &crate::opcode::OpArray,
    cv_idx: u32,
) -> Option<String> {
    use crate::opcode::{OpCode, OperandType};
    // Find the Assign for this CV; the op2 should be a Tmp(t) that was
    // produced by the immediately preceding ConstLookup or StaticPropGet.
    for (i, op) in op_array.ops.iter().enumerate() {
        if op.opcode != OpCode::Assign { continue; }
        if let OperandType::Cv(target_cv) = op.op1 {
            if target_cv != cv_idx { continue; }
            let src_tmp = match op.op2 {
                OperandType::Tmp(t) => t,
                _ => continue,
            };
            // Search backwards for the op producing this Tmp.
            for j in (0..i).rev() {
                let prev = &op_array.ops[j];
                let writes_tmp = match prev.result {
                    OperandType::Tmp(t) => t == src_tmp,
                    _ => false,
                };
                if !writes_tmp { continue; }
                match prev.opcode {
                    OpCode::ConstLookup => {
                        if let OperandType::Const(idx) = prev.op1 {
                            if let Some(Value::String(s)) = op_array.literals.get(idx as usize) {
                                return Some(format!("global:{}", s.to_string_lossy()));
                            }
                        }
                    }
                    OpCode::StaticPropGet => {
                        if let (OperandType::Const(ci), OperandType::Const(ni)) = (prev.op1, prev.op2) {
                            let cls = match op_array.literals.get(ci as usize) {
                                Some(Value::String(s)) => s.to_string_lossy(),
                                _ => return None,
                            };
                            let nm = match op_array.literals.get(ni as usize) {
                                Some(Value::String(s)) => s.to_string_lossy(),
                                _ => return None,
                            };
                            return Some(format!("class:{}::{}", cls, nm));
                        }
                    }
                    _ => {}
                }
                break;
            }
            return None;
        }
    }
    None
}

/// Resolve the underlying OpArray for a ReflectionParameter by checking either
/// the `__reflection_func` property (free function/closure) or the class+method
/// properties (method parameters).
fn reflection_parameter_op_array(vm: &Vm, obj: &Rc<RefCell<PhpObject>>) -> Option<crate::opcode::OpArray> {
    let ob = obj.borrow();
    let func_target = ob.get_property(b"__reflection_func").to_php_string();
    let func_bytes = func_target.as_bytes().to_vec();
    drop(ob);
    if !func_bytes.is_empty() {
        if let Some(oa) = vm.user_functions.get(&func_bytes) {
            return Some(oa.clone());
        }
    }
    // Fall back to class+method lookup for method parameters.
    let ob = obj.borrow();
    let class_name = ob.get_property(b"__reflection_param_class").to_php_string();
    let method_name = ob.get_property(b"__reflection_param_method").to_php_string();
    drop(ob);
    let class_bytes = class_name.as_bytes().to_vec();
    let method_bytes = method_name.as_bytes().to_vec();
    if !class_bytes.is_empty() && !method_bytes.is_empty() {
        let class_lower: Vec<u8> = class_bytes.iter().map(|b| b.to_ascii_lowercase()).collect();
        let method_lower: Vec<u8> = method_bytes.iter().map(|b| b.to_ascii_lowercase()).collect();
        if let Some(ce) = vm.classes.get(&class_lower) {
            if let Some(m) = ce.methods.get(&method_lower) {
                return Some(m.op_array.clone());
            }
        }
    }
    None
}

/// ReflectionParameter no-arg method dispatch
pub fn reflection_parameter_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    let func_target = ob.get_property(b"__reflection_func").to_php_string();
    let param_idx = ob.get_property(b"__reflection_param_idx").to_long() as usize;
    let cv_offset = ob.get_property(b"__reflection_param_cv_offset").to_long() as usize;
    drop(ob);
    let resolved_op_array = reflection_parameter_op_array(vm, obj);
    // CV index for type-info lookups (cv_names / param_types arrays). For free
    // functions and static methods this equals `param_idx`; for non-static
    // methods we shift past the synthetic `$this` slot at index 0.
    let type_idx = param_idx + cv_offset;

    match method {
        b"getname" => {
            let ob = obj.borrow();
            Some(ob.get_property(b"name"))
        }
        b"getposition" => {
            Some(Value::Long(param_idx as i64))
        }
        b"isoptional" => {
            if let Some(op_array) = resolved_op_array.as_ref() {
                Some(if param_idx >= op_array.required_param_count as usize {
                    Value::True
                } else {
                    Value::False
                })
            } else {
                Some(Value::False)
            }
        }
        b"hasdefaultvalue" | b"isdefaultvalueavailable" => {
            if let Some(op_array) = resolved_op_array.as_ref() {
                Some(if param_idx >= op_array.required_param_count as usize {
                    Value::True
                } else {
                    Value::False
                })
            } else {
                Some(Value::False)
            }
        }
        b"getdefaultvalue" => {
            // Try to extract default value from compiled opcode prelude.
            if let Some(op_array) = resolved_op_array.as_ref() {
                if param_idx >= op_array.required_param_count as usize {
                    if let Some(v) = extract_param_default(op_array, type_idx as u32) {
                        return Some(v);
                    }
                    // If the default is a constant reference, try to resolve it.
                    if let Some(name) = extract_param_default_constant(op_array, type_idx as u32) {
                        if let Some(rest) = name.strip_prefix("class:") {
                            // class:<ClassName>::<CONST>
                            if let Some(idx) = rest.find("::") {
                                let cls = &rest[..idx];
                                let const_name = &rest[idx + 2..];
                                // Handle self/static/parent
                                let resolved_class: Vec<u8> = match cls {
                                    "self" | "static" => {
                                        // Use declaring class of the param if known
                                        let ob = obj.borrow();
                                        let pc = ob.get_property(b"__reflection_param_class").to_php_string();
                                        drop(ob);
                                        if pc.as_bytes().is_empty() {
                                            cls.as_bytes().to_vec()
                                        } else {
                                            pc.as_bytes().to_vec()
                                        }
                                    }
                                    "parent" => {
                                        let ob = obj.borrow();
                                        let pc = ob.get_property(b"__reflection_param_class").to_php_string();
                                        drop(ob);
                                        let pcl: Vec<u8> = pc.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                                        vm.classes.get(&pcl).and_then(|c| c.parent.clone()).unwrap_or_else(|| cls.as_bytes().to_vec())
                                    }
                                    _ => cls.as_bytes().to_vec(),
                                };
                                let cl: Vec<u8> = resolved_class.iter().map(|b| b.to_ascii_lowercase()).collect();
                                // Walk class hierarchy for constant
                                let mut current = Some(cl);
                                while let Some(cl) = current {
                                    if let Some(ce) = vm.classes.get(&cl) {
                                        if let Some(c) = ce.constants.get(const_name.as_bytes()) {
                                            return Some(c.clone());
                                        }
                                        current = ce.parent.as_ref().map(|p| p.iter().map(|b| b.to_ascii_lowercase()).collect());
                                    } else {
                                        break;
                                    }
                                }
                            }
                        } else if let Some(rest) = name.strip_prefix("global:") {
                            let const_name = rest.trim_start_matches('\\');
                            if let Some(v) = vm.constants.get(const_name.as_bytes()) {
                                return Some(v.clone());
                            }
                            // Case-insensitive built-ins
                            let lower: Vec<u8> = const_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                            match lower.as_slice() {
                                b"true" => return Some(Value::True),
                                b"false" => return Some(Value::False),
                                b"null" => return Some(Value::Null),
                                _ => {}
                            }
                        }
                    }
                }
            }
            Some(Value::Null)
        }
        b"allowsnull" => {
            // No type hint -> allows null (PHP default behavior).
            // Otherwise: depends on whether the type permits null.
            if let Some(op_array) = resolved_op_array.as_ref() {
                if type_idx < op_array.param_types.len() {
                    if let Some(pti) = &op_array.param_types[type_idx] {
                        return Some(if param_type_allows_null(&pti.param_type) {
                            Value::True
                        } else {
                            Value::False
                        });
                    }
                }
            }
            Some(Value::True)
        }
        b"isvariadic" => {
            if let Some(op_array) = resolved_op_array.as_ref() {
                if let Some(variadic_idx) = op_array.variadic_param {
                    Some(if type_idx == variadic_idx as usize {
                        Value::True
                    } else {
                        Value::False
                    })
                } else {
                    Some(Value::False)
                }
            } else {
                Some(Value::False)
            }
        }
        b"ispassedbyreference" => {
            if let Some(op_array) = resolved_op_array.as_ref() {
                if type_idx < op_array.by_ref_params.len() {
                    return Some(if op_array.by_ref_params[type_idx] { Value::True } else { Value::False });
                }
            }
            Some(Value::False)
        }
        b"hastype" => {
            if let Some(op_array) = resolved_op_array.as_ref() {
                let has = type_idx < op_array.param_types.len()
                    && op_array.param_types[type_idx].is_some();
                Some(if has { Value::True } else { Value::False })
            } else {
                Some(Value::False)
            }
        }
        b"gettype" => {
            let param_type = resolved_op_array.as_ref()
                .and_then(|op_array| {
                    if type_idx < op_array.param_types.len() {
                        op_array.param_types[type_idx].as_ref().map(|pti| pti.param_type.clone())
                    } else {
                        None
                    }
                });
            // Derive a scope class for self/parent resolution from the
            // parameter's declaring class (set when reflecting a method).
            let scope: Option<Vec<u8>> = {
                let ob = obj.borrow();
                let class_lower = ob.get_property(b"__reflection_param_class").to_php_string();
                drop(ob);
                let cb = class_lower.as_bytes();
                if cb.is_empty() {
                    None
                } else {
                    let cl: Vec<u8> = cb.iter().map(|b| b.to_ascii_lowercase()).collect();
                    Some(vm.classes.get(&cl).map(|c| c.name.clone()).unwrap_or_else(|| cb.to_vec()))
                }
            };
            if let Some(pt) = param_type {
                Some(create_reflection_type_scoped(vm, &pt, scope.as_deref()))
            } else {
                Some(Value::Null)
            }
        }
        b"getdeclaringfunction" => {
            // Return a ReflectionFunction/ReflectionMethod for the declaring
            // function. If the parameter belongs to a method (class+method
            // were stored when the parameter was created), construct a
            // ReflectionMethod instead.
            let ob = obj.borrow();
            let class_opt = ob.get_property(b"__reflection_param_class").to_php_string();
            let method_opt = ob.get_property(b"__reflection_param_method").to_php_string();
            // For functions, __reflection_func has the lowercase function name.
            let func_lower = ob.get_property(b"__reflection_func").to_php_string();
            drop(ob);

            let class_bytes = class_opt.as_bytes();
            let method_bytes = method_opt.as_bytes();
            if !class_bytes.is_empty() && !method_bytes.is_empty() {
                // Build a ReflectionMethod
                let class_str = String::from_utf8_lossy(class_bytes).to_string();
                let method_str = String::from_utf8_lossy(method_bytes).to_string();
                let canonical_class = vm.classes.get(class_bytes)
                    .map(|c| String::from_utf8_lossy(&c.name).to_string())
                    .unwrap_or_else(|| class_str.clone());
                let obj_id = vm.next_object_id();
                let mut rm_obj = PhpObject::new(b"ReflectionMethod".to_vec(), obj_id);
                rm_obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(method_str.clone())));
                rm_obj.set_property(b"class".to_vec(), Value::String(PhpString::from_string(canonical_class)));
                rm_obj.set_property(b"__reflection_class".to_vec(), Value::String(PhpString::from_vec(class_bytes.to_vec())));
                rm_obj.set_property(b"__reflection_method".to_vec(), Value::String(PhpString::from_vec(method_bytes.to_vec())));
                return Some(Value::Object(Rc::new(RefCell::new(rm_obj))));
            }

            // Free function path. Resolve the canonical function name from
            // the user_functions table if available; otherwise fall back to
            // the stored lowercase name.
            let lower = func_lower.as_bytes();
            let canonical_func = if !lower.is_empty() {
                vm.user_functions.get(lower)
                    .map(|op| String::from_utf8_lossy(&op.name).to_string())
                    .unwrap_or_else(|| String::from_utf8_lossy(lower).to_string())
            } else {
                String::new()
            };
            let obj_id = vm.next_object_id();
            let mut rf_obj = PhpObject::new(b"ReflectionFunction".to_vec(), obj_id);
            rf_obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(canonical_func)));
            rf_obj.set_property(b"__reflection_target".to_vec(), Value::String(func_target.clone()));
            Some(Value::Object(Rc::new(RefCell::new(rf_obj))))
        }
        b"getdeclaringclass" => {
            // Returns null for functions, ReflectionClass for methods.
            let ob = obj.borrow();
            let class_lower = ob.get_property(b"__reflection_param_class").to_php_string();
            drop(ob);
            let bytes = class_lower.as_bytes();
            if !bytes.is_empty() {
                if let Some(ce) = vm.classes.get(bytes) {
                    let name_str = String::from_utf8_lossy(&ce.name).to_string();
                    return Some(create_reflection_class(vm, &name_str));
                }
            }
            Some(Value::Null)
        }
        b"getclass" => {
            // Deprecated since 8.0: returns ReflectionClass if param type hints a class
            vm.emit_deprecated_raw(
                "Method ReflectionParameter::getClass() is deprecated since 8.0, use ReflectionParameter::getType() instead",
                vm.current_line,
            );
            let class_name_opt = resolved_op_array.as_ref()
                .and_then(|op| {
                    if type_idx < op.param_types.len() {
                        op.param_types[type_idx].as_ref().map(|pti| pti.param_type.clone())
                    } else { None }
                })
                .and_then(|pt| {
                    fn unwrap_simple(pt: &crate::opcode::ParamType) -> Option<Vec<u8>> {
                        match pt {
                            crate::opcode::ParamType::Simple(n) => Some(n.clone()),
                            crate::opcode::ParamType::Nullable(inner) => unwrap_simple(inner.as_ref()),
                            _ => None,
                        }
                    }
                    unwrap_simple(&pt)
                });
            if let Some(name) = class_name_opt {
                let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
                let is_scalar = matches!(lower.as_slice(),
                    b"int" | b"integer" | b"long" | b"float" | b"double" | b"real" |
                    b"string" | b"bool" | b"boolean" | b"array" | b"object" | b"void" |
                    b"null" | b"mixed" | b"iterable" | b"callable" | b"false" | b"true" |
                    b"never"
                );
                if is_scalar {
                    return Some(Value::Null);
                }
                // Resolve self/parent/static against the parameter's declaring class
                let ob = obj.borrow();
                let decl_class_bytes = ob.get_property(b"__reflection_param_class").to_php_string();
                drop(ob);
                let decl_lower: Vec<u8> = decl_class_bytes.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                let resolved_lower: Vec<u8> = match lower.as_slice() {
                    b"self" | b"static" => decl_lower.clone(),
                    b"parent" => vm.classes.get(&decl_lower).and_then(|c| c.parent.as_ref().map(|p| p.iter().map(|b| b.to_ascii_lowercase()).collect())).unwrap_or_default(),
                    _ => lower,
                };
                if vm.classes.contains_key(&resolved_lower) {
                    let name_str = String::from_utf8_lossy(&vm.classes.get(&resolved_lower).map(|c| c.name.clone()).unwrap_or_else(|| resolved_lower.clone())).to_string();
                    return Some(create_reflection_class(vm, &name_str));
                }
            }
            Some(Value::Null)
        }
        b"isarray" => {
            // Deprecated since 8.0, returns true if param type is array.
            vm.emit_deprecated_raw(
                "Method ReflectionParameter::isArray() is deprecated since 8.0, use ReflectionParameter::getType() instead",
                vm.current_line,
            );
            let is_array = resolved_op_array.as_ref()
                .and_then(|op| {
                    if type_idx < op.param_types.len() {
                        op.param_types[type_idx].as_ref().map(|pti| pti.param_type.clone())
                    } else { None }
                })
                .map(|pt| match &pt {
                    crate::opcode::ParamType::Simple(n) => n.as_slice() == b"array",
                    crate::opcode::ParamType::Nullable(inner) => matches!(inner.as_ref(),
                        crate::opcode::ParamType::Simple(n) if n.as_slice() == b"array"),
                    _ => false,
                })
                .unwrap_or(false);
            Some(if is_array { Value::True } else { Value::False })
        }
        b"iscallable" => {
            // Deprecated since 8.0, returns true if param type is callable.
            vm.emit_deprecated_raw(
                "Method ReflectionParameter::isCallable() is deprecated since 8.0, use ReflectionParameter::getType() instead",
                vm.current_line,
            );
            let is_callable = resolved_op_array.as_ref()
                .and_then(|op| {
                    if type_idx < op.param_types.len() {
                        op.param_types[type_idx].as_ref().map(|pti| pti.param_type.clone())
                    } else { None }
                })
                .map(|pt| match &pt {
                    crate::opcode::ParamType::Simple(n) => n.as_slice() == b"callable",
                    crate::opcode::ParamType::Nullable(inner) => matches!(inner.as_ref(),
                        crate::opcode::ParamType::Simple(n) if n.as_slice() == b"callable"),
                    _ => false,
                })
                .unwrap_or(false);
            Some(if is_callable { Value::True } else { Value::False })
        }
        b"isdefaultvalueconstant" => {
            if let Some(op_array) = resolved_op_array.as_ref() {
                if extract_param_default_constant(op_array, type_idx as u32).is_some() {
                    return Some(Value::True);
                }
            }
            Some(Value::False)
        }
        b"getdefaultvalueconstantname" => {
            if let Some(op_array) = resolved_op_array.as_ref() {
                if let Some(name) = extract_param_default_constant(op_array, type_idx as u32) {
                    let display = if let Some(rest) = name.strip_prefix("global:") {
                        // Strip leading backslash from fully-qualified names
                        rest.trim_start_matches('\\').to_string()
                    } else if let Some(rest) = name.strip_prefix("class:") {
                        rest.to_string()
                    } else {
                        name
                    };
                    return Some(Value::String(PhpString::from_string(display)));
                }
            }
            Some(Value::Null)
        }
        b"ispromoted" => {
            Some(Value::False)
        }
        b"canbepassedbyvalue" => {
            if let Some(op_array) = resolved_op_array.as_ref() {
                if type_idx < op_array.by_ref_params.len() && op_array.by_ref_params[type_idx] {
                    return Some(Value::False);
                }
            }
            Some(Value::True)
        }
        b"__tostring" => {
            let ob = obj.borrow();
            let name = ob.get_property(b"name").to_php_string().to_string_lossy();
            drop(ob);
            let is_variadic = resolved_op_array.as_ref()
                .and_then(|op| op.variadic_param)
                .map(|v| v as usize == type_idx)
                .unwrap_or(false);
            // Variadic parameters are always considered "optional" in PHP's
            // ReflectionParameter::__toString (PHP renders them with the
            // <optional> tag and a ... prefix, no default-value suffix).
            let is_optional = is_variadic || if let Some(op_array) = resolved_op_array.as_ref() {
                param_idx >= op_array.required_param_count as usize
            } else {
                false
            };
            let kind = if is_optional { "optional" } else { "required" };
            let variadic_prefix = if is_variadic { "..." } else { "" };
            let default_suffix = if is_optional && !is_variadic {
                if let Some(op_array) = resolved_op_array.as_ref() {
                    // Prefer constant reference rendering when available
                    if let Some(const_ref) = extract_param_default_constant(op_array, type_idx as u32) {
                        let display = if let Some(rest) = const_ref.strip_prefix("global:") {
                            rest.trim_start_matches('\\').to_string()
                        } else if let Some(rest) = const_ref.strip_prefix("class:") {
                            rest.to_string()
                        } else {
                            const_ref
                        };
                        format!(" = {}", display)
                    } else if let Some(v) = extract_param_default(op_array, type_idx as u32) {
                        format!(" = {}", format_reflection_default(&v))
                    } else { String::new() }
                } else { String::new() }
            } else { String::new() };
            Some(Value::String(PhpString::from_string(
                format!("Parameter #{} [ <{}> {}${}{} ]", param_idx, kind, variadic_prefix, name, default_suffix)
            )))
        }
        _ => None,
    }
}

/// ReflectionConstant method dispatch (PHP 8.3+)
pub fn reflection_constant_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    let name = ob.get_property(b"name").to_php_string().to_string_lossy();
    drop(ob);

    match method {
        b"getname" => {
            Some(Value::String(PhpString::from_string(name)))
        }
        b"getnamespacename" => {
            // Strip a leading backslash so the namespace prefix is reported
            // in canonical (unprefixed) form, matching PHP.
            let n = name.strip_prefix('\\').unwrap_or(&name);
            if let Some(pos) = n.rfind('\\') {
                Some(Value::String(PhpString::from_string(n[..pos].to_string())))
            } else {
                Some(Value::String(PhpString::empty()))
            }
        }
        b"getshortname" => {
            if let Some(pos) = name.rfind('\\') {
                Some(Value::String(PhpString::from_string(name[pos + 1..].to_string())))
            } else {
                Some(Value::String(PhpString::from_string(name)))
            }
        }
        b"getvalue" => {
            // Look up the constant value
            let const_lower: Vec<u8> = name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
            if let Some(val) = vm.constants.get(name.as_bytes()) {
                Some(val.clone())
            } else if let Some(val) = vm.constants.get(&const_lower[..]) {
                Some(val.clone())
            } else {
                Some(Value::Null)
            }
        }
        b"isdefault" | b"isdeprecated" => {
            Some(Value::False)
        }
        b"getfilename" => {
            Some(Value::False)
        }
        b"getextension" => {
            Some(Value::Null)
        }
        b"getextensionname" => {
            Some(Value::False)
        }
        b"getattributes" => {
            let attrs = constant_attrs_lookup(vm, name.as_bytes()).unwrap_or_default();
            Some(create_reflection_attributes(vm, &attrs, None, 0, 64))
        }
        b"__tostring" => {
            Some(Value::String(PhpString::from_string(format!("Constant [ {} ]\n", name))))
        }
        _ => None,
    }
}

fn constant_attrs_lookup(vm: &Vm, name: &[u8]) -> Option<Vec<crate::object::RuntimeAttribute>> {
    if let Some(a) = vm.constant_attributes.get(name) {
        return Some(a.clone());
    }
    // Fall back to lowercase
    let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
    if let Some(a) = vm.constant_attributes.get(&lower[..]) {
        return Some(a.clone());
    }
    None
}

pub fn reflection_constant_docall(
    vm: &mut Vm,
    method: &[u8],
    args: &[Value],
) -> Option<Value> {
    let this = args.first()?;
    if let Value::Object(obj) = this {
        let ob = obj.borrow();
        let name = ob.get_property(b"name").to_php_string().to_string_lossy();
        drop(ob);
        match method {
            b"getattributes" => {
                let attrs = constant_attrs_lookup(vm, name.as_bytes()).unwrap_or_default();
                let filter_name = args.get(1).and_then(|v| if matches!(v, Value::Null) { None } else { Some(v.to_php_string().as_bytes().to_vec()) });
                let filter_flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);
                // TARGET_CONSTANT = 64
                Some(create_reflection_attributes(vm, &attrs, filter_name.as_deref(), filter_flags, 64))
            }
            _ => None,
        }
    } else {
        None
    }
}

/// ReflectionGenerator method dispatch (no args). The `__reflection_target`
/// property holds the underlying Generator value.
pub fn reflection_generator_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    // Helper closure to read the underlying generator (if still set).
    let get_gen = |obj: &Rc<RefCell<PhpObject>>| -> Option<Rc<RefCell<crate::generator::PhpGenerator>>> {
        let ob = obj.borrow();
        match ob.get_property(b"__reflection_target") {
            Value::Generator(g) => Some(g),
            _ => None,
        }
    };

    match method {
        b"getexecutingfile" => {
            if let Some(g) = get_gen(obj) {
                let gb = g.borrow();
                if gb.state == crate::generator::GeneratorState::Completed && !gb.is_running {
                    let exc = vm.create_exception(b"ReflectionException", "Cannot fetch information from a closed Generator", vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                let fname = String::from_utf8_lossy(&gb.op_array.filename).to_string();
                if !fname.is_empty() {
                    return Some(Value::String(PhpString::from_string(fname)));
                }
            }
            Some(Value::String(PhpString::from_string(vm.current_file.clone())))
        }
        b"getexecutingline" => {
            if let Some(g) = get_gen(obj) {
                let gb = g.borrow();
                if gb.state == crate::generator::GeneratorState::Completed && !gb.is_running {
                    let exc = vm.create_exception(b"ReflectionException", "Cannot fetch information from a closed Generator", vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                // For a generator not yet started, return the function's
                // declaration line. Once running, use the line of the op at
                // the current ip.
                if gb.state == crate::generator::GeneratorState::Created {
                    return Some(Value::Long(gb.op_array.decl_line as i64));
                }
                let line = gb.op_array.ops.get(gb.ip).map(|op| op.line as i64).unwrap_or(0);
                if line > 0 { return Some(Value::Long(line)); }
                return Some(Value::Long(gb.op_array.decl_line as i64));
            }
            Some(Value::Long(vm.current_line as i64))
        }
        b"getfunction" => {
            // Return a ReflectionFunction for the generator's function.
            let target = {
                let ob = obj.borrow();
                ob.get_property(b"__reflection_target")
            };
            if let Value::Generator(g) = &target {
                let gb = g.borrow();
                let fn_name = String::from_utf8_lossy(&gb.op_array.name).to_string();
                drop(gb);
                let obj_id = vm.next_object_id();
                let mut rf_obj = PhpObject::new(b"ReflectionFunction".to_vec(), obj_id);
                let display = if fn_name.is_empty() { "{closure}".to_string() } else { fn_name.clone() };
                rf_obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(display)));
                rf_obj.set_property(b"__reflection_target".to_vec(), Value::String(PhpString::from_string(fn_name.to_ascii_lowercase())));
                return Some(Value::Object(Rc::new(RefCell::new(rf_obj))));
            }
            Some(Value::Null)
        }
        b"getthis" => {
            if let Some(g) = get_gen(obj) {
                let gb = g.borrow();
                if !gb.cvs.is_empty() {
                    if let Value::Object(_) = &gb.cvs[0] {
                        return Some(gb.cvs[0].clone());
                    }
                }
            }
            Some(Value::Null)
        }
        b"getexecutinggenerator" => {
            let ob = obj.borrow();
            Some(ob.get_property(b"__reflection_target"))
        }
        b"gettrace" => {
            // A closed generator cannot produce a trace.
            if let Some(g) = get_gen(obj) {
                let gb = g.borrow();
                if gb.state == crate::generator::GeneratorState::Completed && !gb.is_running {
                    let exc = vm.create_exception(b"ReflectionException", "Cannot fetch information from a closed Generator", vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
            }
            // Empty trace - generators don't track call frames here.
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
        }
        b"isclosed" => {
            if let Some(g) = get_gen(obj) {
                let gb = g.borrow();
                let closed = gb.state == crate::generator::GeneratorState::Completed && !gb.is_running;
                return Some(if closed { Value::True } else { Value::False });
            }
            Some(Value::False)
        }
        _ => None,
    }
}

/// ReflectionFiber method dispatch (no args). `__reflection_target` is the
/// Fiber object.
pub fn reflection_fiber_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    match method {
        b"getfiber" => {
            let ob = obj.borrow();
            Some(ob.get_property(b"__reflection_target"))
        }
        b"getcallable" => {
            let ob = obj.borrow();
            let fib = ob.get_property(b"__reflection_target");
            drop(ob);
            if let Value::Object(f) = &fib {
                let fb = f.borrow();
                Some(fb.get_property(b"__fiber_callable"))
            } else {
                Some(Value::Null)
            }
        }
        b"getexecutingfile" => {
            Some(Value::String(PhpString::from_string(vm.current_file.clone())))
        }
        b"getexecutingline" => {
            Some(Value::Long(vm.current_line as i64))
        }
        b"gettrace" => {
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
        }
        _ => None,
    }
}

/// ReflectionExtension no-arg method dispatch
pub fn reflection_extension_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    let name = ob.get_property(b"name").to_php_string().to_string_lossy();
    drop(ob);

    let ext_lower: String = name.to_ascii_lowercase();

    match method {
        b"getname" => {
            Some(Value::String(PhpString::from_string(name)))
        }
        b"getversion" => {
            Some(Value::String(PhpString::from_bytes(b"8.5.4")))
        }
        b"getfunctions" => {
            // Enumerate all registered internal and user functions that belong
            // to this extension (matched via get_function_extension()). Return
            // a string-keyed map of ReflectionFunction objects.
            let mut result = PhpArray::new();
            let func_names: Vec<String> = vm.functions.keys()
                .map(|k| String::from_utf8_lossy(k).to_string())
                .collect();
            for fname in func_names {
                let ext = get_function_extension(&fname).unwrap_or("standard");
                if ext.eq_ignore_ascii_case(&ext_lower) || (ext_lower == "standard" && get_function_extension(&fname).is_none()) {
                    let rf = create_reflection_function(vm, &fname);
                    result.set(
                        ArrayKey::String(PhpString::from_string(fname)),
                        rf,
                    );
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"getclasses" => {
            let mut result = PhpArray::new();
            let class_list = get_extension_classes(&ext_lower);
            for cname in class_list {
                if vm.is_known_builtin_class(&cname.to_ascii_lowercase().as_bytes().to_vec())
                    || vm.classes.contains_key(cname.to_ascii_lowercase().as_bytes())
                {
                    let rc = create_reflection_class(vm, cname);
                    result.set(
                        ArrayKey::String(PhpString::from_string(cname.to_string())),
                        rc,
                    );
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"getclassnames" => {
            let mut result = PhpArray::new();
            let class_list = get_extension_classes(&ext_lower);
            for cname in class_list {
                if vm.is_known_builtin_class(&cname.to_ascii_lowercase().as_bytes().to_vec())
                    || vm.classes.contains_key(cname.to_ascii_lowercase().as_bytes())
                {
                    result.push(Value::String(PhpString::from_string(cname.to_string())));
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"getconstants" => {
            let mut result = PhpArray::new();
            let const_list = get_extension_constants(&ext_lower);
            for (cname, val) in const_list {
                let v = vm.constants.get(cname.as_bytes()).cloned().unwrap_or(val);
                result.set(
                    ArrayKey::String(PhpString::from_string(cname.to_string())),
                    v,
                );
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"getinientries" => {
            let mut result = PhpArray::new();
            let ini_list = get_extension_ini_entries(&ext_lower);
            for entry in ini_list {
                let v = vm.constants.get(entry.as_bytes())
                    .map(|v| Value::String(v.to_php_string()))
                    .unwrap_or_else(|| Value::String(PhpString::from_bytes(b"")));
                result.set(
                    ArrayKey::String(PhpString::from_string(entry.to_string())),
                    v,
                );
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"getdependencies" => {
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
        }
        b"info" => {
            // Mimic PHP's per-extension MINFO block. Print the section banner,
            // then a minimal block of key=>value entries.
            let banner = match ext_lower.as_str() {
                "reflection" => "Reflection\n\nReflection => enabled\n\n".to_string(),
                "date" => {
                    let mut s = String::new();
                    s.push_str("date\n\n");
                    s.push_str("date/time support => enabled\n");
                    s.push_str("timelib version => 2025.01\n");
                    s.push_str("\"Olson\" Timezone Database Version => 0.system\n");
                    s.push_str("Timezone Database => internal\n");
                    let tz = vm.constants.get(b"__default_timezone".as_ref())
                        .map(|v| v.to_php_string().to_string_lossy())
                        .unwrap_or_else(|| "UTC".to_string());
                    s.push_str(&format!("Default timezone => {}\n", tz));
                    s.push_str("\n");
                    s.push_str("Directive => Local Value => Master Value\n");
                    s.push_str(&format!("date.timezone => {} => {}\n", tz, tz));
                    s.push_str("date.default_latitude => 31.7667 => 31.7667\n");
                    s.push_str("date.default_longitude => 35.2333 => 35.2333\n");
                    s.push_str("date.sunset_zenith => 90.833333 => 90.833333\n");
                    s.push_str("date.sunrise_zenith => 90.833333 => 90.833333\n");
                    s
                }
                _ => format!("{}\n\n{} => enabled\n", name, name),
            };
            vm.write_output(banner.as_bytes());
            Some(Value::Null)
        }
        b"ispersistent" => {
            Some(Value::True)
        }
        b"istemporary" => {
            Some(Value::False)
        }
        b"__tostring" => {
            Some(Value::String(PhpString::from_string(format!(
                "Extension [ <persistent> extension #0 {} version <{}> ] {{\n}}\n",
                name, "8.5.4"
            ))))
        }
        _ => None,
    }
}

fn get_extension_classes(ext: &str) -> &'static [&'static str] {
    match ext {
        "reflection" => &[
            "ReflectionException", "Reflection", "Reflector",
            "ReflectionFunctionAbstract",
            "ReflectionFunction", "ReflectionGenerator",
            "ReflectionParameter", "ReflectionType",
            "ReflectionNamedType", "ReflectionUnionType", "ReflectionIntersectionType",
            "ReflectionMethod",
            "ReflectionClass", "ReflectionObject", "ReflectionProperty",
            "ReflectionClassConstant",
            "ReflectionExtension", "ReflectionZendExtension",
            "ReflectionReference", "ReflectionAttribute", "ReflectionEnum",
            "ReflectionEnumUnitCase", "ReflectionEnumBackedCase",
            "ReflectionFiber", "ReflectionConstant",
            "PropertyHookType",
        ],
        "standard" => &[
            "AssertionError", "Directory", "RoundingMode", "StreamBucket",
            "__PHP_Incomplete_Class", "php_user_filter",
        ],
        "core" => &[
            "stdClass", "Exception", "Error", "TypeError", "ValueError",
            "ArgumentCountError", "ArithmeticError", "DivisionByZeroError",
            "Closure", "Generator", "WeakMap", "WeakReference",
            "UnhandledMatchError",
        ],
        "spl" => &[
            "ArrayObject", "ArrayIterator", "RecursiveArrayIterator",
            "SplStack", "SplQueue", "SplDoublyLinkedList", "SplPriorityQueue",
            "SplObjectStorage", "SplFixedArray", "SplHeap", "SplMinHeap", "SplMaxHeap",
            "SplFileInfo", "SplFileObject", "SplTempFileObject",
            "DirectoryIterator", "FilesystemIterator", "RecursiveDirectoryIterator",
            "GlobIterator", "AppendIterator", "MultipleIterator",
            "CachingIterator", "RecursiveCachingIterator",
            "RecursiveTreeIterator", "LimitIterator", "NoRewindIterator",
            "RegexIterator", "RecursiveRegexIterator",
            "RuntimeException", "OutOfBoundsException", "OutOfRangeException",
            "LengthException", "LogicException", "InvalidArgumentException",
            "RangeException", "UnderflowException", "OverflowException",
            "UnexpectedValueException", "BadMethodCallException", "BadFunctionCallException",
            "DomainException",
        ],
        _ => &[],
    }
}

fn get_extension_ini_entries(ext: &str) -> &'static [&'static str] {
    match ext {
        "standard" => &[
            "user_agent", "default_socket_timeout", "from",
            "auto_detect_line_endings", "url_rewriter.tags", "url_rewriter.hosts",
            "default_mimetype", "default_charset",
        ],
        _ => &[],
    }
}

fn get_extension_constants(ext: &str) -> Vec<(&'static str, Value)> {
    match ext {
        "standard" => vec![
            ("CONNECTION_NORMAL", Value::Long(0)),
            ("CONNECTION_ABORTED", Value::Long(1)),
            ("CONNECTION_TIMEOUT", Value::Long(2)),
            ("INI_USER", Value::Long(1)),
            ("INI_PERDIR", Value::Long(2)),
            ("INI_SYSTEM", Value::Long(4)),
            ("INI_ALL", Value::Long(7)),
            ("PHP_URL_SCHEME", Value::Long(0)),
            ("PHP_URL_HOST", Value::Long(1)),
            ("PHP_URL_PORT", Value::Long(2)),
            ("PHP_URL_USER", Value::Long(3)),
            ("PHP_URL_PASS", Value::Long(4)),
            ("PHP_URL_PATH", Value::Long(5)),
            ("PHP_URL_QUERY", Value::Long(6)),
            ("PHP_URL_FRAGMENT", Value::Long(7)),
        ],
        _ => vec![],
    }
}

fn create_reflection_function(vm: &mut Vm, func_name: &str) -> Value {
    let obj_id = vm.next_object_id();
    let mut rf_obj = PhpObject::new(b"ReflectionFunction".to_vec(), obj_id);
    rf_obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(func_name.to_string())));
    let lower = func_name.to_ascii_lowercase();
    rf_obj.set_property(b"__reflection_target".to_vec(), Value::String(PhpString::from_string(lower)));
    Value::Object(Rc::new(RefCell::new(rf_obj)))
}

/// ReflectionNamedType no-arg method dispatch
pub fn reflection_named_type_method(
    _vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    let type_name = ob.get_property(b"__type_name").to_php_string().to_string_lossy();
    let allows_null = ob.get_property(b"__allows_null");
    let is_builtin_type = ob.get_property(b"__is_builtin");
    drop(ob);

    match method {
        b"getname" => {
            Some(Value::String(PhpString::from_string(type_name)))
        }
        b"allowsnull" => {
            Some(if matches!(allows_null, Value::True) { Value::True } else { Value::False })
        }
        b"isbuiltin" => {
            Some(if matches!(is_builtin_type, Value::True) { Value::True } else { Value::False })
        }
        b"__tostring" => {
            let ob = obj.borrow();
            let nullable = ob.get_property(b"__allows_null");
            let name = ob.get_property(b"__type_name").to_php_string().to_string_lossy();
            drop(ob);
            if matches!(nullable, Value::True) && name != "null" && name != "mixed" {
                Some(Value::String(PhpString::from_string(format!("?{}", name))))
            } else {
                Some(Value::String(PhpString::from_string(name)))
            }
        }
        _ => None,
    }
}

/// ReflectionUnionType / ReflectionIntersectionType method dispatch
pub fn reflection_composite_type_method(
    _vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    let allows_null = ob.get_property(b"__allows_null");
    drop(ob);

    match method {
        b"gettypes" => {
            let ob = obj.borrow();
            Some(ob.get_property(b"__types"))
        }
        b"allowsnull" => {
            Some(if matches!(allows_null, Value::True) { Value::True } else { Value::False })
        }
        b"__tostring" => {
            // Recursive helper: render a ReflectionType into a string.
            // Named types -> their __type_name (with leading ? if implicitly nullable named type).
            // Union types  -> children joined with `|`.
            // Intersection types -> children joined with `&` and wrapped in parens
            // when the parent is a union (DNF rendering).
            fn render(v: &Value, parent_union: bool) -> String {
                if let Value::Object(t) = v {
                    let tb = t.borrow();
                    let cn: Vec<u8> = tb.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                    if cn == b"reflectionnamedtype" {
                        let name = tb.get_property(b"__type_name").to_php_string().to_string_lossy();
                        // Only union types include "null" as a separate member, so
                        // we don't emit `?` here.
                        name
                    } else if cn == b"reflectionuniontype" {
                        let types = tb.get_property(b"__types");
                        drop(tb);
                        if let Value::Array(arr) = types {
                            let parts: Vec<String> = arr.borrow().iter().map(|(_, v)| render(v, true)).collect();
                            parts.join("|")
                        } else {
                            String::new()
                        }
                    } else if cn == b"reflectionintersectiontype" {
                        let types = tb.get_property(b"__types");
                        drop(tb);
                        if let Value::Array(arr) = types {
                            let parts: Vec<String> = arr.borrow().iter().map(|(_, v)| render(v, false)).collect();
                            let joined = parts.join("&");
                            if parent_union {
                                format!("({})", joined)
                            } else {
                                joined
                            }
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            }

            let ob = obj.borrow();
            let class_lower: Vec<u8> = ob.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            drop(ob);
            // Render with the appropriate "parent is union" flag depending on
            // self class.
            let self_value = Value::Object(obj.clone());
            let _ = class_lower; // already encoded in render via class_name
            Some(Value::String(PhpString::from_string(render(&self_value, false))))
        }
        _ => None,
    }
}

/// ReflectionClassConstant method dispatch
pub fn reflection_class_constant_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    let const_name = ob.get_property(b"name").to_php_string().to_string_lossy();
    let class_name = ob.get_property(b"class").to_php_string().to_string_lossy();
    let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

    // Look up constant metadata
    let meta = vm.classes.get(&class_lower)
        .and_then(|ce| ce.constants_meta.get(const_name.as_bytes()).cloned());

    let visibility = meta.as_ref().map(|m| m.visibility).unwrap_or(Visibility::Public);
    let is_final = meta.as_ref().map(|m| m.is_final).unwrap_or(false);

    match method {
        b"getname" => Some(ob.get_property(b"name")),
        b"getvalue" => Some(ob.get_property(b"__reflection_value")),
        b"getdeclaringclass" => {
            let declaring = meta.as_ref()
                .map(|m| {
                    let dc_lower = &m.declaring_class;
                    vm.classes.get(dc_lower.as_slice())
                        .map(|c| String::from_utf8_lossy(&c.name).to_string())
                        .unwrap_or(class_name.clone())
                })
                .unwrap_or(class_name.clone());
            drop(ob);
            Some(create_reflection_class(vm, &declaring))
        }
        b"ispublic" => Some(if visibility == Visibility::Public { Value::True } else { Value::False }),
        b"isprotected" => Some(if visibility == Visibility::Protected { Value::True } else { Value::False }),
        b"isprivate" => Some(if visibility == Visibility::Private { Value::True } else { Value::False }),
        b"getmodifiers" => {
            let mut mods = 0i64;
            match visibility {
                Visibility::Public => mods |= 1,
                Visibility::Protected => mods |= 2,
                Visibility::Private => mods |= 4,
            }
            if is_final { mods |= 0x20; }
            Some(Value::Long(mods))
        }
        b"getdoccomment" => {
            if let Some(doc) = meta.as_ref().and_then(|m| m.doc_comment.clone()) {
                Some(Value::String(PhpString::from_string(doc)))
            } else {
                Some(Value::False)
            }
        }
        b"isfinal" => Some(if is_final { Value::True } else { Value::False }),
        b"isenumcase" => {
            // A ReflectionClassConstant is an enum case only when the
            // constant was declared as `case Foo;` on an enum — not when
            // it's a regular constant that *happens* to reference an enum
            // case value (e.g. `const Baz = self::Bar;`).
            let is_case = vm.classes.get(&class_lower)
                .map(|ce| ce.enum_cases.iter().any(|(cn, _)| cn == const_name.as_bytes()))
                .unwrap_or(false);
            drop(ob);
            Some(if is_case { Value::True } else { Value::False })
        }
        b"isdeprecated" => {
            let has_dep = meta.as_ref().map(|m| {
                m.attributes.iter().any(|a| {
                    let n = String::from_utf8_lossy(&a.name);
                    n.eq_ignore_ascii_case("deprecated") || n.eq_ignore_ascii_case("\\deprecated")
                })
            }).unwrap_or(false);
            Some(if has_dep { Value::True } else { Value::False })
        }
        b"hastype" => {
            let has = meta.as_ref().map(|m| m.declared_type.is_some()).unwrap_or(false);
            Some(if has { Value::True } else { Value::False })
        }
        b"gettype" => {
            let pt = meta.as_ref().and_then(|m| m.declared_type.clone());
            if let Some(pt) = pt {
                Some(create_reflection_type(vm, &pt))
            } else {
                Some(Value::Null)
            }
        }
        // ReflectionEnumUnitCase/BackedCase methods
        b"getenum" => {
            drop(ob);
            Some(create_reflection_class(vm, &class_name))
        }
        b"getbackingvalue" => {
            // For backed enum cases, return the backing value
            let val = ob.get_property(b"__reflection_value");
            drop(ob);
            if let Value::Object(enum_obj) = &val {
                let eo = enum_obj.borrow();
                let backing = eo.get_property(b"value");
                if matches!(backing, Value::Null) && !eo.has_property(b"value") {
                    // Not a backed case
                    let case_name_str = const_name.clone();
                    let err_msg = format!("Enum case {}::{} is not a backed case", class_name, case_name_str);
                    let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    Some(Value::Null)
                } else {
                    Some(backing)
                }
            } else {
                Some(Value::Null)
            }
        }
        b"__tostring" => {
            let name = ob.get_property(b"name").to_php_string().to_string_lossy();
            let val = ob.get_property(b"__reflection_value");
            drop(ob);
            let vis_str = match visibility {
                Visibility::Public => "public",
                Visibility::Protected => "protected",
                Visibility::Private => "private",
            };
            let final_str = if is_final { "final " } else { "" };
            let type_str = reflection_const_type_name(meta.as_ref().and_then(|m| m.declared_type.as_ref()), &val)
                .map(|n| format!("{} ", n))
                .unwrap_or_default();
            let val_str = reflection_const_value_repr(&val);
            let mut s = String::new();
            if let Some(doc) = meta.as_ref().and_then(|m| m.doc_comment.clone()) {
                s.push_str(&doc);
                s.push('\n');
            }
            s.push_str(&format!("Constant [ {}{} {}{} ] {{ {} }}\n", final_str, vis_str, type_str, name, val_str));
            Some(Value::String(PhpString::from_string(s)))
        }
        _ => None,
    }
}

/// ReflectionParameter methods with args
pub fn reflection_parameter_docall(
    vm: &mut Vm,
    method: &[u8],
    args: &[Value],
) -> Option<Value> {
    let this = args.first()?;
    if let Value::Object(obj) = this {
        let ob = obj.borrow();
        let func_target = ob.get_property(b"__reflection_func").to_php_string();
        let param_idx = ob.get_property(b"__reflection_param_idx").to_long() as usize;
        let param_class = ob.get_property(b"__reflection_param_class");
        let param_method = ob.get_property(b"__reflection_param_method");
        let position = ob.get_property(b"__reflection_param_position").to_long() as usize;
        drop(ob);

        match method {
            b"getattributes" => {
                let filter_name = args.get(1).and_then(|v| if matches!(v, Value::Null) { None } else { Some(v.to_php_string().as_bytes().to_vec()) });
                let filter_flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);

                if !matches!(&param_class, Value::Null) {
                    let class_lower = param_class.to_php_string().as_bytes().to_vec();
                    let method_lower = param_method.to_php_string().as_bytes().to_vec();
                    let attrs = vm.classes.get(&class_lower)
                        .and_then(|c| c.get_method(&method_lower))
                        .and_then(|m| m.op_array.param_attributes.get(position))
                        .cloned()
                        .unwrap_or_default();
                    Some(create_reflection_attributes(vm, &attrs, filter_name.as_deref(), filter_flags, 32))
                } else {
                    let attrs = vm.user_functions.get(func_target.as_bytes())
                        .and_then(|op| op.param_attributes.get(param_idx))
                        .cloned()
                        .unwrap_or_default();
                    Some(create_reflection_attributes(vm, &attrs, filter_name.as_deref(), filter_flags, 32))
                }
            }
            _ => None,
        }
    } else {
        None
    }
}

/// ReflectionClassConstant methods with args
pub fn reflection_class_constant_docall(
    vm: &mut Vm,
    method: &[u8],
    args: &[Value],
) -> Option<Value> {
    let this = args.first()?;
    if let Value::Object(obj) = this {
        let ob = obj.borrow();
        let const_name = ob.get_property(b"name").to_php_string().to_string_lossy();
        let class_name = ob.get_property(b"class").to_php_string().to_string_lossy();
        let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
        drop(ob);

        match method {
            b"getattributes" => {
                let meta = vm.classes.get(&class_lower)
                    .and_then(|ce| ce.constants_meta.get(const_name.as_bytes()).cloned());
                let attrs = meta.as_ref().map(|m| m.attributes.clone()).unwrap_or_default();
                let filter_name = args.get(1).and_then(|v| if matches!(v, Value::Null) { None } else { Some(v.to_php_string().as_bytes().to_vec()) });
                let filter_flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);
                Some(create_reflection_attributes(vm, &attrs, filter_name.as_deref(), filter_flags, 16))
            }
            _ => None,
        }
    } else {
        None
    }
}

/// ReflectionAttribute methods with args
pub fn reflection_attribute_docall(
    vm: &mut Vm,
    method: &[u8],
    args: &[Value],
) -> Option<Value> {
    let this = args.first()?;
    if let Value::Object(obj) = this {
        // Delegate to the no-arg handler, which handles newInstance
        reflection_attribute_method(vm, method, obj)
    } else {
        None
    }
}

/// Handle static method calls on Reflection classes
/// Wrapper that includes the original (case-preserved) method name for error messages.
pub fn reflection_static_call_named(vm: &mut Vm, class_lower: &str, method_lower: &str, method_original: &str, args: &[Value], line: u32) -> Option<Value> {
    // Forbid calling non-static instance methods statically. We list common
    // ReflectionClass/Method/Property/etc instance methods that have no
    // legitimate static dispatch.
    let is_instance_only = match class_lower {
        "reflectionclass" | "reflectionobject" | "reflectionenum" => matches!(
            method_lower,
            "getname" | "getshortname" | "getnamespacename" | "getfilename"
                | "getstartline" | "getendline" | "getdoccomment"
                | "isiterateable" | "isiterable" | "isabstract" | "isfinal"
                | "isinterface" | "istrait" | "isenum" | "isanonymous"
                | "isinstantiable" | "iscloneable" | "isuserdefined" | "isinternal"
                | "isreadonly"
                | "getmodifiers" | "getmethods" | "getproperties" | "getconstants"
                | "getreflectionconstants" | "getmethod" | "getproperty"
                | "hasmethod" | "hasproperty" | "hasconstant" | "getconstant"
                | "getclassconstant" | "getreflectionconstant" | "hasreflectionconstant"
                | "getinterfaces" | "getinterfacenames"
                | "getparentclass" | "implementsinterface"
                | "issubclassof" | "isinstance"
                | "newinstance" | "newinstanceargs" | "newinstancewithoutconstructor"
                | "getconstructor" | "getstaticproperties"
                | "getstaticpropertyvalue" | "setstaticpropertyvalue"
                | "getdefaultproperties" | "gettraits" | "gettraitnames"
                | "gettraitaliases" | "getattributes" | "getextension"
                | "getextensionname" | "inNamespace" | "innamespace"
                | "__tostring" | "getcases" | "iscase" | "hascase" | "getcase"
                | "getbackingtype"
        ),
        "reflectionmethod" => matches!(
            method_lower,
            "getname" | "getshortname" | "getnamespacename" | "getfilename"
                | "getstartline" | "getendline" | "getdoccomment"
                | "isuserdefined" | "isinternal" | "isabstract" | "isfinal"
                | "ispublic" | "isprotected" | "isprivate" | "isstatic"
                | "isconstructor" | "isdestructor" | "iscloneable"
                | "isvariadic" | "isgenerator" | "isclosure" | "isdeprecated"
                | "returnsreference" | "hasreturntype" | "getreturntype"
                | "hastentativereturntype" | "gettentativereturntype"
                | "getmodifiers" | "getparameters" | "getnumberofparameters"
                | "getnumberofrequiredparameters" | "getclosure"
                | "getclosurethis" | "getclosurescopeclass" | "getclosurecalledclass"
                | "getclosureusedvariables" | "getdeclaringclass" | "getprototype"
                | "hasprototype" | "getattributes" | "getextension"
                | "getextensionname" | "__tostring" | "invoke" | "invokeargs"
                | "settaccessible"
        ),
        "reflectionproperty" => matches!(
            method_lower,
            "getname" | "getvalue" | "setvalue" | "isinitialized"
                | "hasdefaultvalue" | "getdefaultvalue" | "getdeclaringclass"
                | "getmodifiers" | "ispublic" | "isprotected" | "isprivate"
                | "isstatic" | "isreadonly" | "ispromoted" | "isdefault"
                | "isvirtual" | "getdoccomment" | "gettype" | "hastype"
                | "getattributes" | "settaccessible" | "__tostring"
                | "gethooks" | "hashook" | "gethook" | "getrawvalue" | "setrawvalue"
                | "setrawvaluewithouthook" | "isdynamic" | "isabstract" | "isfinal"
        ),
        "reflectionfunction" => matches!(
            method_lower,
            "getname" | "getshortname" | "getnamespacename" | "getfilename"
                | "getstartline" | "getendline" | "getdoccomment"
                | "isuserdefined" | "isinternal" | "isdisabled"
                | "isclosure" | "isgenerator" | "isvariadic" | "isdeprecated"
                | "returnsreference" | "hasreturntype" | "getreturntype"
                | "hastentativereturntype" | "gettentativereturntype"
                | "getparameters" | "getnumberofparameters"
                | "getnumberofrequiredparameters" | "getclosure"
                | "getclosurethis" | "getclosurescopeclass" | "getclosurecalledclass"
                | "getclosureusedvariables" | "getattributes" | "getextension"
                | "getextensionname" | "__tostring" | "invoke" | "invokeargs"
        ),
        "reflectionparameter" => matches!(
            method_lower,
            "getname" | "getposition" | "ispassedbyreference"
                | "canbepassedbyvalue" | "getdeclaringfunction"
                | "getdeclaringclass" | "getclass" | "hastype" | "gettype"
                | "isarray" | "iscallable" | "allowsnull" | "isoptional"
                | "isdefaultvalueavailable" | "getdefaultvalue"
                | "isdefaultvalueconstant" | "getdefaultvalueconstantname"
                | "isvariadic" | "ispromoted" | "getattributes" | "__tostring"
        ),
        _ => false,
    };
    if is_instance_only {
        // Match PHP's casing: keep the original method name as written.
        // PHP uses canonical class names: ReflectionClass, ReflectionMethod etc.
        // For subclasses (ReflectionObject extends ReflectionClass), report the
        // declaring class name in the error message — that's how Zend reports
        // "Non-static method ReflectionClass::getName() cannot be called statically"
        // even when the user wrote ReflectionObject::getName().
        let class_canonical = match class_lower {
            "reflectionclass" => "ReflectionClass",
            "reflectionobject" => "ReflectionClass",
            "reflectionenum" => "ReflectionClass",
            "reflectionmethod" => "ReflectionMethod",
            "reflectionproperty" => "ReflectionProperty",
            "reflectionfunction" => "ReflectionFunction",
            "reflectionparameter" => "ReflectionParameter",
            _ => class_lower,
        };
        let err_msg = format!(
            "Non-static method {}::{}() cannot be called statically",
            class_canonical, method_original
        );
        let exc = vm.create_exception(b"Error", &err_msg, line);
        vm.current_exception = Some(exc);
        return Some(Value::Null);
    }
    reflection_static_call(vm, class_lower, method_lower, args, line)
}

pub fn reflection_static_call(vm: &mut Vm, class_lower: &str, method_lower: &str, args: &[Value], line: u32) -> Option<Value> {
    match class_lower {
        "reflection" => {
            match method_lower {
                "getmodifiernames" => {
                    let modifiers = args.first().map(|v| v.to_long()).unwrap_or(0);
                    let mut names = PhpArray::new();
                    // Order matches PHP: abstract, final, public, protected,
                    // private, static, readonly, virtual, protected(set), private(set).
                    if modifiers & 0x40 != 0 { names.push(Value::String(PhpString::from_bytes(b"abstract"))); }
                    if modifiers & 0x20 != 0 { names.push(Value::String(PhpString::from_bytes(b"final"))); }
                    if modifiers & 1 != 0 { names.push(Value::String(PhpString::from_bytes(b"public"))); }
                    if modifiers & 2 != 0 { names.push(Value::String(PhpString::from_bytes(b"protected"))); }
                    if modifiers & 4 != 0 { names.push(Value::String(PhpString::from_bytes(b"private"))); }
                    if modifiers & 0x10 != 0 { names.push(Value::String(PhpString::from_bytes(b"static"))); }
                    if modifiers & 0x10000 != 0 { names.push(Value::String(PhpString::from_bytes(b"readonly"))); }
                    if modifiers & 0x200000 != 0 { names.push(Value::String(PhpString::from_bytes(b"virtual"))); }
                    if modifiers & 0x800 != 0 { names.push(Value::String(PhpString::from_bytes(b"protected(set)"))); }
                    if modifiers & 0x1000 != 0 { names.push(Value::String(PhpString::from_bytes(b"private(set)"))); }
                    Some(Value::Array(Rc::new(RefCell::new(names))))
                }
                _ => None,
            }
        }
        "reflectionmethod" => {
            match method_lower {
                "createfrommethodname" => {
                    let method_str = args.first().map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
                    if let Some(pos) = method_str.find("::") {
                        let class_name = &method_str[..pos];
                        let method_name = &method_str[pos + 2..];
                        // Create a ReflectionMethod
                        let class_lower_bytes: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                        let method_lower_bytes: Vec<u8> = method_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

                        // Check class exists
                        if !vm.classes.contains_key(&class_lower_bytes) {
                            let err_msg = format!("Class \"{}\" does not exist", class_name);
                            let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
                            vm.current_exception = Some(exc);
                            return Some(Value::Null);
                        }

                        // Check method exists
                        let method_exists = vm.classes.get(&class_lower_bytes)
                            .map(|c| c.get_method(&method_lower_bytes).is_some())
                            .unwrap_or(false);

                        if !method_exists {
                            let canonical_class = vm.classes.get(&class_lower_bytes)
                                .map(|c| String::from_utf8_lossy(&c.name).to_string())
                                .unwrap_or(class_name.to_string());
                            let err_msg = format!("Method {}::{}() does not exist", canonical_class, method_name);
                            let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
                            vm.current_exception = Some(exc);
                            return Some(Value::Null);
                        }

                        let canonical_class = vm.classes.get(&class_lower_bytes)
                            .map(|c| String::from_utf8_lossy(&c.name).to_string())
                            .unwrap_or(class_name.to_string());
                        Some(create_reflection_method(vm, &canonical_class, method_name))
                    } else {
                        let err_msg = "ReflectionMethod::createFromMethodName(): Argument #1 ($method) must be a valid method name".to_string();
                        let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
                        vm.current_exception = Some(exc);
                        Some(Value::Null)
                    }
                }
                _ => None,
            }
        }
        "reflectionclass" | "reflectionobject" => {
            match method_lower {
                "export" => {
                    // Deprecated, return null
                    Some(Value::Null)
                }
                _ => None,
            }
        }
        "reflectionenum" => {
            match method_lower {
                "export" => Some(Value::Null),
                _ => None,
            }
        }
        _ => None,
    }
}


/// Create a ReflectionClass object for a given class name
pub fn create_reflection_class(vm: &mut Vm, class_name: &str) -> Value {
    let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    // If the target class is an enum, materialize a ReflectionEnum so that
    // callers like `getDeclaringClass()::class` see the right runtime type.
    let is_enum = vm.classes.get(&class_lower).map(|c| c.is_enum).unwrap_or(false);
    let reflector = if is_enum { b"ReflectionEnum".to_vec() } else { b"ReflectionClass".to_vec() };
    let obj_id = vm.next_object_id();
    let mut obj = PhpObject::new(reflector, obj_id);
    obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(class_name.to_string())));
    obj.set_property(b"__reflection_target".to_vec(), Value::String(PhpString::from_string(class_name.to_string())));
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Create a ReflectionMethod object for a given class and method name
pub fn create_reflection_method(vm: &mut Vm, class_name: &str, method_name: &str) -> Value {
    create_reflection_method_with_lookup(vm, class_name, method_name, None)
}

/// Create a ReflectionMethod with an optional lookup-class override.
/// `class_name` is the declaring class (used for `$class` / `__reflection_class`).
/// `lookup_class` is the class the method was originally fetched from; it
/// surfaces as `<user, inherits Declaring>` in __toString output when it
/// differs from the declaring class.
pub fn create_reflection_method_with_lookup(vm: &mut Vm, class_name: &str, method_name: &str, lookup_class: Option<&str>) -> Value {
    let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    let method_lower: Vec<u8> = method_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

    let obj_id = vm.next_object_id();
    let mut obj = PhpObject::new(b"ReflectionMethod".to_vec(), obj_id);

    let canonical_method = vm.classes.get(&class_lower)
        .and_then(|c| c.get_method(&method_lower).map(|m| String::from_utf8_lossy(&m.name).to_string()))
        .unwrap_or_else(|| method_name.to_string());

    let canonical_class = vm.classes.get(&class_lower)
        .map(|c| String::from_utf8_lossy(&c.name).to_string())
        .unwrap_or_else(|| class_name.to_string());

    obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(canonical_method)));
    obj.set_property(b"class".to_vec(), Value::String(PhpString::from_string(canonical_class.clone())));
    obj.set_property(b"__reflection_class".to_vec(), Value::String(PhpString::from_string(canonical_class)));
    obj.set_property(b"__reflection_method".to_vec(), Value::String(PhpString::from_vec(method_lower)));
    if let Some(lookup) = lookup_class {
        let lookup_canonical = {
            let lookup_lower: Vec<u8> = lookup.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
            vm.classes.get(&lookup_lower)
                .map(|c| String::from_utf8_lossy(&c.name).to_string())
                .unwrap_or_else(|| lookup.to_string())
        };
        obj.set_property(b"__reflection_lookup_class".to_vec(),
            Value::String(PhpString::from_string(lookup_canonical)));
    }
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Create a ReflectionProperty object
pub fn create_reflection_property(vm: &mut Vm, class_name: &str, prop_name: &str) -> Value {
    let obj_id = vm.next_object_id();
    let mut obj = PhpObject::new(b"ReflectionProperty".to_vec(), obj_id);
    obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(prop_name.to_string())));
    obj.set_property(b"class".to_vec(), Value::String(PhpString::from_string(class_name.to_string())));
    obj.set_property(b"__reflection_class".to_vec(), Value::String(PhpString::from_string(class_name.to_string())));
    obj.set_property(b"__reflection_prop".to_vec(), Value::String(PhpString::from_string(prop_name.to_string())));
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Create a ReflectionClassConstant object
pub fn create_reflection_class_constant(vm: &mut Vm, class_name: &str, const_name: &str, value: Value) -> Value {
    let obj_id = vm.next_object_id();
    let mut obj = PhpObject::new(b"ReflectionClassConstant".to_vec(), obj_id);
    obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(const_name.to_string())));
    // Walk up the parent chain to find which class actually declares this
    // constant. The visible `class` property and getDeclaringClass() both
    // return the declaring class, not the class the reflection was made on.
    let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
    let const_lower: Vec<u8> = const_name.as_bytes().to_vec();
    let declaring = find_constant_declaring_class(vm, &class_lower, &const_lower)
        .unwrap_or_else(|| class_name.to_string());
    obj.set_property(b"class".to_vec(), Value::String(PhpString::from_string(declaring)));
    obj.set_property(b"__reflection_value".to_vec(), value);
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Walk a class's parent chain to find the class that declares a given
/// static property. Static properties are stored on the declaring class,
/// even when accessed via a subclass.
fn find_static_property_owner(vm: &Vm, class_lower: &[u8], prop_name: &[u8]) -> Option<Vec<u8>> {
    let mut owner: Option<Vec<u8>> = None;
    let mut cur: Option<Vec<u8>> = Some(class_lower.to_vec());
    let mut visited: Vec<Vec<u8>> = Vec::new();
    while let Some(c) = cur {
        if visited.contains(&c) { break; }
        visited.push(c.clone());
        if let Some(ce) = vm.classes.get(c.as_slice()) {
            if ce.properties.iter().any(|p| p.is_static && p.name == prop_name) {
                owner = Some(c.clone());
            }
            cur = ce.parent.as_ref()
                .map(|p| p.iter().map(|b| b.to_ascii_lowercase()).collect());
        } else {
            break;
        }
    }
    owner
}

/// Walk a class's parent chain to find a static property's current value.
fn find_static_property_value(vm: &Vm, class_lower: &[u8], prop_name: &[u8]) -> Option<Value> {
    let owner = find_static_property_owner(vm, class_lower, prop_name)?;
    vm.classes.get(owner.as_slice())
        .and_then(|c| c.static_properties.get(prop_name).cloned())
}

/// Walk a class's parent chain (and interfaces) to find the deepest ancestor
/// that declares the given constant. Used for ReflectionClassConstant::class
/// and ::getDeclaringClass() so that inherited constants report their actual
/// definition site rather than the child class.
fn find_constant_declaring_class(vm: &Vm, class_lower: &[u8], const_name: &[u8]) -> Option<String> {
    let mut owner: Option<String> = None;
    let mut cur: Option<Vec<u8>> = Some(class_lower.to_vec());
    let mut visited: Vec<Vec<u8>> = Vec::new();
    while let Some(c) = cur {
        if visited.contains(&c) { break; }
        visited.push(c.clone());
        if let Some(ce) = vm.classes.get(c.as_slice()) {
            if ce.constants.contains_key(const_name) {
                owner = Some(String::from_utf8_lossy(&ce.name).to_string());
            }
            cur = ce.parent.as_ref()
                .map(|p| p.iter().map(|b| b.to_ascii_lowercase()).collect());
        } else {
            break;
        }
    }
    owner
}

/// Create ReflectionParameter objects for a function's parameters
pub fn create_reflection_parameters(vm: &mut Vm, op_array: &OpArray) -> Value {
    let mut result = PhpArray::new();
    let func_name = String::from_utf8_lossy(&op_array.name).to_string();
    let func_lower: Vec<u8> = op_array.name.iter().map(|b| b.to_ascii_lowercase()).collect();
    for i in 0..op_array.param_count as usize {
        let param_name = if i < op_array.cv_names.len() {
            String::from_utf8_lossy(&op_array.cv_names[i]).to_string()
        } else {
            format!("param{}", i)
        };
        let obj_id = vm.next_object_id();
        let mut obj = PhpObject::new(b"ReflectionParameter".to_vec(), obj_id);
        obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(param_name)));
        obj.set_property(b"__reflection_func".to_vec(), Value::String(PhpString::from_vec(func_lower.clone())));
        obj.set_property(b"__reflection_param_idx".to_vec(), Value::Long(i as i64));
        result.push(Value::Object(Rc::new(RefCell::new(obj))));
    }
    Value::Array(Rc::new(RefCell::new(result)))
}

/// Create ReflectionParameter objects for a method's parameters (skipping $this for non-static methods)
pub fn create_reflection_parameters_method(vm: &mut Vm, op_array: &OpArray, is_static: bool) -> Value {
    let mut result = PhpArray::new();
    let skip = if !is_static { 1usize } else { 0usize };
    let param_count = if op_array.param_count as usize > skip { op_array.param_count as usize - skip } else { 0 };
    for i in 0..param_count {
        let cv_idx = i + skip;
        let param_name = if cv_idx < op_array.cv_names.len() {
            String::from_utf8_lossy(&op_array.cv_names[cv_idx]).to_string()
        } else {
            format!("param{}", i)
        };
        let obj_id = vm.next_object_id();
        let mut obj = PhpObject::new(b"ReflectionParameter".to_vec(), obj_id);
        obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(param_name)));
        // Store the user-visible parameter position (0-based, excludes $this).
        obj.set_property(b"__reflection_param_idx".to_vec(), Value::Long(i as i64));
        // Store the CV index offset (1 for non-static methods because of $this,
        // 0 for static methods). Used to map back to the OpArray's cv_names /
        // param_types arrays where $this occupies slot 0 for non-static methods.
        obj.set_property(b"__reflection_param_cv_offset".to_vec(), Value::Long(skip as i64));
        // Store class and method info so the OpArray can be re-fetched at call
        // time. Required for isOptional / hasType / getType / etc. on methods.
        if let Some(ref scope) = op_array.scope_class {
            obj.set_property(b"__reflection_param_class".to_vec(), Value::String(PhpString::from_vec(scope.clone())));
            obj.set_property(b"__reflection_param_method".to_vec(), Value::String(PhpString::from_vec(op_array.name.iter().map(|b| b.to_ascii_lowercase()).collect())));
        }
        // Store param position (0-based, adjusted for $this skip)
        obj.set_property(b"__reflection_param_position".to_vec(), Value::Long(i as i64));
        result.push(Value::Object(Rc::new(RefCell::new(obj))));
    }
    Value::Array(Rc::new(RefCell::new(result)))
}

/// Determine whether a ParamType permits null.
fn param_type_allows_null(pt: &ParamType) -> bool {
    match pt {
        ParamType::Simple(name) => matches!(name.as_slice(), b"null" | b"mixed"),
        ParamType::Nullable(_) => true,
        ParamType::Union(items) => items.iter().any(param_type_allows_null),
        ParamType::Intersection(_) => false,
    }
}

/// Create a ReflectionType object from a ParamType
pub fn create_reflection_type(vm: &mut Vm, param_type: &ParamType) -> Value {
    create_reflection_type_scoped(vm, param_type, None)
}

/// Create a ReflectionType with optional scope class name (canonical) used to
/// resolve `self` references in PHP types. `static` is left as the literal
/// string "static" (it's a late-bound type).
pub fn create_reflection_type_scoped(vm: &mut Vm, param_type: &ParamType, scope_class: Option<&[u8]>) -> Value {
    // Helper: resolve a simple type name with `self`/`parent` substitution.
    // `self` resolves to the declaring class; `parent` resolves to that class's
    // direct parent (canonical name preserved). `static` is left literal.
    fn resolve_name(vm: &Vm, name: &[u8], scope: Option<&[u8]>) -> Vec<u8> {
        if name == b"self" {
            if let Some(s) = scope {
                return s.to_vec();
            }
        }
        if name == b"parent" {
            if let Some(s) = scope {
                let scope_lower: Vec<u8> = s.iter().map(|b| b.to_ascii_lowercase()).collect();
                if let Some(ce) = vm.classes.get(&scope_lower) {
                    if let Some(parent) = ce.parent.as_ref() {
                        // Try canonical name from class entry
                        let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                        if let Some(parent_ce) = vm.classes.get(&parent_lower) {
                            return parent_ce.name.clone();
                        }
                        return parent.clone();
                    }
                }
            }
        }
        name.to_vec()
    }
    match param_type {
        ParamType::Simple(name) => {
            let resolved = resolve_name(vm, name, scope_class);
            let type_name = String::from_utf8_lossy(&resolved).to_string();
            let obj_id = vm.next_object_id();
            let mut obj = PhpObject::new(b"ReflectionNamedType".to_vec(), obj_id);
            let is_builtin = matches!(
                name.as_slice(),
                b"int" | b"float" | b"string" | b"bool" | b"array" | b"callable"
                    | b"void" | b"null" | b"mixed" | b"never" | b"object"
                    | b"iterable" | b"false" | b"true"
            );
            // `null` and `mixed` implicitly allow null
            let allows_null = matches!(name.as_slice(), b"null" | b"mixed");
            obj.set_property(b"__type_name".to_vec(), Value::String(PhpString::from_string(type_name)));
            obj.set_property(b"__allows_null".to_vec(), if allows_null { Value::True } else { Value::False });
            obj.set_property(b"__is_builtin".to_vec(), if is_builtin { Value::True } else { Value::False });
            Value::Object(Rc::new(RefCell::new(obj)))
        }
        ParamType::Nullable(inner) => {
            match inner.as_ref() {
                ParamType::Simple(name) => {
                    let resolved = resolve_name(vm, name, scope_class);
                    let type_name = String::from_utf8_lossy(&resolved).to_string();
                    let obj_id = vm.next_object_id();
                    let mut obj = PhpObject::new(b"ReflectionNamedType".to_vec(), obj_id);
                    let is_builtin = matches!(
                        name.as_slice(),
                        b"int" | b"float" | b"string" | b"bool" | b"array" | b"callable"
                            | b"void" | b"null" | b"mixed" | b"never" | b"object"
                            | b"iterable" | b"false" | b"true"
                    );
                    obj.set_property(b"__type_name".to_vec(), Value::String(PhpString::from_string(type_name)));
                    obj.set_property(b"__allows_null".to_vec(), Value::True);
                    obj.set_property(b"__is_builtin".to_vec(), if is_builtin { Value::True } else { Value::False });
                    Value::Object(Rc::new(RefCell::new(obj)))
                }
                _ => {
                    // Nullable complex type becomes union with null
                    let inner_type = create_reflection_type_scoped(vm, inner, scope_class);
                    let null_type = create_reflection_type_scoped(vm, &ParamType::Simple(b"null".to_vec()), scope_class);
                    let mut types = PhpArray::new();
                    types.push(inner_type);
                    types.push(null_type);
                    let obj_id = vm.next_object_id();
                    let mut obj = PhpObject::new(b"ReflectionUnionType".to_vec(), obj_id);
                    obj.set_property(b"__types".to_vec(), Value::Array(Rc::new(RefCell::new(types))));
                    obj.set_property(b"__allows_null".to_vec(), Value::True);
                    Value::Object(Rc::new(RefCell::new(obj)))
                }
            }
        }
        ParamType::Union(types) => {
            // PHP expands `iterable` inside a union to `Traversable|array`.
            let mut expanded: Vec<ParamType> = Vec::new();
            for t in types {
                match t {
                    ParamType::Simple(n) if n == b"iterable" => {
                        expanded.push(ParamType::Simple(b"Traversable".to_vec()));
                        expanded.push(ParamType::Simple(b"array".to_vec()));
                    }
                    _ => expanded.push(t.clone()),
                }
            }
            let types: &Vec<ParamType> = &expanded;
            // PHP normalizes `T|null` to nullable `?T` and exposes it as a
            // single ReflectionNamedType (not a union).
            let non_null: Vec<&ParamType> = types.iter().filter(|t| !matches!(t, ParamType::Simple(n) if n == b"null")).collect();
            let has_null = types.iter().any(|t| matches!(t, ParamType::Simple(n) if n == b"null"));
            if has_null && non_null.len() == 1 {
                if let ParamType::Simple(name) = non_null[0] {
                    let resolved = resolve_name(vm, name, scope_class);
                    let type_name = String::from_utf8_lossy(&resolved).to_string();
                    let obj_id = vm.next_object_id();
                    let mut obj = PhpObject::new(b"ReflectionNamedType".to_vec(), obj_id);
                    let is_builtin = matches!(
                        name.as_slice(),
                        b"int" | b"float" | b"string" | b"bool" | b"array" | b"callable"
                            | b"void" | b"null" | b"mixed" | b"never" | b"object"
                            | b"iterable" | b"false" | b"true"
                    );
                    obj.set_property(b"__type_name".to_vec(), Value::String(PhpString::from_string(type_name)));
                    obj.set_property(b"__allows_null".to_vec(), Value::True);
                    obj.set_property(b"__is_builtin".to_vec(), if is_builtin { Value::True } else { Value::False });
                    return Value::Object(Rc::new(RefCell::new(obj)));
                }
            }
            let mut type_arr = PhpArray::new();
            let mut allows_null = false;
            for t in types {
                let rt = create_reflection_type_scoped(vm, t, scope_class);
                if param_type_allows_null(t) {
                    allows_null = true;
                }
                type_arr.push(rt);
            }
            let obj_id = vm.next_object_id();
            let mut obj = PhpObject::new(b"ReflectionUnionType".to_vec(), obj_id);
            obj.set_property(b"__types".to_vec(), Value::Array(Rc::new(RefCell::new(type_arr))));
            obj.set_property(b"__allows_null".to_vec(), if allows_null { Value::True } else { Value::False });
            Value::Object(Rc::new(RefCell::new(obj)))
        }
        ParamType::Intersection(types) => {
            let mut type_arr = PhpArray::new();
            for t in types {
                let rt = create_reflection_type_scoped(vm, t, scope_class);
                type_arr.push(rt);
            }
            let obj_id = vm.next_object_id();
            let mut obj = PhpObject::new(b"ReflectionIntersectionType".to_vec(), obj_id);
            obj.set_property(b"__types".to_vec(), Value::Array(Rc::new(RefCell::new(type_arr))));
            obj.set_property(b"__allows_null".to_vec(), Value::False);
            Value::Object(Rc::new(RefCell::new(obj)))
        }
    }
}


/// Helper to check if a class has a constant (walks parent chain)
fn reflection_class_has_constant(vm: &Vm, class_lower: &[u8], const_name: &[u8]) -> bool {
    if let Some(ce) = vm.classes.get(class_lower) {
        if ce.constants.contains_key(const_name) {
            return true;
        }
        // Check parent chain
        if let Some(ref parent) = ce.parent {
            let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
            return reflection_class_has_constant(vm, &parent_lower, const_name);
        }
    }
    false
}

/// Helper to get a constant from a class (walks parent chain). For enum
/// cases stored as the marker `__enum_case__::Name`, resolve to the actual
/// enum case object.
fn reflection_class_get_constant(vm: &mut Vm, class_lower: &[u8], const_name: &[u8]) -> Option<Value> {
    let raw = {
        let ce = vm.classes.get(class_lower)?;
        if let Some(val) = ce.constants.get(const_name) {
            Some(val.clone())
        } else if let Some(ref parent) = ce.parent {
            let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
            // Avoid recursion with mut borrow by releasing ce first.
            drop(ce);
            return reflection_class_get_constant(vm, &parent_lower, const_name);
        } else {
            None
        }
    };
    let raw = raw?;
    // Resolve the enum-case marker to the actual case object.
    if let Value::String(ref s) = raw {
        let sb = s.as_bytes();
        if sb.starts_with(b"__enum_case__::") {
            let case_name = &sb[b"__enum_case__::".len()..];
            if let Some(case_obj) = vm.get_enum_case(class_lower, case_name) {
                return Some(case_obj);
            }
        }
    }
    Some(raw)
}

/// Collect all interfaces reachable from a class by walking its
/// `implements` list, then each interface's `extends` chain, and finally
/// repeating for the class's parent chain. Names are returned in the order
/// PHP emits (BFS-ish: directly declared first, then their parents).
///
/// For interfaces themselves, the `extends` chain is stored as `parent`
/// (rather than `interfaces`), so we additionally walk the parent chain
/// when the target IS an interface.
fn collect_all_interfaces(vm: &Vm, class_lower: &[u8], out: &mut Vec<Vec<u8>>) {
    let mut seen: std::collections::HashSet<Vec<u8>> = std::collections::HashSet::new();
    let starts_as_interface = vm.classes.get(class_lower).map(|c| c.is_interface).unwrap_or(false);
    // Walk class -> parents collecting their `implements` list.
    let mut cur: Vec<u8> = class_lower.to_vec();
    for _ in 0..50 {
        if let Some(ce) = vm.classes.get(&cur) {
            for iface in &ce.interfaces {
                let iface_canonical = iface.clone();
                let iface_lower: Vec<u8> = iface.iter().map(|b| b.to_ascii_lowercase()).collect();
                if !seen.contains(&iface_lower) {
                    seen.insert(iface_lower.clone());
                    out.push(iface_canonical.clone());
                    // Walk this interface's `extends` chain (interfaces are
                    // represented with parent = extended interface).
                    let mut pcur = iface_lower.clone();
                    for _ in 0..50 {
                        let parent = vm.classes.get(&pcur).and_then(|c| c.parent.clone());
                        match parent {
                            Some(p) => {
                                let p_lower: Vec<u8> = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                                if seen.insert(p_lower.clone()) {
                                    out.push(p.clone());
                                }
                                pcur = p_lower;
                            }
                            None => break,
                        }
                    }
                }
            }
            // If our starting class is itself an interface, walk its parent
            // chain (PHP stores interface->interface inheritance as parent).
            if starts_as_interface {
                match &ce.parent {
                    Some(p) => {
                        let p_lower: Vec<u8> = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                        if seen.insert(p_lower.clone()) {
                            out.push(p.clone());
                        }
                        cur = p_lower;
                        continue;
                    }
                    None => break,
                }
            }
            match &ce.parent {
                Some(p) => cur = p.iter().map(|b| b.to_ascii_lowercase()).collect(),
                None => break,
            }
        } else {
            break;
        }
    }
}

/// Collect parent constants into the result array
fn reflection_collect_parent_constants(vm: &Vm, class_lower: &[u8], result: &mut PhpArray) {
    if let Some(ce) = vm.classes.get(class_lower) {
        if let Some(ref parent) = ce.parent {
            let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
            if let Some(pce) = vm.classes.get(&parent_lower) {
                for (name, val) in &pce.constants {
                    let key = ArrayKey::String(PhpString::from_vec(name.clone()));
                    if result.get(&key).is_none() {
                        result.set(key, val.clone());
                    }
                }
            }
            reflection_collect_parent_constants(vm, &parent_lower, result);
        }
    }
}

/// Get modifier flags for a method
fn reflection_method_modifiers(method: &crate::object::MethodDef) -> i64 {
    reflection_method_modifiers_static(method)
}

/// Get modifier flags for a method (static version for use in closures)
fn reflection_method_modifiers_static(method: &crate::object::MethodDef) -> i64 {
    let mut mods = 0i64;
    match method.visibility {
        Visibility::Public => mods |= 1,    // IS_PUBLIC
        Visibility::Protected => mods |= 2, // IS_PROTECTED
        Visibility::Private => mods |= 4,   // IS_PRIVATE
    }
    if method.is_static { mods |= 0x10; }   // IS_STATIC
    if method.is_abstract { mods |= 0x40; } // IS_ABSTRACT
    if method.is_final { mods |= 0x20; }    // IS_FINAL
    mods
}

/// Get modifier flags for a property
fn reflection_property_modifiers(prop: &crate::object::PropertyDef) -> i64 {
    reflection_property_modifiers_static(prop)
}

/// Get modifier flags for a property (static version for use in closures)
fn reflection_property_modifiers_static(prop: &crate::object::PropertyDef) -> i64 {
    let mut mods = 0i64;
    match prop.visibility {
        Visibility::Public => mods |= 1,
        Visibility::Protected => mods |= 2,
        Visibility::Private => mods |= 4,
    }
    if prop.is_static { mods |= 0x10; }
    if prop.is_final { mods |= 0x20; }
    if prop.is_abstract { mods |= 0x40; }
    if prop.is_virtual { mods |= 0x200; }
    if prop.is_readonly { mods |= 0x10000; }
    mods
}

/// Selector for which reflector class is producing the __toString output.
/// Affects the header line: `Class [ ... ]`, `Object of class [ ... ]`,
/// or `Enum [ ... ]`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReflectionStringKind {
    Class,
    Object,
    Enum,
}

/// Build the complete __toString representation for a ReflectionClass
fn reflection_class_to_string(
    vm: &Vm,
    name: &str,
    class_lower: &[u8],
    kind: ReflectionStringKind,
    wrapped_obj: Option<&std::rc::Rc<std::cell::RefCell<PhpObject>>>,
) -> String {
    let mut s = String::new();

    let is_interface = vm.classes.get(class_lower).map(|c| c.is_interface).unwrap_or(false);
    let is_trait = vm.classes.get(class_lower).map(|c| c.is_trait).unwrap_or(false);
    let is_enum = vm.classes.get(class_lower).map(|c| c.is_enum).unwrap_or(false);
    let is_abstract = vm.classes.get(class_lower).map(|c| c.is_abstract).unwrap_or(false);
    let is_final = vm.classes.get(class_lower).map(|c| c.is_final).unwrap_or(false);
    let is_readonly = vm.classes.get(class_lower).map(|c| c.is_readonly).unwrap_or(false);
    // A class is "user-defined" when it has a source filename recorded; built-in
    // classes loaded synthetically into vm.classes have no filename.
    let is_user = vm.classes.get(class_lower).map(|c| c.filename.is_some()).unwrap_or(false);
    let user_or_internal = if is_user { "user" } else { "internal" };

    // Header line
    if is_interface {
        s.push_str(&format!("Interface [ <{}> interface {} ", user_or_internal, name));
    } else if is_trait {
        s.push_str(&format!("Trait [ <{}> trait {} ", user_or_internal, name));
    } else if is_enum || kind == ReflectionStringKind::Enum {
        s.push_str(&format!("Enum [ <{}> enum {} ", user_or_internal, name));
    } else if kind == ReflectionStringKind::Object {
        let mut modifiers = String::new();
        if is_abstract { modifiers.push_str("abstract "); }
        if is_final { modifiers.push_str("final "); }
        if is_readonly { modifiers.push_str("readonly "); }
        s.push_str(&format!("Object of class [ <{}> {}class {} ", user_or_internal, modifiers, name));
    } else {
        let mut modifiers = String::new();
        if is_abstract { modifiers.push_str("abstract "); }
        if is_final { modifiers.push_str("final "); }
        if is_readonly { modifiers.push_str("readonly "); }
        s.push_str(&format!("Class [ <{}> {}class {} ", user_or_internal, modifiers, name));
    }

    // Parent and interfaces
    if let Some(ce) = vm.classes.get(class_lower) {
        if let Some(ref parent) = ce.parent {
            s.push_str(&format!("extends {} ", String::from_utf8_lossy(parent)));
        }
        if !ce.interfaces.is_empty() {
            let keyword = if is_interface { "extends" } else { "implements" };
            s.push_str(&format!("{} ", keyword));
            let ifaces: Vec<String> = ce.interfaces.iter().map(|i| String::from_utf8_lossy(i).to_string()).collect();
            s.push_str(&ifaces.join(", "));
            s.push(' ');
        }
    }
    s.push_str("] {\n");

    // File info
    if let Some(ce) = vm.classes.get(class_lower) {
        if let Some(ref filename) = ce.filename {
            if ce.start_line > 0 {
                let end = if ce.end_line > 0 { ce.end_line } else { ce.start_line };
                s.push_str(&format!("  @@ {} {}-{}\n", filename, ce.start_line, end));
            }
        }
    }

    // Enum cases section (for enums)
    if is_enum {
        if let Some(ce) = vm.classes.get(class_lower) {
            s.push_str(&format!("\n  - Enum cases [{}] {{\n", ce.enum_cases.len()));
            for (case_name, _) in &ce.enum_cases {
                s.push_str(&format!("    Case {}\n", String::from_utf8_lossy(case_name)));
            }
            s.push_str("  }\n");
        }
    }

    // Constants section. Enum cases are constants in the class table but should
    // not appear in the - Constants - section for an enum.
    if let Some(ce) = vm.classes.get(class_lower) {
        let case_names: std::collections::HashSet<&[u8]> = ce.enum_cases.iter().map(|(n, _)| n.as_slice()).collect();
        let regular_consts: Vec<_> = ce.constants.iter().filter(|(n, _)| !case_names.contains(n.as_slice())).collect();
        s.push_str(&format!("\n  - Constants [{}] {{\n", regular_consts.len()));
        for (cname, cval) in regular_consts {
            let const_name_str = String::from_utf8_lossy(cname);
            let meta = ce.constants_meta.get(cname.as_slice());
            let vis = meta.map(|m| m.visibility).unwrap_or(Visibility::Public);
            let is_final_const = meta.map(|m| m.is_final).unwrap_or(false);
            let vis_str = match vis {
                Visibility::Public => "public",
                Visibility::Protected => "protected",
                Visibility::Private => "private",
            };
            let final_str = if is_final_const { "final " } else { "" };
            let type_str = reflection_const_type_name(meta.and_then(|m| m.declared_type.as_ref()), cval)
                .map(|n| format!("{} ", n))
                .unwrap_or_default();
            let val_str = reflection_const_value_repr(cval);
            s.push_str(&format!("    Constant [ {}{} {}{} ] {{ {} }}\n", final_str, vis_str, type_str, const_name_str, val_str));
        }
        s.push_str("  }\n");
    } else {
        s.push_str("\n  - Constants [0] {\n  }\n");
    }

    // Static properties. Inherited private static props are not listed on subclasses.
    if let Some(ce) = vm.classes.get(class_lower) {
        let static_props: Vec<_> = ce.properties.iter()
            .filter(|p| p.is_static)
            .filter(|p| !(p.visibility == Visibility::Private && p.declaring_class != class_lower))
            .collect();
        s.push_str(&format!("\n  - Static properties [{}] {{\n", static_props.len()));
        for prop in static_props {
            let vis = match prop.visibility {
                Visibility::Public => "public",
                Visibility::Protected => "protected",
                Visibility::Private => "private",
            };
            let type_str = prop.property_type.as_ref().map(|t| format!(" {}", param_type_name(t))).unwrap_or_default();
            let prop_name_str = String::from_utf8_lossy(&prop.name);
            // For uninitialized static props, PHP renders the default as NULL.
            let default_str = if matches!(prop.default, Value::Undef) {
                " = NULL".to_string()
            } else {
                format!(" = {}", reflection_value_repr(&prop.default))
            };
            s.push_str(&format!("    Property [ {} static{} ${}{} ]\n", vis, type_str, prop_name_str, default_str));
        }
        s.push_str("  }\n");
    } else {
        s.push_str("\n  - Static properties [0] {\n  }\n");
    }

    // Static methods. Hide property hook synthetic methods. Inherited private
    // methods are not exposed on subclasses.
    if let Some(ce) = vm.classes.get(class_lower) {
        let static_methods: Vec<_> = ce.methods.values()
            .filter(|m| m.is_static)
            .filter(|m| !m.name.starts_with(b"__property_get_") && !m.name.starts_with(b"__property_set_"))
            .filter(|m| !(m.visibility == Visibility::Private && m.declaring_class.iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<_>>() != *class_lower))
            .collect();
        s.push_str(&format!("\n  - Static methods [{}] {{\n", static_methods.len()));
        let total = static_methods.len();
        for (i, m) in static_methods.iter().enumerate() {
            reflection_method_to_string_section(&mut s, m, vm, class_lower);
            if i + 1 < total { s.push('\n'); }
        }
        s.push_str("  }\n");
    } else {
        s.push_str("\n  - Static methods [0] {\n  }\n");
    }

    // Properties (non-static). Inherited private (instance) props are not
    // listed on subclasses.
    if let Some(ce) = vm.classes.get(class_lower) {
        let is_readonly_class = ce.is_readonly;
        let inst_props: Vec<_> = ce.properties.iter()
            .filter(|p| !p.is_static)
            .filter(|p| !(p.visibility == Visibility::Private && p.declaring_class != class_lower))
            .collect();
        s.push_str(&format!("\n  - Properties [{}] {{\n", inst_props.len()));
        for prop in inst_props {
            let vis = match prop.visibility {
                Visibility::Public => "public",
                Visibility::Protected => "protected",
                Visibility::Private => "private",
            };
            // In readonly classes, public properties get implicit
            // protected(set) visibility on the set hook.
            let set_vis_str = if is_readonly_class && prop.visibility == Visibility::Public {
                " protected(set)"
            } else {
                ""
            };
            let readonly_str = if prop.is_readonly { " readonly" } else { "" };
            let type_str = prop.property_type.as_ref().map(|t| format!(" {}", param_type_name(t))).unwrap_or_default();
            let prop_name_str = String::from_utf8_lossy(&prop.name);
            let default_str = if !matches!(prop.default, Value::Undef) {
                format!(" = {}", reflection_value_repr(&prop.default))
            } else {
                String::new()
            };
            s.push_str(&format!("    Property [ {}{}{}{} ${}{} ]\n", vis, set_vis_str, readonly_str, type_str, prop_name_str, default_str));
        }
        s.push_str("  }\n");
    } else {
        s.push_str("\n  - Properties [0] {\n  }\n");
    }

    // For ReflectionObject, render dynamic instance properties (those not
    // declared on the class) in a separate "Dynamic properties" section.
    if kind == ReflectionStringKind::Object {
        if let Some(inst_rc) = wrapped_obj {
            let visible_declared_names: Vec<Vec<u8>> = if let Some(ce) = vm.classes.get(class_lower) {
                ce.properties.iter()
                    .filter(|p| !(p.visibility == Visibility::Private && p.declaring_class != class_lower))
                    .map(|p| p.name.clone())
                    .collect()
            } else { Vec::new() };
            let dyn_props: Vec<(Vec<u8>, Value)> = {
                let io = inst_rc.borrow();
                io.properties.iter()
                    .filter(|(name, _)| !name.starts_with(b"__"))
                    .filter(|(name, _)| !visible_declared_names.iter().any(|n| n == name))
                    .map(|(n, v)| (n.clone(), v.clone()))
                    .collect()
            };
            s.push_str(&format!("\n  - Dynamic properties [{}] {{\n", dyn_props.len()));
            for (pname, _pval) in &dyn_props {
                let prop_name_str = String::from_utf8_lossy(pname);
                s.push_str(&format!("    Property [ <dynamic> public ${} ]\n", prop_name_str));
            }
            s.push_str("  }\n");
        }
    }

    // Methods (non-static). Hide property hook synthetic methods and
    // inherited private methods.
    if let Some(ce) = vm.classes.get(class_lower) {
        let inst_methods: Vec<_> = ce.methods.values()
            .filter(|m| !m.is_static)
            .filter(|m| !m.name.starts_with(b"__property_get_") && !m.name.starts_with(b"__property_set_"))
            .filter(|m| !(m.visibility == Visibility::Private && m.declaring_class != class_lower))
            .collect();
        s.push_str(&format!("\n  - Methods [{}] {{\n", inst_methods.len()));
        let total = inst_methods.len();
        for (i, m) in inst_methods.iter().enumerate() {
            reflection_method_to_string_section(&mut s, m, vm, class_lower);
            if i + 1 < total { s.push('\n'); }
        }
        s.push_str("  }\n");
    } else {
        s.push_str("\n  - Methods [0] {\n  }\n");
    }

    s.push_str("}\n");
    s
}

/// Helper to add a method's __toString to the class output
fn reflection_method_to_string_section(s: &mut String, m: &crate::object::MethodDef, vm: &Vm, owner_lower: &[u8]) {
    let vis = match m.visibility {
        Visibility::Public => "public",
        Visibility::Protected => "protected",
        Visibility::Private => "private",
    };
    let mut modifiers = String::new();
    if m.is_abstract { modifiers.push_str("abstract "); }
    if m.is_final { modifiers.push_str("final "); }
    if m.is_static { modifiers.push_str("static "); }
    let method_name = String::from_utf8_lossy(&m.name);

    // Skip internal hook methods
    if method_name.starts_with("__property_get_") || method_name.starts_with("__property_set_") {
        return;
    }

    // Check if it's a constructor
    let ctor_str = if method_name.eq_ignore_ascii_case("__construct") { ", ctor" } else { "" };

    // Walk parent chain (and interfaces) starting from `start_class` to find
    // the deepest ancestor that visibly declares `method_bytes`. Private
    // ancestor methods are skipped. Returns the canonical name of the
    // first overridden class (closest ancestor) and the deepest prototype.
    fn find_overrides(vm: &Vm, start_class_lower: &[u8], method_bytes: &[u8]) -> (Option<String>, Option<String>) {
        let mut overwrites: Option<String> = None;
        let mut prototype: Option<String> = None;
        let mut visited: Vec<Vec<u8>> = Vec::new();
        // Walk parent chain first (depth-first)
        if let Some(ce) = vm.classes.get(start_class_lower) {
            let mut cur_parent = ce.parent.clone();
            while let Some(ref p) = cur_parent {
                let p_lower: Vec<u8> = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                if visited.contains(&p_lower) { break; }
                visited.push(p_lower.clone());
                if let Some(pce) = vm.classes.get(&p_lower) {
                    if let Some(pm) = pce.get_method(method_bytes) {
                        if pm.visibility != Visibility::Private {
                            // Use the method's declaring class so an inherited
                            // method counts as overwritten at the original
                            // definition site, not the inheriting class.
                            let decl_lower: Vec<u8> = pm.declaring_class.iter().map(|b| b.to_ascii_lowercase()).collect();
                            let canonical = if !decl_lower.is_empty() {
                                vm.classes.get(decl_lower.as_slice())
                                    .map(|c| String::from_utf8_lossy(&c.name).to_string())
                                    .unwrap_or_else(|| String::from_utf8_lossy(&pce.name).to_string())
                            } else {
                                String::from_utf8_lossy(&pce.name).to_string()
                            };
                            if overwrites.is_none() {
                                overwrites = Some(canonical.clone());
                            }
                            prototype = Some(canonical);
                        }
                    }
                    cur_parent = pce.parent.clone();
                } else {
                    break;
                }
            }
            // Also walk interfaces of start class and ancestors
            let mut iface_stack: Vec<Vec<u8>> = ce.interfaces.iter()
                .map(|i| i.iter().map(|b| b.to_ascii_lowercase()).collect())
                .collect();
            // Add interfaces from each ancestor we visited
            for p_lower in &visited {
                if let Some(pce) = vm.classes.get(p_lower.as_slice()) {
                    for i in &pce.interfaces {
                        let il: Vec<u8> = i.iter().map(|b| b.to_ascii_lowercase()).collect();
                        if !iface_stack.contains(&il) { iface_stack.push(il); }
                    }
                }
            }
            let mut iface_seen: Vec<Vec<u8>> = Vec::new();
            while let Some(iface_lower) = iface_stack.pop() {
                if iface_seen.contains(&iface_lower) { continue; }
                iface_seen.push(iface_lower.clone());
                if let Some(ice) = vm.classes.get(&iface_lower) {
                    if let Some(im) = ice.get_method(method_bytes) {
                        if im.visibility != Visibility::Private {
                            let canonical = String::from_utf8_lossy(&ice.name).to_string();
                            // Interface methods take precedence as prototype.
                            prototype = Some(canonical);
                        }
                    }
                    for i in &ice.interfaces {
                        let il: Vec<u8> = i.iter().map(|b| b.to_ascii_lowercase()).collect();
                        if !iface_seen.contains(&il) { iface_stack.push(il); }
                    }
                }
            }
        }
        (overwrites, prototype)
    }

    // For inherited methods, PHP renders "<user, inherits Parent>" in
    // ReflectionClass::__toString output. If the declaring class itself
    // overrode the method from an ancestor, PHP additionally shows
    // ", prototype <Ancestor>". If the method is declared on this class
    // and overrides an ancestor, it renders ", overwrites X, prototype Y".
    let inherits_str = if m.declaring_class.as_slice() != owner_lower {
        let decl_name = vm.classes.get(m.declaring_class.as_slice())
            .map(|c| String::from_utf8_lossy(&c.name).to_string())
            .unwrap_or_default();
        // Walk from the declaring class to find a prototype.
        let (_, prototype_name) = if m.visibility != Visibility::Private {
            find_overrides(vm, m.declaring_class.as_slice(), m.name.as_slice())
        } else {
            (None, None)
        };
        if let Some(proto) = prototype_name {
            format!(", inherits {}, prototype {}", decl_name, proto)
        } else if !decl_name.is_empty() {
            format!(", inherits {}", decl_name)
        } else {
            String::new()
        }
    } else {
        // Method declared on this class — check whether it overrides an
        // ancestor (parent chain or implements an interface method).
        if m.visibility != Visibility::Private {
            let (ow, pt) = find_overrides(vm, owner_lower, m.name.as_slice());
            match (ow, pt) {
                (Some(o), Some(p)) => format!(", overwrites {}, prototype {}", o, p),
                (None, Some(p)) => format!(", prototype {}", p),
                _ => String::new(),
            }
        } else {
            String::new()
        }
    };

    let mod_vis = if modifiers.is_empty() {
        vis.to_string()
    } else {
        format!("{}{}", modifiers, vis)
    };
    s.push_str(&format!("    Method [ <user{}{}> {} method {} ] {{\n", ctor_str, inherits_str, mod_vis, method_name));

    // Add @@ filename startline - endline for the method.
    let m_end = if m.op_array.end_line > 0 { m.op_array.end_line } else { m.op_array.decl_line };
    if m.op_array.decl_line > 0 {
        let fname = String::from_utf8_lossy(&m.op_array.filename);
        let fname_str = if !fname.is_empty() {
            fname.to_string()
        } else {
            // Fallback: use the current vm file? We don't have access to vm here,
            // but reflection passes us the class with filename. Just skip if absent.
            String::new()
        };
        if !fname_str.is_empty() {
            s.push_str(&format!("      @@ {} {} - {}\n", fname_str, m.op_array.decl_line, m_end));
        }
    }

    // Parameters - skip $this (first param for non-static methods).
    // The compiler stores `param_count` as user-declared params + 1 for $this
    // on instance methods, but `required_param_count` is just the user-declared
    // count without $this. So we subtract `skip` from `param_count` only.
    let skip = if !m.is_static { 1usize } else { 0usize };
    let param_count = if m.op_array.param_count as usize > skip { m.op_array.param_count as usize - skip } else { 0 };
    let required_count = m.op_array.required_param_count as usize;

    // PHP omits the "Parameters [0]" section for user methods that
    // also have no return type. When there are parameters, or when
    // a return type is present, the section is emitted.
    let has_return = m.op_array.return_type.is_some();
    if param_count > 0 || has_return {
        s.push_str(&format!("\n      - Parameters [{}] {{\n", param_count));
        for i in 0..param_count {
            let cv_idx = i + skip;
            if cv_idx < m.op_array.cv_names.len() {
                let pname = String::from_utf8_lossy(&m.op_array.cv_names[cv_idx]);
                let is_variadic = m.op_array.variadic_param.map(|v| v as usize == cv_idx).unwrap_or(false);
                let required = !is_variadic && i < required_count;
                let type_str = if cv_idx < m.op_array.param_types.len() {
                    if let Some(ref pti) = m.op_array.param_types[cv_idx] {
                        format!("{} ", param_type_name(&pti.param_type))
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                let default_suffix = if !required && !is_variadic {
                    if let Some(const_ref) = extract_param_default_constant(&m.op_array, cv_idx as u32) {
                        let display = if let Some(rest) = const_ref.strip_prefix("global:") {
                            rest.trim_start_matches('\\').to_string()
                        } else if let Some(rest) = const_ref.strip_prefix("class:") {
                            rest.to_string()
                        } else {
                            const_ref
                        };
                        format!(" = {}", display)
                    } else if let Some(v) = extract_param_default(&m.op_array, cv_idx as u32) {
                        format!(" = {}", format_reflection_default(&v))
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                let variadic_prefix = if is_variadic { "..." } else { "" };
                s.push_str(&format!("        Parameter #{} [ <{}> {}{}${}{} ]\n", i,
                    if required { "required" } else { "optional" }, type_str, variadic_prefix, pname, default_suffix));
            }
        }
        s.push_str("      }\n");
    }

    // Return type
    if let Some(ref rt) = m.op_array.return_type {
        s.push_str(&format!("      - Return [ {} ]\n", param_type_name(rt)));
    }

    s.push_str("    }\n");
}

/// Convert a ParamType to its string representation
fn param_type_name(pt: &crate::opcode::ParamType) -> String {
    use crate::opcode::ParamType;
    match pt {
        ParamType::Simple(name) => String::from_utf8_lossy(name).to_string(),
        ParamType::Nullable(inner) => format!("?{}", param_type_name(inner)),
        ParamType::Union(types) => {
            let names: Vec<String> = types.iter().map(|t| param_type_name(t)).collect();
            names.join("|")
        }
        ParamType::Intersection(types) => {
            let names: Vec<String> = types.iter().map(|t| param_type_name(t)).collect();
            names.join("&")
        }
    }
}

/// Format a value for display in reflection __toString output
/// Render a Value the way ReflectionClassConstant::__toString does. PHP
/// uses single-line var_export style: true=>1, false=>'', null=>'',
/// strings without quotes, arrays as `Array`.
fn reflection_const_value_repr(val: &Value) -> String {
    match val {
        Value::Null => String::new(),
        Value::True => "1".to_string(),
        Value::False => String::new(),
        _ => reflection_value_repr(val),
    }
}

/// Return the displayed type name for a class constant. If a type is declared,
/// use it. Otherwise, derive it from the value (PHP 8.4+ behaviour).
fn reflection_const_type_name(declared: Option<&crate::opcode::ParamType>, val: &Value) -> Option<String> {
    if let Some(pt) = declared {
        return Some(param_type_name(pt));
    }
    match val {
        Value::Null => Some("null".to_string()),
        Value::True | Value::False => Some("bool".to_string()),
        Value::Long(_) => Some("int".to_string()),
        Value::Double(_) => Some("float".to_string()),
        Value::String(_) => Some("string".to_string()),
        Value::Array(_) => Some("array".to_string()),
        Value::Object(obj) => {
            let ob = obj.borrow();
            if ob.has_property(b"__enum_name") {
                Some(String::from_utf8_lossy(&ob.class_name).into_owned())
            } else {
                Some(String::from_utf8_lossy(&ob.class_name).into_owned())
            }
        }
        _ => None,
    }
}

fn reflection_value_repr(val: &Value) -> String {
    match val {
        Value::Null => "NULL".to_string(),
        Value::True => "true".to_string(),
        Value::False => "false".to_string(),
        Value::Long(n) => n.to_string(),
        Value::Double(f) => {
            if f.fract() == 0.0 && f.is_finite() {
                format!("{:.1}", f)
            } else {
                format!("{}", f)
            }
        }
        Value::String(s) => s.to_string_lossy(),
        Value::Array(_) => "Array".to_string(),
        Value::Object(obj) => {
            let ob = obj.borrow();
            if ob.has_property(b"__enum_name") {
                let enum_class = String::from_utf8_lossy(&ob.class_name).to_string();
                let case_name = ob.get_property(b"__enum_name").to_php_string().to_string_lossy();
                format!("\\{}::{}", enum_class, case_name)
            } else {
                format!("object({})", String::from_utf8_lossy(&ob.class_name))
            }
        }
        _ => String::new(),
    }
}

/// Render a value the way ReflectionProperty::__toString does -- strings
/// are single-quoted, undef/null both render as `NULL`.
fn reflection_repr_for_property_default(val: &Value) -> String {
    match val {
        Value::Undef => "NULL".to_string(),
        Value::String(s) => format!("'{}'", s.to_string_lossy()),
        other => reflection_value_repr(other),
    }
}

/// Create a PHP array of ReflectionAttribute objects from a slice of RuntimeAttributes.
/// Optionally filter by attribute name and flags.
pub fn create_reflection_attributes(
    vm: &mut Vm,
    attrs: &[RuntimeAttribute],
    filter_name: Option<&[u8]>,
    filter_flags: i64,
    target: i64,
) -> Value {
    let mut result = PhpArray::new();
    // Validate flags: only 0 (default) and 2 (IS_INSTANCEOF) are allowed.
    if filter_flags != 0 && filter_flags != 2 {
        let line = vm.current_line;
        let exc = vm.create_exception(
            b"ValueError",
            "ReflectionFunctionAbstract::getAttributes(): Argument #2 ($flags) must be a valid attribute filter flag",
            line,
        );
        vm.current_exception = Some(exc);
        return Value::Null;
    }
    // Resolve filter_name; if given and IS_INSTANCEOF specified, the class
    // must exist or throw an error.
    if filter_flags == 2 {
        if let Some(fname) = filter_name {
            let fl: Vec<u8> = fname.iter().map(|b| b.to_ascii_lowercase()).collect();
            let exists = vm.classes.contains_key(&fl) ||
                // Built-in well-known classes
                matches!(fl.as_slice(),
                    b"stdclass" | b"exception" | b"error" | b"throwable" | b"closure" |
                    b"generator" | b"weakreference" | b"weakmap" |
                    b"arrayaccess" | b"iterator" | b"iteratoraggregate" |
                    b"traversable" | b"countable" | b"stringable" |
                    b"reflectionclass" | b"reflectionobject" | b"reflectionmethod" |
                    b"reflectionfunction" | b"reflectionproperty" | b"reflectionparameter" |
                    b"reflectionconstant" | b"reflectionclassconstant" | b"reflectionattribute" |
                    b"reflectionextension" | b"reflectionnamedtype" | b"reflectionuniontype" |
                    b"reflectionintersectiontype"
                );
            if !exists {
                let display = String::from_utf8_lossy(fname);
                let line = vm.current_line;
                let exc = vm.create_exception(
                    b"Error",
                    &format!("Class \"{}\" not found", display),
                    line,
                );
                vm.current_exception = Some(exc);
                return Value::Null;
            }
        }
    }
    for attr in attrs {
        // If a filter name is specified, check it
        if let Some(fname) = filter_name {
            if filter_flags == 0 {
                // Exact match (default)
                if !attr.name.eq_ignore_ascii_case(fname) {
                    continue;
                }
            } else {
                // IS_INSTANCEOF = 2: check if attribute class is the given class
                // or any subclass/implementer (transitive).
                if !attr.name.eq_ignore_ascii_case(fname) {
                    let attr_lower: Vec<u8> = attr.name.iter().map(|b| b.to_ascii_lowercase()).collect();
                    let filter_lower: Vec<u8> = fname.iter().map(|b| b.to_ascii_lowercase()).collect();
                    // Walk attr class + ancestors checking class + interfaces (transitively).
                    let mut visited: std::collections::HashSet<Vec<u8>> = std::collections::HashSet::new();
                    let mut queue: Vec<Vec<u8>> = vec![attr_lower];
                    let mut is_subclass = false;
                    while let Some(cur) = queue.pop() {
                        if !visited.insert(cur.clone()) { continue; }
                        if cur == filter_lower { is_subclass = true; break; }
                        if let Some(ce) = vm.classes.get(&cur) {
                            if let Some(p) = &ce.parent {
                                let pl: Vec<u8> = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                                queue.push(pl);
                            }
                            for iface in &ce.interfaces {
                                let il: Vec<u8> = iface.iter().map(|b| b.to_ascii_lowercase()).collect();
                                queue.push(il);
                            }
                        }
                    }
                    if !is_subclass {
                        continue;
                    }
                }
            }
        }
        let obj_id = vm.next_object_id();
        let mut obj = PhpObject::new(b"ReflectionAttribute".to_vec(), obj_id);
        obj.set_property(b"__attr_name".to_vec(), Value::String(PhpString::from_vec(attr.name.clone())));
        // Store the op_array for lazy argument evaluation
        obj.set_property(b"__attr_args_evaluated".to_vec(), Value::False);
        obj.set_property(b"__attr_target".to_vec(), Value::Long(target));
        // Store args_op_array reference as a serialized form
        // We'll evaluate it lazily when getArguments() is called
        // For now, eagerly evaluate it
        // Push class scope if the attribute was defined in a class context
        let pushed_scope = if let Some(ref scope) = attr.args_op_array.scope_class {
            vm.push_class_scope(scope.clone());
            vm.called_class_stack.push(scope.clone());
            true
        } else {
            false
        };
        let args_val = match vm.execute_op_array_pub(&attr.args_op_array, vec![]) {
            Ok(v) => v,
            Err(_) => Value::Array(Rc::new(RefCell::new(PhpArray::new()))),
        };
        if pushed_scope {
            vm.pop_class_scope();
            vm.called_class_stack.pop();
        }
        obj.set_property(b"__attr_args".to_vec(), args_val);
        // Check if the attribute is repeated (count how many times this name appears)
        let repeat_count = attrs.iter().filter(|a| a.name.eq_ignore_ascii_case(&attr.name)).count();
        obj.set_property(b"__attr_repeated".to_vec(), if repeat_count > 1 { Value::True } else { Value::False });
        result.push(Value::Object(Rc::new(RefCell::new(obj))));
    }
    Value::Array(Rc::new(RefCell::new(result)))
}

/// Handle method calls on ReflectionAttribute objects
pub fn reflection_attribute_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    match method {
        b"__construct" => {
            drop(ob);
            let exc = vm.create_exception(
                b"ReflectionException",
                "Cannot directly instantiate ReflectionAttribute",
                vm.current_line,
            );
            vm.current_exception = Some(exc);
            Some(Value::Null)
        }
        b"getname" => {
            let name = ob.get_property(b"__attr_name");
            Some(name)
        }
        b"getarguments" => {
            let args = ob.get_property(b"__attr_args");
            // Return the arguments array directly
            match &args {
                Value::Array(arr) => {
                    let borrowed = arr.borrow();
                    let mut result = PhpArray::new();
                    for (key, val) in borrowed.iter() {
                        result.set(key.clone(), val.clone());
                    }
                    Some(Value::Array(Rc::new(RefCell::new(result))))
                }
                _ => Some(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
            }
        }
        b"gettarget" => {
            let target = ob.get_property(b"__attr_target");
            Some(target)
        }
        b"isrepeated" => {
            let repeated = ob.get_property(b"__attr_repeated");
            Some(repeated)
        }
        b"newinstance" => {
            let attr_name = ob.get_property(b"__attr_name").to_php_string().to_string_lossy();
            let args = ob.get_property(b"__attr_args");
            drop(ob);
            // Capture the call-site line before any inner op-array execution
            // (e.g. evaluating #[Attribute(...)] args) overwrites self.current_line.
            let call_site_line = vm.current_line;
            // Create a new instance of the attribute class
            let class_lower: Vec<u8> = attr_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
            // Built-in attribute classes: AllowDynamicProperties, Override,
            // SensitiveParameter, ReturnTypeWillChange, DelayedTargetValidation
            // are marker attributes that have no constructor body.
            // Their newInstance() returns an empty object of the right type.
            let builtin_marker_attrs: &[&[u8]] = &[
                b"allowdynamicproperties",
                b"override",
                b"sensitiveparameter",
                b"returntypewillchange",
                b"delayedtargetvalidation",
            ];
            if builtin_marker_attrs.contains(&class_lower.as_slice())
                && !vm.classes.contains_key(&class_lower)
            {
                let obj_id = vm.next_object_id();
                // Preserve canonical PHP class name casing
                let canonical_name: &[u8] = match class_lower.as_slice() {
                    b"allowdynamicproperties" => b"AllowDynamicProperties",
                    b"override" => b"Override",
                    b"sensitiveparameter" => b"SensitiveParameter",
                    b"returntypewillchange" => b"ReturnTypeWillChange",
                    b"delayedtargetvalidation" => b"DelayedTargetValidation",
                    _ => unreachable!(),
                };
                let new_obj = PhpObject::new(canonical_name.to_vec(), obj_id);
                return Some(Value::Object(Rc::new(RefCell::new(new_obj))));
            }
            if !vm.classes.contains_key(&class_lower) {
                let err_msg = format!("Attribute class \"{}\" not found", attr_name);
                let exc = vm.create_exception(b"Error", &err_msg, call_site_line);
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
            // Check if the class has #[Attribute] -- only attribute classes can be instantiated via newInstance
            let attribute_args_op = vm.classes.get(&class_lower)
                .and_then(|ce| ce.attributes.iter().find(|a| a.name.eq_ignore_ascii_case(b"attribute")))
                .map(|a| a.args_op_array.clone());
            let is_attribute_class = attribute_args_op.is_some();
            if !is_attribute_class && class_lower != b"attribute" {
                let err_msg = format!("Attempting to use non-attribute class \"{}\" as attribute", attr_name);
                let exc = vm.create_exception(b"Error", &err_msg, call_site_line);
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
            // Validate the #[Attribute(...)] flags on this attribute class.
            let mut attr_flags: i64 = 127; // Default TARGET_ALL
            if let Some(args_op) = attribute_args_op {
                let args_val = vm.execute_op_array_pub(&args_op, vec![]).unwrap_or(Value::Null);
                // Restore the call-site line after the inner op-array updated it.
                vm.current_line = call_site_line;
                if let Value::Array(arr) = &args_val {
                    let arr_b = arr.borrow();
                    if let Some((_, flag_val)) = arr_b.iter().next() {
                        // Type-check first: must be int
                        if !matches!(flag_val, Value::Long(_) | Value::Null) {
                            let actual_type = match flag_val {
                                Value::String(_) => "string",
                                Value::Double(_) => "float",
                                Value::True | Value::False => "bool",
                                Value::Array(_) => "array",
                                Value::Object(_) => "object",
                                _ => "unknown",
                            };
                            let err_msg = format!(
                                "Attribute::__construct(): Argument #1 ($flags) must be of type int, {} given",
                                actual_type
                            );
                            let exc = vm.create_exception(b"TypeError", &err_msg, call_site_line);
                            vm.current_exception = Some(exc);
                            return Some(Value::Null);
                        }
                        // Value range check: must be 0..=255 (TARGET_ALL=127 | IS_REPEATABLE=128)
                        if let Value::Long(n) = flag_val {
                            if *n < 0 || *n > (127 | 128) {
                                let err_msg = "Invalid attribute flags specified".to_string();
                                let exc = vm.create_exception(b"ValueError", &err_msg, call_site_line);
                                vm.current_exception = Some(exc);
                                return Some(Value::Null);
                            }
                            attr_flags = *n;
                        }
                    }
                }
            }
            // Check target compatibility. The attribute object stores
            // `__attr_target` as the active target bit (e.g. TARGET_CLASS=1).
            let active_target = {
                let ob2 = obj.borrow();
                ob2.get_property(b"__attr_target").to_long()
            };
            let target_mask = attr_flags & 127;
            if active_target != 0 && (target_mask & active_target) == 0 {
                let target_str = match active_target {
                    1 => "class",
                    2 => "function",
                    4 => "method",
                    8 => "property",
                    16 => "class constant",
                    32 => "parameter",
                    64 => "constant",
                    _ => "unknown",
                };
                let mut allowed: Vec<&str> = Vec::new();
                if target_mask & 1 != 0 { allowed.push("class"); }
                if target_mask & 2 != 0 { allowed.push("function"); }
                if target_mask & 4 != 0 { allowed.push("method"); }
                if target_mask & 8 != 0 { allowed.push("property"); }
                if target_mask & 16 != 0 { allowed.push("class constant"); }
                if target_mask & 32 != 0 { allowed.push("parameter"); }
                if target_mask & 64 != 0 { allowed.push("constant"); }
                let err_msg = format!(
                    "Attribute \"{}\" cannot target {} (allowed targets: {})",
                    attr_name, target_str, allowed.join(", ")
                );
                let exc = vm.create_exception(b"Error", &err_msg, call_site_line);
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
            // Check repetition: if the attribute is repeated but not flagged
            // as IS_REPEATABLE, throw an error.
            let is_repeated = {
                let ob2 = obj.borrow();
                matches!(ob2.get_property(b"__attr_repeated"), Value::True)
            };
            if is_repeated && (attr_flags & 128) == 0 {
                let err_msg = format!("Attribute \"{}\" must not be repeated", attr_name);
                let exc = vm.create_exception(b"Error", &err_msg, call_site_line);
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
            // Build argument list from args array
            let mut call_args = Vec::new();
            let mut named_args = Vec::new();
            if let Value::Array(arr) = &args {
                for (key, val) in arr.borrow().iter() {
                    match key {
                        ArrayKey::String(name) => {
                            named_args.push((name.as_bytes().to_vec(), val.clone()));
                        }
                        ArrayKey::Int(_) => {
                            call_args.push(val.clone());
                        }
                    }
                }
            }
            // Create the object and call its constructor
            let obj_id = vm.next_object_id();
            let ce = vm.classes.get(&class_lower).unwrap().clone();
            // Pre-flight: validate constructor visibility and argument count.
            let construct_lower = b"__construct".to_vec();
            if let Some(constructor) = ce.get_method(&construct_lower) {
                use crate::object::Visibility;
                if matches!(constructor.visibility, Visibility::Private) {
                    let err_msg = format!(
                        "Call to private {}::__construct() from global scope",
                        std::str::from_utf8(&ce.name).unwrap_or("")
                    );
                    let exc = vm.create_exception(b"Error", &err_msg, 0);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                if matches!(constructor.visibility, Visibility::Protected) {
                    let err_msg = format!(
                        "Call to protected {}::__construct() from global scope",
                        std::str::from_utf8(&ce.name).unwrap_or("")
                    );
                    let exc = vm.create_exception(b"Error", &err_msg, 0);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                // Build effective argument count after merging positional and
                // named arguments against parameter names.
                let op = &constructor.op_array;
                let required = op.required_param_count as usize;
                let positional = call_args.len();
                // Count distinct CV indices satisfied via positional + named.
                let mut satisfied = vec![false; op.cv_names.len()];
                // Param CVs follow $this, so start at offset 1 for non-static.
                let skip = 1usize;
                for i in 0..positional {
                    if i + skip < satisfied.len() {
                        satisfied[i + skip] = true;
                    }
                }
                for (name, _) in &named_args {
                    for (ci, cv_name) in op.cv_names.iter().enumerate() {
                        if cv_name == name {
                            if ci < satisfied.len() { satisfied[ci] = true; }
                            break;
                        }
                    }
                }
                let mut filled_required = 0usize;
                for i in 0..required {
                    if i + skip < satisfied.len() && satisfied[i + skip] {
                        filled_required += 1;
                    }
                }
                if filled_required < required {
                    let err_msg = format!(
                        "Too few arguments to function {}::__construct(), {} passed and {} {} expected",
                        std::str::from_utf8(&ce.name).unwrap_or(""),
                        positional + named_args.len(),
                        required,
                        if (op.param_count as usize) > required { "at least" } else { "exactly" }
                    );
                    let exc = vm.create_exception(b"ArgumentCountError", &err_msg, 0);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
            }
            let mut new_obj = PhpObject::new(ce.name.clone(), obj_id);
            // Initialize properties from class definition
            for prop in &ce.properties {
                if !prop.is_static && !matches!(prop.default, Value::Undef) {
                    new_obj.set_property(prop.name.clone(), prop.default.clone());
                }
            }
            let instance = Value::Object(Rc::new(RefCell::new(new_obj)));
            // Call __construct if it exists
            if let Some(constructor) = ce.get_method(&construct_lower) {
                let op = constructor.op_array.clone();
                let mut cvs = vec![Value::Undef; op.cv_names.len()];
                if !constructor.is_static && !cvs.is_empty() {
                    cvs[0] = instance.clone();
                }
                let skip = if !constructor.is_static { 1 } else { 0 };
                for (i, arg) in call_args.iter().enumerate() {
                    if i + skip < cvs.len() {
                        cvs[i + skip] = arg.clone();
                    }
                }
                // Resolve named args
                for (name, val) in &named_args {
                    for (ci, cv_name) in op.cv_names.iter().enumerate() {
                        if cv_name == name {
                            if ci < cvs.len() {
                                cvs[ci] = val.clone();
                            }
                            break;
                        }
                    }
                }
                let _ = vm.execute_op_array_pub(&op, cvs);
            }
            Some(instance)
        }
        b"__tostring" => {
            let name = ob.get_property(b"__attr_name").to_php_string().to_string_lossy();
            Some(Value::String(PhpString::from_string(format!("Attribute [ {} ]", name))))
        }
        _ => None,
    }
}

/// Determine which extension a built-in function belongs to
fn get_function_extension(func_name: &str) -> Option<&'static str> {
    // Standard library functions
    let standard = [
        "array_shift", "array_unshift", "array_pop", "array_push", "array_splice",
        "array_slice", "array_merge", "array_combine", "array_chunk", "array_unique",
        "array_flip", "array_reverse", "array_keys", "array_values", "array_search",
        "array_map", "array_filter", "array_reduce", "array_walk", "array_walk_recursive",
        "array_column", "array_fill", "array_fill_keys", "array_pad", "array_rand",
        "array_sum", "array_product", "array_count_values", "array_diff", "array_diff_key",
        "array_diff_assoc", "array_intersect", "array_intersect_key", "array_intersect_assoc",
        "array_key_exists", "array_key_first", "array_key_last", "array_multisort",
        "array_replace", "array_replace_recursive", "in_array", "compact", "extract",
        "sort", "rsort", "asort", "arsort", "ksort", "krsort", "usort", "uasort", "uksort",
        "natsort", "natcasesort", "count", "sizeof", "range", "shuffle", "list",
        "strlen", "strpos", "strrpos", "strstr", "stristr", "substr", "substr_count",
        "substr_replace", "str_replace", "str_ireplace", "str_repeat", "str_pad",
        "str_split", "str_word_count", "str_contains", "str_starts_with", "str_ends_with",
        "strtolower", "strtoupper", "ucfirst", "lcfirst", "ucwords",
        "trim", "ltrim", "rtrim", "nl2br", "wordwrap", "number_format",
        "sprintf", "printf", "fprintf", "sscanf", "vsprintf",
        "implode", "join", "explode", "chunk_split",
        "ord", "chr", "hex2bin", "bin2hex", "pack", "unpack",
        "md5", "sha1", "crc32", "base64_encode", "base64_decode",
        "htmlspecialchars", "htmlspecialchars_decode", "htmlentities", "html_entity_decode",
        "urlencode", "urldecode", "rawurlencode", "rawurldecode", "http_build_query",
        "parse_str", "parse_url",
        "intval", "floatval", "strval", "boolval", "settype", "gettype",
        "is_null", "is_int", "is_integer", "is_long", "is_float", "is_double",
        "is_string", "is_bool", "is_array", "is_object", "is_numeric", "is_callable",
        "is_resource", "is_finite", "is_infinite", "is_nan", "is_countable",
        "isset", "unset", "empty", "var_dump", "var_export", "print_r", "debug_zval_refs",
        "serialize", "unserialize",
        "abs", "ceil", "floor", "round", "max", "min", "pow", "sqrt", "log", "log2", "log10",
        "exp", "fmod", "intdiv", "fdiv",
        "rand", "mt_rand", "random_int", "random_bytes", "srand", "mt_srand",
        "sin", "cos", "tan", "asin", "acos", "atan", "atan2",
        "pi", "deg2rad", "rad2deg", "base_convert", "bindec", "octdec", "hexdec", "decoct", "dechex",
        "time", "microtime", "sleep", "usleep", "time_sleep_until",
        "fopen", "fclose", "fread", "fwrite", "fgets", "fgetc", "feof", "fseek", "ftell",
        "rewind", "fflush", "flock", "fstat", "ftruncate", "fgetcsv", "fputcsv",
        "file_get_contents", "file_put_contents", "file_exists", "file",
        "readfile", "copy", "rename", "unlink", "mkdir", "rmdir",
        "is_file", "is_dir", "is_link", "is_readable", "is_writable", "is_executable",
        "stat", "lstat", "realpath", "basename", "dirname", "pathinfo",
        "glob", "tempnam", "sys_get_temp_dir", "tmpfile",
        "chmod", "chown", "chgrp", "touch", "clearstatcache",
        "readlink", "symlink", "link",
        "opendir", "closedir", "readdir", "scandir",
        "preg_match", "preg_match_all", "preg_replace", "preg_replace_callback",
        "preg_split", "preg_quote", "preg_last_error",
        "class_exists", "interface_exists", "trait_exists",
        "function_exists", "method_exists", "property_exists",
        "get_class", "get_parent_class", "get_called_class",
        "get_object_vars", "get_class_vars", "get_class_methods",
        "defined", "define", "constant",
        "trigger_error", "user_error", "set_error_handler", "restore_error_handler",
        "set_exception_handler", "restore_exception_handler",
        "debug_backtrace", "debug_print_backtrace",
        "header", "headers_sent", "headers_list", "setcookie",
        "ini_get", "ini_set", "ini_restore", "get_cfg_var",
        "phpversion", "phpinfo", "php_uname", "php_sapi_name",
        "get_defined_vars", "get_defined_functions", "get_defined_constants",
        "getenv", "putenv",
        "call_user_func", "call_user_func_array",
        "array_is_list",
    ];
    if standard.contains(&func_name) {
        return Some("standard");
    }

    // Date functions
    let date = [
        "date", "time", "mktime", "gmmktime", "strtotime", "getdate", "localtime",
        "checkdate", "date_create", "date_create_immutable", "date_create_from_format",
        "date_format", "date_modify", "date_add", "date_sub", "date_diff",
        "date_timezone_get", "date_timezone_set", "date_offset_get",
        "date_time_set", "date_date_set", "date_isodate_set",
        "date_timestamp_get", "date_timestamp_set",
        "date_default_timezone_set", "date_default_timezone_get",
        "date_parse", "date_parse_from_format",
        "timezone_open", "timezone_name_get", "timezone_offset_get",
        "timezone_identifiers_list", "timezone_abbreviations_list",
        "strftime", "gmstrftime", "idate",
    ];
    if date.contains(&func_name) {
        return Some("date");
    }

    // JSON functions
    if func_name.starts_with("json_") {
        return Some("json");
    }

    // Ctype functions
    if func_name.starts_with("ctype_") {
        return Some("ctype");
    }

    // mbstring functions
    if func_name.starts_with("mb_") {
        return Some("mbstring");
    }

    None
}
