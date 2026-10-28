//! openssl_pkcs12_*() and openssl_spki_*() (PHP manual: OpenSSL
//! functions).

use std::rc::Rc;

use goro_ext_openssl as ossl;
use goro_runtime::{Key, PhpArray, Value};

use super::*;

fn export_common(vm: &mut Vm, a: &Args) -> R<Option<Vec<u8>>> {
    let Some(cert) = cert_from_value(vm, a, &a.val(0))? else {
        vm.fwarn(a, "X.509 Certificate cannot be retrieved")?;
        return Ok(None);
    };
    let Some(key) = key_from_value(vm, a, &a.val(2), Want::Private, None)? else {
        kwarn(vm, a, "Cannot get private key from parameter 3")?;
        return Ok(None);
    };
    match cert.public_key() {
        Ok(pk) if pk.public_eq(&key) => {}
        _ => {
            vm.fwarn(a, "Private key does not correspond to cert")?;
            return Ok(None);
        }
    }
    let pass = a.str(3);
    let mut extra: Vec<ossl::x509::Cert> = Vec::new();
    let mut friendly = None;
    if let Value::Array(opts) = a.val(4) {
        if let Some(v) = opts.get_bytes(b"friendly_name") {
            friendly = Some(vm.to_php_str(&v.deref())?.to_string_lossy());
        }
        if let Some(v) = opts.get_bytes(b"extracerts") {
            let list: Vec<Value> = match v.deref() {
                Value::Array(l) => l.iter().map(|(_, x)| x.deref()).collect(),
                other => vec![other],
            };
            for item in list {
                // Unreadable extra certificates are skipped silently (test
                // bug81713.phpt).
                if let Some(c) = cert_from_value(vm, a, &item)? {
                    extra.push((*c).clone());
                }
            }
        }
    }
    Ok(ossl::pkcs12::export(
        &cert,
        &key,
        &extra,
        pass.as_bytes(),
        friendly.as_deref(),
    ))
}

pub(super) fn openssl_pkcs12_export(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match export_common(vm, a)? {
        Some(d) => {
            set_out(a, 1, bin(d));
            Ok(Value::Bool(true))
        }
        None => Ok(Value::Bool(false)),
    }
}

pub(super) fn openssl_pkcs12_export_to_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(path) = check_output_path(vm, a, &a.str(1), "output_filename")? else {
        return Ok(Value::Bool(false));
    };
    let Some(d) = export_common(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    if std::fs::write(&path, &d).is_err() {
        vm.fwarn(a, &format!("Error opening file {}", path.display()))?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(true))
}

pub(super) fn openssl_pkcs12_read(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.str(0);
    let pass = a.str(2);
    let c = match ossl::pkcs12::read(data.as_bytes(), pass.as_bytes()) {
        Ok(c) => c,
        Err(e) => {
            match e {
                ossl::pkcs12::P12Error::MacVerify => {
                    ossl::err::push(ossl::err::E::new(
                        0x11800071,
                        "PKCS12 routines",
                        "mac verify failure",
                    ));
                }
                _ => ossl::err::push(ossl::err::errs::ASN1_HEADER_TOO_LONG),
            }
            return Ok(Value::Bool(false));
        }
    };
    let _ = vm;
    let (key, cert, rest) = c.split();
    let mut out = PhpArray::new();
    if let Some(c) = &cert {
        out.insert(Key::from("cert"), str_val(&c.to_pem()));
    }
    if let Some(k) = &key {
        match k.to_pkcs8_pem() {
            Some(p) => {
                out.insert(Key::from("pkey"), bin(p.to_vec()));
            }
            None => return Ok(Value::Bool(false)),
        }
    }
    if !rest.is_empty() {
        let mut list = PhpArray::new();
        for c in rest {
            let _ = list.push(str_val(&c.to_pem()));
        }
        out.insert(Key::from("extracerts"), Value::Array(list));
    }
    set_out(a, 1, Value::Array(out));
    Ok(Value::Bool(true))
}

// ------------------------------------------------------------ SPKI

pub(super) fn openssl_spki_new(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(o) = a.obj(0) else {
        return Ok(Value::Bool(false));
    };
    let Some(k) = with_native::<KeyObj, _>(&o, |k| k.0.clone()) else {
        return Ok(Value::Bool(false));
    };
    if !k.is_private() {
        vm.fwarn(a, "Unable to use supplied private key")?;
        return Ok(Value::Bool(false));
    }
    let challenge = a.str(1);
    let Some(md) = ossl::digest::Md::by_algo_const(a.int_or(2, 2)) else {
        vm.fwarn(a, "Unknown digest algorithm")?;
        return Ok(Value::Bool(false));
    };
    match ossl::spki::new(&k, challenge.as_bytes(), md) {
        Some(s) => Ok(str_val(&s)),
        None => {
            vm.fwarn(a, "Unable to sign with specified digest algorithm")?;
            Ok(Value::Bool(false))
        }
    }
}

fn spki_arg(vm: &mut Vm, a: &Args) -> R<Option<ossl::spki::Spkac>> {
    let s = a.str(0);
    // Observed on PHP 8.5: undecodable input gives false without a
    // warning.
    let _ = vm;
    Ok(ossl::spki::parse(s.as_bytes()))
}

pub(super) fn openssl_spki_verify(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(s) = spki_arg(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    Ok(Value::Bool(s.verify()))
}

pub(super) fn openssl_spki_export(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(s) = spki_arg(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    match s.public_key() {
        Some(k) => Ok(str_val(&k.to_spki_pem())),
        None => {
            vm.fwarn(a, "Unable to acquire signed public key")?;
            Ok(Value::Bool(false))
        }
    }
}

pub(super) fn openssl_spki_export_challenge(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(s) = spki_arg(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    Ok(bin(s.challenge))
}

#[allow(dead_code)]
fn unused(_: Rc<()>) {}
