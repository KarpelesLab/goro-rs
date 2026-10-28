//! `crypto_secretstream_xchacha20poly1305` (libsodium documentation,
//! "Encrypted streams and file encryption", including its "Algorithm
//! details").
//!
//! The state string is 52 bytes, as on PHP 8.5: the 32-byte subkey, the
//! 12-byte ChaCha20 (RFC 8439) nonce — a 32-bit little-endian counter
//! starting at 1 followed by an 8-byte "inonce" — and 8 zero bytes. Each
//! message is `encrypted tag byte ‖ ciphertext ‖ 16-byte MAC`: block 0 of
//! the keystream keys Poly1305, block 1 encrypts a 64-byte block holding
//! the tag, the message is encrypted from block 2, and the MAC covers
//! `ad ‖ pad16(ad) ‖ encrypted block ‖ c ‖ 0^(|c| mod 16) ‖ le64(|ad|) ‖
//! le64(64 + |c|)` (that second padding length observed on PHP 8.5).
//! Afterwards the inonce is XORed with the MAC's first 8 bytes and the
//! counter incremented; the key is ratcheted when the counter wraps or the
//! tag has the REKEY bit. A failed `pull` leaves the state untouched.

use goro_runtime::Value;
use purecrypto::cipher::{ChaCha20, Poly1305};

use crate::consts::*;
use crate::error::{SodiumResult, arg, want_len};
use crate::{chacha, ct, random_bytes};

/// Size of the state string.
pub const STATE_BYTES: usize = 52;

struct State {
    k: [u8; 32],
    nonce: [u8; 12],
    /// The trailing padding, kept as given.
    pad: [u8; 8],
}

impl State {
    fn new(key: &[u8], header: &[u8]) -> State {
        let k = chacha::hchacha20(key.try_into().unwrap(), header[..16].try_into().unwrap());
        let mut nonce = [0u8; 12];
        nonce[0] = 1;
        nonce[4..].copy_from_slice(&header[16..24]);
        State {
            k,
            nonce,
            pad: [0; 8],
        }
    }

    fn from_bytes(s: &[u8]) -> State {
        State {
            k: s[..32].try_into().unwrap(),
            nonce: s[32..44].try_into().unwrap(),
            pad: s[44..52].try_into().unwrap(),
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut s = vec![0u8; STATE_BYTES];
        s[..32].copy_from_slice(&self.k);
        s[32..44].copy_from_slice(&self.nonce);
        s[44..].copy_from_slice(&self.pad);
        s
    }

    fn rekey(&mut self) {
        let mut buf = [0u8; 40];
        buf[..32].copy_from_slice(&self.k);
        buf[32..].copy_from_slice(&self.nonce[4..]);
        ChaCha20::new(&self.k).apply_keystream(&self.nonce, 0, &mut buf);
        self.k.copy_from_slice(&buf[..32]);
        self.nonce[4..].copy_from_slice(&buf[32..]);
        self.nonce[..4].copy_from_slice(&1u32.to_le_bytes());
        ct::wipe(&mut buf);
    }

    /// After a message: mix the MAC into the inonce, bump the counter and
    /// ratchet when needed.
    fn advance(&mut self, mac: &[u8; 16], tag: u8) {
        for (n, m) in self.nonce[4..].iter_mut().zip(mac) {
            *n ^= m;
        }
        let ctr = u32::from_le_bytes(self.nonce[..4].try_into().unwrap()).wrapping_add(1);
        self.nonce[..4].copy_from_slice(&ctr.to_le_bytes());
        if tag & SECRETSTREAM_TAG_REKEY != 0 || ctr == 0 {
            self.rekey();
        }
    }
}

impl Drop for State {
    fn drop(&mut self) {
        ct::wipe(&mut self.k);
    }
}

const PAD0: [u8; 16] = [0; 16];

fn mac(otk: &[u8; 32], ad: &[u8], block: &[u8; 64], c: &[u8]) -> [u8; 16] {
    let mut p = Poly1305::new(otk);
    p.update(ad);
    p.update(&PAD0[..(16 - ad.len() % 16) % 16]);
    p.update(block);
    p.update(c);
    // Observed on PHP 8.5: the padding after the ciphertext is
    // `|c| mod 16` zero bytes (not the complement to a multiple of 16).
    p.update(&PAD0[..c.len() % 16]);
    p.update(&(ad.len() as u64).to_le_bytes());
    p.update(&((64 + c.len()) as u64).to_le_bytes());
    p.finish()
}

/// The state string of a `string &$state` argument. A wrong length is
/// "must have a correct length" for `push` and "incorrect state length"
/// for `pull` and `rekey` (observed on PHP 8.5).
fn state_value<'a>(func: &str, state: &'a mut Value) -> SodiumResult<&'a mut goro_runtime::PhpStr> {
    match state {
        Value::Str(s) => {
            if s.len() != STATE_BYTES {
                if func.ends_with("_push") {
                    return Err(arg(func, 1, "state", "must have a correct length"));
                }
                return Err(crate::error::exc("incorrect state length"));
            }
            Ok(s)
        }
        _ => Err(arg(func, 1, "state", "must be a reference to a state")),
    }
}

/// `sodium_crypto_secretstream_xchacha20poly1305_keygen(): string`
pub fn sodium_crypto_secretstream_xchacha20poly1305_keygen() -> SodiumResult<Vec<u8>> {
    random_bytes(SECRETSTREAM_KEYBYTES)
}

/// `sodium_crypto_secretstream_xchacha20poly1305_init_push(string $key): array`
/// — `[state, header]`.
pub fn sodium_crypto_secretstream_xchacha20poly1305_init_push(
    key: &[u8],
) -> SodiumResult<(Vec<u8>, Vec<u8>)> {
    want_len(
        key,
        SECRETSTREAM_KEYBYTES,
        "sodium_crypto_secretstream_xchacha20poly1305_init_push",
        1,
        "key",
        "CRYPTO_SECRETSTREAM_XCHACHA20POLY1305_KEYBYTES",
    )?;
    let header = random_bytes(SECRETSTREAM_HEADERBYTES)?;
    let st = State::new(key, &header);
    Ok((st.to_bytes(), header))
}

/// `sodium_crypto_secretstream_xchacha20poly1305_init_pull(string $header, string $key): string`
pub fn sodium_crypto_secretstream_xchacha20poly1305_init_pull(
    header: &[u8],
    key: &[u8],
) -> SodiumResult<Vec<u8>> {
    const F: &str = "sodium_crypto_secretstream_xchacha20poly1305_init_pull";
    want_len(
        header,
        SECRETSTREAM_HEADERBYTES,
        F,
        1,
        "header",
        "CRYPTO_SECRETSTREAM_XCHACHA20POLY1305_HEADERBYTES",
    )?;
    want_len(
        key,
        SECRETSTREAM_KEYBYTES,
        F,
        2,
        "key",
        "CRYPTO_SECRETSTREAM_XCHACHA20POLY1305_KEYBYTES",
    )?;
    Ok(State::new(key, header).to_bytes())
}

/// `sodium_crypto_secretstream_xchacha20poly1305_push(string &$state, string $message, string $additional_data = "", int $tag = SODIUM_CRYPTO_SECRETSTREAM_XCHACHA20POLY1305_TAG_MESSAGE): string`
pub fn sodium_crypto_secretstream_xchacha20poly1305_push(
    state: &mut Value,
    message: &[u8],
    additional_data: &[u8],
    tag: i64,
) -> SodiumResult<Vec<u8>> {
    const F: &str = "sodium_crypto_secretstream_xchacha20poly1305_push";
    let s = state_value(F, state)?;
    if !(0..=255).contains(&tag) {
        return Err(arg(F, 4, "tag", "must be in the range of 0-255"));
    }
    let tag = tag as u8;
    let mut st = State::from_bytes(s.as_bytes());
    let c = ChaCha20::new(&st.k);
    let mut b0 = c.block(&st.nonce, 0);
    let mut block = [0u8; 64];
    block[0] = tag;
    c.apply_keystream(&st.nonce, 1, &mut block);
    let mut out = Vec::with_capacity(SECRETSTREAM_ABYTES + message.len());
    out.push(block[0]);
    out.extend_from_slice(message);
    c.apply_keystream(&st.nonce, 2, &mut out[1..]);
    let m = mac(
        b0[..32].try_into().unwrap(),
        additional_data,
        &block,
        &out[1..],
    );
    out.extend_from_slice(&m);
    ct::wipe(&mut b0);
    st.advance(&m, tag);
    *s = st.to_bytes().into();
    Ok(out)
}

/// `sodium_crypto_secretstream_xchacha20poly1305_pull(string &$state, string $ciphertext, string $additional_data = ""): array|false`
/// — `[message, tag]`.
pub fn sodium_crypto_secretstream_xchacha20poly1305_pull(
    state: &mut Value,
    ciphertext: &[u8],
    additional_data: &[u8],
) -> SodiumResult<Option<(Vec<u8>, i64)>> {
    const F: &str = "sodium_crypto_secretstream_xchacha20poly1305_pull";
    let s = state_value(F, state)?;
    if ciphertext.len() < SECRETSTREAM_ABYTES {
        return Ok(None);
    }
    let mut st = State::from_bytes(s.as_bytes());
    let c = ChaCha20::new(&st.k);
    let mut b0 = c.block(&st.nonce, 0);
    let mut block = [0u8; 64];
    block[0] = ciphertext[0];
    c.apply_keystream(&st.nonce, 1, &mut block);
    let tag = block[0];
    block[0] = ciphertext[0];
    let body = &ciphertext[1..ciphertext.len() - 16];
    let want = mac(b0[..32].try_into().unwrap(), additional_data, &block, body);
    ct::wipe(&mut b0);
    if !ct::eq(&want, &ciphertext[ciphertext.len() - 16..]) {
        return Ok(None);
    }
    let mut m = body.to_vec();
    c.apply_keystream(&st.nonce, 2, &mut m);
    st.advance(&want, tag);
    *s = st.to_bytes().into();
    Ok(Some((m, tag as i64)))
}

/// `sodium_crypto_secretstream_xchacha20poly1305_rekey(string &$state): void`
pub fn sodium_crypto_secretstream_xchacha20poly1305_rekey(state: &mut Value) -> SodiumResult<()> {
    let s = state_value("sodium_crypto_secretstream_xchacha20poly1305_rekey", state)?;
    let mut st = State::from_bytes(s.as_bytes());
    st.rekey();
    *s = st.to_bytes().into();
    Ok(())
}
