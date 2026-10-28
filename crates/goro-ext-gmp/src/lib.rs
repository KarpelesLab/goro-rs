//! PHP's gmp extension (PHP manual: GMP, "GNU Multiple Precision") on
//! plain values: every `gmp_*()` function and the behaviour of the `GMP`
//! class (operators, comparison, casts, serialization, debug output, the
//! constructor), with puremp as the integer backend.
//!
//! Behaviour follows the PHP manual's GMP chapter, the GMP manual's
//! description of the underlying operations, the expected output of
//! php-src's `ext/gmp/tests/*.phpt`, and observations of PHP 8.5 with GMP
//! 6.3 (the fixture in `tests/fixtures` was generated once from the
//! reference binary by `tests/fixtures/gmp_driver.php`).
//!
//! # Engine wiring
//!
//! * A `GMP` object's native data is a [`Gmp`] (it implements
//!   [`goro_runtime::NativeData`]). `GMP` is a `final` class with no
//!   declared properties and the methods `__construct(string|int $num = 0,
//!   int $base = 0)` ([`functions::gmp_construct`]), `__serialize(): array`
//!   and `__unserialize(array $data): void` (see [`object`]).
//! * Parameters typed `GMP|string|int` are passed as a [`Num`]: the engine
//!   maps the PHP value without converting it (a `GMP` object to
//!   [`Num::Gmp`], any other object to [`Num::Other`] with its class name,
//!   arrays and resources to [`Num::Other`] with `"array"`/`"resource"`).
//!   The library applies PHP's rules for them, including `strict_types`
//!   ([`Env::strict`]), the null and float-to-int deprecations (reported
//!   through [`goro_runtime::Ctx::notice`] in PHP's order) and the exact
//!   TypeError/ValueError messages. `gmp_init()`'s and the constructor's
//!   `string|int $num` is a [`Num`] too (a GMP object there is read as its
//!   decimal string in coercive mode, as PHP does through `__toString`).
//! * Parameters typed `int` or `bool` (bases, rounding modes, bit
//!   indexes, `$word_size`, `$repetitions`, ...) are coerced by the
//!   engine's usual parameter parsing and passed as `i64`/`bool`;
//!   [`SIGNATURES`] lists every function with its parameter names, types
//!   and defaults. `gmp_setbit()`/`gmp_clrbit()` take a `GMP $num` object
//!   (mutated in place); [`functions::gmp_object_type_error`] builds the
//!   TypeError for anything else.
//! * Functions return a [`GmpError`] to throw. [`GmpErrorKind::OutOfMemory`]
//!   is not a Throwable: GMP aborts the process when a result is too large
//!   to allocate ("GNU MP: Cannot allocate memory"); the engine reports it
//!   as a fatal error. [`set_max_bits`] bounds result sizes.
//! * Operators: the engine calls [`object::binary_op`] when either operand
//!   of `+ - * / % ** & | ^ << >>` is a GMP object (it returns `None` for
//!   operators GMP does not overload, such as `.`), [`object::compare`]
//!   for comparisons, and [`object::negate`]/[`object::bitwise_not`]/
//!   [`object::increment`]/[`object::decrement`] for the unary forms (`+$a`
//!   is `$a * 1`). Casts are [`object::to_int`], [`object::to_float`],
//!   [`object::to_bool`] and [`object::to_string`]; `var_dump()`/`print_r()`
//!   show the object's dynamic properties followed by `["num"] => string`
//!   (the decimal value); `var_export()`, `(array)`, `get_object_vars()` and
//!   `json_encode()` see only the dynamic properties.
//! * Two parts of PHP's operator behaviour belong to the engine (observed
//!   on PHP 8.5; `tests/php85_fixture.rs` plays them): when the left operand
//!   of `% & | ^ << >>` is not an object, PHP first reads it as an int as
//!   for `int % int` (float deprecations and "not representable" warnings,
//!   "A non-numeric value encountered" for leading-numeric strings,
//!   `TypeError: Unsupported operand types: string % GMP` for non-numeric
//!   strings, arrays, other objects and resources) and only then calls the
//!   GMP handler with the original value. And in a comparison of two
//!   objects whose left one is not a GMP object, PHP uses the left object's
//!   comparison (`stdClass <=> GMP` is 1, uncomparable, without an error).
//!   `$a > $b` is evaluated as `$b < $a`.

mod arith;
pub mod functions;
pub mod object;
mod parse;
pub mod random;
mod sigs;
mod small;

use std::cell::Cell;
use std::fmt;

use goro_runtime::Ctx;

pub use functions::*;
pub use parse::{parse_integer, to_string_radix};
pub use puremp::Int;
pub use sigs::{FnSig, Param, SIGNATURES};

/// `GMP_ROUND_ZERO`
pub const GMP_ROUND_ZERO: i64 = 0;
/// `GMP_ROUND_PLUSINF`
pub const GMP_ROUND_PLUSINF: i64 = 1;
/// `GMP_ROUND_MINUSINF`
pub const GMP_ROUND_MINUSINF: i64 = 2;
/// `GMP_MSW_FIRST`
pub const GMP_MSW_FIRST: i64 = 1;
/// `GMP_LSW_FIRST`
pub const GMP_LSW_FIRST: i64 = 2;
/// `GMP_LITTLE_ENDIAN`
pub const GMP_LITTLE_ENDIAN: i64 = 4;
/// `GMP_BIG_ENDIAN`
pub const GMP_BIG_ENDIAN: i64 = 8;
/// `GMP_NATIVE_ENDIAN`
pub const GMP_NATIVE_ENDIAN: i64 = 16;
/// `GMP_VERSION`: the GMP version PHP 8.5 was observed with. GMP's
/// behaviour (prime test results, the `gmp_random_*` streams) follows it.
pub const GMP_VERSION: &str = "6.3.0";

/// The class name.
pub const CLASS_NAME: &str = "GMP";

/// The `GMP` constants with their values, for registration.
pub const CONSTANTS: &[(&str, i64)] = &[
    ("GMP_ROUND_ZERO", GMP_ROUND_ZERO),
    ("GMP_ROUND_PLUSINF", GMP_ROUND_PLUSINF),
    ("GMP_ROUND_MINUSINF", GMP_ROUND_MINUSINF),
    ("GMP_MSW_FIRST", GMP_MSW_FIRST),
    ("GMP_LSW_FIRST", GMP_LSW_FIRST),
    ("GMP_LITTLE_ENDIAN", GMP_LITTLE_ENDIAN),
    ("GMP_BIG_ENDIAN", GMP_BIG_ENDIAN),
    ("GMP_NATIVE_ENDIAN", GMP_NATIVE_ENDIAN),
];

/// An arbitrary precision integer: the native data of a `GMP` object.
#[derive(Clone, Default, PartialEq, Eq, Hash)]
pub struct Gmp(pub Int);

impl Gmp {
    /// The value.
    #[inline]
    pub fn value(&self) -> &Int {
        &self.0
    }

    /// Builds a value from an int.
    #[inline]
    pub fn from_i64(v: i64) -> Gmp {
        Gmp(Int::from_i64(v))
    }

    /// The value in `base` (2..=62, or -2..=-36 for upper-case digits), as
    /// `gmp_strval()` writes it. `base` must be valid.
    pub fn to_string_radix(&self, base: i32) -> String {
        to_string_radix(&self.0, base)
    }
}

impl From<Int> for Gmp {
    #[inline]
    fn from(v: Int) -> Gmp {
        Gmp(v)
    }
}

impl fmt::Display for Gmp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&to_string_radix(&self.0, 10))
    }
}

impl fmt::Debug for Gmp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Gmp({self})")
    }
}

impl goro_runtime::NativeData for Gmp {}

/// A `GMP|string|int` argument or operand as the engine received it.
#[derive(Clone, Copy, Debug)]
pub enum Num<'a> {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(&'a [u8]),
    Gmp(&'a Gmp),
    /// Any other value: the type name PHP uses in messages (`"array"`,
    /// `"resource"`, or the class name of an object other than GMP).
    Other(&'a str),
}

impl Num<'_> {
    /// The type name in "must be of type ..., X given" messages.
    pub(crate) fn given_name(&self) -> &str {
        match self {
            Num::Null => "null",
            Num::Bool(true) => "true",
            Num::Bool(false) => "false",
            Num::Int(_) => "int",
            Num::Float(_) => "float",
            Num::Str(_) => "string",
            Num::Gmp(_) => "GMP",
            Num::Other(t) => t,
        }
    }

    /// The type name in "Unsupported operand types" messages.
    pub(crate) fn operand_name(&self) -> &str {
        match self {
            Num::Bool(_) => "bool",
            n => n.given_name(),
        }
    }
}

/// The calling context of a function: diagnostics sink and whether the
/// calling file uses `declare(strict_types=1)`.
pub struct Env<'c> {
    pub ctx: &'c mut dyn Ctx,
    pub strict: bool,
}

impl<'c> Env<'c> {
    pub fn new(ctx: &'c mut dyn Ctx, strict: bool) -> Env<'c> {
        Env { ctx, strict }
    }
}

/// The class of a Throwable (or fatal error) raised by gmp.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GmpErrorKind {
    /// `TypeError`
    TypeError,
    /// `ValueError`
    ValueError,
    /// `DivisionByZeroError`
    DivisionByZero,
    /// `Exception` (unserialization of bad data)
    Exception,
    /// Not a Throwable: GMP could not allocate the result and PHP aborts.
    OutOfMemory,
}

/// An error gmp raises.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GmpError {
    pub kind: GmpErrorKind,
    pub message: String,
}

/// A gmp result.
pub type GmpResult<T> = Result<T, GmpError>;

impl GmpError {
    #[cold]
    pub(crate) fn new(kind: GmpErrorKind, message: impl Into<String>) -> GmpError {
        GmpError {
            kind,
            message: message.into(),
        }
    }
    #[cold]
    pub(crate) fn type_error(message: impl Into<String>) -> GmpError {
        GmpError::new(GmpErrorKind::TypeError, message)
    }
    #[cold]
    pub(crate) fn value(message: impl Into<String>) -> GmpError {
        GmpError::new(GmpErrorKind::ValueError, message)
    }
    #[cold]
    pub(crate) fn div_zero(message: impl Into<String>) -> GmpError {
        GmpError::new(GmpErrorKind::DivisionByZero, message)
    }
    #[cold]
    pub(crate) fn exception(message: impl Into<String>) -> GmpError {
        GmpError::new(GmpErrorKind::Exception, message)
    }
    #[cold]
    pub(crate) fn out_of_memory(bits: u128) -> GmpError {
        GmpError::new(
            GmpErrorKind::OutOfMemory,
            format!("GNU MP: Cannot allocate memory (size={})", bits.div_ceil(8)),
        )
    }

    /// The PHP class name (`"Error"` for the out-of-memory case, which the
    /// engine reports as a fatal error rather than throwing).
    pub fn class_name(&self) -> &'static str {
        match self.kind {
            GmpErrorKind::TypeError => "TypeError",
            GmpErrorKind::ValueError => "ValueError",
            GmpErrorKind::DivisionByZero => "DivisionByZeroError",
            GmpErrorKind::Exception => "Exception",
            GmpErrorKind::OutOfMemory => "Error",
        }
    }
}

impl fmt::Display for GmpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.class_name(), self.message)
    }
}

impl std::error::Error for GmpError {}

thread_local! {
    static MAX_BITS: Cell<u64> = const { Cell::new(1 << 35) };
}

/// The largest result, in bits, an operation may build (default 2^35 bits,
/// 4 GiB); larger ones fail with [`GmpErrorKind::OutOfMemory`].
pub fn max_bits() -> u64 {
    MAX_BITS.with(Cell::get)
}

/// Sets [`max_bits`] for this thread.
pub fn set_max_bits(bits: u64) {
    MAX_BITS.with(|c| c.set(bits));
}

/// Fails when a result of `bits` bits would exceed [`max_bits`].
#[inline]
pub(crate) fn check_bits(bits: u128) -> GmpResult<()> {
    if bits > max_bits() as u128 {
        Err(GmpError::out_of_memory(bits))
    } else {
        Ok(())
    }
}
