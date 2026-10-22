//! The PHP_FUNCTIONs of libsodium.c, in its order of checks and with its
//! messages.

use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use purecrypto::cipher::{
    Aegis128L, Aegis256, Aes256, Aes256Gcm, ChaCha20Poly1305, Poly1305, XChaCha20Poly1305,
};
use purecrypto::ec::ristretto255::{CompressedRistretto, RistrettoPoint, Scalar};
use purecrypto::ec::x25519::{BASE_POINT, x25519};
use purecrypto::hash::HmacSha512;
use purecrypto::mac::SipHash24;

use crate::blake2b::{self, Blake2b};
use crate::codecs;
use crate::ed25519;
use crate::pwhash;
use crate::salsa;
use crate::{A, R, arg_error, bv, list, random_bytes, sval, throw, write_ref, zpp, zpp_none};

/// ZSTR_MAX_LEN on 64-bit builds
const ZSTR_MAX_LEN: u64 = (isize::MAX as u64) - 24;

const SECRETSTREAM_ABYTES: usize = 17;
const SECRETSTREAM_HEADERBYTES: usize = 24;
const SECRETSTREAM_MESSAGEBYTES_MAX: u64 = 274877906816;
const SECRETSTREAM_STATEBYTES: usize = 52;
const TAG_MESSAGE: i64 = 0;
const TAG_REKEY: u8 = 2;

pub fn constants() -> Vec<(&'static str, Value)> {
    let s = |b: &[u8]| Value::String(goro_core::string::PhpString::from_bytes(b));
    let l = Value::Long;
    vec![
        (
            "SODIUM_LIBRARY_VERSION",
            s(crate::LIBRARY_VERSION.as_bytes()),
        ),
        (
            "SODIUM_LIBRARY_MAJOR_VERSION",
            l(crate::LIBRARY_VERSION_MAJOR),
        ),
        (
            "SODIUM_LIBRARY_MINOR_VERSION",
            l(crate::LIBRARY_VERSION_MINOR),
        ),
        ("SODIUM_CRYPTO_AEAD_AES256GCM_KEYBYTES", l(32)),
        ("SODIUM_CRYPTO_AEAD_AES256GCM_NSECBYTES", l(0)),
        ("SODIUM_CRYPTO_AEAD_AES256GCM_NPUBBYTES", l(12)),
        ("SODIUM_CRYPTO_AEAD_AES256GCM_ABYTES", l(16)),
        ("SODIUM_CRYPTO_AEAD_AEGIS128L_KEYBYTES", l(16)),
        ("SODIUM_CRYPTO_AEAD_AEGIS128L_NSECBYTES", l(0)),
        ("SODIUM_CRYPTO_AEAD_AEGIS128L_NPUBBYTES", l(16)),
        ("SODIUM_CRYPTO_AEAD_AEGIS128L_ABYTES", l(32)),
        ("SODIUM_CRYPTO_AEAD_AEGIS256_KEYBYTES", l(32)),
        ("SODIUM_CRYPTO_AEAD_AEGIS256_NSECBYTES", l(0)),
        ("SODIUM_CRYPTO_AEAD_AEGIS256_NPUBBYTES", l(32)),
        ("SODIUM_CRYPTO_AEAD_AEGIS256_ABYTES", l(32)),
        ("SODIUM_CRYPTO_AEAD_CHACHA20POLY1305_KEYBYTES", l(32)),
        ("SODIUM_CRYPTO_AEAD_CHACHA20POLY1305_NSECBYTES", l(0)),
        ("SODIUM_CRYPTO_AEAD_CHACHA20POLY1305_NPUBBYTES", l(8)),
        ("SODIUM_CRYPTO_AEAD_CHACHA20POLY1305_ABYTES", l(16)),
        ("SODIUM_CRYPTO_AEAD_CHACHA20POLY1305_IETF_KEYBYTES", l(32)),
        ("SODIUM_CRYPTO_AEAD_CHACHA20POLY1305_IETF_NSECBYTES", l(0)),
        ("SODIUM_CRYPTO_AEAD_CHACHA20POLY1305_IETF_NPUBBYTES", l(12)),
        ("SODIUM_CRYPTO_AEAD_CHACHA20POLY1305_IETF_ABYTES", l(16)),
        ("SODIUM_CRYPTO_AEAD_XCHACHA20POLY1305_IETF_KEYBYTES", l(32)),
        ("SODIUM_CRYPTO_AEAD_XCHACHA20POLY1305_IETF_NSECBYTES", l(0)),
        ("SODIUM_CRYPTO_AEAD_XCHACHA20POLY1305_IETF_NPUBBYTES", l(24)),
        ("SODIUM_CRYPTO_AEAD_XCHACHA20POLY1305_IETF_ABYTES", l(16)),
        ("SODIUM_CRYPTO_AUTH_BYTES", l(32)),
        ("SODIUM_CRYPTO_AUTH_KEYBYTES", l(32)),
        ("SODIUM_CRYPTO_BOX_SEALBYTES", l(48)),
        ("SODIUM_CRYPTO_BOX_SECRETKEYBYTES", l(32)),
        ("SODIUM_CRYPTO_BOX_PUBLICKEYBYTES", l(32)),
        ("SODIUM_CRYPTO_BOX_KEYPAIRBYTES", l(64)),
        ("SODIUM_CRYPTO_BOX_MACBYTES", l(16)),
        ("SODIUM_CRYPTO_BOX_NONCEBYTES", l(24)),
        ("SODIUM_CRYPTO_BOX_SEEDBYTES", l(32)),
        ("SODIUM_CRYPTO_KDF_BYTES_MIN", l(16)),
        ("SODIUM_CRYPTO_KDF_BYTES_MAX", l(64)),
        ("SODIUM_CRYPTO_KDF_CONTEXTBYTES", l(8)),
        ("SODIUM_CRYPTO_KDF_KEYBYTES", l(32)),
        ("SODIUM_CRYPTO_KX_SEEDBYTES", l(32)),
        ("SODIUM_CRYPTO_KX_SESSIONKEYBYTES", l(32)),
        ("SODIUM_CRYPTO_KX_PUBLICKEYBYTES", l(32)),
        ("SODIUM_CRYPTO_KX_SECRETKEYBYTES", l(32)),
        ("SODIUM_CRYPTO_KX_KEYPAIRBYTES", l(64)),
        (
            "SODIUM_CRYPTO_SECRETSTREAM_XCHACHA20POLY1305_ABYTES",
            l(SECRETSTREAM_ABYTES as i64),
        ),
        (
            "SODIUM_CRYPTO_SECRETSTREAM_XCHACHA20POLY1305_HEADERBYTES",
            l(SECRETSTREAM_HEADERBYTES as i64),
        ),
        (
            "SODIUM_CRYPTO_SECRETSTREAM_XCHACHA20POLY1305_KEYBYTES",
            l(32),
        ),
        (
            "SODIUM_CRYPTO_SECRETSTREAM_XCHACHA20POLY1305_MESSAGEBYTES_MAX",
            l(SECRETSTREAM_MESSAGEBYTES_MAX as i64),
        ),
        (
            "SODIUM_CRYPTO_SECRETSTREAM_XCHACHA20POLY1305_TAG_MESSAGE",
            l(0),
        ),
        (
            "SODIUM_CRYPTO_SECRETSTREAM_XCHACHA20POLY1305_TAG_PUSH",
            l(1),
        ),
        (
            "SODIUM_CRYPTO_SECRETSTREAM_XCHACHA20POLY1305_TAG_REKEY",
            l(2),
        ),
        (
            "SODIUM_CRYPTO_SECRETSTREAM_XCHACHA20POLY1305_TAG_FINAL",
            l(3),
        ),
        ("SODIUM_CRYPTO_GENERICHASH_BYTES", l(32)),
        ("SODIUM_CRYPTO_GENERICHASH_BYTES_MIN", l(16)),
        ("SODIUM_CRYPTO_GENERICHASH_BYTES_MAX", l(64)),
        ("SODIUM_CRYPTO_GENERICHASH_KEYBYTES", l(32)),
        ("SODIUM_CRYPTO_GENERICHASH_KEYBYTES_MIN", l(16)),
        ("SODIUM_CRYPTO_GENERICHASH_KEYBYTES_MAX", l(64)),
        (
            "SODIUM_CRYPTO_PWHASH_ALG_ARGON2I13",
            l(pwhash::ALG_ARGON2I13),
        ),
        (
            "SODIUM_CRYPTO_PWHASH_ALG_ARGON2ID13",
            l(pwhash::ALG_ARGON2ID13),
        ),
        ("SODIUM_CRYPTO_PWHASH_ALG_DEFAULT", l(pwhash::ALG_DEFAULT)),
        (
            "SODIUM_CRYPTO_PWHASH_SALTBYTES",
            l(pwhash::SALTBYTES as i64),
        ),
        ("SODIUM_CRYPTO_PWHASH_STRPREFIX", s(pwhash::STRPREFIX)),
        (
            "SODIUM_CRYPTO_PWHASH_OPSLIMIT_INTERACTIVE",
            l(pwhash::OPSLIMIT_INTERACTIVE),
        ),
        (
            "SODIUM_CRYPTO_PWHASH_MEMLIMIT_INTERACTIVE",
            l(pwhash::MEMLIMIT_INTERACTIVE),
        ),
        (
            "SODIUM_CRYPTO_PWHASH_OPSLIMIT_MODERATE",
            l(pwhash::OPSLIMIT_MODERATE),
        ),
        (
            "SODIUM_CRYPTO_PWHASH_MEMLIMIT_MODERATE",
            l(pwhash::MEMLIMIT_MODERATE),
        ),
        (
            "SODIUM_CRYPTO_PWHASH_OPSLIMIT_SENSITIVE",
            l(pwhash::OPSLIMIT_SENSITIVE),
        ),
        (
            "SODIUM_CRYPTO_PWHASH_MEMLIMIT_SENSITIVE",
            l(pwhash::MEMLIMIT_SENSITIVE),
        ),
        (
            "SODIUM_CRYPTO_PWHASH_SCRYPTSALSA208SHA256_SALTBYTES",
            l(pwhash::scrypt::SALTBYTES as i64),
        ),
        (
            "SODIUM_CRYPTO_PWHASH_SCRYPTSALSA208SHA256_STRPREFIX",
            s(pwhash::scrypt::STRPREFIX),
        ),
        (
            "SODIUM_CRYPTO_PWHASH_SCRYPTSALSA208SHA256_OPSLIMIT_INTERACTIVE",
            l(pwhash::scrypt::OPSLIMIT_INTERACTIVE),
        ),
        (
            "SODIUM_CRYPTO_PWHASH_SCRYPTSALSA208SHA256_MEMLIMIT_INTERACTIVE",
            l(pwhash::scrypt::MEMLIMIT_INTERACTIVE),
        ),
        (
            "SODIUM_CRYPTO_PWHASH_SCRYPTSALSA208SHA256_OPSLIMIT_SENSITIVE",
            l(pwhash::scrypt::OPSLIMIT_SENSITIVE),
        ),
        (
            "SODIUM_CRYPTO_PWHASH_SCRYPTSALSA208SHA256_MEMLIMIT_SENSITIVE",
            l(pwhash::scrypt::MEMLIMIT_SENSITIVE),
        ),
        ("SODIUM_CRYPTO_SCALARMULT_BYTES", l(32)),
        ("SODIUM_CRYPTO_SCALARMULT_SCALARBYTES", l(32)),
        ("SODIUM_CRYPTO_SHORTHASH_BYTES", l(8)),
        ("SODIUM_CRYPTO_SHORTHASH_KEYBYTES", l(16)),
        ("SODIUM_CRYPTO_SECRETBOX_KEYBYTES", l(32)),
        ("SODIUM_CRYPTO_SECRETBOX_MACBYTES", l(16)),
        ("SODIUM_CRYPTO_SECRETBOX_NONCEBYTES", l(24)),
        ("SODIUM_CRYPTO_SIGN_BYTES", l(64)),
        ("SODIUM_CRYPTO_SIGN_SEEDBYTES", l(32)),
        ("SODIUM_CRYPTO_SIGN_PUBLICKEYBYTES", l(32)),
        ("SODIUM_CRYPTO_SIGN_SECRETKEYBYTES", l(64)),
        ("SODIUM_CRYPTO_SIGN_KEYPAIRBYTES", l(96)),
        ("SODIUM_CRYPTO_STREAM_NONCEBYTES", l(24)),
        ("SODIUM_CRYPTO_STREAM_KEYBYTES", l(32)),
        ("SODIUM_CRYPTO_STREAM_XCHACHA20_NONCEBYTES", l(24)),
        ("SODIUM_CRYPTO_STREAM_XCHACHA20_KEYBYTES", l(32)),
        ("SODIUM_BASE64_VARIANT_ORIGINAL", l(1)),
        ("SODIUM_BASE64_VARIANT_ORIGINAL_NO_PADDING", l(3)),
        ("SODIUM_BASE64_VARIANT_URLSAFE", l(5)),
        ("SODIUM_BASE64_VARIANT_URLSAFE_NO_PADDING", l(7)),
        ("SODIUM_CRYPTO_SCALARMULT_RISTRETTO255_BYTES", l(32)),
        ("SODIUM_CRYPTO_SCALARMULT_RISTRETTO255_SCALARBYTES", l(32)),
        ("SODIUM_CRYPTO_CORE_RISTRETTO255_BYTES", l(32)),
        ("SODIUM_CRYPTO_CORE_RISTRETTO255_HASHBYTES", l(64)),
        ("SODIUM_CRYPTO_CORE_RISTRETTO255_SCALARBYTES", l(32)),
        (
            "SODIUM_CRYPTO_CORE_RISTRETTO255_NONREDUCEDSCALARBYTES",
            l(64),
        ),
    ]
}

macro_rules! args {
    ($vm:expr, $f:expr, $spec:expr, $args:expr) => {
        match zpp($vm, $f, $spec, $args, true) {
            Some(a) => a,
            None => return Ok(Value::Null),
        }
    };
}

macro_rules! none {
    ($vm:expr, $f:expr, $args:expr) => {
        if !zpp_none($vm, $f, $args) {
            return Ok(Value::Null);
        }
    };
}

/// zend_string_alloc(len): over memory_limit, the "Allowed memory size"
/// fatal error.
fn alloc_check(vm: &mut Vm, len: usize) -> Result<(), VmError> {
    vm.memory_check(len.saturating_add(24 + 1 + 7) & !7)
}

fn arr<const N: usize>(b: &[u8]) -> [u8; N] {
    b[..N].try_into().unwrap()
}

fn keygen(vm: &mut Vm, f: &str, args: &[Value], n: usize) -> R {
    none!(vm, f, args);
    let mut k = vec![0u8; n];
    random_bytes(&mut k);
    Ok(sval(k))
}

// ---------------------------------------------------------------------------
// utils

fn sodium_memzero(vm: &mut Vm, args: &[Value]) -> R {
    let a = args!(vm, "sodium_memzero", "z", args);
    let z = a[0].z();
    if !matches!(z.deref(), Value::String(_)) {
        return throw(vm, "a PHP string is required");
    }
    // (only this variable's string is wiped: copies keep theirs)
    write_ref(&z, Value::Null);
    Ok(Value::Null)
}

fn sodium_increment(vm: &mut Vm, args: &[Value]) -> R {
    let a = args!(vm, "sodium_increment", "z", args);
    let z = a[0].z();
    let Value::String(s) = z.deref() else {
        return throw(vm, "a PHP string is required");
    };
    let mut v = s.as_bytes().to_vec();
    codecs::increment(&mut v);
    write_ref(&z, sval(v));
    Ok(Value::Null)
}

fn sodium_add(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_add";
    let a = args!(vm, f, "zs", args);
    let z = a[0].z();
    let Value::String(s) = z.deref() else {
        return throw(vm, "PHP strings are required");
    };
    let mut v = s.as_bytes().to_vec();
    if v.len() != a[1].s().len() {
        return arg_error(
            vm,
            f,
            1,
            "and argument #2 ($string_2) must have the same length",
        );
    }
    codecs::add(&mut v, a[1].s());
    write_ref(&z, sval(v));
    Ok(Value::Null)
}

fn sodium_memcmp(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_memcmp";
    let a = args!(vm, f, "ss", args);
    if a[0].s().len() != a[1].s().len() {
        return arg_error(
            vm,
            f,
            1,
            "and argument #2 ($string_2) must have the same length",
        );
    }
    Ok(Value::Long(codecs::memcmp(a[0].s(), a[1].s())))
}

fn sodium_compare(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_compare";
    let a = args!(vm, f, "ss", args);
    if a[0].s().len() != a[1].s().len() {
        return arg_error(
            vm,
            f,
            1,
            "and argument #2 ($string_2) must have the same length",
        );
    }
    Ok(Value::Long(codecs::compare(a[0].s(), a[1].s())))
}

fn sodium_pad(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_pad";
    let a = args!(vm, f, "sl", args);
    let unpadded = a[0].s();
    let blocksize = a[1].l();
    if blocksize <= 0 {
        return arg_error(vm, f, 2, "must be greater than 0");
    }
    let bs = blocksize as usize;
    let mut xpadlen = bs - 1;
    if bs & (bs - 1) == 0 {
        xpadlen -= unpadded.len() & (bs - 1);
    } else {
        xpadlen -= unpadded.len() % bs;
    }
    if usize::MAX - unpadded.len() <= xpadlen {
        return throw(vm, "input is too large");
    }
    let xpadded_len = unpadded.len() + xpadlen;
    alloc_check(vm, xpadded_len + 1)?;
    let mut padded = Vec::with_capacity(xpadded_len + 1);
    padded.extend_from_slice(unpadded);
    padded.push(0x80);
    padded.resize(xpadded_len + 1, 0);
    Ok(sval(padded))
}

fn sodium_unpad(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_unpad";
    let a = args!(vm, f, "sl", args);
    let padded = a[0].s();
    let blocksize = a[1].l();
    if blocksize <= 0 {
        return arg_error(vm, f, 2, "must be greater than 0");
    }
    if (padded.len() as u64) < blocksize as u64 {
        return arg_error(vm, f, 1, "must be at least as long as the block size");
    }
    match codecs::unpad(padded, blocksize as usize) {
        Ok(n) if n as u64 <= i64::MAX as u64 => Ok(sval(padded[..n].to_vec())),
        _ => throw(vm, "invalid padding"),
    }
}

fn sodium_bin2hex(vm: &mut Vm, args: &[Value]) -> R {
    let a = args!(vm, "sodium_bin2hex", "s", args);
    Ok(sval(codecs::bin2hex(a[0].s())))
}

fn sodium_hex2bin(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_hex2bin";
    let a = args!(vm, f, "s|s", args);
    let hex = a[0].s();
    let (ok, bin, end) = codecs::hex2bin(hex, a[1].opt_s(), hex.len() / 2);
    if !ok || end != hex.len() {
        return arg_error(vm, f, 1, "must be a valid hexadecimal string");
    }
    Ok(sval(bin))
}

fn base64_variant_ok(variant: i64) -> bool {
    ((variant as u32) & !0x6) == 0x1
}

fn sodium_bin2base64(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_bin2base64";
    let a = args!(vm, f, "sl", args);
    let variant = a[1].l();
    if !base64_variant_ok(variant) {
        return arg_error(vm, f, 2, "must be a valid base64 variant identifier");
    }
    Ok(sval(codecs::bin2base64(a[0].s(), variant as u32)))
}

fn sodium_base642bin(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_base642bin";
    let a = args!(vm, f, "sl|s", args);
    let variant = a[1].l();
    if !base64_variant_ok(variant) {
        return arg_error(vm, f, 2, "must be a valid base64 variant identifier");
    }
    let b64 = a[0].s();
    let bin_len = b64.len() / 4 * 3 + 2;
    let (ok, bin, end) = codecs::base642bin(b64, a[2].opt_s(), variant as u32, bin_len);
    if !ok || end != b64.len() {
        return arg_error(vm, f, 1, "must be a valid base64 string");
    }
    Ok(sval(bin))
}

// ---------------------------------------------------------------------------
// hashing

fn sodium_crypto_shorthash(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_shorthash";
    let a = args!(vm, f, "ss", args);
    if a[1].s().len() != 16 {
        return arg_error(
            vm,
            f,
            2,
            "must be SODIUM_CRYPTO_SHORTHASH_KEYBYTES bytes long",
        );
    }
    let h = SipHash24::compute(&arr::<16>(a[1].s()), a[0].s());
    Ok(sval(h.to_vec()))
}

fn generichash_check(vm: &mut Vm, hash_len: i64, key_len: usize) -> bool {
    if !(16..=64).contains(&hash_len) {
        let _ = throw(vm, "unsupported output length");
        return false;
    }
    if key_len != 0 && !(16..=64).contains(&key_len) {
        let _ = throw(vm, "unsupported key length");
        return false;
    }
    true
}

fn sodium_crypto_generichash(vm: &mut Vm, args: &[Value]) -> R {
    let a = args!(vm, "sodium_crypto_generichash", "s|sl", args);
    let hash_len = a[2].l_or(32);
    let key = a[1].s();
    if !generichash_check(vm, hash_len, key.len()) {
        return Ok(Value::Null);
    }
    let mut out = vec![0u8; hash_len as usize];
    if blake2b::hash(&mut out, a[0].s(), key, None, None).is_err() {
        return throw(vm, "internal error");
    }
    Ok(sval(out))
}

fn sodium_crypto_generichash_init(vm: &mut Vm, args: &[Value]) -> R {
    let a = args!(vm, "sodium_crypto_generichash_init", "|sl", args);
    let hash_len = a[1].l_or(32);
    let key = a[0].s();
    if !generichash_check(vm, hash_len, key.len()) {
        return Ok(Value::Null);
    }
    match Blake2b::new(hash_len as usize, key, None, None) {
        Ok(s) => Ok(sval(s.to_bytes())),
        Err(()) => throw(vm, "internal error"),
    }
}

/// The state string of a by-reference state argument.
fn state_of(vm: &mut Vm, f: &str, z: &Value) -> Option<Vec<u8>> {
    match z.deref() {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        _ => {
            let _ = arg_error(vm, f, 1, "must be a reference to a state");
            None
        }
    }
}

fn sodium_crypto_generichash_update(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_generichash_update";
    let a = args!(vm, f, "zs", args);
    let z = a[0].z();
    let Some(state) = state_of(vm, f, &z) else {
        return Ok(Value::Null);
    };
    if state.len() != blake2b::STATE_BYTES {
        return throw(vm, "incorrect state length");
    }
    let Ok(mut s) = Blake2b::from_bytes(&state) else {
        return throw(vm, "internal error");
    };
    s.update(a[1].s());
    write_ref(&z, sval(s.to_bytes()));
    Ok(Value::True)
}

fn sodium_crypto_generichash_final(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_generichash_final";
    let a = args!(vm, f, "z|l", args);
    let z = a[0].z();
    let Some(state) = state_of(vm, f, &z) else {
        return Ok(Value::Null);
    };
    if state.len() != blake2b::STATE_BYTES {
        return throw(vm, "incorrect state length");
    }
    let hash_len = a[1].l_or(32);
    if !(16..=64).contains(&hash_len) {
        return throw(vm, "unsupported output length");
    }
    let mut out = vec![0u8; hash_len as usize];
    let ok = Blake2b::from_bytes(&state).and_then(|mut s| s.finalize(&mut out));
    if ok.is_err() {
        return throw(vm, "internal error");
    }
    write_ref(&z, Value::Null);
    Ok(sval(out))
}

fn sodium_crypto_generichash_keygen(vm: &mut Vm, args: &[Value]) -> R {
    keygen(vm, "sodium_crypto_generichash_keygen", args, 32)
}

fn sodium_crypto_kdf_derive_from_key(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_kdf_derive_from_key";
    let a = args!(vm, f, "llss", args);
    let subkey_len = a[0].l();
    let subkey_id = a[1].l();
    if subkey_len < 16 {
        return arg_error(
            vm,
            f,
            1,
            "must be greater than or equal to SODIUM_CRYPTO_KDF_BYTES_MIN",
        );
    }
    if subkey_len > 64 {
        return arg_error(
            vm,
            f,
            1,
            "must be less than or equal to SODIUM_CRYPTO_KDF_BYTES_MAX",
        );
    }
    if subkey_id < 0 {
        return arg_error(vm, f, 2, "must be greater than or equal to 0");
    }
    if a[2].s().len() != 8 {
        return arg_error(
            vm,
            f,
            3,
            "must be SODIUM_CRYPTO_KDF_CONTEXTBYTES bytes long",
        );
    }
    if a[3].s().len() != 32 {
        return arg_error(vm, f, 4, "must be SODIUM_CRYPTO_KDF_KEYBYTES bytes long");
    }
    let mut ctx = [0u8; 16];
    ctx[..8].copy_from_slice(a[2].s());
    let mut salt = [0u8; 16];
    salt[..8].copy_from_slice(&(subkey_id as u64).to_le_bytes());
    let mut out = vec![0u8; subkey_len as usize];
    let _ = blake2b::hash(&mut out, b"", a[3].s(), Some(&salt), Some(&ctx));
    Ok(sval(out))
}

fn sodium_crypto_kdf_keygen(vm: &mut Vm, args: &[Value]) -> R {
    keygen(vm, "sodium_crypto_kdf_keygen", args, 32)
}

fn sodium_crypto_auth(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_auth";
    let a = args!(vm, f, "ss", args);
    if a[1].s().len() != 32 {
        return arg_error(vm, f, 2, "must be SODIUM_CRYPTO_AUTH_KEYBYTES bytes long");
    }
    let mac = HmacSha512::mac(a[1].s(), a[0].s());
    Ok(sval(mac[..32].to_vec()))
}

fn sodium_crypto_auth_verify(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_auth_verify";
    let a = args!(vm, f, "sss", args);
    if a[2].s().len() != 32 {
        return arg_error(vm, f, 3, "must be SODIUM_CRYPTO_AUTH_KEYBYTES bytes long");
    }
    if a[0].s().len() != 32 {
        return arg_error(vm, f, 1, "must be SODIUM_CRYPTO_AUTH_BYTES bytes long");
    }
    let mac = HmacSha512::mac(a[2].s(), a[1].s());
    Ok(bv(codecs::memcmp(&mac[..32], a[0].s()) == 0))
}

fn sodium_crypto_auth_keygen(vm: &mut Vm, args: &[Value]) -> R {
    keygen(vm, "sodium_crypto_auth_keygen", args, 32)
}

// ---------------------------------------------------------------------------
// secretbox / box

/// crypto_secretbox_easy(): mac || c
fn secretbox_easy(m: &[u8], nonce: &[u8; 24], key: &[u8; 32]) -> Vec<u8> {
    let mut buf = vec![0u8; 32 + m.len()];
    buf[32..].copy_from_slice(m);
    salsa::xsalsa20_xor_ic(&mut buf, nonce, 0, key);
    let mut p = Poly1305::new(&arr::<32>(&buf[..32]));
    p.update(&buf[32..]);
    let mac = p.finish();
    let mut out = Vec::with_capacity(16 + m.len());
    out.extend_from_slice(&mac);
    out.extend_from_slice(&buf[32..]);
    buf.fill(0);
    out
}

/// crypto_secretbox_open_easy()
fn secretbox_open_easy(c: &[u8], nonce: &[u8; 24], key: &[u8; 32]) -> Option<Vec<u8>> {
    if c.len() < 16 {
        return None;
    }
    let mut block0 = [0u8; 32];
    salsa::xsalsa20_xor_ic(&mut block0, nonce, 0, key);
    let mut p = Poly1305::new(&block0);
    block0.fill(0);
    p.update(&c[16..]);
    let mac = p.finish();
    if codecs::memcmp(&mac, &c[..16]) != 0 {
        return None;
    }
    let mut buf = vec![0u8; 32 + c.len() - 16];
    buf[32..].copy_from_slice(&c[16..]);
    salsa::xsalsa20_xor_ic(&mut buf, nonce, 0, key);
    Some(buf[32..].to_vec())
}

/// crypto_box_beforenm()
fn box_beforenm(pk: &[u8; 32], sk: &[u8; 32]) -> Option<[u8; 32]> {
    let s = x25519(sk, pk);
    if codecs::is_zero(&s) {
        return None;
    }
    Some(salsa::hsalsa20(&s, &[0u8; 16]))
}

fn box_easy(m: &[u8], nonce: &[u8; 24], pk: &[u8; 32], sk: &[u8; 32]) -> Option<Vec<u8>> {
    let k = box_beforenm(pk, sk)?;
    Some(secretbox_easy(m, nonce, &k))
}

fn box_open_easy(c: &[u8], nonce: &[u8; 24], pk: &[u8; 32], sk: &[u8; 32]) -> Option<Vec<u8>> {
    let k = box_beforenm(pk, sk)?;
    secretbox_open_easy(c, nonce, &k)
}

/// _crypto_box_seal_nonce()
fn seal_nonce(epk: &[u8], pk: &[u8]) -> [u8; 24] {
    let mut s = Blake2b::new(24, b"", None, None).unwrap();
    s.update(epk);
    s.update(pk);
    let mut n = [0u8; 24];
    let _ = s.finalize(&mut n);
    n
}

fn scalarmult_base(sk: &[u8; 32]) -> [u8; 32] {
    x25519(sk, &BASE_POINT)
}

fn sodium_crypto_secretbox(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_secretbox";
    let a = args!(vm, f, "sss", args);
    if a[1].s().len() != 24 {
        return arg_error(
            vm,
            f,
            2,
            "must be SODIUM_CRYPTO_SECRETBOX_NONCEBYTES bytes long",
        );
    }
    if a[2].s().len() != 32 {
        return arg_error(
            vm,
            f,
            3,
            "must be SODIUM_CRYPTO_SECRETBOX_KEYBYTES bytes long",
        );
    }
    Ok(sval(secretbox_easy(
        a[0].s(),
        &arr(a[1].s()),
        &arr(a[2].s()),
    )))
}

fn sodium_crypto_secretbox_open(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_secretbox_open";
    let a = args!(vm, f, "sss", args);
    if a[1].s().len() != 24 {
        return arg_error(
            vm,
            f,
            2,
            "must be SODIUM_CRYPTO_SECRETBOX_NONCEBYTES bytes long",
        );
    }
    if a[2].s().len() != 32 {
        return arg_error(
            vm,
            f,
            3,
            "must be SODIUM_CRYPTO_SECRETBOX_KEYBYTES bytes long",
        );
    }
    match secretbox_open_easy(a[0].s(), &arr(a[1].s()), &arr(a[2].s())) {
        Some(m) => Ok(sval(m)),
        None => Ok(Value::False),
    }
}

fn sodium_crypto_secretbox_keygen(vm: &mut Vm, args: &[Value]) -> R {
    keygen(vm, "sodium_crypto_secretbox_keygen", args, 32)
}

fn sodium_crypto_box_keypair(vm: &mut Vm, args: &[Value]) -> R {
    none!(vm, "sodium_crypto_box_keypair", args);
    let mut sk = [0u8; 32];
    random_bytes(&mut sk);
    let pk = scalarmult_base(&sk);
    let mut kp = sk.to_vec();
    kp.extend_from_slice(&pk);
    sk.fill(0);
    Ok(sval(kp))
}

fn sodium_crypto_box_seed_keypair(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_box_seed_keypair";
    let a = args!(vm, f, "s", args);
    if a[0].s().len() != 32 {
        return arg_error(vm, f, 1, "must be SODIUM_CRYPTO_BOX_SEEDBYTES bytes long");
    }
    let h = purecrypto::hash::sha512(a[0].s());
    let sk: [u8; 32] = arr(&h);
    let pk = scalarmult_base(&sk);
    let mut kp = sk.to_vec();
    kp.extend_from_slice(&pk);
    Ok(sval(kp))
}

fn sodium_crypto_box_keypair_from_secretkey_and_publickey(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_box_keypair_from_secretkey_and_publickey";
    let a = args!(vm, f, "ss", args);
    if a[0].s().len() != 32 {
        return arg_error(
            vm,
            f,
            1,
            "must be SODIUM_CRYPTO_BOX_SECRETKEYBYTES bytes long",
        );
    }
    if a[1].s().len() != 32 {
        return arg_error(
            vm,
            f,
            2,
            "must be SODIUM_CRYPTO_BOX_PUBLICKEYBYTES bytes long",
        );
    }
    let mut kp = a[0].s().to_vec();
    kp.extend_from_slice(a[1].s());
    Ok(sval(kp))
}

fn keypair_part(vm: &mut Vm, f: &str, args: &[Value], len: usize, what: &str, first: bool) -> R {
    let a = args!(vm, f, "s", args);
    let kp = a[0].s();
    if kp.len() != len {
        return arg_error(vm, f, 1, &format!("must be {} bytes long", what));
    }
    let half = if len == 96 { 64 } else { 32 };
    Ok(sval(if first {
        kp[..half].to_vec()
    } else {
        kp[half..].to_vec()
    }))
}

fn sodium_crypto_box_secretkey(vm: &mut Vm, args: &[Value]) -> R {
    keypair_part(
        vm,
        "sodium_crypto_box_secretkey",
        args,
        64,
        "SODIUM_CRYPTO_BOX_KEYPAIRBYTES",
        true,
    )
}

fn sodium_crypto_box_publickey(vm: &mut Vm, args: &[Value]) -> R {
    keypair_part(
        vm,
        "sodium_crypto_box_publickey",
        args,
        64,
        "SODIUM_CRYPTO_BOX_KEYPAIRBYTES",
        false,
    )
}

fn box_publickey_from_secretkey(vm: &mut Vm, f: &str, args: &[Value]) -> R {
    let a = args!(vm, f, "s", args);
    if a[0].s().len() != 32 {
        return arg_error(
            vm,
            f,
            1,
            "must be SODIUM_CRYPTO_BOX_SECRETKEYBYTES bytes long",
        );
    }
    Ok(sval(scalarmult_base(&arr(a[0].s())).to_vec()))
}

fn sodium_crypto_box_publickey_from_secretkey(vm: &mut Vm, args: &[Value]) -> R {
    box_publickey_from_secretkey(vm, "sodium_crypto_box_publickey_from_secretkey", args)
}

fn sodium_crypto_scalarmult_base(vm: &mut Vm, args: &[Value]) -> R {
    box_publickey_from_secretkey(vm, "sodium_crypto_scalarmult_base", args)
}

fn sodium_crypto_box(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_box";
    let a = args!(vm, f, "sss", args);
    if a[1].s().len() != 24 {
        return arg_error(vm, f, 2, "must be SODIUM_CRYPTO_BOX_NONCEBYTES bytes long");
    }
    if a[2].s().len() != 64 {
        return arg_error(
            vm,
            f,
            3,
            "must be SODIUM_CRYPTO_BOX_KEYPAIRBYTES bytes long",
        );
    }
    let kp = a[2].s();
    match box_easy(a[0].s(), &arr(a[1].s()), &arr(&kp[32..]), &arr(kp)) {
        Some(c) => Ok(sval(c)),
        None => throw(vm, "internal error"),
    }
}

fn sodium_crypto_box_open(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_box_open";
    let a = args!(vm, f, "sss", args);
    if a[1].s().len() != 24 {
        return arg_error(vm, f, 2, "must be SODIUM_CRYPTO_BOX_NONCEBYTES bytes long");
    }
    if a[2].s().len() != 64 {
        return arg_error(
            vm,
            f,
            3,
            "must be SODIUM_CRYPTO_BOX_KEYPAIRBYTES bytes long",
        );
    }
    let kp = a[2].s();
    if a[0].s().len() < 16 {
        return Ok(Value::False);
    }
    match box_open_easy(a[0].s(), &arr(a[1].s()), &arr(&kp[32..]), &arr(kp)) {
        Some(m) => Ok(sval(m)),
        None => Ok(Value::False),
    }
}

fn sodium_crypto_box_seal(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_box_seal";
    let a = args!(vm, f, "ss", args);
    if a[1].s().len() != 32 {
        return arg_error(
            vm,
            f,
            2,
            "must be SODIUM_CRYPTO_BOX_PUBLICKEYBYTES bytes long",
        );
    }
    let pk: [u8; 32] = arr(a[1].s());
    let mut esk = [0u8; 32];
    random_bytes(&mut esk);
    let epk = scalarmult_base(&esk);
    let nonce = seal_nonce(&epk, &pk);
    let c = box_easy(a[0].s(), &nonce, &pk, &esk);
    esk.fill(0);
    match c {
        Some(c) => {
            let mut out = epk.to_vec();
            out.extend_from_slice(&c);
            Ok(sval(out))
        }
        None => throw(vm, "internal error"),
    }
}

fn sodium_crypto_box_seal_open(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_box_seal_open";
    let a = args!(vm, f, "ss", args);
    if a[1].s().len() != 64 {
        return arg_error(
            vm,
            f,
            2,
            "must be SODIUM_CRYPTO_BOX_KEYPAIRBYTES bytes long",
        );
    }
    let kp = a[1].s();
    let c = a[0].s();
    if c.len() < 48 {
        return Ok(Value::False);
    }
    let epk: [u8; 32] = arr(c);
    let nonce = seal_nonce(&epk, &kp[32..]);
    match box_open_easy(&c[32..], &nonce, &epk, &arr(kp)) {
        Some(m) => Ok(sval(m)),
        None => Ok(Value::False),
    }
}

// ---------------------------------------------------------------------------
// kx / scalarmult

fn sodium_crypto_kx_seed_keypair(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_kx_seed_keypair";
    let a = args!(vm, f, "s", args);
    if a[0].s().len() != 32 {
        return arg_error(vm, f, 1, "must be SODIUM_CRYPTO_KX_SEEDBYTES bytes long");
    }
    let mut sk = [0u8; 32];
    let _ = blake2b::hash(&mut sk, a[0].s(), b"", None, None);
    let pk = scalarmult_base(&sk);
    let mut kp = sk.to_vec();
    kp.extend_from_slice(&pk);
    Ok(sval(kp))
}

fn sodium_crypto_kx_keypair(vm: &mut Vm, args: &[Value]) -> R {
    none!(vm, "sodium_crypto_kx_keypair", args);
    let mut sk = [0u8; 32];
    random_bytes(&mut sk);
    let pk = scalarmult_base(&sk);
    let mut kp = sk.to_vec();
    kp.extend_from_slice(&pk);
    Ok(sval(kp))
}

fn sodium_crypto_kx_secretkey(vm: &mut Vm, args: &[Value]) -> R {
    keypair_part(
        vm,
        "sodium_crypto_kx_secretkey",
        args,
        64,
        "SODIUM_CRYPTO_KX_KEYPAIRBYTES",
        true,
    )
}

fn sodium_crypto_kx_publickey(vm: &mut Vm, args: &[Value]) -> R {
    keypair_part(
        vm,
        "sodium_crypto_kx_publickey",
        args,
        64,
        "SODIUM_CRYPTO_KX_KEYPAIRBYTES",
        false,
    )
}

fn kx_session_keys(vm: &mut Vm, f: &str, args: &[Value], client: bool) -> R {
    let a = args!(vm, f, "ss", args);
    if a[0].s().len() != 64 {
        return arg_error(vm, f, 1, "must be SODIUM_CRYPTO_KX_KEYPAIRBYTES bytes long");
    }
    if a[1].s().len() != 32 {
        return arg_error(
            vm,
            f,
            2,
            "must be SODIUM_CRYPTO_KX_PUBLICKEYBYTES bytes long",
        );
    }
    let kp = a[0].s();
    let (sk, own_pk) = (&kp[..32], &kp[32..]);
    let peer = a[1].s();
    let mut q = x25519(&arr(sk), &arr(peer));
    if codecs::is_zero(&q) {
        return throw(vm, "internal error");
    }
    let (client_pk, server_pk) = if client {
        (own_pk, peer)
    } else {
        (peer, own_pk)
    };
    let mut h = Blake2b::new(64, b"", None, None).unwrap();
    h.update(&q);
    q.fill(0);
    h.update(client_pk);
    h.update(server_pk);
    let mut keys = [0u8; 64];
    let _ = h.finalize(&mut keys);
    let (k0, k1) = (keys[..32].to_vec(), keys[32..].to_vec());
    keys.fill(0);
    Ok(if client {
        list(vec![sval(k0), sval(k1)])
    } else {
        list(vec![sval(k1), sval(k0)])
    })
}

fn sodium_crypto_kx_client_session_keys(vm: &mut Vm, args: &[Value]) -> R {
    kx_session_keys(vm, "sodium_crypto_kx_client_session_keys", args, true)
}

fn sodium_crypto_kx_server_session_keys(vm: &mut Vm, args: &[Value]) -> R {
    kx_session_keys(vm, "sodium_crypto_kx_server_session_keys", args, false)
}

fn sodium_crypto_scalarmult(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_scalarmult";
    let a = args!(vm, f, "ss", args);
    if a[0].s().len() != 32 {
        return arg_error(
            vm,
            f,
            1,
            "must be SODIUM_CRYPTO_SCALARMULT_SCALARBYTES bytes long",
        );
    }
    if a[1].s().len() != 32 {
        return arg_error(
            vm,
            f,
            2,
            "must be SODIUM_CRYPTO_SCALARMULT_BYTES bytes long",
        );
    }
    let q = x25519(&arr(a[0].s()), &arr(a[1].s()));
    if codecs::is_zero(&q) {
        return throw(vm, "internal error");
    }
    Ok(sval(q.to_vec()))
}

// ---------------------------------------------------------------------------
// sign

fn sodium_crypto_sign_keypair(vm: &mut Vm, args: &[Value]) -> R {
    none!(vm, "sodium_crypto_sign_keypair", args);
    let mut seed = [0u8; 32];
    random_bytes(&mut seed);
    let (pk, sk) = ed25519::seed_keypair(&seed);
    seed.fill(0);
    let mut kp = sk.to_vec();
    kp.extend_from_slice(&pk);
    Ok(sval(kp))
}

fn sodium_crypto_sign_seed_keypair(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_sign_seed_keypair";
    let a = args!(vm, f, "s", args);
    if a[0].s().len() != 32 {
        return arg_error(vm, f, 1, "must be SODIUM_CRYPTO_SIGN_SEEDBYTES bytes long");
    }
    let (pk, sk) = ed25519::seed_keypair(&arr(a[0].s()));
    let mut kp = sk.to_vec();
    kp.extend_from_slice(&pk);
    Ok(sval(kp))
}

fn sodium_crypto_sign_keypair_from_secretkey_and_publickey(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_sign_keypair_from_secretkey_and_publickey";
    let a = args!(vm, f, "ss", args);
    if a[0].s().len() != 64 {
        return arg_error(
            vm,
            f,
            1,
            "must be SODIUM_CRYPTO_SIGN_SECRETKEYBYTES bytes long",
        );
    }
    if a[1].s().len() != 32 {
        return arg_error(
            vm,
            f,
            2,
            "must be SODIUM_CRYPTO_SIGN_PUBLICKEYBYTES bytes long",
        );
    }
    let mut kp = a[0].s().to_vec();
    kp.extend_from_slice(a[1].s());
    Ok(sval(kp))
}

fn sodium_crypto_sign_publickey_from_secretkey(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_sign_publickey_from_secretkey";
    let a = args!(vm, f, "s", args);
    if a[0].s().len() != 64 {
        return arg_error(
            vm,
            f,
            1,
            "must be SODIUM_CRYPTO_SIGN_SECRETKEYBYTES bytes long",
        );
    }
    // crypto_sign_ed25519_sk_to_pk(): the public half of the secret key
    Ok(sval(a[0].s()[32..].to_vec()))
}

fn sodium_crypto_sign_secretkey(vm: &mut Vm, args: &[Value]) -> R {
    keypair_part(
        vm,
        "sodium_crypto_sign_secretkey",
        args,
        96,
        "SODIUM_CRYPTO_SIGN_KEYPAIRBYTES",
        true,
    )
}

fn sodium_crypto_sign_publickey(vm: &mut Vm, args: &[Value]) -> R {
    keypair_part(
        vm,
        "sodium_crypto_sign_publickey",
        args,
        96,
        "SODIUM_CRYPTO_SIGN_KEYPAIRBYTES",
        false,
    )
}

fn sodium_crypto_sign(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_sign";
    let a = args!(vm, f, "ss", args);
    if a[1].s().len() != 64 {
        return arg_error(
            vm,
            f,
            2,
            "must be SODIUM_CRYPTO_SIGN_SECRETKEYBYTES bytes long",
        );
    }
    let sig = ed25519::sign_detached(a[0].s(), &arr(a[1].s()));
    let mut out = sig.to_vec();
    out.extend_from_slice(a[0].s());
    Ok(sval(out))
}

fn sodium_crypto_sign_open(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_sign_open";
    let a = args!(vm, f, "ss", args);
    if a[1].s().len() != 32 {
        return arg_error(
            vm,
            f,
            2,
            "must be SODIUM_CRYPTO_SIGN_PUBLICKEYBYTES bytes long",
        );
    }
    let sm = a[0].s();
    if sm.len() < 64 {
        return Ok(Value::False);
    }
    if ed25519::verify_detached(&arr(sm), &sm[64..], &arr(a[1].s())) {
        Ok(sval(sm[64..].to_vec()))
    } else {
        Ok(Value::False)
    }
}

fn sodium_crypto_sign_detached(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_sign_detached";
    let a = args!(vm, f, "ss", args);
    if a[1].s().len() != 64 {
        return arg_error(
            vm,
            f,
            2,
            "must be SODIUM_CRYPTO_SIGN_SECRETKEYBYTES bytes long",
        );
    }
    Ok(sval(
        ed25519::sign_detached(a[0].s(), &arr(a[1].s())).to_vec(),
    ))
}

fn sodium_crypto_sign_verify_detached(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_sign_verify_detached";
    let a = args!(vm, f, "sss", args);
    if a[0].s().len() != 64 {
        return arg_error(vm, f, 1, "must be SODIUM_CRYPTO_SIGN_BYTES bytes long");
    }
    if a[2].s().len() != 32 {
        return arg_error(
            vm,
            f,
            3,
            "must be SODIUM_CRYPTO_SIGN_PUBLICKEYBYTES bytes long",
        );
    }
    Ok(bv(ed25519::verify_detached(
        &arr(a[0].s()),
        a[1].s(),
        &arr(a[2].s()),
    )))
}

fn sodium_crypto_sign_ed25519_sk_to_curve25519(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_sign_ed25519_sk_to_curve25519";
    let a = args!(vm, f, "s", args);
    if a[0].s().len() != 64 {
        return arg_error(
            vm,
            f,
            1,
            "must be SODIUM_CRYPTO_SIGN_SECRETKEYBYTES bytes long",
        );
    }
    Ok(sval(ed25519::sk_to_curve25519(&arr(a[0].s())).to_vec()))
}

fn sodium_crypto_sign_ed25519_pk_to_curve25519(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_sign_ed25519_pk_to_curve25519";
    let a = args!(vm, f, "s", args);
    if a[0].s().len() != 32 {
        return arg_error(
            vm,
            f,
            1,
            "must be SODIUM_CRYPTO_SIGN_PUBLICKEYBYTES bytes long",
        );
    }
    match ed25519::pk_to_curve25519(&arr(a[0].s())) {
        Some(k) => Ok(sval(k.to_vec())),
        None => throw(vm, "conversion failed"),
    }
}

// ---------------------------------------------------------------------------
// stream

fn stream_len_check(vm: &mut Vm, f: &str, len: i64) -> bool {
    if len <= 0 || len as u64 >= ZSTR_MAX_LEN {
        let _ = arg_error(vm, f, 1, "must be greater than 0");
        return false;
    }
    true
}

fn stream_impl(vm: &mut Vm, f: &str, args: &[Value], xchacha: bool) -> R {
    let a = args!(vm, f, "lss", args);
    if !stream_len_check(vm, f, a[0].l()) {
        return Ok(Value::Null);
    }
    let (nname, kname) = if xchacha {
        (
            "SODIUM_CRYPTO_STREAM_XCHACHA20_NONCEBYTES",
            "SODIUM_CRYPTO_STREAM_XCHACHA20_KEYBYTES",
        )
    } else {
        (
            "SODIUM_CRYPTO_STREAM_NONCEBYTES",
            "SODIUM_CRYPTO_STREAM_KEYBYTES",
        )
    };
    if a[1].s().len() != 24 {
        return arg_error(vm, f, 2, &format!("must be {} bytes long", nname));
    }
    if a[2].s().len() != 32 {
        return arg_error(vm, f, 3, &format!("must be {} bytes long", kname));
    }
    alloc_check(vm, a[0].l() as usize)?;
    let mut buf = vec![0u8; a[0].l() as usize];
    if xchacha {
        salsa::xchacha20_xor_ic(&mut buf, &arr(a[1].s()), 0, &arr(a[2].s()));
    } else {
        salsa::xsalsa20_xor_ic(&mut buf, &arr(a[1].s()), 0, &arr(a[2].s()));
    }
    Ok(sval(buf))
}

fn stream_xor_impl(vm: &mut Vm, f: &str, args: &[Value], xchacha: bool, ic: bool) -> R {
    let a = if ic {
        args!(vm, f, "ssls", args)
    } else {
        args!(vm, f, "sss", args)
    };
    let (key, counter) = if ic {
        (a[3].s(), a[2].l() as u64)
    } else {
        (a[2].s(), 0)
    };
    let (nname, kname) = if xchacha {
        (
            "SODIUM_CRYPTO_STREAM_XCHACHA20_NONCEBYTES",
            "SODIUM_CRYPTO_STREAM_XCHACHA20_KEYBYTES",
        )
    } else {
        (
            "SODIUM_CRYPTO_STREAM_NONCEBYTES",
            "SODIUM_CRYPTO_STREAM_KEYBYTES",
        )
    };
    if a[1].s().len() != 24 {
        return arg_error(vm, f, 2, &format!("must be {} bytes long", nname));
    }
    if key.len() != 32 {
        return arg_error(vm, f, 3, &format!("must be {} bytes long", kname));
    }
    let mut buf = a[0].s().to_vec();
    if xchacha {
        salsa::xchacha20_xor_ic(&mut buf, &arr(a[1].s()), counter, &arr(key));
    } else {
        salsa::xsalsa20_xor_ic(&mut buf, &arr(a[1].s()), 0, &arr(key));
    }
    Ok(sval(buf))
}

fn sodium_crypto_stream(vm: &mut Vm, args: &[Value]) -> R {
    stream_impl(vm, "sodium_crypto_stream", args, false)
}

fn sodium_crypto_stream_xor(vm: &mut Vm, args: &[Value]) -> R {
    stream_xor_impl(vm, "sodium_crypto_stream_xor", args, false, false)
}

fn sodium_crypto_stream_keygen(vm: &mut Vm, args: &[Value]) -> R {
    keygen(vm, "sodium_crypto_stream_keygen", args, 32)
}

fn sodium_crypto_stream_xchacha20(vm: &mut Vm, args: &[Value]) -> R {
    stream_impl(vm, "sodium_crypto_stream_xchacha20", args, true)
}

fn sodium_crypto_stream_xchacha20_xor(vm: &mut Vm, args: &[Value]) -> R {
    stream_xor_impl(vm, "sodium_crypto_stream_xchacha20_xor", args, true, false)
}

fn sodium_crypto_stream_xchacha20_xor_ic(vm: &mut Vm, args: &[Value]) -> R {
    stream_xor_impl(
        vm,
        "sodium_crypto_stream_xchacha20_xor_ic",
        args,
        true,
        true,
    )
}

fn sodium_crypto_stream_xchacha20_keygen(vm: &mut Vm, args: &[Value]) -> R {
    keygen(vm, "sodium_crypto_stream_xchacha20_keygen", args, 32)
}

// ---------------------------------------------------------------------------
// AEAD

#[derive(Clone, Copy, PartialEq)]
enum Aead {
    Aes256Gcm,
    Aegis128L,
    Aegis256,
    ChaCha20Poly1305,
    ChaCha20Poly1305Ietf,
    XChaCha20Poly1305Ietf,
}

impl Aead {
    fn name(self) -> &'static str {
        match self {
            Aead::Aes256Gcm => "AES256GCM",
            Aead::Aegis128L => "AEGIS128L",
            Aead::Aegis256 => "AEGIS256",
            Aead::ChaCha20Poly1305 => "CHACHA20POLY1305",
            Aead::ChaCha20Poly1305Ietf => "CHACHA20POLY1305_IETF",
            Aead::XChaCha20Poly1305Ietf => "XCHACHA20POLY1305_IETF",
        }
    }
    fn fname(self) -> &'static str {
        match self {
            Aead::Aes256Gcm => "aes256gcm",
            Aead::Aegis128L => "aegis128l",
            Aead::Aegis256 => "aegis256",
            Aead::ChaCha20Poly1305 => "chacha20poly1305",
            Aead::ChaCha20Poly1305Ietf => "chacha20poly1305_ietf",
            Aead::XChaCha20Poly1305Ietf => "xchacha20poly1305_ietf",
        }
    }
    fn keybytes(self) -> usize {
        match self {
            Aead::Aegis128L => 16,
            _ => 32,
        }
    }
    fn npubbytes(self) -> usize {
        match self {
            Aead::Aes256Gcm | Aead::ChaCha20Poly1305Ietf => 12,
            Aead::Aegis128L => 16,
            Aead::Aegis256 => 32,
            Aead::ChaCha20Poly1305 => 8,
            Aead::XChaCha20Poly1305Ietf => 24,
        }
    }
    fn abytes(self) -> usize {
        match self {
            Aead::Aegis128L | Aead::Aegis256 => 32,
            _ => 16,
        }
    }
}

/// crypto_aead_chacha20poly1305_encrypt_detached()'s MAC (the original
/// construction: no padding, lengths after each part).
fn chacha20poly1305_mac(ad: &[u8], c: &[u8], npub: &[u8; 8], key: &[u8; 32]) -> [u8; 16] {
    let mut block0 = [0u8; 64];
    salsa::chacha20_xor_ic(&mut block0, npub, 0, key);
    let mut p = Poly1305::new(&arr::<32>(&block0));
    block0.fill(0);
    p.update(ad);
    p.update(&(ad.len() as u64).to_le_bytes());
    p.update(c);
    p.update(&(c.len() as u64).to_le_bytes());
    p.finish()
}

/// (ciphertext || tag)
fn aead_encrypt(alg: Aead, m: &[u8], ad: &[u8], npub: &[u8], key: &[u8]) -> Vec<u8> {
    let mut buf = m.to_vec();
    let tag: Vec<u8> = match alg {
        Aead::Aes256Gcm => Aes256Gcm::new(Aes256::new(&arr(key)))
            .encrypt(npub, ad, &mut buf)
            .to_vec(),
        Aead::Aegis128L => Aegis128L::new(&arr(key))
            .encrypt_tag256(&arr(npub), ad, &mut buf)
            .to_vec(),
        Aead::Aegis256 => Aegis256::new(&arr(key))
            .encrypt_tag256(&arr(npub), ad, &mut buf)
            .to_vec(),
        Aead::ChaCha20Poly1305 => {
            let (n, k) = (arr::<8>(npub), arr::<32>(key));
            salsa::chacha20_xor_ic(&mut buf, &n, 1, &k);
            chacha20poly1305_mac(ad, &buf, &n, &k).to_vec()
        }
        Aead::ChaCha20Poly1305Ietf => ChaCha20Poly1305::new(&arr(key))
            .encrypt(&arr(npub), ad, &mut buf)
            .to_vec(),
        Aead::XChaCha20Poly1305Ietf => XChaCha20Poly1305::new(&arr(key))
            .encrypt(&arr(npub), ad, &mut buf)
            .to_vec(),
    };
    buf.extend_from_slice(&tag);
    buf
}

fn aead_decrypt(alg: Aead, c: &[u8], ad: &[u8], npub: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let ab = alg.abytes();
    let (body, tag) = c.split_at(c.len() - ab);
    let mut buf = body.to_vec();
    let ok = match alg {
        Aead::Aes256Gcm => Aes256Gcm::new(Aes256::new(&arr(key)))
            .decrypt(npub, ad, &mut buf, &arr(tag))
            .is_ok(),
        Aead::Aegis128L => Aegis128L::new(&arr(key))
            .decrypt_tag256(&arr(npub), ad, &mut buf, &arr(tag))
            .is_ok(),
        Aead::Aegis256 => Aegis256::new(&arr(key))
            .decrypt_tag256(&arr(npub), ad, &mut buf, &arr(tag))
            .is_ok(),
        Aead::ChaCha20Poly1305 => {
            let (n, k) = (arr::<8>(npub), arr::<32>(key));
            let mac = chacha20poly1305_mac(ad, &buf, &n, &k);
            if codecs::memcmp(&mac, tag) == 0 {
                salsa::chacha20_xor_ic(&mut buf, &n, 1, &k);
                true
            } else {
                false
            }
        }
        Aead::ChaCha20Poly1305Ietf => ChaCha20Poly1305::new(&arr(key))
            .decrypt(&arr(npub), ad, &mut buf, &arr(tag))
            .is_ok(),
        Aead::XChaCha20Poly1305Ietf => XChaCha20Poly1305::new(&arr(key))
            .decrypt(&arr(npub), ad, &mut buf, &arr(tag))
            .is_ok(),
    };
    if ok {
        Some(buf)
    } else {
        buf.fill(0);
        None
    }
}

fn aead_args_check(vm: &mut Vm, f: &str, alg: Aead, a: &[A]) -> bool {
    if a[2].s().len() != alg.npubbytes() {
        let _ = arg_error(
            vm,
            f,
            3,
            &format!(
                "must be SODIUM_CRYPTO_AEAD_{}_NPUBBYTES bytes long",
                alg.name()
            ),
        );
        return false;
    }
    if a[3].s().len() != alg.keybytes() {
        let _ = arg_error(
            vm,
            f,
            4,
            &format!(
                "must be SODIUM_CRYPTO_AEAD_{}_KEYBYTES bytes long",
                alg.name()
            ),
        );
        return false;
    }
    true
}

fn aead_encrypt_fn(vm: &mut Vm, alg: Aead, args: &[Value]) -> R {
    let f = format!("sodium_crypto_aead_{}_encrypt", alg.fname());
    let a = args!(vm, &f, "ssss", args);
    if !aead_args_check(vm, &f, alg, &a) {
        return Ok(Value::Null);
    }
    let mlen = a[0].s().len() as u64;
    if ZSTR_MAX_LEN - mlen <= alg.abytes() as u64 {
        return throw(vm, "arithmetic overflow");
    }
    match alg {
        Aead::Aes256Gcm if mlen > 16 * ((1u64 << 32) - 2) - 16 => {
            return throw(vm, "message too long for a single key");
        }
        Aead::ChaCha20Poly1305Ietf if mlen > 64 * (1u64 << 32) - 64 => {
            return throw(vm, "message too long for a single key");
        }
        _ => {}
    }
    Ok(sval(aead_encrypt(
        alg,
        a[0].s(),
        a[1].s(),
        a[2].s(),
        a[3].s(),
    )))
}

fn aead_decrypt_fn(vm: &mut Vm, alg: Aead, args: &[Value]) -> R {
    let f = format!("sodium_crypto_aead_{}_decrypt", alg.fname());
    let a = args!(vm, &f, "ssss", args);
    if !aead_args_check(vm, &f, alg, &a) {
        return Ok(Value::Null);
    }
    let clen = a[0].s().len() as u64;
    let ab = alg.abytes() as u64;
    if alg == Aead::ChaCha20Poly1305Ietf {
        if clen >= ZSTR_MAX_LEN {
            return throw(vm, "arithmetic overflow");
        }
        if clen < ab {
            return Ok(Value::False);
        }
        if clen - ab > 64 * (1u64 << 32) - 64 {
            return throw(vm, "message too long for a single key");
        }
    } else {
        if clen < ab {
            return Ok(Value::False);
        }
        match alg {
            Aead::Aes256Gcm if clen - ab > 16 * ((1u64 << 32) - 2) => {
                return arg_error(vm, &f, 1, "is too long");
            }
            Aead::XChaCha20Poly1305Ietf => {
                if clen - ab >= ZSTR_MAX_LEN || clen - ab > 64 * (1u64 << 32) - 64 {
                    return throw(vm, "arithmetic overflow");
                }
            }
            _ => {}
        }
        if clen >= ZSTR_MAX_LEN {
            return throw(vm, "arithmetic overflow");
        }
    }
    match aead_decrypt(alg, a[0].s(), a[1].s(), a[2].s(), a[3].s()) {
        Some(m) => Ok(sval(m)),
        None => Ok(Value::False),
    }
}

fn sodium_crypto_aead_aes256gcm_is_available(vm: &mut Vm, args: &[Value]) -> R {
    none!(vm, "sodium_crypto_aead_aes256gcm_is_available", args);
    Ok(Value::True)
}

macro_rules! aead_fns {
    ($alg:expr, $enc:ident, $dec:ident, $kg:ident) => {
        fn $enc(vm: &mut Vm, args: &[Value]) -> R {
            aead_encrypt_fn(vm, $alg, args)
        }
        fn $dec(vm: &mut Vm, args: &[Value]) -> R {
            aead_decrypt_fn(vm, $alg, args)
        }
        fn $kg(vm: &mut Vm, args: &[Value]) -> R {
            keygen(vm, stringify!($kg), args, $alg.keybytes())
        }
    };
}

aead_fns!(
    Aead::Aes256Gcm,
    sodium_crypto_aead_aes256gcm_encrypt,
    sodium_crypto_aead_aes256gcm_decrypt,
    sodium_crypto_aead_aes256gcm_keygen
);
aead_fns!(
    Aead::Aegis128L,
    sodium_crypto_aead_aegis128l_encrypt,
    sodium_crypto_aead_aegis128l_decrypt,
    sodium_crypto_aead_aegis128l_keygen
);
aead_fns!(
    Aead::Aegis256,
    sodium_crypto_aead_aegis256_encrypt,
    sodium_crypto_aead_aegis256_decrypt,
    sodium_crypto_aead_aegis256_keygen
);
aead_fns!(
    Aead::ChaCha20Poly1305,
    sodium_crypto_aead_chacha20poly1305_encrypt,
    sodium_crypto_aead_chacha20poly1305_decrypt,
    sodium_crypto_aead_chacha20poly1305_keygen
);
aead_fns!(
    Aead::ChaCha20Poly1305Ietf,
    sodium_crypto_aead_chacha20poly1305_ietf_encrypt,
    sodium_crypto_aead_chacha20poly1305_ietf_decrypt,
    sodium_crypto_aead_chacha20poly1305_ietf_keygen
);
aead_fns!(
    Aead::XChaCha20Poly1305Ietf,
    sodium_crypto_aead_xchacha20poly1305_ietf_encrypt,
    sodium_crypto_aead_xchacha20poly1305_ietf_decrypt,
    sodium_crypto_aead_xchacha20poly1305_ietf_keygen
);

// ---------------------------------------------------------------------------
// secretstream_xchacha20poly1305 (state: k[32], nonce[12] = counter[4] ||
// inonce[8], pad[8])

fn ss_counter_reset(st: &mut [u8]) {
    st[32..36].copy_from_slice(&[1, 0, 0, 0]);
}

fn ss_rekey(st: &mut [u8]) {
    let mut nk = [0u8; 40];
    nk[..32].copy_from_slice(&st[..32]);
    nk[32..].copy_from_slice(&st[36..44]);
    let nonce: [u8; 12] = arr(&st[32..44]);
    let key: [u8; 32] = arr(&st[..32]);
    salsa::chacha20_ietf_xor_ic(&mut nk, &nonce, 0, &key);
    st[..32].copy_from_slice(&nk[..32]);
    st[36..44].copy_from_slice(&nk[32..]);
    nk.fill(0);
    ss_counter_reset(st);
}

fn ss_init(st: &mut [u8], header: &[u8], key: &[u8]) {
    let k = salsa::hchacha20(&arr(key), &arr(header));
    st[..32].copy_from_slice(&k);
    ss_counter_reset(st);
    st[36..44].copy_from_slice(&header[16..24]);
    st[44..52].fill(0);
}

/// The poly1305 MAC of a message (`c` = the encrypted tag block's first
/// byte has been put in `block`).
fn ss_mac(ad: &[u8], block: &[u8; 64], c: &[u8], polykey: &[u8; 32]) -> [u8; 16] {
    const PAD0: [u8; 16] = [0; 16];
    let mut p = Poly1305::new(polykey);
    p.update(ad);
    p.update(&PAD0[..(0x10 - ad.len() % 16) % 16]);
    p.update(block);
    p.update(c);
    p.update(&PAD0[..(0x10usize.wrapping_sub(64).wrapping_add(c.len())) & 0xf]);
    p.update(&(ad.len() as u64).to_le_bytes());
    p.update(&((64 + c.len()) as u64).to_le_bytes());
    p.finish()
}

fn ss_after(st: &mut [u8], mac: &[u8; 16], tag: u8) {
    for i in 0..8 {
        st[36 + i] ^= mac[i];
    }
    codecs::increment(&mut st[32..36]);
    if tag & TAG_REKEY != 0 || codecs::is_zero(&st[32..36]) {
        ss_rekey(st);
    }
}

fn ss_push(st: &mut [u8], m: &[u8], ad: &[u8], tag: u8) -> Vec<u8> {
    let nonce: [u8; 12] = arr(&st[32..44]);
    let key: [u8; 32] = arr(&st[..32]);
    let mut polykey = [0u8; 64];
    salsa::chacha20_ietf_xor_ic(&mut polykey, &nonce, 0, &key);
    let mut block = [0u8; 64];
    block[0] = tag;
    salsa::chacha20_ietf_xor_ic(&mut block, &nonce, 1, &key);
    let mut out = Vec::with_capacity(1 + m.len() + 16);
    out.push(block[0]);
    let mut c = m.to_vec();
    salsa::chacha20_ietf_xor_ic(&mut c, &nonce, 2, &key);
    let mac = ss_mac(ad, &block, &c, &arr(&polykey));
    polykey.fill(0);
    out.extend_from_slice(&c);
    out.extend_from_slice(&mac);
    ss_after(st, &mac, tag);
    out
}

fn ss_pull(st: &mut [u8], input: &[u8], ad: &[u8]) -> Option<(Vec<u8>, u8)> {
    if input.len() < SECRETSTREAM_ABYTES {
        return None;
    }
    let mlen = input.len() - SECRETSTREAM_ABYTES;
    let nonce: [u8; 12] = arr(&st[32..44]);
    let key: [u8; 32] = arr(&st[..32]);
    let mut polykey = [0u8; 64];
    salsa::chacha20_ietf_xor_ic(&mut polykey, &nonce, 0, &key);
    let mut block = [0u8; 64];
    block[0] = input[0];
    salsa::chacha20_ietf_xor_ic(&mut block, &nonce, 1, &key);
    let tag = block[0];
    block[0] = input[0];
    let c = &input[1..1 + mlen];
    let mac = ss_mac(ad, &block, c, &arr(&polykey));
    polykey.fill(0);
    if codecs::memcmp(&mac, &input[1 + mlen..]) != 0 {
        return None;
    }
    let mut m = c.to_vec();
    salsa::chacha20_ietf_xor_ic(&mut m, &nonce, 2, &key);
    ss_after(st, &mac, tag);
    Some((m, tag))
}

fn sodium_crypto_secretstream_xchacha20poly1305_keygen(vm: &mut Vm, args: &[Value]) -> R {
    keygen(
        vm,
        "sodium_crypto_secretstream_xchacha20poly1305_keygen",
        args,
        32,
    )
}

fn sodium_crypto_secretstream_xchacha20poly1305_init_push(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_secretstream_xchacha20poly1305_init_push";
    let a = args!(vm, f, "s", args);
    if a[0].s().len() != 32 {
        return arg_error(
            vm,
            f,
            1,
            "must be SODIUM_CRYPTO_SECRETSTREAM_XCHACHA20POLY1305_KEYBYTES bytes long",
        );
    }
    let mut header = [0u8; SECRETSTREAM_HEADERBYTES];
    random_bytes(&mut header);
    let mut st = vec![0u8; SECRETSTREAM_STATEBYTES];
    ss_init(&mut st, &header, a[0].s());
    Ok(list(vec![sval(st), sval(header.to_vec())]))
}

/// The state of a by-reference secretstream state argument, with the
/// check of the given function.
fn ss_state(vm: &mut Vm, f: &str, z: &Value, push: bool) -> Option<Vec<u8>> {
    let st = state_of(vm, f, z)?;
    if st.len() != SECRETSTREAM_STATEBYTES {
        if push {
            let _ = arg_error(vm, f, 1, "must have a correct length");
        } else {
            let _ = throw(vm, "incorrect state length");
        }
        return None;
    }
    Some(st)
}

fn sodium_crypto_secretstream_xchacha20poly1305_push(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_secretstream_xchacha20poly1305_push";
    let a = args!(vm, f, "zs|sl", args);
    let z = a[0].z();
    let Some(mut st) = ss_state(vm, f, &z, true) else {
        return Ok(Value::Null);
    };
    let m = a[1].s();
    if m.len() as u64 > SECRETSTREAM_MESSAGEBYTES_MAX
        || m.len() as u64 > ZSTR_MAX_LEN - SECRETSTREAM_ABYTES as u64
    {
        return arg_error(
            vm,
            f,
            2,
            "must be at most SODIUM_CRYPTO_SECRETSTREAM_XCHACHA20POLY1305_MESSAGEBYTES_MAX bytes long",
        );
    }
    let tag = a[3].l_or(TAG_MESSAGE);
    if !(0..=255).contains(&tag) {
        return arg_error(vm, f, 4, "must be in the range of 0-255");
    }
    let c = ss_push(&mut st, m, a[2].s(), tag as u8);
    write_ref(&z, sval(st));
    Ok(sval(c))
}

fn sodium_crypto_secretstream_xchacha20poly1305_init_pull(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_secretstream_xchacha20poly1305_init_pull";
    let a = args!(vm, f, "ss", args);
    if a[0].s().len() != SECRETSTREAM_HEADERBYTES {
        return arg_error(
            vm,
            f,
            1,
            "must be SODIUM_CRYPTO_SECRETSTREAM_XCHACHA20POLY1305_HEADERBYTES bytes long",
        );
    }
    if a[1].s().len() != 32 {
        return arg_error(
            vm,
            f,
            2,
            "must be SODIUM_CRYPTO_SECRETSTREAM_XCHACHA20POLY1305_KEYBYTES bytes long",
        );
    }
    let mut st = vec![0u8; SECRETSTREAM_STATEBYTES];
    ss_init(&mut st, a[0].s(), a[1].s());
    Ok(sval(st))
}

fn sodium_crypto_secretstream_xchacha20poly1305_pull(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_secretstream_xchacha20poly1305_pull";
    let a = args!(vm, f, "zs|s", args);
    let z = a[0].z();
    let Some(mut st) = ss_state(vm, f, &z, false) else {
        return Ok(Value::Null);
    };
    if a[1].s().len() < SECRETSTREAM_ABYTES {
        return Ok(Value::False);
    }
    let r = ss_pull(&mut st, a[1].s(), a[2].s());
    write_ref(&z, sval(st));
    match r {
        Some((m, tag)) => Ok(list(vec![sval(m), Value::Long(tag as i64)])),
        None => Ok(Value::False),
    }
}

fn sodium_crypto_secretstream_xchacha20poly1305_rekey(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_secretstream_xchacha20poly1305_rekey";
    let a = args!(vm, f, "z", args);
    let z = a[0].z();
    let Some(mut st) = ss_state(vm, f, &z, false) else {
        return Ok(Value::Null);
    };
    ss_rekey(&mut st);
    write_ref(&z, sval(st));
    Ok(Value::Null)
}

// ---------------------------------------------------------------------------
// pwhash

fn sodium_crypto_pwhash(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_pwhash";
    let a = args!(vm, f, "lssll|l", args);
    let hash_len = a[0].l();
    let passwd = a[1].s();
    let salt = a[2].s();
    let opslimit = a[3].l();
    let memlimit = a[4].l();
    let alg = a[5].l_or(pwhash::ALG_DEFAULT);
    if hash_len <= 0 {
        return arg_error(vm, f, 1, "must be greater than 0");
    }
    if hash_len >= 0xffff_ffff {
        return arg_error(vm, f, 1, "is too large");
    }
    if passwd.len() as u64 >= 0xffff_ffff {
        return arg_error(vm, f, 2, "is too long");
    }
    if opslimit <= 0 {
        return arg_error(vm, f, 4, "must be greater than 0");
    }
    if memlimit <= 0 {
        return arg_error(vm, f, 5, "must be greater than 0");
    }
    if alg != pwhash::ALG_ARGON2I13 && alg != pwhash::ALG_ARGON2ID13 && alg != pwhash::ALG_DEFAULT {
        return throw(vm, "unsupported password hashing algorithm");
    }
    if passwd.is_empty() {
        vm.emit_warning("empty password");
    }
    if salt.len() != pwhash::SALTBYTES {
        return arg_error(
            vm,
            f,
            3,
            "must be SODIUM_CRYPTO_PWHASH_SALTBYTES bytes long",
        );
    }
    if opslimit < pwhash::OPSLIMIT_MIN {
        return arg_error(
            vm,
            f,
            4,
            &format!("must be greater than or equal to {}", pwhash::OPSLIMIT_MIN),
        );
    }
    if (memlimit as u64) < pwhash::MEMLIMIT_MIN {
        // (no return: the hashing below fails too)
        let _ = arg_error(
            vm,
            f,
            5,
            &format!("must be greater than or equal to {}", pwhash::MEMLIMIT_MIN),
        );
    }
    alloc_check(vm, hash_len as usize)?;
    match pwhash::pwhash(
        hash_len as u64,
        passwd,
        salt,
        opslimit as u64,
        memlimit as u64,
        alg,
    ) {
        Ok(h) => {
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            Ok(sval(h))
        }
        Err(()) => throw(vm, "internal error"),
    }
}

fn sodium_crypto_pwhash_str(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_pwhash_str";
    let a = args!(vm, f, "sll", args);
    let passwd = a[0].s();
    let opslimit = a[1].l();
    let memlimit = a[2].l();
    if opslimit <= 0 {
        return arg_error(vm, f, 2, "must be greater than 0");
    }
    if memlimit <= 0 {
        return arg_error(vm, f, 3, "must be greater than 0");
    }
    if passwd.len() as u64 >= 0xffff_ffff {
        return arg_error(vm, f, 1, "is too long");
    }
    if passwd.is_empty() {
        vm.emit_warning("empty password");
    }
    if opslimit < pwhash::OPSLIMIT_MIN {
        let _ = arg_error(
            vm,
            f,
            2,
            &format!("must be greater than or equal to {}", pwhash::OPSLIMIT_MIN),
        );
    }
    if (memlimit as u64) < pwhash::MEMLIMIT_MIN {
        let _ = arg_error(
            vm,
            f,
            3,
            &format!("must be greater than or equal to {}", pwhash::MEMLIMIT_MIN),
        );
    }
    match pwhash::pwhash_str(
        passwd,
        opslimit as u64,
        memlimit as u64,
        pwhash::ALG_DEFAULT,
    ) {
        Ok(h) => {
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            Ok(sval(h))
        }
        Err(()) => throw(vm, "internal error"),
    }
}

fn sodium_crypto_pwhash_str_needs_rehash(vm: &mut Vm, args: &[Value]) -> R {
    let Some(a) = zpp(
        vm,
        "sodium_crypto_pwhash_str_needs_rehash",
        "sll",
        args,
        false,
    ) else {
        return Ok(Value::Null);
    };
    Ok(bv(pwhash::str_needs_rehash(
        a[0].s(),
        a[1].l() as u64,
        a[2].l() as u64,
    ) != 0))
}

fn sodium_crypto_pwhash_str_verify(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_pwhash_str_verify";
    let a = args!(vm, f, "ss", args);
    let passwd = a[1].s();
    if passwd.len() as u64 >= 0xffff_ffff {
        return arg_error(vm, f, 2, "is too long");
    }
    if passwd.is_empty() {
        vm.emit_warning("empty password");
    }
    Ok(bv(pwhash::str_verify(a[0].s(), passwd)))
}

fn sodium_crypto_pwhash_scryptsalsa208sha256(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_pwhash_scryptsalsa208sha256";
    let a = args!(vm, f, "lssll", args);
    let hash_len = a[0].l();
    let passwd = a[1].s();
    let salt = a[2].s();
    let opslimit = a[3].l();
    let memlimit = a[4].l();
    if hash_len <= 0 || hash_len as u64 >= ZSTR_MAX_LEN || hash_len as u64 > 0x1f_ffff_ffe0 {
        return arg_error(vm, f, 1, "must be greater than 0");
    }
    if opslimit <= 0 {
        return arg_error(vm, f, 4, "must be greater than 0");
    }
    if memlimit <= 0 {
        return arg_error(vm, f, 5, "must be greater than 0");
    }
    if passwd.is_empty() {
        vm.emit_warning("empty password");
    }
    if salt.len() != pwhash::scrypt::SALTBYTES {
        return arg_error(
            vm,
            f,
            3,
            "must be SODIUM_CRYPTO_PWHASH_SCRYPTSALSA208SHA256_SALTBYTES bytes long",
        );
    }
    if opslimit < pwhash::scrypt::OPSLIMIT_INTERACTIVE {
        let _ = arg_error(
            vm,
            f,
            4,
            &format!(
                "must be greater than or equal to {}",
                pwhash::scrypt::OPSLIMIT_INTERACTIVE
            ),
        );
    }
    if memlimit < pwhash::scrypt::MEMLIMIT_INTERACTIVE {
        let _ = arg_error(
            vm,
            f,
            5,
            &format!(
                "must be greater than or equal to {}",
                pwhash::scrypt::MEMLIMIT_INTERACTIVE
            ),
        );
    }
    alloc_check(vm, hash_len as usize)?;
    match pwhash::scrypt::pwhash(
        hash_len as u64,
        passwd,
        salt,
        opslimit as u64,
        memlimit as u64,
    ) {
        Ok(h) => {
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            Ok(sval(h))
        }
        Err(()) => throw(vm, "internal error"),
    }
}

fn sodium_crypto_pwhash_scryptsalsa208sha256_str(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_pwhash_scryptsalsa208sha256_str";
    let a = args!(vm, f, "sll", args);
    let passwd = a[0].s();
    let opslimit = a[1].l();
    let memlimit = a[2].l();
    if opslimit <= 0 {
        return arg_error(vm, f, 2, "must be greater than 0");
    }
    if memlimit <= 0 {
        return arg_error(vm, f, 3, "must be greater than 0");
    }
    if passwd.is_empty() {
        vm.emit_warning("empty password");
    }
    if opslimit < pwhash::scrypt::OPSLIMIT_INTERACTIVE {
        let _ = arg_error(
            vm,
            f,
            2,
            &format!(
                "must be greater than or equal to {}",
                pwhash::scrypt::OPSLIMIT_INTERACTIVE
            ),
        );
    }
    if memlimit < pwhash::scrypt::MEMLIMIT_INTERACTIVE {
        let _ = arg_error(
            vm,
            f,
            3,
            &format!(
                "must be greater than or equal to {}",
                pwhash::scrypt::MEMLIMIT_INTERACTIVE
            ),
        );
    }
    match pwhash::scrypt::pwhash_str(passwd, opslimit as u64, memlimit as u64) {
        Ok(h) => {
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            Ok(sval(h))
        }
        Err(()) => throw(vm, "internal error"),
    }
}

fn sodium_crypto_pwhash_scryptsalsa208sha256_str_verify(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_pwhash_scryptsalsa208sha256_str_verify";
    let a = args!(vm, f, "ss", args);
    let passwd = a[1].s();
    if passwd.is_empty() {
        vm.emit_warning("empty password");
    }
    if a[0].s().len() != pwhash::scrypt::STRBYTES - 1 {
        vm.emit_warning("wrong size for the hashed password");
        return Ok(Value::False);
    }
    Ok(bv(pwhash::scrypt::str_verify(a[0].s(), passwd)))
}

// ---------------------------------------------------------------------------
// ristretto255

fn point(b: &[u8]) -> Option<RistrettoPoint> {
    CompressedRistretto::from_slice(&arr(b)).decompress()
}

fn encode(p: &RistrettoPoint) -> Vec<u8> {
    p.compress().to_bytes().to_vec()
}

/// A 32-byte (possibly non-reduced) scalar, reduced modulo L.
fn scalar_wide(b: &[u8]) -> Scalar {
    let mut w = [0u8; 64];
    w[..32].copy_from_slice(&b[..32]);
    Scalar::from_bytes_mod_order(&w)
}

/// crypto_core_ed25519_scalar_negate()
fn scalar_negate(s: &[u8]) -> [u8; 32] {
    scalar_wide(s).negate().to_bytes()
}

/// crypto_core_ed25519_scalar_add(): the sum is taken modulo 2^256 before
/// the reduction (sodium_add() over 32 bytes).
fn scalar_add(x: &[u8], y: &[u8]) -> [u8; 32] {
    let mut t: [u8; 32] = arr(x);
    codecs::add(&mut t, y);
    scalar_wide(&t).to_bytes()
}

fn ristretto_point_args(
    vm: &mut Vm,
    f: &str,
    args: &[Value],
    what: &str,
    n: usize,
) -> Option<Vec<A>> {
    let a = zpp(vm, f, if n == 2 { "ss" } else { "s" }, args, true)?;
    let len = if what.ends_with("NONREDUCEDSCALARBYTES") || what.ends_with("HASHBYTES") {
        64
    } else {
        32
    };
    for (i, x) in a.iter().enumerate() {
        if x.s().len() != len {
            let _ = arg_error(vm, f, i + 1, &format!("must be {} bytes long", what));
            return None;
        }
    }
    Some(a)
}

macro_rules! rargs {
    ($vm:expr, $f:expr, $args:expr, $what:expr, $n:expr) => {
        match ristretto_point_args($vm, $f, $args, $what, $n) {
            Some(a) => a,
            None => return Ok(Value::Null),
        }
    };
}

const R_BYTES: &str = "SODIUM_CRYPTO_CORE_RISTRETTO255_BYTES";
const R_SCALARBYTES: &str = "SODIUM_CRYPTO_CORE_RISTRETTO255_SCALARBYTES";

fn sodium_crypto_core_ristretto255_add(vm: &mut Vm, args: &[Value]) -> R {
    let a = rargs!(vm, "sodium_crypto_core_ristretto255_add", args, R_BYTES, 2);
    match (point(a[0].s()), point(a[1].s())) {
        (Some(p), Some(q)) => Ok(sval(encode(&p.add(&q)))),
        _ => throw(vm, "internal error"),
    }
}

fn sodium_crypto_core_ristretto255_sub(vm: &mut Vm, args: &[Value]) -> R {
    let a = rargs!(vm, "sodium_crypto_core_ristretto255_sub", args, R_BYTES, 2);
    match (point(a[0].s()), point(a[1].s())) {
        (Some(p), Some(q)) => Ok(sval(encode(&p.sub(&q)))),
        _ => throw(vm, "internal error"),
    }
}

fn sodium_crypto_core_ristretto255_from_hash(vm: &mut Vm, args: &[Value]) -> R {
    let a = rargs!(
        vm,
        "sodium_crypto_core_ristretto255_from_hash",
        args,
        "SODIUM_CRYPTO_CORE_RISTRETTO255_HASHBYTES",
        1
    );
    Ok(sval(encode(&RistrettoPoint::from_uniform_bytes(&arr(a
        [0]
    .s())))))
}

fn sodium_crypto_core_ristretto255_is_valid_point(vm: &mut Vm, args: &[Value]) -> R {
    let a = rargs!(
        vm,
        "sodium_crypto_core_ristretto255_is_valid_point",
        args,
        R_BYTES,
        1
    );
    Ok(bv(point(a[0].s()).is_some()))
}

fn sodium_crypto_core_ristretto255_random(vm: &mut Vm, args: &[Value]) -> R {
    none!(vm, "sodium_crypto_core_ristretto255_random", args);
    let mut h = [0u8; 64];
    random_bytes(&mut h);
    Ok(sval(encode(&RistrettoPoint::from_uniform_bytes(&h))))
}

fn sodium_crypto_core_ristretto255_scalar_add(vm: &mut Vm, args: &[Value]) -> R {
    let a = rargs!(
        vm,
        "sodium_crypto_core_ristretto255_scalar_add",
        args,
        R_SCALARBYTES,
        2
    );
    Ok(sval(scalar_add(a[0].s(), a[1].s()).to_vec()))
}

fn sodium_crypto_core_ristretto255_scalar_sub(vm: &mut Vm, args: &[Value]) -> R {
    let a = rargs!(
        vm,
        "sodium_crypto_core_ristretto255_scalar_sub",
        args,
        R_SCALARBYTES,
        2
    );
    let yn = scalar_negate(a[1].s());
    Ok(sval(scalar_add(a[0].s(), &yn).to_vec()))
}

fn sodium_crypto_core_ristretto255_scalar_mul(vm: &mut Vm, args: &[Value]) -> R {
    let a = rargs!(
        vm,
        "sodium_crypto_core_ristretto255_scalar_mul",
        args,
        R_SCALARBYTES,
        2
    );
    let r = scalar_wide(a[0].s()).mul(&scalar_wide(a[1].s()));
    Ok(sval(r.to_bytes().to_vec()))
}

fn sodium_crypto_core_ristretto255_scalar_negate(vm: &mut Vm, args: &[Value]) -> R {
    let a = rargs!(
        vm,
        "sodium_crypto_core_ristretto255_scalar_negate",
        args,
        R_SCALARBYTES,
        1
    );
    Ok(sval(scalar_negate(a[0].s()).to_vec()))
}

fn sodium_crypto_core_ristretto255_scalar_complement(vm: &mut Vm, args: &[Value]) -> R {
    let a = rargs!(
        vm,
        "sodium_crypto_core_ristretto255_scalar_complement",
        args,
        R_SCALARBYTES,
        1
    );
    let r = Scalar::ONE.sub(&scalar_wide(a[0].s()));
    Ok(sval(r.to_bytes().to_vec()))
}

fn sodium_crypto_core_ristretto255_scalar_invert(vm: &mut Vm, args: &[Value]) -> R {
    let a = rargs!(
        vm,
        "sodium_crypto_core_ristretto255_scalar_invert",
        args,
        R_SCALARBYTES,
        1
    );
    if codecs::is_zero(a[0].s()) {
        return throw(vm, "internal error");
    }
    Ok(sval(scalar_wide(a[0].s()).invert().to_bytes().to_vec()))
}

fn sodium_crypto_core_ristretto255_scalar_random(vm: &mut Vm, args: &[Value]) -> R {
    none!(vm, "sodium_crypto_core_ristretto255_scalar_random", args);
    loop {
        let mut r = [0u8; 32];
        random_bytes(&mut r);
        r[31] &= 0x1f;
        if !codecs::is_zero(&r) {
            if let Some(s) = Scalar::from_bytes_canonical(&r) {
                return Ok(sval(s.to_bytes().to_vec()));
            }
        }
    }
}

fn sodium_crypto_core_ristretto255_scalar_reduce(vm: &mut Vm, args: &[Value]) -> R {
    let a = rargs!(
        vm,
        "sodium_crypto_core_ristretto255_scalar_reduce",
        args,
        "SODIUM_CRYPTO_CORE_RISTRETTO255_NONREDUCEDSCALARBYTES",
        1
    );
    Ok(sval(
        Scalar::from_bytes_mod_order(&arr(a[0].s()))
            .to_bytes()
            .to_vec(),
    ))
}

fn sodium_crypto_scalarmult_ristretto255(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_scalarmult_ristretto255";
    let a = args!(vm, f, "ss", args);
    if a[0].s().len() != 32 {
        return arg_error(
            vm,
            f,
            1,
            "must be SODIUM_CRYPTO_SCALARMULT_RISTRETTO255_SCALARBYTES bytes long",
        );
    }
    if a[1].s().len() != 32 {
        return arg_error(
            vm,
            f,
            2,
            "must be SODIUM_CRYPTO_SCALARMULT_RISTRETTO255_BYTES bytes long",
        );
    }
    let Some(p) = point(a[1].s()) else {
        return throw(vm, "Result is identity element");
    };
    let mut t: [u8; 32] = arr(a[0].s());
    t[31] &= 127;
    let q = encode(&p.mul(&scalar_wide(&t)));
    if codecs::is_zero(&q) {
        return throw(vm, "Result is identity element");
    }
    Ok(sval(q))
}

fn sodium_crypto_scalarmult_ristretto255_base(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sodium_crypto_scalarmult_ristretto255_base";
    let a = args!(vm, f, "s", args);
    if a[0].s().len() != 32 {
        return arg_error(
            vm,
            f,
            1,
            "must be SODIUM_CRYPTO_SCALARMULT_RISTRETTO255_SCALARBYTES bytes long",
        );
    }
    let mut t: [u8; 32] = arr(a[0].s());
    t[31] &= 127;
    let q = encode(&RistrettoPoint::mul_base(&scalar_wide(&t)));
    if codecs::is_zero(&q) {
        return arg_error(vm, f, 1, "must not be zero");
    }
    Ok(sval(q))
}

fn sodium_crypto_shorthash_keygen(vm: &mut Vm, args: &[Value]) -> R {
    keygen(vm, "sodium_crypto_shorthash_keygen", args, 16)
}

type F = fn(&mut Vm, &[Value]) -> Result<Value, VmError>;

/// The functions, in libsodium_arginfo.h's order.
pub const FUNCTIONS: &[(&str, F)] = &[
    (
        "sodium_crypto_aead_aes256gcm_is_available",
        sodium_crypto_aead_aes256gcm_is_available,
    ),
    (
        "sodium_crypto_aead_aes256gcm_decrypt",
        sodium_crypto_aead_aes256gcm_decrypt,
    ),
    (
        "sodium_crypto_aead_aes256gcm_encrypt",
        sodium_crypto_aead_aes256gcm_encrypt,
    ),
    (
        "sodium_crypto_aead_aes256gcm_keygen",
        sodium_crypto_aead_aes256gcm_keygen,
    ),
    (
        "sodium_crypto_aead_aegis128l_decrypt",
        sodium_crypto_aead_aegis128l_decrypt,
    ),
    (
        "sodium_crypto_aead_aegis128l_encrypt",
        sodium_crypto_aead_aegis128l_encrypt,
    ),
    (
        "sodium_crypto_aead_aegis128l_keygen",
        sodium_crypto_aead_aegis128l_keygen,
    ),
    (
        "sodium_crypto_aead_aegis256_decrypt",
        sodium_crypto_aead_aegis256_decrypt,
    ),
    (
        "sodium_crypto_aead_aegis256_encrypt",
        sodium_crypto_aead_aegis256_encrypt,
    ),
    (
        "sodium_crypto_aead_aegis256_keygen",
        sodium_crypto_aead_aegis256_keygen,
    ),
    (
        "sodium_crypto_aead_chacha20poly1305_decrypt",
        sodium_crypto_aead_chacha20poly1305_decrypt,
    ),
    (
        "sodium_crypto_aead_chacha20poly1305_encrypt",
        sodium_crypto_aead_chacha20poly1305_encrypt,
    ),
    (
        "sodium_crypto_aead_chacha20poly1305_keygen",
        sodium_crypto_aead_chacha20poly1305_keygen,
    ),
    (
        "sodium_crypto_aead_chacha20poly1305_ietf_decrypt",
        sodium_crypto_aead_chacha20poly1305_ietf_decrypt,
    ),
    (
        "sodium_crypto_aead_chacha20poly1305_ietf_encrypt",
        sodium_crypto_aead_chacha20poly1305_ietf_encrypt,
    ),
    (
        "sodium_crypto_aead_chacha20poly1305_ietf_keygen",
        sodium_crypto_aead_chacha20poly1305_ietf_keygen,
    ),
    (
        "sodium_crypto_aead_xchacha20poly1305_ietf_decrypt",
        sodium_crypto_aead_xchacha20poly1305_ietf_decrypt,
    ),
    (
        "sodium_crypto_aead_xchacha20poly1305_ietf_keygen",
        sodium_crypto_aead_xchacha20poly1305_ietf_keygen,
    ),
    (
        "sodium_crypto_aead_xchacha20poly1305_ietf_encrypt",
        sodium_crypto_aead_xchacha20poly1305_ietf_encrypt,
    ),
    ("sodium_crypto_auth", sodium_crypto_auth),
    ("sodium_crypto_auth_keygen", sodium_crypto_auth_keygen),
    ("sodium_crypto_auth_verify", sodium_crypto_auth_verify),
    ("sodium_crypto_box", sodium_crypto_box),
    ("sodium_crypto_box_keypair", sodium_crypto_box_keypair),
    (
        "sodium_crypto_box_seed_keypair",
        sodium_crypto_box_seed_keypair,
    ),
    (
        "sodium_crypto_box_keypair_from_secretkey_and_publickey",
        sodium_crypto_box_keypair_from_secretkey_and_publickey,
    ),
    ("sodium_crypto_box_open", sodium_crypto_box_open),
    ("sodium_crypto_box_publickey", sodium_crypto_box_publickey),
    (
        "sodium_crypto_box_publickey_from_secretkey",
        sodium_crypto_box_publickey_from_secretkey,
    ),
    ("sodium_crypto_box_seal", sodium_crypto_box_seal),
    ("sodium_crypto_box_seal_open", sodium_crypto_box_seal_open),
    ("sodium_crypto_box_secretkey", sodium_crypto_box_secretkey),
    (
        "sodium_crypto_core_ristretto255_add",
        sodium_crypto_core_ristretto255_add,
    ),
    (
        "sodium_crypto_core_ristretto255_from_hash",
        sodium_crypto_core_ristretto255_from_hash,
    ),
    (
        "sodium_crypto_core_ristretto255_is_valid_point",
        sodium_crypto_core_ristretto255_is_valid_point,
    ),
    (
        "sodium_crypto_core_ristretto255_random",
        sodium_crypto_core_ristretto255_random,
    ),
    (
        "sodium_crypto_core_ristretto255_scalar_add",
        sodium_crypto_core_ristretto255_scalar_add,
    ),
    (
        "sodium_crypto_core_ristretto255_scalar_complement",
        sodium_crypto_core_ristretto255_scalar_complement,
    ),
    (
        "sodium_crypto_core_ristretto255_scalar_invert",
        sodium_crypto_core_ristretto255_scalar_invert,
    ),
    (
        "sodium_crypto_core_ristretto255_scalar_mul",
        sodium_crypto_core_ristretto255_scalar_mul,
    ),
    (
        "sodium_crypto_core_ristretto255_scalar_negate",
        sodium_crypto_core_ristretto255_scalar_negate,
    ),
    (
        "sodium_crypto_core_ristretto255_scalar_random",
        sodium_crypto_core_ristretto255_scalar_random,
    ),
    (
        "sodium_crypto_core_ristretto255_scalar_reduce",
        sodium_crypto_core_ristretto255_scalar_reduce,
    ),
    (
        "sodium_crypto_core_ristretto255_scalar_sub",
        sodium_crypto_core_ristretto255_scalar_sub,
    ),
    (
        "sodium_crypto_core_ristretto255_sub",
        sodium_crypto_core_ristretto255_sub,
    ),
    ("sodium_crypto_kx_keypair", sodium_crypto_kx_keypair),
    ("sodium_crypto_kx_publickey", sodium_crypto_kx_publickey),
    ("sodium_crypto_kx_secretkey", sodium_crypto_kx_secretkey),
    (
        "sodium_crypto_kx_seed_keypair",
        sodium_crypto_kx_seed_keypair,
    ),
    (
        "sodium_crypto_kx_client_session_keys",
        sodium_crypto_kx_client_session_keys,
    ),
    (
        "sodium_crypto_kx_server_session_keys",
        sodium_crypto_kx_server_session_keys,
    ),
    ("sodium_crypto_generichash", sodium_crypto_generichash),
    (
        "sodium_crypto_generichash_keygen",
        sodium_crypto_generichash_keygen,
    ),
    (
        "sodium_crypto_generichash_init",
        sodium_crypto_generichash_init,
    ),
    (
        "sodium_crypto_generichash_update",
        sodium_crypto_generichash_update,
    ),
    (
        "sodium_crypto_generichash_final",
        sodium_crypto_generichash_final,
    ),
    (
        "sodium_crypto_kdf_derive_from_key",
        sodium_crypto_kdf_derive_from_key,
    ),
    ("sodium_crypto_kdf_keygen", sodium_crypto_kdf_keygen),
    ("sodium_crypto_pwhash", sodium_crypto_pwhash),
    ("sodium_crypto_pwhash_str", sodium_crypto_pwhash_str),
    (
        "sodium_crypto_pwhash_str_verify",
        sodium_crypto_pwhash_str_verify,
    ),
    (
        "sodium_crypto_pwhash_str_needs_rehash",
        sodium_crypto_pwhash_str_needs_rehash,
    ),
    (
        "sodium_crypto_pwhash_scryptsalsa208sha256",
        sodium_crypto_pwhash_scryptsalsa208sha256,
    ),
    (
        "sodium_crypto_pwhash_scryptsalsa208sha256_str",
        sodium_crypto_pwhash_scryptsalsa208sha256_str,
    ),
    (
        "sodium_crypto_pwhash_scryptsalsa208sha256_str_verify",
        sodium_crypto_pwhash_scryptsalsa208sha256_str_verify,
    ),
    ("sodium_crypto_scalarmult", sodium_crypto_scalarmult),
    (
        "sodium_crypto_scalarmult_ristretto255",
        sodium_crypto_scalarmult_ristretto255,
    ),
    (
        "sodium_crypto_scalarmult_ristretto255_base",
        sodium_crypto_scalarmult_ristretto255_base,
    ),
    ("sodium_crypto_secretbox", sodium_crypto_secretbox),
    (
        "sodium_crypto_secretbox_keygen",
        sodium_crypto_secretbox_keygen,
    ),
    ("sodium_crypto_secretbox_open", sodium_crypto_secretbox_open),
    (
        "sodium_crypto_secretstream_xchacha20poly1305_keygen",
        sodium_crypto_secretstream_xchacha20poly1305_keygen,
    ),
    (
        "sodium_crypto_secretstream_xchacha20poly1305_init_push",
        sodium_crypto_secretstream_xchacha20poly1305_init_push,
    ),
    (
        "sodium_crypto_secretstream_xchacha20poly1305_push",
        sodium_crypto_secretstream_xchacha20poly1305_push,
    ),
    (
        "sodium_crypto_secretstream_xchacha20poly1305_init_pull",
        sodium_crypto_secretstream_xchacha20poly1305_init_pull,
    ),
    (
        "sodium_crypto_secretstream_xchacha20poly1305_pull",
        sodium_crypto_secretstream_xchacha20poly1305_pull,
    ),
    (
        "sodium_crypto_secretstream_xchacha20poly1305_rekey",
        sodium_crypto_secretstream_xchacha20poly1305_rekey,
    ),
    ("sodium_crypto_shorthash", sodium_crypto_shorthash),
    (
        "sodium_crypto_shorthash_keygen",
        sodium_crypto_shorthash_keygen,
    ),
    ("sodium_crypto_sign", sodium_crypto_sign),
    ("sodium_crypto_sign_detached", sodium_crypto_sign_detached),
    (
        "sodium_crypto_sign_ed25519_pk_to_curve25519",
        sodium_crypto_sign_ed25519_pk_to_curve25519,
    ),
    (
        "sodium_crypto_sign_ed25519_sk_to_curve25519",
        sodium_crypto_sign_ed25519_sk_to_curve25519,
    ),
    ("sodium_crypto_sign_keypair", sodium_crypto_sign_keypair),
    (
        "sodium_crypto_sign_keypair_from_secretkey_and_publickey",
        sodium_crypto_sign_keypair_from_secretkey_and_publickey,
    ),
    ("sodium_crypto_sign_open", sodium_crypto_sign_open),
    ("sodium_crypto_sign_publickey", sodium_crypto_sign_publickey),
    ("sodium_crypto_sign_secretkey", sodium_crypto_sign_secretkey),
    (
        "sodium_crypto_sign_publickey_from_secretkey",
        sodium_crypto_sign_publickey_from_secretkey,
    ),
    (
        "sodium_crypto_sign_seed_keypair",
        sodium_crypto_sign_seed_keypair,
    ),
    (
        "sodium_crypto_sign_verify_detached",
        sodium_crypto_sign_verify_detached,
    ),
    ("sodium_crypto_stream", sodium_crypto_stream),
    ("sodium_crypto_stream_keygen", sodium_crypto_stream_keygen),
    ("sodium_crypto_stream_xor", sodium_crypto_stream_xor),
    (
        "sodium_crypto_stream_xchacha20",
        sodium_crypto_stream_xchacha20,
    ),
    (
        "sodium_crypto_stream_xchacha20_keygen",
        sodium_crypto_stream_xchacha20_keygen,
    ),
    (
        "sodium_crypto_stream_xchacha20_xor",
        sodium_crypto_stream_xchacha20_xor,
    ),
    (
        "sodium_crypto_stream_xchacha20_xor_ic",
        sodium_crypto_stream_xchacha20_xor_ic,
    ),
    ("sodium_add", sodium_add),
    ("sodium_compare", sodium_compare),
    ("sodium_increment", sodium_increment),
    ("sodium_memcmp", sodium_memcmp),
    ("sodium_memzero", sodium_memzero),
    ("sodium_pad", sodium_pad),
    ("sodium_unpad", sodium_unpad),
    ("sodium_bin2hex", sodium_bin2hex),
    ("sodium_hex2bin", sodium_hex2bin),
    ("sodium_bin2base64", sodium_bin2base64),
    ("sodium_base642bin", sodium_base642bin),
    (
        "sodium_crypto_scalarmult_base",
        sodium_crypto_scalarmult_base,
    ),
];
