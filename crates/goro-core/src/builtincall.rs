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

use super::{ForeachState, OpArray, PendingCall, Vm, VmError, push_lossy};
use crate::fastpath::{store_tmp, tmp_writable};
use crate::opcode::OperandType;
use crate::value::Value;

/// A running lean call's callee (see leancall.rs: Vm::lean_run's state, alive
/// while its run of fast opcodes runs), for the builtin calls it makes.
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
        let op = &op_array.ops[ip];
        let mask = free_mask.get(ip).copied().unwrap_or(0);
        self.last_call_by_ref = false;
        let mut call = self.pending_calls.pop()?;
        while matches!(self.closure_pending_calls.last(), Some(&i) if i > self.pending_calls.len())
        {
            self.closure_pending_calls.pop();
        }
        let via_closure = self.closure_pending_calls.last() == Some(&self.pending_calls.len());
        if via_closure {
            self.closure_pending_calls.pop();
        }
        let Some((func, zpp)) = call.target.builtin else {
            self.pending_calls.push(call);
            return None;
        };
        if !internal_code {
            self.current_line = op.line;
        }
        let name = call.name.as_bytes();
        let mut buf = [0u8; 64];
        for (d, s) in buf.iter_mut().zip(name) {
            *d = s.to_ascii_lowercase();
        }
        let func_name_lower = &buf[..name.len().min(64)];
        self.note_call_result(op, func_name_lower);
        // Compiled to an opcode by php-src: no frame of its own
        let special =
            !via_closure && crate::internal_frames::compiled_special(func_name_lower, &call.args);
        // Generic zpp (verified stub arginfo)
        let mut zpp_orig: Option<Vec<Value>> = None;
        if let Some((info, flags)) = zpp {
            if !special {
                self.zpp_frame_at = Some(op.line);
            }
            if let Err(e) = self.zpp_check(
                info,
                flags,
                &mut call.args,
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
                            zpp_orig.as_deref().unwrap_or(&call.args),
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
                return Some(self.fast_call_fail(op_array, ip, message, call));
            }
        }
        self.caller_strict_types = op_array.strict_types;
        self.builtin_named_skipped = Vec::new();
        let own_frame = !special;
        if own_frame {
            self.push_builtin_frame(
                func_name_lower,
                zpp_orig.as_deref().unwrap_or(&call.args),
                op.line,
            );
        }
        let builtin_result = if own_frame {
            self.call_builtin_observed(func, func_name_lower, &call.args)
        } else {
            func(self, &call.args)
        };
        if own_frame {
            self.release_in_builtin_frame(&call.args);
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
                    return Some(self.fast_call_fail(op_array, ip, message, call));
                }
                result
            }
            Err(e) => {
                // The arguments are released before the exception unwinds
                // (a destructor's exception chains to it).
                if self.current_exception.is_some()
                    && self.has_release_work()
                    && !call.args.is_empty()
                {
                    call.args.clear();
                    self.current_line = op.line;
                    self.run_pending_destructors();
                }
                return Some(self.fast_call_fail(op_array, ip, e.message, call));
            }
        };
        // zend_vm_stack_free_args(): the arguments are released when the
        // call returns.
        if self.has_release_work() && !call.args.is_empty() {
            call.args.clear();
            self.current_line = op.line;
            self.run_pending_destructors();
            if self.current_exception.is_some() {
                drop(result);
                let message = "Uncaught exception from destructor".to_string();
                return Some(self.fast_call_fail(op_array, ip, message, call));
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
                    return Some(self.fast_call_fail(op_array, ip, message, call));
                }
            }
        } else {
            store_tmp(&op.result, result, tmps);
        }
        self.recycle_args(&mut call);
        Some(true)
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
        let n = self.lean_frames.len();
        if n == 0
            || n != self.lean_depth as usize
            || !std::ptr::eq(self.lean_frames[n - 1].f, op_array)
            || !self.builtin_dofcall_ok(op_array, ip, tmps, free_mask)
        {
            return None;
        }
        // (a method's frame is named after what its call site resolved)
        // SAFETY: see LeanFrame (lean_run() keeps them while the run runs).
        if self
            .lean_frames
            .iter()
            .any(|lf| lf.sep.is_some() && unsafe { &*lf.call }.target.method_info.is_none())
        {
            return None;
        }
        let depth = self.call_stack.len();
        let cv_depth = self.cv_frames.len();
        let gc_depth = self.gc_frames.len();
        let no_sym = None;
        let no_fin = Vec::new();
        // Each running lean callee, outermost first, gets the frame DO_FCALL's
        // fast path would have given it.
        for k in 0..n {
            let lf = self.lean_frames[k];
            // SAFETY: see LeanFrame.
            let (f, call, slots) = unsafe { (&*lf.f, &*lf.call, &*lf.slots) };
            let mut display = self.string_pool.pop().unwrap_or_default();
            display.clear();
            match (lf.sep, &call.target.method_info) {
                (Some(_), Some(i)) => display.push_str(&i.display),
                _ if !f.display_name.is_empty() => push_lossy(&mut display, &f.display_name),
                _ => push_lossy(&mut display, call.name.as_bytes()),
            }
            let this = lf.sep.is_some_and(|(_, this)| this);
            let base = usize::from(this);
            let frame_this = this && matches!(slots.first(), Some(Value::Object(_)));
            let mut args = self.value_pool.pop().unwrap_or_default();
            args.clear();
            if let Some(a) = slots.get(base..lf.nargs.min(slots.len())) {
                args.extend_from_slice(a);
            }
            // (the file the call was made in: the caller's)
            let file = match k {
                0 => self.current_file.clone(),
                _ => unsafe { &*self.lean_frames[k - 1].f }.file_name(),
            };
            self.call_stack
                .push((display, file, lf.line, args, frame_this));
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
        let outer_exec = std::mem::replace(&mut self.exec_frame_len, self.call_stack.len());
        let saved_file = self.enter_op_array_file(op_array);
        let was_global = std::mem::replace(&mut self.is_global_scope, false);
        // (the call runs as from frames of their own, as many levels down)
        let lean_depth = std::mem::replace(&mut self.lean_depth, 0);
        self.call_depth += n as u32;
        let r = self.fast_builtin_dofcall(op_array, ip, tmps, false, free_mask);
        self.gc_frames.truncate(gc_depth);
        self.call_depth -= n as u32;
        self.lean_depth = lean_depth;
        self.is_global_scope = was_global;
        self.leave_op_array_file(saved_file);
        self.exec_frame_len = outer_exec;
        self.cv_frames.truncate(cv_depth);
        while self.call_stack.len() > depth {
            if let Some(frame) = self.call_stack.pop() {
                self.recycle_frame(frame);
            }
        }
        r
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
        mut call: super::PendingCall,
    ) -> bool {
        let line = op_array.ops[ip].line;
        self.fast_call_failed = Some(VmError { message, line });
        self.recycle_args(&mut call);
        false
    }
}
