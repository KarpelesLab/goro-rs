//! sodium_pwhash.c: the argon2i / argon2id password_hash() algorithms
//! ext/sodium registers with ext/standard, and their constants.

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use goro_ext_standard::crypt::{
    PasswordAlgoHandler, PasswordOptions, option_long, password_algo_register,
    password_algo_registered,
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

/// get_options(): (memlimit in bytes, opslimit), or None after the
/// ValueError.
fn get_options(vm: &mut Vm, options: PasswordOptions) -> Option<(u64, u64)> {
    let mut opslimit = OPSLIMIT as u64;
    let mut memlimit = (MEMLIMIT as u64) << 10;
    if options.is_none() {
        return Some((memlimit, opslimit));
    }
    if let Some(m) = option_long(vm, options, b"memory_cost") {
        if m < 0
            || (m as u64) < pwhash::MEMLIMIT_MIN >> 10
            || (m as u64) > pwhash::MEMLIMIT_MAX >> 10
        {
            value_error(vm, "Memory cost is outside of allowed memory range");
            return None;
        }
        memlimit = (m as u64) << 10;
    }
    if let Some(t) = option_long(vm, options, b"time_cost") {
        opslimit = t as u64;
        if opslimit < pwhash::OPSLIMIT_MIN as u64 || opslimit > pwhash::OPSLIMIT_MAX {
            value_error(vm, "Time cost is outside of allowed time range");
            return None;
        }
    }
    if let Some(p) = option_long(vm, options, b"threads") {
        if p != 1 {
            value_error(
                vm,
                "A thread value other than 1 is not supported by this implementation",
            );
            return None;
        }
    }
    Some((memlimit, opslimit))
}

/// php_sodium_argon2_hash()
fn argon2_hash(
    vm: &mut Vm,
    password: &[u8],
    options: PasswordOptions,
    alg: i64,
) -> Result<Option<Vec<u8>>, VmError> {
    if password.len() as u64 >= 0xffff_ffff {
        value_error(vm, "Password is too long");
        return Ok(None);
    }
    let Some((memlimit, opslimit)) = get_options(vm, options) else {
        return Ok(None);
    };
    match pwhash::pwhash_str(password, opslimit, memlimit, alg) {
        Ok(h) => Ok(Some(h)),
        Err(()) => {
            value_error(vm, "Unexpected failure hashing password");
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

/// php_sodium_argon2_verify()
fn argon2_verify(password: &[u8], hash: &[u8]) -> bool {
    if password.len() as u64 >= 0xffff_ffff || hash.len() as u64 >= 0xffff_ffff {
        return false;
    }
    pwhash::str_verify(hash, password)
}

/// php_sodium_argon2_needs_rehash()
fn argon2_needs_rehash(vm: &mut Vm, hash: &[u8], options: PasswordOptions) -> bool {
    let Some((memlimit, opslimit)) = get_options(vm, options) else {
        return true;
    };
    pwhash::str_needs_rehash(hash, opslimit, memlimit) != 0
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

/// php_sodium_argon2_get_info()
fn argon2_get_info(options: &mut PhpArray, hash: &[u8]) {
    if hash.len() < b"$argon2id$".len() + 1 {
        return;
    }
    let p = if hash.starts_with(b"$argon2i$") {
        &hash[b"$argon2i$".len()..]
    } else if hash.starts_with(b"$argon2id$") {
        &hash[b"$argon2id$".len()..]
    } else {
        return;
    };
    let p = match p.iter().position(|&c| c == 0) {
        Some(n) => &p[..n],
        None => p,
    };
    let mut vals = [0, MEMLIMIT, OPSLIMIT, 1];
    scan_params(p, &mut vals);
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

/// PHP_MINIT_FUNCTION(sodium_password_hash)
pub fn minit(vm: &mut Vm, constants: &mut Vec<Vec<u8>>) {
    // (registered once per process; the constants for every VM)
    if !password_algo_registered(b"argon2i") {
        password_algo_register(b"argon2i", &ARGON2I);
        password_algo_register(b"argon2id", &ARGON2ID);
    }
    let s = |b: &[u8]| Value::String(PhpString::from_bytes(b));
    for (name, v) in [
        ("PASSWORD_ARGON2I", s(b"argon2i")),
        ("PASSWORD_ARGON2ID", s(b"argon2id")),
        ("PASSWORD_ARGON2_DEFAULT_MEMORY_COST", Value::Long(MEMLIMIT)),
        ("PASSWORD_ARGON2_DEFAULT_TIME_COST", Value::Long(OPSLIMIT)),
        ("PASSWORD_ARGON2_DEFAULT_THREADS", Value::Long(THREADS)),
        ("PASSWORD_ARGON2_PROVIDER", s(b"sodium")),
    ] {
        vm.constants.insert(name.as_bytes().to_vec(), v);
        constants.push(name.as_bytes().to_vec());
    }
}
