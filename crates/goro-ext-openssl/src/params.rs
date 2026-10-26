//! Parameter names and required counts of every openssl builtin (from
//! openssl.stub.php), used for named arguments and ArgumentCountErrors.

use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

pub const PARAMS: &[(&str, usize, &[&str])] = &[
    (
        "openssl_x509_export_to_file",
        2,
        &["certificate", "output_filename", "no_text"],
    ),
    (
        "openssl_x509_export",
        2,
        &["certificate", "output", "no_text"],
    ),
    (
        "openssl_x509_fingerprint",
        1,
        &["certificate", "digest_algo", "binary"],
    ),
    (
        "openssl_x509_check_private_key",
        2,
        &["certificate", "private_key"],
    ),
    ("openssl_x509_verify", 2, &["certificate", "public_key"]),
    ("openssl_x509_parse", 1, &["certificate", "short_names"]),
    (
        "openssl_x509_checkpurpose",
        2,
        &[
            "certificate",
            "purpose",
            "ca_info",
            "untrusted_certificates_file",
        ],
    ),
    ("openssl_x509_read", 1, &["certificate"]),
    ("openssl_x509_free", 1, &["certificate"]),
    (
        "openssl_pkcs12_export_to_file",
        4,
        &[
            "certificate",
            "output_filename",
            "private_key",
            "passphrase",
            "options",
        ],
    ),
    (
        "openssl_pkcs12_export",
        4,
        &[
            "certificate",
            "output",
            "private_key",
            "passphrase",
            "options",
        ],
    ),
    (
        "openssl_pkcs12_read",
        3,
        &["pkcs12", "certificates", "passphrase"],
    ),
    (
        "openssl_csr_export_to_file",
        2,
        &["csr", "output_filename", "no_text"],
    ),
    ("openssl_csr_export", 2, &["csr", "output", "no_text"]),
    (
        "openssl_csr_sign",
        4,
        &[
            "csr",
            "ca_certificate",
            "private_key",
            "days",
            "options",
            "serial",
            "serial_hex",
        ],
    ),
    (
        "openssl_csr_new",
        2,
        &[
            "distinguished_names",
            "private_key",
            "options",
            "extra_attributes",
        ],
    ),
    ("openssl_csr_get_subject", 1, &["csr", "short_names"]),
    ("openssl_csr_get_public_key", 1, &["csr", "short_names"]),
    ("openssl_pkey_new", 0, &["options"]),
    (
        "openssl_pkey_export_to_file",
        2,
        &["key", "output_filename", "passphrase", "options"],
    ),
    (
        "openssl_pkey_export",
        2,
        &["key", "output", "passphrase", "options"],
    ),
    ("openssl_pkey_get_public", 1, &["public_key"]),
    ("openssl_get_publickey", 1, &["public_key"]),
    ("openssl_pkey_free", 1, &["key"]),
    ("openssl_free_key", 1, &["key"]),
    (
        "openssl_pkey_get_private",
        1,
        &["private_key", "passphrase"],
    ),
    ("openssl_get_privatekey", 1, &["private_key", "passphrase"]),
    ("openssl_pkey_get_details", 1, &["key"]),
    (
        "openssl_pbkdf2",
        4,
        &[
            "password",
            "salt",
            "key_length",
            "iterations",
            "digest_algo",
        ],
    ),
    (
        "openssl_pkcs7_verify",
        2,
        &[
            "input_filename",
            "flags",
            "signers_certificates_filename",
            "ca_info",
            "untrusted_certificates_filename",
            "content",
            "output_filename",
        ],
    ),
    (
        "openssl_pkcs7_encrypt",
        4,
        &[
            "input_filename",
            "output_filename",
            "certificate",
            "headers",
            "flags",
            "cipher_algo",
        ],
    ),
    (
        "openssl_pkcs7_sign",
        5,
        &[
            "input_filename",
            "output_filename",
            "certificate",
            "private_key",
            "headers",
            "flags",
            "untrusted_certificates_filename",
        ],
    ),
    (
        "openssl_pkcs7_decrypt",
        3,
        &[
            "input_filename",
            "output_filename",
            "certificate",
            "private_key",
        ],
    ),
    ("openssl_pkcs7_read", 2, &["data", "certificates"]),
    (
        "openssl_cms_verify",
        2, // (zpp: "pl|p!ap!p!p!p!l", though $flags has a default)
        &[
            "input_filename",
            "flags",
            "certificates",
            "ca_info",
            "untrusted_certificates_filename",
            "content",
            "pk7",
            "sigfile",
            "encoding",
        ],
    ),
    (
        "openssl_cms_encrypt",
        4,
        &[
            "input_filename",
            "output_filename",
            "certificate",
            "headers",
            "flags",
            "encoding",
            "cipher_algo",
        ],
    ),
    (
        "openssl_cms_sign",
        5,
        &[
            "input_filename",
            "output_filename",
            "certificate",
            "private_key",
            "headers",
            "flags",
            "encoding",
            "untrusted_certificates_filename",
        ],
    ),
    (
        "openssl_cms_decrypt",
        3,
        &[
            "input_filename",
            "output_filename",
            "certificate",
            "private_key",
            "encoding",
        ],
    ),
    ("openssl_cms_read", 2, &["input_filename", "certificates"]),
    (
        "openssl_private_encrypt",
        3,
        &["data", "encrypted_data", "private_key", "padding"],
    ),
    (
        "openssl_private_decrypt",
        3,
        &[
            "data",
            "decrypted_data",
            "private_key",
            "padding",
            "digest_algo",
        ],
    ),
    (
        "openssl_public_encrypt",
        3,
        &[
            "data",
            "encrypted_data",
            "public_key",
            "padding",
            "digest_algo",
        ],
    ),
    (
        "openssl_public_decrypt",
        3,
        &["data", "decrypted_data", "public_key", "padding"],
    ),
    ("openssl_error_string", 0, &[]),
    (
        "openssl_sign",
        3,
        &["data", "signature", "private_key", "algorithm", "padding"],
    ),
    (
        "openssl_verify",
        3,
        &["data", "signature", "public_key", "algorithm", "padding"],
    ),
    (
        "openssl_seal",
        5,
        &[
            "data",
            "sealed_data",
            "encrypted_keys",
            "public_key",
            "cipher_algo",
            "iv",
        ],
    ),
    (
        "openssl_open",
        5,
        &[
            "data",
            "output",
            "encrypted_key",
            "private_key",
            "cipher_algo",
            "iv",
        ],
    ),
    ("openssl_get_md_methods", 0, &["aliases"]),
    ("openssl_get_cipher_methods", 0, &["aliases"]),
    ("openssl_get_curve_names", 0, &[]),
    ("openssl_digest", 2, &["data", "digest_algo", "binary"]),
    (
        "openssl_encrypt",
        3,
        &[
            "data",
            "cipher_algo",
            "passphrase",
            "options",
            "iv",
            "tag",
            "aad",
            "tag_length",
        ],
    ),
    (
        "openssl_decrypt",
        3,
        &[
            "data",
            "cipher_algo",
            "passphrase",
            "options",
            "iv",
            "tag",
            "aad",
        ],
    ),
    ("openssl_cipher_iv_length", 1, &["cipher_algo"]),
    ("openssl_cipher_key_length", 1, &["cipher_algo"]),
    ("openssl_dh_compute_key", 2, &["public_key", "private_key"]),
    (
        "openssl_pkey_derive",
        2,
        &["public_key", "private_key", "key_length"],
    ),
    (
        "openssl_random_pseudo_bytes",
        1,
        &["length", "strong_result"],
    ),
    (
        "openssl_spki_new",
        2,
        &["private_key", "challenge", "digest_algo"],
    ),
    ("openssl_spki_verify", 1, &["spki"]),
    ("openssl_spki_export", 1, &["spki"]),
    ("openssl_spki_export_challenge", 1, &["spki"]),
    ("openssl_get_cert_locations", 0, &[]),
    ("openssl_password_hash", 2, &["algo", "password", "options"]),
    ("openssl_password_verify", 3, &["algo", "password", "hash"]),
];

pub fn apply(vm: &mut Vm) {
    for (name, _, params) in PARAMS {
        let lower = name.as_bytes().to_ascii_lowercase();
        if vm.functions.contains_key(&lower) {
            vm.builtin_param_names.insert(
                lower,
                params.iter().map(|p| p.as_bytes().to_vec()).collect(),
            );
        }
    }
}

/// zend_wrong_parameters_count_error()
pub fn check_argc(vm: &mut Vm, args: &[Value], fname: &str) -> Result<(), VmError> {
    let Some((_, min, params)) = PARAMS.iter().find(|(n, _, _)| *n == fname) else {
        return Ok(());
    };
    let max = params.len();
    let given = args.len();
    if given >= *min && given <= max {
        return Ok(());
    }
    let (kind, n) = if min == &max {
        ("exactly", max)
    } else if given < *min {
        ("at least", *min)
    } else {
        ("at most", max)
    };
    let msg = format!(
        "{}() expects {} {} argument{}, {} given",
        fname,
        kind,
        n,
        if n == 1 { "" } else { "s" },
        given
    );
    Err(crate::zpp::throw(vm, b"ArgumentCountError", msg))
}
