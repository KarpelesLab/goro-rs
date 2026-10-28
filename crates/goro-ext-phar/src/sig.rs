//! Archive signatures (PHP manual: Phar::setSignatureAlgorithm, "Phar
//! Signature format": the hash of everything before it, then the flags
//! and `GBMB`).

/// A signature algorithm, with its flag value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SigType {
    Md5,
    Sha1,
    Sha256,
    Sha512,
    /// OpenSSL with SHA-1, SHA-256 or SHA-512 digests.
    OpenSsl,
    OpenSslSha256,
    OpenSslSha512,
}

/// Makes an OpenSSL signature of the data: (type, data) → signature.
pub type Signer = dyn Fn(SigType, &[u8]) -> Result<Vec<u8>, String>;
/// Checks an OpenSSL signature: (type, data, signature) → valid.
pub type Verifier = dyn Fn(SigType, &[u8], &[u8]) -> Result<bool, String>;

impl SigType {
    /// The flag value (PHP manual: Phar::MD5 = 1, SHA1 = 2, SHA256 = 3,
    /// SHA512 = 4, OPENSSL = 16, OPENSSL_SHA256 = 17, OPENSSL_SHA512 = 18).
    pub fn flag(self) -> u32 {
        match self {
            SigType::Md5 => 1,
            SigType::Sha1 => 2,
            SigType::Sha256 => 3,
            SigType::Sha512 => 4,
            SigType::OpenSsl => 0x10,
            SigType::OpenSslSha256 => 0x11,
            SigType::OpenSslSha512 => 0x12,
        }
    }

    pub fn from_flag(f: u32) -> Option<SigType> {
        Some(match f {
            1 => SigType::Md5,
            2 => SigType::Sha1,
            3 => SigType::Sha256,
            4 => SigType::Sha512,
            0x10 => SigType::OpenSsl,
            0x11 => SigType::OpenSslSha256,
            0x12 => SigType::OpenSslSha512,
            _ => return None,
        })
    }

    /// Phar::getSignature()'s "hash_type".
    pub fn name(self) -> &'static str {
        match self {
            SigType::Md5 => "MD5",
            SigType::Sha1 => "SHA-1",
            SigType::Sha256 => "SHA-256",
            SigType::Sha512 => "SHA-512",
            SigType::OpenSsl => "OpenSSL",
            SigType::OpenSslSha256 => "OpenSSL_SHA256",
            SigType::OpenSslSha512 => "OpenSSL_SHA512",
        }
    }

    pub fn is_openssl(self) -> bool {
        matches!(
            self,
            SigType::OpenSsl | SigType::OpenSslSha256 | SigType::OpenSslSha512
        )
    }

    /// The digest length of a hash signature.
    pub fn digest_len(self) -> Option<usize> {
        match self {
            SigType::Md5 => Some(16),
            SigType::Sha1 => Some(20),
            SigType::Sha256 => Some(32),
            SigType::Sha512 => Some(64),
            _ => None,
        }
    }
}

/// The digest of a hash signature type.
pub fn digest(t: SigType, data: &[u8]) -> Vec<u8> {
    use purecrypto::hash as h;
    match t {
        SigType::Md5 => h::md5(data).to_vec(),
        SigType::Sha1 => h::sha1(data).to_vec(),
        SigType::Sha256 => h::sha256(data).to_vec(),
        SigType::Sha512 => h::sha512(data).to_vec(),
        _ => Vec::new(),
    }
}

/// Signs `data` with `t`.
pub fn make(t: SigType, data: &[u8], sign: &Signer) -> Result<Vec<u8>, String> {
    if t.is_openssl() {
        sign(t, data)
    } else {
        Ok(digest(t, data))
    }
}

/// Upper-case hex (Phar::getSignature()'s "hash").
pub fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{x:02X}"));
    }
    s
}

/// The phar-format signature trailer of `data`: the signature, its length
/// for OpenSSL ones, the flags and `GBMB`.
pub fn trailer(t: SigType, sig: &[u8]) -> Vec<u8> {
    let mut out = sig.to_vec();
    if t.is_openssl() {
        out.extend_from_slice(&(sig.len() as u32).to_le_bytes());
    }
    out.extend_from_slice(&t.flag().to_le_bytes());
    out.extend_from_slice(b"GBMB");
    out
}

/// Checks a signature of `data`: Ok(()) or false when it doesn't match.
pub fn check(
    t: SigType,
    data: &[u8],
    sig: &[u8],
    verify: Option<&Verifier>,
) -> Result<bool, String> {
    if t.is_openssl() {
        match verify {
            Some(v) => v(t, data, sig),
            None => Ok(false),
        }
    } else {
        Ok(digest(t, data) == sig)
    }
}

/// The tar/zip `.phar/signature.bin` contents: flags, length, signature.
pub fn bin(t: SigType, sig: &[u8]) -> Vec<u8> {
    let mut out = t.flag().to_le_bytes().to_vec();
    out.extend_from_slice(&(sig.len() as u32).to_le_bytes());
    out.extend_from_slice(sig);
    out
}
