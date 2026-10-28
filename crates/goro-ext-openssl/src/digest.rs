//! Message digests by OpenSSL name: openssl_digest(), openssl_get_md_methods()
//! and the digests the signature functions accept.
//!
//! The name lists are those PHP 8.5 reports (observed on PHP 8.5:
//! openssl_get_md_methods() with and without aliases).

pub use purecrypto::hash::HashAlgorithm;
use purecrypto::hash::{ExtendableOutput, Shake128, Shake256};

/// A digest algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Md {
    H(HashAlgorithm),
    /// MD5 || SHA-1 (the TLS 1.0 handshake hash).
    Md5Sha1,
    /// SHAKE128 with OpenSSL's default 16-byte output.
    Shake128,
    /// SHAKE256 with OpenSSL's default 32-byte output.
    Shake256,
}

/// openssl_get_md_methods() (observed on PHP 8.5).
pub const MD_METHODS: &[&str] = &[
    "blake2b512",
    "blake2s256",
    "md4",
    "md5",
    "md5-sha1",
    "ripemd160",
    "sha1",
    "sha224",
    "sha256",
    "sha3-224",
    "sha3-256",
    "sha3-384",
    "sha3-512",
    "sha384",
    "sha512",
    "sha512-224",
    "sha512-256",
    "shake128",
    "shake256",
    "sm3",
    "whirlpool",
];

/// openssl_get_md_methods(true) (observed on PHP 8.5).
pub const MD_METHODS_ALIASES: &[&str] = &[
    "RSA-MD4",
    "RSA-MD5",
    "RSA-RIPEMD160",
    "RSA-SHA1",
    "RSA-SHA1-2",
    "RSA-SHA224",
    "RSA-SHA256",
    "RSA-SHA3-224",
    "RSA-SHA3-256",
    "RSA-SHA3-384",
    "RSA-SHA3-512",
    "RSA-SHA384",
    "RSA-SHA512",
    "RSA-SHA512/224",
    "RSA-SHA512/256",
    "RSA-SM3",
    "blake2b512",
    "blake2s256",
    "id-rsassa-pkcs1-v1_5-with-sha3-224",
    "id-rsassa-pkcs1-v1_5-with-sha3-256",
    "id-rsassa-pkcs1-v1_5-with-sha3-384",
    "id-rsassa-pkcs1-v1_5-with-sha3-512",
    "md4",
    "md4WithRSAEncryption",
    "md5",
    "md5-sha1",
    "md5WithRSAEncryption",
    "ripemd",
    "ripemd160",
    "ripemd160WithRSA",
    "rmd160",
    "sha1",
    "sha1WithRSAEncryption",
    "sha224",
    "sha224WithRSAEncryption",
    "sha256",
    "sha256WithRSAEncryption",
    "sha3-224",
    "sha3-256",
    "sha3-384",
    "sha3-512",
    "sha384",
    "sha384WithRSAEncryption",
    "sha512",
    "sha512-224",
    "sha512-224WithRSAEncryption",
    "sha512-256",
    "sha512-256WithRSAEncryption",
    "sha512WithRSAEncryption",
    "shake128",
    "shake256",
    "sm3",
    "sm3WithRSAEncryption",
    "ssl3-md5",
    "ssl3-sha1",
    "whirlpool",
];

impl Md {
    /// Looks a digest up by any of its names (ASCII case ignored), its
    /// signature-algorithm aliases or its OID.
    pub fn by_name(name: &str) -> Option<Md> {
        let l = name.to_ascii_lowercase();
        let h = |a| Some(Md::H(a));
        match l.as_str() {
            "md4" | "rsa-md4" | "md4withrsaencryption" | "1.2.840.113549.2.4" => {
                h(HashAlgorithm::Md4)
            }
            "md5" | "rsa-md5" | "md5withrsaencryption" | "ssl3-md5" | "1.2.840.113549.2.5" => {
                h(HashAlgorithm::Md5)
            }
            "md5-sha1" => Some(Md::Md5Sha1),
            "sha1"
            | "sha-1"
            | "rsa-sha1"
            | "rsa-sha1-2"
            | "sha1withrsaencryption"
            | "ssl3-sha1"
            | "1.3.14.3.2.26" => h(HashAlgorithm::Sha1),
            "sha224"
            | "sha2-224"
            | "sha-224"
            | "rsa-sha224"
            | "sha224withrsaencryption"
            | "2.16.840.1.101.3.4.2.4" => h(HashAlgorithm::Sha224),
            "sha256"
            | "sha2-256"
            | "sha-256"
            | "rsa-sha256"
            | "sha256withrsaencryption"
            | "2.16.840.1.101.3.4.2.1" => h(HashAlgorithm::Sha256),
            "sha384"
            | "sha2-384"
            | "sha-384"
            | "rsa-sha384"
            | "sha384withrsaencryption"
            | "2.16.840.1.101.3.4.2.2" => h(HashAlgorithm::Sha384),
            "sha512"
            | "sha2-512"
            | "sha-512"
            | "rsa-sha512"
            | "sha512withrsaencryption"
            | "2.16.840.1.101.3.4.2.3" => h(HashAlgorithm::Sha512),
            "sha512-224"
            | "sha2-512/224"
            | "sha-512/224"
            | "sha512/224"
            | "rsa-sha512/224"
            | "sha512-224withrsaencryption"
            | "2.16.840.1.101.3.4.2.5" => h(HashAlgorithm::Sha512_224),
            "sha512-256"
            | "sha2-512/256"
            | "sha-512/256"
            | "sha512/256"
            | "rsa-sha512/256"
            | "sha512-256withrsaencryption"
            | "2.16.840.1.101.3.4.2.6" => h(HashAlgorithm::Sha512_256),
            "sha3-224"
            | "rsa-sha3-224"
            | "id-rsassa-pkcs1-v1_5-with-sha3-224"
            | "2.16.840.1.101.3.4.2.7" => h(HashAlgorithm::Sha3_224),
            "sha3-256"
            | "rsa-sha3-256"
            | "id-rsassa-pkcs1-v1_5-with-sha3-256"
            | "2.16.840.1.101.3.4.2.8" => h(HashAlgorithm::Sha3_256),
            "sha3-384"
            | "rsa-sha3-384"
            | "id-rsassa-pkcs1-v1_5-with-sha3-384"
            | "2.16.840.1.101.3.4.2.9" => h(HashAlgorithm::Sha3_384),
            "sha3-512"
            | "rsa-sha3-512"
            | "id-rsassa-pkcs1-v1_5-with-sha3-512"
            | "2.16.840.1.101.3.4.2.10" => h(HashAlgorithm::Sha3_512),
            "shake128" | "2.16.840.1.101.3.4.2.11" => Some(Md::Shake128),
            "shake256" | "2.16.840.1.101.3.4.2.12" => Some(Md::Shake256),
            "ripemd160" | "ripemd" | "rmd160" | "ripemd-160" | "rsa-ripemd160"
            | "ripemd160withrsa" | "1.3.36.3.2.1" => h(HashAlgorithm::Ripemd160),
            "sm3" | "rsa-sm3" | "sm3withrsaencryption" | "1.2.156.10197.1.401" => {
                h(HashAlgorithm::Sm3)
            }
            "whirlpool" | "1.0.10118.3.0.55" => h(HashAlgorithm::Whirlpool),
            "blake2b512" | "1.3.6.1.4.1.1722.12.2.1.16" => h(HashAlgorithm::Blake2b512),
            "blake2s256" | "1.3.6.1.4.1.1722.12.2.2.8" => h(HashAlgorithm::Blake2s256),
            _ => None,
        }
    }

    /// The `OPENSSL_ALGO_*` constants (PHP manual: Signature algorithms).
    pub fn by_algo_const(n: i64) -> Option<Md> {
        Some(Md::H(match n {
            1 => HashAlgorithm::Sha1,
            2 => HashAlgorithm::Md5,
            3 => HashAlgorithm::Md4,
            6 => HashAlgorithm::Sha224,
            7 => HashAlgorithm::Sha256,
            8 => HashAlgorithm::Sha384,
            9 => HashAlgorithm::Sha512,
            10 => HashAlgorithm::Ripemd160,
            _ => return None,
        }))
    }

    pub fn output_len(self) -> usize {
        match self {
            Md::H(a) => a.output_len(),
            Md::Md5Sha1 => 36,
            Md::Shake128 => 16,
            Md::Shake256 => 32,
        }
    }

    pub fn block_len(self) -> usize {
        match self {
            Md::H(a) => a.block_len(),
            Md::Md5Sha1 => 64,
            Md::Shake128 => 168,
            Md::Shake256 => 136,
        }
    }

    pub fn digest(self, data: &[u8]) -> Vec<u8> {
        let mut h = Hasher::new(self);
        h.update(data);
        h.finish()
    }

    /// The canonical (OpenSSL) name.
    pub fn name(self) -> &'static str {
        match self {
            Md::H(a) => match a {
                HashAlgorithm::Sha512_224 => "sha512-224",
                HashAlgorithm::Sha512_256 => "sha512-256",
                other => other.name(),
            },
            Md::Md5Sha1 => "md5-sha1",
            Md::Shake128 => "shake128",
            Md::Shake256 => "shake256",
        }
    }

    /// The digest's OID (for DigestInfo and AlgorithmIdentifiers).
    pub fn oid(self) -> Option<&'static str> {
        Some(match self {
            Md::H(a) => match a {
                HashAlgorithm::Md2 => "1.2.840.113549.2.2",
                HashAlgorithm::Md4 => "1.2.840.113549.2.4",
                HashAlgorithm::Md5 => "1.2.840.113549.2.5",
                HashAlgorithm::Sha1 => "1.3.14.3.2.26",
                HashAlgorithm::Sha224 => "2.16.840.1.101.3.4.2.4",
                HashAlgorithm::Sha256 => "2.16.840.1.101.3.4.2.1",
                HashAlgorithm::Sha384 => "2.16.840.1.101.3.4.2.2",
                HashAlgorithm::Sha512 => "2.16.840.1.101.3.4.2.3",
                HashAlgorithm::Sha512_224 => "2.16.840.1.101.3.4.2.5",
                HashAlgorithm::Sha512_256 => "2.16.840.1.101.3.4.2.6",
                HashAlgorithm::Sha3_224 => "2.16.840.1.101.3.4.2.7",
                HashAlgorithm::Sha3_256 => "2.16.840.1.101.3.4.2.8",
                HashAlgorithm::Sha3_384 => "2.16.840.1.101.3.4.2.9",
                HashAlgorithm::Sha3_512 => "2.16.840.1.101.3.4.2.10",
                HashAlgorithm::Ripemd160 => "1.3.36.3.2.1",
                HashAlgorithm::Sm3 => "1.2.156.10197.1.401",
                HashAlgorithm::Whirlpool => "1.0.10118.3.0.55",
                HashAlgorithm::Blake2b512 => "1.3.6.1.4.1.1722.12.2.1.16",
                HashAlgorithm::Blake2s256 => "1.3.6.1.4.1.1722.12.2.2.8",
                _ => return None,
            },
            Md::Shake128 => "2.16.840.1.101.3.4.2.11",
            Md::Shake256 => "2.16.840.1.101.3.4.2.12",
            Md::Md5Sha1 => return None,
        })
    }

    pub fn by_oid(oid: &str) -> Option<Md> {
        let m = Md::by_name(oid)?;
        if m.oid() == Some(oid) { Some(m) } else { None }
    }

    /// The HMAC/PRF OID (RFC 8018 §B.1.2) for PBKDF2 with this digest.
    pub fn hmac_oid(self) -> Option<&'static str> {
        Some(match self {
            Md::H(HashAlgorithm::Sha1) => "1.2.840.113549.2.7",
            Md::H(HashAlgorithm::Sha224) => "1.2.840.113549.2.8",
            Md::H(HashAlgorithm::Sha256) => "1.2.840.113549.2.9",
            Md::H(HashAlgorithm::Sha384) => "1.2.840.113549.2.10",
            Md::H(HashAlgorithm::Sha512) => "1.2.840.113549.2.11",
            Md::H(HashAlgorithm::Sha512_224) => "1.2.840.113549.2.12",
            Md::H(HashAlgorithm::Sha512_256) => "1.2.840.113549.2.13",
            _ => return None,
        })
    }

    pub fn by_hmac_oid(oid: &str) -> Option<Md> {
        Some(Md::H(match oid {
            "1.2.840.113549.2.7" => HashAlgorithm::Sha1,
            "1.2.840.113549.2.8" => HashAlgorithm::Sha224,
            "1.2.840.113549.2.9" => HashAlgorithm::Sha256,
            "1.2.840.113549.2.10" => HashAlgorithm::Sha384,
            "1.2.840.113549.2.11" => HashAlgorithm::Sha512,
            "1.2.840.113549.2.12" => HashAlgorithm::Sha512_224,
            "1.2.840.113549.2.13" => HashAlgorithm::Sha512_256,
            _ => return None,
        }))
    }

    /// The DER DigestInfo of a hash (RFC 8017 §9.2).
    pub fn digest_info(self, hash: &[u8]) -> Option<Vec<u8>> {
        let oid = self.oid()?;
        use crate::der;
        Some(der::seq(&[
            &der::seq(&[&der::oid(oid), &der::null()]),
            &der::octet_string(hash),
        ]))
    }

    /// HMAC (RFC 2104).
    pub fn hmac(self, key: &[u8], data: &[u8]) -> Vec<u8> {
        let bl = self.block_len();
        let mut k = if key.len() > bl {
            self.digest(key)
        } else {
            key.to_vec()
        };
        k.resize(bl, 0);
        let mut ipad = Hasher::new(self);
        let ik: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
        ipad.update(&ik);
        ipad.update(data);
        let inner = ipad.finish();
        let mut opad = Hasher::new(self);
        let ok: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
        opad.update(&ok);
        opad.update(&inner);
        opad.finish()
    }
}

/// A streaming hasher for any [`Md`].
#[allow(clippy::large_enum_variant)]
pub enum Hasher {
    H(purecrypto::hash::Hasher),
    Md5Sha1(purecrypto::hash::Hasher, purecrypto::hash::Hasher),
    Shake128(Shake128),
    Shake256(Shake256),
}

impl Hasher {
    pub fn new(md: Md) -> Hasher {
        match md {
            Md::H(a) => Hasher::H(a.hasher()),
            Md::Md5Sha1 => {
                Hasher::Md5Sha1(HashAlgorithm::Md5.hasher(), HashAlgorithm::Sha1.hasher())
            }
            Md::Shake128 => Hasher::Shake128(Shake128::new()),
            Md::Shake256 => Hasher::Shake256(Shake256::new()),
        }
    }
    pub fn update(&mut self, d: &[u8]) {
        match self {
            Hasher::H(h) => h.update(d),
            Hasher::Md5Sha1(a, b) => {
                a.update(d);
                b.update(d);
            }
            Hasher::Shake128(x) => purecrypto::hash::ExtendableOutput::update(x, d),
            Hasher::Shake256(x) => purecrypto::hash::ExtendableOutput::update(x, d),
        }
    }
    pub fn finish(self) -> Vec<u8> {
        match self {
            Hasher::H(h) => h.finalize().as_slice().to_vec(),
            Hasher::Md5Sha1(a, b) => {
                let mut v = a.finalize().as_slice().to_vec();
                v.extend_from_slice(b.finalize().as_slice());
                v
            }
            Hasher::Shake128(x) => {
                let mut out = vec![0u8; 16];
                purecrypto::hash::ExtendableOutput::finalize_into(x, &mut out);
                out
            }
            Hasher::Shake256(x) => {
                let mut out = vec![0u8; 32];
                purecrypto::hash::ExtendableOutput::finalize_into(x, &mut out);
                out
            }
        }
    }
}

/// PBKDF2 (RFC 8018 §5.2) with HMAC over `md`.
pub fn pbkdf2(md: Md, password: &[u8], salt: &[u8], iterations: u32, len: usize) -> Vec<u8> {
    let hl = md.output_len();
    let mut out = Vec::with_capacity(len);
    let mut block = 1u32;
    // Precompute the keyed pads once.
    let bl = md.block_len();
    let mut k = if password.len() > bl {
        md.digest(password)
    } else {
        password.to_vec()
    };
    k.resize(bl, 0);
    let ik: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    let ok: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    let prf = |data: &[&[u8]]| -> Vec<u8> {
        let mut h = Hasher::new(md);
        h.update(&ik);
        for d in data {
            h.update(d);
        }
        let inner = h.finish();
        let mut h = Hasher::new(md);
        h.update(&ok);
        h.update(&inner);
        h.finish()
    };
    while out.len() < len {
        let mut u = prf(&[salt, &block.to_be_bytes()]);
        let mut t = u.clone();
        for _ in 1..iterations {
            u = prf(&[&u]);
            for (a, b) in t.iter_mut().zip(&u) {
                *a ^= b;
            }
        }
        let take = (len - out.len()).min(hl);
        out.extend_from_slice(&t[..take]);
        block += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pbkdf2_rfc6070() {
        let r = pbkdf2(Md::H(HashAlgorithm::Sha1), b"password", b"salt", 2, 20);
        assert_eq!(
            r,
            [
                0xea, 0x6c, 0x01, 0x4d, 0xc7, 0x2d, 0x6f, 0x8c, 0xcd, 0x1e, 0xd9, 0x2a, 0xce, 0x1d,
                0x41, 0xf0, 0xd8, 0xde, 0x89, 0x57
            ]
        );
    }
}
