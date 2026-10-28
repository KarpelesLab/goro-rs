//! `crypto_aead_*` (libsodium documentation, "Authenticated Encryption with
//! Additional Data"): ChaCha20-Poly1305 in its original and IETF (RFC 8439)
//! forms, XChaCha20-Poly1305 (draft-irtf-cfrg-xchacha), AES-256-GCM and
//! AEGIS-128L / AEGIS-256 (draft-irtf-cfrg-aegis-aead, 256-bit tags as in
//! libsodium). Ciphertexts are `encrypted message ‖ tag`.
//!
//! The original ChaCha20-Poly1305 construction (as libsodium documents it,
//! from draft-agl-tls-chacha20poly1305): the Poly1305 key is the first 32
//! bytes of ChaCha20 block 0, the message is encrypted from block 1, and
//! the tag covers `ad ‖ le64(|ad|) ‖ c ‖ le64(|c|)` without padding.

use purecrypto::cipher::{
    Aegis128L, Aegis256, Aes256, Aes256Gcm, ChaCha20, ChaCha20Poly1305, Poly1305, XChaCha20Poly1305,
};

use crate::consts::*;
use crate::error::{SodiumResult, want_len};
use crate::{chacha, random_bytes};

/// Which AEAD.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    ChaCha,
    ChaChaIetf,
    XChaChaIetf,
    Aes256Gcm,
    Aegis128L,
    Aegis256,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::ChaCha => "chacha20poly1305",
            Kind::ChaChaIetf => "chacha20poly1305_ietf",
            Kind::XChaChaIetf => "xchacha20poly1305_ietf",
            Kind::Aes256Gcm => "aes256gcm",
            Kind::Aegis128L => "aegis128l",
            Kind::Aegis256 => "aegis256",
        }
    }

    /// The constant prefix in messages, e.g. `CRYPTO_AEAD_AES256GCM`.
    fn constant(self) -> &'static str {
        match self {
            Kind::ChaCha => "CRYPTO_AEAD_CHACHA20POLY1305",
            Kind::ChaChaIetf => "CRYPTO_AEAD_CHACHA20POLY1305_IETF",
            Kind::XChaChaIetf => "CRYPTO_AEAD_XCHACHA20POLY1305_IETF",
            Kind::Aes256Gcm => "CRYPTO_AEAD_AES256GCM",
            Kind::Aegis128L => "CRYPTO_AEAD_AEGIS128L",
            Kind::Aegis256 => "CRYPTO_AEAD_AEGIS256",
        }
    }

    fn sizes(self) -> (usize, usize, usize) {
        match self {
            Kind::ChaCha => (
                AEAD_CHACHA20POLY1305_KEYBYTES,
                AEAD_CHACHA20POLY1305_NPUBBYTES,
                AEAD_CHACHA20POLY1305_ABYTES,
            ),
            Kind::ChaChaIetf => (
                AEAD_CHACHA20POLY1305_IETF_KEYBYTES,
                AEAD_CHACHA20POLY1305_IETF_NPUBBYTES,
                AEAD_CHACHA20POLY1305_IETF_ABYTES,
            ),
            Kind::XChaChaIetf => (
                AEAD_XCHACHA20POLY1305_IETF_KEYBYTES,
                AEAD_XCHACHA20POLY1305_IETF_NPUBBYTES,
                AEAD_XCHACHA20POLY1305_IETF_ABYTES,
            ),
            Kind::Aes256Gcm => (
                AEAD_AES256GCM_KEYBYTES,
                AEAD_AES256GCM_NPUBBYTES,
                AEAD_AES256GCM_ABYTES,
            ),
            Kind::Aegis128L => (
                AEAD_AEGIS128L_KEYBYTES,
                AEAD_AEGIS128L_NPUBBYTES,
                AEAD_AEGIS128L_ABYTES,
            ),
            Kind::Aegis256 => (
                AEAD_AEGIS256_KEYBYTES,
                AEAD_AEGIS256_NPUBBYTES,
                AEAD_AEGIS256_ABYTES,
            ),
        }
    }

    fn check(self, func: &str, nonce: &[u8], key: &[u8]) -> SodiumResult<()> {
        let (kl, nl, _) = self.sizes();
        let c = self.constant();
        want_len(nonce, nl, func, 3, "nonce", &format!("{c}_NPUBBYTES"))?;
        want_len(key, kl, func, 4, "key", &format!("{c}_KEYBYTES"))?;
        Ok(())
    }
}

fn chacha_orig_tag(otk: &[u8; 32], ad: &[u8], c: &[u8]) -> [u8; 16] {
    let mut p = Poly1305::new(otk);
    p.update(ad);
    p.update(&(ad.len() as u64).to_le_bytes());
    p.update(c);
    p.update(&(c.len() as u64).to_le_bytes());
    p.finish()
}

/// Encrypts `buf` in place and returns the tag.
fn seal(kind: Kind, key: &[u8], nonce: &[u8], ad: &[u8], buf: &mut [u8]) -> Vec<u8> {
    match kind {
        Kind::ChaCha => {
            let c = ChaCha20::new(key.try_into().unwrap());
            let n: &[u8; 8] = nonce.try_into().unwrap();
            let mut b0 = chacha::block0(&c, n);
            chacha::xor_ic(&c, n, 1, buf);
            let tag = chacha_orig_tag(b0[..32].try_into().unwrap(), ad, buf);
            crate::ct::wipe(&mut b0);
            tag.to_vec()
        }
        Kind::ChaChaIetf => ChaCha20Poly1305::new(key.try_into().unwrap())
            .encrypt(nonce.try_into().unwrap(), ad, buf)
            .to_vec(),
        Kind::XChaChaIetf => XChaCha20Poly1305::new(key.try_into().unwrap())
            .encrypt(nonce.try_into().unwrap(), ad, buf)
            .to_vec(),
        Kind::Aes256Gcm => Aes256Gcm::new(Aes256::new(key.try_into().unwrap()))
            .encrypt(nonce, ad, buf)
            .to_vec(),
        Kind::Aegis128L => Aegis128L::new(key.try_into().unwrap())
            .encrypt_tag256(nonce.try_into().unwrap(), ad, buf)
            .to_vec(),
        Kind::Aegis256 => Aegis256::new(key.try_into().unwrap())
            .encrypt_tag256(nonce.try_into().unwrap(), ad, buf)
            .to_vec(),
    }
}

/// Verifies `tag` and decrypts `buf` in place.
fn open(kind: Kind, key: &[u8], nonce: &[u8], ad: &[u8], buf: &mut [u8], tag: &[u8]) -> bool {
    match kind {
        Kind::ChaCha => {
            let c = ChaCha20::new(key.try_into().unwrap());
            let n: &[u8; 8] = nonce.try_into().unwrap();
            let mut b0 = chacha::block0(&c, n);
            let want = chacha_orig_tag(b0[..32].try_into().unwrap(), ad, buf);
            crate::ct::wipe(&mut b0);
            if !crate::ct::eq(&want, tag) {
                return false;
            }
            chacha::xor_ic(&c, n, 1, buf);
            true
        }
        Kind::ChaChaIetf => ChaCha20Poly1305::new(key.try_into().unwrap())
            .decrypt(nonce.try_into().unwrap(), ad, buf, tag.try_into().unwrap())
            .is_ok(),
        Kind::XChaChaIetf => XChaCha20Poly1305::new(key.try_into().unwrap())
            .decrypt(nonce.try_into().unwrap(), ad, buf, tag.try_into().unwrap())
            .is_ok(),
        Kind::Aes256Gcm => Aes256Gcm::new(Aes256::new(key.try_into().unwrap()))
            .decrypt(nonce, ad, buf, tag.try_into().unwrap())
            .is_ok(),
        Kind::Aegis128L => Aegis128L::new(key.try_into().unwrap())
            .decrypt_tag256(nonce.try_into().unwrap(), ad, buf, tag.try_into().unwrap())
            .is_ok(),
        Kind::Aegis256 => Aegis256::new(key.try_into().unwrap())
            .decrypt_tag256(nonce.try_into().unwrap(), ad, buf, tag.try_into().unwrap())
            .is_ok(),
    }
}

fn encrypt(
    kind: Kind,
    message: &[u8],
    ad: &[u8],
    nonce: &[u8],
    key: &[u8],
) -> SodiumResult<Vec<u8>> {
    let func = format!("sodium_crypto_aead_{}_encrypt", kind.name());
    kind.check(&func, nonce, key)?;
    let (_, _, abytes) = kind.sizes();
    let mut out = Vec::with_capacity(message.len() + abytes);
    out.extend_from_slice(message);
    let tag = seal(kind, key, nonce, ad, &mut out);
    out.extend_from_slice(&tag);
    Ok(out)
}

fn decrypt(
    kind: Kind,
    ciphertext: &[u8],
    ad: &[u8],
    nonce: &[u8],
    key: &[u8],
) -> SodiumResult<Option<Vec<u8>>> {
    let func = format!("sodium_crypto_aead_{}_decrypt", kind.name());
    kind.check(&func, nonce, key)?;
    let (_, _, abytes) = kind.sizes();
    if ciphertext.len() < abytes {
        return Ok(None);
    }
    let (c, tag) = ciphertext.split_at(ciphertext.len() - abytes);
    let mut m = c.to_vec();
    if open(kind, key, nonce, ad, &mut m, tag) {
        Ok(Some(m))
    } else {
        crate::ct::wipe(&mut m);
        Ok(None)
    }
}

macro_rules! aead_fns {
    ($kind:expr, $enc:ident, $dec:ident, $keygen:ident, $keybytes:expr) => {
        #[doc = concat!("`", stringify!($enc), "(string $message, string $additional_data, string $nonce, string $key): string`")]
        pub fn $enc(message: &[u8], additional_data: &[u8], nonce: &[u8], key: &[u8]) -> SodiumResult<Vec<u8>> {
            encrypt($kind, message, additional_data, nonce, key)
        }

        #[doc = concat!("`", stringify!($dec), "(string $ciphertext, string $additional_data, string $nonce, string $key): string|false`")]
        pub fn $dec(
            ciphertext: &[u8],
            additional_data: &[u8],
            nonce: &[u8],
            key: &[u8],
        ) -> SodiumResult<Option<Vec<u8>>> {
            decrypt($kind, ciphertext, additional_data, nonce, key)
        }

        #[doc = concat!("`", stringify!($keygen), "(): string`")]
        pub fn $keygen() -> SodiumResult<Vec<u8>> {
            random_bytes($keybytes)
        }
    };
}

aead_fns!(
    Kind::ChaCha,
    sodium_crypto_aead_chacha20poly1305_encrypt,
    sodium_crypto_aead_chacha20poly1305_decrypt,
    sodium_crypto_aead_chacha20poly1305_keygen,
    AEAD_CHACHA20POLY1305_KEYBYTES
);
aead_fns!(
    Kind::ChaChaIetf,
    sodium_crypto_aead_chacha20poly1305_ietf_encrypt,
    sodium_crypto_aead_chacha20poly1305_ietf_decrypt,
    sodium_crypto_aead_chacha20poly1305_ietf_keygen,
    AEAD_CHACHA20POLY1305_IETF_KEYBYTES
);
aead_fns!(
    Kind::XChaChaIetf,
    sodium_crypto_aead_xchacha20poly1305_ietf_encrypt,
    sodium_crypto_aead_xchacha20poly1305_ietf_decrypt,
    sodium_crypto_aead_xchacha20poly1305_ietf_keygen,
    AEAD_XCHACHA20POLY1305_IETF_KEYBYTES
);
aead_fns!(
    Kind::Aes256Gcm,
    sodium_crypto_aead_aes256gcm_encrypt,
    sodium_crypto_aead_aes256gcm_decrypt,
    sodium_crypto_aead_aes256gcm_keygen,
    AEAD_AES256GCM_KEYBYTES
);
aead_fns!(
    Kind::Aegis128L,
    sodium_crypto_aead_aegis128l_encrypt,
    sodium_crypto_aead_aegis128l_decrypt,
    sodium_crypto_aead_aegis128l_keygen,
    AEAD_AEGIS128L_KEYBYTES
);
aead_fns!(
    Kind::Aegis256,
    sodium_crypto_aead_aegis256_encrypt,
    sodium_crypto_aead_aegis256_decrypt,
    sodium_crypto_aead_aegis256_keygen,
    AEAD_AEGIS256_KEYBYTES
);

/// `sodium_crypto_aead_aes256gcm_is_available(): bool` — always true:
/// purecrypto's AES-GCM uses AES-NI/PCLMULQDQ when present and a
/// constant-time bitsliced implementation otherwise (libsodium only offers
/// AES-256-GCM on CPUs with the hardware instructions).
pub fn sodium_crypto_aead_aes256gcm_is_available() -> bool {
    true
}
