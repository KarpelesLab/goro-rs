//! Glue with the engine: values, objects, properties, user calls, and the
//! SOAP error handler (soap_error_handler() / soap_real_error_handler()),
//! plus SoapFault construction (set_soap_fault(), add_soap_fault()).

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::errdisplay::{E_ERROR, E_FATAL_ERRORS};
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use crate::types::*;

pub fn vstr(s: impl AsRef<[u8]>) -> Value {
    Value::String(PhpString::from_bytes(s.as_ref()))
}

pub fn vopt(s: Option<&str>) -> Value {
    match s {
        Some(s) => vstr(s),
        None => Value::Null,
    }
}

pub fn new_arr() -> Arr {
    Rc::new(RefCell::new(PhpArray::new()))
}

pub fn varr(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

/// The string of a string value.
pub fn as_str(v: &Value) -> Option<String> {
    match v.deref() {
        Value::String(s) => Some(s.to_string_lossy()),
        _ => None,
    }
}

pub fn as_bytes(v: &Value) -> Option<Vec<u8>> {
    match v.deref() {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

pub fn lower(s: &[u8]) -> Vec<u8> {
    s.to_ascii_lowercase()
}

/// A copy of a value as a separate zval (arrays get their own storage:
/// goro copies arrays eagerly).
pub fn copy_value(v: &Value) -> Value {
    match v {
        Value::Array(a) => {
            let c = a.borrow().clone();
            varr(c)
        }
        other => other.clone(),
    }
}

/// zend_symtable_update(): numeric string keys become integer keys.
pub fn symtable_key(k: &[u8]) -> ArrayKey {
    if let Ok(s) = std::str::from_utf8(k) {
        let b = s.as_bytes();
        let digits_ok = !b.is_empty()
            && (b[0] == b'-' && b.len() > 1 || b[0].is_ascii_digit())
            && b[if b[0] == b'-' { 1 } else { 0 }..]
                .iter()
                .all(|c| c.is_ascii_digit());
        let no_leading_zero = {
            let d = &b[if b.first() == Some(&b'-') { 1 } else { 0 }..];
            d.len() == 1 || d.first() != Some(&b'0')
        };
        if digits_ok && no_leading_zero && s != "-0" {
            if let Ok(n) = s.parse::<i64>() {
                return ArrayKey::Int(n);
            }
        }
    }
    ArrayKey::String(PhpString::from_bytes(k))
}

pub fn arr_set_str(a: &mut PhpArray, k: &str, v: Value) {
    a.set(symtable_key(k.as_bytes()), v);
}

/// zend_array_is_list()
pub fn is_list(a: &PhpArray) -> bool {
    let mut i = 0i64;
    for k in a.keys() {
        match k {
            ArrayKey::Int(n) if *n == i => i += 1,
            _ => return false,
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Classes and objects

/// instanceof_function()
pub fn instance_of(vm: &Vm, o: &Obj, class: &str) -> bool {
    let cl = lower(&o.borrow().class_name);
    let target = lower(class.as_bytes());
    if cl == target {
        return true;
    }
    vm.class_extends(&cl, &target) || vm.class_implements_interface(&cl, &target)
}

pub fn class_is(vm: &Vm, class_name: &[u8], target: &str) -> bool {
    let cl = lower(class_name);
    let target = lower(target.as_bytes());
    cl == target || vm.class_extends(&cl, &target) || vm.class_implements_interface(&cl, &target)
}

pub fn val_instance_of(vm: &Vm, v: &Value, class: &str) -> bool {
    match v.deref() {
        Value::Object(o) => instance_of(vm, &o, class),
        _ => false,
    }
}

/// zend_lookup_class() with autoloading: the declared name.
pub fn lookup_class(vm: &mut Vm, name: &[u8]) -> Option<Vec<u8>> {
    let name = name.strip_prefix(b"\\").unwrap_or(name);
    let l = lower(name);
    if let Some(c) = vm.lookup_class(&l) {
        return Some(c.name.to_vec());
    }
    vm.try_autoload_class(name);
    vm.lookup_class(&l).map(|c| c.name.to_vec())
}

/// object_init_ex(): a new object of the class with its default properties
/// (no constructor call).
pub fn new_object(vm: &mut Vm, class: &[u8]) -> Obj {
    let l = lower(class);
    let name = vm
        .classes
        .get(&l)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| class.to_vec());
    let id = vm.alloc_object_id();
    let mut o = PhpObject::new(name, id);
    vm.apply_class_property_defaults(&mut o, &l);
    let o = Rc::new(RefCell::new(o));
    goro_core::gc::track_object(&o);
    if vm
        .classes
        .get(&l)
        .is_some_and(|c| c.get_method(b"__destruct").is_some())
    {
        vm.track_destructible(Value::Object(o.clone()));
    }
    o
}

/// A property by its storage name (public properties, deref'd).
pub fn prop(o: &Obj, name: &str) -> Value {
    let b = o.borrow();
    match b
        .properties
        .iter()
        .find(|(k, _)| k.as_slice() == name.as_bytes())
    {
        Some((_, v)) => v.deref(),
        None => Value::Undef,
    }
}

pub fn has_prop(o: &Obj, name: &str) -> bool {
    o.borrow()
        .properties
        .iter()
        .any(|(k, v)| k.as_slice() == name.as_bytes() && !matches!(v, Value::Undef))
}

pub fn set_prop(o: &Obj, name: &str, v: Value) {
    let mut b = o.borrow_mut();
    if let Some(e) = b
        .properties
        .iter_mut()
        .find(|(k, _)| k.as_slice() == name.as_bytes())
    {
        if let Value::Reference(r) = &e.1 {
            *r.borrow_mut() = v;
        } else {
            e.1 = v;
        }
        return;
    }
    b.properties.push((name.as_bytes().to_vec(), v));
}

/// The storage key of a private property declared by `class`.
fn priv_key(o: &Obj, class: &str, name: &str) -> Vec<u8> {
    let mut m = Vec::with_capacity(2 + class.len() + name.len());
    m.push(0);
    m.extend_from_slice(class.as_bytes());
    m.push(0);
    m.extend_from_slice(name.as_bytes());
    if o.borrow().properties.iter().any(|(k, _)| *k == m) {
        m
    } else {
        name.as_bytes().to_vec()
    }
}

/// A private property of SoapClient (OBJ_PROP_NUM).
pub fn cprop(o: &Obj, name: &str) -> Value {
    let k = priv_key(o, "SoapClient", name);
    let b = o.borrow();
    match b.properties.iter().find(|(kk, _)| *kk == k) {
        Some((_, v)) => v.deref(),
        None => Value::Undef,
    }
}

pub fn set_cprop(o: &Obj, name: &str, v: Value) {
    let k = priv_key(o, "SoapClient", name);
    let mut b = o.borrow_mut();
    if let Some(e) = b.properties.iter_mut().find(|(kk, _)| *kk == k) {
        e.1 = v;
        return;
    }
    b.properties.push((k, v));
}

pub fn sprop(o: &Obj, name: &str) -> Value {
    let k = priv_key(o, "SoapServer", name);
    let b = o.borrow();
    match b.properties.iter().find(|(kk, _)| *kk == k) {
        Some((_, v)) => v.deref(),
        None => Value::Undef,
    }
}

pub fn set_sprop(o: &Obj, name: &str, v: Value) {
    let k = priv_key(o, "SoapServer", name);
    let mut b = o.borrow_mut();
    if let Some(e) = b.properties.iter_mut().find(|(kk, _)| *kk == k) {
        e.1 = v;
        return;
    }
    b.properties.push((k, v));
}

/// Whether the class (lowercase name) or an ancestor declares the method.
pub fn class_method_exists(vm: &Vm, class_lower: &[u8], method_lower: &[u8]) -> bool {
    let mut cur = Some(class_lower.to_vec());
    let mut guard = 0;
    while let Some(c) = cur {
        guard += 1;
        if guard > 64 {
            break;
        }
        let Some(ce) = vm.classes.get(&c) else {
            return false;
        };
        if ce.methods.contains_key(method_lower) {
            return true;
        }
        cur = ce.parent.as_ref().map(|p| lower(p));
    }
    false
}

pub fn is_obj(v: &Value) -> Option<Obj> {
    match v.deref() {
        Value::Object(o) => Some(o),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Calls

thread_local! {
    /// A fatal error raised while running SOAP code: the native entry point
    /// returns it.
    pub static PENDING_FATAL: RefCell<Option<VmError>> = const { RefCell::new(None) };
}

pub fn take_pending_fatal() -> Option<VmError> {
    PENDING_FATAL.with(|p| p.borrow_mut().take())
}

/// call_user_function(): Err when it threw or bailed out.
pub fn call_user(vm: &mut Vm, callable: &Value, args: &[Value]) -> SR<Value> {
    match vm.call_callback(callable, args) {
        Ok(v) => {
            if vm.exit_requested {
                return Err(Bail);
            }
            Ok(v)
        }
        Err(e) => {
            // An exception thrown by the callee stays pending; anything
            // else is a fatal error the native entry point returns.
            if vm.current_exception.is_none() {
                PENDING_FATAL.with(|p| *p.borrow_mut() = Some(e));
            }
            Err(Bail)
        }
    }
}

/// A method call on an object.
pub fn call_method(vm: &mut Vm, o: &Obj, method: &str, args: &[Value]) -> SR<Value> {
    let mut a = PhpArray::new();
    a.push(Value::Object(o.clone()));
    a.push(vstr(method));
    call_user(vm, &varr(a), args)
}

// ---------------------------------------------------------------------------
// The SOAP error handler

/// A plain fatal error (the default error callback, then bailout).
pub fn plain_fatal(vm: &mut Vm, msg: &str) -> Bail {
    let line = vm.current_line;
    let e = vm.emit_fatal_at(msg, line);
    PENDING_FATAL.with(|p| *p.borrow_mut() = Some(e));
    Bail
}

/// zend_error(E_ERROR, ...) with the SOAP error handler installed.
pub fn raise_fatal(vm: &mut Vm, msg: &str) -> Bail {
    let (use_handler, eobj, code) = g(|g| {
        (
            g.use_soap_error_handler,
            g.error_object.clone(),
            g.error_code,
        )
    });
    if !use_handler {
        return plain_fatal(vm, msg);
    }
    if let Some(o) = eobj.as_ref().filter(|o| instance_of(vm, o, "SoapClient")) {
        let use_exceptions = !matches!(cprop(o, "_exceptions"), Value::False);
        if use_exceptions {
            let code = code.unwrap_or("Client");
            let fault = add_soap_fault_ex(vm, o, code, Some(msg.as_bytes()), None, None, b"en");
            vm.current_exception = Some(Value::Object(fault));
            return Bail;
        }
        return plain_fatal(vm, msg);
    }
    // Server
    let code = code.unwrap_or("Server");
    let no_errors = eobj.as_ref().is_some_and(|o| {
        instance_of(vm, o, "SoapServer")
            && crate::server::service_of(o).is_some_and(|s| !s.borrow().send_errors)
    });
    let mut outbuf: Option<Value> = None;
    let buffer = if no_errors {
        "Internal Error".to_string()
    } else {
        if vm.ob_level() > 0 {
            if let Some(c) = vm.ob_stack.last() {
                if !c.is_empty() {
                    outbuf = Some(vstr(c.clone()));
                }
            }
            vm.ob_stack_pop(goro_core::ob::OB_POP_DISCARD | goro_core::ob::OB_POP_SILENT);
        }
        msg.to_string()
    };
    let fault = set_soap_fault(
        vm,
        None,
        None,
        Some(code.as_bytes()),
        Some(buffer.as_bytes()),
        None,
        outbuf.as_ref(),
        None,
        b"en",
    );
    // The default handler runs with display_errors off.
    with_display_off(vm, |vm| {
        let (file, line) = (vm.current_file.clone(), vm.current_line);
        vm.fatal_error_cb(E_ERROR, msg, &file, line);
    });
    vm.exit_code = 255;
    crate::server::soap_server_fault_ex(vm, None, &fault, None);
    vm.exit_requested = true;
    Bail
}

fn with_display_off(vm: &mut Vm, f: impl FnOnce(&mut Vm)) {
    let key = b"display_errors".as_slice();
    let saved = vm.constants.get(key).cloned();
    vm.constants.insert(key.to_vec(), vstr("0"));
    f(vm);
    match saved {
        Some(v) => {
            vm.constants.insert(key.to_vec(), v);
        }
        None => {
            vm.constants.remove(key);
        }
    }
}

/// zend_error() of a non-fatal level with the SOAP error handler.
pub fn raise_nonfatal(vm: &mut Vm, level: i64, msg: &str) {
    let (use_handler, eobj, code) = g(|g| {
        (
            g.use_soap_error_handler,
            g.error_object.clone(),
            g.error_code,
        )
    });
    if !use_handler {
        vm.raise_error(level, msg, vm.current_line);
        return;
    }
    if let Some(o) = eobj.as_ref().filter(|o| instance_of(vm, o, "SoapClient")) {
        let use_exceptions = !matches!(cprop(o, "_exceptions"), Value::False);
        // libxml warnings during WSDL parsing are ignored
        if !use_exceptions || code != Some("WSDL") {
            vm.raise_error(level, msg, vm.current_line);
        }
        return;
    }
    with_display_off(vm, |vm| vm.raise_error(level, msg, vm.current_line));
}

/// php_error(level, ...): fatal levels bail out.
pub fn raise(vm: &mut Vm, level: i64, msg: &str) -> SR<()> {
    if level & E_FATAL_ERRORS != 0 {
        return Err(raise_fatal(vm, msg));
    }
    raise_nonfatal(vm, level, msg);
    Ok(())
}

/// soap_error0(E_ERROR, ...): "SOAP-ERROR: ..."
pub fn soap_error(vm: &mut Vm, msg: impl AsRef<str>) -> Bail {
    let m = format!("SOAP-ERROR: {}", msg.as_ref());
    raise_fatal(vm, &m)
}

/// php_error_docref(NULL, E_ERROR, ...) from a method
pub fn docref_fatal(vm: &mut Vm, fname: &str, msg: impl AsRef<str>) -> Bail {
    let m = format!("{}(): {}", fname, msg.as_ref());
    raise_fatal(vm, &m)
}

pub fn docref_warning(vm: &mut Vm, fname: &str, msg: impl AsRef<str>) {
    let m = format!("{}(): {}", fname, msg.as_ref());
    raise_nonfatal(vm, goro_core::errdisplay::E_WARNING, &m);
}

pub fn docref_deprecated(vm: &mut Vm, fname: &str, msg: impl AsRef<str>) {
    let m = format!("{}(): {}", fname, msg.as_ref());
    raise_nonfatal(vm, goro_core::errdisplay::E_DEPRECATED, &m);
}

/// Saved SOAP globals of SOAP_CLIENT_BEGIN_CODE / SOAP_SERVER_BEGIN_CODE.
pub struct CodeGuard {
    handler: bool,
    code: Option<&'static str>,
    obj: Option<Obj>,
    version: i32,
}

pub fn begin_code(this: &Obj, code: &'static str) -> CodeGuard {
    g(|g| {
        let s = CodeGuard {
            handler: g.use_soap_error_handler,
            code: g.error_code,
            obj: g.error_object.clone(),
            version: g.soap_version,
        };
        g.use_soap_error_handler = true;
        g.error_code = Some(code);
        g.error_object = Some(this.clone());
        s
    })
}

pub fn end_code(s: CodeGuard) {
    g(|g| {
        g.use_soap_error_handler = s.handler;
        g.error_code = s.code;
        g.error_object = s.obj;
        g.soap_version = s.version;
    })
}

// ---------------------------------------------------------------------------
// SoapFault

/// object_init_ex(soap_fault_class_entry): an exception with its file,
/// line and trace, and the SoapFault defaults.
pub fn new_soap_fault(vm: &mut Vm) -> Obj {
    let line = vm.current_line;
    let e = vm.create_exception(b"SoapFault", "", line);
    let o = match e {
        Value::Object(o) => o,
        _ => unreachable!(),
    };
    let mut defaults = PhpObject::new(Vec::new(), 0);
    vm.apply_class_property_defaults(&mut defaults, b"soapfault");
    {
        let mut b = o.borrow_mut();
        for (k, v) in defaults.properties.drain(..) {
            if !b.properties.iter().any(|(kk, _)| *kk == k) {
                b.properties.push((k, v));
            }
        }
    }
    goro_core::gc::track_object(&o);
    o
}

/// set_soap_fault()
#[allow(clippy::too_many_arguments)]
pub fn set_soap_fault(
    vm: &mut Vm,
    obj: Option<&Obj>,
    fault_code_ns: Option<&[u8]>,
    fault_code: Option<&[u8]>,
    fault_string: Option<&[u8]>,
    fault_actor: Option<&[u8]>,
    fault_detail: Option<&Value>,
    name: Option<&[u8]>,
    lang: &[u8],
) -> Obj {
    let o = match obj {
        Some(o) => o.clone(),
        None => new_soap_fault(vm),
    };
    let fs = vstr(fault_string.unwrap_or(b""));
    set_prop(&o, "faultstring", fs.clone());
    set_prop(&o, "message", fs);
    if let Some(code) = fault_code {
        if let Some(ns) = fault_code_ns {
            set_prop(&o, "faultcode", vstr(code));
            set_prop(&o, "faultcodens", vstr(ns));
        } else {
            let v = soap_version();
            if v == SOAP_1_1 {
                set_prop(&o, "faultcode", vstr(code));
                if matches!(
                    code,
                    b"Client" | b"Server" | b"VersionMismatch" | b"MustUnderstand"
                ) {
                    set_prop(&o, "faultcodens", vstr(SOAP_1_1_ENV_NAMESPACE));
                }
            } else if v == SOAP_1_2 {
                match code {
                    b"Client" => {
                        set_prop(&o, "faultcode", vstr("Sender"));
                        set_prop(&o, "faultcodens", vstr(SOAP_1_2_ENV_NAMESPACE));
                    }
                    b"Server" => {
                        set_prop(&o, "faultcode", vstr("Receiver"));
                        set_prop(&o, "faultcodens", vstr(SOAP_1_2_ENV_NAMESPACE));
                    }
                    b"VersionMismatch" | b"MustUnderstand" | b"DataEncodingUnknown" => {
                        set_prop(&o, "faultcode", vstr(code));
                        set_prop(&o, "faultcodens", vstr(SOAP_1_2_ENV_NAMESPACE));
                    }
                    _ => set_prop(&o, "faultcode", vstr(code)),
                }
            }
        }
    }
    if let Some(a) = fault_actor {
        set_prop(&o, "faultactor", vstr(a));
    }
    if let Some(d) = fault_detail {
        if !matches!(d, Value::Undef) {
            set_prop(&o, "detail", copy_value(&d.deref()));
        }
    }
    if let Some(n) = name {
        set_prop(&o, "_name", vstr(n));
    }
    set_prop(&o, "lang", vstr(lang));
    o
}

/// add_soap_fault_ex(): a new fault, stored as the client's or server's
/// __soap_fault.
pub fn add_soap_fault_ex(
    vm: &mut Vm,
    target: &Obj,
    code: &str,
    string: Option<&[u8]>,
    actor: Option<&[u8]>,
    detail: Option<&Value>,
    lang: &[u8],
) -> Obj {
    add_soap_fault_opt(
        vm,
        target,
        Some(code.as_bytes()),
        string,
        actor,
        detail,
        lang,
    )
}

/// add_soap_fault_ex() with a possibly NULL fault code.
pub fn add_soap_fault_opt(
    vm: &mut Vm,
    target: &Obj,
    code: Option<&[u8]>,
    string: Option<&[u8]>,
    actor: Option<&[u8]>,
    detail: Option<&Value>,
    lang: &[u8],
) -> Obj {
    let f = set_soap_fault(vm, None, None, code, string, actor, detail, None, lang);
    if instance_of(vm, target, "SoapClient") {
        set_cprop(target, "__soap_fault", Value::Object(f.clone()));
    } else if instance_of(vm, target, "SoapServer") {
        set_sprop(target, "__soap_fault", Value::Object(f.clone()));
    }
    f
}

pub fn add_soap_fault(vm: &mut Vm, target: &Obj, code: &str, string: &str) {
    add_soap_fault_ex(vm, target, code, Some(string.as_bytes()), None, None, b"en");
}
