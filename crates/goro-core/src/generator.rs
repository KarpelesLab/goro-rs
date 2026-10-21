use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::array::ArrayKey;
use crate::opcode::{OpArray, OpCode, OperandType};
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::{Vm, VmError};

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
#[derive(Debug, Clone)]
pub struct PhpGenerator {
    /// The compiled function body
    pub op_array: OpArray,
    /// Current instruction pointer (where to resume)
    pub ip: usize,
    /// Compiled variables (function locals)
    pub cvs: Vec<Value>,
    /// Temporary variables
    pub tmps: Vec<Value>,
    /// Foreach iteration positions
    pub foreach_positions: HashMap<u32, usize>,
    /// Static variable keys
    pub static_cv_keys: HashMap<u32, Vec<u8>>,
    /// Global variable keys
    pub global_cv_keys: HashMap<u32, Vec<u8>>,
    /// Exception handlers
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
    pub call_file: String,
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
    /// Registered with the cycle collector.
    pub(crate) gc_tracked: std::cell::Cell<bool>,
    /// The collector already ran this generator's destructor
    /// (zend_generator_dtor_storage; IS_OBJ_DESTRUCTOR_CALLED).
    pub(crate) gc_dtor_called: std::cell::Cell<bool>,
    /// The reference yielded by a by-ref generator (bound by `foreach (... as &$v)`).
    pub(crate) current_ref: Option<Rc<RefCell<Value>>>,
    /// Property hook this generator is the body of (`get { yield ...; }`).
    pub(crate) hook_key: Option<Vec<u8>>,
}

impl PhpGenerator {
    /// Create a new generator from a compiled op_array and initial CVs
    pub fn new(op_array: OpArray, cvs: Vec<Value>) -> Self {
        let temp_count = op_array.temp_count as usize;
        Self {
            op_array,
            ip: 0,
            cvs,
            tmps: vec![Value::Undef; temp_count],
            foreach_positions: HashMap::new(),
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
            call_file: String::new(),
            call_line: 0,
            pending_return: None,
            pending_finally_jump: None,
            pending_exception: None,
            force_closed: false,
            has_returned: false,
            advanced_past_first: false,
            saved_calls: Vec::new(),
            gc_tracked: std::cell::Cell::new(false),
            gc_dtor_called: std::cell::Cell::new(false),
            current_ref: None,
            hook_key: None,
        }
    }

    /// Opcodes the generator executor implements itself: suspension,
    /// exception / finally bookkeeping kept in the generator frame, foreach
    /// iterator state, calls (fiber suspension) and a few hot simple ops.
    /// Everything else is delegated to the VM (Vm::execute_single_op).
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
                | OpCode::TypeCheck
                | OpCode::SaveReturn
                | OpCode::SaveJump
                | OpCode::ReturnDeferred
                | OpCode::FinallyEnter
                | OpCode::ForeachInit
                | OpCode::ForeachNext
                | OpCode::ForeachKey
                | OpCode::ForeachInitRef
                | OpCode::ForeachNextRef
                | OpCode::InitFCall
                | OpCode::InitDynamicStaticCall
                | OpCode::InitMethodCall
                | OpCode::SendVal
                | OpCode::SendValChecked
                | OpCode::SendValRaw
                | OpCode::SendNamedVal
                | OpCode::SendUnpack
                | OpCode::SendRef
                | OpCode::DoFCall
                | OpCode::ExitOp
                | OpCode::BindGlobal
                | OpCode::StaticVarInit
                | OpCode::Jmp
                | OpCode::JmpZ
                | OpCode::JmpNz
                | OpCode::Assign
                | OpCode::LoadConst
                | OpCode::QmAssign
                | OpCode::Nop
                | OpCode::FreeTmps
        )
    }

    /// Arithmetic / comparison on plain numbers (and string concatenation)
    /// has no side effects: keep those hot opcodes in the generator loop.
    fn scalar_fast_path(&self, op: &crate::opcode::Op, literals: &[Value]) -> bool {
        let num = |v: &Value| matches!(v, Value::Long(_) | Value::Double(_));
        match op.opcode {
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
        for c in std::mem::take(&mut self.saved_calls) {
            out.extend(c.args);
            out.extend(c.named_args.into_iter().map(|(_, v)| v));
        }
        if let Some(r) = self.current_ref.take() {
            out.push(Value::Reference(r));
        }
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

        self.force_closed = true;

        // If we're yielding from an inner generator, close it first so its
        // finally blocks run before ours (matches PHP semantics).
        if let Some(Value::Generator(inner_rc)) = self.yield_from_source.clone() {
            // Use try_borrow_mut to avoid borrow conflicts; if it's currently
            // borrowed we just skip (it'll be cleaned up on its own drop).
            if let Ok(mut inner) = inner_rc.try_borrow_mut() {
                inner.close(vm);
            }
            self.yield_from_source = None;
        }

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
                            try_stack.push((i, finally_target));
                        }
                    }
                }
                OpCode::TryEnd => {
                    // Pop the matching TryBegin
                    if let Some((begin, finally_target)) = try_stack.pop() {
                        // Check if current_ip is within the try block range
                        if current_ip > begin && current_ip <= i {
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
            if current_ip > *begin {
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
    /// finished generator's frame is freed (its locals are destroyed at the
    /// caller's next release point).
    fn release_finished_frame(&mut self) {
        if !matches!(self.send_value, Value::Null) {
            self.send_value = Value::Null;
        }
        if self.state == GeneratorState::Completed {
            for t in self.tmps.iter_mut() {
                *t = Value::Undef;
            }
            for c in self.cvs.iter_mut() {
                *c = Value::Undef;
            }
            self.current_value = Value::Null;
        }
    }

    /// Resume execution of the generator until the next yield or return.
    /// Returns Ok(true) if a value was yielded, Ok(false) if the generator completed.
    /// Name of the generator function's frame in backtraces, and whether it
    /// is an instance method.
    fn frame_name(&self, vm: &Vm) -> (String, bool) {
        let op = &self.op_array;
        let raw_name = String::from_utf8_lossy(&op.name).to_string();
        let is_instance = op.cv_names.first().map(|n| n.as_slice()) == Some(b"this");
        // Format closure names for stack traces using the canonical display name.
        let name = if !op.display_name.is_empty() {
            String::from_utf8_lossy(&op.display_name).to_string()
        } else if (raw_name.starts_with("__closure_") || raw_name.starts_with("__arrow_"))
            && !raw_name.starts_with("__closure_fcc_")
        {
            let file = String::from_utf8_lossy(&op.filename).to_string();
            format!("{{closure:{}:{}}}", file, op.decl_line)
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

    pub fn resume(&mut self, vm: &mut Vm) -> Result<bool, VmError> {
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
            false
        };
        let pushed_called = if let Some(ref called) = self.called_class {
            vm.called_class_stack.push(called.clone());
            true
        } else {
            false
        };

        // Push a call_stack entry for func_get_args() support
        let (func_name, is_instance) = self.frame_name(vm);
        // The generator's frame in stack traces should reflect the most recent caller.
        // When VM resumes the generator with caller context set (e.g. foreach), we use
        // the caller's file/line; otherwise (Generator method calls, yield-from priming)
        // we show "[internal function]" (empty file, line 0).
        let (func_file, func_line) = if let Some((file, line)) = vm.generator_caller.clone() {
            (file, line)
        } else {
            (String::new(), 0)
        };
        vm.call_stack.push((
            func_name,
            func_file,
            func_line,
            self.args.clone(),
            is_instance,
        ));

        // Restore a deferred exception (yield-in-finally) if we stashed one
        // when this generator suspended. The body's ReturnDeferred will pick
        // it up after the finally completes and re-raise through any outer
        // handlers (or out of the generator).
        if let Some(exc) = self.pending_exception.take() {
            vm.current_exception = Some(exc);
        }

        self.is_running = true;
        let cv_frame = vm.register_cv_frame(&self.op_array);
        if cv_frame {
            vm.set_cv_frame_vars(&self.cvs);
        }
        let result = self.resume_inner(vm);
        if cv_frame {
            vm.cv_frames.pop();
        }
        self.is_running = false;
        self.release_finished_frame();

        // Pop the call_stack entry
        vm.call_stack.pop();

        // Pop class context
        if pushed_called {
            vm.called_class_stack.pop();
        }
        if pushed_scope {
            vm.pop_class_scope();
        }

        vm.leave_generator_resume();

        result
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
    fn resume_inner(&mut self, vm: &mut Vm) -> Result<bool, VmError> {
        if self.state == GeneratorState::Created {
            if let Some(i) = self.op_array.cv_names.iter().position(|n| n == b"GLOBALS") {
                if let Some(slot) = self.cvs.get_mut(i) {
                    *slot = Value::Array(vm.globals_array());
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
                && !self.scalar_fast_path(op, &op_array.literals)
            {
                // Shared semantics: run the VM's implementation of the opcode
                // on this frame's variables.
                match vm.execute_single_op(&op_array, ip - 1, &mut self.cvs, &mut self.tmps) {
                    Ok(next) => ip = next,
                    Err(e) => {
                        if vm.exit_requested {
                            self.state = GeneratorState::Completed;
                            self.ip = ip;
                            return Ok(false);
                        }
                        if vm.current_exception.is_some() {
                            if let Some((catch_target, _, _)) = self.exception_handlers.last() {
                                let ct = *catch_target as usize;
                                self.exception_handlers.pop();
                                ip = ct;
                                continue;
                            }
                        }
                        self.state = GeneratorState::Completed;
                        self.current_value = Value::Null;
                        self.ip = ip;
                        return Err(e);
                    }
                }
                continue;
            }

            match op.opcode {
                OpCode::Yield => {
                    // If generator is force-closed, yield in finally is an error
                    if self.force_closed {
                        let err_msg = "Cannot yield from finally in a force-closed generator";
                        let exc = vm.create_exception(b"Error", err_msg, op.line);
                        vm.current_exception = Some(exc);
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
                    } else {
                        self.read_operand(&op.op1, &op_array.literals)
                    };

                    let yielded_key = if matches!(op.op2, OperandType::Unused) {
                        let key = Value::Long(self.key_counter);
                        self.key_counter += 1;
                        key
                    } else {
                        let key = self.read_operand(&op.op2, &op_array.literals);
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
                    // If generator is force-closed, yield-from in finally is an error.
                    if self.force_closed && self.yield_from_source.is_none() {
                        let err_msg = "Cannot use \"yield from\" in a force-closed generator";
                        let exc = vm.create_exception(b"Error", err_msg, op.line);
                        vm.current_exception = Some(exc);
                        self.state = GeneratorState::Completed;
                        self.current_value = Value::Null;
                        self.ip = ip;
                        return Err(VmError {
                            message: format!("Uncaught Error: {}", err_msg),
                            line: op.line,
                        });
                    }
                    // Check if we're resuming a yield-from iteration
                    if let Some(ref source) = self.yield_from_source.clone() {
                        match source {
                            Value::Array(arr) => {
                                let arr_borrow = arr.borrow();
                                let entries: Vec<_> = arr_borrow.iter().collect();
                                if self.yield_from_pos < entries.len() {
                                    let (key, val) = entries[self.yield_from_pos];
                                    let k = match key {
                                        crate::array::ArrayKey::Int(n) => Value::Long(*n),
                                        crate::array::ArrayKey::String(s) => {
                                            Value::String(s.clone())
                                        }
                                    };
                                    self.current_value = val.clone();
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
                            Value::Generator(inner_gen) => {
                                // If we've already yielded the inner's current value on
                                // a previous resume, advance the inner first. Otherwise
                                // just observe the inner's current state. This way two
                                // generators yielding from the same shared inner see the
                                // same sequence of values without consuming each other's.
                                if self.yield_from_observed {
                                    let mut inner = inner_gen.borrow_mut();
                                    inner.write_send_value();
                                    let prev_caller = vm.generator_caller.take();
                                    let caller_file = if !op_array.filename.is_empty() {
                                        String::from_utf8_lossy(&op_array.filename).to_string()
                                    } else {
                                        vm.current_file.clone()
                                    };
                                    vm.generator_caller = Some((caller_file, op.line));
                                    let resume_outcome = inner.resume(vm);
                                    vm.generator_caller = prev_caller;
                                    drop(inner);
                                    if resume_outcome.is_err() || vm.current_exception.is_some() {
                                        // Inner threw - clear delegation and propagate
                                        self.yield_from_source = None;
                                        self.yield_from_observed = false;
                                        if let Some((catch_target, finally_target, _)) =
                                            self.exception_handlers.pop()
                                        {
                                            if catch_target > 0 {
                                                ip = catch_target as usize;
                                                continue;
                                            } else if finally_target > 0 {
                                                ip = finally_target as usize;
                                                continue;
                                            }
                                        }
                                        self.state = GeneratorState::Completed;
                                        self.ip = ip;
                                        return Err(VmError {
                                            message: "Uncaught exception in generator".to_string(),
                                            line: op.line,
                                        });
                                    }
                                }
                                self.yield_from_observed = false;
                                // Read inner state without holding borrow
                                let (is_suspended, val, key, ret) = {
                                    let inner = inner_gen.borrow();
                                    (
                                        inner.state == GeneratorState::Suspended,
                                        inner.current_value.clone(),
                                        inner.current_key.clone(),
                                        inner.return_value.clone(),
                                    )
                                };
                                if is_suspended {
                                    self.current_value = val;
                                    self.current_key = key;
                                    self.state = GeneratorState::Suspended;
                                    self.ip = ip - 1; // Re-execute YieldFrom on resume
                                    self.yield_from_observed = true;
                                    return Ok(true);
                                }
                                // Inner completed
                                self.yield_from_source = None;
                                self.write_operand(&op.result, ret);
                            }
                            _ => {
                                self.yield_from_source = None;
                                self.write_operand(&op.result, Value::Null);
                            }
                        }
                    } else {
                        // First time: set up the yield-from source
                        let iterable = self.read_operand(&op.op1, &op_array.literals);
                        match &iterable {
                            Value::Array(_) => {
                                self.yield_from_source = Some(iterable);
                                self.yield_from_pos = 0;
                                // Re-execute to start yielding
                                ip -= 1; // Will re-execute this opcode
                            }
                            Value::Generator(inner_gen) => {
                                // Initialize inner generator if needed. The "caller"
                                // for the inner's trace is the yield-from site in
                                // the current op_array, so set generator_caller
                                // accordingly.
                                let mut inner = inner_gen.borrow_mut();
                                if inner.state == GeneratorState::Created {
                                    let prev_caller = vm.generator_caller.take();
                                    let caller_file = if !op_array.filename.is_empty() {
                                        String::from_utf8_lossy(&op_array.filename).to_string()
                                    } else {
                                        vm.current_file.clone()
                                    };
                                    vm.generator_caller = Some((caller_file, op.line));
                                    let exc_before = vm.current_exception.is_some();
                                    let start_outcome = inner.resume(vm);
                                    vm.generator_caller = prev_caller;
                                    if start_outcome.is_err()
                                        && !exc_before
                                        && vm.current_exception.is_some()
                                    {
                                        // Inner threw while starting - propagate.
                                        drop(inner);
                                        if let Some((catch_target, finally_target, _)) =
                                            self.exception_handlers.pop()
                                        {
                                            ip = if catch_target > 0 {
                                                catch_target
                                            } else {
                                                finally_target
                                            }
                                                as usize;
                                            continue;
                                        }
                                        self.state = GeneratorState::Completed;
                                        self.ip = ip;
                                        return Err(VmError {
                                            message: "Uncaught exception in generator".to_string(),
                                            line: op.line,
                                        });
                                    }
                                }
                                drop(inner);
                                self.yield_from_source = Some(iterable);
                                ip -= 1; // Re-execute to start yielding
                            }
                            Value::Object(obj) => {
                                // Check if the object is iterable (Iterator or IteratorAggregate)
                                let class_lower: Vec<u8> = obj
                                    .borrow()
                                    .class_name
                                    .iter()
                                    .map(|b| b.to_ascii_lowercase())
                                    .collect();
                                let is_iterator =
                                    vm.class_implements_interface(&class_lower, b"iterator");
                                let is_iterator_aggregate = !is_iterator
                                    && vm.class_implements_interface(
                                        &class_lower,
                                        b"iteratoraggregate",
                                    );
                                let is_traversable = !is_iterator
                                    && !is_iterator_aggregate
                                    && vm.class_implements_interface(&class_lower, b"traversable");

                                if is_iterator {
                                    // Iterator object - convert to array for simplicity
                                    let exc_before = vm.current_exception.is_some();
                                    let arr = vm.iterator_to_array(&iterable);
                                    if !exc_before && vm.current_exception.is_some() {
                                        if let Some((catch_target, finally_target, _)) =
                                            self.exception_handlers.pop()
                                        {
                                            ip = if catch_target > 0 {
                                                catch_target
                                            } else {
                                                finally_target
                                            }
                                                as usize;
                                            continue;
                                        }
                                        self.state = GeneratorState::Completed;
                                        self.ip = ip;
                                        return Err(VmError {
                                            message: "Uncaught exception in generator".to_string(),
                                            line: op.line,
                                        });
                                    }
                                    self.yield_from_source =
                                        Some(Value::Array(Rc::new(RefCell::new(arr))));
                                    self.yield_from_pos = 0;
                                    ip -= 1;
                                } else if is_iterator_aggregate || is_traversable {
                                    // Call getIterator() to get the underlying iterator
                                    if let Some(iter_val) =
                                        vm.call_object_method(&iterable, b"getIterator", &[])
                                    {
                                        match iter_val {
                                            Value::Generator(inner_gen) => {
                                                let mut inner = inner_gen.borrow_mut();
                                                if inner.state == GeneratorState::Created {
                                                    let _ = inner.resume(vm);
                                                }
                                                drop(inner);
                                                self.yield_from_source =
                                                    Some(Value::Generator(inner_gen));
                                                ip -= 1;
                                            }
                                            _ => {
                                                // Got an iterator object - convert to array for simplicity
                                                let arr = vm.iterator_to_array(&iter_val);
                                                self.yield_from_source =
                                                    Some(Value::Array(Rc::new(RefCell::new(arr))));
                                                self.yield_from_pos = 0;
                                                ip -= 1;
                                            }
                                        }
                                    } else {
                                        self.write_operand(&op.result, Value::Null);
                                    }
                                } else {
                                    // Non-iterable object - create Error exception
                                    let msg =
                                        "Can use \"yield from\" only with arrays and Traversables";
                                    let exc = vm.create_exception(b"Error", msg, op.line);
                                    vm.current_exception = Some(exc);
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
                                    return Err(crate::vm::VmError {
                                        message: format!("Uncaught Error: {}", msg),
                                        line: op.line,
                                    });
                                }
                            }
                            _ => {
                                // Non-iterable - create Error exception
                                let msg =
                                    "Can use \"yield from\" only with arrays and Traversables";
                                let exc = vm.create_exception(b"Error", msg, op.line);
                                vm.current_exception = Some(exc);
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
                                return Err(crate::vm::VmError {
                                    message: format!("Uncaught Error: {}", msg),
                                    line: op.line,
                                });
                            }
                        }
                    }
                }

                OpCode::GeneratorReturn => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
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
                    let val = self.read_operand(&op.op1, &op_array.literals);
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

                // Re-implement all the VM opcodes that a generator might use
                OpCode::Nop => {}

                OpCode::FreeTmps => {
                    if let (OperandType::Tmp(a), OperandType::Tmp(b)) = (op.op1, op.op2) {
                        let end = (b as usize).min(self.tmps.len());
                        let mut freed = false;
                        for slot in self.tmps[(a as usize).min(end)..end].iter_mut() {
                            if matches!(
                                slot,
                                Value::Object(_) | Value::Array(_) | Value::Reference(_)
                            ) {
                                freed = true;
                            }
                            *slot = Value::Undef;
                        }
                        if freed {
                            let had = vm.current_exception.is_some();
                            vm.run_pending_destructors_pub();
                            if !had && vm.current_exception.is_some() {
                                if let Some((catch_target, _, _)) = self.exception_handlers.pop() {
                                    ip = catch_target as usize;
                                    continue;
                                }
                                return Err(VmError {
                                    message: "Uncaught exception from destructor".into(),
                                    line: op.line,
                                });
                            }
                        }
                    }
                }

                OpCode::EmitNotice => {
                    let msg_val = self.read_operand(&op.op1, &op_array.literals);
                    let msg = msg_val.to_php_string().to_string_lossy();
                    vm.emit_notice_at(&msg, op.line);
                }

                OpCode::Echo => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let s = vm.value_to_string(&val);
                    vm.write_output(s.as_bytes());
                }

                OpCode::Print => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let s = val.to_php_string();
                    vm.write_output(s.as_bytes());
                    self.write_operand(&op.result, Value::Long(1));
                }

                OpCode::Assign => {
                    let val = self.read_operand(&op.op2, &op_array.literals);
                    let released = match op.op1 {
                        OperandType::Cv(i) => matches!(
                            self.cvs.get(i as usize),
                            Some(Value::Object(_) | Value::Array(_))
                        ),
                        _ => false,
                    };
                    if matches!(op.result, OperandType::Tmp(_)) {
                        self.write_operand(&op.op1, val);
                        let stored = match op.op1 {
                            OperandType::Cv(i) => self
                                .cvs
                                .get(i as usize)
                                .map(|v| v.deref())
                                .unwrap_or(Value::Null),
                            _ => Value::Null,
                        };
                        self.write_operand(&op.result, stored);
                    } else {
                        self.write_operand(&op.op1, val);
                    }
                    // The source temporary is consumed by the assignment.
                    if let OperandType::Tmp(t) = op.op2 {
                        if let Some(slot) = self.tmps.get_mut(t as usize) {
                            *slot = Value::Undef;
                        }
                    }
                    if released {
                        // The previous value may have lost its last holder.
                        let had = vm.current_exception.is_some();
                        vm.run_pending_destructors_pub();
                        if !had && vm.current_exception.is_some() {
                            if let Some((catch_target, _, _)) = self.exception_handlers.pop() {
                                ip = catch_target as usize;
                                continue;
                            }
                            return Err(VmError {
                                message: "Uncaught exception from destructor".into(),
                                line: op.line,
                            });
                        }
                    }
                }

                OpCode::LoadConst => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    self.write_operand(&op.result, val);
                }

                OpCode::QmAssign => {
                    let v = self.read_operand(&op.op1, &op_array.literals);
                    let v = if matches!(v, Value::Undef) {
                        Value::Null
                    } else {
                        v.deref()
                    };
                    self.write_operand(&op.result, v);
                }

                OpCode::AssignRef
                    if matches!((op.op1, op.op2), (OperandType::Cv(_), OperandType::Tmp(_))) =>
                {
                    if let (OperandType::Cv(target_idx), OperandType::Tmp(tmp_idx)) =
                        (op.op1, op.op2)
                    {
                        let ti = tmp_idx as usize;
                        if ti < self.tmps.len() && (target_idx as usize) < self.cvs.len() {
                            let r = if let Value::Reference(r) = &self.tmps[ti] {
                                r.clone()
                            } else {
                                let r = Rc::new(RefCell::new(self.tmps[ti].clone()));
                                self.tmps[ti] = Value::Reference(r.clone());
                                r
                            };
                            self.cvs[target_idx as usize] = Value::Reference(r);
                        }
                    }
                }

                OpCode::AssignRef => {
                    if let (OperandType::Cv(target_idx), OperandType::Cv(value_idx)) =
                        (op.op1, op.op2)
                    {
                        let ti = target_idx as usize;
                        let vi = value_idx as usize;
                        let ref_cell = if let Value::Reference(r) = &self.cvs[vi] {
                            r.clone()
                        } else {
                            let r = Rc::new(RefCell::new(self.cvs[vi].clone()));
                            self.cvs[vi] = Value::Reference(r.clone());
                            r
                        };
                        self.cvs[ti] = Value::Reference(ref_cell);
                    }
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
                OpCode::Div => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    match a.div(&b) {
                        Ok(result) => self.write_operand(&op.result, result),
                        Err(msg) => {
                            return Err(VmError {
                                message: msg.to_string(),
                                line: op.line,
                            });
                        }
                    }
                }
                OpCode::Mod => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    match a.modulo(&b) {
                        Ok(result) => self.write_operand(&op.result, result),
                        Err(msg) => {
                            return Err(VmError {
                                message: msg.to_string(),
                                line: op.line,
                            });
                        }
                    }
                }
                OpCode::Pow => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.result, a.pow(&b));
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
                OpCode::Negate => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    self.write_operand(&op.result, a.negate());
                }

                OpCode::BitwiseAnd => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.result, Value::Long(a.to_long() & b.to_long()));
                }
                OpCode::BitwiseOr => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.result, Value::Long(a.to_long() | b.to_long()));
                }
                OpCode::BitwiseXor => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.result, Value::Long(a.to_long() ^ b.to_long()));
                }
                OpCode::BoolXor => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    let result = if a.is_truthy() ^ b.is_truthy() {
                        Value::True
                    } else {
                        Value::False
                    };
                    self.write_operand(&op.result, result);
                }
                OpCode::BitwiseNot => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    self.write_operand(&op.result, Value::Long(!a.to_long()));
                }
                OpCode::ShiftLeft => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        Value::Long(a.to_long().wrapping_shl(b.to_long() as u32)),
                    );
                }
                OpCode::ShiftRight => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        Value::Long(a.to_long().wrapping_shr(b.to_long() as u32)),
                    );
                }

                OpCode::BooleanNot => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.is_truthy() {
                            Value::False
                        } else {
                            Value::True
                        },
                    );
                }

                // Comparisons
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
                OpCode::Spaceship => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.result, Value::Long(a.compare(&b)));
                }

                // Compound assignments
                OpCode::AssignAdd => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.op1, cv_val.add(&rhs));
                }
                OpCode::AssignSub => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.op1, cv_val.sub(&rhs));
                }
                OpCode::AssignMul => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.op1, cv_val.mul(&rhs));
                }
                OpCode::AssignDiv => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    match cv_val.div(&rhs) {
                        Ok(result) => self.write_operand(&op.op1, result),
                        Err(msg) => {
                            return Err(VmError {
                                message: msg.to_string(),
                                line: op.line,
                            });
                        }
                    }
                }
                OpCode::AssignMod => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    match cv_val.modulo(&rhs) {
                        Ok(result) => self.write_operand(&op.op1, result),
                        Err(msg) => {
                            return Err(VmError {
                                message: msg.to_string(),
                                line: op.line,
                            });
                        }
                    }
                }
                OpCode::AssignPow => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.op1, cv_val.pow(&rhs));
                }
                OpCode::AssignConcat => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.op1, cv_val.concat(&rhs));
                }
                OpCode::AssignBitwiseAnd => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.op1, Value::Long(cv_val.to_long() & rhs.to_long()));
                }
                OpCode::AssignBitwiseOr => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.op1, Value::Long(cv_val.to_long() | rhs.to_long()));
                }
                OpCode::AssignBitwiseXor => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.op1, Value::Long(cv_val.to_long() ^ rhs.to_long()));
                }
                OpCode::AssignShiftLeft => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.op1,
                        Value::Long(cv_val.to_long().wrapping_shl(rhs.to_long() as u32)),
                    );
                }
                OpCode::AssignShiftRight => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.op1,
                        Value::Long(cv_val.to_long().wrapping_shr(rhs.to_long() as u32)),
                    );
                }

                // Increment / Decrement
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

                // Control flow
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

                // Function calls - delegate to VM
                OpCode::InitFCall => {
                    let name_val = self.read_operand(&op.op1, &op_array.literals);
                    // parent::$prop::get()/set() in a generator hook needs $this.
                    let is_hook_call = matches!(&name_val, Value::String(s)
                        if s.as_bytes().windows(14).any(|w| w.eq_ignore_ascii_case(b"::__property_g") || w.eq_ignore_ascii_case(b"::__property_s")));
                    vm.generator_init_fcall(name_val);
                    if is_hook_call
                        && op_array.cv_names.first().map(|n| n.as_slice()) == Some(b"this")
                    {
                        let this = self.read_operand(&OperandType::Cv(0), &op_array.literals);
                        if matches!(this, Value::Object(_)) {
                            vm.generator_send_val(this);
                        }
                    }
                }
                OpCode::InitDynamicStaticCall => {
                    let class_val = self.read_operand(&op.op1, &op_array.literals);
                    let method_val = self.read_operand(&op.op2, &op_array.literals);
                    let method_name = method_val.to_php_string();
                    let class_name = match &class_val {
                        Value::Object(obj) => obj.borrow().class_name.clone(),
                        Value::String(s) => s.as_bytes().to_vec(),
                        _ => class_val.to_php_string().as_bytes().to_vec(),
                    };
                    let mut func_name = class_name;
                    func_name.extend_from_slice(b"::");
                    func_name.extend_from_slice(method_name.as_bytes());
                    vm.generator_init_fcall(Value::String(PhpString::from_vec(func_name)));
                }
                OpCode::SendVal | OpCode::SendValChecked => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    vm.generator_send_val(val);
                }
                OpCode::SendNamedVal => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let name_val = self.read_operand(&op.op2, &op_array.literals);
                    let name = name_val.to_php_string().as_bytes().to_vec();
                    vm.generator_send_named_val(name, val);
                }
                OpCode::DoFCall => {
                    match vm.generator_do_fcall(op.line, op_array.strict_types) {
                        Ok(result) => {
                            self.write_operand(&op.result, result);
                        }
                        Err(e) => {
                            if vm.fiber_suspended {
                                // Fiber::suspend() was called from within a nested function.
                                // Save the generator state so we can resume later.
                                // ip currently points past the DoFCall; we save it there
                                // because on resume, the send_value will be written to
                                // the result operand as the return value of Fiber::suspend().
                                self.ip = ip;
                                // Store which operand should receive the resume value
                                self.fiber_suspend_result = Some(op.result);
                                return Err(e);
                            }
                            // Check for exception handling in the generator
                            if vm.current_exception.is_some() {
                                if let Some(&(catch_target, finally_target, _)) =
                                    self.exception_handlers.last()
                                {
                                    let ct = if catch_target > 0 {
                                        catch_target
                                    } else {
                                        finally_target
                                    } as usize;
                                    self.exception_handlers.pop();
                                    ip = ct;
                                    continue;
                                }
                            }
                            // Unhandled exception from function call - generator is done
                            self.state = GeneratorState::Completed;
                            self.current_value = Value::Null;
                            self.ip = ip;
                            return Err(e);
                        }
                    }
                }

                // Casts
                OpCode::CastInt => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    self.write_operand(&op.result, Value::Long(val.to_long()));
                }
                OpCode::CastFloat => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    self.write_operand(&op.result, Value::Double(val.to_double()));
                }
                OpCode::CastString => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    self.write_operand(&op.result, Value::String(val.to_php_string()));
                }
                OpCode::CastBool => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if val.is_truthy() {
                            Value::True
                        } else {
                            Value::False
                        },
                    );
                }
                OpCode::CastArray => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let arr = match val {
                        Value::Array(a) => a,
                        other => {
                            let mut arr = crate::array::PhpArray::new();
                            arr.push(other);
                            Rc::new(RefCell::new(arr))
                        }
                    };
                    self.write_operand(&op.result, Value::Array(arr));
                }
                OpCode::CastObject => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let obj = match val {
                        Value::Object(o) => Value::Object(o),
                        _ => {
                            // Simplified - just create an empty stdClass for now
                            let obj = crate::object::PhpObject::new(b"stdClass".to_vec(), 0);
                            Value::Object(Rc::new(RefCell::new(obj)))
                        }
                    };
                    self.write_operand(&op.result, obj);
                }

                // Arrays
                OpCode::ArrayNew => {
                    let arr = Rc::new(RefCell::new(crate::array::PhpArray::new()));
                    self.write_operand(&op.result, Value::Array(arr));
                }
                OpCode::ArrayAppend => {
                    let arr_val = self.read_operand(&op.op1, &op_array.literals);
                    let val = self.read_operand(&op.op2, &op_array.literals);
                    if let Value::Array(arr) = arr_val {
                        arr.borrow_mut().push(val);
                    }
                }
                OpCode::ArraySet => {
                    let arr_val = self.read_operand(&op.op1, &op_array.literals);
                    let val = self.read_operand(&op.op2, &op_array.literals);
                    let key_val = self.read_operand(&op.result, &op_array.literals);
                    if let Value::Array(arr) = arr_val {
                        let key = Vm::value_to_array_key(key_val);
                        arr.borrow_mut().set(key, val);
                    }
                }
                OpCode::ArrayGet | OpCode::ListGet => {
                    let arr_val = self.read_operand(&op.op1, &op_array.literals);
                    let key_val = self.read_operand(&op.op2, &op_array.literals);
                    let result = if let Value::Array(arr) = &arr_val {
                        let key = Vm::value_to_array_key(key_val.clone());
                        arr.borrow().get(&key).cloned().unwrap_or(Value::Null)
                    } else if matches!(op.opcode, OpCode::ArrayGet) {
                        if let Value::String(s) = &arr_val {
                            let idx = key_val.to_long();
                            let bytes = s.as_bytes();
                            if idx >= 0 && (idx as usize) < bytes.len() {
                                Value::String(PhpString::from_bytes(&[bytes[idx as usize]]))
                            } else {
                                Value::String(PhpString::empty())
                            }
                        } else {
                            Value::Null
                        }
                    } else {
                        Value::Null
                    };
                    self.write_operand(&op.result, result);
                }

                // Foreach within generators
                OpCode::ForeachInit => {
                    let arr_val = self.read_operand(&op.op1, &op_array.literals);
                    self.write_operand(&op.result, arr_val);
                    let iter_idx = match op.result {
                        OperandType::Tmp(idx) => idx,
                        _ => 0,
                    };
                    self.foreach_positions.insert(iter_idx, 0usize);
                }
                OpCode::ForeachNext => {
                    let iter_idx = match op.op1 {
                        OperandType::Tmp(idx) => idx,
                        _ => 0,
                    };
                    let pos = self.foreach_positions.get(&iter_idx).copied().unwrap_or(0);
                    let arr_val = self.read_operand(&op.op1, &op_array.literals);
                    if let Value::Array(arr) = &arr_val {
                        let arr_borrow = arr.borrow();
                        let entries: Vec<_> = arr_borrow.iter().collect();
                        if pos >= entries.len() {
                            if let OperandType::JmpTarget(target) = op.op2 {
                                ip = target as usize;
                            }
                        } else {
                            let (_, value) = entries[pos];
                            self.write_operand(&op.result, value.clone());
                            self.foreach_positions.insert(iter_idx, pos + 1);
                        }
                    } else {
                        if let OperandType::JmpTarget(target) = op.op2 {
                            ip = target as usize;
                        }
                    }
                }
                OpCode::ForeachKey => {
                    let iter_idx = match op.op1 {
                        OperandType::Tmp(idx) => idx,
                        _ => 0,
                    };
                    let pos = self.foreach_positions.get(&iter_idx).copied().unwrap_or(1);
                    let arr_val = self.read_operand(&op.op1, &op_array.literals);
                    if let Value::Array(arr) = &arr_val {
                        let arr_borrow = arr.borrow();
                        let entries: Vec<_> = arr_borrow.iter().collect();
                        let actual_pos = pos.saturating_sub(1);
                        if actual_pos < entries.len() {
                            let (key, _) = entries[actual_pos];
                            let key_val = match key {
                                crate::array::ArrayKey::Int(n) => Value::Long(*n),
                                crate::array::ArrayKey::String(s) => Value::String(s.clone()),
                            };
                            self.write_operand(&op.result, key_val);
                        }
                    }
                }

                OpCode::ForeachInitRef => {
                    // By-ref foreach over an array iterates (and modifies) the
                    // variable's own array, separated from other holders.
                    let iter_idx = match op.result {
                        OperandType::Tmp(idx) => idx,
                        _ => 0,
                    };
                    let src = match op.op1 {
                        OperandType::Cv(i) => {
                            let i = i as usize;
                            let cur = self.cvs.get(i).map(|v| v.deref()).unwrap_or(Value::Undef);
                            match cur {
                                Value::Array(a) => {
                                    let a = if Rc::strong_count(&a) > 2 {
                                        let sep = Rc::new(RefCell::new(a.borrow().clone()));
                                        match &mut self.cvs[i] {
                                            Value::Reference(r) => {
                                                *r.borrow_mut() = Value::Array(sep.clone())
                                            }
                                            slot => *slot = Value::Array(sep.clone()),
                                        }
                                        sep
                                    } else {
                                        a
                                    };
                                    Value::Array(a)
                                }
                                other => other,
                            }
                        }
                        _ => match self.read_operand(&op.op1, &op_array.literals) {
                            Value::Reference(r) => r.borrow().clone(),
                            v => v,
                        },
                    };
                    self.write_operand(&op.result, src);
                    self.foreach_positions.insert(iter_idx, 0usize);
                }
                OpCode::ForeachNextRef => {
                    let iter_idx = match op.op1 {
                        OperandType::Tmp(idx) => idx,
                        _ => 0,
                    };
                    let pos = self.foreach_positions.get(&iter_idx).copied().unwrap_or(0);
                    let arr_val = self.read_operand(&op.op1, &op_array.literals);
                    if let Value::Array(arr) = &arr_val {
                        let n = arr.borrow().len();
                        if pos >= n {
                            if let OperandType::JmpTarget(target) = op.op2 {
                                ip = target as usize;
                            }
                        } else {
                            // The element becomes a reference shared with the loop variable.
                            let r = {
                                let mut b = arr.borrow_mut();
                                let slot = b.iter_mut().nth(pos).map(|(_, v)| v);
                                match slot {
                                    Some(Value::Reference(r)) => r.clone(),
                                    Some(v) => {
                                        let r = Rc::new(RefCell::new(std::mem::replace(
                                            v,
                                            Value::Null,
                                        )));
                                        *v = Value::Reference(r.clone());
                                        r
                                    }
                                    None => Rc::new(RefCell::new(Value::Null)),
                                }
                            };
                            // A plain loop variable is rebound to the reference.
                            match op.result {
                                OperandType::Cv(ci) => {
                                    if let Some(slot) = self.cvs.get_mut(ci as usize) {
                                        *slot = Value::Reference(r);
                                    }
                                }
                                _ => self.write_operand(&op.result, Value::Reference(r)),
                            }
                            self.foreach_positions.insert(iter_idx, pos + 1);
                        }
                    } else {
                        if let OperandType::JmpTarget(target) = op.op2 {
                            ip = target as usize;
                        }
                    }
                }

                OpCode::ConstLookup => {
                    let name = self
                        .read_operand(&op.op1, &op_array.literals)
                        .to_php_string();
                    let val = vm.lookup_constant(name.as_bytes());
                    self.write_operand(&op.result, val);
                }

                OpCode::MakeClosure => {
                    let inner = self.read_operand(&op.op1, &op_array.literals);
                    let inner = vm.closure_scope_at_creation(inner, op_array.scope_class.as_ref());
                    let inner = vm.closure_with_own_statics(inner, None);
                    let obj = vm.make_closure_object(inner);
                    self.write_operand(&op.result, obj);
                }
                OpCode::MakeFcc => {
                    let inner = self.read_operand(&op.op1, &op_array.literals);
                    let desc = self
                        .read_operand(&op.op2, &op_array.literals)
                        .to_php_string();
                    let this = if op_array.cv_names.first().map(|n| n.as_slice()) == Some(b"this") {
                        Some(self.read_operand(&OperandType::Cv(0), &op_array.literals))
                            .filter(|v| matches!(v, Value::Object(_)))
                    } else {
                        None
                    };
                    match vm.make_fcc(inner, desc.as_bytes(), this, op_array.scope_class.clone()) {
                        Ok(obj) => self.write_operand(&op.result, obj),
                        Err((cls, msg)) => {
                            if vm.current_exception.is_none() {
                                let exc = vm.create_exception(cls, &msg, op.line);
                                vm.current_exception = Some(exc);
                            }
                            if let Some((catch_target, _, _)) = self.exception_handlers.pop() {
                                ip = catch_target as usize;
                                continue;
                            }
                            self.state = GeneratorState::Completed;
                            self.ip = ip;
                            return Err(VmError {
                                message: format!(
                                    "Uncaught {}: {}",
                                    String::from_utf8_lossy(cls),
                                    msg
                                ),
                                line: op.line,
                            });
                        }
                    }
                }
                OpCode::DeclareFunction => {
                    let name_val = self.read_operand(&op.op1, &op_array.literals);
                    let func_idx_val = self.read_operand(&op.op2, &op_array.literals);
                    let func_idx = func_idx_val.to_long() as usize;
                    if let Some(func_op_array) = op_array.child_functions.get(func_idx) {
                        let name = name_val.to_php_string();
                        vm.register_user_function(name.as_bytes(), func_op_array.clone());
                    }
                }

                OpCode::StaticVarInit => {
                    let key_val = self.read_operand(&op.result, &op_array.literals);
                    let key = key_val.to_php_string().as_bytes().to_vec();
                    let default = if vm.static_vars().contains_key(&key) {
                        Value::Null
                    } else {
                        self.read_operand(&op.op2, &op_array.literals)
                    };
                    let cell = vm.static_var_cell(&key, default);
                    if let OperandType::Cv(cv_idx) = op.op1 {
                        if let Some(slot) = self.cvs.get_mut(cv_idx as usize) {
                            *slot = cell;
                        }
                    }
                }

                OpCode::BindGlobal => {
                    let name_val = self.read_operand(&op.op2, &op_array.literals);
                    let name = name_val.to_php_string().as_bytes().to_vec();
                    // The CV becomes a reference to the global (ZEND_BIND_GLOBAL).
                    let target = match op.op1 {
                        OperandType::Unused => op_array
                            .cv_names
                            .iter()
                            .position(|n| *n == name)
                            .map(|i| OperandType::Cv(i as u32)),
                        o => Some(o),
                    };
                    if let Some(OperandType::Cv(cv_idx)) = target {
                        let cell = vm.global_cell(&name);
                        if let Some(slot) = self.cvs.get_mut(cv_idx as usize) {
                            *slot = Value::Reference(cell);
                        }
                    }
                }

                OpCode::Throw => {
                    let exc_val = self.read_operand(&op.op1, &op_array.literals);
                    vm.current_exception = Some(exc_val);
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
                        self.exception_handlers.push((catch, finally_target, 0));
                    }
                }

                OpCode::TryEnd => {
                    self.exception_handlers.pop();
                }

                // Generators keep the pending exception/return in the
                // executor state while a finally block runs.
                OpCode::FinallyEnter => {}

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
                    self.pending_return = None;
                    self.pending_finally_jump = None;
                    if let Some(exc) = vm.current_exception.take() {
                        self.write_operand(&op.op1, exc);
                    }
                }

                OpCode::SaveReturn => {
                    // Save return value for deferred return (finally blocks)
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    self.pending_return = Some(val);
                    self.pending_finally_jump = None;
                    vm.current_exception = None;
                    self.pop_handlers_for_finally_jump(op_array.ops.get(ip));
                }

                OpCode::SaveJump => {
                    // Save jump target for deferred break/continue (finally blocks)
                    if let OperandType::JmpTarget(target) = op.op1 {
                        self.pending_finally_jump = Some(target);
                    }
                    vm.current_exception = None;
                    self.pop_handlers_for_finally_jump(op_array.ops.get(ip));
                }

                OpCode::ReturnDeferred => {
                    let this_finally_start = if let OperandType::JmpTarget(fs) = op.op1 {
                        fs
                    } else {
                        0
                    };
                    if vm.current_exception.is_some() {
                        self.pending_return = None;
                        self.pending_finally_jump = None;
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
                            if finally_target > 0 {
                                outer_finally = Some(finally_target);
                                self.exception_handlers.pop();
                                break;
                            } else {
                                self.exception_handlers.pop();
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
                        ip = target as usize;
                        continue;
                    }
                    // If there's a pending exception, re-throw it through outer handlers
                    if vm.current_exception.is_some() {
                        if let Some((catch_target, finally_target, _)) =
                            self.exception_handlers.pop()
                        {
                            if catch_target > 0 {
                                ip = catch_target as usize;
                                continue;
                            } else if finally_target > 0 {
                                ip = finally_target as usize;
                                continue;
                            }
                        }
                        // No outer handler - uncaught exception
                        self.state = GeneratorState::Completed;
                        self.current_value = Value::Null;
                        self.ip = ip;
                        #[allow(clippy::unnecessary_unwrap)]
                        // checked just above; keeps the error path flat
                        let exc = vm.current_exception.as_ref().unwrap();
                        let msg = if let Value::Object(obj) = exc {
                            let obj = obj.borrow();
                            let class = String::from_utf8_lossy(&obj.class_name).to_string();
                            let message = obj.get_property(b"message");
                            format!(
                                "Uncaught {}: {}",
                                class,
                                message.to_php_string().to_string_lossy()
                            )
                        } else {
                            "Uncaught exception in generator".to_string()
                        };
                        return Err(VmError {
                            message: msg,
                            line: op.line,
                        });
                    }
                    // Otherwise continue (no pending return, jump, or exception)
                }

                OpCode::TypeCheck => {
                    let exc_val = self.read_operand(&op.op1, &op_array.literals);
                    let type_name = self.read_operand(&op.op2, &op_array.literals);
                    let type_str = type_name.to_php_string();
                    let type_lower: Vec<u8> = type_str
                        .as_bytes()
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect();
                    let matches = if let Value::Object(obj) = &exc_val {
                        let class_lower: Vec<u8> = obj
                            .borrow()
                            .class_name
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect();
                        class_lower == type_lower
                            || crate::vm::is_builtin_subclass(&class_lower, &type_lower)
                            || vm.class_extends(&class_lower, &type_lower)
                            || type_lower == b"throwable"
                    } else {
                        false
                    };
                    self.write_operand(
                        &op.result,
                        if matches { Value::True } else { Value::False },
                    );
                }

                OpCode::PropertyGet => {
                    let obj_val = self.read_operand(&op.op1, &op_array.literals);
                    let prop_name = self
                        .read_operand(&op.op2, &op_array.literals)
                        .to_php_string();
                    let result = if let Value::Object(obj) = &obj_val {
                        obj.borrow().get_property(prop_name.as_bytes())
                    } else {
                        Value::Null
                    };
                    self.write_operand(&op.result, result);
                }

                OpCode::PropertySet => {
                    let obj_val = self.read_operand(&op.op1, &op_array.literals);
                    let prop_name = self
                        .read_operand(&op.op2, &op_array.literals)
                        .to_php_string();
                    let val = self.read_operand(&op.result, &op_array.literals);
                    if let Value::Object(obj) = &obj_val {
                        obj.borrow_mut()
                            .set_property(prop_name.as_bytes().to_vec(), val);
                    } else if let Value::Generator(_) = &obj_val {
                        let prop_str = prop_name.to_string_lossy();
                        let msg =
                            format!("Cannot create dynamic property Generator::${}", prop_str);
                        let exc = vm.create_exception(b"Error", &msg, op.line);
                        vm.current_exception = Some(exc);
                        return Err(VmError {
                            message: format!("Uncaught Error: {}", msg),
                            line: op.line,
                        });
                    }
                }

                OpCode::IssetCheck => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let result = match val {
                        Value::Null | Value::Undef => Value::False,
                        _ => Value::True,
                    };
                    self.write_operand(&op.result, result);
                }

                OpCode::ArrayIsset => {
                    // isset($arr[$key]) in generator context
                    let arr_val = self.read_operand(&op.op1, &op_array.literals);
                    let key_val = self.read_operand(&op.op2, &op_array.literals);
                    let result = if let Value::Array(arr) = &arr_val {
                        let key = Vm::value_to_array_key(key_val.clone());
                        match arr.borrow().get(&key) {
                            Some(v) if !matches!(v, Value::Null | Value::Undef) => Value::True,
                            _ => Value::False,
                        }
                    } else if let Value::Object(obj) = &arr_val {
                        let class_lower: Vec<u8> = obj
                            .borrow()
                            .class_name
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect();
                        let is_spl_array = matches!(
                            class_lower.as_slice(),
                            b"arrayobject"
                                | b"arrayiterator"
                                | b"recursivearrayiterator"
                                | b"splfixedarray"
                                | b"splobjectstorage"
                        );
                        let has_user_offset = vm
                            .classes
                            .get(&class_lower)
                            .map(|c| c.get_method(b"offsetexists").is_some())
                            .unwrap_or(false);
                        if is_spl_array || has_user_offset {
                            let args = vec![arr_val.clone(), key_val.clone()];
                            let exists_result = vm
                                .handle_spl_docall(&class_lower, b"offsetexists", &args)
                                .unwrap_or_else(|| {
                                    vm.call_object_method(&arr_val, b"offsetexists", &[key_val])
                                        .unwrap_or(Value::False)
                                });
                            if exists_result.is_truthy() {
                                Value::True
                            } else {
                                Value::False
                            }
                        } else {
                            Value::False
                        }
                    } else if let Value::String(s) = &arr_val {
                        let idx = key_val.to_long();
                        let len = s.as_bytes().len() as i64;
                        if (idx >= 0 && idx < len) || (idx < 0 && (-idx) <= len) {
                            Value::True
                        } else {
                            Value::False
                        }
                    } else {
                        Value::False
                    };
                    self.write_operand(&op.result, result);
                }

                OpCode::ErrorSuppress => {
                    vm.error_reporting_stack.push(vm.error_reporting);
                    vm.error_reporting = crate::errdisplay::begin_silence(
                        vm.error_reporting,
                        matches!(op.op1, crate::opcode::OperandType::Const(_)),
                    );
                }

                OpCode::ErrorRestore => {
                    if let Some(saved) = vm.error_reporting_stack.pop() {
                        vm.error_reporting =
                            crate::errdisplay::end_silence(vm.error_reporting, saved);
                    }
                }

                OpCode::UnsetCv => {
                    let mut released = false;
                    if let OperandType::Cv(idx) = op.op1 {
                        if let Some(slot) = self.cvs.get_mut(idx as usize) {
                            released = matches!(slot, Value::Object(_) | Value::Array(_));
                            *slot = Value::Undef;
                        }
                    }
                    if released {
                        let had = vm.current_exception.is_some();
                        vm.run_pending_destructors_pub();
                        if !had && vm.current_exception.is_some() {
                            if let Some((catch_target, _, _)) = self.exception_handlers.pop() {
                                ip = catch_target as usize;
                                continue;
                            }
                            return Err(VmError {
                                message: "Uncaught exception from destructor".into(),
                                line: op.line,
                            });
                        }
                    }
                }

                OpCode::ArrayUnset => {
                    let key_val = self.read_operand(&op.op2, &op_array.literals);
                    let arr_val = if let OperandType::Cv(idx) = &op.op1 {
                        self.cvs.get(*idx as usize).cloned()
                    } else {
                        None
                    };
                    if let Some(Value::Array(arr)) = arr_val {
                        let key = Vm::value_to_array_key(key_val);
                        arr.borrow_mut().remove(&key);
                    }
                }

                OpCode::CloneObj => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let cloned = match &val {
                        Value::Object(_) if val.is_closure_obj() => vm.clone_closure(&val),
                        Value::Object(obj) => {
                            let obj_borrow = obj.borrow();
                            let clone_id = vm.next_object_id();
                            let mut new_obj = crate::object::PhpObject::new(
                                obj_borrow.class_name.clone(),
                                clone_id,
                            );
                            for (name, value) in &obj_borrow.properties {
                                new_obj.set_property(name.clone(), value.clone());
                            }
                            Value::Object(Rc::new(RefCell::new(new_obj)))
                        }
                        other => other.clone(),
                    };
                    self.write_operand(&op.result, cloned);
                }

                OpCode::ArraySpread => {
                    let target_val = if let OperandType::Tmp(idx) = &op.op1 {
                        self.tmps.get(*idx as usize).cloned()
                    } else if let OperandType::Cv(idx) = &op.op1 {
                        self.cvs.get(*idx as usize).cloned()
                    } else {
                        None
                    };
                    let source = self.read_operand(&op.op2, &op_array.literals);
                    if let (Some(Value::Array(target)), Value::Array(source_arr)) =
                        (target_val, source)
                    {
                        let source_borrow = source_arr.borrow();
                        let mut target_borrow = target.borrow_mut();
                        for (key, val) in source_borrow.iter() {
                            match key {
                                crate::array::ArrayKey::Int(_) => {
                                    target_borrow.push(val.clone());
                                }
                                crate::array::ArrayKey::String(s) => {
                                    target_borrow.set(
                                        crate::array::ArrayKey::String(s.clone()),
                                        val.clone(),
                                    );
                                }
                            }
                        }
                    }
                }

                OpCode::InitMethodCall => {
                    // Set up method call from generator context
                    let obj_val = self.read_operand(&op.op1, &op_array.literals);
                    let method_name = self
                        .read_operand(&op.op2, &op_array.literals)
                        .to_php_string();
                    if obj_val.is_closure_obj() {
                        if let Err(msg) =
                            vm.init_closure_method_call(&obj_val, method_name.as_bytes())
                        {
                            let exc = vm.create_exception(b"Error", &msg, op.line);
                            vm.current_exception = Some(exc);
                            if let Some((catch_target, _, _)) = self.exception_handlers.pop() {
                                ip = catch_target as usize;
                                continue;
                            }
                            self.state = GeneratorState::Completed;
                            self.ip = ip;
                            return Err(VmError {
                                message: format!("Uncaught Error: {}", msg),
                                line: op.line,
                            });
                        }
                        continue;
                    }
                    if let Value::Object(obj) = &obj_val {
                        let class_name_lower: Vec<u8> = obj
                            .borrow()
                            .class_name
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect();
                        let method_lower: Vec<u8> = method_name
                            .as_bytes()
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect();
                        // Check if method exists in user classes
                        let has_method = vm
                            .classes
                            .get(&class_name_lower)
                            .map(|c| c.get_method(&method_lower).is_some())
                            .unwrap_or(false);
                        // Handle built-in Throwable methods in generator context
                        let is_throwable = class_name_lower == b"exception"
                            || class_name_lower == b"error"
                            || crate::vm::is_builtin_subclass(&class_name_lower, b"exception")
                            || crate::vm::is_builtin_subclass(&class_name_lower, b"error")
                            || vm.class_extends(&class_name_lower, b"exception")
                            || vm.class_extends(&class_name_lower, b"error");
                        if is_throwable && !has_method {
                            let obj_borrow = obj.borrow();
                            let builtin_result = match method_lower.as_slice() {
                                b"getmessage" => Some(obj_borrow.get_property(b"message")),
                                b"getcode" => Some(obj_borrow.get_property(b"code")),
                                b"getfile" => Some(obj_borrow.get_property(b"file")),
                                b"getline" => Some(obj_borrow.get_property(b"line")),
                                b"gettrace" => Some(obj_borrow.get_property(b"trace")),
                                b"getprevious" => Some(obj_borrow.get_property(b"previous")),
                                _ => None,
                            };
                            drop(obj_borrow);
                            if let Some(result) = builtin_result {
                                vm.generator_init_fcall(Value::String(PhpString::from_bytes(
                                    b"__builtin_return",
                                )));
                                vm.generator_send_val(result);
                            } else {
                                // Fall through to general handling
                                let mut func_name = class_name_lower.clone();
                                func_name.extend_from_slice(b"::");
                                func_name.extend_from_slice(&method_lower);
                                vm.generator_init_fcall(Value::String(PhpString::from_vec(
                                    func_name,
                                )));
                                vm.generator_send_val(obj_val.clone());
                            }
                        } else
                        // Handle Fiber methods in generator context
                        if class_name_lower == b"fiber" {
                            match method_lower.as_slice() {
                                b"__construct" => {
                                    // Fiber constructor - set up as fiber::__construct
                                    vm.generator_init_fcall(Value::String(PhpString::from_bytes(
                                        b"fiber::__construct",
                                    )));
                                    vm.generator_send_val(obj_val.clone());
                                }
                                b"isstarted" | b"isrunning" | b"issuspended" | b"isterminated"
                                | b"getreturn" => {
                                    let mut fiber_name = b"__fiber::".to_vec();
                                    fiber_name.extend_from_slice(&method_lower);
                                    vm.generator_init_fcall(Value::String(PhpString::from_vec(
                                        fiber_name,
                                    )));
                                    vm.generator_send_val(obj_val.clone());
                                }
                                b"start" | b"resume" | b"throw" => {
                                    let mut fiber_name = b"__fiber::".to_vec();
                                    fiber_name.extend_from_slice(&method_lower);
                                    vm.generator_init_fcall(Value::String(PhpString::from_vec(
                                        fiber_name,
                                    )));
                                    vm.generator_send_val(obj_val.clone());
                                }
                                _ => {
                                    self.state = GeneratorState::Completed;
                                    self.ip = ip;
                                    return Err(VmError {
                                        message: format!(
                                            "Call to undefined method Fiber::{}()",
                                            method_name.to_string_lossy()
                                        ),
                                        line: op.line,
                                    });
                                }
                            }
                        } else if has_method {
                            // Set up as a method call: name = "class::method", first arg = $this
                            let mut func_name = class_name_lower.clone();
                            func_name.extend_from_slice(b"::");
                            func_name.extend_from_slice(&method_lower);
                            vm.generator_init_fcall(Value::String(PhpString::from_vec(func_name)));
                            vm.generator_send_val(obj_val.clone());
                        } else {
                            // No user method exists. For stdClass specifically (which
                            // has no instance methods other than the implicit __construct),
                            // raise the canonical "Call to undefined method" Error here so
                            // the trace shows the right call frame instead of falling
                            // through to a generic "undefined function" diagnostic.
                            if class_name_lower == b"stdclass" && method_lower != b"__construct" {
                                let class_orig =
                                    String::from_utf8_lossy(&obj.borrow().class_name).to_string();
                                let msg = format!(
                                    "Call to undefined method {}::{}()",
                                    class_orig,
                                    method_name.to_string_lossy()
                                );
                                let exc = vm.create_exception(b"Error", &msg, op.line);
                                vm.current_exception = Some(exc);
                                if let Some((catch_target, finally_target, _)) =
                                    self.exception_handlers.pop()
                                {
                                    if catch_target > 0 {
                                        ip = catch_target as usize;
                                        continue;
                                    } else if finally_target > 0 {
                                        ip = finally_target as usize;
                                        continue;
                                    }
                                }
                                self.state = GeneratorState::Completed;
                                self.ip = ip;
                                return Err(VmError {
                                    message: format!("Uncaught Error: {}", msg),
                                    line: op.line,
                                });
                            }
                            // Try calling built-in/SPL methods via the VM
                            // Set up as a generic method call that the VM's DoFCall handler will dispatch
                            let mut func_name = class_name_lower.clone();
                            func_name.extend_from_slice(b"::");
                            func_name.extend_from_slice(&method_lower);
                            vm.generator_init_fcall(Value::String(PhpString::from_vec(func_name)));
                            vm.generator_send_val(obj_val.clone());
                        }
                    } else if let Value::Generator(inner_gen_rc) = &obj_val {
                        // Calling a method on another generator from within this generator
                        let method_lower: Vec<u8> = method_name
                            .as_bytes()
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect();
                        // Check if the inner generator is already running (to prevent
                        // "Cannot resume an already running generator" crash/panic).
                        // If try_borrow() fails, the RefCell is mutably borrowed = generator is running.
                        // If it succeeds, check the is_running flag for nested resume detection.
                        let already_running = inner_gen_rc
                            .try_borrow()
                            .map(|b| b.is_running)
                            .unwrap_or(true);
                        if already_running
                            && (method_lower == b"next"
                                || method_lower == b"send"
                                || method_lower == b"rewind")
                        {
                            // Push a Generator->X frame so the exception's recorded
                            // backtrace includes the inner call site (matches PHP's
                            // output for "Cannot resume an already running generator"),
                            // build the Error exception while the frame is on the
                            // stack, then pop and propagate as a regular thrown error.
                            let frame_method = match method_lower.as_slice() {
                                b"next" => "Generator->next",
                                b"send" => "Generator->send",
                                b"rewind" => "Generator->rewind",
                                _ => "Generator->?",
                            };
                            vm.call_stack.push((
                                frame_method.to_string(),
                                vm.current_file.clone(),
                                op.line,
                                Vec::new(),
                                true,
                            ));
                            let err_msg = "Cannot resume an already running generator";
                            let exc = vm.create_exception(b"Error", err_msg, op.line);
                            vm.call_stack.pop();
                            vm.current_exception = Some(exc);
                            // Propagate as a thrown exception so any inner try/catch
                            // can intercept it (the original test does exactly this).
                            if let Some((catch_target, finally_target, _)) =
                                self.exception_handlers.pop()
                            {
                                if catch_target > 0 {
                                    ip = catch_target as usize;
                                    continue;
                                } else if finally_target > 0 {
                                    ip = finally_target as usize;
                                    continue;
                                }
                            }
                            self.state = GeneratorState::Completed;
                            self.ip = ip;
                            return Err(VmError {
                                message: format!("Uncaught Error: {}", err_msg),
                                line: op.line,
                            });
                        }
                        // Push a frame for the duration of the inner method call so
                        // backtraces and exception traces include the call site.
                        let push_inner_frame = matches!(
                            method_lower.as_slice(),
                            b"current" | b"key" | b"next" | b"valid" | b"rewind"
                        );
                        let inner_frame_method = match method_lower.as_slice() {
                            b"current" => "Generator->current",
                            b"key" => "Generator->key",
                            b"next" => "Generator->next",
                            b"valid" => "Generator->valid",
                            b"rewind" => "Generator->rewind",
                            _ => "",
                        };
                        if push_inner_frame {
                            vm.call_stack.push((
                                inner_frame_method.to_string(),
                                vm.current_file.clone(),
                                op.line,
                                Vec::new(),
                                true,
                            ));
                        }
                        let result = match method_lower.as_slice() {
                            b"valid" => {
                                if let Ok(mut inner) = inner_gen_rc.try_borrow_mut() {
                                    if inner.state == GeneratorState::Created {
                                        let _ = inner.resume(vm);
                                    }
                                    if inner.state == GeneratorState::Completed {
                                        Value::False
                                    } else {
                                        Value::True
                                    }
                                } else {
                                    Value::False
                                }
                            }
                            b"current" => {
                                if let Ok(mut inner) = inner_gen_rc.try_borrow_mut() {
                                    if inner.state == GeneratorState::Created {
                                        let _ = inner.resume(vm);
                                    }
                                    inner.current_value.clone()
                                } else {
                                    Value::Null
                                }
                            }
                            b"key" => {
                                if let Ok(mut inner) = inner_gen_rc.try_borrow_mut() {
                                    if inner.state == GeneratorState::Created {
                                        let _ = inner.resume(vm);
                                    }
                                    inner.current_key.clone()
                                } else {
                                    Value::Null
                                }
                            }
                            b"next" => {
                                if let Ok(mut inner) = inner_gen_rc.try_borrow_mut() {
                                    if inner.state == GeneratorState::Created {
                                        let _ = inner.resume(vm);
                                    }
                                    inner.advanced_past_first = true;
                                    inner.write_send_value();
                                    let _ = inner.resume(vm);
                                }
                                Value::Null
                            }
                            b"rewind" => Value::Null, // no-op for started generators
                            b"send" => {
                                // send() will be handled via __generator_send in DoFCall
                                // Set up a pending call with the generator as first arg
                                vm.generator_init_fcall(Value::String(
                                    crate::string::PhpString::from_bytes(b"__generator_send"),
                                ));
                                vm.generator_send_val(obj_val.clone());
                                // The actual argument(s) will be added by subsequent SendVal opcodes
                                // Don't push __builtin_return here; let DoFCall handle it
                                continue; // skip the push_pending below
                            }
                            _ => Value::Null,
                        };
                        if push_inner_frame {
                            vm.call_stack.pop();
                        }
                        vm.generator_init_fcall(Value::String(
                            crate::string::PhpString::from_bytes(b"__builtin_return"),
                        ));
                        vm.generator_send_val(result);
                    } else {
                        self.state = GeneratorState::Completed;
                        self.ip = ip;
                        return Err(VmError {
                            message: format!(
                                "Call to a member function {}() on {}",
                                method_name.to_string_lossy(),
                                Vm::value_type_name(&obj_val)
                            ),
                            line: op.line,
                        });
                    }
                }

                OpCode::NewObject => {
                    // Create new object from generator context
                    let class_name = self
                        .read_operand(&op.op1, &op_array.literals)
                        .to_php_string();
                    let class_lower: Vec<u8> = class_name
                        .as_bytes()
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect();
                    let canonical = vm.builtin_canonical_name(&class_lower);
                    let mut obj = crate::object::PhpObject::new(
                        if let Some(cls) = vm.classes.get(&class_lower) {
                            cls.name.clone()
                        } else {
                            canonical.as_bytes().to_vec()
                        },
                        vm.next_object_id(),
                    );
                    // Initialize exception/error properties
                    let is_throwable = class_lower == b"exception"
                        || class_lower == b"error"
                        || crate::vm::is_builtin_subclass(&class_lower, b"exception")
                        || crate::vm::is_builtin_subclass(&class_lower, b"error")
                        || vm.class_extends(&class_lower, b"exception")
                        || vm.class_extends(&class_lower, b"error");
                    if is_throwable {
                        obj.set_property(b"message".to_vec(), Value::String(PhpString::empty()));
                        obj.set_property(b"code".to_vec(), Value::Long(0));
                        obj.set_property(
                            b"file".to_vec(),
                            Value::String(PhpString::from_string(vm.current_file.clone())),
                        );
                        obj.set_property(b"line".to_vec(), Value::Long(op.line as i64));
                        // Capture the current stack trace (includes generator and caller frames)
                        let trace = vm.build_exception_trace();
                        obj.set_property(b"trace".to_vec(), trace);
                        obj.set_property(b"previous".to_vec(), Value::Null);
                    }
                    // Initialize properties from class definition
                    if let Some(class) = vm.classes.get(&class_lower) {
                        let props: Vec<_> = class
                            .properties
                            .iter()
                            .filter(|p| !p.is_static)
                            .map(|p| (p.name.clone(), p.default.clone()))
                            .collect();
                        for (name, default) in props {
                            obj.set_property(name, default);
                        }
                    }
                    // Destructible objects are tracked like in the VM's NewObject;
                    // the destructor is armed once the constructor call completes.
                    let has_destruct = vm
                        .classes
                        .get(&class_lower)
                        .map(|c| c.methods.contains_key(&b"__destruct".to_vec()))
                        .unwrap_or(false);
                    if has_destruct {
                        obj.set_property(b"__ctor_pending".to_vec(), Value::True);
                    }
                    let obj_val = Value::Object(Rc::new(RefCell::new(obj)));
                    if has_destruct {
                        vm.list_destructible(obj_val.clone());
                    }
                    self.write_operand(&op.result, obj_val);
                    // Note: constructor call will be set up by the subsequent InitMethodCall opcode
                }

                OpCode::SendUnpack
                    if matches!(
                        self.read_operand(&op.op1, &op_array.literals).deref(),
                        Value::Array(_)
                    ) =>
                {
                    // f(...$array): each element is an argument (string keys
                    // are named arguments)
                    if let Value::Array(a) = self.read_operand(&op.op1, &op_array.literals).deref()
                    {
                        let items: Vec<(ArrayKey, Value)> = a
                            .borrow()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        for (k, v) in items {
                            match k {
                                ArrayKey::String(s) => {
                                    vm.generator_send_named_val(s.as_bytes().to_vec(), v)
                                }
                                ArrayKey::Int(_) => vm.generator_send_val(v),
                            }
                        }
                    }
                }
                OpCode::SendRef | OpCode::MakeRef | OpCode::SendUnpack => {
                    // Handle SendRef like SendVal in generator context
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    vm.generator_send_val(val);
                }

                OpCode::FastConcat => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    let a_str = a.to_php_string();
                    let b_str = b.to_php_string();
                    let mut result = a_str.as_bytes().to_vec();
                    result.extend_from_slice(b_str.as_bytes());
                    self.write_operand(&op.result, Value::String(PhpString::from_vec(result)));
                }

                OpCode::GetClassName => {
                    let obj = self.read_operand(&op.op1, &op_array.literals);
                    let name = match &obj {
                        Value::Object(o) => {
                            Value::String(PhpString::from_vec(o.borrow().class_name.clone()))
                        }
                        _ => Value::String(PhpString::empty()),
                    };
                    self.write_operand(&op.result, name);
                }

                OpCode::ExitOp => {
                    // exit/die inside a generator/fiber: output value and signal exit
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    match &val {
                        Value::Long(_) => {}
                        _ => {
                            let s = val.to_php_string();
                            vm.write_output(s.as_bytes());
                        }
                    }
                    self.state = GeneratorState::Completed;
                    self.ip = ip;
                    vm.exit_requested = true;
                    return Ok(false);
                }

                // Every other opcode runs through the VM's implementation on
                // this frame's variables.
                _ => match vm.execute_single_op(&op_array, ip - 1, &mut self.cvs, &mut self.tmps) {
                    Ok(next) => ip = next,
                    Err(e) => {
                        if vm.current_exception.is_some() {
                            if let Some((catch_target, _, _)) = self.exception_handlers.last() {
                                let ct = *catch_target as usize;
                                self.exception_handlers.pop();
                                ip = ct;
                                continue;
                            }
                        }
                        self.state = GeneratorState::Completed;
                        self.current_value = Value::Null;
                        self.ip = ip;
                        return Err(e);
                    }
                },
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
    pub fn resume_with_exception(&mut self, vm: &mut Vm) -> Result<bool, VmError> {
        if self.state == GeneratorState::Completed {
            return Ok(false);
        }

        // If we're delegating via yield-from to an inner generator, route the
        // throw into the inner first. The inner may catch and continue (yielding
        // a new value), in which case we just observe its new state. If the
        // inner doesn't catch, the throw propagates up through our normal
        // resume path. Matches PHP semantics for basic_yield_from_exception_handling.
        if let Some(Value::Generator(inner_rc)) = self.yield_from_source.clone() {
            let inner_state = inner_rc.try_borrow().map(|b| b.state.clone()).ok();
            if matches!(inner_state, Some(GeneratorState::Suspended)) {
                let inner_result = {
                    let mut inner = inner_rc.borrow_mut();
                    let prev_caller = vm.generator_caller.take();
                    let r = inner.resume_with_exception(vm);
                    vm.generator_caller = prev_caller;
                    r
                };
                match inner_result {
                    Ok(true) => {
                        // Inner caught and yielded again. Snapshot value/key
                        // into us and stay suspended.
                        let (val, key) = {
                            let inner = inner_rc.borrow();
                            (inner.current_value.clone(), inner.current_key.clone())
                        };
                        self.current_value = val;
                        self.current_key = key;
                        // Need to clear `yield_from_observed` so the next
                        // resume of OUR generator advances the inner first.
                        self.yield_from_observed = true;
                        return Ok(true);
                    }
                    Ok(false) => {
                        // Inner completed; fall through to local resume so the
                        // body continues past the yield-from.
                        self.yield_from_source = None;
                        self.yield_from_observed = false;
                    }
                    Err(_) => {
                        // Inner threw uncaught; vm.current_exception is set.
                        // Fall through to our resume to let our handler catch.
                        self.yield_from_source = None;
                        self.yield_from_observed = false;
                    }
                }
            }
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
            false
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
        // For resume_with_exception, use call_line=0 and empty file to show [internal function]
        // (matches PHP: when an exception is thrown INTO the generator, the
        // top frame shows the generator function was called from [internal function]
        // which is Generator->throw itself).
        vm.call_stack
            .push((func_name, String::new(), 0, self.args.clone(), is_instance));

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
        let cv_frame = vm.register_cv_frame(&self.op_array);
        if cv_frame {
            vm.set_cv_frame_vars(&self.cvs);
        }
        let result = self.resume_inner(vm);
        if cv_frame {
            vm.cv_frames.pop();
        }
        self.is_running = false;
        self.release_finished_frame();

        vm.call_stack.pop();
        if pushed_called {
            vm.called_class_stack.pop();
        }
        if pushed_scope {
            vm.pop_class_scope();
        }

        vm.leave_generator_resume();

        result
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
