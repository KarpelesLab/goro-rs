//! Entry encryption.
//!
//! - Traditional PKWARE encryption (PKWARE APPNOTE.TXT section 6.1): three
//!   32-bit keys updated per plaintext byte, a 12-byte header whose last
//!   byte checks the password.
//! - WinZip AES (the published "AES Encryption Information: Encryption
//!   Specification AE-1 and AE-2"): PBKDF2-HMAC-SHA1 (1000 iterations)
//!   over a random salt gives the AES key, the HMAC key and a 2-byte
//!   password verifier; AES in CTR mode with a little-endian counter
//!   starting at 1; a 10-byte HMAC-SHA1 authentication code at the end.
//!   Entries use method 99 and an extra field 0x9901 carrying the version
//!   (AE-1 keeps the CRC, AE-2 stores 0), the strength and the real
//!   method.

use purecrypto::cipher::{Aes128, Aes192, Aes256, BlockCipher};
use purecrypto::hash::{Hmac, Sha1};
use purecrypto::rng::{OsRng, RngCore};

use crate::error::{Error, Result, er};

/// Fills `buf` with random bytes (salts, encryption headers).
pub fn random_bytes(buf: &mut [u8]) {
    OsRng.fill_bytes(buf);
}

// ------------------------------------------------------------ traditional

/// The traditional PKWARE cipher state (APPNOTE 6.1.5-6.1.7).
#[derive(Clone)]
pub struct PkwareKeys {
    k: [u32; 3],
}

impl PkwareKeys {
    /// Keys initialized from the password.
    pub fn new(password: &[u8]) -> PkwareKeys {
        let mut s = PkwareKeys {
            k: [0x1234_5678, 0x2345_6789, 0x3456_7890],
        };
        for &b in password {
            s.update(b);
        }
        s
    }

    #[inline]
    fn update(&mut self, b: u8) {
        self.k[0] = crate::crc::update_byte(self.k[0], b);
        self.k[1] = self.k[1].wrapping_add(self.k[0] & 0xff);
        self.k[1] = self.k[1].wrapping_mul(134_775_813).wrapping_add(1);
        self.k[2] = crate::crc::update_byte(self.k[2], (self.k[1] >> 24) as u8);
    }

    #[inline]
    fn stream_byte(&self) -> u8 {
        let t = (self.k[2] | 2) & 0xffff;
        ((t.wrapping_mul(t ^ 1)) >> 8) as u8
    }

    /// Decrypts in place.
    pub fn decrypt(&mut self, buf: &mut [u8]) {
        for b in buf {
            let p = *b ^ self.stream_byte();
            self.update(p);
            *b = p;
        }
    }

    /// Encrypts in place.
    pub fn encrypt(&mut self, buf: &mut [u8]) {
        for b in buf {
            let c = *b ^ self.stream_byte();
            self.update(*b);
            *b = c;
        }
    }
}

/// The check byte of the traditional header: the CRC's high byte, or the
/// DOS time's high byte when sizes and CRC follow in a data descriptor
/// (APPNOTE 6.1.6).
pub fn pkware_check_byte(crc: u32, dos_time: u16, data_descriptor: bool) -> u8 {
    if data_descriptor {
        (dos_time >> 8) as u8
    } else {
        (crc >> 24) as u8
    }
}

/// Decrypts traditional-encrypted data (12-byte header first).
pub fn pkware_decrypt(data: &[u8], password: &[u8], check: u8) -> Result<Vec<u8>> {
    if data.len() < 12 {
        return Err(Error::new(er::EOF));
    }
    let mut keys = PkwareKeys::new(password);
    let mut head = [0u8; 12];
    head.copy_from_slice(&data[..12]);
    keys.decrypt(&mut head);
    if head[11] != check {
        return Err(Error::new(er::WRONGPASSWD));
    }
    let mut out = data[12..].to_vec();
    keys.decrypt(&mut out);
    Ok(out)
}

/// Encrypts with the traditional cipher: a random header ending with
/// `check`, then the data.
pub fn pkware_encrypt(data: &[u8], password: &[u8], check: u8) -> Vec<u8> {
    let mut keys = PkwareKeys::new(password);
    let mut out = Vec::with_capacity(data.len() + 12);
    let mut head = [0u8; 12];
    random_bytes(&mut head[..11]);
    head[11] = check;
    out.extend_from_slice(&head);
    out.extend_from_slice(data);
    keys.encrypt(&mut out);
    out
}

// ------------------------------------------------------------ WinZip AES

/// AES key size, as the strength byte of extra field 0x9901 numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AesStrength {
    Aes128 = 1,
    Aes192 = 2,
    Aes256 = 3,
}

impl AesStrength {
    pub fn from_byte(b: u8) -> Option<AesStrength> {
        match b {
            1 => Some(AesStrength::Aes128),
            2 => Some(AesStrength::Aes192),
            3 => Some(AesStrength::Aes256),
            _ => None,
        }
    }

    pub fn key_len(self) -> usize {
        match self {
            AesStrength::Aes128 => 16,
            AesStrength::Aes192 => 24,
            AesStrength::Aes256 => 32,
        }
    }

    pub fn salt_len(self) -> usize {
        self.key_len() / 2
    }

    /// Bytes added to the data: salt, verifier and authentication code.
    pub fn overhead(self) -> usize {
        self.salt_len() + 2 + 10
    }
}

enum AnyAes {
    A128(Aes128),
    A192(Aes192),
    A256(Aes256),
}

impl AnyAes {
    fn new(key: &[u8]) -> AnyAes {
        match key.len() {
            16 => AnyAes::A128(Aes128::new(key.try_into().expect("16 bytes"))),
            24 => AnyAes::A192(Aes192::new(key.try_into().expect("24 bytes"))),
            _ => AnyAes::A256(Aes256::new(key.try_into().expect("32 bytes"))),
        }
    }

    fn encrypt_blocks(&self, b: &mut [u8]) {
        match self {
            AnyAes::A128(c) => c.encrypt_blocks(b),
            AnyAes::A192(c) => c.encrypt_blocks(b),
            AnyAes::A256(c) => c.encrypt_blocks(b),
        }
    }
}

/// CTR with the little-endian counter starting at 1 (WinZip spec).
fn ctr_xor(aes: &AnyAes, data: &mut [u8]) {
    let mut counter: u128 = 1;
    const CHUNK: usize = 64 * 16;
    let mut ks = vec![0u8; CHUNK];
    for chunk in data.chunks_mut(CHUNK) {
        let nblocks = chunk.len().div_ceil(16);
        for i in 0..nblocks {
            ks[i * 16..i * 16 + 16].copy_from_slice(&counter.to_le_bytes());
            counter = counter.wrapping_add(1);
        }
        aes.encrypt_blocks(&mut ks[..nblocks * 16]);
        for (d, k) in chunk.iter_mut().zip(ks.iter()) {
            *d ^= *k;
        }
    }
}

/// The derived keys: AES key, HMAC key, password verifier.
fn aes_keys(password: &[u8], salt: &[u8], strength: AesStrength) -> (Vec<u8>, Vec<u8>, [u8; 2]) {
    let kl = strength.key_len();
    let mut out = vec![0u8; kl * 2 + 2];
    purecrypto::kdf::pbkdf2::<Sha1>(password, salt, 1000, &mut out);
    let pv = [out[kl * 2], out[kl * 2 + 1]];
    (out[..kl].to_vec(), out[kl..kl * 2].to_vec(), pv)
}

fn auth_code(hmac_key: &[u8], ciphertext: &[u8]) -> [u8; 10] {
    let mut h = Hmac::<Sha1>::new(hmac_key);
    h.update(ciphertext);
    let tag = h.finalize();
    let mut out = [0u8; 10];
    out.copy_from_slice(&tag.as_ref()[..10]);
    out
}

/// Decrypts WinZip AES data (salt, verifier, ciphertext, code).
/// A wrong verifier is `ER_WRONGPASSWD`, a bad authentication code
/// `ER_CRC`.
pub fn aes_decrypt(data: &[u8], password: &[u8], strength: AesStrength) -> Result<Vec<u8>> {
    let sl = strength.salt_len();
    if data.len() < strength.overhead() {
        return Err(Error::new(er::EOF));
    }
    let salt = &data[..sl];
    let (key, hkey, pv) = aes_keys(password, salt, strength);
    if data[sl..sl + 2] != pv {
        return Err(Error::new(er::WRONGPASSWD));
    }
    let body = &data[sl + 2..data.len() - 10];
    let code = &data[data.len() - 10..];
    if auth_code(&hkey, body) != code {
        return Err(Error::new(er::CRC));
    }
    let mut out = body.to_vec();
    ctr_xor(&AnyAes::new(&key), &mut out);
    Ok(out)
}

/// Encrypts with WinZip AES under a random salt.
pub fn aes_encrypt(data: &[u8], password: &[u8], strength: AesStrength) -> Vec<u8> {
    let mut salt = vec![0u8; strength.salt_len()];
    random_bytes(&mut salt);
    aes_encrypt_with_salt(data, password, strength, &salt)
}

/// Encrypts with WinZip AES under `salt` (of the strength's salt length).
pub fn aes_encrypt_with_salt(data: &[u8], password: &[u8], strength: AesStrength, salt: &[u8]) -> Vec<u8> {
    let (key, hkey, pv) = aes_keys(password, salt, strength);
    let mut out = Vec::with_capacity(data.len() + strength.overhead());
    out.extend_from_slice(salt);
    out.extend_from_slice(&pv);
    let start = out.len();
    out.extend_from_slice(data);
    ctr_xor(&AnyAes::new(&key), &mut out[start..]);
    let code = auth_code(&hkey, &out[start..]);
    out.extend_from_slice(&code);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkware_roundtrip() {
        let enc = pkware_encrypt(b"hello world", b"secret", 0x5a);
        assert_eq!(pkware_decrypt(&enc, b"secret", 0x5a).unwrap(), b"hello world");
        let e = pkware_decrypt(&enc, b"wrong", 0x5a);
        // one in 256 wrong passwords passes the check byte
        if let Err(e) = e {
            assert_eq!(e.code, er::WRONGPASSWD);
        }
    }

    #[test]
    fn aes_roundtrip() {
        for s in [AesStrength::Aes128, AesStrength::Aes192, AesStrength::Aes256] {
            let data: Vec<u8> = (0..1000u32).map(|i| (i * 7) as u8).collect();
            let enc = aes_encrypt(&data, b"pw", s);
            assert_eq!(enc.len(), data.len() + s.overhead());
            assert_eq!(aes_decrypt(&enc, b"pw", s).unwrap(), data);
            assert!(aes_decrypt(&enc, b"px", s).is_err());
        }
    }
}
