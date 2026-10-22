//! Big integers for ext/openssl.
//!
//! Every integer a key or certificate carries is held as a [`Bn`]: a minimal
//! big-endian unsigned byte string (what DER INTEGERs, `openssl_pkey_get_details()`
//! and the SEC1/PKCS#1 encoders all want anyway).
//!
//! Arithmetic is split by secrecy:
//!
//! - **secret** values (private exponents, primes and CRT values, DSA/DH
//!   private keys, EC scalars, nonces) only ever meet purecrypto's
//!   constant-time layer: [`Modulus`] wraps a `BoxedMontModulus` whose
//!   `pow` / `pow_bits` / `mul_mod` / `add_mod` / `sub_mod` / `reduce` run
//!   in time independent of the operand values, and inverses come from
//!   `inv_mod_odd_ct_boxed`;
//! - **public** values (parameter generation and validation, signature
//!   verification, point decoding, printing) use `puremp` naturals through
//!   [`Bn::nat`] / [`Bn::from_nat`].

use purecrypto::bignum::{BoxedMontModulus, BoxedUint, inv_mod_odd_ct_boxed};
use puremp::Nat;
use std::cmp::Ordering;

/// Minimal big-endian unsigned integer (zero is the empty string).
/// Wiped on drop, since it may hold key material.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Bn(Vec<u8>);

impl Drop for Bn {
    fn drop(&mut self) {
        purecrypto::zeroize::Zeroize::zeroize(self.0.as_mut_slice());
    }
}

impl std::fmt::Debug for Bn {
    // Never print the value: it may be a private key component.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Bn({} bytes)", self.0.len())
    }
}

/// Numeric order. Variable time: only for public values.
impl Ord for Bn {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .len()
            .cmp(&other.0.len())
            .then_with(|| self.0.cmp(&other.0))
    }
}

impl PartialOrd for Bn {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Bn {
    pub fn from_be(b: &[u8]) -> Bn {
        let i = b.iter().position(|&c| c != 0).unwrap_or(b.len());
        Bn(b[i..].to_vec())
    }

    pub fn from_u64(v: u64) -> Bn {
        Bn::from_be(&v.to_be_bytes())
    }

    /// Hex string (curve constants).
    pub fn from_hex(s: &str) -> Bn {
        let s = if s.len() % 2 == 1 {
            format!("0{s}")
        } else {
            s.to_string()
        };
        let b: Option<Vec<u8>> = (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
            .collect();
        Bn::from_be(&b.unwrap_or_default())
    }

    /// Minimal big-endian bytes (empty for zero).
    pub fn be(&self) -> &[u8] {
        &self.0
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.0.clone()
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_empty()
    }

    pub fn is_odd(&self) -> bool {
        self.0.last().is_some_and(|b| b & 1 == 1)
    }

    pub fn is_even(&self) -> bool {
        !self.is_odd()
    }

    pub fn bits(&self) -> usize {
        match self.0.first() {
            None => 0,
            Some(&b) => self.0.len() * 8 - b.leading_zeros() as usize,
        }
    }

    pub fn byte_len(&self) -> usize {
        self.0.len()
    }

    /// Left-pad to `len` bytes (a longer value is returned whole).
    pub fn pad(&self, len: usize) -> Vec<u8> {
        if self.0.len() >= len {
            return self.0.clone();
        }
        let mut out = vec![0u8; len - self.0.len()];
        out.extend_from_slice(&self.0);
        out
    }

    pub fn to_box(&self) -> BoxedUint {
        BoxedUint::from_be_bytes(&self.0)
    }

    pub fn from_box(b: &BoxedUint) -> Bn {
        let mut v = b.to_be_bytes(b.limbs() * 8);
        let r = Bn::from_be(&v);
        purecrypto::zeroize::Zeroize::zeroize(v.as_mut_slice());
        r
    }

    /// puremp natural. Public values only (puremp is not constant time).
    pub fn nat(&self) -> Nat {
        let mut le = self.0.clone();
        le.reverse();
        Nat::from_bytes_le(&le)
    }

    pub fn from_nat(n: &Nat) -> Bn {
        let mut b = n.to_bytes_le();
        b.reverse();
        Bn::from_be(&b)
    }

    /// Decimal string. Public values only.
    pub fn to_dec(&self) -> String {
        self.nat().to_string()
    }

    /// Lowercase hex without leading zeros ("0" for zero). Public values only.
    pub fn to_hex_lower(&self) -> String {
        format!("{:x}", self.nat())
    }
}

/// Public-value helpers on puremp naturals.
pub mod public {
    use puremp::Nat;

    pub fn rem(a: &Nat, m: &Nat) -> Nat {
        a.div_rem(m).map(|(_, r)| r).unwrap_or_else(Nat::zero)
    }

    pub fn div(a: &Nat, m: &Nat) -> Nat {
        a.div_rem(m).map(|(q, _)| q).unwrap_or_else(Nat::zero)
    }

    pub fn sub(a: &Nat, b: &Nat) -> Nat {
        a.checked_sub(b).unwrap_or_else(Nat::zero)
    }

    /// Modular inverse of a public value.
    pub fn inv(a: &Nat, m: &Nat) -> Option<Nat> {
        let r = puremp::Int::from(a.clone()).modinv(&puremp::Int::from(m.clone()))?;
        Some(r.magnitude().clone())
    }

    /// Probable-prime test on a public value (Baillie-PSW plus Miller-Rabin).
    pub fn is_probable_prime(n: &Nat, rounds: u32) -> bool {
        if !n.is_prime_bpsw() {
            return false;
        }
        n.is_probable_prime(rounds, &mut super::OsRandom)
    }
}

/// puremp random source on the OS CSPRNG.
pub struct OsRandom;

impl puremp::random::RandomSource for OsRandom {
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        use purecrypto::rng::RngCore;
        purecrypto::rng::OsRng.fill_bytes(dest);
    }
}

/// Uniform secret value in `[1, n)` from the OS CSPRNG (rejection sampling;
/// only the public accept/reject verdict is branched on). Zero when the
/// range is empty (n <= 1).
pub fn random_below(n: &Bn) -> BoxedUint {
    use purecrypto::rng::RngCore;
    let bits = n.bits();
    if bits < 2 {
        return BoxedUint::zero(1);
    }
    let len = bits.div_ceil(8).max(1);
    let mask = if bits.is_multiple_of(8) {
        0xff
    } else {
        (1u8 << (bits % 8)) - 1
    };
    let nb = n.to_box();
    let mut buf = vec![0u8; len];
    loop {
        purecrypto::rng::OsRng.fill_bytes(&mut buf);
        buf[0] &= mask;
        let k = BoxedUint::from_be_bytes(&buf);
        let ok = !bool::from(k.ct_is_zero()) && k.lt(&nb);
        if ok {
            purecrypto::zeroize::Zeroize::zeroize(buf.as_mut_slice());
            return k;
        }
    }
}

/// Uniform secret value of at most `bits` bits, nonzero and below `bound`.
pub fn random_bits_below(bits: usize, bound: &Bn) -> BoxedUint {
    if bits >= bound.bits() {
        return random_below(bound);
    }
    let mut top = vec![0u8; bits.div_ceil(8)];
    if let Some(f) = top.first_mut() {
        *f = if bits.is_multiple_of(8) {
            0xff
        } else {
            (1u8 << (bits % 8)) - 1
        };
    }
    for b in top.iter_mut().skip(1) {
        *b = 0xff;
    }
    // [1, 2^bits) == [1, (2^bits - 1) + 1)
    let mut lim = Nat::from_bytes_le(&top.iter().rev().copied().collect::<Vec<_>>());
    lim = lim.add(&Nat::one());
    random_below(&Bn::from_nat(&lim))
}

/// Montgomery context for an odd modulus `>= 3`. Constant-time arithmetic
/// on (possibly secret) operands; the modulus width is treated as public.
pub struct Modulus {
    m: BoxedMontModulus,
    mb: BoxedUint,
    len: usize,
}

impl Modulus {
    /// `None` for an even modulus or one below 3 (no Montgomery form).
    pub fn new(m: &Bn) -> Option<Modulus> {
        if m.is_even() || m.bits() < 2 {
            return None;
        }
        let mb = m.to_box();
        Some(Modulus {
            m: BoxedMontModulus::new(&mb),
            mb,
            len: m.byte_len(),
        })
    }

    pub fn modulus(&self) -> &BoxedUint {
        &self.mb
    }

    /// `x mod m` (constant time).
    pub fn reduce(&self, x: &BoxedUint) -> BoxedUint {
        self.m.reduce(x)
    }

    /// `a * b mod m` (operands of any size; constant time).
    pub fn mul(&self, a: &BoxedUint, b: &BoxedUint) -> BoxedUint {
        self.m.mul_mod(&self.m.reduce(a), &self.m.reduce(b))
    }

    /// `a + b mod m` (operands of any size; constant time).
    pub fn add(&self, a: &BoxedUint, b: &BoxedUint) -> BoxedUint {
        self.m.add_mod(&self.m.reduce(a), &self.m.reduce(b))
    }

    /// `a - b mod m` (operands of any size; constant time).
    pub fn sub(&self, a: &BoxedUint, b: &BoxedUint) -> BoxedUint {
        self.m.sub_mod(&self.m.reduce(a), &self.m.reduce(b))
    }

    /// `base^exp mod m` with a secret `exp` (and/or base): constant-time
    /// ladder over the exponent's storage width (at least the modulus').
    pub fn pow_secret(&self, base: &BoxedUint, exp: &BoxedUint) -> BoxedUint {
        self.m.pow(&self.m.reduce(base), exp)
    }

    /// `base^exp mod m` for a secret `exp < 2^bits`, `bits` public.
    pub fn pow_secret_bits(&self, base: &BoxedUint, exp: &BoxedUint, bits: usize) -> BoxedUint {
        self.m.pow_bits(&self.m.reduce(base), exp, bits)
    }

    /// `base^exp mod m` for a public exponent (base may be secret).
    pub fn pow_public(&self, base: &BoxedUint, exp: &BoxedUint) -> BoxedUint {
        self.m.pow_public(&self.m.reduce(base), exp)
    }

    /// `a^-1 mod m` (constant time); `None` when not invertible (a public
    /// verdict: callers only use it on values that must be invertible).
    pub fn inv(&self, a: &BoxedUint) -> Option<BoxedUint> {
        inv_mod_odd_ct_boxed(&self.m.reduce(a), &self.mb).into_option()
    }

    /// Serialize a reduced value at the modulus width.
    pub fn to_bytes(&self, x: &BoxedUint) -> Vec<u8> {
        self.reduce(x).to_be_bytes(self.len)
    }
}

/// `base^exp mod m` for public operands, any modulus (odd moduli go through
/// Montgomery arithmetic, even ones through puremp).
pub fn modpow_public(base: &Bn, exp: &Bn, m: &Bn) -> Bn {
    if m.is_zero() {
        return Bn::default();
    }
    match Modulus::new(m) {
        Some(mm) => Bn::from_box(&mm.pow_public(&base.to_box(), &exp.to_box())),
        None => Bn::from_nat(&base.nat().modpow(&exp.nat(), &m.nat())),
    }
}
