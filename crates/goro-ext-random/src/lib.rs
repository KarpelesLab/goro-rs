//! PHP random extension (PHP manual: Random; RFCs "Random Extension 5.x",
//! "Randomizer Additions" and "Randomizer::getFloat()").
//!
//! Everything here works on plain Rust values; the engine (VM) wires it to
//! PHP classes and functions:
//!
//! - Engines: [`Mt19937`], [`PcgOneseq128XslRr64`], [`Xoshiro256StarStar`]
//!   and [`Secure`], with their `generate()` output, seeding, jumps, and the
//!   `__serialize()`/`__unserialize()`/`__debugInfo()` state formats
//!   ([`state`]). [`NativeEngine`] holds any of them.
//! - User engines (`Random\Engine` implemented in PHP) implement [`Engine`]
//!   in the VM, building each result with [`Generated::from_bytes`].
//! - `Random\Randomizer`'s methods are the functions of [`randomizer`],
//!   generic over [`Engine`].
//! - The global functions (`mt_rand`, `rand`, `shuffle`, `array_rand`,
//!   `random_int`, `lcg_value`, ...) are in [`legacy`].
//!
//! Behaviour was established from the manual, the RFCs, the published
//! algorithms (MT19937 by Matsumoto and Nishimura; PCG by O'Neill;
//! xoshiro256** and SplitMix64 by Blackman and Vigna; the γ-section
//! algorithm by Goualard, "Drawing random floating-point numbers from an
//! interval", ACM TOMACS 2022), the ext/random tests, and observations of
//! PHP 8.5 recorded in `tests/fixtures/`.

pub mod engine;
pub mod float;
pub mod legacy;
pub mod mt19937;
pub mod pcg;
pub mod randomizer;
pub mod secure;
pub mod state;
pub mod xoshiro;

pub use engine::{Engine, Generated, NativeEngine};
pub use float::IntervalBoundary;
pub use mt19937::{MT_RAND_MAX, Mt19937, MtMode};
pub use pcg::PcgOneseq128XslRr64;
pub use secure::Secure;
pub use xoshiro::Xoshiro256StarStar;

/// An error for the wiring layer to throw.
///
/// Messages are the full `getMessage()` text, including the `func(): `
/// prefix when PHP prints one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RandomError {
    /// `ValueError`.
    ValueError(String),
    /// `Random\BrokenRandomEngineError`.
    BrokenEngine(&'static str),
    /// `Random\RandomException`.
    RandomException(String),
    /// A plain `Exception` (invalid serialization data, serialization not
    /// allowed).
    Exception(String),
    /// A user engine's `generate()` threw or exited: the VM already holds the
    /// pending exception (or exit) and resumes unwinding with it.
    Propagate,
}

impl RandomError {
    pub(crate) fn value(msg: impl Into<String>) -> RandomError {
        RandomError::ValueError(msg.into())
    }
}

impl std::fmt::Display for RandomError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RandomError::ValueError(m) => write!(f, "ValueError: {m}"),
            RandomError::BrokenEngine(m) => write!(f, "Random\\BrokenRandomEngineError: {m}"),
            RandomError::RandomException(m) => write!(f, "Random\\RandomException: {m}"),
            RandomError::Exception(m) => write!(f, "Exception: {m}"),
            RandomError::Propagate => write!(f, "(pending exception)"),
        }
    }
}

impl std::error::Error for RandomError {}

/// `Random\BrokenRandomEngineError` when range rejection fails 50 times in a
/// row (test 03_randomizer/engine_unsafe_biased.phpt).
pub const MSG_RETRY_LIMIT: &str = "Failed to generate an acceptable random number in 50 attempts";
/// `Random\BrokenRandomEngineError` when an engine returns `""` (test
/// 03_randomizer/engine_unsafe_empty_string.phpt).
pub const MSG_EMPTY_STRING: &str = "A random engine must return a non-empty string";
/// The `E_DEPRECATED` message of `mt_srand($seed, MT_RAND_PHP)` and
/// `new Mt19937($seed, MT_RAND_PHP)` (test 01_functions/mt_rand_value.phpt).
pub const MSG_MT_RAND_PHP_DEPRECATED: &str = "The MT_RAND_PHP variant of Mt19937 is deprecated";
/// The `E_DEPRECATED` message of `lcg_value()` (test
/// 01_functions/lcg_value_deprecation.phpt).
pub const MSG_LCG_VALUE_DEPRECATED: &str =
    "Function lcg_value() is deprecated since 8.4, use \\Random\\Randomizer::getFloat() instead";

/// The class names, as PHP spells them in messages.
pub mod class {
    pub const MT19937: &str = "Random\\Engine\\Mt19937";
    pub const PCG: &str = "Random\\Engine\\PcgOneseq128XslRr64";
    pub const XOSHIRO: &str = "Random\\Engine\\Xoshiro256StarStar";
    pub const SECURE: &str = "Random\\Engine\\Secure";
    pub const RANDOMIZER: &str = "Random\\Randomizer";
}

/// The `Exception` thrown by `__unserialize()` for malformed data: "Invalid
/// serialization data for Random\Engine\Mt19937 object" (test
/// 02_engine/all_serialize_error.phpt).
pub fn invalid_serialization(class: &str) -> RandomError {
    RandomError::Exception(format!("Invalid serialization data for {class} object"))
}

/// The `Exception` thrown when serializing a `Random\Engine\Secure` (also
/// through a `Randomizer` holding one): "Serialization of
/// 'Random\Engine\Secure' is not allowed" (test
/// 03_randomizer/serialize_disallowed.phpt).
pub fn serialization_not_allowed(class: &str) -> RandomError {
    RandomError::Exception(format!("Serialization of '{class}' is not allowed"))
}
