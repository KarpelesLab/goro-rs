//! The hash extension (PHP manual: Hash, HashContext; mhash functions)
//! on top of goro-ext-hash, which holds the algorithms, their errors and
//! the HashContext state formats.

use goro_ext_hash as h;
use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};

use crate::class::{NativeHooks, cflags};
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "hash",
    functions: &[
        (
            "function hash(string $algo, string $data, bool $binary = false, array $options = []): string",
            hash,
        ),
        (
            "function hash_file(string $algo, string $filename, bool $binary = false, array $options = []): string|false",
            hash_file,
        ),
        (
            "function hash_hmac(string $algo, string $data, #[\\SensitiveParameter] string $key, bool $binary = false): string",
            hash_hmac,
        ),
        (
            "function hash_hmac_file(string $algo, string $filename, #[\\SensitiveParameter] string $key, bool $binary = false): string|false",
            hash_hmac_file,
        ),
        (
            "function hash_init(string $algo, int $flags = 0, #[\\SensitiveParameter] string $key = \"\", array $options = []): HashContext",
            hash_init,
        ),
        (
            "function hash_update(HashContext $context, string $data): true",
            hash_update,
        ),
        (
            "function hash_update_stream(HashContext $context, $stream, int $length = -1): int",
            hash_update_stream,
        ),
        (
            "function hash_update_file(HashContext $context, string $filename, $context = null): bool",
            hash_update_file,
        ),
        (
            "function hash_final(HashContext $context, bool $binary = false): string",
            hash_final,
        ),
        (
            "function hash_copy(HashContext $context): HashContext",
            hash_copy,
        ),
        ("function hash_algos(): array", hash_algos),
        ("function hash_hmac_algos(): array", hash_hmac_algos),
        (
            "function hash_pbkdf2(string $algo, #[\\SensitiveParameter] string $password, string $salt, int $iterations, int $length = 0, bool $binary = false, array $options = []): string",
            hash_pbkdf2,
        ),
        (
            "function hash_equals(#[\\SensitiveParameter] $known_string, #[\\SensitiveParameter] $user_string): bool",
            hash_equals,
        ),
        (
            "function hash_hkdf(string $algo, #[\\SensitiveParameter] string $key, int $length = 0, string $info = \"\", string $salt = \"\"): string",
            hash_hkdf,
        ),
        (
            "function mhash(int $algo, string $data, ?string $key = null): string|false",
            mhash,
        ),
        (
            "function mhash_get_hash_name(int $algo): string|false",
            mhash_get_hash_name,
        ),
        (
            "function mhash_get_block_size(int $algo): int|false",
            mhash_get_block_size,
        ),
        ("function mhash_count(): int", mhash_count),
        (
            "function mhash_keygen_s2k(int $algo, string $password, string $salt, int $length): string|false",
            mhash_keygen_s2k,
        ),
    ],
    classes: &[(
        "hashcontext",
        "<?php final class HashContext {
            private function __construct() {}
            public function __serialize(): array {}
            public function __unserialize(array $data): void {}
            public function __debugInfo(): array {}
        }",
    )],
    methods: &[
        ("hashcontext::__serialize", ctx_serialize),
        ("hashcontext::__unserialize", ctx_unserialize),
        ("hashcontext::__debuginfo", ctx_debug_info),
    ],
    hooks: &[("hashcontext", ctx_hooks)],
    constants: &[
        ("HASH_HMAC", ConstVal::Int(1)),
        ("MHASH_CRC32", ConstVal::Int(0)),
        ("MHASH_MD5", ConstVal::Int(1)),
        ("MHASH_SHA1", ConstVal::Int(2)),
        ("MHASH_HAVAL256", ConstVal::Int(3)),
        ("MHASH_RIPEMD160", ConstVal::Int(5)),
        ("MHASH_TIGER", ConstVal::Int(7)),
        ("MHASH_GOST", ConstVal::Int(8)),
        ("MHASH_CRC32B", ConstVal::Int(9)),
        ("MHASH_HAVAL224", ConstVal::Int(10)),
        ("MHASH_HAVAL192", ConstVal::Int(11)),
        ("MHASH_HAVAL160", ConstVal::Int(12)),
        ("MHASH_HAVAL128", ConstVal::Int(13)),
        ("MHASH_TIGER128", ConstVal::Int(14)),
        ("MHASH_TIGER160", ConstVal::Int(15)),
        ("MHASH_MD4", ConstVal::Int(16)),
        ("MHASH_SHA256", ConstVal::Int(17)),
        ("MHASH_ADLER32", ConstVal::Int(18)),
        ("MHASH_SHA224", ConstVal::Int(19)),
        ("MHASH_SHA512", ConstVal::Int(20)),
        ("MHASH_SHA384", ConstVal::Int(21)),
        ("MHASH_WHIRLPOOL", ConstVal::Int(22)),
        ("MHASH_RIPEMD128", ConstVal::Int(23)),
        ("MHASH_RIPEMD256", ConstVal::Int(24)),
        ("MHASH_RIPEMD320", ConstVal::Int(25)),
        ("MHASH_SNEFRU256", ConstVal::Int(27)),
        ("MHASH_MD2", ConstVal::Int(28)),
        ("MHASH_FNV132", ConstVal::Int(29)),
        ("MHASH_FNV1A32", ConstVal::Int(30)),
        ("MHASH_FNV164", ConstVal::Int(31)),
        ("MHASH_FNV1A64", ConstVal::Int(32)),
        ("MHASH_JOAAT", ConstVal::Int(33)),
        ("MHASH_CRC32C", ConstVal::Int(34)),
        ("MHASH_MURMUR3A", ConstVal::Int(35)),
        ("MHASH_MURMUR3C", ConstVal::Int(36)),
        ("MHASH_MURMUR3F", ConstVal::Int(37)),
        ("MHASH_XXH32", ConstVal::Int(38)),
        ("MHASH_XXH64", ConstVal::Int(39)),
        ("MHASH_XXH3", ConstVal::Int(40)),
        ("MHASH_XXH128", ConstVal::Int(41)),
    ],
    ..Module::EMPTY
};

/// A HashContext object's state (none until hash_init() or
/// __unserialize() fills it).
struct CtxData(Option<h::HashContext>);

impl goro_runtime::NativeData for CtxData {}

fn with_ctx<T>(o: &ObjRef, f: impl FnOnce(&mut CtxData) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<CtxData>().map(f)
}

fn ctx_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|_, o| {
            *o.obj().native.borrow_mut() = Some(Box::new(CtxData(None)));
        }),
        clone: Some(|vm, from, to| {
            // Test ext/hash/tests/gh12186_2.phpt.
            if with_ctx(from, |d| d.0.as_ref().is_some_and(|c| c.is_finalized())) == Some(true)
                && vm.err.deferred.is_none()
            {
                vm.err.deferred =
                    Some(vm.throw_error_msg("Error", "Cannot clone a finalized HashContext"));
            }
            let c = with_ctx(from, |d| d.0.clone()).flatten();
            *to.obj().native.borrow_mut() = Some(Box::new(CtxData(c)));
        }),
        debug_props: Some(|_, o| {
            let mut out = PhpArray::new();
            if let Some(Some(n)) = with_ctx(o, |d| d.0.as_ref().map(|c| c.algo_name())) {
                out.insert(Key::from("algo"), Value::from(n));
            }
            Some(out)
        }),
        ..Default::default()
    }
}

/// Raises a goro-ext-hash error as the PHP exception it names.
fn throw<T>(vm: &mut Vm, e: h::HashError) -> R<T> {
    let (cls, m) = match &e {
        h::HashError::ValueError(m) => ("ValueError", m),
        h::HashError::TypeError(m) => ("TypeError", m),
        h::HashError::Error(m) => ("Error", m),
        h::HashError::Exception(m) => ("Exception", m),
    };
    vm.throw(cls, m)
}

fn diags(vm: &mut Vm, a: &Args, ds: Vec<h::Diag>) -> R<()> {
    for d in ds {
        match d {
            h::Diag::Warning(m) => vm.fwarn(a, &m)?,
            h::Diag::Deprecated(m) => vm.fdeprecated(a, &m)?,
        }
    }
    Ok(())
}

/// The `$options` array: `seed` and `secret`.
fn options(vm: &mut Vm, a: &Args, i: usize) -> R<h::HashOptions> {
    let mut o = h::HashOptions::default();
    let Value::Array(arr) = a.val(i).deref() else {
        return Ok(o);
    };
    if let Some(s) = arr.get_str(&PhpStr::from("seed")) {
        o.seed = match s.deref() {
            Value::Int(v) => h::Seed::Int(v),
            _ => h::Seed::NonInt,
        };
    }
    if let Some(s) = arr.get_str(&PhpStr::from("secret")) {
        let s = s.deref();
        o.secret = Some(match &s {
            Value::Str(b) => b.to_vec(),
            other => {
                let algo = lower_bytes(&a.str(0));
                if h::lookup(&algo).is_some_and(|al| al.uses_secret()) && o.seed == h::Seed::Absent
                {
                    vm.fdeprecated(a, h::SECRET_DEPRECATION)?;
                }
                vm.to_php_str(other)?.to_vec()
            }
        });
    }
    Ok(o)
}

fn lower_bytes(b: &[u8]) -> Vec<u8> {
    b.to_ascii_lowercase()
}

fn out(raw: Vec<u8>, binary: bool) -> Value {
    Value::Str(PhpStr::from(if binary { raw } else { h::to_hex(&raw) }))
}

fn hash(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let algo = match h::lookup_for("hash", &a.str(0)) {
        Ok(x) => x,
        Err(e) => return throw(vm, e),
    };
    let opts = options(vm, a, 3)?;
    let mut ds = Vec::new();
    let r = h::digest(algo, &a.str(1), &opts, &mut ds);
    diags(vm, a, ds)?;
    match r {
        Ok(d) => Ok(out(d, a.bool_or(2, false))),
        Err(e) => throw(vm, e),
    }
}

/// The contents of `filename` through PHP's streams (warns and gives
/// none when it can't be opened).
fn read_file(vm: &mut Vm, a: &Args, filename: &[u8]) -> R<Option<Vec<u8>>> {
    if filename.contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($filename) must not contain any null bytes",
        );
    }
    let Some(mut s) = super::file::open_stream(vm, a, filename, b"rb", false)? else {
        return Ok(None);
    };
    let mut data = Vec::new();
    let mut buf = vec![0u8; 65536];
    loop {
        match s.read_some(vm, &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => data.extend_from_slice(&buf[..n]),
        }
    }
    vm.check_deferred()?;
    Ok(Some(data))
}

fn hash_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let algo = match h::lookup_for("hash_file", &a.str(0)) {
        Ok(x) => x,
        Err(e) => return throw(vm, e),
    };
    let opts = options(vm, a, 3)?;
    let Some(data) = read_file(vm, a, &a.str(1))? else {
        return Ok(Value::Bool(false));
    };
    let mut ds = Vec::new();
    let r = h::digest(algo, &data, &opts, &mut ds);
    diags(vm, a, ds)?;
    match r {
        Ok(d) => Ok(out(d, a.bool_or(2, false))),
        Err(e) => throw(vm, e),
    }
}

fn hash_hmac(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let algo = match h::lookup_crypto("hash_hmac", &a.str(0)) {
        Ok(x) => x,
        Err(e) => return throw(vm, e),
    };
    Ok(out(
        h::hmac(algo, &a.str(2), &a.str(1)),
        a.bool_or(3, false),
    ))
}

fn hash_hmac_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let algo = match h::lookup_crypto("hash_hmac_file", &a.str(0)) {
        Ok(x) => x,
        Err(e) => return throw(vm, e),
    };
    let Some(data) = read_file(vm, a, &a.str(1))? else {
        return Ok(Value::Bool(false));
    };
    Ok(out(h::hmac(algo, &a.str(2), &data), a.bool_or(3, false)))
}

fn new_ctx_object(vm: &mut Vm, c: h::HashContext) -> R<Value> {
    let cls = vm
        .find_class(b"HashContext", false)?
        .expect("HashContext class");
    let o = vm.instantiate_raw(&cls)?;
    with_ctx(&o, |d| d.0 = Some(c));
    let _ = cflags::FINAL;
    Ok(Value::Object(o))
}

fn hash_init(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let key = if a.has(2) { Some(a.str(2)) } else { None };
    let opts = options(vm, a, 3)?;
    let mut ds = Vec::new();
    let r = h::HashContext::new(&a.str(0), a.int_or(1, 0), key.as_deref(), &opts, &mut ds);
    diags(vm, a, ds)?;
    match r {
        Ok(c) => new_ctx_object(vm, c),
        Err(e) => throw(vm, e),
    }
}

/// Runs `f` on the live context of argument 0 (TypeError when finalized).
fn on_ctx<T>(vm: &mut Vm, a: &Args, f: impl FnOnce(&mut h::HashContext) -> T) -> R<T> {
    let o = a.obj(0).expect("HashContext argument");
    let fname = a.fname();
    match with_ctx(&o, |d| match &mut d.0 {
        Some(c) if !c.is_finalized() => Some(f(c)),
        _ => None,
    })
    .flatten()
    {
        Some(v) => Ok(v),
        None => throw(vm, h::finalized_error(&fname)),
    }
}

fn hash_update(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.str(1);
    on_ctx(vm, a, |c| c.update(&data))?;
    Ok(Value::Bool(true))
}

fn hash_update_stream(vm: &mut Vm, a: &mut Args) -> R<Value> {
    on_ctx(vm, a, |_| ())?;
    let mut left = a.int_or(2, -1);
    let mut data = Vec::new();
    super::file::with_stream(vm, a, 1, |vm, s| {
        let mut buf = vec![0u8; 8192];
        while left != 0 {
            let want = if left < 0 {
                buf.len()
            } else {
                (left as usize).min(buf.len())
            };
            match s.read_some(vm, &mut buf[..want]) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    data.extend_from_slice(&buf[..n]);
                    if left > 0 {
                        left -= n as i64;
                    }
                }
            }
        }
        Ok(())
    })?;
    vm.check_deferred()?;
    let n = data.len() as i64;
    on_ctx(vm, a, |c| c.update(&data))?;
    Ok(Value::Int(n))
}

fn hash_update_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    on_ctx(vm, a, |_| ())?;
    let Some(data) = read_file(vm, a, &a.str(1))? else {
        return Ok(Value::Bool(false));
    };
    on_ctx(vm, a, |c| c.update(&data))?;
    Ok(Value::Bool(true))
}

fn hash_final(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = on_ctx(vm, a, |c| c.finalize())?.unwrap_or_default();
    Ok(out(d, a.bool_or(1, false)))
}

fn hash_copy(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = on_ctx(vm, a, |c| c.copy())?;
    match c {
        Some(c) => new_ctx_object(vm, c),
        None => throw(vm, h::finalized_error("hash_copy")),
    }
}

fn hash_algos(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(PhpArray::from_values(
        h::algos().map(Value::from),
    )))
}

fn hash_hmac_algos(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(PhpArray::from_values(
        h::hmac_algos().map(Value::from),
    )))
}

fn hash_pbkdf2(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match h::hash_pbkdf2(
        &a.str(0),
        &a.str(1),
        &a.str(2),
        a.int_or(3, 0),
        a.int_or(4, 0),
        a.bool_or(5, false),
    ) {
        Ok(v) => Ok(Value::Str(PhpStr::from(v))),
        Err(e) => throw(vm, e),
    }
}

fn hash_equals(vm: &mut Vm, a: &mut Args) -> R<Value> {
    // Strings only, in any typing mode (test ext/hash/tests/hash_equals.phpt).
    for (i, n) in [(0usize, "known_string"), (1, "user_string")] {
        let v = a.val(i).deref();
        if !matches!(v, Value::Str(_)) {
            return vm.fthrow(
                a,
                "TypeError",
                &format!(
                    "Argument #{} (${}) must be of type string, {} given",
                    i + 1,
                    n,
                    crate::vm::Vm::type_name_given(&v)
                ),
            );
        }
    }
    Ok(Value::Bool(h::hash_equals(&a.str(0), &a.str(1))))
}

fn hash_hkdf(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match h::hash_hkdf(&a.str(0), &a.str(1), a.int_or(2, 0), &a.str(3), &a.str(4)) {
        Ok(v) => Ok(Value::Str(PhpStr::from(v))),
        Err(e) => throw(vm, e),
    }
}

// ---------------------------------------------------------------- mhash

/// The hash algorithm of an MHASH_* id (names from hash_algos(); ids as
/// the MHASH_* constants have them on PHP 8.5).
fn mhash_algo(id: i64) -> Option<(&'static str, &'static str)> {
    Some(match id {
        0 => ("CRC32", "crc32"),
        1 => ("MD5", "md5"),
        2 => ("SHA1", "sha1"),
        3 => ("HAVAL256", "haval256,3"),
        5 => ("RIPEMD160", "ripemd160"),
        7 => ("TIGER", "tiger192,3"),
        8 => ("GOST", "gost"),
        9 => ("CRC32B", "crc32b"),
        10 => ("HAVAL224", "haval224,3"),
        11 => ("HAVAL192", "haval192,3"),
        12 => ("HAVAL160", "haval160,3"),
        13 => ("HAVAL128", "haval128,3"),
        14 => ("TIGER128", "tiger128,3"),
        15 => ("TIGER160", "tiger160,3"),
        16 => ("MD4", "md4"),
        17 => ("SHA256", "sha256"),
        18 => ("ADLER32", "adler32"),
        19 => ("SHA224", "sha224"),
        20 => ("SHA512", "sha512"),
        21 => ("SHA384", "sha384"),
        22 => ("WHIRLPOOL", "whirlpool"),
        23 => ("RIPEMD128", "ripemd128"),
        24 => ("RIPEMD256", "ripemd256"),
        25 => ("RIPEMD320", "ripemd320"),
        27 => ("SNEFRU256", "snefru256"),
        28 => ("MD2", "md2"),
        29 => ("FNV132", "fnv132"),
        30 => ("FNV1A32", "fnv1a32"),
        31 => ("FNV164", "fnv164"),
        32 => ("FNV1A64", "fnv1a64"),
        33 => ("JOAAT", "joaat"),
        34 => ("CRC32C", "crc32c"),
        35 => ("MURMUR3A", "murmur3a"),
        36 => ("MURMUR3C", "murmur3c"),
        37 => ("MURMUR3F", "murmur3f"),
        38 => ("XXH32", "xxh32"),
        39 => ("XXH64", "xxh64"),
        40 => ("XXH3", "xxh3"),
        41 => ("XXH128", "xxh128"),
        _ => return None,
    })
}

fn mhash_deprecated(vm: &mut Vm, a: &Args) -> R<()> {
    vm.deprecated(&format!("Function {}() is deprecated since 8.1", a.fname()))
}

fn mhash(vm: &mut Vm, a: &mut Args) -> R<Value> {
    mhash_deprecated(vm, a)?;
    let Some((_, name)) = mhash_algo(a.int_or(0, 0)) else {
        return Ok(Value::Bool(false));
    };
    let Some(algo) = h::lookup(name.as_bytes()) else {
        return Ok(Value::Bool(false));
    };
    let data = a.str(1);
    Ok(match a.opt_str(2) {
        Some(k) => Value::Str(PhpStr::from(h::hmac(algo, &k, &data))),
        None => Value::Str(PhpStr::from(h::Hasher::plain(algo).finish_with(&data))),
    })
}

trait FinishWith {
    fn finish_with(self, data: &[u8]) -> Vec<u8>;
}

impl FinishWith for h::Hasher {
    fn finish_with(mut self, data: &[u8]) -> Vec<u8> {
        self.update(data);
        self.finish()
    }
}

fn mhash_get_hash_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    mhash_deprecated(vm, a)?;
    Ok(match mhash_algo(a.int_or(0, 0)) {
        Some((n, _)) => Value::from(n),
        None => Value::Bool(false),
    })
}

fn mhash_get_block_size(vm: &mut Vm, a: &mut Args) -> R<Value> {
    mhash_deprecated(vm, a)?;
    Ok(
        match mhash_algo(a.int_or(0, 0)).and_then(|(_, n)| h::lookup(n.as_bytes())) {
            Some(al) => Value::Int(al.digest_len as i64),
            None => Value::Bool(false),
        },
    )
}

fn mhash_count(vm: &mut Vm, a: &mut Args) -> R<Value> {
    mhash_deprecated(vm, a)?;
    Ok(Value::Int(41))
}

/// The salted S2K of OpenPGP (RFC 4880 3.7.1.2): hashes of the salt and
/// password, each preceded by one more zero byte, until `length` bytes.
fn mhash_keygen_s2k(vm: &mut Vm, a: &mut Args) -> R<Value> {
    mhash_deprecated(vm, a)?;
    let length = a.int_or(3, 0);
    if length <= 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #4 ($length) must be a greater than 0",
        );
    }
    let Some(algo) = mhash_algo(a.int_or(0, 0)).and_then(|(_, n)| h::lookup(n.as_bytes())) else {
        return Ok(Value::Bool(false));
    };
    let mut salt = a.str(2).to_vec();
    salt.truncate(8);
    let pw = a.str(1);
    let mut outb = Vec::new();
    let mut i = 0;
    while outb.len() < length as usize {
        let mut hs = h::Hasher::plain(algo);
        hs.update(&vec![0u8; i]);
        hs.update(&salt);
        hs.update(&pw);
        outb.extend_from_slice(&hs.finish());
        i += 1;
    }
    outb.truncate(length as usize);
    Ok(Value::Str(PhpStr::from(outb)))
}

// ---------------------------------------------------------------- HashContext methods

fn ctx_serialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let r = with_ctx(&o, |d| d.0.as_ref().map(|c| c.serialize())).flatten();
    let s = match r {
        Some(Ok(s)) => s,
        Some(Err(e)) => return throw(vm, e),
        None => {
            return vm.throw(
                "Exception",
                "HashContext for algorithm \"\" cannot be serialized",
            );
        }
    };
    let mut arr = PhpArray::new();
    let _ = arr.push(Value::from(s.algo));
    let _ = arr.push(Value::Int(s.flags));
    let _ = arr.push(Value::Array(PhpArray::from_values(
        s.state.into_iter().map(|v| match v {
            h::SerVal::Int(i) => Value::Int(i),
            h::SerVal::Str(b) => Value::Str(PhpStr::from(b)),
        }),
    )));
    let _ = arr.push(Value::Int(s.magic));
    let _ = arr.push(Value::Array(vm.plain_props_array(&o, true)));
    Ok(Value::Array(arr))
}

fn ctx_unserialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if with_ctx(&o, |d| d.0.is_some()).unwrap_or(false) {
        return vm.throw("Exception", h::UNSERIALIZE_INITIALIZED);
    }
    let Value::Array(data) = a.val(0).deref() else {
        return Ok(Value::Null);
    };
    let get = |i: i64| data.get_int(i).map(|v| v.deref());
    let algo = match get(0) {
        Some(Value::Str(s)) => Some(s.to_vec()),
        _ => None,
    };
    let flags = match get(1) {
        Some(Value::Int(i)) => Some(i),
        _ => None,
    };
    let magic = match get(3) {
        Some(Value::Int(i)) => Some(i),
        _ => None,
    };
    let members = get(4);
    let members_ok = matches!(members, Some(Value::Array(_)));
    let state_v = get(2);
    // The state elements by integer key (None for missing keys).
    let vals: Vec<Option<Value>> = match &state_v {
        Some(Value::Array(arr)) => {
            let n = arr
                .iter()
                .filter_map(|(k, _)| match k {
                    goro_runtime::KeyRef::Int(i) if i >= 0 => Some(i as usize + 1),
                    _ => None,
                })
                .max()
                .unwrap_or(0);
            (0..n)
                .map(|i| arr.get_int(i as i64).map(|v| v.deref()))
                .collect()
        }
        _ => Vec::new(),
    };
    let state_arr: Vec<Option<h::SerInput<'_>>> = vals
        .iter()
        .map(|v| {
            v.as_ref().map(|v| match v {
                Value::Int(i) => h::SerInput::Int(*i),
                Value::Str(s) => h::SerInput::Str(s.as_bytes()),
                _ => h::SerInput::Other,
            })
        })
        .collect();
    let st = StateView(&state_arr);
    let slot = match &state_v {
        None => h::StateSlot::Missing,
        Some(Value::Array(_)) => h::StateSlot::Array(&st),
        Some(_) => h::StateSlot::NotArray,
    };
    let r = h::HashContext::unserialize(algo.as_deref(), flags, slot, magic, members_ok);
    match r {
        Ok(c) => {
            with_ctx(&o, |d| d.0 = Some(c));
            if let Some(Value::Array(m)) = members {
                for (k, v) in m.iter() {
                    if let goro_runtime::KeyRef::Str(n) = k {
                        vm.write_prop(&o, n, v.deref(), None)?;
                    }
                }
            }
            Ok(Value::Null)
        }
        Err(e) => throw(vm, e),
    }
}

fn ctx_debug_info(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut out = PhpArray::new();
    if let Some(Some(n)) = with_ctx(a.this_obj(), |d| d.0.as_ref().map(|c| c.algo_name())) {
        out.insert(Key::from("algo"), Value::from(n));
    }
    Ok(Value::Array(out))
}

/// A serialized state array as goro-ext-hash reads it.
struct StateView<'a>(&'a [Option<h::SerInput<'a>>]);

impl h::StateArray for StateView<'_> {
    fn get(&self, index: usize) -> Option<h::SerInput<'_>> {
        self.0.get(index).copied().flatten()
    }
}
