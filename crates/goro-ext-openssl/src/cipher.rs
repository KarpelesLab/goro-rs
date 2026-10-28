//! Symmetric ciphers by OpenSSL name: openssl_encrypt()/openssl_decrypt(),
//! openssl_cipher_iv_length()/openssl_cipher_key_length(),
//! openssl_get_cipher_methods(), and the ciphers PEM, PKCS #8, PKCS #7/CMS,
//! PKCS #12 and openssl_seal()/openssl_open() use.
//!
//! The cipher list, the aliases, the key and IV lengths and the error each
//! failure queues were observed on PHP 8.5 (OpenSSL 3.5):
//! `openssl_get_cipher_methods()` with and without aliases, and
//! `openssl_cipher_iv_length()`/`openssl_cipher_key_length()` of each name.
//! The OIDs are the ones of the public registries (NIST CSOR for AES,
//! RFC 3713/NTT for Camellia, KISA for ARIA, GM/T 0006 for SM4, RFC 3217 for
//! the CMS Triple-DES key wrap, IEEE 1619 for XTS).
//!
//! The block ciphers and the GCM, CCM, XTS, GCM-SIV (AES-128/256), SIV,
//! ChaCha20 and Poly1305 primitives come from purecrypto. The modes built
//! here, each from its specification:
//! - ECB, CBC, CFB (full block), CFB-1, CFB-8, OFB and CTR: NIST SP 800-38A;
//! - CBC with ciphertext stealing, variant CS1 (NIST SP 800-38A Addendum;
//!   CS1 is what OpenSSL 3's `*-cbc-cts` produce by default, observed on
//!   PHP 8.5);
//! - OCB: RFC 7253 (`ocb.rs`);
//! - AES key wrap with and without padding, and their inverse-cipher forms:
//!   RFC 3394, RFC 5649, NIST SP 800-38F (`keywrap.rs`);
//! - the CMS Triple-DES key wrap: RFC 3217 (`keywrap.rs`);
//! - AES-GCM-SIV with a 192-bit key, which RFC 8452 leaves out but OpenSSL
//!   lists: RFC 8452's construction with a 24-byte derived encryption key
//!   (`gcmsiv.rs`; checked against PHP 8.5's output);
//! - `aes-*-cbc-hmac-sha*`: OpenSSL's TLS record composite. Without the TLS
//!   AAD/MAC key controls (which openssl_encrypt() never sets) it is plain
//!   CBC without padding (observed on PHP 8.5: the output is the CBC
//!   ciphertext, inputs that are not whole blocks fail).
//!
//! Secret-dependent checks (CBC padding, AEAD tags, key-wrap integrity
//! values) are constant time (purecrypto's `ct` module).

mod gcmsiv;
mod keywrap;
mod ocb;

use purecrypto::cipher::{
    Aes128, Aes192, Aes256, AesSiv, Aria128, Aria192, Aria256, BlockCipher, BlockCipher64,
    Camellia128, Camellia192, Camellia256, ChaCha20, ChaCha20Poly1305, Des, Gcm, Poly1305, Sm4,
    TdesEde2, TdesEde3, Xts,
};
use purecrypto::ct::ConstantTimeEq;

use crate::err::{E, lib};

/// The block cipher (or stream cipher) under a mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algo {
    Aes,
    Aria,
    Camellia,
    Sm4,
    /// Single DES (only reachable through the legacy names).
    Des,
    /// Two-key Triple DES (EDE).
    DesEde,
    /// Three-key Triple DES (EDE3).
    DesEde3,
    ChaCha20,
    Null,
    /// A cipher of OpenSSL's legacy provider that PHP 8.5 knows by name but
    /// cannot run (Blowfish, CAST5, RC2, RC4, SEED, DESX).
    Legacy,
}

/// The mode of operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Ecb,
    Cbc,
    /// CBC with ciphertext stealing (CS1).
    CbcCts,
    /// OpenSSL's TLS CBC+HMAC composite (plain CBC without padding here).
    CbcHmac,
    /// CFB with full-block feedback (CFB-128 / CFB-64).
    Cfb,
    Cfb1,
    Cfb8,
    Ofb,
    Ctr,
    Gcm,
    Ccm,
    Ocb,
    GcmSiv,
    Siv,
    Xts,
    Wrap,
    WrapInv,
    WrapPad,
    WrapPadInv,
    /// RFC 3217 CMS Triple-DES key wrap.
    Des3Wrap,
    /// A stream cipher (ChaCha20) or the null cipher.
    Stream,
    /// ChaCha20-Poly1305 (RFC 8439).
    ChaChaPoly,
}

/// A cipher: its canonical (lower-case) name, algorithm, mode and sizes.
#[derive(Debug, PartialEq, Eq)]
pub struct CipherInfo {
    name: &'static str,
    algo: Algo,
    mode: Mode,
    key_len: u8,
    iv_len: u8,
    oid: Option<&'static str>,
    /// Listed by openssl_get_cipher_methods().
    listed: bool,
}

impl CipherInfo {
    /// The canonical lower-case name (as openssl_get_cipher_methods() lists
    /// it).
    pub fn name(&self) -> &'static str {
        self.name
    }
    pub fn algo(&self) -> Algo {
        self.algo
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }
    /// Key length in bytes (openssl_cipher_key_length()).
    pub fn key_len(&self) -> usize {
        self.key_len as usize
    }
    /// Default IV length in bytes (openssl_cipher_iv_length()).
    pub fn iv_len(&self) -> usize {
        self.iv_len as usize
    }
    /// The underlying block size of the cipher (8 for DES, 16 for the
    /// 128-bit block ciphers; 1 for ChaCha20 and the null cipher).
    pub fn cipher_block_size(&self) -> usize {
        match self.algo {
            Algo::Des | Algo::DesEde | Algo::DesEde3 => 8,
            Algo::ChaCha20 | Algo::Null => 1,
            Algo::Legacy => {
                if self.name.starts_with("seed") {
                    16
                } else if self.name.starts_with("rc4") {
                    1
                } else {
                    8
                }
            }
            _ => 16,
        }
    }
    /// The block size of the mode as OpenSSL reports it: the cipher block
    /// for the padded modes (ECB, CBC), 8 for the key wraps, 1 for the
    /// stream-like modes.
    pub fn block_size(&self) -> usize {
        match self.mode {
            Mode::Ecb | Mode::Cbc | Mode::CbcCts | Mode::CbcHmac => self.cipher_block_size(),
            Mode::Wrap | Mode::WrapInv | Mode::WrapPad | Mode::WrapPadInv | Mode::Des3Wrap => 8,
            _ => 1,
        }
    }
    /// Whether the mode pads (ECB and CBC: PKCS #7 padding unless disabled).
    pub fn pads(&self) -> bool {
        matches!(self.mode, Mode::Ecb | Mode::Cbc)
    }
    /// Whether PHP handles the cipher as AEAD (IV length control, AAD and
    /// tag): GCM, CCM, OCB and ChaCha20-Poly1305. GCM-SIV and SIV are not
    /// (observed on PHP 8.5: no tag is returned for them).
    pub fn is_aead(&self) -> bool {
        matches!(
            self.mode,
            Mode::Gcm | Mode::Ccm | Mode::Ocb | Mode::ChaChaPoly
        )
    }
    /// A legacy-provider cipher: known by name, not usable through
    /// openssl_encrypt() (observed on PHP 8.5: "unsupported").
    pub fn is_legacy(&self) -> bool {
        self.algo == Algo::Legacy || self.algo == Algo::Des
    }
    /// The cipher's OID (dotted form), when it has one.
    pub fn oid(&self) -> Option<&'static str> {
        self.oid
    }

    /// Whether an AEAD IV (nonce) of `len` bytes is accepted (observed on
    /// PHP 8.5: GCM takes any non-empty IV, CCM 7 to 13 bytes, OCB 1 to 15,
    /// ChaCha20-Poly1305 only 12).
    pub fn aead_iv_ok(&self, len: usize) -> bool {
        match self.mode {
            Mode::Gcm => len >= 1,
            Mode::Ccm => (7..=13).contains(&len),
            Mode::Ocb => (1..=15).contains(&len),
            Mode::ChaChaPoly => len == 12,
            _ => len == self.iv_len(),
        }
    }
    /// Whether an AEAD tag of `len` bytes is accepted (observed on PHP 8.5:
    /// CCM takes 4, 6, … 16 (NIST SP 800-38C), the others 1 to 16).
    pub fn tag_len_ok(&self, len: i64) -> bool {
        match self.mode {
            Mode::Ccm => (4..=16).contains(&len) && len % 2 == 0,
            Mode::Gcm | Mode::Ocb | Mode::ChaChaPoly => (1..=16).contains(&len),
            _ => false,
        }
    }
}

const fn c(
    name: &'static str,
    algo: Algo,
    mode: Mode,
    key_len: u8,
    iv_len: u8,
    oid: Option<&'static str>,
) -> CipherInfo {
    CipherInfo {
        name,
        algo,
        mode,
        key_len,
        iv_len,
        oid,
        listed: true,
    }
}

const fn legacy(
    name: &'static str,
    algo: Algo,
    mode: Mode,
    key_len: u8,
    iv_len: u8,
    oid: Option<&'static str>,
) -> CipherInfo {
    CipherInfo {
        name,
        algo,
        mode,
        key_len,
        iv_len,
        oid,
        listed: false,
    }
}

use Algo as A;
use Mode as M;

/// Every cipher, the listed ones in openssl_get_cipher_methods() order
/// (observed on PHP 8.5), then the legacy names.
#[rustfmt::skip]
static CIPHERS: &[CipherInfo] = &[
    c("aes-128-cbc", A::Aes, M::Cbc, 16, 16, Some("2.16.840.1.101.3.4.1.2")),
    c("aes-128-cbc-cts", A::Aes, M::CbcCts, 16, 16, None),
    c("aes-128-cbc-hmac-sha1", A::Aes, M::CbcHmac, 16, 16, None),
    c("aes-128-cbc-hmac-sha256", A::Aes, M::CbcHmac, 16, 16, None),
    c("aes-128-ccm", A::Aes, M::Ccm, 16, 12, Some("2.16.840.1.101.3.4.1.7")),
    c("aes-128-cfb", A::Aes, M::Cfb, 16, 16, Some("2.16.840.1.101.3.4.1.4")),
    c("aes-128-cfb1", A::Aes, M::Cfb1, 16, 16, None),
    c("aes-128-cfb8", A::Aes, M::Cfb8, 16, 16, None),
    c("aes-128-ctr", A::Aes, M::Ctr, 16, 16, None),
    c("aes-128-ecb", A::Aes, M::Ecb, 16, 0, Some("2.16.840.1.101.3.4.1.1")),
    c("aes-128-gcm", A::Aes, M::Gcm, 16, 12, Some("2.16.840.1.101.3.4.1.6")),
    c("aes-128-gcm-siv", A::Aes, M::GcmSiv, 16, 12, None),
    c("aes-128-ocb", A::Aes, M::Ocb, 16, 12, None),
    c("aes-128-ofb", A::Aes, M::Ofb, 16, 16, Some("2.16.840.1.101.3.4.1.3")),
    c("aes-128-siv", A::Aes, M::Siv, 32, 0, None),
    c("aes-128-wrap", A::Aes, M::Wrap, 16, 8, Some("2.16.840.1.101.3.4.1.5")),
    c("aes-128-wrap-inv", A::Aes, M::WrapInv, 16, 8, None),
    c("aes-128-wrap-pad", A::Aes, M::WrapPad, 16, 4, Some("2.16.840.1.101.3.4.1.8")),
    c("aes-128-wrap-pad-inv", A::Aes, M::WrapPadInv, 16, 4, None),
    c("aes-128-xts", A::Aes, M::Xts, 32, 16, Some("1.3.111.2.1619.0.1.1")),
    c("aes-192-cbc", A::Aes, M::Cbc, 24, 16, Some("2.16.840.1.101.3.4.1.22")),
    c("aes-192-cbc-cts", A::Aes, M::CbcCts, 24, 16, None),
    c("aes-192-ccm", A::Aes, M::Ccm, 24, 12, Some("2.16.840.1.101.3.4.1.27")),
    c("aes-192-cfb", A::Aes, M::Cfb, 24, 16, Some("2.16.840.1.101.3.4.1.24")),
    c("aes-192-cfb1", A::Aes, M::Cfb1, 24, 16, None),
    c("aes-192-cfb8", A::Aes, M::Cfb8, 24, 16, None),
    c("aes-192-ctr", A::Aes, M::Ctr, 24, 16, None),
    c("aes-192-ecb", A::Aes, M::Ecb, 24, 0, Some("2.16.840.1.101.3.4.1.21")),
    c("aes-192-gcm", A::Aes, M::Gcm, 24, 12, Some("2.16.840.1.101.3.4.1.26")),
    c("aes-192-gcm-siv", A::Aes, M::GcmSiv, 24, 12, None),
    c("aes-192-ocb", A::Aes, M::Ocb, 24, 12, None),
    c("aes-192-ofb", A::Aes, M::Ofb, 24, 16, Some("2.16.840.1.101.3.4.1.23")),
    c("aes-192-siv", A::Aes, M::Siv, 48, 0, None),
    c("aes-192-wrap", A::Aes, M::Wrap, 24, 8, Some("2.16.840.1.101.3.4.1.25")),
    c("aes-192-wrap-inv", A::Aes, M::WrapInv, 24, 8, None),
    c("aes-192-wrap-pad", A::Aes, M::WrapPad, 24, 4, Some("2.16.840.1.101.3.4.1.28")),
    c("aes-192-wrap-pad-inv", A::Aes, M::WrapPadInv, 24, 4, None),
    c("aes-256-cbc", A::Aes, M::Cbc, 32, 16, Some("2.16.840.1.101.3.4.1.42")),
    c("aes-256-cbc-cts", A::Aes, M::CbcCts, 32, 16, None),
    c("aes-256-cbc-hmac-sha1", A::Aes, M::CbcHmac, 32, 16, None),
    c("aes-256-cbc-hmac-sha256", A::Aes, M::CbcHmac, 32, 16, None),
    c("aes-256-ccm", A::Aes, M::Ccm, 32, 12, Some("2.16.840.1.101.3.4.1.47")),
    c("aes-256-cfb", A::Aes, M::Cfb, 32, 16, Some("2.16.840.1.101.3.4.1.44")),
    c("aes-256-cfb1", A::Aes, M::Cfb1, 32, 16, None),
    c("aes-256-cfb8", A::Aes, M::Cfb8, 32, 16, None),
    c("aes-256-ctr", A::Aes, M::Ctr, 32, 16, None),
    c("aes-256-ecb", A::Aes, M::Ecb, 32, 0, Some("2.16.840.1.101.3.4.1.41")),
    c("aes-256-gcm", A::Aes, M::Gcm, 32, 12, Some("2.16.840.1.101.3.4.1.46")),
    c("aes-256-gcm-siv", A::Aes, M::GcmSiv, 32, 12, None),
    c("aes-256-ocb", A::Aes, M::Ocb, 32, 12, None),
    c("aes-256-ofb", A::Aes, M::Ofb, 32, 16, Some("2.16.840.1.101.3.4.1.43")),
    c("aes-256-siv", A::Aes, M::Siv, 64, 0, None),
    c("aes-256-wrap", A::Aes, M::Wrap, 32, 8, Some("2.16.840.1.101.3.4.1.45")),
    c("aes-256-wrap-inv", A::Aes, M::WrapInv, 32, 8, None),
    c("aes-256-wrap-pad", A::Aes, M::WrapPad, 32, 4, Some("2.16.840.1.101.3.4.1.48")),
    c("aes-256-wrap-pad-inv", A::Aes, M::WrapPadInv, 32, 4, None),
    c("aes-256-xts", A::Aes, M::Xts, 64, 16, Some("1.3.111.2.1619.0.1.2")),
    c("aria-128-cbc", A::Aria, M::Cbc, 16, 16, Some("1.2.410.200046.1.1.2")),
    c("aria-128-ccm", A::Aria, M::Ccm, 16, 12, Some("1.2.410.200046.1.1.37")),
    c("aria-128-cfb", A::Aria, M::Cfb, 16, 16, Some("1.2.410.200046.1.1.3")),
    c("aria-128-cfb1", A::Aria, M::Cfb1, 16, 16, None),
    c("aria-128-cfb8", A::Aria, M::Cfb8, 16, 16, None),
    c("aria-128-ctr", A::Aria, M::Ctr, 16, 16, Some("1.2.410.200046.1.1.5")),
    c("aria-128-ecb", A::Aria, M::Ecb, 16, 0, Some("1.2.410.200046.1.1.1")),
    c("aria-128-gcm", A::Aria, M::Gcm, 16, 12, Some("1.2.410.200046.1.1.34")),
    c("aria-128-ofb", A::Aria, M::Ofb, 16, 16, Some("1.2.410.200046.1.1.4")),
    c("aria-192-cbc", A::Aria, M::Cbc, 24, 16, Some("1.2.410.200046.1.1.7")),
    c("aria-192-ccm", A::Aria, M::Ccm, 24, 12, Some("1.2.410.200046.1.1.38")),
    c("aria-192-cfb", A::Aria, M::Cfb, 24, 16, Some("1.2.410.200046.1.1.8")),
    c("aria-192-cfb1", A::Aria, M::Cfb1, 24, 16, None),
    c("aria-192-cfb8", A::Aria, M::Cfb8, 24, 16, None),
    c("aria-192-ctr", A::Aria, M::Ctr, 24, 16, Some("1.2.410.200046.1.1.10")),
    c("aria-192-ecb", A::Aria, M::Ecb, 24, 0, Some("1.2.410.200046.1.1.6")),
    c("aria-192-gcm", A::Aria, M::Gcm, 24, 12, Some("1.2.410.200046.1.1.35")),
    c("aria-192-ofb", A::Aria, M::Ofb, 24, 16, Some("1.2.410.200046.1.1.9")),
    c("aria-256-cbc", A::Aria, M::Cbc, 32, 16, Some("1.2.410.200046.1.1.12")),
    c("aria-256-ccm", A::Aria, M::Ccm, 32, 12, Some("1.2.410.200046.1.1.39")),
    c("aria-256-cfb", A::Aria, M::Cfb, 32, 16, Some("1.2.410.200046.1.1.13")),
    c("aria-256-cfb1", A::Aria, M::Cfb1, 32, 16, None),
    c("aria-256-cfb8", A::Aria, M::Cfb8, 32, 16, None),
    c("aria-256-ctr", A::Aria, M::Ctr, 32, 16, Some("1.2.410.200046.1.1.15")),
    c("aria-256-ecb", A::Aria, M::Ecb, 32, 0, Some("1.2.410.200046.1.1.11")),
    c("aria-256-gcm", A::Aria, M::Gcm, 32, 12, Some("1.2.410.200046.1.1.36")),
    c("aria-256-ofb", A::Aria, M::Ofb, 32, 16, Some("1.2.410.200046.1.1.14")),
    c("camellia-128-cbc", A::Camellia, M::Cbc, 16, 16, Some("1.2.392.200011.61.1.1.1.2")),
    c("camellia-128-cbc-cts", A::Camellia, M::CbcCts, 16, 16, None),
    c("camellia-128-cfb", A::Camellia, M::Cfb, 16, 16, Some("0.3.4401.5.3.1.9.4")),
    c("camellia-128-cfb1", A::Camellia, M::Cfb1, 16, 16, None),
    c("camellia-128-cfb8", A::Camellia, M::Cfb8, 16, 16, None),
    c("camellia-128-ctr", A::Camellia, M::Ctr, 16, 16, Some("0.3.4401.5.3.1.9.9")),
    c("camellia-128-ecb", A::Camellia, M::Ecb, 16, 0, Some("0.3.4401.5.3.1.9.1")),
    c("camellia-128-ofb", A::Camellia, M::Ofb, 16, 16, Some("0.3.4401.5.3.1.9.3")),
    c("camellia-192-cbc", A::Camellia, M::Cbc, 24, 16, Some("1.2.392.200011.61.1.1.1.3")),
    c("camellia-192-cbc-cts", A::Camellia, M::CbcCts, 24, 16, None),
    c("camellia-192-cfb", A::Camellia, M::Cfb, 24, 16, Some("0.3.4401.5.3.1.9.24")),
    c("camellia-192-cfb1", A::Camellia, M::Cfb1, 24, 16, None),
    c("camellia-192-cfb8", A::Camellia, M::Cfb8, 24, 16, None),
    c("camellia-192-ctr", A::Camellia, M::Ctr, 24, 16, Some("0.3.4401.5.3.1.9.29")),
    c("camellia-192-ecb", A::Camellia, M::Ecb, 24, 0, Some("0.3.4401.5.3.1.9.21")),
    c("camellia-192-ofb", A::Camellia, M::Ofb, 24, 16, Some("0.3.4401.5.3.1.9.23")),
    c("camellia-256-cbc", A::Camellia, M::Cbc, 32, 16, Some("1.2.392.200011.61.1.1.1.4")),
    c("camellia-256-cbc-cts", A::Camellia, M::CbcCts, 32, 16, None),
    c("camellia-256-cfb", A::Camellia, M::Cfb, 32, 16, Some("0.3.4401.5.3.1.9.44")),
    c("camellia-256-cfb1", A::Camellia, M::Cfb1, 32, 16, None),
    c("camellia-256-cfb8", A::Camellia, M::Cfb8, 32, 16, None),
    c("camellia-256-ctr", A::Camellia, M::Ctr, 32, 16, Some("0.3.4401.5.3.1.9.49")),
    c("camellia-256-ecb", A::Camellia, M::Ecb, 32, 0, Some("0.3.4401.5.3.1.9.41")),
    c("camellia-256-ofb", A::Camellia, M::Ofb, 32, 16, Some("0.3.4401.5.3.1.9.43")),
    c("chacha20", A::ChaCha20, M::Stream, 32, 16, None),
    c("chacha20-poly1305", A::ChaCha20, M::ChaChaPoly, 32, 12, None),
    c("des-ede-cbc", A::DesEde, M::Cbc, 16, 8, None),
    c("des-ede-cfb", A::DesEde, M::Cfb, 16, 8, None),
    c("des-ede-ecb", A::DesEde, M::Ecb, 16, 0, Some("1.3.14.3.2.17")),
    c("des-ede-ofb", A::DesEde, M::Ofb, 16, 8, None),
    c("des-ede3-cbc", A::DesEde3, M::Cbc, 24, 8, Some("1.2.840.113549.3.7")),
    c("des-ede3-cfb", A::DesEde3, M::Cfb, 24, 8, None),
    c("des-ede3-cfb1", A::DesEde3, M::Cfb1, 24, 8, None),
    c("des-ede3-cfb8", A::DesEde3, M::Cfb8, 24, 8, None),
    c("des-ede3-ecb", A::DesEde3, M::Ecb, 24, 0, None),
    c("des-ede3-ofb", A::DesEde3, M::Ofb, 24, 8, None),
    c("des3-wrap", A::DesEde3, M::Des3Wrap, 24, 0, Some("1.2.840.113549.1.9.16.3.6")),
    c("null", A::Null, M::Stream, 0, 0, None),
    c("sm4-cbc", A::Sm4, M::Cbc, 16, 16, Some("1.2.156.10197.1.104.2")),
    c("sm4-ccm", A::Sm4, M::Ccm, 16, 12, Some("1.2.156.10197.1.104.9")),
    c("sm4-cfb", A::Sm4, M::Cfb, 16, 16, Some("1.2.156.10197.1.104.4")),
    c("sm4-ctr", A::Sm4, M::Ctr, 16, 16, Some("1.2.156.10197.1.104.7")),
    c("sm4-ecb", A::Sm4, M::Ecb, 16, 0, Some("1.2.156.10197.1.104.1")),
    c("sm4-gcm", A::Sm4, M::Gcm, 16, 12, Some("1.2.156.10197.1.104.8")),
    c("sm4-ofb", A::Sm4, M::Ofb, 16, 16, Some("1.2.156.10197.1.104.3")),
    c("sm4-xts", A::Sm4, M::Xts, 32, 16, Some("1.2.156.10197.1.104.10")),
    // Legacy-provider ciphers: openssl_cipher_iv_length() and
    // openssl_cipher_key_length() answer for them, openssl_encrypt() fails
    // with "unsupported" (observed on PHP 8.5). Single DES is implemented
    // for the formats that use it (PKCS #5 PBES1, PEM "DES-CBC").
    legacy("bf-cbc", A::Legacy, M::Cbc, 16, 8, Some("1.3.6.1.4.1.3029.1.2")),
    legacy("bf-cfb", A::Legacy, M::Cfb, 16, 8, None),
    legacy("bf-ecb", A::Legacy, M::Ecb, 16, 0, None),
    legacy("bf-ofb", A::Legacy, M::Ofb, 16, 8, None),
    legacy("cast5-cbc", A::Legacy, M::Cbc, 16, 8, Some("1.2.840.113533.7.66.10")),
    legacy("cast5-cfb", A::Legacy, M::Cfb, 16, 8, None),
    legacy("cast5-ecb", A::Legacy, M::Ecb, 16, 0, None),
    legacy("cast5-ofb", A::Legacy, M::Ofb, 16, 8, None),
    legacy("des-cbc", A::Des, M::Cbc, 8, 8, Some("1.3.14.3.2.7")),
    legacy("des-cfb", A::Des, M::Cfb, 8, 8, Some("1.3.14.3.2.9")),
    legacy("des-cfb1", A::Des, M::Cfb1, 8, 8, None),
    legacy("des-cfb8", A::Des, M::Cfb8, 8, 8, None),
    legacy("des-ecb", A::Des, M::Ecb, 8, 0, Some("1.3.14.3.2.6")),
    legacy("des-ofb", A::Des, M::Ofb, 8, 8, Some("1.3.14.3.2.8")),
    legacy("desx-cbc", A::Legacy, M::Cbc, 24, 8, None),
    legacy("rc2-40-cbc", A::Legacy, M::Cbc, 5, 8, None),
    legacy("rc2-64-cbc", A::Legacy, M::Cbc, 8, 8, None),
    legacy("rc2-cbc", A::Legacy, M::Cbc, 16, 8, Some("1.2.840.113549.3.2")),
    legacy("rc2-cfb", A::Legacy, M::Cfb, 16, 8, None),
    legacy("rc2-ecb", A::Legacy, M::Ecb, 16, 0, None),
    legacy("rc2-ofb", A::Legacy, M::Ofb, 16, 8, None),
    legacy("rc4", A::Legacy, M::Stream, 16, 0, Some("1.2.840.113549.3.4")),
    legacy("rc4-40", A::Legacy, M::Stream, 5, 0, None),
    legacy("rc4-hmac-md5", A::Legacy, M::Stream, 16, 0, None),
    legacy("seed-cbc", A::Legacy, M::Cbc, 16, 16, Some("1.2.410.200004.1.4")),
    legacy("seed-cfb", A::Legacy, M::Cfb, 16, 16, Some("1.2.410.200004.1.5")),
    legacy("seed-ecb", A::Legacy, M::Ecb, 16, 0, Some("1.2.410.200004.1.3")),
    legacy("seed-ofb", A::Legacy, M::Ofb, 16, 16, Some("1.2.410.200004.1.6")),
];

/// Aliases: those openssl_get_cipher_methods(true) lists (observed on
/// PHP 8.5), then the legacy ones it does not list.
static ALIASES: &[(&str, &str, bool)] = &[
    ("aes128", "aes-128-cbc", true),
    ("aes128-wrap", "aes-128-wrap", true),
    ("aes128-wrap-inv", "aes-128-wrap-inv", true),
    ("aes128-wrap-pad", "aes-128-wrap-pad", true),
    ("aes128-wrap-pad-inv", "aes-128-wrap-pad-inv", true),
    ("aes192", "aes-192-cbc", true),
    ("aes192-wrap", "aes-192-wrap", true),
    ("aes192-wrap-inv", "aes-192-wrap-inv", true),
    ("aes192-wrap-pad", "aes-192-wrap-pad", true),
    ("aes192-wrap-pad-inv", "aes-192-wrap-pad-inv", true),
    ("aes256", "aes-256-cbc", true),
    ("aes256-wrap", "aes-256-wrap", true),
    ("aes256-wrap-inv", "aes-256-wrap-inv", true),
    ("aes256-wrap-pad", "aes-256-wrap-pad", true),
    ("aes256-wrap-pad-inv", "aes-256-wrap-pad-inv", true),
    ("aria128", "aria-128-cbc", true),
    ("aria192", "aria-192-cbc", true),
    ("aria256", "aria-256-cbc", true),
    ("camellia128", "camellia-128-cbc", true),
    ("camellia192", "camellia-192-cbc", true),
    ("camellia256", "camellia-256-cbc", true),
    ("des-ede", "des-ede-ecb", true),
    ("des-ede3", "des-ede3-ecb", true),
    ("des3", "des-ede3-cbc", true),
    ("id-aes128-ccm", "aes-128-ccm", true),
    ("id-aes128-gcm", "aes-128-gcm", true),
    ("id-aes128-wrap", "aes-128-wrap", true),
    ("id-aes128-wrap-pad", "aes-128-wrap-pad", true),
    ("id-aes192-ccm", "aes-192-ccm", true),
    ("id-aes192-gcm", "aes-192-gcm", true),
    ("id-aes192-wrap", "aes-192-wrap", true),
    ("id-aes192-wrap-pad", "aes-192-wrap-pad", true),
    ("id-aes256-ccm", "aes-256-ccm", true),
    ("id-aes256-gcm", "aes-256-gcm", true),
    ("id-aes256-wrap", "aes-256-wrap", true),
    ("id-aes256-wrap-pad", "aes-256-wrap-pad", true),
    ("id-smime-alg-cms3deswrap", "des3-wrap", true),
    ("sm4", "sm4-cbc", true),
    ("sm4-cfb128", "sm4-cfb", true),
    ("sm4-ofb128", "sm4-ofb", true),
    ("bf", "bf-cbc", false),
    ("blowfish", "bf-cbc", false),
    ("cast", "cast5-cbc", false),
    ("cast-cbc", "cast5-cbc", false),
    ("des", "des-cbc", false),
    ("desx", "desx-cbc", false),
    ("rc2", "rc2-cbc", false),
    ("seed", "seed-cbc", false),
];

fn by_canonical(name: &str) -> Option<&'static CipherInfo> {
    CIPHERS.iter().find(|c| c.name == name)
}

/// Looks a cipher up by name, alias or OID, as OpenSSL does: ASCII case is
/// ignored; the name ends at a NUL byte and trailing spaces are dropped
/// (observed on PHP 8.5: "aes-128-cbc  " and "aes-128-cbc\0x" are
/// accepted, leading spaces, tabs and newlines are not).
pub fn lookup(name: &str) -> Option<&'static CipherInfo> {
    lookup_bytes(name.as_bytes())
}

/// [`lookup`] on raw bytes.
pub fn lookup_bytes(name: &[u8]) -> Option<&'static CipherInfo> {
    let name = match name.iter().position(|&b| b == 0) {
        Some(p) => &name[..p],
        None => name,
    };
    let mut end = name.len();
    while end > 0 && name[end - 1] == b' ' {
        end -= 1;
    }
    let name = &name[..end];
    if name.is_empty() || name.len() > 64 {
        return None;
    }
    let l = String::from_utf8(name.to_ascii_lowercase()).ok()?;
    if let Some(c) = by_canonical(&l) {
        return Some(c);
    }
    if let Some((_, t, _)) = ALIASES.iter().find(|(a, _, _)| *a == l) {
        return by_canonical(t);
    }
    by_oid(&l)
}

/// Looks a cipher up by its OID (dotted form).
pub fn by_oid(oid: &str) -> Option<&'static CipherInfo> {
    CIPHERS.iter().find(|c| c.oid == Some(oid))
}

/// openssl_get_cipher_methods(): the canonical names, and with `aliases`
/// also the listed aliases and OIDs, sorted bytewise (observed on PHP 8.5).
pub fn methods(aliases: bool) -> Vec<&'static str> {
    let mut v: Vec<&'static str> = CIPHERS
        .iter()
        .filter(|c| c.listed)
        .map(|c| c.name)
        .collect();
    if aliases {
        v.extend(ALIASES.iter().filter(|a| a.2).map(|a| a.0));
        v.extend(CIPHERS.iter().filter(|c| c.listed).filter_map(|c| c.oid));
        v.sort_unstable();
    }
    v
}

/// Why a cipher operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CipherError {
    /// The cipher cannot run here (legacy provider).
    Unsupported,
    /// The key is not the cipher's key length.
    InvalidKeyLength,
    /// The IV is not a length the cipher accepts.
    InvalidIvLength,
    /// The tag length is not one the mode accepts.
    InvalidTagLength,
    /// Without padding, the input is not a whole number of blocks; or the
    /// ciphertext to decrypt is not.
    WrongFinalBlockLength,
    /// The decrypted padding is malformed.
    BadDecrypt,
    /// The mode rejects the input (XTS shorter than a block, CBC-HMAC not
    /// whole blocks, CCM tag mismatch, CMS key wrap failure).
    OperationFailed,
    /// Key wrap: the input length is not a multiple of 8.
    KwInvalidLength,
    /// Key wrap: the input is too short, or the integrity check failed.
    KwFailed,
    /// XTS with both halves of the key equal (refused when encrypting).
    XtsDuplicatedKeys,
    /// The AEAD tag did not verify (GCM, OCB, ChaCha20-Poly1305; queues no
    /// error, observed on PHP 8.5).
    TagMismatch,
    /// A failure that queues no error (CBC-CTS shorter than a block; SIV and
    /// GCM-SIV decryption, which PHP never gives a tag).
    Silent,
}

/// Errors queued for openssl_error_string() (codes observed on PHP 8.5).
pub mod errs {
    use super::{E, lib};
    pub const UNSUPPORTED: E = E::new(0x0308010C, lib::EVP, "unsupported");
    pub const INITIALIZATION_ERROR: E = E::new(0x03000086, lib::EVP, "initialization error");
    pub const INVALID_KEY_LENGTH: E = E::new(0x03000082, lib::EVP, "invalid key length");
    pub const UPDATE_ERROR: E = E::new(0x030000BD, lib::EVP, "update error");
    pub const BAD_DECRYPT: E = E::new(0x1C800064, lib::PROV, "bad decrypt");
    pub const WRONG_FINAL_BLOCK_LENGTH: E =
        E::new(0x1C80006B, lib::PROV, "wrong final block length");
    pub const CIPHER_OPERATION_FAILED: E = E::new(0x1C800066, lib::PROV, "cipher operation failed");
    pub const INVALID_INPUT_LENGTH: E = E::new(0x1C8000E6, lib::PROV, "invalid input length");
    pub const XTS_DUPLICATED_KEYS: E = E::new(0x1C800095, lib::PROV, "xts duplicated keys");
    pub const INVALID_TAG: E = E::new(0x1C80006E, lib::PROV, "invalid tag");
    pub const INVALID_TAG_LENGTH: E = E::new(0x1C800076, lib::PROV, "invalid tag length");
    pub const INVALID_IV_LENGTH: E = E::new(0x1C80006D, lib::PROV, "invalid iv length");
    pub const FAILED_TO_GET_PARAMETER: E = E::new(0x1C800067, lib::PROV, "failed to get parameter");
    pub const INVALID_ITERATION_COUNT: E = E::new(0x1C80007B, lib::PROV, "invalid iteration count");
    pub const XOF_DIGESTS_NOT_ALLOWED: E = E::new(0x1C8000B7, lib::PROV, "xof digests not allowed");
    pub const TOO_SMALL_BUFFER: E = E::new(0x07800074, COMMON, "too small buffer");
    pub const PARAM_NEGATIVE: E = E::new(
        0x0780007D,
        COMMON,
        "param unsigned integer negative value unsupported",
    );
    pub const COMMON: &str = "common libcrypto routines";
}

impl CipherError {
    /// The errors the failure queues (observed on PHP 8.5).
    pub fn errors(self) -> &'static [E] {
        use errs::*;
        match self {
            CipherError::Unsupported => &[UNSUPPORTED],
            CipherError::InvalidKeyLength => &[INVALID_KEY_LENGTH],
            CipherError::InvalidIvLength => &[INVALID_IV_LENGTH],
            CipherError::InvalidTagLength => &[INVALID_TAG_LENGTH],
            CipherError::WrongFinalBlockLength => &[WRONG_FINAL_BLOCK_LENGTH],
            CipherError::BadDecrypt => &[BAD_DECRYPT],
            CipherError::OperationFailed => &[CIPHER_OPERATION_FAILED],
            CipherError::KwInvalidLength => &[INVALID_INPUT_LENGTH, UPDATE_ERROR],
            CipherError::KwFailed => &[CIPHER_OPERATION_FAILED, UPDATE_ERROR],
            CipherError::XtsDuplicatedKeys => &[XTS_DUPLICATED_KEYS],
            CipherError::TagMismatch | CipherError::Silent => &[],
        }
    }
    /// Queues [`errors`](Self::errors).
    pub fn push(self) {
        for e in self.errors() {
            crate::err::push(*e);
        }
    }
}

impl std::fmt::Display for CipherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.errors().first() {
            Some(e) => f.write_str(e.reason),
            None => f.write_str("cipher operation failed"),
        }
    }
}

impl std::error::Error for CipherError {}

// ---------------------------------------------------------------------------
// Block cipher abstraction over 64- and 128-bit blocks.

/// A block cipher with an `N`-byte block (8 or 16).
pub(crate) trait Bc {
    const N: usize;
    fn e(&self, b: &mut [u8]);
    fn d(&self, b: &mut [u8]);
    /// Encrypts consecutive independent blocks (the hardware-pipelined
    /// path for AES).
    #[allow(clippy::chunks_exact_to_as_chunks)] // N is generic
    fn e_many(&self, b: &mut [u8]) {
        for c in b.chunks_exact_mut(Self::N) {
            self.e(c);
        }
    }
    #[allow(clippy::chunks_exact_to_as_chunks)]
    fn d_many(&self, b: &mut [u8]) {
        for c in b.chunks_exact_mut(Self::N) {
            self.d(c);
        }
    }
}

/// A 128-bit purecrypto block cipher.
pub(crate) struct W16<C>(pub C);

impl<C: BlockCipher> Bc for W16<C> {
    const N: usize = 16;
    #[inline]
    fn e(&self, b: &mut [u8]) {
        let b: &mut [u8; 16] = b.try_into().expect("16-byte block");
        self.0.encrypt_block(b);
    }
    #[inline]
    fn d(&self, b: &mut [u8]) {
        let b: &mut [u8; 16] = b.try_into().expect("16-byte block");
        self.0.decrypt_block(b);
    }
    #[inline]
    fn e_many(&self, b: &mut [u8]) {
        self.0.encrypt_blocks(b);
    }
    #[inline]
    fn d_many(&self, b: &mut [u8]) {
        self.0.decrypt_blocks(b);
    }
}

/// A 64-bit purecrypto block cipher.
pub(crate) struct W8<C>(pub C);

impl<C: BlockCipher64> Bc for W8<C> {
    const N: usize = 8;
    #[inline]
    fn e(&self, b: &mut [u8]) {
        let b: &mut [u8; 8] = b.try_into().expect("8-byte block");
        self.0.encrypt_block(b);
    }
    #[inline]
    fn d(&self, b: &mut [u8]) {
        let b: &mut [u8; 8] = b.try_into().expect("8-byte block");
        self.0.decrypt_block(b);
    }
}

fn arr<const L: usize>(k: &[u8]) -> &[u8; L] {
    k.try_into().expect("key length checked")
}

/// Runs `$body` with `$c` bound to the 128-bit block cipher (raw purecrypto
/// type) of `$algo` keyed with `$key` (whose length was checked).
macro_rules! with16 {
    ($algo:expr, $key:expr, $c:ident => $body:expr) => {{
        let key: &[u8] = $key;
        match ($algo, key.len()) {
            (Algo::Aes, 16) => {
                let $c = Aes128::new(arr(key));
                $body
            }
            (Algo::Aes, 24) => {
                let $c = Aes192::new(arr(key));
                $body
            }
            (Algo::Aes, 32) => {
                let $c = Aes256::new(arr(key));
                $body
            }
            (Algo::Aria, 16) => {
                let $c = Aria128::new(arr(key));
                $body
            }
            (Algo::Aria, 24) => {
                let $c = Aria192::new(arr(key));
                $body
            }
            (Algo::Aria, 32) => {
                let $c = Aria256::new(arr(key));
                $body
            }
            (Algo::Camellia, 16) => {
                let $c = Camellia128::new(arr(key));
                $body
            }
            (Algo::Camellia, 24) => {
                let $c = Camellia192::new(arr(key));
                $body
            }
            (Algo::Camellia, 32) => {
                let $c = Camellia256::new(arr(key));
                $body
            }
            (Algo::Sm4, 16) => {
                let $c = Sm4::new(arr(key));
                $body
            }
            _ => Err(CipherError::Unsupported),
        }
    }};
}

/// Runs `$body` with `$c` bound to a [`Bc`] for `$algo` (64- or 128-bit).
macro_rules! with_bc {
    ($algo:expr, $key:expr, $c:ident => $body:expr) => {{
        let key: &[u8] = $key;
        match $algo {
            Algo::Des if key.len() == 8 => {
                let $c = W8(Des::new(arr(key)));
                $body
            }
            Algo::DesEde if key.len() == 16 => {
                let $c = W8(TdesEde2::new(arr(key)));
                $body
            }
            Algo::DesEde3 if key.len() == 24 => {
                let $c = W8(TdesEde3::new(arr(key)));
                $body
            }
            a => with16!(a, key, raw => {
                let $c = W16(raw);
                $body
            }),
        }
    }};
}

// ---------------------------------------------------------------------------
// Padding (PKCS #7, RFC 5652 §6.3).

fn pad(out: &mut Vec<u8>, n: usize) {
    let p = n - out.len() % n;
    out.resize(out.len() + p, p as u8);
}

/// Removes PKCS #7 padding in constant time over the last block; `None`
/// when malformed.
fn unpad(buf: &[u8], n: usize) -> Option<usize> {
    use purecrypto::ct::{ConstantTimeGreater, ConstantTimeLess};
    let len = buf.len();
    let last = buf[len - 1] as u32;
    let mut bad = last.ct_eq(&0) | last.ct_gt(&(n as u32));
    for i in 0..n {
        let b = buf[len - 1 - i] as u32;
        bad |= (i as u32).ct_lt(&last) & !b.ct_eq(&last);
    }
    // The verdict and the plaintext length are public outcomes.
    if bool::from(bad) {
        None
    } else {
        Some(len - last as usize)
    }
}

// ---------------------------------------------------------------------------
// NIST SP 800-38A modes over a [`Bc`].

fn ecb_enc<B: Bc>(c: &B, buf: &mut [u8]) {
    c.e_many(buf);
}

fn ecb_dec<B: Bc>(c: &B, buf: &mut [u8]) {
    c.d_many(buf);
}

fn cbc_enc<B: Bc>(c: &B, iv: &[u8], buf: &mut [u8]) {
    let n = B::N;
    let mut chain = [0u8; 16];
    chain[..n].copy_from_slice(iv);
    for blk in buf.chunks_exact_mut(n) {
        for (b, x) in blk.iter_mut().zip(chain.iter()) {
            *b ^= *x;
        }
        c.e(blk);
        chain[..n].copy_from_slice(blk);
    }
}

fn cbc_dec<B: Bc>(c: &B, iv: &[u8], buf: &mut [u8]) {
    // Block decryptions are independent: a window goes through the batched
    // path, then each block is XORed with the ciphertext block before it.
    const W: usize = 64;
    let n = B::N;
    let mut prev = [0u8; 16 * W];
    let mut chain = [0u8; 16];
    chain[..n].copy_from_slice(iv);
    for win in buf.chunks_mut(n * W) {
        let l = win.len();
        prev[..n].copy_from_slice(&chain[..n]);
        prev[n..l].copy_from_slice(&win[..l - n]);
        chain[..n].copy_from_slice(&win[l - n..]);
        c.d_many(win);
        for (b, p) in win.iter_mut().zip(prev.iter()) {
            *b ^= *p;
        }
    }
}

/// CBC-CS1 encryption (NIST SP 800-38A Addendum): CBC over the zero-padded
/// input, with the next-to-last ciphertext block truncated to the length of
/// the last partial block.
fn cts_enc<B: Bc>(c: &B, iv: &[u8], data: &[u8]) -> Result<Vec<u8>, CipherError> {
    let n = B::N;
    if data.len() < n {
        return Err(CipherError::Silent);
    }
    let d = data.len() % n;
    let mut out = data.to_vec();
    if d == 0 {
        cbc_enc(c, iv, &mut out);
        return Ok(out);
    }
    out.resize(data.len() - d + n, 0);
    cbc_enc(c, iv, &mut out);
    // out = C1 .. C(m-1) C(m): drop the tail of C(m-1).
    let m = out.len() / n;
    let last: Vec<u8> = out[(m - 1) * n..].to_vec();
    out.truncate((m - 2) * n + d);
    out.extend_from_slice(&last);
    Ok(out)
}

fn cts_dec<B: Bc>(c: &B, iv: &[u8], data: &[u8]) -> Result<Vec<u8>, CipherError> {
    let n = B::N;
    if data.len() < n {
        return Err(CipherError::Silent);
    }
    let d = data.len() % n;
    let mut out = data.to_vec();
    if d == 0 {
        cbc_dec(c, iv, &mut out);
        return Ok(out);
    }
    // Layout: full blocks, C(m-1)* (d bytes), C(m) (n bytes).
    let full = data.len() - d - n; // bytes before C(m-1)*
    let cm = &data[full + d..];
    let mut z = [0u8; 16];
    z[..n].copy_from_slice(cm);
    c.d(&mut z[..n]); // Z = C(m-1) xor (P(m) || 0)
    let mut cm1 = [0u8; 16];
    cm1[..d].copy_from_slice(&data[full..full + d]);
    cm1[d..n].copy_from_slice(&z[d..n]);
    let mut pm = [0u8; 16];
    for i in 0..d {
        pm[i] = z[i] ^ cm1[i];
    }
    // Decrypt the full blocks and C(m-1) as ordinary CBC.
    let mut body = data[..full].to_vec();
    body.extend_from_slice(&cm1[..n]);
    cbc_dec(c, iv, &mut body);
    out.clear();
    out.extend_from_slice(&body);
    out.extend_from_slice(&pm[..d]);
    Ok(out)
}

fn cfb_enc<B: Bc>(c: &B, iv: &[u8], buf: &mut [u8], decrypt: bool) {
    let n = B::N;
    let mut reg = [0u8; 16];
    reg[..n].copy_from_slice(iv);
    for blk in buf.chunks_mut(n) {
        let mut ks = reg;
        c.e(&mut ks[..n]);
        for (i, b) in blk.iter_mut().enumerate() {
            let ct = if decrypt { *b } else { *b ^ ks[i] };
            *b ^= ks[i];
            reg[i] = ct;
        }
    }
}

/// CFB-8: one byte per block encryption.
fn cfb8<B: Bc>(c: &B, iv: &[u8], buf: &mut [u8], decrypt: bool) {
    let n = B::N;
    let mut reg = [0u8; 16];
    reg[..n].copy_from_slice(iv);
    for b in buf.iter_mut() {
        let mut ks = reg;
        c.e(&mut ks[..n]);
        let ct = if decrypt { *b } else { *b ^ ks[0] };
        *b ^= ks[0];
        reg.copy_within(1..n, 0);
        reg[n - 1] = ct;
    }
}

/// CFB-1: one bit per block encryption, most significant bit first.
fn cfb1<B: Bc>(c: &B, iv: &[u8], buf: &mut [u8], decrypt: bool) {
    let n = B::N;
    let mut reg = [0u8; 16];
    reg[..n].copy_from_slice(iv);
    for b in buf.iter_mut() {
        let mut outb = 0u8;
        for bit in (0..8).rev() {
            let mut ks = reg;
            c.e(&mut ks[..n]);
            let inb = (*b >> bit) & 1;
            let ob = inb ^ (ks[0] >> 7);
            let ctb = if decrypt { inb } else { ob };
            outb |= ob << bit;
            // Shift the register left one bit, feeding in the ciphertext bit.
            for i in 0..n {
                let next = if i + 1 < n { reg[i + 1] >> 7 } else { ctb };
                reg[i] = (reg[i] << 1) | next;
            }
        }
        *b = outb;
    }
}

fn ofb<B: Bc>(c: &B, iv: &[u8], buf: &mut [u8]) {
    let n = B::N;
    let mut reg = [0u8; 16];
    reg[..n].copy_from_slice(iv);
    for blk in buf.chunks_mut(n) {
        c.e(&mut reg[..n]);
        for (b, k) in blk.iter_mut().zip(reg.iter()) {
            *b ^= *k;
        }
    }
}

/// CTR with the whole 128-bit counter block incremented big-endian
/// (NIST SP 800-38A B.1 with m = 128; observed on PHP 8.5: the counter
/// wraps from all-ones to zero).
fn ctr<B: Bc>(c: &B, iv: &[u8], buf: &mut [u8]) {
    const W: usize = 64;
    let mut ctr = u128::from_be_bytes(iv.try_into().expect("16-byte counter"));
    let mut ks = [0u8; 16 * W];
    for win in buf.chunks_mut(16 * W) {
        let nb = win.len().div_ceil(16);
        for blk in ks[..nb * 16].as_chunks_mut::<16>().0 {
            blk.copy_from_slice(&ctr.to_be_bytes());
            ctr = ctr.wrapping_add(1);
        }
        c.e_many(&mut ks[..nb * 16]);
        for (b, k) in win.iter_mut().zip(ks.iter()) {
            *b ^= *k;
        }
    }
    purecrypto::zeroize::Zeroize::zeroize(&mut ks[..]);
}

// ---------------------------------------------------------------------------
// One-shot entry points.

fn check_sizes(info: &CipherInfo, key: &[u8], iv: &[u8]) -> Result<(), CipherError> {
    if key.len() != info.key_len() {
        return Err(CipherError::InvalidKeyLength);
    }
    if !info.is_aead() && iv.len() != info.iv_len() {
        return Err(CipherError::InvalidIvLength);
    }
    if info.algo == Algo::Legacy {
        return Err(CipherError::Unsupported);
    }
    Ok(())
}

/// Encrypts with a non-AEAD cipher. `key` and `iv` must have the cipher's
/// lengths. `padding` selects PKCS #7 padding for ECB and CBC; without it
/// the input must be whole blocks.
pub fn encrypt(
    info: &CipherInfo,
    key: &[u8],
    iv: &[u8],
    data: &[u8],
    padding: bool,
) -> Result<Vec<u8>, CipherError> {
    check_sizes(info, key, iv)?;
    if info.is_aead() {
        return aead_encrypt(info, key, iv, b"", data, 16).map(|(c, _)| c);
    }
    match info.mode {
        Mode::Stream => stream(info, key, iv, data),
        Mode::GcmSiv => gcmsiv::encrypt(info.algo, key, iv, data),
        Mode::Siv => siv_encrypt(key, data),
        Mode::Xts => xts(info.algo, key, iv, data, true),
        Mode::Wrap | Mode::WrapInv | Mode::WrapPad | Mode::WrapPadInv => {
            with16!(info.algo, key, raw => keywrap::wrap(&W16(raw), info.mode, iv, data))
        }
        Mode::Des3Wrap => keywrap::des3_wrap(&W8(TdesEde3::new(arr(key))), data),
        _ => with_bc!(info.algo, key, c => block_mode(&c, info, iv, data, padding, false)),
    }
}

/// Decrypts with a non-AEAD cipher (see [`encrypt`]).
pub fn decrypt(
    info: &CipherInfo,
    key: &[u8],
    iv: &[u8],
    data: &[u8],
    padding: bool,
) -> Result<Vec<u8>, CipherError> {
    check_sizes(info, key, iv)?;
    if info.is_aead() {
        return Err(CipherError::TagMismatch);
    }
    match info.mode {
        Mode::Stream => stream(info, key, iv, data),
        // Without a tag (PHP never passes one for these) nothing verifies.
        Mode::GcmSiv | Mode::Siv => Err(CipherError::Silent),
        Mode::Xts => xts(info.algo, key, iv, data, false),
        Mode::Wrap | Mode::WrapInv | Mode::WrapPad | Mode::WrapPadInv => {
            with16!(info.algo, key, raw => keywrap::unwrap(&W16(raw), info.mode, iv, data))
        }
        Mode::Des3Wrap => keywrap::des3_unwrap(&W8(TdesEde3::new(arr(key))), data),
        _ => with_bc!(info.algo, key, c => block_mode(&c, info, iv, data, padding, true)),
    }
}

fn block_mode<B: Bc>(
    c: &B,
    info: &CipherInfo,
    iv: &[u8],
    data: &[u8],
    padding: bool,
    decrypt: bool,
) -> Result<Vec<u8>, CipherError> {
    let n = B::N;
    match info.mode {
        Mode::Ecb | Mode::Cbc => {
            let cbc = info.mode == Mode::Cbc;
            if decrypt {
                if !data.len().is_multiple_of(n) || (padding && data.is_empty()) {
                    return Err(CipherError::WrongFinalBlockLength);
                }
                let mut out = data.to_vec();
                if cbc {
                    cbc_dec(c, iv, &mut out);
                } else {
                    ecb_dec(c, &mut out);
                }
                if padding {
                    match unpad(&out, n) {
                        Some(l) => out.truncate(l),
                        None => {
                            purecrypto::zeroize::Zeroize::zeroize(&mut out[..]);
                            return Err(CipherError::BadDecrypt);
                        }
                    }
                }
                Ok(out)
            } else {
                let mut out = Vec::with_capacity(data.len() + n);
                out.extend_from_slice(data);
                if padding {
                    pad(&mut out, n);
                } else if !data.len().is_multiple_of(n) {
                    return Err(CipherError::WrongFinalBlockLength);
                }
                if cbc {
                    cbc_enc(c, iv, &mut out);
                } else {
                    ecb_enc(c, &mut out);
                }
                Ok(out)
            }
        }
        Mode::CbcHmac => {
            if !data.len().is_multiple_of(n) {
                return Err(CipherError::OperationFailed);
            }
            let mut out = data.to_vec();
            if decrypt {
                cbc_dec(c, iv, &mut out);
            } else {
                cbc_enc(c, iv, &mut out);
            }
            Ok(out)
        }
        Mode::CbcCts => {
            if decrypt {
                cts_dec(c, iv, data)
            } else {
                cts_enc(c, iv, data)
            }
        }
        Mode::Cfb => {
            let mut out = data.to_vec();
            cfb_enc(c, iv, &mut out, decrypt);
            Ok(out)
        }
        Mode::Cfb8 => {
            let mut out = data.to_vec();
            cfb8(c, iv, &mut out, decrypt);
            Ok(out)
        }
        Mode::Cfb1 => {
            let mut out = data.to_vec();
            cfb1(c, iv, &mut out, decrypt);
            Ok(out)
        }
        Mode::Ofb => {
            let mut out = data.to_vec();
            ofb(c, iv, &mut out);
            Ok(out)
        }
        Mode::Ctr if n == 16 => {
            let mut out = data.to_vec();
            ctr(c, iv, &mut out);
            Ok(out)
        }
        _ => Err(CipherError::Unsupported),
    }
}

/// ChaCha20 (OpenSSL's IV: a 32-bit little-endian block counter then the
/// 96-bit nonce, observed on PHP 8.5) and the null cipher.
fn stream(info: &CipherInfo, key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>, CipherError> {
    let mut out = data.to_vec();
    stream_in_place(info, key, iv, &mut out);
    Ok(out)
}

fn stream_in_place(info: &CipherInfo, key: &[u8], iv: &[u8], out: &mut [u8]) {
    if info.algo == Algo::Null {
        return;
    }
    let cc = ChaCha20::new(arr(key));
    let mut counter = u32::from_le_bytes(iv[..4].try_into().expect("4 bytes"));
    let mut nonce: [u8; 12] = iv[4..16].try_into().expect("12 bytes");
    // The counter carries into the nonce's first word when it wraps
    // (OpenSSL treats the first 64 bits as the counter; observed on
    // PHP 8.5).
    let mut rest = out;
    while !rest.is_empty() {
        let blocks_left = (u32::MAX - counter) as u64 + 1;
        let take = (blocks_left * 64).min(rest.len() as u64) as usize;
        let (now, later) = rest.split_at_mut(take);
        cc.apply_keystream(&nonce, counter, now);
        rest = later;
        counter = 0;
        let hi = u32::from_le_bytes(nonce[..4].try_into().expect("4 bytes")).wrapping_add(1);
        nonce[..4].copy_from_slice(&hi.to_le_bytes());
    }
}

fn siv_encrypt(key: &[u8], data: &[u8]) -> Result<Vec<u8>, CipherError> {
    // RFC 5297 S2V over the plaintext alone (openssl_encrypt() passes no
    // associated data); PHP returns the ciphertext without the synthetic IV
    // (observed on PHP 8.5).
    let siv = AesSiv::try_new(key).map_err(|_| CipherError::InvalidKeyLength)?;
    let mut out = siv.seal(&[], data);
    out.drain(..16);
    Ok(out)
}

/// XTS (IEEE 1619): the IV is the 128-bit tweak, little-endian as in the
/// standard's data-unit number. Encryption refuses equal key halves
/// (observed on PHP 8.5: "xts duplicated keys"; decryption accepts them).
fn xts(algo: Algo, key: &[u8], iv: &[u8], data: &[u8], enc: bool) -> Result<Vec<u8>, CipherError> {
    let mut out = data.to_vec();
    xts_in_place(algo, key, iv, &mut out, enc)?;
    Ok(out)
}

fn xts_in_place(
    algo: Algo,
    key: &[u8],
    iv: &[u8],
    out: &mut [u8],
    enc: bool,
) -> Result<(), CipherError> {
    if out.len() < 16 {
        return Err(CipherError::OperationFailed);
    }
    let (k1, k2) = key.split_at(key.len() / 2);
    if enc && bool::from(k1.ct_eq(k2)) {
        return Err(CipherError::XtsDuplicatedKeys);
    }
    let tweak = u128::from_le_bytes(iv.try_into().map_err(|_| CipherError::InvalidIvLength)?);
    macro_rules! go {
        ($t:ty) => {{
            let x = Xts::new(<$t>::new(arr(k1)), <$t>::new(arr(k2)));
            let r = if enc {
                x.encrypt_sector(tweak, out)
            } else {
                x.decrypt_sector(tweak, out)
            };
            r.map_err(|_| CipherError::OperationFailed)?;
        }};
    }
    match (algo, k1.len()) {
        (Algo::Aes, 16) => go!(Aes128),
        (Algo::Aes, 32) => go!(Aes256),
        (Algo::Sm4, 16) => {
            let c1 = W16(Sm4::new(arr(k1)));
            let c2 = W16(Sm4::new(arr(k2)));
            xts_gb(&c1, &c2, iv, out, enc);
        }
        _ => return Err(CipherError::Unsupported),
    }
    Ok(())
}

/// Whether [`crypt_in_place`] handles the cipher: the non-AEAD modes whose
/// output has the input's length (stream-like modes, XTS, CBC-HMAC, and
/// ECB/CBC without padding).
pub fn in_place_ok(info: &CipherInfo, padding: bool) -> bool {
    !info.is_legacy()
        && match info.mode {
            Mode::Stream
            | Mode::Cfb
            | Mode::Cfb1
            | Mode::Cfb8
            | Mode::Ofb
            | Mode::Ctr
            | Mode::CbcHmac
            | Mode::Xts => true,
            Mode::Ecb | Mode::Cbc => !padding,
            _ => false,
        }
}

/// The PKCS #7 padding to append to `len` bytes for an ECB/CBC cipher
/// (for encrypting a padded buffer with [`crypt_in_place`]).
pub fn padding_for(info: &CipherInfo, len: usize) -> Vec<u8> {
    let n = info.cipher_block_size();
    let p = n - len % n;
    vec![p as u8; p]
}

/// The plaintext length of a decrypted, PKCS #7-padded buffer (checked in
/// constant time), or the error decryption reports: a buffer that is empty
/// or not whole blocks is [`CipherError::WrongFinalBlockLength`], malformed
/// padding [`CipherError::BadDecrypt`].
pub fn unpadded_len(info: &CipherInfo, buf: &[u8]) -> Result<usize, CipherError> {
    let n = info.cipher_block_size();
    if buf.is_empty() || !buf.len().is_multiple_of(n) {
        return Err(CipherError::WrongFinalBlockLength);
    }
    unpad(buf, n).ok_or(CipherError::BadDecrypt)
}

/// Encrypts or decrypts `buf` in place with a cipher [`in_place_ok`]
/// accepts (saves a copy of large inputs; same results and errors as
/// [`encrypt`]/[`decrypt`]). On error the buffer's content is unspecified.
pub fn crypt_in_place(
    info: &CipherInfo,
    key: &[u8],
    iv: &[u8],
    buf: &mut [u8],
    decrypt: bool,
) -> Result<(), CipherError> {
    check_sizes(info, key, iv)?;
    if !in_place_ok(info, false) {
        return Err(CipherError::Unsupported);
    }
    match info.mode {
        Mode::Stream => {
            stream_in_place(info, key, iv, buf);
            Ok(())
        }
        Mode::Xts => xts_in_place(info.algo, key, iv, buf, !decrypt),
        _ => with_bc!(info.algo, key, c => in_place_mode(&c, info.mode, iv, buf, decrypt)),
    }
}

fn in_place_mode<B: Bc>(
    c: &B,
    mode: Mode,
    iv: &[u8],
    buf: &mut [u8],
    decrypt: bool,
) -> Result<(), CipherError> {
    let n = B::N;
    match mode {
        Mode::Ecb | Mode::Cbc | Mode::CbcHmac => {
            if !buf.len().is_multiple_of(n) {
                return Err(if mode == Mode::CbcHmac {
                    CipherError::OperationFailed
                } else {
                    CipherError::WrongFinalBlockLength
                });
            }
            match (mode == Mode::Ecb, decrypt) {
                (true, false) => ecb_enc(c, buf),
                (true, true) => ecb_dec(c, buf),
                (false, false) => cbc_enc(c, iv, buf),
                (false, true) => cbc_dec(c, iv, buf),
            }
        }
        Mode::Cfb => cfb_enc(c, iv, buf, decrypt),
        Mode::Cfb8 => cfb8(c, iv, buf, decrypt),
        Mode::Cfb1 => cfb1(c, iv, buf, decrypt),
        Mode::Ofb => ofb(c, iv, buf),
        Mode::Ctr if n == 16 => ctr(c, iv, buf),
        _ => return Err(CipherError::Unsupported),
    }
    Ok(())
}

/// XTS with the GB/T 17964-2021 tweak update, which OpenSSL's `sm4-xts`
/// uses by default (OpenSSL documentation: EVP_CIPHER-SM4, the
/// "xts_standard" parameter defaults to "GB"): the tweak is multiplied by
/// α in the bit-reflected representation (a right shift of the big-endian
/// value, folding 0xE1 into the top byte), checked against PHP 8.5's output.
/// Ciphertext stealing as in IEEE 1619 §5.3.2.
fn xts_gb<B: Bc>(c1: &B, c2: &B, iv: &[u8], buf: &mut [u8], enc: bool) {
    fn dbl(t: &mut [u8; 16]) {
        let v = u128::from_be_bytes(*t);
        let r = (v >> 1) ^ (0u128.wrapping_sub(v & 1) & (0xE1u128 << 120));
        *t = r.to_be_bytes();
    }
    fn xex<B: Bc>(c: &B, b: &mut [u8], t: &[u8; 16], enc: bool) {
        for (x, y) in b.iter_mut().zip(t.iter()) {
            *x ^= *y;
        }
        if enc {
            c.e(b);
        } else {
            c.d(b);
        }
        for (x, y) in b.iter_mut().zip(t.iter()) {
            *x ^= *y;
        }
    }
    let mut t: [u8; 16] = iv.try_into().expect("16-byte tweak");
    c2.e(&mut t);
    let rem = buf.len() % 16;
    let full = buf.len() / 16 - usize::from(rem != 0);
    for b in buf[..full * 16].as_chunks_mut::<16>().0 {
        xex(c1, b, &t, enc);
        dbl(&mut t);
    }
    if rem == 0 {
        return;
    }
    let off = full * 16;
    let mut t2 = t;
    dbl(&mut t2);
    // Encryption uses T then T' on the last two blocks; decryption T' then T.
    let (ta, tb) = if enc { (t, t2) } else { (t2, t) };
    let mut cc = [0u8; 16];
    cc.copy_from_slice(&buf[off..off + 16]);
    xex(c1, &mut cc, &ta, enc);
    let mut pp = [0u8; 16];
    pp[..rem].copy_from_slice(&buf[off + 16..]);
    pp[rem..].copy_from_slice(&cc[rem..]);
    xex(c1, &mut pp, &tb, enc);
    buf[off..off + 16].copy_from_slice(&pp);
    buf[off + 16..].copy_from_slice(&cc[..rem]);
}

// ---------------------------------------------------------------------------
// AEAD.

/// Encrypts with an AEAD cipher (GCM, CCM, OCB, ChaCha20-Poly1305) and
/// returns the ciphertext and a tag of `tag_len` bytes. The IV may be any
/// length the mode accepts ([`CipherInfo::aead_iv_ok`]).
pub fn aead_encrypt(
    info: &CipherInfo,
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    data: &[u8],
    tag_len: usize,
) -> Result<(Vec<u8>, Vec<u8>), CipherError> {
    let mut out = data.to_vec();
    let tag = aead_encrypt_in_place(info, key, iv, aad, &mut out, tag_len)?;
    Ok((out, tag))
}

/// Decrypts with an AEAD cipher; see [`aead_decrypt_in_place`].
pub fn aead_decrypt(
    info: &CipherInfo,
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    data: &[u8],
    tag: Option<&[u8]>,
) -> Result<Vec<u8>, CipherError> {
    let mut out = data.to_vec();
    aead_decrypt_in_place(info, key, iv, aad, &mut out, tag)?;
    Ok(out)
}

/// [`aead_encrypt`] in place: encrypts `out` and returns the tag.
pub fn aead_encrypt_in_place(
    info: &CipherInfo,
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    out: &mut [u8],
    tag_len: usize,
) -> Result<Vec<u8>, CipherError> {
    if key.len() != info.key_len() {
        return Err(CipherError::InvalidKeyLength);
    }
    if !info.is_aead() {
        return Err(CipherError::Unsupported);
    }
    if !info.aead_iv_ok(iv.len()) {
        return Err(CipherError::InvalidIvLength);
    }
    if !info.tag_len_ok(tag_len as i64) {
        return Err(CipherError::InvalidTagLength);
    }
    let tag: Vec<u8> = match info.mode {
        Mode::Gcm => with16!(info.algo, key, raw => {
            let g = Gcm::new(raw);
            let t = g
                .try_encrypt(iv, aad, out)
                .map_err(|_| CipherError::OperationFailed)?;
            Ok(t[..tag_len].to_vec())
        })?,
        Mode::Ccm => with16!(info.algo, key, raw => ccm_encrypt(raw, iv, aad, out, tag_len))?,
        Mode::Ocb => with16!(info.algo, key, raw => {
            Ok(ocb::Ocb::new(W16(raw)).encrypt(iv, aad, out, tag_len))
        })?,
        Mode::ChaChaPoly => {
            let n: &[u8; 12] = iv.try_into().map_err(|_| CipherError::InvalidIvLength)?;
            let t = ChaCha20Poly1305::new(arr(key)).encrypt(n, aad, out);
            t[..tag_len].to_vec()
        }
        _ => return Err(CipherError::Unsupported),
    };
    Ok(tag)
}

/// Decrypts with an AEAD cipher, verifying `tag` (which may be truncated to
/// any length the mode accepts). Without a tag, ChaCha20-Poly1305 decrypts
/// unverified (observed on PHP 8.5), GCM and OCB fail without queuing an
/// error, and CCM fails with "cipher operation failed".
pub fn aead_decrypt_in_place(
    info: &CipherInfo,
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    out: &mut [u8],
    tag: Option<&[u8]>,
) -> Result<(), CipherError> {
    if key.len() != info.key_len() {
        return Err(CipherError::InvalidKeyLength);
    }
    if !info.is_aead() {
        return Err(CipherError::Unsupported);
    }
    if !info.aead_iv_ok(iv.len()) {
        return Err(CipherError::InvalidIvLength);
    }
    let tag = tag.filter(|t| !t.is_empty());
    if let Some(t) = tag
        && !info.tag_len_ok(t.len() as i64)
    {
        return Err(CipherError::InvalidTagLength);
    }
    match info.mode {
        Mode::Gcm => {
            let Some(tag) = tag else {
                return Err(CipherError::TagMismatch);
            };
            with16!(info.algo, key, raw => gcm_decrypt(raw, iv, aad, out, tag))?
        }
        Mode::Ccm => {
            let Some(tag) = tag else {
                return Err(CipherError::OperationFailed);
            };
            with16!(info.algo, key, raw => ccm_decrypt(raw, iv, aad, out, tag))?
        }
        Mode::Ocb => {
            let Some(tag) = tag else {
                return Err(CipherError::TagMismatch);
            };
            with16!(info.algo, key, raw => {
                if ocb::Ocb::new(W16(raw)).decrypt(iv, aad, out, tag) {
                    Ok(())
                } else {
                    Err(CipherError::TagMismatch)
                }
            })?
        }
        Mode::ChaChaPoly => chachapoly_decrypt(key, iv, aad, out, tag)?,
        _ => return Err(CipherError::Unsupported),
    }
    Ok(())
}

fn gcm_decrypt<C: BlockCipher>(
    c: C,
    iv: &[u8],
    aad: &[u8],
    buf: &mut [u8],
    tag: &[u8],
) -> Result<(), CipherError> {
    let g = Gcm::new(c);
    if let Ok(t16) = <&[u8; 16]>::try_from(tag) {
        return g
            .try_decrypt(iv, aad, buf, t16)
            .map_err(|_| CipherError::TagMismatch);
    }
    // A truncated tag (NIST SP 800-38D §5.2.1.2): GCM's keystream is that
    // of its encryption, so encrypting the ciphertext yields the plaintext,
    // and encrypting that plaintext again yields the full tag over the
    // ciphertext.
    let ct = buf.to_vec();
    g.try_encrypt(iv, aad, buf)
        .map_err(|_| CipherError::OperationFailed)?;
    let mut again = buf.to_vec();
    let full = g
        .try_encrypt(iv, aad, &mut again)
        .map_err(|_| CipherError::OperationFailed)?;
    debug_assert_eq!(again, ct);
    // The verdict is the public outcome of decryption.
    if bool::from(full[..tag.len()].ct_eq(tag)) {
        Ok(())
    } else {
        purecrypto::zeroize::Zeroize::zeroize(buf);
        Err(CipherError::TagMismatch)
    }
}

/// CCM's tag length is a type parameter: one arm per length NIST SP
/// 800-38C allows.
macro_rules! ccm_dispatch {
    ($len:expr, $m:ident => $body:expr) => {
        match $len {
            4 => {
                const $m: usize = 4;
                $body
            }
            6 => {
                const $m: usize = 6;
                $body
            }
            8 => {
                const $m: usize = 8;
                $body
            }
            10 => {
                const $m: usize = 10;
                $body
            }
            12 => {
                const $m: usize = 12;
                $body
            }
            14 => {
                const $m: usize = 14;
                $body
            }
            16 => {
                const $m: usize = 16;
                $body
            }
            _ => Err(CipherError::InvalidTagLength),
        }
    };
}

fn ccm_encrypt<C: BlockCipher>(
    c: C,
    iv: &[u8],
    aad: &[u8],
    buf: &mut [u8],
    tag_len: usize,
) -> Result<Vec<u8>, CipherError> {
    ccm_dispatch!(tag_len, TL => {
        let m = purecrypto::cipher::Ccm::<C, TL>::try_new(c)
            .map_err(|_| CipherError::InvalidTagLength)?;
        m.try_encrypt(iv, aad, buf)
            .map(|t| t.to_vec())
            .map_err(|_| CipherError::OperationFailed)
    })
}

fn ccm_decrypt<C: BlockCipher>(
    c: C,
    iv: &[u8],
    aad: &[u8],
    buf: &mut [u8],
    tag: &[u8],
) -> Result<(), CipherError> {
    ccm_dispatch!(tag.len(), TL => {
        let m = purecrypto::cipher::Ccm::<C, TL>::try_new(c)
            .map_err(|_| CipherError::InvalidTagLength)?;
        let t: &[u8; TL] = tag.try_into().map_err(|_| CipherError::InvalidTagLength)?;
        // CCM's tag failure queues "cipher operation failed" (observed on
        // PHP 8.5).
        m.try_decrypt(iv, aad, buf, t)
            .map_err(|_| CipherError::OperationFailed)
    })
}

/// RFC 8439 §2.8 with the tag optionally truncated or absent.
fn chachapoly_decrypt(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    buf: &mut [u8],
    tag: Option<&[u8]>,
) -> Result<(), CipherError> {
    let n: &[u8; 12] = iv.try_into().map_err(|_| CipherError::InvalidIvLength)?;
    if let Some(t) = tag
        && let Ok(t16) = <&[u8; 16]>::try_from(t)
    {
        return ChaCha20Poly1305::new(arr(key))
            .decrypt(n, aad, buf, t16)
            .map_err(|_| CipherError::TagMismatch);
    }
    let cc = ChaCha20::new(arr(key));
    if let Some(t) = tag {
        let block = cc.block(n, 0);
        let mut otk = [0u8; 32];
        otk.copy_from_slice(&block[..32]);
        let mut p = Poly1305::new(&otk);
        purecrypto::zeroize::Zeroize::zeroize(&mut otk);
        let zeros = [0u8; 16];
        p.update(aad);
        p.update(&zeros[..(16 - aad.len() % 16) % 16]);
        p.update(buf);
        p.update(&zeros[..(16 - buf.len() % 16) % 16]);
        p.update(&(aad.len() as u64).to_le_bytes());
        p.update(&(buf.len() as u64).to_le_bytes());
        let full = p.finish();
        // The verdict is the public outcome of decryption.
        if !bool::from(full[..t.len()].ct_eq(t)) {
            return Err(CipherError::TagMismatch);
        }
    }
    cc.apply_keystream(n, 1, buf);
    Ok(())
}

#[cfg(test)]
mod tests;
