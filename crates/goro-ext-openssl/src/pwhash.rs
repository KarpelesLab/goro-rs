//! openssl_pwhash.c: openssl_password_hash() / openssl_password_verify()
//! (Argon2i / Argon2id v1.3 over purecrypto's argon2()).

use goro_core::array::PhpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use purecrypto::kdf::argon2::{Argon2Params, Argon2Type, argon2};

use crate::zpp;

const MEMLIMIT_MIN: i64 = 8;
const MEMLIMIT_MAX: i64 = u32::MAX as i64;
const ITERLIMIT_MIN: i64 = 1;
const ITERLIMIT_MAX: i64 = u32::MAX as i64;
const THREADS_MIN: i64 = 1;
const THREADS_MAX: i64 = u32::MAX as i64;
/// PHP_OPENSSL_PWHASH_MEMLIMIT / ITERLIMIT / THREADS
const DEFAULT_MEMLIMIT: u32 = 64 << 10;
const DEFAULT_ITERLIMIT: u32 = 4;
const DEFAULT_THREADS: u32 = 1;
const ARGON_VERSION: u32 = 0x13;
const SALT_SIZE: usize = 16;
const HASH_SIZE: usize = 32;

/// get_options(): (memlimit, iterlimit, threads)
fn get_options(vm: &mut Vm, options: Option<&PhpArray>) -> Result<(u32, u32, u32), VmError> {
    let mut mem = DEFAULT_MEMLIMIT;
    let mut iter = DEFAULT_ITERLIMIT;
    let mut threads = DEFAULT_THREADS;
    let Some(o) = options else {
        return Ok((mem, iter, threads));
    };
    let get = |k: &str| {
        o.get(&goro_core::array::ArrayKey::String(PhpString::from_bytes(
            k.as_bytes(),
        )))
    };
    if let Some(v) = get("memory_cost") {
        let m = v.deref().to_long();
        if !(MEMLIMIT_MIN..=MEMLIMIT_MAX).contains(&m) {
            return Err(zpp::value_error(
                vm,
                "Memory cost is outside of allowed memory range".into(),
            ));
        }
        mem = m as u32;
    }
    if let Some(v) = get("time_cost") {
        let t = v.deref().to_long();
        if !(ITERLIMIT_MIN..=ITERLIMIT_MAX).contains(&t) {
            return Err(zpp::value_error(
                vm,
                "Time cost is outside of allowed time range".into(),
            ));
        }
        iter = t as u32;
    }
    if let Some(v) = get("threads") {
        let t = v.deref().to_long();
        if t != 1 {
            if !(THREADS_MIN..=THREADS_MAX).contains(&t) {
                return Err(zpp::value_error(vm, "Invalid number of threads".into()));
            }
            threads = t as u32;
        }
    }
    Ok((mem, iter, threads))
}

fn variant(algo: &[u8]) -> Option<Argon2Type> {
    match algo {
        b"argon2i" => Some(Argon2Type::Argon2i),
        b"argon2id" => Some(Argon2Type::Argon2id),
        _ => None,
    }
}

/// php_openssl_argon2_compute_hash()
#[allow(clippy::too_many_arguments)]
fn compute_hash(
    ty: Argon2Type,
    version: u32,
    mem: u32,
    iter: u32,
    threads: u32,
    pass: &[u8],
    salt: &[u8],
    out: &mut [u8],
) -> bool {
    let params = Argon2Params {
        t_cost: iter,
        m_cost_kib: mem,
        parallelism: threads,
        variant: ty,
        version,
    };
    argon2(&params, pass, salt, &[], &[], out).is_ok()
}

fn b64_nopad(data: &[u8]) -> String {
    crate::base64_encode(data).trim_end_matches('=').to_string()
}

/// php_openssl_argon2_extract(): (version, memlimit, iterlimit, threads,
/// salt, hash)
fn extract(digest: &[u8]) -> Option<(u32, u32, u32, u32, Vec<u8>, Vec<u8>)> {
    if digest.len() < b"$argon2id$".len() + 1 {
        return None;
    }
    let rest = if let Some(r) = digest.strip_prefix(b"$argon2i$") {
        r
    } else {
        digest.strip_prefix(b"$argon2id$")?
    };
    // sscanf("v=%u$m=%u,t=%u,p=%u")
    let mut p = 0;
    let mut num = |lit: &[u8]| -> Option<u32> {
        if !rest[p..].starts_with(lit) {
            return None;
        }
        p += lit.len();
        let start = p;
        while p < rest.len() && rest[p].is_ascii_digit() {
            p += 1;
        }
        std::str::from_utf8(&rest[start..p]).ok()?.parse().ok()
    };
    let v = num(b"v=")?;
    let m = num(b"$m=")?;
    let t = num(b",t=")?;
    let th = num(b",p=")?;
    // start of param, start of salt
    let d = std::str::from_utf8(rest).ok()?;
    let a = d.find('$')?;
    let b = a + 1 + d[a + 1..].find('$')?;
    let salt_hash = &d[b + 1..];
    let (salt64, hash64) = salt_hash.split_once('$')?;
    let salt = crate::base64_decode_lenient(salt64.as_bytes());
    let hash = crate::base64_decode_lenient(hash64.as_bytes());
    Some((v, m, t, th, salt, hash))
}

/// php_safe_bcmp()
fn safe_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn check_algo(vm: &mut Vm, f: &str, algo: &[u8]) -> Result<Argon2Type, VmError> {
    variant(algo).ok_or_else(|| {
        zpp::arg_value_error(
            vm,
            f,
            1,
            "algo",
            "must be a valid password openssl hashing algorithm",
        )
    })
}

/// PHP_FUNCTION(openssl_password_hash)
pub fn openssl_password_hash(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "openssl_password_hash";
    let algo = zpp::str(vm, args, 0, F, "algo")?;
    let password = zpp::str(vm, args, 1, F, "password")?;
    let options = zpp::array_nullable(vm, args, 2, F, "options")?;
    let ty = check_algo(vm, F, algo.as_bytes())?;
    if password.len() as u64 >= u32::MAX as u64 {
        return Err(zpp::value_error(vm, "Password is too long".into()));
    }
    let (mem, iter, threads) = {
        let o = options.as_ref().map(|o| o.borrow().clone());
        get_options(vm, o.as_ref())?
    };
    let mut salt = [0u8; SALT_SIZE];
    {
        use purecrypto::rng::RngCore;
        purecrypto::rng::OsRng.fill_bytes(&mut salt);
    }
    let mut hash = [0u8; HASH_SIZE];
    if !compute_hash(
        ty,
        ARGON_VERSION,
        mem,
        iter,
        threads,
        password.as_bytes(),
        &salt,
        &mut hash,
    ) {
        return Err(zpp::value_error(
            vm,
            "Unexpected failure hashing password".into(),
        ));
    }
    let digest = format!(
        "${}$v={}$m={},t={},p={}${}${}",
        String::from_utf8_lossy(algo.as_bytes()),
        ARGON_VERSION,
        mem,
        iter,
        threads,
        b64_nopad(&salt),
        b64_nopad(&hash)
    );
    Ok(Value::String(PhpString::from_string(digest)))
}

/// PHP_FUNCTION(openssl_password_verify)
pub fn openssl_password_verify(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "openssl_password_verify";
    let algo = zpp::str(vm, args, 0, F, "algo")?;
    let password = zpp::str(vm, args, 1, F, "password")?;
    let digest = zpp::str(vm, args, 2, F, "hash")?;
    let ty = check_algo(vm, F, algo.as_bytes())?;
    if password.len() as u64 >= u32::MAX as u64 || digest.len() as u64 >= u32::MAX as u64 {
        return Ok(Value::False);
    }
    let Some((version, mem, iter, threads, salt, hash)) = extract(digest.as_bytes()) else {
        return Ok(Value::False);
    };
    let mut new = vec![0u8; hash.len()];
    let ok = compute_hash(
        ty,
        version,
        mem,
        iter,
        threads,
        password.as_bytes(),
        &salt,
        &mut new,
    ) && safe_eq(&hash, &new);
    Ok(if ok { Value::True } else { Value::False })
}
