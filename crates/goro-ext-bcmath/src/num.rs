//! Port of php-src's libbcmath (ext/bcmath/libbcmath/src): arbitrary
//! precision decimal numbers as BCD digit strings.
//!
//! The digit layout, the scales of intermediate and final results and the
//! truncation rules follow the C library exactly (they decide how many
//! digits results keep, and the Newton iteration of bc_sqrt() depends on
//! them). The vectorized multiplication / division kernels are replaced by
//! num-bigint, which computes the same exact integer products and
//! truncated quotients.

use num_bigint::BigUint;
use num_traits::Zero;

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

fn to_big(digits: &[u8]) -> BigUint {
    if digits.is_empty() {
        return BigUint::zero();
    }
    BigUint::from_radix_be(digits, 10).unwrap_or_default()
}

/// The decimal digits of `b`, right-aligned in `width` digits.
fn from_big(b: &BigUint, width: usize) -> Vec<u8> {
    let mut out = vec![0u8; width];
    if b.is_zero() {
        return out;
    }
    let d = b.to_radix_be(10);
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
    let p = to_big(&n1.v) * to_big(&n2.v);
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
    let q = to_big(&ndig) / to_big(&dd[..divisor_size]);
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
                    ROUND_HALF_EVEN => !(rounded_len == 0 || num.v[rounded_len - 1].is_multiple_of(2)),
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
    let p = num_traits::pow::Pow::pow(&b, exp);
    let digits = if p.is_zero() {
        vec![0]
    } else {
        p.to_radix_be(10)
    };
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
    // All operands are integers: the multiply / modulo steps of the C
    // loop keep scale 0 and C's truncated remainder (the sign follows the
    // dividend), which BigInt's `%` also gives.
    use num_bigint::{BigInt, Sign};
    let to_int = |n: &Num| -> BigInt {
        BigInt::from_biguint(if n.neg { Sign::Minus } else { Sign::Plus }, to_big(&n.v))
    };
    let mut power = to_int(base);
    let mut exponent = to_big(&expo.v);
    let modulus = to_int(md);
    let mut temp = BigInt::from(1);
    let two = BigUint::from(2u32);
    while !exponent.is_zero() {
        let parity = &exponent % &two;
        exponent /= &two;
        if !parity.is_zero() {
            temp = (&temp * &power) % &modulus;
        }
        power = (&power * &power) % &modulus;
    }
    let (sign, mag) = temp.into_parts();
    let digits = if mag.is_zero() {
        vec![0]
    } else {
        mag.to_radix_be(10)
    };
    Ok(Num {
        neg: sign == Sign::Minus,
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
