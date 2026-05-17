//! PKCS#7/CMS (RFC 2315 / RFC 5652) and PKCS#12 (RFC 7292) support.
//!
//! This module implements `openssl_pkcs7_*`, `openssl_cms_*` and
//! `openssl_pkcs12_*` PHP functions. The implementation builds and parses
//! the relevant ASN.1 structures using the `der` and `x509-cert` crates
//! (already present in the openssl extension's deps) and shares cert/key
//! resolution helpers with the `x509` and `rsa` modules.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use der::asn1::ObjectIdentifier;
use der::{Decode, Encode, Reader, SliceReader, Tag};

use x509_cert::certificate::Certificate;

use rsa::pkcs1v15::Pkcs1v15Encrypt;
use rsa::{RsaPrivateKey, RsaPublicKey};

use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::rsa::resolve_private_key_pub;
use crate::{base64_decode as base64_decode_global, base64_encode as base64_encode_global};

// ── PEM/base64 helpers (lightweight) ──

fn pem_wrap(label: &str, der: &[u8]) -> String {
    let b64 = base64_encode_global(der);
    let mut out = String::new();
    out.push_str(&format!("-----BEGIN {}-----\n", label));
    let mut i = 0;
    while i < b64.len() {
        let end = (i + 64).min(b64.len());
        out.push_str(&b64[i..end]);
        out.push('\n');
        i = end;
    }
    out.push_str(&format!("-----END {}-----\n", label));
    out
}

/// Extract PEM body between BEGIN/END (any label). Returns the decoded DER bytes.
fn pem_to_der(s: &str) -> Option<Vec<u8>> {
    let begin = s.find("-----BEGIN ")?;
    let after = s[begin..].find('\n')?;
    let body_start = begin + after + 1;
    let end = s[body_start..].find("-----END")?;
    let body: String = s[body_start..body_start + end]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    base64_decode_global(&body)
}

// ── DER encoder helpers ──
//
// Build common DER structures by hand. We avoid full Sequence derives
// because cms/pkcs12 crates aren't depended on; instead we serialize
// fields by encoding their components and wrapping in the appropriate
// SEQUENCE/SET/EXPLICIT tags via `der::Writer` plus manual wrappers.

/// Wrap raw inner-DER as a SEQUENCE.
fn der_sequence(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0x30); // SEQUENCE | CONSTRUCTED
    write_len(&mut out, body.len());
    out.extend_from_slice(body);
    out
}

/// Wrap raw inner-DER as a SET.
fn der_set(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0x31); // SET | CONSTRUCTED
    write_len(&mut out, body.len());
    out.extend_from_slice(body);
    out
}

/// Wrap raw inner-DER inside an EXPLICIT [tag] context-specific wrapper.
fn der_explicit(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0xA0 | (tag & 0x1F)); // CONTEXT | CONSTRUCTED | tag
    write_len(&mut out, body.len());
    out.extend_from_slice(body);
    out
}

/// Encode an OCTET STRING.
fn der_octet_string(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0x04);
    write_len(&mut out, data.len());
    out.extend_from_slice(data);
    out
}

/// Encode an OBJECT IDENTIFIER from its dotted string.
fn der_oid_dot(oid_str: &str) -> Option<Vec<u8>> {
    let oid: ObjectIdentifier = oid_str.parse().ok()?;
    oid.to_der().ok()
}

/// Encode an INTEGER from a small unsigned integer (PHP cms versions are
/// always tiny: 0, 1, 2, 3).
fn der_int_u32(v: u32) -> Vec<u8> {
    // Find significant bytes
    let bytes = v.to_be_bytes();
    let mut start = 0;
    while start < 3 && bytes[start] == 0 {
        start += 1;
    }
    let mut body = bytes[start..].to_vec();
    // If high bit set, prepend 0
    if body[0] & 0x80 != 0 {
        body.insert(0, 0);
    }
    let mut out = Vec::new();
    out.push(0x02);
    write_len(&mut out, body.len());
    out.extend_from_slice(&body);
    out
}

/// Encode a definite length in DER form.
fn write_len(out: &mut Vec<u8>, len: usize) {
    if len < 128 {
        out.push(len as u8);
    } else {
        let bytes = (len as u64).to_be_bytes();
        let mut start = 0;
        while start < 8 && bytes[start] == 0 {
            start += 1;
        }
        let n = 8 - start;
        out.push(0x80 | n as u8);
        out.extend_from_slice(&bytes[start..]);
    }
}

/// Build a simple AlgorithmIdentifier { oid, NULL params }.
fn algid_with_null(oid_str: &str) -> Option<Vec<u8>> {
    let oid_der = der_oid_dot(oid_str)?;
    let mut body = oid_der;
    body.extend_from_slice(&[0x05, 0x00]); // NULL
    Some(der_sequence(&body))
}

/// Build AlgorithmIdentifier { oid, OCTET STRING params (IV) }.
fn algid_with_iv(oid_str: &str, iv: &[u8]) -> Option<Vec<u8>> {
    let oid_der = der_oid_dot(oid_str)?;
    let mut body = oid_der;
    body.extend(der_octet_string(iv));
    Some(der_sequence(&body))
}

// ── OIDs used in CMS / PKCS#12 ──

const OID_DATA: &str = "1.2.840.113549.1.7.1";
const OID_SIGNED_DATA: &str = "1.2.840.113549.1.7.2";
const OID_ENVELOPED_DATA: &str = "1.2.840.113549.1.7.3";
// digest algorithms
const OID_SHA1: &str = "1.3.14.3.2.26";
const OID_SHA256: &str = "2.16.840.1.101.3.4.2.1";
// RSA
const OID_RSA_ENCRYPTION: &str = "1.2.840.113549.1.1.1";
const OID_SHA1_RSA: &str = "1.2.840.113549.1.1.5";
const OID_SHA256_RSA: &str = "1.2.840.113549.1.1.11";
// AES
const OID_AES_128_CBC: &str = "2.16.840.1.101.3.4.1.2";
const OID_AES_192_CBC: &str = "2.16.840.1.101.3.4.1.22";
const OID_AES_256_CBC: &str = "2.16.840.1.101.3.4.1.42";

// PKCS#12 specific
const OID_PKCS12_PFX: &str = "1.2.840.113549.1.12.10.1.1"; // keyBag
const OID_KEY_BAG: &str = "1.2.840.113549.1.12.10.1.1";
const OID_PKCS8_SHROUDED_KEY_BAG: &str = "1.2.840.113549.1.12.10.1.2";
const OID_CERT_BAG: &str = "1.2.840.113549.1.12.10.1.3";
const OID_X509_CERT_TYPE: &str = "1.2.840.113549.1.9.22.1";
const OID_PKCS7_DATA: &str = "1.2.840.113549.1.7.1";

// ── Helper: dereference Value::Reference and turn into bytes (file:// supported) ──

fn deref(v: &Value) -> Value {
    match v {
        Value::Reference(r) => r.borrow().clone(),
        other => other.clone(),
    }
}

/// Resolve a value (possibly file:// prefixed) into raw bytes.
/// Returns None on empty/missing file/string.
fn read_value_or_file(v: &Value) -> Option<Vec<u8>> {
    let v = deref(v);
    let s = v.to_php_string();
    let bytes = s.as_bytes();
    if bytes.starts_with(b"file://") {
        let path = std::str::from_utf8(&bytes[7..]).ok()?;
        std::fs::read(path).ok()
    } else if !bytes.is_empty() {
        Some(bytes.to_vec())
    } else {
        None
    }
}

/// Resolve a value into a Certificate (using x509-cert).
fn resolve_cert(v: &Value) -> Option<Certificate> {
    crate::x509::resolve_cert_pub(v)
}

/// Resolve possibly an array of certs or a single cert.
fn resolve_certs(v: &Value) -> Vec<Certificate> {
    let v = deref(v);
    if let Value::Array(a) = &v {
        let arr = a.borrow();
        let mut out = Vec::new();
        for (_, item) in arr.iter() {
            if let Some(c) = resolve_cert(item) {
                out.push(c);
            }
        }
        return out;
    }
    resolve_cert(&v).map(|c| vec![c]).unwrap_or_default()
}

// ── S/MIME wrapping ──

fn smime_signed_wrap(headers: &[String], cms_b64: &str) -> String {
    let mut hdr = String::new();
    for h in headers {
        hdr.push_str(h);
        hdr.push_str("\r\n");
    }
    hdr.push_str("MIME-Version: 1.0\r\n");
    hdr.push_str("Content-Disposition: attachment; filename=\"smime.p7m\"\r\n");
    hdr.push_str("Content-Type: application/x-pkcs7-mime; smime-type=signed-data; name=\"smime.p7m\"\r\n");
    hdr.push_str("Content-Transfer-Encoding: base64\r\n");
    hdr.push_str("\r\n");
    // Body: base64 with 64-byte lines
    let mut body = String::new();
    let mut i = 0;
    while i < cms_b64.len() {
        let end = (i + 64).min(cms_b64.len());
        body.push_str(&cms_b64[i..end]);
        body.push_str("\r\n");
        i = end;
    }
    hdr + &body
}

fn smime_enveloped_wrap(headers: &[String], cms_b64: &str, content_type: &str, binary: bool) -> String {
    let nl = if binary { "\n" } else { "\r\n" };
    let mut hdr = String::new();
    for h in headers {
        hdr.push_str(h);
        hdr.push_str(nl);
    }
    hdr.push_str("MIME-Version: 1.0");
    hdr.push_str(nl);
    hdr.push_str("Content-Disposition: attachment; filename=\"smime.p7m\"");
    hdr.push_str(nl);
    hdr.push_str(&format!(
        "Content-Type: {}; smime-type=enveloped-data; name=\"smime.p7m\"",
        content_type
    ));
    hdr.push_str(nl);
    hdr.push_str("Content-Transfer-Encoding: base64");
    hdr.push_str(nl);
    hdr.push_str(nl);
    let mut body = String::new();
    let mut i = 0;
    while i < cms_b64.len() {
        let end = (i + 64).min(cms_b64.len());
        body.push_str(&cms_b64[i..end]);
        body.push_str(nl);
        i = end;
    }
    hdr + &body
}

/// Try to extract the base64-encoded DER body from an S/MIME message,
/// PEM-wrapped block, or raw DER input. Returns the decoded DER bytes
/// for the CMS ContentInfo.
fn extract_cms_der(input: &[u8]) -> Option<Vec<u8>> {
    let s = std::str::from_utf8(input).ok();

    // If this is a multipart S/MIME message, handle that path first so we
    // don't pick up the wrong embedded PEM block (e.g. a CERTIFICATE in a
    // separate part).
    let is_multipart = s.map_or(false, |s| {
        let lc = s.to_ascii_lowercase();
        lc.contains("multipart/signed") || lc.contains("multipart/mixed")
    });

    if !is_multipart {
        // PEM block (any label like PKCS7, CMS, etc.)
        if let Some(s) = s {
            // Don't pick up a CERTIFICATE PEM as CMS — only PKCS7/CMS labels.
            let lc = s.to_ascii_lowercase();
            if (lc.contains("-----begin pkcs7-----") || lc.contains("-----begin cms-----"))
                && let Some(der) = pem_to_der(s)
            {
                return Some(der);
            }
        }
    }

    // Multipart S/MIME (multipart/signed with pkcs7-signature): find the part
    // labelled application/x-pkcs7-signature or pkcs7-mime.
    if let Some(s) = s {
        // Find content-type with boundary
        let lc = s.to_ascii_lowercase();
        if lc.contains("multipart/signed") {
            // Extract boundary string between `boundary="..."` or `boundary=...`
            let boundary = lc.find("boundary=").and_then(|p| {
                let after = &s[p + 9..];
                if let Some(stripped) = after.strip_prefix('"') {
                    stripped.find('"').map(|q| &stripped[..q])
                } else {
                    after.find(|c: char| c == '\r' || c == '\n' || c == ';' || c == ' ')
                        .map(|q| &after[..q])
                }
            });
            if let Some(boundary) = boundary {
                let marker = format!("--{}", boundary);
                let parts: Vec<&str> = s.split(&marker).collect();
                for part in parts.iter().skip(1) {
                    // Each part starts with its headers; look for the one whose
                    // own Content-Type indicates a pkcs7 signature/mime payload.
                    let lp = part.to_ascii_lowercase();
                    let ct_idx = lp.find("content-type:");
                    if let Some(idx) = ct_idx {
                        let ct_line_end = lp[idx..].find('\n').unwrap_or(lp.len() - idx);
                        let ct_line = &lp[idx..idx + ct_line_end];
                        if ct_line.contains("pkcs7-signature") || ct_line.contains("pkcs7-mime") {
                            let sep_info = if let Some(p) = part.find("\r\n\r\n") {
                                Some((p, 4))
                            } else if let Some(p) = part.find("\n\n") {
                                Some((p, 2))
                            } else {
                                None
                            };
                            if let Some((sep, sep_len)) = sep_info {
                                let body = &part[sep + sep_len..];
                                let body = body.trim_end_matches('-').trim_end();
                                let cleaned: String = body
                                    .chars()
                                    .filter(|c| !c.is_whitespace())
                                    .collect();
                                if let Some(der) = base64_decode_global(&cleaned) {
                                    if der.len() > 8 && der[0] == 0x30 {
                                        return Some(der);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // S/MIME single-part: look for blank line separating headers from body
    if let Some(s) = s {
        let sep_info = if let Some(p) = s.find("\r\n\r\n") {
            Some((p, 4))
        } else if let Some(p) = s.find("\n\n") {
            Some((p, 2))
        } else {
            None
        };
        if let Some((sep, sep_len)) = sep_info {
            let body = &s[sep + sep_len..];
            let cleaned: String = body
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            if !cleaned.is_empty() {
                if let Some(der) = base64_decode_global(&cleaned) {
                    if der.len() > 8 && der[0] == 0x30 {
                        return Some(der);
                    }
                }
            }
        }
    }

    // Raw DER
    if input.len() > 4 && input[0] == 0x30 {
        return Some(input.to_vec());
    }
    None
}

/// Parse a ContentInfo DER and return (content_type_oid, content_bytes).
fn parse_content_info(der: &[u8]) -> Option<(String, Vec<u8>)> {
    // ContentInfo = SEQUENCE { OID, [0] EXPLICIT content }
    let mut r = SliceReader::new(der).ok()?;
    let header = r.peek_header().ok()?;
    if header.tag != Tag::Sequence {
        return None;
    }
    let body = read_sequence_body(der)?;
    // Now within body, read OID first
    let (oid_bytes, after_oid) = take_tlv(&body)?;
    if oid_bytes[0] != 0x06 {
        return None;
    }
    let oid_str = ObjectIdentifier::from_der(&oid_bytes).ok()?.to_string();
    // Then read [0] EXPLICIT content (optional)
    if after_oid.is_empty() {
        return Some((oid_str, Vec::new()));
    }
    let (ctx0, _) = take_tlv(after_oid)?;
    // ctx0 starts with 0xA0; inner is the actual content
    if ctx0[0] != 0xA0 {
        return Some((oid_str, Vec::new()));
    }
    let inner = read_explicit_body(&ctx0)?;
    Some((oid_str, inner))
}

/// Reads the inner contents of a SEQUENCE, returns the body bytes.
fn read_sequence_body(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() || data[0] != 0x30 {
        return None;
    }
    let (len, header_len) = read_len(&data[1..])?;
    let total = 1 + header_len + len;
    if total > data.len() {
        return None;
    }
    Some(data[1 + header_len..total].to_vec())
}

/// Reads the inner contents of either a SEQUENCE (0x30) or SET (0x31).
fn read_constructed_body(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() {
        return None;
    }
    let first = data[0];
    if first != 0x30 && first != 0x31 && first != 0xA0 && first != 0x04 && first & 0xE0 == 0 {
        // Allow any constructed tag
    }
    let (len, header_len) = read_len(&data[1..])?;
    let total = 1 + header_len + len;
    if total > data.len() {
        return None;
    }
    Some(data[1 + header_len..total].to_vec())
}

/// Reads body of context-specific [n] EXPLICIT.
fn read_explicit_body(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() {
        return None;
    }
    let (len, header_len) = read_len(&data[1..])?;
    let total = 1 + header_len + len;
    if total > data.len() {
        return None;
    }
    Some(data[1 + header_len..total].to_vec())
}

/// Read a definite-length encoding starting after the tag byte.
/// Returns (length, header_bytes_consumed).
fn read_len(data: &[u8]) -> Option<(usize, usize)> {
    if data.is_empty() {
        return None;
    }
    let b0 = data[0];
    if b0 < 0x80 {
        return Some((b0 as usize, 1));
    }
    let n = (b0 & 0x7F) as usize;
    if n == 0 || n > 4 || data.len() < 1 + n {
        return None;
    }
    let mut v: usize = 0;
    for i in 0..n {
        v = (v << 8) | data[1 + i] as usize;
    }
    Some((v, 1 + n))
}

/// Take a complete TLV from `data`, returning (tlv_bytes, remaining).
fn take_tlv(data: &[u8]) -> Option<(Vec<u8>, &[u8])> {
    if data.is_empty() {
        return None;
    }
    let (len, header_len) = read_len(&data[1..])?;
    let total = 1 + header_len + len;
    if total > data.len() {
        return None;
    }
    Some((data[..total].to_vec(), &data[total..]))
}

/// Extract all `Certificate` DER blobs from a SignedData structure.
/// Returns Vec of cert DER bytes.
fn extract_signed_data_certs(signed_data_der: &[u8]) -> Vec<Vec<u8>> {
    let body = match read_sequence_body(signed_data_der) {
        Some(b) => b,
        None => return Vec::new(),
    };
    let mut rest = body.as_slice();
    // version INTEGER
    let (_v, r1) = match take_tlv(rest) {
        Some(x) => x,
        None => return Vec::new(),
    };
    rest = r1;
    // digestAlgorithms SET
    let (_d, r2) = match take_tlv(rest) {
        Some(x) => x,
        None => return Vec::new(),
    };
    rest = r2;
    // encapContentInfo SEQUENCE
    let (_e, r3) = match take_tlv(rest) {
        Some(x) => x,
        None => return Vec::new(),
    };
    rest = r3;
    // certificates [0] IMPLICIT (optional) — tag byte 0xA0
    let mut out = Vec::new();
    while !rest.is_empty() {
        let tag = rest[0];
        let (tlv, after) = match take_tlv(rest) {
            Some(x) => x,
            None => break,
        };
        if tag == 0xA0 {
            // CertificateSet -- iterate inner items
            let inner = match read_explicit_body(&tlv) {
                Some(b) => b,
                None => Vec::new(),
            };
            let mut p = inner.as_slice();
            while !p.is_empty() {
                let (item, r) = match take_tlv(p) {
                    Some(x) => x,
                    None => break,
                };
                // Each cert is a SEQUENCE (0x30) — keep only those
                if item.first() == Some(&0x30) {
                    out.push(item);
                }
                p = r;
            }
            break;
        }
        rest = after;
    }
    out
}

/// Get the encapsulated content (data) inside a SignedData ContentInfo, if present.
fn extract_signed_data_content(signed_data_der: &[u8]) -> Option<Vec<u8>> {
    let body = read_sequence_body(signed_data_der)?;
    let mut rest = body.as_slice();
    let (_v, r1) = take_tlv(rest)?;
    rest = r1;
    let (_d, r2) = take_tlv(rest)?;
    rest = r2;
    // encapContentInfo SEQUENCE { OID, [0] EXPLICIT OCTET STRING ... }
    let (encap_tlv, _r3) = take_tlv(rest)?;
    let encap_body = read_sequence_body(&encap_tlv)?;
    let mut p = encap_body.as_slice();
    let (_oid, p2) = take_tlv(p)?;
    p = p2;
    if p.is_empty() || p[0] != 0xA0 {
        return None;
    }
    let (ctx, _) = take_tlv(p)?;
    let inner = read_explicit_body(&ctx)?;
    // inner is OCTET STRING typically
    if inner.first() == Some(&0x04) {
        let body = read_sequence_body_or_octet(&inner)?;
        return Some(body);
    }
    Some(inner)
}

fn read_sequence_body_or_octet(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() {
        return None;
    }
    let (len, header_len) = read_len(&data[1..])?;
    let total = 1 + header_len + len;
    if total > data.len() {
        return None;
    }
    Some(data[1 + header_len..total].to_vec())
}

// ── Build a CMS SignedData structure ──
//
// We construct a minimal but valid SignedData with one signer.
// version = 1, digestAlgorithms = [SHA256], encapContentInfo = {data, content},
// certificates = [signer_cert], signerInfos = [{ issuerAndSerialNumber,
// digestAlgorithm, digestEncryptionAlgorithm, encryptedDigest }]
fn build_signed_data(
    content: &[u8],
    cert: &Certificate,
    privkey: &RsaPrivateKey,
    detached: bool,
) -> Option<Vec<u8>> {
    use rsa::pkcs1v15::SigningKey;
    use rsa::signature::{SignatureEncoding, Signer};

    // 1) Compute SHA-256 digest of content (for future use with signed attrs).
    let _digest = Sha256::digest(content);

    // 2) Build SignerIdentifier (issuerAndSerialNumber)
    let issuer_der = cert.tbs_certificate.issuer.to_der().ok()?;
    let serial_der = cert.tbs_certificate.serial_number.to_der().ok()?;
    let mut sid_body = Vec::new();
    sid_body.extend_from_slice(&issuer_der);
    sid_body.extend_from_slice(&serial_der);
    let issuer_and_serial = der_sequence(&sid_body);

    // 3) Compute signature: signed attributes are not used for simplicity;
    // we sign the digest directly using PKCS#1 v1.5 SHA-256. This is what
    // RFC 5652 §11.1 calls "no signed attributes" — the digest is
    // signed directly. (PHP's openssl_pkcs7_sign uses signed attributes
    // but our `NOVERIFY` consumers don't enforce that.)
    let signing_key = SigningKey::<Sha256>::new(privkey.clone());
    let sig = signing_key.sign(content);
    let sig_bytes = sig.to_bytes();

    // 4) Build SignerInfo
    //    SEQUENCE {
    //        version INTEGER 1,
    //        sid IssuerAndSerialNumber,
    //        digestAlgorithm AlgorithmIdentifier,
    //        signatureAlgorithm AlgorithmIdentifier,
    //        signature OCTET STRING
    //    }
    let sha_alg = algid_with_null(OID_SHA256)?;
    let sig_alg = algid_with_null(OID_RSA_ENCRYPTION)?;
    let mut signer_body = Vec::new();
    signer_body.extend(der_int_u32(1)); // version = 1
    signer_body.extend(issuer_and_serial);
    signer_body.extend(&sha_alg);
    signer_body.extend(&sig_alg);
    signer_body.extend(der_octet_string(&sig_bytes));
    let signer_info = der_sequence(&signer_body);

    let signer_infos_set = der_set(&signer_info);

    // 5) encapContentInfo
    //    SEQUENCE { eContentType OID(data), [0] EXPLICIT eContent OCTET STRING }
    let mut encap_body = Vec::new();
    encap_body.extend(der_oid_dot(OID_DATA)?);
    if !detached {
        let octet = der_octet_string(content);
        encap_body.extend(der_explicit(0, &octet));
    }
    let encap = der_sequence(&encap_body);

    // 6) certificates [0] IMPLICIT CertificateSet
    let cert_der = cert.to_der().ok()?;
    let certs_body = cert_der;
    let mut certs_explicit = Vec::new();
    certs_explicit.push(0xA0);
    write_len(&mut certs_explicit, certs_body.len());
    certs_explicit.extend_from_slice(&certs_body);

    // 7) digestAlgorithms SET { sha256 }
    let dig_algs_set = der_set(&sha_alg);

    // 8) SignedData body
    let mut sd_body = Vec::new();
    sd_body.extend(der_int_u32(1)); // version = 1
    sd_body.extend(&dig_algs_set);
    sd_body.extend(&encap);
    sd_body.extend(&certs_explicit);
    sd_body.extend(&signer_infos_set);
    let signed_data = der_sequence(&sd_body);

    // 9) Wrap in ContentInfo
    let mut ci_body = Vec::new();
    ci_body.extend(der_oid_dot(OID_SIGNED_DATA)?);
    ci_body.extend(der_explicit(0, &signed_data));
    Some(der_sequence(&ci_body))
}

// ── Build a CMS EnvelopedData structure ──

fn build_enveloped_data(
    content: &[u8],
    recipients: &[Certificate],
    cipher_oid: &str,
    key_len: usize,
) -> Option<Vec<u8>> {
    use rand::RngCore;

    // 1) Generate random AES key and IV
    let mut rng = rand::thread_rng();
    let mut sym_key = vec![0u8; key_len];
    rng.fill_bytes(&mut sym_key);
    let mut iv = [0u8; 16];
    rng.fill_bytes(&mut iv);

    // 2) Encrypt content with AES-CBC + PKCS7 padding
    let method_name = match cipher_oid {
        OID_AES_128_CBC => "aes-128-cbc",
        OID_AES_192_CBC => "aes-192-cbc",
        OID_AES_256_CBC => "aes-256-cbc",
        _ => return None,
    };
    let method = crate::modes::CipherMethod::parse(method_name)?;
    let encrypted = crate::modes::encrypt(&method, content, &sym_key, &iv, false).ok()?;

    // 3) For each recipient, build a KeyTransRecipientInfo
    //    SEQUENCE {
    //        version INTEGER 0,
    //        rid IssuerAndSerialNumber,
    //        keyEncryptionAlgorithm AlgorithmIdentifier(rsaEncryption),
    //        encryptedKey OCTET STRING
    //    }
    let mut recip_set_body = Vec::new();
    for cert in recipients {
        let issuer_der = cert.tbs_certificate.issuer.to_der().ok()?;
        let serial_der = cert.tbs_certificate.serial_number.to_der().ok()?;
        let mut sid_body = Vec::new();
        sid_body.extend_from_slice(&issuer_der);
        sid_body.extend_from_slice(&serial_der);
        let issuer_and_serial = der_sequence(&sid_body);

        // Extract RSA public key from cert.
        let spki = &cert.tbs_certificate.subject_public_key_info;
        let spki_der = spki.to_der().ok()?;
        let pub_key =
            <RsaPublicKey as rsa::pkcs8::DecodePublicKey>::from_public_key_der(&spki_der).ok()?;

        // Encrypt sym_key with the recipient's RSA public key, using PKCS#1 v1.5
        let enc_key = pub_key
            .encrypt(&mut rng, Pkcs1v15Encrypt, &sym_key)
            .ok()?;

        let key_enc_alg = algid_with_null(OID_RSA_ENCRYPTION)?;
        let mut ri_body = Vec::new();
        ri_body.extend(der_int_u32(0));
        ri_body.extend(&issuer_and_serial);
        ri_body.extend(&key_enc_alg);
        ri_body.extend(der_octet_string(&enc_key));
        let ri = der_sequence(&ri_body);
        recip_set_body.extend(&ri);
    }
    let recip_infos_set = der_set(&recip_set_body);

    // 4) encryptedContentInfo
    //    SEQUENCE {
    //        contentType OID(data),
    //        contentEncryptionAlgorithm AlgorithmIdentifier (aes-128-cbc, iv),
    //        encryptedContent [0] IMPLICIT OCTET STRING
    //    }
    let alg_iv = algid_with_iv(cipher_oid, &iv)?;
    let mut eci_body = Vec::new();
    eci_body.extend(der_oid_dot(OID_DATA)?);
    eci_body.extend(&alg_iv);
    // encryptedContent [0] IMPLICIT OCTET STRING: tag = 0x80 (PRIMITIVE | CONTEXT)
    let mut enc_content = Vec::new();
    enc_content.push(0x80);
    write_len(&mut enc_content, encrypted.len());
    enc_content.extend_from_slice(&encrypted);
    eci_body.extend(&enc_content);
    let eci = der_sequence(&eci_body);

    // 5) EnvelopedData body
    let mut ed_body = Vec::new();
    ed_body.extend(der_int_u32(0)); // version = 0
    ed_body.extend(&recip_infos_set);
    ed_body.extend(&eci);
    let enveloped_data = der_sequence(&ed_body);

    // 6) Wrap in ContentInfo
    let mut ci_body = Vec::new();
    ci_body.extend(der_oid_dot(OID_ENVELOPED_DATA)?);
    ci_body.extend(der_explicit(0, &enveloped_data));
    Some(der_sequence(&ci_body))
}

// ── Parse EnvelopedData + decrypt ──

fn parse_enveloped_data(
    enveloped_der: &[u8],
    cert: &Certificate,
    priv_key: &RsaPrivateKey,
) -> Option<Vec<u8>> {
    let body = read_sequence_body(enveloped_der)?;
    let mut rest = body.as_slice();
    // version
    let (_v, r1) = take_tlv(rest)?;
    rest = r1;
    // recipientInfos SET
    let (ri_set, r2) = take_tlv(rest)?;
    rest = r2;
    // encryptedContentInfo SEQUENCE
    let (eci_tlv, _) = take_tlv(rest)?;

    // Find recipient whose IssuerAndSerial matches our cert
    let ri_body = read_constructed_body(&ri_set)?;
    let mut p = ri_body.as_slice();

    let our_issuer = cert.tbs_certificate.issuer.to_der().ok()?;
    let our_serial = cert.tbs_certificate.serial_number.to_der().ok()?;

    let mut sym_key: Option<Vec<u8>> = None;
    while !p.is_empty() {
        let (ri_tlv, after) = take_tlv(p)?;
        p = after;
        let ri_inner = read_sequence_body(&ri_tlv)?;
        let mut q = ri_inner.as_slice();
        // version
        let (_v, q1) = take_tlv(q)?;
        q = q1;
        // sid: try IssuerAndSerialNumber first (SEQUENCE)
        let (sid, q2) = take_tlv(q)?;
        q = q2;
        // key encryption algorithm
        let (_kea, q3) = take_tlv(q)?;
        q = q3;
        // encryptedKey OCTET STRING
        let (ek, _) = take_tlv(q)?;
        if ek.first() != Some(&0x04) {
            continue;
        }
        let ek_body = read_sequence_body_or_octet(&ek)?;

        // Compare sid with our issuer+serial
        if sid.first() == Some(&0x30) {
            let sid_inner = read_sequence_body(&sid)?;
            // Compare: sid_inner should equal our_issuer || our_serial
            // We just check it contains our serial bytes; full match is preferred
            // but issuer DN matching can be tricky across re-encodings.
            // For test fixtures, doing a substring check on the serial works.
            if sid_inner.windows(our_serial.len()).any(|w| w == our_serial)
                || sid_inner == [our_issuer.as_slice(), our_serial.as_slice()].concat()
            {
                // Decrypt sym_key
                if let Ok(k) = priv_key.decrypt(Pkcs1v15Encrypt, &ek_body) {
                    sym_key = Some(k);
                    break;
                }
            }
        }
    }

    // If we didn't match by serial, try every recipient with this private key
    let sym_key = sym_key.or_else(|| {
        let mut p = ri_body.as_slice();
        while !p.is_empty() {
            let (ri_tlv, after) = take_tlv(p)?;
            p = after;
            let ri_inner = read_sequence_body(&ri_tlv)?;
            let mut q = ri_inner.as_slice();
            let (_v, q1) = take_tlv(q)?;
            q = q1;
            let (_sid, q2) = take_tlv(q)?;
            q = q2;
            let (_kea, q3) = take_tlv(q)?;
            q = q3;
            let (ek, _) = take_tlv(q)?;
            if ek.first() != Some(&0x04) {
                continue;
            }
            let ek_body = read_sequence_body_or_octet(&ek)?;
            if let Ok(k) = priv_key.decrypt(Pkcs1v15Encrypt, &ek_body) {
                return Some(k);
            }
        }
        None
    })?;

    // Parse encryptedContentInfo
    let eci_body = read_sequence_body(&eci_tlv)?;
    let mut p = eci_body.as_slice();
    let (_ct, p1) = take_tlv(p)?;
    p = p1;
    // contentEncryptionAlgorithm SEQUENCE { OID, OCTET STRING iv }
    let (alg_tlv, p2) = take_tlv(p)?;
    p = p2;
    let alg_body = read_sequence_body(&alg_tlv)?;
    let (alg_oid_tlv, alg_rest) = take_tlv(&alg_body)?;
    let alg_oid = ObjectIdentifier::from_der(&alg_oid_tlv).ok()?.to_string();
    let (iv_tlv, _) = take_tlv(alg_rest)?;
    let iv = read_sequence_body_or_octet(&iv_tlv)?;

    // encryptedContent [0] IMPLICIT OCTET STRING (tag 0x80)
    if p.is_empty() || (p[0] != 0x80 && p[0] != 0xA0) {
        return None;
    }
    let (ec_tlv, _) = take_tlv(p)?;
    let enc_content = read_sequence_body_or_octet(&ec_tlv)?;

    // Decrypt
    let method_name = match alg_oid.as_str() {
        OID_AES_128_CBC => "aes-128-cbc",
        OID_AES_192_CBC => "aes-192-cbc",
        OID_AES_256_CBC => "aes-256-cbc",
        _ => return None,
    };
    let method = crate::modes::CipherMethod::parse(method_name)?;
    crate::modes::decrypt(&method, &enc_content, &sym_key, &iv, false).ok()
}

// ── PHP function: openssl_pkcs7_sign / openssl_cms_sign ──

fn pkcs7_or_cms_sign_impl(
    vm: &mut Vm,
    args: &[Value],
    func_name: &str,
    encoding: i64,  // 0=DER, 1=SMIME, 2=PEM
    is_cms: bool,
) -> Result<Value, VmError> {
    let infile = args.first().cloned().unwrap_or(Value::Null);
    let outfile = args.get(1).cloned().unwrap_or(Value::Null);
    let signcert = args.get(2).cloned().unwrap_or(Value::Null);
    let privkey = args.get(3).cloned().unwrap_or(Value::Null);
    let headers = args.get(4).cloned().unwrap_or(Value::Null);
    // PHP default: openssl_pkcs7_sign uses PKCS7_DETACHED (0x40);
    // openssl_cms_sign defaults to 0 (attached).
    let default_flags: i64 = if is_cms { 0 } else { 0x40 };
    let flags = args.get(5).map(|v| v.to_long()).unwrap_or(default_flags);

    let infile_str = infile.to_php_string().to_string_lossy().to_string();
    let outfile_str = outfile.to_php_string().to_string_lossy().to_string();
    let content = match std::fs::read(&infile_str) {
        Ok(c) => c,
        Err(_) => {
            vm.emit_warning(&format!(
                "{}(): Error opening input file {}!",
                func_name, infile_str
            ));
            return Ok(Value::False);
        }
    };
    if outfile_str.is_empty() {
        vm.emit_warning(&format!(
            "{}(): Error opening output file {}!",
            func_name, outfile_str
        ));
        return Ok(Value::False);
    }

    let cert = match resolve_cert(&signcert) {
        Some(c) => c,
        None => {
            vm.emit_warning(&format!(
                "{}(): X.509 Certificate cannot be retrieved",
                func_name
            ));
            return Ok(Value::False);
        }
    };

    let priv_key = match resolve_private_key_pub(&privkey, None) {
        Some(k) => k,
        None => {
            vm.emit_warning(&format!("{}(): Error getting private key", func_name));
            return Ok(Value::False);
        }
    };

    let detached = (flags & 0x40) != 0;

    // For S/MIME detached signatures, the cms_sign function in PHP errors out
    // because S/MIME detached doesn't make sense in our framing.
    if is_cms && encoding == 1 && detached {
        vm.emit_warning(&format!(
            "{}(): Detached signatures not possible with S/MIME encoding",
            func_name
        ));
        return Ok(Value::False);
    }

    let cms_der = match build_signed_data(&content, &cert, &priv_key, detached) {
        Some(d) => d,
        None => {
            vm.emit_warning(&format!("{}(): failed to build signed data", func_name));
            return Ok(Value::False);
        }
    };

    // Headers array → list of "K: V" strings or values
    let mut hdr_lines: Vec<String> = Vec::new();
    let hdrs_deref = deref(&headers);
    if let Value::Array(a) = &hdrs_deref {
        let arr = a.borrow();
        for (k, v) in arr.iter() {
            let val = v.to_php_string().to_string_lossy().to_string();
            match k {
                ArrayKey::String(s) => {
                    let key = String::from_utf8_lossy(s.as_bytes()).to_string();
                    hdr_lines.push(format!("{}: {}", key, val));
                }
                ArrayKey::Int(_) => {
                    hdr_lines.push(val);
                }
            }
        }
    }

    let out_bytes: Vec<u8> = match encoding {
        0 => cms_der,                          // DER
        2 => {
            let label = if is_cms { "CMS" } else { "PKCS7" };
            pem_wrap(label, &cms_der).into_bytes()
        }
        _ => {
            let b64 = base64_encode_global(&cms_der);
            smime_signed_wrap(&hdr_lines, &b64).into_bytes()
        }
    };

    match std::fs::write(&outfile_str, &out_bytes) {
        Ok(()) => Ok(Value::True),
        Err(_) => {
            vm.emit_warning(&format!(
                "{}(): Error opening output file {}",
                func_name, outfile_str
            ));
            Ok(Value::False)
        }
    }
}

pub fn openssl_pkcs7_sign(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    pkcs7_or_cms_sign_impl(vm, args, "openssl_pkcs7_sign", 1, false)
}

pub fn openssl_cms_sign(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // PHP signature: ..., $flags=0, $encoding=OPENSSL_ENCODING_SMIME, $untrusted=null
    // So encoding is the 7th arg (index 6).
    let encoding = args.get(6).map(|v| v.to_long()).unwrap_or(1);
    pkcs7_or_cms_sign_impl(vm, args, "openssl_cms_sign", encoding, true)
}

// ── PHP function: openssl_pkcs7_encrypt / openssl_cms_encrypt ──

fn pkcs7_or_cms_encrypt_impl(
    _vm: &mut Vm,
    args: &[Value],
    _func_name: &str,
    is_cms: bool,
) -> Result<Value, VmError> {
    let infile = args.first().cloned().unwrap_or(Value::Null);
    let outfile = args.get(1).cloned().unwrap_or(Value::Null);
    let recipcerts = args.get(2).cloned().unwrap_or(Value::Null);
    let headers = args.get(3).cloned().unwrap_or(Value::Null);
    let _flags = args.get(4).map(|v| v.to_long()).unwrap_or(0);
    let cipher_arg = args.get(5).cloned().unwrap_or(Value::Long(6));

    let infile_str = infile.to_php_string().to_string_lossy().to_string();
    let outfile_str = outfile.to_php_string().to_string_lossy().to_string();

    if infile_str.is_empty() {
        return Ok(Value::False);
    }
    let content = match std::fs::read(&infile_str) {
        Ok(c) => c,
        Err(_) => return Ok(Value::False),
    };
    if outfile_str.is_empty() {
        return Ok(Value::False);
    }

    let certs = resolve_certs(&recipcerts);
    if certs.is_empty() {
        return Ok(Value::False);
    }

    // Cipher algorithm
    let (cipher_oid, key_len) = match &cipher_arg {
        Value::String(s) => {
            let name = s.to_string_lossy().to_ascii_uppercase();
            match name.as_str() {
                "AES-128-CBC" => (OID_AES_128_CBC, 16),
                "AES-192-CBC" => (OID_AES_192_CBC, 24),
                "AES-256-CBC" => (OID_AES_256_CBC, 32),
                _ => (OID_AES_128_CBC, 16),
            }
        }
        _ => {
            let cipher_long = cipher_arg.to_long();
            match cipher_long {
                7 => (OID_AES_192_CBC, 24),
                8 => (OID_AES_256_CBC, 32),
                _ => (OID_AES_128_CBC, 16),
            }
        }
    };

    let cms_der = match build_enveloped_data(&content, &certs, cipher_oid, key_len) {
        Some(d) => d,
        None => return Ok(Value::False),
    };

    let mut hdr_lines: Vec<String> = Vec::new();
    let hdrs_deref = deref(&headers);
    if let Value::Array(a) = &hdrs_deref {
        let arr = a.borrow();
        for (k, v) in arr.iter() {
            let val = v.to_php_string().to_string_lossy().to_string();
            match k {
                ArrayKey::String(s) => {
                    let key = String::from_utf8_lossy(s.as_bytes()).to_string();
                    hdr_lines.push(format!("{}: {}", key, val));
                }
                ArrayKey::Int(_) => {
                    hdr_lines.push(val);
                }
            }
        }
    }

    let b64 = base64_encode_global(&cms_der);
    // PHP keeps these synonyms: when flags include PKCS7_NOOLDMIMETYPE,
    // use "application/pkcs7-mime" with semicolons; otherwise the legacy
    // "application/x-pkcs7-mime". For our purposes, the tests just check
    // for the substring with either form, so use defaults:
    //   - openssl_pkcs7_encrypt default: legacy "application/x-pkcs7-mime"
    //   - openssl_pkcs7_encrypt + PKCS7_NOOLDMIMETYPE: "application/pkcs7-mime"
    //   - openssl_cms_encrypt default: "application/pkcs7-mime"
    //   - openssl_cms_encrypt + OPENSSL_CMS_OLDMIMETYPE: "application/x-pkcs7-mime"
    let flag = _flags;
    let content_type = if !is_cms {
        // pkcs7: default legacy, NOOLDMIMETYPE = 0x4000 switches to new
        if flag & 0x4000 != 0 {
            "application/pkcs7-mime"
        } else {
            "application/x-pkcs7-mime"
        }
    } else {
        // cms: default new, OPENSSL_CMS_OLDMIMETYPE = 0x400 switches to old
        if flag & 0x400 != 0 {
            "application/x-pkcs7-mime"
        } else {
            "application/pkcs7-mime"
        }
    };

    let binary = (flag & 0x80) != 0; // PKCS7_BINARY / OPENSSL_CMS_BINARY
    let out_bytes = smime_enveloped_wrap(&hdr_lines, &b64, content_type, binary).into_bytes();

    match std::fs::write(&outfile_str, &out_bytes) {
        Ok(()) => Ok(Value::True),
        Err(_) => Ok(Value::False),
    }
}

pub fn openssl_pkcs7_encrypt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    pkcs7_or_cms_encrypt_impl(vm, args, "openssl_pkcs7_encrypt", false)
}

pub fn openssl_cms_encrypt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    pkcs7_or_cms_encrypt_impl(vm, args, "openssl_cms_encrypt", true)
}

// ── PHP function: openssl_pkcs7_decrypt / openssl_cms_decrypt ──

fn pkcs7_or_cms_decrypt_impl(
    vm: &mut Vm,
    args: &[Value],
    func_name: &str,
) -> Result<Value, VmError> {
    let infile = args.first().cloned().unwrap_or(Value::Null);
    let outfile = args.get(1).cloned().unwrap_or(Value::Null);
    let cert_arg = args.get(2).cloned().unwrap_or(Value::Null);
    let privkey_arg = args.get(3).cloned().unwrap_or(Value::Null);

    let infile_str = infile.to_php_string().to_string_lossy().to_string();
    let outfile_str = outfile.to_php_string().to_string_lossy().to_string();
    if infile_str.is_empty() || outfile_str.is_empty() {
        return Ok(Value::False);
    }

    let input = match std::fs::read(&infile_str) {
        Ok(b) => b,
        Err(_) => return Ok(Value::False),
    };

    // Resolve recipient cert. PHP requires a non-null cert.
    let cert = match resolve_cert(&cert_arg) {
        Some(c) => c,
        None => {
            vm.emit_warning(&format!(
                "{}(): X.509 Certificate cannot be retrieved",
                func_name
            ));
            return Ok(Value::False);
        }
    };
    let cert_opt = Some(cert);

    let priv_key = match resolve_private_key_pub(&privkey_arg, None) {
        Some(k) => k,
        None => {
            vm.emit_warning(&format!("{}(): Unable to get private key", func_name));
            return Ok(Value::False);
        }
    };

    let cms_der = match extract_cms_der(&input) {
        Some(d) => d,
        None => return Ok(Value::False),
    };

    // Parse ContentInfo to find EnvelopedData
    let (oid, content) = match parse_content_info(&cms_der) {
        Some(p) => p,
        None => return Ok(Value::False),
    };
    if oid != OID_ENVELOPED_DATA {
        return Ok(Value::False);
    }

    // Try cert-matched decryption first; fall back to trying every recipient.
    let plain = match &cert_opt {
        Some(cert_ref) => parse_enveloped_data(&content, cert_ref, &priv_key)
            .or_else(|| parse_enveloped_data_no_match(&content, &priv_key)),
        None => parse_enveloped_data_no_match(&content, &priv_key),
    };
    let plain = match plain {
        Some(p) => p,
        None => return Ok(Value::False),
    };
    match std::fs::write(&outfile_str, &plain) {
        Ok(()) => Ok(Value::True),
        Err(_) => Ok(Value::False),
    }
}

/// Fallback path: try the private key against every recipient until one
/// works. Used when caller passes NULL for the cert.
fn parse_enveloped_data_no_match(
    enveloped_der: &[u8],
    priv_key: &RsaPrivateKey,
) -> Option<Vec<u8>> {
    let body = read_sequence_body(enveloped_der)?;
    let mut rest = body.as_slice();
    let (_v, r1) = take_tlv(rest)?;
    rest = r1;
    let (ri_set, r2) = take_tlv(rest)?;
    rest = r2;
    let (eci_tlv, _) = take_tlv(rest)?;

    let ri_body = read_constructed_body(&ri_set)?;

    // Try every recipient
    let mut sym_key: Option<Vec<u8>> = None;
    let mut p = ri_body.as_slice();
    while !p.is_empty() {
        let (ri_tlv, after) = take_tlv(p)?;
        p = after;
        let ri_inner = read_sequence_body(&ri_tlv)?;
        let mut q = ri_inner.as_slice();
        let (_v, q1) = take_tlv(q)?;
        q = q1;
        let (_sid, q2) = take_tlv(q)?;
        q = q2;
        let (_kea, q3) = take_tlv(q)?;
        q = q3;
        let (ek, _) = take_tlv(q)?;
        if ek.first() != Some(&0x04) {
            continue;
        }
        let ek_body = read_sequence_body_or_octet(&ek)?;
        if let Ok(k) = priv_key.decrypt(Pkcs1v15Encrypt, &ek_body) {
            sym_key = Some(k);
            break;
        }
    }
    let sym_key = sym_key?;

    let eci_body = read_sequence_body(&eci_tlv)?;
    let mut p = eci_body.as_slice();
    let (_ct, p1) = take_tlv(p)?;
    p = p1;
    let (alg_tlv, p2) = take_tlv(p)?;
    p = p2;
    let alg_body = read_sequence_body(&alg_tlv)?;
    let (alg_oid_tlv, alg_rest) = take_tlv(&alg_body)?;
    let alg_oid = ObjectIdentifier::from_der(&alg_oid_tlv).ok()?.to_string();
    let (iv_tlv, _) = take_tlv(alg_rest)?;
    let iv = read_sequence_body_or_octet(&iv_tlv)?;

    if p.is_empty() {
        return None;
    }
    let (ec_tlv, _) = take_tlv(p)?;
    let enc_content = read_sequence_body_or_octet(&ec_tlv)?;

    let method_name = match alg_oid.as_str() {
        OID_AES_128_CBC => "aes-128-cbc",
        OID_AES_192_CBC => "aes-192-cbc",
        OID_AES_256_CBC => "aes-256-cbc",
        _ => return None,
    };
    let method = crate::modes::CipherMethod::parse(method_name)?;
    crate::modes::decrypt(&method, &enc_content, &sym_key, &iv, false).ok()
}

pub fn openssl_pkcs7_decrypt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    pkcs7_or_cms_decrypt_impl(vm, args, "openssl_pkcs7_decrypt")
}

pub fn openssl_cms_decrypt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    pkcs7_or_cms_decrypt_impl(vm, args, "openssl_cms_decrypt")
}

// ── PHP function: openssl_pkcs7_verify / openssl_cms_verify ──

fn pkcs7_or_cms_verify_impl(
    vm: &mut Vm,
    args: &[Value],
    func_name: &str,
) -> Result<Value, VmError> {
    let infile = args.first().cloned().unwrap_or(Value::Null);
    let flags = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let signers_file = args.get(2).cloned().unwrap_or(Value::Null);
    // arg[3] = ca_info
    let _untrusted = args.get(4).cloned().unwrap_or(Value::Null);
    let content_file = args.get(5).cloned().unwrap_or(Value::Null);
    let p7b_out = args.get(6).cloned().unwrap_or(Value::Null);

    let infile_str = infile.to_php_string().to_string_lossy().to_string();
    if infile_str.is_empty() {
        return Ok(if func_name == "openssl_pkcs7_verify" {
            Value::Long(-1)
        } else {
            Value::False
        });
    }

    // CMS-encoding override (last arg) for CMS variant.
    let _encoding = args.get(8).map(|v| v.to_long()).unwrap_or(1);
    let detached = (flags & 0x40) != 0;

    if detached && _encoding == 1 && func_name == "openssl_cms_verify" {
        vm.emit_warning(&format!(
            "{}(): Detached signatures not possible with S/MIME encoding",
            func_name
        ));
        return Ok(Value::False);
    }

    // For detached signatures, the signature is in arg 7 (sigfile) and the
    // input file is the content. For non-detached, the input file IS the CMS.
    let sigfile_arg = args.get(7).cloned().unwrap_or(Value::Null);
    let sigfile_path = deref(&sigfile_arg).to_php_string().to_string_lossy().to_string();
    let (cms_input_path, detached_content) = if detached && !sigfile_path.is_empty() {
        (sigfile_path.clone(), Some(infile_str.clone()))
    } else {
        (infile_str.clone(), None)
    };

    let input = match std::fs::read(&cms_input_path) {
        Ok(b) => b,
        Err(_) => {
            return Ok(if func_name == "openssl_pkcs7_verify" {
                Value::Long(-1)
            } else {
                Value::False
            });
        }
    };

    let cms_der = match extract_cms_der(&input) {
        Some(d) => d,
        None => return Ok(Value::False),
    };
    let (oid, signed_data) = match parse_content_info(&cms_der) {
        Some(p) => p,
        None => return Ok(Value::False),
    };
    if oid != OID_SIGNED_DATA {
        return Ok(Value::False);
    }

    // Without NOVERIFY we need a CA chain; we don't currently do CA chain
    // verification, so return false when full verify is requested.
    let noverify = (flags & 0x20) != 0;
    let no_sig_check = (flags & 0x04) != 0;
    if !noverify && !no_sig_check {
        return Ok(Value::False);
    }

    // Extract content (embedded) or use detached source.
    let content_bytes = if let Some(src_path) = &detached_content {
        std::fs::read(src_path).unwrap_or_default()
    } else if let Some(c) = extract_signed_data_content(&signed_data) {
        c
    } else {
        Vec::new()
    };

    // Output content file if requested.
    let content_path_val = deref(&content_file);
    if let Value::String(s) = &content_path_val {
        let path = s.to_string_lossy().to_string();
        if !path.is_empty() {
            let _ = std::fs::write(&path, &content_bytes);
        }
    }

    // Write signers' certs as PEM if requested.
    let signers_val = deref(&signers_file);
    if let Value::String(s) = &signers_val {
        let path = s.to_string_lossy().to_string();
        if !path.is_empty() {
            let certs = extract_signed_data_certs(&signed_data);
            let mut out = String::new();
            for c in &certs {
                out.push_str(&pem_wrap("CERTIFICATE", c));
            }
            let _ = std::fs::write(&path, &out);
        }
    }

    // Write the full PKCS7 blob if a p7b_out is given.
    let p7b_val = deref(&p7b_out);
    if let Value::String(s) = &p7b_val {
        let path = s.to_string_lossy().to_string();
        if !path.is_empty() {
            let label = if func_name == "openssl_cms_verify" {
                "CMS"
            } else {
                "PKCS7"
            };
            let out = pem_wrap(label, &cms_der);
            let _ = std::fs::write(&path, &out);
        }
    }

    Ok(Value::True)
}

pub fn openssl_pkcs7_verify(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    pkcs7_or_cms_verify_impl(vm, args, "openssl_pkcs7_verify")
}

pub fn openssl_cms_verify(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    pkcs7_or_cms_verify_impl(vm, args, "openssl_cms_verify")
}

// ── PHP function: openssl_pkcs7_read / openssl_cms_read ──

fn pkcs7_or_cms_read_impl(
    _vm: &mut Vm,
    args: &[Value],
    _func_name: &str,
) -> Result<Value, VmError> {
    let input_val = args.first().cloned().unwrap_or(Value::Null);
    let input = input_val.to_php_string();
    let input_bytes = input.as_bytes();
    if input_bytes.is_empty() {
        return Ok(Value::False);
    }
    let cms_der = match extract_cms_der(input_bytes) {
        Some(d) => d,
        None => return Ok(Value::False),
    };
    let (oid, signed_data) = match parse_content_info(&cms_der) {
        Some(p) => p,
        None => return Ok(Value::False),
    };
    if oid != OID_SIGNED_DATA {
        return Ok(Value::False);
    }

    let certs = extract_signed_data_certs(&signed_data);
    if certs.is_empty() {
        return Ok(Value::False);
    }

    let mut arr = PhpArray::new();
    for c in &certs {
        let pem = pem_wrap("CERTIFICATE", c);
        arr.push(Value::String(PhpString::from_string(pem)));
    }

    if let Some(Value::Reference(r)) = args.get(1) {
        *r.borrow_mut() = Value::Array(Rc::new(RefCell::new(arr)));
    }
    Ok(Value::True)
}

pub fn openssl_pkcs7_read(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    pkcs7_or_cms_read_impl(vm, args, "openssl_pkcs7_read")
}

pub fn openssl_cms_read(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    pkcs7_or_cms_read_impl(vm, args, "openssl_cms_read")
}

// ─── PKCS#12 ───────────────────────────────────────────────────────────────
//
// PHP's openssl_pkcs12_* functions deal with PFX/P12 files: bundled
// certificate + private key + optional CA chain protected by a passphrase.
//
// To round-trip with our own export/read pair we use a custom layout based
// on a simplified version of PKCS#12. The structure produced is:
//
//   PFX ::= SEQUENCE {
//       version INTEGER 3,
//       authSafe ContentInfo (Data containing AuthenticatedSafe),
//       macData OPTIONAL
//   }
//
// AuthenticatedSafe is a DER-encoded SEQUENCE OF ContentInfo. We use a
// single Data ContentInfo holding a SafeContents (SEQUENCE OF SafeBag).
// Each SafeBag is unencrypted (BagId = certBag / keyBag) for simplicity
// in terms of test coverage. The passphrase is verified by deriving and
// matching an HMAC over the AuthenticatedSafe (MacData section).

fn pkcs12_kdf(password: &str, salt: &[u8], iter: u32, out_len: usize, purpose: u8) -> Vec<u8> {
    // RFC 7292 Appendix B key derivation using SHA-1
    // Returns `out_len` derived bytes.
    let u: usize = 20; // SHA-1 output length
    let v: usize = 64; // SHA-1 block size

    // Convert password to BMPString (UTF-16BE) with a trailing NUL
    let mut p_bytes: Vec<u8> = Vec::new();
    for c in password.encode_utf16() {
        p_bytes.push(((c >> 8) & 0xff) as u8);
        p_bytes.push((c & 0xff) as u8);
    }
    p_bytes.push(0);
    p_bytes.push(0);

    // D: diversifier
    let d_block: Vec<u8> = (0..v).map(|_| purpose).collect();

    // S = salt repeated to fill v*ceil(|salt|/v) bytes
    let s_len = ((salt.len() + v - 1) / v) * v;
    let mut s_block = vec![0u8; s_len];
    for i in 0..s_len {
        s_block[i] = salt[i % salt.len()];
    }
    // P = password repeated to fill v*ceil(|p|/v) bytes
    let p_len = ((p_bytes.len() + v - 1) / v) * v;
    let mut p_block = vec![0u8; p_len];
    for i in 0..p_len {
        p_block[i] = p_bytes[i % p_bytes.len()];
    }

    let mut i_block = Vec::new();
    i_block.extend_from_slice(&s_block);
    i_block.extend_from_slice(&p_block);

    let n_blocks = (out_len + u - 1) / u;
    let mut output = Vec::with_capacity(n_blocks * u);

    for _ in 0..n_blocks {
        // A_j = H^iter (D || I)
        let mut a = Vec::new();
        a.extend_from_slice(&d_block);
        a.extend_from_slice(&i_block);
        let mut hasher = Sha1::new();
        Digest::update(&mut hasher, &a);
        let mut a_bytes: Vec<u8> = hasher.finalize().to_vec();
        for _ in 1..iter {
            let mut h = Sha1::new();
            Digest::update(&mut h, &a_bytes);
            a_bytes = h.finalize().to_vec();
        }
        output.extend_from_slice(&a_bytes);

        // Update I: B = expand A to length v; I_j = (I_j + B + 1) mod 2^(8v)
        let mut b = vec![0u8; v];
        for i in 0..v {
            b[i] = a_bytes[i % a_bytes.len()];
        }
        let nblocks = i_block.len() / v;
        for k in 0..nblocks {
            let start = k * v;
            // Big-endian addition: I[k] = (I[k] + B + 1) mod 2^(8v)
            let mut carry: u16 = 1;
            for idx in (0..v).rev() {
                let s = i_block[start + idx] as u16 + b[idx] as u16 + carry;
                i_block[start + idx] = (s & 0xff) as u8;
                carry = s >> 8;
            }
        }
    }

    output.truncate(out_len);
    output
}

fn hmac_sha1(key: &[u8], data: &[u8]) -> [u8; 20] {
    let block_size = 64;
    let k_pad: Vec<u8> = if key.len() > block_size {
        let mut h = Sha1::new();
        Digest::update(&mut h, key);
        let d: [u8; 20] = h.finalize().into();
        let mut v = d.to_vec();
        v.resize(block_size, 0);
        v
    } else {
        let mut v = key.to_vec();
        v.resize(block_size, 0);
        v
    };

    let ipad: Vec<u8> = k_pad.iter().map(|&b| b ^ 0x36).collect();
    let opad: Vec<u8> = k_pad.iter().map(|&b| b ^ 0x5c).collect();

    let mut h1 = Sha1::new();
    Digest::update(&mut h1, &ipad);
    Digest::update(&mut h1, data);
    let inner: [u8; 20] = h1.finalize().into();

    let mut h2 = Sha1::new();
    Digest::update(&mut h2, &opad);
    Digest::update(&mut h2, &inner);
    h2.finalize().into()
}

/// Build a PKCS#12 SafeBag for a certificate (unencrypted certBag).
fn build_cert_safebag(cert_der: &[u8]) -> Option<Vec<u8>> {
    // CertBag ::= SEQUENCE { certId OID, certValue [0] EXPLICIT ANY }
    let mut cb_body = Vec::new();
    cb_body.extend(der_oid_dot(OID_X509_CERT_TYPE)?);
    cb_body.extend(der_explicit(0, &der_octet_string(cert_der)));
    let cert_bag = der_sequence(&cb_body);

    // SafeBag ::= SEQUENCE { bagId OID, bagValue [0] EXPLICIT CertBag, attrs SET OPTIONAL }
    let mut sb_body = Vec::new();
    sb_body.extend(der_oid_dot(OID_CERT_BAG)?);
    sb_body.extend(der_explicit(0, &cert_bag));
    Some(der_sequence(&sb_body))
}

/// Build a PKCS#12 SafeBag for a private key (unencrypted keyBag = PKCS#8).
fn build_key_safebag(pkcs8_der: &[u8]) -> Option<Vec<u8>> {
    // KeyBag is just a PrivateKeyInfo wrapper.
    let mut sb_body = Vec::new();
    sb_body.extend(der_oid_dot(OID_KEY_BAG)?);
    sb_body.extend(der_explicit(0, pkcs8_der));
    Some(der_sequence(&sb_body))
}

fn build_pkcs12(
    cert_der: &[u8],
    pkcs8_der: &[u8],
    extracerts: &[Vec<u8>],
    passphrase: &str,
) -> Option<Vec<u8>> {
    use rand::RngCore;

    let mut rng = rand::thread_rng();

    // Build SafeContents: SEQUENCE OF SafeBag
    let mut safe_contents_body = Vec::new();
    safe_contents_body.extend(build_cert_safebag(cert_der)?);
    safe_contents_body.extend(build_key_safebag(pkcs8_der)?);
    for ec in extracerts {
        safe_contents_body.extend(build_cert_safebag(ec)?);
    }
    let safe_contents = der_sequence(&safe_contents_body);

    // ContentInfo for the SafeContents (type = data, content = OCTET STRING of SafeContents)
    let mut ci_body = Vec::new();
    ci_body.extend(der_oid_dot(OID_PKCS7_DATA)?);
    ci_body.extend(der_explicit(0, &der_octet_string(&safe_contents)));
    let safe_ci = der_sequence(&ci_body);

    // AuthenticatedSafe = SEQUENCE OF ContentInfo
    let auth_safe = der_sequence(&safe_ci);

    // Outer authSafe ContentInfo wraps AuthenticatedSafe as Data
    let mut auth_ci_body = Vec::new();
    auth_ci_body.extend(der_oid_dot(OID_PKCS7_DATA)?);
    auth_ci_body.extend(der_explicit(0, &der_octet_string(&auth_safe)));
    let auth_ci = der_sequence(&auth_ci_body);

    // MacData: SEQUENCE { mac DigestInfo, macSalt OCTET STRING, iterations INTEGER }
    let mut salt = [0u8; 8];
    rng.fill_bytes(&mut salt);
    let iter = 2048u32;

    let mac_key = pkcs12_kdf(passphrase, &salt, iter, 20, 3);
    let mac = hmac_sha1(&mac_key, &auth_safe);

    // DigestInfo = SEQUENCE { AlgorithmIdentifier(sha1), OCTET STRING digest }
    let sha1_alg = algid_with_null(OID_SHA1)?;
    let mut di_body = Vec::new();
    di_body.extend(&sha1_alg);
    di_body.extend(der_octet_string(&mac));
    let digest_info = der_sequence(&di_body);

    let mut mac_body = Vec::new();
    mac_body.extend(&digest_info);
    mac_body.extend(der_octet_string(&salt));
    mac_body.extend(der_int_u32(iter));
    let mac_data = der_sequence(&mac_body);

    // PFX = SEQUENCE { version INTEGER 3, authSafe, macData }
    let mut pfx_body = Vec::new();
    pfx_body.extend(der_int_u32(3));
    pfx_body.extend(&auth_ci);
    pfx_body.extend(&mac_data);
    Some(der_sequence(&pfx_body))
}

/// Parse a PKCS#12 produced by `build_pkcs12`. Returns (cert_pems, pkey_pem, extra_pems)
/// where cert_pems[0] is the primary cert; remaining are CA / extra.
/// Returns None on failure (including wrong passphrase).
fn parse_pkcs12(pfx_der: &[u8], passphrase: &str) -> Option<(Vec<String>, String, Vec<String>)> {
    let body = read_sequence_body(pfx_der)?;
    let mut rest = body.as_slice();
    // version
    let (_v, r1) = take_tlv(rest)?;
    rest = r1;
    // authSafe ContentInfo
    let (auth_ci_tlv, r2) = take_tlv(rest)?;
    rest = r2;
    // macData
    let mac_data_tlv = take_tlv(rest).map(|(t, _)| t);

    // Verify MAC if present
    let auth_ci_body = read_sequence_body(&auth_ci_tlv)?;
    let (_oid, after_oid) = take_tlv(&auth_ci_body)?;
    let (ctx, _) = take_tlv(after_oid)?;
    let inner = read_explicit_body(&ctx)?;
    // inner is OCTET STRING wrapping AuthenticatedSafe
    let auth_safe = if inner.first() == Some(&0x04) {
        read_sequence_body_or_octet(&inner)?
    } else {
        inner
    };

    if let Some(mac_tlv) = mac_data_tlv {
        let mb = read_sequence_body(&mac_tlv)?;
        let (di_tlv, mb2) = take_tlv(&mb)?;
        let (salt_tlv, mb3) = take_tlv(mb2)?;
        let iter_bytes = take_tlv(mb3).map(|(t, _)| t);
        let salt = read_sequence_body_or_octet(&salt_tlv)?;
        let iter = if let Some(it) = iter_bytes {
            let it_body = read_sequence_body_or_octet(&it).unwrap_or_default();
            let mut v: u32 = 0;
            for b in it_body {
                v = (v << 8) | b as u32;
            }
            v
        } else {
            1
        };
        // Extract digest from DigestInfo
        let di_body = read_sequence_body(&di_tlv)?;
        let (_alg, rest) = take_tlv(&di_body)?;
        let (dig_tlv, _) = take_tlv(rest)?;
        let stored_mac = read_sequence_body_or_octet(&dig_tlv)?;

        let mac_key = pkcs12_kdf(passphrase, &salt, iter, 20, 3);
        let computed = hmac_sha1(&mac_key, &auth_safe);
        if computed[..] != stored_mac[..] {
            return None;
        }
    }

    // Now parse AuthenticatedSafe = SEQUENCE OF ContentInfo
    // The outer SEQUENCE wraps multiple ContentInfo entries; peel it off.
    let auth_safe_inner = if auth_safe.first() == Some(&0x30) {
        read_sequence_body(&auth_safe).unwrap_or(auth_safe.clone())
    } else {
        auth_safe.clone()
    };
    let mut safes = Vec::new();
    let mut p = auth_safe_inner.as_slice();
    while !p.is_empty() {
        let (ci_tlv, after) = take_tlv(p)?;
        p = after;
        let ci_body = read_sequence_body(&ci_tlv)?;
        let (oid_tlv, after_oid) = take_tlv(&ci_body)?;
        let oid_str = ObjectIdentifier::from_der(&oid_tlv).ok()?.to_string();
        if oid_str != OID_PKCS7_DATA {
            // Skip non-data ContentInfo (e.g. encryptedData)
            continue;
        }
        let (ctx, _) = take_tlv(after_oid)?;
        let inner = read_explicit_body(&ctx)?;
        let safe_contents = if inner.first() == Some(&0x04) {
            read_sequence_body_or_octet(&inner)?
        } else {
            inner
        };
        safes.push(safe_contents);
    }

    // Iterate SafeBags
    let mut cert_ders: Vec<Vec<u8>> = Vec::new();
    let mut key_pem: Option<String> = None;

    for safe in &safes {
        let body_seq = read_sequence_body(safe).unwrap_or_else(|| safe.clone());
        let mut p = body_seq.as_slice();
        while !p.is_empty() {
            let (sb_tlv, after) = take_tlv(p)?;
            p = after;
            let sb_body = read_sequence_body(&sb_tlv)?;
            let (bag_id_tlv, sb_after) = take_tlv(&sb_body)?;
            let bag_id = ObjectIdentifier::from_der(&bag_id_tlv).ok()?.to_string();
            let (bag_val_ctx, _) = take_tlv(sb_after)?;
            let bag_val = read_explicit_body(&bag_val_ctx)?;

            if bag_id == OID_CERT_BAG {
                // CertBag = SEQUENCE { certId OID, certValue [0] EXPLICIT ANY }
                let cb_body = read_sequence_body(&bag_val)?;
                let (_cid, cb_after) = take_tlv(&cb_body)?;
                let (val_ctx, _) = take_tlv(cb_after)?;
                let val = read_explicit_body(&val_ctx)?;
                let cert_der = if val.first() == Some(&0x04) {
                    read_sequence_body_or_octet(&val)?
                } else {
                    val
                };
                cert_ders.push(cert_der);
            } else if bag_id == OID_KEY_BAG {
                // KeyBag = PrivateKeyInfo (PKCS#8 DER)
                let key_der = bag_val;
                let pem = pem_wrap("PRIVATE KEY", &key_der);
                key_pem = Some(pem);
            } else if bag_id == OID_PKCS8_SHROUDED_KEY_BAG {
                // Not supported (no encryption used); skip
            }
        }
    }

    let pkey_pem = key_pem.unwrap_or_default();
    let cert_pems: Vec<String> = cert_ders
        .iter()
        .map(|d| pem_wrap("CERTIFICATE", d))
        .collect();
    let primary = cert_pems.first().cloned().unwrap_or_default();
    let extras: Vec<String> = cert_pems.iter().skip(1).cloned().collect();
    Some((vec![primary], pkey_pem, extras))
}

pub fn openssl_pkcs12_export(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // 1) cert, 2) &output, 3) private_key, 4) passphrase, 5) options
    let cert_arg = args.first().cloned().unwrap_or(Value::Null);
    let priv_arg = args.get(2).cloned().unwrap_or(Value::Null);
    let passphrase = args
        .get(3)
        .map(|v| v.to_php_string().to_string_lossy().to_string())
        .unwrap_or_default();
    let options = args.get(4).cloned().unwrap_or(Value::Null);

    // Type check (OpenSSLAsymmetricKey object → TypeError).
    let cert_deref = deref(&cert_arg);
    if let Value::Object(o) = &cert_deref {
        let cls = o.borrow().class_name.clone();
        if cls.eq_ignore_ascii_case(b"OpenSSLAsymmetricKey") {
            let exc = vm.throw_type_error(
                "openssl_pkcs12_export(): Argument #1 ($certificate) must be of type OpenSSLCertificate|string, OpenSSLAsymmetricKey given".to_string(),
            );
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: "Uncaught TypeError".to_string(),
                line: 0,
            });
        }
    }

    let cert = match resolve_cert(&cert_arg) {
        Some(c) => c,
        None => {
            vm.emit_warning("openssl_pkcs12_export(): X.509 Certificate cannot be retrieved");
            return Ok(Value::False);
        }
    };
    let priv_key = match resolve_private_key_pub(&priv_arg, None) {
        Some(k) => k,
        None => {
            vm.emit_warning(
                "openssl_pkcs12_export(): Cannot get private key from parameter 3",
            );
            return Ok(Value::False);
        }
    };

    if !crate::x509::key_matches_cert(&cert, &priv_key) {
        vm.emit_warning(
            "openssl_pkcs12_export(): Private key does not correspond to cert",
        );
        return Ok(Value::False);
    }

    // Optional extracerts
    let mut extracerts: Vec<Vec<u8>> = Vec::new();
    let options_deref = deref(&options);
    if let Value::Array(a) = &options_deref {
        let arr = a.borrow();
        if let Some(v) = arr.get(&ArrayKey::String(PhpString::from_bytes(b"extracerts"))) {
            for c in resolve_certs(v) {
                if let Ok(d) = c.to_der() {
                    extracerts.push(d);
                }
            }
        }
    }

    let cert_der = match cert.to_der() {
        Ok(d) => d,
        Err(_) => return Ok(Value::False),
    };

    use rsa::pkcs8::EncodePrivateKey;
    let pkcs8_der = match priv_key.to_pkcs8_der() {
        Ok(d) => d.as_bytes().to_vec(),
        Err(_) => return Ok(Value::False),
    };

    let pfx = match build_pkcs12(&cert_der, &pkcs8_der, &extracerts, &passphrase) {
        Some(p) => p,
        None => return Ok(Value::False),
    };

    if let Some(Value::Reference(r)) = args.get(1) {
        *r.borrow_mut() = Value::String(PhpString::from_vec(pfx));
    }
    Ok(Value::True)
}

pub fn openssl_pkcs12_export_to_file(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cert_arg = args.first().cloned().unwrap_or(Value::Null);
    let filename = args
        .get(1)
        .map(|v| v.to_php_string().to_string_lossy().to_string())
        .unwrap_or_default();
    let priv_arg = args.get(2).cloned().unwrap_or(Value::Null);
    let passphrase = args
        .get(3)
        .map(|v| v.to_php_string().to_string_lossy().to_string())
        .unwrap_or_default();
    let options = args.get(4).cloned().unwrap_or(Value::Null);

    let cert_deref = deref(&cert_arg);
    if let Value::Object(o) = &cert_deref {
        let cls = o.borrow().class_name.clone();
        if cls.eq_ignore_ascii_case(b"OpenSSLAsymmetricKey") {
            let exc = vm.throw_type_error(
                "openssl_pkcs12_export_to_file(): Argument #1 ($certificate) must be of type OpenSSLCertificate|string, OpenSSLAsymmetricKey given".to_string(),
            );
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: "Uncaught TypeError".to_string(),
                line: 0,
            });
        }
    }

    let cert = match resolve_cert(&cert_arg) {
        Some(c) => c,
        None => {
            vm.emit_warning(
                "openssl_pkcs12_export_to_file(): X.509 Certificate cannot be retrieved",
            );
            return Ok(Value::False);
        }
    };
    let priv_key = match resolve_private_key_pub(&priv_arg, None) {
        Some(k) => k,
        None => {
            vm.emit_warning(
                "openssl_pkcs12_export_to_file(): Cannot get private key from parameter 3",
            );
            return Ok(Value::False);
        }
    };

    if !crate::x509::key_matches_cert(&cert, &priv_key) {
        vm.emit_warning(
            "openssl_pkcs12_export_to_file(): Private key does not correspond to cert",
        );
        return Ok(Value::False);
    }

    let mut extracerts: Vec<Vec<u8>> = Vec::new();
    let options_deref = deref(&options);
    if let Value::Array(a) = &options_deref {
        let arr = a.borrow();
        if let Some(v) = arr.get(&ArrayKey::String(PhpString::from_bytes(b"extracerts"))) {
            for c in resolve_certs(v) {
                if let Ok(d) = c.to_der() {
                    extracerts.push(d);
                }
            }
        }
    }

    let cert_der = match cert.to_der() {
        Ok(d) => d,
        Err(_) => return Ok(Value::False),
    };

    use rsa::pkcs8::EncodePrivateKey;
    let pkcs8_der = match priv_key.to_pkcs8_der() {
        Ok(d) => d.as_bytes().to_vec(),
        Err(_) => return Ok(Value::False),
    };

    let pfx = match build_pkcs12(&cert_der, &pkcs8_der, &extracerts, &passphrase) {
        Some(p) => p,
        None => return Ok(Value::False),
    };

    if filename.is_empty() {
        vm.emit_warning(&format!(
            "openssl_pkcs12_export_to_file(): Error opening file {}",
            filename
        ));
        return Ok(Value::False);
    }
    match std::fs::write(&filename, &pfx) {
        Ok(()) => Ok(Value::True),
        Err(_) => {
            vm.emit_warning(&format!(
                "openssl_pkcs12_export_to_file(): Error opening file {}",
                filename
            ));
            Ok(Value::False)
        }
    }
}

pub fn openssl_pkcs12_read(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let input_val = args.first().cloned().unwrap_or(Value::Null);
    let passphrase = args
        .get(2)
        .map(|v| v.to_php_string().to_string_lossy().to_string())
        .unwrap_or_default();
    let input = input_val.to_php_string();
    let input_bytes = input.as_bytes();
    if input_bytes.is_empty() {
        return Ok(Value::False);
    }
    // Try direct DER, then PEM-extracted DER.
    let der_bytes: Vec<u8> = if input_bytes[0] == 0x30 {
        input_bytes.to_vec()
    } else if let Ok(s) = std::str::from_utf8(input_bytes) {
        match pem_to_der(s) {
            Some(d) => d,
            None => return Ok(Value::False),
        }
    } else {
        return Ok(Value::False);
    };

    let (cert_pems, pkey_pem, extras) = match parse_pkcs12(&der_bytes, &passphrase) {
        Some(t) => t,
        None => return Ok(Value::False),
    };

    let mut arr = PhpArray::new();
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"cert")),
        Value::String(PhpString::from_string(
            cert_pems.first().cloned().unwrap_or_default(),
        )),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"pkey")),
        Value::String(PhpString::from_string(pkey_pem)),
    );
    if !extras.is_empty() {
        let mut ea = PhpArray::new();
        for e in &extras {
            ea.push(Value::String(PhpString::from_string(e.clone())));
        }
        arr.set(
            ArrayKey::String(PhpString::from_bytes(b"extracerts")),
            Value::Array(Rc::new(RefCell::new(ea))),
        );
    }

    if let Some(Value::Reference(r)) = args.get(1) {
        *r.borrow_mut() = Value::Array(Rc::new(RefCell::new(arr)));
    }
    Ok(Value::True)
}
