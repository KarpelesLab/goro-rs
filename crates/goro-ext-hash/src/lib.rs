//! PHP's hash extension on bytes: every algorithm of `hash_algos()` with
//! streaming contexts (`hash_init()` / `hash_update()` / `hash_final()` /
//! `hash_copy()`), HMAC, HKDF, PBKDF2, `hash_equals()`, the `seed` and
//! `secret` options, and `HashContext` serialization.
//!
//! The functions work on plain Rust values: errors come back as
//! [`HashError`] values carrying PHP's full message, and diagnostics PHP
//! emits without throwing are pushed into a `Vec<Diag>` (their messages
//! exclude the `func(): ` prefix, which the wiring layer adds).
//!
//! Behaviour sources: the PHP manual's hash pages, the algorithms'
//! specifications (cited per module), the tests in ext/hash/tests, and
//! observation of PHP 8.5 (tests/fixtures/).

// Hash rounds index several parallel arrays by position, and HAVAL's
// boolean functions take seven words; the lints would only obscure them.
#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

mod checksum;
mod crc_fold;
mod engine;
mod gost;
mod haval;
mod md;
mod murmur;
mod ripemd;
mod sha;
mod sha3;
mod shani;
mod snefru;
mod tables;
mod tiger;
mod whirlpool;
mod xxh;
mod xxh3_lanes;

use engine::{Engine, Field};
pub use engine::{SerInput, SerVal, StateArray};
pub use xxh::{XXH3_SECRET_MAX, XXH3_SECRET_MIN};

/// An error PHP throws, with its full `getMessage()` text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashError {
    /// `ValueError`.
    ValueError(String),
    /// `TypeError`.
    TypeError(String),
    /// `Error`.
    Error(String),
    /// `Exception`.
    Exception(String),
}

impl HashError {
    /// The message.
    pub fn message(&self) -> &str {
        match self {
            HashError::ValueError(m)
            | HashError::TypeError(m)
            | HashError::Error(m)
            | HashError::Exception(m) => m,
        }
    }
}

/// A diagnostic PHP emits without throwing; the message excludes the
/// `func(): ` prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diag {
    /// `E_WARNING`.
    Warning(String),
    /// `E_DEPRECATED`.
    Deprecated(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Md2,
    Md4,
    Md5,
    Sha1,
    Sha224,
    Sha256,
    Sha384,
    Sha512_224,
    Sha512_256,
    Sha512,
    Sha3(usize),
    Ripemd(usize),
    Whirlpool,
    Tiger { passes: usize, len: usize },
    Snefru,
    Gost { crypto: bool },
    Adler32,
    Crc32(checksum::CrcKind),
    Fnv132 { a: bool },
    Fnv164 { a: bool },
    Joaat,
    Murmur3a,
    Murmur3c,
    Murmur3f,
    Xxh32,
    Xxh64,
    Xxh3,
    Xxh128,
    Haval { bits: usize, passes: usize },
}

/// One hash algorithm.
#[derive(Debug)]
pub struct Algo {
    /// The name `hash_algos()` lists.
    pub name: &'static str,
    /// The name a context reports (`HashContext::__debugInfo()`, the
    /// serialized form); `snefru256` is an alias of `snefru`.
    pub context_name: &'static str,
    /// Digest length in bytes.
    pub digest_len: usize,
    /// Block length in bytes, used to pad HMAC keys.
    pub block_len: usize,
    /// Whether HMAC, HKDF and PBKDF2 accept it (`hash_hmac_algos()`).
    pub crypto: bool,
    kind: Kind,
    layout: &'static [Field],
}

impl Algo {
    /// Whether `serialize()` works on its contexts.
    pub fn serializable(&self) -> bool {
        !matches!(self.kind, Kind::Xxh3 | Kind::Xxh128)
    }

    /// The serialization magic number (PHP 8.5: 100 for SHA-3's
    /// lane-complemented state, 2 otherwise).
    pub fn magic(&self) -> i64 {
        if matches!(self.kind, Kind::Sha3(_)) {
            sha3::SHA3_MAGIC
        } else {
            2
        }
    }

    /// Whether the `secret` option applies (xxh3 and xxh128).
    pub fn uses_secret(&self) -> bool {
        matches!(self.kind, Kind::Xxh3 | Kind::Xxh128)
    }

    /// Whether the `seed` option applies.
    pub fn uses_seed(&self) -> bool {
        matches!(
            self.kind,
            Kind::Murmur3a
                | Kind::Murmur3c
                | Kind::Murmur3f
                | Kind::Xxh32
                | Kind::Xxh64
                | Kind::Xxh3
                | Kind::Xxh128
        )
    }
}

macro_rules! algo {
    ($name:expr, $ctx:expr, $len:expr, $block:expr, $crypto:expr, $kind:expr, $layout:expr) => {
        Algo {
            name: $name,
            context_name: $ctx,
            digest_len: $len,
            block_len: $block,
            crypto: $crypto,
            kind: $kind,
            layout: $layout,
        }
    };
}

use checksum::CrcKind;

const NONE: &[Field] = &[];

/// Every algorithm, in `hash_algos()` order (test
/// ext/hash/tests/hash_algos.phpt).
static ALGOS: [Algo; 60] = [
    algo!("md2", "md2", 16, 16, true, Kind::Md2, md::MD2_LAYOUT),
    algo!("md4", "md4", 16, 64, true, Kind::Md4, md::MD4_LAYOUT),
    algo!("md5", "md5", 16, 64, true, Kind::Md5, md::MD5_LAYOUT),
    algo!("sha1", "sha1", 20, 64, true, Kind::Sha1, sha::SHA1_LAYOUT),
    algo!(
        "sha224",
        "sha224",
        28,
        64,
        true,
        Kind::Sha224,
        sha::SHA256_LAYOUT
    ),
    algo!(
        "sha256",
        "sha256",
        32,
        64,
        true,
        Kind::Sha256,
        sha::SHA256_LAYOUT
    ),
    algo!(
        "sha384",
        "sha384",
        48,
        128,
        true,
        Kind::Sha384,
        sha::SHA512_LAYOUT
    ),
    algo!(
        "sha512/224",
        "sha512/224",
        28,
        128,
        true,
        Kind::Sha512_224,
        sha::SHA512_LAYOUT
    ),
    algo!(
        "sha512/256",
        "sha512/256",
        32,
        128,
        true,
        Kind::Sha512_256,
        sha::SHA512_LAYOUT
    ),
    algo!(
        "sha512",
        "sha512",
        64,
        128,
        true,
        Kind::Sha512,
        sha::SHA512_LAYOUT
    ),
    algo!(
        "sha3-224",
        "sha3-224",
        28,
        144,
        true,
        Kind::Sha3(28),
        sha3::SHA3_LAYOUT
    ),
    algo!(
        "sha3-256",
        "sha3-256",
        32,
        136,
        true,
        Kind::Sha3(32),
        sha3::SHA3_LAYOUT
    ),
    algo!(
        "sha3-384",
        "sha3-384",
        48,
        104,
        true,
        Kind::Sha3(48),
        sha3::SHA3_LAYOUT
    ),
    algo!(
        "sha3-512",
        "sha3-512",
        64,
        72,
        true,
        Kind::Sha3(64),
        sha3::SHA3_LAYOUT
    ),
    algo!(
        "ripemd128",
        "ripemd128",
        16,
        64,
        true,
        Kind::Ripemd(128),
        &ripemd::RMD128_LAYOUT
    ),
    algo!(
        "ripemd160",
        "ripemd160",
        20,
        64,
        true,
        Kind::Ripemd(160),
        &ripemd::RMD160_LAYOUT
    ),
    algo!(
        "ripemd256",
        "ripemd256",
        32,
        64,
        true,
        Kind::Ripemd(256),
        &ripemd::RMD256_LAYOUT
    ),
    algo!(
        "ripemd320",
        "ripemd320",
        40,
        64,
        true,
        Kind::Ripemd(320),
        &ripemd::RMD320_LAYOUT
    ),
    algo!(
        "whirlpool",
        "whirlpool",
        64,
        64,
        true,
        Kind::Whirlpool,
        whirlpool::WHIRLPOOL_LAYOUT
    ),
    algo!(
        "tiger128,3",
        "tiger128,3",
        16,
        64,
        true,
        Kind::Tiger { passes: 3, len: 16 },
        tiger::TIGER_LAYOUT
    ),
    algo!(
        "tiger160,3",
        "tiger160,3",
        20,
        64,
        true,
        Kind::Tiger { passes: 3, len: 20 },
        tiger::TIGER_LAYOUT
    ),
    algo!(
        "tiger192,3",
        "tiger192,3",
        24,
        64,
        true,
        Kind::Tiger { passes: 3, len: 24 },
        tiger::TIGER_LAYOUT
    ),
    algo!(
        "tiger128,4",
        "tiger128,4",
        16,
        64,
        true,
        Kind::Tiger { passes: 4, len: 16 },
        tiger::TIGER_LAYOUT
    ),
    algo!(
        "tiger160,4",
        "tiger160,4",
        20,
        64,
        true,
        Kind::Tiger { passes: 4, len: 20 },
        tiger::TIGER_LAYOUT
    ),
    algo!(
        "tiger192,4",
        "tiger192,4",
        24,
        64,
        true,
        Kind::Tiger { passes: 4, len: 24 },
        tiger::TIGER_LAYOUT
    ),
    algo!(
        "snefru",
        "snefru",
        32,
        32,
        true,
        Kind::Snefru,
        snefru::SNEFRU_LAYOUT
    ),
    algo!(
        "snefru256",
        "snefru",
        32,
        32,
        true,
        Kind::Snefru,
        snefru::SNEFRU_LAYOUT
    ),
    algo!(
        "gost",
        "gost",
        32,
        32,
        true,
        Kind::Gost { crypto: false },
        gost::GOST_LAYOUT
    ),
    algo!(
        "gost-crypto",
        "gost-crypto",
        32,
        32,
        true,
        Kind::Gost { crypto: true },
        gost::GOST_LAYOUT
    ),
    algo!(
        "adler32",
        "adler32",
        4,
        4,
        false,
        Kind::Adler32,
        checksum::U32_LAYOUT
    ),
    algo!(
        "crc32",
        "crc32",
        4,
        4,
        false,
        Kind::Crc32(CrcKind::Bzip),
        checksum::U32_LAYOUT
    ),
    algo!(
        "crc32b",
        "crc32b",
        4,
        4,
        false,
        Kind::Crc32(CrcKind::B),
        checksum::U32_LAYOUT
    ),
    algo!(
        "crc32c",
        "crc32c",
        4,
        4,
        false,
        Kind::Crc32(CrcKind::C),
        checksum::U32_LAYOUT
    ),
    algo!(
        "fnv132",
        "fnv132",
        4,
        4,
        false,
        Kind::Fnv132 { a: false },
        checksum::U32_LAYOUT
    ),
    algo!(
        "fnv1a32",
        "fnv1a32",
        4,
        4,
        false,
        Kind::Fnv132 { a: true },
        checksum::U32_LAYOUT
    ),
    algo!(
        "fnv164",
        "fnv164",
        8,
        4,
        false,
        Kind::Fnv164 { a: false },
        checksum::U64_LAYOUT
    ),
    algo!(
        "fnv1a64",
        "fnv1a64",
        8,
        4,
        false,
        Kind::Fnv164 { a: true },
        checksum::U64_LAYOUT
    ),
    algo!(
        "joaat",
        "joaat",
        4,
        4,
        false,
        Kind::Joaat,
        checksum::U32_LAYOUT
    ),
    algo!(
        "murmur3a",
        "murmur3a",
        4,
        4,
        false,
        Kind::Murmur3a,
        murmur::MURMUR3A_LAYOUT
    ),
    algo!(
        "murmur3c",
        "murmur3c",
        16,
        4,
        false,
        Kind::Murmur3c,
        murmur::MURMUR3C_LAYOUT
    ),
    algo!(
        "murmur3f",
        "murmur3f",
        16,
        8,
        false,
        Kind::Murmur3f,
        murmur::MURMUR3F_LAYOUT
    ),
    algo!(
        "xxh32",
        "xxh32",
        4,
        4,
        false,
        Kind::Xxh32,
        xxh::XXH32_LAYOUT
    ),
    algo!(
        "xxh64",
        "xxh64",
        8,
        8,
        false,
        Kind::Xxh64,
        xxh::XXH64_LAYOUT
    ),
    algo!("xxh3", "xxh3", 8, 8, false, Kind::Xxh3, NONE),
    algo!("xxh128", "xxh128", 16, 8, false, Kind::Xxh128, NONE),
    algo!(
        "haval128,3",
        "haval128,3",
        16,
        128,
        true,
        Kind::Haval {
            bits: 128,
            passes: 3
        },
        haval::HAVAL_LAYOUT
    ),
    algo!(
        "haval160,3",
        "haval160,3",
        20,
        128,
        true,
        Kind::Haval {
            bits: 160,
            passes: 3
        },
        haval::HAVAL_LAYOUT
    ),
    algo!(
        "haval192,3",
        "haval192,3",
        24,
        128,
        true,
        Kind::Haval {
            bits: 192,
            passes: 3
        },
        haval::HAVAL_LAYOUT
    ),
    algo!(
        "haval224,3",
        "haval224,3",
        28,
        128,
        true,
        Kind::Haval {
            bits: 224,
            passes: 3
        },
        haval::HAVAL_LAYOUT
    ),
    algo!(
        "haval256,3",
        "haval256,3",
        32,
        128,
        true,
        Kind::Haval {
            bits: 256,
            passes: 3
        },
        haval::HAVAL_LAYOUT
    ),
    algo!(
        "haval128,4",
        "haval128,4",
        16,
        128,
        true,
        Kind::Haval {
            bits: 128,
            passes: 4
        },
        haval::HAVAL_LAYOUT
    ),
    algo!(
        "haval160,4",
        "haval160,4",
        20,
        128,
        true,
        Kind::Haval {
            bits: 160,
            passes: 4
        },
        haval::HAVAL_LAYOUT
    ),
    algo!(
        "haval192,4",
        "haval192,4",
        24,
        128,
        true,
        Kind::Haval {
            bits: 192,
            passes: 4
        },
        haval::HAVAL_LAYOUT
    ),
    algo!(
        "haval224,4",
        "haval224,4",
        28,
        128,
        true,
        Kind::Haval {
            bits: 224,
            passes: 4
        },
        haval::HAVAL_LAYOUT
    ),
    algo!(
        "haval256,4",
        "haval256,4",
        32,
        128,
        true,
        Kind::Haval {
            bits: 256,
            passes: 4
        },
        haval::HAVAL_LAYOUT
    ),
    algo!(
        "haval128,5",
        "haval128,5",
        16,
        128,
        true,
        Kind::Haval {
            bits: 128,
            passes: 5
        },
        haval::HAVAL_LAYOUT
    ),
    algo!(
        "haval160,5",
        "haval160,5",
        20,
        128,
        true,
        Kind::Haval {
            bits: 160,
            passes: 5
        },
        haval::HAVAL_LAYOUT
    ),
    algo!(
        "haval192,5",
        "haval192,5",
        24,
        128,
        true,
        Kind::Haval {
            bits: 192,
            passes: 5
        },
        haval::HAVAL_LAYOUT
    ),
    algo!(
        "haval224,5",
        "haval224,5",
        28,
        128,
        true,
        Kind::Haval {
            bits: 224,
            passes: 5
        },
        haval::HAVAL_LAYOUT
    ),
    algo!(
        "haval256,5",
        "haval256,5",
        32,
        128,
        true,
        Kind::Haval {
            bits: 256,
            passes: 5
        },
        haval::HAVAL_LAYOUT
    ),
];

/// `hash_algos()`.
pub fn algos() -> impl Iterator<Item = &'static str> {
    ALGOS.iter().map(|a| a.name)
}

/// `hash_hmac_algos()`: the cryptographic algorithms.
pub fn hmac_algos() -> impl Iterator<Item = &'static str> {
    ALGOS.iter().filter(|a| a.crypto).map(|a| a.name)
}

/// Looks an algorithm up by name, ignoring ASCII case (observed on PHP
/// 8.5: `hash('MD5', ...)` works).
pub fn lookup(name: &[u8]) -> Option<&'static Algo> {
    ALGOS
        .iter()
        .find(|a| a.name.as_bytes().eq_ignore_ascii_case(name))
}

/// The value of the `seed` option.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Seed {
    /// No `seed` key.
    #[default]
    Absent,
    /// An int.
    Int(i64),
    /// Any other type (deprecated; treated as 0).
    NonInt,
}

/// The `options` array of `hash()`, `hash_init()` and friends.
///
/// A `secret` that is not a string must be converted by the wiring layer
/// (which may call `__toString()`); for algorithms where
/// [`Algo::uses_secret`] holds and no seed is given, PHP first emits
/// `Deprecated: func(): ` [`SECRET_DEPRECATION`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HashOptions {
    /// `seed`.
    pub seed: Seed,
    /// `secret`, as a string.
    pub secret: Option<Vec<u8>>,
}

/// The deprecation for a non-string `secret` (test
/// ext/hash/tests/xxhash_secret.phpt).
pub const SECRET_DEPRECATION: &str = "Passing a secret of a type other than string is deprecated because it implicitly converts to a string, potentially hiding bugs";

const SEED_ZERO_DEPRECATION: &str = "Passing a seed of a type other than int is deprecated because it is the same as setting the seed to 0";
const SEED_IGNORED_DEPRECATION: &str =
    "Passing a seed of a type other than int is deprecated because it is ignored";

/// Creates the engine of `algo` with `opts`, validating them (tests
/// xxhash_seed.phpt, xxhash_secret.phpt, murmur_seed_deprecation.phpt,
/// xxhash_seed_deprecation.phpt; seeds are truncated to 32 bits for
/// murmur3a, murmur3c and xxh32, observed on PHP 8.5).
fn new_engine(
    algo: &Algo,
    opts: &HashOptions,
    diags: &mut Vec<Diag>,
) -> Result<Box<dyn Engine>, HashError> {
    let seed_zero = |diags: &mut Vec<Diag>| match opts.seed {
        Seed::Int(v) => v,
        Seed::NonInt => {
            diags.push(Diag::Deprecated(SEED_ZERO_DEPRECATION.into()));
            0
        }
        Seed::Absent => 0,
    };
    Ok(match algo.kind {
        Kind::Md2 => Box::new(md::Md2::new()),
        Kind::Md4 => Box::new(md::Md4::new()),
        Kind::Md5 => Box::new(md::Md5::new()),
        Kind::Sha1 => Box::new(sha::Sha1::new()),
        Kind::Sha224 => Box::new(sha::Sha256::new_224()),
        Kind::Sha256 => Box::new(sha::Sha256::new_256()),
        Kind::Sha384 => Box::new(sha::Sha512::new_384()),
        Kind::Sha512_224 => Box::new(sha::Sha512::new_512_224()),
        Kind::Sha512_256 => Box::new(sha::Sha512::new_512_256()),
        Kind::Sha512 => Box::new(sha::Sha512::new_512()),
        Kind::Sha3(n) => Box::new(sha3::Sha3::new(n)),
        Kind::Ripemd(b) => Box::new(ripemd::Ripemd::new(b)),
        Kind::Whirlpool => Box::new(whirlpool::Whirlpool::new()),
        Kind::Tiger { passes, len } => Box::new(tiger::Tiger::new(passes, len)),
        Kind::Snefru => Box::new(snefru::Snefru::new()),
        Kind::Gost { crypto } => Box::new(gost::Gost::new(crypto)),
        Kind::Adler32 => Box::new(checksum::Adler32::new()),
        Kind::Crc32(k) => Box::new(checksum::Crc32::new(k)),
        Kind::Fnv132 { a } => Box::new(checksum::Fnv32::new(a)),
        Kind::Fnv164 { a } => Box::new(checksum::Fnv64::new(a)),
        Kind::Joaat => Box::new(checksum::Joaat::new()),
        Kind::Haval { bits, passes } => Box::new(haval::Haval::new(bits, passes)),
        Kind::Murmur3a => Box::new(murmur::Murmur3a::new(seed_zero(diags) as u32)),
        Kind::Murmur3c => Box::new(murmur::Murmur3c::new(seed_zero(diags) as u32)),
        Kind::Murmur3f => Box::new(murmur::Murmur3f::new(seed_zero(diags) as u64)),
        Kind::Xxh32 => Box::new(xxh::Xxh32::new(seed_zero(diags) as u32)),
        Kind::Xxh64 => Box::new(xxh::Xxh64::new(seed_zero(diags) as u64)),
        Kind::Xxh3 | Kind::Xxh128 => {
            let wide = algo.kind == Kind::Xxh128;
            let name = algo.name;
            if opts.seed != Seed::Absent && opts.secret.is_some() {
                return Err(HashError::Error(format!(
                    "{name}: Only one of seed or secret is to be passed for initialization"
                )));
            }
            if let Some(secret) = &opts.secret {
                if secret.len() < XXH3_SECRET_MIN {
                    return Err(HashError::Error(format!(
                        "{name}: Secret length must be >= {XXH3_SECRET_MIN} bytes, {} bytes passed",
                        secret.len()
                    )));
                }
                let mut s = &secret[..];
                if s.len() > XXH3_SECRET_MAX {
                    diags.push(Diag::Warning(format!(
                        "{name}: Secret content exceeding {XXH3_SECRET_MAX} bytes discarded"
                    )));
                    s = &s[..XXH3_SECRET_MAX];
                }
                Box::new(xxh::Xxh3::with_secret(wide, s))
            } else {
                let seed = match opts.seed {
                    Seed::Int(v) => v as u64,
                    Seed::NonInt => {
                        diags.push(Diag::Deprecated(SEED_IGNORED_DEPRECATION.into()));
                        0
                    }
                    Seed::Absent => 0,
                };
                Box::new(xxh::Xxh3::with_seed(wide, seed))
            }
        }
    })
}

fn finish_engine(algo: &Algo, mut e: Box<dyn Engine>) -> Vec<u8> {
    let mut out = vec![0u8; algo.digest_len];
    e.finish(&mut out);
    out
}

fn invalid_algo(func: &str) -> HashError {
    HashError::ValueError(format!(
        "{func}(): Argument #1 ($algo) must be a valid hashing algorithm"
    ))
}

fn invalid_crypto_algo(func: &str) -> HashError {
    HashError::ValueError(format!(
        "{func}(): Argument #1 ($algo) must be a valid cryptographic hashing algorithm"
    ))
}

/// Looks up an algorithm for `hash()`, `hash_file()` or `hash_init()`.
pub fn lookup_for(func: &str, name: &[u8]) -> Result<&'static Algo, HashError> {
    lookup(name).ok_or_else(|| invalid_algo(func))
}

/// Looks up a cryptographic algorithm for `hash_hmac()`,
/// `hash_hmac_file()`, `hash_hkdf()` or `hash_pbkdf2()`.
pub fn lookup_crypto(func: &str, name: &[u8]) -> Result<&'static Algo, HashError> {
    match lookup(name) {
        Some(a) if a.crypto => Ok(a),
        _ => Err(invalid_crypto_algo(func)),
    }
}

/// A plain streaming hasher (no HMAC), for `hash_file()` and friends.
pub struct Hasher {
    algo: &'static Algo,
    engine: Box<dyn Engine>,
}

impl Clone for Hasher {
    fn clone(&self) -> Self {
        Hasher {
            algo: self.algo,
            engine: self.engine.clone_box(),
        }
    }
}

impl Hasher {
    /// A hasher with options.
    pub fn new(
        algo: &'static Algo,
        opts: &HashOptions,
        diags: &mut Vec<Diag>,
    ) -> Result<Self, HashError> {
        Ok(Hasher {
            algo,
            engine: new_engine(algo, opts, diags)?,
        })
    }

    /// A hasher without options.
    pub fn plain(algo: &'static Algo) -> Self {
        Self::new(algo, &HashOptions::default(), &mut Vec::new()).expect("no options")
    }

    /// Absorbs data.
    pub fn update(&mut self, data: &[u8]) {
        self.engine.update(data);
    }

    /// The raw digest.
    pub fn finish(self) -> Vec<u8> {
        finish_engine(self.algo, self.engine)
    }

    /// The algorithm.
    pub fn algo(&self) -> &'static Algo {
        self.algo
    }
}

/// `hash()` on raw bytes: the raw digest (see [`to_hex`]).
pub fn hash(
    name: &[u8],
    data: &[u8],
    opts: &HashOptions,
    diags: &mut Vec<Diag>,
) -> Result<Vec<u8>, HashError> {
    let algo = lookup_for("hash", name)?;
    digest(algo, data, opts, diags)
}

/// The digest of `data` under an already looked-up algorithm.
pub fn digest(
    algo: &'static Algo,
    data: &[u8],
    opts: &HashOptions,
    diags: &mut Vec<Diag>,
) -> Result<Vec<u8>, HashError> {
    if opts.seed == Seed::Absent
        && opts.secret.is_none()
        && let Some(d) = fast_digest(algo, data)
    {
        return Ok(d);
    }
    let mut h = Hasher::new(algo, opts, diags)?;
    h.update(data);
    Ok(h.finish())
}

/// One-shot digests through purecrypto for the algorithms it provides.
/// MD5, SHA-3, MD2 and Whirlpool stay on the local engines, which
/// measured faster.
fn fast_digest(algo: &Algo, data: &[u8]) -> Option<Vec<u8>> {
    use purecrypto::hash as pc;
    Some(match algo.kind {
        Kind::Md4 => pc::md4(data).to_vec(),
        Kind::Ripemd(160) => pc::ripemd160(data).to_vec(),
        Kind::Sha1 => pc::sha1(data).to_vec(),
        Kind::Sha224 => pc::sha224(data).to_vec(),
        Kind::Sha256 => pc::sha256(data).to_vec(),
        Kind::Sha384 => pc::sha384(data).to_vec(),
        Kind::Sha512 => pc::sha512(data).to_vec(),
        Kind::Sha512_224 => pc::sha512_224(data).to_vec(),
        Kind::Sha512_256 => pc::sha512_256(data).to_vec(),
        _ => return None,
    })
}

/// Lowercase hex, as `hash()` returns without `$binary`.
pub fn to_hex(raw: &[u8]) -> Vec<u8> {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::with_capacity(raw.len() * 2);
    for &b in raw {
        out.push(H[(b >> 4) as usize]);
        out.push(H[(b & 15) as usize]);
    }
    out
}

// ---------------------------------------------------------------- HMAC

/// The HMAC key block: the key (hashed first when longer than a block)
/// zero-padded to the block length (RFC 2104).
fn hmac_key_block(algo: &'static Algo, key: &[u8]) -> Vec<u8> {
    let mut k = if key.len() > algo.block_len {
        let mut h = Hasher::plain(algo);
        h.update(key);
        h.finish()
    } else {
        key.to_vec()
    };
    k.resize(algo.block_len, 0);
    k
}

fn xor_pad(k: &[u8], pad: u8) -> Vec<u8> {
    k.iter().map(|b| b ^ pad).collect()
}

/// An HMAC in progress (RFC 2104).
#[derive(Clone)]
struct HmacState {
    inner: Hasher,
    /// The outer hash right after its padded key block.
    outer: Hasher,
}

impl HmacState {
    fn new(algo: &'static Algo, key: &[u8]) -> Self {
        let k = hmac_key_block(algo, key);
        let mut inner = Hasher::plain(algo);
        inner.update(&xor_pad(&k, 0x36));
        let mut outer = Hasher::plain(algo);
        outer.update(&xor_pad(&k, 0x5c));
        HmacState { inner, outer }
    }

    fn finish(self) -> Vec<u8> {
        let mut outer = self.outer;
        outer.update(&self.inner.finish());
        outer.finish()
    }
}

/// HMAC of `data` under an already looked-up (cryptographic) algorithm.
pub fn hmac(algo: &'static Algo, key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut s = HmacState::new(algo, key);
    s.inner.update(data);
    s.finish()
}

/// `hash_hmac()` on raw bytes: the raw MAC.
pub fn hash_hmac(name: &[u8], data: &[u8], key: &[u8]) -> Result<Vec<u8>, HashError> {
    let algo = lookup_crypto("hash_hmac", name)?;
    Ok(hmac(algo, key, data))
}

/// A streaming HMAC, for `hash_hmac_file()`.
#[derive(Clone)]
pub struct Hmac(HmacState);

impl Hmac {
    /// Starts a MAC.
    pub fn new(algo: &'static Algo, key: &[u8]) -> Self {
        Hmac(HmacState::new(algo, key))
    }
    /// Absorbs data.
    pub fn update(&mut self, data: &[u8]) {
        self.0.inner.update(data);
    }
    /// The raw MAC.
    pub fn finish(self) -> Vec<u8> {
        self.0.finish()
    }
}

// ---------------------------------------------------------------- HKDF / PBKDF2

/// `hash_hkdf()` (RFC 5869): the raw output key material. `length` 0
/// means the digest length. Errors as in test
/// ext/hash/tests/hash_hkdf_error.phpt.
pub fn hash_hkdf(
    name: &[u8],
    key: &[u8],
    length: i64,
    info: &[u8],
    salt: &[u8],
) -> Result<Vec<u8>, HashError> {
    let algo = lookup_crypto("hash_hkdf", name)?;
    if key.is_empty() {
        return Err(HashError::ValueError(
            "hash_hkdf(): Argument #2 ($key) must not be empty".into(),
        ));
    }
    if length < 0 {
        return Err(HashError::ValueError(
            "hash_hkdf(): Argument #3 ($length) must be greater than or equal to 0".into(),
        ));
    }
    let max = 255 * algo.digest_len as i64;
    if length > max {
        return Err(HashError::ValueError(format!(
            "hash_hkdf(): Argument #3 ($length) must be less than or equal to {max}"
        )));
    }
    let length = if length == 0 {
        algo.digest_len
    } else {
        length as usize
    };
    // An empty salt is HashLen zero bytes, which HMAC pads the same way.
    let prk = hmac(algo, salt, key);
    let base = HmacState::new(algo, &prk);
    let mut out = Vec::with_capacity(length + algo.digest_len);
    let mut t: Vec<u8> = Vec::new();
    let mut i = 1u8;
    while out.len() < length {
        let mut s = base.clone();
        s.inner.update(&t);
        s.inner.update(info);
        s.inner.update(&[i]);
        t = s.finish();
        out.extend_from_slice(&t);
        i = i.wrapping_add(1);
    }
    out.truncate(length);
    Ok(out)
}

/// `hash_pbkdf2()` (RFC 8018 PBKDF2 with HMAC): the hex string, or the
/// raw bytes with `binary`. `length` counts output characters (hex digits
/// unless `binary`); 0 means the whole digest. Errors as in test
/// ext/hash/tests/hash_pbkdf2_error.phpt.
pub fn hash_pbkdf2(
    name: &[u8],
    password: &[u8],
    salt: &[u8],
    iterations: i64,
    length: i64,
    binary: bool,
) -> Result<Vec<u8>, HashError> {
    let algo = lookup_crypto("hash_pbkdf2", name)?;
    if iterations <= 0 {
        return Err(HashError::ValueError(
            "hash_pbkdf2(): Argument #4 ($iterations) must be greater than 0".into(),
        ));
    }
    if length < 0 {
        return Err(HashError::ValueError(
            "hash_pbkdf2(): Argument #5 ($length) must be greater than or equal to 0".into(),
        ));
    }
    let raw_len = match (length, binary) {
        (0, _) => algo.digest_len,
        (n, true) => n as usize,
        (n, false) => (n as usize).div_ceil(2),
    };
    let base = HmacState::new(algo, password);
    let mut raw = Vec::with_capacity(raw_len + algo.digest_len);
    let mut block = 1u32;
    while raw.len() < raw_len {
        let mut s = base.clone();
        s.inner.update(salt);
        s.inner.update(&block.to_be_bytes());
        let mut u = s.finish();
        let mut acc = u.clone();
        for _ in 1..iterations {
            let mut s = base.clone();
            s.inner.update(&u);
            u = s.finish();
            for (a, b) in acc.iter_mut().zip(&u) {
                *a ^= b;
            }
        }
        raw.extend_from_slice(&acc);
        block = block.wrapping_add(1);
    }
    raw.truncate(raw_len);
    if binary {
        return Ok(raw);
    }
    let mut hex = to_hex(&raw);
    if length > 0 {
        hex.truncate(length as usize);
    }
    Ok(hex)
}

/// `hash_equals()`: a comparison whose time depends only on the lengths.
pub fn hash_equals(known: &[u8], user: &[u8]) -> bool {
    if known.len() != user.len() {
        return false;
    }
    let mut d = 0u8;
    for (a, b) in known.iter().zip(user) {
        d |= a ^ b;
    }
    std::hint::black_box(d) == 0
}

// ---------------------------------------------------------------- HashContext

/// `HASH_HMAC`.
pub const HASH_HMAC: i64 = 1;

/// The state behind a PHP `HashContext` object.
pub struct HashContext {
    algo: &'static Algo,
    flags: i64,
    /// `None` once finalized.
    engine: Option<Box<dyn Engine>>,
    /// The outer HMAC hash after its key block, for HMAC contexts.
    hmac_outer: Option<Box<dyn Engine>>,
}

/// A context's serialized form, the array `HashContext::__serialize()`
/// returns: `[algo, flags, state, magic, members]` (the members array is
/// the object's properties, added by the wiring layer).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerializedContext {
    /// Element 0.
    pub algo: &'static str,
    /// Element 1.
    pub flags: i64,
    /// Element 2.
    pub state: Vec<SerVal>,
    /// Element 3.
    pub magic: i64,
}

/// Element 2 of a serialized context as handed to `__unserialize()`.
pub enum StateSlot<'a> {
    /// The key is missing.
    Missing,
    /// Present but not an array.
    NotArray,
    /// An array.
    Array(&'a dyn StateArray),
}

const ILL_FORMED: &str = "Incomplete or ill-formed serialization data";
const HMAC_NO_SERIALIZE: &str = "HashContext with HASH_HMAC option cannot be serialized";

/// The message of `__unserialize()` on an object already initialized
/// (test ext/hash/tests/hash_serialize_004.phpt), an `Exception`.
pub const UNSERIALIZE_INITIALIZED: &str = "HashContext::__unserialize called on initialized object";

/// The `TypeError` for a finalized context passed to `func`.
pub fn finalized_error(func: &str) -> HashError {
    HashError::TypeError(format!(
        "{func}(): Argument #1 ($context) must be a valid, non-finalized HashContext"
    ))
}

impl Clone for HashContext {
    fn clone(&self) -> Self {
        HashContext {
            algo: self.algo,
            flags: self.flags,
            engine: self.engine.as_ref().map(|e| e.clone_box()),
            hmac_outer: self.hmac_outer.as_ref().map(|e| e.clone_box()),
        }
    }
}

impl HashContext {
    /// `hash_init()`. `key` is `None` when omitted; errors as in test
    /// ext/hash/tests/hash_init_error.phpt.
    pub fn new(
        name: &[u8],
        flags: i64,
        key: Option<&[u8]>,
        opts: &HashOptions,
        diags: &mut Vec<Diag>,
    ) -> Result<Self, HashError> {
        let algo = lookup_for("hash_init", name)?;
        let hmac = flags & HASH_HMAC != 0;
        if hmac && !algo.crypto {
            return Err(HashError::ValueError(
                "hash_init(): Argument #1 ($algo) must be a cryptographic hashing algorithm if HMAC is requested"
                    .into(),
            ));
        }
        let key = key.unwrap_or(b"");
        if hmac && key.is_empty() {
            return Err(HashError::ValueError(
                "hash_init(): Argument #3 ($key) must not be empty when HMAC is requested".into(),
            ));
        }
        let mut engine = new_engine(algo, opts, diags)?;
        let mut hmac_outer = None;
        if hmac {
            let k = hmac_key_block(algo, key);
            engine.update(&xor_pad(&k, 0x36));
            let mut outer = new_engine(algo, &HashOptions::default(), &mut Vec::new())?;
            outer.update(&xor_pad(&k, 0x5c));
            hmac_outer = Some(outer);
        }
        Ok(HashContext {
            algo,
            flags,
            engine: Some(engine),
            hmac_outer,
        })
    }

    /// The algorithm.
    pub fn algo(&self) -> &'static Algo {
        self.algo
    }

    /// The `algo` entry of `__debugInfo()`.
    pub fn algo_name(&self) -> &'static str {
        self.algo.context_name
    }

    /// Whether `hash_final()` was called.
    pub fn is_finalized(&self) -> bool {
        self.engine.is_none()
    }

    /// `hash_update()` and friends. Returns `false` on a finalized context
    /// (the wiring layer throws [`finalized_error`]).
    pub fn update(&mut self, data: &[u8]) -> bool {
        match &mut self.engine {
            Some(e) => {
                e.update(data);
                true
            }
            None => false,
        }
    }

    /// `hash_final()`: the raw digest, or `None` when already finalized.
    pub fn finalize(&mut self) -> Option<Vec<u8>> {
        let e = self.engine.take()?;
        let inner = finish_engine(self.algo, e);
        Some(match self.hmac_outer.take() {
            Some(mut outer) => {
                outer.update(&inner);
                finish_engine(self.algo, outer)
            }
            None => inner,
        })
    }

    /// `hash_copy()`: `None` when finalized.
    pub fn copy(&self) -> Option<HashContext> {
        if self.is_finalized() {
            None
        } else {
            Some(self.clone())
        }
    }

    /// `HashContext::__serialize()`; errors are `Exception`s (tests
    /// ext/hash/tests/hash_serialize_002.phpt, observed on PHP 8.5).
    pub fn serialize(&self) -> Result<SerializedContext, HashError> {
        if self.flags & HASH_HMAC != 0 {
            return Err(HashError::Exception(HMAC_NO_SERIALIZE.into()));
        }
        let engine = match &self.engine {
            Some(e) if self.algo.serializable() => e,
            _ => {
                return Err(HashError::Exception(format!(
                    "HashContext for algorithm \"{}\" cannot be serialized",
                    self.algo.context_name
                )));
            }
        };
        Ok(SerializedContext {
            algo: self.algo.context_name,
            flags: self.flags,
            state: engine::encode(self.algo.layout, &engine.export()),
            magic: self.algo.magic(),
        })
    }

    /// `HashContext::__unserialize()` on a fresh object. `algo`, `flags`
    /// and `magic` are `None` when missing or of the wrong type;
    /// `members_ok` tells whether element 4 is an array. Errors are
    /// `Exception`s (test ext/hash/tests/hash_serialize_004.phpt; field
    /// codes observed on PHP 8.5).
    pub fn unserialize(
        algo: Option<&[u8]>,
        flags: Option<i64>,
        state: StateSlot<'_>,
        magic: Option<i64>,
        members_ok: bool,
    ) -> Result<HashContext, HashError> {
        let ill = || HashError::Exception(ILL_FORMED.into());
        let (Some(name), Some(flags), Some(magic)) = (algo, flags, magic) else {
            return Err(ill());
        };
        if !members_ok || matches!(state, StateSlot::Missing) {
            return Err(ill());
        }
        if flags & HASH_HMAC != 0 {
            return Err(HashError::Exception(HMAC_NO_SERIALIZE.into()));
        }
        let Some(algo) = lookup(name) else {
            return Err(HashError::Exception("Unknown hash algorithm".into()));
        };
        let code = |c: i32| {
            HashError::Exception(format!("{ILL_FORMED} (\"{}\" code {c})", algo.context_name))
        };
        let StateSlot::Array(arr) = state else {
            return Err(code(-1));
        };
        if magic != algo.magic() || !algo.serializable() {
            return Err(code(-1));
        }
        let vals = engine::decode(algo.layout, arr).map_err(code)?;
        let mut e = new_engine(algo, &HashOptions::default(), &mut Vec::new())?;
        if !e.import(&vals) {
            return Err(code(-2000));
        }
        Ok(HashContext {
            algo,
            flags,
            engine: Some(e),
            hmac_outer: None,
        })
    }
}
