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
        // (an object without extension state casts to true: no
        // cast_object handler of its own)
        Value::Object(o) => {
            let b = o.try_borrow().ok()?;
            if b.native.is_some() || b.properties.has_internal() {
                return None;
            }
            true
        }
        _ => return None,
    })
}

/// Whether the last holder of `v` letting it go only frees memory, when no
/// destructor can be due (`no_release`: no object with a destructor
/// exists, no generator has a finally block to run, no resource waits for
/// its destructor): plain values, containers held elsewhere too, and
/// containers (arrays, objects without state of their own, references)
/// of such values, within `budget` elements; not $GLOBALS, a resource or a
/// generator this holder is the last of.
pub(crate) fn drops_quietly_deep(
    v: &Value,
    budget: &mut u32,
    globals: &Rc<RefCell<crate::array::PhpArray>>,
) -> bool {
    match v {
        Value::Array(a) => {
            if Rc::ptr_eq(a, globals) {
                return false;
            }
            if Rc::strong_count(a) > 1 {
                return true;
            }
            let Ok(arr) = a.try_borrow() else {
                return false;
            };
            // (a copy sharing its storage with another: the elements stay)
            if arr.shared_store().is_some_and(|s| Rc::strong_count(s) > 1) {
                return true;
            }
            if arr.len() as u32 > *budget {
                return false;
            }
            *budget -= arr.len() as u32;
            arr.values().all(|v| drops_quietly_deep(v, budget, globals))
        }
        Value::Object(o) => {
            if Rc::strong_count(o) > 1 || object_release_quiet(o) {
                return true;
            }
            // (object_release_quiet(), its properties released quietly in
            // turn)
            let Ok(ob) = o.try_borrow() else {
                return false;
            };
            let gc_weak = ob.gc_tracked.get() as usize + ob.gc_buffered.get() as usize;
            if Rc::weak_count(o) > gc_weak
                || crate::gc::gc_pending()
                || ob.dtor_listed.get()
                || ob.weak_key.get()
                || ob.native.is_some()
                || ob.closure.is_some()
                || ob.properties.has_internal()
                || ob.class_name.eq_ignore_ascii_case(b"WeakMap")
                || ob.properties.len() as u32 > *budget
            {
                return false;
            }
            *budget -= ob.properties.len() as u32;
            ob.properties
                .iter()
                .all(|(_, v)| drops_quietly_deep(v, budget, globals))
        }
        Value::Reference(r) => {
            Rc::strong_count(r) > 1
                || r.try_borrow()
                    .is_ok_and(|b| drops_quietly_deep(&b, budget, globals))
        }
        Value::Generator(g) => Rc::strong_count(g) > 1,
        Value::Resource(_) => false,
        v => is_plain_scalar(v),
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
            let same = match (&a, &b) {
                (Value::Array(_), Value::Array(_)) => return Fast::No,
                // (an array is identical to no other type; a temporary's is
                // released after the opcode: the general handler's)
                (Value::Array(_), _) if !matches!(op.op1, OperandType::Tmp(_)) => false,
                (_, Value::Array(_)) if !matches!(op.op2, OperandType::Tmp(_)) => false,
                (Value::Array(_), _) | (_, Value::Array(_)) => return Fast::No,
                (Value::Object(x), Value::Object(y)) => Rc::ptr_eq(x, y),
                _ => a.identical(&b),
            };
            store!(bool_val(same == (op.opcode == OpCode::Identical)))
        }
        // (of ints: the general handler's int case)
        OpCode::BitwiseAnd | OpCode::BitwiseOr | OpCode::BitwiseXor => {
            let (Some(Value::Long(a)), Some(Value::Long(b))) = (
                defined_operand(&op.op1, cvs, tmps, literals),
                defined_operand(&op.op2, cvs, tmps, literals),
            ) else {
                return Fast::No;
            };
            store!(Value::Long(match op.opcode {
                OpCode::BitwiseAnd => a & b,
                OpCode::BitwiseOr => a | b,
                _ => a ^ b,
            }))
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
/// release could be due). Its handle is then just recycled. The only weak
/// handles on it are the cycle collector's own (its list of objects, its
/// root buffer slot: PhpObject's drop releases the slot); any other one
/// (WeakReference, a foreach over it...) takes the general handler.
#[inline]
pub(crate) fn object_release_quiet(o: &Rc<RefCell<crate::object::PhpObject>>) -> bool {
    if Rc::strong_count(o) != 1 {
        return false;
    }
    let Ok(ob) = o.try_borrow() else {
        return false;
    };
    let gc_weak = ob.gc_tracked.get() as usize + ob.gc_buffered.get() as usize;
    Rc::weak_count(o) <= gc_weak
        && !crate::gc::gc_pending()
        && !ob.dtor_listed.get()
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

/// Whether the temporary the ARRAY_SET / ARRAY_APPEND at `ip` writes into
/// (its op1) holds the very element the opcode before it, the write fetch
/// of `$a[k][...] = v` / `$o->p[...] = v` (FETCH_DIM_W / FETCH_OBJ_W),
/// fetched into it: an array element of an array variable / temporary, or
/// a property slot of a plain object (not what an offsetGet() / __get()
/// returned).
#[inline(never)]
fn fetched_container(
    op_array: &crate::opcode::OpArray,
    ip: usize,
    cvs: &[Value],
    tmps: &[Value],
    literals: &[Value],
) -> bool {
    let Some(prev) = ip.checked_sub(1).and_then(|p| op_array.ops.get(p)) else {
        return false;
    };
    let op = &op_array.ops[ip];
    if prev.result != op.op1 || prev.op1 == prev.result {
        return false;
    }
    let OperandType::Tmp(t) = op.op1 else {
        return false;
    };
    let Some(Value::Array(fetched)) = tmps.get(t as usize) else {
        return false;
    };
    let is_it = |v: &Value| match v {
        Value::Array(a) => Rc::ptr_eq(a, fetched),
        Value::Reference(r) => {
            matches!(r.try_borrow().as_deref(), Ok(Value::Array(a)) if Rc::ptr_eq(a, fetched))
        }
        _ => false,
    };
    match prev.opcode {
        OpCode::ArrayGetW => {
            let container = match prev.op1 {
                OperandType::Cv(i) => cvs.get(i as usize),
                OperandType::Tmp(i) => tmps.get(i as usize),
                _ => None,
            };
            let arr = match container {
                Some(Value::Array(a)) => a.clone(),
                Some(Value::Reference(r)) => match r.try_borrow().as_deref() {
                    Ok(Value::Array(a)) => a.clone(),
                    _ => return false,
                },
                _ => return false,
            };
            let Ok(a) = arr.try_borrow() else {
                return false;
            };
            match prev.op2 {
                // (`$a[][k] = v`: the element appended last)
                OperandType::Unused => a.values().last().is_some_and(is_it),
                _ => match dim_key(&prev.op2, cvs, tmps, literals) {
                    Some(k) => a.get(&k).is_some_and(is_it),
                    None => false,
                },
            }
        }
        OpCode::PropertyGetW => {
            let obj = match prev.op1 {
                OperandType::Cv(i) => cvs.get(i as usize),
                _ => None,
            };
            let obj = match obj {
                Some(Value::Object(o)) => o.clone(),
                Some(Value::Reference(r)) => match r.try_borrow().as_deref() {
                    Ok(Value::Object(o)) => o.clone(),
                    _ => return false,
                },
                _ => return false,
            };
            let OperandType::Const(n) = prev.op2 else {
                return false;
            };
            let Some(Value::String(name)) = literals.get(n as usize) else {
                return false;
            };
            let Ok(ob) = obj.try_borrow() else {
                return false;
            };
            ob.native.is_none()
                && !ob.properties.has_internal()
                && ob
                    .properties
                    .iter()
                    .find(|(k, _)| key_eq(k, name.as_bytes()))
                    .is_some_and(|(_, v)| is_it(v))
        }
        _ => false,
    }
}

/// ARRAY_NEW (`[]`, the start of an array literal) into a temporary.
#[inline(never)]
fn fast_array_new(op: &Op, tmps: &mut [Value]) -> Fast {
    if matches!(op.result, OperandType::Tmp(_)) && tmp_writable(&op.result, tmps) {
        let a = Rc::new(RefCell::new(crate::array::PhpArray::new()));
        store_tmp(&op.result, Value::Array(a), tmps);
        Fast::Done
    } else {
        Fast::No
    }
}

/// Whether the send at `ip` sends a variable's array that code run before
/// the call may write (see Vm::send_needs_snapshot): the general handler
/// sends a copy of its own.
#[inline(always)]
fn send_snapshot(op_array: &crate::opcode::OpArray, ip: usize, op: &Op, cvs: &[Value]) -> bool {
    let OperandType::Cv(c) = op.op1 else {
        return false;
    };
    let arr = match cvs.get(c as usize) {
        Some(Value::Array(_)) => true,
        Some(Value::Reference(r)) => matches!(r.try_borrow().as_deref(), Ok(Value::Array(_))),
        _ => false,
    };
    arr && crate::vm::Vm::send_needs_snapshot(op_array, ip)
}

/// `$s[i]` of a string and an int offset within it (FETCH_DIM_R on a
/// string): the one-byte string there; anything else (an offset outside
/// it warns, other offset types convert or throw) takes the general path.
#[inline(never)]
fn fast_string_offset(op: &Op, cvs: &[Value], tmps: &mut [Value], literals: &[Value]) -> Fast {
    if op.opcode != OpCode::ArrayGet || !tmp_writable(&op.result, tmps) {
        return Fast::No;
    }
    let s = match op.op1 {
        OperandType::Cv(i) => match cvs.get(i as usize) {
            Some(Value::String(s)) => s,
            _ => return Fast::No,
        },
        OperandType::Tmp(i) => match tmps.get(i as usize) {
            Some(Value::String(s)) => s,
            _ => return Fast::No,
        },
        OperandType::Const(i) => match literals.get(i as usize) {
            Some(Value::String(s)) => s,
            _ => return Fast::No,
        },
        _ => return Fast::No,
    };
    let idx = match op.op2 {
        OperandType::Cv(i) => cvs.get(i as usize),
        OperandType::Tmp(i) => tmps.get(i as usize),
        OperandType::Const(i) => literals.get(i as usize),
        _ => None,
    };
    let Some(Value::Long(idx)) = idx else {
        return Fast::No;
    };
    let b = s.as_bytes();
    let at = if *idx < 0 {
        match b.len().checked_sub(idx.unsigned_abs() as usize) {
            Some(at) => at,
            None => return Fast::No,
        }
    } else {
        *idx as usize
    };
    let Some(&c) = b.get(at) else {
        return Fast::No;
    };
    let v = Value::String(crate::string::PhpString::one_char(c));
    if store_tmp(&op.result, v, tmps) {
        Fast::Done
    } else {
        Fast::No
    }
}

/// FETCH_OBJ_W of a plain property (slot `idx` of `obj`) holding an array
/// (see fast_prop_op): the array, separated from its other holders, is left
/// in the result temporary for the write into it that follows.
#[inline(never)]
fn prop_fetch_w(
    obj: &Rc<RefCell<crate::object::PhpObject>>,
    idx: usize,
    op: &Op,
    tmps: &mut [Value],
) -> Fast {
    let OperandType::Tmp(t) = op.result else {
        return Fast::No;
    };
    // (the previous fetch's holder, in a loop, goes first)
    let stale_ok = match tmps.get(t as usize) {
        Some(v) if is_plain_scalar(v) => true,
        Some(Value::Array(a)) => Rc::strong_count(a) > 1,
        _ => false,
    };
    if !stale_ok {
        return Fast::No;
    }
    if let Some(v) = tmps.get_mut(t as usize) {
        if matches!(v, Value::Array(_)) {
            *v = Value::Undef;
        }
    }
    let Ok(mut ob) = obj.try_borrow_mut() else {
        return Fast::No;
    };
    let arr = match ob.properties.get(idx) {
        Some((_, Value::Array(a))) => a.clone(),
        _ => return Fast::No,
    };
    // (the slot holds one count, `arr` the other)
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
    if let Some(slot) = tmps.get_mut(t as usize) {
        *slot = Value::Array(arr);
    }
    Fast::Done
}

/// Whether property name `k` is `name` (short names compared inline, not
/// through a bcmp() call).
#[inline(always)]
fn key_eq(k: &[u8], name: &[u8]) -> bool {
    crate::string::bytes_eq(k, name)
}

/// bytes_eq() inlined, for names of up to 16 bytes (a property's, a
/// class's): two word loads.
#[inline(always)]
fn short_bytes_eq(a: &[u8], b: &[u8]) -> bool {
    let n = a.len();
    if n != b.len() {
        return false;
    }
    if (8..=16).contains(&n) {
        let w = |s: &[u8], at: usize| u64::from_ne_bytes(s[at..at + 8].try_into().unwrap());
        return w(a, 0) == w(b, 0) && w(a, n - 8) == w(b, n - 8);
    }
    if (4..8).contains(&n) {
        let w = |s: &[u8], at: usize| u32::from_ne_bytes(s[at..at + 4].try_into().unwrap());
        return w(a, 0) == w(b, 0) && w(a, n - 4) == w(b, n - 4);
    }
    crate::string::bytes_eq(a, b)
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

/// `(int) $v` / `(string) $v` of a scalar whose conversion reports nothing
/// and needs no INI setting (an int, a bool, null, a string; not a float,
/// whose string form follows `precision`): what the general handlers
/// store then.
#[inline(never)]
fn fast_cast(op: &Op, cvs: &[Value], tmps: &mut [Value], literals: &[Value]) -> Fast {
    if !tmp_writable(&op.result, tmps) {
        return Fast::No;
    }
    let Some(v) = defined_operand(&op.op1, cvs, tmps, literals) else {
        return Fast::No;
    };
    let r = match (op.opcode, v) {
        (OpCode::CastInt, v @ (Value::Long(_) | Value::True | Value::False | Value::Null)) => {
            Value::Long(v.to_long())
        }
        (OpCode::CastInt, Value::String(s)) => Value::Long(Value::String(s).to_long()),
        (OpCode::CastString, Value::String(s)) => Value::String(s),
        (OpCode::CastString, v @ (Value::Long(_) | Value::True | Value::False | Value::Null)) => {
            Value::String(v.to_php_string())
        }
        _ => return Fast::No,
    };
    if store_tmp(&op.result, r, tmps) {
        Fast::Done
    } else {
        Fast::No
    }
}

/// INIT_CV_NULL (the write-fetch of a variable a builtin's `array &$array`
/// parameter takes): an undefined variable becomes null.
#[inline]
fn fast_init_cv_null(op: &Op, cvs: &mut [Value]) -> Fast {
    let OperandType::Cv(i) = op.op1 else {
        return Fast::No;
    };
    match cvs.get_mut(i as usize) {
        Some(slot @ Value::Undef) => {
            *slot = Value::Null;
            Fast::Done
        }
        Some(Value::Reference(r)) => match r.try_borrow_mut() {
            Ok(mut b) => {
                if matches!(*b, Value::Undef) {
                    *b = Value::Null;
                }
                Fast::Done
            }
            Err(_) => Fast::No,
        },
        Some(_) => Fast::Done,
        None => Fast::No,
    }
}

/// The names of the variables isset() of an undefined one looks up as a
/// global (a superglobal not loaded yet, $GLOBALS).
fn lazy_global_name(name: &[u8]) -> bool {
    matches!(
        name,
        b"_SESSION"
            | b"_SERVER"
            | b"_GET"
            | b"_POST"
            | b"_COOKIE"
            | b"_FILES"
            | b"_REQUEST"
            | b"_ENV"
            | b"GLOBALS"
    )
}

/// FE_FETCH of the next element of an array a foreach loop iterates live
/// (see FeView::live_array): what the general handler does then. At the
/// end of the loop, the iterator is released (when that only frees memory:
/// `no_release`, see drops_quietly_deep()) and the loop left.
#[inline(never)]
fn fast_foreach_next(
    op: &Op,
    tmps: &mut [Value],
    fe: &mut FeView,
    no_release: bool,
    globals: &Rc<RefCell<crate::array::PhpArray>>,
) -> Fast {
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
            None => {
                drop(a);
                drop(arr);
                return foreach_live_end(op, it, tmps, fe, no_release, globals);
            }
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

/// FE_FREE of the iterator `op` releases (see the general handler of
/// FREE_LOOP_VAR), when no destructor can be due and the release only frees
/// memory. False: nothing was done.
#[inline(never)]
fn fast_free_loop_var(
    op: &Op,
    tmps: &mut [Value],
    fe: &FeView,
    globals: &Rc<RefCell<crate::array::PhpArray>>,
) -> bool {
    let OperandType::Tmp(t) = op.op1 else {
        return false;
    };
    if (!fe.iter_handles.is_empty() && fe.iter_handles.contains_key(&t))
        || (!fe.ref_cells.is_empty() && fe.ref_cells.contains_key(&t))
    {
        return false;
    }
    let Some(slot) = tmps.get_mut(t as usize) else {
        return false;
    };
    match slot {
        Value::Array(a) if Rc::strong_count(a) == 1 && crate::gc::gc_pending() => return false,
        Value::Reference(_) | Value::Generator(_) | Value::Resource(_) => return false,
        _ => {}
    }
    if !drops_quietly_deep(slot, &mut 256, globals) {
        return false;
    }
    if matches!(slot, Value::Object(_)) {
        crate::gc::possible_root(slot);
    }
    *slot = Value::Undef;
    true
}

/// The end of a foreach loop over an array iterated live (FE_FETCH found
/// no next element): the general handler's end of it (the iterator's
/// position forgotten, its temporary released, the jump out of the loop),
/// when the release of the iterator's array only frees memory.
#[cold]
#[inline(never)]
fn foreach_live_end(
    op: &Op,
    it: u32,
    tmps: &mut [Value],
    fe: &mut FeView,
    no_release: bool,
    globals: &Rc<RefCell<crate::array::PhpArray>>,
) -> Fast {
    let OperandType::JmpTarget(target) = op.op2 else {
        return Fast::No;
    };
    if !no_release || (!fe.cur_key.is_empty() && fe.cur_key.contains_key(&it)) {
        return Fast::No;
    }
    let Some(slot) = tmps.get_mut(it as usize) else {
        return Fast::No;
    };
    // (an array held here alone may free containers: not while the cycle
    // collector is due, as FreeTmps)
    if let Value::Array(a) = slot {
        if Rc::strong_count(a) == 1 && crate::gc::gc_pending() {
            return Fast::No;
        }
    }
    if !drops_quietly_deep(slot, &mut 256, globals) {
        return Fast::No;
    }
    *slot = Value::Undef;
    fe.positions.remove(&it);
    Fast::Jump(target as usize)
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
        // (a string that cannot be an integer's: as it is, see
        // canonical_int_key())
        Value::String(s) if !matches!(s.as_bytes().first(), Some(b'0'..=b'9' | b'-')) => {
            Some(crate::array::ArrayKey::String(s.clone()))
        }
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

/// OpArray::free_mask() of the code run_fast_ops() runs, looked up once
/// it is needed.
struct LazyFreeMask<'a> {
    op_array: &'a crate::opcode::OpArray,
    mask: Option<&'a [u8]>,
}

impl<'a> LazyFreeMask<'a> {
    #[inline(always)]
    fn get(&mut self) -> &'a [u8] {
        match self.mask {
            Some(m) => m,
            None => {
                let m = self.op_array.free_mask();
                self.mask = Some(m);
                m
            }
        }
    }
}

/// The op1 / op2 temporaries (bits 0 and 1) the opcode at `ip` is the last
/// user of (see OpArray::free_mask()) that hold an object or an array, when
/// the dispatch loop's release of them right after the opcode would only
/// drop a holder: each is held elsewhere too, and no destructor is due
/// (`no_release`: the release point that follows does nothing). The fast
/// path then runs the opcode and releases them itself (release_consumed()).
/// The value a write stores (`$a[k] = f()`, `$o->p = f()`) may be held by
/// the temporary alone: the container holds it then, or its release only
/// frees memory (drops_quietly_deep()). None: the opcode is left to the
/// dispatch loop.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn consumed_tmps(
    free_mask: &[u8],
    ops: &[Op],
    ip: usize,
    op: &Op,
    tmps: &[Value],
    no_release: bool,
    globals: &Rc<RefCell<crate::array::PhpArray>>,
) -> Option<u8> {
    let m = free_mask.get(ip).copied().unwrap_or(0);
    if m & 11 == 0 {
        return Some(0);
    }
    consumed_tmps_of(m, ops, ip, op, tmps, no_release, globals)
}

/// consumed_tmps() of an opcode consuming temporaries (free mask `m`).
#[inline(never)]
fn consumed_tmps_of(
    m: u8,
    ops: &[Op],
    ip: usize,
    op: &Op,
    tmps: &[Value],
    no_release: bool,
    globals: &Rc<RefCell<crate::array::PhpArray>>,
) -> Option<u8> {
    // (bit 3: a compound assignment's operand, released after its write:
    // the previous opcode's op2; one holding a plain value has nothing to
    // release then)
    if m & 8 != 0 {
        let plain = match ip.checked_sub(1).and_then(|p| ops.get(p)).map(|p| p.op2) {
            Some(OperandType::Tmp(v)) => tmps.get(v as usize).is_some_and(is_plain_scalar),
            _ => false,
        };
        if !plain {
            return None;
        }
    }
    let mut rel = 0;
    for (b, o) in [(1u8, &op.op1), (2u8, &op.op2)] {
        if m & b == 0 {
            continue;
        }
        let OperandType::Tmp(i) = o else {
            continue;
        };
        let shared = match tmps.get(*i as usize) {
            Some(Value::Object(x)) => Rc::strong_count(x) > 1,
            Some(Value::Array(x)) => Rc::strong_count(x) > 1,
            _ => continue,
        };
        if !no_release {
            return None;
        }
        if !shared {
            let stored = b == 2
                && matches!(
                    op.opcode,
                    OpCode::ArraySet | OpCode::ArrayAppend | OpCode::PropertySet
                );
            if !stored || !drops_quietly_deep(&tmps[*i as usize], &mut 256, globals) {
                return None;
            }
        }
        rel |= b;
    }
    Some(rel)
}

/// After the opcode `op` ran (`fast` is not Fast::No), the temporaries
/// consumed_tmps() found (`rel`) are released, as the dispatch loop would
/// right after it.
#[inline]
fn release_consumed(rel: u8, fast: Fast, op: &Op, tmps: &mut [Value]) -> Fast {
    if rel != 0 && !matches!(fast, Fast::No) {
        for (b, o) in [(1u8, &op.op1), (2u8, &op.op2)] {
            if rel & b != 0 {
                if let OperandType::Tmp(i) = o {
                    if let Some(slot @ (Value::Object(_) | Value::Array(_))) =
                        tmps.get_mut(*i as usize)
                    {
                        *slot = Value::Undef;
                    }
                }
            }
        }
    }
    fast
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
    pub(crate) ref_cells: &'a crate::fxhash::FxHashMap<u32, Rc<RefCell<Value>>>,
    /// The depth of the frame's exception handler stack (SaveHandlerDepth).
    pub(crate) handler_depth: u32,
}

impl FeView<'_> {
    /// The temporaries whose foreach state (snapshot, object or iterator
    /// handle) the frame holds.
    #[inline(always)]
    fn busy(&self) -> FeTmps {
        // (no loop with such state: the common case, without walking the maps)
        if self.snap.is_empty() && self.obj.is_empty() && self.iter_handles.is_empty() {
            return FeTmps { n: 0, keys: [0; 6] };
        }
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

/// Whether the INIT_FCALL `op` may call a builtin of SPECIAL_CALLS (is_*,
/// count, sizeof, strlen, array_key_exists), from its name's first byte and
/// length: other calls do not look for one (see fast_special_call).
#[inline(always)]
fn maybe_special_call(op: &Op, literals: &[Value]) -> bool {
    let OperandType::Const(i) = op.op1 else {
        return false;
    };
    let Some(Value::String(name)) = literals.get(i as usize) else {
        return false;
    };
    let nb = name.as_bytes();
    match nb.first().map(u8::to_ascii_lowercase) {
        Some(b'i') => nb.len() >= 6 && nb[..3].eq_ignore_ascii_case(b"is_"),
        Some(b'c') => nb.len() == 5,
        Some(b's') => nb.len() == 6,
        Some(b'a') => nb.len() == 16,
        _ => false,
    }
}

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
            | OpCode::PropertyGetRW
            | OpCode::PropertyGetWUnset
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
            | OpCode::ArrayGetW
            | OpCode::BitwiseAnd
            | OpCode::BitwiseOr
            | OpCode::BitwiseXor
            | OpCode::ArrayNew
            | OpCode::SaveHandlerDepth
            | OpCode::FreeLoopVar
            | OpCode::ArrayGetIncDec
            | OpCode::ArrayGetWUnset
            | OpCode::CastInt
            | OpCode::CastString
            | OpCode::InitCvNull
            // (a call whose arguments the general handlers sent: a lean
            // call, or a builtin's, see the DO_FCALL arm of run_fast_ops)
            | OpCode::DoFCall
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
        // (the slices: the vectors are not looked at again for each opcode)
        let ops: &[Op] = &op_array.ops;
        let literals: &[Value] = &op_array.literals;
        // An opcode consuming a temporary object or array is left to the
        // dispatch loop, which releases it after the opcode (see
        // OpArray::free_mask()). (Only the property and dimension opcodes
        // here take such operands.) (Looked up by the first such opcode:
        // the runs of most calls have none.)
        let mut free_mask = LazyFreeMask {
            op_array,
            mask: None,
        };
        // (the index of the last opcode run, usize::MAX: none; its line is
        // the dispatch loop's current line once control goes back to it,
        // nothing here reads it)
        let mut last = usize::MAX;
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
                    // (only when no destructor is due: the call's release of
                    // its arguments is then a no-op)
                    let special = if no_release && maybe_special_call(op, literals) {
                        self.fast_special_call(op_array, ip, cvs, tmps)
                    } else {
                        None
                    };
                    match special {
                        Some(next) => Fast::Jump(next),
                        None if self.init_fcall_fast(op_array, ip, cvs) => {
                            // (a plain user function called with plain
                            // arguments: the whole call, see leancall.rs)
                            let lean = no_release && self.lean_candidate();
                            match lean.then(|| self.lean_fused(op_array, ip, cvs, tmps)) {
                                Some(Some(next)) => Fast::Jump(next),
                                _ => Fast::Done,
                            }
                        }
                        None => break 'run ip,
                    }
                }
                // A by-value argument of a call resolved when it was set up
                // (see the SEND_VAL handler).
                OpCode::SendVal => {
                    let snapshot = send_snapshot(op_array, ip, op, cvs);
                    if self.send_val_fast(op, cvs, tmps, literals, snapshot, false) {
                        Fast::Done
                    } else {
                        break 'run ip;
                    }
                }
                // A non-variable argument of a user function's by-value
                // parameter (SEND_VAL_EX / SEND_VAR_NO_REF_EX): sent as is.
                OpCode::SendValChecked => {
                    // (or of an internal function's by-value parameter; an
                    // array not as the first argument of the builtins taking
                    // an `array &$array` there: the general handler separates
                    // one)
                    let fn_gen = self.user_functions.len() + 1;
                    let mut ucf = false;
                    let by_value_target = self.pending_calls.last().is_some_and(|c| {
                        c.target.fn_gen == fn_gen
                            && match (&c.target.user, &c.target.builtin) {
                                (Some(_), _) => true,
                                // (call_user_func(_array)(): by value)
                                (None, None) if is_direct_ucf(c) => {
                                    ucf = true;
                                    true
                                }
                                (None, Some(crate::vm::BuiltinDesc { zpp: Some((fi, _)), .. })) => {
                                    let idx = c.args.len();
                                    fi.params
                                        .get(idx)
                                        .or_else(|| fi.params.last().filter(|p| p.variadic))
                                        .is_none_or(|p| p.by_ref == 0)
                                        && match defined_operand(&op.op1, cvs, tmps, literals) {
                                            None => false,
                                            Some(Value::Array(_)) => {
                                                ucf = idx != 0
                                                    || !crate::compiler::Compiler::is_array_ref_builtin(
                                                        Some(c.name.as_bytes()),
                                                    );
                                                ucf
                                            }
                                            Some(_) => true,
                                        }
                                }
                                _ => false,
                            }
                    });
                    if by_value_target
                        && self.send_val_fast(
                            op,
                            cvs,
                            tmps,
                            literals,
                            send_snapshot(op_array, ip, op, cvs),
                            ucf,
                        )
                    {
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
                | OpCode::PropertyGetRW
                | OpCode::PropertyIsset
                | OpCode::PropertyIssetFetch => {
                    if self.isset_get_cache.is_some() {
                        break 'run ip;
                    }
                    // (only a temporary operand is consumed)
                    let rel = if matches!(op.op1, OperandType::Tmp(_))
                        || matches!(op.op2, OperandType::Tmp(_))
                    {
                        let Some(rel) =
                            consumed_tmps(free_mask.get(), ops, ip, op, tmps, no_release, &self.globals)
                        else {
                            break 'run ip;
                        };
                        rel
                    } else {
                        0
                    };
                    let f = match self.prop_ic_hit(op_array, ip, op, cvs, tmps) {
                        Some(f) => f,
                        None => self.fast_prop_op(op_array, ip, cvs, tmps),
                    };
                    release_consumed(rel, f, op, tmps)
                }
                OpCode::ArrayGet | OpCode::ArraySet | OpCode::ArrayAppend => {
                    let Some(rel) =
                        consumed_tmps(free_mask.get(), ops, ip, op, tmps, no_release, &self.globals)
                    else {
                        break 'run ip;
                    };
                    // (a write into the container the previous opcode fetched)
                    let tmp_container = op.opcode != OpCode::ArrayGet
                        && matches!(op.op1, OperandType::Tmp(_))
                        && fetched_container(op_array, ip, cvs, tmps, literals);
                    let f = self.fast_dim_op(op, cvs, tmps, literals, tmp_container);
                    release_consumed(rel, f, op, tmps)
                }
                // `unset($a[k][j])` (see fast_dim_unset_pair)
                OpCode::ArrayGetWUnset if no_release => {
                    match self.fast_dim_unset_pair(op_array, ip, cvs, tmps) {
                        Some(next) => Fast::Jump(next),
                        None => break 'run ip,
                    }
                }
                // `unset($cv->p[k])` (see fast_prop_unset_pair)
                OpCode::PropertyGetWUnset if no_release => {
                    match self.fast_prop_unset_pair(op_array, ip, cvs, tmps) {
                        Some(next) => Fast::Jump(next),
                        None => break 'run ip,
                    }
                }
                // `$a[k]++` of an int element (see fast_dim_incdec)
                OpCode::ArrayGetIncDec => match self.fast_dim_incdec(op_array, ip, cvs, tmps) {
                    Some(next) => Fast::Jump(next),
                    None => break 'run ip,
                },
                // FETCH_DIM_W of an array element (see fast_dim_fetch_w)
                OpCode::ArrayGetW => {
                    let Some(rel) =
                        consumed_tmps(free_mask.get(), ops, ip, op, tmps, no_release, &self.globals)
                    else {
                        break 'run ip;
                    };
                    let f = self.fast_dim_fetch_w(op_array, ip, cvs, tmps);
                    release_consumed(rel, f, op, tmps)
                }
                // (on an array, FETCH_DIM_IS's isset answers as ISSET_ISEMPTY_DIM's)
                OpCode::ArrayIsset | OpCode::ArrayIssetFetch => {
                    let Some(rel) =
                        consumed_tmps(free_mask.get(), ops, ip, op, tmps, no_release, &self.globals)
                    else {
                        break 'run ip;
                    };
                    let f = self.fast_dim_isset(op, cvs, tmps, literals);
                    release_consumed(rel, f, op, tmps)
                }
                OpCode::IssetCheck => {
                    let Some(rel) =
                        consumed_tmps(free_mask.get(), ops, ip, op, tmps, no_release, &self.globals)
                    else {
                        break 'run ip;
                    };
                    let f = match fast_isset_check(op, cvs, tmps, literals) {
                        // isset() of a variable that is null or undefined:
                        // false, unless it names a global looked up then
                        Fast::No => match op.op1 {
                            OperandType::Cv(i)
                                if matches!(
                                    cvs.get(i as usize),
                                    Some(Value::Undef | Value::Null)
                                ) && op_array
                                    .cv_names
                                    .get(i as usize)
                                    .is_some_and(|n| !lazy_global_name(n)) =>
                            {
                                if store_tmp(&op.result, Value::False, tmps) {
                                    Fast::Done
                                } else {
                                    Fast::No
                                }
                            }
                            _ => Fast::No,
                        },
                        f => f,
                    };
                    release_consumed(rel, f, op, tmps)
                }
                OpCode::CastInt | OpCode::CastString => fast_cast(op, cvs, tmps, literals),
                OpCode::InitCvNull => fast_init_cv_null(op, cvs),
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
                    // (one pass: scalars are emptied at once, the containers
                    // checked; nothing is released unless all of them may be)
                    let mut heap = false;
                    for v in range.iter() {
                        match v {
                            Value::Undef
                            | Value::Null
                            | Value::False
                            | Value::True
                            | Value::Long(_)
                            | Value::Double(_)
                            | Value::String(_) => {}
                            Value::Array(_) | Value::Object(_) => {
                                if !drops_quietly_deep(v, &mut 256, &self.globals) {
                                    break 'run ip;
                                }
                                heap = true;
                            }
                            _ => break 'run ip,
                        }
                    }
                    if heap && crate::gc::gc_pending() {
                        break 'run ip;
                    }
                    for slot in range {
                        match slot {
                            Value::Undef => {}
                            Value::Array(_) | Value::Object(_) | Value::String(_) => {
                                *slot = Value::Undef
                            }
                            // (plain values: nothing to drop)
                            v => std::mem::forget(std::mem::replace(v, Value::Undef)),
                        }
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
                        // (a temporary no running loop holds; or the iterator
                        // of a loop left by a `break`, released: FE_FREE)
                        let tmp_free = match op.op1 {
                            OperandType::Tmp(t) => {
                                !fe.busy().any_in(t, t + 1)
                                    && (fe.positions.is_empty()
                                        || !fe.positions.contains_key(&t)
                                        || matches!(op.op2, OperandType::Const(c)
                                            if matches!(literals.get(c as usize), Some(Value::Undef))))
                            }
                            _ => false,
                        };
                        self.fast_assign(
                            op, cvs, tmps, literals, refs_ok, no_release, fe_quiet, tmp_free,
                        )
                    }
                    f => f,
                },
                OpCode::QmAssign => match fast_op(op, cvs, tmps, literals, refs_ok, no_release) {
                    Fast::No => self.fast_qm_assign_container(op, cvs, tmps),
                    f => f,
                },
                OpCode::AssignConcat => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::BooleanNot => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::BitwiseAnd => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::BitwiseOr => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                OpCode::BitwiseXor => fast_op(op, cvs, tmps, literals, refs_ok, no_release),
                // `[]` (the start of an array literal) into a temporary
                OpCode::ArrayNew => fast_array_new(op, tmps),
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
                // (the depth of the exception handler stack at a loop's
                // start, for the release of its iterator on `return`)
                OpCode::SaveHandlerDepth => match op.result {
                    OperandType::Tmp(t) if tmp_writable(&op.result, tmps) => {
                        tmps[t as usize] = Value::Long(i64::from(fe.handler_depth));
                        Fast::Done
                    }
                    _ => break 'run ip,
                },
                // The release of a loop's iterator on `break` or `return`
                // (FE_FREE), when it only frees memory.
                OpCode::FreeLoopVar if no_release => {
                    if fast_free_loop_var(op, tmps, fe, &self.globals) {
                        Fast::Done
                    } else {
                        break 'run ip;
                    }
                }
                OpCode::ForeachNext => fast_foreach_next(op, tmps, fe, no_release, &self.globals),
                OpCode::ForeachKey => fast_foreach_key(op, tmps, fe),
                // A public method of a user class by a constant name (what
                // the INIT_METHOD_CALL arm does first)
                OpCode::InitMethodCall => {
                    if matches!(op.op2, OperandType::Const(_))
                        && self.init_method_call_fast(op_array, ip, cvs, tmps)
                    {
                        // (with plain arguments: the whole call, see
                        // leancall.rs)
                        let lean = no_release && self.lean_candidate();
                        match lean.then(|| self.lean_fused(op_array, ip, cvs, tmps)) {
                            Some(Some(next)) => Fast::Jump(next),
                            _ => Fast::Done,
                        }
                    } else {
                        break 'run ip;
                    }
                }
                OpCode::Concat => fast_concat(op, cvs, tmps, literals),
                OpCode::TypeCheck => match fast_instanceof(op, cvs, tmps, literals) {
                    Fast::No => self.cached_instanceof(op_array, ip, cvs, tmps),
                    f => f,
                },
                OpCode::ForeachInit => self.fast_foreach_init(op, cvs, tmps, literals, fe),
                OpCode::NewObject => match self.fast_new_object(op_array, ip, tmps) {
                    Some(next) => Fast::Jump(next),
                    None => break 'run ip,
                },
                // (a frame bound to a symbol table rebinds itself: the general handler)
                OpCode::BindGlobal if bound.is_none() => self.fast_bind_global(op, cvs, literals),
                // A call of a plain user function: its callee's opcodes run
                // here too, without a frame (see leancall.rs).
                OpCode::DoFCall => {
                    // (a lean call that failed in its callee, see
                    // builtincall.rs: the dispatch loop's DO_FCALL takes it)
                    if self.fast_call_failed.is_some() {
                        break 'run ip;
                    }
                    if no_release && self.lean_candidate() {
                        if self.lean_call(op_array, ip, cvs, tmps) {
                            Fast::Done
                        } else if self.lean_depth >= 1 {
                            // (the callee of a lean call makes the call
                            // itself, see Vm::lean_user_dofcall)
                            match self.lean_user_dofcall(op_array, ip, cvs, tmps, free_mask.get()) {
                                Some(true) if self.lean_frame_call_left() => {
                                    last = ip;
                                    break 'run ip + 1;
                                }
                                Some(true) => Fast::Done,
                                _ => break 'run ip,
                            }
                        } else {
                            break 'run ip;
                        }
                    } else if no_release
                        && bound.is_none()
                        && self.lean_depth == 0
                        // (a lean call handed over here: it takes the
                        // DO_FCALL, its pending call is not the top one)
                        && self.lean_resume.is_empty()
                        && self.fast_builtin_target()
                    {
                        // A builtin by its plain name (see builtincall.rs).
                        match self.fast_builtin_dofcall(
                            op_array,
                            ip,
                            tmps,
                            internal_code,
                            free_mask.get(),
                        ) {
                            // (an object with a destructor came to be: the
                            // run's release-free assumption no longer holds)
                            Some(true) if self.has_release_work() => {
                                last = ip;
                                break 'run ip + 1;
                            }
                            Some(true) => Fast::Done,
                            Some(false) => break 'run ip,
                            None => return ip,
                        }
                    } else if no_release
                        && self.lean_depth >= 1
                        && self.lean_resume.is_empty()
                        && self.fast_builtin_target()
                    {
                        // The callee of a lean call (see builtincall.rs).
                        match self.lean_builtin_dofcall(op_array, ip, tmps, free_mask.get()) {
                            Some(true) if self.has_release_work() => {
                                last = ip;
                                break 'run ip + 1;
                            }
                            Some(true) => Fast::Done,
                            Some(false) => break 'run ip,
                            None => return ip,
                        }
                    } else if no_release && self.lean_depth >= 1 {
                        // A user function the callee of a lean call calls
                        // with a frame (see Vm::lean_user_dofcall).
                        match self.lean_user_dofcall(op_array, ip, cvs, tmps, free_mask.get()) {
                            Some(true) if self.lean_frame_call_left() => {
                                last = ip;
                                break 'run ip + 1;
                            }
                            Some(true) => Fast::Done,
                            Some(false) => break 'run ip,
                            None => return ip,
                        }
                    } else {
                        return ip;
                    }
                }
                // (whether the next argument of the call is sent by reference)
                OpCode::JmpIfArgByRef => match (self.fast_arg_by_ref(op), op.op1) {
                    (Some(true), OperandType::JmpTarget(t)) => Fast::Jump(t as usize),
                    (Some(false), _) => Fast::Done,
                    _ => break 'run ip,
                },
                _ => return ip,
            };
            last = ip;
            match fast {
                Fast::No => return ip,
                Fast::Done => ip += 1,
                // EG(vm_interrupt) is looked at where control jumps (loops
                // and calls, as php-src's ZEND_VM_LOOP_INTERRUPT_CHECK):
                // straight runs of opcodes end soon enough.
                Fast::Jump(t) => {
                    ip = t;
                    if crate::value::VM_INTERRUPT.load(std::sync::atomic::Ordering::Relaxed) {
                        break 'run ip;
                    }
                }
            }
            if crate::value::memory_exhausted() != 0 {
                break 'run ip;
            }
        };
        if let (Some(op), false) = (ops.get(last), internal_code) {
            self.current_line = op.line;
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
    /// (nothing done). Called only when no destructor is due (the call's
    /// release of its arguments is then a no-op) and the name may be one
    /// (maybe_special_call).
    #[inline(never)]
    fn fast_special_call(
        &mut self,
        op_array: &crate::opcode::OpArray,
        ip: usize,
        cvs: &[Value],
        tmps: &mut [Value],
    ) -> Option<usize> {
        let ops = &op_array.ops;
        let literals = &op_array.literals;
        let op = &ops[ip];
        let name = crate::vm::plain_fcall_name(op, literals)?;
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
            return ((t.builtin.is_some() && !name.windows(2).any(|w| w == b"::"))
                || is_direct_ucf(call))
            .then_some(false);
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
            // (an undefined function: the dispatch loop's handler throws)
            if self.top_call_unresolved() {
                if let Some(mut call) = self.pending_calls.pop() {
                    self.recycle_args(&mut call);
                }
                return false;
            }
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
            // (no such method: the dispatch loop's handler may throw)
            if self.top_call_unresolved() {
                if let Some(mut call) = self.pending_calls.pop() {
                    self.recycle_args(&mut call);
                }
                return false;
            }
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
        tmp_free: bool,
    ) -> Fast {
        // (the general handler is a release point: it is only bypassed when
        // no destructor can be due)
        if !no_release || !tmp_writable(&op.result, tmps) {
            return Fast::No;
        }
        // (an overwritten array or object loses this holder)
        let releasable = |v: &Value| match v {
            Value::Array(_) | Value::Object(_) => drops_quietly_deep(v, &mut 256, &self.globals),
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
            // (a temporary holding a plain value, not an iterator's; or a
            // container released quietly, held by no running loop)
            OperandType::Tmp(ti) if fe_quiet => match tmps.get(ti as usize) {
                Some(v) if is_plain_scalar(v) => Some(ti as usize),
                Some(v @ (Value::Array(_) | Value::Object(_))) if tmp_free && releasable(v) => {
                    Some(ti as usize)
                }
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
            // (an empty array is iterated live too: its copy is its own)
            if view.shared_store().is_none() && !view.is_empty() {
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
            && self.new_skips_ctor_call(op_array, ip + 1, &obj, false);
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
        tmp_container: bool,
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
            // The container a write fetch (`$a[k][j] = v`, `$o->p[k] = v`)
            // left in its temporary, found to be the element it fetched (see
            // fetched_container): separated already, held by its container
            // and the temporary only, it is written in place as the general
            // handler writes it.
            OperandType::Tmp(i) if tmp_container => match tmps.get(i as usize) {
                Some(Value::Array(a)) if Rc::strong_count(a) == 2 => Some(a.clone()),
                _ => None,
            },
            // (an array only the temporary holds, an array literal being
            // built: nothing else sees the write)
            OperandType::Tmp(i) => match tmps.get(i as usize) {
                Some(Value::Array(a)) if Rc::strong_count(a) == 1 => Some(a.clone()),
                _ => None,
            },
            _ => None,
        };
        let Some(arr) = arr else {
            if !write {
                return fast_string_offset(op, cvs, tmps, literals);
            }
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
            // (the container's holders: its variable or temporary and the
            // clone here; a fetched container's also its own container)
            let holders = if tmp_container && matches!(op.op1, OperandType::Tmp(_)) {
                3
            } else {
                2
            };
            let container_ok = |a: &Rc<RefCell<crate::array::PhpArray>>| {
                !Rc::ptr_eq(a, &arr)
                    && !Rc::ptr_eq(a, &self.globals)
                    && Rc::strong_count(&arr) <= holders
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
            // (an overwritten element: a plain value, or one whose release
            // only frees memory when no destructor can be due; a reference
            // is written through by the general handler)
            match a.get(&key) {
                None => {}
                Some(v) if is_plain_scalar(v) => {}
                Some(v @ (Value::Array(_) | Value::Object(_)))
                    if !self.has_release_work()
                        && drops_quietly_deep(v, &mut 256, &self.globals) => {}
                _ => return Fast::No,
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

    /// `$a[k]++` / `++$a[k]` (and `--`) of an array variable's element
    /// holding an int: FETCH_DIM_RW at `ip`, the increment of its result,
    /// the write-back (ArraySetIncDec) of the same variable and key, run at
    /// once as the general handlers would run them then (the key needs no
    /// conversion, the element exists: nothing is reported). Returns the
    /// index of the opcode after the write-back, or None (nothing done).
    #[inline(never)]
    pub(crate) fn fast_dim_incdec(
        &self,
        op_array: &crate::opcode::OpArray,
        ip: usize,
        cvs: &[Value],
        tmps: &mut [Value],
    ) -> Option<usize> {
        let ops = &op_array.ops[..];
        let op = &ops[ip];
        let (OperandType::Cv(c), OperandType::Tmp(t)) = (op.op1, op.result) else {
            return None;
        };
        let inc = ops.get(ip + 1)?;
        let set = ops.get(ip + 2)?;
        let (up, post) = match inc.opcode {
            OpCode::PreIncrement => (true, false),
            OpCode::PostIncrement => (true, true),
            OpCode::PreDecrement => (false, false),
            OpCode::PostDecrement => (false, true),
            _ => return None,
        };
        if inc.op1 != OperandType::Tmp(t)
            || set.opcode != OpCode::ArraySetIncDec
            || set.op1 != op.op1
            || set.op2 != OperandType::Tmp(t)
            || set.result != op.op2
            || self.dim_rw_key.is_some()
            || self.dim_rw_abort.is_some()
            || !tmp_writable(&op.result, tmps)
            || !tmp_writable(&inc.result, tmps)
        {
            return None;
        }
        let arr = match cvs.get(c as usize)? {
            Value::Array(a) => a.clone(),
            Value::Reference(r) => match &*r.try_borrow().ok()? {
                Value::Array(a) => a.clone(),
                _ => return None,
            },
            _ => return None,
        };
        if Rc::ptr_eq(&arr, &self.globals) {
            return None;
        }
        let key = dim_key(&op.op2, cvs, tmps, &op_array.literals)?;
        let mut a = arr.try_borrow_mut().ok()?;
        let old = match a.get(&key)? {
            Value::Long(n) => *n,
            _ => return None,
        };
        let new = if up {
            old.checked_add(1)?
        } else {
            old.checked_sub(1)?
        };
        a.set(key, Value::Long(new));
        drop(a);
        tmps[t as usize] = Value::Long(new);
        if let OperandType::Tmp(r) = inc.result {
            tmps[r as usize] = Value::Long(if post { old } else { new });
        }
        Some(ip + 3)
    }

    /// `unset($a[k][j])` of an array variable's element: FETCH_DIM_UNSET
    /// at `ip` and the UNSET_DIM of what it fetched, run at once as the
    /// general handlers would run them when both keys need no conversion
    /// and what goes only frees memory: an element array held elsewhere
    /// too is separated first; a missing or null element has nothing to
    /// unset. Returns the index of the opcode after the UNSET_DIM, or None
    /// (nothing done).
    #[inline(never)]
    pub(crate) fn fast_dim_unset_pair(
        &self,
        op_array: &crate::opcode::OpArray,
        ip: usize,
        cvs: &[Value],
        tmps: &[Value],
    ) -> Option<usize> {
        let ops = &op_array.ops[..];
        let literals = &op_array.literals[..];
        let op = &ops[ip];
        let (OperandType::Cv(c), OperandType::Tmp(t)) = (op.op1, op.result) else {
            return None;
        };
        let unset = ops.get(ip + 1)?;
        if unset.opcode != OpCode::ArrayUnset
            || unset.op1 != OperandType::Tmp(t)
            || self.has_release_work()
            || crate::gc::gc_pending()
        {
            return None;
        }
        let arr = match cvs.get(c as usize)? {
            Value::Array(a) => a.clone(),
            Value::Reference(r) => match &*r.try_borrow().ok()? {
                Value::Array(a) => a.clone(),
                _ => return None,
            },
            _ => return None,
        };
        if Rc::ptr_eq(&arr, &self.globals) {
            return None;
        }
        let k1 = dim_key(&op.op2, cvs, tmps, literals)?;
        let k2 = dim_key(&unset.op2, cvs, tmps, literals)?;
        // (what goes must only free memory: looked at before anything
        // changes)
        {
            let a = arr.try_borrow().ok()?;
            match a.get(&k1) {
                None | Some(Value::Null) => return Some(ip + 2),
                Some(Value::Array(e)) => match e.try_borrow().ok()?.get(&k2) {
                    None => {}
                    Some(v) if is_plain_scalar(v) => {}
                    Some(v @ (Value::Array(_) | Value::Object(_)))
                        if drops_quietly_deep(v, &mut 256, &self.globals) => {}
                    _ => return None,
                },
                _ => return None,
            }
        }
        // The element array, separated from its other holders (the
        // container's storage first: a copy of the container shares it).
        let elem = {
            let mut a = arr.try_borrow_mut().ok()?;
            let slot = a.get_mut(&k1)?;
            let Value::Array(e) = slot else {
                return None;
            };
            if Rc::strong_count(e) > 1 {
                let sep = Rc::new(RefCell::new(e.try_borrow().ok()?.clone()));
                let old = std::mem::replace(slot, Value::Array(sep.clone()));
                crate::gc::on_value_replaced(&old, slot);
                drop(old);
                sep
            } else {
                e.clone()
            }
        };
        let mut e = elem.try_borrow_mut().ok()?;
        e.remove(&k2);
        Some(ip + 2)
    }

    /// PropertyGetWUnset + ArrayUnset at `ip` (`unset($cv->p[k])`) of a
    /// plain property (for a write, see plain_public_prop()) holding an
    /// array, whose element going only frees memory: what both do (the
    /// property's array separated from its other holders, the element
    /// removed). Returns the index of the opcode after them, or None
    /// (nothing done).
    #[inline(never)]
    pub(crate) fn fast_prop_unset_pair(
        &self,
        op_array: &crate::opcode::OpArray,
        ip: usize,
        cvs: &[Value],
        tmps: &[Value],
    ) -> Option<usize> {
        let ops = &op_array.ops[..];
        let literals = &op_array.literals[..];
        let op = &ops[ip];
        let (OperandType::Cv(c), OperandType::Const(ni), OperandType::Tmp(t)) =
            (op.op1, op.op2, op.result)
        else {
            return None;
        };
        let unset = ops.get(ip + 1)?;
        if unset.opcode != OpCode::ArrayUnset
            || unset.op1 != OperandType::Tmp(t)
            || self.has_release_work()
            || crate::gc::gc_pending()
            || !self.property_hook_stack.is_empty()
            || self.isset_get_cache.is_some()
        {
            return None;
        }
        let Some(Value::String(name)) = literals.get(ni as usize) else {
            return None;
        };
        let name = name.as_bytes();
        if matches!(name, [] | [0, ..] | [b'_', b'_', ..]) {
            return None;
        }
        let held;
        let obj = match cvs.get(c as usize)? {
            Value::Object(o) => o,
            Value::Reference(r) => {
                held = r.try_borrow().ok()?;
                match &*held {
                    Value::Object(o) => o,
                    _ => return None,
                }
            }
            _ => return None,
        };
        let k2 = dim_key(&unset.op2, cvs, tmps, literals)?;
        let idx = {
            let ob = obj.try_borrow().ok()?;
            if ob.native.is_some() || ob.closure.is_some() || ob.properties.has_internal() {
                return None;
            }
            let idx = ob.properties.iter().position(|(k, _)| key_eq(k, name))?;
            let scope = self.class_scope_ref();
            let plain = self.plain_public_prop(&ob.class_name, name, true, scope);
            if plain == 0 || (plain == PLAIN_UNMANGLED && ob.properties.has_mangled()) {
                return None;
            }
            // (what goes must only free memory: looked at before anything
            // changes)
            match &ob.properties[idx].1 {
                Value::Array(a) if !Rc::ptr_eq(a, &self.globals) => {
                    match a.try_borrow().ok()?.get(&k2) {
                        None => {}
                        Some(v) if is_plain_scalar(v) => {}
                        Some(v @ (Value::Array(_) | Value::Object(_)))
                            if drops_quietly_deep(v, &mut 256, &self.globals) => {}
                        _ => return None,
                    }
                }
                _ => return None,
            }
            idx
        };
        // The property's array, separated from its other holders.
        let arr = {
            let mut ob = obj.try_borrow_mut().ok()?;
            let slot = &mut ob.properties.get_mut(idx)?.1;
            let Value::Array(a) = slot else {
                return None;
            };
            if Rc::strong_count(a) > 1 {
                let sep = Rc::new(RefCell::new(a.try_borrow().ok()?.clone()));
                let old = std::mem::replace(slot, Value::Array(sep.clone()));
                crate::gc::on_value_replaced(&old, slot);
                drop(old);
                sep
            } else {
                a.clone()
            }
        };
        let mut a = arr.try_borrow_mut().ok()?;
        a.remove(&k2);
        Some(ip + 2)
    }

    /// FETCH_DIM_W (the container fetch of `$a[k][...] = v`, `$a[][...] =
    /// v`) of an array variable's element, or of the element of the container
    /// the previous fetch left in a temporary (see fetched_container), when
    /// the element is an array, null or missing: what the general handler
    /// (fetch_dim_from_array()) does then: the element is separated from
    /// its other holders, or made a new array, and left in the result.
    #[inline(never)]
    pub(crate) fn fast_dim_fetch_w(
        &mut self,
        op_array: &crate::opcode::OpArray,
        ip: usize,
        cvs: &mut [Value],
        tmps: &mut [Value],
    ) -> Fast {
        let op = &op_array.ops[ip];
        let literals = &op_array.literals;
        if self.dim_rw_abort.is_some() || self.dim_rw_key.is_some() {
            return Fast::No;
        }
        let OperandType::Tmp(rt) = op.result else {
            return Fast::No;
        };
        // (sort($a[k]): the general handler creates the element null)
        if matches!(op_array.ops.get(ip + 1), Some(n) if n.opcode == OpCode::SendVal && n.op1 == op.result)
        {
            return Fast::No;
        }
        let arr = match op.op1 {
            OperandType::Cv(i) => match cvs.get(i as usize) {
                Some(Value::Array(a)) => a.clone(),
                _ => return Fast::No,
            },
            OperandType::Tmp(i) if op.op1 != op.result => {
                if !fetched_container(op_array, ip, cvs, tmps, literals) {
                    return Fast::No;
                }
                match tmps.get(i as usize) {
                    Some(Value::Array(a)) if Rc::strong_count(a) == 2 => a.clone(),
                    _ => return Fast::No,
                }
            }
            _ => return Fast::No,
        };
        if Rc::ptr_eq(&arr, &self.globals) {
            return Fast::No;
        }
        let key = match op.op2 {
            OperandType::Unused => None,
            _ => match dim_key(&op.op2, cvs, tmps, literals) {
                Some(k) => Some(k),
                None => return Fast::No,
            },
        };
        // The result's stale holder (the previous fetch's, in a loop) goes
        // first, as the general handler drops it: when it only loses a
        // holder.
        let stale_ok = match tmps.get(rt as usize) {
            Some(v) if is_plain_scalar(v) => true,
            Some(Value::Array(a)) => Rc::strong_count(a) > 1,
            _ => false,
        };
        if !stale_ok {
            return Fast::No;
        }
        if let Some(v) = tmps.get_mut(rt as usize) {
            if matches!(v, Value::Array(_)) {
                *v = Value::Undef;
            }
        }
        let Ok(mut a) = arr.try_borrow_mut() else {
            return Fast::No;
        };
        // (the general handler's error / deprecation cases first: nothing
        // is changed for them)
        if let Some(k) = &key {
            match a.get(k) {
                Some(Value::Array(_)) | None | Some(Value::Null) | Some(Value::Undef) => {}
                _ => return Fast::No,
            }
        }
        // An element is fetched for writing: the array's storage is its own
        // first (fetch_dim_from_array()).
        a.make_unique();
        let elem = match &key {
            None => {
                let rc = Rc::new(RefCell::new(crate::array::PhpArray::new()));
                // (a full array: the general handler's Error)
                if a.try_push(Value::Array(rc.clone())).is_err() {
                    return Fast::No;
                }
                rc
            }
            Some(k) => match a.get(k) {
                Some(Value::Array(e)) => {
                    let e = e.clone();
                    // (the container holds one count, `e` the other)
                    if Rc::strong_count(&e) > 2 {
                        let sep = Rc::new(RefCell::new(e.borrow().clone()));
                        a.set(k.clone(), Value::Array(sep.clone()));
                        sep
                    } else {
                        e
                    }
                }
                _ => {
                    let rc = Rc::new(RefCell::new(crate::array::PhpArray::new()));
                    a.set(k.clone(), Value::Array(rc.clone()));
                    rc
                }
            },
        };
        drop(a);
        self.str_wb_pending = None;
        self.str_offset_wb = None;
        if let Some(slot) = tmps.get_mut(rt as usize) {
            *slot = Value::Array(elem);
        }
        Fast::Done
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
            if self.native_class_handlers.contains_key(pl)
                && !native_plain_base(pl, &self.native_class_handlers)
            {
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
        let cvs: &[Value] = cvs;
        let op = &op_array.ops[ip];
        let literals = &op_array.literals;
        if !self.property_hook_stack.is_empty() {
            return Fast::No;
        }
        if let Some(f) = self.prop_site_hit(op_array, ip, op, cvs, tmps) {
            return f;
        }
        let opc = op.opcode;
        let write = opc == OpCode::PropertySet;
        // (the fetch of `isset($o->p[k])` / `isset($o->p->q)` answers the
        // same for a plain slot)
        let isset = matches!(opc, OpCode::PropertyIsset | OpCode::PropertyIssetFetch);
        // `$o->p[k] = v` / `$o->p[] = v` (see prop_dim_write)
        let w_fetch = opc == OpCode::PropertyGetW;
        // (the read of `$o->p op= v`: of a property its write may change)
        let rw = opc == OpCode::PropertyGetRW;
        // (the object of a variable is borrowed from it: nothing here
        // changes the variables)
        let held;
        let obj: &Rc<RefCell<crate::object::PhpObject>> = match op.op1 {
            // (a variable bound to a symbol table holds a reference)
            OperandType::Cv(ci) => match cvs.get(ci as usize) {
                Some(Value::Object(obj)) => obj,
                Some(Value::Reference(r)) => match r.try_borrow().as_deref() {
                    Ok(Value::Object(obj)) => {
                        held = obj.clone();
                        &held
                    }
                    _ => return Fast::No,
                },
                _ => return Fast::No,
            },
            // `$a->b->c`, `f()->p`: a read of an object held by a temporary
            // (one held elsewhere too: releasing the temporary then destroys
            // nothing)
            OperandType::Tmp(ti)
                if matches!(
                    opc,
                    OpCode::PropertyGet | OpCode::PropertyIsset | OpCode::PropertyIssetFetch
                ) =>
            {
                match tmps.get(ti as usize) {
                    Some(Value::Object(obj)) if Rc::strong_count(obj) > 1 => {
                        held = obj.clone();
                        &held
                    }
                    _ => return Fast::No,
                }
            }
            _ => return Fast::No,
        };
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
        match name {
            [] | [0, ..] | [b'_', b'_', ..] => return Fast::No,
            _ => {}
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
                Some(Value::Null) => Some(Value::Null),
                Some(Value::True) => Some(Value::True),
                Some(Value::False) => Some(Value::False),
                Some(Value::Long(n)) => Some(Value::Long(*n)),
                Some(Value::Double(d)) => Some(Value::Double(*d)),
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
        // (no internal state marker: see PropTable)
        if (ob.native.is_some() && !self.native_props_plain(&ob.class_name))
            || ob.closure.is_some()
            || ob.properties.has_internal()
        {
            return Fast::No;
        }
        let mangled = ob.properties.has_mangled();
        // What the site found last: whether the property is a plain one
        // (a function of the class and the property, for the scope:
        // classes do not change once declared) and the slot the object
        // held it in (a hint: checked against the slot's key).
        let scope = self.class_scope_ref();
        let mut sites = const_name.then(|| op_array.sites().borrow_mut());
        let mut site = sites
            .as_mut()
            .and_then(|g| g.as_mut())
            .and_then(|s| s.prop_slot(&op_array.ops, ip));
        let (cached, hint) = match &site {
            Some(c)
                if c.set
                    && key_eq(&c.class, &ob.class_name)
                    && match (&c.scope, scope) {
                        (None, None) => true,
                        (Some(a), Some(b)) => key_eq(a, b),
                        _ => false,
                    } =>
            {
                (Some(c.plain), c.slot as usize)
            }
            _ => (None, usize::MAX),
        };
        let idx = match ob.properties.get(hint) {
            Some((k, _)) if key_eq(k, name) => hint,
            _ => match ob.properties.iter().position(|(k, _)| key_eq(k, name)) {
                Some(i) => i,
                None => {
                    // isset() of a property a stdClass object does not have
                    // (it has no magic methods).
                    if isset && ob.class_name == b"stdClass" {
                        drop(ob);
                        return if store_tmp(&op.result, Value::False, tmps) {
                            Fast::Done
                        } else {
                            Fast::No
                        };
                    }
                    return Fast::No;
                }
            },
        };
        let cur = &ob.properties[idx].1;
        // An uninitialized slot, or (for a write) one that is a reference
        // or holds something releasing could care about (an object, which
        // may be destroyed, also when the cycle collector's buffer holds
        // it): only an array of a few plain values is overwritten.
        let ok = if write {
            self.prop_overwrite_quiet(cur)
        } else if w_fetch {
            matches!(cur, Value::Array(_))
        } else if (opc == OpCode::PropertyGetIncDec || rw) && matches!(cur, Value::Object(_)) {
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
        let plain = match cached {
            Some(p) => {
                if hint != idx {
                    if let Some(c) = site.as_mut() {
                        c.slot = idx as u32;
                    }
                }
                p
            }
            None => {
                let p = self.plain_public_prop(&ob.class_name, name, write || w_fetch || rw, scope);
                if let Some(c) = sites.as_mut().and_then(|g| {
                    g.get_or_insert_with(Default::default)
                        .prop_slot(&op_array.ops, ip)
                }) {
                    c.class.clear();
                    c.class.extend_from_slice(&ob.class_name);
                    c.scope = scope.map(|s| s.to_vec());
                    c.plain = p;
                    c.slot = idx as u32;
                    c.set = true;
                }
                p
            }
        };
        drop(sites);
        if plain == 0 || (plain == PLAIN_UNMANGLED && mangled) {
            return Fast::No;
        }
        if const_name {
            self.prop_ic_fill(op_array, ip, &ob, idx, plain, write || w_fetch || rw, scope, name);
        }
        if w_fetch {
            drop(ob);
            return match self.prop_dim_write(op_array, ip, obj, idx, cvs, tmps) {
                Fast::No => prop_fetch_w(obj, idx, op, tmps),
                f => f,
            };
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
    /// The read, isset() or write of `$cv->name` (a constant name), or the
    /// read of a temporary's object's, whose site found
    /// (see PropIc, filled by fast_prop_op()) a plain property for the
    /// object's class and the class scope, in the slot the object holds it
    /// in: what fast_prop_op() does then. None: nothing was done.
    #[inline(always)]
    fn prop_ic_hit(
        &self,
        op_array: &crate::opcode::OpArray,
        ip: usize,
        op: &Op,
        cvs: &[Value],
        tmps: &mut [Value],
    ) -> Option<Fast> {
        let ic = op_array.prop_ic(ip)?;
        let tag = ic.tag.get();
        if tag == 0 || !self.property_hook_stack.is_empty() {
            return None;
        }
        // (a variable bound to a symbol table holds a reference)
        let held;
        let tmp_obj;
        let obj = match op.op1 {
            OperandType::Cv(ci) => match cvs.get(ci as usize)? {
                Value::Object(o) => o,
                Value::Reference(r) => {
                    held = r.try_borrow().ok()?;
                    match &*held {
                        Value::Object(o) => o,
                        _ => return None,
                    }
                }
                _ => return None,
            },
            // `$a->b->c`, `f()->p`: a read of an object held by a temporary
            // (one held elsewhere too: releasing the temporary then destroys
            // nothing) (only reads have a PropIc for one)
            OperandType::Tmp(ti) => match tmps.get(ti as usize)? {
                Value::Object(o) if Rc::strong_count(o) > 1 => {
                    tmp_obj = o.clone();
                    &tmp_obj
                }
                _ => return None,
            },
            _ => return None,
        };
        let ob = obj.try_borrow().ok()?;
        // (no internal state marker: see PropTable)
        if ob.class_tag() != tag
            || (ob.native.is_some() && !ic.native_ok.get())
            || ob.closure.is_some()
            || ob.properties.has_internal()
            || (ic.plain.get() == PLAIN_UNMANGLED && ob.properties.has_mangled())
            || !(ic.any_scope.get() || self.prop_ic_scope_ok(ic))
        {
            return None;
        }
        let opc = op.opcode;
        let write = opc == OpCode::PropertySet;
        let is_isset = matches!(opc, OpCode::PropertyIsset | OpCode::PropertyIssetFetch);
        let OperandType::Const(ni) = (if write { op.result } else { op.op2 }) else {
            return None;
        };
        let Some(Value::String(name)) = op_array.literals.get(ni as usize) else {
            return None;
        };
        let slot = ic.slot.get() as usize;
        let (k, cur) = ob.properties.get(slot)?;
        if !short_bytes_eq(k, name.as_bytes()) {
            return None;
        }
        if write {
            // (a plain scalar replacing one: nothing to destroy)
            let src = match op.op2 {
                OperandType::Tmp(i) => tmps.get(i as usize),
                OperandType::Cv(i) => cvs.get(i as usize),
                OperandType::Const(i) => op_array.literals.get(i as usize),
                _ => None,
            };
            let v = match src? {
                Value::Null => Value::Null,
                Value::True => Value::True,
                Value::False => Value::False,
                Value::Long(n) => Value::Long(*n),
                Value::Double(d) => Value::Double(*d),
                Value::String(s) => Value::String(s.clone()),
                _ => Value::Undef,
            };
            let plain_cur = matches!(
                cur,
                Value::Null
                    | Value::True
                    | Value::False
                    | Value::Long(_)
                    | Value::Double(_)
                    | Value::String(_)
            );
            drop(ob);
            if matches!(v, Value::Undef) || !plain_cur {
                return self.prop_ic_write(obj, slot, op, &op_array.literals, cvs, tmps);
            }
            let mut obm = obj.try_borrow_mut().ok()?;
            set_plain(&mut obm.properties.get_mut(slot)?.1, v);
            return Some(Fast::Done);
        }
        match opc {
            // (++/-- of an object: the general handler checks readonly)
            OpCode::PropertyGetIncDec | OpCode::PropertyGetRW if matches!(cur, Value::Object(_)) => {
                return None;
            }
            OpCode::PropertyGetW => {
                if !matches!(cur, Value::Array(_)) {
                    return None;
                }
                drop(ob);
                return Some(match self.prop_dim_write(op_array, ip, obj, slot, cvs, tmps) {
                    Fast::No => prop_fetch_w(obj, slot, op, tmps),
                    f => f,
                });
            }
            _ => {}
        }
        let v = match cur {
            Value::Undef => return None,
            // (a property bound to a reference: its value)
            Value::Reference(r) => {
                let Ok(b) = r.try_borrow() else {
                    return None;
                };
                match &*b {
                    Value::Undef => return None,
                    v if is_isset => {
                        if matches!(v, Value::Null) {
                            Value::False
                        } else {
                            Value::True
                        }
                    }
                    v => v.clone(),
                }
            }
            v if is_isset => {
                if matches!(v, Value::Null) {
                    Value::False
                } else {
                    Value::True
                }
            }
            Value::Null => Value::Null,
            Value::True => Value::True,
            Value::False => Value::False,
            Value::Long(n) => Value::Long(*n),
            Value::Double(d) => Value::Double(*d),
            v => v.clone(),
        };
        drop(ob);
        let ok = match op.result {
            OperandType::Tmp(i) => match tmps.get_mut(i as usize) {
                Some(slot) if is_plain_scalar(slot) => {
                    // As write_operand(): objects / generators stored in
                    // a temporary are registered with the cycle collector.
                    match &v {
                        Value::Object(o) => crate::gc::track_object(o),
                        Value::Generator(g) => crate::gc::track_generator(g),
                        _ => {}
                    }
                    set_plain(slot, v);
                    true
                }
                _ => false,
            },
            OperandType::Unused => true,
            _ => false,
        };
        ok.then_some(Fast::Done)
    }

    /// prop_ic_hit() of a write of anything but a plain scalar over one:
    /// what fast_prop_op() does then (an object, or an array, a variable's
    /// or a literal's copied as the general handler's dup_stored_array(),
    /// over a plain scalar or an array of a few of them).
    #[inline(never)]
    fn prop_ic_write(
        &self,
        obj: &Rc<RefCell<crate::object::PhpObject>>,
        slot: usize,
        op: &Op,
        literals: &[Value],
        cvs: &[Value],
        tmps: &[Value],
    ) -> Option<Fast> {
        let src = match op.op2 {
            OperandType::Tmp(i) => tmps.get(i as usize),
            OperandType::Cv(i) => cvs.get(i as usize),
            OperandType::Const(i) => literals.get(i as usize),
            _ => None,
        };
        let v = match src? {
            v if is_plain_scalar(v) && !matches!(v, Value::Undef) => v.clone(),
            // (the value of a variable bound to a symbol table)
            Value::Reference(r) if matches!(op.op2, OperandType::Cv(_)) => {
                match r.try_borrow().as_deref() {
                    Ok(v) if is_plain_scalar(v) && !matches!(v, Value::Undef) => v.clone(),
                    _ => return None,
                }
            }
            Value::Object(o) => Value::Object(o.clone()),
            v @ Value::Array(a) if !Rc::ptr_eq(a, &self.globals) => {
                if matches!(op.op2, OperandType::Tmp(_)) {
                    v.clone()
                } else {
                    self.dup_array_value(v.clone())
                }
            }
            _ => return None,
        };
        let mut obm = obj.try_borrow_mut().ok()?;
        let cur = &mut obm.properties.get_mut(slot)?.1;
        if !self.prop_overwrite_quiet(cur) {
            return None;
        }
        if let Value::Object(o) = &v {
            crate::gc::track_object(o);
        }
        let old = std::mem::replace(cur, v);
        drop(obm);
        // (the overwritten array loses a holder)
        crate::gc::on_value_replaced(&old, &Value::Null);
        drop(old);
        Some(Fast::Done)
    }

    /// Whether a property's value `cur` overwritten by a write only frees
    /// memory: a plain scalar, an array of a few of them, or (when no
    /// destructor can be due, the cycle collector not due) what drops
    /// quietly (see drops_quietly_deep()). (An uninitialized slot, a
    /// reference...: the general handler.)
    fn prop_overwrite_quiet(&self, cur: &Value) -> bool {
        match cur {
            Value::Array(a) if Rc::ptr_eq(a, &self.globals) => false,
            Value::Array(a)
                if a.try_borrow()
                    .is_ok_and(|a| a.len() <= 8 && a.values().all(is_plain_scalar)) =>
            {
                true
            }
            Value::Array(_) | Value::Object(_) => {
                !self.has_release_work()
                    && !crate::gc::gc_pending()
                    && drops_quietly_deep(cur, &mut 256, &self.globals)
            }
            Value::Undef => false,
            v => is_plain_scalar(v),
        }
    }

    /// Whether the native data of an object of class `class_name` has
    /// nothing to do with its properties: a user class extending PDO or
    /// PDOStatement (their native handlers have no property hooks; their
    /// properties are plain slots, see plain_internal_base()).
    #[cold]
    #[inline(never)]
    fn native_props_plain(&self, class_name: &[u8]) -> bool {
        let mut cur = class_name.to_ascii_lowercase();
        for _ in 0..64 {
            match self.classes.get(cur.as_slice()) {
                Some(ce) if ce.filename.is_some() => match &ce.parent {
                    Some(p) => cur = p.to_ascii_lowercase(),
                    None => return false,
                },
                Some(_) => return false,
                None => return native_plain_base(&cur, &self.native_class_handlers),
            }
        }
        false
    }

    /// Whether the class scope is the one `ic` found its answer for.
    #[inline(always)]
    fn prop_ic_scope_ok(&self, ic: &crate::opcode::PropIc) -> bool {
        // (class_scope_ref())
        let scope = match self.class_scope_stack.last() {
            Some(s) if s.is_empty() => None,
            Some(s) => Some(s.as_slice()),
            None => self.called_class_stack.last().map(|s| s.as_slice()),
        };
        let Ok(want) = ic.scope.try_borrow() else {
            return false;
        };
        match (want.as_deref(), scope) {
            (None, None) => true,
            (Some(a), Some(b)) => short_bytes_eq(a, b),
            _ => false,
        }
    }

    /// What fast_prop_op() found at `ip` (a plain property of `ob`'s
    /// class, in slot `idx`, `plain` its plain_public_prop() in `scope`),
    /// for prop_ic_hit().
    #[cold]
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn prop_ic_fill(
        &self,
        op_array: &crate::opcode::OpArray,
        ip: usize,
        ob: &crate::object::PhpObject,
        idx: usize,
        plain: u8,
        write: bool,
        scope: Option<&[u8]>,
        name: &[u8],
    ) {
        let Some(ic) = op_array.prop_ic(ip) else {
            return;
        };
        let tag = ob.class_tag();
        if tag == u32::MAX || idx > u32::MAX as usize {
            return;
        }
        // (the answer outside any class is one for all scopes: only a
        // public property is accessible there)
        let Ok(mut s) = ic.scope.try_borrow_mut() else {
            return;
        };
        // (what it holds already: a write over something but a plain
        // value, a read of a reference...)
        if ic.tag.get() == tag
            && ic.slot.get() as usize == idx
            && ic.plain.get() == plain
            && ic.native_ok.get() == ob.native.is_some()
            && (ic.any_scope.get() || s.as_deref() == scope)
        {
            return;
        }
        let any = plain == PLAIN
            && (scope.is_none()
                || self.plain_public_prop(&ob.class_name, name, write, None) == PLAIN);
        *s = if any { None } else { scope.map(Box::from) };
        ic.tag.set(tag);
        ic.slot.set(idx as u32);
        ic.plain.set(plain);
        ic.any_scope.set(any);
        // (fast_prop_op() let the object's native data be: see
        // native_props_plain())
        ic.native_ok.set(ob.native.is_some());
    }

    /// What fast_prop_op() does for the commonest accesses, without its
    /// general checks: a read, isset() or plain scalar write of `$cv->name`
    /// (a constant name) whose site found (for this class and scope) a plain
    /// property, in the slot the object holds it in. None: fast_prop_op()
    /// looks at it in full (nothing was done). (Out of line: a small
    /// function of its own, not one more path through fast_prop_op's
    /// registers.)
    #[inline(never)]
    fn prop_site_hit(
        &self,
        op_array: &crate::opcode::OpArray,
        ip: usize,
        op: &Op,
        cvs: &[Value],
        tmps: &mut [Value],
    ) -> Option<Fast> {
        let OperandType::Cv(ci) = op.op1 else {
            return None;
        };
        let Some(Value::Object(obj)) = cvs.get(ci as usize) else {
            return None;
        };
        let opc = op.opcode;
        let write = opc == OpCode::PropertySet;
        let name_op = if write { &op.result } else { &op.op2 };
        let OperandType::Const(ni) = *name_op else {
            return None;
        };
        let Some(Value::String(name)) = op_array.literals.get(ni as usize) else {
            return None;
        };
        let name = name.as_bytes();
        // (the value of a write: a plain scalar but a string)
        let new_val = if write {
            let src = match op.op2 {
                OperandType::Tmp(i) => tmps.get(i as usize),
                OperandType::Cv(i) => cvs.get(i as usize),
                OperandType::Const(i) => op_array.literals.get(i as usize),
                _ => None,
            };
            Some(match src? {
                Value::Null => Value::Null,
                Value::True => Value::True,
                Value::False => Value::False,
                Value::Long(n) => Value::Long(*n),
                Value::Double(d) => Value::Double(*d),
                _ => return None,
            })
        } else {
            if !matches!(
                opc,
                OpCode::PropertyGet | OpCode::PropertyIsset | OpCode::PropertyIssetFetch
            ) || !tmp_writable(&op.result, tmps)
            {
                return None;
            }
            None
        };
        let ob = obj.try_borrow().ok()?;
        if ob.native.is_some() || ob.closure.is_some() || ob.properties.has_internal() {
            return None;
        }
        // (class_scope_ref())
        let scope = match self.class_scope_stack.last() {
            Some(s) if s.is_empty() => None,
            Some(s) => Some(s.as_slice()),
            None => self.called_class_stack.last().map(|s| s.as_slice()),
        };
        let idx = {
            let mut sites = op_array.sites().try_borrow_mut().ok()?;
            let c = sites.as_mut()?.prop_slot(&op_array.ops, ip)?;
            if !(c.set
                && (c.plain == PLAIN
                    || (c.plain == PLAIN_UNMANGLED && !ob.properties.has_mangled()))
                && key_eq(&c.class, &ob.class_name)
                && match (&c.scope, scope) {
                    (None, None) => true,
                    (Some(a), Some(b)) => key_eq(a, b),
                    _ => false,
                })
            {
                return None;
            }
            c.slot as usize
        };
        let (k, cur) = ob.properties.get(idx)?;
        if !key_eq(k, name) {
            return None;
        }
        if let Some(v) = new_val {
            // (an overwritten plain scalar but a string: nothing to release)
            if !matches!(
                cur,
                Value::Null | Value::True | Value::False | Value::Long(_) | Value::Double(_)
            ) {
                return None;
            }
            drop(ob);
            let mut obm = obj.try_borrow_mut().ok()?;
            set_plain(&mut obm.properties[idx].1, v);
            return Some(Fast::Done);
        }
        let v = match cur {
            Value::Undef | Value::Reference(_) => return None,
            v if opc != OpCode::PropertyGet => {
                if matches!(v, Value::Null) {
                    Value::False
                } else {
                    Value::True
                }
            }
            Value::Null => Value::Null,
            Value::True => Value::True,
            Value::False => Value::False,
            Value::Long(n) => Value::Long(*n),
            Value::Double(d) => Value::Double(*d),
            v => v.clone(),
        };
        drop(ob);
        match &v {
            Value::Object(o) => crate::gc::track_object(o),
            Value::Generator(g) => crate::gc::track_generator(g),
            _ => {}
        }
        Some(if store_tmp(&op.result, v, tmps) {
            Fast::Done
        } else {
            Fast::No
        })
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
            None => {
                return matches!(c.name, "Exception" | "Error" | "PDO" | "PDOStatement");
            }
            Some(p) => cur = crate::stubs::find_class(p.as_bytes()),
        }
    }
    false
}

/// Whether the internal class `lower` is PDO or PDOStatement, whose native
/// handler (if registered) has no property hooks: its objects' native data
/// (the connection, the statement) is not what their properties are.
fn native_plain_base(
    lower: &[u8],
    handlers: &crate::fxhash::FxHashMap<Vec<u8>, crate::vm::NativeClassHandler>,
) -> bool {
    matches!(lower, b"pdo" | b"pdostatement")
        && handlers.get(lower).is_none_or(|h| {
            h.prop_read.is_none() && h.prop_write.is_none() && h.prop_added.is_none()
        })
}

/// Whether `c` is a call of call_user_func() or call_user_func_array() by
/// that plain name (not a dynamic call, not through a namespace fallback):
/// its arguments are all sent by value.
pub(crate) fn is_direct_ucf(c: &crate::vm::PendingCall) -> bool {
    let t = &c.target;
    if !t.has_builtin || t.method.is_some() || t.dynamic || t.closure {
        return false;
    }
    let n = c.name.as_bytes();
    n.eq_ignore_ascii_case(b"call_user_func") || n.eq_ignore_ascii_case(b"call_user_func_array")
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
