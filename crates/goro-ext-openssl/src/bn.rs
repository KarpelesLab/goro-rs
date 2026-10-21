//! Modular exponentiation through purecrypto's Montgomery arithmetic
//! (constant-time for secret exponents), with `num-bigint-dig` values at the
//! API boundary.

use num_bigint_dig::BigUint;
use num_integer::Integer;
use purecrypto::bignum::{BoxedMontModulus, BoxedUint};

fn to_box(n: &BigUint, len: usize) -> BoxedUint {
    let b = n.to_bytes_be();
    let mut v = vec![0u8; len.saturating_sub(b.len())];
    v.extend(b);
    BoxedUint::from_be_bytes(&v)
}

fn mont(m: &BigUint) -> Option<(BoxedMontModulus, usize)> {
    if m.is_even() || m.bits() < 2 {
        return None;
    }
    let len = m.bits().div_ceil(8);
    Some((
        BoxedMontModulus::new(&BoxedUint::from_be_bytes(&m.to_bytes_be())),
        len,
    ))
}

fn from_box(b: &BoxedUint, len: usize) -> BigUint {
    BigUint::from_bytes_be(&b.to_be_bytes(len))
}

/// `base^exp mod m` for a secret exponent (constant time when `m` is odd).
pub fn modpow(base: &BigUint, exp: &BigUint, m: &BigUint) -> BigUint {
    match mont(m) {
        Some((mm, len)) => {
            let b = to_box(&(base % m), len);
            let e = BoxedUint::from_be_bytes(&exp.to_bytes_be());
            from_box(&mm.pow(&b, &e), len)
        }
        None => base.modpow(exp, m),
    }
}

/// `base^exp mod m` for public values (variable time).
pub fn modpow_public(base: &BigUint, exp: &BigUint, m: &BigUint) -> BigUint {
    match mont(m) {
        Some((mm, len)) => {
            let b = to_box(&(base % m), len);
            let e = BoxedUint::from_be_bytes(&exp.to_bytes_be());
            from_box(&mm.pow_public(&b, &e), len)
        }
        None => base.modpow(exp, m),
    }
}
