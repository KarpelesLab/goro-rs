//! EVP_PKEY emulation: RSA, DSA, DH, EC and X25519/Ed25519/X448/Ed448 keys,
//! their DER/PEM encodings, generation and the low-level operations.

use crate::asn1::{self, Tlv};
use crate::bn::{self, Bn, Modulus, public};
use crate::cipher::{self, CipherSpec};
use crate::digest::{self, Md};
use crate::ec::{Group, Point};
use crate::ecx::{self, EcxKind};
use crate::err;
use purecrypto::bignum::BoxedUint;
use puremp::Nat;

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
    pub n: Bn,
    pub e: Bn,
    pub d: Option<Bn>,
    pub p: Option<Bn>,
    pub q: Option<Bn>,
    pub dmp1: Option<Bn>,
    pub dmq1: Option<Bn>,
    pub iqmp: Option<Bn>,
}

#[derive(Clone, Debug)]
pub struct Ffc {
    pub p: Bn,
    pub q: Option<Bn>,
    pub g: Bn,
    pub pub_key: Option<Bn>,
    pub priv_key: Option<Bn>,
}

#[derive(Clone, Debug)]
pub struct EcKey {
    pub group: Group,
    pub pub_key: Option<Point>,
    pub priv_key: Option<Bn>,
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

fn bn(b: &[u8]) -> Bn {
    Bn::from_be(b)
}

fn bytes_len(n: &Bn) -> usize {
    n.byte_len()
}

pub fn pad_be(n: &Bn, len: usize) -> Vec<u8> {
    n.pad(len)
}

/// FFC public value g^x mod p for a secret x (constant time); `None` when
/// p has no Montgomery form (even), as BN_mod_exp_mont() fails there.
fn ffc_pub(g: &Bn, x: &Bn, p: &Bn) -> Option<Bn> {
    let m = Modulus::new(p)?;
    Some(Bn::from_box(&m.pow_secret(&g.to_box(), &x.to_box())))
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
        let z = Bn::default();
        let g = |v: &Option<Bn>| asn1::uint(v.as_ref().unwrap_or(&z));
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

fn parse_ffc_params(t: &Tlv) -> Option<(Bn, Option<Bn>, Bn)> {
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
        gx: Bn::default(),
        gy: Bn::default(),
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
            let y = ffc_pub(&g, &x, &p);
            Some(PKey::Dsa(Ffc {
                p,
                q,
                g,
                pub_key: y,
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
            let y = ffc_pub(&g, &x, &p);
            Some(PKey::Dh(Ffc {
                p,
                q,
                g,
                pub_key: y,
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

/// Complete an RSA key from n, e, d, p, q: dP = d mod (p-1), dQ = d mod
/// (q-1) (constant-time long division) and qInv = q^-1 mod p (constant-time
/// binary xgcd).
fn rsa_with_crt(n: Bn, e: Bn, d: Bn, p: Bn, q: Bn) -> Rsa {
    let one = BoxedUint::from_u64(1);
    let db = d.to_box();
    let crt_exp = |pr: &Bn| {
        if pr.bits() < 2 {
            return Bn::default();
        }
        Bn::from_box(&db.reduce(&pr.to_box().sub(&one)))
    };
    let dmp1 = crt_exp(&p);
    let dmq1 = crt_exp(&q);
    let iqmp = Modulus::new(&p)
        .and_then(|mp| mp.inv(&q.to_box()))
        .map(|v| Bn::from_box(&v))
        .unwrap_or_default();
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

/// A random `bits`-bit probable prime with the top two bits set, from
/// purecrypto's constant-time prime generator (its runtime-sized variant
/// is crate-private, so dispatch to the smallest of a few fixed limb widths
/// that fits; a sparse set keeps the monomorphised code small).
fn random_prime(bits: usize) -> Option<BoxedUint> {
    macro_rules! dispatch {
        ($($l:literal)*) => {
            $(if bits <= $l * 64 {
                let p = purecrypto::rsa::random_prime::<$l, _>(
                    &mut purecrypto::rng::OsRng,
                    bits,
                    0,
                );
                return Some(BoxedUint::from_limbs(p.as_limbs().to_vec()));
            })*
        };
    }
    if bits < 2 {
        return None;
    }
    dispatch!(2 4 6 8 12 16 24 32 48 64);
    None
}

pub fn generate_rsa(bits: usize) -> Option<Rsa> {
    use purecrypto::bignum::inv_mod_ct_boxed;
    use purecrypto::rsa::BoxedRsaPrivateKey;
    let e = BoxedUint::from_u64(65537);
    if bits.is_multiple_of(2) && bits >= 512 {
        let k = BoxedRsaPrivateKey::generate(bits, e.clone(), &mut purecrypto::rng::OsRng, 0);
        let n = Bn::from_box(k.modulus());
        let d = Bn::from_box(k.private_exponent());
        let (p, q) = k.primes()?;
        let (p, q) = (Bn::from_box(p), Bn::from_box(q));
        return Some(rsa_with_crt(n, Bn::from_u64(65537), d, p, q));
    }
    // purecrypto's key generator only makes even sizes >= 512 bits: odd and
    // smaller sizes use unbalanced primes (as OpenSSL does) from its
    // constant-time prime generator, d = e^-1 mod phi from the constant-time
    // inverse.
    if bits < 16 {
        return None;
    }
    let one = BoxedUint::from_u64(1);
    loop {
        let p = random_prime(bits.div_ceil(2))?;
        let q = random_prime(bits / 2)?;
        if bool::from(purecrypto::ct::ConstantTimeEq::ct_eq(&p, &q)) {
            continue;
        }
        let n = Bn::from_box(&p.mul(&q));
        if n.bits() != bits {
            continue;
        }
        let phi = p.sub(&one).mul(&q.sub(&one));
        let Some(d) = inv_mod_ct_boxed(&e, &phi).into_option() else {
            continue;
        };
        return Some(rsa_with_crt(
            n,
            Bn::from_u64(65537),
            Bn::from_box(&d),
            Bn::from_box(&p),
            Bn::from_box(&q),
        ));
    }
}

pub fn generate_ec(group: Group) -> EcKey {
    let d = Bn::from_box(&group.random_scalar());
    let q = group.pub_from_priv(&d);
    let sm2 = group.oid == Some(OID_SM2);
    EcKey {
        group,
        pub_key: Some(q),
        priv_key: Some(d),
        sm2,
    }
}

/// FFC key generation from (p, q, g): DSA/DH. The private value is drawn
/// from the OS CSPRNG and the public one computed in constant time.
pub fn generate_ffc(p: &Bn, q: Option<&Bn>, g: &Bn) -> (Bn, Bn) {
    let x = match q {
        Some(q) => bn::random_below(q),
        // private value length = bits(p) - 1
        None => bn::random_bits_below(p.bits().saturating_sub(1).max(1), p),
    };
    let x = Bn::from_box(&x);
    let y = ffc_pub(g, &x, p).unwrap_or_default();
    (x, y)
}

/// ossl_ffc_params_simple_validate(): basic p/q/g sanity checks (public).
pub fn ffc_simple_validate(p: &Bn, q: Option<&Bn>, g: &Bn) -> bool {
    let one = Bn::from_u64(1);
    if p.is_zero() || p.is_even() {
        return false;
    }
    let pm1 = Bn::from_nat(&public::sub(&p.nat(), &Nat::one()));
    if g <= &one || g >= &pm1 {
        return false;
    }
    if let Some(q) = q {
        if q.is_zero() || q.is_even() || q >= p {
            return false;
        }
        if bn::modpow_public(g, q, p) != one {
            return false;
        }
    }
    true
}

/// Random public `bits`-bit natural with the top bit (and optionally the
/// low bit) set.
fn random_nat(bits: usize, odd: bool) -> Nat {
    use purecrypto::rng::RngCore;
    let mut b = vec![0u8; bits.div_ceil(8)];
    purecrypto::rng::OsRng.fill_bytes(&mut b);
    let top = (bits - 1) % 8;
    b[0] &= ((1u16 << (top + 1)) - 1) as u8;
    b[0] |= 1 << top;
    if odd {
        *b.last_mut().unwrap() |= 1;
    }
    Bn::from_be(&b).nat()
}

const SMALL_PRIMES: &[u64] = &[
    3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83, 89, 97,
    101, 103, 107, 109, 113, 127, 131, 137, 139, 149, 151, 157, 163, 167, 173, 179, 181, 191, 193,
    197, 199, 211, 223, 227, 229, 233, 239, 241, 251, 257, 263, 269, 271, 277, 281, 283, 293, 307,
    311, 313, 317, 331, 337, 347, 349, 353, 359, 367, 373, 379, 383, 389, 397, 401, 409, 419, 421,
    431, 433, 439, 443, 449, 457, 461, 463, 467, 479, 487, 491, 499, 503, 509, 521, 523, 541, 547,
    557, 563, 569, 571, 577, 587, 593, 599, 601, 607, 613, 617, 619, 631, 641, 643, 647, 653, 659,
    661, 673, 677, 683, 691, 701, 709, 719, 727, 733, 739, 743, 751, 757, 761, 769, 773, 787, 797,
    809, 811, 821, 823, 827, 829, 839, 853, 857, 859, 863, 877, 881, 883, 887, 907, 911, 919, 929,
    937, 941, 947, 953, 967, 971, 977, 983, 991, 997,
];

fn small_rem(n: &Nat, d: u64) -> u64 {
    public::rem(n, &Nat::from_u64(d)).to_u64().unwrap_or(0)
}

/// Public probable-prime test (parameters, never key material).
fn is_probable_prime(n: &Nat) -> bool {
    public::is_probable_prime(n, 8)
}

/// DSA parameter generation (FIPS 186-4 style probable primes; public).
pub fn generate_dsa_params(pbits: usize) -> Option<(Bn, Bn, Bn)> {
    let qbits = if pbits >= 2048 {
        224
    } else {
        224.min(pbits / 2).max(160)
    };
    if pbits < qbits + 2 {
        return None;
    }
    let one = Nat::one();
    for _ in 0..10000 {
        let q = random_nat(qbits, true);
        if !is_probable_prime(&q) {
            continue;
        }
        let q2 = q.shl(1);
        for _ in 0..4096 {
            let x = random_nat(pbits, false);
            let c = public::rem(&x, &q2);
            let p = public::sub(&x, &c).add(&one);
            if (p.bit_len() as usize) < pbits {
                continue;
            }
            if SMALL_PRIMES
                .iter()
                .any(|&sp| small_rem(&p, sp) == 0 && p != Nat::from_u64(sp))
            {
                continue;
            }
            if is_probable_prime(&p) {
                let e = public::div(&public::sub(&p, &one), &q);
                let mut h = Nat::from_u64(2);
                loop {
                    let g = h.modpow(&e, &p);
                    if g > one {
                        return Some((Bn::from_nat(&p), Bn::from_nat(&q), Bn::from_nat(&g)));
                    }
                    h = h.add(&one);
                }
            }
        }
    }
    None
}

/// DH safe-prime parameter generation with generator 2 (public).
pub fn generate_dh_params(pbits: usize) -> Option<(Bn, Bn)> {
    if pbits < 512 {
        return None;
    }
    let one = Nat::one();
    let two = Nat::from_u64(2);
    loop {
        // p = 2q + 1 with p = 23 mod 24 (so that 2 is a quadratic residue
        // generator of the q-order subgroup, as OpenSSL does).
        let q = random_nat(pbits - 1, true);
        let p = q.shl(1).add(&one);
        if small_rem(&p, 24) != 23 {
            continue;
        }
        if SMALL_PRIMES
            .iter()
            .any(|&sp| small_rem(&q, sp) == 0 || small_rem(&p, sp) == 0)
        {
            continue;
        }
        // Cheap Fermat screens before the full tests.
        if two.modpow(&public::sub(&q, &one), &q) != one
            || two.modpow(&public::sub(&p, &one), &p) != one
        {
            continue;
        }
        if is_probable_prime(&q) && is_probable_prime(&p) {
            return Some((Bn::from_nat(&p), Bn::from_u64(2)));
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

    /// m^e mod n. The exponent is public; purecrypto's `pow_public` keeps a
    /// secret base (an encryption's padded plaintext) off any branch or
    /// table index.
    fn public_op(&self, m: &Bn) -> Option<Vec<u8>> {
        let Some(nm) = Modulus::new(&self.n) else {
            err::push(err::LIB_BN, 108, "no inverse");
            return None;
        };
        let mb = m.to_box();
        if !mb.lt(nm.modulus()) {
            rsa_err(132, "data too large for modulus");
            return None;
        }
        Some(pad_be(
            &Bn::from_box(&nm.pow_public(&mb, &self.e.to_box())),
            self.k(),
        ))
    }

    /// c^d mod n in constant time. With the CRT components it runs two
    /// half-width ladders (mod p and q), Garner recombination in Montgomery
    /// form, and checks the result against the public exponent (falling
    /// back to the full-width ladder for inconsistent components).
    ///
    /// purecrypto's `BoxedRsaPrivateKey` has a blinded CRT private
    /// operation, but only behind its padding schemes (no raw RSA), so the
    /// same constant-time primitives are used directly here.
    fn private_op(&self, c: &Bn) -> Option<Vec<u8>> {
        let Some(nm) = Modulus::new(&self.n) else {
            err::push(err::LIB_BN, 108, "no inverse");
            return None;
        };
        let cb = c.to_box();
        if !cb.lt(nm.modulus()) {
            rsa_err(132, "data too large for modulus");
            return None;
        }
        let d = self.d.as_ref()?;
        if let Some(m) = self.private_op_crt(&nm, &cb) {
            return Some(nm.to_bytes(&m));
        }
        Some(nm.to_bytes(&nm.pow_secret(&cb, &d.to_box())))
    }

    fn private_op_crt(&self, nm: &Modulus, c: &BoxedUint) -> Option<BoxedUint> {
        let (p, q) = (self.p.as_ref()?, self.q.as_ref()?);
        let (dp, dq, qinv) = (
            self.dmp1.as_ref()?,
            self.dmq1.as_ref()?,
            self.iqmp.as_ref()?,
        );
        if p.is_zero() || q.is_zero() || dp.is_zero() || dq.is_zero() || qinv.is_zero() {
            return None;
        }
        let mp = Modulus::new(p)?;
        let mq = Modulus::new(q)?;
        let m1 = mp.pow_secret(c, &dp.to_box());
        let m2 = mq.pow_secret(c, &dq.to_box());
        // h = qInv (m1 - m2) mod p ; m = m2 + h q
        let h = mp.mul(&qinv.to_box(), &mp.sub(&m1, &m2));
        let m = nm.reduce(&m2.add(&h.mul(&q.to_box())));
        // Fault / consistency check: m^e == c (public exponent).
        let back = nm.pow_public(&m, &self.e.to_box());
        if back == nm.reduce(c) { Some(m) } else { None }
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
                rsa_err(118, "unknown padding type");
                return None;
            }
        };
        self.public_op(&bn(&em))
    }

    /// The PKCS#1 v1.5 implicit-rejection key derivation key
    /// (rsa_ossl.c derive_kdk()): HMAC-SHA256 keyed with SHA256(d) over
    /// the ciphertext left-padded to the modulus size.
    fn implicit_rejection_kdk(&self, ct: &[u8]) -> Option<Vec<u8>> {
        let k = self.k();
        let d = self.d.as_ref()?;
        let d_hash = Md::Sha256.digest(&pad_be(d, k));
        let mut msg = vec![0u8; k.saturating_sub(ct.len())];
        msg.extend_from_slice(ct);
        Some(crate::digest::hmac(Md::Sha256, &d_hash, &msg))
    }

    /// rsa_pk1.c ossl_rsa_prf()
    fn implicit_rejection_prf(kdk: &[u8], label: &[u8], len: usize) -> Vec<u8> {
        let bitlen = ((len * 8) as u16).to_be_bytes();
        let mut out = Vec::with_capacity(len + 32);
        let mut iter: u16 = 0;
        while out.len() < len {
            let mut m = iter.to_be_bytes().to_vec();
            m.extend_from_slice(label);
            m.extend_from_slice(&bitlen);
            out.extend(crate::digest::hmac(Md::Sha256, kdk, &m));
            iter = iter.wrapping_add(1);
        }
        out.truncate(len);
        out
    }

    /// ossl_rsa_padding_check_PKCS1_type_2(): a bad padding yields a
    /// synthetic message derived from the key and ciphertext instead of an
    /// error (OpenSSL 3.2+ implicit rejection).
    fn pkcs1_type2_implicit(&self, ct: &[u8], em: &[u8]) -> Option<Vec<u8>> {
        let flen = em.len();
        let kdk = self.implicit_rejection_kdk(ct)?;
        let synthetic = Self::implicit_rejection_prf(&kdk, b"message", flen);
        let cands = Self::implicit_rejection_prf(&kdk, b"length", 256);
        let max_sep_offset = (flen - 10) as u16;
        let mut mask = max_sep_offset;
        mask |= mask >> 1;
        mask |= mask >> 2;
        mask |= mask >> 4;
        mask |= mask >> 8;
        let mut synthetic_length = 0usize;
        for c in cands.chunks(2) {
            let l = u16::from_be_bytes([c[0], c[1]]) & mask;
            if l < max_sep_offset {
                synthetic_length = l as usize;
            }
        }
        let good_hdr = em[0] == 0 && em[1] == 2;
        let zero = em[2..].iter().position(|&b| b == 0).map(|z| z + 2);
        match zero {
            Some(z) if good_hdr && z >= 10 => Some(em[z + 1..].to_vec()),
            _ => Some(synthetic[flen - synthetic_length..].to_vec()),
        }
    }

    /// RSA private decrypt (EVP_PKEY_decrypt).
    pub fn decrypt(&self, data: &[u8], padding: i64, oaep_md: Md) -> Option<Vec<u8>> {
        let k = self.k();
        if data.len() > k {
            rsa_err(108, "data greater than mod len");
            return None;
        }
        let em = self.private_op(&bn(data))?;
        match padding {
            RSA_NO_PADDING => Some(em),
            RSA_PKCS1_PADDING => self.pkcs1_type2_implicit(data, &em),
            RSA_PKCS1_OAEP_PADDING => {
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
            _ => {
                rsa_err(118, "unknown padding type");
                None
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
        Self::check_pkcs1_type1(&em).map(|t| t.to_vec())
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

    /// rsa_ossl_public_decrypt() without the padding removal: s^e mod n as
    /// `k` bytes (shorter inputs are accepted, as for RSA_NO_PADDING).
    fn public_decrypt_raw(&self, sig: &[u8]) -> Option<Vec<u8>> {
        let k = self.k();
        if sig.len() > k {
            rsa_err(108, "data greater than mod len");
            return None;
        }
        let s = bn(sig);
        if s >= self.n {
            rsa_err(132, "data too large for modulus");
            return None;
        }
        let nm = Modulus::new(&self.n)?;
        Some(pad_be(
            &Bn::from_box(&nm.pow_public(&s.to_box(), &self.e.to_box())),
            k,
        ))
    }

    /// RSA_padding_check_PKCS1_type_1() on a `k`-byte block, raising the
    /// reason and then rsa_ossl_public_decrypt()'s "padding check failed".
    fn check_pkcs1_type1(em: &[u8]) -> Option<&[u8]> {
        let fail = |reason: u32, s: &str| {
            rsa_err(reason, s);
            rsa_err(114, "padding check failed");
            None
        };
        if em.len() < 11 {
            rsa_err(114, "padding check failed");
            return None;
        }
        if em[0] != 0 {
            return fail(138, "invalid padding");
        }
        if em[1] != 1 {
            return fail(106, "block type is not 01");
        }
        let body = &em[2..];
        let mut i = 0;
        while i < body.len() && body[i] == 0xff {
            i += 1;
        }
        if i == body.len() {
            return fail(113, "null before block missing");
        }
        if body[i] != 0 {
            return fail(102, "bad fixed header decrypt");
        }
        if i < 8 {
            return fail(103, "bad pad byte count");
        }
        Some(&body[i + 1..])
    }

    /// RSA_verify() (PKCS#1 v1.5) or the PSS provider path; errors are
    /// queued the way OpenSSL raises them (the caller adds "RSA lib").
    pub fn verify_digest(&self, md: Md, hash: &[u8], sig: &[u8], pss: bool) -> bool {
        let k = self.k();
        if pss {
            // RSA_public_decrypt(RSA_NO_PADDING) then
            // ossl_rsa_verify_PKCS1_PSS_mgf1_ex() with an automatic salt.
            let Some(em_full) = self.public_decrypt_raw(sig) else {
                return false;
            };
            let hlen = md.size();
            let msbits = (self.n.bits() - 1) & 7;
            if em_full[0] & (0xffu8 << msbits) != 0 {
                rsa_err(133, "first octet invalid");
                return false;
            }
            let em = if msbits == 0 { &em_full[1..] } else { &em_full[..] };
            let emlen = em.len();
            if emlen < hlen + 2 {
                rsa_err(109, "data too large");
                return false;
            }
            if em[emlen - 1] != 0xbc {
                rsa_err(134, "last octet invalid");
                return false;
            }
            let masked_len = emlen - hlen - 1;
            let h = &em[masked_len..emlen - 1];
            let mut db = Self::mgf1(md, h, masked_len);
            for (a, b) in db.iter_mut().zip(&em[..masked_len]) {
                *a ^= b;
            }
            if msbits != 0 {
                db[0] &= 0xff >> (8 - msbits);
            }
            let mut i = 0;
            while db[i] == 0 && i < masked_len - 1 {
                i += 1;
            }
            if db[i] != 1 {
                rsa_err(135, "slen recovery failed");
                return false;
            }
            let salt = &db[i + 1..];
            let mut mp = vec![0u8; 8];
            mp.extend_from_slice(hash);
            mp.extend_from_slice(salt);
            if md.digest(&mp) != h {
                rsa_err(104, "bad signature");
                return false;
            }
            return true;
        }
        if sig.len() != k {
            rsa_err(119, "wrong signature length");
            return false;
        }
        let Some(em) = self.public_decrypt_raw(sig) else {
            return false;
        };
        let Some(t) = Self::check_pkcs1_type1(&em) else {
            return false;
        };
        if t != Self::digest_info(md, hash).as_slice() {
            rsa_err(104, "bad signature");
            return false;
        }
        true
    }
}

// -------------------------------------------------------------------------
// Generic sign / verify / derive
// -------------------------------------------------------------------------

fn dsa_sig_der(r: &Bn, s: &Bn) -> Vec<u8> {
    asn1::seq(&[asn1::uint(r), asn1::uint(s)])
}

fn parse_dsa_sig(sig: &[u8]) -> Option<(Bn, Bn)> {
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

/// ossl_digest_rsa_sign_get_md_nid(): the digests RSA signatures take.
fn rsa_md_allowed(md: Md) -> bool {
    !matches!(
        md,
        Md::Shake128 | Md::Shake256 | Md::Blake2b512 | Md::Blake2s256 | Md::Whirlpool
    )
}

/// ossl_digest_get_approved_nid(): SHA-1, SHA-2 and SHA-3 only.
fn dsa_md_allowed(md: Md) -> bool {
    matches!(
        md,
        Md::Sha1
            | Md::Sha224
            | Md::Sha256
            | Md::Sha384
            | Md::Sha512
            | Md::Sha512_224
            | Md::Sha512_256
            | Md::Sha3_224
            | Md::Sha3_256
            | Md::Sha3_384
            | Md::Sha3_512
    )
}

fn prov_digest_not_allowed() {
    err::push(err::LIB_PROV, 174, "digest not allowed");
}

fn prov_invalid_digest() {
    err::push(err::LIB_PROV, 122, "invalid digest");
}

/// The RSA provider's ERR_raise(ERR_LIB_PROV, ERR_R_RSA_LIB) after a
/// failed RSA_*() call.
pub fn prov_rsa_lib() {
    err::push(err::LIB_PROV, err::RFLAG_COMMON | 4, "RSA lib");
}

impl PKey {
    /// EVP_DigestSign()
    pub fn sign(&self, md: Option<Md>, data: &[u8], pss: bool) -> Result<Vec<u8>, SignErr> {
        match self {
            PKey::Rsa(r) => {
                let md = md.unwrap_or(Md::Sha256);
                if !rsa_md_allowed(md) {
                    prov_digest_not_allowed();
                    return Err(SignErr::Failed);
                }
                let hash = md.digest(data);
                r.sign_digest(md, &hash, pss).ok_or_else(|| {
                    prov_rsa_lib();
                    SignErr::Failed
                })
            }
            PKey::Dsa(f) => {
                let md = md.unwrap_or(Md::Sha256);
                if !dsa_md_allowed(md) {
                    prov_digest_not_allowed();
                    return Err(SignErr::Failed);
                }
                let hash = md.digest(data);
                let (q, x) = match (&f.q, &f.priv_key) {
                    (Some(q), Some(x)) => (q, x),
                    _ => return Err(SignErr::Failed),
                };
                dsa_sign(f, q, x, &hash)
                    .map(|(r, s)| dsa_sig_der(&r, &s))
                    .ok_or(SignErr::Failed)
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
                let md = md.unwrap_or(Md::Sha256);
                if md.is_xof() {
                    prov_invalid_digest();
                    return Err(SignErr::Failed);
                }
                let hash = md.digest(data);
                let (r, s) = e.group.ecdsa_sign(d, &hash).ok_or(SignErr::Failed)?;
                Ok(dsa_sig_der(&r, &s))
            }
            PKey::Ecx(x) => {
                if md.is_some() || !x.kind.is_signature() {
                    prov_invalid_digest();
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

    /// EVP_DigestVerify(): 1 ok, 0 mismatch, -1 error. A mismatch the
    /// provider did not explain leaves "provider signature failure".
    pub fn verify(&self, md: Option<Md>, data: &[u8], sig: &[u8], pss: bool) -> i64 {
        let queued = err::mark().len();
        let r = self.verify_inner(md, data, sig, pss);
        if r == 0 && err::mark().len() == queued {
            err::push(err::LIB_EVP, 234, "provider signature failure");
        }
        r
    }

    fn verify_inner(&self, md: Option<Md>, data: &[u8], sig: &[u8], pss: bool) -> i64 {
        match self {
            PKey::Rsa(r) => {
                let md = md.unwrap_or(Md::Sha256);
                if !rsa_md_allowed(md) {
                    prov_digest_not_allowed();
                    return -1;
                }
                let hash = md.digest(data);
                if r.verify_digest(md, &hash, sig, pss) {
                    1
                } else {
                    prov_rsa_lib();
                    0
                }
            }
            PKey::Dsa(f) => {
                let md = md.unwrap_or(Md::Sha256);
                if !dsa_md_allowed(md) {
                    prov_digest_not_allowed();
                    return -1;
                }
                let hash = md.digest(data);
                let (Some(q), Some(y)) = (&f.q, &f.pub_key) else {
                    return -1;
                };
                let Some((r, s)) = parse_dsa_sig(sig) else {
                    return -1;
                };
                if r.is_zero() || s.is_zero() || &r >= q || &s >= q {
                    return 0;
                }
                if dsa_verify(f, q, y, &hash, &r, &s) {
                    1
                } else {
                    0
                }
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
                let md = md.unwrap_or(Md::Sha256);
                if md.is_xof() {
                    prov_invalid_digest();
                    return -1;
                }
                let hash = md.digest(data);
                let Some(q) = &e.pub_key else { return -1 };
                let Some((r, s)) = parse_dsa_sig(sig) else {
                    return -1;
                };
                if r.is_zero() || s.is_zero() || r >= e.group.n || s >= e.group.n {
                    // ossl_ecdsa_simple_verify_sig()
                    err::push(err::LIB_EC, 156, "bad signature");
                    return 0;
                }
                if e.group.ecdsa_verify(q, &hash, &r, &s) {
                    1
                } else {
                    0
                }
            }
            PKey::Ecx(x) => {
                if md.is_some() || !x.kind.is_signature() {
                    prov_invalid_digest();
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
                let one = Bn::from_u64(1);
                let pm1 = Bn::from_nat(&public::sub(&a.p.nat(), &one.nat()));
                if y <= &one || y >= &pm1 {
                    err::push(err::LIB_DH, 102, "invalid public key");
                    return None;
                }
                let Some(m) = Modulus::new(&a.p) else {
                    err::push(err::LIB_BN, 102, "called a function you should not call");
                    return None;
                };
                Some(m.to_bytes(&m.pow_secret(&y.to_box(), &x.to_box())))
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

/// php_openssl_dh_pub_from_priv(): g^priv mod p (constant time in priv)
pub fn dh_pub_from_priv(priv_key: &Bn, g: &Bn, p: &Bn) -> Option<Bn> {
    ffc_pub(g, priv_key, p)
}

pub fn is_even(n: &Bn) -> bool {
    n.is_even()
}

/// DSA signature over a digest. Mirrors purecrypto's
/// `DsaPrivateKey::sign_prehash` (which only accepts FIPS 186-4 parameter
/// sizes and derives RFC 6979 nonces from a compile-time digest type) on its
/// constant-time primitives: `g^k mod p` by the fixed-window ladder over the
/// public width of q, `k^-1 mod q` by the constant-time binary xgcd, and
/// `k^-1 (z + x r) mod q` in Montgomery form.
fn dsa_sign(f: &Ffc, q: &Bn, x: &Bn, hash: &[u8]) -> Option<(Bn, Bn)> {
    let mp = Modulus::new(&f.p)?;
    let mq = Modulus::new(q)?;
    let qbits = q.bits();
    let mut z = BoxedUint::from_be_bytes(hash);
    if hash.len() * 8 > qbits {
        z = z.shr_bits(hash.len() * 8 - qbits);
    }
    let z = mq.reduce(&z);
    let x = mq.reduce(&x.to_box());
    let g = f.g.to_box();
    for _ in 0..64 {
        let k = bn::random_below(q);
        let r = mq.reduce(&mp.pow_secret_bits(&g, &k, qbits));
        if r.is_zero() {
            continue;
        }
        let kinv = mq.inv(&k)?;
        let s = mq.mul(&kinv, &mq.add(&z, &mq.mul(&x, &r)));
        if s.is_zero() {
            continue;
        }
        return Some((Bn::from_box(&r), Bn::from_box(&s)));
    }
    None
}

/// DSA verification (public values only).
fn dsa_verify(f: &Ffc, q: &Bn, y: &Bn, hash: &[u8], r: &Bn, s: &Bn) -> bool {
    if r.is_zero() || s.is_zero() || r >= q || s >= q {
        return false;
    }
    let (Some(mp), Some(mq)) = (Modulus::new(&f.p), Modulus::new(q)) else {
        return false;
    };
    let qbits = q.bits();
    let mut z = BoxedUint::from_be_bytes(hash);
    if hash.len() * 8 > qbits {
        z = z.shr_bits(hash.len() * 8 - qbits);
    }
    let Some(w) = public::inv(&s.nat(), &q.nat()) else {
        return false;
    };
    let w = Bn::from_nat(&w).to_box();
    let u1 = mq.mul(&z, &w);
    let u2 = mq.mul(&r.to_box(), &w);
    let v = mp.mul(
        &mp.pow_public(&f.g.to_box(), &u1),
        &mp.pow_public(&y.to_box(), &u2),
    );
    Bn::from_box(&mq.reduce(&v)) == *r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rsa_crt_matches_full_ladder() {
        for bits in [512usize, 777, 1024] {
            let k = generate_rsa(bits).unwrap();
            assert_eq!(k.n.bits(), bits);
            let c = Bn::from_be(&[0x42u8; 40]);
            let crt = k.private_op(&c).unwrap();
            let nm = Modulus::new(&k.n).unwrap();
            let full = nm.to_bytes(&nm.pow_secret(&c.to_box(), &k.d.as_ref().unwrap().to_box()));
            assert_eq!(crt, full, "{bits}");
            assert_eq!(k.public_op(&Bn::from_be(&crt)).unwrap(), c.pad(k.k()));
            // Inconsistent CRT values fall back to the full ladder.
            let mut bad = k.clone();
            bad.iqmp = Some(Bn::from_u64(3));
            assert_eq!(bad.private_op(&c).unwrap(), full);
        }
    }

    #[test]
    fn dsa_sign_verify() {
        let (p, q, g) = generate_dsa_params(1024).unwrap();
        let (x, y) = generate_ffc(&p, Some(&q), &g);
        let f = Ffc {
            p,
            q: Some(q.clone()),
            g,
            pub_key: Some(y.clone()),
            priv_key: Some(x.clone()),
        };
        let h = [9u8; 32];
        let (r, s) = dsa_sign(&f, &q, &x, &h).unwrap();
        assert!(dsa_verify(&f, &q, &y, &h, &r, &s));
        assert!(!dsa_verify(&f, &q, &y, &[8u8; 32], &r, &s));
    }
}
