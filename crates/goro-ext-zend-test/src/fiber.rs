//! _ZendTestFiber (ext/zend_test/fiber.c): a fiber implementation of its
//! own, built on the engine's fiber contexts (custom fiber contexts of the
//! VM, see goro_core::custom_fiber). Its `suspend()` and `Fiber::suspend()`
//! each switch out of the innermost context of their own kind.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goro_core::custom_fiber::CustomSwitch;
use goro_core::fiber::FiberResume;
use goro_core::object::{ClassEntry, PhpObject};
use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, Vm};

const CLASS: &[u8] = b"_ZendTestFiber";
const CLASS_LOWER: &[u8] = b"_zendtestfiber";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Init,
    Running,
    Suspended,
    Dead,
}

struct TestFiber {
    callback: Value,
    status: Status,
    /// ZEND_FIBER_FLAG_DESTROYED
    destroyed: bool,
    /// The fiber this one is the target of (`previous` is set).
    is_target: bool,
    /// pipeTo(): the fiber run with this one's result once it returns.
    target: Option<Rc<RefCell<PhpObject>>>,
}

thread_local! {
    static FIBERS: RefCell<HashMap<u64, TestFiber>> = RefCell::new(HashMap::new());
}

fn with<R>(id: u64, f: impl FnOnce(&mut TestFiber) -> R) -> Option<R> {
    FIBERS.with(|m| m.borrow_mut().get_mut(&id).map(f))
}

pub(crate) fn register(vm: &mut Vm) {
    FIBERS.with(|m| m.borrow_mut().clear());
    let mut ce = match goro_core::stubs::class_entry(vm, CLASS) {
        Some(stub) => {
            let mut ce = stub.clone();
            ce.methods.clear();
            ce
        }
        None => ClassEntry::new(CLASS.to_vec()),
    };
    ce.name = CLASS.to_vec();
    vm.classes.insert(CLASS_LOWER.to_vec(), ce);
    vm.native_class_handlers.insert(
        CLASS_LOWER.to_vec(),
        NativeClassHandler {
            has_method,
            call,
            prop_added: None,
            prop_write: None,
            compare_check: None,
            prop_read: None,
            param_names: Some(param_names),
        },
    );
    // clone_obj = NULL
    vm.uncloneable_classes.push(CLASS_LOWER.to_vec());
}

const METHODS: &[&str] = &["__construct", "start", "resume", "pipeTo", "suspend"];

fn canon(method: &[u8]) -> Option<&'static str> {
    METHODS
        .iter()
        .copied()
        .find(|m| m.as_bytes().eq_ignore_ascii_case(method))
}

fn has_method(_class: &[u8], method: &[u8], _is_static: bool) -> bool {
    canon(method).is_some()
}

fn param_names(_class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    Some(match canon(method)? {
        "__construct" | "pipeTo" => &["callback"],
        "start" => &["args"],
        "resume" | "suspend" => &["value"],
        _ => &[],
    })
}

fn throw(vm: &mut Vm, class: &[u8], msg: &str) -> Value {
    let exc = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(exc);
    Value::Null
}

fn call(
    vm: &mut Vm,
    _class: &[u8],
    method: &[u8],
    _called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    let Some(m) = canon(method) else {
        return Value::Null;
    };
    let args: Vec<Value> = args.iter().map(|a| a.deref()).collect();
    if m == "suspend" {
        return suspend(vm, &args);
    }
    let Some(Value::Object(o)) = this.map(|t| t.deref()) else {
        return throw(
            vm,
            b"Error",
            &format!(
                "Non-static method _ZendTestFiber::{}() cannot be called statically",
                m
            ),
        );
    };
    match m {
        "__construct" => construct(vm, &o, &args),
        "start" => start(vm, &o, args),
        "resume" => resume(vm, &o, &args),
        "pipeTo" => pipe_to(vm, &o, &args),
        _ => Value::Null,
    }
}

/// Z_PARAM_FUNC
fn callback_arg(vm: &mut Vm, method: &str, args: &[Value]) -> Option<Value> {
    if args.len() != 1 {
        let msg = format!(
            "_ZendTestFiber::{}() expects exactly 1 argument, {} given",
            method,
            args.len()
        );
        throw(vm, b"ArgumentCountError", &msg);
        return None;
    }
    if let Some(err) = vm.callable_error(&args[0]) {
        let msg = format!(
            "_ZendTestFiber::{}(): Argument #1 ($callback) must be a valid callback, {}",
            method, err
        );
        throw(vm, b"TypeError", &msg);
        return None;
    }
    Some(args[0].clone())
}

fn new_state(callback: Value, is_target: bool) -> TestFiber {
    TestFiber {
        callback,
        status: Status::Init,
        destroyed: false,
        is_target,
        target: None,
    }
}

fn construct(vm: &mut Vm, o: &Rc<RefCell<PhpObject>>, args: &[Value]) -> Value {
    let Some(cb) = callback_arg(vm, "__construct", args) else {
        return Value::Null;
    };
    let id = o.borrow().object_id;
    FIBERS.with(|m| m.borrow_mut().insert(id, new_state(cb, false)));
    Value::Null
}

/// zend_test_fiber_execute(): run the fiber's function, then the fibers
/// it is piped to, each with the previous one's result.
fn execute(vm: &mut Vm, id: u64, args: Vec<Value>) -> Result<Value, goro_core::vm::VmError> {
    let cb = with(id, |f| f.callback.clone()).unwrap_or(Value::Null);
    let mut result = vm.call_callback(&cb, &args)?;
    let mut cur = id;
    while let Some(target) = with(cur, |f| f.target.clone()).flatten() {
        let tid = target.borrow().object_id;
        let tcb = with(tid, |f| {
            f.status = Status::Running;
            f.callback.clone()
        })
        .unwrap_or(Value::Null);
        let r = vm.call_callback(&tcb, std::slice::from_ref(&result));
        with(tid, |f| f.status = Status::Dead);
        result = r?;
        cur = tid;
    }
    Ok(result)
}

fn start(vm: &mut Vm, o: &Rc<RefCell<PhpObject>>, args: Vec<Value>) -> Value {
    let id = o.borrow().object_id;
    let Some((status, is_target)) = with(id, |f| (f.status, f.is_target)) else {
        return Value::Null;
    };
    if status != Status::Init {
        return throw(
            vm,
            b"Error",
            "Cannot start a fiber that has already been started",
        );
    }
    if is_target {
        return throw(
            vm,
            b"Error",
            "Cannot start a fiber that is the target of another fiber",
        );
    }
    let cb = with(id, |f| f.callback.clone()).unwrap_or(Value::Null);
    let name = vm.callable_display_name(&cb);
    vm.custom_fiber_init(
        o,
        name,
        Box::new(move |vm: &mut Vm, _| execute(vm, id, args)),
        destroy,
    );
    switch(vm, id, Value::Null)
}

/// zend_test_fiber_resume() + delegate_transfer_result()
fn switch(vm: &mut Vm, id: u64, value: Value) -> Value {
    with(id, |f| f.status = Status::Running);
    match vm.custom_fiber_switch(id, FiberResume::Value(value)) {
        CustomSwitch::Suspended(v) => {
            with(id, |f| f.status = Status::Suspended);
            v
        }
        CustomSwitch::Returned(_) => {
            with(id, |f| f.status = Status::Dead);
            Value::Null
        }
        CustomSwitch::Threw(e) => {
            let destroyed = with(id, |f| {
                f.status = Status::Dead;
                f.destroyed
            })
            .unwrap_or(false);
            if !(destroyed && Vm::is_graceful_exit(&e)) {
                vm.current_exception = Some(e);
            }
            Value::Null
        }
        CustomSwitch::Failed(e) => {
            with(id, |f| f.status = Status::Dead);
            vm.ob_pending_fatal.get_or_insert(e);
            Value::Null
        }
    }
}

fn resume(vm: &mut Vm, o: &Rc<RefCell<PhpObject>>, args: &[Value]) -> Value {
    let id = o.borrow().object_id;
    let status = with(id, |f| f.status);
    if status != Some(Status::Suspended) || !vm.custom_fiber_idle(id) {
        return throw(vm, b"Error", "Cannot resume a fiber that is not suspended");
    }
    let value = args.first().cloned().unwrap_or(Value::Null);
    switch(vm, id, value)
}

fn suspend(vm: &mut Vm, args: &[Value]) -> Value {
    let value = args.first().cloned().unwrap_or(Value::Null);
    let Some(id) = vm.custom_fiber_running() else {
        return throw(vm, b"Error", "Cannot suspend outside of fiber");
    };
    let r = vm.custom_fiber_suspend(value);
    if with(id, |f| f.destroyed).unwrap_or(false) {
        // The fiber is destroyed while suspended: unwind it.
        let ge = vm.create_graceful_exit();
        vm.current_exception = Some(ge);
        return Value::Null;
    }
    match r {
        Some(FiberResume::Value(v)) => v,
        Some(FiberResume::Throw(e)) => {
            vm.current_exception = Some(e);
            Value::Null
        }
        None => Value::Null,
    }
}

fn pipe_to(vm: &mut Vm, o: &Rc<RefCell<PhpObject>>, args: &[Value]) -> Value {
    let Some(cb) = callback_arg(vm, "pipeTo", args) else {
        return Value::Null;
    };
    let tid = vm.alloc_object_id();
    let target = Rc::new(RefCell::new(PhpObject::new(CLASS.to_vec(), tid)));
    FIBERS.with(|m| m.borrow_mut().insert(tid, new_state(cb, true)));
    let id = o.borrow().object_id;
    with(id, |f| f.target = Some(target.clone()));
    Value::Object(target)
}

/// zend_test_fiber_object_destroy(): a suspended fiber is resumed so that
/// its suspend() call unwinds it.
fn destroy(vm: &mut Vm, o: &Rc<RefCell<PhpObject>>) {
    let id = o.borrow().object_id;
    if with(id, |f| f.status) != Some(Status::Suspended) {
        return;
    }
    let saved = vm.current_exception.take();
    with(id, |f| f.destroyed = true);
    switch(vm, id, Value::Null);
    match vm.current_exception.clone() {
        Some(exc) => {
            if let Some(prev) = saved {
                Vm::exception_set_previous(&exc, prev);
            }
        }
        None => vm.current_exception = saved,
    }
}
