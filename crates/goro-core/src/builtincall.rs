//! DO_FCALL of a builtin made by run_fast_ops().
//!
//! A call of a builtin by its plain name (the INIT_FCALL resolved it, see
//! Vm::fast_builtin_target) is what the dispatch loop's builtin fast path
//! runs: the generic zpp checks, the builtin's frame, the call, its result.
//! run_fast_ops() makes such calls itself instead of handing control back
//! to the dispatch loop for each of them, when the frame needs nothing the
//! loop does between opcodes (no destructor due, nothing to bind, no
//! observer). What the call raises (a zpp error, an exception of the
//! builtin, of a destructor it led to) cannot be dispatched from there: the
//! run stops at the DO_FCALL, recording the failure, and the dispatch
//! loop's DO_FCALL arm takes it from there as the builtin fast path would
//! have (Vm::fast_call_failed), without calling again.
//!
//! The callee of a lean call (see leancall.rs) makes such calls too: it has
//! no frame of its own, so for the call it gets the one DO_FCALL's fast
//! path would have pushed for it (its backtrace frame, its parameters
//! registered for backtraces, its file, its variables as cycle collector
//! roots, see LeanFrame), and loses it right after. A failure there ends
//! the lean call itself: the caller's DO_FCALL dispatches it.

use super::{BuiltinFn, ForeachState, OpArray, PendingCall, Vm, VmError, push_lossy};
use crate::fastpath::{store_tmp, tmp_writable};
use crate::opcode::OperandType;
use crate::value::Value;

/// The builtin a call site resolved to (see CallTarget::builtin), with
/// what its calls would otherwise work out from its name each time: its
/// lowercase name (the name of its frame) and whether php-src compiles its
/// calls into an opcode of their own (see
/// internal_frames::compiled_special). One per builtin (made once, kept
/// for good: see BuiltinDesc::get).
#[derive(Debug)]
pub(crate) struct BuiltinDesc {
    pub(crate) func: BuiltinFn,
    /// Its generic zpp info (see Vm::zpp_info).
    pub(crate) zpp: Option<(&'static crate::arginfo::FuncInfo, u8)>,
    /// Its lowercase name.
    pub(crate) lower: &'static [u8],
    /// compiled_special() of a call with N arguments: bit N (N < 3);
    /// SPECIAL_ALWAYS: every call (the in_array() opcode's builtin).
    special: u8,
}

const SPECIAL_ALWAYS: u8 = 0x80;

impl BuiltinDesc {
    /// The BuiltinDesc of builtin `func` by the lowercase name `lower`,
    /// with generic zpp info `zpp`: made the first time (there are as many
    /// as builtins), the same one after that.
    #[inline(never)]
    pub(crate) fn get(
        lower: &[u8],
        func: BuiltinFn,
        zpp: Option<(&'static crate::arginfo::FuncInfo, u8)>,
    ) -> &'static BuiltinDesc {
        type Descs = crate::fxhash::FxHashMap<Vec<u8>, Vec<&'static BuiltinDesc>>;
        thread_local! {
            static DESCS: std::cell::RefCell<Descs> = Default::default();
        }
        let same_zpp = |a: Option<(&'static crate::arginfo::FuncInfo, u8)>| match (a, zpp) {
            (None, None) => true,
            (Some((a, af)), Some((b, bf))) => std::ptr::eq(a, b) && af == bf,
            _ => false,
        };
        DESCS.with(|d| {
            let mut d = d.borrow_mut();
            let found = d.get(lower).and_then(|list| {
                list.iter()
                    .find(|x| x.func as usize == func as usize && same_zpp(x.zpp))
            });
            if let Some(&found) = found {
                return found;
            }
            let list = d.entry(lower.to_vec()).or_default();
            // (sprintf() with a constant format is compiled to a rope)
            let special = if lower == crate::compiler::IN_ARRAY_OP_FN {
                SPECIAL_ALWAYS
            } else {
                let none = [Value::Null, Value::Null];
                (0..3).fold(0u8, |bits, n| {
                    let special = crate::internal_frames::compiled_special(lower, &none[..n]);
                    bits | (u8::from(special) << n)
                })
            };
            let desc: &'static BuiltinDesc = Box::leak(Box::new(BuiltinDesc {
                func,
                zpp,
                lower: Box::leak(lower.to_vec().into_boxed_slice()),
                special,
            }));
            list.push(desc);
            desc
        })
    }

    /// internal_frames::compiled_special() of a call of this builtin with
    /// `args`.
    #[inline]
    pub(crate) fn compiled_special(&self, args: &[Value]) -> bool {
        if self.special & SPECIAL_ALWAYS != 0 {
            return true;
        }
        args.len() < 3 && (self.special >> args.len()) & 1 != 0
    }

    /// Whether a call of this builtin by `name` (the name it was resolved
    /// for, or a longer unqualified namespaced one that fell back to it:
    /// INIT_NS_FCALL_BY_NAME) is such a fallback. Its frame is named after
    /// the builtin all the same (`lower`), but php-src compiles none of
    /// those calls into an opcode of their own (zend_compile_ns_call()).
    #[inline]
    pub(crate) fn ns_fallback(&'static self, name: &[u8]) -> bool {
        name.len() != self.lower.len()
    }
}

/// A running lean call's callee (see leancall.rs: Vm::lean_run's state, alive
/// while its run of fast opcodes runs), for the builtin calls it makes.
/// What enter_lean_frames() changed, for leave_lean_frames().
pub(crate) struct LeanFramesSaved {
    frames: Vec<LeanFrame>,
    n: u32,
    /// The lengths of call_stack, cv_frames and gc_frames with the lean
    /// callees' frames.
    top: (usize, usize, usize),
    outer_exec: usize,
    saved_file: Option<crate::rcstr::RcStr>,
    was_global: bool,
    lean_depth: u32,
}

/// Where a running lean callee's frames (see enter_lean_frames) are: the
/// lengths of call_stack, cv_frames and gc_frames before them.
#[derive(Clone, Copy)]
pub(crate) struct LeanFrameAt {
    call: u32,
    cv: u32,
    gc: u32,
}

#[derive(Clone, Copy)]
pub(crate) struct LeanFrame {
    pub(crate) f: *const OpArray,
    pub(crate) slots: *const Vec<Value>,
    pub(crate) tmps: *const Vec<Value>,
    pub(crate) fe: *const ForeachState,
    pub(crate) call: *const PendingCall,
    /// (of a method: where "::" is in the call's name, and whether `$this`
    /// leads the arguments)
    pub(crate) sep: Option<(usize, bool)>,
    /// The line of the DO_FCALL that made the call.
    pub(crate) line: u32,
    /// The number of arguments the call passed (`$this` included).
    pub(crate) nargs: usize,
    /// Its frames, once a call it made gave it them (see
    /// enter_lean_frames: they stay until it ends).
    pub(crate) at: Option<LeanFrameAt>,
    /// Its backtrace frame is the top of call_stack already (a callback's,
    /// pushed by the builtin's call of it: see Vm::callback_frame).
    pub(crate) has_call_frame: bool,
}

impl Vm {
    /// The DO_FCALL at `ip` of `op_array` (run by run_fast_ops(), the top
    /// pending call set up for a builtin: see fast_builtin_target), made
    /// as the dispatch loop's builtin fast path makes it. Some(true): done,
    /// its result stored. Some(false): the call failed (see
    /// fast_call_failed): the run stops there. None: nothing was done.
    /// `internal_code`: see run_fast_ops(); `free_mask`: the frame's (see
    /// OpArray::free_mask()).
    #[inline(never)]
    pub(crate) fn fast_builtin_dofcall(
        &mut self,
        op_array: &OpArray,
        ip: usize,
        tmps: &mut [Value],
        internal_code: bool,
        free_mask: &[u8],
    ) -> Option<bool> {
        if !self.builtin_dofcall_ok(op_array, ip, tmps, free_mask) {
            return None;
        }
        let top = self.pending_calls.len().checked_sub(1)?;
        let desc = self.pending_calls[top].target.builtin?;
        let op = &op_array.ops[ip];
        let mask = free_mask.get(ip).copied().unwrap_or(0);
        self.last_call_by_ref = false;
        // The call: its arguments, its name's lowercase form; the rest of it
        // goes at once (in place: nothing of it is released with any effect).
        let call = &mut self.pending_calls[top];
        // (a dynamic call is a real one: it has a frame)
        let dynamic = call.target.dynamic;
        let mut args = std::mem::take(&mut call.args);
        let ns_fallback = desc.ns_fallback(call.name.as_bytes());
        let func_name_lower = desc.lower;
        if call.named_args.capacity() == 0
            && call.target.user.is_none()
            && call.target.method_info.is_none()
            && call.target.forward.is_none()
        {
            // (of what is left, only the name has anything to drop)
            // SAFETY: the call is no longer in the vector, and read once.
            unsafe {
                self.pending_calls.set_len(top);
                drop(std::ptr::read(
                    &(*self.pending_calls.as_ptr().add(top)).name,
                ));
            }
        } else {
            self.pending_calls.truncate(top);
        }
        while matches!(self.closure_pending_calls.last(), Some(&i) if i > self.pending_calls.len())
        {
            self.closure_pending_calls.pop();
        }
        let via_closure = self.closure_pending_calls.last() == Some(&self.pending_calls.len());
        if via_closure {
            self.closure_pending_calls.pop();
        }
        let (func, zpp) = (desc.func, desc.zpp);
        if !internal_code {
            self.current_line = op.line;
        }
        self.note_call_result(op, func_name_lower);
        // Compiled to an opcode by php-src: no frame of its own
        let special = !via_closure && !dynamic && !ns_fallback && desc.compiled_special(&args);
        // Generic zpp (verified stub arginfo)
        let mut zpp_orig: Option<Vec<Value>> = None;
        if let Some((info, flags)) = zpp {
            if !special {
                self.zpp_frame_at = Some(op.line);
            }
            if let Err(e) = self.zpp_check(
                info,
                flags,
                &mut args,
                &mut zpp_orig,
                op_array.strict_types,
                op.line,
            ) {
                if let crate::arginfo::ZppError::Throw(cls, msg) = e {
                    // zpp errors are raised inside the function: its frame
                    // is in the trace
                    if !special {
                        self.push_builtin_frame(
                            func_name_lower,
                            zpp_orig.as_deref().unwrap_or(&args),
                            op.line,
                        );
                    }
                    let exc_val = self.create_exception(cls, &msg, op.line);
                    if !special {
                        self.pop_builtin_frame();
                    }
                    self.current_exception = Some(exc_val);
                }
                let message = self.uncaught_exception_message();
                return Some(self.fast_call_fail(op_array, ip, message, args));
            }
        }
        self.caller_strict_types = op_array.strict_types;
        self.reset_named_skipped();
        let own_frame = !special;
        if own_frame {
            self.push_builtin_frame(
                func_name_lower,
                zpp_orig.as_deref().unwrap_or(&args),
                op.line,
            );
        }
        let builtin_result = if own_frame {
            self.call_builtin_observed(func, func_name_lower, &args)
        } else {
            func(self, &args)
        };
        if own_frame {
            self.release_in_builtin_frame(&args);
            self.pop_builtin_frame();
        } else {
            self.ucf_call_line = None;
            self.ucf_strict = false;
        }
        drop(zpp_orig);
        let result = match builtin_result {
            Ok(result) => {
                if self.current_exception.is_some() {
                    let message = self.uncaught_exception_message();
                    return Some(self.fast_call_fail(op_array, ip, message, args));
                }
                result
            }
            Err(e) => {
                // The arguments are released before the exception unwinds
                // (a destructor's exception chains to it).
                if self.current_exception.is_some() && self.has_release_work() && !args.is_empty() {
                    args.clear();
                    self.current_line = op.line;
                    self.run_pending_destructors();
                }
                return Some(self.fast_call_fail(op_array, ip, e.message, args));
            }
        };
        // zend_vm_stack_free_args(): the arguments are released when the
        // call returns.
        if self.has_release_work() && !args.is_empty() {
            args.clear();
            self.current_line = op.line;
            self.run_pending_destructors();
            if self.current_exception.is_some() {
                drop(result);
                let message = "Uncaught exception from destructor".to_string();
                return Some(self.fast_call_fail(op_array, ip, message, args));
            }
        }
        match &result {
            Value::Object(o) => crate::gc::track_object(o),
            Value::Generator(g) => crate::gc::track_generator(g),
            _ => {}
        }
        if mask & 4 != 0 {
            // (the result's last user is the call: released at once)
            let heap = matches!(result, Value::Object(_) | Value::Array(_));
            drop(result);
            if heap && self.has_release_work() {
                self.current_line = op.line;
                self.run_pending_destructors();
                if self.current_exception.is_some() {
                    let message = "Uncaught exception from destructor".to_string();
                    return Some(self.fast_call_fail(op_array, ip, message, args));
                }
            }
        } else {
            store_tmp(&op.result, result, tmps);
        }
        self.recycle_arg_vec(args);
        Some(true)
    }

    /// No named argument was skipped for the builtin about to be called
    /// (`builtin_named_skipped = Vec::new()`, without dropping a vector
    /// that holds no allocation).
    #[inline(always)]
    pub(crate) fn reset_named_skipped(&mut self) {
        if self.builtin_named_skipped.capacity() != 0 {
            self.builtin_named_skipped = Vec::new();
        }
    }

    /// What fast_builtin_dofcall() needs of the DO_FCALL at `ip` (before it
    /// takes the call).
    #[inline]
    fn builtin_dofcall_ok(
        &self,
        op_array: &OpArray,
        ip: usize,
        tmps: &[Value],
        free_mask: &[u8],
    ) -> bool {
        let op = &op_array.ops[ip];
        let mask = free_mask.get(ip).copied().unwrap_or(0);
        mask & !4 == 0
            && !self.has_release_work()
            && matches!(op.result, OperandType::Tmp(_) | OperandType::Unused)
            && tmp_writable(&op.result, tmps)
            && self.internal_function_decls.is_empty()
    }

    /// fast_builtin_dofcall() for the callee of a lean call (the running
    /// LeanFrame's function, `op_array`): with, for the call, the frame
    /// DO_FCALL's fast path would have set up for the callee.
    #[inline(never)]
    pub(crate) fn lean_builtin_dofcall(
        &mut self,
        op_array: &OpArray,
        ip: usize,
        tmps: &mut [Value],
        free_mask: &[u8],
    ) -> Option<bool> {
        if !self.builtin_dofcall_ok(op_array, ip, tmps, free_mask) {
            return None;
        }
        let saved = self.enter_lean_frames(op_array)?;
        let r = self.fast_builtin_dofcall(op_array, ip, tmps, false, free_mask);
        self.leave_lean_frames(saved);
        r
    }

    /// For a call the callee of the running lean calls makes (`op_array`,
    /// the innermost's function): each running lean callee, outermost
    /// first, gets the frame DO_FCALL's fast path would have given it (its
    /// backtrace frame, its parameters registered for backtraces, its
    /// variables as cycle collector roots), and the call runs as from
    /// frames of their own, as many levels down. A callee keeps its frame
    /// until it ends (see leave_lean_frame): the next calls made below it
    /// find it. None: a callee's frame cannot be named so (nothing was
    /// done). See leave_lean_frames().
    pub(crate) fn enter_lean_frames(&mut self, op_array: &OpArray) -> Option<LeanFramesSaved> {
        let n = self.lean_frames.len();
        if n == 0
            || n != self.lean_depth as usize
            || !std::ptr::eq(self.lean_frames[n - 1].f, op_array)
        {
            return None;
        }
        let framed = (self.lean_framed as usize).min(n);
        let no_sym = None;
        let no_fin = Vec::new();
        for k in framed..n {
            let lf = self.lean_frames[k];
            self.lean_frames[k].at = Some(LeanFrameAt {
                call: self.call_stack.len() as u32,
                cv: self.cv_frames.len() as u32,
                gc: self.gc_frames.len() as u32,
            });
            // SAFETY: see LeanFrame.
            let (f, call, slots) = unsafe { (&*lf.f, &*lf.call, &*lf.slots) };
            let this = lf.sep.is_some_and(|(_, this)| this);
            let frame_this = this && matches!(slots.first(), Some(Value::Object(_)));
            if !lf.has_call_frame {
                self.push_lean_backtrace_frame(k, &lf, f, call, slots, frame_this);
            }
            if self.register_cv_frame_hinted(f, true) {
                self.set_cv_frame_vars(slots);
            }
            self.gc_frames.push(crate::gc::GcFrame::new(
                slots,
                unsafe { &*lf.tmps },
                &no_sym,
                unsafe { &*lf.fe },
                &no_fin,
                &call.args,
                frame_this,
            ));
        }
        self.lean_framed = n as u32;
        let top = (
            self.call_stack.len(),
            self.cv_frames.len(),
            self.gc_frames.len(),
        );
        let outer_exec = std::mem::replace(&mut self.exec_frame_len, self.call_stack.len());
        let saved_file = self.enter_op_array_file(op_array);
        let was_global = std::mem::replace(&mut self.is_global_scope, false);
        let lean_depth = std::mem::replace(&mut self.lean_depth, 0);
        self.call_depth += n as u32;
        // (the lean calls the call makes start a list of their own)
        let spare = self.lean_frames_pool.pop().unwrap_or_default();
        let frames = std::mem::replace(&mut self.lean_frames, spare);
        self.lean_framed = 0;
        Some(LeanFramesSaved {
            frames,
            n: n as u32,
            top,
            outer_exec,
            saved_file,
            was_global,
            lean_depth,
        })
    }

    /// enter_lean_frames(): the backtrace frame of the running lean callee
    /// `lf` (the `k`th), of `f`, made by `call`.
    fn push_lean_backtrace_frame(
        &mut self,
        k: usize,
        lf: &LeanFrame,
        f: &OpArray,
        call: &PendingCall,
        slots: &[Value],
        frame_this: bool,
    ) {
        {
            let mut display = self.string_pool.pop().unwrap_or_default();
            display.clear();
            // (a method's frame is named after what its call site resolved;
            // a static or forwarding call's as DO_FCALL's fast path names
            // it: its class, as declared, and the method as called)
            match (lf.sep, &call.target.method_info, &f.scope_class) {
                (Some(_), Some(i), _) => display.push_str(&i.display),
                _ if !f.display_name.is_empty() => push_lossy(&mut display, &f.display_name),
                (Some((sep, _)), None, Some(scope)) => {
                    match self.classes.get(scope) {
                        Some(c) => push_lossy(&mut display, &c.name),
                        None => push_lossy(&mut display, scope),
                    }
                    display.push_str("::");
                    push_lossy(&mut display, &call.name.as_bytes()[sep + 2..]);
                }
                _ => push_lossy(&mut display, call.name.as_bytes()),
            }
            let base = usize::from(lf.sep.is_some_and(|(_, this)| this));
            let mut args = self.value_pool.pop().unwrap_or_default();
            args.clear();
            super::leancall::moved_frame_args(f, slots, &call.args, base, lf.nargs, &mut args);
            // (the file the call was made in: the caller's)
            let file = match k {
                0 => self.current_file.clone(),
                _ => unsafe { &*self.lean_frames[k - 1].f }.file_name(),
            };
            self.call_stack
                .push((display, file, lf.line, args, frame_this));
        }
    }

    /// The end of a call enter_lean_frames() set up for (the running lean
    /// callees keep their frames; what the call left above them goes).
    pub(crate) fn leave_lean_frames(&mut self, s: LeanFramesSaved) {
        let mut inner = std::mem::replace(&mut self.lean_frames, s.frames);
        inner.clear();
        if self.lean_frames_pool.len() < 8 {
            self.lean_frames_pool.push(inner);
        }
        self.lean_framed = s.n;
        self.call_depth -= s.n;
        self.lean_depth = s.lean_depth;
        self.is_global_scope = s.was_global;
        self.leave_op_array_file(s.saved_file);
        self.exec_frame_len = s.outer_exec;
        self.truncate_frames(s.top.0, s.top.1, s.top.2);
    }

    /// The innermost running lean callee ends: it loses the frames a call
    /// it made gave it (see enter_lean_frames).
    pub(crate) fn leave_lean_frame(&mut self) {
        let k = self.lean_frames.len() - 1;
        let Some(at) = self.lean_frames[k].at.take() else {
            return;
        };
        self.lean_framed = self.lean_framed.min(k as u32);
        self.truncate_frames(at.call as usize, at.cv as usize, at.gc as usize);
    }

    fn truncate_frames(&mut self, call: usize, cv: usize, gc: usize) {
        self.gc_frames.truncate(gc);
        self.cv_frames.truncate(cv);
        while self.call_stack.len() > call {
            if let Some(frame) = self.call_stack.pop() {
                self.recycle_frame(frame);
            }
        }
    }

    /// The builtin call made by the DO_FCALL at `ip` of `op_array` failed
    /// (see fast_builtin_dofcall): what the dispatch loop's DO_FCALL takes.
    #[cold]
    #[inline(never)]
    fn fast_call_fail(
        &mut self,
        op_array: &OpArray,
        ip: usize,
        message: String,
        args: Vec<Value>,
    ) -> bool {
        let line = op_array.ops[ip].line;
        self.fast_call_failed = Some(VmError { message, line });
        self.recycle_arg_vec(args);
        false
    }

    /// recycle_args() of the arguments taken out of a call.
    #[inline]
    fn recycle_arg_vec(&mut self, mut args: Vec<Value>) {
        super::clear_values(&mut args);
        self.recycle_values(args);
    }
}
