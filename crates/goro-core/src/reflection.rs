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
    }
    true
}

/// ReflectionMethod constructor
pub fn reflection_method_construct(vm: &mut Vm, args: &[Value], line: u32) -> bool {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return true,
    };

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

    let func_name = func_arg.to_php_string().to_string_lossy();
    let param_idx = param_arg.to_long() as usize;

    let func_lower: Vec<u8> = func_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

    // Look up the function
    if let Some(op_array) = vm.user_functions.get(&func_lower).cloned() {
        let param_name = if param_idx < op_array.cv_names.len() {
            String::from_utf8_lossy(&op_array.cv_names[param_idx]).to_string()
        } else {
            format!("param{}", param_idx)
        };

        let mut obj = this.borrow_mut();
        obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(param_name)));
        obj.set_property(b"__reflection_func".to_vec(), Value::String(PhpString::from_vec(func_lower)));
        obj.set_property(b"__reflection_param_idx".to_vec(), Value::Long(param_idx as i64));
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
            let mut result = PhpArray::new();
            if let Some(ce) = vm.classes.get(&class_lower) {
                for prop in &ce.properties {
                    if !prop.is_static {
                        result.set(
                            ArrayKey::String(PhpString::from_vec(prop.name.clone())),
                            prop.default.clone(),
                        );
                    }
                }
                // Also add parent properties
                let mut parent = ce.parent.clone();
                while let Some(ref p) = parent {
                    let p_lower: Vec<u8> = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                    if let Some(pce) = vm.classes.get(&p_lower) {
                        for prop in &pce.properties {
                            if !prop.is_static {
                                let key = ArrayKey::String(PhpString::from_vec(prop.name.clone()));
                                // Don't override child properties
                                if result.get(&key).is_none() {
                                    // Skip private properties from parent
                                    if prop.visibility != Visibility::Private {
                                        result.set(key, prop.default.clone());
                                    }
                                }
                            }
                        }
                        parent = pce.parent.clone();
                    } else {
                        break;
                    }
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"getstaticproperties" => {
            let mut result = PhpArray::new();
            if let Some(ce) = vm.classes.get(&class_lower) {
                for (name, val) in &ce.static_properties {
                    result.set(
                        ArrayKey::String(PhpString::from_vec(name.clone())),
                        val.clone(),
                    );
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
        b"hascase" => {
            if let Some(ce) = vm.classes.get(&class_lower) {
                let ob = obj.borrow();
                // The case name is passed as first arg but for no-arg dispatch, we check if it's been called
                // Actually hasCase needs args - but since it's dispatched via no-arg, let's handle it here
                // No-arg means we got called with no specific args (should be impossible for hasCase)
                drop(ob);
                Some(Value::False)
            } else {
                Some(Value::False)
            }
        }
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
                Some(if has { Value::True } else { Value::False })
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
                let mut result = PhpArray::new();
                if let Some(ce) = vm.classes.get(&class_lower) {
                    for (name, val) in &ce.constants {
                        result.set(ArrayKey::String(PhpString::from_vec(name.clone())), val.clone());
                    }
                }
                // Also check parent constants
                reflection_collect_parent_constants(vm, &class_lower, &mut result);
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
                Some(Value::Array(Rc::new(RefCell::new(result))))
            }
            b"getproperty" => {
                let prop_name = args.get(1)?.to_php_string().to_string_lossy();
                let has = vm.classes.get(&class_lower)
                    .map(|c| c.properties.iter().any(|p| p.name == prop_name.as_bytes()))
                    .unwrap_or(false);
                if has {
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
                let prop_names: Vec<String> = if let Some(ce) = vm.classes.get(&class_lower) {
                    ce.properties.iter().filter_map(|prop| {
                        if filter != -1 {
                            let prop_mod = reflection_property_modifiers_static(prop);
                            if prop_mod & filter == 0 {
                                return None;
                            }
                        }
                        if !prop.is_static {
                            Some(String::from_utf8_lossy(&prop.name).to_string())
                        } else {
                            None
                        }
                    }).collect()
                } else {
                    vec![]
                };
                for prop_name in prop_names {
                    result.push(create_reflection_property(vm, &target, &prop_name));
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
    drop(ob);

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
        b"getclosure" => {
            // getClosure() with no args returns a closure for the method
            Some(Value::Null)
        }
        b"getstaticvariables" => {
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
        }
        b"setaccessible" => {
            Some(Value::Null)
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
        _ => None,
    }
}

/// ReflectionProperty methods with args
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

/// ReflectionExtension no-arg method dispatch
pub fn reflection_extension_method(
    _vm: &mut Vm,
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
        b"getversion" => {
            Some(Value::String(PhpString::from_bytes(b"8.5.4")))
        }
        b"getfunctions" => {
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
        }
        b"getclasses" => {
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
        }
        b"getclassnames" => {
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
        }
        b"getconstants" => {
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
        }
        b"getinientries" => {
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
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
        _ => None,
    }
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
        "reflectionclass" => {
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
            obj.set_property(b"__type_name".to_vec(), Value::String(PhpString::from_string(type_name)));
            obj.set_property(b"__allows_null".to_vec(), Value::False);
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
                if let ParamType::Simple(name) = t {
                    if name == b"null" {
                        allows_null = true;
                    }
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

/// Helper to get a constant from a class (walks parent chain)
fn reflection_class_get_constant(vm: &Vm, class_lower: &[u8], const_name: &[u8]) -> Option<Value> {
    if let Some(ce) = vm.classes.get(class_lower) {
        if let Some(val) = ce.constants.get(const_name) {
            return Some(val.clone());
        }
        // Check parent chain
        if let Some(ref parent) = ce.parent {
            let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
            return reflection_class_get_constant(vm, &parent_lower, const_name);
        }
    }
    None
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
