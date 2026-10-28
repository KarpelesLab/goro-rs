//! Password-based encryption for keys: PKCS #5 v2.1 PBES2 (RFC 8018 §6.2)
//! with PBKDF2, PKCS #5 PBES1 and the PKCS #12 PBE schemes (RFC 7292
//! appendix B/C), and the legacy PEM encryption headers (RFC 1421
//! `Proc-Type`/`DEK-Info`, keys from OpenSSL's documented EVP_BytesToKey
//! scheme: MD5 over passphrase and the first 8 IV bytes, one round).

use purecrypto::cipher::{Aes128, Aes192, Aes256, Cbc, Cbc64, Des, TdesEde2, TdesEde3};

use crate::der::{self, tag};
use crate::digest::Md;
use purecrypto::hash::HashAlgorithm;

/// A CBC block cipher usable for key encryption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CbcCipher {
    Aes128,
    Aes192,
    Aes256,
    DesEde3,
    DesEde,
    Des,
}

impl CbcCipher {
    pub fn by_name(n: &str) -> Option<CbcCipher> {
        Some(match n.to_ascii_lowercase().as_str() {
            "aes-128-cbc" | "aes128" => CbcCipher::Aes128,
            "aes-192-cbc" | "aes192" => CbcCipher::Aes192,
            "aes-256-cbc" | "aes256" => CbcCipher::Aes256,
            "des-ede3-cbc" | "des3" => CbcCipher::DesEde3,
            "des-ede-cbc" => CbcCipher::DesEde,
            "des-cbc" | "des" => CbcCipher::Des,
            _ => return None,
        })
    }
    pub fn by_oid(o: &str) -> Option<CbcCipher> {
        Some(match o {
            "2.16.840.1.101.3.4.1.2" => CbcCipher::Aes128,
            "2.16.840.1.101.3.4.1.22" => CbcCipher::Aes192,
            "2.16.840.1.101.3.4.1.42" => CbcCipher::Aes256,
            "1.2.840.113549.3.7" => CbcCipher::DesEde3,
            "1.3.14.3.2.7" => CbcCipher::Des,
            _ => return None,
        })
    }
    pub fn oid(self) -> &'static str {
        match self {
            CbcCipher::Aes128 => "2.16.840.1.101.3.4.1.2",
            CbcCipher::Aes192 => "2.16.840.1.101.3.4.1.22",
            CbcCipher::Aes256 => "2.16.840.1.101.3.4.1.42",
            CbcCipher::DesEde3 => "1.2.840.113549.3.7",
            CbcCipher::DesEde => "1.2.840.113549.3.7",
            CbcCipher::Des => "1.3.14.3.2.7",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            CbcCipher::Aes128 => "AES-128-CBC",
            CbcCipher::Aes192 => "AES-192-CBC",
            CbcCipher::Aes256 => "AES-256-CBC",
            CbcCipher::DesEde3 => "DES-EDE3-CBC",
            CbcCipher::DesEde => "DES-EDE-CBC",
            CbcCipher::Des => "DES-CBC",
        }
    }
    pub fn key_len(self) -> usize {
        match self {
            CbcCipher::Aes128 => 16,
            CbcCipher::Aes192 => 24,
            CbcCipher::Aes256 => 32,
            CbcCipher::DesEde3 => 24,
            CbcCipher::DesEde => 16,
            CbcCipher::Des => 8,
        }
    }
    pub fn block(self) -> usize {
        match self {
            CbcCipher::Aes128 | CbcCipher::Aes192 | CbcCipher::Aes256 => 16,
            _ => 8,
        }
    }

    fn run(self, key: &[u8], iv: &[u8], data: &mut [u8], enc: bool) -> Option<()> {
        macro_rules! c16 {
            ($t:ty) => {{
                let k = key.try_into().ok()?;
                let iv: &[u8; 16] = iv.try_into().ok()?;
                let mut c = Cbc::new(<$t>::new(k), iv);
                if enc {
                    c.encrypt(data).ok()?
                } else {
                    c.decrypt(data).ok()?
                }
            }};
        }
        macro_rules! c8 {
            ($t:ty) => {{
                let k = key.try_into().ok()?;
                let iv: &[u8; 8] = iv.try_into().ok()?;
                let mut c = Cbc64::new(<$t>::new(k), iv);
                if enc {
                    c.encrypt(data).ok()?
                } else {
                    c.decrypt(data).ok()?
                }
            }};
        }
        match self {
            CbcCipher::Aes128 => c16!(Aes128),
            CbcCipher::Aes192 => c16!(Aes192),
            CbcCipher::Aes256 => c16!(Aes256),
            CbcCipher::DesEde3 => c8!(TdesEde3),
            CbcCipher::DesEde => c8!(TdesEde2),
            CbcCipher::Des => c8!(Des),
        }
        Some(())
    }

    /// CBC with PKCS #7 padding.
    pub fn encrypt(self, key: &[u8], iv: &[u8], data: &[u8]) -> Option<Vec<u8>> {
        let bs = self.block();
        let pad = bs - data.len() % bs;
        let mut buf = data.to_vec();
        buf.extend(std::iter::repeat_n(pad as u8, pad));
        self.run(key, iv, &mut buf, true)?;
        Some(buf)
    }

    /// CBC decryption and PKCS #7 padding removal; the padding check runs
    /// in constant time over the last block.
    pub fn decrypt(self, key: &[u8], iv: &[u8], data: &[u8]) -> Option<Vec<u8>> {
        let bs = self.block();
        if data.is_empty() || !data.len().is_multiple_of(bs) {
            return None;
        }
        let mut buf = data.to_vec();
        self.run(key, iv, &mut buf, false)?;
        let n = unpad_ct(&buf, bs)?;
        let mut out = buf;
        out.truncate(n);
        Some(out)
    }
}

/// Constant-time PKCS #7 unpadding: the plaintext length or none.
pub fn unpad_ct(buf: &[u8], bs: usize) -> Option<usize> {
    let len = buf.len();
    if len < bs {
        return None;
    }
    let last = buf[len - 1] as usize;
    // good = 1 <= last <= bs, and every one of the last `last` bytes == last.
    let mut bad: usize = 0;
    bad |= (last.wrapping_sub(1)) >> (usize::BITS - 1); // last == 0
    bad |= (bs.wrapping_sub(last)) >> (usize::BITS - 1); // last > bs
    for i in 0..bs {
        let b = buf[len - 1 - i] as usize;
        // in_pad = i < last
        let in_pad = (i.wrapping_sub(last)) >> (usize::BITS - 1);
        bad |= in_pad & (((b ^ last).wrapping_neg()) >> (usize::BITS - 1));
    }
    if bad == 0 { Some(len - last) } else { None }
}

/// EVP_BytesToKey with MD5 and one iteration (OpenSSL's documented legacy
/// PEM key derivation; RFC 1423 §1.1 for the DES case).
pub fn bytes_to_key(pass: &[u8], salt: &[u8], key_len: usize) -> Vec<u8> {
    let md = Md::H(HashAlgorithm::Md5);
    let mut out = Vec::new();
    let mut prev: Vec<u8> = Vec::new();
    while out.len() < key_len {
        let mut h = crate::digest::Hasher::new(md);
        h.update(&prev);
        h.update(pass);
        h.update(salt);
        prev = h.finish();
        out.extend_from_slice(&prev);
    }
    out.truncate(key_len);
    out
}

/// Decrypts a legacy encrypted PEM body (`DEK-Info: CIPHER,IVHEX`).
pub fn pem_decrypt(dek_info: &str, pass: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    let (name, ivhex) = dek_info.split_once(',')?;
    let c = CbcCipher::by_name(name.trim())?;
    let iv = hex_decode(ivhex.trim())?;
    if iv.len() != c.block() {
        return None;
    }
    let key = bytes_to_key(pass, &iv[..8], c.key_len());
    c.decrypt(&key, &iv, data)
}

/// Encrypts a legacy PEM body; returns (DEK-Info value, ciphertext).
pub fn pem_encrypt(c: CbcCipher, pass: &[u8], data: &[u8]) -> Option<(String, Vec<u8>)> {
    let mut iv = vec![0u8; c.block()];
    crate::random_bytes(&mut iv);
    let key = bytes_to_key(pass, &iv[..8], c.key_len());
    let ct = c.encrypt(&key, &iv, data)?;
    Some((format!("{},{}", c.name(), hex_upper(&iv)), ct))
}

pub fn hex_upper(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02X}", x)).collect()
}

pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PbeError {
    /// Unknown or unsupported algorithm.
    Unsupported,
    /// Wrong password (or corrupt data).
    BadDecrypt,
    Decode,
}

/// Decrypts an EncryptedPrivateKeyInfo-style (AlgorithmIdentifier,
/// ciphertext) pair.
pub fn decrypt(alg: &der::Tlv, data: &[u8], pass: &[u8]) -> Result<Vec<u8>, PbeError> {
    let e = |_| PbeError::Decode;
    let mut r = alg.reader();
    let oid = r.next().and_then(|x| x.oid()).map_err(e)?;
    let params = r.next().map_err(e)?;
    match oid.as_str() {
        crate::oid::ids::PBES2 => {
            let mut pr = params.reader();
            let kdf = pr.expect(tag::SEQUENCE).map_err(e)?;
            let enc = pr.expect(tag::SEQUENCE).map_err(e)?;
            let mut kr = kdf.reader();
            let koid = kr.next().and_then(|x| x.oid()).map_err(e)?;
            if koid != crate::oid::ids::PBKDF2 {
                return Err(PbeError::Unsupported);
            }
            let kp = kr.expect(tag::SEQUENCE).map_err(e)?;
            let mut kpr = kp.reader();
            let salt = kpr.expect(tag::OCTET_STRING).map_err(e)?.content.to_vec();
            let iter = kpr.next().and_then(|x| x.small_int()).map_err(e)?;
            let mut key_len = None;
            let mut prf = Md::H(HashAlgorithm::Sha1);
            while !kpr.is_empty() {
                let x = kpr.next().map_err(e)?;
                if x.is_universal(tag::INTEGER) {
                    key_len = Some(x.small_int().map_err(e)? as usize);
                } else if x.is_universal(tag::SEQUENCE) {
                    let po = x.reader().next().and_then(|y| y.oid()).map_err(e)?;
                    prf = Md::by_hmac_oid(&po).ok_or(PbeError::Unsupported)?;
                }
            }
            let mut er = enc.reader();
            let coid = er.next().and_then(|x| x.oid()).map_err(e)?;
            let iv = er.expect(tag::OCTET_STRING).map_err(e)?.content.to_vec();
            let c = CbcCipher::by_oid(&coid).ok_or(PbeError::Unsupported)?;
            if !(1..=10_000_000).contains(&iter) {
                return Err(PbeError::Decode);
            }
            let key = crate::digest::pbkdf2(
                prf,
                pass,
                &salt,
                iter as u32,
                key_len.unwrap_or(c.key_len()),
            );
            c.decrypt(&key, &iv, data).ok_or(PbeError::BadDecrypt)
        }
        // PKCS #12 PBE (RFC 7292 appendix C).
        "1.2.840.113549.1.12.1.3" | "1.2.840.113549.1.12.1.4" => {
            let mut pr = params.reader();
            let salt = pr.expect(tag::OCTET_STRING).map_err(e)?.content.to_vec();
            let iter = pr.next().and_then(|x| x.small_int()).map_err(e)?;
            let (c, klen) = if oid.ends_with(".3") {
                (CbcCipher::DesEde3, 24)
            } else {
                (CbcCipher::DesEde, 16)
            };
            let md = Md::H(HashAlgorithm::Sha1);
            let key = pkcs12_kdf(md, pass, &salt, 1, iter as u32, klen);
            let iv = pkcs12_kdf(md, pass, &salt, 2, iter as u32, 8);
            c.decrypt(&key, &iv, data).ok_or(PbeError::BadDecrypt)
        }
        // PBES1 (RFC 8018 §6.1) with DES.
        "1.2.840.113549.1.5.3" | "1.2.840.113549.1.5.10" => {
            let mut pr = params.reader();
            let salt = pr.expect(tag::OCTET_STRING).map_err(e)?.content.to_vec();
            let iter = pr.next().and_then(|x| x.small_int()).map_err(e)?;
            let md = if oid.ends_with(".3") {
                Md::H(HashAlgorithm::Md5)
            } else {
                Md::H(HashAlgorithm::Sha1)
            };
            let mut t = [pass, &salt[..]].concat();
            for _ in 0..iter.max(1) {
                t = md.digest(&t);
            }
            CbcCipher::Des
                .decrypt(&t[..8], &t[8..16], data)
                .ok_or(PbeError::BadDecrypt)
        }
        _ => Err(PbeError::Unsupported),
    }
}

/// The PKCS #12 key derivation (RFC 7292 appendix B.2). The password is
/// the BMPString form with its terminating NUL.
pub fn pkcs12_kdf(md: Md, pass: &[u8], salt: &[u8], id: u8, iter: u32, n: usize) -> Vec<u8> {
    let u = md.output_len();
    let v = md.block_len();
    let d = vec![id; v];
    let fill = |s: &[u8]| -> Vec<u8> {
        if s.is_empty() {
            return Vec::new();
        }
        let len = v * s.len().div_ceil(v);
        (0..len).map(|i| s[i % s.len()]).collect()
    };
    let bmp = bmp_password(pass);
    let mut i_buf = fill(salt);
    i_buf.extend(fill(&bmp));
    let mut out = Vec::new();
    while out.len() < n {
        let mut a = md.digest(&[&d[..], &i_buf[..]].concat());
        for _ in 1..iter {
            a = md.digest(&a);
        }
        out.extend_from_slice(&a);
        if out.len() >= n {
            break;
        }
        let b: Vec<u8> = (0..v).map(|i| a[i % u]).collect();
        for chunk in i_buf.chunks_mut(v) {
            // chunk = (chunk + b + 1) mod 2^(8v)
            let mut carry = 1u16;
            for k in (0..v).rev() {
                let s = chunk[k] as u16 + b[k] as u16 + carry;
                chunk[k] = s as u8;
                carry = s >> 8;
            }
        }
    }
    out.truncate(n);
    out
}

/// A password as a NUL-terminated BMPString (RFC 7292 appendix B.1),
/// reading the bytes as UTF-8 (Latin-1 for invalid sequences).
pub fn bmp_password(pass: &[u8]) -> Vec<u8> {
    let s: Vec<u16> = match std::str::from_utf8(pass) {
        Ok(t) => t.encode_utf16().collect(),
        Err(_) => pass.iter().map(|&b| b as u16).collect(),
    };
    let mut out = Vec::with_capacity(s.len() * 2 + 2);
    for c in s {
        out.extend_from_slice(&c.to_be_bytes());
    }
    out.extend_from_slice(&[0, 0]);
    out
}

/// PBES2 encryption of a PrivateKeyInfo into an EncryptedPrivateKeyInfo
/// with PBKDF2-HMAC-SHA256, a 16-byte salt and 2048 iterations (the
/// parameters PHP 8.5 writes, observed by decoding its output).
pub fn pbes2_encrypt(c: CbcCipher, pass: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    let mut salt = [0u8; 16];
    crate::random_bytes(&mut salt);
    let iter = 2048;
    let mut iv = vec![0u8; c.block()];
    crate::random_bytes(&mut iv);
    let key = crate::digest::pbkdf2(Md::H(HashAlgorithm::Sha256), pass, &salt, iter, c.key_len());
    let ct = c.encrypt(&key, &iv, data)?;
    let kdf = der::seq_vec(&[
        der::oid(crate::oid::ids::PBKDF2),
        der::seq_vec(&[
            der::octet_string(&salt),
            der::int(iter as i64),
            der::seq_vec(&[der::oid("1.2.840.113549.2.9"), der::null()]),
        ]),
    ]);
    let enc = der::seq_vec(&[der::oid(c.oid()), der::octet_string(&iv)]);
    let alg = der::seq_vec(&[der::oid(crate::oid::ids::PBES2), der::seq_vec(&[kdf, enc])]);
    Some(der::seq_vec(&[alg, der::octet_string(&ct)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unpad() {
        let mut b = vec![1u8; 16];
        b[15] = 1;
        assert_eq!(unpad_ct(&b, 16), Some(15));
        let b = [3u8; 16];
        assert_eq!(unpad_ct(&b, 16), Some(13));
        let mut b = [3u8; 16];
        b[13] = 2;
        assert_eq!(unpad_ct(&b, 16), None);
        let b = [0u8; 16];
        assert_eq!(unpad_ct(&b, 16), None);
        let b = [17u8; 16];
        assert_eq!(unpad_ct(&b, 16), None);
        let b = [16u8; 16];
        assert_eq!(unpad_ct(&b, 16), Some(0));
    }
    #[test]
    fn cbc_round_trip() {
        for c in [CbcCipher::Aes128, CbcCipher::DesEde3, CbcCipher::Aes256] {
            let key = vec![7u8; c.key_len()];
            let iv = vec![9u8; c.block()];
            let ct = c
                .encrypt(&key, &iv, b"hello world, this is a test")
                .unwrap();
            assert_eq!(
                c.decrypt(&key, &iv, &ct).unwrap(),
                b"hello world, this is a test"
            );
        }
    }
}
