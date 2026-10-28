//! openssl_pkcs7_*() and openssl_cms_*() (PHP manual: OpenSSL functions;
//! PKCS #7 / CMS and S/MIME in goro-ext-openssl's cms module).

use std::rc::Rc;

use goro_ext_openssl as ossl;
use goro_runtime::{KeyRef, PhpArray, Value};
use ossl::cms::{self, ContentCipher, SignOpts, VerifyOpts};
use ossl::err::{self as oerr, E, errs};
use ossl::x509::Cert;

use super::*;

const F_TEXT: i64 = 1;
const F_NOCERTS: i64 = 2;
const F_NOSIGS: i64 = 4;
const F_NOCHAIN: i64 = 8;
const F_NOINTERN: i64 = 16;
const F_NOVERIFY: i64 = 32;
const F_DETACHED: i64 = 64;
const F_BINARY: i64 = 128;
const F_NOATTR: i64 = 256;
const F_NOSMIMECAP: i64 = 512;
const F_NOOLDMIMETYPE: i64 = 1024;

const ENC_DER: i64 = 0;
const ENC_SMIME: i64 = 1;
const ENC_PEM: i64 = 2;

fn read_file(p: &[u8]) -> Option<Vec<u8>> {
    if p.is_empty() {
        return None;
    }
    std::fs::read(os_path(p)).ok()
}

fn headers_text(vm: &mut Vm, h: &Value) -> R<Vec<u8>> {
    let mut out = Vec::new();
    if let Value::Array(arr) = h.deref() {
        for (k, v) in arr.iter() {
            let val = vm.to_php_str(&v.deref())?;
            match k {
                KeyRef::Str(name) => {
                    out.extend_from_slice(name.as_bytes());
                    out.extend_from_slice(b": ");
                    out.extend_from_slice(val.as_bytes());
                }
                KeyRef::Int(_) => out.extend_from_slice(val.as_bytes()),
            }
            out.push(b'\n');
        }
    }
    Ok(out)
}

/// Certificates of a `$certificate` argument that may be a list.
fn cert_list(vm: &mut Vm, a: &Args, v: &Value) -> R<Option<Vec<Cert>>> {
    let items: Vec<Value> = match v.deref() {
        Value::Array(arr) => arr.iter().map(|(_, x)| x.deref()).collect(),
        other => vec![other],
    };
    let mut out = Vec::new();
    for it in items {
        match cert_from_value(vm, a, &it)? {
            Some(c) => out.push((*c).clone()),
            None => return Ok(None),
        }
    }
    Ok(Some(out))
}

fn certs_from_file(path: Option<PhpStr>) -> Option<Vec<Cert>> {
    match path {
        None => Some(Vec::new()),
        Some(p) => read_file(p.as_bytes()).map(|d| ossl::x509::certs_from_bytes(&d)),
    }
}

fn prepare_content(data: &[u8], flags: i64) -> Vec<u8> {
    let mut c = Vec::new();
    if flags & F_TEXT != 0 && flags & F_BINARY == 0 {
        c.extend_from_slice(b"Content-Type: text/plain\r\n\r\n");
    }
    if flags & F_BINARY != 0 {
        c.extend_from_slice(data);
    } else {
        c.extend_from_slice(&cms::crlf(data));
    }
    c
}

/// Whether the pre-RFC 5751 `application/x-pkcs7-*` types are written:
/// PKCS #7 defaults to them (PKCS7_NOOLDMIMETYPE turns them off), CMS
/// writes them only with OPENSSL_CMS_OLDMIMETYPE (same bit).
fn old_mime(is_cms: bool, flags: i64) -> bool {
    if is_cms {
        flags & F_NOOLDMIMETYPE != 0
    } else {
        flags & F_NOOLDMIMETYPE == 0
    }
}

fn push_queue(es: &[E]) {
    for e in es {
        oerr::push(*e);
    }
}

// ------------------------------------------------------------ signing

/// Shared body of openssl_pkcs7_sign() and openssl_cms_sign().
#[allow(clippy::too_many_arguments)]
fn sign_common(
    vm: &mut Vm,
    a: &Args,
    flags: i64,
    encoding: i64,
    extra_file: Option<PhpStr>,
    is_cms: bool,
) -> R<Value> {
    if is_cms && encoding == ENC_SMIME && flags & F_DETACHED != 0 {
        vm.fwarn(a, "Detached signatures not possible with S/MIME encoding")?;
        return Ok(Value::Bool(false));
    }
    let infile = a.str(0);
    let outfile = a.str(1);
    let extra = match certs_from_file(extra_file) {
        Some(e) => e,
        None => {
            vm.fwarn(a, "Error loading extra certs")?;
            return Ok(Value::Bool(false));
        }
    };
    let Some(key) = key_from_value(vm, a, &a.val(3), Want::Private, None)? else {
        kwarn(vm, a, "Error getting private key")?;
        return Ok(Value::Bool(false));
    };
    let Some(cert) = cert_from_value(vm, a, &a.val(2))? else {
        vm.fwarn(a, "X.509 Certificate cannot be retrieved")?;
        return Ok(Value::Bool(false));
    };
    if !vm.basedir_allows(&infile) || !vm.basedir_allows(&outfile) {
        return Ok(Value::Bool(false));
    }
    let Some(data) = read_file(&infile) else {
        vm.fwarn(
            a,
            &format!("Error opening input file {}!", infile.to_string_lossy()),
        )?;
        return Ok(Value::Bool(false));
    };
    if outfile.is_empty() {
        vm.fwarn(
            a,
            &format!("Error opening output file {}!", outfile.to_string_lossy()),
        )?;
        return Ok(Value::Bool(false));
    }
    let headers = headers_text(vm, &a.val(4))?;
    let content = prepare_content(&data, flags);
    let md = ossl::digest::Md::H(ossl::digest::HashAlgorithm::Sha256);
    let opts = SignOpts {
        detached: flags & F_DETACHED != 0,
        no_attrs: flags & F_NOATTR != 0,
        no_certs: flags & F_NOCERTS != 0,
        no_smime_cap: flags & F_NOSMIMECAP != 0,
        cms: is_cms,
    };
    let Some(p7) = cms::sign(&content, &cert, &key, &extra, md, opts, super::x509::now()) else {
        return Ok(Value::Bool(false));
    };
    let mut out = if encoding == ENC_SMIME {
        headers
    } else {
        Vec::new()
    };
    match encoding {
        ENC_DER => out.extend_from_slice(&p7),
        ENC_PEM => out.extend_from_slice(ossl::pem::encode("CMS", &p7).as_bytes()),
        _ => {
            if opts.detached {
                out.extend_from_slice(&cms::smime_signed_detached(
                    &content,
                    &p7,
                    cms::micalg(md),
                    old_mime(is_cms, flags),
                ));
            } else {
                out.extend_from_slice(&cms::smime_opaque(
                    &p7,
                    "signed-data",
                    old_mime(is_cms, flags),
                ));
            }
        }
    }
    match std::fs::write(os_path(&outfile), &out) {
        Ok(()) => Ok(Value::Bool(true)),
        Err(_) => {
            vm.fwarn(
                a,
                &format!("Error opening output file {}!", outfile.to_string_lossy()),
            )?;
            Ok(Value::Bool(false))
        }
    }
}

pub(super) fn openssl_pkcs7_sign(vm: &mut Vm, a: &mut Args) -> R<Value> {
    check_path_args(vm, a, &[0, 1, 6])?;
    let flags = a.int_or(5, F_DETACHED);
    let extra = a.opt_str(6);
    sign_common(vm, a, flags, ENC_SMIME, extra, false)
}

pub(super) fn openssl_cms_sign(vm: &mut Vm, a: &mut Args) -> R<Value> {
    check_path_args(vm, a, &[0, 1, 7])?;
    let flags = a.int_or(5, 0);
    let enc = a.int_or(6, ENC_SMIME);
    let extra = a.opt_str(7);
    sign_common(vm, a, flags, enc, extra, true)
}

// ------------------------------------------------------------ verification

struct VerifyArgs {
    infile: PhpStr,
    flags: i64,
    signers_out: Option<PhpStr>,
    ca_info: PhpArray,
    untrusted: Option<PhpStr>,
    content_out: Option<PhpStr>,
    p7_out: Option<PhpStr>,
    sigfile: Option<PhpStr>,
    encoding: i64,
    is_cms: bool,
}

fn verify_common(vm: &mut Vm, a: &Args, v: VerifyArgs) -> R<Value> {
    let fail = if v.is_cms {
        Value::Bool(false)
    } else {
        Value::Int(-1)
    };
    if v.sigfile.is_some() && v.encoding == ENC_SMIME {
        vm.fwarn(a, "Detached signatures not possible with S/MIME encoding")?;
        return Ok(Value::Bool(false));
    }
    let trusted = super::x509::load_ca_info(vm, a, &v.ca_info)?.unwrap_or_default();
    if !vm.basedir_allows(&v.infile) {
        basedir_warning(vm, a, &v.infile)?;
        return Ok(fail);
    }
    let Some(data) = read_file(&v.infile) else {
        return Ok(fail);
    };
    // The message (and detached content).
    for p in [&v.sigfile, &v.signers_out, &v.content_out, &v.p7_out]
        .into_iter()
        .flatten()
    {
        if !vm.basedir_allows(p) {
            basedir_warning(vm, a, p)?;
            return Ok(Value::Bool(false));
        }
    }
    let (p7, mut detached) = match (&v.sigfile, v.encoding) {
        (Some(sf), enc) => {
            let Some(sig) = read_file(sf.as_bytes()) else {
                return Ok(fail);
            };
            let p7 = if enc == ENC_PEM {
                match ossl::pem::parse_all(&sig).into_iter().next() {
                    Some(b) => b.data,
                    None => return Ok(fail),
                }
            } else {
                sig
            };
            (p7, Some(data))
        }
        (None, ENC_DER) => (data, None),
        (None, ENC_PEM) => match ossl::pem::parse_all(&data).into_iter().next() {
            Some(b) => (b.data, None),
            None => return Ok(fail),
        },
        (None, _) => match cms::smime_read(&data) {
            Some(s) => (s.p7, s.content),
            None => {
                oerr::push(E::new(
                    0x0D06B08E,
                    "asn1 encoding routines",
                    "not enough data",
                ));
                return Ok(fail);
            }
        },
    };
    let Ok((ctype, inner)) = cms::content_info(&p7) else {
        return Ok(fail);
    };
    if ctype != ossl::oid::ids::SIGNED_DATA {
        return Ok(fail);
    }
    let Ok(sd) = cms::parse_signed(&inner) else {
        return Ok(fail);
    };
    if let Some(c) = &detached
        && v.flags & F_BINARY == 0
    {
        detached = Some(cms::crlf(c));
    }
    let extra = match certs_from_file(v.untrusted.clone()) {
        Some(e) => e,
        None => return Ok(fail),
    };
    let opts = VerifyOpts {
        no_verify: v.flags & F_NOVERIFY != 0,
        no_intern: v.flags & F_NOINTERN != 0,
        no_sigs: v.flags & F_NOSIGS != 0,
        no_chain: v.flags & F_NOCHAIN != 0,
    };
    match cms::verify(
        &sd,
        detached.as_deref(),
        &trusted,
        &extra,
        opts,
        super::x509::now(),
    ) {
        Ok(signers) => {
            if let Some(f) = &v.signers_out {
                let pem: String = signers.iter().map(|c| c.to_pem()).collect();
                if std::fs::write(os_path(f.as_bytes()), pem).is_err() {
                    vm.fwarn(
                        a,
                        &format!(
                            "Signature OK, but cannot open {} for writing",
                            f.to_string_lossy()
                        ),
                    )?;
                    return Ok(fail);
                }
            }
            if let Some(f) = &v.content_out {
                let content = detached.clone().or(sd.content.clone()).unwrap_or_default();
                let _ = std::fs::write(os_path(f.as_bytes()), content);
            }
            if let Some(f) = &v.p7_out {
                let label = if v.is_cms { "CMS" } else { "PKCS7" };
                let _ = std::fs::write(os_path(f.as_bytes()), ossl::pem::encode(label, &p7));
            }
            Ok(Value::Bool(true))
        }
        Err(e) => {
            match e {
                cms::CmsError::CertVerify(_) => push_queue(&[E::new(
                    0x10800075,
                    "PKCS7 routines",
                    "certificate verify error",
                )]),
                cms::CmsError::BadSignature => {
                    push_queue(&[E::new(0x10800069, "PKCS7 routines", "digest failure")])
                }
                _ => push_queue(&[E::new(
                    0x1080006A,
                    "PKCS7 routines",
                    "signer certificate not found",
                )]),
            }
            Ok(Value::Bool(false))
        }
    }
}

pub(super) fn openssl_pkcs7_verify(vm: &mut Vm, a: &mut Args) -> R<Value> {
    check_path_args(vm, a, &[0, 2, 4, 5, 6])?;
    let v = VerifyArgs {
        infile: a.str(0),
        flags: a.int(1),
        signers_out: a.opt_str(2),
        ca_info: a.array(3),
        untrusted: a.opt_str(4),
        content_out: a.opt_str(5),
        p7_out: a.opt_str(6),
        sigfile: None,
        encoding: ENC_SMIME,
        is_cms: false,
    };
    verify_common(vm, a, v)
}

pub(super) fn openssl_cms_verify(vm: &mut Vm, a: &mut Args) -> R<Value> {
    check_path_args(vm, a, &[0, 2, 4, 5, 6, 7])?;
    let v = VerifyArgs {
        infile: a.str(0),
        flags: a.int_or(1, 0),
        signers_out: a.opt_str(2),
        ca_info: a.array(3),
        untrusted: a.opt_str(4),
        content_out: a.opt_str(5),
        p7_out: a.opt_str(6),
        sigfile: a.opt_str(7),
        encoding: a.int_or(8, ENC_SMIME),
        is_cms: true,
    };
    verify_common(vm, a, v)
}

// ------------------------------------------------------------ encryption

fn cipher_from_const(n: i64) -> Option<ContentCipher> {
    use ossl::pbe::CbcCipher;
    Some(ContentCipher::Cbc(match n {
        4 => CbcCipher::DesEde3,
        5 => CbcCipher::Aes128,
        6 => CbcCipher::Aes192,
        7 => CbcCipher::Aes256,
        _ => return None,
    }))
}

fn encrypt_common(
    vm: &mut Vm,
    a: &Args,
    flags: i64,
    encoding: i64,
    cipher: Option<ContentCipher>,
    is_cms: bool,
) -> R<Value> {
    let infile = a.str(0);
    let outfile = a.str(1);
    let Some(certs) = cert_list(vm, a, &a.val(2))? else {
        return Ok(Value::Bool(false));
    };
    let Some(cipher) = cipher else {
        oerr::push(errs::EVP_UNSUPPORTED);
        return Ok(Value::Bool(false));
    };
    let Some(data) = read_file(&infile) else {
        return Ok(Value::Bool(false));
    };
    if outfile.is_empty() {
        return Ok(Value::Bool(false));
    }
    let headers = headers_text(vm, &a.val(3))?;
    let content = if flags & F_TEXT != 0 {
        prepare_content(&data, flags)
    } else {
        data
    };
    let Some(p7) = cms::envelope(&content, &certs, cipher) else {
        return Ok(Value::Bool(false));
    };
    let mut out = if encoding == ENC_SMIME {
        headers
    } else {
        Vec::new()
    };
    match encoding {
        ENC_DER => out.extend_from_slice(&p7),
        ENC_PEM => out.extend_from_slice(ossl::pem::encode("CMS", &p7).as_bytes()),
        _ => {
            let old = if is_cms {
                flags & F_NOOLDMIMETYPE != 0
            } else {
                flags & F_NOOLDMIMETYPE == 0
            };
            let st = if matches!(cipher, ContentCipher::Gcm(_)) {
                "authEnveloped-data"
            } else {
                "enveloped-data"
            };
            out.extend_from_slice(&cms::smime_opaque(&p7, st, old));
        }
    }
    Ok(Value::Bool(std::fs::write(os_path(&outfile), &out).is_ok()))
}

pub(super) fn openssl_pkcs7_encrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    check_path_args(vm, a, &[0, 1])?;
    let flags = a.int_or(4, 0);
    let c = cipher_from_const(a.int_or(5, 5));
    encrypt_common(vm, a, flags, ENC_SMIME, c, false)
}

pub(super) fn openssl_cms_encrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    check_path_args(vm, a, &[0, 1])?;
    let flags = a.int_or(4, 0);
    let enc = a.int_or(5, ENC_SMIME);
    let c = match a.val(6) {
        Value::Str(s) => ContentCipher::by_name(&s.to_string_lossy()),
        Value::Null => cipher_from_const(5),
        other => cipher_from_const(match other {
            Value::Int(n) => n,
            _ => 5,
        }),
    };
    encrypt_common(vm, a, flags, enc, c, true)
}

fn decrypt_common(vm: &mut Vm, a: &Args, encoding: i64) -> R<Value> {
    let infile = a.str(0);
    let outfile = a.str(1);
    let Some(cert) = cert_from_value(vm, a, &a.val(2))? else {
        vm.fwarn(a, "X.509 Certificate cannot be retrieved")?;
        return Ok(Value::Bool(false));
    };
    let keyv = if a.has(3) && !matches!(a.val(3), Value::Null) {
        a.val(3)
    } else {
        a.val(2)
    };
    let Some(key) = key_from_value(vm, a, &keyv, Want::Private, None)? else {
        kwarn(vm, a, "Unable to get private key")?;
        return Ok(Value::Bool(false));
    };
    let Some(data) = read_file(&infile) else {
        return Ok(Value::Bool(false));
    };
    let p7 = match encoding {
        ENC_DER => data,
        ENC_PEM => match ossl::pem::parse_all(&data).into_iter().next() {
            Some(b) => b.data,
            None => return Ok(Value::Bool(false)),
        },
        _ => match cms::smime_read(&data) {
            Some(s) => s.p7,
            None => return Ok(Value::Bool(false)),
        },
    };
    let Ok((ctype, inner)) = cms::content_info(&p7) else {
        return Ok(Value::Bool(false));
    };
    match cms::decrypt(&ctype, &inner, Some(&cert), &key) {
        Ok(pt) => Ok(Value::Bool(std::fs::write(os_path(&outfile), pt).is_ok())),
        Err(_) => {
            oerr::push(E::new(0x10800073, "PKCS7 routines", "decrypt error"));
            Ok(Value::Bool(false))
        }
    }
}

pub(super) fn openssl_pkcs7_decrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    check_path_args(vm, a, &[0, 1])?;
    decrypt_common(vm, a, ENC_SMIME)
}

pub(super) fn openssl_cms_decrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    check_path_args(vm, a, &[0, 1])?;
    let enc = a.int_or(4, ENC_SMIME);
    decrypt_common(vm, a, enc)
}

// ------------------------------------------------------------ reading

fn read_common(vm: &mut Vm, a: &Args, label_ok: &[&str]) -> R<Value> {
    let data = a.str(0);
    let _ = vm;
    let blocks = ossl::pem::parse_all(data.as_bytes());
    let Some(b) = blocks.iter().find(|b| label_ok.contains(&b.label.as_str())) else {
        oerr::push(errs::PEM_NO_START_LINE);
        return Ok(Value::Bool(false));
    };
    let Ok((ctype, inner)) = cms::content_info(&b.data) else {
        return Ok(Value::Bool(false));
    };
    let certs = if ctype == ossl::oid::ids::SIGNED_DATA {
        match cms::parse_signed(&inner) {
            Ok(sd) => sd.certs,
            Err(_) => return Ok(Value::Bool(false)),
        }
    } else {
        Vec::new()
    };
    let mut arr = PhpArray::new();
    for c in certs {
        let _ = arr.push(str_val(&c.to_pem()));
    }
    set_out(a, 1, Value::Array(arr));
    Ok(Value::Bool(true))
}

pub(super) fn openssl_pkcs7_read(vm: &mut Vm, a: &mut Args) -> R<Value> {
    read_common(vm, a, &["PKCS7", "CMS"])
}

pub(super) fn openssl_cms_read(vm: &mut Vm, a: &mut Args) -> R<Value> {
    read_common(vm, a, &["CMS", "PKCS7"])
}

#[allow(dead_code)]
fn unused(_: Rc<()>) {}
