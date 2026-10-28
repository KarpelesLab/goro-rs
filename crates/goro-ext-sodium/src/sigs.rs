//! Signatures of the sodium functions (observed on PHP 8.5 through
//! Reflection; the AEGIS functions, absent from the reference build's
//! libsodium 1.0.18, from the PHP manual), for the engine's parameter
//! parsing.

/// A parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Param {
    pub name: &'static str,
    /// The declared type as PHP prints it.
    pub ty: &'static str,
    /// The default value as PHP source (`None`: required).
    pub default: Option<&'static str>,
    /// Passed by reference.
    pub by_ref: bool,
    /// Marked `#[\SensitiveParameter]` (hidden in stack traces).
    pub sensitive: bool,
}

/// A function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FnSig {
    pub name: &'static str,
    pub params: &'static [Param],
    pub ret: &'static str,
}

const fn p(name: &'static str, ty: &'static str, sensitive: bool) -> Param {
    Param {
        name,
        ty,
        default: None,
        by_ref: false,
        sensitive,
    }
}

const fn r(name: &'static str, ty: &'static str, sensitive: bool) -> Param {
    Param {
        name,
        ty,
        default: None,
        by_ref: true,
        sensitive,
    }
}

const fn d(name: &'static str, ty: &'static str, default: &'static str, sensitive: bool) -> Param {
    Param {
        name,
        ty,
        default: Some(default),
        by_ref: false,
        sensitive,
    }
}

const fn f(name: &'static str, params: &'static [Param], ret: &'static str) -> FnSig {
    FnSig { name, params, ret }
}

/// Every sodium function, in PHP's registration order.
pub const SIGNATURES: &[FnSig] = &[
    f("sodium_crypto_aead_aes256gcm_is_available", &[], "bool"),
    f(
        "sodium_crypto_aead_aes256gcm_decrypt",
        &[
            p("ciphertext", "string", false),
            p("additional_data", "string", false),
            p("nonce", "string", false),
            p("key", "string", true),
        ],
        "string|false",
    ),
    f(
        "sodium_crypto_aead_aes256gcm_encrypt",
        &[
            p("message", "string", true),
            p("additional_data", "string", false),
            p("nonce", "string", false),
            p("key", "string", true),
        ],
        "string",
    ),
    f("sodium_crypto_aead_aes256gcm_keygen", &[], "string"),
    f(
        "sodium_crypto_aead_aegis128l_decrypt",
        &[
            p("ciphertext", "string", false),
            p("additional_data", "string", false),
            p("nonce", "string", false),
            p("key", "string", true),
        ],
        "string|false",
    ),
    f(
        "sodium_crypto_aead_aegis128l_encrypt",
        &[
            p("message", "string", true),
            p("additional_data", "string", false),
            p("nonce", "string", false),
            p("key", "string", true),
        ],
        "string",
    ),
    f("sodium_crypto_aead_aegis128l_keygen", &[], "string"),
    f(
        "sodium_crypto_aead_aegis256_decrypt",
        &[
            p("ciphertext", "string", false),
            p("additional_data", "string", false),
            p("nonce", "string", false),
            p("key", "string", true),
        ],
        "string|false",
    ),
    f(
        "sodium_crypto_aead_aegis256_encrypt",
        &[
            p("message", "string", true),
            p("additional_data", "string", false),
            p("nonce", "string", false),
            p("key", "string", true),
        ],
        "string",
    ),
    f("sodium_crypto_aead_aegis256_keygen", &[], "string"),
    f(
        "sodium_crypto_aead_chacha20poly1305_decrypt",
        &[
            p("ciphertext", "string", false),
            p("additional_data", "string", false),
            p("nonce", "string", false),
            p("key", "string", true),
        ],
        "string|false",
    ),
    f(
        "sodium_crypto_aead_chacha20poly1305_encrypt",
        &[
            p("message", "string", true),
            p("additional_data", "string", false),
            p("nonce", "string", false),
            p("key", "string", true),
        ],
        "string",
    ),
    f("sodium_crypto_aead_chacha20poly1305_keygen", &[], "string"),
    f(
        "sodium_crypto_aead_chacha20poly1305_ietf_decrypt",
        &[
            p("ciphertext", "string", false),
            p("additional_data", "string", false),
            p("nonce", "string", false),
            p("key", "string", true),
        ],
        "string|false",
    ),
    f(
        "sodium_crypto_aead_chacha20poly1305_ietf_encrypt",
        &[
            p("message", "string", true),
            p("additional_data", "string", false),
            p("nonce", "string", false),
            p("key", "string", true),
        ],
        "string",
    ),
    f(
        "sodium_crypto_aead_chacha20poly1305_ietf_keygen",
        &[],
        "string",
    ),
    f(
        "sodium_crypto_aead_xchacha20poly1305_ietf_decrypt",
        &[
            p("ciphertext", "string", false),
            p("additional_data", "string", false),
            p("nonce", "string", false),
            p("key", "string", true),
        ],
        "string|false",
    ),
    f(
        "sodium_crypto_aead_xchacha20poly1305_ietf_keygen",
        &[],
        "string",
    ),
    f(
        "sodium_crypto_aead_xchacha20poly1305_ietf_encrypt",
        &[
            p("message", "string", true),
            p("additional_data", "string", false),
            p("nonce", "string", false),
            p("key", "string", true),
        ],
        "string",
    ),
    f(
        "sodium_crypto_auth",
        &[p("message", "string", false), p("key", "string", true)],
        "string",
    ),
    f("sodium_crypto_auth_keygen", &[], "string"),
    f(
        "sodium_crypto_auth_verify",
        &[
            p("mac", "string", false),
            p("message", "string", false),
            p("key", "string", true),
        ],
        "bool",
    ),
    f(
        "sodium_crypto_box",
        &[
            p("message", "string", true),
            p("nonce", "string", false),
            p("key_pair", "string", true),
        ],
        "string",
    ),
    f("sodium_crypto_box_keypair", &[], "string"),
    f(
        "sodium_crypto_box_seed_keypair",
        &[p("seed", "string", true)],
        "string",
    ),
    f(
        "sodium_crypto_box_keypair_from_secretkey_and_publickey",
        &[
            p("secret_key", "string", true),
            p("public_key", "string", false),
        ],
        "string",
    ),
    f(
        "sodium_crypto_box_open",
        &[
            p("ciphertext", "string", false),
            p("nonce", "string", false),
            p("key_pair", "string", true),
        ],
        "string|false",
    ),
    f(
        "sodium_crypto_box_publickey",
        &[p("key_pair", "string", true)],
        "string",
    ),
    f(
        "sodium_crypto_box_publickey_from_secretkey",
        &[p("secret_key", "string", true)],
        "string",
    ),
    f(
        "sodium_crypto_box_seal",
        &[
            p("message", "string", true),
            p("public_key", "string", false),
        ],
        "string",
    ),
    f(
        "sodium_crypto_box_seal_open",
        &[
            p("ciphertext", "string", false),
            p("key_pair", "string", true),
        ],
        "string|false",
    ),
    f(
        "sodium_crypto_box_secretkey",
        &[p("key_pair", "string", true)],
        "string",
    ),
    f(
        "sodium_crypto_core_ristretto255_add",
        &[p("p", "string", false), p("q", "string", false)],
        "string",
    ),
    f(
        "sodium_crypto_core_ristretto255_from_hash",
        &[p("s", "string", false)],
        "string",
    ),
    f(
        "sodium_crypto_core_ristretto255_is_valid_point",
        &[p("s", "string", false)],
        "bool",
    ),
    f("sodium_crypto_core_ristretto255_random", &[], "string"),
    f(
        "sodium_crypto_core_ristretto255_scalar_add",
        &[p("x", "string", false), p("y", "string", false)],
        "string",
    ),
    f(
        "sodium_crypto_core_ristretto255_scalar_complement",
        &[p("s", "string", false)],
        "string",
    ),
    f(
        "sodium_crypto_core_ristretto255_scalar_invert",
        &[p("s", "string", false)],
        "string",
    ),
    f(
        "sodium_crypto_core_ristretto255_scalar_mul",
        &[p("x", "string", false), p("y", "string", false)],
        "string",
    ),
    f(
        "sodium_crypto_core_ristretto255_scalar_negate",
        &[p("s", "string", false)],
        "string",
    ),
    f(
        "sodium_crypto_core_ristretto255_scalar_random",
        &[],
        "string",
    ),
    f(
        "sodium_crypto_core_ristretto255_scalar_reduce",
        &[p("s", "string", false)],
        "string",
    ),
    f(
        "sodium_crypto_core_ristretto255_scalar_sub",
        &[p("x", "string", false), p("y", "string", false)],
        "string",
    ),
    f(
        "sodium_crypto_core_ristretto255_sub",
        &[p("p", "string", false), p("q", "string", false)],
        "string",
    ),
    f("sodium_crypto_kx_keypair", &[], "string"),
    f(
        "sodium_crypto_kx_publickey",
        &[p("key_pair", "string", true)],
        "string",
    ),
    f(
        "sodium_crypto_kx_secretkey",
        &[p("key_pair", "string", true)],
        "string",
    ),
    f(
        "sodium_crypto_kx_seed_keypair",
        &[p("seed", "string", true)],
        "string",
    ),
    f(
        "sodium_crypto_kx_client_session_keys",
        &[
            p("client_key_pair", "string", true),
            p("server_key", "string", false),
        ],
        "array",
    ),
    f(
        "sodium_crypto_kx_server_session_keys",
        &[
            p("server_key_pair", "string", true),
            p("client_key", "string", false),
        ],
        "array",
    ),
    f(
        "sodium_crypto_generichash",
        &[
            p("message", "string", false),
            d("key", "string", "''", true),
            d("length", "int", "32", false),
        ],
        "string",
    ),
    f("sodium_crypto_generichash_keygen", &[], "string"),
    f(
        "sodium_crypto_generichash_init",
        &[
            d("key", "string", "''", true),
            d("length", "int", "32", false),
        ],
        "string",
    ),
    f(
        "sodium_crypto_generichash_update",
        &[r("state", "string", false), p("message", "string", false)],
        "true",
    ),
    f(
        "sodium_crypto_generichash_final",
        &[r("state", "string", false), d("length", "int", "32", false)],
        "string",
    ),
    f(
        "sodium_crypto_kdf_derive_from_key",
        &[
            p("subkey_length", "int", false),
            p("subkey_id", "int", false),
            p("context", "string", false),
            p("key", "string", true),
        ],
        "string",
    ),
    f("sodium_crypto_kdf_keygen", &[], "string"),
    f(
        "sodium_crypto_pwhash",
        &[
            p("length", "int", false),
            p("password", "string", true),
            p("salt", "string", false),
            p("opslimit", "int", false),
            p("memlimit", "int", false),
            d("algo", "int", "2", false),
        ],
        "string",
    ),
    f(
        "sodium_crypto_pwhash_str",
        &[
            p("password", "string", true),
            p("opslimit", "int", false),
            p("memlimit", "int", false),
        ],
        "string",
    ),
    f(
        "sodium_crypto_pwhash_str_verify",
        &[p("hash", "string", false), p("password", "string", true)],
        "bool",
    ),
    f(
        "sodium_crypto_pwhash_str_needs_rehash",
        &[
            p("password", "string", false),
            p("opslimit", "int", false),
            p("memlimit", "int", false),
        ],
        "bool",
    ),
    f(
        "sodium_crypto_pwhash_scryptsalsa208sha256",
        &[
            p("length", "int", false),
            p("password", "string", true),
            p("salt", "string", false),
            p("opslimit", "int", false),
            p("memlimit", "int", false),
        ],
        "string",
    ),
    f(
        "sodium_crypto_pwhash_scryptsalsa208sha256_str",
        &[
            p("password", "string", true),
            p("opslimit", "int", false),
            p("memlimit", "int", false),
        ],
        "string",
    ),
    f(
        "sodium_crypto_pwhash_scryptsalsa208sha256_str_verify",
        &[p("hash", "string", false), p("password", "string", true)],
        "bool",
    ),
    f(
        "sodium_crypto_scalarmult",
        &[p("n", "string", false), p("p", "string", false)],
        "string",
    ),
    f(
        "sodium_crypto_scalarmult_ristretto255",
        &[p("n", "string", false), p("p", "string", false)],
        "string",
    ),
    f(
        "sodium_crypto_scalarmult_ristretto255_base",
        &[p("n", "string", false)],
        "string",
    ),
    f(
        "sodium_crypto_secretbox",
        &[
            p("message", "string", true),
            p("nonce", "string", false),
            p("key", "string", true),
        ],
        "string",
    ),
    f("sodium_crypto_secretbox_keygen", &[], "string"),
    f(
        "sodium_crypto_secretbox_open",
        &[
            p("ciphertext", "string", false),
            p("nonce", "string", false),
            p("key", "string", true),
        ],
        "string|false",
    ),
    f(
        "sodium_crypto_secretstream_xchacha20poly1305_keygen",
        &[],
        "string",
    ),
    f(
        "sodium_crypto_secretstream_xchacha20poly1305_init_push",
        &[p("key", "string", true)],
        "array",
    ),
    f(
        "sodium_crypto_secretstream_xchacha20poly1305_push",
        &[
            r("state", "string", false),
            p("message", "string", true),
            d("additional_data", "string", "''", false),
            d("tag", "int", "0", false),
        ],
        "string",
    ),
    f(
        "sodium_crypto_secretstream_xchacha20poly1305_init_pull",
        &[p("header", "string", false), p("key", "string", true)],
        "string",
    ),
    f(
        "sodium_crypto_secretstream_xchacha20poly1305_pull",
        &[
            r("state", "string", false),
            p("ciphertext", "string", false),
            d("additional_data", "string", "''", false),
        ],
        "array|false",
    ),
    f(
        "sodium_crypto_secretstream_xchacha20poly1305_rekey",
        &[r("state", "string", false)],
        "void",
    ),
    f(
        "sodium_crypto_shorthash",
        &[p("message", "string", false), p("key", "string", true)],
        "string",
    ),
    f("sodium_crypto_shorthash_keygen", &[], "string"),
    f(
        "sodium_crypto_sign",
        &[
            p("message", "string", false),
            p("secret_key", "string", true),
        ],
        "string",
    ),
    f(
        "sodium_crypto_sign_detached",
        &[
            p("message", "string", false),
            p("secret_key", "string", true),
        ],
        "string",
    ),
    f(
        "sodium_crypto_sign_ed25519_pk_to_curve25519",
        &[p("public_key", "string", false)],
        "string",
    ),
    f(
        "sodium_crypto_sign_ed25519_sk_to_curve25519",
        &[p("secret_key", "string", true)],
        "string",
    ),
    f("sodium_crypto_sign_keypair", &[], "string"),
    f(
        "sodium_crypto_sign_keypair_from_secretkey_and_publickey",
        &[
            p("secret_key", "string", true),
            p("public_key", "string", false),
        ],
        "string",
    ),
    f(
        "sodium_crypto_sign_open",
        &[
            p("signed_message", "string", false),
            p("public_key", "string", false),
        ],
        "string|false",
    ),
    f(
        "sodium_crypto_sign_publickey",
        &[p("key_pair", "string", true)],
        "string",
    ),
    f(
        "sodium_crypto_sign_secretkey",
        &[p("key_pair", "string", true)],
        "string",
    ),
    f(
        "sodium_crypto_sign_publickey_from_secretkey",
        &[p("secret_key", "string", true)],
        "string",
    ),
    f(
        "sodium_crypto_sign_seed_keypair",
        &[p("seed", "string", true)],
        "string",
    ),
    f(
        "sodium_crypto_sign_verify_detached",
        &[
            p("signature", "string", false),
            p("message", "string", false),
            p("public_key", "string", false),
        ],
        "bool",
    ),
    f(
        "sodium_crypto_stream",
        &[
            p("length", "int", false),
            p("nonce", "string", false),
            p("key", "string", true),
        ],
        "string",
    ),
    f("sodium_crypto_stream_keygen", &[], "string"),
    f(
        "sodium_crypto_stream_xor",
        &[
            p("message", "string", true),
            p("nonce", "string", false),
            p("key", "string", true),
        ],
        "string",
    ),
    f(
        "sodium_crypto_stream_xchacha20",
        &[
            p("length", "int", false),
            p("nonce", "string", false),
            p("key", "string", true),
        ],
        "string",
    ),
    f("sodium_crypto_stream_xchacha20_keygen", &[], "string"),
    f(
        "sodium_crypto_stream_xchacha20_xor",
        &[
            p("message", "string", true),
            p("nonce", "string", false),
            p("key", "string", true),
        ],
        "string",
    ),
    f(
        "sodium_crypto_stream_xchacha20_xor_ic",
        &[
            p("message", "string", true),
            p("nonce", "string", false),
            p("counter", "int", false),
            p("key", "string", true),
        ],
        "string",
    ),
    f(
        "sodium_add",
        &[r("string1", "string", false), p("string2", "string", false)],
        "void",
    ),
    f(
        "sodium_compare",
        &[p("string1", "string", true), p("string2", "string", true)],
        "int",
    ),
    f("sodium_increment", &[r("string", "string", false)], "void"),
    f(
        "sodium_memcmp",
        &[p("string1", "string", true), p("string2", "string", true)],
        "int",
    ),
    f("sodium_memzero", &[r("string", "string", true)], "void"),
    f(
        "sodium_pad",
        &[p("string", "string", true), p("block_size", "int", false)],
        "string",
    ),
    f(
        "sodium_unpad",
        &[p("string", "string", true), p("block_size", "int", false)],
        "string",
    ),
    f("sodium_bin2hex", &[p("string", "string", true)], "string"),
    f(
        "sodium_hex2bin",
        &[
            p("string", "string", true),
            d("ignore", "string", "''", false),
        ],
        "string",
    ),
    f(
        "sodium_bin2base64",
        &[p("string", "string", true), p("id", "int", false)],
        "string",
    ),
    f(
        "sodium_base642bin",
        &[
            p("string", "string", true),
            p("id", "int", false),
            d("ignore", "string", "''", false),
        ],
        "string",
    ),
    f(
        "sodium_crypto_scalarmult_base",
        &[p("secret_key", "string", true)],
        "string",
    ),
];
