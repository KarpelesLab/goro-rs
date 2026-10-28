//! Fibers (PHP manual: Fibers; RFC "Fibers").
//!
//! Each fiber runs on its own native stack (a `corosensei` coroutine) with
//! its own engine execution state (frames, slot stack, pending calls). The
//! resumer's state is kept on `Vm::ex_parents` while the fiber runs, so
//! backtraces can continue into it. A suspended fiber that is destroyed is
//! resumed with an uncatchable "graceful exit" that unwinds it, running
//! `finally` blocks.

use std::rc::Rc;

use corosensei::stack::DefaultStack;
use corosensei::{Coroutine, CoroutineResult, Yielder};
use goro_runtime::{ObjRef, PhpStr, Value};

use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{ExecState, R, Unwind, Vm};

pub enum FiberIn {
    Start(Vec<Value>),
    Resume(Value),
    Throw(ObjRef),
    Destroy,
}

pub enum FiberOut {
    Suspend(Value),
}

type FiberCo = Coroutine<FiberIn, FiberOut, R<Value>, DefaultStack>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FiberStatus {
    Init,
    Running,
    Suspended,
    Terminated,
}

pub struct FiberData {
    co: Option<FiberCo>,
    pub status: FiberStatus,
    callable: Value,
    ex: Option<ExecState>,
    yielder: *const Yielder<FiberIn, FiberOut>,
    result: Option<Value>,
    threw: bool,
    force_closing: bool,
    constructed: bool,
    /// error_reporting and `@` nesting of the fiber while it is suspended.
    er: Option<(i64, Vec<i64>)>,
}

impl goro_runtime::NativeData for FiberData {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        visit(&self.callable);
        if let Some(r) = &self.result {
            visit(r);
        }
        if let Some(ex) = &self.ex {
            ex.for_each_value(visit);
        }
    }
}

pub static MODULE: Module = Module {
    name: "Core",
    classes: &[
        (
            "fiber",
            "<?php final class Fiber { public function __construct(callable $callback) {} public function start(mixed ...$args): mixed {} public function resume(mixed $value = null): mixed {} public function throw(Throwable $exception): mixed {} public function getReturn(): mixed {} public function isStarted(): bool {} public function isSuspended(): bool {} public function isRunning(): bool {} public function isTerminated(): bool {} public static function suspend(mixed $value = null): mixed {} public static function getCurrent(): ?Fiber {} public function __destruct() {} }",
        ),
        (
            "fibererror",
            "<?php final class FiberError extends Error { public function __construct() { throw new \\Error('The \"FiberError\" class is reserved for internal use and cannot be manually instantiated'); } }",
        ),
        ("fiberexit_", "<?php final class FiberExit_ {}"),
    ],
    methods: &[
        ("fiber::__construct", fiber_construct),
        ("fiber::start", fiber_start),
        ("fiber::resume", fiber_resume),
        ("fiber::throw", fiber_throw),
        ("fiber::getreturn", fiber_get_return),
        ("fiber::isstarted", fiber_is_started),
        ("fiber::issuspended", fiber_is_suspended),
        ("fiber::isrunning", fiber_is_running),
        ("fiber::isterminated", fiber_is_terminated),
        ("fiber::suspend", fiber_suspend),
        ("fiber::getcurrent", fiber_get_current),
        ("fiber::__destruct", fiber_destruct),
    ],
    hooks: &[("fiber", fiber_hooks)],
    listed: false,
    ..Module::EMPTY
};

fn fiber_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|_, o| {
            *o.obj().native.borrow_mut() = Some(Box::new(FiberData {
                co: None,
                status: FiberStatus::Init,
                callable: Value::Null,
                ex: None,
                yielder: std::ptr::null(),
                result: None,
                threw: false,
                force_closing: false,
                constructed: false,
                er: None,
            }));
        }),
        clone: None,
        debug_props: Some(|_, _| Some(goro_runtime::PhpArray::new())),
        ..Default::default()
    }
}

fn with_fiber<T>(o: &ObjRef, f: impl FnOnce(&mut FiberData) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    Some(f(any.downcast_mut::<FiberData>()?))
}

fn fiber_error<T>(vm: &mut Vm, msg: &str) -> R<T> {
    // FiberError can't be constructed by scripts: build it directly.
    let cls = vm.find_class(b"FiberError", false)?.expect("FiberError");
    let o = vm.instantiate_raw(&cls)?;
    vm.set_prop_raw(&o, b"message", Value::Str(PhpStr::from(msg)));
    Err(Unwind::Throw(o))
}

fn fiber_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let cb = a.val(0);
    let twice = with_fiber(&o, |d| std::mem::replace(&mut d.constructed, true)).unwrap_or(false);
    if twice {
        return fiber_error(vm, "Cannot call constructor twice");
    }
    with_fiber(&o, |d| d.callable = cb);
    Ok(Value::Null)
}

/// Whether an exception is the internal graceful-exit marker.
fn is_exit_marker(vm: &Vm, e: &ObjRef) -> bool {
    vm.class_of(e).lname.as_bytes() == b"fiberexit_"
}

impl Vm {
    /// Switches into a fiber with `input`; returns the value it suspends
    /// with (null when it finishes).
    fn fiber_switch(&mut self, f: &ObjRef, input: FiberIn) -> R<Value> {
        let (co, fex) = with_fiber(f, |d| (d.co.take(), d.ex.take())).unwrap();
        let Some(mut co) = co else {
            return Ok(Value::Null);
        };
        let fex = fex.unwrap_or_default();
        let caller = std::mem::replace(&mut self.ex, fex);
        self.ex_parents.push(caller);
        // The `@` state belongs to each fiber.
        let caller_er = (
            self.ini.error_reporting,
            std::mem::take(&mut self.err.silence),
        );
        match with_fiber(f, |d| d.er.take()).flatten() {
            Some((er, sil)) => {
                self.ini.error_reporting = er;
                self.err.silence = sil;
            }
            None => {
                // A new fiber starts outside the caller's `@`.
                if let Some(&first) = caller_er.1.first() {
                    self.ini.error_reporting = first;
                }
            }
        }
        self.fiber_stack.push(f.clone());
        with_fiber(f, |d| d.status = FiberStatus::Running);
        let r = co.resume(input);
        let fiber_er = (
            self.ini.error_reporting,
            std::mem::take(&mut self.err.silence),
        );
        self.ini.error_reporting = caller_er.0;
        self.err.silence = caller_er.1;
        with_fiber(f, |d| d.er = Some(fiber_er));
        self.fiber_stack.pop();
        let caller = self.ex_parents.pop().unwrap();
        let fex = std::mem::replace(&mut self.ex, caller);
        match r {
            CoroutineResult::Yield(FiberOut::Suspend(v)) => {
                with_fiber(f, |d| {
                    d.co = Some(co);
                    d.ex = Some(fex);
                    d.status = FiberStatus::Suspended;
                });
                Ok(v)
            }
            CoroutineResult::Return(res) => {
                drop(co);
                drop(fex);
                match res {
                    Ok(v) => {
                        with_fiber(f, |d| {
                            d.status = FiberStatus::Terminated;
                            d.result = Some(v);
                        });
                        Ok(Value::Null)
                    }
                    Err(Unwind::Throw(e)) if is_exit_marker(self, &e) => {
                        with_fiber(f, |d| d.status = FiberStatus::Terminated);
                        Ok(Value::Null)
                    }
                    Err(e) => {
                        with_fiber(f, |d| {
                            d.status = FiberStatus::Terminated;
                            d.threw = true;
                        });
                        Err(e)
                    }
                }
            }
        }
    }
}

fn fiber_start(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let (status, cb) = with_fiber(&o, |d| (d.status, d.callable.clone())).unwrap();
    if status != FiberStatus::Init {
        return fiber_error(vm, "Cannot start a fiber that has already been started");
    }
    let args: Vec<Value> = a.vals.clone();
    let size = match vm.get_ini(b"fiber.stack_size") {
        Some(s) if !s.is_empty() => crate::vm::setup::parse_size(&s).max(64 * 1024) as usize,
        _ => 8 << 20,
    };
    let stack = match DefaultStack::new(size) {
        Ok(s) => s,
        Err(_) => return fiber_error(vm, "Fiber stack allocate failed"),
    };
    let vm_ptr = vm as *mut Vm;
    let weak = o.downgrade();
    let co: FiberCo = Coroutine::with_stack(
        stack,
        move |y: &Yielder<FiberIn, FiberOut>, input: FiberIn| {
            // SAFETY: the engine outlives its fibers and runs one at a time.
            let vm = unsafe { &mut *vm_ptr };
            if let Some(f) = weak.upgrade() {
                with_fiber(&f, |d| d.yielder = y as *const _);
            }
            let FiberIn::Start(args) = input else {
                return Ok(Value::Null);
            };
            vm.call_value(&cb, args)
        },
    );
    with_fiber(&o, |d| d.co = Some(co));
    vm.fiber_switch(&o, FiberIn::Start(args))
}

fn fiber_resume(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let status = with_fiber(&o, |d| d.status).unwrap();
    if status != FiberStatus::Suspended {
        if status == FiberStatus::Running {
            return fiber_error(vm, "Cannot resume a fiber that is not suspended");
        }
        return fiber_error(vm, "Cannot resume a fiber that is not suspended");
    }
    let v = a.val(0);
    vm.fiber_switch(&o, FiberIn::Resume(v))
}

fn fiber_throw(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let status = with_fiber(&o, |d| d.status).unwrap();
    if status != FiberStatus::Suspended {
        return fiber_error(vm, "Cannot resume a fiber that is not suspended");
    }
    let Some(e) = a.obj(0) else {
        return Ok(Value::Null);
    };
    vm.fiber_switch(&o, FiberIn::Throw(e))
}

fn fiber_get_return(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let (status, threw, result) =
        with_fiber(&o, |d| (d.status, d.threw, d.result.clone())).unwrap();
    match status {
        FiberStatus::Terminated if threw => fiber_error(
            vm,
            "Cannot get fiber return value: The fiber threw an exception",
        ),
        FiberStatus::Terminated => match result {
            Some(v) => Ok(v),
            None => fiber_error(
                vm,
                "Cannot get fiber return value: The fiber exited with a fatal error",
            ),
        },
        FiberStatus::Init => fiber_error(
            vm,
            "Cannot get fiber return value: The fiber has not been started",
        ),
        _ => fiber_error(
            vm,
            "Cannot get fiber return value: The fiber has not returned",
        ),
    }
}

fn status_is(a: &Args, f: impl Fn(FiberStatus) -> bool) -> Value {
    Value::Bool(with_fiber(a.this_obj(), |d| f(d.status)).unwrap_or(false))
}

fn fiber_is_started(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(status_is(a, |s| s != FiberStatus::Init))
}
fn fiber_is_suspended(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(status_is(a, |s| s == FiberStatus::Suspended))
}
fn fiber_is_running(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(status_is(a, |s| s == FiberStatus::Running))
}
fn fiber_is_terminated(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(status_is(a, |s| s == FiberStatus::Terminated))
}

fn fiber_suspend(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(f) = vm.fiber_stack.last().cloned() else {
        return fiber_error(vm, "Cannot suspend outside of a fiber");
    };
    let (y, closing) = with_fiber(&f, |d| (d.yielder, d.force_closing)).unwrap();
    // Don't keep the fiber alive while it is suspended.
    drop(f);
    if closing {
        return fiber_error(vm, "Cannot suspend in a force-closed fiber");
    }
    if vm.dtor_depth > 0 && false {
        return fiber_error(vm, "Cannot switch fibers in current execution state");
    }
    let v = a.val(0);
    // SAFETY: the yielder lives on this fiber's stack, which is running.
    let input = unsafe { (*y).suspend(FiberOut::Suspend(v)) };
    match input {
        FiberIn::Resume(v) => Ok(v),
        FiberIn::Throw(e) => Err(Unwind::Throw(e)),
        FiberIn::Destroy => {
            let cls = vm.find_class(b"FiberExit_", false)?.expect("FiberExit_");
            let o = vm.instantiate_raw(&cls)?;
            Err(Unwind::Throw(o))
        }
        FiberIn::Start(_) => Ok(Value::Null),
    }
}

fn fiber_get_current(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(match vm.fiber_stack.last() {
        Some(f) => Value::Object(f.clone()),
        None => Value::Null,
    })
}

/// A suspended fiber that goes away is unwound (its `finally` blocks run).
fn fiber_destruct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let status = with_fiber(&o, |d| d.status).unwrap();
    if status == FiberStatus::Suspended {
        with_fiber(&o, |d| d.force_closing = true);
        vm.fiber_switch(&o, FiberIn::Destroy)?;
    }
    Ok(Value::Null)
}

#[allow(dead_code)]
fn keep(_: Rc<()>) {}
