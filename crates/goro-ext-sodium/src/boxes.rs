//! Public-key encryption and key exchange: `crypto_box`
//! (X25519-XSalsa20-Poly1305), sealed boxes, `crypto_scalarmult` (X25519)
//! and `crypto_kx` (libsodium documentation: "Public-key authenticated
//! encryption", "Sealed boxes", "Point*scalar multiplication", "Key
//! exchange"; NaCl: the box key is HSalsa20 of the shared secret with a
//! zero input).
//!
//! Key pairs are `secret key ‖ public key`. A shared secret that is all
//! zeros (a small-order public key) is rejected, as libsodium does.

use crate::blake2b::{self, Blake2b};
use crate::consts::*;
use crate::error::{SodiumResult, internal, want_len};
use crate::{ct, ed25519, field, random_bytes, salsa, stream};

/// X25519, `None` for an all-zero result.
fn dh(sk: &[u8], pk: &[u8]) -> Option<[u8; 32]> {
    let q = field::x25519(sk.try_into().unwrap(), pk.try_into().unwrap());
    if ct::is_zero(&q) { None } else { Some(q) }
}

/// The `crypto_box` key for a key pair, `None` for a small-order key.
fn beforenm(pk: &[u8], sk: &[u8]) -> Option<[u8; 32]> {
    let mut q = dh(sk, pk)?;
    let k = salsa::hsalsa20(&q, &[0; 16]);
    ct::wipe(&mut q);
    Some(k)
}

fn box_seal_with(message: &[u8], nonce: &[u8; 24], pk: &[u8], sk: &[u8]) -> Option<Vec<u8>> {
    let mut k = beforenm(pk, sk)?;
    let mut out = Vec::with_capacity(message.len() + 16);
    out.extend_from_slice(&[0u8; 16]);
    out.extend_from_slice(message);
    let tag = stream::secretbox_seal(&k, nonce, &mut out[16..]);
    out[..16].copy_from_slice(&tag);
    ct::wipe(&mut k);
    Some(out)
}

fn box_open_with(c: &[u8], nonce: &[u8; 24], pk: &[u8], sk: &[u8]) -> Option<Vec<u8>> {
    if c.len() < BOX_MACBYTES {
        return None;
    }
    let mut k = beforenm(pk, sk)?;
    let (tag, body) = c.split_at(16);
    let mut m = body.to_vec();
    let ok = stream::secretbox_open(&k, nonce, tag, &mut m);
    ct::wipe(&mut k);
    ok.then_some(m)
}

fn keypair_from_sk(sk: &[u8; 32]) -> Vec<u8> {
    let pk = ed25519::x25519_base(sk);
    let mut kp = Vec::with_capacity(64);
    kp.extend_from_slice(sk);
    kp.extend_from_slice(&pk);
    kp
}

/// `sodium_crypto_box(string $message, string $nonce, string $key_pair): string`
pub fn sodium_crypto_box(message: &[u8], nonce: &[u8], key_pair: &[u8]) -> SodiumResult<Vec<u8>> {
    const F: &str = "sodium_crypto_box";
    want_len(
        nonce,
        BOX_NONCEBYTES,
        F,
        2,
        "nonce",
        "CRYPTO_BOX_NONCEBYTES",
    )?;
    want_len(
        key_pair,
        BOX_KEYPAIRBYTES,
        F,
        3,
        "key_pair",
        "CRYPTO_BOX_KEYPAIRBYTES",
    )?;
    let (sk, pk) = key_pair.split_at(32);
    box_seal_with(message, nonce.try_into().unwrap(), pk, sk).ok_or_else(internal)
}

/// `sodium_crypto_box_open(string $ciphertext, string $nonce, string $key_pair): string|false`
pub fn sodium_crypto_box_open(
    ciphertext: &[u8],
    nonce: &[u8],
    key_pair: &[u8],
) -> SodiumResult<Option<Vec<u8>>> {
    const F: &str = "sodium_crypto_box_open";
    want_len(
        nonce,
        BOX_NONCEBYTES,
        F,
        2,
        "nonce",
        "CRYPTO_BOX_NONCEBYTES",
    )?;
    want_len(
        key_pair,
        BOX_KEYPAIRBYTES,
        F,
        3,
        "key_pair",
        "CRYPTO_BOX_KEYPAIRBYTES",
    )?;
    let (sk, pk) = key_pair.split_at(32);
    Ok(box_open_with(ciphertext, nonce.try_into().unwrap(), pk, sk))
}

/// `sodium_crypto_box_keypair(): string`
pub fn sodium_crypto_box_keypair() -> SodiumResult<Vec<u8>> {
    let mut sk = random_bytes(BOX_SECRETKEYBYTES)?;
    let kp = keypair_from_sk(sk[..].try_into().unwrap());
    ct::wipe(&mut sk);
    Ok(kp)
}

/// `sodium_crypto_box_seed_keypair(string $seed): string` — the secret key
/// is the first half of SHA-512 of the seed (observed on PHP 8.5).
pub fn sodium_crypto_box_seed_keypair(seed: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        seed,
        BOX_SEEDBYTES,
        "sodium_crypto_box_seed_keypair",
        1,
        "seed",
        "CRYPTO_BOX_SEEDBYTES",
    )?;
    let mut h = crate::hash::sha512(seed);
    let kp = keypair_from_sk(h[..32].try_into().unwrap());
    ct::wipe(&mut h);
    Ok(kp)
}

/// `sodium_crypto_box_keypair_from_secretkey_and_publickey(string $secret_key, string $public_key): string`
pub fn sodium_crypto_box_keypair_from_secretkey_and_publickey(
    secret_key: &[u8],
    public_key: &[u8],
) -> SodiumResult<Vec<u8>> {
    const F: &str = "sodium_crypto_box_keypair_from_secretkey_and_publickey";
    want_len(
        secret_key,
        BOX_SECRETKEYBYTES,
        F,
        1,
        "secret_key",
        "CRYPTO_BOX_SECRETKEYBYTES",
    )?;
    want_len(
        public_key,
        BOX_PUBLICKEYBYTES,
        F,
        2,
        "public_key",
        "CRYPTO_BOX_PUBLICKEYBYTES",
    )?;
    Ok([secret_key, public_key].concat())
}

/// `sodium_crypto_box_publickey(string $key_pair): string`
pub fn sodium_crypto_box_publickey(key_pair: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        key_pair,
        BOX_KEYPAIRBYTES,
        "sodium_crypto_box_publickey",
        1,
        "key_pair",
        "CRYPTO_BOX_KEYPAIRBYTES",
    )?;
    Ok(key_pair[32..].to_vec())
}

/// `sodium_crypto_box_secretkey(string $key_pair): string`
pub fn sodium_crypto_box_secretkey(key_pair: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        key_pair,
        BOX_KEYPAIRBYTES,
        "sodium_crypto_box_secretkey",
        1,
        "key_pair",
        "CRYPTO_BOX_KEYPAIRBYTES",
    )?;
    Ok(key_pair[..32].to_vec())
}

/// `sodium_crypto_box_publickey_from_secretkey(string $secret_key): string`
pub fn sodium_crypto_box_publickey_from_secretkey(secret_key: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        secret_key,
        BOX_SECRETKEYBYTES,
        "sodium_crypto_box_publickey_from_secretkey",
        1,
        "secret_key",
        "CRYPTO_BOX_SECRETKEYBYTES",
    )?;
    Ok(ed25519::x25519_base(secret_key.try_into().unwrap()).to_vec())
}

/// The sealed-box nonce: BLAKE2b-192 of `epk ‖ pk` (libsodium
/// documentation, "Sealed boxes: algorithm details").
fn seal_nonce(epk: &[u8], pk: &[u8]) -> [u8; 24] {
    let mut st = Blake2b::new(24, b"", None, None);
    st.update(epk);
    st.update(pk);
    let mut n = [0u8; 24];
    st.finalize_into(&mut n);
    n
}

/// `sodium_crypto_box_seal(string $message, string $public_key): string`
pub fn sodium_crypto_box_seal(message: &[u8], public_key: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        public_key,
        BOX_PUBLICKEYBYTES,
        "sodium_crypto_box_seal",
        2,
        "public_key",
        "CRYPTO_BOX_PUBLICKEYBYTES",
    )?;
    let mut esk = random_bytes(32)?;
    let epk = ed25519::x25519_base(esk[..].try_into().unwrap());
    let nonce = seal_nonce(&epk, public_key);
    let sealed = box_seal_with(message, &nonce, public_key, &esk);
    ct::wipe(&mut esk);
    let c = sealed.ok_or_else(internal)?;
    let mut out = Vec::with_capacity(32 + c.len());
    out.extend_from_slice(&epk);
    out.extend_from_slice(&c);
    Ok(out)
}

/// `sodium_crypto_box_seal_open(string $ciphertext, string $key_pair): string|false`
pub fn sodium_crypto_box_seal_open(
    ciphertext: &[u8],
    key_pair: &[u8],
) -> SodiumResult<Option<Vec<u8>>> {
    want_len(
        key_pair,
        BOX_KEYPAIRBYTES,
        "sodium_crypto_box_seal_open",
        2,
        "key_pair",
        "CRYPTO_BOX_KEYPAIRBYTES",
    )?;
    if ciphertext.len() < BOX_SEALBYTES {
        return Ok(None);
    }
    let (sk, pk) = key_pair.split_at(32);
    let (epk, c) = ciphertext.split_at(32);
    let nonce = seal_nonce(epk, pk);
    Ok(box_open_with(c, &nonce, epk, sk))
}

/// `sodium_crypto_scalarmult(string $n, string $p): string`
pub fn sodium_crypto_scalarmult(n: &[u8], p: &[u8]) -> SodiumResult<Vec<u8>> {
    const F: &str = "sodium_crypto_scalarmult";
    want_len(
        n,
        SCALARMULT_SCALARBYTES,
        F,
        1,
        "n",
        "CRYPTO_SCALARMULT_SCALARBYTES",
    )?;
    want_len(p, SCALARMULT_BYTES, F, 2, "p", "CRYPTO_SCALARMULT_BYTES")?;
    dh(n, p).map(|q| q.to_vec()).ok_or_else(internal)
}

/// `sodium_crypto_scalarmult_base(string $secret_key): string`
pub fn sodium_crypto_scalarmult_base(secret_key: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        secret_key,
        BOX_SECRETKEYBYTES,
        "sodium_crypto_scalarmult_base",
        1,
        "secret_key",
        "CRYPTO_BOX_SECRETKEYBYTES",
    )?;
    Ok(ed25519::x25519_base(secret_key.try_into().unwrap()).to_vec())
}

/// `sodium_crypto_kx_keypair(): string`
pub fn sodium_crypto_kx_keypair() -> SodiumResult<Vec<u8>> {
    let mut sk = random_bytes(KX_SECRETKEYBYTES)?;
    let kp = keypair_from_sk(sk[..].try_into().unwrap());
    ct::wipe(&mut sk);
    Ok(kp)
}

/// `sodium_crypto_kx_seed_keypair(string $seed): string` — the secret key
/// is BLAKE2b-256 of the seed (observed on PHP 8.5).
pub fn sodium_crypto_kx_seed_keypair(seed: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        seed,
        KX_SEEDBYTES,
        "sodium_crypto_kx_seed_keypair",
        1,
        "seed",
        "CRYPTO_KX_SEEDBYTES",
    )?;
    let mut sk = [0u8; 32];
    blake2b::hash(&mut sk, b"", seed);
    let kp = keypair_from_sk(&sk);
    ct::wipe(&mut sk);
    Ok(kp)
}

/// `sodium_crypto_kx_publickey(string $key_pair): string`
pub fn sodium_crypto_kx_publickey(key_pair: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        key_pair,
        KX_KEYPAIRBYTES,
        "sodium_crypto_kx_publickey",
        1,
        "key_pair",
        "CRYPTO_KX_KEYPAIRBYTES",
    )?;
    Ok(key_pair[32..].to_vec())
}

/// `sodium_crypto_kx_secretkey(string $key_pair): string`
pub fn sodium_crypto_kx_secretkey(key_pair: &[u8]) -> SodiumResult<Vec<u8>> {
    want_len(
        key_pair,
        KX_KEYPAIRBYTES,
        "sodium_crypto_kx_secretkey",
        1,
        "key_pair",
        "CRYPTO_KX_KEYPAIRBYTES",
    )?;
    Ok(key_pair[..32].to_vec())
}

/// BLAKE2b-512 of `q ‖ client_pk ‖ server_pk` (libsodium documentation,
/// "Key exchange: algorithm details").
fn kx_keys(q: &[u8; 32], client_pk: &[u8], server_pk: &[u8]) -> [u8; 64] {
    let mut st = Blake2b::new(64, b"", None, None);
    st.update(q);
    st.update(client_pk);
    st.update(server_pk);
    let mut h = [0u8; 64];
    st.finalize_into(&mut h);
    h
}

/// `sodium_crypto_kx_client_session_keys(string $client_key_pair, string $server_key): array`
/// — `[rx, tx]`.
pub fn sodium_crypto_kx_client_session_keys(
    client_key_pair: &[u8],
    server_key: &[u8],
) -> SodiumResult<(Vec<u8>, Vec<u8>)> {
    const F: &str = "sodium_crypto_kx_client_session_keys";
    want_len(
        client_key_pair,
        KX_KEYPAIRBYTES,
        F,
        1,
        "client_key_pair",
        "CRYPTO_KX_KEYPAIRBYTES",
    )?;
    want_len(
        server_key,
        KX_PUBLICKEYBYTES,
        F,
        2,
        "server_key",
        "CRYPTO_KX_PUBLICKEYBYTES",
    )?;
    let (sk, pk) = client_key_pair.split_at(32);
    let mut q = dh(sk, server_key).ok_or_else(internal)?;
    let mut h = kx_keys(&q, pk, server_key);
    let keys = (h[..32].to_vec(), h[32..].to_vec());
    ct::wipe(&mut q);
    ct::wipe(&mut h);
    Ok(keys)
}

/// `sodium_crypto_kx_server_session_keys(string $server_key_pair, string $client_key): array`
/// — `[rx, tx]`.
pub fn sodium_crypto_kx_server_session_keys(
    server_key_pair: &[u8],
    client_key: &[u8],
) -> SodiumResult<(Vec<u8>, Vec<u8>)> {
    const F: &str = "sodium_crypto_kx_server_session_keys";
    want_len(
        server_key_pair,
        KX_KEYPAIRBYTES,
        F,
        1,
        "server_key_pair",
        "CRYPTO_KX_KEYPAIRBYTES",
    )?;
    want_len(
        client_key,
        KX_PUBLICKEYBYTES,
        F,
        2,
        "client_key",
        "CRYPTO_KX_PUBLICKEYBYTES",
    )?;
    let (sk, pk) = server_key_pair.split_at(32);
    let mut q = dh(sk, client_key).ok_or_else(internal)?;
    let mut h = kx_keys(&q, client_key, pk);
    let keys = (h[32..].to_vec(), h[..32].to_vec());
    ct::wipe(&mut q);
    ct::wipe(&mut h);
    Ok(keys)
}
