/// Block cipher modes: ECB, CBC, CTR with PKCS7 padding support.

use crate::aes::{AesKey, AesKeySize};

const BLOCK_SIZE: usize = 16;

/// Cipher mode
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CipherMode {
    Ecb,
    Cbc,
    Ctr,
}

impl CipherMode {
    /// IV length required for this mode
    pub fn iv_length(self) -> usize {
        match self {
            CipherMode::Ecb => 0,
            CipherMode::Cbc => BLOCK_SIZE,
            CipherMode::Ctr => BLOCK_SIZE,
        }
    }
}

/// Parsed cipher method string
pub struct CipherMethod {
    pub key_size: AesKeySize,
    pub mode: CipherMode,
}

impl CipherMethod {
    /// Parse a cipher method string like "aes-128-cbc", "aes-256-ecb", etc.
    pub fn parse(method: &str) -> Option<CipherMethod> {
        let lower = method.to_ascii_lowercase();
        let parts: Vec<&str> = lower.split('-').collect();
        if parts.len() != 3 || parts[0] != "aes" {
            return None;
        }

        let key_size = match parts[1] {
            "128" => AesKeySize::Aes128,
            "192" => AesKeySize::Aes192,
            "256" => AesKeySize::Aes256,
            _ => return None,
        };

        let mode = match parts[2] {
            "ecb" => CipherMode::Ecb,
            "cbc" => CipherMode::Cbc,
            "ctr" => CipherMode::Ctr,
            _ => return None,
        };

        Some(CipherMethod { key_size, mode })
    }

    /// Return the IV length for this cipher method
    pub fn iv_length(&self) -> usize {
        self.mode.iv_length()
    }

    /// Return the key length for this cipher method
    pub fn key_length(&self) -> usize {
        self.key_size.key_len()
    }
}

/// Apply PKCS7 padding to data
fn pkcs7_pad(data: &[u8]) -> Vec<u8> {
    let pad_len = BLOCK_SIZE - (data.len() % BLOCK_SIZE);
    let mut padded = data.to_vec();
    padded.extend(std::iter::repeat(pad_len as u8).take(pad_len));
    padded
}

/// Remove PKCS7 padding. Returns None if padding is invalid.
fn pkcs7_unpad(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() || data.len() % BLOCK_SIZE != 0 {
        return None;
    }
    let pad_byte = *data.last().unwrap();
    if pad_byte == 0 || pad_byte as usize > BLOCK_SIZE {
        return None;
    }
    let pad_len = pad_byte as usize;
    if data.len() < pad_len {
        return None;
    }
    // Verify all padding bytes are correct
    for &b in &data[data.len() - pad_len..] {
        if b != pad_byte {
            return None;
        }
    }
    Some(data[..data.len() - pad_len].to_vec())
}

/// Prepare key: pad with zeros or truncate to expected length
fn prepare_key(key: &[u8], expected_len: usize) -> Vec<u8> {
    let mut k = vec![0u8; expected_len];
    let copy_len = key.len().min(expected_len);
    k[..copy_len].copy_from_slice(&key[..copy_len]);
    k
}

/// Prepare IV: pad with zeros or truncate to expected length
fn prepare_iv(iv: &[u8], expected_len: usize) -> Vec<u8> {
    if expected_len == 0 {
        return Vec::new();
    }
    let mut v = vec![0u8; expected_len];
    let copy_len = iv.len().min(expected_len);
    v[..copy_len].copy_from_slice(&iv[..copy_len]);
    v
}

/// Increment a 16-byte counter (big-endian increment)
fn increment_counter(ctr: &mut [u8; BLOCK_SIZE]) {
    for i in (0..BLOCK_SIZE).rev() {
        ctr[i] = ctr[i].wrapping_add(1);
        if ctr[i] != 0 {
            break;
        }
    }
}

/// Encrypt data using the specified cipher method.
/// `zero_padding`: if true, do not apply PKCS7 padding (OPENSSL_ZERO_PADDING)
pub fn encrypt(
    method: &CipherMethod,
    data: &[u8],
    key: &[u8],
    iv: &[u8],
    zero_padding: bool,
) -> Result<Vec<u8>, &'static str> {
    let prepared_key = prepare_key(key, method.key_length());
    let aes_key = AesKey::new(&prepared_key, method.key_size);

    match method.mode {
        CipherMode::Ecb => encrypt_ecb(&aes_key, data, zero_padding),
        CipherMode::Cbc => {
            let prepared_iv = prepare_iv(iv, BLOCK_SIZE);
            encrypt_cbc(&aes_key, data, &prepared_iv, zero_padding)
        }
        CipherMode::Ctr => {
            let prepared_iv = prepare_iv(iv, BLOCK_SIZE);
            encrypt_ctr(&aes_key, data, &prepared_iv)
        }
    }
}

/// Decrypt data using the specified cipher method.
/// `zero_padding`: if true, do not remove PKCS7 padding (OPENSSL_ZERO_PADDING)
pub fn decrypt(
    method: &CipherMethod,
    data: &[u8],
    key: &[u8],
    iv: &[u8],
    zero_padding: bool,
) -> Result<Vec<u8>, &'static str> {
    let prepared_key = prepare_key(key, method.key_length());
    let aes_key = AesKey::new(&prepared_key, method.key_size);

    match method.mode {
        CipherMode::Ecb => decrypt_ecb(&aes_key, data, zero_padding),
        CipherMode::Cbc => {
            let prepared_iv = prepare_iv(iv, BLOCK_SIZE);
            decrypt_cbc(&aes_key, data, &prepared_iv, zero_padding)
        }
        CipherMode::Ctr => {
            let prepared_iv = prepare_iv(iv, BLOCK_SIZE);
            // CTR decryption is the same operation as encryption
            encrypt_ctr(&aes_key, data, &prepared_iv)
        }
    }
}

/// ECB encryption
fn encrypt_ecb(aes_key: &AesKey, data: &[u8], zero_padding: bool) -> Result<Vec<u8>, &'static str> {
    let padded = if zero_padding {
        if data.len() % BLOCK_SIZE != 0 {
            return Err("data length is not a multiple of block size");
        }
        data.to_vec()
    } else {
        pkcs7_pad(data)
    };

    let mut output = Vec::with_capacity(padded.len());
    for chunk in padded.chunks(BLOCK_SIZE) {
        let mut block = [0u8; BLOCK_SIZE];
        block.copy_from_slice(chunk);
        let encrypted = aes_key.encrypt_block(&block);
        output.extend_from_slice(&encrypted);
    }
    Ok(output)
}

/// ECB decryption
fn decrypt_ecb(aes_key: &AesKey, data: &[u8], zero_padding: bool) -> Result<Vec<u8>, &'static str> {
    if data.is_empty() || data.len() % BLOCK_SIZE != 0 {
        return Err("data length is not a multiple of block size");
    }

    let mut output = Vec::with_capacity(data.len());
    for chunk in data.chunks(BLOCK_SIZE) {
        let mut block = [0u8; BLOCK_SIZE];
        block.copy_from_slice(chunk);
        let decrypted = aes_key.decrypt_block(&block);
        output.extend_from_slice(&decrypted);
    }

    if zero_padding {
        Ok(output)
    } else {
        pkcs7_unpad(&output).ok_or("invalid PKCS7 padding")
    }
}

/// CBC encryption
fn encrypt_cbc(
    aes_key: &AesKey,
    data: &[u8],
    iv: &[u8],
    zero_padding: bool,
) -> Result<Vec<u8>, &'static str> {
    let padded = if zero_padding {
        if data.len() % BLOCK_SIZE != 0 {
            return Err("data length is not a multiple of block size");
        }
        data.to_vec()
    } else {
        pkcs7_pad(data)
    };

    let mut output = Vec::with_capacity(padded.len());
    let mut prev = [0u8; BLOCK_SIZE];
    prev.copy_from_slice(&iv[..BLOCK_SIZE]);

    for chunk in padded.chunks(BLOCK_SIZE) {
        let mut block = [0u8; BLOCK_SIZE];
        block.copy_from_slice(chunk);
        // XOR with previous ciphertext (or IV for first block)
        for i in 0..BLOCK_SIZE {
            block[i] ^= prev[i];
        }
        let encrypted = aes_key.encrypt_block(&block);
        prev = encrypted;
        output.extend_from_slice(&encrypted);
    }
    Ok(output)
}

/// CBC decryption
fn decrypt_cbc(
    aes_key: &AesKey,
    data: &[u8],
    iv: &[u8],
    zero_padding: bool,
) -> Result<Vec<u8>, &'static str> {
    if data.is_empty() || data.len() % BLOCK_SIZE != 0 {
        return Err("data length is not a multiple of block size");
    }

    let mut output = Vec::with_capacity(data.len());
    let mut prev = [0u8; BLOCK_SIZE];
    prev.copy_from_slice(&iv[..BLOCK_SIZE]);

    for chunk in data.chunks(BLOCK_SIZE) {
        let mut block = [0u8; BLOCK_SIZE];
        block.copy_from_slice(chunk);
        let decrypted = aes_key.decrypt_block(&block);
        let mut plain_block = [0u8; BLOCK_SIZE];
        for i in 0..BLOCK_SIZE {
            plain_block[i] = decrypted[i] ^ prev[i];
        }
        prev.copy_from_slice(chunk);
        output.extend_from_slice(&plain_block);
    }

    if zero_padding {
        Ok(output)
    } else {
        pkcs7_unpad(&output).ok_or("invalid PKCS7 padding")
    }
}

/// CTR encryption/decryption (same operation)
fn encrypt_ctr(
    aes_key: &AesKey,
    data: &[u8],
    iv: &[u8],
) -> Result<Vec<u8>, &'static str> {
    let mut counter = [0u8; BLOCK_SIZE];
    counter.copy_from_slice(&iv[..BLOCK_SIZE]);

    let mut output = Vec::with_capacity(data.len());

    for chunk in data.chunks(BLOCK_SIZE) {
        let keystream = aes_key.encrypt_block(&counter);
        for (i, &byte) in chunk.iter().enumerate() {
            output.push(byte ^ keystream[i]);
        }
        increment_counter(&mut counter);
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkcs7_pad_unpad() {
        let data = b"Hello";
        let padded = pkcs7_pad(data);
        assert_eq!(padded.len(), 16);
        assert_eq!(padded[5..], [11u8; 11]);
        let unpadded = pkcs7_unpad(&padded).unwrap();
        assert_eq!(unpadded, data);
    }

    #[test]
    fn test_pkcs7_full_block() {
        let data = [0u8; 16];
        let padded = pkcs7_pad(&data);
        assert_eq!(padded.len(), 32);
        assert_eq!(&padded[16..], &[16u8; 16]);
    }

    #[test]
    fn test_ecb_roundtrip() {
        let key = [0x42u8; 16];
        let method = CipherMethod::parse("aes-128-ecb").unwrap();
        let plaintext = b"Hello, World!!!";

        let encrypted = encrypt(&method, plaintext, &key, &[], false).unwrap();
        let decrypted = decrypt(&method, &encrypted, &key, &[], false).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_cbc_roundtrip() {
        let key = [0x42u8; 32];
        let iv = [0x13u8; 16];
        let method = CipherMethod::parse("aes-256-cbc").unwrap();
        let plaintext = b"This is a test of AES-256-CBC mode encryption.";

        let encrypted = encrypt(&method, plaintext, &key, &iv, false).unwrap();
        let decrypted = decrypt(&method, &encrypted, &key, &iv, false).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_ctr_roundtrip() {
        let key = [0x55u8; 16];
        let iv = [0xAA; 16];
        let method = CipherMethod::parse("aes-128-ctr").unwrap();
        let plaintext = b"CTR mode does not need padding for arbitrary lengths!";

        let encrypted = encrypt(&method, plaintext, &key, &iv, false).unwrap();
        let decrypted = decrypt(&method, &encrypted, &key, &iv, false).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_zero_padding_mode() {
        let key = [0x42u8; 16];
        let method = CipherMethod::parse("aes-128-ecb").unwrap();
        // Exactly one block
        let plaintext = [0x41u8; 16];

        let encrypted = encrypt(&method, &plaintext, &key, &[], true).unwrap();
        assert_eq!(encrypted.len(), 16); // No padding added
        let decrypted = decrypt(&method, &encrypted, &key, &[], true).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_cipher_method_parse() {
        assert!(CipherMethod::parse("aes-128-cbc").is_some());
        assert!(CipherMethod::parse("aes-192-cbc").is_some());
        assert!(CipherMethod::parse("aes-256-cbc").is_some());
        assert!(CipherMethod::parse("aes-128-ecb").is_some());
        assert!(CipherMethod::parse("aes-256-ctr").is_some());
        assert!(CipherMethod::parse("AES-128-CBC").is_some());
        assert!(CipherMethod::parse("des-cbc").is_none());
        assert!(CipherMethod::parse("aes-512-cbc").is_none());
        assert!(CipherMethod::parse("blowfish").is_none());
    }
}
