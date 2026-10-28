//! Password hashing: `crypto_pwhash` (Argon2i/Argon2id v1.3, RFC 9106) and
//! `crypto_pwhash_scryptsalsa208sha256` (scrypt, RFC 7914), with their
//! string formats (libsodium documentation, "Password hashing").
//!
//! Argon2 strings use the PHC string format:
//! `$argon2id$v=19$m=<KiB>,t=<passes>,p=<lanes>$<salt>$<hash>`, Base64
//! without padding. The decoding rules were observed on PHP 8.5: decimal
//! fields without signs, spaces or leading zeros and within 32 bits, a
//! canonical Base64 encoding, a salt of at least 8 bytes, a hash of at least
//! 16, `m ≥ 8·p`, the version 19, the string read up to its first NUL byte
//! and shorter than [`ARGON2_STR_MAX`] bytes.
//!
//! scrypt strings use the `$7$` format: `$7$`, one character for log2(N),
//! five for r and five for p (30-bit little-endian groups of six bits in
//! the alphabet `./0-9A-Za-z`), the salt (32 random bytes in the same
//! encoding, used as text), `$` and the 32-byte hash in that encoding. The
//! mapping from (opslimit, memlimit) to (N, r, p) was fitted to PHP 8.5
//! output over a grid of limits: r = 8; when `opslimit < memlimit / 32`,
//! p = 1 and N is the smallest power of two (at least 2) above
//! `opslimit / (4r) / 2`; otherwise N is the smallest power of two above
//! `memlimit / (128r) / 2` and p = `min(opslimit / 4 / N, 2^30 − 1) / r`.

use goro_runtime::{Ctx, Level};

use crate::argon2::{self, Variant as A2};
use crate::codec::{self, Variant};
use crate::consts::*;
use crate::error::{SodiumResult, arg, exc, internal, want_len};
use crate::{ct, random_bytes};

/// Argon2 strings must be shorter than this (observed on PHP 8.5; libsodium
/// documents `crypto_pwhash_STRBYTES` = 128 including the terminator).
pub const ARGON2_STR_MAX: usize = 128;

/// Length of a `$7$` string (`crypto_pwhash_scryptsalsa208sha256_STRBYTES`
/// minus the terminator).
pub const SCRYPT_STR_LEN: usize = 101;

const ARGON2_MEMLIMIT_MIN: i64 = 8192;
const ARGON2_MEMLIMIT_MAX: i64 = 4_398_046_510_080;
const ARGON2_OPSLIMIT_MAX: i64 = 4_294_967_295;
const ARGON2_BYTES_MIN: i64 = 16;

const B64: Variant = Variant {
    urlsafe: false,
    padded: false,
};

fn empty_password(ctx: &mut dyn Ctx, password: &[u8]) {
    if password.is_empty() {
        ctx.notice(Level::Warning, "empty password".to_string());
    }
}

fn argon2_raw(
    variant: A2,
    t: i64,
    mem: i64,
    lanes: u32,
    password: &[u8],
    salt: &[u8],
    out: &mut [u8],
) -> Result<(), ()> {
    if !(1..=ARGON2_OPSLIMIT_MAX).contains(&t)
        || !(ARGON2_MEMLIMIT_MIN..=ARGON2_MEMLIMIT_MAX).contains(&mem)
    {
        return Err(());
    }
    let params = argon2::Params {
        t_cost: t as u32,
        m_cost_kib: (mem / 1024) as u32,
        lanes,
        variant,
        version: argon2::VERSION,
    };
    argon2::hash(&params, password, salt, &[], &[], out).map_err(|_| ())
}

/// `sodium_crypto_pwhash(int $length, string $password, string $salt, int $opslimit, int $memlimit, int $algo = SODIUM_CRYPTO_PWHASH_ALG_DEFAULT): string`
pub fn sodium_crypto_pwhash(
    ctx: &mut dyn Ctx,
    length: i64,
    password: &[u8],
    salt: &[u8],
    opslimit: i64,
    memlimit: i64,
    algo: i64,
) -> SodiumResult<Vec<u8>> {
    const F: &str = "sodium_crypto_pwhash";
    if length <= 0 {
        return Err(arg(F, 1, "length", "must be greater than 0"));
    }
    if length >= 0xffff_ffff {
        return Err(arg(F, 1, "length", "is too large"));
    }
    if opslimit <= 0 {
        return Err(arg(F, 4, "opslimit", "must be greater than 0"));
    }
    if memlimit <= 0 {
        return Err(arg(F, 5, "memlimit", "must be greater than 0"));
    }
    let variant = match algo {
        PWHASH_ALG_ARGON2I13 => A2::Argon2i,
        PWHASH_ALG_ARGON2ID13 => A2::Argon2id,
        _ => return Err(exc("unsupported password hashing algorithm")),
    };
    empty_password(ctx, password);
    want_len(
        salt,
        PWHASH_SALTBYTES,
        F,
        3,
        "salt",
        "CRYPTO_PWHASH_SALTBYTES",
    )?;
    // libsodium's minimums: 16 output bytes, 3 passes for Argon2i, 1 for
    // Argon2id (documentation: crypto_pwhash_OPSLIMIT_MIN per algorithm).
    let min_ops = if variant == A2::Argon2i { 3 } else { 1 };
    if length < ARGON2_BYTES_MIN || opslimit < min_ops {
        return Err(internal());
    }
    let mut out = crate::error::zeroed(length as u64)?;
    argon2_raw(variant, opslimit, memlimit, 1, password, salt, &mut out).map_err(|_| internal())?;
    Ok(out)
}

fn argon2_str(variant: A2, t: u32, m: u32, p: u32, salt: &[u8], hash: &[u8]) -> Vec<u8> {
    let name = if variant == A2::Argon2i {
        "argon2i"
    } else {
        "argon2id"
    };
    let mut s = format!("${name}$v=19$m={m},t={t},p={p}$").into_bytes();
    s.extend_from_slice(&codec::bin2base64(salt, B64));
    s.push(b'$');
    s.extend_from_slice(&codec::bin2base64(hash, B64));
    s
}

/// `sodium_crypto_pwhash_str(string $password, int $opslimit, int $memlimit): string`
pub fn sodium_crypto_pwhash_str(
    ctx: &mut dyn Ctx,
    password: &[u8],
    opslimit: i64,
    memlimit: i64,
) -> SodiumResult<Vec<u8>> {
    const F: &str = "sodium_crypto_pwhash_str";
    if opslimit <= 0 {
        return Err(arg(F, 2, "opslimit", "must be greater than 0"));
    }
    if memlimit <= 0 {
        return Err(arg(F, 3, "memlimit", "must be greater than 0"));
    }
    empty_password(ctx, password);
    let salt = random_bytes(PWHASH_SALTBYTES)?;
    let mut out = [0u8; 32];
    argon2_raw(
        A2::Argon2id,
        opslimit,
        memlimit,
        1,
        password,
        &salt,
        &mut out,
    )
    .map_err(|_| internal())?;
    Ok(argon2_str(
        A2::Argon2id,
        opslimit as u32,
        (memlimit / 1024) as u32,
        1,
        &salt,
        &out,
    ))
}

/// A decoded Argon2 string.
struct Argon2Str {
    variant: A2,
    m: u32,
    t: u32,
    p: u32,
    salt: Vec<u8>,
    hash: Vec<u8>,
}

/// A decimal without sign, spaces or leading zeros, within 32 bits.
fn decimal(s: &[u8]) -> Option<(u32, &[u8])> {
    let n = s.iter().take_while(|c| c.is_ascii_digit()).count();
    if n == 0 || (n > 1 && s[0] == b'0') || n > 10 {
        return None;
    }
    let v: u64 = std::str::from_utf8(&s[..n]).ok()?.parse().ok()?;
    if v > u32::MAX as u64 {
        return None;
    }
    Some((v as u32, &s[n..]))
}

fn expect<'a>(s: &'a [u8], lit: &[u8]) -> Option<&'a [u8]> {
    s.strip_prefix(lit)
}

fn decode_argon2(s: &[u8]) -> Option<Argon2Str> {
    let s = match s.iter().position(|&b| b == 0) {
        Some(n) => &s[..n],
        None => s,
    };
    if s.len() >= ARGON2_STR_MAX {
        return None;
    }
    let (variant, rest) = match s.strip_prefix(b"$argon2id$") {
        Some(r) => (A2::Argon2id, r),
        None => (A2::Argon2i, s.strip_prefix(b"$argon2i$")?),
    };
    let rest = expect(rest, b"v=")?;
    let (v, rest) = decimal(rest)?;
    if v != argon2::VERSION {
        return None;
    }
    let rest = expect(rest, b"$m=")?;
    let (m, rest) = decimal(rest)?;
    let rest = expect(rest, b",t=")?;
    let (t, rest) = decimal(rest)?;
    let rest = expect(rest, b",p=")?;
    let (p, rest) = decimal(rest)?;
    let rest = expect(rest, b"$")?;
    let dollar = rest.iter().position(|&b| b == b'$')?;
    let salt = codec::base642bin(&rest[..dollar], B64, b"")?;
    let hash = codec::base642bin(&rest[dollar + 1..], B64, b"")?;
    if salt.len() < 8 || hash.len() < 16 {
        return None;
    }
    if t == 0 || p == 0 || p > 0x00ff_ffff || (m as u64) < 8 * p as u64 {
        return None;
    }
    Some(Argon2Str {
        variant,
        m,
        t,
        p,
        salt,
        hash,
    })
}

/// `sodium_crypto_pwhash_str_verify(string $hash, string $password): bool`
pub fn sodium_crypto_pwhash_str_verify(
    ctx: &mut dyn Ctx,
    hash: &[u8],
    password: &[u8],
) -> SodiumResult<bool> {
    empty_password(ctx, password);
    let Some(d) = decode_argon2(hash) else {
        return Ok(false);
    };
    let params = argon2::Params {
        t_cost: d.t,
        m_cost_kib: d.m,
        lanes: d.p,
        variant: d.variant,
        version: argon2::VERSION,
    };
    let mut out = vec![0u8; d.hash.len()];
    if argon2::hash(&params, password, &d.salt, &[], &[], &mut out).is_err() {
        return Ok(false);
    }
    let ok = ct::eq(&out, &d.hash);
    ct::wipe(&mut out);
    Ok(ok)
}

/// `sodium_crypto_pwhash_str_needs_rehash(string $password, int $opslimit, int $memlimit): bool`
/// — true when the string is not a valid Argon2 string or was made with
/// other limits.
pub fn sodium_crypto_pwhash_str_needs_rehash(
    password: &[u8],
    opslimit: i64,
    memlimit: i64,
) -> SodiumResult<bool> {
    let Some(d) = decode_argon2(password) else {
        return Ok(true);
    };
    if !(0..=ARGON2_OPSLIMIT_MAX).contains(&opslimit)
        || !(0..=ARGON2_MEMLIMIT_MAX).contains(&memlimit)
    {
        return Ok(true);
    }
    Ok(d.t as i64 != opslimit || d.m as i64 != memlimit / 1024)
}

// ---- Argon2 for password_hash() ----

/// The Argon2 parameters of a `$argon2i$` / `$argon2id$` string, as read
/// by [`sodium_crypto_pwhash_str_verify`]: `(id, memory KiB, passes,
/// lanes)` with `id` "argon2i" or "argon2id". For ext/standard's
/// `password_get_info()` / `password_needs_rehash()` (PHP 8.5's reference
/// build has its own Argon2 provider, "standard"; this one can back it).
pub fn argon2_string_params(hash: &[u8]) -> Option<(&'static str, u32, u32, u32)> {
    let d = decode_argon2(hash)?;
    let id = if d.variant == A2::Argon2i {
        "argon2i"
    } else {
        "argon2id"
    };
    Some((id, d.m, d.t, d.p))
}

/// An Argon2 v1.3 password string with a random 16-byte salt and a 32-byte
/// hash, for ext/standard's `password_hash()` with `PASSWORD_ARGON2I` or
/// `PASSWORD_ARGON2ID`: `memory_cost` in KiB, `time_cost` passes,
/// `threads` lanes (computed sequentially). `None` for parameters Argon2
/// rejects or memory that cannot be allocated; the range checks and their
/// messages are ext/standard's.
pub fn argon2_password_hash(
    argon2id: bool,
    password: &[u8],
    memory_cost: u32,
    time_cost: u32,
    threads: u32,
) -> Option<Vec<u8>> {
    let variant = if argon2id { A2::Argon2id } else { A2::Argon2i };
    let salt = random_bytes(PWHASH_SALTBYTES).ok()?;
    let params = argon2::Params {
        t_cost: time_cost,
        m_cost_kib: memory_cost,
        lanes: threads,
        variant,
        version: argon2::VERSION,
    };
    let mut out = [0u8; 32];
    argon2::hash(&params, password, &salt, &[], &[], &mut out).ok()?;
    Some(argon2_str(
        variant,
        time_cost,
        memory_cost,
        threads,
        &salt,
        &out,
    ))
}

/// Verifies a password against an Argon2 string (for `password_verify()`),
/// with [`sodium_crypto_pwhash_str_verify`]'s decoding and without its
/// empty-password warning.
pub fn argon2_password_verify(hash: &[u8], password: &[u8]) -> bool {
    let mut quiet = goro_runtime::Diagnostics::new();
    sodium_crypto_pwhash_str_verify(&mut quiet, hash, password).unwrap_or(false)
}

// ---- scrypt ----

const ITOA64: &[u8; 64] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn atoi64(c: u8) -> Option<u32> {
    ITOA64.iter().position(|&x| x == c).map(|p| p as u32)
}

fn encode_u30(out: &mut Vec<u8>, v: u32) {
    for i in 0..5 {
        out.push(ITOA64[((v >> (6 * i)) & 63) as usize]);
    }
}

fn decode_u30(s: &[u8]) -> Option<u32> {
    let mut v = 0u32;
    for (i, &c) in s.iter().enumerate() {
        v |= atoi64(c)? << (6 * i);
    }
    Some(v)
}

/// Bytes in 24-bit little-endian groups, six bits per character.
fn encode64(out: &mut Vec<u8>, bytes: &[u8]) {
    for chunk in bytes.chunks(3) {
        let mut v = 0u32;
        for (i, &b) in chunk.iter().enumerate() {
            v |= (b as u32) << (8 * i);
        }
        let chars = (chunk.len() * 8).div_ceil(6);
        for i in 0..chars {
            out.push(ITOA64[((v >> (6 * i)) & 63) as usize]);
        }
    }
}

/// (log2 N, r, p) for the limits (see the module documentation).
fn pick_params(opslimit: u64, memlimit: u64) -> (u8, u32, u32) {
    let opslimit = opslimit.max(32768);
    let r: u64 = 8;
    let smallest_above = |half: u64| -> u8 {
        let mut n = 1u8;
        while n < 63 && (1u64 << n) <= half {
            n += 1;
        }
        n
    };
    if opslimit < memlimit / 32 {
        let max_n = opslimit / (r * 4);
        (smallest_above(max_n / 2), r as u32, 1)
    } else {
        let max_n = memlimit / (r * 128);
        let n_log2 = smallest_above(max_n / 2);
        let maxrp = ((opslimit / 4) >> n_log2).min(0x3fff_ffff);
        (n_log2, r as u32, (maxrp / r) as u32)
    }
}

/// scrypt, `Err` when the parameters are invalid or the memory cannot be
/// allocated.
fn scrypt(
    password: &[u8],
    salt: &[u8],
    n_log2: u8,
    r: u32,
    p: u32,
    out: &mut [u8],
) -> Result<(), ()> {
    if n_log2 == 0 || n_log2 >= 64 || r == 0 || p == 0 {
        return Err(());
    }
    // Check the allocation first: an impossible size is a failure, not an
    // abort.
    let need = 128u64
        .checked_mul(r as u64)
        .and_then(|x| x.checked_mul(1u64 << n_log2))
        .ok_or(())?;
    if need > isize::MAX as u64 {
        return Err(());
    }
    let mut probe: Vec<u8> = Vec::new();
    probe.try_reserve_exact(need as usize).map_err(|_| ())?;
    drop(probe);
    purecrypto::kdf::scrypt::scrypt(password, salt, n_log2, r, p, out).map_err(|_| ())
}

fn scrypt_limits(func: &str, ops_arg: u32, opslimit: i64, memlimit: i64) -> SodiumResult<()> {
    if opslimit < PWHASH_SCRYPT_OPSLIMIT_INTERACTIVE {
        return Err(arg(
            func,
            ops_arg,
            "opslimit",
            "must be greater than or equal to 524288",
        ));
    }
    if memlimit < PWHASH_SCRYPT_MEMLIMIT_INTERACTIVE {
        return Err(arg(
            func,
            ops_arg + 1,
            "memlimit",
            "must be greater than or equal to 16777216",
        ));
    }
    Ok(())
}

/// `sodium_crypto_pwhash_scryptsalsa208sha256(int $length, string $password, string $salt, int $opslimit, int $memlimit): string`
pub fn sodium_crypto_pwhash_scryptsalsa208sha256(
    ctx: &mut dyn Ctx,
    length: i64,
    password: &[u8],
    salt: &[u8],
    opslimit: i64,
    memlimit: i64,
) -> SodiumResult<Vec<u8>> {
    const F: &str = "sodium_crypto_pwhash_scryptsalsa208sha256";
    if length <= 0 {
        return Err(arg(F, 1, "length", "must be greater than 0"));
    }
    if opslimit <= 0 {
        return Err(arg(F, 4, "opslimit", "must be greater than 0"));
    }
    if memlimit <= 0 {
        return Err(arg(F, 5, "memlimit", "must be greater than 0"));
    }
    empty_password(ctx, password);
    want_len(
        salt,
        PWHASH_SCRYPT_SALTBYTES,
        F,
        3,
        "salt",
        "CRYPTO_PWHASH_SCRYPTSALSA208SHA256_SALTBYTES",
    )?;
    scrypt_limits(F, 4, opslimit, memlimit)?;
    // libsodium's minimum output length (crypto_pwhash_scryptsalsa208sha256_BYTES_MIN).
    if length < 16 {
        return Err(internal());
    }
    let mut out = crate::error::zeroed(length as u64)?;
    let (n_log2, r, p) = pick_params(opslimit as u64, memlimit as u64);
    scrypt(password, salt, n_log2, r, p, &mut out).map_err(|_| internal())?;
    Ok(out)
}

/// `sodium_crypto_pwhash_scryptsalsa208sha256_str(string $password, int $opslimit, int $memlimit): string`
pub fn sodium_crypto_pwhash_scryptsalsa208sha256_str(
    ctx: &mut dyn Ctx,
    password: &[u8],
    opslimit: i64,
    memlimit: i64,
) -> SodiumResult<Vec<u8>> {
    const F: &str = "sodium_crypto_pwhash_scryptsalsa208sha256_str";
    if opslimit <= 0 {
        return Err(arg(F, 2, "opslimit", "must be greater than 0"));
    }
    if memlimit <= 0 {
        return Err(arg(F, 3, "memlimit", "must be greater than 0"));
    }
    empty_password(ctx, password);
    scrypt_limits(F, 2, opslimit, memlimit)?;
    let (n_log2, r, p) = pick_params(opslimit as u64, memlimit as u64);
    let raw_salt = random_bytes(PWHASH_SCRYPT_SALTBYTES)?;
    let mut setting = b"$7$".to_vec();
    setting.push(ITOA64[n_log2 as usize]);
    encode_u30(&mut setting, r);
    encode_u30(&mut setting, p);
    let salt_start = setting.len();
    encode64(&mut setting, &raw_salt);
    let mut hash = [0u8; 32];
    scrypt(password, &setting[salt_start..], n_log2, r, p, &mut hash).map_err(|_| internal())?;
    setting.push(b'$');
    encode64(&mut setting, &hash);
    Ok(setting)
}

/// `sodium_crypto_pwhash_scryptsalsa208sha256_str_verify(string $hash, string $password): bool`
pub fn sodium_crypto_pwhash_scryptsalsa208sha256_str_verify(
    ctx: &mut dyn Ctx,
    hash: &[u8],
    password: &[u8],
) -> SodiumResult<bool> {
    empty_password(ctx, password);
    if hash.len() != SCRYPT_STR_LEN {
        ctx.notice(
            Level::Warning,
            "wrong size for the hashed password".to_string(),
        );
        return Ok(false);
    }
    let Some(rest) = hash.strip_prefix(b"$7$") else {
        return Ok(false);
    };
    if rest.len() < 11 {
        return Ok(false);
    }
    let (Some(n_log2), Some(r), Some(p)) = (
        atoi64(rest[0]),
        decode_u30(&rest[1..6]),
        decode_u30(&rest[6..11]),
    ) else {
        return Ok(false);
    };
    let tail = &rest[11..];
    let Some(dollar) = tail.iter().position(|&b| b == b'$') else {
        return Ok(false);
    };
    let salt = &tail[..dollar];
    let mut out = [0u8; 32];
    if scrypt(password, salt, n_log2 as u8, r, p, &mut out).is_err() {
        return Ok(false);
    }
    let mut expected = hash[..3 + 11 + dollar + 1].to_vec();
    encode64(&mut expected, &out);
    Ok(ct::eq(&expected, hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_fit() {
        // Observed on PHP 8.5 (`tests/fixtures`).
        assert_eq!(pick_params(524288, 16777216), (14, 8, 1));
        assert_eq!(pick_params(1048576, 16777216), (14, 8, 2));
        assert_eq!(pick_params(33554432, 16777216), (14, 8, 64));
        assert_eq!(pick_params(524288, 1073741824), (14, 8, 1));
        assert_eq!(pick_params(4194304, 33554432), (15, 8, 4));
        assert_eq!(pick_params(67108864, 16777216), (14, 8, 128));
    }
}
