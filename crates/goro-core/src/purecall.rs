//! Builtin calls made without their frame.
//!
//! A call of a builtin has a frame of its own (its backtrace frame, see
//! Vm::push_builtin_frame) only for what can look at it while it runs: a
//! warning or exception it raises (an error handler, the trace), user code
//! it calls, a signal handled before it returns, the memory limit's fatal
//! error. Some builtins, given arguments of the exact types their
//! parameters take (no coercion: strict_types makes no difference), do
//! none of that: max() of two ints, substr() of a string and ints... For
//! such a call nothing can see whether the frame was there, and
//! INIT_FCALL / SEND_VAL... / DO_FCALL run as one step (see
//! Vm::fast_pure_call), without the pending call, the frame, the zpp
//! checks: the builtin, or what it computes, runs at once.

use super::{BuiltinFn, Vm};
use crate::fastpath::{is_plain_scalar, store_tmp, tmp_writable};
use crate::opcode::{OpArray, OpCode, OperandType};
use crate::string::PhpString;
use crate::value::Value;

/// What a builtin of PURE_CALLS does (see there).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Pure {
    /// max(int, int), max(float, float)
    Max,
    /// min(int, int), min(float, float)
    Min,
    /// abs(int|float)
    Abs,
    /// intdiv(int, int), not by 0, not PHP_INT_MIN by -1
    Intdiv,
    /// substr(string, int, int|null)
    Substr,
    /// strcmp(string, string)
    Strcmp,
    /// str_pad(string, int, non-empty string, STR_PAD_*)
    StrPad,
    /// strtolower(string), strtoupper(string)
    Case,
    /// str_contains / str_starts_with / str_ends_with(string, string)
    StrStr,
}

/// The builtins Vm::fast_pure_call() runs without a frame: (lowercase
/// name, what it does, least and most arguments).
pub(crate) const PURE_CALLS: &[(&[u8], Pure, u8, u8)] = &[
    (b"max", Pure::Max, 2, 2),
    (b"min", Pure::Min, 2, 2),
    (b"abs", Pure::Abs, 1, 1),
    (b"intdiv", Pure::Intdiv, 2, 2),
    (b"substr", Pure::Substr, 2, 3),
    (b"strcmp", Pure::Strcmp, 2, 2),
    (b"str_pad", Pure::StrPad, 2, 4),
    (b"strtolower", Pure::Case, 1, 1),
    (b"strtoupper", Pure::Case, 1, 1),
    (b"str_contains", Pure::StrStr, 2, 2),
    (b"str_starts_with", Pure::StrStr, 2, 2),
    (b"str_ends_with", Pure::StrStr, 2, 2),
];

/// Whether `need` more bytes stay well below memory_limit: what a call
/// allocating at most that much cannot make it go over (it would raise its
/// fatal error inside the call, with the call's frame).
#[inline]
fn memory_room(need: usize) -> bool {
    let limit = crate::value::memory_get_limit();
    limit == 0
        || crate::value::memory_get_usage()
            .saturating_add(need)
            .saturating_add(8192)
            <= limit
}

/// substr()'s result (see strings.rs's substr): the bytes of `s` it keeps.
#[inline]
fn substr_range(len: usize, start: i64, length: Option<i64>) -> (usize, usize) {
    let len = len as i64;
    let mut start = start;
    if start < 0 {
        start = (len + start).max(0);
    }
    let start = start.min(len);
    let end = match length {
        Some(l) if l < 0 => {
            let end_pos = len + l;
            if end_pos <= start { start } else { end_pos }
        }
        Some(l) => start.saturating_add(l).min(len),
        None => len,
    };
    (start as usize, end as usize)
}

impl Vm {
    /// INIT_FCALL / SEND_VAL... / DO_FCALL at `ip` of `op_array` of a call
    /// of builtin `func`, the `which`th of PURE_CALLS (the builtin the call
    /// site resolved to), whose arguments are of the exact types that make
    /// it raise nothing and run no user code: its result, computed at
    /// once, with what DO_FCALL does around the call that anything can
    /// see (see purecall.rs). Returns the index of the opcode after the
    /// DO_FCALL, or None (nothing done). Called only when no destructor is
    /// due (the call's release of its arguments is then a no-op).
    /// `bound`: the frame's variables are bound to a symbol table (only
    /// those bound, references, are read here).
    #[inline(never)]
    pub(crate) fn fast_pure_call(
        &mut self,
        op_array: &OpArray,
        ip: usize,
        cvs: &[Value],
        tmps: &mut [Value],
        func: BuiltinFn,
        which: u8,
        bound: bool,
    ) -> Option<usize> {
        let &(lower, kind, min, max) = PURE_CALLS.get(which as usize)?;
        let ops = &op_array.ops;
        let literals = &op_array.literals;
        let op = &ops[ip];
        // INIT_FCALL's op2 is the number of arguments; the sends follow it.
        let nargs = match op.op2 {
            OperandType::Const(c) => match literals.get(c as usize) {
                Some(Value::Long(n)) if (min as i64..=max as i64).contains(n) => *n as usize,
                _ => return None,
            },
            _ => return None,
        };
        // (an observer sees the call; a signal is handled inside it)
        if self.observer.is_some()
            || !self.internal_function_decls.is_empty()
            || crate::value::VM_INTERRUPT.load(std::sync::atomic::Ordering::Relaxed)
        {
            return None;
        }
        let sends = ops.get(ip + 1..ip + 1 + nargs)?;
        let fcall = ops.get(ip + 1 + nargs)?;
        if fcall.opcode != OpCode::DoFCall
            || !matches!(fcall.op2, OperandType::Unused)
            || !matches!(fcall.result, OperandType::Tmp(_) | OperandType::Unused)
            || !tmp_writable(&fcall.result, tmps)
        {
            return None;
        }
        // The arguments, as the sends would pass them: the values where they
        // are, not copies (only plain values, which the builtins only look
        // at).
        let mut args =
            std::mem::ManuallyDrop::new([Value::Null, Value::Null, Value::Null, Value::Null]);
        for (i, s) in sends.iter().enumerate() {
            let plain_send = match s.opcode {
                OpCode::SendVal => matches!(s.result, OperandType::Unused),
                OpCode::SendValChecked => true,
                _ => false,
            };
            if !plain_send {
                return None;
            }
            let v: &Value = match s.op1 {
                OperandType::Cv(c) => match cvs.get(c as usize)? {
                    Value::Reference(r) => {
                        let b = r.try_borrow().ok()?;
                        if !is_plain_scalar(&b) || matches!(*b, Value::Undef) {
                            return None;
                        }
                        // SAFETY: the referenced value stays where it is
                        // for the call (nothing here writes it).
                        unsafe { &*(&*b as *const Value) }
                    }
                    _ if bound => return None,
                    v => v,
                },
                OperandType::Const(c) => literals.get(c as usize)?,
                OperandType::Tmp(c) => tmps.get(c as usize)?,
                _ => return None,
            };
            if !is_plain_scalar(v) || matches!(v, Value::Undef) {
                return None;
            }
            // SAFETY: a bitwise copy of a plain value that stays where it
            // is for the call, never dropped (`args` is ManuallyDrop; the
            // Null replaced holds nothing to drop).
            args[i] = unsafe { std::ptr::read(v) };
        }
        let args = &args[..nargs];
        let result = self.pure_compute(op_array, func, kind, args)?;
        self.last_call_by_ref = false;
        self.note_call_result(fcall, lower);
        for s in sends {
            if let OperandType::Tmp(c) = s.op1 {
                if let Some(slot) = tmps.get_mut(c as usize) {
                    *slot = Value::Undef;
                }
            }
        }
        store_tmp(&fcall.result, result, tmps);
        Some(ip + 2 + nargs)
    }

    /// The DO_FCALL at `ip` of `op_array` (run by run_fast_ops(), the top
    /// pending call set up for a builtin of PURE_CALLS, see
    /// fast_builtin_target; what fast_builtin_dofcall() /
    /// lean_builtin_dofcall() checked of it is so) whose arguments are of
    /// the exact types that make the builtin raise nothing and run no user
    /// code: its result, computed at once, with what DO_FCALL does around
    /// the call that anything can see (see purecall.rs). Some(true): done,
    /// its result stored. None: nothing was done.
    #[inline(never)]
    pub(crate) fn pure_dofcall(
        &mut self,
        op_array: &OpArray,
        ip: usize,
        tmps: &mut [Value],
        internal_code: bool,
        free_mask: &[u8],
    ) -> Option<bool> {
        let top = self.pending_calls.len().checked_sub(1)?;
        let call = &self.pending_calls[top];
        let desc = call.target.builtin?;
        let &(lower, kind, min, max) = PURE_CALLS.get(desc.pure.checked_sub(1)? as usize)?;
        let t = &call.target;
        if t.dynamic
            || call.named_args.capacity() != 0
            || t.user.is_some()
            || t.method_info.is_some()
            || t.forward.is_some()
            || !(min as usize..=max as usize).contains(&call.args.len())
            || call
                .args
                .iter()
                .any(|v| !is_plain_scalar(v) || matches!(v, Value::Undef))
            || crate::value::VM_INTERRUPT.load(std::sync::atomic::Ordering::Relaxed)
            || self.closure_pending_calls.last() == Some(&top)
        {
            return None;
        }
        // (the arguments stay in the call until it is done with)
        let args = std::mem::take(&mut self.pending_calls[top].args);
        let Some(result) = self.pure_compute(op_array, desc.func, kind, &args) else {
            self.pending_calls[top].args = args;
            return None;
        };
        let op = &op_array.ops[ip];
        // (of what is left of the call, only the name has anything to drop)
        // SAFETY: the call is no longer in the vector, and read once.
        unsafe {
            self.pending_calls.set_len(top);
            drop(std::ptr::read(
                &(*self.pending_calls.as_ptr().add(top)).name,
            ));
        }
        while matches!(self.closure_pending_calls.last(), Some(&i) if i > self.pending_calls.len())
        {
            self.closure_pending_calls.pop();
        }
        self.last_call_by_ref = false;
        self.nonframeless_call = false;
        if !internal_code {
            self.current_line = op.line;
        }
        self.note_call_result(op, lower);
        if free_mask.get(ip).copied().unwrap_or(0) & 4 != 0 {
            // (the result's last user is the call: a plain value)
            drop(result);
        } else {
            store_tmp(&op.result, result, tmps);
        }
        self.recycle_arg_vec(args);
        Some(true)
    }

    /// What builtin `func` (doing `kind`, see PURE_CALLS) returns for
    /// `args`, a call of `op_array`'s, when they are what make it raise
    /// nothing (None: they are not, nothing done but what DO_FCALL sets up
    /// for a call).
    #[inline]
    fn pure_compute(
        &mut self,
        op_array: &OpArray,
        func: BuiltinFn,
        kind: Pure,
        args: &[Value],
    ) -> Option<Value> {
        // (what DO_FCALL sets up for the call that is seen after it)
        self.caller_strict_types = op_array.strict_types;
        self.reset_named_skipped();
        let result = match kind {
            Pure::Max | Pure::Min => match (&args[0], &args[1]) {
                // (math.rs's max() / min() of two arguments)
                (Value::Long(a), Value::Long(b)) => {
                    let lhs = if kind == Pure::Max { a >= b } else { a < b };
                    Value::Long(if lhs { *a } else { *b })
                }
                (Value::Double(a), Value::Double(b)) => {
                    let lhs = if kind == Pure::Max { a >= b } else { a < b };
                    Value::Double(if lhs { *a } else { *b })
                }
                _ => return None,
            },
            Pure::Abs => match args[0] {
                Value::Long(n) => match n.checked_abs() {
                    Some(a) => Value::Long(a),
                    None => Value::Double((n as f64).abs()),
                },
                Value::Double(d) => Value::Double(d.abs()),
                _ => return None,
            },
            Pure::Intdiv => match (&args[0], &args[1]) {
                (Value::Long(a), Value::Long(b)) if *b != 0 && !(*a == i64::MIN && *b == -1) => {
                    Value::Long(a / b)
                }
                _ => return None,
            },
            Pure::Substr => {
                let (Value::String(s), Value::Long(start)) = (&args[0], &args[1]) else {
                    return None;
                };
                let length = match args.get(2) {
                    None | Some(Value::Null) => None,
                    Some(Value::Long(l)) => Some(*l),
                    _ => return None,
                };
                let bytes = s.as_bytes();
                let (start, end) = substr_range(bytes.len(), *start, length);
                if start >= bytes.len() || start >= end {
                    Value::String(PhpString::empty())
                } else {
                    if !memory_room(end - start) {
                        return None;
                    }
                    Value::String(PhpString::from_vec(bytes[start..end].to_vec()))
                }
            }
            Pure::Strcmp | Pure::StrStr => {
                if !matches!((&args[0], &args[1]), (Value::String(_), Value::String(_))) {
                    return None;
                }
                self.pure_builtin(func, args)?
            }
            Pure::Case => {
                let Value::String(s) = &args[0] else {
                    return None;
                };
                if !memory_room(s.len()) {
                    return None;
                }
                self.pure_builtin(func, args)?
            }
            Pure::StrPad => {
                let (Value::String(_), Value::Long(length)) = (&args[0], &args[1]) else {
                    return None;
                };
                // (a non-empty pad string, a valid pad type: no ValueError)
                match args.get(2) {
                    None => {}
                    Some(Value::String(p)) if !p.is_empty() => {}
                    _ => return None,
                }
                match args.get(3) {
                    None | Some(Value::Long(0..=2)) => {}
                    _ => return None,
                }
                if !memory_room((*length).clamp(0, i64::MAX >> 1) as usize) {
                    return None;
                }
                self.pure_builtin(func, args)?
            }
        };
        Some(result)
    }

    /// pure_compute(): the builtin `func` itself, with `args` (which make
    /// it raise nothing: None if it did all the same, nothing done).
    #[inline]
    fn pure_builtin(&mut self, func: BuiltinFn, args: &[Value]) -> Option<Value> {
        match func(self, args) {
            Ok(v) if self.current_exception.is_none() && crate::value::memory_exhausted() == 0 => {
                Some(v)
            }
            _ => None,
        }
    }
}
