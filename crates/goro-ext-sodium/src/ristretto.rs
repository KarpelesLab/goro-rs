//! Ristretto255 (RFC 9496) group and scalar operations:
//! `crypto_core_ristretto255_*` and `crypto_scalarmult_ristretto255`
//! (libsodium documentation, "Ristretto"), on purecrypto's constant-time
//! implementation. Scalar inputs need not be reduced: they are taken modulo
//! the group order `L`.

use purecrypto::ec::ristretto255::{CompressedRistretto, RistrettoPoint, Scalar};

use crate::consts::*;
use crate::error::{SodiumResult, arg, exc, internal, want_len};
use crate::random_bytes;

fn point(b: &[u8]) -> Option<RistrettoPoint> {
    CompressedRistretto::from_slice(b.try_into().unwrap()).decompress()
}

fn scalar(b: &[u8]) -> Scalar {
    let mut wide = [0u8; 64];
    wide[..32].copy_from_slice(b);
    let s = Scalar::from_bytes_mod_order(&wide);
    crate::ct::wipe(&mut wide);
    s
}

fn is_identity(p: &RistrettoPoint) -> bool {
    bool::from(p.ct_eq(&RistrettoPoint::identity()))
}

/// `sodium_crypto_core_ristretto255_is_valid_point(string $s): bool`
pub fn sodium_crypto_core_ristretto255_is_valid_point(s: &[u8]) -> SodiumResult<bool> {
    want_len(
        s,
        RISTRETTO255_BYTES,
        "sodium_crypto_core_ristretto255_is_valid_point",
        1,
        "s",
        "CRYPTO_CORE_RISTRETTO255_BYTES",
    )?;
    Ok(point(s).is_some())
}

fn binop(func: &str, p: &[u8], q: &[u8], sub: bool) -> SodiumResult<Vec<u8>> {
    want_len(
        p,
        RISTRETTO255_BYTES,
        func,
        1,
        "p",
        "CRYPTO_CORE_RISTRETTO255_BYTES",
    )?;
    want_len(
        q,
        RISTRETTO255_BYTES,
        func,
        2,
        "q",
        "CRYPTO_CORE_RISTRETTO255_BYTES",
    )?;
    let (Some(a), Some(b)) = (point(p), point(q)) else {
        return Err(internal());
    };
    let r = if sub { a.sub(&b) } else { a.add(&b) };
    Ok(r.compress().to_bytes().to_vec())
}

/// `sodium_crypto_core_ristretto255_add(string $p, string $q): string`
pub fn sodium_crypto_core_ristretto255_add(p: &[u8], q: &[u8]) -> SodiumResult<Vec<u8>> {
    binop("sodium_crypto_core_ristretto255_add", p, q, false)
}

/// `sodium_crypto_core_ristretto255_sub(string $p, string $q): string`
pub fn sodium_crypto_core_ristretto255_sub(p: &[u8], q: &[u8]) -> SodiumResult<Vec<u8>> {
    binop("sodium_crypto_core_ristretto255_sub", p, q, true)
}

/// `sodium_crypto_core_ristretto255_from_hash(string $s): string` — the
/// RFC 9496 §4.3.4 one-way map of 64 bytes.
pub fn sodium_crypto_core_ristretto255_from_hash(s: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        s,
        RISTRETTO255_HASHBYTES,
        "sodium_crypto_core_ristretto255_from_hash",
        1,
        "s",
        "CRYPTO_CORE_RISTRETTO255_HASHBYTES",
    )?;
    Ok(RistrettoPoint::from_uniform_bytes(s.try_into().unwrap())
        .compress()
        .to_bytes()
        .to_vec())
}

/// `sodium_crypto_core_ristretto255_random(): string`
pub fn sodium_crypto_core_ristretto255_random() -> SodiumResult<Vec<u8>> {
    let mut h = random_bytes(64)?;
    let p = RistrettoPoint::from_uniform_bytes(h[..].try_into().unwrap());
    crate::ct::wipe(&mut h);
    Ok(p.compress().to_bytes().to_vec())
}

/// `sodium_crypto_core_ristretto255_scalar_random(): string`
pub fn sodium_crypto_core_ristretto255_scalar_random() -> SodiumResult<Vec<u8>> {
    let mut h = random_bytes(64)?;
    let s = Scalar::from_bytes_mod_order(h[..].try_into().unwrap());
    crate::ct::wipe(&mut h);
    Ok(s.to_bytes().to_vec())
}

fn unary(func: &str, s: &[u8]) -> SodiumResult<Scalar> {
    want_len(
        s,
        RISTRETTO255_SCALARBYTES,
        func,
        1,
        "s",
        "CRYPTO_CORE_RISTRETTO255_SCALARBYTES",
    )?;
    Ok(scalar(s))
}

fn binary(func: &str, x: &[u8], y: &[u8]) -> SodiumResult<(Scalar, Scalar)> {
    want_len(
        x,
        RISTRETTO255_SCALARBYTES,
        func,
        1,
        "x",
        "CRYPTO_CORE_RISTRETTO255_SCALARBYTES",
    )?;
    want_len(
        y,
        RISTRETTO255_SCALARBYTES,
        func,
        2,
        "y",
        "CRYPTO_CORE_RISTRETTO255_SCALARBYTES",
    )?;
    Ok((scalar(x), scalar(y)))
}

/// `sodium_crypto_core_ristretto255_scalar_invert(string $s): string` —
/// fails for the all-zero string only (observed on PHP 8.5: `L` itself
/// "inverts" to zero).
pub fn sodium_crypto_core_ristretto255_scalar_invert(s: &[u8]) -> SodiumResult<Vec<u8>> {
    let x = unary("sodium_crypto_core_ristretto255_scalar_invert", s)?;
    if crate::ct::is_zero(s) {
        return Err(internal());
    }
    Ok(x.invert().to_bytes().to_vec())
}

/// `(x + y) mod 2^256`, reduced modulo `L`: libsodium adds the 32-byte
/// strings before reducing (observed on PHP 8.5 with unreduced inputs).
fn add_wrapping(x: &[u8], y: &[u8]) -> Scalar {
    let mut sum = [0u8; 32];
    sum.copy_from_slice(x);
    crate::ct::add_le(&mut sum, y);
    let s = scalar(&sum);
    crate::ct::wipe(&mut sum);
    s
}

/// `sodium_crypto_core_ristretto255_scalar_negate(string $s): string`
pub fn sodium_crypto_core_ristretto255_scalar_negate(s: &[u8]) -> SodiumResult<Vec<u8>> {
    let x = unary("sodium_crypto_core_ristretto255_scalar_negate", s)?;
    Ok(x.negate().to_bytes().to_vec())
}

/// `sodium_crypto_core_ristretto255_scalar_complement(string $s): string` — `1 − s`.
pub fn sodium_crypto_core_ristretto255_scalar_complement(s: &[u8]) -> SodiumResult<Vec<u8>> {
    let x = unary("sodium_crypto_core_ristretto255_scalar_complement", s)?;
    Ok(Scalar::ONE.sub(&x).to_bytes().to_vec())
}

/// `sodium_crypto_core_ristretto255_scalar_add(string $x, string $y): string`
pub fn sodium_crypto_core_ristretto255_scalar_add(x: &[u8], y: &[u8]) -> SodiumResult<Vec<u8>> {
    binary("sodium_crypto_core_ristretto255_scalar_add", x, y)?;
    Ok(add_wrapping(x, y).to_bytes().to_vec())
}

/// `sodium_crypto_core_ristretto255_scalar_sub(string $x, string $y): string`
/// — `x + (−y mod L)` with the same 256-bit wrap-around as `scalar_add`.
pub fn sodium_crypto_core_ristretto255_scalar_sub(x: &[u8], y: &[u8]) -> SodiumResult<Vec<u8>> {
    let (_, b) = binary("sodium_crypto_core_ristretto255_scalar_sub", x, y)?;
    let neg = b.negate().to_bytes();
    Ok(add_wrapping(x, &neg).to_bytes().to_vec())
}

/// `sodium_crypto_core_ristretto255_scalar_mul(string $x, string $y): string`
pub fn sodium_crypto_core_ristretto255_scalar_mul(x: &[u8], y: &[u8]) -> SodiumResult<Vec<u8>> {
    let (a, b) = binary("sodium_crypto_core_ristretto255_scalar_mul", x, y)?;
    Ok(a.mul(&b).to_bytes().to_vec())
}

/// `sodium_crypto_core_ristretto255_scalar_reduce(string $s): string`
pub fn sodium_crypto_core_ristretto255_scalar_reduce(s: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        s,
        RISTRETTO255_NONREDUCEDSCALARBYTES,
        "sodium_crypto_core_ristretto255_scalar_reduce",
        1,
        "s",
        "CRYPTO_CORE_RISTRETTO255_NONREDUCEDSCALARBYTES",
    )?;
    Ok(Scalar::from_bytes_mod_order(s.try_into().unwrap())
        .to_bytes()
        .to_vec())
}

/// The scalar of `crypto_scalarmult_ristretto255`: the top bit is ignored.
fn mult_scalar(n: &[u8]) -> Scalar {
    let mut b = [0u8; 32];
    b.copy_from_slice(n);
    b[31] &= 0x7f;
    let s = scalar(&b);
    crate::ct::wipe(&mut b);
    s
}

/// `sodium_crypto_scalarmult_ristretto255(string $n, string $p): string`
pub fn sodium_crypto_scalarmult_ristretto255(n: &[u8], p: &[u8]) -> SodiumResult<Vec<u8>> {
    const F: &str = "sodium_crypto_scalarmult_ristretto255";
    want_len(
        n,
        RISTRETTO255_SCALARBYTES,
        F,
        1,
        "n",
        "CRYPTO_SCALARMULT_RISTRETTO255_SCALARBYTES",
    )?;
    want_len(
        p,
        RISTRETTO255_BYTES,
        F,
        2,
        "p",
        "CRYPTO_SCALARMULT_RISTRETTO255_BYTES",
    )?;
    let identity = || exc("Result is identity element");
    let q = point(p).ok_or_else(identity)?;
    let r = q.mul(&mult_scalar(n));
    if is_identity(&r) {
        return Err(identity());
    }
    Ok(r.compress().to_bytes().to_vec())
}

/// `sodium_crypto_scalarmult_ristretto255_base(string $n): string`
pub fn sodium_crypto_scalarmult_ristretto255_base(n: &[u8]) -> SodiumResult<Vec<u8>> {
    const F: &str = "sodium_crypto_scalarmult_ristretto255_base";
    want_len(
        n,
        RISTRETTO255_SCALARBYTES,
        F,
        1,
        "n",
        "CRYPTO_SCALARMULT_RISTRETTO255_SCALARBYTES",
    )?;
    let r = RistrettoPoint::mul_base(&mult_scalar(n));
    if is_identity(&r) {
        return Err(arg(F, 1, "n", "must not be zero"));
    }
    Ok(r.compress().to_bytes().to_vec())
}
