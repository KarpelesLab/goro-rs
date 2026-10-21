//! X.509 certificate and PKCS#10 request models (DER level), signature
//! algorithm tables, signing and verification.

use crate::asn1::{self, Tlv};
use crate::digest::Md;
use crate::name::Name;
use crate::pkey::{self, PKey};

#[derive(Clone, Debug)]
pub struct Ext {
    pub oid: String,
    pub critical: bool,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Cert {
    pub der: Vec<u8>,
    pub tbs: Vec<u8>,
    /// 0-based version (X509_get_version)
    pub version: i64,
    /// INTEGER content (two's complement, big endian)
    pub serial: Vec<u8>,
    pub tbs_sig_alg: Vec<u8>,
    pub issuer: Name,
    /// Original DER of the issuer Name.
    pub issuer_der: Vec<u8>,
    pub subject: Name,
    pub not_before: (u8, Vec<u8>),
    pub not_after: (u8, Vec<u8>),
    pub spki: Vec<u8>,
    pub extensions: Vec<Ext>,
    pub sig_alg_oid: String,
    pub sig_alg: Vec<u8>,
    pub signature: Vec<u8>,
}

fn parse_extensions(t: &Tlv) -> Option<Vec<Ext>> {
    let mut out = Vec::new();
    for e in t.children()? {
        let c = e.children()?;
        if c.len() < 2 {
            return None;
        }
        let oid = c[0].oid()?;
        let (critical, val) = if c.len() == 3 {
            (c[1].content.first().copied().unwrap_or(0) != 0, c[2])
        } else {
            (false, c[1])
        };
        if val.tag != asn1::OCTET_STRING {
            return None;
        }
        out.push(Ext {
            oid,
            critical,
            value: val.content.to_vec(),
        });
    }
    Some(out)
}

pub fn encode_extensions(exts: &[Ext]) -> Vec<u8> {
    let items: Vec<Vec<u8>> = exts
        .iter()
        .map(|e| {
            let mut v = vec![asn1::oid(&e.oid)];
            if e.critical {
                v.push(asn1::boolean(true));
            }
            v.push(asn1::octet(&e.value));
            asn1::seq(&v)
        })
        .collect();
    asn1::seq(&items)
}

impl Cert {
    pub fn parse(der: &[u8]) -> Option<Cert> {
        let t = asn1::parse_one(der)?;
        let c = t.children()?;
        if c.len() != 3 {
            return None;
        }
        let tbs = c[0];
        let tc = tbs.children()?;
        let mut i = 0;
        let mut version = 0;
        if tc.first()?.context() == Some(0) {
            version = asn1::parse_one(tc[0].content)?.as_i64()?;
            i = 1;
        }
        let serial = tc.get(i)?;
        if serial.tag != asn1::INTEGER {
            return None;
        }
        let tbs_sig_alg = tc.get(i + 1)?;
        let issuer_t = tc.get(i + 2)?;
        let issuer = Name::parse(issuer_t)?;
        let validity = tc.get(i + 3)?.children()?;
        if validity.len() != 2 {
            return None;
        }
        let subject = Name::parse(tc.get(i + 4)?)?;
        let spki = tc.get(i + 5)?;
        let mut extensions = Vec::new();
        for x in &tc[i + 6..] {
            if x.context() == Some(3) {
                extensions = parse_extensions(&asn1::parse_one(x.content)?)?;
            }
        }
        let sig_alg_oid = c[1].children()?.first()?.oid()?;
        let signature = c[2].bits()?.to_vec();
        Some(Cert {
            der: der.to_vec(),
            tbs: tbs.raw.to_vec(),
            version,
            serial: serial.content.to_vec(),
            tbs_sig_alg: tbs_sig_alg.raw.to_vec(),
            issuer,
            issuer_der: issuer_t.raw.to_vec(),
            subject,
            not_before: (validity[0].tag, validity[0].content.to_vec()),
            not_after: (validity[1].tag, validity[1].content.to_vec()),
            spki: spki.raw.to_vec(),
            extensions,
            sig_alg_oid,
            sig_alg: c[1].raw.to_vec(),
            signature,
        })
    }

    pub fn ext(&self, oid: &str) -> Option<&Ext> {
        self.extensions.iter().find(|e| e.oid == oid)
    }

    pub fn public_key(&self) -> Option<PKey> {
        pkey::parse_spki(&self.spki)
    }

    pub fn pem(&self) -> String {
        asn1::pem_encode("CERTIFICATE", &self.der)
    }

    /// DER INTEGER of the serial number.
    pub fn serial_der(&self) -> Vec<u8> {
        asn1::tlv(asn1::INTEGER, &self.serial)
    }

    /// X509_verify(): 1 ok, 0 bad signature, -1 error.
    pub fn verify(&self, key: &PKey) -> i64 {
        verify_signed(&self.tbs, &self.sig_alg, &self.signature, key)
    }

    /// Subject key identifier (extension value) if present.
    pub fn skid(&self) -> Option<Vec<u8>> {
        let e = self.ext("2.5.29.14")?;
        let t = asn1::parse_one(&e.value)?;
        Some(t.content.to_vec())
    }
}

pub fn spki_bits(spki: &[u8]) -> Option<Vec<u8>> {
    let t = asn1::parse_one(spki)?;
    let c = t.children()?;
    Some(c.get(1)?.bits()?.to_vec())
}

#[derive(Clone, Debug)]
pub struct Csr {
    pub der: Vec<u8>,
    pub info: Vec<u8>,
    pub version: i64,
    pub subject: Name,
    pub spki: Vec<u8>,
    /// (oid, raw SET content values)
    pub attributes: Vec<(String, Vec<u8>)>,
    pub sig_alg: Vec<u8>,
    pub signature: Vec<u8>,
}

impl Csr {
    pub fn parse(der: &[u8]) -> Option<Csr> {
        let t = asn1::parse_one(der)?;
        let c = t.children()?;
        if c.len() != 3 {
            return None;
        }
        let info = c[0];
        let ic = info.children()?;
        if ic.len() < 3 {
            return None;
        }
        let version = ic[0].as_i64()?;
        let subject = Name::parse(&ic[1])?;
        let spki = ic[2].raw.to_vec();
        let mut attributes = Vec::new();
        if let Some(a) = ic.get(3) {
            if a.context() == Some(0) {
                for attr in asn1::parse_all(a.content)? {
                    let ac = attr.children()?;
                    if ac.len() != 2 {
                        return None;
                    }
                    attributes.push((ac[0].oid()?, ac[1].content.to_vec()));
                }
            }
        }
        Some(Csr {
            der: der.to_vec(),
            info: info.raw.to_vec(),
            version,
            subject,
            spki,
            attributes,
            sig_alg: c[1].raw.to_vec(),
            signature: c[2].bits()?.to_vec(),
        })
    }

    pub fn public_key(&self) -> Option<PKey> {
        pkey::parse_spki(&self.spki)
    }

    pub fn pem(&self) -> String {
        asn1::pem_encode("CERTIFICATE REQUEST", &self.der)
    }

    pub fn verify(&self, key: &PKey) -> i64 {
        verify_signed(&self.info, &self.sig_alg, &self.signature, key)
    }

    /// Requested extensions (PKCS#9 extensionRequest).
    pub fn extensions(&self) -> Vec<Ext> {
        for (oid, vals) in &self.attributes {
            if oid == "1.2.840.113549.1.9.14" || oid == "1.3.6.1.4.1.311.2.1.14" {
                if let Some(t) = asn1::parse_all(vals).and_then(|v| v.first().copied()) {
                    if let Some(e) = parse_extensions(&t) {
                        return e;
                    }
                }
            }
        }
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Signature algorithms
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SigKind {
    Rsa,
    RsaPss,
    Dsa,
    Ecdsa,
    Sm2,
    Ed25519,
    Ed448,
}

/// (md, kind) for a signature algorithm OID.
pub fn sig_alg_info(oid: &str) -> Option<(Option<Md>, SigKind)> {
    use SigKind::*;
    Some(match oid {
        "1.2.840.113549.1.1.3" => (Some(Md::Md4), Rsa),
        "1.2.840.113549.1.1.4" => (Some(Md::Md5), Rsa),
        "1.2.840.113549.1.1.5" | "1.3.14.3.2.29" => (Some(Md::Sha1), Rsa),
        "1.2.840.113549.1.1.14" => (Some(Md::Sha224), Rsa),
        "1.2.840.113549.1.1.11" => (Some(Md::Sha256), Rsa),
        "1.2.840.113549.1.1.12" => (Some(Md::Sha384), Rsa),
        "1.2.840.113549.1.1.13" => (Some(Md::Sha512), Rsa),
        "1.2.840.113549.1.1.15" => (Some(Md::Sha512_224), Rsa),
        "1.2.840.113549.1.1.16" => (Some(Md::Sha512_256), Rsa),
        "2.16.840.1.101.3.4.3.13" => (Some(Md::Sha3_224), Rsa),
        "2.16.840.1.101.3.4.3.14" => (Some(Md::Sha3_256), Rsa),
        "2.16.840.1.101.3.4.3.15" => (Some(Md::Sha3_384), Rsa),
        "2.16.840.1.101.3.4.3.16" => (Some(Md::Sha3_512), Rsa),
        "1.3.36.3.3.1.2" => (Some(Md::Ripemd160), Rsa),
        "1.2.156.10197.1.504" => (Some(Md::Sm3), Rsa),
        "2.5.8.3.100" => (Some(Md::Mdc2), Rsa),
        "1.2.840.113549.1.1.10" => (None, RsaPss),
        "1.2.840.10040.4.3" => (Some(Md::Sha1), Dsa),
        "2.16.840.1.101.3.4.3.1" => (Some(Md::Sha224), Dsa),
        "2.16.840.1.101.3.4.3.2" => (Some(Md::Sha256), Dsa),
        "2.16.840.1.101.3.4.3.3" => (Some(Md::Sha384), Dsa),
        "2.16.840.1.101.3.4.3.4" => (Some(Md::Sha512), Dsa),
        "1.2.840.10045.4.1" => (Some(Md::Sha1), Ecdsa),
        "1.2.840.10045.4.3.1" => (Some(Md::Sha224), Ecdsa),
        "1.2.840.10045.4.3.2" => (Some(Md::Sha256), Ecdsa),
        "1.2.840.10045.4.3.3" => (Some(Md::Sha384), Ecdsa),
        "1.2.840.10045.4.3.4" => (Some(Md::Sha512), Ecdsa),
        "2.16.840.1.101.3.4.3.9" => (Some(Md::Sha3_224), Ecdsa),
        "2.16.840.1.101.3.4.3.10" => (Some(Md::Sha3_256), Ecdsa),
        "2.16.840.1.101.3.4.3.11" => (Some(Md::Sha3_384), Ecdsa),
        "2.16.840.1.101.3.4.3.12" => (Some(Md::Sha3_512), Ecdsa),
        "1.2.156.10197.1.501" => (Some(Md::Sm3), Sm2),
        "1.3.101.112" => (None, Ed25519),
        "1.3.101.113" => (None, Ed448),
        _ => return None,
    })
}

/// Signature AlgorithmIdentifier for signing with `key` and `md`.
pub fn sig_alg_for(key: &PKey, md: Option<Md>) -> Option<Vec<u8>> {
    let oid = match key {
        PKey::Rsa(_) => match md? {
            Md::Md4 => "1.2.840.113549.1.1.3",
            Md::Md5 => "1.2.840.113549.1.1.4",
            Md::Sha1 => "1.2.840.113549.1.1.5",
            Md::Sha224 => "1.2.840.113549.1.1.14",
            Md::Sha256 => "1.2.840.113549.1.1.11",
            Md::Sha384 => "1.2.840.113549.1.1.12",
            Md::Sha512 => "1.2.840.113549.1.1.13",
            Md::Sha512_224 => "1.2.840.113549.1.1.15",
            Md::Sha512_256 => "1.2.840.113549.1.1.16",
            Md::Sha3_224 => "2.16.840.1.101.3.4.3.13",
            Md::Sha3_256 => "2.16.840.1.101.3.4.3.14",
            Md::Sha3_384 => "2.16.840.1.101.3.4.3.15",
            Md::Sha3_512 => "2.16.840.1.101.3.4.3.16",
            Md::Ripemd160 => "1.3.36.3.3.1.2",
            Md::Sm3 => "1.2.156.10197.1.504",
            Md::Mdc2 => "2.5.8.3.100",
            _ => return None,
        },
        PKey::Dsa(_) => match md? {
            Md::Sha1 => "1.2.840.10040.4.3",
            Md::Sha224 => "2.16.840.1.101.3.4.3.1",
            Md::Sha256 => "2.16.840.1.101.3.4.3.2",
            Md::Sha384 => "2.16.840.1.101.3.4.3.3",
            Md::Sha512 => "2.16.840.1.101.3.4.3.4",
            _ => return None,
        },
        PKey::Ec(e) => {
            if e.sm2 {
                "1.2.156.10197.1.501"
            } else {
                match md? {
                    Md::Sha1 => "1.2.840.10045.4.1",
                    Md::Sha224 => "1.2.840.10045.4.3.1",
                    Md::Sha256 => "1.2.840.10045.4.3.2",
                    Md::Sha384 => "1.2.840.10045.4.3.3",
                    Md::Sha512 => "1.2.840.10045.4.3.4",
                    Md::Sha3_224 => "2.16.840.1.101.3.4.3.9",
                    Md::Sha3_256 => "2.16.840.1.101.3.4.3.10",
                    Md::Sha3_384 => "2.16.840.1.101.3.4.3.11",
                    Md::Sha3_512 => "2.16.840.1.101.3.4.3.12",
                    _ => return None,
                }
            }
        }
        PKey::Ecx(x) => {
            if md.is_some() {
                return None;
            }
            match x.kind {
                crate::ecx::EcxKind::Ed25519 => "1.3.101.112",
                crate::ecx::EcxKind::Ed448 => "1.3.101.113",
                _ => return None,
            }
        }
        PKey::Dh(_) => return None,
    };
    let params = if matches!(key, PKey::Rsa(_)) {
        Some(asn1::null())
    } else {
        None
    };
    Some(asn1::algid(oid, params))
}

/// Sign a TBS structure: returns (algid, signature bits).
pub fn sign_tbs(tbs: &[u8], key: &PKey, md: Option<Md>) -> Option<(Vec<u8>, Vec<u8>)> {
    let alg = sig_alg_for(key, md)?;
    let sig = key.sign(md, tbs, false).ok()?;
    Some((alg, sig))
}

pub fn verify_signed(tbs: &[u8], sig_alg: &[u8], sig: &[u8], key: &PKey) -> i64 {
    let Some(oid) = asn1::parse_one(sig_alg)
        .and_then(|t| t.children())
        .and_then(|c| c.first().and_then(|o| o.oid()))
    else {
        return -1;
    };
    let Some((md, kind)) = sig_alg_info(&oid) else {
        crate::err::push(crate::err::LIB_ASN1, 199, "unknown signature algorithm");
        return -1;
    };
    let ok = match (kind, key) {
        (SigKind::Rsa, PKey::Rsa(_)) => key.verify(md, tbs, sig, false),
        (SigKind::RsaPss, PKey::Rsa(_)) => {
            // hash from the PSS parameters (default sha1)
            let md = pss_md(sig_alg).unwrap_or(Md::Sha1);
            key.verify(Some(md), tbs, sig, true)
        }
        (SigKind::Dsa, PKey::Dsa(_))
        | (SigKind::Ecdsa, PKey::Ec(_))
        | (SigKind::Sm2, PKey::Ec(_)) => key.verify(md, tbs, sig, false),
        (SigKind::Ed25519, PKey::Ecx(_)) | (SigKind::Ed448, PKey::Ecx(_)) => {
            key.verify(None, tbs, sig, false)
        }
        _ => {
            crate::err::push(crate::err::LIB_ASN1, 200, "wrong public key type");
            return -1;
        }
    };
    if ok == 1 { 1 } else { 0 }
}

fn pss_md(sig_alg: &[u8]) -> Option<Md> {
    let t = asn1::parse_one(sig_alg)?;
    let c = t.children()?;
    let params = c.get(1)?.children()?;
    for p in params {
        if p.context() == Some(0) {
            let alg = asn1::parse_one(p.content)?;
            let oid = alg.children()?.first()?.oid()?;
            return Md::by_name(&oid);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Time helpers
// ---------------------------------------------------------------------------

pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// X509_gmtime_adj(): UTCTime for 1950..2049, GeneralizedTime otherwise.
pub fn asn1_time_from_unix(t: i64) -> (u8, Vec<u8>) {
    let days = t.div_euclid(86400);
    let secs = t.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if (1950..2050).contains(&y) {
        (
            asn1::UTC_TIME,
            format!("{:02}{:02}{:02}{:02}{:02}{:02}Z", y % 100, m, d, hh, mm, ss).into_bytes(),
        )
    } else {
        (
            asn1::GENERALIZED_TIME,
            format!("{:04}{:02}{:02}{:02}{:02}{:02}Z", y, m, d, hh, mm, ss).into_bytes(),
        )
    }
}

/// ASN1_TIME to unix time (strict parsing as ASN1_TIME_to_tm), None if invalid.
pub fn asn1_time_to_unix(tag: u8, v: &[u8]) -> Option<i64> {
    let s = std::str::from_utf8(v).ok()?;
    let digits = |r: std::ops::Range<usize>| -> Option<i64> {
        let p = s.get(r)?;
        if !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        p.parse().ok()
    };
    let (y, rest) = if tag == asn1::UTC_TIME {
        let yy = digits(0..2)?;
        (if yy < 50 { 2000 + yy } else { 1900 + yy }, 2)
    } else {
        (digits(0..4)?, 4)
    };
    let mo = digits(rest..rest + 2)?;
    let d = digits(rest + 2..rest + 4)?;
    let h = digits(rest + 4..rest + 6)?;
    let mi = digits(rest + 6..rest + 8)?;
    let mut idx = rest + 8;
    let sec = if s.len() >= idx + 2 && s.as_bytes()[idx].is_ascii_digit() {
        idx += 2;
        digits(idx - 2..idx)?
    } else {
        0
    };
    let mut off = 0;
    match s.as_bytes().get(idx) {
        Some(b'Z') => {}
        Some(b'+') | Some(b'-') => {
            let sign = if s.as_bytes()[idx] == b'+' { 1 } else { -1 };
            let oh = digits(idx + 1..idx + 3)?;
            let om = digits(idx + 3..idx + 5)?;
            off = sign * (oh * 3600 + om * 60);
        }
        _ => return None,
    }
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 59 {
        return None;
    }
    Some(days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + sec - off)
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
