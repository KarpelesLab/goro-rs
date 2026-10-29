//! User save handlers (PHP manual: session_set_save_handler(),
//! SessionHandlerInterface, SessionIdInterface,
//! SessionUpdateTimestampHandlerInterface) and SessionHandler, which
//! calls the module that was in use before (the files handler).

use goro_runtime::{ObjRef, PhpArray, PhpStr, Value};

use super::{Mod, files, st, warn};
use crate::func::Args;
use crate::vm::{R, Vm};

pub(super) const OPEN: usize = 0;
pub(super) const CLOSE: usize = 1;
pub(super) const READ: usize = 2;
pub(super) const WRITE: usize = 3;
pub(super) const DESTROY: usize = 4;
pub(super) const GC: usize = 5;
pub(super) const CREATE: usize = 6;
pub(super) const VALIDATE: usize = 7;
pub(super) const UPDATE: usize = 8;

/// The callbacks' parameter names (in session_set_save_handler()'s
/// messages) and the handler object's method names.
const PARAMS: [&str; 9] = [
    "open",
    "close",
    "read",
    "write",
    "destroy",
    "gc",
    "create_sid",
    "validate_sid",
    "update_timestamp",
];
const METHOD_NAMES: [&str; 9] = [
    "open",
    "close",
    "read",
    "write",
    "destroy",
    "gc",
    "create_sid",
    "validateId",
    "updateTimestamp",
];

pub(super) static CLASSES: &[(&str, &str)] = &[
    (
        "sessionhandlerinterface",
        "<?php interface SessionHandlerInterface {
    public function open(string $path, string $name): bool;
    public function close(): bool;
    public function read(string $id): string|false;
    public function write(string $id, string $data): bool;
    public function destroy(string $id): bool;
    public function gc(int $max_lifetime): int|false;
}",
    ),
    (
        "sessionidinterface",
        "<?php interface SessionIdInterface { public function create_sid(): string; }",
    ),
    (
        "sessionupdatetimestamphandlerinterface",
        "<?php interface SessionUpdateTimestampHandlerInterface {
    public function validateId(string $id): bool;
    public function updateTimestamp(string $id, string $data): bool;
}",
    ),
    (
        "sessionhandler",
        "<?php class SessionHandler implements SessionHandlerInterface, SessionIdInterface {
    public function open(string $path, string $name): bool {}
    public function close(): bool {}
    public function read(string $id): string|false {}
    public function write(string $id, string $data): bool {}
    public function destroy(string $id): bool {}
    public function gc(int $max_lifetime): int|false {}
    public function create_sid(): string {}
}",
    ),
];

pub(super) static METHODS: &[(&str, crate::func::NativeFn)] = &[
    ("sessionhandler::open", sh_open),
    ("sessionhandler::close", sh_close),
    ("sessionhandler::read", sh_read),
    ("sessionhandler::write", sh_write),
    ("sessionhandler::destroy", sh_destroy),
    ("sessionhandler::gc", sh_gc),
    ("sessionhandler::create_sid", sh_create_sid),
];

/// A save handler set with session_set_save_handler().
#[derive(Clone)]
pub struct UserHandler {
    /// The handler object (the SessionHandlerInterface form).
    obj: Option<ObjRef>,
    cbs: Vec<Option<Value>>,
}

impl UserHandler {
    pub(super) fn has(&self, i: usize) -> bool {
        self.cbs.get(i).is_some_and(|c| c.is_some())
    }
}

/// The handler's name in "Failed to write session data" messages.
pub(super) fn callback_name(vm: &mut Vm, i: usize) -> String {
    let obj = st(vm).user.as_ref().and_then(|u| u.obj.clone());
    match obj {
        Some(o) => format!(
            "{}::{}",
            vm.class_of(&o).name.to_string_lossy(),
            METHOD_NAMES[i]
        ),
        None => PARAMS[i].to_string(),
    }
}

fn call(vm: &mut Vm, i: usize, args: Vec<Value>) -> R<Option<Value>> {
    let cb = st(vm).user.as_ref().and_then(|u| u.cbs.get(i).cloned().flatten());
    match cb {
        Some(cb) => Ok(Some(vm.call_value(&cb, args)?.into_deref())),
        None => Ok(None),
    }
}

/// A callback's bool result: 0 and -1 (the old SUCCESS and FAILURE) are
/// deprecated, anything else is a TypeError (observed on PHP 8.5; tests
/// session_set_save_handler_type_error*.phpt).
fn bool_ret(vm: &mut Vm, v: Value) -> R<bool> {
    match v {
        Value::Bool(b) => Ok(b),
        Value::Int(n @ (0 | -1)) => {
            super::deprecated(
                vm,
                "Session callback must have a return value of type bool, int returned",
            )?;
            Ok(n == 0)
        }
        other => vm.type_error(&format!(
            "Session callback must have a return value of type bool, {} returned",
            crate::class::TypeName::tname(&other)
        )),
    }
}

fn s(b: &[u8]) -> Value {
    Value::Str(PhpStr::from(b.to_vec()))
}

pub(super) fn open(vm: &mut Vm, path: &[u8], name: &[u8]) -> R<bool> {
    match call(vm, OPEN, vec![s(path), s(name)])? {
        Some(v) => bool_ret(vm, v),
        None => Ok(false),
    }
}

pub(super) fn close(vm: &mut Vm) -> R<bool> {
    match call(vm, CLOSE, Vec::new())? {
        Some(v) => bool_ret(vm, v),
        None => Ok(false),
    }
}

pub(super) fn read(vm: &mut Vm, id: &[u8]) -> R<Option<Vec<u8>>> {
    Ok(match call(vm, READ, vec![s(id)])? {
        Some(Value::Str(d)) => Some(d.to_vec()),
        _ => None,
    })
}

pub(super) fn write(vm: &mut Vm, id: &[u8], data: &[u8]) -> R<bool> {
    match call(vm, WRITE, vec![s(id), s(data)])? {
        Some(v) => bool_ret(vm, v),
        None => Ok(false),
    }
}

pub(super) fn destroy(vm: &mut Vm, id: &[u8]) -> R<bool> {
    match call(vm, DESTROY, vec![s(id)])? {
        Some(v) => bool_ret(vm, v),
        None => Ok(false),
    }
}

/// gc(): the number of deleted sessions; true counts as 1 (observed on
/// PHP 8.5).
pub(super) fn gc(vm: &mut Vm, maxlifetime: i64) -> R<Option<i64>> {
    Ok(match call(vm, GC, vec![Value::Int(maxlifetime)])? {
        Some(Value::Int(n)) => Some(n),
        Some(Value::Bool(true)) => Some(1),
        _ => None,
    })
}

pub(super) fn create_sid(vm: &mut Vm) -> R<Option<PhpStr>> {
    match call(vm, CREATE, Vec::new())? {
        None => Ok(Some(super::new_id(vm))),
        Some(Value::Str(id)) if !id.is_empty() => Ok(Some(id)),
        Some(Value::Str(_)) => Ok(None),
        Some(_) => vm.throw("Error", "Session id must be a string"),
    }
}

pub(super) fn validate_sid(vm: &mut Vm, id: &[u8]) -> R<bool> {
    match call(vm, VALIDATE, vec![s(id)])? {
        Some(v) => bool_ret(vm, v),
        None => Ok(true),
    }
}

pub(super) fn update_timestamp(vm: &mut Vm, id: &[u8], data: &[u8]) -> R<bool> {
    match call(vm, UPDATE, vec![s(id), s(data)])? {
        Some(v) => bool_ret(vm, v),
        None => Ok(false),
    }
}

// ---------------------------------------------------------------- session_set_save_handler

pub(super) fn session_set_save_handler(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let first = a.val(0);
    let object_form = a.len() <= 2
        && matches!(&first, Value::Object(o) if vm.class_of(o).name.as_bytes() != b"Closure");
    let handler = if object_form {
        let Value::Object(o) = first else {
            unreachable!()
        };
        let cls = vm.class_of(&o);
        let iface = vm.find_class(b"SessionHandlerInterface", false)?;
        if !iface.is_some_and(|i| cls.is_subclass_of(&i)) {
            return vm.fthrow(
                a,
                "TypeError",
                &format!(
                    "Argument #1 ($open) must be of type SessionHandlerInterface, {} given",
                    cls.name.to_string_lossy()
                ),
            );
        }
        let mut cbs = Vec::new();
        for (i, m) in METHOD_NAMES.iter().enumerate() {
            let lm = crate::class::lower(m.as_bytes());
            let present = i < CREATE || cls.find_method_str(&lm).is_some();
            cbs.push(present.then(|| {
                Value::Array(PhpArray::from_values([
                    Value::Object(o.clone()),
                    Value::Str(PhpStr::from(*m)),
                ]))
            }));
        }
        UserHandler {
            obj: Some(o),
            cbs,
        }
    } else {
        vm.fdeprecated(
            a,
            "Providing individual callbacks instead of an object implementing SessionHandlerInterface is deprecated",
        )?;
        let mut cbs = Vec::new();
        for (i, p) in PARAMS.iter().enumerate() {
            let v = a.val(i);
            if i >= CREATE && matches!(v, Value::Null) {
                cbs.push(None);
                continue;
            }
            if i >= a.len() {
                cbs.push(None);
                continue;
            }
            if !vm.is_callable_value(&v, false) {
                let why = vm.callable_error(&v);
                return vm.fthrow(
                    a,
                    "TypeError",
                    &format!("Argument #{} (${}) must be a valid callback, {}", i + 1, p, why),
                );
            }
            cbs.push(Some(v));
        }
        UserHandler { obj: None, cbs }
    };
    if super::active_refusal(vm, "Session save handler cannot be changed")? {
        return Ok(Value::Bool(false));
    }
    if super::sent_refusal(vm, "Session save handler cannot be changed")? {
        return Ok(Value::Bool(false));
    }
    let cur = super::ini_str(vm, "session.save_handler");
    let default_files = match &cur[..] {
        b"files" => Some(true),
        b"user" => st(vm).default_files,
        _ => Some(false),
    };
    {
        let s = st(vm);
        s.user = Some(handler);
        s.default_files = default_files;
    }
    super::ini::store(vm, b"session.save_handler", b"user");
    if object_form && a.bool_or(1, true) {
        super::register_shutdown(vm);
    }
    Ok(Value::Bool(true))
}

// ---------------------------------------------------------------- SessionHandler

/// SessionHandler's methods call the module in use before the user
/// handler; only the files handler can be called (test
/// session_set_save_handler_error5.phpt).
fn default_check(vm: &mut Vm) -> R<()> {
    let files = match st(vm).default_files {
        Some(f) => f,
        None => super::module_of(vm) == Some(Mod::Files),
    };
    if !files {
        return vm.throw("Error", "Cannot call default session handler");
    }
    Ok(())
}

/// The other methods need SessionHandler::open() first.
fn open_check(vm: &mut Vm) -> R<bool> {
    default_check(vm)?;
    if !st(vm).parent_open {
        warn(vm, "Parent session handler is not open")?;
        return Ok(false);
    }
    Ok(true)
}

fn sh_open(vm: &mut Vm, a: &mut Args) -> R<Value> {
    default_check(vm)?;
    let path = a.str(0);
    let ok = files::open(vm, &path)?;
    if ok {
        st(vm).parent_open = true;
    }
    Ok(Value::Bool(ok))
}

fn sh_close(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if !open_check(vm)? {
        return Ok(Value::Bool(false));
    }
    st(vm).parent_open = false;
    Ok(Value::Bool(files::close(vm)))
}

fn sh_read(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !open_check(vm)? {
        return Ok(Value::Bool(false));
    }
    let id = a.str(0);
    Ok(match files::read(vm, &id)? {
        Some(d) => Value::Str(PhpStr::from(d)),
        None => Value::Bool(false),
    })
}

fn sh_write(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !open_check(vm)? {
        return Ok(Value::Bool(false));
    }
    let id = a.str(0);
    let data = a.str(1);
    Ok(Value::Bool(files::write(vm, &id, &data)?))
}

fn sh_destroy(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !open_check(vm)? {
        return Ok(Value::Bool(false));
    }
    let id = a.str(0);
    Ok(Value::Bool(files::destroy(vm, &id)))
}

fn sh_gc(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !open_check(vm)? {
        return Ok(Value::Bool(false));
    }
    let life = a.int(0);
    Ok(match files::gc(vm, life) {
        Some(n) => Value::Int(n),
        None => Value::Bool(false),
    })
}

fn sh_create_sid(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    default_check(vm)?;
    Ok(Value::Str(super::new_id(vm)))
}
