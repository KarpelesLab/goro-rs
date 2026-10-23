//! The mhash compatibility layer of ext/hash (built with --with-mhash):
//! mhash(), mhash_count(), mhash_get_block_size(), mhash_get_hash_name(),
//! mhash_keygen_s2k() and the MHASH_* constants, all deprecated.

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use crate::ops::fetch_ops;

/// mhash_to_hash[]: (MHASH_ name, hash algorithm) by mhash number.
const MHASH_TO_HASH: [Option<(&str, &str)>; 42] = [
    Some(("CRC32", "crc32")),
    Some(("MD5", "md5")),
    Some(("SHA1", "sha1")),
    Some(("HAVAL256", "haval256,3")),
    None,
    Some(("RIPEMD160", "ripemd160")),
    None,
    Some(("TIGER", "tiger192,3")),
    Some(("GOST", "gost")),
    Some(("CRC32B", "crc32b")),
    Some(("HAVAL224", "haval224,3")),
    Some(("HAVAL192", "haval192,3")),
    Some(("HAVAL160", "haval160,3")),
    Some(("HAVAL128", "haval128,3")),
    Some(("TIGER128", "tiger128,3")),
    Some(("TIGER160", "tiger160,3")),
    Some(("MD4", "md4")),
    Some(("SHA256", "sha256")),
    Some(("ADLER32", "adler32")),
    Some(("SHA224", "sha224")),
    Some(("SHA512", "sha512")),
    Some(("SHA384", "sha384")),
    Some(("WHIRLPOOL", "whirlpool")),
    Some(("RIPEMD128", "ripemd128")),
    Some(("RIPEMD256", "ripemd256")),
    Some(("RIPEMD320", "ripemd320")),
    None,
    Some(("SNEFRU256", "snefru256")),
    Some(("MD2", "md2")),
    Some(("FNV132", "fnv132")),
    Some(("FNV1A32", "fnv1a32")),
    Some(("FNV164", "fnv164")),
    Some(("FNV1A64", "fnv1a64")),
    Some(("JOAAT", "joaat")),
    Some(("CRC32C", "crc32c")),
    Some(("MURMUR3A", "murmur3a")),
    Some(("MURMUR3C", "murmur3c")),
    Some(("MURMUR3F", "murmur3f")),
    Some(("XXH32", "xxh32")),
    Some(("XXH64", "xxh64")),
    Some(("XXH3", "xxh3")),
    Some(("XXH128", "xxh128")),
];

fn lookup(algo: i64) -> Option<(&'static str, &'static str)> {
    usize::try_from(algo)
        .ok()
        .and_then(|i| MHASH_TO_HASH.get(i).copied().flatten())
}

pub fn register(vm: &mut Vm) {
    vm.register_function(b"mhash", mhash_fn);
    vm.register_function(b"mhash_count", mhash_count_fn);
    vm.register_function(b"mhash_get_block_size", mhash_get_block_size_fn);
    vm.register_function(b"mhash_get_hash_name", mhash_get_hash_name_fn);
    vm.register_function(b"mhash_keygen_s2k", mhash_keygen_s2k_fn);
    // mhash_init(): #[\Deprecated(since: "8.5")] constants
    let mut names: Vec<Vec<u8>> = vec![b"HASH_HMAC".to_vec()];
    for (i, e) in MHASH_TO_HASH.iter().enumerate() {
        if let Some((name, _)) = e {
            let cname = format!("MHASH_{}", name);
            names.push(cname.clone().into_bytes());
            vm.constants
                .insert(cname.clone().into_bytes(), Value::Long(i as i64));
            vm.internal_deprecated_constants.insert(
                cname.clone().into_bytes(),
                format!(
                    "Constant {} is deprecated since 8.5, as the mhash*() functions were deprecated",
                    cname
                ),
            );
        }
    }
    vm.module_constants.push((b"hash".to_vec(), names));
}

fn deprecated(vm: &mut Vm, f: &str) {
    vm.emit_deprecated(&format!("Function {}() is deprecated since 8.1", f));
}

fn long_arg(args: &[Value], i: usize) -> i64 {
    args.get(i).map(|v| v.to_long()).unwrap_or(0)
}

fn bytes_arg(args: &[Value], i: usize) -> Vec<u8> {
    args.get(i)
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default()
}

/// PHP_FUNCTION(mhash)
fn mhash_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    deprecated(vm, "mhash");
    let Some((_, algo)) = lookup(long_arg(args, 0)) else {
        return Ok(Value::False);
    };
    let data = bytes_arg(args, 1);
    let key = match args.get(2) {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.to_php_string().as_bytes().to_vec()),
    };
    let Some(ops) = fetch_ops(algo.as_bytes()) else {
        return Ok(Value::False);
    };
    let digest = match key {
        Some(key) => {
            if !ops.is_crypto {
                return crate::throw(
                    vm,
                    b"ValueError",
                    "mhash(): Argument #1 ($algo) must be a valid cryptographic hashing algorithm",
                );
            }
            let mut k = crate::hmac_prep_key(ops, &key);
            let inner = crate::hmac_round(ops, &k, &data);
            crate::xor_6a(&mut k);
            crate::hmac_round(ops, &k, &inner)
        }
        None => ops.digest(&data),
    };
    Ok(Value::String(PhpString::from_vec(digest)))
}

/// PHP_FUNCTION(mhash_get_hash_name)
fn mhash_get_hash_name_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    deprecated(vm, "mhash_get_hash_name");
    Ok(match lookup(long_arg(args, 0)) {
        Some((name, _)) => Value::String(PhpString::from_bytes(name.as_bytes())),
        None => Value::False,
    })
}

/// PHP_FUNCTION(mhash_count)
fn mhash_count_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    deprecated(vm, "mhash_count");
    Ok(Value::Long(MHASH_TO_HASH.len() as i64 - 1))
}

/// PHP_FUNCTION(mhash_get_block_size)
fn mhash_get_block_size_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    deprecated(vm, "mhash_get_block_size");
    Ok(lookup(long_arg(args, 0))
        .and_then(|(_, algo)| fetch_ops(algo.as_bytes()))
        .map(|ops| Value::Long(ops.digest_size as i64))
        .unwrap_or(Value::False))
}

/// PHP_FUNCTION(mhash_keygen_s2k)
fn mhash_keygen_s2k_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    deprecated(vm, "mhash_keygen_s2k");
    let algo = long_arg(args, 0);
    let password = bytes_arg(args, 1);
    let salt = bytes_arg(args, 2);
    let bytes = long_arg(args, 3) as i32;
    if bytes <= 0 {
        return crate::throw(
            vm,
            b"ValueError",
            "mhash_keygen_s2k(): Argument #4 ($length) must be a greater than 0",
        );
    }
    const SALT_SIZE: usize = 8;
    let mut padded_salt = [0u8; SALT_SIZE];
    let n = salt.len().min(SALT_SIZE);
    padded_salt[..n].copy_from_slice(&salt[..n]);
    let Some(ops) = lookup(algo).and_then(|(_, a)| fetch_ops(a.as_bytes())) else {
        return Ok(Value::False);
    };
    let block_size = ops.digest_size;
    let bytes = bytes as usize;
    let times = bytes.div_ceil(block_size);
    let mut key = Vec::with_capacity(times * block_size);
    for i in 0..times {
        let mut ctx = ops.new_ctx(None);
        for _ in 0..i {
            ctx.update(&[0]);
        }
        ctx.update(&padded_salt);
        ctx.update(&password);
        key.extend_from_slice(&crate::finish(ops, ctx));
    }
    key.truncate(bytes);
    Ok(Value::String(PhpString::from_vec(key)))
}
