//! The argon2i / argon2id password_hash() algorithms and their constants.
//!
//! The reference PHP build links libargon2 (HAVE_ARGON2LIB), so they are
//! ext/standard's (password.c, PASSWORD_ARGON2_PROVIDER "standard", any
//! number of threads, a 16-character salt) and ext/sodium's own
//! (sodium_pwhash.c) is not registered. The argon2 core is shared.

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use goro_ext_standard::crypt::{
    PasswordAlgoHandler, PasswordOptions, option_long, password_algo_register,
    password_algo_registered, password_make_salt,
};

use crate::pwhash;

/// PHP_SODIUM_PWHASH_MEMLIMIT (KiB)
pub const MEMLIMIT: i64 = 64 << 10;
/// PHP_SODIUM_PWHASH_OPSLIMIT
pub const OPSLIMIT: i64 = 4;
/// PHP_SODIUM_PWHASH_THREADS
pub const THREADS: i64 = 1;

fn value_error(vm: &mut Vm, msg: &str) {
    let e = vm.create_exception(b"ValueError", msg, vm.current_line);
    vm.current_exception = Some(e);
}

/// php_password_argon2_hash() (libargon2)
fn argon2_hash(
    vm: &mut Vm,
    password: &[u8],
    options: PasswordOptions,
    alg: i64,
) -> Result<Option<Vec<u8>>, VmError> {
    // (size_t: a negative value is out of range)
    let memory_cost = option_long(vm, options, b"memory_cost").unwrap_or(MEMLIMIT) as u64;
    if !(8..=0xffff_ffff).contains(&memory_cost) {
        value_error(vm, "Memory cost is outside of allowed memory range");
        return Ok(None);
    }
    let time_cost = option_long(vm, options, b"time_cost").unwrap_or(OPSLIMIT) as u64;
    if !(1..=0xffff_ffff).contains(&time_cost) {
        value_error(vm, "Time cost is outside of allowed time range");
        return Ok(None);
    }
    let threads = option_long(vm, options, b"threads").unwrap_or(THREADS) as u64;
    if threads > 0xff_ffff || threads == 0 {
        value_error(vm, "Invalid number of threads");
        return Ok(None);
    }
    // php_password_get_salt()
    if options.is_some_and(|o| o.borrow().get_str(b"salt").is_some()) {
        vm.emit_warning(
            "password_hash(): The \"salt\" option has been ignored, since providing a custom salt is no longer supported",
        );
    }
    let salt = password_make_salt(vm, 16)?;
    if password.len() as u64 > 0xffff_ffff {
        value_error(vm, "Password is too long");
        return Ok(None);
    }
    match pwhash::argon2_hash_encoded(
        alg,
        time_cost as u32,
        memory_cost as u32,
        threads as u32,
        password,
        &salt,
    ) {
        Ok(h) => Ok(Some(h)),
        Err(msg) => {
            value_error(vm, msg);
            Ok(None)
        }
    }
}

fn argon2i_hash(
    vm: &mut Vm,
    password: &[u8],
    options: PasswordOptions,
) -> Result<Option<Vec<u8>>, VmError> {
    argon2_hash(vm, password, options, pwhash::ALG_ARGON2I13)
}

fn argon2id_hash(
    vm: &mut Vm,
    password: &[u8],
    options: PasswordOptions,
) -> Result<Option<Vec<u8>>, VmError> {
    argon2_hash(vm, password, options, pwhash::ALG_ARGON2ID13)
}

/// argon2_verify()
fn argon2_verify(password: &[u8], hash: &[u8]) -> bool {
    if password.len() as u64 >= 0xffff_ffff || hash.len() as u64 >= 0xffff_ffff {
        return false;
    }
    pwhash::str_verify(hash, password)
}

/// php_password_argon2_needs_rehash()
fn argon2_needs_rehash(vm: &mut Vm, hash: &[u8], options: PasswordOptions) -> bool {
    let new_memory = option_long(vm, options, b"memory_cost").unwrap_or(MEMLIMIT);
    let new_time = option_long(vm, options, b"time_cost").unwrap_or(OPSLIMIT);
    let new_threads = option_long(vm, options, b"threads").unwrap_or(THREADS);
    // extract_argon2_parameters(): v, m, t, p start at 0
    let mut vals = [0i64; 4];
    if let Some(p) = argon2_params(hash) {
        scan_params(p, &mut vals);
    }
    new_time != vals[2] || new_memory != vals[1] || new_threads != vals[3]
}

/// The text after "$argon2i$" / "$argon2id$" (extract_argon2_parameters()).
fn argon2_params(hash: &[u8]) -> Option<&[u8]> {
    if hash.len() < b"$argon2id$".len() + 1 {
        return None;
    }
    let p = if hash.starts_with(b"$argon2i$") {
        &hash[b"$argon2i$".len()..]
    } else if hash.starts_with(b"$argon2id$") {
        &hash[b"$argon2id$".len()..]
    } else {
        return None;
    };
    Some(match p.iter().position(|&c| c == 0) {
        Some(n) => &p[..n],
        None => p,
    })
}

/// sscanf(p, "v=%ld$m=%ld,t=%ld,p=%ld", ...): the conversions up to the
/// first mismatch.
fn scan_params(p: &[u8], out: &mut [i64; 4]) {
    let lits: [&[u8]; 4] = [b"v=", b"$m=", b",t=", b",p="];
    let mut i = 0usize;
    for (k, lit) in lits.iter().enumerate() {
        for &c in lit.iter() {
            if i >= p.len() || p[i] != c {
                return;
            }
            i += 1;
        }
        // %ld: optional whitespace, sign, digits
        while i < p.len() && matches!(p[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
            i += 1;
        }
        let mut neg = false;
        if i < p.len() && (p[i] == b'+' || p[i] == b'-') {
            neg = p[i] == b'-';
            i += 1;
        }
        let start = i;
        let mut v: i64 = 0;
        let mut overflow = false;
        while i < p.len() && p[i].is_ascii_digit() {
            match v
                .checked_mul(10)
                .and_then(|x| x.checked_add((p[i] - b'0') as i64))
            {
                Some(x) => v = x,
                None => overflow = true,
            }
            i += 1;
        }
        if i == start {
            return;
        }
        out[k] = if overflow {
            if neg { i64::MIN } else { i64::MAX }
        } else if neg {
            v.wrapping_neg()
        } else {
            v
        };
    }
}

/// php_password_argon2_get_info()
fn argon2_get_info(options: &mut PhpArray, hash: &[u8]) {
    let mut vals = [0, MEMLIMIT, OPSLIMIT, THREADS];
    if let Some(p) = argon2_params(hash) {
        scan_params(p, &mut vals);
    }
    let key = |s: &[u8]| ArrayKey::String(PhpString::from_bytes(s));
    options.set(key(b"memory_cost"), Value::Long(vals[1]));
    options.set(key(b"time_cost"), Value::Long(vals[2]));
    options.set(key(b"threads"), Value::Long(vals[3]));
}

static ARGON2I: PasswordAlgoHandler = PasswordAlgoHandler {
    name: "argon2i",
    hash: argon2i_hash,
    verify: argon2_verify,
    needs_rehash: argon2_needs_rehash,
    get_info: argon2_get_info,
};

static ARGON2ID: PasswordAlgoHandler = PasswordAlgoHandler {
    name: "argon2id",
    hash: argon2id_hash,
    verify: argon2_verify,
    needs_rehash: argon2_needs_rehash,
    get_info: argon2_get_info,
};

/// PHP_MINIT_FUNCTION(password)'s HAVE_ARGON2LIB part: the algorithms and
/// their constants, which belong to ext/standard (no module list claims
/// them: get_defined_constants(true) files them under "standard").
pub fn minit(vm: &mut Vm) {
    // (registered once per process; the constants for every VM)
    if !password_algo_registered(b"argon2i") {
        password_algo_register(b"argon2i", &ARGON2I);
        password_algo_register(b"argon2id", &ARGON2ID);
    }
    let s = |b: &[u8]| Value::String(PhpString::from_bytes(b));
    let consts = [
        ("PASSWORD_ARGON2I", s(b"argon2i")),
        ("PASSWORD_ARGON2ID", s(b"argon2id")),
        ("PASSWORD_ARGON2_PROVIDER", s(b"standard")),
        ("PASSWORD_ARGON2_DEFAULT_MEMORY_COST", Value::Long(MEMLIMIT)),
        ("PASSWORD_ARGON2_DEFAULT_TIME_COST", Value::Long(OPSLIMIT)),
        ("PASSWORD_ARGON2_DEFAULT_THREADS", Value::Long(THREADS)),
    ];
    for (name, v) in consts {
        vm.constants.insert(name.as_bytes().to_vec(), v);
    }
}
