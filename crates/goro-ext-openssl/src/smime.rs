//! PKCS#7 / CMS SignedData and EnvelopedData (OpenSSL `pk7_smime.c`,
//! `cms_smime.c`, `asn_mime.c` semantics) and the php-src
//! openssl_pkcs7_* / openssl_cms_* functions.

use crate::asn1::{self, Tlv};
use crate::cert::{self, Cert};
use crate::cipher::{self, CipherSpec};
use crate::digest::Md;
use crate::pkey::{PKey, RSA_PKCS1_PADDING};
use crate::x509::{self, Store};
use crate::{err, pkey_fns, zpp};
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

// Flags (PKCS7_* == CMS_* bit values)
pub const F_TEXT: i64 = 0x1;
pub const F_NOCERTS: i64 = 0x2;
pub const F_NOSIGS: i64 = 0x4;
pub const F_NOCHAIN: i64 = 0x8;
pub const F_NOINTERN: i64 = 0x10;
pub const F_NOVERIFY: i64 = 0x20;
pub const F_DETACHED: i64 = 0x40;
pub const F_BINARY: i64 = 0x80;
pub const F_NOATTR: i64 = 0x100;
pub const F_NOSMIMECAP: i64 = 0x200;
pub const F_OLDMIME: i64 = 0x400;
pub const F_CRLFEOL: i64 = 0x800;

pub const ENCODING_DER: i64 = 0;
pub const ENCODING_SMIME: i64 = 1;
pub const ENCODING_PEM: i64 = 2;

const OID_DATA: &str = "1.2.840.113549.1.7.1";
const OID_SIGNED: &str = "1.2.840.113549.1.7.2";
const OID_ENVELOPED: &str = "1.2.840.113549.1.7.3";
const OID_SIGNED_ENVELOPED: &str = "1.2.840.113549.1.7.4";
const OID_AUTH_ENVELOPED: &str = "1.2.840.113549.1.9.16.1.23";
const OID_RSA: &str = "1.2.840.113549.1.1.1";
const OID_RSA_OAEP: &str = "1.2.840.113549.1.1.7";
const OID_CONTENT_TYPE: &str = "1.2.840.113549.1.9.3";
const OID_MESSAGE_DIGEST: &str = "1.2.840.113549.1.9.4";
const OID_SIGNING_TIME: &str = "1.2.840.113549.1.9.5";
const OID_SMIME_CAPS: &str = "1.2.840.113549.1.9.15";

// ---------------------------------------------------------------------------
// MIME helpers (asn_mime.c)
// ---------------------------------------------------------------------------

/// strip_eol(): drop trailing CR/LF, report whether a LF was present.
fn strip_eol(line: &[u8]) -> (&[u8], bool) {
    let mut len = line.len();
    let mut is_eol = false;
    while len > 0 {
        let c = line[len - 1];
        if c == b'\n' {
            is_eol = true;
        } else if c != b'\r' {
            break;
        }
        len -= 1;
    }
    (&line[..len], is_eol)
}

fn lines(data: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, &b) in data.iter().enumerate() {
        if b == b'\n' {
            out.push(&data[start..=i]);
            start = i + 1;
        }
    }
    if start < data.len() {
        out.push(&data[start..]);
    }
    out
}

/// SMIME_crlf_copy()
pub fn crlf_copy(data: &[u8], flags: i64) -> Vec<u8> {
    if flags & F_BINARY != 0 {
        return data.to_vec();
    }
    let mut out = Vec::with_capacity(data.len() + 32);
    if flags & F_TEXT != 0 {
        out.extend_from_slice(b"Content-Type: text/plain\r\n\r\n");
    }
    for l in lines(data) {
        let (body, eol) = strip_eol(l);
        out.extend_from_slice(body);
        if eol {
            out.extend_from_slice(b"\r\n");
        }
    }
    out
}

#[derive(Debug, Clone)]
struct MimeHeader {
    name: String,
    value: String,
    params: Vec<(String, String)>,
}

/// mime_parse_hdr(): returns headers and the offset of the body.
fn parse_headers(data: &[u8]) -> (Vec<MimeHeader>, usize) {
    let mut headers: Vec<MimeHeader> = Vec::new();
    let mut pos = 0;
    let mut current: Option<String> = None;
    let flush = |cur: &mut Option<String>, headers: &mut Vec<MimeHeader>| {
        if let Some(line) = cur.take() {
            if let Some(h) = parse_header_line(&line) {
                headers.push(h);
            }
        }
    };
    while pos < data.len() {
        let end = data[pos..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|p| pos + p + 1)
            .unwrap_or(data.len());
        let raw = &data[pos..end];
        pos = end;
        let (line, _) = strip_eol(raw);
        if line.is_empty() {
            break;
        }
        let s = String::from_utf8_lossy(line).to_string();
        if s.starts_with(' ') || s.starts_with('\t') {
            if let Some(c) = current.as_mut() {
                c.push_str(&s);
                continue;
            }
        }
        flush(&mut current, &mut headers);
        current = Some(s);
    }
    flush(&mut current, &mut headers);
    (headers, pos)
}

fn parse_header_line(line: &str) -> Option<MimeHeader> {
    let colon = line.find(':')?;
    let name = line[..colon].trim().to_ascii_lowercase();
    let rest = &line[colon + 1..];
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut inq = false;
    for c in rest.chars() {
        match c {
            '"' => {
                inq = !inq;
            }
            ';' if !inq => {
                parts.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    parts.push(cur);
    let value = parts
        .first()
        .map(|v| v.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let mut params = Vec::new();
    for p in parts.iter().skip(1) {
        if let Some(eq) = p.find('=') {
            params.push((
                p[..eq].trim().to_ascii_lowercase(),
                p[eq + 1..].trim().to_string(),
            ));
        }
    }
    Some(MimeHeader {
        name,
        value,
        params,
    })
}

fn find_header<'a>(h: &'a [MimeHeader], name: &str) -> Option<&'a MimeHeader> {
    h.iter().find(|x| x.name == name)
}

/// multi_split()
fn multi_split(data: &[u8], bound: &str) -> Option<Vec<Vec<u8>>> {
    let b = bound.as_bytes();
    let mut parts: Vec<Vec<u8>> = Vec::new();
    let mut part = 0;
    let mut first = true;
    let mut eol = false;
    let mut cur: Option<Vec<u8>> = None;
    for l in lines(data) {
        let state = if l.len() >= b.len() + 2 && l.starts_with(b"--") && &l[2..2 + b.len()] == b {
            if l[2 + b.len()..].starts_with(b"--") {
                2
            } else {
                1
            }
        } else {
            0
        };
        if state == 1 {
            first = true;
            part += 1;
        } else if state == 2 {
            if let Some(c) = cur.take() {
                parts.push(c);
            }
            return Some(parts);
        } else if part != 0 {
            let (body, next_eol) = strip_eol(l);
            if first {
                first = false;
                if let Some(c) = cur.take() {
                    parts.push(c);
                }
                cur = Some(Vec::new());
            } else if eol {
                cur.as_mut()?.extend_from_slice(b"\r\n");
            }
            eol = next_eol;
            cur.as_mut()?.extend_from_slice(body);
        }
    }
    None
}

fn b64_body(data: &[u8]) -> Option<Vec<u8>> {
    let s: String = String::from_utf8_lossy(data)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let d = crate::base64_decode(&s)?;
    if d.is_empty() { None } else { Some(d) }
}

/// SMIME_read_ASN1_ex(): (ContentInfo DER, detached content)
fn smime_read(data: &[u8]) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    let (headers, body_off) = parse_headers(data);
    let Some(ct) = find_header(&headers, "content-type") else {
        err::push(err::LIB_ASN1, 209, "no content type");
        return None;
    };
    if ct.value == "multipart/signed" {
        let Some(bound) = ct
            .params
            .iter()
            .find(|(n, _)| n == "boundary")
            .map(|(_, v)| v.clone())
        else {
            err::push(err::LIB_ASN1, 211, "no multipart boundary");
            return None;
        };
        let Some(parts) = multi_split(&data[body_off..], &bound) else {
            err::push(err::LIB_ASN1, 210, "no multipart body failure");
            return None;
        };
        if parts.len() != 2 {
            err::push(err::LIB_ASN1, 210, "no multipart body failure");
            return None;
        }
        let (sh, soff) = parse_headers(&parts[1]);
        let Some(sct) = find_header(&sh, "content-type") else {
            err::push(err::LIB_ASN1, 212, "no sig content type");
            return None;
        };
        if sct.value != "application/x-pkcs7-signature"
            && sct.value != "application/pkcs7-signature"
        {
            err::push(err::LIB_ASN1, 213, "sig invalid mime type");
            return None;
        }
        let der = b64_body(&parts[1][soff..]).or_else(|| {
            err::push(err::LIB_ASN1, 204, "asn1 sig parse error");
            None
        })?;
        return Some((der, Some(parts[0].clone())));
    }
    if ct.value != "application/x-pkcs7-mime" && ct.value != "application/pkcs7-mime" {
        err::push(err::LIB_ASN1, 205, "invalid mime type");
        return None;
    }
    let der = b64_body(&data[body_off..]).or_else(|| {
        err::push(err::LIB_ASN1, 203, "asn1 parse error");
        None
    })?;
    Some((der, None))
}

fn b64_lines(der: &[u8]) -> String {
    let b = crate::base64_encode(der);
    let mut s = String::with_capacity(b.len() + b.len() / 64 + 2);
    for chunk in b.as_bytes().chunks(64) {
        s.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        s.push('\n');
    }
    s
}

fn micalg(md: Md) -> &'static str {
    match md {
        Md::Md5 => "md5",
        Md::Sha1 => "sha1",
        Md::Sha224 => "sha-224",
        Md::Sha256 => "sha-256",
        Md::Sha384 => "sha-384",
        Md::Sha512 => "sha-512",
        _ => "unknown",
    }
}

/// SMIME_write_ASN1_ex()
fn smime_write(
    der: &[u8],
    ctype: &str,
    detached_data: Option<&[u8]>,
    mds: &[Md],
    flags: i64,
    oldmime: bool,
) -> Vec<u8> {
    let eol = if flags & F_CRLFEOL != 0 { "\r\n" } else { "\n" };
    let prefix = if oldmime {
        "application/x-pkcs7-"
    } else {
        "application/pkcs7-"
    };
    let mut out = Vec::new();
    if let Some(data) = detached_data {
        use purecrypto::rng::RngCore;
        let mut rb = [0u8; 32];
        purecrypto::rng::OsRng.fill_bytes(&mut rb);
        let bound: String = rb
            .iter()
            .map(|b| {
                let c = b & 0xf;
                if c < 10 {
                    (b'0' + c) as char
                } else {
                    (b'A' + c - 10) as char
                }
            })
            .collect();
        let mic: Vec<&str> = mds.iter().map(|m| micalg(*m)).collect();
        out.extend(format!("MIME-Version: 1.0{}", eol).bytes());
        out.extend(
            format!(
                "Content-Type: multipart/signed; protocol=\"{}signature\";",
                prefix
            )
            .bytes(),
        );
        out.extend(
            format!(
                " micalg=\"{}\"; boundary=\"----{}\"{}{}",
                mic.join(","),
                bound,
                eol,
                eol
            )
            .bytes(),
        );
        out.extend(format!("This is an S/MIME signed message{}{}", eol, eol).bytes());
        out.extend(format!("------{}{}", bound, eol).bytes());
        out.extend(crlf_copy(data, flags));
        out.extend(format!("{}------{}{}", eol, bound, eol).bytes());
        out.extend(
            format!(
                "Content-Type: {}signature; name=\"smime.p7s\"{}",
                prefix, eol
            )
            .bytes(),
        );
        out.extend(format!("Content-Transfer-Encoding: base64{}", eol).bytes());
        out.extend(
            format!(
                "Content-Disposition: attachment; filename=\"smime.p7s\"{}{}",
                eol, eol
            )
            .bytes(),
        );
        out.extend(b64_lines(der).bytes());
        out.extend(format!("{}------{}--{}{}", eol, bound, eol, eol).bytes());
        return out;
    }
    let (msg_type, cname) = match ctype {
        OID_ENVELOPED => (Some("enveloped-data"), "smime.p7m"),
        OID_AUTH_ENVELOPED => (Some("authEnveloped-data"), "smime.p7m"),
        OID_SIGNED => (
            Some(if mds.is_empty() {
                "certs-only"
            } else {
                "signed-data"
            }),
            "smime.p7m",
        ),
        _ => (None, "smime.p7m"),
    };
    out.extend(format!("MIME-Version: 1.0{}", eol).bytes());
    out.extend(
        format!(
            "Content-Disposition: attachment; filename=\"{}\"{}",
            cname, eol
        )
        .bytes(),
    );
    out.extend(format!("Content-Type: {}mime;", prefix).bytes());
    if let Some(t) = msg_type {
        out.extend(format!(" smime-type={};", t).bytes());
    }
    out.extend(format!(" name=\"{}\"{}", cname, eol).bytes());
    out.extend(format!("Content-Transfer-Encoding: base64{}{}", eol, eol).bytes());
    out.extend(b64_lines(der).bytes());
    out.extend(eol.bytes());
    out
}

/// SMIME_text(): strip the MIME header of a text/plain part.
fn smime_text(data: &[u8]) -> Option<Vec<u8>> {
    let (h, off) = parse_headers(data);
    match find_header(&h, "content-type") {
        Some(ct) if ct.value == "text/plain" => Some(data[off..].to_vec()),
        Some(_) => {
            err::push(err::LIB_ASN1, 205, "invalid mime type");
            None
        }
        None => {
            err::push(err::LIB_ASN1, 206, "mime no content type");
            None
        }
    }
}

/// Pick the PKCS7_R_* or CMS_R_* reason code.
fn r(cms: bool, pkcs7: u32, cms_code: u32) -> u32 {
    if cms { cms_code } else { pkcs7 }
}

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Sid {
    IssuerSerial(Vec<u8>, Vec<u8>),
    Ski(Vec<u8>),
}

#[derive(Clone, Debug)]
struct SignerInfo {
    sid: Sid,
    digest_oid: String,
    /// Content of the [0] signed attributes (re-tagged as SET for signing).
    signed_attrs: Option<Vec<u8>>,
    signature: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
struct SignedData {
    digest_algs: Vec<String>,
    econtent_type: String,
    content: Option<Vec<u8>>,
    certs: Vec<Vec<u8>>,
    crls: Vec<Vec<u8>>,
    signers: Vec<SignerInfo>,
}

fn parse_sid(t: &Tlv) -> Option<Sid> {
    if t.tag == asn1::SEQUENCE {
        let c = t.children()?;
        return Some(Sid::IssuerSerial(
            c.first()?.raw.to_vec(),
            c.get(1)?.content.to_vec(),
        ));
    }
    if t.context() == Some(0) {
        return Some(Sid::Ski(t.content.to_vec()));
    }
    None
}

fn content_info(der: &[u8]) -> Option<(String, Option<Tlv<'_>>)> {
    let t = asn1::parse_one(der).or_else(|| asn1::parse(der).map(|(t, _)| t))?;
    let c = t.children()?;
    let oid = c.first()?.oid()?;
    let inner = match c.get(1) {
        Some(x) if x.context() == Some(0) => asn1::parse(x.content).map(|(t, _)| t),
        _ => None,
    };
    Some((oid, inner))
}

fn octets_content(t: &Tlv) -> Vec<u8> {
    // OCTET STRING, possibly constructed (BER)
    if t.tag == 0x24 {
        let mut v = Vec::new();
        if let Some(ch) = t.children() {
            for c in ch {
                v.extend(octets_content(&c));
            }
        }
        return v;
    }
    t.content.to_vec()
}

fn parse_signed(t: &Tlv) -> Option<SignedData> {
    let c = t.children()?;
    let mut sd = SignedData::default();
    let mut i = 1;
    for a in c.get(i)?.children()? {
        sd.digest_algs.push(a.children()?.first()?.oid()?);
    }
    i += 1;
    let encap = c.get(i)?.children()?;
    sd.econtent_type = encap.first()?.oid()?;
    if let Some(ec) = encap.get(1) {
        if ec.context() == Some(0) {
            let inner = asn1::parse(ec.content)?.0;
            sd.content = Some(if inner.tag == asn1::OCTET_STRING || inner.tag == 0x24 {
                octets_content(&inner)
            } else {
                inner.raw.to_vec()
            });
        }
    }
    i += 1;
    while let Some(x) = c.get(i) {
        match x.context() {
            Some(0) => {
                for cert in asn1::parse_all(x.content)? {
                    if cert.tag == asn1::SEQUENCE {
                        sd.certs.push(cert.raw.to_vec());
                    }
                }
                i += 1;
            }
            Some(1) => {
                for crl in asn1::parse_all(x.content)? {
                    sd.crls.push(crl.raw.to_vec());
                }
                i += 1;
            }
            _ => break,
        }
    }
    if let Some(sis) = c.get(i) {
        for si in sis.children()? {
            let f = si.children()?;
            let sid = parse_sid(f.get(1)?)?;
            let digest_oid = f.get(2)?.children()?.first()?.oid()?;
            let mut j = 3;
            let mut signed_attrs = None;
            if f.get(j)?.context() == Some(0) {
                signed_attrs = Some(f[j].content.to_vec());
                j += 1;
            }
            f.get(j)?; // signatureAlgorithm
            let signature = f.get(j + 1)?.content.to_vec();
            sd.signers.push(SignerInfo {
                sid,
                digest_oid,
                signed_attrs,
                signature,
            });
        }
    }
    Some(sd)
}

fn find_attr<'a>(attrs: &'a [u8], oid: &str) -> Option<Vec<Tlv<'a>>> {
    for a in asn1::parse_all(attrs)? {
        let c = a.children()?;
        if c.first()?.oid()? == oid {
            return c.get(1)?.children();
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Signing
// ---------------------------------------------------------------------------

/// EVP_PKEY_get_default_digest_nid()
fn default_md(key: &PKey) -> Option<Md> {
    match key {
        PKey::Ecx(_) => Some(Md::Sha512),
        PKey::Ec(e) if e.sm2 => Some(Md::Sm3),
        _ => Some(Md::Sha256),
    }
}

fn signer_sig_alg(key: &PKey, md: Md) -> Option<Vec<u8>> {
    match key {
        PKey::Rsa(_) => Some(asn1::algid(OID_RSA, Some(asn1::null()))),
        PKey::Ecx(x) => Some(asn1::algid(x.kind.oid(), None)),
        _ => cert::sig_alg_for(key, Some(md)),
    }
}

fn smime_caps() -> Vec<u8> {
    let c = |oid: &str, bits: Option<i64>| {
        let mut v = vec![asn1::oid(oid)];
        if let Some(b) = bits {
            v.push(asn1::int_i64(b));
        }
        asn1::seq(&v)
    };
    asn1::seq(&[
        c("2.16.840.1.101.3.4.1.42", None),
        c("2.16.840.1.101.3.4.1.22", None),
        c("2.16.840.1.101.3.4.1.2", None),
        c("1.2.840.113549.3.7", None),
        c("1.2.840.113549.3.2", Some(128)),
        c("1.2.840.113549.3.2", Some(64)),
        c("1.3.14.3.2.7", None),
        c("1.2.840.113549.3.2", Some(40)),
    ])
}

/// PKCS7_sign() / CMS_sign(): ContentInfo DER and the digest used.
fn sign(
    cert: &Cert,
    key: &PKey,
    others: &[Rc<Cert>],
    data: &[u8],
    flags: i64,
) -> Option<(Vec<u8>, Md)> {
    let md = default_md(key)?;
    let content = crlf_copy(data, flags);
    let sid = asn1::seq(&[cert.issuer_der.clone(), cert.serial_der()]);
    let digest_alg = match md {
        // OpenSSL puts NULL parameters on the digest AlgorithmIdentifier
        Md::Sha512 if matches!(key, PKey::Ecx(_)) => asn1::algid(md.oid()?, None),
        _ => asn1::algid(md.oid()?, Some(asn1::null())),
    };
    let sig_alg = signer_sig_alg(key, md)?;
    let (attrs_field, signature) = if flags & F_NOATTR == 0 {
        let mut attrs = vec![
            asn1::seq(&[
                asn1::oid(OID_CONTENT_TYPE),
                asn1::set(&[asn1::oid(OID_DATA)]),
            ]),
            {
                let (tag, t) = cert::asn1_time_from_unix(cert::now());
                asn1::seq(&[
                    asn1::oid(OID_SIGNING_TIME),
                    asn1::set(&[asn1::tlv(tag, &t)]),
                ])
            },
            asn1::seq(&[
                asn1::oid(OID_MESSAGE_DIGEST),
                asn1::set(&[asn1::octet(&md.digest(&content))]),
            ]),
        ];
        if flags & F_NOSMIMECAP == 0 {
            attrs.push(asn1::seq(&[
                asn1::oid(OID_SMIME_CAPS),
                asn1::set(&[smime_caps()]),
            ]));
        }
        attrs.sort();
        let set = asn1::tlv(asn1::SET, &attrs.concat());
        let sig = signer_sign(key, md, &set)?;
        (Some(asn1::tlv(0xa0, &attrs.concat())), sig)
    } else {
        (None, signer_sign(key, md, &content)?)
    };
    let mut si = vec![asn1::int_i64(1), sid, digest_alg.clone()];
    if let Some(a) = attrs_field {
        si.push(a);
    }
    si.push(sig_alg);
    si.push(asn1::octet(&signature));
    let mut encap = vec![asn1::oid(OID_DATA)];
    if flags & F_DETACHED == 0 {
        encap.push(asn1::explicit(0, &asn1::octet(&content)));
    }
    let mut certs = Vec::new();
    if flags & F_NOCERTS == 0 {
        certs.push(cert.der.clone());
    }
    for o in others {
        certs.push(o.der.clone());
    }
    let mut sd = vec![
        asn1::int_i64(1),
        asn1::set(&[digest_alg]),
        asn1::seq(&encap),
    ];
    if !certs.is_empty() {
        sd.push(asn1::tlv(0xa0, &certs.concat()));
    }
    sd.push(asn1::set(&[asn1::seq(&si)]));
    let ci = asn1::seq(&[asn1::oid(OID_SIGNED), asn1::explicit(0, &asn1::seq(&sd))]);
    Some((ci, md))
}

fn signer_sign(key: &PKey, md: Md, data: &[u8]) -> Option<Vec<u8>> {
    let mdo = if matches!(key, PKey::Ecx(_)) {
        None
    } else {
        Some(md)
    };
    key.sign(mdo, data, false).ok()
}

// ---------------------------------------------------------------------------
// Verification
// ---------------------------------------------------------------------------

fn find_signer_cert(
    sid: &Sid,
    internal: &[Rc<Cert>],
    others: &[Rc<Cert>],
    flags: i64,
) -> Option<Rc<Cert>> {
    let matches = |c: &Rc<Cert>| match sid {
        Sid::IssuerSerial(iss, ser) => {
            let n = asn1::parse_one(iss).and_then(|t| crate::name::Name::parse(&t));
            n.map(|n| n.eq_canon(&c.issuer)).unwrap_or(false) && c.serial == *ser
        }
        Sid::Ski(s) => c.skid().as_deref() == Some(s.as_slice()),
    };
    if flags & F_NOINTERN == 0 {
        if let Some(c) = internal.iter().find(|c| matches(c)) {
            return Some(c.clone());
        }
    }
    others.iter().find(|c| matches(c)).cloned()
}

/// PKCS7_verify()/CMS_verify(): content and signer certificates.
fn verify(
    sd: &SignedData,
    others: &[Rc<Cert>],
    store: &Store,
    indata: Option<&[u8]>,
    flags: i64,
    cms: bool,
    written: &mut Option<Vec<u8>>,
) -> Option<(Vec<u8>, Vec<Rc<Cert>>)> {
    let lib = if cms { err::LIB_CMS } else { err::LIB_PKCS7 };
    let content: Vec<u8> = match (&sd.content, indata) {
        (Some(c), None) => c.clone(),
        (Some(c), Some(_)) if cms => c.clone(),
        // CMS_verify() canonicalises detached content unless CMS_BINARY
        (_, Some(d)) if cms && flags & F_BINARY == 0 => crlf_copy(d, 0),
        (_, Some(d)) => d.to_vec(),
        (None, None) => {
            err::push(lib, r(cms, 122, 127), "no content");
            return None;
        }
    };
    let internal: Vec<Rc<Cert>> = sd
        .certs
        .iter()
        .filter_map(|d| Cert::parse(d))
        .map(Rc::new)
        .collect();
    let mut signers = Vec::new();
    for si in &sd.signers {
        match find_signer_cert(&si.sid, &internal, others, flags) {
            Some(c) => signers.push(c),
            None => {
                err::push(lib, r(cms, 128, 138), "signer certificate not found");
                return None;
            }
        }
    }
    if sd.signers.is_empty() {
        if cms {
            err::push(lib, 135, "no signers");
        } else {
            err::push(lib, 123, "no signatures on data");
        }
        return None;
    }
    if flags & F_NOVERIFY == 0 {
        for s in &signers {
            let chain: &[Rc<Cert>] = if flags & F_NOCHAIN != 0 && !cms {
                &[]
            } else {
                &internal
            };
            let purpose = if flags & F_NOCHAIN != 0 && !cms { 0 } else { 4 };
            if x509::verify_chain(store, s, chain, purpose) != 1 {
                err::push(lib, r(cms, 117, 100), "certificate verify error");
                return None;
            }
        }
    }
    let text_out = |content: &[u8]| -> Option<Vec<u8>> {
        if flags & F_TEXT != 0 {
            match smime_text(content) {
                Some(t) => Some(t),
                None => {
                    err::push(lib, r(cms, 129, 140), "smime text error");
                    None
                }
            }
        } else {
            Some(content.to_vec())
        }
    };
    if !cms {
        *written = Some(text_out(&content)?);
    }
    if flags & F_NOSIGS == 0 {
        for (si, c) in sd.signers.iter().zip(signers.iter()) {
            let Some(md) = Md::by_name(&si.digest_oid) else {
                if cms {
                    err::push(lib, 149, "unknown digest algorithm")
                } else {
                    err::push(lib, 109, "unknown digest type")
                };
                return None;
            };
            let key = c.public_key()?;
            let digest = md.digest(&content);
            let ok = match &si.signed_attrs {
                Some(attrs) => {
                    let md_attr = find_attr(attrs, OID_MESSAGE_DIGEST)
                        .and_then(|v| v.first().map(|t| t.content.to_vec()));
                    if md_attr.as_deref() != Some(digest.as_slice()) {
                        if cms {
                            err::push(lib, 158, "verification failure");
                        } else {
                            // PKCS7_signatureVerify(), then PKCS7_verify()
                            err::push(lib, 101, "digest failure");
                            err::push(lib, 105, "signature failure");
                        }
                        return None;
                    }
                    let set = asn1::tlv(asn1::SET, attrs);
                    verify_sig(&key, md, &set, &si.signature)
                }
                None => verify_sig(&key, md, &content, &si.signature),
            };
            if !ok {
                if cms {
                    err::push(lib, 158, "verification failure");
                } else {
                    err::push(lib, 105, "signature failure");
                    err::push(lib, 105, "signature failure");
                }
                return None;
            }
        }
    }
    let out = match written.take() {
        Some(o) => o,
        None => text_out(&content)?,
    };
    Some((out, signers))
}

fn verify_sig(key: &PKey, md: Md, data: &[u8], sig: &[u8]) -> bool {
    let mdo = if matches!(key, PKey::Ecx(_)) {
        None
    } else {
        Some(md)
    };
    key.verify(mdo, data, sig, false) == 1
}

// ---------------------------------------------------------------------------
// Enveloping
// ---------------------------------------------------------------------------

fn rc2_version(bits: usize) -> i64 {
    match bits {
        40 => 160,
        64 => 120,
        128 => 58,
        b => b as i64,
    }
}

fn cipher_params(spec: &CipherSpec, iv: &[u8]) -> Vec<u8> {
    match spec.algo {
        cipher::Algo::Rc2(bits) => asn1::seq(&[asn1::int_i64(rc2_version(bits)), asn1::octet(iv)]),
        _ => asn1::octet(iv),
    }
}

/// PKCS7_encrypt()/CMS_encrypt(): ContentInfo DER.
fn encrypt(
    certs: &[Rc<Cert>],
    data: &[u8],
    spec: &'static CipherSpec,
    flags: i64,
    cms: bool,
) -> Option<Vec<u8>> {
    use purecrypto::rng::RngCore;
    let lib = if cms { err::LIB_CMS } else { err::LIB_PKCS7 };
    let content = crlf_copy(data, flags);
    let mut key = vec![0u8; spec.key_len];
    purecrypto::rng::OsRng.fill_bytes(&mut key);
    let aead = spec.mode == cipher::Mode::Gcm;
    let mut iv = vec![0u8; if aead { 12 } else { spec.iv_len }];
    purecrypto::rng::OsRng.fill_bytes(&mut iv);
    let mut ris = Vec::new();
    for c in certs {
        let Some(PKey::Rsa(r)) = c.public_key() else {
            if cms {
                err::push(lib, 154, "unsupported recipient type");
            } else {
                // pkcs7_encode_rinfo(), then PKCS7_add_recipient()
                err::push(lib, 150, "encryption not supported for this key type");
                err::push(lib, 120, "error adding recipient");
            }
            return None;
        };
        let ek = r.encrypt(&key, RSA_PKCS1_PADDING, Md::Sha1)?;
        ris.push(asn1::seq(&[
            asn1::int_i64(0),
            asn1::seq(&[c.issuer_der.clone(), c.serial_der()]),
            asn1::algid(OID_RSA, Some(asn1::null())),
            asn1::octet(&ek),
        ]));
    }
    let Some(coid) = spec.oid() else {
        err::push(
            lib,
            r(cms, 144, 194),
            if cms {
                "unsupported content encryption algorithm"
            } else {
                "cipher has no object identifier"
            },
        );
        return None;
    };
    if aead {
        if !cms {
            err::push(lib, 116, "cipher not initialized");
            return None;
        }
        let op = cipher::Op {
            spec,
            key: &key,
            iv: &iv,
            enc: true,
            padding: true,
            aad: &[],
            tag_len: 16,
            tag: None,
        };
        let (ct, tag) = cipher::run(&op, &content).ok()?;
        let alg = asn1::algid(
            coid,
            Some(asn1::seq(&[asn1::octet(&iv), asn1::int_i64(16)])),
        );
        let eci = asn1::seq(&[asn1::oid(OID_DATA), alg, asn1::implicit(0, &ct)]);
        let aed = asn1::seq(&[
            asn1::int_i64(0),
            asn1::set_of(&ris),
            eci,
            asn1::octet(&tag[..16]),
        ]);
        return Some(asn1::seq(&[
            asn1::oid(OID_AUTH_ENVELOPED),
            asn1::explicit(0, &aed),
        ]));
    }
    let ct = cipher::simple(&spec.name, &content, &key, &iv, true)?;
    let alg = asn1::algid(coid, Some(cipher_params(spec, &iv)));
    let eci = asn1::seq(&[asn1::oid(OID_DATA), alg, asn1::implicit(0, &ct)]);
    let ed = asn1::seq(&[asn1::int_i64(0), asn1::set_of(&ris), eci]);
    Some(asn1::seq(&[
        asn1::oid(OID_ENVELOPED),
        asn1::explicit(0, &ed),
    ]))
}

fn cipher_by_oid(oid: &str, params: Option<&Tlv>) -> Option<(&'static CipherSpec, Vec<u8>)> {
    let name = match oid {
        "1.2.840.113549.3.2" => {
            let p = params?;
            if p.tag == asn1::SEQUENCE {
                let c = p.children()?;
                let ver = c.first()?.as_i64()?;
                let iv = c.get(1)?.content.to_vec();
                let n = match ver {
                    160 => "rc2-40-cbc",
                    120 => "rc2-64-cbc",
                    _ => "rc2-cbc",
                };
                return Some((cipher::by_name(n)?, iv));
            }
            "rc2-cbc"
        }
        o => o,
    };
    let spec = cipher::by_name(name).or_else(|| cipher::by_oid(name))?;
    let iv = match params {
        Some(p) if p.tag == asn1::OCTET_STRING => p.content.to_vec(),
        Some(p) if p.tag == asn1::SEQUENCE => p.children()?.first()?.content.to_vec(),
        _ => Vec::new(),
    };
    Some((spec, iv))
}

/// PKCS7_decrypt()/CMS_decrypt()
fn decrypt(der: &[u8], key: &PKey, cert: &Cert, cms: bool) -> Option<Vec<u8>> {
    let lib = if cms { err::LIB_CMS } else { err::LIB_PKCS7 };
    let (oid, inner) = content_info(der)?;
    let auth = oid == OID_AUTH_ENVELOPED;
    if oid != OID_ENVELOPED && !auth {
        err::push(
            lib,
            r(cms, 113, 146),
            if cms {
                "type not enveloped data"
            } else {
                "wrong content type"
            },
        );
        return None;
    }
    let inner = inner?;
    let c = inner.children()?;
    let mut i = 1;
    if c.get(i)?.context() == Some(0) {
        i += 1; // originatorInfo
    }
    let ris = c.get(i)?.children()?;
    let eci = c.get(i + 1)?.children()?;
    let PKey::Rsa(rsa) = key else {
        if cms {
            err::push(lib, 154, "unsupported recipient type");
        } else {
            // EVP_PKEY_decrypt_init() on a key that cannot decrypt
            err::push(err::LIB_EVP, 150, "operation not supported for this keytype");
            err::push(lib, 119, "decrypt error");
        }
        return None;
    };
    let mut cek: Option<Vec<u8>> = None;
    for ri in &ris {
        let f = ri.children()?;
        if f.first()?.tag != asn1::INTEGER {
            continue;
        }
        let Some(sid) = parse_sid(f.get(1)?) else {
            continue;
        };
        let matches = match &sid {
            Sid::IssuerSerial(iss, ser) => {
                let n = asn1::parse_one(iss).and_then(|t| crate::name::Name::parse(&t));
                n.map(|n| n.eq_canon(&cert.issuer)).unwrap_or(false) && *ser == cert.serial
            }
            Sid::Ski(s) => cert.skid().as_deref() == Some(s.as_slice()),
        };
        if !matches {
            continue;
        }
        let alg = f.get(2)?.children()?.first()?.oid()?;
        let ek = f.get(3)?.content;
        let pad = if alg == OID_RSA_OAEP {
            crate::pkey::RSA_PKCS1_OAEP_PADDING
        } else {
            RSA_PKCS1_PADDING
        };
        cek = rsa.decrypt(ek, pad, Md::Sha1);
        break;
    }
    let Some(cek) = cek else {
        err::push(
            lib,
            r(cms, 115, 132),
            if cms {
                "no matching recipient"
            } else {
                "no recipient matches certificate"
            },
        );
        return None;
    };
    let alg = eci.get(1)?.children()?;
    let coid = alg.first()?.oid()?;
    let enc = eci.get(2).map(|t| {
        if t.constructed() {
            octets_content(&asn1::Tlv { tag: 0x24, ..*t })
        } else {
            t.content.to_vec()
        }
    })?;
    if auth {
        let spec = cipher::by_oid(&coid)?;
        let gp = alg.get(1)?.children()?;
        let nonce = gp.first()?.content.to_vec();
        let mac = c.get(i + 2).and_then(|m| {
            if m.tag == asn1::OCTET_STRING {
                Some(m.content.to_vec())
            } else {
                c.get(i + 3).map(|x| x.content.to_vec())
            }
        })?;
        let op = cipher::Op {
            spec,
            key: &cek,
            iv: &nonce,
            enc: false,
            padding: true,
            aad: &[],
            tag_len: mac.len(),
            tag: Some(&mac),
        };
        return match cipher::run(&op, &enc) {
            Ok((pt, _)) => Some(pt),
            Err(_) => {
                err::push(lib, 155, "cipher final error");
                None
            }
        };
    }
    let (spec, iv) = cipher_by_oid(&coid, alg.get(1))?;
    let out = cipher::simple(&spec.name, &enc, &cek, &iv, false);
    if out.is_none() {
        err::push(lib, r(cms, 119, 112), "decrypt error");
    }
    out
}

// ---------------------------------------------------------------------------
// php-src functions
// ---------------------------------------------------------------------------

/// php_openssl_bio_new_file() for reading.
fn read_file(
    vm: &mut Vm,
    f: &str,
    path: &[u8],
    arg_num: usize,
) -> Result<Option<Vec<u8>>, VmError> {
    let Some(p) = x509::check_plain_path(vm, f, arg_num, path, false, None)? else {
        return Ok(None);
    };
    match std::fs::read(&p) {
        Ok(d) => Ok(Some(d)),
        Err(e) => {
            err::bio_new_file_failed(&e);
            Ok(None)
        }
    }
}

/// php_openssl_bio_new_file() for writing (opens/truncates).
fn open_out(vm: &mut Vm, f: &str, path: &[u8], arg_num: usize) -> Result<Option<String>, VmError> {
    let Some(p) = x509::check_plain_path(vm, f, arg_num, path, false, None)? else {
        return Ok(None);
    };
    match std::fs::File::create(&p) {
        Ok(_) => Ok(Some(p)),
        Err(e) => {
            err::bio_new_file_failed(&e);
            Ok(None)
        }
    }
}

fn headers_text(vm: &mut Vm, headers: Option<&PhpArray>) -> Result<Vec<u8>, VmError> {
    let mut out = Vec::new();
    if let Some(h) = headers {
        let items: Vec<(ArrayKey, Value)> = h.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (k, v) in items {
            let s = vm.value_to_string(&v.deref());
            if vm.current_exception.is_some() {
                return Err(VmError {
                    message: "exception".into(),
                    line: vm.current_line,
                });
            }
            match k {
                ArrayKey::String(ks) => {
                    out.extend_from_slice(ks.as_bytes());
                    out.extend_from_slice(b": ");
                    out.extend_from_slice(s.as_bytes());
                    out.push(b'\n');
                }
                _ => {
                    out.extend_from_slice(s.as_bytes());
                    out.push(b'\n');
                }
            }
        }
    }
    Ok(out)
}

/// Recipient certificates (array or single value).
fn recip_certs(vm: &mut Vm, f: &str, v: &Value) -> Result<Option<Vec<Rc<Cert>>>, VmError> {
    let mut out = Vec::new();
    if let Value::Array(a) = v.deref() {
        let items: Vec<Value> = a.borrow().iter().map(|(_, v)| v.clone()).collect();
        for it in items {
            match x509::cert_from_zval(vm, f, 3, &it, true)? {
                Some(c) => out.push(c),
                None => return Ok(None),
            }
        }
    } else {
        match x509::cert_from_zval(vm, f, 3, v, false)? {
            Some(c) => out.push(c),
            None => return Ok(None),
        }
    }
    Ok(Some(out))
}

fn signer_pems(signers: &[Rc<Cert>]) -> String {
    signers.iter().map(|c| c.pem()).collect()
}

fn pem_label(cms: bool) -> &'static str {
    if cms { "CMS" } else { "PKCS7" }
}

fn cert_list(certs: &[Vec<u8>], crls: &[Vec<u8>]) -> PhpArray {
    let mut arr = PhpArray::new();
    for (i, c) in certs.iter().enumerate() {
        arr.set(
            ArrayKey::Int(i as i64),
            Value::String(PhpString::from_string(asn1::pem_encode("CERTIFICATE", c))),
        );
    }
    for (i, c) in crls.iter().enumerate() {
        arr.set(
            ArrayKey::Int(i as i64),
            Value::String(PhpString::from_string(asn1::pem_encode("X509 CRL", c))),
        );
    }
    arr
}

fn ci_signed(der: &[u8]) -> Option<SignedData> {
    let (oid, inner) = content_info(der)?;
    if oid != OID_SIGNED && oid != OID_SIGNED_ENVELOPED {
        return None;
    }
    parse_signed(&inner?)
}

pub fn openssl_pkcs7_verify(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_pkcs7_verify";
    let filename = zpp::path(vm, args, 0, f, "input_filename")?;
    let mut flags = zpp::long(vm, args, 1, f, "flags")?;
    let signers_file = path_nullable(vm, args, 2, f, "signers_certificates_filename")?;
    let cainfo = if zpp::is_passed(args, 3) {
        Some(zpp::array(vm, args, 3, f, "ca_info")?.borrow().clone())
    } else {
        None
    };
    let extracerts = path_nullable(vm, args, 4, f, "untrusted_certificates_filename")?;
    let content_file = path_nullable(vm, args, 5, f, "content")?;
    let p7b_file = path_nullable(vm, args, 6, f, "output_filename")?;
    let mut others = Vec::new();
    if let Some(e) = &extracerts {
        match x509::load_all_certs_from_file(vm, f, e.as_bytes(), 5)? {
            Some(c) => others = c,
            None => return Ok(Value::Long(-1)),
        }
    }
    flags &= !F_DETACHED;
    let Some(store) = x509::setup_verify(vm, f, cainfo.as_ref(), 4)? else {
        return Ok(Value::Long(-1));
    };
    let Some(input) = read_file(vm, f, filename.as_bytes(), 1)? else {
        return Ok(Value::Long(-1));
    };
    let Some((der, datain)) = smime_read(&input) else {
        return Ok(Value::Long(-1));
    };
    let Some(sd) = ci_signed(&der) else {
        err::push(err::LIB_PKCS7, 113, "wrong content type");
        return Ok(Value::False);
    };
    let mut dataout = None;
    if let Some(c) = &content_file {
        match open_out(vm, f, c.as_bytes(), 6)? {
            Some(p) => dataout = Some(p),
            None => return Ok(Value::Long(-1)),
        }
    }
    let mut p7bout = None;
    if let Some(c) = &p7b_file {
        match open_out(vm, f, c.as_bytes(), 7)? {
            Some(p) => p7bout = Some(p),
            None => return Ok(Value::Long(-1)),
        }
    }
    let mut written = None;
    let res = verify(&sd, &others, &store, datain.as_deref(), flags, false, &mut written);
    if res.is_none() {
        // the content went out before the signatures were checked
        if let (Some(p), Some(w)) = (&dataout, &written) {
            let _ = std::fs::write(p, w);
        }
    }
    match res {
        Some((content, signers)) => {
            let mut ret = Value::True;
            if let Some(p) = &dataout {
                let _ = std::fs::write(p, &content);
            }
            if let Some(sf) = &signers_file {
                match open_out(vm, f, sf.as_bytes(), 3)? {
                    Some(p) => {
                        let _ = std::fs::write(p, signer_pems(&signers));
                    }
                    None => {
                        vm.emit_warning(&format!(
                            "{}(): Signature OK, but cannot open {} for writing",
                            f,
                            sf.to_string_lossy()
                        ));
                        ret = Value::Long(-1);
                    }
                }
                if let Some(p) = &p7bout {
                    let _ = std::fs::write(p, asn1::pem_encode("PKCS7", &der));
                }
            }
            Ok(ret)
        }
        None => Ok(Value::False),
    }
}

fn path_nullable(
    vm: &mut Vm,
    args: &[Value],
    idx: usize,
    f: &str,
    pname: &str,
) -> Result<Option<PhpString>, VmError> {
    match zpp::str_nullable(vm, args, idx, f, pname)? {
        Some(s) => {
            if s.as_bytes().contains(&0) {
                return Err(zpp::arg_value_error(
                    vm,
                    f,
                    idx + 1,
                    pname,
                    "must not contain any null bytes",
                ));
            }
            Ok(Some(s))
        }
        None => Ok(None),
    }
}

fn cipher_arg(
    vm: &mut Vm,
    args: &[Value],
    idx: usize,
    f: &str,
    allow_str: bool,
) -> Result<Option<&'static CipherSpec>, VmError> {
    match zpp::arg(args, idx) {
        None => Ok(crate::req::cipher_from_algo(5)),
        Some(Value::String(s)) if allow_str => Ok(cipher::by_name(&s.to_string_lossy())),
        Some(_) => {
            let id = zpp::long(vm, args, idx, f, "cipher_algo")?;
            Ok(crate::req::cipher_from_algo(id))
        }
    }
}

pub fn openssl_pkcs7_encrypt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_pkcs7_encrypt";
    let infile = zpp::path(vm, args, 0, f, "input_filename")?;
    let outfile = zpp::path(vm, args, 1, f, "output_filename")?;
    let certv = args.get(2).cloned().unwrap_or(Value::Null);
    let headers = zpp::array_nullable(vm, args, 3, f, "headers")?.map(|a| a.borrow().clone());
    let flags = if zpp::is_passed(args, 4) {
        zpp::long(vm, args, 4, f, "flags")?
    } else {
        0
    };
    let cipher = cipher_arg(vm, args, 5, f, false)?;
    let Some(input) = read_file(vm, f, infile.as_bytes(), 1)? else {
        return Ok(Value::False);
    };
    let Some(out_path) = open_out(vm, f, outfile.as_bytes(), 2)? else {
        return Ok(Value::False);
    };
    let Some(certs) = recip_certs(vm, f, &certv)? else {
        return Ok(Value::False);
    };
    let Some(spec) = cipher else {
        vm.emit_warning(&format!("{}(): Failed to get cipher", f));
        return Ok(Value::False);
    };
    let Some(ci) = encrypt(&certs, &input, spec, flags, false) else {
        return Ok(Value::False);
    };
    let mut out = headers_text(vm, headers.as_ref())?;
    out.extend(smime_write(
        &ci,
        OID_ENVELOPED,
        None,
        &[],
        flags,
        flags & F_OLDMIME == 0,
    ));
    Ok(if std::fs::write(&out_path, out).is_ok() {
        Value::True
    } else {
        Value::False
    })
}

pub fn openssl_pkcs7_read(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_pkcs7_read";
    let data = zpp::str(vm, args, 0, f, "data")?;
    if data.is_empty() {
        // BIO_write() of nothing fails before any parsing
        return Ok(Value::False);
    }
    let Some(block) = asn1::pem_find(data.as_bytes(), &["PKCS7", "PKCS #7 SIGNED DATA"]) else {
        err::pem_no_start_line();
        return Ok(Value::False);
    };
    let Some((oid, inner)) = content_info(&block.der) else {
        return Ok(Value::False);
    };
    let arr = if oid == OID_SIGNED || oid == OID_SIGNED_ENVELOPED {
        match inner.and_then(|t| parse_signed(&t)) {
            Some(sd) => cert_list(&sd.certs, &sd.crls),
            None => PhpArray::new(),
        }
    } else {
        PhpArray::new()
    };
    zpp::set_ref(args, 1, Value::Array(Rc::new(RefCell::new(arr))));
    Ok(Value::True)
}

fn sign_common(vm: &mut Vm, args: &[Value], f: &str, cms: bool) -> Result<Value, VmError> {
    let infile = zpp::path(vm, args, 0, f, "input_filename")?;
    let outfile = zpp::path(vm, args, 1, f, "output_filename")?;
    let certv = args.get(2).cloned().unwrap_or(Value::Null);
    if !matches!(certv.deref(), Value::Object(_)) || !x509_is_cert(&certv) {
        // Z_PARAM_OBJ_OF_CLASS_OR_STR
        if let Value::Object(_) | Value::Array(_) = certv.deref() {
            if !has_tostring(vm, &certv) {
                return Err(zpp::arg_type_error(
                    vm,
                    f,
                    3,
                    "certificate",
                    "OpenSSLCertificate|string",
                    &certv.deref(),
                ));
            }
        }
    }
    let keyv = args.get(3).cloned().unwrap_or(Value::Null);
    let headers = zpp::array_nullable(vm, args, 4, f, "headers")?.map(|a| a.borrow().clone());
    let default_flags = if cms { 0 } else { F_DETACHED };
    let flags = if zpp::is_passed(args, 5) {
        zpp::long(vm, args, 5, f, "flags")?
    } else {
        default_flags
    };
    let (encoding, extra_idx) = if cms {
        (
            if zpp::is_passed(args, 6) {
                zpp::long(vm, args, 6, f, "encoding")?
            } else {
                ENCODING_SMIME
            },
            7,
        )
    } else {
        (ENCODING_SMIME, 6)
    };
    let extracerts = path_nullable(vm, args, extra_idx, f, "untrusted_certificates_filename")?;
    let mut others = Vec::new();
    if let Some(e) = &extracerts {
        match x509::load_all_certs_from_file(vm, f, e.as_bytes(), extra_idx + 1)? {
            Some(c) => others = c,
            None => return Ok(Value::False),
        }
    }
    let Some(key) = pkey_fns::from_zval(vm, f, &keyv, false, Some(&[]), 4)? else {
        if vm.current_exception.is_none() {
            vm.emit_warning(&format!("{}(): Error getting private key", f));
        }
        return Ok(Value::False);
    };
    let r = x509::cert_from_zval(vm, f, 3, &certv, false);
    let Some(cert) = x509::cert_failed(vm, f, r)? else {
        vm.emit_warning(&format!("{}(): X.509 Certificate cannot be retrieved", f));
        return Ok(Value::False);
    };
    if cms && (encoding & ENCODING_SMIME) != 0 && (flags & F_DETACHED) != 0 {
        vm.emit_warning(&format!(
            "{}(): Detached signatures not possible with S/MIME encoding",
            f
        ));
        return Ok(Value::False);
    }
    let Some(input) = read_file(vm, f, infile.as_bytes(), 1)? else {
        vm.emit_warning(&format!(
            "{}(): Error opening input file {}!",
            f,
            infile.to_string_lossy()
        ));
        return Ok(Value::False);
    };
    let Some(out_path) = open_out(vm, f, outfile.as_bytes(), 2)? else {
        vm.emit_warning(&format!(
            "{}(): Error opening output file {}!",
            f,
            outfile.to_string_lossy()
        ));
        return Ok(Value::False);
    };
    // PKCS7_sign() always produces a detachable structure; SMIME output of
    // a detached signature embeds the content as the first MIME part.
    // PKCS7_add_signature()/CMS_add1_signer() check the key first.
    let key_ok = x509::check_private_key(&cert.spki, &key);
    if !key_ok {
        if cms {
            err::push(err::LIB_CMS, 136, "private key does not match certificate");
            err::push(err::LIB_CMS, 99, "add signer error");
        } else {
            err::push(err::LIB_PKCS7, 127, "private key does not match certificate");
            err::push(err::LIB_PKCS7, 153, "pkcs7 add signer error");
        }
    }
    let signed = if key_ok { sign(&cert, &key, &others, &input, flags) } else { None };
    let Some((ci, md)) = signed else {
        let kind = if cms { "CMS" } else { "PKCS7" };
        vm.emit_warning(&format!("{}(): Error creating {} structure!", f, kind));
        return Ok(Value::False);
    };
    let mut out = if encoding == ENCODING_SMIME {
        headers_text(vm, headers.as_ref())?
    } else {
        Vec::new()
    };
    match encoding {
        ENCODING_SMIME => {
            let oldmime = if cms {
                flags & F_OLDMIME != 0
            } else {
                flags & F_OLDMIME == 0
            };
            let detached = if flags & F_DETACHED != 0 {
                Some(input.as_slice())
            } else {
                None
            };
            out.extend(smime_write(
                &ci,
                OID_SIGNED,
                detached,
                &[md],
                flags,
                oldmime,
            ));
        }
        ENCODING_DER => out.extend(ci),
        ENCODING_PEM => out.extend(asn1::pem_encode(pem_label(cms), &ci).into_bytes()),
        _ => {
            vm.emit_warning(&format!("{}(): Unknown OPENSSL encoding", f));
            return Ok(Value::False);
        }
    }
    Ok(if std::fs::write(&out_path, out).is_ok() {
        Value::True
    } else {
        Value::False
    })
}

fn x509_is_cert(v: &Value) -> bool {
    match v.deref() {
        Value::Object(o) => o
            .borrow()
            .class_name
            .eq_ignore_ascii_case(b"OpenSSLCertificate"),
        _ => false,
    }
}

fn has_tostring(vm: &mut Vm, v: &Value) -> bool {
    match v.deref() {
        Value::Object(o) => {
            let lc = o.borrow().class_name.to_ascii_lowercase();
            vm.get_class_def(&lc)
                .map(|c| c.get_method(b"__tostring").is_some())
                .unwrap_or(false)
        }
        _ => false,
    }
}

pub fn openssl_pkcs7_sign(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    sign_common(vm, args, "openssl_pkcs7_sign", false)
}

pub fn openssl_cms_sign(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    sign_common(vm, args, "openssl_cms_sign", true)
}

fn decrypt_common(vm: &mut Vm, args: &[Value], f: &str, cms: bool) -> Result<Value, VmError> {
    let infile = zpp::path(vm, args, 0, f, "input_filename")?;
    let outfile = zpp::path(vm, args, 1, f, "output_filename")?;
    let certv = args.get(2).cloned().unwrap_or(Value::Null);
    let keyv = match zpp::arg(args, 3) {
        None | Some(Value::Null) => None,
        Some(v) => Some(v),
    };
    let encoding = if cms && zpp::is_passed(args, 4) {
        zpp::long(vm, args, 4, f, "encoding")?
    } else {
        ENCODING_SMIME
    };
    let r = x509::cert_from_zval(vm, f, 3, &certv, false);
    let Some(cert) = x509::cert_failed(vm, f, r)? else {
        vm.emit_warning(&format!("{}(): X.509 Certificate cannot be retrieved", f));
        return Ok(Value::False);
    };
    let key_arg_num = if keyv.is_some() || !cms { 4 } else { 3 };
    let kv = keyv.clone().unwrap_or_else(|| certv.clone());
    let Some(key) = pkey_fns::from_zval(vm, f, &kv, false, Some(&[]), key_arg_num)? else {
        if vm.current_exception.is_none() {
            vm.emit_warning(&format!("{}(): Unable to get private key", f));
        }
        return Ok(Value::False);
    };
    let Some(input) = read_file(vm, f, infile.as_bytes(), 1)? else {
        return Ok(Value::False);
    };
    let Some(out_path) = open_out(vm, f, outfile.as_bytes(), 2)? else {
        return Ok(Value::False);
    };
    let der = match encoding {
        ENCODING_DER => Some(input),
        ENCODING_PEM => asn1::pem_find(&input, &["CMS", "PKCS7"]).map(|b| b.der),
        ENCODING_SMIME => smime_read(&input).map(|(d, _)| d),
        _ => {
            return Err(zpp::arg_value_error(
                vm,
                f,
                5,
                "encoding",
                "must be an OPENSSL_ENCODING_* constant",
            ));
        }
    };
    let Some(der) = der else {
        return Ok(Value::False);
    };
    match decrypt(&der, &key, &cert, cms) {
        Some(pt) => Ok(if std::fs::write(&out_path, pt).is_ok() {
            Value::True
        } else {
            Value::False
        }),
        None => Ok(Value::False),
    }
}

pub fn openssl_pkcs7_decrypt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    decrypt_common(vm, args, "openssl_pkcs7_decrypt", false)
}

pub fn openssl_cms_decrypt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    decrypt_common(vm, args, "openssl_cms_decrypt", true)
}

pub fn openssl_cms_encrypt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_cms_encrypt";
    let infile = zpp::path(vm, args, 0, f, "input_filename")?;
    let outfile = zpp::path(vm, args, 1, f, "output_filename")?;
    let certv = args.get(2).cloned().unwrap_or(Value::Null);
    let headers = zpp::array_nullable(vm, args, 3, f, "headers")?.map(|a| a.borrow().clone());
    let flags = if zpp::is_passed(args, 4) {
        zpp::long(vm, args, 4, f, "flags")?
    } else {
        0
    };
    let encoding = if zpp::is_passed(args, 5) {
        zpp::long(vm, args, 5, f, "encoding")?
    } else {
        ENCODING_SMIME
    };
    let cipher = cipher_arg(vm, args, 6, f, true)?;
    let Some(input) = read_file(vm, f, infile.as_bytes(), 1)? else {
        return Ok(Value::False);
    };
    let Some(out_path) = open_out(vm, f, outfile.as_bytes(), 2)? else {
        return Ok(Value::False);
    };
    let Some(certs) = recip_certs(vm, f, &certv)? else {
        return Ok(Value::False);
    };
    let Some(spec) = cipher else {
        vm.emit_warning(&format!("{}(): Failed to get cipher", f));
        return Ok(Value::False);
    };
    let Some(ci) = encrypt(&certs, &input, spec, flags, true) else {
        return Ok(Value::False);
    };
    let ctype = content_info(&ci).map(|(o, _)| o).unwrap_or_default();
    let mut out = if encoding == ENCODING_SMIME {
        headers_text(vm, headers.as_ref())?
    } else {
        Vec::new()
    };
    match encoding {
        ENCODING_SMIME => out.extend(smime_write(
            &ci,
            &ctype,
            None,
            &[],
            flags,
            flags & F_OLDMIME != 0,
        )),
        ENCODING_DER => out.extend(ci),
        ENCODING_PEM => out.extend(asn1::pem_encode("CMS", &ci).into_bytes()),
        _ => {
            vm.emit_warning(&format!("{}(): Unknown OPENSSL encoding", f));
            return Ok(Value::False);
        }
    }
    Ok(if std::fs::write(&out_path, out).is_ok() {
        Value::True
    } else {
        Value::False
    })
}

pub fn openssl_cms_read(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_cms_read";
    let data = zpp::str(vm, args, 0, f, "input_filename")?;
    let Some(block) = asn1::pem_find(data.as_bytes(), &["CMS", "PKCS7"]) else {
        err::pem_no_start_line();
        return Ok(Value::False);
    };
    let arr = match ci_signed(&block.der) {
        Some(sd) => cert_list(&sd.certs, &sd.crls),
        None => PhpArray::new(),
    };
    zpp::set_ref(args, 1, Value::Array(Rc::new(RefCell::new(arr))));
    Ok(Value::True)
}

pub fn openssl_cms_verify(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_cms_verify";
    let filename = zpp::path(vm, args, 0, f, "input_filename")?;
    let flags = zpp::long(vm, args, 1, f, "flags")?;
    let signers_file = path_nullable(vm, args, 2, f, "certificates")?;
    let cainfo = if zpp::is_passed(args, 3) {
        Some(zpp::array(vm, args, 3, f, "ca_info")?.borrow().clone())
    } else {
        None
    };
    let extracerts = path_nullable(vm, args, 4, f, "untrusted_certificates_filename")?;
    let content_file = path_nullable(vm, args, 5, f, "content")?;
    let p7b_file = path_nullable(vm, args, 6, f, "pk7")?;
    let sigfile = path_nullable(vm, args, 7, f, "sigfile")?;
    let encoding = if zpp::is_passed(args, 8) {
        zpp::long(vm, args, 8, f, "encoding")?
    } else {
        ENCODING_SMIME
    };
    let Some(input) = read_file(vm, f, filename.as_bytes(), 1)? else {
        return Ok(Value::False);
    };
    let sig_input = match &sigfile {
        Some(s) if flags & F_DETACHED != 0 => {
            if encoding == ENCODING_SMIME {
                vm.emit_warning(&format!(
                    "{}(): Detached signatures not possible with S/MIME encoding",
                    f
                ));
                return Ok(Value::False);
            }
            match read_file(vm, f, s.as_bytes(), 1)? {
                Some(d) => d,
                None => return Ok(Value::False),
            }
        }
        _ => input.clone(),
    };
    let (der, mut datain) = match encoding {
        ENCODING_PEM => match asn1::pem_find(&sig_input, &["CMS", "PKCS7"]) {
            Some(b) => (b.der, Some(input.clone())),
            None => {
                err::pem_no_start_line();
                return Ok(Value::False);
            }
        },
        ENCODING_DER => (sig_input, Some(input.clone())),
        ENCODING_SMIME => match smime_read(&sig_input) {
            Some((d, c)) => (d, c),
            None => return Ok(Value::False),
        },
        _ => {
            vm.emit_warning(&format!("{}(): Unknown encoding", f));
            return Ok(Value::False);
        }
    };
    if encoding != ENCODING_SMIME && flags & F_DETACHED == 0 {
        datain = None;
    }
    let mut others = Vec::new();
    if let Some(e) = &extracerts {
        match x509::load_all_certs_from_file(vm, f, e.as_bytes(), 5)? {
            Some(c) => others = c,
            None => return Ok(Value::False),
        }
    }
    let Some(store) = x509::setup_verify(vm, f, cainfo.as_ref(), 4)? else {
        return Ok(Value::False);
    };
    let mut dataout = None;
    if let Some(c) = &content_file {
        match open_out(vm, f, c.as_bytes(), 6)? {
            Some(p) => dataout = Some(p),
            None => return Ok(Value::False),
        }
    }
    let mut p7bout = None;
    if let Some(c) = &p7b_file {
        match open_out(vm, f, c.as_bytes(), 7)? {
            Some(p) => p7bout = Some(p),
            None => return Ok(Value::False),
        }
    }
    let Some(sd) = ci_signed(&der) else {
        err::push(err::LIB_CMS, 108, "content type not signed data");
        return Ok(Value::False);
    };
    match verify(&sd, &others, &store, datain.as_deref(), flags, true, &mut None) {
        Some((content, signers)) => {
            let mut ret = Value::True;
            if let Some(p) = &dataout {
                let _ = std::fs::write(p, &content);
            }
            if let Some(sf) = &signers_file {
                match open_out(vm, f, sf.as_bytes(), 3)? {
                    Some(p) => {
                        let _ = std::fs::write(p, signer_pems(&signers));
                    }
                    None => {
                        vm.emit_warning(&format!(
                            "{}(): Signature OK, but cannot open {} for writing",
                            f,
                            sf.to_string_lossy()
                        ));
                        ret = Value::False;
                    }
                }
                if let Some(p) = &p7bout {
                    let _ = std::fs::write(p, asn1::pem_encode("CMS", &der));
                }
            }
            Ok(ret)
        }
        None => Ok(Value::False),
    }
}
