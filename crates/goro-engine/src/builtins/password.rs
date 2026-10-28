//! crypt() and the password hashing API (PHP manual: crypt, Password
//! Hashing): bcrypt and Argon2i/Argon2id, with messages from tests
//! ext/standard/tests/password/*.phpt and observations of PHP 8.5.

use goro_ext_standard::argon2::{self, Variant};
use goro_ext_standard::crypt as cr;
use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::math::os_random;
use super::string::sv;
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[
        (
            "function crypt(string $string, string $salt): string",
            crypt,
        ),
        (
            "function password_hash(string $password, string|int|null $algo, array $options = []): string",
            password_hash,
        ),
        (
            "function password_verify(string $password, string $hash): bool",
            password_verify,
        ),
        (
            "function password_get_info(string $hash): array",
            password_get_info,
        ),
        (
            "function password_needs_rehash(string $hash, string|int|null $algo, array $options = []): bool",
            password_needs_rehash,
        ),
        ("function password_algos(): array", password_algos),
    ],
    constants: &[
        ("PASSWORD_DEFAULT", ConstVal::Str("2y")),
        ("PASSWORD_BCRYPT", ConstVal::Str("2y")),
        ("PASSWORD_BCRYPT_DEFAULT_COST", ConstVal::Int(BCRYPT_COST)),
        ("PASSWORD_ARGON2I", ConstVal::Str("argon2i")),
        ("PASSWORD_ARGON2ID", ConstVal::Str("argon2id")),
        (
            "PASSWORD_ARGON2_DEFAULT_MEMORY_COST",
            ConstVal::Int(ARGON_M as i64),
        ),
        (
            "PASSWORD_ARGON2_DEFAULT_TIME_COST",
            ConstVal::Int(ARGON_T as i64),
        ),
        (
            "PASSWORD_ARGON2_DEFAULT_THREADS",
            ConstVal::Int(ARGON_P as i64),
        ),
        ("PASSWORD_ARGON2_PROVIDER", ConstVal::Str("standard")),
    ],
    listed: false,
    ..Module::EMPTY
};

/// Default bcrypt cost (PHP 8.4 and later: 12).
const BCRYPT_COST: i64 = 12;
const ARGON_M: u32 = 65536;
const ARGON_T: u32 = 4;
const ARGON_P: u32 = 1;

fn crypt(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(sv(cr::crypt(a.bytes(0), a.bytes(1))))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Algo {
    Bcrypt,
    Argon(Variant),
}

/// The `$algo` argument: None for an unknown algorithm.
fn algo_of(v: &Value) -> Option<Algo> {
    match v {
        Value::Null => Some(Algo::Bcrypt),
        Value::Int(1) => Some(Algo::Bcrypt),
        Value::Int(2) => Some(Algo::Argon(Variant::I)),
        Value::Int(3) => Some(Algo::Argon(Variant::Id)),
        Value::Str(s) => match s.as_bytes() {
            b"2y" => Some(Algo::Bcrypt),
            b"argon2i" => Some(Algo::Argon(Variant::I)),
            b"argon2id" => Some(Algo::Argon(Variant::Id)),
            _ => None,
        },
        _ => None,
    }
}

fn opt(vm: &mut Vm, opts: &PhpArray, name: &str) -> R<Option<i64>> {
    match opts.get_bytes(name.as_bytes()) {
        Some(v) => {
            let v = v.deref();
            Ok(Some(vm.to_int(&v)?))
        }
        None => Ok(None),
    }
}

fn random16() -> [u8; 16] {
    let mut b = [0u8; 16];
    os_random(&mut b);
    b
}

fn password_hash(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pw = a.str(0);
    let Some(algo) = algo_of(&a.val(1)) else {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($algo) must be a valid password hashing algorithm",
        );
    };
    let opts = a.array(2);
    if opts.get_bytes(b"salt").is_some() {
        vm.fwarn(
            a,
            "The \"salt\" option has been ignored, since providing a custom salt is no longer supported",
        )?;
    }
    match algo {
        Algo::Bcrypt => {
            let cost = opt(vm, &opts, "cost")?.unwrap_or(BCRYPT_COST);
            if !(4..=31).contains(&cost) {
                return vm.throw(
                    "ValueError",
                    &format!("Invalid bcrypt cost parameter specified: {}", cost),
                );
            }
            if pw.contains(&0) {
                return vm.throw(
                    "ValueError",
                    "Bcrypt password must not contain null character",
                );
            }
            let mut setting = format!("$2y${:02}$", cost).into_bytes();
            setting.extend(cr::bcrypt_salt(&random16()));
            match cr::bcrypt(&pw, &setting) {
                Some(h) => Ok(sv(h)),
                None => vm.throw("Error", "Bcrypt hashing failed"),
            }
        }
        Algo::Argon(variant) => {
            let m = opt(vm, &opts, "memory_cost")?.unwrap_or(ARGON_M as i64);
            let t = opt(vm, &opts, "time_cost")?.unwrap_or(ARGON_T as i64);
            let p = opt(vm, &opts, "threads")?.unwrap_or(ARGON_P as i64);
            if !(8..=u32::MAX as i64).contains(&m) {
                return vm.throw(
                    "ValueError",
                    "Memory cost is outside of allowed memory range",
                );
            }
            if !(1..=u32::MAX as i64).contains(&t) {
                return vm.throw("ValueError", "Time cost is outside of allowed time range");
            }
            if !(1..=0xff_ffff).contains(&p) {
                return vm.throw("ValueError", "Invalid number of threads");
            }
            let salt = random16();
            let h = argon2::hash(variant, &pw, &salt, t as u32, m as u32, p as u32, 32);
            Ok(sv(argon2::encode(
                variant, m as u32, t as u32, p as u32, &salt, &h,
            )))
        }
    }
}

fn password_verify(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pw = a.bytes(0);
    let hash = a.bytes(1);
    if hash.starts_with(b"$argon2") {
        return Ok(Value::Bool(argon2::verify(pw, hash)));
    }
    let c = cr::crypt(pw, hash);
    let eq =
        c.len() == hash.len() && c.iter().zip(hash).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0;
    Ok(Value::Bool(eq && !hash.is_empty()))
}

/// What password_get_info() knows about a hash.
fn info(hash: &[u8]) -> Option<(Algo, Vec<(&'static str, i64)>)> {
    if hash.len() == 60 && hash.starts_with(b"$2y$") && hash[6] == b'$' {
        let cost = std::str::from_utf8(&hash[4..6]).ok()?.parse::<i64>().ok()?;
        return Some((Algo::Bcrypt, vec![("cost", cost)]));
    }
    if hash.starts_with(b"$argon2") {
        let e = argon2::decode(hash)?;
        if e.variant == Variant::D {
            return None;
        }
        return Some((
            Algo::Argon(e.variant),
            vec![
                ("memory_cost", e.m as i64),
                ("time_cost", e.t as i64),
                ("threads", e.p as i64),
            ],
        ));
    }
    None
}

fn algo_name(a: Algo) -> &'static str {
    match a {
        Algo::Bcrypt => "2y",
        Algo::Argon(Variant::I) => "argon2i",
        Algo::Argon(_) => "argon2id",
    }
}

fn password_get_info(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut out = PhpArray::new();
    let mut opts = PhpArray::new();
    match info(a.bytes(0)) {
        Some((algo, params)) => {
            out.insert(Key::from("algo"), Value::from(algo_name(algo)));
            let name = if algo == Algo::Bcrypt {
                "bcrypt"
            } else {
                algo_name(algo)
            };
            out.insert(Key::from("algoName"), Value::from(name));
            for (k, v) in params {
                opts.insert(Key::from(k), Value::Int(v));
            }
        }
        None => {
            out.insert(Key::from("algo"), Value::Null);
            out.insert(Key::from("algoName"), Value::from("unknown"));
        }
    }
    out.insert(Key::from("options"), Value::Array(opts));
    Ok(Value::Array(out))
}

fn password_needs_rehash(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(want) = algo_of(&a.val(1)) else {
        return Ok(Value::Bool(true));
    };
    let opts = a.array(2);
    let Some((have, params)) = info(a.bytes(0)) else {
        return Ok(Value::Bool(true));
    };
    if have != want {
        return Ok(Value::Bool(true));
    }
    let get = |k: &str| {
        params
            .iter()
            .find(|(n, _)| *n == k)
            .map(|x| x.1)
            .unwrap_or(0)
    };
    let differs = match want {
        Algo::Bcrypt => opt(vm, &opts, "cost")?.unwrap_or(BCRYPT_COST) != get("cost"),
        Algo::Argon(_) => {
            opt(vm, &opts, "memory_cost")?.unwrap_or(ARGON_M as i64) != get("memory_cost")
                || opt(vm, &opts, "time_cost")?.unwrap_or(ARGON_T as i64) != get("time_cost")
                || opt(vm, &opts, "threads")?.unwrap_or(ARGON_P as i64) != get("threads")
        }
    };
    Ok(Value::Bool(differs))
}

fn password_algos(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(PhpArray::from_values(
        ["2y", "argon2i", "argon2id"]
            .into_iter()
            .map(|s| Value::Str(PhpStr::from(s))),
    )))
}
