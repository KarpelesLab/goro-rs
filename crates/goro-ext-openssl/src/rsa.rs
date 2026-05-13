//! RSA key generation, parsing, signing and verification using pure-Rust crates
//! (`rsa`, `pkcs1`, `pkcs8`). Keys are stored in a thread-local registry indexed
//! by an integer id stored on the `OpenSSLAsymmetricKey` object as the hidden
//! property `__pkey_id`.

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

use rsa::pkcs1v15::{Signature as PkcsSig, SigningKey, VerifyingKey};
use rsa::pss::{
    BlindedSigningKey as PssBlindedSigningKey, Signature as PssSig,
    VerifyingKey as PssVerifyingKey,
};
use rsa::signature::{RandomizedSigner, SignatureEncoding, Signer, Verifier};
use rsa::traits::{PrivateKeyParts, PublicKeyParts};
use rsa::{BigUint, RsaPrivateKey, RsaPublicKey};

use pkcs1::{DecodeRsaPrivateKey, DecodeRsaPublicKey};
use pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey, LineEnding};

use sha1::Sha1;
use sha2::{Sha256, Sha384, Sha512};

const OPENSSL_KEYTYPE_RSA: i64 = 0;
const OPENSSL_ALGO_SHA1: i64 = 1;
// const OPENSSL_ALGO_MD5: i64 = 2;
const OPENSSL_ALGO_SHA256: i64 = 7;
const OPENSSL_ALGO_SHA384: i64 = 8;
const OPENSSL_ALGO_SHA512: i64 = 9;

const OPENSSL_PKCS1_PADDING: i64 = 1;
const OPENSSL_PKCS1_PSS_PADDING: i64 = 6;

/// Stored key kind: either we have a private key (which contains the public)
/// or only a public key.
#[derive(Clone)]
pub enum StoredKey {
    Private(RsaPrivateKey),
    Public(RsaPublicKey),
}

impl StoredKey {
    fn public(&self) -> RsaPublicKey {
        match self {
            StoredKey::Private(p) => p.to_public_key(),
            StoredKey::Public(k) => k.clone(),
        }
    }
}

std::thread_local! {
    static PKEY_REGISTRY: RefCell<std::collections::HashMap<u64, StoredKey>> =
        RefCell::new(std::collections::HashMap::new());
    static PKEY_NEXT_ID: RefCell<u64> = const { RefCell::new(1) };
}

fn store_key(k: StoredKey) -> u64 {
    let id = PKEY_NEXT_ID.with(|n| {
        let mut n = n.borrow_mut();
        let id = *n;
        *n += 1;
        id
    });
    PKEY_REGISTRY.with(|r| {
        r.borrow_mut().insert(id, k);
    });
    id
}

fn get_key(id: u64) -> Option<StoredKey> {
    PKEY_REGISTRY.with(|r| r.borrow().get(&id).cloned())
}

/// Create an OpenSSLAsymmetricKey PHP object wrapping the given key id.
fn make_key_object(vm: &mut Vm, key: StoredKey) -> Value {
    let id = store_key(key);
    let oid = vm.next_object_id();
    let mut obj = PhpObject::new(b"OpenSSLAsymmetricKey".to_vec(), oid);
    obj.set_property(b"__pkey_id".to_vec(), Value::Long(id as i64));
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Extract the StoredKey from a PHP value if it's an OpenSSLAsymmetricKey object.
fn key_from_object(v: &Value) -> Option<StoredKey> {
    let v = match v {
        Value::Reference(r) => r.borrow().clone(),
        other => other.clone(),
    };
    if let Value::Object(o) = v {
        let o = o.borrow();
        if o.class_name.eq_ignore_ascii_case(b"OpenSSLAsymmetricKey") {
            let id_val = o.get_property(b"__pkey_id");
            let id = id_val.to_long();
            if id > 0 {
                return get_key(id as u64);
            }
        }
    }
    None
}

/// Read raw bytes from a possibly file:// or PEM string. Returns None on failure.
fn read_key_data(v: &Value) -> Option<Vec<u8>> {
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

/// Try parsing a PEM/DER blob as a private key in several formats.
fn parse_private_key(data: &[u8], passphrase: Option<&[u8]>) -> Option<RsaPrivateKey> {
    // Try PEM first (most common)
    if let Ok(s) = std::str::from_utf8(data) {
        // Try PKCS#8 first (BEGIN PRIVATE KEY)
        if let Ok(k) = RsaPrivateKey::from_pkcs8_pem(s) {
            return Some(k);
        }
        // Try PKCS#1 (BEGIN RSA PRIVATE KEY)
        if let Ok(k) = RsaPrivateKey::from_pkcs1_pem(s) {
            return Some(k);
        }
        // Try encrypted PKCS#8 with passphrase
        if let Some(pw) = passphrase {
            if let Ok(k) = RsaPrivateKey::from_pkcs8_encrypted_pem(s, pw) {
                return Some(k);
            }
        }
    }
    // Try DER blobs as well
    if let Ok(k) = RsaPrivateKey::from_pkcs8_der(data) {
        return Some(k);
    }
    if let Ok(k) = RsaPrivateKey::from_pkcs1_der(data) {
        return Some(k);
    }
    None
}

/// Try parsing a PEM/DER blob as a public key in several formats.
/// If `allow_private` is true, also tries parsing as a private key and
/// extracts the public component (used by `openssl_pkey_get_public`).
/// `openssl_verify` calls with `allow_private = false`.
fn parse_public_key(data: &[u8], allow_private: bool) -> Option<RsaPublicKey> {
    if let Ok(s) = std::str::from_utf8(data) {
        // SPKI: BEGIN PUBLIC KEY
        if let Ok(k) = RsaPublicKey::from_public_key_pem(s) {
            return Some(k);
        }
        // PKCS#1: BEGIN RSA PUBLIC KEY
        if let Ok(k) = RsaPublicKey::from_pkcs1_pem(s) {
            return Some(k);
        }
        if allow_private {
            if let Some(pk) = parse_private_key(data, None) {
                return Some(pk.to_public_key());
            }
        }
    }
    if let Ok(k) = RsaPublicKey::from_public_key_der(data) {
        return Some(k);
    }
    if let Ok(k) = RsaPublicKey::from_pkcs1_der(data) {
        return Some(k);
    }
    if allow_private {
        if let Some(pk) = parse_private_key(data, None) {
            return Some(pk.to_public_key());
        }
    }
    None
}

/// Resolve any kind of input (object, file://, PEM string) into a private key.
fn resolve_private_key(v: &Value, passphrase: Option<&[u8]>) -> Option<RsaPrivateKey> {
    // First, try as already-parsed object
    if let Some(StoredKey::Private(p)) = key_from_object(v) {
        return Some(p);
    }
    // Now try reading bytes (PEM string or file://)
    let bytes = read_key_data(v)?;
    parse_private_key(&bytes, passphrase)
}

/// Resolve any kind of input (object, file://, PEM string) into a public key.
/// `allow_private_data` controls whether a PEM/DER private-key blob can be
/// coerced into a public key (true for `openssl_pkey_get_public`, false for
/// `openssl_verify`). Already-parsed key objects always work since the public
/// part is derivable.
fn resolve_public_key(v: &Value, allow_private_data: bool) -> Option<RsaPublicKey> {
    if let Some(k) = key_from_object(v) {
        return Some(k.public());
    }
    let bytes = read_key_data(v)?;
    parse_public_key(&bytes, allow_private_data)
}

// ---- Helpers for big-endian unsigned bytes (no sign byte, no leading zeros) ----

fn biguint_to_bytes(n: &BigUint) -> Vec<u8> {
    n.to_bytes_be()
}

fn bytes_to_biguint(b: &[u8]) -> BigUint {
    BigUint::from_bytes_be(b)
}

/// Build an RSA private key from raw n/e/d/p/q components.
/// Only n, e, d, p, q are strictly needed; rsa crate computes precomputed
/// values internally.
fn build_rsa_from_components(arr: &PhpArray) -> Option<RsaPrivateKey> {
    let get = |k: &[u8]| -> Option<Vec<u8>> {
        arr.get(&ArrayKey::String(PhpString::from_bytes(k)))
            .map(|v| v.to_php_string().as_bytes().to_vec())
    };
    let n_bytes = get(b"n")?;
    let e_bytes = get(b"e")?;
    let d_bytes = get(b"d")?;
    let p_bytes = get(b"p")?;
    let q_bytes = get(b"q")?;

    let n = bytes_to_biguint(&n_bytes);
    let e = bytes_to_biguint(&e_bytes);
    let d = bytes_to_biguint(&d_bytes);
    let p = bytes_to_biguint(&p_bytes);
    let q = bytes_to_biguint(&q_bytes);

    RsaPrivateKey::from_components(n, e, d, vec![p, q]).ok()
}

// ---- PHP function implementations ----

/// openssl_pkey_new(?array $options = null) : OpenSSLAsymmetricKey|false
pub fn openssl_pkey_new(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let mut bits: usize = 2048;
    let mut explicit_rsa: Option<PhpArray> = None;

    if let Some(arg) = args.first() {
        // Dereference
        let arg = match arg {
            Value::Reference(r) => r.borrow().clone(),
            other => other.clone(),
        };
        if let Value::Array(a) = &arg {
            let arr = a.borrow();
            // Look for 'rsa' sub-array (build key from components)
            if let Some(rsa_v) = arr.get(&ArrayKey::String(PhpString::from_bytes(b"rsa"))) {
                if let Value::Array(rsa_arr) = rsa_v {
                    let rsa_arr = rsa_arr.borrow();
                    if !rsa_arr.is_empty() {
                        explicit_rsa = Some(rsa_arr.clone());
                    }
                }
            }
            // Look for private_key_bits
            if let Some(b) = arr.get(&ArrayKey::String(PhpString::from_bytes(b"private_key_bits"))) {
                let b = b.to_long();
                if b > 0 {
                    bits = b as usize;
                }
            }
            // private_key_type defaults to RSA - we only support RSA so warn for others
            if let Some(t) =
                arr.get(&ArrayKey::String(PhpString::from_bytes(b"private_key_type")))
            {
                let t = t.to_long();
                if t != OPENSSL_KEYTYPE_RSA {
                    vm.emit_warning(&format!(
                        "openssl_pkey_new(): Only RSA keys are currently supported (got type {})",
                        t
                    ));
                    return Ok(Value::False);
                }
            }
            // If 'dsa' or 'dh' sub-arrays exist, we don't support those yet
            for unsupported in [b"dsa".as_ref(), b"dh".as_ref(), b"ec".as_ref()] {
                if let Some(v) = arr.get(&ArrayKey::String(PhpString::from_bytes(unsupported))) {
                    if let Value::Array(_) = v {
                        vm.emit_warning(&format!(
                            "openssl_pkey_new(): {} key generation not supported",
                            std::str::from_utf8(unsupported).unwrap_or("unknown")
                        ));
                        return Ok(Value::False);
                    }
                }
            }
        }
    }

    if let Some(arr) = explicit_rsa {
        match build_rsa_from_components(&arr) {
            Some(k) => return Ok(make_key_object(vm, StoredKey::Private(k))),
            None => {
                vm.emit_warning("openssl_pkey_new(): unable to build RSA key from components");
                return Ok(Value::False);
            }
        }
    }

    // Sanity: minimum bits
    if bits < 512 {
        vm.emit_warning("openssl_pkey_new(): private_key_bits must be at least 512");
        return Ok(Value::False);
    }
    if bits > 16384 {
        vm.emit_warning("openssl_pkey_new(): private_key_bits is too large");
        return Ok(Value::False);
    }

    // Generate key. Use thread_rng().
    let mut rng = rand::thread_rng();
    match RsaPrivateKey::new(&mut rng, bits) {
        Ok(k) => Ok(make_key_object(vm, StoredKey::Private(k))),
        Err(e) => {
            vm.emit_warning(&format!("openssl_pkey_new(): key generation failed: {}", e));
            Ok(Value::False)
        }
    }
}

/// openssl_pkey_export(key, &output, ?passphrase = null, ?config = null): bool
pub fn openssl_pkey_export(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let key_arg = args.first().cloned().unwrap_or(Value::Null);
    let passphrase = args.get(2).map(|v| {
        let v = match v {
            Value::Reference(r) => r.borrow().clone(),
            other => other.clone(),
        };
        v.to_php_string().as_bytes().to_vec()
    });
    let pw: Option<&[u8]> = passphrase.as_deref().filter(|b| !b.is_empty());

    let priv_key = match resolve_private_key(&key_arg, pw) {
        Some(k) => k,
        None => {
            vm.emit_warning(
                "openssl_pkey_export(): Cannot get key from parameter 1",
            );
            return Ok(Value::False);
        }
    };

    // Encode as PKCS#8 PEM
    let pem_doc = if let Some(pwb) = pw {
        // Encrypted PKCS#8
        let mut rng = rand::thread_rng();
        match priv_key.to_pkcs8_encrypted_pem(&mut rng, pwb, LineEnding::LF) {
            Ok(s) => s.to_string(),
            Err(e) => {
                vm.emit_warning(&format!("openssl_pkey_export(): encryption failed: {}", e));
                return Ok(Value::False);
            }
        }
    } else {
        match priv_key.to_pkcs8_pem(LineEnding::LF) {
            Ok(s) => s.to_string(),
            Err(e) => {
                vm.emit_warning(&format!("openssl_pkey_export(): encoding failed: {}", e));
                return Ok(Value::False);
            }
        }
    };

    // Write to &$output by reference
    if let Some(Value::Reference(r)) = args.get(1) {
        *r.borrow_mut() = Value::String(PhpString::from_string(pem_doc));
    }

    Ok(Value::True)
}

/// openssl_pkey_export_to_file(key, filename, ?passphrase = null, ?config = null): bool
pub fn openssl_pkey_export_to_file(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let key_arg = args.first().cloned().unwrap_or(Value::Null);
    let filename = args.get(1).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b""));
    let passphrase = args.get(2).map(|v| {
        let v = match v {
            Value::Reference(r) => r.borrow().clone(),
            other => other.clone(),
        };
        v.to_php_string().as_bytes().to_vec()
    });
    let pw: Option<&[u8]> = passphrase.as_deref().filter(|b| !b.is_empty());

    let priv_key = match resolve_private_key(&key_arg, pw) {
        Some(k) => k,
        None => {
            vm.emit_warning("openssl_pkey_export_to_file(): Cannot get key from parameter 1");
            return Ok(Value::False);
        }
    };

    let pem_doc = if let Some(pwb) = pw {
        let mut rng = rand::thread_rng();
        match priv_key.to_pkcs8_encrypted_pem(&mut rng, pwb, LineEnding::LF) {
            Ok(s) => s.to_string(),
            Err(e) => {
                vm.emit_warning(&format!("openssl_pkey_export_to_file(): {}", e));
                return Ok(Value::False);
            }
        }
    } else {
        match priv_key.to_pkcs8_pem(LineEnding::LF) {
            Ok(s) => s.to_string(),
            Err(e) => {
                vm.emit_warning(&format!("openssl_pkey_export_to_file(): {}", e));
                return Ok(Value::False);
            }
        }
    };

    let path = filename.to_string_lossy().to_string();
    match std::fs::write(&path, pem_doc) {
        Ok(()) => Ok(Value::True),
        Err(e) => {
            vm.emit_warning(&format!("openssl_pkey_export_to_file(): {}", e));
            Ok(Value::False)
        }
    }
}

/// openssl_pkey_get_private(string|array|OpenSSLAsymmetricKey $privateKey, ?string $passphrase = null)
pub fn openssl_pkey_get_private(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let raw = args.first().cloned().unwrap_or(Value::Null);
    let passphrase_arg = args.get(1).map(|v| v.to_php_string().as_bytes().to_vec());

    // Support array syntax: [file_or_pem, passphrase]
    let (key_arg, pw_vec): (Value, Option<Vec<u8>>) = {
        let raw_deref = match &raw {
            Value::Reference(r) => r.borrow().clone(),
            other => other.clone(),
        };
        if let Value::Array(a) = &raw_deref {
            let arr = a.borrow();
            let first = arr.get_int(0).cloned().unwrap_or(Value::Null);
            let second = arr.get_int(1).cloned();
            let pw = second.map(|v| v.to_php_string().as_bytes().to_vec());
            (first, pw.or(passphrase_arg))
        } else {
            (raw, passphrase_arg)
        }
    };

    let pw: Option<&[u8]> = pw_vec.as_deref().filter(|b| !b.is_empty());

    // If already an OpenSSLAsymmetricKey containing a private key, return as-is
    if let Some(StoredKey::Private(p)) = key_from_object(&key_arg) {
        return Ok(make_key_object(vm, StoredKey::Private(p)));
    }
    // Otherwise parse from string or file
    let bytes = match read_key_data(&key_arg) {
        Some(b) => b,
        None => {
            vm.emit_warning("openssl_pkey_get_private(): Cannot read key data");
            return Ok(Value::False);
        }
    };
    match parse_private_key(&bytes, pw) {
        Some(k) => Ok(make_key_object(vm, StoredKey::Private(k))),
        None => {
            vm.emit_warning("openssl_pkey_get_private(): Cannot parse private key");
            Ok(Value::False)
        }
    }
}

/// openssl_pkey_get_public(string|OpenSSLAsymmetricKey $publicKey)
pub fn openssl_pkey_get_public(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let key_arg = args.first().cloned().unwrap_or(Value::Null);
    // Pass-through for existing key object
    if let Some(k) = key_from_object(&key_arg) {
        return Ok(make_key_object(vm, StoredKey::Public(k.public())));
    }
    let bytes = match read_key_data(&key_arg) {
        Some(b) => b,
        None => {
            vm.emit_warning("openssl_pkey_get_public(): Cannot read key data");
            return Ok(Value::False);
        }
    };
    match parse_public_key(&bytes, /* allow_private */ true) {
        Some(k) => Ok(make_key_object(vm, StoredKey::Public(k))),
        None => {
            vm.emit_warning("openssl_pkey_get_public(): Cannot parse public key");
            Ok(Value::False)
        }
    }
}

/// openssl_pkey_get_details(OpenSSLAsymmetricKey $key): array|false
pub fn openssl_pkey_get_details(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let key_arg = args.first().cloned().unwrap_or(Value::Null);
    let stored = match key_from_object(&key_arg) {
        Some(k) => k,
        None => {
            vm.emit_warning(
                "openssl_pkey_get_details(): supplied key is not a valid OpenSSLAsymmetricKey",
            );
            return Ok(Value::False);
        }
    };
    let pubkey = stored.public();
    let n = pubkey.n();
    let e = pubkey.e();

    let mut result = PhpArray::new();
    // bits = bit length of modulus
    let bits = n.bits() as i64;
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"bits")),
        Value::Long(bits),
    );

    // key: PEM-encoded SubjectPublicKeyInfo (BEGIN PUBLIC KEY)
    let pub_pem = pubkey
        .to_public_key_pem(LineEnding::LF)
        .unwrap_or_default();
    result.set(
        ArrayKey::String(PhpString::from_bytes(b"key")),
        Value::String(PhpString::from_string(pub_pem)),
    );

    // rsa sub-array
    let mut rsa_sub = PhpArray::new();
    rsa_sub.set(
        ArrayKey::String(PhpString::from_bytes(b"n")),
        Value::String(PhpString::from_vec(biguint_to_bytes(n))),
    );
    rsa_sub.set(
        ArrayKey::String(PhpString::from_bytes(b"e")),
        Value::String(PhpString::from_vec(biguint_to_bytes(e))),
    );
    if let StoredKey::Private(p) = &stored {
        let d = p.d();
        let primes = p.primes();
        rsa_sub.set(
            ArrayKey::String(PhpString::from_bytes(b"d")),
            Value::String(PhpString::from_vec(biguint_to_bytes(d))),
        );
        if primes.len() >= 2 {
            rsa_sub.set(
                ArrayKey::String(PhpString::from_bytes(b"p")),
                Value::String(PhpString::from_vec(biguint_to_bytes(&primes[0]))),
            );
            rsa_sub.set(
                ArrayKey::String(PhpString::from_bytes(b"q")),
                Value::String(PhpString::from_vec(biguint_to_bytes(&primes[1]))),
            );
        }
        // dmp1 = d mod (p-1), dmq1 = d mod (q-1), iqmp = q^-1 mod p
        if let (Some(dp), Some(dq), Some(qinv)) = (p.dp(), p.dq(), p.qinv()) {
            rsa_sub.set(
                ArrayKey::String(PhpString::from_bytes(b"dmp1")),
                Value::String(PhpString::from_vec(biguint_to_bytes(dp))),
            );
            rsa_sub.set(
                ArrayKey::String(PhpString::from_bytes(b"dmq1")),
                Value::String(PhpString::from_vec(biguint_to_bytes(dq))),
            );
            // qinv is a BigInt; convert to its absolute big-endian bytes
            let qinv_bytes = qinv.to_bytes_be().1;
            rsa_sub.set(
                ArrayKey::String(PhpString::from_bytes(b"iqmp")),
                Value::String(PhpString::from_vec(qinv_bytes)),
            );
        }
    }

    result.set(
        ArrayKey::String(PhpString::from_bytes(b"rsa")),
        Value::Array(Rc::new(RefCell::new(rsa_sub))),
    );

    result.set(
        ArrayKey::String(PhpString::from_bytes(b"type")),
        Value::Long(OPENSSL_KEYTYPE_RSA),
    );

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

/// openssl_pkey_free(key): void
pub fn openssl_pkey_free(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // PHP 8.0+ deprecation notice
    vm.emit_deprecated(
        "Function openssl_pkey_free() is deprecated since 8.0, as OpenSSLAsymmetricKey objects are freed automatically",
    );
    if let Some(arg) = args.first() {
        let arg = match arg {
            Value::Reference(r) => r.borrow().clone(),
            other => other.clone(),
        };
        if let Value::Object(o) = arg {
            let id = o.borrow().get_property(b"__pkey_id").to_long();
            if id > 0 {
                PKEY_REGISTRY.with(|r| {
                    r.borrow_mut().remove(&(id as u64));
                });
            }
        }
    }
    Ok(Value::Null)
}

/// openssl_sign(data, &signature, key, algo = OPENSSL_ALGO_SHA1, padding = OPENSSL_PKCS1_PADDING): bool
pub fn openssl_sign(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let key_arg = args.get(2).cloned().unwrap_or(Value::Null);
    let algo = args.get(3).map(|v| v.to_long()).unwrap_or(OPENSSL_ALGO_SHA1);
    let padding = args.get(4).map(|v| v.to_long()).unwrap_or(OPENSSL_PKCS1_PADDING);

    let priv_key = match resolve_private_key(&key_arg, None) {
        Some(k) => k,
        None => {
            vm.emit_warning(
                "openssl_sign(): Supplied key param cannot be coerced into a private key",
            );
            return Ok(Value::False);
        }
    };

    let signature_bytes: Vec<u8> = match padding {
        OPENSSL_PKCS1_PSS_PADDING => {
            let mut rng = rand::thread_rng();
            match algo {
                OPENSSL_ALGO_SHA1 => {
                    let signing = PssBlindedSigningKey::<Sha1>::new(priv_key);
                    let sig: PssSig = signing.sign_with_rng(&mut rng, data.as_bytes());
                    sig.to_bytes().to_vec()
                }
                OPENSSL_ALGO_SHA256 => {
                    let signing = PssBlindedSigningKey::<Sha256>::new(priv_key);
                    let sig: PssSig = signing.sign_with_rng(&mut rng, data.as_bytes());
                    sig.to_bytes().to_vec()
                }
                OPENSSL_ALGO_SHA384 => {
                    let signing = PssBlindedSigningKey::<Sha384>::new(priv_key);
                    let sig: PssSig = signing.sign_with_rng(&mut rng, data.as_bytes());
                    sig.to_bytes().to_vec()
                }
                OPENSSL_ALGO_SHA512 => {
                    let signing = PssBlindedSigningKey::<Sha512>::new(priv_key);
                    let sig: PssSig = signing.sign_with_rng(&mut rng, data.as_bytes());
                    sig.to_bytes().to_vec()
                }
                _ => {
                    vm.emit_warning(&format!(
                        "openssl_sign(): Unknown signature algorithm (id={})",
                        algo
                    ));
                    return Ok(Value::False);
                }
            }
        }
        OPENSSL_PKCS1_PADDING | _ => match algo {
            OPENSSL_ALGO_SHA1 => {
                let signing = SigningKey::<Sha1>::new(priv_key);
                let sig: PkcsSig = signing.sign(data.as_bytes());
                sig.to_bytes().to_vec()
            }
            OPENSSL_ALGO_SHA256 => {
                let signing = SigningKey::<Sha256>::new(priv_key);
                let sig: PkcsSig = signing.sign(data.as_bytes());
                sig.to_bytes().to_vec()
            }
            OPENSSL_ALGO_SHA384 => {
                let signing = SigningKey::<Sha384>::new(priv_key);
                let sig: PkcsSig = signing.sign(data.as_bytes());
                sig.to_bytes().to_vec()
            }
            OPENSSL_ALGO_SHA512 => {
                let signing = SigningKey::<Sha512>::new(priv_key);
                let sig: PkcsSig = signing.sign(data.as_bytes());
                sig.to_bytes().to_vec()
            }
            _ => {
                vm.emit_warning(&format!(
                    "openssl_sign(): Unknown signature algorithm (id={})",
                    algo
                ));
                return Ok(Value::False);
            }
        },
    };

    if let Some(Value::Reference(r)) = args.get(1) {
        *r.borrow_mut() = Value::String(PhpString::from_vec(signature_bytes));
    }

    Ok(Value::True)
}

/// openssl_verify(data, signature, key, algo = OPENSSL_ALGO_SHA1, padding = OPENSSL_PKCS1_PADDING): int|false
pub fn openssl_verify(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let signature = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let key_arg = args.get(2).cloned().unwrap_or(Value::Null);
    let algo = args.get(3).map(|v| v.to_long()).unwrap_or(OPENSSL_ALGO_SHA1);
    let padding = args.get(4).map(|v| v.to_long()).unwrap_or(OPENSSL_PKCS1_PADDING);

    let pub_key = match resolve_public_key(&key_arg, /* allow_private_data */ false) {
        Some(k) => k,
        None => {
            vm.emit_warning(
                "openssl_verify(): Supplied key param cannot be coerced into a public key",
            );
            return Ok(Value::False);
        }
    };

    let verified = if padding == OPENSSL_PKCS1_PSS_PADDING {
        let sig = match PssSig::try_from(signature.as_bytes()) {
            Ok(s) => s,
            Err(_) => return Ok(Value::Long(0)),
        };
        match algo {
            OPENSSL_ALGO_SHA1 => {
                let v: PssVerifyingKey<Sha1> = PssVerifyingKey::new(pub_key);
                v.verify(data.as_bytes(), &sig).is_ok()
            }
            OPENSSL_ALGO_SHA256 => {
                let v: PssVerifyingKey<Sha256> = PssVerifyingKey::new(pub_key);
                v.verify(data.as_bytes(), &sig).is_ok()
            }
            OPENSSL_ALGO_SHA384 => {
                let v: PssVerifyingKey<Sha384> = PssVerifyingKey::new(pub_key);
                v.verify(data.as_bytes(), &sig).is_ok()
            }
            OPENSSL_ALGO_SHA512 => {
                let v: PssVerifyingKey<Sha512> = PssVerifyingKey::new(pub_key);
                v.verify(data.as_bytes(), &sig).is_ok()
            }
            _ => {
                vm.emit_warning(&format!(
                    "openssl_verify(): Unknown signature algorithm (id={})",
                    algo
                ));
                return Ok(Value::False);
            }
        }
    } else {
        let sig = match PkcsSig::try_from(signature.as_bytes()) {
            Ok(s) => s,
            Err(_) => return Ok(Value::Long(0)),
        };
        match algo {
            OPENSSL_ALGO_SHA1 => {
                let v: VerifyingKey<Sha1> = VerifyingKey::new(pub_key);
                v.verify(data.as_bytes(), &sig).is_ok()
            }
            OPENSSL_ALGO_SHA256 => {
                let v: VerifyingKey<Sha256> = VerifyingKey::new(pub_key);
                v.verify(data.as_bytes(), &sig).is_ok()
            }
            OPENSSL_ALGO_SHA384 => {
                let v: VerifyingKey<Sha384> = VerifyingKey::new(pub_key);
                v.verify(data.as_bytes(), &sig).is_ok()
            }
            OPENSSL_ALGO_SHA512 => {
                let v: VerifyingKey<Sha512> = VerifyingKey::new(pub_key);
                v.verify(data.as_bytes(), &sig).is_ok()
            }
            _ => {
                vm.emit_warning(&format!(
                    "openssl_verify(): Unknown signature algorithm (id={})",
                    algo
                ));
                return Ok(Value::False);
            }
        }
    };

    Ok(Value::Long(if verified { 1 } else { 0 }))
}

/// Register the OpenSSLAsymmetricKey class.
pub fn register_class(vm: &mut Vm) {
    use goro_core::object::ClassEntry;
    let class = ClassEntry::new(b"OpenSSLAsymmetricKey".to_vec());
    vm.register_class(class);
    // Also register OpenSSLCertificate so instanceof checks pass when tests reference it
    let cert_class = ClassEntry::new(b"OpenSSLCertificate".to_vec());
    vm.register_class(cert_class);
    // OpenSSLCertificateSigningRequest used by some tests
    let csr_class = ClassEntry::new(b"OpenSSLCertificateSigningRequest".to_vec());
    vm.register_class(csr_class);
}
