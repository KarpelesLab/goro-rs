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


/// ReflectionClass constructor: sets up __reflection_target and name properties
pub fn reflection_class_construct(vm: &mut Vm, args: &[Value], line: u32) -> bool {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return true,
    };
    let arg = args.get(1).cloned().unwrap_or(Value::Null);

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
    let class_exists = vm.classes.contains_key(&class_lower)
        || vm.is_known_builtin_class(&class_lower);

    if !class_exists {
        let err_msg = format!("Class \"{}\" does not exist", class_name);
        let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
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
    }

    // Handle array callables like [ClassName, method] - these should throw for ReflectionFunction
    if let Value::Array(_) = &arg {
        let err_msg = "Function Array() does not exist".to_string();
        let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
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

    let class_arg = args.get(1).cloned().unwrap_or(Value::Null);
    let prop_arg = args.get(2).cloned().unwrap_or(Value::Null);

    let class_name = match &class_arg {
        Value::Object(obj) => {
            let ob = obj.borrow();
            String::from_utf8_lossy(&ob.class_name).to_string()
        }
        _ => class_arg.to_php_string().to_string_lossy(),
    };
    let prop_name = prop_arg.to_php_string().to_string_lossy();

    let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

    let is_user_class = vm.classes.contains_key(&class_lower);
    let is_builtin = vm.is_known_builtin_class(&class_lower);

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

    // For user classes, check property exists; for built-in classes, accept any property name
    if is_user_class {
        let prop_exists = vm.classes.get(&class_lower)
            .map(|c| c.properties.iter().any(|p| p.name == prop_name.as_bytes()))
            .unwrap_or(false);

        if !prop_exists {
            let err_msg = format!("Property {}::${} does not exist", canonical_class, prop_name);
            let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
            vm.current_exception = Some(exc);
            return false;
        }
    }

    let mut obj = this.borrow_mut();
    obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(prop_name.clone())));
    obj.set_property(b"class".to_vec(), Value::String(PhpString::from_string(canonical_class.clone())));
    obj.set_property(b"__reflection_class".to_vec(), Value::String(PhpString::from_string(canonical_class)));
    obj.set_property(b"__reflection_prop".to_vec(), Value::String(PhpString::from_string(prop_name)));
    true
}

/// ReflectionParameter constructor
pub fn reflection_parameter_construct(vm: &mut Vm, args: &[Value], _line: u32) -> bool {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return true,
    };
    let func_arg = args.get(1).cloned().unwrap_or(Value::Null);
    let param_arg = args.get(2).cloned().unwrap_or(Value::Null);

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
                let method_part = items[1].to_php_string().to_string_lossy();
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

    // Parameter may be specified by index or by name.
    let param_idx_opt: Option<usize> = match &param_arg {
        Value::String(s) => {
            let name = s.as_bytes();
            vm.user_functions.get(&func_lower)
                .and_then(|op| op.cv_names.iter().position(|n| n == name))
        }
        Value::Long(i) => Some(*i as usize),
        _ => Some(param_arg.to_long() as usize),
    };

    // Look up the function
    if let Some(op_array) = vm.user_functions.get(&func_lower).cloned() {
        let param_idx = param_idx_opt.unwrap_or(0);
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

    let class_arg = args.get(1).cloned().unwrap_or(Value::Null);
    let const_arg = args.get(2).cloned().unwrap_or(Value::Null);

    let class_name = match &class_arg {
        Value::Object(obj) => {
            let ob = obj.borrow();
            String::from_utf8_lossy(&ob.class_name).to_string()
        }
        _ => class_arg.to_php_string().to_string_lossy(),
    };
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

    // Check constant exists
    let const_val = reflection_class_get_constant(vm, &class_lower, const_name.as_bytes());
    if const_val.is_none() {
        let err_msg = format!("Constant {}::{} does not exist", canonical_class, const_name);
        let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }

    let mut obj = this.borrow_mut();
    obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(const_name.clone())));
    obj.set_property(b"class".to_vec(), Value::String(PhpString::from_string(canonical_class)));
    obj.set_property(b"__reflection_value".to_vec(), const_val.unwrap());
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
                let instantiable = !ce.is_abstract && !ce.is_interface && !ce.is_trait && !ce.is_enum;
                Some(if instantiable { Value::True } else { Value::False })
            } else {
                // Built-in classes are generally instantiable
                Some(Value::True)
            }
        }
        b"iscloneable" => {
            Some(Value::True)
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
            // Check if class implements Iterator or IteratorAggregate
            let is_iterable = vm.class_implements_interface(&class_lower, b"iterator")
                || vm.class_implements_interface(&class_lower, b"iteratoraggregate")
                || vm.builtin_implements_interface(&class_lower, b"iterator")
                || vm.builtin_implements_interface(&class_lower, b"iteratoraggregate");
            Some(if is_iterable { Value::True } else { Value::False })
        }
        b"getinterfacenames" => {
            let mut names = PhpArray::new();
            if let Some(ce) = vm.classes.get(&class_lower) {
                for iface in &ce.interfaces {
                    names.push(Value::String(PhpString::from_vec(iface.clone())));
                }
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
            let mut result = PhpArray::new();
            if let Some(ce) = vm.classes.get(&class_lower) {
                for iface in ce.interfaces.clone() {
                    let iface_name = String::from_utf8_lossy(&iface).to_string();
                    let rc = create_reflection_class(vm, &iface_name);
                    result.set(ArrayKey::String(PhpString::from_vec(iface.clone())), rc);
                }
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

            // Static properties pass (child first, inherited non-private)
            for (idx, link) in chain.iter().enumerate() {
                if let Some(lce) = vm.classes.get(link) {
                    for prop in &lce.properties {
                        if !prop.is_static { continue; }
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
            // Include own static props and inherited non-private statics.
            let mut result = PhpArray::new();
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
            for (idx, link) in chain.iter().enumerate() {
                if let Some(lce) = vm.classes.get(link) {
                    for prop in &lce.properties {
                        if !prop.is_static { continue; }
                        if idx > 0 && prop.visibility == Visibility::Private { continue; }
                        let key = ArrayKey::String(PhpString::from_vec(prop.name.clone()));
                        if result.get(&key).is_none() {
                            let val = lce.static_properties.get(&prop.name)
                                .cloned()
                                .unwrap_or_else(|| prop.default.clone());
                            result.set(key, val);
                        }
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
            drop(ob);
            Some(Value::String(PhpString::from_string(
                reflection_class_to_string(vm, &name, &class_lower)
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
                let method_name = args.get(1)?.to_php_string().to_string_lossy();
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
                let prop_name = args.get(1)?.to_php_string();
                let has = vm.classes.get(&class_lower)
                    .map(|c| c.properties.iter().any(|p| p.name == prop_name.as_bytes()))
                    .unwrap_or(false);
                Some(if has { Value::True } else { Value::False })
            }
            b"hasconstant" => {
                let const_name = args.get(1)?.to_php_string();
                let has = reflection_class_has_constant(vm, &class_lower, const_name.as_bytes());
                Some(if has { Value::True } else { Value::False })
            }
            b"getconstant" => {
                let const_name = args.get(1)?.to_php_string();
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
                let method_name = args.get(1)?.to_php_string().to_string_lossy();
                let method_lower: Vec<u8> = method_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                let has = vm.classes.get(&class_lower)
                    .map(|c| c.get_method(&method_lower).is_some())
                    .unwrap_or(false);
                if has {
                    let canonical_class = vm.classes.get(&class_lower)
                        .map(|c| String::from_utf8_lossy(&c.name).to_string())
                        .unwrap_or(target.clone());
                    Some(create_reflection_method(vm, &canonical_class, &method_name))
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
                let prop_name = args.get(1)?.to_php_string().to_string_lossy();
                let has_declared = vm.classes.get(&class_lower)
                    .map(|c| c.properties.iter().any(|p| p.name == prop_name.as_bytes()))
                    .unwrap_or(false);
                // For ReflectionObject, check the actual object for dynamic
                // properties. `__reflection_object` stores the associated
                // object instance when the Reflection target came from one.
                let has_dynamic = if !has_declared {
                    let ob = obj.borrow();
                    let self_obj = ob.get_property(b"__reflection_object");
                    drop(ob);
                    if let Value::Object(inst) = &self_obj {
                        let io = inst.borrow();
                        io.has_property(prop_name.as_bytes())
                    } else {
                        false
                    }
                } else {
                    false
                };
                if has_declared || has_dynamic {
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
                let declared: Vec<String> = if let Some(ce) = vm.classes.get(&class_lower) {
                    let mut v = Vec::new();
                    for prop in ce.properties.iter() {
                        if filter != -1 {
                            let prop_mod = reflection_property_modifiers_static(prop);
                            if prop_mod & filter == 0 { continue; }
                        }
                        if !prop.is_static {
                            let name_str = String::from_utf8_lossy(&prop.name).to_string();
                            v.push(name_str);
                            declared_names.push(prop.name.clone());
                        }
                    }
                    v
                } else { Vec::new() };
                for name_str in declared {
                    result.push(create_reflection_property(vm, &target, &name_str));
                }
                // For ReflectionObject, append dynamic instance properties
                // (these are considered IS_PUBLIC). Collect names first.
                let include_dynamic = filter == -1 || (filter & 1) != 0;
                if include_dynamic {
                    let dyn_names: Vec<Vec<u8>> = {
                        let ob = obj.borrow();
                        let self_obj = ob.get_property(b"__reflection_object");
                        drop(ob);
                        if let Value::Object(inst) = &self_obj {
                            let io = inst.borrow();
                            io.properties.iter()
                                .filter(|(name, _)| !name.starts_with(b"__"))
                                .filter(|(name, _)| !declared_names.iter().any(|n| n == name))
                                .map(|(n, _)| n.clone())
                                .collect()
                        } else {
                            Vec::new()
                        }
                    };
                    for dn in dyn_names {
                        let name_str = String::from_utf8_lossy(&dn).to_string();
                        result.push(create_reflection_property(vm, &target, &name_str));
                    }
                }
                Some(Value::Array(Rc::new(RefCell::new(result))))
            }
            b"issubclassof" => {
                let parent_arg = args.get(1)?;
                let parent_name = match parent_arg {
                    Value::Object(o) => {
                        let ob = o.borrow();
                        // If it's a ReflectionClass, use its name
                        if ob.class_name.eq_ignore_ascii_case(b"ReflectionClass") {
                            ob.get_property(b"name").to_php_string().to_string_lossy()
                        } else {
                            String::from_utf8_lossy(&ob.class_name).to_string()
                        }
                    }
                    _ => parent_arg.to_php_string().to_string_lossy(),
                };
                let parent_lower: Vec<u8> = parent_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
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
                let iface_arg = args.get(1)?;
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

                let result = vm.class_implements_interface(&class_lower, &iface_lower)
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
                // Create instance and call constructor with remaining args
                let obj_id = vm.next_object_id();
                let canonical = vm.classes.get(&class_lower)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| vm.builtin_canonical_name(&class_lower).into_bytes());

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
                let new_val = Value::Object(Rc::new(RefCell::new(new_obj)));

                // Call constructor if it exists
                if has_constructor {
                    let ctor = vm.classes.get(&class_lower)
                        .and_then(|c| c.get_method(b"__construct"))
                        .cloned();
                    if let Some(ctor_method) = ctor {
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
                let args_arr = args.get(1).cloned().unwrap_or(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                let ctor_args: Vec<Value> = if let Value::Array(arr) = &args_arr {
                    arr.borrow().iter().map(|(_, v)| v.clone()).collect()
                } else {
                    vec![]
                };

                let obj_id = vm.next_object_id();
                let canonical = vm.classes.get(&class_lower)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| vm.builtin_canonical_name(&class_lower).into_bytes());

                let has_constructor = vm.classes.get(&class_lower)
                    .map(|c| c.get_method(b"__construct").is_some())
                    .unwrap_or(false);

                let mut new_obj = PhpObject::new(canonical, obj_id);
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
                let is_proxy = method == b"resetaslazyproxy";
                if let Value::Object(obj) = &target {
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
                let _options = args.get(2).map(|v| v.to_long()).unwrap_or(0);
                let is_proxy = method == b"newlazyproxy";
                let obj_id = vm.next_object_id();
                let canonical = vm.classes.get(&class_lower)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| vm.builtin_canonical_name(&class_lower).into_bytes());
                let mut new_obj = PhpObject::new(canonical, obj_id);
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
                }
                Some(target)
            }
            b"marklazyobjectasinitialized" => {
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                if let Value::Object(inner) = &target {
                    let mut ob = inner.borrow_mut();
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
                let prop_name = args.get(1)?.to_php_string();
                let default = args.get(2);
                if let Some(ce) = vm.classes.get(&class_lower) {
                    if let Some(val) = ce.static_properties.get(prop_name.as_bytes()) {
                        Some(val.clone())
                    } else if let Some(d) = default {
                        Some(d.clone())
                    } else {
                        Some(Value::Null)
                    }
                } else if let Some(d) = default {
                    Some(d.clone())
                } else {
                    Some(Value::Null)
                }
            }
            b"setstaticpropertyvalue" => {
                let prop_name = args.get(1)?.to_php_string();
                let value = args.get(2).cloned().unwrap_or(Value::Null);
                if let Some(ce) = vm.classes.get_mut(&class_lower) {
                    ce.static_properties.insert(prop_name.as_bytes().to_vec(), value);
                }
                Some(Value::Null)
            }
            b"getreflectionconstant" | b"getreflectionconstants" => {
                // Return ReflectionClassConstant objects
                let mut result = PhpArray::new();
                if method == b"getreflectionconstant" {
                    let const_name = args.get(1)?.to_php_string();
                    let val = reflection_class_get_constant(vm, &class_lower, const_name.as_bytes());
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
            let count = method_def.map(|m| {
                let skip = if !m.is_static { 1u32 } else { 0u32 };
                m.op_array.required_param_count.saturating_sub(skip)
            }).unwrap_or(0);
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
            if let Some(rt) = ret {
                Some(create_reflection_type(vm, &rt))
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
            Some(Value::True)
        }
        b"isinternal" => {
            Some(Value::False)
        }
        b"returnsreference" => {
            Some(Value::False)
        }
        b"getfilename" => {
            // Use the class's filename if available
            let filename = vm.classes.get(&class_lower)
                .and_then(|c| c.filename.clone())
                .unwrap_or_else(|| vm.current_file.clone());
            Some(Value::String(PhpString::from_string(filename)))
        }
        b"getstartline" => {
            let line = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| m.op_array.decl_line))
                .unwrap_or(0);
            Some(Value::Long(line as i64))
        }
        b"getendline" => {
            Some(Value::False)
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
            Some(Value::False)
        }
        b"getstaticvariables" => {
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
        }
        b"setaccessible" => {
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
                let modifiers = format!("{}{}{}",
                    if m.is_abstract { "abstract " } else { "" },
                    if m.is_final { "final " } else { "" },
                    vis,
                );
                let mut s = format!("Method [ <user> {} method {} ] {{\n", modifiers, name);
                s.push_str(&format!("  @@ {} {} - {}\n", vm.current_file, m.op_array.decl_line, m.op_array.decl_line));
                s.push_str(&format!("\n  - Parameters [{}] {{\n", m.op_array.param_count));
                for i in 0..m.op_array.param_count as usize {
                    if i < m.op_array.cv_names.len() {
                        let pname = String::from_utf8_lossy(&m.op_array.cv_names[i]);
                        let required = i < m.op_array.required_param_count as usize;
                        s.push_str(&format!("    Parameter #{} [ <{}> ${} ]\n", i,
                            if required { "required" } else { "optional" }, pname));
                    }
                }
                s.push_str("  }\n}\n");
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
                let target_obj = args.get(1)?.clone();
                let invoke_args: Vec<Value> = args.iter().skip(2).cloned().collect();

                let method_def = vm.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower_bytes.as_bytes()))
                    .cloned();

                if let Some(m) = method_def {
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
                let args_arr = args.get(2).cloned().unwrap_or(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                let invoke_args: Vec<Value> = if let Value::Array(arr) = &args_arr {
                    arr.borrow().iter().map(|(_, v)| v.clone()).collect()
                } else {
                    vec![]
                };

                let method_def = vm.classes.get(&class_lower)
                    .and_then(|c| c.get_method(method_lower_bytes.as_bytes()))
                    .cloned();

                if let Some(m) = method_def {
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
            if vm.user_functions.contains_key(func_lower.as_slice()) {
                Some(Value::String(PhpString::from_string(vm.current_file.clone())))
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
            // We don't track end line in op_array, return false
            Some(Value::False)
        }
        b"getdoccomment" => {
            Some(Value::False)
        }
        b"getstaticvariables" => {
            // Return static variables of the function
            let mut result = PhpArray::new();
            // Look up static vars with the function name prefix
            let prefix = format!("{}::", String::from_utf8_lossy(&func_lower));
            for (key, val) in vm.static_vars() {
                let key_str = String::from_utf8_lossy(key);
                if key_str.starts_with(&prefix) {
                    let var_name = &key_str[prefix.len()..];
                    result.set(ArrayKey::String(PhpString::from_string(var_name.to_string())), val.clone());
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
            Some(Value::False)
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
            Some(Value::False)
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
            Some(if is_closure { Value::True } else { Value::False })
        }
        b"isstatic" => {
            // Only closures explicitly declared `static function()` are
            // static; we don't distinguish them, so return false.
            Some(Value::False)
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
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
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
                let mut s = format!("Function [ <user> function {} ] {{\n", name);
                s.push_str(&format!("  @@ {} {}\n\n", vm.current_file, op_array.decl_line));
                s.push_str(&format!("  - Parameters [{}] {{\n", op_array.param_count));
                for i in 0..op_array.param_count as usize {
                    if i < op_array.cv_names.len() {
                        let pname = String::from_utf8_lossy(&op_array.cv_names[i]);
                        let required = i < op_array.required_param_count as usize;
                        s.push_str(&format!("    Parameter #{} [ <{}> ${} ]\n", i,
                            if required { "required" } else { "optional" }, pname));
                    }
                }
                s.push_str("  }\n}\n");
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
    drop(ob);

    match method {
        b"getname" => {
            let ob = obj.borrow();
            Some(ob.get_property(b"name"))
        }
        b"getdeclaringclass" => {
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
            let vis = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| p.visibility))
                .unwrap_or(Visibility::Public);
            Some(if vis == Visibility::Public { Value::True } else { Value::False })
        }
        b"isprotected" => {
            let vis = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| p.visibility))
                .unwrap_or(Visibility::Public);
            Some(if vis == Visibility::Protected { Value::True } else { Value::False })
        }
        b"isprivate" => {
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
            let is_final = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| p.is_final))
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
            let default = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()).map(|p| p.default.clone()))
                .unwrap_or(Value::Null);
            Some(default)
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
            Some(Value::False)
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
            Some(Value::Null)
        }
        b"getmangledname" => {
            // PHP: private properties -> "\0ClassName\0name",
            // protected properties -> "\0*\0name", public -> "name".
            let (vis, declaring) = vm.classes.get(&class_lower)
                .and_then(|c| c.properties.iter().find(|p| p.name == prop_name.as_bytes()))
                .map(|p| (p.visibility, String::from_utf8_lossy(&p.declaring_class).to_string()))
                .unwrap_or((Visibility::Public, class_name.clone()));
            let mangled = match vis {
                Visibility::Public => prop_name.clone(),
                Visibility::Protected => format!("\0*\0{}", prop_name),
                Visibility::Private => format!("\0{}\0{}", declaring, prop_name),
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
                let static_str = if p.is_static { "static " } else { "" };
                let mut s = format!("Property [ {}{} ${} ]\n", static_str, vis, prop_name);
                if !matches!(p.default, Value::Undef | Value::Null) {
                    s = format!("Property [ {}{} ${} = {} ]\n", static_str, vis, prop_name, reflection_value_repr(&p.default));
                }
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
                    // For static properties, look up from class
                    let val = vm.classes.get(&class_lower)
                        .and_then(|c| c.static_properties.get(prop_name.as_bytes()).cloned())
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
                    // For static properties, set on the class
                    let value = if args.len() >= 3 {
                        args[2].clone()
                    } else if args.len() >= 2 {
                        args[1].clone()
                    } else {
                        Value::Null
                    };
                    if let Some(ce) = vm.classes.get_mut(&class_lower) {
                        ce.static_properties.insert(prop_name.as_bytes().to_vec(), value);
                    }
                } else if args.len() >= 3 {
                    let target = &args[1];
                    let value = args[2].clone();
                    if let Value::Object(target_obj) = target {
                        let mut target_ob = target_obj.borrow_mut();
                        target_ob.set_property(prop_name.as_bytes().to_vec(), value);
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

/// ReflectionParameter no-arg method dispatch
pub fn reflection_parameter_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    let func_target = ob.get_property(b"__reflection_func").to_php_string();
    let param_idx = ob.get_property(b"__reflection_param_idx").to_long() as usize;
    drop(ob);

    match method {
        b"getname" => {
            let ob = obj.borrow();
            Some(ob.get_property(b"name"))
        }
        b"getposition" => {
            Some(Value::Long(param_idx as i64))
        }
        b"isoptional" => {
            if let Some(op_array) = vm.user_functions.get(func_target.as_bytes()) {
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
            if let Some(op_array) = vm.user_functions.get(func_target.as_bytes()) {
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
            // We don't easily have access to default values at runtime
            Some(Value::Null)
        }
        b"allowsnull" => {
            Some(Value::True)
        }
        b"isvariadic" => {
            if let Some(op_array) = vm.user_functions.get(func_target.as_bytes()) {
                if let Some(variadic_idx) = op_array.variadic_param {
                    Some(if param_idx == variadic_idx as usize {
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
            Some(Value::False)
        }
        b"hastype" => {
            if let Some(op_array) = vm.user_functions.get(func_target.as_bytes()) {
                let has = param_idx < op_array.param_types.len()
                    && op_array.param_types[param_idx].is_some();
                Some(if has { Value::True } else { Value::False })
            } else {
                Some(Value::False)
            }
        }
        b"gettype" => {
            let param_type = vm.user_functions.get(func_target.as_bytes())
                .and_then(|op_array| {
                    if param_idx < op_array.param_types.len() {
                        op_array.param_types[param_idx].as_ref().map(|pti| pti.param_type.clone())
                    } else {
                        None
                    }
                });
            if let Some(pt) = param_type {
                Some(create_reflection_type(vm, &pt))
            } else {
                Some(Value::Null)
            }
        }
        b"getdeclaringfunction" => {
            // Return a ReflectionFunction for the declaring function
            let ob = obj.borrow();
            let func_name = ob.get_property(b"name").to_php_string().to_string_lossy();
            drop(ob);
            let obj_id = vm.next_object_id();
            let mut rf_obj = PhpObject::new(b"ReflectionFunction".to_vec(), obj_id);
            rf_obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(func_name.clone())));
            rf_obj.set_property(b"__reflection_target".to_vec(), Value::String(func_target.clone()));
            Some(Value::Object(Rc::new(RefCell::new(rf_obj))))
        }
        b"getdeclaringclass" => {
            // Returns null for functions, ReflectionClass for methods
            Some(Value::Null)
        }
        b"getclass" => {
            // Deprecated: returns the class if the param type hints a class
            Some(Value::Null)
        }
        b"isdefaultvalueconstant" => {
            Some(Value::False)
        }
        b"getdefaultvalueconstantname" => {
            Some(Value::String(PhpString::empty()))
        }
        b"ispromoted" => {
            Some(Value::False)
        }
        b"canbepassedbyvalue" => {
            Some(Value::True)
        }
        b"__tostring" => {
            let ob = obj.borrow();
            let name = ob.get_property(b"name").to_php_string().to_string_lossy();
            drop(ob);
            let is_optional = if let Some(op_array) = vm.user_functions.get(func_target.as_bytes()) {
                param_idx >= op_array.required_param_count as usize
            } else {
                false
            };
            let kind = if is_optional { "optional" } else { "required" };
            Some(Value::String(PhpString::from_string(
                format!("Parameter #{} [ <{}> ${} ]", param_idx, kind, name)
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
            if let Some(pos) = name.rfind('\\') {
                Some(Value::String(PhpString::from_string(name[..pos].to_string())))
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
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
        }
        b"__tostring" => {
            Some(Value::String(PhpString::from_string(format!("Constant [ {} ]\n", name))))
        }
        _ => None,
    }
}

/// ReflectionGenerator method dispatch (no args). The `__reflection_target`
/// property holds the underlying Generator value.
pub fn reflection_generator_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    match method {
        b"getexecutingfile" => {
            Some(Value::String(PhpString::from_string(vm.current_file.clone())))
        }
        b"getexecutingline" => {
            Some(Value::Long(vm.current_line as i64))
        }
        b"getfunction" => {
            // Return a ReflectionFunction for the generator's function.
            let target = {
                let ob = obj.borrow();
                ob.get_property(b"__reflection_target")
            };
            if let Value::Generator(_) = &target {
                // We don't track the originating function name on the
                // Generator object in our VM; return a minimal Reflection
                // object pointing at "{closure}".
                let obj_id = vm.next_object_id();
                let mut rf_obj = PhpObject::new(b"ReflectionFunction".to_vec(), obj_id);
                rf_obj.set_property(b"name".to_vec(), Value::String(PhpString::from_bytes(b"{closure}")));
                rf_obj.set_property(b"__reflection_target".to_vec(), Value::String(PhpString::from_bytes(b"")));
                return Some(Value::Object(Rc::new(RefCell::new(rf_obj))));
            }
            Some(Value::Null)
        }
        b"getthis" => {
            Some(Value::Null)
        }
        b"getexecutinggenerator" => {
            let ob = obj.borrow();
            Some(ob.get_property(b"__reflection_target"))
        }
        b"gettrace" => {
            // Return an empty trace array (our generators don't track frames
            // in a way we can expose here).
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
        }
        b"isclosed" => {
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
            "Reflection", "ReflectionException", "ReflectionFunctionAbstract",
            "ReflectionFunction", "ReflectionGenerator", "ReflectionMethod",
            "ReflectionClass", "ReflectionObject", "ReflectionProperty",
            "ReflectionClassConstant", "ReflectionParameter", "ReflectionType",
            "ReflectionNamedType", "ReflectionUnionType", "ReflectionIntersectionType",
            "ReflectionExtension", "ReflectionZendExtension",
            "ReflectionReference", "ReflectionAttribute", "ReflectionEnum",
            "ReflectionEnumUnitCase", "ReflectionEnumBackedCase",
            "ReflectionFiber", "ReflectionConstant",
        ],
        "standard" => &[
            "AssertionError", "__PHP_Incomplete_Class",
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
            let ob = obj.borrow();
            let types = ob.get_property(b"__types");
            let class_lower: Vec<u8> = ob.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            drop(ob);
            let sep = if class_lower == b"reflectionuniontype" { "|" } else { "&" };
            if let Value::Array(arr) = types {
                let names: Vec<String> = arr.borrow().iter().map(|(_, v)| {
                    if let Value::Object(t) = v {
                        let t = t.borrow();
                        t.get_property(b"__type_name").to_php_string().to_string_lossy()
                    } else {
                        String::new()
                    }
                }).collect();
                Some(Value::String(PhpString::from_string(names.join(sep))))
            } else {
                Some(Value::String(PhpString::empty()))
            }
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
        b"getdoccomment" => Some(Value::False),
        b"isfinal" => Some(if is_final { Value::True } else { Value::False }),
        b"isenumcase" => {
            let val = ob.get_property(b"__reflection_value");
            drop(ob);
            Some(if Vm::is_enum_case(&val) { Value::True } else { Value::False })
        }
        b"isdeprecated" => Some(Value::False),
        b"hastype" => Some(Value::False),
        b"gettype" => Some(Value::Null),
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
            let val_str = reflection_value_repr(&val);
            Some(Value::String(PhpString::from_string(format!("Constant [ {}{} {} ] {{ {} }}\n", final_str, vis_str, name, val_str))))
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
pub fn reflection_static_call(vm: &mut Vm, class_lower: &str, method_lower: &str, args: &[Value], line: u32) -> Option<Value> {
    match class_lower {
        "reflection" => {
            match method_lower {
                "getmodifiernames" => {
                    let modifiers = args.first().map(|v| v.to_long()).unwrap_or(0);
                    let mut names = PhpArray::new();
                    if modifiers & 0x10 != 0 { names.push(Value::String(PhpString::from_bytes(b"static"))); }
                    if modifiers & 0x40 != 0 { names.push(Value::String(PhpString::from_bytes(b"abstract"))); }
                    if modifiers & 0x20 != 0 { names.push(Value::String(PhpString::from_bytes(b"final"))); }
                    if modifiers & 1 != 0 { names.push(Value::String(PhpString::from_bytes(b"public"))); }
                    if modifiers & 2 != 0 { names.push(Value::String(PhpString::from_bytes(b"protected"))); }
                    if modifiers & 4 != 0 { names.push(Value::String(PhpString::from_bytes(b"private"))); }
                    if modifiers & 0x10000 != 0 { names.push(Value::String(PhpString::from_bytes(b"readonly"))); }
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
                // Non-static instance methods cannot be called statically.
                "getname" | "isiterateable" | "isiterable" | "getmethods"
                | "getproperties" | "getconstants" => {
                    let err_msg = format!(
                        "Non-static method ReflectionClass::{}() cannot be called statically",
                        method_lower
                    );
                    let exc = vm.create_exception(b"Error", &err_msg, line);
                    vm.current_exception = Some(exc);
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
    let obj_id = vm.next_object_id();
    let mut obj = PhpObject::new(b"ReflectionClass".to_vec(), obj_id);
    obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(class_name.to_string())));
    obj.set_property(b"__reflection_target".to_vec(), Value::String(PhpString::from_string(class_name.to_string())));
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Create a ReflectionMethod object for a given class and method name
pub fn create_reflection_method(vm: &mut Vm, class_name: &str, method_name: &str) -> Value {
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
    obj.set_property(b"class".to_vec(), Value::String(PhpString::from_string(class_name.to_string())));
    obj.set_property(b"__reflection_value".to_vec(), value);
    Value::Object(Rc::new(RefCell::new(obj)))
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
        obj.set_property(b"__reflection_param_idx".to_vec(), Value::Long(cv_idx as i64));
        // Store class and method info for attribute lookup
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
    match param_type {
        ParamType::Simple(name) => {
            let type_name = String::from_utf8_lossy(name).to_string();
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
                    let type_name = String::from_utf8_lossy(name).to_string();
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
                    let inner_type = create_reflection_type(vm, inner);
                    let null_type = create_reflection_type(vm, &ParamType::Simple(b"null".to_vec()));
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
            let mut type_arr = PhpArray::new();
            let mut allows_null = false;
            for t in types {
                let rt = create_reflection_type(vm, t);
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
                let rt = create_reflection_type(vm, t);
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
    if prop.is_readonly { mods |= 0x10000; }
    mods
}

/// Build the complete __toString representation for a ReflectionClass
fn reflection_class_to_string(vm: &Vm, name: &str, class_lower: &[u8]) -> String {
    let mut s = String::new();

    let is_interface = vm.classes.get(class_lower).map(|c| c.is_interface).unwrap_or(false);
    let is_trait = vm.classes.get(class_lower).map(|c| c.is_trait).unwrap_or(false);
    let is_enum = vm.classes.get(class_lower).map(|c| c.is_enum).unwrap_or(false);
    let is_abstract = vm.classes.get(class_lower).map(|c| c.is_abstract).unwrap_or(false);
    let is_final = vm.classes.get(class_lower).map(|c| c.is_final).unwrap_or(false);
    let is_readonly = vm.classes.get(class_lower).map(|c| c.is_readonly).unwrap_or(false);
    let is_user = vm.classes.contains_key(class_lower);

    // Header line
    if is_interface {
        s.push_str(&format!("Interface [ <user> interface {} ", name));
    } else if is_trait {
        s.push_str(&format!("Trait [ <user> trait {} ", name));
    } else if is_enum {
        let kind = if is_user { "user" } else { "internal" };
        s.push_str(&format!("Class [ <{}> final class {} ", kind, name));
    } else {
        let kind = if is_user { "user" } else { "internal" };
        let mut modifiers = String::new();
        if is_abstract { modifiers.push_str("abstract "); }
        if is_final { modifiers.push_str("final "); }
        if is_readonly { modifiers.push_str("readonly "); }
        s.push_str(&format!("Class [ <{}> {}class {} ", kind, modifiers, name));
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

    // Constants section
    if let Some(ce) = vm.classes.get(class_lower) {
        let const_count = ce.constants.len();
        s.push_str(&format!("\n  - Constants [{}] {{\n", const_count));
        for (cname, cval) in &ce.constants {
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
            let val_str = reflection_value_repr(cval);
            s.push_str(&format!("    Constant [ {}{} {} ] {{ {} }}\n", final_str, vis_str, const_name_str, val_str));
        }
        s.push_str("  }\n");
    } else {
        s.push_str("\n  - Constants [0] {\n  }\n");
    }

    // Static properties
    if let Some(ce) = vm.classes.get(class_lower) {
        let static_props: Vec<_> = ce.properties.iter().filter(|p| p.is_static).collect();
        s.push_str(&format!("\n  - Static properties [{}] {{\n", static_props.len()));
        for prop in static_props {
            let vis = match prop.visibility {
                Visibility::Public => "public",
                Visibility::Protected => "protected",
                Visibility::Private => "private",
            };
            let type_str = prop.property_type.as_ref().map(|t| format!(" {}", param_type_name(t))).unwrap_or_default();
            let prop_name_str = String::from_utf8_lossy(&prop.name);
            s.push_str(&format!("    Property [ {} static{} ${} ]\n", vis, type_str, prop_name_str));
        }
        s.push_str("  }\n");
    } else {
        s.push_str("\n  - Static properties [0] {\n  }\n");
    }

    // Static methods
    if let Some(ce) = vm.classes.get(class_lower) {
        let static_methods: Vec<_> = ce.methods.values().filter(|m| m.is_static).collect();
        s.push_str(&format!("\n  - Static methods [{}] {{\n", static_methods.len()));
        for m in static_methods {
            reflection_method_to_string_section(&mut s, m);
        }
        s.push_str("  }\n");
    } else {
        s.push_str("\n  - Static methods [0] {\n  }\n");
    }

    // Properties (non-static)
    if let Some(ce) = vm.classes.get(class_lower) {
        let inst_props: Vec<_> = ce.properties.iter().filter(|p| !p.is_static).collect();
        s.push_str(&format!("\n  - Properties [{}] {{\n", inst_props.len()));
        for prop in inst_props {
            let vis = match prop.visibility {
                Visibility::Public => "public",
                Visibility::Protected => "protected",
                Visibility::Private => "private",
            };
            let readonly_str = if prop.is_readonly { " readonly" } else { "" };
            let type_str = prop.property_type.as_ref().map(|t| format!(" {}", param_type_name(t))).unwrap_or_default();
            let prop_name_str = String::from_utf8_lossy(&prop.name);
            let default_str = if !matches!(prop.default, Value::Undef) {
                format!(" = {}", reflection_value_repr(&prop.default))
            } else {
                String::new()
            };
            s.push_str(&format!("    Property [ {}{}{} ${}{} ]\n", vis, readonly_str, type_str, prop_name_str, default_str));
        }
        s.push_str("  }\n");
    } else {
        s.push_str("\n  - Properties [0] {\n  }\n");
    }

    // Methods (non-static)
    if let Some(ce) = vm.classes.get(class_lower) {
        let inst_methods: Vec<_> = ce.methods.values().filter(|m| !m.is_static).collect();
        s.push_str(&format!("\n  - Methods [{}] {{\n", inst_methods.len()));
        for m in inst_methods {
            reflection_method_to_string_section(&mut s, m);
        }
        s.push_str("  }\n");
    } else {
        s.push_str("\n  - Methods [0] {\n  }\n");
    }

    s.push_str("}\n");
    s
}

/// Helper to add a method's __toString to the class output
fn reflection_method_to_string_section(s: &mut String, m: &crate::object::MethodDef) {
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

    let mod_vis = if modifiers.is_empty() {
        vis.to_string()
    } else {
        format!("{}{}", modifiers, vis)
    };
    s.push_str(&format!("    Method [ <user{}> {} method {} ] {{\n", ctor_str, mod_vis, method_name));

    // Parameters - skip $this (first param for non-static methods)
    let skip = if !m.is_static { 1usize } else { 0usize };
    let param_count = if m.op_array.param_count as usize > skip { m.op_array.param_count as usize - skip } else { 0 };
    let required_count = if m.op_array.required_param_count as usize > skip { m.op_array.required_param_count as usize - skip } else { 0 };

    s.push_str(&format!("\n      - Parameters [{}] {{\n", param_count));
    for i in 0..param_count {
        let cv_idx = i + skip;
        if cv_idx < m.op_array.cv_names.len() {
            let pname = String::from_utf8_lossy(&m.op_array.cv_names[cv_idx]);
            let required = i < required_count;
            let type_str = if cv_idx < m.op_array.param_types.len() {
                if let Some(ref pti) = m.op_array.param_types[cv_idx] {
                    format!("{} ", param_type_name(&pti.param_type))
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            s.push_str(&format!("        Parameter #{} [ <{}> {}${} ]\n", i,
                if required { "required" } else { "optional" }, type_str, pname));
        }
    }
    s.push_str("      }\n");

    // Return type
    if let Some(ref rt) = m.op_array.return_type {
        s.push_str(&format!("      - Return [ {} ]\n", param_type_name(rt)));
    }

    s.push_str("    }\n\n");
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
    for attr in attrs {
        // If a filter name is specified, check it
        if let Some(fname) = filter_name {
            if filter_flags == 0 {
                // Exact match (default)
                if !attr.name.eq_ignore_ascii_case(fname) {
                    continue;
                }
            } else {
                // IS_INSTANCEOF = 2: check if attribute class is the given class or subclass
                // For now, just do case-insensitive match (we can refine later)
                if !attr.name.eq_ignore_ascii_case(fname) {
                    // Check if class inherits from filter_name
                    let attr_lower: Vec<u8> = attr.name.iter().map(|b| b.to_ascii_lowercase()).collect();
                    let filter_lower: Vec<u8> = fname.iter().map(|b| b.to_ascii_lowercase()).collect();
                    let is_subclass = vm.classes.get(&attr_lower)
                        .map(|ce| {
                            // Walk parent chain
                            let mut current = ce.parent.clone();
                            while let Some(ref p) = current {
                                let p_lower: Vec<u8> = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                                if p_lower == filter_lower {
                                    return true;
                                }
                                current = vm.classes.get(&p_lower).and_then(|c| c.parent.clone());
                            }
                            // Check interfaces
                            for iface in &ce.interfaces {
                                let i_lower: Vec<u8> = iface.iter().map(|b| b.to_ascii_lowercase()).collect();
                                if i_lower == filter_lower {
                                    return true;
                                }
                            }
                            false
                        })
                        .unwrap_or(false);
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
            // Create a new instance of the attribute class
            let class_lower: Vec<u8> = attr_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
            if !vm.classes.contains_key(&class_lower) {
                let err_msg = format!("Attribute class \"{}\" not found", attr_name);
                let exc = vm.create_exception(b"Error", &err_msg, 0);
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
            // Check if the class has #[Attribute] -- only attribute classes can be instantiated via newInstance
            let is_attribute_class = vm.classes.get(&class_lower)
                .map(|ce| ce.attributes.iter().any(|a| a.name.eq_ignore_ascii_case(b"attribute")))
                .unwrap_or(false);
            if !is_attribute_class && class_lower != b"attribute" {
                let err_msg = format!("Attempting to use non-attribute class \"{}\" as attribute", attr_name);
                let exc = vm.create_exception(b"Error", &err_msg, 0);
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
            let mut new_obj = PhpObject::new(ce.name.clone(), obj_id);
            // Initialize properties from class definition
            for prop in &ce.properties {
                if !prop.is_static && !matches!(prop.default, Value::Undef) {
                    new_obj.set_property(prop.name.clone(), prop.default.clone());
                }
            }
            let instance = Value::Object(Rc::new(RefCell::new(new_obj)));
            // Call __construct if it exists
            let construct_lower = b"__construct".to_vec();
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
