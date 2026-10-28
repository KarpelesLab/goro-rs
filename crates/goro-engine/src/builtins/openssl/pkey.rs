//! openssl_pkey_*(), signatures, RSA encryption and key agreement (PHP
//! manual: OpenSSL functions).

use std::rc::Rc;

use goro_ext_openssl as ossl;
use goro_runtime::{Key, PhpArray, Value};
use ossl::err::{self as oerr, errs};
use ossl::key::{Detail, Key as PKey, KeyType};

use super::*;

// ------------------------------------------------------------ openssl_pkey_new

fn arr_bytes(vm: &mut Vm, arr: &PhpArray, k: &str) -> R<Option<Vec<u8>>> {
    match arr.get_bytes(k.as_bytes()) {
        Some(v) => {
            let v = v.deref();
            if matches!(v, Value::Null) {
                return Ok(None);
            }
            Ok(Some(vm.to_php_str(&v)?.to_vec()))
        }
        None => Ok(None),
    }
}

pub(super) fn openssl_pkey_new(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let opts = match a.val(0) {
        Value::Array(x) => Some(x),
        _ => None,
    };
    if let Some(o) = &opts {
        // Key from components (PHP manual: openssl_pkey_new() "rsa",
        // "dsa", "dh", "ec" and curve-key arrays).
        for name in [
            "rsa", "dsa", "dh", "ec", "x25519", "ed25519", "x448", "ed448",
        ] {
            if let Some(Value::Array(sub)) = o.get_bytes(name.as_bytes()).map(|v| v.deref()) {
                let r = key_from_components(vm, a, name, &sub)?;
                return match r {
                    Some(k) => key_obj(vm, k),
                    None => Ok(Value::Bool(false)),
                };
            }
        }
    }
    let req = match req_from_options(vm, a, opts.as_ref())? {
        Ok(r) => r,
        Err(_) => return Ok(Value::Bool(false)),
    };
    match generate_key(vm, a, &req)? {
        Some(k) => key_obj(vm, k),
        None => Ok(Value::Bool(false)),
    }
}

/// Generates a key per the request settings (also used by
/// openssl_csr_new() when no key is given).
pub(super) fn generate_key(vm: &mut Vm, a: &Args, req: &ossl::req::Req) -> R<Option<PKey>> {
    let Some(t) = KeyType::from_int(req.key_type) else {
        vm.fwarn(a, "Unsupported private key type")?;
        return Ok(None);
    };
    match t {
        KeyType::Rsa | KeyType::Dsa | KeyType::Dh if req.bits < 384 => {
            vm.fwarn(
                a,
                &format!(
                    "Private key length must be at least 384 bits, configured to {}",
                    req.bits
                ),
            )?;
            return Ok(None);
        }
        _ => {}
    }
    let r = match t {
        KeyType::Rsa => {
            if req.bits < 512 {
                oerr::push(errs::PROV_KEY_SIZE_TOO_SMALL);
                return Ok(None);
            }
            PKey::rsa_generate(req.bits as usize, &[1, 0, 1])
        }
        KeyType::Dsa => {
            if req.bits < 1024 {
                oerr::push(errs::DSA_BAD_FFC_PARAMETERS);
                return Ok(None);
            }
            PKey::dsa_generate(req.bits as usize)
        }
        KeyType::Dh => {
            // A safe-prime group of the requested size: the RFC 3526 /
            // RFC 7919 groups cover the common sizes.
            match ossl_dh_group(req.bits) {
                Some((p, g)) => PKey::dh_generate_from(&p, &g, None),
                None => Err(ossl::key::KeyError::Unsupported),
            }
        }
        KeyType::Ec => {
            let Some(name) = &req.curve_name else {
                vm.fwarn(a, "Missing configuration value: \"curve_name\" not set")?;
                return Ok(None);
            };
            let Some(c) = ossl::key::Curve::named(name) else {
                vm.fwarn(a, &format!("Unknown elliptic curve (short) name {}", name))?;
                return Ok(None);
            };
            PKey::ec_generate(c)
        }
        other => PKey::generate_raw(other),
    };
    Ok(r.ok())
}

/// MODP groups for DH key generation without parameters (RFC 3526 §2-§5
/// and RFC 2409 §6.2 for 1024 bits); the generator is 2.
fn ossl_dh_group(bits: i64) -> Option<(Vec<u8>, Vec<u8>)> {
    const P1024: &str = "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7EDEE386BFB5A899FA5AE9F24117C4B1FE649286651ECE65381FFFFFFFFFFFFFFFF";
    const P2048: &str = "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7EDEE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3DC2007CB8A163BF0598DA48361C55D39A69163FA8FD24CF5F83655D23DCA3AD961C62F356208552BB9ED529077096966D670C354E4ABC9804F1746C08CA18217C32905E462E36CE3BE39E772C180E86039B2783A2EC07A28FB5C55DF06F4C52C9DE2BCBF6955817183995497CEA956AE515D2261898FA051015728E5A8AACAA68FFFFFFFFFFFFFFFF";
    let h = match bits {
        ..=1024 => P1024,
        _ => P2048,
    };
    Some((ossl::pbe::hex_decode(h)?, vec![2]))
}

fn key_from_components(vm: &mut Vm, a: &Args, kind: &str, sub: &PhpArray) -> R<Option<PKey>> {
    let g = |vm: &mut Vm, k: &str| arr_bytes(vm, sub, k);
    let r = match kind {
        "rsa" => {
            let (Some(n), Some(e)) = (g(vm, "n")?, g(vm, "e")?) else {
                return Ok(None);
            };
            let Some(d) = g(vm, "d")? else {
                return Ok(None);
            };
            let p = g(vm, "p")?;
            let q = g(vm, "q")?;
            let dp = g(vm, "dmp1")?;
            let dq = g(vm, "dmq1")?;
            let qi = g(vm, "iqmp")?;
            PKey::rsa_private(
                &n,
                &e,
                &d,
                p.as_deref(),
                q.as_deref(),
                dp.as_deref(),
                dq.as_deref(),
                qi.as_deref(),
            )
        }
        "dsa" => {
            let (Some(p), Some(q), Some(gg)) = (g(vm, "p")?, g(vm, "q")?, g(vm, "g")?) else {
                return Ok(None);
            };
            let x = g(vm, "priv_key")?;
            let y = g(vm, "pub_key")?;
            if x.is_none() && y.is_none() {
                PKey::dsa_generate_from(&p, &q, &gg)
            } else {
                PKey::dsa(&p, &q, &gg, y.as_deref(), x.as_deref())
            }
        }
        "dh" => {
            let (Some(p), Some(gg)) = (g(vm, "p")?, g(vm, "g")?) else {
                return Ok(None);
            };
            let q = g(vm, "q")?;
            let x = g(vm, "priv_key")?;
            let y = g(vm, "pub_key")?;
            if x.is_none() && y.is_none() {
                PKey::dh_generate_from(&p, &gg, q.as_deref())
            } else {
                PKey::dh(&p, &gg, q.as_deref(), y.as_deref(), x.as_deref())
            }
        }
        "ec" => return ec_from_components(vm, a, sub),
        "x25519" | "ed25519" | "x448" | "ed448" => {
            let x = g(vm, "priv_key")?;
            let y = g(vm, "pub_key")?;
            if x.is_none() && y.is_none() {
                let t = match kind {
                    "x25519" => KeyType::X25519,
                    "ed25519" => KeyType::Ed25519,
                    "x448" => KeyType::X448,
                    _ => KeyType::Ed448,
                };
                PKey::generate_raw(t)
            } else {
                match kind {
                    "x25519" => PKey::x25519(y.as_deref(), x.as_deref()),
                    "ed25519" => PKey::ed25519(y.as_deref(), x.as_deref()),
                    "x448" => PKey::x448(y.as_deref(), x.as_deref()),
                    _ => PKey::ed448(y.as_deref(), x.as_deref()),
                }
            }
        }
        _ => return Ok(None),
    };
    Ok(r.ok())
}

fn ec_from_components(vm: &mut Vm, a: &Args, sub: &PhpArray) -> R<Option<PKey>> {
    let g = |vm: &mut Vm, k: &str| arr_bytes(vm, sub, k);
    let curve = if let Some(name) = g(vm, "curve_name")? {
        let name = String::from_utf8_lossy(&name).into_owned();
        match ossl::key::Curve::named(&name) {
            Some(c) => c,
            None => {
                vm.fwarn(a, &format!("Unknown elliptic curve (short) name {}", name))?;
                return Ok(None);
            }
        }
    } else {
        let (p, ca, cb, order) = (g(vm, "p")?, g(vm, "a")?, g(vm, "b")?, g(vm, "order")?);
        if p.is_none() && ca.is_none() && cb.is_none() && order.is_none() {
            vm.fwarn(a, "Missing params: curve_name")?;
            return Ok(None);
        }
        let (Some(p), Some(ca), Some(cb), Some(order)) = (p, ca, cb, order) else {
            vm.fwarn(a, "Missing params: curve_name or p, a, b, order")?;
            return Ok(None);
        };
        let (gx, gy) = match (g(vm, "g_x")?, g(vm, "g_y")?, g(vm, "generator")?) {
            (Some(x), Some(y), _) => (x, y),
            (_, _, Some(gpt)) if gpt.first() == Some(&4) && gpt.len() % 2 == 1 => {
                let l = (gpt.len() - 1) / 2;
                (gpt[1..1 + l].to_vec(), gpt[1 + l..].to_vec())
            }
            _ => {
                vm.fwarn(a, "Missing params: generator or g_x and g_y")?;
                return Ok(None);
            }
        };
        let h = g(vm, "cofactor")?.unwrap_or_else(|| vec![1]);
        match ossl::key::Curve::explicit(&p, &ca, &cb, &gx, &gy, &order, &h) {
            Ok(c) => c,
            Err(_) => return Ok(None),
        }
    };
    let d = g(vm, "d")?;
    let x = g(vm, "x")?;
    let y = g(vm, "y")?;
    let point = match (x, y) {
        (Some(x), Some(y)) => {
            let fl = curve.field_len();
            let mut pt = vec![4u8];
            pt.extend(pad_left(&x, fl));
            pt.extend(pad_left(&y, fl));
            Some(pt)
        }
        _ => None,
    };
    let r = if d.is_none() && point.is_none() {
        PKey::ec_generate(curve)
    } else {
        PKey::ec(curve, point.as_deref(), d.as_deref())
    };
    Ok(r.ok())
}

fn pad_left(b: &[u8], n: usize) -> Vec<u8> {
    let s = ossl::key::strip(b);
    let mut v = vec![0u8; n.saturating_sub(s.len())];
    v.extend_from_slice(&s);
    v
}

// ------------------------------------------------------------ export / load

fn export_common(vm: &mut Vm, a: &Args) -> R<Option<ossl::key::Secret>> {
    let pass = a.opt_str(2);
    let opts = match a.val(3) {
        Value::Array(x) => Some(x),
        _ => None,
    };
    let Some(k) = key_from_value(
        vm,
        a,
        &a.val(0),
        Want::Private,
        pass.as_ref().map(|p| p.as_bytes()),
    )?
    else {
        kwarn(vm, a, "Cannot get key from parameter 1")?;
        return Ok(None);
    };
    let req = match req_from_options(vm, a, opts.as_ref())? {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };
    let cipher = match req.cipher {
        None => ossl::pbe::CbcCipher::DesEde3,
        Some(n) => match n {
            4 => ossl::pbe::CbcCipher::DesEde3,
            5 => ossl::pbe::CbcCipher::Aes128,
            6 => ossl::pbe::CbcCipher::Aes192,
            7 => ossl::pbe::CbcCipher::Aes256,
            _ => {
                vm.fwarn(a, "Unknown cipher algorithm for private key")?;
                return Ok(None);
            }
        },
    };
    let pass = if req.encrypt_key { pass } else { None };
    Ok(k.export_private_pem(pass.as_ref().map(|p| p.as_bytes()), cipher))
}

pub(super) fn openssl_pkey_export(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match export_common(vm, a)? {
        Some(pem) => {
            set_out(a, 1, bin(pem.to_vec()));
            Ok(Value::Bool(true))
        }
        None => Ok(Value::Bool(false)),
    }
}

pub(super) fn openssl_pkey_export_to_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(path) = check_output_path(vm, a, &a.str(1), "output_filename")? else {
        return Ok(Value::Bool(false));
    };
    match export_common(vm, a)? {
        Some(pem) => Ok(Value::Bool(write_output(&path, &pem))),
        None => Ok(Value::Bool(false)),
    }
}

pub(super) fn openssl_pkey_get_public(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match key_from_value(vm, a, &a.val(0), Want::Public, None)? {
        Some(k) => key_obj_rc(vm, k),
        None => Ok(Value::Bool(false)),
    }
}

pub(super) fn openssl_pkey_get_private(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pass = a.opt_str(1);
    match key_from_value(
        vm,
        a,
        &a.val(0),
        Want::Private,
        pass.as_ref().map(|p| p.as_bytes()),
    )? {
        Some(k) => key_obj_rc(vm, k),
        None => Ok(Value::Bool(false)),
    }
}

pub(super) fn openssl_pkey_free(vm: &mut Vm, a: &mut Args) -> R<Value> {
    vm.deprecated(&format!(
        "Function {}() is deprecated since 8.0, as OpenSSLAsymmetricKey objects are freed automatically",
        a.fname()
    ))?;
    Ok(Value::Null)
}

pub(super) fn openssl_pkey_get_details(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(o) = a.obj(0) else {
        return Ok(Value::Bool(false));
    };
    let Some(k) = with_native::<KeyObj, _>(&o, |k| k.0.clone()) else {
        return Ok(Value::Bool(false));
    };
    let _ = vm;
    Ok(Value::Array(details_array(&k)))
}

pub(super) fn details_array(k: &PKey) -> PhpArray {
    let d = k.details();
    let mut arr = PhpArray::new();
    arr.insert(Key::from("bits"), Value::Int(d.bits));
    arr.insert(Key::from("key"), str_val(&d.key_pem));
    let mut sub = PhpArray::new();
    for (n, v) in d.fields {
        sub.insert(
            Key::from(n),
            match v {
                Detail::Bin(b) => bin(b),
                Detail::Str(s) => str_val(&s),
            },
        );
    }
    if let Some(t) = d.typ {
        arr.insert(Key::from(t.name()), Value::Array(sub));
        arr.insert(Key::from("type"), Value::Int(t as i64));
    } else {
        arr.insert(Key::from("type"), Value::Int(-1));
    }
    arr
}

// ------------------------------------------------------------ sign / verify

/// The digest of a `string|int $algorithm` argument.
fn md_arg(v: &Value) -> Option<ossl::digest::Md> {
    match v.deref() {
        Value::Int(n) => ossl::digest::Md::by_algo_const(n),
        Value::Str(s) => ossl::digest::Md::by_name(&s.to_string_lossy()),
        _ => None,
    }
}

pub(super) fn openssl_sign(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.str(0);
    let Some(k) = key_from_value(vm, a, &a.val(2), Want::Private, None)? else {
        kwarn(
            vm,
            a,
            "Supplied key param cannot be coerced into a private key",
        )?;
        return Ok(Value::Bool(false));
    };
    let alg = if a.has(3) { a.val(3) } else { Value::Int(1) };
    let eddsa = matches!(k.key_type(), KeyType::Ed25519 | KeyType::Ed448);
    let md = if eddsa && matches!(alg.deref(), Value::Int(0)) {
        None
    } else {
        match md_arg(&alg) {
            Some(m) => Some(m),
            None => {
                if eddsa {
                    None
                } else {
                    if matches!(alg.deref(), Value::Str(_)) {
                        oerr::push(errs::EVP_UNSUPPORTED);
                    }
                    vm.fwarn(a, "Unknown digest algorithm")?;
                    return Ok(Value::Bool(false));
                }
            }
        }
    };
    let pss = a.int_or(4, 0) == ossl::key::PAD_OAEP + 2;
    match k.sign(if eddsa { None } else { md }, data.as_bytes(), pss) {
        Ok(sig) => {
            set_out(a, 1, bin(sig));
            Ok(Value::Bool(true))
        }
        Err(_) => Ok(Value::Bool(false)),
    }
}

pub(super) fn openssl_verify(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.str(0);
    let sig = a.str(1);
    let Some(k) = key_from_value(vm, a, &a.val(2), Want::Public, None)? else {
        kwarn(
            vm,
            a,
            "Supplied key param cannot be coerced into a public key",
        )?;
        return Ok(Value::Bool(false));
    };
    let alg = if a.has(3) { a.val(3) } else { Value::Int(1) };
    let eddsa = matches!(k.key_type(), KeyType::Ed25519 | KeyType::Ed448);
    let md = if eddsa {
        None
    } else {
        match md_arg(&alg) {
            Some(m) => Some(m),
            None => {
                if matches!(alg.deref(), Value::Str(_)) {
                    oerr::push(errs::EVP_UNSUPPORTED);
                }
                vm.fwarn(a, "Unknown digest algorithm")?;
                return Ok(Value::Bool(false));
            }
        }
    };
    let pss = a.int_or(4, 0) == 6;
    match k.verify(md, data.as_bytes(), sig.as_bytes(), pss) {
        Ok(true) => Ok(Value::Int(1)),
        Ok(false) => {
            if k.key_type() == KeyType::Rsa {
                if sig.len() != k.size() {
                    oerr::push(errs::RSA_WRONG_SIG_LEN);
                } else {
                    oerr::push(errs::RSA_INVALID_PADDING);
                    oerr::push(errs::RSA_PADDING_CHECK_FAILED);
                }
                oerr::push(errs::PROV_RSA_LIB);
            }
            Ok(Value::Int(0))
        }
        Err(_) => Ok(Value::Int(-1)),
    }
}

// ------------------------------------------------------------ RSA encryption

fn oaep_md(vm: &mut Vm, a: &Args, i: usize) -> R<Option<ossl::digest::Md>> {
    match a.opt_str(i) {
        None => Ok(Some(ossl::digest::Md::H(ossl::digest::HashAlgorithm::Sha1))),
        Some(s) => match ossl::digest::Md::by_name(&s.to_string_lossy()) {
            Some(m) => Ok(Some(m)),
            None => {
                oerr::push(errs::EVP_UNSUPPORTED);
                vm.fwarn(
                    a,
                    &format!("Unknown digest algorithm: {}", s.to_string_lossy()),
                )?;
                Ok(None)
            }
        },
    }
}

pub(super) fn openssl_public_encrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    check_path_args(vm, a, &[4])?;
    let data = a.str(0);
    let Some(k) = key_from_value(vm, a, &a.val(2), Want::Public, None)? else {
        kwarn(vm, a, "key parameter is not a valid public key")?;
        return Ok(Value::Bool(false));
    };
    let padding = a.int_or(3, 1);
    let Some(md) = oaep_md(vm, a, 4)? else {
        return Ok(Value::Bool(false));
    };
    match ossl::key::rsa_public_encrypt(&k, data.as_bytes(), padding, md) {
        Ok(c) => {
            set_out(a, 1, bin(c));
            Ok(Value::Bool(true))
        }
        Err(e) => {
            oerr::push(e);
            Ok(Value::Bool(false))
        }
    }
}

pub(super) fn openssl_private_decrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    check_path_args(vm, a, &[4])?;
    let data = a.str(0);
    let Some(k) = key_from_value(vm, a, &a.val(2), Want::Private, None)? else {
        kwarn(vm, a, "key parameter is not a valid private key")?;
        return Ok(Value::Bool(false));
    };
    let padding = a.int_or(3, 1);
    let Some(md) = oaep_md(vm, a, 4)? else {
        return Ok(Value::Bool(false));
    };
    match ossl::key::rsa_private_decrypt(&k, data.as_bytes(), padding, md) {
        Ok(p) => {
            set_out(a, 1, bin(p.to_vec()));
            Ok(Value::Bool(true))
        }
        Err(e) => {
            oerr::push(e);
            Ok(Value::Bool(false))
        }
    }
}

pub(super) fn openssl_private_encrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.str(0);
    let Some(k) = key_from_value(vm, a, &a.val(2), Want::Private, None)? else {
        kwarn(vm, a, "key param is not a valid private key")?;
        return Ok(Value::Bool(false));
    };
    let padding = a.int_or(3, 1);
    match ossl::key::rsa_private_encrypt(&k, data.as_bytes(), padding) {
        Ok(c) => {
            set_out(a, 1, bin(c));
            Ok(Value::Bool(true))
        }
        Err(e) => {
            oerr::push(e);
            Ok(Value::Bool(false))
        }
    }
}

pub(super) fn openssl_public_decrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.str(0);
    let Some(k) = key_from_value(vm, a, &a.val(2), Want::Public, None)? else {
        kwarn(vm, a, "key parameter is not a valid public key")?;
        return Ok(Value::Bool(false));
    };
    let padding = a.int_or(3, 1);
    match ossl::key::rsa_public_decrypt(&k, data.as_bytes(), padding) {
        Ok(p) => {
            set_out(a, 1, bin(p));
            Ok(Value::Bool(true))
        }
        Err(es) => {
            for e in es {
                oerr::push(e);
            }
            Ok(Value::Bool(false))
        }
    }
}

// ------------------------------------------------------------ key agreement

pub(super) fn openssl_dh_compute_key(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pubv = a.str(0);
    let Some(o) = a.obj(1) else {
        return Ok(Value::Bool(false));
    };
    let Some(k) = with_native::<KeyObj, _>(&o, |k| k.0.clone()) else {
        return Ok(Value::Bool(false));
    };
    let ossl::key::KeyData::Dh { p, x: Some(x), .. } = &k.data else {
        return Ok(Value::Bool(false));
    };
    let _ = vm;
    match PKey::dh_compute(p, x, pubv.as_bytes()) {
        // openssl_dh_compute_key() returns the secret without the
        // left-padding (PHP manual; test openssl_dh_compute_key.phpt).
        Ok(s) => Ok(bin(ossl::key::strip(&s))),
        Err(_) => Ok(Value::Bool(false)),
    }
}

pub(super) fn openssl_pkey_derive(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if a.has(2) && a.int(2) != 0 {
        vm.fdeprecated(
            a,
            "the $key_length parameter is deprecated as it is either ignored or truncates the key",
        )?;
    }
    let pk = key_from_value(vm, a, &a.val(0), Want::Public, None)?;
    let sk = key_from_value(vm, a, &a.val(1), Want::Private, None)?;
    let (Some(pk), Some(sk)) = (pk, sk) else {
        return Ok(Value::Bool(false));
    };
    match sk.derive(&pk) {
        Ok(mut s) => {
            let kl = a.int_or(2, 0);
            if kl > 0 && (kl as usize) < s.len() {
                // DH secrets are not truncated (test gh19428.phpt).
                if matches!(sk.key_type(), KeyType::Dh) {
                    return Ok(Value::Bool(false));
                }
                s.truncate(kl as usize);
            }
            Ok(bin(s))
        }
        Err(_) => {
            oerr::push(errs::EVP_OP_NOT_SUPPORTED_KEYTYPE);
            Ok(Value::Bool(false))
        }
    }
}

pub(super) fn openssl_get_curve_names(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let mut arr = PhpArray::new();
    for (n, _, _) in ossl::key::CURVES {
        let _ = arr.push(str_val(n));
    }
    Ok(Value::Array(arr))
}

// ------------------------------------------------------------ seal / open

/// openssl_seal() (PHP manual): a random key encrypts the data, and each
/// public key gets that key RSA-encrypted (PKCS #1 v1.5).
pub(super) fn openssl_seal(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.str(0);
    let keys = a.array(3);
    if keys.is_empty() {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #4 ($public_key) must not be empty",
        );
    }
    let name = a.str(4);
    let Some(info) = ossl::cipher::lookup_bytes(name.as_bytes()) else {
        vm.fwarn(a, "Unknown cipher algorithm")?;
        return Ok(Value::Bool(false));
    };
    if info.iv_len() > 0 && !a.has(5) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #6 ($iv) cannot be null for the chosen cipher algorithm",
        );
    }
    let mut pubs = Vec::new();
    for (i, (_, v)) in keys.iter().enumerate() {
        match key_from_value(vm, a, &v.deref(), Want::Public, None)? {
            Some(k) => pubs.push(k),
            None => {
                kwarn(
                    vm,
                    a,
                    &format!("Not a public key ({}th member of pubkeys)", i + 1),
                )?;
                return Ok(Value::Bool(false));
            }
        }
    }
    let mut key = vec![0u8; info.key_len()];
    ossl::random_bytes(&mut key);
    let mut iv = vec![0u8; info.iv_len()];
    ossl::random_bytes(&mut iv);
    let md = ossl::digest::Md::H(ossl::digest::HashAlgorithm::Sha1);
    let mut ekeys = PhpArray::new();
    for k in &pubs {
        match ossl::key::rsa_public_encrypt(k, &key, ossl::key::PAD_PKCS1, md) {
            Ok(ek) => {
                let _ = ekeys.push(bin(ek));
            }
            Err(_) => return Ok(Value::Bool(false)),
        }
    }
    let ct = match ossl::cipher::encrypt(info, &key, &iv, data.as_bytes(), true) {
        Ok(c) => c,
        Err(e) => {
            e.push();
            return Ok(Value::Bool(false));
        }
    };
    let n = ct.len() as i64;
    set_out(a, 1, bin(ct));
    set_out(a, 2, Value::Array(ekeys));
    if a.has(5) {
        set_out(a, 5, bin(iv));
    }
    Ok(Value::Int(n))
}

/// openssl_open() (PHP manual).
pub(super) fn openssl_open(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.str(0);
    let ekey = a.str(2);
    let Some(k) = key_from_value(vm, a, &a.val(3), Want::Private, None)? else {
        kwarn(vm, a, "Unable to coerce parameter 4 into a private key")?;
        return Ok(Value::Bool(false));
    };
    let name = a.str(4);
    let Some(info) = ossl::cipher::lookup_bytes(name.as_bytes()) else {
        vm.fwarn(a, "Unknown cipher algorithm")?;
        return Ok(Value::Bool(false));
    };
    let iv = a.opt_str(5).map(|s| s.to_vec()).unwrap_or_default();
    if info.iv_len() > 0 && a.opt_str(5).is_none() {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #6 ($iv) cannot be null for the chosen cipher algorithm",
        );
    }
    if iv.len() != info.iv_len() {
        return Ok(Value::Bool(false));
    }
    let md = ossl::digest::Md::H(ossl::digest::HashAlgorithm::Sha1);
    let Ok(key) = ossl::key::rsa_private_decrypt(&k, ekey.as_bytes(), ossl::key::PAD_PKCS1, md)
    else {
        return Ok(Value::Bool(false));
    };
    if key.len() != info.key_len() {
        return Ok(Value::Bool(false));
    }
    match ossl::cipher::decrypt(info, &key, &iv, data.as_bytes(), true) {
        Ok(p) => {
            set_out(a, 1, bin(p));
            Ok(Value::Bool(true))
        }
        Err(e) => {
            e.push();
            Ok(Value::Bool(false))
        }
    }
}

#[allow(dead_code)]
fn unused(_: Rc<PKey>) {}
