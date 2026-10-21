//! Userland output-control functions (main/output.c PHP_FUNCTIONs).

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::ob::*;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

pub fn register(vm: &mut Vm) {
    vm.register_function(b"ob_start", ob_start);
    vm.register_function(b"ob_flush", ob_flush);
    vm.register_function(b"ob_clean", ob_clean);
    vm.register_function(b"ob_end_flush", ob_end_flush);
    vm.register_function(b"ob_end_clean", ob_end_clean);
    vm.register_function(b"ob_get_flush", ob_get_flush);
    vm.register_function(b"ob_get_clean", ob_get_clean);
    vm.register_function(b"ob_get_contents", ob_get_contents);
    vm.register_function(b"ob_get_level", ob_get_level);
    vm.register_function(b"ob_get_length", ob_get_length);
    vm.register_function(b"ob_list_handlers", ob_list_handlers);
    vm.register_function(b"ob_get_status", ob_get_status);
    vm.register_function(b"ob_implicit_flush", ob_implicit_flush);
    for (name, v) in [
        (&b"PHP_OUTPUT_HANDLER_CLEANABLE"[..], OBH_CLEANABLE),
        (b"PHP_OUTPUT_HANDLER_FLUSHABLE", OBH_FLUSHABLE),
        (b"PHP_OUTPUT_HANDLER_REMOVABLE", OBH_REMOVABLE),
        (b"PHP_OUTPUT_HANDLER_STDFLAGS", OBH_STDFLAGS),
        (b"PHP_OUTPUT_HANDLER_STARTED", OBH_STARTED),
        (b"PHP_OUTPUT_HANDLER_DISABLED", OBH_DISABLED),
        (b"PHP_OUTPUT_HANDLER_PROCESSED", OBH_PROCESSED),
    ] {
        vm.constants.insert(name.to_vec(), Value::Long(v));
    }
}

/// php_output_handler_alias_ctor_t: builds the internal handler an alias
/// name stands for (name, chunk_size, flags).
pub type ObAliasCtor = fn(&mut Vm, &[u8], usize, i64) -> Option<ObHandler>;

/// php_output_handler_conflict_check_t: Ok to start handler `name`?
pub type ObConflictCheck = fn(&mut Vm, &[u8]) -> bool;

thread_local! {
    /// php_output_handler_aliases
    static OB_ALIASES: RefCell<Vec<(Vec<u8>, ObAliasCtor)>> = const { RefCell::new(Vec::new()) };
    /// php_output_handler_conflicts
    static OB_CONFLICTS: RefCell<Vec<(Vec<u8>, ObConflictCheck)>> = const { RefCell::new(Vec::new()) };
}

/// php_output_handler_alias_register()
pub fn register_output_handler_alias(name: &[u8], ctor: ObAliasCtor) {
    OB_ALIASES.with(|a| a.borrow_mut().push((name.to_vec(), ctor)));
}

/// php_output_handler_conflict_register()
pub fn register_output_handler_conflict(name: &[u8], check: ObConflictCheck) {
    OB_CONFLICTS.with(|a| a.borrow_mut().push((name.to_vec(), check)));
}

fn output_handler_alias(name: &[u8]) -> Option<ObAliasCtor> {
    OB_ALIASES.with(|a| {
        a.borrow()
            .iter()
            .find(|(n, _)| n.as_slice() == name)
            .map(|(_, f)| *f)
    })
}

/// php_output_handler_start(): run the conflict checks, then start.
pub fn output_handler_start(vm: &mut Vm, handler: Option<ObHandler>) -> bool {
    if let Some(h) = &handler {
        let check = OB_CONFLICTS.with(|a| {
            a.borrow()
                .iter()
                .find(|(n, _)| n.as_slice() == h.name.as_slice())
                .map(|(_, f)| *f)
        });
        if let Some(check) = check {
            let name = h.name.clone();
            if !check(vm, &name) {
                return false;
            }
        }
    }
    vm.ob_handler_start(handler)
}

/// php_output_handler_create_user() for a callable or alias name.
fn create_user_handler(
    vm: &mut Vm,
    cb: &Value,
    chunk: usize,
    flags: i64,
) -> Result<Option<ObHandler>, String> {
    if let Value::String(n) = cb {
        if let Some(ctor) = output_handler_alias(n.as_bytes()) {
            return Ok(ctor(vm, n.as_bytes(), chunk, flags));
        }
    }
    let name = ob_callable_name(vm, cb)?;
    Ok(Some(Vm::ob_make_handler(
        name,
        ObKind::User(cb.clone()),
        chunk,
        flags,
    )))
}

/// php_request_startup(): start the `output_handler` INI handler, or a
/// default buffer when `output_buffering` is enabled.
pub fn request_startup(vm: &mut Vm) {
    let ini = |vm: &Vm, k: &[u8]| {
        vm.constants
            .get(k)
            .map(|v| v.to_php_string().to_string_lossy())
    };
    let handler = ini(vm, b"output_handler").unwrap_or_default();
    if !handler.is_empty() {
        let cb = Value::String(PhpString::from_string(handler.clone()));
        let h = match create_user_handler(vm, &cb, 0, OBH_STDFLAGS) {
            Ok(h) => h,
            Err(e) => {
                vm.write_output(
                    format!("\nWarning: (null)(): {} in Unknown on line 0\n", e).as_bytes(),
                );
                None
            }
        };
        output_handler_start(vm, h);
        return;
    }
    let ob = ini(vm, b"output_buffering").unwrap_or_default();
    let l = ob.trim().to_ascii_lowercase();
    let size: i64 = match l.as_str() {
        "on" | "yes" | "true" => 1,
        _ => {
            let (v, _) = goro_core::ini_quantity::parse_quantity(l.as_bytes(), true);
            v as i64
        }
    };
    if size > 0 {
        let chunk = if size > 1 { size as usize } else { 0 };
        let h = Vm::ob_make_handler(
            b"default output handler".to_vec(),
            ObKind::Default,
            chunk,
            OBH_STDFLAGS,
        );
        vm.ob_handler_start(Some(h));
    }
}

fn s(b: &[u8]) -> ArrayKey {
    ArrayKey::String(PhpString::from_bytes(b))
}

/// Run `f` with `vm.ob_fname` set so output-layer diagnostics carry the
/// right "fn(): " prefix, then surface any fatal raised inside the layer
/// (or an exception thrown by a user handler).
fn with_fname<F: FnOnce(&mut Vm) -> Value>(
    vm: &mut Vm,
    name: &str,
    f: F,
) -> Result<Value, VmError> {
    let prev = vm.ob_fname.replace(name.to_string());
    let r = f(vm);
    vm.ob_fname = prev;
    if let Some(e) = vm.ob_take_pending_fatal() {
        return Err(e);
    }
    if vm.current_exception.is_some() {
        return Err(VmError {
            message: "exception in output handler".into(),
            line: vm.current_line,
        });
    }
    Ok(r)
}

fn lower(b: &[u8]) -> Vec<u8> {
    b.iter().map(|c| c.to_ascii_lowercase()).collect()
}

fn is_closure_name(b: &[u8]) -> bool {
    b.starts_with(b"__closure_")
        || b.starts_with(b"__arrow_")
        || b.starts_with(b"__bound_closure_")
        || b.starts_with(b"__closure_fcc_")
}

fn closure_display(vm: &Vm, fname: &[u8]) -> Vec<u8> {
    if let Some(uf) = vm.user_functions.get(&lower(fname)) {
        if !uf.display_name.is_empty() {
            return uf.display_name.clone();
        }
        return format!(
            "{{closure:{}:{}}}",
            String::from_utf8_lossy(&uf.filename),
            uf.decl_line
        )
        .into_bytes();
    }
    b"Closure::__invoke".to_vec()
}

/// Resolve the callable name for ob_start (zend_fcall_info_init), or the
/// zend_is_callable error text.
fn ob_callable_name(vm: &mut Vm, cb: &Value) -> Result<Vec<u8>, String> {
    match cb {
        Value::Reference(r) => {
            let inner = r.borrow().clone();
            ob_callable_name(vm, &inner)
        }
        Value::String(st) => {
            let name = st.as_bytes();
            if is_closure_name(name) {
                return Ok(closure_display(vm, name));
            }
            let stripped = name.strip_prefix(b"\\").unwrap_or(name);
            if let Some(pos) = stripped.windows(2).position(|w| w == b"::") {
                let cls = &stripped[..pos];
                let meth = &stripped[pos + 2..];
                return check_method(vm, cls, meth, None, name);
            }
            let l = lower(stripped);
            if vm.functions.contains_key(&l) || vm.user_functions.contains_key(&l) {
                Ok(name.to_vec())
            } else {
                Err(format!(
                    "function \"{}\" not found or invalid function name",
                    String::from_utf8_lossy(name)
                ))
            }
        }
        Value::Array(arr) => {
            let vals: Vec<Value> = arr.borrow().values().cloned().collect();
            if let Some(Value::String(f)) = vals.first() {
                if is_closure_name(f.as_bytes()) {
                    return Ok(closure_display(vm, f.as_bytes()));
                }
            }
            if vals.len() != 2 {
                return Err("array callback must have exactly two members".into());
            }
            let meth = match vals[1].deref() {
                Value::String(m) => m.as_bytes().to_vec(),
                _ => return Err("second array member is not a valid method".into()),
            };
            match vals[0].deref() {
                Value::Object(o) => {
                    let cn = o.borrow().class_name.clone();
                    let mut full = cn.clone();
                    full.extend_from_slice(b"::");
                    full.extend_from_slice(&meth);
                    check_method(vm, &cn, &meth, Some(()), &full)
                }
                Value::String(c) => {
                    let cn = c.as_bytes().to_vec();
                    let mut full = cn.clone();
                    full.extend_from_slice(b"::");
                    full.extend_from_slice(&meth);
                    check_method(vm, &cn, &meth, None, &full)
                }
                _ => Err("first array member is not a valid class name or object".into()),
            }
        }
        Value::Object(o) => {
            let cn = o.borrow().class_name.clone();
            let cl = lower(&cn);
            if cl == b"closure" {
                return Ok(vm.closure_function_name(cb));
            }
            if vm
                .classes
                .get(&cl)
                .and_then(|c| c.get_method(b"__invoke"))
                .is_some()
            {
                let mut n = cn;
                n.extend_from_slice(b"::__invoke");
                Ok(n)
            } else {
                Err("no array or string given".into())
            }
        }
        _ => Err("no array or string given".into()),
    }
}

fn check_method(
    vm: &mut Vm,
    cls: &[u8],
    meth: &[u8],
    has_obj: Option<()>,
    full: &[u8],
) -> Result<Vec<u8>, String> {
    let mut cl = lower(cls);
    if cl.first() == Some(&b'\\') {
        cl.remove(0);
    }
    if !vm.classes.contains_key(&cl) {
        vm.try_autoload_class(cls);
    }
    let cls_disp = vm.classes.get(&cl).map(|c| c.name.clone());
    let cls_disp = match cls_disp {
        Some(n) => n,
        None => {
            return Err(format!(
                "class \"{}\" not found",
                String::from_utf8_lossy(cls)
            ));
        }
    };
    let ml = lower(meth);
    let ce = vm.classes.get(&cl).unwrap();
    match ce.get_method(&ml) {
        Some(m) => {
            if has_obj.is_none() && !m.is_static {
                return Err(format!(
                    "non-static method {}::{}() cannot be called statically",
                    String::from_utf8_lossy(&cls_disp),
                    String::from_utf8_lossy(&m.name)
                ));
            }
            Ok(full.to_vec())
        }
        None => {
            if (has_obj.is_some() && ce.get_method(b"__call").is_some())
                || ce.get_method(b"__callstatic").is_some()
            {
                return Ok(full.to_vec());
            }
            Err(format!(
                "class {} does not have a method \"{}\"",
                String::from_utf8_lossy(&cls_disp),
                String::from_utf8_lossy(meth)
            ))
        }
    }
}

fn notice(vm: &mut Vm, msg: &str) {
    let line = vm.current_line;
    vm.emit_notice_at(msg, line);
}

fn ob_start(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cb = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    let mut chunk = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let flags = args.get(2).map(|v| v.to_long()).unwrap_or(OBH_STDFLAGS);
    if chunk < 0 {
        chunk = 0;
    }
    let chunk = chunk as usize;
    with_fname(vm, "ob_start", |vm| {
        let handler = match &cb {
            Value::Null => Some(Vm::ob_make_handler(
                b"default output handler".to_vec(),
                ObKind::Default,
                chunk,
                flags,
            )),
            _ => match create_user_handler(vm, &cb, chunk, flags) {
                Ok(h) => h,
                Err(e) => {
                    vm.emit_warning(&format!("ob_start(): {}", e));
                    None
                }
            },
        };
        let ok = output_handler_start(vm, handler);
        if vm.ob_pending_fatal.is_some() {
            return Value::False;
        }
        if !ok {
            notice(vm, "ob_start(): Failed to create buffer");
            return Value::False;
        }
        Value::True
    })
}

fn ob_flush(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    with_fname(vm, "ob_flush", |vm| {
        if vm.ob_level() == 0 {
            notice(vm, "ob_flush(): Failed to flush buffer. No buffer to flush");
            return Value::False;
        }
        if !vm.ob_flush_active() {
            if vm.ob_level() > 0 {
                let name = String::from_utf8_lossy(&vm.ob_active_name()).to_string();
                let lvl = vm.ob_level() - 1;
                notice(
                    vm,
                    &format!("ob_flush(): Failed to flush buffer of {} ({})", name, lvl),
                );
            }
            return Value::False;
        }
        Value::True
    })
}

fn ob_clean(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    with_fname(vm, "ob_clean", |vm| {
        if vm.ob_level() == 0 {
            notice(
                vm,
                "ob_clean(): Failed to delete buffer. No buffer to delete",
            );
            return Value::False;
        }
        if !vm.ob_clean_active() {
            if vm.ob_level() > 0 {
                let name = String::from_utf8_lossy(&vm.ob_active_name()).to_string();
                let lvl = vm.ob_level() - 1;
                notice(
                    vm,
                    &format!("ob_clean(): Failed to delete buffer of {} ({})", name, lvl),
                );
            }
            return Value::False;
        }
        Value::True
    })
}

fn ob_end_flush(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    with_fname(vm, "ob_end_flush", |vm| {
        if vm.ob_level() == 0 {
            notice(
                vm,
                "ob_end_flush(): Failed to delete and flush buffer. No buffer to delete or flush",
            );
            return Value::False;
        }
        if vm.ob_stack_pop(OB_POP_TRY) {
            Value::True
        } else {
            Value::False
        }
    })
}

fn ob_end_clean(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    with_fname(vm, "ob_end_clean", |vm| {
        if vm.ob_level() == 0 {
            notice(
                vm,
                "ob_end_clean(): Failed to delete buffer. No buffer to delete",
            );
            return Value::False;
        }
        if vm.ob_stack_pop(OB_POP_DISCARD | OB_POP_TRY) {
            Value::True
        } else {
            Value::False
        }
    })
}

fn contents(vm: &mut Vm) -> Option<Vec<u8>> {
    if vm.ob_level() == 0 {
        return None;
    }
    vm.ob_stack.last().cloned()
}

fn ob_get_flush(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    with_fname(vm, "ob_get_flush", |vm| {
        let c = match contents(vm) {
            Some(c) => c,
            None => {
                notice(
                    vm,
                    "ob_get_flush(): Failed to delete and flush buffer. No buffer to delete or flush",
                );
                return Value::False;
            }
        };
        if !vm.ob_stack_pop(OB_POP_TRY) && vm.ob_level() > 0 {
            let name = String::from_utf8_lossy(&vm.ob_active_name()).to_string();
            let lvl = vm.ob_level() - 1;
            notice(
                vm,
                &format!(
                    "ob_get_flush(): Failed to delete buffer of {} ({})",
                    name, lvl
                ),
            );
        }
        Value::String(PhpString::from_vec(c))
    })
}

fn ob_get_clean(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    with_fname(vm, "ob_get_clean", |vm| {
        let c = match contents(vm) {
            Some(c) => c,
            None => return Value::False,
        };
        if !vm.ob_stack_pop(OB_POP_DISCARD | OB_POP_TRY) && vm.ob_level() > 0 {
            let name = String::from_utf8_lossy(&vm.ob_active_name()).to_string();
            let lvl = vm.ob_level() - 1;
            notice(
                vm,
                &format!(
                    "ob_get_clean(): Failed to delete buffer of {} ({})",
                    name, lvl
                ),
            );
        }
        Value::String(PhpString::from_vec(c))
    })
}

fn ob_get_contents(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(match contents(vm) {
        Some(c) => Value::String(PhpString::from_vec(c)),
        None => Value::False,
    })
}

fn ob_get_level(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(vm.ob_level() as i64))
}

fn ob_get_length(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(match contents(vm) {
        Some(c) => Value::Long(c.len() as i64),
        None => Value::False,
    })
}

fn ob_list_handlers(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    for n in vm.ob_handler_names() {
        arr.push(Value::String(PhpString::from_vec(n)));
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn status_array(vm: &mut Vm, level: usize) -> Value {
    let mut e = PhpArray::new();
    if let Some((name, ty, flags, lvl, chunk, size, used)) = vm.ob_status_entry(level) {
        e.set(s(b"name"), Value::String(PhpString::from_vec(name)));
        e.set(s(b"type"), Value::Long(ty));
        e.set(s(b"flags"), Value::Long(flags));
        e.set(s(b"level"), Value::Long(lvl));
        e.set(s(b"chunk_size"), Value::Long(chunk));
        e.set(s(b"buffer_size"), Value::Long(size));
        e.set(s(b"buffer_used"), Value::Long(used));
    }
    Value::Array(Rc::new(RefCell::new(e)))
}

fn ob_get_status(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let full = args.first().map(|v| v.to_bool()).unwrap_or(false);
    let n = vm.ob_level();
    if n == 0 {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    if full {
        let mut arr = PhpArray::new();
        for i in 0..n {
            arr.push(status_array(vm, i));
        }
        Ok(Value::Array(Rc::new(RefCell::new(arr))))
    } else {
        Ok(status_array(vm, n - 1))
    }
}

fn ob_implicit_flush(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}
