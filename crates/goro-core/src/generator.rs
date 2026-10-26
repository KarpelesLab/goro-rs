use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::opcode::{OpArray, OpCode, OperandType};
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::{Vm, VmError};

type GenRc = Rc<RefCell<PhpGenerator>>;

/// How the root of a `yield from` tree is continued.
#[derive(Clone, Copy)]
enum Kick {
    /// Advance it (next()/send()).
    Resume,
    /// Throw vm.current_exception at its suspension point.
    Throw,
    /// A generator just started delegating to it: prime it if it has not
    /// started, otherwise leave it at its current value.
    Fresh,
}

/// The state of a PHP generator
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeneratorState {
    /// Generator has been created but not yet started (before first next()/rewind())
    Created,
    /// Generator is suspended at a yield point
    Suspended,
    /// Generator has completed (returned or no more yields)
    Completed,
}

/// A PHP Generator object
///
/// Generators are functions that use `yield` to produce values lazily.
/// When called, a generator function returns a Generator object that
/// implements the Iterator interface.
#[derive(Debug)]
pub struct PhpGenerator {
    /// The compiled function body
    /// Shared with the function's definition (a generator never patches it).
    pub op_array: Rc<OpArray>,
    /// Current instruction pointer (where to resume)
    pub ip: usize,
    /// Compiled variables (function locals)
    pub cvs: Vec<Value>,
    /// Temporary variables
    pub tmps: Vec<Value>,
    /// The frame's symbol table, once dynamic variables needed one.
    pub(crate) symtab: Option<crate::symtab::FrameSymtab>,
    /// Static variable keys
    pub static_cv_keys: HashMap<u32, Vec<u8>>,
    /// Global variable keys
    pub global_cv_keys: HashMap<u32, Vec<u8>>,
    /// Exception handlers: (catch target, finally target,
    /// error_reporting_stack depth at the TryBegin)
    pub exception_handlers: Vec<(u32, u32, u32)>,
    /// Current state
    pub state: GeneratorState,
    /// The current yielded value
    pub current_value: Value,
    /// The current yield key (auto-incrementing integer by default)
    pub current_key: Value,
    /// Auto-increment key counter
    pub key_counter: i64,
    /// Value sent to the generator via send()
    pub send_value: Value,
    /// The return value of the generator (set when it returns)
    pub return_value: Value,
    /// Yield from: inner iterable being delegated to
    pub yield_from_source: Option<Value>,
    /// Yield from: position within array iteration
    pub yield_from_pos: usize,
    /// Yield from (Generator only): set after we yielded a value from the inner
    /// generator's current state. On re-entry to YieldFrom we must first advance
    /// the inner before yielding again. Without this flag we either advance too
    /// eagerly (sharing-an-inner case) or never (single-owner case).
    pub yield_from_observed: bool,
    /// When fiber suspension happens during DoFCall, this stores the result operand
    /// that should receive the resume value when the fiber resumes
    pub fiber_suspend_result: Option<crate::opcode::OperandType>,
    /// Whether this generator is currently running (executing its body).
    /// Used to detect and prevent recursive resumption.
    pub is_running: bool,
    /// The called class (for late static binding / get_called_class())
    /// Captured at generator creation and pushed onto the VM stack during resume.
    pub called_class: Option<Vec<u8>>,
    /// The class scope (defining class, for get_class() / visibility)
    /// Captured at generator creation and pushed onto the VM stack during resume.
    pub class_scope: Option<Vec<u8>>,
    /// The arguments passed to the generator function (for func_get_args())
    pub args: Vec<Value>,
    /// The file the generator was called from (for backtrace)
    pub call_file: crate::rcstr::RcStr,
    /// The line the generator was called from (for backtrace)
    pub call_line: u32,
    /// Pending return value for deferred return (finally blocks)
    pub pending_return: Option<Value>,
    /// Pending jump target for deferred break/continue (finally blocks)
    pub pending_finally_jump: Option<u32>,
    /// Pending exception value: if the generator yields while an exception is still
    /// in flight (e.g. yield inside a `finally` block), we stash the exception here
    /// and clear `vm.current_exception` so callers can iterate without seeing the
    /// in-flight exception until the finally block completes.
    pub pending_exception: Option<Value>,
    /// Whether this generator has been force-closed (destruction while suspended)
    pub force_closed: bool,
    /// Whether this generator completed with an explicit return (vs exception/abort)
    pub has_returned: bool,
    /// Whether the generator has been advanced past the first yield (for rewind check)
    pub advanced_past_first: bool,
    /// Calls whose arguments were being evaluated when the generator yielded
    /// (`f($a, yield)`); restored onto the VM's call stack on resume.
    pub(crate) saved_calls: Vec<crate::vm::PendingCall>,
    /// The function name shown for it while it runs (computed once).
    pub(crate) running_name: Option<Rc<str>>,
    /// Registered with the cycle collector.
    pub(crate) gc_tracked: std::cell::Cell<bool>,
    /// The collector already ran this generator's destructor
    /// (zend_generator_dtor_storage; IS_OBJ_DESTRUCTOR_CALLED).
    pub(crate) gc_dtor_called: std::cell::Cell<bool>,
    /// The reference yielded by a by-ref generator (bound by `foreach (... as &$v)`).
    pub(crate) current_ref: Option<Rc<RefCell<Value>>>,
    /// Property hook this generator is the body of (`get { yield ...; }`).
    pub(crate) hook_key: Option<Vec<u8>>,
    /// frame_name(), computed on the first resume (it only depends on the
    /// function and its class, which do not change).
    frame_name_cache: Option<(String, bool)>,
    /// call_stack index of the generator's frame while it runs (relative to
    /// the running fiber's bottom frame)
    frame_depth: usize,
    /// Finally blocks being executed: (handler depth at entry, the exception
    /// that led into it), as in Vm::execute_frame.
    pub(crate) fin: Vec<(usize, Option<Value>)>,
    /// Levels a deferred `break N` still has to leave.
    pub(crate) pending_finally_levels: u32,
    /// Result operand of the `yield from` delegating to a generator: receives
    /// the inner generator's return value.
    pub(crate) yield_from_result: OperandType,
    /// The last resume_single() executed a `yield from` that delegates to a
    /// generator (ZEND_YIELD_FROM returned to zend_generator_resume()).
    pub(crate) just_delegated: bool,
    /// Bytes charged against memory_limit for the generator and its frame.
    pub(crate) mem_charged: usize,
    /// Started delegating with `yield from` (ZEND_GENERATOR_DO_INIT): the
    /// next resume of this generator does not advance a root that already
    /// produced a value.
    pub(crate) do_init: bool,
    /// The last value/key yielded, kept after completion (generator->value
    /// lives until the object is freed).
    pub(crate) last_value: Value,
    pub(crate) last_key: Value,
    /// Iterator state of the foreach loops running in the frame.
    pub(crate) fe: crate::vm::ForeachState,
    /// The Closure whose body this generator runs (it keeps it alive).
    pub(crate) closure: Option<Value>,
    /// The object handle (allocated by Vm::new_generator; 0 = none).
    pub object_id: u64,
}

impl Drop for PhpGenerator {
    /// Release a `yield from` delegation chain iteratively: a deep chain
    /// would otherwise overflow the native stack in nested drops.
    fn drop(&mut self) {
        crate::value::memory_free(self.mem_charged);
        let mut next = self.yield_from_source.take();
        while let Some(Value::Generator(rc)) = next {
            next = match Rc::try_unwrap(rc) {
                Ok(cell) => {
                    let mut g = cell.into_inner();
                    g.yield_from_source.take()
                }
                Err(_) => None,
            };
        }
        // free_obj releases the frame before the handle goes back to the
        // free list (zend_objects_store_del).
        if self.object_id != 0 {
            self.release_param_copies();
            drop(std::mem::take(&mut self.cvs));
            drop(std::mem::take(&mut self.tmps));
            drop(self.symtab.take());
            drop(std::mem::take(&mut self.args));
            drop(std::mem::replace(&mut self.current_value, Value::Null));
            drop(std::mem::replace(&mut self.current_key, Value::Null));
            drop(std::mem::replace(&mut self.send_value, Value::Null));
            drop(std::mem::replace(&mut self.return_value, Value::Null));
            drop(std::mem::replace(&mut self.last_value, Value::Null));
            drop(std::mem::replace(&mut self.last_key, Value::Null));
            drop(self.pending_return.take());
            drop(self.pending_exception.take());
            drop(self.current_ref.take());
            drop(std::mem::take(&mut self.saved_calls));
            drop(std::mem::take(&mut self.fe));
            drop(std::mem::take(&mut self.fin));
            drop(self.closure.take());
            let id = self.object_id;
            let _ = crate::object::FREED_OBJECT_IDS.try_with(|ids| ids.borrow_mut().push(id));
        }
    }
}

impl PhpGenerator {
    /// zend_generator_get_current(): the value and key of the innermost
    /// still-running generator `gen` delegates to with `yield from` (several
    /// generators may delegate to the same one).
    pub fn leaf_current(
        generator: &std::rc::Rc<std::cell::RefCell<PhpGenerator>>,
    ) -> (Value, Value) {
        let mut cur = generator.clone();
        for _ in 0..10_000 {
            let next = match cur.try_borrow() {
                Ok(b) => match &b.yield_from_source {
                    Some(Value::Generator(inner))
                        if matches!(
                            inner.try_borrow().map(|ib| ib.state.clone()),
                            Ok(GeneratorState::Suspended)
                        ) =>
                    {
                        Some(inner.clone())
                    }
                    _ => None,
                },
                Err(_) => None,
            };
            match next {
                Some(n) => cur = n,
                None => break,
            }
        }
        match cur.try_borrow() {
            Ok(b) => (b.current_value.clone(), b.current_key.clone()),
            Err(_) => (Value::Null, Value::Null),
        }
    }

    /// Create a new generator from a compiled op_array and initial CVs
    pub fn new(op_array: impl Into<Rc<OpArray>>, cvs: Vec<Value>) -> Self {
        let op_array: Rc<OpArray> = op_array.into();
        let temp_count = op_array.temp_count as usize;
        // zend_generator + its execute_data (call frame, CVs and TMPs).
        let mem_charged = 256 + 16 * (cvs.len() + temp_count);
        crate::value::memory_charge(mem_charged);
        Self {
            op_array,
            ip: 0,
            cvs,
            tmps: vec![Value::Undef; temp_count],
            symtab: None,
            static_cv_keys: HashMap::new(),
            global_cv_keys: HashMap::new(),
            exception_handlers: Vec::new(),
            state: GeneratorState::Created,
            current_value: Value::Null,
            current_key: Value::Null,
            key_counter: 0,
            send_value: Value::Null,
            return_value: Value::Null,
            yield_from_source: None,
            yield_from_pos: 0,
            yield_from_observed: false,
            fiber_suspend_result: None,
            is_running: false,
            called_class: None,
            class_scope: None,
            args: Vec::new(),
            call_file: crate::rcstr::RcStr::new(),
            call_line: 0,
            pending_return: None,
            pending_finally_jump: None,
            pending_exception: None,
            force_closed: false,
            has_returned: false,
            advanced_past_first: false,
            saved_calls: Vec::new(),
            running_name: None,
            gc_tracked: std::cell::Cell::new(false),
            gc_dtor_called: std::cell::Cell::new(false),
            current_ref: None,
            hook_key: None,
            frame_name_cache: None,
            frame_depth: 0,
            fin: Vec::new(),
            pending_finally_levels: 1,
            yield_from_result: OperandType::Unused,
            just_delegated: false,
            mem_charged,
            do_init: false,
            last_value: Value::Null,
            last_key: Value::Null,
            fe: Default::default(),
            closure: None,
            object_id: 0,
        }
    }

    /// Opcodes the generator executor implements itself: suspension and the
    /// exception / finally bookkeeping kept in the generator frame.
    /// Everything else is delegated to the VM (Vm::execute_single_op), apart
    /// from the simple cases of local_fast_path().
    fn generator_owned_op(op: OpCode) -> bool {
        matches!(
            op,
            OpCode::Yield
                | OpCode::YieldFrom
                | OpCode::GeneratorReturn
                | OpCode::Return
                | OpCode::Throw
                | OpCode::TryBegin
                | OpCode::TryEnd
                | OpCode::CatchException
                | OpCode::SaveReturn
                | OpCode::SaveJump
                | OpCode::ReturnDeferred
                | OpCode::FinallyEnter
        )
    }

    /// Hot opcodes whose operands make them side-effect free (no warning,
    /// destructor, typed reference or symbol table involved): run them in the
    /// generator loop, with the same result as the VM's implementation.
    fn local_fast_path(&self, vm: &Vm, op: &crate::opcode::Op, literals: &[Value]) -> bool {
        let num = |v: &Value| matches!(v, Value::Long(_) | Value::Double(_));
        let undef_cv = |o: &OperandType| matches!(o, OperandType::Cv(i) if matches!(self.cvs.get(*i as usize), Some(Value::Undef) | None));
        match op.opcode {
            OpCode::Nop | OpCode::Jmp | OpCode::LoadConst => true,
            // Scalars only, no foreach state, no destructor pending.
            OpCode::FreeTmps => {
                !vm.has_release_work()
                    && self.fe.snap.is_empty()
                    && self.fe.obj.is_empty()
                    && self.fe.iter_handles.is_empty()
                    && match (op.op1, op.op2) {
                        (OperandType::Tmp(a), OperandType::Tmp(b)) => {
                            let end = (b as usize).min(self.tmps.len());
                            self.tmps[(a as usize).min(end)..end].iter().all(|v| {
                                !matches!(
                                    v,
                                    Value::Object(_)
                                        | Value::Array(_)
                                        | Value::Reference(_)
                                        | Value::Generator(_)
                                )
                            })
                        }
                        _ => true,
                    }
            }
            OpCode::QmAssign => !undef_cv(&op.op1),
            OpCode::JmpZ | OpCode::JmpNz => {
                !undef_cv(&op.op1)
                    && !matches!(self.read_operand(&op.op1, literals), Value::Double(d) if d.is_nan())
            }
            // A scalar replaces a scalar in a plain variable (no reference,
            // copy-on-write array, tracked object or release involved; see
            // Vm::write_operand).
            OpCode::Assign => {
                let scalar = |v: Option<&Value>| {
                    matches!(
                        v,
                        Some(
                            Value::Null
                                | Value::True
                                | Value::False
                                | Value::Long(_)
                                | Value::Double(_)
                                | Value::String(_)
                        )
                    )
                };
                let src = match op.op2 {
                    OperandType::Cv(i) => self.cvs.get(i as usize),
                    OperandType::Tmp(i) => self.tmps.get(i as usize),
                    OperandType::Const(i) => literals.get(i as usize),
                    _ => None,
                };
                self.symtab.is_none()
                    && matches!(op.op1, OperandType::Cv(i) if matches!(self.cvs.get(i as usize), Some(Value::Undef)) || scalar(self.cvs.get(i as usize)))
                    && scalar(src)
                    && !matches!(op.result, OperandType::Cv(_))
            }
            OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Less
            | OpCode::LessEqual
            | OpCode::Greater
            | OpCode::GreaterEqual
            | OpCode::Equal
            | OpCode::NotEqual => {
                num(&self.read_operand(&op.op1, literals))
                    && num(&self.read_operand(&op.op2, literals))
            }
            OpCode::Identical | OpCode::NotIdentical => {
                let ok = |v: &Value| {
                    matches!(
                        v,
                        Value::Long(_)
                            | Value::Double(_)
                            | Value::String(_)
                            | Value::True
                            | Value::False
                            | Value::Null
                    )
                };
                ok(&self.read_operand(&op.op1, literals))
                    && ok(&self.read_operand(&op.op2, literals))
            }
            OpCode::Concat => {
                let ok = |v: &Value| matches!(v, Value::Long(_) | Value::String(_));
                ok(&self.read_operand(&op.op1, literals))
                    && ok(&self.read_operand(&op.op2, literals))
            }
            OpCode::PreIncrement
            | OpCode::PostIncrement
            | OpCode::PreDecrement
            | OpCode::PostDecrement => {
                matches!(op.op1, OperandType::Cv(_))
                    && matches!(self.read_operand(&op.op1, literals), Value::Long(n) if n > i64::MIN + 1 && n < i64::MAX - 1)
            }
            OpCode::ArrayNew => true,
            _ => false,
        }
    }

    /// Was this generator destroyed while it ran in a suspended fiber (see
    /// Vm::forced_close_generators)?
    fn check_forced_close(&mut self, vm: &mut Vm) {
        if !self.force_closed && !vm.forced_close_generators.is_empty() {
            let me = self as *const PhpGenerator as usize;
            if let Some(i) = vm.forced_close_generators.iter().position(|&p| p == me) {
                vm.forced_close_generators.swap_remove(i);
                self.force_closed = true;
            }
        }
    }

    /// Values owned by the suspended generator frame (for the cycle collector).
    pub(crate) fn for_each_gc_child(&self, f: &mut dyn FnMut(&Value)) {
        for v in self
            .cvs
            .iter()
            .chain(self.tmps.iter())
            .chain(self.args.iter())
        {
            f(v);
        }
        for v in [
            &self.current_value,
            &self.current_key,
            &self.send_value,
            &self.return_value,
        ] {
            f(v);
        }
        for v in self
            .yield_from_source
            .iter()
            .chain(self.pending_return.iter())
            .chain(self.pending_exception.iter())
            .chain(self.closure.iter())
        {
            f(v);
        }
        for c in &self.saved_calls {
            for v in c.args.iter().chain(c.named_args.iter().map(|(_, v)| v)) {
                f(v);
            }
        }
        if let Some(r) = &self.current_ref {
            f(&Value::Reference(r.clone()));
        }
        f(&self.last_value);
        f(&self.last_key);
        for (_, e) in &self.fin {
            if let Some(e) = e {
                f(e);
            }
        }
        self.fe.for_each_value(f);
        // A local symbol table holds the dynamic variables (the global
        // one is not the frame's).
        if let Some(fs) = &self.symtab {
            if fs.owner && !fs.tab.global {
                f(&Value::Array(fs.tab.arr.clone()));
            }
            // (the table cells its CVs are bound to)
            for c in fs.bind.cells.iter().flatten() {
                f(&Value::Reference(c.clone()));
            }
        }
    }

    /// The frame's copies of its parameters' values (for backtraces, which
    /// read the CVs while it runs) are released first: the parameters are
    /// freed with the CVs, in declaration order.
    fn release_param_copies(&mut self) {
        let op = &self.op_array;
        let base = crate::internal_frames::implicit_args(op);
        let nfixed = (op.param_count as usize)
            .saturating_sub(base + usize::from(op.variadic_param.is_some()));
        for a in self.args.iter_mut().take(nfixed) {
            if matches!(a, Value::Object(_) | Value::Array(_) | Value::Reference(_)) {
                *a = Value::Null;
            }
        }
    }

    /// Detach everything the frame owns (the generator is garbage).
    pub(crate) fn take_gc_children(&mut self) -> Vec<Value> {
        let mut out: Vec<Value> = Vec::new();
        out.extend(std::mem::take(&mut self.cvs));
        out.extend(std::mem::take(&mut self.tmps));
        out.extend(std::mem::take(&mut self.args));
        out.push(std::mem::replace(&mut self.current_value, Value::Null));
        out.push(std::mem::replace(&mut self.current_key, Value::Null));
        out.push(std::mem::replace(&mut self.send_value, Value::Null));
        out.push(std::mem::replace(&mut self.return_value, Value::Null));
        out.extend(self.yield_from_source.take());
        out.extend(self.pending_return.take());
        out.extend(self.pending_exception.take());
        out.extend(self.closure.take());
        for c in std::mem::take(&mut self.saved_calls) {
            out.extend(c.args);
            out.extend(c.named_args.into_iter().map(|(_, v)| v));
        }
        if let Some(r) = self.current_ref.take() {
            out.push(Value::Reference(r));
        }
        out.push(std::mem::replace(&mut self.last_value, Value::Null));
        out.push(std::mem::replace(&mut self.last_key, Value::Null));
        out.extend(
            std::mem::take(&mut self.fin)
                .into_iter()
                .filter_map(|(_, e)| e),
        );
        self.fe.take_values(&mut out);
        self.state = GeneratorState::Completed;
        out
    }

    /// Force-close the generator, running any finally blocks.
    /// Called when a suspended generator is destroyed (e.g. unset or goes out of scope).
    pub fn close(&mut self, vm: &mut Vm) {
        if self.state != GeneratorState::Suspended {
            return;
        }
        if self.is_running {
            return;
        }
        // The root of our `yield from` tree is running (suspended in a
        // fiber): we are unwound with it instead.
        let mut cur = self.live_delegate();
        while let Some(g) = cur {
            match g.try_borrow() {
                Ok(b) if !b.is_running && b.state != GeneratorState::Completed => {
                    cur = b.live_delegate();
                }
                Ok(b) if !b.is_running => break,
                _ => {
                    self.force_closed = true;
                    return;
                }
            }
        }

        self.force_closed = true;

        // Exceptions pending in the finally blocks we are suspended in are
        // raised again once a finally block run by the close completes
        // (zend_dispatch_try_catch_finally_helper() from ZEND_FAST_RET).
        let mut resurrect: Option<Value> = None;
        for (_, e) in self.fin.iter().rev() {
            if let Some(e) = e {
                match &resurrect {
                    None => resurrect = Some(e.clone()),
                    Some(x) => Vm::exception_set_previous(x, e.clone()),
                }
            }
        }

        // Leave `yield from` mode: the reference to the delegate is released
        // first, so a delegate only we held is destroyed (running its
        // finally blocks) before ours run (zend_generator_dtor_storage()).
        if let Some(src) = self.yield_from_source.take() {
            let delegate = matches!(src, Value::Generator(_));
            drop(src);
            if delegate {
                vm.close_released_generators();
            }
        }
        self.fin.clear();

        // Find finally blocks that need to be executed.
        // Scan TryBegin/TryEnd in the op_array to find finally targets
        // that encompass the current IP position.
        let current_ip = self.ip;
        let op_array = &self.op_array;

        // Collect all try-finally ranges: (try_begin_ip, finally_target)
        // A try block runs from try_begin_ip until we find TryEnd or
        // reach the catch/finally target.
        let mut finally_targets = Vec::new();
        let mut try_stack: Vec<(usize, u32)> = Vec::new();

        for (i, op) in op_array.ops.iter().enumerate() {
            match op.opcode {
                OpCode::TryBegin => {
                    if let OperandType::JmpTarget(finally_target) = op.op2 {
                        if finally_target > 0 {
                            // (a try without finally jumps to its end
                            // instead: 0, no finally to run)
                            let is_finally =
                                op_array.ops.get(finally_target as usize).map(|o| o.opcode)
                                    == Some(OpCode::FinallyEnter);
                            try_stack.push((i, if is_finally { finally_target } else { 0 }));
                        }
                    }
                }
                OpCode::TryEnd => {
                    // Pop the matching TryBegin
                    if let Some((begin, finally_target)) = try_stack.pop() {
                        // Check if current_ip is within the try block range
                        if current_ip > begin && current_ip <= i && finally_target > 0 {
                            finally_targets.push(finally_target);
                        }
                    }
                }
                _ => {}
            }
        }

        // Also check try blocks that are still open (not TryEnd'd because
        // the generator suspended before reaching TryEnd)
        for (begin, finally_target) in &try_stack {
            if current_ip > *begin && *finally_target > 0 {
                finally_targets.push(*finally_target);
            }
        }

        if finally_targets.is_empty() {
            self.state = GeneratorState::Completed;
            self.current_value = Value::Null;
            return;
        }

        // Sort finally targets so innermost (smallest) runs first
        finally_targets.sort();
        finally_targets.dedup();

        // Jump to the innermost finally block
        if let Some(&first_finally) = finally_targets.first() {
            // zend_generator_cleanup_unfinished_execution(): the live
            // temporaries (foreach loop variables, arguments of calls in
            // progress) are freed before the finally block runs.
            let mut freed = !self.saved_calls.is_empty();
            self.saved_calls.clear();
            for t in self.tmps.iter_mut() {
                if matches!(t, Value::Object(_) | Value::Array(_) | Value::Reference(_)) {
                    freed = true;
                }
                *t = Value::Undef;
            }
            self.fe = Default::default();
            if freed {
                vm.run_pending_destructors_pub();
            }
            self.ip = first_finally as usize;
            // Clear exception handlers and set up for finally execution
            self.exception_handlers.clear();

            // Set up the enclosing finally targets as exception handlers in
            // reverse order (outermost at bottom of stack) so ReturnDeferred
            // chains to the next one. The finally block being entered is not
            // a handler for itself: an exception thrown inside it must go to
            // the enclosing finally (or out), not re-run it.
            for ft in finally_targets.iter().skip(1).rev() {
                self.exception_handlers.push((0, *ft, 0));
            }

            // Mark that we're doing a deferred return (force close)
            self.pending_return = Some(Value::Null);
            self.state = GeneratorState::Suspended; // Keep suspended so resume works

            // Resume to execute the finally block(s). If the finally produced
            // an Error (e.g., "Cannot yield from finally in a force-closed
            // generator"), leave the exception on the VM so the caller can
            // catch it (or surface it as a fatal error at the top level).
            let _ = self.resume(vm);
            if let Some(r) = resurrect {
                match &vm.current_exception {
                    Some(cur) => Vm::exception_set_previous(cur, r),
                    None => vm.current_exception = Some(r),
                }
            }
        }

        self.state = GeneratorState::Completed;
        self.current_value = Value::Null;
        // Drop any temporaries / CVs we held while suspended. This lets
        // partially-constructed objects (e.g. `new C(yield)` interrupted at
        // yield) destruct on close. Run pending object destructors so the
        // user-visible __destruct fires before close() returns.
        for t in self.tmps.iter_mut() {
            *t = Value::Undef;
        }
        for c in self.cvs.iter_mut() {
            *c = Value::Undef;
        }
        vm.run_pending_destructors_pub();
    }

    /// After a resume: the value sent in now lives in the frame, and a
    /// finished generator's frame is freed, destroying its locals
    /// (zend_generator_close()).
    fn release_finished_frame(&mut self, vm: &mut Vm) {
        if !matches!(self.send_value, Value::Null) {
            self.send_value = Value::Null;
        }
        if self.state == GeneratorState::Completed {
            let mut freed = false;
            self.release_param_copies();
            for v in self.tmps.iter_mut().chain(self.cvs.iter_mut()) {
                if matches!(v, Value::Object(_) | Value::Array(_) | Value::Reference(_)) {
                    freed = true;
                }
                *v = Value::Undef;
            }
            self.current_value = Value::Null;
            if freed {
                // Destructors run from the frame that resumed the generator.
                if let Some((_, line)) = vm.generator_caller.as_ref().filter(|c| c.1 > 0) {
                    vm.current_line = *line;
                }
                vm.run_pending_destructors_pub();
            }
        }
    }

    /// Resume execution of the generator until the next yield or return.
    /// Returns Ok(true) if a value was yielded, Ok(false) if the generator completed.
    /// Name of the generator function's frame in backtraces, and whether it
    /// is an instance method.
    /// The generator function's `$this`.
    fn this_obj(&self) -> Option<Value> {
        let i = self
            .op_array
            .cv_names
            .iter()
            .position(|n| n.as_slice() == b"this")?;
        match self.cvs.get(i).map(|v| v.deref()) {
            Some(v @ Value::Object(_)) => Some(v),
            _ => None,
        }
    }

    fn frame_name(&self, vm: &Vm) -> (String, bool) {
        let op = &self.op_array;
        let raw_name = String::from_utf8_lossy(&op.name).to_string();
        let is_instance = op.cv_names.first().map(|n| n.as_slice()) == Some(b"this");
        // Format closure names for stack traces using the canonical display name.
        let is_closure = (raw_name.starts_with("__closure_")
            || raw_name.starts_with("__arrow_")
            || raw_name.starts_with("__bound_closure_"))
            && !raw_name.starts_with("__closure_fcc_");
        let name = if !op.display_name.is_empty() || is_closure {
            let n = if !op.display_name.is_empty() {
                String::from_utf8_lossy(&op.display_name).to_string()
            } else {
                let file = String::from_utf8_lossy(&op.filename).to_string();
                format!("{{closure:{}:{}}}", file, op.decl_line)
            };
            // A closure's frame is its scope's (Class->{closure} with a
            // `$this`, Class::{closure} without).
            match (
                is_closure,
                self.class_scope.as_ref().or(op.scope_class.as_ref()),
            ) {
                (true, Some(scope)) if !scope.is_empty() => {
                    let class = vm
                        .classes
                        .get(scope)
                        .map(|c| String::from_utf8_lossy(&c.name).to_string())
                        .unwrap_or_else(|| String::from_utf8_lossy(scope).to_string());
                    return (format!("{}::{}", class, n), self.this_obj().is_some());
                }
                _ => n,
            }
        } else if let (Some(scope), false) = (&op.scope_class, raw_name.contains("::")) {
            // A generator method: Class::method, as for other method frames
            let class = vm
                .classes
                .get(scope)
                .map(|c| String::from_utf8_lossy(&c.name).to_string())
                .unwrap_or_else(|| String::from_utf8_lossy(scope).to_string());
            format!("{}::{}", class, raw_name)
        } else {
            raw_name
        };
        (name, is_instance)
    }

    /// Record the value/key this generator holds while it runs.
    #[inline(never)]
    /// generator_function_name_of() of this generator, kept for its next
    /// runs.
    fn running_function_name(&mut self, vm: &Vm) -> Rc<str> {
        if let Some(n) = &self.running_name {
            return n.clone();
        }
        let n: Rc<str> = Rc::from(vm.generator_function_name_of(self));
        self.running_name = Some(n.clone());
        n
    }

    /// (moved there while it runs: the body does not read them, a yield
    /// replaces them)
    fn push_running_values(&mut self, vm: &mut Vm) {
        let vals = (self.state != GeneratorState::Created).then(|| {
            (
                std::mem::replace(&mut self.current_value, Value::Undef),
                std::mem::replace(&mut self.current_key, Value::Undef),
            )
        });
        vm.running_gen_values
            .push((self as *const PhpGenerator as usize, vals));
    }

    /// The values return to a generator that did not yield new ones.
    #[inline(never)]
    fn pop_running_values(&mut self, vm: &mut Vm) {
        let addr = self as *const PhpGenerator as usize;
        let entry = match vm.running_gen_values.last() {
            Some((a, _)) if *a == addr => vm.running_gen_values.pop(),
            _ => match vm.running_gen_values.iter().rposition(|(a, _)| *a == addr) {
                Some(i) => Some(vm.running_gen_values.remove(i)),
                None => None,
            },
        };
        if let Some((_, Some((v, k)))) = entry {
            if matches!(self.current_value, Value::Undef) && self.state != GeneratorState::Completed
            {
                self.current_value = v;
                self.current_key = k;
            }
        }
    }

    fn resume_single(&mut self, vm: &mut Vm) -> Result<bool, VmError> {
        if self.state == GeneratorState::Completed {
            return Ok(false);
        }
        if self.is_running {
            let err_msg = "Cannot resume an already running generator";
            let exc = vm.create_exception(b"Error", err_msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: format!("Uncaught Error: {}", err_msg),
                line: 0,
            });
        }

        // Track call depth to prevent stack overflow from recursive yield-from
        vm.enter_generator_resume(0)?;

        // Push class context for get_class() / get_called_class() inside generators
        let pushed_scope = if let Some(ref scope) = self.class_scope {
            vm.push_class_scope(scope.clone());
            true
        } else {
            // The function's own scope (none for a function outside any
            // class: the resumer's does not carry over).
            vm.push_class_scope(self.op_array.scope_class.clone().unwrap_or_default());
            true
        };
        let pushed_called = if let Some(ref called) = self.called_class {
            vm.called_class_stack.push(called.clone());
            true
        } else {
            false
        };

        // Push a call_stack entry for func_get_args() support
        if self.frame_name_cache.is_none() {
            self.frame_name_cache = Some(self.frame_name(vm));
        }
        let (func_name, is_instance) = self.frame_name_cache.clone().unwrap_or_default();
        // The generator's frame in stack traces should reflect the most recent caller.
        // When VM resumes the generator with caller context set (e.g. foreach), we use
        // the caller's file/line; otherwise (Generator method calls, yield-from priming)
        // we show "[internal function]" (empty file, line 0).
        let (func_file, func_line) = if let Some((file, line)) = vm.generator_caller.clone() {
            (file, line)
        } else {
            (crate::rcstr::RcStr::new(), 0)
        };
        self.frame_depth = vm.call_stack.len().saturating_sub(vm.fiber_call_base());
        if let Some(Value::Object(c)) = &self.closure {
            // (Closure::getCurrent() in the body)
            vm.note_generator_closure_frame(&func_name, c);
        }
        vm.call_stack.push((
            func_name,
            func_file,
            func_line,
            self.args.clone(),
            is_instance,
        ));
        if is_instance {
            if let Some(t) = self.this_obj() {
                vm.note_frame_this(&t);
            }
        }

        // Restore a deferred exception (yield-in-finally) if we stashed one
        // when this generator suspended. The body's ReturnDeferred will pick
        // it up after the finally completes and re-raise through any outer
        // handlers (or out of the generator).
        if let Some(exc) = self.pending_exception.take() {
            vm.current_exception = Some(exc);
        }

        self.is_running = true;
        // The running frame's values (a fiber suspended inside it owns them).
        vm.gc_frames.push(crate::gc::GcFrame::for_generator(self));
        let fname = self.running_function_name(vm);
        vm.running_generators
            .push((self as *const PhpGenerator as usize, fname, self.object_id));
        self.push_running_values(vm);
        let cv_frame = vm.register_cv_frame(&self.op_array);
        if cv_frame {
            vm.set_cv_frame_vars(&self.cvs);
        }
        let saved_file = vm.enter_op_array_file(&self.op_array);
        let outer_exec = std::mem::replace(&mut vm.exec_frame_len, vm.call_stack.len());
        let result = self.resume_inner_observed(vm);
        vm.exec_frame_len = outer_exec;
        vm.leave_op_array_file(saved_file);
        if let Ok(true) = result {
            self.last_value = self.current_value.clone();
            self.last_key = self.current_key.clone();
        }
        if cv_frame {
            vm.cv_frames.pop();
        }
        self.is_running = false;
        vm.gc_frames.pop();
        vm.running_generators.pop();
        self.pop_running_values(vm);

        // Pop the call_stack entry (with the frames of the calls an
        // exception left)
        vm.call_stack
            .truncate(self.frame_depth + vm.fiber_call_base());

        // Pop class context
        if pushed_called {
            vm.called_class_stack.pop();
        }
        if pushed_scope {
            vm.pop_class_scope();
        }

        vm.leave_generator_resume();
        // The finished frame is freed once the generator's frame is left.
        self.release_finished_frame(vm);

        result
    }

    /// The generator this one delegates to with `yield from` (node.parent).
    fn live_delegate(&self) -> Option<GenRc> {
        match &self.yield_from_source {
            Some(Value::Generator(g)) => Some(g.clone()),
            _ => None,
        }
    }

    /// File and line of the `yield from` this generator is suspended at.
    fn yield_from_site(&self) -> (crate::rcstr::RcStr, u32) {
        let file = self.op_array.file_name();
        let line = self
            .ip
            .checked_sub(1)
            .and_then(|i| self.op_array.ops.get(i))
            .map(|o| o.line)
            .unwrap_or(0);
        (file, line)
    }

    /// This generator's frame in backtraces, called from `caller`.
    fn trace_frame(
        &self,
        vm: &Vm,
        caller: &Option<(crate::rcstr::RcStr, u32)>,
    ) -> crate::vm::CallFrame {
        let (name, is_instance) = self.frame_name(vm);
        let (file, line) = caller.clone().unwrap_or_default();
        (name, file, line, self.args.clone(), is_instance)
    }

    /// zend_generator_resume(): run the generator, or the innermost
    /// generator of its `yield from` delegation chain, until a value is
    /// produced. Delegation is driven iteratively (no native recursion per
    /// `yield from` level). Returns Ok(true) if a value was yielded,
    /// Ok(false) if the generator completed.
    pub fn resume(&mut self, vm: &mut Vm) -> Result<bool, VmError> {
        if self.state == GeneratorState::Completed {
            return Ok(false);
        }
        if self.is_running {
            return self.resume_single(vm);
        }
        if self.live_delegate().is_some() {
            return self.drive(vm, Kick::Resume);
        }
        let r = self.resume_single(vm);
        if std::mem::take(&mut self.just_delegated) && r.is_ok() {
            return self.drive(vm, Kick::Fresh);
        }
        r
    }

    /// Resume with the exception in vm.current_exception thrown at the
    /// point the generator (or the innermost generator it delegates to) is
    /// suspended (Generator::throw(), Fiber::throw()).
    pub fn resume_with_exception(&mut self, vm: &mut Vm) -> Result<bool, VmError> {
        if self.state == GeneratorState::Completed {
            return Ok(false);
        }
        if self.live_delegate().is_some() {
            return self.drive(vm, Kick::Throw);
        }
        let r = self.resume_single_throw(vm);
        if std::mem::take(&mut self.just_delegated) && r.is_ok() {
            return self.drive(vm, Kick::Fresh);
        }
        r
    }

    /// The delegation loop of zend_generator_resume() (try_again): resume the
    /// root of the tree; when it finishes, continue its child with the
    /// return value (or the exception) until a value is yielded or `self`
    /// itself finishes.
    fn drive(&mut self, vm: &mut Vm, kick: Kick) -> Result<bool, VmError> {
        // While the delegate runs, a fiber suspended inside it owns what
        // this generator holds too.
        let mark = vm.gc_frames.len();
        vm.gc_frames.push(crate::gc::GcFrame::for_generator(self));
        let r = self.drive_inner(vm, kick);
        // (with the pins drive_inner() left)
        vm.gc_frames.truncate(mark);
        r
    }

    fn drive_inner(&mut self, vm: &mut Vm, mut kick: Kick) -> Result<bool, VmError> {
        let self_ptr = self as *const PhpGenerator;
        // Path from our delegate down to the root.
        let mut path: Vec<GenRc> = Vec::new();
        let mut cur = self.live_delegate();
        while let Some(g) = cur {
            if std::ptr::eq(RefCell::as_ptr(&g) as *const PhpGenerator, self_ptr) {
                break;
            }
            cur = match g.try_borrow() {
                Ok(b) if b.state != GeneratorState::Completed => b.live_delegate(),
                _ => None,
            };
            path.push(g);
        }
        let base_cs = vm.call_stack.len();
        // (the generators between us and the root are running too: their
        // frames are on the stack, ReflectionGenerator sees them)
        let base_rg = vm.running_generators.len();
        let outer_caller = vm.generator_caller.clone();
        // The frames of the generators between us and the root appear in
        // backtraces taken while the root runs.
        let f = self.trace_frame(vm, &outer_caller);
        let inst = f.4;
        vm.call_stack.push(f);
        if let (true, Some(t)) = (inst, self.this_obj()) {
            vm.note_frame_this(&t);
        }
        vm.push_delegating_generator(self);
        for i in 0..path.len().saturating_sub(1) {
            let site = if i == 0 {
                self.yield_from_site()
            } else {
                path[i - 1].borrow().yield_from_site()
            };
            let f = path[i].borrow().trace_frame(vm, &Some(site));
            let inst = f.4;
            vm.call_stack.push(f);
            if inst {
                let t = path[i].borrow().this_obj();
                if let Some(t) = t {
                    vm.note_frame_this(&t);
                }
            }
            vm.push_delegating_generator(&path[i].borrow());
        }
        // The generators the loop holds belong to a fiber suspended in it
        // (unpinned by drive()).
        let mut root_slot: Option<GenRc> = None;
        vm.gc_pin(&path);
        vm.gc_pin(&root_slot);
        let result = loop {
            root_slot = path.last().cloned();
            let Some(root) = root_slot.as_ref() else {
                // We are the root again: continue our own body.
                vm.call_stack.truncate(base_cs);
                vm.running_generators.truncate(base_rg);
                vm.generator_caller = outer_caller.clone();
                let r = match kick {
                    Kick::Throw => self.resume_single_throw(vm),
                    _ => self.resume_single(vm),
                };
                if std::mem::take(&mut self.just_delegated) && r.is_ok() {
                    let f = self.trace_frame(vm, &outer_caller);
                    vm.call_stack.push(f);
                    vm.push_delegating_generator(self);
                    if let Some(d) = self.live_delegate() {
                        path.push(d);
                    }
                    kick = Kick::Fresh;
                    continue;
                }
                vm.generator_caller = outer_caller;
                self.do_init = false;
                return r;
            };
            let (completed, returned, retval, last) = match root.try_borrow() {
                Ok(b) => (
                    b.state == GeneratorState::Completed,
                    b.has_returned,
                    b.return_value.clone(),
                    (b.last_value.clone(), b.last_key.clone()),
                ),
                Err(_) => break Err(Self::running_error(vm)),
            };
            if completed {
                // zend_generator_update_current(): the child continues
                // after its `yield from`. Its frame is pushed again when it
                // runs.
                path.pop();
                vm.call_stack.pop();
                if vm.running_generators.len() > base_rg {
                    vm.running_generators.pop();
                }
                let closed = !returned && !matches!(kick, Kick::Throw);
                let child = path.last().cloned();
                let mut child_ref;
                let c: &mut PhpGenerator = match &child {
                    Some(c) => {
                        child_ref = match c.try_borrow_mut() {
                            Ok(cb) => cb,
                            Err(_) => break Err(Self::running_error(vm)),
                        };
                        &mut child_ref
                    }
                    None => self,
                };
                c.yield_from_source = None;
                if returned {
                    let res = c.yield_from_result;
                    c.write_operand(&res, retval);
                    c.current_value = last.0;
                    c.current_key = last.1;
                } else if closed {
                    // Thrown in the context of the child.
                    let site = if path.len() >= 2 {
                        Some(path[path.len() - 2].borrow().yield_from_site())
                    } else if child.is_some() {
                        None
                    } else {
                        outer_caller.clone()
                    };
                    let f = c.trace_frame(vm, &site);
                    vm.call_stack.push(f);
                    let exc = vm.create_exception(
                        b"ClosedGeneratorException",
                        "Generator yielded from aborted, no return value available",
                        0,
                    );
                    vm.call_stack.pop();
                    vm.current_exception = Some(exc);
                    kick = Kick::Throw;
                }
                if matches!(kick, Kick::Fresh) {
                    kick = Kick::Resume;
                }
                continue;
            }
            let mut rb = match root.try_borrow_mut() {
                Ok(rb) => rb,
                Err(_) => break Err(Self::running_error(vm)),
            };
            if rb.is_running {
                drop(rb);
                break Err(Self::running_error(vm));
            }
            if self.do_init && !matches!(kick, Kick::Throw) && rb.state == GeneratorState::Suspended
            {
                // We started delegating to a generator that already produced
                // a value: it is not advanced (ZEND_GENERATOR_DO_INIT).
                self.do_init = false;
                drop(rb);
                break Ok(true);
            }
            let site = if path.len() >= 2 {
                path[path.len() - 2].borrow().yield_from_site()
            } else {
                self.yield_from_site()
            };
            vm.generator_caller = Some(site.clone());
            let r = match kick {
                Kick::Throw => rb.resume_single_throw(vm),
                Kick::Resume => {
                    rb.write_send_value();
                    rb.resume_single(vm)
                }
                Kick::Fresh => rb.resume_single(vm),
            };
            vm.generator_caller = outer_caller.clone();
            if vm.exit_requested || vm.fiber_suspended {
                drop(rb);
                break r;
            }
            if crate::value::memory_exhausted() != 0 {
                drop(rb);
                break Err(vm.memory_limit_fatal());
            }
            if std::mem::take(&mut rb.just_delegated) && r.is_ok() {
                let f = rb.trace_frame(vm, &Some(site));
                vm.call_stack.push(f);
                vm.push_delegating_generator(&rb);
                let d = rb.live_delegate();
                drop(rb);
                if let Some(d) = d {
                    path.push(d);
                }
                kick = Kick::Fresh;
                continue;
            }
            drop(rb);
            match r {
                Ok(true) => break Ok(true),
                Ok(false) => {
                    kick = if vm.current_exception.is_some() {
                        Kick::Throw
                    } else {
                        Kick::Resume
                    };
                }
                Err(e) => {
                    if vm.current_exception.is_some() {
                        kick = Kick::Throw;
                    } else {
                        break Err(e);
                    }
                }
            }
        };
        vm.call_stack.truncate(base_cs);
        vm.running_generators.truncate(base_rg);
        vm.generator_caller = outer_caller;
        self.do_init = false;
        if let Ok(true) = result {
            // The value of every generator on the path is the root's.
            if let Some(root) = path.last() {
                let (v, k, r) = match root.try_borrow() {
                    Ok(b) => (
                        b.current_value.clone(),
                        b.current_key.clone(),
                        b.current_ref.clone(),
                    ),
                    Err(_) => (Value::Null, Value::Null, None),
                };
                for g in &path[..path.len() - 1] {
                    if let Ok(mut gb) = g.try_borrow_mut() {
                        gb.current_value = v.clone();
                        gb.current_key = k.clone();
                        gb.current_ref = r.clone();
                    }
                }
                self.current_value = v;
                self.current_key = k;
                self.current_ref = r;
            }
            self.state = GeneratorState::Suspended;
        }
        result
    }

    /// Pop the innermost exception handler: where an exception raised in the
    /// body continues (its catch, or its finally block).
    fn dispatch_exception(&mut self) -> Option<usize> {
        let (catch_target, finally_target, _) = self.exception_handlers.pop()?;
        if catch_target > 0 {
            Some(catch_target as usize)
        } else if finally_target > 0 {
            Some(finally_target as usize)
        } else {
            None
        }
    }

    /// "Cannot resume an already running generator", thrown in the caller.
    fn running_error(vm: &mut Vm) -> VmError {
        let err_msg = "Cannot resume an already running generator";
        let exc = vm.create_exception(b"Error", err_msg, 0);
        vm.current_exception = Some(exc);
        VmError {
            message: format!("Uncaught Error: {}", err_msg),
            line: 0,
        }
    }

    /// For `SaveReturn`/`SaveJump` followed by `Jmp <finally_start>`: pop the
    /// exception handlers up to and including the one whose finally block is
    /// the jump target, so exceptions thrown in the finally propagate outward.
    fn pop_handlers_for_finally_jump(&mut self, next: Option<&crate::opcode::Op>) {
        let Some(next) = next else { return };
        if next.opcode != OpCode::Jmp {
            return;
        }
        let OperandType::JmpTarget(target) = next.op1 else {
            return;
        };
        if target == 0 || !self.exception_handlers.iter().any(|h| h.1 == target) {
            return;
        }
        while let Some(h) = self.exception_handlers.pop() {
            if h.1 == target {
                break;
            }
        }
    }

    /// Inner resume implementation (depth check is done by the caller)
    /// Run the body until it suspends or finishes, with the observer's
    /// begin/end hooks around it: a generator is observed per resumption,
    /// its end getting the value yielded or returned. (A generator
    /// delegating with `yield from` is suspended while its leaf runs.)
    fn resume_inner_observed(&mut self, vm: &mut Vm) -> Result<bool, VmError> {
        let Some(h) = vm.observer else {
            return match crate::stack::with_room(|| self.resume_inner(vm)) {
                Ok(r) => r,
                Err(crate::stack::Exhausted) => Err(vm.stack_exhausted_error(false)),
            };
        };
        let op = self.op_array.clone();
        (h.begin)(vm, &crate::vm::ObservedFrame::User(&op));
        let result = match crate::stack::with_room(|| self.resume_inner(vm)) {
            Ok(r) => r,
            Err(crate::stack::Exhausted) => Err(vm.stack_exhausted_error(false)),
        };
        let rv = match &result {
            // (a generator delegating with `yield from` keeps its value,
            // moved aside while it runs)
            Ok(true) if matches!(self.current_value, Value::Undef) => {
                let addr = self as *const PhpGenerator as usize;
                vm.running_gen_values
                    .iter()
                    .rev()
                    .find(|(a, _)| *a == addr)
                    .and_then(|(_, v)| v.as_ref().map(|(v, _)| v.deref()))
                    .or(Some(Value::Null))
            }
            Ok(true) => Some(self.current_value.deref()),
            Ok(false) => Some(self.return_value.deref()),
            Err(_) => None,
        };
        (h.end)(vm, &crate::vm::ObservedFrame::User(&op), rv.as_ref());
        result
    }

    fn resume_inner(&mut self, vm: &mut Vm) -> Result<bool, VmError> {
        if self.state == GeneratorState::Created {
            if let Some(i) = self.op_array.cv_names.iter().position(|n| n == b"GLOBALS") {
                if let Some(slot) = self.cvs.get_mut(i) {
                    *slot = Value::Array(vm.globals_array());
                }
            }
            // Auto-globals ($_GET, $_SERVER, ...) are the global slots.
            for (i, cv_name) in self.op_array.cv_names.iter().enumerate() {
                if crate::vm::is_auto_global_name(cv_name)
                    && matches!(self.cvs.get(i), Some(Value::Undef))
                    && vm.global_exists(cv_name)
                {
                    self.cvs[i] = Value::Reference(vm.global_cell(cv_name));
                }
            }
        }
        // The generator body is a function frame, whatever frame resumes it.
        let was_global = vm.is_global_scope_flag();
        vm.set_global_scope_flag(false);
        let depth = vm.pending_calls_depth();
        vm.restore_pending_calls(std::mem::take(&mut self.saved_calls));
        if let Some(k) = &self.hook_key {
            vm.property_hook_stack.push(k.clone());
        }
        let result = self.resume_body(vm);
        // An exception leaving the generator from a finally block entered
        // for another one: that one becomes its previous.
        if result.is_err() && !self.fin.is_empty() && self.state == GeneratorState::Completed {
            if let Some(cur) = &vm.current_exception {
                while let Some((_, e)) = self.fin.pop() {
                    if let Some(prev) = e {
                        Vm::exception_set_previous(cur, prev);
                    }
                }
            }
        }
        if self.hook_key.is_some() {
            vm.property_hook_stack.pop();
        }
        let leftover = vm.take_pending_calls_from(depth);
        if self.state != GeneratorState::Completed {
            self.saved_calls = leftover;
        }
        vm.set_global_scope_flag(was_global);
        result
    }

    fn resume_body(&mut self, vm: &mut Vm) -> Result<bool, VmError> {
        let op_array = self.op_array.clone();
        let mut ip = self.ip;

        loop {
            if vm.exit_requested {
                self.state = GeneratorState::Completed;
                self.ip = ip;
                return Ok(false);
            }
            if ip >= op_array.ops.len() {
                self.state = GeneratorState::Completed;
                self.has_returned = true;
                self.ip = ip;
                return Ok(false);
            }

            let op = &op_array.ops[ip];
            ip += 1;

            if !Self::generator_owned_op(op.opcode)
                && !self.local_fast_path(vm, op, &op_array.literals)
            {
                // Shared semantics: run the VM's implementation of the opcode
                // on this frame's variables.
                match self.run_on_vm(vm, &op_array, ip) {
                    Ok(next) => ip = next,
                    Err(done) => return done,
                }
                continue;
            }

            match op.opcode {
                OpCode::Yield => {
                    self.check_forced_close(vm);
                    // If generator is force-closed, yield in finally is an error
                    if self.force_closed {
                        let err_msg = "Cannot yield from finally in a force-closed generator";
                        let exc = vm.create_exception(b"Error", err_msg, op.line);
                        vm.current_exception = Some(exc);
                        // (the try/finally blocks around it still run)
                        if let Some(t) = self.dispatch_exception() {
                            ip = t;
                            continue;
                        }
                        self.state = GeneratorState::Completed;
                        self.current_value = Value::Null;
                        self.ip = ip;
                        return Err(VmError {
                            message: format!("Uncaught Error: {}", err_msg),
                            line: op.line,
                        });
                    }
                    // op1 = value to yield (or Unused for bare yield)
                    // op2 = key to yield (or Unused for auto-key)
                    // result = where to store the sent value (or Unused)
                    let yielded_value = if matches!(op.op1, OperandType::Unused) {
                        Value::Null
                    } else if let OperandType::Tmp(t) = op.op1 {
                        // YIELD consumes a temporary operand (the generator
                        // holds the value from now on).
                        match self.tmps.get_mut(t as usize) {
                            Some(slot) if matches!(slot, Value::Object(_)) => {
                                std::mem::replace(slot, Value::Undef)
                            }
                            _ => self.read_operand(&op.op1, &op_array.literals),
                        }
                    } else if op_array.returns_by_ref {
                        self.read_operand(&op.op1, &op_array.literals)
                    } else {
                        self.read_operand_r(vm, &op.op1, &op_array.literals, op.line)
                    };

                    let yielded_key = if matches!(op.op2, OperandType::Unused) {
                        let key = Value::Long(self.key_counter);
                        self.key_counter += 1;
                        key
                    } else {
                        let key = self.read_operand_r(vm, &op.op2, &op_array.literals, op.line);
                        // Update key_counter to be after the explicit key
                        if let Value::Long(n) = &key {
                            if *n >= self.key_counter {
                                self.key_counter = *n + 1;
                            }
                        }
                        key
                    };

                    // `function &gen()` yields the variable's reference.
                    self.current_ref = None;
                    if op_array.returns_by_ref {
                        let raw = match op.op1 {
                            OperandType::Cv(i) => self.cvs.get(i as usize).cloned(),
                            OperandType::Tmp(i) => self.tmps.get(i as usize).cloned(),
                            _ => None,
                        };
                        if let Some(Value::Reference(r)) = raw {
                            self.current_ref = Some(r);
                        }
                    }
                    self.current_value = yielded_value;
                    self.current_key = yielded_key;
                    self.state = GeneratorState::Suspended;
                    self.ip = ip;

                    // If we are yielding while an exception is in flight (e.g. yield
                    // inside a `finally` block), stash the exception so the caller's
                    // iteration sees a clean state. We'll restore it on resume so
                    // ReturnDeferred can re-raise after the finally completes.
                    if let Some(exc) = vm.current_exception.take() {
                        self.pending_exception = Some(exc);
                    }

                    // The send value will be written to the result operand when we resume
                    return Ok(true);
                }

                OpCode::YieldFrom => {
                    self.check_forced_close(vm);
                    // If generator is force-closed, yield-from in finally is an error.
                    if self.force_closed && self.yield_from_source.is_none() {
                        let err_msg = "Cannot use \"yield from\" in a force-closed generator";
                        let exc = vm.create_exception(b"Error", err_msg, op.line);
                        vm.current_exception = Some(exc);
                        // (the try/finally blocks around it still run)
                        if let Some(t) = self.dispatch_exception() {
                            ip = t;
                            continue;
                        }
                        self.state = GeneratorState::Completed;
                        self.current_value = Value::Null;
                        self.ip = ip;
                        return Err(VmError {
                            message: format!("Uncaught Error: {}", err_msg),
                            line: op.line,
                        });
                    }
                    // Error raised by the `yield from` itself: dispatched to
                    // this frame's handlers.
                    let mut error: Option<(&[u8], String)> = None;
                    if let Some(source) = self.yield_from_source.clone() {
                        // Resuming an array / iterator delegation
                        // (zend_generator_get_next_delegated_value()).
                        match source {
                            Value::Array(arr) => {
                                let next = {
                                    let arr_borrow = arr.borrow();
                                    arr_borrow
                                        .iter()
                                        .nth(self.yield_from_pos)
                                        .map(|(key, val)| {
                                            let k = match key {
                                                crate::array::ArrayKey::Int(n) => Value::Long(*n),
                                                crate::array::ArrayKey::String(s) => {
                                                    Value::String(s.clone())
                                                }
                                            };
                                            (k, val.clone())
                                        })
                                };
                                if let Some((k, val)) = next {
                                    self.current_value = val;
                                    self.current_key = k;
                                    self.yield_from_pos += 1;
                                    self.state = GeneratorState::Suspended;
                                    self.ip = ip - 1; // Re-execute YieldFrom on next resume
                                    return Ok(true);
                                }
                                // Array exhausted
                                self.yield_from_source = None;
                                self.yield_from_pos = 0;
                                self.write_operand(&op.result, Value::Null);
                            }
                            Value::Object(_) => {
                                let idx = self.yield_from_pos;
                                self.yield_from_pos += 1;
                                vm.current_line = op.line;
                                let had = vm.current_exception.is_some();
                                let failed = |vm: &Vm| !had && vm.current_exception.is_some();
                                let mut item = None;
                                'step: {
                                    if idx > 0 {
                                        vm.spl_call_method_at(&source, b"next", &[], false);
                                        if failed(vm) {
                                            break 'step;
                                        }
                                    }
                                    let valid = vm
                                        .spl_call_method_at(&source, b"valid", &[], false)
                                        .unwrap_or(Value::False);
                                    if failed(vm) || !valid.is_truthy() {
                                        break 'step;
                                    }
                                    let value = vm
                                        .spl_call_method_at(&source, b"current", &[], false)
                                        .unwrap_or(Value::Null);
                                    if failed(vm) {
                                        break 'step;
                                    }
                                    let key = vm
                                        .spl_call_method_at(&source, b"key", &[], false)
                                        .unwrap_or(Value::Null);
                                    if failed(vm) {
                                        break 'step;
                                    }
                                    item = Some((key, value));
                                }
                                if let Some((key, value)) = item {
                                    self.current_value = value;
                                    self.current_key = key;
                                    self.state = GeneratorState::Suspended;
                                    self.ip = ip - 1;
                                    return Ok(true);
                                }
                                self.yield_from_source = None;
                                self.yield_from_pos = 0;
                                if failed(vm) {
                                    if let Some(t) = self.dispatch_exception() {
                                        ip = t;
                                        continue;
                                    }
                                    self.state = GeneratorState::Completed;
                                    self.current_value = Value::Null;
                                    self.ip = ip;
                                    return Err(VmError {
                                        message: "Uncaught exception in generator".to_string(),
                                        line: op.line,
                                    });
                                }
                                self.write_operand(&op.result, Value::Null);
                            }
                            _ => {
                                self.yield_from_source = None;
                                self.write_operand(&op.result, Value::Null);
                            }
                        }
                    } else {
                        // First time: set up the yield-from source. The
                        // operand is consumed (FREE_OP1).
                        let iterable =
                            self.read_operand_r(vm, &op.op1, &op_array.literals, op.line);
                        if let OperandType::Tmp(t) = op.op1 {
                            if let Some(slot) = self.tmps.get_mut(t as usize) {
                                *slot = Value::Undef;
                            }
                        }
                        let mut iterable = iterable;
                        // IteratorAggregate: delegate to what getIterator() returns.
                        if let Value::Object(obj) = &iterable {
                            let class_lower: Vec<u8> = obj
                                .borrow()
                                .class_name
                                .iter()
                                .map(|b| b.to_ascii_lowercase())
                                .collect();
                            if !vm.class_implements_interface(&class_lower, b"iterator")
                                && (vm
                                    .class_implements_interface(&class_lower, b"iteratoraggregate")
                                    || vm.class_implements_interface(&class_lower, b"traversable"))
                            {
                                let had = vm.current_exception.is_some();
                                let it = vm.aggregate_get_iterator_chain(&iterable);
                                if !had && vm.current_exception.is_some() {
                                    if let Some(t) = self.dispatch_exception() {
                                        ip = t;
                                        continue;
                                    }
                                    self.state = GeneratorState::Completed;
                                    self.current_value = Value::Null;
                                    self.ip = ip;
                                    return Err(VmError {
                                        message: "Uncaught exception in generator".to_string(),
                                        line: op.line,
                                    });
                                }
                                iterable = it.unwrap_or(Value::Null);
                            }
                        }
                        match &iterable {
                            Value::Array(_) => {
                                self.yield_from_source = Some(iterable);
                                self.yield_from_pos = 0;
                                // Re-execute to start yielding
                                ip -= 1; // Will re-execute this opcode
                            }
                            Value::Generator(inner_gen) => {
                                // zend_generator_yield_from(): this generator
                                // becomes a child of `inner_gen` and the
                                // resume loop continues with the tree's root.
                                let self_ptr = self as *const PhpGenerator;
                                let mut impossible = false;
                                let mut cur = Some(inner_gen.clone());
                                while let Some(g) = cur {
                                    if std::ptr::eq(
                                        RefCell::as_ptr(&g) as *const PhpGenerator,
                                        self_ptr,
                                    ) {
                                        impossible = true;
                                        break;
                                    }
                                    cur = match g.try_borrow() {
                                        Ok(b) if b.state != GeneratorState::Completed => {
                                            b.live_delegate()
                                        }
                                        Ok(_) => None,
                                        Err(_) => {
                                            impossible = true;
                                            None
                                        }
                                    };
                                }
                                let completed = inner_gen
                                    .try_borrow()
                                    .map(|b| b.state == GeneratorState::Completed)
                                    .unwrap_or(false);
                                if impossible {
                                    error = Some((
                                        b"Error",
                                        "Impossible to yield from the Generator being currently run"
                                            .into(),
                                    ));
                                } else if completed {
                                    error = Some((
                                        b"Error",
                                        "Generator passed to yield from was aborted without proper return and is unable to continue".into(),
                                    ));
                                } else {
                                    self.yield_from_source = Some(iterable.clone());
                                    self.yield_from_result = op.result;
                                    self.write_operand(&op.result, Value::Null);
                                    self.state = GeneratorState::Suspended;
                                    self.ip = ip;
                                    self.just_delegated = true;
                                    self.do_init = true;
                                    return Ok(true);
                                }
                            }
                            Value::Object(obj) => {
                                let class_lower: Vec<u8> = obj
                                    .borrow()
                                    .class_name
                                    .iter()
                                    .map(|b| b.to_ascii_lowercase())
                                    .collect();
                                if vm.class_implements_interface(&class_lower, b"iterator") {
                                    // Iterated lazily, one element per resume.
                                    let had = vm.current_exception.is_some();
                                    vm.current_line = op.line;
                                    vm.spl_call_method_at(&iterable, b"rewind", &[], false);
                                    if !had && vm.current_exception.is_some() {
                                        if let Some(t) = self.dispatch_exception() {
                                            ip = t;
                                            continue;
                                        }
                                        self.state = GeneratorState::Completed;
                                        self.current_value = Value::Null;
                                        self.ip = ip;
                                        return Err(VmError {
                                            message: "Uncaught exception in generator".to_string(),
                                            line: op.line,
                                        });
                                    }
                                    self.yield_from_source = Some(iterable.clone());
                                    self.yield_from_pos = 0;
                                    ip -= 1;
                                } else if vm
                                    .class_implements_interface(&class_lower, b"traversable")
                                {
                                    let arr = vm.iterator_to_array(&iterable);
                                    self.yield_from_source =
                                        Some(Value::Array(Rc::new(RefCell::new(arr))));
                                    self.yield_from_pos = 0;
                                    ip -= 1;
                                } else {
                                    error = Some((
                                        b"Error",
                                        "Can use \"yield from\" only with arrays and Traversables"
                                            .into(),
                                    ));
                                }
                            }
                            _ => {
                                error = Some((
                                    b"Error",
                                    "Can use \"yield from\" only with arrays and Traversables"
                                        .into(),
                                ));
                            }
                        }
                    }
                    if let Some((class, msg)) = error {
                        let exc = vm.create_exception(class, &msg, op.line);
                        vm.current_exception = Some(exc);
                        if let Some(t) = self.dispatch_exception() {
                            ip = t;
                            continue;
                        }
                        self.state = GeneratorState::Completed;
                        self.current_value = Value::Null;
                        self.ip = ip;
                        return Err(crate::vm::VmError {
                            message: format!("Uncaught Error: {}", msg),
                            line: op.line,
                        });
                    }
                }

                OpCode::GeneratorReturn => {
                    let had_exc = vm.current_exception.is_some();
                    let val = self.read_operand_r(vm, &op.op1, &op_array.literals, op.line);
                    if !had_exc && vm.current_exception.is_some() {
                        return Err(self.return_cv_escape(vm, ip, op.line));
                    }
                    self.return_value = val;
                    self.state = GeneratorState::Completed;
                    self.has_returned = true;
                    self.current_value = Value::Null;
                    self.current_key = Value::Null;
                    self.ip = ip;
                    return Ok(false);
                }

                OpCode::Return => {
                    // In a generator, return acts as GeneratorReturn.
                    // An explicit return inside a finally block discards any pending
                    // exception that triggered the finally (matches PHP semantics).
                    let had_exc = vm.current_exception.is_some();
                    let val = self.read_operand_r(vm, &op.op1, &op_array.literals, op.line);
                    if !had_exc && vm.current_exception.is_some() {
                        return Err(self.return_cv_escape(vm, ip, op.line));
                    }
                    self.return_value = val;
                    self.state = GeneratorState::Completed;
                    self.has_returned = true;
                    self.current_value = Value::Null;
                    self.current_key = Value::Null;
                    self.pending_exception = None;
                    vm.current_exception = None;
                    self.ip = ip;
                    return Ok(false);
                }

                OpCode::Nop => {}

                OpCode::FreeTmps => {
                    // (local_fast_path: only scalars to drop)
                    if let (OperandType::Tmp(a), OperandType::Tmp(b)) = (op.op1, op.op2) {
                        let end = (b as usize).min(self.tmps.len());
                        for slot in self.tmps[(a as usize).min(end)..end].iter_mut() {
                            *slot = Value::Undef;
                        }
                    }
                }

                OpCode::Assign => {
                    // (local_fast_path: a scalar into a plain CV holding one)
                    let val = self.read_operand(&op.op2, &op_array.literals);
                    if matches!(op.result, OperandType::Tmp(_)) {
                        self.write_operand(&op.result, val.clone());
                    }
                    self.write_operand(&op.op1, val);
                    // The source temporary is consumed by the assignment.
                    if let OperandType::Tmp(t) = op.op2 {
                        if let Some(slot) = self.tmps.get_mut(t as usize) {
                            *slot = Value::Undef;
                        }
                    }
                }

                OpCode::LoadConst => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    self.write_operand(&op.result, val);
                }

                OpCode::QmAssign => {
                    let v = self.read_operand(&op.op1, &op_array.literals);
                    // the container of `f()[k] = v` (see the VM's handler)
                    if let OperandType::Const(k) = op.op2 {
                        if matches!(op_array.literals.get(k as usize), Some(Value::Long(2))) {
                            let v = match v {
                                Value::Array(a) if !vm.last_call_by_ref => {
                                    let copy = a.borrow().clone();
                                    Value::Array(std::rc::Rc::new(std::cell::RefCell::new(copy)))
                                }
                                other => other,
                            };
                            self.write_operand(&op.result, v);
                            continue;
                        }
                        // `$a[k] = $a`: the value is copied before the write
                        // fetches the container (see the VM's handler).
                        if matches!(op_array.literals.get(k as usize), Some(Value::Long(3))) {
                            let v = vm.dup_array_value(v.deref());
                            self.write_operand(&op.result, v);
                            continue;
                        }
                    }
                    let v = if matches!(v, Value::Undef) {
                        Value::Null
                    } else {
                        v.deref()
                    };
                    self.write_operand(&op.result, v);
                }

                OpCode::Add => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.result, a.add(&b));
                }

                OpCode::Sub => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.result, a.sub(&b));
                }

                OpCode::Mul => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.result, a.mul(&b));
                }

                OpCode::Concat => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    let a_str = vm.value_to_string(&a);
                    let b_str = vm.value_to_string(&b);
                    let mut result = a_str.as_bytes().to_vec();
                    result.extend_from_slice(b_str.as_bytes());
                    self.write_operand(&op.result, Value::String(PhpString::from_vec(result)));
                }

                OpCode::Equal => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.equals(&b) {
                            Value::True
                        } else {
                            Value::False
                        },
                    );
                }

                OpCode::NotEqual => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.equals(&b) {
                            Value::False
                        } else {
                            Value::True
                        },
                    );
                }

                OpCode::Identical => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.identical(&b) {
                            Value::True
                        } else {
                            Value::False
                        },
                    );
                }

                OpCode::NotIdentical => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.identical(&b) {
                            Value::False
                        } else {
                            Value::True
                        },
                    );
                }

                OpCode::Less => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.compare(&b) < 0 {
                            Value::True
                        } else {
                            Value::False
                        },
                    );
                }

                OpCode::LessEqual => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.compare(&b) <= 0 {
                            Value::True
                        } else {
                            Value::False
                        },
                    );
                }

                OpCode::Greater => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.compare(&b) > 0 {
                            Value::True
                        } else {
                            Value::False
                        },
                    );
                }

                OpCode::GreaterEqual => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.compare(&b) >= 0 {
                            Value::True
                        } else {
                            Value::False
                        },
                    );
                }

                OpCode::PreIncrement => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let new_val = val.add(&Value::Long(1));
                    self.write_operand(&op.op1, new_val.clone());
                    self.write_operand(&op.result, new_val);
                }

                OpCode::PreDecrement => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let new_val = val.sub(&Value::Long(1));
                    self.write_operand(&op.op1, new_val.clone());
                    self.write_operand(&op.result, new_val);
                }

                OpCode::PostIncrement => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let new_val = val.add(&Value::Long(1));
                    self.write_operand(&op.result, val);
                    self.write_operand(&op.op1, new_val);
                }

                OpCode::PostDecrement => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let new_val = val.sub(&Value::Long(1));
                    self.write_operand(&op.result, val);
                    self.write_operand(&op.op1, new_val);
                }

                OpCode::Jmp => {
                    if let OperandType::JmpTarget(target) = op.op1 {
                        ip = target as usize;
                    }
                }

                OpCode::JmpZ => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    if !val.is_truthy() {
                        if let OperandType::JmpTarget(target) = op.op2 {
                            ip = target as usize;
                        }
                    }
                }

                OpCode::JmpNz => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    if val.is_truthy() {
                        if let OperandType::JmpTarget(target) = op.op2 {
                            ip = target as usize;
                        }
                    }
                }

                OpCode::ArrayNew => {
                    let arr = Rc::new(RefCell::new(crate::array::PhpArray::new()));
                    self.write_operand(&op.result, Value::Array(arr));
                }

                OpCode::Throw => {
                    let exc_val = self.read_operand_r(vm, &op.op1, &op_array.literals, op.line);
                    // Leave the `@` levels entered since the handler's try.
                    match self.exception_handlers.last() {
                        Some(&(_, _, depth)) => vm.restore_error_reporting_to(depth as usize),
                        None => vm.restore_error_reporting_to(
                            vm.error_reporting_stack.len().saturating_sub(1),
                        ),
                    }
                    let exc = vm.thrown_exception(exc_val, op.line);
                    vm.current_exception = Some(exc);
                    if let Some((catch_target, finally_target, _)) = self.exception_handlers.pop() {
                        // Prefer catch handler if present; otherwise jump to finally
                        // so the finally block runs and re-raises the pending exception
                        // after it completes.
                        if catch_target > 0 {
                            ip = catch_target as usize;
                        } else if finally_target > 0 {
                            ip = finally_target as usize;
                        } else {
                            self.state = GeneratorState::Completed;
                            self.current_value = Value::Null;
                            self.ip = ip;
                            return Err(VmError {
                                message: "Uncaught exception in generator".to_string(),
                                line: op.line,
                            });
                        }
                    } else {
                        self.state = GeneratorState::Completed;
                        self.current_value = Value::Null;
                        self.ip = ip;
                        return Err(VmError {
                            message: "Uncaught exception in generator".to_string(),
                            line: op.line,
                        });
                    }
                }

                OpCode::TryBegin => {
                    if let (OperandType::JmpTarget(catch), OperandType::JmpTarget(finally_target)) =
                        (op.op1, op.op2)
                    {
                        self.exception_handlers.push((
                            catch,
                            finally_target,
                            vm.error_reporting_stack.len() as u32,
                        ));
                    }
                }

                OpCode::TryEnd => {
                    self.exception_handlers.pop();
                }

                OpCode::FinallyEnter => {
                    // Move what led here (pending exception, deferred return
                    // or jump) into the finally's slot, kept in the frame
                    // across yields (as Vm::execute_frame does).
                    let depth = self.exception_handlers.len();
                    Vm::finally_escape(&mut self.fin, depth, &vm.current_exception);
                    let mut pending_exc = None;
                    let slot = if let Some(exc) = vm.current_exception.take() {
                        pending_exc = Some(exc.clone());
                        Vm::finally_slot(1, exc)
                    } else if let Some(v) = self.pending_return.take() {
                        Vm::finally_slot(2, v)
                    } else if let Some(t) = self.pending_finally_jump.take() {
                        let levels = std::mem::replace(&mut self.pending_finally_levels, 1);
                        Vm::finally_slot(3, Value::Long(t as i64 | ((levels as i64) << 32)))
                    } else {
                        Value::Null
                    };
                    self.fin.push((depth, pending_exc));
                    self.write_operand(&op.op1, slot);
                }

                OpCode::CatchException => {
                    // Graceful-exit unwinding (fiber destruction) skips catch clauses.
                    if vm
                        .current_exception
                        .as_ref()
                        .is_some_and(crate::vm::Vm::is_graceful_exit)
                    {
                        if let Some((catch_target, finally_target, _)) =
                            self.exception_handlers.pop()
                        {
                            ip = if catch_target > 0 {
                                catch_target
                            } else {
                                finally_target
                            } as usize;
                            continue;
                        }
                        self.state = GeneratorState::Completed;
                        self.current_value = Value::Null;
                        self.ip = ip;
                        return Err(VmError {
                            message: "Graceful exit".into(),
                            line: op.line,
                        });
                    }
                    // The catch of a try with a finally starts with the finally
                    // guard's TryBegin, already pushed above the caught level.
                    let guarded = ip >= 2
                        && matches!(op_array.ops.get(ip - 2), Some(g) if g.opcode == OpCode::TryBegin && g.op1 == g.op2);
                    let caught_at = self
                        .exception_handlers
                        .len()
                        .saturating_sub(usize::from(guarded));
                    Vm::finally_escape(&mut self.fin, caught_at, &vm.current_exception);
                    // (the frames of the calls the exception left)
                    vm.call_stack
                        .truncate(self.frame_depth + vm.fiber_call_base() + 1);
                    self.pending_return = None;
                    self.pending_finally_jump = None;
                    if let Some(exc) = vm.current_exception.take() {
                        self.write_operand(&op.op1, exc);
                    }
                }

                OpCode::SaveReturn => {
                    // Save return value for deferred return (finally blocks)
                    let val = if vm.current_exception.is_none() {
                        // (copied out first: an undefined variable warns, and
                        // an exception the error handler throws is caught here)
                        let v = self.read_operand_r(vm, &op.op1, &op_array.literals, op.line);
                        if vm.current_exception.is_some() {
                            vm.error_handler_threw = false;
                            if let Some(t) = self.dispatch_exception() {
                                ip = t;
                                continue;
                            }
                            self.state = GeneratorState::Completed;
                            self.current_value = Value::Null;
                            self.ip = ip;
                            return Err(VmError {
                                message: "Uncaught exception in generator".to_string(),
                                line: op.line,
                            });
                        }
                        v
                    } else {
                        self.read_operand(&op.op1, &op_array.literals)
                    };
                    self.pending_return = Some(val);
                    self.pending_finally_jump = None;
                    vm.current_exception = None;
                    self.pop_handlers_for_finally_jump(op_array.ops.get(ip));
                    // A return out of a running finally block discards what it
                    // had pending (ZEND_DISCARD_EXCEPTION).
                    while self
                        .fin
                        .last()
                        .is_some_and(|f| f.0 > self.exception_handlers.len())
                    {
                        self.fin.pop();
                    }
                }

                OpCode::SaveJump => {
                    // Save jump target for deferred break/continue (finally blocks)
                    if let OperandType::JmpTarget(target) = op.op1 {
                        self.pending_finally_jump = Some(target);
                    }
                    self.pending_finally_levels = match op.op2 {
                        OperandType::Const(_) => {
                            self.read_operand(&op.op2, &op_array.literals).to_long() as u32
                        }
                        _ => 1,
                    };
                    vm.current_exception = None;
                    self.pop_handlers_for_finally_jump(op_array.ops.get(ip));
                    while self
                        .fin
                        .last()
                        .is_some_and(|f| f.0 > self.exception_handlers.len())
                    {
                        self.fin.pop();
                    }
                }

                OpCode::ReturnDeferred => {
                    let this_finally_start = if let OperandType::JmpTarget(fs) = op.op1 {
                        fs
                    } else {
                        0
                    };
                    // ZEND_FAST_RET: resume what led into the finally block.
                    if let OperandType::Tmp(t) = op.op2 {
                        self.fin.pop();
                        let slot = self
                            .tmps
                            .get_mut(t as usize)
                            .map(|s| std::mem::replace(s, Value::Undef))
                            .unwrap_or(Value::Undef);
                        if let Value::Array(a) = slot {
                            let (kind, payload) = {
                                let a = a.borrow();
                                (
                                    a.get_int(0).map(|v| v.to_long()).unwrap_or(0),
                                    a.get_int(1).cloned().unwrap_or(Value::Null),
                                )
                            };
                            match kind {
                                1 => vm.current_exception = Some(payload),
                                2 => self.pending_return = Some(payload),
                                3 => {
                                    let v = payload.to_long();
                                    self.pending_finally_jump = Some(v as u32);
                                    self.pending_finally_levels = (v >> 32) as u32;
                                }
                                _ => {}
                            }
                        }
                    }
                    if let Some(val) = self.pending_return.take() {
                        // Pop exception handlers for this finally block
                        if this_finally_start > 0
                            && self
                                .exception_handlers
                                .iter()
                                .any(|h| h.1 == this_finally_start)
                        {
                            while let Some(&(_catch_target, finally_target, _)) =
                                self.exception_handlers.last()
                            {
                                self.exception_handlers.pop();
                                if finally_target == this_finally_start {
                                    break;
                                }
                            }
                        }
                        // Look for an outer finally block to chain through
                        let mut outer_finally: Option<u32> = None;
                        while let Some(&(_catch_target, finally_target, _)) =
                            self.exception_handlers.last()
                        {
                            self.exception_handlers.pop();
                            if finally_target > 0 {
                                outer_finally = Some(finally_target);
                                break;
                            }
                        }
                        if let Some(finally_ip) = outer_finally {
                            self.pending_return = Some(val);
                            ip = finally_ip as usize;
                            continue;
                        }
                        // No outer finally - complete the generator with return value
                        self.return_value = val;
                        self.state = GeneratorState::Completed;
                        self.has_returned = true;
                        self.current_value = Value::Null;
                        self.ip = ip;
                        return Ok(false);
                    }
                    // If there's a pending jump (deferred break/continue)
                    if let Some(target) = self.pending_finally_jump.take() {
                        let levels = std::mem::replace(&mut self.pending_finally_levels, 1);
                        if levels > 1 {
                            // The jump also leaves enclosing try/finally blocks:
                            // run the next one first.
                            if this_finally_start > 0
                                && self
                                    .exception_handlers
                                    .iter()
                                    .any(|h| h.1 == this_finally_start)
                            {
                                while let Some(&(_, finally_target, _)) =
                                    self.exception_handlers.last()
                                {
                                    self.exception_handlers.pop();
                                    if finally_target == this_finally_start {
                                        break;
                                    }
                                }
                            }
                            let mut outer_finally = None;
                            while let Some(&(_, finally_target, _)) = self.exception_handlers.last()
                            {
                                self.exception_handlers.pop();
                                if finally_target > 0 {
                                    outer_finally = Some(finally_target);
                                    break;
                                }
                            }
                            if let Some(finally_ip) = outer_finally {
                                self.pending_finally_jump = Some(target);
                                self.pending_finally_levels = levels - 1;
                                ip = finally_ip as usize;
                                continue;
                            }
                        }
                        ip = target as usize;
                        continue;
                    }
                    // If there's a pending exception, re-throw it through outer handlers
                    if vm.current_exception.is_some() {
                        if let Some(t) = self.dispatch_exception() {
                            ip = t;
                            continue;
                        }
                        // No outer handler - uncaught exception
                        self.state = GeneratorState::Completed;
                        self.current_value = Value::Null;
                        self.ip = ip;
                        return Err(VmError {
                            message: "Uncaught exception in generator".to_string(),
                            line: op.line,
                        });
                    }
                    // Otherwise continue (no pending return, jump, or exception)
                }

                // Every other opcode runs through the VM's implementation on
                // this frame's variables.
                _ => match self.run_on_vm(vm, &op_array, ip) {
                    Ok(next) => ip = next,
                    Err(done) => return done,
                },
            }
        }
    }

    /// Run the opcode before `ip` with the VM's implementation on this
    /// frame's variables. Ok: where execution continues (an exception it
    /// raised is dispatched to the frame's handlers); Err: what the resumption
    /// returns (exit, or an exception leaving the generator).
    fn run_on_vm(
        &mut self,
        vm: &mut Vm,
        op_array: &OpArray,
        ip: usize,
    ) -> Result<usize, Result<bool, VmError>> {
        match vm.execute_single_op(
            op_array,
            ip - 1,
            &mut self.cvs,
            &mut self.tmps,
            &mut self.symtab,
            &mut self.fe,
        ) {
            Ok(next) => Ok(next),
            Err(e) => {
                if vm.exit_requested {
                    self.state = GeneratorState::Completed;
                    self.ip = ip;
                    return Err(Ok(false));
                }
                if vm.current_exception.is_some() {
                    if let Some(target) = self.dispatch_exception() {
                        return Ok(target);
                    }
                }
                self.state = GeneratorState::Completed;
                self.current_value = Value::Null;
                self.ip = ip;
                Err(Err(e))
            }
        }
    }

    /// Write the send_value to the result operand of the last Yield instruction
    /// or to the fiber_suspend_result operand if resuming from a fiber suspension
    pub fn write_send_value(&mut self) {
        // Check if we're resuming from a fiber suspension (Fiber::suspend() in nested call)
        if let Some(result_op) = self.fiber_suspend_result.take() {
            self.write_operand(&result_op, self.send_value.clone());
            return;
        }
        // After resuming, if the previous Yield had a result operand, write the send value there
        if self.ip > 0 {
            let prev_ip = self.ip - 1;
            if prev_ip < self.op_array.ops.len() {
                let prev_op = self.op_array.ops[prev_ip].clone();
                if prev_op.opcode == OpCode::Yield && !matches!(prev_op.result, OperandType::Unused)
                {
                    self.write_operand(&prev_op.result, self.send_value.clone());
                }
            }
        }
    }

    /// Resume the generator with an exception (for Fiber::throw)
    /// The exception should already be set in vm.current_exception
    fn resume_single_throw(&mut self, vm: &mut Vm) -> Result<bool, VmError> {
        if self.state == GeneratorState::Completed {
            return Ok(false);
        }

        vm.enter_generator_resume(0)?;

        // If we have a fiber_suspend_result, we're resuming from a nested call
        // Clear it since we're throwing instead
        self.fiber_suspend_result = None;

        // Push class context for get_class() / get_called_class() inside generators
        let pushed_scope = if let Some(ref scope) = self.class_scope {
            vm.push_class_scope(scope.clone());
            true
        } else {
            // The function's own scope (none for a function outside any
            // class: the resumer's does not carry over).
            vm.push_class_scope(self.op_array.scope_class.clone().unwrap_or_default());
            true
        };
        let pushed_called = if let Some(ref called) = self.called_class {
            vm.called_class_stack.push(called.clone());
            true
        } else {
            false
        };

        // Push a call_stack entry for this generator so any exception
        // constructed/thrown inside it gets a proper trace frame.
        let (func_name, is_instance) = self.frame_name(vm);
        // Without a caller context (Generator->throw()), the frame shows
        // [internal function]; inside a `yield from` tree, the delegating
        // generator's `yield from` site.
        let (file, line) = vm.generator_caller.clone().unwrap_or_default();
        self.frame_depth = vm.call_stack.len().saturating_sub(vm.fiber_call_base());
        vm.call_stack
            .push((func_name, file, line, self.args.clone(), is_instance));

        // An array / iterator `yield from` is abandoned: the exception reaches
        // the generator now, not after the remaining values
        // (zend_generator_throw_exception()).
        if matches!(
            self.yield_from_source,
            Some(Value::Array(_)) | Some(Value::Object(_))
        ) {
            let src = self.yield_from_source.take();
            self.yield_from_pos = 0;
            drop(src);
            vm.run_pending_destructors_pub();
        }

        // Try to find an exception handler in the generator
        let had_handler = if let Some((catch_target, finally_target, _)) =
            self.exception_handlers.last().copied()
        {
            self.exception_handlers.pop();
            if catch_target > 0 {
                self.ip = catch_target as usize;
                true
            } else if finally_target > 0 {
                self.ip = finally_target as usize;
                true
            } else {
                false
            }
        } else {
            false
        };

        if !had_handler {
            // No valid handler - propagate exception out
            self.state = GeneratorState::Completed;
            self.current_value = Value::Null;
            vm.call_stack.pop();
            if pushed_called {
                vm.called_class_stack.pop();
            }
            if pushed_scope {
                vm.pop_class_scope();
            }
            vm.leave_generator_resume();
            return Err(VmError {
                message: "Uncaught exception in generator".to_string(),
                line: 0,
            });
        }

        self.is_running = true;
        // The running frame's values (a fiber suspended inside it owns them).
        vm.gc_frames.push(crate::gc::GcFrame::for_generator(self));
        let fname = self.running_function_name(vm);
        vm.running_generators
            .push((self as *const PhpGenerator as usize, fname, self.object_id));
        self.push_running_values(vm);
        let cv_frame = vm.register_cv_frame(&self.op_array);
        if cv_frame {
            vm.set_cv_frame_vars(&self.cvs);
        }
        let saved_file = vm.enter_op_array_file(&self.op_array);
        let outer_exec = std::mem::replace(&mut vm.exec_frame_len, vm.call_stack.len());
        let result = self.resume_inner_observed(vm);
        vm.exec_frame_len = outer_exec;
        vm.leave_op_array_file(saved_file);
        if let Ok(true) = result {
            self.last_value = self.current_value.clone();
            self.last_key = self.current_key.clone();
        }
        if cv_frame {
            vm.cv_frames.pop();
        }
        self.is_running = false;
        vm.gc_frames.pop();
        vm.running_generators.pop();
        self.pop_running_values(vm);

        vm.call_stack
            .truncate(self.frame_depth + vm.fiber_call_base());
        if pushed_called {
            vm.called_class_stack.pop();
        }
        if pushed_scope {
            vm.pop_class_scope();
        }

        vm.leave_generator_resume();
        // The finished frame is freed once the generator's frame is left.
        self.release_finished_frame(vm);

        result
    }

    /// A read (BP_VAR_R) of an operand: an undefined CV warns and reads null.
    /// ZEND_GENERATOR_RETURN of an undefined CV: an exception the error
    /// handler throws on its warning leaves the generator (finished) past
    /// its catch blocks.
    #[inline(never)]
    fn return_cv_escape(&mut self, vm: &mut Vm, ip: usize, line: u32) -> VmError {
        vm.error_handler_threw = false;
        self.state = GeneratorState::Completed;
        self.has_returned = true;
        self.return_value = Value::Null;
        self.current_value = Value::Null;
        self.current_key = Value::Null;
        self.ip = ip;
        VmError {
            message: "Uncaught exception in generator".to_string(),
            line,
        }
    }

    fn read_operand_r(
        &self,
        vm: &mut Vm,
        operand: &OperandType,
        literals: &[Value],
        line: u32,
    ) -> Value {
        if let OperandType::Cv(idx) = operand {
            if matches!(self.cvs.get(*idx as usize), Some(Value::Undef) | None) {
                let name = self
                    .op_array
                    .cv_names
                    .get(*idx as usize)
                    .map(|n| String::from_utf8_lossy(n).to_string())
                    .unwrap_or_default();
                vm.emit_warning_at(&format!("Undefined variable ${}", name), line);
                return Value::Null;
            }
        }
        self.read_operand(operand, literals)
    }

    fn read_operand(&self, operand: &OperandType, literals: &[Value]) -> Value {
        match operand {
            OperandType::Cv(idx) => {
                let val = self.cvs.get(*idx as usize).cloned().unwrap_or(Value::Null);
                val.deref()
            }
            OperandType::Const(idx) => literals.get(*idx as usize).cloned().unwrap_or(Value::Null),
            OperandType::Tmp(idx) => self.tmps.get(*idx as usize).cloned().unwrap_or(Value::Null),
            OperandType::Unused => Value::Null,
            OperandType::JmpTarget(_) => Value::Null,
        }
    }

    fn write_operand(&mut self, operand: &OperandType, value: Value) {
        match operand {
            OperandType::Cv(idx) => {
                if let Some(slot) = self.cvs.get_mut(*idx as usize) {
                    if let Value::Reference(r) = slot {
                        *r.borrow_mut() = value.clone();
                    } else {
                        *slot = value.clone();
                    }
                }
                // Persist static variables
                // Note: we can't access vm.static_vars here directly, but
                // the static_cv_keys are tracked for when we have VM access
            }
            OperandType::Tmp(idx) => {
                if let Some(slot) = self.tmps.get_mut(*idx as usize) {
                    *slot = value;
                }
            }
            _ => {}
        }
    }
}

/// Generator methods (zend_generators.c), shared by every call path.
impl Vm {
    /// The "function" shown by var_dump() of a Generator.
    pub fn generator_function_name(&self, g: &GenRc) -> String {
        match g.try_borrow() {
            Ok(b) => self.generator_function_name_of(&b),
            Err(_) => {
                let addr = RefCell::as_ptr(g) as usize;
                self.running_generators
                    .iter()
                    .rev()
                    .find(|(a, _, _)| *a == addr)
                    .map(|(_, n, _)| n.to_string())
                    .unwrap_or_default()
            }
        }
    }

    /// Generator `g` delegates with `yield from` to the one that runs: it
    /// is on the running stack too.
    #[inline(never)]
    fn push_delegating_generator(&mut self, g: &PhpGenerator) {
        let fname: Rc<str> = match &g.running_name {
            Some(n) => n.clone(),
            None => Rc::from(self.generator_function_name_of(g)),
        };
        self.running_generators
            .push((g as *const PhpGenerator as usize, fname, g.object_id));
    }

    /// The object handle of a generator (also while it runs).
    pub fn generator_handle(&self, g: &GenRc) -> u64 {
        match g.try_borrow() {
            Ok(b) => b.object_id,
            Err(_) => {
                let addr = RefCell::as_ptr(g) as usize;
                self.running_generators
                    .iter()
                    .rev()
                    .find(|(a, _, _)| *a == addr)
                    .map_or(0, |(_, _, id)| *id)
            }
        }
    }

    fn generator_function_name_of(&self, gen_ref: &PhpGenerator) -> String {
        let raw_name = String::from_utf8_lossy(&gen_ref.op_array.name).to_string();
        // - Closures: __closure_N -> {closure:file:line}
        // - Class methods: prepend declaring class (with anonymous classes
        //   rendered as 'class@anonymous...')
        // - Plain functions: keep as-is
        if !gen_ref.op_array.display_name.is_empty() {
            String::from_utf8_lossy(&gen_ref.op_array.display_name).to_string()
        } else if (raw_name.starts_with("__closure_") || raw_name.starts_with("__arrow_"))
            && !raw_name.starts_with("__closure_fcc_")
        {
            let file = String::from_utf8_lossy(&gen_ref.op_array.filename).to_string();
            format!("{{closure:{}:{}}}", file, gen_ref.op_array.decl_line)
        } else if let Some(ref scope) = gen_ref.class_scope {
            // The scope may be stored lowercased: show the declared name
            // (anonymous classes keep their full internal name).
            let class_display = match self.classes.get(&scope.to_ascii_lowercase()) {
                Some(ce) => crate::string::msg_str(&ce.name).to_string(),
                None => crate::string::msg_str(scope).to_string(),
            };
            format!("{}::{}", class_display, raw_name)
        } else {
            raw_name
        }
    }

    /// zend_generator_ensure_initialized(): run a fresh generator to its
    /// first yield.
    fn generator_ensure_initialized(&mut self, g: &GenRc) -> Result<(), VmError> {
        let fresh = g
            .try_borrow()
            .map(|b| b.state == GeneratorState::Created && b.live_delegate().is_none())
            .unwrap_or(false);
        if fresh {
            let mut b = g.borrow_mut();
            let r = b.resume(self);
            b.advanced_past_first = false;
            r?;
        }
        Ok(())
    }

    /// zend_generator_get_current(): the root of the `yield from` tree `g`
    /// is a leaf of. A root that finished meanwhile is replaced by its
    /// child: with its return value as the result of the child's `yield
    /// from`, or by throwing a ClosedGeneratorException into the child
    /// (which then runs) if it was aborted.
    pub(crate) fn generator_get_current(&mut self, g: &GenRc) -> GenRc {
        loop {
            let mut path: Vec<GenRc> = vec![g.clone()];
            loop {
                let next = match path.last().unwrap().try_borrow() {
                    Ok(b) if b.state != GeneratorState::Completed => b.live_delegate(),
                    _ => None,
                };
                match next {
                    Some(n) if !path.iter().any(|p| Rc::ptr_eq(p, &n)) => path.push(n),
                    _ => break,
                }
            }
            let root = path.last().unwrap().clone();
            if path.len() == 1 {
                return root;
            }
            let info = root.try_borrow().ok().map(|b| {
                (
                    b.state == GeneratorState::Completed,
                    b.has_returned,
                    b.return_value.clone(),
                    (b.last_value.clone(), b.last_key.clone()),
                )
            });
            let Some((completed, returned, retval, last)) = info else {
                return root;
            };
            if !completed {
                return root;
            }
            if !returned {
                // update_current() throws into the child and resumes `g`.
                if let Ok(mut gb) = g.try_borrow_mut() {
                    let _ = gb.resume(self);
                    continue;
                }
                return root;
            }
            let child = path[path.len() - 2].clone();
            match child.try_borrow_mut() {
                Ok(mut c) => {
                    c.yield_from_source = None;
                    let res = c.yield_from_result;
                    c.write_operand(&res, retval);
                    c.current_value = last.0;
                    c.current_key = last.1;
                }
                Err(_) => return root,
            }
        }
    }

    /// Generator::current() / key() / next() / send() / throw() / valid() /
    /// rewind() / getReturn() / __wakeup(). An exception is left in
    /// current_exception (with Err).
    pub(crate) fn generator_method(
        &mut self,
        g: &GenRc,
        this: &Value,
        method: &[u8],
        args: &[Value],
        line: u32,
    ) -> Result<Value, VmError> {
        let display = match method {
            b"current" => "current",
            b"key" => "key",
            b"next" => "next",
            b"valid" => "valid",
            b"rewind" => "rewind",
            b"send" => "send",
            b"throw" => "throw",
            b"getreturn" => "getReturn",
            b"__wakeup" => "__wakeup",
            b"__debuginfo" => "__debugInfo",
            _ => {
                let msg = format!(
                    "Call to undefined method Generator::{}()",
                    crate::string::msg_str(method)
                );
                let exc = self.create_exception(b"Error", &msg, line);
                self.current_exception = Some(exc);
                return Err(VmError {
                    message: format!("Uncaught Error: {}", msg),
                    line,
                });
            }
        };
        let arity = if matches!(method, b"send" | b"throw") {
            1
        } else {
            0
        };
        if args.len() != arity {
            let msg = format!(
                "Generator::{}() expects exactly {} argument{}, {} given",
                display,
                arity,
                if arity == 1 { "" } else { "s" },
                args.len()
            );
            let exc = self.create_exception(b"ArgumentCountError", &msg, line);
            self.current_exception = Some(exc);
            return Err(VmError {
                message: format!("Uncaught ArgumentCountError: {}", msg),
                line,
            });
        }
        if method == b"__debuginfo" {
            let mut arr = crate::array::PhpArray::new();
            arr.set(
                crate::array::ArrayKey::String(PhpString::from_bytes(b"function")),
                Value::String(PhpString::from_string(self.generator_function_name(g))),
            );
            return Ok(Value::Array(Rc::new(RefCell::new(arr))));
        }
        if method == b"throw" {
            let throwable = match &args[0] {
                Value::Object(obj) => {
                    let class_lower: Vec<u8> = obj
                        .borrow()
                        .class_name
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect();
                    class_lower == b"exception"
                        || class_lower == b"error"
                        || crate::vm::is_builtin_subclass(&class_lower, b"exception")
                        || crate::vm::is_builtin_subclass(&class_lower, b"error")
                        || self.class_extends(&class_lower, b"exception")
                        || self.class_extends(&class_lower, b"error")
                }
                _ => false,
            };
            if !throwable {
                let type_name = match &args[0] {
                    Value::Object(obj) => {
                        crate::value::display_class_name(&obj.borrow().class_name)
                    }
                    v => Vm::value_type_name(v).to_string(),
                };
                let msg = format!(
                    "Generator::throw(): Argument #1 ($exception) must be of type Throwable, {} given",
                    type_name
                );
                self.call_stack.push((
                    "Generator->throw".to_string(),
                    self.current_file.clone(),
                    line,
                    args.to_vec(),
                    true,
                ));
                let exc = self.create_exception(b"TypeError", &msg, line);
                self.call_stack.pop();
                self.current_exception = Some(exc);
                return Err(VmError {
                    message: format!("Uncaught TypeError: {}", msg),
                    line,
                });
            }
        }
        let depth = self.call_stack.len();
        self.call_stack.push((
            format!("Generator->{}", display),
            self.current_file.clone(),
            line,
            args.to_vec(),
            true,
        ));
        self.note_frame_this(this);
        let prev_caller = self.generator_caller.take();
        let observed = self.observer.map(|_| format!("Generator::{}", display));
        if let Some(n) = &observed {
            self.observe_internal_begin(n);
        }
        let r = self.generator_method_inner(g, method, args, line);
        if let Some(n) = &observed {
            self.observe_internal_end(n, r.as_ref().ok());
        }
        self.generator_caller = prev_caller;
        self.call_stack.truncate(depth);
        if r.is_ok() && self.current_exception.is_some() && !self.exit_requested {
            return Err(VmError {
                message: "Uncaught exception in generator".into(),
                line,
            });
        }
        r
    }

    /// A method of a generator called while it runs: the value and key it
    /// yielded last are still there; anything that would run it (also
    /// running it to its first yield) is "already running".
    #[inline(never)]
    fn running_generator_method(
        &mut self,
        g: &GenRc,
        method: &[u8],
        line: u32,
    ) -> Result<Value, VmError> {
        let addr = RefCell::as_ptr(g) as usize;
        let entry = self
            .running_gen_values
            .iter()
            .rev()
            .find(|(a, _)| *a == addr)
            .map(|(_, v)| v.clone());
        let (value, key) = match entry {
            Some(Some(vals)) => vals,
            // Not run to its first yield yet: that would run it.
            Some(None) => return Err(PhpGenerator::running_error(self)),
            // Delegating with `yield from` to the generator that runs.
            None => match method {
                b"next" | b"send" | b"throw" | b"rewind" => {
                    return Err(PhpGenerator::running_error(self));
                }
                _ => return Ok(Value::Null),
            },
        };
        let msg = match method {
            b"next" | b"send" | b"throw" => return Err(PhpGenerator::running_error(self)),
            b"valid" => return Ok(Value::True),
            b"current" => return Ok(value),
            b"key" => return Ok(key),
            b"rewind" => "Cannot rewind a generator that was already run",
            _ => "Cannot get return value of a generator that hasn't returned",
        };
        let exc = self.create_exception(b"Exception", msg, line);
        self.current_exception = Some(exc);
        Err(VmError {
            message: format!("Uncaught Exception: {}", msg),
            line,
        })
    }

    fn generator_method_inner(
        &mut self,
        g: &GenRc,
        method: &[u8],
        args: &[Value],
        line: u32,
    ) -> Result<Value, VmError> {
        let running = g.try_borrow().map(|b| b.is_running).unwrap_or(true);
        if method == b"__debuginfo" {
            // ["function" => the generator function's name]
            let name = self.generator_function_name(g);
            let mut arr = crate::array::PhpArray::new();
            arr.set(
                crate::array::ArrayKey::String(PhpString::from_bytes(b"function")),
                Value::String(PhpString::from_string(name)),
            );
            return Ok(Value::Array(Rc::new(RefCell::new(arr))));
        }
        if method == b"__wakeup" {
            let msg = "Unserialization of 'Generator' is not allowed";
            let exc = self.create_exception(b"Exception", msg, line);
            self.current_exception = Some(exc);
            return Err(VmError {
                message: format!("Uncaught Exception: {}", msg),
                line,
            });
        }
        if running {
            return self.running_generator_method(g, method, line);
        }
        self.generator_ensure_initialized(g)?;
        if self.current_exception.is_some() {
            return Err(VmError {
                message: "Uncaught exception in generator".into(),
                line,
            });
        }
        let completed = |g: &GenRc| {
            g.try_borrow()
                .map(|b| b.state == GeneratorState::Completed)
                .unwrap_or(false)
        };
        match method {
            b"current" | b"key" => {
                let root = self.generator_get_current(g);
                if completed(g) {
                    return Ok(Value::Null);
                }
                let b = root.try_borrow();
                Ok(match b {
                    Ok(b) if method == b"current" => b.current_value.clone(),
                    Ok(b) => b.current_key.clone(),
                    Err(_) => Value::Null,
                })
            }
            b"valid" => {
                self.generator_get_current(g);
                Ok(if completed(g) {
                    Value::False
                } else {
                    Value::True
                })
            }
            b"next" | b"send" => {
                if completed(g) {
                    return Ok(Value::Null);
                }
                {
                    // (not held while it runs)
                    let root = self.generator_get_current(g);
                    if let Ok(mut rb) = root.try_borrow_mut() {
                        rb.send_value = args.first().cloned().unwrap_or(Value::Null);
                        rb.write_send_value();
                    }
                }
                {
                    let mut gb = match g.try_borrow_mut() {
                        Ok(gb) => gb,
                        Err(_) => return Err(PhpGenerator::running_error(self)),
                    };
                    gb.advanced_past_first = true;
                    gb.resume(self)?;
                }
                if method == b"next" || completed(g) {
                    return Ok(Value::Null);
                }
                let root = self.generator_get_current(g);
                let v = root
                    .try_borrow()
                    .map(|b| b.current_value.clone())
                    .unwrap_or(Value::Null);
                Ok(v)
            }
            b"throw" => {
                let exc = args[0].clone();
                if completed(g) {
                    // Thrown in the caller's context.
                    self.current_exception = Some(exc);
                    return Err(VmError {
                        message: "Uncaught exception".into(),
                        line,
                    });
                }
                self.generator_get_current(g);
                self.current_exception = Some(exc);
                {
                    let mut gb = match g.try_borrow_mut() {
                        Ok(gb) => gb,
                        Err(_) => return Err(PhpGenerator::running_error(self)),
                    };
                    gb.advanced_past_first = true;
                    gb.resume_with_exception(self)?;
                }
                if completed(g) {
                    return Ok(Value::Null);
                }
                let root = self.generator_get_current(g);
                let v = root
                    .try_borrow()
                    .map(|b| b.current_value.clone())
                    .unwrap_or(Value::Null);
                Ok(v)
            }
            b"rewind" => {
                let past = g
                    .try_borrow()
                    .map(|b| b.advanced_past_first)
                    .unwrap_or(false);
                if past {
                    let msg = "Cannot rewind a generator that was already run";
                    let exc = self.create_exception(b"Exception", msg, line);
                    self.current_exception = Some(exc);
                    return Err(VmError {
                        message: format!("Uncaught Exception: {}", msg),
                        line,
                    });
                }
                Ok(Value::Null)
            }
            b"getreturn" => {
                let (returned, v) = g
                    .try_borrow()
                    .map(|b| {
                        (
                            b.has_returned && b.state == GeneratorState::Completed,
                            b.return_value.clone(),
                        )
                    })
                    .unwrap_or((false, Value::Null));
                if returned {
                    return Ok(v);
                }
                let msg = "Cannot get return value of a generator that hasn't returned";
                let exc = self.create_exception(b"Exception", msg, line);
                self.current_exception = Some(exc);
                Err(VmError {
                    message: format!("Uncaught Exception: {}", msg),
                    line,
                })
            }
            _ => Ok(Value::Null),
        }
    }
}
