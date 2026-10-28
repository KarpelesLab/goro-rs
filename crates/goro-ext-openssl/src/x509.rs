//! X.509 certificates (RFC 5280): parsing, the PHP array and text forms,
//! names, extensions, purposes, signatures.
//!
//! The text renderings of names and extensions follow what PHP 8.5 prints
//! for them (observed on PHP 8.5 with OpenSSL 3.5: openssl_x509_parse()
//! on certificates carrying each extension, and the expected output of
//! ext/openssl/tests/*.phpt).

use crate::der::{self, Reader, Tlv, tag};
use crate::digest::Md;
use crate::key::{Key, KeyError};
use crate::oid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct X509Error;

type XResult<T> = Result<T, X509Error>;

fn de<T>(_: T) -> X509Error {
    X509Error
}

// ------------------------------------------------------------------ names

/// One AttributeTypeAndValue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    pub oid: String,
    /// The ASN.1 string type's universal tag number.
    pub tag: u32,
    pub value: Vec<u8>,
}

impl Attr {
    pub fn sn(&self) -> String {
        oid::sn(&self.oid)
    }
    pub fn ln(&self) -> String {
        oid::ln(&self.oid)
    }
    /// The value as UTF-8 (ASN1_STRING_to_UTF8 semantics: BMPString is
    /// UTF-16BE, UniversalString UCS-4, T61String/others byte-per-char
    /// Latin-1 except UTF8String which is kept).
    pub fn utf8(&self) -> Vec<u8> {
        to_utf8(self.tag, &self.value)
    }
}

pub fn to_utf8(t: u32, v: &[u8]) -> Vec<u8> {
    match t {
        tag::UTF8_STRING => v.to_vec(),
        tag::BMP_STRING => {
            let u: Vec<u16> = v
                .chunks(2)
                .map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)]))
                .collect();
            String::from_utf16_lossy(&u).into_bytes()
        }
        tag::UNIVERSAL_STRING => {
            let mut s = String::new();
            for c in v.chunks(4) {
                if c.len() == 4 {
                    let n = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
                    s.push(char::from_u32(n).unwrap_or('\u{fffd}'));
                }
            }
            s.into_bytes()
        }
        tag::T61_STRING
        | tag::PRINTABLE_STRING
        | tag::IA5_STRING
        | tag::VISIBLE_STRING
        | tag::NUMERIC_STRING => {
            if v.is_ascii() {
                v.to_vec()
            } else {
                v.iter()
                    .map(|&b| b as char)
                    .collect::<String>()
                    .into_bytes()
            }
        }
        _ => v.to_vec(),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Name {
    pub der: Vec<u8>,
    /// RDNs in order, each a set of attributes.
    pub rdns: Vec<Vec<Attr>>,
}

impl Name {
    pub fn parse(t: &Tlv) -> XResult<Name> {
        if !t.is_universal(tag::SEQUENCE) {
            return Err(X509Error);
        }
        let mut rdns = Vec::new();
        let mut r = t.reader();
        while !r.is_empty() {
            let set = r.expect(tag::SET).map_err(de)?;
            let mut sr = set.reader();
            let mut rdn = Vec::new();
            while !sr.is_empty() {
                let atv = sr.expect(tag::SEQUENCE).map_err(de)?;
                let mut ar = atv.reader();
                let oid = ar.next().and_then(|x| x.oid()).map_err(de)?;
                let v = ar.next().map_err(de)?;
                rdn.push(Attr {
                    oid,
                    tag: if v.class == der::UNIVERSAL { v.num } else { 0 },
                    value: v.string_bytes().map_err(de)?,
                });
            }
            rdns.push(rdn);
        }
        Ok(Name {
            der: der::to_der(t).map_err(de)?,
            rdns,
        })
    }

    pub fn from_der(d: &[u8]) -> XResult<Name> {
        Name::parse(&der::parse(d).map_err(de)?)
    }

    /// A name from attributes, one per RDN.
    pub fn from_attrs(attrs: Vec<Attr>) -> Name {
        let rdns: Vec<Vec<Attr>> = attrs.into_iter().map(|a| vec![a]).collect();
        let mut n = Name {
            der: Vec::new(),
            rdns,
        };
        n.der = n.encode();
        n
    }

    pub fn encode(&self) -> Vec<u8> {
        let sets: Vec<Vec<u8>> = self
            .rdns
            .iter()
            .map(|rdn| {
                let atvs: Vec<Vec<u8>> = rdn
                    .iter()
                    .map(|a| der::seq_vec(&[der::oid(&a.oid), der::tlv(a.tag as u8, &a.value)]))
                    .collect();
                der::set_of(&atvs)
            })
            .collect();
        der::seq_vec(&sets)
    }

    pub fn entries(&self) -> impl Iterator<Item = &Attr> {
        self.rdns.iter().flatten()
    }

    pub fn is_empty(&self) -> bool {
        self.rdns.is_empty()
    }

    /// The `/C=BR/ST=...` form (X509_NAME_oneline; observed on PHP 8.5:
    /// bytes outside printable ASCII as `\xHH`, `/` as `\/`).
    pub fn oneline(&self) -> String {
        let mut s = String::new();
        for a in self.entries() {
            s.push('/');
            s.push_str(&a.sn());
            s.push('=');
            let v = oneline_bytes(a);
            for &b in &v {
                if b == b'/' {
                    s.push_str("\\/");
                } else if !(b' '..=b'~').contains(&b) {
                    s.push_str(&format!("\\x{:02X}", b));
                } else {
                    s.push(b as char);
                }
            }
        }
        s
    }

    /// The comma-separated form (`C=FR, ST=..., CN=x/emailAddress=y`) the
    /// certificate text uses: the oneline form split before each
    /// `/X=` or `/XY=` whose letters are upper case (observed on PHP 8.5).
    pub fn print_compat(&self) -> String {
        let one = self.oneline();
        let b = one.as_bytes();
        let mut out = String::new();
        let mut i = 1;
        let mut start = 1;
        while i < b.len() {
            if b[i] == b'/'
                && b[i - 1] != b'\\'
                && i + 2 < b.len()
                && b[i + 1].is_ascii_uppercase()
                && (b[i + 2] == b'='
                    || (i + 3 < b.len() && b[i + 2].is_ascii_uppercase() && b[i + 3] == b'='))
            {
                out.push_str(&one[start..i]);
                out.push_str(", ");
                start = i + 1;
            }
            i += 1;
        }
        if start <= one.len() {
            out.push_str(&one[start.min(one.len())..]);
        }
        out
    }

    /// The `C = UK, O = My Organization` form (RFC 2253-style escaping
    /// with spaces around `=`; observed on PHP 8.5 in DirName entries).
    pub fn print_oneline_spaced(&self) -> String {
        let mut parts = Vec::new();
        for rdn in &self.rdns {
            let mut sub = Vec::new();
            for a in rdn {
                sub.push(format!("{} = {}", a.sn(), escape_2253(&a.utf8())));
            }
            parts.push(sub.join(" + "));
        }
        parts.join(", ")
    }

    /// X509_NAME_hash: the first four bytes, little-endian, of SHA-1 over
    /// the canonical encoding (each value as a lower-cased UTF8String
    /// with whitespace runs collapsed and trimmed, the RDN SETs
    /// concatenated without the outer SEQUENCE; OpenSSL documentation of
    /// `openssl rehash` / `-subject_hash`).
    pub fn hash(&self) -> u32 {
        let canon = self.canonical();
        let h = Md::H(purecrypto::hash::HashAlgorithm::Sha1).digest(&canon);
        u32::from_le_bytes([h[0], h[1], h[2], h[3]])
    }

    /// The old MD5-based hash (`-subject_hash_old`).
    pub fn hash_old(&self) -> u32 {
        let h = Md::H(purecrypto::hash::HashAlgorithm::Md5).digest(&self.der);
        u32::from_le_bytes([h[0], h[1], h[2], h[3]])
    }

    pub fn canonical(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for rdn in &self.rdns {
            let atvs: Vec<Vec<u8>> = rdn
                .iter()
                .map(|a| {
                    let v = if is_string_tag(a.tag) {
                        der::tlv(0x0c, &canon_value(&a.utf8()))
                    } else {
                        der::tlv(a.tag as u8, &a.value)
                    };
                    der::seq_vec(&[der::oid(&a.oid), v])
                })
                .collect();
            out.extend_from_slice(&der::set_of(&atvs));
        }
        out
    }

    /// Case-insensitive comparison by canonical form (X509_NAME_cmp is
    /// over the canonical encoding).
    pub fn matches(&self, other: &Name) -> bool {
        self.canonical() == other.canonical()
    }

    pub fn find(&self, oid: &str) -> Vec<&Attr> {
        self.entries().filter(|a| a.oid == oid).collect()
    }
}

fn is_string_tag(t: u32) -> bool {
    matches!(
        t,
        tag::UTF8_STRING
            | tag::PRINTABLE_STRING
            | tag::T61_STRING
            | tag::IA5_STRING
            | tag::VISIBLE_STRING
            | tag::BMP_STRING
            | tag::UNIVERSAL_STRING
            | tag::NUMERIC_STRING
    )
}

fn canon_value(v: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len());
    let mut space = false;
    for &b in v {
        if b.is_ascii_whitespace() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(b' ');
        }
        space = false;
        out.push(b.to_ascii_lowercase());
    }
    out
}

fn oneline_bytes(a: &Attr) -> Vec<u8> {
    // BMPString / UniversalString whose high bytes are all zero are shown
    // by their low bytes.
    match a.tag {
        tag::BMP_STRING if a.value.len() % 2 == 0 => {
            if a.value.chunks(2).all(|c| c[0] == 0) {
                a.value.chunks(2).map(|c| c[1]).collect()
            } else {
                a.value.clone()
            }
        }
        tag::UNIVERSAL_STRING if a.value.len() % 4 == 0 => {
            if a.value
                .chunks(4)
                .all(|c| c[0] == 0 && c[1] == 0 && c[2] == 0)
            {
                a.value.chunks(4).map(|c| c[3]).collect()
            } else {
                a.value.clone()
            }
        }
        _ => a.value.clone(),
    }
}

fn escape_2253(v: &[u8]) -> String {
    let mut s = String::new();
    let n = v.len();
    for (i, &b) in v.iter().enumerate() {
        let special = matches!(b, b',' | b'+' | b'"' | b'\\' | b'<' | b'>' | b';')
            || (i == 0 && (b == b'#' || b == b' '))
            || (i + 1 == n && b == b' ');
        if special {
            s.push('\\');
            s.push(b as char);
        } else if b < 0x20 || b == 0x7f {
            s.push_str(&format!("\\{:02X}", b));
        } else if b >= 0x80 {
            s.push_str(&format!("\\{:02X}", b));
        } else {
            s.push(b as char);
        }
    }
    s
}

// ------------------------------------------------------------------ certs

#[derive(Debug, Clone)]
pub struct Extension {
    pub oid: String,
    pub critical: bool,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Time {
    pub raw: String,
    pub generalized: bool,
    pub unix: i64,
}

impl Time {
    fn parse(t: &Tlv) -> XResult<Time> {
        let generalized = t.is_universal(tag::GENERALIZED_TIME);
        if !generalized && !t.is_universal(tag::UTC_TIME) {
            return Err(X509Error);
        }
        let raw = String::from_utf8_lossy(t.content).into_owned();
        // OpenSSL 3.3+ rejects UTCTime without seconds (test
        // gh13343_openssl33.phpt); a malformed GeneralizedTime still
        // decodes and shows as -1 (test cve-2013-6420.phpt).
        if !generalized {
            if t.content.len() != 13 {
                return Err(X509Error);
            }
            let unix = crate::time::parse_asn1_time(t.content, false).ok_or(X509Error)?;
            return Ok(Time {
                raw,
                generalized,
                unix,
            });
        }
        let unix = crate::time::parse_asn1_time(t.content, true).unwrap_or(-1);
        Ok(Time {
            raw,
            generalized,
            unix,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Cert {
    pub der: Vec<u8>,
    pub tbs: Vec<u8>,
    /// 0 for v1, 2 for v3.
    pub version: i64,
    /// The serial's two's-complement content octets.
    pub serial: Vec<u8>,
    pub sig_alg: String,
    pub sig_alg_params: Option<Vec<u8>>,
    pub issuer: Name,
    pub subject: Name,
    pub not_before: Time,
    pub not_after: Time,
    pub spki: Vec<u8>,
    pub extensions: Vec<Extension>,
    pub signature: Vec<u8>,
}

impl Cert {
    pub fn from_der(d: &[u8]) -> XResult<Cert> {
        let t = der::parse(d).map_err(de)?;
        if !t.is_universal(tag::SEQUENCE) {
            return Err(X509Error);
        }
        let mut r = t.reader();
        let tbs = r.expect(tag::SEQUENCE).map_err(de)?;
        let alg = r.expect(tag::SEQUENCE).map_err(de)?;
        let sig = r.next().and_then(|x| x.bit_string()).map_err(de)?;
        let (sig_alg, sig_alg_params) = alg_id(&alg)?;
        let mut tr = tbs.reader();
        let mut version = 0;
        if let Some(v) = tr.next_if(der::CONTEXT, 0).map_err(de)? {
            version = der::parse(v.content)
                .and_then(|x| x.small_int())
                .map_err(de)?;
        }
        let serial = tr.next().and_then(|x| x.integer()).map_err(de)?.to_vec();
        let _inner_alg = tr.expect(tag::SEQUENCE).map_err(de)?;
        let issuer = Name::parse(&tr.next().map_err(de)?)?;
        let validity = tr.expect(tag::SEQUENCE).map_err(de)?;
        let mut vr = validity.reader();
        let not_before = Time::parse(&vr.next().map_err(de)?)?;
        let not_after = Time::parse(&vr.next().map_err(de)?)?;
        let subject = Name::parse(&tr.next().map_err(de)?)?;
        let spki = tr.expect(tag::SEQUENCE).map_err(de)?.raw.to_vec();
        let mut extensions = Vec::new();
        while !tr.is_empty() {
            let x = tr.next().map_err(de)?;
            if x.is_context(3) {
                extensions = parse_extensions(&der::parse(x.content).map_err(de)?)?;
            }
        }
        Ok(Cert {
            der: d.to_vec(),
            tbs: der::to_der(&tbs).map_err(de)?,
            version,
            serial,
            sig_alg,
            sig_alg_params,
            issuer,
            subject,
            not_before,
            not_after,
            spki,
            extensions,
            signature: sig,
        })
    }

    pub fn ext(&self, oid: &str) -> Option<&Extension> {
        self.extensions.iter().find(|e| e.oid == oid)
    }

    pub fn public_key(&self) -> Result<Key, KeyError> {
        Key::from_spki_der(&self.spki)
    }

    pub fn to_pem(&self) -> String {
        crate::pem::encode("CERTIFICATE", &self.der)
    }

    /// Decimal up to 128 bits, `0x`-prefixed hex beyond (observed on PHP
    /// 8.5; test openssl_csr_sign_with_serial_hex.phpt).
    pub fn serial_decimal(&self) -> String {
        let neg = self.serial.first().is_some_and(|b| b & 0x80 != 0);
        // 128-bit serials still print in decimal (test bug39217.phpt).
        if !neg && crate::key::strip(&self.serial).len() > 16 {
            return format!("0x{}", signed_hex(&self.serial));
        }
        signed_decimal(&self.serial)
    }

    /// Upper-case hex of the serial (two's complement magnitude for
    /// negatives with a leading `-`).
    pub fn serial_hex(&self) -> String {
        signed_hex(&self.serial)
    }

    pub fn fingerprint(&self, md: Md) -> Vec<u8> {
        md.digest(&self.der)
    }

    /// Whether `key` signed this certificate.
    pub fn verify_signature(&self, key: &Key) -> Option<bool> {
        verify_sig(
            &self.sig_alg,
            self.sig_alg_params.as_deref(),
            &self.tbs,
            &self.signature,
            key,
        )
    }

    pub fn is_self_issued(&self) -> bool {
        self.issuer.matches(&self.subject)
    }

    // --- extension accessors

    /// basicConstraints: (ca, pathlen).
    pub fn basic_constraints(&self) -> Option<(bool, Option<i64>)> {
        let e = self.ext("2.5.29.19")?;
        let t = der::parse(&e.value).ok()?;
        let mut r = t.reader();
        let mut ca = false;
        let mut pl = None;
        while !r.is_empty() {
            let x = r.next().ok()?;
            if x.is_universal(tag::BOOLEAN) {
                ca = x.boolean().ok()?;
            } else if x.is_universal(tag::INTEGER) {
                pl = x.small_int().ok();
            }
        }
        Some((ca, pl))
    }

    /// keyUsage bits (bit 0 = digitalSignature, as RFC 5280 numbers them).
    pub fn key_usage(&self) -> Option<u16> {
        let e = self.ext("2.5.29.15")?;
        let t = der::parse(&e.value).ok()?;
        let b = t.bit_string().ok()?;
        Some(bits_to_u16(&b))
    }

    pub fn ext_key_usage(&self) -> Option<Vec<String>> {
        let e = self.ext("2.5.29.37")?;
        let t = der::parse(&e.value).ok()?;
        let mut r = t.reader();
        let mut v = Vec::new();
        while !r.is_empty() {
            v.push(r.next().ok()?.oid().ok()?);
        }
        Some(v)
    }

    pub fn ns_cert_type(&self) -> Option<u16> {
        let e = self.ext("2.16.840.1.113730.1.1")?;
        let t = der::parse(&e.value).ok()?;
        let b = t.bit_string().ok()?;
        Some(bits_to_u16(&b))
    }

    pub fn subject_key_id(&self) -> Option<Vec<u8>> {
        let e = self.ext("2.5.29.14")?;
        let t = der::parse(&e.value).ok()?;
        Some(t.content.to_vec())
    }

    pub fn authority_key_id(&self) -> Option<Vec<u8>> {
        let e = self.ext("2.5.29.35")?;
        let t = der::parse(&e.value).ok()?;
        let mut r = t.reader();
        while !r.is_empty() {
            let x = r.next().ok()?;
            if x.is_context(0) {
                return Some(x.content.to_vec());
            }
        }
        None
    }

    /// subjectAltName entries.
    pub fn alt_names(&self) -> Vec<GeneralName> {
        let Some(e) = self.ext("2.5.29.17") else {
            return Vec::new();
        };
        der::parse(&e.value)
            .ok()
            .and_then(|t| parse_general_names(&t).ok())
            .unwrap_or_default()
    }
}

/// RFC 5280 bit order: bit 0 is the most significant bit of the first byte.
fn bits_to_u16(b: &[u8]) -> u16 {
    let mut v = 0u16;
    for i in 0..16 {
        let byte = b.get(i / 8).copied().unwrap_or(0);
        if byte & (0x80 >> (i % 8)) != 0 {
            v |= 1 << i;
        }
    }
    v
}

pub fn alg_id(t: &Tlv) -> XResult<(String, Option<Vec<u8>>)> {
    let mut r = t.reader();
    let o = r.next().and_then(|x| x.oid()).map_err(de)?;
    let p = if r.is_empty() {
        None
    } else {
        Some(r.next().map_err(de)?.raw.to_vec())
    };
    Ok((o, p))
}

pub fn parse_extensions(t: &Tlv) -> XResult<Vec<Extension>> {
    let mut out = Vec::new();
    let mut r = t.reader();
    while !r.is_empty() {
        let e = r.expect(tag::SEQUENCE).map_err(de)?;
        let mut er = e.reader();
        let oid = er.next().and_then(|x| x.oid()).map_err(de)?;
        let mut critical = false;
        let mut x = er.next().map_err(de)?;
        if x.is_universal(tag::BOOLEAN) {
            critical = x.boolean().map_err(de)?;
            x = er.next().map_err(de)?;
        }
        if !x.is_universal(tag::OCTET_STRING) {
            return Err(X509Error);
        }
        out.push(Extension {
            oid,
            critical,
            value: x.string_bytes().map_err(de)?,
        });
    }
    Ok(out)
}

/// Decimal text of a two's-complement integer.
pub fn signed_decimal(c: &[u8]) -> String {
    if c.is_empty() {
        return "0".into();
    }
    let neg = c[0] & 0x80 != 0;
    let mag = if neg { twos_negate(c) } else { c.to_vec() };
    let s = unsigned_decimal(&mag);
    if neg { format!("-{}", s) } else { s }
}

fn twos_negate(c: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = c.iter().map(|b| !b).collect();
    for b in v.iter_mut().rev() {
        let (n, o) = b.overflowing_add(1);
        *b = n;
        if !o {
            break;
        }
    }
    v
}

pub fn unsigned_decimal(be: &[u8]) -> String {
    let mut digits: Vec<u8> = Vec::new();
    let mut num: Vec<u8> = be.to_vec();
    while num.iter().any(|&b| b != 0) {
        let mut rem = 0u32;
        for b in num.iter_mut() {
            let cur = (rem << 8) | *b as u32;
            *b = (cur / 10) as u8;
            rem = cur % 10;
        }
        digits.push(b'0' + rem as u8);
    }
    if digits.is_empty() {
        return "0".into();
    }
    digits.reverse();
    String::from_utf8(digits).unwrap_or_default()
}

pub fn signed_hex(c: &[u8]) -> String {
    if c.is_empty() {
        return "00".into();
    }
    let neg = c[0] & 0x80 != 0;
    let mag = if neg { twos_negate(c) } else { c.to_vec() };
    let m = crate::key::strip(&mag);
    let h: String = m.iter().map(|b| format!("{:02X}", b)).collect();
    if neg { format!("-{}", h) } else { h }
}

/// Colon-separated upper-case hex (`DB:7E:40`).
pub fn colon_hex(b: &[u8]) -> String {
    b.iter()
        .map(|x| format!("{:02X}", x))
        .collect::<Vec<_>>()
        .join(":")
}

// ------------------------------------------------------------ general names

#[derive(Debug, Clone)]
pub enum GeneralName {
    Other(String, Vec<u8>),
    Email(Vec<u8>),
    Dns(Vec<u8>),
    X400,
    Dir(Name),
    Edi,
    Uri(Vec<u8>),
    Ip(Vec<u8>),
    Rid(String),
}

pub fn parse_general_names(t: &Tlv) -> XResult<Vec<GeneralName>> {
    let mut out = Vec::new();
    let mut r = t.reader();
    while !r.is_empty() {
        out.push(parse_general_name(&r.next().map_err(de)?)?);
    }
    Ok(out)
}

pub fn parse_general_name(x: &Tlv) -> XResult<GeneralName> {
    if x.class != der::CONTEXT {
        return Err(X509Error);
    }
    Ok(match x.num {
        0 => {
            let mut r = x.reader();
            let o = r.next().and_then(|y| y.oid()).map_err(de)?;
            let v = r.next().map(|y| y.content.to_vec()).unwrap_or_default();
            GeneralName::Other(o, v)
        }
        1 => GeneralName::Email(x.string_bytes().map_err(de)?),
        2 => GeneralName::Dns(x.string_bytes().map_err(de)?),
        3 => GeneralName::X400,
        4 => GeneralName::Dir(Name::parse(&der::parse(x.content).map_err(de)?)?),
        5 => GeneralName::Edi,
        6 => GeneralName::Uri(x.string_bytes().map_err(de)?),
        7 => GeneralName::Ip(x.content.to_vec()),
        8 => GeneralName::Rid(der::oid_to_string(x.content).map_err(de)?),
        _ => return Err(X509Error),
    })
}

fn ip_text(b: &[u8]) -> String {
    match b.len() {
        4 => format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3]),
        16 => b
            .chunks(2)
            .map(|c| format!("{:X}", u16::from_be_bytes([c[0], c[1]])))
            .collect::<Vec<_>>()
            .join(":"),
        _ => "<invalid>".into(),
    }
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// A GeneralName as PHP prints it in openssl_x509_parse() (`DNS:x`,
/// `IP Address:1.2.3.4`, `DirName:C = UK, ...`, `othername:<unsupported>`).
pub fn general_name_text(g: &GeneralName, in_list: bool) -> String {
    match g {
        GeneralName::Other(..) => "othername:<unsupported>".into(),
        GeneralName::Email(v) => format!("email:{}", lossy(v)),
        GeneralName::Dns(v) => format!("DNS:{}", lossy(v)),
        GeneralName::X400 => "X400Name:<unsupported>".into(),
        GeneralName::Dir(n) => {
            if in_list {
                format!("DirName:{}", n.print_oneline_spaced())
            } else {
                format!("DirName:{}", n.oneline())
            }
        }
        GeneralName::Edi => "EdiPartyName:<unsupported>".into(),
        GeneralName::Uri(v) => format!("URI:{}", lossy(v)),
        GeneralName::Ip(v) => format!("IP Address:{}", ip_text(v)),
        GeneralName::Rid(o) => format!("Registered ID:{}", oid::ln(o)),
    }
}

// ------------------------------------------------------------ extension text

const KEY_USAGE_NAMES: [&str; 9] = [
    "Digital Signature",
    "Non Repudiation",
    "Key Encipherment",
    "Data Encipherment",
    "Key Agreement",
    "Certificate Sign",
    "CRL Sign",
    "Encipher Only",
    "Decipher Only",
];

const NS_CERT_TYPE_NAMES: [&str; 8] = [
    "SSL Client",
    "SSL Server",
    "S/MIME",
    "Object Signing",
    "Unused",
    "SSL CA",
    "S/MIME CA",
    "Object Signing CA",
];

/// The text PHP's openssl_x509_parse() shows for an extension; `None`
/// when the extension can't be rendered (PHP then shows its raw value).
pub fn extension_text(e: &Extension) -> Option<String> {
    let t = der::parse(&e.value).ok()?;
    match e.oid.as_str() {
        "2.5.29.19" => {
            let mut r = t.reader();
            let mut ca = false;
            let mut pl = None;
            while !r.is_empty() {
                let x = r.next().ok()?;
                if x.is_universal(tag::BOOLEAN) {
                    ca = x.boolean().ok()?;
                } else if x.is_universal(tag::INTEGER) {
                    pl = Some(signed_decimal(x.content));
                }
            }
            let mut s = format!("CA:{}", if ca { "TRUE" } else { "FALSE" });
            if let Some(p) = pl {
                s.push_str(&format!(", pathlen:{}", p));
            }
            Some(s)
        }
        "2.5.29.15" => {
            let bits = bits_to_u16(&t.bit_string().ok()?);
            let v: Vec<&str> = (0..9)
                .filter(|i| bits & (1 << i) != 0)
                .map(|i| KEY_USAGE_NAMES[i])
                .collect();
            Some(v.join(", "))
        }
        "2.16.840.1.113730.1.1" => {
            let bits = bits_to_u16(&t.bit_string().ok()?);
            let v: Vec<&str> = (0..8)
                .filter(|i| bits & (1 << i) != 0)
                .map(|i| NS_CERT_TYPE_NAMES[i])
                .collect();
            Some(v.join(", "))
        }
        "2.5.29.37" => {
            let mut r = t.reader();
            let mut v = Vec::new();
            while !r.is_empty() {
                v.push(oid::ln(&r.next().ok()?.oid().ok()?));
            }
            Some(v.join(", "))
        }
        "2.5.29.14" => {
            if !t.is_universal(tag::OCTET_STRING) {
                return None;
            }
            Some(colon_hex(t.content))
        }
        "2.5.29.35" => {
            let mut r = t.reader();
            let mut s = String::new();
            while !r.is_empty() {
                let x = r.next().ok()?;
                match x.num {
                    0 => s.push_str(&format!("keyid:{}\n", colon_hex(x.content))),
                    1 => {
                        let names = parse_general_names(&x).ok()?;
                        for n in names {
                            s.push_str(&general_name_text(&n, false));
                            s.push('\n');
                        }
                    }
                    2 => s.push_str(&format!(
                        "serial:{}\n",
                        colon_hex(&crate::key::strip(x.content))
                    )),
                    _ => return None,
                }
            }
            if s.is_empty() {
                return None;
            }
            // Observed on PHP 8.5: the trailing newline is dropped.
            s.pop();
            Some(s)
        }
        "2.5.29.17" | "2.5.29.18" => {
            let names = parse_general_names(&t).ok()?;
            Some(
                names
                    .iter()
                    .map(|g| general_name_text(g, true))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        }
        "2.5.29.31" | "2.5.29.46" => crl_dp_text(&t),
        "1.3.6.1.5.5.7.1.1" | "1.3.6.1.5.5.7.1.11" => {
            let mut r = t.reader();
            let mut lines = Vec::new();
            while !r.is_empty() {
                let ad = r.next().ok()?;
                let mut ar = ad.reader();
                let m = ar.next().ok()?.oid().ok()?;
                let g = parse_general_name(&ar.next().ok()?).ok()?;
                lines.push(format!("{} - {}", oid::ln(&m), general_name_text(&g, true)));
            }
            Some(lines.join("\n"))
        }
        "2.5.29.32" => policies_text(&t),
        "2.5.29.30" => {
            let mut r = t.reader();
            let mut s = String::new();
            while !r.is_empty() {
                let x = r.next().ok()?;
                s.push_str(if x.num == 0 {
                    "Permitted:\n"
                } else {
                    "Excluded:\n"
                });
                let mut sr = x.reader();
                while !sr.is_empty() {
                    let sub = sr.next().ok()?;
                    let mut gr = sub.reader();
                    let g = parse_general_name(&gr.next().ok()?).ok()?;
                    let txt = match &g {
                        GeneralName::Ip(b) if b.len() == 8 => {
                            format!("IP:{}/{}", ip_text(&b[..4]), ip_text(&b[4..]))
                        }
                        GeneralName::Ip(b) if b.len() == 32 => {
                            format!("IP:{}/{}", ip_text(&b[..16]), ip_text(&b[16..]))
                        }
                        other => general_name_text(other, true),
                    };
                    s.push_str(&format!("  {}\n", txt));
                }
            }
            s.pop();
            Some(s)
        }
        "2.5.29.36" => {
            let mut r = t.reader();
            let mut v = Vec::new();
            while !r.is_empty() {
                let x = r.next().ok()?;
                let n = signed_decimal(x.content);
                v.push(match x.num {
                    0 => format!("Require Explicit Policy:{}", n),
                    _ => format!("Inhibit Policy Mapping:{}", n),
                });
            }
            Some(v.join(", "))
        }
        "2.5.29.54" => Some(signed_decimal(t.integer().ok()?)),
        "2.5.29.20" => Some(signed_hex(t.integer().ok()?)),
        "2.16.840.1.113730.1.13"
        | "2.16.840.1.113730.1.2"
        | "2.16.840.1.113730.1.3"
        | "2.16.840.1.113730.1.4"
        | "2.16.840.1.113730.1.7"
        | "2.16.840.1.113730.1.8"
        | "2.16.840.1.113730.1.12" => Some(lossy(&t.string_bytes().ok()?)),
        _ => None,
    }
}

fn crl_dp_text(t: &Tlv) -> Option<String> {
    let mut r = t.reader();
    let mut s = String::new();
    while !r.is_empty() {
        let dp = r.next().ok()?;
        let mut dr = dp.reader();
        while !dr.is_empty() {
            let x = dr.next().ok()?;
            match x.num {
                0 => {
                    let inner = der::parse(x.content).ok()?;
                    match inner.num {
                        0 => {
                            s.push_str("Full Name:\n");
                            let names = parse_general_names(&inner).ok()?;
                            for n in names {
                                s.push_str(&format!("  {}\n", general_name_text(&n, true)));
                            }
                        }
                        _ => {
                            s.push_str("Relative Name:\n");
                        }
                    }
                }
                1 => {
                    s.push_str("Reasons:");
                    s.push('\n');
                }
                2 => {
                    s.push_str("CRL Issuer:\n");
                    let names = parse_general_names(&x).ok()?;
                    for n in names {
                        s.push_str(&format!("  {}\n", general_name_text(&n, true)));
                    }
                }
                _ => {}
            }
        }
        s.push('\n');
    }
    Some(s)
}

fn policies_text(t: &Tlv) -> Option<String> {
    let mut r = t.reader();
    let mut lines: Vec<String> = Vec::new();
    while !r.is_empty() {
        let pi = r.next().ok()?;
        let mut pr = pi.reader();
        let id = pr.next().ok()?.oid().ok()?;
        lines.push(format!("Policy: {}", oid::ln(&id)));
        if pr.is_empty() {
            continue;
        }
        let quals = pr.next().ok()?;
        let mut qr = quals.reader();
        while !qr.is_empty() {
            let q = qr.next().ok()?;
            let mut qir = q.reader();
            let qid = qir.next().ok()?.oid().ok()?;
            let qv = qir.next().ok()?;
            match qid.as_str() {
                "1.3.6.1.5.5.7.2.1" => lines.push(format!("  CPS: {}", lossy(qv.content))),
                "1.3.6.1.5.5.7.2.2" => {
                    lines.push("  User Notice:".into());
                    let mut ur = qv.reader();
                    while !ur.is_empty() {
                        let x = ur.next().ok()?;
                        if x.is_universal(tag::SEQUENCE) {
                            let mut nr = x.reader();
                            let org = nr.next().ok()?;
                            lines.push(format!(
                                "    Organization: {}",
                                lossy(&to_utf8(org.num, org.content))
                            ));
                            let nums = nr.next().ok()?;
                            let mut nnr = nums.reader();
                            let mut ns = Vec::new();
                            while !nnr.is_empty() {
                                ns.push(signed_decimal(nnr.next().ok()?.content));
                            }
                            lines.push(format!("    Numbers: {}", ns.join(", ")));
                        } else {
                            lines.push(format!(
                                "    Explicit Text: {}",
                                lossy(&to_utf8(x.num, x.content))
                            ));
                        }
                    }
                }
                _ => lines.push(format!("  Unknown Qualifier: {}", oid::ln(&qid))),
            }
        }
    }
    Some(lines.join("\n"))
}

// ------------------------------------------------------------ purposes

/// The X509_PURPOSE ids and their names (observed on PHP 8.5).
pub const PURPOSES: &[(i64, &str, &str)] = &[
    (1, "sslclient", "SSL client"),
    (2, "sslserver", "SSL server"),
    (3, "nssslserver", "Netscape SSL server"),
    (4, "smimesign", "S/MIME signing"),
    (5, "smimeencrypt", "S/MIME encryption"),
    (6, "crlsign", "CRL signing"),
    (7, "any", "Any Purpose"),
    (8, "ocsphelper", "OCSP helper"),
    (9, "timestampsign", "Time Stamp signing"),
    (10, "codesign", "Code signing"),
];

const KU_DIGITAL_SIGNATURE: u16 = 1;
const KU_NON_REPUDIATION: u16 = 2;
const KU_KEY_ENCIPHERMENT: u16 = 4;
const KU_KEY_AGREEMENT: u16 = 16;
const KU_KEY_CERT_SIGN: u16 = 32;
const KU_CRL_SIGN: u16 = 64;

const NS_SSL_CLIENT: u16 = 1;
const NS_SSL_SERVER: u16 = 2;
const NS_SMIME: u16 = 4;
const NS_SSL_CA: u16 = 32;
const NS_SMIME_CA: u16 = 64;
const NS_ANY_CA: u16 = 32 | 64 | 128;

impl Cert {
    fn ku_ok(&self, bits: u16) -> bool {
        self.key_usage().is_none_or(|k| k & bits != 0)
    }
    fn eku_ok(&self, oids: &[&str]) -> bool {
        self.ext_key_usage()
            .is_none_or(|v| v.iter().any(|o| oids.contains(&o.as_str())))
    }
    fn ns_ok(&self, bits: u16) -> bool {
        self.ns_cert_type().is_none_or(|n| n & bits != 0)
    }

    /// The generic CA test (OpenSSL documentation, "Certificate
    /// Extensions": basicConstraints CA, or a v1 self-signed certificate,
    /// or keyCertSign / Netscape CA bits without basicConstraints).
    pub fn ca_check(&self) -> i32 {
        if let Some(k) = self.key_usage()
            && k & KU_KEY_CERT_SIGN == 0
        {
            return 0;
        }
        match self.basic_constraints() {
            Some((true, _)) => 1,
            Some((false, _)) => 0,
            None => {
                if self.version == 0 && self.is_self_issued() {
                    3
                } else if self.key_usage().is_some() {
                    4
                } else if self.ns_cert_type().is_some_and(|n| n & NS_ANY_CA != 0) {
                    5
                } else {
                    0
                }
            }
        }
    }

    /// X509_check_purpose(cert, id, ca) as an int (0 = no).
    pub fn check_purpose(&self, id: i64, ca: bool) -> i32 {
        const SERVER_EKU: &[&str] = &[
            "1.3.6.1.5.5.7.3.1",
            "2.16.840.1.113730.4.1",
            "1.3.6.1.4.1.311.10.3.3",
        ];
        const CLIENT_EKU: &[&str] = &["1.3.6.1.5.5.7.3.2"];
        const EMAIL_EKU: &[&str] = &["1.3.6.1.5.5.7.3.4"];
        match id {
            1 => {
                if !self.eku_ok(CLIENT_EKU) {
                    return 0;
                }
                if ca {
                    return self.ssl_ca(NS_SSL_CA);
                }
                if !self.ku_ok(KU_DIGITAL_SIGNATURE | KU_KEY_AGREEMENT) {
                    return 0;
                }
                self.ns_ok(NS_SSL_CLIENT) as i32
            }
            2 | 3 => {
                if !self.eku_ok(SERVER_EKU) {
                    return 0;
                }
                if ca {
                    return self.ssl_ca(NS_SSL_CA);
                }
                if !self.ns_ok(NS_SSL_SERVER) {
                    return 0;
                }
                let bits = if id == 3 {
                    KU_KEY_ENCIPHERMENT
                } else {
                    KU_DIGITAL_SIGNATURE | KU_KEY_ENCIPHERMENT | KU_KEY_AGREEMENT
                };
                self.ku_ok(bits) as i32
            }
            4 | 5 => {
                if !self.eku_ok(EMAIL_EKU) {
                    return 0;
                }
                if ca {
                    let r = self.ca_check();
                    if r == 0 {
                        return 0;
                    }
                    if self.basic_constraints().is_none() {
                        return self.ns_ok(NS_SMIME_CA) as i32;
                    }
                    return r;
                }
                if let Some(n) = self.ns_cert_type()
                    && n & NS_SMIME == 0
                {
                    if n & NS_SSL_CLIENT == 0 {
                        return 0;
                    }
                    // Tolerated with a warning.
                }
                let bits = if id == 4 {
                    KU_DIGITAL_SIGNATURE | KU_NON_REPUDIATION
                } else {
                    KU_KEY_ENCIPHERMENT
                };
                self.ku_ok(bits) as i32
            }
            6 => {
                if ca {
                    let r = self.ca_check();
                    return if r == 2 { 0 } else { r };
                }
                self.ku_ok(KU_CRL_SIGN) as i32
            }
            7 => 1,
            8 => {
                if ca {
                    return self.ca_check();
                }
                1
            }
            9 => {
                if ca {
                    return self.ca_check();
                }
                // RFC 3161 §2.3: the only EKU is timeStamping, critical.
                let Some(e) = self.ext("2.5.29.37") else {
                    return 0;
                };
                let Some(v) = self.ext_key_usage() else {
                    return 0;
                };
                if v.len() != 1 || v[0] != "1.3.6.1.5.5.7.3.8" || !e.critical {
                    return 0;
                }
                if let Some(k) = self.key_usage()
                    && k & !(KU_DIGITAL_SIGNATURE | KU_NON_REPUDIATION) != 0
                {
                    return 0;
                }
                1
            }
            10 => {
                if ca {
                    return self.ca_check();
                }
                let Some(v) = self.ext_key_usage() else {
                    return 0;
                };
                if !v.iter().any(|o| o == "1.3.6.1.5.5.7.3.3") {
                    return 0;
                }
                if self.basic_constraints().is_some_and(|(c, _)| c) {
                    return 0;
                }
                match self.key_usage() {
                    Some(k) if k & KU_DIGITAL_SIGNATURE != 0 => 1,
                    _ => 0,
                }
            }
            _ => -1,
        }
    }

    fn ssl_ca(&self, ns_bit: u16) -> i32 {
        let r = self.ca_check();
        if r == 0 {
            return 0;
        }
        if self.basic_constraints().is_none() && !self.ns_ok(ns_bit) {
            return 0;
        }
        r
    }
}

// ------------------------------------------------------------ signatures

/// (digest, scheme) of a signature algorithm OID.
pub fn sig_alg_info(oid: &str) -> Option<(Option<Md>, SigScheme)> {
    use purecrypto::hash::HashAlgorithm as H;
    let h = |a| Some(Md::H(a));
    Some(match oid {
        "1.2.840.113549.1.1.4" => (h(H::Md5), SigScheme::Rsa),
        "1.2.840.113549.1.1.3" => (h(H::Md4), SigScheme::Rsa),
        "1.2.840.113549.1.1.5" | "1.3.14.3.2.29" => (h(H::Sha1), SigScheme::Rsa),
        "1.2.840.113549.1.1.14" => (h(H::Sha224), SigScheme::Rsa),
        "1.2.840.113549.1.1.11" => (h(H::Sha256), SigScheme::Rsa),
        "1.2.840.113549.1.1.12" => (h(H::Sha384), SigScheme::Rsa),
        "1.2.840.113549.1.1.13" => (h(H::Sha512), SigScheme::Rsa),
        "1.2.840.113549.1.1.15" => (h(H::Sha512_224), SigScheme::Rsa),
        "1.2.840.113549.1.1.16" => (h(H::Sha512_256), SigScheme::Rsa),
        "1.3.36.3.3.1.2" => (h(H::Ripemd160), SigScheme::Rsa),
        "2.16.840.1.101.3.4.3.13" => (h(H::Sha3_224), SigScheme::Rsa),
        "2.16.840.1.101.3.4.3.14" => (h(H::Sha3_256), SigScheme::Rsa),
        "2.16.840.1.101.3.4.3.15" => (h(H::Sha3_384), SigScheme::Rsa),
        "2.16.840.1.101.3.4.3.16" => (h(H::Sha3_512), SigScheme::Rsa),
        "1.2.840.113549.1.1.10" => (None, SigScheme::RsaPss),
        "1.2.840.10045.4.1" => (h(H::Sha1), SigScheme::Ecdsa),
        "1.2.840.10045.4.3.1" => (h(H::Sha224), SigScheme::Ecdsa),
        "1.2.840.10045.4.3.2" => (h(H::Sha256), SigScheme::Ecdsa),
        "1.2.840.10045.4.3.3" => (h(H::Sha384), SigScheme::Ecdsa),
        "1.2.840.10045.4.3.4" => (h(H::Sha512), SigScheme::Ecdsa),
        "2.16.840.1.101.3.4.3.9" => (h(H::Sha3_224), SigScheme::Ecdsa),
        "2.16.840.1.101.3.4.3.10" => (h(H::Sha3_256), SigScheme::Ecdsa),
        "2.16.840.1.101.3.4.3.11" => (h(H::Sha3_384), SigScheme::Ecdsa),
        "2.16.840.1.101.3.4.3.12" => (h(H::Sha3_512), SigScheme::Ecdsa),
        "1.2.840.10040.4.3" => (h(H::Sha1), SigScheme::Dsa),
        "2.16.840.1.101.3.4.3.1" => (h(H::Sha224), SigScheme::Dsa),
        "2.16.840.1.101.3.4.3.2" => (h(H::Sha256), SigScheme::Dsa),
        "1.3.101.112" => (None, SigScheme::Ed25519),
        "1.3.101.113" => (None, SigScheme::Ed448),
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigScheme {
    Rsa,
    RsaPss,
    Ecdsa,
    Dsa,
    Ed25519,
    Ed448,
}

/// The signature AlgorithmIdentifier for a key type and digest; none
/// when the combination has no OID.
pub fn sig_alg_for(key: &Key, md: Option<Md>) -> Option<Vec<u8>> {
    use crate::key::KeyType as T;
    use purecrypto::hash::HashAlgorithm as H;
    let name = md.map(|m| m.name());
    let o = match (key.key_type(), name) {
        (T::Rsa, Some("md5")) => "1.2.840.113549.1.1.4",
        (T::Rsa, Some("md4")) => "1.2.840.113549.1.1.3",
        (T::Rsa, Some("sha1")) => "1.2.840.113549.1.1.5",
        (T::Rsa, Some("sha224")) => "1.2.840.113549.1.1.14",
        (T::Rsa, Some("sha256")) => "1.2.840.113549.1.1.11",
        (T::Rsa, Some("sha384")) => "1.2.840.113549.1.1.12",
        (T::Rsa, Some("sha512")) => "1.2.840.113549.1.1.13",
        (T::Rsa, Some("ripemd160")) => "1.3.36.3.3.1.2",
        (T::Rsa, Some("sha3-224")) => "2.16.840.1.101.3.4.3.13",
        (T::Rsa, Some("sha3-256")) => "2.16.840.1.101.3.4.3.14",
        (T::Rsa, Some("sha3-384")) => "2.16.840.1.101.3.4.3.15",
        (T::Rsa, Some("sha3-512")) => "2.16.840.1.101.3.4.3.16",
        (T::Ec, Some("sha1")) => "1.2.840.10045.4.1",
        (T::Ec, Some("sha224")) => "1.2.840.10045.4.3.1",
        (T::Ec, Some("sha256")) => "1.2.840.10045.4.3.2",
        (T::Ec, Some("sha384")) => "1.2.840.10045.4.3.3",
        (T::Ec, Some("sha512")) => "1.2.840.10045.4.3.4",
        (T::Ec, Some("sha3-224")) => "2.16.840.1.101.3.4.3.9",
        (T::Ec, Some("sha3-256")) => "2.16.840.1.101.3.4.3.10",
        (T::Ec, Some("sha3-384")) => "2.16.840.1.101.3.4.3.11",
        (T::Ec, Some("sha3-512")) => "2.16.840.1.101.3.4.3.12",
        (T::Dsa, Some("sha1")) => "1.2.840.10040.4.3",
        (T::Dsa, Some("sha224")) => "2.16.840.1.101.3.4.3.1",
        (T::Dsa, Some("sha256")) => "2.16.840.1.101.3.4.3.2",
        (T::Ed25519, None) => return Some(der::algorithm_id("1.3.101.112", None)),
        (T::Ed448, None) => return Some(der::algorithm_id("1.3.101.113", None)),
        _ => return None,
    };
    let _ = H::Sha1;
    if key.key_type() == T::Rsa {
        Some(der::algorithm_id(o, Some(&der::null())))
    } else {
        Some(der::algorithm_id(o, None))
    }
}

/// Verifies `sig` over `tbs` with algorithm `alg`. None when the
/// algorithm doesn't fit the key.
pub fn verify_sig(
    alg: &str,
    params: Option<&[u8]>,
    tbs: &[u8],
    sig: &[u8],
    key: &Key,
) -> Option<bool> {
    use crate::key::KeyType as T;
    let (md, scheme) = sig_alg_info(alg)?;
    let kt = key.key_type();
    let fits = matches!(
        (scheme, kt),
        (SigScheme::Rsa | SigScheme::RsaPss, T::Rsa)
            | (SigScheme::Ecdsa, T::Ec)
            | (SigScheme::Dsa, T::Dsa)
            | (SigScheme::Ed25519, T::Ed25519)
            | (SigScheme::Ed448, T::Ed448)
    );
    if !fits {
        return None;
    }
    if scheme == SigScheme::RsaPss {
        let (hmd, slen) = pss_params(params?)?;
        let crate::key::KeyData::Rsa { public, n, .. } = &key.data else {
            return None;
        };
        let Ok(em) = public.raw_public(sig) else {
            return Some(false);
        };
        let h = hmd.digest(tbs);
        let bits = {
            let s = crate::key::strip(n);
            (s.len() - 1) * 8 + (8 - s[0].leading_zeros() as usize)
        };
        return Some(crate::key::pss_verify(&em, &h, hmd, bits - 1, Some(slen)));
    }
    key.verify(md, tbs, sig, false).ok()
}

/// (hash, salt length) of RSASSA-PSS-params (RFC 4055 §3.1).
pub fn pss_params(p: &[u8]) -> Option<(Md, usize)> {
    let t = der::parse(p).ok()?;
    let mut md = Md::H(purecrypto::hash::HashAlgorithm::Sha1);
    let mut slen = 20usize;
    let mut r = t.reader();
    while !r.is_empty() {
        let x = r.next().ok()?;
        let inner = der::parse(x.content).ok()?;
        match x.num {
            0 => {
                let o = inner.reader().next().ok()?.oid().ok()?;
                md = Md::by_oid(&o)?;
            }
            2 => slen = inner.small_int().ok()? as usize,
            _ => {}
        }
    }
    Some((md, slen))
}

/// All certificates in PEM text (or one DER certificate).
pub fn certs_from_bytes(data: &[u8]) -> Vec<Cert> {
    let blocks = crate::pem::parse_all(data);
    let mut out = Vec::new();
    for b in blocks {
        if (b.label == "CERTIFICATE"
            || b.label == "X509 CERTIFICATE"
            || b.label == "TRUSTED CERTIFICATE")
            && let Ok(c) = Cert::from_der(&b.data)
        {
            out.push(c);
        }
    }
    out
}

/// Reads one element of a sequence, for callers composing structures.
pub fn reader(d: &[u8]) -> Option<Reader<'_>> {
    der::parse(d).ok().map(|t| Reader::new(t.content))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A certificate PHP 8.5 issued (observed: hash f640d64b, the
    /// oneline name and the SAN rendering below).
    const CERT: &str = "-----BEGIN CERTIFICATE-----
MIIC4zCCAkygAwIBAgICEjQwDQYJKoZIhvcNAQELBQAwgZsxCzAJBgNVBAYTAkZS
MRcwFQYDVQQIDA7DjmxlLWRlLUZyYW5jZTEOMAwGA1UEBwwFUGFyaXMxFjAUBgNV
BAoMDU9yZyAiUSIgJiA8eD4xDjAMBgNVBAsMBVVuaXQxMQ4wDAYDVQQLDAVVbml0
MjEVMBMGA1UEAwwMdMOrc3QvbmFtZT0xMRQwEgYJKoZIhvcNAQkBFgVhQGIuYzAe
Fw0yNjEwMjgxNDMxNDlaFw0yNjExMjcxNDMxNDlaMIGbMQswCQYDVQQGEwJGUjEX
MBUGA1UECAwOw45sZS1kZS1GcmFuY2UxDjAMBgNVBAcMBVBhcmlzMRYwFAYDVQQK
DA1PcmcgIlEiICYgPHg+MQ4wDAYDVQQLDAVVbml0MTEOMAwGA1UECwwFVW5pdDIx
FTATBgNVBAMMDHTDq3N0L25hbWU9MTEUMBIGCSqGSIb3DQEJARYFYUBiLmMwgZ8w
DQYJKoZIhvcNAQEBBQADgY0AMIGJAoGBAL3R/h19FLnCrKU/56KoAvzx/K8JpY/B
ilHNc2NnOUcmQm0gAJB9xkVwzUNWdv/Mh1y+KHj1UpLe9N71DWaDEaKySU82s45c
f8Wr+9dqiXrjOmhEx0xTTvri+CsRJZ4NjDeuHpVCZNu/8wxOZRMlj9Ojfm8Z/h7j
6lqXcUTIdT3hAgMBAAGjNDAyMAwGA1UdEwQFMAMBAf8wFQYDVR0RBA4wDIEFYUBi
LmOCA2EuYjALBgMqA2MEBAECAwQwDQYJKoZIhvcNAQELBQADgYEAf3eSTeScTqL9
xpYIMNjRs0wopDHbxIdtl/Dc1qmuGbLXitX/yLj8s5oTUPnvC9UJaQs5sWJwq1y9
elDLkERpvhWWKbJONd+Qm52Fdh5mgvM8FruKeFLoJo76ShnWvBVnmqP4sLTKXemM
k7cdtlwtLX5rMfssQY+be0HPQX2sCY4=
-----END CERTIFICATE-----
";

    #[test]
    fn parse_observed_cert() {
        let c = certs_from_bytes(CERT.as_bytes()).pop().unwrap();
        assert_eq!(
            c.subject.oneline(),
            "/C=FR/ST=\\xC3\\x8Ele-de-France/L=Paris/O=Org \"Q\" & <x>/OU=Unit1/OU=Unit2/CN=t\\xC3\\xABst\\/name=1/emailAddress=a@b.c"
        );
        assert_eq!(format!("{:08x}", c.subject.hash()), "f640d64b");
        assert_eq!(c.serial_decimal(), "4660");
        assert_eq!(c.serial_hex(), "1234");
        let san = c.ext("2.5.29.17").unwrap();
        assert_eq!(extension_text(san).unwrap(), "email:a@b.c, DNS:a.b");
        assert_eq!(c.check_purpose(1, false), 1);
        assert_eq!(c.check_purpose(9, false), 0);
        assert_eq!(c.check_purpose(10, true), 1);
        assert_eq!(c.verify_signature_by(&c), true);
        assert_eq!(
            c.issuer.print_compat(),
            "C=FR, ST=\\xC3\\x8Ele-de-France, L=Paris, O=Org \"Q\" & <x>, OU=Unit1, OU=Unit2, CN=t\\xC3\\xABst\\/name=1/emailAddress=a@b.c"
        );
    }
}
