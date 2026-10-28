//! PHP's sodium extension (PHP manual: Sodium) on plain values: every
//! `sodium_*()` function and `SODIUM_*` constant of PHP 8.5, with the
//! cryptography from purecrypto where it has the primitive and from this
//! crate where it does not (Salsa20/XSalsa20, HSalsa20/HChaCha20, BLAKE2b
//! with libsodium's state layout, Argon2 with a SIMD kernel, X25519 and
//! variable-time Ed25519 verification arithmetic).
//!
//! Behaviour follows the PHP manual, libsodium's documentation, the
//! algorithm specifications (RFC 7748, 8032, 8439, 7693, 9106, 9496, 7914,
//! the XChaCha and AEGIS drafts, the Salsa20/XSalsa20 and NaCl papers), the
//! expected output of php-src's `ext/sodium/tests/*.phpt`, and observations
//! of PHP 8.5 with libsodium 1.0.18 (`tests/fixtures/sodium_php85.txt`,
//! generated once by `tests/fixtures/sodium_driver.php`).
//!
//! # Engine wiring
//!
//! * Functions are named after their PHP counterparts. [`SIGNATURES`] lists
//!   every function with its parameter names, types, defaults, by-reference
//!   and `#[\SensitiveParameter]` flags, in PHP's registration order;
//!   [`CONSTANTS`] lists the constants with their values.
//! * `string` parameters arrive as `&[u8]` (after the engine's usual
//!   coercion, `strict_types` and TypeErrors), `int` parameters as `i64`.
//!   Results map as: `Vec<u8>` → string, `bool` → bool, `i64` → int,
//!   `Option<Vec<u8>>` → `string|false` (`None` is false), tuples → lists
//!   (`[rx, tx]`, `[state, header]`, `[message, tag]`), `()` → null.
//! * By-reference parameters (`sodium_increment`, `sodium_add`,
//!   `sodium_memzero`, the generichash and secretstream `$state`) take the
//!   referenced [`goro_runtime::Value`] itself (dereferenced): the library
//!   checks its type, as PHP does even in coercive mode ("a PHP string is
//!   required", "must be a reference to a state"), and updates it in place
//!   (copy-on-write separation through `PhpStr::make_mut`).
//!   `sodium_memzero()` and a successful `sodium_crypto_generichash_final()`
//!   set it to null.
//! * Errors are [`SodiumError::Exception`] — throw `SodiumException` (a
//!   non-final subclass of `Exception`) with the message — or
//!   [`SodiumError::OutOfMemory`] for results too large to allocate (PHP's
//!   fatal "Out of memory" error). The password-hashing functions take a
//!   [`goro_runtime::Ctx`] for their "empty password" / "wrong size for the
//!   hashed password" warnings (E_WARNING), raised in PHP's order.
//! * Stack traces of exceptions thrown by sodium functions omit the
//!   arguments (test `ext/sodium/tests/exception_trace_without_args.phpt`);
//!   parameters marked sensitive are also hidden from other traces.
//! * [`set_max_string_len`] caps the strings whose size comes from an
//!   argument (`sodium_crypto_stream*()`, `sodium_pad()`, the password
//!   hashes' `$length`); the engine sets it from `memory_limit` so that an
//!   oversized request is its "Out of memory" fatal error, as on PHP.
//! * Randomness (key generation, nonces of sealed boxes, salts) comes from
//!   the OS through `goro_ext_random::secure`.
//! * [`argon2_password_hash`], [`argon2_password_verify`] and
//!   [`argon2_string_params`] can back ext/standard's `password_hash()`
//!   family for `PASSWORD_ARGON2I`/`PASSWORD_ARGON2ID` (PHP 8.5's reference
//!   build reports `PASSWORD_ARGON2_PROVIDER` "standard").
//!
//! # Constant time
//!
//! Secret-dependent code has no secret-dependent branches or table
//! indexes: purecrypto's constant-time primitives, and here ARX ciphers and
//! hashes, a mask-swapped X25519 ladder, comparisons and padding without
//! data-dependent branches, and hex/Base64 codecs that classify characters
//! without branching (only invalid or ignored characters change the
//! control flow, as in libsodium). Argon2id's second half is data-dependent by
//! design. Verification-side curve arithmetic (Ed25519 verification,
//! public-key checks and conversions) is variable-time over public data
//! only.

mod aead;
mod argon2;
mod blake2b;
mod boxes;
mod chacha;
mod codec;
pub mod consts;
mod ct;
mod ed25519;
mod edwards;
mod error;
mod field;
mod hash;
mod pwhash;
mod ristretto;
mod salsa;
mod secretstream;
mod sign;
mod sigs;
mod stream;
mod util;

pub use aead::*;
pub use boxes::*;
pub use consts::{CONSTANTS, ConstValue};
pub use error::{SodiumError, SodiumResult, set_max_string_len};
pub use hash::*;
pub use pwhash::*;
pub use ristretto::*;
pub use secretstream::*;
pub use sign::*;
pub use sigs::{FnSig, Param, SIGNATURES};
pub use stream::*;
pub use util::*;

/// The class of the exceptions sodium functions throw.
pub const EXCEPTION_CLASS: &str = "SodiumException";

/// `n` random bytes from the OS.
pub(crate) fn random_bytes(n: usize) -> SodiumResult<Vec<u8>> {
    let mut v = vec![0u8; n];
    goro_ext_random::secure::fill(&mut v).map_err(|_| error::internal())?;
    Ok(v)
}
