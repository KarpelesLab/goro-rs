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

pub mod cipher;
pub mod der;
pub mod digest;
pub mod err;
pub mod oid;
pub mod pem;
pub mod time;

/// Fills `buf` from the operating system's CSPRNG.
pub fn random_bytes(buf: &mut [u8]) {
    use purecrypto::rng::RngCore;
    purecrypto::rng::OsRng.fill_bytes(buf);
}
