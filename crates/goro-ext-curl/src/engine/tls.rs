//! TLS client layer for the libcurl emulation, on top of the pure-Rust
//! `purecrypto` crate.
//!
//! It reproduces what a libcurl 8.14.1 user observes from the OpenSSL
//! backend (`lib/vtls/openssl.c`, `vtls.c`, `hostcheck.c`): the CURLcode and
//! error-buffer text of every failure, the verbose (`infof`) lines and the
//! `CURLOPT_DEBUGFUNCTION` TLS trace, `CURLINFO_SSL_VERIFYRESULT` (OpenSSL
//! `X509_V_*` numbers) and `CURLINFO_CERTINFO`.
//!
//! The session is sans-IO: the caller feeds socket bytes in with
//! [`TlsSession::feed`] / [`TlsSession::feed_eof`] and writes out what
//! [`TlsSession::take_outgoing`] returns.
//!
//! The peer chain is verified by this module (an `X509_verify_cert` work
//! alike with `X509_V_FLAG_TRUSTED_FIRST | X509_V_FLAG_PARTIAL_CHAIN`, CA
//! file / CA path (hashed directory) / CA blob lookups, CRLs), as soon as
//! the server `Certificate` message has been processed and before anything
//! more is sent, like `SSL_VERIFY_PEER` does.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use purecrypto::tls::{Config, ConfigBuilder, Connection, KeyLog, NamedGroup, ProtocolVersion};

// ===========================================================================
// Public API
// ===========================================================================

/// The `CURLOPT_*` SSL options libcurl hands to the TLS backend.
#[derive(Clone, Debug)]
pub struct TlsConfig {
    /// CURLOPT_SSL_VERIFYPEER
    pub verify_peer: bool,
    /// CURLOPT_SSL_VERIFYHOST (1 and 2 both mean "on")
    pub verify_host: bool,
    /// CURLOPT_SSL_VERIFYSTATUS
    pub verify_status: bool,
    /// CURLOPT_CAINFO. `None` means no CA file; libcurl's default is
    /// [`default_cainfo`] (what [`TlsConfig::default`] puts here).
    pub cainfo: Option<PathBuf>,
    /// CURLOPT_CAPATH (default [`default_capath`]).
    pub capath: Option<PathBuf>,
    /// CURLOPT_CAINFO_BLOB (overrides `cainfo`).
    pub ca_blob: Option<Vec<u8>>,
    /// CURLOPT_SSLCERT: a file name (or an engine/provider "nickname").
    pub cert: Option<Vec<u8>>,
    /// CURLOPT_SSLCERTTYPE: "PEM" (default), "DER", "P12", "ENG", "PROV".
    pub cert_type: Option<String>,
    /// CURLOPT_SSLCERT_BLOB
    pub cert_blob: Option<Vec<u8>>,
    /// CURLOPT_SSLKEY
    pub key: Option<PathBuf>,
    /// CURLOPT_SSLKEYTYPE: "PEM" (default), "DER", "ENG", "PROV".
    pub key_type: Option<String>,
    /// CURLOPT_SSLKEY_BLOB
    pub key_blob: Option<Vec<u8>>,
    /// CURLOPT_KEYPASSWD
    pub key_passwd: Option<Vec<u8>>,
    /// CURLOPT_ISSUERCERT
    pub issuercert: Option<PathBuf>,
    /// CURLOPT_ISSUERCERT_BLOB
    pub issuercert_blob: Option<Vec<u8>>,
    /// CURLOPT_CRLFILE
    pub crlfile: Option<PathBuf>,
    /// CURL_SSLVERSION_* (the low 16 bits of CURLOPT_SSLVERSION; a full
    /// CURLOPT_SSLVERSION value with the MAX bits is accepted too).
    pub version_min: i64,
    /// CURL_SSLVERSION_MAX_* (already shifted, e.g. `CURL_SSLVERSION_MAX_TLSv1_2`
    /// = 6 << 16). 0 = take the max bits of `version_min`.
    pub version_max: i64,
    /// CURLOPT_SSL_CIPHER_LIST (OpenSSL cipher-list syntax, TLS <= 1.2).
    pub cipher_list: Option<String>,
    /// CURLOPT_TLS13_CIPHERS
    pub tls13_ciphers: Option<String>,
    /// CURLOPT_SSL_EC_CURVES
    pub curves: Option<String>,
    /// CURLOPT_SSL_SIGNATURE_ALGORITHMS
    pub signature_algorithms: Option<String>,
    /// CURLOPT_PINNEDPUBLICKEY
    pub pinned_pubkey: Option<String>,
    /// CURLOPT_SSL_OPTIONS (CURLSSLOPT_* bits; NO_PARTIALCHAIN is honoured)
    pub ssl_options: i64,
    /// CURLOPT_CERTINFO
    pub certinfo: bool,
    /// The ALPN protocols curl offers, e.g. `["http/1.1"]`; empty = no ALPN.
    pub alpn: Vec<String>,
    /// CURLOPT_SSL_SESSIONID_CACHE (informational: the caller owns the
    /// session cache, see [`TlsSession::take_resumption`]).
    pub sessionid_cache: bool,
    /// Produce the `CURLOPT_DEBUGFUNCTION` TLS trace ([`TlsLog::Trace`],
    /// [`TlsLog::DataIn`], [`TlsLog::DataOut`]). libcurl installs its
    /// OpenSSL message callback only when `CURLOPT_VERBOSE` is set *and* a
    /// debug function is installed.
    pub trace: bool,
    /// This is the TLS layer to an HTTPS proxy ("Proxy certificate:").
    pub proxy: bool,
}

impl Default for TlsConfig {
    fn default() -> Self {
        TlsConfig {
            verify_peer: true,
            verify_host: true,
            verify_status: false,
            cainfo: default_cainfo().map(PathBuf::from),
            capath: default_capath().map(PathBuf::from),
            ca_blob: None,
            cert: None,
            cert_type: None,
            cert_blob: None,
            key: None,
            key_type: None,
            key_blob: None,
            key_passwd: None,
            issuercert: None,
            issuercert_blob: None,
            crlfile: None,
            version_min: 0,
            version_max: 0,
            cipher_list: None,
            tls13_ciphers: None,
            curves: None,
            signature_algorithms: None,
            pinned_pubkey: None,
            ssl_options: 0,
            certinfo: false,
            alpn: Vec::new(),
            sessionid_cache: true,
            trace: false,
            proxy: false,
        }
    }
}

/// One verbose event, in the order libcurl would emit it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TlsLog {
    /// An `infof()` line (CURLINFO_TEXT; shown with CURLOPT_VERBOSE), without
    /// the "* " prefix and the trailing newline.
    Info(String),
    /// A TLS trace line from OpenSSL's message callback ("TLSv1.3 (OUT), TLS
    /// handshake, Client hello (1):"), CURLINFO_TEXT, only produced with
    /// [`TlsConfig::trace`].
    Trace(String),
    /// CURLINFO_SSL_DATA_IN payload (record headers, handshake messages...).
    DataIn(Vec<u8>),
    /// CURLINFO_SSL_DATA_OUT payload.
    DataOut(Vec<u8>),
}

/// A resumable session (libcurl's SSL session cache entry), from
/// [`TlsSession::take_resumption`] and given back to
/// [`TlsSession::new_with_resumption`] for the same peer.
#[derive(Clone)]
pub struct TlsResumption {
    session: purecrypto::tls::ResumptionSession,
    alpn: Option<String>,
    peer_chain: Vec<Vec<u8>>,
    verify_result: i64,
    verified_chain: Vec<Vec<u8>>,
}

impl std::fmt::Debug for TlsResumption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsResumption")
            .field("alpn", &self.alpn)
            .finish()
    }
}

/// libcurl's CURLINFO_CAINFO default (the CURL_CA_BUNDLE of the 8.14 build).
pub fn default_cainfo() -> Option<String> {
    Some("/etc/ssl/certs/ca-certificates.crt".to_string())
}

/// libcurl's CURLINFO_CAPATH default (CURL_CA_PATH).
pub fn default_capath() -> Option<String> {
    Some("/etc/ssl/certs".to_string())
}

/// The TLS library version string for curl_version() ("OpenSSL/3.5.7" for
/// the real thing).
pub fn backend_version() -> String {
    BACKEND_VERSION.to_string()
}

const BACKEND_VERSION: &str = "purecrypto/0.10.1";

// CURLcode values used here.
const E_NOT_BUILT_IN: u32 = 4;
const E_OUT_OF_MEMORY: u32 = 27;
const E_SSL_CONNECT_ERROR: u32 = 35;
const E_SEND_ERROR: u32 = 55;
const E_RECV_ERROR: u32 = 56;
const E_SSL_CERTPROBLEM: u32 = 58;
const E_SSL_CIPHER: u32 = 59;
const E_PEER_FAILED_VERIFICATION: u32 = 60;
const E_SSL_CACERT_BADFILE: u32 = 77;
const E_SSL_CRL_BADFILE: u32 = 82;
const E_SSL_ISSUER_ERROR: u32 = 83;
const E_SSL_PINNEDPUBKEYNOTMATCH: u32 = 90;
const E_SSL_INVALIDCERTSTATUS: u32 = 91;
const E_SSL_CLIENTCERT: u32 = 98;

type TlsErr = (u32, String);

// ===========================================================================
// DER
// ===========================================================================

const T_BOOLEAN: u8 = 0x01;
const T_INTEGER: u8 = 0x02;
const T_BIT_STRING: u8 = 0x03;
const T_OCTET_STRING: u8 = 0x04;
const T_NULL: u8 = 0x05;
const T_OID: u8 = 0x06;
const T_UTF8: u8 = 0x0c;
const T_PRINTABLE: u8 = 0x13;
const T_T61: u8 = 0x14;
const T_IA5: u8 = 0x16;
const T_UTCTIME: u8 = 0x17;
const T_GENTIME: u8 = 0x18;
const T_VISIBLE: u8 = 0x1a;
const T_UNIVERSAL: u8 = 0x1c;
const T_BMP: u8 = 0x1e;
const T_SEQ: u8 = 0x30;
const T_SET: u8 = 0x31;

#[derive(Clone, Copy, Debug)]
struct Tlv<'a> {
    tag: u8,
    content: &'a [u8],
    raw: &'a [u8],
}

impl<'a> Tlv<'a> {
    fn children(&self) -> Option<Vec<Tlv<'a>>> {
        der_list(self.content)
    }
    /// Context-specific tag number (constructed or primitive).
    fn ctx(&self) -> Option<u8> {
        if self.tag & 0xc0 == 0x80 {
            Some(self.tag & 0x1f)
        } else {
            None
        }
    }
    fn bits(&self) -> Option<&'a [u8]> {
        if self.tag != T_BIT_STRING || self.content.is_empty() {
            return None;
        }
        Some(&self.content[1..])
    }
}

fn der_read(input: &[u8]) -> Option<(Tlv<'_>, &[u8])> {
    if input.len() < 2 {
        return None;
    }
    let tag = input[0];
    if tag & 0x1f == 0x1f {
        return None;
    }
    let (len, hdr) = if input[1] < 0x80 {
        (input[1] as usize, 2)
    } else {
        let n = (input[1] & 0x7f) as usize;
        if n == 0 || n > 4 || input.len() < 2 + n {
            return None;
        }
        let mut l = 0usize;
        for &b in &input[2..2 + n] {
            l = (l << 8) | b as usize;
        }
        (l, 2 + n)
    };
    if input.len() < hdr + len {
        return None;
    }
    Some((
        Tlv {
            tag,
            content: &input[hdr..hdr + len],
            raw: &input[..hdr + len],
        },
        &input[hdr + len..],
    ))
}

fn der_one(input: &[u8]) -> Option<Tlv<'_>> {
    der_read(input).map(|(t, _)| t)
}

fn der_list(mut input: &[u8]) -> Option<Vec<Tlv<'_>>> {
    let mut v = Vec::new();
    while !input.is_empty() {
        let (t, rest) = der_read(input)?;
        v.push(t);
        input = rest;
    }
    Some(v)
}

fn der_len(n: usize) -> Vec<u8> {
    if n < 0x80 {
        vec![n as u8]
    } else if n < 0x100 {
        vec![0x81, n as u8]
    } else if n < 0x10000 {
        vec![0x82, (n >> 8) as u8, n as u8]
    } else {
        vec![0x83, (n >> 16) as u8, (n >> 8) as u8, n as u8]
    }
}

fn der_tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut v = vec![tag];
    v.extend(der_len(content.len()));
    v.extend_from_slice(content);
    v
}

fn der_seq(parts: &[&[u8]]) -> Vec<u8> {
    der_tlv(T_SEQ, &parts.concat())
}

fn oid_der(dotted: &str) -> Vec<u8> {
    let arcs: Vec<u64> = dotted.split('.').filter_map(|a| a.parse().ok()).collect();
    let mut body = Vec::new();
    if arcs.len() >= 2 {
        let mut push = |mut v: u64| {
            let mut tmp = vec![(v & 0x7f) as u8];
            v >>= 7;
            while v > 0 {
                tmp.push(0x80 | (v & 0x7f) as u8);
                v >>= 7;
            }
            tmp.reverse();
            body.extend(tmp);
        };
        push(arcs[0] * 40 + arcs[1]);
        for &a in &arcs[2..] {
            push(a);
        }
    }
    der_tlv(T_OID, &body)
}

/// Dotted form of an OBJECT IDENTIFIER's content octets.
fn oid_str(content: &[u8]) -> String {
    let mut arcs: Vec<u64> = Vec::new();
    let mut v: u64 = 0;
    for &b in content {
        v = (v << 7) | (b & 0x7f) as u64;
        if b & 0x80 == 0 {
            if arcs.is_empty() {
                let first = if v < 40 {
                    0
                } else if v < 80 {
                    1
                } else {
                    2
                };
                arcs.push(first);
                arcs.push(v - first * 40);
            } else {
                arcs.push(v);
            }
            v = 0;
        }
    }
    arcs.iter()
        .map(|a| a.to_string())
        .collect::<Vec<_>>()
        .join(".")
}

fn tlv_oid(t: &Tlv) -> Option<String> {
    if t.tag != T_OID {
        return None;
    }
    Some(oid_str(t.content))
}

/// INTEGER content -> u64 (small values only).
fn int_u64(content: &[u8]) -> Option<u64> {
    if content.is_empty() || content.len() > 9 || content[0] & 0x80 != 0 {
        return None;
    }
    let mut v: u64 = 0;
    for &b in content {
        v = v.checked_mul(256)? + b as u64;
    }
    Some(v)
}

// ===========================================================================
// Base64 / PEM
// ===========================================================================

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn b64_decode(s: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut n = 0;
    let mut pad = 0;
    for &c in s {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                pad += 1;
                0
            }
            b' ' | b'\t' | b'\r' | b'\n' => continue,
            _ => return None,
        };
        if pad > 0 && c != b'=' {
            return None;
        }
        acc = (acc << 6) | v as u32;
        n += 1;
        if n == 4 {
            out.push((acc >> 16) as u8);
            out.push((acc >> 8) as u8);
            out.push(acc as u8);
            acc = 0;
            n = 0;
        }
    }
    if n != 0 || pad > 2 {
        return None;
    }
    out.truncate(out.len() - pad);
    Some(out)
}

#[derive(Clone, Debug)]
struct PemBlock {
    label: String,
    headers: Vec<(String, String)>,
    der: Vec<u8>,
}

/// All PEM blocks of `data` (undecodable blocks are skipped).
fn pem_blocks(data: &[u8]) -> Vec<PemBlock> {
    let text = String::from_utf8_lossy(data);
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let line = line.trim_end();
        let Some(rest) = line.strip_prefix("-----BEGIN ") else {
            continue;
        };
        let Some(label) = rest.strip_suffix("-----") else {
            continue;
        };
        let end = format!("-----END {}-----", label);
        let mut headers = Vec::new();
        let mut body = String::new();
        let mut ok = false;
        let mut in_headers = true;
        for l in lines.by_ref() {
            let l = l.trim();
            if l == end {
                ok = true;
                break;
            }
            if in_headers {
                if let Some((k, v)) = l.split_once(':') {
                    headers.push((k.trim().to_string(), v.trim().to_string()));
                    continue;
                }
                in_headers = false;
                if l.is_empty() {
                    continue;
                }
            }
            body.push_str(l);
        }
        if !ok {
            break;
        }
        if let Some(der) = b64_decode(body.as_bytes()) {
            out.push(PemBlock {
                label: label.to_string(),
                headers,
                der,
            });
        }
    }
    out
}

fn pem_encode(label: &str, der: &[u8]) -> String {
    let b = b64_encode(der);
    let mut s = format!("-----BEGIN {}-----\n", label);
    for chunk in b.as_bytes().chunks(64) {
        s.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        s.push('\n');
    }
    s.push_str(&format!("-----END {}-----\n", label));
    s
}

fn is_cert_label(l: &str) -> bool {
    l == "CERTIFICATE" || l == "X509 CERTIFICATE" || l == "TRUSTED CERTIFICATE"
}

fn hex_upper(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02X}", b)).collect()
}

fn hex_colon_upper(data: &[u8]) -> String {
    data.iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(":")
}

// ===========================================================================
// Object names (OpenSSL's obj_dat.h subset): dotted -> (short, long)
// ===========================================================================

const OIDS: &[(&str, &str, &str)] = &[
    // DN attributes
    ("2.5.4.3", "CN", "commonName"),
    ("2.5.4.4", "SN", "surname"),
    ("2.5.4.5", "serialNumber", "serialNumber"),
    ("2.5.4.6", "C", "countryName"),
    ("2.5.4.7", "L", "localityName"),
    ("2.5.4.8", "ST", "stateOrProvinceName"),
    ("2.5.4.9", "street", "streetAddress"),
    ("2.5.4.10", "O", "organizationName"),
    ("2.5.4.11", "OU", "organizationalUnitName"),
    ("2.5.4.12", "title", "title"),
    ("2.5.4.13", "description", "description"),
    ("2.5.4.15", "businessCategory", "businessCategory"),
    ("2.5.4.16", "postalAddress", "postalAddress"),
    ("2.5.4.17", "postalCode", "postalCode"),
    ("2.5.4.18", "postOfficeBox", "postOfficeBox"),
    ("2.5.4.41", "name", "name"),
    ("2.5.4.42", "GN", "givenName"),
    ("2.5.4.43", "initials", "initials"),
    ("2.5.4.44", "generationQualifier", "generationQualifier"),
    ("2.5.4.45", "x500UniqueIdentifier", "x500UniqueIdentifier"),
    ("2.5.4.46", "dnQualifier", "dnQualifier"),
    ("2.5.4.65", "pseudonym", "pseudonym"),
    ("2.5.4.72", "role", "role"),
    (
        "2.5.4.97",
        "organizationIdentifier",
        "organizationIdentifier",
    ),
    ("1.2.840.113549.1.9.1", "emailAddress", "emailAddress"),
    (
        "1.2.840.113549.1.9.2",
        "unstructuredName",
        "unstructuredName",
    ),
    ("0.9.2342.19200300.100.1.1", "UID", "userId"),
    ("0.9.2342.19200300.100.1.25", "DC", "domainComponent"),
    (
        "1.3.6.1.4.1.311.60.2.1.1",
        "jurisdictionL",
        "jurisdictionLocalityName",
    ),
    (
        "1.3.6.1.4.1.311.60.2.1.2",
        "jurisdictionST",
        "jurisdictionStateOrProvinceName",
    ),
    (
        "1.3.6.1.4.1.311.60.2.1.3",
        "jurisdictionC",
        "jurisdictionCountryName",
    ),
    // public key / signature algorithms
    ("1.2.840.113549.1.1.1", "rsaEncryption", "rsaEncryption"),
    ("1.2.840.113549.1.1.2", "RSA-MD2", "md2WithRSAEncryption"),
    ("1.2.840.113549.1.1.4", "RSA-MD5", "md5WithRSAEncryption"),
    ("1.2.840.113549.1.1.5", "RSA-SHA1", "sha1WithRSAEncryption"),
    ("1.2.840.113549.1.1.10", "RSASSA-PSS", "rsassaPss"),
    (
        "1.2.840.113549.1.1.11",
        "RSA-SHA256",
        "sha256WithRSAEncryption",
    ),
    (
        "1.2.840.113549.1.1.12",
        "RSA-SHA384",
        "sha384WithRSAEncryption",
    ),
    (
        "1.2.840.113549.1.1.13",
        "RSA-SHA512",
        "sha512WithRSAEncryption",
    ),
    (
        "1.2.840.113549.1.1.14",
        "RSA-SHA224",
        "sha224WithRSAEncryption",
    ),
    ("1.2.840.10040.4.1", "DSA", "dsaEncryption"),
    ("1.2.840.10040.4.3", "DSA-SHA1", "dsaWithSHA1"),
    (
        "2.16.840.1.101.3.4.3.1",
        "dsa_with_SHA224",
        "dsa_with_SHA224",
    ),
    (
        "2.16.840.1.101.3.4.3.2",
        "dsa_with_SHA256",
        "dsa_with_SHA256",
    ),
    ("1.2.840.10046.2.1", "dhpublicnumber", "X9.42 DH"),
    ("1.2.840.113549.1.3.1", "dhKeyAgreement", "dhKeyAgreement"),
    ("1.2.840.10045.2.1", "id-ecPublicKey", "id-ecPublicKey"),
    ("1.2.840.10045.4.1", "ecdsa-with-SHA1", "ecdsa-with-SHA1"),
    (
        "1.2.840.10045.4.3.1",
        "ecdsa-with-SHA224",
        "ecdsa-with-SHA224",
    ),
    (
        "1.2.840.10045.4.3.2",
        "ecdsa-with-SHA256",
        "ecdsa-with-SHA256",
    ),
    (
        "1.2.840.10045.4.3.3",
        "ecdsa-with-SHA384",
        "ecdsa-with-SHA384",
    ),
    (
        "1.2.840.10045.4.3.4",
        "ecdsa-with-SHA512",
        "ecdsa-with-SHA512",
    ),
    (
        "2.16.840.1.101.3.4.3.9",
        "id-ecdsa-with-sha3-224",
        "ecdsa_with_SHA3-224",
    ),
    (
        "2.16.840.1.101.3.4.3.10",
        "id-ecdsa-with-sha3-256",
        "ecdsa_with_SHA3-256",
    ),
    (
        "2.16.840.1.101.3.4.3.11",
        "id-ecdsa-with-sha3-384",
        "ecdsa_with_SHA3-384",
    ),
    (
        "2.16.840.1.101.3.4.3.12",
        "id-ecdsa-with-sha3-512",
        "ecdsa_with_SHA3-512",
    ),
    (
        "2.16.840.1.101.3.4.3.14",
        "id-rsassa-pkcs1-v1_5-with-sha3-256",
        "RSA-SHA3-256",
    ),
    ("1.3.101.110", "X25519", "X25519"),
    ("1.3.101.111", "X448", "X448"),
    ("1.3.101.112", "ED25519", "ED25519"),
    ("1.3.101.113", "ED448", "ED448"),
    ("2.16.840.1.101.3.4.3.17", "id-ml-dsa-44", "ML-DSA-44"),
    ("2.16.840.1.101.3.4.3.18", "id-ml-dsa-65", "ML-DSA-65"),
    ("2.16.840.1.101.3.4.3.19", "id-ml-dsa-87", "ML-DSA-87"),
    // curves
    ("1.2.840.10045.3.1.1", "prime192v1", "prime192v1"),
    ("1.2.840.10045.3.1.7", "prime256v1", "prime256v1"),
    ("1.3.132.0.33", "secp224r1", "secp224r1"),
    ("1.3.132.0.34", "secp384r1", "secp384r1"),
    ("1.3.132.0.35", "secp521r1", "secp521r1"),
    ("1.3.132.0.10", "secp256k1", "secp256k1"),
    ("1.3.36.3.3.2.8.1.1.7", "brainpoolP256r1", "brainpoolP256r1"),
    (
        "1.3.36.3.3.2.8.1.1.11",
        "brainpoolP384r1",
        "brainpoolP384r1",
    ),
    (
        "1.3.36.3.3.2.8.1.1.13",
        "brainpoolP512r1",
        "brainpoolP512r1",
    ),
    // extensions
    (
        "2.5.29.9",
        "subjectDirectoryAttributes",
        "X509v3 Subject Directory Attributes",
    ),
    (
        "2.5.29.14",
        "subjectKeyIdentifier",
        "X509v3 Subject Key Identifier",
    ),
    ("2.5.29.15", "keyUsage", "X509v3 Key Usage"),
    (
        "2.5.29.16",
        "privateKeyUsagePeriod",
        "X509v3 Private Key Usage Period",
    ),
    (
        "2.5.29.17",
        "subjectAltName",
        "X509v3 Subject Alternative Name",
    ),
    (
        "2.5.29.18",
        "issuerAltName",
        "X509v3 Issuer Alternative Name",
    ),
    ("2.5.29.19", "basicConstraints", "X509v3 Basic Constraints"),
    ("2.5.29.20", "crlNumber", "X509v3 CRL Number"),
    ("2.5.29.21", "CRLReason", "X509v3 CRL Reason Code"),
    ("2.5.29.30", "nameConstraints", "X509v3 Name Constraints"),
    (
        "2.5.29.31",
        "crlDistributionPoints",
        "X509v3 CRL Distribution Points",
    ),
    (
        "2.5.29.32",
        "certificatePolicies",
        "X509v3 Certificate Policies",
    ),
    ("2.5.29.32.0", "anyPolicy", "X509v3 Any Policy"),
    ("2.5.29.33", "policyMappings", "X509v3 Policy Mappings"),
    (
        "2.5.29.35",
        "authorityKeyIdentifier",
        "X509v3 Authority Key Identifier",
    ),
    (
        "2.5.29.36",
        "policyConstraints",
        "X509v3 Policy Constraints",
    ),
    ("2.5.29.37", "extendedKeyUsage", "X509v3 Extended Key Usage"),
    ("2.5.29.46", "freshestCRL", "X509v3 Freshest CRL"),
    ("2.5.29.54", "inhibitAnyPolicy", "X509v3 Inhibit Any Policy"),
    ("2.5.29.55", "targetInformation", "X509v3 AC Targeting"),
    ("2.5.29.56", "noRevAvail", "X509v3 No Revocation Available"),
    (
        "1.3.6.1.5.5.7.1.1",
        "authorityInfoAccess",
        "Authority Information Access",
    ),
    (
        "1.3.6.1.5.5.7.1.11",
        "subjectInfoAccess",
        "Subject Information Access",
    ),
    ("1.3.6.1.5.5.7.1.24", "tlsfeature", "TLS Feature"),
    ("1.3.6.1.5.5.7.1.3", "qcStatements", "qcStatements"),
    (
        "1.3.6.1.4.1.11129.2.4.2",
        "ct_precert_scts",
        "CT Precertificate SCTs",
    ),
    (
        "1.3.6.1.4.1.11129.2.4.3",
        "ct_precert_poison",
        "CT Precertificate Poison",
    ),
    (
        "1.3.6.1.4.1.11129.2.4.4",
        "ct_precert_signer",
        "CT Precertificate Signer",
    ),
    (
        "1.3.6.1.4.1.11129.2.4.5",
        "ct_cert_scts",
        "CT Certificate SCTs",
    ),
    ("2.16.840.1.113730.1.1", "nsCertType", "Netscape Cert Type"),
    ("2.16.840.1.113730.1.2", "nsBaseUrl", "Netscape Base Url"),
    (
        "2.16.840.1.113730.1.3",
        "nsRevocationUrl",
        "Netscape Revocation Url",
    ),
    (
        "2.16.840.1.113730.1.4",
        "nsCaRevocationUrl",
        "Netscape CA Revocation Url",
    ),
    (
        "2.16.840.1.113730.1.7",
        "nsRenewalUrl",
        "Netscape Renewal Url",
    ),
    (
        "2.16.840.1.113730.1.8",
        "nsCaPolicyUrl",
        "Netscape CA Policy Url",
    ),
    (
        "2.16.840.1.113730.1.12",
        "nsSslServerName",
        "Netscape SSL Server Name",
    ),
    ("2.16.840.1.113730.1.13", "nsComment", "Netscape Comment"),
    ("1.3.6.1.5.5.7.48.1.5", "noCheck", "OCSP No Check"),
    (
        "1.3.6.1.4.1.311.20.2",
        "msCertTemplateName?",
        "Microsoft Certificate Template Name",
    ),
    (
        "1.3.6.1.4.1.311.21.1",
        "msCAVersion?",
        "Microsoft CA Version",
    ),
    // EKU purposes
    (
        "1.3.6.1.5.5.7.3.1",
        "serverAuth",
        "TLS Web Server Authentication",
    ),
    (
        "1.3.6.1.5.5.7.3.2",
        "clientAuth",
        "TLS Web Client Authentication",
    ),
    ("1.3.6.1.5.5.7.3.3", "codeSigning", "Code Signing"),
    ("1.3.6.1.5.5.7.3.4", "emailProtection", "E-mail Protection"),
    ("1.3.6.1.5.5.7.3.5", "ipsecEndSystem", "IPSec End System"),
    ("1.3.6.1.5.5.7.3.6", "ipsecTunnel", "IPSec Tunnel"),
    ("1.3.6.1.5.5.7.3.7", "ipsecUser", "IPSec User"),
    ("1.3.6.1.5.5.7.3.8", "timeStamping", "Time Stamping"),
    ("1.3.6.1.5.5.7.3.9", "OCSPSigning", "OCSP Signing"),
    (
        "1.3.6.1.5.5.7.3.17",
        "ipsecIKE",
        "ipsec Internet Key Exchange",
    ),
    (
        "2.5.29.37.0",
        "anyExtendedKeyUsage",
        "Any Extended Key Usage",
    ),
    (
        "1.3.6.1.4.1.311.10.3.3",
        "msSGC",
        "Microsoft Server Gated Crypto",
    ),
    (
        "2.16.840.1.113730.4.1",
        "nsSGC",
        "Netscape Server Gated Crypto",
    ),
    (
        "1.3.6.1.4.1.311.2.1.21",
        "msCodeInd",
        "Microsoft Individual Code Signing",
    ),
    (
        "1.3.6.1.4.1.311.2.1.22",
        "msCodeCom",
        "Microsoft Commercial Code Signing",
    ),
    (
        "1.3.6.1.4.1.311.10.3.1",
        "msCTLSign",
        "Microsoft Trust List Signing",
    ),
    (
        "1.3.6.1.4.1.311.10.3.4",
        "msEFS",
        "Microsoft Encrypted File System",
    ),
    (
        "1.3.6.1.4.1.311.20.2.2",
        "msSmartcardLogin",
        "Microsoft Smartcard Login",
    ),
    (
        "1.3.6.1.4.1.311.20.2.3",
        "msUPN",
        "Microsoft User Principal Name",
    ),
    // access methods / qualifiers
    ("1.3.6.1.5.5.7.48.1", "OCSP", "OCSP"),
    ("1.3.6.1.5.5.7.48.2", "caIssuers", "CA Issuers"),
    ("1.3.6.1.5.5.7.48.3", "ad_timestamping", "AD Time Stamping"),
    ("1.3.6.1.5.5.7.48.5", "caRepository", "CA Repository"),
    ("1.3.6.1.5.5.7.2.1", "id-qt-cps", "Policy Qualifier CPS"),
    (
        "1.3.6.1.5.5.7.2.2",
        "id-qt-unotice",
        "Policy Qualifier User Notice",
    ),
    // hashes
    ("1.3.14.3.2.26", "SHA1", "sha1"),
    ("2.16.840.1.101.3.4.2.1", "SHA256", "sha256"),
    ("2.16.840.1.101.3.4.2.2", "SHA384", "sha384"),
    ("2.16.840.1.101.3.4.2.3", "SHA512", "sha512"),
    ("2.16.840.1.101.3.4.2.4", "SHA224", "sha224"),
    ("1.2.840.113549.2.5", "MD5", "md5"),
];

fn oid_lookup(dotted: &str) -> Option<(&'static str, &'static str)> {
    OIDS.iter()
        .find(|(o, _, _)| *o == dotted)
        .map(|(_, s, l)| (*s, *l))
}

/// OBJ_obj2txt(no_name = 0): the long name, else the dotted form.
fn oid_ln(dotted: &str) -> String {
    match oid_lookup(dotted) {
        Some((_, l)) => l.to_string(),
        None => dotted.to_string(),
    }
}

/// OBJ_nid2sn() or the dotted form.
fn oid_sn(dotted: &str) -> String {
    match oid_lookup(dotted) {
        Some((s, _)) => s.to_string(),
        None => dotted.to_string(),
    }
}

// ===========================================================================
// X509_NAME
// ===========================================================================

#[derive(Clone, Debug)]
struct NameEntry {
    oid: String,
    oid_raw: Vec<u8>,
    tag: u8,
    value: Vec<u8>,
    /// Index of the RDN (SET) this entry belongs to.
    set: usize,
}

#[derive(Clone, Debug, Default)]
struct Name {
    entries: Vec<NameEntry>,
    canon: Vec<u8>,
}

impl Name {
    fn parse(t: &Tlv) -> Option<Name> {
        if t.tag != T_SEQ {
            return None;
        }
        let mut entries = Vec::new();
        for (i, rdn) in t.children()?.iter().enumerate() {
            if rdn.tag != T_SET {
                return None;
            }
            for atv in rdn.children()? {
                let c = atv.children()?;
                if c.len() != 2 {
                    return None;
                }
                entries.push(NameEntry {
                    oid: tlv_oid(&c[0])?,
                    oid_raw: c[0].raw.to_vec(),
                    tag: c[1].tag,
                    value: c[1].content.to_vec(),
                    set: i,
                });
            }
        }
        let mut n = Name {
            entries,
            canon: Vec::new(),
        };
        n.canon = n.canonical();
        Some(n)
    }

    /// x509_name_canon(): what X509_NAME_cmp() compares and
    /// X509_NAME_hash() digests.
    fn canonical(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.entries.len() {
            let set = self.entries[i].set;
            let mut members: Vec<Vec<u8>> = Vec::new();
            while i < self.entries.len() && self.entries[i].set == set {
                let e = &self.entries[i];
                let val = match canon_value(e.tag, &e.value) {
                    Some(v) => der_tlv(T_UTF8, &v),
                    None => der_tlv(e.tag, &e.value),
                };
                members.push(der_seq(&[&e.oid_raw, &val]));
                i += 1;
            }
            members.sort();
            out.extend(der_tlv(T_SET, &members.concat()));
        }
        out
    }

    /// X509_NAME_hash_ex(): the CApath file name hash.
    fn hash(&self) -> u32 {
        let h = purecrypto::hash::sha1(&self.canon);
        u32::from_le_bytes([h[0], h[1], h[2], h[3]])
    }

    fn eq(&self, other: &Name) -> bool {
        self.canon == other.canon
    }

    /// The value of the last entry of type `oid` (the "most significant").
    fn last(&self, oid: &str) -> Option<&NameEntry> {
        self.entries.iter().rev().find(|e| e.oid == oid)
    }
}

/// Code points of an ASN.1 string, by tag (latin-1 for 8-bit types).
fn string_code_points(tag: u8, v: &[u8]) -> Option<Vec<u32>> {
    match tag {
        T_UTF8 => Some(
            String::from_utf8(v.to_vec())
                .ok()?
                .chars()
                .map(|c| c as u32)
                .collect(),
        ),
        T_BMP => {
            if !v.len().is_multiple_of(2) {
                return None;
            }
            Some(
                v.chunks(2)
                    .map(|c| ((c[0] as u32) << 8) | c[1] as u32)
                    .collect(),
            )
        }
        T_UNIVERSAL => {
            if !v.len().is_multiple_of(4) {
                return None;
            }
            Some(
                v.chunks(4)
                    .map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]]))
                    .collect(),
            )
        }
        _ => Some(v.iter().map(|&b| b as u32).collect()),
    }
}

/// asn1_string_canon(): UTF-8, trimmed, inner whitespace collapsed,
/// ASCII-lowercased; None for types copied as-is.
fn canon_value(tag: u8, v: &[u8]) -> Option<Vec<u8>> {
    if !matches!(
        tag,
        T_UTF8 | T_BMP | T_UNIVERSAL | T_PRINTABLE | T_T61 | T_IA5 | T_VISIBLE
    ) {
        return None;
    }
    let cps = string_code_points(tag, v)?;
    let s: String = cps.iter().filter_map(|&c| char::from_u32(c)).collect();
    let b = s.as_bytes();
    let is_space = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r');
    let mut start = 0;
    let mut end = b.len();
    while start < end && is_space(b[start]) {
        start += 1;
    }
    while end > start && is_space(b[end - 1]) {
        end -= 1;
    }
    let mut out = Vec::with_capacity(end - start);
    let mut i = start;
    while i < end {
        if is_space(b[i]) {
            out.push(b' ');
            while i < end && is_space(b[i]) {
                i += 1;
            }
        } else {
            out.push(b[i].to_ascii_lowercase());
            i += 1;
        }
    }
    Some(out)
}

/// ASN.1 string type -> character width for do_print_ex (tag2nbyte).
fn tag2nbyte(tag: u8) -> i32 {
    match tag {
        T_UTF8 => 0,
        0x12 | T_PRINTABLE | T_T61 | T_IA5 | T_UTCTIME | T_GENTIME | T_VISIBLE => 1,
        T_UNIVERSAL => 4,
        T_BMP => 2,
        _ => -1,
    }
}

/// X509_NAME_print_ex() flavours curl uses.
#[derive(Clone, Copy, PartialEq)]
enum NameFmt {
    /// XN_FLAG_SEP_SPLUS_SPC (curl's verbose " subject: ").
    SplusSpc,
    /// XN_FLAG_ONELINE (CERTINFO "Subject:").
    Oneline,
}

fn name_print(n: &Name, fmt: NameFmt) -> Vec<u8> {
    let (sep_dn, sep_mv, eq): (&[u8], &[u8], &[u8]) = match fmt {
        NameFmt::SplusSpc => (b"; ", b" + ", b"="),
        NameFmt::Oneline => (b", ", b" + ", b" = "),
    };
    let mut out = Vec::new();
    for (i, e) in n.entries.iter().enumerate() {
        if i > 0 {
            if e.set == n.entries[i - 1].set {
                out.extend_from_slice(sep_mv);
            } else {
                out.extend_from_slice(sep_dn);
            }
        }
        out.extend_from_slice(oid_sn(&e.oid).as_bytes());
        out.extend_from_slice(eq);
        match fmt {
            NameFmt::SplusSpc => print_value_raw(&mut out, e.tag, &e.value),
            NameFmt::Oneline => print_value_rfc2253(&mut out, e.tag, &e.value),
        }
    }
    out
}

/// do_print_ex() with no ASN1_STRFLGS_* flags.
fn print_value_raw(out: &mut Vec<u8>, tag: u8, v: &[u8]) {
    let width = match tag2nbyte(tag) {
        -1 => 1,
        w => w,
    };
    let cps: Vec<u32> = match width {
        0 => match std::str::from_utf8(v) {
            Ok(s) => s.chars().map(|c| c as u32).collect(),
            Err(_) => return,
        },
        2 => string_code_points(T_BMP, v).unwrap_or_default(),
        4 => string_code_points(T_UNIVERSAL, v).unwrap_or_default(),
        _ => v.iter().map(|&b| b as u32).collect(),
    };
    for c in cps {
        if c > 0xffff {
            out.extend_from_slice(format!("\\W{:08X}", c).as_bytes());
        } else if c > 0xff {
            out.extend_from_slice(format!("\\U{:04X}", c).as_bytes());
        } else {
            out.push(c as u8);
        }
    }
}

/// do_print_ex() with ASN1_STRFLGS_RFC2253 | ASN1_STRFLGS_ESC_QUOTE.
fn print_value_rfc2253(out: &mut Vec<u8>, tag: u8, v: &[u8]) {
    let width = tag2nbyte(tag);
    if width == -1 {
        // ASN1_STRFLGS_DUMP_UNKNOWN | DUMP_DER: '#' + hex of the DER.
        out.push(b'#');
        let der = der_tlv(tag, v);
        out.extend_from_slice(
            der.iter()
                .map(|b| format!("{:02X}", b))
                .collect::<String>()
                .as_bytes(),
        );
        return;
    }
    // UTF8_CONVERT: UTF8String bytes go through one by one, the others are
    // converted to UTF-8 first.
    let bytes: Vec<u8> = if width == 0 {
        v.to_vec()
    } else {
        let cps = string_code_points(tag, v).unwrap_or_default();
        let mut b = Vec::new();
        for c in cps {
            let ch = char::from_u32(c).unwrap_or('\u{fffd}');
            let mut buf = [0u8; 4];
            b.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        }
        b
    };
    let mut body = Vec::new();
    let mut quote = false;
    let n = bytes.len();
    for (i, &c) in bytes.iter().enumerate() {
        let first = i == 0;
        let last = i + 1 == n;
        match c {
            b'"' | b'\\' => {
                body.push(b'\\');
                body.push(c);
            }
            b',' | b'+' | b';' | b'<' | b'>' => {
                quote = true;
                body.push(c);
            }
            b'#' if first => {
                quote = true;
                body.push(c);
            }
            b' ' if first || last => {
                quote = true;
                body.push(c);
            }
            0..=0x1f | 0x7f..=0xff => {
                body.extend_from_slice(format!("\\{:02X}", c).as_bytes());
            }
            _ => body.push(c),
        }
    }
    if quote {
        out.push(b'"');
        out.extend(body);
        out.push(b'"');
    } else {
        out.extend(body);
    }
}

/// X509_NAME_oneline(): "/C=US/O=Org/CN=name".
fn name_oneline_slash(n: &Name) -> String {
    let mut s = String::new();
    for e in &n.entries {
        s.push('/');
        s.push_str(&oid_sn(&e.oid));
        s.push('=');
        let bytes: Vec<u8> = match e.tag {
            T_BMP | T_UNIVERSAL => string_code_points(e.tag, &e.value)
                .unwrap_or_default()
                .iter()
                .map(|&c| if c < 0x100 { c as u8 } else { b'?' })
                .collect(),
            _ => e.value.clone(),
        };
        for b in bytes {
            if (0x20..0x7f).contains(&b) {
                s.push(b as char);
            } else {
                s.push_str(&format!("\\x{:02X}", b));
            }
        }
    }
    s
}

// ===========================================================================
// Time
// ===========================================================================

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
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

#[derive(Clone, Debug, Default)]
struct Asn1Time {
    unix: i64,
    /// Fractional seconds of a GeneralizedTime (".123"), printed back.
    frac: String,
}

fn parse_time(t: &Tlv) -> Option<Asn1Time> {
    let s = std::str::from_utf8(t.content).ok()?;
    let (year, rest) = match t.tag {
        T_UTCTIME => {
            let yy: i64 = s.get(0..2)?.parse().ok()?;
            (if yy < 50 { 2000 + yy } else { 1900 + yy }, s.get(2..)?)
        }
        T_GENTIME => (s.get(0..4)?.parse().ok()?, s.get(4..)?),
        _ => return None,
    };
    let num = |r: &str, i: usize| -> Option<i64> { r.get(i..i + 2)?.parse().ok() };
    let mo = num(rest, 0)?;
    let d = num(rest, 2)?;
    let h = num(rest, 4)?;
    let mi = num(rest, 6)?;
    let (sec, mut tail) = match rest.get(8..10).and_then(|x| x.parse::<i64>().ok()) {
        Some(sec) => (sec, rest.get(10..)?),
        None => (0, rest.get(8..)?),
    };
    let mut frac = String::new();
    if t.tag == T_GENTIME && tail.starts_with('.') {
        let end = tail[1..]
            .find(|c: char| !c.is_ascii_digit())
            .map(|p| p + 1)
            .unwrap_or(tail.len());
        frac = tail[..end].to_string();
        tail = &tail[end..];
    }
    let mut unix = days_from_civil(year, mo, d) * 86400 + h * 3600 + mi * 60 + sec;
    if let Some(off) = tail.strip_prefix(['+', '-']) {
        let oh: i64 = off.get(0..2)?.parse().ok()?;
        let om: i64 = off.get(2..4)?.parse().ok()?;
        let o = oh * 3600 + om * 60;
        if tail.starts_with('+') {
            unix -= o;
        } else {
            unix += o;
        }
    }
    Some(Asn1Time { unix, frac })
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// ASN1_TIME_print(): "Jan  1 00:00:00 2025 GMT".
fn time_print(t: &Asn1Time) -> String {
    let days = t.unix.div_euclid(86400);
    let secs = t.unix.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{} {:2} {:02}:{:02}:{:02}{} {} GMT",
        MONTHS[(m - 1) as usize],
        d,
        secs / 3600,
        secs / 60 % 60,
        secs % 60,
        t.frac,
        y
    )
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ===========================================================================
// X509 certificate model
// ===========================================================================

#[derive(Clone, Debug)]
struct Ext {
    oid: String,
    critical: bool,
    value: Vec<u8>,
}

#[derive(Clone, Debug)]
struct X509 {
    der: Vec<u8>,
    /// 0-based X509_get_version()
    version: i64,
    /// INTEGER content (two's complement)
    serial: Vec<u8>,
    sig_alg: String,
    issuer: Name,
    subject: Name,
    not_before: Asn1Time,
    not_after: Asn1Time,
    /// SubjectPublicKeyInfo TLV
    spki: Vec<u8>,
    pk_alg: String,
    /// AlgorithmIdentifier parameters TLV (empty when absent)
    pk_params: Vec<u8>,
    /// subjectPublicKey BIT STRING value
    pk_bits: Vec<u8>,
    exts: Vec<Ext>,
    signature: Vec<u8>,
    skid: Option<Vec<u8>>,
    akid: Option<Vec<u8>>,
    akid_serial: Option<Vec<u8>>,
    akid_issuer: Option<Name>,
    /// basicConstraints (cA, pathLen)
    bc: Option<(bool, Option<u64>)>,
    /// keyUsage bits, bit 0 (digitalSignature) = 0x80 of the first byte,
    /// packed as u16 big-endian of the BIT STRING bytes.
    ku: Option<u16>,
    eku: Option<Vec<String>>,
    ns_cert_type: Option<u8>,
}

const KU_DIGITAL_SIGNATURE: u16 = 0x8000;
const KU_KEY_ENCIPHERMENT: u16 = 0x2000;
const KU_KEY_AGREEMENT: u16 = 0x0800;
const KU_KEY_CERT_SIGN: u16 = 0x0400;

impl X509 {
    fn parse(der: &[u8]) -> Option<X509> {
        let top = der_one(der)?;
        if top.tag != T_SEQ {
            return None;
        }
        let c = top.children()?;
        if c.len() != 3 {
            return None;
        }
        let tbs = c[0].children()?;
        let mut i = 0;
        let mut version = 0;
        if tbs.first()?.ctx() == Some(0) {
            version = int_u64(der_one(tbs[0].content)?.content)? as i64;
            i = 1;
        }
        let serial = tbs.get(i)?;
        if serial.tag != T_INTEGER {
            return None;
        }
        let issuer = Name::parse(tbs.get(i + 2)?)?;
        let validity = tbs.get(i + 3)?.children()?;
        if validity.len() != 2 {
            return None;
        }
        let subject = Name::parse(tbs.get(i + 4)?)?;
        let spki = tbs.get(i + 5)?;
        let sc = spki.children()?;
        let alg = sc.first()?.children()?;
        let pk_alg = tlv_oid(alg.first()?)?;
        let pk_params = alg.get(1).map(|t| t.raw.to_vec()).unwrap_or_default();
        let pk_bits = sc.get(1)?.bits()?.to_vec();
        let mut exts = Vec::new();
        for x in &tbs[i + 6..] {
            if x.ctx() == Some(3) {
                for e in der_one(x.content)?.children()? {
                    let ec = e.children()?;
                    let oid = tlv_oid(ec.first()?)?;
                    let (critical, val) = if ec.len() == 3 && ec[1].tag == T_BOOLEAN {
                        (ec[1].content.first().copied().unwrap_or(0) != 0, ec[2])
                    } else {
                        (false, *ec.get(1)?)
                    };
                    if val.tag != T_OCTET_STRING {
                        return None;
                    }
                    exts.push(Ext {
                        oid,
                        critical,
                        value: val.content.to_vec(),
                    });
                }
            }
        }
        let sig_alg = tlv_oid(c[1].children()?.first()?)?;
        let mut x = X509 {
            der: top.raw.to_vec(),
            version,
            serial: serial.content.to_vec(),
            sig_alg,
            issuer,
            subject,
            not_before: parse_time(&validity[0]).unwrap_or_default(),
            not_after: parse_time(&validity[1]).unwrap_or_default(),
            spki: spki.raw.to_vec(),
            pk_alg,
            pk_params,
            pk_bits,
            exts,
            signature: c[2].bits()?.to_vec(),
            skid: None,
            akid: None,
            akid_serial: None,
            akid_issuer: None,
            bc: None,
            ku: None,
            eku: None,
            ns_cert_type: None,
        };
        x.cache_extensions();
        Some(x)
    }

    fn ext(&self, oid: &str) -> Option<&Ext> {
        self.exts.iter().find(|e| e.oid == oid)
    }

    fn cache_extensions(&mut self) {
        let exts = std::mem::take(&mut self.exts);
        let find = |oid: &str| exts.iter().find(|e| e.oid == oid);
        if let Some(e) = find("2.5.29.14")
            && let Some(t) = der_one(&e.value)
            && t.tag == T_OCTET_STRING
        {
            self.skid = Some(t.content.to_vec());
        }
        if let Some(e) = find("2.5.29.35")
            && let Some(t) = der_one(&e.value)
            && let Some(items) = t.children()
        {
            for it in items {
                match it.ctx() {
                    Some(0) => self.akid = Some(it.content.to_vec()),
                    Some(1) => {
                        if let Some(gns) = der_list(it.content) {
                            for g in gns {
                                if g.ctx() == Some(4)
                                    && let Some(n) =
                                        der_one(g.content).and_then(|n| Name::parse(&n))
                                {
                                    self.akid_issuer = Some(n);
                                }
                            }
                        }
                    }
                    Some(2) => self.akid_serial = Some(it.content.to_vec()),
                    _ => {}
                }
            }
        }
        if let Some(e) = find("2.5.29.19")
            && let Some(t) = der_one(&e.value)
            && let Some(items) = t.children()
        {
            let mut ca = false;
            let mut pl = None;
            for it in items {
                if it.tag == T_BOOLEAN {
                    ca = it.content.first().copied().unwrap_or(0) != 0;
                } else if it.tag == T_INTEGER {
                    pl = int_u64(it.content);
                }
            }
            self.bc = Some((ca, pl));
        }
        if let Some(e) = find("2.5.29.15")
            && let Some(t) = der_one(&e.value)
            && let Some(b) = t.bits()
        {
            let hi = b.first().copied().unwrap_or(0) as u16;
            let lo = b.get(1).copied().unwrap_or(0) as u16;
            self.ku = Some((hi << 8) | lo);
        }
        if let Some(e) = find("2.5.29.37")
            && let Some(t) = der_one(&e.value)
            && let Some(items) = t.children()
        {
            self.eku = Some(items.iter().filter_map(tlv_oid).collect());
        }
        if let Some(e) = find("2.16.840.1.113730.1.1")
            && let Some(t) = der_one(&e.value)
            && let Some(b) = t.bits()
        {
            self.ns_cert_type = Some(b.first().copied().unwrap_or(0));
        }
        self.exts = exts;
    }

    /// The SAN GeneralNames (tag number, content).
    fn san(&self) -> Option<Vec<(u8, Vec<u8>)>> {
        let e = self.ext("2.5.29.17")?;
        let t = der_one(&e.value)?;
        Some(
            t.children()?
                .iter()
                .filter_map(|g| g.ctx().map(|n| (n, g.content.to_vec())))
                .collect(),
        )
    }

    /// EXFLAG_CRITICAL: a critical extension X509_supported_extension()
    /// does not know.
    fn has_unhandled_critical(&self) -> bool {
        const SUPPORTED: &[&str] = &[
            "2.16.840.1.113730.1.1",
            "2.5.29.15",
            "2.5.29.17",
            "2.5.29.19",
            "2.5.29.32",
            "2.5.29.31",
            "2.5.29.37",
            "1.3.6.1.5.5.7.1.7",
            "1.3.6.1.5.5.7.1.8",
            "1.3.6.1.5.5.7.48.1.5",
            "2.5.29.36",
            "1.3.6.1.5.5.7.1.14",
            "2.5.29.30",
            "2.5.29.33",
            "2.5.29.54",
        ];
        self.exts
            .iter()
            .any(|e| e.critical && !SUPPORTED.contains(&e.oid.as_str()))
    }

    fn is_self_issued(&self) -> bool {
        self.subject.eq(&self.issuer)
    }

    /// EXFLAG_SS: self-issued and the AKID (if any) names its own key.
    fn is_self_signed(&self) -> bool {
        self.is_self_issued() && akid_matches(self, self)
    }

    fn public_key(&self) -> Option<purecrypto::x509::AnyPublicKey> {
        purecrypto::x509::AnyPublicKey::from_spki_der(&self.spki).ok()
    }

    /// Signature check of `self` against `issuer`'s key.
    fn signed_by(&self, issuer: &X509) -> bool {
        let Some(k) = issuer.public_key() else {
            return false;
        };
        match purecrypto::x509::Certificate::from_der(self.der.clone()) {
            Ok(c) => c.verify_signature_with(&k).is_ok(),
            Err(_) => false,
        }
    }

    fn pem(&self) -> String {
        pem_encode("CERTIFICATE", &self.der)
    }
}

/// X509_check_akid(issuer, subject->akid) == X509_V_OK
fn akid_matches(issuer: &X509, subject: &X509) -> bool {
    if let (Some(a), Some(s)) = (&subject.akid, &issuer.skid)
        && a != s
    {
        return false;
    }
    if let Some(serial) = &subject.akid_serial
        && strip_int(serial) != strip_int(&issuer.serial)
    {
        return false;
    }
    if let Some(n) = &subject.akid_issuer
        && !n.eq(&issuer.issuer)
    {
        return false;
    }
    true
}

fn strip_int(v: &[u8]) -> &[u8] {
    let mut v = v;
    while v.len() > 1 && v[0] == 0 {
        v = &v[1..];
    }
    v
}

/// X509_check_issued(issuer, subject) == X509_V_OK
fn check_issued(issuer: &X509, subject: &X509) -> bool {
    if !issuer.subject.eq(&subject.issuer) {
        return false;
    }
    if !akid_matches(issuer, subject) {
        return false;
    }
    if let Some(ku) = issuer.ku
        && ku & KU_KEY_CERT_SIGN == 0
    {
        return false;
    }
    true
}

// ===========================================================================
// Public keys
// ===========================================================================

#[derive(Clone, Debug)]
enum PkInfo {
    Rsa {
        n: Vec<u8>,
        e: Vec<u8>,
        pss: bool,
    },
    Ec {
        curve: String,
    },
    Ed25519,
    Ed448,
    Dsa {
        p: Vec<u8>,
        q: Vec<u8>,
        g: Vec<u8>,
        y: Vec<u8>,
    },
    Dh {
        p: Vec<u8>,
        g: Vec<u8>,
        y: Vec<u8>,
    },
    Other,
}

fn pk_info(x: &X509) -> PkInfo {
    let ints = |d: &[u8]| -> Option<Vec<Vec<u8>>> {
        Some(
            der_one(d)?
                .children()?
                .iter()
                .map(|t| strip_int(t.content).to_vec())
                .collect(),
        )
    };
    match x.pk_alg.as_str() {
        "1.2.840.113549.1.1.1" | "1.2.840.113549.1.1.10" => match ints(&x.pk_bits) {
            Some(v) if v.len() >= 2 => PkInfo::Rsa {
                n: v[0].clone(),
                e: v[1].clone(),
                pss: x.pk_alg.ends_with(".10"),
            },
            _ => PkInfo::Other,
        },
        "1.2.840.10045.2.1" => match der_one(&x.pk_params).and_then(|t| tlv_oid(&t)) {
            Some(curve) => PkInfo::Ec { curve },
            None => PkInfo::Other,
        },
        "1.3.101.112" => PkInfo::Ed25519,
        "1.3.101.113" => PkInfo::Ed448,
        "1.2.840.10040.4.1" => {
            let pqg = ints(&x.pk_params).unwrap_or_default();
            let y = der_one(&x.pk_bits)
                .map(|t| strip_int(t.content).to_vec())
                .unwrap_or_default();
            if pqg.len() == 3 {
                PkInfo::Dsa {
                    p: pqg[0].clone(),
                    q: pqg[1].clone(),
                    g: pqg[2].clone(),
                    y,
                }
            } else {
                PkInfo::Other
            }
        }
        "1.2.840.113549.1.3.1" => {
            let pg = ints(&x.pk_params).unwrap_or_default();
            let y = der_one(&x.pk_bits)
                .map(|t| strip_int(t.content).to_vec())
                .unwrap_or_default();
            if pg.len() >= 2 {
                PkInfo::Dh {
                    p: pg[0].clone(),
                    g: pg[1].clone(),
                    y,
                }
            } else {
                PkInfo::Other
            }
        }
        _ => PkInfo::Other,
    }
}

fn bn_bits(v: &[u8]) -> i64 {
    let v = strip_int(v);
    match v.first() {
        None | Some(0) => 0,
        Some(&b) => (v.len() as i64 - 1) * 8 + (8 - b.leading_zeros() as i64),
    }
}

/// BN_print(): uppercase hex without leading zeros.
fn bn_hex(v: &[u8]) -> String {
    let s = hex_upper(strip_int(v));
    let t = s.trim_start_matches('0');
    if t.is_empty() {
        "0".to_string()
    } else {
        t.to_string()
    }
}

/// ossl_ifc_ffc_compute_security_bits()
fn ifc_security_bits(n: i64) -> i64 {
    match n {
        2048 => return 112,
        3072 => return 128,
        4096 => return 152,
        6144 => return 176,
        7680 => return 192,
        8192 => return 200,
        15360 => return 256,
        _ => {}
    }
    if n >= 687737 {
        return 1200;
    }
    if n < 8 {
        return 0;
    }
    let cap = if n <= 7680 {
        192
    } else if n <= 15360 {
        256
    } else {
        1200
    };
    let ln2 = std::f64::consts::LN_2;
    let x = n as f64 * ln2;
    let lx = x.ln();
    let y = ((1.923 * (x * lx * lx).cbrt() - 4.69) / ln2) as i64;
    let y = (y + 4) & !7;
    y.min(cap)
}

fn curve_bits(curve: &str) -> i64 {
    match curve {
        "1.2.840.10045.3.1.7" | "1.3.132.0.10" | "1.3.36.3.3.2.8.1.1.7" => 256,
        "1.3.132.0.34" | "1.3.36.3.3.2.8.1.1.11" => 384,
        "1.3.132.0.35" => 521,
        "1.3.132.0.33" => 224,
        "1.2.840.10045.3.1.1" => 192,
        "1.3.36.3.3.2.8.1.1.13" => 512,
        _ => 0,
    }
}

/// (EVP_PKEY_get0_type_name, group, EVP_PKEY_bits, EVP_PKEY_get_security_bits)
fn pk_summary(x: &X509) -> (String, Option<String>, i64, i64) {
    match pk_info(x) {
        PkInfo::Rsa { n, pss, .. } => {
            let b = bn_bits(&n);
            let name = if pss { "RSA-PSS" } else { "RSA" };
            (name.to_string(), None, b, ifc_security_bits(b))
        }
        PkInfo::Ec { curve } => {
            let b = curve_bits(&curve);
            let sec = if b >= 512 {
                256
            } else if b >= 384 {
                192
            } else if b >= 256 {
                128
            } else if b >= 224 {
                112
            } else if b >= 160 {
                80
            } else {
                b / 2
            };
            ("EC".to_string(), Some(oid_sn(&curve)), b, sec)
        }
        PkInfo::Ed25519 => ("ED25519".to_string(), None, 256, 128),
        PkInfo::Ed448 => ("ED448".to_string(), None, 456, 224),
        PkInfo::Dsa { p, q, .. } => {
            let l = bn_bits(&p);
            let nq = bn_bits(&q);
            let sec = if l >= 15360 {
                256
            } else if l >= 7680 {
                192
            } else if l >= 3072 {
                128
            } else if l >= 2048 {
                112
            } else if l >= 1024 {
                80
            } else {
                0
            };
            let half = nq / 2;
            let sec = if half < 80 { 0 } else { sec.min(half) };
            ("DSA".to_string(), None, l, sec)
        }
        PkInfo::Dh { p, .. } => {
            let l = bn_bits(&p);
            ("DH".to_string(), None, l, ifc_security_bits(l))
        }
        PkInfo::Other => match x.pk_alg.as_str() {
            "2.16.840.1.101.3.4.3.17" => ("ML-DSA-44".into(), None, 10496, 128),
            "2.16.840.1.101.3.4.3.18" => ("ML-DSA-65".into(), None, 15616, 192),
            "2.16.840.1.101.3.4.3.19" => ("ML-DSA-87".into(), None, 20736, 256),
            _ => ("?".into(), None, 0, 0),
        },
    }
}

// ===========================================================================
// X509V3_EXT_print()
// ===========================================================================

fn ip_to_asc(ip: &[u8]) -> String {
    match ip.len() {
        4 => format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]),
        16 => ip
            .chunks(2)
            .map(|c| format!("{:X}", ((c[0] as u16) << 8) | c[1] as u16))
            .collect::<Vec<_>>()
            .join(":"),
        _ => "<invalid>".to_string(),
    }
}

/// OpenSSL prints the code points up to U+00FF of a name as one raw byte;
/// as a `String` those bytes are the latin-1 characters they stand for.
fn latin1(v: &[u8]) -> String {
    v.iter().map(|&b| b as char).collect()
}

fn utf8_lossy(v: &[u8]) -> String {
    String::from_utf8_lossy(v).into_owned()
}

/// i2v_GENERAL_NAME() as (name, value).
fn gen_name_conf(g: &Tlv) -> (String, String) {
    match g.ctx() {
        Some(0) => {
            // otherName: type-id + [0] EXPLICIT value
            let parts = der_list(g.content).unwrap_or_default();
            let oid = parts.first().and_then(tlv_oid).unwrap_or_default();
            let val = parts
                .get(1)
                .and_then(|v| der_one(v.content))
                .map(|v| utf8_lossy(v.content));
            match (oid.as_str(), val) {
                ("1.3.6.1.4.1.311.20.2.3", Some(v)) => ("othername".into(), format!("UPN:{}", v)),
                ("1.3.6.1.5.5.7.8.9", Some(v)) => {
                    ("othername".into(), format!("SmtpUTF8Mailbox:{}", v))
                }
                _ => ("othername".into(), "<unsupported>".into()),
            }
        }
        Some(1) => ("email".into(), utf8_lossy(g.content)),
        Some(2) => ("DNS".into(), utf8_lossy(g.content)),
        Some(3) => ("X400Name".into(), "<unsupported>".into()),
        Some(4) => {
            let n = der_one(g.content)
                .and_then(|t| Name::parse(&t))
                .map(|n| name_oneline_slash(&n))
                .unwrap_or_default();
            ("DirName".into(), n)
        }
        Some(5) => ("EdiPartyName".into(), "<unsupported>".into()),
        Some(6) => ("URI".into(), utf8_lossy(g.content)),
        Some(7) => ("IP Address".into(), ip_to_asc(g.content)),
        Some(8) => ("Registered ID".into(), oid_ln(&oid_str(g.content))),
        _ => ("othername".into(), "<unsupported>".into()),
    }
}

/// GENERAL_NAME_print()
fn gen_name_print(g: &Tlv) -> String {
    if g.ctx() == Some(4) {
        let n = der_one(g.content)
            .and_then(|t| Name::parse(&t))
            .map(|n| utf8_lossy(&name_print(&n, NameFmt::Oneline)))
            .unwrap_or_default();
        return format!("DirName:{}", n);
    }
    let (k, v) = gen_name_conf(g);
    format!("{}:{}", k, v)
}

fn conf_join(vals: &[(Option<String>, String)], multiline: bool) -> String {
    if vals.is_empty() {
        return "<EMPTY>\n".to_string();
    }
    let sep = if multiline { "\n" } else { ", " };
    vals.iter()
        .map(|(k, v)| match k {
            Some(k) if !v.is_empty() => format!("{}:{}", k, v),
            Some(k) => k.clone(),
            None => v.clone(),
        })
        .collect::<Vec<_>>()
        .join(sep)
}

fn gen_names_conf(content: &[u8]) -> Option<Vec<(Option<String>, String)>> {
    let list = der_list(content)?;
    Some(
        list.iter()
            .map(|g| {
                let (k, v) = gen_name_conf(g);
                (Some(k), v)
            })
            .collect(),
    )
}

fn print_gens(out: &mut String, content: &[u8], indent: usize) {
    for g in der_list(content).unwrap_or_default() {
        out.push_str(&" ".repeat(indent + 2));
        out.push_str(&gen_name_print(&g));
        out.push('\n');
    }
}

const REASONS: [&str; 9] = [
    "Unused",
    "Key Compromise",
    "CA Compromise",
    "Affiliation Changed",
    "Superseded",
    "Cessation Of Operation",
    "Certificate Hold",
    "Privilege Withdrawn",
    "AA Compromise",
];

fn bit_names(bits: &[u8], names: &[&str]) -> Vec<String> {
    let mut v = Vec::new();
    for (i, n) in names.iter().enumerate() {
        let byte = bits.get(i / 8).copied().unwrap_or(0);
        if byte & (0x80 >> (i % 8)) != 0 {
            v.push(n.to_string());
        }
    }
    v
}

fn print_crldp(v: &[u8]) -> Option<String> {
    let mut out = String::new();
    for (i, dp) in der_one(v)?.children()?.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        for f in dp.children()? {
            match f.ctx() {
                Some(0) => {
                    let dpn = der_one(f.content)?;
                    match dpn.ctx() {
                        Some(0) => {
                            out.push_str("Full Name:\n");
                            print_gens(&mut out, dpn.content, 0);
                        }
                        Some(1) => {
                            let n = Name::parse(&Tlv {
                                tag: T_SEQ,
                                content: dpn.content,
                                raw: dpn.raw,
                            })?;
                            out.push_str("Relative Name:\n  ");
                            out.push_str(&utf8_lossy(&name_print(&n, NameFmt::Oneline)));
                            out.push('\n');
                        }
                        _ => {}
                    }
                }
                Some(1) => {
                    let b = f.content.get(1..).unwrap_or(&[]);
                    let names = bit_names(b, &REASONS);
                    out.push_str("Reasons:\n  ");
                    if names.is_empty() {
                        out.push_str("<EMPTY>\n");
                    } else {
                        out.push_str(&names.join(", "));
                        out.push('\n');
                    }
                }
                Some(2) => {
                    out.push_str("CRL Issuer:\n");
                    print_gens(&mut out, f.content, 0);
                }
                _ => {}
            }
        }
    }
    Some(out)
}

fn print_policies(v: &[u8]) -> Option<String> {
    let mut out = String::new();
    for (i, pi) in der_one(v)?.children()?.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let c = pi.children()?;
        out.push_str("Policy: ");
        out.push_str(&oid_ln(&tlv_oid(c.first()?)?));
        if let Some(quals) = c.get(1) {
            out.push('\n');
            for (j, q) in quals.children()?.iter().enumerate() {
                if j > 0 {
                    out.push('\n');
                }
                let qc = q.children()?;
                let qid = tlv_oid(qc.first()?)?;
                match qid.as_str() {
                    "1.3.6.1.5.5.7.2.1" => {
                        out.push_str("  CPS: ");
                        out.push_str(&utf8_lossy(qc.get(1)?.content));
                    }
                    "1.3.6.1.5.5.7.2.2" => {
                        out.push_str("  User Notice:\n");
                        let mut noticeref = None;
                        let mut text = None;
                        if let Some(un) = qc.get(1) {
                            for f in un.children()? {
                                if f.tag == T_SEQ {
                                    noticeref = Some(f);
                                } else {
                                    text = Some(f);
                                }
                            }
                        }
                        if let Some(nr) = noticeref {
                            let nc = nr.children()?;
                            out.push_str("    Organization: ");
                            out.push_str(&utf8_lossy(nc.first()?.content));
                            out.push('\n');
                            let nums: Vec<String> = nc
                                .get(1)
                                .and_then(|n| n.children())
                                .unwrap_or_default()
                                .iter()
                                .map(|n| {
                                    int_u64(n.content)
                                        .map(|v| v.to_string())
                                        .unwrap_or_default()
                                })
                                .collect();
                            out.push_str(if nums.len() > 1 {
                                "    Numbers: "
                            } else {
                                "    Number: "
                            });
                            out.push_str(&nums.join(", "));
                            if text.is_some() {
                                out.push('\n');
                            }
                        }
                        if let Some(t) = text {
                            out.push_str("    Explicit Text: ");
                            let s: String = string_code_points(t.tag, t.content)
                                .unwrap_or_default()
                                .iter()
                                .filter_map(|&c| char::from_u32(c))
                                .collect();
                            out.push_str(&s);
                        }
                    }
                    _ => {
                        out.push_str("    Unknown Qualifier: ");
                        out.push_str(&oid_ln(&qid));
                    }
                }
            }
        }
    }
    Some(out)
}

fn hex_block(data: &[u8], indent: usize, width: usize) -> String {
    let mut s = String::new();
    let mut j = 0;
    for (i, b) in data.iter().enumerate() {
        if i > 0 && j == 0 {
            s.push_str(&" ".repeat(indent));
        }
        s.push_str(&format!("{:02X}", b));
        if !(j == width - 1 || i == data.len() - 1) {
            s.push(':');
        }
        j += 1;
        if j >= width {
            if i != data.len() - 1 {
                s.push('\n');
            }
            j = 0;
        }
    }
    s
}

fn print_scts(v: &[u8]) -> Option<String> {
    let os = der_one(v)?;
    if os.tag != T_OCTET_STRING {
        return None;
    }
    let d = os.content;
    let total = ((*d.first()? as usize) << 8) | *d.get(1)? as usize;
    let mut p = d.get(2..2 + total)?;
    let mut items = Vec::new();
    while !p.is_empty() {
        let l = ((*p.first()? as usize) << 8) | *p.get(1)? as usize;
        let sct = p.get(2..2 + l)?;
        p = &p[2 + l..];
        let mut s = String::from("Signed Certificate Timestamp:\n    Version   : ");
        if sct.first() != Some(&0) {
            s.push_str("unknown\n                ");
            s.push_str(&hex_block(sct, 16, 16));
            items.push(s);
            continue;
        }
        s.push_str("v1 (0x0)");
        let log_id = sct.get(1..33)?;
        let ts = u64::from_be_bytes(sct.get(33..41)?.try_into().ok()?);
        let el = ((*sct.get(41)? as usize) << 8) | *sct.get(42)? as usize;
        let ext = sct.get(43..43 + el)?;
        let q = sct.get(43 + el..)?;
        let (hash, sig_alg) = (*q.first()?, *q.get(1)?);
        let sl = ((*q.get(2)? as usize) << 8) | *q.get(3)? as usize;
        let sig = q.get(4..4 + sl)?;
        s.push_str("\n    Log ID    : ");
        s.push_str(&hex_block(log_id, 16, 16));
        s.push_str("\n    Timestamp : ");
        let t = Asn1Time {
            unix: (ts / 1000) as i64,
            frac: format!(".{:03}", ts % 1000),
        };
        s.push_str(&time_print(&t));
        s.push_str("\n    Extensions: ");
        if ext.is_empty() {
            s.push_str("none");
        } else {
            s.push_str(&hex_block(ext, 16, 16));
        }
        s.push_str("\n    Signature : ");
        let name = match (hash, sig_alg) {
            (4, 3) => Some("ecdsa-with-SHA256"),
            (4, 1) => Some("sha256WithRSAEncryption"),
            _ => None,
        };
        match name {
            Some(n) => s.push_str(n),
            None => s.push_str(&format!("{:02X}{:02X}", hash, sig_alg)),
        }
        s.push_str("\n                ");
        s.push_str(&hex_block(sig, 16, 16));
        items.push(s);
    }
    Some(items.join("\n"))
}

fn print_name_constraints(v: &[u8]) -> Option<String> {
    let mut parts = Vec::new();
    for t in der_one(v)?.children()? {
        let label = match t.ctx() {
            Some(0) => "Permitted",
            Some(1) => "Excluded",
            _ => continue,
        };
        let subtrees = der_list(t.content)?;
        let mut s = String::new();
        if !subtrees.is_empty() {
            s.push_str(label);
            s.push_str(":\n");
        }
        for (i, st) in subtrees.iter().enumerate() {
            if i > 0 {
                s.push('\n');
            }
            s.push_str("  ");
            let base = *st.children()?.first()?;
            if base.ctx() == Some(7) {
                let ip = base.content;
                let half = ip.len() / 2;
                s.push_str("IP:");
                if ip.len() == 8 || ip.len() == 32 {
                    s.push_str(&ip_to_asc(&ip[..half]));
                    s.push('/');
                    s.push_str(&ip_to_asc(&ip[half..]));
                } else {
                    s.push_str("IP Address:<invalid>");
                }
            } else {
                s.push_str(&gen_name_print(&base));
            }
        }
        parts.push(s);
    }
    Some(parts.join("\n"))
}

/// X509V3_EXT_print(out, ext, 0, 0); None when OpenSSL would not know the
/// extension (curl then prints the raw OCTET STRING).
fn ext_print(e: &Ext) -> Option<String> {
    let v = &e.value;
    match e.oid.as_str() {
        "2.5.29.19" => {
            let (ca, pl) = (|| {
                let mut ca = false;
                let mut pl = None;
                for it in der_one(v)?.children()? {
                    if it.tag == T_BOOLEAN {
                        ca = it.content.first().copied().unwrap_or(0) != 0;
                    } else if it.tag == T_INTEGER {
                        pl = Some(int_u64(it.content)?);
                    }
                }
                Some((ca, pl))
            })()?;
            let mut s = format!("CA:{}", if ca { "TRUE" } else { "FALSE" });
            if let Some(p) = pl {
                s.push_str(&format!(", pathlen:{}", p));
            }
            Some(s)
        }
        "2.5.29.15" => {
            let t = der_one(v)?;
            let b = t.bits()?;
            Some(
                bit_names(
                    b,
                    &[
                        "Digital Signature",
                        "Non Repudiation",
                        "Key Encipherment",
                        "Data Encipherment",
                        "Key Agreement",
                        "Certificate Sign",
                        "CRL Sign",
                        "Encipher Only",
                        "Decipher Only",
                    ],
                )
                .join(", "),
            )
        }
        "2.5.29.37" => Some(
            der_one(v)?
                .children()?
                .iter()
                .map(|o| tlv_oid(o).map(|d| oid_ln(&d)))
                .collect::<Option<Vec<_>>>()?
                .join(", "),
        ),
        "2.5.29.14" => {
            let t = der_one(v)?;
            if t.tag != T_OCTET_STRING {
                return None;
            }
            Some(hex_colon_upper(t.content))
        }
        "2.5.29.35" => {
            let items = der_one(v)?.children()?;
            let has_more = items.iter().any(|i| matches!(i.ctx(), Some(1) | Some(2)));
            let mut vals: Vec<(Option<String>, String)> = Vec::new();
            for it in &items {
                match it.ctx() {
                    Some(0) => vals.push((
                        if has_more { Some("keyid".into()) } else { None },
                        hex_colon_upper(it.content),
                    )),
                    Some(1) => vals.extend(gen_names_conf(it.content)?),
                    Some(2) => vals.push((Some("serial".into()), hex_colon_upper(it.content))),
                    _ => {}
                }
            }
            Some(conf_join(&vals, true))
        }
        "2.5.29.17" | "2.5.29.18" => Some(conf_join(&gen_names_conf(der_one(v)?.content)?, false)),
        "2.5.29.31" | "2.5.29.46" => print_crldp(v),
        "1.3.6.1.5.5.7.1.1" | "1.3.6.1.5.5.7.1.11" => {
            let mut vals = Vec::new();
            for ad in der_one(v)?.children()? {
                let c = ad.children()?;
                let method = oid_ln(&tlv_oid(c.first()?)?);
                let (k, val) = gen_name_conf(c.get(1)?);
                vals.push((Some(format!("{} - {}", method, k)), val));
            }
            Some(conf_join(&vals, true))
        }
        "2.5.29.32" => print_policies(v),
        "2.16.840.1.113730.1.13"
        | "2.16.840.1.113730.1.2"
        | "2.16.840.1.113730.1.3"
        | "2.16.840.1.113730.1.4"
        | "2.16.840.1.113730.1.7"
        | "2.16.840.1.113730.1.8"
        | "2.16.840.1.113730.1.12" => {
            let t = der_one(v)?;
            Some(utf8_lossy(t.content))
        }
        "2.16.840.1.113730.1.1" => {
            let t = der_one(v)?;
            Some(
                bit_names(
                    t.bits()?,
                    &[
                        "SSL Client",
                        "SSL Server",
                        "S/MIME",
                        "Object Signing",
                        "Unused",
                        "SSL CA",
                        "S/MIME CA",
                        "Object Signing CA",
                    ],
                )
                .join(", "),
            )
        }
        "1.3.6.1.5.5.7.1.24" => {
            let vals: Vec<String> = der_one(v)?
                .children()?
                .iter()
                .map(|i| match int_u64(i.content) {
                    Some(5) => "status_request".to_string(),
                    Some(17) => "status_request_v2".to_string(),
                    Some(n) => n.to_string(),
                    None => String::new(),
                })
                .collect();
            Some(vals.join(", "))
        }
        "1.3.6.1.5.5.7.48.1.5" => Some(String::new()),
        "1.3.6.1.4.1.11129.2.4.3" => Some("NULL".to_string()),
        "1.3.6.1.4.1.11129.2.4.2" | "1.3.6.1.4.1.11129.2.4.5" => print_scts(v),
        "2.5.29.30" => print_name_constraints(v),
        "2.5.29.36" => {
            let mut vals = Vec::new();
            for it in der_one(v)?.children()? {
                let n = int_u64(it.content)?.to_string();
                match it.ctx() {
                    Some(0) => vals.push((Some("Require Explicit Policy".into()), n)),
                    Some(1) => vals.push((Some("Inhibit Policy Mapping".into()), n)),
                    _ => {}
                }
            }
            Some(conf_join(&vals, false))
        }
        "2.5.29.54" => Some(int_u64(der_one(v)?.content)?.to_string()),
        "2.5.29.33" => {
            let mut vals = Vec::new();
            for m in der_one(v)?.children()? {
                let c = m.children()?;
                vals.push((
                    Some(oid_ln(&tlv_oid(c.first()?)?)),
                    oid_ln(&tlv_oid(c.get(1)?)?),
                ));
            }
            Some(conf_join(&vals, false))
        }
        "2.5.29.16" => {
            let mut nb = None;
            let mut na = None;
            for it in der_one(v)?.children()? {
                let t = Tlv {
                    tag: T_GENTIME,
                    content: it.content,
                    raw: it.raw,
                };
                match it.ctx() {
                    Some(0) => nb = parse_time(&t),
                    Some(1) => na = parse_time(&t),
                    _ => {}
                }
            }
            let mut s = String::new();
            if let Some(t) = &nb {
                s.push_str("Not Before: ");
                s.push_str(&time_print(t));
                if na.is_some() {
                    s.push_str(", ");
                }
            }
            if let Some(t) = &na {
                s.push_str("Not After: ");
                s.push_str(&time_print(t));
            }
            Some(s)
        }
        _ => None,
    }
}

/// ASN1_STRING_print() of the raw extension value.
fn asn1_string_print(v: &[u8]) -> Vec<u8> {
    v.iter()
        .map(|&c| {
            if c > b'~' || (c < b' ' && c != b'\n' && c != b'\r') {
                b'.'
            } else {
                c
            }
        })
        .collect()
}

/// ossl_certchain() for one certificate: the CURLINFO_CERTINFO entries.
fn cert_info_entries(x: &X509) -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = Vec::new();
    let mut push = |label: &str, value: &[u8]| {
        let mut e = label.as_bytes().to_vec();
        e.push(b':');
        e.extend_from_slice(value);
        v.push(e);
    };
    push("Subject", &name_print(&x.subject, NameFmt::Oneline));
    push("Issuer", &name_print(&x.issuer, NameFmt::Oneline));
    push("Version", format!("{:x}", x.version).as_bytes());
    let mut serial = String::new();
    let mut s = x.serial.as_slice();
    if s.first().is_some_and(|b| b & 0x80 != 0) {
        // negative: two's complement magnitude
        serial.push('-');
        let mut mag: Vec<u8> = s.iter().map(|b| !b).collect();
        for b in mag.iter_mut().rev() {
            let (r, c) = b.overflowing_add(1);
            *b = r;
            if !c {
                break;
            }
        }
        let m = strip_int(&mag).to_vec();
        serial.push_str(&m.iter().map(|b| format!("{:02x}", b)).collect::<String>());
    } else {
        while s.len() > 1 && s[0] == 0 {
            s = &s[1..];
        }
        if s == [0] {
            s = &[];
        }
        serial.push_str(&s.iter().map(|b| format!("{:02x}", b)).collect::<String>());
    }
    push("Serial Number", serial.as_bytes());
    push("Signature Algorithm", oid_ln(&x.sig_alg).as_bytes());
    push("Public Key Algorithm", oid_ln(&x.pk_alg).as_bytes());
    for e in &x.exts {
        let val = match ext_print(e) {
            Some(s) => s.into_bytes(),
            None => asn1_string_print(&e.value),
        };
        push(&oid_ln(&e.oid), &val);
    }
    push("Start date", time_print(&x.not_before).as_bytes());
    push("Expire date", time_print(&x.not_after).as_bytes());
    match pk_info(x) {
        PkInfo::Rsa { n, e, pss: false } => {
            push("RSA Public Key", bn_bits(&n).to_string().as_bytes());
            push("rsa(n)", bn_hex(&n).as_bytes());
            push("rsa(e)", bn_hex(&e).as_bytes());
        }
        PkInfo::Dsa { p, q, g, y } => {
            push("dsa(p)", bn_hex(&p).as_bytes());
            push("dsa(q)", bn_hex(&q).as_bytes());
            push("dsa(g)", bn_hex(&g).as_bytes());
            push("dsa(pub_key)", bn_hex(&y).as_bytes());
        }
        PkInfo::Dh { p, g, y } => {
            push("dh(p)", bn_hex(&p).as_bytes());
            push("dh(q)", b"");
            push("dh(g)", bn_hex(&g).as_bytes());
            push("dh(pub_key)", bn_hex(&y).as_bytes());
        }
        _ => {}
    }
    let sig: String = x.signature.iter().map(|b| format!("{:02x}:", b)).collect();
    push("Signature", sig.as_bytes());
    push("Cert", x.pem().as_bytes());
    v
}

// ===========================================================================
// Trust store (X509_STORE with a file lookup and a hashed-dir lookup)
// ===========================================================================

#[derive(Clone, Debug)]
struct Crl {
    issuer: Name,
    der: Vec<u8>,
    this_update: i64,
    next_update: Option<i64>,
    revoked: Vec<Vec<u8>>,
}

impl Crl {
    fn parse(der: &[u8]) -> Option<Crl> {
        let top = der_one(der)?;
        let c = top.children()?;
        let tbs = c.first()?.children()?;
        let mut i = 0;
        if tbs.first()?.tag == T_INTEGER {
            i = 1;
        }
        let issuer = Name::parse(tbs.get(i + 1)?)?;
        let this_update = parse_time(tbs.get(i + 2)?)?.unix;
        let mut next_update = None;
        let mut revoked = Vec::new();
        for t in &tbs[i + 3..] {
            if matches!(t.tag, T_UTCTIME | T_GENTIME) {
                next_update = parse_time(t).map(|t| t.unix);
            } else if t.tag == T_SEQ {
                for r in t.children()? {
                    if let Some(s) = r.children()?.first() {
                        revoked.push(strip_int(s.content).to_vec());
                    }
                }
            }
        }
        Some(Crl {
            issuer,
            der: top.raw.to_vec(),
            this_update,
            next_update,
            revoked,
        })
    }

    fn signed_by(&self, issuer: &X509) -> bool {
        let Some(k) = issuer.public_key() else {
            return false;
        };
        match purecrypto::x509::CertificateRevocationList::from_der(self.der.clone()) {
            Ok(c) => c.verify_signature_with(&k).is_ok(),
            Err(_) => false,
        }
    }
}

#[derive(Clone, Default)]
struct Store {
    certs: Vec<Arc<X509>>,
    dirs: Vec<PathBuf>,
    crls: Vec<Crl>,
}

type Loaded = (Vec<Arc<X509>>, Vec<Crl>);

/// Parsed CA files, keyed by path and invalidated on mtime/size change
/// (libcurl caches its X509_STORE for CURLOPT_CA_CACHE_TIMEOUT too).
type CaCache = Mutex<HashMap<PathBuf, (std::time::SystemTime, u64, Arc<Loaded>)>>;

fn ca_file_cache() -> &'static CaCache {
    static C: OnceLock<CaCache> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// PEM_X509_INFO_read_bio(): every certificate and CRL of a PEM buffer.
fn load_pem_infos(data: &[u8]) -> Loaded {
    let mut certs = Vec::new();
    let mut crls = Vec::new();
    for b in pem_blocks(data) {
        if is_cert_label(&b.label) {
            if let Some(x) = X509::parse(&b.der) {
                certs.push(Arc::new(x));
            }
        } else if b.label == "X509 CRL"
            && let Some(c) = Crl::parse(&b.der)
        {
            crls.push(c);
        }
    }
    (certs, crls)
}

/// X509_STORE_load_file(): None when the file is unreadable or holds
/// neither a certificate nor a CRL.
fn load_ca_file(path: &Path) -> Option<Arc<Loaded>> {
    let md = std::fs::metadata(path).ok()?;
    let mtime = md.modified().unwrap_or(std::time::UNIX_EPOCH);
    if let Ok(cache) = ca_file_cache().lock()
        && let Some((t, l, v)) = cache.get(path)
        && *t == mtime
        && *l == md.len()
    {
        return Some(v.clone());
    }
    let data = std::fs::read(path).ok()?;
    let loaded = load_pem_infos(&data);
    if loaded.0.is_empty() && loaded.1.is_empty() {
        return None;
    }
    let v = Arc::new(loaded);
    if let Ok(mut cache) = ca_file_cache().lock() {
        cache.insert(path.to_path_buf(), (mtime, md.len(), v.clone()));
    }
    Some(v)
}

impl Store {
    /// Every store certificate whose subject is `name`.
    fn lookup(&self, name: &Name) -> Vec<Arc<X509>> {
        let mut v: Vec<Arc<X509>> = self
            .certs
            .iter()
            .filter(|c| c.subject.eq(name))
            .cloned()
            .collect();
        if !self.dirs.is_empty() {
            let h = name.hash();
            for d in &self.dirs {
                for i in 0.. {
                    let p = d.join(format!("{:08x}.{}", h, i));
                    let Ok(data) = std::fs::read(&p) else { break };
                    let mut found = false;
                    for b in pem_blocks(&data) {
                        if is_cert_label(&b.label)
                            && let Some(x) = X509::parse(&b.der)
                            && x.subject.eq(name)
                        {
                            found = true;
                            if !v.iter().any(|c| c.der == x.der) {
                                v.push(Arc::new(x));
                            }
                        }
                    }
                    if !found
                        && let Some(x) = X509::parse(&data)
                        && x.subject.eq(name)
                    {
                        v.push(Arc::new(x));
                    }
                }
            }
        }
        v
    }

    fn contains(&self, x: &X509) -> bool {
        self.lookup(&x.subject).iter().any(|c| c.der == x.der)
    }

    fn crls_for(&self, issuer: &Name) -> Vec<Crl> {
        let mut v: Vec<Crl> = self
            .crls
            .iter()
            .filter(|c| c.issuer.eq(issuer))
            .cloned()
            .collect();
        if v.is_empty() && !self.dirs.is_empty() {
            let h = issuer.hash();
            for d in &self.dirs {
                for i in 0.. {
                    let p = d.join(format!("{:08x}.r{}", h, i));
                    let Ok(data) = std::fs::read(&p) else { break };
                    for b in pem_blocks(&data) {
                        if b.label == "X509 CRL"
                            && let Some(c) = Crl::parse(&b.der)
                            && c.issuer.eq(issuer)
                        {
                            v.push(c);
                        }
                    }
                }
            }
        }
        v
    }
}

// ===========================================================================
// X509_verify_cert()
// ===========================================================================

const X509_V_OK: i64 = 0;
const X509_V_ERR_UNSPECIFIED: i64 = 1;

/// X509_verify_cert_error_string()
fn verify_error_string(code: i64) -> &'static str {
    match code {
        0 => "ok",
        1 => "unspecified certificate verification error",
        2 => "unable to get issuer certificate",
        3 => "unable to get certificate CRL",
        4 => "unable to decrypt certificate's signature",
        5 => "unable to decrypt CRL's signature",
        6 => "unable to decode issuer public key",
        7 => "certificate signature failure",
        8 => "CRL signature failure",
        9 => "certificate is not yet valid",
        10 => "certificate has expired",
        11 => "CRL is not yet valid",
        12 => "CRL has expired",
        13 => "format error in certificate's notBefore field",
        14 => "format error in certificate's notAfter field",
        15 => "format error in CRL's lastUpdate field",
        16 => "format error in CRL's nextUpdate field",
        17 => "out of memory",
        18 => "self-signed certificate",
        19 => "self-signed certificate in certificate chain",
        20 => "unable to get local issuer certificate",
        21 => "unable to verify the first certificate",
        22 => "certificate chain too long",
        23 => "certificate revoked",
        24 => "invalid CA certificate",
        25 => "path length constraint exceeded",
        26 => "unsupported certificate purpose",
        27 => "certificate not trusted",
        28 => "certificate rejected",
        29 => "subject issuer mismatch",
        30 => "authority and subject key identifier mismatch",
        31 => "authority and issuer serial number mismatch",
        32 => "key usage does not include certificate signing",
        33 => "unable to get CRL issuer certificate",
        34 => "unhandled critical extension",
        35 => "key usage does not include CRL signing",
        36 => "unhandled critical CRL extension",
        62 => "hostname mismatch",
        65 => "proxy certificates not allowed, please set the appropriate flag",
        66 => "EE certificate key too weak",
        67 => "CA certificate key too weak",
        68 => "CA signature digest algorithm too weak",
        _ => "unknown certificate verification error",
    }
}

/// ssl_x509err2alert()
fn x509err_alert(code: i64) -> u8 {
    match code {
        10 | 12 => 45,
        9 | 11 | 13 | 14 | 15 | 16 | 27 | 28 | 62 | 66 | 68 => 42,
        23 => 44,
        7 | 8 | 4 | 5 => 51,
        26 => 43,
        2 | 3 | 18 | 19 | 20 | 21 | 22 | 24 | 25 | 33 => 48,
        17 | 1 => 80,
        _ => 46,
    }
}

/// Security level 1 (OpenSSL 3 default): 80 bits.
const SECURITY_BITS: i64 = 80;

fn sig_alg_security_bits(oid: &str) -> i64 {
    match oid {
        "1.2.840.113549.1.1.4" | "1.2.840.113549.1.1.2" => 39,
        "1.2.840.113549.1.1.5" | "1.2.840.10045.4.1" | "1.2.840.10040.4.3" => 63,
        _ => 128,
    }
}

struct VerifyResult {
    code: i64,
    chain: Vec<Arc<X509>>,
}

/// SSL_SERVER purpose check (check_purpose_ssl_server).
fn purpose_ok(x: &X509, ca: bool) -> bool {
    const SERVER_AUTH: &str = "1.3.6.1.5.5.7.3.1";
    const NS_SGC: &str = "2.16.840.1.113730.4.1";
    const MS_SGC: &str = "1.3.6.1.4.1.311.10.3.3";
    if let Some(eku) = &x.eku
        && !eku
            .iter()
            .any(|o| o == SERVER_AUTH || o == NS_SGC || o == MS_SGC)
    {
        return false;
    }
    if ca {
        // check_ssl_ca(): a CA (or a v1 root), and nsCertType SSL CA if set
        if let Some(ns) = x.ns_cert_type
            && ns & 0x04 == 0
        {
            return false;
        }
        return check_ca(x) != 0;
    }
    if let Some(ns) = x.ns_cert_type
        && ns & 0x40 == 0
    {
        return false;
    }
    if let Some(ku) = x.ku
        && ku & (KU_DIGITAL_SIGNATURE | KU_KEY_ENCIPHERMENT | KU_KEY_AGREEMENT) == 0
    {
        return false;
    }
    true
}

/// X509_check_ca()
fn check_ca(x: &X509) -> i32 {
    if let Some(ku) = x.ku
        && ku & KU_KEY_CERT_SIGN == 0
    {
        return 0;
    }
    match x.bc {
        Some((true, _)) => 1,
        Some((false, _)) => 0,
        None => {
            if x.version == 0 && x.is_self_signed() {
                3
            } else if x.ku.is_some() {
                4
            } else if x.ns_cert_type.is_some_and(|n| n & 0x07 != 0) {
                5
            } else {
                0
            }
        }
    }
}

/// X509_verify_cert() with X509_V_FLAG_TRUSTED_FIRST (+ PARTIAL_CHAIN unless
/// disabled or CRLs are checked) for the SSL server purpose. Stops at the
/// first error like OpenSSL's default verify callback does, returning it.
fn verify_chain(
    store: &Store,
    peer: &[Arc<X509>],
    partial: bool,
    crl_check: bool,
    now: i64,
) -> VerifyResult {
    let mut chain: Vec<Arc<X509>> = Vec::new();
    let Some(leaf) = peer.first() else {
        return VerifyResult {
            code: X509_V_ERR_UNSPECIFIED,
            chain,
        };
    };
    chain.push(leaf.clone());
    let fail = |code: i64, chain: Vec<Arc<X509>>| VerifyResult { code, chain };
    let partial = partial && !crl_check;
    // ---- build_chain ----
    let mut num_untrusted = 1;
    while num_untrusted > 0 {
        if chain.len() > 100 {
            return fail(22, chain);
        }
        let cur = chain.last().unwrap().clone();
        // trusted first
        let mut cands: Vec<Arc<X509>> = store
            .lookup(&cur.issuer)
            .into_iter()
            .filter(|c| check_issued(c, &cur))
            .collect();
        // prefer a currently valid issuer
        cands.sort_by_key(|c| !(c.not_before.unix <= now && now <= c.not_after.unix));
        if let Some(c) = cands.into_iter().next() {
            if c.der == cur.der {
                // self-signed and in the store
                num_untrusted = chain.len() - 1;
                break;
            }
            chain.push(c.clone());
            if c.is_self_signed() || partial {
                break;
            }
            // keep going through the store only
            loop {
                let top = chain.last().unwrap().clone();
                if top.is_self_signed() {
                    break;
                }
                let next = store
                    .lookup(&top.issuer)
                    .into_iter()
                    .find(|c| check_issued(c, &top) && !chain.iter().any(|x| x.der == c.der));
                match next {
                    Some(n) => chain.push(n),
                    None => return fail(2, chain),
                }
                if chain.len() > 100 {
                    return fail(22, chain);
                }
            }
            break;
        }
        // check_trust(): the certificate itself is in the store
        if (partial || cur.is_self_signed()) && store.contains(&cur) {
            num_untrusted = chain.len() - 1;
            break;
        }
        if cur.is_self_signed() {
            let code = if chain.len() == 1 { 18 } else { 19 };
            return fail(code, chain);
        }
        match peer[1..]
            .iter()
            .find(|c| check_issued(c, &cur) && !chain.iter().any(|x| x.der == c.der))
        {
            Some(c) => {
                chain.push(c.clone());
                num_untrusted += 1;
            }
            None => {
                let code = if num_untrusted < chain.len() { 2 } else { 20 };
                return fail(code, chain);
            }
        }
    }
    let n = chain.len();
    // ---- check_extensions ----
    let mut plen = 0usize;
    for (i, x) in chain.iter().enumerate() {
        let from_store = i >= num_untrusted;
        if x.has_unhandled_critical() {
            return fail(34, chain);
        }
        if i > 0 {
            let ret = check_ca(x);
            if ret == 0 {
                return fail(24, chain);
            }
            if !from_store && !purpose_ok(x, true) {
                return fail(26, chain);
            }
            if let Some((_, Some(pl))) = x.bc
                && plen > pl as usize + 1
            {
                return fail(25, chain);
            }
        } else if !from_store && !purpose_ok(x, false) {
            return fail(26, chain);
        }
        if i == 0 || !x.is_self_issued() {
            plen += 1;
        }
    }
    // ---- check_auth_level ----
    for (i, x) in chain.iter().enumerate() {
        let (_, _, _, sec) = pk_summary(x);
        if sec > 0 && sec < SECURITY_BITS {
            return fail(if i == 0 { 66 } else { 67 }, chain);
        }
        if i + 1 < n && sig_alg_security_bits(&x.sig_alg) < SECURITY_BITS {
            return fail(68, chain);
        }
    }
    // ---- check_revocation (CRL_CHECK | CRL_CHECK_ALL) ----
    if crl_check {
        let last = if chain[n - 1].is_self_signed() {
            n - 1
        } else {
            n
        };
        for i in 0..last {
            let x = &chain[i];
            let issuer = if i + 1 < n { &chain[i + 1] } else { x };
            let crls = store.crls_for(&x.issuer);
            let Some(crl) = crls.iter().find(|c| c.signed_by(issuer)).or(crls.first()) else {
                return fail(3, chain);
            };
            if let Some(ku) = issuer.ku
                && ku & 0x0200 == 0
            {
                return fail(35, chain);
            }
            if !crl.signed_by(issuer) {
                return fail(8, chain);
            }
            if crl.this_update > now {
                return fail(11, chain);
            }
            if let Some(nu) = crl.next_update
                && nu < now
            {
                return fail(12, chain);
            }
            if crl
                .revoked
                .iter()
                .any(|s| s.as_slice() == strip_int(&x.serial))
            {
                return fail(23, chain);
            }
        }
    }
    // ---- internal_verify ----
    for i in (0..n).rev() {
        let x = &chain[i];
        if i + 1 < n {
            let issuer = &chain[i + 1];
            if issuer.public_key().is_none() {
                return fail(6, chain);
            }
            if !x.signed_by(issuer) {
                return fail(7, chain);
            }
        }
        if now < x.not_before.unix {
            return fail(9, chain);
        }
        if now > x.not_after.unix {
            return fail(10, chain);
        }
    }
    VerifyResult {
        code: X509_V_OK,
        chain,
    }
}

// ===========================================================================
// Host name verification (ossl_verifyhost + hostcheck.c)
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Debug)]
enum PeerType {
    Dns,
    Ipv4,
    Ipv6,
}

fn peer_type(host: &str) -> PeerType {
    if host.parse::<std::net::Ipv4Addr>().is_ok() {
        PeerType::Ipv4
    } else if host.parse::<std::net::Ipv6Addr>().is_ok() {
        PeerType::Ipv6
    } else {
        PeerType::Dns
    }
}

fn host_is_ipnum(h: &str) -> bool {
    peer_type(h) != PeerType::Dns
}

fn pmatch(host: &[u8], pattern: &[u8]) -> bool {
    host.len() == pattern.len() && host.eq_ignore_ascii_case(pattern)
}

/// Curl_cert_hostcheck()
fn cert_hostcheck(pattern: &[u8], host: &[u8]) -> bool {
    if pattern.is_empty() || host.is_empty() {
        return false;
    }
    let mut host = host;
    let mut pattern = pattern;
    if host.last() == Some(&b'.') {
        host = &host[..host.len() - 1];
    }
    if pattern.last() == Some(&b'.') {
        pattern = &pattern[..pattern.len() - 1];
    }
    if !pattern.starts_with(b"*.") {
        return pmatch(host, pattern);
    }
    if host_is_ipnum(&String::from_utf8_lossy(host)) {
        return false;
    }
    let first_dot = pattern.iter().position(|&c| c == b'.');
    let last_dot = pattern.iter().rposition(|&c| c == b'.');
    match first_dot {
        None => pmatch(host, pattern),
        Some(fd) if Some(fd) == last_dot => pmatch(host, pattern),
        Some(fd) => match host.iter().position(|&c| c == b'.') {
            Some(hd) => pmatch(&host[hd..], &pattern[fd..]),
            None => false,
        },
    }
}

// ===========================================================================
// Client certificate and private key (cert_stuff)
// ===========================================================================

/// OpenSSL error-queue text for a failed fopen().
fn sys_error(e: &std::io::Error) -> String {
    let code = e.raw_os_error().unwrap_or(0);
    let msg = match e.kind() {
        std::io::ErrorKind::NotFound => "No such file or directory".to_string(),
        std::io::ErrorKind::PermissionDenied => "Permission denied".to_string(),
        _ => {
            let s = e.to_string();
            match s.find(" (os error") {
                Some(p) => s[..p].to_string(),
                None => s,
            }
        }
    };
    format!(
        "error:{:08X}:system library::{}",
        0x8000_0000u32 | code as u32,
        msg
    )
}

const ERR_PEM_NO_START_LINE: &str = "error:0480006C:PEM routines::no start line";
const ERR_ASN1_WRONG_TAG: &str = "error:068000A8:asn1 encoding routines::wrong tag";

fn bytes_path(b: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(b))
}

#[derive(PartialEq, Clone, Copy, Debug)]
enum FileType {
    Pem,
    Asn1,
    Engine,
    Provider,
    Pkcs12,
    Unknown,
}

/// ossl_do_file_type()
fn file_type(t: Option<&str>) -> FileType {
    match t {
        None | Some("") => FileType::Pem,
        Some(t) if t.eq_ignore_ascii_case("PEM") => FileType::Pem,
        Some(t) if t.eq_ignore_ascii_case("DER") => FileType::Asn1,
        Some(t) if t.eq_ignore_ascii_case("PROV") => FileType::Provider,
        Some(t) if t.eq_ignore_ascii_case("ENG") => FileType::Engine,
        Some(t) if t.eq_ignore_ascii_case("P12") => FileType::Pkcs12,
        _ => FileType::Unknown,
    }
}

fn evp_bytes_to_key_md5(pass: &[u8], salt: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut prev: Vec<u8> = Vec::new();
    while out.len() < len {
        let mut data = prev.clone();
        data.extend_from_slice(pass);
        data.extend_from_slice(salt);
        prev = purecrypto::hash::md5(&data).to_vec();
        out.extend_from_slice(&prev);
    }
    out.truncate(len);
    out
}

fn unpad_pkcs7(mut v: Vec<u8>, bs: usize) -> Option<Vec<u8>> {
    let p = *v.last()? as usize;
    if p == 0 || p > bs || p > v.len() || !v[v.len() - p..].iter().all(|&b| b as usize == p) {
        return None;
    }
    v.truncate(v.len() - p);
    Some(v)
}

/// Legacy "Proc-Type: 4,ENCRYPTED" PEM decryption (EVP_BytesToKey + CBC).
fn pem_legacy_decrypt(b: &PemBlock, pass: &[u8]) -> Option<Vec<u8>> {
    let dek = b.headers.iter().find(|(k, _)| k == "DEK-Info")?.1.clone();
    let (alg, iv_hex) = dek.split_once(',')?;
    let iv: Vec<u8> = (0..iv_hex.len() / 2)
        .map(|i| u8::from_str_radix(iv_hex.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect::<Option<_>>()?;
    let salt = iv.get(..8)?;
    let alg = alg.trim().to_ascii_lowercase();
    let kl = match alg.as_str() {
        "aes-128-cbc" => 16,
        "aes-192-cbc" | "des-ede3-cbc" => 24,
        "aes-256-cbc" => 32,
        "des-cbc" => 8,
        _ => return None,
    };
    let key = evp_bytes_to_key_md5(pass, salt, kl);
    cbc_decrypt(&alg, &key, &iv, &b.der)
}

/// Traditional RSA / EC private key DER -> PKCS#8 PrivateKeyInfo.
fn traditional_to_pkcs8(der: &[u8], kind: &str) -> Option<Vec<u8>> {
    let zero = der_tlv(T_INTEGER, &[0]);
    match kind {
        "RSA" => {
            let alg = der_seq(&[&oid_der("1.2.840.113549.1.1.1"), &[T_NULL, 0]]);
            Some(der_seq(&[&zero, &alg, &der_tlv(T_OCTET_STRING, der)]))
        }
        "EC" => {
            // the curve comes from ECPrivateKey.parameters [0]
            let c = der_one(der)?.children()?;
            let params = c.iter().find(|t| t.ctx() == Some(0))?;
            let curve = der_one(params.content)?;
            let alg = der_seq(&[&oid_der("1.2.840.10045.2.1"), curve.raw]);
            Some(der_seq(&[&zero, &alg, &der_tlv(T_OCTET_STRING, der)]))
        }
        _ => None,
    }
}

fn cbc_decrypt(cipher: &str, key: &[u8], iv: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    use purecrypto::cipher::{Aes128, Aes192, Aes256, Cbc, Cbc64, Des, TdesEde3};
    let mut d = data.to_vec();
    match cipher {
        "aes-128-cbc" | "aes-192-cbc" | "aes-256-cbc" => {
            let iv16: [u8; 16] = iv.try_into().ok()?;
            match key.len() {
                16 => Cbc::new(Aes128::new(key.try_into().ok()?), &iv16)
                    .decrypt(&mut d)
                    .ok()?,
                24 => Cbc::new(Aes192::new(key.try_into().ok()?), &iv16)
                    .decrypt(&mut d)
                    .ok()?,
                32 => Cbc::new(Aes256::new(key.try_into().ok()?), &iv16)
                    .decrypt(&mut d)
                    .ok()?,
                _ => return None,
            }
            unpad_pkcs7(d, 16)
        }
        "des-ede3-cbc" => {
            let iv8: [u8; 8] = iv.try_into().ok()?;
            Cbc64::new(TdesEde3::new(key.try_into().ok()?), &iv8)
                .decrypt(&mut d)
                .ok()?;
            unpad_pkcs7(d, 8)
        }
        "des-cbc" => {
            let iv8: [u8; 8] = iv.try_into().ok()?;
            Cbc64::new(Des::new(key.try_into().ok()?), &iv8)
                .decrypt(&mut d)
                .ok()?;
            unpad_pkcs7(d, 8)
        }
        _ => None,
    }
}

/// PKCS#8 EncryptedPrivateKeyInfo with PBES2 (PBKDF2 + a CBC cipher), with
/// OpenSSL's parameter range (purecrypto refuses < 10000 iterations, while
/// OpenSSL writes 2048).
fn pbes2_decrypt(der: &[u8], pass: &[u8]) -> Option<Vec<u8>> {
    use purecrypto::hash::{Sha1, Sha224, Sha256, Sha384, Sha512};
    use purecrypto::kdf::try_pbkdf2;
    let top = der_one(der)?.children()?;
    let alg = top.first()?.children()?;
    if tlv_oid(alg.first()?)? != "1.2.840.113549.1.5.13" {
        return None;
    }
    let data = top.get(1)?.content;
    let params = alg.get(1)?.children()?;
    let kdf = params.first()?.children()?;
    if tlv_oid(kdf.first()?)? != "1.2.840.113549.1.5.12" {
        return None;
    }
    let kp = kdf.get(1)?.children()?;
    let salt = kp.first()?.content;
    let iter = int_u64(kp.get(1)?.content)? as u32;
    let mut prf = "1.2.840.113549.2.7".to_string();
    let mut keylen = None;
    for t in &kp[2..] {
        if t.tag == T_INTEGER {
            keylen = int_u64(t.content).map(|v| v as usize);
        } else if t.tag == T_SEQ {
            prf = tlv_oid(t.children()?.first()?)?;
        }
    }
    let enc = params.get(1)?.children()?;
    let (cipher, kl) = match tlv_oid(enc.first()?)?.as_str() {
        "2.16.840.1.101.3.4.1.2" => ("aes-128-cbc", 16),
        "2.16.840.1.101.3.4.1.22" => ("aes-192-cbc", 24),
        "2.16.840.1.101.3.4.1.42" => ("aes-256-cbc", 32),
        "1.2.840.113549.3.7" => ("des-ede3-cbc", 24),
        "1.3.14.3.2.7" => ("des-cbc", 8),
        _ => return None,
    };
    let iv = enc.get(1)?.content;
    if iter == 0 || iter > 10_000_000 {
        return None;
    }
    let mut key = vec![0u8; keylen.unwrap_or(kl)];
    match prf.as_str() {
        "1.2.840.113549.2.7" => try_pbkdf2::<Sha1>(pass, salt, iter, &mut key).ok()?,
        "1.2.840.113549.2.8" => try_pbkdf2::<Sha224>(pass, salt, iter, &mut key).ok()?,
        "1.2.840.113549.2.9" => try_pbkdf2::<Sha256>(pass, salt, iter, &mut key).ok()?,
        "1.2.840.113549.2.10" => try_pbkdf2::<Sha384>(pass, salt, iter, &mut key).ok()?,
        "1.2.840.113549.2.11" => try_pbkdf2::<Sha512>(pass, salt, iter, &mut key).ok()?,
        _ => return None,
    }
    cbc_decrypt(cipher, &key, iv, data)
}

fn any_key_from_pkcs8(der: &[u8], pass: Option<&[u8]>) -> Option<purecrypto::x509::AnyPrivateKey> {
    use purecrypto::x509::{AnyPrivateKey, Pkcs8ReadOptions};
    let encrypted = der_one(der)
        .and_then(|t| t.children())
        .is_some_and(|c| c.len() == 2 && c[0].tag == T_SEQ && c[1].tag == T_OCTET_STRING);
    if encrypted {
        let plain = pbes2_decrypt(der, pass?)?;
        return AnyPrivateKey::from_pkcs8_der(&plain, Pkcs8ReadOptions::new()).ok();
    }
    AnyPrivateKey::from_pkcs8_der(der, Pkcs8ReadOptions::new()).ok()
}

/// d2i_PrivateKey(): PKCS#8, then traditional RSA, then traditional EC.
fn key_from_der(der: &[u8]) -> Option<purecrypto::x509::AnyPrivateKey> {
    if let Some(k) = any_key_from_pkcs8(der, None) {
        return Some(k);
    }
    for kind in ["RSA", "EC"] {
        if let Some(p8) = traditional_to_pkcs8(der, kind)
            && let Some(k) = any_key_from_pkcs8(&p8, None)
        {
            return Some(k);
        }
    }
    None
}

/// PEM_read_bio_PrivateKey() with the password callback.
fn key_from_pem(data: &[u8], pass: Option<&[u8]>) -> Option<purecrypto::x509::AnyPrivateKey> {
    for b in pem_blocks(data) {
        let kind = match b.label.as_str() {
            "PRIVATE KEY" => return any_key_from_pkcs8(&b.der, None),
            "ENCRYPTED PRIVATE KEY" => return any_key_from_pkcs8(&b.der, Some(pass?)),
            "RSA PRIVATE KEY" => "RSA",
            "EC PRIVATE KEY" => "EC",
            _ => continue,
        };
        let encrypted = b
            .headers
            .iter()
            .any(|(k, v)| k == "Proc-Type" && v.contains("ENCRYPTED"));
        let der = if encrypted {
            pem_legacy_decrypt(&b, pass?)?
        } else {
            b.der.clone()
        };
        return any_key_from_pkcs8(&traditional_to_pkcs8(&der, kind)?, None);
    }
    None
}

fn signing_key(k: purecrypto::x509::AnyPrivateKey) -> Option<purecrypto::tls::SigningKey> {
    use purecrypto::tls::SigningKey;
    use purecrypto::x509::AnyPrivateKey;
    match k {
        AnyPrivateKey::Rsa(r) => Some(SigningKey::Rsa(r)),
        AnyPrivateKey::Ecdsa(e) => Some(SigningKey::Ecdsa(e)),
        AnyPrivateKey::Ed25519(e) => Some(SigningKey::Ed25519(e)),
        AnyPrivateKey::Ed448(e) => Some(SigningKey::Ed448(e)),
        _ => None,
    }
}

fn key_matches(chain: &[Vec<u8>], key: &purecrypto::tls::SigningKey) -> bool {
    purecrypto::tls::Identity::new(chain.to_vec(), key.clone())
        .check_key_matches_leaf()
        .is_ok()
}

struct ClientIdentity {
    chain: Vec<Vec<u8>>,
    key: purecrypto::tls::SigningKey,
}

/// cert_stuff(): Ok(None) when no client certificate is configured.
fn load_client_identity(cfg: &TlsConfig) -> Result<Option<ClientIdentity>, TlsErr> {
    let cert_type = cfg.cert_type.as_deref();
    let mut ftype = file_type(cert_type);
    if cfg.cert.is_none()
        && cfg.cert_blob.is_none()
        && ftype != FileType::Engine
        && ftype != FileType::Provider
    {
        return Ok(None);
    }
    let pass = cfg.key_passwd.as_deref();
    let cert_name = cfg
        .cert
        .as_ref()
        .map(|c| utf8_lossy(c))
        .unwrap_or_else(|| "(nil)".to_string());
    let err = |m: String| Err((E_SSL_CERTPROBLEM, m));
    let mut chain: Vec<Vec<u8>> = Vec::new();
    let mut key: Option<purecrypto::tls::SigningKey> = None;
    let mut cert_done = false;
    let read_cert_src = || -> Result<Vec<u8>, String> {
        match &cfg.cert_blob {
            Some(b) => Ok(b.clone()),
            None => std::fs::read(bytes_path(cfg.cert.as_deref().unwrap_or_default()))
                .map_err(|e| sys_error(&e)),
        }
    };
    let src_name = if cfg.cert_blob.is_some() {
        "CURLOPT_SSLCERT_BLOB".to_string()
    } else {
        cert_name.clone()
    };
    match ftype {
        FileType::Pem => {
            let data = read_cert_src().map_err(|e| {
                (
                    E_SSL_CERTPROBLEM,
                    format!(
                        "could not load PEM client certificate from {}, OpenSSL error {}, \
                         (no key found, wrong pass phrase, or wrong file format?)",
                        src_name, e
                    ),
                )
            })?;
            for b in pem_blocks(&data) {
                if is_cert_label(&b.label) {
                    let der = der_one(&b.der).map(|t| t.raw.to_vec()).unwrap_or_default();
                    if X509::parse(&der).is_some() {
                        chain.push(der);
                    }
                }
            }
            if chain.is_empty() {
                return err(format!(
                    "could not load PEM client certificate from {}, OpenSSL error {}, \
                     (no key found, wrong pass phrase, or wrong file format?)",
                    src_name, ERR_PEM_NO_START_LINE
                ));
            }
        }
        FileType::Asn1 => {
            let data = read_cert_src().map_err(|e| {
                (
                    E_SSL_CERTPROBLEM,
                    format!(
                        "could not load ASN1 client certificate from {}, OpenSSL error {}, \
                         (no key found, wrong pass phrase, or wrong file format?)",
                        src_name, e
                    ),
                )
            })?;
            match der_one(&data)
                .filter(|t| t.tag == T_SEQ)
                .and_then(|t| X509::parse(t.raw))
            {
                Some(x) => chain.push(x.der),
                None => {
                    return err(format!(
                        "could not load ASN1 client certificate from {}, OpenSSL error {}, \
                         (no key found, wrong pass phrase, or wrong file format?)",
                        src_name, ERR_ASN1_WRONG_TAG
                    ));
                }
            }
        }
        FileType::Engine => return err("crypto engine not set, cannot load certificate".into()),
        FileType::Provider => {
            return err("crypto provider not set, cannot load certificate".into());
        }
        FileType::Pkcs12 => {
            let data = match &cfg.cert_blob {
                Some(b) => b.clone(),
                None => match std::fs::read(bytes_path(cfg.cert.as_deref().unwrap_or_default())) {
                    Ok(d) => d,
                    Err(_) => return err(format!("could not open PKCS12 file '{}'", cert_name)),
                },
            };
            let shown = if cfg.cert_blob.is_some() {
                "(memory blob)".to_string()
            } else {
                cert_name.clone()
            };
            if der_one(&data).is_none_or(|t| t.tag != T_SEQ) {
                return err(format!("error reading PKCS12 file '{}'", shown));
            }
            let pw = String::from_utf8_lossy(pass.unwrap_or_default()).into_owned();
            let parsed = match purecrypto::pkcs12::Pfx::parse(&data, &pw) {
                Ok(p) => p,
                Err(e) => {
                    let detail = match e {
                        purecrypto::pkcs12::Error::MacMismatch => {
                            "error:11800071:PKCS12 routines::mac verify failure"
                        }
                        purecrypto::pkcs12::Error::Malformed => {
                            "error:0680007B:asn1 encoding routines::header too long"
                        }
                        _ => "error:11800073:PKCS12 routines::pkcs12 cipherfinal error",
                    };
                    return err(format!(
                        "could not parse PKCS12 file, check password, OpenSSL error {}",
                        detail
                    ));
                }
            };
            let k = parsed
                .keys
                .first()
                .and_then(|k| any_key_from_pkcs8(k, None))
                .and_then(signing_key);
            // the certificate matching the key first, the rest as chain
            let mut certs = parsed.certs.clone();
            if let Some(k) = &k
                && let Some(pos) = certs
                    .iter()
                    .position(|c| key_matches(std::slice::from_ref(c), k))
            {
                let leaf = certs.remove(pos);
                certs.insert(0, leaf);
            }
            if certs.is_empty() {
                return err(format!(
                    "could not load PKCS12 client certificate, OpenSSL error {}",
                    "error:0A0C0102:SSL routines::passed a null parameter"
                ));
            }
            let Some(k) = k else {
                return err(format!(
                    "unable to use private key from PKCS12 file '{}'",
                    cert_name
                ));
            };
            if !key_matches(&certs[..1], &k) {
                return err(format!(
                    "private key from PKCS12 file '{}' does not match certificate in same file",
                    cert_name
                ));
            }
            chain = certs;
            key = Some(k);
            cert_done = true;
        }
        FileType::Unknown => {
            return err(format!(
                "not supported file type '{}' for certificate",
                cert_type.unwrap_or("")
            ));
        }
    }
    // ---- the private key ----
    let (key_file, key_blob): (Option<PathBuf>, Option<&Vec<u8>>) =
        if cfg.key.is_none() && cfg.key_blob.is_none() {
            (cfg.cert.as_deref().map(bytes_path), cfg.cert_blob.as_ref())
        } else {
            ftype = file_type(cfg.key_type.as_deref());
            (cfg.key.clone(), cfg.key_blob.as_ref())
        };
    let key_err = || -> TlsErr {
        (
            E_SSL_CERTPROBLEM,
            format!(
                "unable to set private key file: '{}' type {}",
                match (&key_blob, &key_file) {
                    (Some(_), _) | (None, None) => "(memory blob)".to_string(),
                    (None, Some(p)) => p.to_string_lossy().into_owned(),
                },
                cfg.key_type.as_deref().unwrap_or("PEM")
            ),
        )
    };
    match ftype {
        FileType::Pem | FileType::Asn1 if !(ftype == FileType::Pem && cert_done) => {
            let data = match (key_blob, &key_file) {
                (Some(b), _) => b.clone(),
                (None, Some(p)) => std::fs::read(p).map_err(|_| key_err())?,
                (None, None) => return Err(key_err()),
            };
            let k = if ftype == FileType::Pem {
                key_from_pem(&data, pass)
            } else {
                key_from_der(&data)
            };
            let k = k.and_then(signing_key).ok_or_else(key_err)?;
            if !key_matches(&chain[..1], &k) {
                return Err(key_err());
            }
            key = Some(k);
        }
        FileType::Pem => {}
        FileType::Engine => {
            return err("crypto engine not set, cannot load private key".into());
        }
        FileType::Provider => {
            return err("crypto provider not set, cannot load private key".into());
        }
        FileType::Pkcs12 => {
            if !cert_done {
                return err("file type P12 for private key not supported".into());
            }
        }
        _ => return err("not supported file type for private key".into()),
    }
    let Some(key) = key else {
        return err("Private key does not match the certificate public key".into());
    };
    Ok(Some(ClientIdentity { chain, key }))
}

// ===========================================================================
// Cipher lists, groups, signature algorithms
// ===========================================================================

struct Suite12 {
    name: &'static str,
    id: u16,
    ecdsa: bool,
    aes: u16,
    chacha: bool,
    sha384: bool,
}

/// OpenSSL's DEFAULT order for the TLS 1.2 suites purecrypto implements.
const SUITES12: &[Suite12] = &[
    Suite12 {
        name: "ECDHE-ECDSA-AES256-GCM-SHA384",
        id: 0xC02C,
        ecdsa: true,
        aes: 256,
        chacha: false,
        sha384: true,
    },
    Suite12 {
        name: "ECDHE-RSA-AES256-GCM-SHA384",
        id: 0xC030,
        ecdsa: false,
        aes: 256,
        chacha: false,
        sha384: true,
    },
    Suite12 {
        name: "ECDHE-ECDSA-CHACHA20-POLY1305",
        id: 0xCCA9,
        ecdsa: true,
        aes: 0,
        chacha: true,
        sha384: false,
    },
    Suite12 {
        name: "ECDHE-RSA-CHACHA20-POLY1305",
        id: 0xCCA8,
        ecdsa: false,
        aes: 0,
        chacha: true,
        sha384: false,
    },
    Suite12 {
        name: "ECDHE-ECDSA-AES128-GCM-SHA256",
        id: 0xC02B,
        ecdsa: true,
        aes: 128,
        chacha: false,
        sha384: false,
    },
    Suite12 {
        name: "ECDHE-RSA-AES128-GCM-SHA256",
        id: 0xC02F,
        ecdsa: false,
        aes: 128,
        chacha: false,
        sha384: false,
    },
];

/// OpenSSL cipher names / aliases that exist but match none of the suites
/// above (accepted by SSL_CTX_set_cipher_list, usable only through
/// TLS 1.3 here).
const KNOWN_OTHER_CIPHERS: &[&str] = &[
    "AES128-SHA",
    "AES256-SHA",
    "AES128-SHA256",
    "AES256-SHA256",
    "AES128-GCM-SHA256",
    "AES256-GCM-SHA384",
    "AES128-CCM",
    "AES256-CCM",
    "AES128-CCM8",
    "AES256-CCM8",
    "ECDHE-RSA-AES128-SHA",
    "ECDHE-RSA-AES256-SHA",
    "ECDHE-ECDSA-AES128-SHA",
    "ECDHE-ECDSA-AES256-SHA",
    "ECDHE-RSA-AES128-SHA256",
    "ECDHE-RSA-AES256-SHA384",
    "ECDHE-ECDSA-AES128-SHA256",
    "ECDHE-ECDSA-AES256-SHA384",
    "ECDHE-ECDSA-AES128-CCM",
    "ECDHE-ECDSA-AES256-CCM",
    "ECDHE-ECDSA-AES128-CCM8",
    "ECDHE-ECDSA-AES256-CCM8",
    "DHE-RSA-AES128-GCM-SHA256",
    "DHE-RSA-AES256-GCM-SHA384",
    "DHE-RSA-CHACHA20-POLY1305",
    "DHE-RSA-AES128-SHA",
    "DHE-RSA-AES256-SHA",
    "DHE-RSA-AES128-SHA256",
    "DHE-RSA-AES256-SHA256",
    "DHE-RSA-AES128-CCM",
    "DHE-RSA-AES256-CCM",
    "DES-CBC3-SHA",
    "ECDHE-RSA-DES-CBC3-SHA",
    "ECDHE-ECDSA-DES-CBC3-SHA",
    "DHE-DSS-AES128-SHA",
    "DHE-DSS-AES256-SHA",
    "DHE-DSS-AES128-GCM-SHA256",
    "DHE-DSS-AES256-GCM-SHA384",
    "ECDHE-ECDSA-ARIA128-GCM-SHA256",
    "ECDHE-ECDSA-ARIA256-GCM-SHA384",
    "ECDHE-RSA-ARIA128-GCM-SHA256",
    "ECDHE-RSA-ARIA256-GCM-SHA384",
    "CAMELLIA128-SHA",
    "CAMELLIA256-SHA",
    "PSK-AES128-CBC-SHA",
    "PSK-AES256-CBC-SHA",
    "NULL-SHA",
    "NULL-SHA256",
    "NULL-MD5",
    "RC4-SHA",
    "RC4-MD5",
    "SEED-SHA",
    "IDEA-CBC-SHA",
    "ADH-AES128-SHA",
    "ADH-AES256-SHA",
    "AECDH-AES128-SHA",
    "AECDH-AES256-SHA",
    "RSA",
    "kRSA",
    "aDSS",
    "DSS",
    "aDH",
    "DH",
    "kDHE",
    "kEDH",
    "DHE",
    "EDH",
    "ADH",
    "AECDH",
    "PSK",
    "kPSK",
    "aPSK",
    "SRP",
    "kSRP",
    "aSRP",
    "3DES",
    "DES",
    "RC4",
    "RC2",
    "IDEA",
    "SEED",
    "MD5",
    "SHA1",
    "SHA",
    "CAMELLIA",
    "CAMELLIA128",
    "CAMELLIA256",
    "ARIA",
    "ARIA128",
    "ARIA256",
    "CCM",
    "AESCCM",
    "AESCCM8",
    "NULL",
    "eNULL",
    "aNULL",
    "COMPLEMENTOFALL",
    "COMPLEMENTOFDEFAULT",
    "LOW",
    "MEDIUM",
    "EXPORT",
    "EXPORT40",
    "EXPORT56",
    "SSLv3",
    "TLSv1",
    "TLSv1.0",
    "kGOST",
    "aGOST",
    "GOST94",
    "GOST89MAC",
    "CHACHA20-POLY1305",
    "SUITEB128ONLY",
    "SUITEB128C2",
    "kECDHEPSK",
    "ECDHEPSK",
    "DHEPSK",
    "RSAPSK",
    "aNULL",
];

/// Does an OpenSSL cipher alias select `s`? None when the alias is unknown.
fn alias_matches(alias: &str, s: &Suite12) -> Option<bool> {
    let a = alias;
    Some(match a {
        "ALL" | "DEFAULT" | "HIGH" | "TLSv1.2" | "AEAD" | "kECDHE" | "kEECDH" | "ECDHE"
        | "EECDH" | "ECDH" | "FIPS" | "SUITEB128" | "SUITEB192" => true,
        "aRSA" => !s.ecdsa,
        "aECDSA" | "ECDSA" => s.ecdsa,
        "AES" => s.aes > 0,
        "AES128" => s.aes == 128,
        "AES256" => s.aes == 256,
        "AESGCM" => s.aes > 0,
        "CHACHA20" => s.chacha,
        "SHA256" => !s.sha384,
        "SHA384" => s.sha384,
        _ => {
            if let Some(x) = SUITES12.iter().find(|x| x.name == a) {
                return Some(x.id == s.id);
            }
            if KNOWN_OTHER_CIPHERS.contains(&a) {
                return Some(false);
            }
            return None;
        }
    })
}

/// SSL_CTX_set_cipher_list(): the selected TLS 1.2 suite ids, or None when
/// the list selects nothing at all.
fn parse_cipher_list(list: &str) -> Option<Vec<u16>> {
    let mut order: Vec<u16> = Vec::new();
    let mut killed: Vec<u16> = Vec::new();
    let mut matched_any = false;
    for item in list.split([':', ',', ' ', ';']).filter(|s| !s.is_empty()) {
        if item.starts_with('@') {
            if item == "@STRENGTH" {
                let strength = |id: &u16| {
                    let s = SUITES12.iter().find(|s| s.id == *id).unwrap();
                    -(if s.chacha { 256 } else { s.aes as i32 })
                };
                order.sort_by_key(strength);
            }
            continue;
        }
        let (op, body) = match item.as_bytes()[0] {
            b'!' | b'-' | b'+' => (item.as_bytes()[0], &item[1..]),
            _ => (0, item),
        };
        let mut known = true;
        let mut selected: Vec<u16> = Vec::new();
        for s in SUITES12 {
            let mut all = true;
            for part in body.split('+') {
                match alias_matches(part, s) {
                    Some(true) => {}
                    Some(false) => all = false,
                    None => {
                        known = false;
                        all = false;
                    }
                }
            }
            if all {
                selected.push(s.id);
            }
        }
        if known && op == 0 {
            matched_any = true;
        }
        match op {
            b'!' => {
                killed.extend(&selected);
                order.retain(|id| !selected.contains(id));
            }
            b'-' => order.retain(|id| !selected.contains(id)),
            b'+' => {
                let moved: Vec<u16> = order
                    .iter()
                    .copied()
                    .filter(|id| selected.contains(id))
                    .collect();
                order.retain(|id| !selected.contains(id));
                order.extend(moved);
            }
            _ => {
                for id in selected {
                    if !killed.contains(&id) && !order.contains(&id) {
                        order.push(id);
                    }
                }
            }
        }
    }
    // An empty `order` with `matched_any`: only ciphers OpenSSL knows but
    // purecrypto does not implement (TLS 1.3 still works).
    if order.is_empty() && !matched_any {
        return None;
    }
    Some(order)
}

/// SSL_CTX_set_ciphersuites(): TLS 1.3 suite ids, None when nothing valid.
fn parse_tls13_ciphers(list: &str) -> Option<Vec<u16>> {
    let mut v = Vec::new();
    let mut any = list.is_empty();
    for name in list.split(':') {
        let id = match name {
            "TLS_AES_128_GCM_SHA256" => 0x1301,
            "TLS_AES_256_GCM_SHA384" => 0x1302,
            "TLS_CHACHA20_POLY1305_SHA256" => 0x1303,
            "TLS_AES_128_CCM_SHA256"
            | "TLS_AES_128_CCM_8_SHA256"
            | "TLS_SHA256_SHA256"
            | "TLS_SHA384_SHA384" => {
                any = true;
                continue;
            }
            _ => continue,
        };
        any = true;
        if !v.contains(&id) {
            v.push(id);
        }
    }
    if any { Some(v) } else { None }
}

/// SSL_CTX_set1_groups_list() (OpenSSL 3.5 syntax): (groups, key shares).
fn parse_groups(list: &str) -> Option<(Vec<NamedGroup>, Vec<NamedGroup>)> {
    let mut groups: Vec<NamedGroup> = Vec::new();
    let mut shares: Vec<NamedGroup> = Vec::new();
    let mut first_of_tuple: Vec<NamedGroup> = Vec::new();
    for tuple in list.split('/') {
        let mut tuple_first = true;
        for item in tuple.split(':') {
            if item.is_empty() {
                continue;
            }
            let mut name = item;
            let mut ignore_unknown = false;
            let mut share = false;
            let mut remove = false;
            loop {
                if let Some(r) = name.strip_prefix('?') {
                    ignore_unknown = true;
                    name = r;
                } else if let Some(r) = name.strip_prefix('*') {
                    share = true;
                    name = r;
                } else if let Some(r) = name.strip_prefix('-') {
                    remove = true;
                    name = r;
                } else {
                    break;
                }
            }
            if name.eq_ignore_ascii_case("DEFAULT") {
                for g in [
                    NamedGroup::X25519MlKem768,
                    NamedGroup::X25519,
                    NamedGroup::Secp256r1,
                    NamedGroup::Secp384r1,
                    NamedGroup::Secp521r1,
                ] {
                    if !groups.contains(&g) {
                        groups.push(g);
                    }
                }
                continue;
            }
            let lname = name.to_ascii_lowercase();
            let g = match lname.as_str() {
                "secp256r1" | "p-256" | "prime256v1" => Some(NamedGroup::Secp256r1),
                "secp384r1" | "p-384" => Some(NamedGroup::Secp384r1),
                "secp521r1" | "p-521" => Some(NamedGroup::Secp521r1),
                "x25519" => Some(NamedGroup::X25519),
                "x25519mlkem768" => Some(NamedGroup::X25519MlKem768),
                "secp256r1mlkem768" => Some(NamedGroup::SecP256r1MlKem768),
                "secp384r1mlkem1024" => Some(NamedGroup::SecP384r1MlKem1024),
                "x448"
                | "ffdhe2048"
                | "ffdhe3072"
                | "ffdhe4096"
                | "ffdhe6144"
                | "ffdhe8192"
                | "mlkem512"
                | "mlkem768"
                | "mlkem1024"
                | "brainpoolp256r1tls13"
                | "brainpoolp384r1tls13"
                | "brainpoolp512r1tls13"
                | "brainpoolp256r1"
                | "brainpoolp384r1"
                | "brainpoolp512r1"
                | "secp224r1"
                | "secp192r1"
                | "secp256k1"
                | "p-224"
                | "p-192"
                | "prime192v1"
                | "sect163k1"
                | "sect163r2"
                | "sect233k1"
                | "sect233r1"
                | "sect283k1"
                | "sect283r1"
                | "sect409k1"
                | "sect409r1"
                | "sect571k1"
                | "sect571r1"
                | "curvesm2"
                | "curvesm2mlkem768"
                | "x448mlkem1024" => None,
                _ => {
                    if ignore_unknown {
                        continue;
                    }
                    return None;
                }
            };
            let Some(g) = g else { continue };
            if remove {
                groups.retain(|x| *x != g);
                shares.retain(|x| *x != g);
                continue;
            }
            if !groups.contains(&g) {
                groups.push(g);
            }
            if share && !shares.contains(&g) {
                shares.push(g);
            }
            if tuple_first {
                first_of_tuple.push(g);
                tuple_first = false;
            }
        }
    }
    if shares.is_empty() {
        shares = first_of_tuple.into_iter().take(1).collect();
    }
    Some((groups, shares))
}

/// SSL_CTX_set1_sigalgs_list() validation.
fn valid_sigalgs(list: &str) -> bool {
    if list.is_empty() {
        return false;
    }
    list.split(':').all(|item| {
        let names = [
            "rsa_pkcs1_sha1",
            "rsa_pkcs1_sha224",
            "rsa_pkcs1_sha256",
            "rsa_pkcs1_sha384",
            "rsa_pkcs1_sha512",
            "rsa_pss_rsae_sha256",
            "rsa_pss_rsae_sha384",
            "rsa_pss_rsae_sha512",
            "rsa_pss_pss_sha256",
            "rsa_pss_pss_sha384",
            "rsa_pss_pss_sha512",
            "ecdsa_secp256r1_sha256",
            "ecdsa_secp384r1_sha384",
            "ecdsa_secp521r1_sha512",
            "ecdsa_sha1",
            "ecdsa_sha224",
            "ed25519",
            "ed448",
            "ecdsa_brainpoolP256r1_sha256",
            "ecdsa_brainpoolP384r1_sha384",
            "ecdsa_brainpoolP512r1_sha512",
            "dsa_sha1",
            "dsa_sha224",
            "dsa_sha256",
            "dsa_sha384",
            "dsa_sha512",
            "mldsa44",
            "mldsa65",
            "mldsa87",
        ];
        if names.iter().any(|n| n.eq_ignore_ascii_case(item)) {
            return true;
        }
        let Some((alg, hash)) = item.split_once('+') else {
            return false;
        };
        let alg_ok = ["RSA", "RSA-PSS", "PSS", "ECDSA", "DSA"]
            .iter()
            .any(|a| a.eq_ignore_ascii_case(alg));
        let hash_ok = ["SHA1", "SHA224", "SHA256", "SHA384", "SHA512"]
            .iter()
            .any(|h| h.eq_ignore_ascii_case(hash));
        alg_ok && hash_ok
    })
}

/// OpenSSL's name of a negotiated cipher suite.
fn suite_name(id: u16) -> String {
    match id {
        0x1301 => "TLS_AES_128_GCM_SHA256".into(),
        0x1302 => "TLS_AES_256_GCM_SHA384".into(),
        0x1303 => "TLS_CHACHA20_POLY1305_SHA256".into(),
        _ => SUITES12
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.name.to_string())
            .unwrap_or_else(|| format!("0x{:04X}", id)),
    }
}

/// OBJ_nid2sn(SSL_get_peer_signature_type_nid())
fn sig_type_name(scheme: Option<u16>) -> &'static str {
    match scheme {
        Some(0x0804..=0x0806) | Some(0x0809..=0x080b) => "RSASSA-PSS",
        Some(0x0201) | Some(0x0401) | Some(0x0501) | Some(0x0601) | Some(0x0301) => "rsaEncryption",
        Some(0x0203)
        | Some(0x0303)
        | Some(0x0403)
        | Some(0x0503)
        | Some(0x0603)
        | Some(0x081a..=0x081c) => "id-ecPublicKey",
        Some(0x0807) => "ED25519",
        Some(0x0808) => "ED448",
        Some(0x0904) => "ML-DSA-44",
        Some(0x0905) => "ML-DSA-65",
        Some(0x0906) => "ML-DSA-87",
        _ => "UNDEF",
    }
}

fn version_name(v: Option<ProtocolVersion>) -> &'static str {
    match v {
        Some(ProtocolVersion::TLSv1_3) => "TLSv1.3",
        Some(ProtocolVersion::TLSv1_2) => "TLSv1.2",
        Some(ProtocolVersion::TLSv1_1) => "TLSv1.1",
        Some(ProtocolVersion::TLSv1_0) => "TLSv1",
        Some(ProtocolVersion::SSLv3) => "SSLv3",
        _ => "unknown",
    }
}

// ===========================================================================
// Key log (SSLKEYLOGFILE and the trace decoder's secrets)
// ===========================================================================

#[derive(Default)]
struct KeyStore {
    secrets: HashMap<String, Vec<u8>>,
}

struct KeySink {
    store: Arc<Mutex<KeyStore>>,
    file: Option<PathBuf>,
}

impl KeyLog for KeySink {
    fn log(&self, label: &str, client_random: &[u8; 32], secret: &[u8]) {
        if let Ok(mut s) = self.store.lock() {
            s.secrets.insert(label.to_string(), secret.to_vec());
        }
        if let Some(p) = &self.file {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
            {
                let hex = |d: &[u8]| d.iter().map(|b| format!("{:02x}", b)).collect::<String>();
                let _ = writeln!(f, "{} {} {}", label, hex(client_random), hex(secret));
            }
        }
    }
}

// ===========================================================================
// Record crypto for the trace decoder
// ===========================================================================

fn hmac(sha384: bool, key: &[u8], data: &[u8]) -> Vec<u8> {
    use purecrypto::hash::{Hmac, Sha256, Sha384};
    if sha384 {
        Hmac::<Sha384>::mac(key, data).to_vec()
    } else {
        Hmac::<Sha256>::mac(key, data).to_vec()
    }
}

fn hkdf_expand_label(sha384: bool, secret: &[u8], label: &str, len: usize) -> Vec<u8> {
    let full = format!("tls13 {}", label);
    let mut info = vec![(len >> 8) as u8, len as u8, full.len() as u8];
    info.extend_from_slice(full.as_bytes());
    info.push(0);
    let mut out = Vec::new();
    let mut prev: Vec<u8> = Vec::new();
    let mut i = 1u8;
    while out.len() < len {
        let mut d = prev.clone();
        d.extend_from_slice(&info);
        d.push(i);
        prev = hmac(sha384, secret, &d);
        out.extend_from_slice(&prev);
        i += 1;
    }
    out.truncate(len);
    out
}

fn tls12_prf(sha384: bool, secret: &[u8], label: &str, seed: &[u8], len: usize) -> Vec<u8> {
    let mut ls = label.as_bytes().to_vec();
    ls.extend_from_slice(seed);
    let mut a = hmac(sha384, secret, &ls);
    let mut out = Vec::new();
    while out.len() < len {
        let mut d = a.clone();
        d.extend_from_slice(&ls);
        out.extend(hmac(sha384, secret, &d));
        a = hmac(sha384, secret, &a);
    }
    out.truncate(len);
    out
}

enum AeadKind {
    Aes128(purecrypto::cipher::Gcm<purecrypto::cipher::Aes128>),
    Aes256(purecrypto::cipher::Gcm<purecrypto::cipher::Aes256>),
    Chacha(purecrypto::cipher::ChaCha20Poly1305),
}

struct RecordKeys {
    aead: AeadKind,
    iv: Vec<u8>,
    seq: u64,
    /// TLS 1.3 traffic secret (for KeyUpdate)
    secret: Vec<u8>,
    tls13: bool,
    sha384: bool,
}

/// (key length, sha384, chacha) of a suite.
fn suite_params(id: u16) -> Option<(usize, bool, bool)> {
    match id {
        0x1301 | 0xC02B | 0xC02F => Some((16, false, false)),
        0x1302 | 0xC02C | 0xC030 => Some((32, true, false)),
        0x1303 | 0xCCA8 | 0xCCA9 => Some((32, false, true)),
        _ => None,
    }
}

fn make_aead(key: &[u8], chacha: bool) -> Option<AeadKind> {
    use purecrypto::cipher::{Aes128, Aes256, ChaCha20Poly1305, Gcm};
    Some(if chacha {
        AeadKind::Chacha(ChaCha20Poly1305::new(key.try_into().ok()?))
    } else if key.len() == 16 {
        AeadKind::Aes128(Gcm::new(Aes128::new(key.try_into().ok()?)))
    } else {
        AeadKind::Aes256(Gcm::new(Aes256::new(key.try_into().ok()?)))
    })
}

impl RecordKeys {
    fn tls13(suite: u16, secret: &[u8]) -> Option<RecordKeys> {
        let (kl, sha384, chacha) = suite_params(suite)?;
        let key = hkdf_expand_label(sha384, secret, "key", kl);
        let iv = hkdf_expand_label(sha384, secret, "iv", 12);
        Some(RecordKeys {
            aead: make_aead(&key, chacha)?,
            iv,
            seq: 0,
            secret: secret.to_vec(),
            tls13: true,
            sha384,
        })
    }

    fn key_update(&mut self, suite: u16) {
        let next = hkdf_expand_label(self.sha384, &self.secret, "traffic upd", self.secret.len());
        if let Some(k) = RecordKeys::tls13(suite, &next) {
            *self = k;
        }
    }

    fn open(&self, nonce: &[u8; 12], aad: &[u8], data: &mut [u8], tag: &[u8; 16]) -> bool {
        match &self.aead {
            AeadKind::Aes128(g) => g.decrypt(nonce, aad, data, tag).is_ok(),
            AeadKind::Aes256(g) => g.decrypt(nonce, aad, data, tag).is_ok(),
            AeadKind::Chacha(c) => c.decrypt(nonce, aad, data, tag).is_ok(),
        }
    }

    /// Decrypt one record body; (content type, plaintext).
    fn decrypt(&mut self, hdr: &[u8], body: &[u8]) -> Option<(u8, Vec<u8>)> {
        let mut seqn = [0u8; 12];
        seqn[4..].copy_from_slice(&self.seq.to_be_bytes());
        let (nonce, ct, aad): ([u8; 12], &[u8], Vec<u8>) = if self.tls13 || self.iv.len() == 12 {
            let mut n = [0u8; 12];
            for i in 0..12 {
                n[i] = self.iv[i] ^ seqn[i];
            }
            if self.tls13 {
                (n, body, hdr.to_vec())
            } else {
                let plen = body.len().checked_sub(16)?;
                let mut aad = self.seq.to_be_bytes().to_vec();
                aad.extend_from_slice(&hdr[..3]);
                aad.extend_from_slice(&(plen as u16).to_be_bytes());
                (n, body, aad)
            }
        } else {
            // TLS 1.2 AES-GCM: 4-byte salt + 8-byte explicit nonce
            let mut n = [0u8; 12];
            n[..4].copy_from_slice(&self.iv);
            n[4..].copy_from_slice(body.get(..8)?);
            let ct = &body[8..];
            let plen = ct.len().checked_sub(16)?;
            let mut aad = self.seq.to_be_bytes().to_vec();
            aad.extend_from_slice(&hdr[..3]);
            aad.extend_from_slice(&(plen as u16).to_be_bytes());
            (n, ct, aad)
        };
        let split = ct.len().checked_sub(16)?;
        let mut data = ct[..split].to_vec();
        let tag: [u8; 16] = ct[split..].try_into().ok()?;
        if !self.open(&nonce, &aad, &mut data, &tag) {
            return None;
        }
        self.seq += 1;
        if self.tls13 {
            while data.last() == Some(&0) {
                data.pop();
            }
            let t = data.pop()?;
            Some((t, data))
        } else {
            Some((hdr[0], data))
        }
    }
}

// ===========================================================================
// OpenSSL message-callback trace (ossl_trace)
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Debug)]
enum Dir {
    In,
    Out,
}

#[derive(Default)]
struct DirState {
    hs: Vec<u8>,
    keys: Option<RecordKeys>,
    /// TLS 1.2: a ChangeCipherSpec went by in this direction.
    ccs: bool,
    /// TLS 1.3: the keylog label of the next traffic secret.
    next_label: Option<&'static str>,
}

const HRR_RANDOM: [u8; 32] = [
    0xCF, 0x21, 0xAD, 0x74, 0xE5, 0x9A, 0x61, 0x11, 0xBE, 0x1D, 0x8C, 0x02, 0x1E, 0x65, 0xB8, 0x91,
    0xC2, 0xA2, 0x11, 0x16, 0x7A, 0xBB, 0x8C, 0x5E, 0x07, 0x9E, 0x09, 0xE2, 0xC8, 0xA8, 0x33, 0x9C,
];

struct Tracer {
    /// Emit trace events (CURLOPT_DEBUGFUNCTION); the parse runs anyway.
    enabled: bool,
    pre_label: &'static str,
    version: Option<ProtocolVersion>,
    suite: Option<u16>,
    client_random: Vec<u8>,
    server_random: Vec<u8>,
    inn: DirState,
    out: DirState,
    keys: Arc<Mutex<KeyStore>>,
    /// Partial record bytes per direction.
    pending_in: Vec<u8>,
    pending_out: Vec<u8>,
    /// The TLS 1.3 compatibility CCS events, shown with the next record.
    deferred_ccs: Vec<TlsLog>,
    hrr: bool,
    ccs_out_shown: bool,
}

fn handshake_name(t: u8) -> &'static str {
    match t {
        0 => "Hello request",
        1 => "Client hello",
        2 => "Server hello",
        4 => "Newsession Ticket",
        5 => "End of early data",
        8 => "Encrypted Extensions",
        11 => "Certificate",
        12 => "Server key exchange",
        13 => "Request CERT",
        14 => "Server finished",
        15 => "CERT verify",
        16 => "Client key exchange",
        20 => "Finished",
        22 => "Certificate Status",
        23 => "Supplemental data",
        24 => "Key update",
        67 => "Next protocol",
        254 => "Message hash",
        _ => "Unknown",
    }
}

/// SSL_alert_desc_string_long()
fn alert_desc_long(d: u8) -> &'static str {
    match d {
        0 => "close notify",
        10 => "unexpected_message",
        20 => "bad record mac",
        21 => "decryption failed",
        22 => "record overflow",
        30 => "decompression failure",
        40 => "handshake failure",
        41 => "no certificate",
        42 => "bad certificate",
        43 => "unsupported certificate",
        44 => "certificate revoked",
        45 => "certificate expired",
        46 => "certificate unknown",
        47 => "illegal parameter",
        48 => "unknown CA",
        49 => "access denied",
        50 => "decode error",
        51 => "decrypt error",
        60 => "export restriction",
        70 => "protocol version",
        71 => "insufficient security",
        80 => "internal error",
        86 => "inappropriate fallback",
        90 => "user canceled",
        100 => "no renegotiation",
        110 => "unsupported extension",
        111 => "certificate unobtainable",
        112 => "unrecognized name",
        113 => "bad certificate status response",
        114 => "bad certificate hash value",
        115 => "unknown PSK identity",
        120 => "no application protocol",
        _ => "unknown",
    }
}

/// The OpenSSL error-queue reason of a received alert (SSL_R_*_ALERT_*).
fn alert_reason(d: u8) -> String {
    let r = match d {
        0 => return "error:0A000126:SSL routines::unexpected eof while reading".to_string(),
        10 => "ssl/tls alert unexpected message",
        20 => "ssl/tls alert bad record mac",
        21 => "tlsv1 alert decryption failed",
        22 => "tlsv1 alert record overflow",
        30 => "ssl/tls alert decompression failure",
        40 => "ssl/tls alert handshake failure",
        41 => "ssl/tls alert no certificate",
        42 => "ssl/tls alert bad certificate",
        43 => "ssl/tls alert unsupported certificate",
        44 => "ssl/tls alert certificate revoked",
        45 => "ssl/tls alert certificate expired",
        46 => "ssl/tls alert certificate unknown",
        47 => "ssl/tls alert illegal parameter",
        48 => "tlsv1 alert unknown ca",
        49 => "tlsv1 alert access denied",
        50 => "tlsv1 alert decode error",
        51 => "tlsv1 alert decrypt error",
        60 => "tlsv1 alert export restriction",
        70 => "tlsv1 alert protocol version",
        71 => "tlsv1 alert insufficient security",
        80 => "tlsv1 alert internal error",
        86 => "tlsv1 alert inappropriate fallback",
        90 => "tlsv1 alert user cancelled",
        100 => "tlsv1 alert no renegotiation",
        109 => "tlsv13 alert missing extension",
        110 => "tlsv1 unsupported extension",
        111 => "tlsv1 certificate unobtainable",
        112 => "tlsv1 unrecognized name",
        113 => "tlsv1 bad certificate status response",
        114 => "tlsv1 bad certificate hash value",
        115 => "tlsv1 alert unknown psk identity",
        116 => "tlsv13 alert certificate required",
        120 => "tlsv1 alert no application protocol",
        _ => {
            return format!(
                "error:{:08X}:SSL routines::reason({})",
                0x0A00_0000u32 + 1000 + d as u32,
                1000 + d as u32
            );
        }
    };
    format!(
        "error:{:08X}:SSL routines::{}",
        0x0A00_0000u32 + 1000 + d as u32,
        r
    )
}

impl Tracer {
    fn new(enabled: bool, max13: bool, keys: Arc<Mutex<KeyStore>>) -> Tracer {
        Tracer {
            enabled,
            pre_label: if max13 { "TLSv1.3" } else { "TLSv1.2" },
            version: None,
            suite: None,
            client_random: Vec::new(),
            server_random: Vec::new(),
            inn: DirState::default(),
            out: DirState::default(),
            keys,
            pending_in: Vec::new(),
            pending_out: Vec::new(),
            deferred_ccs: Vec::new(),
            hrr: false,
            ccs_out_shown: false,
        }
    }

    fn label(&self) -> &'static str {
        match self.version {
            Some(v) => version_name(Some(v)),
            None => self.pre_label,
        }
    }

    fn tls13(&self) -> bool {
        self.version == Some(ProtocolVersion::TLSv1_3)
    }

    fn dir(&mut self, d: Dir) -> &mut DirState {
        match d {
            Dir::In => &mut self.inn,
            Dir::Out => &mut self.out,
        }
    }

    fn text(&self, log: &mut Vec<TlsLog>, _d: Dir, s: String) {
        if self.enabled {
            log.push(TlsLog::Trace(s));
        }
    }

    fn data(&self, log: &mut Vec<TlsLog>, d: Dir, b: &[u8]) {
        if self.enabled {
            log.push(match d {
                Dir::In => TlsLog::DataIn(b.to_vec()),
                Dir::Out => TlsLog::DataOut(b.to_vec()),
            });
        }
    }

    fn dname(d: Dir) -> &'static str {
        match d {
            Dir::In => "IN",
            Dir::Out => "OUT",
        }
    }

    /// Bytes of a wire stream in direction `d` (complete records traced,
    /// a trailing partial record kept for later).
    fn stream(&mut self, d: Dir, bytes: &[u8], log: &mut Vec<TlsLog>) {
        let mut buf = match d {
            Dir::In => std::mem::take(&mut self.pending_in),
            Dir::Out => std::mem::take(&mut self.pending_out),
        };
        buf.extend_from_slice(bytes);
        let mut off = 0;
        while buf.len() - off >= 5 {
            let len = ((buf[off + 3] as usize) << 8) | buf[off + 4] as usize;
            if buf.len() - off < 5 + len {
                break;
            }
            let rec = buf[off..off + 5 + len].to_vec();
            self.record(d, &rec, log);
            off += 5 + len;
        }
        buf.drain(..off);
        match d {
            Dir::In => self.pending_in = buf,
            Dir::Out => self.pending_out = buf,
        }
    }

    fn load_keys(&mut self, d: Dir) {
        if self.dir(d).keys.is_some() {
            return;
        }
        let Some(suite) = self.suite else { return };
        if self.tls13() {
            let label = self.dir(d).next_label.unwrap_or(match d {
                Dir::In => "SERVER_HANDSHAKE_TRAFFIC_SECRET",
                Dir::Out => "CLIENT_HANDSHAKE_TRAFFIC_SECRET",
            });
            let secret = self
                .keys
                .lock()
                .ok()
                .and_then(|k| k.secrets.get(label).cloned());
            if let Some(s) = secret {
                self.dir(d).keys = RecordKeys::tls13(suite, &s);
            }
        } else {
            let master = self
                .keys
                .lock()
                .ok()
                .and_then(|k| k.secrets.get("CLIENT_RANDOM").cloned());
            let (Some(master), Some((kl, sha384, chacha))) = (master, suite_params(suite)) else {
                return;
            };
            let ivl = if chacha { 12 } else { 4 };
            let mut seed = self.server_random.clone();
            seed.extend_from_slice(&self.client_random);
            let kb = tls12_prf(sha384, &master, "key expansion", &seed, 2 * kl + 2 * ivl);
            let (ck, rest) = kb.split_at(kl);
            let (sk, rest) = rest.split_at(kl);
            let (civ, siv) = rest.split_at(ivl);
            let (key, iv) = match d {
                Dir::Out => (ck, civ),
                Dir::In => (sk, siv),
            };
            if let Some(aead) = make_aead(key, chacha) {
                self.dir(d).keys = Some(RecordKeys {
                    aead,
                    iv: iv.to_vec(),
                    seq: 0,
                    secret: Vec::new(),
                    tls13: false,
                    sha384,
                });
            }
        }
    }

    fn record(&mut self, d: Dir, rec: &[u8], log: &mut Vec<TlsLog>) {
        let (hdr, body) = rec.split_at(5);
        if d == Dir::Out
            && hdr[0] == 22
            && self.hrr
            && !self.ccs_out_shown
            && body.first() == Some(&1)
        {
            // after a HelloRetryRequest OpenSSL sends its compatibility CCS
            // ahead of the second ClientHello
            self.ccs_out_shown = true;
            self.data(log, d, &[20, 3, 3, 0, 1]);
            self.text(
                log,
                d,
                format!(
                    "{} (OUT), TLS change cipher, Change cipher spec (1):",
                    self.label()
                ),
            );
            self.data(log, d, &[1]);
        }
        if d == Dir::Out && hdr[0] == 20 && self.ccs_out_shown {
            return;
        }
        if d == Dir::Out && hdr[0] == 20 && self.tls13() {
            // OpenSSL writes the TLS 1.3 compatibility CCS right before its
            // second flight, purecrypto right after the ServerHello: show it
            // where OpenSSL would.
            self.ccs_out_shown = true;
            let mut tmp = Vec::new();
            self.data(&mut tmp, d, hdr);
            self.text(
                &mut tmp,
                d,
                format!(
                    "{} (OUT), TLS change cipher, Change cipher spec (1):",
                    self.label()
                ),
            );
            self.data(&mut tmp, d, body);
            self.deferred_ccs = tmp;
            return;
        }
        if d == Dir::Out && !self.deferred_ccs.is_empty() {
            let def = std::mem::take(&mut self.deferred_ccs);
            if hdr[0] != 21 {
                log.extend(def);
            }
        }
        self.data(log, d, hdr);
        let label = self.label();
        let dn = Tracer::dname(d);
        match hdr[0] {
            20 => {
                if d == Dir::Out || self.version != Some(ProtocolVersion::TLSv1_2) {
                    self.text(
                        log,
                        d,
                        format!(
                            "{} ({}), TLS change cipher, Change cipher spec (1):",
                            label, dn
                        ),
                    );
                    self.data(log, d, body);
                }
                if !self.tls13() {
                    self.dir(d).ccs = true;
                }
            }
            21 | 22 if !self.tls13() && self.dir(d).ccs => {
                self.load_keys(d);
                let plain = self.dir(d).keys.as_mut().and_then(|k| k.decrypt(hdr, body));
                if let Some((t, p)) = plain {
                    self.content(d, t, &p, log);
                }
            }
            21 | 22 => self.content(d, hdr[0], body, log),
            23 if !self.tls13() => {
                // TLS 1.2 application data: only the header is shown, but
                // the record still uses a sequence number
                if self.dir(d).ccs {
                    self.load_keys(d);
                    if let Some(k) = self.dir(d).keys.as_mut() {
                        k.seq += 1;
                    }
                }
            }
            23 => {
                self.load_keys(d);
                let plain = self.dir(d).keys.as_mut().and_then(|k| k.decrypt(hdr, body));
                if let Some((t, p)) = plain {
                    self.data(log, d, &[t]);
                    self.content(d, t, &p, log);
                }
            }
            _ => {}
        }
    }

    fn content(&mut self, d: Dir, t: u8, p: &[u8], log: &mut Vec<TlsLog>) {
        let dn = Tracer::dname(d);
        match t {
            21 => {
                if p.len() >= 2 {
                    let label = self.label();
                    let code = ((p[0] as u32) << 8) + p[1] as u32;
                    self.text(
                        log,
                        d,
                        format!(
                            "{} ({}), TLS alert, {} ({}):",
                            label,
                            dn,
                            alert_desc_long(p[1]),
                            code
                        ),
                    );
                    self.data(log, d, &p[..2]);
                }
            }
            22 => {
                self.dir(d).hs.extend_from_slice(p);
                loop {
                    let hs = &self.dir(d).hs;
                    if hs.len() < 4 {
                        break;
                    }
                    let len = ((hs[1] as usize) << 16) | ((hs[2] as usize) << 8) | hs[3] as usize;
                    if hs.len() < 4 + len {
                        break;
                    }
                    let msg: Vec<u8> = self.dir(d).hs.drain(..4 + len).collect();
                    self.message(d, &msg, log);
                }
            }
            _ => {}
        }
    }

    fn message(&mut self, d: Dir, msg: &[u8], log: &mut Vec<TlsLog>) {
        let label = self.label();
        let t = msg[0];
        self.text(
            log,
            d,
            format!(
                "{} ({}), TLS handshake, {} ({}):",
                label,
                Tracer::dname(d),
                handshake_name(t),
                t
            ),
        );
        self.data(log, d, msg);
        match (d, t) {
            (Dir::Out, 1) => {
                if let Some(r) = msg.get(6..38) {
                    self.client_random = r.to_vec();
                }
            }
            (Dir::In, 2) => self.server_hello(&msg[4..]),
            (_, 20) if self.tls13() => {
                let next = match d {
                    Dir::In => "SERVER_TRAFFIC_SECRET_0",
                    Dir::Out => "CLIENT_TRAFFIC_SECRET_0",
                };
                let st = self.dir(d);
                st.keys = None;
                st.next_label = Some(next);
            }
            (_, 24) if self.tls13() => {
                if let Some(s) = self.suite
                    && let Some(k) = self.dir(d).keys.as_mut()
                {
                    k.key_update(s);
                }
            }
            _ => {}
        }
    }

    fn server_hello(&mut self, b: &[u8]) {
        let Some(random) = b.get(2..34) else { return };
        if random == HRR_RANDOM {
            self.hrr = true;
        } else {
            self.server_random = random.to_vec();
        }
        let Some(&sid_len) = b.get(34) else { return };
        let p = 35 + sid_len as usize;
        let Some(cs) = b.get(p..p + 2) else { return };
        self.suite = Some(u16::from_be_bytes([cs[0], cs[1]]));
        let mut version = if b[0] == 3 && b[1] == 3 {
            ProtocolVersion::TLSv1_2
        } else {
            ProtocolVersion::from_u16(u16::from_be_bytes([b[0], b[1]]))
        };
        let mut q = p + 3;
        if let Some(el) = b.get(q..q + 2) {
            let end = q + 2 + u16::from_be_bytes([el[0], el[1]]) as usize;
            q += 2;
            while q + 4 <= end.min(b.len()) {
                let et = u16::from_be_bytes([b[q], b[q + 1]]);
                let l = u16::from_be_bytes([b[q + 2], b[q + 3]]) as usize;
                if et == 0x002b && l == 2 && q + 6 <= b.len() {
                    version = ProtocolVersion::from_u16(u16::from_be_bytes([b[q + 4], b[q + 5]]));
                }
                q += 4 + l;
            }
        }
        self.version = Some(version);
    }
}

// ===========================================================================
// The session
// ===========================================================================

/// The OpenSSL error-queue line for an engine error.
fn engine_error(e: &purecrypto::tls::Error) -> String {
    use purecrypto::tls::Error as E;
    let (code, reason): (u32, &str) = match e {
        E::AlertReceived(a) => return alert_reason(a.as_u8()),
        E::UnsupportedVersion => (0x0A00_0102, "unsupported protocol"),
        E::BadCertificate => (0x0A00_0086, "certificate verify failed"),
        E::CertificateRevoked | E::OcspResponseInvalid => {
            (0x0A00_0086, "certificate verify failed")
        }
        E::BadRecordMac => (0x0A00_0119, "decryption failed or bad record mac"),
        E::Decode => (0x0A00_009F, "length mismatch"),
        E::UnexpectedMessage => (0x0A00_00F4, "unexpected message"),
        E::HandshakeFailure => (0x0A00_0410, "ssl/tls alert handshake failure"),
        E::PeerMisbehaved => (0x0A00_007B, "bad signature"),
        E::IllegalParameter | E::UnsupportedExtension | E::MissingExtension => {
            (0x0A00_006E, "bad extension")
        }
        E::RecordOverflow => (0x0A00_00C6, "packet length too long"),
        E::DecryptError => (0x0A00_007B, "bad signature"),
        E::NoUsableCipherSuites => (0x0A00_00B5, "no ciphers available"),
        E::NoApplicationProtocol => (0x0A00_00EB, "no application protocol"),
        E::InappropriateFallback => (0x0A00_0175, "inappropriate fallback"),
        E::CertificateRequired => (0x0A00_00C7, "peer did not return a certificate"),
        _ => (0x0A00_0139, "record layer failure"),
    };
    format!("error:{:08X}:SSL routines::{}", code, reason)
}

fn alert_record(version_minor: u8, desc: u8) -> Vec<u8> {
    vec![0x15, 0x03, version_minor, 0x00, 0x02, 0x02, desc]
}

/// A TLS client connection, driven by the caller's socket I/O.
pub struct TlsSession {
    cfg: TlsConfig,
    conn: Option<Connection>,
    host: String,
    disp: String,
    ptype: PeerType,
    port: u16,
    log: Vec<TlsLog>,
    out: Vec<u8>,
    inbuf: Vec<u8>,
    in_off: usize,
    tracer: Tracer,
    store: Store,
    store_setup: bool,
    partial: bool,
    crl_check: bool,
    chain_checked: bool,
    vresult: Option<VerifyResult>,
    verify_result: i64,
    certinfo: Vec<Vec<Vec<u8>>>,
    failed: Option<TlsErr>,
    pending_err: Option<TlsErr>,
    no_protocols: bool,
    eof: bool,
    connected: bool,
    resumed_from: Option<TlsResumption>,
    server_hello_seen: bool,
    hs_scan: Vec<u8>,
    scan_done: bool,
    ocsp12: Option<Vec<u8>>,
}

impl TlsSession {
    /// Create a session for connecting to `hostname` (an IP literal, with or
    /// without brackets, disables SNI like libcurl). This is libcurl's
    /// `ossl_connect_step1`: configuration errors are returned here; the
    /// ClientHello is ready in [`take_outgoing`](Self::take_outgoing).
    pub fn new(cfg: &TlsConfig, hostname: &str, port: u16) -> Result<TlsSession, TlsErr> {
        Self::new_with_resumption(cfg, hostname, port, None)
    }

    /// [`new`](Self::new) offering a session from libcurl's session cache.
    pub fn new_with_resumption(
        cfg: &TlsConfig,
        hostname: &str,
        port: u16,
        resume: Option<&TlsResumption>,
    ) -> Result<TlsSession, TlsErr> {
        let mut log: Vec<TlsLog> = Vec::new();
        let host = match hostname.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
            Some(h) => h.to_string(),
            None => hostname.to_string(),
        };
        if host.is_empty() {
            return Err((E_SSL_CONNECT_ERROR, "SSL: no hostname".into()));
        }
        let ptype = peer_type(&host);
        let sni = if ptype == PeerType::Dns {
            host.trim_end_matches('.').to_ascii_lowercase()
        } else {
            host.clone()
        };

        // ---- ssl_prefs_check / ossl_init_method ----
        let minv = cfg.version_min & 0xffff;
        let maxv = if cfg.version_max != 0 {
            cfg.version_max & 0xffff_0000
        } else {
            cfg.version_min & 0xffff_0000
        };
        if !(0..8).contains(&minv) {
            return Err((
                E_SSL_CONNECT_ERROR,
                "Unrecognized parameter value passed via CURLOPT_SSLVERSION".into(),
            ));
        }
        if maxv != 0 && maxv != 1 << 16 && (maxv >> 16) < minv {
            return Err((
                E_SSL_CONNECT_ERROR,
                "CURL_SSLVERSION_MAX incompatible with CURL_SSLVERSION".into(),
            ));
        }
        match minv {
            2 => return Err((E_NOT_BUILT_IN, "No SSLv2 support".into())),
            3 => return Err((E_NOT_BUILT_IN, "No SSLv3 support".into())),
            _ => {}
        }
        let min_pv = if minv == 7 {
            ProtocolVersion::TLSv1_3
        } else {
            ProtocolVersion::TLSv1_2
        };
        let (max_pv, no_protocols) = match maxv >> 16 {
            4 | 5 => (ProtocolVersion::TLSv1_2, true),
            6 => (ProtocolVersion::TLSv1_2, false),
            _ => (ProtocolVersion::TLSv1_3, false),
        };
        let no_protocols = no_protocols
            || (min_pv == ProtocolVersion::TLSv1_3 && max_pv == ProtocolVersion::TLSv1_2);

        // ---- cipher selection ----
        let mut suites12: Vec<u16> = SUITES12.iter().map(|s| s.id).collect();
        if let Some(list) = &cfg.cipher_list
            && minv < 7
        {
            match parse_cipher_list(list) {
                Some(v) => suites12 = v,
                None => {
                    return Err((
                        E_SSL_CIPHER,
                        format!("failed setting cipher list: {}", list),
                    ));
                }
            }
            log.push(TlsLog::Info(format!("Cipher selection: {}", list)));
        }
        let mut suites13: Vec<u16> = vec![0x1302, 0x1303, 0x1301];
        if let Some(list) = &cfg.tls13_ciphers
            && (maxv == 0 || maxv >= 7 << 16)
        {
            match parse_tls13_ciphers(list) {
                Some(v) => suites13 = v,
                None => {
                    return Err((
                        E_SSL_CIPHER,
                        format!("failed setting TLS 1.3 cipher suite: {}", list),
                    ));
                }
            }
            log.push(TlsLog::Info(format!("TLS 1.3 cipher selection: {}", list)));
        }

        // ---- client certificate ----
        let identity = if cfg.cert.is_some() || cfg.cert_blob.is_some() || cfg.cert_type.is_some() {
            load_client_identity(cfg)?
        } else {
            None
        };

        // ---- groups / signature algorithms ----
        let mut groups = None;
        if let Some(c) = &cfg.curves {
            match parse_groups(c) {
                Some(g) => groups = Some(g),
                None => {
                    return Err((E_SSL_CIPHER, format!("failed setting curves list: '{}'", c)));
                }
            }
        }
        if let Some(s) = &cfg.signature_algorithms
            && !valid_sigalgs(s)
        {
            return Err((
                E_SSL_CIPHER,
                format!("failed setting signature algorithms: '{}'", s),
            ));
        }

        // ---- purecrypto configuration ----
        let keys = Arc::new(Mutex::new(KeyStore::default()));
        let sink = Arc::new(KeySink {
            store: keys.clone(),
            file: std::env::var_os("SSLKEYLOGFILE")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
        });
        let mut b: ConfigBuilder = Config::builder()
            .rng(Arc::new(purecrypto::rng::OsRng))
            .verify_certificates(false)
            .require_extended_master_secret(false)
            .server_name(sni)
            .key_log(sink)
            .versions(min_pv, max_pv);
        if !cfg.alpn.is_empty() {
            b = b.alpn(cfg.alpn.iter().map(|a| a.as_bytes().to_vec()).collect());
        }
        let mut suites = suites13.clone();
        if max_pv == ProtocolVersion::TLSv1_2 {
            suites.clear();
        }
        if min_pv != ProtocolVersion::TLSv1_3 {
            suites.extend(&suites12);
        }
        b = b.cipher_suites(&suites);
        if let Some((g, shares)) = &groups {
            if g.is_empty() {
                b = b.key_exchange_groups(&[NamedGroup::X25519]).key_shares(&[]);
            } else {
                b = b.key_exchange_groups(g).key_shares(shares);
            }
        }
        if let Some(id) = identity {
            b = b.try_identity(id.chain, id.key).map_err(|_| {
                (
                    E_SSL_CERTPROBLEM,
                    "Private key does not match the certificate public key".to_string(),
                )
            })?;
        }
        if let Some(r) = resume {
            b = b.resumption_session(r.session.clone());
            log.push(TlsLog::Info(format!(
                "SSL reusing session with ALPN '{}'",
                r.alpn.as_deref().unwrap_or("-")
            )));
        }
        let config = b.build();
        if !cfg.alpn.is_empty() {
            log.push(TlsLog::Info(format!(
                "ALPN: curl offers {}",
                cfg.alpn.join(",")
            )));
        }
        let mut pending_err = None;
        let conn = if no_protocols {
            None
        } else {
            match Connection::client(&config) {
                Ok(c) => Some(c),
                Err(e) => {
                    pending_err = Some((
                        E_SSL_CONNECT_ERROR,
                        format!("TLS connect error: {}", engine_error(&e)),
                    ));
                    None
                }
            }
        };
        let crl_check = cfg.crlfile.is_some();
        let mut s = TlsSession {
            partial: cfg.verify_peer && cfg.ssl_options & 4 == 0 && !crl_check,
            crl_check,
            cfg: cfg.clone(),
            conn,
            disp: host.clone(),
            host,
            ptype,
            port,
            log,
            out: Vec::new(),
            inbuf: Vec::new(),
            in_off: 0,
            tracer: Tracer::new(cfg.trace, max_pv == ProtocolVersion::TLSv1_3, keys),
            store: Store::default(),
            store_setup: false,
            chain_checked: false,
            vresult: None,
            verify_result: X509_V_ERR_UNSPECIFIED,
            certinfo: Vec::new(),
            failed: None,
            pending_err,
            no_protocols,
            eof: false,
            connected: false,
            resumed_from: resume.cloned(),
            server_hello_seen: false,
            hs_scan: Vec::new(),
            scan_done: false,
            ocsp12: None,
        };
        s.flush_conn();
        Ok(s)
    }

    /// Bytes read from the socket.
    pub fn feed(&mut self, wire: &[u8]) -> Result<(), TlsErr> {
        self.inbuf.extend_from_slice(wire);
        Ok(())
    }

    /// The socket reached EOF.
    pub fn feed_eof(&mut self) {
        self.eof = true;
    }

    /// Bytes that must be written to the socket.
    pub fn take_outgoing(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out)
    }

    /// Whether [`take_outgoing`](Self::take_outgoing) has something.
    pub fn wants_write(&self) -> bool {
        !self.out.is_empty()
    }

    /// The verbose events produced so far (drained).
    pub fn take_log(&mut self) -> Vec<TlsLog> {
        std::mem::take(&mut self.log)
    }

    /// The `infof()` lines produced so far (drains the whole log; trace
    /// events are dropped, use [`take_log`](Self::take_log) to get them).
    pub fn take_info_lines(&mut self) -> Vec<String> {
        self.take_log()
            .into_iter()
            .filter_map(|l| match l {
                TlsLog::Info(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    fn info(&mut self, s: impl Into<String>) {
        self.log.push(TlsLog::Info(s.into()));
    }

    fn push_out(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if self.tracer.enabled {
            let mut log = std::mem::take(&mut self.log);
            self.tracer.stream(Dir::Out, bytes, &mut log);
            self.log = log;
        }
        self.out.extend_from_slice(bytes);
    }

    fn flush_conn(&mut self) {
        loop {
            let Some(c) = self.conn.as_mut() else { return };
            match c.pop() {
                Ok(b) if !b.is_empty() => self.push_out(&b),
                _ => return,
            }
        }
    }

    fn discard_conn_output(&mut self) {
        if let Some(c) = self.conn.as_mut() {
            while let Ok(b) = c.pop() {
                if b.is_empty() {
                    break;
                }
            }
        }
    }

    fn fail(&mut self, e: TlsErr) -> Result<bool, TlsErr> {
        self.failed = Some(e.clone());
        Err(e)
    }

    fn send_alert(&mut self, desc: u8) {
        let minor = if self.server_hello_seen { 3 } else { 1 };
        let rec = alert_record(minor, desc);
        self.push_out(&rec);
    }

    /// The next complete record of the input buffer; Err on a record
    /// header OpenSSL would refuse.
    fn next_record(&mut self) -> Result<Option<Vec<u8>>, TlsErr> {
        let avail = &self.inbuf[self.in_off..];
        if avail.len() < 5 {
            return Ok(None);
        }
        let h = &avail[..5];
        let len = ((h[3] as usize) << 8) | h[4] as usize;
        if !(20..=24).contains(&h[0]) || h[1] != 3 {
            let hdr = h.to_vec();
            if self.tracer.enabled {
                self.log.push(TlsLog::DataIn(hdr));
            }
            return Err((
                E_SSL_CONNECT_ERROR,
                "error:0A00010B:SSL routines::wrong version number".to_string(),
            ));
        }
        if len > 16384 + 2048 {
            return Err((
                E_SSL_CONNECT_ERROR,
                "error:0A0000C6:SSL routines::packet length too long".to_string(),
            ));
        }
        if avail.len() < 5 + len {
            return Ok(None);
        }
        let rec: Vec<u8> = avail[..5 + len].to_vec();
        self.in_off += 5 + len;
        if self.in_off == self.inbuf.len() {
            self.inbuf.clear();
            self.in_off = 0;
        } else if self.in_off > 65536 && self.in_off * 2 > self.inbuf.len() {
            self.inbuf.drain(..self.in_off);
            self.in_off = 0;
        }
        if self.tracer.enabled {
            let mut log = std::mem::take(&mut self.log);
            self.tracer.record(Dir::In, &rec, &mut log);
            self.log = log;
        }
        if rec[0] == 22 && rec.get(5) == Some(&2) {
            self.server_hello_seen = true;
        }
        self.scan_plain_handshake(&rec);
        Ok(Some(rec))
    }

    /// Picks the TLS 1.2 CertificateStatus (OCSP staple) out of the
    /// plaintext handshake: purecrypto only exposes the TLS 1.3 one.
    fn scan_plain_handshake(&mut self, rec: &[u8]) {
        if self.scan_done {
            return;
        }
        match rec[0] {
            22 => self.hs_scan.extend_from_slice(&rec[5..]),
            _ => {
                self.scan_done = true;
                self.hs_scan.clear();
                return;
            }
        }
        while self.hs_scan.len() >= 4 {
            let h = &self.hs_scan;
            let len = ((h[1] as usize) << 16) | ((h[2] as usize) << 8) | h[3] as usize;
            if h.len() < 4 + len {
                break;
            }
            let msg: Vec<u8> = self.hs_scan.drain(..4 + len).collect();
            if msg[0] == 22 && msg.len() >= 8 && msg[4] == 1 {
                let l = ((msg[5] as usize) << 16) | ((msg[6] as usize) << 8) | msg[7] as usize;
                if let Some(o) = msg.get(8..8 + l) {
                    self.ocsp12 = Some(o.to_vec());
                }
            }
        }
    }

    /// Advance the handshake (libcurl's `ossl_connect_step2` and
    /// `ossl_connect_step3`). `Ok(true)` once connected and all of libcurl's
    /// post-handshake checks passed, `Ok(false)` when more input is needed
    /// (send [`take_outgoing`](Self::take_outgoing) first).
    pub fn handshake(&mut self) -> Result<bool, TlsErr> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        if self.connected {
            return Ok(true);
        }
        if !self.store_setup {
            self.store_setup = true;
            if self.no_protocols {
                self.send_alert(70);
            }
            if let Err(e) = self.setup_store() {
                return self.fail(e);
            }
            if self.no_protocols {
                return self.fail((
                    E_SSL_CONNECT_ERROR,
                    "TLS connect error: error:0A0000BF:SSL routines::no protocols available".into(),
                ));
            }
            if let Some(e) = self.pending_err.take() {
                return self.fail(e);
            }
        }
        loop {
            let rec = match self.next_record() {
                Ok(Some(r)) => r,
                Ok(None) => break,
                Err((code, msg)) => {
                    return self.fail((code, format!("TLS connect error: {}", msg)));
                }
            };
            let Some(conn) = self.conn.as_mut() else {
                break;
            };
            if let Err(e) = conn.feed(&rec) {
                self.flush_conn();
                let err = self.map_handshake_error(&e);
                return self.fail(err);
            }
            if !self.chain_checked
                && self
                    .conn
                    .as_ref()
                    .is_some_and(|c| !c.peer_certificates().is_empty())
            {
                self.chain_checked = true;
                let chain: Vec<Vec<u8>> = self.conn.as_ref().unwrap().peer_certificates().to_vec();
                self.check_chain(&chain);
                let code = self.vresult.as_ref().map(|v| v.code).unwrap_or(0);
                if self.cfg.verify_peer && code != X509_V_OK {
                    // SSL_VERIFY_PEER: abort before sending anything else
                    self.discard_conn_output();
                    self.send_alert(x509err_alert(code));
                    self.verify_result = code;
                    return self.fail((
                        E_PEER_FAILED_VERIFICATION,
                        format!("SSL certificate problem: {}", verify_error_string(code)),
                    ));
                }
            }
            self.flush_conn();
            if self
                .conn
                .as_ref()
                .is_some_and(|c| c.is_handshake_complete())
            {
                break;
            }
        }
        self.flush_conn();
        if self
            .conn
            .as_ref()
            .is_some_and(|c| c.is_handshake_complete())
        {
            self.connected = true;
            if let Err(e) = self.post_handshake() {
                self.connected = false;
                return self.fail(e);
            }
            return Ok(true);
        }
        if self.eof {
            self.send_alert(50);
            return self.fail((
                E_SSL_CONNECT_ERROR,
                "TLS connect error: error:0A000126:SSL routines::unexpected eof while reading"
                    .into(),
            ));
        }
        Ok(false)
    }

    fn map_handshake_error(&mut self, e: &purecrypto::tls::Error) -> TlsErr {
        use purecrypto::tls::Error as E;
        match e {
            E::CertificateRevoked => {
                self.verify_result = 23;
                (
                    E_PEER_FAILED_VERIFICATION,
                    format!("SSL certificate problem: {}", verify_error_string(23)),
                )
            }
            E::AlertReceived(a) if a.as_u8() == 116 => (
                E_SSL_CLIENTCERT,
                format!("TLS cert problem: {}", alert_reason(116)),
            ),
            E::AlertReceived(a) if a.as_u8() == 45 => (
                E_PEER_FAILED_VERIFICATION,
                "SSL certificate verification failed".to_string(),
            ),
            _ => (
                E_SSL_CONNECT_ERROR,
                format!("TLS connect error: {}", engine_error(e)),
            ),
        }
    }

    /// Curl_ssl_setup_x509_store() / ossl_populate_x509_store()
    fn setup_store(&mut self) -> Result<(), TlsErr> {
        let cfg = self.cfg.clone();
        let mut imported_blob = false;
        if cfg.verify_peer {
            if let Some(blob) = &cfg.ca_blob {
                let (certs, crls) = load_pem_infos(blob);
                if certs.is_empty() && crls.is_empty() {
                    return Err((
                        E_SSL_CACERT_BADFILE,
                        "error importing CA certificate blob".into(),
                    ));
                }
                self.store.certs.extend(certs);
                self.store.crls.extend(crls);
                imported_blob = true;
                self.info("successfully imported CA certificate blob");
            }
            let cafile = if cfg.ca_blob.is_some() {
                None
            } else {
                cfg.cainfo.clone()
            };
            let capath = cfg.capath.clone();
            if cafile.is_some() || capath.is_some() {
                if let Some(f) = &cafile {
                    match load_ca_file(f) {
                        Some(l) => {
                            self.store.certs.extend(l.0.iter().cloned());
                            self.store.crls.extend(l.1.iter().cloned());
                        }
                        None => {
                            if !imported_blob {
                                return Err((
                                    E_SSL_CACERT_BADFILE,
                                    format!("error setting certificate file: {}", f.display()),
                                ));
                            }
                            self.info("error setting certificate file, continuing anyway");
                        }
                    }
                }
                if let Some(p) = &capath {
                    self.store.dirs.push(p.clone());
                }
                let f = cafile
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "none".into());
                let p = capath
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "none".into());
                self.info(format!(" CAfile: {}", f));
                self.info(format!(" CApath: {}", p));
            }
        }
        if let Some(crl) = &cfg.crlfile {
            let crls: Vec<Crl> = std::fs::read(crl)
                .map(|d| load_pem_infos(&d).1)
                .unwrap_or_default();
            if crls.is_empty() {
                return Err((
                    E_SSL_CRL_BADFILE,
                    format!("error loading CRL file: {}", crl.display()),
                ));
            }
            self.store.crls.extend(crls);
            self.info("successfully loaded CRL file:");
            self.info(format!("  CRLfile: {}", crl.display()));
        }
        Ok(())
    }

    fn check_chain(&mut self, chain: &[Vec<u8>]) {
        let peer: Vec<Arc<X509>> = chain
            .iter()
            .filter_map(|d| X509::parse(d).map(Arc::new))
            .collect();
        let r = verify_chain(&self.store, &peer, self.partial, self.crl_check, now_unix());
        self.vresult = Some(r);
    }

    fn peer_chain(&self) -> Vec<Vec<u8>> {
        let live: Vec<Vec<u8>> = self
            .conn
            .as_ref()
            .map(|c| c.peer_certificates().to_vec())
            .unwrap_or_default();
        if live.is_empty()
            && let Some(r) = &self.resumed_from
        {
            return r.peer_chain.clone();
        }
        live
    }

    fn resumed(&self) -> bool {
        self.conn.as_ref().is_some_and(|c| c.resumed())
    }

    /// The success half of ossl_connect_step2 and Curl_oss_check_peer_cert().
    fn post_handshake(&mut self) -> Result<(), TlsErr> {
        let resumed = self.resumed();
        let (ver, suite, group, sig) = {
            let c = self.conn.as_ref().unwrap();
            (
                version_name(c.negotiated_version()),
                c.negotiated_cipher_suite()
                    .map(suite_name)
                    .unwrap_or_default(),
                c.negotiated_group()
                    .map(|g| g.name().to_string())
                    .unwrap_or_else(|| "[blank]".into()),
                if resumed {
                    "UNDEF"
                } else {
                    sig_type_name(c.peer_signature_scheme())
                },
            )
        };
        self.info(format!(
            "SSL connection using {} / {} / {} / {}",
            ver, suite, group, sig
        ));
        if !self.cfg.alpn.is_empty() {
            match self.negotiated_alpn() {
                Some(p) => self.info(format!("ALPN: server accepted {}", p)),
                None => self.info("ALPN: server did not agree on a protocol. Uses default."),
            }
        }
        // ---- Curl_oss_check_peer_cert ----
        let strict = self.cfg.verify_peer || self.cfg.verify_host;
        let chain_der = self.peer_chain();
        let peer: Vec<X509> = chain_der.iter().filter_map(|d| X509::parse(d)).collect();
        if self.cfg.certinfo {
            self.certinfo = peer.iter().map(cert_info_entries).collect();
        }
        let Some(leaf) = peer.first().cloned() else {
            if !strict {
                return Ok(());
            }
            return Err((
                E_PEER_FAILED_VERIFICATION,
                "SSL: could not get peer certificate".into(),
            ));
        };
        if !self.chain_checked {
            self.chain_checked = true;
            match &self.resumed_from {
                Some(r) if resumed => {
                    let chain = r
                        .verified_chain
                        .iter()
                        .filter_map(|d| X509::parse(d).map(Arc::new))
                        .collect();
                    self.vresult = Some(VerifyResult {
                        code: r.verify_result,
                        chain,
                    });
                }
                _ => self.check_chain(&chain_der),
            }
        }
        self.info(if self.cfg.proxy {
            "Proxy certificate:"
        } else {
            "Server certificate:"
        });
        let subject = latin1(&name_print(&leaf.subject, NameFmt::SplusSpc));
        self.info(format!(" subject: {}", subject));
        self.info(format!(" start date: {}", time_print(&leaf.not_before)));
        self.info(format!(" expire date: {}", time_print(&leaf.not_after)));
        if self.cfg.verify_host {
            self.verify_host(&leaf)?;
        }
        let issuer = latin1(&name_print(&leaf.issuer, NameFmt::SplusSpc));
        self.info(format!(" issuer: {}", issuer));
        if self.cfg.issuercert.is_some() || self.cfg.issuercert_blob.is_some() {
            self.check_issuer_cert(&leaf, strict)?;
        }
        let lerr = self.vresult.as_ref().map(|v| v.code).unwrap_or(X509_V_OK);
        self.verify_result = lerr;
        if lerr != X509_V_OK {
            if self.cfg.verify_peer {
                return Err((
                    E_PEER_FAILED_VERIFICATION,
                    format!(
                        "SSL certificate verify result: {} ({})",
                        verify_error_string(lerr),
                        lerr
                    ),
                ));
            }
            self.info(format!(
                " SSL certificate verify result: {} ({}), continuing anyway.",
                verify_error_string(lerr),
                lerr
            ));
        } else {
            self.info(" SSL certificate verify ok.");
        }
        // infof_certstack
        let stack: Vec<Arc<X509>> = if lerr != X509_V_OK {
            peer.iter().cloned().map(Arc::new).collect()
        } else {
            self.vresult
                .as_ref()
                .map(|v| v.chain.clone())
                .unwrap_or_default()
        };
        for (lvl, x) in stack.iter().enumerate() {
            let (tname, group, bits, sec) = pk_summary(x);
            self.info(format!(
                "  Certificate level {}: Public key type {}{} ({}/{} Bits/secBits), signed using {}",
                lvl,
                tname,
                group.map(|g| format!("/{}", g)).unwrap_or_default(),
                bits,
                sec,
                oid_ln(&x.sig_alg)
            ));
        }
        if self.cfg.verify_status && !resumed {
            self.verify_status(&peer)?;
        }
        if let Some(pin) = self.cfg.pinned_pubkey.clone()
            && let Err(e) = self.pin_pubkey(&pin, &leaf.spki)
        {
            return Err(e);
        }
        Ok(())
    }

    /// ossl_verifyhost()
    fn verify_host(&mut self, leaf: &X509) -> Result<(), TlsErr> {
        let host = self.host.clone();
        let disp = self.disp.clone();
        let addr: Vec<u8> = match self.ptype {
            PeerType::Ipv4 => host
                .parse::<std::net::Ipv4Addr>()
                .map(|a| a.octets().to_vec())
                .unwrap_or_default(),
            PeerType::Ipv6 => host
                .parse::<std::net::Ipv6Addr>()
                .map(|a| a.octets().to_vec())
                .unwrap_or_default(),
            PeerType::Dns => Vec::new(),
        };
        let target = if self.ptype == PeerType::Dns { 2 } else { 7 };
        let mut matched = false;
        let mut dns_name = false;
        let mut ip_address = false;
        if let Some(alts) = leaf.san() {
            for (t, v) in alts {
                if t == 2 {
                    dns_name = true;
                } else if t == 7 {
                    ip_address = true;
                }
                if t != target {
                    continue;
                }
                if t == 2 {
                    if !v.contains(&0) && cert_hostcheck(&v, host.as_bytes()) {
                        self.info(format!(
                            " subjectAltName: host \"{}\" matched cert's \"{}\"",
                            disp,
                            utf8_lossy(&v)
                        ));
                        matched = true;
                        break;
                    }
                } else if v == addr {
                    self.info(format!(
                        " subjectAltName: host \"{}\" matched cert's IP address!",
                        disp
                    ));
                    matched = true;
                }
            }
        }
        if matched {
            return Ok(());
        }
        let tname = match self.ptype {
            PeerType::Dns => "hostname",
            PeerType::Ipv4 => "ipv4 address",
            PeerType::Ipv6 => "ipv6 address",
        };
        if dns_name || ip_address {
            self.info(format!(" subjectAltName does not match {} {}", tname, disp));
            return Err((
                E_PEER_FAILED_VERIFICATION,
                format!(
                    "SSL: no alternative certificate subject name matches target {} '{}'",
                    tname, disp
                ),
            ));
        }
        let Some(cn) = leaf.subject.last("2.5.4.3") else {
            return Err((
                E_PEER_FAILED_VERIFICATION,
                "SSL: unable to obtain common name from peer certificate".into(),
            ));
        };
        let cn_bytes: Vec<u8> = if cn.tag == T_UTF8 {
            cn.value.clone()
        } else {
            string_code_points(cn.tag, &cn.value)
                .unwrap_or_default()
                .iter()
                .filter_map(|&c| char::from_u32(c))
                .collect::<String>()
                .into_bytes()
        };
        if cn_bytes.is_empty() {
            return Err((E_OUT_OF_MEMORY, "Out of memory".into()));
        }
        if cn_bytes.contains(&0) {
            return Err((
                E_PEER_FAILED_VERIFICATION,
                "SSL: illegal cert name field".into(),
            ));
        }
        let cn_s = utf8_lossy(&cn_bytes);
        if !cert_hostcheck(&cn_bytes, host.as_bytes()) {
            return Err((
                E_PEER_FAILED_VERIFICATION,
                format!(
                    "SSL: certificate subject name '{}' does not match target hostname '{}'",
                    cn_s, disp
                ),
            ));
        }
        self.info(format!(" common name: {} (matched)", cn_s));
        Ok(())
    }

    fn check_issuer_cert(&mut self, leaf: &X509, strict: bool) -> Result<(), TlsErr> {
        let name = self
            .cfg
            .issuercert
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(nil)".into());
        let generic = "Issuer check against peer certificate failed".to_string();
        let err = |m: String| -> TlsErr {
            (E_SSL_ISSUER_ERROR, if strict { m } else { generic.clone() })
        };
        let data = match &self.cfg.issuercert_blob {
            Some(b) => b.clone(),
            None => match std::fs::read(self.cfg.issuercert.as_ref().unwrap()) {
                Ok(d) => d,
                Err(_) => return Err(err(format!("SSL: Unable to open issuer cert ({})", name))),
            },
        };
        let issuer = pem_blocks(&data)
            .into_iter()
            .find(|b| is_cert_label(&b.label))
            .and_then(|b| X509::parse(&b.der));
        let Some(issuer) = issuer else {
            return Err(err(format!("SSL: Unable to read issuer cert ({})", name)));
        };
        if !check_issued(&issuer, leaf) {
            return Err(err(format!(
                "SSL: Certificate issuer check failed ({})",
                name
            )));
        }
        self.info(format!(" SSL certificate issuer check ok ({})", name));
        Ok(())
    }

    /// verifystatus(): the stapled OCSP response.
    fn verify_status(&mut self, peer: &[X509]) -> Result<(), TlsErr> {
        use purecrypto::x509::{OcspCertStatus, OcspResponse, OcspResponseStatus};
        let e = |m: &str| -> TlsErr { (E_SSL_INVALIDCERTSTATUS, m.to_string()) };
        let resp = self
            .conn
            .as_ref()
            .and_then(|c| c.peer_ocsp_response().map(|r| r.to_vec()))
            .or_else(|| self.ocsp12.clone());
        let Some(resp) = resp else {
            return Err(e("No OCSP response received"));
        };
        let Ok(r) = OcspResponse::from_der(resp) else {
            return Err(e("Invalid OCSP response"));
        };
        match r.response_status() {
            Ok(OcspResponseStatus::Successful) => {}
            Ok(st) => {
                let (name, n) = match st {
                    OcspResponseStatus::MalformedRequest => ("malformedrequest", 1),
                    OcspResponseStatus::InternalError => ("internalerror", 2),
                    OcspResponseStatus::TryLater => ("trylater", 3),
                    OcspResponseStatus::SigRequired => ("sigrequired", 5),
                    OcspResponseStatus::Unauthorized => ("unauthorized", 6),
                    OcspResponseStatus::Successful => ("successful", 0),
                };
                return Err(e(&format!(
                    "Invalid OCSP response status: {} ({})",
                    name, n
                )));
            }
            Err(_) => return Err(e("Invalid OCSP response")),
        }
        let leaf = &peer[0];
        let Some(issuer) = peer.iter().skip(1).find(|c| check_issued(c, leaf)) else {
            return Err(e("Error computing OCSP ID"));
        };
        let (Ok(l), Ok(i)) = (
            purecrypto::x509::Certificate::from_der(leaf.der.clone()),
            purecrypto::x509::Certificate::from_der(issuer.der.clone()),
        ) else {
            return Err(e("Error computing OCSP ID"));
        };
        let now = purecrypto::x509::Time::from_unix(now_unix().max(0) as u64);
        let status = match r.check_for_cert(&l, &i, Some(&now)) {
            Ok(s) => s,
            Err(purecrypto::x509::Error::Verification) => {
                return Err(e("OCSP response verification failed"));
            }
            Err(_) => return Err(e("Could not find certificate ID in OCSP response")),
        };
        match status {
            OcspCertStatus::Good => {
                self.info("SSL certificate status: good (0)");
                Ok(())
            }
            OcspCertStatus::Revoked { reason, .. } => {
                self.info("SSL certificate status: revoked (1)");
                let (name, n) = match reason.map(|r| r as u8) {
                    Some(1) => ("keyCompromise", 1),
                    Some(2) => ("cACompromise", 2),
                    Some(3) => ("affiliationChanged", 3),
                    Some(4) => ("superseded", 4),
                    Some(5) => ("cessationOfOperation", 5),
                    Some(6) => ("certificateHold", 6),
                    Some(8) => ("removeFromCRL", 8),
                    Some(0) => ("unspecified", 0),
                    _ => ("(UNKNOWN)", -1),
                };
                Err(e(&format!(
                    "SSL certificate revocation reason: {} ({})",
                    name, n
                )))
            }
            OcspCertStatus::Unknown => {
                self.info("SSL certificate status: unknown (2)");
                // libcurl fails without a failf(): the error buffer stays empty
                Err(e(""))
            }
        }
    }

    /// Curl_pin_peer_pubkey()
    fn pin_pubkey(&mut self, pin: &str, spki: &[u8]) -> Result<(), TlsErr> {
        let fail = || -> TlsErr {
            (
                E_SSL_PINNEDPUBKEYNOTMATCH,
                "SSL: public key does not match pinned public key".into(),
            )
        };
        if pin.starts_with("sha256//") {
            let digest = purecrypto::hash::sha256(spki);
            let encoded = b64_encode(&digest);
            self.info(format!(" public key hash: sha256//{}", encoded));
            let ok = pin.split(";sha256//").enumerate().any(|(i, p)| {
                let p = if i == 0 { &p[8..] } else { p };
                p == encoded
            });
            return if ok { Ok(()) } else { Err(fail()) };
        }
        let Ok(data) = std::fs::read(pin) else {
            return Err(fail());
        };
        if data.len() > 1_048_576 || spki.len() > data.len() {
            return Err(fail());
        }
        if spki.len() == data.len() {
            return if spki == data.as_slice() {
                Ok(())
            } else {
                Err(fail())
            };
        }
        let text = String::from_utf8_lossy(&data);
        let Some(begin) = text.find("-----BEGIN PUBLIC KEY-----") else {
            return Err(fail());
        };
        if begin != 0 && text.as_bytes()[begin - 1] != b'\n' {
            return Err(fail());
        }
        let after = &text[begin + 26..];
        let Some(end) = after.find("\n-----END PUBLIC KEY-----") else {
            return Err(fail());
        };
        let b64: String = after[..end]
            .chars()
            .filter(|c| *c != '\n' && *c != '\r')
            .collect();
        match b64_decode(b64.as_bytes()) {
            Some(der) if der == spki => Ok(()),
            _ => Err(fail()),
        }
    }

    /// Queue application data (then send [`take_outgoing`](Self::take_outgoing)).
    pub fn send(&mut self, plain: &[u8]) -> Result<(), TlsErr> {
        if let Some(e) = &self.failed {
            return Err((E_SEND_ERROR, e.1.clone()));
        }
        let Some(c) = self.conn.as_mut() else {
            return Err((E_SEND_ERROR, "SSL_write() error: not connected".into()));
        };
        // records are at most 16 KiB of plaintext
        for chunk in plain.chunks(16384) {
            if let Err(e) = c.send(chunk) {
                // (ossl_strerror()'s return value skips the version prefix)
                let msg = format!("SSL_write() error: {}", engine_error(&e));
                return Err((E_SEND_ERROR, msg));
            }
        }
        self.flush_conn();
        Ok(())
    }

    /// Decrypted application data available so far (empty when none; after
    /// a close_notify [`peer_closed`](Self::peer_closed) turns true).
    pub fn recv(&mut self) -> Result<Vec<u8>, TlsErr> {
        let mut out = Vec::new();
        if !self.connected {
            return Ok(out);
        }
        loop {
            let rec = match self.next_record() {
                Ok(Some(r)) => r,
                Ok(None) => break,
                Err((_, msg)) => {
                    if !out.is_empty() {
                        break;
                    }
                    return Err((
                        E_RECV_ERROR,
                        format!("OpenSSL SSL_read: {}: {}, errno 0", BACKEND_VERSION, msg),
                    ));
                }
            };
            let c = self.conn.as_mut().unwrap();
            if let Err(e) = c.feed(&rec) {
                out.extend(c.recv().unwrap_or_default());
                self.flush_conn();
                let msg = format!(
                    "OpenSSL SSL_read: {}: {}, errno 0",
                    BACKEND_VERSION,
                    engine_error(&e)
                );
                self.failed = Some((E_RECV_ERROR, msg.clone()));
                if !out.is_empty() {
                    return Ok(out);
                }
                return Err((E_RECV_ERROR, msg));
            }
            out.extend(c.recv().unwrap_or_default());
            if c.received_close_notify() {
                break;
            }
        }
        self.flush_conn();
        if out.is_empty()
            && self.eof
            && self.inbuf.len() == self.in_off
            && !self.peer_closed()
            && self.failed.is_none()
        {
            let msg = format!(
                "OpenSSL SSL_read: {}: error:0A000126:SSL routines::unexpected eof while reading, errno 0",
                BACKEND_VERSION
            );
            return Err((E_RECV_ERROR, msg));
        }
        if out.is_empty()
            && let Some(e) = &self.failed
        {
            return Err(e.clone());
        }
        Ok(out)
    }

    /// close_notify received.
    pub fn peer_closed(&self) -> bool {
        self.conn
            .as_ref()
            .is_some_and(|c| c.received_close_notify())
    }

    /// Queue a close_notify alert.
    pub fn close(&mut self) {
        if let Some(c) = self.conn.as_mut()
            && c.is_handshake_complete()
        {
            let _ = c.close();
        }
        self.flush_conn();
    }

    /// CURLINFO_SSL_VERIFYRESULT
    pub fn verify_result(&self) -> i64 {
        self.verify_result
    }

    /// CURLINFO_CERTINFO (with [`TlsConfig::certinfo`]): per certificate of
    /// the chain the server sent, its "Label:value" entries.
    pub fn certinfo(&self) -> Vec<Vec<Vec<u8>>> {
        self.certinfo.clone()
    }

    /// The ALPN protocol the server selected.
    pub fn negotiated_alpn(&self) -> Option<String> {
        self.conn
            .as_ref()
            .and_then(|c| c.alpn_selected())
            .map(|p| String::from_utf8_lossy(p).into_owned())
    }

    /// The host name / port this session connects to.
    pub fn peer(&self) -> (&str, u16) {
        (&self.host, self.port)
    }

    /// A session for libcurl's session cache once the server issued one
    /// (TLS 1.3 tickets arrive after the handshake, while reading).
    pub fn take_resumption(&mut self) -> Option<TlsResumption> {
        if !self.connected || self.failed.is_some() {
            return None;
        }
        let session = self.conn.as_mut()?.take_session()?;
        Some(TlsResumption {
            session,
            alpn: self.negotiated_alpn(),
            peer_chain: self.peer_chain(),
            verify_result: self.vresult.as_ref().map(|v| v.code).unwrap_or(0),
            verified_chain: self
                .vresult
                .as_ref()
                .map(|v| v.chain.iter().map(|c| c.der.clone()).collect())
                .unwrap_or_default(),
        })
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use purecrypto::tls::{ClientAuth, RootCertStore};

    // Test PKI (openssl CLI, fixed dates 2025-01-01 .. 2125-01-01):
    // root CA -> intermediate -> leaf "localhost" (SAN localhost,
    // *.example.test, 127.0.0.1, ::1), plus self-signed, expired, wrong-name,
    // CN-only, EC, client and "kitchen sink" certificates.
    const CA_PEM: &str = "\
-----BEGIN CERTIFICATE-----\n\
MIIDOjCCAiKgAwIBAgICEAEwDQYJKoZIhvcNAQELBQAwPTELMAkGA1UEBhMCRlIx\n\
EjAQBgNVBAoMCUdvcm8gVGVzdDEaMBgGA1UEAwwRR29ybyBUZXN0IFJvb3QgQ0Ew\n\
IBcNMjUwMTAxMDAwMDAwWhgPMjEyNTAxMDEwMDAwMDBaMD0xCzAJBgNVBAYTAkZS\n\
MRIwEAYDVQQKDAlHb3JvIFRlc3QxGjAYBgNVBAMMEUdvcm8gVGVzdCBSb290IENB\n\
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAlYzyxeKr6vERS6BPJMTZ\n\
+6DCj6pCklheAiIg4jGhoeg88Y/mIQw3vxRyPEbMbHRLyz+qYa2/g0VZl+Nu8OGP\n\
RMJxrt29mD36iKXWoOhdGT70YELrWuSTb2XM+C+vOijqoDp/Mb/+LRN9SilOZiw8\n\
Wne9BFT+M1nfIlvLGAwABkWH2/LRVy4dC0t+yVK8aymnp13APNIg9HmqT0cayQMo\n\
s3B6hEZXDg3+aRaBP9QezDZIUjxnJCbKkptjuHGIG1VLb/SqqKvgwPWXWcw3F5j8\n\
SFmAFDP1d2Mi+Oa1aFH8JfazikNstNzARZbF84VGLAG6wiFNW6rOGM9hfy16w1cn\n\
CQIDAQABo0IwQDAPBgNVHRMBAf8EBTADAQH/MA4GA1UdDwEB/wQEAwIBBjAdBgNV\n\
HQ4EFgQUedtQqFzFGZdjGrtbFeLsNYZ3BcYwDQYJKoZIhvcNAQELBQADggEBAAtU\n\
bIjl31yAF6fV9pebae6uiBNGsnC4pZh/3z/v7w2LeXBVODi3HHaCKOsdTGwENS1u\n\
DdbkMP8wQhGZ+e75DEwDv6XygDDYGpCuj2HTLXUkKi+ywm1EkJOvsdaxSS8cckxZ\n\
ejU5mF0QlWuvRJmqxM6s2YgPX5fMqvQDW4QARICvrUnBg1NZ8RR7laEyhmahLtlx\n\
rG0FZf4My46BQgfp1B+hJmmg6pxiOQOWQST3+Nfv1RhUyumDmAdTNwvoP6lWXjOE\n\
69j0wm8R83O25SISj5LXp3Xbw7ts5lqCTVs2e93ivPpNTFCCpjwCF3mpzU7RC3BT\n\
lM8Q93TQJ4kAam1Rk3Q=\n\
-----END CERTIFICATE-----\n\
";
    const INT_PEM: &str = "\
-----BEGIN CERTIFICATE-----\n\
MIIDYzCCAkugAwIBAgICIAIwDQYJKoZIhvcNAQELBQAwPTELMAkGA1UEBhMCRlIx\n\
EjAQBgNVBAoMCUdvcm8gVGVzdDEaMBgGA1UEAwwRR29ybyBUZXN0IFJvb3QgQ0Ew\n\
IBcNMjUwMTAxMDAwMDAwWhgPMjEyNTAxMDEwMDAwMDBaMEIxCzAJBgNVBAYTAkZS\n\
MRIwEAYDVQQKDAlHb3JvIFRlc3QxHzAdBgNVBAMMFkdvcm8gVGVzdCBJbnRlcm1l\n\
ZGlhdGUwggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQCoe6PwrIHaPiOK\n\
gLwqK1HO5xPMoDEFeeUjxUDJQ2FUMXaSVen0Ak5vQjRBjnsog/s8NHDNPbvmDdtV\n\
U81nDV6RI1rDZUedk5CYaLe0HcHb7Te2TLPhWSVZ2DrAIbanTbg8k72ITj7J6ne0\n\
BXw2KnEXKgRT0GYAb4bzlt1KP84fgUhneOjSmkD0QxgsvOvfFDLrzWLlPPvhykpk\n\
iuqUeVgfps9+5cWedz1bk6Z5OjH5V7O65uS97EF3GDhYh7Wm7f7j7tQPOtvtsNFW\n\
RIKNGtLvtjq0Yuos0XSPbXPuXJGQ1U1aPOlF8HgKrP4Hrneq9x9x81gkDurKzMz5\n\
g+0aDVdfAgMBAAGjZjBkMBIGA1UdEwEB/wQIMAYBAf8CAQAwDgYDVR0PAQH/BAQD\n\
AgEGMB0GA1UdDgQWBBQhUQwUCMcldzqulo2e7iMY+CNndDAfBgNVHSMEGDAWgBR5\n\
21CoXMUZl2Mau1sV4uw1hncFxjANBgkqhkiG9w0BAQsFAAOCAQEAGGz0bi0UdAx6\n\
ebs2+6vrH91kMhcFQpQDYx4yFEQ1a5tlSkV4S4KMUtN2A9xi5459FYhq1md6225w\n\
lNeTuvOYX28WTZgD5Uy90UuFhwxGnsqfMciHD8zycirtuLtJCpmr9ZTlt0qMlh9g\n\
e9ffoSjgqa7aRHiUX5j6orrO+DfBvhswGtvWeZuPQZ9WcS8sYd6utVRmYcdyj+3+\n\
Tohd+pL1t5p/blbf5qpUpp8K8p0EGHrv0iAbkaZl7QED8VYmWP9fYJ0u3Lo1JfMM\n\
g41P7fHEKThluQHn6EKaBiwZqbMLXPPn8P1ocdpSSyCmmgVVvmKrXNhB3o5cjDK4\n\
G9p/sqy8cQ==\n\
-----END CERTIFICATE-----\n\
";
    const LEAF_PEM: &str = "\
-----BEGIN CERTIFICATE-----\n\
MIIDtzCCAp+gAwIBAgICMAMwDQYJKoZIhvcNAQELBQAwQjELMAkGA1UEBhMCRlIx\n\
EjAQBgNVBAoMCUdvcm8gVGVzdDEfMB0GA1UEAwwWR29ybyBUZXN0IEludGVybWVk\n\
aWF0ZTAgFw0yNTAxMDEwMDAwMDBaGA8yMTI1MDEwMTAwMDAwMFowRTELMAkGA1UE\n\
BhMCRlIxDjAMBgNVBAgMBVBhcmlzMRIwEAYDVQQKDAlHb3JvIFRlc3QxEjAQBgNV\n\
BAMMCWxvY2FsaG9zdDCCASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBAKEd\n\
HWPEDMjzybh3J361ZQIkpB8ki8wXciRey+KTpbkFiN+PypLwRcgenFf5ibyYzKTf\n\
fGqXZhIMYcGM3n4ZzNFb37BbxNEt4QPCot7x3F3JMovRW040GXJ4bAfSpLYvTSz7\n\
Ucw5hlFU60r1GCDtjdL6wtbAwjn6M5G8V0c9qxIiKgaQ+N0qJjdsNkiofrdWZr5Q\n\
ihxNd4/BfUepqdu6UcG3kTuFsZOU8VsijnptAp71ln8FL2LMnm3NW9zQu7YFh2gz\n\
8LDA7hUAnVirD6e3GxhZQFPZfPIw/8BxMsg47Yu3RGCPKJL28Y0qwUQR+DyBrr03\n\
XwtDLQlnlnBKJRtQEFcCAwEAAaOBsTCBrjAJBgNVHRMEAjAAMA4GA1UdDwEB/wQE\n\
AwIFoDATBgNVHSUEDDAKBggrBgEFBQcDATAdBgNVHQ4EFgQU57+N4Dbz0gMxXTKM\n\
LYC0ua/zUgAwHwYDVR0jBBgwFoAUIVEMFAjHJXc6rpaNnu4jGPgjZ3QwPAYDVR0R\n\
BDUwM4IJbG9jYWxob3N0gg4qLmV4YW1wbGUudGVzdIcEfwAAAYcQAAAAAAAAAAAA\n\
AAAAAAAAATANBgkqhkiG9w0BAQsFAAOCAQEAhdGS4FpXpNBQaEBVx7r6KhxBz9Ar\n\
c82rCq/LVHWArQHFvCqEmpq2gbRbr1fAZYQgmCJkZH3geSq7IlEKSSt71DnoQgjo\n\
DtPrH5Z6iSnfJfmXk4mKXVeKPs5gpe4ja94TSWVe4Xwg4+qMXLqXuyRmBQEX3msS\n\
tyPZIsj0J5cn4Ja4Mc5qrqNjfojP+BgBUNX5uxAmmKgt9lKbrkvbTDIaUAXKjpY4\n\
7kkVpf0caYdcqXb1iByaPXTZNhZn3GoiIPKWemPtB2ec3UqMA2lG0FSrTVNmXnSb\n\
tvZ+fWxVje29R/dNxyjPU1/zca0LFvnzSi19SXmXSkZ9BjPsT9xL/EkFmw==\n\
-----END CERTIFICATE-----\n\
";
    const LEAF_KEY: &str = "\
-----BEGIN PRIVATE KEY-----\n\
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQChHR1jxAzI88m4\n\
dyd+tWUCJKQfJIvMF3IkXsvik6W5BYjfj8qS8EXIHpxX+Ym8mMyk33xql2YSDGHB\n\
jN5+GczRW9+wW8TRLeEDwqLe8dxdyTKL0VtONBlyeGwH0qS2L00s+1HMOYZRVOtK\n\
9Rgg7Y3S+sLWwMI5+jORvFdHPasSIioGkPjdKiY3bDZIqH63Vma+UIocTXePwX1H\n\
qanbulHBt5E7hbGTlPFbIo56bQKe9ZZ/BS9izJ5tzVvc0Lu2BYdoM/CwwO4VAJ1Y\n\
qw+ntxsYWUBT2XzyMP/AcTLIOO2Lt0RgjyiS9vGNKsFEEfg8ga69N18LQy0JZ5Zw\n\
SiUbUBBXAgMBAAECggEABIyYaFlATSpJD1rZ9CN2Ey3qrnBvPBeLYwiENEUcAFHu\n\
L/XU7/o11jU7D+bdY0RjPuKKlk3pFWhhGEB8hc3/sQeplMa6k3d6q4sC402r/b/x\n\
G+6Q5+/jqg5Jap0hhn1mbfc4s44KQ2Ck9ixefouGnibzenHd6hos+5f4VOwiWPQv\n\
iG3J0jNnOA/lXYVjpf653JKLlitREV6hmRSmCswCBHApgFCDJ5a2ltpx3PcOSNjc\n\
ZxaDBdOVfA/Eu+7YneDoapYoPwEHxNXq7WSPb86Vkg47+JFMlrdFj8/EXfwhdp0A\n\
MDZSfFkq5AaBYmk2828XETSsbviklSe9PbCCGlL7SQKBgQDM0VOvPNRWAybcAAWu\n\
Wh81J0OLDujBH5BsAwpJ1xEx6TtJRp0Ms+Zsffsz1utGGKuNyj0aKnar+c5e24bT\n\
+OimFCz7jDSVFGZUPKBmrn5yO04hgmmcynxyXqVnbzpfMtrgWKiCZpVEafmvBll5\n\
Yj+pA3k0UAfyYocTPaZoDYBKGQKBgQDJX/E8EgU9ov85TQcWcyj25GVxf/QASVwG\n\
3PiWiDdLo1Z0BYH/0aiLs9qu5p0GNF8G++aSXzHdwEGO/v61xDvlnhk940g/A0ZG\n\
tvw6HJCF6bqIeAgHcR1OogKNeW+ExJB4D5PTHFAS7gORzZTkThzgrayCWbC+CNjZ\n\
Uy83BlRb7wKBgQCoZkOkBSOgRJ8iq3IAVGDsVwgVwYBlqQoZhLZk18tvehAgw0pF\n\
M3T1Ju/vQ3nV/FqxDeMHLHq+E3o2S9CjYDgItPX3LOPe3xLzl5VaV4XnbhNjQdKL\n\
84A2mNLuJGmd9DY51ytVa/q2cVu1KEdQohMtZ4m0FUkzZQciLsIvM0qToQKBgHLg\n\
InOMLCUvOL1tbZ+m7jV5sU0AGyeWscLQh5cH+dNSSF1DTxeqeKXJYfToZadGdPc8\n\
+BmD6KSBqmRynsb+PNUOTYP6hho8OfjZJiXYabGa1ehH4Pyt0HQtA5vFLJLmtytm\n\
Z4PE4QELeknQpyFeVibtrD1oAOVHkrzXMVYIbFNNAoGAB311jBszgaiFdB9cYG2E\n\
+QzKEbFDaqk91X1sPAXeV2WSpb0aUZI4gXYHVOzfnPfsydyIWGSqE2Yul9TdgvIJ\n\
xb40q8Bfwq5uy1Kv6NOu2Uj7iPNnW1qQOATvDSf55EsdRe4BnaIQUC9nNV4CzGdu\n\
pm00VU2AsFkGmhmJmNyUB+Q=\n\
-----END PRIVATE KEY-----\n\
";
    const SELF_PEM: &str = "\
-----BEGIN CERTIFICATE-----\n\
MIIDNzCCAh+gAwIBAgICUAUwDQYJKoZIhvcNAQELBQAwFDESMBAGA1UEAwwJbG9j\n\
YWxob3N0MCAXDTI1MDEwMTAwMDAwMFoYDzIxMjUwMTAxMDAwMDAwWjAUMRIwEAYD\n\
VQQDDAlsb2NhbGhvc3QwggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQDE\n\
dfmibcasXP41s4E7NLfo6cKpxu5amsBcPQTTfshHI9n4Rs74Uq6caN0uUvzKgm6I\n\
V+pEsBfIgeQQSoieRFxhhSOP4Q/R82Ob7TSLiz27eFaEugXr4tAIKM72hF9DyKYs\n\
mvU5ZVubk7i8PnmIKTJ8ner1BI7+kyMSnn6toI3+Q+1jMSGpBNrt4eDoMPmc1iMt\n\
NH1ZRPjg/HfhAfhZU6E67Z9dclBoxULuPaFmhgXWmhy96FSuYP13XEgVIzrShp1c\n\
rw+Hkd6+N7Gn6s9AUJJcB8IHrDtAc1VHM/D0hub032IIRz9hwGk3MkwjAFkb3IQ2\n\
HzqJylDD5aZfqCfSEB4fAgMBAAGjgZAwgY0wCQYDVR0TBAIwADAOBgNVHQ8BAf8E\n\
BAMCBaAwEwYDVR0lBAwwCgYIKwYBBQUHAwEwHQYDVR0OBBYEFKd1myBDREhqAuTZ\n\
oRWwHNEryzkMMDwGA1UdEQQ1MDOCCWxvY2FsaG9zdIIOKi5leGFtcGxlLnRlc3SH\n\
BH8AAAGHEAAAAAAAAAAAAAAAAAAAAAEwDQYJKoZIhvcNAQELBQADggEBADvsEfsX\n\
Jac5mn9jfZjtqLkJb4tNRhrUyo9zZ512TLdgcUVOMpD82fRUMwin2pNNKWpKdHUT\n\
+rgIbOsA/1ExgRthcPFcOmrB0IVwaw2KrDdP0L6iGL+M8aHavi9XD6WqcIQzhbPR\n\
fkWeMBAIMfBqoNw079aLh8E6e3xCJo6gPAS3aS1MuKKnl98v6t2qlWQ54UxHPH7o\n\
bG0iQphCmKFwwGBg7Tv6RqQjX8A8nh6KmbBeUP7GYrG38F2CTbm5jWz4xosbA310\n\
fNKhJHvYhyDrYfczgv8xNYDXCkwFAH59isW1D/NPdSk5aik3GfvDGEvS0fU2YJNc\n\
ASmv10ynNqNTgjg=\n\
-----END CERTIFICATE-----\n\
";
    const SELF_KEY: &str = "\
-----BEGIN PRIVATE KEY-----\n\
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDEdfmibcasXP41\n\
s4E7NLfo6cKpxu5amsBcPQTTfshHI9n4Rs74Uq6caN0uUvzKgm6IV+pEsBfIgeQQ\n\
SoieRFxhhSOP4Q/R82Ob7TSLiz27eFaEugXr4tAIKM72hF9DyKYsmvU5ZVubk7i8\n\
PnmIKTJ8ner1BI7+kyMSnn6toI3+Q+1jMSGpBNrt4eDoMPmc1iMtNH1ZRPjg/Hfh\n\
AfhZU6E67Z9dclBoxULuPaFmhgXWmhy96FSuYP13XEgVIzrShp1crw+Hkd6+N7Gn\n\
6s9AUJJcB8IHrDtAc1VHM/D0hub032IIRz9hwGk3MkwjAFkb3IQ2HzqJylDD5aZf\n\
qCfSEB4fAgMBAAECggEAT02J/6mNXW1KRjYX7+THahLWO78r3tIB3RnadV8DjEfq\n\
CCDZEf8aMdPYmIbyOPKumIXYcHKenxbjjznu7OgOXGUoZj9EqgyzxDDbmYqS2DZq\n\
wjfvBPlw3SJP7M2hQNThUzcXztvdvy7BjucvGqn6kbgTLGVzLUsKiM0GuKKLPrvJ\n\
8UAOYZrj1vKB/9b42fietCQ3KUnYKYI3nQZSYHBqpqKnZY6RTIsthEXqBdikAZm/\n\
5BmXSNpqL6fwF6eX+LDksvuwCFaEYhsTjqciilCmyhdol3AB7Uw0XCqeXFj9oqkX\n\
ER2BDYrahm5WtGUxjLM7q8v72tLiOux/WXwtyK0RzQKBgQD1YZbpAXXdpfdakRPL\n\
2j/Q+6laCLyQtS8qgZPXhpDZaCbRweTd7OCAM0QkwTgeE/G8yE3KI/RbqNdp3t6m\n\
E9iU061QvwHqydEoei653fj1+VBySwEXQXpo5cA75tZ5JcNzkCUo77oWu/bB6iby\n\
JtFWQVLeHBFTmdzXaN0/T8th+wKBgQDM9m4rnPvbKBXKhp5dYFUGHEiwHWKSAtD0\n\
QUg6rrnKLd9x/5MDS1My4eGu9+kO1q1LsUAbIA2LqQT78ZLvNljI8atfg2GKOj6f\n\
G/11nNZ5qYaJpAjm/UDgB3w1TZBIOhnAguD2+HVYfiz6xd0EXnjFz+m1vA45LYy/\n\
K92lJ0+fLQKBgHER6g8ubBiZnJRwG5Yo2JU9Efl54yIvSy+kC/MZf++ebZzkYsjc\n\
mUqZ1QG2w1wgxpi+GBJYZQZDlBel6iTPKlLSA6AclA6TtryXKKbfIRR/MNGpvDPy\n\
KSzjCaTQpu2mOB0tQ+JMLQzCyl7qvc2Bk57SdRQCDP3f/YKJQMImy8o3AoGBAKYl\n\
5e0I1skzV2bTdbDwFDxd1Taeu3btPYGjIfAgD4+rXATHTt9OwmTu4sm5QX273MMN\n\
LpBOjWx3WQj7LUco4RQTg/5rPDiXOog0YmRi/SGmoN8Wz/ift2Vj6/YxqBJEtvdt\n\
oPvTMRzJO6EVoDKYeoKwJh4iEc2b9QpLfuMQWOUJAoGASPxae+g6/Vdhf5GFjGxJ\n\
re+ZvqvBbYGQI002AhMStNBt8hu6WuOZ7tS27yBgs4mFprch3D1RYWp8EdpWsvje\n\
zgjkrFpXshexw5mxm0B1dlV4jOnK7zuklWJ9JvFxK0BOYyfunrg9BYNKAfOVx+uJ\n\
jDKV4DqOSnpdtFCFH3OwrKY=\n\
-----END PRIVATE KEY-----\n\
";
    const EXPIRED_PEM: &str = "\
-----BEGIN CERTIFICATE-----\n\
MIIDfzCCAmegAwIBAgICYAYwDQYJKoZIhvcNAQELBQAwPTELMAkGA1UEBhMCRlIx\n\
EjAQBgNVBAoMCUdvcm8gVGVzdDEaMBgGA1UEAwwRR29ybyBUZXN0IFJvb3QgQ0Ew\n\
HhcNMjAwMTAxMDAwMDAwWhcNMjEwMTAxMDAwMDAwWjAUMRIwEAYDVQQDDAlsb2Nh\n\
bGhvc3QwggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQChHR1jxAzI88m4\n\
dyd+tWUCJKQfJIvMF3IkXsvik6W5BYjfj8qS8EXIHpxX+Ym8mMyk33xql2YSDGHB\n\
jN5+GczRW9+wW8TRLeEDwqLe8dxdyTKL0VtONBlyeGwH0qS2L00s+1HMOYZRVOtK\n\
9Rgg7Y3S+sLWwMI5+jORvFdHPasSIioGkPjdKiY3bDZIqH63Vma+UIocTXePwX1H\n\
qanbulHBt5E7hbGTlPFbIo56bQKe9ZZ/BS9izJ5tzVvc0Lu2BYdoM/CwwO4VAJ1Y\n\
qw+ntxsYWUBT2XzyMP/AcTLIOO2Lt0RgjyiS9vGNKsFEEfg8ga69N18LQy0JZ5Zw\n\
SiUbUBBXAgMBAAGjgbEwga4wCQYDVR0TBAIwADAOBgNVHQ8BAf8EBAMCBaAwEwYD\n\
VR0lBAwwCgYIKwYBBQUHAwEwHQYDVR0OBBYEFOe/jeA289IDMV0yjC2AtLmv81IA\n\
MB8GA1UdIwQYMBaAFHnbUKhcxRmXYxq7WxXi7DWGdwXGMDwGA1UdEQQ1MDOCCWxv\n\
Y2FsaG9zdIIOKi5leGFtcGxlLnRlc3SHBH8AAAGHEAAAAAAAAAAAAAAAAAAAAAEw\n\
DQYJKoZIhvcNAQELBQADggEBAG32VyNnG82FS80WAo5a75jRApKx09j7MkUEwiCC\n\
b6hx5n4rmk4QDVV0U9W7xe8uYr7pxw2EkUv1S/mNJq6guPQ7ot3P0/MSRY22ZNtR\n\
FAmD5CDTsCKK+N20ZMZZSlA/4TtvlnZJ9XQ+cNlWXeIRMqUCFx74j6kRpBduZ5AK\n\
SCz/ZTTvbL+Ivln1Yubz33zCIhZUg7z/RQH0tj3OJI1aYCfX84EmXfMOmIkZ14vp\n\
LHKu+o2Y6kALGUyE6HxPDdNgVYwbbaeSL5Pvk9hWXZPMSurFJPTj3xd35BXEbLVa\n\
C0RUUoeO4+lhB1RB6sGQB9IYyRctwERg08k3hucLYgB3b0s=\n\
-----END CERTIFICATE-----\n\
";
    const WRONG_PEM: &str = "\
-----BEGIN CERTIFICATE-----\n\
MIIDOjCCAiKgAwIBAgICcAcwDQYJKoZIhvcNAQELBQAwPTELMAkGA1UEBhMCRlIx\n\
EjAQBgNVBAoMCUdvcm8gVGVzdDEaMBgGA1UEAwwRR29ybyBUZXN0IFJvb3QgQ0Ew\n\
IBcNMjUwMTAxMDAwMDAwWhgPMjEyNTAxMDEwMDAwMDBaMBgxFjAUBgNVBAMMDW90\n\
aGVyLmV4YW1wbGUwggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQChHR1j\n\
xAzI88m4dyd+tWUCJKQfJIvMF3IkXsvik6W5BYjfj8qS8EXIHpxX+Ym8mMyk33xq\n\
l2YSDGHBjN5+GczRW9+wW8TRLeEDwqLe8dxdyTKL0VtONBlyeGwH0qS2L00s+1HM\n\
OYZRVOtK9Rgg7Y3S+sLWwMI5+jORvFdHPasSIioGkPjdKiY3bDZIqH63Vma+UIoc\n\
TXePwX1HqanbulHBt5E7hbGTlPFbIo56bQKe9ZZ/BS9izJ5tzVvc0Lu2BYdoM/Cw\n\
wO4VAJ1Yqw+ntxsYWUBT2XzyMP/AcTLIOO2Lt0RgjyiS9vGNKsFEEfg8ga69N18L\n\
Qy0JZ5ZwSiUbUBBXAgMBAAGjZzBlMAkGA1UdEwQCMAAwGAYDVR0RBBEwD4INb3Ro\n\
ZXIuZXhhbXBsZTAdBgNVHQ4EFgQU57+N4Dbz0gMxXTKMLYC0ua/zUgAwHwYDVR0j\n\
BBgwFoAUedtQqFzFGZdjGrtbFeLsNYZ3BcYwDQYJKoZIhvcNAQELBQADggEBAEYO\n\
anRP+FBSeMlyBqAu/QW93kOXZJYyl8uj7TMNA3Pf1NYb904mrlTZMRGDT0etsAWP\n\
sp82MA5uRs0z/X90DFNYLOnR/WRzJ9/pMJaYeaBm9RHjWDkUFx3QNYEPGxgAUZ1l\n\
RCHWzvtLMquC7Xsrn1CBv9SV5WuxU7nxxN1TS5B9+N5U+Yb1SltF5rqGN6ZtICT4\n\
Szr0HKyVlL/4d0YKkuF7yYznubk8TVnK+TA0pUTNG1L1trfm6qxgVallhKuMzJM9\n\
NidpNjZhtC/7f04dzmKtd3yV8m/2Cerf0c7zY55H8KInTxDMyq9xuUIP2xlCP8bJ\n\
5StBt4m8c9Qiw5OjuKk=\n\
-----END CERTIFICATE-----\n\
";
    const CNONLY_PEM: &str = "\
-----BEGIN CERTIFICATE-----\n\
MIIDMTCCAhmgAwIBAgIDAIAIMA0GCSqGSIb3DQEBCwUAMD0xCzAJBgNVBAYTAkZS\n\
MRIwEAYDVQQKDAlHb3JvIFRlc3QxGjAYBgNVBAMMEUdvcm8gVGVzdCBSb290IENB\n\
MCAXDTI1MDEwMTAwMDAwMFoYDzIxMjUwMTAxMDAwMDAwWjAoMRIwEAYDVQQKDAlH\n\
b3JvIFRlc3QxEjAQBgNVBAMMCWxvY2FsaG9zdDCCASIwDQYJKoZIhvcNAQEBBQAD\n\
ggEPADCCAQoCggEBAKEdHWPEDMjzybh3J361ZQIkpB8ki8wXciRey+KTpbkFiN+P\n\
ypLwRcgenFf5ibyYzKTffGqXZhIMYcGM3n4ZzNFb37BbxNEt4QPCot7x3F3JMovR\n\
W040GXJ4bAfSpLYvTSz7Ucw5hlFU60r1GCDtjdL6wtbAwjn6M5G8V0c9qxIiKgaQ\n\
+N0qJjdsNkiofrdWZr5QihxNd4/BfUepqdu6UcG3kTuFsZOU8VsijnptAp71ln8F\n\
L2LMnm3NW9zQu7YFh2gz8LDA7hUAnVirD6e3GxhZQFPZfPIw/8BxMsg47Yu3RGCP\n\
KJL28Y0qwUQR+DyBrr03XwtDLQlnlnBKJRtQEFcCAwEAAaNNMEswCQYDVR0TBAIw\n\
ADAdBgNVHQ4EFgQU57+N4Dbz0gMxXTKMLYC0ua/zUgAwHwYDVR0jBBgwFoAUedtQ\n\
qFzFGZdjGrtbFeLsNYZ3BcYwDQYJKoZIhvcNAQELBQADggEBAIXlMbm/9JJVwp7Q\n\
PRuV0SeXNnZYBpTJRsJ+SfZmUeukJZPXV2OpuRzoftem6OoaCuNtssA4Mv6X+79D\n\
G4o7x4+cRwcKIYeUvQxhPQjalbzHylTdrTwYMLbWejEplsWqPSu+/E88SDo3jmqi\n\
nfxU4nneNf6LeV3lI9r0SRWg/DCBA5i6KfcMIYCgKW9QEgz3uSHUlkisPfxCzW3X\n\
ybe2Pn02+JvfTEmrN33TFb7EiqgiXrIZm2yoP+nfm2L1hXMvAOB37MORouSiM65b\n\
gjDvaFysAN7RW297y6FxvRZSGsqMzKA8Ox8HJHSR6rfwZNtkkqcDNydv7eKx3SYl\n\
I9m0tuw=\n\
-----END CERTIFICATE-----\n\
";
    const EC_PEM: &str = "\
-----BEGIN CERTIFICATE-----\n\
MIICtjCCAZ6gAwIBAgICQAQwDQYJKoZIhvcNAQELBQAwPTELMAkGA1UEBhMCRlIx\n\
EjAQBgNVBAoMCUdvcm8gVGVzdDEaMBgGA1UEAwwRR29ybyBUZXN0IFJvb3QgQ0Ew\n\
IBcNMjUwMTAxMDAwMDAwWhgPMjEyNTAxMDEwMDAwMDBaMBQxEjAQBgNVBAMMCWxv\n\
Y2FsaG9zdDBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABMeb59F5aHzQ+mvxibdl\n\
i1igUK0tJ8H9O72TW15M93h0JVCpk9Xsnj4D8Iu2TZM2M0kpWdwOSf0Oei1Y2VtF\n\
fbyjgbEwga4wCQYDVR0TBAIwADAOBgNVHQ8BAf8EBAMCBaAwEwYDVR0lBAwwCgYI\n\
KwYBBQUHAwEwHQYDVR0OBBYEFPxndvu67eqqFSWMT3UdiqNtg56PMB8GA1UdIwQY\n\
MBaAFHnbUKhcxRmXYxq7WxXi7DWGdwXGMDwGA1UdEQQ1MDOCCWxvY2FsaG9zdIIO\n\
Ki5leGFtcGxlLnRlc3SHBH8AAAGHEAAAAAAAAAAAAAAAAAAAAAEwDQYJKoZIhvcN\n\
AQELBQADggEBAIGjguQheZeoQbhgH4yBKKjzg4wdeTM/xjiDh7poDKCx3NZsV7Qg\n\
kTkYbTukbJnI0rPfpQroElCvJ0EYwZxH46sVHGO9M1A7VdCJ0gO8TJ1gJTFmxe2I\n\
uH5/xkY6RtuxJaZ5DqfN4nItfRsPN/aSAw8ooHlA2hbwkq8I+UYP7yDWYBx3NW5g\n\
S5KRLHR3t3qKmy9yh9R0eBHaJ5O5jWI91JtRE2pioGWY6W/f7yAuzf+hDhJ3i6FV\n\
UEAy8qo1y1zil0OLr2ocSs6ukJmPAw57BqgoA6UpphrlCv3+1v3eq8Okm1jTYMvm\n\
eLG/ym/dQ2XFknRqn0QWvSGWtNBtS5+zCvE=\n\
-----END CERTIFICATE-----\n\
";
    const EC_KEY: &str = "\
-----BEGIN PRIVATE KEY-----\n\
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgGz6xGHeAcxPgwKlk\n\
lrkF3npqxfMg3w9FqNVKUq7XhjGhRANCAATHm+fReWh80Ppr8Ym3ZYtYoFCtLSfB\n\
/Tu9k1teTPd4dCVQqZPV7J4+A/CLtk2TNjNJKVncDkn9DnotWNlbRX28\n\
-----END PRIVATE KEY-----\n\
";
    const CLIENT_PEM: &str = "\
-----BEGIN CERTIFICATE-----\n\
MIIDNDCCAhygAwIBAgIDAJAJMA0GCSqGSIb3DQEBCwUAMD0xCzAJBgNVBAYTAkZS\n\
MRIwEAYDVQQKDAlHb3JvIFRlc3QxGjAYBgNVBAMMEUdvcm8gVGVzdCBSb290IENB\n\
MCAXDTI1MDEwMTAwMDAwMFoYDzIxMjUwMTAxMDAwMDAwWjAWMRQwEgYDVQQDDAtH\n\
b3JvIENsaWVudDCCASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBAKXDEvDB\n\
PL0vYxFTwpuTzbKiT9Jdmv+ckxy3uj3KsxB4Xw8H0pntsNfbq8aeUD/pZBq1w6mP\n\
XkVC5EAcCs5hAJws/QneGsmIlbEcVr7tVGawVEHX/rzAO2DplEtta7K9YYSPyg9w\n\
0sJDHk1ZU2hTNOaGmAIX61zLNxFmA2fpY4eByTd3kpVt8+84Ak+DkM5NaUEo855q\n\
gI7FLSHsHnYsm0x0cyTgGPW/ztq4QPqno5fAw1HdakexsESEKRNhBnPCr/B9Z2ym\n\
r7jXIC8zUbidOJtsfx+o8KHh7+OVqTPlxWLV8lKLtuHAr4k1XEwMJd+zSdllXz4W\n\
VNxXVeuMzy7B/VECAwEAAaNiMGAwCQYDVR0TBAIwADATBgNVHSUEDDAKBggrBgEF\n\
BQcDAjAdBgNVHQ4EFgQUqeMnB7zsNS9F3jMISuI/SRLvFcwwHwYDVR0jBBgwFoAU\n\
edtQqFzFGZdjGrtbFeLsNYZ3BcYwDQYJKoZIhvcNAQELBQADggEBADXPPWlRukSP\n\
L/Z0Oog5HdQUfTA62dWIzYK6lls2ukvrZeGdBjCPdJL+Hzf27SkK42EaKUT1cBMj\n\
K99dGQvKLex8smI3UG/BpBq7BJqzt6xnect/8lO7ci+7Vl3sf0DzCuKIZU2rKAv7\n\
fN7pbk6T9c4qN/xq6Hp6mUHqu3Tr7oPFZsOJ3kFCn4/hRH6OVbkJ/OCdUGaxHs+h\n\
YEwSSEn4XqB+ZxL0zagJ4c7tBlgYqX+2a24xJGS6bEGMxF9fb00xPstY5ZOkvXpl\n\
b+oQccjEsFurzQfaIme1hasX9PDWGC2+cH7Pgf5YJj77OIOrTMWRXRndlb//ktuz\n\
GeCRw/6E844=\n\
-----END CERTIFICATE-----\n\
";
    const CLIENT_KEY: &str = "\
-----BEGIN PRIVATE KEY-----\n\
MIIEvAIBADANBgkqhkiG9w0BAQEFAASCBKYwggSiAgEAAoIBAQClwxLwwTy9L2MR\n\
U8Kbk82yok/SXZr/nJMct7o9yrMQeF8PB9KZ7bDX26vGnlA/6WQatcOpj15FQuRA\n\
HArOYQCcLP0J3hrJiJWxHFa+7VRmsFRB1/68wDtg6ZRLbWuyvWGEj8oPcNLCQx5N\n\
WVNoUzTmhpgCF+tcyzcRZgNn6WOHgck3d5KVbfPvOAJPg5DOTWlBKPOeaoCOxS0h\n\
7B52LJtMdHMk4Bj1v87auED6p6OXwMNR3WpHsbBEhCkTYQZzwq/wfWdspq+41yAv\n\
M1G4nTibbH8fqPCh4e/jlakz5cVi1fJSi7bhwK+JNVxMDCXfs0nZZV8+FlTcV1Xr\n\
jM8uwf1RAgMBAAECggEAFtpAg7mYUUVgWpSvc+RvdjeuBS9g0umirHxgluvu6OU3\n\
cb/fLIKE9WrJu/hlJufbJFribpTJ5zGUJzSBLDevrlhTdadNuR11SbVsTiaJEskv\n\
geOwhmmC0NcTBJ0WvSMid8nyY6jnHYoZpN9Hl6PqW00QqtqNA74udDMaDXAzMmHy\n\
LhNg2/khT0A0+DldNEF7Mw6Pt0dpKdNTEmEDm0kMoVU1utUEychO9cVe5BBcz8Vd\n\
uO3Q38XAZIh2LEt5RdR3oAC7yQ6vpJPYaYoXDRaCTuHq4dniQfq8IfneWoMLRKD0\n\
Hhum42qcV4dL6riBkoPepfBxDWMTzsPGYpkKl+Qo2wKBgQDZ0xjR7giHTljrpeed\n\
e/UeVDldCL7bzz07i2vi6hDlH58INieT/25AtUe/ni9sbFKmfTjAoPR0bOKQ8bDE\n\
mot4DDoNqxlSuli50tx14+HndcXMvHLErH6fDWw4VXLAW7LfbqFNBnzglOH660yM\n\
kCLZ1oL8E1XG+Oe1ptcZO2+w6wKBgQDC0CP7FQ63EIK23Qju+Ugw4YsGKaY1tzrG\n\
ZF8StvHGfnVO0wIQzKy4u/xyHWMFSzTCt15gg8WVMuctEyPDMpLQKHFaCS923WLY\n\
d/gmkMuPVBpG268PKUMzdnId9ViJ3tN0KINxsFgRtGkLBpGuM5wift7FNOHtBf3M\n\
69aiQ+GbswKBgCjhoarkzdz3K2EuxgWcmiiTDiET3x5KCuf+BsWTwJAAG0zjPRJd\n\
wkeKvmiMO8JRokF1Bw3AJ6EiPnONqOt9RefGA69FCX+pUbWuiYnmtyNFy1XdmHyp\n\
EyquzjzUbLEFhNDCZquaKB3OU4tQVhGbi67uv66nwlWRqYtCQOSPbyr5AoGABUkE\n\
UOGHXqwR/8gLYABfVwQnc2lZ2wc3jv5WHfncoVf48sSCv9Xtsa1X8fieIRyjN80l\n\
wFSS3bNd1jk4tSkAFqmiOaEaKo1jCyBmGRXAkAjmFZoMK8sgyZqjCfEID+V9sx8O\n\
Qd1Y5lIQr33kIefAiqcKgCspHfcVpaOYVRqmWkUCgYAxC+LjaFsWEeRCOCt3cu0a\n\
mK7IgB3C67Aex+XPfq/vllhkQ+IsMvQQFcMOUL8iVvy4sIQh+x5KtEFkOSuJhu/m\n\
HrzxxceEJDFnf4bClndGNta3iFq/8e9pq+v/xWD/GdtkV22C1PelFCabQ1V6TFfN\n\
jHtRSh+GNVeFU/fmlMj0Kw==\n\
-----END PRIVATE KEY-----\n\
";
    const CLIENT_ENC_KEY: &str = "\
-----BEGIN ENCRYPTED PRIVATE KEY-----\n\
MIIFNTBfBgkqhkiG9w0BBQ0wUjAxBgkqhkiG9w0BBQwwJAQQDGXm+5d49b5sQ4oD\n\
bcHxxgICCAAwDAYIKoZIhvcNAgkFADAdBglghkgBZQMEASoEEJcwRFhfKZ8BvQpN\n\
+0ZZdVEEggTQiRKN/BGmtDxEfoFxJnuMTazqnc2+3OC2ml49Jq3yx2/kQ0X+7XP2\n\
2UiNaXPgydLmlGK7zt0lV+i/T0dxkgBe+Bj62CovcZbf7m5RKedGKI2AYXQxd4bt\n\
SK4zERyv770Xc2TFAKYtJ1O77PPE9XlF+q9VWq1vQ9rH8LDerb0V27HdLT2ysjRN\n\
KfVPjOXk96rCE0H4obOfcPOE6A1nxLlVn0Nz8atqB/gT+/+gB+UjdmwNsDEJM8/j\n\
H7qEZSNVrGZ//nU8zy5KQ29XMpOlfV0tETUDFmzwMjkZSvCUlG+pFPjcjrhWrSyV\n\
JcoAe4Bgxu3pdNdNzlaUhRSjyhbSn/uQCsYMlrpFBNlQUwJDHZgsDxEAabge+dE8\n\
lsRE4cpqltIzdiE5AAkyo9sL2sMQOBopZohjgA8PZpquaNPTS0bIhL8rAKQgkH5p\n\
I6kNeR+O/oUoAhhbLqEDHZ2n8Moatu0noJHNLvDu2H8EHrGhByNcXzDccY2UBex5\n\
f3c5+2OYzy/g9Nn/4YGR6Vh2lsji2bBs55eCAS0qqGAFzxnHi0G9f14lSLswogl2\n\
AGKZmY10zAUubWRCGJ6zI4/JuQ4W3KETD5OVng+9MVVaCutj39GjjhhI5viYa0cd\n\
HMyYiIhPww3HxQffQ8Io3LhAnOK8NvCi0Xjy+DnIwB+sr92273VsdzdKUmiuy0kM\n\
4w8ig8BVniqWlF7wyW5MWMGsZVjeIWKn61lTqqciG+dOyi35quwPEAEJ36ecXRiB\n\
SXrZAeWkQypDV+ROc25f+bB3vicoMbbJyM7nXOu1tU6aeJx+Zhqnb/PcjE70LlpX\n\
b5+RyBhSLn++Cu8mehooIcgLxT4KoOXjm4IsevFc5UukeMt0cawL1Ghu0PavbmK7\n\
ma9RtPRsQc5W/cqkTVEqTbSeIzCMgS+BsHUJEKVHvHi7uCwm1S2lxQWm6VZkTd58\n\
cZr9ZesTBgVJeZ1UOXjI4qbc298zc/msHH3EjkWNO8w73a5JZLclXZAmpYpZMJ/g\n\
aWBqgk0pbvzYZkdWW7CC2H8J1CyvzNIZ8AJLbh1c8IMV52OtA03Yn8uL57vHu4+X\n\
s+CuPk4dJcQ8PUtmvMtHqSmpoMuOcV3RKyRk4yKf0KR8wzVIPhhiK7En8Clmmm63\n\
fEQsvFlVxprVkcO2lxEuBZTL+RyoYgSxGhxPPFwWeu7an9D1qRbC9vCNJSgqIZK2\n\
WEKmghYn7XK26r0mIVp92e2KanjCZKKuAxlBJGX+LFX0CAxJZrOxqhDjvqHz3JyH\n\
/8Oz5kVlwu+kbbTM7txf1upjcBg0mudICCtoSWwiquZdZ2bVYoNKbPkvleSRQpkO\n\
8QwbXeWeCncOiGZ72XW87lftmm7liRmkZELgIs5x54ivKv0xi6niQVAFsW4ZR2xn\n\
i7fg807o9ZJmIXVXSXHW2ZmAeHzNIeC6pSl/cpSeZ7dTnfFM0xwgWmynNLN1OVjx\n\
CTvPqZ2SCspjhrJLTBf6UVbx1TN6K2PxagoYdUzCA/t4r+inP/W2tTmDZRJfgHma\n\
TQImXfwZWgVZWOlhIJpoP5nFpAi53VFf93/3SNd8gBUr+QOXrvJaYK1sAVszFuZA\n\
uBXD3pkO0rCBqFHLSRVyEkA5QikVG40On010kdjEBdQ8tuPROmxXyTY=\n\
-----END ENCRYPTED PRIVATE KEY-----\n\
";
    const CLIENT_TRAD_ENC_KEY: &str = "\
-----BEGIN RSA PRIVATE KEY-----\n\
Proc-Type: 4,ENCRYPTED\n\
DEK-Info: AES-128-CBC,914307438DB7D2C75F4E354E6E84D98B\n\
\n\
ubc0KUPsIseS5crlzcy9zg8JhDB8HfeKARUHyPgiWPqi1Bzwcc0eXyafotZXAyww\n\
hkN3ZzQYmj0eNodi4k7GB1esSRLMV8wMkcMagbBaSZAZ813dYlpaI+Ud7vp4+m9L\n\
+CXNBc8/szemlJu1Uj2WCYCQSR5EZXo3tf4CCLbpJYdq+DcXalgKfjh0BVd9xyRJ\n\
wgkB9Gof1sl+m80lN9wSkhS6ey7GNANmYAIaCFldbsNH8Z3RRTPwyENnwcp+MPbs\n\
5DsW9fgNJ7IlaB5c5sWfG8Uammarb8aIvB5GM4wc0mSCU87oWhuDrccfI7nAFpMp\n\
tl+snYZHVH3CbaIT8y2emwdaSQIahkgQWjWdPiuElJ8aaR2xHmPxcHhzcdXvH9qj\n\
oBWyd3VS8738da5zV6nJiwGesSTcn4b9qEhRLifP1/JaaEXt1OcFd4u0s+HIkTrs\n\
H572cmRqQLyss38URnBAM7zSVIilDiE/fvi8BOuTP5nBBQ4ObmILi9NUnpqj8dwL\n\
B/vR6Js+V8xMlVufgvjdUAq58HC2ffyPJFjktJ9noeDGeke6gTxn297acvOrEMp/\n\
b66aL9VkjoUXwQE1oTfJBeLPlRNkNs5uXvo0af0gzzAE/0MZPb7kRk413ND2uyJF\n\
rBGAESw9L2hDPcgwix58btPtLbePBfMVy6A0deLjI3Y4j4FOKcTLIJKNL1rJS3R/\n\
upKZ5uB/NuHGEvZ1O+YPogVhGNXHodyPNq527deoe51sQdCtzKuyaT+bESQv1D5K\n\
iQi4W/cg2MErVOVs4QMRwUvh2pVjo5KMN6/eS1spAbnbKx6N/RDVdIMTcmYgEzjt\n\
k/cuS0secsco7CMWjA7mbETvz9tcD6/7nyX5gAx9dyXcLJiaqgsr48aG9j9wdBKx\n\
6sEVBh3KN170RIudGbpwgqM0v6Hp3KDwSKRXRBi4enBQjvpxsSwscyP8P3cFLgk/\n\
Ov8IfjiML8OPMJt7H8GsdT/mRKgZ6bcmjCqPOAbTo+0y6AfvEUCBUCUhqXV9Cf/Z\n\
ZxchOROYuTzbIgxyjR51bsBsmMN77xYIutvkSLKKSsqtK5YyzeqIUIlGmgf67SzT\n\
I5k35BMjERJSRUDTPA7lFlKBNDf9WJbphsrdunzPYa38m1BGxlrSMyLDJ8AUdvWf\n\
8mFg0IWFx5mJk4hnvLuNZAUOr9L6Vo/LQMaAP0CUHbl70KrnFZNrilzoGwsaDMsa\n\
ZfV1uRJ0ZcuXXggRzOXwEe+NhcFuehHuGTsnUqHMuJ+JzIbjLDDOyInr9EwAtaUh\n\
AzgOzxgmrM1P6dPRZ1qmSpn3IUSEHgOKJ4q30Liu+sLKzHt9PYbHKcI3PiqXpj2+\n\
GAyATIw4iDV/WCoxR2Mp0FeTlkh2JcL/XlsOeeXOizbl8rbPSmY2UCWMpKFE4wLc\n\
DDADK9pcLpWNb4mUDfD9ca096ZiFXYYMhE+57ed2l1+krU4ZyQiiO+ZsmnRbnfp5\n\
Bvd2xdKYYDeWi+JRn56QpQVJxUX/04r+5l8A9hraDGMlrOTnbdn8iMju/WU7kz7r\n\
L8up4Wdfc3zsR3KhxaCw3wGL6A8y+46dErCvnNLcI0Ps3DPR2GQMOmCQHrZS4djP\n\
-----END RSA PRIVATE KEY-----\n\
";
    const KS_PEM: &str = "\
-----BEGIN CERTIFICATE-----\n\
MIIG3TCCBcWgAwIBAgIKAP8A/wD/AP8A/zANBgkqhkiG9w0BAQwFADA9MQswCQYD\n\
VQQGEwJGUjESMBAGA1UECgwJR29ybyBUZXN0MRowGAYDVQQDDBFHb3JvIFRlc3Qg\n\
Um9vdCBDQTAgFw0yNTAxMDEwMDAwMDBaGA8yMTI1MDEwMTAwMDAwMFowgdoxCzAJ\n\
BgNVBAYTAkZSMRcwFQYDVQQIDA7DjmxlLWRlLUZyYW5jZTEVMBMGA1UEBwwMUGFy\n\
aXMsIDc1MDAwMRUwEwYDVQQKDAxHb3JvICJUZXN0IiAxDzANBgNVBAsMBlVuaXQg\n\
MjESMBAGA1UEAwwJbG9jYWxob3N0MSAwHgYJKoZIhvcNAQkBFhFhZG1pbkBleGFt\n\
cGxlLmNvbTEOMAwGA1UEBRMFMTIzNDUxFzAVBgoJkiaJk/IsZAEZFgdleGFtcGxl\n\
MRQwEgYKCZImiZPyLGQBAQwEdWlkMTCCASIwDQYJKoZIhvcNAQEBBQADggEPADCC\n\
AQoCggEBAKEdHWPEDMjzybh3J361ZQIkpB8ki8wXciRey+KTpbkFiN+PypLwRcge\n\
nFf5ibyYzKTffGqXZhIMYcGM3n4ZzNFb37BbxNEt4QPCot7x3F3JMovRW040GXJ4\n\
bAfSpLYvTSz7Ucw5hlFU60r1GCDtjdL6wtbAwjn6M5G8V0c9qxIiKgaQ+N0qJjds\n\
NkiofrdWZr5QihxNd4/BfUepqdu6UcG3kTuFsZOU8VsijnptAp71ln8FL2LMnm3N\n\
W9zQu7YFh2gz8LDA7hUAnVirD6e3GxhZQFPZfPIw/8BxMsg47Yu3RGCPKJL28Y0q\n\
wUQR+DyBrr03XwtDLQlnlnBKJRtQEFcCAwEAAaOCAz0wggM5MAwGA1UdEwEB/wQC\n\
MAAwCwYDVR0PBAQDAgD5MEsGA1UdJQREMEIGCCsGAQUFBwMBBggrBgEFBQcDAgYI\n\
KwYBBQUHAwMGCCsGAQUFBwMEBggrBgEFBQcDCAYIKwYBBQUHAwkGBCoDBAUwHQYD\n\
VR0OBBYEFOe/jeA289IDMV0yjC2AtLmv81IAMGYGA1UdIwRfMF2AFHnbUKhcxRmX\n\
Yxq7WxXi7DWGdwXGoUGkPzA9MQswCQYDVQQGEwJGUjESMBAGA1UECgwJR29ybyBU\n\
ZXN0MRowGAYDVQQDDBFHb3JvIFRlc3QgUm9vdCBDQYICEAEwgZAGA1UdEQSBiDCB\n\
hYIJbG9jYWxob3N0gQ9mb29AZXhhbXBsZS5jb22GFGh0dHA6Ly9leGFtcGxlLmNv\n\
bS94hwQKAAABhxAgAQ24AAAAAAAAAAAAAAABpDQwMjELMAkGA1UEBhMCVVMxEDAO\n\
BgNVBAoMB0RpciBPcmcxETAPBgNVBAMMCERpciBOYW1liAMqAwQwGQYDVR0SBBIw\n\
EIIOaXNzdWVyLmV4YW1wbGUwUgYDVR0fBEswSTAioCCgHoYcaHR0cDovL2NybC5l\n\
eGFtcGxlLmNvbS9hLmNybDAjoCGgH4YdaHR0cDovL2NybDIuZXhhbXBsZS5jb20v\n\
Yi5jcmwwXgYIKwYBBQUHAQEEUjBQMCQGCCsGAQUFBzABhhhodHRwOi8vb2NzcC5l\n\
eGFtcGxlLmNvbS8wKAYIKwYBBQUHMAKGHGh0dHA6Ly9jYS5leGFtcGxlLmNvbS9j\n\
YS5jcnQwfwYDVR0gBHgwdjAIBgZngQwBAgEwagYJKwYBBAGGjR8BMF0wIwYIKwYB\n\
BQUHAgEWF2h0dHA6Ly9jcHMuZXhhbXBsZS5jb20vMDYGCCsGAQUFBwICMCowEhoI\n\
T3JnIE5hbWUwBgIBAQIBAhoURXhwbGljaXQgbm90aWNlIHRleHQwIgYJYIZIAYb4\n\
QgENBBUWE0hlbGxvIHdvcmxkIGNvbW1lbnQwEQYJYIZIAYb4QgEBBAQDAgXgMBEG\n\
CCsGAQUFBwEYBAUwAwIBBTAbBgQqAwRjBBMMEVNvbWUgY3VzdG9tIHZhbHVlMA0G\n\
CSqGSIb3DQEBDAUAA4IBAQBj5VyQaLE9bO6AzFeAZQrFjqG8tSCV1VzYwI431lFT\n\
6gC4lGiSetOTuWmVNkxY3ZoN1Pd2iGbIjfeuGnPRLZGwIK2QDyqvP2JgJASk/DXG\n\
gWcGKeK4wDJVDtnZS81q0fEUuGqZe6ltsr50XIRCwDwna5F44utcUP1dqlz5h61d\n\
32V8isI+5cRJASWkSOts63pk85BjR6CkKG3c9roYk3PoDWfWX4q9Fs59mG6vLy4Q\n\
tcO+VbhqtSxbLFEGRxvpFAMeACH5rUhRk9Cd4gNqMhRblQnNBDR3Jd08QYIDibbA\n\
jqpxdw/U/FTb/21k/V3OO8t84cKI5h/9yv/meCHeLgCR\n\
-----END CERTIFICATE-----\n\
";
    const CRL_BOTH: &str = "\
-----BEGIN X509 CRL-----\n\
MIIBmDCBgQIBATANBgkqhkiG9w0BAQsFADA9MQswCQYDVQQGEwJGUjESMBAGA1UE\n\
CgwJR29ybyBUZXN0MRowGAYDVQQDDBFHb3JvIFRlc3QgUm9vdCBDQRcNMjYxMDIx\n\
MjAwMTUzWhgPMjEyNjA5MjcyMDAxNTNaoA4wDDAKBgNVHRQEAwIBATANBgkqhkiG\n\
9w0BAQsFAAOCAQEAfJ6CJnKyP8lyW2nfcUQGt3si09F3S9PaxhFxZjjRlTEj1l4q\n\
P6av2VMdnuYI8bwgykkV/QfCn7bLQGpTjDCBc3hRZihUQjRQK1tOgDswOjjyOG1V\n\
J3w9zhyN3xq9rUJ4Dy3XNyOacfyn9Wpitjfxr3zROlqg53VUq403yPh+0N8R7JrD\n\
x/jD/NyYKE+jhoDs1KP6ao3VC3CQn4d+VXVBw8iegqQwxLAdmgVCuWu4k8rcWLJp\n\
Ttz0nDcNNUJxO9E2yXj0IXqyDjS6Z8r/e2UuWrBOn0b+y4IDCboDYDyxNUBioIKX\n\
ZS1V6MX6sjzFvmG0UsBpmfWzwkzKSNpN5Gsupw==\n\
-----END X509 CRL-----\n\
-----BEGIN X509 CRL-----\n\
MIIBnTCBhgIBATANBgkqhkiG9w0BAQsFADBCMQswCQYDVQQGEwJGUjESMBAGA1UE\n\
CgwJR29ybyBUZXN0MR8wHQYDVQQDDBZHb3JvIFRlc3QgSW50ZXJtZWRpYXRlFw0y\n\
NjEwMjEyMDAxNTNaGA8yMTI2MDkyNzIwMDE1M1qgDjAMMAoGA1UdFAQDAgEBMA0G\n\
CSqGSIb3DQEBCwUAA4IBAQBLWW4ssCjxCMnC8tZMpY5FYzqw3LlMmsTplcnd4XzY\n\
4eshVtIus5A3NYaPCgyztL7T60vAUva5vBtP7Za0DkS0WH0rUHo1+in//Qv92/wL\n\
VbkCP34K2N8WCibYwzZoF6zFz8SpTKaV0CgugzsZ28t6tqL3Hb8hiRbXesCfvEcO\n\
5poE8WpFAfGYwWjevwE45Mo3sTZvnVT7k13azggW2/8Ii2XMbb3U/YxjW0ZZ6vCF\n\
wGqowbSeD7c/JN3Up8nmthAnKsmFazGygdqz4YA3FUbCGjkZnCGJAnXn34MVo2gJ\n\
tSS1ZUaDU+xB4lRImTVg6frCfbU9I2Xb5dwz5Xl4uP6l\n\
-----END X509 CRL-----\n\
";
    const CRL_BOTH_REVOKED: &str = "\
-----BEGIN X509 CRL-----\n\
MIIBmDCBgQIBATANBgkqhkiG9w0BAQsFADA9MQswCQYDVQQGEwJGUjESMBAGA1UE\n\
CgwJR29ybyBUZXN0MRowGAYDVQQDDBFHb3JvIFRlc3QgUm9vdCBDQRcNMjYxMDIx\n\
MjAwMTUzWhgPMjEyNjA5MjcyMDAxNTNaoA4wDDAKBgNVHRQEAwIBATANBgkqhkiG\n\
9w0BAQsFAAOCAQEAfJ6CJnKyP8lyW2nfcUQGt3si09F3S9PaxhFxZjjRlTEj1l4q\n\
P6av2VMdnuYI8bwgykkV/QfCn7bLQGpTjDCBc3hRZihUQjRQK1tOgDswOjjyOG1V\n\
J3w9zhyN3xq9rUJ4Dy3XNyOacfyn9Wpitjfxr3zROlqg53VUq403yPh+0N8R7JrD\n\
x/jD/NyYKE+jhoDs1KP6ao3VC3CQn4d+VXVBw8iegqQwxLAdmgVCuWu4k8rcWLJp\n\
Ttz0nDcNNUJxO9E2yXj0IXqyDjS6Z8r/e2UuWrBOn0b+y4IDCboDYDyxNUBioIKX\n\
ZS1V6MX6sjzFvmG0UsBpmfWzwkzKSNpN5Gsupw==\n\
-----END X509 CRL-----\n\
-----BEGIN X509 CRL-----\n\
MIIBtDCBnQIBATANBgkqhkiG9w0BAQsFADBCMQswCQYDVQQGEwJGUjESMBAGA1UE\n\
CgwJR29ybyBUZXN0MR8wHQYDVQQDDBZHb3JvIFRlc3QgSW50ZXJtZWRpYXRlFw0y\n\
NjEwMjEyMDAxNTNaGA8yMTI2MDkyNzIwMDE1M1owFTATAgIwAxcNMjYxMDIxMjAw\n\
MTUzWqAOMAwwCgYDVR0UBAMCAQEwDQYJKoZIhvcNAQELBQADggEBAEtIJFcmXbeK\n\
hI7ItmOTlBTu/9ejOkMmO4mxnwv4EfsS8/k6YlUlx0wQLrbrlLqzI+zFe+kUaiXu\n\
qQaRKgU6Niwn9vZQAaS+SM9UfLyL/IHaeXFkPsS/wccDlowjeB/0stq3z4hgiv8R\n\
u55F4Ds3Out7h/ibsMdHpnPoLqsWW/z/tboDcQpok9ejVagp5qQxRfblkCDpP+G0\n\
d50WfIeCRxjPivO6c3m553v0+sqUvzX1FSAQAukROpfsonmH03Dm9st0PUZ/vhLA\n\
72dNTULp/jzBMl77873/wI0XyhOd0UMoP+R9HXpaOBOOhSsZF3tkYX8++9REU96d\n\
IKRJ9+D0f+8=\n\
-----END X509 CRL-----\n\
";
    const CRL_INT: &str = "\
-----BEGIN X509 CRL-----\n\
MIIBnTCBhgIBATANBgkqhkiG9w0BAQsFADBCMQswCQYDVQQGEwJGUjESMBAGA1UE\n\
CgwJR29ybyBUZXN0MR8wHQYDVQQDDBZHb3JvIFRlc3QgSW50ZXJtZWRpYXRlFw0y\n\
NjEwMjEyMDAxNTNaGA8yMTI2MDkyNzIwMDE1M1qgDjAMMAoGA1UdFAQDAgEBMA0G\n\
CSqGSIb3DQEBCwUAA4IBAQBLWW4ssCjxCMnC8tZMpY5FYzqw3LlMmsTplcnd4XzY\n\
4eshVtIus5A3NYaPCgyztL7T60vAUva5vBtP7Za0DkS0WH0rUHo1+in//Qv92/wL\n\
VbkCP34K2N8WCibYwzZoF6zFz8SpTKaV0CgugzsZ28t6tqL3Hb8hiRbXesCfvEcO\n\
5poE8WpFAfGYwWjevwE45Mo3sTZvnVT7k13azggW2/8Ii2XMbb3U/YxjW0ZZ6vCF\n\
wGqowbSeD7c/JN3Up8nmthAnKsmFazGygdqz4YA3FUbCGjkZnCGJAnXn34MVo2gJ\n\
tSS1ZUaDU+xB4lRImTVg6frCfbU9I2Xb5dwz5Xl4uP6l\n\
-----END X509 CRL-----\n\
";
    const CLIENT_P12: &[u8] = &[
        0x30, 0x82, 0x0a, 0x1f, 0x02, 0x01, 0x03, 0x30, 0x82, 0x09, 0xd5, 0x06, 0x09, 0x2a, 0x86,
        0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x01, 0xa0, 0x82, 0x09, 0xc6, 0x04, 0x82, 0x09, 0xc2,
        0x30, 0x82, 0x09, 0xbe, 0x30, 0x82, 0x04, 0x2a, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7,
        0x0d, 0x01, 0x07, 0x06, 0xa0, 0x82, 0x04, 0x1b, 0x30, 0x82, 0x04, 0x17, 0x02, 0x01, 0x00,
        0x30, 0x82, 0x04, 0x10, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x01,
        0x30, 0x5f, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x05, 0x0d, 0x30, 0x52,
        0x30, 0x31, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x05, 0x0c, 0x30, 0x24,
        0x04, 0x10, 0x70, 0x80, 0xf8, 0x23, 0x94, 0x1b, 0x1d, 0x80, 0xe7, 0xd6, 0xb4, 0x6f, 0x60,
        0x46, 0x43, 0xe1, 0x02, 0x02, 0x08, 0x00, 0x30, 0x0c, 0x06, 0x08, 0x2a, 0x86, 0x48, 0x86,
        0xf7, 0x0d, 0x02, 0x09, 0x05, 0x00, 0x30, 0x1d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65,
        0x03, 0x04, 0x01, 0x2a, 0x04, 0x10, 0x21, 0x91, 0xbb, 0x8c, 0x7d, 0xcc, 0xee, 0x44, 0xfe,
        0xbd, 0x44, 0xba, 0x50, 0x43, 0xc8, 0x32, 0x80, 0x82, 0x03, 0xa0, 0x64, 0x92, 0xb4, 0xce,
        0x98, 0x7a, 0xa2, 0x12, 0xa2, 0x65, 0xcb, 0xfe, 0x22, 0xf2, 0x7e, 0x50, 0xbc, 0xef, 0xae,
        0x35, 0xbb, 0x26, 0xd7, 0x44, 0x19, 0x47, 0x5c, 0xd8, 0x97, 0x31, 0xa7, 0x98, 0x48, 0xee,
        0x69, 0x94, 0x95, 0xcf, 0x5c, 0xac, 0xed, 0x8c, 0xce, 0x25, 0x55, 0x3b, 0xec, 0x47, 0x52,
        0x0a, 0xf9, 0x61, 0xca, 0x97, 0xcb, 0x13, 0xb0, 0xe6, 0x60, 0x28, 0x04, 0x24, 0x83, 0x30,
        0xde, 0x10, 0x85, 0x5e, 0x16, 0x99, 0xc7, 0xfa, 0xa3, 0x0e, 0x5c, 0x4a, 0x80, 0xb4, 0x48,
        0xc9, 0xfb, 0x68, 0x24, 0x3a, 0x3b, 0x2c, 0xa6, 0xe3, 0xe0, 0xe6, 0xce, 0x18, 0x92, 0x62,
        0x8f, 0x56, 0x2c, 0x13, 0xbb, 0xff, 0x52, 0xc0, 0x00, 0x66, 0xa6, 0x72, 0xe8, 0x8d, 0x04,
        0xcb, 0x60, 0xbb, 0x27, 0x25, 0xb2, 0x9e, 0x7d, 0x75, 0x37, 0x31, 0x24, 0x40, 0xf3, 0x07,
        0x78, 0x11, 0x60, 0x65, 0x16, 0xe9, 0x35, 0x74, 0x69, 0x23, 0x62, 0x1d, 0x79, 0x8d, 0x05,
        0xea, 0xd1, 0xe7, 0x02, 0x2c, 0x1d, 0xeb, 0x8e, 0xb9, 0xae, 0x5e, 0x13, 0x3e, 0x3f, 0x9d,
        0x0a, 0xac, 0x5f, 0xb9, 0xb9, 0x27, 0x32, 0x1c, 0x87, 0x34, 0x9e, 0x7f, 0x0d, 0x58, 0xa1,
        0x2e, 0x8c, 0x71, 0xa5, 0x57, 0x67, 0x91, 0xa2, 0x1c, 0x3a, 0x6d, 0x53, 0xbb, 0x0c, 0xd5,
        0x0b, 0xad, 0xa4, 0x45, 0x9e, 0xad, 0xd7, 0xd4, 0x33, 0x6f, 0x9d, 0xba, 0xf1, 0xc4, 0x81,
        0xe1, 0xcd, 0xb6, 0xfe, 0x40, 0xaa, 0xd3, 0x72, 0x0f, 0xc8, 0x25, 0xfb, 0x3a, 0xf2, 0x1f,
        0xb6, 0xfc, 0xd8, 0x0b, 0xef, 0x6d, 0x64, 0xe2, 0x43, 0x9d, 0x50, 0xf0, 0x0e, 0x8b, 0xb8,
        0x78, 0xfe, 0xee, 0xfc, 0x1a, 0x52, 0x13, 0x78, 0x61, 0xe2, 0xd7, 0x22, 0x65, 0xd6, 0x03,
        0x05, 0x2e, 0xc6, 0x15, 0xc0, 0x72, 0x06, 0x98, 0x1b, 0x96, 0xe3, 0x5b, 0xb1, 0x39, 0x7c,
        0x05, 0xd5, 0x5c, 0x29, 0xc2, 0x70, 0x85, 0xd2, 0x7c, 0x8e, 0x49, 0x88, 0xfe, 0x3b, 0x40,
        0x7e, 0xef, 0x75, 0x3d, 0x95, 0x57, 0x92, 0xf2, 0x2c, 0xe1, 0x89, 0xe4, 0x3a, 0x83, 0x11,
        0x7a, 0xd0, 0x9a, 0xb0, 0x6d, 0xcf, 0xea, 0x5e, 0xbc, 0xbf, 0xf6, 0x80, 0x4c, 0x43, 0x75,
        0x22, 0xe9, 0x98, 0xf3, 0x81, 0x27, 0x63, 0x2d, 0x33, 0x60, 0xa2, 0x70, 0x98, 0x7f, 0x31,
        0x6b, 0xfc, 0xf8, 0x99, 0x70, 0x2a, 0x3a, 0xe5, 0x39, 0xe2, 0xb5, 0x3f, 0x38, 0x60, 0x37,
        0xd9, 0x15, 0x88, 0x23, 0x9c, 0x52, 0xba, 0xaf, 0xf2, 0xc7, 0xdf, 0x7f, 0x41, 0xad, 0x42,
        0x9a, 0xa1, 0xcf, 0xe6, 0x1d, 0x12, 0x6d, 0xa7, 0x08, 0xc8, 0xef, 0xb1, 0x02, 0xc3, 0x44,
        0xf4, 0x51, 0x29, 0xfc, 0x9e, 0xa4, 0x38, 0x8e, 0x01, 0xd6, 0xfc, 0x6e, 0x0a, 0xdc, 0x66,
        0x77, 0xc1, 0x5b, 0x08, 0x2f, 0xdc, 0xac, 0xf4, 0x77, 0x2d, 0xb7, 0x10, 0xb0, 0x17, 0xf6,
        0xf9, 0xef, 0x38, 0xb8, 0xee, 0xee, 0xa8, 0xfc, 0xb4, 0xb1, 0x3c, 0x46, 0xf1, 0xd7, 0x31,
        0xf4, 0x8e, 0xdb, 0x0c, 0x69, 0xf6, 0xad, 0x7e, 0xe9, 0x6c, 0x2a, 0x41, 0xce, 0x91, 0xc6,
        0x45, 0x16, 0x6e, 0x33, 0x4f, 0x00, 0x97, 0x2e, 0x22, 0x28, 0xee, 0x67, 0xcf, 0xf9, 0x9a,
        0x97, 0x5b, 0x96, 0xba, 0x4d, 0xe1, 0xca, 0x5a, 0x3d, 0x96, 0xfa, 0xb6, 0x7f, 0x7b, 0x13,
        0xba, 0x51, 0x0c, 0x99, 0x4f, 0xac, 0xb4, 0x74, 0x28, 0xcd, 0xeb, 0x48, 0xeb, 0xe1, 0xba,
        0xd7, 0xd1, 0xa7, 0x15, 0x76, 0x6e, 0x3b, 0x43, 0xf2, 0x68, 0x22, 0x85, 0x40, 0xa8, 0x4b,
        0xeb, 0xd6, 0x8b, 0x29, 0x60, 0x11, 0xd5, 0xca, 0xd0, 0x1c, 0xb6, 0xcd, 0x77, 0x2c, 0x83,
        0xb1, 0xe3, 0xa1, 0xb5, 0x09, 0x3d, 0xc7, 0xb9, 0xf1, 0xdc, 0xb2, 0x00, 0x75, 0x49, 0x2c,
        0x24, 0xf7, 0x0c, 0xdf, 0x7c, 0x23, 0x96, 0xed, 0x30, 0xbe, 0x4e, 0x49, 0x7d, 0x35, 0x13,
        0xc8, 0xa7, 0xf9, 0x57, 0xa3, 0xf2, 0x54, 0x5a, 0x42, 0x5b, 0x93, 0x14, 0x05, 0xae, 0xe6,
        0x06, 0xbf, 0xef, 0x2b, 0x94, 0x99, 0x39, 0xb0, 0x8c, 0x55, 0x48, 0x69, 0x59, 0x23, 0x04,
        0x69, 0x52, 0xd9, 0xa2, 0x1d, 0xef, 0x77, 0xe8, 0x51, 0x22, 0x21, 0x1e, 0x87, 0x30, 0x91,
        0x26, 0xfa, 0x16, 0x42, 0x3f, 0x65, 0x90, 0x34, 0xaa, 0x35, 0x60, 0x04, 0xac, 0x76, 0x03,
        0x3d, 0xee, 0x4d, 0x89, 0x43, 0x8c, 0x15, 0xe2, 0xc5, 0x9f, 0x19, 0xc6, 0xaf, 0x75, 0xd6,
        0xb1, 0x37, 0x8f, 0x51, 0xb1, 0xb8, 0x8e, 0xc3, 0xdb, 0xd1, 0x4e, 0xfc, 0x82, 0xb9, 0xf1,
        0xd8, 0xd8, 0x43, 0xed, 0x22, 0x81, 0x30, 0x43, 0x9f, 0x9c, 0xc5, 0x12, 0x33, 0x6d, 0xf4,
        0x19, 0x43, 0x50, 0xfd, 0x58, 0xd7, 0x75, 0xf2, 0xfd, 0x90, 0xa5, 0x95, 0x41, 0x1b, 0x8b,
        0x55, 0x8f, 0x8c, 0xc5, 0x7e, 0x1c, 0x7c, 0x66, 0x54, 0xf7, 0x6a, 0x5d, 0xcb, 0xee, 0x45,
        0x58, 0xbe, 0x82, 0x7e, 0xda, 0xca, 0xe1, 0xf3, 0xdb, 0xd5, 0x27, 0x3a, 0x33, 0xce, 0xce,
        0xe3, 0x12, 0x64, 0x07, 0x8a, 0x7a, 0x3b, 0xc9, 0x92, 0x85, 0x2b, 0x74, 0xb0, 0xb7, 0x2c,
        0xc6, 0xe6, 0xa2, 0x39, 0xd9, 0x25, 0x3b, 0xfe, 0xf9, 0x4c, 0x30, 0xbb, 0x7b, 0xa3, 0x81,
        0x50, 0x2a, 0xd0, 0x56, 0x11, 0x1d, 0x91, 0xc3, 0x5f, 0xac, 0xe6, 0xd8, 0xdf, 0x56, 0x95,
        0xae, 0xfd, 0x58, 0xf0, 0xde, 0xd5, 0x09, 0x8f, 0x3d, 0x11, 0x5c, 0x3c, 0x9f, 0x04, 0xe3,
        0x7d, 0xad, 0x22, 0x02, 0x40, 0x3a, 0x65, 0x75, 0x46, 0x6d, 0x59, 0x69, 0xca, 0xa3, 0x6d,
        0x75, 0x33, 0xae, 0xd5, 0xb7, 0x6b, 0x9e, 0x92, 0xe7, 0x36, 0x89, 0x89, 0xc6, 0xdc, 0xec,
        0x8a, 0xb5, 0xcc, 0xa5, 0x18, 0xe9, 0x94, 0x4d, 0x9a, 0xf6, 0xda, 0x4a, 0x97, 0x38, 0x70,
        0x9e, 0x5e, 0xcd, 0x9d, 0xda, 0xb7, 0x70, 0xec, 0x1a, 0x24, 0xdd, 0x37, 0xa7, 0x1c, 0xe4,
        0xad, 0xba, 0x93, 0x21, 0x99, 0x65, 0xb3, 0xa0, 0x5c, 0x0c, 0xc8, 0xab, 0x37, 0x4a, 0xc9,
        0xaa, 0x0f, 0xb1, 0xbc, 0x68, 0x50, 0x66, 0x35, 0x46, 0x2b, 0xdc, 0xbf, 0x7e, 0xc1, 0x56,
        0x67, 0x34, 0x2e, 0xe6, 0x6e, 0x44, 0xda, 0x09, 0xee, 0xf1, 0xb4, 0x40, 0x45, 0x50, 0xfe,
        0xdf, 0xcf, 0xdf, 0x38, 0xa4, 0xfd, 0x79, 0xc1, 0x1e, 0x41, 0x64, 0x30, 0x59, 0x02, 0xb2,
        0x32, 0x7f, 0x04, 0xfb, 0xfc, 0xb6, 0xcb, 0xdb, 0x39, 0xa8, 0xdf, 0x05, 0xc4, 0x5e, 0x22,
        0xe2, 0x42, 0xc1, 0x51, 0x81, 0x69, 0xe4, 0xe2, 0x2e, 0xcb, 0x65, 0x40, 0xfc, 0xeb, 0x21,
        0x90, 0x19, 0xdc, 0x2c, 0x8a, 0x3b, 0x0b, 0xff, 0x5e, 0x6c, 0xbf, 0x58, 0x60, 0x28, 0xd4,
        0x09, 0xca, 0x54, 0xc3, 0x73, 0x9e, 0x11, 0x96, 0x4f, 0xfb, 0x8d, 0x41, 0x2a, 0xeb, 0xda,
        0x3d, 0xdc, 0xef, 0xc6, 0xef, 0x27, 0x62, 0x2d, 0x7b, 0x30, 0x82, 0x05, 0x8c, 0x06, 0x09,
        0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x01, 0xa0, 0x82, 0x05, 0x7d, 0x04, 0x82,
        0x05, 0x79, 0x30, 0x82, 0x05, 0x75, 0x30, 0x82, 0x05, 0x71, 0x06, 0x0b, 0x2a, 0x86, 0x48,
        0x86, 0xf7, 0x0d, 0x01, 0x0c, 0x0a, 0x01, 0x02, 0xa0, 0x82, 0x05, 0x39, 0x30, 0x82, 0x05,
        0x35, 0x30, 0x5f, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x05, 0x0d, 0x30,
        0x52, 0x30, 0x31, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x05, 0x0c, 0x30,
        0x24, 0x04, 0x10, 0x98, 0x5e, 0xf6, 0x67, 0x9f, 0xc5, 0xee, 0x97, 0xa5, 0xe6, 0x60, 0x63,
        0x80, 0xf5, 0xc2, 0x95, 0x02, 0x02, 0x08, 0x00, 0x30, 0x0c, 0x06, 0x08, 0x2a, 0x86, 0x48,
        0x86, 0xf7, 0x0d, 0x02, 0x09, 0x05, 0x00, 0x30, 0x1d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01,
        0x65, 0x03, 0x04, 0x01, 0x2a, 0x04, 0x10, 0xd1, 0xdd, 0x6c, 0x77, 0xb7, 0x08, 0x75, 0x87,
        0xdf, 0x6d, 0xd4, 0x0b, 0x5e, 0x68, 0x46, 0x9f, 0x04, 0x82, 0x04, 0xd0, 0xea, 0xce, 0xe4,
        0x9e, 0x3b, 0x26, 0x96, 0x13, 0x3c, 0xed, 0xbc, 0x54, 0x29, 0x36, 0xf7, 0x7a, 0x51, 0x84,
        0xda, 0x6b, 0x61, 0x0f, 0x71, 0x82, 0x17, 0xba, 0xec, 0x21, 0x67, 0x63, 0x5a, 0xed, 0x3f,
        0xbd, 0x5d, 0x32, 0x1f, 0xe7, 0xcd, 0xa4, 0xcd, 0xe8, 0xdb, 0xc6, 0x47, 0x28, 0xca, 0xaf,
        0x05, 0x9e, 0xfe, 0xdb, 0x93, 0x6d, 0xdd, 0xce, 0x8b, 0xc8, 0xb3, 0xeb, 0x34, 0x0e, 0x34,
        0x5e, 0xba, 0x61, 0xc4, 0x05, 0xde, 0x20, 0x33, 0xe5, 0x56, 0x1b, 0x05, 0xc8, 0x1c, 0xc2,
        0x58, 0xaa, 0x23, 0xd3, 0xf0, 0xac, 0x34, 0x56, 0x06, 0x1e, 0xc6, 0xcd, 0xa9, 0x3b, 0x7e,
        0x4e, 0xab, 0x6a, 0x32, 0xef, 0xaa, 0xd4, 0x86, 0x2c, 0x4d, 0xf1, 0x11, 0x09, 0xfb, 0x71,
        0x26, 0xaa, 0x93, 0xd8, 0x35, 0xe1, 0x0d, 0xcc, 0xea, 0xed, 0x48, 0x68, 0x20, 0x2e, 0x89,
        0xc0, 0xbc, 0xc7, 0x5e, 0x81, 0x53, 0xf9, 0x1c, 0x20, 0x74, 0xb9, 0x89, 0xd5, 0xbe, 0xf7,
        0x5e, 0x22, 0x73, 0xe1, 0xd0, 0x2e, 0x6d, 0xa1, 0x80, 0x45, 0xa3, 0x49, 0x54, 0x12, 0x6a,
        0x4c, 0x9f, 0x94, 0xd6, 0xf7, 0x59, 0x70, 0xb6, 0x82, 0x0c, 0x9a, 0x99, 0x9a, 0x82, 0xd9,
        0x34, 0x1b, 0xbf, 0x47, 0x77, 0x38, 0x14, 0x95, 0x66, 0x5e, 0x41, 0x60, 0x6a, 0xc2, 0x9e,
        0x0e, 0xf4, 0xa1, 0xf7, 0x9b, 0x4e, 0x35, 0xaf, 0x16, 0x01, 0x3b, 0xf5, 0xa3, 0x27, 0x95,
        0x7e, 0x43, 0xa3, 0x09, 0x0a, 0x2d, 0x25, 0x9c, 0x83, 0xc2, 0x47, 0xe4, 0x97, 0x49, 0x4a,
        0x24, 0x17, 0x98, 0x48, 0x40, 0x50, 0x5e, 0x07, 0xc8, 0x52, 0x67, 0xb0, 0xac, 0x89, 0x9c,
        0x30, 0xa6, 0x40, 0x43, 0xb2, 0x5c, 0xb4, 0x6a, 0x0a, 0x3c, 0x28, 0xea, 0xd8, 0xda, 0x06,
        0x49, 0x7d, 0x2c, 0xfd, 0x08, 0xe0, 0xd5, 0xd0, 0x79, 0x96, 0x2c, 0xa9, 0x7c, 0xc2, 0xb1,
        0xf6, 0x0c, 0x58, 0xe2, 0x67, 0xdc, 0x0a, 0x2c, 0x32, 0x50, 0x6a, 0x79, 0xcf, 0x13, 0x90,
        0x81, 0x6d, 0xc3, 0x64, 0xc8, 0x62, 0xb6, 0xe5, 0x64, 0x65, 0xf2, 0x8d, 0x57, 0xc5, 0x14,
        0x8f, 0xc6, 0xd2, 0xe0, 0x93, 0x7c, 0x31, 0xda, 0x22, 0xe2, 0x33, 0x54, 0x14, 0xa6, 0xa2,
        0x43, 0xed, 0x04, 0xfb, 0xa6, 0x4d, 0xde, 0x68, 0xc0, 0xf9, 0x23, 0x5b, 0xa1, 0x9f, 0x6e,
        0x77, 0x4c, 0x61, 0xe0, 0xf6, 0x19, 0xb8, 0xa2, 0xfa, 0xeb, 0x01, 0xba, 0x63, 0x59, 0xad,
        0xb0, 0xb7, 0x64, 0x71, 0x5e, 0x37, 0x01, 0x88, 0x14, 0xfa, 0xfc, 0xac, 0x1e, 0x2d, 0x00,
        0x90, 0x90, 0x3f, 0x39, 0xb4, 0xb3, 0x06, 0x4d, 0x40, 0x5f, 0x02, 0xbc, 0xaf, 0x44, 0xe5,
        0xcf, 0x7f, 0x0e, 0x11, 0x27, 0xc4, 0xbb, 0xb9, 0x3c, 0x11, 0xa3, 0x0c, 0xe5, 0xea, 0x8f,
        0xe4, 0xc5, 0x32, 0x73, 0x7f, 0x23, 0xb1, 0x5f, 0xda, 0x2b, 0xb9, 0x8d, 0x99, 0xd1, 0xbb,
        0xd3, 0xfd, 0x21, 0x32, 0xd1, 0x17, 0x1b, 0x62, 0x22, 0x3f, 0x54, 0x10, 0x6c, 0xa0, 0xdf,
        0xa5, 0x17, 0x9b, 0x4c, 0x5b, 0x0e, 0x62, 0xdd, 0xe0, 0x96, 0xa3, 0x41, 0x67, 0xd1, 0x71,
        0x7e, 0x0c, 0xf5, 0xe0, 0x91, 0x8d, 0x99, 0xfb, 0xcd, 0xe3, 0x40, 0x52, 0x52, 0x63, 0xdc,
        0xdd, 0x5e, 0xf8, 0xd0, 0xab, 0x28, 0xdf, 0x5c, 0x3a, 0xa1, 0x6d, 0x73, 0xdb, 0x53, 0x06,
        0x33, 0xf1, 0x4a, 0x01, 0xf7, 0x72, 0xd5, 0x2c, 0x20, 0x35, 0x60, 0x6d, 0xe1, 0x37, 0x9f,
        0x0a, 0xd5, 0x00, 0xb8, 0x03, 0x88, 0x0f, 0xf1, 0xfc, 0x99, 0xc4, 0x35, 0xaa, 0x02, 0x57,
        0x20, 0x7e, 0x59, 0x4d, 0xc2, 0x2d, 0x11, 0x29, 0x8f, 0xce, 0xea, 0x2f, 0x88, 0xf3, 0xd0,
        0xf1, 0xaf, 0x7e, 0x6a, 0x36, 0x1e, 0xfa, 0x6c, 0xa9, 0x81, 0xf4, 0x9a, 0xca, 0xd1, 0x87,
        0xa3, 0x8f, 0x18, 0x96, 0x39, 0xbf, 0xe0, 0xfd, 0x7c, 0x8d, 0xac, 0xa4, 0xdc, 0xa9, 0xbb,
        0xbd, 0x40, 0xf6, 0xb9, 0x6f, 0x5a, 0x3c, 0xd2, 0x26, 0xb9, 0x0e, 0x56, 0x6b, 0xa5, 0xc8,
        0xa1, 0xba, 0x8c, 0x55, 0xc6, 0x2d, 0x04, 0xa0, 0xd6, 0x3d, 0x5c, 0xaf, 0x18, 0xdc, 0x07,
        0x68, 0x31, 0xab, 0x19, 0x9c, 0xfd, 0xfd, 0x5e, 0xee, 0x50, 0x75, 0x37, 0x9a, 0x43, 0x19,
        0x2b, 0xe2, 0x2c, 0xa2, 0x55, 0x9e, 0x59, 0x21, 0x21, 0x4c, 0xb7, 0x88, 0x08, 0x35, 0x6c,
        0x0f, 0x96, 0x40, 0x75, 0x97, 0x88, 0x5f, 0xe9, 0xae, 0xeb, 0x3d, 0x89, 0x2b, 0xda, 0x58,
        0xc9, 0xeb, 0x3d, 0x5d, 0x8f, 0x5d, 0x0f, 0x98, 0xc2, 0x9e, 0xbc, 0x2f, 0x27, 0x22, 0x97,
        0xa0, 0x80, 0xde, 0xc3, 0x7a, 0xbc, 0xa5, 0xc5, 0xe6, 0xa9, 0xa0, 0xc7, 0x87, 0x18, 0xe0,
        0xe8, 0x91, 0x16, 0x07, 0xdf, 0xfa, 0xea, 0xa6, 0xc1, 0x3f, 0x91, 0xe0, 0xa9, 0xc6, 0x38,
        0x61, 0x86, 0x44, 0xb3, 0x6c, 0x16, 0x5c, 0xa1, 0x8a, 0xf0, 0x39, 0x23, 0x5c, 0xff, 0xb3,
        0xe8, 0xf4, 0xdd, 0xfd, 0x8c, 0xda, 0x64, 0x9c, 0xc6, 0xc8, 0x70, 0x82, 0x4e, 0x33, 0xb7,
        0xcb, 0x6f, 0x50, 0x43, 0x26, 0xc4, 0x3e, 0x6f, 0x88, 0xe5, 0x15, 0xc1, 0x23, 0x97, 0x8a,
        0x15, 0x63, 0x18, 0x6c, 0x25, 0x7f, 0x49, 0x3e, 0xa2, 0x6d, 0x53, 0xb7, 0x04, 0x78, 0xa6,
        0x38, 0x22, 0x96, 0x18, 0x76, 0x41, 0x51, 0x0b, 0xb5, 0x11, 0x25, 0xc5, 0x8c, 0x21, 0x81,
        0x5f, 0x57, 0x82, 0x93, 0x84, 0xbf, 0xa7, 0xed, 0x6c, 0x24, 0x58, 0xc9, 0x54, 0xd0, 0x34,
        0x3e, 0xbb, 0xff, 0x29, 0x7e, 0xa1, 0x4a, 0x02, 0x93, 0x02, 0x10, 0x99, 0xfd, 0xee, 0xf4,
        0x65, 0xdf, 0x75, 0xe8, 0x92, 0x1e, 0x2e, 0x74, 0x94, 0x25, 0xb3, 0x2d, 0xaa, 0xee, 0x95,
        0x5f, 0x18, 0xe0, 0xa2, 0x31, 0x80, 0x5a, 0xcd, 0xf7, 0x0a, 0xd9, 0xa1, 0x8b, 0x02, 0xfd,
        0x6b, 0xb7, 0x6c, 0xba, 0x07, 0xbe, 0x31, 0x1c, 0x50, 0xed, 0x44, 0x4f, 0x86, 0xa8, 0x25,
        0x66, 0xd2, 0x6f, 0xc5, 0xf9, 0x0a, 0x96, 0x91, 0xc1, 0xa9, 0xbb, 0x47, 0xf5, 0xc6, 0xcb,
        0x29, 0xbc, 0x60, 0xe9, 0x79, 0x39, 0x41, 0x80, 0xce, 0x46, 0x53, 0x98, 0x98, 0x26, 0xcf,
        0xea, 0x53, 0xaa, 0xf9, 0x71, 0xc0, 0xdb, 0x4e, 0xc2, 0xcd, 0xbd, 0x30, 0xbf, 0x66, 0x5b,
        0xde, 0xbb, 0x6f, 0xda, 0x1c, 0xab, 0x09, 0xbd, 0x6b, 0xa4, 0xf8, 0x21, 0x94, 0xe8, 0x3b,
        0xa5, 0xe6, 0x23, 0xea, 0x3e, 0x27, 0x70, 0x64, 0x2f, 0x0a, 0x26, 0x2c, 0x7d, 0x28, 0x6c,
        0x2c, 0x80, 0xe5, 0xf8, 0xa4, 0x2e, 0x18, 0x1b, 0xbf, 0x60, 0x60, 0x0e, 0xe4, 0xb1, 0x34,
        0x73, 0xce, 0xb8, 0xab, 0xa0, 0x52, 0x3f, 0x81, 0x2e, 0xdf, 0x80, 0xfc, 0xf0, 0x11, 0x28,
        0x6c, 0xd8, 0x89, 0xb5, 0x53, 0x02, 0xd6, 0x57, 0x11, 0x76, 0x5e, 0x34, 0x99, 0xaa, 0x8e,
        0x28, 0xd9, 0x9d, 0xf9, 0x6f, 0x1e, 0x1f, 0xdb, 0x72, 0x43, 0x0e, 0x8b, 0xcc, 0xb5, 0x2a,
        0xcc, 0xbf, 0xc0, 0x67, 0xed, 0x9b, 0x5b, 0x4c, 0xba, 0x07, 0xa9, 0xdc, 0x2f, 0x77, 0x29,
        0x03, 0x20, 0x35, 0x8f, 0x25, 0x63, 0xb8, 0x04, 0xa5, 0xbf, 0xe4, 0x54, 0x4e, 0xa2, 0x95,
        0xa3, 0x1a, 0x1e, 0x1a, 0x2f, 0x76, 0x7b, 0xd9, 0xa9, 0xdc, 0x8e, 0x66, 0x1a, 0xe7, 0x30,
        0x61, 0x8a, 0x3d, 0xae, 0x76, 0x47, 0xda, 0x0d, 0x45, 0xd4, 0xca, 0x57, 0x16, 0x9e, 0xe2,
        0x55, 0xee, 0x9f, 0x95, 0xee, 0x5c, 0x92, 0xdd, 0x07, 0x9c, 0x87, 0x0b, 0x11, 0x35, 0x4c,
        0xb2, 0x4a, 0xe8, 0x2f, 0xd6, 0x38, 0xc0, 0x81, 0x04, 0xda, 0xdd, 0xc3, 0x64, 0x29, 0x2c,
        0x4e, 0x8d, 0xf4, 0x5b, 0x82, 0x80, 0x4a, 0xda, 0xd5, 0x69, 0xbf, 0x88, 0xe8, 0xb5, 0xaa,
        0xe0, 0xf5, 0xa4, 0x81, 0xa4, 0xc5, 0x2b, 0xbe, 0xe0, 0x0c, 0x99, 0x4c, 0xbc, 0x86, 0xc2,
        0xdc, 0xe9, 0xd6, 0x01, 0x64, 0xdb, 0x45, 0xc3, 0x07, 0xa8, 0x29, 0x59, 0x64, 0xa7, 0xa0,
        0x46, 0x8e, 0xb1, 0x34, 0xa9, 0xec, 0x80, 0xda, 0x45, 0xdb, 0xc1, 0xf3, 0x80, 0x5d, 0xe0,
        0xbf, 0x00, 0x90, 0xf8, 0xe1, 0xce, 0x25, 0xc8, 0xa6, 0x54, 0xbb, 0x6d, 0xa5, 0x66, 0x21,
        0xde, 0xdf, 0xe1, 0x43, 0x1e, 0x86, 0xda, 0xdc, 0x97, 0xf9, 0x58, 0x2e, 0xe5, 0x04, 0x66,
        0x3f, 0x02, 0x3a, 0x5f, 0xb4, 0x81, 0xe7, 0xe1, 0x31, 0xfd, 0x04, 0x5a, 0x2f, 0x4e, 0xbf,
        0xfb, 0x07, 0x21, 0xf8, 0x69, 0x8f, 0x07, 0x90, 0x03, 0x92, 0x8d, 0xba, 0xd0, 0xde, 0x86,
        0x6f, 0x61, 0xec, 0xb7, 0x54, 0xac, 0x59, 0x31, 0x3f, 0x09, 0xf7, 0xc9, 0x8f, 0x3f, 0x5b,
        0x8e, 0x01, 0x98, 0xfa, 0xb2, 0xb1, 0x33, 0xb3, 0x1e, 0xc4, 0xef, 0x06, 0x76, 0xd8, 0x1a,
        0x49, 0x29, 0xfa, 0x47, 0xa1, 0x5f, 0x4b, 0xa9, 0x32, 0x07, 0x05, 0xa4, 0x9e, 0x66, 0x05,
        0xa3, 0x1e, 0x75, 0x53, 0x37, 0xa7, 0x5e, 0x2b, 0xcc, 0x01, 0x38, 0xe0, 0x01, 0xc1, 0x41,
        0x71, 0x5a, 0x34, 0x6b, 0xac, 0x95, 0x9f, 0x38, 0x33, 0x65, 0x19, 0xb2, 0x17, 0x66, 0x30,
        0x7c, 0xe1, 0x85, 0x09, 0x94, 0x7f, 0x7a, 0x2b, 0xcf, 0x25, 0x8b, 0x25, 0x10, 0x69, 0x31,
        0x25, 0x30, 0x23, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x15, 0x31,
        0x16, 0x04, 0x14, 0x35, 0xcf, 0xe1, 0x1c, 0xbf, 0xb1, 0xa0, 0xeb, 0xba, 0xde, 0xcb, 0xd9,
        0x1c, 0x7c, 0x1d, 0x59, 0x74, 0xa3, 0x9d, 0x9c, 0x30, 0x41, 0x30, 0x31, 0x30, 0x0d, 0x06,
        0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20, 0x1e,
        0x82, 0x07, 0xe2, 0xa8, 0x97, 0xa4, 0x0f, 0xb8, 0xfa, 0x19, 0x43, 0xf5, 0x6c, 0x8d, 0xe9,
        0x0c, 0x35, 0xa4, 0xd8, 0x1f, 0xff, 0x17, 0xdf, 0x41, 0xeb, 0x28, 0x4f, 0xa3, 0x05, 0x73,
        0x8e, 0x04, 0x08, 0xb2, 0xe7, 0xd2, 0x09, 0x12, 0xd4, 0xbc, 0x91, 0x02, 0x02, 0x08, 0x00,
    ];
    const CLIENT_DER: &[u8] = &[
        0x30, 0x82, 0x03, 0x34, 0x30, 0x82, 0x02, 0x1c, 0xa0, 0x03, 0x02, 0x01, 0x02, 0x02, 0x03,
        0x00, 0x90, 0x09, 0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01,
        0x0b, 0x05, 0x00, 0x30, 0x3d, 0x31, 0x0b, 0x30, 0x09, 0x06, 0x03, 0x55, 0x04, 0x06, 0x13,
        0x02, 0x46, 0x52, 0x31, 0x12, 0x30, 0x10, 0x06, 0x03, 0x55, 0x04, 0x0a, 0x0c, 0x09, 0x47,
        0x6f, 0x72, 0x6f, 0x20, 0x54, 0x65, 0x73, 0x74, 0x31, 0x1a, 0x30, 0x18, 0x06, 0x03, 0x55,
        0x04, 0x03, 0x0c, 0x11, 0x47, 0x6f, 0x72, 0x6f, 0x20, 0x54, 0x65, 0x73, 0x74, 0x20, 0x52,
        0x6f, 0x6f, 0x74, 0x20, 0x43, 0x41, 0x30, 0x20, 0x17, 0x0d, 0x32, 0x35, 0x30, 0x31, 0x30,
        0x31, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x5a, 0x18, 0x0f, 0x32, 0x31, 0x32, 0x35, 0x30,
        0x31, 0x30, 0x31, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x5a, 0x30, 0x16, 0x31, 0x14, 0x30,
        0x12, 0x06, 0x03, 0x55, 0x04, 0x03, 0x0c, 0x0b, 0x47, 0x6f, 0x72, 0x6f, 0x20, 0x43, 0x6c,
        0x69, 0x65, 0x6e, 0x74, 0x30, 0x82, 0x01, 0x22, 0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48,
        0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00, 0x03, 0x82, 0x01, 0x0f, 0x00, 0x30, 0x82,
        0x01, 0x0a, 0x02, 0x82, 0x01, 0x01, 0x00, 0xa5, 0xc3, 0x12, 0xf0, 0xc1, 0x3c, 0xbd, 0x2f,
        0x63, 0x11, 0x53, 0xc2, 0x9b, 0x93, 0xcd, 0xb2, 0xa2, 0x4f, 0xd2, 0x5d, 0x9a, 0xff, 0x9c,
        0x93, 0x1c, 0xb7, 0xba, 0x3d, 0xca, 0xb3, 0x10, 0x78, 0x5f, 0x0f, 0x07, 0xd2, 0x99, 0xed,
        0xb0, 0xd7, 0xdb, 0xab, 0xc6, 0x9e, 0x50, 0x3f, 0xe9, 0x64, 0x1a, 0xb5, 0xc3, 0xa9, 0x8f,
        0x5e, 0x45, 0x42, 0xe4, 0x40, 0x1c, 0x0a, 0xce, 0x61, 0x00, 0x9c, 0x2c, 0xfd, 0x09, 0xde,
        0x1a, 0xc9, 0x88, 0x95, 0xb1, 0x1c, 0x56, 0xbe, 0xed, 0x54, 0x66, 0xb0, 0x54, 0x41, 0xd7,
        0xfe, 0xbc, 0xc0, 0x3b, 0x60, 0xe9, 0x94, 0x4b, 0x6d, 0x6b, 0xb2, 0xbd, 0x61, 0x84, 0x8f,
        0xca, 0x0f, 0x70, 0xd2, 0xc2, 0x43, 0x1e, 0x4d, 0x59, 0x53, 0x68, 0x53, 0x34, 0xe6, 0x86,
        0x98, 0x02, 0x17, 0xeb, 0x5c, 0xcb, 0x37, 0x11, 0x66, 0x03, 0x67, 0xe9, 0x63, 0x87, 0x81,
        0xc9, 0x37, 0x77, 0x92, 0x95, 0x6d, 0xf3, 0xef, 0x38, 0x02, 0x4f, 0x83, 0x90, 0xce, 0x4d,
        0x69, 0x41, 0x28, 0xf3, 0x9e, 0x6a, 0x80, 0x8e, 0xc5, 0x2d, 0x21, 0xec, 0x1e, 0x76, 0x2c,
        0x9b, 0x4c, 0x74, 0x73, 0x24, 0xe0, 0x18, 0xf5, 0xbf, 0xce, 0xda, 0xb8, 0x40, 0xfa, 0xa7,
        0xa3, 0x97, 0xc0, 0xc3, 0x51, 0xdd, 0x6a, 0x47, 0xb1, 0xb0, 0x44, 0x84, 0x29, 0x13, 0x61,
        0x06, 0x73, 0xc2, 0xaf, 0xf0, 0x7d, 0x67, 0x6c, 0xa6, 0xaf, 0xb8, 0xd7, 0x20, 0x2f, 0x33,
        0x51, 0xb8, 0x9d, 0x38, 0x9b, 0x6c, 0x7f, 0x1f, 0xa8, 0xf0, 0xa1, 0xe1, 0xef, 0xe3, 0x95,
        0xa9, 0x33, 0xe5, 0xc5, 0x62, 0xd5, 0xf2, 0x52, 0x8b, 0xb6, 0xe1, 0xc0, 0xaf, 0x89, 0x35,
        0x5c, 0x4c, 0x0c, 0x25, 0xdf, 0xb3, 0x49, 0xd9, 0x65, 0x5f, 0x3e, 0x16, 0x54, 0xdc, 0x57,
        0x55, 0xeb, 0x8c, 0xcf, 0x2e, 0xc1, 0xfd, 0x51, 0x02, 0x03, 0x01, 0x00, 0x01, 0xa3, 0x62,
        0x30, 0x60, 0x30, 0x09, 0x06, 0x03, 0x55, 0x1d, 0x13, 0x04, 0x02, 0x30, 0x00, 0x30, 0x13,
        0x06, 0x03, 0x55, 0x1d, 0x25, 0x04, 0x0c, 0x30, 0x0a, 0x06, 0x08, 0x2b, 0x06, 0x01, 0x05,
        0x05, 0x07, 0x03, 0x02, 0x30, 0x1d, 0x06, 0x03, 0x55, 0x1d, 0x0e, 0x04, 0x16, 0x04, 0x14,
        0xa9, 0xe3, 0x27, 0x07, 0xbc, 0xec, 0x35, 0x2f, 0x45, 0xde, 0x33, 0x08, 0x4a, 0xe2, 0x3f,
        0x49, 0x12, 0xef, 0x15, 0xcc, 0x30, 0x1f, 0x06, 0x03, 0x55, 0x1d, 0x23, 0x04, 0x18, 0x30,
        0x16, 0x80, 0x14, 0x79, 0xdb, 0x50, 0xa8, 0x5c, 0xc5, 0x19, 0x97, 0x63, 0x1a, 0xbb, 0x5b,
        0x15, 0xe2, 0xec, 0x35, 0x86, 0x77, 0x05, 0xc6, 0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48,
        0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b, 0x05, 0x00, 0x03, 0x82, 0x01, 0x01, 0x00, 0x35, 0xcf,
        0x3d, 0x69, 0x51, 0xba, 0x44, 0x8f, 0x2f, 0xf6, 0x74, 0x3a, 0x88, 0x39, 0x1d, 0xd4, 0x14,
        0x7d, 0x30, 0x3a, 0xd9, 0xd5, 0x88, 0xcd, 0x82, 0xba, 0x96, 0x5b, 0x36, 0xba, 0x4b, 0xeb,
        0x65, 0xe1, 0x9d, 0x06, 0x30, 0x8f, 0x74, 0x92, 0xfe, 0x1f, 0x37, 0xf6, 0xed, 0x29, 0x0a,
        0xe3, 0x61, 0x1a, 0x29, 0x44, 0xf5, 0x70, 0x13, 0x23, 0x2b, 0xdf, 0x5d, 0x19, 0x0b, 0xca,
        0x2d, 0xec, 0x7c, 0xb2, 0x62, 0x37, 0x50, 0x6f, 0xc1, 0xa4, 0x1a, 0xbb, 0x04, 0x9a, 0xb3,
        0xb7, 0xac, 0x67, 0x79, 0xcb, 0x7f, 0xf2, 0x53, 0xbb, 0x72, 0x2f, 0xbb, 0x56, 0x5d, 0xec,
        0x7f, 0x40, 0xf3, 0x0a, 0xe2, 0x88, 0x65, 0x4d, 0xab, 0x28, 0x0b, 0xfb, 0x7c, 0xde, 0xe9,
        0x6e, 0x4e, 0x93, 0xf5, 0xce, 0x2a, 0x37, 0xfc, 0x6a, 0xe8, 0x7a, 0x7a, 0x99, 0x41, 0xea,
        0xbb, 0x74, 0xeb, 0xee, 0x83, 0xc5, 0x66, 0xc3, 0x89, 0xde, 0x41, 0x42, 0x9f, 0x8f, 0xe1,
        0x44, 0x7e, 0x8e, 0x55, 0xb9, 0x09, 0xfc, 0xe0, 0x9d, 0x50, 0x66, 0xb1, 0x1e, 0xcf, 0xa1,
        0x60, 0x4c, 0x12, 0x48, 0x49, 0xf8, 0x5e, 0xa0, 0x7e, 0x67, 0x12, 0xf4, 0xcd, 0xa8, 0x09,
        0xe1, 0xce, 0xed, 0x06, 0x58, 0x18, 0xa9, 0x7f, 0xb6, 0x6b, 0x6e, 0x31, 0x24, 0x64, 0xba,
        0x6c, 0x41, 0x8c, 0xc4, 0x5f, 0x5f, 0x6f, 0x4d, 0x31, 0x3e, 0xcb, 0x58, 0xe5, 0x93, 0xa4,
        0xbd, 0x7a, 0x65, 0x6f, 0xea, 0x10, 0x71, 0xc8, 0xc4, 0xb0, 0x5b, 0xab, 0xcd, 0x07, 0xda,
        0x22, 0x67, 0xb5, 0x85, 0xab, 0x17, 0xf4, 0xf0, 0xd6, 0x18, 0x2d, 0xbe, 0x70, 0x7e, 0xcf,
        0x81, 0xfe, 0x58, 0x26, 0x3e, 0xfb, 0x38, 0x83, 0xab, 0x4c, 0xc5, 0x91, 0x5d, 0x19, 0xdd,
        0x95, 0xbf, 0xff, 0x92, 0xdb, 0xb3, 0x19, 0xe0, 0x91, 0xc3, 0xfe, 0x84, 0xf3, 0x8e,
    ];
    const CLIENT_KEY_DER: &[u8] = &[
        0x30, 0x82, 0x04, 0xbc, 0x02, 0x01, 0x00, 0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86,
        0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00, 0x04, 0x82, 0x04, 0xa6, 0x30, 0x82, 0x04, 0xa2,
        0x02, 0x01, 0x00, 0x02, 0x82, 0x01, 0x01, 0x00, 0xa5, 0xc3, 0x12, 0xf0, 0xc1, 0x3c, 0xbd,
        0x2f, 0x63, 0x11, 0x53, 0xc2, 0x9b, 0x93, 0xcd, 0xb2, 0xa2, 0x4f, 0xd2, 0x5d, 0x9a, 0xff,
        0x9c, 0x93, 0x1c, 0xb7, 0xba, 0x3d, 0xca, 0xb3, 0x10, 0x78, 0x5f, 0x0f, 0x07, 0xd2, 0x99,
        0xed, 0xb0, 0xd7, 0xdb, 0xab, 0xc6, 0x9e, 0x50, 0x3f, 0xe9, 0x64, 0x1a, 0xb5, 0xc3, 0xa9,
        0x8f, 0x5e, 0x45, 0x42, 0xe4, 0x40, 0x1c, 0x0a, 0xce, 0x61, 0x00, 0x9c, 0x2c, 0xfd, 0x09,
        0xde, 0x1a, 0xc9, 0x88, 0x95, 0xb1, 0x1c, 0x56, 0xbe, 0xed, 0x54, 0x66, 0xb0, 0x54, 0x41,
        0xd7, 0xfe, 0xbc, 0xc0, 0x3b, 0x60, 0xe9, 0x94, 0x4b, 0x6d, 0x6b, 0xb2, 0xbd, 0x61, 0x84,
        0x8f, 0xca, 0x0f, 0x70, 0xd2, 0xc2, 0x43, 0x1e, 0x4d, 0x59, 0x53, 0x68, 0x53, 0x34, 0xe6,
        0x86, 0x98, 0x02, 0x17, 0xeb, 0x5c, 0xcb, 0x37, 0x11, 0x66, 0x03, 0x67, 0xe9, 0x63, 0x87,
        0x81, 0xc9, 0x37, 0x77, 0x92, 0x95, 0x6d, 0xf3, 0xef, 0x38, 0x02, 0x4f, 0x83, 0x90, 0xce,
        0x4d, 0x69, 0x41, 0x28, 0xf3, 0x9e, 0x6a, 0x80, 0x8e, 0xc5, 0x2d, 0x21, 0xec, 0x1e, 0x76,
        0x2c, 0x9b, 0x4c, 0x74, 0x73, 0x24, 0xe0, 0x18, 0xf5, 0xbf, 0xce, 0xda, 0xb8, 0x40, 0xfa,
        0xa7, 0xa3, 0x97, 0xc0, 0xc3, 0x51, 0xdd, 0x6a, 0x47, 0xb1, 0xb0, 0x44, 0x84, 0x29, 0x13,
        0x61, 0x06, 0x73, 0xc2, 0xaf, 0xf0, 0x7d, 0x67, 0x6c, 0xa6, 0xaf, 0xb8, 0xd7, 0x20, 0x2f,
        0x33, 0x51, 0xb8, 0x9d, 0x38, 0x9b, 0x6c, 0x7f, 0x1f, 0xa8, 0xf0, 0xa1, 0xe1, 0xef, 0xe3,
        0x95, 0xa9, 0x33, 0xe5, 0xc5, 0x62, 0xd5, 0xf2, 0x52, 0x8b, 0xb6, 0xe1, 0xc0, 0xaf, 0x89,
        0x35, 0x5c, 0x4c, 0x0c, 0x25, 0xdf, 0xb3, 0x49, 0xd9, 0x65, 0x5f, 0x3e, 0x16, 0x54, 0xdc,
        0x57, 0x55, 0xeb, 0x8c, 0xcf, 0x2e, 0xc1, 0xfd, 0x51, 0x02, 0x03, 0x01, 0x00, 0x01, 0x02,
        0x82, 0x01, 0x00, 0x16, 0xda, 0x40, 0x83, 0xb9, 0x98, 0x51, 0x45, 0x60, 0x5a, 0x94, 0xaf,
        0x73, 0xe4, 0x6f, 0x76, 0x37, 0xae, 0x05, 0x2f, 0x60, 0xd2, 0xe9, 0xa2, 0xac, 0x7c, 0x60,
        0x96, 0xeb, 0xee, 0xe8, 0xe5, 0x37, 0x71, 0xbf, 0xdf, 0x2c, 0x82, 0x84, 0xf5, 0x6a, 0xc9,
        0xbb, 0xf8, 0x65, 0x26, 0xe7, 0xdb, 0x24, 0x5a, 0xe2, 0x6e, 0x94, 0xc9, 0xe7, 0x31, 0x94,
        0x27, 0x34, 0x81, 0x2c, 0x37, 0xaf, 0xae, 0x58, 0x53, 0x75, 0xa7, 0x4d, 0xb9, 0x1d, 0x75,
        0x49, 0xb5, 0x6c, 0x4e, 0x26, 0x89, 0x12, 0xc9, 0x2f, 0x81, 0xe3, 0xb0, 0x86, 0x69, 0x82,
        0xd0, 0xd7, 0x13, 0x04, 0x9d, 0x16, 0xbd, 0x23, 0x22, 0x77, 0xc9, 0xf2, 0x63, 0xa8, 0xe7,
        0x1d, 0x8a, 0x19, 0xa4, 0xdf, 0x47, 0x97, 0xa3, 0xea, 0x5b, 0x4d, 0x10, 0xaa, 0xda, 0x8d,
        0x03, 0xbe, 0x2e, 0x74, 0x33, 0x1a, 0x0d, 0x70, 0x33, 0x32, 0x61, 0xf2, 0x2e, 0x13, 0x60,
        0xdb, 0xf9, 0x21, 0x4f, 0x40, 0x34, 0xf8, 0x39, 0x5d, 0x34, 0x41, 0x7b, 0x33, 0x0e, 0x8f,
        0xb7, 0x47, 0x69, 0x29, 0xd3, 0x53, 0x12, 0x61, 0x03, 0x9b, 0x49, 0x0c, 0xa1, 0x55, 0x35,
        0xba, 0xd5, 0x04, 0xc9, 0xc8, 0x4e, 0xf5, 0xc5, 0x5e, 0xe4, 0x10, 0x5c, 0xcf, 0xc5, 0x5d,
        0xb8, 0xed, 0xd0, 0xdf, 0xc5, 0xc0, 0x64, 0x88, 0x76, 0x2c, 0x4b, 0x79, 0x45, 0xd4, 0x77,
        0xa0, 0x00, 0xbb, 0xc9, 0x0e, 0xaf, 0xa4, 0x93, 0xd8, 0x69, 0x8a, 0x17, 0x0d, 0x16, 0x82,
        0x4e, 0xe1, 0xea, 0xe1, 0xd9, 0xe2, 0x41, 0xfa, 0xbc, 0x21, 0xf9, 0xde, 0x5a, 0x83, 0x0b,
        0x44, 0xa0, 0xf4, 0x1e, 0x1b, 0xa6, 0xe3, 0x6a, 0x9c, 0x57, 0x87, 0x4b, 0xea, 0xb8, 0x81,
        0x92, 0x83, 0xde, 0xa5, 0xf0, 0x71, 0x0d, 0x63, 0x13, 0xce, 0xc3, 0xc6, 0x62, 0x99, 0x0a,
        0x97, 0xe4, 0x28, 0xdb, 0x02, 0x81, 0x81, 0x00, 0xd9, 0xd3, 0x18, 0xd1, 0xee, 0x08, 0x87,
        0x4e, 0x58, 0xeb, 0xa5, 0xe7, 0x9d, 0x7b, 0xf5, 0x1e, 0x54, 0x39, 0x5d, 0x08, 0xbe, 0xdb,
        0xcf, 0x3d, 0x3b, 0x8b, 0x6b, 0xe2, 0xea, 0x10, 0xe5, 0x1f, 0x9f, 0x08, 0x36, 0x27, 0x93,
        0xff, 0x6e, 0x40, 0xb5, 0x47, 0xbf, 0x9e, 0x2f, 0x6c, 0x6c, 0x52, 0xa6, 0x7d, 0x38, 0xc0,
        0xa0, 0xf4, 0x74, 0x6c, 0xe2, 0x90, 0xf1, 0xb0, 0xc4, 0x9a, 0x8b, 0x78, 0x0c, 0x3a, 0x0d,
        0xab, 0x19, 0x52, 0xba, 0x58, 0xb9, 0xd2, 0xdc, 0x75, 0xe3, 0xe1, 0xe7, 0x75, 0xc5, 0xcc,
        0xbc, 0x72, 0xc4, 0xac, 0x7e, 0x9f, 0x0d, 0x6c, 0x38, 0x55, 0x72, 0xc0, 0x5b, 0xb2, 0xdf,
        0x6e, 0xa1, 0x4d, 0x06, 0x7c, 0xe0, 0x94, 0xe1, 0xfa, 0xeb, 0x4c, 0x8c, 0x90, 0x22, 0xd9,
        0xd6, 0x82, 0xfc, 0x13, 0x55, 0xc6, 0xf8, 0xe7, 0xb5, 0xa6, 0xd7, 0x19, 0x3b, 0x6f, 0xb0,
        0xeb, 0x02, 0x81, 0x81, 0x00, 0xc2, 0xd0, 0x23, 0xfb, 0x15, 0x0e, 0xb7, 0x10, 0x82, 0xb6,
        0xdd, 0x08, 0xee, 0xf9, 0x48, 0x30, 0xe1, 0x8b, 0x06, 0x29, 0xa6, 0x35, 0xb7, 0x3a, 0xc6,
        0x64, 0x5f, 0x12, 0xb6, 0xf1, 0xc6, 0x7e, 0x75, 0x4e, 0xd3, 0x02, 0x10, 0xcc, 0xac, 0xb8,
        0xbb, 0xfc, 0x72, 0x1d, 0x63, 0x05, 0x4b, 0x34, 0xc2, 0xb7, 0x5e, 0x60, 0x83, 0xc5, 0x95,
        0x32, 0xe7, 0x2d, 0x13, 0x23, 0xc3, 0x32, 0x92, 0xd0, 0x28, 0x71, 0x5a, 0x09, 0x2f, 0x76,
        0xdd, 0x62, 0xd8, 0x77, 0xf8, 0x26, 0x90, 0xcb, 0x8f, 0x54, 0x1a, 0x46, 0xdb, 0xaf, 0x0f,
        0x29, 0x43, 0x33, 0x76, 0x72, 0x1d, 0xf5, 0x58, 0x89, 0xde, 0xd3, 0x74, 0x28, 0x83, 0x71,
        0xb0, 0x58, 0x11, 0xb4, 0x69, 0x0b, 0x06, 0x91, 0xae, 0x33, 0x9c, 0x22, 0x7e, 0xde, 0xc5,
        0x34, 0xe1, 0xed, 0x05, 0xfd, 0xcc, 0xeb, 0xd6, 0xa2, 0x43, 0xe1, 0x9b, 0xb3, 0x02, 0x81,
        0x80, 0x28, 0xe1, 0xa1, 0xaa, 0xe4, 0xcd, 0xdc, 0xf7, 0x2b, 0x61, 0x2e, 0xc6, 0x05, 0x9c,
        0x9a, 0x28, 0x93, 0x0e, 0x21, 0x13, 0xdf, 0x1e, 0x4a, 0x0a, 0xe7, 0xfe, 0x06, 0xc5, 0x93,
        0xc0, 0x90, 0x00, 0x1b, 0x4c, 0xe3, 0x3d, 0x12, 0x5d, 0xc2, 0x47, 0x8a, 0xbe, 0x68, 0x8c,
        0x3b, 0xc2, 0x51, 0xa2, 0x41, 0x75, 0x07, 0x0d, 0xc0, 0x27, 0xa1, 0x22, 0x3e, 0x73, 0x8d,
        0xa8, 0xeb, 0x7d, 0x45, 0xe7, 0xc6, 0x03, 0xaf, 0x45, 0x09, 0x7f, 0xa9, 0x51, 0xb5, 0xae,
        0x89, 0x89, 0xe6, 0xb7, 0x23, 0x45, 0xcb, 0x55, 0xdd, 0x98, 0x7c, 0xa9, 0x13, 0x2a, 0xae,
        0xce, 0x3c, 0xd4, 0x6c, 0xb1, 0x05, 0x84, 0xd0, 0xc2, 0x66, 0xab, 0x9a, 0x28, 0x1d, 0xce,
        0x53, 0x8b, 0x50, 0x56, 0x11, 0x9b, 0x8b, 0xae, 0xee, 0xbf, 0xae, 0xa7, 0xc2, 0x55, 0x91,
        0xa9, 0x8b, 0x42, 0x40, 0xe4, 0x8f, 0x6f, 0x2a, 0xf9, 0x02, 0x81, 0x80, 0x05, 0x49, 0x04,
        0x50, 0xe1, 0x87, 0x5e, 0xac, 0x11, 0xff, 0xc8, 0x0b, 0x60, 0x00, 0x5f, 0x57, 0x04, 0x27,
        0x73, 0x69, 0x59, 0xdb, 0x07, 0x37, 0x8e, 0xfe, 0x56, 0x1d, 0xf9, 0xdc, 0xa1, 0x57, 0xf8,
        0xf2, 0xc4, 0x82, 0xbf, 0xd5, 0xed, 0xb1, 0xad, 0x57, 0xf1, 0xf8, 0x9e, 0x21, 0x1c, 0xa3,
        0x37, 0xcd, 0x25, 0xc0, 0x54, 0x92, 0xdd, 0xb3, 0x5d, 0xd6, 0x39, 0x38, 0xb5, 0x29, 0x00,
        0x16, 0xa9, 0xa2, 0x39, 0xa1, 0x1a, 0x2a, 0x8d, 0x63, 0x0b, 0x20, 0x66, 0x19, 0x15, 0xc0,
        0x90, 0x08, 0xe6, 0x15, 0x9a, 0x0c, 0x2b, 0xcb, 0x20, 0xc9, 0x9a, 0xa3, 0x09, 0xf1, 0x08,
        0x0f, 0xe5, 0x7d, 0xb3, 0x1f, 0x0e, 0x41, 0xdd, 0x58, 0xe6, 0x52, 0x10, 0xaf, 0x7d, 0xe4,
        0x21, 0xe7, 0xc0, 0x8a, 0xa7, 0x0a, 0x80, 0x2b, 0x29, 0x1d, 0xf7, 0x15, 0xa5, 0xa3, 0x98,
        0x55, 0x1a, 0xa6, 0x5a, 0x45, 0x02, 0x81, 0x80, 0x31, 0x0b, 0xe2, 0xe3, 0x68, 0x5b, 0x16,
        0x11, 0xe4, 0x42, 0x38, 0x2b, 0x77, 0x72, 0xed, 0x1a, 0x98, 0xae, 0xc8, 0x80, 0x1d, 0xc2,
        0xeb, 0xb0, 0x1e, 0xc7, 0xe5, 0xcf, 0x7e, 0xaf, 0xef, 0x96, 0x58, 0x64, 0x43, 0xe2, 0x2c,
        0x32, 0xf4, 0x10, 0x15, 0xc3, 0x0e, 0x50, 0xbf, 0x22, 0x56, 0xfc, 0xb8, 0xb0, 0x84, 0x21,
        0xfb, 0x1e, 0x4a, 0xb4, 0x41, 0x64, 0x39, 0x2b, 0x89, 0x86, 0xef, 0xe6, 0x1e, 0xbc, 0xf1,
        0xc5, 0xc7, 0x84, 0x24, 0x31, 0x67, 0x7f, 0x86, 0xc2, 0x96, 0x77, 0x46, 0x36, 0xd6, 0xb7,
        0x88, 0x5a, 0xbf, 0xf1, 0xef, 0x69, 0xab, 0xeb, 0xff, 0xc5, 0x60, 0xff, 0x19, 0xdb, 0x64,
        0x57, 0x6d, 0x82, 0xd4, 0xf7, 0xa5, 0x14, 0x26, 0x9b, 0x43, 0x55, 0x7a, 0x4c, 0x57, 0xcd,
        0x8c, 0x7b, 0x51, 0x4a, 0x1f, 0x86, 0x35, 0x57, 0x85, 0x53, 0xf7, 0xe6, 0x94, 0xc8, 0xf4,
        0x2b,
    ];
    const LEAF_PUB_DER: &[u8] = &[
        0x30, 0x82, 0x01, 0x22, 0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01,
        0x01, 0x01, 0x05, 0x00, 0x03, 0x82, 0x01, 0x0f, 0x00, 0x30, 0x82, 0x01, 0x0a, 0x02, 0x82,
        0x01, 0x01, 0x00, 0xa1, 0x1d, 0x1d, 0x63, 0xc4, 0x0c, 0xc8, 0xf3, 0xc9, 0xb8, 0x77, 0x27,
        0x7e, 0xb5, 0x65, 0x02, 0x24, 0xa4, 0x1f, 0x24, 0x8b, 0xcc, 0x17, 0x72, 0x24, 0x5e, 0xcb,
        0xe2, 0x93, 0xa5, 0xb9, 0x05, 0x88, 0xdf, 0x8f, 0xca, 0x92, 0xf0, 0x45, 0xc8, 0x1e, 0x9c,
        0x57, 0xf9, 0x89, 0xbc, 0x98, 0xcc, 0xa4, 0xdf, 0x7c, 0x6a, 0x97, 0x66, 0x12, 0x0c, 0x61,
        0xc1, 0x8c, 0xde, 0x7e, 0x19, 0xcc, 0xd1, 0x5b, 0xdf, 0xb0, 0x5b, 0xc4, 0xd1, 0x2d, 0xe1,
        0x03, 0xc2, 0xa2, 0xde, 0xf1, 0xdc, 0x5d, 0xc9, 0x32, 0x8b, 0xd1, 0x5b, 0x4e, 0x34, 0x19,
        0x72, 0x78, 0x6c, 0x07, 0xd2, 0xa4, 0xb6, 0x2f, 0x4d, 0x2c, 0xfb, 0x51, 0xcc, 0x39, 0x86,
        0x51, 0x54, 0xeb, 0x4a, 0xf5, 0x18, 0x20, 0xed, 0x8d, 0xd2, 0xfa, 0xc2, 0xd6, 0xc0, 0xc2,
        0x39, 0xfa, 0x33, 0x91, 0xbc, 0x57, 0x47, 0x3d, 0xab, 0x12, 0x22, 0x2a, 0x06, 0x90, 0xf8,
        0xdd, 0x2a, 0x26, 0x37, 0x6c, 0x36, 0x48, 0xa8, 0x7e, 0xb7, 0x56, 0x66, 0xbe, 0x50, 0x8a,
        0x1c, 0x4d, 0x77, 0x8f, 0xc1, 0x7d, 0x47, 0xa9, 0xa9, 0xdb, 0xba, 0x51, 0xc1, 0xb7, 0x91,
        0x3b, 0x85, 0xb1, 0x93, 0x94, 0xf1, 0x5b, 0x22, 0x8e, 0x7a, 0x6d, 0x02, 0x9e, 0xf5, 0x96,
        0x7f, 0x05, 0x2f, 0x62, 0xcc, 0x9e, 0x6d, 0xcd, 0x5b, 0xdc, 0xd0, 0xbb, 0xb6, 0x05, 0x87,
        0x68, 0x33, 0xf0, 0xb0, 0xc0, 0xee, 0x15, 0x00, 0x9d, 0x58, 0xab, 0x0f, 0xa7, 0xb7, 0x1b,
        0x18, 0x59, 0x40, 0x53, 0xd9, 0x7c, 0xf2, 0x30, 0xff, 0xc0, 0x71, 0x32, 0xc8, 0x38, 0xed,
        0x8b, 0xb7, 0x44, 0x60, 0x8f, 0x28, 0x92, 0xf6, 0xf1, 0x8d, 0x2a, 0xc1, 0x44, 0x11, 0xf8,
        0x3c, 0x81, 0xae, 0xbd, 0x37, 0x5f, 0x0b, 0x43, 0x2d, 0x09, 0x67, 0x96, 0x70, 0x4a, 0x25,
        0x1b, 0x50, 0x10, 0x57, 0x02, 0x03, 0x01, 0x00, 0x01,
    ];
    const CERTINFO_KS: &[&[&[u8]]] = &[
        &[
            b"Subject:C = FR, ST = \\C3\\8Ele-de-France, L = \"Paris, 75000\", O = \"Goro \\\"Test\\\" \", OU = Unit 2, CN = localhost, emailAddress = admin@example.com, serialNumber = 12345, DC = example, UID = uid1",
            b"Issuer:C = FR, O = Goro Test, CN = Goro Test Root CA",
            b"Version:2",
            b"Serial Number:ff00ff00ff00ff00ff",
            b"Signature Algorithm:sha384WithRSAEncryption",
            b"Public Key Algorithm:rsaEncryption",
            b"X509v3 Basic Constraints:CA:FALSE",
            b"X509v3 Key Usage:Digital Signature, Non Repudiation, Key Encipherment, Data Encipherment, Key Agreement, Encipher Only",
            b"X509v3 Extended Key Usage:TLS Web Server Authentication, TLS Web Client Authentication, Code Signing, E-mail Protection, Time Stamping, OCSP Signing, 1.2.3.4.5",
            b"X509v3 Subject Key Identifier:E7:BF:8D:E0:36:F3:D2:03:31:5D:32:8C:2D:80:B4:B9:AF:F3:52:00",
            b"X509v3 Authority Key Identifier:keyid:79:DB:50:A8:5C:C5:19:97:63:1A:BB:5B:15:E2:EC:35:86:77:05:C6\nDirName:/C=FR/O=Goro Test/CN=Goro Test Root CA\nserial:10:01",
            b"X509v3 Subject Alternative Name:DNS:localhost, email:foo@example.com, URI:http://example.com/x, IP Address:10.0.0.1, IP Address:2001:DB8:0:0:0:0:0:1, DirName:/C=US/O=Dir Org/CN=Dir Name, Registered ID:1.2.3.4",
            b"X509v3 Issuer Alternative Name:DNS:issuer.example",
            b"X509v3 CRL Distribution Points:Full Name:\n  URI:http://crl.example.com/a.crl\n\nFull Name:\n  URI:http://crl2.example.com/b.crl\n",
            b"Authority Information Access:OCSP - URI:http://ocsp.example.com/\nCA Issuers - URI:http://ca.example.com/ca.crt",
            b"X509v3 Certificate Policies:Policy: 2.23.140.1.2.1\nPolicy: 1.3.6.1.4.1.99999.1\n  CPS: http://cps.example.com/\n  User Notice:\n    Organization: Org Name\n    Numbers: 1, 2\n    Explicit Text: Explicit notice text",
            b"Netscape Comment:Hello world comment",
            b"Netscape Cert Type:SSL Client, SSL Server, S/MIME",
            b"TLS Feature:status_request",
            b"1.2.3.4.99:..Some custom value",
            b"Start date:Jan  1 00:00:00 2025 GMT",
            b"Expire date:Jan  1 00:00:00 2125 GMT",
            b"RSA Public Key:2048",
            b"rsa(n):A11D1D63C40CC8F3C9B877277EB5650224A41F248BCC1772245ECBE293A5B90588DF8FCA92F045C81E9C57F989BC98CCA4DF7C6A9766120C61C18CDE7E19CCD15BDFB05BC4D12DE103C2A2DEF1DC5DC9328BD15B4E341972786C07D2A4B62F4D2CFB51CC39865154EB4AF51820ED8DD2FAC2D6C0C239FA3391BC57473DAB12222A0690F8DD2A26376C3648A87EB75666BE508A1C4D778FC17D47A9A9DBBA51C1B7913B85B19394F15B228E7A6D029EF5967F052F62CC9E6DCD5BDCD0BBB605876833F0B0C0EE15009D58AB0FA7B71B18594053D97CF230FFC07132C838ED8BB744608F2892F6F18D2AC14411F83C81AEBD375F0B432D096796704A251B501057",
            b"rsa(e):10001",
            b"Signature:63:e5:5c:90:68:b1:3d:6c:ee:80:cc:57:80:65:0a:c5:8e:a1:bc:b5:20:95:d5:5c:d8:c0:8e:37:d6:51:53:ea:00:b8:94:68:92:7a:d3:93:b9:69:95:36:4c:58:dd:9a:0d:d4:f7:76:88:66:c8:8d:f7:ae:1a:73:d1:2d:91:b0:20:ad:90:0f:2a:af:3f:62:60:24:04:a4:fc:35:c6:81:67:06:29:e2:b8:c0:32:55:0e:d9:d9:4b:cd:6a:d1:f1:14:b8:6a:99:7b:a9:6d:b2:be:74:5c:84:42:c0:3c:27:6b:91:78:e2:eb:5c:50:fd:5d:aa:5c:f9:87:ad:5d:df:65:7c:8a:c2:3e:e5:c4:49:01:25:a4:48:eb:6c:eb:7a:64:f3:90:63:47:a0:a4:28:6d:dc:f6:ba:18:93:73:e8:0d:67:d6:5f:8a:bd:16:ce:7d:98:6e:af:2f:2e:10:b5:c3:be:55:b8:6a:b5:2c:5b:2c:51:06:47:1b:e9:14:03:1e:00:21:f9:ad:48:51:93:d0:9d:e2:03:6a:32:14:5b:95:09:cd:04:34:77:25:dd:3c:41:82:03:89:b6:c0:8e:aa:71:77:0f:d4:fc:54:db:ff:6d:64:fd:5d:ce:3b:cb:7c:e1:c2:88:e6:1f:fd:ca:ff:e6:78:21:de:2e:00:91:",
            b"Cert:-----BEGIN CERTIFICATE-----\nMIIG3TCCBcWgAwIBAgIKAP8A/wD/AP8A/zANBgkqhkiG9w0BAQwFADA9MQswCQYD\nVQQGEwJGUjESMBAGA1UECgwJR29ybyBUZXN0MRowGAYDVQQDDBFHb3JvIFRlc3Qg\nUm9vdCBDQTAgFw0yNTAxMDEwMDAwMDBaGA8yMTI1MDEwMTAwMDAwMFowgdoxCzAJ\nBgNVBAYTAkZSMRcwFQYDVQQIDA7DjmxlLWRlLUZyYW5jZTEVMBMGA1UEBwwMUGFy\naXMsIDc1MDAwMRUwEwYDVQQKDAxHb3JvICJUZXN0IiAxDzANBgNVBAsMBlVuaXQg\nMjESMBAGA1UEAwwJbG9jYWxob3N0MSAwHgYJKoZIhvcNAQkBFhFhZG1pbkBleGFt\ncGxlLmNvbTEOMAwGA1UEBRMFMTIzNDUxFzAVBgoJkiaJk/IsZAEZFgdleGFtcGxl\nMRQwEgYKCZImiZPyLGQBAQwEdWlkMTCCASIwDQYJKoZIhvcNAQEBBQADggEPADCC\nAQoCggEBAKEdHWPEDMjzybh3J361ZQIkpB8ki8wXciRey+KTpbkFiN+PypLwRcge\nnFf5ibyYzKTffGqXZhIMYcGM3n4ZzNFb37BbxNEt4QPCot7x3F3JMovRW040GXJ4\nbAfSpLYvTSz7Ucw5hlFU60r1GCDtjdL6wtbAwjn6M5G8V0c9qxIiKgaQ+N0qJjds\nNkiofrdWZr5QihxNd4/BfUepqdu6UcG3kTuFsZOU8VsijnptAp71ln8FL2LMnm3N\nW9zQu7YFh2gz8LDA7hUAnVirD6e3GxhZQFPZfPIw/8BxMsg47Yu3RGCPKJL28Y0q\nwUQR+DyBrr03XwtDLQlnlnBKJRtQEFcCAwEAAaOCAz0wggM5MAwGA1UdEwEB/wQC\nMAAwCwYDVR0PBAQDAgD5MEsGA1UdJQREMEIGCCsGAQUFBwMBBggrBgEFBQcDAgYI\nKwYBBQUHAwMGCCsGAQUFBwMEBggrBgEFBQcDCAYIKwYBBQUHAwkGBCoDBAUwHQYD\nVR0OBBYEFOe/jeA289IDMV0yjC2AtLmv81IAMGYGA1UdIwRfMF2AFHnbUKhcxRmX\nYxq7WxXi7DWGdwXGoUGkPzA9MQswCQYDVQQGEwJGUjESMBAGA1UECgwJR29ybyBU\nZXN0MRowGAYDVQQDDBFHb3JvIFRlc3QgUm9vdCBDQYICEAEwgZAGA1UdEQSBiDCB\nhYIJbG9jYWxob3N0gQ9mb29AZXhhbXBsZS5jb22GFGh0dHA6Ly9leGFtcGxlLmNv\nbS94hwQKAAABhxAgAQ24AAAAAAAAAAAAAAABpDQwMjELMAkGA1UEBhMCVVMxEDAO\nBgNVBAoMB0RpciBPcmcxETAPBgNVBAMMCERpciBOYW1liAMqAwQwGQYDVR0SBBIw\nEIIOaXNzdWVyLmV4YW1wbGUwUgYDVR0fBEswSTAioCCgHoYcaHR0cDovL2NybC5l\neGFtcGxlLmNvbS9hLmNybDAjoCGgH4YdaHR0cDovL2NybDIuZXhhbXBsZS5jb20v\nYi5jcmwwXgYIKwYBBQUHAQEEUjBQMCQGCCsGAQUFBzABhhhodHRwOi8vb2NzcC5l\neGFtcGxlLmNvbS8wKAYIKwYBBQUHMAKGHGh0dHA6Ly9jYS5leGFtcGxlLmNvbS9j\nYS5jcnQwfwYDVR0gBHgwdjAIBgZngQwBAgEwagYJKwYBBAGGjR8BMF0wIwYIKwYB\nBQUHAgEWF2h0dHA6Ly9jcHMuZXhhbXBsZS5jb20vMDYGCCsGAQUFBwICMCowEhoI\nT3JnIE5hbWUwBgIBAQIBAhoURXhwbGljaXQgbm90aWNlIHRleHQwIgYJYIZIAYb4\nQgENBBUWE0hlbGxvIHdvcmxkIGNvbW1lbnQwEQYJYIZIAYb4QgEBBAQDAgXgMBEG\nCCsGAQUFBwEYBAUwAwIBBTAbBgQqAwRjBBMMEVNvbWUgY3VzdG9tIHZhbHVlMA0G\nCSqGSIb3DQEBDAUAA4IBAQBj5VyQaLE9bO6AzFeAZQrFjqG8tSCV1VzYwI431lFT\n6gC4lGiSetOTuWmVNkxY3ZoN1Pd2iGbIjfeuGnPRLZGwIK2QDyqvP2JgJASk/DXG\ngWcGKeK4wDJVDtnZS81q0fEUuGqZe6ltsr50XIRCwDwna5F44utcUP1dqlz5h61d\n32V8isI+5cRJASWkSOts63pk85BjR6CkKG3c9roYk3PoDWfWX4q9Fs59mG6vLy4Q\ntcO+VbhqtSxbLFEGRxvpFAMeACH5rUhRk9Cd4gNqMhRblQnNBDR3Jd08QYIDibbA\njqpxdw/U/FTb/21k/V3OO8t84cKI5h/9yv/meCHeLgCR\n-----END CERTIFICATE-----\n",
        ],
    ];
    const CERTINFO_EC: &[&[&[u8]]] = &[
        &[
            b"Subject:CN = localhost",
            b"Issuer:C = FR, O = Goro Test, CN = Goro Test Root CA",
            b"Version:2",
            b"Serial Number:4004",
            b"Signature Algorithm:sha256WithRSAEncryption",
            b"Public Key Algorithm:id-ecPublicKey",
            b"X509v3 Basic Constraints:CA:FALSE",
            b"X509v3 Key Usage:Digital Signature, Key Encipherment",
            b"X509v3 Extended Key Usage:TLS Web Server Authentication",
            b"X509v3 Subject Key Identifier:FC:67:76:FB:BA:ED:EA:AA:15:25:8C:4F:75:1D:8A:A3:6D:83:9E:8F",
            b"X509v3 Authority Key Identifier:79:DB:50:A8:5C:C5:19:97:63:1A:BB:5B:15:E2:EC:35:86:77:05:C6",
            b"X509v3 Subject Alternative Name:DNS:localhost, DNS:*.example.test, IP Address:127.0.0.1, IP Address:0:0:0:0:0:0:0:1",
            b"Start date:Jan  1 00:00:00 2025 GMT",
            b"Expire date:Jan  1 00:00:00 2125 GMT",
            b"Signature:81:a3:82:e4:21:79:97:a8:41:b8:60:1f:8c:81:28:a8:f3:83:8c:1d:79:33:3f:c6:38:83:87:ba:68:0c:a0:b1:dc:d6:6c:57:b4:20:91:39:18:6d:3b:a4:6c:99:c8:d2:b3:df:a5:0a:e8:12:50:af:27:41:18:c1:9c:47:e3:ab:15:1c:63:bd:33:50:3b:55:d0:89:d2:03:bc:4c:9d:60:25:31:66:c5:ed:88:b8:7e:7f:c6:46:3a:46:db:b1:25:a6:79:0e:a7:cd:e2:72:2d:7d:1b:0f:37:f6:92:03:0f:28:a0:79:40:da:16:f0:92:af:08:f9:46:0f:ef:20:d6:60:1c:77:35:6e:60:4b:92:91:2c:74:77:b7:7a:8a:9b:2f:72:87:d4:74:78:11:da:27:93:b9:8d:62:3d:d4:9b:51:13:6a:62:a0:65:98:e9:6f:df:ef:20:2e:cd:ff:a1:0e:12:77:8b:a1:55:50:40:32:f2:aa:35:cb:5c:e2:97:43:8b:af:6a:1c:4a:ce:ae:90:99:8f:03:0e:7b:06:a8:28:03:a5:29:a6:1a:e5:0a:fd:fe:d6:fd:de:ab:c3:a4:9b:58:d3:60:cb:e6:78:b1:bf:ca:6f:dd:43:65:c5:92:74:6a:9f:44:16:bd:21:96:b4:d0:6d:4b:9f:b3:0a:f1:",
            b"Cert:-----BEGIN CERTIFICATE-----\nMIICtjCCAZ6gAwIBAgICQAQwDQYJKoZIhvcNAQELBQAwPTELMAkGA1UEBhMCRlIx\nEjAQBgNVBAoMCUdvcm8gVGVzdDEaMBgGA1UEAwwRR29ybyBUZXN0IFJvb3QgQ0Ew\nIBcNMjUwMTAxMDAwMDAwWhgPMjEyNTAxMDEwMDAwMDBaMBQxEjAQBgNVBAMMCWxv\nY2FsaG9zdDBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABMeb59F5aHzQ+mvxibdl\ni1igUK0tJ8H9O72TW15M93h0JVCpk9Xsnj4D8Iu2TZM2M0kpWdwOSf0Oei1Y2VtF\nfbyjgbEwga4wCQYDVR0TBAIwADAOBgNVHQ8BAf8EBAMCBaAwEwYDVR0lBAwwCgYI\nKwYBBQUHAwEwHQYDVR0OBBYEFPxndvu67eqqFSWMT3UdiqNtg56PMB8GA1UdIwQY\nMBaAFHnbUKhcxRmXYxq7WxXi7DWGdwXGMDwGA1UdEQQ1MDOCCWxvY2FsaG9zdIIO\nKi5leGFtcGxlLnRlc3SHBH8AAAGHEAAAAAAAAAAAAAAAAAAAAAEwDQYJKoZIhvcN\nAQELBQADggEBAIGjguQheZeoQbhgH4yBKKjzg4wdeTM/xjiDh7poDKCx3NZsV7Qg\nkTkYbTukbJnI0rPfpQroElCvJ0EYwZxH46sVHGO9M1A7VdCJ0gO8TJ1gJTFmxe2I\nuH5/xkY6RtuxJaZ5DqfN4nItfRsPN/aSAw8ooHlA2hbwkq8I+UYP7yDWYBx3NW5g\nS5KRLHR3t3qKmy9yh9R0eBHaJ5O5jWI91JtRE2pioGWY6W/f7yAuzf+hDhJ3i6FV\nUEAy8qo1y1zil0OLr2ocSs6ukJmPAw57BqgoA6UpphrlCv3+1v3eq8Okm1jTYMvm\neLG/ym/dQ2XFknRqn0QWvSGWtNBtS5+zCvE=\n-----END CERTIFICATE-----\n",
        ],
    ];
    const CERTINFO_CHAIN: &[&[&[u8]]] = &[
        &[
            b"Subject:C = FR, ST = Paris, O = Goro Test, CN = localhost",
            b"Issuer:C = FR, O = Goro Test, CN = Goro Test Intermediate",
            b"Version:2",
            b"Serial Number:3003",
            b"Signature Algorithm:sha256WithRSAEncryption",
            b"Public Key Algorithm:rsaEncryption",
            b"X509v3 Basic Constraints:CA:FALSE",
            b"X509v3 Key Usage:Digital Signature, Key Encipherment",
            b"X509v3 Extended Key Usage:TLS Web Server Authentication",
            b"X509v3 Subject Key Identifier:E7:BF:8D:E0:36:F3:D2:03:31:5D:32:8C:2D:80:B4:B9:AF:F3:52:00",
            b"X509v3 Authority Key Identifier:21:51:0C:14:08:C7:25:77:3A:AE:96:8D:9E:EE:23:18:F8:23:67:74",
            b"X509v3 Subject Alternative Name:DNS:localhost, DNS:*.example.test, IP Address:127.0.0.1, IP Address:0:0:0:0:0:0:0:1",
            b"Start date:Jan  1 00:00:00 2025 GMT",
            b"Expire date:Jan  1 00:00:00 2125 GMT",
            b"RSA Public Key:2048",
            b"rsa(n):A11D1D63C40CC8F3C9B877277EB5650224A41F248BCC1772245ECBE293A5B90588DF8FCA92F045C81E9C57F989BC98CCA4DF7C6A9766120C61C18CDE7E19CCD15BDFB05BC4D12DE103C2A2DEF1DC5DC9328BD15B4E341972786C07D2A4B62F4D2CFB51CC39865154EB4AF51820ED8DD2FAC2D6C0C239FA3391BC57473DAB12222A0690F8DD2A26376C3648A87EB75666BE508A1C4D778FC17D47A9A9DBBA51C1B7913B85B19394F15B228E7A6D029EF5967F052F62CC9E6DCD5BDCD0BBB605876833F0B0C0EE15009D58AB0FA7B71B18594053D97CF230FFC07132C838ED8BB744608F2892F6F18D2AC14411F83C81AEBD375F0B432D096796704A251B501057",
            b"rsa(e):10001",
            b"Signature:85:d1:92:e0:5a:57:a4:d0:50:68:40:55:c7:ba:fa:2a:1c:41:cf:d0:2b:73:cd:ab:0a:af:cb:54:75:80:ad:01:c5:bc:2a:84:9a:9a:b6:81:b4:5b:af:57:c0:65:84:20:98:22:64:64:7d:e0:79:2a:bb:22:51:0a:49:2b:7b:d4:39:e8:42:08:e8:0e:d3:eb:1f:96:7a:89:29:df:25:f9:97:93:89:8a:5d:57:8a:3e:ce:60:a5:ee:23:6b:de:13:49:65:5e:e1:7c:20:e3:ea:8c:5c:ba:97:bb:24:66:05:01:17:de:6b:12:b7:23:d9:22:c8:f4:27:97:27:e0:96:b8:31:ce:6a:ae:a3:63:7e:88:cf:f8:18:01:50:d5:f9:bb:10:26:98:a8:2d:f6:52:9b:ae:4b:db:4c:32:1a:50:05:ca:8e:96:38:ee:49:15:a5:fd:1c:69:87:5c:a9:76:f5:88:1c:9a:3d:74:d9:36:16:67:dc:6a:22:20:f2:96:7a:63:ed:07:67:9c:dd:4a:8c:03:69:46:d0:54:ab:4d:53:66:5e:74:9b:b6:f6:7e:7d:6c:55:8d:ed:bd:47:f7:4d:c7:28:cf:53:5f:f3:71:ad:0b:16:f9:f3:4a:2d:7d:49:79:97:4a:46:7d:06:33:ec:4f:dc:4b:fc:49:05:9b:",
            b"Cert:-----BEGIN CERTIFICATE-----\nMIIDtzCCAp+gAwIBAgICMAMwDQYJKoZIhvcNAQELBQAwQjELMAkGA1UEBhMCRlIx\nEjAQBgNVBAoMCUdvcm8gVGVzdDEfMB0GA1UEAwwWR29ybyBUZXN0IEludGVybWVk\naWF0ZTAgFw0yNTAxMDEwMDAwMDBaGA8yMTI1MDEwMTAwMDAwMFowRTELMAkGA1UE\nBhMCRlIxDjAMBgNVBAgMBVBhcmlzMRIwEAYDVQQKDAlHb3JvIFRlc3QxEjAQBgNV\nBAMMCWxvY2FsaG9zdDCCASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBAKEd\nHWPEDMjzybh3J361ZQIkpB8ki8wXciRey+KTpbkFiN+PypLwRcgenFf5ibyYzKTf\nfGqXZhIMYcGM3n4ZzNFb37BbxNEt4QPCot7x3F3JMovRW040GXJ4bAfSpLYvTSz7\nUcw5hlFU60r1GCDtjdL6wtbAwjn6M5G8V0c9qxIiKgaQ+N0qJjdsNkiofrdWZr5Q\nihxNd4/BfUepqdu6UcG3kTuFsZOU8VsijnptAp71ln8FL2LMnm3NW9zQu7YFh2gz\n8LDA7hUAnVirD6e3GxhZQFPZfPIw/8BxMsg47Yu3RGCPKJL28Y0qwUQR+DyBrr03\nXwtDLQlnlnBKJRtQEFcCAwEAAaOBsTCBrjAJBgNVHRMEAjAAMA4GA1UdDwEB/wQE\nAwIFoDATBgNVHSUEDDAKBggrBgEFBQcDATAdBgNVHQ4EFgQU57+N4Dbz0gMxXTKM\nLYC0ua/zUgAwHwYDVR0jBBgwFoAUIVEMFAjHJXc6rpaNnu4jGPgjZ3QwPAYDVR0R\nBDUwM4IJbG9jYWxob3N0gg4qLmV4YW1wbGUudGVzdIcEfwAAAYcQAAAAAAAAAAAA\nAAAAAAAAATANBgkqhkiG9w0BAQsFAAOCAQEAhdGS4FpXpNBQaEBVx7r6KhxBz9Ar\nc82rCq/LVHWArQHFvCqEmpq2gbRbr1fAZYQgmCJkZH3geSq7IlEKSSt71DnoQgjo\nDtPrH5Z6iSnfJfmXk4mKXVeKPs5gpe4ja94TSWVe4Xwg4+qMXLqXuyRmBQEX3msS\ntyPZIsj0J5cn4Ja4Mc5qrqNjfojP+BgBUNX5uxAmmKgt9lKbrkvbTDIaUAXKjpY4\n7kkVpf0caYdcqXb1iByaPXTZNhZn3GoiIPKWemPtB2ec3UqMA2lG0FSrTVNmXnSb\ntvZ+fWxVje29R/dNxyjPU1/zca0LFvnzSi19SXmXSkZ9BjPsT9xL/EkFmw==\n-----END CERTIFICATE-----\n",
        ],
        &[
            b"Subject:C = FR, O = Goro Test, CN = Goro Test Intermediate",
            b"Issuer:C = FR, O = Goro Test, CN = Goro Test Root CA",
            b"Version:2",
            b"Serial Number:2002",
            b"Signature Algorithm:sha256WithRSAEncryption",
            b"Public Key Algorithm:rsaEncryption",
            b"X509v3 Basic Constraints:CA:TRUE, pathlen:0",
            b"X509v3 Key Usage:Certificate Sign, CRL Sign",
            b"X509v3 Subject Key Identifier:21:51:0C:14:08:C7:25:77:3A:AE:96:8D:9E:EE:23:18:F8:23:67:74",
            b"X509v3 Authority Key Identifier:79:DB:50:A8:5C:C5:19:97:63:1A:BB:5B:15:E2:EC:35:86:77:05:C6",
            b"Start date:Jan  1 00:00:00 2025 GMT",
            b"Expire date:Jan  1 00:00:00 2125 GMT",
            b"RSA Public Key:2048",
            b"rsa(n):A87BA3F0AC81DA3E238A80BC2A2B51CEE713CCA0310579E523C540C943615431769255E9F4024E6F4234418E7B2883FB3C3470CD3DBBE60DDB5553CD670D5E91235AC365479D93909868B7B41DC1DBED37B64CB3E1592559D83AC021B6A74DB83C93BD884E3EC9EA77B4057C362A71172A0453D066006F86F396DD4A3FCE1F81486778E8D29A40F443182CBCEBDF1432EBCD62E53CFBE1CA4A648AEA9479581FA6CF7EE5C59E773D5B93A6793A31F957B3BAE6E4BDEC417718385887B5A6EDFEE3EED40F3ADBEDB0D15644828D1AD2EFB63AB462EA2CD1748F6D73EE5C9190D54D5A3CE945F0780AACFE07AE77AAF71F71F358240EEACACCCCF983ED1A0D575F",
            b"rsa(e):10001",
            b"Signature:18:6c:f4:6e:2d:14:74:0c:7a:79:bb:36:fb:ab:eb:1f:dd:64:32:17:05:42:94:03:63:1e:32:14:44:35:6b:9b:65:4a:45:78:4b:82:8c:52:d3:76:03:dc:62:e7:8e:7d:15:88:6a:d6:67:7a:db:6e:70:94:d7:93:ba:f3:98:5f:6f:16:4d:98:03:e5:4c:bd:d1:4b:85:87:0c:46:9e:ca:9f:31:c8:87:0f:cc:f2:72:2a:ed:b8:bb:49:0a:99:ab:f5:94:e5:b7:4a:8c:96:1f:60:7b:d7:df:a1:28:e0:a9:ae:da:44:78:94:5f:98:fa:a2:ba:ce:f8:37:c1:be:1b:30:1a:db:d6:79:9b:8f:41:9f:56:71:2f:2c:61:de:ae:b5:54:66:61:c7:72:8f:ed:fe:4e:88:5d:fa:92:f5:b7:9a:7f:6e:56:df:e6:aa:54:a6:9f:0a:f2:9d:04:18:7a:ef:d2:20:1b:91:a6:65:ed:01:03:f1:56:26:58:ff:5f:60:9d:2e:dc:ba:35:25:f3:0c:83:8d:4f:ed:f1:c4:29:38:65:b9:01:e7:e8:42:9a:06:2c:19:a9:b3:0b:5c:f3:e7:f0:fd:68:71:da:52:4b:20:a6:9a:05:55:be:62:ab:5c:d8:41:de:8e:5c:8c:32:b8:1b:da:7f:b2:ac:bc:71:",
            b"Cert:-----BEGIN CERTIFICATE-----\nMIIDYzCCAkugAwIBAgICIAIwDQYJKoZIhvcNAQELBQAwPTELMAkGA1UEBhMCRlIx\nEjAQBgNVBAoMCUdvcm8gVGVzdDEaMBgGA1UEAwwRR29ybyBUZXN0IFJvb3QgQ0Ew\nIBcNMjUwMTAxMDAwMDAwWhgPMjEyNTAxMDEwMDAwMDBaMEIxCzAJBgNVBAYTAkZS\nMRIwEAYDVQQKDAlHb3JvIFRlc3QxHzAdBgNVBAMMFkdvcm8gVGVzdCBJbnRlcm1l\nZGlhdGUwggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQCoe6PwrIHaPiOK\ngLwqK1HO5xPMoDEFeeUjxUDJQ2FUMXaSVen0Ak5vQjRBjnsog/s8NHDNPbvmDdtV\nU81nDV6RI1rDZUedk5CYaLe0HcHb7Te2TLPhWSVZ2DrAIbanTbg8k72ITj7J6ne0\nBXw2KnEXKgRT0GYAb4bzlt1KP84fgUhneOjSmkD0QxgsvOvfFDLrzWLlPPvhykpk\niuqUeVgfps9+5cWedz1bk6Z5OjH5V7O65uS97EF3GDhYh7Wm7f7j7tQPOtvtsNFW\nRIKNGtLvtjq0Yuos0XSPbXPuXJGQ1U1aPOlF8HgKrP4Hrneq9x9x81gkDurKzMz5\ng+0aDVdfAgMBAAGjZjBkMBIGA1UdEwEB/wQIMAYBAf8CAQAwDgYDVR0PAQH/BAQD\nAgEGMB0GA1UdDgQWBBQhUQwUCMcldzqulo2e7iMY+CNndDAfBgNVHSMEGDAWgBR5\n21CoXMUZl2Mau1sV4uw1hncFxjANBgkqhkiG9w0BAQsFAAOCAQEAGGz0bi0UdAx6\nebs2+6vrH91kMhcFQpQDYx4yFEQ1a5tlSkV4S4KMUtN2A9xi5459FYhq1md6225w\nlNeTuvOYX28WTZgD5Uy90UuFhwxGnsqfMciHD8zycirtuLtJCpmr9ZTlt0qMlh9g\ne9ffoSjgqa7aRHiUX5j6orrO+DfBvhswGtvWeZuPQZ9WcS8sYd6utVRmYcdyj+3+\nTohd+pL1t5p/blbf5qpUpp8K8p0EGHrv0iAbkaZl7QED8VYmWP9fYJ0u3Lo1JfMM\ng41P7fHEKThluQHn6EKaBiwZqbMLXPPn8P1ocdpSSyCmmgVVvmKrXNhB3o5cjDK4\nG9p/sqy8cQ==\n-----END CERTIFICATE-----\n",
        ],
    ];
    fn ders(pem: &str) -> Vec<Vec<u8>> {
        pem_blocks(pem.as_bytes())
            .into_iter()
            .filter(|b| is_cert_label(&b.label))
            .map(|b| b.der)
            .collect()
    }

    fn tmpdir() -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!(
            "curl-tls-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(dir: &Path, name: &str, data: &[u8]) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, data).unwrap();
        p
    }

    struct Srv<'a> {
        chain: Vec<&'a str>,
        key: &'a str,
        client_ca: Option<&'a str>,
        require_client: bool,
        max12: bool,
        alpn: Vec<&'a str>,
        tickets: bool,
    }

    impl<'a> Srv<'a> {
        fn new(chain: &[&'a str], key: &'a str) -> Srv<'a> {
            Srv {
                chain: chain.to_vec(),
                key,
                client_ca: None,
                require_client: false,
                max12: false,
                alpn: Vec::new(),
                tickets: false,
            }
        }

        fn config(&self) -> Config {
            let key = key_from_pem(self.key.as_bytes(), None)
                .and_then(signing_key)
                .unwrap();
            let chain: Vec<Vec<u8>> = self.chain.iter().flat_map(|p| ders(p)).collect();
            let mut b = Config::builder()
                .rng(Arc::new(purecrypto::rng::OsRng))
                .identity(chain, key)
                .require_extended_master_secret(false)
                .cipher_suites(&[
                    0x1302, 0x1303, 0x1301, 0xC030, 0xC02C, 0xC02F, 0xC02B, 0xCCA8, 0xCCA9,
                ]);
            if let Some(ca) = self.client_ca {
                let mut roots = RootCertStore::new();
                for d in ders(ca) {
                    roots.add_der(d).unwrap();
                }
                b = b.client_auth(ClientAuth::new(roots, self.require_client));
            }
            if self.max12 {
                b = b.versions(ProtocolVersion::TLSv1_2, ProtocolVersion::TLSv1_2);
            }
            if !self.alpn.is_empty() {
                b = b.alpn(self.alpn.iter().map(|a| a.as_bytes().to_vec()).collect());
            }
            if self.tickets {
                b = b.ticket_key([7u8; 32]);
            }
            b.build()
        }

        fn conn(&self) -> Connection {
            Connection::server(&self.config()).unwrap()
        }
    }

    /// Move bytes between the client and an in-process server until the
    /// client handshake finishes.
    fn pump(cli: &mut TlsSession, srv: &mut Connection) -> Result<bool, TlsErr> {
        for _ in 0..64 {
            let out = cli.take_outgoing();
            if !out.is_empty() {
                let _ = srv.feed(&out);
            }
            loop {
                match srv.pop() {
                    Ok(b) if !b.is_empty() => cli.feed(&b).unwrap(),
                    _ => break,
                }
            }
            match cli.handshake() {
                Ok(true) => {
                    let out = cli.take_outgoing();
                    if !out.is_empty() {
                        let _ = srv.feed(&out);
                    }
                    return Ok(true);
                }
                Ok(false) => {}
                Err(e) => {
                    let out = cli.take_outgoing();
                    if !out.is_empty() {
                        let _ = srv.feed(&out);
                    }
                    return Err(e);
                }
            }
        }
        Ok(false)
    }

    fn base_cfg(dir: &Path) -> TlsConfig {
        TlsConfig {
            cainfo: Some(write(dir, "ca.pem", CA_PEM.as_bytes())),
            capath: None,
            alpn: vec!["http/1.1".into()],
            ..TlsConfig::default()
        }
    }

    fn connect(
        cfg: &TlsConfig,
        host: &str,
        srv: &Srv,
    ) -> (TlsSession, Result<bool, TlsErr>, Vec<String>) {
        let mut cli = TlsSession::new(cfg, host, 443).unwrap();
        let mut s = srv.conn();
        let r = pump(&mut cli, &mut s);
        let lines = cli.take_info_lines();
        (cli, r, lines)
    }

    fn info_after_alpn(lines: &[String]) -> Vec<String> {
        lines.to_vec()
    }

    const LEVELS3: [&str; 3] = [
        "  Certificate level 0: Public key type RSA (2048/112 Bits/secBits), signed using sha256WithRSAEncryption",
        "  Certificate level 1: Public key type RSA (2048/112 Bits/secBits), signed using sha256WithRSAEncryption",
        "  Certificate level 2: Public key type RSA (2048/112 Bits/secBits), signed using sha256WithRSAEncryption",
    ];

    #[test]
    fn verify_ok_chain() {
        let dir = tmpdir();
        let cfg = base_cfg(&dir);
        let (cli, r, lines) = connect(&cfg, "localhost", &Srv::new(&[LEAF_PEM, INT_PEM], LEAF_KEY));
        assert_eq!(r, Ok(true));
        let ca = cfg.cainfo.as_ref().unwrap().display().to_string();
        let mut expected = vec![
            "ALPN: curl offers http/1.1".to_string(),
            format!(" CAfile: {}", ca),
            " CApath: none".to_string(),
            "SSL connection using TLSv1.3 / TLS_AES_256_GCM_SHA384 / X25519MLKEM768 / RSASSA-PSS"
                .to_string(),
            "ALPN: server did not agree on a protocol. Uses default.".to_string(),
            "Server certificate:".to_string(),
            " subject: C=FR; ST=Paris; O=Goro Test; CN=localhost".to_string(),
            " start date: Jan  1 00:00:00 2025 GMT".to_string(),
            " expire date: Jan  1 00:00:00 2125 GMT".to_string(),
            " subjectAltName: host \"localhost\" matched cert's \"localhost\"".to_string(),
            " issuer: C=FR; O=Goro Test; CN=Goro Test Intermediate".to_string(),
            " SSL certificate verify ok.".to_string(),
        ];
        expected.extend(LEVELS3.iter().map(|s| s.to_string()));
        assert_eq!(info_after_alpn(&lines), expected);
        assert_eq!(cli.verify_result(), 0);
    }

    fn expect_err(
        cfg: &TlsConfig,
        host: &str,
        srv: &Srv,
        code: u32,
        msg: &str,
    ) -> (TlsSession, Vec<String>) {
        let (cli, r, lines) = connect(cfg, host, srv);
        assert_eq!(r, Err((code, msg.to_string())), "lines: {:#?}", lines);
        (cli, lines)
    }

    fn chain_srv() -> Srv<'static> {
        Srv::new(&[LEAF_PEM, INT_PEM], LEAF_KEY)
    }

    #[test]
    fn subject_hash_matches_openssl() {
        let h = |p: &str| X509::parse(&ders(p)[0]).unwrap().subject.hash();
        assert_eq!(h(CA_PEM), 0xe49e1e5f);
        assert_eq!(h(INT_PEM), 0x22ff05e8);
        assert_eq!(h(KS_PEM), 0x502cdf01);
        assert_eq!(h(LEAF_PEM), 0xd4c9f9f6);
    }

    #[test]
    fn self_signed() {
        let dir = tmpdir();
        let (cli, _) = expect_err(
            &base_cfg(&dir),
            "localhost",
            &Srv::new(&[SELF_PEM], SELF_KEY),
            60,
            "SSL certificate problem: self-signed certificate",
        );
        assert_eq!(cli.verify_result(), 18);
    }

    #[test]
    fn self_signed_no_verify_peer_with_certinfo() {
        let dir = tmpdir();
        let cfg = TlsConfig {
            verify_peer: false,
            certinfo: true,
            ..base_cfg(&dir)
        };
        let (cli, r, lines) = connect(&cfg, "localhost", &Srv::new(&[SELF_PEM], SELF_KEY));
        assert_eq!(r, Ok(true));
        assert!(!lines.iter().any(|l| l.contains("CAfile")));
        assert!(
            lines.contains(
                &" SSL certificate verify result: self-signed certificate (18), continuing anyway."
                    .to_string()
            )
        );
        assert!(lines.contains(&" issuer: CN=localhost".to_string()));
        assert_eq!(
            lines.last().unwrap(),
            "  Certificate level 0: Public key type RSA (2048/112 Bits/secBits), signed using sha256WithRSAEncryption"
        );
        assert_eq!(cli.verify_result(), 18);
        assert_eq!(cli.certinfo().len(), 1);
    }

    #[test]
    fn missing_intermediate() {
        let dir = tmpdir();
        let (cli, _) = expect_err(
            &base_cfg(&dir),
            "localhost",
            &Srv::new(&[LEAF_PEM], LEAF_KEY),
            60,
            "SSL certificate problem: unable to get local issuer certificate",
        );
        assert_eq!(cli.verify_result(), 20);
    }

    #[test]
    fn intermediate_as_trust_anchor() {
        let dir = tmpdir();
        let cfg = TlsConfig {
            cainfo: Some(write(&dir, "int.pem", INT_PEM.as_bytes())),
            ..base_cfg(&dir)
        };
        let (cli, r, _) = connect(&cfg, "localhost", &Srv::new(&[LEAF_PEM], LEAF_KEY));
        assert_eq!(r, Ok(true));
        assert_eq!(cli.verify_result(), 0);
        // no partial chains: the root is needed
        let cfg = TlsConfig {
            ssl_options: 4,
            ..cfg
        };
        let (cli, _) = expect_err(
            &cfg,
            "localhost",
            &Srv::new(&[LEAF_PEM], LEAF_KEY),
            60,
            "SSL certificate problem: unable to get issuer certificate",
        );
        assert_eq!(cli.verify_result(), 2);
    }

    #[test]
    fn expired() {
        let dir = tmpdir();
        let (cli, _) = expect_err(
            &base_cfg(&dir),
            "localhost",
            &Srv::new(&[EXPIRED_PEM], LEAF_KEY),
            60,
            "SSL certificate problem: certificate has expired",
        );
        assert_eq!(cli.verify_result(), 10);
        let cfg = TlsConfig {
            verify_peer: false,
            ..base_cfg(&dir)
        };
        let (cli, r, lines) = connect(&cfg, "localhost", &Srv::new(&[EXPIRED_PEM], LEAF_KEY));
        assert_eq!(r, Ok(true));
        assert!(lines.contains(&" start date: Jan  1 00:00:00 2020 GMT".to_string()));
        assert!(lines.contains(&" expire date: Jan  1 00:00:00 2021 GMT".to_string()));
        assert!(lines.contains(&" SSL certificate verify result: unable to get local issuer certificate (20), continuing anyway.".to_string()));
        assert_eq!(cli.verify_result(), 20);
    }

    #[test]
    fn wrong_host() {
        let dir = tmpdir();
        let srv = Srv::new(&[WRONG_PEM], LEAF_KEY);
        let (cli, lines) = expect_err(
            &base_cfg(&dir),
            "localhost",
            &srv,
            60,
            "SSL: no alternative certificate subject name matches target hostname 'localhost'",
        );
        assert_eq!(cli.verify_result(), 1);
        assert!(lines.contains(&" subject: CN=other.example".to_string()));
        assert!(lines.ends_with(&[
            " expire date: Jan  1 00:00:00 2125 GMT".to_string(),
            " subjectAltName does not match hostname localhost".to_string()
        ]));
        // verify_host off
        let cfg = TlsConfig {
            verify_host: false,
            ..base_cfg(&dir)
        };
        let (cli, r, lines) = connect(&cfg, "localhost", &srv);
        assert_eq!(r, Ok(true));
        assert!(lines.contains(&" issuer: C=FR; O=Goro Test; CN=Goro Test Root CA".to_string()));
        assert_eq!(cli.verify_result(), 0);
        // verify_peer off still checks the name
        let cfg = TlsConfig {
            verify_peer: false,
            ..base_cfg(&dir)
        };
        expect_err(
            &cfg,
            "localhost",
            &srv,
            60,
            "SSL: no alternative certificate subject name matches target hostname 'localhost'",
        );
        expect_err(
            &base_cfg(&dir),
            "127.0.0.1",
            &srv,
            60,
            "SSL: no alternative certificate subject name matches target ipv4 address '127.0.0.1'",
        );
    }

    #[test]
    fn common_name_only() {
        let dir = tmpdir();
        let srv = Srv::new(&[CNONLY_PEM], LEAF_KEY);
        let (_, r, lines) = connect(&base_cfg(&dir), "localhost", &srv);
        assert_eq!(r, Ok(true));
        assert!(lines.contains(&" subject: O=Goro Test; CN=localhost".to_string()));
        assert!(lines.contains(&" common name: localhost (matched)".to_string()));
        let (cli, _) = expect_err(
            &base_cfg(&dir),
            "127.0.0.1",
            &srv,
            60,
            "SSL: certificate subject name 'localhost' does not match target hostname '127.0.0.1'",
        );
        assert_eq!(cli.verify_result(), 1);
    }

    #[test]
    fn ip_and_wildcard_names() {
        let dir = tmpdir();
        let cfg = base_cfg(&dir);
        let (_, r, lines) = connect(&cfg, "127.0.0.1", &chain_srv());
        assert_eq!(r, Ok(true));
        assert!(lines.contains(
            &" subjectAltName: host \"127.0.0.1\" matched cert's IP address!".to_string()
        ));
        let (_, r, lines) = connect(&cfg, "[::1]", &chain_srv());
        assert_eq!(r, Ok(true));
        assert!(
            lines.contains(&" subjectAltName: host \"::1\" matched cert's IP address!".to_string())
        );
        let (_, r, lines) = connect(&cfg, "foo.example.test", &chain_srv());
        assert_eq!(r, Ok(true));
        assert!(
            lines.contains(
                &" subjectAltName: host \"foo.example.test\" matched cert's \"*.example.test\""
                    .to_string()
            )
        );
        expect_err(
            &cfg,
            "a.b.example.test",
            &chain_srv(),
            60,
            "SSL: no alternative certificate subject name matches target hostname 'a.b.example.test'",
        );
    }

    #[test]
    fn hostcheck_rules() {
        assert!(cert_hostcheck(b"*.example.com", b"www.example.com"));
        assert!(cert_hostcheck(b"*.example.com.", b"WWW.example.com"));
        assert!(!cert_hostcheck(b"*.example.com", b"example.com"));
        assert!(!cert_hostcheck(b"*.com", b"example.com"));
        assert!(cert_hostcheck(b"*.com", b"*.com"));
        assert!(!cert_hostcheck(b"f*.example.com", b"foo.example.com"));
        assert!(!cert_hostcheck(b"*.0.0.1", b"127.0.0.1"));
        assert!(cert_hostcheck(b"localhost", b"localhost."));
    }

    #[test]
    fn ec_certificate_and_certinfo() {
        let dir = tmpdir();
        let cfg = TlsConfig {
            certinfo: true,
            ..base_cfg(&dir)
        };
        let (cli, r, lines) = connect(&cfg, "localhost", &Srv::new(&[EC_PEM], EC_KEY));
        assert_eq!(r, Ok(true));
        assert_eq!(
            lines[3],
            "SSL connection using TLSv1.3 / TLS_AES_256_GCM_SHA384 / X25519MLKEM768 / id-ecPublicKey"
        );
        assert!(lines.ends_with(&[
            "  Certificate level 0: Public key type EC/prime256v1 (256/128 Bits/secBits), signed using sha256WithRSAEncryption".to_string(),
            "  Certificate level 1: Public key type RSA (2048/112 Bits/secBits), signed using sha256WithRSAEncryption".to_string(),
        ]));
        let ci = cli.certinfo();
        let exp: Vec<Vec<Vec<u8>>> = CERTINFO_EC
            .iter()
            .map(|c| c.iter().map(|e| e.to_vec()).collect())
            .collect();
        assert_eq!(ci, exp);
    }

    #[test]
    fn certinfo_chain_matches_libcurl() {
        let dir = tmpdir();
        let cfg = TlsConfig {
            certinfo: true,
            ..base_cfg(&dir)
        };
        let (cli, r, _) = connect(&cfg, "localhost", &chain_srv());
        assert_eq!(r, Ok(true));
        let exp: Vec<Vec<Vec<u8>>> = CERTINFO_CHAIN
            .iter()
            .map(|c| c.iter().map(|e| e.to_vec()).collect())
            .collect();
        let ci = cli.certinfo();
        for (a, b) in ci.iter().zip(exp.iter()) {
            for (x, y) in a.iter().zip(b.iter()) {
                assert_eq!(String::from_utf8_lossy(x), String::from_utf8_lossy(y));
            }
        }
        assert_eq!(ci, exp);
    }

    #[test]
    fn certinfo_kitchen_sink_matches_libcurl() {
        let x = X509::parse(&ders(KS_PEM)[0]).unwrap();
        let got = cert_info_entries(&x);
        let exp: Vec<Vec<u8>> = CERTINFO_KS[0].iter().map(|e| e.to_vec()).collect();
        for (a, b) in got.iter().zip(exp.iter()) {
            assert_eq!(String::from_utf8_lossy(a), String::from_utf8_lossy(b));
        }
        assert_eq!(got, exp);
        let mut out = Vec::new();
        out.extend(name_print(&x.subject, NameFmt::SplusSpc));
        assert_eq!(
            out,
            b"C=FR; ST=\xcele-de-France; L=Paris, 75000; O=Goro \"Test\" ; OU=Unit 2; CN=localhost; emailAddress=admin@example.com; serialNumber=12345; DC=example; UID=uid1".to_vec()
        );
    }

    #[test]
    fn ca_file_errors() {
        let dir = tmpdir();
        let cfg = TlsConfig {
            cainfo: Some(PathBuf::from("/nonexistent/ca.pem")),
            ..base_cfg(&dir)
        };
        let mut cli = TlsSession::new(&cfg, "localhost", 443).unwrap();
        assert!(!cli.take_outgoing().is_empty());
        assert_eq!(
            cli.handshake(),
            Err((
                77,
                "error setting certificate file: /nonexistent/ca.pem".to_string()
            ))
        );
        assert_eq!(cli.verify_result(), 1);
        // a file without certificates
        let cfg = TlsConfig {
            cainfo: Some(write(&dir, "notcert.pem", CLIENT_KEY.as_bytes())),
            ..base_cfg(&dir)
        };
        let mut cli = TlsSession::new(&cfg, "localhost", 443).unwrap();
        let p = cfg.cainfo.as_ref().unwrap().display().to_string();
        assert_eq!(
            cli.handshake(),
            Err((77, format!("error setting certificate file: {}", p)))
        );
        // verify_peer off: not even loaded
        let cfg = TlsConfig {
            cainfo: Some(PathBuf::from("/nonexistent/ca.pem")),
            verify_peer: false,
            ..base_cfg(&dir)
        };
        let (cli, r, lines) = connect(&cfg, "localhost", &chain_srv());
        assert_eq!(r, Ok(true));
        assert!(lines.contains(&" SSL certificate verify result: unable to get local issuer certificate (20), continuing anyway.".to_string()));
        assert_eq!(cli.verify_result(), 20);
        // no CA at all
        let cfg = TlsConfig {
            cainfo: None,
            capath: None,
            ..base_cfg(&dir)
        };
        let (_, lines) = expect_err(
            &cfg,
            "localhost",
            &chain_srv(),
            60,
            "SSL certificate problem: unable to get local issuer certificate",
        );
        assert!(!lines.iter().any(|l| l.contains("CAfile")));
    }

    #[test]
    fn ca_path_and_blob() {
        let dir = tmpdir();
        let hdir = dir.join("hashed");
        std::fs::create_dir_all(&hdir).unwrap();
        write(&hdir, "e49e1e5f.0", CA_PEM.as_bytes());
        let cfg = TlsConfig {
            cainfo: None,
            capath: Some(hdir.clone()),
            ..base_cfg(&dir)
        };
        let (_, r, lines) = connect(&cfg, "localhost", &chain_srv());
        assert_eq!(r, Ok(true));
        assert_eq!(lines[1], " CAfile: none");
        assert_eq!(lines[2], format!(" CApath: {}", hdir.display()));
        // a missing CApath is not an error
        let cfg = TlsConfig {
            capath: Some(PathBuf::from("/nonexistent/dir")),
            ..base_cfg(&dir)
        };
        let (_, r, lines) = connect(&cfg, "localhost", &chain_srv());
        assert_eq!(r, Ok(true));
        assert_eq!(lines[2], " CApath: /nonexistent/dir");
        // blob
        let cfg = TlsConfig {
            ca_blob: Some(CA_PEM.as_bytes().to_vec()),
            cainfo: Some(PathBuf::from("/ignored")),
            capath: None,
            ..base_cfg(&dir)
        };
        let (_, r, lines) = connect(&cfg, "localhost", &chain_srv());
        assert_eq!(r, Ok(true));
        assert_eq!(lines[1], "successfully imported CA certificate blob");
        // the blob replaces CURLOPT_CAINFO, no CAfile/CApath lines
        assert_eq!(lines.len(), 14);
        let cfg = TlsConfig {
            ca_blob: Some(b"garbage".to_vec()),
            ..base_cfg(&dir)
        };
        let mut cli = TlsSession::new(&cfg, "localhost", 443).unwrap();
        assert_eq!(
            cli.handshake(),
            Err((77, "error importing CA certificate blob".to_string()))
        );
    }

    #[test]
    fn tls12() {
        let dir = tmpdir();
        let mut srv = chain_srv();
        srv.max12 = true;
        let (cli, r, lines) = connect(&base_cfg(&dir), "localhost", &srv);
        assert_eq!(r, Ok(true));
        // (purecrypto's TLS 1.2 server picks in its own order; OpenSSL's
        // s_server took the client's first, ECDHE-RSA-AES256-GCM-SHA384)
        assert!(
            lines[3].starts_with("SSL connection using TLSv1.2 / ECDHE-RSA-AES"),
            "{}",
            lines[3]
        );
        assert!(lines[3].ends_with(" / x25519 / RSASSA-PSS"), "{}", lines[3]);
        assert_eq!(cli.verify_result(), 0);
        // client max 1.2 against a 1.3 server
        let cfg = TlsConfig {
            version_max: 6 << 16,
            ..base_cfg(&dir)
        };
        let (_, r, lines) = connect(&cfg, "localhost", &chain_srv());
        assert_eq!(r, Ok(true));
        assert!(lines[3].starts_with("SSL connection using TLSv1.2 / "));
    }

    #[test]
    fn version_errors() {
        let dir = tmpdir();
        let cfg = TlsConfig {
            version_min: 4,
            version_max: 5 << 16,
            ..base_cfg(&dir)
        };
        let mut cli = TlsSession::new(&cfg, "localhost", 443).unwrap();
        assert_eq!(
            cli.handshake(),
            Err((
                35,
                "TLS connect error: error:0A0000BF:SSL routines::no protocols available"
                    .to_string()
            ))
        );
        let cfg = TlsConfig {
            version_min: 3,
            ..base_cfg(&dir)
        };
        assert_eq!(
            TlsSession::new(&cfg, "localhost", 443).err(),
            Some((4, "No SSLv3 support".to_string()))
        );
        let cfg = TlsConfig {
            version_min: 7,
            version_max: 6 << 16,
            ..base_cfg(&dir)
        };
        assert_eq!(
            TlsSession::new(&cfg, "localhost", 443).err(),
            Some((
                35,
                "CURL_SSLVERSION_MAX incompatible with CURL_SSLVERSION".to_string()
            ))
        );
    }

    #[test]
    fn tls13_only_client_vs_tls12_server() {
        let dir = tmpdir();
        let mut srv = chain_srv();
        srv.max12 = true;
        let cfg = TlsConfig {
            version_min: 7,
            ..base_cfg(&dir)
        };
        let (_, r, _) = connect(&cfg, "localhost", &srv);
        let e = r.unwrap_err();
        assert_eq!(e.0, 35);
        assert!(e.1.starts_with("TLS connect error: error:0A0"), "{}", e.1);
    }

    #[test]
    fn cipher_and_group_options() {
        let dir = tmpdir();
        let bad = |cfg: TlsConfig, msg: &str| {
            assert_eq!(
                TlsSession::new(&cfg, "localhost", 443).err(),
                Some((59, msg.to_string()))
            );
        };
        bad(
            TlsConfig {
                cipher_list: Some("FOOBAR".into()),
                ..base_cfg(&dir)
            },
            "failed setting cipher list: FOOBAR",
        );
        bad(
            TlsConfig {
                tls13_ciphers: Some("FOO".into()),
                ..base_cfg(&dir)
            },
            "failed setting TLS 1.3 cipher suite: FOO",
        );
        bad(
            TlsConfig {
                curves: Some("X25519:nope".into()),
                ..base_cfg(&dir)
            },
            "failed setting curves list: 'X25519:nope'",
        );
        bad(
            TlsConfig {
                signature_algorithms: Some("nope".into()),
                ..base_cfg(&dir)
            },
            "failed setting signature algorithms: 'nope'",
        );
        // TLS 1.2 cipher list
        let cfg = TlsConfig {
            cipher_list: Some("ECDHE-RSA-AES128-GCM-SHA256".into()),
            version_max: 6 << 16,
            ..base_cfg(&dir)
        };
        let (_, r, lines) = connect(&cfg, "localhost", &chain_srv());
        assert_eq!(r, Ok(true));
        assert_eq!(lines[0], "Cipher selection: ECDHE-RSA-AES128-GCM-SHA256");
        assert!(
            lines[4].starts_with("SSL connection using TLSv1.2 / ECDHE-RSA-AES128-GCM-SHA256 / ")
        );
        // TLS 1.3 suites
        let cfg = TlsConfig {
            tls13_ciphers: Some("TLS_CHACHA20_POLY1305_SHA256:FOO".into()),
            ..base_cfg(&dir)
        };
        let (_, r, lines) = connect(&cfg, "localhost", &chain_srv());
        assert_eq!(r, Ok(true));
        assert_eq!(
            lines[0],
            "TLS 1.3 cipher selection: TLS_CHACHA20_POLY1305_SHA256:FOO"
        );
        assert_eq!(
            lines[4],
            "SSL connection using TLSv1.3 / TLS_CHACHA20_POLY1305_SHA256 / X25519MLKEM768 / RSASSA-PSS"
        );
        // groups
        let cfg = TlsConfig {
            curves: Some("P-256".into()),
            ..base_cfg(&dir)
        };
        let (_, r, lines) = connect(&cfg, "localhost", &chain_srv());
        assert_eq!(r, Ok(true));
        assert_eq!(
            lines[3],
            "SSL connection using TLSv1.3 / TLS_AES_256_GCM_SHA384 / secp256r1 / RSASSA-PSS"
        );
        // a cipher list ignored with a TLS 1.3 minimum
        let cfg = TlsConfig {
            cipher_list: Some("FOO".into()),
            version_min: 7,
            ..base_cfg(&dir)
        };
        let (_, r, lines) = connect(&cfg, "localhost", &chain_srv());
        assert_eq!(r, Ok(true));
        assert!(!lines[0].starts_with("Cipher selection"));
        assert!(parse_cipher_list("ALL:!aNULL:@STRENGTH").is_some());
        assert_eq!(
            parse_cipher_list("ECDHE+AESGCM+aRSA"),
            Some(vec![0xC030, 0xC02F])
        );
        assert_eq!(parse_cipher_list("AES128-SHA"), Some(vec![]));
        assert_eq!(parse_cipher_list("!ALL"), None);
        assert!(valid_sigalgs("RSA-PSS+SHA256:ECDSA+SHA256:ed25519"));
    }

    #[test]
    fn pinned_public_key() {
        let dir = tmpdir();
        let leaf = X509::parse(&ders(LEAF_PEM)[0]).unwrap();
        let pem_path = write(
            &dir,
            "pub.pem",
            pem_encode("PUBLIC KEY", &leaf.spki).as_bytes(),
        );
        let der_path = write(&dir, "pub.der", LEAF_PUB_DER);
        for pin in [
            pem_path.display().to_string(),
            der_path.display().to_string(),
            "sha256//AAAA;sha256///pQnMKZtvwZhSQ3F3PkoOxkPH5Wy+JVNJpxHmokmRHI=".to_string(),
        ] {
            let cfg = TlsConfig {
                pinned_pubkey: Some(pin),
                ..base_cfg(&dir)
            };
            let (_, r, _) = connect(&cfg, "localhost", &chain_srv());
            assert_eq!(r, Ok(true));
        }
        let cfg = TlsConfig {
            pinned_pubkey: Some("sha256//AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into()),
            ..base_cfg(&dir)
        };
        let (cli, lines) = expect_err(
            &cfg,
            "localhost",
            &chain_srv(),
            90,
            "SSL: public key does not match pinned public key",
        );
        assert_eq!(
            lines.last().unwrap(),
            " public key hash: sha256///pQnMKZtvwZhSQ3F3PkoOxkPH5Wy+JVNJpxHmokmRHI="
        );
        assert_eq!(cli.verify_result(), 0);
        let cfg = TlsConfig {
            pinned_pubkey: Some("/nonexistent".into()),
            ..base_cfg(&dir)
        };
        expect_err(
            &cfg,
            "localhost",
            &chain_srv(),
            90,
            "SSL: public key does not match pinned public key",
        );
    }

    fn client_cfg(dir: &Path) -> TlsConfig {
        base_cfg(dir)
    }

    fn mtls_srv() -> Srv<'static> {
        let mut s = chain_srv();
        s.client_ca = Some(CA_PEM);
        s.require_client = true;
        s
    }

    #[test]
    fn client_certificate_errors() {
        let dir = tmpdir();
        let cpem = write(&dir, "client.pem", CLIENT_PEM.as_bytes());
        let cases: Vec<(TlsConfig, String)> = vec![
            (
                TlsConfig {
                    cert: Some(b"/nonexistent/c.pem".to_vec()),
                    ..client_cfg(&dir)
                },
                "could not load PEM client certificate from /nonexistent/c.pem, OpenSSL error error:80000002:system library::No such file or directory, (no key found, wrong pass phrase, or wrong file format?)".into(),
            ),
            (
                TlsConfig {
                    cert: Some(cpem.display().to_string().into_bytes()),
                    key: Some(PathBuf::from("/nonexistent/k.pem")),
                    ..client_cfg(&dir)
                },
                "unable to set private key file: '/nonexistent/k.pem' type PEM".into(),
            ),
            (
                TlsConfig {
                    cert: Some(cpem.display().to_string().into_bytes()),
                    key: Some(write(&dir, "leaf.key", LEAF_KEY.as_bytes())),
                    ..client_cfg(&dir)
                },
                format!("unable to set private key file: '{}/leaf.key' type PEM", dir.display()),
            ),
            (
                TlsConfig {
                    cert: Some(cpem.display().to_string().into_bytes()),
                    key: Some(write(&dir, "enc.key", CLIENT_ENC_KEY.as_bytes())),
                    ..client_cfg(&dir)
                },
                format!("unable to set private key file: '{}/enc.key' type PEM", dir.display()),
            ),
            (
                TlsConfig {
                    cert: Some(cpem.display().to_string().into_bytes()),
                    key: Some(write(&dir, "enc.key", CLIENT_ENC_KEY.as_bytes())),
                    key_passwd: Some(b"wrong".to_vec()),
                    ..client_cfg(&dir)
                },
                format!("unable to set private key file: '{}/enc.key' type PEM", dir.display()),
            ),
            (
                TlsConfig {
                    cert: Some(write(&dir, "k.pem", CLIENT_KEY.as_bytes()).display().to_string().into_bytes()),
                    ..client_cfg(&dir)
                },
                format!("could not load PEM client certificate from {}/k.pem, OpenSSL error error:0480006C:PEM routines::no start line, (no key found, wrong pass phrase, or wrong file format?)", dir.display()),
            ),
            (
                TlsConfig {
                    cert: Some(cpem.display().to_string().into_bytes()),
                    cert_type: Some("DER".into()),
                    ..client_cfg(&dir)
                },
                format!("could not load ASN1 client certificate from {}, OpenSSL error error:068000A8:asn1 encoding routines::wrong tag, (no key found, wrong pass phrase, or wrong file format?)", cpem.display()),
            ),
            (
                TlsConfig {
                    cert: Some(cpem.display().to_string().into_bytes()),
                    cert_type: Some("XYZ".into()),
                    ..client_cfg(&dir)
                },
                "not supported file type 'XYZ' for certificate".into(),
            ),
            (
                TlsConfig {
                    cert: Some(b"/nonexistent.p12".to_vec()),
                    cert_type: Some("P12".into()),
                    ..client_cfg(&dir)
                },
                "could not open PKCS12 file '/nonexistent.p12'".into(),
            ),
            (
                TlsConfig {
                    cert: Some(write(&dir, "c.p12", CLIENT_P12).display().to_string().into_bytes()),
                    cert_type: Some("P12".into()),
                    key_passwd: Some(b"nope".to_vec()),
                    ..client_cfg(&dir)
                },
                "could not parse PKCS12 file, check password, OpenSSL error error:11800071:PKCS12 routines::mac verify failure".into(),
            ),
        ];
        for (cfg, msg) in cases {
            assert_eq!(
                TlsSession::new(&cfg, "localhost", 443).err(),
                Some((58, msg))
            );
        }
    }

    #[test]
    fn client_certificate_variants() {
        let dir = tmpdir();
        let cpem = write(&dir, "client.pem", CLIENT_PEM.as_bytes())
            .display()
            .to_string()
            .into_bytes();
        let mut combined = CLIENT_PEM.to_string();
        combined.push_str(CLIENT_KEY);
        let cases: Vec<TlsConfig> = vec![
            TlsConfig {
                cert: Some(cpem.clone()),
                key: Some(write(&dir, "enc.key", CLIENT_ENC_KEY.as_bytes())),
                key_passwd: Some(b"secret".to_vec()),
                ..client_cfg(&dir)
            },
            TlsConfig {
                cert: Some(cpem.clone()),
                key: Some(write(&dir, "trad.key", CLIENT_TRAD_ENC_KEY.as_bytes())),
                key_passwd: Some(b"secret".to_vec()),
                ..client_cfg(&dir)
            },
            TlsConfig {
                cert: Some(
                    write(&dir, "comb.pem", combined.as_bytes())
                        .display()
                        .to_string()
                        .into_bytes(),
                ),
                ..client_cfg(&dir)
            },
            TlsConfig {
                cert: Some(
                    write(&dir, "c.der", CLIENT_DER)
                        .display()
                        .to_string()
                        .into_bytes(),
                ),
                cert_type: Some("DER".into()),
                key: Some(write(&dir, "k.der", CLIENT_KEY_DER)),
                key_type: Some("DER".into()),
                ..client_cfg(&dir)
            },
            TlsConfig {
                cert: Some(
                    write(&dir, "c.p12", CLIENT_P12)
                        .display()
                        .to_string()
                        .into_bytes(),
                ),
                cert_type: Some("P12".into()),
                key_passwd: Some(b"p12pass".to_vec()),
                ..client_cfg(&dir)
            },
            TlsConfig {
                cert_blob: Some(CLIENT_PEM.as_bytes().to_vec()),
                key_blob: Some(CLIENT_KEY.as_bytes().to_vec()),
                ..client_cfg(&dir)
            },
        ];
        for cfg in cases {
            let (_, r, lines) = connect(&cfg, "localhost", &mtls_srv());
            assert_eq!(r, Ok(true), "{:?}", lines);
        }
    }

    #[test]
    fn client_certificate_required_but_missing() {
        let dir = tmpdir();
        let mut cli = TlsSession::new(&client_cfg(&dir), "localhost", 443).unwrap();
        let mut srv = mtls_srv().conn();
        let r = pump(&mut cli, &mut srv);
        assert_eq!(r, Ok(true));
        // the server's alert comes after the client's Finished
        loop {
            match srv.pop() {
                Ok(b) if !b.is_empty() => cli.feed(&b).unwrap(),
                _ => break,
            }
        }
        let e = cli.recv().unwrap_err();
        assert_eq!(
            e,
            (
                56,
                format!(
                    "OpenSSL SSL_read: {}: error:0A00045C:SSL routines::tlsv13 alert certificate required, errno 0",
                    BACKEND_VERSION
                )
            )
        );
    }

    #[test]
    fn issuer_certificate() {
        let dir = tmpdir();
        let cfg = TlsConfig {
            issuercert: Some(write(&dir, "int.pem", INT_PEM.as_bytes())),
            ..base_cfg(&dir)
        };
        let (_, r, lines) = connect(&cfg, "localhost", &chain_srv());
        assert_eq!(r, Ok(true));
        let p = cfg.issuercert.as_ref().unwrap().display().to_string();
        assert!(lines.contains(&format!(" SSL certificate issuer check ok ({})", p)));
        let cfg = TlsConfig {
            issuercert: Some(write(&dir, "ca.pem", CA_PEM.as_bytes())),
            ..base_cfg(&dir)
        };
        let p = cfg.issuercert.as_ref().unwrap().display().to_string();
        let (cli, _) = expect_err(
            &cfg,
            "localhost",
            &chain_srv(),
            83,
            &format!("SSL: Certificate issuer check failed ({})", p),
        );
        assert_eq!(cli.verify_result(), 1);
        let cfg = TlsConfig {
            issuercert: Some(PathBuf::from("/nonexistent")),
            ..base_cfg(&dir)
        };
        expect_err(
            &cfg,
            "localhost",
            &chain_srv(),
            83,
            "SSL: Unable to open issuer cert (/nonexistent)",
        );
    }

    #[test]
    fn crl_checks() {
        let dir = tmpdir();
        let cfg = TlsConfig {
            crlfile: Some(PathBuf::from("/nonexistent")),
            ..base_cfg(&dir)
        };
        let mut cli = TlsSession::new(&cfg, "localhost", 443).unwrap();
        assert_eq!(
            cli.handshake(),
            Err((82, "error loading CRL file: /nonexistent".to_string()))
        );
        let cfg = TlsConfig {
            crlfile: Some(write(&dir, "both.pem", CRL_BOTH.as_bytes())),
            ..base_cfg(&dir)
        };
        let (_, r, lines) = connect(&cfg, "localhost", &chain_srv());
        assert_eq!(r, Ok(true));
        assert_eq!(lines[3], "successfully loaded CRL file:");
        assert_eq!(
            lines[4],
            format!("  CRLfile: {}", cfg.crlfile.as_ref().unwrap().display())
        );
        let cfg = TlsConfig {
            crlfile: Some(write(&dir, "rev.pem", CRL_BOTH_REVOKED.as_bytes())),
            ..base_cfg(&dir)
        };
        let (cli, _) = expect_err(
            &cfg,
            "localhost",
            &chain_srv(),
            60,
            "SSL certificate problem: certificate revoked",
        );
        assert_eq!(cli.verify_result(), 23);
        let cfg = TlsConfig {
            crlfile: Some(write(&dir, "int.pem", CRL_INT.as_bytes())),
            ..base_cfg(&dir)
        };
        let (cli, _) = expect_err(
            &cfg,
            "localhost",
            &chain_srv(),
            60,
            "SSL certificate problem: unable to get certificate CRL",
        );
        assert_eq!(cli.verify_result(), 3);
    }

    #[test]
    fn status_without_staple() {
        let dir = tmpdir();
        let cfg = TlsConfig {
            verify_status: true,
            ..base_cfg(&dir)
        };
        expect_err(
            &cfg,
            "localhost",
            &chain_srv(),
            91,
            "No OCSP response received",
        );
    }

    #[test]
    fn alpn_negotiation() {
        let dir = tmpdir();
        let mut srv = chain_srv();
        srv.alpn = vec!["http/1.1"];
        let cfg = TlsConfig {
            alpn: vec!["h2".into(), "http/1.1".into()],
            ..base_cfg(&dir)
        };
        let (cli, r, lines) = connect(&cfg, "localhost", &srv);
        assert_eq!(r, Ok(true));
        assert_eq!(lines[0], "ALPN: curl offers h2,http/1.1");
        assert_eq!(lines[4], "ALPN: server accepted http/1.1");
        assert_eq!(cli.negotiated_alpn().as_deref(), Some("http/1.1"));
        let cfg = TlsConfig {
            alpn: vec![],
            ..base_cfg(&dir)
        };
        let (_, r, lines) = connect(&cfg, "localhost", &chain_srv());
        assert_eq!(r, Ok(true));
        assert!(!lines.iter().any(|l| l.starts_with("ALPN")));
    }

    #[test]
    fn data_exchange_close_and_eof() {
        let dir = tmpdir();
        let mut cli = TlsSession::new(&base_cfg(&dir), "localhost", 443).unwrap();
        let mut srv = chain_srv().conn();
        assert_eq!(pump(&mut cli, &mut srv), Ok(true));
        cli.send(b"GET / HTTP/1.1\r\n\r\n").unwrap();
        srv.feed(&cli.take_outgoing()).unwrap();
        let mut got = Vec::new();
        for _ in 0..10 {
            got.extend(srv.recv().unwrap());
        }
        assert_eq!(got, b"GET / HTTP/1.1\r\n\r\n");
        let big = vec![b'x'; 100_000];
        srv.send(&big).unwrap();
        loop {
            match srv.pop() {
                Ok(b) if !b.is_empty() => cli.feed(&b).unwrap(),
                _ => break,
            }
        }
        let mut data = Vec::new();
        data.extend(cli.recv().unwrap());
        assert_eq!(data.len(), big.len());
        assert!(!cli.peer_closed());
        srv.close().unwrap();
        cli.feed(&srv.pop().unwrap()).unwrap();
        assert_eq!(cli.recv().unwrap(), b"");
        assert!(cli.peer_closed());
        cli.close();
        assert!(!cli.take_outgoing().is_empty());
        // EOF without close_notify
        let mut cli = TlsSession::new(&base_cfg(&dir), "localhost", 443).unwrap();
        let mut srv = chain_srv().conn();
        assert_eq!(pump(&mut cli, &mut srv), Ok(true));
        cli.feed_eof();
        assert_eq!(
            cli.recv(),
            Err((
                56,
                format!(
                    "OpenSSL SSL_read: {}: error:0A000126:SSL routines::unexpected eof while reading, errno 0",
                    BACKEND_VERSION
                )
            ))
        );
        // EOF during the handshake
        let mut cli = TlsSession::new(&base_cfg(&dir), "localhost", 443).unwrap();
        cli.take_outgoing();
        cli.feed_eof();
        assert_eq!(
            cli.handshake(),
            Err((
                35,
                "TLS connect error: error:0A000126:SSL routines::unexpected eof while reading"
                    .to_string()
            ))
        );
        // not TLS at all
        let mut cli = TlsSession::new(&base_cfg(&dir), "localhost", 443).unwrap();
        cli.feed(b"HTTP/1.1 400 Bad Request\r\n\r\n").unwrap();
        assert_eq!(
            cli.handshake(),
            Err((
                35,
                "TLS connect error: error:0A00010B:SSL routines::wrong version number".to_string()
            ))
        );
        // a handshake_failure alert
        let mut cli = TlsSession::new(&base_cfg(&dir), "localhost", 443).unwrap();
        cli.feed(&[0x15, 0x03, 0x03, 0x00, 0x02, 0x02, 0x28])
            .unwrap();
        assert_eq!(
            cli.handshake(),
            Err((
                35,
                "TLS connect error: error:0A000410:SSL routines::ssl/tls alert handshake failure"
                    .to_string()
            ))
        );
    }

    #[test]
    fn session_resumption() {
        let dir = tmpdir();
        let mut srvc = chain_srv();
        srvc.tickets = true;
        let srv_cfg = srvc.config();
        let cfg = base_cfg(&dir);
        let mut cli = TlsSession::new(&cfg, "localhost", 443).unwrap();
        let mut srv = Connection::server(&srv_cfg).unwrap();
        assert_eq!(pump(&mut cli, &mut srv), Ok(true));
        loop {
            match srv.pop() {
                Ok(b) if !b.is_empty() => cli.feed(&b).unwrap(),
                _ => break,
            }
        }
        cli.recv().unwrap();
        let res = cli.take_resumption().expect("a ticket");
        let mut cli = TlsSession::new_with_resumption(&cfg, "localhost", 443, Some(&res)).unwrap();
        let mut srv = Connection::server(&srv_cfg).unwrap();
        assert_eq!(pump(&mut cli, &mut srv), Ok(true));
        let lines = cli.take_info_lines();
        assert_eq!(lines[0], "SSL reusing session with ALPN '-'");
        assert!(lines[4].ends_with(" / UNDEF"), "{:?}", lines);
        assert!(lines.contains(&" SSL certificate verify ok.".to_string()));
        assert_eq!(cli.verify_result(), 0);
    }

    fn trace_texts(log: &[TlsLog]) -> Vec<String> {
        log.iter()
            .filter_map(|l| match l {
                TlsLog::Trace(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn trace_tls13() {
        let dir = tmpdir();
        let cfg = TlsConfig {
            trace: true,
            ..base_cfg(&dir)
        };
        let mut cli = TlsSession::new(&cfg, "localhost", 443).unwrap();
        let mut srv = chain_srv().conn();
        assert_eq!(pump(&mut cli, &mut srv), Ok(true));
        let log = cli.take_log();
        let t: Vec<String> = trace_texts(&log)
            .into_iter()
            .filter(|s| !s.contains("change cipher"))
            .collect();
        assert_eq!(
            t,
            [
                "TLSv1.3 (OUT), TLS handshake, Client hello (1):",
                "TLSv1.3 (IN), TLS handshake, Server hello (2):",
                "TLSv1.3 (IN), TLS handshake, Encrypted Extensions (8):",
                "TLSv1.3 (IN), TLS handshake, Certificate (11):",
                "TLSv1.3 (IN), TLS handshake, CERT verify (15):",
                "TLSv1.3 (IN), TLS handshake, Finished (20):",
                "TLSv1.3 (OUT), TLS handshake, Finished (20):",
            ]
        );
        // record header, inner content type, then the message
        let pos = log
            .iter()
            .position(|l| {
                *l == TlsLog::Trace("TLSv1.3 (IN), TLS handshake, Encrypted Extensions (8):".into())
            })
            .unwrap();
        assert!(matches!(&log[pos - 2], TlsLog::DataIn(h) if h.len() == 5 && h[0] == 0x17));
        assert_eq!(log[pos - 1], TlsLog::DataIn(vec![0x16]));
        assert!(matches!(&log[pos + 1], TlsLog::DataIn(m) if m[0] == 8));
        // the ClientHello trace comes after "ALPN: curl offers" and before CAfile
        let ch = log
            .iter()
            .position(|l| matches!(l, TlsLog::Trace(s) if s.contains("Client hello")))
            .unwrap();
        let ca = log
            .iter()
            .position(|l| matches!(l, TlsLog::Info(s) if s.starts_with(" CAfile")))
            .unwrap();
        assert!(ch < ca);
    }

    #[test]
    fn trace_tls12() {
        let dir = tmpdir();
        let cfg = TlsConfig {
            trace: true,
            ..base_cfg(&dir)
        };
        let mut srv = chain_srv();
        srv.max12 = true;
        let mut cli = TlsSession::new(&cfg, "localhost", 443).unwrap();
        let mut s = srv.conn();
        assert_eq!(pump(&mut cli, &mut s), Ok(true));
        let t = trace_texts(&cli.take_log());
        assert_eq!(
            t,
            [
                "TLSv1.3 (OUT), TLS handshake, Client hello (1):",
                "TLSv1.3 (IN), TLS handshake, Server hello (2):",
                "TLSv1.2 (IN), TLS handshake, Certificate (11):",
                "TLSv1.2 (IN), TLS handshake, Server key exchange (12):",
                "TLSv1.2 (IN), TLS handshake, Server finished (14):",
                "TLSv1.2 (OUT), TLS handshake, Client key exchange (16):",
                "TLSv1.2 (OUT), TLS change cipher, Change cipher spec (1):",
                "TLSv1.2 (OUT), TLS handshake, Finished (20):",
                "TLSv1.2 (IN), TLS handshake, Finished (20):",
            ]
        );
    }

    #[test]
    fn trace_verify_failure_alert() {
        let dir = tmpdir();
        let cfg = TlsConfig {
            trace: true,
            ..base_cfg(&dir)
        };
        let mut cli = TlsSession::new(&cfg, "localhost", 443).unwrap();
        let mut srv = Srv::new(&[SELF_PEM], SELF_KEY).conn();
        assert!(pump(&mut cli, &mut srv).is_err());
        let log = cli.take_log();
        let t = trace_texts(&log);
        assert_eq!(
            t.last().unwrap(),
            "TLSv1.3 (OUT), TLS alert, unknown CA (560):"
        );
        assert_eq!(log.last().unwrap(), &TlsLog::DataOut(vec![2, 48]));
    }

    #[test]
    fn name_printing() {
        let x = X509::parse(&ders(KS_PEM)[0]).unwrap();
        assert_eq!(
            name_oneline_slash(&x.issuer),
            "/C=FR/O=Goro Test/CN=Goro Test Root CA"
        );
        let t = Asn1Time {
            unix: 0,
            frac: String::new(),
        };
        assert_eq!(time_print(&t), "Jan  1 00:00:00 1970 GMT");
        assert_eq!(ifc_security_bits(1024), 80);
        assert_eq!(ifc_security_bits(2048), 112);
        assert_eq!(ifc_security_bits(4096), 152);
    }

    #[test]
    fn defaults() {
        assert_eq!(
            default_cainfo().as_deref(),
            Some("/etc/ssl/certs/ca-certificates.crt")
        );
        assert_eq!(default_capath().as_deref(), Some("/etc/ssl/certs"));
        assert_eq!(backend_version(), "purecrypto/0.10.1");
        let c = TlsConfig::default();
        assert!(c.verify_peer && c.verify_host);
    }

    #[test]
    fn session_is_send() {
        fn f<T: Send>() {}
        f::<TlsSession>();
        f::<TlsResumption>();
    }

    #[test]
    fn hello_retry_request() {
        let dir = tmpdir();
        let srv_cfg = {
            let s = chain_srv();
            let key = key_from_pem(s.key.as_bytes(), None)
                .and_then(signing_key)
                .unwrap();
            let chain: Vec<Vec<u8>> = s.chain.iter().flat_map(|p| ders(p)).collect();
            Config::builder()
                .rng(Arc::new(purecrypto::rng::OsRng))
                .identity(chain, key)
                .key_exchange_groups(&[NamedGroup::Secp384r1])
                .build()
        };
        let cfg = TlsConfig {
            trace: true,
            ..base_cfg(&dir)
        };
        let mut cli = TlsSession::new(&cfg, "localhost", 443).unwrap();
        let mut srv = Connection::server(&srv_cfg).unwrap();
        assert_eq!(pump(&mut cli, &mut srv), Ok(true));
        let log = cli.take_log();
        let t = trace_texts(&log);
        assert_eq!(t.iter().filter(|s| s.contains("Client hello")).count(), 2);
        assert_eq!(t.iter().filter(|s| s.contains("Server hello")).count(), 2);
        assert!(t.contains(&"TLSv1.3 (IN), TLS handshake, Finished (20):".to_string()));
        let ccs = t
            .iter()
            .position(|s| s.starts_with("TLSv1.3 (OUT), TLS change cipher"))
            .unwrap();
        let ch2 = t.iter().rposition(|s| s.contains("Client hello")).unwrap();
        assert_eq!(ccs + 1, ch2);
        assert!(log.iter().any(|l| matches!(l, TlsLog::Info(s)
            if s.starts_with("SSL connection using TLSv1.3 / ") && s.ends_with(" / secp384r1 / RSASSA-PSS"))));
    }
}
