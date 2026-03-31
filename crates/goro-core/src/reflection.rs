// Reflection API implementation for goro-rs
// Extracted from vm.rs - provides PHP Reflection classes

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::{PhpObject, Visibility};
use crate::opcode::{OpArray, ParamType};
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::Vm;

// ==================== Constructors ====================

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

// ==================== No-arg method dispatchers ====================

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
            // User-defined classes - return false for built-in
            if vm.classes.contains_key(&class_lower) {
                Some(Value::String(PhpString::from_string(vm.current_file.clone())))
            } else {
                Some(Value::False)
            }
        }
        b"getstartline" => {
            Some(Value::False)
        }
        b"getendline" => {
            Some(Value::False)
        }
        b"getdoccomment" => {
            Some(Value::False)
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
        b"getattributes" => {
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
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
        b"__tostring" => {
            // Build a __toString representation for ReflectionClass
            let ob = obj.borrow();
            let name = ob.get_property(b"name").to_php_string().to_string_lossy();
            drop(ob);
            let mut s = String::new();
            // Determine class type
            let is_interface = vm.classes.get(&class_lower).map(|c| c.is_interface).unwrap_or(false);
            let is_trait = vm.classes.get(&class_lower).map(|c| c.is_trait).unwrap_or(false);
            let is_abstract = vm.classes.get(&class_lower).map(|c| c.is_abstract).unwrap_or(false);
            let is_final = vm.classes.get(&class_lower).map(|c| c.is_final).unwrap_or(false);

            if is_interface {
                s.push_str(&format!("Interface [ <user> interface {} ", name));
            } else if is_trait {
                s.push_str(&format!("Trait [ <user> trait {} ", name));
            } else {
                let kind = if vm.classes.contains_key(&class_lower) { "user" } else { "internal" };
                let modifiers = if is_abstract { "abstract " } else if is_final { "final " } else { "" };
                s.push_str(&format!("Class [ <{}> {}class {} ", kind, modifiers, name));
            }

            // Parent
            if let Some(ce) = vm.classes.get(&class_lower) {
                if let Some(ref parent) = ce.parent {
                    s.push_str(&format!("extends {} ", String::from_utf8_lossy(parent)));
                }
                if !ce.interfaces.is_empty() {
                    s.push_str("implements ");
                    let ifaces: Vec<String> = ce.interfaces.iter().map(|i| String::from_utf8_lossy(i).to_string()).collect();
                    s.push_str(&ifaces.join(", "));
                    s.push(' ');
                }
            }
            s.push_str("] {\n}\n");
            Some(Value::String(PhpString::from_string(s)))
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
                let filter = args.get(1).map(|v| v.to_long()).unwrap_or(-1);
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
                let filter = args.get(1).map(|v| v.to_long()).unwrap_or(-1);
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
            let count = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| m.op_array.param_count))
                .unwrap_or(0);
            Some(Value::Long(count as i64))
        }
        b"getnumberofrequiredparameters" => {
            let count = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()).map(|m| m.op_array.required_param_count))
                .unwrap_or(0);
            Some(Value::Long(count as i64))
        }
        b"getparameters" => {
            let op_array = vm.classes.get(&class_lower)
                .and_then(|c| c.get_method(method_lower.as_bytes()))
                .map(|m| m.op_array.clone());
            let params = if let Some(oa) = op_array {
                create_reflection_parameters(vm, &oa)
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
            Some(Value::String(PhpString::from_string(vm.current_file.clone())))
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
            Some(Value::False)
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
        b"getattributes" => {
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
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
            Some(Value::Null)
        }
        b"getextensionname" => {
            if vm.functions.contains_key(func_lower.as_slice()) {
                // Built-in functions have extension names, but we don't track them
                Some(Value::False)
            } else {
                Some(Value::False)
            }
        }
        b"getattributes" => {
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
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
            Some(Value::False)
        }
        b"gettype" => {
            Some(Value::Null)
        }
        b"setaccessible" => {
            Some(Value::Null)
        }
        b"getattributes" => {
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
        }
        _ => None,
    }
}

/// ReflectionProperty methods with args
pub fn reflection_property_docall(
    _vm: &mut Vm,
    method: &[u8],
    args: &[Value],
) -> Option<Value> {
    let this = args.first()?;
    if let Value::Object(obj) = this {
        let ob = obj.borrow();
        let prop_name = ob.get_property(b"__reflection_prop").to_php_string().to_string_lossy();
        drop(ob);

        match method {
            b"getvalue" => {
                let target = args.get(1)?;
                if let Value::Object(target_obj) = target {
                    let target_ob = target_obj.borrow();
                    Some(target_ob.get_property(prop_name.as_bytes()))
                } else {
                    Some(Value::Null)
                }
            }
            b"setvalue" => {
                if args.len() >= 3 {
                    let target = &args[1];
                    let value = args[2].clone();
                    if let Value::Object(target_obj) = target {
                        let mut target_ob = target_obj.borrow_mut();
                        target_ob.set_property(prop_name.as_bytes().to_vec(), value);
                    }
                }
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
        b"getattributes" => {
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
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
    match method {
        b"getname" => Some(ob.get_property(b"name")),
        b"getvalue" => Some(ob.get_property(b"__reflection_value")),
        b"getdeclaringclass" => {
            let class_name = ob.get_property(b"class").to_php_string().to_string_lossy();
            drop(ob);
            Some(create_reflection_class(vm, &class_name))
        }
        b"ispublic" => Some(Value::True),
        b"isprotected" => Some(Value::False),
        b"isprivate" => Some(Value::False),
        b"getmodifiers" => Some(Value::Long(1)), // IS_PUBLIC
        b"getdoccomment" => Some(Value::False),
        b"isfinal" => Some(Value::False),
        b"isenumcase" => {
            let val = ob.get_property(b"__reflection_value");
            drop(ob);
            Some(if Vm::is_enum_case(&val) { Value::True } else { Value::False })
        }
        b"isdeprecated" => Some(Value::False),
        b"hastype" => Some(Value::False),
        b"gettype" => Some(Value::Null),
        b"__tostring" => {
            let name = ob.get_property(b"name").to_php_string().to_string_lossy();
            let val = ob.get_property(b"__reflection_value");
            drop(ob);
            Some(Value::String(PhpString::from_string(format!("Constant [ public {} {} ]", name, val.to_php_string().to_string_lossy()))))
        }
        _ => None,
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

// ==================== Helper functions ====================

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
    for i in 0..op_array.param_count as usize {
        let param_name = if i < op_array.cv_names.len() {
            String::from_utf8_lossy(&op_array.cv_names[i]).to_string()
        } else {
            format!("param{}", i)
        };
        let obj_id = vm.next_object_id();
        let mut obj = PhpObject::new(b"ReflectionParameter".to_vec(), obj_id);
        obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(param_name)));
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

// ==================== Internal helpers ====================

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
