//! Port of php-src's libbcmath (ext/bcmath/libbcmath/src): arbitrary
//! precision decimal numbers as BCD digit strings.
//!
//! The digit layout, the scales of intermediate and final results and the
//! truncation rules follow the C library exactly (they decide how many
//! digits results keep, and the Newton iteration of bc_sqrt() depends on
//! them). The vectorized multiplication / division kernels are replaced by
//! num-bigint, which computes the same exact integer products and
//! truncated quotients.

use puremp::{Int, Nat};

/// A bc_num: `v` holds `len` integer digits followed by `scale` fraction
/// digits (values 0-9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Num {
    pub neg: bool,
    pub len: usize,
    pub scale: usize,
    pub v: Vec<u8>,
}

pub const EQUAL: i32 = 0;
pub const LEFT_GREATER: i32 = 1;
pub const RIGHT_GREATER: i32 = -1;

impl Num {
    /// bc_new_num(): a zero with the given shape.
    pub fn new(len: usize, scale: usize) -> Num {
        Num {
            neg: false,
            len,
            scale,
            v: vec![0; len + scale],
        }
    }

    pub fn zero() -> Num {
        Num::new(1, 0)
    }

    pub fn one() -> Num {
        Num {
            neg: false,
            len: 1,
            scale: 0,
            v: vec![1],
        }
    }

    pub fn two() -> Num {
        Num {
            neg: false,
            len: 1,
            scale: 0,
            v: vec![2],
        }
    }

    /// `n_scale = MIN(scale, n_scale)`
    pub fn trunc_scale(&mut self, scale: usize) {
        if scale < self.scale {
            self.scale = scale;
            self.v.truncate(self.len + scale);
        }
    }

    /// _bc_rm_leading_zeros()
    pub fn rm_leading_zeros(&mut self) {
        let mut n = 0;
        while n < self.v.len() && self.v[n] == 0 && self.len - n > 1 {
            n += 1;
        }
        if n > 0 {
            self.v.drain(..n);
            self.len -= n;
        }
    }

    /// bc_rm_trailing_zeros()
    pub fn rm_trailing_zeros(&mut self) {
        while self.scale > 0 && self.v[self.len + self.scale - 1] == 0 {
            self.scale -= 1;
        }
        self.v.truncate(self.len + self.scale);
    }

    /// bc_is_zero_for_scale()
    pub fn is_zero_for_scale(&self, scale: usize) -> bool {
        let count = (self.len + scale).min(self.v.len());
        self.v[..count].iter().all(|&d| d == 0)
    }

    /// bc_is_zero()
    pub fn is_zero(&self) -> bool {
        self.is_zero_for_scale(self.scale)
    }

    /// bc_is_near_zero(): all but the last digit (by scale) are 0 and the
    /// last one is 0 or 1.
    pub fn is_near_zero(&self, scale: usize) -> bool {
        let scale = scale.min(self.scale);
        let count = self.len + scale;
        let mut i = 0;
        while i < count && self.v[i] == 0 {
            i += 1;
        }
        let rest = count - i;
        rest == 0 || (rest == 1 && self.v[i] == 1)
    }

    fn sign_fixed_zero(mut self) -> Num {
        if self.is_zero() {
            self.neg = false;
            self.scale = 0;
            self.v.truncate(self.len);
        }
        self
    }
}

/// bc_str2num(): parse a decimal number ([+-]digits[.digits]). Returns
/// None for a malformed string. `full_scale` is the number of fraction
/// digits as written (trailing zeros included); with `auto_scale` false the
/// fraction is truncated to `scale` digits. A NUL byte ends the string, as
/// in C.
pub fn str2num(s: &[u8], scale: usize, auto_scale: bool) -> (Option<Num>, usize) {
    let at = |i: usize| -> u8 { s.get(i).copied().unwrap_or(0) };
    let mut ptr = 0usize;
    let mut full_scale = 0usize;
    if at(ptr) == b'+' || at(ptr) == b'-' {
        ptr += 1;
    }
    while at(ptr) == b'0' {
        ptr += 1;
    }
    let integer_ptr = ptr;
    while at(ptr).is_ascii_digit() {
        ptr += 1;
    }
    let mut digits = ptr - integer_ptr;
    let decimal_point = at(ptr) == b'.';
    if !decimal_point && at(ptr) != 0 {
        return (None, full_scale);
    }
    let mut fractional_ptr = 0;
    let mut fractional_end = 0;
    let mut str_scale = 0usize;
    if decimal_point {
        fractional_ptr = ptr + 1;
        fractional_end = fractional_ptr;
        if at(fractional_ptr) != 0 {
            while at(fractional_end).is_ascii_digit() {
                fractional_end += 1;
            }
            if at(fractional_end) != 0 {
                return (None, full_scale);
            }
            full_scale = fractional_end - fractional_ptr;
            while fractional_end > fractional_ptr && at(fractional_end - 1) == b'0' {
                fractional_end -= 1;
            }
            str_scale = fractional_end - fractional_ptr;
            if str_scale > scale && !auto_scale {
                fractional_end -= str_scale - scale;
                str_scale = scale;
                if str_scale > 0 {
                    let before = fractional_end;
                    while fractional_end > fractional_ptr && at(fractional_end - 1) == b'0' {
                        fractional_end -= 1;
                    }
                    str_scale -= before - fractional_end;
                }
            }
        }
    }
    if digits + str_scale == 0 {
        return (Some(Num::zero()), full_scale);
    }
    let mut zero_int = false;
    if digits == 0 {
        zero_int = true;
        digits = 1;
    }
    let mut v = Vec::with_capacity(digits + str_scale);
    if zero_int {
        v.push(0);
    } else {
        v.extend(
            s[integer_ptr..integer_ptr + digits]
                .iter()
                .map(|c| c - b'0'),
        );
    }
    if str_scale > 0 {
        v.extend(s[fractional_ptr..fractional_end].iter().map(|c| c - b'0'));
    }
    let n = Num {
        neg: at(0) == b'-',
        len: digits,
        scale: str_scale,
        v,
    };
    (Some(n), full_scale)
}

/// bc_num2str_ex()
pub fn num2str(n: &Num, scale: usize) -> Vec<u8> {
    let min_scale = n.scale.min(scale);
    let signch = n.neg && !n.is_zero_for_scale(min_scale);
    let mut out = Vec::with_capacity(n.len + scale + 2);
    if signch {
        out.push(b'-');
    }
    out.extend(n.v[..n.len].iter().map(|d| d + b'0'));
    if scale > 0 {
        out.push(b'.');
        out.extend(n.v[n.len..n.len + min_scale].iter().map(|d| d + b'0'));
        out.resize(out.len() + scale.saturating_sub(n.scale), b'0');
    }
    out
}

/// bc_long2num()
pub fn long2num(l: i64) -> Num {
    if l == 0 {
        return Num::zero();
    }
    let neg = l < 0;
    let digits: Vec<u8> = l
        .unsigned_abs()
        .to_string()
        .bytes()
        .map(|c| c - b'0')
        .collect();
    Num {
        neg,
        len: digits.len(),
        scale: 0,
        v: digits,
    }
}

/// bc_num2long(): the integer part, 0 when it does not fit.
pub fn num2long(n: &Num) -> i64 {
    let mut val: i64 = 0;
    for &d in &n.v[..n.len] {
        if val > i64::MAX / 10 {
            return 0;
        }
        val *= 10;
        if val > i64::MAX - d as i64 {
            return 0;
        }
        val += d as i64;
    }
    if n.neg { -val } else { val }
}

/// _bc_do_compare()
pub fn do_compare(n1: &Num, n2: &Num, scale: usize, use_sign: bool) -> i32 {
    let res = |left_abs_greater: bool| -> i32 {
        if left_abs_greater == (!use_sign || !n1.neg) {
            LEFT_GREATER
        } else {
            RIGHT_GREATER
        }
    };
    if use_sign && n1.neg != n2.neg {
        if (n1.scale > scale || n2.scale > scale)
            && n1.len == 1
            && n2.len == 1
            && n1.v[0] == 0
            && n2.v[0] == 0
            && n1.is_zero_for_scale(scale)
            && n2.is_zero_for_scale(scale)
        {
            return EQUAL;
        }
        return if !n1.neg { LEFT_GREATER } else { RIGHT_GREATER };
    }
    if n1.len != n2.len {
        return res(n1.len > n2.len);
    }
    let n1_scale = n1.scale.min(scale);
    let n2_scale = n2.scale.min(scale);
    let count = n1.len + n1_scale.min(n2_scale);
    for i in 0..count {
        if n1.v[i] != n2.v[i] {
            return res(n1.v[i] > n2.v[i]);
        }
    }
    if n1_scale > n2_scale {
        if n1.v[count..n1.len + n1_scale].iter().any(|&d| d != 0) {
            return res(true);
        }
    } else if n2_scale > n1_scale && n2.v[count..n2.len + n2_scale].iter().any(|&d| d != 0) {
        return res(false);
    }
    EQUAL
}

/// bc_compare()
pub fn compare(n1: &Num, n2: &Num, scale: usize) -> i32 {
    do_compare(n1, n2, scale, true)
}

/// Digits of `n` aligned to `len` integer and `scale` fraction digits.
fn aligned(n: &Num, len: usize, scale: usize) -> Vec<u8> {
    let mut out = vec![0u8; len - n.len];
    out.extend_from_slice(&n.v);
    out.resize(len + scale, 0);
    out
}

/// _bc_do_add(): |n1| + |n2|.
pub fn do_add(n1: &Num, n2: &Num) -> Num {
    let sum_len = n1.len.max(n2.len) + 1;
    let sum_scale = n1.scale.max(n2.scale);
    let a = aligned(n1, sum_len, sum_scale);
    let b = aligned(n2, sum_len, sum_scale);
    let mut v = vec![0u8; sum_len + sum_scale];
    let mut carry = 0u8;
    for i in (0..v.len()).rev() {
        let mut d = a[i] + b[i] + carry;
        if d >= 10 {
            d -= 10;
            carry = 1;
        } else {
            carry = 0;
        }
        v[i] = d;
    }
    let mut n = Num {
        neg: false,
        len: sum_len,
        scale: sum_scale,
        v,
    };
    n.rm_leading_zeros();
    n
}

/// _bc_do_sub(): |n1| - |n2|, with |n1| >= |n2|.
pub fn do_sub(n1: &Num, n2: &Num) -> Num {
    let diff_len = n1.len.max(n2.len);
    let diff_scale = n1.scale.max(n2.scale);
    let a = aligned(n1, diff_len, diff_scale);
    let b = aligned(n2, diff_len, diff_scale);
    let mut v = vec![0u8; diff_len + diff_scale];
    let mut borrow = 0i32;
    for i in (0..v.len()).rev() {
        let mut d = a[i] as i32 - b[i] as i32 - borrow;
        if d < 0 {
            d += 10;
            borrow = 1;
        } else {
            borrow = 0;
        }
        v[i] = d as u8;
    }
    let mut n = Num {
        neg: false,
        len: diff_len,
        scale: diff_scale,
        v,
    };
    n.rm_leading_zeros();
    n
}

/// bc_add()
pub fn add(n1: &Num, n2: &Num, scale_min: usize) -> Num {
    if n1.neg == n2.neg {
        let mut sum = do_add(n1, n2);
        sum.neg = n1.neg;
        return sum;
    }
    match do_compare(n1, n2, scale_min, false) {
        RIGHT_GREATER => {
            let mut sum = do_sub(n2, n1);
            sum.neg = n2.neg;
            sum
        }
        EQUAL => Num::new(1, scale_min.max(n1.scale.max(n2.scale))),
        _ => {
            let mut sum = do_sub(n1, n2);
            sum.neg = n1.neg;
            sum
        }
    }
}

/// bc_sub()
pub fn sub(n1: &Num, n2: &Num, scale_min: usize) -> Num {
    if n1.neg != n2.neg {
        let mut diff = do_add(n1, n2);
        diff.neg = n1.neg;
        return diff;
    }
    match do_compare(n1, n2, scale_min, false) {
        RIGHT_GREATER => {
            let mut diff = do_sub(n2, n1);
            diff.neg = !n2.neg;
            diff
        }
        EQUAL => Num::new(1, scale_min.max(n1.scale.max(n2.scale))),
        _ => {
            let mut diff = do_sub(n1, n2);
            diff.neg = n1.neg;
            diff
        }
    }
}

// Decimal digit-value <-> Nat conversions. puremp only converts through
// ASCII strings, and its parser / printer lose to plain limb loops below a
// few thousand digits (the common bcmath sizes), so both directions are
// done here on u64 limbs; huge inputs still use puremp's sub-quadratic
// parser and its division for the printer's divide-and-conquer split.

const P19: u64 = 10_000_000_000_000_000_000;
/// floor((2^128 - 1) / 10^19) - 2^64; 10^19 > 2^63 is already normalized.
const P19_INV: u64 = ((u128::MAX / P19 as u128) - (1u128 << 64)) as u64;
/// Digit count above which puremp's sub-quadratic parser wins.
const PARSE_SCHOOLBOOK_DIGITS: usize = 30_000;
/// Limb count at or below which the printer divides limb by limb.
const PRINT_BASE_LIMBS: usize = 16;

/// (u1:u0) / 10^19 for u1 < 10^19 (Möller-Granlund 2-by-1 division with a
/// precomputed reciprocal), as (quotient, remainder).
#[inline]
fn div_p19(u1: u64, u0: u64) -> (u64, u64) {
    let q = (P19_INV as u128) * (u1 as u128) + (((u1 as u128) << 64) | u0 as u128);
    let mut q1 = ((q >> 64) as u64).wrapping_add(1);
    let q0 = q as u64;
    let mut r = u0.wrapping_sub(q1.wrapping_mul(P19));
    if r > q0 {
        q1 = q1.wrapping_sub(1);
        r = r.wrapping_add(P19);
    }
    if r >= P19 {
        q1 += 1;
        r -= P19;
    }
    (q1, r)
}

/// The value of a big-endian run of decimal digit values (0-9).
fn to_big(digits: &[u8]) -> Nat {
    let Some(start) = digits.iter().position(|&d| d != 0) else {
        return Nat::zero();
    };
    let d = &digits[start..];
    if d.len() > PARSE_SCHOOLBOOK_DIGITS {
        let s: String = d.iter().map(|&x| (b'0' + x) as char).collect();
        return s.parse().unwrap_or_default();
    }
    // Horner over 19-digit chunks on little-endian u64 limbs.
    let chunk = |c: &[u8]| c.iter().fold(0u64, |acc, &x| acc * 10 + x as u64);
    let first = d.len() % 19;
    let mut limbs: Vec<u64> = Vec::with_capacity(d.len() / 19 + 1);
    if first > 0 {
        limbs.push(chunk(&d[..first]));
    }
    for c in d[first..].as_chunks::<19>().0 {
        let mut carry = chunk(c) as u128;
        for l in limbs.iter_mut() {
            let t = *l as u128 * P19 as u128 + carry;
            *l = t as u64;
            carry = t >> 64;
        }
        if carry != 0 {
            limbs.push(carry as u64);
        }
    }
    Nat::from_limbs(&limbs)
}

/// Fills `out` with the decimal digits of `limbs`, zero-padded on the left
/// (the value must fit in out.len() digits).
fn digits_base_case(limbs: &[u64], out: &mut [u8]) {
    let mut limbs = limbs.to_vec();
    let mut pos = out.len();
    while let Some(&top) = limbs.last() {
        if top == 0 {
            limbs.pop();
            continue;
        }
        let mut rem = 0u64;
        for l in limbs.iter_mut().rev() {
            let (q, r) = div_p19(rem, *l);
            *l = q;
            rem = r;
        }
        for _ in 0..19 {
            if pos == 0 {
                break;
            }
            pos -= 1;
            out[pos] = (rem % 10) as u8;
            rem /= 10;
        }
    }
    out[..pos].fill(0);
}

/// Divide-and-conquer digit split; `powers[k] = (10^d, d)` with
/// d = 19 * 2^k.
fn digits_rec(v: &Nat, powers: &[(Nat, usize)], out: &mut [u8]) {
    if v.as_limbs().len() <= PRINT_BASE_LIMBS {
        digits_base_case(v.as_limbs(), out);
        return;
    }
    let Some(k) = powers.iter().rposition(|(_, d)| *d < out.len()) else {
        digits_base_case(v.as_limbs(), out);
        return;
    };
    let (p, d) = &powers[k];
    let (q, r) = v.div_rem(p).unwrap_or_default();
    let (hi, lo) = out.split_at_mut(out.len() - d);
    digits_rec(&q, powers, hi);
    digits_rec(&r, powers, lo);
}

/// The minimal big-endian decimal digit values of `b` ([0] for zero).
fn big_digits(b: &Nat) -> Vec<u8> {
    // bit_len * log10(2) + 1 digits always suffice.
    let nd = (b.bit_len() as f64 * std::f64::consts::LOG10_2) as usize + 2;
    let mut out = vec![0u8; nd];
    if b.as_limbs().len() <= PRINT_BASE_LIMBS {
        digits_base_case(b.as_limbs(), &mut out);
    } else {
        let mut powers = vec![(Nat::from_u64(P19), 19usize)];
        while let Some((p, d)) = powers.last()
            && d * 2 < nd
        {
            let next = (p.square(), d * 2);
            powers.push(next);
        }
        digits_rec(b, &powers, &mut out);
    }
    let lead = out.iter().position(|&d| d != 0).unwrap_or(out.len() - 1);
    out.drain(..lead);
    out
}

/// The decimal digits of `b`, right-aligned in `width` digits.
fn from_big(b: &Nat, width: usize) -> Vec<u8> {
    let mut out = vec![0u8; width];
    if b.is_zero() {
        return out;
    }
    let d = big_digits(b);
    if d.len() >= width {
        out.copy_from_slice(&d[d.len() - width..]);
    } else {
        out[width - d.len()..].copy_from_slice(&d);
    }
    out
}

/// bc_multiply()
pub fn multiply(n1: &Num, n2: &Num, scale: usize) -> Num {
    let len1 = n1.len + n1.scale;
    let len2 = n2.len + n2.scale;
    let full_scale = n1.scale + n2.scale;
    let prod_scale = full_scale.min(scale.max(n1.scale.max(n2.scale)));
    let prodlen = len1 + len2;
    let p = to_big(&n1.v).mul(&to_big(&n2.v));
    let mut v = from_big(&p, prodlen);
    let len = prodlen - full_scale;
    v.truncate(len + prod_scale);
    let mut prod = Num {
        neg: n1.neg != n2.neg,
        len,
        scale: prod_scale,
        v,
    };
    prod.rm_leading_zeros();
    prod.sign_fixed_zero()
}

/// bc_divide(): the quotient truncated to `scale` fraction digits, None
/// for a zero divisor.
pub fn divide(numerator: &Num, divisor: &Num, scale: usize) -> Option<Num> {
    if divisor.is_zero() {
        return None;
    }
    let quot_scale = scale;
    if numerator.is_zero() {
        return Some(Num::zero());
    }
    let neg = numerator.neg != divisor.neg;
    if do_compare(divisor, &Num::one(), divisor.scale, false) == EQUAL {
        let qs = numerator.scale.min(quot_scale);
        return Some(Num {
            neg,
            len: numerator.len,
            scale: qs,
            v: numerator.v[..numerator.len + qs].to_vec(),
        });
    }
    let nd = &numerator.v;
    let mut numerator_size = numerator.len + quot_scale + divisor.scale;
    let mut nlz = 0;
    while nd[nlz] == 0 {
        nlz += 1;
        if nlz == numerator_size {
            return Some(Num::zero());
        }
    }
    numerator_size -= nlz;
    let mut dlz = 0;
    while divisor.v[dlz] == 0 {
        dlz += 1;
    }
    let mut divisor_size = divisor.len + divisor.scale - dlz;
    if divisor_size > numerator_size {
        return Some(Num::zero());
    }
    let dd = &divisor.v[dlz..];
    let mut dtz = 0;
    let mut i = divisor_size - 1;
    while i > 0 {
        if dd[i] != 0 {
            break;
        }
        dtz += 1;
        i -= 1;
    }
    divisor_size -= dtz;
    numerator_size -= dtz;
    let quot_size = numerator_size - divisor_size + 1;
    let (qlen, qscale) = if quot_size > quot_scale {
        (quot_size - quot_scale, quot_scale)
    } else {
        (1, quot_scale)
    };
    let readable = nd.len() - nlz;
    if divisor_size == 1 && dd[0] == 1 {
        // bc_divide_by_pow_10()
        let mut q = vec![0u8; qlen + qscale];
        let mut qptr = 0;
        let mut i = quot_size;
        while i <= quot_scale {
            q[qptr] = 0;
            qptr += 1;
            i += 1;
        }
        let use_size = quot_size.min(readable);
        q[qptr..qptr + use_size].copy_from_slice(&nd[nlz..nlz + use_size]);
        qptr += use_size;
        let mut n = Num {
            neg,
            len: qlen,
            scale: qscale,
            v: q,
        };
        if use_size < qlen {
            n.scale = 0;
            n.v.truncate(qlen);
        } else {
            let qend = qlen + qscale;
            n.scale -= qend - qptr;
            n.v.truncate(qptr);
        }
        return Some(n);
    }
    let mut ndig = nd[nlz..nlz + numerator_size.min(readable)].to_vec();
    ndig.resize(numerator_size, 0);
    let q = to_big(&ndig)
        .div_rem(&to_big(&dd[..divisor_size]))
        .map(|(q, _)| q)
        .unwrap_or_default();
    let mut n = Num {
        neg,
        len: qlen,
        scale: qscale,
        v: from_big(&q, qlen + qscale),
    };
    n.rm_leading_zeros();
    Some(n.sign_fixed_zero())
}

/// bc_divmod(): (truncated integer quotient, remainder).
pub fn divmod(num1: &Num, num2: &Num, scale: usize) -> Option<(Num, Num)> {
    if num2.is_zero() {
        return None;
    }
    let rscale = num1.scale.max(num2.scale + scale);
    let temp = divide(num1, num2, 0)?;
    let quot = temp.clone();
    let temp = multiply(&temp, num2, rscale);
    let mut rem = sub(num1, &temp, rscale);
    rem.trunc_scale(scale);
    if rem.is_zero() {
        rem.neg = false;
        rem.scale = 0;
        rem.v.truncate(rem.len);
    }
    Some((quot, rem))
}

/// bc_modulo()
pub fn modulo(num1: &Num, num2: &Num, scale: usize) -> Option<Num> {
    divmod(num1, num2, scale).map(|(_, r)| r)
}

/// bc_floor_or_ceil()
pub fn floor_or_ceil(num: &Num, is_floor: bool) -> Num {
    let mut result = Num {
        neg: num.neg,
        len: num.len,
        scale: 0,
        v: num.v[..num.len].to_vec(),
    };
    if !(num.scale == 0 || result.neg != is_floor) {
        let frac_nonzero = num.v[num.len..num.len + num.scale].iter().any(|&d| d != 0);
        if frac_nonzero {
            let mut tmp = do_add(&result, &Num::one());
            tmp.neg = result.neg;
            result = tmp;
        }
    }
    if result.is_zero() {
        result.neg = false;
    }
    result
}

pub const ROUND_HALF_UP: i64 = 1;
pub const ROUND_HALF_DOWN: i64 = 2;
pub const ROUND_HALF_EVEN: i64 = 3;
pub const ROUND_HALF_ODD: i64 = 4;
pub const ROUND_CEILING: i64 = 5;
pub const ROUND_FLOOR: i64 = 6;
pub const ROUND_TOWARD_ZERO: i64 = 7;
pub const ROUND_AWAY_FROM_ZERO: i64 = 8;

/// bc_round(): (result, scale of the result).
/// The bc_new_num() size (sizeof(bc_struct) + digits) of bc_round()'s
/// result when it is a padded copy or a power of ten, for the memory
/// limit check before a huge precision is honoured.
pub fn round_alloc_size(num: &Num, precision: i64, mode: i64) -> Option<u64> {
    if precision < 0 && (num.len as u64) < precision.unsigned_abs() {
        let ten_power = match mode {
            ROUND_AWAY_FROM_ZERO => true,
            ROUND_CEILING => !num.neg,
            ROUND_FLOOR => num.neg,
            _ => false,
        };
        if !ten_power || num.is_zero() {
            return None;
        }
        return Some((32 + precision.unsigned_abs().saturating_add(1)).next_multiple_of(8));
    }
    if precision >= 0 && (num.scale as u64) < precision as u64 {
        return Some((32 + num.len as u64 + precision as u64).next_multiple_of(8));
    }
    None
}

pub fn round(num: &Num, precision: i64, mode: i64) -> (Num, usize) {
    if precision < 0 && (num.len as u64) < precision.unsigned_abs() {
        match mode {
            ROUND_HALF_UP | ROUND_HALF_DOWN | ROUND_HALF_EVEN | ROUND_HALF_ODD
            | ROUND_TOWARD_ZERO => return (Num::zero(), 0),
            ROUND_CEILING if num.neg => return (Num::zero(), 0),
            ROUND_FLOOR if !num.neg => return (Num::zero(), 0),
            _ => {}
        }
        if num.is_zero() {
            return (Num::zero(), 0);
        }
        let len = (precision.unsigned_abs() as usize).saturating_add(1);
        let mut r = Num::new(len, 0);
        r.v[0] = 1;
        r.neg = num.neg;
        return (r, 0);
    }
    if precision >= 0 && num.scale as u64 <= precision as u64 {
        let p = precision as usize;
        if num.scale == p {
            return (num.clone(), p);
        }
        let mut r = Num::new(num.len, p);
        r.neg = num.neg;
        r.v[..num.len + num.scale].copy_from_slice(&num.v);
        return (r, p);
    }
    let rounded_len = (num.len as i64 + precision) as usize;
    let mut result = if rounded_len == 0 {
        Num::new(1, 0)
    } else {
        let mut r = Num::new(num.len, if precision > 0 { precision as usize } else { 0 });
        r.v[..rounded_len].copy_from_slice(&num.v[..rounded_len]);
        r
    };
    result.neg = num.neg;
    let nd = num.v[rounded_len];
    let check_zero = |mut r: Num| -> (Num, usize) {
        let scale = r.scale;
        if r.is_zero() {
            r.neg = false;
            r.scale = 0;
            r.v.truncate(r.len);
        }
        (r, scale)
    };
    let up = |result: Num| -> Num {
        if rounded_len == 0 {
            let mut t = Num::new(num.len + 1, 0);
            t.v[0] = 1;
            t.neg = num.neg;
            t
        } else {
            let mut scaled_one = Num::new(result.len, result.scale);
            scaled_one.v[rounded_len - 1] = 1;
            let mut t = do_add(&result, &scaled_one);
            t.neg = result.neg;
            t
        }
    };
    // Cases decided by the first dropped digit.
    let decided = match mode {
        ROUND_HALF_UP => Some(nd >= 5),
        ROUND_HALF_DOWN | ROUND_HALF_EVEN | ROUND_HALF_ODD => {
            if nd > 5 {
                Some(true)
            } else if nd < 5 {
                Some(false)
            } else {
                None
            }
        }
        ROUND_CEILING => {
            if num.neg {
                Some(false)
            } else if nd > 0 {
                Some(true)
            } else {
                None
            }
        }
        ROUND_FLOOR => {
            if !num.neg {
                Some(false)
            } else if nd > 0 {
                Some(true)
            } else {
                None
            }
        }
        ROUND_TOWARD_ZERO => Some(false),
        _ => {
            if nd > 0 {
                Some(true)
            } else {
                None
            }
        }
    };
    let go_up = match decided {
        Some(u) => u,
        None => {
            let rest_nonzero = num.v[rounded_len + 1..].iter().any(|&d| d != 0);
            if rest_nonzero {
                true
            } else {
                match mode {
                    ROUND_HALF_EVEN => {
                        !(rounded_len == 0 || num.v[rounded_len - 1].is_multiple_of(2))
                    }
                    ROUND_HALF_ODD => !(rounded_len != 0 && num.v[rounded_len - 1] % 2 == 1),
                    _ => false,
                }
            }
        }
    };
    if go_up {
        result = up(result);
    }
    check_zero(result)
}

pub enum RaiseError {
    LenOverflow,
    ScaleOverflow,
    FullLenOverflow,
    DivideByZero,
}

/// The BC_VECTOR count of bc_standard_raise()'s work buffer (it allocates
/// three times that many 8-byte vectors up front), or None when bc_raise()
/// takes another path (zero exponent or base, overflow errors, the fast
/// path for small results).
pub fn raise_buffer_vectors(base: &Num, exponent: i64) -> Option<u64> {
    if exponent == 0 || base.is_zero() {
        return None;
    }
    let exp = exponent.unsigned_abs();
    let expz = usize::try_from(exp).ok()?;
    if base.len > usize::MAX / expz || base.scale > usize::MAX / expz {
        return None;
    }
    let power_len = base.len * expz;
    let power_scale = base.scale * expz;
    let full = power_len.checked_add(power_scale)?;
    let base_len = base.len + base.scale;
    if base_len <= 8 && full <= 16 {
        return None;
    }
    // (leading zeros of the base are not stored in its vectors)
    let lz = base.v.iter().take_while(|&&d| d == 0).count();
    let arr = (base_len - lz).div_ceil(8) as u64;
    arr.checked_mul(exp)
}

/// bc_raise()
pub fn raise(base: &Num, exponent: i64, scale: usize) -> Result<Num, RaiseError> {
    if exponent == 0 {
        return Ok(Num::one());
    }
    let is_neg = exponent < 0;
    let exp = exponent.unsigned_abs();
    let rscale = scale;
    if base.is_zero() {
        if is_neg {
            return Err(RaiseError::DivideByZero);
        }
        return Ok(Num::zero());
    }
    let expz = usize::try_from(exp).unwrap_or(usize::MAX);
    if base.len > usize::MAX / expz {
        return Err(RaiseError::LenOverflow);
    }
    if base.scale > usize::MAX / expz {
        return Err(RaiseError::ScaleOverflow);
    }
    let power_len = base.len * expz;
    let power_scale = base.scale * expz;
    if power_len > usize::MAX - power_scale {
        return Err(RaiseError::FullLenOverflow);
    }
    let neg = base.neg && exp & 1 == 1;
    let b = to_big(&base.v);
    // Nat::pow takes a u32 exponent; Int::pow takes any u64.
    let digits = big_digits(&Int::from(b).pow(exp).into_magnitude());
    let width = digits.len().max(power_scale + 1);
    let mut v = vec![0u8; width - digits.len()];
    v.extend_from_slice(&digits);
    let mut power = Num {
        neg: false,
        len: width - power_scale,
        scale: power_scale,
        v,
    };
    power.rm_leading_zeros();
    if power.is_zero() {
        power.scale = 0;
        power.v.truncate(power.len);
    } else {
        power.neg = neg;
    }
    if is_neg {
        divide(&Num::one(), &power, rscale).ok_or(RaiseError::DivideByZero)
    } else {
        power.trunc_scale(scale);
        Ok(power)
    }
}

pub enum RaiseModError {
    BaseHasFractional,
    ExpoHasFractional,
    ExpoIsNegative,
    ModHasFractional,
    ModIsZero,
}

/// bc_raisemod()
pub fn raisemod(base: &Num, expo: &Num, md: &Num, scale: usize) -> Result<Num, RaiseModError> {
    if base.scale != 0 {
        return Err(RaiseModError::BaseHasFractional);
    }
    if expo.scale != 0 {
        return Err(RaiseModError::ExpoHasFractional);
    }
    if expo.neg {
        return Err(RaiseModError::ExpoIsNegative);
    }
    if md.scale != 0 {
        return Err(RaiseModError::ModHasFractional);
    }
    if md.is_zero() {
        return Err(RaiseModError::ModIsZero);
    }
    if do_compare(md, &Num::one(), md.scale, false) == EQUAL {
        return Ok(Num::new(1, scale));
    }
    // All operands are integers: the C square-and-multiply loop keeps
    // scale 0 and C's truncated remainder (the sign follows the dividend).
    // Only the first multiply can see a negative factor (the base itself,
    // when the low exponent bit is set; every later factor is a square), so
    // the loop equals |base|^expo mod |md|, negated for a negative base and
    // an odd exponent.
    let mag = to_big(&base.v).modpow(&to_big(&expo.v), &to_big(&md.v));
    let odd = expo.v.last().is_some_and(|d| d % 2 == 1);
    let digits = big_digits(&mag);
    Ok(Num {
        neg: base.neg && odd && !mag.is_zero(),
        len: digits.len(),
        scale: 0,
        v: digits,
    })
}

/// bc_int2num()
fn int2num(val: i64) -> Num {
    let mut n = long2num(val);
    if n.v.is_empty() {
        n = Num::zero();
    }
    n
}

/// bc_sqrt(): None for a negative number.
pub fn sqrt(num: &Num, scale: usize) -> Option<Num> {
    if num.neg {
        return None;
    }
    if num.is_zero() {
        return Some(Num::zero());
    }
    let num_cmp_one = compare(num, &Num::one(), num.scale);
    if num_cmp_one == EQUAL {
        return Some(Num::one());
    }
    let rscale = scale.max(num.scale);
    let point5 = Num {
        neg: false,
        len: 1,
        scale: 1,
        v: vec![0, 5],
    };
    let mut guess;
    let mut cscale;
    if num_cmp_one == RIGHT_GREATER {
        guess = Num::one();
        cscale = num.scale;
    } else {
        let ten = int2num(10);
        let mut guess1 = int2num(num.len as i64);
        guess1 = multiply(&guess1, &point5, 0);
        guess1.trunc_scale(0);
        let e = num2long(&guess1);
        guess = raise(&ten, e, 0).ok()?;
        cscale = 3;
    }
    loop {
        let guess1 = guess.clone();
        guess = divide(num, &guess, cscale)?;
        guess = add(&guess, &guess1, 0);
        guess = multiply(&guess, &point5, cscale);
        let diff = sub(&guess, &guess1, cscale + 1);
        if diff.is_near_zero(cscale) {
            if cscale < rscale + 1 {
                cscale = (cscale * 3).min(rscale + 1);
            } else {
                break;
            }
        }
    }
    divide(&guess, &Num::one(), rscale)
}
