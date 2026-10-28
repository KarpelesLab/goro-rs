//! `crypto_sign` (Ed25519; libsodium documentation, "Public-key
//! signatures"). Secret keys are `seed ‖ public key`, key pairs
//! `secret key ‖ public key`, combined signed messages `signature ‖ message`.

use crate::consts::*;
use crate::error::{SodiumResult, exc, want_len};
use crate::{ed25519, random_bytes};

fn keypair_from_seed(seed: &[u8; 32]) -> Vec<u8> {
    let pk = ed25519::public_from_seed(seed);
    let mut kp = Vec::with_capacity(SIGN_KEYPAIRBYTES);
    kp.extend_from_slice(seed);
    kp.extend_from_slice(&pk);
    kp.extend_from_slice(&pk);
    kp
}

/// `sodium_crypto_sign_keypair(): string`
pub fn sodium_crypto_sign_keypair() -> SodiumResult<Vec<u8>> {
    let mut seed = random_bytes(SIGN_SEEDBYTES)?;
    let kp = keypair_from_seed(seed[..].try_into().unwrap());
    crate::ct::wipe(&mut seed);
    Ok(kp)
}

/// `sodium_crypto_sign_seed_keypair(string $seed): string`
pub fn sodium_crypto_sign_seed_keypair(seed: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        seed,
        SIGN_SEEDBYTES,
        "sodium_crypto_sign_seed_keypair",
        1,
        "seed",
        "CRYPTO_SIGN_SEEDBYTES",
    )?;
    Ok(keypair_from_seed(seed.try_into().unwrap()))
}

/// `sodium_crypto_sign_keypair_from_secretkey_and_publickey(string $secret_key, string $public_key): string`
pub fn sodium_crypto_sign_keypair_from_secretkey_and_publickey(
    secret_key: &[u8],
    public_key: &[u8],
) -> SodiumResult<Vec<u8>> {
    const F: &str = "sodium_crypto_sign_keypair_from_secretkey_and_publickey";
    want_len(
        secret_key,
        SIGN_SECRETKEYBYTES,
        F,
        1,
        "secret_key",
        "CRYPTO_SIGN_SECRETKEYBYTES",
    )?;
    want_len(
        public_key,
        SIGN_PUBLICKEYBYTES,
        F,
        2,
        "public_key",
        "CRYPTO_SIGN_PUBLICKEYBYTES",
    )?;
    Ok([secret_key, public_key].concat())
}

/// `sodium_crypto_sign_publickey(string $key_pair): string`
pub fn sodium_crypto_sign_publickey(key_pair: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        key_pair,
        SIGN_KEYPAIRBYTES,
        "sodium_crypto_sign_publickey",
        1,
        "key_pair",
        "CRYPTO_SIGN_KEYPAIRBYTES",
    )?;
    Ok(key_pair[SIGN_SECRETKEYBYTES..].to_vec())
}

/// `sodium_crypto_sign_secretkey(string $key_pair): string`
pub fn sodium_crypto_sign_secretkey(key_pair: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        key_pair,
        SIGN_KEYPAIRBYTES,
        "sodium_crypto_sign_secretkey",
        1,
        "key_pair",
        "CRYPTO_SIGN_KEYPAIRBYTES",
    )?;
    Ok(key_pair[..SIGN_SECRETKEYBYTES].to_vec())
}

/// `sodium_crypto_sign_publickey_from_secretkey(string $secret_key): string`
/// — the public half stored in the secret key.
pub fn sodium_crypto_sign_publickey_from_secretkey(secret_key: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        secret_key,
        SIGN_SECRETKEYBYTES,
        "sodium_crypto_sign_publickey_from_secretkey",
        1,
        "secret_key",
        "CRYPTO_SIGN_SECRETKEYBYTES",
    )?;
    Ok(secret_key[32..].to_vec())
}

/// `sodium_crypto_sign(string $message, string $secret_key): string`
pub fn sodium_crypto_sign(message: &[u8], secret_key: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        secret_key,
        SIGN_SECRETKEYBYTES,
        "sodium_crypto_sign",
        2,
        "secret_key",
        "CRYPTO_SIGN_SECRETKEYBYTES",
    )?;
    let sig = ed25519::sign(secret_key.try_into().unwrap(), message);
    let mut out = Vec::with_capacity(64 + message.len());
    out.extend_from_slice(&sig);
    out.extend_from_slice(message);
    Ok(out)
}

/// `sodium_crypto_sign_open(string $signed_message, string $public_key): string|false`
pub fn sodium_crypto_sign_open(
    signed_message: &[u8],
    public_key: &[u8],
) -> SodiumResult<Option<Vec<u8>>> {
    want_len(
        public_key,
        SIGN_PUBLICKEYBYTES,
        "sodium_crypto_sign_open",
        2,
        "public_key",
        "CRYPTO_SIGN_PUBLICKEYBYTES",
    )?;
    if signed_message.len() < SIGN_BYTES {
        return Ok(None);
    }
    let (sig, m) = signed_message.split_at(SIGN_BYTES);
    Ok(
        ed25519::verify(sig.try_into().unwrap(), m, public_key.try_into().unwrap())
            .then(|| m.to_vec()),
    )
}

/// `sodium_crypto_sign_detached(string $message, string $secret_key): string`
pub fn sodium_crypto_sign_detached(message: &[u8], secret_key: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        secret_key,
        SIGN_SECRETKEYBYTES,
        "sodium_crypto_sign_detached",
        2,
        "secret_key",
        "CRYPTO_SIGN_SECRETKEYBYTES",
    )?;
    Ok(ed25519::sign(secret_key.try_into().unwrap(), message).to_vec())
}

/// `sodium_crypto_sign_verify_detached(string $signature, string $message, string $public_key): bool`
pub fn sodium_crypto_sign_verify_detached(
    signature: &[u8],
    message: &[u8],
    public_key: &[u8],
) -> SodiumResult<bool> {
    const F: &str = "sodium_crypto_sign_verify_detached";
    want_len(
        signature,
        SIGN_BYTES,
        F,
        1,
        "signature",
        "CRYPTO_SIGN_BYTES",
    )?;
    want_len(
        public_key,
        SIGN_PUBLICKEYBYTES,
        F,
        3,
        "public_key",
        "CRYPTO_SIGN_PUBLICKEYBYTES",
    )?;
    Ok(ed25519::verify(
        signature.try_into().unwrap(),
        message,
        public_key.try_into().unwrap(),
    ))
}

/// `sodium_crypto_sign_ed25519_pk_to_curve25519(string $public_key): string`
pub fn sodium_crypto_sign_ed25519_pk_to_curve25519(public_key: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        public_key,
        SIGN_PUBLICKEYBYTES,
        "sodium_crypto_sign_ed25519_pk_to_curve25519",
        1,
        "public_key",
        "CRYPTO_SIGN_PUBLICKEYBYTES",
    )?;
    ed25519::pk_to_curve25519(public_key.try_into().unwrap())
        .map(|u| u.to_vec())
        .ok_or_else(|| exc("conversion failed"))
}

/// `sodium_crypto_sign_ed25519_sk_to_curve25519(string $secret_key): string`
pub fn sodium_crypto_sign_ed25519_sk_to_curve25519(secret_key: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        secret_key,
        SIGN_SECRETKEYBYTES,
        "sodium_crypto_sign_ed25519_sk_to_curve25519",
        1,
        "secret_key",
        "CRYPTO_SIGN_SECRETKEYBYTES",
    )?;
    Ok(ed25519::sk_to_curve25519(secret_key.try_into().unwrap()).to_vec())
}
