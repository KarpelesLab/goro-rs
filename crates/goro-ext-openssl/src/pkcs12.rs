//! PKCS#12 (RFC 7292) archives with OpenSSL 3 `PKCS12_create()` /
//! `PKCS12_parse()` semantics, and the php-src openssl_pkcs12_* functions.

use crate::asn1::{self, Tlv};
use crate::cert::Cert;
use crate::cipher;
use crate::digest::{self, Md};
use crate::pkey::PKey;
use crate::{err, pkey_fns, x509, zpp};
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

const OID_DATA: &str = "1.2.840.113549.1.7.1";
const OID_ENCRYPTED_DATA: &str = "1.2.840.113549.1.7.6";
const OID_KEY_BAG: &str = "1.2.840.113549.1.12.10.1.1";
const OID_SHROUDED_KEY_BAG: &str = "1.2.840.113549.1.12.10.1.2";
const OID_CERT_BAG: &str = "1.2.840.113549.1.12.10.1.3";
const OID_SAFE_CONTENTS_BAG: &str = "1.2.840.113549.1.12.10.1.6";
const OID_X509_CERT: &str = "1.2.840.113549.1.9.22.1";
const OID_FRIENDLY_NAME: &str = "1.2.840.113549.1.9.20";
const OID_LOCAL_KEY_ID: &str = "1.2.840.113549.1.9.21";

const LIB_PKCS12: u32 = 35;
/// PKCS12_DEFAULT_ITER
const DEFAULT_ITER: u32 = 2048;

/// OPENSSL_utf82uni(): UTF-16BE with a two-byte terminator. A missing
/// password (NULL) derives from an empty string without terminator.
fn bmp_password(pass: Option<&[u8]>) -> Vec<u8> {
    let Some(p) = pass else { return Vec::new() };
    let mut out = Vec::with_capacity(p.len() * 2 + 2);
    for c in String::from_utf8_lossy(p).encode_utf16() {
        out.extend_from_slice(&c.to_be_bytes());
    }
    out.extend_from_slice(&[0, 0]);
    out
}

/// RFC 7292 Appendix B key derivation (PKCS12_key_gen_uni()).
pub fn kdf(md: Md, pass_bmp: &[u8], salt: &[u8], iter: u32, id: u8, n: usize) -> Vec<u8> {
    let u = md.size();
    let v = md.block_size();
    let d = vec![id; v];
    let fill = |src: &[u8]| -> Vec<u8> {
        if src.is_empty() {
            return Vec::new();
        }
        let len = v * src.len().div_ceil(v);
        (0..len).map(|i| src[i % src.len()]).collect()
    };
    let mut i_buf = [fill(salt), fill(pass_bmp)].concat();
    let mut out = Vec::with_capacity(n + u);
    loop {
        let mut a = md.digest(&[d.as_slice(), i_buf.as_slice()].concat());
        for _ in 1..iter.max(1) {
            a = md.digest(&a);
        }
        out.extend_from_slice(&a);
        if out.len() >= n {
            out.truncate(n);
            return out;
        }
        let b: Vec<u8> = (0..v).map(|k| a[k % u]).collect();
        for chunk in i_buf.chunks_mut(v) {
            let mut carry = 1u16;
            for k in (0..v).rev() {
                let s = chunk[k] as u16 + b[k] as u16 + carry;
                chunk[k] = s as u8;
                carry = s >> 8;
            }
        }
    }
}

/// PKCS#12 KDF with a UTF-8 password (for the pbeWithSHAAnd* schemes).
pub fn kdf_str(password: &str, salt: &[u8], iter: u32, n: usize, id: u8) -> Vec<u8> {
    kdf(
        Md::Sha1,
        &bmp_password(Some(password.as_bytes())),
        salt,
        iter,
        id,
        n,
    )
}

fn random(n: usize) -> Vec<u8> {
    use purecrypto::rng::RngCore;
    let mut v = vec![0u8; n];
    purecrypto::rng::OsRng.fill_bytes(&mut v);
    v
}

fn bmp_string(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for c in String::from_utf8_lossy(s).encode_utf16() {
        out.extend_from_slice(&c.to_be_bytes());
    }
    asn1::tlv(0x1e, &out)
}

fn bag_attributes(friendly_name: Option<&[u8]>, local_key_id: Option<&[u8]>) -> Option<Vec<u8>> {
    let mut attrs = Vec::new();
    if let Some(n) = friendly_name {
        attrs.push(asn1::seq(&[
            asn1::oid(OID_FRIENDLY_NAME),
            asn1::set(&[bmp_string(n)]),
        ]));
    }
    if let Some(id) = local_key_id {
        attrs.push(asn1::seq(&[
            asn1::oid(OID_LOCAL_KEY_ID),
            asn1::set(&[asn1::octet(id)]),
        ]));
    }
    if attrs.is_empty() {
        None
    } else {
        Some(asn1::set(&attrs))
    }
}

fn safe_bag(oid: &str, value: Vec<u8>, attrs: Option<Vec<u8>>) -> Vec<u8> {
    let mut items = vec![asn1::oid(oid), asn1::explicit(0, &value)];
    if let Some(a) = attrs {
        items.push(a);
    }
    asn1::seq(&items)
}

/// PKCS12_create() with OpenSSL 3 defaults: AES-256-CBC/PBKDF2-SHA256 for
/// both the shrouded key and the certificate SafeContents, SHA-256 MAC.
pub fn create(
    pass: &[u8],
    friendly_name: Option<&[u8]>,
    key: &PKey,
    cert: &Cert,
    ca: &[Rc<Cert>],
) -> Option<Vec<u8>> {
    let spec = cipher::by_name("aes-256-cbc")?;
    let key_id = Md::Sha1.digest(&cert.der);
    let mut cert_bags = vec![safe_bag(
        OID_CERT_BAG,
        asn1::seq(&[
            asn1::oid(OID_X509_CERT),
            asn1::explicit(0, &asn1::octet(&cert.der)),
        ]),
        bag_attributes(friendly_name, Some(&key_id)),
    )];
    for c in ca {
        cert_bags.push(safe_bag(
            OID_CERT_BAG,
            asn1::seq(&[
                asn1::oid(OID_X509_CERT),
                asn1::explicit(0, &asn1::octet(&c.der)),
            ]),
            None,
        ));
    }
    let certs_sc = asn1::seq(&cert_bags);
    // EncryptedData: reuse the PKCS#8 PBES2 encoder for AlgorithmIdentifier + ciphertext
    let enc = crate::pkey::pkcs8_encrypt(&certs_sc, spec, pass)?;
    let enc_t = asn1::parse_one(&enc)?;
    let enc_c = enc_t.children()?;
    let eci = asn1::seq(&[
        asn1::oid(OID_DATA),
        enc_c[0].raw.to_vec(),
        asn1::implicit(0, enc_c[1].content),
    ]);
    let encrypted_data = asn1::seq(&[asn1::int_i64(0), eci]);
    let ci_certs = asn1::seq(&[
        asn1::oid(OID_ENCRYPTED_DATA),
        asn1::explicit(0, &encrypted_data),
    ]);

    let shrouded = crate::pkey::pkcs8_encrypt(&key.pkcs8_der(), spec, pass)?;
    let key_sc = asn1::seq(&[safe_bag(
        OID_SHROUDED_KEY_BAG,
        shrouded,
        bag_attributes(friendly_name, Some(&key_id)),
    )]);
    let ci_key = asn1::seq(&[
        asn1::oid(OID_DATA),
        asn1::explicit(0, &asn1::octet(&key_sc)),
    ]);

    let auth_safe = asn1::seq(&[ci_certs, ci_key]);
    let salt = random(8);
    let mac_key = kdf(
        Md::Sha256,
        &bmp_password(Some(pass)),
        &salt,
        DEFAULT_ITER,
        3,
        32,
    );
    let mac = digest::hmac(Md::Sha256, &mac_key, &auth_safe);
    let mac_data = asn1::seq(&[
        asn1::seq(&[
            asn1::algid(Md::Sha256.oid()?, Some(asn1::null())),
            asn1::octet(&mac),
        ]),
        asn1::octet(&salt),
        asn1::int_i64(DEFAULT_ITER as i64),
    ]);
    Some(asn1::seq(&[
        asn1::int_i64(3),
        asn1::seq(&[
            asn1::oid(OID_DATA),
            asn1::explicit(0, &asn1::octet(&auth_safe)),
        ]),
        mac_data,
    ]))
}

fn octets(t: &Tlv) -> Vec<u8> {
    if t.constructed() {
        let mut v = Vec::new();
        for c in t.children().unwrap_or_default() {
            v.extend(octets(&c));
        }
        return v;
    }
    t.content.to_vec()
}

/// Content of a ContentInfo's [0] (Data: the OCTET STRING payload).
fn data_content(ci: &[Tlv]) -> Option<Vec<u8>> {
    let e = ci.get(1)?;
    if e.context() != Some(0) {
        return None;
    }
    let inner = asn1::parse(e.content)?.0;
    Some(octets(&inner))
}

#[derive(Default)]
struct Parsed {
    keys: Vec<PKey>,
    certs: Vec<Cert>,
}

fn parse_bags(sc: &[u8], pass: Option<&[u8]>, out: &mut Parsed) -> bool {
    let Some(bags) = asn1::parse_one(sc).and_then(|t| t.children()) else {
        return false;
    };
    for bag in bags {
        let Some(f) = bag.children() else {
            return false;
        };
        let Some(oid) = f.first().and_then(|o| o.oid()) else {
            return false;
        };
        let Some(val) = f
            .get(1)
            .filter(|v| v.context() == Some(0))
            .and_then(|v| asn1::parse(v.content))
            .map(|x| x.0)
        else {
            return false;
        };
        match oid.as_str() {
            OID_KEY_BAG => {
                if let Some(k) = crate::pkey::parse_pkcs8(val.raw) {
                    out.keys.push(k);
                }
            }
            OID_SHROUDED_KEY_BAG => {
                let pw = pass.unwrap_or(b"");
                match crate::pkey::pkcs8_decrypt(val.raw, pw) {
                    Ok(plain) => {
                        if let Some(k) = crate::pkey::parse_pkcs8(&plain) {
                            out.keys.push(k);
                        }
                    }
                    Err(_) => {
                        err::push(LIB_PKCS12, 115, "pkcs12 pbe crypt error");
                        return false;
                    }
                }
            }
            OID_CERT_BAG => {
                let Some(cb) = val.children() else {
                    return false;
                };
                if cb.first().and_then(|o| o.oid()).as_deref() != Some(OID_X509_CERT) {
                    continue;
                }
                let Some(der) = cb
                    .get(1)
                    .and_then(|x| asn1::parse(x.content))
                    .map(|x| octets(&x.0))
                else {
                    return false;
                };
                match Cert::parse(&der) {
                    Some(c) => out.certs.push(c),
                    None => return false,
                }
            }
            OID_SAFE_CONTENTS_BAG => {
                if !parse_bags(val.raw, pass, out) {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

/// PKCS12_verify_mac()
fn verify_mac(auth_safe: &[u8], mac_data: &Tlv, pass: Option<&[u8]>) -> bool {
    let Some(m) = mac_data.children() else {
        return false;
    };
    let Some(di) = m.first().and_then(|d| d.children()) else {
        return false;
    };
    let Some(md) = di
        .first()
        .and_then(|a| a.children())
        .and_then(|a| a.first().and_then(|o| o.oid()))
        .and_then(|o| Md::by_name(&o))
    else {
        return false;
    };
    let Some(expected) = di.get(1).map(|d| d.content) else {
        return false;
    };
    let salt = m.get(1).map(|s| s.content).unwrap_or(&[]);
    let iter = m.get(2).and_then(|i| i.as_i64()).unwrap_or(1) as u32;
    let key = kdf(md, &bmp_password(pass), salt, iter, 3, md.size());
    digest::hmac(md, &key, auth_safe) == expected
}

/// d2i_PKCS12_bio() + PKCS12_parse(): (key, cert, ca certificates)
fn parse(der: &[u8], pass: &[u8]) -> Option<(Option<PKey>, Option<Cert>, Vec<Cert>)> {
    let Some(pfx) = asn1::parse_one(der).and_then(|t| t.children()) else {
        err::push(err::LIB_ASN1, 142, "not enough data");
        return None;
    };
    let auth = pfx.get(1)?.children()?;
    if auth.first()?.oid()? != OID_DATA {
        err::push(LIB_PKCS12, 121, "content type not data");
        return None;
    }
    let auth_safe = data_content(&auth)?;
    let mut pw: Option<&[u8]> = Some(pass);
    if let Some(mac) = pfx.get(2) {
        if pass.is_empty() {
            if verify_mac(&auth_safe, mac, None) {
                pw = None;
            } else if !verify_mac(&auth_safe, mac, Some(b"")) {
                err::push(LIB_PKCS12, 113, "mac verify failure");
                return None;
            }
        } else if !verify_mac(&auth_safe, mac, Some(pass)) {
            err::push(LIB_PKCS12, 113, "mac verify failure");
            return None;
        }
    } else if pass.is_empty() {
        pw = None;
    }
    let mut parsed = Parsed::default();
    let cis = asn1::parse_one(&auth_safe)?.children()?;
    for ci in cis {
        let c = ci.children()?;
        match c.first()?.oid()?.as_str() {
            OID_DATA => {
                if !parse_bags(&data_content(&c)?, pw, &mut parsed) {
                    err::push(LIB_PKCS12, 114, "parse error");
                    return None;
                }
            }
            OID_ENCRYPTED_DATA => {
                let ed = asn1::parse(c.get(1)?.content)?.0;
                let f = ed.children()?;
                let eci = f.get(1)?.children()?;
                let alg = eci.get(1)?.raw.to_vec();
                let enc = eci
                    .get(2)
                    .map(|t| {
                        if t.constructed() {
                            octets(t)
                        } else {
                            t.content.to_vec()
                        }
                    })
                    .unwrap_or_default();
                let epki = asn1::seq(&[alg, asn1::octet(&enc)]);
                let pwb = pw.unwrap_or(b"");
                let mark = err::mark();
                let plain = match crate::pkey::pkcs8_decrypt(&epki, pwb) {
                    Ok(p) => p,
                    Err(_) => {
                        err::restore(mark);
                        err::push(LIB_PKCS12, 117, "pkcs12 cipherfinal error");
                        err::push(LIB_PKCS12, 115, "pkcs12 pbe crypt error");
                        return None;
                    }
                };
                if !parse_bags(&plain, pw, &mut parsed) {
                    err::push(LIB_PKCS12, 114, "parse error");
                    return None;
                }
            }
            _ => {}
        }
    }
    let key = parsed.keys.into_iter().next();
    let mut cert = None;
    let mut ca = Vec::new();
    for c in parsed.certs {
        if cert.is_none() {
            if let Some(k) = &key {
                if k.spki_der() == c.spki {
                    cert = Some(c);
                    continue;
                }
            }
        }
        ca.push(c);
    }
    Some((key, cert, ca))
}

// ---------------------------------------------------------------------------
// php-src functions
// ---------------------------------------------------------------------------

/// php_openssl_array_to_X509_sk()
fn array_to_certs(
    vm: &mut Vm,
    f: &str,
    v: &Value,
    arg_num: usize,
    option: &str,
) -> Result<Option<Vec<Rc<Cert>>>, VmError> {
    let mut out = Vec::new();
    if let Value::Array(a) = v.deref() {
        let items: Vec<Value> = a.borrow().iter().map(|(_, v)| v.clone()).collect();
        for it in items {
            match x509::cert_from_zval_opt(vm, f, arg_num, &it, true, Some(option))? {
                Some(c) => out.push(c),
                None => return Ok(None),
            }
        }
    } else {
        match x509::cert_from_zval_opt(vm, f, arg_num, v, false, Some(option))? {
            Some(c) => out.push(c),
            None => return Ok(None),
        }
    }
    Ok(Some(out))
}

struct ExportArgs {
    cert: Rc<Cert>,
    key: Rc<PKey>,
    pass: Vec<u8>,
    friendly_name: Option<Vec<u8>>,
    args: Option<Rc<RefCell<PhpArray>>>,
}

/// Shared prologue of openssl_pkcs12_export(_to_file)(): certificate, key,
/// key/cert match. `None` means the function returns false.
fn export_prologue(
    vm: &mut Vm,
    args: &[Value],
    f: &str,
    to_file: bool,
) -> Result<Option<ExportArgs>, VmError> {
    let a = x509::cert_param_pub(vm, args, 0, f, "certificate")?;
    if to_file {
        zpp::path(vm, args, 1, f, "output_filename")?;
    }
    let zpkey = args.get(2).cloned().unwrap_or(Value::Null);
    let pass = zpp::str(vm, args, 3, f, "passphrase")?.as_bytes().to_vec();
    let opts = if zpp::is_passed(args, 4) {
        Some(zpp::array(vm, args, 4, f, "options")?)
    } else {
        None
    };
    let r = x509::cert_from_arg_pub(vm, f, 1, a);
    let Some(cert) = x509::cert_failed(vm, f, r)? else {
        vm.emit_warning(&format!("{}(): X.509 Certificate cannot be retrieved", f));
        return Ok(None);
    };
    let Some(key) = pkey_fns::from_zval(vm, f, &zpkey, false, Some(&[]), 3)? else {
        if vm.current_exception.is_none() {
            vm.emit_warning(&format!("{}(): Cannot get private key from parameter 3", f));
        }
        return Ok(None);
    };
    if !x509::check_private_key(&cert.spki, &key) {
        vm.emit_warning(&format!("{}(): Private key does not correspond to cert", f));
        return Ok(None);
    }
    let friendly_name = opts.as_ref().and_then(|o| {
        match o
            .borrow()
            .get(&ArrayKey::String(PhpString::from_bytes(b"friendly_name")))
        {
            Some(Value::String(s)) => Some(s.as_bytes().to_vec()),
            _ => None,
        }
    });
    Ok(Some(ExportArgs {
        cert,
        key,
        pass,
        friendly_name,
        args: opts,
    }))
}

fn extracerts(
    vm: &mut Vm,
    f: &str,
    opts: &Option<Rc<RefCell<PhpArray>>>,
) -> Result<Option<Vec<Rc<Cert>>>, VmError> {
    let item = opts.as_ref().and_then(|o| {
        o.borrow()
            .get(&ArrayKey::String(PhpString::from_bytes(b"extracerts")))
            .cloned()
    });
    match item {
        Some(v) => array_to_certs(vm, f, &v, 5, "extracerts"),
        None => Ok(Some(Vec::new())),
    }
}

pub fn openssl_pkcs12_export_to_file(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_pkcs12_export_to_file";
    let Some(ea) = export_prologue(vm, args, f, true)? else {
        return Ok(Value::False);
    };
    let filename = zpp::path(vm, args, 1, f, "output_filename")?;
    let Some(path) = x509::check_plain_path(vm, f, 2, filename.as_bytes(), false, None)? else {
        return Ok(Value::False);
    };
    let Some(ca) = extracerts(vm, f, &ea.args)? else {
        return Ok(Value::False);
    };
    let Some(p12) = create(
        &ea.pass,
        ea.friendly_name.as_deref(),
        &ea.key,
        &ea.cert,
        &ca,
    ) else {
        return Ok(Value::False);
    };
    match std::fs::write(&path, p12) {
        Ok(()) => Ok(Value::True),
        Err(e) => {
            err::bio_new_file_failed(&e);
            vm.emit_warning(&format!("{}(): Error opening file {}", f, path));
            Ok(Value::False)
        }
    }
}

pub fn openssl_pkcs12_export(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_pkcs12_export";
    let Some(ea) = export_prologue(vm, args, f, false)? else {
        return Ok(Value::False);
    };
    let Some(ca) = extracerts(vm, f, &ea.args)? else {
        return Ok(Value::False);
    };
    let Some(p12) = create(
        &ea.pass,
        ea.friendly_name.as_deref(),
        &ea.key,
        &ea.cert,
        &ca,
    ) else {
        return Ok(Value::False);
    };
    zpp::set_ref(args, 1, zpp::bytes(p12));
    Ok(Value::True)
}

pub fn openssl_pkcs12_read(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_pkcs12_read";
    let data = zpp::str(vm, args, 0, f, "pkcs12")?;
    let pass = zpp::str(vm, args, 2, f, "passphrase")?;
    if data.as_bytes().is_empty() {
        // BIO_write() of zero bytes fails
        return Ok(Value::False);
    }
    let Some((key, cert, mut ca)) = parse(data.as_bytes(), pass.as_bytes()) else {
        return Ok(Value::False);
    };
    let mut out = PhpArray::new();
    let s = |v: String| Value::String(PhpString::from_string(v));
    if let Some(c) = &cert {
        out.set(ArrayKey::String(PhpString::from_bytes(b"cert")), s(c.pem()));
    }
    if let Some(k) = &key {
        if let Some(p) = k.private_pem(None, b"") {
            out.set(ArrayKey::String(PhpString::from_bytes(b"pkey")), s(p));
        }
    }
    if !ca.is_empty() {
        let mut extra = PhpArray::new();
        let mut i = 0;
        while let Some(c) = ca.pop() {
            extra.set(ArrayKey::Int(i), s(c.pem()));
            i += 1;
        }
        out.set(
            ArrayKey::String(PhpString::from_bytes(b"extracerts")),
            Value::Array(Rc::new(RefCell::new(extra))),
        );
    }
    zpp::set_ref(args, 1, Value::Array(Rc::new(RefCell::new(out))));
    Ok(Value::True)
}
