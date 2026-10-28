//! openssl_csr_*() (PHP manual: OpenSSL functions; the subject, attribute
//! and extension rules are described in goro-ext-openssl's csr module).

use std::rc::Rc;

use goro_ext_openssl as ossl;
use goro_runtime::{KeyRef, PhpArray, Value};
use ossl::csr::{Csr, StrError};
use ossl::err::{self as oerr, errs};
use ossl::x509::{Attr, Name};

use super::*;

pub(super) fn csr_from_value(vm: &mut Vm, a: &Args, v: &Value) -> R<Option<Rc<Csr>>> {
    match v.deref() {
        Value::Object(o) => Ok(with_native::<CsrData, _>(&o, |c| c.0.clone())),
        other => {
            let s = vm.to_php_str(&other)?;
            let Some(data) = arg_data(vm, a, s.as_bytes())? else {
                return Ok(None);
            };
            for b in ossl::pem::parse_all(&data) {
                if b.label == "CERTIFICATE REQUEST" || b.label == "NEW CERTIFICATE REQUEST" {
                    return Ok(Csr::from_der(&b.data).ok().map(Rc::new));
                }
            }
            if let Ok(c) = Csr::from_der(&data) {
                return Ok(Some(Rc::new(c)));
            }
            oerr::push(errs::PEM_NO_START_LINE);
            Ok(None)
        }
    }
}

fn csr_arg(vm: &mut Vm, a: &Args, i: usize) -> R<Option<Rc<Csr>>> {
    let r = csr_from_value(vm, a, &a.val(i))?;
    if r.is_none() {
        vm.fwarn(a, "X.509 Certificate Signing Request cannot be retrieved")?;
    }
    Ok(r)
}

fn str_error_queue(e: StrError) {
    match e {
        StrError::TooLong => oerr::push(errs::ASN1_STRING_TOO_LONG),
        StrError::TooShort => oerr::push(errs::ASN1_STRING_TOO_SHORT),
        StrError::IllegalChars => {}
    }
}

/// The value of a dn/attribute entry as a string.
fn entry_str(vm: &mut Vm, v: &Value) -> R<Vec<u8>> {
    Ok(vm.to_php_str(&v.deref())?.to_vec())
}

const ADD_FAILED: &str = "(failed; check error queue and value of string_mask OpenSSL option if illegal characters are reported)";

/// The subject from the `$distinguished_names` array and the config's
/// `*_default` entries. None after a warning when an entry can't be
/// encoded.
fn build_subject(vm: &mut Vm, a: &Args, req: &ossl::req::Req, dn: &PhpArray) -> R<Option<Name>> {
    let mut attrs: Vec<Attr> = Vec::new();
    for (k, v) in dn.iter() {
        let KeyRef::Str(kname) = k else {
            continue;
        };
        let kname = kname.to_string_lossy();
        let Some(oid) = req.resolve_oid(&kname) else {
            oerr::push(errs::OBJ_UNKNOWN_OBJECT_NAME);
            vm.fwarn(a, &format!("dn: {} is not a recognized name", kname))?;
            continue;
        };
        let values: Vec<Value> = match v.deref() {
            Value::Array(list) => list.iter().map(|(_, x)| x.deref()).collect(),
            other => vec![other],
        };
        for val in values {
            let s = entry_str(vm, &val)?;
            match ossl::csr::make_attr(&oid, &s, req.string_mask) {
                Ok(at) => attrs.push(at),
                Err(e) => {
                    str_error_queue(e);
                    let nid = ossl::oid::by_oid(&oid).map_or(0, |o| o.nid);
                    vm.fwarn(
                        a,
                        &format!(
                            "dn: add_entry_by_NID {} -> {} {}",
                            nid,
                            String::from_utf8_lossy(&s),
                            ADD_FAILED
                        ),
                    )?;
                    return Ok(None);
                }
            }
        }
    }
    // Defaults from the distinguished_name section.
    let dn_sec = req.get("distinguished_name").map(|s| s.to_string());
    if let Some(sec) = dn_sec
        && let Some(entries) = req.conf.section(&sec)
    {
        for (name, value) in entries {
            let Some(base) = name.strip_suffix("_default") else {
                continue;
            };
            let base = match base.split_once('.') {
                Some((pre, rest)) if pre.bytes().all(|c| c.is_ascii_digit()) => rest,
                _ => base,
            };
            let Some(oid) = req.resolve_oid(base) else {
                continue;
            };
            if attrs.iter().any(|x| x.oid == oid) {
                continue;
            }
            match ossl::csr::make_attr(&oid, value.as_bytes(), req.string_mask) {
                Ok(at) => attrs.push(at),
                Err(e) => {
                    str_error_queue(e);
                    vm.fwarn(
                        a,
                        &format!("dn: add_entry_by_txt {} -> {} {}", base, value, ADD_FAILED),
                    )?;
                    return Ok(None);
                }
            }
        }
    }
    Ok(Some(Name::from_attrs(attrs)))
}

/// The request attributes (config section entries, replaced by the
/// `$extra_attributes` values). None after a warning on failure.
fn build_attributes(
    vm: &mut Vm,
    a: &Args,
    req: &ossl::req::Req,
    extra: &PhpArray,
) -> R<Option<Vec<(String, Vec<Vec<u8>>)>>> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut ok = true;
    let mut given: Vec<(String, Vec<u8>)> = Vec::new();
    for (k, v) in extra.iter() {
        let KeyRef::Str(kname) = k else {
            vm.fwarn(a, "attributes: numeric fild names are not supported")?;
            continue;
        };
        let kname = kname.to_string_lossy();
        let Some(oid) = req.resolve_oid(&kname) else {
            vm.fwarn(
                a,
                &format!("attributes: {} is not a recognized attribute name", kname),
            )?;
            continue;
        };
        let s = entry_str(vm, &v)?;
        given.push((oid, s));
    }
    let attr_sec = req.get("attributes").map(|s| s.to_string());
    if let Some(sec) = attr_sec
        && let Some(entries) = req.conf.section(&sec)
    {
        for (name, value) in entries {
            let Some(oid) = req.resolve_oid(name) else {
                oerr::push(errs::OBJ_UNKNOWN_OBJECT_NAME);
                oerr::push(errs::OBJ_UNKNOWN_OBJECT_NAME);
                oerr::push(errs::X509_INVALID_FIELD_NAME);
                vm.fwarn(
                    a,
                    &format!("add1_attr_by_txt {} -> {} {}", name, value, ADD_FAILED),
                )?;
                ok = false;
                break;
            };
            let v = given
                .iter()
                .find(|(o, _)| *o == oid)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| value.as_bytes().to_vec());
            out.push((oid, v));
        }
    }
    if !ok {
        return Ok(None);
    }
    for (oid, v) in given {
        if !out.iter().any(|(o, _)| *o == oid) {
            out.push((oid, v));
        }
    }
    let mut res = Vec::new();
    for (oid, v) in out {
        // Attribute values use the same string rules as names.
        let (t, c) = match ossl::csr::encode_string(&oid, &v, req.string_mask) {
            Ok(x) => x,
            Err(e) => {
                str_error_queue(e);
                vm.fwarn(
                    a,
                    &format!(
                        "add1_attr_by_txt {} -> {} {}",
                        ossl::oid::sn(&oid),
                        String::from_utf8_lossy(&v),
                        ADD_FAILED
                    ),
                )?;
                return Ok(None);
            }
        };
        res.push((oid, vec![ossl::der::tlv(t as u8, &c)]));
    }
    Ok(Some(res))
}

/// The digest named by the settings; OpenSSL 3's default provider
/// refuses MD4 for signatures (observed on PHP 8.5).
pub(super) fn req_digest(
    req: &ossl::req::Req,
    key: &ossl::key::Key,
) -> Result<Option<ossl::digest::Md>, ()> {
    use ossl::key::KeyType;
    if matches!(key.key_type(), KeyType::Ed25519 | KeyType::Ed448) {
        return Ok(None);
    }
    let name = req.digest.clone().unwrap_or_else(|| "sha256".into());
    match ossl::digest::Md::by_name(&name) {
        Some(m) if m.name() == "md4" => Err(()),
        Some(m) => Ok(Some(m)),
        None => {
            oerr::push(errs::EVP_UNSUPPORTED);
            Ok(Some(ossl::digest::Md::H(
                ossl::digest::HashAlgorithm::Sha256,
            )))
        }
    }
}

fn section_warning(vm: &mut Vm, a: &Args, e: &ossl::req::ReqError, path: &str) -> R<()> {
    if let ossl::req::ReqError::MissingSection(which, sec) = e {
        vm.fwarn(
            a,
            &format!("Error loading {} section {} of {}", which, sec, path),
        )?;
    }
    Ok(())
}

fn options_arg(a: &Args, i: usize) -> Option<PhpArray> {
    match a.val(i) {
        Value::Array(x) => Some(x),
        _ => None,
    }
}

pub(super) fn openssl_csr_new(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let dn = a.array(0);
    let opts = options_arg(a, 2);
    let req = match req_from_options(vm, a, opts.as_ref())? {
        Ok(r) => r,
        Err(e) => {
            let path = opts
                .as_ref()
                .and_then(|o| o.get_bytes(b"config").map(|v| v.deref()))
                .map(|v| match v {
                    Value::Str(s) => s.to_string_lossy(),
                    _ => String::new(),
                })
                .unwrap_or_default();
            section_warning(vm, a, &e, &path)?;
            return Ok(Value::Bool(false));
        }
    };
    let Some(subject) = build_subject(vm, a, &req, &dn)? else {
        return Ok(Value::Bool(false));
    };
    let attrs = match a.val(3) {
        Value::Array(extra) => build_attributes(vm, a, &req, &extra)?,
        _ => Some(Vec::new()),
    };
    // The key: generated when the argument is null.
    let keyv = a.val(1);
    let key: Rc<ossl::key::Key> = if matches!(keyv, Value::Null) {
        let Some(k) = super::pkey::generate_key(vm, a, &req)? else {
            return Ok(Value::Bool(false));
        };
        let k = Rc::new(k);
        let obj = key_obj_rc(vm, k.clone())?;
        set_out(a, 1, obj);
        k
    } else {
        match key_from_value(vm, a, &keyv, Want::Private, None)? {
            Some(k) => k,
            None => return Ok(Value::Bool(false)),
        }
    };
    let Some(mut attrs) = attrs else {
        return Ok(Value::Bool(false));
    };
    if let Some(sec) = &req.req_extensions {
        let ctx = ossl::v3conf::Ctx {
            subject_spki: &key.to_spki_der(),
            subject: Some(&subject),
            issuer: None,
            issuer_name: None,
            serial: None,
            conf: &req.conf,
        };
        match ossl::v3conf::build_section(sec, &ctx) {
            Ok(exts) => {
                let ders: Vec<Vec<u8>> = exts.iter().map(ext_der).collect();
                attrs.push((
                    ossl::oid::ids::EXT_REQ.to_string(),
                    vec![ossl::der::seq_vec(&ders)],
                ));
            }
            Err(_) => {
                oerr::push(errs::X509V3_NO_ISSUER_CERT);
                oerr::push(errs::X509V3_ERROR_IN_EXTENSION);
                vm.fwarn(a, &format!("Error loading extension section {}", sec))?;
                return Ok(Value::Bool(false));
            }
        }
    }
    let md = match req_digest(&req, &key) {
        Ok(m) => m,
        Err(()) => {
            oerr::push(errs::EVP_UNSUPPORTED);
            oerr::push(errs::PROV_INVALID_DIGEST);
            vm.fwarn(a, "Error signing request")?;
            return Ok(Value::Bool(false));
        }
    };
    match ossl::csr::build(&subject, &key, &attrs, md) {
        Some(c) => csr_obj(vm, c),
        None => {
            vm.fwarn(a, "Error signing request")?;
            Ok(Value::Bool(false))
        }
    }
}

pub(super) fn ext_der(e: &ossl::x509::Extension) -> Vec<u8> {
    use ossl::der;
    if e.critical {
        der::seq_vec(&[
            der::oid(&e.oid),
            der::boolean(true),
            der::octet_string(&e.value),
        ])
    } else {
        der::seq_vec(&[der::oid(&e.oid), der::octet_string(&e.value)])
    }
}

pub(super) fn openssl_csr_sign(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(csr) = csr_arg(vm, a, 0)? else {
        return Ok(Value::Bool(false));
    };
    let ca = match a.val(1) {
        Value::Null => None,
        v => match cert_from_value(vm, a, &v)? {
            Some(c) => Some(c),
            None => {
                vm.fwarn(a, "X.509 Certificate cannot be retrieved")?;
                return Ok(Value::Bool(false));
            }
        },
    };
    let Some(key) = key_from_value(vm, a, &a.val(2), Want::Private, None)? else {
        vm.fwarn(a, "Cannot get private key from parameter 3")?;
        return Ok(Value::Bool(false));
    };
    let days = a.int(3);
    let max_days = i32::MAX as i64 / 86400;
    if !(0..=max_days).contains(&days) {
        vm.fwarn(a, &format!("Days must be between 0 and {}", max_days))?;
        return Ok(Value::Bool(false));
    }
    let opts = options_arg(a, 4);
    let req = match req_from_options(vm, a, opts.as_ref())? {
        Ok(r) => r,
        Err(e) => {
            section_warning(vm, a, &e, "")?;
            return Ok(Value::Bool(false));
        }
    };
    let serial = match a.opt_str(6) {
        Some(h) => match ossl::csr::serial_from_hex(&h.to_string_lossy()) {
            Ok(s) => s,
            Err(too_long) => {
                vm.fwarn(
                    a,
                    if too_long {
                        "Error parsing serial number because it is too long"
                    } else {
                        "Error parsing serial number"
                    },
                )?;
                return Ok(Value::Bool(false));
            }
        },
        None => ossl::csr::serial_from_int(a.int_or(5, 0)),
    };
    if let Some(c) = &ca
        && let Ok(ck) = c.public_key()
        && !ck.public_eq(&key)
    {
        vm.fwarn(a, "Private key does not correspond to signing cert")?;
        return Ok(Value::Bool(false));
    }
    if csr.verify_signature() != Some(true) {
        oerr::push(errs::ASN1_UNKNOWN_SIG_ALG);
        vm.fwarn(a, "Signature verification problems")?;
        return Ok(Value::Bool(false));
    }
    let issuer_name = match &ca {
        Some(c) => c.subject.clone(),
        None => csr.subject.clone(),
    };
    let mut exts = Vec::new();
    if let Some(sec) = &req.x509_extensions {
        let ctx = ossl::v3conf::Ctx {
            subject_spki: &csr.spki,
            subject: Some(&csr.subject),
            issuer: ca.as_deref(),
            issuer_name: Some(&issuer_name),
            serial: Some(&serial),
            conf: &req.conf,
        };
        match ossl::v3conf::build_section(sec, &ctx) {
            Ok(e) => exts = e,
            Err(_) => {
                oerr::push(errs::X509V3_ERROR_IN_EXTENSION);
                vm.fwarn(a, &format!("Error loading extension section {}", sec))?;
                return Ok(Value::Bool(false));
            }
        }
    }
    let md = match req_digest(&req, &key) {
        Ok(m) => m,
        Err(()) => {
            oerr::push(errs::EVP_UNSUPPORTED);
            vm.fwarn(a, "Failed to sign it")?;
            return Ok(Value::Bool(false));
        }
    };
    let now = super::x509::now();
    match ossl::csr::issue(
        &csr.subject,
        &issuer_name,
        &csr.spki,
        &serial,
        now,
        now + days * 86400,
        &exts,
        &key,
        md,
    ) {
        Some(c) => cert_obj(vm, Rc::new(c)),
        None => {
            vm.fwarn(a, "Failed to sign it")?;
            Ok(Value::Bool(false))
        }
    }
}

pub(super) fn openssl_csr_export(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(c) = csr_arg(vm, a, 0)? else {
        return Ok(Value::Bool(false));
    };
    let mut out = String::new();
    if !a.bool_or(2, true) {
        out.push_str(&ossl::certtext::csr_text(&c));
    }
    out.push_str(&c.to_pem());
    set_out(a, 1, str_val(&out));
    Ok(Value::Bool(true))
}

pub(super) fn openssl_csr_export_to_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(c) = csr_arg(vm, a, 0)? else {
        return Ok(Value::Bool(false));
    };
    let Some(path) = check_output_path(vm, a, &a.str(1), "output_filename")? else {
        return Ok(Value::Bool(false));
    };
    let mut out = String::new();
    if !a.bool_or(2, true) {
        out.push_str(&ossl::certtext::csr_text(&c));
    }
    out.push_str(&c.to_pem());
    Ok(Value::Bool(write_output(&path, out.as_bytes())))
}

pub(super) fn openssl_csr_get_subject(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(c) = csr_from_value(vm, a, &a.val(0))? else {
        return Ok(Value::Bool(false));
    };
    Ok(Value::Array(super::x509::name_array(
        &c.subject,
        a.bool_or(1, true),
    )))
}

pub(super) fn openssl_csr_get_public_key(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(c) = csr_from_value(vm, a, &a.val(0))? else {
        return Ok(Value::Bool(false));
    };
    match c.public_key() {
        Ok(k) => key_obj(vm, k),
        Err(_) => Ok(Value::Bool(false)),
    }
}
