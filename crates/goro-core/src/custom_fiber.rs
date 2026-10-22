//! Fiber contexts that are not `Fiber`s: what an extension builds with
//! zend_fiber_init_context() / zend_fiber_switch_context() (ext/zend_test's
//! _ZendTestFiber).
//!
//! Such a context runs on its own native stack (a corosensei coroutine, like
//! a `Fiber`) and on its own VM stack: its frames do not sit on top of the
//! resumer's (its backtraces end with its own bottom frame), so switching to
//! it swaps the VM's frame stacks ([`FrameStacks`]) with the ones it saved.
//!
//! It is not a `Fiber`: `Fiber::getCurrent()` inside it is the enclosing
//! fiber, and `Fiber::suspend()` inside it suspends that fiber, taking the
//! custom context along (it is resumed with the fiber). Likewise a custom
//! context suspending while a `Fiber` it started runs takes that fiber along.
//! The bookkeeping for both directions is in [`Vm::fiber_suspend_switch`]
//! and [`Vm::custom_fiber_suspend`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::fiber::{FiberOutcome, FiberResume, FiberYield, FiberYielder, PhpFiber};
use crate::object::PhpObject;
use crate::value::Value;
use crate::vm::{Vm, VmError};

/// The VM stacks describing a context's running frames (all indexed from the
/// context's own bottom frame).
#[derive(Default)]
pub struct FrameStacks {
    call_stack: Vec<(String, String, u32, Vec<Value>, bool)>,
    class_scope_stack: Vec<Vec<u8>>,
    called_class_stack: Vec<Vec<u8>>,
    property_hook_stack: Vec<Vec<u8>>,
    builtin_frames: Vec<crate::internal_frames::BuiltinFrame>,
    cv_frames: Vec<crate::internal_frames::CvFrame>,
    frame_this: Vec<crate::internal_frames::FrameThis>,
    gc_frames: Vec<crate::gc::GcFrame>,
    pending_calls: Vec<crate::vm::PendingCall>,
    closure_pending_calls: Vec<usize>,
    closure_frames: Vec<(usize, String, std::rc::Weak<RefCell<PhpObject>>)>,
    spl_internal_frames: Vec<(usize, String)>,
    exec_frame_len: usize,
    fiber_bases: Vec<(u64, usize)>,
    current_file: String,
    current_line: u32,
}

/// What a custom context destroys itself with (its object's dtor_obj).
pub type CustomFiberDestroy = fn(&mut Vm, &Rc<RefCell<PhpObject>>);

/// A custom fiber context that is not running.
pub struct CustomFiber {
    fiber: PhpFiber,
    stacks: FrameStacks,
    destroy: CustomFiberDestroy,
}

/// The `Fiber` state when a custom context was switched to.
#[derive(Clone)]
pub struct FiberMark {
    stack_len: usize,
    current: Option<Value>,
    depth: u32,
    yielder: *const FiberYielder,
    marks_len: usize,
}

/// The running custom context.
#[derive(Clone)]
pub struct CustomActive {
    id: u64,
    yielder: Rc<Cell<*const FiberYielder>>,
    fiber: FiberMark,
}

/// How control came back from a custom context.
pub enum CustomSwitch {
    /// It suspended, passing this value.
    Suspended(Value),
    /// Its function returned this value.
    Returned(Value),
    /// Its function threw this exception.
    Threw(Value),
    /// It ended with a fatal error.
    Failed(VmError),
}

impl Vm {
    fn swap_frame_stacks(&mut self, s: &mut FrameStacks) {
        use std::mem::swap;
        swap(&mut self.call_stack, &mut s.call_stack);
        swap(&mut self.class_scope_stack, &mut s.class_scope_stack);
        swap(&mut self.called_class_stack, &mut s.called_class_stack);
        swap(&mut self.property_hook_stack, &mut s.property_hook_stack);
        swap(&mut self.builtin_frames, &mut s.builtin_frames);
        swap(&mut self.cv_frames, &mut s.cv_frames);
        swap(&mut self.frame_this, &mut s.frame_this);
        swap(&mut self.gc_frames, &mut s.gc_frames);
        swap(&mut self.pending_calls, &mut s.pending_calls);
        swap(
            &mut self.closure_pending_calls,
            &mut s.closure_pending_calls,
        );
        swap(&mut self.closure_frames, &mut s.closure_frames);
        swap(&mut self.spl_internal_frames, &mut s.spl_internal_frames);
        swap(&mut self.exec_frame_len, &mut s.exec_frame_len);
        swap(&mut self.fiber_bases, &mut s.fiber_bases);
        swap(&mut self.current_file, &mut s.current_file);
        swap(&mut self.current_line, &mut s.current_line);
    }

    /// zend_fiber_init_context(): make object `obj` a custom fiber context
    /// that runs `body` when first switched to (with the value it is
    /// switched to with). `destroy` is called when the object is released
    /// while the context is suspended.
    pub fn custom_fiber_init(
        &mut self,
        obj: &Rc<RefCell<PhpObject>>,
        display_name: String,
        body: Box<dyn FnOnce(&mut Vm, Value) -> Result<Value, VmError>>,
        destroy: CustomFiberDestroy,
    ) {
        let entry = move |input: FiberResume| -> FiberOutcome {
            let vm_ptr = crate::vm::current_vm_ptr();
            if vm_ptr.is_null() {
                return FiberOutcome::Threw(Value::Null);
            }
            // SAFETY: as in Vm::fiber_start(): the resumer set the pointer
            // to the live VM, and the coroutine runs on its thread.
            let vm: &mut Vm = unsafe { &mut *vm_ptr };
            let input = match input {
                FiberResume::Value(v) | FiberResume::Throw(v) => v,
            };
            match body(vm, input) {
                Ok(v) => FiberOutcome::Returned(v),
                Err(e) => match vm.current_exception.take() {
                    Some(exc) => FiberOutcome::Threw(exc),
                    None => FiberOutcome::Failed(e.message, e.line),
                },
            }
        };
        let fiber =
            PhpFiber::new_custom(display_name, crate::fiber::DEFAULT_FIBER_STACK_SIZE, entry);
        let (id, file) = (obj.borrow().object_id, self.current_file.clone());
        let stacks = FrameStacks {
            current_file: file,
            ..Default::default()
        };
        self.custom_fibers.insert(
            id,
            CustomFiber {
                fiber,
                stacks,
                destroy,
            },
        );
        // Released while suspended: destroyed (see custom_fiber_released).
        if !obj.borrow().dtor_listed.get() {
            self.list_destructible(Value::Object(obj.clone()));
        }
    }

    /// Whether object `id` is a custom fiber context that is not running
    /// (not started yet, or suspended).
    pub fn custom_fiber_idle(&self, id: u64) -> bool {
        self.custom_fibers.contains_key(&id)
    }

    /// The object id of the running custom context.
    pub fn custom_fiber_running(&self) -> Option<u64> {
        self.custom_active.as_ref().map(|a| a.id)
    }

    /// An object nothing references any more is a suspended custom context:
    /// destroy it (true), instead of running a destructor.
    pub(crate) fn custom_fiber_released(&mut self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        let id = obj.borrow().object_id;
        let Some(cf) = self.custom_fibers.get(&id) else {
            return false;
        };
        let destroy = cf.destroy;
        if cf.fiber.started() {
            destroy(self, obj);
        } else {
            self.custom_fibers.remove(&id);
        }
        true
    }

    /// zend_fiber_switch_context() to the custom context of object `id`
    /// (which must be idle), until it suspends or ends.
    pub fn custom_fiber_switch(&mut self, id: u64, input: FiberResume) -> CustomSwitch {
        let Some(mut cf) = self.custom_fibers.remove(&id) else {
            return CustomSwitch::Failed(VmError {
                message: "Cannot switch to a running fiber context".into(),
                line: self.current_line,
            });
        };
        let line = self.current_line;
        self.swap_frame_stacks(&mut cf.stacks);
        self.custom_parked.push(std::mem::take(&mut cf.stacks));
        let mark = FiberMark {
            stack_len: self.fiber_stack.len(),
            current: self.current_fiber.clone(),
            depth: self.fiber_depth,
            yielder: crate::fiber::current_yielder(),
            marks_len: self.fiber_custom_marks.len(),
        };
        let prev_active = self.custom_active.replace(CustomActive {
            id,
            yielder: cf.fiber.yielder_cell(),
            fiber: mark,
        });
        let prev_vm = crate::vm::current_vm_ptr();
        crate::vm::set_current_vm_ptr(self as *mut Vm);
        let result = cf.fiber.resume(input);
        crate::vm::set_current_vm_ptr(prev_vm);
        self.custom_active = prev_active;
        let mut outer = self.custom_parked.pop().unwrap_or_default();
        self.swap_frame_stacks(&mut outer);
        self.current_line = line;
        match result {
            Ok(FiberYield::Suspend(v)) => {
                cf.stacks = outer;
                self.custom_fibers.insert(id, cf);
                CustomSwitch::Suspended(v)
            }
            Err(FiberOutcome::Returned(v)) => CustomSwitch::Returned(v),
            Err(FiberOutcome::Threw(e)) => CustomSwitch::Threw(e),
            Err(FiberOutcome::Failed(message, line)) => {
                CustomSwitch::Failed(VmError { message, line })
            }
        }
    }

    /// zend_fiber_switch_context() from the running custom context back to
    /// the one that switched to it. None if no custom context is running.
    pub fn custom_fiber_suspend(&mut self, value: Value) -> Option<FiberResume> {
        let active = self.custom_active.clone()?;
        // Fibers started inside the context since it was resumed are
        // suspended with it.
        let m = &active.fiber;
        let stack = self
            .fiber_stack
            .split_off(m.stack_len.min(self.fiber_stack.len()));
        let current = std::mem::replace(&mut self.current_fiber, m.current.clone());
        let depth = std::mem::replace(&mut self.fiber_depth, m.depth);
        let yielder = crate::fiber::set_current_yielder(m.yielder);
        let marks = self
            .fiber_custom_marks
            .split_off(m.marks_len.min(self.fiber_custom_marks.len()));
        let r = crate::fiber::suspend_yielder(active.yielder.get(), value);
        self.fiber_stack.extend(stack);
        self.current_fiber = current;
        self.fiber_depth = depth;
        crate::fiber::set_current_yielder(yielder);
        self.fiber_custom_marks.extend(marks);
        r
    }

    /// `Fiber::suspend()`: suspend the running fiber. Custom contexts
    /// switched to since it was resumed are suspended with it: their frame
    /// stacks are put aside until it is resumed.
    pub(crate) fn fiber_suspend_switch(&mut self, value: Value) -> Option<FiberResume> {
        let (mark, active_at_resume) = match self.fiber_custom_marks.last() {
            Some((m, a)) if self.custom_parked.len() > *m => (*m, a.clone()),
            _ => return crate::fiber::suspend_current(value),
        };
        // custom_parked[mark] holds the fiber's own frame stacks; the ones
        // above it and the running ones are the custom contexts'.
        let mut chain = self.custom_parked.split_off(mark);
        let mut own = chain.remove(0);
        self.swap_frame_stacks(&mut own);
        chain.push(own);
        let active = std::mem::replace(&mut self.custom_active, active_at_resume);
        let r = crate::fiber::suspend_current(value);
        // Resumed: the fiber's frames are back on the VM stacks.
        let mut innermost = chain.pop().unwrap_or_default();
        self.swap_frame_stacks(&mut innermost);
        self.custom_parked.push(innermost);
        self.custom_parked.extend(chain);
        self.custom_active = active;
        r
    }
}
