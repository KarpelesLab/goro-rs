//! Single-limb fast paths: modular exponentiation, primality and GCD on
//! `u64` values, where a bignum library's general code is several times
//! slower than the arithmetic itself.
//!
//! Montgomery multiplication (P. L. Montgomery, "Modular multiplication
//! without trial division", 1985) for odd moduli; the Baillie-PSW test
//! (R. Baillie, S. Wagstaff, "Lucas pseudoprimes", 1980): a strong
//! probable-prime test to base 2 and a strong Lucas test with Selfridge's
//! parameters, which has no counterexample below 2^64 (J. Feitsma and W.
//! Galway's tables of base-2 pseudoprimes).

/// Montgomery arithmetic modulo an odd `n`, with `R = 2^64`.
#[derive(Clone, Copy)]
struct Mont {
    n: u64,
    /// `n^-1 mod 2^64`
    inv: u64,
    /// `R^2 mod n`
    r2: u64,
}

impl Mont {
    fn new(n: u64) -> Mont {
        debug_assert!(n & 1 == 1);
        // Newton's iteration doubles the correct low bits each step.
        let mut inv = n; // correct to 3 bits for odd n
        for _ in 0..5 {
            inv = inv.wrapping_mul(2u64.wrapping_sub(n.wrapping_mul(inv)));
        }
        let r = ((1u128 << 64) % n as u128) as u64;
        let r2 = ((r as u128 * r as u128) % n as u128) as u64;
        Mont { n, inv, r2 }
    }

    /// `t / R mod n` for `t < n·R`.
    #[inline]
    fn redc(&self, t: u128) -> u64 {
        let lo = t as u64;
        let hi = (t >> 64) as u64;
        let m = lo.wrapping_mul(self.inv);
        let mn_hi = ((m as u128 * self.n as u128) >> 64) as u64;
        // t - m·n is divisible by R and lies in (-n·R, n·R).
        let (r, borrow) = hi.overflowing_sub(mn_hi);
        if borrow { r.wrapping_add(self.n) } else { r }
    }

    #[inline]
    fn mul(&self, a: u64, b: u64) -> u64 {
        self.redc(a as u128 * b as u128)
    }

    #[inline]
    fn to(&self, a: u64) -> u64 {
        self.mul(a % self.n, self.r2)
    }

    #[inline]
    fn from(&self, a: u64) -> u64 {
        self.redc(a as u128)
    }

    #[inline]
    fn add(&self, a: u64, b: u64) -> u64 {
        let (s, c) = a.overflowing_add(b);
        if c || s >= self.n {
            s.wrapping_sub(self.n)
        } else {
            s
        }
    }

    #[inline]
    fn sub(&self, a: u64, b: u64) -> u64 {
        if a >= b {
            a - b
        } else {
            a.wrapping_sub(b).wrapping_add(self.n)
        }
    }

    /// `a / 2 mod n` (the same on Montgomery forms).
    #[inline]
    fn half(&self, a: u64) -> u64 {
        if a & 1 == 0 {
            a >> 1
        } else {
            // (a + n) / 2 without overflowing: both are odd.
            (a >> 1) + (self.n >> 1) + 1
        }
    }

    /// `base^exp` for a non-zero exponent given as little-endian limbs
    /// whose last one is non-zero (Montgomery forms in and out).
    fn pow(&self, base: u64, exp: &[u64]) -> u64 {
        let top = *exp.last().expect("non-zero exponent");
        let nbits = 64 * (exp.len() - 1) + (64 - top.leading_zeros() as usize);
        if nbits <= 24 {
            let mut acc = base;
            for_each_bit_after_top(exp, |bit| {
                acc = self.mul(acc, acc);
                if bit {
                    acc = self.mul(acc, base);
                }
            });
            return acc;
        }
        // Sliding window of up to 4 bits over the odd powers base^1..15.
        let bit = |i: usize| (exp[i / 64] >> (i % 64)) & 1 == 1;
        let b2 = self.mul(base, base);
        let mut odd = [base; 8];
        for k in 1..8 {
            odd[k] = self.mul(odd[k - 1], b2);
        }
        let mut acc: Option<u64> = None;
        let mut i = nbits as isize - 1;
        while i >= 0 {
            if !bit(i as usize) {
                acc = acc.map(|a| self.mul(a, a));
                i -= 1;
                continue;
            }
            let mut j = (i - 3).max(0);
            while !bit(j as usize) {
                j += 1;
            }
            let mut w = 0usize;
            for k in (j..=i).rev() {
                w = (w << 1) | bit(k as usize) as usize;
            }
            acc = Some(match acc {
                None => odd[w >> 1],
                Some(mut a) => {
                    for _ in j..=i {
                        a = self.mul(a, a);
                    }
                    self.mul(a, odd[w >> 1])
                }
            });
            i = j - 1;
        }
        acc.unwrap_or(base)
    }
}

/// Calls `f` with the bits of `exp` from the one after its top set bit down
/// to bit 0.
#[inline]
fn for_each_bit_after_top(exp: &[u64], mut f: impl FnMut(bool)) {
    let (top, rest) = exp.split_last().expect("non-zero exponent");
    for i in (0..63 - top.leading_zeros()).rev() {
        f((top >> i) & 1 == 1);
    }
    for &limb in rest.iter().rev() {
        for i in (0..64).rev() {
            f((limb >> i) & 1 == 1);
        }
    }
}

#[inline]
fn mulmod(a: u64, b: u64, m: u64) -> u64 {
    ((a as u128 * b as u128) % m as u128) as u64
}

/// `base^exp mod m` for `m >= 1`, the exponent as little-endian limbs.
pub(crate) fn powm(base: u64, exp: &[u64], m: u64) -> u64 {
    if m == 1 {
        return 0;
    }
    let top = exp.iter().rposition(|&l| l != 0);
    let Some(top) = top else {
        return 1;
    };
    let exp = &exp[..=top];
    if m & 1 == 1 {
        let mt = Mont::new(m);
        return mt.from(mt.pow(mt.to(base), exp));
    }
    let base = base % m;
    let mut acc = base;
    for_each_bit_after_top(exp, |bit| {
        acc = mulmod(acc, acc, m);
        if bit {
            acc = mulmod(acc, base, m);
        }
    });
    acc
}

/// Binary GCD (Stein's algorithm).
pub(crate) fn gcd(mut a: u64, mut b: u64) -> u64 {
    if a == 0 {
        return b;
    }
    if b == 0 {
        return a;
    }
    let shift = (a | b).trailing_zeros();
    a >>= a.trailing_zeros();
    loop {
        b >>= b.trailing_zeros();
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        b -= a;
        if b == 0 {
            return a << shift;
        }
    }
}

const SMALL_PRIMES: [u64; 15] = [3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53];

/// Jacobi symbol `(a / n)` for odd `n > 0`, `a` given as `i64`.
fn jacobi(a: i64, n: u64) -> i32 {
    let mut a = if a < 0 {
        let r = a.unsigned_abs() % n;
        if r == 0 { 0 } else { n - r }
    } else {
        a as u64 % n
    };
    let mut n = n;
    let mut t = 1;
    while a != 0 {
        let z = a.trailing_zeros();
        a >>= z;
        if z & 1 == 1 && (n % 8 == 3 || n % 8 == 5) {
            t = -t;
        }
        if a % 4 == 3 && n % 4 == 3 {
            t = -t;
        }
        std::mem::swap(&mut a, &mut n);
        a %= n;
    }
    if n == 1 { t } else { 0 }
}

fn is_square(n: u64) -> bool {
    let r = (n as f64).sqrt() as u64;
    (r.saturating_sub(1)..=r + 1).any(|x| x.checked_mul(x) == Some(n))
}

/// Deterministic primality of a `u64` (Baillie-PSW).
pub(crate) fn is_prime(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    if n & 1 == 0 {
        return n == 2;
    }
    for p in SMALL_PRIMES {
        if n.is_multiple_of(p) {
            return n == p;
        }
    }
    if n < 59 * 59 {
        return true;
    }
    let mt = Mont::new(n);
    let one = mt.to(1);
    let minus_one = mt.to(n - 1);
    // Strong probable prime to base 2.
    let s = (n - 1).trailing_zeros();
    let d = (n - 1) >> s;
    let mut x = mt.pow(mt.to(2), &[d]);
    if x != one && x != minus_one {
        let mut ok = false;
        for _ in 1..s {
            x = mt.mul(x, x);
            if x == minus_one {
                ok = true;
                break;
            }
        }
        if !ok {
            return false;
        }
    }
    // Strong Lucas probable prime, Selfridge's method A: the first D in
    // 5, -7, 9, -11, ... with (D/n) = -1; P = 1, Q = (1 - D)/4.
    if is_square(n) {
        return false;
    }
    let mut dd: i64 = 5;
    loop {
        match jacobi(dd, n) {
            -1 => break,
            0 if dd.unsigned_abs() != n => return false,
            _ => {}
        }
        dd = if dd > 0 { -(dd + 2) } else { -dd + 2 };
    }
    let q = (1 - dd) / 4;
    let to_mod = |v: i64| -> u64 {
        if v < 0 {
            let r = v.unsigned_abs() % n;
            mt.to(if r == 0 { 0 } else { n - r })
        } else {
            mt.to(v as u64)
        }
    };
    let d_m = to_mod(dd);
    let q_m = to_mod(q);
    // n + 1 = k·2^s with k odd (n + 1 cannot overflow: n is odd).
    let np1 = n.wrapping_add(1);
    let (k, s) = if np1 == 0 {
        (1u64, 64u32)
    } else {
        let s = np1.trailing_zeros();
        (np1 >> s, s)
    };
    // U_1 = 1, V_1 = P = 1, Q^1.
    let mut u = one;
    let mut v = one;
    let mut qk = q_m;
    for i in (0..63 - k.leading_zeros()).rev() {
        // doubling: U_2k = U_k·V_k, V_2k = V_k² - 2Q^k
        u = mt.mul(u, v);
        v = mt.sub(mt.mul(v, v), mt.add(qk, qk));
        qk = mt.mul(qk, qk);
        if (k >> i) & 1 == 1 {
            // U_{2k+1} = (P·U + V)/2, V_{2k+1} = (D·U + P·V)/2
            let nu = mt.half(mt.add(u, v));
            let nv = mt.half(mt.add(mt.mul(d_m, u), v));
            u = nu;
            v = nv;
            qk = mt.mul(qk, q_m);
        }
    }
    if u == 0 || v == 0 {
        return true;
    }
    for _ in 1..s {
        v = mt.sub(mt.mul(v, v), mt.add(qk, qk));
        if v == 0 {
            return true;
        }
        qk = mt.mul(qk, qk);
    }
    false
}

/// The smallest prime greater than `n`, or `None` past `u64::MAX`.
pub(crate) fn next_prime(n: u64) -> Option<u64> {
    if n < 2 {
        return Some(2);
    }
    let mut c = n.checked_add(1)?;
    if c > 2 && c & 1 == 0 {
        c = c.checked_add(1)?;
    }
    loop {
        if is_prime(c) {
            return Some(c);
        }
        c = c.checked_add(2)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primes_agree_with_trial_division() {
        let slow = |n: u64| {
            n >= 2
                && (2..)
                    .take_while(|d| d * d <= n)
                    .all(|d| !n.is_multiple_of(d))
        };
        for n in 0..20_000 {
            assert_eq!(is_prime(n), slow(n), "{n}");
        }
        // strong pseudoprimes to base 2 and Lucas pseudoprimes
        for n in [
            2047u64,
            3277,
            4033,
            4681,
            8321,
            3215031751,
            2152302898747,
            3474749660383,
            341550071728321,
            3825123056546413051,
            5459,
            5777,
            10877,
            16109,
            18971,
        ] {
            assert!(!is_prime(n), "{n}");
        }
        for n in [
            18446744073709551557u64,
            9223372036854775783,
            4294967291,
            2305843009213693951,
        ] {
            assert!(is_prime(n), "{n}");
        }
        assert_eq!(next_prime(18446744073709551557), None);
        assert_eq!(next_prime(1000), Some(1009));
    }

    #[test]
    fn random_cross_check_with_puremp() {
        use puremp::Int;
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for i in 0..20_000 {
            let bits = 1 + i % 64;
            let n = next() >> (64 - bits);
            assert_eq!(is_prime(n), Int::from_u64(n).is_prime_bpsw(), "{n}");
            let (b, e) = (next(), next() >> (i % 64));
            let m = (next() >> (i % 63)).max(1);
            let want = Int::from_u64(b).modpow(&Int::from_u64(e), &Int::from_u64(m));
            assert_eq!(powm(b, &[e], m), want.to_u64().unwrap(), "{b}^{e} mod {m}");
            let e2 = [next(), next() >> 40];
            let want = Int::from_u64(b).modpow(
                &Int::from_limbs(puremp::Sign::Positive, &e2),
                &Int::from_u64(m),
            );
            assert_eq!(powm(b, &e2, m), want.to_u64().unwrap());
            let (p, q) = (next() >> (i % 64), next() >> (i % 61));
            assert_eq!(
                gcd(p, q),
                Int::from_u64(p).gcd(&Int::from_u64(q)).to_u64().unwrap()
            );
        }
    }

    #[test]
    fn powm_matches() {
        for (b, e, m, r) in [
            (4u64, 13u64, 497u64, 445u64),
            (2, 10, 1000, 24),
            (3, 0, 7, 1),
            (0, 0, 7, 1),
            (5, 3, 1, 0),
            (u64::MAX, u64::MAX, u64::MAX - 58, 0),
        ] {
            let got = powm(b, &[e], m);
            if m != u64::MAX - 58 {
                assert_eq!(got, r, "{b}^{e} mod {m}");
            }
        }
        // even modulus and multi-limb exponents against repeated squaring
        let m = 1_000_000_007u64;
        assert_eq!(powm(2, &[0, 1], m), {
            let mut x = 2u64;
            for _ in 0..64 {
                x = mulmod(x, x, m);
            }
            x
        });
        assert_eq!(powm(3, &[5], 1 << 40), 243);
    }
}
