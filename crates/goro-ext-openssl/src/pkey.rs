//! EVP_PKEY emulation: RSA, DSA, DH, EC and X25519/Ed25519/X448/Ed448 keys,
//! their DER/PEM encodings, generation and the low-level operations.

use crate::asn1::{self, Tlv};
use crate::cipher::{self, CipherSpec};
use crate::digest::{self, Md};
use crate::ec::{Group, Point};
use crate::ecx::{self, EcxKind};
use crate::err;
use num_bigint_dig::{BigUint, RandBigInt};
use num_integer::Integer;
use num_traits::{One, Zero};

pub const OID_RSA: &str = "1.2.840.113549.1.1.1";
pub const OID_RSA_PSS: &str = "1.2.840.113549.1.1.10";
pub const OID_DSA: &str = "1.2.840.10040.4.1";
pub const OID_DH: &str = "1.2.840.113549.1.3.1";
pub const OID_DHX: &str = "1.2.840.10046.2.1";
pub const OID_EC: &str = "1.2.840.10045.2.1";
pub const OID_SM2: &str = "1.2.156.10197.1.301";
pub const OID_PRIME_FIELD: &str = "1.2.840.10045.1.1";

#[derive(Clone, Debug)]
pub struct Rsa {
    pub n: BigUint,
    pub e: BigUint,
    pub d: Option<BigUint>,
    pub p: Option<BigUint>,
    pub q: Option<BigUint>,
    pub dmp1: Option<BigUint>,
    pub dmq1: Option<BigUint>,
    pub iqmp: Option<BigUint>,
}

#[derive(Clone, Debug)]
pub struct Ffc {
    pub p: BigUint,
    pub q: Option<BigUint>,
    pub g: BigUint,
    pub pub_key: Option<BigUint>,
    pub priv_key: Option<BigUint>,
}

#[derive(Clone, Debug)]
pub struct EcKey {
    pub group: Group,
    pub pub_key: Option<Point>,
    pub priv_key: Option<BigUint>,
    /// SM2 keys share the EC representation.
    pub sm2: bool,
}

#[derive(Clone, Debug)]
pub struct Ecx {
    pub kind: EcxKind,
    pub pub_key: Vec<u8>,
    pub priv_key: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub enum PKey {
    Rsa(Rsa),
    Dsa(Ffc),
    Dh(Ffc),
    Ec(EcKey),
    Ecx(Ecx),
}

pub const KEYTYPE_RSA: i64 = 0;
pub const KEYTYPE_DSA: i64 = 1;
pub const KEYTYPE_DH: i64 = 2;
pub const KEYTYPE_EC: i64 = 3;

fn bn(b: &[u8]) -> BigUint {
    BigUint::from_bytes_be(b)
}

fn bytes_len(n: &BigUint) -> usize {
    n.bits().div_ceil(8)
}

pub fn pad_be(n: &BigUint, len: usize) -> Vec<u8> {
    crate::ec::pad(n, len)
}

impl PKey {
    pub fn type_const(&self) -> i64 {
        match self {
            PKey::Rsa(_) => KEYTYPE_RSA,
            PKey::Dsa(_) => KEYTYPE_DSA,
            PKey::Dh(_) => KEYTYPE_DH,
            PKey::Ec(_) => KEYTYPE_EC,
            PKey::Ecx(x) => match x.kind {
                EcxKind::X25519 => 4,
                EcxKind::Ed25519 => 5,
                EcxKind::X448 => 6,
                EcxKind::Ed448 => 7,
            },
        }
    }

    /// EVP_PKEY_bits()
    pub fn bits(&self) -> i64 {
        match self {
            PKey::Rsa(r) => r.n.bits() as i64,
            PKey::Dsa(f) | PKey::Dh(f) => f.p.bits() as i64,
            PKey::Ec(e) => e.group.order_bits() as i64,
            PKey::Ecx(x) => x.kind.bits(),
        }
    }

    pub fn has_private(&self) -> bool {
        match self {
            PKey::Rsa(r) => r.d.is_some(),
            PKey::Dsa(f) | PKey::Dh(f) => f.priv_key.is_some(),
            PKey::Ec(e) => e.priv_key.is_some(),
            PKey::Ecx(x) => x.priv_key.is_some(),
        }
    }

    // ---------------------------------------------------------------------
    // Encoding
    // ---------------------------------------------------------------------

    fn ffc_params_der(f: &Ffc, dsa: bool) -> Vec<u8> {
        if dsa {
            let q = f.q.clone().unwrap_or_default();
            asn1::seq(&[asn1::uint(&f.p), asn1::uint(&q), asn1::uint(&f.g)])
        } else {
            asn1::seq(&[asn1::uint(&f.p), asn1::uint(&f.g)])
        }
    }

    fn ec_params_der(g: &Group) -> Vec<u8> {
        if !g.explicit {
            if let Some(oid) = g.oid {
                return asn1::oid(oid);
            }
        }
        // ECParameters (explicit)
        let fl = g.field_len();
        let field = asn1::seq(&[asn1::oid(OID_PRIME_FIELD), asn1::uint(&g.p)]);
        let mut curve = vec![
            asn1::octet(&pad_be(&g.a, fl)),
            asn1::octet(&pad_be(&g.b, fl)),
        ];
        if let Some(s) = &g.seed {
            curve.push(asn1::bitstr(s));
        }
        let base = g.encode(&g.generator(), false);
        asn1::seq(&[
            asn1::int_i64(1),
            field,
            asn1::seq(&curve),
            asn1::octet(&base),
            asn1::uint(&g.n),
            asn1::uint(&g.h),
        ])
    }

    fn algid(&self) -> Vec<u8> {
        match self {
            PKey::Rsa(_) => asn1::algid(OID_RSA, Some(asn1::null())),
            PKey::Dsa(f) => asn1::algid(OID_DSA, Some(Self::ffc_params_der(f, true))),
            PKey::Dh(f) => asn1::algid(OID_DH, Some(Self::ffc_params_der(f, false))),
            PKey::Ec(e) => asn1::algid(OID_EC, Some(Self::ec_params_der(&e.group))),
            PKey::Ecx(x) => asn1::algid(x.kind.oid(), None),
        }
    }

    pub fn rsa_public_der(r: &Rsa) -> Vec<u8> {
        asn1::seq(&[asn1::uint(&r.n), asn1::uint(&r.e)])
    }

    fn public_key_bits(&self) -> Vec<u8> {
        match self {
            PKey::Rsa(r) => Self::rsa_public_der(r),
            PKey::Dsa(f) | PKey::Dh(f) => asn1::uint(&f.pub_key.clone().unwrap_or_default()),
            PKey::Ec(e) => e
                .group
                .encode(e.pub_key.as_ref().unwrap_or(&Point::Inf), false),
            PKey::Ecx(x) => x.pub_key.clone(),
        }
    }

    /// SubjectPublicKeyInfo DER (i2d_PUBKEY).
    pub fn spki_der(&self) -> Vec<u8> {
        asn1::seq(&[self.algid(), asn1::bitstr(&self.public_key_bits())])
    }

    /// PEM_write_bio_PUBKEY()
    pub fn public_pem(&self) -> String {
        asn1::pem_encode("PUBLIC KEY", &self.spki_der())
    }

    pub fn rsa_private_der(r: &Rsa) -> Vec<u8> {
        let z = BigUint::zero();
        let g = |v: &Option<BigUint>| asn1::uint(v.as_ref().unwrap_or(&z));
        asn1::seq(&[
            asn1::int_i64(0),
            asn1::uint(&r.n),
            asn1::uint(&r.e),
            g(&r.d),
            g(&r.p),
            g(&r.q),
            g(&r.dmp1),
            g(&r.dmq1),
            g(&r.iqmp),
        ])
    }

    fn ec_private_der(e: &EcKey, with_params: bool) -> Vec<u8> {
        let ol = bytes_len(&e.group.n);
        let d = e.priv_key.clone().unwrap_or_default();
        let mut items = vec![asn1::int_i64(1), asn1::octet(&pad_be(&d, ol))];
        if with_params {
            items.push(asn1::explicit(0, &Self::ec_params_der(&e.group)));
        }
        if let Some(p) = &e.pub_key {
            items.push(asn1::explicit(1, &asn1::bitstr(&e.group.encode(p, false))));
        }
        asn1::seq(&items)
    }

    /// PrivateKeyInfo (PKCS#8) DER.
    pub fn pkcs8_der(&self) -> Vec<u8> {
        let inner = match self {
            PKey::Rsa(r) => Self::rsa_private_der(r),
            PKey::Dsa(f) | PKey::Dh(f) => asn1::uint(&f.priv_key.clone().unwrap_or_default()),
            PKey::Ec(e) => Self::ec_private_der(e, false),
            PKey::Ecx(x) => asn1::octet(x.priv_key.as_deref().unwrap_or(&[])),
        };
        asn1::seq(&[asn1::int_i64(0), self.algid(), asn1::octet(&inner)])
    }

    /// PEM_write_bio_PrivateKey(): PKCS#8, optionally encrypted with PBES2.
    pub fn private_pem(&self, cipher: Option<&CipherSpec>, pass: &[u8]) -> Option<String> {
        if !self.has_private() {
            return None;
        }
        let der = self.pkcs8_der();
        match cipher {
            None => Some(asn1::pem_encode("PRIVATE KEY", &der)),
            Some(c) => {
                let enc = pkcs8_encrypt(&der, c, pass)?;
                Some(asn1::pem_encode("ENCRYPTED PRIVATE KEY", &enc))
            }
        }
    }

    pub fn public_only(&self) -> PKey {
        let mut k = self.clone();
        match &mut k {
            PKey::Rsa(r) => {
                r.d = None;
                r.p = None;
                r.q = None;
                r.dmp1 = None;
                r.dmq1 = None;
                r.iqmp = None;
            }
            PKey::Dsa(f) | PKey::Dh(f) => f.priv_key = None,
            PKey::Ec(e) => e.priv_key = None,
            PKey::Ecx(x) => x.priv_key = None,
        }
        k
    }
}

// -------------------------------------------------------------------------
// Decoding
// -------------------------------------------------------------------------

fn parse_ffc_params(t: &Tlv) -> Option<(BigUint, Option<BigUint>, BigUint)> {
    let c = t.children()?;
    match c.len() {
        2 => Some((c[0].uint(), None, c[1].uint())),
        n if n >= 3 => {
            // DSA: p, q, g ; DH (PKCS#3): p, g, [privateValueLength]
            Some((c[0].uint(), Some(c[1].uint()), c[2].uint()))
        }
        _ => None,
    }
}

fn parse_ec_params(t: &Tlv) -> Option<Group> {
    if t.tag == asn1::OID {
        let oid = t.oid()?;
        return Group::by_oid(&oid);
    }
    if t.tag != asn1::SEQUENCE {
        return None;
    }
    let c = t.children()?;
    if c.len() < 5 {
        return None;
    }
    let field = c[1].children()?;
    if field.len() < 2 || field[0].oid()? != OID_PRIME_FIELD {
        return None;
    }
    let p = field[1].uint();
    let curve = c[2].children()?;
    if curve.len() < 2 {
        return None;
    }
    let a = bn(curve[0].content);
    let b = bn(curve[1].content);
    let seed = curve.get(2).and_then(|s| s.bits()).map(|s| s.to_vec());
    let n = c[4].uint();
    let h = c.get(5).map(|t| t.uint()).unwrap_or_default();
    let mut g = Group {
        p,
        a,
        b,
        gx: BigUint::zero(),
        gy: BigUint::zero(),
        n,
        h,
        seed,
        name: None,
        oid: None,
        explicit: true,
    };
    match g.decode(c[3].content)? {
        Point::Aff(x, y) => {
            g.gx = x;
            g.gy = y;
        }
        Point::Inf => return None,
    }
    if g.h.is_zero() {
        g.guess_cofactor();
    }
    g.identify();
    Some(g)
}

fn parse_rsa_public(der: &[u8]) -> Option<Rsa> {
    let t = asn1::parse_one(der)?;
    let c = t.children()?;
    if c.len() != 2 {
        return None;
    }
    Some(Rsa {
        n: c[0].uint(),
        e: c[1].uint(),
        d: None,
        p: None,
        q: None,
        dmp1: None,
        dmq1: None,
        iqmp: None,
    })
}

fn parse_rsa_private(der: &[u8]) -> Option<Rsa> {
    let t = asn1::parse_one(der)?;
    let c = t.children()?;
    if c.len() < 9 {
        return None;
    }
    Some(Rsa {
        n: c[1].uint(),
        e: c[2].uint(),
        d: Some(c[3].uint()),
        p: Some(c[4].uint()),
        q: Some(c[5].uint()),
        dmp1: Some(c[6].uint()),
        dmq1: Some(c[7].uint()),
        iqmp: Some(c[8].uint()),
    })
}

fn parse_ec_private(der: &[u8], params: Option<Group>) -> Option<EcKey> {
    let t = asn1::parse_one(der)?;
    let c = t.children()?;
    if c.len() < 2 || c[1].tag != asn1::OCTET_STRING {
        return None;
    }
    let d = bn(c[1].content);
    let mut group = params;
    let mut pubk = None;
    for x in &c[2..] {
        match x.context() {
            Some(0) => {
                let inner = asn1::parse_one(x.content)?;
                group = Some(parse_ec_params(&inner)?);
            }
            Some(1) => {
                let inner = asn1::parse_one(x.content)?;
                pubk = Some(inner.bits()?.to_vec());
            }
            _ => {}
        }
    }
    let group = group?;
    let pub_point = match pubk {
        Some(b) => group.decode(&b)?,
        None => group.pub_from_priv(&d),
    };
    Some(EcKey {
        group,
        pub_key: Some(pub_point),
        priv_key: Some(d),
        sm2: false,
    })
}

/// d2i_PUBKEY()
pub fn parse_spki(der: &[u8]) -> Option<PKey> {
    let t = asn1::parse_one(der)?;
    let c = t.children()?;
    if c.len() != 2 {
        return None;
    }
    let alg = c[0].children()?;
    let oid = alg.first()?.oid()?;
    let bits = c[1].bits()?;
    match oid.as_str() {
        OID_RSA | OID_RSA_PSS => Some(PKey::Rsa(parse_rsa_public(bits)?)),
        OID_DSA => {
            let (p, q, g) = parse_ffc_params(alg.get(1)?)?;
            let y = asn1::parse_one(bits)?.uint();
            Some(PKey::Dsa(Ffc {
                p,
                q,
                g,
                pub_key: Some(y),
                priv_key: None,
            }))
        }
        OID_DH | OID_DHX => {
            let pc = alg.get(1)?.children()?;
            let (p, q, g) = if oid == OID_DH {
                (pc.first()?.uint(), None, pc.get(1)?.uint())
            } else {
                (
                    pc.first()?.uint(),
                    pc.get(2).map(|t| t.uint()),
                    pc.get(1)?.uint(),
                )
            };
            let y = asn1::parse_one(bits)?.uint();
            Some(PKey::Dh(Ffc {
                p,
                q,
                g,
                pub_key: Some(y),
                priv_key: None,
            }))
        }
        OID_EC => {
            let group = parse_ec_params(alg.get(1)?)?;
            let sm2 = group.oid == Some(OID_SM2);
            let pt = group.decode(bits)?;
            Some(PKey::Ec(EcKey {
                group,
                pub_key: Some(pt),
                priv_key: None,
                sm2,
            }))
        }
        o => {
            let kind = EcxKind::from_oid(o)?;
            if bits.len() != kind.key_len() {
                return None;
            }
            Some(PKey::Ecx(Ecx {
                kind,
                pub_key: bits.to_vec(),
                priv_key: None,
            }))
        }
    }
}

/// d2i_PrivateKey on a PrivateKeyInfo.
pub fn parse_pkcs8(der: &[u8]) -> Option<PKey> {
    let t = asn1::parse_one(der)?;
    let c = t.children()?;
    if c.len() < 3 {
        return None;
    }
    let alg = c[1].children()?;
    let oid = alg.first()?.oid()?;
    let inner = c[2].content;
    match oid.as_str() {
        OID_RSA | OID_RSA_PSS => Some(PKey::Rsa(parse_rsa_private(inner)?)),
        OID_DSA => {
            let (p, q, g) = parse_ffc_params(alg.get(1)?)?;
            let x = asn1::parse_one(inner)?.uint();
            let y = crate::bn::modpow(&g, &x, &p);
            Some(PKey::Dsa(Ffc {
                p,
                q,
                g,
                pub_key: Some(y),
                priv_key: Some(x),
            }))
        }
        OID_DH | OID_DHX => {
            let pc = alg.get(1)?.children()?;
            let (p, q, g) = if oid == OID_DH {
                (pc.first()?.uint(), None, pc.get(1)?.uint())
            } else {
                (
                    pc.first()?.uint(),
                    pc.get(2).map(|t| t.uint()),
                    pc.get(1)?.uint(),
                )
            };
            let x = asn1::parse_one(inner)?.uint();
            let y = crate::bn::modpow(&g, &x, &p);
            Some(PKey::Dh(Ffc {
                p,
                q,
                g,
                pub_key: Some(y),
                priv_key: Some(x),
            }))
        }
        OID_EC => {
            let group = parse_ec_params(alg.get(1)?)?;
            let sm2 = group.oid == Some(OID_SM2);
            let mut k = parse_ec_private(inner, Some(group))?;
            k.sm2 = sm2;
            Some(PKey::Ec(k))
        }
        o => {
            let kind = EcxKind::from_oid(o)?;
            let priv_key = asn1::parse_one(inner)?.content.to_vec();
            let pub_key = kind.pub_from_priv(&priv_key)?;
            Some(PKey::Ecx(Ecx {
                kind,
                pub_key,
                priv_key: Some(priv_key),
            }))
        }
    }
}

// -------------------------------------------------------------------------
// PEM key reading with passphrases
// -------------------------------------------------------------------------

/// EVP_BytesToKey(cipher, md5, salt, pass, 1)
pub fn bytes_to_key(
    md: Md,
    salt: &[u8],
    pass: &[u8],
    key_len: usize,
    iv_len: usize,
) -> (Vec<u8>, Vec<u8>) {
    let mut out = Vec::new();
    let mut prev: Vec<u8> = Vec::new();
    while out.len() < key_len + iv_len {
        let mut h = digest::Hasher::new(md);
        h.update(&prev);
        h.update(pass);
        h.update(salt);
        prev = h.finish();
        out.extend_from_slice(&prev);
    }
    (
        out[..key_len].to_vec(),
        out[key_len..key_len + iv_len].to_vec(),
    )
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

fn cipher_by_oid(oid: &str) -> Option<&'static CipherSpec> {
    let name = match oid {
        "2.16.840.1.101.3.4.1.2" => "aes-128-cbc",
        "2.16.840.1.101.3.4.1.22" => "aes-192-cbc",
        "2.16.840.1.101.3.4.1.42" => "aes-256-cbc",
        "1.2.840.113549.3.7" => "des-ede3-cbc",
        "1.3.14.3.2.7" => "des-cbc",
        "1.2.840.113549.3.2" => "rc2-cbc",
        "2.16.840.1.101.3.4.1.1" => "aes-128-ecb",
        _ => return None,
    };
    cipher::by_name(name)
}

fn prf_md(oid: &str) -> Option<Md> {
    Some(match oid {
        "1.2.840.113549.2.7" => Md::Sha1,
        "1.2.840.113549.2.8" => Md::Sha224,
        "1.2.840.113549.2.9" => Md::Sha256,
        "1.2.840.113549.2.10" => Md::Sha384,
        "1.2.840.113549.2.11" => Md::Sha512,
        "1.2.840.113549.2.12" => Md::Sha512_224,
        "1.2.840.113549.2.13" => Md::Sha512_256,
        _ => return None,
    })
}

/// Outcome of a failed private key read, to emit the right error codes.
#[derive(Debug, PartialEq, Eq)]
pub enum ReadErr {
    /// Not a (supported) key at all.
    Unsupported,
    /// Decryption failed (wrong passphrase).
    BadDecrypt,
}

/// Decrypt an EncryptedPrivateKeyInfo.
pub fn pkcs8_decrypt(der: &[u8], pass: &[u8]) -> Result<Vec<u8>, ReadErr> {
    let t = asn1::parse_one(der).ok_or(ReadErr::Unsupported)?;
    let c = t.children().ok_or(ReadErr::Unsupported)?;
    if c.len() != 2 {
        return Err(ReadErr::Unsupported);
    }
    let alg = c[0].children().ok_or(ReadErr::Unsupported)?;
    let oid = alg
        .first()
        .and_then(|o| o.oid())
        .ok_or(ReadErr::Unsupported)?;
    let data = c[1].content;
    let plain = match oid.as_str() {
        "1.2.840.113549.1.5.13" => {
            // PBES2
            let params = alg
                .get(1)
                .and_then(|p| p.children())
                .ok_or(ReadErr::Unsupported)?;
            let kdf = params
                .first()
                .and_then(|p| p.children())
                .ok_or(ReadErr::Unsupported)?;
            let enc = params
                .get(1)
                .and_then(|p| p.children())
                .ok_or(ReadErr::Unsupported)?;
            if kdf.first().and_then(|o| o.oid()).as_deref() != Some("1.2.840.113549.1.5.12") {
                return Err(ReadErr::Unsupported);
            }
            let kp = kdf
                .get(1)
                .and_then(|p| p.children())
                .ok_or(ReadErr::Unsupported)?;
            let salt = kp.first().ok_or(ReadErr::Unsupported)?.content;
            let iter = kp
                .get(1)
                .and_then(|i| i.as_i64())
                .ok_or(ReadErr::Unsupported)? as u32;
            let mut md = Md::Sha1;
            let mut key_len_param = None;
            for x in &kp[2..] {
                if x.tag == asn1::INTEGER {
                    key_len_param = x.as_i64();
                } else if x.tag == asn1::SEQUENCE {
                    let pc = x.children().ok_or(ReadErr::Unsupported)?;
                    md = pc
                        .first()
                        .and_then(|o| o.oid())
                        .and_then(|o| prf_md(&o))
                        .ok_or(ReadErr::Unsupported)?;
                }
            }
            let coid = enc
                .first()
                .and_then(|o| o.oid())
                .ok_or(ReadErr::Unsupported)?;
            let spec = cipher_by_oid(&coid).ok_or(ReadErr::Unsupported)?;
            let iv_t = enc.get(1).ok_or(ReadErr::Unsupported)?;
            let iv = if iv_t.tag == asn1::OCTET_STRING {
                iv_t.content.to_vec()
            } else {
                // RC2 params: SEQUENCE { version, iv }
                let ic = iv_t.children().ok_or(ReadErr::Unsupported)?;
                ic.last().ok_or(ReadErr::Unsupported)?.content.to_vec()
            };
            let klen = key_len_param.map(|k| k as usize).unwrap_or(spec.key_len);
            let key = digest::pbkdf2(md, pass, salt, iter, klen);
            decrypt_quiet(&spec.name, data, &key, &iv)
        }
        "1.2.840.113549.1.5.3"
        | "1.2.840.113549.1.5.10"
        | "1.2.840.113549.1.5.6"
        | "1.2.840.113549.1.5.11" => {
            // PBES1
            let p = alg
                .get(1)
                .and_then(|p| p.children())
                .ok_or(ReadErr::Unsupported)?;
            let salt = p.first().ok_or(ReadErr::Unsupported)?.content;
            let iter = p
                .get(1)
                .and_then(|i| i.as_i64())
                .ok_or(ReadErr::Unsupported)? as u32;
            let (md, cname) = match oid.as_str() {
                "1.2.840.113549.1.5.3" => (Md::Md5, "des-cbc"),
                "1.2.840.113549.1.5.10" => (Md::Sha1, "des-cbc"),
                "1.2.840.113549.1.5.6" => (Md::Md5, "rc2-64-cbc"),
                _ => (Md::Sha1, "rc2-64-cbc"),
            };
            let mut dk = [pass, salt].concat();
            for _ in 0..iter {
                dk = md.digest(&dk);
            }
            decrypt_quiet(cname, data, &dk[..8], &dk[8..16])
        }
        "1.2.840.113549.1.12.1.3"
        | "1.2.840.113549.1.12.1.6"
        | "1.2.840.113549.1.12.1.5"
        | "1.2.840.113549.1.12.1.1" => {
            let p = alg
                .get(1)
                .and_then(|p| p.children())
                .ok_or(ReadErr::Unsupported)?;
            let salt = p.first().ok_or(ReadErr::Unsupported)?.content;
            let iter = p
                .get(1)
                .and_then(|i| i.as_i64())
                .ok_or(ReadErr::Unsupported)? as u32;
            let pw = String::from_utf8_lossy(pass).to_string();
            let (cname, klen, ivlen) = match oid.as_str() {
                "1.2.840.113549.1.12.1.3" => ("des-ede3-cbc", 24, 8),
                "1.2.840.113549.1.12.1.6" => ("rc2-40-cbc", 5, 8),
                "1.2.840.113549.1.12.1.5" => ("rc2-cbc", 16, 8),
                _ => ("rc4", 16, 0),
            };
            let key = crate::pkcs12::kdf_str(&pw, salt, iter, klen, 1);
            let iv = if ivlen > 0 {
                crate::pkcs12::kdf_str(&pw, salt, iter, ivlen, 2)
            } else {
                Vec::new()
            };
            decrypt_quiet(cname, data, &key, &iv)
        }
        _ => return Err(ReadErr::Unsupported),
    };
    plain.ok_or(ReadErr::BadDecrypt)
}

/// PKCS#8 encryption with PBES2/PBKDF2(hmacWithSHA256, 2048 iterations).
pub fn pkcs8_encrypt(der: &[u8], spec: &CipherSpec, pass: &[u8]) -> Option<Vec<u8>> {
    use purecrypto::rng::RngCore;
    let mut rng = purecrypto::rng::OsRng;
    let mut salt = [0u8; 16];
    rng.fill_bytes(&mut salt);
    let mut iv = vec![0u8; spec.iv_len];
    rng.fill_bytes(&mut iv);
    let iter = 2048u32;
    let key = digest::pbkdf2(Md::Sha256, pass, &salt, iter, spec.key_len);
    let enc = cipher::simple(&spec.name, der, &key, &iv, true)?;
    let coid = spec.oid()?;
    let enc_params = if matches!(spec.algo, cipher::Algo::Rc2(_)) {
        let ver = match spec.algo {
            cipher::Algo::Rc2(40) => 160,
            cipher::Algo::Rc2(64) => 120,
            _ => 58,
        };
        asn1::seq(&[asn1::int_i64(ver), asn1::octet(&iv)])
    } else {
        asn1::octet(&iv)
    };
    let kdf = asn1::seq(&[
        asn1::oid("1.2.840.113549.1.5.12"),
        asn1::seq(&[
            asn1::octet(&salt),
            asn1::int_i64(iter as i64),
            asn1::algid("1.2.840.113549.2.9", Some(asn1::null())),
        ]),
    ]);
    let alg = asn1::algid(
        "1.2.840.113549.1.5.13",
        Some(asn1::seq(&[kdf, asn1::seq(&[asn1::oid(coid), enc_params])])),
    );
    Some(asn1::seq(&[alg, asn1::octet(&enc)]))
}

/// Decrypt a traditional PEM body with Proc-Type/DEK-Info headers.
/// Decrypt without leaving the cipher's own errors on the queue (the
/// caller reports the PEM/provider "bad decrypt" pair).
fn decrypt_quiet(name: &str, data: &[u8], key: &[u8], iv: &[u8]) -> Option<Vec<u8>> {
    let mark = err::mark();
    let r = cipher::simple(name, data, key, iv, false);
    if r.is_none() {
        err::restore(mark);
    }
    r
}

fn legacy_decrypt(block: &asn1::PemBlock, pass: &[u8]) -> Result<Vec<u8>, ReadErr> {
    let dek = block
        .headers
        .iter()
        .find(|(k, _)| k == "DEK-Info")
        .map(|(_, v)| v.clone());
    let Some(dek) = dek else {
        return Ok(block.der.clone());
    };
    let mut it = dek.splitn(2, ',');
    let cname = it.next().unwrap_or("").trim();
    let iv = hex_decode(it.next().unwrap_or("")).ok_or(ReadErr::Unsupported)?;
    let spec = cipher::by_name(cname).ok_or(ReadErr::Unsupported)?;
    if iv.len() < 8 {
        return Err(ReadErr::Unsupported);
    }
    let (key, _) = bytes_to_key(Md::Md5, &iv[..8], pass, spec.key_len, 0);
    decrypt_quiet(&spec.name, &block.der, &key, &iv).ok_or(ReadErr::BadDecrypt)
}

fn is_encrypted(block: &asn1::PemBlock) -> bool {
    block
        .headers
        .iter()
        .any(|(k, v)| k == "Proc-Type" && v.contains("ENCRYPTED"))
}

/// PEM_read_bio_PrivateKey(): the first private key PEM block in `data`.
pub fn read_private_pem(data: &[u8], pass: &[u8]) -> Result<PKey, ReadErr> {
    let labels = [
        "PRIVATE KEY",
        "ENCRYPTED PRIVATE KEY",
        "RSA PRIVATE KEY",
        "EC PRIVATE KEY",
        "DSA PRIVATE KEY",
    ];
    let blocks = asn1::pem_blocks(data);
    let Some(idx) = blocks
        .iter()
        .position(|b| labels.contains(&b.label.as_str()))
    else {
        return Err(ReadErr::Unsupported);
    };
    let block = &blocks[idx];
    let key = match block.label.as_str() {
        "PRIVATE KEY" => parse_pkcs8(&block.der),
        "ENCRYPTED PRIVATE KEY" => {
            let plain = pkcs8_decrypt(&block.der, pass)?;
            match parse_pkcs8(&plain) {
                Some(k) => Some(k),
                None => return Err(ReadErr::BadDecrypt),
            }
        }
        "RSA PRIVATE KEY" => {
            let enc = is_encrypted(block);
            let der = legacy_decrypt(block, pass)?;
            match parse_rsa_private(&der) {
                Some(r) => Some(PKey::Rsa(r)),
                None if enc => return Err(ReadErr::BadDecrypt),
                None => None,
            }
        }
        "EC PRIVATE KEY" => {
            let enc = is_encrypted(block);
            let der = legacy_decrypt(block, pass)?;
            // optional preceding "EC PARAMETERS" block
            let params = blocks[..idx]
                .iter()
                .rev()
                .find(|b| b.label == "EC PARAMETERS")
                .and_then(|b| asn1::parse_one(&b.der).and_then(|t| parse_ec_params(&t)));
            match parse_ec_private(&der, params) {
                Some(k) => {
                    let mut k = k;
                    k.sm2 = k.group.oid == Some(OID_SM2);
                    Some(PKey::Ec(k))
                }
                None if enc => return Err(ReadErr::BadDecrypt),
                None => None,
            }
        }
        "DSA PRIVATE KEY" => {
            let enc = is_encrypted(block);
            let der = legacy_decrypt(block, pass)?;
            let parsed = asn1::parse_one(&der)
                .and_then(|t| t.children())
                .and_then(|c| {
                    if c.len() < 6 {
                        return None;
                    }
                    Some(PKey::Dsa(Ffc {
                        p: c[1].uint(),
                        q: Some(c[2].uint()),
                        g: c[3].uint(),
                        pub_key: Some(c[4].uint()),
                        priv_key: Some(c[5].uint()),
                    }))
                });
            match parsed {
                Some(k) => Some(k),
                None if enc => return Err(ReadErr::BadDecrypt),
                None => None,
            }
        }
        _ => None,
    };
    key.ok_or(ReadErr::Unsupported)
}

/// PEM_read_bio_PUBKEY(): "PUBLIC KEY" or "RSA PUBLIC KEY".
pub fn read_public_pem(data: &[u8]) -> Option<PKey> {
    let block = asn1::pem_find(data, &["PUBLIC KEY", "RSA PUBLIC KEY"])?;
    if block.label == "RSA PUBLIC KEY" {
        return Some(PKey::Rsa(parse_rsa_public(&block.der)?));
    }
    parse_spki(&block.der)
}

pub fn push_read_error(e: &ReadErr) {
    match e {
        ReadErr::Unsupported => err::push(err::LIB_DECODER, err::R_UNSUPPORTED, "unsupported"),
        ReadErr::BadDecrypt => {
            err::prov_bad_decrypt();
            err::push(err::LIB_PEM, 101, "bad decrypt");
        }
    }
}

// -------------------------------------------------------------------------
// Key generation
// -------------------------------------------------------------------------

fn rand_below(n: &BigUint) -> BigUint {
    crate::ec::random_below(n)
}

pub fn mod_inv(a: &BigUint, m: &BigUint) -> Option<BigUint> {
    use num_bigint_dig::ModInverse;
    let r = a.clone().mod_inverse(m)?;
    r.to_biguint()
}

/// Complete an RSA key from n, e, d, p, q (CRT values).
fn rsa_with_crt(n: BigUint, e: BigUint, d: BigUint, p: BigUint, q: BigUint) -> Rsa {
    let one = BigUint::one();
    let dmp1 = &d % (&p - &one);
    let dmq1 = &d % (&q - &one);
    let iqmp = mod_inv(&q, &p).unwrap_or_default();
    Rsa {
        n,
        e,
        d: Some(d),
        p: Some(p),
        q: Some(q),
        dmp1: Some(dmp1),
        dmq1: Some(dmq1),
        iqmp: Some(iqmp),
    }
}

pub fn generate_rsa(bits: usize) -> Option<Rsa> {
    let e = BigUint::from(65537u32);
    if bits.is_multiple_of(2) {
        use purecrypto::bignum::BoxedUint;
        use purecrypto::rsa::BoxedRsaPrivateKey;
        let k = BoxedRsaPrivateKey::generate(
            bits,
            BoxedUint::from_u64(65537),
            &mut purecrypto::rng::OsRng,
            0,
        );
        let len = bits.div_ceil(8);
        let conv = |b: &BoxedUint| BigUint::from_bytes_be(&b.to_be_bytes(len));
        let n = conv(k.modulus());
        let d = conv(k.private_exponent());
        let (p, q) = k.primes()?;
        let (p, q) = (conv(p), conv(q));
        return Some(rsa_with_crt(n, e, d, p, q));
    }
    // purecrypto only generates even-sized moduli: odd sizes use unbalanced
    // primes from num-bigint-dig (as OpenSSL does).
    use num_bigint_dig::RandPrime;
    let mut rng = rand::thread_rng();
    let one = BigUint::one();
    loop {
        let p = rng.gen_prime(bits.div_ceil(2));
        let q = rng.gen_prime(bits / 2);
        if p == q {
            continue;
        }
        let n = &p * &q;
        if n.bits() != bits {
            continue;
        }
        let phi = (&p - &one) * (&q - &one);
        let Some(d) = mod_inv(&e, &phi) else { continue };
        return Some(rsa_with_crt(n, e, d, p, q));
    }
}

pub fn generate_ec(group: Group) -> EcKey {
    let d = group.random_scalar();
    let q = group.pub_from_priv(&d);
    let sm2 = group.oid == Some(OID_SM2);
    EcKey {
        group,
        pub_key: Some(q),
        priv_key: Some(d),
        sm2,
    }
}

/// FFC key generation from (p, q, g): DSA/DH.
pub fn generate_ffc(p: &BigUint, q: Option<&BigUint>, g: &BigUint) -> (BigUint, BigUint) {
    let x = match q {
        Some(q) => rand_below(q),
        None => {
            // private value length = bits(p) - 1
            let mut rng = rand::thread_rng();
            let bits = p.bits().saturating_sub(1).max(1);
            loop {
                let x = rng.gen_biguint(bits);
                if !x.is_zero() && &x < p {
                    break x;
                }
            }
        }
    };
    let y = crate::bn::modpow(g, &x, p);
    (x, y)
}

/// ossl_ffc_params_simple_validate(): basic p/q/g sanity checks.
pub fn ffc_simple_validate(p: &BigUint, q: Option<&BigUint>, g: &BigUint) -> bool {
    let one = BigUint::one();
    if p.is_zero() || p.is_even() {
        return false;
    }
    if g <= &one || g >= &(p - &one) {
        return false;
    }
    if let Some(q) = q {
        if q.is_zero() || q.is_even() || q >= p {
            return false;
        }
        if crate::bn::modpow_public(g, q, p) != one {
            return false;
        }
    }
    true
}

fn is_probable_prime(n: &BigUint) -> bool {
    num_bigint_dig::prime::probably_prime(n, 64)
}

/// DSA parameter generation (FIPS 186-4 style probable primes).
pub fn generate_dsa_params(pbits: usize) -> Option<(BigUint, BigUint, BigUint)> {
    let qbits = if pbits >= 2048 {
        224
    } else {
        224.min(pbits / 2).max(160)
    };
    let mut rng = rand::thread_rng();
    let one = BigUint::one();
    for _ in 0..10000 {
        let mut q = rng.gen_biguint(qbits);
        q |= (BigUint::one() << (qbits - 1)) | BigUint::one();
        if !is_probable_prime(&q) {
            continue;
        }
        for _ in 0..4096 {
            let mut x = rng.gen_biguint(pbits);
            x |= BigUint::one() << (pbits - 1);
            let c = &x % (&q << 1usize);
            let p = &x - &c + &one;
            if p.bits() < pbits {
                continue;
            }
            if is_probable_prime(&p) {
                let e = (&p - &one) / &q;
                let mut h = BigUint::from(2u32);
                loop {
                    let g = crate::bn::modpow_public(&h, &e, &p);
                    if g > one {
                        return Some((p, q, g));
                    }
                    h += &one;
                }
            }
        }
    }
    None
}

/// DH safe-prime parameter generation with generator 2.
pub fn generate_dh_params(pbits: usize) -> Option<(BigUint, BigUint)> {
    if pbits < 512 {
        return None;
    }
    let mut rng = rand::thread_rng();
    let one = BigUint::one();
    let g = BigUint::from(2u32);
    loop {
        // p = 2q + 1 with p = 23 mod 24 (so that 2 is a quadratic residue
        // generator of the q-order subgroup, as OpenSSL does).
        let mut q = rng.gen_biguint(pbits - 1);
        q |= (BigUint::one() << (pbits - 2)) | BigUint::one();
        let p = (&q << 1usize) + &one;
        if (&p % BigUint::from(24u32)) != BigUint::from(23u32) {
            continue;
        }
        if !num_bigint_dig::prime::probably_prime(&q, 2)
            || !num_bigint_dig::prime::probably_prime(&p, 2)
        {
            continue;
        }
        if is_probable_prime(&q) && is_probable_prime(&p) {
            return Some((p, g));
        }
    }
}

pub fn generate_ecx(kind: EcxKind) -> Ecx {
    use purecrypto::rng::RngCore;
    let mut seed = vec![0u8; kind.key_len()];
    purecrypto::rng::OsRng.fill_bytes(&mut seed);
    let pub_key = kind.pub_from_priv(&seed).unwrap_or_default();
    Ecx {
        kind,
        pub_key,
        priv_key: Some(seed),
    }
}

// -------------------------------------------------------------------------
// RSA operations
// -------------------------------------------------------------------------

pub const RSA_PKCS1_PADDING: i64 = 1;
pub const RSA_NO_PADDING: i64 = 3;
pub const RSA_PKCS1_OAEP_PADDING: i64 = 4;
pub const RSA_PKCS1_PSS_PADDING: i64 = 6;

fn rsa_err(reason: u32, s: &str) {
    err::push(err::LIB_RSA, reason, s);
}

impl Rsa {
    pub fn k(&self) -> usize {
        bytes_len(&self.n)
    }

    fn public_op(&self, m: &BigUint) -> Option<Vec<u8>> {
        if self.n.is_even() {
            err::push(err::LIB_BN, 108, "no inverse");
            return None;
        }
        if m >= &self.n {
            rsa_err(132, "data too large for modulus");
            return None;
        }
        Some(pad_be(
            &crate::bn::modpow_public(m, &self.e, &self.n),
            self.k(),
        ))
    }

    fn private_op(&self, c: &BigUint) -> Option<Vec<u8>> {
        if self.n.is_even() {
            err::push(err::LIB_BN, 108, "no inverse");
            return None;
        }
        if c >= &self.n {
            rsa_err(132, "data too large for modulus");
            return None;
        }
        let d = self.d.as_ref()?;
        Some(pad_be(&crate::bn::modpow(c, d, &self.n), self.k()))
    }

    fn rand_nonzero(len: usize) -> Vec<u8> {
        use purecrypto::rng::RngCore;
        let mut rng = purecrypto::rng::OsRng;
        let mut v = vec![0u8; len];
        rng.fill_bytes(&mut v);
        for b in v.iter_mut() {
            while *b == 0 {
                let mut one = [0u8; 1];
                rng.fill_bytes(&mut one);
                *b = one[0];
            }
        }
        v
    }

    fn mgf1(md: Md, seed: &[u8], len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len);
        let mut c: u32 = 0;
        while out.len() < len {
            let mut h = digest::Hasher::new(md);
            h.update(seed);
            h.update(&c.to_be_bytes());
            out.extend(h.finish());
            c += 1;
        }
        out.truncate(len);
        out
    }

    /// RSA public encrypt (EVP_PKEY_encrypt).
    pub fn encrypt(&self, data: &[u8], padding: i64, oaep_md: Md) -> Option<Vec<u8>> {
        let k = self.k();
        let em = match padding {
            RSA_PKCS1_PADDING => {
                if data.len() > k.saturating_sub(11) {
                    rsa_err(110, "data too large for key size");
                    return None;
                }
                let mut em = vec![0u8, 2];
                em.extend(Self::rand_nonzero(k - 3 - data.len()));
                em.push(0);
                em.extend_from_slice(data);
                em
            }
            RSA_NO_PADDING => {
                if data.len() > k {
                    rsa_err(110, "data too large for key size");
                    return None;
                }
                if data.len() < k {
                    rsa_err(122, "data too small for key size");
                    return None;
                }
                data.to_vec()
            }
            RSA_PKCS1_OAEP_PADDING => {
                use purecrypto::rng::RngCore;
                let hlen = oaep_md.size();
                if k < 2 * hlen + 2 || data.len() > k - 2 * hlen - 2 {
                    rsa_err(110, "data too large for key size");
                    return None;
                }
                let lhash = oaep_md.digest(&[]);
                let mut db = lhash;
                db.extend(std::iter::repeat_n(0u8, k - data.len() - 2 * hlen - 2));
                db.push(1);
                db.extend_from_slice(data);
                let mut seed = vec![0u8; hlen];
                purecrypto::rng::OsRng.fill_bytes(&mut seed);
                let dbmask = Self::mgf1(oaep_md, &seed, k - hlen - 1);
                for (a, b) in db.iter_mut().zip(dbmask) {
                    *a ^= b;
                }
                let smask = Self::mgf1(oaep_md, &db, hlen);
                for (a, b) in seed.iter_mut().zip(smask) {
                    *a ^= b;
                }
                let mut em = vec![0u8];
                em.extend(seed);
                em.extend(db);
                em
            }
            _ => {
                err::push(err::LIB_PROV, 165, "illegal or unsupported padding mode");
                return None;
            }
        };
        self.public_op(&bn(&em))
    }

    /// RSA private decrypt (EVP_PKEY_decrypt).
    pub fn decrypt(&self, data: &[u8], padding: i64, oaep_md: Md) -> Option<Vec<u8>> {
        let k = self.k();
        if !matches!(
            padding,
            RSA_PKCS1_PADDING | RSA_NO_PADDING | RSA_PKCS1_OAEP_PADDING
        ) {
            err::push(err::LIB_PROV, 165, "illegal or unsupported padding mode");
            return None;
        }
        if data.len() > k {
            rsa_err(108, "data greater than mod len");
            return None;
        }
        let em = self.private_op(&bn(data))?;
        match padding {
            RSA_NO_PADDING => Some(em),
            RSA_PKCS1_PADDING => {
                if em[0] != 0 || em[1] != 2 {
                    rsa_err(159, "pkcs decoding error");
                    return None;
                }
                match em[2..].iter().position(|&b| b == 0) {
                    Some(z) if z >= 8 => Some(em[2 + z + 1..].to_vec()),
                    _ => {
                        rsa_err(159, "pkcs decoding error");
                        None
                    }
                }
            }
            _ => {
                let hlen = oaep_md.size();
                if k < 2 * hlen + 2 {
                    rsa_err(121, "oaep decoding error");
                    return None;
                }
                let mut seed = em[1..1 + hlen].to_vec();
                let mut db = em[1 + hlen..].to_vec();
                let smask = Self::mgf1(oaep_md, &db, hlen);
                for (a, b) in seed.iter_mut().zip(smask) {
                    *a ^= b;
                }
                let dbmask = Self::mgf1(oaep_md, &seed, k - hlen - 1);
                for (a, b) in db.iter_mut().zip(dbmask) {
                    *a ^= b;
                }
                let lhash = oaep_md.digest(&[]);
                if em[0] != 0 || db[..hlen] != lhash[..] {
                    rsa_err(121, "oaep decoding error");
                    return None;
                }
                match db[hlen..].iter().position(|&b| b != 0) {
                    Some(i) if db[hlen + i] == 1 => Some(db[hlen + i + 1..].to_vec()),
                    _ => {
                        rsa_err(121, "oaep decoding error");
                        None
                    }
                }
            }
        }
    }

    /// RSA private encrypt (raw EVP_PKEY_sign without a digest).
    pub fn sign_raw(&self, data: &[u8], padding: i64) -> Option<Vec<u8>> {
        let k = self.k();
        let em = match padding {
            RSA_PKCS1_PADDING => {
                if data.len() > k.saturating_sub(11) {
                    rsa_err(110, "data too large for key size");
                    return None;
                }
                let mut em = vec![0u8, 1];
                em.extend(std::iter::repeat_n(0xffu8, k - 3 - data.len()));
                em.push(0);
                em.extend_from_slice(data);
                em
            }
            RSA_NO_PADDING => {
                if data.len() > k {
                    rsa_err(110, "data too large for key size");
                    return None;
                }
                if data.len() < k {
                    rsa_err(122, "data too small for key size");
                    return None;
                }
                data.to_vec()
            }
            _ => {
                err::push(err::LIB_PROV, 165, "illegal or unsupported padding mode");
                return None;
            }
        };
        self.private_op(&bn(&em))
    }

    /// RSA public decrypt (EVP_PKEY_verify_recover).
    pub fn verify_recover(&self, data: &[u8], padding: i64) -> Option<Vec<u8>> {
        let k = self.k();
        if !matches!(padding, RSA_PKCS1_PADDING | RSA_NO_PADDING) {
            err::push(err::LIB_PROV, 165, "illegal or unsupported padding mode");
            return None;
        }
        if data.len() > k {
            rsa_err(108, "data greater than mod len");
            return None;
        }
        let em = self.public_op(&bn(data))?;
        if padding == RSA_NO_PADDING {
            return Some(em);
        }
        if em[0] != 0 || em[1] != 1 {
            rsa_err(106, "block type is not 01");
            rsa_err(114, "padding check failed");
            return None;
        }
        let mut i = 2;
        while i < em.len() && em[i] == 0xff {
            i += 1;
        }
        if i >= em.len() || em[i] != 0 || i - 2 < 8 {
            rsa_err(113, "bad fixed header decrypt");
            rsa_err(114, "padding check failed");
            return None;
        }
        Some(em[i + 1..].to_vec())
    }

    fn digest_info(md: Md, hash: &[u8]) -> Vec<u8> {
        match md.oid() {
            Some(oid) if md != Md::Md5Sha1 => {
                asn1::seq(&[asn1::algid(oid, Some(asn1::null())), asn1::octet(hash)])
            }
            _ => hash.to_vec(),
        }
    }

    /// RSASSA-PKCS1-v1_5 / PSS signature over a message digest.
    pub fn sign_digest(&self, md: Md, hash: &[u8], pss: bool) -> Option<Vec<u8>> {
        let k = self.k();
        if pss {
            use purecrypto::rng::RngCore;
            let hlen = md.size();
            let embits = self.n.bits() - 1;
            let emlen = embits.div_ceil(8);
            if emlen < hlen + 2 {
                rsa_err(110, "data too large for key size");
                return None;
            }
            let slen = emlen - hlen - 2;
            let mut salt = vec![0u8; slen];
            purecrypto::rng::OsRng.fill_bytes(&mut salt);
            let mut mp = vec![0u8; 8];
            mp.extend_from_slice(hash);
            mp.extend_from_slice(&salt);
            let h = md.digest(&mp);
            let mut db = vec![0u8; emlen - slen - hlen - 2];
            db.push(1);
            db.extend(salt);
            let mask = Self::mgf1(md, &h, emlen - hlen - 1);
            for (a, b) in db.iter_mut().zip(mask) {
                *a ^= b;
            }
            let zbits = 8 * emlen - embits;
            db[0] &= 0xff >> zbits;
            let mut em = db;
            em.extend(h);
            em.push(0xbc);
            return self.private_op(&bn(&em));
        }
        let t = Self::digest_info(md, hash);
        if t.len() + 11 > k {
            rsa_err(112, "digest too big for rsa key");
            return None;
        }
        let mut em = vec![0u8, 1];
        em.extend(std::iter::repeat_n(0xffu8, k - 3 - t.len()));
        em.push(0);
        em.extend(t);
        self.private_op(&bn(&em))
    }

    pub fn verify_digest(&self, md: Md, hash: &[u8], sig: &[u8], pss: bool) -> bool {
        let k = self.k();
        if sig.len() != k {
            return false;
        }
        let s = bn(sig);
        if s >= self.n {
            return false;
        }
        let em_full = pad_be(&crate::bn::modpow_public(&s, &self.e, &self.n), k);
        if pss {
            let hlen = md.size();
            let embits = self.n.bits() - 1;
            let emlen = embits.div_ceil(8);
            let em = &em_full[k - emlen..];
            if k > emlen && em_full[0] != 0 {
                return false;
            }
            if emlen < hlen + 2 || *em.last().unwrap() != 0xbc {
                return false;
            }
            let mut db = em[..emlen - hlen - 1].to_vec();
            let h = &em[emlen - hlen - 1..emlen - 1];
            let zbits = 8 * emlen - embits;
            if db[0] & !(0xffu8 >> zbits) != 0 {
                return false;
            }
            let mask = Self::mgf1(md, h, emlen - hlen - 1);
            for (a, b) in db.iter_mut().zip(mask) {
                *a ^= b;
            }
            db[0] &= 0xff >> zbits;
            let Some(i) = db.iter().position(|&b| b != 0) else {
                return false;
            };
            if db[i] != 1 {
                return false;
            }
            let salt = &db[i + 1..];
            let mut mp = vec![0u8; 8];
            mp.extend_from_slice(hash);
            mp.extend_from_slice(salt);
            return md.digest(&mp) == h;
        }
        let t = Self::digest_info(md, hash);
        if t.len() + 11 > k {
            return false;
        }
        let mut em = vec![0u8, 1];
        em.extend(std::iter::repeat_n(0xffu8, k - 3 - t.len()));
        em.push(0);
        em.extend(t);
        em == em_full
    }
}

// -------------------------------------------------------------------------
// Generic sign / verify / derive
// -------------------------------------------------------------------------

fn dsa_sig_der(r: &BigUint, s: &BigUint) -> Vec<u8> {
    asn1::seq(&[asn1::uint(r), asn1::uint(s)])
}

fn parse_dsa_sig(sig: &[u8]) -> Option<(BigUint, BigUint)> {
    let t = asn1::parse_one(sig)?;
    if t.tag != asn1::SEQUENCE {
        return None;
    }
    let c = t.children()?;
    if c.len() != 2 || c[0].tag != asn1::INTEGER || c[1].tag != asn1::INTEGER {
        return None;
    }
    // reject non-minimal encodings (re-encode check)
    let r = c[0].uint();
    let s = c[1].uint();
    if dsa_sig_der(&r, &s) != sig {
        return None;
    }
    Some((r, s))
}

/// Errors from sign().
pub enum SignErr {
    /// Error already queued.
    Failed,
}

impl PKey {
    /// EVP_DigestSign()
    pub fn sign(&self, md: Option<Md>, data: &[u8], pss: bool) -> Result<Vec<u8>, SignErr> {
        match self {
            PKey::Rsa(r) => {
                let md = md.unwrap_or(Md::Sha256);
                let hash = md.digest(data);
                r.sign_digest(md, &hash, pss).ok_or(SignErr::Failed)
            }
            PKey::Dsa(f) => {
                let md = md.unwrap_or(Md::Sha256);
                let hash = md.digest(data);
                let (q, x) = match (&f.q, &f.priv_key) {
                    (Some(q), Some(x)) => (q, x),
                    _ => return Err(SignErr::Failed),
                };
                let qbits = q.bits();
                let mut z = bn(&hash);
                if hash.len() * 8 > qbits {
                    z >>= hash.len() * 8 - qbits;
                }
                loop {
                    let k = rand_below(q);
                    let r = crate::bn::modpow(&f.g, &k, &f.p) % q;
                    if r.is_zero() {
                        continue;
                    }
                    let kinv = mod_inv(&k, q).ok_or(SignErr::Failed)?;
                    let s = (kinv * ((&z + x * &r) % q)) % q;
                    if s.is_zero() {
                        continue;
                    }
                    return Ok(dsa_sig_der(&r, &s));
                }
            }
            PKey::Ec(e) if e.sm2 => {
                // OpenSSL 3 SM2 without a distinguishing ID: e = H(M), no Z
                let d = e.priv_key.as_ref().ok_or(SignErr::Failed)?;
                let md = md.unwrap_or(Md::Sm3);
                let (r, s) = e
                    .group
                    .sm2_sign(d, &md.digest(data))
                    .ok_or(SignErr::Failed)?;
                Ok(dsa_sig_der(&r, &s))
            }
            PKey::Ec(e) => {
                let d = e.priv_key.as_ref().ok_or(SignErr::Failed)?;
                let md = md.unwrap_or(if e.sm2 { Md::Sm3 } else { Md::Sha256 });
                let hash = md.digest(data);
                let (r, s) = e.group.ecdsa_sign(d, &hash);
                Ok(dsa_sig_der(&r, &s))
            }
            PKey::Ecx(x) => {
                if md.is_some() || !x.kind.is_signature() {
                    err::push(err::LIB_PROV, 133, "invalid digest");
                    return Err(SignErr::Failed);
                }
                ecx::sign(x.kind, x.priv_key.as_deref().ok_or(SignErr::Failed)?, data)
                    .ok_or(SignErr::Failed)
            }
            PKey::Dh(_) => {
                err::push(
                    err::LIB_EVP,
                    150,
                    "operation not supported for this keytype",
                );
                Err(SignErr::Failed)
            }
        }
    }

    /// EVP_DigestVerify(): 1 ok, 0 mismatch, -1 error.
    pub fn verify(&self, md: Option<Md>, data: &[u8], sig: &[u8], pss: bool) -> i64 {
        match self {
            PKey::Rsa(r) => {
                let md = md.unwrap_or(Md::Sha256);
                let hash = md.digest(data);
                if r.verify_digest(md, &hash, sig, pss) {
                    1
                } else {
                    err::push(err::LIB_RSA, 0x68, "bad signature");
                    0
                }
            }
            PKey::Dsa(f) => {
                let md = md.unwrap_or(Md::Sha256);
                let hash = md.digest(data);
                let (Some(q), Some(y)) = (&f.q, &f.pub_key) else {
                    return -1;
                };
                let Some((r, s)) = parse_dsa_sig(sig) else {
                    return 0;
                };
                if r.is_zero() || s.is_zero() || &r >= q || &s >= q {
                    return 0;
                }
                let qbits = q.bits();
                let mut z = bn(&hash);
                if hash.len() * 8 > qbits {
                    z >>= hash.len() * 8 - qbits;
                }
                let Some(w) = mod_inv(&s, q) else { return 0 };
                let u1 = (&z * &w) % q;
                let u2 = (&r * &w) % q;
                let v = ((crate::bn::modpow_public(&f.g, &u1, &f.p)
                    * crate::bn::modpow_public(y, &u2, &f.p))
                    % &f.p)
                    % q;
                if v == r { 1 } else { 0 }
            }
            PKey::Ec(e) if e.sm2 => {
                let md = md.unwrap_or(Md::Sm3);
                let Some(q) = &e.pub_key else { return -1 };
                let Some((r, s)) = parse_dsa_sig(sig) else {
                    return 0;
                };
                if e.group.sm2_verify(q, &md.digest(data), &r, &s) {
                    1
                } else {
                    0
                }
            }
            PKey::Ec(e) => {
                let md = md.unwrap_or(if e.sm2 { Md::Sm3 } else { Md::Sha256 });
                let hash = md.digest(data);
                let Some(q) = &e.pub_key else { return -1 };
                let Some((r, s)) = parse_dsa_sig(sig) else {
                    return 0;
                };
                if e.group.ecdsa_verify(q, &hash, &r, &s) {
                    1
                } else {
                    0
                }
            }
            PKey::Ecx(x) => {
                if md.is_some() || !x.kind.is_signature() {
                    err::push(err::LIB_PROV, 133, "invalid digest");
                    return -1;
                }
                if ecx::verify(x.kind, &x.pub_key, data, sig) {
                    1
                } else {
                    0
                }
            }
            PKey::Dh(_) => -1,
        }
    }

    /// EVP_PKEY_derive() with a peer key.
    pub fn derive(&self, peer: &PKey) -> Option<Vec<u8>> {
        match (self, peer) {
            (PKey::Dh(a), PKey::Dh(b)) => {
                let x = a.priv_key.as_ref()?;
                let y = b.pub_key.as_ref()?;
                if a.p != b.p || a.g != b.g {
                    err::push(err::LIB_EVP, 101, "different parameters");
                    return None;
                }
                let one = BigUint::one();
                if y <= &one || y >= &(&a.p - &one) {
                    err::push(err::LIB_DH, 102, "invalid public key");
                    return None;
                }
                let z = crate::bn::modpow(y, x, &a.p);
                Some(pad_be(&z, bytes_len(&a.p)))
            }
            (PKey::Ec(a), PKey::Ec(b)) => {
                if a.group.p != b.group.p || a.group.n != b.group.n {
                    err::push(err::LIB_EVP, 101, "different parameters");
                    return None;
                }
                let d = a.priv_key.as_ref()?;
                a.group.ecdh(d, b.pub_key.as_ref()?)
            }
            (PKey::Ecx(a), PKey::Ecx(b)) if a.kind == b.kind => {
                ecx::derive(a.kind, a.priv_key.as_deref()?, &b.pub_key)
            }
            _ => {
                err::push(err::LIB_EVP, 101, "different parameters");
                None
            }
        }
    }
}

/// php_openssl_dh_pub_from_priv(): g^priv mod p
pub fn dh_pub_from_priv(priv_key: &BigUint, g: &BigUint, p: &BigUint) -> BigUint {
    crate::bn::modpow(g, priv_key, p)
}

pub fn is_even(n: &BigUint) -> bool {
    n.is_even()
}
