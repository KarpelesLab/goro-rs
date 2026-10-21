//! Hash algorithm registry (php_hash_ops) and the context (un)serialization
//! spec engine of ext/hash/hash.c.
//!
//! Every algorithm context mirrors the C struct of php-src: `image()` /
//! `set_image()` convert it to and from the struct's bytes (x86_64 layout,
//! little endian), which is what `php_hash_serialize_spec()` walks.

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::algos;

/// A running hash computation (the `void *context` of php-src).
pub trait Ctx: CtxClone {
    fn update(&mut self, data: &[u8]);
    /// Write the digest (`digest_size` bytes) into `out`.
    fn finish(&mut self, out: &mut [u8]);
    /// The context as the bytes of its C struct.
    fn image(&self) -> Vec<u8> {
        Vec::new()
    }
    /// Load the context from the bytes of its C struct.
    fn set_image(&mut self, _img: &[u8]) {}
    /// Extra validation run after a successful unserialization.
    fn valid(&self) -> bool {
        true
    }
}

pub trait CtxClone {
    fn box_clone(&self) -> Box<dyn Ctx>;
}

impl<T: Ctx + Clone + 'static> CtxClone for T {
    fn box_clone(&self) -> Box<dyn Ctx> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn Ctx> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

/// The `HashTable *args` passed to an algorithm's init function, with what
/// it needs to report deprecations and errors.
pub struct InitArgs<'a> {
    pub vm: &'a mut Vm,
    pub args: &'a PhpArray,
    /// Name of the PHP function being run (for "fname(): ..." messages).
    pub fname: &'a str,
}

impl InitArgs<'_> {
    /// zend_hash_str_find_deref()
    pub fn find(&self, key: &str) -> Option<Value> {
        self.args.get_str(key.as_bytes()).map(|v| v.deref())
    }

    /// php_error_docref(NULL, E_DEPRECATED, ...)
    pub fn deprecated(&mut self, msg: &str) {
        let m = format!("{}(): {}", self.fname, msg);
        self.vm.emit_deprecated(&m);
    }

    /// php_error_docref(NULL, E_WARNING, ...)
    pub fn warning(&mut self, msg: &str) {
        let m = format!("{}(): {}", self.fname, msg);
        self.vm.emit_warning(&m);
    }

    /// zend_throw_error(NULL, ...)
    pub fn throw_error(&mut self, msg: &str) {
        if self.vm.current_exception.is_none() {
            let line = self.vm.current_line;
            let exc = self.vm.create_exception(b"Error", msg, line);
            self.vm.current_exception = Some(exc);
        }
    }
}

pub type InitFn = fn(Option<&mut InitArgs>) -> Box<dyn Ctx>;

/// php_hash_ops
pub struct Ops {
    pub algo: &'static str,
    pub init: InitFn,
    /// serialize_spec; None when the context can't be serialized.
    pub spec: Option<&'static str>,
    /// Magic number of the serialization format.
    pub magic: i64,
    pub digest_size: usize,
    pub block_size: usize,
    pub context_size: usize,
    pub is_crypto: bool,
}

impl Ops {
    pub fn new_ctx(&self, args: Option<&mut InitArgs>) -> Box<dyn Ctx> {
        (self.init)(args)
    }

    /// One-shot digest of `data`.
    pub fn digest(&self, data: &[u8]) -> Vec<u8> {
        let mut c = self.new_ctx(None);
        c.update(data);
        let mut out = vec![0u8; self.digest_size];
        c.finish(&mut out);
        out
    }
}

pub const PHP_HASH_SERIALIZE_MAGIC_SPEC: i64 = 2;
pub const HASH_SPEC_FAILURE: i64 = -1;
pub const WRONG_CONTEXT_SIZE: i64 = -999;
pub const BYTE_OFFSET_POS_ERROR: i64 = -1000;
pub const CONTEXT_VALIDATION_FAILURE: i64 = -2000;

macro_rules! ops {
    ($algo:expr, $init:expr, $spec:expr, $digest:expr, $block:expr, $size:expr, $crypto:expr) => {
        Ops {
            algo: $algo,
            init: $init,
            spec: $spec,
            magic: PHP_HASH_SERIALIZE_MAGIC_SPEC,
            digest_size: $digest,
            block_size: $block,
            context_size: $size,
            is_crypto: $crypto,
        }
    };
}

macro_rules! sha3_ops {
    ($algo:expr, $init:expr, $bits:expr) => {
        Ops {
            algo: $algo,
            init: $init,
            spec: Some("b200IiIIB"),
            magic: 100,
            digest_size: $bits / 8,
            block_size: (1600 - 2 * $bits) / 8,
            context_size: 224,
            is_crypto: true,
        }
    };
}

use algos::*;

pub static MD2: Ops = ops!("md2", md::md2_init, Some("b48b16b16b."), 16, 16, 81, true);
pub static MD4: Ops = ops!("md4", md::md4_init, Some("l4l2b64."), 16, 64, 88, true);
pub static MD5: Ops = ops!(
    "md5",
    md::md5_init,
    Some("llllllb64l16."),
    16,
    64,
    152,
    true
);
pub static SHA1: Ops = ops!("sha1", sha::sha1_init, Some("l5l2b64."), 20, 64, 92, true);
pub static SHA224: Ops = ops!(
    "sha224",
    sha::sha224_init,
    Some("l8l2b64."),
    28,
    64,
    104,
    true
);
pub static SHA256: Ops = ops!(
    "sha256",
    sha::sha256_init,
    Some("l8l2b64."),
    32,
    64,
    104,
    true
);
pub static SHA384: Ops = ops!(
    "sha384",
    sha::sha384_init,
    Some("q8q2b128."),
    48,
    128,
    208,
    true
);
pub static SHA512_224: Ops = ops!(
    "sha512/224",
    sha::sha512_224_init,
    Some("q8q2b128."),
    28,
    128,
    208,
    true
);
pub static SHA512_256: Ops = ops!(
    "sha512/256",
    sha::sha512_256_init,
    Some("q8q2b128."),
    32,
    128,
    208,
    true
);
pub static SHA512: Ops = ops!(
    "sha512",
    sha::sha512_init,
    Some("q8q2b128."),
    64,
    128,
    208,
    true
);
pub static SHA3_224: Ops = sha3_ops!("sha3-224", sha3::sha3_224_init, 224);
pub static SHA3_256: Ops = sha3_ops!("sha3-256", sha3::sha3_256_init, 256);
pub static SHA3_384: Ops = sha3_ops!("sha3-384", sha3::sha3_384_init, 384);
pub static SHA3_512: Ops = sha3_ops!("sha3-512", sha3::sha3_512_init, 512);
pub static RIPEMD128: Ops = ops!(
    "ripemd128",
    ripemd::r128_init,
    Some("l4l2b64."),
    16,
    64,
    88,
    true
);
pub static RIPEMD160: Ops = ops!(
    "ripemd160",
    ripemd::r160_init,
    Some("l5l2b64."),
    20,
    64,
    92,
    true
);
pub static RIPEMD256: Ops = ops!(
    "ripemd256",
    ripemd::r256_init,
    Some("l8l2b64."),
    32,
    64,
    104,
    true
);
pub static RIPEMD320: Ops = ops!(
    "ripemd320",
    ripemd::r320_init,
    Some("l10l2b64."),
    40,
    64,
    112,
    true
);
pub static WHIRLPOOL: Ops = ops!(
    "whirlpool",
    whirlpool::init,
    Some("q8b32iib64."),
    64,
    64,
    168,
    true
);
pub static TIGER128_3: Ops = ops!(
    "tiger128,3",
    tiger::init3_128,
    Some("q3qb64l"),
    16,
    64,
    104,
    true
);
pub static TIGER160_3: Ops = ops!(
    "tiger160,3",
    tiger::init3_160,
    Some("q3qb64l"),
    20,
    64,
    104,
    true
);
pub static TIGER192_3: Ops = ops!(
    "tiger192,3",
    tiger::init3_192,
    Some("q3qb64l"),
    24,
    64,
    104,
    true
);
pub static TIGER128_4: Ops = ops!(
    "tiger128,4",
    tiger::init4_128,
    Some("q3qb64l"),
    16,
    64,
    104,
    true
);
pub static TIGER160_4: Ops = ops!(
    "tiger160,4",
    tiger::init4_160,
    Some("q3qb64l"),
    20,
    64,
    104,
    true
);
pub static TIGER192_4: Ops = ops!(
    "tiger192,4",
    tiger::init4_192,
    Some("q3qb64l"),
    24,
    64,
    104,
    true
);
pub static SNEFRU: Ops = ops!("snefru", snefru::init, Some("l16l2bb32"), 32, 32, 108, true);
pub static GOST: Ops = ops!("gost", gost::init, Some("l16l2bb32"), 32, 32, 120, true);
pub static GOST_CRYPTO: Ops = ops!(
    "gost-crypto",
    gost::init_crypto,
    Some("l16l2bb32"),
    32,
    32,
    120,
    true
);
pub static ADLER32: Ops = ops!("adler32", simple::adler32_init, Some("l."), 4, 4, 4, false);
pub static CRC32: Ops = ops!("crc32", simple::crc32_init, Some("l."), 4, 4, 4, false);
pub static CRC32B: Ops = ops!("crc32b", simple::crc32b_init, Some("l."), 4, 4, 4, false);
pub static CRC32C: Ops = ops!("crc32c", simple::crc32c_init, Some("l."), 4, 4, 4, false);
pub static FNV132: Ops = ops!("fnv132", simple::fnv132_init, Some("l."), 4, 4, 4, false);
pub static FNV1A32: Ops = ops!("fnv1a32", simple::fnv1a32_init, Some("l."), 4, 4, 4, false);
pub static FNV164: Ops = ops!("fnv164", simple::fnv164_init, Some("q."), 8, 4, 8, false);
pub static FNV1A64: Ops = ops!("fnv1a64", simple::fnv1a64_init, Some("q."), 8, 4, 8, false);
pub static JOAAT: Ops = ops!("joaat", simple::joaat_init, Some("l."), 4, 4, 4, false);
pub static MURMUR3A: Ops = ops!("murmur3a", murmur::m3a_init, Some("lll"), 4, 4, 12, false);
pub static MURMUR3C: Ops = ops!(
    "murmur3c",
    murmur::m3c_init,
    Some("lllllllll"),
    16,
    4,
    36,
    false
);
pub static MURMUR3F: Ops = ops!(
    "murmur3f",
    murmur::m3f_init,
    Some("qqqql"),
    16,
    8,
    40,
    false
);
pub static XXH32: Ops = ops!(
    "xxh32",
    xxhash::xxh32_init,
    Some("llllllllllll"),
    4,
    4,
    48,
    false
);
pub static XXH64: Ops = ops!(
    "xxh64",
    xxhash::xxh64_init,
    Some("qqqqqqqqqllq"),
    8,
    8,
    88,
    false
);
pub static XXH3_64: Ops = ops!("xxh3", xxhash::xxh3_init, None, 8, 8, 832, false);
pub static XXH3_128: Ops = ops!("xxh128", xxhash::xxh128_init, None, 16, 8, 832, false);

macro_rules! haval_ops {
    ($name:ident, $algo:expr, $init:expr, $bits:expr) => {
        pub static $name: Ops = ops!($algo, $init, Some("l8l2b128"), $bits / 8, 128, 184, true);
    };
}
haval_ops!(HAVAL128_3, "haval128,3", haval::init_3_128, 128);
haval_ops!(HAVAL160_3, "haval160,3", haval::init_3_160, 160);
haval_ops!(HAVAL192_3, "haval192,3", haval::init_3_192, 192);
haval_ops!(HAVAL224_3, "haval224,3", haval::init_3_224, 224);
haval_ops!(HAVAL256_3, "haval256,3", haval::init_3_256, 256);
haval_ops!(HAVAL128_4, "haval128,4", haval::init_4_128, 128);
haval_ops!(HAVAL160_4, "haval160,4", haval::init_4_160, 160);
haval_ops!(HAVAL192_4, "haval192,4", haval::init_4_192, 192);
haval_ops!(HAVAL224_4, "haval224,4", haval::init_4_224, 224);
haval_ops!(HAVAL256_4, "haval256,4", haval::init_4_256, 256);
haval_ops!(HAVAL128_5, "haval128,5", haval::init_5_128, 128);
haval_ops!(HAVAL160_5, "haval160,5", haval::init_5_160, 160);
haval_ops!(HAVAL192_5, "haval192,5", haval::init_5_192, 192);
haval_ops!(HAVAL224_5, "haval224,5", haval::init_5_224, 224);
haval_ops!(HAVAL256_5, "haval256,5", haval::init_5_256, 256);

/// The algorithm registry in PHP_MINIT_FUNCTION(hash) registration order
/// (the order of hash_algos()).
pub static REGISTRY: &[(&str, &Ops)] = &[
    ("md2", &MD2),
    ("md4", &MD4),
    ("md5", &MD5),
    ("sha1", &SHA1),
    ("sha224", &SHA224),
    ("sha256", &SHA256),
    ("sha384", &SHA384),
    ("sha512/224", &SHA512_224),
    ("sha512/256", &SHA512_256),
    ("sha512", &SHA512),
    ("sha3-224", &SHA3_224),
    ("sha3-256", &SHA3_256),
    ("sha3-384", &SHA3_384),
    ("sha3-512", &SHA3_512),
    ("ripemd128", &RIPEMD128),
    ("ripemd160", &RIPEMD160),
    ("ripemd256", &RIPEMD256),
    ("ripemd320", &RIPEMD320),
    ("whirlpool", &WHIRLPOOL),
    ("tiger128,3", &TIGER128_3),
    ("tiger160,3", &TIGER160_3),
    ("tiger192,3", &TIGER192_3),
    ("tiger128,4", &TIGER128_4),
    ("tiger160,4", &TIGER160_4),
    ("tiger192,4", &TIGER192_4),
    ("snefru", &SNEFRU),
    ("snefru256", &SNEFRU),
    ("gost", &GOST),
    ("gost-crypto", &GOST_CRYPTO),
    ("adler32", &ADLER32),
    ("crc32", &CRC32),
    ("crc32b", &CRC32B),
    ("crc32c", &CRC32C),
    ("fnv132", &FNV132),
    ("fnv1a32", &FNV1A32),
    ("fnv164", &FNV164),
    ("fnv1a64", &FNV1A64),
    ("joaat", &JOAAT),
    ("murmur3a", &MURMUR3A),
    ("murmur3c", &MURMUR3C),
    ("murmur3f", &MURMUR3F),
    ("xxh32", &XXH32),
    ("xxh64", &XXH64),
    ("xxh3", &XXH3_64),
    ("xxh128", &XXH3_128),
    ("haval128,3", &HAVAL128_3),
    ("haval160,3", &HAVAL160_3),
    ("haval192,3", &HAVAL192_3),
    ("haval224,3", &HAVAL224_3),
    ("haval256,3", &HAVAL256_3),
    ("haval128,4", &HAVAL128_4),
    ("haval160,4", &HAVAL160_4),
    ("haval192,4", &HAVAL192_4),
    ("haval224,4", &HAVAL224_4),
    ("haval256,4", &HAVAL256_4),
    ("haval128,5", &HAVAL128_5),
    ("haval160,5", &HAVAL160_5),
    ("haval192,5", &HAVAL192_5),
    ("haval224,5", &HAVAL224_5),
    ("haval256,5", &HAVAL256_5),
];

/// php_hash_fetch_ops(): case-insensitive lookup.
pub fn fetch_ops(algo: &[u8]) -> Option<&'static Ops> {
    let lower = algo.to_ascii_lowercase();
    REGISTRY
        .iter()
        .find(|(n, _)| n.as_bytes() == lower.as_slice())
        .map(|(_, o)| *o)
}

// ---------------------------------------------------------------------------
// Serialization spec engine (php_hash_serialize_spec / _unserialize_spec)

fn align_to(pos: usize, alignment: usize) -> usize {
    let offset = pos & (alignment - 1);
    pos + if offset != 0 { alignment - offset } else { 0 }
}

struct SpecItem {
    ch: u8,
    sz: usize,
    count: usize,
}

/// parse_serialize_spec(): one item, updating the position for alignment.
fn parse_item(spec: &[u8], i: &mut usize, pos: &mut usize, max_align: &mut usize) -> SpecItem {
    let ch = spec[*i];
    let (sz, alignment) = match ch {
        b's' | b'S' => (2, 2),
        b'l' | b'L' => (4, 4),
        b'q' | b'Q' => (8, 8),
        b'i' | b'I' => (4, 4),
        _ => (1, 1),
    };
    *pos = align_to(*pos, alignment);
    if *max_align < alignment {
        *max_align = alignment;
    }
    *i += 1;
    let count = if *i < spec.len() && spec[*i].is_ascii_digit() {
        let mut c = 0usize;
        while *i < spec.len() && spec[*i].is_ascii_digit() {
            c = 10 * c + (spec[*i] - b'0') as usize;
            *i += 1;
        }
        c
    } else {
        1
    };
    SpecItem { ch, sz, count }
}

fn one_from_buffer(sz: usize, buf: &[u8]) -> u64 {
    match sz {
        2 => u16::from_le_bytes([buf[0], buf[1]]) as u64,
        4 => u32::from_le_bytes(buf[..4].try_into().unwrap()) as u64,
        8 => u64::from_le_bytes(buf[..8].try_into().unwrap()),
        _ => buf[0] as u64,
    }
}

fn one_to_buffer(sz: usize, buf: &mut [u8], val: u64) {
    match sz {
        2 => buf[..2].copy_from_slice(&(val as u16).to_le_bytes()),
        4 => buf[..4].copy_from_slice(&(val as u32).to_le_bytes()),
        8 => buf[..8].copy_from_slice(&val.to_le_bytes()),
        _ => buf[0] = val as u8,
    }
}

/// php_hash_serialize_spec(): the serialized state array, or None on failure.
pub fn serialize_spec(buf: &[u8], context_size: usize, spec: &str) -> Option<PhpArray> {
    let spec = spec.as_bytes();
    let mut arr = PhpArray::new();
    let (mut pos, mut max_align, mut i) = (0usize, 1usize, 0usize);
    while i < spec.len() && spec[i] != b'.' {
        let it = parse_item(spec, &mut i, &mut pos, &mut max_align);
        if pos + it.count * it.sz > context_size {
            return None;
        }
        if it.ch.is_ascii_uppercase() {
            pos += it.count * it.sz;
        } else if it.sz == 1 && it.count > 1 {
            arr.push(Value::String(PhpString::from_vec(
                buf[pos..pos + it.count].to_vec(),
            )));
            pos += it.count;
        } else {
            for _ in 0..it.count {
                let val = one_from_buffer(it.sz, &buf[pos..]);
                pos += it.sz;
                arr.push(Value::Long(val as i32 as i64));
                if it.sz == 8 {
                    arr.push(Value::Long((val >> 32) as i32 as i64));
                }
            }
        }
    }
    if i < spec.len() && spec[i] == b'.' && align_to(pos, max_align) != context_size {
        return None;
    }
    Some(arr)
}

/// php_hash_unserialize_spec(): 0 on success, else the (negative) error code.
pub fn unserialize_spec(buf: &mut [u8], context_size: usize, zv: &Value, spec: &str) -> i64 {
    let arr = match zv {
        Value::Array(a) => a.borrow(),
        _ => return HASH_SPEC_FAILURE,
    };
    let spec = spec.as_bytes();
    let (mut pos, mut max_align, mut i, mut j) = (0usize, 1usize, 0usize, 0i64);
    while i < spec.len() && spec[i] != b'.' {
        let it = parse_item(spec, &mut i, &mut pos, &mut max_align);
        if pos + it.count * it.sz > context_size {
            return WRONG_CONTEXT_SIZE;
        }
        if it.ch.is_ascii_uppercase() {
            pos += it.count * it.sz;
        } else if it.sz == 1 && it.count > 1 {
            match arr.get(&ArrayKey::Int(j)) {
                Some(Value::String(s)) if s.as_bytes().len() == it.count => {
                    buf[pos..pos + it.count].copy_from_slice(s.as_bytes());
                }
                _ => return BYTE_OFFSET_POS_ERROR - pos as i64,
            }
            j += 1;
            pos += it.count;
        } else {
            for _ in 0..it.count {
                let mut val = match arr.get(&ArrayKey::Int(j)) {
                    Some(Value::Long(n)) => *n as u32 as u64,
                    _ => return BYTE_OFFSET_POS_ERROR - pos as i64,
                };
                j += 1;
                if it.sz == 8 {
                    match arr.get(&ArrayKey::Int(j)) {
                        Some(Value::Long(n)) => val = val.wrapping_add((*n as u64) << 32),
                        _ => return BYTE_OFFSET_POS_ERROR - pos as i64,
                    }
                    j += 1;
                }
                one_to_buffer(it.sz, &mut buf[pos..], val);
                pos += it.sz;
            }
        }
    }
    if i < spec.len() && spec[i] == b'.' && align_to(pos, max_align) != context_size {
        return WRONG_CONTEXT_SIZE;
    }
    0
}

// ---------------------------------------------------------------------------
// Helpers for building C struct images

/// Little-endian writer/reader over a C struct image.
pub struct Img<'a>(pub &'a mut [u8]);

impl Img<'_> {
    pub fn u32s(&mut self, off: usize, v: &[u32]) {
        for (k, x) in v.iter().enumerate() {
            self.0[off + 4 * k..off + 4 * k + 4].copy_from_slice(&x.to_le_bytes());
        }
    }
    pub fn u64s(&mut self, off: usize, v: &[u64]) {
        for (k, x) in v.iter().enumerate() {
            self.0[off + 8 * k..off + 8 * k + 8].copy_from_slice(&x.to_le_bytes());
        }
    }
    pub fn bytes(&mut self, off: usize, v: &[u8]) {
        self.0[off..off + v.len()].copy_from_slice(v);
    }
}

pub fn rd_u32(img: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(img[off..off + 4].try_into().unwrap())
}

pub fn rd_u64(img: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(img[off..off + 8].try_into().unwrap())
}

pub fn rd_u32s<const N: usize>(img: &[u8], off: usize) -> [u32; N] {
    let mut a = [0u32; N];
    for (k, x) in a.iter_mut().enumerate() {
        *x = rd_u32(img, off + 4 * k);
    }
    a
}

pub fn rd_u64s<const N: usize>(img: &[u8], off: usize) -> [u64; N] {
    let mut a = [0u64; N];
    for (k, x) in a.iter_mut().enumerate() {
        *x = rd_u64(img, off + 8 * k);
    }
    a
}

pub fn rd_bytes<const N: usize>(img: &[u8], off: usize) -> [u8; N] {
    img[off..off + N].try_into().unwrap()
}
