//! The System V IPC extensions of php-src: ext/shmop, ext/sysvsem,
//! ext/sysvshm and ext/sysvmsg, each registered as its own extension.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use goro_core::object::{ClassEntry, NativeData, PhpObject};
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm, VmError};

mod shmop;
mod sysvmsg;
mod sysvsem;
mod sysvshm;
mod zpp;

type R = Result<Value, VmError>;

/// MINIT of the four extensions.
pub fn register(vm: &mut Vm) {
    shmop::register(vm);
    sysvsem::register(vm);
    sysvshm::register(vm);
    sysvmsg::register(vm);
}

/// zend_objects_not_comparable
fn not_comparable(a: &Value, b: &Value) -> Option<i64> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) if Rc::ptr_eq(x, y) => Some(0),
        (Value::Object(_), Value::Object(_)) => Some(1),
        _ => None,
    }
}

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    compare: Some(not_comparable),
    ..NativeObjectHandlers::EMPTY
};

thread_local! {
    static CTOR_MSGS: RefCell<Vec<(Vec<u8>, &'static str)>> = const { RefCell::new(Vec::new()) };
}

/// Register an extension: its functions (arity checked from the stubs)
/// and integer constants.
fn register_ext(
    vm: &mut Vm,
    name: &[u8],
    functions: &[(&str, goro_core::vm::BuiltinFn)],
    constants: &[(&str, i64)],
) -> bool {
    if vm.loaded_extensions.contains(name) {
        return false;
    }
    vm.register_extension(name);
    for &(fname, f) in functions {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(fname.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(fname.as_bytes(), f, &params);
        vm.zpp_extension_verified
            .insert(fname.as_bytes().to_vec(), goro_core::arginfo::CHECK_ARITY);
    }
    for &(cname, v) in constants {
        vm.constants
            .insert(cname.as_bytes().to_vec(), Value::Long(v));
    }
    if !constants.is_empty() {
        vm.module_constants.push((
            name.to_vec(),
            constants
                .iter()
                .map(|(n, _)| n.as_bytes().to_vec())
                .collect(),
        ));
    }
    true
}

/// A final class that cannot be constructed (get_constructor throws).
fn register_class(vm: &mut Vm, name: &[u8], ctor_msg: &'static str) {
    let mut ce = match goro_core::stubs::class_entry(vm, name) {
        Some(stub) => stub.clone(),
        None => ClassEntry::new(name.to_vec()),
    };
    ce.is_final = true;
    let lower = name.to_ascii_lowercase();
    vm.classes.insert(lower.clone(), ce);
    CTOR_MSGS.with(|m| m.borrow_mut().push((lower.clone(), ctor_msg)));
    vm.native_class_handlers.insert(
        lower,
        goro_core::vm::NativeClassHandler {
            has_method: |_c, m, _s| m.eq_ignore_ascii_case(b"__construct"),
            call: ctor_call,
            prop_added: None,
            prop_write: None,
            compare_check: None,
            prop_read: None,
            param_names: None,
        },
    );
}

fn ctor_call(
    vm: &mut Vm,
    class: &[u8],
    method: &[u8],
    _called: &[u8],
    _this: Option<&Value>,
    _args: &[Value],
) -> Value {
    if method.eq_ignore_ascii_case(b"__construct") {
        let lower = class.to_ascii_lowercase();
        let msg = CTOR_MSGS.with(|m| {
            m.borrow()
                .iter()
                .find(|(c, _)| *c == lower)
                .map(|(_, s)| *s)
        });
        if let Some(msg) = msg {
            zpp::throw(vm, b"Error", msg);
        }
    }
    Value::Null
}

/// object_init_ex() of `class` holding `data`.
fn new_object<T: Any>(vm: &mut Vm, class: &[u8], data: T) -> Value {
    let id = vm.next_object_id();
    let mut obj = PhpObject::new(class.to_vec(), id);
    obj.native = Some(NativeData {
        data: Rc::new(data) as Rc<dyn Any>,
        handlers: &HANDLERS,
    });
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// The native state of an object argument.
fn native<T: Any>(a: &zpp::Arg) -> Option<Rc<T>> {
    let zpp::Arg::Obj(o) = a else {
        return None;
    };
    let d = o.borrow().native.as_ref().map(|n| n.data.clone())?;
    d.downcast::<T>().ok()
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn strerror(e: i32) -> String {
    unsafe { std::ffi::CStr::from_ptr(libc::strerror(e)) }
        .to_string_lossy()
        .into_owned()
}

fn warn(vm: &mut Vm, func: &str, msg: &str) {
    vm.emit_warning(&format!("{}(): {}", func, msg));
}

fn bval(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

/// ZEND_TRY_ASSIGN_REF_*()
fn assign_ref(target: &Value, v: Value) {
    if let Value::Reference(r) = target {
        *r.borrow_mut() = v;
    }
}

/// ZEND_XLONG_FMT
fn xlong(v: i64) -> String {
    format!("{:x}", v as u64)
}
