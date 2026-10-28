//! The openssl extension (PHP manual: OpenSSL) on goro-ext-openssl: the
//! OpenSSLCertificate, OpenSSLCertificateSigningRequest and
//! OpenSSLAsymmetricKey objects, the openssl_*() functions and their
//! constants.
//!
//! Behaviour sources: the PHP manual, the expected output of
//! ext/openssl/tests/*.phpt, and PHP 8.5 observed as a black box.

mod cipher;
mod csr;
mod pkcs12;
mod pkcs7;
mod pkey;
pub(crate) mod tls;
mod x509;

use std::rc::Rc;

use goro_ext_openssl as ossl;
use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};

use crate::class::{NativeHooks, cflags};
use crate::func::{Args, NativeFn};
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

use ossl::err::{self as oerr, errs};

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
    // x509
    (
        "function openssl_x509_export_to_file(OpenSSLCertificate|string $certificate, string $output_filename, bool $no_text = true): bool",
        x509::openssl_x509_export_to_file,
    ),
    (
        "function openssl_x509_export(OpenSSLCertificate|string $certificate, &$output, bool $no_text = true): bool",
        x509::openssl_x509_export,
    ),
    (
        "function openssl_x509_fingerprint(OpenSSLCertificate|string $certificate, string $digest_algo = \"sha1\", bool $binary = false): string|false",
        x509::openssl_x509_fingerprint,
    ),
    (
        "function openssl_x509_check_private_key(OpenSSLCertificate|string $certificate, #[\\SensitiveParameter] $private_key): bool",
        x509::openssl_x509_check_private_key,
    ),
    (
        "function openssl_x509_verify(OpenSSLCertificate|string $certificate, $public_key): int",
        x509::openssl_x509_verify,
    ),
    (
        "function openssl_x509_parse(OpenSSLCertificate|string $certificate, bool $short_names = true): array|false",
        x509::openssl_x509_parse,
    ),
    (
        "function openssl_x509_checkpurpose(OpenSSLCertificate|string $certificate, int $purpose, array $ca_info = [], ?string $untrusted_certificates_file = null): int|bool",
        x509::openssl_x509_checkpurpose,
    ),
    (
        "function openssl_x509_read(OpenSSLCertificate|string $certificate): OpenSSLCertificate|false",
        x509::openssl_x509_read,
    ),
    (
        "function openssl_x509_free(OpenSSLCertificate $certificate): void",
        x509::openssl_x509_free,
    ),
    // csr
    (
        "function openssl_csr_export_to_file(OpenSSLCertificateSigningRequest|string $csr, string $output_filename, bool $no_text = true): bool",
        csr::openssl_csr_export_to_file,
    ),
    (
        "function openssl_csr_export(OpenSSLCertificateSigningRequest|string $csr, &$output, bool $no_text = true): bool",
        csr::openssl_csr_export,
    ),
    (
        "function openssl_csr_sign(OpenSSLCertificateSigningRequest|string $csr, OpenSSLCertificate|string|null $ca_certificate, #[\\SensitiveParameter] $private_key, int $days, ?array $options = null, int $serial = 0, ?string $serial_hex = null): OpenSSLCertificate|false",
        csr::openssl_csr_sign,
    ),
    (
        "function openssl_csr_new(array $distinguished_names, #[\\SensitiveParameter] &$private_key, ?array $options = null, ?array $extra_attributes = null): OpenSSLCertificateSigningRequest|bool",
        csr::openssl_csr_new,
    ),
    (
        "function openssl_csr_get_subject(OpenSSLCertificateSigningRequest|string $csr, bool $short_names = true): array|false",
        csr::openssl_csr_get_subject,
    ),
    (
        "function openssl_csr_get_public_key(OpenSSLCertificateSigningRequest|string $csr, bool $short_names = true): OpenSSLAsymmetricKey|false",
        csr::openssl_csr_get_public_key,
    ),
    // pkcs12
    (
        "function openssl_pkcs12_export_to_file(OpenSSLCertificate|string $certificate, string $output_filename, #[\\SensitiveParameter] $private_key, #[\\SensitiveParameter] string $passphrase, array $options = []): bool",
        pkcs12::openssl_pkcs12_export_to_file,
    ),
    (
        "function openssl_pkcs12_export(OpenSSLCertificate|string $certificate, &$output, #[\\SensitiveParameter] $private_key, #[\\SensitiveParameter] string $passphrase, array $options = []): bool",
        pkcs12::openssl_pkcs12_export,
    ),
    (
        "function openssl_pkcs12_read(string $pkcs12, &$certificates, #[\\SensitiveParameter] string $passphrase): bool",
        pkcs12::openssl_pkcs12_read,
    ),
    // pkcs7 / cms
    (
        "function openssl_pkcs7_verify(string $input_filename, int $flags, ?string $signers_certificates_filename = null, array $ca_info = [], ?string $untrusted_certificates_filename = null, ?string $content = null, ?string $output_filename = null): int|bool",
        pkcs7::openssl_pkcs7_verify,
    ),
    (
        "function openssl_pkcs7_encrypt(string $input_filename, string $output_filename, $certificate, ?array $headers, int $flags = 0, int $cipher_algo = OPENSSL_CIPHER_AES_128_CBC): bool",
        pkcs7::openssl_pkcs7_encrypt,
    ),
    (
        "function openssl_pkcs7_sign(string $input_filename, string $output_filename, OpenSSLCertificate|string $certificate, #[\\SensitiveParameter] $private_key, ?array $headers, int $flags = PKCS7_DETACHED, ?string $untrusted_certificates_filename = null): bool",
        pkcs7::openssl_pkcs7_sign,
    ),
    (
        "function openssl_pkcs7_decrypt(string $input_filename, string $output_filename, #[\\SensitiveParameter] $certificate, #[\\SensitiveParameter] $private_key = null): bool",
        pkcs7::openssl_pkcs7_decrypt,
    ),
    (
        "function openssl_pkcs7_read(string $data, &$certificates): bool",
        pkcs7::openssl_pkcs7_read,
    ),
    (
        "function openssl_cms_verify(string $input_filename, int $flags = 0, ?string $certificates = null, array $ca_info = [], ?string $untrusted_certificates_filename = null, ?string $content = null, ?string $pk7 = null, ?string $sigfile = null, int $encoding = OPENSSL_ENCODING_SMIME): bool",
        pkcs7::openssl_cms_verify,
    ),
    (
        "function openssl_cms_encrypt(string $input_filename, string $output_filename, $certificate, ?array $headers, int $flags = 0, int $encoding = OPENSSL_ENCODING_SMIME, string|int $cipher_algo = OPENSSL_CIPHER_AES_128_CBC): bool",
        pkcs7::openssl_cms_encrypt,
    ),
    (
        "function openssl_cms_sign(string $input_filename, string $output_filename, OpenSSLCertificate|string $certificate, #[\\SensitiveParameter] $private_key, ?array $headers, int $flags = 0, int $encoding = OPENSSL_ENCODING_SMIME, ?string $untrusted_certificates_filename = null): bool",
        pkcs7::openssl_cms_sign,
    ),
    (
        "function openssl_cms_decrypt(string $input_filename, string $output_filename, #[\\SensitiveParameter] $certificate, #[\\SensitiveParameter] $private_key = null, int $encoding = OPENSSL_ENCODING_SMIME): bool",
        pkcs7::openssl_cms_decrypt,
    ),
    (
        "function openssl_cms_read(string $input_filename, &$certificates): bool",
        pkcs7::openssl_cms_read,
    ),
    // spki
    (
        "function openssl_spki_new(#[\\SensitiveParameter] OpenSSLAsymmetricKey $private_key, string $challenge, int $digest_algo = OPENSSL_ALGO_MD5): string|false",
        pkcs12::openssl_spki_new,
    ),
    (
        "function openssl_spki_verify(string $spki): bool",
        pkcs12::openssl_spki_verify,
    ),
    (
        "function openssl_spki_export(string $spki): string|false",
        pkcs12::openssl_spki_export,
    ),
    (
        "function openssl_spki_export_challenge(string $spki): string|false",
        pkcs12::openssl_spki_export_challenge,
    ),
    // keys
    (
        "function openssl_pkey_new(?array $options = null): OpenSSLAsymmetricKey|false",
        pkey::openssl_pkey_new,
    ),
    (
        "function openssl_pkey_export_to_file(#[\\SensitiveParameter] $key, string $output_filename, #[\\SensitiveParameter] ?string $passphrase = null, ?array $options = null): bool",
        pkey::openssl_pkey_export_to_file,
    ),
    (
        "function openssl_pkey_export(#[\\SensitiveParameter] $key, &$output, #[\\SensitiveParameter] ?string $passphrase = null, ?array $options = null): bool",
        pkey::openssl_pkey_export,
    ),
    (
        "function openssl_pkey_get_public($public_key): OpenSSLAsymmetricKey|false",
        pkey::openssl_pkey_get_public,
    ),
    (
        "function openssl_get_publickey($public_key): OpenSSLAsymmetricKey|false",
        pkey::openssl_pkey_get_public,
    ),
    (
        "function openssl_pkey_free(OpenSSLAsymmetricKey $key): void",
        pkey::openssl_pkey_free,
    ),
    (
        "function openssl_free_key(OpenSSLAsymmetricKey $key): void",
        pkey::openssl_pkey_free,
    ),
    (
        "function openssl_pkey_get_private(#[\\SensitiveParameter] $private_key, #[\\SensitiveParameter] ?string $passphrase = null): OpenSSLAsymmetricKey|false",
        pkey::openssl_pkey_get_private,
    ),
    (
        "function openssl_get_privatekey(#[\\SensitiveParameter] $private_key, #[\\SensitiveParameter] ?string $passphrase = null): OpenSSLAsymmetricKey|false",
        pkey::openssl_pkey_get_private,
    ),
    (
        "function openssl_pkey_get_details(OpenSSLAsymmetricKey $key): array|false",
        pkey::openssl_pkey_get_details,
    ),
    (
        "function openssl_private_encrypt(#[\\SensitiveParameter] string $data, &$encrypted_data, #[\\SensitiveParameter] $private_key, int $padding = OPENSSL_PKCS1_PADDING): bool",
        pkey::openssl_private_encrypt,
    ),
    (
        "function openssl_private_decrypt(string $data, #[\\SensitiveParameter] &$decrypted_data, #[\\SensitiveParameter] $private_key, int $padding = OPENSSL_PKCS1_PADDING, ?string $digest_algo = null): bool",
        pkey::openssl_private_decrypt,
    ),
    (
        "function openssl_public_encrypt(#[\\SensitiveParameter] string $data, &$encrypted_data, $public_key, int $padding = OPENSSL_PKCS1_PADDING, ?string $digest_algo = null): bool",
        pkey::openssl_public_encrypt,
    ),
    (
        "function openssl_public_decrypt(string $data, #[\\SensitiveParameter] &$decrypted_data, $public_key, int $padding = OPENSSL_PKCS1_PADDING): bool",
        pkey::openssl_public_decrypt,
    ),
    (
        "function openssl_sign(string $data, &$signature, #[\\SensitiveParameter] $private_key, string|int $algorithm = OPENSSL_ALGO_SHA1, int $padding = 0): bool",
        pkey::openssl_sign,
    ),
    (
        "function openssl_verify(string $data, string $signature, $public_key, string|int $algorithm = OPENSSL_ALGO_SHA1, int $padding = 0): int|false",
        pkey::openssl_verify,
    ),
    (
        "function openssl_dh_compute_key(string $public_key, OpenSSLAsymmetricKey $private_key): string|false",
        pkey::openssl_dh_compute_key,
    ),
    (
        "function openssl_pkey_derive($public_key, #[\\SensitiveParameter] $private_key, int $key_length = 0): string|false",
        pkey::openssl_pkey_derive,
    ),
    (
        "function openssl_seal(#[\\SensitiveParameter] string $data, &$sealed_data, &$encrypted_keys, array $public_key, string $cipher_algo, &$iv = null): int|false",
        pkey::openssl_seal,
    ),
    (
        "function openssl_open(string $data, #[\\SensitiveParameter] &$output, string $encrypted_key, #[\\SensitiveParameter] $private_key, string $cipher_algo, ?string $iv = null): bool",
        pkey::openssl_open,
    ),
    (
        "function openssl_get_curve_names(): array|false",
        pkey::openssl_get_curve_names,
    ),
    (
        "function openssl_get_cert_locations(): array",
        openssl_get_cert_locations,
    ),
    // ciphers, digests, randomness
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

// ------------------------------------------------------------ objects

/// Native data of an OpenSSLCertificate.
pub(crate) struct CertData(pub Rc<ossl::x509::Cert>);
impl goro_runtime::NativeData for CertData {}

/// Native data of an OpenSSLCertificateSigningRequest.
pub(crate) struct CsrData(pub Rc<ossl::csr::Csr>);
impl goro_runtime::NativeData for CsrData {}

/// Native data of an OpenSSLAsymmetricKey.
pub(crate) struct KeyObj(pub Rc<ossl::key::Key>);
impl goro_runtime::NativeData for KeyObj {}

/// Runs `f` on an object's native data of type `T`.
pub(crate) fn with_native<T: 'static, U>(o: &ObjRef, f: impl FnOnce(&mut T) -> U) -> Option<U> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<T>().map(f)
}

/// A new object of an extension class holding `data`.
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
/// `capture_peer_cert` options and the PKCS functions); none when the
/// DER doesn't parse.
#[allow(dead_code)]
pub(crate) fn cert_value(vm: &mut Vm, der: Vec<u8>) -> R<Value> {
    match ossl::x509::Cert::from_der(&der) {
        Ok(c) => cert_obj(vm, Rc::new(c)),
        Err(_) => Ok(Value::Bool(false)),
    }
}

pub(crate) fn cert_obj(vm: &mut Vm, c: Rc<ossl::x509::Cert>) -> R<Value> {
    Ok(Value::Object(new_object(
        vm,
        b"OpenSSLCertificate",
        CertData(c),
    )?))
}

pub(crate) fn key_obj(vm: &mut Vm, k: ossl::key::Key) -> R<Value> {
    Ok(Value::Object(new_object(
        vm,
        b"OpenSSLAsymmetricKey",
        KeyObj(Rc::new(k)),
    )?))
}

pub(crate) fn key_obj_rc(vm: &mut Vm, k: Rc<ossl::key::Key>) -> R<Value> {
    Ok(Value::Object(new_object(
        vm,
        b"OpenSSLAsymmetricKey",
        KeyObj(k),
    )?))
}

pub(crate) fn csr_obj(vm: &mut Vm, c: ossl::csr::Csr) -> R<Value> {
    Ok(Value::Object(new_object(
        vm,
        b"OpenSSLCertificateSigningRequest",
        CsrData(Rc::new(c)),
    )?))
}

fn obj_class_is(vm: &Vm, o: &ObjRef, name: &[u8]) -> bool {
    vm.class_of(o).name.as_bytes().eq_ignore_ascii_case(name)
}

// ------------------------------------------------------------ files

/// Reads the file of a `file://` argument (PHP's openssl functions read
/// such paths directly, after the open_basedir check). Pushes the
/// library errors of a failed open.
pub(crate) fn read_openssl_file(vm: &mut Vm, a: &Args, path: &[u8]) -> R<Option<Vec<u8>>> {
    if !vm.basedir_allows(path) {
        basedir_warning(vm, a, path)?;
        return Ok(None);
    }
    match std::fs::read(os_path(path)) {
        Ok(d) => Ok(Some(d)),
        Err(_) => {
            oerr::push(errs::SYS_NOENT);
            oerr::push(errs::BIO_NO_SUCH_FILE);
            Ok(None)
        }
    }
}

pub(crate) fn os_path(b: &[u8]) -> std::path::PathBuf {
    use std::os::unix::ffi::OsStrExt;
    std::path::PathBuf::from(std::ffi::OsStr::from_bytes(b))
}

fn basedir_warning(vm: &mut Vm, a: &Args, path: &[u8]) -> R<()> {
    let bd = vm
        .get_ini(b"open_basedir")
        .map(|v| v.to_string_lossy())
        .unwrap_or_default();
    vm.fwarn(
        a,
        &format!(
            "open_basedir restriction in effect. File({}) is not within the allowed path(s): ({})",
            String::from_utf8_lossy(path),
            bd
        ),
    )
}

/// The data behind a key/cert string argument: the file for `file://`
/// paths, else the string itself. A path with a NUL byte defers the
/// ValueError PHP 8.5 raises for it (test bug81713.phpt: the function
/// still finishes, with its usual warning for certificates).
pub(crate) fn arg_data(vm: &mut Vm, a: &Args, s: &[u8]) -> R<Option<Vec<u8>>> {
    if s.len() >= 7 && s[..7].eq_ignore_ascii_case(b"file://") {
        if s.contains(&0) {
            defer_nul_error(vm, a, s);
            return Ok(None);
        }
        return read_openssl_file(vm, a, &s[7..]);
    }
    Ok(Some(s.to_vec()))
}

/// "#N ($name)" of argument `i`.
pub(crate) fn arg_label(vm: &mut Vm, a: &Args, i: usize) -> String {
    let sig = vm.native_sig(a.func);
    let name = sig
        .params
        .get(i)
        .map(|p| p.name.to_string_lossy())
        .unwrap_or_default();
    format!("#{} (${})", i + 1, name)
}

fn defer_nul_error(vm: &mut Vm, a: &Args, s: &[u8]) {
    if vm.err.deferred.is_some() {
        return;
    }
    // Which argument holds the string: directly, in a list, or in the
    // options' extracerts.
    let mut where_ = None;
    for (i, v) in a.vals.iter().enumerate() {
        match v.deref() {
            Value::Str(x) if x.as_bytes() == s => {
                where_ = Some((i, ""));
                break;
            }
            Value::Array(arr) => {
                for (k, item) in arr.iter() {
                    match item.deref() {
                        Value::Str(x) if x.as_bytes() == s => {
                            where_ = Some((i, "array item "));
                        }
                        Value::Array(inner)
                            if matches!(k, goro_runtime::KeyRef::Str(n) if n.as_bytes() == b"extracerts")
                                && inner
                                    .iter()
                                    .any(|(_, y)| matches!(y.deref(), Value::Str(z) if z.as_bytes() == s)) =>
                        {
                            where_ = Some((i, "option extracerts array item "));
                        }
                        _ => {}
                    }
                }
                if where_.is_some() {
                    break;
                }
            }
            _ => {}
        }
    }
    let (i, mut sfx) = where_.unwrap_or((0, ""));
    // openssl_seal() names its key list without "array item" (test
    // bug81713.phpt).
    if a.fname() == "openssl_seal" {
        sfx = "";
    }
    let label = arg_label(vm, a, i);
    let msg = format!(
        "{}(): Argument {} {}must not contain any null bytes",
        a.fname(),
        label,
        sfx
    );
    vm.err.deferred = Some(vm.throw_error_msg("ValueError", &msg));
}

/// A warning for a key that couldn't be had, unless a ValueError is
/// already pending for it.
pub(crate) fn kwarn(vm: &mut Vm, a: &Args, msg: &str) -> R<()> {
    if vm.err.deferred.is_some() {
        return Ok(());
    }
    vm.fwarn(a, msg)
}

/// PHP's path parameters: a NUL byte is a ValueError before anything
/// else runs.
pub(crate) fn check_path_args(vm: &mut Vm, a: &Args, idx: &[usize]) -> R<()> {
    for &i in idx {
        if let Some(Value::Str(s)) = a.get(i).map(|v| v.deref())
            && s.contains(&0)
        {
            let label = arg_label(vm, a, i);
            return vm.throw(
                "ValueError",
                &format!(
                    "{}(): Argument {} must not contain any null bytes",
                    a.fname(),
                    label
                ),
            );
        }
    }
    Ok(())
}

/// Checks an output path argument (PHP: "must be a valid file path";
/// open_basedir), returning the path.
pub(crate) fn check_output_path(
    vm: &mut Vm,
    a: &Args,
    path: &[u8],
    argname: &str,
) -> R<Option<std::path::PathBuf>> {
    if path.contains(&0) {
        check_path_args(vm, a, &[1])?;
    }
    // Empty or longer than PATH_MAX (observed on PHP 8.5; tests
    // openssl_*_export_to_file_leak.phpt).
    if path.is_empty() || path.len() >= 4096 {
        vm.warn(&format!(
            "{}({}): must be a valid file path ",
            a.fname(),
            argname
        ))?;
        return Ok(None);
    }
    let path = path.strip_prefix(b"file://").unwrap_or(path);
    if !vm.basedir_allows(path) {
        basedir_warning(vm, a, path)?;
        return Ok(None);
    }
    Ok(Some(os_path(path)))
}

/// Writes an output file; false (with the library errors queued) when it
/// can't be written.
pub(crate) fn write_output(p: &std::path::Path, data: &[u8]) -> bool {
    match std::fs::write(p, data) {
        Ok(()) => true,
        Err(_) => {
            oerr::push(errs::BIO_SYS);
            false
        }
    }
}

// ------------------------------------------------------------ arguments

/// Certificate of an `OpenSSLCertificate|string` argument: the object, a
/// PEM string, or a `file://` path. `None` when it can't be read (errors
/// queued).
pub(crate) fn cert_from_value(vm: &mut Vm, a: &Args, v: &Value) -> R<Option<Rc<ossl::x509::Cert>>> {
    match v.deref() {
        Value::Object(o) if with_native::<CertData, _>(&o, |_| ()).is_some() => {
            Ok(with_native::<CertData, _>(&o, |c| c.0.clone()))
        }
        Value::Object(o) if obj_class_is(vm, &o, b"OpenSSLAsymmetricKey") => Ok(None),
        other => {
            let s = vm.to_php_str(&other)?;
            let Some(data) = arg_data(vm, a, s.as_bytes())? else {
                return Ok(None);
            };
            Ok(cert_from_bytes(&data).map(Rc::new))
        }
    }
}

/// The first PEM certificate in `data` (or a DER certificate).
pub(crate) fn cert_from_bytes(data: &[u8]) -> Option<ossl::x509::Cert> {
    for b in ossl::pem::parse_all(data) {
        if b.label == "CERTIFICATE"
            || b.label == "X509 CERTIFICATE"
            || b.label == "TRUSTED CERTIFICATE"
        {
            if let Ok(c) = ossl::x509::Cert::from_der(&b.data) {
                return Some(c);
            }
            oerr::push(errs::ASN1_NESTED_ERROR);
            return None;
        }
    }
    if let Ok(c) = ossl::x509::Cert::from_der(data) {
        return Some(c);
    }
    oerr::push(errs::PEM_NO_START_LINE);
    None
}

/// What a key argument should yield.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Want {
    Public,
    Private,
}

/// The key of a key argument (PHP manual: "Key/Certificate parameters":
/// an OpenSSLAsymmetricKey, an OpenSSLCertificate for a public key, a PEM
/// string or `file://` path, or `[$key, $passphrase]`). `None` when it
/// can't be had; `Err` for the array-shape ValueError.
pub(crate) fn key_from_value(
    vm: &mut Vm,
    a: &Args,
    v: &Value,
    want: Want,
    pass: Option<&[u8]>,
) -> R<Option<Rc<ossl::key::Key>>> {
    let v = v.deref();
    if let Value::Array(arr) = &v {
        let (k, p) = match (arr.get_int(0), arr.get_int(1)) {
            (Some(k), Some(p)) if arr.len() == 2 => (k.deref(), p.deref()),
            _ => {
                return vm.throw(
                    "ValueError",
                    "Key array must be of the form array(0 => key, 1 => phrase)",
                );
            }
        };
        let ps = vm.to_php_str(&p)?;
        return key_from_value(vm, a, &k, want, Some(ps.as_bytes()));
    }
    if let Value::Object(o) = &v {
        if let Some(k) = with_native::<KeyObj, _>(o, |k| k.0.clone()) {
            // Observed on PHP 8.5: key objects keep their kind.
            if want == Want::Private && !k.is_private() {
                vm.fwarn(a, "Supplied key param is a public key")?;
                return Ok(None);
            }
            if want == Want::Public && k.is_private() {
                vm.fwarn(a, "Don't know how to get public key from this private key")?;
                return Ok(None);
            }
            return Ok(Some(k));
        }
        if let Some(c) = with_native::<CertData, _>(o, |c| c.0.clone()) {
            if want == Want::Private {
                return Ok(None);
            }
            return Ok(c.public_key().ok().map(Rc::new));
        }
        if obj_class_is(vm, o, b"OpenSSLCertificateSigningRequest") {
            return Ok(None);
        }
    }
    if matches!(v, Value::Null | Value::Bool(_)) && want == Want::Private {
        // Converted to a string like any scalar.
    }
    let s = vm.to_php_str(&v)?;
    let Some(data) = arg_data(vm, a, s.as_bytes())? else {
        return Ok(None);
    };
    // Parsed keys are kept in a small cache keyed by a digest of the text
    // and passphrase: loading (and validating) an RSA key costs far more
    // than using it, and scripts pass the same PEM string or file again
    // and again.
    let ck = {
        let mut h =
            ossl::digest::Hasher::new(ossl::digest::Md::H(ossl::digest::HashAlgorithm::Sha256));
        h.update(&[want as u8]);
        h.update(&(data.len() as u64).to_le_bytes());
        h.update(&data);
        if let Some(p) = pass {
            h.update(&[1]);
            h.update(p);
        }
        h.finish()
    };
    if let Some(k) = vm
        .ext::<KeyCache>()
        .0
        .iter()
        .find(|(h, _)| *h == ck)
        .map(|(_, k)| k.clone())
    {
        return Ok(Some(k));
    }
    let r = load_key_text(&data, want, pass);
    if let Some(k) = &r {
        let c = &mut vm.ext::<KeyCache>().0;
        if c.len() >= 16 {
            c.remove(0);
        }
        c.push((ck, k.clone()));
    }
    Ok(r)
}

/// Recently parsed keys (see [`key_from_value`]).
#[derive(Default)]
struct KeyCache(Vec<(Vec<u8>, Rc<ossl::key::Key>)>);

fn load_key_text(data: &[u8], want: Want, pass: Option<&[u8]>) -> Option<Rc<ossl::key::Key>> {
    let r = match want {
        Want::Public => ossl::key::load_public_pem(data),
        Want::Private => ossl::key::load_private_pem(data, pass),
    };
    match r {
        Ok(k) => Some(Rc::new(k)),
        Err(ossl::key::LoadError::BadDecrypt | ossl::key::LoadError::NoPassphrase) => {
            // Observed on PHP 8.5: a wrong or missing passphrase queues
            // both "bad decrypt" errors.
            oerr::push(errs::EVP_BAD_DECRYPT);
            oerr::push(errs::PEM_BAD_DECRYPT);
            None
        }
        Err(_) => {
            push_decode_error(data);
            None
        }
    }
}

/// The errors OpenSSL 3's decoders queue for unusable key text (observed
/// on PHP 8.5: "DECODER routines::unsupported").
fn push_decode_error(data: &[u8]) {
    let _ = data;
    oerr::push(errs::DECODER_UNSUPPORTED);
}

/// The options array as a lookup for the request settings.
pub(crate) fn req_from_options(
    vm: &mut Vm,
    a: &Args,
    opts: Option<&PhpArray>,
) -> R<Result<ossl::req::Req, ossl::req::ReqError>> {
    let mut map: Vec<(String, ossl::req::Opt)> = Vec::new();
    if let Some(o) = opts {
        for (k, v) in o.iter() {
            let Some(kname) = (match k.to_owned() {
                Key::Str(s) => Some(s.to_string_lossy()),
                Key::Int(_) => None,
            }) else {
                continue;
            };
            let ov = match v.deref() {
                Value::Int(n) => ossl::req::Opt::Int(n),
                Value::Bool(b) => ossl::req::Opt::Bool(b),
                Value::Str(s) => ossl::req::Opt::Str(s.to_string_lossy()),
                Value::Float(f) => ossl::req::Opt::Int(f as i64),
                Value::Null => continue,
                _ => continue,
            };
            map.push((kname, ov));
        }
    }
    let _ = a;
    let get = |k: &str| map.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let read = |p: &str| std::fs::read(p).ok();
    let r = ossl::req::Req::new(&get, &read);
    if let Some(ossl::req::Opt::Str(p)) = get("config")
        && !std::path::Path::new(&p).exists()
    {
        oerr::push(errs::SYS_NOENT);
        oerr::push(errs::BIO_NO_SUCH_FILE);
    }
    if let Ok(req) = &r
        && req.oid_file_bad
    {
        // Observed on PHP 8.5 (test gh9339.phpt).
        vm.fwarn(a, "Path for oid_file option must be a valid file path")?;
    }
    Ok(r)
}

/// The by-reference argument's cell value setter.
pub(crate) fn set_out(a: &Args, i: usize, v: Value) {
    a.set_ref(i, v);
}

pub(crate) fn bin(v: Vec<u8>) -> Value {
    Value::Str(PhpStr::from(v))
}

pub(crate) fn str_val(s: &str) -> Value {
    Value::Str(PhpStr::from(s))
}

pub(crate) fn hex_lower(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

// ------------------------------------------------------------ misc

fn openssl_error_string(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(match oerr::pop() {
        Some(m) => Value::Str(PhpStr::from(m)),
        None => Value::Bool(false),
    })
}

/// openssl_get_cert_locations() (PHP manual): goro has no compiled-in
/// OpenSSL directory; it reports the conventional locations and the
/// openssl.cafile / openssl.capath settings.
fn openssl_get_cert_locations(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let mut arr = PhpArray::new();
    let put = |arr: &mut PhpArray, k: &str, v: Value| {
        arr.insert(Key::from(k), v);
    };
    put(
        &mut arr,
        "default_cert_file",
        str_val("/usr/lib/ssl/cert.pem"),
    );
    put(&mut arr, "default_cert_file_env", str_val("SSL_CERT_FILE"));
    put(&mut arr, "default_cert_dir", str_val("/usr/lib/ssl/certs"));
    put(&mut arr, "default_cert_dir_env", str_val("SSL_CERT_DIR"));
    put(
        &mut arr,
        "default_private_dir",
        str_val("/usr/lib/ssl/private"),
    );
    put(
        &mut arr,
        "default_default_cert_area",
        str_val("/usr/lib/ssl"),
    );
    let cafile = vm.get_ini(b"openssl.cafile").unwrap_or_else(PhpStr::empty);
    let capath = vm.get_ini(b"openssl.capath").unwrap_or_else(PhpStr::empty);
    put(&mut arr, "ini_cafile", Value::Str(cafile));
    put(&mut arr, "ini_capath", Value::Str(capath));
    Ok(Value::Array(arr))
}
