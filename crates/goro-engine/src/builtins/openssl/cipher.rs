//! openssl_encrypt()/openssl_decrypt() and friends: ciphers, digests,
//! PBKDF2 and randomness (PHP manual: OpenSSL functions).
//!
//! (Stubs: being written.)

use goro_runtime::Value;

use crate::func::Args;
use crate::vm::{R, Vm};

pub(super) fn openssl_get_cipher_methods(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(goro_runtime::PhpArray::new()))
}
pub(super) fn openssl_cipher_iv_length(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(false))
}
pub(super) fn openssl_cipher_key_length(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(false))
}
pub(super) fn openssl_encrypt(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(false))
}
pub(super) fn openssl_decrypt(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(false))
}
pub(super) fn openssl_get_md_methods(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(goro_runtime::PhpArray::new()))
}
pub(super) fn openssl_digest(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(false))
}
pub(super) fn openssl_pbkdf2(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(false))
}
pub(super) fn openssl_random_pseudo_bytes(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(false))
}
