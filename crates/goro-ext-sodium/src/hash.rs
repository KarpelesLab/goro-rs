//! Hashing and MACs: `crypto_generichash` (BLAKE2b), `crypto_shorthash`
//! (SipHash-2-4), `crypto_kdf` (BLAKE2b with salt and personalization) and
//! `crypto_auth` (HMAC-SHA-512 truncated to 256 bits), per libsodium's
//! documentation and the PHP manual.

use goro_runtime::Value;
use purecrypto::hash::{Digest, Hmac, Sha512};
use purecrypto::mac::SipHash24;

use crate::blake2b::{self, Blake2b, STATE_BYTES};
use crate::consts::*;
use crate::error::{SodiumResult, arg, exc, want_len};
use crate::random_bytes;

fn check_out_len(len: i64) -> SodiumResult<usize> {
    if len < GENERICHASH_BYTES_MIN as i64 || len > GENERICHASH_BYTES_MAX as i64 {
        return Err(exc("unsupported output length"));
    }
    Ok(len as usize)
}

fn check_key(key: &[u8]) -> SodiumResult<()> {
    if !key.is_empty()
        && !(GENERICHASH_KEYBYTES_MIN..=GENERICHASH_KEYBYTES_MAX).contains(&key.len())
    {
        return Err(exc("unsupported key length"));
    }
    Ok(())
}

/// `sodium_crypto_generichash(string $message, string $key = "", int $length = 32): string`
pub fn sodium_crypto_generichash(message: &[u8], key: &[u8], length: i64) -> SodiumResult<Vec<u8>> {
    let n = check_out_len(length)?;
    check_key(key)?;
    let mut out = vec![0u8; n];
    blake2b::hash(&mut out, key, message);
    Ok(out)
}

/// `sodium_crypto_generichash_keygen(): string`
pub fn sodium_crypto_generichash_keygen() -> SodiumResult<Vec<u8>> {
    random_bytes(GENERICHASH_KEYBYTES)
}

/// `sodium_crypto_generichash_init(string $key = "", int $length = 32): string`
/// — the 384-byte state string.
pub fn sodium_crypto_generichash_init(key: &[u8], length: i64) -> SodiumResult<Vec<u8>> {
    let n = check_out_len(length)?;
    check_key(key)?;
    Ok(Blake2b::new(n, key, None, None).to_bytes())
}

/// The state string held by a `string &$state` argument.
fn state_arg<'a>(
    func: &str,
    state: &'a mut Value,
    len: usize,
    len_err: impl Fn() -> crate::SodiumError,
) -> SodiumResult<&'a mut goro_runtime::PhpStr> {
    match state {
        Value::Str(s) => {
            if s.len() != len {
                return Err(len_err());
            }
            Ok(s)
        }
        _ => Err(arg(func, 1, "state", "must be a reference to a state")),
    }
}

/// `sodium_crypto_generichash_update(string &$state, string $message): true`
pub fn sodium_crypto_generichash_update(state: &mut Value, message: &[u8]) -> SodiumResult<bool> {
    let s = state_arg(
        "sodium_crypto_generichash_update",
        state,
        STATE_BYTES,
        || exc("incorrect state length"),
    )?;
    let mut st = Blake2b::from_bytes(s.as_bytes());
    st.update(message);
    *s = st.to_bytes().into();
    Ok(true)
}

/// `sodium_crypto_generichash_final(string &$state, int $length = 32): string`
/// — on success the state variable becomes null (observed on PHP 8.5).
pub fn sodium_crypto_generichash_final(state: &mut Value, length: i64) -> SodiumResult<Vec<u8>> {
    let s = state_arg(
        "sodium_crypto_generichash_final",
        state,
        STATE_BYTES,
        || exc("incorrect state length"),
    )?;
    let n = check_out_len(length)?;
    let st = Blake2b::from_bytes(s.as_bytes());
    let mut out = vec![0u8; n];
    st.finalize_into(&mut out);
    crate::ct::wipe(s.make_mut());
    *state = Value::Null;
    Ok(out)
}

/// `sodium_crypto_shorthash(string $message, string $key): string`
pub fn sodium_crypto_shorthash(message: &[u8], key: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        key,
        SHORTHASH_KEYBYTES,
        "sodium_crypto_shorthash",
        2,
        "key",
        "CRYPTO_SHORTHASH_KEYBYTES",
    )?;
    Ok(SipHash24::compute(key.try_into().unwrap(), message).to_vec())
}

/// `sodium_crypto_shorthash_keygen(): string`
pub fn sodium_crypto_shorthash_keygen() -> SodiumResult<Vec<u8>> {
    random_bytes(SHORTHASH_KEYBYTES)
}

/// `sodium_crypto_kdf_derive_from_key(int $subkey_length, int $subkey_id, string $context, string $key): string`
pub fn sodium_crypto_kdf_derive_from_key(
    subkey_length: i64,
    subkey_id: i64,
    context: &[u8],
    key: &[u8],
) -> SodiumResult<Vec<u8>> {
    const F: &str = "sodium_crypto_kdf_derive_from_key";
    if subkey_length < KDF_BYTES_MIN as i64 {
        return Err(arg(
            F,
            1,
            "subkey_length",
            "must be greater than or equal to SODIUM_CRYPTO_KDF_BYTES_MIN",
        ));
    }
    if subkey_length > KDF_BYTES_MAX as i64 {
        return Err(arg(
            F,
            1,
            "subkey_length",
            "must be less than or equal to SODIUM_CRYPTO_KDF_BYTES_MAX",
        ));
    }
    if subkey_id < 0 {
        return Err(arg(F, 2, "subkey_id", "must be greater than or equal to 0"));
    }
    want_len(
        context,
        KDF_CONTEXTBYTES,
        F,
        3,
        "context",
        "CRYPTO_KDF_CONTEXTBYTES",
    )?;
    want_len(key, KDF_KEYBYTES, F, 4, "key", "CRYPTO_KDF_KEYBYTES")?;
    // libsodium documentation (Key derivation): BLAKE2b keyed with the
    // master key, salt = le64(subkey_id) ‖ 0^8, personal = context ‖ 0^8.
    let mut salt = [0u8; 16];
    salt[..8].copy_from_slice(&(subkey_id as u64).to_le_bytes());
    let mut personal = [0u8; 16];
    personal[..8].copy_from_slice(context);
    let st = Blake2b::new(subkey_length as usize, key, Some(&salt), Some(&personal));
    let mut out = vec![0u8; subkey_length as usize];
    st.finalize_into(&mut out);
    Ok(out)
}

/// `sodium_crypto_kdf_keygen(): string`
pub fn sodium_crypto_kdf_keygen() -> SodiumResult<Vec<u8>> {
    random_bytes(KDF_KEYBYTES)
}

fn hmac512_256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut h = Hmac::<Sha512>::new(key);
    h.update(message);
    let full = h.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&full[..32]);
    out
}

/// `sodium_crypto_auth(string $message, string $key): string`
pub fn sodium_crypto_auth(message: &[u8], key: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        key,
        AUTH_KEYBYTES,
        "sodium_crypto_auth",
        2,
        "key",
        "CRYPTO_AUTH_KEYBYTES",
    )?;
    Ok(hmac512_256(key, message).to_vec())
}

/// `sodium_crypto_auth_verify(string $mac, string $message, string $key): bool`
pub fn sodium_crypto_auth_verify(mac: &[u8], message: &[u8], key: &[u8]) -> SodiumResult<bool> {
    const F: &str = "sodium_crypto_auth_verify";
    want_len(mac, AUTH_BYTES, F, 1, "mac", "CRYPTO_AUTH_BYTES")?;
    want_len(key, AUTH_KEYBYTES, F, 3, "key", "CRYPTO_AUTH_KEYBYTES")?;
    Ok(crate::ct::eq(&hmac512_256(key, message), mac))
}

/// `sodium_crypto_auth_keygen(): string`
pub fn sodium_crypto_auth_keygen() -> SodiumResult<Vec<u8>> {
    random_bytes(AUTH_KEYBYTES)
}

pub(crate) fn sha512(data: &[u8]) -> [u8; 64] {
    let d = Sha512::digest(data);
    let mut out = [0u8; 64];
    out.copy_from_slice(&d[..64]);
    out
}
