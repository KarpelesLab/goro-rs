//! spl_autoload_* functions (php-src ext/spl/php_spl.c).
//!
//! Registered autoloaders are stored normalized in `vm.autoload_functions`
//! the way spl_autoload_functions() reports them: a function name string,
//! a `[class-or-object, method]` array, or the closure / invokable object.

use goro_core::array::PhpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

thread_local! {
    static AUTOLOAD_EXTENSIONS: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

const DEFAULT_EXTENSIONS: &[u8] = b".inc,.php";

fn lower(b: &[u8]) -> Vec<u8> {
    b.iter().map(|c| c.to_ascii_lowercase()).collect()
}

fn throw(vm: &mut Vm, class: &[u8], msg: &str) -> Result<Value, VmError> {
    let line = vm.current_line;
    let exc = vm.create_exception(class, msg, line);
    vm.current_exception = Some(exc);
    Err(VmError { message: msg.to_string(), line })
}

fn is_closure_name(b: &[u8]) -> bool {
    b.starts_with(b"__closure_") || b.starts_with(b"__arrow_") || b.starts_with(b"__bound_closure_") || b.starts_with(b"__closure_fcc_")
}

fn class_display(vm: &Vm, class_lower: &[u8]) -> String {
    vm.classes
        .get(class_lower)
        .map(|c| String::from_utf8_lossy(&c.name).to_string())
        .unwrap_or_else(|| vm.builtin_canonical_name(class_lower))
}

/// Visibility check for a method callable from the current scope.
fn check_access(vm: &Vm, ce_name: &[u8], m: &goro_core::object::MethodDef) -> Result<(), String> {
    use goro_core::object::Visibility;
    let vis = match m.visibility {
        Visibility::Public => return Ok(()),
        Visibility::Protected => "protected",
        Visibility::Private => "private",
    };
    if let Some(scope) = vm.current_class_scope() {
        let scope_l = lower(&scope);
        let decl = &m.declaring_class;
        let ok = match m.visibility {
            Visibility::Private => scope_l == *decl,
            _ => scope_l == *decl || vm.class_extends(&scope_l, decl) || vm.class_extends(decl, &scope_l),
        };
        if ok {
            return Ok(());
        }
    }
    Err(format!(
        "cannot access {} method {}::{}()",
        vis,
        String::from_utf8_lossy(ce_name),
        String::from_utf8_lossy(&m.name)
    ))
}

/// Resolve a static "Class::method" / ["Class", "method"] callable.
fn resolve_static(vm: &mut Vm, class: &[u8], method: &[u8]) -> Result<Value, String> {
    let class = class.strip_prefix(b"\\").unwrap_or(class);
    let cl = lower(class);
    if !vm.classes.contains_key(&cl) {
        vm.try_autoload_class(class);
    }
    let ce = match vm.classes.get(&cl) {
        Some(c) => c.clone(),
        None => return Err(format!("class \"{}\" not found", String::from_utf8_lossy(class))),
    };
    let ml = lower(method);
    match ce.get_method(&ml) {
        Some(m) => {
            check_access(vm, &ce.name, m)?;
            if !m.is_static {
                return Err(format!(
                    "non-static method {}::{}() cannot be called statically",
                    String::from_utf8_lossy(&ce.name),
                    String::from_utf8_lossy(&m.name)
                ));
            }
            let mut a = PhpArray::new();
            a.push(Value::String(PhpString::from_vec(ce.name.clone())));
            a.push(Value::String(PhpString::from_vec(m.name.clone())));
            Ok(Value::Array(Rc::new(RefCell::new(a))))
        }
        None => {
            if ce.get_method(b"__callstatic").is_some() {
                let mut a = PhpArray::new();
                a.push(Value::String(PhpString::from_vec(ce.name.clone())));
                a.push(Value::String(PhpString::from_vec(method.to_vec())));
                return Ok(Value::Array(Rc::new(RefCell::new(a))));
            }
            Err(format!(
                "class {} does not have a method \"{}\"",
                String::from_utf8_lossy(&ce.name),
                String::from_utf8_lossy(method)
            ))
        }
    }
}

/// zend_is_callable() for autoload registration: the normalized callable,
/// or the error detail.
fn resolve_callable(vm: &mut Vm, cb: &Value) -> Result<Value, String> {
    match cb {
        Value::String(s) => {
            let b = s.as_bytes();
            if is_closure_name(b) {
                return Ok(cb.clone());
            }
            if let Some(pos) = b.windows(2).position(|w| w == b"::") {
                return resolve_static(vm, &b[..pos], &b[pos + 2..]);
            }
            let name = b.strip_prefix(b"\\").unwrap_or(b);
            let l = lower(name);
            if vm.user_functions.contains_key(&l) || vm.functions.contains_key(&l) {
                // Report the declared name.
                let declared = vm
                    .user_functions
                    .get(&l)
                    .map(|f| f.name.clone())
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| name.to_vec());
                return Ok(Value::String(PhpString::from_vec(declared)));
            }
            Err(format!("function \"{}\" not found or invalid function name", s.to_string_lossy()))
        }
        Value::Array(a) => {
            let vals: Vec<Value> = a.borrow().values().cloned().collect();
            if vals.len() != 2 {
                return Err("array callback must have exactly two members".to_string());
            }
            if let Value::String(s0) = &vals[0] {
                if is_closure_name(s0.as_bytes()) {
                    return Ok(cb.clone());
                }
            }
            let method = match &vals[1] {
                Value::String(m) => m.as_bytes().to_vec(),
                _ => return Err("second array member is not a valid method".to_string()),
            };
            match &vals[0] {
                Value::Object(o) => {
                    let cl = lower(&o.borrow().class_name);
                    let ml = lower(&method);
                    let ce = vm.classes.get(&cl).cloned();
                    let found = ce.as_ref().and_then(|c| c.get_method(&ml).cloned());
                    match found {
                        Some(m) => {
                            let ce_name = ce.as_ref().map(|c| c.name.clone()).unwrap_or_default();
                            if let Err(e) = check_access(vm, &ce_name, &m) {
                                // Inaccessible methods go through __call.
                                if ce.as_ref().map(|c| c.get_method(b"__call").is_some()).unwrap_or(false) {
                                    return Ok(cb.clone());
                                }
                                return Err(e);
                            }
                            let mut arr = PhpArray::new();
                            if m.is_static {
                                // Static methods are not bound to the object.
                                arr.push(Value::String(PhpString::from_vec(ce_name)));
                            } else {
                                arr.push(vals[0].clone());
                            }
                            arr.push(Value::String(PhpString::from_vec(m.name.clone())));
                            Ok(Value::Array(Rc::new(RefCell::new(arr))))
                        }
                        None => {
                            if ce.as_ref().map(|c| c.get_method(b"__call").is_some()).unwrap_or(false) {
                                return Ok(cb.clone());
                            }
                            Err(format!(
                                "class {} does not have a method \"{}\"",
                                class_display(vm, &cl),
                                String::from_utf8_lossy(&method)
                            ))
                        }
                    }
                }
                Value::String(c) => resolve_static(vm, c.as_bytes(), &method),
                _ => Err("first array member is not a valid class name or object".to_string()),
            }
        }
        Value::Object(o) => {
            let cl = lower(&o.borrow().class_name);
            if cl == b"closure" || vm.classes.get(&cl).map(|c| c.get_method(b"__invoke").is_some()).unwrap_or(false) {
                Ok(cb.clone())
            } else {
                Err("no array or string given".to_string())
            }
        }
        _ => Err("no array or string given".to_string()),
    }
}

fn same_callable(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::String(x), Value::String(y)) => x.as_bytes().eq_ignore_ascii_case(y.as_bytes()),
        (Value::Array(x), Value::Array(y)) => {
            let xb = x.borrow();
            let yb = y.borrow();
            xb.len() == yb.len()
                && xb.iter().zip(yb.iter()).all(|((_, va), (_, vb))| match (va, vb) {
                    (Value::String(sa), Value::String(sb)) => sa.as_bytes().eq_ignore_ascii_case(sb.as_bytes()),
                    (Value::Object(oa), Value::Object(ob)) => Rc::ptr_eq(oa, ob),
                    _ => false,
                })
        }
        (Value::Object(x), Value::Object(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

pub fn spl_autoload_register_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let callback = args.first().cloned().unwrap_or(Value::Null).deref();
    let normalized = if matches!(callback, Value::Null) {
        Value::String(PhpString::from_bytes(b"spl_autoload"))
    } else {
        match resolve_callable(vm, &callback) {
            Ok(v) => v,
            Err(detail) => {
                let msg = format!(
                    "spl_autoload_register(): Argument #1 ($callback) must be a valid callback or null, {}",
                    detail
                );
                return throw(vm, b"TypeError", &msg);
            }
        }
    };
    if let Value::String(s) = &normalized {
        if s.as_bytes().eq_ignore_ascii_case(b"spl_autoload_call") {
            return throw(
                vm,
                b"ValueError",
                "spl_autoload_register(): Argument #1 ($callback) must not be the spl_autoload_call() function",
            );
        }
    }
    if let Some(v) = args.get(1) {
        if !v.to_bool() {
            let line = vm.current_line;
            vm.emit_notice_at(
                "spl_autoload_register(): Argument #2 ($do_throw) has been ignored, spl_autoload_register() will always throw",
                line,
            );
        }
    }
    let prepend = args.get(2).is_some_and(|v| v.to_bool());
    if vm.autoload_functions.iter().any(|e| same_callable(e, &normalized)) {
        return Ok(Value::True);
    }
    if prepend {
        vm.autoload_functions.insert(0, normalized);
    } else {
        vm.autoload_functions.push(normalized);
    }
    Ok(Value::True)
}

pub fn spl_autoload_functions_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    for f in vm.autoload_functions.iter() {
        arr.push(f.clone());
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

pub fn spl_autoload_unregister_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let callback = args.first().cloned().unwrap_or(Value::Null).deref();
    if let Value::String(s) = &callback {
        if s.as_bytes().eq_ignore_ascii_case(b"spl_autoload_call") {
            let line = vm.current_line;
            vm.emit_deprecated_at(
                "spl_autoload_unregister(): Using spl_autoload_call() as a callback for spl_autoload_unregister() is deprecated, to remove all registered autoloaders, call spl_autoload_unregister() for all values returned from spl_autoload_functions()",
                line,
            );
            vm.autoload_functions.clear();
            return Ok(Value::True);
        }
    }
    let normalized = match resolve_callable(vm, &callback) {
        Ok(v) => v,
        Err(detail) => {
            let msg = format!("spl_autoload_unregister(): Argument #1 ($callback) must be a valid callback, {}", detail);
            return throw(vm, b"TypeError", &msg);
        }
    };
    if let Some(p) = vm.autoload_functions.iter().position(|e| same_callable(e, &normalized)) {
        vm.autoload_functions.remove(p);
        Ok(Value::True)
    } else {
        Ok(Value::False)
    }
}

fn current_extensions() -> Vec<u8> {
    AUTOLOAD_EXTENSIONS.with(|e| e.borrow().clone()).unwrap_or_else(|| DEFAULT_EXTENSIONS.to_vec())
}

pub fn spl_autoload_extensions_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(v) = args.first() {
        if !matches!(v.deref(), Value::Null) {
            let s = v.to_php_string().as_bytes().to_vec();
            AUTOLOAD_EXTENSIONS.with(|e| *e.borrow_mut() = Some(s));
        }
    }
    Ok(Value::String(PhpString::from_vec(current_extensions())))
}

/// Resolve a relative path against include_path and the running script's
/// directory (php_resolve_path).
fn resolve_include(vm: &Vm, file: &str) -> Option<String> {
    if file.starts_with('/') {
        return if std::path::Path::new(file).is_file() { Some(file.to_string()) } else { None };
    }
    let inc = vm
        .constants
        .get(b"include_path".as_slice())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| ".".to_string());
    for dir in inc.split(':') {
        let base = if dir.is_empty() || dir == "." {
            std::env::current_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| ".".to_string())
        } else {
            dir.to_string()
        };
        let p = format!("{}/{}", base.trim_end_matches('/'), file);
        if std::path::Path::new(&p).is_file() {
            return Some(std::fs::canonicalize(&p).map(|c| c.to_string_lossy().to_string()).unwrap_or(p));
        }
    }
    // Fallback: the directory of the executing script.
    if let Some(pos) = vm.current_file.rfind('/') {
        let p = format!("{}/{}", &vm.current_file[..pos], file);
        if std::path::Path::new(&p).is_file() {
            return Some(std::fs::canonicalize(&p).map(|c| c.to_string_lossy().to_string()).unwrap_or(p));
        }
    }
    None
}

pub fn spl_autoload_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let class = match args.first().map(|v| v.deref()) {
        Some(Value::String(s)) => s.as_bytes().to_vec(),
        Some(Value::Array(_)) => {
            return throw(vm, b"TypeError", "spl_autoload(): Argument #1 ($class) must be of type string, array given");
        }
        Some(v) => v.to_php_string().as_bytes().to_vec(),
        None => {
            return throw(vm, b"ArgumentCountError", "spl_autoload() expects at least 1 argument, 0 given");
        }
    };
    let exts = match args.get(1).map(|v| v.deref()) {
        Some(Value::Null) | None => current_extensions(),
        Some(v) => v.to_php_string().as_bytes().to_vec(),
    };
    let lc = lower(&class);
    let lc_name = String::from_utf8_lossy(&lc).replace('\\', "/");
    for ext in exts.split(|&c| c == b',') {
        if vm.current_exception.is_some() {
            break;
        }
        let file = format!("{}{}", lc_name, String::from_utf8_lossy(ext));
        if let Some(path) = resolve_include(vm, &file) {
            vm.spl_include_once(&path);
            if vm.classes.contains_key(&lc) {
                break;
            }
        }
    }
    Ok(Value::Null)
}

pub fn spl_autoload_call_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let class = match args.first().map(|v| v.deref()) {
        Some(Value::String(s)) => s,
        Some(Value::Long(_)) | Some(Value::Double(_)) | Some(Value::True) | Some(Value::False) => {
            args[0].to_php_string()
        }
        Some(other) => {
            let msg = format!(
                "spl_autoload_call(): Argument #1 ($class) must be of type string, {} given",
                Vm::value_type_name(&other)
            );
            return throw(vm, b"TypeError", &msg);
        }
        None => {
            return throw(vm, b"ArgumentCountError", "spl_autoload_call() expects exactly 1 argument, 0 given");
        }
    };
    let lc = lower(class.as_bytes());
    let autoloaders = vm.autoload_functions.clone();
    let cv = Value::String(class.clone());
    for cb in &autoloaders {
        if let Some(e) = vm.spl_call_autoloader(cb, &cv) {
            if vm.current_exception.is_none() {
                return Err(e);
            }
        }
        if vm.current_exception.is_some() {
            break;
        }
        if vm.classes.contains_key(&lc) {
            break;
        }
    }
    Ok(Value::Null)
}

