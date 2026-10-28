//! `Random\Engine\Secure` and the CSPRNG behind `random_bytes()`,
//! `random_int()` and the default seeds: the operating system's generator
//! (`getrandom(2)` on Linux, `getentropy(3)` on other Unix systems,
//! `/dev/urandom` otherwise). Nothing is buffered in user space, so a forked
//! process never repeats its parent's output.

use crate::engine::{Engine, Generated};
use crate::{RandomError, serialization_not_allowed};

/// Fills `buf` from the operating system's CSPRNG.
pub fn fill(buf: &mut [u8]) -> Result<(), RandomError> {
    os_fill(buf).map_err(|e| {
        RandomError::RandomException(format!(
            "Failed to retrieve randomness from the operating system ({e})"
        ))
    })
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn os_fill(mut buf: &mut [u8]) -> std::io::Result<()> {
    while !buf.is_empty() {
        // SAFETY: the pointer and length describe the writable slice `buf`.
        let n = unsafe { libc::getrandom(buf.as_mut_ptr().cast(), buf.len(), 0) };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        buf = &mut buf[n as usize..];
    }
    Ok(())
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
fn os_fill(buf: &mut [u8]) -> std::io::Result<()> {
    for chunk in buf.chunks_mut(256) {
        // SAFETY: the pointer and length describe the writable chunk.
        if unsafe { libc::getentropy(chunk.as_mut_ptr().cast(), chunk.len()) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn os_fill(buf: &mut [u8]) -> std::io::Result<()> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")?.read_exact(buf)
}

/// `Random\Engine\Secure`: 8 bytes from the CSPRNG per `generate()`. It has
/// no state: `__debugInfo()` is empty and serializing it throws
/// [`Secure::serialize_error`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Secure;

impl Secure {
    /// The exception `serialize()` throws for this engine (and for a
    /// Randomizer holding it).
    pub fn serialize_error() -> RandomError {
        serialization_not_allowed(crate::class::SECURE)
    }
}

impl Engine for Secure {
    fn generate(&mut self) -> Result<Generated, RandomError> {
        let mut b = [0u8; 8];
        fill(&mut b)?;
        Ok(Generated::u64(u64::from_le_bytes(b)))
    }
    fn fill_unobservable(&mut self, out: &mut [u8]) -> Option<Result<(), RandomError>> {
        Some(fill(out))
    }
}
