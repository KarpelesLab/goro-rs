//! X.509 certificate and PKCS#10 CSR support using pure-Rust crates
//! (`x509-cert`, `der`, `spki`, `const-oid`).
//!
//! Certificate and CSR objects are stored in thread-local registries and
//! referenced from PHP via the hidden properties `__cert_id` (on
//! `OpenSSLCertificate`) and `__csr_id` (on `OpenSSLCertificateSigningRequest`).

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

use crate::rsa::{
    make_key_object_pub, resolve_private_key_pub, StoredKey,
};

use der::asn1::{
    BitString, Ia5StringRef, OctetString, PrintableStringRef, SetOfVec, Utf8StringRef,
};
use der::oid::ObjectIdentifier;
use der::{Any, Decode, DecodePem, Encode, EncodePem};

use const_oid::db::DB;

use spki::{AlgorithmIdentifierOwned, SubjectPublicKeyInfoOwned};

use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::{DecodePublicKey, EncodePublicKey};
use rsa::signature::SignatureEncoding;
use rsa::signature::Signer;
use rsa::{RsaPrivateKey, RsaPublicKey};
use sha1::Sha1;
use sha2::{Digest, Sha224, Sha256, Sha384, Sha512};

use x509_cert::attr::AttributeTypeAndValue;
use x509_cert::certificate::{Certificate, TbsCertificate, Version};
use x509_cert::name::{Name, RdnSequence, RelativeDistinguishedName};
use x509_cert::request::{CertReq, CertReqInfo, Version as ReqVersion};
use x509_cert::serial_number::SerialNumber;
use x509_cert::time::{Time, Validity};

// ── Registries for OpenSSLCertificate and OpenSSLCertificateSigningRequest ──

std::thread_local! {
    static CERT_REGISTRY: RefCell<std::collections::HashMap<u64, Certificate>> =
        RefCell::new(std::collections::HashMap::new());
    static CERT_NEXT_ID: RefCell<u64> = const { RefCell::new(1) };
    static CSR_REGISTRY: RefCell<std::collections::HashMap<u64, CertReq>> =
        RefCell::new(std::collections::HashMap::new());
    static CSR_NEXT_ID: RefCell<u64> = const { RefCell::new(1) };
}

fn store_cert(c: Certificate) -> u64 {
    let id = CERT_NEXT_ID.with(|n| {
        let mut n = n.borrow_mut();
        let id = *n;
        *n += 1;
        id
    });
    CERT_REGISTRY.with(|r| {
        r.borrow_mut().insert(id, c);
    });
    id
}

fn get_cert(id: u64) -> Option<Certificate> {
    CERT_REGISTRY.with(|r| r.borrow().get(&id).cloned())
}

fn store_csr(c: CertReq) -> u64 {
    let id = CSR_NEXT_ID.with(|n| {
        let mut n = n.borrow_mut();
        let id = *n;
        *n += 1;
        id
    });
    CSR_REGISTRY.with(|r| {
        r.borrow_mut().insert(id, c);
    });
    id
}

fn get_csr(id: u64) -> Option<CertReq> {
    CSR_REGISTRY.with(|r| r.borrow().get(&id).cloned())
}

fn make_cert_object(vm: &mut Vm, cert: Certificate) -> Value {
    let id = store_cert(cert);
    let oid = vm.next_object_id();
    let mut obj = PhpObject::new(b"OpenSSLCertificate".to_vec(), oid);
    obj.set_property(b"__cert_id".to_vec(), Value::Long(id as i64));
    Value::Object(Rc::new(RefCell::new(obj)))
}

fn make_csr_object(vm: &mut Vm, csr: CertReq) -> Value {
    let id = store_csr(csr);
    let oid = vm.next_object_id();
    let mut obj = PhpObject::new(b"OpenSSLCertificateSigningRequest".to_vec(), oid);
    obj.set_property(b"__csr_id".to_vec(), Value::Long(id as i64));
    Value::Object(Rc::new(RefCell::new(obj)))
}

fn cert_from_object(v: &Value) -> Option<Certificate> {
    let v = match v {
        Value::Reference(r) => r.borrow().clone(),
        other => other.clone(),
    };
    if let Value::Object(o) = v {
        let o = o.borrow();
        if o.class_name.eq_ignore_ascii_case(b"OpenSSLCertificate") {
            let id = o.get_property(b"__cert_id").to_long();
            if id > 0 {
                return get_cert(id as u64);
            }
        }
    }
    None
}

fn csr_from_object(v: &Value) -> Option<CertReq> {
    let v = match v {
        Value::Reference(r) => r.borrow().clone(),
        other => other.clone(),
    };
    if let Value::Object(o) = v {
        let o = o.borrow();
        if o.class_name.eq_ignore_ascii_case(b"OpenSSLCertificateSigningRequest") {
            let id = o.get_property(b"__csr_id").to_long();
            if id > 0 {
                return get_csr(id as u64);
            }
        }
    }
    None
}

fn read_pem_or_file(v: &Value) -> Option<Vec<u8>> {
    let v = match v {
        Value::Reference(r) => r.borrow().clone(),
        other => other.clone(),
    };
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

fn parse_certificate_blob(data: &[u8]) -> Option<Certificate> {
    if let Ok(s) = std::str::from_utf8(data) {
        if s.contains("-----BEGIN CERTIFICATE-----") {
            if let Ok(c) = Certificate::from_pem(s.as_bytes()) {
                return Some(c);
            }
        }
    }
    if let Ok(c) = Certificate::from_der(data) {
        return Some(c);
    }
    None
}

fn parse_csr_blob(data: &[u8]) -> Option<CertReq> {
    if let Ok(s) = std::str::from_utf8(data) {
        if s.contains("BEGIN CERTIFICATE REQUEST")
            || s.contains("BEGIN NEW CERTIFICATE REQUEST")
        {
            // Try standard label parser
            if let Ok(c) = CertReq::from_pem(s.as_bytes()) {
                return Some(c);
            }
            // Some pem files use "NEW CERTIFICATE REQUEST" label which the
            // x509-cert crate's PemLabel forbids — fall back to manually
            // extracting the base64 body and parsing the DER.
            if let Some(b64) = extract_pem_body(s) {
                if let Ok(der) = base64_decode_strict(&b64) {
                    if let Ok(c) = CertReq::from_der(&der) {
                        return Some(c);
                    }
                }
            }
        }
    }
    if let Ok(c) = CertReq::from_der(data) {
        return Some(c);
    }
    None
}

fn extract_pem_body(s: &str) -> Option<String> {
    let begin = s.find("-----BEGIN")?;
    let after_begin_line = s[begin..].find('\n')?;
    let body_start = begin + after_begin_line + 1;
    let end = s[body_start..].find("-----END")?;
    let body: String = s[body_start..body_start + end]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    Some(body)
}

fn base64_decode_strict(s: &str) -> Result<Vec<u8>, ()> {
    let clean: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if clean.is_empty() {
        return Ok(Vec::new());
    }
    if clean.len() % 4 != 0 {
        return Err(());
    }
    let dec = |c: u8| -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for chunk in clean.chunks(4) {
        let a = dec(chunk[0]).ok_or(())?;
        let b = dec(chunk[1]).ok_or(())?;
        let c = if chunk[2] == b'=' { None } else { Some(dec(chunk[2]).ok_or(())?) };
        let d = if chunk[3] == b'=' { None } else { Some(dec(chunk[3]).ok_or(())?) };
        let triple = ((a as u32) << 18)
            | ((b as u32) << 12)
            | ((c.unwrap_or(0) as u32) << 6)
            | (d.unwrap_or(0) as u32);
        out.push((triple >> 16) as u8);
        if c.is_some() {
            out.push((triple >> 8) as u8);
        }
        if d.is_some() {
            out.push(triple as u8);
        }
    }
    Ok(out)
}

fn resolve_cert(v: &Value) -> Option<Certificate> {
    if let Some(c) = cert_from_object(v) {
        return Some(c);
    }
    let bytes = read_pem_or_file(v)?;
    parse_certificate_blob(&bytes)
}

fn resolve_csr(v: &Value) -> Option<CertReq> {
    if let Some(c) = csr_from_object(v) {
        return Some(c);
    }
    let bytes = read_pem_or_file(v)?;
    parse_csr_blob(&bytes)
}

// ── DN handling ──

/// Look up an OID by attribute name. Accepts both long form ("commonName")
/// and short form ("CN"), with leading-zero "0.organizationName"-style
/// prefix stripped.
fn oid_for_dn_key(key: &str) -> Option<ObjectIdentifier> {
    let stripped = match key.find('.') {
        Some(idx) if key[..idx].chars().all(|c| c.is_ascii_digit()) => &key[idx + 1..],
        _ => key,
    };
    let normalized: &str = match stripped.to_ascii_lowercase().as_str() {
        "c" | "countryname" => "C",
        "st" | "stateorprovincename" => "ST",
        "l" | "localityname" => "L",
        "o" | "organizationname" => "O",
        "ou" | "organizationalunitname" => "OU",
        "cn" | "commonname" => "CN",
        "emailaddress" | "email" => "emailAddress",
        "serialnumber" => "serialNumber",
        "title" => "title",
        "givenname" | "gn" => "givenName",
        "surname" | "sn" => "surname",
        "dc" | "domaincomponent" => "DC",
        "uid" | "userid" => "UID",
        "businesscategory" => "businessCategory",
        "street" | "streetaddress" => "street",
        "postalcode" => "postalCode",
        _ => stripped,
    };
    DB.by_name(normalized).copied()
}

/// Return (long_name, short_name) for a given DN attribute OID.
fn names_for_dn_oid(oid: &ObjectIdentifier) -> (String, String) {
    let oid_str = oid.to_string();
    let (long, short) = match oid_str.as_str() {
        "2.5.4.6" => ("countryName", "C"),
        "2.5.4.8" => ("stateOrProvinceName", "ST"),
        "2.5.4.7" => ("localityName", "L"),
        "2.5.4.10" => ("organizationName", "O"),
        "2.5.4.11" => ("organizationalUnitName", "OU"),
        "2.5.4.3" => ("commonName", "CN"),
        "1.2.840.113549.1.9.1" => ("emailAddress", "emailAddress"),
        "2.5.4.5" => ("serialNumber", "serialNumber"),
        "2.5.4.12" => ("title", "title"),
        "2.5.4.42" => ("givenName", "GN"),
        "2.5.4.4" => ("surname", "SN"),
        "0.9.2342.19200300.100.1.25" => ("domainComponent", "DC"),
        "0.9.2342.19200300.100.1.1" => ("userId", "UID"),
        "2.5.4.15" => ("businessCategory", "businessCategory"),
        "2.5.4.9" => ("streetAddress", "street"),
        "2.5.4.17" => ("postalCode", "postalCode"),
        _ => {
            if let Some(name) = DB.by_oid(oid) {
                return (name.to_string(), name.to_string());
            }
            return (oid_str.clone(), oid_str);
        }
    };
    (long.to_string(), short.to_string())
}

/// Decode an ASN.1 string value into a UTF-8 String. Best-effort.
fn decode_any_string(any: &Any) -> Option<String> {
    let der_bytes = any.to_der().ok()?;
    if der_bytes.is_empty() {
        return None;
    }
    let tag = der_bytes[0];
    match tag {
        0x0c => {
            if let Ok(s) = Utf8StringRef::from_der(&der_bytes) {
                return Some(s.as_str().to_string());
            }
        }
        0x13 => {
            if let Ok(s) = PrintableStringRef::from_der(&der_bytes) {
                return Some(s.as_str().to_string());
            }
        }
        0x16 => {
            if let Ok(s) = Ia5StringRef::from_der(&der_bytes) {
                return Some(s.as_str().to_string());
            }
        }
        0x14 => {
            // T61String (Teletex). Decode as latin-1.
            let body = any.value();
            return Some(body.iter().map(|&b| b as char).collect());
        }
        0x1e => {
            // BMPString: 16-bit big-endian
            let body = any.value();
            if body.len() % 2 != 0 {
                return None;
            }
            let mut out = String::new();
            for chunk in body.chunks(2) {
                let cp = ((chunk[0] as u32) << 8) | chunk[1] as u32;
                if let Some(c) = char::from_u32(cp) {
                    if c != '\0' {
                        out.push(c);
                    }
                } else {
                    return None;
                }
            }
            return Some(out);
        }
        _ => {}
    }
    let body = any.value();
    std::str::from_utf8(body).ok().map(|s| s.to_string())
}

fn name_to_php_array(name: &Name, use_shortnames: bool) -> PhpArray {
    let mut arr = PhpArray::new();
    for rdn in &name.0 {
        for atv in rdn.0.iter() {
            let (long, short) = names_for_dn_oid(&atv.oid);
            let key_str = if use_shortnames { short } else { long };
            let key_bytes = key_str.as_bytes().to_vec();
            let value_str = decode_any_string(&atv.value).unwrap_or_default();
            let val_value = Value::String(PhpString::from_string(value_str));
            let key = ArrayKey::String(PhpString::from_bytes(&key_bytes));
            if let Some(existing) = arr.get(&key).cloned() {
                match existing {
                    Value::Array(a) => {
                        let mut ar = a.borrow_mut();
                        ar.push(val_value);
                    }
                    other => {
                        let mut new_arr = PhpArray::new();
                        new_arr.push(other);
                        new_arr.push(val_value);
                        arr.set(
                            ArrayKey::String(PhpString::from_bytes(&key_bytes)),
                            Value::Array(Rc::new(RefCell::new(new_arr))),
                        );
                    }
                }
            } else {
                arr.set(key, val_value);
            }
        }
    }
    arr
}

fn dn_array_to_name(arr: &PhpArray) -> Result<Name, String> {
    let mut rdns: Vec<RelativeDistinguishedName> = Vec::new();
    for (k, v) in arr.iter() {
        let key_str = match k {
            ArrayKey::String(s) => String::from_utf8_lossy(s.as_bytes()).to_string(),
            ArrayKey::Int(_) => continue,
        };
        let oid = oid_for_dn_key(&key_str).ok_or_else(|| format!("Unknown DN key: {}", key_str))?;

        let values: Vec<String> = match v {
            Value::Array(a) => {
                let inner = a.borrow();
                inner
                    .iter()
                    .map(|(_, v)| v.to_php_string().to_string_lossy().to_string())
                    .collect()
            }
            other => vec![other.to_php_string().to_string_lossy().to_string()],
        };

        for value_str in values {
            let oid_str = oid.to_string();
            let any_val = if oid_str == "1.2.840.113549.1.9.1"
                || oid_str == "0.9.2342.19200300.100.1.25"
            {
                let s = Ia5StringRef::new(&value_str).map_err(|e| e.to_string())?;
                Any::from(s)
            } else if oid_str == "2.5.4.6" {
                let s = PrintableStringRef::new(&value_str).map_err(|e| e.to_string())?;
                Any::from(s)
            } else {
                let s = Utf8StringRef::new(&value_str).map_err(|e| e.to_string())?;
                Any::from(s)
            };

            let atv = AttributeTypeAndValue {
                oid,
                value: any_val,
            };
            let mut set: SetOfVec<AttributeTypeAndValue> = SetOfVec::new();
            set.insert(atv).map_err(|e| e.to_string())?;
            rdns.push(RelativeDistinguishedName(set));
        }
    }
    Ok(RdnSequence(rdns))
}

fn name_to_php_string(name: &Name) -> String {
    let mut out = String::new();
    for rdn in &name.0 {
        for atv in rdn.0.iter() {
            let (_long, short) = names_for_dn_oid(&atv.oid);
            let val = decode_any_string(&atv.value).unwrap_or_default();
            out.push('/');
            out.push_str(&short);
            out.push('=');
            out.push_str(&val);
        }
    }
    out
}

// ── Signature algorithm OIDs ──

const OID_MD2_RSA: &str = "1.2.840.113549.1.1.2";
const OID_MD5_RSA: &str = "1.2.840.113549.1.1.4";
const OID_SHA1_RSA: &str = "1.2.840.113549.1.1.5";
const OID_SHA224_RSA: &str = "1.2.840.113549.1.1.14";
const OID_SHA256_RSA: &str = "1.2.840.113549.1.1.11";
const OID_SHA384_RSA: &str = "1.2.840.113549.1.1.12";
const OID_SHA512_RSA: &str = "1.2.840.113549.1.1.13";

fn sig_alg_info(oid: &ObjectIdentifier) -> (String, String, i64) {
    let oid_str = oid.to_string();
    match oid_str.as_str() {
        OID_MD2_RSA => ("RSA-MD2".into(), "md2WithRSAEncryption".into(), 7),
        OID_MD5_RSA => ("RSA-MD5".into(), "md5WithRSAEncryption".into(), 8),
        OID_SHA1_RSA => ("RSA-SHA1".into(), "sha1WithRSAEncryption".into(), 65),
        OID_SHA224_RSA => ("RSA-SHA224".into(), "sha224WithRSAEncryption".into(), 671),
        OID_SHA256_RSA => ("RSA-SHA256".into(), "sha256WithRSAEncryption".into(), 668),
        OID_SHA384_RSA => ("RSA-SHA384".into(), "sha384WithRSAEncryption".into(), 669),
        OID_SHA512_RSA => ("RSA-SHA512".into(), "sha512WithRSAEncryption".into(), 670),
        _ => {
            if let Some(name) = DB.by_oid(oid) {
                return (name.to_string(), name.to_string(), 0);
            }
            (oid_str.clone(), oid_str, 0)
        }
    }
}

// ── openssl_x509_read ──

pub fn openssl_x509_read(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().cloned().unwrap_or(Value::Null);
    let deref = match &arg {
        Value::Reference(r) => r.borrow().clone(),
        other => other.clone(),
    };
    match &deref {
        Value::Array(_) => {
            let exc = vm.throw_type_error(
                "openssl_x509_read(): Argument #1 ($certificate) must be of type OpenSSLCertificate|string, array given".to_string(),
            );
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: "Uncaught TypeError".to_string(),
                line: 0,
            });
        }
        Value::Object(o) => {
            let cls = o.borrow().class_name.clone();
            if !cls.eq_ignore_ascii_case(b"OpenSSLCertificate") {
                let exc = vm.throw_type_error(format!(
                    "openssl_x509_read(): Argument #1 ($certificate) must be of type OpenSSLCertificate|string, {} given",
                    String::from_utf8_lossy(&cls)
                ));
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: "Uncaught TypeError".to_string(),
                    line: 0,
                });
            }
        }
        _ => {}
    }

    if let Some(c) = resolve_cert(&arg) {
        return Ok(make_cert_object(vm, c));
    }
    vm.emit_warning("openssl_x509_read(): X.509 Certificate cannot be retrieved");
    Ok(Value::False)
}

// ── openssl_x509_free (no-op deprecated) ──

pub fn openssl_x509_free(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    vm.emit_deprecated(
        "Function openssl_x509_free() is deprecated since 8.0, as OpenSSLCertificate objects are freed automatically",
    );
    Ok(Value::Null)
}

// ── openssl_x509_parse ──

pub fn openssl_x509_parse(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cert_arg = args.first().cloned().unwrap_or(Value::Null);
    let short = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
    let cert = match resolve_cert(&cert_arg) {
        Some(c) => c,
        None => {
            vm.emit_warning("openssl_x509_parse(): X.509 Certificate cannot be retrieved");
            return Ok(Value::False);
        }
    };
    Ok(build_parse_result(&cert, short))
}

fn build_parse_result(cert: &Certificate, short: bool) -> Value {
    let tbs = &cert.tbs_certificate;
    let mut result = PhpArray::new();

    let subject_name = name_to_php_string(&tbs.subject);
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"name")),
        Value::String(PhpString::from_string(subject_name.clone())),
    );

    let subj_arr = name_to_php_array(&tbs.subject, short);
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"subject")),
        Value::Array(Rc::new(RefCell::new(subj_arr))),
    );

    let subj_der = tbs.subject.to_der().unwrap_or_default();
    let mut hasher = Sha1::new();
    hasher.update(&subj_der);
    let h = hasher.finalize();
    let hash_val = u32::from_le_bytes([h[0], h[1], h[2], h[3]]);
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"hash")),
        Value::String(PhpString::from_string(format!("{:08x}", hash_val))),
    );

    let iss_arr = name_to_php_array(&tbs.issuer, short);
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"issuer")),
        Value::Array(Rc::new(RefCell::new(iss_arr))),
    );

    let ver = match tbs.version {
        Version::V1 => 0,
        Version::V2 => 1,
        Version::V3 => 2,
    };
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"version")),
        Value::Long(ver),
    );

    let serial_bytes_raw = tbs.serial_number.as_bytes();
    // Strip a single leading 0x00 sign byte if present (PHP/OpenSSL show
    // the unsigned magnitude without the sign byte).
    let serial_bytes: &[u8] = if serial_bytes_raw.len() > 1
        && serial_bytes_raw[0] == 0
        && (serial_bytes_raw[1] & 0x80) != 0
    {
        &serial_bytes_raw[1..]
    } else {
        serial_bytes_raw
    };
    let serial_dec = bytes_be_to_decimal_string(serial_bytes);
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"serialNumber")),
        Value::String(PhpString::from_string(serial_dec)),
    );

    let serial_hex: String = serial_bytes.iter().map(|b| format!("{:02X}", b)).collect();
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"serialNumberHex")),
        Value::String(PhpString::from_string(serial_hex)),
    );

    let (vf, vf_t) = encode_time(&tbs.validity.not_before);
    let (vt, vt_t) = encode_time(&tbs.validity.not_after);
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"validFrom")),
        Value::String(PhpString::from_string(vf)),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"validTo")),
        Value::String(PhpString::from_string(vt)),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"validFrom_time_t")),
        Value::Long(vf_t),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"validTo_time_t")),
        Value::Long(vt_t),
    );

    let (sn, ln, nid) = sig_alg_info(&cert.signature_algorithm.oid);
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"signatureTypeSN")),
        Value::String(PhpString::from_string(sn)),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"signatureTypeLN")),
        Value::String(PhpString::from_string(ln)),
    );
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"signatureTypeNID")),
        Value::Long(nid),
    );

    // purposes: PHP fills with the standard 9 purposes; we always return
    // pass=true and CA=true since we don't do real purpose checking.
    let purposes = build_purposes(short);
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"purposes")),
        Value::Array(Rc::new(RefCell::new(purposes))),
    );

    let mut ext_arr = PhpArray::new();
    if let Some(exts) = &tbs.extensions {
        for ext in exts {
            let name = extension_name(&ext.extn_id);
            let val = format_extension(&ext.extn_id, ext.extn_value.as_bytes());
            ext_arr.set(
                ArrayKey::String(PhpString::from_bytes(name.as_bytes())),
                Value::String(PhpString::from_string(val)),
            );
        }
    }
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"extensions")),
        Value::Array(Rc::new(RefCell::new(ext_arr))),
    );

    Value::Array(Rc::new(RefCell::new(result)))
}

fn bytes_be_to_decimal_string(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "0".to_string();
    }
    let mut digits: Vec<u8> = vec![0];
    for &b in bytes {
        let mut carry: u32 = b as u32;
        for d in digits.iter_mut() {
            let cur = (*d as u32) * 256 + carry;
            *d = (cur % 10) as u8;
            carry = cur / 10;
        }
        while carry > 0 {
            digits.push((carry % 10) as u8);
            carry /= 10;
        }
    }
    while digits.len() > 1 && *digits.last().unwrap() == 0 {
        digits.pop();
    }
    digits.iter().rev().map(|d| (b'0' + d) as char).collect()
}

fn encode_time(t: &Time) -> (String, i64) {
    let dt = t.to_date_time();
    let year = dt.year();
    let month = dt.month();
    let day = dt.day();
    let hour = dt.hour();
    let minute = dt.minutes();
    let second = dt.seconds();
    // Match the format that's actually encoded on-wire: UTCTime uses 2-digit
    // year, GeneralizedTime uses 4-digit year.
    let s = match t {
        Time::UtcTime(_) => format!(
            "{:02}{:02}{:02}{:02}{:02}{:02}Z",
            year % 100,
            month,
            day,
            hour,
            minute,
            second
        ),
        Time::GeneralTime(_) => format!(
            "{:04}{:02}{:02}{:02}{:02}{:02}Z",
            year, month, day, hour, minute, second
        ),
    };
    let unix = t.to_unix_duration().as_secs() as i64;
    (s, unix)
}

/// Build the "purposes" array similar to PHP's openssl_x509_parse output.
/// PHP returns an array indexed 1..9, each element a 3-element list:
///   [0] => pass, [1] => CA pass, [2] => purpose name (short or long)
fn build_purposes(short: bool) -> PhpArray {
    let entries: &[(&str, &str, bool, bool)] = &[
        ("sslclient", "SSL client", true, true),
        ("sslserver", "SSL server", true, true),
        ("nssslserver", "Netscape SSL server", true, true),
        ("smimesign", "S/MIME signing", true, true),
        ("smimeencrypt", "S/MIME encryption", true, true),
        ("crlsign", "CRL signing", true, true),
        ("any", "Any Purpose", true, true),
        ("ocsphelper", "OCSP helper", true, true),
        ("timestampsign", "Time Stamp signing", false, true),
    ];
    let mut arr = PhpArray::new();
    for (i, (s, l, pass, capass)) in entries.iter().enumerate() {
        let mut row = PhpArray::new();
        row.push(if *pass { Value::True } else { Value::False });
        row.push(if *capass { Value::True } else { Value::False });
        row.push(Value::String(PhpString::from_string(if short {
            s.to_string()
        } else {
            l.to_string()
        })));
        arr.set(
            ArrayKey::Int((i + 1) as i64),
            Value::Array(Rc::new(RefCell::new(row))),
        );
    }
    arr
}

/// Map X.509 extension OIDs to PHP/OpenSSL short names. Falls back to dotted
/// OID string if unknown.
fn extension_name(oid: &ObjectIdentifier) -> String {
    let oid_str = oid.to_string();
    match oid_str.as_str() {
        "2.5.29.14" => "subjectKeyIdentifier".to_string(),
        "2.5.29.15" => "keyUsage".to_string(),
        "2.5.29.17" => "subjectAltName".to_string(),
        "2.5.29.18" => "issuerAltName".to_string(),
        "2.5.29.19" => "basicConstraints".to_string(),
        "2.5.29.20" => "cRLNumber".to_string(),
        "2.5.29.21" => "cRLReason".to_string(),
        "2.5.29.23" => "holdInstructionCode".to_string(),
        "2.5.29.24" => "invalidityDate".to_string(),
        "2.5.29.27" => "deltaCRLIndicator".to_string(),
        "2.5.29.28" => "issuingDistributionPoint".to_string(),
        "2.5.29.29" => "certificateIssuer".to_string(),
        "2.5.29.30" => "nameConstraints".to_string(),
        "2.5.29.31" => "crlDistributionPoints".to_string(),
        "2.5.29.32" => "certificatePolicies".to_string(),
        "2.5.29.33" => "policyMappings".to_string(),
        "2.5.29.35" => "authorityKeyIdentifier".to_string(),
        "2.5.29.36" => "policyConstraints".to_string(),
        "2.5.29.37" => "extendedKeyUsage".to_string(),
        "2.5.29.46" => "freshestCRL".to_string(),
        "2.5.29.54" => "inhibitAnyPolicy".to_string(),
        "1.3.6.1.5.5.7.1.1" => "authorityInfoAccess".to_string(),
        "1.3.6.1.5.5.7.1.11" => "subjectInfoAccess".to_string(),
        "2.16.840.1.113730.1.1" => "nsCertType".to_string(),
        "2.16.840.1.113730.1.13" => "nsComment".to_string(),
        _ => oid_str,
    }
}

fn format_extension(oid: &ObjectIdentifier, value: &[u8]) -> String {
    let oid_str = oid.to_string();
    match oid_str.as_str() {
        "2.5.29.19" => {
            if value.windows(3).any(|w| w == [0x01, 0x01, 0xff]) {
                return "CA:TRUE".to_string();
            }
            "CA:FALSE".to_string()
        }
        "2.5.29.14" => {
            // subjectKeyIdentifier: OCTET STRING containing the key id
            if let Ok(os) = OctetString::from_der(value) {
                return os
                    .as_bytes()
                    .iter()
                    .map(|b| format!("{:02X}", b))
                    .collect::<Vec<_>>()
                    .join(":");
            }
            String::new()
        }
        "2.5.29.35" => {
            // authorityKeyIdentifier: emit "keyid:...\n[DirName:...\nserial:...]"
            // best-effort decode. The value is a SEQUENCE with optional
            // [0] keyIdentifier (OCTET STRING IMPLICIT), [1] GeneralNames
            // IMPLICIT, [2] serialNumber INTEGER IMPLICIT.
            return decode_authority_key_identifier(value);
        }
        _ => value
            .iter()
            .take(64)
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(":"),
    }
}

fn decode_authority_key_identifier(value: &[u8]) -> String {
    // Parse top-level SEQUENCE
    if value.len() < 2 || value[0] != 0x30 {
        return String::new();
    }
    let (seq_body, _) = match parse_der_length(&value[1..]) {
        Some(p) => p,
        None => return String::new(),
    };
    let mut rest = seq_body;
    let mut out = String::new();
    while !rest.is_empty() {
        let tag = rest[0];
        let (body, remaining) = match parse_der_length(&rest[1..]) {
            Some(p) => p,
            None => break,
        };
        match tag {
            0x80 => {
                // [0] keyIdentifier IMPLICIT OCTET STRING
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str("keyid:");
                out.push_str(
                    &body
                        .iter()
                        .map(|b| format!("{:02X}", b))
                        .collect::<Vec<_>>()
                        .join(":"),
                );
            }
            0xa1 => {
                // [1] GeneralNames EXPLICIT
                // Iterate alternatives
                let mut inner = body;
                while !inner.is_empty() {
                    let it_tag = inner[0];
                    let (it_body, inner_rest) = match parse_der_length(&inner[1..]) {
                        Some(p) => p,
                        None => break,
                    };
                    if it_tag == 0xa4 {
                        // [4] directoryName EXPLICIT Name
                        // Name is a SEQUENCE; reconstruct Name from inside
                        if let Ok(name) = Name::from_der(it_body) {
                            if !out.is_empty() {
                                out.push('\n');
                            }
                            out.push_str("DirName:");
                            out.push_str(&name_to_php_string(&name));
                        }
                    }
                    inner = inner_rest;
                }
            }
            0x82 => {
                // [2] authorityCertSerialNumber IMPLICIT INTEGER
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str("serial:");
                out.push_str(
                    &body
                        .iter()
                        .map(|b| format!("{:02X}", b))
                        .collect::<Vec<_>>()
                        .join(":"),
                );
            }
            _ => {}
        }
        rest = remaining;
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Parse DER length from the start of `data`. Returns (body, rest_after_body).
fn parse_der_length(data: &[u8]) -> Option<(&[u8], &[u8])> {
    if data.is_empty() {
        return None;
    }
    let first = data[0];
    let (len, hdr_len) = if first & 0x80 == 0 {
        (first as usize, 1)
    } else {
        let n = (first & 0x7f) as usize;
        if n == 0 || n > 4 || data.len() < 1 + n {
            return None;
        }
        let mut len = 0usize;
        for i in 1..=n {
            len = (len << 8) | data[i] as usize;
        }
        (len, 1 + n)
    };
    if data.len() < hdr_len + len {
        return None;
    }
    Some((
        &data[hdr_len..hdr_len + len],
        &data[hdr_len + len..],
    ))
}

// ── openssl_x509_check_private_key ──

pub fn openssl_x509_check_private_key(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cert_arg = args.first().cloned().unwrap_or(Value::Null);
    let key_arg = args.get(1).cloned().unwrap_or(Value::Null);
    let cert = match resolve_cert(&cert_arg) {
        Some(c) => c,
        None => return Ok(Value::False),
    };
    let priv_key = match resolve_private_key_pub(&key_arg, None) {
        Some(k) => k,
        None => return Ok(Value::False),
    };

    let cert_spki = match cert.tbs_certificate.subject_public_key_info.to_der() {
        Ok(b) => b,
        Err(_) => return Ok(Value::False),
    };
    let pub_from_priv = priv_key.to_public_key();
    let priv_spki = match pub_from_priv.to_public_key_der() {
        Ok(d) => d.as_bytes().to_vec(),
        Err(_) => return Ok(Value::False),
    };
    let _ = vm;
    Ok(if cert_spki == priv_spki {
        Value::True
    } else {
        Value::False
    })
}

/// Check whether the certificate argument is an acceptable type (string,
/// null, or OpenSSLCertificate object). Returns a `VmError` with a TypeError
/// exception attached if the type is wrong.
fn ensure_cert_arg_type(vm: &mut Vm, func: &str, arg: &Value) -> Result<(), VmError> {
    let deref = match arg {
        Value::Reference(r) => r.borrow().clone(),
        other => other.clone(),
    };
    let bad: Option<&str> = match deref {
        Value::Array(_) => Some("array"),
        Value::Object(ref o) => {
            let cls = o.borrow().class_name.clone();
            if cls.eq_ignore_ascii_case(b"OpenSSLCertificate") {
                None
            } else {
                // Borrow already dropped; static-ish error
                let cls_str = String::from_utf8_lossy(&cls).to_string();
                let exc = vm.throw_type_error(format!(
                    "{}(): Argument #1 ($certificate) must be of type OpenSSLCertificate|string, {} given",
                    func, cls_str
                ));
                vm.current_exception = Some(exc);
                return Err(VmError {
                    message: "Uncaught TypeError".to_string(),
                    line: 0,
                });
            }
        }
        _ => None,
    };
    if let Some(t) = bad {
        let exc = vm.throw_type_error(format!(
            "{}(): Argument #1 ($certificate) must be of type OpenSSLCertificate|string, {} given",
            func, t
        ));
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: "Uncaught TypeError".to_string(),
            line: 0,
        });
    }
    Ok(())
}

// ── openssl_x509_export ──

pub fn openssl_x509_export(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cert_arg = args.first().cloned().unwrap_or(Value::Null);
    ensure_cert_arg_type(vm, "openssl_x509_export", &cert_arg)?;
    let cert = match resolve_cert(&cert_arg) {
        Some(c) => c,
        None => {
            vm.emit_warning("openssl_x509_export(): X.509 Certificate cannot be retrieved");
            return Ok(Value::False);
        }
    };
    let pem = match cert.to_pem(der::pem::LineEnding::LF) {
        Ok(s) => s,
        Err(e) => {
            vm.emit_warning(&format!("openssl_x509_export(): {}", e));
            return Ok(Value::False);
        }
    };
    if let Some(Value::Reference(r)) = args.get(1) {
        *r.borrow_mut() = Value::String(PhpString::from_string(pem));
    }
    Ok(Value::True)
}

// ── openssl_x509_export_to_file ──

pub fn openssl_x509_export_to_file(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cert_arg = args.first().cloned().unwrap_or(Value::Null);
    ensure_cert_arg_type(vm, "openssl_x509_export_to_file", &cert_arg)?;
    let filename = args
        .get(1)
        .map(|v| v.to_php_string())
        .unwrap_or_else(|| PhpString::from_bytes(b""));
    let cert = match resolve_cert(&cert_arg) {
        Some(c) => c,
        None => {
            vm.emit_warning("openssl_x509_export_to_file(): X.509 Certificate cannot be retrieved");
            return Ok(Value::False);
        }
    };
    let pem = match cert.to_pem(der::pem::LineEnding::LF) {
        Ok(s) => s,
        Err(e) => {
            vm.emit_warning(&format!("openssl_x509_export_to_file(): {}", e));
            return Ok(Value::False);
        }
    };
    let path = filename.to_string_lossy().to_string();
    if path.len() > 4096 || path.is_empty() {
        vm.emit_warning(
            "openssl_x509_export_to_file(output_filename): must be a valid file path",
        );
        return Ok(Value::False);
    }
    match std::fs::write(&path, pem) {
        Ok(()) => Ok(Value::True),
        Err(e) => {
            vm.emit_warning(&format!("openssl_x509_export_to_file(): {}", e));
            Ok(Value::False)
        }
    }
}

// ── openssl_x509_fingerprint ──

pub fn openssl_x509_fingerprint(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let cert_arg = args.first().cloned().unwrap_or(Value::Null);
    let algo = args
        .get(1)
        .map(|v| v.to_php_string().to_string_lossy().to_string())
        .unwrap_or_else(|| "sha1".to_string());
    let binary = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
    let cert = match resolve_cert(&cert_arg) {
        Some(c) => c,
        None => {
            vm.emit_warning("openssl_x509_fingerprint(): X.509 Certificate cannot be retrieved");
            return Ok(Value::False);
        }
    };
    let der = match cert.to_der() {
        Ok(d) => d,
        Err(_) => return Ok(Value::False),
    };
    let digest: Vec<u8> = match algo.to_ascii_lowercase().as_str() {
        "sha1" => Sha1::digest(&der).to_vec(),
        "sha224" => Sha224::digest(&der).to_vec(),
        "sha256" => Sha256::digest(&der).to_vec(),
        "sha384" => Sha384::digest(&der).to_vec(),
        "sha512" => Sha512::digest(&der).to_vec(),
        "md5" => {
            use md5::Digest as MdDigest;
            let mut h = md5::Md5::new();
            MdDigest::update(&mut h, &der);
            MdDigest::finalize(h).to_vec()
        }
        _ => {
            vm.emit_warning("openssl_x509_fingerprint(): Unknown digest algorithm");
            return Ok(Value::False);
        }
    };
    if binary {
        Ok(Value::String(PhpString::from_vec(digest)))
    } else {
        let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
        Ok(Value::String(PhpString::from_string(hex)))
    }
}

// ── CSR ──

pub fn openssl_csr_new(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let dn_arg = args.first().cloned().unwrap_or(Value::Null);
    let priv_arg = args.get(1).cloned().unwrap_or(Value::Null);
    let options_arg = args.get(2).cloned().unwrap_or(Value::Null);

    let deref_dn = match &dn_arg {
        Value::Reference(r) => r.borrow().clone(),
        other => other.clone(),
    };
    let dn_arr = match deref_dn {
        Value::Array(a) => a,
        _ => {
            vm.emit_warning(
                "openssl_csr_new(): Argument #1 ($distinguished_names) must be of type array",
            );
            return Ok(Value::False);
        }
    };
    let dn_arr_ref = dn_arr.borrow();

    let subject = match dn_array_to_name(&dn_arr_ref) {
        Ok(n) => n,
        Err(_) => return Ok(Value::False),
    };

    let priv_deref = match &priv_arg {
        Value::Reference(r) => r.borrow().clone(),
        other => other.clone(),
    };

    let priv_key: RsaPrivateKey = if matches!(priv_deref, Value::Null) {
        let bits = read_bits_from_options(&options_arg).unwrap_or(2048);
        let mut rng = rand::thread_rng();
        match RsaPrivateKey::new(&mut rng, bits) {
            Ok(k) => {
                if let Some(Value::Reference(r)) = args.get(1) {
                    let obj = make_key_object_pub(vm, StoredKey::Private(k.clone()));
                    *r.borrow_mut() = obj;
                }
                k
            }
            Err(_) => return Ok(Value::False),
        }
    } else {
        match resolve_private_key_pub(&priv_arg, None) {
            Some(k) => k,
            None => {
                let bits = read_bits_from_options(&options_arg).unwrap_or(2048);
                let mut rng = rand::thread_rng();
                match RsaPrivateKey::new(&mut rng, bits) {
                    Ok(k) => {
                        if let Some(Value::Reference(r)) = args.get(1) {
                            let obj = make_key_object_pub(vm, StoredKey::Private(k.clone()));
                            *r.borrow_mut() = obj;
                        }
                        k
                    }
                    Err(_) => return Ok(Value::False),
                }
            }
        }
    };

    let digest_name = read_digest_alg_from_options(&options_arg).unwrap_or_else(|| "sha256".into());
    let sig_oid = sig_oid_for_digest(&digest_name);

    let pub_der_doc = match priv_key.to_public_key().to_public_key_der() {
        Ok(d) => d,
        Err(_) => return Ok(Value::False),
    };
    let spki = match SubjectPublicKeyInfoOwned::from_der(pub_der_doc.as_bytes()) {
        Ok(s) => s,
        Err(_) => return Ok(Value::False),
    };

    let info = CertReqInfo {
        version: ReqVersion::V1,
        subject,
        public_key: spki,
        attributes: SetOfVec::new(),
    };
    let info_der = match info.to_der() {
        Ok(d) => d,
        Err(_) => return Ok(Value::False),
    };

    let signature_bytes = match sign_with_digest(&priv_key, &digest_name, &info_der) {
        Some(s) => s,
        None => return Ok(Value::False),
    };

    let sig_oid_obj = match ObjectIdentifier::new(&sig_oid) {
        Ok(o) => o,
        Err(_) => return Ok(Value::False),
    };
    let alg = AlgorithmIdentifierOwned {
        oid: sig_oid_obj,
        parameters: Some(Any::null()),
    };

    let sig_bs = match BitString::from_bytes(&signature_bytes) {
        Ok(b) => b,
        Err(_) => return Ok(Value::False),
    };

    let csr = CertReq {
        info,
        algorithm: alg,
        signature: sig_bs,
    };

    Ok(make_csr_object(vm, csr))
}

fn read_bits_from_options(v: &Value) -> Option<usize> {
    let v = match v {
        Value::Reference(r) => r.borrow().clone(),
        other => other.clone(),
    };
    if let Value::Array(a) = v {
        let a = a.borrow();
        if let Some(b) = a.get(&ArrayKey::String(PhpString::from_bytes(b"private_key_bits"))) {
            let b = b.to_long();
            if b > 0 {
                return Some(b as usize);
            }
        }
    }
    None
}

fn read_digest_alg_from_options(v: &Value) -> Option<String> {
    let v = match v {
        Value::Reference(r) => r.borrow().clone(),
        other => other.clone(),
    };
    if let Value::Array(a) = v {
        let a = a.borrow();
        if let Some(b) = a.get(&ArrayKey::String(PhpString::from_bytes(b"digest_alg"))) {
            return Some(b.to_php_string().to_string_lossy().to_string());
        }
    }
    None
}

fn sig_oid_for_digest(digest: &str) -> String {
    match digest.to_ascii_lowercase().as_str() {
        "sha1" => OID_SHA1_RSA.to_string(),
        "sha224" => OID_SHA224_RSA.to_string(),
        "sha256" => OID_SHA256_RSA.to_string(),
        "sha384" => OID_SHA384_RSA.to_string(),
        "sha512" => OID_SHA512_RSA.to_string(),
        _ => OID_SHA256_RSA.to_string(),
    }
}

fn sign_with_digest(priv_key: &RsaPrivateKey, digest: &str, data: &[u8]) -> Option<Vec<u8>> {
    match digest.to_ascii_lowercase().as_str() {
        "sha1" => {
            let signing = SigningKey::<Sha1>::new(priv_key.clone());
            let sig = signing.sign(data);
            Some(sig.to_bytes().to_vec())
        }
        "sha224" => {
            let signing = SigningKey::<Sha224>::new(priv_key.clone());
            let sig = signing.sign(data);
            Some(sig.to_bytes().to_vec())
        }
        "sha256" => {
            let signing = SigningKey::<Sha256>::new(priv_key.clone());
            let sig = signing.sign(data);
            Some(sig.to_bytes().to_vec())
        }
        "sha384" => {
            let signing = SigningKey::<Sha384>::new(priv_key.clone());
            let sig = signing.sign(data);
            Some(sig.to_bytes().to_vec())
        }
        "sha512" => {
            let signing = SigningKey::<Sha512>::new(priv_key.clone());
            let sig = signing.sign(data);
            Some(sig.to_bytes().to_vec())
        }
        _ => None,
    }
}

fn ensure_csr_arg_type(vm: &mut Vm, func: &str, arg: &Value) -> Result<(), VmError> {
    let deref = match arg {
        Value::Reference(r) => r.borrow().clone(),
        other => other.clone(),
    };
    let bad: Option<String> = match deref {
        Value::Array(_) => Some("array".to_string()),
        Value::Object(ref o) => {
            let cls = o.borrow().class_name.clone();
            if cls.eq_ignore_ascii_case(b"OpenSSLCertificateSigningRequest") {
                None
            } else {
                Some(String::from_utf8_lossy(&cls).to_string())
            }
        }
        _ => None,
    };
    if let Some(t) = bad {
        let exc = vm.throw_type_error(format!(
            "{}(): Argument #1 ($csr) must be of type OpenSSLCertificateSigningRequest|string, {} given",
            func, t
        ));
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: "Uncaught TypeError".to_string(),
            line: 0,
        });
    }
    Ok(())
}

pub fn openssl_csr_export(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().cloned().unwrap_or(Value::Null);
    ensure_csr_arg_type(vm, "openssl_csr_export", &arg)?;
    let csr = match resolve_csr(&arg) {
        Some(c) => c,
        None => {
            vm.emit_warning(
                "openssl_csr_export(): X.509 Certificate Signing Request cannot be retrieved",
            );
            return Ok(Value::False);
        }
    };
    let pem = match csr.to_pem(der::pem::LineEnding::LF) {
        Ok(s) => s,
        Err(e) => {
            vm.emit_warning(&format!("openssl_csr_export(): {}", e));
            return Ok(Value::False);
        }
    };
    if let Some(Value::Reference(r)) = args.get(1) {
        *r.borrow_mut() = Value::String(PhpString::from_string(pem));
    }
    Ok(Value::True)
}

pub fn openssl_csr_export_to_file(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().cloned().unwrap_or(Value::Null);
    ensure_csr_arg_type(vm, "openssl_csr_export_to_file", &arg)?;
    let filename = args
        .get(1)
        .map(|v| v.to_php_string())
        .unwrap_or_else(|| PhpString::from_bytes(b""));
    let csr = match resolve_csr(&arg) {
        Some(c) => c,
        None => {
            vm.emit_warning(
                "openssl_csr_export_to_file(): X.509 Certificate Signing Request cannot be retrieved",
            );
            return Ok(Value::False);
        }
    };
    let pem = match csr.to_pem(der::pem::LineEnding::LF) {
        Ok(s) => s,
        Err(e) => {
            vm.emit_warning(&format!("openssl_csr_export_to_file(): {}", e));
            return Ok(Value::False);
        }
    };
    let path = filename.to_string_lossy().to_string();
    if path.len() > 4096 || path.is_empty() {
        vm.emit_warning(
            "openssl_csr_export_to_file(output_filename): must be a valid file path",
        );
        return Ok(Value::False);
    }
    match std::fs::write(&path, pem) {
        Ok(()) => Ok(Value::True),
        Err(e) => {
            vm.emit_warning(&format!("openssl_csr_export_to_file(): {}", e));
            Ok(Value::False)
        }
    }
}

pub fn openssl_csr_get_public_key(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().cloned().unwrap_or(Value::Null);
    let csr = match resolve_csr(&arg) {
        Some(c) => c,
        None => {
            vm.emit_warning(
                "openssl_csr_get_public_key(): X.509 Certificate Signing Request cannot be retrieved",
            );
            return Ok(Value::False);
        }
    };
    let der = match csr.info.public_key.to_der() {
        Ok(d) => d,
        Err(_) => return Ok(Value::False),
    };
    match RsaPublicKey::from_public_key_der(&der) {
        Ok(k) => Ok(make_key_object_pub(vm, StoredKey::Public(k))),
        Err(_) => Ok(Value::False),
    }
}

pub fn openssl_csr_get_subject(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().cloned().unwrap_or(Value::Null);
    let short = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
    let csr = match resolve_csr(&arg) {
        Some(c) => c,
        None => {
            vm.emit_warning(
                "openssl_csr_get_subject(): X.509 Certificate Signing Request cannot be retrieved",
            );
            return Ok(Value::False);
        }
    };
    let arr = name_to_php_array(&csr.info.subject, short);
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn ensure_csr_sign_csr_type(vm: &mut Vm, arg: &Value) -> Result<(), VmError> {
    let deref = match arg {
        Value::Reference(r) => r.borrow().clone(),
        other => other.clone(),
    };
    let bad: Option<String> = match deref {
        Value::Array(_) => Some("array".to_string()),
        Value::Object(ref o) => {
            let cls = o.borrow().class_name.clone();
            if cls.eq_ignore_ascii_case(b"OpenSSLCertificateSigningRequest") {
                None
            } else {
                Some(String::from_utf8_lossy(&cls).to_string())
            }
        }
        _ => None,
    };
    if let Some(t) = bad {
        let exc = vm.throw_type_error(format!(
            "openssl_csr_sign(): Argument #1 ($csr) must be of type OpenSSLCertificateSigningRequest|string, {} given",
            t
        ));
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: "Uncaught TypeError".to_string(),
            line: 0,
        });
    }
    Ok(())
}

fn ensure_csr_sign_ca_type(vm: &mut Vm, arg: &Value) -> Result<(), VmError> {
    let deref = match arg {
        Value::Reference(r) => r.borrow().clone(),
        other => other.clone(),
    };
    let bad: Option<String> = match deref {
        Value::Array(_) => Some("array".to_string()),
        Value::Object(ref o) => {
            let cls = o.borrow().class_name.clone();
            if cls.eq_ignore_ascii_case(b"OpenSSLCertificate") {
                None
            } else {
                Some(String::from_utf8_lossy(&cls).to_string())
            }
        }
        _ => None,
    };
    if let Some(t) = bad {
        let exc = vm.throw_type_error(format!(
            "openssl_csr_sign(): Argument #2 ($ca_certificate) must be of type OpenSSLCertificate|string|null, {} given",
            t
        ));
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: "Uncaught TypeError".to_string(),
            line: 0,
        });
    }
    Ok(())
}

pub fn openssl_csr_sign(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let csr_arg = args.first().cloned().unwrap_or(Value::Null);
    let ca_arg = args.get(1).cloned().unwrap_or(Value::Null);
    let priv_arg = args.get(2).cloned().unwrap_or(Value::Null);
    let days = args.get(3).map(|v| v.to_long()).unwrap_or(365);
    let options_arg = args.get(4).cloned().unwrap_or(Value::Null);
    let serial = args.get(5).map(|v| v.to_long()).unwrap_or(0);
    let serial_hex_arg = args.get(6).cloned().unwrap_or(Value::Null);

    // Type-check args before doing any work.
    ensure_csr_sign_csr_type(vm, &csr_arg)?;
    ensure_csr_sign_ca_type(vm, &ca_arg)?;
    // private_key arrays are special: only [key, passphrase] form allowed.
    // If $priv_arg is an array with !=2 ints, throw ValueError.
    let priv_deref = match &priv_arg {
        Value::Reference(r) => r.borrow().clone(),
        other => other.clone(),
    };
    if let Value::Array(_) = priv_deref {
        // Caller passed array — must be [key, phrase]. The non-key-pair form
        // raises ValueError.
        let exc = vm.create_exception(
            b"ValueError",
            "Key array must be of the form array(0 => key, 1 => phrase)",
            0,
        );
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: "Uncaught ValueError".to_string(),
            line: 0,
        });
    }

    let csr = match resolve_csr(&csr_arg) {
        Some(c) => c,
        None => {
            vm.emit_warning(
                "openssl_csr_sign(): X.509 Certificate Signing Request cannot be retrieved",
            );
            return Ok(Value::False);
        }
    };
    let priv_key = match resolve_private_key_pub(&priv_arg, None) {
        Some(k) => k,
        None => {
            vm.emit_warning("openssl_csr_sign(): Cannot get private key from parameter 3");
            return Ok(Value::False);
        }
    };

    let issuer_name: Name = {
        let ca_deref = match &ca_arg {
            Value::Reference(r) => r.borrow().clone(),
            other => other.clone(),
        };
        if matches!(ca_deref, Value::Null) {
            csr.info.subject.clone()
        } else {
            match resolve_cert(&ca_arg) {
                Some(c) => c.tbs_certificate.subject.clone(),
                None => {
                    vm.emit_warning("openssl_csr_sign(): X.509 Certificate cannot be retrieved");
                    return Ok(Value::False);
                }
            }
        }
    };

    let serial_bytes: Vec<u8> = {
        let sh_deref = match &serial_hex_arg {
            Value::Reference(r) => r.borrow().clone(),
            other => other.clone(),
        };
        match sh_deref {
            Value::Null => i64_to_be_bytes_no_leading_zeros(serial.max(0)),
            _ => {
                let s = sh_deref.to_php_string().to_string_lossy().to_string();
                if s.is_empty() {
                    i64_to_be_bytes_no_leading_zeros(serial.max(0))
                } else {
                    if !s.chars().all(|c| c.is_ascii_hexdigit()) {
                        vm.emit_warning("openssl_csr_sign(): Error parsing serial number");
                        return Ok(Value::False);
                    }
                    if s.len() > 80 {
                        vm.emit_warning(
                            "openssl_csr_sign(): Error parsing serial number because it is too long",
                        );
                        return Ok(Value::False);
                    }
                    hex_to_bytes(&s).unwrap_or_else(|| vec![1])
                }
            }
        }
    };

    let serial_num = match SerialNumber::new(&serial_bytes) {
        Ok(s) => s,
        Err(_) => return Ok(Value::False),
    };

    // Sanity check on days: openssl uses 30-bit-ish int for days; refuse
    // crazy values to avoid Duration overflow and stack overflow.
    if !(0..=24855).contains(&days) {
        vm.emit_warning(
            "openssl_csr_sign(): Days must be between 0 and 24855",
        );
        return Ok(Value::False);
    }
    let now = std::time::SystemTime::now();
    let then = now + std::time::Duration::from_secs((days as u64) * 86400);
    let not_before = Time::try_from(now).unwrap_or(Time::INFINITY);
    let not_after = Time::try_from(then).unwrap_or(Time::INFINITY);
    let validity = Validity {
        not_before,
        not_after,
    };

    let digest_name = read_digest_alg_from_options(&options_arg).unwrap_or_else(|| "sha256".into());
    let sig_oid_str = sig_oid_for_digest(&digest_name);
    let sig_oid = match ObjectIdentifier::new(&sig_oid_str) {
        Ok(o) => o,
        Err(_) => return Ok(Value::False),
    };
    let sig_alg = AlgorithmIdentifierOwned {
        oid: sig_oid,
        parameters: Some(Any::null()),
    };

    let tbs = TbsCertificate {
        version: Version::V3,
        serial_number: serial_num,
        signature: sig_alg.clone(),
        issuer: issuer_name,
        validity,
        subject: csr.info.subject.clone(),
        subject_public_key_info: csr.info.public_key.clone(),
        issuer_unique_id: None,
        subject_unique_id: None,
        extensions: None,
    };

    let tbs_der = match tbs.to_der() {
        Ok(d) => d,
        Err(_) => return Ok(Value::False),
    };
    let signature_bytes = match sign_with_digest(&priv_key, &digest_name, &tbs_der) {
        Some(b) => b,
        None => return Ok(Value::False),
    };
    let sig_bs = match BitString::from_bytes(&signature_bytes) {
        Ok(b) => b,
        Err(_) => return Ok(Value::False),
    };

    let cert = Certificate {
        tbs_certificate: tbs,
        signature_algorithm: sig_alg,
        signature: sig_bs,
    };

    Ok(make_cert_object(vm, cert))
}

fn i64_to_be_bytes_no_leading_zeros(n: i64) -> Vec<u8> {
    if n == 0 {
        return vec![0];
    }
    let bytes = n.to_be_bytes();
    let mut start = 0;
    while start < bytes.len() - 1 && bytes[start] == 0 {
        start += 1;
    }
    let mut v = bytes[start..].to_vec();
    if v[0] & 0x80 != 0 {
        v.insert(0, 0);
    }
    v
}

fn hex_to_bytes(s: &str) -> Option<Vec<u8>> {
    let s = if s.len() % 2 == 1 {
        format!("0{}", s)
    } else {
        s.to_string()
    };
    let mut out = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let byte = u8::from_str_radix(&s[i..i + 2], 16).ok()?;
        out.push(byte);
    }
    Some(out)
}
