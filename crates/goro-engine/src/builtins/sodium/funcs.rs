//! The sodium_*() handlers, one per entry of goro-ext-sodium's
//! `SIGNATURES` table (written from it mechanically).

use super::*;

pub(super) const FUNCTIONS: &[(&str, NativeFn)] = &[
    (
        "function sodium_crypto_aead_aes256gcm_is_available(): bool",
        sodium_crypto_aead_aes256gcm_is_available,
    ),
    (
        "function sodium_crypto_aead_aes256gcm_decrypt(string $ciphertext, string $additional_data, string $nonce, #[\\SensitiveParameter] string $key): string|false",
        sodium_crypto_aead_aes256gcm_decrypt,
    ),
    (
        "function sodium_crypto_aead_aes256gcm_encrypt(#[\\SensitiveParameter] string $message, string $additional_data, string $nonce, #[\\SensitiveParameter] string $key): string",
        sodium_crypto_aead_aes256gcm_encrypt,
    ),
    (
        "function sodium_crypto_aead_aes256gcm_keygen(): string",
        sodium_crypto_aead_aes256gcm_keygen,
    ),
    (
        "function sodium_crypto_aead_aegis128l_decrypt(string $ciphertext, string $additional_data, string $nonce, #[\\SensitiveParameter] string $key): string|false",
        sodium_crypto_aead_aegis128l_decrypt,
    ),
    (
        "function sodium_crypto_aead_aegis128l_encrypt(#[\\SensitiveParameter] string $message, string $additional_data, string $nonce, #[\\SensitiveParameter] string $key): string",
        sodium_crypto_aead_aegis128l_encrypt,
    ),
    (
        "function sodium_crypto_aead_aegis128l_keygen(): string",
        sodium_crypto_aead_aegis128l_keygen,
    ),
    (
        "function sodium_crypto_aead_aegis256_decrypt(string $ciphertext, string $additional_data, string $nonce, #[\\SensitiveParameter] string $key): string|false",
        sodium_crypto_aead_aegis256_decrypt,
    ),
    (
        "function sodium_crypto_aead_aegis256_encrypt(#[\\SensitiveParameter] string $message, string $additional_data, string $nonce, #[\\SensitiveParameter] string $key): string",
        sodium_crypto_aead_aegis256_encrypt,
    ),
    (
        "function sodium_crypto_aead_aegis256_keygen(): string",
        sodium_crypto_aead_aegis256_keygen,
    ),
    (
        "function sodium_crypto_aead_chacha20poly1305_decrypt(string $ciphertext, string $additional_data, string $nonce, #[\\SensitiveParameter] string $key): string|false",
        sodium_crypto_aead_chacha20poly1305_decrypt,
    ),
    (
        "function sodium_crypto_aead_chacha20poly1305_encrypt(#[\\SensitiveParameter] string $message, string $additional_data, string $nonce, #[\\SensitiveParameter] string $key): string",
        sodium_crypto_aead_chacha20poly1305_encrypt,
    ),
    (
        "function sodium_crypto_aead_chacha20poly1305_keygen(): string",
        sodium_crypto_aead_chacha20poly1305_keygen,
    ),
    (
        "function sodium_crypto_aead_chacha20poly1305_ietf_decrypt(string $ciphertext, string $additional_data, string $nonce, #[\\SensitiveParameter] string $key): string|false",
        sodium_crypto_aead_chacha20poly1305_ietf_decrypt,
    ),
    (
        "function sodium_crypto_aead_chacha20poly1305_ietf_encrypt(#[\\SensitiveParameter] string $message, string $additional_data, string $nonce, #[\\SensitiveParameter] string $key): string",
        sodium_crypto_aead_chacha20poly1305_ietf_encrypt,
    ),
    (
        "function sodium_crypto_aead_chacha20poly1305_ietf_keygen(): string",
        sodium_crypto_aead_chacha20poly1305_ietf_keygen,
    ),
    (
        "function sodium_crypto_aead_xchacha20poly1305_ietf_decrypt(string $ciphertext, string $additional_data, string $nonce, #[\\SensitiveParameter] string $key): string|false",
        sodium_crypto_aead_xchacha20poly1305_ietf_decrypt,
    ),
    (
        "function sodium_crypto_aead_xchacha20poly1305_ietf_keygen(): string",
        sodium_crypto_aead_xchacha20poly1305_ietf_keygen,
    ),
    (
        "function sodium_crypto_aead_xchacha20poly1305_ietf_encrypt(#[\\SensitiveParameter] string $message, string $additional_data, string $nonce, #[\\SensitiveParameter] string $key): string",
        sodium_crypto_aead_xchacha20poly1305_ietf_encrypt,
    ),
    (
        "function sodium_crypto_auth(string $message, #[\\SensitiveParameter] string $key): string",
        sodium_crypto_auth,
    ),
    (
        "function sodium_crypto_auth_keygen(): string",
        sodium_crypto_auth_keygen,
    ),
    (
        "function sodium_crypto_auth_verify(string $mac, string $message, #[\\SensitiveParameter] string $key): bool",
        sodium_crypto_auth_verify,
    ),
    (
        "function sodium_crypto_box(#[\\SensitiveParameter] string $message, string $nonce, #[\\SensitiveParameter] string $key_pair): string",
        sodium_crypto_box,
    ),
    (
        "function sodium_crypto_box_keypair(): string",
        sodium_crypto_box_keypair,
    ),
    (
        "function sodium_crypto_box_seed_keypair(#[\\SensitiveParameter] string $seed): string",
        sodium_crypto_box_seed_keypair,
    ),
    (
        "function sodium_crypto_box_keypair_from_secretkey_and_publickey(#[\\SensitiveParameter] string $secret_key, string $public_key): string",
        sodium_crypto_box_keypair_from_secretkey_and_publickey,
    ),
    (
        "function sodium_crypto_box_open(string $ciphertext, string $nonce, #[\\SensitiveParameter] string $key_pair): string|false",
        sodium_crypto_box_open,
    ),
    (
        "function sodium_crypto_box_publickey(#[\\SensitiveParameter] string $key_pair): string",
        sodium_crypto_box_publickey,
    ),
    (
        "function sodium_crypto_box_publickey_from_secretkey(#[\\SensitiveParameter] string $secret_key): string",
        sodium_crypto_box_publickey_from_secretkey,
    ),
    (
        "function sodium_crypto_box_seal(#[\\SensitiveParameter] string $message, string $public_key): string",
        sodium_crypto_box_seal,
    ),
    (
        "function sodium_crypto_box_seal_open(string $ciphertext, #[\\SensitiveParameter] string $key_pair): string|false",
        sodium_crypto_box_seal_open,
    ),
    (
        "function sodium_crypto_box_secretkey(#[\\SensitiveParameter] string $key_pair): string",
        sodium_crypto_box_secretkey,
    ),
    (
        "function sodium_crypto_core_ristretto255_add(string $p, string $q): string",
        sodium_crypto_core_ristretto255_add,
    ),
    (
        "function sodium_crypto_core_ristretto255_from_hash(string $s): string",
        sodium_crypto_core_ristretto255_from_hash,
    ),
    (
        "function sodium_crypto_core_ristretto255_is_valid_point(string $s): bool",
        sodium_crypto_core_ristretto255_is_valid_point,
    ),
    (
        "function sodium_crypto_core_ristretto255_random(): string",
        sodium_crypto_core_ristretto255_random,
    ),
    (
        "function sodium_crypto_core_ristretto255_scalar_add(string $x, string $y): string",
        sodium_crypto_core_ristretto255_scalar_add,
    ),
    (
        "function sodium_crypto_core_ristretto255_scalar_complement(string $s): string",
        sodium_crypto_core_ristretto255_scalar_complement,
    ),
    (
        "function sodium_crypto_core_ristretto255_scalar_invert(string $s): string",
        sodium_crypto_core_ristretto255_scalar_invert,
    ),
    (
        "function sodium_crypto_core_ristretto255_scalar_mul(string $x, string $y): string",
        sodium_crypto_core_ristretto255_scalar_mul,
    ),
    (
        "function sodium_crypto_core_ristretto255_scalar_negate(string $s): string",
        sodium_crypto_core_ristretto255_scalar_negate,
    ),
    (
        "function sodium_crypto_core_ristretto255_scalar_random(): string",
        sodium_crypto_core_ristretto255_scalar_random,
    ),
    (
        "function sodium_crypto_core_ristretto255_scalar_reduce(string $s): string",
        sodium_crypto_core_ristretto255_scalar_reduce,
    ),
    (
        "function sodium_crypto_core_ristretto255_scalar_sub(string $x, string $y): string",
        sodium_crypto_core_ristretto255_scalar_sub,
    ),
    (
        "function sodium_crypto_core_ristretto255_sub(string $p, string $q): string",
        sodium_crypto_core_ristretto255_sub,
    ),
    (
        "function sodium_crypto_kx_keypair(): string",
        sodium_crypto_kx_keypair,
    ),
    (
        "function sodium_crypto_kx_publickey(#[\\SensitiveParameter] string $key_pair): string",
        sodium_crypto_kx_publickey,
    ),
    (
        "function sodium_crypto_kx_secretkey(#[\\SensitiveParameter] string $key_pair): string",
        sodium_crypto_kx_secretkey,
    ),
    (
        "function sodium_crypto_kx_seed_keypair(#[\\SensitiveParameter] string $seed): string",
        sodium_crypto_kx_seed_keypair,
    ),
    (
        "function sodium_crypto_kx_client_session_keys(#[\\SensitiveParameter] string $client_key_pair, string $server_key): array",
        sodium_crypto_kx_client_session_keys,
    ),
    (
        "function sodium_crypto_kx_server_session_keys(#[\\SensitiveParameter] string $server_key_pair, string $client_key): array",
        sodium_crypto_kx_server_session_keys,
    ),
    (
        "function sodium_crypto_generichash(string $message, #[\\SensitiveParameter] string $key = '', int $length = 32): string",
        sodium_crypto_generichash,
    ),
    (
        "function sodium_crypto_generichash_keygen(): string",
        sodium_crypto_generichash_keygen,
    ),
    (
        "function sodium_crypto_generichash_init(#[\\SensitiveParameter] string $key = '', int $length = 32): string",
        sodium_crypto_generichash_init,
    ),
    (
        "function sodium_crypto_generichash_update(string &$state, string $message): true",
        sodium_crypto_generichash_update,
    ),
    (
        "function sodium_crypto_generichash_final(string &$state, int $length = 32): string",
        sodium_crypto_generichash_final,
    ),
    (
        "function sodium_crypto_kdf_derive_from_key(int $subkey_length, int $subkey_id, string $context, #[\\SensitiveParameter] string $key): string",
        sodium_crypto_kdf_derive_from_key,
    ),
    (
        "function sodium_crypto_kdf_keygen(): string",
        sodium_crypto_kdf_keygen,
    ),
    (
        "function sodium_crypto_pwhash(int $length, #[\\SensitiveParameter] string $password, string $salt, int $opslimit, int $memlimit, int $algo = 2): string",
        sodium_crypto_pwhash,
    ),
    (
        "function sodium_crypto_pwhash_str(#[\\SensitiveParameter] string $password, int $opslimit, int $memlimit): string",
        sodium_crypto_pwhash_str,
    ),
    (
        "function sodium_crypto_pwhash_str_verify(string $hash, #[\\SensitiveParameter] string $password): bool",
        sodium_crypto_pwhash_str_verify,
    ),
    (
        "function sodium_crypto_pwhash_str_needs_rehash(string $password, int $opslimit, int $memlimit): bool",
        sodium_crypto_pwhash_str_needs_rehash,
    ),
    (
        "function sodium_crypto_pwhash_scryptsalsa208sha256(int $length, #[\\SensitiveParameter] string $password, string $salt, int $opslimit, int $memlimit): string",
        sodium_crypto_pwhash_scryptsalsa208sha256,
    ),
    (
        "function sodium_crypto_pwhash_scryptsalsa208sha256_str(#[\\SensitiveParameter] string $password, int $opslimit, int $memlimit): string",
        sodium_crypto_pwhash_scryptsalsa208sha256_str,
    ),
    (
        "function sodium_crypto_pwhash_scryptsalsa208sha256_str_verify(string $hash, #[\\SensitiveParameter] string $password): bool",
        sodium_crypto_pwhash_scryptsalsa208sha256_str_verify,
    ),
    (
        "function sodium_crypto_scalarmult(string $n, string $p): string",
        sodium_crypto_scalarmult,
    ),
    (
        "function sodium_crypto_scalarmult_ristretto255(string $n, string $p): string",
        sodium_crypto_scalarmult_ristretto255,
    ),
    (
        "function sodium_crypto_scalarmult_ristretto255_base(string $n): string",
        sodium_crypto_scalarmult_ristretto255_base,
    ),
    (
        "function sodium_crypto_secretbox(#[\\SensitiveParameter] string $message, string $nonce, #[\\SensitiveParameter] string $key): string",
        sodium_crypto_secretbox,
    ),
    (
        "function sodium_crypto_secretbox_keygen(): string",
        sodium_crypto_secretbox_keygen,
    ),
    (
        "function sodium_crypto_secretbox_open(string $ciphertext, string $nonce, #[\\SensitiveParameter] string $key): string|false",
        sodium_crypto_secretbox_open,
    ),
    (
        "function sodium_crypto_secretstream_xchacha20poly1305_keygen(): string",
        sodium_crypto_secretstream_xchacha20poly1305_keygen,
    ),
    (
        "function sodium_crypto_secretstream_xchacha20poly1305_init_push(#[\\SensitiveParameter] string $key): array",
        sodium_crypto_secretstream_xchacha20poly1305_init_push,
    ),
    (
        "function sodium_crypto_secretstream_xchacha20poly1305_push(string &$state, #[\\SensitiveParameter] string $message, string $additional_data = '', int $tag = 0): string",
        sodium_crypto_secretstream_xchacha20poly1305_push,
    ),
    (
        "function sodium_crypto_secretstream_xchacha20poly1305_init_pull(string $header, #[\\SensitiveParameter] string $key): string",
        sodium_crypto_secretstream_xchacha20poly1305_init_pull,
    ),
    (
        "function sodium_crypto_secretstream_xchacha20poly1305_pull(string &$state, string $ciphertext, string $additional_data = ''): array|false",
        sodium_crypto_secretstream_xchacha20poly1305_pull,
    ),
    (
        "function sodium_crypto_secretstream_xchacha20poly1305_rekey(string &$state): void",
        sodium_crypto_secretstream_xchacha20poly1305_rekey,
    ),
    (
        "function sodium_crypto_shorthash(string $message, #[\\SensitiveParameter] string $key): string",
        sodium_crypto_shorthash,
    ),
    (
        "function sodium_crypto_shorthash_keygen(): string",
        sodium_crypto_shorthash_keygen,
    ),
    (
        "function sodium_crypto_sign(string $message, #[\\SensitiveParameter] string $secret_key): string",
        sodium_crypto_sign,
    ),
    (
        "function sodium_crypto_sign_detached(string $message, #[\\SensitiveParameter] string $secret_key): string",
        sodium_crypto_sign_detached,
    ),
    (
        "function sodium_crypto_sign_ed25519_pk_to_curve25519(string $public_key): string",
        sodium_crypto_sign_ed25519_pk_to_curve25519,
    ),
    (
        "function sodium_crypto_sign_ed25519_sk_to_curve25519(#[\\SensitiveParameter] string $secret_key): string",
        sodium_crypto_sign_ed25519_sk_to_curve25519,
    ),
    (
        "function sodium_crypto_sign_keypair(): string",
        sodium_crypto_sign_keypair,
    ),
    (
        "function sodium_crypto_sign_keypair_from_secretkey_and_publickey(#[\\SensitiveParameter] string $secret_key, string $public_key): string",
        sodium_crypto_sign_keypair_from_secretkey_and_publickey,
    ),
    (
        "function sodium_crypto_sign_open(string $signed_message, string $public_key): string|false",
        sodium_crypto_sign_open,
    ),
    (
        "function sodium_crypto_sign_publickey(#[\\SensitiveParameter] string $key_pair): string",
        sodium_crypto_sign_publickey,
    ),
    (
        "function sodium_crypto_sign_secretkey(#[\\SensitiveParameter] string $key_pair): string",
        sodium_crypto_sign_secretkey,
    ),
    (
        "function sodium_crypto_sign_publickey_from_secretkey(#[\\SensitiveParameter] string $secret_key): string",
        sodium_crypto_sign_publickey_from_secretkey,
    ),
    (
        "function sodium_crypto_sign_seed_keypair(#[\\SensitiveParameter] string $seed): string",
        sodium_crypto_sign_seed_keypair,
    ),
    (
        "function sodium_crypto_sign_verify_detached(string $signature, string $message, string $public_key): bool",
        sodium_crypto_sign_verify_detached,
    ),
    (
        "function sodium_crypto_stream(int $length, string $nonce, #[\\SensitiveParameter] string $key): string",
        sodium_crypto_stream,
    ),
    (
        "function sodium_crypto_stream_keygen(): string",
        sodium_crypto_stream_keygen,
    ),
    (
        "function sodium_crypto_stream_xor(#[\\SensitiveParameter] string $message, string $nonce, #[\\SensitiveParameter] string $key): string",
        sodium_crypto_stream_xor,
    ),
    (
        "function sodium_crypto_stream_xchacha20(int $length, string $nonce, #[\\SensitiveParameter] string $key): string",
        sodium_crypto_stream_xchacha20,
    ),
    (
        "function sodium_crypto_stream_xchacha20_keygen(): string",
        sodium_crypto_stream_xchacha20_keygen,
    ),
    (
        "function sodium_crypto_stream_xchacha20_xor(#[\\SensitiveParameter] string $message, string $nonce, #[\\SensitiveParameter] string $key): string",
        sodium_crypto_stream_xchacha20_xor,
    ),
    (
        "function sodium_crypto_stream_xchacha20_xor_ic(#[\\SensitiveParameter] string $message, string $nonce, int $counter, #[\\SensitiveParameter] string $key): string",
        sodium_crypto_stream_xchacha20_xor_ic,
    ),
    (
        "function sodium_add(string &$string1, string $string2): void",
        sodium_add,
    ),
    (
        "function sodium_compare(#[\\SensitiveParameter] string $string1, #[\\SensitiveParameter] string $string2): int",
        sodium_compare,
    ),
    (
        "function sodium_increment(string &$string): void",
        sodium_increment,
    ),
    (
        "function sodium_memcmp(#[\\SensitiveParameter] string $string1, #[\\SensitiveParameter] string $string2): int",
        sodium_memcmp,
    ),
    (
        "function sodium_memzero(#[\\SensitiveParameter] string &$string): void",
        sodium_memzero,
    ),
    (
        "function sodium_pad(#[\\SensitiveParameter] string $string, int $block_size): string",
        sodium_pad,
    ),
    (
        "function sodium_unpad(#[\\SensitiveParameter] string $string, int $block_size): string",
        sodium_unpad,
    ),
    (
        "function sodium_bin2hex(#[\\SensitiveParameter] string $string): string",
        sodium_bin2hex,
    ),
    (
        "function sodium_hex2bin(#[\\SensitiveParameter] string $string, string $ignore = ''): string",
        sodium_hex2bin,
    ),
    (
        "function sodium_bin2base64(#[\\SensitiveParameter] string $string, int $id): string",
        sodium_bin2base64,
    ),
    (
        "function sodium_base642bin(#[\\SensitiveParameter] string $string, int $id, string $ignore = ''): string",
        sodium_base642bin,
    ),
    (
        "function sodium_crypto_scalarmult_base(#[\\SensitiveParameter] string $secret_key): string",
        sodium_crypto_scalarmult_base,
    ),
];

fn sodium_crypto_aead_aes256gcm_is_available(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_aead_aes256gcm_is_available();
    Ok(Value::Bool(r))
}
fn sodium_crypto_aead_aes256gcm_decrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    let p3 = a.str(3);
    set_limit(vm);
    let r = s::sodium_crypto_aead_aes256gcm_decrypt(
        p0.as_bytes(),
        p1.as_bytes(),
        p2.as_bytes(),
        p3.as_bytes(),
    );
    let r = sr(vm, r)?;
    Ok(r.map_or(Value::Bool(false), bytes))
}
fn sodium_crypto_aead_aes256gcm_encrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    let p3 = a.str(3);
    set_limit(vm);
    let r = s::sodium_crypto_aead_aes256gcm_encrypt(
        p0.as_bytes(),
        p1.as_bytes(),
        p2.as_bytes(),
        p3.as_bytes(),
    );
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_aead_aes256gcm_keygen(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_aead_aes256gcm_keygen();
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_aead_aegis128l_decrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    let p3 = a.str(3);
    set_limit(vm);
    let r = s::sodium_crypto_aead_aegis128l_decrypt(
        p0.as_bytes(),
        p1.as_bytes(),
        p2.as_bytes(),
        p3.as_bytes(),
    );
    let r = sr(vm, r)?;
    Ok(r.map_or(Value::Bool(false), bytes))
}
fn sodium_crypto_aead_aegis128l_encrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    let p3 = a.str(3);
    set_limit(vm);
    let r = s::sodium_crypto_aead_aegis128l_encrypt(
        p0.as_bytes(),
        p1.as_bytes(),
        p2.as_bytes(),
        p3.as_bytes(),
    );
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_aead_aegis128l_keygen(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_aead_aegis128l_keygen();
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_aead_aegis256_decrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    let p3 = a.str(3);
    set_limit(vm);
    let r = s::sodium_crypto_aead_aegis256_decrypt(
        p0.as_bytes(),
        p1.as_bytes(),
        p2.as_bytes(),
        p3.as_bytes(),
    );
    let r = sr(vm, r)?;
    Ok(r.map_or(Value::Bool(false), bytes))
}
fn sodium_crypto_aead_aegis256_encrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    let p3 = a.str(3);
    set_limit(vm);
    let r = s::sodium_crypto_aead_aegis256_encrypt(
        p0.as_bytes(),
        p1.as_bytes(),
        p2.as_bytes(),
        p3.as_bytes(),
    );
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_aead_aegis256_keygen(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_aead_aegis256_keygen();
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_aead_chacha20poly1305_decrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    let p3 = a.str(3);
    set_limit(vm);
    let r = s::sodium_crypto_aead_chacha20poly1305_decrypt(
        p0.as_bytes(),
        p1.as_bytes(),
        p2.as_bytes(),
        p3.as_bytes(),
    );
    let r = sr(vm, r)?;
    Ok(r.map_or(Value::Bool(false), bytes))
}
fn sodium_crypto_aead_chacha20poly1305_encrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    let p3 = a.str(3);
    set_limit(vm);
    let r = s::sodium_crypto_aead_chacha20poly1305_encrypt(
        p0.as_bytes(),
        p1.as_bytes(),
        p2.as_bytes(),
        p3.as_bytes(),
    );
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_aead_chacha20poly1305_keygen(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_aead_chacha20poly1305_keygen();
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_aead_chacha20poly1305_ietf_decrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    let p3 = a.str(3);
    set_limit(vm);
    let r = s::sodium_crypto_aead_chacha20poly1305_ietf_decrypt(
        p0.as_bytes(),
        p1.as_bytes(),
        p2.as_bytes(),
        p3.as_bytes(),
    );
    let r = sr(vm, r)?;
    Ok(r.map_or(Value::Bool(false), bytes))
}
fn sodium_crypto_aead_chacha20poly1305_ietf_encrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    let p3 = a.str(3);
    set_limit(vm);
    let r = s::sodium_crypto_aead_chacha20poly1305_ietf_encrypt(
        p0.as_bytes(),
        p1.as_bytes(),
        p2.as_bytes(),
        p3.as_bytes(),
    );
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_aead_chacha20poly1305_ietf_keygen(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_aead_chacha20poly1305_ietf_keygen();
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_aead_xchacha20poly1305_ietf_decrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    let p3 = a.str(3);
    set_limit(vm);
    let r = s::sodium_crypto_aead_xchacha20poly1305_ietf_decrypt(
        p0.as_bytes(),
        p1.as_bytes(),
        p2.as_bytes(),
        p3.as_bytes(),
    );
    let r = sr(vm, r)?;
    Ok(r.map_or(Value::Bool(false), bytes))
}
fn sodium_crypto_aead_xchacha20poly1305_ietf_keygen(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_aead_xchacha20poly1305_ietf_keygen();
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_aead_xchacha20poly1305_ietf_encrypt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    let p3 = a.str(3);
    set_limit(vm);
    let r = s::sodium_crypto_aead_xchacha20poly1305_ietf_encrypt(
        p0.as_bytes(),
        p1.as_bytes(),
        p2.as_bytes(),
        p3.as_bytes(),
    );
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_auth(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_auth(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_auth_keygen(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_auth_keygen();
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_auth_verify(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    set_limit(vm);
    let r = s::sodium_crypto_auth_verify(p0.as_bytes(), p1.as_bytes(), p2.as_bytes());
    let r = sr(vm, r)?;
    Ok(Value::Bool(r))
}
fn sodium_crypto_box(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    set_limit(vm);
    let r = s::sodium_crypto_box(p0.as_bytes(), p1.as_bytes(), p2.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_box_keypair(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_box_keypair();
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_box_seed_keypair(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_box_seed_keypair(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_box_keypair_from_secretkey_and_publickey(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_box_keypair_from_secretkey_and_publickey(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_box_open(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    set_limit(vm);
    let r = s::sodium_crypto_box_open(p0.as_bytes(), p1.as_bytes(), p2.as_bytes());
    let r = sr(vm, r)?;
    Ok(r.map_or(Value::Bool(false), bytes))
}
fn sodium_crypto_box_publickey(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_box_publickey(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_box_publickey_from_secretkey(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_box_publickey_from_secretkey(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_box_seal(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_box_seal(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_box_seal_open(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_box_seal_open(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(r.map_or(Value::Bool(false), bytes))
}
fn sodium_crypto_box_secretkey(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_box_secretkey(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_core_ristretto255_add(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_core_ristretto255_add(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_core_ristretto255_from_hash(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_core_ristretto255_from_hash(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_core_ristretto255_is_valid_point(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_core_ristretto255_is_valid_point(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(Value::Bool(r))
}
fn sodium_crypto_core_ristretto255_random(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_core_ristretto255_random();
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_core_ristretto255_scalar_add(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_core_ristretto255_scalar_add(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_core_ristretto255_scalar_complement(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_core_ristretto255_scalar_complement(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_core_ristretto255_scalar_invert(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_core_ristretto255_scalar_invert(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_core_ristretto255_scalar_mul(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_core_ristretto255_scalar_mul(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_core_ristretto255_scalar_negate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_core_ristretto255_scalar_negate(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_core_ristretto255_scalar_random(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_core_ristretto255_scalar_random();
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_core_ristretto255_scalar_reduce(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_core_ristretto255_scalar_reduce(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_core_ristretto255_scalar_sub(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_core_ristretto255_scalar_sub(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_core_ristretto255_sub(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_core_ristretto255_sub(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_kx_keypair(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_kx_keypair();
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_kx_publickey(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_kx_publickey(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_kx_secretkey(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_kx_secretkey(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_kx_seed_keypair(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_kx_seed_keypair(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_kx_client_session_keys(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_kx_client_session_keys(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(list([bytes(r.0), bytes(r.1)]))
}
fn sodium_crypto_kx_server_session_keys(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_kx_server_session_keys(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(list([bytes(r.0), bytes(r.1)]))
}
fn sodium_crypto_generichash(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.int_or(2, 32);
    set_limit(vm);
    let r = s::sodium_crypto_generichash(p0.as_bytes(), p1.as_bytes(), p2);
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_generichash_keygen(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_generichash_keygen();
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_generichash_init(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.int_or(1, 32);
    set_limit(vm);
    let r = s::sodium_crypto_generichash_init(p0.as_bytes(), p1);
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_generichash_update(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r0 = by_ref(a, 0);
    let mut p0 = r0.get().deref();
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_generichash_update(&mut p0, p1.as_bytes());
    let r = sr(vm, r)?;
    r0.set(p0);
    Ok(Value::Bool(r))
}
fn sodium_crypto_generichash_final(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r0 = by_ref(a, 0);
    let mut p0 = r0.get().deref();
    let p1 = a.int_or(1, 32);
    set_limit(vm);
    let r = s::sodium_crypto_generichash_final(&mut p0, p1);
    let r = sr(vm, r)?;
    r0.set(p0);
    Ok(bytes(r))
}
fn sodium_crypto_kdf_derive_from_key(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.int(0);
    let p1 = a.int(1);
    let p2 = a.str(2);
    let p3 = a.str(3);
    set_limit(vm);
    let r = s::sodium_crypto_kdf_derive_from_key(p0, p1, p2.as_bytes(), p3.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_kdf_keygen(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_kdf_keygen();
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_pwhash(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.int(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    let p3 = a.int(3);
    let p4 = a.int(4);
    let p5 = a.int_or(5, 2);
    set_limit(vm);
    let r = s::sodium_crypto_pwhash(vm, p0, p1.as_bytes(), p2.as_bytes(), p3, p4, p5);
    vm.check_deferred()?;
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_pwhash_str(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.int(1);
    let p2 = a.int(2);
    set_limit(vm);
    let r = s::sodium_crypto_pwhash_str(vm, p0.as_bytes(), p1, p2);
    vm.check_deferred()?;
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_pwhash_str_verify(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_pwhash_str_verify(vm, p0.as_bytes(), p1.as_bytes());
    vm.check_deferred()?;
    let r = sr(vm, r)?;
    Ok(Value::Bool(r))
}
fn sodium_crypto_pwhash_str_needs_rehash(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.int(1);
    let p2 = a.int(2);
    set_limit(vm);
    let r = s::sodium_crypto_pwhash_str_needs_rehash(p0.as_bytes(), p1, p2);
    let r = sr(vm, r)?;
    Ok(Value::Bool(r))
}
fn sodium_crypto_pwhash_scryptsalsa208sha256(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.int(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    let p3 = a.int(3);
    let p4 = a.int(4);
    set_limit(vm);
    let r =
        s::sodium_crypto_pwhash_scryptsalsa208sha256(vm, p0, p1.as_bytes(), p2.as_bytes(), p3, p4);
    vm.check_deferred()?;
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_pwhash_scryptsalsa208sha256_str(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.int(1);
    let p2 = a.int(2);
    set_limit(vm);
    let r = s::sodium_crypto_pwhash_scryptsalsa208sha256_str(vm, p0.as_bytes(), p1, p2);
    vm.check_deferred()?;
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_pwhash_scryptsalsa208sha256_str_verify(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r =
        s::sodium_crypto_pwhash_scryptsalsa208sha256_str_verify(vm, p0.as_bytes(), p1.as_bytes());
    vm.check_deferred()?;
    let r = sr(vm, r)?;
    Ok(Value::Bool(r))
}
fn sodium_crypto_scalarmult(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_scalarmult(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_scalarmult_ristretto255(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_scalarmult_ristretto255(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_scalarmult_ristretto255_base(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_scalarmult_ristretto255_base(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_secretbox(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    set_limit(vm);
    let r = s::sodium_crypto_secretbox(p0.as_bytes(), p1.as_bytes(), p2.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_secretbox_keygen(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_secretbox_keygen();
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_secretbox_open(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    set_limit(vm);
    let r = s::sodium_crypto_secretbox_open(p0.as_bytes(), p1.as_bytes(), p2.as_bytes());
    let r = sr(vm, r)?;
    Ok(r.map_or(Value::Bool(false), bytes))
}
fn sodium_crypto_secretstream_xchacha20poly1305_keygen(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_secretstream_xchacha20poly1305_keygen();
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_secretstream_xchacha20poly1305_init_push(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_secretstream_xchacha20poly1305_init_push(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(list([bytes(r.0), bytes(r.1)]))
}
fn sodium_crypto_secretstream_xchacha20poly1305_push(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r0 = by_ref(a, 0);
    let mut p0 = r0.get().deref();
    let p1 = a.str(1);
    let p2 = a.str(2);
    let p3 = a.int_or(3, 0);
    set_limit(vm);
    let r = s::sodium_crypto_secretstream_xchacha20poly1305_push(
        &mut p0,
        p1.as_bytes(),
        p2.as_bytes(),
        p3,
    );
    let r = sr(vm, r)?;
    r0.set(p0);
    Ok(bytes(r))
}
fn sodium_crypto_secretstream_xchacha20poly1305_init_pull(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_secretstream_xchacha20poly1305_init_pull(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_secretstream_xchacha20poly1305_pull(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r0 = by_ref(a, 0);
    let mut p0 = r0.get().deref();
    let p1 = a.str(1);
    let p2 = a.str(2);
    set_limit(vm);
    let r =
        s::sodium_crypto_secretstream_xchacha20poly1305_pull(&mut p0, p1.as_bytes(), p2.as_bytes());
    let r = sr(vm, r)?;
    r0.set(p0);
    Ok(r.map_or(Value::Bool(false), |(m, t)| list([bytes(m), Value::Int(t)])))
}
fn sodium_crypto_secretstream_xchacha20poly1305_rekey(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r0 = by_ref(a, 0);
    let mut p0 = r0.get().deref();
    set_limit(vm);
    let r = s::sodium_crypto_secretstream_xchacha20poly1305_rekey(&mut p0);
    sr(vm, r)?;
    r0.set(p0);
    Ok(Value::Null)
}
fn sodium_crypto_shorthash(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_shorthash(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_shorthash_keygen(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_shorthash_keygen();
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_sign(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_sign(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_sign_detached(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_sign_detached(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_sign_ed25519_pk_to_curve25519(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_sign_ed25519_pk_to_curve25519(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_sign_ed25519_sk_to_curve25519(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_sign_ed25519_sk_to_curve25519(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_sign_keypair(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_sign_keypair();
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_sign_keypair_from_secretkey_and_publickey(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r =
        s::sodium_crypto_sign_keypair_from_secretkey_and_publickey(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_sign_open(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_crypto_sign_open(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(r.map_or(Value::Bool(false), bytes))
}
fn sodium_crypto_sign_publickey(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_sign_publickey(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_sign_secretkey(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_sign_secretkey(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_sign_publickey_from_secretkey(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_sign_publickey_from_secretkey(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_sign_seed_keypair(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_sign_seed_keypair(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_sign_verify_detached(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    set_limit(vm);
    let r = s::sodium_crypto_sign_verify_detached(p0.as_bytes(), p1.as_bytes(), p2.as_bytes());
    let r = sr(vm, r)?;
    Ok(Value::Bool(r))
}
fn sodium_crypto_stream(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.int(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    set_limit(vm);
    let r = s::sodium_crypto_stream(p0, p1.as_bytes(), p2.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_stream_keygen(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_stream_keygen();
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_stream_xor(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    set_limit(vm);
    let r = s::sodium_crypto_stream_xor(p0.as_bytes(), p1.as_bytes(), p2.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_stream_xchacha20(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.int(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    set_limit(vm);
    let r = s::sodium_crypto_stream_xchacha20(p0, p1.as_bytes(), p2.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_stream_xchacha20_keygen(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    set_limit(vm);
    let r = s::sodium_crypto_stream_xchacha20_keygen();
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_stream_xchacha20_xor(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.str(2);
    set_limit(vm);
    let r = s::sodium_crypto_stream_xchacha20_xor(p0.as_bytes(), p1.as_bytes(), p2.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_stream_xchacha20_xor_ic(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    let p2 = a.int(2);
    let p3 = a.str(3);
    set_limit(vm);
    let r =
        s::sodium_crypto_stream_xchacha20_xor_ic(p0.as_bytes(), p1.as_bytes(), p2, p3.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_add(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r0 = by_ref(a, 0);
    let mut p0 = r0.get().deref();
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_add(&mut p0, p1.as_bytes());
    sr(vm, r)?;
    r0.set(p0);
    Ok(Value::Null)
}
fn sodium_compare(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_compare(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(Value::Int(r))
}
fn sodium_increment(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r0 = by_ref(a, 0);
    let mut p0 = r0.get().deref();
    set_limit(vm);
    let r = s::sodium_increment(&mut p0);
    sr(vm, r)?;
    r0.set(p0);
    Ok(Value::Null)
}
fn sodium_memcmp(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_memcmp(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(Value::Int(r))
}
fn sodium_memzero(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r0 = by_ref(a, 0);
    let mut p0 = r0.get().deref();
    set_limit(vm);
    let r = s::sodium_memzero(&mut p0);
    sr(vm, r)?;
    r0.set(p0);
    Ok(Value::Null)
}
fn sodium_pad(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.int(1);
    set_limit(vm);
    let r = s::sodium_pad(p0.as_bytes(), p1);
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_unpad(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.int(1);
    set_limit(vm);
    let r = s::sodium_unpad(p0.as_bytes(), p1);
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_bin2hex(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_bin2hex(p0.as_bytes());
    Ok(bytes(r))
}
fn sodium_hex2bin(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.str(1);
    set_limit(vm);
    let r = s::sodium_hex2bin(p0.as_bytes(), p1.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_bin2base64(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.int(1);
    set_limit(vm);
    let r = s::sodium_bin2base64(p0.as_bytes(), p1);
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_base642bin(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    let p1 = a.int(1);
    let p2 = a.str(2);
    set_limit(vm);
    let r = s::sodium_base642bin(p0.as_bytes(), p1, p2.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
fn sodium_crypto_scalarmult_base(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p0 = a.str(0);
    set_limit(vm);
    let r = s::sodium_crypto_scalarmult_base(p0.as_bytes());
    let r = sr(vm, r)?;
    Ok(bytes(r))
}
