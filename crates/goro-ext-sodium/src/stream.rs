//! `crypto_secretbox` (XSalsa20-Poly1305), `crypto_stream` (XSalsa20) and
//! `crypto_stream_xchacha20` (libsodium documentation: "Secret-key
//! authenticated encryption", "Stream ciphers"; NaCl's secretbox: the first
//! 32 keystream bytes key Poly1305, the message is encrypted with the
//! keystream from byte 32, and the tag is prepended).

use purecrypto::cipher::{ChaCha20, Poly1305};

use crate::consts::*;
use crate::error::{SodiumResult, arg, want_len, zeroed};
use crate::{chacha, random_bytes, salsa};

fn alloc_len(length: i64) -> SodiumResult<Vec<u8>> {
    zeroed(length as u64)
}

/// XSalsa20-Poly1305 of `buf` in place; returns the tag.
pub(crate) fn secretbox_seal(key: &[u8; 32], nonce: &[u8; 24], buf: &mut [u8]) -> [u8; 16] {
    let (mut sub, n8) = salsa::xsalsa20_key(key, nonce);
    let mut otk = salsa::xor_after_key(&sub, &n8, buf);
    let mut p = Poly1305::new(&otk);
    p.update(buf);
    crate::ct::wipe(&mut otk);
    crate::ct::wipe(&mut sub);
    p.finish()
}

/// Verifies the tag over `buf` and decrypts it in place.
pub(crate) fn secretbox_open(key: &[u8; 32], nonce: &[u8; 24], tag: &[u8], buf: &mut [u8]) -> bool {
    let (mut sub, n8) = salsa::xsalsa20_key(key, nonce);
    let mut otk = [0u8; 64];
    salsa::xor_stream(&sub, &n8, 0, &mut otk[..]);
    let mut p = Poly1305::new(otk[..32].try_into().unwrap());
    p.update(buf);
    let ok = crate::ct::eq(&p.finish(), tag);
    if ok {
        let n = buf.len().min(32);
        for (b, k) in buf[..n].iter_mut().zip(&otk[32..]) {
            *b ^= k;
        }
        if buf.len() > 32 {
            salsa::xor_stream(&sub, &n8, 1, &mut buf[32..]);
        }
    }
    crate::ct::wipe(&mut otk);
    crate::ct::wipe(&mut sub);
    ok
}

/// `sodium_crypto_secretbox(string $message, string $nonce, string $key): string`
pub fn sodium_crypto_secretbox(message: &[u8], nonce: &[u8], key: &[u8]) -> SodiumResult<Vec<u8>> {
    const F: &str = "sodium_crypto_secretbox";
    want_len(
        nonce,
        SECRETBOX_NONCEBYTES,
        F,
        2,
        "nonce",
        "CRYPTO_SECRETBOX_NONCEBYTES",
    )?;
    want_len(
        key,
        SECRETBOX_KEYBYTES,
        F,
        3,
        "key",
        "CRYPTO_SECRETBOX_KEYBYTES",
    )?;
    let mut out = Vec::with_capacity(message.len() + 16);
    out.extend_from_slice(&[0u8; 16]);
    out.extend_from_slice(message);
    let tag = secretbox_seal(
        key.try_into().unwrap(),
        nonce.try_into().unwrap(),
        &mut out[16..],
    );
    out[..16].copy_from_slice(&tag);
    Ok(out)
}

/// `sodium_crypto_secretbox_open(string $ciphertext, string $nonce, string $key): string|false`
pub fn sodium_crypto_secretbox_open(
    ciphertext: &[u8],
    nonce: &[u8],
    key: &[u8],
) -> SodiumResult<Option<Vec<u8>>> {
    const F: &str = "sodium_crypto_secretbox_open";
    want_len(
        nonce,
        SECRETBOX_NONCEBYTES,
        F,
        2,
        "nonce",
        "CRYPTO_SECRETBOX_NONCEBYTES",
    )?;
    want_len(
        key,
        SECRETBOX_KEYBYTES,
        F,
        3,
        "key",
        "CRYPTO_SECRETBOX_KEYBYTES",
    )?;
    if ciphertext.len() < SECRETBOX_MACBYTES {
        return Ok(None);
    }
    let (tag, c) = ciphertext.split_at(16);
    let mut m = c.to_vec();
    if secretbox_open(
        key.try_into().unwrap(),
        nonce.try_into().unwrap(),
        tag,
        &mut m,
    ) {
        Ok(Some(m))
    } else {
        Ok(None)
    }
}

/// `sodium_crypto_secretbox_keygen(): string`
pub fn sodium_crypto_secretbox_keygen() -> SodiumResult<Vec<u8>> {
    random_bytes(SECRETBOX_KEYBYTES)
}

/// `sodium_crypto_stream(int $length, string $nonce, string $key): string`
pub fn sodium_crypto_stream(length: i64, nonce: &[u8], key: &[u8]) -> SodiumResult<Vec<u8>> {
    const F: &str = "sodium_crypto_stream";
    if length <= 0 {
        return Err(arg(F, 1, "length", "must be greater than 0"));
    }
    want_len(
        nonce,
        STREAM_NONCEBYTES,
        F,
        2,
        "nonce",
        "CRYPTO_STREAM_NONCEBYTES",
    )?;
    want_len(key, STREAM_KEYBYTES, F, 3, "key", "CRYPTO_STREAM_KEYBYTES")?;
    let mut out = alloc_len(length)?;
    salsa::xsalsa20_xor(
        key.try_into().unwrap(),
        nonce.try_into().unwrap(),
        0,
        &mut out,
    );
    Ok(out)
}

/// `sodium_crypto_stream_xor(string $message, string $nonce, string $key): string`
pub fn sodium_crypto_stream_xor(message: &[u8], nonce: &[u8], key: &[u8]) -> SodiumResult<Vec<u8>> {
    const F: &str = "sodium_crypto_stream_xor";
    want_len(
        nonce,
        STREAM_NONCEBYTES,
        F,
        2,
        "nonce",
        "CRYPTO_STREAM_NONCEBYTES",
    )?;
    want_len(key, STREAM_KEYBYTES, F, 3, "key", "CRYPTO_STREAM_KEYBYTES")?;
    let mut out = message.to_vec();
    salsa::xsalsa20_xor(
        key.try_into().unwrap(),
        nonce.try_into().unwrap(),
        0,
        &mut out,
    );
    Ok(out)
}

/// `sodium_crypto_stream_keygen(): string`
pub fn sodium_crypto_stream_keygen() -> SodiumResult<Vec<u8>> {
    random_bytes(STREAM_KEYBYTES)
}

fn xchacha20_xor(key: &[u8], nonce: &[u8], ic: u64, buf: &mut [u8]) {
    let (mut sub, n8) = chacha::xchacha20_key(key.try_into().unwrap(), nonce.try_into().unwrap());
    let c = ChaCha20::new(&sub);
    chacha::xor_ic(&c, &n8, ic, buf);
    crate::ct::wipe(&mut sub);
}

/// `sodium_crypto_stream_xchacha20(int $length, string $nonce, string $key): string`
pub fn sodium_crypto_stream_xchacha20(
    length: i64,
    nonce: &[u8],
    key: &[u8],
) -> SodiumResult<Vec<u8>> {
    const F: &str = "sodium_crypto_stream_xchacha20";
    if length <= 0 {
        return Err(arg(F, 1, "length", "must be greater than 0"));
    }
    want_len(
        nonce,
        STREAM_XCHACHA20_NONCEBYTES,
        F,
        2,
        "nonce",
        "CRYPTO_STREAM_XCHACHA20_NONCEBYTES",
    )?;
    want_len(
        key,
        STREAM_XCHACHA20_KEYBYTES,
        F,
        3,
        "key",
        "CRYPTO_STREAM_XCHACHA20_KEYBYTES",
    )?;
    let mut out = alloc_len(length)?;
    xchacha20_xor(key, nonce, 0, &mut out);
    Ok(out)
}

/// `sodium_crypto_stream_xchacha20_xor(string $message, string $nonce, string $key): string`
pub fn sodium_crypto_stream_xchacha20_xor(
    message: &[u8],
    nonce: &[u8],
    key: &[u8],
) -> SodiumResult<Vec<u8>> {
    const F: &str = "sodium_crypto_stream_xchacha20_xor";
    want_len(
        nonce,
        STREAM_XCHACHA20_NONCEBYTES,
        F,
        2,
        "nonce",
        "CRYPTO_STREAM_XCHACHA20_NONCEBYTES",
    )?;
    want_len(
        key,
        STREAM_XCHACHA20_KEYBYTES,
        F,
        3,
        "key",
        "CRYPTO_STREAM_XCHACHA20_KEYBYTES",
    )?;
    let mut out = message.to_vec();
    xchacha20_xor(key, nonce, 0, &mut out);
    Ok(out)
}

/// `sodium_crypto_stream_xchacha20_xor_ic(string $message, string $nonce, int $counter, string $key): string`
/// — the counter is the 64-bit initial block counter (a negative int is
/// read as its two's complement).
pub fn sodium_crypto_stream_xchacha20_xor_ic(
    message: &[u8],
    nonce: &[u8],
    counter: i64,
    key: &[u8],
) -> SodiumResult<Vec<u8>> {
    const F: &str = "sodium_crypto_stream_xchacha20_xor_ic";
    want_len(
        nonce,
        STREAM_XCHACHA20_NONCEBYTES,
        F,
        2,
        "nonce",
        "CRYPTO_STREAM_XCHACHA20_NONCEBYTES",
    )?;
    // Observed on PHP 8.5: the key's error names argument #3 ($counter).
    want_len(
        key,
        STREAM_XCHACHA20_KEYBYTES,
        F,
        3,
        "counter",
        "CRYPTO_STREAM_XCHACHA20_KEYBYTES",
    )?;
    let mut out = message.to_vec();
    xchacha20_xor(key, nonce, counter as u64, &mut out);
    Ok(out)
}

/// `sodium_crypto_stream_xchacha20_keygen(): string`
pub fn sodium_crypto_stream_xchacha20_keygen() -> SodiumResult<Vec<u8>> {
    random_bytes(STREAM_XCHACHA20_KEYBYTES)
}
