//! Errors the sodium functions raise.

use std::fmt;

/// An error to raise from a sodium function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SodiumError {
    /// Throw a `SodiumException` with this message.
    Exception(String),
    /// The result would be a string of this many bytes, too large to
    /// allocate: PHP reports its fatal "Out of memory" error (observed on
    /// PHP 8.5 for `sodium_pad('', PHP_INT_MAX)`); the engine applies its
    /// memory limit and raises it.
    OutOfMemory(usize),
}

impl fmt::Display for SodiumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SodiumError::Exception(m) => f.write_str(m),
            SodiumError::OutOfMemory(n) => write!(f, "out of memory ({n} bytes)"),
        }
    }
}

impl std::error::Error for SodiumError {}

/// Result of a sodium function.
pub type SodiumResult<T> = Result<T, SodiumError>;

thread_local! {
    static MAX_STRING_LEN: std::cell::Cell<usize> = const { std::cell::Cell::new(usize::MAX) };
}

/// Caps the length of the strings whose size comes from an argument
/// (`sodium_crypto_stream*()`'s `$length`, `sodium_pad()`'s block size,
/// the password hashes' `$length`): above it, functions return
/// [`SodiumError::OutOfMemory`] instead of allocating. The engine sets it
/// from its memory limit (per thread; unlimited by default).
pub fn set_max_string_len(max: usize) {
    MAX_STRING_LEN.with(|m| m.set(max));
}

/// A zeroed buffer of `n` bytes, or [`SodiumError::OutOfMemory`].
pub(crate) fn zeroed(n: u64) -> SodiumResult<Vec<u8>> {
    let max = MAX_STRING_LEN.with(|m| m.get()).min(isize::MAX as usize) as u64;
    if n > max {
        return Err(SodiumError::OutOfMemory(n.min(usize::MAX as u64) as usize));
    }
    let n = n as usize;
    let mut v = Vec::new();
    if v.try_reserve_exact(n).is_err() {
        return Err(SodiumError::OutOfMemory(n));
    }
    v.resize(n, 0);
    Ok(v)
}

/// `SodiumException` with a fixed message.
pub(crate) fn exc(msg: &str) -> SodiumError {
    SodiumError::Exception(msg.to_string())
}

/// "internal error": libsodium reported a failure.
pub(crate) fn internal() -> SodiumError {
    exc("internal error")
}

/// `"f(): Argument #n ($name) what"`.
pub(crate) fn arg(func: &str, n: u32, name: &str, what: &str) -> SodiumError {
    SodiumError::Exception(format!("{func}(): Argument #{n} (${name}) {what}"))
}

/// `"f(): Argument #n ($name) must be CONST bytes long"`.
pub(crate) fn arg_len(func: &str, n: u32, name: &str, constant: &str) -> SodiumError {
    SodiumError::Exception(format!(
        "{func}(): Argument #{n} (${name}) must be SODIUM_{constant} bytes long"
    ))
}

/// Checks that `v` is exactly `len` bytes long.
#[inline]
pub(crate) fn want_len(
    v: &[u8],
    len: usize,
    func: &str,
    n: u32,
    name: &str,
    constant: &str,
) -> SodiumResult<()> {
    if v.len() == len {
        Ok(())
    } else {
        Err(arg_len(func, n, name, constant))
    }
}
