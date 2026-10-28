//! PKCS #10 certification requests (RFC 2986) and the certificate
//! issuing behind openssl_csr_new() / openssl_csr_sign().
//!
//! How the request's subject and attributes are assembled from the
//! `$distinguished_names` / `$extra_attributes` arrays and the config
//! file (given entries first, then the `*_default` entries of the
//! distinguished_name section for names not given; the attributes
//! section's entries with the extra attributes replacing them) and the
//! string types chosen (a `string_mask` default of `utf8only`, the
//! RFC 5280 fixed types and size bounds) were observed on PHP 8.5.

use crate::der::{self, tag};
use crate::digest::Md;
use crate::key::Key;
use crate::x509::{self, Attr, Cert, Name, X509Error};

#[derive(Debug, Clone)]
pub struct Csr {
    pub der: Vec<u8>,
    pub info: Vec<u8>,
    pub subject: Name,
    pub spki: Vec<u8>,
    /// (oid, values) of the attributes.
    pub attributes: Vec<(String, Vec<Vec<u8>>)>,
    pub sig_alg: String,
    pub sig_alg_params: Option<Vec<u8>>,
    pub signature: Vec<u8>,
}

impl Csr {
    pub fn from_der(d: &[u8]) -> Result<Csr, X509Error> {
        let e = |_| X509Error;
        let t = der::parse(d).map_err(e)?;
        let mut r = t.reader();
        let info = r.expect(tag::SEQUENCE).map_err(e)?;
        let alg = r.expect(tag::SEQUENCE).map_err(e)?;
        let sig = r.next().and_then(|x| x.bit_string()).map_err(e)?;
        let (sig_alg, sig_alg_params) = x509::alg_id(&alg)?;
        let mut ir = info.reader();
        let _ver = ir.expect(tag::INTEGER).map_err(e)?;
        let subject = Name::parse(&ir.next().map_err(e)?)?;
        let spki = ir.expect(tag::SEQUENCE).map_err(e)?.raw.to_vec();
        let mut attributes = Vec::new();
        if let Some(a) = ir.next_if(der::CONTEXT, 0).map_err(e)? {
            let mut ar = a.reader();
            while !ar.is_empty() {
                let at = ar.expect(tag::SEQUENCE).map_err(e)?;
                let mut atr = at.reader();
                let oid = atr.next().and_then(|x| x.oid()).map_err(e)?;
                let set = atr.expect(tag::SET).map_err(e)?;
                let mut vals = Vec::new();
                let mut sr = set.reader();
                while !sr.is_empty() {
                    vals.push(sr.next().map_err(e)?.raw.to_vec());
                }
                attributes.push((oid, vals));
            }
        }
        Ok(Csr {
            der: d.to_vec(),
            info: der::to_der(&info).map_err(e)?,
            subject,
            spki,
            attributes,
            sig_alg,
            sig_alg_params,
            signature: sig,
        })
    }

    pub fn public_key(&self) -> Result<Key, crate::key::KeyError> {
        Key::from_spki_der(&self.spki)
    }

    pub fn to_pem(&self) -> String {
        crate::pem::encode("CERTIFICATE REQUEST", &self.der)
    }

    pub fn verify_signature(&self) -> Option<bool> {
        let k = self.public_key().ok()?;
        x509::verify_sig(
            &self.sig_alg,
            self.sig_alg_params.as_deref(),
            &self.info,
            &self.signature,
            &k,
        )
    }

    /// The extensionRequest attribute's extensions.
    pub fn requested_extensions(&self) -> Vec<x509::Extension> {
        for (oid, vals) in &self.attributes {
            if oid == crate::oid::ids::EXT_REQ
                && let Some(v) = vals.first()
                && let Ok(t) = der::parse(v)
                && let Ok(exts) = x509::parse_extensions(&t)
            {
                return exts;
            }
        }
        Vec::new()
    }
}

/// Signs `tbs` with `key`: the AlgorithmIdentifier and signature bytes.
pub fn sign_tbs(key: &Key, md: Option<Md>, tbs: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let alg = x509::sig_alg_for(key, md)?;
    let sig = key.sign(md, tbs, false).ok()?;
    Some((alg, sig))
}

/// Builds a signed request.
pub fn build(
    subject: &Name,
    key: &Key,
    attrs: &[(String, Vec<Vec<u8>>)],
    md: Option<Md>,
) -> Option<Csr> {
    let attr_ders: Vec<Vec<u8>> = attrs
        .iter()
        .map(|(o, vals)| der::seq_vec(&[der::oid(o), der::set_of(vals)]))
        .collect();
    let info = der::seq_vec(&[
        der::int(0),
        subject.der.clone(),
        key.to_spki_der(),
        der::implicit_constructed(0, &sorted_concat(&attr_ders)),
    ]);
    let (alg, sig) = sign_tbs(key, md, &info)?;
    let d = der::seq_vec(&[info, alg, der::bit_string(&sig)]);
    Csr::from_der(&d).ok()
}

fn sorted_concat(v: &[Vec<u8>]) -> Vec<u8> {
    let mut p: Vec<&Vec<u8>> = v.iter().collect();
    p.sort();
    p.into_iter().flatten().copied().collect()
}

/// Issues a certificate.
#[allow(clippy::too_many_arguments)]
pub fn issue(
    subject: &Name,
    issuer: &Name,
    spki: &[u8],
    serial: &[u8],
    not_before: i64,
    not_after: i64,
    extensions: &[x509::Extension],
    key: &Key,
    md: Option<Md>,
) -> Option<Cert> {
    let alg = x509::sig_alg_for(key, md)?;
    let mut parts = vec![
        der::explicit(0, &der::int(2)),
        der::int_raw(serial),
        alg.clone(),
        issuer.der.clone(),
        der::seq_vec(&[der::time(not_before), der::time(not_after)]),
        subject.der.clone(),
        spki.to_vec(),
    ];
    if !extensions.is_empty() {
        let exts: Vec<Vec<u8>> = extensions
            .iter()
            .map(|e| {
                if e.critical {
                    der::seq_vec(&[
                        der::oid(&e.oid),
                        der::boolean(true),
                        der::octet_string(&e.value),
                    ])
                } else {
                    der::seq_vec(&[der::oid(&e.oid), der::octet_string(&e.value)])
                }
            })
            .collect();
        parts.push(der::explicit(3, &der::seq_vec(&exts)));
    }
    let tbs = der::seq_vec(&parts);
    let sig = key.sign(md, &tbs, false).ok()?;
    let d = der::seq_vec(&[tbs, alg, der::bit_string(&sig)]);
    Cert::from_der(&d).ok()
}

// ------------------------------------------------------------ string types

/// The ASN.1 string types a `string_mask` allows (OpenSSL documentation:
/// req(1) `string_mask`: `default`, `pkix`, `nombstr`, `utf8only`, or
/// `MASK:<number>` of the B_ASN1_* bits).
#[derive(Debug, Clone, Copy)]
pub struct StringMask(pub u64);

const B_PRINTABLE: u64 = 0x0002;
const B_T61: u64 = 0x0004;
const B_IA5: u64 = 0x0010;
const B_UNIVERSAL: u64 = 0x0100;
const B_BMP: u64 = 0x0800;
const B_UTF8: u64 = 0x2000;

impl StringMask {
    pub fn parse(s: Option<&str>) -> StringMask {
        let s = s.unwrap_or("utf8only");
        if let Some(n) = s.strip_prefix("MASK:") {
            let n = n.trim();
            let v = if let Some(h) = n.strip_prefix("0x") {
                u64::from_str_radix(h, 16).unwrap_or(0)
            } else {
                n.parse().unwrap_or(0)
            };
            return StringMask(v);
        }
        StringMask(match s {
            "default" => 0xffff_ffff & !0x10000,
            "pkix" => 0xffff_ffff & !(B_T61 | 0x10000),
            "nombstr" => 0xffff_ffff & !(B_BMP | B_UTF8 | B_UNIVERSAL | 0x10000),
            _ => B_UTF8,
        })
    }
}

/// RFC 5280 appendix A: the fixed type and size bounds of some
/// attributes ((oid, tag, min, max)).
fn fixed_rule(oid: &str) -> Option<(Option<u32>, usize, usize)> {
    Some(match oid {
        "2.5.4.6" => (Some(tag::PRINTABLE_STRING), 2, 2),
        "2.5.4.5" | "2.5.4.46" => (Some(tag::PRINTABLE_STRING), 1, 64),
        "1.2.840.113549.1.9.1" | "0.9.2342.19200300.100.1.25" => (Some(tag::IA5_STRING), 1, 128),
        "2.5.4.3" | "2.5.4.10" | "2.5.4.11" | "2.5.4.12" => (None, 1, 64),
        "2.5.4.7" | "2.5.4.8" => (None, 1, 128),
        "2.5.4.41" => (None, 1, 32768),
        "2.5.4.4" | "2.5.4.42" => (None, 1, 32768),
        "2.5.4.43" => (None, 1, 32768),
        "2.5.4.44" => (None, 1, 32768),
        "2.5.4.65" => (None, 1, 128),
        "1.2.840.113549.1.9.2" | "1.2.840.113549.1.9.7" => (None, 1, 255),
        _ => return None,
    })
}

fn is_printable(c: char) -> bool {
    c.is_ascii_alphanumeric() || " '()+,-./:=?".contains(c)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrError {
    TooShort,
    TooLong,
    IllegalChars,
}

/// Encodes a UTF-8 value as the ASN.1 string type the mask and the
/// attribute's rules choose: (tag, content).
pub fn encode_string(
    oid: &str,
    value: &[u8],
    mask: StringMask,
) -> Result<(u32, Vec<u8>), StrError> {
    let s = String::from_utf8_lossy(value).into_owned();
    let chars: Vec<char> = s.chars().collect();
    let (fixed, min, max) = fixed_rule(oid).unwrap_or((None, 0, usize::MAX));
    if chars.len() < min {
        return Err(StrError::TooShort);
    }
    if chars.len() > max {
        return Err(StrError::TooLong);
    }
    let allow = match fixed {
        Some(tag::PRINTABLE_STRING) => B_PRINTABLE,
        Some(tag::IA5_STRING) => B_IA5,
        _ => mask.0,
    };
    let all_printable = chars.iter().all(|&c| is_printable(c));
    let max_cp = chars.iter().map(|&c| c as u32).max().unwrap_or(0);
    if allow & B_PRINTABLE != 0 && all_printable {
        return Ok((tag::PRINTABLE_STRING, s.into_bytes()));
    }
    if allow & B_T61 != 0 && max_cp < 0x100 {
        return Ok((tag::T61_STRING, chars.iter().map(|&c| c as u8).collect()));
    }
    if allow & B_IA5 != 0 && max_cp < 0x80 {
        return Ok((tag::IA5_STRING, s.into_bytes()));
    }
    if allow & B_BMP != 0 && max_cp < 0x10000 {
        let mut v = Vec::new();
        for c in &chars {
            v.extend_from_slice(&(*c as u16).to_be_bytes());
        }
        return Ok((tag::BMP_STRING, v));
    }
    if allow & B_UNIVERSAL != 0 && allow & B_UTF8 == 0 {
        let mut v = Vec::new();
        for c in &chars {
            v.extend_from_slice(&(*c as u32).to_be_bytes());
        }
        return Ok((tag::UNIVERSAL_STRING, v));
    }
    if allow & B_UTF8 != 0 {
        return Ok((tag::UTF8_STRING, s.into_bytes()));
    }
    Err(StrError::IllegalChars)
}

/// A name attribute from text.
pub fn make_attr(oid: &str, value: &[u8], mask: StringMask) -> Result<Attr, StrError> {
    let (tag, v) = encode_string(oid, value, mask)?;
    Ok(Attr {
        oid: oid.to_string(),
        tag,
        value: v,
    })
}

/// A random positive serial would be OpenSSL's choice for `openssl x509`;
/// PHP uses the given integer.
pub fn serial_from_int(n: i64) -> Vec<u8> {
    let d = der::int(n);
    d[2..].to_vec()
}

/// Parses `serial_hex` (hex digits only; observed on PHP 8.5: "Error
/// parsing serial number", and "... because it is too long" past 20
/// bytes... observed as rejecting 500 bytes).
pub fn serial_from_hex(s: &str) -> Result<Vec<u8>, bool> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(false);
    }
    if s.len() > 2 * 128 {
        return Err(true);
    }
    let padded = if s.len() % 2 == 1 {
        format!("0{}", s)
    } else {
        s.to_string()
    };
    let b = crate::pbe::hex_decode(&padded).ok_or(false)?;
    let b = crate::key::strip(&b);
    let mut c = Vec::new();
    if b[0] & 0x80 != 0 {
        c.push(0);
    }
    c.extend_from_slice(&b);
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strings() {
        let m = StringMask(0xffff_ffff);
        assert_eq!(
            encode_string("2.5.4.10", b"plain", m).unwrap().0,
            tag::PRINTABLE_STRING
        );
        assert_eq!(
            encode_string("2.5.4.10", b"a@b", m).unwrap().0,
            tag::T61_STRING
        );
        assert_eq!(
            encode_string("2.5.4.3", "日本".as_bytes(), m).unwrap().0,
            tag::BMP_STRING
        );
        let u = StringMask::parse(None);
        assert_eq!(
            encode_string("2.5.4.10", b"plain", u).unwrap().0,
            tag::UTF8_STRING
        );
        assert_eq!(
            encode_string("2.5.4.6", b"FR", u).unwrap().0,
            tag::PRINTABLE_STRING
        );
        assert_eq!(encode_string("2.5.4.6", b"x", u), Err(StrError::TooShort));
    }
}
