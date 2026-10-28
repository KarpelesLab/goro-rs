//! The openssl extension (PHP manual: OpenSSL) on goro-ext-openssl: the
//! OpenSSLCertificate, OpenSSLCertificateSigningRequest and
//! OpenSSLAsymmetricKey objects, the openssl_*() functions and their
//! constants.
//!
//! Behaviour sources: the PHP manual, the expected output of
//! ext/openssl/tests/*.phpt, and PHP 8.5 observed as a black box.

mod cipher;

use goro_ext_openssl as ossl;
use goro_runtime::{ObjRef, PhpStr, Value};

use crate::class::{NativeHooks, cflags};
use crate::func::NativeFn;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "openssl",
    functions: FUNCTIONS,
    classes: &[
        (
            "opensslcertificate",
            "<?php #[NotSerializable] final class OpenSSLCertificate {}",
        ),
        (
            "opensslcertificatesigningrequest",
            "<?php #[NotSerializable] final class OpenSSLCertificateSigningRequest {}",
        ),
        (
            "opensslasymmetrickey",
            "<?php #[NotSerializable] final class OpenSSLAsymmetricKey {}",
        ),
    ],
    hooks: &[
        ("opensslcertificate", object_hooks),
        ("opensslcertificatesigningrequest", object_hooks),
        ("opensslasymmetrickey", object_hooks),
    ],
    constants: CONSTANTS,
    // PHP manual: OpenSSL runtime configuration.
    ini: &[
        ("openssl.cafile", ""),
        ("openssl.capath", ""),
        ("openssl.libctx", "custom"),
    ],
    listed: true,
    ..Module::EMPTY
};

const FUNCTIONS: &[(&str, NativeFn)] = &[
    (
        "function openssl_get_cipher_methods(bool $aliases = false): array",
        cipher::openssl_get_cipher_methods,
    ),
    (
        "function openssl_cipher_iv_length(string $cipher_algo): int|false",
        cipher::openssl_cipher_iv_length,
    ),
    (
        "function openssl_cipher_key_length(string $cipher_algo): int|false",
        cipher::openssl_cipher_key_length,
    ),
    (
        "function openssl_encrypt(#[\\SensitiveParameter] string $data, string $cipher_algo, #[\\SensitiveParameter] string $passphrase, int $options = 0, string $iv = \"\", &$tag = null, string $aad = \"\", int $tag_length = 16): string|false",
        cipher::openssl_encrypt,
    ),
    (
        "function openssl_decrypt(string $data, string $cipher_algo, #[\\SensitiveParameter] string $passphrase, int $options = 0, string $iv = \"\", ?string $tag = null, string $aad = \"\"): string|false",
        cipher::openssl_decrypt,
    ),
    (
        "function openssl_get_md_methods(bool $aliases = false): array",
        cipher::openssl_get_md_methods,
    ),
    (
        "function openssl_digest(string $data, string $digest_algo, bool $binary = false): string|false",
        cipher::openssl_digest,
    ),
    (
        "function openssl_pbkdf2(#[\\SensitiveParameter] string $password, string $salt, int $key_length, int $iterations, string $digest_algo = \"sha1\"): string|false",
        cipher::openssl_pbkdf2,
    ),
    (
        "function openssl_random_pseudo_bytes(int $length, &$strong_result = null): string",
        cipher::openssl_random_pseudo_bytes,
    ),
    (
        "function openssl_error_string(): string|false",
        openssl_error_string,
    ),
];

/// The extension's constants (observed on PHP 8.5; PHP manual: OpenSSL
/// predefined constants). OPENSSL_VERSION_* describe the OpenSSL release
/// whose behaviour goro follows (3.5), which the tests check against.
const CONSTANTS: &[(&str, ConstVal)] = &[
    (
        "OPENSSL_VERSION_TEXT",
        ConstVal::Str("OpenSSL 3.5.5 27 Jan 2026"),
    ),
    ("OPENSSL_VERSION_NUMBER", ConstVal::Int(0x30500050)),
    ("X509_PURPOSE_SSL_CLIENT", ConstVal::Int(1)),
    ("X509_PURPOSE_SSL_SERVER", ConstVal::Int(2)),
    ("X509_PURPOSE_NS_SSL_SERVER", ConstVal::Int(3)),
    ("X509_PURPOSE_SMIME_SIGN", ConstVal::Int(4)),
    ("X509_PURPOSE_SMIME_ENCRYPT", ConstVal::Int(5)),
    ("X509_PURPOSE_CRL_SIGN", ConstVal::Int(6)),
    ("X509_PURPOSE_ANY", ConstVal::Int(7)),
    ("X509_PURPOSE_OCSP_HELPER", ConstVal::Int(8)),
    ("X509_PURPOSE_TIMESTAMP_SIGN", ConstVal::Int(9)),
    ("OPENSSL_ALGO_SHA1", ConstVal::Int(1)),
    ("OPENSSL_ALGO_MD5", ConstVal::Int(2)),
    ("OPENSSL_ALGO_MD4", ConstVal::Int(3)),
    ("OPENSSL_ALGO_SHA224", ConstVal::Int(6)),
    ("OPENSSL_ALGO_SHA256", ConstVal::Int(7)),
    ("OPENSSL_ALGO_SHA384", ConstVal::Int(8)),
    ("OPENSSL_ALGO_SHA512", ConstVal::Int(9)),
    ("OPENSSL_ALGO_RMD160", ConstVal::Int(10)),
    ("PKCS7_DETACHED", ConstVal::Int(64)),
    ("PKCS7_TEXT", ConstVal::Int(1)),
    ("PKCS7_NOINTERN", ConstVal::Int(16)),
    ("PKCS7_NOVERIFY", ConstVal::Int(32)),
    ("PKCS7_NOCHAIN", ConstVal::Int(8)),
    ("PKCS7_NOCERTS", ConstVal::Int(2)),
    ("PKCS7_NOATTR", ConstVal::Int(256)),
    ("PKCS7_BINARY", ConstVal::Int(128)),
    ("PKCS7_NOSIGS", ConstVal::Int(4)),
    ("PKCS7_NOOLDMIMETYPE", ConstVal::Int(1024)),
    ("PKCS7_NOSMIMECAP", ConstVal::Int(512)),
    ("PKCS7_CRLFEOL", ConstVal::Int(2048)),
    ("PKCS7_NOCRL", ConstVal::Int(8192)),
    ("PKCS7_NO_DUAL_CONTENT", ConstVal::Int(65536)),
    ("OPENSSL_CMS_DETACHED", ConstVal::Int(64)),
    ("OPENSSL_CMS_TEXT", ConstVal::Int(1)),
    ("OPENSSL_CMS_NOINTERN", ConstVal::Int(16)),
    ("OPENSSL_CMS_NOVERIFY", ConstVal::Int(32)),
    ("OPENSSL_CMS_NOCERTS", ConstVal::Int(2)),
    ("OPENSSL_CMS_NOATTR", ConstVal::Int(256)),
    ("OPENSSL_CMS_BINARY", ConstVal::Int(128)),
    ("OPENSSL_CMS_NOSIGS", ConstVal::Int(12)),
    ("OPENSSL_CMS_OLDMIMETYPE", ConstVal::Int(1024)),
    ("OPENSSL_PKCS1_PADDING", ConstVal::Int(1)),
    ("OPENSSL_NO_PADDING", ConstVal::Int(3)),
    ("OPENSSL_PKCS1_OAEP_PADDING", ConstVal::Int(4)),
    ("OPENSSL_PKCS1_PSS_PADDING", ConstVal::Int(6)),
    (
        "OPENSSL_DEFAULT_STREAM_CIPHERS",
        ConstVal::Str(
            "ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES256-GCM-SHA384:DHE-RSA-AES128-GCM-SHA256:DHE-DSS-AES128-GCM-SHA256:kEDH+AESGCM:ECDHE-RSA-AES128-SHA256:ECDHE-ECDSA-AES128-SHA256:ECDHE-RSA-AES128-SHA:ECDHE-ECDSA-AES128-SHA:ECDHE-RSA-AES256-SHA384:ECDHE-ECDSA-AES256-SHA384:ECDHE-RSA-AES256-SHA:ECDHE-ECDSA-AES256-SHA:DHE-RSA-AES128-SHA256:DHE-RSA-AES128-SHA:DHE-DSS-AES128-SHA256:DHE-RSA-AES256-SHA256:DHE-DSS-AES256-SHA:DHE-RSA-AES256-SHA:AES128-GCM-SHA256:AES256-GCM-SHA384:AES128:AES256:HIGH:!SSLv2:!aNULL:!eNULL:!EXPORT:!DES:!MD5:!RC4:!ADH",
        ),
    ),
    ("OPENSSL_CIPHER_RC2_40", ConstVal::Int(0)),
    ("OPENSSL_CIPHER_RC2_128", ConstVal::Int(1)),
    ("OPENSSL_CIPHER_RC2_64", ConstVal::Int(2)),
    ("OPENSSL_CIPHER_DES", ConstVal::Int(3)),
    ("OPENSSL_CIPHER_3DES", ConstVal::Int(4)),
    ("OPENSSL_CIPHER_AES_128_CBC", ConstVal::Int(5)),
    ("OPENSSL_CIPHER_AES_192_CBC", ConstVal::Int(6)),
    ("OPENSSL_CIPHER_AES_256_CBC", ConstVal::Int(7)),
    ("OPENSSL_KEYTYPE_RSA", ConstVal::Int(0)),
    ("OPENSSL_KEYTYPE_DSA", ConstVal::Int(1)),
    ("OPENSSL_KEYTYPE_DH", ConstVal::Int(2)),
    ("OPENSSL_KEYTYPE_EC", ConstVal::Int(3)),
    ("OPENSSL_KEYTYPE_X25519", ConstVal::Int(4)),
    ("OPENSSL_KEYTYPE_ED25519", ConstVal::Int(5)),
    ("OPENSSL_KEYTYPE_X448", ConstVal::Int(6)),
    ("OPENSSL_KEYTYPE_ED448", ConstVal::Int(7)),
    ("OPENSSL_RAW_DATA", ConstVal::Int(1)),
    ("OPENSSL_ZERO_PADDING", ConstVal::Int(2)),
    ("OPENSSL_DONT_ZERO_PAD_KEY", ConstVal::Int(4)),
    ("OPENSSL_TLSEXT_SERVER_NAME", ConstVal::Int(1)),
    ("OPENSSL_ENCODING_DER", ConstVal::Int(0)),
    ("OPENSSL_ENCODING_SMIME", ConstVal::Int(1)),
    ("OPENSSL_ENCODING_PEM", ConstVal::Int(2)),
];

fn object_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|vm, o| {
            let cls = vm.class_of(o);
            cls.flags.set(
                cls.flags.get()
                    | cflags::NO_DYNAMIC
                    | cflags::NOT_SERIALIZABLE
                    | cflags::NOT_CLONEABLE,
            );
        }),
        ..Default::default()
    }
}

/// Native data of an OpenSSLCertificate: the certificate's DER.
#[allow(dead_code)]
pub(crate) struct CertData(pub std::rc::Rc<Vec<u8>>);
impl goro_runtime::NativeData for CertData {}

/// Runs `f` on an object's native data of type `T`.
#[allow(dead_code)]
pub(crate) fn with_native<T: 'static, U>(o: &ObjRef, f: impl FnOnce(&mut T) -> U) -> Option<U> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<T>().map(f)
}

/// A new object of an extension class holding `data`.
#[allow(dead_code)]
pub(crate) fn new_object<T: goro_runtime::NativeData + 'static>(
    vm: &mut Vm,
    class: &[u8],
    data: T,
) -> R<ObjRef> {
    let cls = vm.find_class(class, false)?.expect("openssl class");
    let o = vm.instantiate_raw(&cls)?;
    *o.obj().native.borrow_mut() = Some(Box::new(data));
    Ok(o)
}

/// An OpenSSLCertificate object for a DER certificate (for the TLS layer's
/// `capture_peer_cert` options and the PKCS functions).
#[allow(dead_code)]
pub(crate) fn cert_value(vm: &mut Vm, der: Vec<u8>) -> R<Value> {
    let o = new_object(vm, b"OpenSSLCertificate", CertData(std::rc::Rc::new(der)))?;
    Ok(Value::Object(o))
}

fn openssl_error_string(_vm: &mut Vm, _a: &mut crate::func::Args) -> R<Value> {
    Ok(match ossl::err::pop() {
        Some(m) => Value::Str(PhpStr::from(m)),
        None => Value::Bool(false),
    })
}
