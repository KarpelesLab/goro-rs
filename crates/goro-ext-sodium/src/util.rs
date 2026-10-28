//! Helper functions: hex/Base64 codecs, constant-time comparisons,
//! little-endian arithmetic, padding and `sodium_memzero()` (PHP manual:
//! Sodium "Functions"; libsodium documentation "Helpers" and "Padding").

use goro_runtime::Value;

use crate::codec::{self, Variant};
use crate::ct;
use crate::error::{SodiumError, SodiumResult, arg, exc};

/// `sodium_bin2hex(string $string): string`
pub fn sodium_bin2hex(string: &[u8]) -> Vec<u8> {
    codec::bin2hex(string)
}

/// `sodium_hex2bin(string $string, string $ignore = ""): string`
pub fn sodium_hex2bin(string: &[u8], ignore: &[u8]) -> SodiumResult<Vec<u8>> {
    codec::hex2bin(string, ignore).ok_or_else(|| {
        arg(
            "sodium_hex2bin",
            1,
            "string",
            "must be a valid hexadecimal string",
        )
    })
}

fn variant(func: &str, id: i64) -> SodiumResult<Variant> {
    Variant::from_id(id)
        .ok_or_else(|| arg(func, 2, "id", "must be a valid base64 variant identifier"))
}

/// `sodium_bin2base64(string $string, int $id): string`
pub fn sodium_bin2base64(string: &[u8], id: i64) -> SodiumResult<Vec<u8>> {
    let v = variant("sodium_bin2base64", id)?;
    Ok(codec::bin2base64(string, v))
}

/// `sodium_base642bin(string $string, int $id, string $ignore = ""): string`
pub fn sodium_base642bin(string: &[u8], id: i64, ignore: &[u8]) -> SodiumResult<Vec<u8>> {
    let v = variant("sodium_base642bin", id)?;
    codec::base642bin(string, v, ignore).ok_or_else(|| {
        arg(
            "sodium_base642bin",
            1,
            "string",
            "must be a valid base64 string",
        )
    })
}

fn same_length(func: &str) -> SodiumError {
    // Observed on PHP 8.5, including the "string_2" spelling.
    SodiumError::Exception(format!(
        "{func}(): Argument #1 ($string1) and argument #2 ($string_2) must have the same length"
    ))
}

/// `sodium_compare(string $string1, string $string2): int` — little-endian
/// comparison, constant-time for a given length.
pub fn sodium_compare(string1: &[u8], string2: &[u8]) -> SodiumResult<i64> {
    if string1.len() != string2.len() {
        return Err(same_length("sodium_compare"));
    }
    Ok(ct::compare_le(string1, string2) as i64)
}

/// `sodium_memcmp(string $string1, string $string2): int` — 0 when equal,
/// -1 otherwise, constant-time for a given length.
pub fn sodium_memcmp(string1: &[u8], string2: &[u8]) -> SodiumResult<i64> {
    if string1.len() != string2.len() {
        return Err(same_length("sodium_memcmp"));
    }
    Ok(if ct::eq(string1, string2) { 0 } else { -1 })
}

/// `sodium_increment(string &$string): void` — `string` is the referenced
/// value; a non-string throws (observed on PHP 8.5, in coercive mode too).
/// The string is separated from other copies first (copy-on-write), as on
/// PHP.
pub fn sodium_increment(string: &mut Value) -> SodiumResult<()> {
    match string {
        Value::Str(s) => {
            ct::increment_le(s.make_mut());
            Ok(())
        }
        _ => Err(exc("a PHP string is required")),
    }
}

/// `sodium_add(string &$string1, string $string2): void` — little-endian
/// addition modulo 2^(8·length).
pub fn sodium_add(string1: &mut Value, string2: &[u8]) -> SodiumResult<()> {
    match string1 {
        Value::Str(s) => {
            if s.len() != string2.len() {
                return Err(same_length("sodium_add"));
            }
            ct::add_le(s.make_mut(), string2);
            Ok(())
        }
        _ => Err(exc("PHP strings are required")),
    }
}

/// `sodium_memzero(string &$string): void` — wipes the string's bytes when
/// no other value shares them, and sets the variable to null (observed on
/// PHP 8.5: copies made before the call keep their contents).
pub fn sodium_memzero(string: &mut Value) -> SodiumResult<()> {
    match string {
        Value::Str(s) => {
            if s.refcount() == 1 && !s.is_immortal() {
                ct::wipe(s.make_mut());
            }
            *string = Value::Null;
            Ok(())
        }
        _ => Err(exc("a PHP string is required")),
    }
}

/// `sodium_pad(string $string, int $block_size): string` — ISO/IEC 7816-4
/// padding: `0x80`, then zeros up to the next multiple of the block size
/// (a whole block when already aligned).
pub fn sodium_pad(string: &[u8], block_size: i64) -> SodiumResult<Vec<u8>> {
    if block_size <= 0 {
        return Err(arg("sodium_pad", 2, "block_size", "must be greater than 0"));
    }
    let bs = block_size as u64;
    let len = string.len() as u64;
    let padded = (len / bs + 1).saturating_mul(bs);
    let mut out = crate::error::zeroed(padded)?;
    out[..string.len()].copy_from_slice(string);
    out[string.len()] = 0x80;
    Ok(out)
}

/// `sodium_unpad(string $string, int $block_size): string` — removes the
/// padding [`sodium_pad`] adds; the scan of the last block is
/// constant-time.
pub fn sodium_unpad(string: &[u8], block_size: i64) -> SodiumResult<Vec<u8>> {
    if block_size <= 0 {
        return Err(arg(
            "sodium_unpad",
            2,
            "block_size",
            "must be greater than 0",
        ));
    }
    if (string.len() as u64) < block_size as u64 {
        return Err(arg(
            "sodium_unpad",
            1,
            "string",
            "must be at least as long as the block size",
        ));
    }
    let bs = block_size as usize;
    let tail = &string[string.len() - bs..];
    // From the end: skip zeros; the first non-zero byte must be 0x80.
    // `found` becomes 1 at that byte; `pad` counts the bytes after it.
    let mut found: u32 = 0;
    let mut valid: u32 = 0;
    let mut pad: usize = 0;
    for (i, &b) in tail.iter().enumerate().rev() {
        let is_zero = ((b as u32).wrapping_sub(1) >> 8) & 1;
        let is_80 = (((b ^ 0x80) as u32).wrapping_sub(1) >> 8) & 1;
        let first_nonzero = (found ^ 1) & (is_zero ^ 1);
        valid |= first_nonzero & is_80;
        // Record the position of the marker without branching on it.
        let m = (first_nonzero as usize).wrapping_neg();
        pad = (pad & !m) | ((bs - i) & m);
        found |= is_zero ^ 1;
    }
    if valid == 0 {
        return Err(exc("invalid padding"));
    }
    Ok(string[..string.len() - pad].to_vec())
}
