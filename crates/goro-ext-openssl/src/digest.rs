//! EVP message digests (`EVP_get_digestbyname` / `EVP_MD_fetch` emulation).
//!
//! Names are resolved case-insensitively against the union of the legacy
//! `OBJ_NAME` table (which also contains the `RSA-*` / `*WithRSAEncryption`
//! signature aliases) and the provider fetch names, as php-src does in
//! `php_openssl_get_evp_md_by_name()`.

use purecrypto::hash::{ExtendableOutput, HashAlgorithm, Hasher as PcHasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(clippy::enum_variant_names)] // OpenSSL digest names (MD4, MD5, MD5-SHA1, MDC2)
pub enum Md {
    Md4,
    Md5,
    Md5Sha1,
    Mdc2,
    Ripemd160,
    Sha1,
    Sha224,
    Sha256,
    Sha384,
    Sha512,
    Sha512_224,
    Sha512_256,
    Sha3_224,
    Sha3_256,
    Sha3_384,
    Sha3_512,
    Shake128,
    Shake256,
    Sm3,
    Blake2b512,
    Blake2s256,
    Whirlpool,
}

/// `openssl_get_md_methods(false)` (OBJ_NAME_do_all_sorted, non-aliases).
pub const MD_METHODS: &[&str] = &[
    "blake2b512",
    "blake2s256",
    "md4",
    "md5",
    "md5-sha1",
    "mdc2",
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

/// `openssl_get_md_methods(true)` (OBJ_NAME_do_all_sorted, names and aliases).
pub const MD_METHODS_ALIASES: &[&str] = &[
    "RSA-MD4",
    "RSA-MD5",
    "RSA-MDC2",
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
    "mdc2",
    "mdc2WithRSA",
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
    /// Resolve a digest name (case-insensitive).
    pub fn by_name(name: &str) -> Option<Md> {
        let n = name.to_ascii_lowercase();
        Some(match n.as_str() {
            "md4" | "rsa-md4" | "md4withrsaencryption" | "1.2.840.113549.2.4" => Md::Md4,
            "md5" | "rsa-md5" | "md5withrsaencryption" | "ssl3-md5" | "1.2.840.113549.2.5" => {
                Md::Md5
            }
            "md5-sha1" => Md::Md5Sha1,
            "mdc2" | "rsa-mdc2" | "mdc2withrsa" | "2.5.8.3.101" => Md::Mdc2,
            "ripemd160" | "ripemd" | "rmd160" | "ripemd-160" | "rsa-ripemd160"
            | "ripemd160withrsa" | "1.3.36.3.2.1" => Md::Ripemd160,
            "sha1"
            | "sha-1"
            | "ssl3-sha1"
            | "rsa-sha1"
            | "rsa-sha1-2"
            | "sha1withrsaencryption"
            | "1.3.14.3.2.26" => Md::Sha1,
            "sha224"
            | "sha2-224"
            | "sha-224"
            | "rsa-sha224"
            | "sha224withrsaencryption"
            | "2.16.840.1.101.3.4.2.4" => Md::Sha224,
            "sha256"
            | "sha2-256"
            | "sha-256"
            | "rsa-sha256"
            | "sha256withrsaencryption"
            | "2.16.840.1.101.3.4.2.1" => Md::Sha256,
            "sha384"
            | "sha2-384"
            | "sha-384"
            | "rsa-sha384"
            | "sha384withrsaencryption"
            | "2.16.840.1.101.3.4.2.2" => Md::Sha384,
            "sha512"
            | "sha2-512"
            | "sha-512"
            | "rsa-sha512"
            | "sha512withrsaencryption"
            | "2.16.840.1.101.3.4.2.3" => Md::Sha512,
            "sha512-224"
            | "sha2-512/224"
            | "sha-512/224"
            | "sha512/224"
            | "rsa-sha512/224"
            | "sha512-224withrsaencryption"
            | "2.16.840.1.101.3.4.2.5" => Md::Sha512_224,
            "sha512-256"
            | "sha2-512/256"
            | "sha-512/256"
            | "sha512/256"
            | "rsa-sha512/256"
            | "sha512-256withrsaencryption"
            | "2.16.840.1.101.3.4.2.6" => Md::Sha512_256,
            "sha3-224"
            | "rsa-sha3-224"
            | "id-rsassa-pkcs1-v1_5-with-sha3-224"
            | "2.16.840.1.101.3.4.2.7" => Md::Sha3_224,
            "sha3-256"
            | "rsa-sha3-256"
            | "id-rsassa-pkcs1-v1_5-with-sha3-256"
            | "2.16.840.1.101.3.4.2.8" => Md::Sha3_256,
            "sha3-384"
            | "rsa-sha3-384"
            | "id-rsassa-pkcs1-v1_5-with-sha3-384"
            | "2.16.840.1.101.3.4.2.9" => Md::Sha3_384,
            "sha3-512"
            | "rsa-sha3-512"
            | "id-rsassa-pkcs1-v1_5-with-sha3-512"
            | "2.16.840.1.101.3.4.2.10" => Md::Sha3_512,
            "shake128" | "shake-128" | "2.16.840.1.101.3.4.2.11" => Md::Shake128,
            "shake256" | "shake-256" | "2.16.840.1.101.3.4.2.12" => Md::Shake256,
            "sm3" | "rsa-sm3" | "sm3withrsaencryption" | "1.2.156.10197.1.401" => Md::Sm3,
            "blake2b512" | "blake2b-512" | "1.3.6.1.4.1.1722.12.2.1.16" => Md::Blake2b512,
            "blake2s256" | "blake2s-256" | "1.3.6.1.4.1.1722.12.2.2.8" => Md::Blake2s256,
            "whirlpool" | "1.0.10118.3.0.55" => Md::Whirlpool,
            _ => return None,
        })
    }

    /// Extendable-output function (EVP_MD_FLAG_XOF).
    pub fn is_xof(self) -> bool {
        matches!(self, Md::Shake128 | Md::Shake256)
    }

    /// Digest OID (DER content bytes, dotted form).
    pub fn oid(self) -> Option<&'static str> {
        Some(match self {
            Md::Md4 => "1.2.840.113549.2.4",
            Md::Md5 => "1.2.840.113549.2.5",
            Md::Mdc2 => "2.5.8.3.101",
            Md::Ripemd160 => "1.3.36.3.2.1",
            Md::Sha1 => "1.3.14.3.2.26",
            Md::Sha224 => "2.16.840.1.101.3.4.2.4",
            Md::Sha256 => "2.16.840.1.101.3.4.2.1",
            Md::Sha384 => "2.16.840.1.101.3.4.2.2",
            Md::Sha512 => "2.16.840.1.101.3.4.2.3",
            Md::Sha512_224 => "2.16.840.1.101.3.4.2.5",
            Md::Sha512_256 => "2.16.840.1.101.3.4.2.6",
            Md::Sha3_224 => "2.16.840.1.101.3.4.2.7",
            Md::Sha3_256 => "2.16.840.1.101.3.4.2.8",
            Md::Sha3_384 => "2.16.840.1.101.3.4.2.9",
            Md::Sha3_512 => "2.16.840.1.101.3.4.2.10",
            Md::Shake128 => "2.16.840.1.101.3.4.2.11",
            Md::Shake256 => "2.16.840.1.101.3.4.2.12",
            Md::Sm3 => "1.2.156.10197.1.401",
            Md::Blake2b512 => "1.3.6.1.4.1.1722.12.2.1.16",
            Md::Blake2s256 => "1.3.6.1.4.1.1722.12.2.2.8",
            Md::Whirlpool => "1.0.10118.3.0.55",
            Md::Md5Sha1 => return None,
        })
    }

    /// `EVP_MD_size()`.
    pub fn size(self) -> usize {
        match self {
            Md::Md4 | Md::Md5 | Md::Mdc2 => 16,
            Md::Md5Sha1 => 36,
            Md::Ripemd160 | Md::Sha1 => 20,
            Md::Sha224 | Md::Sha512_224 | Md::Sha3_224 => 28,
            Md::Sha256 | Md::Sha512_256 | Md::Sha3_256 | Md::Sm3 | Md::Blake2s256 => 32,
            Md::Shake128 => 16,
            Md::Shake256 => 32,
            Md::Sha384 | Md::Sha3_384 => 48,
            Md::Sha512 | Md::Sha3_512 | Md::Blake2b512 | Md::Whirlpool => 64,
        }
    }

    /// `EVP_MD_block_size()`.
    pub fn block_size(self) -> usize {
        match self {
            Md::Md4
            | Md::Md5
            | Md::Md5Sha1
            | Md::Ripemd160
            | Md::Sha1
            | Md::Sha224
            | Md::Sha256
            | Md::Sm3
            | Md::Whirlpool
            | Md::Blake2s256 => 64,
            Md::Mdc2 => 8,
            Md::Sha384 | Md::Sha512 | Md::Sha512_224 | Md::Sha512_256 | Md::Blake2b512 => 128,
            Md::Sha3_224 => 144,
            Md::Sha3_256 => 136,
            Md::Sha3_384 => 104,
            Md::Sha3_512 => 72,
            Md::Shake128 => 168,
            Md::Shake256 => 136,
        }
    }

    /// One-shot digest.
    pub fn digest(self, data: &[u8]) -> Vec<u8> {
        let mut h = Hasher::new(self);
        h.update(data);
        h.finish()
    }
}

/// Incremental hasher over any [`Md`] (purecrypto digests).
pub enum Hasher {
    Pc(Box<PcHasher>),
    Md5Sha1(Box<PcHasher>, Box<PcHasher>),
    Mdc2(Mdc2),
    Shake128(purecrypto::hash::Shake128),
    Shake256(purecrypto::hash::Shake256),
}

fn pc(alg: HashAlgorithm) -> Box<PcHasher> {
    Box::new(PcHasher::new(alg))
}

impl Hasher {
    pub fn new(md: Md) -> Hasher {
        match md {
            Md::Md4 => Hasher::Pc(pc(HashAlgorithm::Md4)),
            Md::Md5 => Hasher::Pc(pc(HashAlgorithm::Md5)),
            Md::Md5Sha1 => Hasher::Md5Sha1(pc(HashAlgorithm::Md5), pc(HashAlgorithm::Sha1)),
            Md::Mdc2 => Hasher::Mdc2(Mdc2::new()),
            Md::Ripemd160 => Hasher::Pc(pc(HashAlgorithm::Ripemd160)),
            Md::Sha1 => Hasher::Pc(pc(HashAlgorithm::Sha1)),
            Md::Sha224 => Hasher::Pc(pc(HashAlgorithm::Sha224)),
            Md::Sha256 => Hasher::Pc(pc(HashAlgorithm::Sha256)),
            Md::Sha384 => Hasher::Pc(pc(HashAlgorithm::Sha384)),
            Md::Sha512 => Hasher::Pc(pc(HashAlgorithm::Sha512)),
            Md::Sha512_224 => Hasher::Pc(pc(HashAlgorithm::Sha512_224)),
            Md::Sha512_256 => Hasher::Pc(pc(HashAlgorithm::Sha512_256)),
            Md::Sha3_224 => Hasher::Pc(pc(HashAlgorithm::Sha3_224)),
            Md::Sha3_256 => Hasher::Pc(pc(HashAlgorithm::Sha3_256)),
            Md::Sha3_384 => Hasher::Pc(pc(HashAlgorithm::Sha3_384)),
            Md::Sha3_512 => Hasher::Pc(pc(HashAlgorithm::Sha3_512)),
            Md::Shake128 => {
                Hasher::Shake128(<purecrypto::hash::Shake128 as ExtendableOutput>::new())
            }
            Md::Shake256 => {
                Hasher::Shake256(<purecrypto::hash::Shake256 as ExtendableOutput>::new())
            }
            Md::Sm3 => Hasher::Pc(pc(HashAlgorithm::Sm3)),
            Md::Blake2b512 => Hasher::Pc(pc(HashAlgorithm::Blake2b512)),
            Md::Blake2s256 => Hasher::Pc(pc(HashAlgorithm::Blake2s256)),
            Md::Whirlpool => Hasher::Pc(pc(HashAlgorithm::Whirlpool)),
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        match self {
            Hasher::Pc(d) => d.update(data),
            Hasher::Md5Sha1(a, b) => {
                a.update(data);
                b.update(data);
            }
            Hasher::Mdc2(m) => m.update(data),
            Hasher::Shake128(s) => ExtendableOutput::update(s, data),
            Hasher::Shake256(s) => ExtendableOutput::update(s, data),
        }
    }

    pub fn finish(self) -> Vec<u8> {
        match self {
            Hasher::Pc(d) => d.finalize().as_slice().to_vec(),
            Hasher::Md5Sha1(a, b) => {
                let mut v = a.finalize().as_slice().to_vec();
                v.extend_from_slice(b.finalize().as_slice());
                v
            }
            Hasher::Mdc2(m) => m.finish().to_vec(),
            Hasher::Shake128(s) => {
                let mut out = vec![0u8; 16];
                s.finalize_into(&mut out);
                out
            }
            Hasher::Shake256(s) => {
                let mut out = vec![0u8; 32];
                s.finalize_into(&mut out);
                out
            }
        }
    }
}

/// MDC-2 (ISO/IEC 10118-2) with DES, as in OpenSSL's `mdc2dgst.c` (pad type 1).
pub struct Mdc2 {
    h: [u8; 8],
    hh: [u8; 8],
    buf: Vec<u8>,
}

impl Mdc2 {
    fn new() -> Mdc2 {
        Mdc2 {
            h: [0x52; 8],
            hh: [0x25; 8],
            buf: Vec::new(),
        }
    }

    fn body(&mut self, block: &[u8]) {
        use purecrypto::cipher::BlockCipher64;
        self.h[0] = (self.h[0] & 0x9f) | 0x40;
        self.hh[0] = (self.hh[0] & 0x9f) | 0x20;
        let k = purecrypto::cipher::Des::new(&self.h);
        let kk = purecrypto::cipher::Des::new(&self.hh);
        let mut d = [0u8; 8];
        d.copy_from_slice(block);
        let mut dd = d;
        k.encrypt_block(&mut d);
        kk.encrypt_block(&mut dd);
        let mut x = [0u8; 8];
        let mut y = [0u8; 8];
        for i in 0..8 {
            x[i] = block[i] ^ d[i];
            y[i] = block[i] ^ dd[i];
        }
        self.h[..4].copy_from_slice(&x[..4]);
        self.h[4..].copy_from_slice(&y[4..]);
        self.hh[..4].copy_from_slice(&y[..4]);
        self.hh[4..].copy_from_slice(&x[4..]);
    }

    fn update(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
        let full = self.buf.len() / 8 * 8;
        let blocks: Vec<u8> = self.buf.drain(..full).collect();
        for b in blocks.chunks(8) {
            self.body(b);
        }
    }

    fn finish(mut self) -> [u8; 16] {
        if !self.buf.is_empty() {
            let mut b = std::mem::take(&mut self.buf);
            b.resize(8, 0);
            self.body(&b);
        }
        let mut out = [0u8; 16];
        out[..8].copy_from_slice(&self.h);
        out[8..].copy_from_slice(&self.hh);
        out
    }
}

/// HMAC over any non-XOF digest.
/// php_openssl_get_evp_md_by_name(): a failed EVP_MD_fetch() leaves
/// "unsupported" on the error queue.
pub fn fetch(name: &str) -> Option<Md> {
    let r = Md::by_name(name);
    if r.is_none() {
        crate::err::push(
            crate::err::LIB_EVP,
            crate::err::R_UNSUPPORTED,
            "unsupported",
        );
    }
    r
}

pub fn hmac(md: Md, key: &[u8], data: &[u8]) -> Vec<u8> {
    let bs = md.block_size();
    let mut k = if key.len() > bs {
        md.digest(key)
    } else {
        key.to_vec()
    };
    k.resize(bs, 0);
    let mut ipad = k.clone();
    let mut opad = k;
    for b in ipad.iter_mut() {
        *b ^= 0x36;
    }
    for b in opad.iter_mut() {
        *b ^= 0x5c;
    }
    let mut h = Hasher::new(md);
    h.update(&ipad);
    h.update(data);
    let inner = h.finish();
    let mut h = Hasher::new(md);
    h.update(&opad);
    h.update(&inner);
    h.finish()
}

/// PKCS#5 PBKDF2 (`PKCS5_PBKDF2_HMAC`).
pub fn pbkdf2(md: Md, password: &[u8], salt: &[u8], iterations: u32, out_len: usize) -> Vec<u8> {
    let hlen = md.size();
    let mut out = Vec::with_capacity(out_len);
    let mut block: u32 = 1;
    while out.len() < out_len {
        let mut s = salt.to_vec();
        s.extend_from_slice(&block.to_be_bytes());
        let mut u = hmac(md, password, &s);
        let mut t = u.clone();
        for _ in 1..iterations {
            u = hmac(md, password, &u);
            for (a, b) in t.iter_mut().zip(u.iter()) {
                *a ^= b;
            }
        }
        let take = (out_len - out.len()).min(hlen);
        out.extend_from_slice(&t[..take]);
        block += 1;
    }
    out
}
