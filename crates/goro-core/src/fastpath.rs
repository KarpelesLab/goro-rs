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
    #[inline]
    pub(crate) fn of(v: &Value) -> Option<Num> {
        match v {
            Value::Long(n) => Some(Num::L(*n)),
            Value::Double(d) => Some(Num::D(*d)),
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn value(self) -> Value {
        match self {
            Num::L(n) => Value::Long(n),
            Num::D(d) => Value::Double(d),
        }
    }
}

/// Read an int/float operand (through a CV's reference if it has one).
/// Undefined variables, strings, objects... give None.
#[inline]
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
#[inline]
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

/// Store into a temporary result slot whose current content is a plain
/// scalar.  Returns false (and stores nothing) otherwise.
#[inline]
pub(crate) fn store_tmp(result: &OperandType, v: Value, tmps: &mut [Value]) -> bool {
    match result {
        OperandType::Tmp(i) => match tmps.get_mut(*i as usize) {
            Some(slot) if is_plain_scalar(slot) => {
                *slot = v;
                true
            }
            _ => false,
        },
        OperandType::Unused => true,
        _ => false,
    }
}

/// Whether `result` is a slot store_tmp() can write.
#[inline]
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
#[inline]
pub(crate) fn store_num_cv(i: u32, v: Value, cvs: &mut [Value], refs_ok: bool) -> bool {
    match cvs.get_mut(i as usize) {
        Some(Value::Reference(r)) if refs_ok => match r.try_borrow_mut() {
            Ok(mut b) if matches!(*b, Value::Long(_) | Value::Double(_)) => {
                *b = v;
                true
            }
            _ => false,
        },
        Some(slot @ (Value::Long(_) | Value::Double(_))) => {
            *slot = v;
            true
        }
        _ => false,
    }
}

/// Whether CV `i` holds a number that store_num_cv() can overwrite.
#[inline]
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
#[inline]
pub(crate) fn arith(opc: u8, a: Num, b: Num) -> Value {
    let (a, b) = (a.value(), b.value());
    match opc {
        b'+' => a.add(&b),
        b'-' => a.sub(&b),
        _ => a.mul(&b),
    }
}

/// `a < b` (and friends, with swapped operands) on numbers: compare like
/// Value::compare, which the general handlers use.
#[inline]
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

#[inline]
fn bool_val(b: bool) -> Value {
    if b { Value::True } else { Value::False }
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
#[inline]
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
            let OperandType::Cv(ci) = op.op1 else {
                return Fast::No;
            };
            if !cv_num_writable(ci, cvs, refs_ok) || !tmp_writable(&op.result, tmps) {
                return Fast::No;
            }
            let old = num!(op.op1);
            let inc = matches!(op.opcode, OpCode::PreIncrement | OpCode::PostIncrement);
            // php_increment / php_decrement on numbers.
            let new = match (old, inc) {
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
            };
            let res = if matches!(op.opcode, OpCode::PreIncrement | OpCode::PreDecrement) {
                new.clone()
            } else {
                old.value()
            };
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

#[cfg(test)]
mod tests {
    use super::*;

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
