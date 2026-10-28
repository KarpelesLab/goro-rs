//! openssl_x509_*() (PHP manual: OpenSSL functions).

use std::rc::Rc;

use goro_ext_openssl as ossl;
use goro_runtime::{Key, PhpArray, Value};
use ossl::err::{self as oerr, errs};
use ossl::x509::{Cert, Name};

use super::*;

fn cert_arg(vm: &mut Vm, a: &Args, i: usize) -> R<Option<Rc<Cert>>> {
    let r = cert_from_value(vm, a, &a.val(i))?;
    if r.is_none() {
        vm.fwarn(a, "X.509 Certificate cannot be retrieved")?;
    }
    Ok(r)
}

pub(super) fn openssl_x509_read(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match cert_arg(vm, a, 0)? {
        Some(c) => cert_obj(vm, c),
        None => Ok(Value::Bool(false)),
    }
}

pub(super) fn openssl_x509_free(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.deprecated(
        "Function openssl_x509_free() is deprecated since 8.0, as OpenSSLCertificate objects are freed automatically",
    )?;
    Ok(Value::Null)
}

/// A name as PHP's arrays show it: short or long attribute names, values
/// as UTF-8, repeated names gathered in a list.
pub(super) fn name_array(n: &Name, short: bool) -> PhpArray {
    let mut arr = PhpArray::new();
    for at in n.entries() {
        let k = if short { at.sn() } else { at.ln() };
        let v = bin(at.utf8());
        let key = Key::from(k.as_str());
        match arr.get_bytes(k.as_bytes()).cloned() {
            None => {
                arr.insert(key, v);
            }
            Some(Value::Array(mut list)) => {
                let _ = list.push(v);
                arr.insert(key, Value::Array(list));
            }
            Some(old) => {
                arr.insert(key, Value::Array(PhpArray::from_values([old, v])));
            }
        }
    }
    arr
}

fn time_t(vm: &mut Vm, a: &Args, t: &ossl::x509::Time) -> R<Value> {
    let _ = (vm, a);
    Ok(Value::Int(t.unix))
}

pub(super) fn openssl_x509_parse(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(c) = cert_from_value(vm, a, &a.val(0))? else {
        return Ok(Value::Bool(false));
    };
    let short = a.bool_or(1, true);
    let mut arr = PhpArray::new();
    let put = |arr: &mut PhpArray, k: &str, v: Value| {
        arr.insert(Key::from(k), v);
    };
    put(&mut arr, "name", str_val(&c.subject.oneline()));
    put(
        &mut arr,
        "subject",
        Value::Array(name_array(&c.subject, short)),
    );
    put(
        &mut arr,
        "hash",
        str_val(&format!("{:08x}", c.subject.hash())),
    );
    put(
        &mut arr,
        "issuer",
        Value::Array(name_array(&c.issuer, short)),
    );
    put(&mut arr, "version", Value::Int(c.version));
    put(&mut arr, "serialNumber", str_val(&c.serial_decimal()));
    put(&mut arr, "serialNumberHex", str_val(&c.serial_hex()));
    put(&mut arr, "validFrom", str_val(&c.not_before.raw));
    put(&mut arr, "validTo", str_val(&c.not_after.raw));
    let vf = time_t(vm, a, &c.not_before)?;
    let vt = time_t(vm, a, &c.not_after)?;
    put(&mut arr, "validFrom_time_t", vf);
    put(&mut arr, "validTo_time_t", vt);
    let (sn, ln, nid) = match ossl::oid::by_oid(&c.sig_alg) {
        Some(o) => (o.sn.to_string(), o.ln.to_string(), o.nid),
        None => (c.sig_alg.clone(), c.sig_alg.clone(), 0),
    };
    put(&mut arr, "signatureTypeSN", str_val(&sn));
    put(&mut arr, "signatureTypeLN", str_val(&ln));
    put(&mut arr, "signatureTypeNID", Value::Int(nid));
    let mut purposes = PhpArray::new();
    for (id, sname, lname) in ossl::x509::PURPOSES {
        let p = PhpArray::from_values([
            Value::Bool(c.check_purpose(*id, false) != 0),
            Value::Bool(c.check_purpose(*id, true) != 0),
            str_val(if short { sname } else { lname }),
        ]);
        purposes.insert(Key::Int(*id), Value::Array(p));
    }
    put(&mut arr, "purposes", Value::Array(purposes));
    let mut exts = PhpArray::new();
    for e in &c.extensions {
        let name = match ossl::oid::by_oid(&e.oid) {
            Some(o) => o.sn.to_string(),
            None => e.oid.clone(),
        };
        let v = match ossl::x509::extension_text(e) {
            Some(t) => str_val(&t),
            None => bin(e.value.clone()),
        };
        exts.insert(Key::from(name.as_str()), v);
    }
    put(&mut arr, "extensions", Value::Array(exts));
    Ok(Value::Array(arr))
}

pub(super) fn openssl_x509_export(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(c) = cert_arg(vm, a, 0)? else {
        return Ok(Value::Bool(false));
    };
    let notext = a.bool_or(2, true);
    let mut out = String::new();
    if !notext {
        out.push_str(&ossl::certtext::cert_text(&c));
    }
    out.push_str(&c.to_pem());
    set_out(a, 1, str_val(&out));
    Ok(Value::Bool(true))
}

pub(super) fn openssl_x509_export_to_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(c) = cert_arg(vm, a, 0)? else {
        return Ok(Value::Bool(false));
    };
    let Some(path) = check_output_path(vm, a, &a.str(1), "output_filename")? else {
        return Ok(Value::Bool(false));
    };
    let notext = a.bool_or(2, true);
    let mut out = String::new();
    if !notext {
        out.push_str(&ossl::certtext::cert_text(&c));
    }
    out.push_str(&c.to_pem());
    Ok(Value::Bool(write_output(&path, out.as_bytes())))
}

pub(super) fn openssl_x509_fingerprint(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(c) = cert_arg(vm, a, 0)? else {
        return Ok(Value::Bool(false));
    };
    let algo = a
        .opt_str(1)
        .map(|s| s.to_string_lossy())
        .unwrap_or_else(|| "sha1".into());
    let Some(md) = ossl::digest::Md::by_name(&algo) else {
        vm.fwarn(a, "Unknown digest algorithm")?;
        return Ok(Value::Bool(false));
    };
    let fp = c.fingerprint(md);
    if a.bool_or(2, false) {
        Ok(bin(fp))
    } else {
        Ok(str_val(&hex_lower(&fp)))
    }
}

pub(super) fn openssl_x509_check_private_key(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(c) = cert_from_value(vm, a, &a.val(0))? else {
        return Ok(Value::Bool(false));
    };
    let Some(k) = key_from_value(vm, a, &a.val(1), Want::Private, None)? else {
        return Ok(Value::Bool(false));
    };
    let Ok(ck) = c.public_key() else {
        return Ok(Value::Bool(false));
    };
    let ok = ck.public_eq(&k);
    if !ok {
        oerr::push(errs::X509_KEY_VALUES_MISMATCH);
    }
    Ok(Value::Bool(ok))
}

pub(super) fn openssl_x509_verify(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(c) = cert_from_value(vm, a, &a.val(0))? else {
        return Ok(Value::Int(-1));
    };
    let Some(k) = key_from_value(vm, a, &a.val(1), Want::Public, None)? else {
        return Ok(Value::Int(-1));
    };
    Ok(Value::Int(match c.verify_signature(&k) {
        Some(true) => 1,
        Some(false) => 0,
        None => -1,
    }))
}

pub(super) fn openssl_x509_checkpurpose(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(c) = cert_from_value(vm, a, &a.val(0))? else {
        return Ok(Value::Int(-1));
    };
    let purpose = a.int(1);
    if !(1..=10).contains(&purpose) {
        oerr::push(errs::X509V3_INVALID_PURPOSE);
        return Ok(Value::Int(-1));
    }
    let trusted = match load_ca_info(vm, a, &a.array(2))? {
        Some(t) => t,
        None => return Ok(Value::Int(-1)),
    };
    let mut untrusted = Vec::new();
    if let Some(f) = a.opt_str(3) {
        match std::fs::read(os_path(f.as_bytes())) {
            Ok(d) => untrusted = ossl::x509::certs_from_bytes(&d),
            Err(_) => {
                vm.fwarn(a, &format!("Error loading file {}", f.to_string_lossy()))?;
                return Ok(Value::Int(-1));
            }
        }
    }
    let ok = ossl::verify::verify_chain(&c, &trusted, &untrusted, Some(purpose), now()).is_ok();
    Ok(Value::Bool(ok))
}

pub(super) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Trusted certificates from a `$ca_info` array: files and directories
/// (every certificate file in a directory, like a hashed CA path).
pub(super) fn load_ca_info(vm: &mut Vm, a: &Args, ca: &PhpArray) -> R<Option<Vec<Cert>>> {
    let mut out = Vec::new();
    for (_, v) in ca.iter() {
        let s = vm.to_php_str(&v.deref())?;
        let p = os_path(s.as_bytes());
        if p.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                let mut names: Vec<_> = rd.flatten().map(|e| e.path()).collect();
                names.sort();
                for f in names {
                    if let Ok(d) = std::fs::read(&f) {
                        out.extend(ossl::x509::certs_from_bytes(&d));
                    }
                }
            }
        } else {
            match std::fs::read(&p) {
                Ok(d) => out.extend(ossl::x509::certs_from_bytes(&d)),
                Err(_) => {
                    vm.fwarn(a, &format!("Unable to stat {}", s.to_string_lossy()))?;
                }
            }
        }
    }
    Ok(Some(out))
}
