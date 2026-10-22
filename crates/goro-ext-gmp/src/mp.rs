//! GMP (mpz_*) semantics on top of `puremp::Int`.
//!
//! puremp provides the arithmetic; this module adds the handful of mpz
//! operations it does not expose directly (two's-complement bit access,
//! floor shifts, bit scans, radixes above 36, mpz_get_si / mpz_get_d
//! conversions, Kronecker symbol, binomials of arbitrary n, ...) with GMP's
//! exact conventions.

use puremp::{Int, Nat, Sign};
use std::fmt::Write;

fn neg_if(neg: bool, mag: Nat) -> Int {
    Int::from_sign_magnitude(if neg { Sign::Negative } else { Sign::Positive }, mag)
}

/// `n << k` (mpz_mul_2exp).
pub fn shl(n: &Int, k: u64) -> Int {
    neg_if(n.is_negative(), n.magnitude().shl(k))
}

/// `n >> k` rounding toward -inf (mpz_fdiv_q_2exp, PHP's `>>`).
pub fn shr_floor(n: &Int, k: u64) -> Int {
    if !n.is_negative() {
        return Int::from(n.magnitude().shr(k));
    }
    // -(((|n| - 1) >> k) + 1)
    let m = n.magnitude().checked_sub(&Nat::one()).expect("n < 0");
    neg_if(true, m.shr(k).add(&Nat::one()))
}

/// 2^k.
pub fn pow2(k: u64) -> Int {
    Int::from(Nat::one().shl(k))
}

/// Bit `i` of the infinite two's-complement representation (mpz_tstbit).
pub fn tstbit(n: &Int, i: u64) -> bool {
    if n.is_negative() {
        // ~(|n| - 1)
        let m = n.magnitude().checked_sub(&Nat::one()).expect("n < 0");
        !m.bit(i)
    } else {
        n.magnitude().bit(i)
    }
}

/// mpz_setbit / mpz_clrbit.
pub fn set_bit(n: &Int, i: u64, value: bool) -> Int {
    if tstbit(n, i) == value {
        return n.clone();
    }
    // Flipping a single bit of the two's-complement form never carries.
    if value { n + pow2(i) } else { n - pow2(i) }
}

/// Population count of the magnitude limbs.
fn nat_popcount(limbs: &[u64]) -> u64 {
    limbs.iter().map(|l| l.count_ones() as u64).sum()
}

/// mpz_popcount: -1 (ULONG_MAX) for negative numbers.
pub fn popcount(n: &Int) -> i64 {
    if n.is_negative() {
        -1
    } else {
        nat_popcount(n.limbs()) as i64
    }
}

/// mpz_hamdist: -1 (ULONG_MAX) when the signs differ.
pub fn hamdist(a: &Int, b: &Int) -> i64 {
    popcount(&a.bitxor(b))
}

/// First set (`want == true`) or clear bit at or after `start` in a
/// non-negative magnitude; None when no set bit remains.
fn nat_scan(limbs: &[u64], start: u64, want: bool) -> Option<u64> {
    let mut idx = (start / 64) as usize;
    if idx >= limbs.len() {
        return if want { None } else { Some(start) };
    }
    let first_mask = !0u64 << (start % 64);
    let mut word = if want { limbs[idx] } else { !limbs[idx] } & first_mask;
    loop {
        if word != 0 {
            return Some(idx as u64 * 64 + word.trailing_zeros() as u64);
        }
        idx += 1;
        if idx >= limbs.len() {
            return if want { None } else { Some(idx as u64 * 64) };
        }
        word = if want { limbs[idx] } else { !limbs[idx] };
    }
}

/// mpz_scan0 / mpz_scan1 on the two's-complement form; -1 when there is
/// no such bit.
pub fn scan(n: &Int, start: u64, want_one: bool) -> i64 {
    let r = if n.is_negative() {
        // The two's-complement bits of n are the inverted bits of |n| - 1.
        let m = n.magnitude().checked_sub(&Nat::one()).expect("n < 0");
        nat_scan(m.as_limbs(), start, !want_one)
    } else {
        nat_scan(n.limbs(), start, want_one)
    };
    r.map(|v| v as i64).unwrap_or(-1)
}

/// mpz_get_si(): the low bits with the sign of `n`.
pub fn get_si(n: &Int) -> i64 {
    let zl = n.least_significant_limb();
    match n.sign() {
        Sign::Zero => 0,
        Sign::Positive => (zl & i64::MAX as u64) as i64,
        Sign::Negative => -1 - (zl.wrapping_sub(1) & i64::MAX as u64) as i64,
    }
}

/// mpz_get_ui(): the low limb of |n|.
pub fn get_ui(n: &Int) -> u64 {
    n.least_significant_limb()
}

/// mpz_get_d(): truncation toward zero (not round-to-nearest).
pub fn get_d(n: &Int) -> f64 {
    let bits = n.bit_len() as u64;
    if bits <= 53 {
        let v = n.least_significant_limb() as f64;
        return if n.is_negative() { -v } else { v };
    }
    if bits > 1024 {
        return if n.is_negative() {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    // Keep the top 53 bits (truncating) and scale back up exactly.
    let shift = bits - 53;
    let top = n.magnitude().shr(shift).to_u64().expect("53 bits");
    let v = top as f64 * 2f64.powi(shift as i32);
    if n.is_negative() { -v } else { v }
}

pub fn digit_value(ch: u8, base: u32) -> Option<u32> {
    let d = match ch {
        b'0'..=b'9' => (ch - b'0') as u32,
        // Bases up to 36 are case-insensitive; above, A-Z are 10..35 and
        // a-z are 36..61 (mpz_set_str).
        b'A'..=b'Z' => (ch - b'A') as u32 + 10,
        b'a'..=b'z' if base <= 36 => (ch - b'a') as u32 + 10,
        b'a'..=b'z' => (ch - b'a') as u32 + 36,
        _ => return None,
    };
    (d < base).then_some(d)
}

/// Parse validated digits (no sign/prefix/whitespace) in base 2..=62.
pub fn parse_digits(digits: &[u8], base: u32) -> Option<Int> {
    if digits.is_empty() {
        return None;
    }
    if base <= 36 {
        // Validate with GMP's alphabet first (puremp accepts the same set).
        if !digits.iter().all(|&c| digit_value(c, base).is_some()) {
            return None;
        }
        let s = std::str::from_utf8(digits).ok()?;
        return Int::from_str_radix(s, base).ok();
    }
    // puremp only parses radixes up to 36: Horner over u64-sized chunks.
    let (mut chunk, mut chunk_pow) = (0usize, 1u64);
    while let Some(p) = chunk_pow.checked_mul(base as u64) {
        chunk_pow = p;
        chunk += 1;
    }
    let mut acc = Int::ZERO;
    for part in digits.chunks(chunk) {
        let mut v = 0u64;
        for &c in part {
            v = v * base as u64 + digit_value(c, base)? as u64;
        }
        let scale = if part.len() == chunk {
            chunk_pow
        } else {
            (base as u64).pow(part.len() as u32)
        };
        acc = acc * Int::from(scale) + Int::from(v);
    }
    Some(acc)
}

/// mpz_get_str() for base 2..=62 (lowercase for bases up to 36).
pub fn to_str_radix(n: &Int, base: u32) -> String {
    if base <= 36 {
        let mut s = String::new();
        if base == 10 {
            write!(s, "{}", n).ok();
        } else {
            n.write_radix(&mut s, base).ok();
        }
        return s;
    }
    if n.is_zero() {
        return "0".to_string();
    }
    const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let (mut chunk, mut chunk_pow) = (0usize, 1u64);
    while let Some(p) = chunk_pow.checked_mul(base as u64) {
        chunk_pow = p;
        chunk += 1;
    }
    let divisor = Int::from(chunk_pow);
    let mut rest = n.abs();
    let mut out: Vec<u8> = Vec::new();
    while !rest.is_zero() {
        let (q, r) = rest.div_rem_trunc(&divisor);
        let mut v = r.to_u64().expect("remainder < chunk_pow");
        let last = q.is_zero();
        let mut produced = 0;
        while v > 0 || (!last && produced < chunk) {
            out.push(ALPHABET[(v % base as u64) as usize]);
            v /= base as u64;
            produced += 1;
        }
        rest = q;
    }
    if n.is_negative() {
        out.push(b'-');
    }
    out.reverse();
    String::from_utf8(out).expect("ascii")
}

/// Integer square root of a non-negative value.
pub fn isqrt(n: &Int) -> Int {
    Int::from(n.magnitude().isqrt())
}

/// mpz_root(): truncated k-th root (k >= 1; odd k for negative n).
pub fn root(n: &Int, k: u64) -> Int {
    let m = n.magnitude();
    let r = if m.is_zero() || m.is_one() || k == 1 {
        m
    } else if k >= m.bit_len() {
        // 2^k > |n| >= 1: the root is 1.
        Nat::one()
    } else {
        m.nth_root_floor(k as u32)
    };
    neg_if(n.is_negative(), r)
}

/// mpz_perfect_power_p(): 0, 1 and -1 are perfect powers; negative values
/// only count with odd exponents.
pub fn is_perfect_power(n: &Int) -> bool {
    let m = n.magnitude();
    if m.bit_len() <= 1 {
        return true;
    }
    let bits = m.bit_len();
    // It suffices to try prime exponents.
    let mut p: u64 = 2;
    while p <= bits {
        if is_small_prime(p) && n.nth_root_exact(p as u32).is_some() {
            return true;
        }
        p += 1;
    }
    false
}

fn is_small_prime(p: u64) -> bool {
    if p < 2 {
        return false;
    }
    let mut d = 2;
    while d * d <= p {
        if p.is_multiple_of(d) {
            return false;
        }
        d += 1;
    }
    true
}

/// mpz_gcdext(): Euclid's cofactors, except that gcd(0, 0) has s = t = 0.
pub fn gcdext(a: &Int, b: &Int) -> (Int, Int, Int) {
    if a.is_zero() && b.is_zero() {
        return (Int::ZERO, Int::ZERO, Int::ZERO);
    }
    a.extended_gcd(b)
}

/// mpz_perfect_square_p().
pub fn is_perfect_square(n: &Int) -> bool {
    n.sqrt_exact().is_some()
}

/// mpz_kronecker() (GMP's mpz_jacobi and mpz_legendre are the same
/// function, so all three PHP functions use this).
pub fn kronecker(a: &Int, b: &Int) -> i32 {
    if b.is_zero() {
        return (a.magnitude().is_one()) as i32;
    }
    if a.is_even() && b.is_even() {
        return 0;
    }
    let v = b.trailing_zeros();
    let odd = Int::from(b.magnitude().shr(v as u64));
    let mut result = 1;
    if v % 2 == 1 {
        // (a/2): 1 for a = ±1 (mod 8), -1 for a = ±3 (mod 8).
        let r = a.rem_euclid(&Int::from(8u64)).to_u64().unwrap_or(0);
        if r == 3 || r == 5 {
            result = -result;
        }
    }
    if b.is_negative() && a.is_negative() {
        result = -result;
    }
    if odd.is_one() {
        return result;
    }
    result * a.jacobi(&odd)
}

/// mpz_bin_ui(): C(n, k) for any integer n.
pub fn binomial(n: &Int, k: u64) -> Int {
    if n.is_negative() {
        // C(n, k) = (-1)^k C(k - n - 1, k)
        let m = Int::from(k) - n - Int::ONE;
        let r = binomial(&m, k);
        return if k % 2 == 1 { -r } else { r };
    }
    if let Some(nn) = n.to_u64() {
        return Int::binomial(nn, k);
    }
    // n > 2^64 > k: the falling product n(n-1)...(n-k+1) / k!.
    let mut num = Int::ONE;
    let mut i = 0u64;
    while i < k {
        num *= n - Int::from(i);
        i += 1;
    }
    num.div_exact(&Int::factorial(k))
}

/// mpz_probab_prime_p(): 2 = definitely prime, 1 = probably prime,
/// 0 = composite. Negative numbers are tested by absolute value.
pub fn probab_prime(n: &Int) -> i64 {
    let m = Int::from(n.magnitude());
    if let Some(v) = m.to_u64() {
        if v <= 1_000_000 {
            return if m.is_prime_bpsw() { 2 } else { 0 };
        }
    }
    if m.is_even() || !m.is_prime_bpsw() {
        return 0;
    }
    // mpz_millerrabin() trusts Baillie-PSW below 35 * 2^46; above it says
    // "probably prime" (its extra random-base Miller-Rabin rounds only run
    // on numbers BPSW already accepted).
    if m < Int::from(35u64 << 46) { 2 } else { 1 }
}

/// mpz_pow_ui().
pub fn pow(base: &Int, exp: u64) -> Int {
    if exp <= u32::MAX as u64 {
        return base.pow(exp as u32);
    }
    let m = base.magnitude();
    if m.is_zero() || m.is_one() {
        return if base.is_negative() && exp % 2 == 1 {
            Int::MINUS_ONE
        } else if m.is_zero() {
            Int::ZERO
        } else {
            Int::ONE
        };
    }
    // Square-and-multiply past u32 exponents (memory-bound like GMP).
    let mut result = Int::ONE;
    let mut b = base.clone();
    let mut e = exp;
    while e > 0 {
        if e & 1 == 1 {
            result *= &b;
        }
        e >>= 1;
        if e > 0 {
            b = b.square();
        }
    }
    result
}

/// Magnitude as big-endian bytes (empty for zero).
pub fn to_bytes_be(n: &Int) -> Vec<u8> {
    let mut v = n.magnitude().to_bytes_le();
    v.reverse();
    v
}

/// Non-negative value from big-endian bytes.
pub fn from_bytes_be(bytes: &[u8]) -> Int {
    let mut le = bytes.to_vec();
    le.reverse();
    Int::from(Nat::from_bytes_le(&le))
}
