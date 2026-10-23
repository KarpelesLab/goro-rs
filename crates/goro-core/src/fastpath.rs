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

/// An operand that is not an undefined variable (through a CV's
/// reference), as a value (sharing a string or array).
#[inline(always)]
fn defined_operand(
    operand: &OperandType,
    cvs: &[Value],
    tmps: &[Value],
    literals: &[Value],
) -> Option<Value> {
    let v = match operand {
        OperandType::Cv(i) => match cvs.get(*i as usize)? {
            Value::Reference(r) => r.try_borrow().ok()?.clone(),
            v => v.clone(),
        },
        OperandType::Tmp(i) => tmps.get(*i as usize)?.clone(),
        OperandType::Const(i) => literals.get(*i as usize)?.clone(),
        _ => return None,
    };
    (!matches!(v, Value::Undef | Value::Reference(_))).then_some(v)
}

/// i_zend_is_true() of a value whose conversion has no side effect (no
/// object, no NAN, which warns).
#[inline(always)]
fn plain_truthy(v: &Value) -> Option<bool> {
    Some(match v {
        Value::True => true,
        Value::False | Value::Null => false,
        Value::Long(n) => *n != 0,
        Value::Double(d) if !d.is_nan() => *d != 0.0,
        Value::String(s) => {
            let b = s.as_bytes();
            !(b.is_empty() || b == b"0")
        }
        Value::Array(a) => !a.try_borrow().ok()?.is_empty(),
        _ => return None,
    })
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
            let eq = match (
                num_operand(&op.op1, cvs, tmps, literals),
                num_operand(&op.op2, cvs, tmps, literals),
            ) {
                (Some(a), Some(b)) => a.value().equals(&b.value()),
                // Scalars (strings compared as numbers when both are
                // numeric): no conversion with a side effect.
                _ => {
                    let Some(a) = defined_operand(&op.op1, cvs, tmps, literals) else {
                        return Fast::No;
                    };
                    let Some(b) = defined_operand(&op.op2, cvs, tmps, literals) else {
                        return Fast::No;
                    };
                    if !is_plain_scalar(&a) || !is_plain_scalar(&b) {
                        return Fast::No;
                    }
                    a.equals_with_object_cast(&b)
                }
            };
            store!(bool_val(eq == (op.opcode == OpCode::Equal)))
        }
        OpCode::Identical | OpCode::NotIdentical => {
            // `$p === <undef literal>`: the compiled check of whether an
            // optional parameter was passed (no warning).
            if let OperandType::Const(c) = op.op2 {
                if matches!(literals.get(c as usize), Some(Value::Undef)) {
                    let undef = match op.op1 {
                        OperandType::Cv(i) => match cvs.get(i as usize) {
                            Some(Value::Undef) => true,
                            Some(Value::Reference(r)) => match r.try_borrow() {
                                Ok(v) => matches!(*v, Value::Undef),
                                Err(_) => return Fast::No,
                            },
                            Some(_) => false,
                            None => return Fast::No,
                        },
                        _ => return Fast::No,
                    };
                    return store!(bool_val(undef == (op.opcode == OpCode::Identical)));
                }
            }
            // Scalars and objects (arrays are compared element by element,
            // with a recursion guard: the general handler's).
            let Some(a) = defined_operand(&op.op1, cvs, tmps, literals) else {
                return Fast::No;
            };
            let Some(b) = defined_operand(&op.op2, cvs, tmps, literals) else {
                return Fast::No;
            };
            if matches!(a, Value::Array(_)) || matches!(b, Value::Array(_)) {
                return Fast::No;
            }
            let same = match (&a, &b) {
                (Value::Object(x), Value::Object(y)) => Rc::ptr_eq(x, y),
                _ => a.identical(&b),
            };
            store!(bool_val(same == (op.opcode == OpCode::Identical)))
        }
        OpCode::BooleanNot | OpCode::CastBool => {
            let Some(v) = defined_operand(&op.op1, cvs, tmps, literals) else {
                return Fast::No;
            };
            let Some(t) = plain_truthy(&v) else {
                return Fast::No;
            };
            store!(bool_val(t == (op.opcode == OpCode::CastBool)))
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
                Some(Value::Reference(_)) | None => {
                    match defined_operand(&op.op1, cvs, tmps, literals)
                        .as_ref()
                        .and_then(plain_truthy)
                    {
                        Some(t) => t,
                        None => return Fast::No,
                    }
                }
                Some(v) => match plain_truthy(v) {
                    Some(t) => t,
                    None => return Fast::No,
                },
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

/// Whether releasing the last holder of object `o` runs nothing: it is not
/// awaiting its destructor (the destructible list would hold it too), not
/// a WeakMap or a WeakMap key, has no native state or closure payload, no
/// internal markers, and holds only plain values (no object or array whose
/// release could be due). Its handle is then just recycled.
#[inline]
pub(crate) fn object_release_quiet(o: &Rc<RefCell<crate::object::PhpObject>>) -> bool {
    if Rc::strong_count(o) != 1 || Rc::weak_count(o) != 0 {
        return false;
    }
    let Ok(ob) = o.try_borrow() else {
        return false;
    };
    !ob.dtor_listed.get()
        && !ob.weak_key.get()
        && ob.native.is_none()
        && ob.closure.is_none()
        && ob.properties.len() <= 32
        && !ob.class_name.eq_ignore_ascii_case(b"WeakMap")
        && ob
            .properties
            .iter()
            .all(|(k, v)| !k.starts_with(b"__") && is_plain_scalar(v))
}

/// `a . b` of strings and ints (concat_function(); ZEND_CONCAT shares the
/// other string when one is empty, except for a literal string operand).
/// Other operands (an undefined variable, arrays, objects, floats...) take
/// the general handler.
#[inline(never)]
fn fast_concat(op: &Op, cvs: &[Value], tmps: &mut [Value], literals: &[Value]) -> Fast {
    if !tmp_writable(&op.result, tmps) {
        return Fast::No;
    }
    let get = |o: &OperandType| -> Option<Value> {
        let v = match o {
            OperandType::Cv(i) => match cvs.get(*i as usize)? {
                Value::Reference(r) => r.try_borrow().ok()?.clone(),
                v => v.clone(),
            },
            OperandType::Tmp(i) => tmps.get(*i as usize)?.clone(),
            OperandType::Const(i) => literals.get(*i as usize)?.clone(),
            _ => return None,
        };
        matches!(v, Value::String(_) | Value::Long(_)).then_some(v)
    };
    let (Some(a), Some(b)) = (get(&op.op1), get(&op.op2)) else {
        return Fast::No;
    };
    let both_str = matches!(a, Value::String(_)) && matches!(b, Value::String(_));
    let a_lit = both_str && matches!(op.op1, OperandType::Const(_));
    let b_lit = both_str && matches!(op.op2, OperandType::Const(_));
    let s = |v: Value| match v {
        Value::String(s) => s,
        v => v.to_php_string(),
    };
    let (a, b) = (s(a), s(b));
    let result = if a.is_empty() && !a_lit {
        b
    } else if b.is_empty() && !b_lit {
        a
    } else {
        let mut r = Vec::with_capacity(a.len() + b.len());
        r.extend_from_slice(a.as_bytes());
        r.extend_from_slice(b.as_bytes());
        PhpString::from_vec(r)
    };
    store_tmp(&op.result, Value::String(result), tmps);
    Fast::Done
}

/// `$v instanceof C` of a literal class name: a value that is no object is
/// not an instance (without the class being fetched), an object of class
/// `C` itself is. Other cases (a parent class, an interface, an alias,
/// `self` / `parent` / `static`, Closure and Generator checks, an
/// undefined variable) take the general handler.
#[inline(never)]
fn fast_instanceof(op: &Op, cvs: &[Value], tmps: &mut [Value], literals: &[Value]) -> Fast {
    let OperandType::Const(c) = op.op2 else {
        return Fast::No;
    };
    let Some(Value::String(class)) = literals.get(c as usize) else {
        return Fast::No;
    };
    let class = class.as_bytes();
    if class.eq_ignore_ascii_case(b"self")
        || class.eq_ignore_ascii_case(b"parent")
        || class.eq_ignore_ascii_case(b"static")
        || class.eq_ignore_ascii_case(b"closure")
        || !tmp_writable(&op.result, tmps)
    {
        return Fast::No;
    }
    let is = |v: &Value| -> Option<bool> {
        match v {
            Value::Undef | Value::Reference(_) | Value::Generator(_) => None,
            Value::Object(o) => o
                .try_borrow()
                .ok()?
                .class_name
                .eq_ignore_ascii_case(class)
                .then_some(true),
            _ => Some(false),
        }
    };
    let r = match op.op1 {
        OperandType::Cv(i) => match cvs.get(i as usize) {
            Some(Value::Reference(r)) => match r.try_borrow() {
                Ok(b) => is(&b),
                Err(_) => None,
            },
            Some(v) => is(v),
            None => None,
        },
        OperandType::Tmp(i) => tmps.get(i as usize).and_then(is),
        _ => None,
    };
    match r {
        Some(b) if store_tmp(&op.result, bool_val(b), tmps) => Fast::Done,
        _ => Fast::No,
    }
}

/// Whether property name `k` is `name` (short names compared inline, not
/// through a bcmp() call).
#[inline(always)]
fn key_eq(k: &[u8], name: &[u8]) -> bool {
    let n = k.len();
    if n != name.len() {
        return false;
    }
    if (8..=16).contains(&n) {
        let w = |s: &[u8], at: usize| u64::from_ne_bytes(s[at..at + 8].try_into().unwrap());
        return w(k, 0) == w(name, 0) && w(k, n - 8) == w(name, n - 8);
    }
    if (4..8).contains(&n) {
        let w = |s: &[u8], at: usize| u32::from_ne_bytes(s[at..at + 4].try_into().unwrap());
        return w(k, 0) == w(name, 0) && w(k, n - 4) == w(name, n - 4);
    }
    k == name
}

/// The isset() check of a value (`??`, isset($v)): whether it is not null.
/// (An undefined or null variable may name a superglobal not loaded yet:
/// the general handler looks.)
#[inline]
fn fast_isset_check(op: &Op, cvs: &[Value], tmps: &mut [Value], literals: &[Value]) -> Fast {
    let set = match op.op1 {
        OperandType::Cv(i) => match cvs.get(i as usize) {
            Some(Value::Undef | Value::Null) | None => return Fast::No,
            Some(Value::Reference(r)) => match r.try_borrow().as_deref() {
                Ok(Value::Undef | Value::Null) | Err(_) => return Fast::No,
                Ok(_) => true,
            },
            Some(_) => true,
        },
        OperandType::Tmp(i) => match tmps.get(i as usize) {
            Some(Value::Undef | Value::Null) => false,
            Some(Value::Reference(r)) => match r.try_borrow().as_deref() {
                Ok(Value::Undef | Value::Null) => false,
                Ok(_) => true,
                Err(_) => return Fast::No,
            },
            Some(_) => true,
            None => return Fast::No,
        },
        OperandType::Const(i) => match literals.get(i as usize) {
            Some(v) => !matches!(v, Value::Undef | Value::Null),
            None => return Fast::No,
        },
        _ => return Fast::No,
    };
    if store_tmp(&op.result, bool_val(set), tmps) {
        Fast::Done
    } else {
        Fast::No
    }
}

/// FE_FETCH of the next element of an array a foreach loop iterates live
/// (see FeView::live_array): what the general handler does then, but for
/// the end of the loop.
#[inline(never)]
fn fast_foreach_next(op: &Op, tmps: &mut [Value], fe: &mut FeView) -> Fast {
    let OperandType::Tmp(it) = op.op1 else {
        return Fast::No;
    };
    if !matches!(op.result, OperandType::Tmp(_)) || !tmp_writable(&op.result, tmps) {
        return Fast::No;
    }
    let Some(arr) = fe.live_array(it, tmps) else {
        return Fast::No;
    };
    let pos = fe.positions.get(&it).copied().unwrap_or(0);
    let v = {
        let Ok(a) = arr.try_borrow() else {
            return Fast::No;
        };
        match a.entry_at(pos) {
            Some((_, v)) => v.deref(),
            None => return Fast::No,
        }
    };
    match &v {
        Value::Object(o) => crate::gc::track_object(o),
        Value::Generator(g) => crate::gc::track_generator(g),
        _ => {}
    }
    store_tmp(&op.result, v, tmps);
    fe.positions.insert(it, pos + 1);
    Fast::Done
}

/// The key of the element FE_FETCH fetched last from an array a foreach
/// loop iterates live (see fast_foreach_next).
#[inline(never)]
fn fast_foreach_key(op: &Op, tmps: &mut [Value], fe: &mut FeView) -> Fast {
    let OperandType::Tmp(it) = op.op1 else {
        return Fast::No;
    };
    if !matches!(op.result, OperandType::Tmp(_))
        || !tmp_writable(&op.result, tmps)
        || (!fe.cur_key.is_empty() && fe.cur_key.contains_key(&it))
    {
        return Fast::No;
    }
    let Some(arr) = fe.live_array(it, tmps) else {
        return Fast::No;
    };
    let pos = fe
        .positions
        .get(&it)
        .copied()
        .unwrap_or(1)
        .saturating_sub(1);
    let k = {
        let Ok(a) = arr.try_borrow() else {
            return Fast::No;
        };
        match a.entry_at(pos) {
            Some((crate::array::ArrayKey::Int(n), _)) => Value::Long(*n),
            Some((crate::array::ArrayKey::String(s), _)) => Value::String(s.clone()),
            None => return Fast::No,
        }
    };
    store_tmp(&op.result, k, tmps);
    Fast::Done
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

/// plain_public_prop(): a plain slot under its bare name.
const PLAIN: u8 = 1;
/// plain_public_prop(): a plain slot under its bare name when the object
/// holds no NUL-mangled key (see plain_ancestor_private).
const PLAIN_UNMANGLED: u8 = 2;

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
    let ok = |o: OperandType| match o {
        OperandType::Cv(i) => {
            Some(i) != globals && matches!(cvs.get(i as usize), Some(Value::Reference(_)))
        }
        _ => true,
    };
    ok(op.op1) && ok(op.op2) && ok(op.result)
}

/// The temporaries whose foreach iteration state (snapshot, object or
/// iterator handle) a frame holds, for FreeTmps (a few; more count as all).
#[derive(Clone, Copy)]
pub(crate) struct FeTmps {
    n: u8,
    keys: [u32; 6],
}

impl FeTmps {
    pub(crate) fn of<'a>(keys: impl Iterator<Item = &'a u32>) -> FeTmps {
        let mut t = FeTmps { n: 0, keys: [0; 6] };
        for &k in keys {
            if t.n as usize == t.keys.len() {
                t.n = u8::MAX;
                break;
            }
            t.keys[t.n as usize] = k;
            t.n += 1;
        }
        t
    }

    /// Whether one of them is in `a..b` (all are, when there were many).
    #[inline]
    fn any_in(&self, a: u32, b: u32) -> bool {
        self.n == u8::MAX
            || self.keys[..self.n as usize]
                .iter()
                .any(|k| (a..b).contains(k))
    }
}

/// The iteration state of a frame's running foreach loops that
/// run_fast_ops() reads (FreeTmps) and advances (FE_FETCH of an array
/// iterated live): see vm::ForeachState.
pub(crate) struct FeView<'a> {
    pub(crate) positions: &'a mut crate::fxhash::FxHashMap<u32, usize>,
    pub(crate) snap: &'a crate::fxhash::FxHashMap<u32, Vec<(crate::array::ArrayKey, Value)>>,
    pub(crate) obj:
        &'a crate::fxhash::FxHashMap<u32, std::rc::Weak<RefCell<crate::object::PhpObject>>>,
    pub(crate) iter_handles: &'a crate::fxhash::FxHashMap<u32, Value>,
    pub(crate) cur_key: &'a crate::fxhash::FxHashMap<u32, crate::array::ArrayKey>,
    pub(crate) weakmap: &'a crate::fxhash::FxHashMap<u32, Vec<Value>>,
}

impl FeView<'_> {
    /// The temporaries whose foreach state (snapshot, object or iterator
    /// handle) the frame holds.
    fn busy(&self) -> FeTmps {
        FeTmps::of(
            self.snap
                .keys()
                .chain(self.obj.keys())
                .chain(self.iter_handles.keys()),
        )
    }

    /// The array a foreach loop over an array iterates live through its
    /// iterator temporary `it` (no snapshot, object or WeakMap state).
    #[inline]
    fn live_array(&self, it: u32, tmps: &[Value]) -> Option<Rc<RefCell<crate::array::PhpArray>>> {
        let Some(Value::Array(a)) = tmps.get(it as usize) else {
            return None;
        };
        if self.snap.contains_key(&it)
            || (!self.obj.is_empty() && self.obj.contains_key(&it))
            || (!self.weakmap.is_empty() && self.weakmap.contains_key(&it))
        {
            return None;
        }
        Some(a.clone())
    }
}

/// What the arguments of a builtin of SPECIAL_CALLS must be for it to run
/// directly (see Vm::fast_special_call).
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum SpecialArgs {
    /// is_*(mixed $value)
    Any,
    /// count(array)
    Array,
    /// strlen(string)
    Str,
    /// array_key_exists(int|string $key, array $array)
    KeyArray,
}

/// The builtins php-src compiles into opcodes of their own (see
/// internal_frames::compiled_special) that Vm::fast_special_call() runs
/// directly: (lowercase name, arguments, number of arguments).
pub(crate) const SPECIAL_CALLS: &[(&[u8], SpecialArgs, usize)] = &[
    (b"is_null", SpecialArgs::Any, 1),
    (b"is_bool", SpecialArgs::Any, 1),
    (b"is_long", SpecialArgs::Any, 1),
    (b"is_int", SpecialArgs::Any, 1),
    (b"is_integer", SpecialArgs::Any, 1),
    (b"is_float", SpecialArgs::Any, 1),
    (b"is_double", SpecialArgs::Any, 1),
    (b"is_string", SpecialArgs::Any, 1),
    (b"is_array", SpecialArgs::Any, 1),
    (b"is_object", SpecialArgs::Any, 1),
    (b"is_scalar", SpecialArgs::Any, 1),
    (b"count", SpecialArgs::Array, 1),
    (b"sizeof", SpecialArgs::Array, 1),
    (b"strlen", SpecialArgs::Str, 1),
    (b"array_key_exists", SpecialArgs::KeyArray, 2),
];

/// The SPECIAL_CALLS index + 1 of the builtin `lower` (0: not one).
pub(crate) fn special_call_index(lower: &[u8]) -> u8 {
    SPECIAL_CALLS
        .iter()
        .position(|(n, _, _)| *n == lower)
        .map_or(0, |i| i as u8 + 1)
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
            | OpCode::PropertyIsset
            | OpCode::PropertyIssetFetch
            | OpCode::ArrayIsset
            | OpCode::ArrayIssetFetch
            | OpCode::IssetCheck
            | OpCode::ArrayGet
            | OpCode::ArraySet
            | OpCode::ArrayAppend
            | OpCode::Jmp
            | OpCode::FreeTmps
            | OpCode::InitFCall
            | OpCode::SendVal
            | OpCode::SendValChecked
            | OpCode::StaticPropGet
            | OpCode::BooleanNot
            | OpCode::CastBool
            | OpCode::ErrorSuppress
            | OpCode::ErrorRestore
            | OpCode::Nop
            | OpCode::ForeachNext
            | OpCode::ForeachKey
            | OpCode::JmpIfArgByRef
            | OpCode::InitMethodCall
            | OpCode::Concat
            | OpCode::TypeCheck
            | OpCode::ForeachInit
            | OpCode::NewObject
            | OpCode::BindGlobal
    )
}

impl crate::vm::Vm {
    /// Run the opcodes from `ip` on while fast_op(), fast_dim_op() and
    /// fast_prop_op() handle them, as the dispatch loop would one at a time
    /// (they only touch plain values: nothing the loop checks between two
    /// opcodes changes, but for memory_limit and max_execution_time, which
    /// hand control back to it). Returns the index of the first opcode not
    /// handled (or of the next one to run when the loop has to look).
    /// `refs_ok` / `no_release`: see fast_op(); `fe`: the frame's foreach
    /// loops (see FreeTmps, ForeachNext); `fe_quiet`: no foreach
    /// iterator handle or by-reference loop pins a temporary (see
    /// fast_assign()); `bound`: the frame's
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
        fe: &mut FeView,
        fe_quiet: bool,
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
                    // (a builtin php-src compiles into an opcode of its own)
                    match self.fast_special_call(op_array, ip, cvs, tmps, no_release) {
                        Some(next) => Fast::Jump(next),
                        None if self.init_fcall_fast(op_array, ip, cvs) => Fast::Done,
                        None => break 'run ip,
                    }
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
                // A non-variable argument of a user function's by-value
                // parameter (SEND_VAL_EX / SEND_VAR_NO_REF_EX): sent as is.
                OpCode::SendValChecked => {
                    // (or of an internal function's by-value parameter, for
                    // a value that is not an array: the general handler may
                    // separate one passed to an `array &$array`)
                    let fn_gen = self.user_functions.len() + 1;
                    let by_value_target = self.pending_calls.last().is_some_and(|c| {
                        c.target.fn_gen == fn_gen
                            && match (&c.target.user, &c.target.builtin) {
                                (Some(_), _) => true,
                                (None, Some((_, Some((fi, _))))) => {
                                    let idx = c.args.len();
                                    fi.params
                                        .get(idx)
                                        .or_else(|| fi.params.last().filter(|p| p.variadic))
                                        .is_none_or(|p| p.by_ref == 0)
                                        && !matches!(
                                            defined_operand(&op.op1, cvs, tmps, literals),
                                            Some(Value::Array(_)) | None
                                        )
                                }
                                _ => false,
                            }
                    });
                    if by_value_target && self.send_val_fast(op, cvs, tmps, literals) {
                        Fast::Done
                    } else {
                        break 'run ip;
                    }
                }
                // (the read of `$o->p++` is one of an object's property)
                OpCode::PropertyGet
                | OpCode::PropertySet
                | OpCode::PropertyGetW
                | OpCode::PropertyGetIncDec
                | OpCode::PropertyIsset
                | OpCode::PropertyIssetFetch => {
                    if self.isset_get_cache.is_some() {
                        break 'run ip;
                    }
                    self.fast_prop_op(op_array, ip, cvs, tmps)
                }
                OpCode::ArrayGet | OpCode::ArraySet | OpCode::ArrayAppend => {
                    self.fast_dim_op(op, cvs, tmps, literals)
                }
                // (on an array, FETCH_DIM_IS's isset answers as ISSET_ISEMPTY_DIM's)
                OpCode::ArrayIsset | OpCode::ArrayIssetFetch => {
                    self.fast_dim_isset(op, cvs, tmps, literals)
                }
                OpCode::IssetCheck => fast_isset_check(op, cvs, tmps, literals),
                // A class constant its site resolved (see fast_class_const).
                OpCode::StaticPropGet => {
                    if !tmp_writable(&op.result, tmps) {
                        break 'run ip;
                    }
                    match self.fast_class_const(op_array, ip) {
                        Some(v) => {
                            if let Value::Object(o) = &v {
                                crate::gc::track_object(o);
                            }
                            if store_tmp(&op.result, v, tmps) {
                                Fast::Done
                            } else {
                                Fast::No
                            }
                        }
                        None => Fast::No,
                    }
                }
                OpCode::Jmp => match op.op1 {
                    OperandType::JmpTarget(t) => Fast::Jump(t as usize),
                    _ => return ip,
                },
                // A statement's temporaries that hold plain values, with no
                // foreach state and nothing to destroy: the general handler
                // just empties them.
                OpCode::FreeTmps if no_release => {
                    let (OperandType::Tmp(a), OperandType::Tmp(b)) = (op.op1, op.op2) else {
                        break 'run ip;
                    };
                    // (no running foreach's state is held by these)
                    if fe.busy().any_in(a, b) {
                        break 'run ip;
                    }
                    let end = (b as usize).min(tmps.len());
                    let range = &mut tmps[(a as usize).min(end)..end];
                    // An array or object held elsewhere too only loses a
                    // holder; one held here alone is freed when it holds a
                    // few plain values (nothing to destroy). (No object
                    // with a destructor exists: `no_release`; the cycle
                    // collector is not due.)
                    let releasable = |v: &Value| match v {
                        Value::Array(a) => {
                            Rc::strong_count(a) > 1
                                || a.try_borrow()
                                    .is_ok_and(|a| a.len() <= 16 && a.values().all(is_plain_scalar))
                        }
                        Value::Object(o) => Rc::strong_count(o) > 1 || object_release_quiet(o),
                        Value::Reference(_) | Value::Resource(_) | Value::Generator(_) => false,
                        _ => true,
                    };
                    if !range.iter().all(releasable)
                        || (range
                            .iter()
                            .any(|v| matches!(v, Value::Array(_) | Value::Object(_)))
                            && crate::gc::gc_pending())
                    {
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
                OpCode::Assign => match fast_op(op, cvs, tmps, literals, refs_ok, no_release) {
                    Fast::No => {
                        self.fast_assign(op, cvs, tmps, literals, refs_ok, no_release, fe_quiet)
                    }
                    f => f,
                },
                OpCode::QmAssign => match fast_op(op, cvs, tmps, literals, refs_ok, no_release) {
                    Fast::No => self.fast_qm_assign_container(op, cvs, tmps),
                    f => f,
                },
                OpCode::AssignConcat => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::BooleanNot => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::CastBool => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                // (the silence of isset() / empty() / `??` / `@`)
                OpCode::ErrorSuppress => {
                    self.error_reporting_stack.push(self.error_reporting);
                    self.error_reporting = crate::errdisplay::begin_silence(
                        self.error_reporting,
                        matches!(op.op1, OperandType::Const(_)),
                    );
                    Fast::Done
                }
                OpCode::ErrorRestore => {
                    if let Some(saved) = self.error_reporting_stack.pop() {
                        self.error_reporting =
                            crate::errdisplay::end_silence(self.error_reporting, saved);
                    }
                    Fast::Done
                }
                OpCode::Nop => Fast::Done,
                OpCode::ForeachNext => fast_foreach_next(op, tmps, fe),
                OpCode::ForeachKey => fast_foreach_key(op, tmps, fe),
                // A public method of a user class by a constant name (what
                // the INIT_METHOD_CALL arm does first)
                OpCode::InitMethodCall => {
                    if matches!(op.op2, OperandType::Const(_))
                        && self.init_method_call_fast(op_array, ip, cvs, tmps)
                    {
                        Fast::Done
                    } else {
                        break 'run ip;
                    }
                }
                OpCode::Concat => fast_concat(op, cvs, tmps, literals),
                OpCode::TypeCheck => fast_instanceof(op, cvs, tmps, literals),
                OpCode::ForeachInit => self.fast_foreach_init(op, cvs, tmps, literals, fe),
                OpCode::NewObject => match self.fast_new_object(op_array, ip, tmps) {
                    Some(next) => Fast::Jump(next),
                    None => break 'run ip,
                },
                // (a frame bound to a symbol table rebinds itself: the general handler)
                OpCode::BindGlobal if bound.is_none() => self.fast_bind_global(op, cvs, literals),
                // (whether the next argument of the call is sent by reference)
                OpCode::JmpIfArgByRef => match (self.fast_arg_by_ref(op), op.op1) {
                    (Some(true), OperandType::JmpTarget(t)) => Fast::Jump(t as usize),
                    (Some(false), _) => Fast::Done,
                    _ => break 'run ip,
                },
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

    /// INIT_FCALL / SEND_VAL... / DO_FCALL at `ip` of a call of a builtin
    /// php-src compiles into an opcode of its own (ZEND_TYPE_CHECK,
    /// ZEND_COUNT, ZEND_STRLEN, ZEND_ARRAY_KEY_EXISTS...: see
    /// internal_frames::compiled_special, such a call has no frame), whose
    /// arguments are plain values its parameters take as they are (no
    /// argument check or coercion happens): the builtin the call site
    /// resolved to runs at once, with what DO_FCALL does around it for such
    /// a call. Returns the index of the opcode after the DO_FCALL, or None
    /// (nothing done). Only when no destructor is due (the call's release of
    /// its arguments is then a no-op).
    #[inline(never)]
    fn fast_special_call(
        &mut self,
        op_array: &crate::opcode::OpArray,
        ip: usize,
        cvs: &[Value],
        tmps: &mut [Value],
        no_release: bool,
    ) -> Option<usize> {
        if !no_release {
            return None;
        }
        let ops = &op_array.ops;
        let literals = &op_array.literals;
        let op = &ops[ip];
        let name = crate::vm::plain_fcall_name(op, literals)?;
        // (a name of SPECIAL_CALLS at all: is_*, count, sizeof, strlen,
        // array_key_exists; other calls are not looked up again)
        let nb = name.as_bytes();
        let maybe = match nb.first().map(u8::to_ascii_lowercase) {
            Some(b'i') => nb.len() >= 6 && nb[..3].eq_ignore_ascii_case(b"is_"),
            Some(b'c') => nb.len() == 5,
            Some(b's') => nb.len() == 6,
            Some(b'a') => nb.len() == 16,
            _ => false,
        };
        if !maybe {
            return None;
        }
        let fn_gen = self.user_functions.len() + 1;
        let (func, special) = Self::cached_special_call(op_array, ip, name.as_bytes(), fn_gen)?;
        let &(lower, kind, nargs) = SPECIAL_CALLS.get(special as usize - 1)?;
        // INIT_FCALL's op2 is the number of arguments; the sends follow it.
        if !matches!(op.op2, OperandType::Const(c)
            if matches!(literals.get(c as usize), Some(Value::Long(n)) if *n == nargs as i64))
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
        // The arguments, as the sends would pass them.
        let mut args: [Value; 2] = [Value::Null, Value::Null];
        for (i, s) in sends.iter().enumerate() {
            let plain_send = match s.opcode {
                OpCode::SendVal => matches!(s.result, OperandType::Unused),
                OpCode::SendValChecked => true,
                _ => false,
            };
            if !plain_send {
                return None;
            }
            let v = match s.op1 {
                OperandType::Cv(c) => match cvs.get(c as usize)? {
                    Value::Undef => return None,
                    Value::Reference(r) => match &*r.try_borrow().ok()? {
                        Value::Undef | Value::Reference(_) => return None,
                        v => v.clone(),
                    },
                    v => v.clone(),
                },
                OperandType::Const(c) => literals.get(c as usize)?.clone(),
                // (a temporary the send releases: one that holds a plain
                // value, or a container held elsewhere too)
                OperandType::Tmp(c) => match tmps.get(c as usize)? {
                    Value::Undef | Value::Reference(_) => return None,
                    v if is_plain_scalar(v) => v.clone(),
                    v @ Value::Array(a) if Rc::strong_count(a) > 1 => v.clone(),
                    v @ Value::Object(o) if Rc::strong_count(o) > 1 => v.clone(),
                    _ => return None,
                },
                _ => return None,
            };
            if matches!(&v, Value::Array(a) if Rc::ptr_eq(a, &self.globals)) {
                return None;
            }
            args[i] = v;
        }
        let ok = match kind {
            SpecialArgs::Any => true,
            SpecialArgs::Array => matches!(args[0], Value::Array(_)),
            SpecialArgs::Str => matches!(args[0], Value::String(_)),
            SpecialArgs::KeyArray => {
                matches!(args[0], Value::Long(_) | Value::String(_))
                    && matches!(args[1], Value::Array(_))
            }
        };
        if !ok {
            return None;
        }
        // (what DO_FCALL does for the call)
        self.last_call_by_ref = false;
        self.note_call_result(fcall, lower);
        self.caller_strict_types = op_array.strict_types;
        self.builtin_named_skipped = Vec::new();
        let r = func(self, &args[..nargs]);
        self.ucf_call_line = None;
        let result = match r {
            Ok(v) if self.current_exception.is_none() => v,
            // (cannot happen with such arguments)
            _ => return None,
        };
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

    /// JMP_IF_ARG_BY_REF of a positional argument of a call set up with its
    /// user function resolved (a plain function name, or a method of a user
    /// class INIT_METHOD_CALL's fast path resolved): whether the function
    /// takes that argument by reference, as the general handler finds it
    /// (no internal method or dynamic builtin call is involved). None: the
    /// general handler decides.
    #[inline]
    fn fast_arg_by_ref(&self, op: &Op) -> Option<bool> {
        if matches!(op.op2, OperandType::Const(_)) {
            return None;
        }
        let call = self.pending_calls.last()?;
        let t = &call.target;
        if t.fn_gen != self.user_functions.len() + 1 || t.dynamic {
            return None;
        }
        let Some(f) = t.user.as_ref() else {
            // A builtin by a plain name (not a method, not a dynamic call):
            // the general handler finds no by-reference parameter there.
            let name = call.name.as_bytes();
            return (t.builtin.is_some() && !name.windows(2).any(|w| w == b"::")).then_some(false);
        };
        if t.method.is_some() {
            t.method_info.as_ref()?;
        } else if call.name.as_bytes().windows(2).any(|w| w == b"::") {
            return None;
        }
        let idx = call.args.len();
        Some(match f.by_ref_params.get(idx) {
            Some(b) => *b,
            None => f.variadic_param.is_some() && f.by_ref_params.last().copied().unwrap_or(false),
        })
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

    /// The assignments fast_op() leaves: `$cv = <array|object>` (a
    /// variable's or a literal's array is copied, a temporary's moved, as
    /// write_operand() stores them; objects are registered with the cycle
    /// collector), assignments to a temporary (short-circuit results, the
    /// release of a temporary), and assignments over an array or object
    /// whose release destroys nothing (held elsewhere too, or a small
    /// array of plain values), when no destructor can be due. `fe_quiet`:
    /// no foreach iterator handle or by-reference loop pins a temporary.
    #[allow(clippy::too_many_arguments)]
    fn fast_assign(
        &self,
        op: &Op,
        cvs: &mut [Value],
        tmps: &mut [Value],
        literals: &[Value],
        refs_ok: bool,
        no_release: bool,
        fe_quiet: bool,
    ) -> Fast {
        // (the general handler is a release point: it is only bypassed when
        // no destructor can be due)
        if !no_release || !tmp_writable(&op.result, tmps) {
            return Fast::No;
        }
        // (an overwritten array or object loses this holder)
        let releasable = |v: &Value| match v {
            Value::Array(a) => {
                !Rc::ptr_eq(a, &self.globals)
                    && (Rc::strong_count(a) > 1
                        || a.try_borrow()
                            .is_ok_and(|a| a.len() <= 64 && a.values().all(is_plain_scalar)))
            }
            Value::Object(o) => Rc::strong_count(o) > 1 || object_release_quiet(o),
            v => is_plain_scalar(v),
        };
        let to_tmp = match op.op1 {
            OperandType::Cv(ci) => {
                let ok = match cvs.get(ci as usize) {
                    Some(Value::Reference(r)) => {
                        refs_ok && r.try_borrow_mut().is_ok_and(|b| releasable(&b))
                    }
                    Some(v) => releasable(v),
                    None => false,
                };
                if !ok {
                    return Fast::No;
                }
                None
            }
            // (a temporary holding a plain value, not an iterator's)
            OperandType::Tmp(ti) if fe_quiet => match tmps.get(ti as usize) {
                Some(v) if is_plain_scalar(v) => Some(ti as usize),
                _ => return Fast::No,
            },
            _ => return Fast::No,
        };
        let val = match op.op2 {
            OperandType::Tmp(i) => match tmps.get(i as usize) {
                Some(Value::Object(_)) | Some(Value::Array(_)) => {
                    std::mem::replace(&mut tmps[i as usize], Value::Undef)
                }
                Some(v)
                    if is_plain_scalar(v) && (to_tmp.is_some() || !matches!(v, Value::Undef)) =>
                {
                    std::mem::replace(&mut tmps[i as usize], Value::Undef)
                }
                _ => return Fast::No,
            },
            OperandType::Cv(i) => match cvs.get(i as usize) {
                Some(v @ (Value::Object(_) | Value::Array(_))) => v.clone(),
                Some(v) if is_plain_scalar(v) && !matches!(v, Value::Undef) => v.clone(),
                _ => return Fast::No,
            },
            OperandType::Const(i) => match literals.get(i as usize) {
                // (an Undef literal releases a temporary)
                Some(v)
                    if is_plain_scalar(v) && (to_tmp.is_some() || !matches!(v, Value::Undef)) =>
                {
                    v.clone()
                }
                Some(v @ Value::Array(_)) => v.clone(),
                _ => return Fast::No,
            },
            _ => return Fast::No,
        };
        let val = match val {
            Value::Array(a) if Rc::ptr_eq(&a, &self.globals) => {
                // ($GLOBALS: the general handler's copy)
                if let OperandType::Tmp(i) = op.op2 {
                    tmps[i as usize] = Value::Array(a);
                }
                return Fast::No;
            }
            // (a temporary's array is moved, also into a temporary)
            Value::Array(a) if to_tmp.is_some() || Rc::strong_count(&a) == 1 => Value::Array(a),
            v @ Value::Array(_) => self.dup_array_value(v),
            Value::Object(o) => {
                crate::gc::track_object(&o);
                Value::Object(o)
            }
            v => v,
        };
        let result = matches!(op.result, OperandType::Tmp(_)).then(|| val.clone());
        if let Some(ti) = to_tmp {
            tmps[ti] = val;
        } else if let OperandType::Cv(ci) = op.op1 {
            let old = match &mut cvs[ci as usize] {
                Value::Reference(r) => {
                    let Ok(mut b) = r.try_borrow_mut() else {
                        return Fast::No;
                    };
                    let old = std::mem::replace(&mut *b, val);
                    // (as write_operand(): the overwritten value loses a holder)
                    crate::gc::on_value_replaced(&old, &b);
                    old
                }
                slot => {
                    if matches!(slot, Value::Object(_) | Value::Array(_)) {
                        crate::gc::possible_root(slot);
                    }
                    std::mem::replace(slot, val)
                }
            };
            drop(old);
        }
        if let Some(v) = result {
            store_tmp(&op.result, v, tmps);
        }
        Fast::Done
    }

    /// FE_RESET_R of an array (not the symbol table): what the general
    /// handler does then, but for an array holding references (it takes a
    /// snapshot of those). A temporary array nobody else holds is iterated
    /// live; any other array through a copy-on-write copy of it.
    #[inline(never)]
    fn fast_foreach_init(
        &self,
        op: &Op,
        cvs: &[Value],
        tmps: &mut [Value],
        literals: &[Value],
        fe: &mut FeView,
    ) -> Fast {
        let OperandType::Tmp(it) = op.result else {
            return Fast::No;
        };
        if !tmp_writable(&op.result, tmps)
            || fe.snap.contains_key(&it)
            || (!fe.obj.is_empty() && fe.obj.contains_key(&it))
            || (!fe.cur_key.is_empty() && fe.cur_key.contains_key(&it))
            || (!fe.iter_handles.is_empty() && fe.iter_handles.contains_key(&it))
            || (!fe.weakmap.is_empty() && fe.weakmap.contains_key(&it))
        {
            return Fast::No;
        }
        let arr = match op.op1 {
            OperandType::Cv(i) => match cvs.get(i as usize) {
                Some(Value::Array(a)) => a.clone(),
                Some(Value::Reference(r)) => match r.try_borrow().as_deref() {
                    Ok(Value::Array(a)) => a.clone(),
                    _ => return Fast::No,
                },
                _ => return Fast::No,
            },
            OperandType::Tmp(i) => match tmps.get(i as usize) {
                Some(Value::Array(a)) => a.clone(),
                _ => return Fast::No,
            },
            OperandType::Const(i) => match literals.get(i as usize) {
                Some(Value::Array(a)) => a.clone(),
                _ => return Fast::No,
            },
            _ => return Fast::No,
        };
        if Rc::ptr_eq(&arr, &self.globals) {
            return Fast::No;
        }
        // (holders: the source operand and `arr`; the iterator makes one more)
        let unshared = matches!(op.op1, OperandType::Tmp(_) | OperandType::Const(_))
            && Rc::strong_count(&arr) <= 2;
        let iter = if unshared {
            arr
        } else {
            let view = match arr.try_borrow_mut() {
                Ok(mut a) => a.cow_clone(),
                Err(_) => return Fast::No,
            };
            if view.shared_store().is_none() {
                return Fast::No;
            }
            Rc::new(RefCell::new(view))
        };
        store_tmp(&op.result, Value::Array(iter), tmps);
        fe.positions.insert(it, 0);
        Fast::Done
    }

    /// NEW of a user class without a destructor from the template its site
    /// used last (see new_site_template), skipping the constructor call of a
    /// class without one (see new_skips_ctor_call). The index of the next
    /// opcode, or None (nothing done).
    #[inline(never)]
    fn fast_new_object(
        &mut self,
        op_array: &crate::opcode::OpArray,
        ip: usize,
        tmps: &mut [Value],
    ) -> Option<usize> {
        let op = &op_array.ops[ip];
        // (an anonymous class is bound first: the general handler)
        match op.op1 {
            OperandType::Const(c) => match op_array.literals.get(c as usize) {
                Some(Value::String(n)) if !n.as_bytes().contains(&0) => {}
                _ => return None,
            },
            _ => return None,
        }
        if !matches!(op.result, OperandType::Tmp(_)) || !tmp_writable(&op.result, tmps) {
            return None;
        }
        // (an object with a destructor is listed for it: the release points
        // the run skips would then matter)
        let t = self
            .new_site_template(op_array, ip)
            .filter(|t| !t.has_destruct)?;
        let no_ctor = t.no_ctor;
        let obj = self.new_from_template(&t, op, tmps);
        // (the DO_FCALL's result slot is checked before the skip changes anything)
        let skip = no_ctor
            && op_array
                .ops
                .get(ip + 2)
                .is_some_and(|f| tmp_writable(&f.result, tmps))
            && self.new_skips_ctor_call(op_array, ip + 1, &obj);
        // (write_operand() registers the object with the cycle collector)
        if let Value::Object(o) = &obj {
            crate::gc::track_object(o);
        }
        store_tmp(&op.result, obj, tmps);
        if skip {
            // (what DO_FCALL of the missing constructor leaves: a NULL result)
            let fcall = &op_array.ops[ip + 2];
            if let OperandType::Tmp(t) = fcall.result {
                if let Some(slot) = tmps.get_mut(t as usize) {
                    *slot = Value::Null;
                }
            }
            return Some(ip + 3);
        }
        Some(ip + 1)
    }

    /// `global $name` in a frame not bound to a symbol table: the variable
    /// becomes a reference to the global's cell (created as null when
    /// missing), as the general handler binds it, when what it held is a
    /// plain value (releasing it does nothing).
    #[inline(never)]
    fn fast_bind_global(&mut self, op: &Op, cvs: &mut [Value], literals: &[Value]) -> Fast {
        let (OperandType::Cv(ci), OperandType::Const(n)) = (op.op1, op.op2) else {
            return Fast::No;
        };
        let Some(Value::String(name)) = literals.get(n as usize) else {
            return Fast::No;
        };
        if !cvs.get(ci as usize).is_some_and(is_plain_scalar) {
            return Fast::No;
        }
        let cell = self.global_cell(name.as_bytes());
        set_plain(&mut cvs[ci as usize], Value::Reference(cell));
        Fast::Done
    }

    /// QM_ASSIGN of a variable holding an array or an object into a
    /// temporary (the container of `$a[k] ?? d`, `empty($a[k])`...): the
    /// general handler's copy of the value (a variable's array is shared
    /// with the temporary, as read_operand() makes it).
    fn fast_qm_assign_container(&self, op: &Op, cvs: &[Value], tmps: &mut [Value]) -> Fast {
        if !matches!(op.op2, OperandType::Unused) || !tmp_writable(&op.result, tmps) {
            return Fast::No;
        }
        let OperandType::Cv(i) = op.op1 else {
            return Fast::No;
        };
        let v = match cvs.get(i as usize) {
            Some(Value::Reference(r)) => match r.try_borrow() {
                Ok(b) => b.clone(),
                Err(_) => return Fast::No,
            },
            Some(v) => v.clone(),
            None => return Fast::No,
        };
        match &v {
            Value::Array(a) if !Rc::ptr_eq(a, &self.globals) => {}
            Value::Object(o) => crate::gc::track_object(o),
            _ => return Fast::No,
        }
        store_tmp(&op.result, v, tmps);
        Fast::Done
    }

    /// `isset($a[k])` of a real array with an int / string key (the general
    /// handler's array case: the element exists and is not null).
    pub(crate) fn fast_dim_isset(
        &self,
        op: &Op,
        cvs: &[Value],
        tmps: &mut [Value],
        literals: &[Value],
    ) -> Fast {
        let arr = match op.op1 {
            OperandType::Cv(i) => match cvs.get(i as usize) {
                Some(Value::Array(a)) => a.clone(),
                Some(Value::Reference(r)) => match r.try_borrow().as_deref() {
                    Ok(Value::Array(a)) => a.clone(),
                    _ => return Fast::No,
                },
                _ => return Fast::No,
            },
            OperandType::Tmp(i) => match tmps.get(i as usize) {
                Some(Value::Array(a)) => a.clone(),
                _ => return Fast::No,
            },
            _ => return Fast::No,
        };
        if Rc::ptr_eq(&arr, &self.globals) || !tmp_writable(&op.result, tmps) {
            return Fast::No;
        }
        let Some(key) = dim_key(&op.op2, cvs, tmps, literals) else {
            return Fast::No;
        };
        let set = {
            let Ok(a) = arr.try_borrow() else {
                return Fast::No;
            };
            match a.get(&key) {
                Some(Value::Reference(r)) => match r.try_borrow() {
                    Ok(v) => !matches!(*v, Value::Null | Value::Undef),
                    Err(_) => return Fast::No,
                },
                Some(v) => !matches!(v, Value::Null | Value::Undef),
                None => false,
            }
        };
        if store_tmp(
            &op.result,
            if set { Value::True } else { Value::False },
            tmps,
        ) {
            Fast::Done
        } else {
            Fast::No
        }
    }

    /// `$a[k]` read, `$cv[k] = v` write and `$cv[] = v` append (v a scalar,
    /// an array or an object) on a real array with an int / string key,
    /// where the element (for a read) exists or (for a write) is absent or a
    /// plain scalar that is not a reference.  The
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
        // (the value is an array of a variable or literal to copy: once the
        // write is known to be done here)
        let mut dup = false;
        let val = if write {
            let val = match op.op2 {
                OperandType::Tmp(i) => tmps.get(i as usize),
                OperandType::Cv(i) => cvs.get(i as usize),
                OperandType::Const(i) => literals.get(i as usize),
                _ => None,
            };
            // An array or object value, stored as the general handlers store it
            // (an array of a variable or literal is copied, copy-on-write; a
            // temporary's is shared with it until it is freed): not the
            // container itself (`$a[] = $a`, a reference to it), nor into a
            // container held elsewhere too (see unshare_container).
            let container_ok = |a: &Rc<RefCell<crate::array::PhpArray>>| {
                !Rc::ptr_eq(a, &arr) && !Rc::ptr_eq(a, &self.globals) && Rc::strong_count(&arr) <= 2
            };
            match val {
                Some(v) if is_plain_scalar(v) && !matches!(v, Value::Undef) => Some(v.clone()),
                // (a variable bound to a reference, as in the global scope:
                // its value is stored)
                Some(Value::Reference(r)) if matches!(op.op2, OperandType::Cv(_)) => {
                    match r.try_borrow().as_deref() {
                        Ok(v) if is_plain_scalar(v) && !matches!(v, Value::Undef) => {
                            Some(v.clone())
                        }
                        Ok(v @ Value::Array(a)) if container_ok(a) => {
                            dup = true;
                            Some(v.clone())
                        }
                        Ok(Value::Object(o)) => Some(Value::Object(o.clone())),
                        _ => return Fast::No,
                    }
                }
                Some(v @ Value::Array(a)) if container_ok(a) => {
                    dup = !matches!(op.op2, OperandType::Tmp(_));
                    Some(v.clone())
                }
                Some(Value::Object(o)) => Some(Value::Object(o.clone())),
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
            let val = val.unwrap_or_default();
            let val = if dup {
                drop(a);
                let v = self.dup_array_value(val);
                match arr.try_borrow_mut() {
                    Ok(b) => {
                        a = b;
                        v
                    }
                    Err(_) => return Fast::No,
                }
            } else {
                val
            };
            return match a.try_push(val) {
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
            let val = if dup {
                drop(a);
                let v = self.dup_array_value(val);
                match arr.try_borrow_mut() {
                    Ok(b) => {
                        a = b;
                        v
                    }
                    Err(_) => return Fast::No,
                }
            } else {
                val
            };
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
    ) -> u8 {
        // (a long name: an anonymous class's)
        let mut buf = [0u8; 128];
        let long;
        let lower = match lower_into(class_name, &mut buf) {
            Some(l) => l,
            None => {
                long = class_name.to_ascii_lowercase();
                &long
            }
        };
        let Some(ce) = self.classes.get(lower) else {
            // stdClass: only dynamic properties, no handlers of any kind.
            return PLAIN * u8::from(lower == b"stdclass");
        };
        if self.plain_declared_prop(ce, class_name, lower, prop, write, scope) {
            PLAIN
        } else {
            self.plain_ancestor_private(ce, class_name, prop, write, scope)
        }
    }

    /// plain_public_prop() of a class in the class table.
    fn plain_declared_prop(
        &self,
        ce: &crate::object::ClassEntry,
        class_name: &[u8],
        lower: &[u8],
        prop: &[u8],
        write: bool,
        scope: Option<&[u8]>,
    ) -> bool {
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

    /// A private property declared by an ancestor of the object's class,
    /// read or written from that ancestor's methods: it is stored under its
    /// bare name unless a same-named property of another scope moved it
    /// away (which leaves a NUL-mangled key in the object): PLAIN_UNMANGLED
    /// when it is otherwise a plain slot.
    fn plain_ancestor_private(
        &self,
        ce: &crate::object::ClassEntry,
        class_name: &[u8],
        prop: &[u8],
        write: bool,
        scope: Option<&[u8]>,
    ) -> u8 {
        let Some(scope) = scope else {
            return 0;
        };
        let mut decl = None;
        for p in &ce.properties {
            if p.name == prop {
                if decl.is_some() {
                    return 0;
                }
                decl = Some(p);
            }
        }
        let Some(p) = decl else {
            return 0;
        };
        if p.visibility != crate::object::Visibility::Private
            || p.declaring_class.eq_ignore_ascii_case(class_name)
            || !scope.eq_ignore_ascii_case(&p.declaring_class)
            || p.is_static
            || p.has_get_hook
            || p.has_set_hook
            || p.is_virtual
            || (write && (p.is_readonly || p.property_type.is_some() || p.set_visibility.is_some()))
        {
            return 0;
        }
        // The declaring class (with its ancestors) and the classes down to
        // the object's are plain user classes.
        let dl = p.declaring_class.to_ascii_lowercase();
        let dl = dl.as_slice();
        let Some(dce) = self.classes.get(dl) else {
            return 0;
        };
        if dce.is_readonly && write {
            return 0;
        }
        if !self.plain_declared_prop(dce, &p.declaring_class, dl, prop, write, Some(scope)) {
            return 0;
        }
        let mut cur = Some(ce);
        for _ in 0..64 {
            let Some(c) = cur else {
                return 0;
            };
            if c.name.eq_ignore_ascii_case(&p.declaring_class) {
                return PLAIN_UNMANGLED;
            }
            let cl = c.name.to_ascii_lowercase();
            let cl = cl.as_slice();
            if c.filename.is_none()
                || c.is_enum
                || (write && c.is_readonly)
                || self.native_class_handlers.contains_key(cl)
            {
                return 0;
            }
            let mut hbuf = [0u8; 160];
            let prefix: &[u8] = if write {
                b"__property_set_"
            } else {
                b"__property_get_"
            };
            let Some(h) = hbuf.get_mut(..prefix.len() + prop.len()) else {
                return 0;
            };
            h[..prefix.len()].copy_from_slice(prefix);
            for (o, ch) in h[prefix.len()..].iter_mut().zip(prop) {
                *o = ch.to_ascii_lowercase();
            }
            if c.methods.contains_key(&*h) {
                return 0;
            }
            cur = c
                .parent
                .as_deref()
                .and_then(|pn| self.classes.get(pn.to_ascii_lowercase().as_slice()));
        }
        0
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
        // (the fetch of `isset($o->p[k])` / `isset($o->p->q)` answers the
        // same for a plain slot)
        let isset = matches!(
            op.opcode,
            OpCode::PropertyIsset | OpCode::PropertyIssetFetch
        );
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
            OperandType::Tmp(ti)
                if matches!(
                    op.opcode,
                    OpCode::PropertyGet | OpCode::PropertyIsset | OpCode::PropertyIssetFetch
                ) =>
            {
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
        // (a dynamic name, `$o->$p`: the site does not remember the answer)
        let dyn_name;
        let name = match name_op {
            OperandType::Const(i) => match literals.get(*i as usize) {
                Some(Value::String(s)) => s.as_bytes(),
                _ => return Fast::No,
            },
            OperandType::Cv(i) => {
                dyn_name = match cvs.get(*i as usize) {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Reference(r)) => match r.try_borrow().as_deref() {
                        Ok(Value::String(s)) => s.clone(),
                        _ => return Fast::No,
                    },
                    _ => return Fast::No,
                };
                dyn_name.as_bytes()
            }
            OperandType::Tmp(i) => match tmps.get(*i as usize) {
                Some(Value::String(s)) => {
                    dyn_name = s.clone();
                    dyn_name.as_bytes()
                }
                _ => return Fast::No,
            },
            _ => return Fast::No,
        };
        let const_name = matches!(name_op, OperandType::Const(_));
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
                // An object, or an array (a variable's or a literal's is
                // copied, as the general handler's dup_stored_array()).
                Some(Value::Object(o)) => Some(Value::Object(o.clone())),
                Some(v @ Value::Array(a)) if !Rc::ptr_eq(a, &self.globals) => {
                    if matches!(op.op2, OperandType::Tmp(_)) {
                        Some(v.clone())
                    } else {
                        Some(self.dup_array_value(v.clone()))
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
        let mut mangled = false;
        for (i, (k, v)) in ob.properties.iter().enumerate() {
            if k.starts_with(b"__") {
                return Fast::No;
            }
            mangled |= k.first() == Some(&0);
            if key_eq(k, name) {
                slot = Some((i, v));
            }
        }
        let Some((idx, cur)) = slot else {
            // isset() of a property a stdClass object does not have (it has
            // no magic methods).
            if isset && ob.class_name == b"stdClass" {
                drop(ob);
                return if store_tmp(&op.result, Value::False, tmps) {
                    Fast::Done
                } else {
                    Fast::No
                };
            }
            return Fast::No;
        };
        // An uninitialized slot, or (for a write) one that is a reference
        // or holds something releasing could care about (an object, which
        // may be destroyed, also when the cycle collector's buffer holds
        // it): only an array of a few plain values is overwritten.
        let ok = if write {
            match cur {
                Value::Array(a) => {
                    !Rc::ptr_eq(a, &self.globals)
                        && a.try_borrow()
                            .is_ok_and(|a| a.len() <= 8 && a.values().all(is_plain_scalar))
                }
                Value::Undef => false,
                v => is_plain_scalar(v),
            }
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
        let cached = if !const_name {
            None
        } else {
            match op_array
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
            }
        };
        let plain = match cached {
            Some(p) => p,
            None => {
                let p = self.plain_public_prop(&ob.class_name, name, write || w_fetch, scope);
                if const_name {
                    op_array
                        .sites()
                        .borrow_mut()
                        .get_or_insert_with(Default::default)
                        .prop_sites
                        .insert(
                            ip as u32,
                            (ob.class_name.clone(), scope.map(|s| s.to_vec()), p),
                        );
                }
                p
            }
        };
        if plain == 0 || (plain == PLAIN_UNMANGLED && mangled) {
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
            if let Value::Object(o) = &v {
                crate::gc::track_object(o);
            }
            let old = std::mem::replace(&mut obm.properties[idx].1, v);
            drop(obm);
            // (the overwritten array / object loses a holder)
            crate::gc::on_value_replaced(&old, &Value::Null);
            drop(old);
            return Fast::Done;
        }
        if isset {
            // (an accessible initialized slot: set unless null)
            let set = match cur {
                Value::Reference(r) => !matches!(*r.borrow(), Value::Null),
                v => !matches!(v, Value::Null),
            };
            drop(ob);
            let v = if set { Value::True } else { Value::False };
            return if store_tmp(&op.result, v, tmps) {
                Fast::Done
            } else {
                Fast::No
            };
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
