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

use super::{OpArray, Vm, VmError};
use crate::fastpath::{store_tmp, tmp_writable};
use crate::opcode::OperandType;
use crate::value::Value;

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
        let op = &op_array.ops[ip];
        let mask = free_mask.get(ip).copied().unwrap_or(0);
        if mask & !4 != 0
            || !matches!(op.result, OperandType::Tmp(_) | OperandType::Unused)
            || !tmp_writable(&op.result, tmps)
            || !self.internal_function_decls.is_empty()
        {
            return None;
        }
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
