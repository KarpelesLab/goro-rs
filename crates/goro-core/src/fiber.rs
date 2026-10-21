//! Stackful PHP Fiber implementation.
//!
//! PHP 8.1 Fibers are stackful coroutines: `Fiber::suspend()` can be called
//! from arbitrarily deep call chains and execution resumes from exactly the
//! same point. We implement this with the pure-Rust `corosensei` crate.
//!
//! The bridge between `Fiber::suspend()` (called by user PHP code via the
//! main VM) and the coroutine's `Yielder` is a thread-local pointer
//! installed before each resume of a fiber.

use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;

use corosensei::stack::DefaultStack;
use corosensei::{Coroutine, CoroutineResult, Yielder};

use crate::value::Value;

/// Fiber state machine states.
///
/// These integer values are stored on the Fiber object's `__fiber_state`
/// property and are part of the contract with `vm.rs`; do not change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiberState {
    Created = 0,
    Running = 1,
    Suspended = 2,
    Terminated = 3,
}

/// Sentinel error message used to signal a fiber suspension from
/// generator-style code paths that pre-date the corosensei rewrite
/// (still used to unwind certain VM call frames).
pub const FIBER_SUSPEND_SENTINEL: &str = "__FIBER_SUSPEND__";

/// What a fiber's coroutine yielded back to the resumer.
#[derive(Debug, Clone)]
pub enum FiberYield {
    /// `Fiber::suspend($value)` was called from inside the fiber.
    Suspend(Value),
}

/// What the resumer passes into the coroutine when it resumes.
#[derive(Debug, Clone)]
pub enum FiberResume {
    /// Resume with a value to return from the pending `Fiber::suspend()`.
    Value(Value),
    /// Throw an exception out of the pending `Fiber::suspend()`
    /// (used to implement `Fiber::throw($exc)`).
    Throw(Value),
}

/// What a finished coroutine produced.
#[derive(Debug, Clone)]
pub enum FiberOutcome {
    /// The fiber's callback returned normally with the given value.
    Returned(Value),
    /// The fiber's callback raised an exception that escaped to the top.
    /// Contains the exception object (or Value::Null if none).
    Threw(Value),
    /// The fiber's callback failed with a fatal error (no exception
    /// object; the VM produced a `VmError` directly, e.g. via
    /// `trigger_error(E_USER_ERROR)`). The string is the error message
    /// and the u32 is the line number.
    Failed(String, u32),
}

/// fiber.stack_size when unset (ZEND_FIBER_DEFAULT_C_STACK_SIZE).
pub const DEFAULT_FIBER_STACK_SIZE: usize = 2 * 1024 * 1024;

/// Native stack a fiber starts with (calls that need more continue on
/// extra segments, see stack.rs).
const FIBER_NATIVE_STACK: usize = 2 * 1024 * 1024;

/// Switches back to the stack context saved when the guard was made.
struct RestoreStack(crate::stack::StackCtx);

impl Drop for RestoreStack {
    fn drop(&mut self) {
        crate::stack::restore(self.0);
    }
}

/// Concrete corosensei yielder type used for fibers.
pub(crate) type FiberYielder = Yielder<FiberResume, FiberYield>;

/// The PHP-facing handle around a stackful coroutine.
pub struct PhpFiber {
    coroutine: Option<Coroutine<FiberResume, FiberYield, FiberOutcome>>,
    /// Pointer to this fiber's yielder, captured on first resume.
    /// Used by the parent to re-install CURRENT_YIELDER before every
    /// subsequent resume so `Fiber::suspend()` inside the body sees the
    /// correct yielder.
    yielder_ptr: Rc<Cell<*const FiberYielder>>,
    /// Display name of the user callback (e.g. `{closure:file:42}`),
    /// pushed onto the VM call stack while the fiber is running so it
    /// shows up correctly in backtraces.
    pub display_name: String,
    /// Saved `error_reporting` value at the point the fiber suspended,
    /// restored at resume so that `@`-silenced regions survive across
    /// suspension boundaries.
    pub saved_error_reporting: i64,
    /// Saved `error_reporting_stack` snapshot at suspension.
    pub saved_error_reporting_stack: Vec<i64>,
    /// Arguments passed to `Fiber::start()`, shown on the callback's
    /// backtrace frame.
    pub start_args: Vec<Value>,
    /// VM frame stacks (call stack, class scopes, called classes, active
    /// property hooks) owned by the suspended fiber; moved off the shared
    /// VM stacks while the fiber is suspended and restored on resume.
    pub saved_call_stack: Vec<(String, String, u32, Vec<Value>, bool)>,
    /// Builtin calls in progress inside the suspended fiber
    pub(crate) saved_builtin_frames: Vec<crate::internal_frames::BuiltinFrame>,
    /// Parameter CVs of the suspended fiber's running functions
    pub(crate) saved_cv_frames: Vec<crate::internal_frames::CvFrame>,
    /// Locals of the suspended fiber's running functions, for the cycle
    /// collector (see gc::GcFrame).
    pub(crate) saved_gc_frames: Vec<crate::gc::GcFrame>,
    /// Calls whose arguments the suspended fiber was still building
    pub(crate) saved_pending_calls: Vec<crate::vm::PendingCall>,
    /// closure_pending_calls entries of those, relative to their base
    pub(crate) saved_closure_pending: Vec<usize>,
    pub saved_class_scope_stack: Vec<Vec<u8>>,
    pub saved_called_class_stack: Vec<Vec<u8>>,
    pub saved_property_hook_stack: Vec<Vec<u8>>,
    /// File and line of the `Fiber::suspend()` call it is suspended at.
    pub suspended_at: Option<(String, u32)>,
}

thread_local! {
    /// The currently active fiber's yielder. Set by `PhpFiber::resume()`
    /// before driving the coroutine, restored to its previous value after.
    /// `Fiber::suspend()` reads this to know which coroutine to yield from.
    static CURRENT_YIELDER: RefCell<*const FiberYielder> =
        const { RefCell::new(std::ptr::null()) };
}

impl PhpFiber {
    /// Create a new, unstarted fiber from a closure that executes the
    /// user's PHP callable. `display_name` is shown in backtraces;
    /// `stack_size` is fiber.stack_size.
    pub fn new<F>(display_name: String, stack_size: usize, body: F) -> Self
    where
        F: FnOnce(FiberResume) -> FiberOutcome + 'static,
    {
        let yielder_ptr: Rc<Cell<*const FiberYielder>> = Rc::new(Cell::new(std::ptr::null()));
        let yielder_ptr_for_body = yielder_ptr.clone();
        let stack = DefaultStack::new(FIBER_NATIVE_STACK.max(stack_size.min(64 << 20)))
            .expect("failed to allocate fiber stack");
        let ctx = crate::stack::fiber_ctx(&stack, stack_size);
        let coroutine =
            Coroutine::with_stack(stack, move |yielder: &FiberYielder, input: FiberResume| {
                // The fiber runs on its own stack.
                crate::stack::restore(ctx);
                // Stash the yielder pointer so the parent can re-install it on
                // every resume (the body wrapper code only runs on the very
                // first invocation; subsequent resumes continue from inside
                // yielder.suspend()).
                yielder_ptr_for_body.set(yielder as *const FiberYielder);
                // On the very first entry, the parent already arranged for
                // CURRENT_YIELDER to point at this yielder (we'll do the same
                // before every later resume), but make sure it's set in case
                // a fiber's body runs without the parent setting it (paranoia).
                let prev = CURRENT_YIELDER.with(|cell| {
                    let prev = *cell.borrow();
                    *cell.borrow_mut() = yielder as *const FiberYielder;
                    prev
                });
                let outcome = body(input);
                // Restore on body return so the outer code sees the correct
                // yielder for any outer fiber it might still be inside.
                CURRENT_YIELDER.with(|cell| *cell.borrow_mut() = prev);
                outcome
            });
        Self {
            coroutine: Some(coroutine),
            yielder_ptr,
            display_name,
            saved_error_reporting: 0,
            saved_error_reporting_stack: Vec::new(),
            start_args: Vec::new(),
            saved_call_stack: Vec::new(),
            saved_builtin_frames: Vec::new(),
            saved_cv_frames: Vec::new(),
            saved_gc_frames: Vec::new(),
            saved_pending_calls: Vec::new(),
            saved_closure_pending: Vec::new(),
            saved_class_scope_stack: Vec::new(),
            saved_called_class_stack: Vec::new(),
            saved_property_hook_stack: Vec::new(),
            suspended_at: None,
        }
    }

    /// True if this fiber has already been started at least once.
    pub fn started(&self) -> bool {
        self.coroutine.as_ref().map(|c| c.started()).unwrap_or(true)
    }

    /// True if this fiber has finished (returned or threw).
    pub fn done(&self) -> bool {
        self.coroutine.as_ref().map(|c| c.done()).unwrap_or(true)
    }

    /// Get the yielder pointer for this fiber (null until it's first
    /// resumed). The parent uses this to set the thread-local before
    /// driving the coroutine.
    pub fn yielder_ptr(&self) -> *const FiberYielder {
        self.yielder_ptr.get()
    }

    /// Resume the fiber. Before driving the coroutine, the caller should
    /// have set CURRENT_YIELDER appropriately (via `set_current_yielder`)
    /// so that `Fiber::suspend()` calls in the body can reach it.
    pub fn resume(&mut self, input: FiberResume) -> Result<FiberYield, FiberOutcome> {
        let co = self
            .coroutine
            .as_mut()
            .expect("fiber coroutine already consumed");
        let _back = RestoreStack(crate::stack::save());
        match co.resume(input) {
            CoroutineResult::Yield(y) => Ok(y),
            CoroutineResult::Return(r) => Err(r),
        }
    }

    /// Force-unwind a fiber that's still suspended. Used when the
    /// `Fiber` object is being destroyed without having been resumed
    /// to completion (so destructors and `finally` blocks run).
    pub fn force_unwind(&mut self) {
        if let Some(mut co) = self.coroutine.take() {
            if co.started() && !co.done() {
                let _back = RestoreStack(crate::stack::save());
                co.force_unwind();
            }
        }
    }
}

impl Drop for PhpFiber {
    fn drop(&mut self) {
        self.force_unwind();
    }
}

/// Suspend the currently running fiber from anywhere in the call chain.
/// Returns `Some(resume_value)` if there is a current fiber, `None` if
/// not (caller is responsible for raising a `FiberError`).
pub fn suspend_current(value: Value) -> Option<FiberResume> {
    let raw = CURRENT_YIELDER.with(|cell| *cell.borrow());
    if raw.is_null() {
        return None;
    }
    // SAFETY: the yielder pointer is valid as long as the coroutine is
    // alive and we only call this from inside one on the same thread.
    let yielder: &FiberYielder = unsafe { &*raw };
    // Resumed on this fiber's stack, whatever ran meanwhile.
    let _back = RestoreStack(crate::stack::save());
    let r = yielder.suspend(FiberYield::Suspend(value));
    Some(r)
}

/// True if a fiber is currently running on this thread.
pub fn in_fiber() -> bool {
    CURRENT_YIELDER.with(|cell| !cell.borrow().is_null())
}

/// Set the current fiber yielder pointer, returning the previous value
/// for later restoration. Used by the resumer of a fiber so its
/// `Fiber::suspend()` calls reach the right yielder.
pub fn set_current_yielder(p: *const FiberYielder) -> *const FiberYielder {
    CURRENT_YIELDER.with(|cell| {
        let prev = *cell.borrow();
        *cell.borrow_mut() = p;
        prev
    })
}

/// Restore a previously-saved yielder pointer.
pub fn restore_current_yielder(prev: *const FiberYielder) {
    CURRENT_YIELDER.with(|cell| {
        *cell.borrow_mut() = prev;
    });
}
