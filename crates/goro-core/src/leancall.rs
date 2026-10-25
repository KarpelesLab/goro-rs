//! Calls of plain user functions run without a frame of their own.
//!
//! DO_FCALL of a user function sets up a lot before the callee's first
//! opcode runs: a backtrace frame (its name, file, line and a copy of the
//! arguments), the registration of the callee's parameters for backtraces,
//! the executor's own frame and its native stack... Nothing reads any of it
//! while the callee only runs opcodes run_fast_ops() handles: they raise
//! nothing, call no user code and depend on no frame state but the class
//! scope.
//!
//! So a call of a plain user function (see lean_function()) that
//! run_fast_ops() meets runs its callee's opcodes in run_fast_ops() too,
//! with just the callee's variables and temporaries (a "lean" call): right
//! after its INIT_FCALL when the arguments are plain sends of the next
//! opcodes (see Vm::lean_fused), else at its DO_FCALL. When the callee reaches a RETURN whose release of its
//! variables only frees memory (see drops_quietly()), the call ends as
//! DO_FCALL's fast path would end it. When it stops at any other opcode,
//! the call is handed over to that fast path (a LeanResume, taken by the
//! DO_FCALL of the call): it sets up all of the frame, as it would have
//! before the callee's first opcode, and runs the callee from that opcode
//! on (see Vm::resume_frame).

use std::cell::RefCell;
use std::rc::Rc;

use super::builtincall::LeanFrame;
use super::{ForeachState, PendingCall, Vm, arg_by_ref, new_foreach_state};
use crate::fastpath::{FeView, is_plain_scalar, object_release_quiet, store_tmp, tmp_writable};
use crate::opcode::{Op, OpArray, OpCode, OperandType};
use crate::value::Value;

/// A lean call stopped at an opcode run_fast_ops() does not run: what the
/// DO_FCALL of the call needs to finish it (see Vm::take_lean_resume).
pub(crate) struct LeanResume {
    /// The DO_FCALL: its code, its index there, and the depth of the frame
    /// running it (call_depth plus the lean calls around it).
    caller: *const OpArray,
    caller_ip: usize,
    depth: u32,
    user_fn: Rc<OpArray>,
    /// (of a method: where "::" is in the call's name, and whether `$this`
    /// leads the arguments: not for a static method)
    sep: Option<(usize, bool)>,
    /// The call (its arguments were moved to the callee's variables).
    call: PendingCall,
    cvs: Vec<Value>,
    frame: ResumeFrame,
}

/// What a resumed lean call's frame starts with (see Vm::resume_frame).
pub(crate) struct ResumeFrame {
    pub(crate) tmps: Vec<Value>,
    pub(crate) fe: Box<ForeachState>,
    /// The opcode to run first.
    pub(crate) ip: usize,
}

/// Whether a call of `f` may be a lean one (what DO_FCALL's fast path
/// accepts of a plain function or of a public instance method of a user
/// class, see Vm::fast_fcall_simple, without what the frame would have to
/// check or set up: see OpArray::lean_kind). `sep`: of a method, whether
/// it is an instance method.
#[inline]
fn lean_function(f: &OpArray, sep: Option<(usize, bool)>) -> bool {
    match sep {
        None => f.lean_kind() & 3 == 1 && f.scope_class.is_none(),
        Some((_, this)) => f.lean_kind() & 3 == 1 + u8::from(this) && f.scope_class.is_some(),
    }
}

/// How many of a lean call's first arguments are not its parameters (its
/// `$this`).
#[inline]
fn lead(sep: Option<(usize, bool)>) -> usize {
    usize::from(sep.is_some_and(|(_, this)| this))
}

/// Whether the last holder of `v` letting it go here only frees memory
/// and makes possible cycle roots (no resource is freed, no generator
/// closed, nothing a destructor or a release point would see): plain
/// values, containers held elsewhere too, and containers of such values.
fn drops_quietly(v: &Value) -> bool {
    fn shallow(v: &Value) -> bool {
        match v {
            Value::Array(a) => Rc::strong_count(a) > 1,
            Value::Object(o) => Rc::strong_count(o) > 1,
            Value::Reference(r) => Rc::strong_count(r) > 1,
            Value::Generator(g) => Rc::strong_count(g) > 1,
            Value::Resource(_) => false,
            v => is_plain_scalar(v),
        }
    }
    match v {
        Value::Array(a) => {
            Rc::strong_count(a) > 1
                || a.try_borrow()
                    .is_ok_and(|a| a.len() <= 64 && a.values().all(shallow))
        }
        Value::Object(o) => Rc::strong_count(o) > 1 || object_release_quiet(o),
        Value::Reference(r) => Rc::strong_count(r) > 1 || r.try_borrow().is_ok_and(|b| shallow(&b)),
        v => shallow(v),
    }
}

/// Release the values of `slots` in order, as a frame's end does.
#[inline]
fn release_slots(slots: &mut Vec<Value>) {
    for v in slots.iter_mut() {
        if !matches!(
            v,
            Value::Undef
                | Value::Null
                | Value::False
                | Value::True
                | Value::Long(_)
                | Value::Double(_)
        ) {
            *v = Value::Undef;
        }
    }
    slots.clear();
}

impl Vm {
    /// What a lean call needs of the running state, for the call made by
    /// the DO_FCALL `op` (its result stored in `tmps`).
    #[inline(always)]
    fn lean_ready(&self, op: &Op, tmps: &[Value]) -> bool {
        self.lean_resume.is_empty()
            && self.observer.is_none()
            && !self.has_release_work()
            && matches!(op.result, OperandType::Tmp(_) | OperandType::Unused)
            && tmp_writable(&op.result, tmps)
            && self.error_reporting != crate::errdisplay::ER_SILENT
            && !self.bind_next_frame
            && self.bind_next_tab.is_none()
            && self.current_exception.is_none()
            // (what execute_op_array_vm() needs to run a frame at once)
            && self.stack_configured
            && self.builtin_frames_synced()
            && crate::stack::has_room()
    }

    /// The user function the top pending call was set up for, when it may
    /// be a lean call's (what fast_fcall_target() accepts of a plain
    /// function name, or of an instance method INIT_METHOD_CALL's fast path
    /// resolved: see lean_function()), with no argument sent yet (`fresh`:
    /// but a method's `$this`) or with enough arguments; and for a method,
    /// where "::" is in the call's name. A forwarding call (`self::m()`,
    /// `static::m()`, `parent::m()`, resolved when it was set up: see
    /// push_forward_call) is one when fast_fcall_target() takes it: "::" is
    /// then where it is in the name it resolved to, which the call takes.
    #[inline(always)]
    fn lean_target(&self, fresh: bool) -> Option<(&Rc<OpArray>, Option<(usize, bool)>)> {
        let call = self.pending_calls.last()?;
        let t = &call.target;
        let f = t.user.as_ref()?;
        if t.has_builtin
            || t.closure
            || t.fn_gen != self.user_functions.len() + 1
            || !call.named_args.is_empty()
        {
            return None;
        }
        let name = call.name.as_bytes();
        if name.is_empty() || name.len() > 128 {
            return None;
        }
        let sep =
            if let Some(fwd) = &t.forward {
                let (sep, is_static) = t.method?;
                let sep = sep as usize;
                let has_this = matches!(call.args.first(), Some(Value::Object(_)));
                if has_this != t.forward_this
                    || is_static == has_this
                    || fwd.as_bytes().get(sep + 2..).is_none_or(|m| {
                        m.starts_with(b"__") && !m.eq_ignore_ascii_case(b"__construct")
                    })
                {
                    return None;
                }
                Some((sep, has_this))
            } else {
                match t.method {
                    // (the engine's own names start with two underscores)
                    None => {
                        if name[0] == b'_' && name.get(1) == Some(&b'_') {
                            return None;
                        }
                        None
                    }
                    // (on an object: its `$this` leads the arguments; with the
                    // frame name and scope its call site resolved, see
                    // MethodCallInfo; a constructor too: the call would arm the
                    // destructor of its object, but a lean call is only made while
                    // no object has one, see lean_ready())
                    Some((sep, false)) => {
                        let sep = sep as usize;
                        if !matches!(call.args.first(), Some(Value::Object(_)))
                            || !t.method_info.as_ref().is_some_and(|i| {
                                i.name.ptr_eq(&call.name) || i.name.as_bytes() == name
                            })
                            || name.get(sep + 2..).is_none_or(|m| {
                                m.starts_with(b"__") && !m.eq_ignore_ascii_case(b"__construct")
                            })
                        {
                            return None;
                        }
                        Some((sep, true))
                    }
                    // A static method, named by its class (late static binding:
                    // that class).
                    Some((sep, true)) => {
                        let sep = sep as usize;
                        if !t
                            .method_info
                            .as_ref()
                            .is_some_and(|i| i.name.ptr_eq(&call.name) || i.name.as_bytes() == name)
                            || name.get(sep + 2..).is_none_or(|m| m.starts_with(b"__"))
                        {
                            return None;
                        }
                        Some((sep, false))
                    }
                }
            };
        if !lean_function(f, sep) {
            return None;
        }
        if !fresh {
            let nargs = call.args.len();
            let base = lead(sep);
            let required = f.required_param_count as usize;
            if nargs > f.param_count as usize
                || nargs < base + required
                || call.args[base..base + required]
                    .iter()
                    .any(|a| matches!(a, Value::Undef))
            {
                return None;
            }
        }
        Some((f, sep))
    }

    /// Whether the top pending call was set up for a user function or an
    /// instance method that may be a lean call's (lean_target() checks the
    /// rest), whose calls are tried: one whose calls kept being handed
    /// over is tried again after a while (see CvNameInfo::lean_score).
    #[inline]
    pub(crate) fn lean_candidate(&self) -> bool {
        let Some(c) = self.pending_calls.last() else {
            return false;
        };
        let t = &c.target;
        let Some(f) = t.user.as_ref() else {
            return false;
        };
        if t.closure {
            return false;
        }
        if f.lean_kind() == 0 {
            return false;
        }
        let info = f.cv_name_info();
        let score = info.lean_score.get();
        if score < 0 {
            info.lean_score.set(score + 1);
            return false;
        }
        true
    }

    /// The top pending call, taken for a lean call of its function.
    #[inline(always)]
    fn pop_lean_call(&mut self) -> PendingCall {
        let call = self.pending_calls.pop().expect("pending call");
        while matches!(self.closure_pending_calls.last(), Some(&i) if i > self.pending_calls.len())
        {
            self.closure_pending_calls.pop();
        }
        if self.closure_pending_calls.last() == Some(&self.pending_calls.len()) {
            self.closure_pending_calls.pop();
        }
        call
    }

    /// DO_FCALL at `ip` of `op_array`, met by run_fast_ops(): a call set up
    /// for a plain user function is made as a lean one (see the module's
    /// doc). True: the call was made and its result stored. False: nothing
    /// was done, or the call was handed over to DO_FCALL's general handler
    /// (a LeanResume): the run stops at the DO_FCALL either way.
    #[inline(never)]
    pub(crate) fn lean_call(
        &mut self,
        op_array: &OpArray,
        ip: usize,
        cvs: &[Value],
        tmps: &mut [Value],
    ) -> bool {
        let op = &op_array.ops[ip];
        let Some((f, sep)) = self.lean_target(false) else {
            return false;
        };
        // (typed parameters: see recv_fast.rs)
        if f.lean_kind() & 4 != 0 {
            let args = &self.pending_calls.last().expect("pending call").args;
            if !self.recv_has_nothing_to_do(f, args) {
                return false;
            }
        }
        let f = Rc::clone(f);
        if !self.lean_ready(op, tmps) {
            return false;
        }
        let mut call = self.pop_lean_call();
        let nargs = call.args.len();
        // The parameters move to the callee's variables; `$this` stays
        // (DO_FCALL's fast path, move_args()).
        let mut slots = self.take_values(f.cv_names.len());
        let keep = lead(sep);
        if keep == 1 {
            slots[0] = call.args[0].clone();
        }
        for (d, s) in slots[keep..].iter_mut().zip(call.args[keep..].iter_mut()) {
            std::mem::swap(d, s);
        }
        self.lean_run(op_array, ip, f, sep, slots, nargs, call, cvs, tmps)
    }

    /// The INIT_FCALL at `ip` of `op_array` just set up a call (the top
    /// pending call) of a plain user function: when the opcodes after it
    /// are the plain sends of its arguments (SEND_VAL of a defined
    /// variable, a literal or a temporary, to by-value parameters) and the
    /// DO_FCALL, the call is made as a lean one (see the module's doc), the
    /// sends and the DO_FCALL with it. Returns the index of the opcode
    /// after the DO_FCALL (the call was made), of the DO_FCALL (the call was
    /// handed over to it), or None (nothing was done).
    #[inline(never)]
    pub(crate) fn lean_fused(
        &mut self,
        op_array: &OpArray,
        ip: usize,
        cvs: &[Value],
        tmps: &mut [Value],
    ) -> Option<usize> {
        let ops = &op_array.ops;
        let literals = &op_array.literals;
        let (f, sep) = self.lean_target(true)?;
        // (a method's `$this` leads the arguments)
        let base = lead(sep);
        // The sends, then the DO_FCALL (a nested call's INIT_* ends them).
        let nsends = ops
            .get(ip + 1..)?
            .iter()
            .take(17)
            .position(|s| !matches!(s.opcode, OpCode::SendVal | OpCode::SendValChecked))?;
        let nargs = base + nsends;
        if nargs > f.param_count as usize || nargs < base + f.required_param_count as usize {
            return None;
        }
        let do_ip = ip + 1 + nsends;
        let fcall = &ops[do_ip];
        if fcall.opcode != OpCode::DoFCall {
            return None;
        }
        // The sends: what send_val_fast() sends.
        let sends = &ops[ip + 1..do_ip];
        // (typed parameters: the arguments must have their types already,
        // see recv_fast.rs)
        let typed = f.lean_kind() & 4 != 0;
        if typed && !self.pending_callable_deprecations.borrow().is_empty() {
            return None;
        }
        for (i, s) in sends.iter().enumerate() {
            if typed {
                let v = match s.op1 {
                    OperandType::Cv(c) => match cvs.get(c as usize) {
                        Some(Value::Reference(r)) => match r.try_borrow() {
                            Ok(b) => b.clone(),
                            Err(_) => return None,
                        },
                        Some(v) => v.clone(),
                        None => return None,
                    },
                    OperandType::Tmp(t) => tmps.get(t as usize)?.clone(),
                    OperandType::Const(c) => literals.get(c as usize)?.clone(),
                    _ => return None,
                };
                if !crate::recv_fast::arg_matches_exactly(f, base + i, &v) {
                    return None;
                }
            }
            let ok = !arg_by_ref(f, base + i)
                && match s.op1 {
                    OperandType::Cv(c) => match cvs.get(c as usize) {
                        None | Some(Value::Undef) => false,
                        // ($GLOBALS is sent as a copy: the general handler's)
                        Some(Value::Reference(r)) => r.try_borrow().is_ok_and(|b| match &*b {
                            Value::Undef => false,
                            Value::Array(a) => !Rc::ptr_eq(a, &self.globals),
                            _ => true,
                        }),
                        Some(Value::Array(a)) => !Rc::ptr_eq(a, &self.globals),
                        Some(_) => true,
                    },
                    OperandType::Tmp(t) => match tmps.get(t as usize) {
                        Some(Value::Reference(_)) => false,
                        Some(Value::Array(a)) => {
                            !Rc::ptr_eq(a, &self.globals)
                                && !matches!(s.result, OperandType::Const(_))
                        }
                        _ => true,
                    },
                    OperandType::Const(_) => true,
                    _ => false,
                };
            if !ok {
                return None;
            }
        }
        let f = Rc::clone(f);
        if !self.lean_ready(fcall, tmps) {
            return None;
        }
        let call = self.pop_lean_call();
        let mut slots = self.take_values(f.cv_names.len());
        if base == 1 {
            slots[0] = call.args[0].clone();
        }
        for (slot, s) in slots[base..].iter_mut().zip(sends) {
            *slot = match s.op1 {
                OperandType::Cv(c) => match &cvs[c as usize] {
                    Value::Reference(r) => r.borrow().clone(),
                    v => v.clone(),
                },
                OperandType::Tmp(t) => match std::mem::take(&mut tmps[t as usize]) {
                    Value::Undef => Value::Null,
                    v => v,
                },
                OperandType::Const(c) => literals[c as usize].clone(),
                _ => Value::Null,
            };
        }
        let done = self.lean_run(op_array, do_ip, f, sep, slots, nargs, call, cvs, tmps);
        Some(if done { do_ip + 1 } else { do_ip })
    }

    /// The lean call of `f` made by the DO_FCALL at `ip` of `op_array`,
    /// the first `nargs` of its variables `slots` set to the arguments.
    /// True: the call was made, its result stored in `tmps`. False: it was
    /// handed over.
    #[allow(clippy::too_many_arguments)]
    fn lean_run(
        &mut self,
        op_array: &OpArray,
        ip: usize,
        f: Rc<OpArray>,
        sep: Option<(usize, bool)>,
        mut slots: Vec<Value>,
        nargs: usize,
        mut call: PendingCall,
        caller_cvs: &[Value],
        tmps: &mut [Value],
    ) -> bool {
        let op = &op_array.ops[ip];
        // (what DO_FCALL's fast path does with the call)
        self.last_call_by_ref = false;
        // A forwarding call takes the name it resolved to, and keeps the
        // called class (fast_fcall_target()).
        let forward = match &call.target.forward {
            Some(fwd) => {
                call.name = fwd.clone();
                true
            }
            None => false,
        };
        // By-value array parameters get their own copy (nothing of the call
        // holds one any more: the frame's copy of the arguments is not made).
        let base = lead(sep);
        for (ci, slot) in slots.iter_mut().enumerate().take(nargs).skip(base) {
            if let Value::Array(a) = slot {
                if Rc::strong_count(a) > 1 && !f.by_ref_params.get(ci).copied().unwrap_or(false) {
                    let copy = crate::array::cow_copy(a);
                    *slot = Value::Array(Rc::new(RefCell::new(copy)));
                }
            }
        }
        // The frame's room on the VM stack (execute_op_array_frame()).
        let frame_size = (slots.len() + f.temp_count as usize) * 16 + crate::stack::CALL_FRAME_SIZE;
        let vm_frame = match crate::stack::vm_push(frame_size) {
            Ok(fr) => Some(crate::stack::VmFrameGuard(fr)),
            // (memory_limit: the general handler reports it)
            Err(_) => None,
        };
        let depth = self.call_depth + self.lean_depth;
        let calls_base = self.pending_calls.len();
        let mut fe = self.foreach_pool.pop().unwrap_or_else(new_foreach_state);
        let mut ctmps = self.take_values(f.temp_count as usize);
        let stop = if vm_frame.is_none() {
            0
        } else {
            match sep {
                // (a method runs in its class's scope, called on its object's
                // class: DO_FCALL's fast path)
                Some((sep, this)) => {
                    // (a forwarding call's called class: the caller's
                    // `$this`'s, as DO_FCALL's fast path finds it)
                    let caller_this = match caller_cvs.first() {
                        Some(Value::Object(o))
                            if forward
                                && op_array.cv_names.first().map(|n| n.as_slice())
                                    == Some(b"this") =>
                        {
                            Some(o)
                        }
                        _ => None,
                    };
                    self.push_fast_method_scopes(
                        &call,
                        sep,
                        this,
                        forward,
                        caller_this,
                        call.target.method_info.as_ref().map(|i| i.scope.as_slice()),
                    )
                }
                // (a function outside any class runs without a class scope)
                None => self.class_scope_stack.push(Vec::new()),
            }
            self.lean_depth += 1;
            // (for the builtin calls the callee makes, see builtincall.rs)
            self.lean_frames.push(LeanFrame {
                f: &*f,
                slots: &slots,
                tmps: &ctmps,
                fe: &*fe,
                call: &call,
                sep,
                line: op.line,
                nargs,
            });
            let ForeachState {
                positions,
                snap,
                obj,
                iter_handles,
                cur_key,
                weakmap_iter_keys,
                ref_cells,
                ..
            } = &mut *fe;
            let no_release = !self.has_release_work();
            let stop = self.run_fast_ops(
                &f,
                0,
                &mut slots,
                &mut ctmps,
                false,
                true,
                no_release,
                &mut FeView {
                    positions,
                    snap,
                    obj,
                    iter_handles,
                    cur_key,
                    weakmap: weakmap_iter_keys,
                    ref_cells,
                    // (a lean callee has entered no try block)
                    handler_depth: 0,
                },
                true,
                None,
            );
            self.lean_frames.pop();
            self.lean_depth -= 1;
            if sep.is_some() {
                for v in [self.called_class_stack.pop(), self.class_scope_stack.pop()]
                    .into_iter()
                    .flatten()
                {
                    self.recycle_bytes(v);
                }
            } else {
                self.class_scope_stack.pop();
            }
            stop
        };
        // A builtin call of the callee failed (see builtincall.rs): the call
        // ends with what it raised, the caller's DO_FCALL dispatches it.
        if self.fast_call_failed.is_some() {
            self.lean_failed(fe, slots, ctmps, calls_base, vm_frame, call);
            return false;
        }
        // (a result the DO_FCALL is the last user of is released right after
        // it, see OpArray::free_mask(): as an unused one)
        let unused = matches!(op.result, OperandType::Unused)
            || op_array.free_mask().get(ip).is_some_and(|m| m & 4 != 0);
        let result = match vm_frame {
            Some(_) => self.lean_return(&f, stop, &slots, &mut ctmps, unused),
            None => None,
        };
        let Some(result) = result else {
            // Handed over to DO_FCALL's general handler.
            drop(vm_frame);
            let info = f.cv_name_info();
            info.lean_score
                .set(info.lean_score.get().saturating_sub(16).max(-256));
            // (the arguments, moved: as many as were sent)
            if call.args.len() < nargs {
                call.args.resize_with(nargs, || Value::Undef);
            }
            self.lean_resume.push(LeanResume {
                caller: op_array,
                caller_ip: ip,
                depth,
                user_fn: f,
                sep,
                call,
                cvs: slots,
                frame: ResumeFrame {
                    tmps: ctmps,
                    fe,
                    ip: stop,
                },
            });
            return false;
        };
        // The end of the frame (execute_op_array_inner(), then
        // execute_op_array_frame()'s common end). (Of the foreach state,
        // fast opcodes only change the positions.)
        if fe.positions.capacity() != 0 {
            fe.positions = Default::default();
        }
        if self.foreach_pool.len() < 16 {
            self.foreach_pool.push(fe);
        }
        crate::gc::release_frame(&slots);
        // (the temporaries, then the variables)
        release_slots(&mut ctmps);
        release_slots(&mut slots);
        self.recycle_values(ctmps);
        self.recycle_values(slots);
        if !crate::gc::release_quiet() {
            // (frees memory only: no destructor is listed, nothing was
            // released that runs one)
            self.run_pending_destructors();
        }
        drop(vm_frame);
        // (DO_FCALL: the arguments' buffer is kept, the result stored)
        self.recycle_args(&mut call);
        match &result {
            Value::Object(o) => crate::gc::track_object(o),
            Value::Generator(g) => crate::gc::track_generator(g),
            _ => {}
        }
        store_tmp(&op.result, result, tmps);
        let info = f.cv_name_info();
        info.lean_score.set((info.lean_score.get() + 1).min(64));
        true
    }

    /// The end of a lean call whose callee's builtin call failed (see
    /// builtincall.rs): what execute_op_array_inner() does for a frame an
    /// exception leaves (the calls it interrupted are dropped, its
    /// temporaries and variables released, their destructors run).
    #[cold]
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn lean_failed(
        &mut self,
        mut fe: Box<ForeachState>,
        mut slots: Vec<Value>,
        mut ctmps: Vec<Value>,
        calls_base: usize,
        vm_frame: Option<crate::stack::VmFrameGuard>,
        mut call: PendingCall,
    ) {
        if self.pending_calls.len() > calls_base {
            self.pending_calls.truncate(calls_base);
        }
        fe.clear();
        if self.foreach_pool.len() < 16 {
            self.foreach_pool.push(fe);
        }
        crate::gc::release_frame(&slots);
        release_slots(&mut ctmps);
        release_slots(&mut slots);
        self.recycle_values(ctmps);
        self.recycle_values(slots);
        if !crate::gc::release_quiet() {
            self.run_pending_destructors();
        }
        drop(vm_frame);
        self.recycle_args(&mut call);
    }

    /// The RETURN at `ip` of a lean call's callee (stopped there), when it
    /// is one DO_FCALL's fast path makes (a defined operand, see the fast
    /// RETURN arm of the dispatch loop) and the release of the frame only
    /// frees memory: the returned value. None: the call is handed over.
    /// `unused`: the result of the call is not used (it is released too).
    fn lean_return(
        &mut self,
        f: &OpArray,
        ip: usize,
        cvs: &[Value],
        tmps: &mut [Value],
        unused: bool,
    ) -> Option<Value> {
        let r = f.ops.get(ip)?;
        if r.opcode != OpCode::Return || self.current_exception.is_some() || self.has_release_work()
        {
            return None;
        }
        let v = match r.op1 {
            OperandType::Cv(i) => match cvs.get(i as usize)? {
                Value::Undef => return None,
                Value::Reference(rc) => match &*rc.try_borrow().ok()? {
                    Value::Undef => return None,
                    v => v.clone(),
                },
                v => v.clone(),
            },
            OperandType::Tmp(i) => match tmps.get(i as usize)? {
                Value::Undef => Value::Null,
                v => v.clone(),
            },
            OperandType::Const(i) => f.literals.get(i as usize)?.clone(),
            _ => Value::Null,
        };
        // (the returned temporary is moved out, not released)
        let ret_tmp = match r.op1 {
            OperandType::Tmp(i) => i as usize,
            _ => usize::MAX,
        };
        // (a declared return type: the value must have it already, see
        // recv_fast.rs)
        if f.lean_kind() & 8 != 0 && !crate::recv_fast::return_matches_exactly(f, &v, r.line == 0) {
            return None;
        }
        if (unused && !is_plain_scalar(&v))
            || !cvs.iter().all(drops_quietly)
            || !tmps
                .iter()
                .enumerate()
                .all(|(i, t)| i == ret_tmp || drops_quietly(t))
        {
            return None;
        }
        if let Some(slot) = tmps.get_mut(ret_tmp) {
            *slot = Value::Undef;
        }
        self.return_discarded_exc = None;
        self.last_return_line = r.line;
        self.current_line = r.line;
        Some(v)
    }

    /// Whether the DO_FCALL at `ip` of `op_array` handed over a lean call
    /// (see lean_call()).
    #[inline]
    pub(crate) fn lean_resume_here(&self, op_array: &OpArray, ip: usize) -> bool {
        self.lean_resume.last().is_some_and(|r| {
            std::ptr::eq(r.caller, op_array)
                && r.caller_ip == ip
                && r.depth == self.call_depth + self.lean_depth
        })
    }

    /// DO_FCALL at `ip` of `op_array`: when it handed over a lean call,
    /// what DO_FCALL's fast path takes it from, as fast_fcall_target()
    /// leaves them for a call (the pending call, its function) and the
    /// callee's variables and frame start (Vm::lean_resumed). False:
    /// nothing was handed over there.
    #[inline(never)]
    pub(crate) fn resume_lean_call(&mut self, op_array: &OpArray, ip: usize) -> bool {
        if !self.lean_resume_here(op_array, ip) {
            return false;
        }
        let Some(r) = self.lean_resume.pop() else {
            return false;
        };
        // (a forwarding call, renamed already: see lean_run())
        self.fast_fcall_forward = r.call.target.forward.is_some();
        self.pending_calls.push(r.call);
        self.fast_fcall_fn = Some(r.user_fn);
        self.fast_fcall_sep = r.sep;
        self.fast_fcall_captures = None;
        self.lean_resumed = Some((r.cvs, r.frame));
        true
    }
}
