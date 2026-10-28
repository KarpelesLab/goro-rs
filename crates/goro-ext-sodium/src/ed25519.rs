//! Ed25519 (RFC 8032 §5.1) in libsodium's key format: a 64-byte secret key
//! `seed ‖ public key` and 64-byte signatures `R ‖ S`.
//!
//! Signing uses the public key stored in the secret key, as libsodium
//! does (libsodium documentation: the secret key "contains the seed and the
//! public key"; observed on PHP 8.5: a secret key whose second half is not
//! the seed's public key signs with that second half). Verification is
//! cofactorless — `encode([S]B − [k]A)` must equal the `R` bytes — with `S`
//! below the group order and `A` a canonical point not of small order
//! (behaviour observed on PHP 8.5, libsodium 1.0.18).
//!
//! Group operations come from purecrypto's edwards25519 hazmat layer
//! (constant-time); verification, whose inputs are all public, uses a
//! variable-time double-scalar multiplication on [`crate::field`].

use crate::edwards;
use purecrypto::ec::edwards25519::hazmat::{EdwardsPoint, Scalar};
use purecrypto::hash::{Digest, Sha512};

/// Expands a seed into the clamped secret scalar and the nonce prefix.
fn expand(seed: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let h = Sha512::digest(seed);
    let mut a = [0u8; 32];
    a.copy_from_slice(&h[..32]);
    a[0] &= 248;
    a[31] &= 127;
    a[31] |= 64;
    let mut prefix = [0u8; 32];
    prefix.copy_from_slice(&h[32..64]);
    let mut h = h;
    crate::ct::wipe(&mut h[..]);
    (a, prefix)
}

/// A scalar from 32 little-endian bytes, reduced modulo the group order.
pub(crate) fn scalar_from_32(b: &[u8; 32]) -> Scalar {
    let mut wide = [0u8; 64];
    wide[..32].copy_from_slice(b);
    let s = Scalar::from_bytes_mod_order(&wide);
    crate::ct::wipe(&mut wide);
    s
}

fn scalar_from_hash(h: &[u8]) -> Scalar {
    let mut wide = [0u8; 64];
    wide.copy_from_slice(&h[..64]);
    Scalar::from_bytes_mod_order(&wide)
}

/// The public key of a seed.
pub fn public_from_seed(seed: &[u8; 32]) -> [u8; 32] {
    let (mut a, mut prefix) = expand(seed);
    let s = scalar_from_32(&a);
    let pk = EdwardsPoint::mul_base(&s).compress();
    crate::ct::wipe(&mut a);
    crate::ct::wipe(&mut prefix);
    pk
}

/// Signs `msg` with `sk` (`seed ‖ public key`).
pub fn sign(sk: &[u8; 64], msg: &[u8]) -> [u8; 64] {
    let seed: &[u8; 32] = sk[..32].try_into().unwrap();
    let pk = &sk[32..];
    let (mut a, mut prefix) = expand(seed);
    let mut hr = Sha512::new();
    hr.update(&prefix);
    hr.update(msg);
    let mut rh = hr.finalize();
    let r = scalar_from_hash(&rh);
    let r_enc = EdwardsPoint::mul_base(&r).compress();
    let mut hk = Sha512::new();
    hk.update(&r_enc);
    hk.update(pk);
    hk.update(msg);
    let k = scalar_from_hash(&hk.finalize());
    let a_s = scalar_from_32(&a);
    let s = r.add(&k.mul(&a_s));
    let mut sig = [0u8; 64];
    sig[..32].copy_from_slice(&r_enc);
    sig[32..].copy_from_slice(&s.to_bytes());
    crate::ct::wipe(&mut a);
    crate::ct::wipe(&mut prefix);
    crate::ct::wipe(&mut rh[..]);
    sig
}

/// Verifies a detached signature.
pub fn verify(sig: &[u8; 64], msg: &[u8], pk: &[u8; 32]) -> bool {
    let r_bytes: &[u8; 32] = sig[..32].try_into().unwrap();
    let s_bytes: &[u8; 32] = sig[32..].try_into().unwrap();
    if Scalar::from_bytes_canonical(s_bytes).is_none() {
        return false;
    }
    let Some(a) = edwards::decompress(pk, true) else {
        return false;
    };
    if a.is_small_order() {
        return false;
    }
    if edwards::is_small_order_encoding(r_bytes) {
        return false;
    }
    let mut hk = Sha512::new();
    hk.update(r_bytes);
    hk.update(pk);
    hk.update(msg);
    let k = scalar_from_hash(&hk.finalize());
    let enc = edwards::double_scalar_mul_vartime(&k.to_bytes(), &a.negate(), s_bytes);
    crate::ct::eq(&enc, r_bytes)
}

/// `crypto_sign_ed25519_pk_to_curve25519`: the Montgomery `u` of a public
/// key, `None` for keys that are not canonical points of large order
/// (PHP: "conversion failed").
pub fn pk_to_curve25519(pk: &[u8; 32]) -> Option<[u8; 32]> {
    let a = edwards::decompress(pk, true)?;
    if a.is_small_order() {
        return None;
    }
    Some(crate::field::edwards_y_to_montgomery_u(&a.y_affine()))
}

/// `crypto_sign_ed25519_sk_to_curve25519`: the clamped scalar of the seed.
pub fn sk_to_curve25519(sk: &[u8; 64]) -> [u8; 32] {
    let (a, mut prefix) = expand(sk[..32].try_into().unwrap());
    crate::ct::wipe(&mut prefix);
    a
}

/// X25519 with the base point, through the Edwards fixed-base table:
/// `[a]B` on edwards25519 maps to `[a]9` on Curve25519 (RFC 7748 §4.1).
pub fn x25519_base(scalar: &[u8; 32]) -> [u8; 32] {
    let mut k = *scalar;
    k[0] &= 248;
    k[31] &= 127;
    k[31] |= 64;
    let s = scalar_from_32(&k);
    crate::ct::wipe(&mut k);
    let p = EdwardsPoint::mul_base(&s);
    crate::field::edwards_y_to_montgomery_u(&p.y_bytes())
}
