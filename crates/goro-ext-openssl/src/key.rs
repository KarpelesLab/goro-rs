//! Asymmetric keys: RSA, DSA, DH, EC, X25519, Ed25519, X448, Ed448.
//!
//! Encodings: PKCS #8 (RFC 5958) and SubjectPublicKeyInfo (RFC 5280
//! §4.1.2.7) for every type, PKCS #1 (RFC 8017 appendix A) for RSA,
//! SEC 1 (RFC 5915) for EC, the DSA and DH parameter structures of
//! RFC 3279 §2.3.2 / PKCS #3, and the RFC 8410 curve keys. Arithmetic on
//! secrets goes through purecrypto's constant-time code (RSA private
//! operations, EC scalar multiplication, the Montgomery ladder for DSA/DH
//! exponents).

use purecrypto::bignum::{BoxedMontModulus, BoxedUint};
use purecrypto::ec::{
    BoxedCurve, BoxedEcdsaPrivateKey, BoxedEcdsaPublicKey, BoxedEcdsaSignature, CurveId,
    Ed448PrivateKey, Ed448PublicKey, Ed448Signature, Ed25519PrivateKey, Ed25519PublicKey,
    Ed25519Signature, X448PrivateKey, X25519PrivateKey,
};
use purecrypto::rsa::{BoxedRsaPrivateKey, BoxedRsaPublicKey};
use purecrypto::zeroize::Zeroizing;

use crate::der::{self, Tlv, tag};
use crate::digest::Md;
use crate::oid::ids;

pub type Secret = Zeroizing<Vec<u8>>;

fn secret(v: Vec<u8>) -> Secret {
    Zeroizing::new(v)
}

/// The `OPENSSL_KEYTYPE_*` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    Rsa = 0,
    Dsa = 1,
    Dh = 2,
    Ec = 3,
    X25519 = 4,
    Ed25519 = 5,
    X448 = 6,
    Ed448 = 7,
}

impl KeyType {
    pub fn from_int(n: i64) -> Option<KeyType> {
        Some(match n {
            0 => KeyType::Rsa,
            1 => KeyType::Dsa,
            2 => KeyType::Dh,
            3 => KeyType::Ec,
            4 => KeyType::X25519,
            5 => KeyType::Ed25519,
            6 => KeyType::X448,
            7 => KeyType::Ed448,
            _ => return None,
        })
    }
    /// The details array key (openssl_pkey_get_details()).
    pub fn name(self) -> &'static str {
        match self {
            KeyType::Rsa => "rsa",
            KeyType::Dsa => "dsa",
            KeyType::Dh => "dh",
            KeyType::Ec => "ec",
            KeyType::X25519 => "x25519",
            KeyType::Ed25519 => "ed25519",
            KeyType::X448 => "x448",
            KeyType::Ed448 => "ed448",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyError {
    /// Not a key in any accepted encoding.
    Decode,
    /// The key is encrypted and the passphrase is wrong or missing.
    BadDecrypt,
    /// The components don't make a valid key.
    Invalid,
    /// Not supported (algorithm, curve, parameter size).
    Unsupported,
}

pub type KResult<T> = Result<T, KeyError>;

pub struct RsaPriv {
    pub d: Secret,
    pub p: Secret,
    pub q: Secret,
    pub dmp1: Secret,
    pub dmq1: Secret,
    pub iqmp: Secret,
    pub key: BoxedRsaPrivateKey,
}

/// An elliptic curve: named or given by explicit parameters.
#[derive(Clone)]
pub struct Curve {
    /// The named curve's OID.
    pub oid: Option<&'static str>,
    pub boxed: BoxedCurve,
    /// The ECParameters encoding (namedCurve OID or specifiedCurve).
    pub params_der: Vec<u8>,
}

pub enum KeyData {
    Rsa {
        n: Vec<u8>,
        e: Vec<u8>,
        /// None for an even modulus: such keys load, as on PHP 8.5 (test
        /// bug74651.phpt), but can't be used.
        public: Option<BoxedRsaPublicKey>,
        private: Option<Box<RsaPriv>>,
    },
    Dsa {
        p: Vec<u8>,
        q: Vec<u8>,
        g: Vec<u8>,
        y: Vec<u8>,
        x: Option<Secret>,
    },
    Dh {
        p: Vec<u8>,
        g: Vec<u8>,
        /// X9.42 (`dhpublicnumber`) keys carry q.
        q: Option<Vec<u8>>,
        y: Vec<u8>,
        x: Option<Secret>,
    },
    Ec {
        curve: Curve,
        /// Uncompressed point 04||X||Y.
        point: Vec<u8>,
        d: Option<Secret>,
    },
    X25519 {
        public: [u8; 32],
        private: Option<Zeroizing<[u8; 32]>>,
    },
    Ed25519 {
        public: [u8; 32],
        private: Option<Zeroizing<[u8; 32]>>,
    },
    X448 {
        public: [u8; 56],
        private: Option<Zeroizing<[u8; 56]>>,
    },
    Ed448 {
        public: [u8; 57],
        private: Option<Zeroizing<[u8; 57]>>,
    },
}

/// A key (public, or private with its public half).
pub struct Key {
    pub data: KeyData,
}

/// A detail value of openssl_pkey_get_details().
pub enum Detail {
    Bin(Vec<u8>),
    Str(String),
}

pub struct Details {
    pub bits: i64,
    pub key_pem: String,
    pub typ: Option<KeyType>,
    pub fields: Vec<(&'static str, Detail)>,
}

/// Strips leading zero bytes (keeping one byte).
pub fn strip(b: &[u8]) -> Vec<u8> {
    let mut i = 0;
    while i + 1 < b.len() && b[i] == 0 {
        i += 1;
    }
    b[i..].to_vec()
}

fn bits_of(b: &[u8]) -> usize {
    let b = strip(b);
    if b.is_empty() || b == [0] {
        return 0;
    }
    (b.len() - 1) * 8 + (8 - b[0].leading_zeros() as usize)
}

/// An RSA public key; sizes down to 256 bits are accepted (PHP accepts
/// the 512-bit keys of its tests; purecrypto's `try_new` floor is 1024).
fn rsa_pub(n: &[u8], e: &[u8]) -> KResult<BoxedRsaPublicKey> {
    let nb = uint(n);
    let eb = uint(e);
    let bits = nb.bit_len();
    // Even or tiny exponents are accepted like OpenSSL does (test
    // bug79145.phpt loads a key with e = 65538).
    if !(256..=16384).contains(&bits) || !nb.is_odd() || eb.is_zero() {
        return Err(KeyError::Invalid);
    }
    if !eb.lt(&nb) || eb.bit_len() > 256 {
        return Err(KeyError::Invalid);
    }
    Ok(BoxedRsaPublicKey::new(nb, eb))
}

fn uint(b: &[u8]) -> BoxedUint {
    BoxedUint::from_be_bytes(b)
}

/// Named curves: OpenSSL short name, OID and purecrypto curve (SEC 2,
/// FIPS 186-4 D.1.2, RFC 5639).
pub static CURVES: &[(&str, &str, CurveId)] = &[
    ("secp160k1", "1.3.132.0.9", CurveId::Secp160k1),
    ("secp160r1", "1.3.132.0.8", CurveId::Secp160r1),
    ("secp160r2", "1.3.132.0.30", CurveId::Secp160r2),
    ("secp192k1", "1.3.132.0.31", CurveId::Secp192k1),
    ("secp224k1", "1.3.132.0.32", CurveId::Secp224k1),
    ("secp224r1", "1.3.132.0.33", CurveId::P224),
    ("secp256k1", "1.3.132.0.10", CurveId::Secp256k1),
    ("secp384r1", "1.3.132.0.34", CurveId::P384),
    ("secp521r1", "1.3.132.0.35", CurveId::P521),
    ("prime192v1", "1.2.840.10045.3.1.1", CurveId::P192),
    ("prime256v1", "1.2.840.10045.3.1.7", CurveId::P256),
    (
        "brainpoolP224r1",
        "1.3.36.3.3.2.8.1.1.5",
        CurveId::BrainpoolP224r1,
    ),
    (
        "brainpoolP256r1",
        "1.3.36.3.3.2.8.1.1.7",
        CurveId::BrainpoolP256r1,
    ),
    (
        "brainpoolP320r1",
        "1.3.36.3.3.2.8.1.1.9",
        CurveId::BrainpoolP320r1,
    ),
    (
        "brainpoolP384r1",
        "1.3.36.3.3.2.8.1.1.11",
        CurveId::BrainpoolP384r1,
    ),
    (
        "brainpoolP512r1",
        "1.3.36.3.3.2.8.1.1.13",
        CurveId::BrainpoolP512r1,
    ),
    ("SM2", "1.2.156.10197.1.301", CurveId::Sm2p256v1),
];

impl Curve {
    pub fn named(name: &str) -> Option<Curve> {
        let alias = match name {
            "P-192" => "prime192v1",
            "P-224" => "secp224r1",
            "P-256" | "secp256r1" => "prime256v1",
            "P-384" => "secp384r1",
            "P-521" => "secp521r1",
            other => other,
        };
        let (_, oid, id) = CURVES.iter().find(|(n, _, _)| *n == alias)?;
        Some(Curve::from_id(oid, *id))
    }
    fn from_id(oid: &'static str, id: CurveId) -> Curve {
        Curve {
            oid: Some(oid),
            boxed: BoxedCurve::from(id),
            params_der: der::oid(oid),
        }
    }
    pub fn by_oid(oid: &str) -> Option<Curve> {
        let (_, o, id) = CURVES.iter().find(|(_, o, _)| *o == oid)?;
        Some(Curve::from_id(o, *id))
    }
    pub fn name(&self) -> Option<&'static str> {
        let oid = self.oid?;
        CURVES
            .iter()
            .find(|(_, o, _)| *o == oid)
            .map(|(n, _, _)| *n)
    }
    /// A curve from explicit prime-field parameters (SEC 1 §3.1.1).
    pub fn explicit(
        p: &[u8],
        a: &[u8],
        b: &[u8],
        gx: &[u8],
        gy: &[u8],
        n: &[u8],
        h: &[u8],
    ) -> KResult<Curve> {
        let boxed = BoxedCurve::from_params(
            &uint(p),
            &uint(a),
            &uint(b),
            &uint(gx),
            &uint(gy),
            &uint(n),
            &uint(if h.is_empty() { &[1] } else { h }),
        )
        .map_err(|_| KeyError::Invalid)?;
        // Curves given by parameters keep the explicit encoding even when
        // they equal a named curve (observed on PHP 8.5, test
        // ecc_sm2.phpt: a 475-byte public key PEM).
        // SpecifiedECDomain (SEC 1 §C.2), version 1, prime field.
        let flen = p.len().max(strip(p).len());
        let pad = |v: &[u8]| {
            let s = strip(v);
            let mut o = vec![0u8; flen.saturating_sub(s.len())];
            o.extend_from_slice(&s);
            o
        };
        let mut point = vec![4u8];
        point.extend_from_slice(&pad(gx));
        point.extend_from_slice(&pad(gy));
        let params = der::seq_vec(&[
            der::int(1),
            der::seq_vec(&[der::oid("1.2.840.10045.1.1"), der::uint(p)]),
            der::seq_vec(&[der::octet_string(&pad(a)), der::octet_string(&pad(b))]),
            der::octet_string(&point),
            der::uint(n),
            der::uint(if h.is_empty() { &[1] } else { h }),
        ]);
        Ok(Curve {
            oid: None,
            boxed,
            params_der: params,
        })
    }
    /// From an ECParameters element.
    pub fn from_params(t: &Tlv) -> KResult<Curve> {
        if t.is_universal(tag::OID) {
            let oid = t.oid().map_err(|_| KeyError::Decode)?;
            return Curve::by_oid(&oid).ok_or(KeyError::Unsupported);
        }
        if !t.is_universal(tag::SEQUENCE) {
            return Err(KeyError::Decode);
        }
        let d = |_| KeyError::Decode;
        let mut r = t.reader();
        let _ver = r.expect(tag::INTEGER).map_err(d)?;
        let field = r.expect(tag::SEQUENCE).map_err(d)?;
        let mut fr = field.reader();
        let ftype = fr.next().and_then(|x| x.oid()).map_err(d)?;
        if ftype != "1.2.840.10045.1.1" {
            return Err(KeyError::Unsupported);
        }
        let p = fr.next().and_then(|x| x.uint()).map_err(d)?;
        let coeffs = r.expect(tag::SEQUENCE).map_err(d)?;
        let mut cr = coeffs.reader();
        let a = cr.expect(tag::OCTET_STRING).map_err(d)?.content;
        let b = cr.expect(tag::OCTET_STRING).map_err(d)?.content;
        let base = r.expect(tag::OCTET_STRING).map_err(d)?.content;
        let n = r.next().and_then(|x| x.uint()).map_err(d)?;
        let h = if r.is_empty() {
            vec![1]
        } else {
            r.next().and_then(|x| x.uint()).map_err(d)?.to_vec()
        };
        let flen = strip(p).len();
        if base.len() != 1 + 2 * flen || base[0] != 4 {
            return Err(KeyError::Unsupported);
        }
        let gx = &base[1..1 + flen];
        let gy = &base[1 + flen..];
        let mut c = Curve::explicit(p, a, b, gx, gy, n, &h)?;
        if c.oid.is_none() {
            c.params_der = t.raw.to_vec();
        }
        Ok(c)
    }
    pub fn field_len(&self) -> usize {
        self.boxed.field_len()
    }
    pub fn order(&self) -> Vec<u8> {
        let n = self.boxed.order();
        n.to_be_bytes(n.bit_len().div_ceil(8))
    }
    pub fn order_bits(&self) -> usize {
        self.boxed.order().bit_len()
    }
}

/// A uniformly random integer in [1, n-1] (rejection sampling).
fn random_below(n: &[u8]) -> Vec<u8> {
    let n = strip(n);
    let nb = bits_of(&n);
    let len = n.len();
    let mask = if nb.is_multiple_of(8) {
        0xff
    } else {
        (1u8 << (nb % 8)) - 1
    };
    loop {
        let mut buf = vec![0u8; len];
        crate::random_bytes(&mut buf);
        buf[0] &= mask;
        // Public comparison: rejection only leaks the number of draws.
        if buf.iter().all(|&b| b == 0) {
            continue;
        }
        if buf.as_slice() < n.as_slice() {
            return buf;
        }
    }
}

fn pad_to(b: &[u8], len: usize) -> Vec<u8> {
    let s = strip(b);
    if s.len() >= len {
        return s;
    }
    let mut v = vec![0u8; len - s.len()];
    v.extend_from_slice(&s);
    v
}

/// g^x mod p on the constant-time ladder (x secret).
fn modpow_ct(g: &[u8], x: &[u8], p: &[u8]) -> KResult<Vec<u8>> {
    let pm = uint(p);
    if !pm.is_odd() {
        return Err(KeyError::Invalid);
    }
    let m = BoxedMontModulus::new(&pm);
    let r = m.pow(&uint(g).reduce(&pm), &uint(x));
    Ok(strip(&r.to_be_bytes(p.len())))
}

impl Key {
    pub fn key_type(&self) -> KeyType {
        match &self.data {
            KeyData::Rsa { .. } => KeyType::Rsa,
            KeyData::Dsa { .. } => KeyType::Dsa,
            KeyData::Dh { .. } => KeyType::Dh,
            KeyData::Ec { .. } => KeyType::Ec,
            KeyData::X25519 { .. } => KeyType::X25519,
            KeyData::Ed25519 { .. } => KeyType::Ed25519,
            KeyData::X448 { .. } => KeyType::X448,
            KeyData::Ed448 { .. } => KeyType::Ed448,
        }
    }

    pub fn is_private(&self) -> bool {
        match &self.data {
            KeyData::Rsa { private, .. } => private.is_some(),
            KeyData::Dsa { x, .. } | KeyData::Dh { x, .. } => x.is_some(),
            KeyData::Ec { d, .. } => d.is_some(),
            KeyData::X25519 { private, .. } | KeyData::Ed25519 { private, .. } => private.is_some(),
            KeyData::X448 { private, .. } => private.is_some(),
            KeyData::Ed448 { private, .. } => private.is_some(),
        }
    }

    /// The public half.
    pub fn public_only(&self) -> Key {
        Key::from_spki_der(&self.to_spki_der()).expect("own SPKI parses")
    }

    pub fn bits(&self) -> i64 {
        match &self.data {
            KeyData::Rsa { n, .. } => bits_of(n) as i64,
            KeyData::Dsa { p, .. } | KeyData::Dh { p, .. } => bits_of(p) as i64,
            KeyData::Ec { curve, .. } => curve.order_bits() as i64,
            // Observed on PHP 8.5.
            KeyData::X25519 { .. } => 253,
            KeyData::Ed25519 { .. } => 256,
            KeyData::X448 { .. } => 448,
            KeyData::Ed448 { .. } => 456,
        }
    }

    /// The modulus / signature size in bytes (RSA_size, DSA/EC max sig).
    pub fn size(&self) -> usize {
        match &self.data {
            KeyData::Rsa { n, .. } => strip(n).len(),
            KeyData::Dsa { q, .. } => 2 * strip(q).len() + 8,
            KeyData::Dh { p, .. } => strip(p).len(),
            KeyData::Ec { curve, .. } => 2 * curve.order().len() + 8,
            KeyData::Ed25519 { .. } => 64,
            KeyData::Ed448 { .. } => 114,
            KeyData::X25519 { .. } => 32,
            KeyData::X448 { .. } => 56,
        }
    }

    // ------------------------------------------------------------ building

    pub fn rsa_public(n: &[u8], e: &[u8]) -> KResult<Key> {
        let n = strip(n);
        let e = strip(e);
        let public = if n.last().is_some_and(|b| b & 1 == 0) {
            None
        } else {
            Some(rsa_pub(&n, &e)?)
        };
        Ok(Key {
            data: KeyData::Rsa {
                n,
                e,
                public,
                private: None,
            },
        })
    }

    /// An RSA private key from its components; the CRT values are
    /// derived when missing.
    #[allow(clippy::too_many_arguments)]
    pub fn rsa_private(
        n: &[u8],
        e: &[u8],
        d: &[u8],
        p: Option<&[u8]>,
        q: Option<&[u8]>,
        dmp1: Option<&[u8]>,
        dmq1: Option<&[u8]>,
        iqmp: Option<&[u8]>,
    ) -> KResult<Key> {
        let n = strip(n);
        let e = strip(e);
        let nb = uint(&n);
        if !nb.is_odd() || nb.is_zero() {
            return Err(KeyError::Invalid);
        }
        let public = Some(rsa_pub(&n, &e)?);
        let key = match (p, q) {
            (Some(p), Some(q)) => match (dmp1, dmq1, iqmp) {
                (Some(dp), Some(dq), Some(qi)) => BoxedRsaPrivateKey::from_crt_components(
                    nb.clone(),
                    uint(&e),
                    uint(d),
                    uint(p),
                    uint(q),
                    uint(dp),
                    uint(dq),
                    uint(qi),
                )
                .unwrap_or_else(|_| {
                    BoxedRsaPrivateKey::from_components_with_primes(
                        nb.clone(),
                        uint(&e),
                        uint(d),
                        uint(p),
                        uint(q),
                    )
                }),
                _ => BoxedRsaPrivateKey::from_components_with_primes(
                    nb.clone(),
                    uint(&e),
                    uint(d),
                    uint(p),
                    uint(q),
                ),
            },
            _ => BoxedRsaPrivateKey::from_components(nb.clone(), uint(&e), uint(d)),
        };
        let k = n.len();
        let get = |v: Option<&BoxedUint>| -> Secret {
            secret(
                v.map(|x| strip(&x.to_be_bytes(k)))
                    .unwrap_or_else(|| vec![0]),
            )
        };
        let (pp, qq) = match key.primes() {
            Some((a, b)) => (get(Some(a)), get(Some(b))),
            None => (
                secret(p.map(strip).unwrap_or_else(|| vec![0])),
                secret(q.map(strip).unwrap_or_else(|| vec![0])),
            ),
        };
        let given = dmp1.is_some() && dmq1.is_some() && iqmp.is_some();
        let (dp, dq, qi) = match key.crt_params().filter(|_| !given) {
            Some((a, b, c)) => (get(Some(a)), get(Some(b)), get(Some(c))),
            None => (
                secret(dmp1.map(strip).unwrap_or_else(|| vec![0])),
                secret(dmq1.map(strip).unwrap_or_else(|| vec![0])),
                secret(iqmp.map(strip).unwrap_or_else(|| vec![0])),
            ),
        };
        Ok(Key {
            data: KeyData::Rsa {
                n,
                e,
                public,
                private: Some(Box::new(RsaPriv {
                    d: secret(strip(d)),
                    p: pp,
                    q: qq,
                    dmp1: dp,
                    dmq1: dq,
                    iqmp: qi,
                    key,
                })),
            },
        })
    }

    pub fn rsa_generate(bits: usize, e: &[u8]) -> KResult<Key> {
        let mut rng = purecrypto::rng::OsRng;
        let k = BoxedRsaPrivateKey::try_generate(bits, uint(e), &mut rng, 0)
            .map_err(|_| KeyError::Invalid)?;
        let n = k.modulus().clone();
        let nb = n.to_be_bytes(n.bit_len().div_ceil(8));
        let len = nb.len();
        let d = k.private_exponent().to_be_bytes(len);
        let (p, q) = k.primes().ok_or(KeyError::Invalid)?;
        let (p, q) = (p.to_be_bytes(len), q.to_be_bytes(len));
        let (dp, dq, qi) = k.crt_params().ok_or(KeyError::Invalid)?;
        let (dp, dq, qi) = (
            dp.to_be_bytes(len),
            dq.to_be_bytes(len),
            qi.to_be_bytes(len),
        );
        let r = Key::rsa_private(
            &nb,
            e,
            &d,
            Some(&p),
            Some(&q),
            Some(&dp),
            Some(&dq),
            Some(&qi),
        );
        drop(k);
        let mut d = d;
        purecrypto::zeroize::Zeroize::zeroize(&mut d[..]);
        r
    }

    pub fn dsa(p: &[u8], q: &[u8], g: &[u8], y: Option<&[u8]>, x: Option<&[u8]>) -> KResult<Key> {
        let (p, q, g) = (strip(p), strip(q), strip(g));
        if bits_of(&p) < 2 || bits_of(&q) < 2 {
            return Err(KeyError::Invalid);
        }
        let y = match (y, x) {
            (Some(y), _) => strip(y),
            (None, Some(x)) => modpow_ct(&g, x, &p)?,
            (None, None) => return Err(KeyError::Invalid),
        };
        Ok(Key {
            data: KeyData::Dsa {
                p,
                q,
                g,
                y,
                x: x.map(|x| secret(strip(x))),
            },
        })
    }

    /// A DSA key with a fresh private value for the given parameters.
    pub fn dsa_generate_from(p: &[u8], q: &[u8], g: &[u8]) -> KResult<Key> {
        let x = secret(random_below(q));
        Key::dsa(p, q, g, None, Some(&x))
    }

    /// Fresh DSA parameters and key (FIPS 186-4 A.1.1.2 via purecrypto).
    pub fn dsa_generate(p_bits: usize) -> KResult<Key> {
        // Observed on PHP 8.5: 1024-bit keys come with a 224-bit q; FIPS
        // 186-4 §4.2 pairs 2048 with 224 or 256 and 3072 with 256.
        let q_bits = if p_bits >= 2048 {
            256
        } else if p_bits >= 1024 {
            224
        } else {
            160
        };
        let mut rng = purecrypto::rng::OsRng;
        let params = purecrypto::dsa::DsaParams::generate(p_bits, q_bits, &mut rng)
            .map_err(|_| KeyError::Invalid)?;
        let p = params.p().to_be_bytes(params.p_len());
        let q = params.q().to_be_bytes(params.q_len());
        let g = params.g().to_be_bytes(params.p_len());
        Key::dsa_generate_from(&p, &q, &g)
    }

    pub fn dh(
        p: &[u8],
        g: &[u8],
        q: Option<&[u8]>,
        y: Option<&[u8]>,
        x: Option<&[u8]>,
    ) -> KResult<Key> {
        let (p, g) = (strip(p), strip(g));
        if bits_of(&p) < 2 {
            return Err(KeyError::Invalid);
        }
        let y = match (y, x) {
            (Some(y), _) => strip(y),
            (None, Some(x)) => modpow_ct(&g, x, &p)?,
            (None, None) => return Err(KeyError::Invalid),
        };
        Ok(Key {
            data: KeyData::Dh {
                p,
                g,
                q: q.map(strip),
                y,
                x: x.map(|x| secret(strip(x))),
            },
        })
    }

    /// A DH key with a fresh private value (uniform below p-1, or below
    /// q for X9.42 parameters).
    pub fn dh_generate_from(p: &[u8], g: &[u8], q: Option<&[u8]>) -> KResult<Key> {
        let bound = match q {
            Some(q) => strip(q),
            None => {
                let mut pm1 = strip(p);
                // p - 1 (p is odd, so only the last byte changes).
                if let Some(l) = pm1.last_mut() {
                    *l = l.wrapping_sub(1);
                }
                pm1
            }
        };
        let x = secret(random_below(&bound));
        Key::dh(p, g, q, None, Some(&x))
    }

    pub fn ec(curve: Curve, point: Option<&[u8]>, d: Option<&[u8]>) -> KResult<Key> {
        let flen = curve.field_len();
        let point = match (point, d) {
            (Some(pt), _) => {
                let pk = BoxedEcdsaPublicKey::from_sec1(curve.boxed.clone(), pt)
                    .map_err(|_| KeyError::Invalid)?;
                pk.to_sec1()
            }
            (None, Some(d)) => {
                let sk = BoxedEcdsaPrivateKey::from_bytes(curve.boxed.clone(), d)
                    .map_err(|_| KeyError::Invalid)?;
                sk.public_key().to_sec1()
            }
            (None, None) => return Err(KeyError::Invalid),
        };
        let olen = curve.order().len();
        let d = match d {
            Some(d) => {
                // Range check (1 <= d < n) through purecrypto.
                BoxedEcdsaPrivateKey::from_bytes(curve.boxed.clone(), d)
                    .map_err(|_| KeyError::Invalid)?;
                Some(secret(pad_to(d, olen)))
            }
            None => None,
        };
        let _ = flen;
        Ok(Key {
            data: KeyData::Ec { curve, point, d },
        })
    }

    pub fn ec_generate(curve: Curve) -> KResult<Key> {
        let n = curve.order();
        let d = secret(random_below(&n));
        Key::ec(curve, None, Some(&d))
    }

    pub fn x25519(public: Option<&[u8]>, private: Option<&[u8]>) -> KResult<Key> {
        let sk = match private {
            Some(p) => Some(Zeroizing::new(
                <[u8; 32]>::try_from(p).map_err(|_| KeyError::Invalid)?,
            )),
            None => None,
        };
        let public = match (public, &sk) {
            (Some(p), _) => <[u8; 32]>::try_from(p).map_err(|_| KeyError::Invalid)?,
            (None, Some(s)) => X25519PrivateKey::from_bytes(**s).public_key(),
            _ => return Err(KeyError::Invalid),
        };
        Ok(Key {
            data: KeyData::X25519 {
                public,
                private: sk,
            },
        })
    }

    pub fn x448(public: Option<&[u8]>, private: Option<&[u8]>) -> KResult<Key> {
        let sk = match private {
            Some(p) => Some(Zeroizing::new(
                <[u8; 56]>::try_from(p).map_err(|_| KeyError::Invalid)?,
            )),
            None => None,
        };
        let public = match (public, &sk) {
            (Some(p), _) => <[u8; 56]>::try_from(p).map_err(|_| KeyError::Invalid)?,
            (None, Some(s)) => X448PrivateKey::from_bytes(**s).public_key(),
            _ => return Err(KeyError::Invalid),
        };
        Ok(Key {
            data: KeyData::X448 {
                public,
                private: sk,
            },
        })
    }

    pub fn ed25519(public: Option<&[u8]>, private: Option<&[u8]>) -> KResult<Key> {
        let sk = match private {
            Some(p) => Some(Zeroizing::new(
                <[u8; 32]>::try_from(p).map_err(|_| KeyError::Invalid)?,
            )),
            None => None,
        };
        let public = match (public, &sk) {
            (Some(p), _) => <[u8; 32]>::try_from(p).map_err(|_| KeyError::Invalid)?,
            (None, Some(s)) => Ed25519PrivateKey::from_bytes(**s).public_key().to_bytes(),
            _ => return Err(KeyError::Invalid),
        };
        Ok(Key {
            data: KeyData::Ed25519 {
                public,
                private: sk,
            },
        })
    }

    pub fn ed448(public: Option<&[u8]>, private: Option<&[u8]>) -> KResult<Key> {
        let sk = match private {
            Some(p) => Some(Zeroizing::new(
                <[u8; 57]>::try_from(p).map_err(|_| KeyError::Invalid)?,
            )),
            None => None,
        };
        let public = match (public, &sk) {
            (Some(p), _) => <[u8; 57]>::try_from(p).map_err(|_| KeyError::Invalid)?,
            (None, Some(s)) => Ed448PrivateKey::from_bytes(**s).public_key().to_bytes(),
            _ => return Err(KeyError::Invalid),
        };
        Ok(Key {
            data: KeyData::Ed448 {
                public,
                private: sk,
            },
        })
    }

    pub fn generate_raw(t: KeyType) -> KResult<Key> {
        match t {
            KeyType::X25519 => {
                let mut b = Zeroizing::new([0u8; 32]);
                crate::random_bytes(&mut b[..]);
                Key::x25519(None, Some(&b[..]))
            }
            KeyType::Ed25519 => {
                let mut b = Zeroizing::new([0u8; 32]);
                crate::random_bytes(&mut b[..]);
                Key::ed25519(None, Some(&b[..]))
            }
            KeyType::X448 => {
                let mut b = Zeroizing::new([0u8; 56]);
                crate::random_bytes(&mut b[..]);
                Key::x448(None, Some(&b[..]))
            }
            KeyType::Ed448 => {
                let mut b = Zeroizing::new([0u8; 57]);
                crate::random_bytes(&mut b[..]);
                Key::ed448(None, Some(&b[..]))
            }
            _ => Err(KeyError::Unsupported),
        }
    }

    // ------------------------------------------------------------ encoding

    /// The algorithm OID and parameters of the key's AlgorithmIdentifier.
    fn algorithm_id(&self) -> Vec<u8> {
        match &self.data {
            KeyData::Rsa { .. } => der::algorithm_id(ids::RSA, Some(&der::null())),
            KeyData::Dsa { p, q, g, .. } => der::algorithm_id(
                ids::DSA,
                Some(&der::seq_vec(&[der::uint(p), der::uint(q), der::uint(g)])),
            ),
            KeyData::Dh { p, g, q, .. } => match q {
                Some(q) => der::algorithm_id(
                    ids::DHX,
                    Some(&der::seq_vec(&[der::uint(p), der::uint(g), der::uint(q)])),
                ),
                None => {
                    der::algorithm_id(ids::DH, Some(&der::seq_vec(&[der::uint(p), der::uint(g)])))
                }
            },
            KeyData::Ec { curve, .. } => der::algorithm_id(ids::EC, Some(&curve.params_der)),
            KeyData::X25519 { .. } => der::algorithm_id(ids::X25519, None),
            KeyData::Ed25519 { .. } => der::algorithm_id(ids::ED25519, None),
            KeyData::X448 { .. } => der::algorithm_id(ids::X448, None),
            KeyData::Ed448 { .. } => der::algorithm_id(ids::ED448, None),
        }
    }

    /// The subjectPublicKey bits.
    pub fn public_bits(&self) -> Vec<u8> {
        match &self.data {
            KeyData::Rsa { n, e, .. } => der::seq_vec(&[der::uint(n), der::uint(e)]),
            KeyData::Dsa { y, .. } | KeyData::Dh { y, .. } => der::uint(y),
            KeyData::Ec { point, .. } => point.clone(),
            KeyData::X25519 { public, .. } | KeyData::Ed25519 { public, .. } => public.to_vec(),
            KeyData::X448 { public, .. } => public.to_vec(),
            KeyData::Ed448 { public, .. } => public.to_vec(),
        }
    }

    pub fn to_spki_der(&self) -> Vec<u8> {
        der::seq_vec(&[self.algorithm_id(), der::bit_string(&self.public_bits())])
    }

    pub fn to_spki_pem(&self) -> String {
        crate::pem::encode("PUBLIC KEY", &self.to_spki_der())
    }

    /// The PKCS #1 RSAPrivateKey (RSA only).
    pub fn rsa_pkcs1_der(&self) -> Option<Secret> {
        let KeyData::Rsa {
            n,
            e,
            private: Some(p),
            ..
        } = &self.data
        else {
            return None;
        };
        Some(secret(der::seq_vec(&[
            der::int(0),
            der::uint(n),
            der::uint(e),
            der::uint(&p.d),
            der::uint(&p.p),
            der::uint(&p.q),
            der::uint(&p.dmp1),
            der::uint(&p.dmq1),
            der::uint(&p.iqmp),
        ])))
    }

    /// The SEC 1 ECPrivateKey (RFC 5915), with the parameters and public
    /// key when `full`.
    fn ec_private_der(&self, full: bool) -> Option<Secret> {
        let KeyData::Ec {
            curve,
            point,
            d: Some(d),
        } = &self.data
        else {
            return None;
        };
        let mut parts = vec![der::int(1), der::octet_string(d)];
        if full {
            parts.push(der::explicit(0, &curve.params_der));
        }
        parts.push(der::explicit(1, &der::bit_string(point)));
        Some(secret(der::seq_vec(&parts)))
    }

    /// The PKCS #8 PrivateKeyInfo.
    pub fn to_pkcs8_der(&self) -> Option<Secret> {
        let inner: Secret = match &self.data {
            KeyData::Rsa { .. } => self.rsa_pkcs1_der()?,
            KeyData::Dsa { x: Some(x), .. } | KeyData::Dh { x: Some(x), .. } => {
                secret(der::uint(x))
            }
            KeyData::Ec { d: Some(_), .. } => self.ec_private_der(false)?,
            KeyData::X25519 {
                private: Some(s), ..
            }
            | KeyData::Ed25519 {
                private: Some(s), ..
            } => secret(der::octet_string(&s[..])),
            KeyData::X448 {
                private: Some(s), ..
            } => secret(der::octet_string(&s[..])),
            KeyData::Ed448 {
                private: Some(s), ..
            } => secret(der::octet_string(&s[..])),
            _ => return None,
        };
        Some(secret(der::seq_vec(&[
            der::int(0),
            self.algorithm_id(),
            der::octet_string(&inner),
        ])))
    }

    pub fn to_pkcs8_pem(&self) -> Option<Secret> {
        let d = self.to_pkcs8_der()?;
        Some(secret(crate::pem::encode("PRIVATE KEY", &d).into_bytes()))
    }

    // ------------------------------------------------------------ decoding

    pub fn from_spki_der(d: &[u8]) -> KResult<Key> {
        let e = |_| KeyError::Decode;
        let t = der::parse(d).map_err(e)?;
        if !t.is_universal(tag::SEQUENCE) {
            return Err(KeyError::Decode);
        }
        let mut r = t.reader();
        let alg = r.expect(tag::SEQUENCE).map_err(e)?;
        let bits = r.next().and_then(|x| x.bit_string()).map_err(e)?;
        let mut ar = alg.reader();
        let oid = ar.next().and_then(|x| x.oid()).map_err(e)?;
        let params = if ar.is_empty() {
            None
        } else {
            Some(ar.next().map_err(e)?)
        };
        match oid.as_str() {
            ids::RSA | ids::RSA_PSS => {
                let k = der::parse(&bits).map_err(e)?;
                let mut kr = k.reader();
                let n = kr.next().and_then(|x| x.uint()).map_err(e)?;
                let ex = kr.next().and_then(|x| x.uint()).map_err(e)?;
                Key::rsa_public(n, ex)
            }
            ids::DSA => {
                let params = params.ok_or(KeyError::Decode)?;
                let mut pr = params.reader();
                let p = pr.next().and_then(|x| x.uint()).map_err(e)?;
                let q = pr.next().and_then(|x| x.uint()).map_err(e)?;
                let g = pr.next().and_then(|x| x.uint()).map_err(e)?;
                let y = der::parse(&bits)
                    .and_then(|x| x.uint().map(|v| v.to_vec()))
                    .map_err(e)?;
                Key::dsa(p, q, g, Some(&y), None)
            }
            ids::DH | ids::DHX => {
                let params = params.ok_or(KeyError::Decode)?;
                let mut pr = params.reader();
                let p = pr.next().and_then(|x| x.uint()).map_err(e)?;
                let g = pr.next().and_then(|x| x.uint()).map_err(e)?;
                let q = if oid == ids::DHX {
                    // X9.42: p, g, q
                    Some(pr.next().and_then(|x| x.uint()).map_err(e)?.to_vec())
                } else {
                    None
                };
                let y = der::parse(&bits)
                    .and_then(|x| x.uint().map(|v| v.to_vec()))
                    .map_err(e)?;
                Key::dh(p, g, q.as_deref(), Some(&y), None)
            }
            ids::EC | ids::SM2 => {
                let params = params.ok_or(KeyError::Decode)?;
                let curve = Curve::from_params(&params)?;
                Key::ec(curve, Some(&bits), None)
            }
            ids::X25519 => Key::x25519(Some(&bits), None),
            ids::ED25519 => Key::ed25519(Some(&bits), None),
            ids::X448 => Key::x448(Some(&bits), None),
            ids::ED448 => Key::ed448(Some(&bits), None),
            _ => Err(KeyError::Unsupported),
        }
    }

    pub fn from_pkcs1_rsa_public(d: &[u8]) -> KResult<Key> {
        let e = |_| KeyError::Decode;
        let k = der::parse(d).map_err(e)?;
        let mut kr = k.reader();
        let n = kr.next().and_then(|x| x.uint()).map_err(e)?;
        let ex = kr.next().and_then(|x| x.uint()).map_err(e)?;
        if !kr.is_empty() {
            return Err(KeyError::Decode);
        }
        Key::rsa_public(n, ex)
    }

    pub fn from_pkcs1_rsa_private(d: &[u8]) -> KResult<Key> {
        let e = |_| KeyError::Decode;
        let t = der::parse(d).map_err(e)?;
        if !t.is_universal(tag::SEQUENCE) {
            return Err(KeyError::Decode);
        }
        let mut r = t.reader();
        let mut v = Vec::new();
        while !r.is_empty() && v.len() < 9 {
            v.push(r.next().and_then(|x| x.uint()).map_err(e)?);
        }
        if v.len() != 9 || v[0] != [0] && v[0] != [1] {
            return Err(KeyError::Decode);
        }
        Key::rsa_private(
            v[1],
            v[2],
            v[3],
            Some(v[4]),
            Some(v[5]),
            Some(v[6]),
            Some(v[7]),
            Some(v[8]),
        )
    }

    /// OpenSSL's traditional DSA private key: SEQUENCE {0, p, q, g, y, x}.
    pub fn from_dsa_private_der(d: &[u8]) -> KResult<Key> {
        let e = |_| KeyError::Decode;
        let t = der::parse(d).map_err(e)?;
        let mut r = t.reader();
        let mut v = Vec::new();
        while !r.is_empty() {
            v.push(r.next().and_then(|x| x.uint()).map_err(e)?);
        }
        if v.len() != 6 {
            return Err(KeyError::Decode);
        }
        Key::dsa(v[1], v[2], v[3], Some(v[4]), Some(v[5]))
    }

    /// SEC 1 ECPrivateKey; `curve` from the PKCS #8 algorithm parameters
    /// when the structure doesn't carry them.
    pub fn from_sec1_der(d: &[u8], curve: Option<Curve>) -> KResult<Key> {
        let e = |_| KeyError::Decode;
        let t = der::parse(d).map_err(e)?;
        let mut r = t.reader();
        let ver = r.next().and_then(|x| x.small_int()).map_err(e)?;
        if ver != 1 {
            return Err(KeyError::Decode);
        }
        let priv_ = r.expect(tag::OCTET_STRING).map_err(e)?.content.to_vec();
        let priv_ = secret(priv_);
        let mut curve = curve;
        let mut point = None;
        while !r.is_empty() {
            let x = r.next().map_err(e)?;
            if x.is_context(0) {
                let inner = der::parse(x.content).map_err(e)?;
                curve = Some(Curve::from_params(&inner)?);
            } else if x.is_context(1) {
                let inner = der::parse(x.content).map_err(e)?;
                point = Some(inner.bit_string().map_err(e)?);
            }
        }
        let curve = curve.ok_or(KeyError::Decode)?;
        Key::ec(curve, point.as_deref(), Some(&priv_))
    }

    /// PKCS #8 PrivateKeyInfo / OneAsymmetricKey.
    pub fn from_pkcs8_der(d: &[u8]) -> KResult<Key> {
        let e = |_| KeyError::Decode;
        let t = der::parse(d).map_err(e)?;
        if !t.is_universal(tag::SEQUENCE) {
            return Err(KeyError::Decode);
        }
        let mut r = t.reader();
        let ver = r.next().and_then(|x| x.small_int()).map_err(e)?;
        if ver != 0 && ver != 1 {
            return Err(KeyError::Decode);
        }
        let alg = r.expect(tag::SEQUENCE).map_err(e)?;
        let pk = secret(
            r.expect(tag::OCTET_STRING)
                .map_err(e)?
                .string_bytes()
                .map_err(e)?,
        );
        let mut ar = alg.reader();
        let oid = ar.next().and_then(|x| x.oid()).map_err(e)?;
        let params = if ar.is_empty() {
            None
        } else {
            Some(ar.next().map_err(e)?)
        };
        let curve_key = |n: usize| -> KResult<Secret> {
            let inner = der::parse(&pk).map_err(e)?;
            if !inner.is_universal(tag::OCTET_STRING) || inner.content.len() != n {
                return Err(KeyError::Decode);
            }
            Ok(secret(inner.content.to_vec()))
        };
        match oid.as_str() {
            ids::RSA | ids::RSA_PSS => Key::from_pkcs1_rsa_private(&pk),
            ids::DSA => {
                let params = params.ok_or(KeyError::Decode)?;
                let mut pr = params.reader();
                let p = pr.next().and_then(|x| x.uint()).map_err(e)?;
                let q = pr.next().and_then(|x| x.uint()).map_err(e)?;
                let g = pr.next().and_then(|x| x.uint()).map_err(e)?;
                let x = secret(
                    der::parse(&pk)
                        .and_then(|x| x.uint().map(|v| v.to_vec()))
                        .map_err(e)?,
                );
                Key::dsa(p, q, g, None, Some(&x))
            }
            ids::DH | ids::DHX => {
                let params = params.ok_or(KeyError::Decode)?;
                let mut pr = params.reader();
                let p = pr.next().and_then(|x| x.uint()).map_err(e)?;
                let g = pr.next().and_then(|x| x.uint()).map_err(e)?;
                let q = if oid == ids::DHX {
                    Some(pr.next().and_then(|x| x.uint()).map_err(e)?.to_vec())
                } else {
                    None
                };
                let x = secret(
                    der::parse(&pk)
                        .and_then(|x| x.uint().map(|v| v.to_vec()))
                        .map_err(e)?,
                );
                Key::dh(p, g, q.as_deref(), None, Some(&x))
            }
            ids::EC | ids::SM2 => {
                let curve = match params {
                    Some(p) => Some(Curve::from_params(&p)?),
                    None => None,
                };
                Key::from_sec1_der(&pk, curve)
            }
            ids::X25519 => Key::x25519(None, Some(&curve_key(32)?)),
            ids::ED25519 => Key::ed25519(None, Some(&curve_key(32)?)),
            ids::X448 => Key::x448(None, Some(&curve_key(56)?)),
            ids::ED448 => Key::ed448(None, Some(&curve_key(57)?)),
            _ => Err(KeyError::Unsupported),
        }
    }

    // ------------------------------------------------------------ details

    pub fn details(&self) -> Details {
        let mut f: Vec<(&'static str, Detail)> = Vec::new();
        let b = |v: &[u8]| Detail::Bin(v.to_vec());
        match &self.data {
            KeyData::Rsa { n, e, private, .. } => {
                f.push(("n", b(n)));
                f.push(("e", b(e)));
                if let Some(p) = private {
                    f.push(("d", b(&p.d)));
                    f.push(("p", b(&p.p)));
                    f.push(("q", b(&p.q)));
                    f.push(("dmp1", b(&p.dmp1)));
                    f.push(("dmq1", b(&p.dmq1)));
                    f.push(("iqmp", b(&p.iqmp)));
                }
            }
            KeyData::Dsa { p, q, g, y, x } => {
                f.push(("p", b(p)));
                f.push(("q", b(q)));
                f.push(("g", b(g)));
                if let Some(x) = x {
                    f.push(("priv_key", b(x)));
                }
                f.push(("pub_key", b(y)));
            }
            KeyData::Dh { p, g, y, x, .. } => {
                f.push(("p", b(p)));
                f.push(("g", b(g)));
                if let Some(x) = x {
                    f.push(("priv_key", b(x)));
                }
                f.push(("pub_key", b(y)));
            }
            KeyData::Ec { curve, point, d } => {
                if let Some(name) = curve.name() {
                    f.push(("curve_name", Detail::Str(name.to_string())));
                }
                if let Some(oid) = curve.oid {
                    f.push(("curve_oid", Detail::Str(oid.to_string())));
                }
                let flen = curve.field_len();
                if point.len() == 1 + 2 * flen {
                    f.push(("x", b(&point[1..1 + flen])));
                    f.push(("y", b(&point[1 + flen..])));
                }
                if let Some(d) = d {
                    f.push(("d", b(d)));
                }
            }
            KeyData::X25519 { public, private } | KeyData::Ed25519 { public, private } => {
                if let Some(s) = private {
                    f.push(("priv_key", b(&s[..])));
                }
                f.push(("pub_key", b(public)));
            }
            KeyData::X448 { public, private } => {
                if let Some(s) = private {
                    f.push(("priv_key", b(&s[..])));
                }
                f.push(("pub_key", b(public)));
            }
            KeyData::Ed448 { public, private } => {
                if let Some(s) = private {
                    f.push(("priv_key", b(&s[..])));
                }
                f.push(("pub_key", b(public)));
            }
        }
        Details {
            bits: self.bits(),
            key_pem: self.to_spki_pem(),
            typ: Some(self.key_type()),
            fields: f,
        }
    }

    /// Whether two keys have the same public part.
    pub fn public_eq(&self, other: &Key) -> bool {
        self.to_spki_der() == other.to_spki_der()
    }

    // ------------------------------------------------------------ operations

    /// Signs `data` with digest `md` (none for the EdDSA keys, which sign
    /// the message itself). RSA uses PKCS #1 v1.5, or PSS when `pss`.
    pub fn sign(&self, md: Option<Md>, data: &[u8], pss: bool) -> KResult<Vec<u8>> {
        match &self.data {
            KeyData::Rsa {
                private: Some(p), ..
            } => {
                let md = md.ok_or(KeyError::Unsupported)?;
                let h = md.digest(data);
                if pss {
                    return rsa_pss_sign(&p.key, md, &h, self.size());
                }
                let t = md.digest_info(&h).ok_or(KeyError::Unsupported)?;
                rsa_pkcs1_sign_raw(&p.key, &t, self.size())
            }
            KeyData::Dsa {
                p,
                q,
                g,
                y,
                x: Some(x),
            } => {
                let md = md.ok_or(KeyError::Unsupported)?;
                let h = md.digest(data);
                let params = purecrypto::dsa::DsaParams::new_nonstandard(uint(p), uint(q), uint(g))
                    .map_err(|_| KeyError::Invalid)?;
                let sk = purecrypto::dsa::DsaPrivateKey::new(params, uint(x))
                    .map_err(|_| KeyError::Invalid)?;
                let _ = y;
                let hash = match md {
                    Md::H(a) => a,
                    _ => purecrypto::hash::HashAlgorithm::Sha256,
                };
                let sig = sk
                    .sign_prehash_with(hash, &h)
                    .map_err(|_| KeyError::Invalid)?;
                Ok(sig.to_der())
            }
            KeyData::Ec {
                curve, d: Some(d), ..
            } => {
                let md = md.ok_or(KeyError::Unsupported)?;
                let h = md.digest(data);
                let sk = BoxedEcdsaPrivateKey::from_bytes(curve.boxed.clone(), d)
                    .map_err(|_| KeyError::Invalid)?;
                let sig = ecdsa_sign(&sk, md, &h)?;
                Ok(sig.to_der(curve.boxed.clone()))
            }
            KeyData::Ed25519 {
                private: Some(s), ..
            } => Ok(Ed25519PrivateKey::from_bytes(**s)
                .sign(data)
                .to_bytes()
                .to_vec()),
            KeyData::Ed448 {
                private: Some(s), ..
            } => Ok(Ed448PrivateKey::from_bytes(**s)
                .sign(data)
                .to_bytes()
                .to_vec()),
            _ => Err(KeyError::Unsupported),
        }
    }

    /// Verifies a signature; Ok(false) on mismatch.
    pub fn verify(&self, md: Option<Md>, data: &[u8], sig: &[u8], pss: bool) -> KResult<bool> {
        match &self.data {
            KeyData::Rsa { public, n, .. } => {
                let Some(public) = public else {
                    return Ok(false);
                };
                let md = md.ok_or(KeyError::Unsupported)?;
                let h = md.digest(data);
                let k = strip(n).len();
                if sig.len() != k {
                    return Ok(false);
                }
                let Ok(em) = public.raw_public(sig) else {
                    return Ok(false);
                };
                if pss {
                    return Ok(pss_verify(&em, &h, md, bits_of(n) - 1, None));
                }
                let t = md.digest_info(&h).ok_or(KeyError::Unsupported)?;
                Ok(pkcs1_type1_check(&em, k).is_some_and(|m| m == t))
            }
            KeyData::Dsa { p, q, g, y, .. } => {
                let md = md.ok_or(KeyError::Unsupported)?;
                let h = md.digest(data);
                let params = purecrypto::dsa::DsaParams::new_nonstandard(uint(p), uint(q), uint(g))
                    .map_err(|_| KeyError::Invalid)?;
                let pk = purecrypto::dsa::DsaPublicKey::new(params, uint(y))
                    .map_err(|_| KeyError::Invalid)?;
                let Ok(s) = purecrypto::dsa::DsaSignature::from_der(sig) else {
                    return Ok(false);
                };
                Ok(pk.verify_prehash(&h, &s).is_ok())
            }
            KeyData::Ec { curve, point, .. } => {
                let md = md.ok_or(KeyError::Unsupported)?;
                let h = md.digest(data);
                let pk = BoxedEcdsaPublicKey::from_sec1(curve.boxed.clone(), point)
                    .map_err(|_| KeyError::Invalid)?;
                let Ok(s) = BoxedEcdsaSignature::from_der(sig) else {
                    return Ok(false);
                };
                Ok(pk.verify_prehash(&h, &s).is_ok())
            }
            KeyData::Ed25519 { public, .. } => {
                let Ok(s) = <[u8; 64]>::try_from(sig) else {
                    return Ok(false);
                };
                Ok(Ed25519PublicKey::from_bytes(*public)
                    .verify(data, &Ed25519Signature::from_bytes(s))
                    .is_ok())
            }
            KeyData::Ed448 { public, .. } => {
                let Ok(s) = <[u8; 114]>::try_from(sig) else {
                    return Ok(false);
                };
                Ok(Ed448PublicKey::from_bytes(*public)
                    .verify(data, &Ed448Signature::from_bytes(s))
                    .is_ok())
            }
            _ => Err(KeyError::Unsupported),
        }
    }

    /// Key agreement with a peer's public key (DH, ECDH, X25519, X448).
    pub fn derive(&self, peer: &Key) -> KResult<Vec<u8>> {
        match (&self.data, &peer.data) {
            (KeyData::Dh { p, x: Some(x), .. }, KeyData::Dh { p: pp, y, .. }) => {
                if p != pp {
                    return Err(KeyError::Invalid);
                }
                Key::dh_compute(p, x, y)
            }
            (
                KeyData::Ec {
                    curve, d: Some(d), ..
                },
                KeyData::Ec {
                    curve: pc, point, ..
                },
            ) => {
                if curve.params_der != pc.params_der {
                    return Err(KeyError::Invalid);
                }
                ecdh(curve, d, point)
            }
            (
                KeyData::X25519 {
                    private: Some(s), ..
                },
                KeyData::X25519 { public, .. },
            ) => X25519PrivateKey::from_bytes(**s)
                .diffie_hellman(public)
                .map(|v| v.to_vec())
                .map_err(|_| KeyError::Invalid),
            (
                KeyData::X448 {
                    private: Some(s), ..
                },
                KeyData::X448 { public, .. },
            ) => X448PrivateKey::from_bytes(**s)
                .diffie_hellman(public)
                .map(|v| v.to_vec())
                .map_err(|_| KeyError::Invalid),
            _ => Err(KeyError::Invalid),
        }
    }

    /// The DH shared secret y^x mod p, left-padded to the size of p
    /// (RFC 2631 §2.1.2; observed on PHP 8.5 for openssl_pkey_derive).
    pub fn dh_compute(p: &[u8], x: &[u8], y: &[u8]) -> KResult<Vec<u8>> {
        let pb = strip(p);
        let yb = strip(y);
        // 1 < y < p-1 (public check).
        if bits_of(&yb) <= 1 || uint(&yb).lt(&BoxedUint::from_u64(2)) || !uint(&yb).lt(&uint(&pb)) {
            return Err(KeyError::Invalid);
        }
        let r = modpow_ct(&yb, x, &pb)?;
        Ok(pad_to(&r, pb.len()))
    }

    // RSA encryption helpers --------------------------------------------

    pub fn rsa_public_key(&self) -> Option<&BoxedRsaPublicKey> {
        match &self.data {
            KeyData::Rsa { public, .. } => public.as_ref(),
            _ => None,
        }
    }

    pub fn rsa_private_key(&self) -> Option<&BoxedRsaPrivateKey> {
        match &self.data {
            KeyData::Rsa {
                private: Some(p), ..
            } => Some(&p.key),
            _ => None,
        }
    }
}

fn ecdh(curve: &Curve, d: &[u8], point: &[u8]) -> KResult<Vec<u8>> {
    let sk = purecrypto::ec::BoxedEcdhPrivateKey::from_bytes(curve.boxed.clone(), d)
        .map_err(|_| KeyError::Invalid)?;
    let pk = BoxedEcdsaPublicKey::from_sec1(curve.boxed.clone(), point)
        .map_err(|_| KeyError::Invalid)?;
    sk.diffie_hellman(&pk).map_err(|_| KeyError::Invalid)
}

fn ecdsa_sign(sk: &BoxedEcdsaPrivateKey, md: Md, h: &[u8]) -> KResult<BoxedEcdsaSignature> {
    let alg = match md {
        Md::H(a) => a,
        _ => purecrypto::hash::HashAlgorithm::Sha256,
    };
    purecrypto::dispatch_digest!(alg, |D| {
        sk.sign_prehash::<D>(h).map_err(|_| KeyError::Invalid)
    }, _ => Err(KeyError::Unsupported))
}

/// EMSA-PKCS1-v1_5 (RFC 8017 §9.2) of an encoded DigestInfo `t`, then
/// RSASP1. The encoding is public; the private operation is purecrypto's
/// blinded constant-time one.
pub fn rsa_pkcs1_sign_raw(k: &BoxedRsaPrivateKey, t: &[u8], klen: usize) -> KResult<Vec<u8>> {
    if t.len() + 11 > klen {
        return Err(KeyError::Invalid);
    }
    let mut em = vec![0u8; klen];
    em[1] = 1;
    for b in &mut em[2..klen - t.len() - 1] {
        *b = 0xff;
    }
    em[klen - t.len()..].copy_from_slice(t);
    k.raw_private(&em).map_err(|_| KeyError::Invalid)
}

/// Checks an EMSA-PKCS1-v1_5 / RSA "type 1" block (public data) and
/// returns the payload.
pub fn pkcs1_type1_check(em: &[u8], k: usize) -> Option<Vec<u8>> {
    if em.len() != k || k < 11 || em[0] != 0 || em[1] != 1 {
        return None;
    }
    let mut i = 2;
    while i < em.len() && em[i] == 0xff {
        i += 1;
    }
    if i < 10 || i >= em.len() || em[i] != 0 {
        return None;
    }
    Some(em[i + 1..].to_vec())
}

/// MGF1 (RFC 8017 §B.2.1).
pub fn mgf1(md: Md, seed: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + md.output_len());
    let mut c = 0u32;
    while out.len() < len {
        let mut h = crate::digest::Hasher::new(md);
        h.update(seed);
        h.update(&c.to_be_bytes());
        out.extend_from_slice(&h.finish());
        c += 1;
    }
    out.truncate(len);
    out
}

/// RSASSA-PSS signing (RFC 8017 §8.1, salt length = digest length as
/// OpenSSL's default for signing with PSS padding... observed as the
/// maximum? kept at the digest length).
fn rsa_pss_sign(k: &BoxedRsaPrivateKey, md: Md, h: &[u8], klen: usize) -> KResult<Vec<u8>> {
    let mod_bits = k.modulus().bit_len();
    let em_bits = mod_bits - 1;
    let em_len = em_bits.div_ceil(8);
    let hlen = md.output_len();
    // Observed on PHP 8.5: OPENSSL_PKCS1_PSS_PADDING signs with the
    // maximum salt length.
    let slen = em_len.checked_sub(hlen + 2).ok_or(KeyError::Invalid)?;
    let mut salt = vec![0u8; slen];
    crate::random_bytes(&mut salt);
    let mut mh = crate::digest::Hasher::new(md);
    mh.update(&[0u8; 8]);
    mh.update(h);
    mh.update(&salt);
    let hh = mh.finish();
    let mut db = vec![0u8; em_len - hlen - 1];
    let ps_len = em_len - slen - hlen - 2;
    db[ps_len] = 1;
    db[ps_len + 1..].copy_from_slice(&salt);
    let mask = mgf1(md, &hh, db.len());
    for (a, b) in db.iter_mut().zip(&mask) {
        *a ^= b;
    }
    let top = 8 * em_len - em_bits;
    db[0] &= 0xff >> top;
    let mut em = db;
    em.extend_from_slice(&hh);
    em.push(0xbc);
    let mut full = vec![0u8; klen - em.len()];
    full.extend_from_slice(&em);
    k.raw_private(&full).map_err(|_| KeyError::Invalid)
}

/// RSASSA-PSS verification (RFC 8017 §9.1.2) on the recovered block;
/// any salt length unless `slen` is given.
pub fn pss_verify(em_full: &[u8], h: &[u8], md: Md, em_bits: usize, slen: Option<usize>) -> bool {
    let em_len = em_bits.div_ceil(8);
    if em_full.len() < em_len {
        return false;
    }
    let skip = em_full.len() - em_len;
    if em_full[..skip].iter().any(|&b| b != 0) {
        return false;
    }
    let em = &em_full[skip..];
    let hlen = md.output_len();
    if em_len < hlen + 2 || em[em_len - 1] != 0xbc {
        return false;
    }
    let db_len = em_len - hlen - 1;
    let masked = &em[..db_len];
    let hh = &em[db_len..db_len + hlen];
    let top = 8 * em_len - em_bits;
    if top > 0 && masked[0] & !(0xffu8 >> top) != 0 {
        return false;
    }
    let mask = mgf1(md, hh, db_len);
    let mut db: Vec<u8> = masked.iter().zip(&mask).map(|(a, b)| a ^ b).collect();
    db[0] &= 0xff >> top;
    let Some(one) = db.iter().position(|&b| b != 0) else {
        return false;
    };
    if db[one] != 1 {
        return false;
    }
    let salt = &db[one + 1..];
    if let Some(s) = slen
        && salt.len() != s
    {
        return false;
    }
    let mut mh = crate::digest::Hasher::new(md);
    mh.update(&[0u8; 8]);
    mh.update(h);
    mh.update(salt);
    mh.finish() == hh
}

// ------------------------------------------------------------ RSA encryption

/// The padding modes of openssl_public_encrypt() and friends.
pub const PAD_PKCS1: i64 = 1;
pub const PAD_NONE: i64 = 3;
pub const PAD_OAEP: i64 = 4;

use crate::err::{E, errs};

fn rsa_k(key: &Key) -> usize {
    match &key.data {
        KeyData::Rsa { n, .. } => strip(n).len(),
        _ => 0,
    }
}

/// Left-pads `data` to `k` bytes (RSA input octet strings shorter than the
/// modulus are integers like any other; OpenSSL accepts them).
fn lpad(data: &[u8], k: usize) -> Vec<u8> {
    let mut v = vec![0u8; k.saturating_sub(data.len())];
    v.extend_from_slice(data);
    v
}

/// openssl_public_encrypt() (RFC 8017 §7.1, §7.2).
pub fn rsa_public_encrypt(key: &Key, data: &[u8], padding: i64, md: Md) -> Result<Vec<u8>, E> {
    let Some(pk) = key.rsa_public_key() else {
        return Err(errs::EVP_OP_NOT_SUPPORTED_KEYTYPE);
    };
    let k = rsa_k(key);
    let mut rng = purecrypto::rng::OsRng;
    match padding {
        PAD_PKCS1 => {
            if data.len() + 11 > k {
                return Err(errs::RSA_DATA_TOO_LARGE_FOR_KEY);
            }
            // EME-PKCS1-v1_5 (RFC 8017 §7.2.1): 00 02 PS 00 M with
            // nonzero random PS, drawn in one batch.
            let ps_len = k - data.len() - 3;
            let mut em = Zeroizing::new(vec![0u8; k]);
            em[1] = 2;
            let mut ps = vec![0u8; ps_len + 64];
            crate::random_bytes(&mut ps);
            let mut j = 0;
            for i in 0..ps_len {
                loop {
                    if j == ps.len() {
                        crate::random_bytes(&mut ps);
                        j = 0;
                    }
                    let b = ps[j];
                    j += 1;
                    if b != 0 {
                        em[2 + i] = b;
                        break;
                    }
                }
            }
            em[k - data.len()..].copy_from_slice(data);
            let _ = &mut rng;
            pk.raw_public(&em)
                .map_err(|_| errs::RSA_DATA_TOO_LARGE_FOR_KEY)
        }
        PAD_OAEP => {
            let hl = md.output_len();
            if data.len() + 2 * hl + 2 > k {
                return Err(errs::RSA_DATA_TOO_LARGE_FOR_KEY);
            }
            let Md::H(alg) = md else {
                return Err(errs::EVP_UNSUPPORTED);
            };
            purecrypto::dispatch_digest!(alg, |D| {
                pk.encrypt_oaep::<D, _>(data, &[], &mut rng)
                    .map_err(|_| errs::RSA_DATA_TOO_LARGE_FOR_KEY)
            }, _ => Err(errs::EVP_UNSUPPORTED))
        }
        PAD_NONE => {
            if data.len() > k {
                return Err(errs::RSA_DATA_TOO_LARGE_FOR_KEY);
            }
            if data.len() < k {
                return Err(errs::RSA_DATA_TOO_SMALL);
            }
            pk.raw_public(data)
                .map_err(|_| errs::RSA_DATA_TOO_LARGE_FOR_MOD)
        }
        _ => Err(errs::PROV_ILLEGAL_PADDING),
    }
}

/// openssl_private_decrypt(). PKCS #1 v1.5 uses implicit rejection (a
/// key-derived pseudo-random result instead of a padding error; observed
/// on PHP 8.5 with OpenSSL 3.5, and RFC 8017 §7.2.2 note), so the padding
/// outcome never leaks.
pub fn rsa_private_decrypt(key: &Key, data: &[u8], padding: i64, md: Md) -> Result<Secret, E> {
    let Some(sk) = key.rsa_private_key() else {
        return Err(errs::EVP_OP_NOT_SUPPORTED_KEYTYPE);
    };
    let k = rsa_k(key);
    if data.len() > k {
        return Err(errs::RSA_DATA_GREATER_THAN_MOD);
    }
    let ct = lpad(data, k);
    match padding {
        PAD_PKCS1 => sk
            .decrypt_pkcs1v15_implicit(&ct)
            .map(secret)
            .map_err(|_| errs::RSA_DATA_GREATER_THAN_MOD),
        PAD_OAEP => {
            let Md::H(alg) = md else {
                return Err(errs::EVP_UNSUPPORTED);
            };
            purecrypto::dispatch_digest!(alg, |D| {
                sk.decrypt_oaep::<D>(&ct, &[])
                    .map(secret)
                    .map_err(|_| errs::RSA_OAEP_DECODING)
            }, _ => Err(errs::EVP_UNSUPPORTED))
        }
        PAD_NONE => sk
            .raw_private(&ct)
            .map(secret)
            .map_err(|_| errs::RSA_DATA_GREATER_THAN_MOD),
        _ => Err(errs::PROV_ILLEGAL_PADDING),
    }
}

/// openssl_private_encrypt(): PKCS #1 v1.5 type 1 or no padding.
pub fn rsa_private_encrypt(key: &Key, data: &[u8], padding: i64) -> Result<Vec<u8>, E> {
    let Some(sk) = key.rsa_private_key() else {
        return Err(errs::EVP_OP_NOT_SUPPORTED_KEYTYPE);
    };
    let k = rsa_k(key);
    match padding {
        PAD_PKCS1 => {
            if data.len() + 11 > k {
                return Err(errs::RSA_DATA_TOO_LARGE_FOR_KEY);
            }
            rsa_pkcs1_sign_raw(sk, data, k).map_err(|_| errs::RSA_DATA_TOO_LARGE_FOR_KEY)
        }
        PAD_NONE => {
            if data.len() > k {
                return Err(errs::RSA_DATA_TOO_LARGE_FOR_KEY);
            }
            if data.len() < k {
                return Err(errs::RSA_DATA_TOO_SMALL);
            }
            sk.raw_private(data)
                .map_err(|_| errs::RSA_DATA_TOO_LARGE_FOR_MOD)
        }
        _ => Err(errs::PROV_ILLEGAL_PADDING),
    }
}

/// openssl_public_decrypt(): recovers a type-1 block or the raw value.
pub fn rsa_public_decrypt(key: &Key, data: &[u8], padding: i64) -> Result<Vec<u8>, Vec<E>> {
    let Some(pk) = key.rsa_public_key() else {
        return Err(vec![errs::EVP_OP_NOT_SUPPORTED_KEYTYPE]);
    };
    let k = rsa_k(key);
    if data.len() > k {
        return Err(vec![errs::RSA_DATA_GREATER_THAN_MOD]);
    }
    let ct = lpad(data, k);
    match padding {
        PAD_PKCS1 => {
            let em = pk
                .raw_public(&ct)
                .map_err(|_| vec![errs::RSA_DATA_GREATER_THAN_MOD])?;
            pkcs1_type1_check(&em, k).ok_or_else(|| {
                vec![
                    errs::RSA_INVALID_PADDING,
                    errs::RSA_PADDING_CHECK_FAILED,
                    errs::PROV_RSA_LIB,
                    errs::EVP_PROVIDER_SIG_FAILURE,
                ]
            })
        }
        PAD_NONE => pk
            .raw_public(&ct)
            .map_err(|_| vec![errs::RSA_DATA_GREATER_THAN_MOD]),
        _ => Err(vec![errs::PROV_ILLEGAL_PADDING]),
    }
}

// ------------------------------------------------------------ PEM loading

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadError {
    /// No PEM block of a usable kind.
    NoStartLine,
    /// The key is encrypted and no passphrase was given.
    NoPassphrase,
    /// Wrong passphrase.
    BadDecrypt,
    /// A block was found but doesn't decode.
    Decode,
}

/// The first private key in PEM text (PKCS #8, encrypted PKCS #8, or the
/// traditional RSA/DSA/EC forms with optional RFC 1421 encryption).
pub fn load_private_pem(data: &[u8], pass: Option<&[u8]>) -> Result<Key, LoadError> {
    let mut found_err = LoadError::NoStartLine;
    for b in crate::pem::parse_all(data) {
        let r = match b.label.as_str() {
            "PRIVATE KEY" => Key::from_pkcs8_der(&b.data).map_err(|_| LoadError::Decode),
            "ENCRYPTED PRIVATE KEY" => {
                let Some(p) = pass else {
                    found_err = LoadError::NoPassphrase;
                    continue;
                };
                decrypt_pkcs8(&b.data, p)
                    .and_then(|d| Key::from_pkcs8_der(&d).map_err(|_| LoadError::Decode))
            }
            "RSA PRIVATE KEY" | "DSA PRIVATE KEY" | "EC PRIVATE KEY" => {
                let body = if b
                    .header("Proc-Type")
                    .is_some_and(|v| v.contains("ENCRYPTED"))
                {
                    let Some(p) = pass else {
                        found_err = LoadError::NoPassphrase;
                        continue;
                    };
                    let dek = b.header("DEK-Info").ok_or(LoadError::Decode);
                    match dek.and_then(|d| {
                        crate::pbe::pem_decrypt(d, p, &b.data).ok_or(LoadError::BadDecrypt)
                    }) {
                        Ok(v) => secret(v),
                        Err(e) => {
                            found_err = e;
                            continue;
                        }
                    }
                } else {
                    secret(b.data.clone())
                };
                match b.label.as_str() {
                    "RSA PRIVATE KEY" => Key::from_pkcs1_rsa_private(&body),
                    "DSA PRIVATE KEY" => Key::from_dsa_private_der(&body),
                    _ => Key::from_sec1_der(&body, None),
                }
                .map_err(|_| LoadError::BadDecrypt)
            }
            _ => continue,
        };
        match r {
            Ok(k) => return Ok(k),
            Err(e) => found_err = e,
        }
    }
    Err(found_err)
}

/// Decrypts an EncryptedPrivateKeyInfo.
pub fn decrypt_pkcs8(d: &[u8], pass: &[u8]) -> Result<Secret, LoadError> {
    let t = der::parse(d).map_err(|_| LoadError::Decode)?;
    let mut r = t.reader();
    let alg = r.expect(tag::SEQUENCE).map_err(|_| LoadError::Decode)?;
    let ct = r
        .expect(tag::OCTET_STRING)
        .and_then(|x| x.string_bytes())
        .map_err(|_| LoadError::Decode)?;
    crate::pbe::decrypt(&alg, &ct, pass)
        .map(secret)
        .map_err(|e| match e {
            crate::pbe::PbeError::BadDecrypt => LoadError::BadDecrypt,
            _ => LoadError::Decode,
        })
}

/// The first public key in PEM text: SubjectPublicKeyInfo, PKCS #1
/// RSAPublicKey, or a certificate's key.
pub fn load_public_pem(data: &[u8]) -> Result<Key, LoadError> {
    let mut found_err = LoadError::NoStartLine;
    for b in crate::pem::parse_all(data) {
        let r = match b.label.as_str() {
            "PUBLIC KEY" => Key::from_spki_der(&b.data),
            "RSA PUBLIC KEY" => Key::from_pkcs1_rsa_public(&b.data),
            "CERTIFICATE" | "X509 CERTIFICATE" | "TRUSTED CERTIFICATE" => {
                match crate::x509::Cert::from_der(&b.data) {
                    Ok(c) => c.public_key(),
                    Err(_) => Err(KeyError::Decode),
                }
            }
            _ => continue,
        };
        match r {
            Ok(k) => return Ok(k),
            Err(_) => found_err = LoadError::Decode,
        }
    }
    Err(found_err)
}

impl Key {
    /// openssl_pkey_export(): PKCS #8 PEM, encrypted with PBES2 when a
    /// passphrase is given.
    pub fn export_private_pem(
        &self,
        pass: Option<&[u8]>,
        cipher: crate::pbe::CbcCipher,
    ) -> Option<Secret> {
        let der = self.to_pkcs8_der()?;
        match pass {
            None => Some(secret(crate::pem::encode("PRIVATE KEY", &der).into_bytes())),
            Some(p) => {
                let enc = crate::pbe::pbes2_encrypt(cipher, p, &der)?;
                Some(secret(
                    crate::pem::encode("ENCRYPTED PRIVATE KEY", &enc).into_bytes(),
                ))
            }
        }
    }
}
