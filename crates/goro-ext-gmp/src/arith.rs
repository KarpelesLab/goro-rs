//! Signed arithmetic that borrows the operands' magnitudes.
//!
//! puremp's `Int` operators copy heap-backed operands into owned
//! magnitudes before working on them; these wrappers keep its single-limb
//! fast paths, handle single-limb overflow with 128-bit arithmetic, and
//! otherwise run the `Nat` algorithms on borrowed magnitudes.

use std::cmp::Ordering;

use puremp::{Int, Nat, Sign};

#[inline]
fn small(n: &Int) -> Option<(bool, u64)> {
    (n.limbs().len() <= 1).then(|| (n.is_negative(), n.least_significant_limb()))
}

#[inline]
fn signed(neg: bool, mag: Nat) -> Int {
    Int::from_sign_magnitude(if neg { Sign::Negative } else { Sign::Positive }, mag)
}

#[inline]
fn signed_u128(neg: bool, mag: u128) -> Int {
    let v = Int::from_u128(mag);
    if neg { v.neg() } else { v }
}

fn add_signed(an: bool, a: &Int, bn: bool, b: &Int) -> Int {
    if let (Some((_, x)), Some((_, y))) = (small(a), small(b)) {
        return if an == bn {
            signed_u128(an, x as u128 + y as u128)
        } else if x >= y {
            signed_u128(an, (x - y) as u128)
        } else {
            signed_u128(bn, (y - x) as u128)
        };
    }
    if a.is_zero() {
        return if bn { b.abs().neg() } else { b.abs() };
    }
    if b.is_zero() {
        return a.clone();
    }
    // A heap value and a single limb: one pass over the limbs.
    if let Some((_, y)) = small(b) {
        return limb_add(an, a.limbs(), an == bn, y);
    }
    if let Some((_, x)) = small(a) {
        return limb_add(bn, b.limbs(), an == bn, x);
    }
    let (x, y) = (a.magnitude_ref(), b.magnitude_ref());
    if an == bn {
        return signed(an, x.add(&y));
    }
    match x.cmp(&y) {
        Ordering::Equal => Int::ZERO,
        Ordering::Greater => signed(an, x.checked_sub(&y).unwrap_or_default()),
        Ordering::Less => signed(bn, y.checked_sub(&x).unwrap_or_default()),
    }
}

/// `±(|x| + y)` (`same`) or `±(|x| - y)` for a magnitude `x` of at least
/// two limbs (so `|x| > y`); the sign is `neg`.
fn limb_add(neg: bool, x: &[u64], same: bool, y: u64) -> Int {
    let mut stack = [0u64; 9];
    let mut heap;
    let out: &mut [u64] = if x.len() < stack.len() {
        &mut stack[..x.len() + 1]
    } else {
        heap = vec![0u64; x.len() + 1];
        &mut heap
    };
    let mut carry = y;
    for (o, &l) in out.iter_mut().zip(x) {
        if same {
            let (s, c) = l.overflowing_add(carry);
            *o = s;
            carry = c as u64;
        } else {
            let (s, c) = l.overflowing_sub(carry);
            *o = s;
            carry = c as u64;
        }
    }
    if same {
        out[x.len()] = carry;
    }
    Int::from_limbs(if neg { Sign::Negative } else { Sign::Positive }, out)
}

/// `a + b`
#[inline]
pub(crate) fn add(a: &Int, b: &Int) -> Int {
    if small(a).is_some() && small(b).is_some() {
        return a.add(b);
    }
    add_signed(a.is_negative(), a, b.is_negative(), b)
}

/// `a - b`
#[inline]
pub(crate) fn sub(a: &Int, b: &Int) -> Int {
    add_signed(a.is_negative(), a, !b.is_negative() && !b.is_zero(), b)
}

/// `a · b`
#[inline]
pub(crate) fn mul(a: &Int, b: &Int) -> Int {
    if let (Some((an, x)), Some((bn, y))) = (small(a), small(b)) {
        if x == 0 || y == 0 {
            return Int::ZERO;
        }
        return signed_u128(an != bn, x as u128 * y as u128);
    }
    if a.is_zero() || b.is_zero() {
        return Int::ZERO;
    }
    signed(
        a.is_negative() != b.is_negative(),
        a.magnitude_ref().mul(&b.magnitude_ref()),
    )
}

/// Truncated division `(q, r)`, `r` with the sign of `a`; `b` non-zero.
pub(crate) fn div_rem_trunc(a: &Int, b: &Int) -> (Int, Int) {
    let qneg = a.is_negative() != b.is_negative();
    if let (Some((an, x)), Some((_, y))) = (small(a), small(b)) {
        let (q, r) = (x / y, x % y);
        let q = Int::from_u64(q);
        let r = Int::from_u64(r);
        return (if qneg { q.neg() } else { q }, if an { r.neg() } else { r });
    }
    let (q, r) = a
        .magnitude_ref()
        .div_rem(&b.magnitude_ref())
        .unwrap_or_default();
    let q = if q.is_zero() {
        Int::ZERO
    } else {
        signed(qneg, q)
    };
    let r = if r.is_zero() {
        Int::ZERO
    } else {
        signed(a.is_negative(), r)
    };
    (q, r)
}

/// The remainder in `[0, |b|)`; `b` non-zero.
pub(crate) fn rem_euclid(a: &Int, b: &Int) -> Int {
    let (_, r) = div_rem_trunc(a, b);
    if r.is_negative() {
        add_signed(true, &r, false, &b.abs())
    } else {
        r
    }
}
