//! Fast paths for the VM's hottest opcodes.
//!
//! The general opcode handlers in vm.rs clone both operands, look for
//! operator-overloading objects, lazy objects, numeric-string warnings and
//! so on before doing the actual work.  For the overwhelmingly common case
//! of plain int / float operands none of that can apply, so these helpers
//! handle exactly that case directly on the operand slots and report
//! whether they did.  Whenever anything is unusual (an object, a string, an
//! undefined variable, a reference that may be typed, ...) they return
//! `false` without touching anything, and the general handler runs as
//! before; the results are identical by construction (they use the same
//! `Value` arithmetic / comparison routines as the general handlers).

use std::cell::RefCell;
use std::rc::Rc;

use crate::opcode::{Op, OpCode, OperandType};
use crate::string::PhpString;
use crate::value::Value;

/// A plain int/float operand, by value (no clone of anything refcounted).
#[derive(Clone, Copy)]
pub(crate) enum Num {
    L(i64),
    D(f64),
}

impl Num {
    #[inline(always)]
    pub(crate) fn of(v: &Value) -> Option<Num> {
        match v {
            Value::Long(n) => Some(Num::L(*n)),
            Value::Double(d) => Some(Num::D(*d)),
            _ => None,
        }
    }

    #[inline(always)]
    pub(crate) fn value(self) -> Value {
        match self {
            Num::L(n) => Value::Long(n),
            Num::D(d) => Value::Double(d),
        }
    }
}

/// Read an int/float operand (through a CV's reference if it has one).
/// Undefined variables, strings, objects... give None.
#[inline(always)]
pub(crate) fn num_operand(
    operand: &OperandType,
    cvs: &[Value],
    tmps: &[Value],
    literals: &[Value],
) -> Option<Num> {
    match operand {
        OperandType::Cv(i) => match cvs.get(*i as usize)? {
            Value::Reference(r) => Num::of(&*r.try_borrow().ok()?),
            v => Num::of(v),
        },
        OperandType::Tmp(i) => Num::of(tmps.get(*i as usize)?),
        OperandType::Const(i) => Num::of(literals.get(*i as usize)?),
        _ => None,
    }
}

/// Whether a value can be overwritten without any side effect (no
/// destructor, GC root or object-id bookkeeping is involved).
#[inline(always)]
pub(crate) fn is_plain_scalar(v: &Value) -> bool {
    matches!(
        v,
        Value::Undef
            | Value::Null
            | Value::False
            | Value::True
            | Value::Long(_)
            | Value::Double(_)
            | Value::String(_)
    )
}

/// `*slot = v` for a slot holding a plain value: only a string is
/// released (the others hold nothing; no call into the drop glue).
#[inline(always)]
pub(crate) fn set_plain(slot: &mut Value, v: Value) {
    let old = std::mem::replace(slot, v);
    if matches!(old, Value::String(_) | Value::Resource(_)) {
        drop(old);
    } else {
        debug_assert!(!matches!(
            old,
            Value::Array(_) | Value::Object(_) | Value::Reference(_) | Value::Generator(_)
        ));
        std::mem::forget(old);
    }
}

/// Store into a temporary result slot whose current content is a plain
/// scalar.  Returns false (and stores nothing) otherwise.
#[inline(always)]
pub(crate) fn store_tmp(result: &OperandType, v: Value, tmps: &mut [Value]) -> bool {
    match result {
        OperandType::Tmp(i) => match tmps.get_mut(*i as usize) {
            Some(slot) if is_plain_scalar(slot) => {
                set_plain(slot, v);
                true
            }
            _ => false,
        },
        OperandType::Unused => true,
        _ => false,
    }
}

/// Whether `result` is a slot store_tmp() can write.
#[inline(always)]
pub(crate) fn tmp_writable(result: &OperandType, tmps: &[Value]) -> bool {
    match result {
        OperandType::Tmp(i) => tmps.get(*i as usize).is_some_and(is_plain_scalar),
        OperandType::Unused => true,
        _ => false,
    }
}

/// Overwrite the int/float held by CV `i` (directly or, when `refs_ok`,
/// through a reference).  Only called after num_operand() read a number
/// from that same CV, so the old value needs no cleanup.
#[inline(always)]
pub(crate) fn store_num_cv(i: u32, v: Value, cvs: &mut [Value], refs_ok: bool) -> bool {
    match cvs.get_mut(i as usize) {
        Some(Value::Reference(r)) if refs_ok => match r.try_borrow_mut() {
            Ok(mut b) if matches!(*b, Value::Long(_) | Value::Double(_)) => {
                set_plain(&mut b, v);
                true
            }
            _ => false,
        },
        Some(slot @ (Value::Long(_) | Value::Double(_))) => {
            set_plain(slot, v);
            true
        }
        _ => false,
    }
}

/// Whether CV `i` holds a number that store_num_cv() can overwrite.
#[inline(always)]
pub(crate) fn cv_num_writable(i: u32, cvs: &[Value], refs_ok: bool) -> bool {
    match cvs.get(i as usize) {
        Some(Value::Reference(r)) if refs_ok => r
            .try_borrow()
            .is_ok_and(|b| matches!(*b, Value::Long(_) | Value::Double(_))),
        Some(Value::Long(_) | Value::Double(_)) => true,
        _ => false,
    }
}

/// `a + b`, `a - b`, `a * b` on numbers (Value's own arithmetic, so the
/// int overflow to float behaviour is the general handler's).
/// (Spelled out on the numbers: the same results as Value::add / sub /
/// mul, without their reference and type juggling checks.)
#[inline(always)]
pub(crate) fn arith(opc: u8, a: Num, b: Num) -> Value {
    match (a, b) {
        (Num::L(x), Num::L(y)) => {
            let r = match opc {
                b'+' => x.checked_add(y),
                b'-' => x.checked_sub(y),
                _ => x.checked_mul(y),
            };
            match r {
                Some(r) => Value::Long(r),
                None => Value::Double(match opc {
                    b'+' => x as f64 + y as f64,
                    b'-' => x as f64 - y as f64,
                    _ => x as f64 * y as f64,
                }),
            }
        }
        _ => {
            let (x, y) = match (a, b) {
                (Num::D(x), Num::D(y)) => (x, y),
                (Num::L(x), Num::D(y)) => (x as f64, y),
                (Num::D(x), Num::L(y)) => (x, y as f64),
                (Num::L(x), Num::L(y)) => (x as f64, y as f64),
            };
            Value::Double(match opc {
                b'+' => x + y,
                b'-' => x - y,
                _ => x * y,
            })
        }
    }
}

/// `a < b` (and friends, with swapped operands) on numbers: compare like
/// Value::compare, which the general handlers use.
#[inline(always)]
pub(crate) fn num_cmp(a: Num, b: Num) -> i64 {
    match (a, b) {
        (Num::L(x), Num::L(y)) => (x > y) as i64 - (x < y) as i64,
        _ => a.value().compare(&b.value()),
    }
}

/// Format an int the way PHP converts it to a string.
#[inline]
pub(crate) fn fmt_long(n: i64, buf: &mut [u8; 20]) -> &[u8] {
    let mut i = buf.len();
    let neg = n < 0;
    let mut u = n.unsigned_abs();
    loop {
        i -= 1;
        buf[i] = b'0' + (u % 10) as u8;
        u /= 10;
        if u == 0 {
            break;
        }
    }
    if neg {
        i -= 1;
        buf[i] = b'-';
    }
    &buf[i..]
}

/// `$cv .= <string|int>` appended in place when the variable's string is
/// not shared (see PhpString::append_in_place).  `refs_ok`: the CV may be
/// followed through a reference (no typed references exist).
/// Returns false when the general handler must run.
pub(crate) fn assign_concat_in_place(
    op1: &OperandType,
    op2: &OperandType,
    cvs: &mut [Value],
    tmps: &[Value],
    literals: &[Value],
    refs_ok: bool,
) -> bool {
    let OperandType::Cv(ci) = *op1 else {
        return false;
    };
    // The appended value: a string (held by its own handle, so `$s .= $s`
    // sees a shared string and takes the general path) or an int.
    enum Rhs {
        S(PhpString),
        L(i64),
    }
    let rhs_of = |v: &Value| match v {
        Value::String(s) => Some(Rhs::S(s.clone())),
        Value::Long(n) => Some(Rhs::L(*n)),
        _ => None,
    };
    let rhs = match op2 {
        OperandType::Cv(i) => match cvs.get(*i as usize) {
            Some(Value::Reference(r)) => match r.try_borrow() {
                Ok(b) => rhs_of(&b),
                Err(_) => None,
            },
            Some(v) => rhs_of(v),
            None => None,
        },
        OperandType::Tmp(i) => tmps.get(*i as usize).and_then(rhs_of),
        OperandType::Const(i) => literals.get(*i as usize).and_then(rhs_of),
        _ => None,
    };
    let Some(rhs) = rhs else {
        return false;
    };
    let mut buf = [0u8; 20];
    let bytes: &[u8] = match &rhs {
        Rhs::S(s) => s.as_bytes(),
        Rhs::L(n) => fmt_long(*n, &mut buf),
    };
    match cvs.get_mut(ci as usize) {
        Some(Value::String(s)) => s.append_in_place(bytes),
        Some(Value::Reference(r)) if refs_ok => match r.try_borrow_mut() {
            Ok(mut b) => match &mut *b {
                Value::String(s) => s.append_in_place(bytes),
                _ => false,
            },
            Err(_) => false,
        },
        _ => false,
    }
}

/// php_increment / php_decrement on a number.
#[inline(always)]
fn inc_dec(old: Num, inc: bool) -> Value {
    match (old, inc) {
        (Num::L(n), true) => match n.checked_add(1) {
            Some(r) => Value::Long(r),
            None => Value::Double(n as f64 + 1.0),
        },
        (Num::L(n), false) => match n.checked_sub(1) {
            Some(r) => Value::Long(r),
            None => Value::Double(n as f64 - 1.0),
        },
        (Num::D(d), true) => Value::Double(d + 1.0),
        (Num::D(d), false) => Value::Double(d - 1.0),
    }
}

#[inline(always)]
fn bool_val(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

/// The integer an array key string stands for, if it is the canonical
/// decimal form of an int ("0", "-5", "42"; not "+5", "05", "-0", " 5" or
/// out of range), like PHP's ZEND_HANDLE_NUMERIC_STR.  Allocation-free
/// equivalent of `s.parse::<i64>()` + `n.to_string() == s`.
#[inline]
pub fn canonical_int_key(b: &[u8]) -> Option<i64> {
    let digits = b.strip_prefix(b"-").unwrap_or(b);
    if digits.is_empty() || digits.len() > 19 || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if digits[0] == b'0' && (digits.len() > 1 || digits.len() != b.len()) {
        return None;
    }
    // ASCII digits (and '-') only, so this is valid UTF-8; parse() rejects
    // values out of i64 range.
    std::str::from_utf8(b).ok()?.parse::<i64>().ok()
}

/// Outcome of fast_op().
pub(crate) enum Fast {
    /// Not handled: run the general handler.
    No,
    /// Handled; continue with the next opcode.
    Done,
    /// Handled; continue at this opcode index.
    Jump(usize),
}

/// Try to execute `op` on plain int/float (and, for assignments, plain
/// scalar) operands.  `refs_ok`: no typed references exist, so CVs bound
/// by reference may be written through.  `no_release`: no object is
/// waiting for its destructor (the general Assign handler is a release
/// point; it is only bypassed when that cannot matter).
#[inline(always)]
pub(crate) fn fast_op(
    op: &Op,
    cvs: &mut [Value],
    tmps: &mut [Value],
    literals: &[Value],
    refs_ok: bool,
    no_release: bool,
) -> Fast {
    macro_rules! num {
        ($o:expr) => {
            match num_operand(&$o, cvs, tmps, literals) {
                Some(n) => n,
                None => return Fast::No,
            }
        };
    }
    macro_rules! store {
        ($v:expr) => {
            if store_tmp(&op.result, $v, tmps) {
                Fast::Done
            } else {
                Fast::No
            }
        };
    }
    match op.opcode {
        OpCode::Add | OpCode::Sub | OpCode::Mul => {
            let a = num!(op.op1);
            let b = num!(op.op2);
            let c = match op.opcode {
                OpCode::Add => b'+',
                OpCode::Sub => b'-',
                _ => b'*',
            };
            store!(arith(c, a, b))
        }
        OpCode::Less | OpCode::LessEqual | OpCode::Greater | OpCode::GreaterEqual => {
            let a = num!(op.op1);
            let b = num!(op.op2);
            // `a > b` is `b < a` (ZEND_IS_SMALLER with swapped operands), as
            // in the general handlers.
            let r = match op.opcode {
                OpCode::Less => num_cmp(a, b) < 0,
                OpCode::LessEqual => num_cmp(a, b) <= 0,
                OpCode::Greater => num_cmp(b, a) < 0,
                _ => num_cmp(b, a) <= 0,
            };
            store!(bool_val(r))
        }
        OpCode::Equal | OpCode::NotEqual => {
            let a = num!(op.op1);
            let b = num!(op.op2);
            let eq = a.value().equals(&b.value());
            store!(bool_val(eq == (op.opcode == OpCode::Equal)))
        }
        OpCode::Identical | OpCode::NotIdentical => {
            let a = num!(op.op1);
            let b = num!(op.op2);
            let same = a.value().identical(&b.value());
            store!(bool_val(same == (op.opcode == OpCode::Identical)))
        }
        OpCode::AssignAdd | OpCode::AssignSub | OpCode::AssignMul => {
            let OperandType::Cv(ci) = op.op1 else {
                return Fast::No;
            };
            if !cv_num_writable(ci, cvs, refs_ok) {
                return Fast::No;
            }
            let a = num!(op.op1);
            let b = num!(op.op2);
            let c = match op.opcode {
                OpCode::AssignAdd => b'+',
                OpCode::AssignSub => b'-',
                _ => b'*',
            };
            if store_num_cv(ci, arith(c, a, b), cvs, refs_ok) {
                Fast::Done
            } else {
                Fast::No
            }
        }
        OpCode::PreIncrement
        | OpCode::PreDecrement
        | OpCode::PostIncrement
        | OpCode::PostDecrement => {
            let inc = matches!(op.opcode, OpCode::PreIncrement | OpCode::PostIncrement);
            let pre = matches!(op.opcode, OpCode::PreIncrement | OpCode::PreDecrement);
            // A temporary (`$o->p++` fetches the property into one): the
            // result is written first, then the temporary.
            if let OperandType::Tmp(ti) = op.op1 {
                let old = match tmps.get(ti as usize) {
                    Some(Value::Long(n)) => Num::L(*n),
                    Some(Value::Double(d)) => Num::D(*d),
                    _ => return Fast::No,
                };
                if !tmp_writable(&op.result, tmps) {
                    return Fast::No;
                }
                let new = inc_dec(old, inc);
                let res = if pre { new.clone() } else { old.value() };
                store_tmp(&op.result, res, tmps);
                if let Some(slot) = tmps.get_mut(ti as usize) {
                    *slot = new;
                }
                return Fast::Done;
            }
            let OperandType::Cv(ci) = op.op1 else {
                return Fast::No;
            };
            if !cv_num_writable(ci, cvs, refs_ok) || !tmp_writable(&op.result, tmps) {
                return Fast::No;
            }
            let old = num!(op.op1);
            let new = inc_dec(old, inc);
            let res = if pre { new.clone() } else { old.value() };
            if !store_num_cv(ci, new, cvs, refs_ok) {
                return Fast::No;
            }
            store_tmp(&op.result, res, tmps);
            Fast::Done
        }
        OpCode::JmpZ | OpCode::JmpNz => {
            let v = match op.op1 {
                OperandType::Tmp(i) => tmps.get(i as usize),
                OperandType::Cv(i) => cvs.get(i as usize),
                OperandType::Const(i) => literals.get(i as usize),
                _ => None,
            };
            let truthy = match v {
                Some(Value::True) => true,
                Some(Value::False) | Some(Value::Null) => false,
                Some(Value::Long(n)) => *n != 0,
                _ => return Fast::No,
            };
            match op.op2 {
                OperandType::JmpTarget(t) if truthy == (op.opcode == OpCode::JmpNz) => {
                    Fast::Jump(t as usize)
                }
                _ => Fast::Done,
            }
        }
        OpCode::Assign => {
            let OperandType::Cv(ci) = op.op1 else {
                return Fast::No;
            };
            if !no_release || !tmp_writable(&op.result, tmps) {
                return Fast::No;
            }
            let src = match op.op2 {
                OperandType::Tmp(i) => tmps.get(i as usize),
                OperandType::Cv(i) => cvs.get(i as usize),
                OperandType::Const(i) => literals.get(i as usize),
                _ => None,
            };
            let val = match src {
                Some(v) if is_plain_scalar(v) && !matches!(v, Value::Undef) => v.clone(),
                _ => return Fast::No,
            };
            let stored = match cvs.get_mut(ci as usize) {
                Some(Value::Reference(r)) if refs_ok => match r.try_borrow_mut() {
                    Ok(mut b) if is_plain_scalar(&b) => {
                        *b = val.clone();
                        true
                    }
                    _ => false,
                },
                Some(slot) if is_plain_scalar(slot) => {
                    *slot = val.clone();
                    true
                }
                _ => false,
            };
            if !stored {
                return Fast::No;
            }
            if let OperandType::Tmp(i) = op.op2 {
                if let Some(slot) = tmps.get_mut(i as usize) {
                    *slot = Value::Undef;
                }
            }
            store_tmp(&op.result, val, tmps);
            Fast::Done
        }
        OpCode::QmAssign => {
            let src = match op.op1 {
                OperandType::Tmp(i) => tmps.get(i as usize),
                OperandType::Cv(i) => cvs.get(i as usize),
                OperandType::Const(i) => literals.get(i as usize),
                _ => None,
            };
            let val = match src {
                Some(v) if is_plain_scalar(v) && !matches!(v, Value::Undef) => v.clone(),
                _ => return Fast::No,
            };
            store!(val)
        }
        OpCode::AssignConcat => {
            if assign_concat_in_place(&op.op1, &op.op2, cvs, tmps, literals, refs_ok) {
                Fast::Done
            } else {
                Fast::No
            }
        }
        _ => Fast::No,
    }
}

/// fast_op() for the dispatch loop's own fast attempt (which runs when
/// run_fast_ops() cannot: out of line, the loop's frame stays small).
#[inline(never)]
pub(crate) fn fast_op_outlined(
    op: &Op,
    cvs: &mut [Value],
    tmps: &mut [Value],
    literals: &[Value],
    refs_ok: bool,
    no_release: bool,
) -> Fast {
    fast_op(op, cvs, tmps, literals, refs_ok, no_release)
}

/// Lowercase `s` into `buf` (None when it does not fit: the caller then
/// takes the general path).
#[inline]
fn lower_into<'b>(s: &[u8], buf: &'b mut [u8]) -> Option<&'b [u8]> {
    let out = buf.get_mut(..s.len())?;
    for (o, c) in out.iter_mut().zip(s) {
        *o = c.to_ascii_lowercase();
    }
    Some(out)
}

/// An int or string array key operand (through a CV's reference).  Other
/// key types (null, float, bool, ...) may warn and take the general path.
#[inline]
fn dim_key(
    operand: &OperandType,
    cvs: &[Value],
    tmps: &[Value],
    literals: &[Value],
) -> Option<crate::array::ArrayKey> {
    let of = |v: &Value| match v {
        Value::Long(n) => Some(crate::array::ArrayKey::Int(*n)),
        Value::String(s) => Some(crate::vm::Vm::value_to_array_key(Value::String(s.clone()))),
        _ => None,
    };
    match operand {
        OperandType::Cv(i) => match cvs.get(*i as usize)? {
            Value::Reference(r) => of(&*r.try_borrow().ok()?),
            v => of(v),
        },
        OperandType::Tmp(i) => of(tmps.get(*i as usize)?),
        OperandType::Const(i) => of(literals.get(*i as usize)?),
        _ => None,
    }
}

/// Whether the CVs `op` names all hold references, none of them `globals`.
#[inline]
fn cv_operands_bound(op: &Op, cvs: &[Value], globals: Option<u32>) -> bool {
    [op.op1, op.op2, op.result].iter().all(|o| match o {
        OperandType::Cv(i) => {
            Some(*i) != globals && matches!(cvs.get(*i as usize), Some(Value::Reference(_)))
        }
        _ => true,
    })
}

/// Whether run_fast_ops() may handle an opcode (a superset of what the
/// dispatch loop's own fast attempt handles).
#[inline(always)]
pub(crate) fn runs_fast(opc: OpCode) -> bool {
    matches!(
        opc,
        OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Less
            | OpCode::LessEqual
            | OpCode::Greater
            | OpCode::GreaterEqual
            | OpCode::Equal
            | OpCode::NotEqual
            | OpCode::Identical
            | OpCode::NotIdentical
            | OpCode::AssignAdd
            | OpCode::AssignSub
            | OpCode::AssignMul
            | OpCode::PreIncrement
            | OpCode::PreDecrement
            | OpCode::PostIncrement
            | OpCode::PostDecrement
            | OpCode::JmpZ
            | OpCode::JmpNz
            | OpCode::Assign
            | OpCode::QmAssign
            | OpCode::AssignConcat
            | OpCode::PropertyGet
            | OpCode::PropertySet
            | OpCode::PropertyGetW
            | OpCode::PropertyGetIncDec
            | OpCode::ArrayGet
            | OpCode::ArraySet
            | OpCode::ArrayAppend
            | OpCode::Jmp
            | OpCode::FreeTmps
            | OpCode::InitFCall
            | OpCode::SendVal
    )
}

impl crate::vm::Vm {
    /// Run the opcodes from `ip` on while fast_op(), fast_dim_op() and
    /// fast_prop_op() handle them, as the dispatch loop would one at a time
    /// (they only touch plain values: nothing the loop checks between two
    /// opcodes changes, but for memory_limit and max_execution_time, which
    /// hand control back to it). Returns the index of the first opcode not
    /// handled (or of the next one to run when the loop has to look).
    /// `refs_ok` / `no_release`: see fast_op(); `foreach_idle`: the frame
    /// has no foreach iteration state (see FreeTmps); `bound`: the frame's
    /// variables are bound to a symbol table or it uses $GLOBALS (the CV
    /// index of $GLOBALS, if it does).
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run_fast_ops(
        &mut self,
        op_array: &crate::opcode::OpArray,
        mut ip: usize,
        cvs: &mut [Value],
        tmps: &mut [Value],
        internal_code: bool,
        refs_ok: bool,
        no_release: bool,
        foreach_idle: bool,
        bound: Option<Option<u32>>,
    ) -> usize {
        let ops = &op_array.ops;
        let literals = &op_array.literals;
        // (the line of the last opcode run: the dispatch loop's current line
        // once control goes back to it; nothing here reads it)
        let mut line = None;
        let next = 'run: loop {
            let Some(op) = ops.get(ip) else {
                break 'run ip;
            };
            // A frame bound to a symbol table: only opcodes whose variables
            // are all bound (references), none of them $GLOBALS, run here
            // (the dispatch loop repairs bindings after the others).
            if let Some(globals) = bound {
                if !cv_operands_bound(op, cvs, globals) {
                    break 'run ip;
                }
            }
            let fast = match op.opcode {
                // A call by a constant plain function name (see the
                // INIT_FCALL arm of the dispatch loop).
                OpCode::InitFCall => {
                    if !self.init_fcall_fast(op_array, ip, cvs) {
                        break 'run ip;
                    }
                    Fast::Done
                }
                // A by-value argument of a call resolved when it was set up
                // (see the SEND_VAL handler).
                OpCode::SendVal => {
                    if self.send_val_fast(op, cvs, tmps, literals) {
                        Fast::Done
                    } else {
                        break 'run ip;
                    }
                }
                // (the read of `$o->p++` is one of an object's property)
                OpCode::PropertyGet
                | OpCode::PropertySet
                | OpCode::PropertyGetW
                | OpCode::PropertyGetIncDec => {
                    if self.isset_get_cache.is_some() {
                        break 'run ip;
                    }
                    self.fast_prop_op(op_array, ip, cvs, tmps)
                }
                OpCode::ArrayGet | OpCode::ArraySet | OpCode::ArrayAppend => {
                    self.fast_dim_op(op, cvs, tmps, literals)
                }
                OpCode::Jmp => match op.op1 {
                    OperandType::JmpTarget(t) => Fast::Jump(t as usize),
                    _ => return ip,
                },
                // A statement's temporaries that hold plain values, with no
                // foreach state and nothing to destroy: the general handler
                // just empties them.
                OpCode::FreeTmps if foreach_idle && no_release => {
                    let (OperandType::Tmp(a), OperandType::Tmp(b)) = (op.op1, op.op2) else {
                        break 'run ip;
                    };
                    let end = (b as usize).min(tmps.len());
                    let range = &mut tmps[(a as usize).min(end)..end];
                    if range.iter().any(|v| {
                        matches!(
                            v,
                            Value::Object(_)
                                | Value::Array(_)
                                | Value::Reference(_)
                                | Value::Resource(_)
                        )
                    }) {
                        break 'run ip;
                    }
                    for slot in range {
                        *slot = Value::Undef;
                    }
                    Fast::Done
                }
                // (one arm per opcode: each inlined fast_op() reduces to its own case)
                OpCode::Add => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::Sub => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::Mul => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::Less => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::LessEqual => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::Greater => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::GreaterEqual => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::Equal => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::NotEqual => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::Identical => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::NotIdentical => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::AssignAdd => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::AssignSub => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::AssignMul => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::PreIncrement => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::PreDecrement => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::PostIncrement => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::PostDecrement => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::JmpZ => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::JmpNz => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::Assign => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::QmAssign => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::AssignConcat => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                _ => return ip,
            };
            ip = match fast {
                Fast::No => return ip,
                Fast::Done => ip + 1,
                Fast::Jump(t) => t,
            };
            line = Some(op.line);
            if crate::value::memory_exhausted() != 0
                || crate::value::VM_INTERRUPT.load(std::sync::atomic::Ordering::Relaxed)
            {
                break 'run ip;
            }
        };
        if let (Some(l), false) = (line, internal_code) {
            self.current_line = l;
        }
        next
    }

    /// INIT_FCALL by a constant plain function name or static method name
    /// (see push_plain_call / push_static_call). False: nothing was done.
    #[inline(never)]
    fn init_fcall_fast(
        &mut self,
        op_array: &crate::opcode::OpArray,
        ip: usize,
        cvs: &[Value],
    ) -> bool {
        let op = &op_array.ops[ip];
        let literals = &op_array.literals;
        if let Some(name) = crate::vm::plain_fcall_name(op, literals) {
            self.push_plain_call(op_array, ip, name.clone());
            true
        } else if let Some(name) = crate::vm::static_fcall_name(op, literals) {
            let frame_this = op_array.cv_names.first().map(|n| n.as_slice()) == Some(b"this")
                && matches!(cvs.first().map(|v| v.deref()), Some(Value::Object(_)));
            self.push_static_call(op_array, ip, name, frame_this)
        } else if let Some(name) = crate::vm::forward_fcall_name(op, literals) {
            let this = if op_array.cv_names.first().map(|n| n.as_slice()) == Some(b"this") {
                match cvs.first().map(|v| v.deref()) {
                    Some(t @ Value::Object(_)) => Some(t),
                    _ => None,
                }
            } else {
                None
            };
            self.push_forward_call(op_array, ip, name, this.as_ref());
            true
        } else {
            false
        }
    }

    /// `$a[k]` read, `$cv[k] = <scalar>` write and `$cv[] = <scalar>` append
    /// on a real array with an int / string key, where the element (for a
    /// read) exists or (for a write) is absent or a plain scalar that is not
    /// a reference.  The
    /// general handlers do exactly this in that case, after checks for
    /// objects, strings, undefined variables, illegal offsets and the
    /// release of an overwritten object / array.
    pub(crate) fn fast_dim_op(
        &self,
        op: &Op,
        cvs: &mut [Value],
        tmps: &mut [Value],
        literals: &[Value],
    ) -> Fast {
        let append = op.opcode == OpCode::ArrayAppend;
        let write = append || op.opcode == OpCode::ArraySet;
        // The write-back of `$a[$k] op= v` uses the key its fetch converted,
        // or is abandoned (see Vm::dim_rw_key / dim_rw_abort).
        if write && (self.dim_rw_key.is_some() || self.dim_rw_abort.is_some()) {
            return Fast::No;
        }
        let arr_of = |v: &Value| match v {
            Value::Array(a) => Some(a.clone()),
            Value::Reference(r) => match &*r.try_borrow().ok()? {
                Value::Array(a) => Some(a.clone()),
                _ => None,
            },
            _ => None,
        };
        let arr = match op.op1 {
            OperandType::Cv(i) => cvs.get(i as usize).and_then(arr_of),
            OperandType::Tmp(i) if !write => match tmps.get(i as usize) {
                Some(Value::Array(a)) => Some(a.clone()),
                _ => None,
            },
            _ => None,
        };
        let Some(arr) = arr else {
            return Fast::No;
        };
        // $GLOBALS (the symbol table) has its own bookkeeping.
        if Rc::ptr_eq(&arr, &self.globals) {
            return Fast::No;
        }
        let val = if write {
            let val = match op.op2 {
                OperandType::Tmp(i) => tmps.get(i as usize),
                OperandType::Cv(i) => cvs.get(i as usize),
                OperandType::Const(i) => literals.get(i as usize),
                _ => None,
            };
            match val {
                Some(v) if is_plain_scalar(v) && !matches!(v, Value::Undef) => Some(v.clone()),
                _ => return Fast::No,
            }
        } else {
            None
        };
        if append {
            let Ok(mut a) = arr.try_borrow_mut() else {
                return Fast::No;
            };
            // A full array ("next element is already occupied") raises its
            // Error in the general handler; try_push() leaves it unchanged.
            return match a.try_push(val.unwrap_or_default()) {
                Ok(()) => Fast::Done,
                Err(_) => Fast::No,
            };
        }
        let key_op = if write { &op.result } else { &op.op2 };
        let Some(key) = dim_key(key_op, cvs, tmps, literals) else {
            return Fast::No;
        };
        if let Some(val) = val {
            let Ok(mut a) = arr.try_borrow_mut() else {
                return Fast::No;
            };
            if !a.get(&key).is_none_or(is_plain_scalar) {
                return Fast::No;
            }
            a.set(key, val);
            return Fast::Done;
        }
        if !tmp_writable(&op.result, tmps) {
            return Fast::No;
        }
        let v = {
            let Ok(a) = arr.try_borrow() else {
                return Fast::No;
            };
            match a.get(&key) {
                Some(v) => v.deref(),
                None => return Fast::No,
            }
        };
        match &v {
            Value::Object(o) => crate::gc::track_object(o),
            Value::Generator(g) => crate::gc::track_generator(g),
            _ => {}
        }
        if store_tmp(&op.result, v, tmps) {
            Fast::Done
        } else {
            Fast::No
        }
    }

    /// Whether `$obj->prop` on an instance of `class_name` is a plain slot
    /// access: the class and all its ancestors are user classes without
    /// extension handlers, `prop` is declared exactly once in the hierarchy
    /// as a non-static instance property without hooks, accessible from
    /// `scope` (public; private, declared by `class_name`, from that class;
    /// protected, from its declaring class or from `class_name`) (for a
    /// write: also untyped, not readonly, no asymmetric set visibility, in
    /// a class that is not readonly).  Magic methods do not matter: they are
    /// only consulted for inaccessible or missing properties, and the
    /// caller checks the object holds an initialized slot for `prop`.
    fn plain_public_prop(
        &self,
        class_name: &[u8],
        prop: &[u8],
        write: bool,
        scope: Option<&[u8]>,
    ) -> bool {
        let mut buf = [0u8; 128];
        let Some(lower) = lower_into(class_name, &mut buf) else {
            return false;
        };
        let Some(ce) = self.classes.get(lower) else {
            return false;
        };
        if ce.is_enum || ce.filename.is_none() || (write && ce.is_readonly) {
            return false;
        }
        let mut decl = None;
        for p in &ce.properties {
            if p.name == prop {
                if decl.is_some() {
                    return false;
                }
                decl = Some(p);
            }
        }
        let Some(p) = decl else {
            return false;
        };
        let in_scope = |c: &[u8]| scope.is_some_and(|s| s.eq_ignore_ascii_case(c));
        let accessible = match p.visibility {
            crate::object::Visibility::Public => true,
            crate::object::Visibility::Private => {
                p.declaring_class.eq_ignore_ascii_case(class_name) && in_scope(&p.declaring_class)
            }
            crate::object::Visibility::Protected => {
                in_scope(&p.declaring_class) || in_scope(class_name)
            }
        };
        if p.is_static
            || !accessible
            || p.has_get_hook
            || p.has_set_hook
            || p.is_virtual
            || (write && (p.is_readonly || p.property_type.is_some() || p.set_visibility.is_some()))
        {
            return false;
        }
        // Hooks are compiled to `__property_get_<name>` / `__property_set_<name>`.
        let mut hbuf = [0u8; 160];
        let prefix: &[u8] = if write {
            b"__property_set_"
        } else {
            b"__property_get_"
        };
        let Some(h) = hbuf.get_mut(..prefix.len() + prop.len()) else {
            return false;
        };
        h[..prefix.len()].copy_from_slice(prefix);
        for (o, c) in h[prefix.len()..].iter_mut().zip(prop) {
            *o = c.to_ascii_lowercase();
        }
        if ce.methods.contains_key(&*h) {
            return false;
        }
        if self.native_class_handlers.contains_key(lower) {
            return false;
        }
        let mut parent = ce.parent.as_deref();
        for _ in 0..64 {
            let Some(pn) = parent else {
                return true;
            };
            let mut pbuf = [0u8; 128];
            let Some(pl) = lower_into(pn, &mut pbuf) else {
                return false;
            };
            if self.native_class_handlers.contains_key(pl) {
                return false;
            }
            let Some(pce) = self.classes.get(pl) else {
                return plain_internal_base(pl, prop);
            };
            if pce.filename.is_none() {
                return false;
            }
            parent = pce.parent.as_deref();
        }
        false
    }

    /// `$cv->name` read / `$cv->name = <scalar>` write of a plain declared
    /// public property (see plain_public_prop) on a user object that has
    /// no internal state markers (lazy, cloning, enum case, destructed... are
    /// all stored as `__`-prefixed slots).  Everything else takes the
    /// general PropertyGet / PropertySet handlers.
    /// (`ip`: the opcode's index in `op_array`, whose prop_sites remember
    /// what plain_public_prop() answered for the class last seen there.)
    pub(crate) fn fast_prop_op(
        &self,
        op_array: &crate::opcode::OpArray,
        ip: usize,
        cvs: &mut [Value],
        tmps: &mut [Value],
    ) -> Fast {
        let op = &op_array.ops[ip];
        let literals = &op_array.literals;
        if !self.property_hook_stack.is_empty() {
            return Fast::No;
        }
        let write = op.opcode == OpCode::PropertySet;
        let obj = match op.op1 {
            // (a variable bound to a symbol table holds a reference)
            OperandType::Cv(ci) => match cvs.get(ci as usize) {
                Some(Value::Object(obj)) => obj.clone(),
                Some(Value::Reference(r)) => match r.try_borrow().as_deref() {
                    Ok(Value::Object(obj)) => obj.clone(),
                    _ => return Fast::No,
                },
                _ => return Fast::No,
            },
            // `$a->b->c`, `f()->p`: a read of an object held by a temporary
            // (one held elsewhere too: releasing the temporary then destroys
            // nothing)
            OperandType::Tmp(ti) if op.opcode == OpCode::PropertyGet => {
                match tmps.get(ti as usize) {
                    Some(Value::Object(obj)) if Rc::strong_count(obj) > 1 => obj.clone(),
                    _ => return Fast::No,
                }
            }
            _ => return Fast::No,
        };
        // `$o->p[k] = v` / `$o->p[] = v` (see prop_dim_write)
        let w_fetch = op.opcode == OpCode::PropertyGetW;
        let name_op = if write { &op.result } else { &op.op2 };
        let name = match name_op {
            OperandType::Const(i) => match literals.get(*i as usize) {
                Some(Value::String(s)) => s.as_bytes(),
                _ => return Fast::No,
            },
            _ => return Fast::No,
        };
        if name.is_empty() || name[0] == 0 || name.starts_with(b"__") {
            return Fast::No;
        }
        // The value to store: a plain scalar.
        let new_val = if write {
            let src = match op.op2 {
                OperandType::Tmp(i) => tmps.get(i as usize),
                OperandType::Cv(i) => cvs.get(i as usize),
                OperandType::Const(i) => literals.get(i as usize),
                _ => None,
            };
            match src {
                Some(v) if is_plain_scalar(v) && !matches!(v, Value::Undef) => Some(v.clone()),
                // (the value of a variable bound to a symbol table)
                Some(Value::Reference(r)) if matches!(op.op2, OperandType::Cv(_)) => {
                    match r.try_borrow().as_deref() {
                        Ok(v) if is_plain_scalar(v) && !matches!(v, Value::Undef) => {
                            Some(v.clone())
                        }
                        _ => return Fast::No,
                    }
                }
                _ => return Fast::No,
            }
        } else {
            if !w_fetch && !tmp_writable(&op.result, tmps) {
                return Fast::No;
            }
            None
        };
        let Ok(ob) = obj.try_borrow() else {
            return Fast::No;
        };
        if ob.native.is_some() || ob.closure.is_some() {
            return Fast::No;
        }
        let mut slot = None;
        for (i, (k, v)) in ob.properties.iter().enumerate() {
            if k.starts_with(b"__") {
                return Fast::No;
            }
            if k.as_slice() == name {
                slot = Some((i, v));
            }
        }
        let Some((idx, cur)) = slot else {
            return Fast::No;
        };
        // An uninitialized slot, or (for a write) one that is a reference
        // or holds something releasing / tracking could care about.
        let ok = if write {
            is_plain_scalar(cur) && !matches!(cur, Value::Undef)
        } else if w_fetch {
            matches!(cur, Value::Array(_))
        } else if op.opcode == OpCode::PropertyGetIncDec && matches!(cur, Value::Object(_)) {
            // (++/-- of a readonly property is refused before the object
            // is incremented: the general handler checks)
            false
        } else {
            match cur {
                Value::Undef => false,
                Value::Reference(r) => r.try_borrow().is_ok_and(|b| !matches!(*b, Value::Undef)),
                _ => true,
            }
        };
        if !ok {
            return Fast::No;
        }
        // (a function of the class and the property: classes do not change
        // once declared)
        let scope = self.class_scope_ref();
        let cached = match op_array
            .sites()
            .borrow()
            .as_ref()
            .and_then(|s| s.prop_sites.get(&(ip as u32)))
        {
            Some((class, site_scope, plain))
                if *class == ob.class_name
                    && match (site_scope, scope) {
                        (None, None) => true,
                        (Some(a), Some(b)) => a.as_slice() == b,
                        _ => false,
                    } =>
            {
                Some(*plain)
            }
            _ => None,
        };
        let plain = match cached {
            Some(p) => p,
            None => {
                let p = self.plain_public_prop(&ob.class_name, name, write || w_fetch, scope);
                op_array
                    .sites()
                    .borrow_mut()
                    .get_or_insert_with(Default::default)
                    .prop_sites
                    .insert(
                        ip as u32,
                        (ob.class_name.clone(), scope.map(|s| s.to_vec()), p),
                    );
                p
            }
        };
        if !plain {
            return Fast::No;
        }
        if w_fetch {
            drop(ob);
            return self.prop_dim_write(op_array, ip, &obj, idx, cvs, tmps);
        }
        if let Some(v) = new_val {
            drop(ob);
            let Ok(mut obm) = obj.try_borrow_mut() else {
                return Fast::No;
            };
            obm.properties[idx].1 = v;
            return Fast::Done;
        }
        let v = cur.deref();
        drop(ob);
        // As write_operand(): objects / generators stored in a temporary
        // are registered with the cycle collector.
        match &v {
            Value::Object(o) => crate::gc::track_object(o),
            Value::Generator(g) => crate::gc::track_generator(g),
            _ => {}
        }
        if store_tmp(&op.result, v, tmps) {
            Fast::Done
        } else {
            Fast::No
        }
    }

    /// fast_prop_op() of the PropertyGetW at `ip` of a plain property
    /// (slot `idx` of `obj`) holding an array, whose result the next opcode
    /// writes a plain value into (`$o->p[k] = v`, `$o->p[] = v`): what both
    /// do then (the write fetch separates an array shared with other
    /// holders; the write is fast_dim_op()'s), the fetched container not
    /// left in its temporary. Jumps past both.
    fn prop_dim_write(
        &self,
        op_array: &crate::opcode::OpArray,
        ip: usize,
        obj: &Rc<RefCell<crate::object::PhpObject>>,
        idx: usize,
        cvs: &[Value],
        tmps: &mut [Value],
    ) -> Fast {
        let op = &op_array.ops[ip];
        let literals = &op_array.literals;
        let OperandType::Tmp(t) = op.result else {
            return Fast::No;
        };
        let Some(next) = op_array.ops.get(ip + 1) else {
            return Fast::No;
        };
        let append = next.opcode == OpCode::ArrayAppend;
        if !(append || next.opcode == OpCode::ArraySet) || next.op1 != OperandType::Tmp(t) {
            return Fast::No;
        }
        if self.dim_rw_key.is_some() || self.dim_rw_abort.is_some() {
            return Fast::No;
        }
        // (a variable bound to a symbol table holds a reference)
        let val = match next.op2 {
            OperandType::Tmp(i) => tmps.get(i as usize).cloned(),
            OperandType::Cv(i) => match cvs.get(i as usize) {
                Some(Value::Reference(r)) => r.try_borrow().ok().map(|v| v.clone()),
                v => v.cloned(),
            },
            OperandType::Const(i) => literals.get(i as usize).cloned(),
            _ => None,
        };
        let val = match val {
            Some(v) if is_plain_scalar(&v) && !matches!(v, Value::Undef) => v,
            _ => return Fast::No,
        };
        let key = if append {
            None
        } else {
            match dim_key(&next.result, cvs, tmps, literals) {
                Some(k) => Some(k),
                None => return Fast::No,
            }
        };
        // (the holder of the previous fetch, as PropertyGetW drops it)
        if let Some(slot) = tmps.get_mut(t as usize) {
            *slot = Value::Undef;
        }
        let Ok(mut ob) = obj.try_borrow_mut() else {
            return Fast::No;
        };
        let arr = match ob.properties.get(idx) {
            Some((_, Value::Array(arr))) => arr.clone(),
            _ => return Fast::No,
        };
        // (an element releasing its value takes the general path)
        match (&key, arr.try_borrow()) {
            (Some(k), Ok(a)) if a.get(k).is_none_or(is_plain_scalar) => {}
            (None, Ok(_)) => {}
            _ => return Fast::No,
        }
        // Shared with other holders (but this clone): the property gets
        // its own copy.
        let arr = if Rc::strong_count(&arr) > 2 {
            let sep = Rc::new(RefCell::new(arr.borrow().clone()));
            let slot = &mut ob.properties[idx].1;
            let old = std::mem::replace(slot, Value::Array(sep.clone()));
            crate::gc::on_value_replaced(&old, slot);
            sep
        } else {
            arr
        };
        drop(ob);
        let Ok(mut a) = arr.try_borrow_mut() else {
            return Fast::No;
        };
        match key {
            Some(k) => a.set(k, val),
            // A full array ("next element is already occupied") raises its
            // Error in the general handler; try_push() leaves it unchanged.
            None => {
                if a.try_push(val).is_err() {
                    return Fast::No;
                }
            }
        }
        Fast::Jump(ip + 2)
    }
}

/// Whether internal class `lower` (and its ancestors) keeps its objects'
/// properties as plain slots, declaring no property `prop`: the Exception
/// and Error hierarchy, whose objects have no handlers of their own.
fn plain_internal_base(lower: &[u8], prop: &[u8]) -> bool {
    let mut cur = crate::stubs::find_class_lower(lower);
    for _ in 0..16 {
        let Some(c) = cur else {
            return false;
        };
        if c.props.iter().any(|p| p.name.as_bytes() == prop) {
            return false;
        }
        match c.parent {
            None => return c.name == "Exception" || c.name == "Error",
            Some(p) => cur = crate::stubs::find_class(p.as_bytes()),
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_keys() {
        for s in [
            "0",
            "1",
            "-1",
            "42",
            "-0",
            "01",
            "+1",
            " 1",
            "1 ",
            "",
            "-",
            "1.0",
            "1e3",
            "9223372036854775807",
            "9223372036854775808",
            "-9223372036854775808",
            "-9223372036854775809",
            "00",
            "abc",
            "12345678901234567890",
        ] {
            let old = s.parse::<i64>().ok().filter(|n| n.to_string() == s);
            assert_eq!(canonical_int_key(s.as_bytes()), old, "{s:?}");
        }
    }

    #[test]
    fn long_formatting() {
        let mut b = [0u8; 20];
        for n in [0i64, 7, -7, 10, 1234567890, i64::MAX, i64::MIN] {
            assert_eq!(fmt_long(n, &mut b), n.to_string().as_bytes());
        }
    }

    #[test]
    fn append_unique_and_shared() {
        let mut s = PhpString::from_bytes(b"ab");
        assert!(s.append_in_place(b"cd"));
        assert_eq!(s.as_bytes(), b"abcd");
        let keep = s.clone();
        assert!(!s.append_in_place(b"ef"));
        assert_eq!(keep.as_bytes(), b"abcd");
    }
}
