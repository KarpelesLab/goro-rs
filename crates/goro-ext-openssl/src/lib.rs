use goro_core::array::PhpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{BuiltinFn, Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

mod asn1;
mod bn;
mod cert;
mod cipher;
mod conf;
mod digest;
mod ec;
mod ec_curves;
mod ecx;
mod err;
mod name;
mod obj;
mod objects;
mod params;
mod pkcs12;
mod pkey;
mod pkey_fns;
mod req;
mod smime;
mod x509;
mod x509v3;
mod xp_ssl;
mod zpp;

// Option flags
const OPENSSL_RAW_DATA: i64 = 1;
const OPENSSL_ZERO_PADDING: i64 = 2;

/// Wrap a builtin with the stub's argument count check.
macro_rules! checked {
    ($name:literal, $f:path) => {{
        fn w(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
            zpp::enter($name, &vm.builtin_named_skipped);
            params::check_argc(vm, args, $name)?;
            $f(vm, args)
        }
        ($name, w as BuiltinFn)
    }};
}

/// Register all openssl extension functions and constants
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"openssl");
    xp_ssl::install(vm);
    pkey_fns::register_class(vm);
    let fns: &[(&str, BuiltinFn)] = &[
        checked!("openssl_encrypt", openssl_encrypt),
        checked!("openssl_decrypt", openssl_decrypt),
        checked!("openssl_random_pseudo_bytes", openssl_random_pseudo_bytes),
        checked!("openssl_digest", openssl_digest),
        checked!("openssl_get_cipher_methods", openssl_get_cipher_methods),
        checked!("openssl_cipher_iv_length", openssl_cipher_iv_length),
        checked!("openssl_cipher_key_length", openssl_cipher_key_length),
        checked!("openssl_get_md_methods", openssl_get_md_methods),
        checked!("openssl_error_string", openssl_error_string),
        checked!("openssl_get_curve_names", pkey_fns::openssl_get_curve_names),
        checked!("openssl_pbkdf2", pkey_fns::openssl_pbkdf2),
        // X.509
        checked!("openssl_x509_read", x509::openssl_x509_read),
        checked!("openssl_x509_parse", x509::openssl_x509_parse),
        checked!("openssl_x509_export", x509::openssl_x509_export),
        checked!(
            "openssl_x509_export_to_file",
            x509::openssl_x509_export_to_file
        ),
        checked!("openssl_x509_free", x509::openssl_x509_free),
        checked!("openssl_x509_checkpurpose", x509::openssl_x509_checkpurpose),
        checked!(
            "openssl_x509_check_private_key",
            x509::openssl_x509_check_private_key
        ),
        checked!("openssl_x509_verify", x509::openssl_x509_verify),
        checked!("openssl_x509_fingerprint", x509::openssl_x509_fingerprint),
        // Keys
        checked!("openssl_pkey_new", pkey_fns::openssl_pkey_new),
        checked!("openssl_pkey_get_public", pkey_fns::openssl_pkey_get_public),
        checked!("openssl_get_publickey", pkey_fns::openssl_pkey_get_public),
        checked!(
            "openssl_pkey_get_private",
            pkey_fns::openssl_pkey_get_private
        ),
        checked!("openssl_get_privatekey", pkey_fns::openssl_pkey_get_private),
        checked!(
            "openssl_pkey_get_details",
            pkey_fns::openssl_pkey_get_details
        ),
        checked!("openssl_pkey_export", pkey_fns::openssl_pkey_export),
        checked!(
            "openssl_pkey_export_to_file",
            pkey_fns::openssl_pkey_export_to_file
        ),
        checked!("openssl_pkey_free", pkey_fns::openssl_pkey_free),
        checked!("openssl_free_key", pkey_fns::openssl_free_key),
        checked!("openssl_public_encrypt", pkey_fns::openssl_public_encrypt),
        checked!("openssl_private_encrypt", pkey_fns::openssl_private_encrypt),
        checked!("openssl_public_decrypt", pkey_fns::openssl_public_decrypt),
        checked!("openssl_private_decrypt", pkey_fns::openssl_private_decrypt),
        checked!("openssl_sign", pkey_fns::openssl_sign),
        checked!("openssl_verify", pkey_fns::openssl_verify),
        checked!("openssl_seal", pkey_fns::openssl_seal),
        checked!("openssl_open", pkey_fns::openssl_open),
        checked!("openssl_dh_compute_key", pkey_fns::openssl_dh_compute_key),
        checked!("openssl_pkey_derive", pkey_fns::openssl_pkey_derive),
        // CSR
        checked!("openssl_csr_new", x509::openssl_csr_new),
        checked!("openssl_csr_export", x509::openssl_csr_export),
        checked!(
            "openssl_csr_export_to_file",
            x509::openssl_csr_export_to_file
        ),
        checked!("openssl_csr_sign", x509::openssl_csr_sign),
        checked!("openssl_csr_get_subject", x509::openssl_csr_get_subject),
        checked!(
            "openssl_csr_get_public_key",
            x509::openssl_csr_get_public_key
        ),
        // PKCS#7 / PKCS#12 / CMS
        checked!("openssl_pkcs7_sign", smime::openssl_pkcs7_sign),
        checked!("openssl_pkcs7_verify", smime::openssl_pkcs7_verify),
        checked!("openssl_pkcs7_encrypt", smime::openssl_pkcs7_encrypt),
        checked!("openssl_pkcs7_decrypt", smime::openssl_pkcs7_decrypt),
        checked!("openssl_pkcs7_read", smime::openssl_pkcs7_read),
        checked!("openssl_pkcs12_export", pkcs12::openssl_pkcs12_export),
        checked!(
            "openssl_pkcs12_export_to_file",
            pkcs12::openssl_pkcs12_export_to_file
        ),
        checked!("openssl_pkcs12_read", pkcs12::openssl_pkcs12_read),
        checked!("openssl_cms_sign", smime::openssl_cms_sign),
        checked!("openssl_cms_verify", smime::openssl_cms_verify),
        checked!("openssl_cms_encrypt", smime::openssl_cms_encrypt),
        checked!("openssl_cms_decrypt", smime::openssl_cms_decrypt),
        checked!("openssl_cms_read", smime::openssl_cms_read),
        // SPKI
        checked!("openssl_spki_new", x509::openssl_spki_new),
        checked!("openssl_spki_verify", x509::openssl_spki_verify),
        checked!("openssl_spki_export", x509::openssl_spki_export),
        checked!(
            "openssl_spki_export_challenge",
            x509::openssl_spki_export_challenge
        ),
        checked!(
            "openssl_get_cert_locations",
            openssl_get_cert_locations_stub
        ),
    ];
    for (name, f) in fns {
        vm.register_function(name.as_bytes(), *f);
    }

    // Constants (values from OpenSSL 3.0 / php-src)
    let longs: &[(&str, i64)] = &[
        ("OPENSSL_VERSION_NUMBER", 0x30000000),
        ("X509_PURPOSE_SSL_CLIENT", 1),
        ("X509_PURPOSE_SSL_SERVER", 2),
        ("X509_PURPOSE_NS_SSL_SERVER", 3),
        ("X509_PURPOSE_SMIME_SIGN", 4),
        ("X509_PURPOSE_SMIME_ENCRYPT", 5),
        ("X509_PURPOSE_CRL_SIGN", 6),
        ("X509_PURPOSE_ANY", 7),
        ("X509_PURPOSE_OCSP_HELPER", 8),
        ("X509_PURPOSE_TIMESTAMP_SIGN", 9),
        ("OPENSSL_ALGO_SHA1", 1),
        ("OPENSSL_ALGO_MD5", 2),
        ("OPENSSL_ALGO_MD4", 3),
        ("OPENSSL_ALGO_SHA224", 6),
        ("OPENSSL_ALGO_SHA256", 7),
        ("OPENSSL_ALGO_SHA384", 8),
        ("OPENSSL_ALGO_SHA512", 9),
        ("OPENSSL_ALGO_RMD160", 10),
        ("PKCS7_DETACHED", 0x40),
        ("PKCS7_TEXT", 0x1),
        ("PKCS7_NOINTERN", 0x10),
        ("PKCS7_NOVERIFY", 0x20),
        ("PKCS7_NOCHAIN", 0x8),
        ("PKCS7_NOCERTS", 0x2),
        ("PKCS7_NOATTR", 0x100),
        ("PKCS7_BINARY", 0x80),
        ("PKCS7_NOSIGS", 0x4),
        ("PKCS7_NOOLDMIMETYPE", 0x400),
        ("PKCS7_NOSMIMECAP", 0x200),
        ("PKCS7_CRLFEOL", 0x800),
        ("PKCS7_NOCRL", 0x2000),
        ("PKCS7_NO_DUAL_CONTENT", 0x10000),
        ("OPENSSL_CMS_DETACHED", 0x40),
        ("OPENSSL_CMS_TEXT", 0x1),
        ("OPENSSL_CMS_NOINTERN", 0x10),
        ("OPENSSL_CMS_NOVERIFY", 0x20),
        ("OPENSSL_CMS_NOCERTS", 0x2),
        ("OPENSSL_CMS_NOATTR", 0x100),
        ("OPENSSL_CMS_BINARY", 0x80),
        ("OPENSSL_CMS_NOSIGS", 0xc),
        ("OPENSSL_CMS_OLDMIMETYPE", 0x400),
        ("OPENSSL_PKCS1_PADDING", 1),
        ("OPENSSL_NO_PADDING", 3),
        ("OPENSSL_PKCS1_OAEP_PADDING", 4),
        ("OPENSSL_PKCS1_PSS_PADDING", 6),
        ("OPENSSL_CIPHER_RC2_40", 0),
        ("OPENSSL_CIPHER_RC2_128", 1),
        ("OPENSSL_CIPHER_RC2_64", 2),
        ("OPENSSL_CIPHER_DES", 3),
        ("OPENSSL_CIPHER_3DES", 4),
        ("OPENSSL_CIPHER_AES_128_CBC", 5),
        ("OPENSSL_CIPHER_AES_192_CBC", 6),
        ("OPENSSL_CIPHER_AES_256_CBC", 7),
        ("OPENSSL_KEYTYPE_RSA", 0),
        ("OPENSSL_KEYTYPE_DSA", 1),
        ("OPENSSL_KEYTYPE_DH", 2),
        ("OPENSSL_KEYTYPE_EC", 3),
        ("OPENSSL_KEYTYPE_X25519", 4),
        ("OPENSSL_KEYTYPE_ED25519", 5),
        ("OPENSSL_KEYTYPE_X448", 6),
        ("OPENSSL_KEYTYPE_ED448", 7),
        ("OPENSSL_RAW_DATA", 1),
        ("OPENSSL_ZERO_PADDING", 2),
        ("OPENSSL_DONT_ZERO_PAD_KEY", 4),
        ("OPENSSL_TLSEXT_SERVER_NAME", 1),
        ("OPENSSL_ENCODING_DER", 0),
        ("OPENSSL_ENCODING_SMIME", 1),
        ("OPENSSL_ENCODING_PEM", 2),
    ];
    for (name, v) in longs {
        vm.constants
            .insert(name.as_bytes().to_vec(), Value::Long(*v));
    }
    vm.constants.insert(
        b"OPENSSL_VERSION_TEXT".to_vec(),
        Value::String(PhpString::from_bytes(b"OpenSSL 3.0.0 7 sep 2021")),
    );
    vm.constants.insert(
        b"OPENSSL_DEFAULT_STREAM_CIPHERS".to_vec(),
        Value::String(PhpString::from_bytes(
            b"ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:\
ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES256-GCM-SHA384:DHE-RSA-AES128-GCM-SHA256:\
DHE-DSS-AES128-GCM-SHA256:kEDH+AESGCM:ECDHE-RSA-AES128-SHA256:ECDHE-ECDSA-AES128-SHA256:\
ECDHE-RSA-AES128-SHA:ECDHE-ECDSA-AES128-SHA:ECDHE-RSA-AES256-SHA384:ECDHE-ECDSA-AES256-SHA384:\
ECDHE-RSA-AES256-SHA:ECDHE-ECDSA-AES256-SHA:DHE-RSA-AES128-SHA256:DHE-RSA-AES128-SHA:\
DHE-DSS-AES128-SHA256:DHE-RSA-AES256-SHA256:DHE-DSS-AES256-SHA:DHE-RSA-AES256-SHA:AES128-GCM-SHA256:\
AES256-GCM-SHA384:AES128:AES256:HIGH:!SSLv2:!aNULL:!eNULL:!EXPORT:!DES:!MD5:!RC4:!ADH",
        )),
    );
    params::apply(vm);
}

// ---- Base64 helpers (php_base64_encode / php_base64_decode) ----

const B64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub(crate) fn base64_encode(data: &[u8]) -> String {
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(B64_CHARS[(n >> 18) as usize & 63] as char);
        result.push(B64_CHARS[(n >> 12) as usize & 63] as char);
        result.push(if chunk.len() > 1 {
            B64_CHARS[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        result.push(if chunk.len() > 2 {
            B64_CHARS[n as usize & 63] as char
        } else {
            '='
        });
    }
    result
}

fn base64_rev(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Non-strict php_base64_decode(): unknown characters are skipped, never fails.
pub(crate) fn base64_decode_lenient(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 4 * 3);
    let mut i = 0usize;
    let mut cur = 0u8;
    for &c in data {
        if c == b'=' {
            continue;
        }
        let Some(ch) = base64_rev(c) else { continue };
        match i % 4 {
            0 => cur = ch << 2,
            1 => {
                out.push(cur | (ch >> 4));
                cur = (ch & 0x0f) << 4;
            }
            2 => {
                out.push(cur | (ch >> 2));
                cur = (ch & 0x03) << 6;
            }
            _ => out.push(cur | ch),
        }
        i += 1;
    }
    out
}

/// Strict-ish decode used for PEM bodies (whitespace skipped, bad chars fail).
pub(crate) fn base64_decode(data: &str) -> Option<Vec<u8>> {
    let mut clean = Vec::with_capacity(data.len());
    for &c in data.as_bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        base64_rev(c)?;
        clean.push(c);
    }
    if clean.len() % 4 == 1 {
        return None;
    }
    Some(base64_decode_lenient(&clean))
}

// ---- PHP function implementations ----

const OPENSSL_DONT_ZERO_PAD_KEY: i64 = 4;

/// Valid AEAD IV lengths (EVP_CTRL_AEAD_SET_IVLEN).
fn aead_ivlen_ok(spec: &cipher::CipherSpec, ivlen: usize) -> bool {
    use cipher::{Algo, Mode};
    if spec.algo == Algo::ChaCha20Poly1305 {
        return (1..=12).contains(&ivlen);
    }
    match spec.mode {
        Mode::Gcm => (1..=128).contains(&ivlen),
        Mode::Ccm => (7..=13).contains(&ivlen),
        Mode::Ocb => (1..=15).contains(&ivlen),
        _ => true,
    }
}

/// Shared implementation of php_openssl_encrypt()/php_openssl_decrypt().
#[allow(clippy::too_many_arguments)]
fn php_openssl_cipher(
    vm: &mut Vm,
    enc: bool,
    data: &[u8],
    method: &str,
    password: &[u8],
    options: i64,
    iv: &[u8],
    tag_wanted: bool,
    tag_in: Option<&[u8]>,
    tag_len: i64,
    aad: &[u8],
) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    use cipher::Mode;
    let fname = if enc {
        "openssl_encrypt"
    } else {
        "openssl_decrypt"
    };
    let Some(spec) = cipher::by_name(method) else {
        vm.emit_warning(&format!("{}(): Unknown cipher algorithm", fname));
        return None;
    };
    let is_aead = spec.is_aead();
    let is_ccm = spec.mode == Mode::Ccm;
    let is_ocb = spec.mode == Mode::Ocb;

    let decoded;
    let data: &[u8] = if !enc && options & OPENSSL_RAW_DATA == 0 {
        decoded = base64_decode_lenient(data);
        &decoded
    } else {
        data
    };

    // php_openssl_cipher_init()
    let max_iv_len = spec.iv_len;
    if enc && iv.is_empty() && max_iv_len > 0 && !is_aead {
        vm.emit_warning(&format!(
            "{}(): Using an empty Initialization Vector (iv) is potentially insecure and not recommended",
            fname
        ));
    }
    // php_openssl_validate_iv()
    let iv_vec: Vec<u8> = if is_aead {
        if !aead_ivlen_ok(spec, iv.len()) {
            vm.emit_warning(&format!(
                "{}(): Setting of IV length for AEAD mode failed",
                fname
            ));
            return None;
        }
        iv.to_vec()
    } else if iv.len() == max_iv_len {
        iv.to_vec()
    } else if iv.is_empty() {
        vec![0u8; max_iv_len]
    } else if iv.len() < max_iv_len {
        vm.emit_warning(&format!(
            "{}(): IV passed is only {} bytes long, cipher expects an IV of precisely {} bytes, padding with \\0",
            fname,
            iv.len(),
            max_iv_len
        ));
        let mut v = iv.to_vec();
        v.resize(max_iv_len, 0);
        v
    } else {
        vm.emit_warning(&format!(
            "{}(): IV passed is {} bytes long which is longer than the {} expected by selected cipher, truncating",
            fname,
            iv.len(),
            max_iv_len
        ));
        iv[..max_iv_len].to_vec()
    };
    // Tag length: always for OCB, when encrypting for CCM.
    let mut eff_tag_len: usize = if is_ccm { 12 } else { 16 };
    if is_ocb || (enc && is_ccm) {
        let ok = if is_ocb {
            (0..=16).contains(&tag_len)
        } else {
            (4..=16).contains(&tag_len) && tag_len % 2 == 0
        };
        if !ok {
            vm.emit_warning(&format!(
                "{}(): Setting tag length for AEAD cipher failed",
                fname
            ));
            return None;
        }
        eff_tag_len = tag_len as usize;
    }
    let mut tag_set: Option<Vec<u8>> = None;
    if !enc {
        if let Some(t) = tag_in.filter(|t| !t.is_empty()) {
            if !is_aead {
                vm.emit_warning(&format!(
                    "{}(): The tag cannot be used because the cipher algorithm does not support AEAD",
                    fname
                ));
            } else {
                let ok = if is_ccm {
                    t.len() % 2 == 0 && (4..=16).contains(&t.len())
                } else if is_ocb {
                    t.len() == eff_tag_len
                } else {
                    t.len() <= 16
                };
                if !ok {
                    vm.emit_warning(&format!(
                        "{}(): Setting tag for AEAD cipher decryption failed",
                        fname
                    ));
                    return None;
                }
                if is_ccm {
                    eff_tag_len = t.len();
                }
                tag_set = Some(t.to_vec());
            }
        }
    }
    // Check and set the key.
    let key_len = spec.key_len;
    let dont_pad = options & OPENSSL_DONT_ZERO_PAD_KEY != 0;
    let key: Vec<u8> = if key_len > password.len() {
        if dont_pad && !spec.var_key {
            err::evp_invalid_key_length();
            vm.emit_warning(&format!(
                "{}(): Key length cannot be set for the cipher algorithm",
                fname
            ));
            return None;
        }
        if dont_pad {
            password.to_vec()
        } else {
            let mut k = password.to_vec();
            k.resize(key_len, 0);
            k
        }
    } else if password.len() > key_len {
        if spec.var_key {
            password.to_vec()
        } else {
            err::evp_invalid_key_length();
            password[..key_len].to_vec()
        }
    } else {
        password.to_vec()
    };
    let padding = options & OPENSSL_ZERO_PADDING == 0;

    // AEAD decryption without a tag cannot verify.
    if !enc && is_aead && tag_set.is_none() {
        return None;
    }
    let op = cipher::Op {
        spec,
        key: &key,
        iv: &iv_vec,
        enc,
        padding,
        aad,
        tag_len: eff_tag_len,
        tag: tag_set.as_deref(),
    };
    let (out, full_tag) = match cipher::run(&op, data) {
        Ok(r) => r,
        Err(cipher::Fail::DataLength) => {
            vm.emit_warning(&format!("{}(): Setting of data length failed", fname));
            return None;
        }
        Err(cipher::Fail::Silent) => return None,
    };
    if !enc {
        return Some((out, None));
    }
    if is_aead && tag_wanted {
        let ok = if is_ccm || is_ocb {
            tag_len >= 0 && tag_len as usize == eff_tag_len && eff_tag_len > 0
        } else {
            (1..=16).contains(&tag_len)
        };
        if !ok {
            vm.emit_warning(&format!("{}(): Retrieving verification tag failed", fname));
            return None;
        }
        let t = full_tag[..tag_len as usize].to_vec();
        return Some((out, Some(t)));
    } else if is_aead && !tag_wanted {
        vm.emit_warning(&format!(
            "{}(): A tag should be provided when using AEAD mode",
            fname
        ));
        return None;
    }
    Some((out, None))
}

/// openssl_encrypt(data, cipher_algo, passphrase, options=0, iv="", &tag=null, aad="", tag_length=16)
fn openssl_encrypt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_encrypt";
    let data = zpp::str(vm, args, 0, f, "data")?;
    let method = zpp::str(vm, args, 1, f, "cipher_algo")?;
    let password = zpp::str(vm, args, 2, f, "passphrase")?;
    let options = if zpp::is_passed(args, 3) {
        zpp::long(vm, args, 3, f, "options")?
    } else {
        0
    };
    let iv = if zpp::is_passed(args, 4) {
        zpp::str(vm, args, 4, f, "iv")?
    } else {
        PhpString::empty()
    };
    let aad = if zpp::is_passed(args, 6) {
        zpp::str(vm, args, 6, f, "aad")?
    } else {
        PhpString::empty()
    };
    let tag_len = if zpp::is_passed(args, 7) {
        zpp::long(vm, args, 7, f, "tag_length")?
    } else {
        16
    };
    let tag_given = matches!(args.get(5), Some(Value::Reference(_)));
    match php_openssl_cipher(
        vm,
        true,
        data.as_bytes(),
        &method.to_string_lossy(),
        password.as_bytes(),
        options,
        iv.as_bytes(),
        tag_given,
        None,
        tag_len,
        aad.as_bytes(),
    ) {
        Some((out, tag)) => {
            if tag_given {
                zpp::set_ref(args, 5, tag.map(zpp::bytes).unwrap_or(Value::Null));
            }
            if options & OPENSSL_RAW_DATA != 0 {
                Ok(zpp::bytes(out))
            } else {
                Ok(Value::String(PhpString::from_string(base64_encode(&out))))
            }
        }
        None => Ok(Value::False),
    }
}

/// openssl_decrypt(data, cipher_algo, passphrase, options=0, iv="", tag=null, aad="")
fn openssl_decrypt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_decrypt";
    let data = zpp::str(vm, args, 0, f, "data")?;
    let method = zpp::str(vm, args, 1, f, "cipher_algo")?;
    let password = zpp::str(vm, args, 2, f, "passphrase")?;
    let options = if zpp::is_passed(args, 3) {
        zpp::long(vm, args, 3, f, "options")?
    } else {
        0
    };
    let iv = if zpp::is_passed(args, 4) {
        zpp::str(vm, args, 4, f, "iv")?
    } else {
        PhpString::empty()
    };
    let tag = zpp::str_nullable(vm, args, 5, f, "tag")?;
    let aad = if zpp::is_passed(args, 6) {
        zpp::str(vm, args, 6, f, "aad")?
    } else {
        PhpString::empty()
    };
    if method.is_empty() {
        return Err(zpp::must_not_be_empty(vm, f, 2, "cipher_algo"));
    }
    let tag_len = tag.as_ref().map(|t| t.len() as i64).unwrap_or(0);
    match php_openssl_cipher(
        vm,
        false,
        data.as_bytes(),
        &method.to_string_lossy(),
        password.as_bytes(),
        options,
        iv.as_bytes(),
        false,
        tag.as_ref().map(|t| t.as_bytes()),
        tag_len,
        aad.as_bytes(),
    ) {
        Some((out, _)) => Ok(zpp::bytes(out)),
        None => Ok(Value::False),
    }
}

/// openssl_random_pseudo_bytes(int $length, &$strong_result = null): string
fn openssl_random_pseudo_bytes(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_random_pseudo_bytes";
    let length = zpp::long(vm, args, 0, f, "length")?;
    if length <= 0 {
        zpp::set_ref(args, 1, Value::False);
        return Err(zpp::arg_value_error(
            vm,
            f,
            1,
            "length",
            "must be greater than 0",
        ));
    }
    if length > i32::MAX as i64 {
        zpp::set_ref(args, 1, Value::False);
        return Err(zpp::arg_value_error(
            vm,
            f,
            1,
            "length",
            &format!("must be less than or equal to {}", i32::MAX),
        ));
    }
    let mut buf = vec![0u8; length as usize];
    use purecrypto::rng::RngCore;
    purecrypto::rng::OsRng.fill_bytes(&mut buf);
    zpp::set_ref(args, 1, Value::True);
    Ok(zpp::bytes(buf))
}

/// openssl_digest(string $data, string $digest_algo, bool $binary = false): string|false
fn openssl_digest(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_digest";
    let data = zpp::str(vm, args, 0, f, "data")?;
    let algo = zpp::str(vm, args, 1, f, "digest_algo")?;
    let binary = if zpp::is_passed(args, 2) {
        zpp::bool(vm, args, 2, f, "binary")?
    } else {
        false
    };
    match digest::Md::by_name(&algo.to_string_lossy()) {
        Some(md) => {
            let d = md.digest(data.as_bytes());
            if binary {
                Ok(zpp::bytes(d))
            } else {
                Ok(Value::String(PhpString::from_string(hex_lower(&d))))
            }
        }
        None => {
            vm.emit_warning(&format!("{}(): Unknown digest algorithm", f));
            Ok(Value::False)
        }
    }
}

pub(crate) fn hex_lower(d: &[u8]) -> String {
    d.iter().map(|b| format!("{:02x}", b)).collect()
}

pub(crate) fn string_list(items: impl IntoIterator<Item = String>) -> Value {
    let mut arr = PhpArray::new();
    for s in items {
        arr.push(Value::String(PhpString::from_string(s)));
    }
    Value::Array(Rc::new(RefCell::new(arr)))
}

/// openssl_get_cipher_methods(bool $aliases = false): array
fn openssl_get_cipher_methods(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_get_cipher_methods";
    let aliases = if zpp::is_passed(args, 0) {
        zpp::bool(vm, args, 0, f, "aliases")?
    } else {
        false
    };
    Ok(string_list(cipher::method_names(aliases)))
}

fn cipher_len_fn(vm: &mut Vm, args: &[Value], f: &str, key: bool) -> Result<Value, VmError> {
    let method = zpp::str(vm, args, 0, f, "cipher_algo")?;
    if method.is_empty() {
        return Err(zpp::must_not_be_empty(vm, f, 1, "cipher_algo"));
    }
    match cipher::by_name(&method.to_string_lossy()) {
        Some(spec) => Ok(Value::Long(
            if key { spec.key_len } else { spec.iv_len } as i64
        )),
        None => {
            vm.emit_warning(&format!("{}(): Unknown cipher algorithm", f));
            Ok(Value::False)
        }
    }
}

fn openssl_cipher_iv_length(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    cipher_len_fn(vm, args, "openssl_cipher_iv_length", false)
}

fn openssl_cipher_key_length(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    cipher_len_fn(vm, args, "openssl_cipher_key_length", true)
}

/// openssl_get_md_methods(bool $aliases = false): array
fn openssl_get_md_methods(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "openssl_get_md_methods";
    let aliases = if zpp::is_passed(args, 0) {
        zpp::bool(vm, args, 0, f, "aliases")?
    } else {
        false
    };
    let list = if aliases {
        digest::MD_METHODS_ALIASES
    } else {
        digest::MD_METHODS
    };
    Ok(string_list(list.iter().map(|s| s.to_string())))
}

/// openssl_error_string(): string|false
fn openssl_error_string(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    match err::pop() {
        Some(s) => Ok(Value::String(PhpString::from_string(s))),
        None => Ok(Value::False),
    }
}

// ── Stub implementations for commonly tested but unimplemented openssl functions ──

fn openssl_get_cert_locations_stub(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = goro_core::array::PhpArray::new();
    arr.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"default_cert_file")),
        Value::String(PhpString::from_bytes(b"/etc/ssl/certs/ca-certificates.crt")),
    );
    arr.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"default_cert_file_env")),
        Value::String(PhpString::from_bytes(b"SSL_CERT_FILE")),
    );
    arr.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"default_cert_dir")),
        Value::String(PhpString::from_bytes(b"/etc/ssl/certs")),
    );
    arr.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"default_cert_dir_env")),
        Value::String(PhpString::from_bytes(b"SSL_CERT_DIR")),
    );
    arr.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"default_private_dir")),
        Value::String(PhpString::from_bytes(b"/etc/ssl/private")),
    );
    arr.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"default_default_cert_area")),
        Value::String(PhpString::from_bytes(b"/etc/ssl")),
    );
    arr.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"ini_cafile")),
        Value::String(PhpString::from_bytes(b"")),
    );
    arr.set(
        goro_core::array::ArrayKey::String(PhpString::from_bytes(b"ini_capath")),
        Value::String(PhpString::from_bytes(b"")),
    );
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}
