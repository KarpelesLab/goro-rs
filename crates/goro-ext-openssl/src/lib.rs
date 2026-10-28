//! PHP openssl extension (PHP manual: OpenSSL) on purecrypto.
//!
//! The crate works on bytes and plain Rust values; the engine wiring
//! (`goro-engine`'s `builtins/openssl`) turns PHP values into these calls
//! and the results back. Behaviour sources: the PHP manual's OpenSSL
//! chapter, the public specifications cited in each module, the expected
//! output of ext/openssl/tests/*.phpt and observation of PHP 8.5.
//!
//! Cryptographic primitives (hashes, ciphers, RSA/DSA/DH/EC arithmetic,
//! randomness) come from purecrypto; the ASN.1, PEM, key formats, X.509
//! text forms, CSR, PKCS #7 / CMS and PKCS #12 layers are written here.

pub mod certtext;
pub mod cipher;
pub mod cms;
pub mod conf;
pub mod csr;
pub mod der;
pub mod digest;
pub mod err;
pub mod key;
pub mod oid;
pub mod pbe;
pub mod pem;
pub mod pkcs12;
pub mod req;
pub mod spki;
pub mod time;
pub mod v3conf;
pub mod verify;
pub mod x509;

/// Fills `buf` from the operating system's CSPRNG.
pub fn random_bytes(buf: &mut [u8]) {
    use purecrypto::rng::RngCore;
    purecrypto::rng::OsRng.fill_bytes(buf);
}
