//! PHP functions working on OpenSSLAsymmetricKey objects.

use crate::bn::Bn;
use crate::digest::Md;
use crate::ec::{Group, Point};
use crate::ecx::EcxKind;
use crate::pkey::{self, EcKey, Ecx, Ffc, PKey, Rsa};
use crate::req::{self, Req};
use crate::{err, zpp};
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

// -------------------------------------------------------------------------
// Object registry
// -------------------------------------------------------------------------

thread_local! {
    static REGISTRY: RefCell<HashMap<u64, (Rc<PKey>, bool)>> = RefCell::new(HashMap::new());
    static NEXT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// php_openssl_pkey_object_init()
pub fn make_object(vm: &mut Vm, key: PKey, is_private: bool) -> Value {
    let id = NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    REGISTRY.with(|r| r.borrow_mut().insert(id, (Rc::new(key), is_private)));
    let oid = vm.next_object_id();
    let mut obj = PhpObject::new(b"OpenSSLAsymmetricKey".to_vec(), oid);
    obj.set_property(b"__pkey_id".to_vec(), Value::Long(id as i64));
    Value::Object(Rc::new(RefCell::new(obj)))
}

pub fn is_pkey_object(v: &Value) -> bool {
    match v.deref() {
        Value::Object(o) => o
            .borrow()
            .class_name
            .eq_ignore_ascii_case(b"OpenSSLAsymmetricKey"),
        _ => false,
    }
}

/// The key held by an OpenSSLAsymmetricKey object.
pub fn from_object(v: &Value) -> Option<(Rc<PKey>, bool)> {
    if let Value::Object(o) = v.deref() {
        let o = o.borrow();
        if o.class_name.eq_ignore_ascii_case(b"OpenSSLAsymmetricKey") {
            let id = o.get_property(b"__pkey_id").to_long();
            return REGISTRY.with(|r| r.borrow().get(&(id as u64)).cloned());
        }
    }
    None
}

fn is_cert_object(v: &Value) -> bool {
    match v.deref() {
        Value::Object(o) => o
            .borrow()
            .class_name
            .eq_ignore_ascii_case(b"OpenSSLCertificate"),
        _ => false,
    }
}

// -------------------------------------------------------------------------
// php_openssl_pkey_from_zval()
// -------------------------------------------------------------------------

/// File path check for "file://" values (php_openssl_check_path_str).
pub fn check_path(
    vm: &mut Vm,
    fname: &str,
    arg_num: usize,
    path: &[u8],
) -> Result<Option<String>, VmError> {
    crate::x509::check_plain_path(vm, fname, arg_num, path, false, None)
}

/// Read the contents of a value that may be "file://path" or inline data.
/// Returns (data, is_file). On file open failure the BIO errors are queued.
pub fn read_maybe_file(
    vm: &mut Vm,
    fname: &str,
    arg_num: usize,
    s: &[u8],
) -> Result<Option<(Vec<u8>, bool)>, VmError> {
    if s.len() > 7 && &s[..7] == b"file://" {
        let Some(path) = check_path(vm, fname, arg_num, &s[7..])? else {
            return Ok(None);
        };
        match std::fs::read(&path) {
            Ok(d) => Ok(Some((d, true))),
            Err(e) => {
                err::bio_new_file_failed(&e);
                Ok(None)
            }
        }
    } else {
        Ok(Some((s.to_vec(), false)))
    }
}

/// php_openssl_pkey_from_zval(). `Ok(None)` = no key (warnings already
/// emitted where php-src emits them); `Err` = exception thrown.
pub fn from_zval(
    vm: &mut Vm,
    fname: &str,
    val: &Value,
    public: bool,
    passphrase: Option<&[u8]>,
    arg_num: usize,
) -> Result<Option<Rc<PKey>>, VmError> {
    let mut val = val.deref();
    let mut pass: Option<Vec<u8>> = passphrase.map(|p| p.to_vec());
    if let Value::Array(a) = &val {
        let a = a.borrow();
        let Some(phrase) = a.get_int(1).cloned() else {
            return Err(zpp::value_error(
                vm,
                "Key array must be of the form array(0 => key, 1 => phrase)".into(),
            ));
        };
        let phrase = phrase.deref();
        let ps = match &phrase {
            Value::String(s) => s.clone(),
            other => {
                let s = vm.value_to_string(other);
                if vm.current_exception.is_some() {
                    return Err(VmError {
                        message: "exception".into(),
                        line: vm.current_line,
                    });
                }
                s
            }
        };
        pass = Some(ps.as_bytes().to_vec());
        let Some(k) = a.get_int(0).cloned() else {
            return Err(zpp::value_error(
                vm,
                "Key array must be of the form array(0 => key, 1 => phrase)".into(),
            ));
        };
        drop(a);
        val = k.deref();
    }
    if let Some((key, is_priv)) = from_object(&val) {
        if !public && !is_priv {
            vm.emit_warning(&format!("{}(): Supplied key param is a public key", fname));
            return Ok(None);
        }
        if public && is_priv {
            vm.emit_warning(&format!(
                "{}(): Don't know how to get public key from this private key",
                fname
            ));
            return Ok(None);
        }
        return Ok(Some(key));
    }
    let mut cert_der: Option<Vec<u8>> = None;
    let mut key: Option<PKey> = None;
    if is_cert_object(&val) {
        cert_der = crate::x509::cert_der_from_object(&val);
    } else {
        if !matches!(val, Value::String(_) | Value::Object(_)) {
            return Ok(None);
        }
        let s = match &val {
            Value::String(s) => s.clone(),
            // (zval_try_get_string(): a non-Stringable object throws)
            other => vm.value_to_string_checked(other)?,
        };
        let bytes = s.as_bytes();
        let is_file = bytes.len() > 7 && &bytes[..7] == b"file://";
        if is_file && check_path(vm, fname, arg_num, &bytes[7..])?.is_none() {
            return Ok(None);
        }
        if public {
            let mark = err::mark();
            match crate::x509::cert_der_from_str(vm, fname, arg_num, bytes)? {
                Some(d) => cert_der = Some(d),
                None => {
                    err::restore(mark);
                    let Some((data, _)) = read_maybe_file(vm, fname, arg_num, bytes)? else {
                        return Ok(None);
                    };
                    key = pkey::read_public_pem(&data);
                    if key.is_none() {
                        // PEM_read_bio_PUBKEY_ex() goes through the decoders.
                        err::push(err::LIB_DECODER, err::R_UNSUPPORTED, "unsupported");
                    }
                }
            }
        } else {
            let Some((data, _)) = read_maybe_file(vm, fname, arg_num, bytes)? else {
                return Ok(None);
            };
            let pw = pass.clone().unwrap_or_default();
            match pkey::read_private_pem(&data, &pw) {
                Ok(k) => key = Some(k),
                Err(e) => pkey::push_read_error(&e),
            }
        }
    }
    if key.is_none() && public {
        if let Some(c) = cert_der {
            key = crate::x509::cert_spki(&c).and_then(|s| pkey::parse_spki(&s));
        }
    }
    Ok(key.map(Rc::new))
}

// -------------------------------------------------------------------------
// openssl_pkey_new()
// -------------------------------------------------------------------------

fn arr_bn(a: &PhpArray, name: &str) -> Option<Bn> {
    match a.get(&ArrayKey::String(PhpString::from_bytes(name.as_bytes()))) {
        Some(Value::String(s)) => Some(Bn::from_be(s.as_bytes())),
        _ => None,
    }
}

fn arr_str(a: &PhpArray, name: &str) -> Option<Vec<u8>> {
    match a.get(&ArrayKey::String(PhpString::from_bytes(name.as_bytes()))) {
        Some(Value::String(s)) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

fn init_rsa(a: &PhpArray) -> Option<PKey> {
    let n = arr_bn(a, "n")?;
    let d = arr_bn(a, "d")?;
    let e = arr_bn(a, "e").unwrap_or_default();
    Some(PKey::Rsa(Rsa {
        n,
        e,
        d: Some(d),
        p: arr_bn(a, "p"),
        q: arr_bn(a, "q"),
        dmp1: arr_bn(a, "dmp1"),
        dmq1: arr_bn(a, "dmq1"),
        iqmp: arr_bn(a, "iqmp"),
    }))
}

fn init_dsa(a: &PhpArray) -> Option<(PKey, bool)> {
    let p = arr_bn(a, "p")?;
    let q = arr_bn(a, "q")?;
    let g = arr_bn(a, "g")?;
    let priv_key = arr_bn(a, "priv_key");
    let pub_key = arr_bn(a, "pub_key");
    if let Some(y) = pub_key {
        let is_priv = priv_key.is_some();
        return Some((
            PKey::Dsa(Ffc {
                p,
                q: Some(q),
                g,
                pub_key: Some(y),
                priv_key,
            }),
            is_priv,
        ));
    }
    if !pkey::ffc_simple_validate(&p, Some(&q), &g) {
        err::push(err::LIB_DSA, 102, "bad ffc parameters");
        return None;
    }
    let (x, y) = pkey::generate_ffc(&p, Some(&q), &g);
    Some((
        PKey::Dsa(Ffc {
            p,
            q: Some(q),
            g,
            pub_key: Some(y),
            priv_key: Some(x),
        }),
        true,
    ))
}

fn init_dh(a: &PhpArray) -> Option<(PKey, bool)> {
    let p = arr_bn(a, "p")?;
    let g = arr_bn(a, "g")?;
    let q = arr_bn(a, "q");
    let priv_key = arr_bn(a, "priv_key");
    let mut pub_key = arr_bn(a, "pub_key");
    if pub_key.is_none() && (p.is_zero() || pkey::is_even(&p)) {
        // BN_mod_exp_mont() needs an odd modulus
        err::push(err::LIB_BN, 102, "called a function you should not call");
        return None;
    }
    if let Some(x) = &priv_key {
        if pub_key.is_none() {
            pub_key = pkey::dh_pub_from_priv(x, &g, &p);
        }
    }
    if pub_key.is_some() || priv_key.is_some() {
        let is_priv = priv_key.is_some();
        return Some((
            PKey::Dh(Ffc {
                p,
                q,
                g,
                pub_key,
                priv_key,
            }),
            is_priv,
        ));
    }
    if p.bits() < 512 {
        err::push(err::LIB_DH, 103, "modulus too small");
        return None;
    }
    let (x, y) = pkey::generate_ffc(&p, q.as_ref(), &g);
    Some((
        PKey::Dh(Ffc {
            p,
            q,
            g,
            pub_key: Some(y),
            priv_key: Some(x),
        }),
        true,
    ))
}

fn init_ec(vm: &mut Vm, a: &PhpArray) -> Option<(PKey, bool)> {
    let curve_name = arr_str(a, "curve_name").filter(|s| !s.is_empty());
    let group = if let Some(cn) = curve_name {
        let name = String::from_utf8_lossy(&cn).to_string();
        match Group::by_name(&name) {
            Some(g) => g,
            None => {
                vm.emit_warning(&format!(
                    "openssl_pkey_new(): Unknown elliptic curve (short) name {}",
                    name
                ));
                return None;
            }
        }
    } else {
        let p = arr_bn(a, "p");
        let ca = arr_bn(a, "a");
        let b = arr_bn(a, "b");
        let order = arr_bn(a, "order");
        let (Some(p), Some(ca), Some(b), Some(order)) =
            (p.clone(), ca.clone(), b.clone(), order.clone())
        else {
            if p.is_none() && ca.is_none() && b.is_none() && order.is_none() {
                vm.emit_warning("openssl_pkey_new(): Missing params: curve_name");
            } else {
                vm.emit_warning("openssl_pkey_new(): Missing params: curve_name or p, a, b, order");
            }
            return None;
        };
        let mut g = Group {
            p,
            a: ca,
            b,
            gx: Bn::default(),
            gy: Bn::default(),
            n: order,
            h: Bn::default(),
            seed: None,
            name: None,
            oid: None,
            explicit: true,
        };
        match arr_str(a, "generator").filter(|s| !s.is_empty()) {
            Some(generator) => match g.decode(&generator) {
                Some(Point::Aff(x, y)) => {
                    g.gx = x;
                    g.gy = y;
                }
                _ => return None,
            },
            None => {
                let (Some(gx), Some(gy)) = (arr_bn(a, "g_x"), arr_bn(a, "g_y")) else {
                    vm.emit_warning("openssl_pkey_new(): Missing params: generator or g_x and g_y");
                    return None;
                };
                if !g.on_curve(&Point::Aff(gx.clone(), gy.clone())) {
                    return None;
                }
                g.gx = gx;
                g.gy = gy;
            }
        }
        if let Some(seed) = arr_str(a, "seed").filter(|s| !s.is_empty()) {
            g.seed = Some(seed);
        }
        match arr_bn(a, "cofactor") {
            Some(h) if !h.is_zero() => g.h = h,
            _ => g.guess_cofactor(),
        }
        g.identify();
        g
    };
    let sm2 = group.oid == Some(pkey::OID_SM2);
    let mut d = None;
    let mut q = None;
    if !sm2 {
        d = arr_bn(a, "d");
        let x = arr_bn(a, "x");
        let y = arr_bn(a, "y");
        if let Some(dv) = &d {
            q = Some(group.pub_from_priv(dv));
        } else if let (Some(x), Some(y)) = (x, y) {
            let pt = Point::Aff(x, y);
            if !group.on_curve(&pt) {
                return None;
            }
            q = Some(pt);
        }
    }
    // EVP_PKEY_check() || EVP_PKEY_public_check_quick()
    let valid = match (&d, &q) {
        (Some(dv), Some(_)) => !dv.is_zero() && dv.to_box().lt(&group.n.to_box()),
        (None, Some(pt)) => *pt != Point::Inf,
        _ => false,
    };
    if valid {
        let is_priv = d.is_some();
        return Some((
            PKey::Ec(EcKey {
                group,
                pub_key: q,
                priv_key: d,
                sm2,
            }),
            is_priv,
        ));
    }
    Some((PKey::Ec(pkey::generate_ec(group)), true))
}

fn init_ecx(kind: EcxKind, a: &PhpArray) -> Option<(PKey, bool)> {
    let priv_key = arr_str(a, "priv_key").filter(|s| !s.is_empty());
    let pub_key = arr_str(a, "pub_key").filter(|s| !s.is_empty());
    let has_priv = a
        .get(&ArrayKey::String(PhpString::from_bytes(b"priv_key")))
        .is_some();
    let has_pub = a
        .get(&ArrayKey::String(PhpString::from_bytes(b"pub_key")))
        .is_some();
    if has_priv || has_pub {
        if let Some(p) = &priv_key {
            if p.len() != kind.key_len() {
                return None;
            }
            let derived = kind.pub_from_priv(p)?;
            if let Some(pk) = &pub_key {
                if *pk != derived {
                    return None;
                }
            }
            return Some((
                PKey::Ecx(Ecx {
                    kind,
                    pub_key: derived,
                    priv_key: Some(p.clone()),
                }),
                has_priv,
            ));
        }
        let pk = pub_key?;
        if pk.len() != kind.key_len() {
            return None;
        }
        return Some((
            PKey::Ecx(Ecx {
                kind,
                pub_key: pk,
                priv_key: None,
            }),
            has_priv,
        ));
    }
    Some((PKey::Ecx(pkey::generate_ecx(kind)), true))
}

/// php_openssl_generate_private_key()
pub fn generate_private_key(vm: &mut Vm, fname: &str, r: &Req) -> Option<PKey> {
    if r.priv_key_bits < 384 {
        vm.emit_warning(&format!(
            "{}(): Private key length must be at least 384 bits, configured to {}",
            fname, r.priv_key_bits
        ));
        return None;
    }
    let bits = r.priv_key_bits as usize;
    match r.priv_key_type {
        req::OPENSSL_KEYTYPE_RSA => {
            if bits < 512 {
                err::push(err::LIB_PROV, 171, "key size too small");
                return None;
            }
            if bits > 16384 {
                err::push(err::LIB_PROV, 171, "key size too large");
                return None;
            }
            pkey::generate_rsa(bits).map(PKey::Rsa)
        }
        req::OPENSSL_KEYTYPE_DSA => {
            // ffc_validate_LN() (non-FIPS): L >= 1024 with the default N.
            if bits < 1024 {
                err::push(err::LIB_DSA, 114, "bad ffc parameters");
                return None;
            }
            let (p, q, g) = pkey::generate_dsa_params(bits)?;
            let (x, y) = pkey::generate_ffc(&p, Some(&q), &g);
            Some(PKey::Dsa(Ffc {
                p,
                q: Some(q),
                g,
                pub_key: Some(y),
                priv_key: Some(x),
            }))
        }
        req::OPENSSL_KEYTYPE_DH => {
            let Some((p, g)) = pkey::generate_dh_params(bits) else {
                err::push(err::LIB_PROV, 182, "modulus too small");
                return None;
            };
            let (x, y) = pkey::generate_ffc(&p, None, &g);
            Some(PKey::Dh(Ffc {
                p,
                q: None,
                g,
                pub_key: Some(y),
                priv_key: Some(x),
            }))
        }
        req::OPENSSL_KEYTYPE_EC => {
            let Some(cn) = &r.curve_name else {
                vm.emit_warning(&format!(
                    "{}(): Missing configuration value: \"curve_name\" not set",
                    fname
                ));
                return None;
            };
            let g = Group::by_name(cn)?;
            Some(PKey::Ec(pkey::generate_ec(g)))
        }
        req::OPENSSL_KEYTYPE_X25519 => Some(PKey::Ecx(pkey::generate_ecx(EcxKind::X25519))),
        req::OPENSSL_KEYTYPE_ED25519 => Some(PKey::Ecx(pkey::generate_ecx(EcxKind::Ed25519))),
        req::OPENSSL_KEYTYPE_X448 => Some(PKey::Ecx(pkey::generate_ecx(EcxKind::X448))),
        req::OPENSSL_KEYTYPE_ED448 => Some(PKey::Ecx(pkey::generate_ecx(EcxKind::Ed448))),
        _ => {
            vm.emit_warning(&format!("{}(): Unsupported private key type", fname));
            None
        }
    }
}

fn sub_array(args: &PhpArray, name: &str) -> Option<PhpArray> {
    match args.get(&ArrayKey::String(PhpString::from_bytes(name.as_bytes()))) {
        Some(Value::Array(a)) => Some(a.borrow().clone()),
        _ => None,
    }
}

pub fn openssl_pkey_new(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_pkey_new";
    let opts = zpp::array_nullable(vm, args, 0, f, "options")?;
    let opts_arr = opts.as_ref().map(|a| a.borrow().clone());
    if let Some(a) = &opts_arr {
        if let Some(d) = sub_array(a, "rsa") {
            return Ok(match init_rsa(&d) {
                Some(k) => make_object(vm, k, true),
                None => Value::False,
            });
        } else if let Some(d) = sub_array(a, "dsa") {
            return Ok(match init_dsa(&d) {
                Some((k, p)) => make_object(vm, k, p),
                None => Value::False,
            });
        } else if let Some(d) = sub_array(a, "dh") {
            return Ok(match init_dh(&d) {
                Some((k, p)) => make_object(vm, k, p),
                None => Value::False,
            });
        } else if let Some(d) = sub_array(a, "ec") {
            return Ok(match init_ec(vm, &d) {
                Some((k, p)) => make_object(vm, k, p),
                None => Value::False,
            });
        }
        for (name, kind) in [
            ("x25519", EcxKind::X25519),
            ("ed25519", EcxKind::Ed25519),
            ("x448", EcxKind::X448),
            ("ed448", EcxKind::Ed448),
        ] {
            if let Some(d) = sub_array(a, name) {
                return Ok(match init_ecx(kind, &d) {
                    Some((k, p)) => make_object(vm, k, p),
                    None => Value::False,
                });
            }
        }
    }
    let Some(r) = req::parse(vm, opts_arr.as_ref()) else {
        return Ok(Value::False);
    };
    Ok(match generate_private_key(vm, f, &r) {
        Some(k) => make_object(vm, k, true),
        None => Value::False,
    })
}

// -------------------------------------------------------------------------
// Export / get / details
// -------------------------------------------------------------------------

fn export_pem(
    vm: &mut Vm,
    key: &PKey,
    passphrase: Option<&[u8]>,
    opts: Option<&PhpArray>,
) -> Option<Option<String>> {
    let r = req::parse(vm, opts)?;
    let cipher = match passphrase {
        Some(_) if r.priv_key_encrypt => Some(
            r.priv_key_encrypt_cipher
                .unwrap_or_else(|| crate::cipher::by_name("des-ede3-cbc").unwrap()),
        ),
        _ => None,
    };
    Some(key.private_pem(cipher, passphrase.unwrap_or(&[])))
}

pub fn openssl_pkey_export(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_pkey_export";
    let key_arg = args.first().cloned().unwrap_or(Value::Null);
    let pass = zpp::str_nullable(vm, args, 2, f, "passphrase")?;
    let opts = zpp::array_nullable(vm, args, 3, f, "options")?;
    let pw = pass.as_ref().map(|p| p.as_bytes().to_vec());
    let Some(key) = from_zval(
        vm,
        f,
        &key_arg,
        false,
        Some(pw.as_deref().unwrap_or(&[])),
        1,
    )?
    else {
        vm.emit_warning(&format!("{}(): Cannot get key from parameter 1", f));
        return Ok(Value::False);
    };
    let opts_arr = opts.map(|a| a.borrow().clone());
    match export_pem(vm, &key, pw.as_deref(), opts_arr.as_ref()) {
        Some(Some(pem)) => {
            zpp::set_ref(args, 1, Value::String(PhpString::from_string(pem)));
            Ok(Value::True)
        }
        _ => Ok(Value::False),
    }
}

pub fn openssl_pkey_export_to_file(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_pkey_export_to_file";
    let key_arg = args.first().cloned().unwrap_or(Value::Null);
    let filename = zpp::path(vm, args, 1, f, "output_filename")?;
    let pass = zpp::str_nullable(vm, args, 2, f, "passphrase")?;
    let opts = zpp::array_nullable(vm, args, 3, f, "options")?;
    let pw = pass.as_ref().map(|p| p.as_bytes().to_vec());
    let Some(key) = from_zval(
        vm,
        f,
        &key_arg,
        false,
        Some(pw.as_deref().unwrap_or(&[])),
        1,
    )?
    else {
        vm.emit_warning(&format!("{}(): Cannot get key from parameter 1", f));
        return Ok(Value::False);
    };
    let Some(path) = check_path(vm, f, 2, filename.as_bytes())? else {
        return Ok(Value::False);
    };
    let opts_arr = opts.map(|a| a.borrow().clone());
    let Some(r) = req::parse(vm, opts_arr.as_ref()) else {
        return Ok(Value::False);
    };
    let file = match std::fs::File::create(&path) {
        Ok(fh) => fh,
        Err(e) => {
            err::bio_new_file_failed(&e);
            return Ok(Value::False);
        }
    };
    let cipher = match &pw {
        Some(_) if r.priv_key_encrypt => Some(
            r.priv_key_encrypt_cipher
                .unwrap_or_else(|| crate::cipher::by_name("des-ede3-cbc").unwrap()),
        ),
        _ => None,
    };
    match key.private_pem(cipher, pw.as_deref().unwrap_or(&[])) {
        Some(pem) => {
            use std::io::Write;
            let mut file = file;
            if file.write_all(pem.as_bytes()).is_err() {
                return Ok(Value::False);
            }
            Ok(Value::True)
        }
        None => Ok(Value::False),
    }
}

pub fn openssl_pkey_get_public(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let key_arg = args.first().cloned().unwrap_or(Value::Null);
    match from_zval(vm, "openssl_pkey_get_public", &key_arg, true, None, 1)? {
        Some(k) => Ok(make_object(vm, (*k).clone(), false)),
        None => Ok(Value::False),
    }
}

pub fn openssl_pkey_get_private(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_pkey_get_private";
    let key_arg = args.first().cloned().unwrap_or(Value::Null);
    let pass = if zpp::is_passed(args, 1) {
        zpp::str_nullable(vm, args, 1, f, "passphrase")?
    } else {
        Some(PhpString::empty())
    };
    let pw = pass.as_ref().map(|p| p.as_bytes().to_vec());
    match from_zval(vm, f, &key_arg, false, pw.as_deref(), 1)? {
        Some(k) => Ok(make_object(vm, (*k).clone(), true)),
        None => Ok(Value::False),
    }
}

fn need_key_object(
    vm: &mut Vm,
    args: &[Value],
    idx: usize,
    f: &str,
    pname: &str,
) -> Result<(Rc<PKey>, bool), VmError> {
    let v = args.get(idx).cloned().unwrap_or(Value::Null);
    match from_object(&v) {
        Some(k) => Ok(k),
        None => Err(zpp::arg_type_error(
            vm,
            f,
            idx + 1,
            pname,
            "OpenSSLAsymmetricKey",
            &v.deref(),
        )),
    }
}

fn set_bn(arr: &mut PhpArray, name: &str, v: &Option<Bn>) {
    if let Some(v) = v {
        let b = if v.is_zero() { Vec::new() } else { v.to_vec() };
        arr.set(
            ArrayKey::String(PhpString::from_bytes(name.as_bytes())),
            zpp::bytes(b),
        );
    }
}

fn key_name(k: &str) -> ArrayKey {
    ArrayKey::String(PhpString::from_bytes(k.as_bytes()))
}

pub fn openssl_pkey_get_details(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (key, _) = need_key_object(vm, args, 0, "openssl_pkey_get_details", "key")?;
    let mut res = PhpArray::new();
    res.set(key_name("bits"), Value::Long(key.bits()));
    res.set(
        key_name("key"),
        Value::String(PhpString::from_string(key.public_pem())),
    );
    let mut sub = PhpArray::new();
    let sub_name = match &*key {
        PKey::Rsa(r) => {
            set_bn(&mut sub, "n", &Some(r.n.clone()));
            set_bn(&mut sub, "e", &Some(r.e.clone()));
            set_bn(&mut sub, "d", &r.d);
            set_bn(&mut sub, "p", &r.p);
            set_bn(&mut sub, "q", &r.q);
            set_bn(&mut sub, "dmp1", &r.dmp1);
            set_bn(&mut sub, "dmq1", &r.dmq1);
            set_bn(&mut sub, "iqmp", &r.iqmp);
            "rsa"
        }
        PKey::Dsa(fc) => {
            set_bn(&mut sub, "p", &Some(fc.p.clone()));
            set_bn(&mut sub, "q", &fc.q);
            set_bn(&mut sub, "g", &Some(fc.g.clone()));
            set_bn(&mut sub, "priv_key", &fc.priv_key);
            set_bn(&mut sub, "pub_key", &fc.pub_key);
            "dsa"
        }
        PKey::Dh(fc) => {
            set_bn(&mut sub, "p", &Some(fc.p.clone()));
            set_bn(&mut sub, "g", &Some(fc.g.clone()));
            set_bn(&mut sub, "priv_key", &fc.priv_key);
            set_bn(&mut sub, "pub_key", &fc.pub_key);
            "dh"
        }
        PKey::Ec(e) => {
            if !e.group.explicit || e.group.name.is_some() {
                if let Some(n) = e.group.name {
                    sub.set(key_name("curve_name"), zpp::sval(n));
                    if let Some(o) = e.group.oid {
                        sub.set(key_name("curve_oid"), zpp::sval(o));
                    }
                }
            }
            if let Some(Point::Aff(x, y)) = &e.pub_key {
                set_bn(&mut sub, "x", &Some(x.clone()));
                set_bn(&mut sub, "y", &Some(y.clone()));
            }
            set_bn(&mut sub, "d", &e.priv_key);
            "ec"
        }
        PKey::Ecx(x) => {
            if let Some(p) = &x.priv_key {
                sub.set(key_name("priv_key"), zpp::bytes(p.clone()));
            }
            sub.set(key_name("pub_key"), zpp::bytes(x.pub_key.clone()));
            match x.kind {
                EcxKind::X25519 => "x25519",
                EcxKind::Ed25519 => "ed25519",
                EcxKind::X448 => "x448",
                EcxKind::Ed448 => "ed448",
            }
        }
    };
    res.set(key_name(sub_name), Value::Array(Rc::new(RefCell::new(sub))));
    res.set(key_name("type"), Value::Long(key.type_const()));
    Ok(Value::Array(Rc::new(RefCell::new(res))))
}

pub fn openssl_pkey_free(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.emit_function_deprecation(
        "Function openssl_pkey_free() is deprecated since 8.0, as OpenSSLAsymmetricKey objects are freed automatically",
    )?;
    let args = &vm.zpp_self_check(b"openssl_pkey_free", args)?[..];
    need_key_object(vm, args, 0, "openssl_pkey_free", "key")?;
    Ok(Value::Null)
}

pub fn openssl_free_key(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.emit_function_deprecation(
        "Function openssl_free_key() is deprecated since 8.0, as OpenSSLAsymmetricKey objects are freed automatically",
    )?;
    let args = &vm.zpp_self_check(b"openssl_free_key", args)?[..];
    need_key_object(vm, args, 0, "openssl_free_key", "key")?;
    Ok(Value::Null)
}

// -------------------------------------------------------------------------
// Sign / verify
// -------------------------------------------------------------------------

/// php_openssl_get_evp_md_from_algo()
pub fn md_from_algo(algo: i64) -> Option<Md> {
    Some(match algo {
        1 => Md::Sha1,
        2 => Md::Md5,
        3 => Md::Md4,
        6 => Md::Sha224,
        7 => Md::Sha256,
        8 => Md::Sha384,
        9 => Md::Sha512,
        10 => Md::Ripemd160,
        _ => return None,
    })
}

enum Algo {
    Str(String),
    Long(i64),
}

fn algo_arg(vm: &mut Vm, args: &[Value], idx: usize, f: &str) -> Result<Algo, VmError> {
    match zpp::arg(args, idx) {
        None => Ok(Algo::Long(1)),
        Some(Value::Long(l)) => Ok(Algo::Long(l)),
        Some(Value::String(s)) => Ok(Algo::Str(s.to_string_lossy())),
        Some(Value::Double(d)) => Ok(Algo::Long(d as i64)),
        Some(Value::True) => Ok(Algo::Long(1)),
        Some(Value::False) => Ok(Algo::Long(0)),
        Some(Value::Null) => {
            vm.emit_deprecated(&format!(
                "{}(): Passing null to parameter #{} ($algorithm) of type string|int is deprecated",
                f,
                idx + 1
            ));
            Ok(Algo::Str(String::new()))
        }
        Some(v) => Err(zpp::arg_type_error(
            vm,
            f,
            idx + 1,
            "algorithm",
            "string|int",
            &v,
        )),
    }
}

/// Resolve the digest: Ok(Some(md)) / Ok(None) default (algo 0) / Err unknown.
fn resolve_md(a: &Algo) -> Result<Option<Md>, ()> {
    match a {
        Algo::Str(s) => crate::digest::fetch(s).map(Some).ok_or(()),
        Algo::Long(0) => Ok(None),
        Algo::Long(l) => md_from_algo(*l).map(Some).ok_or(()),
    }
}

/// php_openssl_setup_rsa_padding(): Ok(pss)
fn setup_padding(vm: &mut Vm, f: &str, key: &PKey, padding: i64) -> Result<bool, ()> {
    if padding != 0 {
        if !matches!(key, PKey::Rsa(_)) {
            vm.emit_warning(&format!(
                "{}(): Padding parameter is only supported for RSA keys",
                f
            ));
            return Err(());
        }
        if padding == pkey::RSA_PKCS1_PSS_PADDING {
            return Ok(true);
        } else if padding != pkey::RSA_PKCS1_PADDING {
            vm.emit_warning(&format!("{}(): Unknown padding type", f));
            return Err(());
        }
    }
    Ok(false)
}

/// Default digest for EVP_DigestSignInit with md=NULL.
fn default_md(key: &PKey) -> Option<Md> {
    match key {
        PKey::Ecx(_) => None,
        PKey::Ec(e) if e.sm2 => Some(Md::Sm3),
        _ => Some(Md::Sha256),
    }
}

pub fn openssl_sign(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_sign";
    let data = zpp::str(vm, args, 0, f, "data")?;
    let key_arg = args.get(2).cloned().unwrap_or(Value::Null);
    let algo = algo_arg(vm, args, 3, f)?;
    let padding = if zpp::is_passed(args, 4) {
        zpp::long(vm, args, 4, f, "padding")?
    } else {
        0
    };
    let Some(key) = from_zval(vm, f, &key_arg, false, Some(&[]), 3)? else {
        vm.emit_warning(&format!(
            "{}(): Supplied key param cannot be coerced into a private key",
            f
        ));
        return Ok(Value::False);
    };
    let md = match resolve_md(&algo) {
        Ok(m) => m,
        Err(()) => {
            vm.emit_warning(&format!("{}(): Unknown digest algorithm", f));
            return Ok(Value::False);
        }
    };
    let md = md.or_else(|| default_md(&key));
    let Ok(pss) = setup_padding(vm, f, &key, padding) else {
        return Ok(Value::False);
    };
    match key.sign(md, data.as_bytes(), pss) {
        Ok(sig) => {
            zpp::set_ref(args, 1, zpp::bytes(sig));
            Ok(Value::True)
        }
        Err(_) => Ok(Value::False),
    }
}

pub fn openssl_verify(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_verify";
    let data = zpp::str(vm, args, 0, f, "data")?;
    let sig = zpp::str(vm, args, 1, f, "signature")?;
    let key_arg = args.get(2).cloned().unwrap_or(Value::Null);
    let algo = algo_arg(vm, args, 3, f)?;
    let padding = if zpp::is_passed(args, 4) {
        zpp::long(vm, args, 4, f, "padding")?
    } else {
        0
    };
    let md = match resolve_md(&algo) {
        Ok(m) => m,
        Err(()) => {
            vm.emit_warning(&format!("{}(): Unknown digest algorithm", f));
            return Ok(Value::False);
        }
    };
    let Some(key) = from_zval(vm, f, &key_arg, true, None, 3)? else {
        vm.emit_warning(&format!(
            "{}(): Supplied key param cannot be coerced into a public key",
            f
        ));
        return Ok(Value::False);
    };
    let md = md.or_else(|| default_md(&key));
    let Ok(pss) = setup_padding(vm, f, &key, padding) else {
        return Ok(Value::Long(-1));
    };
    Ok(Value::Long(key.verify(
        md,
        data.as_bytes(),
        sig.as_bytes(),
        pss,
    )))
}

// -------------------------------------------------------------------------
// RSA encrypt / decrypt
// -------------------------------------------------------------------------

fn rsa_of(key: &PKey) -> Option<&Rsa> {
    match key {
        PKey::Rsa(r) => Some(r),
        _ => None,
    }
}

/// EVP_PKEY_sign()/EVP_PKEY_verify_recover() failure after the provider.
fn provider_signature_failure() {
    err::push(err::LIB_EVP, 234, "provider signature failure");
}

fn not_rsa() {
    err::push(
        err::LIB_EVP,
        150,
        "operation not supported for this keytype",
    );
}

/// php_openssl_set_rsa_padding_and_digest() digest part.
fn oaep_digest(vm: &mut Vm, f: &str, padding: i64, digest: &Option<PhpString>) -> Result<Md, ()> {
    match digest {
        Some(d) => {
            let name = d.to_string_lossy();
            match crate::digest::fetch(&name) {
                Some(m) => Ok(if padding == pkey::RSA_PKCS1_OAEP_PADDING {
                    m
                } else {
                    Md::Sha1
                }),
                None => {
                    vm.emit_warning(&format!("{}(): Unknown digest algorithm: {}", f, name));
                    Err(())
                }
            }
        }
        None => Ok(Md::Sha1),
    }
}

/// The asymmetric-cipher provider refuses only PSS as a padding mode (and
/// says nothing); other unknown modes fail in RSA_*_crypt().
fn valid_enc_padding(p: i64) -> bool {
    p != pkey::RSA_PKCS1_PSS_PADDING
}

pub fn openssl_private_encrypt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_private_encrypt";
    let data = zpp::str(vm, args, 0, f, "data")?;
    let key_arg = args.get(2).cloned().unwrap_or(Value::Null);
    let padding = if zpp::is_passed(args, 3) {
        zpp::long(vm, args, 3, f, "padding")?
    } else {
        1
    };
    let Some(key) = from_zval(vm, f, &key_arg, false, Some(&[]), 3)? else {
        vm.emit_warning(&format!("{}(): key param is not a valid private key", f));
        return Ok(Value::False);
    };
    let Some(r) = rsa_of(&key) else {
        // EVP_PKEY_sign_init() works for EC/DSA keys; the RSA padding
        // ctrl then fails quietly.
        if !matches!(&*key, PKey::Ec(_) | PKey::Dsa(_)) {
            not_rsa();
        }
        return Ok(Value::False);
    };
    if !matches!(padding, pkey::RSA_PKCS1_PADDING | pkey::RSA_NO_PADDING) {
        err::push(err::LIB_PROV, 165, "illegal or unsupported padding mode");
        return Ok(Value::False);
    }
    match r.sign_raw(data.as_bytes(), padding) {
        Some(out) => {
            zpp::set_ref(args, 1, zpp::bytes(out));
            Ok(Value::True)
        }
        None => {
            pkey::prov_rsa_lib();
            provider_signature_failure();
            Ok(Value::False)
        }
    }
}

pub fn openssl_private_decrypt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_private_decrypt";
    let data = zpp::str(vm, args, 0, f, "data")?;
    let key_arg = args.get(2).cloned().unwrap_or(Value::Null);
    let padding = if zpp::is_passed(args, 3) {
        zpp::long(vm, args, 3, f, "padding")?
    } else {
        1
    };
    let digest = zpp::str_nullable(vm, args, 4, f, "digest_algo")?;
    if let Some(d) = &digest {
        if d.as_bytes().contains(&0) {
            return Err(zpp::arg_value_error(
                vm,
                f,
                5,
                "digest_algo",
                "must not contain any null bytes",
            ));
        }
    }
    let Some(key) = from_zval(vm, f, &key_arg, false, Some(&[]), 3)? else {
        vm.emit_warning(&format!(
            "{}(): key parameter is not a valid private key",
            f
        ));
        return Ok(Value::False);
    };
    let Some(r) = rsa_of(&key) else {
        not_rsa();
        return Ok(Value::False);
    };
    if !valid_enc_padding(padding) {
        return Ok(Value::False);
    }
    let Ok(md) = oaep_digest(vm, f, padding, &digest) else {
        return Ok(Value::False);
    };
    match r.decrypt(data.as_bytes(), padding, md) {
        Some(out) => {
            zpp::set_ref(args, 1, zpp::bytes(out));
            Ok(Value::True)
        }
        None => Ok(Value::False),
    }
}

pub fn openssl_public_encrypt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_public_encrypt";
    let data = zpp::str(vm, args, 0, f, "data")?;
    let key_arg = args.get(2).cloned().unwrap_or(Value::Null);
    let padding = if zpp::is_passed(args, 3) {
        zpp::long(vm, args, 3, f, "padding")?
    } else {
        1
    };
    let digest = zpp::str_nullable(vm, args, 4, f, "digest_algo")?;
    if let Some(d) = &digest {
        if d.as_bytes().contains(&0) {
            return Err(zpp::arg_value_error(
                vm,
                f,
                5,
                "digest_algo",
                "must not contain any null bytes",
            ));
        }
    }
    let Some(key) = from_zval(vm, f, &key_arg, true, None, 3)? else {
        vm.emit_warning(&format!("{}(): key parameter is not a valid public key", f));
        return Ok(Value::False);
    };
    let Some(r) = rsa_of(&key) else {
        not_rsa();
        return Ok(Value::False);
    };
    if !valid_enc_padding(padding) {
        return Ok(Value::False);
    }
    let Ok(md) = oaep_digest(vm, f, padding, &digest) else {
        return Ok(Value::False);
    };
    match r.encrypt(data.as_bytes(), padding, md) {
        Some(out) => {
            zpp::set_ref(args, 1, zpp::bytes(out));
            Ok(Value::True)
        }
        None => Ok(Value::False),
    }
}

pub fn openssl_public_decrypt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_public_decrypt";
    let data = zpp::str(vm, args, 0, f, "data")?;
    let key_arg = args.get(2).cloned().unwrap_or(Value::Null);
    let padding = if zpp::is_passed(args, 3) {
        zpp::long(vm, args, 3, f, "padding")?
    } else {
        1
    };
    let Some(key) = from_zval(vm, f, &key_arg, true, None, 3)? else {
        vm.emit_warning(&format!("{}(): key parameter is not a valid public key", f));
        return Ok(Value::False);
    };
    let Some(r) = rsa_of(&key) else {
        not_rsa();
        return Ok(Value::False);
    };
    if !matches!(padding, pkey::RSA_PKCS1_PADDING | pkey::RSA_NO_PADDING) {
        err::push(err::LIB_PROV, 165, "illegal or unsupported padding mode");
        return Ok(Value::False);
    }
    match r.verify_recover(data.as_bytes(), padding) {
        // RSA_public_decrypt() returning 0 bytes is a failure too.
        Some(out) if !out.is_empty() => {
            zpp::set_ref(args, 1, zpp::bytes(out));
            Ok(Value::True)
        }
        _ => {
            pkey::prov_rsa_lib();
            provider_signature_failure();
            Ok(Value::False)
        }
    }
}

// -------------------------------------------------------------------------
// Seal / open
// -------------------------------------------------------------------------

pub fn openssl_seal(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_seal";
    let data = zpp::str(vm, args, 0, f, "data")?;
    let pubkeys = zpp::array(vm, args, 3, f, "public_key")?;
    let method = zpp::str(vm, args, 4, f, "cipher_algo")?;
    let iv_given = matches!(args.get(5), Some(Value::Reference(_)));
    let keys: Vec<Value> = pubkeys.borrow().iter().map(|(_, v)| v.clone()).collect();
    if keys.is_empty() {
        return Err(zpp::must_not_be_empty(vm, f, 4, "public_key"));
    }
    let Some(spec) = crate::cipher::fetch(&method.to_string_lossy()) else {
        vm.emit_warning(&format!("{}(): Unknown cipher algorithm", f));
        return Ok(Value::False);
    };
    let iv_len = spec.iv_len;
    if !iv_given && iv_len > 0 {
        return Err(zpp::arg_value_error(
            vm,
            f,
            6,
            "iv",
            "cannot be null for the chosen cipher algorithm",
        ));
    }
    let mut pkeys = Vec::new();
    for (i, k) in keys.iter().enumerate() {
        match from_zval(vm, f, k, true, None, 4)? {
            Some(pk) => pkeys.push(pk),
            None => {
                vm.emit_warning(&format!(
                    "{}(): Not a public key ({}th member of pubkeys)",
                    f,
                    i + 1
                ));
                return Ok(Value::False);
            }
        }
    }
    use purecrypto::rng::RngCore;
    let mut rng = purecrypto::rng::OsRng;
    let mut sym = vec![0u8; spec.key_len];
    rng.fill_bytes(&mut sym);
    let mut iv = vec![0u8; iv_len];
    rng.fill_bytes(&mut iv);
    let mut eks = Vec::new();
    for k in &pkeys {
        let Some(r) = rsa_of(k) else {
            not_rsa();
            return Ok(Value::False);
        };
        match r.encrypt(&sym, pkey::RSA_PKCS1_PADDING, Md::Sha1) {
            Some(e) => eks.push(e),
            None => return Ok(Value::False),
        }
    }
    // An AEAD cipher seals fine (the tag is simply not kept).
    let op = crate::cipher::Op {
        spec,
        key: &sym,
        iv: &iv,
        enc: true,
        padding: true,
        aad: &[],
        tag_len: 16,
        tag: None,
    };
    let out = match crate::cipher::run(&op, data.as_bytes()) {
        Ok((o, _)) => o,
        Err(_) => return Ok(Value::False),
    };
    let len = out.len() as i64;
    if len > 0 {
        zpp::set_ref(args, 1, zpp::bytes(out));
        let mut arr = PhpArray::new();
        for e in eks {
            arr.push(zpp::bytes(e));
        }
        zpp::set_ref(args, 2, Value::Array(Rc::new(RefCell::new(arr))));
        if iv_given {
            zpp::set_ref(args, 5, zpp::bytes(iv));
        }
    }
    Ok(Value::Long(len))
}

pub fn openssl_open(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_open";
    let data = zpp::str(vm, args, 0, f, "data")?;
    let ekey = zpp::str(vm, args, 2, f, "encrypted_key")?;
    let key_arg = args.get(3).cloned().unwrap_or(Value::Null);
    let method = zpp::str(vm, args, 4, f, "cipher_algo")?;
    let iv = zpp::str_nullable(vm, args, 5, f, "iv")?;
    let Some(key) = from_zval(vm, f, &key_arg, false, Some(&[]), 4)? else {
        vm.emit_warning(&format!(
            "{}(): Unable to coerce parameter 4 into a private key",
            f
        ));
        return Ok(Value::False);
    };
    let Some(spec) = crate::cipher::fetch(&method.to_string_lossy()) else {
        vm.emit_warning(&format!("{}(): Unknown cipher algorithm", f));
        return Ok(Value::False);
    };
    let mut ivb = Vec::new();
    if spec.iv_len > 0 {
        let Some(iv) = iv else {
            return Err(zpp::arg_value_error(
                vm,
                f,
                6,
                "iv",
                "cannot be null for the chosen cipher algorithm",
            ));
        };
        if iv.len() != spec.iv_len {
            vm.emit_warning(&format!("{}(): IV length is invalid", f));
            return Ok(Value::False);
        }
        ivb = iv.as_bytes().to_vec();
    }
    let Some(r) = rsa_of(&key) else {
        not_rsa();
        return Ok(Value::False);
    };
    let Some(sym) = r.decrypt(ekey.as_bytes(), pkey::RSA_PKCS1_PADDING, Md::Sha1) else {
        return Ok(Value::False);
    };
    if sym.len() != spec.key_len && !spec.var_key {
        err::evp_invalid_key_length();
        return Ok(Value::False);
    }
    let op = crate::cipher::Op {
        spec,
        key: &sym,
        iv: &ivb,
        enc: false,
        padding: true,
        aad: &[],
        tag_len: 16,
        // No tag is ever set: ChaCha20-Poly1305 then verifies nothing,
        // the other AEAD modes fail.
        tag: (spec.algo == crate::cipher::Algo::ChaCha20Poly1305).then_some(&[][..]),
    };
    if spec.is_aead() && op.tag.is_none() {
        return Ok(Value::False);
    }
    match crate::cipher::run(&op, data.as_bytes()) {
        Ok((o, _)) if !o.is_empty() => {
            zpp::set_ref(args, 1, zpp::bytes(o));
            Ok(Value::True)
        }
        _ => Ok(Value::False),
    }
}

// -------------------------------------------------------------------------
// Key agreement
// -------------------------------------------------------------------------

pub fn openssl_dh_compute_key(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_dh_compute_key";
    let pubs = zpp::str(vm, args, 0, f, "public_key")?;
    let (key, _) = need_key_object(vm, args, 1, f, "private_key")?;
    let PKey::Dh(d) = &*key else {
        return Ok(Value::False);
    };
    let peer = PKey::Dh(Ffc {
        p: d.p.clone(),
        q: d.q.clone(),
        g: d.g.clone(),
        pub_key: Some(Bn::from_be(pubs.as_bytes())),
        priv_key: None,
    });
    match key.derive(&peer) {
        Some(s) => Ok(zpp::bytes(s)),
        None => Ok(Value::False),
    }
}

pub fn openssl_pkey_derive(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_pkey_derive";
    let peer_arg = args.first().cloned().unwrap_or(Value::Null);
    let priv_arg = args.get(1).cloned().unwrap_or(Value::Null);
    let key_len = if zpp::is_passed(args, 2) {
        zpp::long(vm, args, 2, f, "key_length")?
    } else {
        0
    };
    if zpp::is_passed(args, 2) {
        vm.emit_deprecated(&format!(
            "{}(): the $key_length parameter is deprecated as it is either ignored or truncates the key",
            f
        ));
        if vm.current_exception.is_some() {
            return Err(VmError {
                message: "exception".into(),
                line: vm.current_line,
            });
        }
    }
    if key_len < 0 {
        return Err(zpp::arg_value_error(
            vm,
            f,
            3,
            "key_length",
            "must be greater than or equal to 0",
        ));
    }
    let Some(key) = from_zval(vm, f, &priv_arg, false, Some(&[]), 2)? else {
        return Ok(Value::False);
    };
    let Some(peer) = from_zval(vm, f, &peer_arg, true, None, 1)? else {
        return Ok(Value::False);
    };
    match key.derive(&peer) {
        Some(mut s) => {
            if key_len > 0 {
                let kl = key_len as usize;
                match &*key {
                    // dh_plain_derive() / ecx_derive(): PROV_R_OUTPUT_BUFFER_TOO_SMALL;
                    // only ECDH truncates to the requested length.
                    PKey::Dh(_) | PKey::Ecx(_) if kl < s.len() => {
                        err::push(err::LIB_PROV, 106, "output buffer too small");
                        return Ok(Value::False);
                    }
                    _ => {
                        if kl < s.len() {
                            s.truncate(kl);
                        }
                    }
                }
            }
            Ok(zpp::bytes(s))
        }
        None => Ok(Value::False),
    }
}

pub fn openssl_pbkdf2(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_pbkdf2";
    let password = zpp::str(vm, args, 0, f, "password")?;
    let salt = zpp::str(vm, args, 1, f, "salt")?;
    let key_length = zpp::long(vm, args, 2, f, "key_length")?;
    let iterations = zpp::long(vm, args, 3, f, "iterations")?;
    let mut algo = if zpp::is_passed(args, 4) {
        zpp::str(vm, args, 4, f, "digest_algo")?.to_string_lossy()
    } else {
        String::new()
    };
    if algo.is_empty() {
        algo = "sha1".into();
    }
    // PHP_OPENSSL_CHECK_LONG_TO_INT()
    for (v, n, pname, what) in [
        (key_length, 3, "key_length", "key is too long"),
        (iterations, 4, "iterations", "iterations is too long"),
    ] {
        if i32::try_from(v).is_err() {
            return Err(zpp::arg_value_error(vm, f, n, pname, what));
        }
    }
    if key_length <= 0 {
        return Err(zpp::arg_value_error(
            vm,
            f,
            3,
            "key_length",
            "must be greater than 0",
        ));
    }
    let Some(md) = crate::digest::fetch(&algo) else {
        vm.emit_warning(&format!("{}(): Unknown digest algorithm", f));
        return Ok(Value::False);
    };
    // PKCS5_PBKDF2_HMAC() through the PBKDF2 KDF provider.
    if iterations < 0 {
        crate::err::push(
            crate::err::LIB_CRYPTO,
            125,
            "param unsigned integer negative value unsupported",
        );
        return Ok(Value::False);
    }
    if iterations == 0 {
        crate::err::push(crate::err::LIB_PROV, 123, "invalid iteration count");
        return Ok(Value::False);
    }
    if md.is_xof() {
        crate::err::push(crate::err::LIB_PROV, 183, "xof digests not allowed");
        return Ok(Value::False);
    }
    Ok(zpp::bytes(crate::digest::pbkdf2(
        md,
        password.as_bytes(),
        salt.as_bytes(),
        iterations as u32,
        key_length as usize,
    )))
}

pub fn openssl_get_curve_names(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(crate::string_list(
        crate::ec::curve_names().into_iter().map(|s| s.to_string()),
    ))
}

// -------------------------------------------------------------------------
// Helpers for sibling modules
// -------------------------------------------------------------------------

pub fn register_class(vm: &mut Vm) {
    use goro_core::object::ClassEntry;
    for name in [
        b"OpenSSLAsymmetricKey".as_ref(),
        b"OpenSSLCertificate".as_ref(),
        b"OpenSSLCertificateSigningRequest".as_ref(),
    ] {
        let mut class = ClassEntry::new(name.to_vec());
        class.is_final = true;
        let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
        vm.classes.insert(lower, class);
    }
}
